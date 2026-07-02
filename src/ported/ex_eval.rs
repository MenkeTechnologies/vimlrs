//! Port of `src/nvim/ex_eval.c` (vendored at `vendor/ex_eval.c`).
//!
//! The `:if`/`:while`/`:for`/`:try`/`:catch`/`:finally`/`:throw` conditional and
//! exception command drivers, plus the `cstack_T` condition/exception stack they
//! drive. These `exarg_T` command drivers are SUPERSEDED at runtime by the
//! bytecode frontend (`viml_parser.rs`/`compile_viml.rs`); they are ported here
//! as strict 1:1 REFERENCE ports (dead code allowed), exactly as `eval0`…`eval7`
//! were in wave 1. Sub-expression strings (the C `eval1`/`eval_to_bool` calls)
//! evaluate through the `EVAL_STRING_HOOK` bridge (see `eval.rs::eval_to_bool`).
//!
//! The abort/exception *state* predicates (`aborting()`, `should_abort()`, …)
//! that `ex_eval.c` exposes are also ported here; their inputs are the C globals
//! `force_abort`/`got_int`/`did_throw`/`trylevel`/`emsg_silent`, modelled as
//! thread-local state (all start cleared, as at interpreter startup).
//!
//! RUST-PORT NOTE: intrusive `except_T *` pointers become `Rc<RefCell<except_T>>`
//! (sanctioned by PORT.md); the `cs_pend` union of `csp_rv`/`csp_ex` is modelled
//! as two parallel arrays (only one member per index is live, selected by
//! `cs_pending`, so no aliasing is lost); the `'verbose'`/debugger message
//! rendering (`smsg`/`IObuff`/`report_pending`) is gated off standalone
//! (`p_verbose == 0`, `debug_break_level == 0`) exactly as in C.
#![allow(
    non_upper_case_globals,
    non_camel_case_types,
    dead_code,
    non_snake_case
)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::ported::eval::typval_defs_h::typval_T;
use crate::ported::eval_h::{FAIL, OK};
use crate::ported::message::{did_emsg, emsg, semsg};

/// `linenr_T` (`Src/pos_defs.h`) — a 1-based buffer line number.
type linenr_T = i32;

// ── error messages (Src/errors.h — not vendored) ───────────────────────────
const e_argreq: &str = "E471: Argument required"; // c: errors.h:15
const e_endif: &str = "E171: Missing :endif"; // c: errors.h:22
const e_endtry: &str = "E600: Missing :endtry"; // c: errors.h:23
const e_endwhile: &str = "E170: Missing :endwhile"; // c: errors.h:24
const e_endfor: &str = "E170: Missing :endfor"; // c: errors.h:25
const e_while: &str = "E588: :endwhile without :while"; // c: errors.h:26
const e_for: &str = "E588: :endfor without :for"; // c: errors.h:27
const e_invarg2: &str = "E475: Invalid argument: %s"; // c: errors.h:33
const e_invexpr2: &str = "E15: Invalid expression: \"%s\""; // c: errors.h:37
const e_trailing_arg: &str = "E488: Trailing characters: %s"; // c: errors.h:123
const e_str_not_inside_function: &str = "E193: %s not inside a function"; // c: errors.h:140
const e_multiple_else: &str = "E583: Multiple :else"; // c:39
const e_multiple_finally: &str = "E607: Multiple :finally"; // c:40

// ── cstack constants (Src/ex_eval_defs.h) ───────────────────────────────────
/// c: ex_eval_defs.h:20 — nesting limit for conditional commands.
pub const CSTACK_LEN: usize = 50;

// CSF_ flags — cs_flags[] bits. c: ex_eval_defs.h:43
pub const CSF_TRUE: i32 = 0x0001; // condition was TRUE
pub const CSF_ACTIVE: i32 = 0x0002; // current state is active
pub const CSF_ELSE: i32 = 0x0004; // ":else" has been passed
pub const CSF_WHILE: i32 = 0x0008; // is a ":while"
pub const CSF_FOR: i32 = 0x0010; // is a ":for"
pub const CSF_TRY: i32 = 0x0100; // is a ":try"
pub const CSF_FINALLY: i32 = 0x0200; // ":finally" has been passed
pub const CSF_THROWN: i32 = 0x0800; // exception thrown to this try conditional
pub const CSF_CAUGHT: i32 = 0x1000; // exception caught by this try conditional
pub const CSF_FINISHED: i32 = 0x2000; // CSF_CAUGHT handled by finish_exception()
pub const CSF_SILENT: i32 = 0x4000; // "emsg_silent" reset by ":try"

// CSTP_ — what's pending for reactivation at ":endtry". c: ex_eval_defs.h:62
pub const CSTP_NONE: i32 = 0;
pub const CSTP_ERROR: i32 = 1;
pub const CSTP_INTERRUPT: i32 = 2;
pub const CSTP_THROW: i32 = 4;
pub const CSTP_BREAK: i32 = 8;
pub const CSTP_CONTINUE: i32 = 16;
pub const CSTP_RETURN: i32 = 24;
pub const CSTP_FINISH: i32 = 32;

// CSL_ — cs_lflags loop flags. c: ex_eval_defs.h:74
pub const CSL_HAD_LOOP: i32 = 1;
pub const CSL_HAD_ENDLOOP: i32 = 2;
pub const CSL_HAD_CONT: i32 = 4;
pub const CSL_HAD_FINA: i32 = 8;

// Flags specifying the message displayed by report_pending. c:711
const RP_MAKE: i32 = 0;
const RP_RESUME: i32 = 1;
const RP_DISCARD: i32 = 2;

// Values used for the Vim release. c:73
const THROW_ON_ERROR: bool = true;
const THROW_ON_INTERRUPT: bool = true;

// vim_regcomp() flags (Src/regexp_defs.h — not vendored).
const RE_MAGIC: i32 = 1;
const RE_STRING: i32 = 2;

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
    /// C global `bool did_endif` (`ex_eval.c`) — set by `ex_endif`.
    pub static did_endif: Cell<bool> = const { Cell::new(false) };
    /// C static `bool cause_abort` (`ex_eval.c:99`).
    static cause_abort: Cell<bool> = const { Cell::new(false) };
    /// C global `bool need_rethrow`.
    pub static need_rethrow: Cell<bool> = const { Cell::new(false) };
    /// C global `bool suppress_errthrow`.
    pub static suppress_errthrow: Cell<bool> = const { Cell::new(false) };
    /// C global `long p_verbose` — the `'verbose'` option (0 standalone).
    static p_verbose: Cell<i64> = const { Cell::new(0) };
    /// C global `int debug_break_level` — active debugger level (0 standalone).
    static debug_break_level: Cell<i32> = const { Cell::new(0) };
    /// C global `int emsg_off` — error-message suppression counter.
    static emsg_off: Cell<i32> = const { Cell::new(0) };
    /// C global `except_T *current_exception` — the exception being thrown.
    pub static current_exception: RefCell<Option<Rc<RefCell<except_T>>>> =
        const { RefCell::new(None) };
    /// C global `except_T *caught_stack` — stack of caught exceptions.
    static caught_stack: RefCell<Option<Rc<RefCell<except_T>>>> = const { RefCell::new(None) };
}

/// c: `SOURCING_LNUM` — current sourcing line (0 standalone).
const SOURCING_LNUM: linenr_T = 0;

/// `cmdidx_T` (`Src/ex_cmds_defs.h`) — the command index. RUST-PORT NOTE: only
/// the variants read by the conditional/exception drivers are modelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum cmdidx_T {
    CMD_if,
    CMD_elseif,
    CMD_else,
    CMD_endif,
    CMD_while,
    CMD_for,
    CMD_continue,
    CMD_break,
    CMD_endwhile,
    CMD_endfor,
    CMD_try,
    CMD_catch,
    CMD_finally,
    CMD_endtry,
    CMD_throw,
    CMD_eval,
    CMD_endfunction,
}

/// `eslist_T` (`Src/ex_eval_defs.h:11`) — saved `emsg_silent` values for `:try`.
pub struct eslist_T {
    pub saved_emsg_silent: i32,
    pub next: Option<Box<eslist_T>>,
}

/// `msglist_T` (`Src/ex_eval_defs.h:85`) — error messages convertible to an
/// exception. RUST-PORT NOTE: allocation/refcount are `Box`/`Drop`-managed.
pub struct msglist_T {
    pub next: Option<Box<msglist_T>>,
    pub msg: String,
    pub throw_msg: String,
    pub sfile: String,
    pub slnum: linenr_T,
    pub multiline: bool,
}

/// `except_type_T` (`Src/ex_eval_defs.h:96`) — the kind of a pending exception.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum except_type_T {
    /// A user `:throw` — the value is the thrown string.
    ET_USER,
    /// An internal error raised as an exception — prefixed with `Vim:`.
    ET_ERROR,
    /// An interrupt (`CTRL-C`) raised as an exception.
    ET_INTERRUPT,
}

/// `except_T` / `struct vim_exception` (`Src/ex_eval_defs.h:104`).
/// RUST-PORT NOTE: `type` → `type_` (Rust keyword); `stacktrace` (`list_T*`) is
/// omitted (the standalone value layer has no stacktrace list).
pub struct except_T {
    pub type_: except_type_T,
    pub value: String,
    pub messages: Option<Box<msglist_T>>,
    pub throw_name: String,
    pub throw_lnum: linenr_T,
    pub caught: Option<Rc<RefCell<except_T>>>,
}

/// `cleanup_T` / `struct cleanup_stuff` (`Src/ex_eval_defs.h:118`).
pub struct cleanup_T {
    pub pending: i32,
    pub exception: Option<Rc<RefCell<except_T>>>,
}

/// The `cs_pend` union of `cstack_T` (`Src/ex_eval_defs.h:26`) — `csp_rv` (a
/// pending `:return` value) aliases `csp_ex` (a pending exception). RUST-PORT
/// NOTE: modelled as two parallel arrays; only one member per index is live.
pub struct cs_pend_T {
    /// `cs_rettv` — return typval for a pending `:return`.
    pub csp_rv: [Option<typval_T>; CSTACK_LEN],
    /// `cs_exception` — exception for a pending throw.
    pub csp_ex: [Option<Rc<RefCell<except_T>>>; CSTACK_LEN],
}

