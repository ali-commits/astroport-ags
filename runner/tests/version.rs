#[allow(dead_code)]
mod common;

use astroport_run::version::{detect, sha256_file};
use common::{fake_dump, temp_dir, versions_json};
use std::fs;
use std::path::PathBuf;

const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

struct Fixture {
    root: PathBuf,
    dump: PathBuf,
    versions: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = temp_dir("version");
        let dump = root.join("dump");
        let versions = root.join("versions.json");
        versions_json(&versions, Some(ABC_SHA256));
        Self {
            root,
            dump,
            versions,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn version_1019_with_matching_hash() {
    let f = Fixture::new();
    fake_dump(&f.dump, "01.019.000", b"abc", &[]);
    let detected = detect(&f.dump, &f.versions).unwrap();
    assert_eq!(detected.version, "1.019");
    assert_eq!(detected.content_version, "01.019.000");
    assert_eq!(detected.eboot_sha256, ABC_SHA256);
}

#[test]
fn version_1018_matches_by_version_only() {
    let f = Fixture::new();
    fake_dump(&f.dump, "01.018.000", b"other eboot", &[]);
    assert_eq!(detect(&f.dump, &f.versions).unwrap().version, "1.018");
}

#[test]
fn unsupported_version_message() {
    let f = Fixture::new();
    fake_dump(&f.dump, "01.020.000", b"abc", &[]);
    let error = detect(&f.dump, &f.versions).unwrap_err();
    assert_eq!(error.category, "unsupported");
    assert_eq!(
        error.message,
        "Version 1.020 is not supported. AstroPort runs 1.018 and 1.019."
    );
}

#[test]
fn hash_mismatch_is_refused() {
    let f = Fixture::new();
    fake_dump(&f.dump, "01.019.000", b"other eboot", &[]);
    let error = detect(&f.dump, &f.versions).unwrap_err();
    assert_eq!(error.category, "unsupported");
    assert_eq!(
        error.message,
        "eboot.bin does not match version 1.019. AstroPort runs 1.018 and 1.019."
    );
}

#[test]
fn other_title_is_refused() {
    let f = Fixture::new();
    fake_dump(&f.dump, "01.020.000", b"abc", &[]);
    fs::write(
        f.dump.join("sce_sys/param.json"),
        r#"{"titleId":"PPSA01234","contentVersion":"01.020.000"}"#,
    )
    .unwrap();
    let error = detect(&f.dump, &f.versions).unwrap_err();
    assert_eq!(error.category, "integrity");
    assert_eq!(
        error.message,
        format!(
            "{} holds PPSA01234, not Astro Bot (PPSA21567).",
            f.dump.display()
        )
    );
}

#[test]
fn missing_files_are_refused() {
    let f = Fixture::new();
    fs::create_dir(&f.dump).unwrap();
    let error = detect(&f.dump, &f.versions).unwrap_err();
    assert_eq!(error.category, "integrity");
    assert_eq!(
        error.message,
        format!(
            "No Astro Bot game files in {}: eboot.bin and sce_sys/param.json are needed.",
            f.dump.display()
        )
    );
}

#[test]
fn sha256_of_abc() {
    let f = Fixture::new();
    let dump = fake_dump(&f.dump, "01.019.000", b"abc", &[]);
    assert_eq!(sha256_file(&dump.join("eboot.bin")).unwrap(), ABC_SHA256);
}
