//! Applies a KytyPS5 cheat file (`_Patches/<TITLE>.json`) to an AnyPS5-relinked game at startup.
//!
//! Loaded with `LD_PRELOAD`. Its constructor runs after the dynamic loader has mapped and relocated the
//! game, before the game's own code starts. Each `memory` entry's `offset` is a guest virtual address of
//! `eboot.bin`. AnyPS5's Linux output puts guest address 0 at `ASTRO_PATCH_BASE` (default `0x400000`).
//! Kyty code caves outside the image (for example `0x12000000`) are mapped as fresh RWX pages first.
//!
//! Environment:
//! - `ASTRO_PATCHES`: path to the cheat JSON (required; without it the library does nothing).
//! - `ASTRO_PATCH_BASE`: load address of guest address 0 (default `0x400000`).
//! - `ASTRO_PATCH_ONLY`: optional comma-separated substrings; only mods whose name contains one apply.
//! - `ASTRO_PATCH_SKIP`: optional comma-separated substrings; mods whose name contains one are skipped.
//! - `ASTRO_PATCH_ENABLE`: optional comma-separated substrings; disabled mods whose name contains one apply too.
//! - `ASTRO_PATCH_PROCESS`: executable name to patch (default `AstroBot`); other processes are left alone.

use serde::Deserialize;
use std::io::Write;

#[derive(Deserialize)]
struct CheatFile {
    #[serde(default)]
    id: String,
    #[serde(default)]
    version: String,
    mods: Vec<Mod>,
}

#[derive(Deserialize)]
struct Mod {
    name: String,
    #[serde(default)]
    enabled: bool,
    memory: Vec<Mem>,
}

#[derive(Deserialize)]
struct Mem {
    offset: String,
    on: String,
    off: String,
}

const PAGE: usize = 4096;

fn log(msg: &str) {
    let _ = writeln!(std::io::stderr(), "[astro-patcher] {msg}");
}

fn parse_hex_u64(s: &str) -> Option<u64> {
    let t = s.trim();
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    u64::from_str_radix(t, 16).ok()
}

fn parse_bytes(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

fn list_env(name: &str) -> Vec<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.split(',').map(|x| x.trim().to_lowercase()).filter(|x| !x.is_empty()).collect())
        .unwrap_or_default()
}

/// Ranges currently mapped in this process, from /proc/self/maps.
fn mapped_ranges() -> Vec<(usize, usize)> {
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
    maps.lines()
        .filter_map(|l| {
            let r = l.split_whitespace().next()?;
            let (a, b) = r.split_once('-')?;
            Some((usize::from_str_radix(a, 16).ok()?, usize::from_str_radix(b, 16).ok()?))
        })
        .collect()
}

/// True when every page of [addr, addr+len) is mapped with read permission.
fn readable(addr: usize, len: usize) -> bool {
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
    let mut p = addr & !(PAGE - 1);
    while p < addr + len {
        let ok = maps.lines().any(|l| {
            let mut it = l.split_whitespace();
            let (Some(r), Some(perm)) = (it.next(), it.next()) else { return false };
            let Some((a, b)) = r.split_once('-') else { return false };
            let (a, b) = (usize::from_str_radix(a, 16).unwrap_or(0), usize::from_str_radix(b, 16).unwrap_or(0));
            p >= a && p < b && perm.starts_with('r')
        });
        if !ok { return false; }
        p += PAGE;
    }
    true
}

fn is_mapped(ranges: &[(usize, usize)], page: usize) -> bool {
    ranges.iter().any(|&(a, b)| page >= a && page < b)
}

/// Makes sure every page of [addr, addr+len) is mapped, mapping missing pages as fresh RWX memory.
fn ensure_mapped(addr: usize, len: usize) -> Result<(), String> {
    let ranges = mapped_ranges();
    let first = addr & !(PAGE - 1);
    let last = (addr + len + PAGE - 1) & !(PAGE - 1);
    let mut p = first;
    while p < last {
        if !is_mapped(&ranges, p) {
            let r = unsafe {
                libc::mmap(
                    p as *mut libc::c_void,
                    PAGE,
                    libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED_NOREPLACE,
                    -1,
                    0,
                )
            };
            if r == libc::MAP_FAILED || r as usize != p {
                return Err(format!("cannot map code cave page {p:#x}: {}", std::io::Error::last_os_error()));
            }
        }
        p += PAGE;
    }
    Ok(())
}

/// Writes `bytes` at `addr`, temporarily making the pages writable (and executable, as code is patched).
fn write_mem(addr: usize, bytes: &[u8]) -> Result<(), String> {
    let first = addr & !(PAGE - 1);
    let last = (addr + bytes.len() + PAGE - 1) & !(PAGE - 1);
    let rc = unsafe {
        libc::mprotect(first as *mut libc::c_void, last - first, libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC)
    };
    if rc != 0 {
        return Err(format!("mprotect {first:#x}: {}", std::io::Error::last_os_error()));
    }
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), addr as *mut u8, bytes.len()) };
    Ok(())
}

