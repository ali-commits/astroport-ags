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
    let fullscreen = standard["fullscreen"].as_bool().unwrap_or(true);
    let mut chosen = BTreeMap::from([
        ("display.fps".to_owned(), fps.to_string()),
        ("perf.low_latency".to_owned(), low_latency.to_owned()),
        ("fullscreen".to_owned(), fullscreen.to_string()),
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
            Some(Value::Bool(default)) if key != "graphics.dlss" || *default => {
                values[key].as_bool().unwrap_or(*default).to_string()
            }
            Some(Value::Bool(_)) => "false".to_owned(),
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

    let (width, height) = standard["display.resolution"]
        .as_str()
        .and_then(resolution)
        .or(l.screen)
        .unwrap_or((3840, 2160));
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
            selection.env.remove("KYTY_GPU_WIDTH"),
            selection.env.remove("KYTY_GPU_HEIGHT"),
        )
    } else {
        (None, None)
    };
    env.extend(selection.env);
    env.extend([
        (
            "KYTY_GPU_WIDTH".into(),
            wide_width.unwrap_or_else(|| width.to_string()),
        ),
        (
            "KYTY_GPU_HEIGHT".into(),
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
        "native_frames": false,
        "controllers": ["dualsense"],
        "applied": {
            "display.fps": fps,
            "display.resolution": format!("{}x{}", env["KYTY_GPU_WIDTH"], env["KYTY_GPU_HEIGHT"])
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

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
