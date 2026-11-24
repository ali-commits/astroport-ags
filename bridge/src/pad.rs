//! Relay thread: input, motion, output differences (design.md 3.7). Port 0 only.

use crate::abi::{AstroportPadInput, AstroportPadOutput, AstroportTouch, PadData};
use crate::{host, kyty, STOP};
use std::ffi::{c_int, c_void};
use std::mem::transmute;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const STANDARD_GRAVITY: f32 = 9.80665;

/// Body rate in rad/s from two orientation quaternions (x, y, z, w) `dt` seconds apart:
/// 2 * vec(conj(previous) * current) / dt, sign flipped when the scalar part is negative.
/// Only for 0.5 ms <= dt <= 100 ms.
pub fn body_rate(previous: [f32; 4], current: [f32; 4], dt: f32) -> Option<[f32; 3]> {
    if !(0.0005..=0.1).contains(&dt) {
        return None;
    }
    let [ax, ay, az, aw] = [-previous[0], -previous[1], -previous[2], previous[3]];
    let [bx, by, bz, bw] = current;
    let w = aw * bw - ax * bx - ay * by - az * bz;
    let scale = if w < 0.0 { -2.0 } else { 2.0 } / dt;
    Some([
        (aw * bx + ax * bw + ay * bz - az * by) * scale,
        (aw * by - ax * bz + ay * bw + az * bx) * scale,
        (aw * bz + ax * by - ay * bx + az * bw) * scale,
    ])
}

/// Angular velocity for the host: Kyty's own, or for pads without a gyroscope (all three
/// rates exactly zero) the rate derived from the change of orientation.
#[derive(Default)]
pub struct Gyro {
    previous: Option<([f32; 4], f64)>,
}

impl Gyro {
    pub fn rate(&mut self, data: &PadData, seconds: f64) -> [f32; 3] {
        let derived = match self.previous {
            Some((orientation, then)) if data.angular_velocity == [0.0; 3] => {
                body_rate(orientation, data.orientation, (seconds - then) as f32)
            }
            _ => None,
        };
        self.previous = Some((data.orientation, seconds));
        derived.unwrap_or(data.angular_velocity)
    }
}

pub fn input(data: &PadData, gyro: [f32; 3]) -> AstroportPadInput {
    let touch = |index: usize| AstroportTouch {
        x: data.touch[index].x,
        y: data.touch[index].y,
        id: data.touch[index].id,
        active: (index < data.touch_num as usize) as u8,
    };
    AstroportPadInput {
        buttons: data.buttons,
        sticks: data.sticks,
        l2: data.l2,
        r2: data.r2,
        connected: (data.connected != 0) as u8,
        has_motion: 1,
        touch: [touch(0), touch(1)],
        accel: data.acceleration.map(|g| g * STANDARD_GRAVITY),
        gyro,
    }
}

/// What must be sent to the pad after a new output state.
#[derive(Default, PartialEq, Debug)]
pub struct Changes {
    /// large motor, small motor
    pub rumble: Option<[u8; 2]>,
    /// Some(None) = reset to the default colour
    pub light_bar: Option<Option<[u8; 3]>>,
    /// PS5 trigger effect parameter: mask byte (1 = L2, 2 = R2), padding to 8, two 56-byte commands
    pub trigger: Option<[u8; 120]>,
}

/// What the pad was last told.
#[derive(Default)]
pub struct Applied {
    rumble: Option<[u8; 2]>,
    light_bar: Option<[u8; 3]>,
    trigger: [Option<[u8; 56]>; 2],
}

impl Applied {
    pub fn changes(&mut self, output: &AstroportPadOutput) -> Changes {
        let mut changes = Changes::default();
        let rumble = [output.rumble_large.max(output.audio_rumble_large), output.rumble_small.max(output.audio_rumble_small)];
        if self.rumble != Some(rumble) {
            self.rumble = Some(rumble);
            changes.rumble = Some(rumble);
        }
        let light_bar = (output.light_bar_valid != 0).then_some(output.light_bar);
        if self.light_bar != light_bar {
            self.light_bar = light_bar;
            changes.light_bar = Some(light_bar);
        }
        let mut parameter = [0u8; 120];
        for side in 0..2 {
            if output.trigger_set[side] != 0 && self.trigger[side] != Some(output.trigger[side]) {
                self.trigger[side] = Some(output.trigger[side]);
                parameter[0] |= 1 << side;
                parameter[8 + 56 * side..][..56].copy_from_slice(&output.trigger[side]);
            }
        }
        if parameter[0] != 0 {
            changes.trigger = Some(parameter);
        }
        changes
    }
}

