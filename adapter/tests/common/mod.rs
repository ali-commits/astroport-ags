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

pub struct Fixture {
    pub install: PathBuf,
    pub data: PathBuf,
    #[allow(dead_code)]
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
        Fixture {
            install: install.path().to_path_buf(),
            data: data.path().to_path_buf(),
            dump: dump.path().to_path_buf(),
            _dirs: vec![install, data, dump],
        }
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

    #[allow(dead_code)]
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
