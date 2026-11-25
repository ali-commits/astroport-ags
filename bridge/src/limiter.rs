//! Frame limiter and the video override (design.md 6.1 entry 2.17, 6.3 K9).

use std::ffi::c_int;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Limit {
    /// No limiter: upstream behaviour.
    Off,
    Unlimited,
    Fps(u32),
}

static LIMIT: AtomicU32 = AtomicU32::new(0);
/// Set by AnyPS5's video player through `ExternalRenderer.videoActive`.
pub static VIDEO: AtomicBool = AtomicBool::new(false);

pub fn set_limit(limit: Limit) {
    let bits = match limit {
        Limit::Off => 0,
        Limit::Unlimited => u32::MAX,
        Limit::Fps(fps) => fps,
    };
    LIMIT.store(bits, Ordering::Relaxed);
}

pub fn limit() -> Limit {
    match LIMIT.load(Ordering::Relaxed) {
        0 => Limit::Off,
        u32::MAX => Limit::Unlimited,
        fps => Limit::Fps(fps),
    }
}

/// Videos are paced for 60 frames per second whatever the selected limit is.
pub fn effective(limit: Limit, video: bool) -> Limit {
    if video {
        Limit::Fps(60)
    } else {
        limit
    }
}

/// Minimum frame interval for the low-latency mode: 0 for none or unlimited.
pub fn interval_us(limit: Limit) -> u32 {
    match limit {
        Limit::Fps(fps) => 1_000_000 / fps,
        _ => 0,
    }
}

/// Kyty asks the gate between vblanks only on its absolute-vblank timeline with the Immediate
/// present mode (0); otherwise once per vblank, so a limit is rounded to the vblank rate.
pub fn pacing_warning(limit: Limit, abs_vblank: Option<&str>, present_mode: u8) -> Option<&'static str> {
    match (limit, abs_vblank == Some("1"), present_mode) {
        (Limit::Off, ..) | (_, true, 0) => None,
        (_, false, _) => Some("ASTROPORT_FPS_LIMIT is set without KYTY_PRESENT_ABS_VBLANK=1: the limit is only applied once per vblank"),
        _ => Some("ASTROPORT_FPS_LIMIT is set with a present mode other than immediate: the limit is only applied once per vblank"),
    }
}

/// Per present thread. Times are monotonic ticks, `ticks_per_s` of them per second.
pub struct Limiter {
    selected: Option<Limit>,
    /// Next tick a flip may be presented at; 0 = none.
    due: i64,
}

impl Limiter {
    pub const fn new() -> Self {
        Self { selected: None, due: 0 }
    }

    /// Returns true when the limit changed; the cadence then restarts.
    pub fn select(&mut self, limit: Limit) -> bool {
        let changed = self.selected != Some(limit);
        if changed {
            self.selected = Some(limit);
            self.due = 0;
        }
        changed
    }

    /// 0 = present now; otherwise the tick before which nothing may be presented.
    pub fn gate(&self, now: i64) -> i64 {
        if matches!(self.selected, Some(Limit::Fps(_))) && now < self.due {
            self.due
        } else {
            0
        }
    }

    /// A flip was presented at `now`. At most 1 ms late keeps the cadence, so timer overshoot
    /// does not accumulate; a later frame restarts from now, so no catch-up burst follows.
    pub fn presented(&mut self, now: i64, ticks_per_s: i64) {
        let Some(Limit::Fps(fps)) = self.selected else { return };
        let interval = (ticks_per_s + fps as i64 - 1) / fps as i64;
        let on_time = self.due != 0 && now - self.due <= ticks_per_s / 1000;
        self.due = if on_time { self.due } else { now } + interval;
    }
}

impl Default for Limiter {
    fn default() -> Self {
        Self::new()
    }
}

static STATE: Mutex<Limiter> = Mutex::new(Limiter::new());

unsafe fn follow_limit(state: &mut Limiter) {
    let limit = effective(limit(), VIDEO.load(Ordering::Relaxed));
    if state.select(limit) {
        (crate::kyty().set_low_latency_interval.unwrap())(interval_us(limit));
    }
}

/// A live setting changed the limit: Kyty's low-latency interval (and the session socket's output
/// fps, read right after the reply) follow now instead of at the next present.
pub unsafe fn follow_now() {
    follow_limit(&mut STATE.lock().unwrap());
}

/// `KytyEmbedHooks.present_gate`. A due tick is returned whether or not a flip is ready, so
/// the present thread can sleep until then.
pub unsafe extern "C" fn present_gate(now: i64, _ticks_per_s: i64, _flip_ready: c_int) -> i64 {
    let mut state = STATE.lock().unwrap();
    follow_limit(&mut state);
    state.gate(now)
}

