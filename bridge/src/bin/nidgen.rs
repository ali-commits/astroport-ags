// nidgen <engines/kyty/src/libs> <out dir>
// Writes libSceAgc.S, libSceAgcDriver.S, libSceVideoOut.S and astroport-exports.txt (design.md 4.1, 4.2).

#[path = "../../nidscan.rs"]
mod nidscan;

use nidscan::Table;
use std::{fmt::Write, fs, path::Path, process::ExitCode};

/// One thunk per NID of `library`: load the slot index, jump to the bridge's entry.
fn forwarder_asm(table: &Table, library: &str) -> String {
    let entry = format!("astroport_entry_{}", nidscan::hash8(table));
    let mut out = String::from("\t.text\n");
    for (index, e) in table.exports.iter().enumerate().filter(|(_, e)| e.library == library) {
        let nid = &e.nid;
        write!(
            out,
            "\t.globl \"{nid}\"\n\t.type \"{nid}\", @function\n\"{nid}\":\n\
             \tmovl ${index}, %r11d\n\tjmp *{entry}@GOTPCREL(%rip)\n\t.size \"{nid}\", .-\"{nid}\"\n"
        )
        .unwrap();
    }
    out.push_str("\t.section .note.GNU-stack,\"\",@progbits\n");
    out
}

fn run(libs: &Path, out: &Path) -> Result<(), String> {
    let table = nidscan::scan(libs)?;
    let write = |name: String, text: String| {
        let path = out.join(name);
        fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
    };
    for (_, library) in nidscan::LIBRARIES {
        write(format!("{library}.S"), forwarder_asm(&table, library))?;
    }
    write("astroport-exports.txt".into(), nidscan::listing(&table))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: nidgen <engines/kyty/src/libs> <out dir>");
        return ExitCode::from(2);
    }
    match run(Path::new(&args[1]), Path::new(&args[2])) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nidgen: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nidscan::*;

    const SAMPLE: &str = r#"
LIB_VERSION("Graphics5", 1, "Graphics5", 1, 1);
LIB_DEFINE(InitAgcDriver_1) {
	LIB_FUNC("23LRUSvYu1M", Gen5::AgcInit);
	// LIB_FUNC("commentedOut", Gen5::Nope);
	LIB_FUNC("k-JpyR2dYAM", Gen5::AgcCondExecPatchSetEnd);            // trailing comment
}
LIB_VERSION("Graphics5Driver", 1, "Graphics5Driver", 1, 1);
	LIB_FUNC("w2rJhmD+dsE", Gen5Driver::AgcDriverAddEqEvent);
"#;

    fn real_libs() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../engines/kyty/src/libs")
    }

    #[test]
    fn reads_libraries_functions_and_comments() {
        let mut exports = Vec::new();
        assert_eq!(scan_graphics(SAMPLE, &mut exports), Ok(3));
        let rows: Vec<_> = exports.iter().map(|e| (e.nid.as_str(), e.library, e.function.as_str())).collect();
        assert_eq!(
            rows,
            [
                ("23LRUSvYu1M", "libSceAgc", "Gen5::AgcInit"),
                ("k-JpyR2dYAM", "libSceAgc", "Gen5::AgcCondExecPatchSetEnd"),
                ("w2rJhmD+dsE", "libSceAgcDriver", "Gen5Driver::AgcDriverAddEqEvent"),
            ]
        );
    }

    #[test]
    fn rejects_bad_input() {
        let scan = |s: &str| scan_graphics(s, &mut Vec::new()).unwrap_err();
        assert!(scan("LIB_FUNC(\"23LRUSvYu1M\", A::B);").contains("before LIB_VERSION"));
        assert!(scan("LIB_VERSION(\"Audio\", 1, \"Audio\", 1, 1);").contains("unknown library"));
        let v = "LIB_VERSION(\"VideoOut\", 1, \"VideoOut\", 1, 1);\n";
        assert!(scan(&format!("{v}LIB_OBJECT(\"f7uOxY9mM1U\", &x);")).contains("data registration"));
        assert!(scan(&format!("{v}LIB_FUNC(\"short\", A::B);")).contains("unreadable"));
        assert!(scan(&format!("{v}LIB_FUNC(\"23LRUSvYu1M\", A::B);\nLIB_FUNC(\"23LRUSvYu1M\", A::C);"))
            .contains("duplicate NID 23LRUSvYu1M"));
    }

    #[test]
    fn memory_needs_all_seventeen() {
        let err = scan_memory("LIB_DEFINE(InitLibKernel_1_Mem) {\n\tLIB_FUNC(\"rTXw65xmLIA\", Memory::KernelAllocateDirectMemory);\n}\n");
        assert!(err.unwrap_err().contains("KernelAllocateMainDirectMemory"));
    }

    #[test]
    fn real_tree() {
        let libs = real_libs();
        let mut exports = Vec::new();
        let mut lines = 0;
        for file in ["libAgcDriver.cpp", "libVideoOut.cpp"] {
            lines += scan_graphics(&fs::read_to_string(libs.join(file)).unwrap(), &mut exports).unwrap();
        }
        assert_eq!((lines, exports.len()), (200, 200));

        let table = scan(&libs).unwrap();
        let count = |library| table.exports.iter().filter(|e| e.library == library).count();
        assert_eq!((count("libSceAgc"), count("libSceAgcDriver"), count("libSceVideoOut")), (148, 16, 36));
        let find = |nid: &str| table.exports.iter().find(|e| e.nid == nid).map(|e| e.function.as_str());
        assert_eq!(find("w2rJhmD+dsE"), Some("Gen5Driver::AgcDriverAddEqEvent"));
        assert_eq!(find("DL2RXaXOy88"), Some("Gen5Driver::AgcDriverDeleteEqEvent"));
        assert_eq!(find("HXzjK9yI30k"), Some("VideoOut::VideoOutAddFlipEvent"));

        let memory: Vec<_> = table.memory.iter().map(|e| e.function.trim_start_matches("Memory::")).collect();
        assert_eq!(memory, MEMORY_FUNCTIONS);
        assert_eq!(table.memory[0].nid, "rTXw65xmLIA");
        assert_eq!(table.memory[7].nid, "IWIBBdTHit4"); // first of the two KernelMapFlexibleMemory NIDs
        assert_eq!(listing(&table).lines().count(), table.exports.len() + 17);
        assert_eq!(hash8(&table).len(), 8);
    }

    #[test]
    fn forwarder_text() {
        let mut table = Table::default();
        scan_graphics(SAMPLE, &mut table.exports).unwrap();
        let entry = format!("astroport_entry_{}", hash8(&table));
        let asm = forwarder_asm(&table, "libSceAgcDriver");
        assert!(asm.contains(&format!("\"w2rJhmD+dsE\":\n\tmovl $2, %r11d\n\tjmp *{entry}@GOTPCREL(%rip)\n")));
        assert!(!asm.contains("23LRUSvYu1M"));
    }
}
