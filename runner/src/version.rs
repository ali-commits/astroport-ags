use crate::JobError;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

#[derive(Debug)]
pub struct Detected {
    pub version: String,
    pub content_version: String,
    pub eboot_sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Param {
    title_id: String,
    content_version: String,
}

#[derive(Deserialize)]
struct Versions {
    title_id: String,
    versions: Vec<Version>,
}

#[derive(Deserialize)]
struct Version {
    version: String,
    content_version: String,
    eboot_sha256: Option<String>,
}

pub fn detect(dump: &Path, versions_json: &Path) -> Result<Detected, JobError> {
    let eboot_path = dump.join("eboot.bin");
    let param_path = dump.join("sce_sys/param.json");
    if !eboot_path.is_file() || !param_path.is_file() {
        return Err(JobError {
            category: "integrity",
            message: format!(
                "No Astro Bot game files in {}: eboot.bin and sce_sys/param.json are needed.",
                dump.display()
            ),
        });
    }

    let param: Param =
        serde_json::from_slice(&fs::read(&param_path)?).map_err(|error| JobError {
            category: "integrity",
            message: format!("Invalid {}: {error}", param_path.display()),
        })?;
    let versions: Versions =
        serde_json::from_slice(&fs::read(versions_json)?).map_err(|error| JobError {
            category: "internal",
            message: format!("Invalid {}: {error}", versions_json.display()),
        })?;
    if param.title_id != versions.title_id {
        return Err(JobError {
            category: "integrity",
            message: format!(
                "{} holds {}, not Astro Bot (PPSA21567).",
                dump.display(),
                param.title_id
            ),
        });
    }

    let unsupported = |reason| JobError {
        category: "unsupported",
        message: format!(
            "{reason} AstroPort runs {}.",
            supported_versions(&versions.versions)
        ),
    };
    let entry = versions
        .versions
        .iter()
        .find(|entry| entry.content_version == param.content_version)
        .ok_or_else(|| {
            let version = short_version(&param.content_version);
            unsupported(format!("Version {version} is not supported."))
        })?;
    let eboot_sha256 = sha256_file(&eboot_path)?;
    if let Some(expected) = &entry.eboot_sha256 {
        if expected != &eboot_sha256 {
            return Err(unsupported(format!(
                "eboot.bin does not match version {}.",
                entry.version
            )));
        }
    }

    Ok(Detected {
        version: entry.version.clone(),
        content_version: param.content_version,
        eboot_sha256,
    })
}

fn short_version(content_version: &str) -> String {
    let mut parts = content_version.split('.');
    match (parts.next(), parts.next()) {
        (Some(major), Some(minor)) => {
            let major = major.trim_start_matches('0');
            let major = if major.is_empty() { "0" } else { major };
            format!("{major}.{minor}")
        }
        _ => content_version.to_owned(),
    }
}

fn supported_versions(versions: &[Version]) -> String {
    let mut labels: Vec<_> = versions
        .iter()
        .map(|entry| entry.version.as_str())
        .collect();
    labels.sort_unstable();
    match labels.as_slice() {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [first @ .., last] => format!("{} and {last}", first.join(", ")),
    }
}

pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let count = match file.read(&mut buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
