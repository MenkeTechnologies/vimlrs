//! Port of `src/nvim/eval/vars.c` (vendored at `vendor/eval/vars.c`).
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
    /// script context in the standalone interpreter (zmax maps multiple).
    pub static script_vars: RefCell<dict_T> = RefCell::new(dict_T::default());
    /// `b:` buffer-local (`buf_T.b_vars`). One buffer standalone.
    pub static buffer_vars: RefCell<dict_T> = RefCell::new(dict_T::default());
    /// `w:` window-local (`win_T.w_vars`). One window standalone.
    pub static window_vars: RefCell<dict_T> = RefCell::new(dict_T::default());
    /// `t:` tabpage-local (`tabpage_T.tp_vars`). One tabpage standalone.
    pub static tabpage_vars: RefCell<dict_T> = RefCell::new(dict_T::default());
}

/// Port of `set_var()` from `Src/eval/vars.c:2805` (folding `set_var_const`,
/// `vars.c:2816`).
///
/// Set variable `name` to `tv`. RUST-PORT NOTE: scope resolution is delegated to
/// [`find_var_ht_dict`] (the single resolver shared with [`find_var`]), so this
/// no longer strips prefixes itself. The `dictitem_T` lock / watcher / autoload
/// machinery of C `set_var_const` is not modelled; the reduced setter writes the
/// resolved scope dict, declines read-only `v:` slots (E46 in C), and refuses to
/// add `v:`/`a:` variables (E461, matching `vars.c:2882`).
pub fn set_var(name: &str, name_len: usize, tv: typval_T, _copy: bool) {
    // RUST-PORT NOTE: reduced-model callers (get_lval/set_var_lval, tests, and
    // the eval bridge) pass name_len==0 to mean "the whole name" (the pre-dedup
    // set_var ignored the length). C always passes STRLEN(name); mirror that so
    // find_var_ht_dict doesn't truncate a real name to empty (which would E461).
    let name_len = if name_len == 0 { name.len() } else { name_len };
    // c:2823 ht = find_var_ht_dict(name, name_len, &varname, &dict);
    let (scope, varname) = match find_var_ht_dict(name, name_len) {
        // c:2830 if (ht == NULL || *varname == NUL) { semsg(_(e_illvar), name); return; }
        Some((s, v)) if !v.is_empty() => (s, v),
        _ => {
            crate::ported::message::semsg(&format!("E461: Illegal variable name: {name}"));
            return;
        }
    };
    match scope {
        VarScopeDict::Global => {
            globvardict.with(|d| tv_dict_add_tv(&mut d.borrow_mut(), &varname, tv));
        }
        VarScopeDict::Script => {
            script_vars.with(|d| tv_dict_add_tv(&mut d.borrow_mut(), &varname, tv));
        }
        VarScopeDict::Buffer => {
            buffer_vars.with(|d| tv_dict_add_tv(&mut d.borrow_mut(), &varname, tv));
        }
        VarScopeDict::Window => {
            window_vars.with(|d| tv_dict_add_tv(&mut d.borrow_mut(), &varname, tv));
        }
        VarScopeDict::Tabpage => {
            tabpage_vars.with(|d| tv_dict_add_tv(&mut d.borrow_mut(), &varname, tv));
        }
        VarScopeDict::FuncLocal => {
            funccal_stack.with(|s| {
                if let Some(top) = s.borrow_mut().last_mut() {
                    tv_dict_add_tv(&mut top.fc_l_vars, &varname, tv);
                }
            });
        }
        VarScopeDict::VimVar => {
            // c: existing v: var — decline read-only slots (var_check_ro, E46);
            //    an unknown v: name would be a "new v: variable" → E461 (c:2882).
            if let Some(idx) = VIMVARS_DEF.iter().position(|&(n, _, _)| n == varname) {
                if VIMVARS_DEF[idx].2 & (VV_RO | VV_RO_SBX) == 0 {
                    set_vim_var_tv(idx, tv);
                }
            } else {
                crate::ported::message::semsg(&format!("E461: Illegal variable name: {name}"));
            }
        }
        VarScopeDict::FuncArgs => {
            // c:2882 can't add an "a:" variable; existing a: args are read-only.
            crate::ported::message::semsg(&format!("E461: Illegal variable name: {name}"));
        }
    }
}

