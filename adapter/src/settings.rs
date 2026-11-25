use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[derive(Deserialize)]
pub struct SettingsMap {
    pub format: u32,
    pub fixed_env: BTreeMap<String, String>,
    pub presets: Vec<String>,
    pub patches: BTreeMap<String, String>,
    pub settings: BTreeMap<String, Setting>,
    pub advanced: Vec<Advanced>,
}

#[derive(Deserialize)]
pub struct Setting {
    pub values: BTreeMap<String, Choice>,
}

#[derive(Deserialize, Default)]
pub struct Choice {
    #[serde(default)]
    pub on: Vec<String>,
    #[serde(default)]
    pub off: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Deserialize)]
pub struct Advanced {
    pub id: String,
    pub label: String,
    pub default: bool,
}

pub struct Selection {
    pub on: BTreeSet<String>,
    pub off: BTreeSet<String>,
    pub env: BTreeMap<String, String>,
}

pub fn load(path: &Path) -> Result<SettingsMap, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
    let map: SettingsMap = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Cannot parse {}: {error}", path.display()))?;
    map.check()?;
    Ok(map)
}

impl SettingsMap {
    pub fn check(&self) -> Result<(), String> {
        if self.format != 1 {
            return Err(format!("Unsupported settings format {}.", self.format));
        }
        for (id, pattern) in &self.patches {
            if pattern.is_empty()
                || pattern.trim() != pattern
                || pattern.contains(',')
                || pattern.to_lowercase() != *pattern
            {
                return Err(format!("Patch {id} has an invalid match string."));
            }
        }

        let mut owned = BTreeSet::new();
        let mut claim = |id| {
            if !self.patches.contains_key(id) {
                return Err(format!("Unknown patch {id}."));
            }
            if !owned.insert(id) {
                return Err(format!("Patch {id} has more than one owner."));
            }
            Ok(())
        };

        for (key, setting) in &self.settings {
            let mut group = None;
            for (value, choice) in &setting.values {
                let on: BTreeSet<_> = choice.on.iter().collect();
                let off: BTreeSet<_> = choice.off.iter().collect();
                if !on.is_disjoint(&off) {
                    return Err(format!(
                        "Setting {key} value {value} turns a patch both on and off."
                    ));
                }
                let members: BTreeSet<_> = on.into_iter().chain(off).collect();
                if group.as_ref().is_some_and(|group| *group != members) {
                    return Err(format!("Setting {key} has inconsistent patch groups."));
                }
                group = Some(members);
            }
            for id in group.ok_or_else(|| format!("Setting {key} has no values."))? {
                claim(id)?;
            }
        }
        for entry in &self.advanced {
            claim(&entry.id)?;
        }
        if let Some(id) = self.patches.keys().find(|id| !owned.contains(id)) {
            return Err(format!("Patch {id} has no owner."));
        }
        Ok(())
    }

    pub fn available(&self, patch_file: &Path) -> Result<BTreeSet<String>, String> {
        #[derive(Deserialize)]
        struct PatchFile {
            mods: Vec<PatchMod>,
        }

        #[derive(Deserialize)]
        struct PatchMod {
            name: String,
        }

        let bytes = fs::read(patch_file)
            .map_err(|error| format!("Cannot read {}: {error}", patch_file.display()))?;
        let file: PatchFile = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Cannot parse {}: {error}", patch_file.display()))?;
        let names: Vec<_> = file
            .mods
            .into_iter()
            .map(|entry| entry.name.to_lowercase())
            .collect();
        Ok(self
            .patches
            .iter()
            .filter(|(_, pattern)| names.iter().any(|name| name.contains(pattern.as_str())))
            .map(|(id, _)| id.clone())
            .collect())
    }

    pub fn select(
        &self,
        chosen: &BTreeMap<String, String>,
        advanced_on: &BTreeSet<String>,
    ) -> Result<Selection, String> {
        let mut selection = Selection {
            on: BTreeSet::new(),
            off: BTreeSet::new(),
            env: BTreeMap::new(),
        };
        for (key, value) in chosen {
            let setting = self
                .settings
                .get(key)
                .ok_or_else(|| format!("Unknown setting {key}."))?;
            let choice = setting
                .values
                .get(value)
                .ok_or_else(|| format!("Unknown value {value} for setting {key}."))?;
            selection.on.extend(choice.on.iter().cloned());
            selection.off.extend(choice.off.iter().cloned());
            selection.env.extend(choice.env.clone());
        }
        if let Some(key) = self.settings.keys().find(|key| !chosen.contains_key(*key)) {
            return Err(format!("No value chosen for setting {key}."));
        }
        for entry in &self.advanced {
            let target = if advanced_on.contains(&entry.id) {
                &mut selection.on
            } else {
                &mut selection.off
            };
            target.insert(entry.id.clone());
        }
        Ok(selection)
    }

