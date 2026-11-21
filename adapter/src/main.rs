mod dump;
mod rpc;

use rpc::{read_frame, write_frame, RpcError};
use serde_json::{json, Value};
use std::{io, process::ExitCode};

struct State {
    events: Vec<Value>,
}

fn dispatch(_state: &mut State, method: &str, _params: &Value) -> Result<Value, RpcError> {
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
        _ => Err(RpcError::new(
            "unknown-method",
            format!("AstroPort does not know {method}."),
        )),
    }
}

fn run() -> io::Result<()> {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let mut state = State { events: Vec::new() };

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
