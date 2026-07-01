//! Port of `src/nvim/ex_eval.c` (vendored at `csrc/ex_eval.c`).
//!
//! The `:try`/`:catch`/`:throw` abort-state machinery. The bytecode bridge
//! drives the actual try/catch control flow (`PENDING_EXC`, `b_throw`,
//! `b_catch_match`); this module ports the global abort/exception *state*
//! predicates (`aborting()`, `should_abort()`, …) that `ex_eval.c` exposes.
//! Their inputs are the C globals `force_abort`/`got_int`/`did_throw`/
//! `trylevel`/`emsg_silent`, modelled here as thread-local state (all start
//! cleared, as at interpreter startup).
#![allow(non_upper_case_globals)]

use std::cell::Cell;

use crate::ported::eval_h::FAIL;
use crate::ported::message::did_emsg;

thread_local! {
    /// C global `int force_abort` (`ex_eval.c`) — set while an error is being
    /// converted to an exception, to force `aborting()` true until the throw.
    pub static force_abort: Cell<bool> = const { Cell::new(false) };
    /// C global `int got_int` — the interrupt flag (no interactive interrupt
    /// standalone).
    pub static got_int: Cell<bool> = const { Cell::new(false) };
    /// C global `int did_throw` — an exception is being thrown and not yet caught.
    pub static did_throw: Cell<bool> = const { Cell::new(false) };
    /// C global `int trylevel` — number of active `:try` blocks.
    pub static trylevel: Cell<i32> = const { Cell::new(0) };
    /// C global `int emsg_silent` — error messages suppressed (`:silent!`).
    pub static emsg_silent: Cell<i32> = const { Cell::new(0) };
}

/// `except_type_T` (`Src/ex_eval_defs.h`) — the kind of a pending exception.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum except_type_T {
    /// A user `:throw` — the value is the thrown string.
    ET_USER,
    /// An internal error raised as an exception — prefixed with `Vim:`.
    ET_ERROR,
    /// An interrupt (`CTRL-C`) raised as an exception.
    ET_INTERRUPT,
}

/// Port of `get_exception_string()` from `Src/ex_eval.c:384`.
///
/// Build the `v:exception` string for a raised exception: a user/interrupt
/// exception passes its `value` through unchanged; an error exception is
/// prefixed with `Vim(cmdname):` (or `Vim:` when no command). RUST-PORT NOTE:
/// the C also moves a leading `"filename" ` from an error message into a
/// trailing ` (filename)`; that reordering is not modeled.
pub fn get_exception_string(value: &str, type_: except_type_T, cmdname: Option<&str>) -> String {
    match type_ {
        except_type_T::ET_ERROR => match cmdname.filter(|c| !c.is_empty()) {
            Some(cmd) => format!("Vim({cmd}):{value}"),
            None => format!("Vim:{value}"),
        },
        except_type_T::ET_USER | except_type_T::ET_INTERRUPT => value.to_string(),
    }
}

/// Port of `aborting()` from `Src/ex_eval.c` — true when execution should be
/// aborted: a forced-abort error, an interrupt, or an active throw.
pub fn aborting() -> bool {
    (did_emsg.with(|d| d.get()) != 0 && force_abort.with(|f| f.get()))
        || got_int.with(|g| g.get())
        || did_throw.with(|t| t.get())
}

/// Port of `should_abort()` from `Src/ex_eval.c` — whether `retcode` should
/// abort: a FAIL inside an active (non-silent) `:try`, or [`aborting`].
pub fn should_abort(retcode: i32) -> bool {
    (retcode == FAIL && trylevel.with(|t| t.get()) != 0 && emsg_silent.with(|e| e.get()) == 0)
        || aborting()
}

/// Port of `aborted_in_try()` from `Src/ex_eval.c` — called after an error to
/// see whether it forced an abort.
pub fn aborted_in_try() -> bool {
    force_abort.with(|f| f.get())
}

