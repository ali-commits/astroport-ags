//! M1 smoke test of the Kyty seam (design.md 2.1, 3.1): loads libkyty_renderer.so alone, no AnyPS5
//! and no bridge library. Usage: `embed_smoke <libkyty_renderer.so> [app0-dir]`.
//!
//! Two runs make the M1 check (the library is loaded with dlopen here, so glibc needs room for
//! its static TLS block; the product loads it by DT_NEEDED):
//!
//! ```sh
//! export GLIBC_TUNABLES=glibc.rtld.optional_static_tls=1048576 DISABLE_LSFGVK=1
//! timeout 120 embed_smoke build/kyty/libkyty_renderer.so
//! KYTY_PRESENT_ABS_VBLANK=1 timeout 120 embed_smoke build/kyty/libkyty_renderer.so
//! ```
//!
//! The first run answers the present gate with "now" and gives a live-settings hook; the second
//! (absolute vblank timeline) answers with a tick and gives no hook. The window is looked up with
//! `xprop`, so the example selects SDL's X11 driver; that the window is black is not checked.
//!
//! It has its own copy of the C table (engines/kyty/src/embed/kytyEmbed.h) and does not use the
//! bridge crate, so the bridge's start-up constructor is never linked into it.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[repr(C)]
struct Config {
    size: u32,
    width: u32,
    height: u32,
    vblank_hz: u32,
    vibration_intensity: u32,
    rumble_scale: u32,
    rumble_switch_floor: u32,
    low_latency_interval_us: u32,
    dlss_preset: u32,
    upscale_pass_hash: u64,
    fullscreen: u8,
    present_mode: u8,
    shader_optimization: u8,
    wayland_native: u8,
    low_latency: u8,
    dlss: u8,
    dlss_reverse_depth: u8,
    pad_prefer: u8,
    ds_rumble: u8,
    fsr_fg: u8,
    app0_dir: *const c_char,
    window_title: *const c_char,
    dlss_dir: *const c_char,
    ngx_data_dir: *const c_char,
    fsr_dir: *const c_char,
}

#[repr(C)]
struct Hooks {
    size: u32,
    reserved: Option<extern "C" fn()>,
    ready: Option<extern "C" fn()>,
    present_gate: Option<extern "C" fn(i64, i64, c_int) -> i64>,
    presented: Option<extern "C" fn(i64, i64)>,
    live_setting: Option<extern "C" fn(*const c_char) -> *const c_char>,
}

type PadInit = extern "sysv64" fn() -> c_int;
type PadOpen = extern "sysv64" fn(c_int, c_int, c_int, *const c_void) -> c_int;
type PadReadState = extern "sysv64" fn(c_int, *mut u8) -> c_int;
type EqueueCreate = extern "sysv64" fn(*mut i64, *const c_char) -> c_int;
type EqueueWait = extern "sysv64" fn(i64, *mut KernelEvent, c_int, *mut c_int, *const u32) -> c_int;
type EqueueAddUserEvent = extern "sysv64" fn(i64, c_int) -> c_int;
type EqueueTriggerUserEvent = extern "sysv64" fn(i64, c_int, *mut c_void) -> c_int;

#[repr(C)]
struct Fns {
    pad_init: PadInit,
    pad_open: PadOpen,
    pad_get_handle: *const c_void,
    pad_read_state: PadReadState,
    pad_set_vibration: *const c_void,
    pad_set_light_bar: *const c_void,
    pad_reset_light_bar: *const c_void,
    pad_set_trigger_effect: *const c_void,
    equeue_create: EqueueCreate,
    equeue_wait: EqueueWait,
    equeue_add_user_event: EqueueAddUserEvent,
    equeue_trigger_user_event: EqueueTriggerUserEvent,
    agc_add_eq_event: *const c_void,
    video_out_add_flip_event: *const c_void,
}

#[repr(C)]
struct Api {
    abi: u32,
    size: u32,
    pad_data_size: u32,
    fns: *const Fns,
    run: extern "C" fn(*const Config, *const Hooks) -> c_int,
    find: extern "C" fn(*const c_char) -> *mut c_void,
    fault: extern "C" fn(u64, c_int, u64) -> c_int,
    quit: extern "C" fn(),
    flush: extern "C" fn(),
    user_id: extern "C" fn() -> c_int,
    pad_audio: extern "C" fn(u32, c_int, *const c_void, u32, u32, c_int, f32) -> u64,
    set_low_latency_interval: extern "C" fn(u32),
    set_window_size: extern "C" fn(u32, u32),
}

