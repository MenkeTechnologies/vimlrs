//! Port of `src/nvim/eval/vars.c` (vendored at `csrc/eval/vars.c`).
//!
//! Variable storage and scope resolution. Ports the `g:` scope (`globvardict`),
//! the predefined `v:` constants, and the function scopes `a:` (arguments) and
//! `l:` (locals) via the `current_funccal` chain. The remaining scopes (`s:`/
//! `b:`/`w:`/`t:`, autoload, locking) land later; the reduced surface is noted.
#![allow(non_snake_case, non_upper_case_globals)]

use std::cell::RefCell;
use std::rc::Rc;

use crate::ported::eval::typval::{
    tv_dict_add_tv, tv_dict_alloc, tv_dict_find, tv_get_string, tv_list_alloc,
    tv_list_append_string,
};
use crate::ported::eval::typval_defs_h::{
    dict_T, list_T, partial_T, typval_T, typval_vval_union::*, varnumber_T, BoolVarValue,
    BoolVarValue::*, SpecialVarValue, SpecialVarValue::*, VarLockStatus, VarType, VarType::*,
    VARNUMBER_MAX, VARNUMBER_MIN, VAR_TYPE_BLOB, VAR_TYPE_BOOL, VAR_TYPE_DICT, VAR_TYPE_FLOAT,
    VAR_TYPE_FUNC, VAR_TYPE_LIST, VAR_TYPE_NUMBER, VAR_TYPE_STRING,
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
    /// `ufunc_T *fc_func`'s name (`typval_defs.h:300`) — the called function's
    /// name, for error messages / `v:throwpoint` (`func_name`).
    pub fc_name: String,
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
    if let Some(key) = name.strip_prefix("v:") {
        // c: v: variables — set the vimvars slot; RO entries decline (E46 in C).
        if let Some(idx) = VIMVARS_DEF.iter().position(|&(n, _, _)| n == key) {
            if VIMVARS_DEF[idx].2 & (VV_RO | VV_RO_SBX) == 0 {
                set_vim_var_tv(idx, tv);
            }
        }
        return;
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
    // A `VAR_DICT` snapshot of a scope dict, for a bare scope reference used as a
    // Dict (`keys(g:)`, `get(b:, …)`). Vim exposes the live scope dict; this is a
    // read snapshot (covers introspection, not mutation-through-the-dict).
    let scope_snapshot = |d: &dict_T| -> typval_T {
        let nd = crate::ported::eval::typval::tv_dict_alloc();
        {
            let mut b = nd.borrow_mut();
            for (k, v) in d.dv_hashtab.iter() {
                b.dv_hashtab.insert(k.clone(), v.clone());
            }
        }
        typval_T {
            v_type: VAR_DICT,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_dict(Some(nd)),
        }
    };
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
        "v:null" => {
            return Some(typval_T {
                v_type: VAR_SPECIAL,
                v_lock: VarLockStatus::VAR_UNLOCKED,
                vval: v_special(kSpecialVarNull),
            })
        }
        "v:none" => {
            return Some(typval_T {
                v_type: VAR_SPECIAL,
                v_lock: VarLockStatus::VAR_UNLOCKED,
                vval: v_special(SpecialVarValue::kSpecialVarNone),
            })
        }
        _ => {}
    }
    if let Some(key) = name.strip_prefix("g:") {
        return globvardict.with(|d| {
            let d = d.borrow();
            if key.is_empty() {
                Some(scope_snapshot(&d))
            } else {
                tv_dict_find(&d, key).cloned()
            }
        });
    }
    if let Some(key) = name.strip_prefix("s:") {
        return script_vars.with(|d| {
            let d = d.borrow();
            if key.is_empty() {
                Some(scope_snapshot(&d))
            } else {
                tv_dict_find(&d, key).cloned()
            }
        });
    }
    if let Some(key) = name.strip_prefix("b:") {
        return buffer_vars.with(|d| {
            let d = d.borrow();
            if key.is_empty() {
                Some(scope_snapshot(&d))
            } else {
                tv_dict_find(&d, key).cloned()
            }
        });
    }
    if let Some(key) = name.strip_prefix("w:") {
        return window_vars.with(|d| {
            let d = d.borrow();
            if key.is_empty() {
                Some(scope_snapshot(&d))
            } else {
                tv_dict_find(&d, key).cloned()
            }
        });
    }
    if let Some(key) = name.strip_prefix("t:") {
        return tabpage_vars.with(|d| {
            let d = d.borrow();
            if key.is_empty() {
                Some(scope_snapshot(&d))
            } else {
                tv_dict_find(&d, key).cloned()
            }
        });
    }
    if let Some(key) = name.strip_prefix("a:") {
        return funccal_stack.with(|s| {
            let s = s.borrow();
            let f = s.last()?;
            if key.is_empty() {
                Some(scope_snapshot(&f.fc_l_avars))
            } else {
                tv_dict_find(&f.fc_l_avars, key).cloned()
            }
        });
    }
    if let Some(key) = name.strip_prefix("l:") {
        return funccal_stack.with(|s| {
            let s = s.borrow();
            let f = s.last()?;
            if key.is_empty() {
                Some(scope_snapshot(&f.fc_l_vars))
            } else {
                tv_dict_find(&f.fc_l_vars, key).cloned()
            }
        });
    }
    // c: v: variables live in `vimvardict` — consult the vimvars store. Returns
    // None for VAR_UNKNOWN entries (v:val/v:key, which the bridge supplies
    // dynamically) and for unknown v: names.
    if let Some(key) = name.strip_prefix("v:") {
        if let Some(idx) = VIMVARS_DEF.iter().position(|&(n, _, _)| n == key) {
            let tv = get_vim_var_tv(idx);
            if tv.v_type != VAR_UNKNOWN {
                return Some(tv);
            }
        }
        return None;
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

// ── v: variables (`vimvardict` / vimvars[] table) ───────────────────────────
//
// Port of the `v:` variable subsystem from `eval/vars.c` (the `vimvars[]` table,
// the `VimVarIndex` defines from `eval_defs.h`, and the get/set accessors).
//
// RUST-PORT NOTE: C holds the values inline in `vimvars[idx].vv_tv` and the
// accessors return `&vimvars[idx].vv_tv` (a mutable pointer). A `thread_local`
// `Vec<VimVar>` stands in for the file-static array; `get_vim_var_tv` returns a
// clone and the setters write the slot, which reproduces the observable
// behavior. `VimVarIndex` is a `usize` alias with `VV_*` index constants (the C
// enum) rather than a Rust enum, so the table can be indexed directly.

/// `#define VV_COMPAT 1` — compatible, also usable without `v:`. (vars.c:81)
const VV_COMPAT: u8 = 1;
/// `#define VV_RO 2` — read-only. (vars.c:82)
const VV_RO: u8 = 2;
/// `#define VV_RO_SBX 4` — read-only in the sandbox. (vars.c:83)
const VV_RO_SBX: u8 = 4;

/// `typedef enum { VV_COUNT, … } VimVarIndex;` (eval_defs.h) — the index into
/// `vimvars[]`. Modeled as a `usize` alias; the `VV_*` constants below give the
/// enum values in declaration order.
pub type VimVarIndex = usize;

/// `VV_*` indices into [`vimvars`], in `eval_defs.h` declaration order.
pub mod vv {
    use super::VimVarIndex;
    macro_rules! vv_indices {
        ($($name:ident),+ $(,)?) => { vv_indices!(@n 0usize; $($name),+); };
        (@n $n:expr; $name:ident $(, $rest:ident)*) => {
            pub const $name: VimVarIndex = $n;
            vv_indices!(@n $n + 1; $($rest),*);
        };
        (@n $n:expr;) => { /// Number of `v:` variables (`ARRAY_SIZE(vimvars)`).
            pub const VV_LEN: VimVarIndex = $n; };
    }
    vv_indices!(
        VV_COUNT,
        VV_COUNT1,
        VV_PREVCOUNT,
        VV_ERRMSG,
        VV_WARNINGMSG,
        VV_STATUSMSG,
        VV_SHELL_ERROR,
        VV_THIS_SESSION,
        VV_VERSION,
        VV_LNUM,
        VV_TERMREQUEST,
        VV_TERMRESPONSE,
        VV_FNAME,
        VV_LANG,
        VV_LC_TIME,
        VV_CTYPE,
        VV_CC_FROM,
        VV_CC_TO,
        VV_FNAME_IN,
        VV_FNAME_OUT,
        VV_FNAME_NEW,
        VV_FNAME_DIFF,
        VV_CMDARG,
        VV_FOLDSTART,
        VV_FOLDEND,
        VV_FOLDDASHES,
        VV_FOLDLEVEL,
        VV_PROGNAME,
        VV_SEND_SERVER,
        VV_DYING,
        VV_EXCEPTION,
        VV_THROWPOINT,
        VV_REG,
        VV_CMDBANG,
        VV_INSERTMODE,
        VV_VAL,
        VV_KEY,
        VV_PROFILING,
        VV_FCS_REASON,
        VV_FCS_CHOICE,
        VV_BEVAL_BUFNR,
        VV_BEVAL_WINNR,
        VV_BEVAL_WINID,
        VV_BEVAL_LNUM,
        VV_BEVAL_COL,
        VV_BEVAL_TEXT,
        VV_SCROLLSTART,
        VV_SWAPNAME,
        VV_SWAPCHOICE,
        VV_SWAPCOMMAND,
        VV_CHAR,
        VV_MOUSE_WIN,
        VV_MOUSE_WINID,
        VV_MOUSE_LNUM,
        VV_MOUSE_COL,
        VV_OP,
        VV_SEARCHFORWARD,
        VV_HLSEARCH,
        VV_OLDFILES,
        VV_WINDOWID,
        VV_PROGPATH,
        VV_COMPLETED_ITEM,
        VV_OPTION_NEW,
        VV_OPTION_OLD,
        VV_OPTION_OLDLOCAL,
        VV_OPTION_OLDGLOBAL,
        VV_OPTION_COMMAND,
        VV_OPTION_TYPE,
        VV_ERRORS,
        VV_FALSE,
        VV_TRUE,
        VV_NULL,
        VV_NUMBERMAX,
        VV_NUMBERMIN,
        VV_NUMBERSIZE,
        VV_VIM_DID_ENTER,
        VV_TESTING,
        VV_TYPE_NUMBER,
        VV_TYPE_STRING,
        VV_TYPE_FUNC,
        VV_TYPE_LIST,
        VV_TYPE_DICT,
        VV_TYPE_FLOAT,
        VV_TYPE_BOOL,
        VV_TYPE_BLOB,
        VV_EVENT,
        VV_VERSIONLONG,
        VV_ECHOSPACE,
        VV_ARGF,
        VV_ARGV,
        VV_COLLATE,
        VV_EXITING,
        VV_MAXCOL,
        VV_STACKTRACE,
        VV_VIM_DID_INIT,
        VV_STDERR,
        VV_MSGPACK_TYPES,
        VV_NULL_STRING,
        VV_NULL_LIST,
        VV_NULL_DICT,
        VV_NULL_BLOB,
        VV_LUA,
        VV_RELNUM,
        VV_VIRTNUM,
        VV_STARTTIME,
        VV_EXITREASON,
        VV_USERACTIVE,
        VV_STARTREASON,
    );
}
use vv::*;

/// One `vimvars[]` entry (`struct vimvar`, vars.c:102): the name, the held value,
/// and the `VV_*` flags.
pub struct VimVar {
    /// `char *vv_name` — name without the `v:` prefix.
    pub vv_name: &'static str,
    /// `typval_T vv_tv` (`vv_di.di_tv`) — the value (its `v_type` is the slot's
    /// declared type until set).
    pub vv_tv: typval_T,
    /// `char vv_flags` — VV_COMPAT / VV_RO / VV_RO_SBX.
    pub vv_flags: u8,
}

/// `vimvars[]` declared (name, type, flags), in `VimVarIndex` order (vars.c:106).
/// The value defaults are applied by [`seed_vimvars`] (the `evalvars_init` body).
const VIMVARS_DEF: [(&str, VarType, u8); VV_LEN] = [
    ("count", VAR_NUMBER, VV_RO),
    ("count1", VAR_NUMBER, VV_RO),
    ("prevcount", VAR_NUMBER, VV_RO),
    ("errmsg", VAR_STRING, 0),
    ("warningmsg", VAR_STRING, 0),
    ("statusmsg", VAR_STRING, 0),
    ("shell_error", VAR_NUMBER, VV_RO),
    ("this_session", VAR_STRING, 0),
    ("version", VAR_NUMBER, VV_COMPAT + VV_RO),
    ("lnum", VAR_NUMBER, VV_RO_SBX),
    ("termrequest", VAR_STRING, VV_RO),
    ("termresponse", VAR_STRING, VV_RO),
    ("fname", VAR_STRING, VV_RO),
    ("lang", VAR_STRING, VV_RO),
    ("lc_time", VAR_STRING, VV_RO),
    ("ctype", VAR_STRING, VV_RO),
    ("charconvert_from", VAR_STRING, VV_RO),
    ("charconvert_to", VAR_STRING, VV_RO),
    ("fname_in", VAR_STRING, VV_RO),
    ("fname_out", VAR_STRING, VV_RO),
    ("fname_new", VAR_STRING, VV_RO),
    ("fname_diff", VAR_STRING, VV_RO),
    ("cmdarg", VAR_STRING, VV_RO),
    ("foldstart", VAR_NUMBER, VV_RO_SBX),
    ("foldend", VAR_NUMBER, VV_RO_SBX),
    ("folddashes", VAR_STRING, VV_RO_SBX),
    ("foldlevel", VAR_NUMBER, VV_RO_SBX),
    ("progname", VAR_STRING, VV_RO),
    ("servername", VAR_STRING, VV_RO),
    ("dying", VAR_NUMBER, VV_RO),
    ("exception", VAR_STRING, VV_RO),
    ("throwpoint", VAR_STRING, VV_RO),
    ("register", VAR_STRING, VV_RO),
    ("cmdbang", VAR_NUMBER, VV_RO),
    ("insertmode", VAR_STRING, VV_RO),
    ("val", VAR_UNKNOWN, VV_RO),
    ("key", VAR_UNKNOWN, VV_RO),
    ("profiling", VAR_NUMBER, VV_RO),
    ("fcs_reason", VAR_STRING, VV_RO),
    ("fcs_choice", VAR_STRING, 0),
    ("beval_bufnr", VAR_NUMBER, VV_RO),
    ("beval_winnr", VAR_NUMBER, VV_RO),
    ("beval_winid", VAR_NUMBER, VV_RO),
    ("beval_lnum", VAR_NUMBER, VV_RO),
    ("beval_col", VAR_NUMBER, VV_RO),
    ("beval_text", VAR_STRING, VV_RO),
    ("scrollstart", VAR_STRING, 0),
    ("swapname", VAR_STRING, VV_RO),
    ("swapchoice", VAR_STRING, 0),
    ("swapcommand", VAR_STRING, VV_RO),
    ("char", VAR_STRING, 0),
    ("mouse_win", VAR_NUMBER, 0),
    ("mouse_winid", VAR_NUMBER, 0),
    ("mouse_lnum", VAR_NUMBER, 0),
    ("mouse_col", VAR_NUMBER, 0),
    ("operator", VAR_STRING, VV_RO),
    ("searchforward", VAR_NUMBER, 0),
    ("hlsearch", VAR_NUMBER, 0),
    ("oldfiles", VAR_LIST, 0),
    ("windowid", VAR_NUMBER, VV_RO_SBX),
    ("progpath", VAR_STRING, VV_RO),
    ("completed_item", VAR_DICT, 0),
    ("option_new", VAR_STRING, VV_RO),
    ("option_old", VAR_STRING, VV_RO),
    ("option_oldlocal", VAR_STRING, VV_RO),
    ("option_oldglobal", VAR_STRING, VV_RO),
    ("option_command", VAR_STRING, VV_RO),
    ("option_type", VAR_STRING, VV_RO),
    ("errors", VAR_LIST, 0),
    ("false", VAR_BOOL, VV_RO),
    ("true", VAR_BOOL, VV_RO),
    ("null", VAR_SPECIAL, VV_RO),
    ("numbermax", VAR_NUMBER, VV_RO),
    ("numbermin", VAR_NUMBER, VV_RO),
    ("numbersize", VAR_NUMBER, VV_RO),
    ("vim_did_enter", VAR_NUMBER, VV_RO),
    ("testing", VAR_NUMBER, 0),
    ("t_number", VAR_NUMBER, VV_RO),
    ("t_string", VAR_NUMBER, VV_RO),
    ("t_func", VAR_NUMBER, VV_RO),
    ("t_list", VAR_NUMBER, VV_RO),
    ("t_dict", VAR_NUMBER, VV_RO),
    ("t_float", VAR_NUMBER, VV_RO),
    ("t_bool", VAR_NUMBER, VV_RO),
    ("t_blob", VAR_NUMBER, VV_RO),
    ("event", VAR_DICT, VV_RO),
    ("versionlong", VAR_NUMBER, VV_RO),
    ("echospace", VAR_NUMBER, VV_RO),
    ("argf", VAR_LIST, VV_RO),
    ("argv", VAR_LIST, VV_RO),
    ("collate", VAR_STRING, VV_RO),
    ("exiting", VAR_NUMBER, VV_RO),
    ("maxcol", VAR_NUMBER, VV_RO),
    ("stacktrace", VAR_LIST, VV_RO),
    ("vim_did_init", VAR_NUMBER, VV_RO),
    ("stderr", VAR_NUMBER, VV_RO),
    ("msgpack_types", VAR_DICT, VV_RO),
    ("_null_string", VAR_STRING, VV_RO),
    ("_null_list", VAR_LIST, VV_RO),
    ("_null_dict", VAR_DICT, VV_RO),
    ("_null_blob", VAR_BLOB, VV_RO),
    ("lua", VAR_PARTIAL, VV_RO),
    ("relnum", VAR_NUMBER, VV_RO),
    ("virtnum", VAR_NUMBER, VV_RO),
    ("starttime", VAR_NUMBER, VV_RO),
    ("exitreason", VAR_STRING, VV_RO),
    ("useractive", VAR_NUMBER, VV_RO),
    ("startreason", VAR_STRING, VV_RO),
];

thread_local! {
    /// `static struct vimvar vimvars[];` — the `v:` variable store, built from
    /// [`VIMVARS_DEF`] with the type-zero defaults (`{ .v_type = type }`: number
    /// 0, NULL string → `""`, NULL container → `None`). The value defaults are
    /// applied by [`evalvars_init`], which the bridge `install()` runs before any
    /// script executes.
    pub static vimvars: RefCell<Vec<VimVar>> = RefCell::new(
        VIMVARS_DEF
            .iter()
            .map(|&(name, t, flags)| {
                let vval = match t {
                    VAR_NUMBER => v_number(0),
                    VAR_FLOAT => v_float(0.0),
                    VAR_STRING => v_string(String::new()),
                    VAR_BOOL => v_bool(BoolVarValue::kBoolVarFalse),
                    VAR_SPECIAL => v_special(SpecialVarValue::kSpecialVarNull),
                    VAR_LIST => v_list(None),
                    VAR_DICT => v_dict(None),
                    VAR_BLOB => v_blob(None),
                    VAR_PARTIAL => v_partial(None),
                    _ => v_unknown,
                };
                VimVar {
                    vv_name: name,
                    vv_tv: typval_T { v_type: t, v_lock: VarLockStatus::VAR_UNLOCKED, vval },
                    vv_flags: flags,
                }
            })
            .collect()
    );
}

/// Port of `evalvars_init()` from `Src/eval/vars.c:261` — apply the value
/// defaults to the `v:` store. Idempotent (re-seed = reset). Editor-coupled
/// fields (`echospace`=`sc_col-1`, `progpath`, `servername`, …) keep their
/// type-zero default in the standalone interpreter.
pub fn evalvars_init() {
    // c: VIM_VERSION_MAJOR * 100 + VIM_VERSION_MINOR (the Vim compat floor).
    const VIM_VERSION_MAJOR: varnumber_T = 8;
    const VIM_VERSION_MINOR: varnumber_T = 1;
    let vim_version = VIM_VERSION_MAJOR * 100 + VIM_VERSION_MINOR;

    // Containers are allocated here (no `vimvars` borrow involved).
    // c: v:msgpack_types — {nil:[], boolean:[], …, ext:[]} (8 empty lists).
    let mpd = tv_dict_alloc();
    for name in [
        "nil", "boolean", "integer", "float", "string", "array", "map", "ext",
    ] {
        let lst = tv_list_alloc(0);
        let tv = typval_T {
            v_type: VAR_LIST,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_list(Some(lst)),
        };
        tv_dict_add_tv(&mut mpd.borrow_mut(), name, tv);
    }

    // c: rebuild from the declared type-zero defaults (so re-init fully resets
    // mutable v: vars), then apply the value defaults below.
    let mut v: Vec<VimVar> = VIMVARS_DEF
        .iter()
        .map(|&(name, t, flags)| {
            let vval = match t {
                VAR_NUMBER => v_number(0),
                VAR_FLOAT => v_float(0.0),
                VAR_STRING => v_string(String::new()),
                VAR_BOOL => v_bool(BoolVarValue::kBoolVarFalse),
                VAR_SPECIAL => v_special(SpecialVarValue::kSpecialVarNull),
                VAR_LIST => v_list(None),
                VAR_DICT => v_dict(None),
                VAR_BLOB => v_blob(None),
                VAR_PARTIAL => v_partial(None),
                _ => v_unknown,
            };
            VimVar {
                vv_name: name,
                vv_tv: typval_T {
                    v_type: t,
                    v_lock: VarLockStatus::VAR_UNLOCKED,
                    vval,
                },
                vv_flags: flags,
            }
        })
        .collect();
    {
        let num = |n: varnumber_T| typval_T {
            v_type: VAR_NUMBER,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_number(n),
        };
        v[VV_VERSION].vv_tv = num(vim_version);
        // c: vim_version * 10000 + highest_patch(); no patch table standalone → 0.
        v[VV_VERSIONLONG].vv_tv = num(vim_version * 10000);

        v[VV_MSGPACK_TYPES].vv_tv = typval_T {
            v_type: VAR_DICT,
            v_lock: VarLockStatus::VAR_FIXED,
            vval: v_dict(Some(mpd)),
        };
        v[VV_COMPLETED_ITEM].vv_tv = typval_T {
            v_type: VAR_DICT,
            v_lock: VarLockStatus::VAR_FIXED,
            vval: v_dict(Some(tv_dict_alloc())),
        };
        v[VV_EVENT].vv_tv = typval_T {
            v_type: VAR_DICT,
            v_lock: VarLockStatus::VAR_FIXED,
            vval: v_dict(Some(tv_dict_alloc())),
        };
        v[VV_ERRORS].vv_tv = typval_T {
            v_type: VAR_LIST,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_list(Some(tv_list_alloc(0))),
        };

        v[VV_STDERR].vv_tv = num(2); // CHAN_STDERR
        v[VV_SEARCHFORWARD].vv_tv = num(1);
        v[VV_HLSEARCH].vv_tv = num(1);
        v[VV_COUNT1].vv_tv = num(1);
        v[VV_STARTREASON].vv_tv = typval_T::from("normal".to_string());
        v[VV_EXITING].vv_tv = typval_T {
            v_type: VAR_SPECIAL,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_special(SpecialVarValue::kSpecialVarNull),
        };

        v[VV_TYPE_NUMBER].vv_tv = num(VAR_TYPE_NUMBER);
        v[VV_TYPE_STRING].vv_tv = num(VAR_TYPE_STRING);
        v[VV_TYPE_FUNC].vv_tv = num(VAR_TYPE_FUNC);
        v[VV_TYPE_LIST].vv_tv = num(VAR_TYPE_LIST);
        v[VV_TYPE_DICT].vv_tv = num(VAR_TYPE_DICT);
        v[VV_TYPE_FLOAT].vv_tv = num(VAR_TYPE_FLOAT);
        v[VV_TYPE_BOOL].vv_tv = num(VAR_TYPE_BOOL);
        v[VV_TYPE_BLOB].vv_tv = num(VAR_TYPE_BLOB);

        v[VV_FALSE].vv_tv = typval_T {
            v_type: VAR_BOOL,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_bool(BoolVarValue::kBoolVarFalse),
        };
        v[VV_TRUE].vv_tv = typval_T {
            v_type: VAR_BOOL,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_bool(BoolVarValue::kBoolVarTrue),
        };
        v[VV_NULL].vv_tv = typval_T {
            v_type: VAR_SPECIAL,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_special(SpecialVarValue::kSpecialVarNull),
        };
        v[VV_NUMBERMAX].vv_tv = num(VARNUMBER_MAX);
        v[VV_NUMBERMIN].vv_tv = num(VARNUMBER_MIN);
        v[VV_NUMBERSIZE].vv_tv = num(64); // sizeof(varnumber_T) * 8
        v[VV_MAXCOL].vv_tv = num(i32::MAX as varnumber_T); // MAXCOL

        // c: set_vim_var_partial(VV_LUA, …) — a partial with an empty name.
        v[VV_LUA].vv_tv = typval_T {
            v_type: VAR_PARTIAL,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_partial(Some(Rc::new(partial_T {
                pt_refcount: 1,
                pt_name: String::new(),
                pt_argv: Vec::new(),
                pt_dict: None,
            }))),
        };
        // c: set_reg_var(0) → v:register defaults to '"'.
        v[VV_REG].vv_tv = typval_T::from("\"".to_string());
    }
    vimvars.with(|s| *s.borrow_mut() = v);
}

/// Port of `get_vim_var_tv()` from `Src/eval/vars.c:1892`. RUST-PORT NOTE:
/// returns a clone (C returns `&vimvars[idx].vv_tv`).
pub fn get_vim_var_tv(idx: VimVarIndex) -> typval_T {
    vimvars.with(|s| s.borrow()[idx].vv_tv.clone())
}

/// Port of `set_vim_var_tv()` from `Src/eval/vars.c:1878`.
pub fn set_vim_var_tv(idx: VimVarIndex, tv: typval_T) {
    vimvars.with(|s| s.borrow_mut()[idx].vv_tv = tv);
}

/// Port of `get_vim_var_name()` from `Src/eval/vars.c:1885`.
pub fn get_vim_var_name(idx: VimVarIndex) -> &'static str {
    VIMVARS_DEF[idx].0
}

/// Port of `get_vim_var_nr()` from `Src/eval/vars.c:1898`.
pub fn get_vim_var_nr(idx: VimVarIndex) -> varnumber_T {
    match get_vim_var_tv(idx).vval {
        v_number(n) => n,
        _ => 0,
    }
}

/// Port of `get_vim_var_list()` from `Src/eval/vars.c:1906`.
pub fn get_vim_var_list(idx: VimVarIndex) -> Option<Rc<RefCell<list_T>>> {
    match get_vim_var_tv(idx).vval {
        v_list(l) => l,
        _ => None,
    }
}

/// Port of `assert_error()` from `Src/eval/vars.c:3360` — append an assertion
/// failure message to `v:errors`, first making `v:errors` a List if it is not
/// one yet. (The C `garray_T *gap` accumulates the message bytes; the port
/// receives the already-built message.)
pub fn assert_error(msg: &str) {
    let list = match get_vim_var_list(vv::VV_ERRORS) {
        Some(l) => l,
        None => {
            let l = tv_list_alloc(1);
            set_vim_var_list(vv::VV_ERRORS, Some(Rc::clone(&l)));
            l
        }
    };
    tv_list_append_string(&mut list.borrow_mut(), msg);
}

/// Port of `get_vim_var_dict()` from `Src/eval/vars.c:1914`.
pub fn get_vim_var_dict(idx: VimVarIndex) -> Option<Rc<RefCell<dict_T>>> {
    match get_vim_var_tv(idx).vval {
        v_dict(d) => d,
        _ => None,
    }
}

/// Port of `get_vim_var_str()` from `Src/eval/vars.c:1923` — never NULL.
pub fn get_vim_var_str(idx: VimVarIndex) -> String {
    tv_get_string(&get_vim_var_tv(idx))
}

/// Port of `get_vim_var_partial()` from `Src/eval/vars.c:1931`.
pub fn get_vim_var_partial(idx: VimVarIndex) -> Option<Rc<partial_T>> {
    match get_vim_var_tv(idx).vval {
        v_partial(p) => p,
        _ => None,
    }
}

/// Port of `set_vim_var_type()` from `Src/eval/vars.c:2050`.
pub fn set_vim_var_type(idx: VimVarIndex, t: VarType) {
    vimvars.with(|s| s.borrow_mut()[idx].vv_tv.v_type = t);
}

/// Port of `set_vim_var_nr()` from `Src/eval/vars.c:2061`.
pub fn set_vim_var_nr(idx: VimVarIndex, val: varnumber_T) {
    set_vim_var_tv(
        idx,
        typval_T {
            v_type: get_vim_var_tv(idx).v_type,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_number(val),
        },
    );
}

/// Port of `set_vim_var_bool()` from `Src/eval/vars.c:2072`.
pub fn set_vim_var_bool(idx: VimVarIndex, val: BoolVarValue) {
    set_vim_var_tv(
        idx,
        typval_T {
            v_type: VAR_BOOL,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_bool(val),
        },
    );
}

/// Port of `set_vim_var_special()` from `Src/eval/vars.c:2084`.
pub fn set_vim_var_special(idx: VimVarIndex, val: SpecialVarValue) {
    set_vim_var_tv(
        idx,
        typval_T {
            v_type: VAR_SPECIAL,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_special(val),
        },
    );
}

/// Port of `set_vim_var_char()` from `Src/eval/vars.c:2093` — set v:char.
pub fn set_vim_var_char(c: char) {
    set_vim_var_string(VV_CHAR, &c.to_string());
}

/// Port of `set_vim_var_string()` from `Src/eval/vars.c:2107`. RUST-PORT NOTE:
/// `&str` carries its own length (the C `len`/`-1` distinction is unnecessary).
pub fn set_vim_var_string(idx: VimVarIndex, val: &str) {
    set_vim_var_tv(idx, typval_T::from(val.to_string()));
}

/// Port of `set_vim_var_list()` from `Src/eval/vars.c:2125`.
pub fn set_vim_var_list(idx: VimVarIndex, val: Option<Rc<RefCell<list_T>>>) {
    set_vim_var_tv(
        idx,
        typval_T {
            v_type: VAR_LIST,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_list(val),
        },
    );
}

/// Port of `set_vim_var_dict()` from `Src/eval/vars.c:2141`.
pub fn set_vim_var_dict(idx: VimVarIndex, val: Option<Rc<RefCell<dict_T>>>) {
    set_vim_var_tv(
        idx,
        typval_T {
            v_type: VAR_DICT,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_dict(val),
        },
    );
}

/// Port of `set_vim_var_partial()` from `Src/eval/vars.c:2161`. Note: does not
/// set the type (use [`set_vim_var_type`]), matching C.
pub fn set_vim_var_partial(idx: VimVarIndex, val: Option<Rc<partial_T>>) {
    vimvars.with(|s| s.borrow_mut()[idx].vv_tv.vval = v_partial(val));
}

// ── Misc eval/vars.c helpers (GC is a no-op under Rc; providers absent) ──

/// Port of `set_internal_string_var()` from `Src/eval/vars.c` — set a global
/// variable `name` to the String `value`.
pub fn set_internal_string_var(name: &str, value: &str) {
    set_var(
        name,
        name.len(),
        typval_T {
            v_type: VAR_STRING,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_string(value.to_string()),
        },
        true,
    );
}

/// Port of `prepare_vimvar()` from `Src/eval/vars.c` — save and return the
/// current value of `v:`-variable `idx` (so a temporary value can be set around
/// an evaluation). Pair with [`restore_vimvar`].
pub fn prepare_vimvar(idx: VimVarIndex) -> typval_T {
    get_vim_var_tv(idx)
}

/// Port of `restore_vimvar()` from `Src/eval/vars.c` — restore the value saved
/// by [`prepare_vimvar`].
pub fn restore_vimvar(idx: VimVarIndex, save_tv: typval_T) {
    set_vim_var_tv(idx, save_tv);
}

/// Port of `garbage_collect_globvars()` — the value layer is reference-counted
/// (`Rc`), so there is no mark-and-sweep pass; nothing is freed → 0.
pub fn garbage_collect_globvars(_copy_id: i32) -> i32 {
    0
}
/// Port of `garbage_collect_vimvars()` — no GC pass needed (Rc) → false.
pub fn garbage_collect_vimvars(_copy_id: i32) -> bool {
    false
}
/// Port of `garbage_collect_scriptvars()` — no GC pass needed (Rc) → false.
pub fn garbage_collect_scriptvars(_copy_id: i32) -> bool {
    false
}

/// Port of `eval_charconvert()` — no `'charconvert'` expression standalone, so
/// the conversion cannot run → FAIL.
pub fn eval_charconvert(_from: &str, _to: &str, _fname_from: &str, _fname_to: &str) -> i32 {
    crate::ported::eval_h::FAIL
}
/// Port of `eval_diff()` — no `'diffexpr'` standalone → no-op.
pub fn eval_diff(_orig: &str, _new: &str, _out: &str) {}
/// Port of `eval_patch()` — no `'patchexpr'` standalone → no-op.
pub fn eval_patch(_orig: &str, _diff: &str, _out: &str) {}
/// Port of `eval_spell_expr()` — no `'spellsuggest'` expression standalone, so
/// there are no suggestions → NULL list.
pub fn eval_spell_expr(_badword: &str, _expr: &str) -> Option<Rc<RefCell<list_T>>> {
    None
}
/// Port of `list_vim_vars()` — interactive `:let` listing; no-op standalone.
pub fn list_vim_vars(_first: &mut i32) {}
/// Port of `list_script_vars()` — interactive `:let` listing; no-op standalone.
pub fn list_script_vars(_first: &mut i32) {}

// ── more vars.c helpers (unlet, funcref-name check, reg var, clears) ──

/// Port of `do_unlet()` from `Src/eval/vars.c` — delete variable `name` from its
/// scope. Returns OK if removed (or `forceit`), FAIL if it did not exist.
pub fn do_unlet(name: &str, _name_len: usize, forceit: bool) -> i32 {
    let ok = crate::ported::eval_h::OK;
    let fail = crate::ported::eval_h::FAIL;
    let rm = |store: &'static std::thread::LocalKey<RefCell<dict_T>>, key: &str| -> i32 {
        store.with(|d| {
            if d.borrow_mut().dv_hashtab.shift_remove(key).is_some() || forceit {
                ok
            } else {
                fail
            }
        })
    };
    if let Some(k) = name.strip_prefix("g:") {
        return rm(&globvardict, k);
    }
    if let Some(k) = name.strip_prefix("s:") {
        return rm(&script_vars, k);
    }
    if let Some(k) = name.strip_prefix("b:") {
        return rm(&buffer_vars, k);
    }
    if let Some(k) = name.strip_prefix("w:") {
        return rm(&window_vars, k);
    }
    if let Some(k) = name.strip_prefix("t:") {
        return rm(&tabpage_vars, k);
    }
    // Bare name: current function-local scope, else global.
    let in_func = funccal_stack.with(|s| {
        s.borrow_mut()
            .last_mut()
            .map(|top| top.fc_l_vars.dv_hashtab.shift_remove(name).is_some())
    });
    match in_func {
        Some(true) => ok,
        Some(false) => {
            if forceit {
                ok
            } else {
                fail
            }
        }
        None => rm(&globvardict, name),
    }
}

