//! Pad audio, haptic gain and video-active entries of `ExternalRenderer` (design.md 3.7).
//! Called on AnyPS5's audio threads: atomics only, then Kyty's pad audio entry.

use crate::{entry, limiter, STOP};
use std::ffi::{c_int, c_void};
use std::sync::atomic::{AtomicU32, Ordering};

/// f32 bits; 1.0 until start reads APS5_PAD_VIBRATION_GAIN.
static HAPTIC_GAIN: AtomicU32 = AtomicU32::new(0x3F80_0000);

pub fn set_haptic_gain(gain: f32) {
    HAPTIC_GAIN.store(gain.clamp(0.0, 4.0).to_bits(), Ordering::Relaxed);
}

pub extern "C" fn haptic_gain() -> f32 {
    f32::from_bits(HAPTIC_GAIN.load(Ordering::Relaxed))
}

/// Microseconds queued; 0 before ready and after shutdown started.
pub unsafe extern "C" fn pad_audio(
    rate: u32,
    speaker: c_int,
    samples: *const c_void,
    frames: u32,
    channels: u32,
    is_float: c_int,
    gain: f32,
) -> u64 {
    if !entry::is_ready() || STOP.load(Ordering::Relaxed) {
        return 0;
    }
    (crate::kyty().pad_audio.unwrap())(rate, speaker, samples, frames, channels, is_float, gain)
}

pub extern "C" fn video_active(active: c_int) {
    limiter::VIDEO.store(active != 0, Ordering::Relaxed);
}
