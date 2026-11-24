//! Fault handler with chaining (design.md 5.3), with a prior handler installed first:
//! an unowned page reaches the prior handler, an owned page reaches the fake `fault`.

use astroport_bridge::abi::{AstroportHostApi, KytyEmbedApi};
use astroport_bridge::{entry, fault, memory};
use libc::{c_int, c_void, siginfo_t};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering::SeqCst};

static PRIOR_SAW: AtomicUsize = AtomicUsize::new(0);
static RENDERER_SAW: AtomicUsize = AtomicUsize::new(0);
static RENDERER_ACCESS: AtomicI32 = AtomicI32::new(-1);
static RENDERER_REFUSES: AtomicBool = AtomicBool::new(false);

unsafe fn unprotect(address: usize) {
    libc::mprotect((address & !0xFFF) as *mut c_void, 4096, libc::PROT_READ | libc::PROT_WRITE);
}

unsafe extern "C" fn prior(_signal: c_int, info: *mut siginfo_t, _context: *mut c_void) {
    PRIOR_SAW.store((*info).si_addr() as usize, SeqCst);
    unprotect((*info).si_addr() as usize);
}

unsafe extern "C" fn renderer_fault(address: u64, access: c_int, instruction: u64) -> c_int {
    RENDERER_SAW.store(address as usize, SeqCst);
    RENDERER_ACCESS.store(access, SeqCst);
    assert_ne!(instruction, 0);
    if RENDERER_REFUSES.load(SeqCst) {
        return 0;
    }
    unprotect(address as usize);
    1
}

unsafe fn pages(address: usize, count: usize, flags: c_int) -> usize {
    let flags = libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | flags;
    let base = libc::mmap(address as *mut c_void, count * 4096, libc::PROT_NONE, flags, -1, 0);
    assert_ne!(base, libc::MAP_FAILED);
    base as usize
}

/// A launcher can pass an ignored SIGSEGV on through exec: a real fault must still end the
/// process instead of retrying forever. The fault happens in a child run of this test.
#[test]
fn a_fault_ends_the_process_when_the_prior_action_ignores_it() {
    const CHILD: &str = "ASTROPORT_TEST_IGNORED_FAULT";
    if std::env::var_os(CHILD).is_none() {
        let test = ["--exact", "a_fault_ends_the_process_when_the_prior_action_ignores_it"];
        let mut child = std::process::Command::new("timeout");
        let status = child.arg("10").arg(std::env::current_exe().unwrap()).args(test).env(CHILD, "1").status().unwrap();
        assert!(!status.success() && status.code() != Some(124), "the faulting child spun or survived: {status:?}");
        return;
    }
    unsafe {
        libc::signal(libc::SIGSEGV, libc::SIG_IGN);
        astroport_bridge::set_engines(Box::leak(Box::new(std::mem::zeroed())), Box::leak(Box::new(std::mem::zeroed())));
        fault::install();
        std::ptr::write_volatile(pages(0, 1, 0) as *mut u8, 7);
    }
    std::process::exit(0); // not reached: exit code 0 fails the parent
}

#[test]
fn owned_faults_reach_the_renderer_and_the_rest_the_prior_handler() {
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = prior as *const () as usize;
        action.sa_flags = libc::SA_SIGINFO;
        assert_eq!(libc::sigaction(libc::SIGSEGV, &action, std::ptr::null_mut()), 0);

        let kyty: &'static mut KytyEmbedApi = Box::leak(Box::new(std::mem::zeroed()));
        kyty.fault = Some(renderer_fault);
        let host: &'static AstroportHostApi = Box::leak(Box::new(std::mem::zeroed()));
        astroport_bridge::set_engines(kyty, host);
        fault::install();

        let owned = pages(memory::DONATED[1].0, 4, libc::MAP_FIXED_NOREPLACE);
        let foreign = pages(0, 1, 0);
        let write = |address: usize| std::ptr::write_volatile(address as *mut u8, 7);

        // Not ready yet: even an owned page goes to the prior handler.
        write(owned);
        assert_eq!((PRIOR_SAW.load(SeqCst), RENDERER_SAW.load(SeqCst), fault::counts()), (owned, 0, (0, 1)));

        entry::publish_ready();
        write(owned + 4096 + 5);
        assert_eq!((RENDERER_SAW.load(SeqCst), RENDERER_ACCESS.load(SeqCst)), (owned + 4096 + 5, 1));
        assert_eq!((PRIOR_SAW.load(SeqCst), fault::counts()), (owned, (1, 1)), "handled: the prior handler is not called");

        assert_eq!(std::ptr::read_volatile((owned + 2 * 4096) as *const u8), 0);
        assert_eq!((RENDERER_SAW.load(SeqCst), RENDERER_ACCESS.load(SeqCst), fault::counts()), (owned + 2 * 4096, 0, (2, 1)));

        write(foreign + 9);
        assert_eq!((PRIOR_SAW.load(SeqCst), RENDERER_SAW.load(SeqCst), fault::counts()), (foreign + 9, owned + 2 * 4096, (2, 2)));

        // The renderer does not know the page: passed on.
        RENDERER_REFUSES.store(true, SeqCst);
        write(owned + 3 * 4096);
        assert_eq!((RENDERER_SAW.load(SeqCst), PRIOR_SAW.load(SeqCst), fault::counts()), (owned + 3 * 4096, owned + 3 * 4096, (2, 3)));
    }
}
