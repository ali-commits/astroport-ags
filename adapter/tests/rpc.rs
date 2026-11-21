mod common;

use common::{Adapter, Fixture, FIXTURE_EBOOT_SHA256};
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

fn describe_game(a: &mut Adapter) -> Value {
    let reply = a.call("page.describe", json!({"game": "PPSA21567"}));
    assert!(reply["result"].is_object(), "{reply}");
    reply["result"].clone()
}

#[test]
fn game_page_supported_not_built() {
    let f = Fixture::new();
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    let page = describe_game(&mut a);
    let sections = page["sections"].as_array().unwrap();
    let ids: Vec<_> = sections.iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(
        ids,
        [
            "astroport.build",
            "astroport.graphics",
            "astroport.controller",
            "astroport.game",
            "astroport.advanced",
            "astroport.overlay",
        ]
    );
    assert_eq!(item(&page, "build")["job"], true);
    assert!(item(&page, "graphics.ultrawide").is_object());
    let render = item(&page, "graphics.render");
    assert!(render["options"]
        .as_array()
        .unwrap()
        .iter()
        .any(|option| option["value"] == render["default"]));
    assert_eq!(
        item(&page, "advanced.patches")["default"],
        json!([
            "pad.r1x",
            "fix.havok_contact",
            "fix.havok_tree",
            "fix.bot_rescue",
            "video.speed"
        ])
    );
    assert_eq!(
        page["standard"],
        json!({
            "display.resolution": {"level": "honoured", "live": "next_launch", "applier": "source"},
            "display.fps": {"level": "honoured", "live": "next_launch", "applier": "source", "values": [30, 60]},
            "perf.low_latency": {"level": "honoured", "live": "next_launch", "applier": "source", "values": ["off", "on"]}
        })
    );
    let overlay = &sections[5];
    assert_eq!(overlay["surface"], "overlay");
    assert!(overlay.get("tab").is_none());
    assert_eq!(
        overlay["items"],
        json!([{
            "id": "controller.haptics", "kind": "slider", "label": "Haptic strength",
            "scope": "game_on_device", "restart": "next_launch", "default": 250,
            "min": 100, "max": 400, "step": 25, "unit": "%",
            "stops": [{"value": 100, "label": "As sent"}]
        }])
    );
    assert_eq!(sections[2]["items"].as_array().unwrap().len(), 1);
    assert_eq!(sections[2]["items"][0]["id"], "controller.rumble");
}

#[test]
fn built_run_folder_hides_the_build_button() {
    let f = Fixture::built();
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    let page = describe_game(&mut a);
    assert!(page["sections"]
        .as_array()
        .unwrap()
        .iter()
        .all(|section| section["id"] != "astroport.build"));
    assert_eq!(
        a.call("library.list", json!({}))["result"]["games"][0]["installed"],
        true
    );
}

#[test]
fn rebuilt_dump_shows_build_again() {
    let f = Fixture::new();
    f.write_build_record("1.019", &"0".repeat(64));
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    let page = describe_game(&mut a);
    assert_eq!(page["sections"][0]["id"], "astroport.build");
    assert_eq!(item(&page, "build")["job"], true);
    assert_eq!(
        a.call("library.list", json!({}))["result"]["games"][0]["installed"],
        false
    );
}

#[test]
fn version_1018_has_no_ultrawide() {
    let f = Fixture::new();
    fs::write(
        f.dump.join("sce_sys/param.json"),
        r#"{"titleId":"PPSA21567","contentVersion":"01.018.000"}"#,
    )
    .unwrap();
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    let page = describe_game(&mut a);
    let graphics = &page["sections"][1]["items"];
    assert!(graphics
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["id"] != "graphics.ultrawide"));
}

#[test]
fn unsupported_page_has_only_the_message() {
    let f = Fixture::new();
    fs::write(
        f.dump.join("sce_sys/param.json"),
        r#"{"titleId":"PPSA21567","contentVersion":"01.020.000"}"#,
    )
    .unwrap();
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    let page = describe_game(&mut a);
    assert_eq!(
        page["sections"],
        json!([{
            "id": "astroport.status",
            "surface": "panel",
            "tab": "source",
            "title": "Version",
            "items": [{
                "id": "status",
                "kind": "info",
                "label": "Version",
                "text": "Version 1.020 is not supported. AstroPort runs 1.018 and 1.019."
            }]
        }])
    );
}