/// Port of `var_wrong_func_name()` from `Src/eval/vars.c` — true (with E704) when
/// `name` is an invalid Funcref variable name: it must start with a capital, or
/// be a `w:`/`b:`/`s:`/`t:` scope or an autoload (`#`) name.
pub fn var_wrong_func_name(name: &str, _new_var: bool) -> bool {
    let b = name.as_bytes();
    let scoped = b.first().is_some_and(|c| b"wbst".contains(c)) && b.get(1) == Some(&b':');
    let lead = if b.first().is_some_and(|&c| c != 0) && b.get(1) == Some(&b':') {
        b.get(2).copied().unwrap_or(0)
    } else {
        b.first().copied().unwrap_or(0)
    };
    if !scoped && !lead.is_ascii_uppercase() && !name.contains('#') {
        crate::ported::message::semsg(&format!(
            "E704: Funcref variable name must start with a capital: {name}"
        ));
        return true;
    }
    false
}

/// Port of `set_reg_var()` from `Src/eval/vars.c` — set `v:register` to char `c`.
pub fn set_reg_var(c: u8) {
    set_vim_var_string(vv::VV_REG, &(c as char).to_string());
}

/// Port of `evalvars_clear()` from `Src/eval/vars.c` — teardown of the variable
/// stores; `Rc`/`Drop`-managed, no-op.
pub fn evalvars_clear() {}

