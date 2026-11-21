use serde_json::{json, Value};

// sheet §6.6: controller.haptics appears once, in the game overlay with no tab (m1 §1.2.2).

// sheet §1.2, §2.2: each standard key declares support and its applier and live class.
// sheet §2.1: perf.render_scale uses integer percents; graphics.render uses named sizes.
// sheet §2.5: perf.low_latency reaches launch.plan from M4, with a registry default of "off".
// sheet §2.2: absent perf.frame_generation lets AGS offer LSFG for local play.
pub fn standard_support() -> Value {
    json!({
        "display.resolution": {"level": "honoured", "live": "next_launch", "applier": "source"},
        "display.fps": {"level": "honoured", "live": "next_launch", "applier": "source", "values": [30, 60]},
        "perf.low_latency": {"level": "honoured", "live": "next_launch", "applier": "source", "values": ["off", "on"]}
    })
}