/// Opens Kyty's pad and starts the relay thread.
///
/// # Safety
/// The engines must be set and Kyty's subsystems up.
pub unsafe fn start() {
    let fns = &*kyty().fns;
    let init: unsafe extern "sysv64" fn() -> c_int = transmute(fns.pad_init);
    let open: unsafe extern "sysv64" fn(c_int, c_int, c_int, *const c_void) -> c_int = transmute(fns.pad_open);
    let get_handle: unsafe extern "sysv64" fn(c_int, c_int, c_int) -> c_int = transmute(fns.pad_get_handle);
    let user = (kyty().user_id.unwrap())();
    init();
    let mut handle = open(user, 0, 0, std::ptr::null());
    if handle < 0 {
        handle = get_handle(user, 0, 0);
    }
    if handle < 0 {
        crate::die(format!("Kyty's pad could not be opened: {handle:#x}"));
    }
    std::thread::Builder::new().name("astroport pad".into()).spawn(move || relay(handle)).unwrap();
}

fn relay(handle: c_int) {
    unsafe {
        let fns = &*kyty().fns;
        let read: unsafe extern "sysv64" fn(c_int, *mut PadData) -> c_int = transmute(fns.pad_read_state);
        let set_vibration: unsafe extern "sysv64" fn(c_int, *const [u8; 2]) -> c_int = transmute(fns.pad_set_vibration);
        let set_light_bar: unsafe extern "sysv64" fn(c_int, *const [u8; 4]) -> c_int = transmute(fns.pad_set_light_bar);
        let reset_light_bar: unsafe extern "sysv64" fn(c_int) -> c_int = transmute(fns.pad_reset_light_bar);
        let set_trigger: unsafe extern "sysv64" fn(c_int, *const [u8; 120]) -> c_int = transmute(fns.pad_set_trigger_effect);
        let publish = host().pad_publish.unwrap();
        let fetch_output = host().pad_fetch_output.unwrap();

        let started = Instant::now();
        let (mut gyro, mut applied, mut sequence) = (Gyro::default(), Applied::default(), 0u32);
        while !STOP.load(Ordering::Relaxed) {
            let mut data: PadData = std::mem::zeroed();
            let state = if read(handle, &mut data) == 0 {
                input(&data, gyro.rate(&data, started.elapsed().as_secs_f64()))
            } else {
                AstroportPadInput::default()
            };
            publish(0, &state);

            let mut output: AstroportPadOutput = std::mem::zeroed();
            if fetch_output(0, &mut sequence, &mut output) != 0 {
                let changes = applied.changes(&output);
                if let Some(rumble) = changes.rumble {
                    set_vibration(handle, &rumble);
                }
                match changes.light_bar {
                    Some(Some([r, g, b])) => {
                        set_light_bar(handle, &[r, g, b, 0]);
                    }
                    Some(None) => {
                        reset_light_bar(handle);
                    }
                    None => {}
                }
                if let Some(parameter) = changes.trigger {
                    set_trigger(handle, &parameter);
                }
            }
            std::thread::sleep(Duration::from_millis(4));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output() -> AstroportPadOutput {
        unsafe { std::mem::zeroed() }
    }

    #[test]
    fn rumble_is_the_larger_source_and_sent_on_change_or_first() {
        let mut applied = Applied::default();
        let mut out = output();
        assert_eq!(applied.changes(&out).rumble, Some([0, 0]), "first update");
        assert_eq!(applied.changes(&out), Changes::default());
        (out.rumble_large, out.audio_rumble_large, out.rumble_small, out.audio_rumble_small) = (10, 40, 30, 5);
        assert_eq!(applied.changes(&out).rumble, Some([40, 30]));
        out.rumble_large = 20; // still below the audio-derived strength
        assert_eq!(applied.changes(&out).rumble, None);
        out.audio_rumble_large = 0;
        assert_eq!(applied.changes(&out).rumble, Some([20, 30]));
    }

    #[test]
    fn light_bar_is_set_changed_and_reset() {
        let mut applied = Applied::default();
        let mut out = output();
        assert_eq!(applied.changes(&out).light_bar, None, "never set: nothing to reset");
        (out.light_bar_valid, out.light_bar) = (1, [1, 2, 3]);
        assert_eq!(applied.changes(&out).light_bar, Some(Some([1, 2, 3])));
        assert_eq!(applied.changes(&out).light_bar, None);
        out.light_bar = [9, 2, 3];
        assert_eq!(applied.changes(&out).light_bar, Some(Some([9, 2, 3])));
        out.light_bar_valid = 0;
        assert_eq!(applied.changes(&out).light_bar, Some(None));
        assert_eq!(applied.changes(&out).light_bar, None);
    }

    #[test]
    fn only_new_or_changed_triggers_are_flagged() {
        let mut applied = Applied::default();
        let mut out = output();
        assert_eq!(applied.changes(&out).trigger, None);
        (out.trigger_set, out.trigger[0], out.trigger[1]) = ([1, 1], [7; 56], [8; 56]);
        let both = applied.changes(&out).trigger.unwrap();
        assert_eq!((both[0], &both[1..8], &both[8..64], &both[64..]), (3, &[0u8; 7][..], &[7u8; 56][..], &[8u8; 56][..]));
        assert_eq!(applied.changes(&out).trigger, None);
        out.trigger[1][4] = 99;
        let right = applied.changes(&out).trigger.unwrap();
        assert_eq!((right[0], &right[8..64], right[68]), (2, &[0u8; 56][..], 99), "L2 is unchanged: not flagged, not copied");
    }

    #[test]
    fn body_rate_from_orientation_change() {
        let identity = [0.0, 0.0, 0.0, 1.0];
        let angle = 0.02f32; // rad about z in 10 ms = 2 rad/s
        let turned = [0.0, 0.0, (angle / 2.0).sin(), (angle / 2.0).cos()];
        let rate = body_rate(identity, turned, 0.01).unwrap();
        assert!(rate[0] == 0.0 && rate[1] == 0.0 && (rate[2] - 2.0).abs() < 1e-3, "{rate:?}");
        let negated = turned.map(|c| -c); // the same orientation
        assert!((body_rate(identity, negated, 0.01).unwrap()[2] - 2.0).abs() < 1e-3);
        assert!((body_rate(turned, identity, 0.01).unwrap()[2] + 2.0).abs() < 1e-3);
        // body frame: starting from a quarter turn about x, a further turn about the pad's own z
        let quarter_x = [0.5f32.sqrt(), 0.0, 0.0, 0.5f32.sqrt()];
        let (s, c) = (quarter_x[0] * turned[2], quarter_x[3] * turned[3]);
        let composed = [quarter_x[0] * turned[3], -s, quarter_x[3] * turned[2], c]; // quarter_x * turned
        let rate = body_rate(quarter_x, composed, 0.01).unwrap();
        assert!(rate[0].abs() < 1e-3 && rate[1].abs() < 1e-3 && (rate[2] - 2.0).abs() < 1e-3, "{rate:?}");
        assert_eq!(body_rate(identity, turned, 0.0004), None);
        assert_eq!(body_rate(identity, turned, 0.11), None);
    }

    #[test]
    fn gyro_falls_back_only_when_all_rates_are_zero() {
        let mut data: PadData = unsafe { std::mem::zeroed() };
        data.orientation = [0.0, 0.0, 0.0, 1.0];
        let mut gyro = Gyro::default();
        assert_eq!(gyro.rate(&data, 1.0), [0.0; 3], "no previous sample");
        data.orientation = [0.0, 0.0, 0.01f32.sin(), 0.01f32.cos()];
        assert!((gyro.rate(&data, 1.01)[2] - 2.0).abs() < 1e-3);
        data.angular_velocity = [0.0, 0.5, 0.0];
        data.orientation = [0.0, 0.0, 0.0, 1.0];
        assert_eq!(gyro.rate(&data, 1.02), [0.0, 0.5, 0.0], "a real gyroscope wins");
        data.angular_velocity = [0.0; 3];
        assert_eq!(gyro.rate(&data, 2.0), [0.0; 3], "too long ago");
    }

    #[test]
    fn input_carries_buttons_touch_and_motion() {
        let mut data: PadData = unsafe { std::mem::zeroed() };
        (data.buttons, data.sticks, data.l2, data.r2, data.connected) = (0x4000, [1, 2, 3, 4], 5, 6, 1);
        (data.touch_num, data.touch[0].x, data.touch[0].y, data.touch[0].id) = (1, 100, 200, 7);
        data.acceleration = [0.0, 1.0, 0.0];
        let state = input(&data, [0.1, 0.2, 0.3]);
        assert_eq!((state.buttons, state.sticks, state.l2, state.r2, state.connected), (0x4000, [1, 2, 3, 4], 5, 6, 1));
        assert_eq!(state.touch[0], AstroportTouch { x: 100, y: 200, id: 7, active: 1 });
        assert_eq!(state.touch[1].active, 0);
        assert_eq!((state.has_motion, state.accel, state.gyro), (1, [0.0, 9.80665, 0.0], [0.1, 0.2, 0.3]));
    }
}