    pub fn patch_env(&self, s: &Selection) -> (String, String) {
        let join = |ids: &BTreeSet<String>| {
            let mut patterns: Vec<_> = ids.iter().map(|id| self.patches[id].as_str()).collect();
            patterns.sort_unstable();
            patterns.join(",")
        };
        (join(&s.on), join(&s.off))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo_path(path: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(path)
    }

    fn repo() -> SettingsMap {
        load(&repo_path("patches/settings.json")).unwrap()
    }

    fn chosen() -> BTreeMap<String, String> {
        [
            ("display.fps", "60"),
            ("graphics.render", "1080p"),
            ("graphics.lighting", "fast"),
            ("graphics.detail", "low"),
            ("graphics.textures", "soft"),
            ("graphics.ultrawide", "false"),
            ("game.skip_intro", "true"),
            ("controller.rumble", "true"),
            ("perf.low_latency", "on"),
            ("fullscreen", "true"),
            ("graphics.dlss", "false"),
        ]
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect()
    }

    #[test]
    fn repo_map_passes_check() {
        repo().check().unwrap();
    }

    #[test]
    fn every_mod_owned_once_in_both_versions() {
        let map = repo();
        for version in ["1018", "1019"] {
            let bytes = fs::read(repo_path(&format!("patches/PPSA21567-{version}.json"))).unwrap();
            let file: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let mut matched = BTreeSet::new();
            for entry in file["mods"].as_array().unwrap() {
                let name = entry["name"].as_str().unwrap().to_lowercase();
                let ids: Vec<_> = map
                    .patches
                    .iter()
                    .filter(|(_, pattern)| name.contains(pattern.as_str()))
                    .map(|(id, _)| id.as_str())
                    .collect();
                assert_eq!(ids.len(), 1, "{version}: {name} matched {ids:?}");
                assert!(
                    matched.insert(ids[0]),
                    "{version}: repeated match for {}",
                    ids[0]
                );
            }
            let expected = map
                .patches
                .keys()
                .map(String::as_str)
                .filter(|&id| version == "1019" || id != "ultrawide")
                .collect();
            assert_eq!(matched, expected, "{version}");
        }
    }

    #[test]
    fn ultrawide_only_in_1019() {
        let map = repo();
        assert!(!map
            .available(&repo_path("patches/PPSA21567-1018.json"))
            .unwrap()
            .contains("ultrawide"));
        assert!(map
            .available(&repo_path("patches/PPSA21567-1019.json"))
            .unwrap()
            .contains("ultrawide"));
    }

    #[test]
    fn one_lod_and_one_cap_at_a_time() {
        let map = repo();
        let selection = map
            .select(&chosen(), &BTreeSet::from(["pad.r1x".into()]))
            .unwrap();
        assert!(selection.on.contains("cap.60"));
        assert!(selection.off.contains("cap.30"));
        assert!(selection.off.contains("physics.rope"));
        assert!(selection.on.contains("lod.quarter"));
        assert!(selection.off.contains("lod.half"));
        assert!(selection.on.contains("pad.r1x"));
        assert!(selection.off.contains("video.speed"));
        assert!(selection.on.is_disjoint(&selection.off));
        assert_eq!(selection.on.len() + selection.off.len(), map.patches.len());
        assert_eq!(selection.env["ASTROPORT_FPS_LIMIT"], "60");
        assert_eq!(selection.env["ASTROPORT_LOW_LATENCY"], "1");
    }

    #[test]
    fn patch_env_joins_match_strings() {
        let map = repo();
        let selection = map.select(&chosen(), &BTreeSet::new()).unwrap();
        let (enable, skip) = map.patch_env(&selection);
        for (joined, cap, count) in [
            (enable, "precise frame cap 60 fps", selection.on.len()),
            (skip, "precise frame cap 30 fps", selection.off.len()),
        ] {
            let entries: Vec<_> = joined.split(',').collect();
            assert!(entries.contains(&cap));
            assert_eq!(entries.len(), count);
            assert!(entries.windows(2).all(|pair| pair[0] < pair[1]));
        }
    }

    #[test]
    fn check_refuses_overlap_and_uneven_groups() {
        let mut map = repo();
        for choice in map
            .settings
            .get_mut("graphics.detail")
            .unwrap()
            .values
            .values_mut()
        {
            choice.off.push("cap.60".into());
        }
        assert!(map.check().is_err());

        let mut map = repo();
        map.settings
            .get_mut("graphics.detail")
            .unwrap()
            .values
            .get_mut("low")
            .unwrap()
            .off
            .clear();
        assert!(map.check().is_err());

        let mut map = repo();
        map.advanced[0].id = "gone.patch".into();
        assert!(map.check().is_err());

        let mut map = repo();
        map.settings
            .get_mut("graphics.detail")
            .unwrap()
            .values
            .get_mut("low")
            .unwrap()
            .off
            .push("lod.quarter".into());
        assert!(map.check().is_err());
    }

    #[test]
    fn check_refuses_invalid_match_strings_and_format() {
        for pattern in ["", "Uppercase", "has,comma", " trailing "] {
            let mut map = repo();
            map.patches.insert("cap.30".into(), pattern.into());
            assert!(map.check().is_err(), "{pattern:?}");
        }
        let mut map = repo();
        map.format = 2;
        assert!(map.check().is_err());
    }

    #[test]
    fn select_refuses_unknown_value() {
        let mut chosen = chosen();
        chosen.insert("graphics.render".into(), "4320p".into());
        assert!(repo().select(&chosen, &BTreeSet::new()).is_err());
    }

    #[test]
    fn select_refuses_missing_and_unknown_settings() {
        let mut chosen = chosen();
        chosen.remove("graphics.render");
        let map = repo();
        assert!(map.select(&chosen, &BTreeSet::new()).is_err());
        chosen.insert("graphics.render".into(), "1080p".into());
        chosen.insert("graphics.unknown".into(), "on".into());
        assert!(map.select(&chosen, &BTreeSet::new()).is_err());
    }
}
