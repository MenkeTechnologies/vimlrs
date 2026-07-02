//! Port of `src/nvim/eval/userfunc.c` (vendored at `csrc/eval/userfunc.c`).
//!
//! The user-function call machinery itself is driven by the bytecode bridge
//! (`b_call_user`, the `FUNCTIONS` registry); this module ports the pure
//! function-*name* classification helpers `userfunc.c` exposes — telling apart
//! builtin names, script-local (`s:`/`<SID>`) names, lambda/refcounted names,
//! and emitting a function-name error.
#![allow(non_snake_case, non_upper_case_globals)]

use crate::ported::message::semsg;

/// Port of `eval_fname_sid()` from `Src/eval/userfunc.c` — true when a name that
/// already passed [`eval_fname_script`] starts with `s:` or `<SID>`/`<SNR>`
/// (`'s'`, or `'I'`/`'R'` at index 2). NONNULL, name length ≥ 3 by contract.
pub fn eval_fname_sid(name: &str) -> bool {
    let b = name.as_bytes();
    b.first() == Some(&b's') || b.get(2).map(|c| c.to_ascii_uppercase()) == Some(b'I')
}

/// Port of `eval_fname_script()` from `Src/eval/userfunc.c` — the length of a
/// leading script-local prefix: 5 for `<SID>`/`<SNR>`, 2 for `s:`, else 0.
pub fn eval_fname_script(p: &str) -> i32 {
    let lower = p.to_ascii_lowercase();
    if p.starts_with('<') && (lower.starts_with("<sid>") || lower.starts_with("<snr>")) {
        return 5;
    }
    if p.starts_with("s:") {
        return 2;
    }
    0
}

/// Port of `func_name_refcount()` from `Src/eval/userfunc.c` — true when a
/// function name owns a reference count: a numbered (anonymous) function (leading
/// digit) or a lambda (`<lambda>…`).
pub fn func_name_refcount(name: &str) -> bool {
    let b = name.as_bytes();
    matches!(b.first(), Some(c) if c.is_ascii_digit())
        || (b.first() == Some(&b'<') && b.get(1) == Some(&b'l'))
}

/// Port of `builtin_function()` from `Src/eval/userfunc.c` — true when `name` is
/// a builtin: lowercase first char, not a scope (`x:`), and no autoload `#`.
/// `len < 0` means the whole NUL-terminated name.
pub fn builtin_function(name: &str, len: i32) -> bool {
    let b = name.as_bytes();
    if b.first().map(|c| !c.is_ascii_lowercase()).unwrap_or(true) || b.get(1) == Some(&b':') {
        return false;
    }
    let slice = if len < 0 {
        name
    } else {
        &name[..(len as usize).min(name.len())]
    };
    !slice.contains('#')
}

/// Port of `emsg_funcname()` from `Src/eval/userfunc.c` — report a function-name
/// error, stripping a leading `<SNR>` from the displayed name.
pub fn emsg_funcname(errmsg: &str, name: &str) {
    let shown = name.strip_prefix("\u{1}").unwrap_or(name);
    semsg(&errmsg.replace("%s", shown));
}

/// Port of `eval_fname()`-adjacent `get_scriptlocal_funcname()` from
/// `Src/eval/userfunc.c` — translate a script-local function name to its
/// `<SNR>{sid}_` form. The standalone interpreter has a single, unnumbered
/// script context, so a script-local name has no valid SID to bind → `None`.
pub fn get_scriptlocal_funcname(funcname: Option<&str>) -> Option<String> {
    let name = funcname?;
    if !name.starts_with("s:") && !name.starts_with("<SID>") {
        return None;
    }
    None
}

/// Port of `function_list_modified()` from `Src/eval/userfunc.c` — whether the
/// function table changed since `prev` (its hashtab change counter). The
/// registry is not change-counted here → treat as unmodified (false).
pub fn function_list_modified(_prev_ht_changed: i32) -> bool {
    false
}

/// Port of `call_simple_luafunc()` from `Src/eval/userfunc.c` — call a bare
/// `v:lua` function; no Lua runtime standalone → FAIL.
pub fn call_simple_luafunc() -> i32 {
    crate::ported::eval_h::FAIL
}

/// Port of `func_init()` from `Src/eval/userfunc.c` — function-table init; the
/// bridge's `FUNCTIONS` registry is created lazily, so no-op.
pub fn func_init() {}

/// Port of `free_all_functions()` from `Src/eval/userfunc.c` — teardown of all
/// user functions; the `Rc`-managed registry is dropped automatically, no-op.
pub fn free_all_functions() {}

/// Port of `list_functions()` from `Src/eval/userfunc.c` — interactive
/// `:function` listing; no message output standalone (no-op).
pub fn list_functions() {}

/// Port of `call_simple_func()` from `Src/eval/userfunc.c` — the fast path for a
/// bare-name call; the bridge handles real calls, so this fast path is unused
/// here → FAIL (fall through to the normal call path).
pub fn call_simple_func() -> i32 {
    crate::ported::eval_h::FAIL
}

/// Port of `user_func_error()` from `Src/eval/userfunc.c` — report the error for
/// a failed user-function call (`E117` unknown / `E119`/`E120` arg errors).
pub fn user_func_error(error: i32, name: &str, _found_var: bool) {
    // c: the FCERR_* code selects the message; the common case is "unknown".
    let msg = match error {
        1 => format!("E119: Not enough arguments for function: {name}"),
        2 => format!("E120: Using <SID> not in a script context: {name}"),
        _ => format!("E117: Unknown function: {name}"),
    };
    semsg(&msg);
}

/// Port of `fname_trans_sid()` from `Src/eval/userfunc.c` — translate an `s:` /
/// `<SID>` function name to its `<SNR>` form. The standalone interpreter has a
/// single, unnumbered script context, so the name is returned unchanged.
pub fn fname_trans_sid(name: &str) -> String {
    name.to_string()
}

/// Port of `func_ref()` from `Src/eval/userfunc.c` — increment a function's
/// reference count; the `Rc`/registry model refcounts automatically, so no-op.
pub fn func_ref() {}

/// Port of `func_unref()` from `Src/eval/userfunc.c` — decrement a function's
/// reference count; `Rc`/`Drop`-managed, so no-op.
pub fn func_unref() {}

/// Port of `func_ptr_ref()` from `Src/eval/userfunc.c` — increment a function's
/// `uf_refcount` via a direct `ufunc_T` pointer; `Rc`-managed, no-op (mirrors
/// [`func_ref`]).
pub fn func_ptr_ref() {}

/// Port of `func_ptr_unref()` from `Src/eval/userfunc.c` — decrement a
/// function's `uf_refcount` via a direct `ufunc_T` pointer, freeing at zero;
/// `Rc`/`Drop`-managed, no-op (mirrors [`func_unref`]).
pub fn func_ptr_unref() {}

/// Port of `can_add_defer()` from `Src/eval/userfunc.c` — whether a `:defer` can
/// be registered (inside a running function). The bridge drives calls, so no
/// `:defer` stack is tracked here → false.
pub fn can_add_defer() -> bool {
    false
}

/// Port of `add_defer()` from `Src/eval/userfunc.c` — register a deferred call;
/// not tracked standalone, no-op.
pub fn add_defer() {}

/// Port of `ex_defer_inner()` from `Src/eval/userfunc.c:3397` — parse and
/// register a `:defer` call. No defer stack is tracked standalone (see
/// [`add_defer`]/[`can_add_defer`]), so this accepts and discards it → [`OK`].
pub fn ex_defer_inner() -> i32 {
    crate::ported::eval_h::OK
}

/// Port of `handle_defer_one()` from `Src/eval/userfunc.c:3487` — run one
/// deferred call while unwinding a function; nothing is deferred standalone,
/// no-op.
pub fn handle_defer_one() {}

/// Port of `invoke_all_defer()` from `Src/eval/userfunc.c:3527` — run every
/// deferred call on function exit; nothing is deferred standalone, no-op.
pub fn invoke_all_defer() {}

