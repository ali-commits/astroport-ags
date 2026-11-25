//! Stand-in AGS host for Kyty's session socket (protocol 2), to test the game under a session
//! without the installed AGS. Kyty is the client; this binds a 0600 SOCK_SEQPACKET socket in a
//! fresh 0700 folder, prints the variables to give the game, waits for it and runs the checks.
//!
//! It never sends "release": without Vulkan it cannot signal a slot's DONE semaphore, and a
//! release before DONE would stall the game's graphics queue. Kyty therefore sends at most 3
//! frames per pool (one per lease) and drops later captures; presentation goes on, which the
//! game_fps metric shows. A continuous frame-rate probe needs Vulkan (ash), which is not in
//! Cargo.lock.
//!
//! One summary line per check (PASS/FAIL); exit status 1 if any check failed, 2 on bad usage.
//! Like embed_smoke.rs it writes its JSON by hand and reads fields with plain string searches.

use std::ffi::c_int;
use std::time::{Duration, Instant};

const USAGE: &str = "usage: session_host [--frames SECS] [--reconnect N] [--settings] [--frameless]
                    [--socket-dir DIR] [--wait SECS]
  --frames SECS    accept the frame hello, then watch frames and metrics for SECS seconds:
                   exactly 3 frames (no release is sent), presentation keeps going
  --reconnect N    detach and re-attach N times; the hello must come back each time
                   (frames mode: a new epoch and a frame each time)
  --settings       settings.apply display.fps 30, display.fps 60, display.resolution 1280x720,
                   controller.haptics 200; prints the replies (frame-less: also the output message)
  --frameless      refuse the frame hello; metrics (and --settings) must still work
  --socket-dir DIR the folder to create (must not exist); default a fresh one under
                   $XDG_RUNTIME_DIR or the temp folder
  --wait SECS      how long to wait for the game's first hello (default 600: the frame hello
                   needs the first presented game frame)
Prints 'export AGS_SESSION_SOCKET=... AGS_SESSION_PROTOCOL=2' first, then waits for the game.";

#[derive(Default)]
struct Options {
    frames: Option<u64>,
    reconnect: u32,
    settings: bool,
    frameless: bool,
    dir: Option<String>,
    wait: u64,
}

fn options() -> Options {
    let mut options = Options { wait: 600, ..Default::default() };
    let mut args = std::env::args().skip(1);
    let usage = |what: &str| -> ! {
        eprintln!("session_host: {what}\n{USAGE}");
        std::process::exit(2)
    };
    let number = |args: &mut dyn Iterator<Item = String>, flag: &str| -> u64 {
        args.next().and_then(|v| v.parse().ok()).unwrap_or_else(|| usage(&format!("{flag} needs a number")))
    };
    let mut any = false;
    while let Some(arg) = args.next() {
        any |= arg != "--socket-dir" && arg != "--wait";
        match arg.as_str() {
            "--frames" => options.frames = Some(number(&mut args, "--frames")),
            "--reconnect" => options.reconnect = number(&mut args, "--reconnect") as u32,
            "--settings" => options.settings = true,
            "--frameless" => options.frameless = true,
            "--wait" => options.wait = number(&mut args, "--wait"),
            "--socket-dir" => options.dir = Some(args.next().unwrap_or_else(|| usage("--socket-dir needs a folder"))),
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0)
            }
            _ => usage(&format!("unknown argument {arg}")),
        }
    }
    if !any {
        usage("choose at least one scenario");
    }
    options
}

/// Raw JSON value of `key` (first match, nested included): enough for Kyty's compact,
/// flat-valued packets. Strings keep their quotes.
fn field<'a>(packet: &'a str, key: &str) -> Option<&'a str> {
    let start = packet.find(&format!("\"{key}\":"))? + key.len() + 3;
    let rest = &packet[start..];
    Some(&rest[..rest.find([',', '}']).unwrap_or(rest.len())])
}

