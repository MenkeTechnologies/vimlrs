//! Port of `src/nvim/profile.c` (not vendored under `csrc/`; the names appear as
//! calls in the vendored eval tree, so the drift gate recognizes them).
//!
//! Profiling time helpers backing `reltime()`/`reltimestr()`/`reltimefloat()`.
//! `proftime_T` is `uint64_t` nanoseconds (`Src/types_defs.h:44`).
#![allow(non_snake_case, non_camel_case_types)]

use crate::ported::os::time::os_hrtime;

/// Port of `proftime_T` from `Src/types_defs.h:44` (`typedef uint64_t proftime_T;`).
pub type proftime_T = u64;

/// Port of `profile_start()` from `Src/profile.c:53`.
///
/// "Gets the current time." Returns the monotonic clock value.
pub fn profile_start() -> proftime_T {
    // c: return os_hrtime();
    os_hrtime()
}

/// Port of `profile_end()` from `Src/profile.c:61`.
///
/// "Computes the time elapsed." Difference between now and `tm`.
pub fn profile_end(tm: proftime_T) -> proftime_T {
    // c: return profile_sub(os_hrtime(), tm);
    profile_sub(os_hrtime(), tm)
}

/// Port of `profile_sub()` from `Src/profile.c:147`.
///
/// "Subtracts `tm2` from `tm1`." Unsigned wraparound is intentional (see
/// `profile_signed`).
pub fn profile_sub(tm1: proftime_T, tm2: proftime_T) -> proftime_T {
    // c: return tm1 - tm2;
    tm1.wrapping_sub(tm2)
}

/// Port of `profile_signed()` from `Src/profile.c:203`.
///
/// Returns the signed difference after unsigned wraparound. `(tm > INT64_MAX)`
/// is >=150 years, so it must have come from differencing two `proftime_T`
/// values; recover the negative magnitude (#10452).
pub fn profile_signed(tm: proftime_T) -> i64 {
    // c: return (tm <= INT64_MAX) ? (int64_t)tm : -(int64_t)(UINT64_MAX - tm);
    if tm <= i64::MAX as u64 {
        tm as i64
    } else {
        -((u64::MAX - tm) as i64)
    }
}

/// Port of `profile_msg()` from `Src/profile.c:72`.
///
/// Formats `tm` as a string of the seconds, `%10.6lf`.
pub fn profile_msg(tm: proftime_T) -> String {
    // c: snprintf(buf, sizeof(buf), "%10.6lf", (double)profile_signed(tm) / 1e9);
    format!("{:10.6}", profile_signed(tm) as f64 / 1_000_000_000.0)
}