thread_local! {
    /// C `funccal_entry_T *funccal_stack` (`userfunc.c`) — the save-stack of
    /// suspended scopes. RUST-PORT NOTE: distinct from `vars::funccal_stack`
    /// (which models the active `current_funccal->fc_caller` chain); this stashes
    /// the whole active scope so a temporary global context can run on top.
    static SAVED_FUNCCALS: std::cell::RefCell<Vec<Vec<crate::ported::eval::vars::FuncScope>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Port of `save_funccal()` from `Src/eval/userfunc.c`.
///
/// Suspend the active function scope (so subsequent variable access resolves at
/// global/script level) by stashing it on the save-stack and clearing it.
/// Pairs with [`restore_funccal`].
pub fn save_funccal() {
    let cur =
        crate::ported::eval::vars::funccal_stack.with(|s| std::mem::take(&mut *s.borrow_mut()));
    SAVED_FUNCCALS.with(|s| s.borrow_mut().push(cur));
}

/// Port of `restore_funccal()` from `Src/eval/userfunc.c`.
///
/// Pop the save-stack, restoring the function scope suspended by the matching
/// [`save_funccal`]. Emits an internal error if called unpaired.
pub fn restore_funccal() {
    match SAVED_FUNCCALS.with(|s| s.borrow_mut().pop()) {
        Some(scope) => {
            crate::ported::eval::vars::funccal_stack.with(|s| *s.borrow_mut() = scope);
        }
        None => crate::ported::message::emsg("INTERNAL: restore_funccal()"),
    }
}

/// Port of `remove_funccal()` from `Src/eval/userfunc.c`.
///
/// Pop the innermost active function scope (the C `current_funccal =
/// fc->fc_caller; free_funccal(fc)`), exposing the caller's scope. The freed
/// scope is reclaimed by `Drop`.
pub fn remove_funccal() {
    crate::ported::eval::vars::funccal_stack.with(|s| {
        s.borrow_mut().pop();
    });
}

/// Port of `current_func_returned()` from `Src/eval/userfunc.c`.
///
/// Whether the current function exited via `:return`. RUST-PORT NOTE: the
/// bytecode bridge drives `:return`, so the `fc_returned` flag is not tracked
/// in the reduced funccal scope → false.
pub fn current_func_returned() -> bool {
    false
}

/// Port of `get_user_func_name()` from `Src/eval/userfunc.c:3075` — the `idx`-th
/// user-defined function name for command-line completion. No interactive
/// completion standalone → `None`.
pub fn get_user_func_name(_idx: i32) -> Option<String> {
    None
}

/// Port of `list_func_head()` from `Src/eval/userfunc.c:1907` — print a
/// function's `name(args)` header for `:function`. No interactive listing
/// standalone → no-op returning column 0 (mirrors the `list_*_vars` ports).
pub fn list_func_head() -> i32 {
    0
}

/// Port of `list_functions_matching_pat()` from `Src/eval/userfunc.c:2266` —
/// list user functions matching a `:function /pat` pattern. No interactive
/// listing standalone → `None`.
pub fn list_functions_matching_pat() -> Option<String> {
    None
}

/// Port of `list_one_function()` from `Src/eval/userfunc.c:2294` — print one
/// function definition for `:function {name}`. No interactive listing
/// standalone → `None`.
pub fn list_one_function() -> Option<()> {
    None
}

/// Port of `list_func_vars()` from `Src/eval/userfunc.c` — `:let` listing of the
/// `l:` scope; no interactive listing standalone, no-op (mirrors the
/// `list_glob_vars`/`list_buf_vars` ports in `vars.rs`).
pub fn list_func_vars(_first: &mut i32) {}

// ── funccall_T / ufunc_T reclamation (userfunc.c) ──
//
// RUST-PORT NOTE: these free a `funccall_T` or `ufunc_T` and everything they
// own. Under `Rc`/`Drop` ownership reclamation is automatic, so each is a no-op
// — the same basis as the ported `func_unref`/`partial_unref`/`free_unref_items`.

/// Port of `free_funccal()` (`Src/eval/userfunc.c:760`) — free a funccall_T; no-op.
pub fn free_funccal() {}

/// Port of `free_funccal_contents()` (`Src/eval/userfunc.c:782`) — free a
/// funccall_T's scope dicts; `Drop`-managed, no-op.
pub fn free_funccal_contents() {}

/// Port of `funccal_unref()` (`Src/eval/userfunc.c:869`) — decrement a
/// funccall_T's reference count, freeing at zero; `Rc`-managed, no-op.
pub fn funccal_unref() {}

/// Port of `func_clear_items()` (`Src/eval/userfunc.c:907`) — free a ufunc_T's
/// args/body/defaults; `Drop`-managed, no-op.
pub fn func_clear_items() {}

/// Port of `func_clear()` (`Src/eval/userfunc.c:927`) — clear a ufunc_T's
/// contents; `Drop`-managed, no-op.
pub fn func_clear() {}

/// Port of `func_free()` (`Src/eval/userfunc.c:943`) — free the ufunc_T struct;
/// `Drop`-managed, no-op.
pub fn func_free() {}

/// Port of `func_clear_free()` (`Src/eval/userfunc.c:958`) — clear then free a
/// ufunc_T; `Drop`-managed, no-op.
pub fn func_clear_free() {}

/// Port of `can_free_funccal()` (`Src/eval/userfunc.c`) — whether a retained
/// funccall_T may be reclaimed by a GC pass. There is no manual GC under `Rc`,
/// so nothing is ever manually freed → false.
pub fn can_free_funccal() -> bool {
    false
}

/// Port of `free_unref_funccal()` (`Src/eval/userfunc.c`) — sweep unreferenced
/// retained funccalls. No manual GC under `Rc` → nothing freed → false.
pub fn free_unref_funccal() -> bool {
    false
}

// ── GC markers over the funccall stack/functions (userfunc.c) ──
//
// RUST-PORT NOTE: garbage collection is `Rc`/`Drop`-driven, so there is no mark
// pass to run; each marker is a no-op returning false ("did not abort"),
// matching the `set_ref_in_item`/`set_ref_in_ht` precedent in `eval.rs`.

/// Port of `set_ref_in_funccal()` (`Src/eval/userfunc.c`) — GC marker over one
/// funccall's scopes; no-op (false).
pub fn set_ref_in_funccal() -> bool {
    false
}

/// Port of `set_ref_in_call_stack()` (`Src/eval/userfunc.c`) — GC marker over
/// the active call stack; no-op (false).
pub fn set_ref_in_call_stack() -> bool {
    false
}

/// Port of `set_ref_in_func_args()` (`Src/eval/userfunc.c`) — GC marker over the
/// `get_function_args`-time argument list; no-op (false).
pub fn set_ref_in_func_args() -> bool {
    false
}

/// Port of `set_ref_in_functions()` (`Src/eval/userfunc.c`) — GC marker over all
/// defined user functions; no-op (false).
pub fn set_ref_in_functions() -> bool {
    false
}

/// Port of `set_ref_in_func()` (`Src/eval/userfunc.c`) — GC marker over one
/// function's body references; no-op (false).
pub fn set_ref_in_func() -> bool {
    false
}

/// Port of `set_ref_in_previous_funccal()` (`Src/eval/userfunc.c`) — GC marker
/// over the freed-but-retained funccalls; no-op (false).
pub fn set_ref_in_previous_funccal() -> bool {
    false
}

thread_local! {
    /// `static int lambda_no = 0;` inside `get_lambda_name` (userfunc.c:271) —
    /// the monotonically increasing anonymous-lambda counter.
    static LAMBDA_NO: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
}

// ── reduced ufunc_T model + the arity/name helpers that operate on it ──
//
// RUST-PORT NOTE: the live interpreter represents user functions in the bytecode
// bridge, not here; this is a reduced `ufunc_T` (the `FuncScope`/reduced-
// `funccall_T` precedent in `vars.rs`) carrying just the fields the pure
// argument-count and name helpers below read. Function bodies, refcounts,
// scoped closures, and the K_SPECIAL `<SNR>` name encoding are out of scope.

/// `FCERR_*` (`userfunc.h:47`) — internal-call result codes. Only the values
/// the ported helpers return are modeled.
pub mod fcerr {
    /// `FCERR_UNKNOWN = 0` — also the "argument count OK" result here.
    pub const FCERR_UNKNOWN: i32 = 0;
    /// `FCERR_TOOMANY = 1` — too many arguments.
    pub const FCERR_TOOMANY: i32 = 1;
    /// `FCERR_TOOFEW = 2` — too few arguments.
    pub const FCERR_TOOFEW: i32 = 2;
    /// `FCERR_NONE = 5` — the call succeeded.
    pub const FCERR_NONE: i32 = 5;
    /// `FCERR_NOTMETHOD = 8` — the function cannot be used as a method.
    pub const FCERR_NOTMETHOD: i32 = 8;
}

/// `#define FC_ABORT 0x01` … (`userfunc.h:20`) — `uf_flags` bits.
pub mod fc {
    /// `FC_ABORT 0x01` — abort the function on error.
    pub const FC_ABORT: i32 = 0x01;
    /// `FC_RANGE 0x02` — function accepts a range.
    pub const FC_RANGE: i32 = 0x02;
    /// `FC_DICT 0x04` — Dict function, uses `self`.
    pub const FC_DICT: i32 = 0x04;
    /// `FC_CLOSURE 0x08` — closure, captures the outer scope.
    pub const FC_CLOSURE: i32 = 0x08;
}

/// Reduced `ufunc_T` (`userfunc.h`) — one user function's metadata.
#[derive(Debug, Default, Clone)]
pub struct ufunc_T {
    /// `char uf_name[]` — the function name (reduced model: a plain `String`,
    /// no K_SPECIAL `<SNR>` byte prefix).
    pub uf_name: String,
    /// `char *uf_name_exp` — the displayed name when it differs (e.g. the
    /// expanded `<SNR>123_…`), else `None`.
    pub uf_name_exp: Option<String>,
    /// `garray_T uf_args` — declared argument names (`ga_len` = count).
    pub uf_args: Vec<String>,
    /// `garray_T uf_def_args` — default-valued (optional) arguments.
    pub uf_def_args: Vec<String>,
    /// `bool uf_varargs` — declared with trailing `...`.
    pub uf_varargs: bool,
    /// `int uf_flags` — the `FC_*` bitmask.
    pub uf_flags: i32,
}

/// "Look up a user function's metadata by name → reduced `ufunc_T`" hook,
/// installed by the bridge (which owns the function registry). `None` if the
/// name is not a defined user function.
pub type FindFuncFn = fn(&str) -> Option<ufunc_T>;

thread_local! {
    /// Bridge-installed user-function lookup, backing [`find_func`].
    pub static FIND_FUNC_HOOK: std::cell::RefCell<Option<FindFuncFn>> =
        const { std::cell::RefCell::new(None) };
}

/// "Remove a user function by name → was it present" hook, installed by the
/// bridge (which owns the function registry).
pub type RemoveFuncFn = fn(&str) -> bool;

thread_local! {
    /// Bridge-installed user-function removal, backing [`func_remove`].
    pub static REMOVE_FUNC_HOOK: std::cell::RefCell<Option<RemoveFuncFn>> =
        const { std::cell::RefCell::new(None) };
}

/// Port of `find_var_in_scoped_ht()` from `Src/eval/userfunc.c` — search a
/// lambda's captured parent scope for a variable. RUST-PORT NOTE: closures
/// capture the enclosing `a:`/`l:` scope at compile time and resolve through
/// [`eval_variable`](crate::ported::eval::vars::eval_variable), so this runtime
/// parent-scope search is unwired → `None`.
pub fn find_var_in_scoped_ht() -> Option<crate::ported::eval::typval_defs_h::typval_T> {
    None
}

/// Port of `get_func_line()` from `Src/eval/userfunc.c` — the getline callback
/// feeding a function body to the source machinery. RUST-PORT NOTE: the bridge
/// executes a function from its compiled bytecode, not line-by-line, so this
/// callback is never driven → `None` (end of input).
pub fn get_func_line() -> Option<String> {
    None
}

/// Port of `set_current_funccal()` from `Src/eval/userfunc.c` — set the active
/// function-call context. RUST-PORT NOTE: the active scope is the top of the
/// `funccal_stack` Vec (managed by the bridge's call machinery and
/// [`save_funccal`]/[`restore_funccal`]); there is no separate settable
/// `current_funccal` pointer → no-op.
pub fn set_current_funccal() {}

/// Port of `cleanup_function_call()` from `Src/eval/userfunc.c` — tear down a
/// finished function call: free its locals, pop it off the call chain, run any
/// `:defer`, and retain the scope if a closure references it. RUST-PORT NOTE:
/// the bridge manages the `funccal_stack` push/pop, locals are `Drop`-freed,
/// closures are compile-time-captured, and `:defer` is not tracked — so every
/// part is handled elsewhere → no-op.
pub fn cleanup_function_call() {}

/// Port of `register_closure()` from `Src/eval/userfunc.c` — record the
/// enclosing function scope on a lambda so it can read outer `a:`/`l:` locals.
/// RUST-PORT NOTE: the bridge captures the enclosing scope at compile time, so
/// the C's runtime `uf_scoped` registration is unused → no-op.
pub fn register_closure() {}

/// Port of `func_remove()` from `Src/eval/userfunc.c` — remove user function
/// `fp` from the registry, returning whether it was present (`:delfunction` and
/// redefinition). Goes through the bridge's [`REMOVE_FUNC_HOOK`].
pub fn func_remove(fp: &ufunc_T) -> bool {
    REMOVE_FUNC_HOOK
        .with(|h| *h.borrow())
        .is_some_and(|f| f(&fp.uf_name))
}

/// Port of `find_func()` from `Src/eval/userfunc.c:712`.
///
/// Look up the user function `name`, returning its (reduced) [`ufunc_T`] or
/// `None`. RUST-PORT NOTE: the function registry lives in the bridge, which
/// installs the [`FIND_FUNC_HOOK`]; the C's `<SNR>123_` name translation is not
/// applied.
pub fn find_func(name: &str) -> Option<ufunc_T> {
    FIND_FUNC_HOOK.with(|h| *h.borrow()).and_then(|f| f(name))
}

/// Port of `alloc_ufunc()` from `Src/eval/userfunc.c:280`.
///
/// Allocate a [`ufunc_T`] for a function called `name`. RUST-PORT NOTE: only the
/// reduced fields are set; the C's `<SNR>` `uf_name_exp` derivation keys off the
/// K_SPECIAL byte encoding (not modeled — names are plain `String` here), so it
/// is left unset.
pub fn alloc_ufunc(name: &str) -> ufunc_T {
    ufunc_T {
        uf_name: name.to_string(),
        ..Default::default()
    }
}

/// Port of `call_user_func()` from `Src/eval/userfunc.c:994`.
///
/// Call user function `fp` with `argvars`, storing the result in `rettv`.
/// RUST-PORT NOTE: the call body runs in the bytecode bridge, reached here by
/// name through `CALL_FUNC_HOOK`; the C `funcexe_T`/scope-setup is owned there.
pub fn call_user_func(
    fp: &ufunc_T,
    argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) {
    use crate::ported::eval::typval_defs_h::{
        typval_T, typval_vval_union::v_string, VarLockStatus, VarType::VAR_FUNC,
    };
    let callee = typval_T {
        v_type: VAR_FUNC,
        v_lock: VarLockStatus::VAR_UNLOCKED,
        vval: v_string(fp.uf_name.clone()),
    };
    if let Some(result) = crate::ported::eval::typval::CALL_FUNC_HOOK
        .with(|h| *h.borrow())
        .and_then(|f| f(&callee, argvars))
    {
        *rettv = result;
    }
}

/// Port of `call_user_func_check()` from `Src/eval/userfunc.c:1405`.
///
/// Validate the argument count for user function `fp`, then call it. Returns an
/// `FCERR_*` code (`FCERR_NONE` on success).
pub fn call_user_func_check(
    fp: &ufunc_T,
    argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) -> i32 {
    let err = check_user_func_argcount(fp, argvars.len() as i32);
    if err != fcerr::FCERR_UNKNOWN {
        return err;
    }
    call_user_func(fp, argvars, rettv);
    fcerr::FCERR_NONE
}

/// Port of `check_user_func_argcount()` from `Src/eval/userfunc.c:1391`.
///
/// `FCERR_UNKNOWN` (OK), `FCERR_TOOFEW`, or `FCERR_TOOMANY` for calling `fp`
/// with `argcount` arguments. Required args = declared minus default-valued;
/// extra args are allowed only when the function is varargs.
pub fn check_user_func_argcount(fp: &ufunc_T, argcount: i32) -> i32 {
    let regular_args = fp.uf_args.len() as i32;
    if argcount < regular_args - fp.uf_def_args.len() as i32 {
        fcerr::FCERR_TOOFEW
    } else if !fp.uf_varargs && argcount > regular_args {
        fcerr::FCERR_TOOMANY
    } else {
        fcerr::FCERR_UNKNOWN
    }
}

/// Port of `func_is_global()` from `Src/eval/userfunc.c:722`.
///
/// Whether `ufunc` is a global (not script-local) function. RUST-PORT NOTE: the
/// C tests for a leading `K_SPECIAL` byte (the `<SNR>` encoding); the reduced
/// String-name model carries any script-local name as the literal `<SNR>…`
/// string, so this checks that prefix instead.
pub fn func_is_global(ufunc: &ufunc_T) -> bool {
    !ufunc.uf_name.starts_with("<SNR>")
}

/// Port of `cat_func_name()` from `Src/eval/userfunc.c:731`.
///
/// The displayable name of `fp` for messages/`:function`. RUST-PORT NOTE: the C
/// rewrites a script-local name's `K_SPECIAL` prefix to `<SNR>`; here the name
/// is already in display form, so it is returned as-is.
pub fn cat_func_name(fp: &ufunc_T) -> String {
    fp.uf_name.clone()
}

/// Port of `printable_func_name()` from `Src/eval/userfunc.c:1886`.
///
/// The name to show for `fp`: the expanded `uf_name_exp` when present, else the
/// raw `uf_name`.
pub fn printable_func_name(fp: &ufunc_T) -> &str {
    fp.uf_name_exp.as_deref().unwrap_or(&fp.uf_name)
}

/// Port of `get_func_arity()` from `Src/eval/userfunc.c:671`.
///
/// Resolve the `(required, optional, varargs)` arity of function `name`. A
/// builtin is looked up in the generated `BUILTIN_ARGC` table (never varargs in
/// the C sense — the table caps it); otherwise the reduced `ufunc_T` in `fp` is
/// used. Returns `None` (the C `FAIL`) when `name` is neither a known builtin
/// nor backed by a supplied `ufunc_T`.
pub fn get_func_arity(name: &str, fp: Option<&ufunc_T>) -> Option<(i32, i32, bool)> {
    use crate::ported::eval::funcs_argc::BUILTIN_ARGC;
    if let Ok(i) = BUILTIN_ARGC.binary_search_by(|e| e.0.cmp(name)) {
        let (_, min_argc, max_argc) = BUILTIN_ARGC[i];
        return Some((min_argc as i32, (max_argc - min_argc) as i32, false));
    }
    let f = fp?;
    let argcount = f.uf_args.len() as i32;
    let min_argcount = argcount - f.uf_def_args.len() as i32;
    Some((min_argcount, argcount - min_argcount, f.uf_varargs))
}

/// Port of `get_lambda_name()` from `Src/eval/userfunc.c:269`.
///
/// The next generated lambda name, `"<lambda>N"` with `N` a process-wide
/// counter incremented on each call (so successive lambdas get `<lambda>1`,
/// `<lambda>2`, …).
pub fn get_lambda_name() -> String {
    LAMBDA_NO.with(|n| {
        let next = n.get() + 1;
        n.set(next);
        format!("<lambda>{next}")
    })
}

/// Port of `get_func_tv()` from `Src/eval/userfunc.c:551`.
///
/// Evaluate a function call `name(args)` — parse and evaluate the argument list
/// `args` ([`get_func_arguments`]), then call `name` ([`call_func`]) into
/// `rettv`. Returns [`OK`](crate::ported::eval_h::OK)/[`FAIL`](crate::ported::eval_h::FAIL).
/// RUST-PORT NOTE: the C advances a parse pointer over `name(...)` and threads a
/// `funcexe_T`; here the already-isolated argument text is passed in.
pub fn get_func_tv(
    name: &str,
    args: &str,
    rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) -> i32 {
    match get_func_arguments(args) {
        Some(argvars) => call_func(name, &argvars, rettv),
        None => crate::ported::eval_h::FAIL,
    }
}

/// Port of `get_func_arguments()` from `Src/eval/userfunc.c:510`.
///
/// Evaluate the comma-separated call arguments in `args` (the content between a
/// call's parentheses), returning the resulting values. Returns `None` if any
/// argument fails to evaluate. RUST-PORT NOTE: the C advances a parse pointer
/// and tracks a partial-arg offset; here the list is split on top-level commas
/// (balancing brackets, skipping strings) and each is run via `EVAL_STRING_HOOK`.
pub fn get_func_arguments(args: &str) -> Option<Vec<crate::ported::eval::typval_defs_h::typval_T>> {
    let parts = {
        let b = args.as_bytes();
        let mut out: Vec<&str> = Vec::new();
        let (mut start, mut depth, mut i) = (0usize, 0i32, 0usize);
        while i < b.len() {
            match b[i] {
                b'\'' => {
                    i += 1;
                    while i < b.len() && b[i] != b'\'' {
                        i += 1;
                    }
                }
                b'"' => {
                    i += 1;
                    while i < b.len() && b[i] != b'"' {
                        if b[i] == b'\\' && i + 1 < b.len() {
                            i += 1;
                        }
                        i += 1;
                    }
                }
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b',' if depth == 0 => {
                    out.push(&args[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
            i += 1;
        }
        out.push(&args[start..]);
        out
    };
    let mut argvars = Vec::new();
    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let v = crate::ported::eval::typval::EVAL_STRING_HOOK
            .with(|h| *h.borrow())
            .and_then(|f| f(part))?;
        argvars.push(v);
    }
    Some(argvars)
}

/// Port of `get_function_args()` from `Src/eval/userfunc.c:149`.
///
/// Parse a `:function`/lambda parameter list `params` (the content between the
/// parentheses): comma-separated names (validated by [`one_function_arg`]),
/// optional `name = default` defaults (stored as the default expression source),
/// and a trailing `...` for varargs. Returns `(names, default_exprs, varargs)`,
/// or `None` on an invalid parameter. RUST-PORT NOTE: the C threads an advancing
/// pointer and locates default-expression ends via `eval1`; here the list is
/// split on top-level commas (balancing `()`/`[]`/`{}`, skipping strings).
pub fn get_function_args(params: &str) -> Option<(Vec<String>, Vec<String>, bool)> {
    // Split on commas that are not nested in brackets or inside a string.
    let parts = {
        let b = params.as_bytes();
        let mut out: Vec<&str> = Vec::new();
        let (mut start, mut depth, mut i) = (0usize, 0i32, 0usize);
        while i < b.len() {
            match b[i] {
                b'\'' => {
                    i += 1;
                    while i < b.len() && b[i] != b'\'' {
                        i += 1;
                    }
                }
                b'"' => {
                    i += 1;
                    while i < b.len() && b[i] != b'"' {
                        if b[i] == b'\\' && i + 1 < b.len() {
                            i += 1;
                        }
                        i += 1;
                    }
                }
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b',' if depth == 0 => {
                    out.push(&params[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
            i += 1;
        }
        out.push(&params[start..]);
        out
    };

    let mut names: Vec<String> = Vec::new();
    let mut defaults: Vec<String> = Vec::new();
    let mut varargs = false;
    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if part == "..." {
            varargs = true;
            continue;
        }
        if let Some(eq) = part.find('=') {
            let name = part[..eq].trim();
            if one_function_arg(name, Some(&mut names), false) != name.len() {
                return None;
            }
            defaults.push(part[eq + 1..].trim().to_string());
        } else if one_function_arg(part, Some(&mut names), false) != part.len() {
            return None;
        }
    }
    Some((names, defaults, varargs))
}

/// Port of `one_function_arg()` from `Src/eval/userfunc.c:109`.
///
/// Parse one function parameter name (`[A-Za-z0-9_]+`, not digit-leading, not
/// the reserved `firstline`/`lastline`) at the start of `arg`. On success
/// returns the byte length consumed and, when `newargs` is `Some`, appends the
/// name (rejecting duplicates). On any error returns `0` (the C "no advance",
/// i.e. returns the original pointer), emitting the message unless `skip`.
pub fn one_function_arg(arg: &str, newargs: Option<&mut Vec<String>>, skip: bool) -> usize {
    let b = arg.as_bytes();
    let mut p = 0;
    while p < b.len() && (b[p].is_ascii_alphanumeric() || b[p] == b'_') {
        p += 1;
    }
    let name = &arg[..p];
    if p == 0 || b[0].is_ascii_digit() || name == "firstline" || name == "lastline" {
        if !skip {
            semsg(&format!("E125: Illegal argument: {arg}"));
        }
        return 0;
    }
    if let Some(args) = newargs {
        // Duplicate-name check (the C emits E853 regardless of `skip`).
        if args.iter().any(|a| a == name) {
            semsg(&format!("E853: Duplicate argument name: {name}"));
            return 0;
        }
        args.push(name.to_string());
    }
    p
}

/// Port of `argv_add_base()` from `Src/eval/userfunc.c:1641`.
///
/// For a method call `base->Method(args…)`, prepend `basetv` to `argvars` and
/// report the new argv plus the `argv_base` offset (`1` when a base was added,
/// else `0`). With no base the arguments are returned unchanged.
pub fn argv_add_base(
    basetv: Option<crate::ported::eval::typval_defs_h::typval_T>,
    argvars: &[crate::ported::eval::typval_defs_h::typval_T],
) -> (Vec<crate::ported::eval::typval_defs_h::typval_T>, i32) {
    match basetv {
        Some(base) => {
            let mut new_argvars = Vec::with_capacity(argvars.len() + 1);
            new_argvars.push(base);
            new_argvars.extend_from_slice(argvars);
            (new_argvars, 1)
        }
        None => (argvars.to_vec(), 0),
    }
}

/// Port of `call_func()` from `Src/eval/userfunc.c:1667`.
///
/// Call function `funcname` with `argvars`, storing the result in `rettv`;
/// returns [`OK`](crate::ported::eval_h::OK)/[`FAIL`](crate::ported::eval_h::FAIL).
/// This is the low-level dispatcher [`func_call`]/`call_vim_function` build on.
/// RUST-PORT NOTE: dispatch goes through the bridge's `CALL_FUNC_HOOK`; the C
/// `funcexe_T` (bound partial, self dict, first/last line) is not modeled.
pub fn call_func(
    funcname: &str,
    argvars: &[crate::ported::eval::typval_defs_h::typval_T],
    rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) -> i32 {
    use crate::ported::eval::typval_defs_h::{
        typval_T, typval_vval_union::v_string, VarLockStatus, VarType::VAR_FUNC,
    };
    use crate::ported::eval_h::{FAIL, OK};
    let callee = typval_T {
        v_type: VAR_FUNC,
        v_lock: VarLockStatus::VAR_UNLOCKED,
        vval: v_string(funcname.to_string()),
    };
    match crate::ported::eval::typval::CALL_FUNC_HOOK
        .with(|h| *h.borrow())
        .and_then(|f| f(&callee, argvars))
    {
        Some(result) => {
            *rettv = result;
            OK
        }
        None => FAIL,
    }
}

/// Port of `func_call()` from `Src/eval/userfunc.c:1554`.
///
/// Call function `name` with the arguments in the List `args`, optionally as a
/// `partial` (whose bound args are honored) and/or bound to `selfdict`. Returns
/// [`OK`](crate::ported::eval_h::OK)/[`FAIL`](crate::ported::eval_h::FAIL).
/// RUST-PORT NOTE: dispatch goes through the bridge's `CALL_FUNC_HOOK`; the
/// `selfdict` self-binding and the `MAX_FUNC_ARGS`/E699 guard are not modeled.
pub fn func_call(
    name: &str,
    args: &crate::ported::eval::typval_defs_h::typval_T,
    partial: Option<&std::rc::Rc<crate::ported::eval::typval_defs_h::partial_T>>,
    _selfdict: Option<&std::rc::Rc<std::cell::RefCell<crate::ported::eval::typval_defs_h::dict_T>>>,
    rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) -> i32 {
    use crate::ported::eval::typval_defs_h::{
        typval_T, typval_vval_union::*, VarLockStatus, VarType::*,
    };
    use crate::ported::eval_h::{FAIL, OK};
    let argv: Vec<typval_T> = match &args.vval {
        v_list(Some(l)) => l
            .borrow()
            .lv_items
            .iter()
            .map(|it| it.li_tv.clone())
            .collect(),
        _ => return FAIL,
    };
    let callee = match partial {
        Some(p) => typval_T {
            v_type: VAR_PARTIAL,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_partial(Some(p.clone())),
        },
        None => typval_T {
            v_type: VAR_FUNC,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_string(name.to_string()),
        },
    };
    match crate::ported::eval::typval::CALL_FUNC_HOOK
        .with(|h| *h.borrow())
        .and_then(|f| f(&callee, &argv))
    {
        Some(result) => {
            *rettv = result;
            OK
        }
        None => FAIL,
    }
}

/// Port of `add_nr_var()` from `Src/eval/userfunc.c:749`.
///
/// Add a read-only Number variable `name = nr` to dict `dp` (used to populate
/// the `a:`/funccall info dicts). RUST-PORT NOTE: the C sets `DI_FLAGS_RO |
/// DI_FLAGS_FIX` on the item; the `IndexMap`-backed dict has no per-item flags,
/// so the value is added via the ported [`tv_dict_add_nr`].
pub fn add_nr_var(
    dp: &mut crate::ported::eval::typval_defs_h::dict_T,
    name: &str,
    nr: crate::ported::eval::typval_defs_h::varnumber_T,
) {
    crate::ported::eval::typval::tv_dict_add_nr(dp, name, nr);
}

/// Port of `func_has_ended()` from `Src/eval/userfunc.c:3787` — whether a
/// function call should stop (aborted on error, or `:return`ed). RUST-PORT NOTE:
/// the bridge drives call termination, so this is not tracked here → false.
pub fn func_has_ended() -> bool {
    false
}

/// Port of `func_has_abort()` from `Src/eval/userfunc.c:3798` — whether the
/// function aborts on error (`FC_ABORT`); not tracked standalone → false.
pub fn func_has_abort() -> bool {
    false
}

/// Port of `get_return_cmd()` from `Src/eval/userfunc.c`.
///
/// Format the debugger's `:return <value>` line for a `:return`ed value (or just
/// `:return` for none), truncating to the I/O buffer size with a trailing
/// `...`. Uses the `string()`-style echo encoding.
pub fn get_return_cmd(rettv: Option<&crate::ported::eval::typval_defs_h::typval_T>) -> String {
    const IOSIZE: usize = 1024 + 1;
    let s = rettv.map_or(String::new(), crate::ported::eval::encode::encode_tv2echo);
    let mut buf = format!(":return {s}");
    if buf.len() >= IOSIZE {
        let mut cut = IOSIZE - 4;
        while !buf.is_char_boundary(cut) {
            cut -= 1;
        }
        buf.truncate(cut);
        buf.push_str("...");
    }
    buf
}

// ── current-funccall scope accessors (userfunc.c) ──
//
// RUST-PORT NOTE: the C returns a live `dict_T*`/`hashtab_T*` into the current
// function-call scope (for the GC/listing/debugger to read or mark). The
// `Vec<FuncScope>` model can't hand out a borrow across the thread-local, so
// these return a read-snapshot clone of the innermost active scope (`None` when
// not inside a function). Mutations do not flow back — they are read accessors.

/// Port of `get_funccal_local_var()` from `Src/eval/userfunc.c` — the `l:`
/// scope's self-variable, i.e. `l:` evaluated as a Dict. RUST-PORT NOTE: the C
/// returns the scope's self-`dictitem_T` (whose value is the live scope Dict);
/// this returns that value as a `VAR_DICT` read-snapshot (consistent with
/// `eval_variable("l:")`), or `None` when not in a function.
pub fn get_funccal_local_var() -> Option<crate::ported::eval::typval_defs_h::typval_T> {
    use crate::ported::eval::typval_defs_h::{
        typval_T, typval_vval_union::v_dict, VarLockStatus, VarType::VAR_DICT,
    };
    let nd = crate::ported::eval::typval::tv_dict_alloc();
    nd.borrow_mut().dv_hashtab = get_funccal_local_dict()?.dv_hashtab;
    Some(typval_T {
        v_type: VAR_DICT,
        v_lock: VarLockStatus::VAR_UNLOCKED,
        vval: v_dict(Some(nd)),
    })
}

/// Port of `get_funccal_args_var()` from `Src/eval/userfunc.c` — the `a:` scope's
/// self-variable (`a:` as a Dict read-snapshot), or `None`.
pub fn get_funccal_args_var() -> Option<crate::ported::eval::typval_defs_h::typval_T> {
    use crate::ported::eval::typval_defs_h::{
        typval_T, typval_vval_union::v_dict, VarLockStatus, VarType::VAR_DICT,
    };
    let nd = crate::ported::eval::typval::tv_dict_alloc();
    nd.borrow_mut().dv_hashtab = get_funccal_args_dict()?.dv_hashtab;
    Some(typval_T {
        v_type: VAR_DICT,
        v_lock: VarLockStatus::VAR_UNLOCKED,
        vval: v_dict(Some(nd)),
    })
}

/// Port of `get_funccal_local_dict()` from `Src/eval/userfunc.c` — the current
/// `l:` scope dict (read-snapshot), or `None` when not in a function.
pub fn get_funccal_local_dict() -> Option<crate::ported::eval::typval_defs_h::dict_T> {
    crate::ported::eval::vars::funccal_stack
        .with(|s| s.borrow().last().map(|f| f.fc_l_vars.clone()))
}

/// Port of `get_funccal_args_dict()` from `Src/eval/userfunc.c` — the current
/// `a:` argument scope dict (read-snapshot), or `None`.
pub fn get_funccal_args_dict() -> Option<crate::ported::eval::typval_defs_h::dict_T> {
    crate::ported::eval::vars::funccal_stack
        .with(|s| s.borrow().last().map(|f| f.fc_l_avars.clone()))
}

/// Port of `get_funccal_local_ht()` from `Src/eval/userfunc.c` — the hashtable
/// of the current `l:` scope (read-snapshot), or `None`.
pub fn get_funccal_local_ht(
) -> Option<indexmap::IndexMap<String, crate::ported::eval::typval_defs_h::typval_T>> {
    get_funccal_local_dict().map(|d| d.dv_hashtab)
}

/// Port of `get_funccal_args_ht()` from `Src/eval/userfunc.c` — the hashtable of
/// the current `a:` scope (read-snapshot), or `None`.
pub fn get_funccal_args_ht(
) -> Option<indexmap::IndexMap<String, crate::ported::eval::typval_defs_h::typval_T>> {
    get_funccal_args_dict().map(|d| d.dv_hashtab)
}

/// Port of `func_name()` from `Src/eval/userfunc.c:3875` — the name of the
/// function a call activation belongs to. RUST-PORT NOTE: the C reads a
/// `funccall_T` cookie's `fc_func->uf_name`; the reduced model returns the
/// innermost active [`FuncScope`](crate::ported::eval::vars::FuncScope)'s
/// `fc_name` (empty when not inside a function).
pub fn func_name() -> String {
    crate::ported::eval::vars::funccal_stack
        .with(|s| s.borrow().last().map(|f| f.fc_name.clone()))
        .unwrap_or_default()
}

/// Port of `func_breakpoint()` from `Src/eval/userfunc.c:3881` — the next
/// debugger breakpoint line for a function call. No debugger standalone (no
/// breakpoints) → 0. RUST-PORT NOTE: the C returns a mutable `linenr_T*` the
/// debugger updates; the reduced model returns the (always-absent) value.
pub fn func_breakpoint() -> i64 {
    0
}

/// Port of `func_dbg_tick()` from `Src/eval/userfunc.c:3887` — the debug tick a
/// function call was entered with. No debugger standalone → 0.
pub fn func_dbg_tick() -> i32 {
    0
}

/// Port of `func_level()` from `Src/eval/userfunc.c` — the function-call nesting
/// level. RUST-PORT NOTE: the C reads a specific funccall cookie's `fc_level`;
/// the reduced model reports the current active-scope depth (`funccal_stack`).
pub fn func_level() -> i32 {
    crate::ported::eval::vars::funccal_stack.with(|s| s.borrow().len() as i32)
}

/// Port of `fc_referenced()` from `Src/eval/userfunc.c:3268` — whether a
/// funccall is still referenced (for GC). No manual GC under `Rc` → false.
pub fn fc_referenced() -> bool {
    false
}

/// Port of `callback_call_retnr()` from `Src/eval/userfunc.c:1593`.
///
/// Invoke `callback` with `argvars` and return its result as a Number, or `-2`
/// when the call did not happen (the C sentinel).
pub fn callback_call_retnr(
    callback: &crate::ported::eval::typval::Callback,
    argvars: &[crate::ported::eval::typval_defs_h::typval_T],
) -> crate::ported::eval::typval_defs_h::varnumber_T {
    let mut rettv = crate::ported::eval::typval_defs_h::typval_T::from(0);
    if !crate::ported::eval::callback_call(callback, argvars, &mut rettv) {
        return -2;
    }
    crate::ported::eval::typval::tv_get_number_chk(&rettv, None)
}

/// Port of `translated_function_exists()` from `Src/eval/userfunc.c:3038`.
///
/// Whether a function `name` (already name-translated) exists: a builtin is
/// looked up in the generated `BUILTIN_ARGC` table (`find_internal_func`), a
/// user function via the interpreter's function-existence hook (`find_func`).
pub fn translated_function_exists(name: &str) -> bool {
    if builtin_function(name, -1) {
        crate::ported::eval::funcs_argc::BUILTIN_ARGC
            .binary_search_by(|e| e.0.cmp(name))
            .is_ok()
    } else {
        crate::ported::eval::typval::FUNC_EXISTS_HOOK
            .with(|h| *h.borrow())
            .is_some_and(|f| f(name))
    }
}

/// Port of `function_exists()` from `Src/eval/userfunc.c:3052`.
///
/// Whether a function with the given `name` exists, dereferencing a Funcref
/// variable first unless `no_deref`. RUST-PORT NOTE: the C runs
/// `trans_function_name` to strip sigils/`<SNR>`/trailing whitespace; the subset
/// dereferences via [`deref_func_name`] and checks the name as-is.
pub fn function_exists(name: &str, no_deref: bool) -> bool {
    let resolved = if no_deref {
        name.to_string()
    } else {
        deref_func_name(name, true).name
    };
    translated_function_exists(&resolved)
}

/// Resolved [`deref_func_name`] result: the function name to call, the bound
/// partial if the variable held one, and whether a variable was found.
pub struct DerefedFunc {
    /// The resolved function name (the variable's Funcref/Partial name, or the
    /// original name when no Funcref variable shadows it).
    pub name: String,
    /// The partial when the variable held a `VAR_PARTIAL`, else `None`.
    pub partial: Option<std::rc::Rc<crate::ported::eval::typval_defs_h::partial_T>>,
    /// True when a variable of this name exists.
    pub found_var: bool,
}

/// Port of `deref_func_name()` from `Src/eval/userfunc.c:445`.
///
/// If a variable `name` holds a Funcref or Partial, resolve it to the function
/// name it refers to (and the bound partial); otherwise return `name` itself.
/// RUST-PORT NOTE: the C reads a `dictitem_T` via `find_var`; the subset
/// resolves through [`eval_variable`]. `no_autoload` is accepted for signature
/// fidelity (no autoload standalone).
pub fn deref_func_name(name: &str, _no_autoload: bool) -> DerefedFunc {
    use crate::ported::eval::typval_defs_h::{typval_vval_union::*, VarType::*};
    match crate::ported::eval::vars::eval_variable(name) {
        None => DerefedFunc {
            name: name.to_string(),
            partial: None,
            found_var: false,
        },
        Some(tv) => match (tv.v_type, tv.vval) {
            (VAR_FUNC, v_string(s)) => DerefedFunc {
                name: s,
                partial: None,
                found_var: true,
            },
            (VAR_PARTIAL, v_partial(Some(pt))) => DerefedFunc {
                name: crate::ported::eval::partial_name(&pt).to_string(),
                partial: Some(pt),
                found_var: true,
            },
            _ => DerefedFunc {
                name: name.to_string(),
                partial: None,
                found_var: true,
            },
        },
    }
}

// ── funccall_T pointer chain (userfunc.c) ──
//
// RUST-PORT NOTE: the C `funccall_T` is an intrusive, heap-allocated node linked
// through `fc_caller` and pointed at by the file-static `current_funccal`
// (userfunc.c). The port maps the intrusive `funccall_T*` chain to
// `Rc<RefCell<funccall_T>>` links and the `current_funccal` file-static to a
// `thread_local`, the same convention `list_T`/`dict_T` and the other
// `src/ported` refcounted-pointer ports use. This chain is the faithful port of
// the C call-activation stack the bridge maintains for `:return`/backtrace; it
// is distinct from `vars::funccal_stack` (the reduced `FuncScope` model that
// backs `l:`/`a:` variable *resolution*). Only the fields the ported call
// machinery reads are modeled; profiling, defer, GC copyIDs, `fc_fixvar`, the
// `a:000` list, breakpoint/level bookkeeping, and refcounts are omitted (the
// last three are `Rc`/`Drop`-managed — see the `free_funccal`/`funccal_unref`
// no-ops above).

/// Reduced `struct funccall_S` (`typval_defs.h:299`) — one user-function call
/// activation.
#[derive(Debug, Default, Clone)]
pub struct funccall_T {
    /// `ufunc_T *fc_func` — function being called.
    pub fc_func: ufunc_T,
    /// `int fc_returned` — `":return"` used. RUST-PORT NOTE: `bool` here.
    pub fc_returned: bool,
    /// `dict_T fc_l_vars` — `l:` local function variables.
    pub fc_l_vars: crate::ported::eval::typval_defs_h::dict_T,
    /// `dict_T fc_l_avars` — `a:` argument variables.
    pub fc_l_avars: crate::ported::eval::typval_defs_h::dict_T,
    /// `typval_T *fc_rettv` — return value (`None` until set by [`do_return`]).
    pub fc_rettv: Option<crate::ported::eval::typval_defs_h::typval_T>,
    /// `funccall_T *fc_caller` — calling function or `None`.
    pub fc_caller: Option<std::rc::Rc<std::cell::RefCell<funccall_T>>>,
}

thread_local! {
    /// C `funccall_T *current_funccal` (`userfunc.c`) — the innermost active
    /// call activation, head of the `fc_caller` chain.
    static current_funccal: std::cell::RefCell<Option<std::rc::Rc<std::cell::RefCell<funccall_T>>>> =
        const { std::cell::RefCell::new(None) };
}

/// Port of `create_funccal()` from `Src/eval/userfunc.c:966`.
///
/// Allocate a `funccall_T`, link it in `current_funccal` and fill in `fp` and
/// `rettv`. Must be followed by one call to
/// [`remove_funccal`]/`cleanup_function_call`.
pub fn create_funccal(
    fp: &ufunc_T,
    rettv: Option<crate::ported::eval::typval_defs_h::typval_T>,
) -> std::rc::Rc<std::cell::RefCell<funccall_T>> {
    let fc = std::rc::Rc::new(std::cell::RefCell::new(funccall_T {
        fc_caller: current_funccal.with(|c| c.borrow().clone()), // c:969
        fc_func: fp.clone(),                                     // c:971
        fc_rettv: rettv,                                         // c:973
        ..Default::default()
    }));
    current_funccal.with(|c| *c.borrow_mut() = Some(fc.clone())); // c:970
    func_ptr_ref(); // c:972 (Rc-managed no-op)
    fc
}

/// Port of `get_current_funccal()` from `Src/eval/userfunc.c:1453`.
pub fn get_current_funccal() -> Option<std::rc::Rc<std::cell::RefCell<funccall_T>>> {
    current_funccal.with(|c| c.borrow().clone())
}

/// Port of `get_funccal()` from `Src/eval/userfunc.c:3929`.
///
/// Get function call environment based on backtrace debug level. RUST-PORT NOTE:
/// there is no debugger standalone, so `debug_backtrace_level == 0` and the
/// `fc_caller` walk is skipped — the current funccall is returned.
pub fn get_funccal() -> Option<std::rc::Rc<std::cell::RefCell<funccall_T>>> {
    let funccal = current_funccal.with(|c| c.borrow().clone()); // c:3931
                                                                // c:3932 debug_backtrace_level is always 0 here (no debugger).
    funccal
}

/// Port of `get_current_funccal_dict()` from `Src/eval/userfunc.c:4014`.
///
/// If `ht` is the hashtable for local variables in the current funccal, return
/// the dict that contains it, otherwise `None`. RUST-PORT NOTE: the C compares
/// `ht` by pointer identity against `&current_funccal->fc_l_vars.dv_hashtab`;
/// the `IndexMap`-backed dict has no stable address to compare, so the current
/// funccal's `l:` dict (read-snapshot) is returned whenever inside a function
/// (its sole caller passes the `l:` ht).
pub fn get_current_funccal_dict(
    _ht: &indexmap::IndexMap<String, crate::ported::eval::typval_defs_h::typval_T>,
) -> Option<crate::ported::eval::typval_defs_h::dict_T> {
    current_funccal.with(|c| c.borrow().as_ref().map(|fc| fc.borrow().fc_l_vars.clone()))
}

/// Port of `do_return()` from `Src/eval/userfunc.c:3641`.
///
/// Return from a function. Sets `fc_returned` on the current funccall and stores
/// the return value. Returns `true` when the return can be carried out.
/// RUST-PORT NOTE: the pending-return path uses `cstack_T`/`cleanup_conditionals`
/// (the `:try` conditional stack from `ex_eval.c`) reached via `exarg_T` — the
/// ex-command dispatch subsystem, not present in the standalone interpreter. With
/// no `cstack`, `cleanup_conditionals()` yields `idx < 0` (no pending return), so
/// the return is always carried out immediately; `eap`/`is_cmd` are dropped from
/// the signature and `reanimate` is kept for fidelity.
pub fn do_return(
    reanimate: bool,
    _is_cmd: bool,
    rettv: Option<crate::ported::eval::typval_defs_h::typval_T>,
) -> bool {
    current_funccal.with(|c| {
        if let Some(fc) = c.borrow().as_ref() {
            let mut fc = fc.borrow_mut();
            if reanimate {
                fc.fc_returned = false; // c:3647 Undo the return.
            }
            // c:3654 cleanup_conditionals() -> idx < 0 (no cstack): else branch.
            fc.fc_returned = true; // c:3689
                                   // c:3694 store the return value.
            if !reanimate {
                if let Some(v) = rettv {
                    fc.fc_rettv = Some(v);
                }
            }
        }
    });
    // c:3703 return idx < 0 (always true here).
    true
}

/// Port of `find_hi_in_scoped_ht()` from `Src/eval/userfunc.c:4023`.
///
/// Search a hashitem in a lambda's captured parent scope. RUST-PORT NOTE: the C
/// walks `current_funccal->fc_func->uf_scoped`, the closure's runtime
/// parent-`funccall_T` chain; the reduced `ufunc_T` does not model `uf_scoped`
/// (closures capture their enclosing scope at compile time in the bridge), so
/// `fc_func->uf_scoped` is always absent → `None` (mirrors [`find_var_in_scoped_ht`]).
pub fn find_hi_in_scoped_ht(_name: &str) -> Option<crate::ported::eval::typval_defs_h::typval_T> {
    // c:4025 current_funccal == NULL || fc_func->uf_scoped == NULL -> return NULL.
    None
}

// ── :function / :call / :return / :delfunction command drivers (userfunc.c) ──
//
// RUST-PORT NOTE: these `exarg_T` command drivers are faithful strict REFERENCE
// ports. At runtime the bytecode frontend (`viml_parser.rs`/`compile_viml.rs`)
// supersedes them, exactly as the `eval0..eval7` tree-walker was ported in wave
// 1 as a reference. The `exarg_T` struct is reduced to the fields these drivers
// read; the ex_docmd source-line getter (`eap->ea_getline`/`cookie`) and the
// `:try` conditional stack (`eap->cstack`) have no standalone counterpart, so
// the drivers are ported up to those calls and the getline extern is an honest
// deferred dep (see [`get_function_body`]).

/// `#define K_SPECIAL (0x80)` (`keycodes.h:19`) — first byte of a special-key
/// sequence; the `<SNR>` name encoding leads with it.
const K_SPECIAL: u8 = 0x80;
/// `#define KS_EXTRA 253` (`keycodes.h:41`) — second byte of a `KS_EXTRA` key.
const KS_EXTRA: u8 = 253;
/// `KE_SNR = 82` (`keycodes.h:196`) — the `<SNR>` extra-key code.
const KE_SNR: u8 = 82;

/// `TransFunctionNameFlags` (`eval.h:84`) — `trans_function_name()` flags.
pub mod tfn {
    /// `TFN_INT = 1` — may use an internal (builtin) function name.
    pub const TFN_INT: i32 = 1;
    /// `TFN_QUIET = 2` — do not emit error messages.
    pub const TFN_QUIET: i32 = 2;
    /// `TFN_NO_AUTOLOAD = 4` — do not use script autoloading.
    pub const TFN_NO_AUTOLOAD: i32 = 4;
    /// `TFN_NO_DEREF = 8` — do not dereference a Funcref.
    pub const TFN_NO_DEREF: i32 = 8;
    /// `TFN_READ_ONLY = 16` — caller will not change the variable.
    pub const TFN_READ_ONLY: i32 = 16;
}

/// `typedef struct { … } funcdict_T;` (`csrc/eval/userfunc.h:35`) — the Dict
/// context filled by [`trans_function_name`] for a `dict.func` target.
///
/// RUST-PORT NOTE: `fd_dict` (a `dict_T *`) becomes `Rc<RefCell<dict_T>>` and
/// `fd_di` (a `dictitem_T *`) becomes the item key (the `IndexMap` model has no
/// `dictitem_T`), matching the [`lval_T`](crate::ported::eval::lval_T) mapping.
#[derive(Debug, Default, Clone)]
pub struct funcdict_T {
    /// `dict_T *fd_dict` — Dict used.
    pub fd_dict: Option<std::rc::Rc<std::cell::RefCell<crate::ported::eval::typval_defs_h::dict_T>>>,
    /// `char *fd_newkey` — new key in `dict`.
    pub fd_newkey: Option<String>,
    /// `dictitem_T *fd_di` — Dict item used (key stands in for the pointer).
    pub fd_di: Option<String>,
}

/// Reduced `cmdidx_T` (`ex_cmds_defs.h`, not vendored) — only the command
/// indices the ported drivers compare against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum cmdidx_T {
    /// `CMD_call` — the `:call` command.
    #[default]
    CMD_call,
    /// `CMD_defer` — the `:defer` command.
    CMD_defer,
}

/// Reduced `struct exarg` (`ex_cmds_defs.h:108`, not vendored) — only the fields
/// the ported `:function`/`:call`/`:return`/`:delfunction` drivers read.
///
/// RUST-PORT NOTE: `ea_getline`/`cookie` (the ex_docmd source-line reader) and
/// `cstack` (the `:try` conditional stack from `ex_eval.c`) have no standalone
/// counterpart; the drivers are ported up to their use and those externs are
/// honest deferred deps.
#[derive(Debug, Default, Clone)]
pub struct exarg_T {
    /// `char *arg` — argument of the command.
    pub arg: String,
    /// `char *cmd` — the name of the command.
    pub cmd: String,
    /// `char *nextcmd` — next command (`None` if none).
    pub nextcmd: Option<String>,
    /// `cmdidx_T cmdidx` — the index for the command.
    pub cmdidx: cmdidx_T,
    /// `int skip` — don't execute the command, only parse it.
    pub skip: bool,
    /// `int forceit` — true if `!` present.
    pub forceit: bool,
    /// `int addr_count` — the number of addresses given.
    pub addr_count: i32,
    /// `linenr_T line1` — the first line number.
    pub line1: i64,
    /// `linenr_T line2` — the second line number or count.
    pub line2: i64,
}

/// Port of `func_tbl_get()` from `csrc/eval/userfunc.c:101`.
///
/// Return the function hash table. RUST-PORT NOTE: the C returns
/// `&func_hashtab`, the module-static registry; here the user-function registry
/// lives in the bytecode bridge (reached via [`FIND_FUNC_HOOK`]/
/// [`REMOVE_FUNC_HOOK`]), so there is no standalone `hashtab_T` to hand back —
/// this is a no-op accessor kept for call-site fidelity (mirrors [`func_init`]).
pub fn func_tbl_get() {}

/// Port of `trans_function_name()` from `csrc/eval/userfunc.c:1981`.
///
/// Parse the function name at `*pp` (a plain name, `s:`/`<SID>`/`<SNR>` local,
/// `dict.func`, or a Funcref variable), advancing `*pp` past it and returning
/// the resolved name (or `None` on error / when it is a Dict-key target). Fills
/// `fdp` for a `dict.func` target and `partial` when the variable held one.
/// Uses [`get_lval`](crate::ported::eval::get_lval) to walk `dict.key`/subscripts.
///
/// RUST-PORT NOTE: the C rewrites a leading `<SNR>` to the 3-byte
/// `K_SPECIAL`/`KS_EXTRA`/`KE_SNR` sequence and expands `s:`/`<SID>` with the
/// current script id; the reduced String model keeps the literal `<SNR>` prefix
/// (see [`func_is_global`]) and has a single unnumbered script context — so an
/// `s:`/`<SID>` *definition* errors `E81` (like [`get_scriptlocal_funcname`]).
/// `aborting()` is always false here (matching the [`eval0`](crate::ported::eval::eval0)
/// port), so the aborting branches are never taken.
pub fn trans_function_name(
    pp: &mut &str,
    skip: bool,
    flags: i32,
    mut fdp: Option<&mut funcdict_T>,
    mut partial: Option<&mut Option<std::rc::Rc<crate::ported::eval::typval_defs_h::partial_T>>>,
) -> Option<String> {
    use crate::ported::eval::typval_defs_h::{typval_vval_union::*, VarType::*};
    use crate::ported::eval::{
        check_luafunc_name, get_id_len, get_lval, is_luafunc, lval_T, partial_name, LlTv,
        FNE_CHECK_START, GLV_READ_ONLY,
    };
    use crate::ported::message::{emsg, semsg};
    use tfn::*;

    let mut name: Option<String> = None;
    let mut len: i32 = 0;
    let mut lv = lval_T::default();

    // c:1988 if (fdp != NULL) CLEAR_POINTER(fdp);
    if let Some(fdp) = fdp.as_deref_mut() {
        *fdp = funcdict_T::default();
    }
    let orig: &str = *pp; // c:1991 const char *start = *pp;

    // c:1995 Check for hard coded <SNR>: already translated function ID.
    {
        let b = orig.as_bytes();
        if b.first() == Some(&K_SPECIAL) && b.get(1) == Some(&KS_EXTRA) && b.get(2) == Some(&KE_SNR)
        {
            // c:1996 *pp += 3; len = get_id_len(pp) + 3;
            let after = &orig[3..];
            len = get_id_len(after) + 3;
            let out = orig[..(len as usize).min(orig.len())].to_string();
            *pp = &orig[(len as usize).min(orig.len())..];
            return Some(out); // c:1999 return xmemdupz(start, len);
        }
    }

    // c:2003 A name starting with "<SID>"/"<SNR>" is local to a script.
    let mut lead = eval_fname_script(orig);
    let mut start: &str = orig;
    if lead > 2 {
        start = &orig[lead as usize..]; // c:2005 start += lead;
    }

    // c:2009 get_lval() — TFN_ flags use the same values as GLV_ flags.
    let end = get_lval(
        start,
        None,
        &mut lv,
        false,
        skip,
        flags | GLV_READ_ONLY,
        if lead > 2 { 0 } else { FNE_CHECK_START },
    );
    // c:2011 if (end == start) — offset 0 into `start`.
    if end == Some(0) {
        if !skip {
            emsg("E129: Function name required"); // c:2013
        }
        return None; // c:2015 goto theend
    }
    let ll_tv_present = !matches!(lv.ll_tv, LlTv::Null);
    // c:2017 if (end == NULL || (ll_tv != NULL && (lead > 2 || ll_range)))
    if end.is_none() || (ll_tv_present && (lead > 2 || lv.ll_range)) {
        // c:2021 aborting() is always false here.
        if end.is_some() {
            semsg(&format!("E475: Invalid argument: {start}")); // c:2023 e_invarg2
        }
        return None; // c:2028 goto theend
    }
    let end = end.unwrap();

    // c:2031 if (lv.ll_tv != NULL)
    if ll_tv_present {
        let tv = lv.ll_tv.get(); // *ll_tv (read-snapshot)
        // c:2032 fill fdp from the lval Dict context.
        let ll_dict_none = lv.ll_dict.is_none();
        if let Some(fdp) = fdp.as_deref_mut() {
            fdp.fd_dict = lv.ll_dict.clone();
            fdp.fd_newkey = lv.ll_newkey.take();
            fdp.fd_di = lv.ll_di.clone();
        }
        match tv.map(|t| (t.v_type, t.vval)) {
            // c:2038 VAR_FUNC && v_string != NULL
            Some((VAR_FUNC, v_string(s))) if !s.is_empty() => {
                name = Some(s);
                *pp = &start[end..];
            }
            // c:2041 VAR_PARTIAL && v_partial != NULL
            Some((VAR_PARTIAL, v_partial(Some(pt)))) => {
                let end_str = &start[end..];
                if is_luafunc(&pt) && end_str.as_bytes().first() == Some(&b'.') {
                    // c:2043 is_luafunc && *end == '.'
                    let l = check_luafunc_name(&end_str[1..], true);
                    if l == 0 {
                        semsg("E15: Invalid expression: v:lua"); // c:2046 e_invexpr2
                        return None;
                    }
                    name = Some(end_str[1..1 + l as usize].to_string());
                    *pp = &start[end + 1 + l as usize..];
                } else {
                    name = Some(partial_name(&pt).to_string()); // c:2054
                    *pp = &start[end..];
                }
                if let Some(partial) = partial.as_deref_mut() {
                    *partial = Some(pt); // c:2058
                }
            }
            _ => {
                // c:2061 not a Funcref/Partial value.
                let newkey_none = fdp
                    .as_deref()
                    .map(|f| f.fd_newkey.is_none())
                    .unwrap_or(true);
                if !skip
                    && (flags & TFN_QUIET) == 0
                    && (fdp.is_none() || ll_dict_none || newkey_none)
                {
                    emsg("E718: Funcref required"); // c:2065 e_funcref
                } else {
                    *pp = &start[end..];
                }
                name = None;
            }
        }
        return name; // c:2072 goto theend
    }

    // c:2075 if (lv.ll_name == NULL) — error found, continue after the name.
    if lv.ll_name.is_none() {
        *pp = &start[end..]; // c:2077
        return None; // c:2078 goto theend
    }

    // c:2082 Check if the name is a Funcref. If so, use the value.
    if let Some(exp) = lv.ll_exp_name.clone() {
        let d = deref_func_name(&exp, flags & TFN_NO_AUTOLOAD != 0); // c:2084
        if d.name != exp {
            // c:2085 name != lv.ll_exp_name
            name = Some(d.name);
            if let Some(partial) = partial.as_deref_mut() {
                *partial = d.partial;
            }
        }
    } else if (flags & TFN_NO_DEREF) == 0 {
        // c:2088 len = end - *pp; *pp is `orig` (unchanged).
        let src = &orig[..(end + if lead > 2 { lead as usize } else { 0 })];
        let d = deref_func_name(src, flags & TFN_NO_AUTOLOAD != 0);
        if d.name.as_str() != src {
            // c:2091 name != *pp
            name = Some(d.name);
            if let Some(partial) = partial.as_deref_mut() {
                *partial = d.partial;
            }
        }
    }
    if let Some(nm) = name {
        // c:2094 name = xstrdup(name); *pp = end;
        *pp = &start[end..];
        // RUST-PORT NOTE: the C rewrites a leading "<SNR>" to the 3-byte
        // K_SPECIAL/KS_EXTRA/KE_SNR sequence (c:2097); the reduced String model
        // keeps the literal "<SNR>" prefix (see `func_is_global`).
        return Some(nm); // c:2103 goto theend
    }

    // c:2106 Copy the function name to allocated memory.
    let mut ll_name = lv.ll_name.clone().unwrap_or_default();
    let mut ll_name_len = lv.ll_name_len;
    len = 0;
    if let Some(ref exp_s) = lv.ll_exp_name {
        len = exp_s.len() as i32; // c:2108
        if lead <= 2
            && lv.ll_name.as_deref() == Some(exp_s.as_str())
            && ll_name_len >= 2
            && ll_name.as_bytes().get(..2) == Some(b"s:".as_slice())
        {
            // c:2109 remove a leading "s:".
            ll_name = ll_name[2..].to_string();
            ll_name_len -= 2;
            len -= 2;
            lead = 2;
        }
    } else {
        // c:2119 Skip over "s:" and "g:".
        if lead == 2
            || (ll_name.as_bytes().first() == Some(&b'g') && ll_name.as_bytes().get(1) == Some(&b':'))
        {
            ll_name = ll_name[2..].to_string();
            ll_name_len -= 2;
        }
        // c:2123 len = end - lv.ll_name. RUST-PORT NOTE: the pointer arithmetic
        // reduces to the (post-strip) name length.
        len = ll_name_len as i32;
    }

    let mut sid_buf = String::new();
    // c:2130 Accept <SID>/<SNR> and translate into <SNR>123_.
    if skip {
        lead = 0; // c:2132 do nothing
    } else if lead > 0 {
        lead = 3; // c:2134
        if lv
            .ll_exp_name
            .as_deref()
            .map(eval_fname_sid)
            .unwrap_or(false)
            || eval_fname_sid(orig)
        {
            // c:2136 It's "s:" or "<SID>": needs a script id.
            // c:2138 current_sctx.sc_sid <= 0 in the reduced single-script model.
            emsg("E81: Using <SID> not in a script context"); // e_usingsid
            return None; // c:2141 goto theend
        }
        let _ = &mut sid_buf;
    } else if (flags & TFN_INT) == 0 && builtin_function(&ll_name, ll_name_len as i32) {
        // c:2148 E128: Function name must start with a capital or "s:".
        semsg(&format!(
            "E128: Function name must start with a capital or \"s:\": {orig}"
        ));
        return None; // c:2151 goto theend
    }

    // c:2154 Reject a colon inside the function name.
    if !skip && (flags & TFN_QUIET) == 0 && (flags & TFN_NO_DEREF) == 0 {
        if ll_name.as_bytes()[..ll_name_len]
            .iter()
            .rposition(|&c| c == b':')
            .is_some()
        {
            semsg(&format!(
                "E884: Function name cannot contain a colon: {orig}"
            )); // c:2159
            return None; // c:2160 goto theend
        }
    }

    // c:2164 name = xmalloc(len + lead + 1); build the (literal) <SNR> prefix.
    let mut result = String::new();
    if !skip && lead > 0 {
        // RUST-PORT NOTE: literal "<SNR>" instead of the K_SPECIAL byte prefix.
        result.push_str("<SNR>");
        if !sid_buf.is_empty() {
            result.push_str(&sid_buf);
        }
    }
    result.push_str(&ll_name[..(len as usize).min(ll_name.len())]); // c:2174 memmove
    *pp = &start[end..]; // c:2176
    Some(result)
}

/// Port of `save_function_name()` from `csrc/eval/userfunc.c:2215`.
///
/// Call [`trans_function_name`], except a `<lambda>N` name is returned as-is (the
/// `<lambda>` prefix plus its trailing digits). Advances `*name`.
pub fn save_function_name(
    name: &mut &str,
    skip: bool,
    flags: i32,
    fudi: Option<&mut funcdict_T>,
) -> Option<String> {
    let orig: &str = *name; // c:2217 char *p = *name;
    if orig.starts_with("<lambda>") {
        // c:2220 p += 8; getdigits(&p, false, 0);
        let b = orig.as_bytes();
        let mut p = 8;
        while p < b.len() && b[p].is_ascii_digit() {
            p += 1;
        }
        let saved = orig[..p].to_string(); // c:2222 xmemdupz(*name, p - *name)
        if let Some(fudi) = fudi {
            *fudi = funcdict_T::default(); // c:2224 CLEAR_POINTER(fudi)
        }
        *name = &orig[p..]; // c:2229 *name = p;
        Some(saved)
    } else {
        let mut p: &str = orig;
        let saved = trans_function_name(&mut p, skip, flags, fudi, None); // c:2227
        *name = p;
        saved
    }
}

/// Port of `get_lambda_tv()` from `csrc/eval/userfunc.c:299`.
///
/// Parse a `{args -> expr}` lambda at `*arg`, advancing it past the closing `}`
/// and (when evaluating) storing a `VAR_PARTIAL` Funcref in `rettv`. Returns
/// [`OK`](crate::ported::eval_h::OK)/[`FAIL`](crate::ported::eval_h::FAIL), or
/// [`NOTDONE`](crate::ported::eval::NOTDONE) when the text is not a lambda (no
/// top-level `->`), so the caller can retry it as a Dict / `{expr}` name.
///
/// RUST-PORT NOTE: `skip_expr` (the `eval.c` parse-pointer expression skipper)
/// is replaced by a bracket-balanced scan (the same approach as
/// [`get_func_arguments`]); the generated `ufunc_T` body (`uf_lines = "return
/// <expr>"`), its `hash_add` into `func_hashtab`, `register_closure`, profiling
/// and `sctx` are bridge-owned, so the reduced [`partial_T`] only carries the
/// generated `<lambda>N` name. `eval_lavars_used`/`sandbox` state is not modeled.
pub fn get_lambda_tv(
    arg: &mut &str,
    rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
    evalarg: Option<&crate::ported::eval::evalarg_T>,
) -> i32 {
    use crate::ported::eval::skipwhite;
    use crate::ported::eval::typval_defs_h::{
        partial_T, typval_T, typval_vval_union::v_partial, VarLockStatus, VarType::VAR_PARTIAL,
    };
    use crate::ported::eval::{EVAL_EVALUATE, NOTDONE};
    use crate::ported::eval_h::{FAIL, OK};

    let evaluate = evalarg.map(|e| e.eval_flags & EVAL_EVALUATE != 0).unwrap_or(false);

    let orig: &str = *arg;
    // c:315 char *s = skipwhite(*arg + 1); First check: "->" must exist.
    let after_brace = &orig[1..];
    // Scan for a top-level "->" (balancing brackets, skipping strings).
    let arrow = {
        let b = after_brace.as_bytes();
        let (mut depth, mut i, mut found) = (0i32, 0usize, None);
        while i < b.len() {
            match b[i] {
                b'\'' => {
                    i += 1;
                    while i < b.len() && b[i] != b'\'' {
                        i += 1;
                    }
                }
                b'"' => {
                    i += 1;
                    while i < b.len() && b[i] != b'"' {
                        if b[i] == b'\\' && i + 1 < b.len() {
                            i += 1;
                        }
                        i += 1;
                    }
                }
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b'-' if depth == 0 && b.get(i + 1) == Some(&b'>') => {
                    found = Some(i);
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        found
    };
    // c:317 if (ret == FAIL || *s != '>') return NOTDONE;
    let arrow = match arrow {
        Some(a) => a,
        None => return NOTDONE,
    };

    // c:328 Parse the arguments (the text before "->").
    let params = &after_brace[..arrow];
    let (names, _defaults, _varargs) = match get_function_args(params) {
        Some(v) => v,
        None => return FAIL, // c:330 goto errret
    };

    // c:340 Get the start and end of the expression.
    let expr_tail = &after_brace[arrow + 2..];
    let ws = expr_tail.len() - skipwhite(expr_tail).len();
    let expr_region = skipwhite(expr_tail);
    // skip_expr → the matching top-level '}'.
    let close = {
        let b = expr_region.as_bytes();
        let (mut depth, mut i, mut found) = (0i32, 0usize, None);
        while i < b.len() {
            match b[i] {
                b'\'' => {
                    i += 1;
                    while i < b.len() && b[i] != b'\'' {
                        i += 1;
                    }
                }
                b'"' => {
                    i += 1;
                    while i < b.len() && b[i] != b'"' {
                        if b[i] == b'\\' && i + 1 < b.len() {
                            i += 1;
                        }
                        i += 1;
                    }
                }
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' => depth -= 1,
                b'}' if depth == 0 => {
                    found = Some(i);
                    break;
                }
                b'}' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        found
    };
    // c:352 if (**arg != '}') semsg(E451); goto errret;
    let close = match close {
        Some(c) => c,
        None => {
            crate::ported::message::semsg(&format!("E451: Expected }}: {expr_region}"));
            return FAIL;
        }
    };
    // c:356 (*arg)++ past the '}'.
    let close_abs = 1 + arrow + 2 + ws + close;
    *arg = &orig[close_abs + 1..];

    if evaluate {
        // c:358 String name = get_lambda_name();
        let name = get_lambda_name();
        // c:359 fp = alloc_ufunc(name); (reduced: body/registration is bridge-owned)
        let mut fp = alloc_ufunc(&name);
        fp.uf_args = names;
        fp.uf_varargs = true; // c:404
        let _ = fp;
        // c:360 pt = xcalloc(1, sizeof(partial_T)); pt->pt_func = fp;
        let pt = partial_T {
            pt_refcount: 1, // c:409
            pt_name: name,
            pt_argv: Vec::new(),
            pt_dict: None,
        };
        *rettv = typval_T {
            v_type: VAR_PARTIAL, // c:412
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_partial(Some(std::rc::Rc::new(pt))), // c:410
        };
    }
    OK // c:424
}

/// Port of `ex_return()` from `csrc/eval/userfunc.c:3290`.
///
/// Handle a `:return [expr]` command: evaluate the optional expression with
/// [`eval0`](crate::ported::eval::eval0) and carry out the return via
/// [`do_return`]. Errors `E133` when not inside a function.
///
/// RUST-PORT NOTE: `emsg_skip`/`update_force_abort()`/`aborting()` editor state
/// is not modeled (matching [`eval0`](crate::ported::eval::eval0)); `check_nextcmd`
/// (ex_docmd.c, not vendored) is inlined for the "no argument" advance.
pub fn ex_return(eap: &mut exarg_T) {
    use crate::ported::eval::typval::tv_clear;
    use crate::ported::eval::{eval0, evalarg_T, skipwhite, typval_defs_h::typval_T, EVAL_EVALUATE};
    use crate::ported::eval_h::FAIL;

    let arg = eap.arg.clone(); // c:3292
    let mut returning = false; // c:3294

    // c:3296 if (current_funccal == NULL)
    if get_current_funccal().is_none() {
        crate::ported::message::emsg("E133: :return not inside a function"); // c:3297
        return;
    }

    // c:3301 evalarg_T evalarg = { .eval_flags = eap->skip ? 0 : EVAL_EVALUATE };
    let mut evalarg = evalarg_T {
        eval_flags: if eap.skip { 0 } else { EVAL_EVALUATE },
    };

    eap.nextcmd = None; // c:3307
    let first = arg.as_bytes().first().copied().unwrap_or(0);
    let mut rettv = typval_T::default();
    // c:3308 (*arg != NUL && *arg != '|' && *arg != '\n') && eval0(...) != FAIL
    if first != 0
        && first != b'|'
        && first != b'\n'
        && eval0(&arg, &mut rettv, Some(&mut evalarg)) != FAIL
    {
        if !eap.skip {
            returning = do_return(false, true, Some(rettv.clone())); // c:3311
        } else {
            tv_clear(&mut rettv); // c:3313
        }
    } else if !eap.skip {
        // c:3315 It's safer to return also on error. aborting() is false here.
        returning = do_return(false, true, None); // c:3322
    }

    // c:3328 advance to the next command unless the return was carried out.
    if returning {
        eap.nextcmd = None; // c:3330
    } else if eap.nextcmd.is_none() {
        // c:3332 check_nextcmd(arg) — inlined.
        let p = skipwhite(&arg);
        let c = p.as_bytes().first().copied().unwrap_or(0);
        eap.nextcmd = if c == b'|' || c == b'\n' {
            Some(p[1..].to_string())
        } else {
            None
        };
    }
}

/// Port of `ex_call_inner()` from `csrc/eval/userfunc.c:3342`.
///
/// The lower-level implementation of `:call`: evaluate `name(args)` over the
/// command's line range, dereferencing a returned Funcref/Dict/List via
/// [`handle_subscript`](crate::ported::eval::handle_subscript). Returns non-zero
/// (`failed`) on error.
///
/// RUST-PORT NOTE: the C threads a `funcexe_T` and (when `addr_count > 0`)
/// positions the cursor per range line via `curbuf`/`curwin`; there is no buffer
/// standalone, so the range loop runs `fe_doesrange == false` and the isolated
/// argument text (between the call parens) is passed via the reduced
/// [`get_func_tv`]. `aborting()` is always false here.
pub fn ex_call_inner(
    eap: &exarg_T,
    name: &str,
    args_text: &str,
    rettv: &mut crate::ported::eval::typval_defs_h::typval_T,
) -> i32 {
    use crate::ported::eval::handle_subscript;
    use crate::ported::eval::typval::tv_clear;
    use crate::ported::eval_h::FAIL;

    let mut failed = false; // c:3346
    let mut lnum = eap.line1; // c:3348
    while lnum <= eap.line2 {
        // c:3349 addr_count>0 buffer positioning omitted (no buffer standalone).
        if get_func_tv(name, args_text, rettv) == FAIL {
            // c:3367
            failed = true; // c:3368
            break;
        }
        // c:3373 Handle a function returning a Funcref, Dict or List.
        if handle_subscript(rettv, &[], true) == FAIL {
            failed = true; // c:3375
            break;
        }
        tv_clear(rettv); // c:3379
        // c:3380 doesrange is false in the reduced model → continue.
        // c:3387 aborting() is always false → continue.
        lnum += 1;
    }
    failed as i32 // c:3392
}

/// Port of `ex_call()` from `csrc/eval/userfunc.c:3542`.
///
/// Handle `:call func(args)` (and `:defer func(args)`): resolve the name with
/// [`trans_function_name`]/[`deref_func_name`], then dispatch through
/// [`ex_call_inner`] (or [`ex_defer_inner`] for `:defer`).
///
/// RUST-PORT NOTE: when skipping, the C uses `eval0()` to skip to the next
/// command; `fill_evalarg_from_eap`/`clear_evalarg` and the `fudi.fd_dict`
/// refcount bump are `Rc`/no-op-managed; `cstack->cs_trylevel` is not modeled so
/// the trailing-character check reduces to `!failed`.
pub fn ex_call(eap: &mut exarg_T) {
    use crate::ported::eval::typval::tv_clear;
    use crate::ported::eval::{eval0, ends_excmd, evalarg_T, skipwhite, typval_defs_h::typval_T, EVAL_EVALUATE};
    use crate::ported::message::semsg;

    let orig_arg = eap.arg.clone(); // c:3544
    let mut fudi = funcdict_T::default();
    let mut partial: Option<std::rc::Rc<crate::ported::eval::typval_defs_h::partial_T>> = None;
    let mut evalarg = evalarg_T {
        eval_flags: if eap.skip { 0 } else { EVAL_EVALUATE },
    };

    // c:3551 if (eap->skip) — use eval0() to skip to any following command.
    if eap.skip {
        let mut rettv = typval_T::default();
        if eval0(&orig_arg, &mut rettv, Some(&mut evalarg)) != crate::ported::eval_h::FAIL {
            tv_clear(&mut rettv); // c:3559
        }
        return; // c:3564
    }

    // c:3567 trans_function_name()
    let mut arg: &str = &orig_arg;
    let tofree = trans_function_name(
        &mut arg,
        false,
        tfn::TFN_INT,
        Some(&mut fudi),
        Some(&mut partial),
    );
    if let Some(nk) = fudi.fd_newkey.take() {
        // c:3568 Still need to give an error message for a missing key.
        semsg(&format!("E716: Key not present in Dictionary: {nk}")); // e_dictkey
    }
    let tofree = match tofree {
        Some(t) => t, // c:3572
        None => return,
    };

    // c:3576 fd_dict->dv_refcount++ — Rc-managed, no-op.

    // c:3581 deref_func_name(): use a Funcref/Partial variable's contents.
    let d = deref_func_name(&tofree, false);
    let name = d.name;
    if partial.is_none() {
        partial = d.partial; // c:3584
    }

    // c:3588 Skip white space to allow ":call func ()".
    let startarg = skipwhite(arg);
    if startarg.as_bytes().first() != Some(&b'(') {
        // c:3591 E107: Missing parentheses.
        semsg(&format!("E107: Missing parentheses: {orig_arg}"));
        return; // goto end
    }

    // Isolate the (balanced) argument text between the call parens.
    let (args_text, remaining) = {
        let b = startarg.as_bytes();
        let (mut depth, mut i, mut close) = (0i32, 0usize, None);
        while i < b.len() {
            match b[i] {
                b'\'' => {
                    i += 1;
                    while i < b.len() && b[i] != b'\'' {
                        i += 1;
                    }
                }
                b'"' => {
                    i += 1;
                    while i < b.len() && b[i] != b'"' {
                        if b[i] == b'\\' && i + 1 < b.len() {
                            i += 1;
                        }
                        i += 1;
                    }
                }
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(i);
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        match close {
            Some(c) => (&startarg[1..c], &startarg[c + 1..]),
            None => (&startarg[1..], ""),
        }
    };

    let failed;
    let mut rettv = typval_T::default();
    if eap.cmdidx == cmdidx_T::CMD_defer {
        // c:3600 arg = startarg; ex_defer_inner(...)
        failed = ex_defer_inner() == crate::ported::eval_h::FAIL;
    } else {
        // c:3603 funcexe setup + ex_call_inner()
        failed = ex_call_inner(eap, &name, args_text, &mut rettv) != 0;
    }

    // c:3616 Trailing-character check. cstack->cs_trylevel is not modeled, so
    // this reduces to "!failed" (aborting()/did_throw are false).
    if !failed {
        let c = remaining.as_bytes().first().copied().unwrap_or(0);
        if !ends_excmd(c) {
            // c:3620 emsg_severe; E488 trailing.
            semsg(&format!("E488: Trailing characters: {remaining}"));
        } else {
            // c:3625 eap->nextcmd = check_nextcmd(arg) — inlined.
            let p = skipwhite(remaining);
            let c = p.as_bytes().first().copied().unwrap_or(0);
            eap.nextcmd = if c == b'|' || c == b'\n' {
                Some(p[1..].to_string())
            } else {
                None
            };
        }
    }
}

/// Port of `ex_delfunction()` from `csrc/eval/userfunc.c:3119`.
///
/// Handle `:delfunction[!] {name}`: resolve the name with [`trans_function_name`]
/// and remove it from the registry via [`func_remove`].
///
/// RUST-PORT NOTE: the `uf_calls`/`uf_refcount` in-use guards and
/// `func_clear_free`/`tv_dict_item_remove` are `Rc`/`Drop`-managed (see the
/// no-op reclamation ports above), so a resolved function is simply removed
/// through the bridge's [`REMOVE_FUNC_HOOK`]. `check_nextcmd` (ex_docmd.c) is
/// inlined.
pub fn ex_delfunction(eap: &mut exarg_T) {
    use crate::ported::eval::{ends_excmd, skipwhite};
    use crate::ported::message::{emsg, semsg};

    let orig = eap.arg.clone(); // c:3124
    let mut p: &str = &orig;
    let mut fudi = funcdict_T::default();
    let name = trans_function_name(&mut p, eap.skip, 0, Some(&mut fudi), None); // c:3125
    fudi.fd_newkey = None; // c:3126 xfree(fudi.fd_newkey)
    let name = match name {
        None => {
            // c:3127 if (name == NULL)
            if fudi.fd_dict.is_some() && !eap.skip {
                emsg("E718: Funcref required"); // c:3129 e_funcref
            }
            return; // c:3131
        }
        Some(n) => n,
    };
    // c:3133 if (!ends_excmd(*skipwhite(p)))
    let sw = skipwhite(p);
    if !ends_excmd(sw.as_bytes().first().copied().unwrap_or(0)) {
        semsg(&format!("E488: Trailing characters: {p}")); // c:3136 e_trailing_arg
        return;
    }
    // c:3138 eap->nextcmd = check_nextcmd(p) — inlined.
    let c = sw.as_bytes().first().copied().unwrap_or(0);
    eap.nextcmd = if c == b'|' || c == b'\n' {
        Some(sw[1..].to_string())
    } else {
        None
    };

    // c:3143 if (isdigit(*name) && fudi.fd_dict == NULL)
    if name
        .as_bytes()
        .first()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
        && fudi.fd_dict.is_none()
    {
        if !eap.skip {
            semsg(&format!("E475: Invalid argument: {orig}")); // c:3145 e_invarg2
        }
        return;
    }
    // c:3150 if (!eap->skip) fp = find_func(name);
    let fp = if !eap.skip { find_func(&name) } else { None };

    if !eap.skip {
        match fp {
            None => {
                // c:3155 if (fp == NULL)
                if !eap.forceit {
                    semsg(&format!("E130: Unknown function: {orig}")); // c:3157 e_nofunc
                }
                // c:3159 return
            }
            Some(fp) => {
                // c:3161/3183 uf_calls/uf_refcount guards are Rc-managed; remove.
                func_remove(&fp); // c:3187
            }
        }
    }
}

/// Port of `get_function_body()` from `csrc/eval/userfunc.c:2363`.
///
/// Read the body of a `:function` (every line into `newlines`, up to
/// `:endfunction`). Returns [`OK`](crate::ported::eval_h::OK)/[`FAIL`](crate::ported::eval_h::FAIL).
///
/// RUST-PORT NOTE: the body is read line-by-line via `eap->ea_getline` (the
/// ex_docmd source-line reader) — an honest deferred dep with no standalone
/// counterpart. This is the faithful port of the loop *entry* up to that call:
/// with no getline source the first `theline` is NULL, so the loop reports `E126`
/// and fails (the `endfunction`/heredoc/nesting scan over subsequent lines is
/// bridge-owned). The inline `line_arg_in` split (`eap->arg` by `\n`) is modeled.
pub fn get_function_body(
    _eap: &exarg_T,
    newlines: &mut Vec<String>,
    line_arg_in: Option<&str>,
) -> i32 {
    use crate::ported::eval_h::FAIL;
    let _ = newlines;
    // c:2377 while (true) — the first iteration only (up to the getline call).
    let theline: Option<&str> = match line_arg_in {
        Some(la) => {
            // c:2383 theline = line_arg; p = vim_strchr(theline, '\n');
            match la.find('\n') {
                Some(i) => Some(&la[..i]),
                None => Some(la),
            }
        }
        None => {
            // c:2398 eap->ea_getline(...) — deferred dep (no standalone getline)
            // yields NULL, i.e. end of input.
            None
        }
    };
    // c:2408 if (theline == NULL)
    if theline.is_none() {
        crate::ported::message::emsg("E126: Missing :endfunction"); // c:2412
        return FAIL; // c:2414 goto theend
    }
    // Unreachable standalone (getline yields NULL): the endfunction/heredoc scan
    // over subsequent getline lines is bridge-owned.
    FAIL
}

/// Port of `ex_function()` from `csrc/eval/userfunc.c:2637`.
///
/// Handle a `:function` command: list functions (no argument / `/pat` / bare
/// name), or define one by parsing the name ([`save_function_name`]) and
/// parameter list ([`get_function_args`]), then reading the body
/// ([`get_function_body`]).
///
/// RUST-PORT NOTE: interactive listing (`list_functions`/`list_one_function`),
/// the `range`/`dict`/`abort`/`closure` extra-argument scan, and the registration
/// of the parsed body into `func_hashtab` are bridge-owned / not modeled; this
/// is the faithful port of the driver up to the [`get_function_body`] getline
/// boundary. `aborting()`/`did_emsg` are not modeled.
pub fn ex_function(eap: &mut exarg_T) {
    use crate::ported::eval::{ends_excmd, skipwhite};
    use crate::ported::eval_h::FAIL;
    use crate::ported::message::semsg;

    let orig = eap.arg.clone();
    let mut fudi = funcdict_T::default();

    // c:2654 ":function" without argument: list functions.
    if ends_excmd(orig.as_bytes().first().copied().unwrap_or(0)) {
        if !eap.skip {
            list_functions(); // c:2657
        }
        // c:2659 eap->nextcmd = check_nextcmd(eap->arg) — inlined.
        let c = orig.as_bytes().first().copied().unwrap_or(0);
        eap.nextcmd = if c == b'|' || c == b'\n' {
            Some(orig[1..].to_string())
        } else {
            None
        };
        return;
    }

    // c:2664 ":function /pat": list functions matching pattern.
    if orig.as_bytes().first() == Some(&b'/') {
        let _ = list_functions_matching_pat(); // c:2665
        eap.nextcmd = None;
        return; // c:2667
    }

    // c:2686 Get the function name.
    let mut p: &str = &orig;
    let name = save_function_name(&mut p, eap.skip, tfn::TFN_NO_AUTOLOAD, Some(&mut fudi)); // c:2687
    let paren = p.contains('('); // c:2688
    if name.is_none() && (fudi.fd_dict.is_none() || !paren) && !eap.skip {
        // c:2692 aborting() is false here.
        if let Some(nk) = fudi.fd_newkey.take() {
            semsg(&format!("E716: Key not present in Dictionary: {nk}")); // c:2694 e_dictkey
        }
        return; // c:2697
    }

    // c:2708 ":function func" with only a function name: list the function.
    if !paren {
        let _ = list_one_function(); // c:2710
        return; // c:2711 goto ret_free
    }

    // c:2714 ":function name(arg1, arg2)" Define function.
    p = skipwhite(p);
    if p.as_bytes().first() != Some(&b'(') {
        if !eap.skip {
            semsg(&format!("E124: Missing '(': {orig}")); // c:2717
            return; // c:2718 goto ret_free
        }
    }
    // c:2724 p = skipwhite(p + 1);
    p = if p.is_empty() { p } else { skipwhite(&p[1..]) };

    // c:2761 get_function_args(&p, ')', ...) — reduced takes the params substring
    // (the text up to the matching ')').
    let params = {
        let b = p.as_bytes();
        let (mut depth, mut i, mut close) = (0i32, 0usize, None);
        while i < b.len() {
            match b[i] {
                b')' if depth == 0 => {
                    close = Some(i);
                    break;
                }
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        match close {
            Some(c) => &p[..c],
            None => p,
        }
    };
    if get_function_args(params).is_none() {
        return; // c:2763 goto errret_2
    }

    // c:2766 range/dict/abort/closure extra-argument scan is bridge-owned.

    // c:2xxx Read the function body (up to the ea_getline boundary).
    let mut newlines: Vec<String> = Vec::new();
    if get_function_body(eap, &mut newlines, None) == FAIL {
        return; // errret
    }
    // RUST-PORT NOTE: registration of the parsed body into func_hashtab
    // (hash_add / uf_lines) is bridge-owned; the reference port stops here.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trans_function_name_plain() {
        // A plain global name resolves as-is; *pp advances past it.
        let mut p: &str = "MyFunc()";
        let n = trans_function_name(&mut p, false, tfn::TFN_INT, None, None);
        assert_eq!(n.as_deref(), Some("MyFunc"));
        assert_eq!(p, "()");
        // A lowercase name without TFN_INT is rejected (E128, builtin-shaped).
        let mut q: &str = "foo()";
        assert!(trans_function_name(&mut q, false, 0, None, None).is_none());
    }

    #[test]
    fn save_function_name_lambda_and_name() {
        // <lambda>N is returned verbatim (prefix + digits).
        let mut n: &str = "<lambda>42()";
        assert_eq!(
            save_function_name(&mut n, false, 0, None).as_deref(),
            Some("<lambda>42")
        );
        assert_eq!(n, "()");
        // A normal name delegates to trans_function_name.
        let mut m: &str = "Helper(x)";
        assert_eq!(
            save_function_name(&mut m, false, tfn::TFN_INT, None).as_deref(),
            Some("Helper")
        );
        assert_eq!(m, "(x)");
    }

    #[test]
    fn get_lambda_tv_parses_and_declines() {
        use crate::ported::eval::evalarg_T;
        use crate::ported::eval::typval_defs_h::{typval_T, typval_vval_union::v_partial};
        use crate::ported::eval::{EVAL_EVALUATE, NOTDONE};
        use crate::ported::eval_h::OK;
        let ea = evalarg_T {
            eval_flags: EVAL_EVALUATE,
        };
        // A real lambda evaluates to a VAR_PARTIAL and consumes through '}'.
        let mut arg: &str = "{a, b -> a + b} rest";
        let mut rv = typval_T::default();
        assert_eq!(get_lambda_tv(&mut arg, &mut rv, Some(&ea)), OK);
        assert!(matches!(rv.vval, v_partial(Some(_))));
        assert_eq!(arg, " rest");
        // No top-level "->": it's a Dict/{expr}, not a lambda → NOTDONE.
        let mut arg2: &str = "{'a': 1}";
        let mut rv2 = typval_T::default();
        assert_eq!(get_lambda_tv(&mut arg2, &mut rv2, Some(&ea)), NOTDONE);
    }

    #[test]
    fn ex_return_carries_value() {
        use crate::ported::eval::typval_defs_h::{typval_T, typval_vval_union::v_number};
        // Not inside a function → E133, nothing carried, no panic.
        current_funccal.with(|c| *c.borrow_mut() = None);
        let mut eap = exarg_T {
            arg: "1".into(),
            ..Default::default()
        };
        ex_return(&mut eap);

        // Inside a function, ":return 42" records the value.
        let fp = ufunc_T {
            uf_name: "F".into(),
            ..Default::default()
        };
        let fc = create_funccal(&fp, None);
        let mut eap2 = exarg_T {
            arg: "42".into(),
            ..Default::default()
        };
        ex_return(&mut eap2);
        assert!(fc.borrow().fc_returned);
        assert!(matches!(
            fc.borrow().fc_rettv.as_ref().unwrap().vval,
            v_number(42)
        ));
        let _ = typval_T::default();
        current_funccal.with(|c| *c.borrow_mut() = None);
    }

    #[test]
    fn ex_call_inner_dispatches_via_hooks() {
        use crate::ported::eval::typval::{CALL_FUNC_HOOK, EVAL_STRING_HOOK};
        use crate::ported::eval::typval_defs_h::{typval_T, typval_vval_union::v_number};
        fn eval_hook(e: &str) -> Option<typval_T> {
            e.trim().parse::<i64>().ok().map(typval_T::from)
        }
        fn call_hook(_c: &typval_T, args: &[typval_T]) -> Option<typval_T> {
            Some(typval_T::from(
                args.iter()
                    .map(|a| match a.vval {
                        v_number(n) => n,
                        _ => 0,
                    })
                    .sum::<i64>(),
            ))
        }
        let (se, sc) = (
            EVAL_STRING_HOOK.with(|h| *h.borrow()),
            CALL_FUNC_HOOK.with(|h| *h.borrow()),
        );
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = Some(eval_hook));
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = Some(call_hook));
        let eap = exarg_T {
            line1: 1,
            line2: 1,
            ..Default::default()
        };
        let mut rv = typval_T::default();
        // Range of one line, args evaluate + dispatch → not failed (0).
        assert_eq!(ex_call_inner(&eap, "Sum", "2, 3, 4", &mut rv), 0);
        // A non-evaluable argument fails the call.
        let mut rv2 = typval_T::default();
        assert_eq!(ex_call_inner(&eap, "Sum", "1, bad", &mut rv2), 1);
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = se);
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = sc);
    }

    #[test]
    fn ex_call_resolves_and_runs() {
        use crate::ported::eval::typval::{CALL_FUNC_HOOK, EVAL_STRING_HOOK};
        use crate::ported::eval::typval_defs_h::typval_T;
        fn eval_hook(e: &str) -> Option<typval_T> {
            e.trim().parse::<i64>().ok().map(typval_T::from)
        }
        fn call_hook(_c: &typval_T, args: &[typval_T]) -> Option<typval_T> {
            Some(typval_T::from(args.len() as i64))
        }
        let (se, sc) = (
            EVAL_STRING_HOOK.with(|h| *h.borrow()),
            CALL_FUNC_HOOK.with(|h| *h.borrow()),
        );
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = Some(eval_hook));
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = Some(call_hook));
        // Full driver: name resolution + arg isolation + dispatch, no panic.
        let mut eap = exarg_T {
            arg: "Sum(2, 3, 4)".into(),
            line1: 1,
            line2: 1,
            ..Default::default()
        };
        ex_call(&mut eap);
        // Missing parentheses is reported (no dispatch), no panic.
        let mut eap2 = exarg_T {
            arg: "Sum".into(),
            ..Default::default()
        };
        ex_call(&mut eap2);
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = se);
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = sc);
    }

    #[test]
    fn ex_delfunction_removes_via_hook() {
        fn rm(name: &str) -> bool {
            name == "MyFunc"
        }
        fn find(name: &str) -> Option<ufunc_T> {
            (name == "MyFunc").then(|| ufunc_T {
                uf_name: "MyFunc".into(),
                ..Default::default()
            })
        }
        let savedr = REMOVE_FUNC_HOOK.with(|h| *h.borrow());
        let savedf = FIND_FUNC_HOOK.with(|h| *h.borrow());
        REMOVE_FUNC_HOOK.with(|h| *h.borrow_mut() = Some(rm));
        FIND_FUNC_HOOK.with(|h| *h.borrow_mut() = Some(find));
        // Known function: resolves + removes, no panic.
        let mut eap = exarg_T {
            arg: "MyFunc".into(),
            ..Default::default()
        };
        ex_delfunction(&mut eap);
        // Unknown function with ! is silently tolerated.
        let mut eap2 = exarg_T {
            arg: "Nope".into(),
            forceit: true,
            ..Default::default()
        };
        ex_delfunction(&mut eap2);
        REMOVE_FUNC_HOOK.with(|h| *h.borrow_mut() = savedr);
        FIND_FUNC_HOOK.with(|h| *h.borrow_mut() = savedf);
    }

    #[test]
    fn ex_function_body_needs_getline() {
        // With no getline source, :function reaches get_function_body and fails
        // with E126 (no :endfunction) rather than panicking.
        assert_eq!(
            get_function_body(&exarg_T::default(), &mut Vec::new(), None),
            crate::ported::eval_h::FAIL
        );
        let mut eap = exarg_T {
            arg: "Foo()".into(),
            ..Default::default()
        };
        ex_function(&mut eap);
    }

    #[test]
    fn name_classification() {
        assert!(builtin_function("strlen", -1));
        assert!(!builtin_function("g:Foo", -1)); // scope
        assert!(!builtin_function("Foo", -1)); // uppercase
        assert!(!builtin_function("foo#bar", -1)); // autoload

        assert_eq!(eval_fname_script("s:Helper"), 2);
        assert_eq!(eval_fname_script("<SID>Helper"), 5);
        assert_eq!(eval_fname_script("<SNR>9_x"), 5);
        assert_eq!(eval_fname_script("Helper"), 0);
        assert!(eval_fname_sid("s:Helper"));

        assert!(func_name_refcount("123")); // anonymous
        assert!(func_name_refcount("<lambda>7"));
        assert!(!func_name_refcount("MyFunc"));

        assert_eq!(get_scriptlocal_funcname(Some("Plain")), None);
        assert!(!function_list_modified(0));
    }

    #[test]
    fn lambda_names_increment() {
        let a = get_lambda_name();
        let b = get_lambda_name();
        assert!(a.starts_with("<lambda>"));
        let na: i32 = a.trim_start_matches("<lambda>").parse().unwrap();
        let nb: i32 = b.trim_start_matches("<lambda>").parse().unwrap();
        assert_eq!(nb, na + 1);
    }

    #[test]
    fn call_user_func_check_arity_and_dispatch() {
        use crate::ported::eval::typval::CALL_FUNC_HOOK;
        use crate::ported::eval::typval_defs_h::{typval_T, typval_vval_union::v_number};
        use fcerr::*;
        // F(a, b): 2 required.
        let fp = ufunc_T {
            uf_name: "F".into(),
            uf_args: vec!["a".into(), "b".into()],
            ..Default::default()
        };
        fn hook(_c: &typval_T, args: &[typval_T]) -> Option<typval_T> {
            Some(typval_T::from(args.len() as i64))
        }
        let saved = CALL_FUNC_HOOK.with(|h| *h.borrow());
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = Some(hook));
        let mut rv = typval_T::from(0);
        // too few args → FCERR, no dispatch
        assert_eq!(
            call_user_func_check(&fp, &[typval_T::from(1)], &mut rv),
            FCERR_TOOFEW
        );
        // correct arity → dispatch
        assert_eq!(
            call_user_func_check(&fp, &[typval_T::from(1), typval_T::from(2)], &mut rv),
            FCERR_NONE
        );
        assert!(matches!(rv.vval, v_number(2)));
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = saved);
    }

    #[test]
    fn find_func_via_hook() {
        use super::{find_func, FIND_FUNC_HOOK};
        fn hook(name: &str) -> Option<ufunc_T> {
            (name == "MyFunc").then(|| ufunc_T {
                uf_name: "MyFunc".into(),
                uf_args: vec!["a".into(), "b".into()],
                uf_def_args: vec!["b".into()],
                uf_varargs: false,
                ..Default::default()
            })
        }
        let saved = FIND_FUNC_HOOK.with(|h| *h.borrow());
        FIND_FUNC_HOOK.with(|h| *h.borrow_mut() = None);
        assert!(find_func("MyFunc").is_none()); // no hook
        FIND_FUNC_HOOK.with(|h| *h.borrow_mut() = Some(hook));
        let f = find_func("MyFunc").unwrap();
        assert_eq!(f.uf_args.len(), 2);
        // the looked-up ufunc feeds the ported arity helper
        assert_eq!(get_func_arity("MyFunc", Some(&f)), Some((1, 1, false)));
        assert!(find_func("nope").is_none());
        FIND_FUNC_HOOK.with(|h| *h.borrow_mut() = saved);
    }

    #[test]
    fn user_func_argcount_and_arity() {
        use fcerr::*;
        // F(a, b = 1, ...): 1 required, 1 optional, varargs.
        let f = ufunc_T {
            uf_name: "F".into(),
            uf_args: vec!["a".into(), "b".into()],
            uf_def_args: vec!["b".into()],
            uf_varargs: true,
            ..Default::default()
        };
        assert_eq!(check_user_func_argcount(&f, 0), FCERR_TOOFEW);
        assert_eq!(check_user_func_argcount(&f, 1), FCERR_UNKNOWN); // ok
        assert_eq!(check_user_func_argcount(&f, 9), FCERR_UNKNOWN); // varargs ok
        assert_eq!(get_func_arity("F", Some(&f)), Some((1, 1, true)));

        // A fixed-arity function rejects extra args.
        let g = ufunc_T {
            uf_name: "G".into(),
            uf_args: vec!["x".into()],
            ..Default::default()
        };
        assert_eq!(check_user_func_argcount(&g, 2), FCERR_TOOMANY);

        // Builtins resolve via BUILTIN_ARGC; unknown names fail.
        assert_eq!(get_func_arity("add", None), Some((2, 0, false)));
        assert_eq!(get_func_arity("argc", None), Some((0, 1, false)));
        assert_eq!(get_func_arity("no_such_func_xyz", None), None);
    }

    #[test]
    fn get_return_cmd_formats() {
        use crate::ported::eval::typval_defs_h::typval_T;
        assert_eq!(get_return_cmd(Some(&typval_T::from(42))), ":return 42");
        assert_eq!(
            get_return_cmd(Some(&typval_T::from("hi".to_string()))),
            ":return hi"
        );
        assert_eq!(get_return_cmd(None), ":return ");
    }

    #[test]
    fn add_nr_var_inserts() {
        use crate::ported::eval::typval::tv_dict_find;
        use crate::ported::eval::typval_defs_h::{dict_T, typval_vval_union::v_number};
        let mut d = dict_T::default();
        add_nr_var(&mut d, "lnum", 7);
        add_nr_var(&mut d, "winid", 3);
        assert!(matches!(
            tv_dict_find(&d, "lnum").map(|tv| &tv.vval),
            Some(v_number(7))
        ));
        assert!(matches!(
            tv_dict_find(&d, "winid").map(|tv| &tv.vval),
            Some(v_number(3))
        ));
    }

    #[test]
    fn func_call_via_hook() {
        use crate::ported::eval::typval::tv_list_append_number;
        use crate::ported::eval::typval::CALL_FUNC_HOOK;
        use crate::ported::eval::typval_defs_h::{
            list_T, typval_T, typval_vval_union::*, VarLockStatus, VarType::*,
        };
        use crate::ported::eval_h::{FAIL, OK};
        fn hook(_c: &typval_T, args: &[typval_T]) -> Option<typval_T> {
            Some(typval_T::from(args.len() as i64))
        }
        let saved = CALL_FUNC_HOOK.with(|h| *h.borrow());
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = Some(hook));
        // args as a List typval
        let mut l = list_T::default();
        tv_list_append_number(&mut l, 1);
        tv_list_append_number(&mut l, 2);
        let args = typval_T {
            v_type: VAR_LIST,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_list(Some(std::rc::Rc::new(std::cell::RefCell::new(l)))),
        };
        let mut rv = typval_T::from(-1);
        assert_eq!(func_call("F", &args, None, None, &mut rv), OK);
        assert!(matches!(rv.vval, v_number(2)));
        // Non-list args → FAIL.
        let mut rv2 = typval_T::from(-1);
        assert_eq!(
            func_call("F", &typval_T::from(0), None, None, &mut rv2),
            FAIL
        );
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = saved);
    }

    #[test]
    fn function_exists_builtins() {
        // Builtins resolve via BUILTIN_ARGC.
        assert!(translated_function_exists("add"));
        assert!(translated_function_exists("strlen"));
        assert!(function_exists("add", true));
        // A builtin-shaped name that isn't a builtin: false.
        assert!(!translated_function_exists("notabuiltin_xyz"));
        // A user-func-shaped name with no hook registered (unit test): false.
        assert!(!translated_function_exists("MyFunc"));
        assert!(!function_exists("MyFunc", true));
    }

    #[test]
    fn get_func_tv_calls() {
        use crate::ported::eval::typval::{CALL_FUNC_HOOK, EVAL_STRING_HOOK};
        use crate::ported::eval::typval_defs_h::{typval_T, typval_vval_union::v_number};
        use crate::ported::eval_h::{FAIL, OK};
        fn eval_hook(e: &str) -> Option<typval_T> {
            e.trim().parse::<i64>().ok().map(typval_T::from)
        }
        fn call_hook(_c: &typval_T, args: &[typval_T]) -> Option<typval_T> {
            // return the sum of the (number) args
            Some(typval_T::from(
                args.iter()
                    .map(|a| match a.vval {
                        v_number(n) => n,
                        _ => 0,
                    })
                    .sum::<i64>(),
            ))
        }
        let (se, sc) = (
            EVAL_STRING_HOOK.with(|h| *h.borrow()),
            CALL_FUNC_HOOK.with(|h| *h.borrow()),
        );
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = Some(eval_hook));
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = Some(call_hook));
        let mut rv = typval_T::from(-1);
        assert_eq!(get_func_tv("F", "2, 3, 4", &mut rv), OK);
        assert!(matches!(rv.vval, v_number(9)));
        // a bad argument → FAIL
        assert_eq!(get_func_tv("F", "1, bad", &mut rv), FAIL);
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = se);
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = sc);
    }

    #[test]
    fn get_func_arguments_eval() {
        use crate::ported::eval::typval::EVAL_STRING_HOOK;
        use crate::ported::eval::typval_defs_h::{typval_T, typval_vval_union::v_number};
        fn hook(e: &str) -> Option<typval_T> {
            e.trim().parse::<i64>().ok().map(typval_T::from)
        }
        let saved = EVAL_STRING_HOOK.with(|h| *h.borrow());
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = Some(hook));
        let v = get_func_arguments("1, 2, 3").unwrap();
        assert_eq!(v.len(), 3);
        assert!(matches!(v[2].vval, v_number(3)));
        // empty arg list
        assert_eq!(get_func_arguments("").unwrap().len(), 0);
        // a non-evaluable arg → None
        assert!(get_func_arguments("1, bad").is_none());
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = saved);
    }

    #[test]
    fn get_function_args_parsing() {
        // names, a default, and varargs
        let (names, defaults, va) = get_function_args("a, b = 1 + 2, ...").unwrap();
        assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(defaults, vec!["1 + 2".to_string()]);
        assert!(va);
        // empty list
        assert_eq!(get_function_args(""), Some((vec![], vec![], false)));
        // a default expression with a nested comma must not split the list
        let (n2, d2, _) = get_function_args("x = f(a, b)").unwrap();
        assert_eq!(n2, vec!["x".to_string()]);
        assert_eq!(d2, vec!["f(a, b)".to_string()]);
        // invalid (reserved) name
        assert_eq!(get_function_args("firstline"), None);
    }

    #[test]
    fn one_function_arg_parsing() {
        let mut args: Vec<String> = Vec::new();
        // "foo, bar)" — consumes "foo"
        assert_eq!(one_function_arg("foo, bar)", Some(&mut args), false), 3);
        assert_eq!(args, vec!["foo".to_string()]);
        // duplicate is rejected (no advance)
        assert_eq!(one_function_arg("foo)", Some(&mut args), true), 0);
        // reserved word / digit-leading / empty → 0
        assert_eq!(one_function_arg("firstline)", None, true), 0);
        assert_eq!(one_function_arg("9bad", None, true), 0);
        assert_eq!(one_function_arg(")", None, true), 0);
        // scanning without collecting still advances
        assert_eq!(one_function_arg("baz)", None, true), 3);
    }

    #[test]
    fn argv_add_base_prepends() {
        use crate::ported::eval::typval_defs_h::typval_T;
        let a = typval_T::from(1);
        let b = typval_T::from(2);
        let base = typval_T::from(99);
        let (with, off) = argv_add_base(Some(base), &[a.clone(), b.clone()]);
        assert_eq!(off, 1);
        assert_eq!(with.len(), 3);
        // no base → unchanged, offset 0
        let (without, off0) = argv_add_base(None, &[a.clone(), b.clone()]);
        assert_eq!((without.len(), off0), (2, 0));
    }

    #[test]
    fn funccal_scope_accessors() {
        use crate::ported::eval::typval::tv_dict_add_nr;
        use crate::ported::eval::vars::{funccal_stack, FuncScope};
        funccal_stack.with(|s| s.borrow_mut().clear());
        // No function active → None.
        assert!(get_funccal_local_dict().is_none());
        assert!(get_funccal_args_ht().is_none());
        // Push a frame with one l: and one a: var.
        let mut frame = FuncScope::default();
        tv_dict_add_nr(&mut frame.fc_l_vars, "x", 1);
        tv_dict_add_nr(&mut frame.fc_l_avars, "1", 9);
        funccal_stack.with(|s| s.borrow_mut().push(frame));
        assert_eq!(get_funccal_local_ht().unwrap().len(), 1);
        assert!(get_funccal_local_dict()
            .unwrap()
            .dv_hashtab
            .contains_key("x"));
        assert!(get_funccal_args_dict()
            .unwrap()
            .dv_hashtab
            .contains_key("1"));
        funccal_stack.with(|s| s.borrow_mut().clear());
    }

    #[test]
    fn func_name_reads_active_scope() {
        use crate::ported::eval::vars::{funccal_stack, FuncScope};
        funccal_stack.with(|s| s.borrow_mut().clear());
        // No active function → empty name.
        assert_eq!(func_name(), "");
        // Push a frame named "MyFunc".
        funccal_stack.with(|s| {
            s.borrow_mut().push(FuncScope {
                fc_name: "MyFunc".into(),
                ..Default::default()
            })
        });
        assert_eq!(func_name(), "MyFunc");
        funccal_stack.with(|s| s.borrow_mut().clear());
    }

    #[test]
    fn save_restore_and_remove_funccal() {
        use crate::ported::eval::vars::{funccal_stack, FuncScope};
        // Two nested active frames.
        funccal_stack.with(|s| {
            s.borrow_mut().clear();
            s.borrow_mut().push(FuncScope::default());
            s.borrow_mut().push(FuncScope::default());
        });
        // save clears the active scope; restore brings it back intact.
        save_funccal();
        assert_eq!(funccal_stack.with(|s| s.borrow().len()), 0);
        restore_funccal();
        assert_eq!(funccal_stack.with(|s| s.borrow().len()), 2);
        // remove_funccal pops the innermost frame.
        remove_funccal();
        assert_eq!(funccal_stack.with(|s| s.borrow().len()), 1);
        funccal_stack.with(|s| s.borrow_mut().clear());
    }

    #[test]
    fn func_is_global_and_cat_name() {
        let g = ufunc_T {
            uf_name: "MyFunc".into(),
            ..Default::default()
        };
        assert!(func_is_global(&g));
        assert_eq!(cat_func_name(&g), "MyFunc");
        let sl = ufunc_T {
            uf_name: "<SNR>9_Helper".into(),
            ..Default::default()
        };
        assert!(!func_is_global(&sl));
        assert_eq!(cat_func_name(&sl), "<SNR>9_Helper");
    }

    #[test]
    fn printable_name_prefers_exp() {
        let f = ufunc_T {
            uf_name: "raw".into(),
            uf_name_exp: Some("<SNR>9_raw".into()),
            ..Default::default()
        };
        assert_eq!(printable_func_name(&f), "<SNR>9_raw");
        let g = ufunc_T {
            uf_name: "plain".into(),
            ..Default::default()
        };
        assert_eq!(printable_func_name(&g), "plain");
    }

    #[test]
    fn create_funccal_links_chain() {
        use crate::ported::eval::typval_defs_h::typval_T;
        // Reset the chain.
        current_funccal.with(|c| *c.borrow_mut() = None);
        assert!(get_current_funccal().is_none());

        let outer = ufunc_T {
            uf_name: "Outer".into(),
            ..Default::default()
        };
        let inner = ufunc_T {
            uf_name: "Inner".into(),
            ..Default::default()
        };
        let fc_outer = create_funccal(&outer, Some(typval_T::from(0)));
        assert_eq!(fc_outer.borrow().fc_func.uf_name, "Outer");
        assert!(fc_outer.borrow().fc_caller.is_none());

        let fc_inner = create_funccal(&inner, Some(typval_T::from(0)));
        // current_funccal is now the inner frame.
        assert_eq!(
            get_current_funccal().unwrap().borrow().fc_func.uf_name,
            "Inner"
        );
        // get_funccal() == current_funccal (no debugger).
        assert_eq!(get_funccal().unwrap().borrow().fc_func.uf_name, "Inner");
        // fc_caller links back to the outer frame.
        assert_eq!(
            fc_inner
                .borrow()
                .fc_caller
                .as_ref()
                .unwrap()
                .borrow()
                .fc_func
                .uf_name,
            "Outer"
        );
        current_funccal.with(|c| *c.borrow_mut() = None);
    }

    #[test]
    fn do_return_sets_returned_and_rettv() {
        use crate::ported::eval::typval_defs_h::{typval_T, typval_vval_union::v_number};
        current_funccal.with(|c| *c.borrow_mut() = None);
        // No active funccal → carries out, no panic.
        assert!(do_return(false, true, Some(typval_T::from(1))));

        let fp = ufunc_T {
            uf_name: "F".into(),
            ..Default::default()
        };
        let fc = create_funccal(&fp, Some(typval_T::from(0)));
        assert!(!fc.borrow().fc_returned);
        // A :return 42 carries out and records the value.
        assert!(do_return(false, true, Some(typval_T::from(42))));
        assert!(fc.borrow().fc_returned);
        assert!(matches!(
            fc.borrow().fc_rettv.as_ref().unwrap().vval,
            v_number(42)
        ));
        // reanimate does not overwrite the stored rettv.
        assert!(do_return(true, false, Some(typval_T::from(7))));
        assert!(matches!(
            fc.borrow().fc_rettv.as_ref().unwrap().vval,
            v_number(42)
        ));
        current_funccal.with(|c| *c.borrow_mut() = None);
    }

    #[test]
    fn get_current_funccal_dict_reads_l_vars() {
        use crate::ported::eval::typval::tv_dict_add_nr;
        use indexmap::IndexMap;
        current_funccal.with(|c| *c.borrow_mut() = None);
        // No active funccal → None regardless of the ht passed.
        let empty: IndexMap<String, crate::ported::eval::typval_defs_h::typval_T> = IndexMap::new();
        assert!(get_current_funccal_dict(&empty).is_none());

        let fp = ufunc_T {
            uf_name: "F".into(),
            ..Default::default()
        };
        let fc = create_funccal(&fp, None);
        tv_dict_add_nr(&mut fc.borrow_mut().fc_l_vars, "x", 5);
        let d = get_current_funccal_dict(&empty).unwrap();
        assert!(d.dv_hashtab.contains_key("x"));
        current_funccal.with(|c| *c.borrow_mut() = None);
    }

    #[test]
    fn find_hi_in_scoped_ht_is_none() {
        // No uf_scoped in the reduced model → always None.
        assert!(find_hi_in_scoped_ht("anything").is_none());
    }
}
