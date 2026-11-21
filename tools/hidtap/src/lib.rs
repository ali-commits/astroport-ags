//! LD_PRELOAD library: logs the haptic samples of DualSense Bluetooth audio reports (0x39) a process
//! writes, so two programs' haptic output can be compared. HIDTAP_OUT=<file> (required): each record
//! is u64 nanoseconds (CLOCK_MONOTONIC) followed by 128 signed bytes (64 stereo frames at 3 kHz).
use std::ffi::{c_char, c_int, c_void, CStr};
use std::io::Write;
use std::sync::{Mutex, OnceLock};

type WriteFn = unsafe extern "C" fn(c_int, *const c_void, usize) -> isize;
extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn clock_gettime(clock: c_int, ts: *mut [i64; 2]) -> c_int;
}
const RTLD_NEXT: *mut c_void = -1isize as *mut c_void;

fn real_write() -> WriteFn {
    static REAL: OnceLock<usize> = OnceLock::new();
    let p = *REAL.get_or_init(|| unsafe { dlsym(RTLD_NEXT, CStr::from_bytes_with_nul(b"write\0").unwrap().as_ptr()) as usize });
    unsafe { std::mem::transmute::<usize, WriteFn>(p) }
}

fn out() -> Option<&'static Mutex<std::fs::File>> {
    static FILE: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    FILE.get_or_init(|| {
        let path = std::env::var("HIDTAP_OUT").ok()?;
        std::fs::OpenOptions::new().create(true).append(true).open(path).ok().map(Mutex::new)
    })
    .as_ref()
}

#[no_mangle]
pub unsafe extern "C" fn write(fd: c_int, buf: *const c_void, count: usize) -> isize {
    if count == 547 && !buf.is_null() && *(buf as *const u8) == 0x39 {
        if let Some(file) = out() {
            let bytes = std::slice::from_raw_parts(buf as *const u8, count);
            let mut ts = [0i64; 2];
            clock_gettime(1, &mut ts);
            let ns = (ts[0] as u64) * 1_000_000_000 + ts[1] as u64;
            if let Ok(mut f) = file.lock() {
                let _ = f.write_all(&ns.to_le_bytes());
                let _ = f.write_all(&bytes[12..140]);
            }
        }
    }
    real_write()(fd, buf, count)
}