/// Kyty's `Libs::LibKernel::EventQueue::KernelEvent`.
#[repr(C)]
#[derive(Default)]
struct KernelEvent {
    ident: u64,
    filter: i16,
    flags: u16,
    fflags: u32,
    data: i64,
    udata: u64,
}

/// Order in which the hooks arrived: 0 none, 1 reserved, 2 reserved then ready, 9 wrong order.
static STATE: AtomicU32 = AtomicU32::new(0);

extern "C" fn reserved() {
    let _ = STATE.compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst);
}

extern "C" fn ready() {
    let next = if STATE.load(Ordering::SeqCst) == 1 { 2 } else { 9 };
    STATE.store(next, Ordering::SeqCst);
}

/// Calls of the present gate. It answers "present now", or with GATE_TICK "not before now + 100 us".
static GATE_CALLS: AtomicU32 = AtomicU32::new(0);
static GATE_TICK: AtomicBool = AtomicBool::new(false);

extern "C" fn present_gate(now: i64, ticks_per_s: i64, _flip_ready: c_int) -> i64 {
    if now > 0 && ticks_per_s == 1_000_000_000 {
        GATE_CALLS.fetch_add(1, Ordering::Relaxed);
    }
    if GATE_TICK.load(Ordering::Relaxed) { now + 100_000 } else { 0 }
}

static API: AtomicUsize = AtomicUsize::new(0);
static LIVE_CALLS: AtomicU32 = AtomicU32::new(0);

/// First call: applies a 30 FPS limit as the bridge would. Later calls: a reply that is not JSON.
extern "C" fn live_setting(apply: *const c_char) -> *const c_char {
    let apply = unsafe { CStr::from_ptr(apply) }.to_string_lossy();
    if !apply.contains("settings.apply") {
        fail(&format!("live_setting got {apply}"));
    }
    if LIVE_CALLS.fetch_add(1, Ordering::SeqCst) > 0 {
        return c"not json".as_ptr();
    }
    let api = unsafe { &*(API.load(Ordering::SeqCst) as *const Api) };
    (api.set_low_latency_interval)(33_333);
    c"{\"type\":\"settings.result\",\"applied\":{\"display.fps\":30}}".as_ptr()
}

fn send(fd: c_int, packet: &str) {
    let sent = unsafe { libc::send(fd, packet.as_ptr().cast(), packet.len(), libc::MSG_NOSIGNAL) };
    check(sent == packet.len() as isize, &format!("session socket: sent {packet}"));
}

/// One packet; None after 2 s without one; empty when the peer closed.
fn receive(fd: c_int) -> Option<String> {
    let mut poller = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    if unsafe { libc::poll(&mut poller, 1, 2000) } <= 0 {
        return None;
    }
    let mut bytes = [0u8; 8192];
    let count = unsafe { libc::recv(fd, bytes.as_mut_ptr().cast(), bytes.len(), 0) };
    Some(String::from_utf8_lossy(&bytes[..count.max(0) as usize]).into_owned())
}

/// The session socket Kyty's transport accepts: a 0600 SOCK_SEQPACKET socket in a 0700 folder.
fn listen() -> c_int {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let dir = std::env::temp_dir().join(format!("embed_smoke_session.{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
    let path = dir.join("s");
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC, 0) };
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_family = libc::AF_UNIX as u16;
    for (to, from) in address.sun_path.iter_mut().zip(path.to_str().unwrap().bytes()) {
        *to = from as c_char;
    }
    let size = std::mem::size_of::<libc::sockaddr_un>() as u32;
    let bound = unsafe { libc::bind(fd, (&address as *const libc::sockaddr_un).cast(), size) };
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    if bound != 0 || unsafe { libc::listen(fd, 1) } != 0 {
        fail("session socket: bind or listen");
    }
    std::env::set_var("AGS_SESSION_SOCKET", &path);
    std::env::set_var("AGS_SESSION_PROTOCOL", "2");
    fd
}

/// The X11 window of this process and its title, by `xprop`.
fn window_title() -> Option<String> {
    let xprop = |args: &[&str]| {
        let output = std::process::Command::new("xprop").args(args).output();
        String::from_utf8_lossy(&output.unwrap_or_else(|e| fail(&format!("xprop: {e}"))).stdout).into_owned()
    };
    let pid = format!("= {}\n", std::process::id());
    let clients = xprop(&["-root", "_NET_CLIENT_LIST"]);
    clients
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|id| id.starts_with("0x"))
        .find(|id| xprop(&["-id", id, "_NET_WM_PID"]).ends_with(&pid))
        .map(|id| xprop(&["-id", id, "_NET_WM_NAME"]))
}

