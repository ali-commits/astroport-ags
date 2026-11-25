use crate::{
    dump::{self, VersionEntry},
    settings::SettingsMap,
    DAMAGED_INSTALLATION,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub fn language_index(name: &str) -> Option<u32> {
    const LANGUAGES: [&str; 30] = [
        "japanese",
        "english",
        "french",
        "spanish",
        "german",
        "italian",
        "dutch",
        "portuguese",
        "russian",
        "koreana",
        "tchinese",
        "schinese",
        "finnish",
        "swedish",
        "danish",
        "norwegian",
        "polish",
        "brazilian",
        "-",
        "turkish",
        "latam",
        "arabic",
        "-",
        "czech",
        "hungarian",
        "greek",
        "romanian",
        "thai",
        "vietnamese",
        "indonesian",
    ];
    LANGUAGES
        .iter()
        .position(|language| *language != "-" && language.eq_ignore_ascii_case(name))
        .map(|index| index as u32)
}

pub fn read_preset(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let bytes = fs::read(path).map_err(|_| DAMAGED_INSTALLATION)?;
    let entries: BTreeMap<String, Value> =
        serde_json::from_slice(&bytes).map_err(|_| DAMAGED_INSTALLATION)?;
    Ok(entries
        .into_iter()
        .filter_map(|(key, value)| {
            if key.starts_with('_') {
                return None;
            }
            match value {
                Value::String(value) => Some((key, value)),
                _ => None,
            }
        })
        .collect())
}

pub struct Launch<'a> {
    pub install: &'a Path,
    pub data: &'a Path,
    pub entry: &'a VersionEntry,
    pub map: &'a SettingsMap,
    pub available: &'a BTreeSet<String>,
    pub defaults: BTreeMap<String, Value>,
    pub screen: Option<(u32, u32)>,
    pub nvidia: bool,
}

/// Heights of the game's fixed render patches (`graphics.render` in settings.json).
const RENDER_HEIGHTS: [u32; 4] = [1080, 1368, 1872, 2160];

/// The render patch whose height is nearest `target`, the larger one on a tie (as AGS snaps
/// the Render quality label to `render_sizes`).
fn nearest_render(target: u32) -> String {
    let height = RENDER_HEIGHTS
        .into_iter()
        .rev()
        .min_by_key(|h| h.abs_diff(target))
        .unwrap();
    format!("{height}p")
}

/// `graphics.render` and `graphics.dlss` from the sheet §2.8 upscaling keys (decision 0052).
/// A host that sends no `perf.upscaling` gets the values saved for the old page items, else
/// 100% of the output height without DLSS.
fn upscaling(nvidia: bool, standard: &Value, values: &Value, output_height: u32) -> (String, bool) {
    if let Some(on) = standard["perf.upscaling"].as_bool() {
        // DLSS is the only upscaler declared, so anything but "none" (or a missing key) means DLSS.
        let on = on && nvidia && standard["perf.upscaler"] != "none";
        let percent = if on {
            standard["perf.render_scale"]
                .as_u64()
                .filter(|p| (1..=100).contains(p))
                .unwrap_or(63) as u32
        } else {
            100
        };
        let target = u64::from(output_height) * u64::from(percent) / 100;
        return (nearest_render(target.min(u64::from(u32::MAX)) as u32), on);
    }
    let render = values["graphics.render"]
        .as_str()
        .filter(|value| RENDER_HEIGHTS.iter().any(|h| format!("{h}p") == *value))
        .map_or_else(|| nearest_render(output_height), str::to_owned);
    (
        render,
        nvidia && values["graphics.dlss"].as_bool() == Some(true),
    )
}

fn resolution(value: &str) -> Option<(u32, u32)> {
    let (width, height) = value.split_once('x')?;
    let dimension = |s: &str| {
        if !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        s.parse::<u32>().ok().filter(|n| (2..=8192).contains(n))
    };
    Some((dimension(width)?, dimension(height)?))
}

