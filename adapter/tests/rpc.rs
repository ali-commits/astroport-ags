mod common;

use common::{Adapter, Fixture};
use serde_json::{json, Value};
use std::fs;

fn item<'a>(page: &'a Value, id: &str) -> &'a Value {
    page["sections"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|section| section["items"].as_array().unwrap())
        .find(|item| item["id"] == id)
        .unwrap_or_else(|| panic!("no item {id} in {page}"))
}

fn set_dump(a: &mut Adapter, f: &Fixture) {
    let r = a.call(
        "values.set",
        json!({"scope": "machine", "values": {"dump_folder": f.dump}}),
    );
    assert_eq!(r["result"]["values"]["dump_folder"], json!(f.dump), "{r}");
    assert_eq!(
        a.next_frame(),
        json!({"method": "library.changed", "params": {}})
    );
    assert_eq!(
        a.next_frame(),
        json!({"method": "page.changed", "params": {}})
    );
}

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

#[test]
fn no_dump_no_game() {
    let f = Fixture::new();
    let mut a = Adapter::spawn(&f);
    let library = a.call("library.list", json!({}));
    assert_eq!(library["result"]["games"], json!([]), "{library}");
    let p = a.call("page.describe", json!({}))["result"].clone();
    let section = &p["sections"][0];
    assert_eq!(p["sections"].as_array().unwrap().len(), 1);
    assert_eq!(section["id"], "astroport.dump");
    assert_eq!(section["surface"], "panel");
    assert_eq!(section["title"], "Game dump");
    assert!(section.get("tab").is_none());
    assert_eq!(
        item(&p, "dump_folder"),
        &json!({
            "id": "dump_folder",
            "kind": "path",
            "pick": "folder",
            "nullable": true,
            "null_label": "Not set",
            "scope": "machine",
            "label": "Astro Bot dump folder",
            "help": "The folder with eboot.bin and sce_sys, or the folder above it."
        })
    );
    assert_eq!(
        item(&p, "dump.status")["text"],
        "Choose the folder of your Astro Bot dump."
    );
}

#[test]
fn set_dump_lists_the_game_and_notifies() {
    let f = Fixture::new();
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    let library = a.call("library.list", json!({}));
    assert_eq!(
        library["result"]["games"],
        json!([{
            "id": "PPSA21567",
            "title": "Astro Bot",
            "platform": "PS5",
            "identity": {"scheme": "ps5-title", "value": "PPSA21567"},
            "version": "1.019",
            "installed": false,
            "cover": f.dump.join("sce_sys/icon0.png"),
            "icon": f.dump.join("sce_sys/icon0.png")
        }])
    );
    let p = a.call("page.describe", json!({}))["result"].clone();
    assert_eq!(item(&p, "dump.status")["text"], "Found version 1.019.");

    drop(a);
    let mut b = Adapter::spawn(&f);
    assert_eq!(
        b.call("values.get", json!({"scope": "machine"}))["result"]["values"]["dump_folder"],
        json!(f.dump)
    );
}

#[test]
fn relative_dump_folder_refused() {
    let f = Fixture::new();
    let mut a = Adapter::spawn(&f);
    let r = a.call(
        "values.set",
        json!({"scope": "machine", "values": {"dump_folder": "games/astro"}}),
    );
    assert_eq!(
        r["error"],
        json!({"code": "invalid-params", "message": "Invalid value for dump_folder."}),
        "{r}"
    );
}

#[test]
fn broken_param_json_says_not_found() {
    let f = Fixture::new();
    fs::write(f.dump.join("sce_sys/param.json"), "{not json").unwrap();
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    let p = a.call("page.describe", json!({}))["result"].clone();
    assert_eq!(
        item(&p, "dump.status")["text"],
        "No Astro Bot dump in this folder."
    );
    assert_eq!(
        a.call("library.list", json!({}))["result"]["games"],
        json!([])
    );
}

#[test]
fn unsupported_version_listed_not_installed() {
    let f = Fixture::new();
    fs::write(
        f.dump.join("sce_sys/param.json"),
        r#"{"titleId":"PPSA21567","contentVersion":"01.020.000"}"#,
    )
    .unwrap();
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    let g = &a.call("library.list", json!({}))["result"]["games"][0];
    assert_eq!(g["version"], "1.020");
    assert_eq!(g["installed"], false);
    let p = a.call("page.describe", json!({}))["result"].clone();
    assert_eq!(
        item(&p, "dump.status")["text"],
        "Version 1.020 is not supported. AstroPort runs 1.018 and 1.019."
    );
}

#[test]
fn unsupported_version_status_is_one_line() {
    let f = Fixture::new();
    fs::write(
        f.dump.join("sce_sys/param.json"),
        r#"{"titleId":"PPSA21567","contentVersion":"01.020.000\npreview"}"#,
    )
    .unwrap();
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    let page = a.call("page.describe", json!({}));
    assert_eq!(
        item(&page["result"], "dump.status")["text"],
        "Version 01.020.000 preview is not supported. AstroPort runs 1.018 and 1.019."
    );
}

#[test]
fn oversized_icon_left_out() {
    let f = Fixture::new();
    let icon = fs::File::options()
        .write(true)
        .open(f.dump.join("sce_sys/icon0.png"))
        .unwrap();
    icon.set_len(2 * 1024 * 1024 + 1).unwrap();
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    let g = &a.call("library.list", json!({}))["result"]["games"][0];
    assert_eq!(g["id"], "PPSA21567");
    assert!(g.get("cover").is_none());
    assert!(g.get("icon").is_none());
}

#[test]
fn game_values_are_empty_and_read_only() {
    let f = Fixture::new();
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    let r = a.call("values.get", json!({"scope": "game", "game": "PPSA21567"}));
    assert_eq!(r["result"], json!({"values": {}}));
    let r = a.call(
        "values.set",
        json!({"scope": "game", "game": "PPSA21567", "values": {}}),
    );
    assert_eq!(
        r["error"],
        json!({"code": "invalid-params", "message": "AstroPort has no per-game values."})
    );
}
