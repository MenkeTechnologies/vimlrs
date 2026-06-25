//! Error reporting sink — port of Neovim's `emsg()`/`semsg()` message path
//! (`src/nvim/message.c`), reduced to what the eval engine needs.
//!
//! Vim/Neovim report Vimscript errors by calling `emsg()`, which sets the
//! global `did_emsg` and prints to the message area. The eval functions branch
//! on whether an error was raised (`did_emsg_before == did_emsg`). This module
//! provides the same observable contract: a process-local error log plus a
//! monotonically increasing `did_emsg` counter the evaluator can checkpoint.

use std::cell::RefCell;

thread_local! {
    static ERRORS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static DID_EMSG: RefCell<u64> = const { RefCell::new(0) };
}

/// Port of `emsg()` — record an error message and bump `did_emsg`.
pub fn emsg(msg: &str) {
    ERRORS.with(|e| e.borrow_mut().push(msg.to_string()));
    DID_EMSG.with(|d| *d.borrow_mut() += 1);
}

/// Current `did_emsg` counter. Callers checkpoint this before an operation and
/// compare afterward to detect whether an error was raised (vim's idiom).
pub fn did_emsg() -> u64 {
    DID_EMSG.with(|d| *d.borrow())
}

/// Drain and return all errors recorded so far on this thread.
pub fn take_errors() -> Vec<String> {
    ERRORS.with(|e| std::mem::take(&mut *e.borrow_mut()))
}

/// Clear the error log and reset `did_emsg` (test/REPL boundary).
pub fn reset() {
    ERRORS.with(|e| e.borrow_mut().clear());
    DID_EMSG.with(|d| *d.borrow_mut() = 0);
}
