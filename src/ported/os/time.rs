//! Port of `src/nvim/os/time.c` (not vendored under `csrc/`; the names appear as
//! calls in the vendored eval tree, so the drift gate recognizes them).
//!
//! Only the monotonic clock used by `profile.c`/`reltime()` is ported here.
#![allow(non_snake_case)]

use nix::libc;
use nix::time::{clock_gettime, ClockId};

/// Port of `os_hrtime()` from `Src/os/time.c`.
///
/// "Gets a high-resolution (nanosecond), monotonically-increasing time
/// relative to an arbitrary time in the past." C delegates to `uv_hrtime()`,
/// which reads `CLOCK_MONOTONIC` (Linux) / mach time (macOS); `clock_gettime`
/// is the portable equivalent. Only differences are observable, so the
/// arbitrary epoch is immaterial.
pub fn os_hrtime() -> u64 {
    // c: return uv_hrtime();
    match clock_gettime(ClockId::CLOCK_MONOTONIC) {
        Ok(ts) => (ts.tv_sec() as u64) * 1_000_000_000u64 + (ts.tv_nsec() as u64),
        Err(_) => 0,
    }
}

/// Port of `os_localtime_r()` from `Src/os/time.c:108`.
///
/// Thread-safe broken-down local time. C threads the result through an out-param
/// `struct tm *` (here returned as `Option`, `None` on failure). C also calls
/// `tzset()` (cached) so a changed `$TZ` is honored; POSIX `localtime_r` already
/// does so internally, and the cache is only an optimization.
pub fn os_localtime_r(clock: &libc::time_t) -> Option<libc::tm> {
    // c: return localtime_r(clock, result);
    unsafe {
        let mut result: libc::tm = std::mem::zeroed();
        if libc::localtime_r(clock, &mut result).is_null() {
            None
        } else {
            Some(result)
        }
    }
}

/// Port of `os_strptime()` from `Src/os/time.c:199`.
///
/// Parse `str` per `format` into `tm` (`strptime`); returns whether it parsed
/// (C returns the pointer past the last consumed char, or NULL — the caller only
/// tests for NULL). A NUL byte in either input makes the C string invalid, so it
/// is treated as a parse failure.
pub fn os_strptime(str: &str, format: &str, tm: &mut libc::tm) -> bool {
    // c: #ifdef HAVE_STRPTIME return strptime(str, format, tm); #else NULL
    let (Ok(s), Ok(f)) = (
        std::ffi::CString::new(str),
        std::ffi::CString::new(format),
    ) else {
        return false;
    };
    unsafe { !libc::strptime(s.as_ptr(), f.as_ptr(), tm).is_null() }
}
