mod common;

use common::{Adapter, Fixture, FIXTURE_EBOOT_SHA256};
use serde_json::{json, Value};
use std::fs;

fn find_item<'a>(page: &'a Value, id: &str) -> Option<&'a Value> {
    page["sections"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|section| section["items"].as_array().unwrap())
        .find(|item| item["id"] == id)
}

fn item<'a>(page: &'a Value, id: &str) -> &'a Value {
    find_item(page, id).unwrap_or_else(|| panic!("no item {id} in {page}"))
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
        json!([
            "library",
            "page",
            "values",
            "jobs",
            "live",
            "native_frames",
            "metrics"
        ])
    );
    assert_eq!(
        r["result"]["controllers"],
        json!([{"kind": "dualsense",
            "features": ["gyro", "touchpad", "adaptive_triggers", "haptics"]}])
    );
    assert_eq!(
        r["result"]["metrics"],
        json!([
            {"key": "game_fps", "label": "Game FPS", "unit": "fps", "type": "number", "min_interval_ms": 1000},
            {"key": "frame_time_ms", "label": "Frame time", "unit": "ms", "type": "number", "min_interval_ms": 1000}
        ])
    );
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
    assert!(
        find_item(&page, "graphics.render").is_none()
            && find_item(&page, "graphics.dlss").is_none()
    );
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
    let mut expected = json!({
        "display.resolution": {"level": "honoured", "live": "live", "applier": "source"},
        "display.fps": {"level": "honoured", "live": "next_launch", "applier": "source", "values": [30, 60]},
        "perf.low_latency": {"level": "honoured", "live": "next_launch", "applier": "source", "values": ["off", "on"], "default": "on"}
    });
    // sheet §2.8: the three upscaling keys only on an NVIDIA GPU (this machine's GPU decides).
    if page["standard"].get("perf.upscaler").is_some() {
        expected["perf.upscaling"] = json!({"level": "honoured", "default": false});
        expected["perf.upscaler"] =
            json!({"level": "honoured", "values": ["dlss"], "method": "dlss"});
        expected["perf.render_scale"] = json!({"level": "honoured", "choices": [
            {"value": 50, "label": "Performance"},
            {"value": 63, "label": "Balanced"},
            {"value": 87, "label": "Quality"}
        ], "default": 63, "render_sizes": ["1920x1080", "2432x1368", "3328x1872", "3840x2160"]});
    }
    assert_eq!(page["standard"], expected);
    let overlay = &sections[5];
    assert_eq!(overlay["surface"], "overlay");
    assert!(overlay.get("tab").is_none());
    assert_eq!(
        overlay["items"],
        json!([{
            "id": "controller.haptics", "kind": "slider", "label": "Haptic strength",
            "scope": "game_on_device", "restart": "live", "default": 250,
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
fn build_release_matches_the_installed_manifest() {
    let f = Fixture::built();
    fs::write(
        f.install.join("ags-extension.toml"),
        "version = \"9.0.0-testing.1\"\n",
    )
    .unwrap();
    let path = f.data.join("generated/run-1.019/build.json");
    let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    record["release"] = json!("9.0.0-testing.1");
    fs::write(path, record.to_string()).unwrap();

    let mut a = Adapter::spawn(&f);
    assert_eq!(
        a.call("hello", json!({}))["result"]["version"],
        "9.0.0-testing.1"
    );
    set_dump(&mut a, &f);
    assert_eq!(
        a.call("library.list", json!({}))["result"]["games"][0]["installed"],
        true
    );
    let reply = launch(&mut a, json!({}), json!({}));
    assert!(reply.get("result").is_some(), "{reply}");
}

#[test]
fn missing_or_unlisted_generated_executable_shows_build_again() {
    let f = Fixture::built();
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    assert_eq!(
        a.call("library.list", json!({}))["result"]["games"][0]["installed"],
        true
    );

    let generated = f.data.join("../.generated.json");
    fs::remove_file(&generated).unwrap();
    for record in [None, Some(r#"{"version":1,"files":{}}"#)] {
        if let Some(record) = record {
            fs::write(&generated, record).unwrap();
        }
        let page = describe_game(&mut a);
        assert_eq!(page["sections"][0]["id"], "astroport.build");
        assert_eq!(item(&page, "build")["job"], true);
        assert_eq!(
            a.call("library.list", json!({}))["result"]["games"][0]["installed"],
            false
        );
        assert_eq!(
            launch(&mut a, json!({}), json!({}))["error"],
            json!({"code": "failed", "message": "Build the game files first."})
        );
    }
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
    // No upscaling keys and no old saved values: 100% of the output size, DLSS off.
    assert_eq!(e["ASTROHYBRID_DLSS"], "0");
    assert!(e["ASTRO_PATCH_ENABLE"]
        .as_str()
        .unwrap()
        .split(',')
        .any(|patch| patch == "fixed render resolution 3840x2160"));
    assert_eq!(r["writable"], json!([run, f.data.join("saves")]));
    assert_eq!(r["native_frames"], true);
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
    let resolution = "3840x2160";
    let reply = launch(
        &mut a,
        json!({"display.resolution": resolution}),
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
fn upscaling_picks_the_render_patch_and_dlss() {
    let f = Fixture::built();
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    let nvidia = describe_game(&mut a)["standard"]
        .get("perf.upscaler")
        .is_some();
    let mut patches = |standard: Value| {
        let reply = launch(&mut a, standard, json!({}));
        let env = reply["result"]["env"].clone();
        let enabled: Vec<String> = env["ASTRO_PATCH_ENABLE"]
            .as_str()
            .unwrap()
            .split(',')
            .map(str::to_owned)
            .collect();
        (
            enabled,
            env["ASTROHYBRID_DLSS"].as_str().unwrap().to_owned(),
        )
    };
    let (enabled, dlss) = patches(json!({
        "display.resolution": "3840x2160",
        "perf.upscaling": true, "perf.upscaler": "dlss", "perf.render_scale": 50
    }));
    let (render, want_dlss) = if nvidia {
        ("1920x1080", "1")
    } else {
        ("3840x2160", "0")
    };
    assert!(
        enabled.contains(&format!("fixed render resolution {render}")),
        "{enabled:?}"
    );
    assert_eq!(dlss, want_dlss);
    let (enabled, dlss) = patches(json!({
        "display.resolution": "3840x2160",
        "perf.upscaling": false, "perf.upscaler": "none", "perf.render_scale": 100
    }));
    assert!(
        enabled.contains(&"fixed render resolution 3840x2160".to_owned()),
        "{enabled:?}"
    );
    assert_eq!(dlss, "0");
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

#[test]
fn job_plan_build_returns_the_runner() {
    let f = Fixture::new();
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    let reply = a.call(
        "job.plan",
        json!({"job": "build", "game": "PPSA21567", "params": {}, "cleanup": false, "resume": null}),
    );
    assert_eq!(reply["error"], Value::Null, "{reply}");
    let r = &reply["result"];
    assert_eq!(r["label"], "Build game files");
    assert_eq!(
        r["argv"],
        json!([
            f.install.join("bin/astroport-run"),
            "build",
            "--dump",
            f.dump,
            "--install",
            f.install,
            "--data",
            f.data
        ])
    );
    assert_eq!(
        r["steps"],
        json!([
            "Check the game dump",
            "Unwrap the game files",
            "Convert for Linux",
            "Set up the game folder",
            "Record the result"
        ])
    );
    assert!(r["steps"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| (1..=40).contains(&s.as_str().unwrap().chars().count())));
    assert_eq!(r["env"], json!({}));
    assert_eq!(r["cwd"], json!(f.data));
    assert_eq!(
        r["writable"],
        json!([f.data.join("generated"), f.data.join("saves")])
    );
    assert_eq!(
        r["admission"],
        json!({"blocks_launch": true, "pauses_while_playing": false, "runs_while_playing": false, "counts_against_downloads": false})
    );
    assert_eq!(r["notify"], json!({"completed": true, "failed": true}));
    assert_eq!(
        (
            r["changes_software"].as_bool(),
            r["resumable"].as_bool(),
            r["cleanup"].as_bool()
        ),
        (Some(true), Some(false), Some(false))
    );
    assert_eq!(
        a.call(
            "job.plan",
            json!({"job": "other", "game": "PPSA21567", "params": {}})
        )["error"],
        json!({"code": "not-found", "message": "AstroPort has no such job."})
    );
    for params in [
        json!({"job": "build", "game": "CUSA00001", "params": {}}),
        json!({"job": "build", "params": {}}),
    ] {
        assert_eq!(
            a.call("job.plan", params)["error"],
            json!({"code": "not-found", "message": "AstroPort has no such game."})
        );
    }
    assert_eq!(
        a.call(
            "hello",
            json!({"protocol": 2, "ags": "0.20.0", "features": ["jobs"]})
        )["result"]["features"],
        json!([
            "library",
            "page",
            "values",
            "jobs",
            "live",
            "native_frames",
            "metrics"
        ])
    );
}

#[test]
fn job_plan_refuses_an_unsupported_dump() {
    let f = Fixture::new();
    fs::write(
        f.dump.join("sce_sys/param.json"),
        br#"{"titleId":"PPSA21567","contentVersion":"01.020.000"}"#,
    )
    .unwrap();
    let mut a = Adapter::spawn(&f);
    set_dump(&mut a, &f);
    assert_eq!(
        a.call(
            "job.plan",
            json!({"job": "build", "game": "PPSA21567", "params": {}})
        )["error"],
        json!({
            "code": "failed",
            "message": "Version 1.020 is not supported. AstroPort runs 1.018 and 1.019."
        })
    );
}

#[test]
fn sheet_points_are_marked() {
    let s = include_str!("../src/sheet.rs");
    let n = |p: &str| s.lines().filter(|l| l.trim_start().starts_with(p)).count();
    assert!(n("// sheet §") >= 10);
    assert_eq!(n("// SHEET:"), 2); // both unresolved points concern the library
}
