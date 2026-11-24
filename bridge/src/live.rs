//! Live settings: `settings.apply` JSON -> actions and the `settings.applied` reply
//! (design.md 6.5). `decide` is pure; `live_setting` is the hook Kyty's session socket calls.

use crate::limiter::{self, Limit};
use crate::{audio, kyty};
use std::ffi::{c_char, c_int, CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// Frame caps that can be changed live: each has a "precise frame cap N fps" patch group.
const CAPS: [u32; 2] = [30, 60];

/// Set at start from the configuration.
pub static FULLSCREEN: AtomicBool = AtomicBool::new(false);

pub struct State {
    pub limit: Limit,
    pub fullscreen: bool,
}

#[derive(Default, Debug, PartialEq)]
pub struct Actions {
    pub fps: Option<u32>,
    /// (old, new): the executable patch groups to switch off and on
    pub cap_swap: Option<(u32, u32)>,
    pub size: Option<(u32, u32)>,
    pub haptic_gain: Option<f32>,
}

#[derive(Default)]
pub struct Decision {
    pub actions: Actions,
    /// raw JSON of the request's id
    id: String,
    /// (key, raw JSON value)
    applied: Vec<(String, String)>,
    values: Vec<(String, String)>,
    refused: Vec<(String, &'static str)>,
}

impl Decision {
    pub fn reply(&self) -> String {
        let object = |pairs: Vec<(&str, String)>| {
            let members: Vec<String> = pairs.iter().map(|(key, value)| format!("{}:{value}", quote(key))).collect();
            format!("{{{}}}", members.join(","))
        };
        let raw = |pairs: &[(String, String)]| object(pairs.iter().map(|(key, value)| (key.as_str(), value.clone())).collect());
        let refused = object(self.refused.iter().map(|(key, reason)| (key.as_str(), quote(reason))).collect());
        format!(
            r#"{{"type":"settings.applied","id":{},"applied":{},"values":{},"refused":{}}}"#,
            self.id,
            raw(&self.applied),
            raw(&self.values),
            refused
        )
    }

    fn refuse(&mut self, key: &str, reason: &'static str) {
        self.applied.retain(|(applied, _)| applied != key);
        self.refused.push((key.into(), reason));
    }
}

pub fn decide(apply: &str, now: &State) -> Decision {
    let mut decision = Decision { id: "null".into(), ..Default::default() };
    let Some(message) = json::object(apply) else { return decision };
    let member = |name: &str| message.iter().find(|(key, _)| key == name).map(|(_, value)| *value);
    decision.id = member("id").unwrap_or("null").into();
    let Some(values) = member("values").and_then(json::object) else { return decision };
    for (key, value) in values {
        let whole: Option<u32> = value.parse().ok();
        match key.as_str() {
            "display.fps" if whole.is_some_and(|fps| CAPS.contains(&fps)) => {
                let fps = whole.unwrap();
                match now.limit {
                    Limit::Fps(current) if current == fps => {}
                    Limit::Fps(current) if CAPS.contains(&current) => decision.actions.cap_swap = Some((current, fps)),
                    _ => {
                        decision.refuse(&key, "Applies at the next start");
                        continue;
                    }
                }
                decision.actions.fps = Some(fps);
                decision.applied.push((key, value.into()));
            }
            "display.resolution" if resolution(value).is_some() => {
                if now.fullscreen {
                    decision.refuse(&key, "Leave fullscreen first");
                } else {
                    decision.actions.size = resolution(value);
                    decision.applied.push((key, value.into()));
                }
            }
            "controller.haptics" if whole.is_some_and(|percent| (100..=400).contains(&percent)) => {
                decision.actions.haptic_gain = Some(whole.unwrap() as f32 / 100.0);
                decision.values.push((key, value.into()));
            }
            _ => decision.refuse(&key, "Not supported"),
        }
    }
    decision
}

/// `"<width>x<height>"` as raw JSON, both 1..7680.
fn resolution(raw: &str) -> Option<(u32, u32)> {
    let (width, height) = raw.strip_prefix('"')?.strip_suffix('"')?.split_once('x')?;
    let side = |text: &str| text.parse().ok().filter(|pixels| (1..=7680).contains(pixels));
    Some((side(width)?, side(height)?))
}

fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' | '\\' => out.extend(['\\', c]),
            c if c < ' ' => out += &format!("\\u{:04x}", c as u32),
            c => out.push(c),
        }
    }
    out + "\""
}

/// The patcher library's path in an `LD_AUDIT` value.
fn patcher_path(ld_audit: &str) -> Option<&str> {
    ld_audit.split([':', ' ']).find(|path| path.ends_with("libastro_patcher.so"))
}