/// Port of `update_force_abort()` from `Src/ex_eval.c` — once an error message
/// has been given inside a `:try`, force the abort to persist to the throw point.
pub fn update_force_abort() {
    if did_emsg.with(|d| d.get()) != 0 {
        force_abort.with(|f| f.set(true));
    }
}

/// Port of `cause_errthrow()` from `Src/ex_eval.c` — decide whether an error
/// message becomes an exception. Outside a `:try` (the standalone default), it
/// does not → false.
pub fn cause_errthrow(_mesg: &str, _multiline: bool, _concat: bool, _severe: bool) -> bool {
    if trylevel.with(|t| t.get()) == 0 {
        return false;
    }
    false
}

/// Port of `discard_current_exception()` from `Src/ex_eval.c` — drop the
/// exception being handled and clear the throw flag.
pub fn discard_current_exception() {
    did_throw.with(|t| t.set(false));
    force_abort.with(|f| f.set(false));
}

/// Port of `exception_state_clear()` from `Src/ex_eval.c` — reset the abort /
/// exception flags.
pub fn exception_state_clear() {
    force_abort.with(|f| f.set(false));
    did_throw.with(|t| t.set(false));
    got_int.with(|g| g.set(false));
}

/// Port of `discard_pending_return()` from `Src/ex_eval.c` — free a pending
/// `:return` value; the value layer is `Rc`-managed, so this is a no-op.
pub fn discard_pending_return() {}

/// Port of `report_make_pending()` from `Src/ex_eval.c` — `'verbose'` reporting
/// of a pending `:return`/`:break`/etc.; no message output standalone (no-op).
pub fn report_make_pending() {}

/// Port of `report_pending()` from `Src/ex_eval.c` — `'verbose'` reporting; no-op.
pub fn report_pending() {}

/// Port of `report_resume_pending()` from `Src/ex_eval.c` — `'verbose'`
/// reporting on resume; no-op.
pub fn report_resume_pending() {}

/// Port of `report_discard_pending()` from `Src/ex_eval.c` — `'verbose'`
/// reporting on discard; no-op.
pub fn report_discard_pending() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exception_string_formatting() {
        use super::except_type_T::*;
        // user throw passes through
        assert_eq!(get_exception_string("boom", ET_USER, None), "boom");
        assert_eq!(get_exception_string("boom", ET_USER, Some("catch")), "boom");
        // error exception gets a Vim[(cmd)]: prefix
        assert_eq!(
            get_exception_string("E492: msg", ET_ERROR, None),
            "Vim:E492: msg"
        );
        assert_eq!(
            get_exception_string("E492: msg", ET_ERROR, Some("echo")),
            "Vim(echo):E492: msg"
        );
    }

    #[test]
    fn abort_state_defaults_and_toggles() {
        exception_state_clear();
        assert!(!aborting());
        assert!(!aborted_in_try());
        // FAIL outside a :try does not abort; inside one it does.
        assert!(!should_abort(FAIL));
        trylevel.with(|t| t.set(1));
        assert!(should_abort(FAIL));
        trylevel.with(|t| t.set(0));
        // A throw makes aborting() true until discarded.
        did_throw.with(|t| t.set(true));
        assert!(aborting());
        discard_current_exception();
        assert!(!aborting());
        assert!(!cause_errthrow("E1", false, false, false));
        assert!(has_loop_cmd("  :while x"));
        assert!(has_loop_cmd("for i in x"));
        assert!(!has_loop_cmd("echo 1"));
        let snap = exception_state_save();
        got_int.with(|g| g.set(true));
        exception_state_restore(snap);
        assert!(!got_int.with(|g| g.get()));
    }
}

/// Snapshot of the abort/exception flags, for [`exception_state_save`] /
/// [`exception_state_restore`] (the C `exception_state_T`).
#[derive(Clone, Copy, Default)]
pub struct ExceptionState {
    force_abort: bool,
    did_throw: bool,
    got_int: bool,
}

