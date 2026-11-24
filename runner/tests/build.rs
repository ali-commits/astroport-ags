#[allow(dead_code)]
mod common;

use astroport_run::build::{build, needs_to_intel, run_dir, status, BuildArgs, Record};
use astroport_run::layout::NOTO_FONTS;
use astroport_run::progress::Progress;
use astroport_run::version::sha256_file;
use common::{build_self, fake_dump, fake_install, fake_relinker, temp_dir, versions_json, Seg};
use serde_json::{json, Value};
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

struct Fixture {
    root: PathBuf,
    args: BuildArgs,
    fonts: [PathBuf; 1],
    eboot_sha256: String,
}

impl Fixture {
    fn new(content_version: &str) -> Self {
        let root = temp_dir("build");
        let install = fake_install(&root.join("install"));
        fake_relinker(&install.join("anyps5/relinker"));
        let self_file = |seed| {
            build_self(&[Seg {
                kind: 1,
                vaddr: 0x1000,
                data: common::low_entropy(256, seed),
                memsz: 256,
                align: 0x10,
                recorded: true,
            }])
        };
        let dump = fake_dump(
            &root.join("[DL] Astro Bot 01.019 PPSA21567"),
            content_version,
            &self_file(0),
            &[
                ("libc.prx", self_file(16)),
                ("libSceNpCppWebApi.prx", self_file(32)),
            ],
        );
        let eboot_sha256 = sha256_file(&dump.join("eboot.bin")).unwrap();
        versions_json(&install.join("patches/versions.json"), Some(&eboot_sha256));
        let font_root = root.join("fonts");
        fs::create_dir(&font_root).unwrap();
        for name in NOTO_FONTS {
            fs::write(font_root.join(name), []).unwrap();
        }
        let data = root.join("data");
        fs::create_dir(&data).unwrap();
        Self {
            root,
            args: BuildArgs {
                dump,
                data,
                install,
            },
            fonts: [font_root],
            eboot_sha256,
        }
    }

    fn run(&self) -> PathBuf {
        run_dir(&self.args.data, "1.019")
    }

    fn build(&self) -> Result<Record, astroport_run::JobError> {
        build(
            &self.args,
            &self.fonts,
            &mut Progress::to_writer(Box::new(io::sink())),
        )
    }

