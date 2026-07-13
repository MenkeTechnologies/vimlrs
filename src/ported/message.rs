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

    /// Every error raised, including the ones `:silent!` suppresses and the ones
    /// `assert_fails()` captures — unlike `did_emsg`, which those two paths skip
    /// and `:catch` resets.
    ///
    /// RUST-PORT NOTE: the C propagates "this command failed" as a `FAIL` return
    /// value up the call chain, which unwinds the command. This VM has no such
    /// unwind, so a statement asks "did an error happen while I evaluated my
    /// arguments?" by comparing this counter against a mark taken at its start
    /// (`VIML_ERR_MARK`). `did_emsg` cannot answer that: `:silent!` deliberately
    /// leaves it alone, and an erroring `silent! echo` would then print the
    /// recovered value that Vim never prints.
    pub static err_count: Cell<u64> = const { Cell::new(0) };

    /// C global `int msg_silent` (`globals.h`) — while non-zero, ordinary message
    /// output is suppressed. `:silent` raises it, which is why `silent echo 'x'`
    /// prints nothing. (`emsg_silent`, in `ex_eval`, is the error-message
    /// counterpart that `:silent!` raises.)
    pub static msg_silent: Cell<i32> = const { Cell::new(0) };

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
    // Counted first: this one tracks *every* error, whatever happens to it next.
    err_count.with(|d| d.set(d.get() + 1));
    let captured = ERROR_CAPTURE.with(|c| {
        if let Some(list) = c.borrow_mut().as_mut() {
            list.push(s.to_string());
            true
        } else {
            false
        }
    });
    if captured {
        did_emsg.with(|d| d.set(d.get() + 1));
        return;
    }
    // c: `emsg_silent` (raised by `:silent!`) returns before the message is shown
    // *or* counted. The command still fails, but the error neither prints nor marks
    // the script as having errored — which is why `silent! call Foo()` on a missing
    // function leaves a sourced script exiting 0.
    if crate::ported::ex_eval::emsg_silent.with(|e| e.get()) != 0 {
        return;
    }
    did_emsg.with(|d| d.set(d.get() + 1));
    // c: `emsg_multiline` → `cause_errthrow()` → the message becomes a *catchable
    // exception* when a `:try` is active, instead of being printed:
    //
    //   try | echo [1] . 'x' | catch | echo v:exception | endtry
    //   → "Vim(echo):E730: Using a List as a String"
    //
    // The pending-exception slot lives in the VM bridge (the synthesis zone), so the
    // throw itself does: `errthrow` returns true when it took ownership of the
    // message, in which case nothing is printed here — the exception carries it.
    // Outside a `:try` it declines and the error prints as before.
    if !crate::fusevm_bridge::errthrow(s) {
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