/// Port of `exception_state_save()` from `Src/ex_eval.c` — snapshot the current
/// abort/exception flags.
pub fn exception_state_save() -> ExceptionState {
    ExceptionState {
        force_abort: force_abort.with(|f| f.get()),
        did_throw: did_throw.with(|t| t.get()),
        got_int: got_int.with(|g| g.get()),
    }
}

/// Port of `exception_state_restore()` from `Src/ex_eval.c` — restore a snapshot.
pub fn exception_state_restore(s: ExceptionState) {
    force_abort.with(|f| f.set(s.force_abort));
    did_throw.with(|t| t.set(s.did_throw));
    got_int.with(|g| g.set(s.got_int));
}

/// Port of `free_msglist()` from `Src/ex_eval.c` — free an error-message list;
/// `Rc`/`Drop`-managed, no-op.
pub fn free_msglist() {}

/// Port of `free_global_msglist()` from `Src/ex_eval.c` — free the global
/// deferred-error list; no-op.
pub fn free_global_msglist() {}

/// Port of `enter_cleanup()` from `Src/ex_eval.c` — begin a `:finally` cleanup
/// region (the bridge drives `:finally`); state tracking is a no-op here.
pub fn enter_cleanup() {}

/// Port of `leave_cleanup()` from `Src/ex_eval.c` — end a cleanup region; no-op.
pub fn leave_cleanup() {}

/// Port of `has_loop_cmd()` from `Src/ex_eval.c` — true when the command at `p`
/// (after leading whitespace and `:`) is `:while` or `:for`.
pub fn has_loop_cmd(p: &str) -> bool {
    let t = p.trim_start_matches([' ', '\t', ':']);
    t.starts_with("wh") || t.starts_with("for")
}

// ── exception-machinery helpers (the bridge drives real :try/:catch; these are
// the faithful standalone forms of ex_eval.c's state mutators) ──

/// Port of `do_intthrow()` from `Src/ex_eval.c` — throw an interrupt exception
/// if interrupted. No interactive interrupt standalone → nothing thrown (false).
pub fn do_intthrow() -> bool {
    false
}

/// Port of `do_errthrow()` from `Src/ex_eval.c` — convert a pending error into
/// an exception at a `:try`. The bridge converts errors at its catch sites, so
/// this entry point is a no-op.
pub fn do_errthrow() {}

/// Port of `throw_exception()` from `Src/ex_eval.c` — raise an exception value;
/// the bridge's `b_throw` performs the real raise, so no-op here.
pub fn throw_exception() {}

/// Port of `discard_exception()` from `Src/ex_eval.c` — drop a no-longer-needed
/// exception; `Rc`/`Drop`-managed, no-op.
pub fn discard_exception() {}

/// Port of `catch_exception()` from `Src/ex_eval.c` — mark an exception caught;
/// the bridge's `b_catch_match` clears the pending state, so no-op here.
pub fn catch_exception() {}

/// Port of `finish_exception()` from `Src/ex_eval.c` — finish handling an
/// exception; no-op (bridge-driven).
pub fn finish_exception() {}

/// Port of `do_throw()` from `Src/ex_eval.c` — execute `:throw` on the condition
/// stack; the bridge's `b_throw` does this, no-op.
pub fn do_throw() {}

/// Port of `cleanup_conditionals()` from `Src/ex_eval.c` — unwind the condition
/// stack at a `:finally`/error; the bridge owns the stack → nothing to clean (0).
pub fn cleanup_conditionals() -> i32 {
    0
}

/// Port of `rewind_conditionals()` from `Src/ex_eval.c` — rewind the condition
/// stack on a loop `:continue`; bridge-driven, no-op.
pub fn rewind_conditionals() {}

/// Port of `get_end_emsg()` from `Src/ex_eval.c` — the "missing `:endwhile`/
/// `:endif`" message for an unbalanced block; the bridge reports these at
/// compile time, so none is produced here → "".
pub fn get_end_emsg() -> String {
    String::new()
}
