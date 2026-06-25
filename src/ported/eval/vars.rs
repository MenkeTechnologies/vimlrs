//! Port of `src/nvim/eval/vars.c` (vendored at `csrc/eval/vars.c`).
//!
//! Variable storage and scope resolution. Ports the `g:` scope (`globvardict`),
//! the predefined `v:` constants, and the function scopes `a:` (arguments) and
//! `l:` (locals) via the `current_funccal` chain. The remaining scopes (`s:`/
//! `b:`/`w:`/`t:`, autoload, locking) land later; the reduced surface is noted.
#![allow(non_snake_case, non_upper_case_globals)]

use std::cell::RefCell;

use crate::ported::eval::typval::{tv_dict_add_tv, tv_dict_find};
use crate::ported::eval::typval_defs_h::{
    dict_T, typval_T, typval_vval_union::*, BoolVarValue::*, SpecialVarValue::*, VarLockStatus,
    VarType::*,
};

/// Reduced `funccall_T` (`typval_defs.h:299`) — one function-call activation's
/// scope dicts. The full struct (profiling, defer, breakpoints, …) is not
/// modelled; only the two scope dicts the variable lookup needs.
#[derive(Default)]
pub struct FuncScope {
    /// `dict_T fc_l_vars` — the `l:` local scope. (typval_defs.h:304)
    pub fc_l_vars: dict_T,
    /// `dict_T fc_l_avars` — the `a:` argument scope. (typval_defs.h:306)
    pub fc_l_avars: dict_T,
}

thread_local! {
    /// `static dict_T globvardict;` — Dict with `g:` variables. (vars.c:72)
    ///
    /// RUST-PORT NOTE: a per-thread store stands in for the C file-static.
    /// Exposed `pub` so the debugger's variables view can iterate it (mirrors C
    /// reading `globvardict.dv_hashtab` in `list_glob_vars`).
    pub static globvardict: RefCell<dict_T> = RefCell::new(dict_T::default());

    /// The `current_funccal` chain (`eval.c`): one [`FuncScope`] per active user
    /// function call, top = innermost. Pushed/popped by the call machinery.
    pub static funccal_stack: RefCell<Vec<FuncScope>> = const { RefCell::new(Vec::new()) };

    /// `s:` script-local scope (`SCRIPT_SV(sid)->sv_dict`). RUST-PORT NOTE: one
    /// script context in the standalone interpreter (zemacs maps multiple).
    pub static script_vars: RefCell<dict_T> = RefCell::new(dict_T::default());
    /// `b:` buffer-local (`buf_T.b_vars`). One buffer standalone.
    pub static buffer_vars: RefCell<dict_T> = RefCell::new(dict_T::default());
    /// `w:` window-local (`win_T.w_vars`). One window standalone.
    pub static window_vars: RefCell<dict_T> = RefCell::new(dict_T::default());
    /// `t:` tabpage-local (`tabpage_T.tp_vars`). One tabpage standalone.
    pub static tabpage_vars: RefCell<dict_T> = RefCell::new(dict_T::default());
}

