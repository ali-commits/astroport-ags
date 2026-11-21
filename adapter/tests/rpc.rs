mod common;

use common::{Adapter, Fixture};
use serde_json::json;

#[test]
fn hello_answers_protocol_2() {
    let f = Fixture::new();
    let mut a = Adapter::spawn(&f);
    let r = a.call(
        "hello",
        json!({"protocol": 2, "ags": "0.19.0", "features": []}),
    );
    assert_eq!(r["result"]["protocol"], 2);
    assert_eq!(r["result"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(r["result"]["roles"], json!(["launcher"]));
    assert_eq!(
        r["result"]["features"],
        json!(["library", "page", "values"])
    );
    assert_eq!(
        r["result"]["controllers"],
        json!([{"kind": "dualsense",
            "features": ["gyro", "touchpad", "adaptive_triggers", "haptics"]}])
    );
    assert_eq!(r["result"]["metrics"], json!([]));
}

#[test]
fn unknown_method() {
    let f = Fixture::new();
    let mut a = Adapter::spawn(&f);
    let r = a.call("nope", json!({}));
    assert_eq!(r["error"]["code"], "unknown-method");
    assert_eq!(r["error"]["message"], "AstroPort does not know nope.");
}
