//! Port of `src/nvim/os/time.c` (not vendored under `csrc/`; the names appear as
//! calls in the vendored eval tree, so the drift gate recognizes them).
//!
//! Only the monotonic clock used by `profile.c`/`reltime()` is ported here.
#![allow(non_snake_case)]

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