/// `cstack_T` (`Src/ex_eval_defs.h:23`) — the nested-conditional stack.
/// `cs_idx < 0` means no conditional command is active.
pub struct cstack_T {
    pub cs_flags: [i32; CSTACK_LEN],
    pub cs_pending: [i8; CSTACK_LEN],
    pub cs_pend: cs_pend_T,
    /// `cs_forinfo` — `:for` iterator info (`void*`). RUST-PORT NOTE: the
    /// `forinfo_T` builder (`eval_for_line`) is deferred, so this only tracks
    /// presence.
    pub cs_forinfo: [Option<()>; CSTACK_LEN],
    pub cs_line: [i32; CSTACK_LEN],
    pub cs_idx: i32,
    pub cs_looplevel: i32,
    pub cs_trylevel: i32,
    pub cs_emsg_silent_list: Option<Box<eslist_T>>,
    pub cs_lflags: i32,
}

impl Default for cstack_T {
    /// A fresh, empty condition stack (`cs_idx = -1`, as `do_cmdline()` sets).
    fn default() -> Self {
        cstack_T {
            cs_flags: [0; CSTACK_LEN],
            cs_pending: [0; CSTACK_LEN],
            cs_pend: cs_pend_T {
                csp_rv: [const { None }; CSTACK_LEN],
                csp_ex: [const { None }; CSTACK_LEN],
            },
            cs_forinfo: [const { None }; CSTACK_LEN],
            cs_line: [0; CSTACK_LEN],
            cs_idx: -1,
            cs_looplevel: 0,
            cs_trylevel: 0,
            cs_emsg_silent_list: None,
            cs_lflags: 0,
        }
    }
}

/// `exarg_T` (`Src/ex_cmds_defs.h`) — the ex-command argument struct. RUST-PORT
/// NOTE: modelled minimally with only the fields the conditional/exception
/// drivers read; `eap->cstack` (a pointer to `do_cmdline`'s stack) is owned
/// inline here.
pub struct exarg_T {
    pub arg: String,
    pub cmdidx: cmdidx_T,
    pub forceit: bool,
    pub skip: bool,
    pub errmsg: Option<&'static str>,
    pub nextcmd: Option<String>,
    pub cstack: cstack_T,
}

/// `exception_state_T` / `struct exception_state_S` (`Src/ex_eval_defs.h:126`).
#[derive(Clone, Default)]
pub struct exception_state_T {
    pub estate_current_exception: Option<Rc<RefCell<except_T>>>,
    pub estate_did_throw: bool,
    pub estate_need_rethrow: bool,
    pub estate_trylevel: i32,
    pub estate_did_emsg: u64,
}

/// `regprog_T` (`Src/regexp_defs.h` — not vendored). RUST-PORT NOTE: placeholder
/// for a compiled regex program; `vim_regcomp` is deferred.
pub struct regprog_T;

/// `regmatch_T` (`Src/regexp_defs.h` — not vendored), the fields `ex_catch` uses.
pub struct regmatch_T {
    pub regprog: Option<Box<regprog_T>>,
    pub rm_ic: bool,
}

// ── externs from non-vendored C files (honest stubs / standalone defaults) ───

/// Port of `dbg_check_skipped()` from `Src/debugger.c` (not vendored). RUST-PORT
/// NOTE: no debugger standalone, so no breakpoint is ever skipped → false.
fn dbg_check_skipped(_eap: &exarg_T) -> bool {
    false
}

/// Port of `do_finish()` from `Src/ex_docmd.c` (not vendored). Deferred: the
/// bytecode frontend drives `:finish`.
fn do_finish(_eap: &mut exarg_T, _reanimate: bool) {}

/// Port of `find_nextcmd()` from `Src/ex_docmd.c` (not vendored). Deferred: the
/// command-line scanner is not standalone → no next command.
fn find_nextcmd(_p: &str) -> Option<String> {
    None
}

/// Port of `skip_regexp_err()` from `Src/regexp.c` (not vendored). Deferred.
fn skip_regexp_err(_pat: &str, _delim: u8, _magic: bool) -> Option<String> {
    None
}

/// Port of `vim_regcomp()` from `Src/regexp.c` (not vendored). Deferred.
fn vim_regcomp(_expr: &str, _re_flags: i32) -> Option<Box<regprog_T>> {
    unimplemented!("deferred: vim_regcomp — vendor regexp.c not vendored")
}

/// Port of `vim_regexec_nl()` from `Src/regexp.c` (not vendored). Deferred.
fn vim_regexec_nl(_rmp: &mut regmatch_T, _line: &str, _col: usize) -> bool {
    unimplemented!("deferred: vim_regexec_nl — vendor regexp.c not vendored")
}

/// Port of `vim_regfree()` from `Src/regexp.c` (not vendored). Deferred: `Drop`.
fn vim_regfree(_prog: Option<Box<regprog_T>>) {}

/// Port of `internal_error()` from `Src/message.c` (not vendored). RUST-PORT
/// NOTE: reports an "impossible" internal inconsistency; standalone no-op.
fn internal_error(_where: &str) {}

/// Port of `estack_sfile()` from `Src/runtime.c` (not vendored). Deferred: no
/// exestack standalone → no source-file name.
fn estack_sfile() -> Option<String> {
    None
}

/// Port of `set_vim_var_string()` from `Src/eval/vars.c` (v:exception etc.).
/// Deferred: `vars::set_vim_var_string` exists but its `(VimVarIndex, &str)`
/// signature can't model the C `NULL`-clear (`v:throwpoint = NULL`) calls, so the
/// v: store is not wired to this reference port.
fn set_vim_var_string(_idx: i32, _val: Option<&str>, _len: isize) {}

/// Port of `set_vim_var_list()` from `Src/eval/vars.c`. Deferred (see above).
fn set_vim_var_list() {}

// ── ex_eval.c body ──────────────────────────────────────────────────────────

/// Port of `discard_pending_return()` from `Src/ex_eval.c:87`.
///
/// Free a pending `:return` value. RUST-PORT NOTE: the value layer is
/// `Option<typval_T>`/`Drop`-managed, so the caller drops the slot; no-op here.
fn discard_pending_return(_p: Option<typval_T>) {}

/// Port of `aborting()` from `Src/ex_eval.c:112` — true when execution should be
/// aborted: a forced-abort error, an interrupt, or an active throw.
pub fn aborting() -> bool {
    (did_emsg.with(|d| d.get()) != 0 && force_abort.with(|f| f.get())) // c:114
        || got_int.with(|g| g.get())
        || did_throw.with(|t| t.get())
}

/// Port of `update_force_abort()` from `Src/ex_eval.c:121`.
pub fn update_force_abort() {
    if cause_abort.with(|c| c.get()) {
        force_abort.with(|f| f.set(true)); // c:124
    }
}

/// Port of `should_abort()` from `Src/ex_eval.c:132`.
pub fn should_abort(retcode: i32) -> bool {
    (retcode == FAIL && trylevel.with(|t| t.get()) != 0 && emsg_silent.with(|e| e.get()) == 0)
        || aborting() // c:134
}

/// Port of `aborted_in_try()` from `Src/ex_eval.c:141`.
pub fn aborted_in_try() -> bool {
    force_abort.with(|f| f.get()) // c:146
}

/// Port of `cause_errthrow()` from `Src/ex_eval.c:158` — decide whether an error
/// message becomes an exception. RUST-PORT NOTE: the full `msg_list` accumulation
/// (`throw_msg` prefixing) is deferred; the standalone predicate only reports
/// whether a throw is possible. Outside a `:try` with no active abort/throw and
/// not `:silent!`, it does not → false (matches C's early return at c:189).
pub fn cause_errthrow(_mesg: &str, _multiline: bool, _concat: bool, _severe: bool) -> bool {
    if suppress_errthrow.with(|s| s.get()) {
        return false; // c:167
    }
    if did_emsg.with(|d| d.get()) == 0 {
        cause_abort.with(|c| c.set(force_abort.with(|f| f.get()))); // c:179
        force_abort.with(|f| f.set(false)); // c:180
    }
    if ((trylevel.with(|t| t.get()) == 0 && !cause_abort.with(|c| c.get()))
        || emsg_silent.with(|e| e.get()) != 0)
        && !did_throw.with(|t| t.get())
    {
        return false; // c:190
    }
    // c:205 Ensure nested calls/sourced files are aborted immediately.
    cause_abort.with(|c| c.set(true));
    if did_throw.with(|t| t.get()) {
        // c:214 discard the exception being thrown so it can't be caught.
        discard_current_exception();
    }
    // c:241 msg_list accumulation deferred; report a throw is pending.
    true
}

/// Port of `free_msglist()` from `Src/ex_eval.c:284`. RUST-PORT NOTE: `Box`/
/// `Drop`-managed.
fn free_msglist(_l: Option<Box<msglist_T>>) {}

/// Port of `free_global_msglist()` from `Src/ex_eval.c:298`. RUST-PORT NOTE:
/// the global `msg_list` is not modelled → no-op.
pub fn free_global_msglist() {}

/// Port of `do_errthrow()` from `Src/ex_eval.c:307`.
///
/// RUST-PORT NOTE: the deferred error `msg_list` is not modelled, so there is no
/// accumulated error to convert; this ports the `force_abort` fix-up only.
pub fn do_errthrow(_cstack: Option<&mut cstack_T>, _cmdname: Option<&str>) {
    if cause_abort.with(|c| c.get()) {
        cause_abort.with(|c| c.set(false)); // c:312
        force_abort.with(|f| f.set(true)); // c:313
    }
    // c:318 msg_list == NULL standalone → nothing to throw.
}

