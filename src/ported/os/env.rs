//! Port of `src/nvim/os/env.c` (not vendored under `vendor/`; the names appear as
//! calls in the vendored eval tree, so the drift gate recognizes them).
//!
//! Only the process-id query used by `init_srand()` is ported here.
#![allow(non_snake_case)]

/// Port of `os_get_pid()` from `Src/os/env.c:309`.
///
/// "Get the process ID of the Neovim process." (Here: the host process.)
pub fn os_get_pid() -> i64 {
    // c: return (int64_t)getpid();
    std::process::id() as i64
}
