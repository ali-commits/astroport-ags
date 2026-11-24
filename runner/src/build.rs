use crate::layout::{
    find_fonts, install_libs, link_app0, link_fonts, link_saves, place_outputs, FORWARDERS, RENDERER_LIBS,
};
use crate::progress::{Progress, STEPS};
use crate::unwrap::unwrap_file;
use crate::version::{detect, sha256_file};
use crate::JobError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};
use std::ffi::OsString;
use std::fs::{self, File};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct BuildArgs {
    pub dump: PathBuf,
    pub data: PathBuf,
    pub install: PathBuf,
}

#[derive(Serialize, Deserialize)]
pub struct Record {
    pub format: u32,
    pub version: String,
    pub content_version: String,
    pub eboot_sha256: String,
    pub release: String,
    pub inputs_sha256: String,
    pub to_intel: bool,
    pub dump: String,
}

pub fn run_dir(data: &Path, version: &str) -> PathBuf {
    data.join("generated").join(format!("run-{version}"))
}

pub fn inputs_sha256(install: &Path) -> io::Result<String> {
    let mut paths = vec![PathBuf::from("anyps5/relinker"), PathBuf::from(EXPORTS)];
    paths.extend(FORWARDERS.into_iter().chain(RENDERER_LIBS).map(|name| Path::new("lib").join(name)));
    for (dir, matches) in [("anyps5/libs", is_prx as fn(&Path) -> bool), ("lib", is_dlss)] {
        for entry in fs::read_dir(install.join(dir))? {
            let path = Path::new(dir).join(entry?.file_name());
            if matches(&path) && install.join(&path).is_file() {
                paths.push(path);
            }
        }
    }
    paths.sort_unstable_by(|a, b| a.as_os_str().cmp(b.as_os_str()));

    let mut hash = Sha256::new();
    for path in paths {
        hash.update(path.as_os_str().as_encoded_bytes());
        hash.update(b"\0");
        hash.update(sha256_file(&install.join(path))?.as_bytes());
        hash.update(b"\n");
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn is_prx(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "prx")
}

fn is_dlss(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        name.as_encoded_bytes()
            .starts_with(b"libnvidia-ngx-dlss.so.")
    })
}

/// Written by the engine build: one line per export, "index NID library function".
const EXPORTS: &str = "lib/astroport-exports.txt";

/// NIDs the game imports from the three graphics libraries that no forwarder exports
/// (design.md 4.2). `registry` is a file the relinker wrote; a `#` suffix of a NID is not part of it.
/// Which of the three libraries exports a NID does not matter: the loader resolves symbols by
/// name alone, and Kyty files some NIDs under another library than the game imports them from.
pub fn missing_imports(registry: &str, exports: &str) -> Result<Vec<String>, serde_json::Error> {
    #[derive(Deserialize)]
    struct Import {
        nid: String,
        library: String,
    }
    let forwarded = |library: &str| FORWARDERS.iter().any(|name| name.strip_suffix(".prx") == Some(library));
    let exported: HashSet<&str> = exports
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(' ').skip(1);
            let nid = fields.next()?;
            forwarded(fields.next()?).then_some(nid)
        })
        .collect();
    let mut missing = BTreeSet::new();
    for import in serde_json::from_str::<Vec<Import>>(registry)? {
        let nid = import.nid.split('#').next().unwrap_or_default();
        if let Some(library) = import.library.strip_suffix(".prx").filter(|library| forwarded(library)) {
            if !exported.contains(nid) {
                missing.insert(format!("{nid} ({library})"));
            }
        }
    }
    Ok(missing.into_iter().collect())
}

