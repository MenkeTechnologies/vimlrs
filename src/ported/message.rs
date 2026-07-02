//! Port of the `emsg()` / `did_emsg` error path from `src/nvim/message.c`.
//!
//! `message.c` is not vendored under `vendor/` (only the eval tree is), so these
//! are the extern dependencies the eval ports call, ported against their home
//! file (PORT.md Rule 9 — extern deps get local impls citing the home file).
//! The eval functions signal failure by calling `emsg()`, which sets the global
//! `did_emsg` and writes to the message area; callers checkpoint `did_emsg`
//! before an operation and compare afterward.
#![allow(non_upper_case_globals)]

use std::cell::{Cell, RefCell};

thread_local! {
    /// C global `int did_emsg` (`message.c`): set/bumped on each error message.
    /// Read with `did_emsg.with(|d| d.get())`.
    ///
    /// RUST-PORT NOTE: a per-thread `Cell` counter stands in for the C global,
    /// since the eval engine runs single-threaded per evaluation.
    pub static did_emsg: Cell<u64> = const { Cell::new(0) };

    /// When `Some`, each `emsg` text is captured here instead of printed —
    /// modelling `emsg_silent` + the saved error list that `assert_fails()`
    /// inspects in `message.c`/`testing.c`. `None` is the normal (print) path.
    static ERROR_CAPTURE: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

/// Begin capturing `emsg` text (suppressing stderr output), as `assert_fails()`
/// does while running the command under test.
pub fn capture_errors_begin() {
    ERROR_CAPTURE.with(|c| *c.borrow_mut() = Some(Vec::new()));
}

/// Stop capturing and return the messages collected since `capture_errors_begin`.
pub fn capture_errors_take() -> Vec<String> {
    ERROR_CAPTURE.with(|c| c.borrow_mut().take().unwrap_or_default())
}

/// Port of `emsg(const char *s)` from `Src/nvim/message.c`.
///
/// Record an error message and set `did_emsg`. C writes to the message area;
/// the faithful sink here is stderr (Vim prints errors to the user) — unless a
/// capture is active (`assert_fails`), in which case the text is collected and
/// not printed, like Vim's `emsg_silent` path.
pub fn emsg(s: &str) {
    did_emsg.with(|d| d.set(d.get() + 1));
    let captured = ERROR_CAPTURE.with(|c| {
        if let Some(list) = c.borrow_mut().as_mut() {
            list.push(s.to_string());
            true
        } else {
            false
        }
    });
    if !captured {
        eprintln!("{s}");
    }
}

/// Port of `semsg(const char *fmt, ...)` from `Src/nvim/message.c`.
///
/// The variadic C form is reduced to a pre-formatted message (callers format
/// with Rust's `format!` at the call site, then pass the result).
pub fn semsg(s: &str) {
    emsg(s);
}