/// Port of `do_intthrow()` from `Src/ex_eval.c:339`.
///
/// Replace the current exception by an interrupt exception if appropriate.
/// RUST-PORT NOTE: no interactive interrupt standalone (`got_int` stays clear),
/// so the early return at c:343 always fires → false.
pub fn do_intthrow(cstack: &mut cstack_T) -> bool {
    // c:343 If no interrupt or no active try/throw, do nothing.
    if !got_int.with(|g| g.get())
        || (trylevel.with(|t| t.get()) == 0 && !did_throw.with(|t| t.get()))
    {
        return false;
    }

    // THROW_ON_INTERRUPT is true, so the discard-only branch is compiled out.
    if did_throw.with(|t| t.get()) {
        // c:364 An interrupt exception already being thrown: do nothing.
        if current_exception.with(|c| c.borrow().as_ref().map(|e| e.borrow().type_))
            == Some(except_type_T::ET_INTERRUPT)
        {
            return false;
        }
        // c:369 An interrupt exception replaces any user or error exception.
        discard_current_exception();
    }
    if throw_exception(
        "Vim:Interrupt".to_string(),
        except_type_T::ET_INTERRUPT,
        None,
    ) != FAIL
    {
        do_throw(cstack); // c:372
    }
    true
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
            Some(cmd) => format!("Vim({cmd}):{value}"), // c:394
            None => format!("Vim:{value}"),             // c:399
        },
        except_type_T::ET_USER | except_type_T::ET_INTERRUPT => value.to_string(), // c:435
    }
}

/// Port of `throw_exception()` from `Src/ex_eval.c:447`.
///
/// Throw a new exception. `value` is the exception string for a user or
/// interrupt exception. RUST-PORT NOTE: the `ET_ERROR` `msglist_T*` value path
/// and the `'verbose'`/stacktrace bookkeeping are reduced (see deferred_deps).
///
/// @return  FAIL when it was tried to throw an illegal user exception.
fn throw_exception(value: String, type_: except_type_T, cmdname: Option<&str>) -> i32 {
    // c:452 Disallow faking Interrupt/error exceptions as user exceptions.
    if type_ == except_type_T::ET_USER {
        if value.starts_with("Vim")
            && matches!(value.as_bytes().get(3), None | Some(b':') | Some(b'('))
        {
            emsg("E608: Cannot :throw exceptions with 'Vim' prefix"); // c:456
            current_exception.with(|c| *c.borrow_mut() = None); // fail: c:526
            return FAIL;
        }
    }

    let excp = except_T {
        type_,
        // c:470 get_exception_string() prefixes error exceptions.
        value: get_exception_string(&value, type_, cmdname),
        messages: None,
        // c:482 estack_sfile() — deferred, empty standalone.
        throw_name: estack_sfile().unwrap_or_default(),
        throw_lnum: SOURCING_LNUM,
        caught: None,
    };
    // c:492 verbose "Exception thrown" report gated off (p_verbose < 13).

    current_exception.with(|c| *c.borrow_mut() = Some(Rc::new(RefCell::new(excp)))); // c:518
    OK
}

/// Port of `discard_exception()` from `Src/ex_eval.c:532`.
///
/// Discard an exception. `was_finished` is set when it has been caught and the
/// catch clause ended normally. RUST-PORT NOTE: `value`/`messages`/`throw_name`/
/// `stacktrace` are freed by `Drop`; the `'verbose'` report is gated off.
fn discard_exception(excp: &Rc<RefCell<except_T>>, _was_finished: bool) {
    // c:534 If it is the current exception, clear it.
    let is_current =
        current_exception.with(|c| c.borrow().as_ref().is_some_and(|ce| Rc::ptr_eq(ce, excp)));
    if is_current {
        current_exception.with(|c| *c.borrow_mut() = None);
    }
    // c:542 verbose "Exception discarded/finished" report gated off.
    // c:569.. value/messages/throw_name/stacktrace freed by Drop.
}

/// Port of `discard_current_exception()` from `Src/ex_eval.c:581`.
pub fn discard_current_exception() {
    let cur = current_exception.with(|c| c.borrow().clone());
    if let Some(excp) = cur.as_ref() {
        discard_exception(excp, false); // c:584
    }
    did_throw.with(|t| t.set(false)); // c:588
    need_rethrow.with(|n| n.set(false)); // c:589
}

/// Port of `catch_exception()` from `Src/ex_eval.c:593`.
///
/// Put an exception on the caught stack. RUST-PORT NOTE: `set_vim_var_*`
/// (`v:exception`/`v:throwpoint`/`v:stacktrace`) and the `'verbose'` report are
/// deferred/gated off.
fn catch_exception(excp: &Rc<RefCell<except_T>>) {
    excp.borrow_mut().caught = caught_stack.with(|c| c.borrow().clone()); // c:595
    caught_stack.with(|c| *c.borrow_mut() = Some(excp.clone())); // c:596
    set_vim_var_string(0, Some(excp.borrow().value.as_str()), -1); // c:597 VV_EXCEPTION
    set_vim_var_list(); // c:598 VV_STACKTRACE
                        // c:599 v:throwpoint / verbose report deferred.
}

/// Port of `finish_exception()` from `Src/ex_eval.c:641`.
///
/// Remove an exception from the caught stack.
fn finish_exception(excp: &Rc<RefCell<except_T>>) {
    let is_top = caught_stack.with(|c| c.borrow().as_ref().is_some_and(|t| Rc::ptr_eq(t, excp)));
    if !is_top {
        internal_error("finish_exception()"); // c:644
    }
    let next = caught_stack.with(|c| c.borrow().as_ref().and_then(|t| t.borrow().caught.clone()));
    caught_stack.with(|c| *c.borrow_mut() = next.clone()); // c:646
    if let Some(top) = next.as_ref() {
        set_vim_var_string(0, Some(top.borrow().value.as_str()), -1); // c:648
        set_vim_var_list(); // c:649
    } else {
        set_vim_var_string(0, None, -1); // c:667
        set_vim_var_string(0, None, -1); // c:668
        set_vim_var_list(); // c:669
    }
    // c:673 Discard the exception, using the finish message for 'verbose'.
    discard_exception(excp, true);
}

/// Port of `exception_state_save()` from `Src/ex_eval.c:677`.
pub fn exception_state_save() -> exception_state_T {
    exception_state_T {
        estate_current_exception: current_exception.with(|c| c.borrow().clone()),
        estate_did_throw: did_throw.with(|t| t.get()),
        estate_need_rethrow: need_rethrow.with(|n| n.get()),
        estate_trylevel: trylevel.with(|t| t.get()),
        estate_did_emsg: did_emsg.with(|d| d.get()),
    }
}

/// Port of `exception_state_restore()` from `Src/ex_eval.c:687`.
pub fn exception_state_restore(estate: exception_state_T) {
    // c:690 Handle any outstanding exceptions before restoring the state.
    if did_throw.with(|t| t.get()) {
        handle_did_throw();
    }
    current_exception.with(|c| *c.borrow_mut() = estate.estate_current_exception);
    did_throw.with(|t| t.set(estate.estate_did_throw));
    need_rethrow.with(|n| n.set(estate.estate_need_rethrow));
    trylevel.with(|t| t.set(estate.estate_trylevel));
    did_emsg.with(|d| d.set(estate.estate_did_emsg));
}

/// Port of `exception_state_clear()` from `Src/ex_eval.c:701`.
pub fn exception_state_clear() {
    current_exception.with(|c| *c.borrow_mut() = None);
    did_throw.with(|t| t.set(false));
    need_rethrow.with(|n| n.set(false));
    trylevel.with(|t| t.set(0));
    did_emsg.with(|d| d.set(0));
    // RUST-PORT NOTE: also clear the abort flags used by `aborting()`.
    force_abort.with(|f| f.set(false));
    got_int.with(|g| g.set(false));
}

/// `handle_did_throw()` from `Src/message.c` (not vendored). Deferred: the
/// uncaught-exception reporter is not standalone → drop the throw.
fn handle_did_throw() {
    discard_current_exception();
}

/// Port of `report_pending()` from `Src/ex_eval.c:720`.
///
/// RUST-PORT NOTE: the `'verbose'` message rendering (`smsg`/`IObuff`/
/// `concat_str`) is omitted; the wrappers below only call this when
/// `p_verbose >= 14` (or debugging), which never holds standalone.
fn report_pending(_action: i32, _pending: i32, _value: Option<Rc<RefCell<except_T>>>) {}

/// Port of `report_make_pending()` from `Src/ex_eval.c:796`.
fn report_make_pending(pending: i32, value: Option<Rc<RefCell<except_T>>>) {
    if p_verbose.with(|p| p.get()) >= 14 || debug_break_level.with(|d| d.get()) > 0 {
        report_pending(RP_MAKE, pending, value);
    }
}

/// Port of `report_resume_pending()` from `Src/ex_eval.c:811`.
fn report_resume_pending(pending: i32, value: Option<Rc<RefCell<except_T>>>) {
    if p_verbose.with(|p| p.get()) >= 14 || debug_break_level.with(|d| d.get()) > 0 {
        report_pending(RP_RESUME, pending, value);
    }
}

/// Port of `report_discard_pending()` from `Src/ex_eval.c:826`.
fn report_discard_pending(pending: i32, value: Option<Rc<RefCell<except_T>>>) {
    if p_verbose.with(|p| p.get()) >= 14 || debug_break_level.with(|d| d.get()) > 0 {
        report_pending(RP_DISCARD, pending, value);
    }
}

/// Port of `ex_eval()` from `Src/ex_eval.c:840` — handle `:eval`.
pub fn ex_eval(eap: &mut exarg_T) {
    let mut tv = typval_T::default();
    // c:845 fill_evalarg_from_eap — no-op standalone.
    crate::ported::eval::fill_evalarg_from_eap();
    // c:847 eval0 evaluates the expression string (through EVAL_STRING_HOOK).
    if crate::ported::eval::eval0(&eap.arg, &mut tv, None) == OK {
        crate::ported::eval::typval::tv_clear(&mut tv); // c:848
    }
    crate::ported::eval::clear_evalarg(); // c:851
}

/// Port of `ex_if()` from `Src/ex_eval.c:855` — handle `:if`.
pub fn ex_if(eap: &mut exarg_T) {
    if eap.cstack.cs_idx == CSTACK_LEN as i32 - 1 {
        eap.errmsg = Some("E579: :if nesting too deep"); // c:860
    } else {
        eap.cstack.cs_idx += 1; // c:862
        let idx = eap.cstack.cs_idx as usize;
        eap.cstack.cs_flags[idx] = 0; // c:863

        // CHECK_SKIP (c:80)
        let skip = did_emsg.with(|d| d.get()) != 0
            || got_int.with(|g| g.get())
            || did_throw.with(|t| t.get())
            || (eap.cstack.cs_idx > 0
                && (eap.cstack.cs_flags[(eap.cstack.cs_idx - 1) as usize] & CSF_ACTIVE) == 0);

        // c:868 eval_to_bool through EVAL_STRING_HOOK; error → parse/eval failure.
        let (result, error) = eval_to_bool(&eap.arg, skip);

        if !skip && !error {
            if result {
                eap.cstack.cs_flags[idx] = CSF_ACTIVE | CSF_TRUE; // c:872
            }
        } else {
            // c:876 set TRUE, so this conditional will never get active
            eap.cstack.cs_flags[idx] = CSF_TRUE;
        }
    }
}