fn number(packet: &str, key: &str) -> Option<u64> {
    field(packet, key)?.parse().ok()
}

fn kind(packet: &str) -> &str {
    field(packet, "type").unwrap_or("").trim_matches('"')
}

struct Checks {
    failed: u32,
}

impl Checks {
    fn check(&mut self, ok: bool, what: &str) -> bool {
        println!("{} {what}", if ok { "PASS" } else { "FAIL" });
        self.failed += !ok as u32;
        ok
    }
}

struct Packet {
    text: String,
    fds: usize,
    truncated: bool,
}

/// One attached connection and what it has seen.
struct Session {
    fd: c_int,
    /// 0 when frame-less
    epoch: u64,
    closed: bool,
    /// (epoch, slot, sequence, pts_ns, arrival)
    frames: Vec<(u64, u64, u64, u64, Instant)>,
    /// (game_fps, arrival)
    metrics: Vec<(f64, Instant)>,
}

impl Session {
    fn send(&mut self, packet: &str) -> bool {
        println!("  host -> {packet}");
        let sent = unsafe { libc::send(self.fd, packet.as_ptr().cast(), packet.len(), libc::MSG_NOSIGNAL) };
        sent == packet.len() as isize
    }

    /// One packet before `deadline`, its descriptors closed at once (this host never uses them).
    fn receive(&mut self, deadline: Instant) -> Option<Packet> {
        if self.closed {
            return None;
        }
        let left = deadline.saturating_duration_since(Instant::now()).as_millis().min(i32::MAX as u128) as c_int;
        let mut poller = libc::pollfd { fd: self.fd, events: libc::POLLIN, revents: 0 };
        if unsafe { libc::poll(&mut poller, 1, left) } <= 0 {
            return None;
        }
        let mut bytes = [0u8; 8192];
        let mut iov = libc::iovec { iov_base: bytes.as_mut_ptr().cast(), iov_len: bytes.len() };
        let mut control = [0u64; 32]; // room for 16+ descriptors, aligned for cmsghdr
        let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen = std::mem::size_of_val(&control) as _;
        let count = unsafe { libc::recvmsg(self.fd, &mut message, libc::MSG_CMSG_CLOEXEC) };
        if count <= 0 {
            println!("  game closed the connection");
            self.closed = true;
            return None;
        }
        let mut fds = 0;
        unsafe {
            let mut header = libc::CMSG_FIRSTHDR(&message);
            while !header.is_null() {
                if (*header).cmsg_level == libc::SOL_SOCKET && (*header).cmsg_type == libc::SCM_RIGHTS {
                    let n = ((*header).cmsg_len as usize - libc::CMSG_LEN(0) as usize) / 4;
                    let data = libc::CMSG_DATA(header) as *const c_int;
                    for i in 0..n {
                        libc::close(data.add(i).read_unaligned());
                    }
                    fds += n;
                }
                header = libc::CMSG_NXTHDR(&message, header);
            }
        }
        let truncated = message.msg_flags & (libc::MSG_TRUNC | libc::MSG_CTRUNC) != 0;
        let text = String::from_utf8_lossy(&bytes[..count as usize]).into_owned();
        let now = Instant::now();
        match kind(&text) {
            "frame" => {
                let get = |key| number(&text, key).unwrap_or(u64::MAX);
                self.frames.push((get("epoch"), get("slot"), get("sequence"), get("pts_ns"), now));
                println!("  game -> {text}");
            }
            "metrics" => {
                let fps = field(&text, "game_fps").and_then(|v| v.parse().ok()).unwrap_or(-1.0);
                self.metrics.push((fps, now));
                println!("  game -> {text}");
            }
            _ => println!("  game -> {text}{}", if fds > 0 { format!(" (+{fds} fds)") } else { String::new() }),
        }
        Some(Packet { text, fds, truncated })
    }

