//! Three event exports, proxy queue, pump thread (design.md 3.6).
//! The game's queues are host objects; Kyty raises its events on one proxy queue of its own,
//! and the pump hands them to the host.

use crate::abi::KernelEvent;
use crate::{host, kyty, STOP};
use std::ffi::{c_char, c_int, c_void};
use std::mem::transmute;
use std::ptr::{null, null_mut};
use std::sync::atomic::Ordering;
use std::sync::{Mutex, OnceLock};

pub const EOP: c_int = 0;
pub const FLIP: c_int = 1;
const WAKE_EVENT: c_int = 1;

/// Kyty event filter -> host event kind (KERNEL_EVFILT_GRAPHICS, KERNEL_EVFILT_VIDEO_OUT).
pub fn kind_of(filter: i16) -> Option<c_int> {
    match filter {
        -14 => Some(EOP),
        -13 => Some(FLIP),
        _ => None,
    }
}

/// Event ids and video-out handles already registered on the proxy queue.
#[derive(Default)]
pub struct Registry(Vec<(c_int, c_int)>);

impl Registry {
    /// True the first time a kind and id are seen.
    pub fn first(&mut self, kind: c_int, id: c_int) -> bool {
        let new = !self.0.contains(&(kind, id));
        if new {
            self.0.push((kind, id));
        }
        new
    }
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry(Vec::new()));
static PROXY: OnceLock<i64> = OnceLock::new();

type AddOnProxy = unsafe extern "sysv64" fn(queue: i64, id: c_int, udata: *mut c_void) -> c_int;

unsafe fn add(queue: u64, kind: c_int, id: c_int, udata: *mut c_void) -> c_int {
    let result = (host().event_add.unwrap())(queue, kind, id as u32 as u64, udata);
    if result != 0 || !REGISTRY.lock().unwrap().first(kind, id) {
        return result;
    }
    let fns = &*kyty().fns;
    let register: AddOnProxy = transmute(if kind == EOP { fns.agc_add_eq_event } else { fns.video_out_add_flip_event });
    let result = register(*PROXY.get().unwrap(), id, null_mut());
    if result != 0 {
        crate::die(format!("Kyty refused event {id} (kind {kind}) on the proxy queue: {result:#x}"));
    }
    0
}

/// Bound to `w2rJhmD+dsE` instead of Kyty's AgcDriverAddEqEvent.
pub unsafe extern "sysv64" fn agc_add_eq_event(queue: u64, id: c_int, udata: *mut c_void) -> c_int {
    add(queue, EOP, id, udata)
}

/// Bound to `DL2RXaXOy88`: host only, the proxy registration stays.
pub unsafe extern "sysv64" fn agc_delete_eq_event(queue: u64, id: c_int) -> c_int {
    (host().event_delete.unwrap())(queue, EOP, id as u32 as u64)
}

/// Bound to `HXzjK9yI30k` instead of Kyty's VideoOutAddFlipEvent.
pub unsafe extern "sysv64" fn video_out_add_flip_event(queue: u64, handle: c_int, udata: *mut c_void) -> c_int {
    add(queue, FLIP, handle, udata)
}

/// Creates the proxy queue and starts the pump. Call before the three exports are bound.
///
/// # Safety
/// The engines must be set and Kyty's subsystems up.
pub unsafe fn start() {
    let fns = &*kyty().fns;
    let create: unsafe extern "sysv64" fn(*mut i64, *const c_char) -> c_int = transmute(fns.equeue_create);
    let add_user_event: unsafe extern "sysv64" fn(i64, c_int) -> c_int = transmute(fns.equeue_add_user_event);
    let mut proxy = 0;
    if create(&mut proxy, c"astroport proxy".as_ptr()) != 0 || add_user_event(proxy, WAKE_EVENT) != 0 {
        crate::die("the Kyty proxy event queue could not be created");
    }
    PROXY.set(proxy).unwrap();
    std::thread::Builder::new().name("astroport pump".into()).spawn(move || pump(proxy)).unwrap();
}

/// Wakes the pump so that it sees the stop flag.
pub unsafe fn stop() {
    if let Some(&proxy) = PROXY.get() {
        let trigger: unsafe extern "sysv64" fn(i64, c_int, *mut c_void) -> c_int = transmute((*kyty().fns).equeue_trigger_user_event);
        trigger(proxy, WAKE_EVENT, null_mut());
    }
}

fn pump(proxy: i64) {
    unsafe {
        let wait: unsafe extern "sysv64" fn(i64, *mut KernelEvent, c_int, *mut c_int, *const u32) -> c_int =
            transmute((*kyty().fns).equeue_wait);
        let deliver = host().event_deliver.unwrap();
        let mut events: [KernelEvent; 16] = std::mem::zeroed();
        while !STOP.load(Ordering::Relaxed) {
            let mut count = 0;
            let result = wait(proxy, events.as_mut_ptr(), 16, &mut count, null());
            if STOP.load(Ordering::Relaxed) {
                return;
            }
            if result != 0 {
                crate::die(format!("event pump stopped: KernelWaitEqueue returned {result:#x}"));
            }
            for event in &events[..count.clamp(0, 16) as usize] {
                if let Some(kind) = kind_of(event.filter) {
                    deliver(kind, event.ident as u64, event.data as u64);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_registers_each_kind_and_id_once() {
        let mut registry = Registry::default();
        assert!(registry.first(EOP, 3));
        assert!(!registry.first(EOP, 3));
        assert!(registry.first(FLIP, 3), "a video-out handle is not an event id");
        assert!(registry.first(EOP, 4));
        assert!(!registry.first(FLIP, 3));
    }

    #[test]
    fn filters_map_to_host_kinds() {
        assert_eq!(kind_of(-14), Some(EOP));
        assert_eq!(kind_of(-13), Some(FLIP));
        assert_eq!(kind_of(-11), None, "the wake event is not delivered");
    }
}