/// Port of `set_var()` from `Src/eval/vars.c:2805`.
///
/// Set variable `name` to `tv`, resolving the scope prefix: `g:`/bare-at-script
/// level → `globvardict`; `l:`/bare-in-function → the current `l:`; `a:` is
/// read-only (E46, ignored). (`set_var` delegates to `set_var_const` in C —
/// folded in for the subset.)
pub fn set_var(name: &str, _name_len: usize, tv: typval_T, _copy: bool) {
    if let Some(key) = name.strip_prefix("g:") {
        return globvardict.with(|d| tv_dict_add_tv(&mut d.borrow_mut(), key, tv));
    }
    if let Some(key) = name.strip_prefix("s:") {
        return script_vars.with(|d| tv_dict_add_tv(&mut d.borrow_mut(), key, tv));
    }
    if let Some(key) = name.strip_prefix("b:") {
        return buffer_vars.with(|d| tv_dict_add_tv(&mut d.borrow_mut(), key, tv));
    }
    if let Some(key) = name.strip_prefix("w:") {
        return window_vars.with(|d| tv_dict_add_tv(&mut d.borrow_mut(), key, tv));
    }
    if let Some(key) = name.strip_prefix("t:") {
        return tabpage_vars.with(|d| tv_dict_add_tv(&mut d.borrow_mut(), key, tv));
    }
    if name.strip_prefix("a:").is_some() {
        // c: a: variables are read-only (E46) — silently ignore in the subset.
        return;
    }
    if let Some(key) = name.strip_prefix("l:") {
        funccal_stack.with(|s| {
            if let Some(top) = s.borrow_mut().last_mut() {
                tv_dict_add_tv(&mut top.fc_l_vars, key, tv);
            }
        });
        return;
    }
    // Bare name: the current scope — `l:` inside a function, else `g:`.
    funccal_stack.with(|s| {
        let mut stack = s.borrow_mut();
        match stack.last_mut() {
            Some(top) => tv_dict_add_tv(&mut top.fc_l_vars, name, tv),
            None => globvardict.with(|d| tv_dict_add_tv(&mut d.borrow_mut(), name, tv)),
        }
    });
}

/// Port of `eval_variable()` from `Src/eval/vars.c:2353` (read path).
///
/// Look up a variable, resolving the scope chain: `v:` constants, then the
/// prefix scopes `g:`/`a:`/`l:`, then a bare name (`l:` inside a function, `g:`
/// at script level). Returns the value (the C out-param + dictitem form is
/// restored in the later phase).
pub fn eval_variable(name: &str) -> Option<typval_T> {
    // c: v: predefined constants live in vimvardict (eval_init, eval.c:204).
    match name {
        "v:true" => {
            return Some(typval_T {
                v_type: VAR_BOOL,
                v_lock: VarLockStatus::VAR_UNLOCKED,
                vval: v_bool(kBoolVarTrue),
            })
        }
        "v:false" => {
            return Some(typval_T {
                v_type: VAR_BOOL,
                v_lock: VarLockStatus::VAR_UNLOCKED,
                vval: v_bool(kBoolVarFalse),
            })
        }
        "v:null" | "v:none" => {
            return Some(typval_T {
                v_type: VAR_SPECIAL,
                v_lock: VarLockStatus::VAR_UNLOCKED,
                vval: v_special(kSpecialVarNull),
            })
        }
        _ => {}
    }
    if let Some(key) = name.strip_prefix("g:") {
        return globvardict.with(|d| tv_dict_find(&d.borrow(), key).cloned());
    }
    if let Some(key) = name.strip_prefix("s:") {
        return script_vars.with(|d| tv_dict_find(&d.borrow(), key).cloned());
    }
    if let Some(key) = name.strip_prefix("b:") {
        return buffer_vars.with(|d| tv_dict_find(&d.borrow(), key).cloned());
    }
    if let Some(key) = name.strip_prefix("w:") {
        return window_vars.with(|d| tv_dict_find(&d.borrow(), key).cloned());
    }
    if let Some(key) = name.strip_prefix("t:") {
        return tabpage_vars.with(|d| tv_dict_find(&d.borrow(), key).cloned());
    }
    if let Some(key) = name.strip_prefix("a:") {
        return funccal_stack
            .with(|s| s.borrow().last().and_then(|f| tv_dict_find(&f.fc_l_avars, key).cloned()));
    }
    if let Some(key) = name.strip_prefix("l:") {
        return funccal_stack
            .with(|s| s.borrow().last().and_then(|f| tv_dict_find(&f.fc_l_vars, key).cloned()));
    }
    // Bare name: `l:` inside a function (locals only — no fallthrough to g:),
    // else the global scope.
    funccal_stack.with(|s| {
        let stack = s.borrow();
        match stack.last() {
            Some(top) => tv_dict_find(&top.fc_l_vars, name).cloned(),
            None => globvardict.with(|d| tv_dict_find(&d.borrow(), name).cloned()),
        }
    })
}