/// Port of `del_menutrans_vars()` from `Src/eval/vars.c` — remove `v:` menu
/// translation vars; not used standalone, no-op.
pub fn del_menutrans_vars() {}

/// Port of `check_vars()` from `Src/eval/vars.c` — curly-brace name expansion
/// check; the subset has no `{}` names to expand, no-op.
pub fn check_vars(_name: &str, _len: usize) {}

// ── vars.c listing (interactive :let, no-op) + set delegations ──

/// Port of `set_var_const()` from `Src/eval/vars.c` — the `:const`-aware setter.
/// The subset does not track const locks, so it delegates to [`set_var`].
pub fn set_var_const(name: &str, name_len: usize, tv: typval_T, copy: bool, _is_const: bool) {
    set_var(name, name_len, tv, copy);
}

/// Port of `before_set_vvar()` from `Src/eval/vars.c` — hook before a `v:`
/// variable is set; nothing vetoes the set in the subset → allow (false = no
/// special handling consumed it).
pub fn before_set_vvar(_varname: &str) -> bool {
    false
}

/// Port of `list_glob_vars()` from `Src/eval/vars.c` — `:let` listing of `g:`
/// variables; no interactive listing standalone, no-op.
pub fn list_glob_vars(_first: &mut i32) {}
/// Port of `list_arg_vars()` from `Src/eval/vars.c:1210` — `:let` listing of the
/// `a:` argument scope; no interactive listing standalone, no-op.
pub fn list_arg_vars(_first: &mut i32) {}
/// Port of `list_hashtable_vars()` from `Src/eval/vars.c:1157` — `:let` listing
/// of one scope's hashtable; no interactive listing standalone, no-op.
pub fn list_hashtable_vars(_first: &mut i32) {}
/// Port of `list_buf_vars()` from `Src/eval/vars.c` — no-op.
pub fn list_buf_vars(_first: &mut i32) {}
/// Port of `list_win_vars()` from `Src/eval/vars.c` — no-op.
pub fn list_win_vars(_first: &mut i32) {}
/// Port of `list_tab_vars()` from `Src/eval/vars.c` — no-op.
pub fn list_tab_vars(_first: &mut i32) {}
/// Port of `list_one_var_a()` from `Src/eval/vars.c` — print one variable; no
/// interactive output standalone, no-op.
pub fn list_one_var_a(_prefix: &str, _name: &str, _name_len: isize) {}
/// Port of `list_one_var()` from `Src/eval/vars.c:2682` — print one variable
/// with its scope prefix; no interactive output standalone, no-op (delegates to
/// [`list_one_var_a`] in C).
pub fn list_one_var(_first: &mut i32) {}