    /// The next packet of type `wanted` before `deadline`; frames and metrics are recorded on the way.
    fn wait_for(&mut self, wanted: &str, deadline: Instant) -> Option<Packet> {
        while let Some(packet) = self.receive(deadline) {
            if kind(&packet.text) == wanted {
                return Some(packet);
            }
        }
        None
    }

    /// Records everything until `deadline`.
    fn watch(&mut self, until: Instant) {
        while Instant::now() < until && self.receive(until).is_some() {}
    }

    fn last_fps(&self) -> Option<f64> {
        self.metrics.iter().rev().map(|m| m.0).find(|fps| *fps >= 0.0)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

/// The session socket Kyty's transport accepts: a 0600 SOCK_SEQPACKET socket in a 0700 folder.
fn listen(dir: &std::path::Path) -> c_int {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    if let Err(e) = std::fs::DirBuilder::new().mode(0o700).create(dir) {
        eprintln!("session_host: cannot create {}: {e}", dir.display());
        std::process::exit(2);
    }
    let path = dir.join("s");
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC, 0) };
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_family = libc::AF_UNIX as u16;
    let bytes = path.to_str().unwrap().as_bytes();
    if bytes.len() >= address.sun_path.len() {
        eprintln!("session_host: socket path too long: {}", path.display());
        let _ = std::fs::remove_dir_all(dir);
        std::process::exit(2);
    }
    for (to, from) in address.sun_path.iter_mut().zip(bytes) {
        *to = *from as _;
    }
    let size = std::mem::size_of::<libc::sockaddr_un>() as u32;
    let bound = unsafe { libc::bind(fd, (&address as *const libc::sockaddr_un).cast(), size) };
    if bound != 0 || std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).is_err() || unsafe { libc::listen(fd, 1) } != 0 {
        eprintln!("session_host: bind or listen on {} failed", path.display());
        let _ = std::fs::remove_dir_all(dir);
        std::process::exit(2);
    }
    println!("export AGS_SESSION_SOCKET={} AGS_SESSION_PROTOCOL=2", path.display());
    fd
}

