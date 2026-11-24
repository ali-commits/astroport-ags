#[path = "nidscan.rs"]
mod nidscan;

use std::{env, fmt::Write, fs, path::Path};

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    let libs = Path::new(&manifest).join("../engines/kyty/src/libs");
    println!("cargo:rerun-if-changed=nidscan.rs");
    for file in ["libAgcDriver.cpp", "libVideoOut.cpp", "libKernel.cpp"] {
        println!("cargo:rerun-if-changed={}", libs.join(file).display());
    }
    let table = nidscan::scan(&libs).unwrap_or_else(|e| panic!("nidscan: {e}"));
    let hash = nidscan::hash8(&table);

    let mut out = String::new();
    writeln!(out, "pub struct Export {{ pub nid: &'static str, pub library: &'static str, pub function: &'static str }}").unwrap();
    writeln!(out, "pub const HASH8: &str = \"{hash}\";").unwrap();
    writeln!(out, "#[macro_export] macro_rules! astroport_entry_symbol {{ () => {{ \"astroport_entry_{hash}\" }} }}").unwrap();
    writeln!(out, "pub const EXPORT_COUNT: usize = {};", table.exports.len()).unwrap();
    writeln!(out, "pub const MEMORY_COUNT: usize = {};", table.memory.len()).unwrap();
    writeln!(out, "/// Slot order: graphics exports, then the memory functions in `ExternalRenderer` order.").unwrap();
    writeln!(out, "pub static SLOTS: [Export; EXPORT_COUNT + MEMORY_COUNT] = [").unwrap();
    for e in table.exports.iter().chain(&table.memory) {
        writeln!(out, "    Export {{ nid: {:?}, library: {:?}, function: {:?} }},", e.nid, e.library, e.function).unwrap();
    }
    writeln!(out, "];").unwrap();
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("nids.rs"), out).unwrap();

    println!("cargo:rerun-if-env-changed=ASTROPORT_ENGINE_LIBS");
    if let Ok(dir) = env::var("ASTROPORT_ENGINE_LIBS") {
        println!("cargo:rustc-link-search=native={dir}");
        for arg in ["-Wl,--no-as-needed", "-l:libkyty_renderer.so", "-l:libAstroportHost.prx", "-Wl,-rpath,$ORIGIN"] {
            println!("cargo:rustc-cdylib-link-arg={arg}");
        }
    }
}