fn config(app0: usize, title: *const c_char) -> Config {
    let mut config: Config = unsafe { std::mem::zeroed() };
    config.size = std::mem::size_of::<Config>() as u32;
    config.vibration_intensity = 100;
    config.rumble_scale = 100;
    config.ds_rumble = 1;
    config.app0_dir = app0 as *const c_char;
    config.window_title = title;
    config
}

fn hooks(live: bool) -> Hooks {
    Hooks {
        size: std::mem::size_of::<Hooks>() as u32,
        reserved: Some(reserved),
        ready: Some(ready),
        present_gate: Some(present_gate),
        presented: None,
        live_setting: if live { Some(live_setting) } else { None },
    }
}

fn fail(what: &str) -> ! {
    eprintln!("embed_smoke: FAIL: {what}");
    // No destructors: Kyty's threads are still running.
    unsafe { libc::_exit(1) }
}

fn check(ok: bool, what: &str) {
    if !ok {
        fail(what);
    }
    println!("embed_smoke: ok: {what}");
}

fn main() {
    let mut args = std::env::args().skip(1);
    let library = args.next().unwrap_or_else(|| fail("usage: embed_smoke <libkyty_renderer.so> [app0-dir]"));
    let app0 = args.next().unwrap_or_else(|| {
        let dir = std::env::temp_dir().join("embed_smoke_app0");
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_str().unwrap().to_owned()
    });

    let api: &'static Api = unsafe {
        let path = CString::new(library).unwrap();
        let handle = libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if handle.is_null() {
            fail(&format!("dlopen: {}", CStr::from_ptr(libc::dlerror()).to_string_lossy()));
        }
        let getter = libc::dlsym(handle, c"kyty_embed_api".as_ptr());
        check(!getter.is_null(), "kyty_embed_api is exported");
        let getter: extern "C" fn() -> *const Api = std::mem::transmute(getter);
        &*getter()
    };
    API.store(api as *const Api as usize, Ordering::SeqCst);
    check(api.abi == 1, "abi is 1");
    check(api.size as usize >= std::mem::size_of::<Api>(), "table is at least as large as ours");
    check(api.pad_data_size == 120, "pad_data_size is 120");
    check((api.find)(c"23LRUSvYu1M".as_ptr()).is_null(), "find is null before run");

    // Second run of the M1 check: gate answers with a tick, no live-settings hook.
    let absolute = std::env::var("KYTY_PRESENT_ABS_VBLANK").as_deref() == Ok("1");
    GATE_TICK.store(absolute, Ordering::Relaxed);
    let listener = listen();
    std::env::set_var("SDL_VIDEODRIVER", "x11"); // for window_title()

    let app0 = CString::new(app0).unwrap();
    let app0_ptr = app0.as_ptr() as usize;
    let run = api.run;
    let runner = std::thread::Builder::new()
        .name("renderer".into())
        .stack_size(8 << 20)
        .spawn(move || run(&config(app0_ptr, c"embed_smoke".as_ptr()), &hooks(!absolute)))
        .unwrap();

    let start = Instant::now();
    while STATE.load(Ordering::SeqCst) < 2 {
        if start.elapsed() > Duration::from_secs(90) || runner.is_finished() {
            fail(&format!("no ready within 90 s (hook state {})", STATE.load(Ordering::SeqCst)));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    check(STATE.load(Ordering::SeqCst) == 2, "reserved, then ready");
    println!("embed_smoke: ready after {} ms", start.elapsed().as_millis());

    check(!(api.find)(c"23LRUSvYu1M".as_ptr()).is_null(), "find(23LRUSvYu1M) is non-null");
    check((api.find)(c"no-such-nid".as_ptr()).is_null(), "find of an unknown NID is null");
    check(run(&config(app0_ptr, std::ptr::null()), &hooks(false)) == 1, "a second run is refused");
    let local = 0u64;
    check((api.fault)(&local as *const u64 as u64, 1, 0) == 0, "fault on memory Kyty does not track is not handled");

    // The session socket client: welcome, hello, accepted, then settings.apply packets.
    let mut poller = libc::pollfd { fd: listener, events: libc::POLLIN, revents: 0 };
    check(unsafe { libc::poll(&mut poller, 1, 5000) } == 1, "session socket: Kyty connected");
    let session = unsafe { libc::accept(listener, std::ptr::null_mut(), std::ptr::null_mut()) };
    send(session, r#"{"type":"welcome","version":2}"#);
    let hello = receive(session).unwrap_or_default();
    check(hello.contains(r#""type":"hello""#) && hello.contains(r#""frames":false"#), "session socket: frame-less hello");
    send(session, r#"{"type":"accepted","version":2,"frames":false}"#);
    send(session, r#"{"type":"settings.apply","id":1,"settings":{"display.fps":30}}"#);
    if absolute {
        check(receive(session).is_none(), "no live_setting hook: the apply is skipped, the connection stays");
    } else {
        let reply = receive(session).unwrap_or_default();
        check(reply.contains(r#""display.fps":30"#), "live_setting reply is sent back");
        let output = receive(session).unwrap_or_default();
        check(
            output.contains(r#""type":"output""#) && output.contains(r#""fps":30"#),
            "set_low_latency_interval(33333) is reported as 30 FPS",
        );
        send(session, r#"{"type":"settings.apply","id":2,"settings":{}}"#);
        check(receive(session).as_deref() == Some("{}"), "a live_setting reply that is not JSON becomes {}");
        check(LIVE_CALLS.load(Ordering::SeqCst) == 2, "live_setting was called twice");
    }
    (api.set_window_size)(1280, 720);

    // The direct function addresses, called from this plain (non-Kyty) thread.
    let fns = unsafe { &*api.fns };
    let mut queue = 0i64;
    check((fns.equeue_create)(&mut queue, c"smoke".as_ptr()) == 0 && queue != 0, "equeue_create");
    check((fns.equeue_add_user_event)(queue, 7) == 0, "equeue_add_user_event");
    let (mut event, mut count) = (KernelEvent::default(), 0);
    let no_wait = 0u32;
    check(
        (fns.equeue_wait)(queue, &mut event, 1, &mut count, &no_wait) == 0x8002003Cu32 as c_int && count == 0,
        "equeue_wait times out before the trigger",
    );
    check((fns.equeue_trigger_user_event)(queue, 7, std::ptr::null_mut()) == 0, "equeue_trigger_user_event");
    check(
        (fns.equeue_wait)(queue, &mut event, 1, &mut count, std::ptr::null()) == 0 && count == 1 && event.ident == 7,
        "equeue_wait returns the triggered event",
    );

    (fns.pad_init)();
    let pad = (fns.pad_open)((api.user_id)(), 0, 0, std::ptr::null());
    check(pad >= 0, "pad_open");
    let mut data = [0xAAu8; 120];
    check((fns.pad_read_state)(pad, data.as_mut_ptr()) == 0, "pad_read_state");
    // Sticks are bytes 4..8 of PadData; a pad at rest or no pad reads the centre.
    println!("embed_smoke: pad sticks {:?}, buttons {:#x}", &data[4..8], u32::from_ne_bytes(data[0..4].try_into().unwrap()));

    let silence = [0i16; 960];
    check((api.pad_audio)(48000, 0, silence.as_ptr().cast(), 480, 0, 0, 1.0) == 0, "pad_audio refuses 0 channels");
    let queued = (api.pad_audio)(48000, 0, silence.as_ptr().cast(), 480, 2, 0, 1.0);
    println!("embed_smoke: pad_audio queued {queued} us (0 without a DualSense)");

    // Leave the window up long enough to be seen.
    let before = GATE_CALLS.load(Ordering::Relaxed);
    std::thread::sleep(Duration::from_secs(2));
    let gate_calls = GATE_CALLS.load(Ordering::Relaxed) - before;
    // Once per vblank (60 Hz) on the default timeline. On the absolute timeline the polling loop
    // sleeps to the returned tick (100 us ahead); ignoring the tick it would sleep 500 us.
    let wanted = if absolute { 6000 } else { 100 };
    check(gate_calls > wanted, &format!("the present thread asked the gate ({gate_calls} calls in 2 s, > {wanted})"));
    let title = window_title();
    check(title.is_some(), "this process has a window");
    // Kyty sets the title when it presents; the second run's gate never lets it.
    let title = title.unwrap();
    check(absolute || title.contains("\"embed_smoke\""), &format!("the window title is fixed, or nothing was presented: {}", title.trim()));
    (api.quit)();
    let start = Instant::now();
    while !runner.is_finished() {
        if start.elapsed() > Duration::from_secs(20) {
            fail("run did not return within 20 s of quit");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    check(runner.join().unwrap() == 0, "run returned 0 after quit");
    (api.flush)();
    check((api.pad_audio)(48000, 0, silence.as_ptr().cast(), 480, 2, 0, 1.0) == 0, "pad_audio is refused after flush");
    println!("embed_smoke: PASS");
    // As upstream's quick_exit after its window loop: the subsystems are never torn down.
    unsafe { libc::_exit(0) }
}
