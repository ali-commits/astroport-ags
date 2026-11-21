#[allow(dead_code)] // Launch uses the remaining device fields in a later task.
mod device;
mod dump;
mod page;
mod rpc;
#[allow(dead_code)] // Launch uses the remaining settings map fields in a later task.
mod settings;
mod sheet;
mod values;

use device::Device;
use dump::{Detected, HashCache, Versions};
use page::GAME_ID;
use rpc::{read_frame, write_frame, RpcError};
use serde_json::{json, Value};
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::ExitCode,
};
use values::Store;

const DAMAGED_INSTALLATION: &str = "The AstroPort installation is damaged. Repair it.";

struct State {
    install: PathBuf,
    data: PathBuf,
    versions: Versions,
    map: Result<settings::SettingsMap, String>,
    store: Store,
    cache: HashCache,
    device: Option<Device>,
    events: Vec<Value>,
}

impl State {
    fn device(&mut self) -> &Device {
        self.device
            .get_or_insert_with(|| device::probe(Path::new("/sys"), device::nvidia_smi))
    }

    fn detect(&mut self) -> Detected {
        dump::detect(
            self.store.dump_folder().as_deref(),
            &self.versions,
            &mut self.cache,
        )
    }

    fn game_page(&mut self, game: &Value) -> Result<Value, RpcError> {
        let not_found = || RpcError::new("not-found", "AstroPort has no such game.");
        if game != GAME_ID {
            return Err(not_found());
        }
        let detected = self.detect();
        if matches!(detected, Detected::NotSet | Detected::NotFound) {
            return Err(not_found());
        }

        let defaults = device::defaults(self.device());
        let map = self
            .map
            .as_ref()
            .map_err(|_| RpcError::new("failed", DAMAGED_INSTALLATION))?;
        let (is_built, available) = match &detected {
            Detected::Supported {
                entry,
                eboot_sha256,
                ..
            } => (
                dump::built(
                    &self.data,
                    &entry.version,
                    eboot_sha256,
                    env!("CARGO_PKG_VERSION"),
                ),
                map.available(&self.install.join("patches").join(&entry.patches))
                    .map_err(|_| RpcError::new("failed", DAMAGED_INSTALLATION))?,
            ),
            _ => (false, Default::default()),
        };
        Ok(page::game_page(
            &detected, is_built, map, &available, &defaults,
        ))
    }

    fn library_list(&mut self) -> Value {
        let (directory, version, installed) = match self.detect() {
            Detected::Supported {
                dump,
                entry,
                eboot_sha256,
            } => {
                let installed = dump::built(
                    &self.data,
                    &entry.version,
                    &eboot_sha256,
                    env!("CARGO_PKG_VERSION"),
                );
                (dump, entry.version, installed)
            }
            Detected::Unsupported { dump, version, .. } => (dump, version, false),
            Detected::NotSet | Detected::NotFound => return json!({"games": []}),
        };

        let mut game = json!({
            "id": GAME_ID,
            "title": "Astro Bot",
            "platform": "PS5",
            "identity": {"scheme": "ps5-title", "value": GAME_ID},
            "version": version,
            "installed": installed
        });
        let art = directory.join("sce_sys/icon0.png");
        if let Some(path) = art.to_str().filter(|_| {
            fs::symlink_metadata(&art)
                .is_ok_and(|meta| meta.is_file() && meta.len() <= 2 * 1024 * 1024)
        }) {
            game["cover"] = json!(path);
            game["icon"] = json!(path);
        }
        json!({"games": [game]})
    }
}

fn dispatch(state: &mut State, method: &str, params: &Value) -> Result<Value, RpcError> {
    if method != "hello" && state.map.is_err() {
        return Err(RpcError::new("failed", DAMAGED_INSTALLATION));
    }
    match method {
        "hello" => Ok(json!({
            "protocol": 2,
            "version": env!("CARGO_PKG_VERSION"),
            "roles": ["launcher"],
            "features": ["library", "page", "values"],
            "controllers": [{
                "kind": "dualsense",
                "features": ["gyro", "touchpad", "adaptive_triggers", "haptics"]
            }],
            "metrics": []
        })),
        "library.list" => Ok(state.library_list()),
        "values.get" => match params["scope"].as_str() {
            Some("machine") => Ok(json!({"values": state.store.machine()})),
            Some("game") => Ok(json!({"values": {}})),
            _ => Err(RpcError::new(
                "invalid-params",
                "Choose a valid value scope.",
            )),
        },
        "values.set" => {
            match params["scope"].as_str() {
                Some("machine") => {}
                Some("game") => {
                    return Err(RpcError::new(
                        "invalid-params",
                        "AstroPort has no per-game values.",
                    ));
                }
                _ => {
                    return Err(RpcError::new(
                        "invalid-params",
                        "Choose a valid value scope.",
                    ))
                }
            }
            let values = serde_json::from_value(params["values"].clone()).map_err(|_| {
                RpcError::new("invalid-params", "Provide machine values as an object.")
            })?;
            state.store.set_machine(values).map_err(|message| {
                let code = if message == values::SAVE_ERROR {
                    "failed"
                } else {
                    "invalid-params"
                };
                RpcError::new(code, message)
            })?;
            state
                .events
                .push(json!({"method": "library.changed", "params": {}}));
            state
                .events
                .push(json!({"method": "page.changed", "params": {}}));
            Ok(json!({"values": state.store.machine()}))
        }
        "page.describe" => {
            let page = if params["game"].is_null() {
                let detected = state.detect();
                page::extension_page(&state.store, &detected)
            } else {
                state.game_page(&params["game"])?
            };
            if cfg!(debug_assertions) {
                let limits = page::check_limits(&page);
                debug_assert!(limits.is_ok(), "{limits:?}");
            }
            Ok(page)
        }
        _ => Err(RpcError::new(
            "unknown-method",
            format!("AstroPort does not know {method}."),
        )),
    }
}

fn run() -> io::Result<()> {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let executable = env::current_exe()?;
    let install = executable
        .parent()
        .and_then(|bin| bin.parent())
        .ok_or_else(|| io::Error::other("Cannot find the AstroPort install folder."))?
        .to_owned();
    let data = env::current_dir()?;
    let versions =
        dump::load_versions(&install.join("patches/versions.json")).map_err(io::Error::other)?;
    let map = settings::load(&install.join("patches/settings.json"));
    let store = Store::load(&data);
    let mut state = State {
        install,
        data,
        versions,
        map,
        store,
        cache: HashCache::default(),
        device: None,
        events: Vec::new(),
    };

    while let Some(request) = read_frame(&mut stdin)? {
        let id = &request["id"];
        let result = match request["method"].as_str() {
            Some(method) => dispatch(&mut state, method, &request["params"]),
            None => Err(RpcError::new(
                "invalid-params",
                "An adapter method is required.",
            )),
        };
        let reply = match result {
            Ok(result) => json!({"id": id, "result": result}),
            Err(e) => json!({"id": id, "error": {"code": e.code, "message": e.message}}),
        };
        write_frame(&mut stdout, &reply)?;
        for event in state.events.drain(..) {
            write_frame(&mut stdout, &event)?;
        }
    }

    Ok(())
}

fn main() -> ExitCode {
    if let Err(error) = run() {
        eprintln!("astroport-adapter: {error}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
