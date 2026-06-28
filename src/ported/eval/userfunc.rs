//! Port of `src/nvim/eval/userfunc.c` (vendored at `csrc/eval/userfunc.c`).
//!
//! The user-function call machinery itself is driven by the bytecode bridge
//! (`b_call_user`, the `FUNCTIONS` registry); this module ports the pure
//! function-*name* classification helpers `userfunc.c` exposes — telling apart
//! builtin names, script-local (`s:`/`<SID>`) names, lambda/refcounted names,
//! and emitting a function-name error.
#![allow(non_snake_case)]

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

/// Port of `can_add_defer()` from `Src/eval/userfunc.c` — whether a `:defer` can
/// be registered (inside a running function). The bridge drives calls, so no
/// `:defer` stack is tracked here → false.
pub fn can_add_defer() -> bool {
    false
}

/// Port of `add_defer()` from `Src/eval/userfunc.c` — register a deferred call;
/// not tracked standalone, no-op.
pub fn add_defer() {}

#[cfg(test)]
mod tests {
    use super::*;

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
}
