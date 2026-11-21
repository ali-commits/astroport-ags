use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, BufReader},
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Deserialize, Clone)]
pub struct VersionEntry {
    pub version: String,
    pub content_version: String,
    pub eboot_sha256: Option<String>,
    pub patches: String,
}

#[derive(Deserialize)]
pub struct Versions {
    pub title_id: String,
    pub versions: Vec<VersionEntry>,
}

pub fn load_versions(path: &Path) -> Result<Versions, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("Cannot parse {}: {error}", path.display()))
}

pub enum Detected {
    NotSet,
    NotFound,
    Unsupported {
        dump: PathBuf,
        version: String,
        message: String,
    },
    Supported {
        dump: PathBuf,
        entry: VersionEntry,
        eboot_sha256: String,
    },
}

fn dump_params(folder: &Path, title_id: &str) -> Option<Value> {
    let param_path = folder.join("sce_sys/param.json");
    if !folder.join("eboot.bin").is_file() || !param_path.is_file() {
        return None;
    }
    let params: Value = serde_json::from_slice(&fs::read(param_path).ok()?).ok()?;
    (params["titleId"].as_str() == Some(title_id)).then_some(params)
}

pub fn find_dump(folder: &Path, title_id: &str) -> Option<(PathBuf, Value)> {
    if let Some(params) = dump_params(folder, title_id) {
        return Some((folder.to_owned(), params));
    }

    let mut children: Vec<_> = fs::read_dir(folder).ok()?.filter_map(Result::ok).collect();
    children.sort_by_key(|child| child.file_name());
    children.into_iter().find_map(|child| {
        let path = child.path();
        let params = dump_params(&path, title_id)?;
        Some((path, params))
    })
}

pub fn display_version(content_version: &str) -> String {
    match content_version.as_bytes() {
        [a, b, b'.', c, d, e, b'.', f, g, h]
            if [a, b, c, d, e, f, g, h].into_iter().all(u8::is_ascii_digit) =>
        {
            let display = &content_version[..6];
            display.strip_prefix('0').unwrap_or(display).to_owned()
        }
        _ => content_version.to_owned(),
    }
}

fn supported_versions(v: &Versions) -> String {
    v.versions
        .iter()
        .map(|entry| entry.version.as_str())
        .collect::<Vec<_>>()
        .join(" and ")
}

pub fn unsupported_message(version: &str, v: &Versions) -> String {
    format!(
        "Version {version} is not supported. AstroPort runs {}.",
        supported_versions(v)
    )
}

struct CachedHash {
    len: u64,
    modified: SystemTime,
    digest: String,
}

#[derive(Default)]
pub struct HashCache {
    entries: HashMap<PathBuf, CachedHash>,
}

impl HashCache {
    pub fn sha256(&mut self, path: &Path) -> io::Result<String> {
        let metadata = fs::metadata(path)?;
        let len = metadata.len();
        let modified = metadata.modified()?;
        if let Some(entry) = self.entries.get(path) {
            if entry.len == len && entry.modified == modified {
                return Ok(entry.digest.clone());
            }
        }

        let mut reader = BufReader::with_capacity(1024 * 1024, File::open(path)?);
        let mut hasher = Sha256::new();
        io::copy(&mut reader, &mut hasher)?;

        let digest = format!("{:x}", hasher.finalize());
        self.entries.insert(
            path.to_owned(),
            CachedHash {
                len,
                modified,
                digest: digest.clone(),
            },
        );
        Ok(digest)
    }
}

pub fn detect(folder: Option<&Path>, v: &Versions, cache: &mut HashCache) -> Detected {
    let Some(folder) = folder.filter(|path| !path.as_os_str().is_empty()) else {
        return Detected::NotSet;
    };
    let Some((dump, params)) = find_dump(folder, &v.title_id) else {
        return Detected::NotFound;
    };
    let Some(content_version) = params["contentVersion"].as_str() else {
        return Detected::NotFound;
    };
    let Ok(eboot_sha256) = cache.sha256(&dump.join("eboot.bin")) else {
        return Detected::NotFound;
    };

    let mut matching_versions = v
        .versions
        .iter()
        .filter(|entry| entry.content_version == content_version)
        .peekable();
    let known_version = matching_versions.peek().is_some();
    if let Some(entry) = matching_versions.find(|entry| {
        entry.eboot_sha256.is_none() || entry.eboot_sha256.as_ref() == Some(&eboot_sha256)
    }) {
        return Detected::Supported {
            dump,
            entry: entry.clone(),
            eboot_sha256,
        };
    }

    let version = display_version(content_version);
    let message = if known_version {
        format!(
            "This copy of version {version} is not supported. AstroPort runs {}.",
            supported_versions(v)
        )
    } else {
        unsupported_message(&version, v)
    };
    Detected::Unsupported {
        dump,
        version,
        message,
    }
}