// ── exception/throwpoint save-restore and counts (vars.c) ──

/// Port of `v_exception()` from `Src/eval/vars.c:2189`.
///
/// Save/restore the `v:exception` string. With `None` (C `oldval == NULL`),
/// returns the current value. With `Some(old)`, restores it and returns `None`.
/// Always called in pairs by the `:try`/`:catch` machinery.
pub fn v_exception(oldval: Option<String>) -> Option<String> {
    match oldval {
        None => Some(get_vim_var_str(vv::VV_EXCEPTION)),
        Some(old) => {
            set_vim_var_string(vv::VV_EXCEPTION, &old);
            None
        }
    }
}

/// Port of `v_throwpoint()` from `Src/eval/vars.c:2322`.
///
/// The `v:throwpoint` counterpart of [`v_exception`].
pub fn v_throwpoint(oldval: Option<String>) -> Option<String> {
    match oldval {
        None => Some(get_vim_var_str(vv::VV_THROWPOINT)),
        Some(old) => {
            set_vim_var_string(vv::VV_THROWPOINT, &old);
            None
        }
    }
}

/// Port of `set_vcount()` from `Src/eval/vars.c:2336`.
///
/// Set `v:count`/`v:count1`; if `set_prevcount`, first copy the old `v:count`
/// into `v:prevcount`.
pub fn set_vcount(count: varnumber_T, count1: varnumber_T, set_prevcount: bool) {
    if set_prevcount {
        set_vim_var_nr(vv::VV_PREVCOUNT, get_vim_var_nr(vv::VV_COUNT));
    }
    set_vim_var_nr(vv::VV_COUNT, count);
    set_vim_var_nr(vv::VV_COUNT1, count1);
}

