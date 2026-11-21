// Sheet §3.3: AGS_JOB_PROGRESS_FD is the decimal number (3 or more) of an inherited pipe's write end; AGS_JOB_ID (lowercase UUID) is also set and unused here.
// Sheet §3.3: line keys step/done/total/unit/message, all optional; `step` is the 0-based index into the plan's steps (past the end clamps to the last); done <= total; unit 1-16 chars; message one line, cut to 200 chars.
// Sheet §3.3: final lines {"outcome":"completed"} and {"error":{"category","message"}}, category an ErrorCategory word (temporary account storage integrity unsupported conflict cancelled internal); no final line = failed, internal.
// Sheet §3.4: cancel = SIGTERM to the group, SIGKILL after 10 s; job.plan says "cleanup": false (the next build removes .stage/), so the host never runs a cleanup plan.
// SHEET: open — the sheet sends the worker's stderr to the AGS log but does not say where its stdout goes; this runner writes nothing to stdout while AGS_JOB_PROGRESS_FD is set.

use crate::JobError;
use serde_json::{json, Value};
use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{FromRawFd, RawFd};

pub const STEPS: [&str; 5] = [
    "Check the game dump",
    "Unwrap the game files",
    "Convert for Linux",
    "Set up the game folder",
    "Record the result",
];

const MAX_LINE_BYTES: usize = 4096;

pub struct Progress {
    writer: Box<dyn Write>,
}

impl Progress {
    pub fn from_env() -> Self {
        let fd = std::env::var("AGS_JOB_PROGRESS_FD")
            .ok()
            .and_then(|value| value.parse::<RawFd>().ok())
            .filter(|&fd| fd > 2);

        match fd {
            // SAFETY: The host passes ownership of the inherited pipe's write end.
            Some(fd) => Self::to_writer(Box::new(unsafe { File::from_raw_fd(fd) })),
            None => Self::to_writer(Box::new(io::stdout())),
        }
    }

    pub fn to_writer(w: Box<dyn Write>) -> Self {
        Self { writer: w }
    }

    pub fn step(&mut self, index: usize, done: u64, total: u64, unit: &str, message: &str) {
        if message.chars().nth(200).is_some() {
            let _ = writeln!(io::stderr(), "{message}");
        }
        let unit: String = unit
            .chars()
            .take_while(|&ch| ch != '\n' && ch != '\r')
            .take(16)
            .collect();
        let mut line =
            json!({"step": index, "done": done.min(total), "total": total, "message": ""});
        if !unit.is_empty() {
            line["unit"] = json!(unit);
        }
        self.write_message_line(line, "/message", message);
    }

    pub fn completed(&mut self) {
        self.write_line(json!({"outcome": "completed"}).to_string());
    }

    pub fn error(&mut self, e: &JobError) {
        let category = match e.category {
            "temporary" | "account" | "storage" | "integrity" | "unsupported" | "conflict"
            | "cancelled" => e.category,
            _ => "internal",
        };
        let _ = writeln!(io::stderr(), "{}", e.message);
        let line = json!({"error": {"category": category, "message": ""}});
        self.write_message_line(line, "/error/message", &e.message);
    }

    fn write_message_line(&mut self, mut value: Value, pointer: &str, message: &str) {
        let mut end = message.len().min(MAX_LINE_BYTES);
        loop {
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            *value.pointer_mut(pointer).expect("message field") = json!(&message[..end]);
            let line = value.to_string();
            if line.len() < MAX_LINE_BYTES {
                self.write_line(line);
                return;
            }
            // An escaped byte takes at most six bytes in JSON. Remove only as much as needed.
            end -= (line.len() + 1 - MAX_LINE_BYTES).div_ceil(6);
        }
    }

    fn write_line(&mut self, mut line: String) {
        line.push('\n');
        let _ = self.writer.write_all(line.as_bytes());
        let _ = self.writer.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct Buf(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Buf {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn lines(f: impl FnOnce(&mut Progress)) -> Vec<serde_json::Value> {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let mut p = Progress::to_writer(Box::new(Buf(buf.clone())));
        f(&mut p);
        let text = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        text.lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn step_line_has_the_protocol_fields() {
        // index 1 = "Unwrap the game files" (build step 2); the wire `step` is 0-based (sheet §3.3)
        let v = lines(|p| p.step(1, 120, 400, "files", "libc.prx"));
        assert_eq!(
            v,
            vec![
                serde_json::json!({"step": 1, "done": 120, "total": 400, "unit": "files", "message": "libc.prx"})
            ]
        );
    }

    #[test]
    fn completed_and_error_lines() {
        let e = JobError {
            category: "unsupported",
            message: "Install the Noto fonts".into(),
        };
        let v = lines(|p| {
            p.completed();
            p.error(&e);
        });
        assert_eq!(v[0], serde_json::json!({"outcome": "completed"}));
        assert_eq!(
            v[1],
            serde_json::json!({"error": {"category": "unsupported", "message": "Install the Noto fonts"}})
        );
    }

    #[test]
    fn steps_are_the_five_design_steps() {
        assert_eq!(STEPS.len(), 5);
        assert_eq!(STEPS[2], "Convert for Linux");
    }
}