fn run() -> Result<(), String> {
    let Ok(path) = std::env::var("ASTRO_PATCHES") else { return Ok(()) };
    // Only the game itself: LD_AUDIT also reaches the launcher's helper programs (cp, tr, ...) and
    // processes the game starts, whose memory at the guest addresses is something else.
    let process = std::env::var("ASTRO_PATCH_PROCESS").unwrap_or_else(|_| "AstroBot".into());
    let exe = std::fs::read_link("/proc/self/exe").ok();
    if exe.as_ref().and_then(|p| p.file_name()).map_or(true, |n| n.to_string_lossy() != process) {
        return Ok(());
    }
    // Let a debugger or eu-stack of the same user read the game's threads when it hangs
    // (kernel.yama.ptrace_scope=1 otherwise allows only parent processes).
    extern "C" {
        fn prctl(option: i32, arg2: usize, ...) -> i32;
    }
    const PR_SET_PTRACER: i32 = 0x59616d61;
    unsafe { prctl(PR_SET_PTRACER, usize::MAX) };
    let base = std::env::var("ASTRO_PATCH_BASE").ok().and_then(|v| parse_hex_u64(&v)).unwrap_or(0x400000) as usize;
    let only = list_env("ASTRO_PATCH_ONLY");
    let skip = list_env("ASTRO_PATCH_SKIP");
    let enable = list_env("ASTRO_PATCH_ENABLE");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("read {path}: {e}"))?;
    let file: CheatFile = serde_json::from_str(&text).map_err(|e| format!("parse {path}: {e}"))?;
    log(&format!("{} {} from {path}, guest base {base:#x}, pid {}", file.id, file.version, std::process::id()));
    if std::env::var("ASTRO_PATCH_DEBUG").is_ok() {
        let maps = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
        for l in maps.lines() {
            let Some((a, b)) = l.split_whitespace().next().and_then(|r| r.split_once('-')) else { continue };
            let (a, b) = (usize::from_str_radix(a, 16).unwrap_or(0), usize::from_str_radix(b, 16).unwrap_or(0));
            if b > base && a < base + 0x13000000 { log(&format!("  map {l}")); }
        }
    }

    // Map every cave page first: one mod's code often jumps into another mod's cave.
    let mut chosen = Vec::new();
    for m in &file.mods {
        let lname = m.name.to_lowercase();
        let wanted = (m.enabled || enable.iter().any(|e| lname.contains(e)))
            && (only.is_empty() || only.iter().any(|o| lname.contains(o)))
            && !skip.iter().any(|s| lname.contains(s));
        if wanted {
            chosen.push(m);
        }
    }
    for m in &chosen {
        for e in &m.memory {
            let off = parse_hex_u64(&e.offset).ok_or_else(|| format!("bad offset {}", e.offset))? as usize;
            let on = parse_bytes(&e.on).ok_or_else(|| format!("bad bytes in {}", m.name))?;
            ensure_mapped(base + off, on.len())?;
        }
    }

    let mut applied = 0;
    let mut written: Vec<(usize, usize)> = Vec::new(); // byte ranges written by earlier mods
    for m in &chosen {
        // Check every entry of a mod before writing any, so a mod is applied whole or not at all.
        let mut entries = Vec::new();
        let mut ok = true;
        for e in &m.memory {
            let off = parse_hex_u64(&e.offset).unwrap() as usize;
            let on = parse_bytes(&e.on).unwrap();
            let expect = parse_bytes(&e.off).ok_or_else(|| format!("bad bytes in {}", m.name))?;
            let addr = base + off;
            if !readable(addr, on.len()) {
                log(&format!("  SKIP {}: {off:#x} is reserved without access (owned by the runtime)", m.name));
                ok = false;
                break;
            }
            let cur = unsafe { std::slice::from_raw_parts(addr as *const u8, on.len()) };
            // A shared cave already written by an earlier mod (same bytes) is fine.
            let already = cur == on.as_slice();
            let overlaps = written.iter().any(|&(a, b)| addr < b && addr + on.len() > a);
            if !already && cur != expect.as_slice() && !overlaps {
                log(&format!("  SKIP {}: bytes at {off:#x} differ from the expected original", m.name));
                ok = false;
                break;
            }
            entries.push((addr, on));
        }
        if !ok {
            continue;
        }
        for (addr, on) in entries {
            write_mem(addr, &on)?;
            written.push((addr, addr + on.len()));
        }
        applied += 1;
        log(&format!("  on  {}", m.name));
    }
    log(&format!("{applied} of {} enabled mods applied", chosen.len()));
    Ok(())
}

extern "C" fn init() {
    if let Err(e) = run() {
        log(&format!("error: {e}"));
    }
}

#[used]
#[link_section = ".init_array"]
static INIT: extern "C" fn() = init;

/// Lets the library load through `LD_AUDIT`, so its constructor runs before every library initializer:
/// the code caves are then mapped before AnyPS5's libc reserves the free address space as guest arena.
#[no_mangle]
pub extern "C" fn la_version(version: u32) -> u32 {
    version
}
