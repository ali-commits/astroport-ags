// Scanner for Kyty's LIB_VERSION / LIB_FUNC registrations (design.md 4.2).
// Included by build.rs and src/bin/nidgen.rs via #[path]; no dependency on the crate.

use std::path::Path;

/// Kyty library name (LIB_VERSION) -> the PS5 library the forwarder replaces.
pub const LIBRARIES: [(&str, &str); 3] = [
    ("Graphics5", "libSceAgc"),
    ("Graphics5Driver", "libSceAgcDriver"),
    ("VideoOut", "libSceVideoOut"),
];

/// The 17 libkernel memory functions AnyPS5 hands to the renderer (spec-anyps5 F3), by Kyty
/// function name. This order is the order of the function pointers in `ExternalRenderer`:
/// twelve always forwarded, then five forwarded by address.
pub const MEMORY_FUNCTIONS: [&str; 17] = [
    "KernelAllocateDirectMemory",
    "KernelAllocateMainDirectMemory",
    "KernelDirectMemoryQuery",
    "KernelGetDirectMemorySize",
    "KernelMapDirectMemory",
    "KernelMapDirectMemory2",
    "KernelMapNamedDirectMemory",
    "KernelMapFlexibleMemory",
    "KernelMapNamedFlexibleMemory",
    "KernelReleaseDirectMemory",
    "KernelAvailableFlexibleMemorySize",
    "KernelConfiguredFlexibleMemorySize",
    "KernelMprotect",
    "KernelMunmap",
    "KernelVirtualQuery",
    "KernelSetVirtualRangeName",
    "KernelClearVirtualRangeName",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    pub nid: String,
    /// PS5 library name for graphics exports, "libkernel" for memory functions.
    pub library: &'static str,
    /// Kyty function as written in the source, e.g. `Gen5Driver::AgcDriverAddEqEvent`.
    pub function: String,
}

/// Slot order: `exports` first (index = position), then `memory` (index = exports.len() + position).
#[derive(Debug, Default)]
pub struct Table {
    pub exports: Vec<Export>,
    pub memory: Vec<Export>,
}

fn quoted_arg<'a>(line: &'a str, macro_name: &str) -> Option<(&'a str, &'a str)> {
    let rest = line.trim_start().strip_prefix(macro_name)?.strip_prefix("(\"")?;
    rest.split_once('"')
}

/// `LIB_FUNC("<nid>", <function>);` -> (nid, function)
fn lib_func(line: &str) -> Option<Result<(&str, &str), String>> {
    let (nid, rest) = quoted_arg(line, "LIB_FUNC")?;
    let function = rest.strip_prefix(',').and_then(|r| r.split_once(')')).map(|(f, _)| f.trim());
    Some(match function {
        Some(f) if nid.len() == 11 && !f.is_empty() => Ok((nid, f)),
        _ => Err(format!("unreadable LIB_FUNC line: {}", line.trim())),
    })
}

/// Adds the LIB_FUNC rows of one graphics source file. Returns the number of LIB_FUNC lines read.
pub fn scan_graphics(source: &str, exports: &mut Vec<Export>) -> Result<usize, String> {
    let mut library: Option<&'static str> = None;
    let mut lines = 0;
    for line in source.lines() {
        if let Some((name, _)) = quoted_arg(line, "LIB_VERSION") {
            library = LIBRARIES.iter().find(|(kyty, _)| *kyty == name).map(|(_, ps5)| *ps5);
            if library.is_none() {
                return Err(format!("unknown library in LIB_VERSION: {name}"));
            }
        } else if line.trim_start().starts_with("LIB_OBJECT(") {
            return Err(format!("data registration cannot be forwarded: {}", line.trim()));
        } else if let Some(row) = lib_func(line) {
            let (nid, function) = row?;
            let library = library.ok_or_else(|| format!("LIB_FUNC before LIB_VERSION: {nid}"))?;
            lines += 1;
            if let Some(e) = exports.iter().find(|e| e.nid == nid) {
                return Err(format!("duplicate NID {nid}: {} {} and {library} {function}", e.library, e.function));
            }
            exports.push(Export { nid: nid.into(), library, function: function.into() });
        }
    }
    Ok(lines)
}

/// The 17 memory functions from the body of `LIB_DEFINE(InitLibKernel_1_Mem)`; the first NID
/// registered for a function wins.
pub fn scan_memory(source: &str) -> Result<Vec<Export>, String> {
    let body: Vec<(&str, &str)> = source
        .lines()
        .skip_while(|l| !l.starts_with("LIB_DEFINE(InitLibKernel_1_Mem)"))
        .skip(1)
        .take_while(|l| !l.starts_with('}'))
        .filter_map(lib_func)
        .collect::<Result<_, _>>()?;
    MEMORY_FUNCTIONS
        .iter()
        .map(|name| {
            let function = format!("Memory::{name}");
            body.iter()
                .find(|(_, f)| *f == function)
                .map(|(nid, _)| Export { nid: (*nid).into(), library: "libkernel", function })
                .ok_or_else(|| format!("memory function not registered in InitLibKernel_1_Mem: {name}"))
        })
        .collect()
}

/// Scans `engines/kyty/src/libs`.
pub fn scan(libs_dir: &Path) -> Result<Table, String> {
    let read = |name: &str| {
        let path = libs_dir.join(name);
        std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
    };
    let mut table = Table::default();
    for file in ["libAgcDriver.cpp", "libVideoOut.cpp"] {
        scan_graphics(&read(file)?, &mut table.exports)?;
    }
    for (_, library) in LIBRARIES {
        if !table.exports.iter().any(|e| e.library == library) {
            return Err(format!("no LIB_FUNC found for {library}"));
        }
    }
    table.memory = scan_memory(&read("libKernel.cpp")?)?;
    Ok(table)
}

/// One line per slot: index, NID, library, Kyty function. This is astroport-exports.txt, and the
/// text the hash is taken over.
pub fn listing(table: &Table) -> String {
    table
        .exports
        .iter()
        .chain(&table.memory)
        .enumerate()
        .map(|(i, e)| format!("{i} {} {} {}\n", e.nid, e.library, e.function))
        .collect()
}

/// Eight hex digits tying forwarders to the bridge: `astroport_entry_<hash8>` (FNV-1a, 32 bit).
pub fn hash8(table: &Table) -> String {
    let hash = listing(table).bytes().fold(0x811c9dc5u32, |h, b| (h ^ b as u32).wrapping_mul(0x01000193));
    format!("{hash:08x}")
}