/// Port of `get_var_value()` from `Src/eval/vars.c:2587`.
///
/// String value of variable `name`, or `None` (C `NULL`) if it does not exist.
/// RUST-PORT NOTE: the C looks up a `dictitem_T` via `find_var`; the subset
/// resolves through [`eval_variable`] and stringifies the result.
pub fn get_var_value(name: &str) -> Option<String> {
    eval_variable(name).map(|tv| tv_get_string(&tv))
}

/// Port of `tv_list_unlet_range()` from `Src/eval/vars.c:1688`.
///
/// Delete the List range `[first .. last]` for `:unlet l[n1:n2]`, where `first`
/// is the index of the first item (`n1_arg` its list index), `has_n2`/`n2` give
/// the optional inclusive end. RUST-PORT NOTE: the C threads `listitem_T`
/// pointers; the `Vec`-backed `list_T` works in indices and delegates to the
/// ported [`tv_list_remove_items`].
pub fn tv_list_unlet_range(l: &mut list_T, first: usize, n1_arg: i32, has_n2: bool, n2: i32) {
    let len = crate::ported::eval::typval::tv_list_len(l) as usize;
    let mut last = first;
    let mut n1 = n1_arg;
    loop {
        let next = last + 1;
        n1 += 1; // n1 is now the index of the candidate `next` item
        if next >= len || (has_n2 && n2 < n1) {
            break;
        }
        last = next;
    }
    crate::ported::eval::typval::tv_list_remove_items(l, first, last);
}

/// Port of `get_globvar_dict()` from `Src/eval/vars.c:1855` — the `g:` scope
/// dict. RUST-PORT NOTE: the C returns the live `&globvardict`; the per-thread
/// store can't be borrowed out, so this returns a read-snapshot clone (faithful
/// for reading/iterating `g:`; mutations go through [`set_var`]).
pub fn get_globvar_dict() -> dict_T {
    globvardict.with(|d| d.borrow().clone())
}

/// Port of `get_globvar_ht()` from `Src/eval/vars.c:1862` — the `g:` scope
/// hashtable (read-snapshot, see [`get_globvar_dict`]).
pub fn get_globvar_ht() -> indexmap::IndexMap<String, typval_T> {
    get_globvar_dict().dv_hashtab
}

/// Port of `get_vimvar_dict()` from `Src/eval/vars.c:1868` — the `v:` scope dict.
/// RUST-PORT NOTE: `v:` variables live in the `vimvars[]` table, not a `dict_T`;
/// this builds a read-snapshot dict (bare name → value) from that table.
pub fn get_vimvar_dict() -> dict_T {
    let mut d = dict_T::default();
    for idx in 0..vv::VV_LEN {
        d.dv_hashtab
            .insert(get_vim_var_name(idx).to_string(), get_vim_var_tv(idx));
    }
    d
}