pub fn plan(l: &Launch, params: &Value) -> Result<Value, String> {
    let standard = &params["standard"];
    let values = &params["values"];
    let fps = if standard["display.fps"]
        .as_u64()
        .is_some_and(|fps| fps >= 60)
    {
        60
    } else {
        30
    };
    let low_latency = match standard["perf.low_latency"].as_str() {
        Some("off") => "off",
        _ => "on",
    };
    // FSR 3 frame generation doubles the presented rate (60 -> 120, 30 -> 60); Kyty skips it under vsync
    // when the display refreshes slower than that, and turns low latency off while it is requested.
    let frame_generation = match standard["perf.frame_generation"].as_str() {
        Some("2x" | "3x" | "4x") => "2x",
        _ => "off",
    };
    let (width, height) = standard["display.resolution"]
        .as_str()
        .and_then(resolution)
        .or(l.screen)
        .unwrap_or((3840, 2160));
    let fullscreen = standard["fullscreen"].as_bool().unwrap_or(true);
    let (render, dlss) = upscaling(l.nvidia, standard, values, height);
    let mut chosen = BTreeMap::from([
        ("display.fps".to_owned(), fps.to_string()),
        ("perf.low_latency".to_owned(), low_latency.to_owned()),
        (
            "perf.frame_generation".to_owned(),
            frame_generation.to_owned(),
        ),
        ("fullscreen".to_owned(), fullscreen.to_string()),
        ("graphics.render".to_owned(), render),
        ("graphics.dlss".to_owned(), dlss.to_string()),
    ]);
    for (key, setting) in &l.map.settings {
        if chosen.contains_key(key) {
            continue;
        }
        let value = match l.defaults.get(key) {
            Some(Value::String(default)) => values[key]
                .as_str()
                .filter(|value| setting.values.contains_key(*value))
                .unwrap_or(default)
                .to_owned(),
            Some(Value::Bool(default)) => values[key].as_bool().unwrap_or(*default).to_string(),
            None if key == "graphics.ultrawide" => "false".to_owned(),
            _ => return Err(DAMAGED_INSTALLATION.into()),
        };
        chosen.insert(key.clone(), value);
    }
    let advanced_on = values["advanced.patches"]
        .as_array()
        .filter(|ids| ids.iter().all(Value::is_string))
        .or_else(|| l.defaults["advanced.patches"].as_array())
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|id| l.available.contains(*id))
        .map(str::to_owned)
        .collect();
    let mut selection = l
        .map
        .select(&chosen, &advanced_on)
        .map_err(|_| DAMAGED_INSTALLATION)?;
    let (enable, skip) = l.map.patch_env(&selection);

    let haptics = values["controller.haptics"]
        .as_u64()
        .filter(|value| (100..=400).contains(value))
        .unwrap_or_else(|| l.defaults["controller.haptics"].as_u64().unwrap());
    let language = standard["language"]
        .as_str()
        .and_then(language_index)
        .unwrap_or(1);

    let run = dump::run_folder(l.data, &l.entry.version);
    let run_dir = run.to_str().ok_or(DAMAGED_INSTALLATION)?;
    let install = l.install.to_str().ok_or(DAMAGED_INSTALLATION)?;
    let expand = |value: &str| {
        value
            .split("{run}")
            .map(|part| part.replace("{install}", install))
            .collect::<Vec<_>>()
            .join(run_dir)
    };
    let mut env: BTreeMap<_, _> = l
        .map
        .fixed_env
        .iter()
        .map(|(key, value)| (key.clone(), expand(value)))
        .collect();
    for preset in &l.map.presets {
        env.extend(read_preset(Path::new(&expand(preset)))?);
    }
    // The ultrawide profile overrides the requested output dimensions.
    let (wide_width, wide_height) = if chosen
        .get("graphics.ultrawide")
        .is_some_and(|value| value == "true")
    {
        (
            selection.env.remove("ASTROPORT_WIDTH"),
            selection.env.remove("ASTROPORT_HEIGHT"),
        )
    } else {
        (None, None)
    };
    env.extend(selection.env);
    env.extend([
        (
            "ASTROPORT_WIDTH".into(),
            wide_width.unwrap_or_else(|| width.to_string()),
        ),
        (
            "ASTROPORT_HEIGHT".into(),
            wide_height.unwrap_or_else(|| height.to_string()),
        ),
        (
            "APS5_PAD_VIBRATION_GAIN".into(),
            (haptics as f64 / 100.0).to_string(),
        ),
        ("ANYPS5_LANGUAGE".into(), language.to_string()),
    ]);
    env.extend([
        (
            "ASTRO_PATCHES".into(),
            format!("{install}/patches/{}", l.entry.patches),
        ),
        ("ASTRO_PATCH_ENABLE".into(), enable),
        ("ASTRO_PATCH_SKIP".into(), skip),
    ]);
    env.remove("LD_PRELOAD");
    env.remove("LD_LIBRARY_PATH");

    Ok(json!({
        "argv": [run.join("AstroBot")],
        "env": env,
        "cwd": run,
        "writable": [run, l.data.join("saves")],
        "native_frames": true,
        "controllers": ["dualsense"],
        "applied": {
            "display.fps": fps,
            "display.resolution": format!("{}x{}", env["ASTROPORT_WIDTH"], env["ASTROPORT_HEIGHT"])
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upscaling_maps_to_render_patches() {
        let on = |p: u64| json!({"perf.upscaling": true, "perf.upscaler": "dlss", "perf.render_scale": p});
        let off =
            json!({"perf.upscaling": false, "perf.upscaler": "none", "perf.render_scale": 100});
        let none = json!({});
        // 4K output: 50% -> 1080, 63% -> 1361 -> 1368, 87% -> 1879 -> 1872, off -> 2160.
        assert_eq!(
            upscaling(true, &on(50), &none, 2160),
            ("1080p".into(), true)
        );
        assert_eq!(
            upscaling(true, &on(63), &none, 2160),
            ("1368p".into(), true)
        );
        assert_eq!(
            upscaling(true, &on(87), &none, 2160),
            ("1872p".into(), true)
        );
        assert_eq!(upscaling(true, &off, &none, 2160), ("2160p".into(), false));
        // 1440p output, off: nearest patch to 1440 is 1368.
        assert_eq!(upscaling(true, &off, &none, 1440), ("1368p".into(), false));
        // No DLSS without NVIDIA, even if a host sends it on.
        assert_eq!(
            upscaling(false, &on(50), &none, 2160),
            ("2160p".into(), false)
        );
        // Old hosts: the saved page values carry over, else 100% without DLSS.
        let old = json!({"graphics.render": "1872p", "graphics.dlss": true});
        assert_eq!(upscaling(true, &none, &old, 2160), ("1872p".into(), true));
        assert_eq!(upscaling(false, &none, &old, 2160), ("1872p".into(), false));
        assert_eq!(
            upscaling(true, &none, &json!({"graphics.render": "4320p"}), 1080),
            ("1080p".into(), false)
        );
        // Edge cases: missing upscaler means DLSS, "none" means off, bad percents fall back to 63.
        let no_upscaler = json!({"perf.upscaling": true, "perf.render_scale": 50});
        assert_eq!(
            upscaling(true, &no_upscaler, &none, 2160),
            ("1080p".into(), true)
        );
        let upscaler_none = json!({"perf.upscaling": true, "perf.upscaler": "none"});
        assert_eq!(
            upscaling(true, &upscaler_none, &none, 2160),
            ("2160p".into(), false)
        );
        assert_eq!(upscaling(true, &on(0), &none, 2160), ("1368p".into(), true));
        assert_eq!(
            upscaling(true, &on(101), &none, 2160),
            ("1368p".into(), true)
        );
        assert_eq!(upscaling(true, &on(87), &none, 2), ("1080p".into(), true));
        assert_eq!(upscaling(true, &off, &none, 8192), ("2160p".into(), false));
        assert_eq!(
            upscaling(true, &off, &none, u32::MAX),
            ("2160p".into(), false)
        );
        // Ties go to the larger patch: 1224 is halfway between 1080 and 1368.
        assert_eq!(nearest_render(1224), "1368p");
    }

    #[test]
    fn languages() {
        assert_eq!(language_index("english"), Some(1));
        assert_eq!(language_index("Koreana"), Some(9));
        assert_eq!(language_index("-"), None);
    }

    #[test]
    fn resolution_accepts_only_bounded_dimensions() {
        assert_eq!(resolution("2x8192"), Some((2, 8192)));
        for invalid in ["1x1080", "1920x8193", "1920x", "1920x1080x2", "+2x1080"] {
            assert_eq!(resolution(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn presets_keep_only_non_metadata_strings() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            file.path(),
            br#"{"_note":"ignored","KYTY_HANG_WATCHDOG":"auto","number":12,"enabled":true}"#,
        )
        .unwrap();
        assert_eq!(
            read_preset(file.path()).unwrap(),
            std::collections::BTreeMap::from([("KYTY_HANG_WATCHDOG".into(), "auto".into())])
        );
    }
}
