//! SIGSEGV / SIGBUS action and chaining (design.md 5.3).

use crate::{entry, memory};
use libc::{c_int, c_void, sigaction, siginfo_t, SA_ONSTACK, SA_RESTART, SA_SIGINFO, SIGBUS, SIGSEGV, SIG_DFL, SIG_IGN};
use std::sync::atomic::{AtomicU64, Ordering};

static mut PREVIOUS: [sigaction; 2] = unsafe { std::mem::zeroed() };
static HANDLED: AtomicU64 = AtomicU64::new(0);
static PASSED_ON: AtomicU64 = AtomicU64::new(0);

/// (handled by the renderer, passed on to the previous action)
pub fn counts() -> (u64, u64) {
    (HANDLED.load(Ordering::Relaxed), PASSED_ON.load(Ordering::Relaxed))
}

/// # Safety
/// Replaces the process's SIGSEGV and SIGBUS actions; call once.
pub unsafe fn install() {
    for (index, signal) in [SIGSEGV, SIGBUS].into_iter().enumerate() {
        let mut action: sigaction = std::mem::zeroed();
        action.sa_sigaction = handler as *const () as usize;
        action.sa_flags = SA_SIGINFO | SA_ONSTACK | SA_RESTART;
        libc::sigemptyset(&mut action.sa_mask);
        if libc::sigaction(signal, &action, &raw mut PREVIOUS[index]) != 0 {
            crate::die(format!("sigaction({signal}) failed"));
        }
    }
}

// Async-signal-safe: atomics, the renderer's fault entry, the previous action. No allocation.
unsafe extern "C" fn handler(signal: c_int, info: *mut siginfo_t, context: *mut c_void) {
    let address = (*info).si_addr() as usize;
    if entry::is_ready() && memory::owns(address, 1) {
        let errno = *libc::__errno_location();
        let registers = &(*(context as *const libc::ucontext_t)).uc_mcontext.gregs;
        let error = registers[libc::REG_ERR as usize];
        let access = if error & 0x10 != 0 { 2 } else { (error >> 1 & 1) as c_int };
        let handled = (crate::kyty().fault.unwrap())(address as u64, access, registers[libc::REG_RIP as usize] as u64);
        *libc::__errno_location() = errno;
        if handled != 0 {
            HANDLED.fetch_add(1, Ordering::Relaxed);
            return;
        }
    }
    PASSED_ON.fetch_add(1, Ordering::Relaxed);
    let previous = PREVIOUS[(signal == SIGBUS) as usize];
    match previous.sa_sigaction {
        // The retried instruction faults again and terminates the process, as the kernel does
        // for a synchronous fault even when the signal is ignored.
        SIG_DFL | SIG_IGN => {
            libc::signal(signal, SIG_DFL);
        }
        action if previous.sa_flags & SA_SIGINFO != 0 => {
            std::mem::transmute::<usize, extern "C" fn(c_int, *mut siginfo_t, *mut c_void)>(action)(signal, info, context)
        }
        action => std::mem::transmute::<usize, extern "C" fn(c_int)>(action)(signal),
    }
}
