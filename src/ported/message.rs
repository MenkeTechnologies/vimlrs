//! Port of the `emsg()` / `did_emsg` error path from `src/nvim/message.c`.
//!
//! `message.c` is not vendored under `csrc/` (only the eval tree is), so these
//! are the extern dependencies the eval ports call, ported against their home
//! file (PORT.md Rule 9 — extern deps get local impls citing the home file).
//! The eval functions signal failure by calling `emsg()`, which sets the global
//! `did_emsg` and writes to the message area; callers checkpoint `did_emsg`
//! before an operation and compare afterward.
#![allow(non_upper_case_globals)]

use std::cell::Cell;

thread_local! {
    /// C global `int did_emsg` (`message.c`): set/bumped on each error message.
    /// Read with `did_emsg.with(|d| d.get())`.
    ///
    /// RUST-PORT NOTE: a per-thread `Cell` counter stands in for the C global,
    /// since the eval engine runs single-threaded per evaluation.
    pub static did_emsg: Cell<u64> = const { Cell::new(0) };
}

/// Port of `emsg(const char *s)` from `Src/nvim/message.c`.
///
/// Record an error message and set `did_emsg`. C writes to the message area;
/// the faithful sink here is stderr (Vim prints errors to the user).
pub fn emsg(s: &str) {
    eprintln!("{s}");
    did_emsg.with(|d| d.set(d.get() + 1));
}

/// Port of `semsg(const char *fmt, ...)` from `Src/nvim/message.c`.
///
/// The variadic C form is reduced to a pre-formatted message (callers format
/// with Rust's `format!` at the call site, then pass the result).
pub fn semsg(s: &str) {
    emsg(s);
}