/// Accepts the game, sends welcome and answers its hello (accepted, or refused then accepted
/// frame-less). None (after a FAIL line) if any step is missing.
fn attach(listener: c_int, wait: Duration, refuse: bool, round: &str, checks: &mut Checks) -> Option<Session> {
    let deadline = Instant::now() + wait;
    let mut poller = libc::pollfd { fd: listener, events: libc::POLLIN, revents: 0 };
    let left = wait.as_millis().min(i32::MAX as u128) as c_int;
    let connected = unsafe { libc::poll(&mut poller, 1, left) } == 1;
    // The folder is 0700, so only this user can connect: no SO_PEERCRED check needed here.
    let fd = if connected { unsafe { libc::accept4(listener, std::ptr::null_mut(), std::ptr::null_mut(), libc::SOCK_CLOEXEC) } } else { -1 };
    if !checks.check(fd >= 0, &format!("{round}: the game connected within {} s", wait.as_secs())) {
        return None;
    }
    let mut session = Session { fd, epoch: 0, closed: false, frames: Vec::new(), metrics: Vec::new() };
    session.send(r#"{"type":"welcome","version":2}"#);
    let started = Instant::now();
    let hello = session.wait_for("hello", deadline)?;
    let frames = field(&hello.text, "frames") == Some("true");
    println!("  hello after {} ms", started.elapsed().as_millis());
    if frames {
        let epoch = number(&hello.text, "epoch").unwrap_or(0);
        let well_formed = hello.fds == 9
            && !hello.truncated
            && epoch > 0
            && field(&hello.text, "transport") == Some("\"vulkan-buffer-fd\"")
            && field(&hello.text, "fourcc") == Some("\"BGRA\"")
            && number(&hello.text, "slots") == Some(3);
        if !checks.check(well_formed, &format!("{round}: frame hello, epoch {epoch}, 9 descriptors (got {})", hello.fds)) {
            return None;
        }
        if !refuse {
            session.send(&format!(r#"{{"type":"accepted","version":2,"epoch":{epoch},"transport":"vulkan-buffer-fd"}}"#));
            session.epoch = epoch;
            return Some(session);
        }
        session.send(r#"{"type":"refused","version":2}"#);
        let fallback = session.wait_for("hello", Instant::now() + Duration::from_secs(5));
        let ok = fallback.is_some_and(|h| field(&h.text, "frames") == Some("false") && h.fds == 0);
        if !checks.check(ok, &format!("{round}: refused -> frame-less hello")) {
            return None;
        }
    } else if !checks.check(hello.fds == 0 && !hello.truncated, &format!("{round}: frame-less hello, no descriptors")) {
        return None;
    }
    session.send(r#"{"type":"accepted","version":2,"frames":false}"#);
    Some(session)
}

fn metrics_check(session: &mut Session, round: &str, checks: &mut Checks) {
    // The first sample is only the baseline: the first packet comes after about 2 intervals.
    let deadline = Instant::now() + Duration::from_secs(5);
    while session.last_fps().is_none() && session.receive(deadline).is_some() {}
    let fps = session.last_fps();
    checks.check(fps.is_some_and(|fps| fps > 0.0), &format!("{round}: metrics arrive, game_fps {fps:?}"));
}

fn settings(session: &mut Session, checks: &mut Checks) {
    let cases = [
        (1, "display.fps", "30"),
        (2, "display.fps", "60"),
        (3, "display.resolution", "\"1280x720\""),
        (4, "controller.haptics", "200"),
    ];
    for (id, key, value) in cases {
        session.send(&format!(r#"{{"type":"settings.apply","id":{id},"values":{{"{key}":{value}}}}}"#));
        let reply = session.wait_for("settings.applied", Instant::now() + Duration::from_secs(5));
        let Some(reply) = reply else {
            checks.check(false, &format!("settings {key}={value}: no settings.applied within 5 s"));
            continue;
        };
        let answered = number(&reply.text, "id") == Some(id) && reply.text.contains(&format!("\"{key}\":"));
        // One key per apply, so the cut at the first ',' or '}' still covers the whole object.
        let applied = field(&reply.text, "applied").is_some_and(|v| v.contains(key));
        checks.check(answered, &format!("settings {key}={value}: {}", if applied { "applied" } else { "answered, not in applied (see the reply)" }));
        // Frame-less only: an applied resolution, or a changed fps, is followed by "output".
        if session.epoch == 0 && applied && key != "controller.haptics" {
            let output = session.wait_for("output", Instant::now() + Duration::from_secs(2));
            // Kyty's output size is the game's configured render size (Config::GetScreenWidth), not
            // the window, so only the fps is compared; for a resolution the message itself is checked.
            let want = if key == "display.fps" { format!("\"fps\":{value}") } else { "\"width\":".into() };
            let ok = output.as_ref().is_some_and(|o| o.text.contains(&want));
            // display.fps 30 when the cap already is 30 changes nothing and sends no output.
            if id != 1 || output.is_some() {
                checks.check(ok, &format!("settings {key}={value}: output message with {want}"));
            }
        }
    }
}

fn frames(session: &mut Session, seconds: u64, checks: &mut Checks) {
    if !checks.check(session.epoch != 0, "frames: the game offered frames") {
        return;
    }
    let start = Instant::now();
    session.watch(start + Duration::from_secs(seconds));
    let epoch = session.epoch;
    let list = &session.frames;
    let first = list.first().map(|f| f.4.duration_since(start).as_millis());
    let span_ms = list.last().zip(list.first()).map(|(l, f)| l.3.saturating_sub(f.3) / 1_000_000);
    println!("  {} frames in {seconds} s (first after {first:?} ms, pts span {span_ms:?} ms)", list.len());
    checks.check(list.len() == 3, &format!("frames: exactly 3 frames without release (got {})", list.len()));
    let mut slots: Vec<u64> = list.iter().map(|f| f.1).collect();
    slots.sort();
    slots.dedup();
    let ordered = list.windows(2).all(|w| w[0].2 < w[1].2 && w[0].3 <= w[1].3);
    checks.check(
        list.iter().all(|f| f.0 == epoch && f.1 < 3) && slots.len() == list.len() && ordered,
        "frames: this epoch, distinct slots < 3, increasing sequence and pts",
    );
    // Only samples from the second half of the watch: a stale pre-watch value must not hide a stall.
    let late = start + Duration::from_secs(seconds) / 2;
    let fps = session.metrics.iter().rev().find(|m| m.1 >= late && m.0 >= 0.0).map(|m| m.0);
    checks.check(fps.is_some_and(|fps| fps > 1.0), &format!("frames: game still presents with all slots held, late game_fps {fps:?}"));
}

fn main() {
    let options = options();
    let dir = options.dir.clone().map(std::path::PathBuf::from).unwrap_or_else(|| {
        let base = std::env::var_os("XDG_RUNTIME_DIR").map(Into::into).unwrap_or_else(std::env::temp_dir);
        base.join(format!("kyty-session-host.{}", std::process::id()))
    });
    let listener = listen(&dir);
    let mut checks = Checks { failed: 0 };
    let wait = Duration::from_secs(options.wait);
    let run = |checks: &mut Checks| {
        let mut session = attach(listener, wait, options.frameless, "attach", checks)?;
        session.send(r#"{"type":"metrics.subscribe","keys":["game_fps","frame_time_ms"],"interval_ms":1000}"#);
        if options.frameless {
            checks.check(session.epoch == 0, "frameless: attached frame-less");
        }
        metrics_check(&mut session, "attach", checks);
        if options.settings {
            settings(&mut session, checks);
        }
        if let Some(seconds) = options.frames {
            frames(&mut session, seconds, checks);
        }
        let mut epoch = session.epoch;
        for round in 1..=options.reconnect {
            drop(session); // detach: Kyty sees POLLHUP and reconnects after about 1 s
            let name = format!("reconnect {round}/{}", options.reconnect);
            session = attach(listener, Duration::from_secs(60), false, &name, checks)?;
            session.send(r#"{"type":"metrics.subscribe","keys":["game_fps"],"interval_ms":1000}"#);
            if epoch != 0 {
                checks.check(session.epoch > epoch, &format!("{name}: new epoch {} after {epoch}", session.epoch));
                let deadline = Instant::now() + Duration::from_secs(10);
                while session.frames.is_empty() && session.receive(deadline).is_some() {}
                checks.check(!session.frames.is_empty(), &format!("{name}: a frame of the new pool within 10 s"));
            }
            metrics_check(&mut session, &name, checks);
            epoch = session.epoch;
        }
        Some(())
    };
    let completed = run(&mut checks).is_some();
    if !completed && checks.failed == 0 {
        checks.check(false, "the game answered before the timeout (no hello, or it closed the connection)");
    }
    unsafe { libc::close(listener) };
    let _ = std::fs::remove_dir_all(&dir);
    println!("session_host: {}", if checks.failed == 0 { "ALL PASS".into() } else { format!("{} FAILED", checks.failed) });
    std::process::exit((checks.failed > 0) as i32);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_of_kyty_packets() {
        let frame = r#"{"epoch":2,"pts_ns":123,"sequence":7,"slot":1,"type":"frame"}"#;
        assert_eq!(kind(frame), "frame");
        assert_eq!(number(frame, "slot"), Some(1));
        assert_eq!(number(frame, "pts_ns"), Some(123));
        let metrics = r#"{"t_ms":5,"type":"metrics","values":{"frame_time_ms":16.6,"game_fps":60.2}}"#;
        assert_eq!(field(metrics, "game_fps"), Some("60.2"));
        assert_eq!(field(r#"{"frames":false,"type":"hello"}"#, "frames"), Some("false"));
        assert_eq!(field(metrics, "missing"), None);
    }
}