#[test]
fn other_game_not_found() {
    let f = Fixture::new();
    let mut a = Adapter::spawn(&f);
    let expected = json!({"code": "not-found", "message": "AstroPort has no such game."});
    assert_eq!(
        a.call("page.describe", json!({"game": "PPSA21567"}))["error"],
        expected
    );
    set_dump(&mut a, &f);
    assert_eq!(
        a.call("page.describe", json!({"game": "CUSA00001"}))["error"],
        expected
    );

    fs::remove_file(f.dump.join("sce_sys/param.json")).unwrap();
    assert_eq!(
        a.call("page.describe", json!({"game": "PPSA21567"}))["error"],
        expected
    );
}

fn launch(a: &mut Adapter, standard: Value, values: Value) -> Value {
    a.call(
        "launch.plan",
        json!({
            "game": "PPSA21567", "standard": standard, "values": values,
            "capture": "native", "controllers": ["dualsense"]
        }),
    )
}

#[test]
fn launch_env_matches_play_hybrid() {
    let f = Fixture::built();
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    let reply = launch(
        &mut a,
        json!({
            "display.fps": 30, "display.resolution": "3840x2160", "fullscreen": true, "language": "english"
        }),
        json!({}),
    );
    let r = &reply["result"];
    let run = f.data.join("generated/run-1.019");
    assert_eq!(r["argv"], json!([run.join("AstroBot")]), "{reply}");
    assert_eq!(r["cwd"], json!(run));
    let e = &r["env"];
    assert_eq!(
        e["GLIBC_TUNABLES"],
        "glibc.rtld.optional_static_tls=0x200000"
    );
    assert_eq!(
        e["KYTY_GPU_LIBRARY"],
        json!(run.join("libs/libkyty_gpu.so"))
    );
    assert_eq!(
        e["KYTY_GPU_HOST_MODULE"],
        json!(run.join("libs/libAstroHybridHost.prx"))
    );
    assert_eq!(
        e["LD_AUDIT"],
        json!(f.install.join("lib/libastro_patcher.so"))
    );
    assert_eq!(
        e["ASTRO_PATCHES"],
        json!(f.install.join("patches/PPSA21567-1019.json"))
    );
    for (key, value) in [
        ("KYTY_HANG_WATCHDOG", "auto"),
        ("ASTROHYBRID_FPS_LIMIT", "30"),
        ("ASTROHYBRID_LOW_LATENCY_INTERVAL_US", "33333"),
        ("ASTROHYBRID_LOW_LATENCY", "1"),
        ("KYTY_GPU_WIDTH", "3840"),
        ("KYTY_GPU_HEIGHT", "2160"),
        ("KYTY_GPU_FULLSCREEN", "1"),
        ("APS5_PAD_VIBRATION_GAIN", "2.5"),
        ("ANYPS5_LANGUAGE", "1"),
        ("KYTY_WAYLAND_NATIVE", "1"),
        ("ASTRO_PATCH_PROCESS", "AstroBot"),
    ] {
        assert_eq!(e[key], value, "{key}");
    }
    assert!(e["ASTRO_PATCH_ENABLE"]
        .as_str()
        .unwrap()
        .contains("precise frame cap 30 fps"));
    assert!(e["ASTRO_PATCH_SKIP"]
        .as_str()
        .unwrap()
        .contains("precise frame cap 60 fps"));
    assert!(e.get("LD_PRELOAD").is_none() && e.get("LD_LIBRARY_PATH").is_none());
    let page = describe_game(&mut a);
    let dlss_offered = page["sections"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|section| section["items"].as_array().unwrap())
        .any(|item| item["id"] == "graphics.dlss");
    assert_eq!(e["ASTROHYBRID_DLSS"], if dlss_offered { "1" } else { "0" });
    assert_eq!(r["writable"], json!([run, f.data.join("saves")]));
    assert_eq!(r["native_frames"], false);
    assert_eq!(r["controllers"], json!(["dualsense"]));
    assert_eq!(
        r["applied"],
        json!({"display.fps": 30, "display.resolution": "3840x2160"})
    );
}

#[test]
fn fps_outside_the_offer() {
    let f = Fixture::built();
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    for (standard, fps) in [
        (json!({"display.fps": 144}), 60),
        (json!({"display.fps": u64::MAX}), 60),
        (json!({"display.fps": 0}), 30),
        (json!({}), 30),
        (json!({"display.fps": "fast"}), 30),
        (json!({"display.fps": 60.5}), 30),
    ] {
        let reply = launch(&mut a, standard, json!({}));
        let result = &reply["result"];
        assert_eq!(
            result["env"]["ASTROHYBRID_FPS_LIMIT"],
            fps.to_string(),
            "{reply}"
        );
        assert_eq!(result["applied"]["display.fps"], fps);
    }
}