/// Port of `ex_endif()` from `Src/ex_eval.c:882` — handle `:endif`.
pub fn ex_endif(eap: &mut exarg_T) {
    did_endif.with(|d| d.set(true)); // c:884
    let idx = eap.cstack.cs_idx;
    if idx < 0 || (eap.cstack.cs_flags[idx as usize] & (CSF_WHILE | CSF_FOR | CSF_TRY)) != 0 {
        eap.errmsg = Some("E580: :endif without :if"); // c:888
    } else {
        // c:897 On a not-TRUE ":if" with a skipped breakpoint, throw an interrupt.
        if (eap.cstack.cs_flags[idx as usize] & CSF_TRUE) == 0 && dbg_check_skipped(eap) {
            do_intthrow(&mut eap.cstack); // c:899
        }
        eap.cstack.cs_idx -= 1; // c:902
    }
}

/// Port of `ex_else()` from `Src/ex_eval.c:907` — handle `:else` and `:elseif`.
pub fn ex_else(eap: &mut exarg_T) {
    // CHECK_SKIP (c:80)
    let mut skip = did_emsg.with(|d| d.get()) != 0
        || got_int.with(|g| g.get())
        || did_throw.with(|t| t.get())
        || (eap.cstack.cs_idx > 0
            && (eap.cstack.cs_flags[(eap.cstack.cs_idx - 1) as usize] & CSF_ACTIVE) == 0);
    let idx = eap.cstack.cs_idx;

    if idx < 0 || (eap.cstack.cs_flags[idx as usize] & (CSF_WHILE | CSF_FOR | CSF_TRY)) != 0 {
        if eap.cmdidx == cmdidx_T::CMD_else {
            eap.errmsg = Some("E581: :else without :if"); // c:917
            return;
        }
        eap.errmsg = Some("E582: :elseif without :if"); // c:920
        skip = true;
    } else if eap.cstack.cs_flags[idx as usize] & CSF_ELSE != 0 {
        if eap.cmdidx == cmdidx_T::CMD_else {
            eap.errmsg = Some(e_multiple_else); // c:924
            return;
        }
        eap.errmsg = Some("E584: :elseif after :else"); // c:927
        skip = true;
    }
    let i = eap.cstack.cs_idx as usize;

    // c:932 if skipping or the ":if" was TRUE, reset ACTIVE, else set it
    if skip || eap.cstack.cs_flags[i] & CSF_TRUE != 0 {
        if eap.errmsg.is_none() {
            eap.cstack.cs_flags[i] = CSF_TRUE; // c:934
        }
        skip = true; // c:936 don't evaluate an ":elseif"
    } else {
        eap.cstack.cs_flags[i] = CSF_ACTIVE; // c:938
    }

    // c:949 skipped breakpoint → interrupt.
    if !skip && dbg_check_skipped(eap) && got_int.with(|g| g.get()) {
        do_intthrow(&mut eap.cstack);
        skip = true;
    }

    if eap.cmdidx == cmdidx_T::CMD_elseif {
        let mut result = false;
        let mut error = false;
        // c:960 A missing expression while skipping is still wrong.
        if skip
            && eap.arg.as_bytes().first() != Some(&b'"')
            && crate::ported::eval::ends_excmd(eap.arg.as_bytes().first().copied().unwrap_or(0))
        {
            semsg(e_invexpr2); // c:961
        } else {
            let r = eval_to_bool(&eap.arg, skip);
            result = r.0;
            error = r.1;
        }

        if !skip && !error {
            if result {
                eap.cstack.cs_flags[i] = CSF_ACTIVE | CSF_TRUE; // c:973
            } else {
                eap.cstack.cs_flags[i] = 0; // c:975
            }
        } else if eap.errmsg.is_none() {
            // c:979 set TRUE, so this conditional will never get active
            eap.cstack.cs_flags[i] = CSF_TRUE;
        }
    } else {
        eap.cstack.cs_flags[i] |= CSF_ELSE; // c:982
    }
}

/// Port of `ex_while()` from `Src/ex_eval.c:987` — handle `:while` and `:for`.
pub fn ex_while(eap: &mut exarg_T) {
    let mut error = false;

    if eap.cstack.cs_idx == CSTACK_LEN as i32 - 1 {
        eap.errmsg = Some("E585: :while/:for nesting too deep"); // c:993
    } else {
        let result;
        // c:999 On first entry (not a jump-back from ":endwhile"/":endfor"),
        // initialise this cstack entry.
        if eap.cstack.cs_lflags & CSL_HAD_LOOP == 0 {
            eap.cstack.cs_idx += 1;
            eap.cstack.cs_looplevel += 1;
            eap.cstack.cs_line[eap.cstack.cs_idx as usize] = -1;
        }
        let idx = eap.cstack.cs_idx as usize;
        eap.cstack.cs_flags[idx] = if eap.cmdidx == cmdidx_T::CMD_while {
            CSF_WHILE
        } else {
            CSF_FOR
        }; // c:1004

        // CHECK_SKIP (c:80)
        let skip = did_emsg.with(|d| d.get()) != 0
            || got_int.with(|g| g.get())
            || did_throw.with(|t| t.get())
            || (eap.cstack.cs_idx > 0
                && (eap.cstack.cs_flags[(eap.cstack.cs_idx - 1) as usize] & CSF_ACTIVE) == 0);
        if eap.cmdidx == cmdidx_T::CMD_while {
            // c:1009 ":while bool-expr"
            let r = eval_to_bool(&eap.arg, skip);
            result = r.0;
            error = r.1;
        } else {
            // c:1010 ":for var in list-expr"
            crate::ported::eval::fill_evalarg_from_eap();
            let fi: Option<()>;
            if eap.cstack.cs_lflags & CSL_HAD_LOOP != 0 {
                // c:1015 Jumping back: reuse the previously evaluated list.
                fi = eap.cstack.cs_forinfo[idx];
                error = false;
            } else {
                // c:1021 eval_for_line — deferred (see deferred_deps); no iterator.
                fi = None;
                eap.cstack.cs_forinfo[idx] = None;
            }

            // c:1026 use the element at the start of the list and advance
            if !error && fi.is_some() && !skip {
                result = crate::ported::eval::next_for_item();
            } else {
                result = false;
            }

            if !result {
                crate::ported::eval::free_for_info(); // c:1033
                eap.cstack.cs_forinfo[idx] = None;
            }
            crate::ported::eval::clear_evalarg();
        }

        // c:1042 If just initialised and active, set the loop flag so
        // do_cmdline() records the line number; if executing again, clear it.
        if !skip && !error && result {
            eap.cstack.cs_flags[idx] |= CSF_ACTIVE | CSF_TRUE;
            eap.cstack.cs_lflags ^= CSL_HAD_LOOP;
        } else {
            eap.cstack.cs_lflags &= !CSL_HAD_LOOP;
            if !skip && !error {
                eap.cstack.cs_flags[idx] |= CSF_TRUE; // c:1052
            }
        }
    }
}

/// Port of `ex_continue()` from `Src/ex_eval.c:1059` — handle `:continue`.
pub fn ex_continue(eap: &mut exarg_T) {
    if eap.cstack.cs_looplevel <= 0 || eap.cstack.cs_idx < 0 {
        eap.errmsg = Some("E586: :continue without :while or :for"); // c:1064
    } else {
        // c:1070 Find the matching ":while", deactivating all conditionals
        // except the ":while" itself (if reached).
        let idx = cleanup_conditionals(&mut eap.cstack, CSF_WHILE | CSF_FOR, false);
        debug_assert!(idx >= 0);
        if eap.cstack.cs_flags[idx as usize] & (CSF_WHILE | CSF_FOR) != 0 {
            let mut trylvl = eap.cstack.cs_trylevel;
            rewind_conditionals(&mut eap.cstack, idx, CSF_TRY, &mut trylvl); // c:1073
            eap.cstack.cs_trylevel = trylvl;
            // c:1077 CSL_HAD_CONT: do_cmdline() jumps back to the ":while".
            eap.cstack.cs_lflags |= CSL_HAD_CONT;
        } else {
            // c:1081 A try conditional reached first: make ":continue" pending.
            eap.cstack.cs_pending[idx as usize] = CSTP_CONTINUE as i8;
            report_make_pending(CSTP_CONTINUE, None);
        }
    }
}

/// Port of `ex_break()` from `Src/ex_eval.c:1088` — handle `:break`.
pub fn ex_break(eap: &mut exarg_T) {
    if eap.cstack.cs_looplevel <= 0 || eap.cstack.cs_idx < 0 {
        eap.errmsg = Some("E587: :break without :while or :for"); // c:1093
    } else {
        // c:1099 Deactivate conditionals until the matching ":while"/":for" or a
        // try conditional not in its finally clause is found.
        let idx = cleanup_conditionals(&mut eap.cstack, CSF_WHILE | CSF_FOR, true);
        if idx >= 0 && eap.cstack.cs_flags[idx as usize] & (CSF_WHILE | CSF_FOR) == 0 {
            eap.cstack.cs_pending[idx as usize] = CSTP_BREAK as i8; // c:1101
            report_make_pending(CSTP_BREAK, None);
        }
    }
}

