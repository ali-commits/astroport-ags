//! Thunk ABI (design.md 4.1, 4.3): calls with six integer, eight double, variadic and stack
//! arguments go through a forwarder-style jump and through a memory thunk, first while the
//! slots still hold the wait stub, then bound, and the callee sees every value.

use astroport_bridge::entry;
use astroport_bridge::nids::EXPORT_COUNT;
use std::arch::naked_asm;
use std::ffi::{c_char, c_int, c_void, CStr};
use std::time::Duration;

const EXPORT: usize = 5;

/// What a generated forwarder does.
#[unsafe(naked)]
unsafe extern "C" fn forwarder() {
    naked_asm!("mov r11d, {index}", "jmp {entry}", index = const EXPORT, entry = sym entry::astroport_entry)
}

type Format = unsafe extern "C" fn(*mut c_char, usize, *const c_char, ...) -> c_int;
type Mixed = unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64;

#[allow(clippy::too_many_arguments)]
extern "C" fn mixed(a: i64, b: i64, c: i64, d: i64, e: i64, f: i64, g: f64, h: f64, i: f64, j: f64, k: f64, l: f64, m: f64, n: f64) -> f64 {
    let seen = [a, b, c, d, e, f] == [1, 2, 3, 4, 5, 6] && [g, h, i, j, k, l, m, n] == [0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5];
    if seen {
        42.0
    } else {
        -1.0
    }
}

/// snprintf: six integer registers, eight vector registers (count in rax), two stack arguments.
fn format(thunk: usize) -> String {
    let mut text = [0u8; 128];
    unsafe {
        let snprintf: Format = std::mem::transmute(thunk);
        let pattern = c"%d %d %d %.1f %.1f %.1f %.1f %.1f %.1f %.1f %.1f %d %d";
        snprintf(text.as_mut_ptr().cast(), text.len(), pattern.as_ptr(), 1, 2, 3, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5, 8.5, 4, 5);
    }
    CStr::from_bytes_until_nul(&text).unwrap().to_str().unwrap().to_owned()
}

fn call_mixed(thunk: usize) -> f64 {
    unsafe { std::mem::transmute::<usize, Mixed>(thunk)(1, 2, 3, 4, 5, 6, 0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5) }
}

#[test]
fn calls_pass_through_thunks_before_and_after_binding() {
    const FORMATTED: &str = "1 2 3 1.5 2.5 3.5 4.5 5.5 6.5 7.5 8.5 4 5";
    let forwarder = forwarder as *const () as usize;
    let memory = |position: usize| entry::MEMORY_THUNKS[position] as usize;
    // A started bridge: without engines the wait stub aborts (tests/cdylib.rs).
    unsafe { astroport_bridge::set_engines(Box::leak(Box::new(std::mem::zeroed())), Box::leak(Box::new(std::mem::zeroed()))) };

    // Before binding: every call parks in the wait stub.
    let early = [
        std::thread::spawn(move || format(forwarder)),
        std::thread::spawn(move || format(memory(0))),
        std::thread::spawn(move || call_mixed(memory(1)).to_string()),
    ];
    std::thread::sleep(Duration::from_millis(300));
    assert!(early.iter().all(|call| !call.is_finished()), "a call returned before ready");

    entry::bind(EXPORT, libc::snprintf as *mut c_void);
    entry::bind(EXPORT_COUNT, libc::snprintf as *mut c_void);
    entry::bind(EXPORT_COUNT + 1, mixed as *mut c_void);
    entry::publish_ready();
    let early: Vec<String> = early.into_iter().map(|call| call.join().unwrap()).collect();
    assert_eq!(early, [FORMATTED, FORMATTED, "42"]);

    // After binding: two jumps, no stub.
    assert_eq!(format(forwarder), FORMATTED);
    assert_eq!(format(memory(0)), FORMATTED);
    assert_eq!(call_mixed(memory(1)), 42.0);
}
