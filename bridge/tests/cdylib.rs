//! The built library: one exported symbol (design.md 1.1), and its constructor (3.4) starts
//! only next to both engines and refuses a table of the wrong version (3.2).

use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::{fs, process::Command};

const PROFILE: &str = if cfg!(debug_assertions) { "debug" } else { "release" };

/// cargo builds the cdylib together with the rlib this test links, in a folder of its own
/// choosing below the profile folder; an older copy may lie beside it.
fn newest_library(dir: &Path) -> Option<(SystemTime, PathBuf)> {
    let entries = fs::read_dir(dir).ok()?.flatten();
    let found = entries.filter_map(|entry| match entry.file_type().ok()? {
        kind if kind.is_dir() => newest_library(&entry.path()),
        _ if entry.file_name() == "libastroport_bridge.so" => Some((entry.metadata().ok()?.modified().ok()?, entry.path())),
        _ => None,
    });
    found.max()
}

#[test]
fn library_exports_one_symbol_and_its_constructor_checks_the_engines() {
    let library = newest_library(&Path::new(env!("CARGO_TARGET_TMPDIR")).with_file_name(PROFILE)).expect("libastroport_bridge.so").1;
    let nm = Command::new("nm").args(["-D", "--defined-only"]).arg(&library).output().unwrap();
    let symbols = String::from_utf8(nm.stdout).unwrap();
    let names: Vec<&str> = symbols.lines().filter_map(|line| line.split(' ').nth(2)).collect();
    assert_eq!(names, [format!("astroport_entry_{}", astroport_bridge::nids::HASH8)], "{symbols}");

    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("cdylib");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("main.c"),
        r#"#include <stdio.h>
#ifdef ENGINES
static const unsigned kyty[24] = {99, 96};
static const unsigned host[20] = {1, 80};
const void* kyty_embed_api(void) { return kyty; }
const void* AstroportHostApi_nid_no_patch(void) { return host; }
#endif
#ifdef CALL
__asm__("guest_call: xor %r11d, %r11d\njmp *ENTRY@GOTPCREL(%rip)");
void guest_call(void);
#else
#define guest_call()
#endif
int main(void) { puts("main"); fflush(stdout); guest_call(); return 0; }
"#
        .replace("ENTRY", names[0]),
    )
    .unwrap();
    let build = |output: &str, define: &[&str]| {
        let status = Command::new("cc")
            .current_dir(&dir)
            .args(["-o", output, "main.c", "-rdynamic", "-Wl,--no-as-needed"])
            .args(define)
            .arg(&library)
            .status()
            .unwrap();
        assert!(status.success());
        Command::new(dir.join(output)).output().unwrap()
    };

    let alone = build("alone", &[]);
    assert!(alone.status.success() && alone.stdout == b"main\n", "{alone:?}");

    // Built without the engine libraries: the first guest call names the cause, without waiting.
    let unstarted = build("unstarted", &["-DCALL"]);
    let message = String::from_utf8_lossy(&unstarted.stderr);
    assert!(!unstarted.status.success() && unstarted.stdout == b"main\n", "{unstarted:?}");
    assert!(message.contains("astroport: the bridge was not started: engines not loaded"), "{message}");

    let wrong = build("wrong", &["-DENGINES"]);
    let message = String::from_utf8_lossy(&wrong.stderr);
    assert!(!wrong.status.success() && wrong.stdout.is_empty(), "{wrong:?}");
    assert!(message.contains("astroport: KytyEmbedApi: abi 99 size 96, need abi 1 size >= 96"), "{message}");
}