/// Port of `cat_prefix_varname()` from `Src/eval/vars.c:1945`.
///
/// Concatenate a single-character scope `prefix` and `name` into `"<p>:<name>"`
/// (e.g. `('g', "foo")` → `"g:foo"`). RUST-PORT NOTE: the C reuses a grown
/// static buffer; the port just returns an owned `String`.
pub fn cat_prefix_varname(prefix: u8, name: &str) -> String {
    format!("{}:{}", prefix as char, name)
}

/// Port of `skip_var_one()` from `Src/eval/vars.c:1145`.
///
/// Byte offset past one assignable variable name at `arg`, including the `@r`
/// register, `$VAR` env, `&option`, and `d.key`/`l[idx]` lvalue forms.
pub fn skip_var_one(arg: &str) -> usize {
    let b = arg.as_bytes();
    // c: "@r" — a register name is exactly two bytes.
    if b.first() == Some(&b'@') && b.get(1).is_some_and(|&c| c != 0) {
        return 2;
    }
    // c: "$VAR"/"&opt" — skip the sigil, then take the name (offset re-added).
    let off = usize::from(b.first() == Some(&b'$') || b.first() == Some(&b'&'));
    let (end, _, _) = crate::ported::eval::find_name_end(
        &arg[off..],
        crate::ported::eval::FNE_INCL_BR | crate::ported::eval::FNE_CHECK_START,
    );
    off + end
}

/// Port of `skip_var_list()` from `Src/eval/vars.c:1103`.
///
/// Skip an lvalue: either one name or a `[a, b; rest]` unpack list. Returns
/// `Some((consumed, var_count, semicolon))` — `consumed` is the byte offset
/// past the lvalue, `var_count` the number of names, and `semicolon` whether a
/// `;` rest-binding was seen — or `None` (C `NULL`) on a malformed list, after
/// emitting the matching error unless `silent`.
pub fn skip_var_list(arg: &str, silent: bool) -> Option<(usize, i32, bool)> {
    let b = arg.as_bytes();
    if b.first() != Some(&b'[') {
        let n = skip_var_one(arg);
        return Some((n, 1, false));
    }
    let skipwhite = |s: &str, mut i: usize| {
        let sb = s.as_bytes();
        while i < sb.len() && (sb[i] == b' ' || sb[i] == b'\t') {
            i += 1;
        }
        i
    };
    let mut var_count = 0;
    let mut semicolon = false;
    let mut p = 0usize; // index into `arg`, starts at '['
    loop {
        p = skipwhite(arg, p + 1); // skip whites after '[', ';' or ','
        let s = p + skip_var_one(&arg[p..]);
        if s == p {
            if !silent {
                crate::ported::message::semsg(&format!("E475: Invalid argument: {}", &arg[p..]));
            }
            return None;
        }
        var_count += 1;
        p = skipwhite(arg, s);
        match b.get(p) {
            Some(&b']') => break,
            Some(&b';') => {
                if semicolon {
                    if !silent {
                        crate::ported::message::emsg("E452: Double ; in list of variables");
                    }
                    return None;
                }
                semicolon = true;
            }
            Some(&b',') => {}
            _ => {
                if !silent {
                    crate::ported::message::semsg(&format!(
                        "E475: Invalid argument: {}",
                        &arg[p..]
                    ));
                }
                return None;
            }
        }
    }
    Some((p + 1, var_count, semicolon))
}

/// Port of `valid_varname()` from `Src/eval/vars.c:3060`.
///
/// True when every character of `varname` may appear in a variable name: a name
/// character, a digit (not first), or the autoload `#`. Emits `E461` otherwise.
pub fn valid_varname(varname: &str) -> bool {
    let bytes = varname.as_bytes();
    for (i, &c) in bytes.iter().enumerate() {
        if !crate::ported::eval::eval_isnamec1(c)
            && (i == 0 || !c.is_ascii_digit())
            && c != crate::ported::eval::AUTOLOAD_CHAR
        {
            crate::ported::message::semsg(&format!("E461: Illegal variable name: {varname}"));
            return false;
        }
    }
    true
}

/// Port of `eval_one_expr_in_str()` from `Src/eval/vars.c:621`.
///
/// Evaluate one `{expr}` block at the start of `p` (which begins with `{`),
/// appending its string result to `gap` when `evaluate`. Returns the number of
/// bytes consumed (past the closing `}`), or `None` on error. RUST-PORT NOTE:
/// the C locates the close with `skip_expr`; here an inline brace-balancer finds
/// the matching `}` (balancing `{}`/`[]`/`()`, skipping quoted strings).
pub fn eval_one_expr_in_str(p: &str, gap: &mut String, evaluate: bool) -> Option<usize> {
    let b = p.as_bytes();
    // Skip the opening '{' and following whitespace.
    let mut bs = 1;
    while bs < b.len() && (b[bs] == b' ' || b[bs] == b'\t') {
        bs += 1;
    }
    if bs >= b.len() {
        crate::ported::message::semsg(&format!("E1278: Missing '}}': {p}"));
        return None;
    }
    // Find the matching '}' from bs.
    let close = {
        let mut depth = 0i32;
        let mut i = bs;
        let mut found = None;
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
                b'{' | b'[' | b'(' => depth += 1,
                b'}' if depth == 0 => {
                    found = Some(i);
                    break;
                }
                b'}' | b']' | b')' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        found
    };
    let block_end = match close {
        Some(i) => i,
        None => {
            crate::ported::message::semsg(&format!("E1278: Missing '}}': {p}"));
            return None;
        }
    };
    if evaluate {
        let expr = p[bs..block_end].trim_end();
        let val = crate::ported::eval::eval_to_string(expr)?;
        gap.push_str(&val);
    }
    Some(block_end + 1)
}

/// Port of `eval_all_expr_in_str()` from `Src/eval/vars.c:656`.
///
/// Evaluate every `{expr}` in `str`, returning the interpolated string. `{{` and
/// `}}` are unescaped to `{`/`}`. Returns `None` on error (a stray `}` or a bad
/// expression). Used for interpolated strings / heredoc assignment.
pub fn eval_all_expr_in_str(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut ga = String::new();
    let mut p = 0;
    while p < b.len() {
        let lit_start = p;
        while p < b.len() && b[p] != b'{' && b[p] != b'}' {
            p += 1;
        }
        let mut escaped_brace = false;
        if p < b.len() && p + 1 < b.len() && b[p] == b[p + 1] {
            // "{{" / "}}" → unescape: keep one brace, skip the other.
            p += 1;
            escaped_brace = true;
        } else if p < b.len() && b[p] == b'}' {
            crate::ported::message::semsg(&format!("E1279: Missing '{{': {s}"));
            return None;
        }
        ga.push_str(&s[lit_start..p]);
        if p >= b.len() {
            break;
        }
        if escaped_brace {
            p += 1;
            continue;
        }
        // p is at '{' — evaluate the block.
        let consumed = eval_one_expr_in_str(&s[p..], &mut ga, true)?;
        p += consumed;
    }
    Some(ga)
}

/// Port of `get_spellword()` from `Src/eval/vars.c:559`.
///
/// Validate a `'spellsuggest'` result item — a `[word, score]` List of exactly
/// two values — returning `(word, score)` or `None` (the C `-1`) with an error
/// when the shape is wrong.
pub fn get_spellword(list: &list_T) -> Option<(String, i32)> {
    use crate::ported::eval::typval::{tv_list_find_nr, tv_list_find_str, tv_list_len};
    if tv_list_len(list) != 2 {
        crate::ported::message::emsg(
            "E5700: Expression from 'spellsuggest' must yield lists with exactly two values",
        );
        return None;
    }
    let word = tv_list_find_str(list, 0)?;
    let score = tv_list_find_nr(list, -1, None) as i32;
    Some((word, score))
}

/// Port of `var_set_global()` from `Src/eval/vars.c`.
///
/// Set variable `name` in the global scope even when called from inside a
/// function: suspend the active funccal ([`save_funccal`]), [`set_var`] at
/// global level, then restore. RUST-PORT NOTE: with the active scope cleared a
/// bare `name` resolves to `globvardict` (see [`set_var`]).
pub fn var_set_global(name: &str, vartv: typval_T) {
    crate::ported::eval::userfunc::save_funccal();
    set_var(name, name.len(), vartv, false);
    crate::ported::eval::userfunc::restore_funccal();
}

/// Port of `unref_var_dict()` from `Src/eval/vars.c:2625` — drop a scope dict's
/// extra reference and free it when unused. `Rc`/`Drop`-managed, so no-op.
pub fn unref_var_dict() {}

/// Port of `new_script_vars()` from `Src/eval/vars.c:2600` — allocate the `s:`
/// scope for a newly sourced script. RUST-PORT NOTE: the standalone has a single
/// script context ([`script_vars`]), already initialized, so this is a no-op.
pub fn new_script_vars() {}

/// Port of `init_var_dict()` from `Src/eval/vars.c:2609` — initialize a dict as a
/// scope and point its scope-variable at it. The `IndexMap`-backed scope dicts
/// are default-initialized (see [`evalvars_init`]), so this is a no-op.
pub fn init_var_dict() {}

