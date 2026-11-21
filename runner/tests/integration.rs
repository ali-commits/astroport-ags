#[allow(dead_code)]
mod common;

use astroport_run::build::{needs_to_intel, relink};
use astroport_run::unwrap::unwrap_file;
use astroport_run::version::detect;
use common::temp_dir;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn input_path(name: &str) -> Option<PathBuf> {
    match std::env::var_os(name).filter(|value| !value.is_empty()) {
        Some(path) => Some(PathBuf::from(path)),
        None => {
            eprintln!("{name} not set, skipped");
            None
        }
    }
}

fn dump_files(dump: &Path) -> Vec<(PathBuf, PathBuf)> {
    let mut files = vec![(PathBuf::from("eboot.bin"), PathBuf::from("eboot.elf"))];
    for entry in fs::read_dir(dump.join("sce_module")).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "prx") && path.is_file() {
            let module = Path::new("sce_module").join(entry.file_name());
            files.push((module.clone(), module));
        }
    }
    files[1..].sort_unstable();
    files
}

fn assert_files_match(actual: &Path, expected: &Path, name: &Path) {
    let read =
        |path: &Path| fs::read(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    assert!(read(actual) == read(expected), "{} differs", name.display());
}

#[test]
#[ignore]
fn real_dump_is_1019() {
    let Some(dump) = input_path("ASTROPORT_IT_DUMP") else {
        return;
    };
    let Some(versions) = input_path("ASTROPORT_IT_VERSIONS") else {
        return;
    };
    assert_eq!(detect(&dump, &versions).unwrap().version, "1.019");
}

#[test]
#[ignore]
fn real_unwrap_matches_python() {
    let Some(dump) = input_path("ASTROPORT_IT_DUMP") else {
        return;
    };
    let scratch = Scratch(temp_dir("real-unwrap-python"));
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../engines/anyps5/tools/unwrap_self.py");

    for (input, _) in dump_files(&dump) {
        let source = dump.join(&input);
        let rust = scratch.0.join("rust.elf");
        let python = scratch.0.join("py.elf");
        unwrap_file(&source, &rust).unwrap();
        let output = Command::new("timeout")
            .args(["--foreground", "-k", "10", "300", "python3", "-I"])
            .arg(&script)
            .arg(&source)
            .arg(&python)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}: {}",
            input.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_files_match(&rust, &python, &input);
    }
}

#[test]
#[ignore]
fn real_unwrap_matches_reference() {
    let Some(dump) = input_path("ASTROPORT_IT_DUMP") else {
        return;
    };
    let Some(reference) = input_path("ASTROPORT_IT_UNWRAPPED") else {
        return;
    };
    let scratch = Scratch(temp_dir("real-unwrap-reference"));

    for (input, expected) in dump_files(&dump) {
        let rust = scratch.0.join("rust.elf");
        unwrap_file(&dump.join(&input), &rust).unwrap();
        assert_files_match(&rust, &reference.join(expected), &input);
    }
}

fn reference_listing(root: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    let mut pending = vec![root.to_path_buf()];
    let mut listing = Vec::new();
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            if metadata.is_dir() {
                pending.push(path.clone());
            }
            listing.push((
                path.strip_prefix(root).unwrap().to_path_buf(),
                metadata.len(),
                metadata.modified().unwrap(),
            ));
        }
    }
    listing.sort_unstable();
    listing
}

#[test]
#[ignore]
fn real_relink_writes_the_expected_files() {
    let Some(reference) = input_path("ASTROPORT_IT_UNWRAPPED") else {
        return;
    };
    let Some(relinker) = input_path("ASTROPORT_IT_RELINKER") else {
        return;
    };
    let before = reference_listing(&reference);
    let scratch = Scratch(temp_dir("real-relink"));
    let out = scratch.0.join("out");
    fs::create_dir(&out).unwrap();

    let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap();
    let log = scratch.0.join("relink.log");
    let result = relink(
        &relinker,
        &reference.join("eboot.elf"),
        &out,
        needs_to_intel(&cpuinfo),
        &log,
    );
    assert_eq!(
        reference_listing(&reference),
        before,
        "reference listing changed"
    );
    result.unwrap_or_else(|error| {
        panic!(
            "{}\n{}",
            error.message,
            fs::read_to_string(&log).unwrap_or_default()
        )
    });
    for path in [
        "AstroBot",
        "app0/sce_module/libc.prx.guest.prx",
        "app0/sce_module/libSceNpCppWebApi.prx.guest.prx",
        "AstroBot.registry.json",
        "AstroBot.libc.prx.guest.prx.registry.json",
    ] {
        assert!(out.join(path).is_file(), "missing relinker output {path}");
    }
}