/// Every `AstroBot*.registry.json` of the run folder against the installed export list.
fn check_imports(install: &Path, run: &Path) -> Result<(), JobError> {
    let exports = fs::read_to_string(install.join(EXPORTS))?;
    let mut missing = BTreeSet::new();
    for entry in fs::read_dir(run)? {
        let path = entry?.path();
        if path.to_string_lossy().ends_with(".registry.json") {
            let found = missing_imports(&fs::read_to_string(&path)?, &exports).map_err(|error| JobError {
                category: "internal",
                message: format!("{}: {error}", path.display()),
            })?;
            missing.extend(found);
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    Err(JobError {
        category: "unsupported",
        message: format!(
            "This game version uses graphics functions the renderer does not have: {}.",
            missing.into_iter().collect::<Vec<_>>().join(", ")
        ),
    })
}

pub fn needs_to_intel(cpuinfo: &str) -> bool {
    !cpuinfo.lines().any(|line| {
        line.split_once(':')
            .is_some_and(|(key, value)| key.trim() == "vendor_id" && value.trim() == "AuthenticAMD")
    })
}

pub fn relink(
    relinker: &Path,
    input_elf: &Path,
    out_dir: &Path,
    to_intel: bool,
    log: &Path,
) -> Result<(), JobError> {
    let output = out_dir.join("AstroBot");
    remove_file_if_present(log)?;
    let log_file = File::create_new(log)?;
    let mut command = Command::new(relinker);
    if to_intel {
        command.arg("--to-intel");
    }
    let result = command
        .arg("--registry")
        .arg(input_elf)
        .arg(&output)
        .stdout(log_file.try_clone()?)
        .stderr(log_file)
        .status();
    if result.is_ok_and(|status| status.success()) && is_regular_file(&output) {
        Ok(())
    } else {
        Err(JobError {
            category: "internal",
            message: format!(
                "Converting the game for Linux failed (see {}).",
                log.display()
            ),
        })
    }
}

pub fn build(
    args: &BuildArgs,
    font_roots: &[PathBuf],
    progress: &mut Progress,
) -> Result<Record, JobError> {
    let detected = detect(&args.dump, &args.install.join("patches/versions.json"))?;
    let fonts = find_fonts(font_roots)?;
    let dump = args.dump.canonicalize()?;
    let run = run_dir(&args.data, &detected.version);
    check_output_dir(&run, &dump)?;
    check_output_dir(&args.data.join("saves"), &dump)?;
    create_real_dir(&args.data.join("generated"))?;
    create_real_dir(&run)?;
    let record_path = run.join("build.json");
    remove_file_if_present(&record_path)?;
    let stage = run.join(".stage");
    ignore_missing(fs::remove_dir_all(&stage))?;
    let input = stage.join("in");
    let output = stage.join("out");
    let input_modules = input.join("sce_module");
    fs::create_dir_all(&input_modules)?;
    fs::create_dir(&output)?;
    progress.step(0, 1, 1, "steps", &format!("Version {}", detected.version));

    let dump_modules = args.dump.join("sce_module");
    let modules = module_names(&dump_modules)?;
    let total = modules.len() as u64 + 1;
    let input_elf = input.join("eboot.elf");
    unwrap_file(&args.dump.join("eboot.bin"), &input_elf)?;
    progress.step(1, 1, total, "files", "eboot.bin");
    for (index, name) in modules.iter().enumerate() {
        unwrap_file(&dump_modules.join(name), &input_modules.join(name))?;
        progress.step(1, index as u64 + 2, total, "files", &name.to_string_lossy());
    }

    progress.step(2, 0, 1, "steps", STEPS[2]);
    let to_intel = needs_to_intel(&fs::read_to_string("/proc/cpuinfo").unwrap_or_default());
    relink(
        &args.install.join("anyps5/relinker"),
        &input_elf,
        &output,
        to_intel,
        &run.join("relink.log"),
    )?;
    for name in &modules {
        let mut guest = name.clone();
        guest.push(".guest.prx");
        if !is_regular_file(&output.join("app0/sce_module").join(guest)) {
            return Err(JobError {
                category: "internal",
                message: format!(
                    "Converting the game for Linux did not produce a guest module for {}.",
                    name.to_string_lossy()
                ),
            });
        }
    }

    progress.step(3, 0, 1, "steps", STEPS[3]);
    link_app0(&args.dump, &run)?;
    place_outputs(&output, &run)?;
    check_imports(&args.install, &run)?;
    install_libs(&args.install, &run)?;
    link_fonts(&fonts, &run)?;
    link_saves(&args.data, &run)?;
    fs::set_permissions(run.join("AstroBot"), fs::Permissions::from_mode(0o755))?;
    fs::remove_dir_all(&stage)?;

    progress.step(4, 0, 1, "steps", STEPS[4]);
    let record = Record {
        format: 1,
        version: detected.version,
        content_version: detected.content_version,
        eboot_sha256: detected.eboot_sha256,
        release: installed_release(&args.install)?,
        inputs_sha256: inputs_sha256(&args.install)?,
        to_intel,
        dump: dump.to_string_lossy().into_owned(),
    };
    let temp = run.join(".build.json.astroport-tmp");
    remove_file_if_present(&temp)?;
    let file = File::create_new(&temp)?;
    serde_json::to_writer(file, &record).map_err(io::Error::other)?;
    fs::rename(temp, record_path)?;
    Ok(record)
}

pub fn status(dump: &Path, data: &Path, version: &str, install: &Path) -> &'static str {
    let run = run_dir(data, version);
    let executable = run.join("AstroBot");
    let record_path = run.join("build.json");
    if !is_regular_file(&executable)
        || File::open(executable).is_err()
        || !is_regular_file(&record_path)
    {
        return "missing";
    }
    let Some(record) = fs::read(record_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Record>(&bytes).ok())
    else {
        return "missing";
    };
    if record.version == version
        && dump
            .canonicalize()
            .is_ok_and(|path| path.to_str() == Some(&record.dump))
        && inputs_sha256(install).is_ok_and(|hash| hash == record.inputs_sha256)
    {
        "ready"
    } else {
        "stale"
    }
}

fn module_names(dir: &Path) -> io::Result<Vec<OsString>> {
    let mut names = Vec::new();
    match fs::read_dir(dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                let path = entry.path();
                if is_prx(&path) && path.is_file() {
                    names.push(entry.file_name());
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    names.sort_unstable();
    Ok(names)
}

fn installed_release(install: &Path) -> Result<String, JobError> {
    let path = install.join("ags-extension.toml");
    let manifest = fs::read_to_string(&path)?;
    manifest
        .lines()
        .filter_map(|line| line.split_once('='))
        .find(|(key, _)| key.trim() == "version")
        .and_then(|(_, value)| value.trim_start().strip_prefix('"'))
        .and_then(|value| value.split_once('"'))
        .map(|(version, _)| version.to_owned())
        .ok_or_else(|| JobError {
            category: "internal",
            message: format!("Missing release version in {}.", path.display()),
        })
}

fn check_output_dir(dir: &Path, dump: &Path) -> Result<(), JobError> {
    let dir = std::path::absolute(dir)?;
    // Check the nearest existing parent before creating any missing directories.
    for parent in dir.ancestors() {
        match parent.canonicalize() {
            Ok(path) if path.starts_with(dump) || (parent == dir && dump.starts_with(&path)) => {
                return Err(JobError {
                    category: "conflict",
                    message: format!(
                        "{} overlaps the game dump at {}.",
                        dir.display(),
                        dump.display()
                    ),
                });
            }
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn create_real_dir(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            format!("{} is not a real directory", path.display()),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir_all(path),
        Err(error) => Err(error),
    }
}

fn is_regular_file(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.is_file())
}

fn remove_file_if_present(path: &Path) -> io::Result<()> {
    ignore_missing(fs::remove_file(path))
}

fn ignore_missing(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}
