//! Environment -> Config (design.md 7.1). Pure: the environment is a lookup function.
//! An invalid value is an error naming the variable and what is accepted. An empty value
//! counts as unset.

use crate::abi::KytyEmbedConfig;
use crate::limiter::{interval_us, Limit};
use std::ffi::{c_char, CString};

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// 0 = Kyty's default
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
    /// 0 immediate, 1 fifo, 2 mailbox
    pub present_mode: u8,
    /// 0 none, 1 size, 2 performance
    pub shader_opt: u8,
    pub vblank_hz: u32,
    pub wayland_native: bool,
    pub app0: String,
    pub window_title: Option<String>,
    pub fps_limit: Limit,
    pub low_latency: bool,
    pub dlss: bool,
    /// 0 = DLSS cannot engage
    pub dlss_pass: u64,
    /// Where the pass keeps its inputs (kytyEmbed.h); all zero = DLSS cannot engage
    pub dlss_layout: [u8; 16],
    pub dlss_reverse_depth: bool,
    pub dlss_dir: String,
    /// 0 = model default, 1 = A ..
    pub dlss_preset: u32,
    /// 0 off, 1 auto, 2 ps, 3 xbox, 4 switch, 5 generic
    pub pad_prefer: u8,
    pub rumble_scale: u32,
    pub rumble_switch_floor: u32,
    pub ds_rumble: bool,
    pub fsr_fg: bool,
    pub fsr_dir: String,
    pub ngx_data_dir: String,
    /// APS5_PAD_VIBRATION_GAIN (AnyPS5's variable: lenient, clamped 0..4)
    pub haptic_gain: f32,
}

struct Env<F>(F);

impl<F: Fn(&str) -> Option<String>> Env<F> {
    fn get(&self, name: &str) -> Option<String> {
        (self.0)(name).filter(|value| !value.is_empty())
    }

    fn number(&self, name: &str, low: u32, high: u32, default: u32) -> Result<u32, String> {
        let Some(value) = self.get(name) else { return Ok(default) };
        match value.parse() {
            Ok(number) if (low..=high).contains(&number) => Ok(number),
            _ => Err(format!("{name}={value:?}: expected a whole number {low}..{high}")),
        }
    }

    fn flag(&self, name: &str, default: bool) -> Result<bool, String> {
        Ok(self.choice(name, &["0", "1"], default as u8)? == 1)
    }

    /// Position of the value in `choices`.
    fn choice(&self, name: &str, choices: &[&str], default: u8) -> Result<u8, String> {
        let Some(value) = self.get(name) else { return Ok(default) };
        match choices.iter().position(|choice| *choice == value) {
            Some(position) => Ok(position as u8),
            None => Err(format!("{name}={value:?}: expected one of {}", choices.join(", "))),
        }
    }
}