/// `astro_patch_live(name, on)` of the patcher (patcher/src/lib.rs); non-zero = failed.
fn patch(name: &str, on: bool) -> c_int {
    type PatchLive = unsafe extern "C" fn(*const c_char, i32) -> i32;
    static PATCHER: OnceLock<Option<PatchLive>> = OnceLock::new();
    let patcher = PATCHER.get_or_init(|| unsafe {
        // The audit copy lives in its own namespace; this loads a second one into ours.
        let path = CString::new(patcher_path(&std::env::var("LD_AUDIT").ok()?)?).ok()?;
        let library = libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        let symbol = if library.is_null() { library } else { libc::dlsym(library, c"astro_patch_live".as_ptr()) };
        (!symbol.is_null()).then(|| std::mem::transmute::<*mut libc::c_void, PatchLive>(symbol))
    });
    match patcher {
        Some(patch_live) => unsafe { patch_live(CString::new(name).unwrap().as_ptr(), on as i32) },
        None => -10,
    }
}

fn apply(request: &str) -> String {
    let state = State { limit: limiter::limit(), fullscreen: FULLSCREEN.load(Ordering::Relaxed) };
    let mut decision = decide(request, &state);
    if let Some((old, new)) = decision.actions.cap_swap {
        let name = |fps: u32| format!("precise frame cap {fps} fps");
        if patch(&name(old), false) != 0 || patch(&name(new), true) != 0 {
            patch(&name(old), true);
            decision.refuse("display.fps", "Applies at the next start");
            decision.actions.fps = None;
        }
    }
    if let Some(fps) = decision.actions.fps {
        limiter::set_limit(Limit::Fps(fps));
    }
    if let Some((width, height)) = decision.actions.size {
        unsafe { (kyty().set_window_size.unwrap())(width, height) };
    }
    if let Some(gain) = decision.actions.haptic_gain {
        audio::set_haptic_gain(gain);
    }
    decision.reply()
}

/// `KytyEmbedHooks.live_setting`: the reply stays valid until the next call.
pub unsafe extern "C" fn live_setting(apply_json: *const c_char) -> *const c_char {
    static REPLY: Mutex<Option<CString>> = Mutex::new(None);
    let reply = apply(CStr::from_ptr(apply_json).to_str().unwrap_or(""));
    REPLY.lock().unwrap().insert(CString::new(reply).unwrap_or_default()).as_ptr()
}

/// Just enough JSON: the members of one object, values as raw text.
mod json {
    fn skip_space(text: &str, at: usize) -> usize {
        at + text[at..].len() - text[at..].trim_start().len()
    }

    /// `at` is the opening quote; returns the decoded string and the index after the closing quote.
    fn string(text: &str, at: usize) -> Option<(String, usize)> {
        let mut out = String::new();
        let mut chars = text[at..].char_indices().skip(1);
        while let Some((offset, c)) = chars.next() {
            match c {
                '"' => return Some((out, at + offset + 1)),
                '\\' => match chars.next()?.1 {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    'b' => out.push('\u{8}'),
                    'f' => out.push('\u{c}'),
                    'u' => {
                        let hex: String = chars.by_ref().take(4).map(|(_, c)| c).collect();
                        out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                    }
                    other => out.push(other),
                },
                c => out.push(c),
            }
        }
        None
    }

