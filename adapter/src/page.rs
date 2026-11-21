use crate::{dump::Detected, values::Store};
use serde_json::{json, Value};
use std::borrow::Cow;

pub const GAME_ID: &str = "PPSA21567";

pub fn extension_page(_store: &Store, detected: &Detected) -> Value {
    let status: Cow<'_, str> = match detected {
        Detected::NotSet => "Choose the folder of your Astro Bot dump.".into(),
        Detected::NotFound => "No Astro Bot dump in this folder.".into(),
        Detected::Supported { entry, .. } => format!("Found version {}.", entry.version).into(),
        Detected::Unsupported { message, .. } => message.as_str().into(),
    };
    let text: String = status
        .chars()
        .take(200)
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();

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
                    "text": text
                }
            ]
        }]
    })
}
