//! #[repr(C)] mirrors of the engine tables (design.md 3.1, 2.2) with size asserts.
//! The C side is `engines/kyty/src/embed/kytyEmbed.h`, AnyPS5's `libAstroportHost` and
//! `libkernel/include/ExternalRenderer.hpp`. Fields are only ever appended.

use std::ffi::{c_char, c_int, c_void};

pub const KYTY_EMBED_ABI: u32 = 1;
pub const ASTROPORT_HOST_ABI: u32 = 1;

#[repr(C)]
pub struct KytyEmbedConfig {
    pub size: u32,
    pub width: u32,
    pub height: u32,
    pub vblank_hz: u32,
    pub vibration_intensity: u32,
    pub rumble_scale: u32,
    pub rumble_switch_floor: u32,
    pub low_latency_interval_us: u32,
    pub dlss_preset: u32,
    pub upscale_pass_hash: u64,
    pub fullscreen: u8,
    /// 0 immediate, 1 fifo, 2 mailbox
    pub present_mode: u8,
    /// 0 none, 1 size, 2 performance
    pub shader_optimization: u8,
    pub wayland_native: u8,
    pub low_latency: u8,
    pub dlss: u8,
    pub dlss_reverse_depth: u8,
    /// 0 off, 1 auto, 2 ps, 3 xbox, 4 switch, 5 generic
    pub pad_prefer: u8,
    pub ds_rumble: u8,
    pub fsr_fg: u8,
    pub app0_dir: *const c_char,
    pub window_title: *const c_char,
    pub dlss_dir: *const c_char,
    pub ngx_data_dir: *const c_char,
    pub fsr_dir: *const c_char,
}

#[repr(C)]
pub struct KytyEmbedHooks {
    pub size: u32,
    pub reserved: Option<unsafe extern "C" fn()>,
    pub ready: Option<unsafe extern "C" fn()>,
    /// 0 = present now; >0 = do not present before this tick
    pub present_gate: Option<unsafe extern "C" fn(now: i64, ticks_per_s: i64, flip_ready: c_int) -> i64>,
    pub presented: Option<unsafe extern "C" fn(now: i64, ticks_per_s: i64)>,
    pub live_setting: Option<unsafe extern "C" fn(apply_json: *const c_char) -> *const c_char>,
}

/// Addresses of upstream KYTY_SYSV_ABI functions.
#[repr(C)]
pub struct KytyEmbedFns {
    pub pad_init: *const c_void,
    pub pad_open: *const c_void,
    pub pad_get_handle: *const c_void,
    pub pad_read_state: *const c_void,
    pub pad_set_vibration: *const c_void,
    pub pad_set_light_bar: *const c_void,
    pub pad_reset_light_bar: *const c_void,
    pub pad_set_trigger_effect: *const c_void,
    pub equeue_create: *const c_void,
    pub equeue_wait: *const c_void,
    pub equeue_add_user_event: *const c_void,
    pub equeue_trigger_user_event: *const c_void,
    pub agc_add_eq_event: *const c_void,
    pub video_out_add_flip_event: *const c_void,
}

#[repr(C)]
pub struct KytyEmbedApi {
    pub abi: u32,
    pub size: u32,
    /// sizeof(Libs::Controller::PadData) = 120
    pub pad_data_size: u32,
    pub fns: *const KytyEmbedFns,
    /// Blocks until the window loop ends.
    pub run: Option<unsafe extern "C" fn(*const KytyEmbedConfig, *const KytyEmbedHooks) -> c_int>,
    pub find: Option<unsafe extern "C" fn(nid: *const c_char) -> *mut c_void>,
    /// access: 0 read, 1 write, 2 execute
    pub fault: Option<unsafe extern "C" fn(address: u64, access: c_int, instruction: u64) -> c_int>,
    pub quit: Option<unsafe extern "C" fn()>,
    pub flush: Option<unsafe extern "C" fn()>,
    pub user_id: Option<unsafe extern "C" fn() -> c_int>,
    pub pad_audio: Option<PadAudioFn>,
    pub set_low_latency_interval: Option<unsafe extern "C" fn(us: u32)>,
    pub set_window_size: Option<unsafe extern "C" fn(width: u32, height: u32)>,
}

/// Returns microseconds queued; 0 = refused.
pub type PadAudioFn = unsafe extern "C" fn(
    rate: u32,
    speaker: c_int,
    samples: *const c_void,
    frames: u32,
    channels: u32,
    is_float: c_int,
    gain: f32,
) -> u64;

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct AstroportTouch {
    pub x: u16,
    pub y: u16,
    pub id: u8,
    pub active: u8,
}

/// design.md names this struct but does not lay it out; fields follow spec-anyps5 C8 / C9.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct AstroportPadInput {
    /// PS5 bit layout
    pub buttons: u32,
    /// left x, left y, right x, right y; 128 = centre
    pub sticks: [u8; 4],
    pub l2: u8,
    pub r2: u8,
    pub connected: u8,
    pub has_motion: u8,
    pub touch: [AstroportTouch; 2],
    /// m/s^2 including gravity
    pub accel: [f32; 3],
    /// rad/s
    pub gyro: [f32; 3],
}

