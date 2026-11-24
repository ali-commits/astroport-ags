//! The whole start sequence (design.md 3.4) and the running bridge against fake engine tables:
//! donation, adoption check, slot binding, event exports and pump, pad relay, limiter hooks,
//! live settings, pad audio, shutdown, window close.

use astroport_bridge::abi::*;
use astroport_bridge::{entry, memory, nids, EXTERNAL_RENDERER, HOOKS};
use std::arch::naked_asm;
use std::collections::VecDeque;
use std::ffi::{c_char, c_int, c_void, CStr};
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());
static SHUTDOWN: Mutex<Option<unsafe extern "C" fn()>> = Mutex::new(None);
static QUIT: AtomicBool = AtomicBool::new(false);
/// (ident, filter, data) raised on Kyty's proxy queue
type Raised = VecDeque<(usize, i16, isize)>;
static RAISED: (Mutex<Raised>, Condvar) = (Mutex::new(VecDeque::new()), Condvar::new());

fn log(line: String) {
    LOG.lock().unwrap().push(line);
}

fn count(line: &str) -> usize {
    LOG.lock().unwrap().iter().filter(|logged| *logged == line).count()
}

fn wait_for(line: &str) {
    let start = Instant::now();
    while count(line) == 0 {
        assert!(start.elapsed() < Duration::from_secs(5), "missing {line:?} in {:#?}", LOG.lock().unwrap());
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn raise(ident: usize, filter: i16, data: isize) {
    RAISED.0.lock().unwrap().push_back((ident, filter, data));
    RAISED.1.notify_all();
}

// ---- fake Kyty ----

unsafe extern "C" fn run(config: *const KytyEmbedConfig, hooks: *const KytyEmbedHooks) -> c_int {
    let config = &*config;
    let app0 = CStr::from_ptr(config.app0_dir).to_str().unwrap();
    log(format!("run {}x{} interval {} app0 {app0}", config.width, config.height, config.low_latency_interval_us));
    // Kyty's ReserveGuestRegions takes every free gap of its regions.
    for (base, _) in memory::OWNED {
        let flags = libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED_NOREPLACE;
        assert_ne!(libc::mmap(base as *mut c_void, 4096, libc::PROT_NONE, flags, -1, 0), libc::MAP_FAILED);
    }
    ((*hooks).reserved.unwrap())();
    std::thread::sleep(Duration::from_millis(100)); // a guest call arrives before ready
    ((*hooks).ready.unwrap())();
    while !QUIT.load(SeqCst) {
        std::thread::sleep(Duration::from_millis(2));
    }
    0
}

extern "C" fn guest_function(a: u64, b: u64) -> u64 {
    a * 1000 + b
}

extern "C" fn graphics_function(a: u64, b: u64) -> u64 {
    a * 100 + b
}

extern "C" fn memory_function(a: u64, b: u64) -> u64 {
    a * 10 + b
}

const GRAPHICS_NID: &str = "23LRUSvYu1M";

unsafe extern "C" fn find(nid: *const c_char) -> *mut c_void {
    let nid = CStr::from_ptr(nid).to_str().unwrap();
    assert_eq!(nid.len(), 11);
    match nid {
        GRAPHICS_NID => graphics_function as *mut c_void,
        _ if nid == nids::SLOTS[nids::EXPORT_COUNT].nid => memory_function as *mut c_void,
        _ => guest_function as *mut c_void,
    }
}

unsafe extern "C" fn no_fault(_address: u64, _access: c_int, _instruction: u64) -> c_int {
    0
}

unsafe extern "C" fn flush() {
    log("flush".into());
}

unsafe extern "C" fn user_id() -> c_int {
    1
}

unsafe extern "C" fn pad_audio(_: u32, _: c_int, _: *const c_void, frames: u32, _: u32, _: c_int, _: f32) -> u64 {
    frames as u64
}

unsafe extern "C" fn set_low_latency_interval(us: u32) {
    log(format!("low_latency {us}"));
}

unsafe extern "C" fn set_window_size(width: u32, height: u32) {
    log(format!("window_size {width} {height}"));
}

extern "sysv64" fn pad_init() -> c_int {
    // No slot is bound before the pad is open: READY follows the binding at once.
    let early = std::thread::spawn(|| unsafe { call(slot(GRAPHICS_NID), 6, 7, 0) });
    std::thread::sleep(Duration::from_millis(50));
    assert!(!early.is_finished(), "a slot was bound before the pad was opened");
    0
}

extern "sysv64" fn pad_open(_user: c_int, _type: c_int, _index: c_int, _param: *const c_void) -> c_int {
    -1 // already open: the bridge falls back to the handle lookup
}

extern "sysv64" fn pad_get_handle(user: c_int, _type: c_int, _index: c_int) -> c_int {
    user + 4
}

unsafe extern "sysv64" fn pad_read_state(_handle: c_int, data: *mut PadData) -> c_int {
    (*data).buttons = 0x4000;
    (*data).connected = 1;
    0
}

unsafe extern "sysv64" fn pad_set_vibration(handle: c_int, motors: *const [u8; 2]) -> c_int {
    log(format!("vibration {handle} {:?}", *motors));
    0
}

unsafe extern "sysv64" fn pad_set_light_bar(_handle: c_int, colour: *const [u8; 4]) -> c_int {
    log(format!("light_bar {:?}", *colour));
    0
}

extern "sysv64" fn pad_reset_light_bar(_handle: c_int) -> c_int {
    0
}

unsafe extern "sysv64" fn pad_set_trigger_effect(_handle: c_int, parameter: *const [u8; 120]) -> c_int {
    log(format!("trigger mask {}", (*parameter)[0]));
    0
}

unsafe extern "sysv64" fn equeue_create(queue: *mut i64, _name: *const c_char) -> c_int {
    *queue = 77;
    0
}

unsafe extern "sysv64" fn equeue_wait(queue: i64, events: *mut KernelEvent, capacity: c_int, count: *mut c_int, _timeout: *const u32) -> c_int {
    assert_eq!((queue, capacity), (77, 16));
    let mut raised = RAISED.1.wait_while(RAISED.0.lock().unwrap(), |raised| raised.is_empty()).unwrap();
    let (ident, filter, data) = raised.pop_front().unwrap();
    *events = KernelEvent { ident, filter, flags: 0, fflags: 0, data, udata: std::ptr::null_mut() };
    *count = 1;
    0
}

extern "sysv64" fn equeue_add_user_event(_queue: i64, _id: c_int) -> c_int {
    0
}

extern "sysv64" fn equeue_trigger_user_event(_queue: i64, id: c_int, _udata: *mut c_void) -> c_int {
    raise(id as usize, -11, 0);
    0
}

extern "sysv64" fn kyty_agc_add_eq_event(queue: i64, id: c_int, _udata: *mut c_void) -> c_int {
    log(format!("proxy eop {queue} {id}"));
    0
}

extern "sysv64" fn kyty_video_out_add_flip_event(queue: i64, handle: c_int, _udata: *mut c_void) -> c_int {
    log(format!("proxy flip {queue} {handle}"));
    0
}

// ---- fake host ----

unsafe extern "C" fn donate(base: u64, bytes: u64) -> u64 {
    log(format!("donate {base:#x} {bytes:#x}"));
    base
}

unsafe extern "C" fn event_add(queue: u64, kind: c_int, ident: u64, user_data: *mut c_void) -> c_int {
    log(format!("event_add {queue} {kind} {ident} {user_data:?}"));
    if queue == 666 {
        0x80020009u32 as c_int
    } else {
        0
    }
}

unsafe extern "C" fn event_delete(queue: u64, kind: c_int, ident: u64) -> c_int {
    log(format!("event_delete {queue} {kind} {ident}"));
    0
}

unsafe extern "C" fn event_deliver(kind: c_int, ident: u64, data: u64) {
    log(format!("deliver {kind} {ident} {data}"));
}

unsafe extern "C" fn pad_publish(port: c_int, input: *const AstroportPadInput) {
    let line = format!("publish {port} {:#x} {} {}", (*input).buttons, (*input).connected, (*input).has_motion);
    if count(&line) == 0 {
        log(line);
    }
}

unsafe extern "C" fn pad_fetch_output(_port: c_int, sequence: *mut u32, output: *mut AstroportPadOutput) -> c_int {
    if *sequence != 0 {
        return 0;
    }
    *sequence = 1;
    ((*output).rumble_large, (*output).audio_rumble_small) = (10, 20);
    ((*output).light_bar_valid, (*output).light_bar) = (1, [1, 2, 3]);
    ((*output).trigger_set, (*output).trigger[1]) = ([0, 1], [9; 56]);
    1
}

unsafe extern "C" fn request_exit(code: c_int) {
    log(format!("request_exit {code}"));
}

unsafe extern "C" fn on_shutdown(callback: unsafe extern "C" fn()) {
    *SHUTDOWN.lock().unwrap() = Some(callback);
}

unsafe extern "C" fn install_renderer(renderer: *const ExternalRenderer) {
    assert!(std::ptr::eq(renderer, &EXTERNAL_RENDERER));
    log("install_renderer".into());
}

/// A forwarder with the slot index as first argument.
#[unsafe(naked)]
unsafe extern "C" fn call(index: usize, a: u64, b: u64, c: u64) -> u64 {
    naked_asm!("mov r11, rdi", "mov rdi, rsi", "mov rsi, rdx", "mov rdx, rcx", "jmp {entry}", entry = sym entry::astroport_entry)
}

fn slot(nid: &str) -> usize {
    nids::SLOTS.iter().position(|export| export.nid == nid).unwrap()
}

#[test]
fn bridge_runs_against_fake_engines() {
    let fns: &'static KytyEmbedFns = Box::leak(Box::new(KytyEmbedFns {
        pad_init: pad_init as *const c_void,
        pad_open: pad_open as *const c_void,
        pad_get_handle: pad_get_handle as *const c_void,
        pad_read_state: pad_read_state as *const c_void,
        pad_set_vibration: pad_set_vibration as *const c_void,
        pad_set_light_bar: pad_set_light_bar as *const c_void,
        pad_reset_light_bar: pad_reset_light_bar as *const c_void,
        pad_set_trigger_effect: pad_set_trigger_effect as *const c_void,
        equeue_create: equeue_create as *const c_void,
        equeue_wait: equeue_wait as *const c_void,
        equeue_add_user_event: equeue_add_user_event as *const c_void,
        equeue_trigger_user_event: equeue_trigger_user_event as *const c_void,
        agc_add_eq_event: kyty_agc_add_eq_event as *const c_void,
        video_out_add_flip_event: kyty_video_out_add_flip_event as *const c_void,
    }));
    let kyty: &'static KytyEmbedApi = Box::leak(Box::new(KytyEmbedApi {
        abi: KYTY_EMBED_ABI,
        size: size_of::<KytyEmbedApi>() as u32,
        pad_data_size: 120,
        fns,
        run: Some(run),
        find: Some(find),
        fault: Some(no_fault),
        quit: None,
        flush: Some(flush),
        user_id: Some(user_id),
        pad_audio: Some(pad_audio),
        set_low_latency_interval: Some(set_low_latency_interval),
        set_window_size: Some(set_window_size),
    }));
    let host: &'static AstroportHostApi = Box::leak(Box::new(AstroportHostApi {
        abi: ASTROPORT_HOST_ABI,
        size: size_of::<AstroportHostApi>() as u32 + 8, // a newer host with one more entry
        donate: Some(donate),
        event_add: Some(event_add),
        event_delete: Some(event_delete),
        event_deliver: Some(event_deliver),
        pad_publish: Some(pad_publish),
        pad_fetch_output: Some(pad_fetch_output),
        request_exit: Some(request_exit),
        on_shutdown: Some(on_shutdown),
        install_renderer: Some(install_renderer),
    }));
    std::env::set_var("ASTROPORT_FPS_LIMIT", "30");
    std::env::set_var("ASTROPORT_WIDTH", "1920");
    std::env::set_var("ASTROPORT_HEIGHT", "1080");
    std::env::set_var("ASTROPORT_APP0", "/game/app0");
    std::env::set_var("APS5_PAD_VIBRATION_GAIN", "1.5");

    unsafe {
        // Phase 1 returns once Kyty holds the address space; ready comes later.
        astroport_bridge::start(kyty, host);
        assert_eq!(*LOG.lock().unwrap(), [
            "donate 0x200000000 0x5ffff0000",
            "donate 0xdc00000000 0x1000000000",
            "install_renderer",
            "run 1920x1080 interval 33333 app0 /game/app0",
        ]);
        assert!(SHUTDOWN.lock().unwrap().is_some() && !entry::is_ready());

        // A guest call before ready waits, then reaches the function Kyty's `find` gave.
        let graphics = slot(GRAPHICS_NID);
        assert_eq!(call(graphics, 2, 3, 0), 203, "a slot reaches the function of its own NID");
        assert_eq!(call(graphics + 1, 2, 3, 0), 2003);
        assert!(entry::is_ready());
        let allocate: extern "C" fn(u64, u64) -> u64 = std::mem::transmute(EXTERNAL_RENDERER.memory[0]);
        assert_eq!(allocate(4, 5), 45, "the first memory entry reaches the function of its own NID");
        let next: extern "C" fn(u64, u64) -> u64 = std::mem::transmute(EXTERNAL_RENDERER.memory[1]);
        assert_eq!(next(4, 5), 4005);
        assert!((EXTERNAL_RENDERER.owns)(0x3_0000_0000, 0x1000) && !(EXTERNAL_RENDERER.owns)(0x1000, 1));

        // Event exports: the host registers, the proxy queue learns each id once.
        let (add, delete, add_flip) = (slot("w2rJhmD+dsE"), slot("DL2RXaXOy88"), slot("HXzjK9yI30k"));
        assert_eq!(call(add, 7, 3, 0x1234), 0);
        assert_eq!(call(add, 8, 3, 0), 0);
        assert_eq!(call(add_flip, 7, 3, 0), 0);
        assert_eq!(call(add, 666, 9, 0) as u32, 0x80020009, "a host error is returned unchanged");
        assert_eq!(call(delete, 7, 3, 0), 0);
        assert_eq!((count("event_add 7 0 3 0x1234"), count("event_add 7 1 3 0x0"), count("event_delete 7 0 3")), (1, 1, 1));
        assert_eq!((count("proxy eop 77 3"), count("proxy flip 77 3"), count("proxy eop 77 9")), (1, 1, 0));

        // Pump: Kyty's events reach the host.
        raise(3, -14, 99);
        raise(1, -13, 5);
        wait_for("deliver 0 3 99");
        wait_for("deliver 1 1 5");

        // Pad relay: input published, output differences applied on Kyty's pad (handle 1 + 4).
        wait_for("publish 0 0x4000 1 1");
        wait_for("vibration 5 [10, 20]");
        wait_for("light_bar [1, 2, 3, 0]");
        wait_for("trigger mask 2");

        // Limiter hooks: 30 FPS from the environment, 60 while a video plays.
        let (gate, presented) = (HOOKS.present_gate.unwrap(), HOOKS.presented.unwrap());
        const F: i64 = 1_000_000_000;
        assert_eq!(gate(1000, F, 1), 0);
        presented(1000, F);
        assert_eq!(gate(2000, F, 1), 1000 + 33_333_334);
        (EXTERNAL_RENDERER.video_active)(1);
        assert_eq!(gate(2000, F, 1), 0, "the cadence restarts with the effective limit");
        presented(2000, F);
        assert_eq!(gate(3000, F, 1), 2000 + 16_666_667);
        (EXTERNAL_RENDERER.video_active)(0);
        assert_eq!(gate(3000, F, 1), 0);
        let intervals: Vec<String> = LOG.lock().unwrap().iter().filter(|line| line.starts_with("low_latency")).cloned().collect();
        assert_eq!(intervals, ["low_latency 33333", "low_latency 16666", "low_latency 33333"]);

        // Live settings through the session socket hook.
        assert_eq!((EXTERNAL_RENDERER.haptic_gain)(), 1.5);
        let request = c"{\"type\":\"settings.apply\",\"id\":4,\"values\":{\"controller.haptics\":250,\"display.resolution\":\"2560x1440\",\"display.fps\":60}}";
        let reply = CStr::from_ptr((HOOKS.live_setting.unwrap())(request.as_ptr())).to_str().unwrap();
        // No patcher in LD_AUDIT: the frame cap cannot switch its patch group.
        assert_eq!(reply, "{\"type\":\"settings.applied\",\"id\":4,\"applied\":{\"display.resolution\":\"2560x1440\"},\"values\":{\"controller.haptics\":250},\"refused\":{\"display.fps\":\"Applies at the next start\"}}");
        assert_eq!(((EXTERNAL_RENDERER.haptic_gain)(), count("window_size 2560 1440")), (2.5, 1));

        // Pad audio reaches Kyty until shutdown starts.
        let silence = [0i16; 960];
        assert_eq!((EXTERNAL_RENDERER.pad_audio)(48000, 0, silence.as_ptr().cast(), 480, 2, 0, 1.0), 480);
        let shutdown = SHUTDOWN.lock().unwrap().unwrap();
        shutdown();
        shutdown();
        assert_eq!(count("flush"), 1);
        assert_eq!((EXTERNAL_RENDERER.pad_audio)(48000, 0, silence.as_ptr().cast(), 480, 2, 0, 1.0), 0);

        // The player closes the window: `run` returns and the host is asked to exit.
        QUIT.store(true, SeqCst);
        wait_for("request_exit 0");
    }
}