/// `KytyEmbedHooks.presented`.
pub unsafe extern "C" fn presented(now: i64, ticks_per_s: i64) {
    static FIRST: std::sync::Once = std::sync::Once::new();
    FIRST.call_once(|| eprintln!("astroport: first frame presented after {} ms", crate::uptime_ms()));
    let mut state = STATE.lock().unwrap();
    follow_limit(&mut state);
    state.presented(now, ticks_per_s);
}

#[cfg(test)]
mod tests {
    use super::*;

    const F: i64 = 1_000_000_000;

    /// A present thread that wakes `overshoot(frame)` ticks after the due tick.
    fn run(limiter: &mut Limiter, frames: i64, overshoot: impl Fn(i64) -> i64) -> (i64, i64) {
        let mut now = 12_345;
        let mut first = 0;
        for frame in 0..frames {
            let due = limiter.gate(now);
            if due != 0 {
                assert_eq!(limiter.gate(due - 1), due, "nothing may be presented before the due tick");
                now = due + overshoot(frame);
            }
            assert_eq!(limiter.gate(now), 0);
            limiter.presented(now, F);
            if frame == 0 {
                first = now;
            }
        }
        (first, now)
    }

    #[test]
    fn cap_of_30_delivers_30_00_despite_timer_overshoot() {
        let mut limiter = Limiter::new();
        limiter.select(Limit::Fps(30));
        // 0..0.9 ms late, differently every frame
        let (first, last) = run(&mut limiter, 601, |frame| (frame * 7919 % 10) * 90_000);
        let fps = 600.0 * F as f64 / (last - first) as f64;
        assert!((fps - 30.0).abs() < 0.005, "{fps}");
    }

    #[test]
    fn late_frame_restarts_the_cadence() {
        let mut limiter = Limiter::new();
        limiter.select(Limit::Fps(30));
        let (_, now) = run(&mut limiter, 10, |_| 0);
        let interval = (F + 29) / 30;
        let late = now + interval + 5_000_000;
        assert_eq!(limiter.gate(late), 0);
        limiter.presented(late, F);
        assert_eq!(limiter.gate(late + 1), late + interval, "no catch-up: the next frame is a full interval later");
    }

    #[test]
    fn off_and_unlimited_never_hold_a_flip() {
        for limit in [Limit::Off, Limit::Unlimited] {
            let mut limiter = Limiter::new();
            limiter.select(limit);
            limiter.presented(1000, F);
            assert_eq!(limiter.gate(1001), 0);
        }
    }

    #[test]
    fn changing_the_limit_resets_the_cadence() {
        let mut limiter = Limiter::new();
        assert!(limiter.select(Limit::Fps(30)));
        assert!(!limiter.select(Limit::Fps(30)));
        limiter.presented(1000, F);
        assert_ne!(limiter.gate(1001), 0);
        assert!(limiter.select(Limit::Fps(60)));
        assert_eq!(limiter.gate(1001), 0);
    }

    #[test]
    fn a_limit_warns_unless_kyty_polls_between_vblanks() {
        assert_eq!(pacing_warning(Limit::Off, None, 1), None);
        assert_eq!(pacing_warning(Limit::Fps(30), Some("1"), 0), None);
        assert_eq!(pacing_warning(Limit::Unlimited, Some("1"), 0), None);
        for abs_vblank in [None, Some("0"), Some("")] {
            assert!(pacing_warning(Limit::Fps(30), abs_vblank, 0).unwrap().contains("KYTY_PRESENT_ABS_VBLANK=1"));
        }
        assert!(pacing_warning(Limit::Unlimited, Some("1"), 2).unwrap().contains("present mode"));
    }

    #[test]
    fn video_forces_60() {
        assert_eq!(effective(Limit::Fps(30), true), Limit::Fps(60));
        assert_eq!(effective(Limit::Fps(120), true), Limit::Fps(60));
        assert_eq!(effective(Limit::Off, true), Limit::Fps(60));
        assert_eq!(effective(Limit::Fps(30), false), Limit::Fps(30));
        assert_eq!(effective(Limit::Off, false), Limit::Off);
    }

    #[test]
    fn low_latency_interval_follows_the_limit() {
        assert_eq!(interval_us(Limit::Fps(30)), 33_333);
        assert_eq!(interval_us(Limit::Fps(60)), 16_666);
        assert_eq!(interval_us(Limit::Unlimited), 0);
        assert_eq!(interval_us(Limit::Off), 0);
        assert_eq!(interval_us(effective(Limit::Fps(30), true)), 16_666);
    }
}