    /// Index after the value starting at `at`.
    fn value_end(text: &str, at: usize) -> Option<usize> {
        let bytes = text.as_bytes();
        match *bytes.get(at)? {
            b'"' => Some(string(text, at)?.1),
            b'{' | b'[' => {
                let (mut depth, mut i) = (0, at);
                loop {
                    match *bytes.get(i)? {
                        b'"' => i = string(text, i)?.1 - 1,
                        b'{' | b'[' => depth += 1,
                        b'}' | b']' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
            }
            _ => {
                let end = text[at..].find([',', '}', ']', ' ', '\n', '\t', '\r']).map_or(text.len(), |n| at + n);
                (end > at).then_some(end)
            }
        }
    }

    pub fn object(text: &str) -> Option<Vec<(String, &str)>> {
        let mut members = Vec::new();
        let mut at = skip_space(text, 0);
        if text.as_bytes().get(at) != Some(&b'{') {
            return None;
        }
        at = skip_space(text, at + 1);
        if text.as_bytes().get(at) == Some(&b'}') {
            return Some(members);
        }
        loop {
            if text.as_bytes().get(at) != Some(&b'"') {
                return None;
            }
            let (key, after_key) = string(text, at)?;
            at = skip_space(text, after_key);
            if text.as_bytes().get(at) != Some(&b':') {
                return None;
            }
            at = skip_space(text, at + 1);
            let end = value_end(text, at)?;
            members.push((key, &text[at..end]));
            at = skip_space(text, end);
            match text.as_bytes().get(at)? {
                b',' => at = skip_space(text, at + 1),
                b'}' => return Some(members),
                _ => return None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT_30: State = State { limit: Limit::Fps(30), fullscreen: false };

    fn request(values: &str) -> String {
        format!(r#"{{"type":"settings.apply","id":7,"values":{values}}}"#)
    }

    #[test]
    fn frame_cap() {
        let d = decide(&request(r#"{"display.fps":60}"#), &AT_30);
        assert_eq!(d.actions, Actions { fps: Some(60), cap_swap: Some((30, 60)), ..Default::default() });
        assert_eq!(d.reply(), r#"{"type":"settings.applied","id":7,"applied":{"display.fps":60},"values":{},"refused":{}}"#);

        let d = decide(&request(r#"{"display.fps":30}"#), &AT_30);
        assert_eq!(d.actions, Actions { fps: Some(30), ..Default::default() }, "same value: no patch swap");

        for limit in [Limit::Off, Limit::Unlimited, Limit::Fps(120)] {
            let d = decide(&request(r#"{"display.fps":60}"#), &State { limit, fullscreen: false });
            assert_eq!(d.actions, Actions::default());
            assert!(d.reply().contains(r#""refused":{"display.fps":"Applies at the next start"}"#), "{}", d.reply());
        }
        for bad in ["45", "\"60\"", "60.5", "null"] {
            let d = decide(&request(&format!(r#"{{"display.fps":{bad}}}"#)), &AT_30);
            assert_eq!(d.actions, Actions::default());
            assert!(d.reply().contains(r#""refused":{"display.fps":"Not supported"}"#), "{}", d.reply());
        }
    }

    #[test]
    fn resolution() {
        let d = decide(&request(r#"{"display.resolution":"2560x1440"}"#), &AT_30);
        assert_eq!(d.actions.size, Some((2560, 1440)));
        assert!(d.reply().contains(r#""applied":{"display.resolution":"2560x1440"}"#));
        for bad in ["\"abcx\"", "\"0x0\"", "\"9000x9000\"", "\"1920x\"", "\"1920x1080x2\"", "1920"] {
            let d = decide(&request(&format!(r#"{{"display.resolution":{bad}}}"#)), &AT_30);
            assert!(d.actions.size.is_none() && d.reply().contains("Not supported"), "{bad}");
        }
        let d = decide(&request(r#"{"display.resolution":"1920x1080"}"#), &State { limit: Limit::Off, fullscreen: true });
        assert!(d.actions.size.is_none() && d.reply().contains("Leave fullscreen first"));
    }

    #[test]
    fn haptics_and_a_mixed_request() {
        let d = decide(&request(r#" { "controller.haptics" : 300, "display.fps": "x", "no\"pe": {"a":[1,"}"]} } "#), &AT_30);
        assert_eq!(d.actions, Actions { haptic_gain: Some(3.0), ..Default::default() });
        assert_eq!(
            d.reply(),
            r#"{"type":"settings.applied","id":7,"applied":{},"values":{"controller.haptics":300},"refused":{"display.fps":"Not supported","no\"pe":"Not supported"}}"#
        );
        for bad in ["50", "450", "\"200\"", "2.5"] {
            let d = decide(&request(&format!(r#"{{"controller.haptics":{bad}}}"#)), &AT_30);
            assert!(d.actions.haptic_gain.is_none() && d.reply().contains("Not supported"), "{bad}");
        }
    }

    #[test]
    fn requests_without_values_or_not_json() {
        let empty = r#"{"type":"settings.applied","id":8,"applied":{},"values":{},"refused":{}}"#;
        assert_eq!(decide(r#"{"type":"settings.apply","id":8}"#, &AT_30).reply(), empty);
        assert_eq!(decide(r#"{"id":8,"values":[1]}"#, &AT_30).reply(), empty);
        assert_eq!(decide(r#"{"id":"a-1","values":{}}"#, &AT_30).reply(), empty.replace('8', "\"a-1\""));
        for bad in ["", "[]", "{\"id\":", "{\"values\":{\"display.fps\":60}"] {
            let d = decide(bad, &AT_30);
            assert_eq!(d.actions, Actions::default(), "{bad}");
            assert_eq!(d.reply(), empty.replace('8', "null"));
        }
    }

    #[test]
    fn patcher_is_found_in_ld_audit() {
        assert_eq!(patcher_path("/a/libfoo.so:/run/libs/libastro_patcher.so"), Some("/run/libs/libastro_patcher.so"));
        assert_eq!(patcher_path("/run/libastro_patcher.so /b.so"), Some("/run/libastro_patcher.so"));
        assert_eq!(patcher_path("/a/libfoo.so"), None);
    }
}