impl Config {
    /// `working_dir` and `library_dir` (the folder of the bridge library) supply the defaults
    /// of design.md 7.1.
    pub fn from_env(env: impl Fn(&str) -> Option<String>, working_dir: &str, library_dir: &str) -> Result<Config, String> {
        let env = Env(env);
        let fps_limit = match env.get("ASTROPORT_FPS_LIMIT").as_deref() {
            None => Limit::Off,
            Some("unlimited") => Limit::Unlimited,
            Some(_) => Limit::Fps(env.number("ASTROPORT_FPS_LIMIT", 10, 240, 0).map_err(|e| e + " or unlimited")?),
        };
        let dlss_pass = match env.get("ASTROPORT_DLSS_PASS") {
            None => 0,
            Some(value) => u64::from_str_radix(&value, 16)
                .ok()
                .filter(|hash| *hash != 0)
                .ok_or(format!("ASTROPORT_DLSS_PASS={value:?}: expected a hexadecimal shader hash"))?,
        };
        // Eleven indices: the constant words of jitter x, jitter y, output width, output height,
        // input width, input height, frame counter, motion flag; the motion and the colour image;
        // the constant buffer.
        let mut dlss_layout = [0u8; 16];
        if let Some(value) = env.get("ASTROPORT_DLSS_LAYOUT") {
            let fields: Vec<_> = value.split(',').map(|field| field.parse::<u8>().ok()).collect();
            let fits = |index: usize, field: &Option<u8>| field.is_some_and(|field| field < if index < 8 { 64 } else { 16 });
            if fields.len() != 11 || !fields.iter().enumerate().all(|(index, field)| fits(index, field)) || fields[8] == fields[9] {
                return Err(format!(
                    "ASTROPORT_DLSS_LAYOUT={value:?}: expected 11 comma-separated indices: eight constant words 0..63, \
                     two different images 0..15, one buffer 0..15"
                ));
            }
            for (slot, field) in dlss_layout.iter_mut().zip(fields) {
                *slot = field.unwrap_or(0);
            }
            dlss_layout[11] = 1;
        }
        let dlss_preset = match env.get("ASTROPORT_DLSS_PRESET").as_deref().map(str::as_bytes) {
            None => 0,
            Some([letter @ b'A'..=b'O']) => (letter - b'A') as u32 + 1,
            Some(_) => return Err("ASTROPORT_DLSS_PRESET: expected one letter A..O".into()),
        };
        let cache = env.get("XDG_CACHE_HOME").or_else(|| env.get("HOME").map(|home| home + "/.cache"));
        Ok(Config {
            width: env.number("ASTROPORT_WIDTH", 1, 7680, 0)?,
            height: env.number("ASTROPORT_HEIGHT", 1, 7680, 0)?,
            fullscreen: env.flag("ASTROPORT_FULLSCREEN", false)?,
            present_mode: env.choice("ASTROPORT_PRESENT_MODE", &["immediate", "fifo", "mailbox"], 0)?,
            shader_opt: env.choice("ASTROPORT_SHADER_OPT", &["none", "size", "performance"], 2)?,
            vblank_hz: env.number("ASTROPORT_VBLANK_HZ", 1, 1000, 60)?,
            wayland_native: env.flag("ASTROPORT_WAYLAND_NATIVE", false)?,
            app0: env.get("ASTROPORT_APP0").unwrap_or(format!("{working_dir}/app0")),
            window_title: env.get("ASTROPORT_WINDOW_TITLE"),
            fps_limit,
            low_latency: env.flag("ASTROPORT_LOW_LATENCY", false)?,
            dlss: env.flag("ASTROPORT_DLSS", false)?,
            dlss_pass,
            dlss_layout,
            dlss_reverse_depth: env.flag("ASTROPORT_DLSS_REVERSE_DEPTH", false)?,
            dlss_dir: env.get("ASTROPORT_DLSS_DIR").unwrap_or(library_dir.into()),
            dlss_preset,
            pad_prefer: match env.choice("ASTROPORT_PAD_PREFER", &["auto", "ps", "xbox", "switch", "generic"], u8::MAX)? {
                u8::MAX => 0,
                position => position + 1,
            },
            rumble_scale: env.number("ASTROPORT_RUMBLE_SCALE", 0, 200, 100)?,
            rumble_switch_floor: env.number("ASTROPORT_RUMBLE_SWITCH_FLOOR", 0, 255, 0)?,
            ds_rumble: env.flag("ASTROPORT_DS_RUMBLE", true)?,
            fsr_fg: env.flag("ASTROPORT_FSR_FG", false)?,
            fsr_dir: env.get("ASTROPORT_FSR_DIR").unwrap_or(library_dir.into()),
            ngx_data_dir: cache.unwrap_or(working_dir.into()) + "/astroport/ngx",
            haptic_gain: match env.get("APS5_PAD_VIBRATION_GAIN").and_then(|value| value.parse::<f32>().ok()) {
                Some(gain) if !gain.is_nan() => gain.clamp(0.0, 4.0),
                _ => 1.0,
            },
        })
    }

