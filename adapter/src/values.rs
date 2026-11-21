use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    process,
};

pub(crate) const SAVE_ERROR: &str = "AstroPort could not save machine values.";

#[derive(Deserialize)]
struct SavedValues {
    machine: BTreeMap<String, Value>,
}

pub struct Store {
    path: PathBuf,
    machine: BTreeMap<String, Value>,
}

fn valid_dump_folder(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|folder| folder.is_empty() || Path::new(folder).is_absolute())
}

impl Store {
    pub fn load(data: &Path) -> Store {
        let path = data.join("values.json");
        let machine = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<SavedValues>(&bytes).ok())
            .map(|saved| saved.machine)
            .filter(|machine| {
                machine
                    .iter()
                    .all(|(id, value)| id == "dump_folder" && valid_dump_folder(value))
            })
            .unwrap_or_default();
        Store { path, machine }
    }

    pub fn machine(&self) -> &BTreeMap<String, Value> {
        &self.machine
    }

    pub fn set_machine(&mut self, values: BTreeMap<String, Value>) -> Result<(), String> {
        let mut machine = self.machine.clone();
        for (id, value) in values {
            if id != "dump_folder" || (!value.is_null() && !valid_dump_folder(&value)) {
                return Err(format!("Invalid value for {id}."));
            }
            if value.is_null() {
                machine.remove(&id);
            } else {
                machine.insert(id, value);
            }
        }

        let bytes =
            serde_json::to_vec(&json!({"machine": machine})).map_err(|_| SAVE_ERROR.to_owned())?;
        let temp = self
            .path
            .with_extension(format!("json.{}.tmp", process::id()));
        let mut file = File::create_new(&temp).map_err(|_| SAVE_ERROR.to_owned())?;
        let written = file.write_all(&bytes).and_then(|()| file.sync_all());
        drop(file);
        if written
            .and_then(|()| fs::rename(&temp, &self.path))
            .is_err()
        {
            let _ = fs::remove_file(&temp);
            return Err(SAVE_ERROR.to_owned());
        }
        self.machine = machine;
        Ok(())
    }

    pub fn dump_folder(&self) -> Option<PathBuf> {
        self.machine
            .get("dump_folder")
            .and_then(Value::as_str)
            .filter(|folder| !folder.is_empty())
            .map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_bad_file_loads_empty() {
        let data = tempfile::tempdir().unwrap();
        let path = data.path().join("values.json");
        assert!(Store::load(data.path()).machine().is_empty());
        for bad in ["{not json", "{}", r#"{"machine":{"dump_folder":42}}"#] {
            fs::write(&path, bad).unwrap();
            assert!(Store::load(data.path()).machine().is_empty());
        }
    }

    #[test]
    fn dump_folder_updates_persist_and_null_removes() {
        let data = tempfile::tempdir().unwrap();
        let mut store = Store::load(data.path());
        for value in [json!(data.path()), json!(""), Value::Null] {
            store
                .set_machine(BTreeMap::from([("dump_folder".to_owned(), value.clone())]))
                .unwrap();
            let expected_folder = (value == json!(data.path())).then(|| data.path().to_owned());
            assert_eq!(store.dump_folder(), expected_folder);
            let expected = if value.is_null() {
                json!({"machine": {}})
            } else {
                json!({"machine": {"dump_folder": value}})
            };
            let saved: Value =
                serde_json::from_slice(&fs::read(data.path().join("values.json")).unwrap())
                    .unwrap();
            assert_eq!(saved, expected);
            assert_eq!(Store::load(data.path()).machine(), store.machine());
        }
    }

    #[test]
    fn invalid_updates_leave_machine_unchanged() {
        let data = tempfile::tempdir().unwrap();
        let mut store = Store::load(data.path());
        for value in [
            json!("relative"),
            json!(42),
            json!(true),
            json!([]),
            json!({}),
        ] {
            assert_eq!(
                store.set_machine(BTreeMap::from([("dump_folder".to_owned(), value)])),
                Err("Invalid value for dump_folder.".to_owned())
            );
        }
        assert_eq!(
            store.set_machine(BTreeMap::from([
                ("dump_folder".to_owned(), json!(data.path())),
                ("unknown".to_owned(), Value::Null),
            ])),
            Err("Invalid value for unknown.".to_owned())
        );
        assert!(store.machine().is_empty());
        assert!(!data.path().join("values.json").exists());
    }
}