/// Port of `find_var()` from `Src/eval/vars.c:2404`.
///
/// Look up variable `name` across the scope chain (prefix scopes, the active
/// `l:`/`a:`, and a lambda's captured parent scope). RUST-PORT NOTE: the C
/// returns a mutable `dictitem_T`; this read-reduced port routes the whole
/// resolution through [`eval_variable`] (which already covers closures), so it
/// returns the value. `no_autoload` is moot — no autoload standalone.
pub fn find_var(name: &str, _no_autoload: bool) -> Option<typval_T> {
    eval_variable(name)
}

/// Port of `var_exists()` from `Src/eval/vars.c:3371`.
///
/// Whether variable `var` exists. RUST-PORT NOTE: the C also resolves curly-brace
/// names and trailing `d.key`/`l[idx]` subscripts via `handle_subscript`; this
/// subset checks the bare name through [`eval_variable`].
pub fn var_exists(var: &str) -> bool {
    eval_variable(var).is_some()
}

/// Port of `find_var_in_ht()` from `Src/eval/vars.c:2439`.
///
/// Look up variable `varname` in scope dict `d`, returning its value or `None`.
/// RUST-PORT NOTE: the C's empty-`varname` case returns the scope's own
/// self-dictitem (`g:`/`s:`/`l:`/… as a Dict value) — not modeled here, so it
/// yields `None`. The global-scope autoload retry is also absent standalone.
pub fn find_var_in_ht<'a>(
    d: &'a dict_T,
    _htname: u8,
    varname: &str,
    _no_autoload: bool,
) -> Option<&'a typval_T> {
    if varname.is_empty() {
        return None;
    }
    d.dv_hashtab.get(varname)
}

/// Port of `vars_clear()` from `Src/eval/vars.c:2636` — free all variables in a
/// scope and clear its hashtable.
pub fn vars_clear(d: &mut dict_T) {
    vars_clear_ext(d, true);
}

/// Port of `vars_clear_ext()` from `Src/eval/vars.c:2642` — like [`vars_clear`]
/// but only freeing values when `free_val`. RUST-PORT NOTE: values are
/// `Rc`/`Drop`-managed, so clearing the `IndexMap` reclaims them regardless.
pub fn vars_clear_ext(d: &mut dict_T, _free_val: bool) {
    d.dv_hashtab.clear();
}

/// Port of `delete_var()` from `Src/eval/vars.c:2672` — remove one variable from
/// a scope dict (its value is dropped). RUST-PORT NOTE: the C addresses the item
/// by `hashitem_T`; the `IndexMap` model removes it by key.
pub fn delete_var(d: &mut dict_T, key: &str) {
    d.dv_hashtab.shift_remove(key);
}

/// Port of `get_user_var_name()` from `Src/eval/vars.c:1964` — the `idx`-th
/// user variable name (across `g:`/`b:`/`w:`/`v:`) for command-line completion.
/// No interactive completion standalone → `None`.
pub fn get_user_var_name(_idx: i32) -> Option<String> {
    None
}

// ── :redir => var capture (vars.c) ──
//
// RUST-PORT NOTE: `:redir` is not handled by the carve-out command layer, and
// `var_redir_start` would need the stubbed lvalue machinery (`get_lval`/
// `set_var_lval`), so the redir-to-variable subsystem is inactive — no redirect
// is ever active. These two are therefore faithful no-ops.

/// Port of `var_redir_start()` from `Src/eval/vars.c:3414` — begin a `:redir =>`
/// capture by resolving the target lvalue. RUST-PORT NOTE: the lvalue machinery
/// (`get_lval`/`set_var_lval`) is stubbed and no command layer drives this, so a
/// redirect can never be set up → returns `FAIL` (the cluster stays inactive,
/// keeping [`var_redir_str`]/[`var_redir_stop`] genuine no-ops).
pub fn var_redir_start(_name: &str, _append: bool) -> i32 {
    crate::ported::eval_h::FAIL
}

/// Port of `var_redir_str()` from `Src/eval/vars.c:3475` — append redirected
/// command output to the capture buffer; no active redirect, no-op.
pub fn var_redir_str(_value: &str, _value_len: i32) {}

/// Port of `var_redir_stop()` from `Src/eval/vars.c:3495` — finish a `:redir =>`
/// capture and assign it to the target variable; no active redirect, no-op.
pub fn var_redir_stop() {}

/// Port of `reset_v_option_vars()` from `Src/eval/vars.c:3349`.
///
/// Clear the `v:option_*` variables (set by `OptionSet` autocommands) back to
/// empty strings. RUST-PORT NOTE: the C passes `NULL` (length -1); here an empty
/// string is the equivalent cleared value.
pub fn reset_v_option_vars() {
    for idx in [
        vv::VV_OPTION_NEW,
        vv::VV_OPTION_OLD,
        vv::VV_OPTION_OLDLOCAL,
        vv::VV_OPTION_OLDGLOBAL,
        vv::VV_OPTION_COMMAND,
        vv::VV_OPTION_TYPE,
    ] {
        set_vim_var_string(idx, "");
    }
}

// ── di_flags variable-protection checks (vars.c) ──
//
// RUST-PORT NOTE: the C reads the protection bits from a `dictitem_T.di_flags`;
// these take the flag word as a plain `int`, so they port without the dictitem
// model. The `name_len` `TV_TRANSLATE`/`TV_CSTRING` sentinels collapse — the
// Rust `&str` already carries its length. The `sandbox` nesting counter is not
// modeled standalone (always 0), so the `DI_FLAGS_RO_SBX` branch never fires.

/// Port of `var_check_ro()` from `Src/eval/vars.c:2947`.
///
/// True (and emits an error) when `flags` marks the variable read-only:
/// `DI_FLAGS_RO` always, or `DI_FLAGS_RO_SBX` while in the sandbox.
pub fn var_check_ro(flags: i32, name: &str, _name_len: usize) -> bool {
    use crate::ported::eval::typval_defs_h::{DI_FLAGS_RO, DI_FLAGS_RO_SBX};
    let error_message = if flags & DI_FLAGS_RO != 0 {
        Some(format!("E46: Cannot change read-only variable \"{name}\""))
    } else if flags & DI_FLAGS_RO_SBX != 0 && SANDBOX {
        Some(format!("E794: Cannot set variable in the sandbox: \"{name}\""))
    } else {
        None
    };
    match error_message {
        None => false,
        Some(msg) => {
            crate::ported::message::semsg(&msg);
            true
        }
    }
}

/// Port of `var_check_lock()` from `Src/eval/vars.c:2974`.
///
/// True (and emits `E1122`) when `flags` has `DI_FLAGS_LOCK` set.
pub fn var_check_lock(flags: i32, name: &str, _name_len: usize) -> bool {
    use crate::ported::eval::typval_defs_h::DI_FLAGS_LOCK;
    if flags & DI_FLAGS_LOCK == 0 {
        return false;
    }
    crate::ported::message::semsg(&format!("E1122: Variable is locked: {name}"));
    true
}

/// Port of `var_check_fixed()` from `Src/eval/vars.c:3010`.
///
/// True (and emits `E795`) when `flags` has `DI_FLAGS_FIX` set — the variable
/// cannot be `:unlet` or `remove()`d.
pub fn var_check_fixed(flags: i32, name: &str, _name_len: usize) -> bool {
    use crate::ported::eval::typval_defs_h::DI_FLAGS_FIX;
    if flags & DI_FLAGS_FIX != 0 {
        crate::ported::message::semsg(&format!("E795: Cannot delete variable {name}"));
        return true;
    }
    false
}

/// `int sandbox;` (`Src/ex_docmd.c`) — the sandbox nesting counter. Not modeled
/// in the standalone interpreter, so it is permanently 0 (false).
const SANDBOX: bool = false;

#[cfg(test)]
mod misc_helper_tests {
    use super::*;
    use crate::ported::eval::typval::tv_get_string;

    #[test]
    fn internal_string_var_and_vimvar_save_restore() {
        // set_internal_string_var writes a global readable via eval_variable.
        set_internal_string_var("g:port_test", "hello");
        assert_eq!(
            tv_get_string(&eval_variable("g:port_test").unwrap()),
            "hello"
        );

        // prepare_vimvar saves, then restore_vimvar puts the value back.
        set_vim_var_string(vv::VV_VAL, "original");
        let saved = prepare_vimvar(vv::VV_VAL);
        set_vim_var_string(vv::VV_VAL, "temporary");
        assert_eq!(get_vim_var_str(vv::VV_VAL), "temporary");
        restore_vimvar(vv::VV_VAL, saved);
        assert_eq!(get_vim_var_str(vv::VV_VAL), "original");
        set_internal_string_var("g:unlet_me", "x");
        assert!(eval_variable("g:unlet_me").is_some());
        assert_eq!(do_unlet("g:unlet_me", 0, false), crate::ported::eval_h::OK);
        assert!(eval_variable("g:unlet_me").is_none());
        assert!(var_wrong_func_name("lowercase", false));
        assert!(!var_wrong_func_name("Capital", false));

        // GC is a no-op under Rc.
        assert_eq!(garbage_collect_globvars(0), 0);
        assert!(!garbage_collect_vimvars(0));
    }

