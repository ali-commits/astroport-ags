//! The generated forwarders assemble, link against a stub bridge the way scripts/build-engines.sh
//! links them (design.md 1.4), and a C program reaches the stub through NIDs with the right index.

use std::{fs, path::Path, process::Command};

fn run(cmd: &mut Command) -> String {
    let out = cmd.output().unwrap_or_else(|e| panic!("{cmd:?}: {e}"));
    assert!(out.status.success(), "{cmd:?}\n{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn c_program_calls_nids_through_stub_bridge() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("forwarders");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let libs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../engines/kyty/src/libs");
    run(Command::new(env!("CARGO_BIN_EXE_nidgen")).arg(&libs).arg(&dir));

    let listing = fs::read_to_string(dir.join("astroport-exports.txt")).unwrap();
    let index = |nid: &str| -> usize {
        let line = listing.lines().find(|l| l.split(' ').nth(1) == Some(nid)).expect(nid);
        line.split(' ').next().unwrap().parse().unwrap()
    };
    let hash = &astroport_bridge::nids::HASH8;
    assert_eq!(listing.lines().count(), astroport_bridge::nids::SLOTS.len());

    // Stub bridge: the entry returns first argument + slot index instead of jumping through a slot.
    fs::write(
        dir.join("stub.S"),
        format!(
            "\t.text\n\t.globl astroport_entry_{hash}\n\t.type astroport_entry_{hash}, @function\n\
             astroport_entry_{hash}:\n\tleaq (%rdi,%r11), %rax\n\tret\n\
             \t.section .note.GNU-stack,\"\",@progbits\n"
        ),
    )
    .unwrap();
    let cc = |args: &[&str]| run(Command::new("cc").current_dir(&dir).args(args));
    cc(&["-shared", "-nostdlib", "-o", "libastroport_bridge.so", "stub.S"]);
    for library in ["libSceAgc", "libSceAgcDriver", "libSceVideoOut"] {
        let (soname, out, src) = (format!("-Wl,-soname,{library}.prx"), format!("{library}.prx"), format!("{library}.S"));
        cc(&["-shared", "-nostdlib", "-Wl,-z,now", &soname, "-Wl,-rpath,$ORIGIN", "-o", &out, &src, "libastroport_bridge.so"]);
        let symbols = run(Command::new("nm").current_dir(&dir).args(["-D", "--undefined-only", &out]));
        assert!(symbols.contains(&format!("astroport_entry_{hash}")), "{symbols}");
    }

    // One NID that is a C-linkable name is called directly; NIDs with a leading digit, '+' or '-'
    // cannot be written in C, so they go through dlsym on the already linked libraries.
    let direct = "PjS5uASwcV8";
    fs::write(
        dir.join("main.c"),
        format!(
            r#"#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
long direct(long) __asm__("{direct}");
static int check(const char* nid, long got, long want) {{
    if (got != want) printf("%s: got %ld, want %ld\n", nid, got, want);
    return got != want;
}}
static long by_name(const char* nid) {{
    long (*f)(long) = (long (*)(long))dlsym(RTLD_DEFAULT, nid);
    return f ? f(1000) : -1;
}}
int main(void) {{
    int bad = check("{direct}", direct(1000), 1000 + {});
    bad += check("23LRUSvYu1M", by_name("23LRUSvYu1M"), 1000 + {});
    bad += check("w2rJhmD+dsE", by_name("w2rJhmD+dsE"), 1000 + {});
    bad += check("-Ozn0F1AFRg", by_name("-Ozn0F1AFRg"), 1000 + {});
    if (!bad) puts("ok");
    return bad;
}}
"#,
            index(direct),
            index("23LRUSvYu1M"),
            index("w2rJhmD+dsE"),
            index("-Ozn0F1AFRg")
        ),
    )
    .unwrap();
    cc(&[
        "-o", "main", "main.c", "-Wl,--no-as-needed", "-L.", "-l:libSceAgc.prx", "-l:libSceAgcDriver.prx",
        "-l:libSceVideoOut.prx", "-Wl,-rpath,$ORIGIN", "-Wl,-rpath-link,.",
    ]);
    // cargo's LD_LIBRARY_PATH would put the real bridge before the stub next to the program.
    assert_eq!(run(Command::new(dir.join("main")).env("LD_BIND_NOW", "1").env_remove("LD_LIBRARY_PATH")), "ok\n");
}
