use serde_json::{json, Value};
use std::{
    env, fs, io,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        mpsc::{self, Receiver, Sender},
        Mutex,
    },
    thread,
    time::Duration,
};

#[allow(dead_code)]
#[path = "../../src/rpc.rs"]
mod rpc;

// Forking during a fixture copy can inherit its writable executable descriptor.
static COPY_OR_SPAWN: Mutex<()> = Mutex::new(());

pub const FIXTURE_EBOOT_SHA256: &str =
    "f16d05ec6b29248d2c61adb1e9263f78e4f7bace1b955014a2d17872cfe4064d";

pub struct Fixture {
    pub install: PathBuf,
    pub data: PathBuf,
    pub dump: PathBuf,
    _dirs: Vec<tempfile::TempDir>,
}

impl Fixture {
    pub fn new() -> Fixture {
        let install = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let dump = tempfile::tempdir().unwrap();
        fs::create_dir(install.path().join("bin")).unwrap();
        {
            let _guard = COPY_OR_SPAWN.lock().unwrap();
            fs::copy(
                env!("CARGO_BIN_EXE_astroport-adapter"),
                install.path().join("bin/astroport-adapter"),
            )
            .unwrap();
        }
        let patches = install.path().join("patches");
        fs::create_dir(&patches).unwrap();
        for source in fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../patches")).unwrap() {
            let source = source.unwrap();
            fs::copy(source.path(), patches.join(source.file_name())).unwrap();
        }
        let versions_file = patches.join("versions.json");
        let mut versions: Value =
            serde_json::from_slice(&fs::read(&versions_file).unwrap()).unwrap();
        versions["versions"][1]["eboot_sha256"] = json!(FIXTURE_EBOOT_SHA256);
        fs::write(versions_file, serde_json::to_vec(&versions).unwrap()).unwrap();

        let presets = install.path().join("presets");
        fs::create_dir(&presets).unwrap();
        let tools = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../engines/kyty/tools"
        ));
        for name in ["u59-preset.json", "u59-preset.PPSA21567.json"] {
            fs::copy(tools.join(name), presets.join(name)).unwrap();
        }
        fs::create_dir(install.path().join("lib")).unwrap();
        fs::write(install.path().join("lib/libastro_patcher.so"), []).unwrap();

        let sce_sys = dump.path().join("sce_sys");
        fs::create_dir(&sce_sys).unwrap();
        fs::write(dump.path().join("eboot.bin"), b"fixture").unwrap();
        fs::write(
            sce_sys.join("param.json"),
            br#"{"titleId":"PPSA21567","contentVersion":"01.019.000"}"#,
        )
        .unwrap();
        fs::write(
            sce_sys.join("icon0.png"),
            b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x02\x00\x00\x00\x90wS\xde\x00\x00\x00\x0cIDATx\x9cc```\x00\x00\x00\x04\x00\x01\xf6\x178U\x00\x00\x00\x00IEND\xaeB`\x82",
        )
        .unwrap();
        Fixture {
            install: install.path().to_path_buf(),
            data: data.path().to_path_buf(),
            dump: dump.path().to_path_buf(),
            _dirs: vec![install, data, dump],
        }
    }

    pub fn built() -> Fixture {
        let fixture = Self::new();
        fixture.write_build_record("1.019", FIXTURE_EBOOT_SHA256);
        fixture
    }

    pub fn write_build_record(&self, version: &str, eboot_sha256: &str) {
        let run = self.data.join("generated").join(format!("run-{version}"));
        fs::create_dir_all(&run).unwrap();
        fs::write(run.join("AstroBot"), []).unwrap();
        let record = json!({
            "format": 1,
            "version": version,
            "eboot_sha256": eboot_sha256,
            "release": env!("CARGO_PKG_VERSION")
        });
        fs::write(run.join("build.json"), record.to_string()).unwrap();
    }
}

pub struct Adapter {
    child: Child,
    requests: Sender<Option<Value>>,
    frames: Receiver<io::Result<Option<Value>>>,
    next_id: u64,
}

impl Adapter {
    pub fn spawn(f: &Fixture) -> Adapter {
        let mut command = Command::new(f.install.join("bin/astroport-adapter"));
        command
            .current_dir(&f.data)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        for key in ["PATH", "HOME", "LANG"] {
            if let Some(value) = env::var_os(key) {
                command.env(key, value);
            }
        }
        let mut child = {
            let _guard = COPY_OR_SPAWN.lock().unwrap();
            command.spawn().unwrap()
        };
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let (requests, pending) = mpsc::channel::<Option<Value>>();
        let (output, frames) = mpsc::channel();
        thread::spawn(move || {
            for request in pending {
                let frame = (|| {
                    if let Some(request) = request {
                        rpc::write_frame(&mut stdin, &request)?;
                    }
                    rpc::read_frame(&mut stdout)
                })();
                let finished = !matches!(&frame, Ok(Some(_)));
                if output.send(frame).is_err() || finished {
                    break;
                }
            }
        });
        Adapter {
            child,
            requests,
            frames,
            next_id: 1,
        }
    }

    pub fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let reply = self.exchange(Some(json!({"id": id, "method": method, "params": params})));
        assert_eq!(reply["id"], id);
        reply
    }

    pub fn next_frame(&mut self) -> Value {
        self.exchange(None)
    }

    // None requests a read of the next event without sending an RPC.
    fn exchange(&self, request: Option<Value>) -> Value {
        self.requests
            .send(request)
            .expect("adapter I/O thread stopped");
        self.frames
            .recv_timeout(Duration::from_secs(10))
            .expect("adapter did not send a frame within 10 seconds")
            .expect("adapter frame I/O failed")
            .expect("adapter closed stdout")
    }
}

impl Drop for Adapter {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
