//! Naked `astroport_entry`, wait stub, slot array, 17 memory thunks (design.md 4.1, 4.3).

use crate::nids::{EXPORT_COUNT, MEMORY_COUNT, SLOTS as NAMES};
use std::arch::naked_asm;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

pub const SLOT_COUNT: usize = EXPORT_COUNT + MEMORY_COUNT;

/// One target per slot; every slot starts at the wait stub.
static SLOTS: [AtomicPtr<()>; SLOT_COUNT] = [const { AtomicPtr::new(wait_stub as *mut ()) }; SLOT_COUNT];

static READY: AtomicBool = AtomicBool::new(false);
static READY_LOCK: Mutex<()> = Mutex::new(());
static READY_CHANGED: Condvar = Condvar::new();

/// Jump to SLOTS[r11]. Position independent, touches only r11 and the flags.
macro_rules! dispatch {
    () => {
        "shl r11, 3\nadd r11, qword ptr [rip + {slots}@GOTPCREL]\njmp qword ptr [r11]"
    };
}

/// The forwarders load the slot index into r11 and jump here.
#[unsafe(naked)]
#[export_name = astroport_entry_symbol!()]
pub unsafe extern "C" fn astroport_entry() {
    naked_asm!(dispatch!(), slots = sym SLOTS)
}

/// Target of every slot before ready: keeps all argument registers (rax = vector count of a
/// variadic call), waits, then dispatches again. The dispatch left the slot's address in r11;
/// the index is recovered from it. 200 bytes realign the stack.
#[unsafe(naked)]
unsafe extern "C" fn wait_stub() {
    naked_asm!(
        "sub r11, qword ptr [rip + {slots}@GOTPCREL]",
        "shr r11, 3",
        "sub rsp, 200",
        "mov [rsp], rdi",
        "mov [rsp + 8], rsi",
        "mov [rsp + 16], rdx",
        "mov [rsp + 24], rcx",
        "mov [rsp + 32], r8",
        "mov [rsp + 40], r9",
        "mov [rsp + 48], rax",
        "mov [rsp + 56], r10",
        "mov [rsp + 64], r11",
        "movups [rsp + 72], xmm0",
        "movups [rsp + 88], xmm1",
        "movups [rsp + 104], xmm2",
        "movups [rsp + 120], xmm3",
        "movups [rsp + 136], xmm4",
        "movups [rsp + 152], xmm5",
        "movups [rsp + 168], xmm6",
        "movups [rsp + 184], xmm7",
        "mov rdi, r11",
        "call {wait}",
        "mov rdi, [rsp]",
        "mov rsi, [rsp + 8]",
        "mov rdx, [rsp + 16]",
        "mov rcx, [rsp + 24]",
        "mov r8, [rsp + 32]",
        "mov r9, [rsp + 40]",
        "mov rax, [rsp + 48]",
        "mov r10, [rsp + 56]",
        "mov r11, [rsp + 64]",
        "movups xmm0, [rsp + 72]",
        "movups xmm1, [rsp + 88]",
        "movups xmm2, [rsp + 104]",
        "movups xmm3, [rsp + 120]",
        "movups xmm4, [rsp + 136]",
        "movups xmm5, [rsp + 152]",
        "movups xmm6, [rsp + 168]",
        "movups xmm7, [rsp + 184]",
        "add rsp, 200",
        dispatch!(),
        wait = sym wait_ready,
        slots = sym SLOTS,
    )
}

extern "C" fn wait_ready(index: usize) {
    if !crate::started() {
        crate::die("the bridge was not started: engines not loaded");
    }
    let guard = READY_LOCK.lock().unwrap();
    let limit = Duration::from_secs(120);
    let (_guard, result) = READY_CHANGED.wait_timeout_while(guard, limit, |_| !is_ready()).unwrap();
    if result.timed_out() {
        let export = &NAMES[index];
        crate::die(format!("renderer not ready after 120 s; waiting call: {} ({})", export.nid, export.function));
    }
}

pub fn is_ready() -> bool {
    READY.load(Ordering::Acquire)
}

/// Bind every slot first: a waiter dispatches again as soon as this returns.
pub fn publish_ready() {
    let _guard = READY_LOCK.lock().unwrap();
    READY.store(true, Ordering::Release);
    READY_CHANGED.notify_all();
}

pub fn bind(index: usize, target: *mut c_void) {
    SLOTS[index].store(target.cast(), Ordering::Release);
}

macro_rules! memory_thunks {
    ($($position:literal)*) => {[$({
        #[unsafe(naked)]
        unsafe extern "C" fn thunk() {
            naked_asm!("mov r11d, {index}", dispatch!(), index = const EXPORT_COUNT + $position, slots = sym SLOTS)
        }
        thunk as unsafe extern "C" fn()
    }),*]};
}

/// The memory entries of `ExternalRenderer`, in slot order EXPORT_COUNT.. .
pub static MEMORY_THUNKS: [unsafe extern "C" fn(); MEMORY_COUNT] =
    memory_thunks!(0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16);