/// Port of `ex_endwhile()` from `Src/ex_eval.c:1108` — handle `:endwhile`/`:endfor`.
pub fn ex_endwhile(eap: &mut exarg_T) {
    let err: &'static str;
    let csf: i32;

    if eap.cmdidx == cmdidx_T::CMD_endwhile {
        err = e_while; // c:1115
        csf = CSF_WHILE;
    } else {
        err = e_for; // c:1119
        csf = CSF_FOR;
    }

    if eap.cstack.cs_looplevel <= 0 || eap.cstack.cs_idx < 0 {
        eap.errmsg = Some(err); // c:1123
    } else {
        let cur = eap.cstack.cs_idx as usize;
        let mut fl = eap.cstack.cs_flags[cur];
        if fl & csf == 0 {
            // c:1127 Wrong endloop command: do not rewind.
            if fl & CSF_WHILE != 0 {
                eap.errmsg = Some("E732: Using :endfor with :while"); // c:1130
            } else if fl & CSF_FOR != 0 {
                eap.errmsg = Some("E733: Using :endwhile with :for"); // c:1132
            }
        }
        if fl & (CSF_WHILE | CSF_FOR) == 0 {
            if fl & CSF_TRY == 0 {
                eap.errmsg = Some(e_endif); // c:1137
            } else if fl & CSF_FINALLY != 0 {
                eap.errmsg = Some(e_endtry); // c:1139
            }
            // c:1142 Find the matching ":while" and report what's missing.
            let mut idx = eap.cstack.cs_idx;
            while idx > 0 {
                fl = eap.cstack.cs_flags[idx as usize];
                if (fl & CSF_TRY) != 0 && (fl & CSF_FINALLY) == 0 {
                    // c:1148 Give up at a try conditional not in its finally.
                    eap.errmsg = Some(err);
                    return;
                }
                if fl & csf != 0 {
                    break; // c:1152
                }
                idx -= 1;
            }
            // c:1156 Cleanup and rewind all contained (unclosed) conditionals.
            cleanup_conditionals(&mut eap.cstack, CSF_WHILE | CSF_FOR, false);
            let mut trylvl = eap.cstack.cs_trylevel;
            rewind_conditionals(&mut eap.cstack, idx, CSF_TRY, &mut trylvl);
            eap.cstack.cs_trylevel = trylvl;
        } else if eap.cstack.cs_flags[cur] & CSF_TRUE != 0
            && eap.cstack.cs_flags[cur] & CSF_ACTIVE == 0
            && dbg_check_skipped(eap)
        {
            // c:1169 skipped breakpoint at the endloop → interrupt.
            do_intthrow(&mut eap.cstack);
        }

        // c:1174 CSL_HAD_ENDLOOP: do_cmdline() jumps back to ":while"/":for".
        eap.cstack.cs_lflags |= CSL_HAD_ENDLOOP;
    }
}

/// Port of `ex_throw()` from `Src/ex_eval.c:1179` — handle `:throw expr`.
pub fn ex_throw(eap: &mut exarg_T) {
    let value: Option<String>;

    let first = eap.arg.as_bytes().first().copied();
    if !eap.arg.is_empty() && first != Some(b'|') && first != Some(b'\n') {
        // c:1185 eval_to_string_skip through EVAL_STRING_HOOK.
        value = crate::ported::eval::eval_to_string_skip(&eap.arg, eap.skip);
    } else {
        emsg(e_argreq); // c:1187
        value = None;
    }

    // c:1193 On error or a thrown exception during evaluation, do not throw.
    if !eap.skip {
        if let Some(v) = value {
            if throw_exception(v, except_type_T::ET_USER, None) == FAIL {
                // c:1195 xfree(value) — Drop-managed.
            } else {
                do_throw(&mut eap.cstack); // c:1197
            }
        }
    }
}

/// Port of `do_throw()` from `Src/ex_eval.c:1205`.
///
/// Throw the current exception through the specified cstack. Common routine for
/// `:throw` (user exception) and error/interrupt exceptions; also used for
/// rethrowing an uncaught exception.
pub fn do_throw(cstack: &mut cstack_T) {
    // c:1207 THROW_ON_ERROR/THROW_ON_INTERRUPT are true, so the try-inactivate
    // fix-up blocks are compiled out; inactivate_try stays false.
    let inactivate_try = false;

    let idx = cleanup_conditionals(cstack, 0, inactivate_try); // c:1229
    if idx >= 0 {
        let i = idx as usize;
        // c:1242 Before its first ":catch", mark THROWN so the ":catch" checks
        // for a match.
        if cstack.cs_flags[i] & CSF_CAUGHT == 0 {
            if cstack.cs_flags[i] & CSF_ACTIVE != 0 {
                cstack.cs_flags[i] |= CSF_THROWN; // c:1244
            } else {
                // c:1249 Reset THROWN for the new exception.
                cstack.cs_flags[i] &= !CSF_THROWN;
            }
        }
        cstack.cs_flags[i] &= !CSF_ACTIVE; // c:1252
        cstack.cs_pend.csp_ex[i] = current_exception.with(|c| c.borrow().clone());
        // c:1253
    }

    did_throw.with(|t| t.set(true)); // c:1256
}

/// Port of `ex_try()` from `Src/ex_eval.c:1260` — handle `:try`.
pub fn ex_try(eap: &mut exarg_T) {
    if eap.cstack.cs_idx == CSTACK_LEN as i32 - 1 {
        eap.errmsg = Some("E601: :try nesting too deep"); // c:1265
    } else {
        eap.cstack.cs_idx += 1; // c:1267
        eap.cstack.cs_trylevel += 1;
        let idx = eap.cstack.cs_idx as usize;
        eap.cstack.cs_flags[idx] = CSF_TRY;
        eap.cstack.cs_pending[idx] = CSTP_NONE as i8;

        // CHECK_SKIP (c:80)
        let skip = did_emsg.with(|d| d.get()) != 0
            || got_int.with(|g| g.get())
            || did_throw.with(|t| t.get())
            || (eap.cstack.cs_idx > 0
                && (eap.cstack.cs_flags[(eap.cstack.cs_idx - 1) as usize] & CSF_ACTIVE) == 0);

        if !skip {
            // c:1278 Set ACTIVE and TRUE.
            eap.cstack.cs_flags[idx] |= CSF_ACTIVE | CSF_TRUE;

            // c:1294 ":silent!" inside a try: save and reset "emsg_silent" so
            // errors are again converted to exceptions.
            if emsg_silent.with(|e| e.get()) != 0 {
                let elem = Box::new(eslist_T {
                    saved_emsg_silent: emsg_silent.with(|e| e.get()),
                    next: eap.cstack.cs_emsg_silent_list.take(),
                });
                eap.cstack.cs_emsg_silent_list = Some(elem);
                eap.cstack.cs_flags[idx] |= CSF_SILENT;
                emsg_silent.with(|e| e.set(0));
            }
        }
    }
}

/// Port of `ex_catch()` from `Src/ex_eval.c:1307` — handle `:catch /{pattern}/`.
///
/// RUST-PORT NOTE: the regex match path (`vim_regcomp`/`vim_regexec_nl`) and the
/// pattern scanner (`skip_regexp_err`/`find_nextcmd`) are deferred (see
/// deferred_deps); the cstack bookkeeping is ported faithfully.
pub fn ex_catch(eap: &mut exarg_T) {
    let mut idx: i32 = 0;
    let mut give_up = false;
    let mut skip = false;
    let end: Option<String>;
    let pat: String;

    if eap.cstack.cs_trylevel <= 0 || eap.cstack.cs_idx < 0 {
        eap.errmsg = Some("E603: :catch without :try"); // c:1319
        give_up = true;
    } else {
        if eap.cstack.cs_flags[eap.cstack.cs_idx as usize] & CSF_TRY == 0 {
            // c:1325 Report what's missing if the ":try" is not in its finally.
            eap.errmsg = Some(get_end_emsg(&eap.cstack));
            skip = true;
        }
        idx = eap.cstack.cs_idx;
        while idx > 0 {
            if eap.cstack.cs_flags[idx as usize] & CSF_TRY != 0 {
                break; // c:1330
            }
            idx -= 1;
        }
        if eap.cstack.cs_flags[idx as usize] & CSF_FINALLY != 0 {
            // c:1336 Give up for a ":catch" after ":finally".
            eap.errmsg = Some("E604: :catch after :finally");
            give_up = true;
        } else {
            let mut looplvl = eap.cstack.cs_looplevel;
            rewind_conditionals(&mut eap.cstack, idx, CSF_WHILE | CSF_FOR, &mut looplvl); // c:1339
            eap.cstack.cs_looplevel = looplvl;
        }
    }

    if crate::ported::eval::ends_excmd(eap.arg.as_bytes().first().copied().unwrap_or(0)) {
        // c:1344 no argument, catch all errors
        pat = ".*".to_string();
        end = None;
        eap.nextcmd = find_nextcmd(&eap.arg);
    } else {
        // c:1349 pattern is `eap->arg + 1` up to the matching delimiter.
        pat = eap.arg.get(1..).unwrap_or("").to_string();
        end = skip_regexp_err(&pat, eap.arg.as_bytes()[0], true);
        if end.is_none() {
            give_up = true; // c:1352
        }
    }

    if !give_up {
        let mut caught = false;
        // c:1361 Do nothing when nothing was thrown or the try block never
        // got active.
        if !did_throw.with(|t| t.get()) || eap.cstack.cs_flags[idx as usize] & CSF_TRUE == 0 {
            skip = true;
        }

        // c:1368 Check for a match only if thrown but not yet caught.
        if !skip
            && eap.cstack.cs_flags[idx as usize] & CSF_THROWN != 0
            && eap.cstack.cs_flags[idx as usize] & CSF_CAUGHT == 0
        {
            // c:1370 trailing-characters check on the pattern delimiter.
            if let Some(e) = end.as_deref() {
                // c:1370 ends_excmd(*skipwhite(end + 1))
                let after = crate::ported::eval::skipwhite(e.get(1..).unwrap_or(""));
                if !e.is_empty()
                    && !crate::ported::eval::ends_excmd(
                        after.as_bytes().first().copied().unwrap_or(0),
                    )
                {
                    semsg(e_trailing_arg); // c:1371
                    return;
                }
            }

            // c:1382 With a skipped breakpoint, replace the exception by an
            // interrupt and don't catch it here.
            if !dbg_check_skipped(eap) || !do_intthrow(&mut eap.cstack) {
                // c:1390 Compile the pattern with 'l' disabled in 'cpoptions'.
                emsg_off.with(|e| e.set(e.get() + 1)); // c:1394
                let mut regmatch = regmatch_T {
                    regprog: vim_regcomp(&pat, RE_MAGIC + RE_STRING), // c:1395
                    rm_ic: false,
                };
                emsg_off.with(|e| e.set(e.get() - 1)); // c:1396
                if regmatch.regprog.is_none() {
                    semsg(e_invarg2); // c:1403
                } else {
                    // c:1409 Save got_int; only CTRL-C during matching aborts.
                    let prev_got_int = got_int.with(|g| g.get());
                    got_int.with(|g| g.set(false));
                    let cur_val = current_exception
                        .with(|c| c.borrow().as_ref().map(|e| e.borrow().value.clone()));
                    caught = vim_regexec_nl(&mut regmatch, cur_val.as_deref().unwrap_or(""), 0);
                    got_int.with(|g| g.set(g.get() | prev_got_int));
                    vim_regfree(regmatch.regprog.take());
                }
            }
        }

        if caught {
            // c:1421 Make this ":catch" clause active; put the exception on the
            // caught stack.
            eap.cstack.cs_flags[idx as usize] |= CSF_ACTIVE | CSF_CAUGHT;
            did_emsg.with(|d| d.set(0));
            got_int.with(|g| g.set(false));
            did_throw.with(|t| t.set(false));
            if let Some(excp) = eap.cstack.cs_pend.csp_ex[idx as usize].clone() {
                catch_exception(&excp); // c:1423
            }
            // c:1429 The current exception must be the one stored in the cstack.
            let same = eap.cstack.cs_pend.csp_ex[eap.cstack.cs_idx as usize]
                .as_ref()
                .zip(current_exception.with(|c| c.borrow().clone()))
                .map(|(a, b)| Rc::ptr_eq(a, &b))
                .unwrap_or(false);
            if !same {
                internal_error("ex_catch()"); // c:1430
            }
        } else {
            // c:1441 No match: make the try conditional inactive so following
            // catch clauses are skipped; discard any pending action.
            cleanup_conditionals(&mut eap.cstack, CSF_TRY, true);
        }
    }

    if let Some(e) = end {
        eap.nextcmd = find_nextcmd(&e); // c:1446
    }
}