/// Port of `eval_variable()` from `Src/eval/vars.c:2353` (read path).
///
/// Look up a variable's value, or `None` when it does not exist. RUST-PORT NOTE:
/// the C `eval_variable(name, len, rettv, dip, verbose, no_autoload)` resolves
/// via `find_var(name, len, NULL, no_autoload)` (`vars.c:2371`); the reduced
/// reader keeps that delegation, so scope resolution lives in exactly one place
/// ([`find_var_ht_dict`]). The C out-param `rettv`/`dip` collapse into the
/// returned value.
pub fn eval_variable(name: &str) -> Option<typval_T> {
    // c:2371 v = find_var(name, (size_t)len, NULL, no_autoload);
    find_var(name, false)
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
        // vim-only v: var (vim 8.1.0729+), absent from the neovim-derived base
        // above; appended so the neovim VimVarIndex order of the preceding
        // entries is preserved byte-for-byte.
        VV_COLORNAMES,
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
    // c (vim evalvars.c:155): {VV_NAME("colornames", VAR_DICT), VV_RO}. vim-only
    // (neovim has no v:colornames); appended last to keep the neovim-derived
    // indices above unchanged. Empty writable Dict — colors/lists/*.vim populate
    // it via extend()/indexed assignment; the VV_RO binding blocks reassignment.
    ("colornames", VAR_DICT, VV_RO),
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
        // c: set_vim_var_dict(VV_COLORNAMES, dict_alloc()) — an empty writable
        // Dict (colors/lists/*.vim populate it via extend()); the binding is
        // VV_RO (can't reassign v:colornames) but its contents stay mutable.
        v[VV_COLORNAMES].vv_tv = typval_T {
            v_type: VAR_DICT,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_dict(Some(tv_dict_alloc())),
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

/// Port of `set_cmdarg()` from `Src/eval/vars.c:2204`.
///
/// Set `v:cmdarg`. With `Some(eap)`, build the value from the command's `++`
/// file modifiers and return the old value; with `eap == None` and `oldarg`,
/// restore the saved value and return `None`. Always called in pairs.
///
/// RUST-PORT NOTE: the write-half (the `v:cmdarg` `get_vim_var_tv`/save/restore
/// pairing) is ported faithfully. The reduced [`exarg_T`] does not model the
/// `:command` file-modifier fields, so the `++bin`/`++nobin`/`++edit`/`++ff=…`/
/// `++enc=…`/`++bad=…`/`++p` suffix construction (`vars.c:2213`-`2251`) cannot be
/// reconstructed and is deferred (see deferred_deps); with no modifiers the C
/// value is the base case (`*newval = NUL`, `vars.c:2246`), i.e. the empty
/// string, which is what the `Some(eap)` branch writes.
pub fn set_cmdarg(eap: Option<&exarg_T>, oldarg: Option<String>) -> Option<String> {
    // c:2207 typval_T *tv = get_vim_var_tv(VV_CMDARG);
    // c:2208 char *oldval = tv->vval.v_string;
    let oldval = get_vim_var_str(vv::VV_CMDARG);
    if eap.is_none() {
        // c:2209 if (eap == NULL) goto error;
        // c:2257 error: tv->vval.v_string = oldarg; return NULL;
        set_vim_var_string(vv::VV_CMDARG, &oldarg.unwrap_or_default());
        return None;
    }
    // c:2213-2251 DEFERRED: build newval from eap->force_bin/read_edit/force_ff/
    //   force_enc/bad_char/mkdir_p — the reduced exarg_T lacks these fields, so
    //   the value is the no-modifier base case (empty string).
    // c:2253 tv->vval.v_string = newval;
    set_vim_var_string(vv::VV_CMDARG, "");
    // c:2254 return oldval;
    Some(oldval)
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
pub fn get_globvar_ht() -> crate::ported::hashtab::hashtab_T<typval_T> {
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

/// The scope a variable name resolves to — the reduced stand-in for the C
/// `hashtab_T *` return of [`find_var_ht_dict`] together with its `dict_T **d`
/// out-param.
///
/// RUST-PORT NOTE: C `find_var_ht_dict` returns a single `hashtab_T *` (and sets
/// `*d`) naming one scope dict. The reduced model's scopes are stored
/// heterogeneously — `g:`/`s:`/`b:`/`w:`/`t:` in per-scope thread-local
/// `RefCell<dict_T>`, `l:`/`a:` as `dict_T` fields inside the `funccal_stack`
/// Vec, and `v:` in the `vimvars[]` table — none of them a uniform
/// `Rc<RefCell<dict_T>>`, so a single pointer type cannot name all of them. This
/// enum identifies the resolved scope; the caller ([`find_var`]/[`set_var`])
/// reaches into the matching store.
pub enum VarScopeDict {
    /// `g:` / a bare name at script level — `globvardict` (`vars.c:2531`/`:2525`).
    Global,
    /// `s:` — the script-local scope (`vars.c:2562`, SID/nlua machinery deferred).
    Script,
    /// `b:` — `curbuf->b_vars` (`vars.c:2540`).
    Buffer,
    /// `w:` — `curwin->w_vars` (`vars.c:2542`).
    Window,
    /// `t:` — `curtab->tp_vars` (`vars.c:2544`).
    Tabpage,
    /// `l:` / a bare name inside a function — `get_funccal_local_dict()`
    /// (`vars.c:2550`/`:2520`).
    FuncLocal,
    /// `a:` — `get_funccal_args_dict()` (`vars.c:2548`).
    FuncArgs,
    /// `v:` (and the implicit `version` compat name) — `get_vimvar_dict()`
    /// (`vars.c:2546`/`:2517`).
    VimVar,
}

/// Port of `find_var_ht_dict()` from `Src/eval/vars.c:2498`.
///
/// Find the scope dict (`g:`, `l:`, `s:`, …) used for variable `name`, returning
/// `Some((scope, varname))` where `varname` is `name` without its scope prefix,
/// or `None` when `name` is not a valid variable name. This is the single scope
/// resolver: [`find_var`] (read) and [`set_var`] (write) both delegate here, so
/// prefix stripping lives in exactly one place.
///
/// RUST-PORT NOTE: the C out-params `**varname`/`**d` and the `hashtab_T *`
/// return collapse into the returned tuple ([`VarScopeDict`] naming the resolved
/// dict, plus the owned stripped name). The `s:` branch's SID / anonymous-script
/// creation (`nlua_set_sctx`/`new_script_item`, `vars.c:2551`-`2562`) is not
/// modelled — the standalone has a single always-present script scope. For `l:`
/// and `a:` the C returns NULL when not in a function (`*d ? … : NULL`,
/// `vars.c:2566`); this returns `None` there too.
pub fn find_var_ht_dict(name: &str, name_len: usize) -> Option<(VarScopeDict, String)> {
    let name_len = name_len.min(name.len());
    let b = name.as_bytes();
    // c:2503 if (name_len == 0) return NULL;
    if name_len == 0 {
        return None;
    }
    // c:2506 if (name_len == 1 || name[1] != ':') — implicit scope.
    if name_len == 1 || b[1] != b':' {
        // c:2508 the name must not start with a colon or '#'.
        if b[0] == b':' || b[0] == crate::ported::eval::AUTOLOAD_CHAR {
            return None;
        }
        // c:2512 *varname = name;
        let varname = name[..name_len].to_string();
        // c:2514 "version" is "v:version" in all scopes (compat_hashtab). Only
        //   the VV_COMPAT vimvar ("version") lives in that table.
        if varname == "version" {
            return Some((VarScopeDict::VimVar, varname));
        }
        // c:2520 local variable if inside a function, else global (c:2525).
        if crate::ported::eval::userfunc::get_funccal_local_dict().is_some() {
            return Some((VarScopeDict::FuncLocal, varname));
        }
        return Some((VarScopeDict::Global, varname));
    }

    // c:2529 *varname = name + 2;
    let varname = name[2..name_len].to_string();
    // c:2530 if (*name == 'g') global variable.
    if b[0] == b'g' {
        return Some((VarScopeDict::Global, varname));
    }
    // c:2532 there must be no ':' or '#' in the rest of the name if g: was not
    //   used.
    if name_len > 2
        && (varname.as_bytes().contains(&b':')
            || varname
                .as_bytes()
                .contains(&crate::ported::eval::AUTOLOAD_CHAR))
    {
        return None;
    }

    // c:2539 dispatch on the scope character; `l:`/`a:` are valid only in a
    //   function (C: *d stays the funccal dict, else NULL at c:2566).
    let scope = match b[0] {
        b'b' => VarScopeDict::Buffer,  // c:2539
        b'w' => VarScopeDict::Window,  // c:2541
        b't' => VarScopeDict::Tabpage, // c:2543
        b'v' => VarScopeDict::VimVar,  // c:2545
        b'a' => {
            // c:2547
            crate::ported::eval::userfunc::get_funccal_args_dict()?;
            VarScopeDict::FuncArgs
        }
        b'l' => {
            // c:2549
            crate::ported::eval::userfunc::get_funccal_local_dict()?;
            VarScopeDict::FuncLocal
        }
        b's' => VarScopeDict::Script, // c:2551
        // c:2564 unknown scope char → *d stays NULL → return NULL.
        _ => return None,
    };
    Some((scope, varname))
}

/// Port of `find_var_ht()` from `Src/eval/vars.c:2577`.
///
/// The hashtable/scope used for variable `name`, without the C `dict_T **d`
/// out-param. RUST-PORT NOTE: delegates to [`find_var_ht_dict`] (as C does) and
/// returns the resolved [`VarScopeDict`] plus the stripped `varname` (the C
/// `**varname` out-param).
pub fn find_var_ht(name: &str, name_len: usize) -> Option<(VarScopeDict, String)> {
    // c:2579 dict_T *d; return find_var_ht_dict(name, name_len, varname, &d);
    find_var_ht_dict(name, name_len)
}

/// Port of `find_var()` from `Src/eval/vars.c:2404`.
///
/// Look up variable `name`, returning its value or `None`. RUST-PORT NOTE: the C
/// returns a mutable `dictitem_T`; this read-reduced port resolves the scope
/// through [`find_var_ht_dict`] (the single resolver shared with [`set_var`])
/// and reads the value out of it — folding the `find_var_in_ht` lookup
/// (`vars.c:2439`), including its empty-`varname` "scope self-dict" case
/// (`vars.c:2444`), into this function. The lambda parent-scope retry
/// (`find_var_in_scoped_ht`) and global-scope autoload are not modelled;
/// `no_autoload` is moot.
pub fn find_var(name: &str, _no_autoload: bool) -> Option<typval_T> {
    // A `VAR_DICT` snapshot of a scope dict — the reduced `find_var_in_ht`
    // empty-`varname` case (C returns the scope's `&…_var` self-dictitem, e.g.
    // `&globvars_var`; here a read snapshot of the scope's contents). Covers
    // introspection (`keys(g:)`, `get(b:, …)`), not mutation-through-the-dict.
    let scope_snapshot = |d: &dict_T| -> typval_T {
        let nd = crate::ported::eval::typval::tv_dict_alloc();
        {
            let mut bm = nd.borrow_mut();
            for (k, v) in d.dv_hashtab.iter() {
                bm.dv_hashtab.insert(k.clone(), v.clone());
            }
        }
        typval_T {
            v_type: VAR_DICT,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_dict(Some(nd)),
        }
    };
    // c: v: boolean/special literals. RUST-PORT NOTE: not in C `find_var` (there
    //   they are ordinary vimvars[] entries); kept as a reduced-model safety net
    //   so they resolve even before `evalvars_init` seeds the vimvars[] table
    //   (whose type-zero defaults would otherwise read v:true as false).
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
    // c:2406 ht = find_var_ht(name, name_len, &varname); if (ht == NULL) return NULL;
    let (scope, varname) = find_var_ht_dict(name, name.len())?;
    // c:2439 find_var_in_ht: empty varname → the scope's self-dict (snapshot).
    if varname.is_empty() {
        return Some(match scope {
            VarScopeDict::Global => globvardict.with(|d| scope_snapshot(&d.borrow())),
            VarScopeDict::Script => script_vars.with(|d| scope_snapshot(&d.borrow())),
            VarScopeDict::Buffer => buffer_vars.with(|d| scope_snapshot(&d.borrow())),
            VarScopeDict::Window => window_vars.with(|d| scope_snapshot(&d.borrow())),
            VarScopeDict::Tabpage => tabpage_vars.with(|d| scope_snapshot(&d.borrow())),
            VarScopeDict::VimVar => scope_snapshot(&get_vimvar_dict()),
            // FuncLocal/FuncArgs only reach here with an active frame (the
            // resolver returns them only inside a function), so `last()` is Some.
            VarScopeDict::FuncLocal => funccal_stack.with(|s| {
                s.borrow()
                    .last()
                    .map(|f| scope_snapshot(&f.fc_l_vars))
                    .unwrap_or_default()
            }),
            VarScopeDict::FuncArgs => funccal_stack.with(|s| {
                s.borrow()
                    .last()
                    .map(|f| scope_snapshot(&f.fc_l_avars))
                    .unwrap_or_default()
            }),
        });
    }
    // c:2467 hi = hash_find_len(ht, varname, varname_len); read the value.
    match scope {
        VarScopeDict::Global => globvardict.with(|d| tv_dict_find(&d.borrow(), &varname).cloned()),
        VarScopeDict::Script => script_vars.with(|d| tv_dict_find(&d.borrow(), &varname).cloned()),
        VarScopeDict::Buffer => buffer_vars.with(|d| tv_dict_find(&d.borrow(), &varname).cloned()),
        VarScopeDict::Window => window_vars.with(|d| tv_dict_find(&d.borrow(), &varname).cloned()),
        VarScopeDict::Tabpage => {
            tabpage_vars.with(|d| tv_dict_find(&d.borrow(), &varname).cloned())
        }
        VarScopeDict::FuncLocal => funccal_stack.with(|s| {
            let s = s.borrow();
            tv_dict_find(&s.last()?.fc_l_vars, &varname).cloned()
        }),
        VarScopeDict::FuncArgs => funccal_stack.with(|s| {
            let s = s.borrow();
            tv_dict_find(&s.last()?.fc_l_avars, &varname).cloned()
        }),
        // c: v: variables live in the vimvars[] table. Returns None for
        //   VAR_UNKNOWN slots (v:val/v:key, supplied dynamically by the bridge)
        //   and for unknown v: names.
        VarScopeDict::VimVar => {
            let idx = VIMVARS_DEF.iter().position(|&(n, _, _)| n == varname)?;
            let tv = get_vim_var_tv(idx);
            if tv.v_type != VAR_UNKNOWN {
                Some(tv)
            } else {
                None
            }
        }
    }
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
        Some(format!(
            "E794: Cannot set variable in the sandbox: \"{name}\""
        ))
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

// ── :let / :unlet / :lockvar command drivers (vars.c) ───────────────────────
//
// RUST-PORT NOTE: these `exarg_T` command drivers are SUPERSEDED at runtime by
// the bytecode frontend (`viml_parser.rs` / `compile_viml.rs`); they are ported
// here as strict REFERENCE ports (dead_code allowed), exactly as `eval0..eval7`
// were. The reduced [`exarg_T`] below models only the fields these drivers read
// (`ea_arg`/`cmdidx`/`forceit`/`skip`/`nextcmd`/`cmdlinep`/`ea_getline`+cookie).
// `check_nextcmd` (ex_docmd.c) is not vendored — no bar-command chaining exists
// standalone, so `eap.nextcmd` is set to `None` where C calls `check_nextcmd`.

/// Reduced `cmdidx_T` (`ex_cmds_defs.h`) — only the command indices these
/// drivers branch on. `ex_let` reads `CMD_const`; `do_lock_var` reads
/// `CMD_lockvar`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum cmdidx_T {
    /// `:let`
    CMD_let,
    /// `:const`
    CMD_const,
    /// `:unlet`
    CMD_unlet,
    /// `:lockvar`
    CMD_lockvar,
    /// `:unlockvar`
    CMD_unlockvar,
}

/// Reduced `exarg_T` (`ex_cmds_defs.h:303`) — the command-argument block. Only
/// the fields the ported drivers read are modelled. RUST-PORT NOTE: the C
/// `ea_getline`/`cookie` pair (a `char *(*)(int, void *, int, bool)` plus its
/// opaque cookie) collapses into one owned line source closure.
#[derive(Default)]
pub struct exarg_T {
    /// `char *arg` — argument of the command.
    pub arg: String,
    /// `cmdidx_T cmdidx` — the command's index.
    pub cmdidx: cmdidx_T,
    /// `bool forceit` — the `!` was used.
    pub forceit: bool,
    /// `int skip` — don't execute the command, only parse it.
    pub skip: bool,
    /// `char *nextcmd` — the next command after a `|` (None standalone).
    pub nextcmd: Option<String>,
    /// `char **cmdlinep` — the whole command line (for heredoc `trim` indent).
    pub cmdlinep: String,
    /// `char *(*ea_getline)(...)` + `void *cookie` — next-line source for a
    /// heredoc body. `None` when there is no getline (C `ea_getline == NULL`).
    pub ea_getline: Option<Box<dyn FnMut() -> Option<String>>>,
}

impl Default for cmdidx_T {
    fn default() -> Self {
        cmdidx_T::CMD_let
    }
}

/// `typedef int (*ex_unletlock_callback)(lval_T *, char *, exarg_T *, int);`
/// (vars.c) — the per-name handler dispatched by [`ex_unletlock`].
type ex_unletlock_callback = fn(&mut crate::ported::eval::lval_T, &str, &mut exarg_T, i32) -> i32;

/// Port of `heredoc_get()` from `Src/eval/vars.c:724`.
///
/// Get a List of lines from a here document (`cmd << {marker}` … `{marker}`),
/// honouring the optional `trim`/`eval` words before the marker. When `cmd`
/// contains a newline the body is taken from the string after it (heredoc in a
/// string); otherwise lines come from `eap.ea_getline`. Returns the body List or
/// `None` on failure.
pub fn heredoc_get(eap: &mut exarg_T, cmd: &str, script_get: bool) -> Option<Rc<RefCell<list_T>>> {
    let mut marker_indent_len = 0usize; // c:727
    let mut text_indent_len: i32 = 0; // c:728
    let mut text_indent: Option<String> = None; // c:729
    let mut heredoc_in_string = false; // c:731
    let mut line_arg = String::new(); // c:732

    // c:733 char *nl_ptr = vim_strchr(cmd, '\n');
    let mut cmd_owned = cmd.to_string();
    if let Some(nl) = cmd_owned.find('\n') {
        // c:735 heredoc in a string separated by newlines.
        heredoc_in_string = true;
        line_arg = cmd_owned[nl + 1..].to_string();
        cmd_owned.truncate(nl); // c:738 *nl_ptr = NUL
    } else if eap.ea_getline.is_none() {
        // c:739
        crate::ported::message::emsg("E991: Cannot use =<< here");
        return None;
    }

    // c:745 Check for the optional 'trim'/'eval' words before the marker.
    let mut cmd_s = crate::ported::eval::skipwhite(&cmd_owned).to_string();
    let mut evalstr = false; // c:746
    let mut eval_failed = false; // c:747
    let iswhite = |c: u8| c == b' ' || c == b'\t';
    loop {
        // c:749 "trim"
        if cmd_s.starts_with("trim") && cmd_s.as_bytes().get(4).map_or(true, |&c| iswhite(c)) {
            cmd_s = crate::ported::eval::skipwhite(&cmd_s[4..]).to_string();
            // c:757 marker indent is the indent of the :let command line.
            let p = eap.cmdlinep.as_bytes();
            let mut i = 0;
            while i < p.len() && iswhite(p[i]) {
                i += 1;
                marker_indent_len += 1;
            }
            text_indent_len = -1; // c:762
            continue;
        }
        // c:766 "eval"
        if cmd_s.starts_with("eval") && cmd_s.as_bytes().get(4).map_or(true, |&c| iswhite(c)) {
            cmd_s = crate::ported::eval::skipwhite(&cmd_s[4..]).to_string();
            evalstr = true; // c:769
            continue;
        }
        break; // c:772
    }

    let comment_char = b'"'; // c:775
                             // c:776 The marker is the next word.
    let marker: String;
    let cb = cmd_s.as_bytes();
    if cb.first().is_some_and(|&c| c != 0 && c != comment_char) {
        // c:778 marker = skipwhite(cmd); p = skiptowhite(marker);
        let m = crate::ported::eval::skipwhite(&cmd_s);
        let end = m
            .as_bytes()
            .iter()
            .position(|&c| iswhite(c) || c == 0)
            .unwrap_or(m.len());
        // c:780 trailing after the marker must be empty or a comment.
        let rest = crate::ported::eval::skipwhite(&m[end..]);
        if rest
            .as_bytes()
            .first()
            .is_some_and(|&c| c != 0 && c != comment_char)
        {
            crate::ported::message::semsg(&format!("E488: Trailing characters: {}", &m[end..]));
            return None;
        }
        marker = m[..end].to_string(); // c:784 *p = NUL
                                       // c:785 non-script markers cannot start with a lower case letter.
        if !script_get
            && marker
                .as_bytes()
                .first()
                .is_some_and(|c| c.is_ascii_lowercase())
        {
            crate::ported::message::emsg("E221: Marker cannot start with lower case letter");
            return None;
        }
    } else if script_get {
        // c:792 embedded script with a missing marker accepts '.'.
        marker = ".".to_string();
    } else {
        crate::ported::message::emsg("E172: Missing marker"); // c:795
        return None;
    }

    let l = crate::ported::eval::typval::tv_list_alloc(0); // c:801
    loop {
        let mut mi = 0usize; // c:803
        let mut ti = 0i32; // c:804
        let theline: String;

        if heredoc_in_string {
            // c:810 get the next line from the string.
            if line_arg.is_empty() {
                if !script_get {
                    crate::ported::message::semsg(&format!("E990: Missing end marker '{marker}'"));
                }
                break;
            }
            match line_arg.find('\n') {
                None => {
                    theline = line_arg.clone();
                    line_arg = String::new(); // c:820 line_arg += strlen(line_arg)
                }
                Some(nl) => {
                    theline = line_arg[..nl].to_string();
                    line_arg = line_arg[nl + 1..].to_string(); // c:823
                }
            }
        } else {
            // c:827 theline = eap->ea_getline(...)
            match eap.ea_getline.as_mut().and_then(|g| g()) {
                None => {
                    if !script_get {
                        crate::ported::message::semsg(&format!(
                            "E990: Missing end marker '{marker}'"
                        ));
                    }
                    break;
                }
                Some(t) => theline = t,
            }
        }

        // c:838 with "trim": skip the indent matching the :let line.
        if marker_indent_len > 0
            && theline.len() >= marker_indent_len
            && eap.cmdlinep.len() >= marker_indent_len
            && theline.as_bytes()[..marker_indent_len]
                == eap.cmdlinep.as_bytes()[..marker_indent_len]
        {
            mi = marker_indent_len;
        }
        // c:842 if (strcmp(marker, theline + mi) == 0) break;
        if marker == theline[mi..] {
            break;
        }

        // c:848 skip till the end marker after a failed expression.
        if eval_failed {
            continue;
        }

        // c:852 set the text indent from the first line.
        if text_indent_len == -1 && !theline.is_empty() {
            let tb = theline.as_bytes();
            let mut i = 0;
            let mut tl = 0i32;
            while i < tb.len() && iswhite(tb[i]) {
                i += 1;
                tl += 1;
            }
            text_indent_len = tl;
            text_indent = Some(theline[..tl as usize].to_string()); // c:860
        }
        // c:863 with "trim": skip the indent matching the first line.
        if let Some(ind) = &text_indent {
            let tb = theline.as_bytes();
            let ib = ind.as_bytes();
            let mut t = 0i32;
            while (t as usize) < text_indent_len as usize {
                if tb.get(t as usize) != ib.get(t as usize) {
                    break;
                }
                t += 1;
            }
            ti = t;
        }

        let str_slice = &theline[ti as usize..]; // c:871
        if evalstr && !eap.skip {
            // c:873 str = eval_all_expr_in_str(str);
            match eval_all_expr_in_str(str_slice) {
                None => {
                    eval_failed = true; // c:876
                    continue;
                }
                Some(s) => crate::ported::eval::typval::tv_list_append_allocated_string(
                    &mut l.borrow_mut(),
                    s,
                ),
            }
        } else {
            tv_list_append_string(&mut l.borrow_mut(), str_slice); // c:881
        }
    }
    if heredoc_in_string {
        // c:886 next command follows the heredoc in the string.
        eap.nextcmd = Some(line_arg);
    }

    if eval_failed {
        // c:892 expression evaluation in the heredoc failed.
        crate::ported::eval::typval::tv_list_free(&mut l.borrow_mut());
        return None;
    }
    Some(l)
}

/// Port of `ex_let()` from `Src/eval/vars.c:916`.
///
/// The `:let` / `:const` command: list variables, assign an expression, unpack a
/// List, or read a here document.
pub fn ex_let(eap: &mut exarg_T) {
    use crate::ported::eval::{ends_excmd, skipwhite};
    let is_const = eap.cmdidx == cmdidx_T::CMD_const; // c:918
    let arg = eap.arg.clone(); // c:919
    let mut rettv = typval_T::default(); // c:921
    let mut first = 1i32; // c:926

    // c:928 argend = skip_var_list(arg, &var_count, &semicolon, false);
    let (argend_off, var_count, semicolon) = match skip_var_list(&arg, false) {
        Some(x) => x,
        None => return, // c:930
    };
    // c:932 expr = skipwhite(argend);
    let expr_full = skipwhite(&arg[argend_off..]);
    let eb = expr_full.as_bytes();
    let concat = expr_full.starts_with("..="); // c:933
                                               // c:934 has_assign: '=' or one of +-*/%. followed by '='.
    let has_assign = eb.first() == Some(&b'=')
        || (eb.first().is_some_and(|&c| b"+-*/%.".contains(&c)) && eb.get(1) == Some(&b'='));

    if !has_assign && !concat {
        // c:937 ":let" without "=": list variables.
        if arg.as_bytes().first() == Some(&b'[') {
            crate::ported::message::emsg("E474: Invalid argument"); // c:939 e_invarg
        } else if !ends_excmd(arg.as_bytes().first().copied().unwrap_or(0)) {
            // c:942 ":let var1 var2" — listing is a no-op standalone.
            list_arg_vars(&mut first);
        } else if !eap.skip {
            // c:944 ":let" — list every scope (all no-ops standalone).
            list_glob_vars(&mut first);
            list_buf_vars(&mut first);
            list_win_vars(&mut first);
            list_tab_vars(&mut first);
            list_script_vars(&mut first);
            // c:950 list_func_vars(&first) — defined in eval.c, no-op standalone.
            list_vim_vars(&mut first);
        }
        eap.nextcmd = None; // c:953 check_nextcmd(arg)
        return;
    }

    if eb.first() == Some(&b'=') && eb.get(1) == Some(&b'<') && eb.get(2) == Some(&b'<') {
        // c:957 HERE document.
        if let Some(l) = heredoc_get(eap, &expr_full[3..], false) {
            // c:961 tv_list_set_ret(&rettv, l);
            rettv = typval_T {
                v_type: VAR_LIST,
                v_lock: VarLockStatus::VAR_UNLOCKED,
                vval: v_list(Some(l)),
            };
            if !eap.skip {
                // c:965 op = "="
                let arg2 = eap.arg.clone();
                ex_let_vars(
                    &arg2,
                    &mut rettv,
                    false,
                    if semicolon { 1 } else { 0 },
                    var_count,
                    is_const,
                    Some("="),
                );
            }
            crate::ported::eval::typval::tv_clear(&mut rettv); // c:967
        }
        return;
    }

    rettv.v_type = VAR_UNKNOWN; // c:972

    // c:974 op = "="; parse a compound operator.
    let mut op = String::from("=");
    let expr_after: &str;
    if eb.first() != Some(&b'=') {
        let c = eb.first().copied().unwrap_or(0);
        let mut adv = 2usize; // c:983 expr += 2
        if b"+-*/%.".contains(&c) {
            op = (c as char).to_string(); // c:978 op[0] = *expr
            if c == b'.' && eb.get(1) == Some(&b'.') {
                adv = 3; // c:980 ..= : expr++ then expr += 2
            }
        }
        expr_after = &expr_full[adv..];
    } else {
        expr_after = &expr_full[1..]; // c:985 expr += 1
    }
    let expr_s = skipwhite(expr_after); // c:988

    // c:990 eap->skip → emsg_skip++ (not modeled standalone).
    // c:993 fill_evalarg_from_eap / eval0 / clear_evalarg. RUST-PORT NOTE:
    // fill_evalarg_from_eap sets EVAL_EVALUATE unless eap->skip; model that with
    // an evalarg here (passing None would leave EVAL_EVALUATE unset → the RHS is
    // parsed but never evaluated, so the assignment value stays VAR_UNKNOWN).
    let mut evalarg = crate::ported::eval::evalarg_T {
        eval_flags: if eap.skip {
            0
        } else {
            crate::ported::eval::EVAL_EVALUATE
        },
    };
    let eval_res = crate::ported::eval::eval0(expr_s, &mut rettv, Some(&mut evalarg)); // c:995

    if !eap.skip && eval_res != crate::ported::eval_h::FAIL {
        // c:1002
        let arg2 = eap.arg.clone();
        ex_let_vars(
            &arg2,
            &mut rettv,
            false,
            if semicolon { 1 } else { 0 },
            var_count,
            is_const,
            Some(&op),
        );
    }
    if eval_res != crate::ported::eval_h::FAIL {
        crate::ported::eval::typval::tv_clear(&mut rettv); // c:1005
    }
}

/// Port of `ex_let_vars()` from `Src/eval/vars.c:1021`.
///
/// Assign `tv` to the variable(s) at `arg_start`: a single `var`, or a
/// `[v1, v2; rest]` List unpack. Returns `OK` or `FAIL`.
pub fn ex_let_vars(
    arg_start: &str,
    tv: &mut typval_T,
    copy: bool,
    semicolon: i32,
    var_count: i32,
    is_const: bool,
    op: Option<&str>,
) -> i32 {
    use crate::ported::eval::skipwhite;
    use crate::ported::eval_h::{FAIL, OK};

    if arg_start.as_bytes().first() != Some(&b'[') {
        // c:1027 ":let var = expr" or ":for var in list"
        if ex_let_one(arg_start, tv, copy, is_const, op, op).is_none() {
            return FAIL;
        }
        return OK;
    }

    // c:1036 ":let [v1, v2] = list"
    if tv.v_type != VAR_LIST {
        crate::ported::message::emsg("E714: List required");
        return FAIL;
    }
    // c:1040 list_T *const l = tv->vval.v_list;
    let l_opt: Option<Rc<RefCell<list_T>>> = match &tv.vval {
        v_list(x) => x.clone(),
        _ => None,
    };
    let len = l_opt
        .as_ref()
        .map(|l| crate::ported::eval::typval::tv_list_len(&l.borrow()))
        .unwrap_or(0); // c:1042
    if semicolon == 0 && var_count < len {
        crate::ported::message::emsg("E687: Less targets than List items"); // c:1044
        return FAIL;
    }
    if var_count - semicolon > len {
        crate::ported::message::emsg("E688: More targets than List items"); // c:1048
        return FAIL;
    }
    // c:1053 assert(l != NULL);
    let l = match l_opt {
        Some(l) => l,
        None => return FAIL,
    };

    // c:1055 item = tv_list_first(l); rest_len = tv_list_len(l);
    let mut item_idx = 0usize;
    let mut rest_len = len as usize;
    let mut pos = 0usize; // offset into arg_start; starts at '['
    while arg_start.as_bytes()[pos] != b']' {
        // c:1058 arg = skipwhite(arg + 1);
        let after = skipwhite(&arg_start[pos + 1..]);
        let start = arg_start.len() - after.len();
        // c:1059 ex_let_one(arg, TV_LIST_ITEM_TV(item), true, is_const, ",;]", op)
        let mut item_tv = l
            .borrow()
            .lv_items
            .get(item_idx)
            .map(|it| it.li_tv.clone())
            .unwrap_or_default();
        let r = match ex_let_one(
            &arg_start[start..],
            &mut item_tv,
            true,
            is_const,
            Some(",;]"),
            op,
        ) {
            Some(x) => start + x,
            None => return FAIL, // c:1060
        };
        rest_len -= 1; // c:1063
        item_idx += 1; // c:1065 item = TV_LIST_ITEM_NEXT(l, item)
                       // c:1066 arg = skipwhite(arg);
        let sw = skipwhite(&arg_start[r..]);
        let newpos = arg_start.len() - sw.len();
        match arg_start.as_bytes().get(newpos).copied() {
            Some(b';') => {
                // c:1068 put the rest of the list in the var after ';'.
                let rest_list = crate::ported::eval::typval::tv_list_alloc(rest_len as isize); // c:1070
                {
                    let src = l.borrow();
                    let mut rl = rest_list.borrow_mut();
                    let mut it = item_idx;
                    while it < src.lv_items.len() {
                        crate::ported::eval::typval::tv_list_append_tv(
                            &mut rl,
                            src.lv_items[it].li_tv.clone(),
                        );
                        it += 1;
                    }
                }
                // c:1076 ltv.v_type = VAR_LIST; tv_list_ref(rest_list);
                let mut ltv = typval_T {
                    v_type: VAR_LIST,
                    v_lock: VarLockStatus::VAR_UNLOCKED,
                    vval: v_list(Some(Rc::clone(&rest_list))),
                };
                crate::ported::eval::typval::tv_list_ref(&mut rest_list.borrow_mut());
                // c:1081 ex_let_one(skipwhite(arg + 1), &ltv, false, is_const, "]", op)
                let sw2 = skipwhite(&arg_start[newpos + 1..]);
                let start2 = arg_start.len() - sw2.len();
                let r2 = ex_let_one(
                    &arg_start[start2..],
                    &mut ltv,
                    false,
                    is_const,
                    Some("]"),
                    op,
                );
                crate::ported::eval::typval::tv_clear(&mut ltv); // c:1082
                if r2.is_none() {
                    return FAIL; // c:1084
                }
                break; // c:1086
            }
            Some(b',') | Some(b']') => {
                pos = newpos;
            }
            _ => {
                // c:1088 internal_error("ex_let_vars()");
                crate::ported::message::emsg("E473: Internal error: ex_let_vars()");
                return FAIL;
            }
        }
    }

    OK // c:1093
}

/// Port of `ex_let_env()` from `Src/eval/vars.c:1299`.
///
/// Set an environment variable, part of [`ex_let_one`]. Returns the byte offset
/// (into `arg`) just past the name on success, or `None` on error.
fn ex_let_env(
    arg: &str,
    tv: &mut typval_T,
    is_const: bool,
    endchars: Option<&str>,
    op: Option<&str>,
) -> Option<usize> {
    use crate::ported::eval::skipwhite;
    if is_const {
        crate::ported::message::emsg("E996: Cannot lock an environment variable"); // c:1304
        return None;
    }
    let mut arg_end = None; // c:1309
                            // c:1310 arg++; name = arg; len = get_env_len(&arg);
    let after = &arg[1..];
    let len = crate::ported::eval::get_env_len(after) as usize;
    if len == 0 {
        // c:1314 semsg(e_invarg2, name - 1) — name-1 is the '$'.
        crate::ported::message::semsg(&format!("E475: Invalid argument: {arg}"));
    } else {
        let opc = op.and_then(|o| o.as_bytes().first().copied());
        if op.is_some() && matches!(opc, Some(c) if b"+-*/%".contains(&c)) {
            // c:1317 semsg(e_letwrong, op)
            crate::ported::message::semsg(&format!(
                "E734: Wrong variable type for {}=",
                op.unwrap()
            ));
        } else if endchars.is_some() && {
            // c:1318 vim_strchr(endchars, *skipwhite(arg)) == NULL
            let nc = skipwhite(&after[len..])
                .as_bytes()
                .first()
                .copied()
                .unwrap_or(0);
            !(nc == 0 || endchars.unwrap().as_bytes().contains(&nc))
        } {
            crate::ported::message::emsg("E18: Unexpected characters in :let"); // c:1320
        } else {
            // c:1321 !check_secure() — 'secure'/sandbox not modeled (always allowed).
            let name = &after[..len]; // c:1324 name[len] = NUL
            let mut p = crate::ported::eval::typval::tv_get_string_chk(tv); // c:1325
            if let (Some(pv), Some('.')) = (&p, op.and_then(|o| o.chars().next())) {
                // c:1326 op == '.' : concatenate with the current value.
                if let Ok(s) = std::env::var(name) {
                    // c:1329 concat_str(s, p)
                    p = Some(format!("{s}{pv}"));
                }
            }
            if let Some(pv) = p {
                // c:1335 vim_setenv_ext(name, p)
                std::env::set_var(name, &pv);
                arg_end = Some(1 + len); // c:1336
            }
        }
    }
    arg_end
}

/// Port of `ex_let_option()` from `Src/eval/vars.c:1346`.
///
/// Set an option, part of [`ex_let_one`]. Ports the assignment through the
/// OptVal-typed layer in [`crate::ported::option_optval`]: isolate the name with
/// `find_option_var_end`, read the current value with `get_option_value`, then
/// apply the new value with `set_option_from_tv`.
///
/// RUST-PORT NOTE: `tv_to_optval` and the `NUMBER_OPTVAL`/`BOOLEAN_OPTVAL`
/// constructors (and the `newval` OptVal that C's `+=`/`-=`/… arithmetic and the
/// final `set_option_value_handle_tty` operate on) are file-private to
/// `option_optval`. The public boundary exposes `set_option_from_tv`
/// (= `tv_to_optval` + `set_option_value_handle_tty`), so the compound-op result
/// is assembled as a `typval_T` — the arithmetic ported verbatim from
/// c:1399-1428 at `OptInt`/`varnumber_T` = i64 — and applied via
/// `set_option_from_tv`; the net stored value matches the C control flow. The
/// operand-conversion error path (`tv_to_optval`'s E521/E928, c:1387-1390) is
/// carried by `set_option_from_tv` for `=` but not re-derived for the compound
/// operand. `is_option_hidden`/`get_tty_option` are not modelled: `hidden` is
/// treated as false and tty options (`is_tty_option`) are accepted silently
/// (c:1432 fails silently for them).
/// RUST-PORT NOTE: the `option_optval` `find_option_var_end` returns
/// `(name, opt_idx, opt_flags)` without the C `end` pointer, so the offset `p`
/// just past the option name is recomputed from the sigil + optional `g:`/`l:`
/// scope prefix + name length.
fn ex_let_option(
    arg: &str,
    tv: &mut typval_T,
    is_const: bool,
    endchars: Option<&str>,
    op: Option<&str>,
) -> Option<usize> {
    use crate::ported::eval::skipwhite;
    use crate::ported::option_optval::{
        find_option_var_end, get_option_value, is_tty_option, set_option_from_tv, OptValData,
        OptValType,
    };
    if is_const {
        crate::ported::message::emsg("E996: Cannot lock an option"); // c:1351
        return None;
    }

    // c:1360 find_option_var_end(&arg, &opt_idx, &opt_flags)
    let (name, opt_idx, opt_flags) = find_option_var_end(arg);
    // c:1360 recompute the end offset `p` (see RUST-PORT NOTE).
    let b = arg.as_bytes();
    let prefix = if b.len() >= 3 && (b[1] == b'g' || b[1] == b'l') && b[2] == b':' {
        2
    } else {
        0
    };
    // c:1362 p == NULL || endchars check → e_letunexp.
    let Some(name) = name else {
        crate::ported::message::emsg("E18: Unexpected characters in :let"); // c:1363
        return None;
    };
    let p = 1 + prefix + name.len();
    let nc = skipwhite(&arg[p.min(arg.len())..])
        .as_bytes()
        .first()
        .copied()
        .unwrap_or(0);
    let end_ok = match endchars {
        None => true,
        Some(ec) => nc == 0 || ec.as_bytes().contains(&nc),
    };
    if !end_ok {
        crate::ported::message::emsg("E18: Unexpected characters in :let"); // c:1363
        return None;
    }

    // c:1370 is_tty_opt: tty options have no store standalone — accept silently
    // (c:1432 set_option_value_handle_tty returns NULL for them). RUST-PORT NOTE.
    if is_tty_option(&name) {
        return Some(p);
    }

    // c:1372 curval = get_option_value(opt_idx, opt_flags)
    let curval = get_option_value(opt_idx, opt_flags);
    // c:1375 if (curval.type == kOptValTypeNil) → unknown option.
    if curval.r#type == OptValType::kOptValTypeNil {
        // e_unknown_option2 c:1376
        crate::ported::message::semsg(&format!("E355: Unknown option: {name}"));
        return None;
    }
    // c:1379 op type must match curval type ('.' only for strings).
    if let Some(o) = op {
        let oc = o.as_bytes().first().copied().unwrap_or(0);
        let is_string = curval.r#type == OptValType::kOptValTypeString;
        if oc != b'=' && ((!is_string && oc == b'.') || (is_string && oc != b'.')) {
            // e_letwrong c:1382
            crate::ported::message::semsg(&format!("E734: Wrong variable type for {o}="));
            return None;
        }
    }

    // c:1387 newval = tv_to_optval(tv, …); c:1397-1429 apply the compound op.
    let combined: typval_T = match op {
        Some(o) if o.as_bytes().first().copied().unwrap_or(0) != b'=' => {
            let oc = o.as_bytes()[0];
            if curval.r#type == OptValType::kOptValTypeString {
                // c:1420 string: concat curval + newval.
                let cur_s = match &curval.data {
                    OptValData::string(s) => s.clone(),
                    _ => String::new(),
                };
                typval_T::from(format!("{cur_s}{}", tv_get_string(tv))) // c:1426 concat_str
            } else {
                // c:1398 number or bool.
                let cur_n = match &curval.data {
                    OptValData::number(n) => *n as varnumber_T, // c:1399
                    OptValData::boolean(t) => *t as i64 as varnumber_T,
                    _ => 0,
                };
                let new_n = crate::ported::eval::typval::tv_get_number_chk(tv, None); // c:1400
                let r = match oc {
                    b'+' => cur_n + new_n,                                  // c:1404
                    b'-' => cur_n - new_n,                                  // c:1406
                    b'*' => cur_n * new_n,                                  // c:1408
                    b'/' => crate::ported::eval::num_divide(cur_n, new_n),  // c:1410
                    b'%' => crate::ported::eval::num_modulus(cur_n, new_n), // c:1412
                    _ => new_n,
                };
                // c:1416 NUMBER_OPTVAL / c:1418 BOOLEAN_OPTVAL(TRISTATE_FROM_INT).
                typval_T::from(r as varnumber_T)
            }
        }
        // c:1387 plain '=' / no op: the value is `tv` itself.
        _ => tv.clone(),
    };
    // c:1432 set_option_value_handle_tty(arg, opt_idx, newval, opt_flags) via the
    // public set_option_from_tv (tv_to_optval + set).
    set_option_from_tv(&name, &combined);
    Some(p) // c:1433 arg_end = p
}

/// Port of `ex_let_register()` from `Src/eval/vars.c:1446`.
///
/// Set a register, part of [`ex_let_one`]. Writes the register through
/// [`crate::ported::ops`] (`get_reg_contents` for the `.=` append, then
/// `write_reg_contents`). Returns the byte offset (into `arg`) just past the
/// register name, or `None` on error.
///
/// RUST-PORT NOTE: the reduced `write_reg_contents(name, value, mtype, append)`
/// takes an explicit `MotionType` where C passes the `kMTUnknown` auto-detect
/// sentinel; charwise is used (the common `:let @r = "text"` case). The reduced
/// `get_reg_contents` returns the register lines (C's `kGRegExprSrc` joined
/// string), joined with `\n` for the `.=` concat.
fn ex_let_register(
    arg: &str,
    tv: &mut typval_T,
    is_const: bool,
    endchars: Option<&str>,
    op: Option<&str>,
) -> Option<usize> {
    use crate::ported::eval::skipwhite;
    use crate::ported::eval::typval::tv_get_string_chk;
    use crate::ported::ops::{get_reg_contents, write_reg_contents, MotionType};
    if is_const {
        crate::ported::message::emsg("E996: Cannot lock a register"); // c:1451
        return None;
    }
    let mut arg_end = None; // c:1455
                            // c:1456 arg++ → the register char is arg[1].
    let b = arg.as_bytes();
    let regbyte = b.get(1).copied().unwrap_or(0);
    let opc = op.and_then(|o| o.as_bytes().first().copied());
    if op.is_some() && matches!(opc, Some(c) if b"+-*/%".contains(&c)) {
        // c:1458 semsg(e_letwrong, op)
        crate::ported::message::semsg(&format!("E734: Wrong variable type for {}=", op.unwrap()));
    } else if endchars.is_some() && {
        // c:1460 vim_strchr(endchars, *skipwhite(arg + 1)) == NULL
        let nc = skipwhite(arg.get(2..).unwrap_or(""))
            .as_bytes()
            .first()
            .copied()
            .unwrap_or(0);
        !(nc == 0 || endchars.unwrap().as_bytes().contains(&nc))
    } {
        crate::ported::message::emsg("E18: Unexpected characters in :let"); // c:1461
    } else {
        // c:1463 ptofree = NULL
        let mut p = tv_get_string_chk(tv); // c:1464
                                           // c:1466/1474 *arg == '@' ? '"' : *arg
        let regname = if regbyte == b'@' {
            '"'
        } else {
            regbyte as char
        };
        if let (Some(pv), Some(b'.')) = (&p, opc) {
            // c:1465 op == '.' : append to the current register contents.
            if let Some(lines) = get_reg_contents(regname) {
                // c:1468 concat_str(s, p)
                let s = lines.join("\n");
                p = Some(format!("{s}{pv}"));
            }
        }
        if let Some(pv) = p {
            // c:1474 write_reg_contents(reg, p, strlen(p), false)
            write_reg_contents(regname, &pv, MotionType::CharWise, false);
            arg_end = Some(2); // c:1475 arg_end = arg + 1 (sigil + register char)
        }
    }
    arg_end
}

/// Port of `ex_let_one()` from `Src/eval/vars.c:1493`.
///
/// Set one item of `:let var = expr` (or one target of a List unpack) to its
/// value. `endchars` are the valid characters after the name (or `None`); `op`
/// is `None` for `=` or e.g. `"+"` for `+=`. Returns the byte offset (into
/// `arg`) just past the name, or `None` on error.
fn ex_let_one(
    arg: &str,
    tv: &mut typval_T,
    copy: bool,
    is_const: bool,
    endchars: Option<&str>,
    op: Option<&str>,
) -> Option<usize> {
    use crate::ported::eval::skipwhite;
    let mut arg_end = None; // c:1497
    match arg.as_bytes().first().copied() {
        Some(b'$') => {
            // c:1499 ":let $VAR = expr": Set environment variable.
            ex_let_env(arg, tv, is_const, endchars, op)
        }
        Some(b'&') => {
            // c:1502 ":let &option = expr": Set option value.
            ex_let_option(arg, tv, is_const, endchars, op)
        }
        Some(b'@') => {
            // c:1507 ":let @r = expr": Set register contents.
            ex_let_register(arg, tv, is_const, endchars, op)
        }
        Some(c) if crate::ported::eval::eval_isnamec1(c) || c == b'{' => {
            // c:1510 ":let var = expr" / ":let {expr} = expr": internal variable.
            let mut lv = crate::ported::eval::lval_T::default();
            let p = crate::ported::eval::get_lval(
                arg,
                Some(&*tv),
                &mut lv,
                false,
                false,
                0,
                crate::ported::eval::FNE_CHECK_START,
            ); // c:1514
            if let Some(p) = p {
                if lv.ll_name.is_some() {
                    // c:1516 endchars check (vim_strchr(endchars, *skipwhite(p))).
                    let nc = skipwhite(&arg[p..])
                        .as_bytes()
                        .first()
                        .copied()
                        .unwrap_or(0);
                    let end_ok = match endchars {
                        None => true,
                        Some(ec) => nc == 0 || ec.as_bytes().contains(&nc),
                    };
                    if !end_ok {
                        crate::ported::message::emsg("E18: Unexpected characters in :let");
                    // c:1517
                    } else {
                        // c:1519 set_var_lval(&lv, p, tv, copy, is_const, op);
                        crate::ported::eval::set_var_lval(&mut lv, p, tv, copy, is_const, op);
                        arg_end = Some(p); // c:1520
                    }
                }
            }
            crate::ported::eval::clear_lval(&mut lv); // c:1523
            arg_end
        }
        _ => {
            crate::ported::message::semsg(&format!("E475: Invalid argument: {arg}")); // c:1525
            None
        }
    }
}

/// Port of `ex_unlet()` from `Src/eval/vars.c:1532` — the `:unlet[!]` command.
pub fn ex_unlet(eap: &mut exarg_T) {
    let argstart = eap.arg.clone();
    let glv_flags = if eap.forceit {
        crate::ported::eval::GLV_QUIET
    } else {
        0
    };
    ex_unletlock(eap, &argstart, 0, glv_flags, do_unlet_var);
}

/// Port of `ex_lockvar()` from `Src/eval/vars.c:1538` — `:lockvar`/`:unlockvar`.
pub fn ex_lockvar(eap: &mut exarg_T) {
    use crate::ported::eval::skipwhite;
    let mut arg = eap.arg.clone(); // c:1540
    let mut deep = 2i32; // c:1541

    if eap.forceit {
        deep = -1; // c:1544
    } else if arg.as_bytes().first().is_some_and(|c| c.is_ascii_digit()) {
        // c:1546 deep = getdigits_int(&arg, false, -1);
        let digits: String = arg.chars().take_while(|c| c.is_ascii_digit()).collect();
        deep = digits.parse::<i32>().unwrap_or(-1);
        arg = skipwhite(&arg[digits.len()..]).to_string(); // c:1547
    }

    ex_unletlock(eap, &arg, deep, 0, do_lock_var); // c:1550
}

/// Port of `ex_unletlock()` from `Src/eval/vars.c:1562`.
///
/// Common parsing loop for `:unlet`, `:lockvar` and `:unlockvar`: parse each
/// name (or `$ENV`), then invoke `callback` when not skipping / not already in
/// error.
fn ex_unletlock(
    eap: &mut exarg_T,
    argstart: &str,
    deep: i32,
    glv_flags: i32,
    callback: ex_unletlock_callback,
) {
    use crate::ported::eval::{ends_excmd, skipwhite};
    use crate::ported::eval_h::FAIL;

    let mut arg_pos = 0usize; // offset into argstart
    let mut error = false; // c:1568
    loop {
        let arg = &argstart[arg_pos..];
        let name_end_off: usize;
        if arg.as_bytes().first() == Some(&b'$') {
            // c:1572 environment variable.
            let mut lv = crate::ported::eval::lval_T::default();
            lv.ll_name = Some(arg.to_string()); // c:1573
                                                // c:1574 lv.ll_tv = NULL (default LlTv::Null)
            let after = &arg[1..]; // c:1575 arg++
            let envlen = crate::ported::eval::get_env_len(after) as usize;
            if envlen == 0 {
                // c:1577 semsg(e_invarg2, arg - 1) — the '$'.
                crate::ported::message::semsg(&format!("E475: Invalid argument: {arg}"));
                return;
            }
            let name_end_slice = &after[envlen..];
            if !error && !eap.skip && callback(&mut lv, name_end_slice, eap, deep) == FAIL {
                error = true; // c:1582
            }
            name_end_off = arg_pos + 1 + envlen; // c:1584
        } else {
            // c:1587 parse the name and find the end.
            let mut lv = crate::ported::eval::lval_T::default();
            let p = crate::ported::eval::get_lval(
                arg,
                None,
                &mut lv,
                true,
                eap.skip || error,
                glv_flags,
                crate::ported::eval::FNE_CHECK_START,
            );
            if lv.ll_name.is_none() {
                error = true; // c:1590
            }
            let ne = p.map(|x| arg_pos + x);
            // c:1592 name_end == NULL or not whitespace/end-of-command → trailing.
            let bad = match ne {
                None => true,
                Some(off) => {
                    let c = argstart.as_bytes().get(off).copied().unwrap_or(0);
                    !(c == b' ' || c == b'\t') && !ends_excmd(c)
                }
            };
            if bad {
                if let Some(off) = ne {
                    crate::ported::message::semsg(&format!(
                        "E488: Trailing characters: {}",
                        &argstart[off..]
                    )); // c:1596
                }
                if !(eap.skip || error) {
                    crate::ported::eval::clear_lval(&mut lv); // c:1599
                }
                break; // c:1601
            }
            let off = ne.unwrap();
            if !error && !eap.skip && callback(&mut lv, &argstart[off..], eap, deep) == FAIL {
                error = true; // c:1604
            }
            if !eap.skip {
                crate::ported::eval::clear_lval(&mut lv); // c:1609
            }
            name_end_off = off;
        }
        // c:1612 arg = skipwhite(name_end);
        let sw = skipwhite(&argstart[name_end_off..]);
        arg_pos = argstart.len() - sw.len();
        if ends_excmd(argstart.as_bytes().get(arg_pos).copied().unwrap_or(0)) {
            break; // c:1613 while (!ends_excmd(*arg))
        }
    }
    eap.nextcmd = None; // c:1615 check_nextcmd(arg)
}

/// Port of `do_unlet_var()` from `Src/eval/vars.c:1626`.
///
/// Unlet the variable indicated by `lp`. RUST-PORT NOTE: the C `*name_end = NUL`
/// bracketing is elided — `lp.ll_name` is already an owned exact substring.
fn do_unlet_var(
    lp: &mut crate::ported::eval::lval_T,
    _name_end: &str,
    eap: &mut exarg_T,
    _deep: i32,
) -> i32 {
    use crate::ported::eval::LlTv;
    use crate::ported::eval_h::{FAIL, OK};
    let forceit = eap.forceit; // c:1629
    let mut ret = OK; // c:1630

    if matches!(lp.ll_tv, LlTv::Null) {
        // c:1632 environment variable, normal name or expanded name.
        let name = lp.ll_name.clone().unwrap_or_default();
        if name.as_bytes().first() == Some(&b'$') {
            // c:1638 vim_unsetenv_ext(lp->ll_name + 1)
            std::env::remove_var(&name[1..]);
        } else if do_unlet(&name, lp.ll_name_len, forceit) == FAIL {
            ret = FAIL; // c:1640
        }
    } else if {
        // c:1643 fail when the containing List/Dict is locked.
        let name = lp.ll_name.as_deref();
        let list_locked = lp.ll_list.as_ref().is_some_and(|l| {
            crate::ported::eval::typval::value_check_lock(l.borrow().lv_lock, name, lp.ll_name_len)
        });
        list_locked
            || lp.ll_dict.as_ref().is_some_and(|d| {
                crate::ported::eval::typval::value_check_lock(
                    d.borrow().dv_lock,
                    name,
                    lp.ll_name_len,
                )
            })
    } {
        return FAIL; // c:1653
    } else if lp.ll_range {
        // c:1655 unlet a range of List items.
        let l = lp.ll_list.clone().expect("ll_range implies ll_list");
        tv_list_unlet_range(
            &mut l.borrow_mut(),
            lp.ll_li.unwrap_or(0),
            lp.ll_n1,
            !lp.ll_empty2,
            lp.ll_n2,
        );
    } else if let Some(l) = lp.ll_list.clone() {
        // c:1657 unlet a List item.
        crate::ported::eval::typval::tv_list_item_remove(
            &mut l.borrow_mut(),
            lp.ll_li.unwrap_or(0),
        );
    } else {
        // c:1660 unlet a Dict item. RUST-PORT NOTE: dict watchers not modeled.
        if let (Some(d), Some(key)) = (lp.ll_dict.clone(), lp.ll_di.clone()) {
            crate::ported::eval::typval::tv_dict_item_remove(&mut d.borrow_mut(), &key);
        }
    }

    ret // c:1683
}

/// Port of `do_lock_var()` from `Src/eval/vars.c:1786`.
///
/// Lock (`:lockvar`) or unlock (`:unlockvar`) the variable indicated by `lp`.
/// RUST-PORT NOTE: `dictitem_T.di_flags` is not modeled on scope entries, so the
/// `DI_FLAGS_FIX` guard and the `DI_FLAGS_LOCK` flag toggle are elided; the
/// observable value lock (`tv_item_lock`) is applied. For a plain scope variable
/// [`find_var`] returns a clone, so the locked value is written back through
/// [`set_var`] to persist the lock.
fn do_lock_var(
    lp: &mut crate::ported::eval::lval_T,
    _name_end: &str,
    eap: &mut exarg_T,
    deep: i32,
) -> i32 {
    use crate::ported::eval::typval::tv_item_lock;
    use crate::ported::eval::LlTv;
    use crate::ported::eval_h::{FAIL, OK};
    let lock = eap.cmdidx == cmdidx_T::CMD_lockvar; // c:1789
    let mut ret = OK; // c:1790

    if matches!(lp.ll_tv, LlTv::Null) {
        let name = lp.ll_name.clone().unwrap_or_default();
        if name.as_bytes().first() == Some(&b'$') {
            // c:1794 semsg(e_lock_unlock, lp->ll_name)
            crate::ported::message::semsg(&format!("E940: Cannot lock or unlock variable {name}"));
            ret = FAIL;
        } else {
            // c:1798 di = find_var(lp->ll_name, ..., true)
            match find_var(&name, true) {
                None => ret = FAIL, // c:1801
                Some(mut di_tv) => {
                    // c:1802 DI_FLAGS_FIX guard elided (di_flags not modeled).
                    // c:1810 di->di_flags LOCK toggle elided (di_flags not modeled).
                    if deep != 0 {
                        // c:1816 tv_item_lock(&di->di_tv, deep, lock, false);
                        tv_item_lock(&mut di_tv, deep, lock, false);
                        set_var(&name, name.len(), di_tv, false);
                    }
                }
            }
        }
    } else if deep == 0 {
        // c:1820 nothing to do
    } else if lp.ll_range {
        // c:1822 (un)lock a range of List items.
        let l = lp.ll_list.clone().expect("ll_range implies ll_list");
        let mut li = lp.ll_li.unwrap_or(0);
        loop {
            let len = l.borrow().lv_items.len();
            if !(li < len && (lp.ll_empty2 || lp.ll_n2 >= lp.ll_n1)) {
                break;
            }
            tv_item_lock(&mut l.borrow_mut().lv_items[li].li_tv, deep, lock, false); // c:1827
            li += 1; // c:1828 TV_LIST_ITEM_NEXT
            lp.ll_n1 += 1; // c:1829
        }
    } else if let Some(l) = lp.ll_list.clone() {
        // c:1831 (un)lock a List item.
        let li = lp.ll_li.unwrap_or(0);
        let mut b = l.borrow_mut();
        if li < b.lv_items.len() {
            tv_item_lock(&mut b.lv_items[li].li_tv, deep, lock, false);
        }
    } else {
        // c:1834 (un)lock a Dict item.
        if let (Some(d), Some(key)) = (lp.ll_dict.clone(), lp.ll_di.clone()) {
            let mut b = d.borrow_mut();
            if let Some(tv) = b.dv_hashtab.get_mut(&key) {
                tv_item_lock(tv, deep, lock, false);
            }
        }
    }

    ret // c:1839
}

// ── window / tab-page scoped variables (getwinvar / setwinvar) ───────────────

/// Port of `get_var_from()` from `Src/eval/vars.c:3081`.
///
/// Read a scoped variable (`htname` = `'w'`/`'t'`) for window `win` in tab `tp`
/// into `rettv`, falling back to `deftv` when the variable does not exist.
///
/// RUST-PORT NOTE: C reads `win->w_vars`/`tp->tp_vars` after switching to the
/// target window; the reduced model has no per-window/-tab dict — the `w:`/`t:`
/// scope is the thread-local [`window_vars`]/[`tabpage_vars`] resolved through
/// [`eval_variable`] (as [`set_var`] already models those scopes). The
/// `need_switch_win`/`switch_win` gate is ported faithfully: `switch_win`
/// (window.rs) fails for a non-current window, so only the current window's `w:`
/// scope is readable. The `htname == 'b'` (buffer) branch, `do_change_curbuf`,
/// and `get_winbuf_options` (the whole-options dict for `&`) are not modelled;
/// the `emsg_off` counter has no analog. The `option_optval` split store means a
/// `&opt` read here goes through [`crate::ported::eval::eval_option`] (the
/// `option.rs` store), matching C's `eval_option` call.
fn get_var_from(
    varname: Option<String>,
    rettv: &mut typval_T,
    deftv: Option<&typval_T>,
    htname: char,
    tp: Option<Rc<RefCell<crate::ported::window::tabpage_T>>>,
    win: Option<Rc<RefCell<crate::ported::window::win_T>>>,
) {
    let mut done = false; // c:3084
                          // c:3089 rettv->v_type = VAR_STRING; v_string = NULL → "".
    *rettv = typval_T::from(String::new());

    // c:3092 varname != NULL && tp != NULL && win != NULL (htname != 'b' here).
    if let (Some(varname), Some(tp), Some(win)) = (varname.as_deref(), tp.as_ref(), win.as_ref()) {
        // c:3098 need_switch_win = !(tp == curtab && win == curwin).
        let is_cur_tab = crate::ported::window::curtab
            .with(|c| c.borrow().as_ref().is_some_and(|ct| Rc::ptr_eq(ct, tp)));
        let is_cur_win = crate::ported::window::curwin
            .with(|c| c.borrow().as_ref().is_some_and(|cw| Rc::ptr_eq(cw, win)));
        let need_switch_win = !(is_cur_tab && is_cur_win);
        // c:3100 !need_switch_win || switch_win(...) == OK
        if !need_switch_win
            || crate::ported::eval::window::switch_win() == crate::ported::eval_h::OK
        {
            if varname.starts_with('&') && htname != 't' {
                // c:3101 option value.
                if varname.len() == 1 {
                    // c:3109 whole window-local options dict (get_winbuf_options)
                    // is not modelled — leave `done` false → falls to default.
                } else {
                    // c:3117 eval_option(&varname, rettv, true) == OK → local option.
                    let mut vp: &str = varname;
                    if crate::ported::eval::eval_option(&mut vp, rettv, true)
                        == crate::ported::eval_h::OK
                    {
                        done = true;
                    }
                }
            } else if varname.is_empty() {
                // c:3123 empty string: the whole scope dict (w:/t:).
                if let Some(tv) = eval_variable(&format!("{htname}:")) {
                    *rettv = tv; // c:3133 tv_copy
                    done = true;
                }
            } else {
                // c:3135 look up the variable in the scope hashtable.
                if let Some(tv) = eval_variable(&format!("{htname}:{varname}")) {
                    *rettv = tv; // c:3149 tv_copy
                    done = true;
                }
            }
        }
        // c:3155 restore_win: switch_win never succeeds standalone → nothing to
        // restore.
    }

    // c:3161 if (!done && deftv->v_type != VAR_UNKNOWN) tv_copy(deftv, rettv).
    if !done {
        if let Some(d) = deftv {
            if d.v_type != VAR_UNKNOWN {
                *rettv = d.clone();
            }
        }
    }
}

/// Port of `getwinvar()` from `Src/eval/vars.c:3172` — the `getwinvar()` and
/// `gettabwinvar()` builtins (`off == 1` for `gettabwinvar()`).
pub fn getwinvar(argvars: &[typval_T], rettv: &mut typval_T, off: usize) {
    // c:3176 off == 1 → gettabwinvar(): resolve the tab page.
    let tp = if off == 1 {
        crate::ported::window::find_tabpage(crate::ported::eval::typval::tv_get_number_chk(
            &argvars[0],
            None,
        ) as i32)
    } else {
        crate::ported::window::curtab.with(|c| c.borrow().clone()) // c:3179 tp = curtab
    };
    // c:3181 win = find_win_by_nr(&argvars[off], tp)
    let win = crate::ported::eval::window::find_win_by_nr(&argvars[off], tp.clone());
    // c:3182 varname = tv_get_string_chk(&argvars[off + 1])
    let varname = crate::ported::eval::typval::tv_get_string_chk(&argvars[off + 1]);
    // c:3184 get_var_from(varname, rettv, &argvars[off + 2], 'w', tp, win, NULL)
    get_var_from(varname, rettv, argvars.get(off + 2), 'w', tp, win);
}

/// Port of `setwinvar()` from `Src/eval/vars.c:3308` — the `setwinvar()` and
/// `settabwinvar()` builtins (`off == 1` for `settabwinvar()`).
///
/// RUST-PORT NOTE: `check_secure()` is not modelled (always allowed). The
/// `need_switch_win`/`switch_win` gate is faithful: `switch_win` fails for a
/// non-current window, so only the current window's `w:` scope (the thread-local
/// [`window_vars`]) is writable. A `&opt` write goes through
/// [`crate::ported::option_optval::set_option_from_tv`] as in C.
pub fn setwinvar(argvars: &[typval_T], off: usize) {
    // c:3310 check_secure() — not modelled.
    // c:3314 off == 1 → settabwinvar(): resolve the tab page.
    let tp = if off == 1 {
        crate::ported::window::find_tabpage(crate::ported::eval::typval::tv_get_number_chk(
            &argvars[0],
            None,
        ) as i32)
    } else {
        crate::ported::window::curtab.with(|c| c.borrow().clone()) // c:3318 tp = curtab
    };
    // c:3320 win = find_win_by_nr(&argvars[off], tp)
    let win = crate::ported::eval::window::find_win_by_nr(&argvars[off], tp.clone());
    // c:3321 varname = tv_get_string_chk(&argvars[off + 1])
    let varname = crate::ported::eval::typval::tv_get_string_chk(&argvars[off + 1]);
    // c:3322 varp = &argvars[off + 2]
    let varp = match argvars.get(off + 2) {
        Some(v) => v,
        None => return,
    };

    // c:3324 if (win == NULL || varname == NULL) return
    let (Some(win), Some(varname)) = (win, varname) else {
        return;
    };

    // c:3328 need_switch_win = !(tp == curtab && win == curwin).
    let is_cur_tab = crate::ported::window::curtab.with(|c| {
        c.borrow()
            .as_ref()
            .is_some_and(|ct| tp.as_ref().is_some_and(|t| Rc::ptr_eq(ct, t)))
    });
    let is_cur_win = crate::ported::window::curwin
        .with(|c| c.borrow().as_ref().is_some_and(|cw| Rc::ptr_eq(cw, &win)));
    let need_switch_win = !(is_cur_tab && is_cur_win);
    // c:3330 !need_switch_win || switch_win(...) == OK
    if !need_switch_win || crate::ported::eval::window::switch_win() == crate::ported::eval_h::OK {
        if let Some(optname) = varname.strip_prefix('&') {
            // c:3332 set_option_from_tv(varname + 1, varp)
            crate::ported::option_optval::set_option_from_tv(optname, varp);
        } else {
            // c:3334 winvarname = "w:" + varname; set_var(winvarname, len, varp, true)
            let winvarname = format!("w:{varname}");
            set_var(&winvarname, winvarname.len(), varp.clone(), true);
        }
    }
    // c:3342 restore_win: switch_win never succeeds standalone → nothing to
    // restore.
}

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
        assert!(matches!(
            find_var_in_ht(&d, b'g', "x", false).map(|tv| &tv.vval),
            Some(v_number(5))
        ));
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

#[cfg(test)]
mod let_driver_tests {
    use super::*;
    use crate::ported::eval::typval::tv_get_string;
    use crate::ported::eval::typval_defs_h::{
        typval_vval_union::{v_dict, v_list, v_number},
        varnumber_T, VarLockStatus,
    };

    fn let_eap(arg: &str) -> exarg_T {
        exarg_T {
            arg: arg.to_string(),
            cmdidx: cmdidx_T::CMD_let,
            ..Default::default()
        }
    }

    fn as_num(name: &str) -> Option<varnumber_T> {
        eval_variable(name).and_then(|tv| match tv.vval {
            v_number(n) => Some(n),
            _ => None,
        })
    }

    // The :let RHS is evaluated through EVAL_STRING_HOOK (the bridge integration
    // point ex_let_one/get_lval delegate sub-expressions to). Install the ported
    // eval0 tree-walker as the hook so the driver ports evaluate their operands.
    fn install_eval_hook() {
        use crate::ported::eval::typval::EVAL_STRING_HOOK;
        fn hook(src: &str) -> Option<crate::ported::eval::typval_defs_h::typval_T> {
            let mut tv = crate::ported::eval::typval_defs_h::typval_T::default();
            let mut ev = crate::ported::eval::evalarg_T {
                eval_flags: crate::ported::eval::EVAL_EVALUATE,
            };
            if crate::ported::eval::eval0(src, &mut tv, Some(&mut ev)) == crate::ported::eval_h::OK
            {
                Some(tv)
            } else {
                None
            }
        }
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = Some(hook));
    }

    #[test]
    fn ex_let_scalar_and_compound() {
        install_eval_hook();
        // ":let g:a = 5" then ":let g:a += 3" -> 8.
        ex_let(&mut let_eap("g:ld_a = 5"));
        assert_eq!(as_num("g:ld_a"), Some(5));
        ex_let(&mut let_eap("g:ld_a += 3"));
        assert_eq!(as_num("g:ld_a"), Some(8));
        // ".=" string concat via set_var_lval.
        ex_let(&mut let_eap("g:ld_s = 'ab'"));
        ex_let(&mut let_eap("g:ld_s .= 'cd'"));
        assert_eq!(tv_get_string(&eval_variable("g:ld_s").unwrap()), "abcd");
    }

    #[test]
    fn ex_let_list_unpack() {
        install_eval_hook();
        ex_let(&mut let_eap("[g:ld_x, g:ld_y] = [11, 22]"));
        assert_eq!(as_num("g:ld_x"), Some(11));
        assert_eq!(as_num("g:ld_y"), Some(22));
    }

    #[test]
    fn ex_let_list_unpack_semicolon_rest() {
        install_eval_hook();
        // [head; tail] captures the remaining items in `tail`.
        ex_let(&mut let_eap("[g:ld_h; g:ld_t] = [1, 2, 3]"));
        assert_eq!(as_num("g:ld_h"), Some(1));
        let tail = eval_variable("g:ld_t").unwrap();
        match tail.vval {
            v_list(Some(l)) => {
                let nums: Vec<varnumber_T> = l
                    .borrow()
                    .lv_items
                    .iter()
                    .map(|it| match it.li_tv.vval {
                        v_number(n) => n,
                        _ => -1,
                    })
                    .collect();
                assert_eq!(nums, vec![2, 3]);
            }
            _ => panic!("tail is not a List"),
        }
    }

    #[test]
    fn ex_let_env_and_ex_unlet() {
        install_eval_hook();
        // ":let $VAR = 'v'" sets the process environment.
        ex_let(&mut let_eap("$VIMLRS_LD_ENV = 'envval'"));
        assert_eq!(std::env::var("VIMLRS_LD_ENV").as_deref(), Ok("envval"));
        std::env::remove_var("VIMLRS_LD_ENV");

        // ":unlet g:var" removes a global.
        ex_let(&mut let_eap("g:ld_u = 7"));
        assert!(eval_variable("g:ld_u").is_some());
        let mut eap = exarg_T {
            arg: "g:ld_u".to_string(),
            cmdidx: cmdidx_T::CMD_unlet,
            ..Default::default()
        };
        ex_unlet(&mut eap);
        assert!(eval_variable("g:ld_u").is_none());
    }

    #[test]
    fn ex_unlet_dict_item() {
        install_eval_hook();
        // Build g:d = {'a': 1, 'b': 2} then ":unlet g:d.a".
        ex_let(&mut let_eap("g:ld_d = {'a': 1, 'b': 2}"));
        let mut eap = exarg_T {
            arg: "g:ld_d.a".to_string(),
            cmdidx: cmdidx_T::CMD_unlet,
            ..Default::default()
        };
        ex_unlet(&mut eap);
        let d = eval_variable("g:ld_d").unwrap();
        match d.vval {
            v_dict(Some(dd)) => {
                assert!(!dd.borrow().dv_hashtab.contains_key("a"));
                assert!(dd.borrow().dv_hashtab.contains_key("b"));
            }
            _ => panic!("g:ld_d is not a Dict"),
        }
    }

    #[test]
    fn ex_lockvar_locks_value() {
        ex_let(&mut let_eap("g:ld_lk = 5"));
        let mut eap = exarg_T {
            arg: "g:ld_lk".to_string(),
            cmdidx: cmdidx_T::CMD_lockvar,
            ..Default::default()
        };
        ex_lockvar(&mut eap);
        assert_eq!(
            find_var("g:ld_lk", true).unwrap().v_lock,
            VarLockStatus::VAR_LOCKED
        );

        // ":unlockvar g:x" clears it again.
        let mut eap2 = exarg_T {
            arg: "g:ld_lk".to_string(),
            cmdidx: cmdidx_T::CMD_unlockvar,
            ..Default::default()
        };
        ex_lockvar(&mut eap2);
        assert_eq!(
            find_var("g:ld_lk", true).unwrap().v_lock,
            VarLockStatus::VAR_UNLOCKED
        );
    }

    #[test]
    fn heredoc_get_in_string() {
        // ":let g:hd =<< END\nfoo\nbar\nEND" — heredoc body from the string.
        ex_let(&mut let_eap("g:ld_hd =<< END\nfoo\nbar\nEND"));
        let l = eval_variable("g:ld_hd").unwrap();
        match l.vval {
            v_list(Some(list)) => {
                let items: Vec<String> = list
                    .borrow()
                    .lv_items
                    .iter()
                    .map(|it| tv_get_string(&it.li_tv))
                    .collect();
                assert_eq!(items, vec!["foo".to_string(), "bar".to_string()]);
            }
            _ => panic!("g:ld_hd is not a List"),
        }
    }

    #[test]
    fn heredoc_get_via_getline() {
        // No newline in cmd -> body comes from eap.ea_getline.
        let mut lines = vec!["one".to_string(), "two".to_string(), "EOF".to_string()].into_iter();
        let mut eap = exarg_T {
            arg: String::new(),
            cmdidx: cmdidx_T::CMD_let,
            ea_getline: Some(Box::new(move || lines.next())),
            ..Default::default()
        };
        let l = heredoc_get(&mut eap, "EOF", false).unwrap();
        let items: Vec<String> = l
            .borrow()
            .lv_items
            .iter()
            .map(|it| tv_get_string(&it.li_tv))
            .collect();
        assert_eq!(items, vec!["one".to_string(), "two".to_string()]);
    }

    #[test]
    fn ex_let_option_sets_and_compounds() {
        install_eval_hook();
        use crate::ported::option_optval::{find_option, get_option_value, OptValData};
        // ":let &tabstop = 4" → the option store holds 4.
        ex_let(&mut let_eap("&tabstop = 4"));
        let ts = find_option("tabstop");
        assert_eq!(get_option_value(ts, 0).data, OptValData::number(4));
        // ":let &tabstop += 3" → 7 (compound arithmetic ported from c:1404).
        ex_let(&mut let_eap("&tabstop += 3"));
        assert_eq!(get_option_value(ts, 0).data, OptValData::number(7));
        // ":let &filetype = 'rust'" then ".=" concat.
        ex_let(&mut let_eap("&filetype = 'rust'"));
        let ft = find_option("filetype");
        assert_eq!(
            get_option_value(ft, 0).data,
            OptValData::string("rust".to_string())
        );
        ex_let(&mut let_eap("&filetype .= 'y'"));
        assert_eq!(
            get_option_value(ft, 0).data,
            OptValData::string("rusty".to_string())
        );
    }

    #[test]
    fn ex_let_register_writes_and_appends() {
        install_eval_hook();
        // ":let @a = 'hello'" writes the register.
        ex_let(&mut let_eap("@a = 'hello'"));
        assert_eq!(
            crate::ported::ops::get_reg_contents('a'),
            Some(vec!["hello".to_string()])
        );
        // ":let @a .= 'X'" appends to the current contents.
        ex_let(&mut let_eap("@a .= 'X'"));
        assert_eq!(
            crate::ported::ops::get_reg_contents('a')
                .unwrap()
                .join("\n"),
            "helloX"
        );
    }

    #[test]
    fn setwinvar_getwinvar_roundtrip() {
        use crate::ported::option_optval::{find_option, get_option_value, OptValData};
        use crate::ported::window::{
            curtab, curwin, first_tabpage, firstwin, lastwin, tabpage_T, win_T,
        };
        use std::cell::RefCell;
        use std::rc::Rc;
        // A single window that is curwin/curtab so the switch_win gate passes.
        let w = Rc::new(RefCell::new(win_T {
            handle: 1000,
            ..Default::default()
        }));
        let tab = Rc::new(RefCell::new(tabpage_T {
            handle: 1,
            tp_firstwin: Some(w.clone()),
            tp_curwin: Some(w.clone()),
            ..Default::default()
        }));
        firstwin.with(|c| *c.borrow_mut() = Some(w.clone()));
        lastwin.with(|c| *c.borrow_mut() = Some(w.clone()));
        curwin.with(|c| *c.borrow_mut() = Some(w.clone()));
        first_tabpage.with(|c| *c.borrow_mut() = Some(tab.clone()));
        curtab.with(|c| *c.borrow_mut() = Some(tab.clone()));

        // setwinvar(win 0 = current, 'wv_x', 42) → getwinvar reads it back.
        let set_args = [
            typval_T::from(0 as varnumber_T),
            typval_T::from("wv_x".to_string()),
            typval_T::from(42 as varnumber_T),
        ];
        setwinvar(&set_args, 0);
        let get_args = [
            typval_T::from(0 as varnumber_T),
            typval_T::from("wv_x".to_string()),
            typval_T::from("def".to_string()),
        ];
        let mut rettv = typval_T::default();
        getwinvar(&get_args, &mut rettv, 0);
        assert_eq!(tv_get_string(&rettv), "42");

        // A missing window variable falls back to the default argument.
        let miss_args = [
            typval_T::from(0 as varnumber_T),
            typval_T::from("wv_absent".to_string()),
            typval_T::from("fallback".to_string()),
        ];
        let mut rettv2 = typval_T::default();
        getwinvar(&miss_args, &mut rettv2, 0);
        assert_eq!(tv_get_string(&rettv2), "fallback");

        // A '&opt' write routes to the option_optval store (c:3332).
        let opt_args = [
            typval_T::from(0 as varnumber_T),
            typval_T::from("&shiftwidth".to_string()),
            typval_T::from(3 as varnumber_T),
        ];
        setwinvar(&opt_args, 0);
        let sw = find_option("shiftwidth");
        assert_eq!(get_option_value(sw, 0).data, OptValData::number(3));
    }
}

#[cfg(test)]
mod find_var_ht_dict_tests {
    use super::*;
    use crate::ported::eval::typval::tv_get_string;

    fn scope_of(name: &str) -> Option<VarScopeDict> {
        find_var_ht_dict(name, name.len()).map(|(s, _)| s)
    }

    #[test]
    fn resolves_explicit_scope_prefixes_and_strips_name() {
        // Each explicit prefix resolves to its scope; varname is stripped.
        for (name, want) in [
            ("g:foo", VarScopeDict::Global),
            ("s:foo", VarScopeDict::Script),
            ("b:foo", VarScopeDict::Buffer),
            ("w:foo", VarScopeDict::Window),
            ("t:foo", VarScopeDict::Tabpage),
            ("v:count", VarScopeDict::VimVar),
        ] {
            let (scope, varname) = find_var_ht_dict(name, name.len()).unwrap();
            assert!(std::mem::discriminant(&scope) == std::mem::discriminant(&want));
            assert_eq!(varname, &name[2..]);
        }
    }

    #[test]
    fn implicit_scope_and_compat_version() {
        // A bare name at script level (no active function) → Global.
        funccal_stack.with(|s| s.borrow_mut().clear());
        assert!(matches!(scope_of("bare"), Some(VarScopeDict::Global)));
        // The compat "version" name resolves to v: in every scope.
        let (scope, varname) = find_var_ht_dict("version", 7).unwrap();
        assert!(matches!(scope, VarScopeDict::VimVar));
        assert_eq!(varname, "version");
        // A bare name inside a function → FuncLocal.
        funccal_stack.with(|s| s.borrow_mut().push(FuncScope::default()));
        assert!(matches!(scope_of("bare"), Some(VarScopeDict::FuncLocal)));
        funccal_stack.with(|s| s.borrow_mut().clear());
    }

    #[test]
    fn invalid_names_and_scopeless_func_vars_return_none() {
        // name_len 0, a leading ':' or '#', an unknown scope char, and a ':'/'#'
        // in the tail of a non-g: name are all invalid.
        assert!(scope_of("").is_none());
        assert!(scope_of(":x").is_none());
        assert!(scope_of("#x").is_none());
        assert!(scope_of("x:foo").is_none());
        assert!(scope_of("b:a:b").is_none());
        assert!(scope_of("b:a#b").is_none());
        // g: may contain ':' / '#' in the tail (autoload etc.).
        assert!(matches!(scope_of("g:a#b"), Some(VarScopeDict::Global)));
        // l:/a: are invalid outside a function (C returns NULL there).
        funccal_stack.with(|s| s.borrow_mut().clear());
        assert!(scope_of("l:foo").is_none());
        assert!(scope_of("a:1").is_none());
        // …and valid inside one.
        funccal_stack.with(|s| s.borrow_mut().push(FuncScope::default()));
        assert!(matches!(scope_of("l:foo"), Some(VarScopeDict::FuncLocal)));
        assert!(matches!(scope_of("a:1"), Some(VarScopeDict::FuncArgs)));
        funccal_stack.with(|s| s.borrow_mut().clear());
    }

    #[test]
    fn set_var_find_var_roundtrip_through_one_resolver() {
        // set_var (writer) and find_var (reader) both delegate to the resolver;
        // a write is visible through the read for each non-func scope.
        for (name, key) in [
            ("g:rt_probe", "g:rt_probe"),
            ("s:rt_probe", "s:rt_probe"),
            ("b:rt_probe", "b:rt_probe"),
            ("w:rt_probe", "w:rt_probe"),
            ("t:rt_probe", "t:rt_probe"),
        ] {
            set_var(name, name.len(), typval_T::from("v".to_string()), false);
            assert_eq!(tv_get_string(&find_var(key, false).unwrap()), "v");
            assert_eq!(tv_get_string(&eval_variable(key).unwrap()), "v");
        }
    }

    #[test]
    fn func_local_scope_roundtrip_and_empty_key_snapshot() {
        use crate::ported::eval::typval::tv_dict_add_nr;
        funccal_stack.with(|s| s.borrow_mut().clear());
        funccal_stack.with(|s| {
            let mut frame = FuncScope::default();
            tv_dict_add_nr(&mut frame.fc_l_avars, "1", 7);
            s.borrow_mut().push(frame);
        });
        // Bare write lands in l:, read back through the resolver.
        set_var("loc", 3, typval_T::from(5 as varnumber_T), false);
        assert_eq!(tv_get_string(&find_var("l:loc", false).unwrap()), "5");
        // a: argument is readable, and the empty-key "a:" ref is a scope snapshot.
        assert_eq!(tv_get_string(&find_var("a:1", false).unwrap()), "7");
        let snap = find_var("a:", false).unwrap();
        match snap.vval {
            v_dict(Some(d)) => assert!(d.borrow().dv_hashtab.contains_key("1")),
            _ => panic!("a: is not a Dict snapshot"),
        }
        funccal_stack.with(|s| s.borrow_mut().clear());
    }

    #[test]
    fn set_cmdarg_save_and_restore() {
        // Seed a known v:cmdarg, then set_cmdarg(Some(eap), _) returns the old
        // value and (with no modelled modifiers) writes the empty base case.
        set_vim_var_string(vv::VV_CMDARG, "prev");
        let eap = exarg_T::default();
        let old = set_cmdarg(Some(&eap), None);
        assert_eq!(old.as_deref(), Some("prev"));
        assert_eq!(get_vim_var_str(vv::VV_CMDARG), "");
        // Restore path: eap == None restores oldarg and returns None.
        let ret = set_cmdarg(None, old);
        assert!(ret.is_none());
        assert_eq!(get_vim_var_str(vv::VV_CMDARG), "prev");
    }
}