    #[test]
    fn prefix_and_var_skip_helpers() {
        assert_eq!(cat_prefix_varname(b'g', "foo"), "g:foo");
        assert_eq!(cat_prefix_varname(b'b', "count"), "b:count");

        // skip_var_one: a single name / @reg / $env / &opt / l[idx]
        assert_eq!(skip_var_one("foo = 1"), 3);
        assert_eq!(skip_var_one("@a more"), 2);
        assert_eq!(skip_var_one("$PATH = 1"), 5); // '$' + "PATH"
        assert_eq!(skip_var_one("&ignorecase"), 11); // '&' + name
        assert_eq!(skip_var_one("d.key rest"), 5);

        // skip_var_list: a single name yields count 1, no semicolon
        assert_eq!(skip_var_list("x = 1", false), Some((1, 1, false)));
        // a list unpack: [a, b] -> 2 names, consumed through ']'
        let (consumed, n, semi) = skip_var_list("[a, b] = pair", false).unwrap();
        assert_eq!(&"[a, b] = pair"[..consumed], "[a, b]");
        assert_eq!((n, semi), (2, false));
        // a rest binding: [head; tail]
        let (_, n2, semi2) = skip_var_list("[head; tail] = xs", false).unwrap();
        assert_eq!((n2, semi2), (2, true));
        // double ';' is rejected
        assert_eq!(skip_var_list("[a; b; c]", true), None);
    }

    #[test]
    fn di_flags_protection_checks() {
        use crate::ported::eval::typval_defs_h::{
            DI_FLAGS_FIX, DI_FLAGS_LOCK, DI_FLAGS_RO, DI_FLAGS_RO_SBX,
        };
        // read-only fires; unset and sandbox-only (no sandbox) do not.
        assert!(var_check_ro(DI_FLAGS_RO, "g:v", 3));
        assert!(!var_check_ro(0, "g:v", 3));
        assert!(!var_check_ro(DI_FLAGS_RO_SBX, "g:v", 3)); // sandbox not modeled
        // lock / fixed.
        assert!(var_check_lock(DI_FLAGS_LOCK, "g:v", 3));
        assert!(!var_check_lock(0, "g:v", 3));
        assert!(var_check_fixed(DI_FLAGS_FIX, "g:v", 3));
        assert!(!var_check_fixed(0, "g:v", 3));
    }

    #[test]
    fn list_unlet_range_removes_inclusive() {
        use crate::ported::eval::typval::tv_list_append_number;
        use crate::ported::eval::typval_defs_h::{list_T, typval_vval_union::v_number};
        let vals = |l: &list_T| -> Vec<i64> {
            l.lv_items
                .iter()
                .map(|it| match it.li_tv.vval {
                    v_number(n) => n,
                    _ => -1,
                })
                .collect()
        };
        let mut l = list_T::default();
        for n in [0, 1, 2, 3, 4] {
            tv_list_append_number(&mut l, n);
        }
        // :unlet l[1:3] — inclusive of index 3
        tv_list_unlet_range(&mut l, 1, 1, true, 3);
        assert_eq!(vals(&l), vec![0, 4]);

        // open-ended :unlet l[2:] removes through the end
        let mut l2 = list_T::default();
        for n in [0, 1, 2, 3, 4] {
            tv_list_append_number(&mut l2, n);
        }
        tv_list_unlet_range(&mut l2, 2, 2, false, 0);
        assert_eq!(vals(&l2), vec![0, 1]);
    }

    #[test]
    fn vars_clear_and_delete() {
        use crate::ported::eval::typval::tv_dict_add_nr;
        use crate::ported::eval::typval_defs_h::dict_T;
        let mut d = dict_T::default();
        tv_dict_add_nr(&mut d, "a", 1);
        tv_dict_add_nr(&mut d, "b", 2);
        tv_dict_add_nr(&mut d, "c", 3);
        // delete one
        delete_var(&mut d, "b");
        assert_eq!(d.dv_hashtab.len(), 2);
        assert!(!d.dv_hashtab.contains_key("b"));
        assert!(d.dv_hashtab.contains_key("a"));
        // clear all
        vars_clear(&mut d);
        assert!(d.dv_hashtab.is_empty());
    }

    #[test]
    fn vimvar_dict_snapshot() {
        let d = get_vimvar_dict();
        // Built from the vimvars[] table: bare names, with known v: members.
        assert!(d.dv_hashtab.contains_key("count"));
        assert!(d.dv_hashtab.contains_key("errors"));
        assert!(!d.dv_hashtab.contains_key("v:count"));
        // g: snapshot reflects a set global.
        set_internal_string_var("g:gv_snap", "ok");
        assert!(get_globvar_dict().dv_hashtab.contains_key("gv_snap"));
        assert!(get_globvar_ht().contains_key("gv_snap"));
    }

    #[test]
    fn find_var_and_var_exists() {
        set_internal_string_var("g:fv_probe", "yes");
        assert!(var_exists("g:fv_probe"));
        assert!(find_var("g:fv_probe", false).is_some());
        assert!(!var_exists("g:fv_absent_xyz"));
        assert!(find_var("g:fv_absent_xyz", false).is_none());
    }

    #[test]
    fn find_var_in_ht_lookup() {
        use crate::ported::eval::typval::tv_dict_add_nr;
        use crate::ported::eval::typval_defs_h::{dict_T, typval_vval_union::v_number};
        let mut d = dict_T::default();
        tv_dict_add_nr(&mut d, "x", 5);
        assert!(matches!(find_var_in_ht(&d, b'g', "x", false).map(|tv| &tv.vval), Some(v_number(5))));
        assert!(find_var_in_ht(&d, b'g', "missing", false).is_none());
        // empty varname (scope-self ref) is not modeled → None
        assert!(find_var_in_ht(&d, b'g', "", false).is_none());
    }

    #[test]
    fn expr_interpolation_in_str() {
        use crate::ported::eval::typval::EVAL_STRING_HOOK;
        use crate::ported::eval::typval_defs_h::typval_T;
        // Mock the eval hook: "1+2" → 3, "x" → "hi".
        fn hook(e: &str) -> Option<typval_T> {
            match e {
                "1+2" => Some(typval_T::from(3)),
                "x" => Some(typval_T::from("hi".to_string())),
                _ => None,
            }
        }
        let saved = EVAL_STRING_HOOK.with(|h| *h.borrow());
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = Some(hook));
        assert_eq!(eval_all_expr_in_str("a={1+2}b").as_deref(), Some("a=3b"));
        assert_eq!(eval_all_expr_in_str("{x}!").as_deref(), Some("hi!"));
        // escaped braces
        assert_eq!(eval_all_expr_in_str("{{lit}}").as_deref(), Some("{lit}"));
        // no interpolation
        assert_eq!(eval_all_expr_in_str("plain").as_deref(), Some("plain"));
        // stray close brace → error (None)
        assert_eq!(eval_all_expr_in_str("a}b"), None);
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = saved);
    }

    #[test]
    fn get_spellword_pairs() {
        use crate::ported::eval::typval::{tv_list_append_number, tv_list_append_string};
        use crate::ported::eval::typval_defs_h::list_T;
        // [word, score] → Some(("hello", 42))
        let mut l = list_T::default();
        tv_list_append_string(&mut l, "hello");
        tv_list_append_number(&mut l, 42);
        assert_eq!(get_spellword(&l), Some(("hello".to_string(), 42)));
        // wrong arity → None
        let mut bad = list_T::default();
        tv_list_append_string(&mut bad, "only");
        assert_eq!(get_spellword(&bad), None);
    }

    #[test]
    fn var_set_global_from_function_scope() {
        use crate::ported::eval::typval::tv_get_string;
        // Simulate being inside a function: push an active scope.
        funccal_stack.with(|s| s.borrow_mut().push(FuncScope::default()));
        // var_set_global must write to g:, not the active l: scope.
        var_set_global("vsg_test", typval_T::from("global".to_string()));
        // The active scope is restored (still one frame, and it has no local).
        let depth = funccal_stack.with(|s| s.borrow().len());
        assert_eq!(depth, 1);
        // Visible as a global, not shadowed by the function frame.
        assert_eq!(
            tv_get_string(&eval_variable("g:vsg_test").unwrap()),
            "global"
        );
        // Clean up the simulated frame.
        funccal_stack.with(|s| s.borrow_mut().pop());
    }

    #[test]
    fn reset_v_option_vars_clears() {
        set_vim_var_string(vv::VV_OPTION_NEW, "newval");
        set_vim_var_string(vv::VV_OPTION_TYPE, "global");
        reset_v_option_vars();
        assert_eq!(get_vim_var_str(vv::VV_OPTION_NEW), "");
        assert_eq!(get_vim_var_str(vv::VV_OPTION_TYPE), "");
    }
}