/// design.md names this struct but does not lay it out; fields follow spec-anyps5 C10.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct AstroportPadOutput {
    pub rumble_large: u8,
    pub rumble_small: u8,
    pub audio_rumble_large: u8,
    pub audio_rumble_small: u8,
    pub light_bar_valid: u8,
    pub light_bar: [u8; 3],
    /// per trigger (0 = L2, 1 = R2): a command was set
    pub trigger_set: [u8; 2],
    /// raw 56-byte trigger commands
    pub trigger: [[u8; 56]; 2],
}

#[repr(C)]
pub struct AstroportHostApi {
    pub abi: u32,
    pub size: u32,
    /// Returns base, or 0.
    pub donate: Option<unsafe extern "C" fn(base: u64, bytes: u64) -> u64>,
    /// kind: 0 EOP, 1 flip
    pub event_add: Option<unsafe extern "C" fn(queue: u64, kind: c_int, ident: u64, user_data: *mut c_void) -> c_int>,
    pub event_delete: Option<unsafe extern "C" fn(queue: u64, kind: c_int, ident: u64) -> c_int>,
    pub event_deliver: Option<unsafe extern "C" fn(kind: c_int, ident: u64, data: u64)>,
    pub pad_publish: Option<unsafe extern "C" fn(port: c_int, input: *const AstroportPadInput)>,
    pub pad_fetch_output: Option<unsafe extern "C" fn(port: c_int, sequence: *mut u32, out: *mut AstroportPadOutput) -> c_int>,
    pub request_exit: Option<unsafe extern "C" fn(code: c_int)>,
    pub on_shutdown: Option<unsafe extern "C" fn(callback: unsafe extern "C" fn())>,
    pub install_renderer: Option<unsafe extern "C" fn(external_renderer: *const ExternalRenderer)>,
}

/// Produced by the bridge, consumed by AnyPS5's libkernel (design.md 2.2).
#[repr(C)]
pub struct ExternalRenderer {
    pub size: u32,
    /// The 17 memory functions in the order of `nids::SLOTS[EXPORT_COUNT..]`
    /// (nidscan.rs `MEMORY_FUNCTIONS`). The C header declares them as 17 typed members.
    pub memory: [unsafe extern "C" fn(); 17],
    pub owns: unsafe extern "C" fn(address: usize, bytes: usize) -> bool,
    pub pad_audio: PadAudioFn,
    pub haptic_gain: unsafe extern "C" fn() -> f32,
    pub video_active: unsafe extern "C" fn(active: c_int),
}

/// Mirror of Kyty's `Libs::Controller::PadData` (src/libs/padData.h); guarded by `pad_data_size`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PadData {
    pub buttons: u32,
    pub sticks: [u8; 4],
    pub l2: u8,
    pub r2: u8,
    pub padding: [u8; 2],
    /// x, y, z, w
    pub orientation: [f32; 4],
    /// in g
    pub acceleration: [f32; 3],
    /// rad/s
    pub angular_velocity: [f32; 3],
    pub touch_num: u8,
    pub touch_reserve: [u8; 7],
    pub touch: [PadTouch; 2],
    pub connected: u8,
    pub timestamp: u64,
    pub rest: [u8; 32],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PadTouch {
    pub x: u16,
    pub y: u16,
    pub id: u8,
    pub reserve: [u8; 3],
}

/// The PS5 kernel event structure (Kyty `KernelEvent`, src/kernel/eventQueue.h).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KernelEvent {
    pub ident: usize,
    pub filter: i16,
    pub flags: u16,
    pub fflags: u32,
    pub data: isize,
    pub udata: *mut c_void,
}

const _: () = {
    use std::mem::{offset_of, size_of};
    assert!(size_of::<PadData>() == 120);
    assert!(offset_of!(PadData, touch) == 60);
    assert!(offset_of!(PadData, connected) == 76);
    assert!(offset_of!(PadData, timestamp) == 80);
    assert!(size_of::<KernelEvent>() == 32);
    assert!(size_of::<KytyEmbedConfig>() == 104);
    assert!(offset_of!(KytyEmbedConfig, upscale_pass_hash) == 40);
    assert!(offset_of!(KytyEmbedConfig, app0_dir) == 64);
    assert!(size_of::<KytyEmbedHooks>() == 48);
    assert!(size_of::<KytyEmbedFns>() == 112);
    assert!(size_of::<KytyEmbedApi>() == 96);
    assert!(offset_of!(KytyEmbedApi, fns) == 16);
    assert!(size_of::<AstroportTouch>() == 6);
    assert!(size_of::<AstroportPadInput>() == 48);
    assert!(size_of::<AstroportPadOutput>() == 122);
    assert!(size_of::<AstroportHostApi>() == 80);
    assert!(size_of::<ExternalRenderer>() == 176);
    assert!(offset_of!(ExternalRenderer, owns) == 144);
};