/// Port of `ex_finally()` from `Src/ex_eval.c:1451` — handle `:finally`.
pub fn ex_finally(eap: &mut exarg_T) {
    let mut idx: i32 = eap.cstack.cs_idx;
    let mut pending = CSTP_NONE;

    while idx >= 0 {
        if eap.cstack.cs_flags[idx as usize] & CSF_TRY != 0 {
            break; // c:1458
        }
        idx -= 1;
    }

    if eap.cstack.cs_trylevel <= 0 || idx < 0 {
        eap.errmsg = Some("E606: :finally without :try"); // c:1463
        return;
    }

    if eap.cstack.cs_flags[eap.cstack.cs_idx as usize] & CSF_TRY == 0 {
        eap.errmsg = Some(get_end_emsg(&eap.cstack)); // c:1468
                                                      // c:1472 Make this error pending so the finally clause can run.
        pending = CSTP_ERROR;
    }

    if eap.cstack.cs_flags[idx as usize] & CSF_FINALLY != 0 {
        eap.errmsg = Some(e_multiple_finally); // c:1477
        return;
    }
    let mut looplvl = eap.cstack.cs_looplevel;
    rewind_conditionals(&mut eap.cstack, idx, CSF_WHILE | CSF_FOR, &mut looplvl); // c:1480
    eap.cstack.cs_looplevel = looplvl;

    // c:1489 Do nothing if the try block never got active.
    let skip = (eap.cstack.cs_flags[eap.cstack.cs_idx as usize] & CSF_TRUE) == 0;

    if !skip {
        // c:1495 skipped breakpoint → discard exception, replace by interrupt.
        if dbg_check_skipped(eap) {
            do_intthrow(&mut eap.cstack);
        }

        // c:1509 Finish a caught exception / discard a pending action.
        cleanup_conditionals(&mut eap.cstack, CSF_TRY, false);

        // c:1524 Make did_emsg/got_int/did_throw pending, overruling a pending
        // ":continue"/":break"/":return"/":finish".
        if pending == CSTP_ERROR
            || did_emsg.with(|d| d.get()) != 0
            || got_int.with(|g| g.get())
            || did_throw.with(|t| t.get())
        {
            let ci = eap.cstack.cs_idx as usize;
            if eap.cstack.cs_pending[ci] as i32 == CSTP_RETURN {
                report_discard_pending(CSTP_RETURN, None); // c:1526
                discard_pending_return(eap.cstack.cs_pend.csp_rv[ci].take()); // c:1528
            }
            if pending == CSTP_ERROR && did_emsg.with(|d| d.get()) == 0 {
                pending |= if THROW_ON_ERROR { CSTP_THROW } else { 0 }; // c:1531
            } else {
                pending |= if did_throw.with(|t| t.get()) {
                    CSTP_THROW
                } else {
                    0
                }; // c:1533
            }
            pending |= if did_emsg.with(|d| d.get()) != 0 {
                CSTP_ERROR
            } else {
                0
            }; // c:1535
            pending |= if got_int.with(|g| g.get()) {
                CSTP_INTERRUPT
            } else {
                0
            }; // c:1536
            eap.cstack.cs_pending[ci] = pending as i8; // c:1538

            // c:1547 The current exception must be the stored one.
            if did_throw.with(|t| t.get()) {
                let same = eap.cstack.cs_pend.csp_ex[ci]
                    .as_ref()
                    .zip(current_exception.with(|c| c.borrow().clone()))
                    .map(|(a, b)| Rc::ptr_eq(a, &b))
                    .unwrap_or(false);
                if !same {
                    internal_error("ex_finally()");
                }
            }
        }

        // c:1557 CSL_HAD_FINA: do_cmdline() resets the flags and runs the finally.
        eap.cstack.cs_lflags |= CSL_HAD_FINA;
    }
}

/// Port of `ex_endtry()` from `Src/ex_eval.c:1562` — handle `:endtry`.
pub fn ex_endtry(eap: &mut exarg_T) {
    let mut idx: i32 = eap.cstack.cs_idx;
    let mut rethrow = false;
    let mut pending = CSTP_NONE;
    let mut rettv: Option<typval_T> = None;

    while idx >= 0 {
        if eap.cstack.cs_flags[idx as usize] & CSF_TRY != 0 {
            break; // c:1571
        }
        idx -= 1;
    }
    if eap.cstack.cs_trylevel <= 0 || idx < 0 {
        eap.errmsg = Some("E602: :endtry without :try"); // c:1576
        return;
    }

    // c:1590 Skip on a preceding error/interrupt/throw or an inactive try.
    let mut skip = did_emsg.with(|d| d.get()) != 0
        || got_int.with(|g| g.get())
        || did_throw.with(|t| t.get())
        || (eap.cstack.cs_flags[eap.cstack.cs_idx as usize] & CSF_TRUE) == 0;

    if eap.cstack.cs_flags[eap.cstack.cs_idx as usize] & CSF_TRY == 0 {
        eap.errmsg = Some(get_end_emsg(&eap.cstack)); // c:1593

        // c:1596 Find the matching ":try" and report what's missing.
        let mut looplvl = eap.cstack.cs_looplevel;
        rewind_conditionals(&mut eap.cstack, idx, CSF_WHILE | CSF_FOR, &mut looplvl);
        eap.cstack.cs_looplevel = looplvl;
        skip = true;

        // c:1605 Discard an exception being thrown to stop it being rethrown.
        if did_throw.with(|t| t.get()) {
            discard_current_exception();
        }

        did_emsg.with(|d| d.set(0)); // c:1610
    } else {
        idx = eap.cstack.cs_idx; // c:1612

        // c:1618 If we stopped at this try conditional with the exception still
        // being thrown and there's no finally clause, rethrow after closing it.
        if did_throw.with(|t| t.get())
            && eap.cstack.cs_flags[idx as usize] & CSF_TRUE != 0
            && eap.cstack.cs_flags[idx as usize] & CSF_FINALLY == 0
        {
            rethrow = true;
        }
    }

    // c:1632 With no finally clause, show the debug prompt at the ":endtry".
    if (rethrow
        || (!skip
            && eap.cstack.cs_flags[idx as usize] & CSF_FINALLY == 0
            && eap.cstack.cs_pending[idx as usize] == 0))
        && dbg_check_skipped(eap)
    {
        if got_int.with(|g| g.get()) {
            skip = true;
            do_intthrow(&mut eap.cstack);
            // c:1644 do_intthrow() may have reset did_throw/cs_pending[idx].
            rethrow = false;
            if did_throw.with(|t| t.get()) && eap.cstack.cs_flags[idx as usize] & CSF_FINALLY == 0 {
                rethrow = true;
            }
        }
    }

    // c:1655 Resume a pending ":return"; rethrow a pending exception.
    if !skip {
        pending = eap.cstack.cs_pending[idx as usize] as i32;
        eap.cstack.cs_pending[idx as usize] = CSTP_NONE as i8;
        if pending == CSTP_RETURN {
            rettv = eap.cstack.cs_pend.csp_rv[idx as usize].take(); // c:1659
        } else if pending & CSTP_THROW != 0 {
            current_exception
                .with(|c| *c.borrow_mut() = eap.cstack.cs_pend.csp_ex[idx as usize].clone());
            // c:1661
        }
    }

    // c:1673 Discard anything pending; restore "emsg_silent"; finish a caught
    // exception if there was no finally clause.
    cleanup_conditionals(&mut eap.cstack, CSF_TRY | CSF_SILENT, true);

    if eap.cstack.cs_idx >= 0 && eap.cstack.cs_flags[eap.cstack.cs_idx as usize] & CSF_TRY != 0 {
        eap.cstack.cs_idx -= 1; // c:1676
    }
    eap.cstack.cs_trylevel -= 1; // c:1678

    if !skip {
        report_resume_pending(
            pending,
            if pending & CSTP_THROW != 0 {
                current_exception.with(|c| c.borrow().clone())
            } else {
                None
            },
        ); // c:1681
        match pending {
            CSTP_NONE => {} // c:1686
            // c:1696 Reactivate a pending ":continue"/":break"/":return"/":finish".
            CSTP_CONTINUE => ex_continue(eap),
            CSTP_BREAK => ex_break(eap),
            CSTP_RETURN => {
                crate::ported::eval::userfunc::do_return(false, false, rettv); // c:1703
            }
            CSTP_FINISH => do_finish(eap, false), // c:1706
            // c:1715 Restore the pending did_emsg/got_int/did_throw.
            _ => {
                if pending & CSTP_ERROR != 0 {
                    did_emsg.with(|d| d.set(1)); // c:1717
                }
                if pending & CSTP_INTERRUPT != 0 {
                    got_int.with(|g| g.set(true)); // c:1720
                }
                if pending & CSTP_THROW != 0 {
                    rethrow = true; // c:1723
                }
            }
        }
    }

    if rethrow {
        do_throw(&mut eap.cstack); // c:1731
    }
}

