use crate::{device::Defaults, dump::Detected, settings::SettingsMap, sheet, values::Store};
use serde_json::{json, Value};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
};

pub const GAME_ID: &str = "PPSA21567";

pub fn extension_page(_store: &Store, detected: &Detected) -> Value {
    let status: Cow<'_, str> = match detected {
        Detected::NotSet => "Choose the folder of your Astro Bot dump.".into(),
        Detected::NotFound => "No Astro Bot dump in this folder.".into(),
        Detected::Supported { entry, .. } => format!("Found version {}.", entry.version).into(),
        Detected::Unsupported { message, .. } => message.as_str().into(),
    };
    json!({
        "sections": [{
            "id": "astroport.dump",
            "surface": "panel",
            "title": "Game dump",
            "items": [
                {
                    "id": "dump_folder",
                    "kind": "path",
                    "pick": "folder",
                    "nullable": true,
                    "null_label": "Not set",
                    "scope": "machine",
                    "label": "Astro Bot dump folder",
                    "help": "The folder with eboot.bin and sce_sys, or the folder above it."
                },
                {
                    "id": "dump.status",
                    "kind": "info",
                    "label": "Status",
                    "text": status_text(&status)
                }
            ]
        }]
    })
}

fn status_text(status: &str) -> String {
    status
        .chars()
        .take(200)
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

pub fn item_defaults(
    map: &SettingsMap,
    available: &BTreeSet<String>,
    d: &Defaults,
) -> BTreeMap<String, Value> {
    let advanced: Vec<_> = map
        .advanced
        .iter()
        .filter(|entry| entry.default && available.contains(&entry.id))
        .map(|entry| &entry.id)
        .collect();
    let mut defaults = BTreeMap::from([
        ("graphics.render".into(), json!(d.render)),
        ("graphics.lighting".into(), json!(d.lighting)),
        ("graphics.detail".into(), json!("medium")),
        ("graphics.textures".into(), json!("soft")),
        ("graphics.dlss".into(), json!(d.dlss)),
        ("controller.haptics".into(), json!(250)),
        ("controller.rumble".into(), json!(true)),
        ("game.skip_intro".into(), json!(true)),
        ("advanced.patches".into(), json!(advanced)),
    ]);
    if available.contains("ultrawide") {
        defaults.insert("graphics.ultrawide".into(), json!(false));
    }
    defaults
}

fn panel(id: &str, title: &str, items: Vec<Value>) -> Value {
    json!({"id": id, "surface": "panel", "tab": "source", "title": title, "items": items})
}

fn choice(id: &str, label: &str, options: &[(&str, &str)], segmented: bool) -> Value {
    let options: Vec<_> = options
        .iter()
        .map(|(value, label)| json!({"value": value, "label": label}))
        .collect();
    let mut item = json!({"id": id, "kind": "choice", "label": label, "options": options});
    if segmented {
        item["presentation"] = json!("segmented");
    }
    item
}

pub fn game_page(
    detected: &Detected,
    is_built: bool,
    map: &SettingsMap,
    available: &BTreeSet<String>,
    d: &Defaults,
) -> Value {
    match detected {
        Detected::Unsupported { message, .. } => {
            let status = panel(
                "astroport.status",
                "Version",
                vec![json!({
                    "id": "status", "kind": "info", "label": "Version", "text": status_text(message)
                })],
            );
            return json!({"sections": [status]});
        }
        Detected::NotSet | Detected::NotFound => {
            unreachable!("a game page requires a detected game")
        }
        Detected::Supported { .. } => {}
    }

    let defaults = item_defaults(map, available, d);
    let value_item = |mut item: Value| {
        let id = item["id"].as_str().unwrap();
        item["default"] = defaults[id].clone();
        item["scope"] = json!("game_on_device");
        if item.get("restart").is_none() {
            item["restart"] = json!("next_launch");
        }
        item
    };
    let mut sections = Vec::new();
    if !is_built {
        sections.push(json!({
            "id": "astroport.build",
            "surface": "hero",
            "items": [{"id": "build", "kind": "button", "label": "Build game files", "job": true}]
        }));
    }

    let mut graphics = vec![
        choice(
            "graphics.render",
            "Render resolution",
            &[
                ("1080p", "1080p"),
                ("1368p", "1368p"),
                ("1872p", "1872p"),
                ("2160p", "2160p"),
            ],
            false,
        ),
        choice(
            "graphics.lighting",
            "Lighting",
            &[("fast", "Fast"), ("original", "Original")],
            true,
        ),
        choice(
            "graphics.detail",
            "Detail",
            &[("high", "High"), ("medium", "Medium"), ("low", "Low")],
            true,
        ),
        choice(
            "graphics.textures",
            "Texture sharpness",
            &[
                ("sharp", "Sharp"),
                ("balanced", "Balanced"),
                ("soft", "Soft"),
            ],
            true,
        ),
    ];
    if available.contains("ultrawide") {
        graphics.push(json!({
            "id": "graphics.ultrawide", "kind": "switch", "label": "Ultrawide (21:9)",
            "help": "Renders at 3440x1440."
        }));
    }
    if d.dlss {
        graphics.push(json!({
            "id": "graphics.dlss", "kind": "switch", "label": "DLSS", "help": "NVIDIA upscaling."
        }));
    }
    sections.push(panel(
        "astroport.graphics",
        "Graphics",
        graphics.into_iter().map(value_item).collect(),
    ));
    sections.push(panel(
        "astroport.controller",
        "Controller",
        vec![value_item(json!({
            "id": "controller.rumble", "kind": "switch", "label": "Rumble"
        }))],
    ));
    sections.push(panel(
        "astroport.game",
        "Game",
        vec![value_item(json!({
            "id": "game.skip_intro", "kind": "switch", "label": "Skip the studio intro"
        }))],
    ));

    let entries: Vec<_> = map
        .advanced
        .iter()
        .filter(|entry| available.contains(&entry.id))
        .map(|entry| json!({"id": entry.id, "label": entry.label}))
        .collect();
    sections.push(panel(
        "astroport.advanced",
        "Advanced",
        vec![value_item(json!({
            "id": "advanced.patches", "kind": "toggle_list", "label": "Patches", "entries": entries
        }))],
    ));
    sections.push(json!({
        "id": "astroport.overlay",
        "surface": "overlay",
        "title": "Controller",
        "items": [value_item(json!({
            "id": "controller.haptics", "kind": "slider", "label": "Haptic strength",
            "restart": "live",
            "min": 100, "max": 400, "step": 25, "unit": "%",
            "stops": [{"value": 100, "label": "As sent"}]
        }))]
    }));
    json!({"sections": sections, "standard": sheet::standard_support()})
}

pub fn check_limits(page: &Value) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    let sections = page["sections"]
        .as_array()
        .ok_or("Missing page sections.")?;
    for section in sections {
        check_id(&section["id"], &mut ids)?;
        if section.get("title").is_some()
            || !matches!(section["surface"].as_str(), Some("hero" | "menu"))
        {
            check_text(&section["title"], "section title", 1, 40)?;
        }
        for item in section["items"]
            .as_array()
            .ok_or("Missing section items.")?
        {
            check_id(&item["id"], &mut ids)?;
            check_text(&item["label"], "item label", 1, 80)?;
            if let Some(help) = item.get("help") {
                check_text(help, "help text", 0, 200)?;
            }
            if item["kind"] == "info" {
                check_text(&item["text"], "info text", 1, 200)?;
            }
            for (field, max_label) in [("options", 80), ("entries", 80), ("stops", 32)] {
                if let Some(entries) = item[field].as_array() {
                    for entry in entries {
                        check_text(&entry["label"], "entry label", 1, max_label)?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn check_id<'a>(node: &'a Value, ids: &mut BTreeSet<&'a str>) -> Result<(), String> {
    let id = node.as_str().unwrap_or_default();
    if id.is_empty()
        || id.len() > 64
        || id.starts_with("ags.")
        || !id.as_bytes()[0].is_ascii_alphanumeric()
        || !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
        || !ids.insert(id)
    {
        return Err(format!("Invalid or repeated page id {id:?}."));
    }
    Ok(())
}

fn check_text(value: &Value, field: &str, min: usize, max: usize) -> Result<(), String> {
    let text = value.as_str().ok_or_else(|| format!("Missing {field}."))?;
    if !(min..=max).contains(&text.chars().count())
        || text.chars().any(char::is_control)
        || (min > 0 && text.trim().is_empty())
    {
        return Err(format!("Invalid {field}."));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{device::Defaults, dump::VersionEntry, settings};
    use std::path::{Path, PathBuf};

    #[test]
    fn dlss_only_on_nvidia() {
        let patches = Path::new(env!("CARGO_MANIFEST_DIR")).join("../patches");
        let map = settings::load(&patches.join("settings.json")).unwrap();
        let patch_file = "PPSA21567-1018.json";
        let available = map.available(&patches.join(patch_file)).unwrap();
        let detected = Detected::Supported {
            dump: PathBuf::new(),
            entry: VersionEntry {
                version: "1.018".into(),
                content_version: "01.018.000".into(),
                eboot_sha256: None,
                patches: patch_file.into(),
            },
            eboot_sha256: String::new(),
        };
        for dlss in [true, false] {
            let device = Defaults {
                render: "1080p",
                lighting: "fast",
                dlss,
            };
            let page = game_page(&detected, false, &map, &available, &device);
            check_limits(&page).unwrap();
            let graphics = page["sections"][1]["items"].as_array().unwrap();
            let dlss_item = graphics.iter().find(|item| item["id"] == "graphics.dlss");
            assert_eq!(
                dlss_item.map(|item| &item["default"]),
                dlss.then_some(&json!(true))
            );
            let defaults = item_defaults(&map, &available, &device);
            assert_eq!(defaults["graphics.dlss"], dlss);
            assert!(!defaults.contains_key("graphics.ultrawide"));
        }
    }

    #[test]
    fn check_limits_rejects_bad_ids_and_text() {
        let valid = json!({"sections": [{
            "id": "section", "surface": "panel", "title": "é".repeat(40),
            "items": [{"id": "item", "kind": "info", "label": "é".repeat(80), "text": "é".repeat(200)}]
        }]});
        check_limits(&valid).unwrap();

        for id in [
            String::from(".item"),
            String::from("Item"),
            "a".repeat(65),
            String::from("section"),
        ] {
            let mut page = valid.clone();
            page["sections"][0]["items"][0]["id"] = json!(id);
            assert!(check_limits(&page).is_err(), "{id}");
        }

        let mut repeated_item = valid.clone();
        repeated_item["sections"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "id": "another", "surface": "overlay", "title": "Another",
                "items": [valid["sections"][0]["items"][0].clone()]
            }));
        assert!(check_limits(&repeated_item).is_err());

        let mut page = valid.clone();
        page["sections"][0]["title"] = json!("é".repeat(41));
        assert!(check_limits(&page).is_err());

        for (field, value) in [
            ("label", String::new()),
            ("label", "é".repeat(81)),
            ("help", "é".repeat(201)),
            ("text", "é".repeat(201)),
            ("text", String::from("First\nSecond")),
        ] {
            let mut page = valid.clone();
            page["sections"][0]["items"][0][field] = json!(value);
            assert!(check_limits(&page).is_err(), "{field}");
        }
    }
}