    /// The strings are leaked: they live for the process (design.md 3.3).
    pub fn embed(&self) -> KytyEmbedConfig {
        fn leak(text: &str) -> *const c_char {
            CString::new(text).unwrap_or_default().into_raw()
        }
        KytyEmbedConfig {
            size: size_of::<KytyEmbedConfig>() as u32,
            width: self.width,
            height: self.height,
            vblank_hz: self.vblank_hz,
            vibration_intensity: self.rumble_scale.min(100),
            rumble_scale: self.rumble_scale,
            rumble_switch_floor: self.rumble_switch_floor,
            low_latency_interval_us: interval_us(self.fps_limit),
            dlss_preset: self.dlss_preset,
            upscale_pass_hash: self.dlss_pass,
            fullscreen: self.fullscreen as u8,
            present_mode: self.present_mode,
            shader_optimization: self.shader_opt,
            wayland_native: self.wayland_native as u8,
            low_latency: self.low_latency as u8,
            dlss: self.dlss as u8,
            dlss_reverse_depth: self.dlss_reverse_depth as u8,
            pad_prefer: self.pad_prefer,
            ds_rumble: self.ds_rumble as u8,
            fsr_fg: self.fsr_fg as u8,
            app0_dir: leak(&self.app0),
            window_title: self.window_title.as_deref().map_or(std::ptr::null(), leak),
            dlss_dir: leak(&self.dlss_dir),
            ngx_data_dir: leak(&self.ngx_data_dir),
            fsr_dir: leak(&self.fsr_dir),
            upscale_pass_layout: self.dlss_layout,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    fn parse(vars: &[(&str, &str)]) -> Result<Config, String> {
        let lookup = |name: &str| vars.iter().find(|(key, _)| *key == name).map(|(_, value)| value.to_string());
        Config::from_env(lookup, "/run", "/run/libs")
    }

    #[test]
    fn defaults() {
        let config = parse(&[("HOME", "/home/u")]).unwrap();
        assert_eq!((config.width, config.height, config.fullscreen), (0, 0, false));
        assert_eq!((config.present_mode, config.shader_opt, config.vblank_hz), (0, 2, 60));
        assert_eq!(config.app0, "/run/app0");
        assert_eq!(config.fps_limit, Limit::Off);
        assert_eq!((config.dlss, config.dlss_pass, config.dlss_preset), (false, 0, 0));
        assert_eq!(config.dlss_layout, [0; 16]);
        assert_eq!((config.dlss_dir.as_str(), config.fsr_dir.as_str()), ("/run/libs", "/run/libs"));
        assert_eq!((config.pad_prefer, config.rumble_scale, config.rumble_switch_floor, config.ds_rumble), (0, 100, 0, true));
        assert_eq!(config.ngx_data_dir, "/home/u/.cache/astroport/ngx");
        assert_eq!(config.haptic_gain, 1.0);
        assert_eq!(config.window_title, None);
    }

    #[test]
    fn accepted_values() {
        let config = parse(&[
            ("ASTROPORT_WIDTH", "7680"),
            ("ASTROPORT_HEIGHT", "1"),
            ("ASTROPORT_FULLSCREEN", "1"),
            ("ASTROPORT_PRESENT_MODE", "mailbox"),
            ("ASTROPORT_SHADER_OPT", "none"),
            ("ASTROPORT_FPS_LIMIT", "30"),
            ("ASTROPORT_DLSS_PASS", "2936e5eca99d75e7"),
            ("ASTROPORT_DLSS_PRESET", "K"),
            ("ASTROPORT_DLSS_LAYOUT", "0,1,4,5,6,7,11,13,0,1,0"),
            ("ASTROPORT_PAD_PREFER", "xbox"),
            ("ASTROPORT_RUMBLE_SCALE", "150"),
            ("ASTROPORT_DS_RUMBLE", "0"),
            ("ASTROPORT_WINDOW_TITLE", "ASTRO BOT"),
            ("ASTROPORT_VBLANK_HZ", ""),
            ("XDG_CACHE_HOME", "/c"),
            ("APS5_PAD_VIBRATION_GAIN", "9"),
        ])
        .unwrap();
        assert_eq!((config.width, config.height, config.fullscreen), (7680, 1, true));
        assert_eq!((config.present_mode, config.shader_opt, config.vblank_hz), (2, 0, 60));
        assert_eq!(config.fps_limit, Limit::Fps(30));
        assert_eq!((config.dlss_pass, config.dlss_preset), (0x2936e5eca99d75e7, 11));
        assert_eq!((config.pad_prefer, config.ds_rumble), (3, false));
        assert_eq!((config.ngx_data_dir.as_str(), config.haptic_gain), ("/c/astroport/ngx", 4.0));
        assert_eq!(parse(&[("ASTROPORT_FPS_LIMIT", "unlimited")]).unwrap().fps_limit, Limit::Unlimited);

        let embed = config.embed();
        assert_eq!((embed.vibration_intensity, embed.rumble_scale), (100, 150));
        assert_eq!(embed.upscale_pass_layout, [0, 1, 4, 5, 6, 7, 11, 13, 0, 1, 0, 1, 0, 0, 0, 0]);
        assert_eq!(embed.low_latency_interval_us, 33_333);
        assert_eq!(unsafe { CStr::from_ptr(embed.window_title) }.to_str(), Ok("ASTRO BOT"));
        assert_eq!(unsafe { CStr::from_ptr(embed.app0_dir) }.to_str(), Ok("/run/app0"));
    }

    #[test]
    fn invalid_values_name_the_variable() {
        for (name, value) in [
            ("ASTROPORT_WIDTH", "0"),
            ("ASTROPORT_WIDTH", "7681"),
            ("ASTROPORT_HEIGHT", "720p"),
            ("ASTROPORT_FULLSCREEN", "yes"),
            ("ASTROPORT_PRESENT_MODE", "Immediate"),
            ("ASTROPORT_SHADER_OPT", "fast"),
            ("ASTROPORT_VBLANK_HZ", "0"),
            ("ASTROPORT_WAYLAND_NATIVE", "2"),
            ("ASTROPORT_FPS_LIMIT", "9"),
            ("ASTROPORT_FPS_LIMIT", "241"),
            ("ASTROPORT_FPS_LIMIT", "off"),
            ("ASTROPORT_LOW_LATENCY", "on"),
            ("ASTROPORT_DLSS", "true"),
            ("ASTROPORT_DLSS_PASS", "xyz"),
            ("ASTROPORT_DLSS_PASS", "0"),
            ("ASTROPORT_DLSS_REVERSE_DEPTH", "-1"),
            ("ASTROPORT_DLSS_PRESET", "P"),
            ("ASTROPORT_DLSS_PRESET", "AB"),
            ("ASTROPORT_DLSS_LAYOUT", "0,1,4,5,6,7,11,13,0,1"),
            ("ASTROPORT_DLSS_LAYOUT", "0,1,4,5,6,7,11,64,0,1,0"),
            ("ASTROPORT_DLSS_LAYOUT", "0,1,4,5,6,7,11,13,1,1,0"),
            ("ASTROPORT_DLSS_LAYOUT", "0,1,4,5,6,7,11,13,0,16,0"),
            ("ASTROPORT_DLSS_LAYOUT", "0,1,4,5,6,7,11,13,0,x,0"),
            ("ASTROPORT_PAD_PREFER", "first"),
            ("ASTROPORT_RUMBLE_SCALE", "201"),
            ("ASTROPORT_RUMBLE_SWITCH_FLOOR", "256"),
            ("ASTROPORT_DS_RUMBLE", "x"),
            ("ASTROPORT_FSR_FG", "2"),
        ] {
            let error = parse(&[(name, value)]).expect_err(name);
            assert!(error.starts_with(name) && error.contains("expected"), "{error}");
        }
    }
}