/// Port of `enter_cleanup()` from `Src/ex_eval.c:1752`.
///
/// Save the error/interrupt/exception state before a cleanup autocommand run.
/// RUST-PORT NOTE: the `msg_list` handling is deferred (no global msg_list).
pub fn enter_cleanup(csp: &mut cleanup_T) {
    let pending = CSTP_NONE;

    // c:1759 Postpone did_emsg/got_int/did_throw/need_rethrow.
    if did_emsg.with(|d| d.get()) != 0
        || got_int.with(|g| g.get())
        || did_throw.with(|t| t.get())
        || need_rethrow.with(|n| n.get())
    {
        csp.pending = (if did_emsg.with(|d| d.get()) != 0 {
            CSTP_ERROR
        } else {
            0
        }) | (if got_int.with(|g| g.get()) {
            CSTP_INTERRUPT
        } else {
            0
        }) | (if did_throw.with(|t| t.get()) {
            CSTP_THROW
        } else {
            0
        }) | (if need_rethrow.with(|n| n.get()) {
            CSTP_THROW
        } else {
            0
        }); // c:1760

        if did_throw.with(|t| t.get()) || need_rethrow.with(|n| n.get()) {
            csp.exception = current_exception.with(|c| c.borrow().clone()); // c:1772
            current_exception.with(|c| *c.borrow_mut() = None);
        } else {
            csp.exception = None; // c:1775
            if did_emsg.with(|d| d.get()) != 0 {
                force_abort.with(|f| f.set(f.get() | cause_abort.with(|c| c.get()))); // c:1777
                cause_abort.with(|c| c.set(false));
            }
        }
        did_emsg.with(|d| d.set(0));
        got_int.with(|g| g.set(false));
        did_throw.with(|t| t.set(false));
        need_rethrow.with(|n| n.set(false)); // c:1781

        report_make_pending(pending, csp.exception.clone()); // c:1784
    } else {
        csp.pending = CSTP_NONE; // c:1786
        csp.exception = None;
    }
}

/// Port of `leave_cleanup()` from `Src/ex_eval.c:1804`.
///
/// Restore the error/interrupt/exception state saved by [`enter_cleanup`].
/// RUST-PORT NOTE: `msg_list` freeing is deferred.
pub fn leave_cleanup(csp: &mut cleanup_T) {
    let pending = csp.pending;

    if pending == CSTP_NONE {
        return; // c:1809
    }

    // c:1816 On an aborting error/interrupt/uncaught exception, discard pending.
    if aborting() || need_rethrow.with(|n| n.get()) {
        if pending & CSTP_THROW != 0 {
            if let Some(excp) = csp.exception.as_ref() {
                discard_exception(excp, false); // c:1819
            }
        } else {
            report_discard_pending(pending, None); // c:1821
        }
        // c:1826 free the deferred error msg_list — not modelled.
    } else {
        // c:1837 Restore the pending state.
        if pending & CSTP_THROW != 0 {
            current_exception.with(|c| *c.borrow_mut() = csp.exception.take());
        } else if pending & CSTP_ERROR != 0 {
            cause_abort.with(|c| c.set(force_abort.with(|f| f.get()))); // c:1843
            force_abort.with(|f| f.set(false));
        }

        if pending & CSTP_ERROR != 0 {
            did_emsg.with(|d| d.set(1)); // c:1849
        }
        if pending & CSTP_INTERRUPT != 0 {
            got_int.with(|g| g.set(true)); // c:1851
        }
        if pending & CSTP_THROW != 0 {
            need_rethrow.with(|n| n.set(true)); // c:1855
        }

        report_resume_pending(
            pending,
            if pending & CSTP_THROW != 0 {
                current_exception.with(|c| c.borrow().clone())
            } else {
                None
            },
        ); // c:1859
    }
}

/// Port of `cleanup_conditionals()` from `Src/ex_eval.c:1882`.
///
/// Make conditionals inactive and discard what's pending in finally clauses
/// until `searched_cond` (or a try conditional not in its finally clause) is
/// reached. If in an active catch clause, finish the caught exception.
///
/// @return  the cstack index where the search stopped.
pub fn cleanup_conditionals(cstack: &mut cstack_T, searched_cond: i32, inclusive: bool) -> i32 {
    let mut idx: i32 = cstack.cs_idx;
    let mut stop = false;

    while idx >= 0 {
        let i = idx as usize;
        if cstack.cs_flags[i] & CSF_TRY != 0 {
            // c:1893 Discard anything pending in a finally clause.
            if did_emsg.with(|d| d.get()) != 0
                || got_int.with(|g| g.get())
                || cstack.cs_flags[i] & CSF_FINALLY != 0
            {
                match cstack.cs_pending[i] as i32 {
                    CSTP_NONE => {} // c:1895
                    CSTP_CONTINUE | CSTP_BREAK | CSTP_FINISH => {
                        report_discard_pending(cstack.cs_pending[i] as i32, None); // c:1901
                        cstack.cs_pending[i] = CSTP_NONE as i8;
                    }
                    CSTP_RETURN => {
                        report_discard_pending(CSTP_RETURN, None); // c:1906
                        discard_pending_return(cstack.cs_pend.csp_rv[i].take());
                        cstack.cs_pending[i] = CSTP_NONE as i8;
                    }
                    p => {
                        if cstack.cs_flags[i] & CSF_FINALLY != 0 {
                            if (p & CSTP_THROW) != 0 && cstack.cs_pend.csp_ex[i].is_some() {
                                // c:1918 Cancel the pending exception.
                                if let Some(excp) = cstack.cs_pend.csp_ex[i].clone() {
                                    discard_exception(&excp, false);
                                }
                            } else {
                                report_discard_pending(p, None); // c:1920
                            }
                            cstack.cs_pending[i] = CSTP_NONE as i8;
                        }
                    }
                }
            }

            // c:1931 Stop at a try conditional not in its finally clause.
            if cstack.cs_flags[i] & CSF_FINALLY == 0 {
                if cstack.cs_flags[i] & CSF_ACTIVE != 0
                    && cstack.cs_flags[i] & CSF_CAUGHT != 0
                    && cstack.cs_flags[i] & CSF_FINISHED == 0
                {
                    if let Some(excp) = cstack.cs_pend.csp_ex[i].clone() {
                        finish_exception(&excp); // c:1934
                    }
                    cstack.cs_flags[i] |= CSF_FINISHED;
                }
                // c:1941 Stop at this try conditional (unless it never got active).
                if cstack.cs_flags[i] & CSF_TRUE != 0 {
                    if searched_cond == 0 && !inclusive {
                        break; // c:1943
                    }
                    stop = true;
                }
            }
        }

        // c:1954 Stop on the searched conditional type.
        if cstack.cs_flags[i] & searched_cond != 0 {
            if !inclusive {
                break; // c:1956
            }
            stop = true;
        }
        cstack.cs_flags[i] &= !CSF_ACTIVE; // c:1960
        if stop && searched_cond != (CSF_TRY | CSF_SILENT) {
            break; // c:1961
        }

        // c:1968 Restore "emsg_silent" reset on this try conditional's entry.
        if cstack.cs_flags[i] & CSF_TRY != 0 && cstack.cs_flags[i] & CSF_SILENT != 0 {
            if let Some(mut elem) = cstack.cs_emsg_silent_list.take() {
                cstack.cs_emsg_silent_list = elem.next.take();
                emsg_silent.with(|e| e.set(elem.saved_emsg_silent));
            }
            cstack.cs_flags[i] &= !CSF_SILENT;
        }
        if stop {
            break; // c:1978
        }
        idx -= 1;
    }
    idx
}

/// Port of `get_end_emsg()` from `Src/ex_eval.c:1986`.
///
/// @return  an appropriate error message for a missing endwhile/endfor/endif.
fn get_end_emsg(cstack: &cstack_T) -> &'static str {
    if cstack.cs_flags[cstack.cs_idx as usize] & CSF_WHILE != 0 {
        return e_endwhile; // c:1989
    }
    if cstack.cs_flags[cstack.cs_idx as usize] & CSF_FOR != 0 {
        return e_endfor; // c:1992
    }
    e_endif // c:1994
}

/// Port of `rewind_conditionals()` from `Src/ex_eval.c:2002`.
///
/// Rewind conditionals until index `idx` is reached, decrementing `cond_level`
/// for each skipped conditional of type `cond_type`; free `:for` info as needed.
/// RUST-PORT NOTE: the C passes `&cstack->cs_looplevel`/`&cstack->cs_trylevel`
/// directly; since that aliases `cstack` (also mutated here), callers copy the
/// level into a local, pass `&mut` to it, and write it back.
pub fn rewind_conditionals(cstack: &mut cstack_T, idx: i32, cond_type: i32, cond_level: &mut i32) {
    while cstack.cs_idx > idx {
        let i = cstack.cs_idx as usize;
        if cstack.cs_flags[i] & cond_type != 0 {
            *cond_level -= 1; // c:2006
        }
        if cstack.cs_flags[i] & CSF_FOR != 0 {
            crate::ported::eval::free_for_info(); // c:2009
            cstack.cs_forinfo[i] = None;
        }
        cstack.cs_idx -= 1; // c:2011
    }
}

/// Port of `ex_endfunction()` from `Src/ex_eval.c:2016`.
///
/// Handle `:endfunction` when not after a `:function`.
pub fn ex_endfunction(_eap: &mut exarg_T) {
    semsg(e_str_not_inside_function); // c:2018
}