#[test]
fn ultrawide_sets_3440x1440() {
    let f = Fixture::built();
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    let reply = launch(
        &mut a,
        json!({
            "display.resolution": "1920x1080", "fullscreen": false,
            "perf.low_latency": "off", "language": "Koreana"
        }),
        json!({"graphics.ultrawide": true, "controller.haptics": 100}),
    );
    let r = &reply["result"];
    let env = &r["env"];
    assert!(
        env["ASTRO_PATCH_ENABLE"]
            .as_str()
            .unwrap_or("")
            .contains("ultrawide:"),
        "{reply}"
    );
    assert_eq!(env["KYTY_GPU_WIDTH"], "3440");
    assert_eq!(env["KYTY_GPU_HEIGHT"], "1440");
    assert_eq!(env["KYTY_GPU_FULLSCREEN"], "0");
    assert_eq!(env["ASTROHYBRID_LOW_LATENCY"], "0");
    assert_eq!(env["APS5_PAD_VIBRATION_GAIN"], "1");
    assert_eq!(env["ANYPS5_LANGUAGE"], "9");
    assert_eq!(r["applied"]["display.resolution"], "3440x1440");
}

#[test]
fn stale_values_take_defaults() {
    let f = Fixture::built();
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    let default_render = item(&describe_game(&mut a), "graphics.render")["default"].clone();
    let resolution = match default_render.as_str().unwrap() {
        "1080p" => "1920x1080",
        "1368p" => "2432x1368",
        "1872p" => "3328x1872",
        "2160p" => "3840x2160",
        other => panic!("unexpected default: {other}"),
    };
    let reply = launch(
        &mut a,
        json!({}),
        json!({
            "advanced.patches": ["pad.r1x", "gone.patch"],
            "graphics.render": "4320p", "controller.haptics": "loud"
        }),
    );
    let env = &reply["result"]["env"];
    assert_eq!(env["APS5_PAD_VIBRATION_GAIN"], "2.5", "{reply}");
    let enabled = env["ASTRO_PATCH_ENABLE"].as_str().unwrap();
    assert!(enabled
        .split(',')
        .any(|patch| patch == format!("fixed render resolution {resolution}")));
    assert!(enabled.contains("r1+x toggles"));
    assert!(!enabled.contains("gone.patch"));
    assert!(env["ASTRO_PATCH_SKIP"]
        .as_str()
        .unwrap()
        .contains("havok contact query"));

    let reply = launch(
        &mut a,
        json!({}),
        json!({"advanced.patches": ["pad.r1x", 123]}),
    );
    assert!(reply["result"]["env"]["ASTRO_PATCH_ENABLE"]
        .as_str()
        .unwrap()
        .contains("havok contact query"));
}

#[test]
fn not_built_refused() {
    let f = Fixture::new();
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    assert_eq!(
        launch(&mut a, json!({}), json!({}))["error"],
        json!({"code": "failed", "message": "Build the game files first."})
    );
}

#[test]
fn stale_build_refused() {
    let f = Fixture::new();
    f.write_build_record("1.018", FIXTURE_EBOOT_SHA256);
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    assert_eq!(
        launch(&mut a, json!({}), json!({}))["error"],
        json!({"code": "failed", "message": "Build the game files first."})
    );
}

#[test]
fn unsupported_refused() {
    let f = Fixture::built();
    fs::write(
        f.dump.join("sce_sys/param.json"),
        br#"{"titleId":"PPSA21567","contentVersion":"01.020.000"}"#,
    )
    .unwrap();
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    assert_eq!(
        launch(&mut a, json!({}), json!({}))["error"],
        json!({
            "code": "failed",
            "message": "Version 1.020 is not supported. AstroPort runs 1.018 and 1.019."
        })
    );
}

#[test]
fn missing_preset_refused() {
    let f = Fixture::built();
    fs::remove_file(f.install.join("presets/u59-preset.PPSA21567.json")).unwrap();
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    assert_eq!(
        launch(&mut a, json!({}), json!({}))["error"],
        json!({"code": "failed", "message": "The AstroPort installation is damaged. Repair it."})
    );
}