    fn status(&self) -> &'static str {
        status(
            &self.args.dump,
            &self.args.data,
            "1.019",
            &self.args.install,
        )
    }

    fn cli(&self, subcommand: &str) -> Command {
        let mut command = binary();
        command
            .arg(subcommand)
            .arg("--dump")
            .arg(&self.args.dump)
            .arg("--install")
            .arg(&self.args.install)
            .arg("--data")
            .arg(&self.args.data);
        command
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn binary() -> Command {
    let mut command = Command::new("timeout");
    command
        .args([
            "--foreground",
            "-k",
            "10",
            "30",
            env!("CARGO_BIN_EXE_astroport-run"),
        ])
        .env_remove("AGS_JOB_PROGRESS_FD");
    command
}

fn dump_bytes(path: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    for entry in fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            files.extend(dump_bytes(&entry.path()));
        } else {
            files.push((entry.path(), fs::read(entry.path()).unwrap()));
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

#[test]
fn build_lays_out_the_run_folder() {
    let fixture = Fixture::new("01.019.000");
    let before = dump_bytes(&fixture.args.dump);
    let record = fixture.build().unwrap();
    assert_eq!(record.version, "1.019");
    let run = fixture.run();
    assert_eq!(run, fixture.args.data.join("generated/run-1.019"));
    for path in [
        "AstroBot",
        "AstroBot.registry.json",
        "AstroBot.needed.json",
        "app0/sce_module/libc.prx.guest.prx",
        "app0/sce_module/libSceNpCppWebApi.prx.guest.prx",
        "AstroBot.libc.prx.guest.prx.registry.json",
    ] {
        assert!(run.join(path).is_file(), "missing {path}");
    }
    let binary = fs::symlink_metadata(run.join("AstroBot")).unwrap();
    assert!(binary.is_file());
    assert_eq!(binary.permissions().mode() & 0o777, 0o755);
    assert_eq!(
        fs::read_link(run.join("app0/eboot.bin")).unwrap(),
        fixture.args.dump.join("eboot.bin")
    );
    assert_eq!(
        fs::read(run.join("libs/libSceAgc.prx")).unwrap(),
        b"forwarder libSceAgc.prx"
    );
    assert_eq!(
        fs::read_link(run.join("_sd_mem")).unwrap(),
        fixture.args.data.join("saves/_sd_mem")
    );
    let fonts: Vec<_> = fs::read_dir(run.join("anyps5-fonts"))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(fonts.len(), 24);
    assert!(fonts
        .iter()
        .all(|entry| entry.file_type().unwrap().is_symlink()));
    assert!(!run.join(".stage").exists());
    assert_eq!(fixture.status(), "ready");

    let saved: Record = serde_json::from_slice(&fs::read(run.join("build.json")).unwrap()).unwrap();
    assert_eq!(saved.format, 1);
    assert_eq!(saved.version, "1.019");
    assert_eq!(saved.content_version, "01.019.000");
    assert_eq!(saved.eboot_sha256, fixture.eboot_sha256);
    assert_eq!(saved.release, "0.1.0");
    assert_eq!(
        Path::new(&saved.dump),
        fixture.args.dump.canonicalize().unwrap()
    );
    let recorded_args = fs::read_to_string(run.join("args.txt")).unwrap();
    assert!(recorded_args.ends_with(&format!(
        "--registry\n{}\n{}\n",
        run.join(".stage/in/eboot.elf").display(),
        run.join(".stage/out/AstroBot").display()
    )));
    assert_eq!(dump_bytes(&fixture.args.dump), before);
}

struct ProgressBuffer(Arc<Mutex<Vec<u8>>>);

impl Write for ProgressBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn progress_lines_follow_the_steps() {
    let fixture = Fixture::new("01.019.000");
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let mut progress = Progress::to_writer(Box::new(ProgressBuffer(bytes.clone())));
    build(&fixture.args, &fixture.fonts, &mut progress).unwrap();

    let bytes = bytes.lock().unwrap();
    let lines: Vec<Value> = std::str::from_utf8(&bytes)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        lines,
        [
            json!({"step":0,"done":1,"total":1,"unit":"steps","message":"Version 1.019"}),
            json!({"step":1,"done":1,"total":3,"unit":"files","message":"eboot.bin"}),
            json!({"step":1,"done":2,"total":3,"unit":"files","message":"libSceNpCppWebApi.prx"}),
            json!({"step":1,"done":3,"total":3,"unit":"files","message":"libc.prx"}),
            json!({"step":2,"done":0,"total":1,"unit":"steps","message":"Convert for Linux"}),
            json!({"step":3,"done":0,"total":1,"unit":"steps","message":"Set up the game folder"}),
            json!({"step":4,"done":0,"total":1,"unit":"steps","message":"Record the result"}),
        ]
    );
}

#[test]
fn relinker_failure_is_reported() {
    let fixture = Fixture::new("01.019.000");
    fs::write(fixture.args.install.join("anyps5/fail"), []).unwrap();
    let error = fixture.build().err().expect("expected relinker failure");
    assert_eq!(error.category, "internal");
    assert_eq!(
        error.message,
        format!(
            "Converting the game for Linux failed (see {}).",
            fixture.run().join("relink.log").display()
        )
    );
    assert_eq!(fixture.status(), "missing");
}

#[test]
fn interrupted_build_is_not_ready_and_rebuilds() {
    let fixture = Fixture::new("01.019.000");
    fixture.build().unwrap();
    let stage = fixture.run().join(".stage/in");
    fs::create_dir_all(&stage).unwrap();
    fs::write(stage.join("junk"), b"left by interrupted build").unwrap();
    fs::remove_file(fixture.run().join("build.json")).unwrap();
    assert_eq!(fixture.status(), "missing");
    fixture.build().unwrap();
    assert!(!stage.parent().unwrap().exists());
    assert_eq!(fixture.status(), "ready");
}

#[test]
fn changed_install_makes_it_stale() {
    let fixture = Fixture::new("01.019.000");
    fixture.build().unwrap();
    let other_dump = fake_dump(
        &fixture.root.join("different dump"),
        "01.019.000",
        b"other",
        &[],
    );
    assert_eq!(
        status(
            &other_dump,
            &fixture.args.data,
            "1.019",
            &fixture.args.install
        ),
        "stale"
    );
    fs::write(
        fixture.args.install.join("lib/libkyty_renderer.so"),
        b"updated renderer",
    )
    .unwrap();
    assert_eq!(fixture.status(), "stale");
}

#[test]
fn unsupported_dump_stops_at_step_one() {
    let fixture = Fixture::new("01.020.000");
    let error = fixture.build().err().expect("expected unsupported dump");
    assert_eq!(error.category, "unsupported");
    assert!(!fixture.args.data.join("generated").exists());
}

#[test]
fn to_intel_only_off_on_amd() {
    assert!(!needs_to_intel("vendor_id\t: AuthenticAMD\n"));
    assert!(needs_to_intel("vendor_id\t: GenuineIntel\n"));
    assert!(needs_to_intel(""));
}

#[test]
fn binary_build_unsupported_exits_1_with_error_line() {
    let fixture = Fixture::new("01.020.000");
    let output = fixture.cli("build").output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().lines().last(),
        Some(
            r#"{"error":{"category":"unsupported","message":"Version 1.020 is not supported. AstroPort runs 1.018 and 1.019."}}"#
        )
    );
}

#[test]
fn binary_status_and_usage() {
    let fixture = Fixture::new("01.019.000");
    let output = fixture
        .cli("status")
        .args(["--version", "1.019"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"{\"state\":\"missing\"}\n");
    assert_eq!(binary().output().unwrap().status.code(), Some(2));
}

#[test]
fn missing_imports_names_what_the_renderer_lacks() {
    let registry = r#"[{"nid":"23LRUSvYu1M#w#w","library":"libSceAgc.prx","weak":false},
                       {"nid":"-Ozn0F1AFRg","library":"libSceAgcDriver.prx"},
                       {"nid":"rTXw65xmLIA#v#v","library":"libSceVideoOut.prx"},
                       {"nid":"unknown#s#s","library":"libc.prx"}]"#;
    let exports = "0 23LRUSvYu1M libSceAgc Gen5::AgcInit\n1 -Ozn0F1AFRg libSceVideoOut VideoOut::X\n\
                   2 rTXw65xmLIA libkernel Memory::KernelAllocateDirectMemory\n";
    assert_eq!(
        astroport_run::build::missing_imports(registry, exports).unwrap(),
        ["rTXw65xmLIA (libSceVideoOut)"]
    );
    assert!(astroport_run::build::missing_imports("{}", exports).is_err());
}

#[test]
fn build_fails_on_an_import_the_renderer_lacks() {
    let fixture = Fixture::new("01.019.000");
    fs::write(fixture.args.install.join("lib/astroport-exports.txt"), "0 other libSceAgc X\n").unwrap();
    let Err(error) = fixture.build() else { panic!("the build must fail") };
    assert_eq!(error.category, "unsupported");
    assert!(error.message.contains("23LRUSvYu1M (libSceAgc)"), "{}", error.message);
    assert!(!fixture.run().join("build.json").exists());
}