/// Port of `has_loop_cmd()` from `Src/ex_eval.c:2022`.
///
/// @return  true if the string `p` looks like a `:while` or `:for` command.
/// RUST-PORT NOTE: `modifier_len()` command-modifier skipping is reduced to
/// leading-whitespace/`:` trimming.
pub fn has_loop_cmd(p: &str) -> bool {
    let t = p.trim_start_matches([' ', '\t', ':']);
    let b = t.as_bytes();
    (b.first() == Some(&b'w') && b.get(1) == Some(&b'h'))
        || (b.first() == Some(&b'f') && b.get(1) == Some(&b'o') && b.get(2) == Some(&b'r'))
}

// ── local carve-out glue (no C counterpart) ─────────────────────────────────

/// Port of `eval_to_bool()` from `Src/eval.c:249`.
///
/// Evaluate the boolean expression string `arg` through the `EVAL_STRING_HOOK`
/// bridge (the same integration point the bytecode frontend installs). Returns
/// `(result, error)`: a parse/eval failure (no hook result) sets `error`.
/// RUST-PORT NOTE: the C `eap`/`use_simple_function` params are dropped; `skip`
/// short-circuits without evaluating (result/error both false).
fn eval_to_bool(arg: &str, skip: bool) -> (bool, bool) {
    if skip {
        return (false, false);
    }
    match crate::ported::eval::typval::EVAL_STRING_HOOK
        .with(|h| *h.borrow())
        .and_then(|f| f(arg))
    {
        Some(tv) => (
            crate::ported::eval::typval::tv_get_number_chk(&tv, None) != 0,
            false,
        ),
        None => (false, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::eval::typval::EVAL_STRING_HOOK;

    /// Install a numeric-literal EVAL_STRING_HOOK for the duration of `f`.
    fn with_num_hook<F: FnOnce()>(f: F) {
        fn hook(e: &str) -> Option<typval_T> {
            e.trim().parse::<i64>().ok().map(typval_T::from)
        }
        let saved = EVAL_STRING_HOOK.with(|h| *h.borrow());
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = Some(hook));
        f();
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = saved);
    }

    fn new_eap(cmdidx: cmdidx_T, arg: &str) -> exarg_T {
        exarg_T {
            arg: arg.to_string(),
            cmdidx,
            forceit: false,
            skip: false,
            errmsg: None,
            nextcmd: None,
            cstack: cstack_T::default(),
        }
    }

    fn reset_globals() {
        exception_state_clear();
        emsg_silent.with(|e| e.set(0));
        need_rethrow.with(|n| n.set(false));
        caught_stack.with(|c| *c.borrow_mut() = None);
    }

    #[test]
    fn exception_string_formatting() {
        use super::except_type_T::*;
        assert_eq!(get_exception_string("boom", ET_USER, None), "boom");
        assert_eq!(get_exception_string("boom", ET_USER, Some("catch")), "boom");
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
    fn if_true_activates_then_endif_pops() {
        reset_globals();
        with_num_hook(|| {
            let mut eap = new_eap(cmdidx_T::CMD_if, "1");
            ex_if(&mut eap);
            assert_eq!(eap.cstack.cs_idx, 0);
            assert_eq!(
                eap.cstack.cs_flags[0] & (CSF_ACTIVE | CSF_TRUE),
                CSF_ACTIVE | CSF_TRUE
            );
            assert!(eap.errmsg.is_none());
            // :endif pops the stack.
            eap.cmdidx = cmdidx_T::CMD_endif;
            ex_endif(&mut eap);
            assert_eq!(eap.cstack.cs_idx, -1);
            assert!(eap.errmsg.is_none());
            assert!(did_endif.with(|d| d.get()));
        });
    }

    #[test]
    fn if_false_is_inactive_but_true_flag_set() {
        reset_globals();
        with_num_hook(|| {
            let mut eap = new_eap(cmdidx_T::CMD_if, "0");
            ex_if(&mut eap);
            // A FALSE ":if": neither ACTIVE nor TRUE; the block stays skippable.
            assert_eq!(eap.cstack.cs_flags[0] & CSF_ACTIVE, 0);
            assert_eq!(eap.cstack.cs_flags[0] & CSF_TRUE, 0);
        });
    }

    #[test]
    fn else_activates_after_false_if() {
        reset_globals();
        with_num_hook(|| {
            let mut eap = new_eap(cmdidx_T::CMD_if, "0");
            ex_if(&mut eap);
            eap.cmdidx = cmdidx_T::CMD_else;
            eap.arg.clear();
            ex_else(&mut eap);
            // :else after a not-TRUE :if becomes ACTIVE and marks CSF_ELSE.
            assert_ne!(eap.cstack.cs_flags[0] & CSF_ACTIVE, 0);
            assert_ne!(eap.cstack.cs_flags[0] & CSF_ELSE, 0);
        });
    }

    #[test]
    fn else_without_if_errors() {
        reset_globals();
        let mut eap = new_eap(cmdidx_T::CMD_else, "");
        ex_else(&mut eap);
        assert_eq!(eap.errmsg, Some("E581: :else without :if"));
    }

    #[test]
    fn endif_without_if_errors() {
        reset_globals();
        let mut eap = new_eap(cmdidx_T::CMD_endif, "");
        ex_endif(&mut eap);
        assert_eq!(eap.errmsg, Some("E580: :endif without :if"));
    }

    #[test]
    fn while_true_activates_and_sets_loop_flag() {
        reset_globals();
        with_num_hook(|| {
            let mut eap = new_eap(cmdidx_T::CMD_while, "1");
            ex_while(&mut eap);
            assert_eq!(eap.cstack.cs_idx, 0);
            assert_eq!(eap.cstack.cs_looplevel, 1);
            assert_ne!(eap.cstack.cs_flags[0] & CSF_WHILE, 0);
            assert_eq!(
                eap.cstack.cs_flags[0] & (CSF_ACTIVE | CSF_TRUE),
                CSF_ACTIVE | CSF_TRUE
            );
            assert_ne!(eap.cstack.cs_lflags & CSL_HAD_LOOP, 0);
        });
    }

    #[test]
    fn continue_break_without_loop_error() {
        reset_globals();
        let mut eap = new_eap(cmdidx_T::CMD_continue, "");
        ex_continue(&mut eap);
        assert_eq!(eap.errmsg, Some("E586: :continue without :while or :for"));
        let mut eap2 = new_eap(cmdidx_T::CMD_break, "");
        ex_break(&mut eap2);
        assert_eq!(eap2.errmsg, Some("E587: :break without :while or :for"));
    }

    #[test]
    fn try_pushes_and_endtry_pops() {
        reset_globals();
        let mut eap = new_eap(cmdidx_T::CMD_try, "");
        ex_try(&mut eap);
        assert_eq!(eap.cstack.cs_idx, 0);
        assert_eq!(eap.cstack.cs_trylevel, 1);
        assert_ne!(eap.cstack.cs_flags[0] & CSF_TRY, 0);
        assert_eq!(
            eap.cstack.cs_flags[0] & (CSF_ACTIVE | CSF_TRUE),
            CSF_ACTIVE | CSF_TRUE
        );
        // :endtry closes it.
        eap.cmdidx = cmdidx_T::CMD_endtry;
        ex_endtry(&mut eap);
        assert_eq!(eap.cstack.cs_idx, -1);
        assert_eq!(eap.cstack.cs_trylevel, 0);
        assert!(eap.errmsg.is_none());
    }

    #[test]
    fn catch_finally_without_try_error() {
        reset_globals();
        let mut eap = new_eap(cmdidx_T::CMD_catch, "");
        ex_catch(&mut eap);
        assert_eq!(eap.errmsg, Some("E603: :catch without :try"));
        let mut eap2 = new_eap(cmdidx_T::CMD_finally, "");
        ex_finally(&mut eap2);
        assert_eq!(eap2.errmsg, Some("E606: :finally without :try"));
        let mut eap3 = new_eap(cmdidx_T::CMD_endtry, "");
        ex_endtry(&mut eap3);
        assert_eq!(eap3.errmsg, Some("E602: :endtry without :try"));
    }

    #[test]
    fn throw_sets_current_exception_and_did_throw() {
        reset_globals();
        with_num_hook(|| {
            // Build a :try so do_throw has a conditional to target.
            let mut eap = new_eap(cmdidx_T::CMD_try, "");
            ex_try(&mut eap);
            eap.cmdidx = cmdidx_T::CMD_throw;
            eap.arg = "42".to_string();
            ex_throw(&mut eap);
            assert!(did_throw.with(|t| t.get()));
            let v =
                current_exception.with(|c| c.borrow().as_ref().map(|e| e.borrow().value.clone()));
            assert_eq!(v.as_deref(), Some("42"));
            // The exception was stashed on the try conditional and THROWN set.
            assert_ne!(eap.cstack.cs_flags[0] & CSF_THROWN, 0);
        });
        reset_globals();
    }

    #[test]
    fn throw_vim_prefix_rejected() {
        reset_globals();
        assert_eq!(
            throw_exception("Vim:boom".to_string(), except_type_T::ET_USER, None),
            FAIL
        );
        assert!(current_exception.with(|c| c.borrow().is_none()));
    }

    #[test]
    fn abort_state_defaults_and_toggles() {
        reset_globals();
        assert!(!aborting());
        assert!(!aborted_in_try());
        assert!(!should_abort(FAIL));
        trylevel.with(|t| t.set(1));
        assert!(should_abort(FAIL));
        trylevel.with(|t| t.set(0));
        did_throw.with(|t| t.set(true));
        assert!(aborting());
        discard_current_exception();
        assert!(!aborting());
        assert!(has_loop_cmd("  :while x"));
        assert!(has_loop_cmd("for i in x"));
        assert!(!has_loop_cmd("echo 1"));
        // Snapshot with did_throw clear, dirty it, then restore. Restore first
        // discards the outstanding throw (handle_did_throw) then re-applies the
        // saved (clear) did_throw. got_int is not part of exception_state_T.
        let snap = exception_state_save();
        did_throw.with(|t| t.set(true));
        exception_state_restore(snap);
        assert!(!did_throw.with(|t| t.get()));
    }

    #[test]
    fn nesting_too_deep_errors() {
        reset_globals();
        with_num_hook(|| {
            let mut eap = new_eap(cmdidx_T::CMD_if, "1");
            eap.cstack.cs_idx = CSTACK_LEN as i32 - 1;
            ex_if(&mut eap);
            assert_eq!(eap.errmsg, Some("E579: :if nesting too deep"));
        });
    }
}