pub fn run_folder(data: &Path, version: &str) -> PathBuf {
    data.join("generated").join(format!("run-{version}"))
}

#[derive(Deserialize)]
struct BuildRecord {
    format: u32,
    version: String,
    eboot_sha256: String,
    release: String,
}

pub fn built(data: &Path, version: &str, eboot_sha256: &str, release: &str) -> bool {
    let run = run_folder(data, version);
    // M2-5 sheet §3.5 requires a regular game file without following a symlink.
    if !fs::symlink_metadata(run.join("AstroBot")).is_ok_and(|metadata| metadata.is_file()) {
        return false;
    }

    let record_path = run.join("build.json");
    if !record_path.is_file() {
        return false;
    }
    let Ok(bytes) = fs::read(record_path) else {
        return false;
    };
    let Ok(record) = serde_json::from_slice::<BuildRecord>(&bytes) else {
        return false;
    };
    record.format == 1
        && record.version == version
        && record.eboot_sha256 == eboot_sha256
        && record.release == release
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::symlink;
    use tempfile::tempdir;

    const PATCHES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../patches");
    const TITLE_ID: &str = "PPSA21567";
    const FIXTURE_HASH: &str = "f16d05ec6b29248d2c61adb1e9263f78e4f7bace1b955014a2d17872cfe4064d";

    pub(super) fn repo() -> Versions {
        load_versions(&Path::new(PATCHES_DIR).join("versions.json")).unwrap()
    }

    fn write_dump(folder: &Path, eboot: &[u8], param: &str) {
        fs::create_dir_all(folder.join("sce_sys")).unwrap();
        fs::write(folder.join("eboot.bin"), eboot).unwrap();
        fs::write(folder.join("sce_sys/param.json"), param).unwrap();
    }

    #[test]
    fn repo_versions_file() {
        let v = repo();
        assert_eq!(v.title_id, TITLE_ID);
        assert_eq!(v.versions[0].eboot_sha256, None);
        assert_eq!(
            v.versions[1].eboot_sha256.as_deref(),
            Some("e88e19a688289e1ea557a6100b9b7d89fb3041875a7aa8471b361bfedf59fa86")
        );
        for entry in &v.versions {
            assert!(Path::new(PATCHES_DIR).join(&entry.patches).is_file());
        }
    }

    #[test]
    fn display_versions() {
        assert_eq!(display_version("01.019.000"), "1.019");
        assert_eq!(display_version("10.019.001"), "10.019");
        assert_eq!(display_version("weird"), "weird");
        assert_eq!(display_version("01.019.xyz"), "01.019.xyz");
    }

    #[test]
    fn unsupported_text() {
        assert_eq!(
            unsupported_message("1.020", &repo()),
            "Version 1.020 is not supported. AstroPort runs 1.018 and 1.019."
        );
    }

    #[test]
    fn finds_dump_in_child_folder() {
        let parent = tempdir().unwrap();
        let child = parent.path().join("PPSA21567-app");
        let param = json!({"titleId": TITLE_ID, "contentVersion": "01.019.000"});
        for folder in [parent.path().join("Z-app"), child.clone()] {
            write_dump(&folder, b"fixture", &param.to_string());
        }
        assert_eq!(
            find_dump(parent.path(), TITLE_ID),
            Some((child, param.clone()))
        );

        write_dump(parent.path(), b"fixture", &param.to_string());
        assert_eq!(
            find_dump(parent.path(), TITLE_ID),
            Some((parent.path().to_owned(), param))
        );
    }

    #[test]
    fn foreign_or_broken_param_json_is_not_found() {
        let folder = tempdir().unwrap();
        let v = repo();
        let mut cache = HashCache::default();
        for param in [
            r#"{"titleId":"PPSA00001","contentVersion":"01.019.000"}"#,
            "{not json",
            r#"{"titleId":"PPSA21567"}"#,
        ] {
            write_dump(folder.path(), b"fixture", param);
            assert!(matches!(
                detect(Some(folder.path()), &v, &mut cache),
                Detected::NotFound
            ));
        }
    }

    #[test]
    fn version_only_entry_matches_any_hash() {
        let folder = tempdir().unwrap();
        write_dump(
            folder.path(),
            b"fixture",
            r#"{"titleId":"PPSA21567","contentVersion":"01.018.000"}"#,
        );
        assert!(matches!(
            detect(Some(folder.path()), &repo(), &mut HashCache::default()),
            Detected::Supported { dump, entry, eboot_sha256 }
                if dump == folder.path() && entry.version == "1.018" && eboot_sha256 == FIXTURE_HASH
        ));
    }

    #[test]
    fn hash_must_match_when_listed() {
        let folder = tempdir().unwrap();
        let mut v = repo();
        v.versions[1].eboot_sha256 = Some(FIXTURE_HASH.to_owned());
        let mut cache = HashCache::default();
        let param = r#"{"titleId":"PPSA21567","contentVersion":"01.019.000"}"#;
        write_dump(folder.path(), b"fixture", param);
        assert!(matches!(
            detect(Some(folder.path()), &v, &mut cache),
            Detected::Supported { entry, eboot_sha256, .. }
                if entry.version == "1.019" && eboot_sha256 == FIXTURE_HASH
        ));
        write_dump(folder.path(), b"other", param);
        assert!(matches!(
            detect(Some(folder.path()), &v, &mut cache),
            Detected::Unsupported { dump, version, message }
                if dump == folder.path() && version == "1.019"
                    && message == "This copy of version 1.019 is not supported. AstroPort runs 1.018 and 1.019."
        ));
    }

    #[test]
    fn other_version_unsupported() {
        let folder = tempdir().unwrap();
        write_dump(
            folder.path(),
            b"fixture",
            r#"{"titleId":"PPSA21567","contentVersion":"01.020.000"}"#,
        );
        assert!(matches!(
            detect(Some(folder.path()), &repo(), &mut HashCache::default()),
            Detected::Unsupported { version, message, .. }
                if version == "1.020"
                    && message == "Version 1.020 is not supported. AstroPort runs 1.018 and 1.019."
        ));
    }

    #[test]
    fn not_set() {
        let v = repo();
        let mut cache = HashCache::default();
        for folder in [None, Some(Path::new(""))] {
            assert!(matches!(detect(folder, &v, &mut cache), Detected::NotSet));
        }
    }

    #[test]
    fn built_needs_matching_record() {
        let data = tempdir().unwrap();
        let run = data.path().join("generated/run-1.019");
        assert_eq!(run_folder(data.path(), "1.019"), run);
        let is_built = || built(data.path(), "1.019", "h", "0.1.0");
        assert!(!is_built());

        fs::create_dir_all(&run).unwrap();
        let game = run.join("AstroBot");
        let record = run.join("build.json");
        fs::write(&game, b"fixture").unwrap();
        fs::write(&record, "{broken").unwrap();
        assert!(!is_built());

        let mut valid =
            json!({"format": 1, "version": "1.019", "eboot_sha256": "h", "release": "0.1.0"});
        fs::write(&record, valid.to_string()).unwrap();
        assert!(is_built());
        valid["content_version"] = json!("01.019.000");
        valid["inputs_sha256"] = json!("inputs");
        valid["to_intel"] = json!(false);
        valid["dump"] = json!(data.path());
        fs::write(&record, valid.to_string()).unwrap();
        assert!(is_built());

        for (key, value) in [
            ("format", json!(2)),
            ("release", json!("0.0.9")),
            ("eboot_sha256", json!("other")),
            ("version", json!("1.018")),
        ] {
            let mut invalid = valid.clone();
            invalid[key] = value;
            fs::write(&record, invalid.to_string()).unwrap();
            assert!(!is_built(), "accepted a mismatched {key}");
        }

        fs::write(&record, valid.to_string()).unwrap();
        let target = run.join("stub");
        fs::rename(&game, &target).unwrap();
        symlink(&target, &game).unwrap();
        assert!(!is_built());
    }
}

#[cfg(test)]
#[test]
#[ignore]
fn owner_dump_is_1019() {
    // Opt-in read-only check. The private src-1019 has no sce_sys directory.
    let d = std::env::var("ASTROPORT_TEST_DUMP").expect("set ASTROPORT_TEST_DUMP");
    assert!(matches!(
        detect(Some(Path::new(&d)), &tests::repo(), &mut HashCache::default()),
        Detected::Supported { ref entry, .. } if entry.version == "1.019"
    ));
}
