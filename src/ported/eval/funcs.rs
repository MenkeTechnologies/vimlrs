//! Port of `src/nvim/eval/funcs.c` (vendored at `vendor/eval/funcs.c`).
//!
//! Vimscript builtin functions. Each `f_<name>` matches the C signature
//! `void f_<name>(typval_T *argvars, typval_T *rettv, EvalFuncData fptr)`,
//! reduced to `(argvars, rettv)` (the `fptr` carries no data for these). As in
//! C, the caller (`call_func`) pre-initializes `rettv` to `VAR_NUMBER`/0 before
//! the call, so a numeric function only assigns `rettv->vval.v_number`; only
//! functions returning another type set `v_type`. Phase 3 ports a subset.
#![allow(non_snake_case)]

use crate::ported::buffer::{buf_T, buflist_findnr, buflist_findpat, curbuf, lastbuf};
use crate::ported::eval::buffer::find_buffer;
use crate::ported::eval::encode::{encode_tv2echo, encode_tv2string};
use crate::ported::eval::list::FILTER_MAP_EVAL_HOOK;
use crate::ported::eval::typval::tv_equal;
use crate::ported::eval::typval::{
    callback_from_typval, tv_blob_get, tv_check_for_number_arg, tv_check_for_string_arg,
    tv_check_str_or_nr, tv_dict_watcher_add, tv_dict_watcher_remove, tv_get_number,
    tv_get_string_buf, tv_get_string_buf_chk, tv_get_string_chk, Callback, CALL_FUNC_HOOK,
};
use crate::ported::eval::typval::{
    tv_blob_alloc_ret, tv_blob_len, tv_dict_add_tv, tv_dict_find, tv_dict_len, tv_get_bool,
    tv_get_float, tv_get_number_chk, tv_get_string, tv_list_alloc_ret, tv_list_append_number,
    tv_list_append_string, tv_list_append_tv, tv_list_copy, tv_list_extend, tv_list_find_nr,
    tv_list_flatten, tv_list_len, tv_list_ref,
};
use crate::ported::eval::typval::{
    tv_dict_add_list, tv_dict_add_nr, tv_dict_add_str, tv_dict_alloc, tv_dict_alloc_ret,
    tv_list_alloc, tv_list_append_list,
};
use crate::ported::eval::typval_defs_h::{
    blob_T, list_T, typval_T, typval_vval_union::*, varnumber_T, BoolVarValue::*,
    SpecialVarValue::*, VarType::*, VAR_TYPE_BLOB, VAR_TYPE_BOOL, VAR_TYPE_DICT, VAR_TYPE_FLOAT,
    VAR_TYPE_FUNC, VAR_TYPE_LIST, VAR_TYPE_NUMBER, VAR_TYPE_SPECIAL, VAR_TYPE_STRING,
};
use crate::ported::eval::vars::{
    assert_error, get_vim_var_str, set_vim_var_nr,
    vv::{VV_EXCEPTION, VV_REG, VV_SHELL_ERROR},
};
use crate::ported::eval_h::{FAIL, OK};
use crate::ported::ex_eval::emsg_silent;
use crate::ported::grid::{ui_comp_get_grid_at_coord, ScreenGrid};
use crate::ported::message::emsg;
use crate::ported::ops::{
    format_reg_type, get_reg_contents, get_reg_type, get_yank_type, write_reg_contents,
    write_reg_contents_lst, MotionType,
};
use crate::ported::option::get_option_value;
use crate::ported::os::env::os_get_pid;
use crate::ported::os::time::{os_hrtime, os_localtime_r, os_strptime};
use crate::ported::profile::{
    profile_end, profile_msg, profile_signed, profile_start, profile_sub, proftime_T,
};
use crate::ported::sha256::sha256_bytes;
use crate::viml_regex::regex_match;

/// Port of `f_len()` from `Src/eval/funcs.c`.
///
/// "len()" function — length of a String/List/Dict/Blob (or the decimal width
/// of a Number). `rettv` is pre-set to `VAR_NUMBER`.
pub fn f_len(argvars: &[typval_T], rettv: &mut typval_T) {
    let arg = &argvars[0];
    // c: switch (argvars[0].v_type) { ... rettv->vval.v_number = ...; }
    rettv.vval = match (arg.v_type, &arg.vval) {
        // c: VAR_STRING/VAR_NUMBER → strlen(tv_get_string(...)) — byte length.
        (VAR_STRING, v_string(s)) => v_number(s.len() as varnumber_T),
        // c: only VAR_NUMBER shares the VAR_STRING branch — VAR_FLOAT is listed
        // with the *error* cases, so `len(0.0)` is E701, not the width of the
        // float's rendering.
        (VAR_NUMBER, _) => v_number(tv_get_string(arg).len() as varnumber_T),
        (VAR_LIST, v_list(Some(l))) => v_number(tv_list_len(&l.borrow()) as varnumber_T),
        (VAR_DICT, v_dict(Some(d))) => v_number(tv_dict_len(&d.borrow()) as varnumber_T),
        (VAR_BLOB, v_blob(Some(b))) => v_number(tv_blob_len(&b.borrow()) as varnumber_T),
        (VAR_LIST, _) | (VAR_DICT, _) | (VAR_BLOB, _) => v_number(0),
        _ => {
            // c: emsg(_("E701: Invalid type for len()"));
            emsg("E701: Invalid type for len()");
            v_number(0)
        }
    };
}

/// Port of `f_type()` from `Src/eval/funcs.c`.
///
/// "type(expr)" function — the `VAR_TYPE_*` code of `expr`.
pub fn f_type(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: switch (argvars[0].v_type) { case VAR_NUMBER: n = VAR_TYPE_NUMBER; ... }
    let n = match argvars[0].v_type {
        VAR_NUMBER => VAR_TYPE_NUMBER,
        VAR_STRING => VAR_TYPE_STRING,
        VAR_PARTIAL | VAR_FUNC => VAR_TYPE_FUNC,
        VAR_LIST => VAR_TYPE_LIST,
        VAR_DICT => VAR_TYPE_DICT,
        VAR_FLOAT => VAR_TYPE_FLOAT,
        VAR_BOOL => VAR_TYPE_BOOL,
        VAR_SPECIAL => VAR_TYPE_SPECIAL,
        VAR_BLOB => VAR_TYPE_BLOB,
        VAR_UNKNOWN => {
            emsg("E685: Internal error: f_type(UNKNOWN)");
            -1
        }
    };
    rettv.vval = v_number(n);
}

/// Port of `f_empty()` from `Src/eval/funcs.c`.
///
/// "empty(expr)" function — whether `expr` is empty.
pub fn f_empty(argvars: &[typval_T], rettv: &mut typval_T) {
    let arg = &argvars[0];
    // c: switch (argvars[0].v_type) { ... n = …; } rettv->vval.v_number = n;
    let n = match (arg.v_type, &arg.vval) {
        (VAR_STRING, v_string(s)) => s.is_empty(),
        (VAR_NUMBER, v_number(x)) => *x == 0,
        (VAR_FLOAT, v_float(f)) => *f == 0.0,
        (VAR_LIST, v_list(l)) => l.as_ref().map_or(true, |l| l.borrow().lv_len == 0),
        (VAR_DICT, v_dict(d)) => d
            .as_ref()
            .map_or(true, |d| d.borrow().dv_hashtab.is_empty()),
        (VAR_BLOB, v_blob(b)) => b.as_ref().map_or(true, |b| b.borrow().bv_ga.is_empty()),
        (VAR_BOOL, v_bool(b)) => *b == kBoolVarFalse,
        (VAR_SPECIAL, _) => true,
        (VAR_FUNC | VAR_PARTIAL, _) => false,
        _ => true,
    };
    rettv.vval = v_number(n as varnumber_T);
}

/// Port of `f_abs()` from `Src/eval/funcs.c`.
///
/// "abs(expr)" function — absolute value (Float in, Float out; else Number).
pub fn f_abs(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: if (argvars[0].v_type == VAR_FLOAT) { rettv->v_type = VAR_FLOAT; … }
    if argvars[0].v_type == VAR_FLOAT {
        rettv.v_type = VAR_FLOAT;
        rettv.vval = v_float(tv_get_float(&argvars[0]).abs());
    } else {
        rettv.vval = v_number(tv_get_number_chk(&argvars[0], None).abs());
    }
}

/// Port of `f_str2float()` from `Src/eval/funcs.c`.
///
/// "str2float()" function — parse a float from a string.
pub fn f_str2float(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: p = skipwhite(...); strip a leading sign before string2float (which
    // parses the magnitude, strtod-style, ignoring trailing garbage).
    let s = tv_get_string(&argvars[0]);
    let p = s.trim_start();
    let (isneg, p) = match p.strip_prefix(['+', '-']) {
        Some(rest) if p.starts_with('-') => (true, rest.trim_start()),
        Some(rest) => (false, rest.trim_start()),
        None => (false, p),
    };
    let (mut val, _) = crate::ported::eval::string2float(p);
    if isneg {
        val *= -1.0;
    }
    rettv.v_type = VAR_FLOAT;
    rettv.vval = v_float(val);
}

/// Port of `f_float2nr()` from `Src/eval/funcs.c`.
///
/// "float2nr()" function — truncate a Float to a Number.
pub fn f_float2nr(argvars: &[typval_T], rettv: &mut typval_T) {
    let f = tv_get_float(&argvars[0]);
    // c: clamp to ±VARNUMBER_MAX (not i64::MIN) using DBL_EPSILON slack.
    let n = if f <= -(varnumber_T::MAX as f64) + f64::EPSILON {
        -varnumber_T::MAX
    } else if f >= varnumber_T::MAX as f64 - f64::EPSILON {
        varnumber_T::MAX
    } else {
        f as varnumber_T
    };
    rettv.vval = v_number(n);
}

/// Port of `f_function()` from `Src/eval/funcs.c` — a Funcref/Partial for the
/// named function.
///
/// c: `common_function(argvars, rettv, false)`. It used to build the Funcref
/// directly and skip every check `common_function` does, so `function('nosuchfn')`
/// happily produced a reference to a function that does not exist instead of
/// raising E700.
pub fn f_function(argvars: &[typval_T], rettv: &mut typval_T) {
    common_function(argvars, rettv, false);
}

/// Port of `f_char2nr()` from `Src/eval/funcs.c` — code point of the first char.
pub fn f_char2nr(argvars: &[typval_T], rettv: &mut typval_T) {
    let n = tv_get_string(&argvars[0])
        .chars()
        .next()
        .map_or(0, |c| c as varnumber_T);
    rettv.vval = v_number(n);
}

/// Port of `f_nr2char()` from `Src/eval/funcs.c` — char for a code point.
pub fn f_nr2char(argvars: &[typval_T], rettv: &mut typval_T) {
    let n = tv_get_number_chk(&argvars[0], None);
    // c: `if (num < 0) { emsg(E5070); return; } if (num > INT_MAX) { semsg(E5071); return; }`
    if n < 0 {
        emsg("E5070: Character number must not be less than zero");
        return;
    }
    if n > i32::MAX as varnumber_T {
        emsg(&format!(
            "E5071: Character number must not be greater than INT_MAX ({})",
            i32::MAX
        ));
        return;
    }
    // c: utf_char2bytes() into a buffer, then xmemdupz() — the result is a
    // C string, so it TERMINATES at the first NUL: nr2char(0) is ''.
    let mut buf = [0u8; 6];
    let len = crate::ported::mbyte::utf_char2bytes(n as i32, &mut buf) as usize;
    let bytes = &buf[..len];
    let bytes = &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(len)];
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(String::from_utf8_lossy(bytes).into_owned());
}

/// Port of `repeat_list()` — `vendor/eval/funcs.c:5310`. Repeat list `l` `n` times
/// into `rettv` (a new list).
fn repeat_list(l: &std::rc::Rc<std::cell::RefCell<list_T>>, n: varnumber_T, rettv: &mut typval_T) {
    // c: tv_list_alloc_ret(rettv, (n > 0) * n * tv_list_len(l));
    let len = (n > 0) as varnumber_T * n * tv_list_len(&l.borrow()) as varnumber_T;
    let out = tv_list_alloc_ret(rettv, len as isize);
    // c: while (n-- > 0) tv_list_extend(rettv->vval.v_list, l, NULL);
    // `out` is a freshly allocated list, distinct from `l`, so borrowing both is
    // safe.
    let src = l.borrow();
    let mut count = n;
    while count > 0 {
        tv_list_extend(&mut out.borrow_mut(), &src, None);
        count -= 1;
    }
}

/// Port of `repeat_blob()` — `vendor/eval/funcs.c:5319`. Repeat blob `b` `n` times
/// into `rettv` (a new blob).
fn repeat_blob(
    blob: Option<&std::rc::Rc<std::cell::RefCell<blob_T>>>,
    n: varnumber_T,
    rettv: &mut typval_T,
) {
    // c: blob_T *const blob = blob_tv->vval.v_blob; tv_blob_alloc_ret(rettv);
    let out = tv_blob_alloc_ret(rettv);
    // c: if (blob == NULL || n <= 0) return;
    let Some(blob) = blob else { return };
    if n <= 0 {
        return;
    }
    let src = blob.borrow().bv_ga.clone();
    // c: const int slen = blob->bv_ga.ga_len; const int len = (int)(slen * n);
    let slen = src.len();
    let len = slen * n as usize;
    // c: if (len <= 0) return;
    if len == 0 {
        return;
    }
    // c: ga_grow(&...bv_ga, len); ...bv_ga.ga_len = len; — ga_grow zero-fills the
    // grown space, which `vec![0; len]` reproduces.
    let mut data = vec![0u8; len];
    // c: for (i=0;i<slen;i++) if (tv_blob_get(blob,i)!=0) break; if (i==slen) return;
    // — all source bytes 0, so the already zero-filled destination is correct.
    if src.iter().all(|&b| b == 0) {
        out.borrow_mut().bv_ga = data;
        return;
    }
    // c: for (i=0;i<n;i++) tv_blob_set_range(rettv->vval.v_blob, i*slen,
    //    (i+1)*slen-1, blob_tv);
    for i in 0..n as usize {
        data[i * slen..(i + 1) * slen].copy_from_slice(&src);
    }
    out.borrow_mut().bv_ga = data;
}

/// Port of `repeat_string()` — `vendor/eval/funcs.c:5356`. Repeat string `str_tv`
/// `n` times into `rettv` (a new string).
fn repeat_string(str_tv: &typval_T, n: varnumber_T, rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    // c: if (n <= 0) { rettv->vval.v_string = NULL; return; }
    if n <= 0 {
        rettv.vval = v_string(String::new());
        return;
    }
    // c: const char *const p = tv_get_string(str_tv); const size_t slen = strlen(p);
    let p = tv_get_string(str_tv);
    let bytes = p.as_bytes();
    let slen = bytes.len();
    // c: if (slen == 0) return;  (NULL → empty here)
    if slen == 0 {
        rettv.vval = v_string(String::new());
        return;
    }
    // c: const size_t len = slen * n; if (len / n != slen) return;  (overflow)
    let Some(len) = slen.checked_mul(n as usize) else {
        rettv.vval = v_string(String::new());
        return;
    };
    // c: char *r = xmallocz(len); memmove(r, p, slen);  then the doubling copy:
    // c: while (done < len) { copy_len = min(done, len-done); memmove(r+done, r,
    //    copy_len); done += copy_len; }
    let mut r = vec![0u8; len];
    r[..slen].copy_from_slice(bytes);
    let mut done = slen;
    while done < len {
        let copy_len = done.min(len - done);
        r.copy_within(0..copy_len, done);
        done += copy_len;
    }
    // The source is a valid VAR_STRING (UTF-8 here), so the repeat is valid too.
    rettv.vval = v_string(String::from_utf8_lossy(&r).into_owned());
}

/// Port of `f_repeat()` — `vendor/eval/funcs.c:5393`. Repeat a List, Blob, or
/// String `{count}` times.
pub fn f_repeat(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: varnumber_T n = tv_get_number(&argvars[1]);
    let n = tv_get_number(&argvars[1]);
    match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => repeat_list(l, n, rettv),
        (VAR_LIST, _) => {
            tv_list_alloc_ret(rettv, 0);
        }
        (VAR_BLOB, v_blob(b)) => repeat_blob(b.as_ref(), n, rettv),
        _ => repeat_string(&argvars[0], n, rettv),
    }
}

/// Port of `f_split()` from `Src/eval/funcs.c`.
///
/// "split({str} [, {pat} [, {keepempty}]])" — split on the Vim regex `{pat}`,
/// dropping empty pieces unless `{keepempty}`. c (funcs.c): a missing or empty
/// `{pat}` defaults to `"[\\x01- ]\\+"` — a run of ANY byte from 0x01 through
/// space, not just `\s` whitespace — so `split("\x01")` is `[]`, exactly like
/// `split(" ")`. RUST-PORT NOTE: the collection's `\x01` escape is written as
/// the literal codepoint here because the pattern engine decodes collection
/// escapes at a different layer than the C's `coll_get_char()`.
pub fn f_split(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let keepempty = argvars
        .get(2)
        .is_some_and(|t| tv_get_number_chk(t, None) != 0);
    let pat = argvars.get(1).map(tv_get_string).filter(|p| !p.is_empty());
    let parts: Vec<String> = crate::viml_regex::regex_split(
        &s,
        pat.as_deref().unwrap_or("[\u{01}- ]\\+"),
        tv_get_bool(&get_option_value("ignorecase")) != 0,
        keepempty,
    );
    let l = tv_list_alloc_ret(rettv, parts.len() as isize);
    let mut lb = l.borrow_mut();
    for p in &parts {
        tv_list_append_string(&mut lb, p);
    }
}

/// Port of `f_matchstr()` from `Src/eval/funcs.c` — the matched substring of the
/// Vim regex `{pat}` in `{expr}`, or "".
pub fn f_matchstr(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: kSomeMatchStr — List → the matching item (whole); String → the matched
    // substring; else "".
    match find_some_match(argvars) {
        Some(m) if m.list_idx.is_some() => *rettv = m.item,
        Some(m) => {
            rettv.v_type = VAR_STRING;
            rettv.vval = v_string(m.groups.into_iter().next().unwrap_or_default());
        }
        None => {
            rettv.v_type = VAR_STRING;
            rettv.vval = v_string(String::new());
        }
    }
}

/// Port of `get_list_line()` from `Src/eval/funcs.c:1264` — the getline callback
/// feeding a List of lines (for `execute([list])`) to the source machinery.
/// RUST-PORT NOTE: the bridge sources such input directly, so this callback is
/// never driven → `None` (end of input).
pub fn get_list_line() -> Option<String> {
    None
}

/// Port of `find_internal_func_lua()` from `Src/eval/funcs.c:244`.
///
/// The Lua name of a Lua-implemented builtin, or `None` if not found. RUST-PORT
/// NOTE: the standalone has no Lua-implemented builtins, so this is always
/// `None` (mirrors the absent Lua provider elsewhere).
pub fn find_internal_func_lua(_name: &str) -> Option<String> {
    None
}

/// Reduced `EvalFuncDef` (`Src/eval/funcs.c`) — a builtin's metadata. RUST-PORT
/// NOTE: the C also carries a `func` dispatch pointer and Lua data; dispatch
/// here is by name through `CALL_FUNC_HOOK`, so only the arity/method-base
/// metadata is modeled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvalFuncDef {
    /// Builtin name.
    pub name: &'static str,
    /// Minimum argument count.
    pub min_argc: u8,
    /// Maximum argument count (`MAX` = varargs).
    pub max_argc: u8,
    /// Method-base argument index (`0` = `BASE_NONE`).
    pub base_arg: u8,
}

/// Port of `find_internal_func()` from `Src/eval/funcs.c:235` — look up the
/// (reduced) [`EvalFuncDef`] for builtin `name`, or `None` if not a builtin.
pub fn find_internal_func(name: &str) -> Option<EvalFuncDef> {
    use crate::ported::eval::funcs_argc::{BUILTIN_ARGC, BUILTIN_BASE};
    let i = BUILTIN_ARGC.binary_search_by(|e| e.0.cmp(name)).ok()?;
    let (n, min_argc, max_argc) = BUILTIN_ARGC[i];
    let base_arg = BUILTIN_BASE
        .binary_search_by(|e| e.0.cmp(name))
        .map_or(0, |j| BUILTIN_BASE[j].1);
    Some(EvalFuncDef {
        name: n,
        min_argc,
        max_argc,
        base_arg,
    })
}

/// Port of `check_internal_func()` from `Src/eval/funcs.c:257`.
///
/// Check the argument count for builtin `name` and return its method-base index
/// (`BASE_NONE` = 0, else the 1-based base position), or `-1` (after an `E118`/
/// `E119` error) on an arity mismatch.
pub fn check_internal_func(name: &str, argcount: i32) -> i32 {
    use crate::ported::eval::funcs_argc::{BUILTIN_ARGC, BUILTIN_BASE, MAX};
    let Ok(i) = BUILTIN_ARGC.binary_search_by(|e| e.0.cmp(name)) else {
        return -1;
    };
    let (_, min_argc, max_argc) = BUILTIN_ARGC[i];
    if argcount < min_argc as i32 {
        emsg(&format!("E119: Not enough arguments for function: {name}"));
        return -1;
    }
    if max_argc != MAX && argcount > max_argc as i32 {
        emsg(&format!("E118: Too many arguments for function: {name}"));
        return -1;
    }
    BUILTIN_BASE
        .binary_search_by(|e| e.0.cmp(name))
        .map_or(0, |j| BUILTIN_BASE[j].1 as i32)
}

/// Port of `call_internal_method()` from `Src/eval/funcs.c:297`.
///
/// Invoke builtin `fname` as a method `base->fname(argvars)`: insert `basetv` at
/// the builtin's method-base position, validate arity, and dispatch. Returns an
/// `FCERR_*` code (`FCERR_NONE` on success).
pub fn call_internal_method(
    fname: &str,
    argvars: &[typval_T],
    basetv: &typval_T,
    rettv: &mut typval_T,
) -> i32 {
    use crate::ported::eval::funcs_argc::{BUILTIN_ARGC, BUILTIN_BASE, MAX};
    use crate::ported::eval::typval_defs_h::{VarLockStatus, VarType::VAR_FUNC};
    use crate::ported::eval::userfunc::fcerr::*;
    let Ok(i) = BUILTIN_ARGC.binary_search_by(|e| e.0.cmp(fname)) else {
        return FCERR_UNKNOWN;
    };
    let (_, min_argc, max_argc) = BUILTIN_ARGC[i];
    let base = BUILTIN_BASE
        .binary_search_by(|e| e.0.cmp(fname))
        .map_or(0u8, |j| BUILTIN_BASE[j].1);
    if base == 0 {
        return FCERR_NOTMETHOD;
    }
    let argcount = argvars.len() as i32;
    if argcount + 1 < min_argc as i32 {
        return FCERR_TOOFEW;
    }
    if max_argc != MAX && argcount + 1 > max_argc as i32 {
        return FCERR_TOOMANY;
    }
    let base_index = (base - 1) as usize;
    if argvars.len() < base_index {
        return FCERR_TOOFEW;
    }
    // Insert `basetv` at the method-base position.
    let mut argv: Vec<typval_T> = argvars[..base_index].to_vec();
    argv.push(basetv.clone());
    argv.extend_from_slice(&argvars[base_index..]);
    let callee = typval_T {
        v_type: VAR_FUNC,
        v_lock: VarLockStatus::VAR_UNLOCKED,
        vval: v_string(fname.to_string()),
    };
    match CALL_FUNC_HOOK
        .with(|h| *h.borrow())
        .and_then(|f| f(&callee, &argv))
    {
        Some(result) => {
            *rettv = result;
            FCERR_NONE
        }
        None => FCERR_UNKNOWN,
    }
}

/// Port of `call_internal_func()` from `Src/eval/funcs.c:279`.
///
/// Look up builtin `fname`, validate the argument count against its
/// `BUILTIN_ARGC` row, and dispatch it, returning an `FCERR_*` code
/// (`FCERR_UNKNOWN`/`FCERR_TOOFEW`/`FCERR_TOOMANY`, else `FCERR_NONE`).
/// RUST-PORT NOTE: the C `find_internal_func` returns an `EvalFuncDef` whose
/// `func` pointer it calls; this routes the call through the bridge's
/// `CALL_FUNC_HOOK` (which dispatches builtins by name).
pub fn call_internal_func(fname: &str, argvars: &[typval_T], rettv: &mut typval_T) -> i32 {
    use crate::ported::eval::funcs_argc::{BUILTIN_ARGC, MAX};
    use crate::ported::eval::typval_defs_h::{VarLockStatus, VarType::VAR_FUNC};
    use crate::ported::eval::userfunc::fcerr::*;
    let Ok(i) = BUILTIN_ARGC.binary_search_by(|e| e.0.cmp(fname)) else {
        return FCERR_UNKNOWN;
    };
    let (_, min_argc, max_argc) = BUILTIN_ARGC[i];
    let argcount = argvars.len();
    if argcount < min_argc as usize {
        return FCERR_TOOFEW;
    }
    if max_argc != MAX && argcount > max_argc as usize {
        return FCERR_TOOMANY;
    }
    let callee = typval_T {
        v_type: VAR_FUNC,
        v_lock: VarLockStatus::VAR_UNLOCKED,
        vval: v_string(fname.to_string()),
    };
    match CALL_FUNC_HOOK
        .with(|h| *h.borrow())
        .and_then(|f| f(&callee, argvars))
    {
        Some(result) => {
            *rettv = result;
            FCERR_NONE
        }
        None => FCERR_UNKNOWN,
    }
}

/// Port of `return_register()` from `Src/eval/funcs.c:5193`.
///
/// Set `rettv` to the single-character register name `regname` (an empty string
/// when `regname` is NUL). Backs `reg_executing()`/`reg_recording()`.
pub fn return_register(regname: u8, rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(if regname == 0 {
        String::new()
    } else {
        (regname as char).to_string()
    });
}

/// Port of `may_add_state_char()` from `Src/eval/funcs.c:4589`.
///
/// Append state character `c` to `gap` when `include` is `None` (collect all)
/// or names `c` — the per-flag filter behind `state()`.
pub fn may_add_state_char(gap: &mut String, include: Option<&str>, c: char) {
    if include.map_or(true, |inc| inc.contains(c)) {
        gap.push(c);
    }
}

/// Port of `get_function_name()` from `Src/eval/funcs.c:178` — the `idx`-th
/// builtin/user function name for command-line completion. No interactive
/// completion standalone → `None`.
pub fn get_function_name(_idx: i32) -> Option<String> {
    None
}

/// Port of `get_expr_name()` from `Src/eval/funcs.c:214` — the `idx`-th function
/// or variable name for expression completion. No completion standalone → `None`.
pub fn get_expr_name(_idx: i32) -> Option<String> {
    None
}

/// Port of `api_wrapper()` from `Src/eval/funcs.c:360` — dispatch wrapper for a
/// generated `nvim_*` API builtin. No API builtins exist standalone, so this is
/// never a builtin's dispatch target → no-op.
pub fn api_wrapper(_argvars: &[typval_T], _rettv: &mut typval_T) {}

/// Port of `lua_wrapper()` from `Src/eval/funcs.c:397` — dispatch wrapper for a
/// Lua-implemented builtin. No Lua builtins exist standalone → no-op.
pub fn lua_wrapper(_argvars: &[typval_T], _rettv: &mut typval_T) {}

/// Port of `dummy_timer_due_cb()` from `Src/eval/funcs.c:2568` — the libuv
/// callback for `wait()`'s internal timeout timer; no event loop fires it, no-op.
pub fn dummy_timer_due_cb() {}

/// Port of `dummy_timer_close_cb()` from `Src/eval/funcs.c:2579` — the libuv
/// close callback for `wait()`'s timer; never fires, no-op.
pub fn dummy_timer_close_cb() {}

/// Port of `tv_get_buf()` from `vendor/eval/funcs.c:471`.
///
/// Get buffer by number or pattern.
///
/// RUST-PORT NOTE: the C return `buf_T *` becomes `Option<Rc<RefCell<buf_T>>>`
/// (the pointer→handle map, matching `buflist_findnr`/`find_buffer`). The
/// `p_magic`/`p_cpo` save/set/restore around `buflist_findpat` (c:489-495,
/// c:499-500) is elided: `buflist_findpat`'s regexp path is deferred to a
/// substring match, so 'magic'/'cpoptions' have no bearing on the lookup here.
pub fn tv_get_buf(
    tv: &typval_T,
    curtab_only: bool,
) -> Option<std::rc::Rc<std::cell::RefCell<buf_T>>> {
    // c:473 if (tv->v_type == VAR_NUMBER)
    if tv.v_type == VAR_NUMBER {
        if let v_number(n) = &tv.vval {
            // c:474 return buflist_findnr((int)tv->vval.v_number);
            return buflist_findnr(*n as i32);
        }
    }
    // c:476 if (tv->v_type != VAR_STRING) return NULL;
    if tv.v_type != VAR_STRING {
        return None; // c:477
    }

    // c:480 char *name = tv->vval.v_string;
    let name = match &tv.vval {
        v_string(s) => s,
        _ => return None,
    };

    // c:482 if (name == NULL || *name == NUL) return curbuf;
    if name.is_empty() {
        return curbuf.with(|c| c.borrow().clone());
    }
    // c:485 if (name[0] == '$' && name[1] == NUL) return lastbuf;
    if name == "$" {
        return lastbuf.with(|c| c.borrow().clone());
    }

    // c:497 buf = buflist_findnr(buflist_findpat(name, name + strlen(name),
    // c:498                                       true, false, curtab_only));
    let mut buf = buflist_findnr(buflist_findpat(name, true, false, curtab_only));

    // c:503 if (buf == NULL) buf = find_buffer(tv);
    if buf.is_none() {
        buf = find_buffer(tv); // c:504
    }

    buf // c:506
}

/// Port of `tv_get_buf_from_arg()` from `vendor/eval/funcs.c:510`.
///
/// Like [`tv_get_buf`] but give an error message if the type is wrong.
///
/// RUST-PORT NOTE: the `emsg_off++`/`emsg_off--` guard (c:515, c:517) is elided:
/// `emsg_off` is a private counter in `ex_eval.rs` and `emsg()` does not consult
/// it in this port, so the suppression is a no-op (and `tv_get_buf` emits no
/// errors of its own on the lookup path).
pub fn tv_get_buf_from_arg(tv: &typval_T) -> Option<std::rc::Rc<std::cell::RefCell<buf_T>>> {
    // c:512 if (!tv_check_str_or_nr(tv)) return NULL;
    if !tv_check_str_or_nr(tv) {
        return None; // c:513
    }
    // c:516 buf_T *const buf = tv_get_buf(tv, false);
    let buf = tv_get_buf(tv, false);
    buf // c:518
}

/// Port of `get_buf_arg()` from `vendor/eval/funcs.c:523`.
///
/// Get the buffer from "arg" and give an error and return NULL if it is not
/// valid.
///
/// RUST-PORT NOTE: the `emsg_off++`/`emsg_off--` guard (c:525, c:527) is elided
/// for the same reason as [`tv_get_buf_from_arg`].
pub fn get_buf_arg(arg: &typval_T) -> Option<std::rc::Rc<std::cell::RefCell<buf_T>>> {
    // c:526 buf_T *buf = tv_get_buf(arg, false);
    let buf = tv_get_buf(arg, false);
    // c:528 if (buf == NULL)
    if buf.is_none() {
        // c:529 semsg(_("E158: Invalid buffer name: %s"), tv_get_string(arg));
        crate::ported::message::semsg(&format!(
            "E158: Invalid buffer name: {}",
            tv_get_string(arg)
        ));
    }
    buf // c:531
}

// ── buffer-switching helpers (Src/eval/buffer.c) ──
//
// RUST-PORT NOTE: vimlrs has a single virtual buffer, so there is never another
// buffer to switch to or restore from — the buffer builtins operate on the one
// buffer directly. These context-switch helpers are therefore faithful no-ops.

/// Port of `switch_buffer()` from `Src/eval/buffer.c:784` — make another buffer
/// current, saving the old one. Single buffer standalone → no-op.
pub fn switch_buffer() {}

/// Port of `restore_buffer()` from `Src/eval/buffer.c:795` — restore the buffer
/// saved by [`switch_buffer`]. Single buffer standalone → no-op.
pub fn restore_buffer() {}

/// Port of `change_other_buffer_prepare()` from `Src/eval/buffer.c:92` — set up
/// to change a non-current buffer. Single buffer standalone → no-op.
pub fn change_other_buffer_prepare() {}

/// Port of `change_other_buffer_restore()` from `Src/eval/buffer.c:113` — undo
/// [`change_other_buffer_prepare`]. Single buffer standalone → no-op.
pub fn change_other_buffer_restore() {}

/// Port of `non_zero_arg()` from `Src/eval/funcs.c:328`.
///
/// True for a non-zero Number, a `v:true` Bool, or a non-empty String —
/// the "truthy first argument" test several builtins use.
pub fn non_zero_arg(argvars: &[typval_T]) -> bool {
    match (&argvars[0].v_type, &argvars[0].vval) {
        (VAR_NUMBER, v_number(n)) => *n != 0,
        (VAR_BOOL, v_bool(b)) => *b == kBoolVarTrue,
        (VAR_STRING, v_string(s)) => !s.is_empty(),
        _ => false,
    }
}

/// Port of `f_match()` from `Src/eval/funcs.c` — the char index of the first
/// match of `{pat}` in `{expr}`, or -1.
/// The selected match found by [`find_some_match`].
struct SomeMatch {
    /// `Some(item index)` when the subject is a List, else `None` (a String).
    list_idx: Option<i64>,
    /// Match start / end char index (a column within the item for a List).
    start: i64,
    end: i64,
    /// `[whole, \1..\9]` group strings (padded to 10).
    groups: Vec<String>,
    /// The matching List item (List subject only) — `matchstr()` returns it.
    item: typval_T,
}

/// Port of `find_some_match()` — `vendor/eval/funcs.c:4060`. The shared backend of
/// `match()`/`matchstr()`/`matchend()`/`matchstrpos()`/`matchlist()`.
///
/// String subject: `{start}` is a startcol when `{count}` is given (so `^`/`\<`
/// anchor to 0), else the subject is chopped at `{start}`; `{count}` selects the
/// Nth match. List subject: each item is stringified and tested in turn from
/// `{start}` (an item index), and `{count}` picks the Nth *matching item*.
///
/// RUST-PORT NOTE: the C writes `rettv` per `SomeMatchType`; this returns the
/// data and each `f_*` formats its own result. Positions are char indices
/// (== byte indices for ASCII), as elsewhere in the engine. Both the String and
/// List subject forms are handled.
fn find_some_match(argvars: &[typval_T]) -> Option<SomeMatch> {
    let pat = tv_get_string(&argvars[1]);
    let ic = tv_get_bool(&get_option_value("ignorecase")) != 0;
    // c: reported positions are BYTE offsets (`regmatch.startp[0] - expr`), while
    // the regex engine works in char indices — convert against the subject.
    let char_to_byte = |subject: &str, ci: i64| -> i64 {
        subject
            .char_indices()
            .nth(ci as usize)
            .map_or(subject.len() as i64, |(b, _)| b as i64)
    };
    let has_count = argvars.len() > 3 && argvars[3].v_type != VAR_UNKNOWN;
    let count = if has_count {
        tv_get_number(&argvars[3])
    } else {
        1
    };

    // c: List subject — stringify each item and match it; {start} is an item
    // index (tv_list_uidx), {count} picks the Nth matching item.
    if argvars[0].v_type == VAR_LIST {
        let items: Vec<typval_T> = match &argvars[0].vval {
            v_list(Some(l)) => l
                .borrow()
                .lv_items
                .iter()
                .map(|it| it.li_tv.clone())
                .collect(),
            _ => return None,
        };
        let len = items.len() as i64;
        let start = match argvars.get(2).filter(|t| t.v_type != VAR_UNKNOWN) {
            None => 0,
            Some(t) => {
                let s = tv_get_number(t);
                let s = if s < 0 { s + len } else { s };
                if s < 0 || s >= len {
                    return None;
                }
                s
            }
        };
        let mut remaining = count.max(1);
        for idx in start..len {
            let str = encode_tv2echo(&items[idx as usize]);
            if let Some((s, e, groups)) = crate::viml_regex::regex_search_nth(&pat, &str, ic, 0, 1)
            {
                remaining -= 1;
                if remaining <= 0 {
                    return Some(SomeMatch {
                        list_idx: Some(idx),
                        start: char_to_byte(&str, s),
                        end: char_to_byte(&str, e),
                        groups,
                        item: items[idx as usize].clone(),
                    });
                }
            }
        }
        return None;
    }

    // String subject.
    let s = tv_get_string(&argvars[0]);
    let nchars = s.chars().count() as i64;
    let hit = match argvars.get(2).filter(|t| t.v_type != VAR_UNKNOWN) {
        // c: no {start} — search from the head for the nth match.
        None => crate::viml_regex::regex_search_nth(&pat, &s, ic, 0, count),
        Some(t) => {
            // c: if (start < 0) start = 0; if (start > len) goto theend;
            let st = tv_get_number(t).max(0);
            if st > nchars {
                return None;
            }
            if has_count {
                // c: with {count}, {start} is a startcol — `^`/`\<` anchor to 0.
                crate::viml_regex::regex_search_nth(&pat, &s, ic, st as usize, count)
            } else {
                // c: without {count}, the subject is chopped at {start} (str +=
                // start; len -= start), so `^` matches at the chop; add {start}
                // back to the reported indices.
                let suffix: String = s.chars().skip(st as usize).collect();
                crate::viml_regex::regex_search_nth(&pat, &suffix, ic, 0, count)
                    .map(|(a, b, g)| (a + st, b + st, g))
            }
        }
    };
    hit.map(|(start, end, groups)| SomeMatch {
        list_idx: None,
        start: char_to_byte(&s, start),
        end: char_to_byte(&s, end),
        groups,
        item: typval_T::default(),
    })
}

pub fn f_match(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: kSomeMatch — List → matching item index; String → match start; else -1.
    rettv.vval = v_number(find_some_match(argvars).map_or(-1, |m| m.list_idx.unwrap_or(m.start)));
}

/// Port of `f_substitute()` from `Src/eval/funcs.c` — replace matches of `{pat}`
/// in `{expr}` with `{sub}` per `{flags}` (`g` = all, `i` = ignore case).
pub fn f_substitute(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let pat = tv_get_string(&argvars[1]);
    let sub = tv_get_string(&argvars[2]);
    let flags = argvars.get(3).map(tv_get_string).unwrap_or_default();
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(crate::viml_regex::regex_substitute(&s, &pat, &sub, &flags));
}

// Port of `f_join()` from `Src/eval/funcs.c` — join a List with a separator
// (default " "). `f_join` lives in its real home file,
// `src/ported/eval/typval.rs` (eval/typval.c).

/// Port of `f_range()` from `Src/eval/funcs.c`.
///
/// "range({expr} [, {max} [, {stride}]])" — `range(n)` is `0..n-1`;
/// `range(a, b[, s])` is `a, a+s, …` up to and including `b`.
pub fn f_range(argvars: &[typval_T], rettv: &mut typval_T) {
    let a0 = tv_get_number_chk(&argvars[0], None);
    let (start, end, stride) = match argvars.len() {
        1 => (0, a0 - 1, 1),
        2 => (a0, tv_get_number_chk(&argvars[1], None), 1),
        _ => (
            a0,
            tv_get_number_chk(&argvars[1], None),
            tv_get_number_chk(&argvars[2], None),
        ),
    };
    // c: `if (stride == 0) { emsg(_("E726: Stride is zero")); return; }` — a zero
    // stride is an error, not an empty list (the loop would never terminate).
    if stride == 0 {
        emsg("E726: Stride is zero");
        return;
    }
    // c: `if (stride > 0 ? end + 1 < start : end - 1 > start)` — a range that
    // runs the wrong way is E727, not an empty list: `range(10, 5, 1)` errors.
    // (`end + 1 == start` is the legitimate empty range, e.g. `range(0)`.)
    if if stride > 0 {
        end + 1 < start
    } else {
        end - 1 > start
    } {
        emsg("E727: Start past end");
        return;
    }
    let l = tv_list_alloc_ret(rettv, 0);
    let mut lb = l.borrow_mut();
    if stride > 0 {
        let mut i = start;
        while i <= end {
            tv_list_append_number(&mut lb, i);
            i += stride;
        }
    } else {
        let mut i = start;
        while i >= end {
            tv_list_append_number(&mut lb, i);
            i += stride;
        }
    }
}

/// Port of `f_add()` — `vendor/eval/list.c:429`. Append `{expr}` to a List (any
/// value) or a Blob (as a byte); else E897. Returns the same object, or 1 on
/// failure.
pub fn f_add(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: rettv->vval.v_number = 1;  — default failed.
    *rettv = typval_T::from(1 as varnumber_T);
    match (argvars[0].v_type, &argvars[0].vval) {
        // c: VAR_LIST — append the value.
        (VAR_LIST, v_list(Some(l))) => {
            tv_list_append_tv(&mut l.borrow_mut(), argvars[1].clone());
            *rettv = argvars[0].clone();
        }
        // c: VAR_BLOB — append the value as a byte.
        (VAR_BLOB, v_blob(Some(b))) => {
            let n = tv_get_number_chk(&argvars[1], None);
            b.borrow_mut().bv_ga.push(n as u8);
            *rettv = argvars[0].clone();
        }
        _ => emsg("E897: List or Blob required"),
    }
}

/// Port of `f_reverse()` — `vendor/eval/list.c:826`. Reverse a List or Blob in
/// place (returning the same object), or a String (returning a new, reversed
/// String via `reverse_text()`). Anything else returns 0.
pub fn f_reverse(argvars: &[typval_T], rettv: &mut typval_T) {
    match (argvars[0].v_type, &argvars[0].vval) {
        // c: VAR_LIST — reversed in place, the same List returned.
        (VAR_LIST, v_list(Some(l))) => {
            l.borrow_mut().lv_items.reverse();
            *rettv = argvars[0].clone();
        }
        // c: VAR_BLOB — bytes reversed in place, the same Blob returned.
        (VAR_BLOB, v_blob(Some(b))) => {
            b.borrow_mut().bv_ga.reverse();
            *rettv = argvars[0].clone();
        }
        // c: VAR_STRING — a new String from reverse_text() (by character).
        (VAR_STRING, v_string(s)) => {
            rettv.v_type = VAR_STRING;
            rettv.vval = v_string(reverse_text(s));
        }
        _ => {}
    }
}

/// Port of `reverse_text()` (Neovim strings.c) — reverse a string by character,
/// keeping each base character together with its trailing composing marks (so
/// "e" + combining-acute stays a valid grapheme after reversal). Returns a new
/// owned string.
fn reverse_text(s: &str) -> String {
    use crate::ported::strings::utf_iscomposing;
    let chars: Vec<char> = s.chars().collect();
    // Group each base char with the composing chars that follow it.
    let mut groups: Vec<&[char]> = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        let next = i + 1;
        // Extend the current group over following composing characters.
        if next < chars.len() && utf_iscomposing(chars[next]) {
            i = next;
            continue;
        }
        groups.push(&chars[start..=i]);
        i = next;
        start = i;
    }
    groups.iter().rev().flat_map(|g| g.iter()).collect()
}

/// Port of `f_get()` from `Src/eval/funcs.c` — `get({list}, {idx} [, {def}])`,
/// `get({dict}, {key} [, {def}])`, `get({blob}, {idx} [, {def}])`. A String (or
/// any non-container) errors with E1531, as in Vim — it never silently returns
/// the default.
pub fn f_get(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: VAR_BLOB — the byte at {idx} (negative from the end); out of range
    // gives {def} when present, else -1.
    if let (VAR_BLOB, v_blob(b)) = (argvars[0].v_type, &argvars[0].vval) {
        rettv.v_type = VAR_NUMBER;
        if let Some(b) = b {
            let b = b.borrow();
            let len = tv_blob_len(&b);
            let mut idx = tv_get_number_chk(&argvars[1], None) as i32;
            if idx < 0 {
                idx += len;
            }
            if idx >= 0 && idx < len {
                rettv.vval = v_number(tv_blob_get(&b, idx) as varnumber_T);
                return;
            }
        }
        match argvars.get(2) {
            Some(d) => *rettv = d.clone(),
            None => rettv.vval = v_number(-1),
        }
        return;
    }
    let default = argvars.get(2).cloned();
    let found = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => {
            let l = l.borrow();
            let len = l.lv_len as varnumber_T;
            let mut i = tv_get_number_chk(&argvars[1], None);
            if i < 0 {
                i += len;
            }
            l.lv_items.get(i as usize).map(|it| it.li_tv.clone())
        }
        (VAR_DICT, v_dict(Some(d))) => {
            tv_dict_find(&d.borrow(), &tv_get_string(&argvars[1])).cloned()
        }
        (VAR_LIST, _) | (VAR_DICT, _) => None,
        // c: get() on a Funcref/Partial reads "func"/"name"/"dict"/"args" —
        // not yet ported; fall through to the default rather than error.
        (VAR_FUNC, _) | (VAR_PARTIAL, _) => None,
        // c: else semsg(e_listdictblobarg, "get()").
        _ => {
            emsg("E1531: Argument of get() must be a List, Tuple, Dictionary or Blob");
            return;
        }
    };
    match found.or(default) {
        Some(v) => *rettv = v,
        None => rettv.vval = v_number(0),
    }
}

/// Port of `f_has_key()` from `Src/eval/funcs.c`.
pub fn f_has_key(argvars: &[typval_T], rettv: &mut typval_T) {
    let present = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_DICT, v_dict(Some(d))) => {
            tv_dict_find(&d.borrow(), &tv_get_string(&argvars[1])).is_some()
        }
        _ => false,
    };
    rettv.vval = v_number(present as varnumber_T);
}

/// Port of `f_keys()` from `Src/eval/funcs.c` — a List of a Dict's keys.
pub fn f_keys(argvars: &[typval_T], rettv: &mut typval_T) {
    crate::ported::eval::typval::tv_dict2list(
        argvars,
        rettv,
        crate::ported::eval::typval::DictListType::kDict2ListKeys,
    );
}

/// Port of `f_values()` from `Src/eval/funcs.c` — a List of a Dict's values.
pub fn f_values(argvars: &[typval_T], rettv: &mut typval_T) {
    crate::ported::eval::typval::tv_dict2list(
        argvars,
        rettv,
        crate::ported::eval::typval::DictListType::kDict2ListValues,
    );
}

/// Port of `f_max()` from `Src/eval/funcs.c`.
pub fn f_max(argvars: &[typval_T], rettv: &mut typval_T) {
    max_min(argvars, rettv, true);
}

/// Port of `f_min()` from `Src/eval/funcs.c`.
pub fn f_min(argvars: &[typval_T], rettv: &mut typval_T) {
    max_min(argvars, rettv, false);
}

/// Port of `max_min()` from `Src/eval/funcs.c` — the shared `max`/`min` body
/// over a List's items or a Dict's values; `domax` picks the direction. Empty
/// (or a non-collection) → 0.
fn max_min(argvars: &[typval_T], rettv: &mut typval_T, domax: bool) {
    let pick = |acc: varnumber_T, v: varnumber_T| if domax { acc.max(v) } else { acc.min(v) };
    let n = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => {
            let l = l.borrow();
            let mut it = l.lv_items.iter().map(|x| tv_get_number_chk(&x.li_tv, None));
            match it.next() {
                Some(first) => it.fold(first, pick),
                None => 0,
            }
        }
        (VAR_DICT, v_dict(Some(d))) => {
            let d = d.borrow();
            let mut it = d.dv_hashtab.values().map(|v| tv_get_number_chk(v, None));
            match it.next() {
                Some(first) => it.fold(first, pick),
                None => 0,
            }
        }
        _ => 0,
    };
    rettv.vval = v_number(n);
}

// Port of `f_count()` from `Src/eval/funcs.c` (subset) — occurrences of
// `{expr}` in a List. `f_count` lives in its real home file,
// `src/ported/eval/list.rs` (eval/list.c).

/// Port of `f_index()` from `Src/eval/funcs.c` — the first index of `{expr}` in
/// a List or Blob, or -1. Honours `{start}` (a user index; negative counts from
/// the end) and, for the List form, `{ic}` (ignore case in the comparison).
pub fn f_index(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(-1);
    let needle = &argvars[1];

    // c: VAR_BLOB — scan bytes from {start}; ic is never applied to a Blob.
    if let (VAR_BLOB, v_blob(b)) = (argvars[0].v_type, &argvars[0].vval) {
        let Some(b) = b else { return };
        let b = b.borrow();
        let len = tv_blob_len(&b);
        let mut start = argvars.get(2).map_or(0, tv_get_number) as i32;
        if start < 0 {
            start = (len + start).max(0);
        }
        for idx in start..len {
            let tv = typval_T::from(tv_blob_get(&b, idx) as varnumber_T);
            if tv_equal(&tv, needle, false) {
                rettv.vval = v_number(idx as varnumber_T);
                return;
            }
        }
        return;
    }

    // c: otherwise it must be a List (else e_listblobreq).
    let l = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => l.clone(),
        (VAR_LIST, _) => return, // NULL list → not found
        _ => {
            emsg("E897: List or Blob required");
            return;
        }
    };
    let lb = l.borrow();
    let len = lb.lv_items.len() as isize;
    // c: {start} via tv_list_uidx — a user index, negative from the end; an
    // out-of-range start yields no item (→ -1).
    let mut start: isize = 0;
    if let Some(a2) = argvars.get(2) {
        let mut n = tv_get_number(a2) as isize;
        if n < 0 {
            n += len;
        }
        if n < 0 || n >= len {
            return;
        }
        start = n;
    }
    // c: {ic} — ignore case (only the List form reads it).
    let ic = argvars.get(3).is_some_and(|t| tv_get_number(t) != 0);
    for (i, it) in lb.lv_items.iter().enumerate().skip(start as usize) {
        if tv_equal(&it.li_tv, needle, ic) {
            rettv.vval = v_number(i as varnumber_T);
            return;
        }
    }
}

/// Port of `f_has()` from `vendor/eval/funcs.c:2654` — feature presence.
///
/// NOT a faithful copy of the C `has_list[]`: that table is gated by Neovim's
/// build (`#ifdef UNIX`, `#ifdef __APPLE__`, …) and also lists editor features
/// (windows/syntax/folding/…) that a standalone eval engine does not provide.
/// This deliberately reports (a) the platform features via `cfg!()` — the same
/// conditions the C compiles them under — and (b) only the language/runtime
/// features vimlrs actually implements, so `has()` never claims an absent
/// capability. The fast-path runtime probes (`ttyin`/`ttyout`/`patch-*`/…) follow
/// the C's pre-`has_list[]` checks.
pub fn f_has(argvars: &[typval_T], rettv: &mut typval_T) {
    use std::io::IsTerminal;
    // c: name comparison is case-insensitive (STRICMP) throughout.
    let name = tv_get_string(&argvars[0]).to_ascii_lowercase();

    // c: fast-path features checked before the has_list[] scan. vimlrs reports
    // only what it genuinely is, so the runtime probes resolve to real answers:
    // ttyin/ttyout from the actual std handles, no GUI, and — unlike Neovim —
    // it is not Vim or Nvim, so version/patch and the `nvim` feature are absent.
    let n = match name.as_str() {
        "ttyin" => std::io::stdin().is_terminal(),
        "ttyout" => std::io::stdout().is_terminal(),
        "multi_byte_encoding" => true, // always UTF-8 here
        "gui_running" | "vim_starting" | "syntax_items" | "wsl" => false,
        _ if name.starts_with("patch") || name.starts_with("nvim-") => false,
        // Platform features: provably true from the build target (Neovim's
        // has_list[] gates these the same way with #ifdef).
        "unix" => cfg!(unix),
        "linux" => cfg!(target_os = "linux"),
        "mac" | "macunix" | "osx" | "osxdarwin" => cfg!(target_os = "macos"),
        "bsd" => {
            cfg!(any(
                target_os = "freebsd",
                target_os = "openbsd",
                target_os = "netbsd",
                target_os = "dragonfly"
            )) && !cfg!(target_os = "macos")
        }
        "win32" => cfg!(windows),
        "win64" => cfg!(all(windows, target_pointer_width = "64")),
        "sun" => cfg!(target_os = "solaris"),
        "fork" => cfg!(unix),
        // Language/runtime features vimlrs actually implements (each backed by a
        // working builtin or core behaviour). Editor features Neovim's has_list[]
        // claims — windows, syntax, folding, mouse, statusline, … — are
        // deliberately omitted: vimlrs is a standalone eval engine without them.
        "eval" | "float" | "vimlrs" | "lambda" | "num64" | "vimscript-1" | "vim9script"
        | "multi_byte" | "reltime" | "nanotime" | "iconv" | "digraphs" | "modify_fname"
        | "gettext" | "byte_offset" => true,
        _ => false,
    };
    rettv.vval = v_number(n as varnumber_T);
}

/// Port of `f_exists()` from `Src/eval/funcs.c` (subset) — whether a variable
/// exists (the `*func`/`:cmd`/option forms arrive with their ports).
pub fn f_exists(argvars: &[typval_T], rettv: &mut typval_T) {
    let name = tv_get_string(&argvars[0]);
    // c: a leading '#' queries autocommands — `#{event}` or `#{event}#{pat}`.
    let present = if let Some(au) = name.strip_prefix('#') {
        au_exists(au)
    } else if let Some(func) = name.strip_prefix('*') {
        // c: '*' queries a callable — a builtin or user function by name.
        crate::ported::eval::typval::FUNC_EXISTS_HOOK
            .with(|h| *h.borrow())
            .is_some_and(|f| f(func))
    } else {
        crate::ported::eval::vars::eval_variable(&name).is_some()
    };
    rettv.vval = v_number(present as varnumber_T);
}

/// Port of `f_printf()` from `Src/eval/funcs.c` (subset) — `%[-0][width][.prec]`
/// with conversions `d`/`i`, `s`, `f`, `x`/`X`, and `%%`. The full
/// `vim_vsnprintf` conversion set arrives with that port.
pub fn f_printf(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    let fmt = tv_get_string(&argvars[0]);
    // c (`vim_vsnprintf_typval` → `parse_fmt_types`, Src/strings.c:1101):
    // `$`-style (positional) conversions are validated in a pre-pass over the
    // whole format before anything renders — E1500 (mixed), E1501 (unused
    // slot), E1502/E1504 (slot type conflicts), E1503 (slot past the supplied
    // arguments), E1505 (malformed `$` spec), E1510 (huge digit run) — and on
    // failure printf() yields the empty string.
    if crate::ported::strings::parse_fmt_types(&fmt, argvars.len() - 1)
        == crate::ported::eval_h::FAIL
    {
        rettv.vval = v_string(String::new());
        return;
    }
    let bytes: Vec<char> = fmt.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    let mut arg = 1usize;
    // c (`vim_vsnprintf_typval`, via `tvs_get_number`/`tvs_get_string`): reading
    // past the last supplied argument is E766, and any argument the format never
    // consumed is E767. `used_max` is the highest `argvars` index a conversion
    // actually read, so a positional spec (`%2$s`) counts toward it too.
    let mut missing = false;
    let mut used_max = 0usize;
    while i < bytes.len() {
        if bytes[i] != '%' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        i += 1; // past '%'
                // Positional argument: `%N$conv` selects the Nth argument
                // (1-based, arg 1 = first after the format). The digit run is a
                // position only when followed by `$`; otherwise it is the width.
        let mut explicit_idx: Option<usize> = None;
        {
            let save = i;
            let mut n = 0usize;
            let mut got = false;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                n = n * 10 + (bytes[i] as usize - '0' as usize);
                got = true;
                i += 1;
            }
            if got && i < bytes.len() && bytes[i] == '$' {
                i += 1; // past '$'
                explicit_idx = Some(n);
            } else {
                i = save; // not positional — rewind for the width parse
            }
        }
        // Flags.
        let mut left = false;
        let mut zero = false;
        let mut plus = false;
        let mut space = false;
        let mut alt = false; // `#` alternate form
        while i < bytes.len() && matches!(bytes[i], '-' | '0' | '+' | ' ' | '#') {
            match bytes[i] {
                '-' => left = true,
                '0' => zero = true,
                '+' => plus = true,
                ' ' => space = true,
                '#' => alt = true,
                _ => {}
            }
            i += 1;
        }
        // Width. `*` takes the width from the next (sequential) argument —
        // or, c (`skip_to_arg`): `*N$` takes it from positional argument N. A
        // negative value means left-justify, as in C.
        let mut width = 0usize;
        if i < bytes.len() && bytes[i] == '*' {
            i += 1;
            let mut wsrc = arg;
            let mut positional_w = false;
            {
                let save = i;
                let mut n = 0usize;
                let mut got = false;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    n = n * 10 + (bytes[i] as usize - '0' as usize);
                    got = true;
                    i += 1;
                }
                if got && i < bytes.len() && bytes[i] == '$' {
                    i += 1;
                    wsrc = n;
                    positional_w = true;
                } else {
                    i = save;
                }
            }
            let w = match argvars.get(wsrc) {
                Some(t) => {
                    used_max = used_max.max(wsrc);
                    tv_get_number_chk(t, None)
                }
                None => {
                    missing = true;
                    0
                }
            };
            // A positional width does not advance the sequential counter.
            if !positional_w {
                arg += 1;
            }
            if w < 0 {
                left = true;
                width = (-w) as usize;
            } else {
                width = w as usize;
            }
        } else {
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                width = width * 10 + (bytes[i] as usize - '0' as usize);
                i += 1;
            }
        }
        // Precision. `.*` takes the precision from the next argument — or,
        // c (`skip_to_arg`): `.*N$` from positional argument N.
        let mut prec: Option<usize> = None;
        if i < bytes.len() && bytes[i] == '.' {
            i += 1;
            if i < bytes.len() && bytes[i] == '*' {
                i += 1;
                let mut psrc = arg;
                let mut positional_p = false;
                {
                    let save = i;
                    let mut n = 0usize;
                    let mut got = false;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        n = n * 10 + (bytes[i] as usize - '0' as usize);
                        got = true;
                        i += 1;
                    }
                    if got && i < bytes.len() && bytes[i] == '$' {
                        i += 1;
                        psrc = n;
                        positional_p = true;
                    } else {
                        i = save;
                    }
                }
                let p = match argvars.get(psrc) {
                    Some(t) => {
                        used_max = used_max.max(psrc);
                        tv_get_number_chk(t, None)
                    }
                    None => {
                        missing = true;
                        0
                    }
                };
                // A positional precision does not advance the sequential counter.
                if !positional_p {
                    arg += 1;
                }
                prec = Some(p.max(0) as usize);
            } else {
                let mut p = 0usize;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    p = p * 10 + (bytes[i] as usize - '0' as usize);
                    i += 1;
                }
                prec = Some(p);
            }
        }
        // c (vim_vsnprintf_typval, docs `eval.txt`/vendor/eval.lua:8559): a
        // field-width or precision that would produce a string longer than 1 MB
        // (1024*1024 = 1048576) raises E1510 rather than allocating the buffer.
        // Verified against `/opt/homebrew/bin/vim`: width/precision 1048576 is
        // accepted, 1048577 errors. Without this a huge width (`%999999999d`)
        // hangs building a gigabyte string.
        const PRINTF_MAX: usize = 1024 * 1024;
        if width > PRINTF_MAX {
            emsg(&format!("E1510: Value too large: {width}"));
            rettv.vval = v_string(String::new());
            return;
        }
        if let Some(p) = prec {
            if p > PRINTF_MAX {
                emsg(&format!("E1510: Value too large: {p}"));
                rettv.vval = v_string(String::new());
                return;
            }
        }
        let Some(conv) = bytes.get(i).copied() else {
            out.push('%');
            break;
        };
        i += 1;
        if conv == '%' {
            out.push('%');
            continue;
        }
        let want = explicit_idx.unwrap_or(arg);
        let cur = argvars.get(want);
        // c (`tvs_get_float`): a float conversion reports ONE error for any
        // non-numeric argument — "E807: Expected Float argument for printf()" —
        // rather than the per-type error `tv_get_float` would raise (E892 String,
        // E893 List, …). The integer conversions do keep `tv_get_number`'s
        // per-type errors (`printf('%d', [1])` is E745), so this is specific to
        // the float family.
        if matches!(conv, 'f' | 'F' | 'e' | 'E' | 'g' | 'G')
            && cur.is_some_and(|t| !matches!(t.v_type, VAR_NUMBER | VAR_FLOAT))
        {
            emsg("E807: Expected Float argument for printf()");
            rettv.vval = v_string(String::new());
            return;
        }
        if cur.is_some() {
            used_max = used_max.max(want);
        } else {
            missing = true;
        }
        // c (vim_vsnprintf_typval): inf/nan render as fixed words, lowercase for
        // the lowercase float conversions (f/e/g), uppercase for the uppercase
        // ones (F/E/G); negative infinity keeps a leading '-', nan is unsigned;
        // zero-padding is suppressed (space-padded). `nonfinite`/`nonfinite_nan`
        // carry that state to the sign/pad logic below.
        let mut nonfinite = false;
        let mut nonfinite_nan = false;
        let nf_str = |v: f64, upper: bool| -> String {
            if v.is_nan() {
                if upper { "NAN" } else { "nan" }.to_string()
            } else if v < 0.0 {
                if upper { "-INF" } else { "-inf" }.to_string()
            } else if upper {
                "INF".to_string()
            } else {
                "inf".to_string()
            }
        };
        let core = match conv {
            'd' | 'i' => cur.map_or(0, |t| tv_get_number_chk(t, None)).to_string(),
            // c: `%s`/`%S` fetch the argument through `tv_str()`, which for a
            // non-string typval returns `encode_tv2echo()` — so List/Dict/Funcref/
            // Blob stringify (`[1, 2, 3]`, `{'a': 1}`, `type`) instead of raising
            // E730 as `tv_get_string_buf_chk` would. `%S` differs from `%s` only in
            // that width/precision count screen cells; the value renders the same.
            's' | 'S' => {
                let mut s = cur.map(encode_tv2echo).unwrap_or_default();
                if let Some(p) = prec {
                    // c: precision caps the byte count; keep it a char boundary so
                    // multi-byte container output never splits mid-codepoint.
                    if s.len() > p {
                        let mut end = p;
                        while end > 0 && !s.is_char_boundary(end) {
                            end -= 1;
                        }
                        s.truncate(end);
                    }
                }
                s
            }
            'f' | 'F' => {
                let v = cur.map_or(0.0, tv_get_float);
                if !v.is_finite() {
                    nonfinite = true;
                    nonfinite_nan = v.is_nan();
                    nf_str(v, conv == 'F')
                } else {
                    format!("{:.*}", prec.unwrap_or(6), v)
                }
            }
            'x' => format!("{:x}", cur.map_or(0, |t| tv_get_number_chk(t, None))),
            'X' => format!("{:X}", cur.map_or(0, |t| tv_get_number_chk(t, None))),
            'o' => format!("{:o}", cur.map_or(0, |t| tv_get_number_chk(t, None))),
            'b' | 'B' => format!("{:b}", cur.map_or(0, |t| tv_get_number_chk(t, None))),
            'u' => (cur.map_or(0, |t| tv_get_number_chk(t, None)) as u64).to_string(),
            'c' => {
                // c: `%c` emits a single byte — the value truncated to `char`
                // (`str[0] = (char)uj`), i.e. `value & 0xFF`.
                let byte = (cur.map_or(0, |t| tv_get_number_chk(t, None)) & 0xFF) as u32;
                char::from_u32(byte).unwrap_or('\u{0}').to_string()
            }
            'g' | 'G' => {
                // C `%g`: `prec` significant digits (default 6), trailing zeros
                // stripped, `%e`/`%f` chosen by exponent.
                let v = cur.map_or(0.0, tv_get_float);
                if !v.is_finite() {
                    nonfinite = true;
                    nonfinite_nan = v.is_nan();
                    nf_str(v, conv == 'G')
                } else {
                    let s = crate::ported::eval::encode::vim_float_g(v, prec.map(|p| p as i32));
                    if conv == 'G' {
                        s.to_uppercase()
                    } else {
                        s
                    }
                }
            }
            'e' | 'E' => {
                let v = cur.map_or(0.0, tv_get_float);
                if !v.is_finite() {
                    nonfinite = true;
                    nonfinite_nan = v.is_nan();
                    nf_str(v, conv == 'E')
                } else {
                    let s = format!("{:.*e}", prec.unwrap_or(6), v);
                    // Rust emits "1e2"; C/Vim emit "1.000000e+02" — sign + 2-digit exp.
                    if let Some(ep) = s.find('e') {
                        let (m, ex) = s.split_at(ep);
                        let en: i32 = ex[1..].parse().unwrap_or(0);
                        format!(
                            "{m}{}{}{:02}",
                            conv,
                            if en < 0 { '-' } else { '+' },
                            en.abs()
                        )
                    } else {
                        s
                    }
                }
            }
            other => {
                out.push('%');
                out.push(other);
                continue;
            }
        };
        // A positional spec does not advance the sequential argument counter.
        if explicit_idx.is_none() {
            arg += 1;
        }
        // c: integer conversions treat precision as the minimum number of digits,
        // left-padding the magnitude with `0`. A precision of 0 with value 0
        // produces no digits at all, and specifying a precision makes the `0`
        // width flag have no effect.
        let mut zero = zero;
        let mut core = core;
        // c: inf/nan are space-padded, never zero-padded, regardless of the flag.
        if nonfinite {
            zero = false;
        }
        if matches!(conv, 'd' | 'i' | 'o' | 'u' | 'x' | 'X' | 'b' | 'B') {
            if let Some(p) = prec {
                zero = false;
                let (neg, digits) = match core.strip_prefix('-') {
                    Some(rest) => (true, rest.to_string()),
                    None => (false, core),
                };
                let digits = if p == 0 && digits == "0" {
                    String::new()
                } else if digits.len() < p {
                    format!("{}{digits}", "0".repeat(p - digits.len()))
                } else {
                    digits
                };
                core = if neg { format!("-{digits}") } else { digits };
            }
        }
        // For signed numeric conversions the `+`/space flag forces a sign on
        // non-negative values; split it off `core` so zero-padding lands between
        // the sign and the digits (`%+05d` of 7 → `+0007`).
        let signed = matches!(conv, 'd' | 'i' | 'f' | 'F' | 'e' | 'E' | 'g' | 'G');
        let (sign, core) = if nonfinite_nan {
            // c: nan carries no sign even under the `+`/space flag.
            ("", core)
        } else if signed {
            if let Some(rest) = core.strip_prefix('-') {
                ("-", rest.to_string())
            } else if plus {
                ("+", core)
            } else if space {
                (" ", core)
            } else {
                ("", core)
            }
        } else if alt {
            // `#` alternate form for the unsigned radix conversions: a `0x`/`0X`/
            // `0b`/`0B` prefix on a non-zero x/X/b/B, and a leading `0` on octal.
            // Carried in the sign slot so zero-padding lands after the prefix.
            let prefix = match conv {
                'x' if core != "0" => "0x",
                'X' if core != "0" => "0X",
                'b' if core != "0" => "0b",
                'B' if core != "0" => "0B",
                'o' if !core.starts_with('0') => "0",
                _ => "",
            };
            (prefix, core)
        } else {
            ("", core)
        };
        // Pad to width (width counts the sign). c: `%s` width is the byte length
        // (`strlen`); `%S` counts screen cells (here approximated by the codepoint
        // count); numeric conversions are ASCII so bytes and chars coincide.
        let visible = if conv == 'S' {
            core.chars().count()
        } else {
            core.len()
        };
        let len = sign.len() + visible;
        if len >= width {
            out.push_str(sign);
            out.push_str(&core);
        } else {
            let pad = width - len;
            if left {
                out.push_str(sign);
                out.push_str(&core);
                out.extend(std::iter::repeat(' ').take(pad));
            } else if zero && conv != 's' {
                out.push_str(sign);
                out.extend(std::iter::repeat('0').take(pad));
                out.push_str(&core);
            } else {
                out.extend(std::iter::repeat(' ').take(pad));
                out.push_str(sign);
                out.push_str(&core);
            }
        }
    }
    // c: a conversion that ran off the end of the argument list is E766, and an
    // argument the format never consumed is E767. C reports these from
    // `vim_vsnprintf_typval`, and `f_printf` then leaves the result NULL — so the
    // value is the empty string, not the half-formatted text.
    if missing {
        emsg("E766: Insufficient arguments for printf()");
        rettv.vval = v_string(String::new());
        return;
    }
    if used_max + 1 < argvars.len() {
        emsg("E767: Too many arguments to printf()");
        rettv.vval = v_string(String::new());
        return;
    }
    rettv.vval = v_string(out);
}

// ── float math (Src/eval/funcs.c — one `f_*` per libm call) ──

/// Apply a unary `f64 -> f64` op to argvar 0, returning a `VAR_FLOAT`. Shared
/// by the float-math `f_*` below (the C bodies are each `float_op(argvars,
/// rettv, &fn)`).
/// Port of `float_op_wrapper()` from `Src/eval/funcs.c` (c:344) — apply a C
/// `double(double)` math function to a single float argument. In Neovim the
/// builtins `sqrt()`/`floor()`/`sin()`/… are `eval.lua` table entries that set
/// `func_float` and route here; there are NO per-function `f_sqrt`/`f_floor`/…
/// functions. The caller supplies `op` (the C `fptr.func_float`).
pub fn float_op_wrapper(argvars: &[typval_T], rettv: &mut typval_T, op: fn(f64) -> f64) {
    rettv.v_type = VAR_FLOAT;
    rettv.vval = v_float(op(tv_get_float(&argvars[0])));
}

/// Port of `f_pow()` from `Src/eval/funcs.c` — `pow(x, y)`.
pub fn f_pow(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_FLOAT;
    rettv.vval = v_float(tv_get_float(&argvars[0]).powf(tv_get_float(&argvars[1])));
}

// ── bitwise (Src/eval/funcs.c) ──

/// Port of `f_and()` from `Src/eval/funcs.c` — bitwise AND.
pub fn f_and(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval =
        v_number(tv_get_number_chk(&argvars[0], None) & tv_get_number_chk(&argvars[1], None));
}
/// Port of `f_or()` from `Src/eval/funcs.c` — bitwise OR.
pub fn f_or(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval =
        v_number(tv_get_number_chk(&argvars[0], None) | tv_get_number_chk(&argvars[1], None));
}
/// Port of `f_xor()` from `Src/eval/funcs.c` — bitwise XOR.
pub fn f_xor(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval =
        v_number(tv_get_number_chk(&argvars[0], None) ^ tv_get_number_chk(&argvars[1], None));
}
/// Port of `f_invert()` from `Src/eval/funcs.c` — bitwise NOT.
pub fn f_invert(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(!tv_get_number_chk(&argvars[0], None));
}

// ── more string functions (Src/eval/funcs.c) ──

// ── more list / dict functions (Src/eval/funcs.c) ──

/// Port of `f_insert()` — `vendor/eval/list.c:735`. Insert `{item}` before index
/// `{idx}` (default 0; negative counts from the end) in a List, or insert a byte
/// in a Blob. Out-of-range `{idx}` errors (E684 for a List, E475 for a Blob).
pub fn f_insert(argvars: &[typval_T], rettv: &mut typval_T) {
    let before_arg = argvars.get(2).filter(|t| t.v_type != VAR_UNKNOWN);
    // c: VAR_BLOB — insert a byte (0..255) at {idx} in 0..=len.
    if let (VAR_BLOB, v_blob(b)) = (argvars[0].v_type, &argvars[0].vval) {
        let Some(b) = b else { return };
        let len = b.borrow().bv_ga.len() as varnumber_T;
        let before = before_arg.map_or(0, |t| tv_get_number_chk(t, None));
        if before < 0 || before > len {
            crate::ported::message::semsg(&format!(
                "E475: Invalid argument: {}",
                tv_get_string(&argvars[2])
            ));
            return;
        }
        let val = tv_get_number_chk(&argvars[1], None);
        if !(0..=255).contains(&val) {
            crate::ported::message::semsg(&format!(
                "E475: Invalid argument: {}",
                tv_get_string(&argvars[1])
            ));
            return;
        }
        b.borrow_mut().bv_ga.insert(before as usize, val as u8);
        *rettv = argvars[0].clone();
        return;
    }
    // c: else must be a List.
    let l = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => l.clone(),
        (VAR_LIST, _) => {
            *rettv = argvars[0].clone();
            return;
        }
        _ => {
            emsg("E897: List or Blob required");
            return;
        }
    };
    let mut lb = l.borrow_mut();
    let len = lb.lv_len as varnumber_T;
    let orig = before_arg.map_or(0, |t| tv_get_number_chk(t, None));
    // c: tv_list_find handles a negative index (from the end); an out-of-range
    // index (other than == len, which appends) is E684.
    let idx = if orig < 0 { orig + len } else { orig };
    if orig != len && (idx < 0 || idx >= len) {
        crate::ported::message::semsg(&format!("E684: List index out of range: {orig}"));
        return;
    }
    lb.lv_items.insert(
        idx.clamp(0, len) as usize,
        crate::ported::eval::typval_defs_h::listitem_T {
            li_tv: argvars[1].clone(),
        },
    );
    lb.lv_len = lb.lv_items.len() as i32;
    drop(lb);
    *rettv = argvars[0].clone();
}

// Port of `f_remove()` from `Src/eval/funcs.c` (subset) — remove and return an
// item from a `{list}` by index, or a value from a `{dict}` by key. `f_remove`
// lives in its real home file, `src/ported/eval/list.rs` (eval/list.c).

// `f_extend`/`f_extendnew` live in their real home file, `src/ported/eval/list.rs`.

/// Port of `f_copy()` from `Src/eval/funcs.c` — a shallow copy of `{expr}`.
pub fn f_copy(argvars: &[typval_T], rettv: &mut typval_T) {
    match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => {
            let items: Vec<_> = l
                .borrow()
                .lv_items
                .iter()
                .map(|it| it.li_tv.clone())
                .collect();
            let out = tv_list_alloc_ret(rettv, items.len() as isize);
            let mut ob = out.borrow_mut();
            for tv in items {
                tv_list_append_tv(&mut ob, tv);
            }
        }
        (VAR_DICT, v_dict(Some(d))) => {
            let pairs: Vec<_> = d
                .borrow()
                .dv_hashtab
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let out = crate::ported::eval::typval::tv_dict_alloc_ret(rettv);
            let mut ob = out.borrow_mut();
            for (k, v) in pairs {
                tv_dict_add_tv(&mut ob, &k, v);
            }
        }
        _ => *rettv = argvars[0].clone(),
    }
}

/// Port of `f_items()` from `Src/eval/funcs.c` — `[index/key, value]` pairs of a
/// String/List/Blob/Dict.
pub fn f_items(argvars: &[typval_T], rettv: &mut typval_T) {
    use crate::ported::eval::typval::{
        tv_blob2items, tv_dict2items, tv_list2items, tv_string2items,
    };
    match argvars[0].v_type {
        VAR_STRING => tv_string2items(argvars, rettv),
        VAR_LIST => tv_list2items(argvars, rettv),
        VAR_BLOB => tv_blob2items(argvars, rettv),
        VAR_DICT => tv_dict2items(argvars, rettv),
        _ => emsg("E1225: List, Dictionary, Blob or String required for argument 1"),
    }
}

// Port of `f_uniq()` from `Src/eval/funcs.c` (subset) — remove adjacent
// duplicate items from a `{list}`, returning it. `f_sort`/`f_uniq` live in
// their real home file, `src/ported/eval/typval.rs`.

// ── batch 4: regex-list, more string, list helpers (Src/eval/funcs.c) ──

/// Port of `f_matchlist()` from `Src/eval/funcs.c` — `[whole, sub1, …]` of the
/// first match of `{pat}` in `{expr}`.
pub fn f_matchlist(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: kSomeMatchList — [whole, \1..\9] (10 slots), or [] when no match.
    match find_some_match(argvars) {
        Some(m) => {
            let l = tv_list_alloc_ret(rettv, m.groups.len() as isize);
            let mut lb = l.borrow_mut();
            for g in &m.groups {
                tv_list_append_string(&mut lb, g);
            }
        }
        None => {
            tv_list_alloc_ret(rettv, 0);
        }
    }
}

/// Port of `f_matchend()` from `Src/eval/funcs.c` — char index just past the
/// first match of `{pat}`, or -1.
pub fn f_matchend(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: kSomeMatchEnd — List → matching item index; String → match end; else -1.
    rettv.vval = v_number(find_some_match(argvars).map_or(-1, |m| m.list_idx.unwrap_or(m.end)));
}

/// Port of `f_escape()` from `Src/eval/funcs.c` — prefix each character of
/// `{string}` that occurs in `{chars}` with a backslash.
///
/// The body is `vim_strsave_escaped_ext()` (`Src/strings.c:96`): the walk is by
/// `utfc_ptr2len` units, so a MULTIBYTE unit — including a base character plus
/// its composing marks — is copied verbatim and never escaped (only single-byte
/// characters are looked up in `{chars}`, byte-wise as `vim_strchr` does).
pub fn f_escape(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let esc = tv_get_string(&argvars[1]);
    let (bytes, esc_bytes) = (s.as_bytes(), esc.as_bytes());
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < bytes.len() {
        // c: `const size_t l = utfc_ptr2len(p); if (l > 1) { memcpy; continue; }`
        let l = crate::ported::mbyte::utfc_ptr2len(&bytes[i..]).max(1) as usize;
        if l > 1 {
            out.push_str(&s[i..i + l]);
            i += l;
            continue;
        }
        // c: `if (vim_strchr(esc_chars, *p) != NULL) *p2++ = '\\';` — a
        // single-byte char here is ASCII, which never matches inside a
        // multibyte {chars} character's bytes.
        let b = bytes[i];
        if esc_bytes.contains(&b) {
            out.push('\\');
        }
        out.push(b as char);
        i += 1;
    }
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(out);
}

/// Port of `f_list2str()` from `Src/eval/funcs.c` — a String from a List of code
/// points.
pub fn f_list2str(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    let mut out = String::new();
    if let (VAR_LIST, v_list(Some(l))) = (argvars[0].v_type, &argvars[0].vval) {
        for it in &l.borrow().lv_items {
            let n = tv_get_number_chk(&it.li_tv, None);
            // c: the codepoints are written into a C string, so a 0 terminates it —
            // `list2str([65, 0, 66])` is `'A'`, not `'A<NUL>B'`.
            if n == 0 {
                break;
            }
            if let Some(c) = char::from_u32(n as u32) {
                out.push(c);
            }
        }
    }
    rettv.vval = v_string(out);
}

/// Port of `flatten_common()` from `Src/eval/funcs.c:1529`.
///
/// Shared body of `flatten()`/`flattennew()`: flatten a nested List up to
/// `{maxdepth}` (default 999999). `make_copy` flattens a fresh copy (flattennew)
/// instead of mutating the argument in place (flatten). The actual splicing is
/// done by the ported `tv_list_flatten`.
fn flatten_common(argvars: &[typval_T], rettv: &mut typval_T, make_copy: bool) {
    let mut error = false;
    // c: if (argvars[0].v_type != VAR_LIST) { semsg(_(e_listarg), "flatten()"); return; }
    if argvars[0].v_type != VAR_LIST {
        emsg("E686: Argument of flatten() must be a List");
        return;
    }
    // c: maxdepth = (argvars[1] == UNKNOWN) ? 999999 : tv_get_number_chk(...); E900 if < 0.
    let maxdepth = if argvars.len() < 2 {
        999999
    } else {
        let d = tv_get_number_chk(&argvars[1], Some(&mut error));
        if error {
            return;
        }
        if d < 0 {
            emsg("E900: maxdepth must be non-negative number");
            return;
        }
        d
    };
    rettv.v_type = VAR_LIST;
    // c: list = argvars[0].vval.v_list; if (list == NULL) return;
    let list = match &argvars[0].vval {
        v_list(Some(l)) => l.clone(),
        _ => {
            rettv.vval = v_list(None);
            return;
        }
    };
    let list = if make_copy {
        // c: list = tv_list_copy(NULL, list, false, get_copyID());
        tv_list_copy(&list, false)
    } else {
        // c: value_check_lock(...) (locks unmodeled, skipped); tv_list_ref(list);
        tv_list_ref(&mut list.borrow_mut());
        list
    };
    rettv.vval = v_list(Some(list.clone()));
    // c: tv_list_flatten(list, NULL, tv_list_len(list), maxdepth);
    let len = tv_list_len(&list.borrow()) as i64;
    tv_list_flatten(&mut list.borrow_mut(), len, maxdepth as i64);
}

/// Port of `f_flatten()` from `Src/eval/funcs.c:1576`.
pub fn f_flatten(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: flatten_common(argvars, rettv, false);
    flatten_common(argvars, rettv, false);
}

/// Port of `f_flattennew()` from `Src/eval/funcs.c:1582`.
pub fn f_flattennew(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: flatten_common(argvars, rettv, true);
    flatten_common(argvars, rettv, true);
}

// ── batch 5: deepcopy + more float math (Src/eval/funcs.c) ──

/// Port of `var_item_copy()` from `Src/eval/typval.c` — a deep copy of `from`
/// (Lists/Dicts copied recursively into fresh handles).
pub(crate) fn var_item_copy(from: &typval_T) -> typval_T {
    match (from.v_type, &from.vval) {
        (VAR_LIST, v_list(Some(l))) => {
            let items: Vec<typval_T> = l
                .borrow()
                .lv_items
                .iter()
                .map(|it| var_item_copy(&it.li_tv))
                .collect();
            let out = crate::ported::eval::typval::tv_list_alloc(items.len() as isize);
            {
                let mut ob = out.borrow_mut();
                for tv in items {
                    tv_list_append_tv(&mut ob, tv);
                }
            }
            typval_T {
                v_type: VAR_LIST,
                v_lock: crate::ported::eval::typval_defs_h::VarLockStatus::VAR_UNLOCKED,
                vval: v_list(Some(out)),
            }
        }
        (VAR_DICT, v_dict(Some(d))) => {
            let pairs: Vec<(String, typval_T)> = d
                .borrow()
                .dv_hashtab
                .iter()
                .map(|(k, v)| (k.clone(), var_item_copy(v)))
                .collect();
            let out = crate::ported::eval::typval::tv_dict_alloc();
            {
                let mut ob = out.borrow_mut();
                for (k, v) in pairs {
                    tv_dict_add_tv(&mut ob, &k, v);
                }
            }
            typval_T {
                v_type: VAR_DICT,
                v_lock: crate::ported::eval::typval_defs_h::VarLockStatus::VAR_UNLOCKED,
                vval: v_dict(Some(out)),
            }
        }
        _ => from.clone(),
    }
}

/// Port of `f_deepcopy()` from `Src/eval/funcs.c` — a recursive copy of `{expr}`.
pub fn f_deepcopy(argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = var_item_copy(&argvars[0]);
}

/// Port of `f_fmod()` from `Src/eval/funcs.c` — floating-point remainder.
pub fn f_fmod(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_FLOAT;
    rettv.vval = v_float(tv_get_float(&argvars[0]) % tv_get_float(&argvars[1]));
}

/// Port of `f_atan2()` from `Src/eval/funcs.c`.
pub fn f_atan2(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_FLOAT;
    rettv.vval = v_float(tv_get_float(&argvars[0]).atan2(tv_get_float(&argvars[1])));
}

// (tan/atan/asin/acos/sinh/cosh/tanh/log10 are `eval.lua` `func_float` entries
// routed through `float_op_wrapper` — no per-function `f_*`; see the bridge.)

// ── json (Src/eval/funcs.c → encode.c / decode.c) ──

/// Port of `f_json_encode()` from `Src/eval/funcs.c` — the JSON text of `{expr}`.
pub fn f_json_encode(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(crate::ported::eval::encode::encode_tv2json(&argvars[0]));
}

/// Port of `f_json_decode()` from `Src/eval/funcs.c` — the value of JSON text.
pub fn f_json_decode(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    match crate::ported::eval::decode::json_decode_string(&s) {
        Some(v) => *rettv = v,
        None => emsg("E491: JSON decode error"),
    }
}

// ── batch 5: char-indexed string ops (Src/strings.c), regex pos, env, paths ──

/// Port of `f_matchstrpos()` from `Src/eval/funcs.c` — `[match, start, end]`
/// (character indices) of the first match of `{pat}` in `{expr}`, or `['', -1,
/// -1]`. (c: `find_some_match(argvars, rettv, kSomeMatchStrPos)`.)
pub fn f_matchstrpos(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: kSomeMatchStrPos — String → [match, start, end]; List → [match, idx,
    // start, end]; no match → ["", -1, -1].
    let l = tv_list_alloc_ret(rettv, 3);
    let mut lb = l.borrow_mut();
    match find_some_match(argvars) {
        Some(m) => {
            let sub = m.groups.into_iter().next().unwrap_or_default();
            tv_list_append_string(&mut lb, &sub);
            // The List form inserts the matching item index before start/end.
            if let Some(idx) = m.list_idx {
                tv_list_append_number(&mut lb, idx);
            }
            tv_list_append_number(&mut lb, m.start);
            tv_list_append_number(&mut lb, m.end);
        }
        None => {
            tv_list_append_string(&mut lb, "");
            tv_list_append_number(&mut lb, -1);
            tv_list_append_number(&mut lb, -1);
        }
    }
}

/// Port of `f_getenv()` from `Src/eval/funcs.c` — the value of environment
/// variable `{name}`, or `v:null` if it is not set.
pub fn f_getenv(argvars: &[typval_T], rettv: &mut typval_T) {
    let name = tv_get_string(&argvars[0]);
    // c: char *p = vim_getenv(...); if (p == NULL) { v:null } else { string }
    match std::env::var(&name) {
        Ok(p) => {
            rettv.v_type = VAR_STRING;
            rettv.vval = v_string(p);
        }
        Err(_) => {
            rettv.v_type = VAR_SPECIAL;
            rettv.vval = v_special(kSpecialVarNull);
        }
    }
}

/// Port of `f_setenv()` from `Src/eval/funcs.c` — set environment variable
/// `{name}` to `{value}`, or unset it when `{value}` is `v:null`.
pub fn f_setenv(argvars: &[typval_T], _rettv: &mut typval_T) {
    let name = tv_get_string(&argvars[0]);
    if argvars[1].v_type == VAR_SPECIAL {
        // c: vim_unsetenv_ext(name)
        std::env::remove_var(&name);
    } else {
        // c: vim_setenv_ext(name, tv_get_string_buf(&argvars[1], valbuf))
        std::env::set_var(&name, tv_get_string(&argvars[1]));
    }
}

/// Port of `f_shellescape()` from `Src/eval/funcs.c` — quote `{string}` so it
/// can be used safely as a single shell word. (c: `vim_strsave_shellescape`,
/// default `do_special=false`: wrap in single quotes, `'` → `'\''`.)
pub fn f_shellescape(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    // c: `vim_strsave_shellescape(str, do_special, do_special)` — with {special}
    // truthy, the items the `:!` command would expand (`!`, `%`, `#`) and a
    // newline are preceded by a backslash, which `:!` then strips again
    // (`:help shellescape`). The `<cword>`-style cmdline variables are also
    // escaped in Vim; that needs the cmdline-var table and is not ported.
    let special = argvars
        .get(1)
        .is_some_and(|t| tv_get_number_chk(t, None) != 0);
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            if special && matches!(c, '!' | '%' | '#' | '\n') {
                out.push('\\');
            }
            out.push(c);
        }
    }
    out.push('\'');
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(out);
}

// ── batch 7: float predicates + pid (Src/eval/funcs.c) ──

/// Port of `f_isinf()` from `Src/eval/funcs.c` (c:3265) — `1` for `+inf`, `-1`
/// for `-inf`, `0` otherwise (incl. non-Float).
pub fn f_isinf(argvars: &[typval_T], rettv: &mut typval_T) {
    if let (VAR_FLOAT, v_float(f)) = (argvars[0].v_type, &argvars[0].vval) {
        if f.is_infinite() {
            rettv.vval = v_number(if *f > 0.0 { 1 } else { -1 });
        }
    }
}

/// Port of `f_isnan()` from `Src/eval/funcs.c` (c:3274) — `1` if the argument is
/// a NaN Float, else `0`.
pub fn f_isnan(argvars: &[typval_T], rettv: &mut typval_T) {
    let is = matches!((argvars[0].v_type, &argvars[0].vval), (VAR_FLOAT, v_float(f)) if f.is_nan());
    rettv.vval = v_number(is as varnumber_T);
}

/// Port of `f_getpid()` from `Src/eval/funcs.c` (c:2141) — this process's PID.
pub fn f_getpid(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(std::process::id() as varnumber_T);
}

// ── batch 8: time + soundfold + byteidxcomp (funcs.c / strings.c) ──

/// Port of `f_localtime()` from `Src/eval/funcs.c` (c:4043) — `time(NULL)`, the
/// current time in seconds since the Unix epoch.
pub fn f_localtime(_argvars: &[typval_T], rettv: &mut typval_T) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as varnumber_T)
        .unwrap_or(0);
    rettv.vval = v_number(secs);
}

/// Port of `f_soundfold()` from `Src/eval/funcs.c` (c:6943). `eval_soundfold`
/// returns `{word}` unchanged when no spell file defines a soundfold mapping —
/// which is always the case here (spell support is out of scope), so this
/// returns the word as-is, matching Vim without `:set spell`.
pub fn f_soundfold(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(tv_get_string(&argvars[0]));
}

/// Port of `list2proftime()` from `Src/eval/funcs.c:5229`.
///
/// Reads a 2-element list `[high, low]` back into the 64-bit `proftime_T`. The C
/// version type-puns through a `union { struct { int32_t low, high; }; prof; }`;
/// the equivalent bit recombination keeps the exact wraparound semantics.
fn list2proftime(arg: &typval_T, tm: &mut proftime_T) -> i32 {
    // c: if (arg->v_type != VAR_LIST || tv_list_len(arg->vval.v_list) != 2) return FAIL;
    let l = match (arg.v_type, &arg.vval) {
        (VAR_LIST, v_list(Some(l))) => l,
        _ => return FAIL,
    };
    let lb = l.borrow();
    if tv_list_len(&lb) != 2 {
        return FAIL;
    }
    let mut error = false;
    let n1 = tv_list_find_nr(&lb, 0, Some(&mut error));
    let n2 = tv_list_find_nr(&lb, 1, Some(&mut error));
    if error {
        return FAIL;
    }
    // c: u = { .split.high = (int32_t)n1, .split.low = (int32_t)n2 }; *tm = u.prof;
    let high = (n1 as i32) as u32;
    let low = (n2 as i32) as u32;
    *tm = ((high as u64) << 32) | (low as u64);
    OK
}

/// Port of `f_reltime()` from `Src/eval/funcs.c:5260`.
///
/// "reltime()" function — returns the current time, an elapsed time, or the
/// difference of two times as a 2-element list of 32-bit halves.
pub fn f_reltime(argvars: &[typval_T], rettv: &mut typval_T) {
    let mut res: proftime_T = 0;
    let mut start: proftime_T = 0;
    if argvars.is_empty() {
        // c: no arguments: get current time.
        res = profile_start();
    } else if argvars.len() == 1 {
        if list2proftime(&argvars[0], &mut res) == FAIL {
            return;
        }
        res = profile_end(res);
    } else {
        // c: two arguments: compute the difference.
        if list2proftime(&argvars[0], &mut start) == FAIL
            || list2proftime(&argvars[1], &mut res) == FAIL
        {
            return;
        }
        res = profile_sub(res, start);
    }
    // c: store the 64-bit proftime_T as two 32-bit list values [high, low].
    let high = ((res >> 32) as u32) as i32 as varnumber_T;
    let low = (res as u32) as i32 as varnumber_T;
    let l = tv_list_alloc_ret(rettv, 2);
    let mut lb = l.borrow_mut();
    tv_list_append_number(&mut lb, high);
    tv_list_append_number(&mut lb, low);
}

/// Port of `f_reltimestr()` from `Src/eval/funcs.c:5302`.
///
/// "reltimestr()" function — the elapsed time as a `%10.6f` seconds string.
pub fn f_reltimestr(argvars: &[typval_T], rettv: &mut typval_T) {
    let mut tm: proftime_T = 0;
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(String::new());
    if list2proftime(&argvars[0], &mut tm) == OK {
        rettv.vval = v_string(profile_msg(tm));
    }
}

/// Port of `f_reltimefloat()` from `Src/eval/funcs.c:6935`.
///
/// "reltimefloat()" function — the elapsed time in seconds as a Float.
pub fn f_reltimefloat(argvars: &[typval_T], rettv: &mut typval_T) {
    let mut tm: proftime_T = 0;
    rettv.v_type = VAR_FLOAT;
    rettv.vval = v_float(0.0);
    if list2proftime(&argvars[0], &mut tm) == OK {
        rettv.vval = v_float(profile_signed(tm) as f64 / 1_000_000_000.0);
    }
}

/// Port of `reduce_list()` from `Src/eval/funcs.c:5413`.
fn reduce_list(argvars: &[typval_T], expr: &typval_T, rettv: &mut typval_T) {
    let l = match &argvars[0].vval {
        v_list(Some(l)) => l.clone(),
        _ => return,
    };
    // call `name(acc, item)` via the bridge hook.
    let call = |acc: &typval_T, item: &typval_T| -> Option<typval_T> {
        CALL_FUNC_HOOK
            .with(|h| *h.borrow())
            .and_then(|f| f(expr, &[acc.clone(), item.clone()]))
    };
    let items: Vec<typval_T> = l
        .borrow()
        .lv_items
        .iter()
        .map(|it| it.li_tv.clone())
        .collect();
    let start = if argvars.len() < 3 {
        if items.is_empty() {
            emsg("E998: Reduce of an empty List with no initial value");
            return;
        }
        *rettv = items[0].clone();
        1
    } else {
        *rettv = argvars[2].clone();
        0
    };
    for item in items.iter().skip(start) {
        match call(rettv, item) {
            Some(r) => *rettv = r,
            None => return,
        }
    }
}

/// Port of `reduce_string()` from `Src/eval/funcs.c` — fold over the characters.
fn reduce_string(argvars: &[typval_T], expr: &typval_T, rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let call = |acc: &typval_T, item: &typval_T| -> Option<typval_T> {
        CALL_FUNC_HOOK
            .with(|h| *h.borrow())
            .and_then(|f| f(expr, &[acc.clone(), item.clone()]))
    };
    let chars: Vec<char> = s.chars().collect();
    let start = if argvars.len() < 3 {
        if chars.is_empty() {
            emsg("E998: Reduce of an empty String with no initial value");
            return;
        }
        *rettv = typval_T {
            v_type: VAR_STRING,
            v_lock: crate::ported::eval::typval_defs_h::VarLockStatus::VAR_UNLOCKED,
            vval: v_string(chars[0].to_string()),
        };
        1
    } else {
        if tv_check_for_string_arg(argvars, 2) == FAIL {
            return;
        }
        *rettv = argvars[2].clone();
        0
    };
    for ch in chars.iter().skip(start) {
        let item = typval_T {
            v_type: VAR_STRING,
            v_lock: crate::ported::eval::typval_defs_h::VarLockStatus::VAR_UNLOCKED,
            vval: v_string(ch.to_string()),
        };
        match call(rettv, &item) {
            Some(r) => *rettv = r,
            None => return,
        }
    }
}

/// Port of `reduce_blob()` from `Src/eval/funcs.c` — fold over the bytes.
fn reduce_blob(argvars: &[typval_T], expr: &typval_T, rettv: &mut typval_T) {
    let b = match &argvars[0].vval {
        v_blob(Some(b)) => b.clone(),
        _ => return,
    };
    let call = |acc: &typval_T, item: &typval_T| -> Option<typval_T> {
        CALL_FUNC_HOOK
            .with(|h| *h.borrow())
            .and_then(|f| f(expr, &[acc.clone(), item.clone()]))
    };
    let len = tv_blob_len(&b.borrow());
    let start = if argvars.len() < 3 {
        if len == 0 {
            emsg("E998: Reduce of an empty Blob with no initial value");
            return;
        }
        rettv.v_type = VAR_NUMBER;
        rettv.vval = v_number(tv_blob_get(&b.borrow(), 0) as varnumber_T);
        1
    } else {
        if tv_check_for_number_arg(argvars, 2) == FAIL {
            return;
        }
        *rettv = argvars[2].clone();
        0
    };
    let mut i = start;
    while i < len {
        let item = typval_T {
            v_type: VAR_NUMBER,
            v_lock: crate::ported::eval::typval_defs_h::VarLockStatus::VAR_UNLOCKED,
            vval: v_number(tv_blob_get(&b.borrow(), i) as varnumber_T),
        };
        match call(rettv, &item) {
            Some(r) => *rettv = r,
            None => return,
        }
        i += 1;
    }
}

/// Port of `f_reduce()` from `Src/eval/funcs.c:5554`.
///
/// "reduce({object}, {func} [, {initial}])" — fold a List/String/Blob with
/// `{func}(acc, item)`.
pub fn f_reduce(argvars: &[typval_T], rettv: &mut typval_T) {
    if !matches!(argvars[0].v_type, VAR_STRING | VAR_LIST | VAR_BLOB) {
        emsg("E1098: String, List or Blob required");
        return;
    }
    // c: VAR_FUNC → v_string; VAR_PARTIAL → partial_name(partial); else tv_get_string.
    let func_name = match (argvars[1].v_type, &argvars[1].vval) {
        (VAR_PARTIAL, v_partial(Some(p))) => p.pt_name.clone(),
        _ => tv_get_string(&argvars[1]),
    };
    if func_name.is_empty() {
        emsg("E1132: Missing function argument");
        return;
    }
    match argvars[0].v_type {
        VAR_LIST => reduce_list(argvars, &argvars[1], rettv),
        VAR_STRING => reduce_string(argvars, &argvars[1], rettv),
        _ => reduce_blob(argvars, &argvars[1], rettv),
    }
}

/// Port of `f_dictwatcheradd()` from `Src/eval/funcs.c`.
///
/// "dictwatcheradd({dict}, {pattern}, {callback})" — register a callback fired
/// when a key matching `{pattern}` changes.
pub fn f_dictwatcheradd(argvars: &[typval_T], _rettv: &mut typval_T) {
    // c: check_secure() omitted.
    if argvars[0].v_type != VAR_DICT {
        emsg("E475: Invalid argument: dict");
        return;
    }
    let d = match &argvars[0].vval {
        v_dict(Some(d)) => d.clone(),
        _ => return, // c: NULL dict → readonly error
    };
    if argvars[1].v_type != VAR_STRING && argvars[1].v_type != VAR_NUMBER {
        emsg("E475: Invalid argument: key");
        return;
    }
    let key_pattern = match tv_get_string_chk(&argvars[1]) {
        Some(k) => k,
        None => return,
    };
    let mut callback = Callback::None;
    if !callback_from_typval(&mut callback, &argvars[2]) {
        emsg("E475: Invalid argument: funcref");
        return;
    }
    tv_dict_watcher_add(&d, &key_pattern, callback);
}

/// Port of `f_dictwatcherdel()` from `Src/eval/funcs.c`.
pub fn f_dictwatcherdel(argvars: &[typval_T], _rettv: &mut typval_T) {
    if argvars[0].v_type != VAR_DICT {
        emsg("E475: Invalid argument: dict");
        return;
    }
    if argvars[2].v_type != VAR_FUNC && argvars[2].v_type != VAR_STRING {
        emsg("E475: Invalid argument: funcref");
        return;
    }
    let key_pattern = match tv_get_string_chk(&argvars[1]) {
        Some(k) => k,
        None => return,
    };
    let mut callback = Callback::None;
    if !callback_from_typval(&mut callback, &argvars[2]) {
        return;
    }
    let d = match &argvars[0].vval {
        v_dict(Some(d)) => d.clone(),
        _ => return,
    };
    if !tv_dict_watcher_remove(&d, &key_pattern, &callback) {
        emsg("Couldn't find a watcher matching key and callback");
    }
}

/// Port of `f_strftime()` from `Src/eval/funcs.c:7220`.
///
/// "strftime({format} [, {time}])" function — format a time via the C library's
/// `strftime`. The locale-encoding conversion (`vimconv`) is omitted; the common
/// `CONV_NONE` (UTF-8) path is taken directly.
pub fn f_strftime(argvars: &[typval_T], rettv: &mut typval_T) {
    use nix::libc;
    rettv.v_type = VAR_STRING;
    let p = tv_get_string(&argvars[0]);
    // c: seconds = (argvars[1] == UNKNOWN) ? time(NULL) : (time_t)tv_get_number(&argvars[1]);
    let seconds: libc::time_t = if argvars.len() < 2 {
        unsafe { libc::time(std::ptr::null_mut()) }
    } else {
        tv_get_number(&argvars[1]) as libc::time_t
    };
    let curtime = match os_localtime_r(&seconds) {
        Some(t) => t,
        None => {
            // c: MSVC returns NULL for an invalid value of seconds.
            rettv.vval = v_string("(Invalid)".to_string());
            return;
        }
    };
    // c: strftime(result_buf, sizeof(result_buf), p, curtime_ptr)
    let fmt = match std::ffi::CString::new(p) {
        Ok(c) => c,
        Err(_) => {
            rettv.vval = v_string(String::new());
            return;
        }
    };
    let mut buf = [0u8; 256];
    let n = unsafe {
        libc::strftime(
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            fmt.as_ptr(),
            &curtime,
        )
    };
    // c: if strftime() == 0 → empty result.
    let s = if n == 0 {
        String::new()
    } else {
        String::from_utf8_lossy(&buf[..n]).into_owned()
    };
    rettv.vval = v_string(s);
}

/// Port of `f_strptime()` from `Src/eval/funcs.c:7270`.
///
/// "strptime({format}, {timestring})" function — parse a time string into a
/// seconds-since-epoch Number via the C library's `strptime` + `mktime`. The
/// locale-encoding conversion (`vimconv`) is omitted (UTF-8 / `CONV_NONE`).
pub fn f_strptime(argvars: &[typval_T], rettv: &mut typval_T) {
    use nix::libc;
    // c: struct tm tmval = { .tm_isdst = -1 };
    let mut tmval: libc::tm = unsafe { std::mem::zeroed() };
    tmval.tm_isdst = -1;
    let fmt = tv_get_string_buf(&argvars[0]);
    let str = tv_get_string_buf(&argvars[1]);
    // c: if fmt==NULL || os_strptime(...)==NULL || (rettv = mktime(&tmval))==-1 → 0
    let secs = if os_strptime(&str, &fmt, &mut tmval) {
        unsafe { libc::mktime(&mut tmval) }
    } else {
        -1
    };
    rettv.vval = v_number(if secs == -1 { 0 } else { secs as varnumber_T });
}

/// Port of `f_sha256()` from `Src/eval/funcs.c:6805`.
///
/// "sha256({string})" function — the SHA-256 hex digest of a String or Blob.
pub fn f_sha256(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    if argvars[0].v_type == VAR_BLOB {
        // c: hash the blob's bytes (empty if the blob is NULL).
        let bytes: Vec<u8> = match &argvars[0].vval {
            v_blob(Some(b)) => b.borrow().bv_ga.clone(),
            _ => Vec::new(),
        };
        rettv.vval = v_string(sha256_bytes(&bytes, None));
    } else {
        // c: p = tv_get_string(&argvars[0]); sha256_bytes(p, strlen(p), NULL, 0);
        let p = tv_get_string(&argvars[0]);
        rettv.vval = v_string(sha256_bytes(p.as_bytes(), None));
    }
}

/// Port of `init_srand()` from `Src/eval/funcs.c:4959`.
///
/// Seed the PRNG. `uv_random()` (the OS CSPRNG) is unavailable here, so this
/// uses Neovim's documented fallback: `os_hrtime()` XOR the process id.
fn init_srand(x: &mut u32) {
    // c: *x = (uint32_t)os_hrtime(); *x ^= (uint32_t)os_get_pid();
    *x = os_hrtime() as u32;
    *x ^= os_get_pid() as u32;
}

/// Port of `splitmix32()` from `Src/eval/funcs.c:4978`.
///
/// SplitMix32 step — advances `*x` and returns a well-mixed 32-bit value (used
/// to expand a single seed into the xoshiro state).
fn splitmix32(x: &mut u32) -> u32 {
    // c: uint32_t z = (*x += 0x9e3779b9);
    *x = x.wrapping_add(0x9e37_79b9);
    let mut z = *x;
    z = (z ^ (z >> 16)).wrapping_mul(0x85eb_ca6b);
    z = (z ^ (z >> 13)).wrapping_mul(0xc2b2_ae35);
    z ^ (z >> 16)
}

/// Port of `shuffle_xoshiro128starstar()` from `Src/eval/funcs.c:4987`.
///
/// xoshiro128** step — advances the 4-word state and returns the next value.
/// `ROTL(v, k)` is `v.rotate_left(k)`.
fn shuffle_xoshiro128starstar(x: &mut u32, y: &mut u32, z: &mut u32, w: &mut u32) -> u32 {
    // c: const uint32_t result = ROTL(*y * 5, 7) * 9;
    let result = y.wrapping_mul(5).rotate_left(7).wrapping_mul(9);
    let t = *y << 9;
    *z ^= *x;
    *w ^= *y;
    *y ^= *z;
    *x ^= *w;
    *z ^= t;
    *w = w.rotate_left(11);
    result
}

thread_local! {
    /// `f_rand()`'s global seed state (`static gx,gy,gz,gw` + `initialized`).
    static RAND_GLOBAL: std::cell::Cell<Option<[u32; 4]>> = const { std::cell::Cell::new(None) };
}

/// Port of `f_srand()` from `Src/eval/funcs.c:5075`.
///
/// "srand()" function — returns a 4-number seed list, from the given seed or a
/// fresh entropy seed.
pub fn f_srand(argvars: &[typval_T], rettv: &mut typval_T) {
    let mut x: u32 = 0;
    let l = tv_list_alloc_ret(rettv, 4);
    if argvars.is_empty() {
        init_srand(&mut x);
    } else {
        let mut error = false;
        x = tv_get_number_chk(&argvars[0], Some(&mut error)) as u32;
        if error {
            return;
        }
    }
    let mut lb = l.borrow_mut();
    for _ in 0..4 {
        let v = splitmix32(&mut x) as varnumber_T;
        tv_list_append_number(&mut lb, v);
    }
}

/// Port of `f_rand()` from `Src/eval/funcs.c:5005`.
///
/// "rand()" function — a 32-bit pseudo-random Number. With no argument it draws
/// from (and advances) a lazily-seeded global state; with a 4-number seed list
/// it advances that list in place (so repeated calls continue the sequence).
pub fn f_rand(argvars: &[typval_T], rettv: &mut typval_T) {
    let result: u32;
    if argvars.is_empty() {
        // c: use the global seed list, initializing it on first use.
        let mut s = RAND_GLOBAL.with(|c| c.get()).unwrap_or_else(|| {
            let mut x: u32 = 0;
            init_srand(&mut x);
            [
                splitmix32(&mut x),
                splitmix32(&mut x),
                splitmix32(&mut x),
                splitmix32(&mut x),
            ]
        });
        let [mut gx, mut gy, mut gz, mut gw] = s;
        result = shuffle_xoshiro128starstar(&mut gx, &mut gy, &mut gz, &mut gw);
        s = [gx, gy, gz, gw];
        RAND_GLOBAL.with(|c| c.set(Some(s)));
    } else if let (VAR_LIST, v_list(Some(l))) = (argvars[0].v_type, &argvars[0].vval) {
        let mut lb = l.borrow_mut();
        // c: list must have exactly 4 VAR_NUMBER items, else `goto theend`.
        let nums: Option<[u32; 4]> = if lb.lv_items.len() == 4 {
            let mut a = [0u32; 4];
            let mut ok = true;
            for (i, it) in lb.lv_items.iter().enumerate() {
                match (it.li_tv.v_type, &it.li_tv.vval) {
                    (VAR_NUMBER, v_number(n)) => a[i] = *n as u32,
                    _ => ok = false,
                }
            }
            ok.then_some(a)
        } else {
            None
        };
        match nums {
            Some([mut x, mut y, mut z, mut w]) => {
                result = shuffle_xoshiro128starstar(&mut x, &mut y, &mut z, &mut w);
                // c: write the advanced state back into the caller's list.
                lb.lv_items[0].li_tv.vval = v_number(x as varnumber_T);
                lb.lv_items[1].li_tv.vval = v_number(y as varnumber_T);
                lb.lv_items[2].li_tv.vval = v_number(z as varnumber_T);
                lb.lv_items[3].li_tv.vval = v_number(w as varnumber_T);
            }
            None => {
                // c: theend: semsg(_(e_invarg2), …) → E475; rettv = -1.
                drop(lb);
                emsg(&format!(
                    "E475: Invalid argument: {}",
                    tv_get_string(&argvars[0])
                ));
                rettv.v_type = VAR_NUMBER;
                rettv.vval = v_number(-1);
                return;
            }
        }
    } else {
        // c: theend: semsg(_(e_invarg2), …) → E475; rettv = -1.
        emsg(&format!(
            "E475: Invalid argument: {}",
            tv_get_string(&argvars[0])
        ));
        rettv.v_type = VAR_NUMBER;
        rettv.vval = v_number(-1);
        return;
    }
    rettv.v_type = VAR_NUMBER;
    rettv.vval = v_number(result as varnumber_T);
}

// ── registers (funcs.c, backed by the ops.c carve-out in `ported::ops`) ──────

/// Port of `getreg_get_regname()` from `Src/eval/funcs.c` — the register name
/// from `argvars[0]`, or `v:register` when omitted; `0`/empty → `"`.
fn getreg_get_regname(argvars: &[typval_T]) -> Option<char> {
    let s = if argvars.is_empty() {
        get_vim_var_str(VV_REG)
    } else {
        tv_get_string_chk(&argvars[0])?
    };
    let c = s.chars().next().unwrap_or('\0');
    Some(if c == '\0' { '"' } else { c })
}

/// Port of `f_getreg()` from `Src/eval/funcs.c` — a register's contents (String,
/// or List of lines when the 3rd arg is non-zero).
pub fn f_getreg(argvars: &[typval_T], rettv: &mut typval_T) {
    let regname = match getreg_get_regname(argvars) {
        Some(r) => r,
        None => return,
    };
    let return_list = argvars.len() > 2 && tv_get_number_chk(&argvars[2], None) != 0;
    let lines = get_reg_contents(regname).unwrap_or_default();
    if return_list {
        let l = tv_list_alloc_ret(rettv, 0);
        for line in lines {
            tv_list_append_string(&mut l.borrow_mut(), &line);
        }
    } else {
        // c: string form — lines joined by '\n', trailing '\n' if linewise.
        let mut s = lines.join("\n");
        if get_reg_type(regname).0 == MotionType::LineWise {
            s.push('\n');
        }
        *rettv = typval_T::from(s);
    }
}

/// Port of `f_getregtype()` from `Src/eval/funcs.c` — `v`/`V`/`<C-V>{w}`.
pub fn f_getregtype(argvars: &[typval_T], rettv: &mut typval_T) {
    let regname = match getreg_get_regname(argvars) {
        Some(r) => r,
        None => return,
    };
    let (t, len) = get_reg_type(regname);
    *rettv = typval_T::from(format_reg_type(t, len));
}

/// Port of `f_getreginfo()` from `Src/eval/funcs.c` — `{regcontents, regtype,
/// points_to|isunnamed}`.
pub fn f_getreginfo(argvars: &[typval_T], rettv: &mut typval_T) {
    let mut regname = match getreg_get_regname(argvars) {
        Some(r) => r,
        None => return,
    };
    if regname == '@' {
        regname = '"';
    }
    let d = tv_dict_alloc_ret(rettv);
    // c: get_reg_contents returns NULL for an unset register → empty dict.
    let lines = match get_reg_contents(regname) {
        Some(l) => l,
        None => return,
    };
    let lst = tv_list_alloc(0);
    for line in lines {
        tv_list_append_string(&mut lst.borrow_mut(), &line);
    }
    let mk = |t, v| typval_T {
        v_type: t,
        v_lock: crate::ported::eval::typval_defs_h::VarLockStatus::VAR_UNLOCKED,
        vval: v,
    };
    tv_dict_add_tv(
        &mut d.borrow_mut(),
        "regcontents",
        mk(VAR_LIST, v_list(Some(lst))),
    );
    let (t, len) = get_reg_type(regname);
    tv_dict_add_tv(
        &mut d.borrow_mut(),
        "regtype",
        typval_T::from(format_reg_type(t, len)),
    );
    if regname == '"' {
        tv_dict_add_tv(
            &mut d.borrow_mut(),
            "points_to",
            typval_T::from("\"".to_string()),
        );
    } else {
        tv_dict_add_tv(
            &mut d.borrow_mut(),
            "isunnamed",
            mk(VAR_BOOL, v_bool(kBoolVarFalse)),
        );
    }
}

/// Port of `f_setreg()` from `Src/eval/funcs.c` — store into a register. Options:
/// `a`/`A` append, `c`/`v` charwise, `l`/`V` linewise, `b`/`<C-V>` blockwise.
/// Returns 0 on success.
pub fn f_setreg(argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(1 as varnumber_T); // FAIL default
    let strreg = match tv_get_string_chk(&argvars[0]) {
        Some(s) => s,
        None => return,
    };
    let mut regname = strreg.chars().next().unwrap_or('\0');
    if regname == '\0' || regname == '@' {
        regname = '"';
    }

    // Resolve the contents typval and an optional dict-supplied type.
    let mut yank_type: Option<MotionType> = None;
    let contents: typval_T = if argvars[1].v_type == VAR_DICT {
        if let v_dict(Some(dd)) = &argvars[1].vval {
            let d = dd.borrow();
            if let Some(rt) = d.dv_hashtab.get("regtype") {
                let rt = tv_get_string(rt);
                if let Some(c) = rt.bytes().next() {
                    yank_type = get_yank_type(c, 0);
                }
            }
            d.dv_hashtab.get("regcontents").cloned().unwrap_or_default()
        } else {
            typval_T::default()
        }
    } else {
        argvars[1].clone()
    };

    let mut append = false;
    if argvars.len() > 2 {
        let opt = tv_get_string(&argvars[2]);
        for c in opt.bytes() {
            match c {
                b'a' | b'A' => append = true,
                b'u' | b'"' => {}
                _ => {
                    if let Some(t) = get_yank_type(c, 0) {
                        yank_type = Some(t);
                    }
                }
            }
        }
    }

    // Faithful f_setreg routing (funcs.c): a List value goes through the list
    // path (each element a line, no last-line append); a String value goes
    // through the str path (str_to_reg splits on '\n' AND continues the last
    // line when appending to a charwise register), so `setreg(r,x,'a')` joins.
    match (contents.v_type, &contents.vval) {
        (VAR_LIST, v_list(Some(l))) => {
            let lines: Vec<String> = l
                .borrow()
                .lv_items
                .iter()
                .map(|it| tv_get_string(&it.li_tv))
                .collect();
            write_reg_contents_lst(
                regname,
                lines,
                yank_type.unwrap_or(MotionType::LineWise),
                append,
            );
        }
        _ => {
            let sval = tv_get_string(&contents);
            // C passes kMTUnknown when no type option; str_to_reg then auto-detects
            // (trailing '\n' -> linewise, else charwise). Mirror that default.
            let mtype = yank_type.unwrap_or_else(|| {
                if sval.ends_with('\n') {
                    MotionType::LineWise
                } else {
                    MotionType::CharWise
                }
            });
            write_reg_contents(regname, &sval, mtype, append);
        }
    }
    *rettv = typval_T::from(0 as varnumber_T);
}

/// Port of `f_reg_recording()` from `Src/eval/funcs.c`. RUST-PORT NOTE: the
/// standalone interpreter records no macros → always "".
pub fn f_reg_recording(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(String::new());
}

/// Port of `f_reg_executing()` — no macro playback standalone → "".
pub fn f_reg_executing(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(String::new());
}

/// Port of `f_reg_recorded()` — no macro recording standalone → "".
pub fn f_reg_recorded(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(String::new());
}

// ── misc pure utilities (funcs.c) ───────────────────────────────────────────

/// Port of `f_gettext()` from `Src/eval/funcs.c`. RUST-PORT NOTE: no message
/// catalog is loaded, so translation is the identity (`_(s)` → `s`).
pub fn f_gettext(argvars: &[typval_T], rettv: &mut typval_T) {
    if tv_check_for_string_arg(argvars, 0) == FAIL {
        return;
    }
    *rettv = typval_T::from(tv_get_string(&argvars[0]));
}

/// Port of `f_garbagecollect()` from `Src/eval/funcs.c`. RUST-PORT NOTE: values
/// are `Rc`-managed (no mark-sweep collector) → a no-op.
pub fn f_garbagecollect(_argvars: &[typval_T], _rettv: &mut typval_T) {}

/// Port of `f_funcref()` from `Src/eval/funcs.c` — like `function()` but binds by
/// reference; in vimlrs it builds the same Partial as [`f_function`].
pub fn f_funcref(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: `common_function(argvars, rettv, true)` — the `is_funcref` flag is the
    // difference that matters: funcref() resolves through `find_func`, so it takes
    // a *user* function only and reports E700 for a builtin.
    common_function(argvars, rettv, true);
}

/// Port of `f_id()` from `Src/eval/funcs.c` — a unique id string for a container.
/// RUST-PORT NOTE: the `Rc` pointer address stands in for the C `%p` of the heap
/// object (unique per object, stable while it lives); scalars have no address →
/// the empty string.
pub fn f_id(argvars: &[typval_T], rettv: &mut typval_T) {
    let addr: usize = match &argvars[0].vval {
        v_list(Some(l)) => std::rc::Rc::as_ptr(l) as *const () as usize,
        v_dict(Some(d)) => std::rc::Rc::as_ptr(d) as *const () as usize,
        v_blob(Some(b)) => std::rc::Rc::as_ptr(b) as *const () as usize,
        v_partial(Some(p)) => std::rc::Rc::as_ptr(p) as *const () as usize,
        _ => 0,
    };
    *rettv = typval_T::from(if addr == 0 {
        String::new()
    } else {
        format!("{addr:#018x}")
    });
}

/// Port of `indexof_eval_expr()` — `vendor/eval/funcs.c:2983`. Evaluate `expr`
/// with `v:key`/`v:val` bound and return whether it is true.
///
/// RUST-PORT NOTE: the C reads the globals `VV_KEY`/`VV_VAL` and calls
/// `eval_expr_typval`; the value layer cannot evaluate expressions itself, so it
/// goes through `FILTER_MAP_EVAL_HOOK` (installed by the bridge), which sets
/// `v:key`/`v:val` and evaluates `expr` for us. An eval failure → false.
fn indexof_eval_expr(expr: &typval_T, key: &typval_T, val: &typval_T) -> bool {
    // c: if (eval_expr_typval(...) == FAIL) return false;
    //    found = tv_get_bool_chk(&newtv, &error); return error ? false : found;
    FILTER_MAP_EVAL_HOOK
        .with(|h| *h.borrow())
        .and_then(|f| f(expr, key, val))
        .map(|r| tv_get_bool(&r) != 0)
        .unwrap_or(false)
}

/// Port of `indexof_blob()` — `vendor/eval/funcs.c:3005`. The index of the first
/// byte at/after `startidx` for which `expr` is true, or -1.
fn indexof_blob(
    b: Option<&std::rc::Rc<std::cell::RefCell<blob_T>>>,
    startidx: varnumber_T,
    expr: &typval_T,
) -> varnumber_T {
    // c: if (b == NULL) return -1;
    let Some(b) = b else { return -1 };
    let bytes = b.borrow().bv_ga.clone();
    let blen = bytes.len() as varnumber_T;
    // c: negative index counts from the last byte, clamped to 0.
    let mut idx = startidx;
    if idx < 0 {
        idx += blen;
        if idx < 0 {
            idx = 0;
        }
    }
    // c: for (; idx < tv_blob_len(b); idx++) { VV_KEY=idx; VV_VAL=byte; ... }
    while idx < blen {
        let key = typval_T::from(idx);
        let val = typval_T::from(bytes[idx as usize] as varnumber_T);
        if indexof_eval_expr(expr, &key, &val) {
            return idx;
        }
        idx += 1;
    }
    -1
}

/// Port of `indexof_list()` — `vendor/eval/funcs.c:3042`. The index of the first
/// item at/after `startidx` for which `expr` is true, or -1.
fn indexof_list(
    l: Option<&std::rc::Rc<std::cell::RefCell<list_T>>>,
    startidx: varnumber_T,
    expr: &typval_T,
) -> varnumber_T {
    // c: if (l == NULL) return -1;
    let Some(l) = l else { return -1 };
    let items: Vec<typval_T> = l
        .borrow()
        .lv_items
        .iter()
        .map(|it| it.li_tv.clone())
        .collect();
    let len = items.len() as varnumber_T;
    // c: startidx==0 → first item; else idx = tv_list_uidx(l, startidx) — a user
    // index (negative from the end) that is -1 (no item) when out of range.
    let start = if startidx == 0 {
        0
    } else {
        let s = if startidx < 0 {
            startidx + len
        } else {
            startidx
        };
        if s < 0 || s >= len {
            return -1;
        }
        s
    };
    // c: for (; item != NULL; item = next, idx++) { VV_KEY=idx; VV_VAL=item; ... }
    let mut idx = start;
    while idx < len {
        let key = typval_T::from(idx);
        if indexof_eval_expr(expr, &key, &items[idx as usize]) {
            return idx;
        }
        idx += 1;
    }
    -1
}

/// Port of `f_indexof()` — `vendor/eval/funcs.c:3086`. The index of the first
/// List/Blob item for which `{expr}` (string or funcref, seeing `v:key`/`v:val`)
/// is true, or -1. An optional `{opts}` Dict's `startidx` begins the scan later.
pub fn f_indexof(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: rettv->vval.v_number = -1;
    *rettv = typval_T::from(-1 as varnumber_T);
    let expr = &argvars[1];
    // c: empty string / NULL funcref expr → nothing to test.
    if expr.v_type == VAR_STRING && tv_get_string(expr).is_empty() {
        return;
    }
    // c: startidx = (argvars[2] is Dict) ? tv_dict_get_number_def(d,"startidx",0) : 0;
    let startidx = match argvars.get(2) {
        Some(d) if d.v_type == VAR_DICT => match &d.vval {
            v_dict(Some(dd)) => dd
                .borrow()
                .dv_hashtab
                .get("startidx")
                .map(tv_get_number)
                .unwrap_or(0),
            _ => 0,
        },
        _ => 0,
    };
    let found = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(l)) => indexof_list(l.as_ref(), startidx, expr),
        (VAR_BLOB, v_blob(b)) => indexof_blob(b.as_ref(), startidx, expr),
        _ => -1,
    };
    *rettv = typval_T::from(found);
}

// ── pattern / option / editor-absent builtins (funcs.c) ──────────────────────

/// Port of `get_matches_in_str()` — `vendor/eval/funcs.c:4272`. Append a dict for
/// **every** match of `pat` in `s` to `mlist`: `{idx|lnum, byteidx, text
/// [, submatches]}`. `matchbuf` selects the `lnum` key (`matchbufline`) over
/// `idx` (`matchstrlist`); `submatches` adds the `\1`..`\9` group list.
///
/// RUST-PORT NOTE: positions come from the char-indexed regex engine; `byteidx`
/// is the byte offset of the match start, as Vim reports.
fn get_matches_in_str(
    s: &str,
    pat: &str,
    ic: bool,
    mlist: &std::rc::Rc<std::cell::RefCell<list_T>>,
    idx: varnumber_T,
    submatches: bool,
    matchbuf: bool,
) {
    use crate::ported::eval::typval_defs_h::VarLockStatus::VAR_UNLOCKED;
    let chars: Vec<char> = s.chars().collect();
    let mut from = 0usize;
    // c: while ((match = vim_regexec_nl(rmp, str, startidx))) { ... startidx = endp; }
    while let Some((cstart, cend, groups)) =
        crate::viml_regex::regex_search_nth(pat, s, ic, from, 1)
    {
        // c: byteidx = startp[0] - str (a BYTE offset).
        let byteidx: usize = chars[..cstart as usize].iter().map(|c| c.len_utf8()).sum();
        let d = tv_dict_alloc();
        {
            let mut db = d.borrow_mut();
            // c: matchbuf ? "lnum" : "idx".
            tv_dict_add_tv(
                &mut db,
                if matchbuf { "lnum" } else { "idx" },
                typval_T::from(idx),
            );
            tv_dict_add_tv(&mut db, "byteidx", typval_T::from(byteidx as varnumber_T));
            tv_dict_add_tv(&mut db, "text", typval_T::from(groups[0].clone()));
            if submatches {
                // c: the 9 \1..\9 backrefs, "" for groups that did not match.
                let sub = tv_list_alloc(0);
                {
                    let mut sb = sub.borrow_mut();
                    for g in groups.iter().take(10).skip(1) {
                        tv_list_append_string(&mut sb, g);
                    }
                }
                tv_dict_add_tv(
                    &mut db,
                    "submatches",
                    typval_T {
                        v_type: VAR_LIST,
                        v_lock: VAR_UNLOCKED,
                        vval: v_list(Some(sub)),
                    },
                );
            }
        }
        tv_list_append_tv(
            &mut mlist.borrow_mut(),
            typval_T {
                v_type: VAR_DICT,
                v_lock: VAR_UNLOCKED,
                vval: v_dict(Some(d)),
            },
        );
        // c: startidx = endp[0] - str; stop on end/zero-width to avoid a stall.
        from = if cend > cstart {
            cend as usize
        } else {
            cstart as usize + 1
        };
        if from > chars.len() {
            break;
        }
    }
}

/// Port of `f_matchstrlist()` from `Src/eval/funcs.c` — for each String in a
/// List, **every** match of `{pat}`: `{idx, byteidx, text [, submatches]}`.
pub fn f_matchstrlist(argvars: &[typval_T], rettv: &mut typval_T) {
    let l = tv_list_alloc_ret(rettv, 0);
    let list = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => l.clone(),
        (VAR_LIST, v_list(None)) => return,
        _ => {
            emsg("E1211: List required");
            return;
        }
    };
    let pat = tv_get_string(&argvars[1]);
    let ic = tv_get_number(&get_option_value("ignorecase")) != 0;
    let submatches = argvars.get(2).is_some_and(|d| match &d.vval {
        v_dict(Some(dd)) => {
            dd.borrow()
                .dv_hashtab
                .get("submatches")
                .map(tv_get_bool)
                .unwrap_or(0)
                != 0
        }
        _ => false,
    });
    let items: Vec<typval_T> = list
        .borrow()
        .lv_items
        .iter()
        .map(|it| it.li_tv.clone())
        .collect();
    for (idx, item) in items.iter().enumerate() {
        let s = tv_get_string(item);
        get_matches_in_str(&s, &pat, ic, &l, idx as varnumber_T, submatches, false);
    }
}

/// Port of `f_fnameescape()` from `Src/eval/funcs.c` — backslash-escape the
/// characters special to a `:` command's filename argument.
pub fn f_fnameescape(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: PATH_ESC_CHARS " \t\n*?[{`$\\%#'\"|!<".
    const ESC: &[u8] = b" \t\n*?[{`$\\%#'\"|!<";
    let name = tv_get_string(&argvars[0]);
    let mut out = String::with_capacity(name.len() + 2);
    // Walk *characters*, not bytes: every escapable char is ASCII, and pushing a
    // raw UTF-8 byte as a `char` reinterpreted it as Latin-1 — `fnameescape('ünï…')`
    // came back as `'Ã¼nÃ¯…'`.
    for (i, c) in name.chars().enumerate() {
        // A leading '+' or '>' is also escaped (would start a different arg).
        if (c.is_ascii() && ESC.contains(&(c as u8))) || (i == 0 && (c == '+' || c == '>')) {
            out.push('\\');
        }
        out.push(c);
    }
    *rettv = typval_T::from(out);
}

/// Port of `f_shiftwidth()`/`get_sw_value()` from `Src/eval/funcs.c` —
/// `'shiftwidth'`, or `'tabstop'` when shiftwidth is 0.
pub fn f_shiftwidth(_argvars: &[typval_T], rettv: &mut typval_T) {
    let mut sw = tv_get_number(&get_option_value("shiftwidth"));
    if sw == 0 {
        sw = tv_get_number(&get_option_value("tabstop"));
    }
    *rettv = typval_T::from(sw);
}

// Editor/GUI/server-absent builtins: a standalone VimL interpreter has no
// editor UI, so these report the fixed "nothing here" value Vim returns when
// the corresponding subsystem is inactive.

/// Port of `f_mode()` — normal mode (the standalone interpreter has no modes).
pub fn f_mode(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from("n".to_string());
}
/// Port of `f_state()` — no pending state standalone → "".
pub fn f_state(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(String::new());
}
/// Port of `f_visualmode()` — no prior Visual selection standalone → "".
pub fn f_visualmode(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(String::new());
}
/// Port of `f_pumvisible()` — no popup menu → 0.
pub fn f_pumvisible(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_wildmenumode()` — no wildmenu → 0.
pub fn f_wildmenumode(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_did_filetype()` — no filetype autocommands → 0.
pub fn f_did_filetype(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_eventhandler()` — not inside an event handler → 0.
pub fn f_eventhandler(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_hlexists()` — whether highlight group `{name}` exists. Standalone
/// there is no UI, but a sourced colorscheme/vimrc defines groups via
/// `:highlight`; those are tracked in the highlight registry (see [`HL_GROUPS`]).
pub fn f_hlexists(argvars: &[typval_T], rettv: &mut typval_T) {
    let name = argvars.first().map(tv_get_string).unwrap_or_default();
    *rettv = typval_T::from(hl_exists(&name) as varnumber_T);
}
/// Port of `f_windowsversion()` — non-Windows → "".
pub fn f_windowsversion(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(String::new());
}
/// Port of `f_getfontname()` — no GUI → "".
pub fn f_getfontname(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(String::new());
}
/// Port of `f_foreground()` — no UI to raise → 0 (no-op).
pub fn f_foreground(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_prompt_getprompt()` — no prompt buffer → "".
pub fn f_prompt_getprompt(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(String::new());
}
/// Port of `f_pum_getpos()` — no popup menu → empty Dict.
pub fn f_pum_getpos(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_dict_alloc_ret(rettv);
}
/// Port of `f_serverlist()` — no server standalone → empty List.
pub fn f_serverlist(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_list_alloc_ret(rettv, 0);
}

// ── Editor-position / screen / search builtins (no buffer or UI standalone) ──
//
// A standalone VimL interpreter has no current buffer, window, cursor, or screen
// grid, so the editor-coupled C bodies (`getpos_both`, `get_col`,
// `search_cmn`, `ui_current_row`, `ml_find_line_or_offset`, …) reduce to the
// fixed "nothing here" values their C returns when there is no match / no
// buffer line / off-grid: a cursor list reads as all-zero, a search reports
// "not found", an off-grid screen cell is -1. The list/dict *shapes* are kept
// faithful to the C so callers that index `[lnum, col]` still work.

/// Port of `getpos_both()` from `Src/eval/funcs.c` — the `[bufnum, lnum, col,
/// off]` (plus `curswant` when `getcurpos`) position list. Standalone has no
/// cursor (`fp == NULL`, `fnum == -1`), so every `tv_list_append_number` takes
/// its NULL branch and the list is all zeros.
fn getpos_both(rettv: &mut typval_T, getcurpos: bool) {
    let len = 4 + getcurpos as isize;
    let l = tv_list_alloc_ret(rettv, len);
    let mut lb = l.borrow_mut();
    for _ in 0..len {
        tv_list_append_number(&mut lb, 0);
    }
}

// ── In-memory current buffer (Neovim buffer.c / memline.c). vimlrs runs
//    standalone, but text functions operate on a single virtual buffer that
//    scripts populate with setline()/append(). The buffer always has >= 1 line
//    (an empty buffer is `[""]`), like Vim. ──

thread_local! {
    /// The current buffer's lines (`curbuf->b_ml` in memline.c), 1-based when
    /// addressed. Always non-empty.
    static CURBUF: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// The cursor `(lnum, col, curswant)` (`curwin->w_cursor`), 1-based.
    static CURPOS: std::cell::RefCell<(varnumber_T, varnumber_T, varnumber_T)> =
        const { std::cell::RefCell::new((1, 1, 1)) };
}

/// Number of lines in the current buffer (minimum 1: an unset buffer reads as a
/// single empty line, like Vim).
fn curbuf_len() -> varnumber_T {
    if let Some(n) = crate::fusevm_bridge::editor_line_count() {
        return n.max(1);
    }
    CURBUF.with(|b| b.borrow().len().max(1) as varnumber_T)
}

/// Port of `tv_get_lnum()` (Neovim eval/typval.c) — resolve a line-number
/// argument: a Number, or a String like `.` (cursor), `$` (last line), `w0`/
/// `w$` (window top/bottom = first/last here), or a `'m` mark (0, no marks).
fn tv_get_lnum(tv: &typval_T) -> varnumber_T {
    if tv.v_type == VAR_STRING {
        let s = tv_get_string(tv);
        match s.as_str() {
            "." => crate::fusevm_bridge::editor_curpos(CURPOS.with(|c| *c.borrow())).0,
            "$" | "w$" => curbuf_len(),
            "w0" => 1,
            _ if s.starts_with('\'') => {
                // c: a `'m` mark address resolves to the mark's line (0 if unset).
                s.chars().nth(1).and_then(getmark).map_or(0, |(l, _)| l)
            }
            _ => s.parse().unwrap_or(0),
        }
    } else {
        tv_get_number(tv)
    }
}

thread_local! {
    /// The mark table (`namedfm[]` / `curbuf->b_namedm[]` in mark.c): mark name
    /// → `(lnum, col)`, both 1-based.
    static MARKS: std::cell::RefCell<std::collections::BTreeMap<char, (varnumber_T, varnumber_T)>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
}

/// Port of `setmark()` (Neovim mark.c) — set mark `name` to `(lnum, col)`.
fn setmark(name: char, lnum: varnumber_T, col: varnumber_T) {
    MARKS.with(|m| {
        m.borrow_mut().insert(name, (lnum, col));
    });
}

/// Port of `getmark()` (Neovim mark.c) — the `(lnum, col)` of mark `name`, or
/// `None` if it is not set.
fn getmark(name: char) -> Option<(varnumber_T, varnumber_T)> {
    MARKS.with(|m| m.borrow().get(&name).copied())
}

/// Port of `get_buffer_lines()` (Neovim buffer.c) — the current buffer's lines
/// from `start` to `end` (1-based, inclusive, clamped to the buffer).
fn get_buffer_lines(start: varnumber_T, end: varnumber_T) -> Vec<String> {
    if let Some(v) = crate::fusevm_bridge::editor_get_lines(start, end) {
        return v;
    }
    CURBUF.with(|b| {
        let b = b.borrow();
        // An unset buffer reads as a single empty line.
        if b.is_empty() {
            return if start <= 1 && end >= 1 {
                vec![String::new()]
            } else {
                Vec::new()
            };
        }
        let len = b.len() as varnumber_T;
        let (s, e) = (start.max(1), end.min(len));
        if s > e {
            return Vec::new();
        }
        (s..=e).map(|i| b[(i - 1) as usize].clone()).collect()
    })
}

/// Port of `set_buffer_lines()` (Neovim buffer.c) — replace (`append == false`)
/// the lines from `lnum`, or insert them after line `lnum` (`append == true`,
/// `lnum == 0` inserts before the first line). Returns 0 on success, 1 on
/// failure (an out-of-range replace).
fn set_buffer_lines(lnum: varnumber_T, lines: Vec<String>, append: bool) -> varnumber_T {
    if crate::fusevm_bridge::editor_host_active() {
        return crate::fusevm_bridge::editor_set_lines(lnum, lines, append).unwrap_or(1);
    }
    CURBUF.with(|b| {
        let mut b = b.borrow_mut();
        if b.is_empty() {
            b.push(String::new());
        }
        if append {
            let pos = (lnum.max(0) as usize).min(b.len());
            for (i, l) in lines.into_iter().enumerate() {
                b.insert(pos + i, l);
            }
            0
        } else {
            if lnum < 1 {
                return 1;
            }
            for (i, l) in lines.into_iter().enumerate() {
                let idx = (lnum - 1) as usize + i;
                if idx < b.len() {
                    b[idx] = l;
                } else if idx == b.len() {
                    b.push(l);
                } else {
                    break;
                }
            }
            0
        }
    })
}

/// Collect a String-or-List `{text}` argument into a vector of lines.
fn tv_lines_arg(tv: &typval_T) -> Vec<String> {
    match (tv.v_type, &tv.vval) {
        (VAR_LIST, v_list(Some(l))) => l
            .borrow()
            .lv_items
            .iter()
            .map(|it| tv_get_string(&it.li_tv))
            .collect(),
        _ => vec![tv_get_string(tv)],
    }
}

/// Port of `set_cursorpos()` (Neovim eval/funcs.c) — move the cursor to `lnum`,
/// `col`, clamped to the current buffer (line 1..=last, column 1..=len+1).
pub fn set_cursorpos(lnum: varnumber_T, col: varnumber_T) {
    let len = curbuf_len();
    let l = lnum.clamp(1, len);
    let linelen = get_buffer_lines(l, l)
        .first()
        .map_or(0, |s| s.len() as varnumber_T);
    let c = col.clamp(1, linelen + 1);
    if crate::fusevm_bridge::editor_set_cursor(l, c) {
        return;
    }
    CURPOS.with(|p| {
        let mut p = p.borrow_mut();
        *p = (l, c, c);
    });
}

/// Port of `f_getpos()`/`getpos_both(…,false,false)` — the position of `{expr}`
/// as `[bufnum, lnum, col, off]`; `.` is the cursor, `'m` a mark.
pub fn f_getpos(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let (lnum, col) = if s == "." {
        CURPOS.with(|c| {
            let c = c.borrow();
            (c.0, c.1)
        })
    } else if let Some(name) = s.strip_prefix('\'').and_then(|r| r.chars().next()) {
        getmark(name).unwrap_or((0, 0))
    } else {
        (tv_get_lnum(&argvars[0]), 0)
    };
    let l = tv_list_alloc_ret(rettv, 4);
    let mut lb = l.borrow_mut();
    tv_list_append_number(&mut lb, 0);
    tv_list_append_number(&mut lb, lnum);
    tv_list_append_number(&mut lb, col);
    tv_list_append_number(&mut lb, 0);
}

/// Port of `f_getpos_unused()` — retained stub entry point.
/// Port of `f_getcharpos()` — like `getpos()` but the column is a character
/// index. Here the buffer is byte==char for the common ASCII case.
pub fn f_getcharpos(argvars: &[typval_T], rettv: &mut typval_T) {
    f_getpos(argvars, rettv);
}
/// Port of `f_getcurpos()`/`getpos_both(…,true,false)` — the cursor position as
/// `[bufnum, lnum, col, off, curswant]`.
pub fn f_getcurpos(_argvars: &[typval_T], rettv: &mut typval_T) {
    let (lnum, col, curswant) = crate::fusevm_bridge::editor_curpos(CURPOS.with(|c| *c.borrow()));
    let l = tv_list_alloc_ret(rettv, 5);
    let mut lb = l.borrow_mut();
    tv_list_append_number(&mut lb, 0);
    tv_list_append_number(&mut lb, lnum);
    tv_list_append_number(&mut lb, col);
    tv_list_append_number(&mut lb, 0);
    tv_list_append_number(&mut lb, curswant);
}
/// Port of `f_getcursorcharpos()`/`getpos_both(…,true,true)` — `[0,0,0,0,0]`.
pub fn f_getcursorcharpos(_argvars: &[typval_T], rettv: &mut typval_T) {
    getpos_both(rettv, true);
}
/// Port of `f_col()`/`get_col(…,false)` — the byte column of `{expr}`: `.` is
/// the cursor column, `$` is one past the end of the cursor's line.
pub fn f_col(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let (lnum, ccol) = {
        let __p = crate::fusevm_bridge::editor_curpos(CURPOS.with(|c| *c.borrow()));
        (__p.0, __p.1)
    };
    let col = match s.as_str() {
        "." => ccol,
        "$" => get_buffer_lines(lnum, lnum)
            .first()
            .map_or(1, |l| l.len() as varnumber_T + 1),
        _ if s.starts_with('\'') => s.chars().nth(1).and_then(getmark).map_or(0, |(_, c)| c),
        _ => 0,
    };
    *rettv = typval_T::from(col);
}
/// Port of `f_charcol()`/`get_col(…,true)` — like `col()` but a character index.
pub fn f_charcol(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let (lnum, ccol) = {
        let __p = crate::fusevm_bridge::editor_curpos(CURPOS.with(|c| *c.borrow()));
        (__p.0, __p.1)
    };
    let col = match s.as_str() {
        "." => ccol,
        "$" => get_buffer_lines(lnum, lnum)
            .first()
            .map_or(1, |l| l.chars().count() as varnumber_T + 1),
        _ => 0,
    };
    *rettv = typval_T::from(col);
}
/// Port of `f_line()` — the line number of `{expr}` (`.` cursor, `$` last line).
pub fn f_line(argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(tv_get_lnum(&argvars[0]));
}
/// Port of `f_virtcol()` — no cursor → 0, or `[0,0]` when the second arg
/// (`list`) is truthy, matching the C `theend:` branch.
pub fn f_virtcol(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let want_list = argvars.len() > 1 && tv_get_bool(&argvars[1]) != 0;
    let (lnum, ccol) = {
        let __p = crate::fusevm_bridge::editor_curpos(CURPOS.with(|c| *c.borrow()));
        (__p.0, __p.1)
    };
    let line = get_buffer_lines(lnum, lnum)
        .into_iter()
        .next()
        .unwrap_or_default();
    let ts = {
        let t = tv_get_number_chk(&get_option_value("tabstop"), None);
        if t > 0 {
            t as varnumber_T
        } else {
            8
        }
    };
    // The byte index of the target character (0-based); `$` is one past the end.
    let dollar = s == "$";
    let target_byte = if dollar {
        line.len()
    } else if s == "." {
        (ccol as usize).saturating_sub(1)
    } else if let Some(name) = s.strip_prefix('\'').and_then(|r| r.chars().next()) {
        (getmark(name).map_or(1, |(_, c)| c) as usize).saturating_sub(1)
    } else {
        (ccol as usize).saturating_sub(1)
    };
    let mut vcol: varnumber_T = 0;
    for (bi, c) in line.char_indices() {
        let w = if c == '\t' { ts - (vcol % ts) } else { 1 };
        vcol += w;
        if !dollar && bi >= target_byte {
            break;
        }
    }
    if dollar {
        vcol += 1;
    }
    if want_list {
        let l = tv_list_alloc_ret(rettv, 2);
        let mut lb = l.borrow_mut();
        tv_list_append_number(&mut lb, vcol);
        tv_list_append_number(&mut lb, vcol);
    } else {
        *rettv = typval_T::from(vcol);
    }
}
/// Port of `f_screenrow()`/`ui_current_row()` — no UI grid → 0.
pub fn f_screenrow(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_screencol()`/`ui_current_col()` — no UI grid → 0.
pub fn f_screencol(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_screenchar()` — off-grid (there is no grid) → -1.
pub fn f_screenchar(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
}
/// Port of `f_screenattr()` — off-grid → -1.
pub fn f_screenattr(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
}
/// Port of `f_screenchars()` — off-grid early `return` → empty List.
pub fn f_screenchars(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_list_alloc_ret(rettv, 0);
}
/// Port of `f_screenstring()` — off-grid → "" (empty cell string).
pub fn f_screenstring(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(String::new());
}
/// Port of `f_line2byte()` — the byte offset of the first character of line
/// `{lnum}` (1-based; each line counts its bytes plus one for the newline), or
/// -1 if out of range. `lnum == last+1` gives the buffer's total byte size + 1.
pub fn f_line2byte(argvars: &[typval_T], rettv: &mut typval_T) {
    let lnum = tv_get_lnum(&argvars[0]);
    let len = curbuf_len();
    if lnum < 1 || lnum > len + 1 {
        *rettv = typval_T::from(-1 as varnumber_T);
        return;
    }
    let mut off: varnumber_T = 1;
    for l in get_buffer_lines(1, lnum - 1) {
        off += l.len() as varnumber_T + 1;
    }
    *rettv = typval_T::from(off);
}
/// Port of `f_byte2line()` — the line number containing byte `{byte}` (the
/// inverse of `line2byte()`), or -1 if out of range.
pub fn f_byte2line(argvars: &[typval_T], rettv: &mut typval_T) {
    let target = tv_get_number(&argvars[0]);
    if target < 1 {
        *rettv = typval_T::from(-1 as varnumber_T);
        return;
    }
    let mut off: varnumber_T = 1;
    let mut lnum: varnumber_T = 0;
    for l in get_buffer_lines(1, curbuf_len()) {
        lnum += 1;
        off += l.len() as varnumber_T + 1;
        if target < off {
            *rettv = typval_T::from(lnum);
            return;
        }
    }
    *rettv = typval_T::from(-1 as varnumber_T);
}
/// Port of `f_nextnonblank()` — the first non-blank line at or after `{lnum}`,
/// or 0 if there is none.
pub fn f_nextnonblank(argvars: &[typval_T], rettv: &mut typval_T) {
    let mut lnum = tv_get_lnum(&argvars[0]).max(1);
    let len = curbuf_len();
    while lnum <= len {
        if !get_buffer_lines(lnum, lnum)[0].trim().is_empty() {
            *rettv = typval_T::from(lnum);
            return;
        }
        lnum += 1;
    }
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_prevnonblank()` — the first non-blank line at or before `{lnum}`,
/// or 0 if there is none.
pub fn f_prevnonblank(argvars: &[typval_T], rettv: &mut typval_T) {
    let mut lnum = tv_get_lnum(&argvars[0]).min(curbuf_len());
    while lnum >= 1 {
        if !get_buffer_lines(lnum, lnum)[0].trim().is_empty() {
            *rettv = typval_T::from(lnum);
            return;
        }
        lnum -= 1;
    }
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_wordcount()`/`cursor_pos_info()` — empty buffer → every count 0.
pub fn f_wordcount(_argvars: &[typval_T], rettv: &mut typval_T) {
    let lines = get_buffer_lines(1, curbuf_len());
    let (clnum, ccol) = {
        let __p = crate::fusevm_bridge::editor_curpos(CURPOS.with(|c| *c.borrow()));
        (__p.0, __p.1)
    };
    let (mut bytes, mut chars, mut words) = (0i64, 0i64, 0i64);
    let (mut cbytes, mut cchars, mut cwords) = (0i64, 0i64, 0i64);
    for (i, line) in lines.iter().enumerate() {
        let lnum = i as varnumber_T + 1;
        let lwords = line.split_whitespace().count() as i64;
        words += lwords;
        // Up to the cursor: count whole lines before it, then a partial line.
        if lnum < clnum {
            cbytes += line.len() as i64 + 1;
            cchars += line.chars().count() as i64 + 1;
            cwords += lwords;
        } else if lnum == clnum {
            let upto = &line[..((ccol as usize).saturating_sub(1)).min(line.len())];
            cbytes += upto.len() as i64;
            cchars += upto.chars().count() as i64;
            cwords += upto.split_whitespace().count() as i64;
        }
        bytes += line.len() as i64 + 1;
        chars += line.chars().count() as i64 + 1;
    }
    let d = tv_dict_alloc_ret(rettv);
    let mut db = d.borrow_mut();
    tv_dict_add_nr(&mut db, "bytes", bytes);
    tv_dict_add_nr(&mut db, "chars", chars);
    tv_dict_add_nr(&mut db, "words", words);
    tv_dict_add_nr(&mut db, "cursor_bytes", cbytes);
    tv_dict_add_nr(&mut db, "cursor_chars", cchars);
    tv_dict_add_nr(&mut db, "cursor_words", cwords);
}

/// Port of `f_getjumplist()` — no window → `[[], 0]` (entries, current index).
pub fn f_getjumplist(_argvars: &[typval_T], rettv: &mut typval_T) {
    let l = tv_list_alloc_ret(rettv, 2);
    let inner = tv_list_alloc(0);
    let mut lb = l.borrow_mut();
    tv_list_append_list(&mut lb, inner);
    tv_list_append_number(&mut lb, 0);
}
/// Port of `f_getchangelist()` — no buffer → `[[], 0]`.
pub fn f_getchangelist(_argvars: &[typval_T], rettv: &mut typval_T) {
    let l = tv_list_alloc_ret(rettv, 2);
    let inner = tv_list_alloc(0);
    let mut lb = l.borrow_mut();
    tv_list_append_list(&mut lb, inner);
    tv_list_append_number(&mut lb, 0);
}
/// Port of `f_getmarklist()` — no marks → empty List.
pub fn f_getmarklist(_argvars: &[typval_T], rettv: &mut typval_T) {
    let marks: Vec<(char, (varnumber_T, varnumber_T))> =
        MARKS.with(|m| m.borrow().iter().map(|(k, v)| (*k, *v)).collect());
    let out = tv_list_alloc_ret(rettv, marks.len() as isize);
    let mut ob = out.borrow_mut();
    for (name, (lnum, col)) in marks {
        let d = tv_dict_alloc();
        {
            let mut db = d.borrow_mut();
            tv_dict_add_str(&mut db, "mark", &format!("'{name}"));
            let pos = tv_list_alloc(4);
            {
                let mut pb = pos.borrow_mut();
                tv_list_append_number(&mut pb, 0);
                tv_list_append_number(&mut pb, lnum);
                tv_list_append_number(&mut pb, col);
                tv_list_append_number(&mut pb, 0);
            }
            tv_dict_add_tv(
                &mut db,
                "pos",
                typval_T {
                    v_type: VAR_LIST,
                    v_lock: VarLockStatus::VAR_UNLOCKED,
                    vval: v_list(Some(pos)),
                },
            );
        }
        tv_list_append_tv(&mut ob, match_dict_val(d));
    }
}
/// Port of `f_gettagstack()`/`get_tagstack()` — empty stack → `{items:[],
/// length:0, curidx:0}`.
pub fn f_gettagstack(_argvars: &[typval_T], rettv: &mut typval_T) {
    let d = tv_dict_alloc_ret(rettv);
    let mut db = d.borrow_mut();
    tv_dict_add_list(&mut db, "items", tv_list_alloc(0));
    tv_dict_add_nr(&mut db, "length", 0);
    tv_dict_add_nr(&mut db, "curidx", 0);
}
/// Port of `f_tagfiles()` — no `'tags'` files → empty List.
pub fn f_tagfiles(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_list_alloc_ret(rettv, 0);
}
/// Port of `f_taglist()` — empty pattern → 0 (`false`), else no tags → empty List.
pub fn f_taglist(argvars: &[typval_T], rettv: &mut typval_T) {
    if tv_get_string(&argvars[0]).is_empty() {
        *rettv = typval_T::from(0 as varnumber_T);
        return;
    }
    tv_list_alloc_ret(rettv, 0);
}
/// Port of `f_tabpagebuflist()` — no windows → 0 (rettv left a Number in C).
pub fn f_tabpagebuflist(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}

thread_local! {
    /// The last search pattern (`@/` / `spats[0].pat` in search.c), set by
    /// `search()`/`searchpos()` and read by `searchcount()`.
    static LAST_SEARCH: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// Find the first byte-column match of `pat` in `line` at or after byte offset
/// `from`, as `(start, end)` byte offsets (end exclusive).
fn line_match_from(pat: &str, line: &str, from: usize, ic: bool) -> Option<(usize, usize)> {
    if from > line.len() {
        return None;
    }
    let (_, s, e) = crate::viml_regex::regex_matchstrpos(pat, &line[from..], ic);
    if s < 0 {
        None
    } else {
        Some((from + s as usize, from + e as usize))
    }
}

/// Port of `searchit()` (Neovim search.c) — search the current buffer for `pat`
/// from the cursor and (unless the `n` flag is given) move the cursor to the
/// match. Flags: `b` backward, `n` no-move, `c` accept a match at the cursor,
/// `e` move to the end of the match, `w`/`W` force/forbid wrap-around (default
/// wraps). Returns the 1-based `(lnum, col)` of the match, or `None`.
fn searchit(pat: &str, flags: &str, _stopline: varnumber_T) -> Option<(varnumber_T, varnumber_T)> {
    LAST_SEARCH.with(|p| *p.borrow_mut() = pat.to_string());
    let backward = flags.contains('b');
    let nomove = flags.contains('n');
    let accept = flags.contains('c');
    let want_end = flags.contains('e');
    let wrap = !flags.contains('W');
    let ic = tv_get_number_chk(&get_option_value("ignorecase"), None) != 0;
    let (clnum, ccol) = {
        let __p = crate::fusevm_bridge::editor_curpos(CURPOS.with(|c| *c.borrow()));
        (__p.0, __p.1)
    };
    let lines = get_buffer_lines(1, curbuf_len());
    let n = lines.len();
    if n == 0 {
        return None;
    }
    let cur = (clnum - 1).clamp(0, n as varnumber_T - 1) as usize;
    let cbyte = (ccol - 1).max(0) as usize;
    // Build the line-visit order with the per-line starting byte offset.
    let mut order: Vec<(usize, Option<usize>, Option<usize>)> = Vec::new();
    if !backward {
        // Current line from the cursor, following lines, then (wrap) earlier.
        order.push((cur, Some(if accept { cbyte } else { cbyte + 1 }), None));
        for i in cur + 1..n {
            order.push((i, Some(0), None));
        }
        if wrap {
            for i in 0..=cur {
                order.push((i, Some(0), None));
            }
        }
    } else {
        // Current line up to the cursor, preceding lines, then (wrap) later.
        order.push((cur, None, Some(if accept { cbyte + 1 } else { cbyte })));
        for i in (0..cur).rev() {
            order.push((i, None, Some(usize::MAX)));
        }
        if wrap {
            for i in (cur..n).rev() {
                order.push((i, None, Some(usize::MAX)));
            }
        }
    }
    for (li, fwd_from, back_before) in order {
        let line = &lines[li];
        if let Some(from) = fwd_from {
            if let Some((s, e)) = line_match_from(pat, line, from, ic) {
                let col = if want_end { e } else { s + 1 };
                return finish_search(li, col, nomove, clnum, ccol);
            }
        } else if let Some(before) = back_before {
            // Backward: the last match that starts before `before`.
            let mut found: Option<(usize, usize)> = None;
            let mut scan = 0usize;
            while let Some((s, e)) = line_match_from(pat, line, scan, ic) {
                if s >= before {
                    break;
                }
                found = Some((s, e));
                scan = if e > s { e } else { s + 1 };
            }
            if let Some((s, e)) = found {
                let col = if want_end { e } else { s + 1 };
                return finish_search(li, col, nomove, clnum, ccol);
            }
        }
    }
    None
}

/// Move the cursor to the match (unless `nomove`) and return the 1-based
/// `(lnum, col)`.
fn finish_search(
    li: usize,
    col: usize,
    nomove: bool,
    _clnum: varnumber_T,
    _ccol: varnumber_T,
) -> Option<(varnumber_T, varnumber_T)> {
    let lnum = li as varnumber_T + 1;
    let col = col as varnumber_T;
    if !nomove {
        set_cursorpos(lnum, col);
    }
    Some((lnum, col))
}

/// Port of `f_search()`/`search_cmn()` (search.c) — search the buffer for the
/// pattern, move the cursor, and return the matching line number (0 if none).
pub fn f_search(argvars: &[typval_T], rettv: &mut typval_T) {
    let pat = tv_get_string(&argvars[0]);
    let flags = argvars.get(1).map(tv_get_string).unwrap_or_default();
    let stopline = argvars
        .get(2)
        .filter(|t| t.v_type != VAR_UNKNOWN)
        .map(tv_get_number)
        .unwrap_or(0);
    let lnum = match searchit(&pat, &flags, stopline) {
        Some((lnum, _)) => lnum,
        None => 0,
    };
    *rettv = typval_T::from(lnum);
}
/// Port of `f_searchpos()` (search.c) — like `search()` but returns
/// `[lnum, col]` (`[0, 0]` if not found).
pub fn f_searchpos(argvars: &[typval_T], rettv: &mut typval_T) {
    let pat = tv_get_string(&argvars[0]);
    let flags = argvars.get(1).map(tv_get_string).unwrap_or_default();
    let stopline = argvars
        .get(2)
        .filter(|t| t.v_type != VAR_UNKNOWN)
        .map(tv_get_number)
        .unwrap_or(0);
    let (lnum, col) = searchit(&pat, &flags, stopline).unwrap_or((0, 0));
    let l = tv_list_alloc_ret(rettv, 2);
    let mut lb = l.borrow_mut();
    tv_list_append_number(&mut lb, lnum);
    tv_list_append_number(&mut lb, col);
}
/// Port of `searchpair_cmn()` (Neovim search.c) — from the cursor, find the
/// `end` of a `start`…`end` pair (a `middle` at nesting level 0 also matches),
/// honoring nesting. Forward by default, backward with the `b` flag; moves the
/// cursor unless `n`. Returns the 1-based `(lnum, col)` of the match.
fn do_searchpair(
    start: &str,
    middle: &str,
    end: &str,
    flags: &str,
) -> Option<(varnumber_T, varnumber_T)> {
    let backward = flags.contains('b');
    let nomove = flags.contains('n');
    let ic = tv_get_number_chk(&get_option_value("ignorecase"), None) != 0;
    let lines = get_buffer_lines(1, curbuf_len());
    let n = lines.len();
    let (clnum, ccol) = {
        let __p = crate::fusevm_bridge::editor_curpos(CURPOS.with(|c| *c.borrow()));
        (__p.0, __p.1)
    };
    let cur = (clnum - 1).clamp(0, n as varnumber_T - 1) as usize;
    let cbyte = (ccol - 1).max(0) as usize;
    let m_at = |pat: &str, line: &str, col: usize| -> Option<usize> {
        if pat.is_empty() || col > line.len() {
            return None;
        }
        let (_, s, e) = crate::viml_regex::regex_matchstrpos(pat, &line[col..], ic);
        if s == 0 {
            Some((e - s).max(1) as usize)
        } else {
            None
        }
    };
    // Build the (line, col) scan positions in order; forward starts just after
    // the cursor, backward just before it.
    let mut positions: Vec<(usize, usize)> = Vec::new();
    #[allow(clippy::needless_range_loop)]
    if !backward {
        for li in cur..n {
            let from = if li == cur { cbyte + 1 } else { 0 };
            for col in from..=lines[li].len() {
                positions.push((li, col));
            }
        }
    } else {
        for li in (0..=cur).rev() {
            let to = if li == cur {
                cbyte
            } else {
                lines[li].len() + 1
            };
            for col in (0..to).rev() {
                positions.push((li, col));
            }
        }
    }
    let mut nest = 0i32;
    let mut idx = 0usize;
    while idx < positions.len() {
        let (li, col) = positions[idx];
        let line = &lines[li];
        // On the way out (forward), `end` closes; `start` opens. Backward is the
        // mirror: `start` closes the pair we are inside, `end` opens nesting.
        let (opener, closer) = if backward { (end, start) } else { (start, end) };
        if let Some(len) = m_at(closer, line, col) {
            if nest == 0 {
                let lnum = li as varnumber_T + 1;
                let c = col as varnumber_T + 1;
                if !nomove {
                    set_cursorpos(lnum, c);
                }
                return Some((lnum, c));
            }
            nest -= 1;
            idx += skip_cols(&positions, idx, len);
            continue;
        }
        if let Some(len) = m_at(opener, line, col) {
            nest += 1;
            idx += skip_cols(&positions, idx, len);
            continue;
        }
        if nest == 0 && m_at(middle, line, col).is_some() {
            let lnum = li as varnumber_T + 1;
            let c = col as varnumber_T + 1;
            if !nomove {
                set_cursorpos(lnum, c);
            }
            return Some((lnum, c));
        }
        idx += 1;
    }
    None
}

/// How many scan positions to advance to step past a `len`-byte match (at least
/// one). Positions on the same line are consecutive.
fn skip_cols(positions: &[(usize, usize)], idx: usize, len: usize) -> usize {
    let (li, _) = positions[idx];
    let mut n = 1;
    while n < len && idx + n < positions.len() && positions[idx + n].0 == li {
        n += 1;
    }
    n
}

/// Port of `f_searchpair()` — the line of the matching `end`, 0 if none.
pub fn f_searchpair(argvars: &[typval_T], rettv: &mut typval_T) {
    let start = tv_get_string(&argvars[0]);
    let middle = tv_get_string(&argvars[1]);
    let end = tv_get_string(&argvars[2]);
    let flags = argvars.get(3).map(tv_get_string).unwrap_or_default();
    let lnum = do_searchpair(&start, &middle, &end, &flags).map_or(0, |(l, _)| l);
    *rettv = typval_T::from(lnum);
}
/// Port of `f_searchpairpos()` — the `[lnum, col]` of the matching `end`.
pub fn f_searchpairpos(argvars: &[typval_T], rettv: &mut typval_T) {
    let start = tv_get_string(&argvars[0]);
    let middle = tv_get_string(&argvars[1]);
    let end = tv_get_string(&argvars[2]);
    let flags = argvars.get(3).map(tv_get_string).unwrap_or_default();
    let (lnum, col) = do_searchpair(&start, &middle, &end, &flags).unwrap_or((0, 0));
    let l = tv_list_alloc_ret(rettv, 2);
    let mut lb = l.borrow_mut();
    tv_list_append_number(&mut lb, lnum);
    tv_list_append_number(&mut lb, col);
}
/// Port of `f_searchdecl()` — declaration not found → 1 (the C `FAIL` default).
pub fn f_searchdecl(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(1 as varnumber_T);
}
/// Port of `f_getcharsearch()` — no prior `f`/`t` search → `{char:"",
/// forward:1, until:0}` (the `last_csearch*()` defaults).
pub fn f_getcharsearch(_argvars: &[typval_T], rettv: &mut typval_T) {
    let d = tv_dict_alloc_ret(rettv);
    let mut db = d.borrow_mut();
    tv_dict_add_str(&mut db, "char", "");
    tv_dict_add_nr(&mut db, "forward", 1);
    tv_dict_add_nr(&mut db, "until", 0);
}

// ── Interactive input builtins (stdin-backed standalone equivalent) ──
//
// In the editor these prompt through the command-line UI (`get_user_input`,
// `get_number`, …). A standalone interpreter is a terminal program, so the
// faithful equivalent is to write the prompt to stdout and read one line from
// stdin — the same role `read` plays in a shell script. On EOF the value the
// editor returns when the user cancels (empty / the dialog cancel-arg) is used.

/// Port of `get_user_input()` (the body behind `f_input`/`f_inputdialog` in
/// `Src/eval/funcs.c`, defined outside the vendored tree) — write `{prompt}`
/// (argvars[0]) to stdout and read one line from stdin. `{text}` (argvars[1])
/// is the editable default returned on an empty line / EOF; for `inputdialog`
/// argvars[2] is the value returned when the read is cancelled (EOF).
fn get_user_input(argvars: &[typval_T], rettv: &mut typval_T, dialog: bool, _secret: bool) {
    use std::io::Write;
    let prompt = tv_get_string(&argvars[0]);
    let default = if argvars.len() > 1 {
        tv_get_string(&argvars[1])
    } else {
        String::new()
    };
    print!("{prompt}");
    let _ = std::io::stdout().flush();

    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        // EOF: the command line was cancelled.
        Ok(0) => {
            let cancel = if dialog && argvars.len() > 2 {
                tv_get_string(&argvars[2])
            } else {
                default
            };
            *rettv = typval_T::from(cancel);
            return;
        }
        Ok(_) => {}
        Err(_) => {
            *rettv = typval_T::from(default);
            return;
        }
    }
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
    // An empty line returns the (pre-filled) default, as pressing <CR> would.
    if line.is_empty() && !default.is_empty() {
        *rettv = typval_T::from(default);
    } else {
        *rettv = typval_T::from(line);
    }
}

/// Port of `f_input()` — read a line from stdin after writing the prompt.
pub fn f_input(argvars: &[typval_T], rettv: &mut typval_T) {
    get_user_input(argvars, rettv, false, false);
}
/// Port of `f_inputsecret()` — like `input()`; standalone cannot suppress
/// terminal echo without raw mode, so input is read normally (best effort).
pub fn f_inputsecret(argvars: &[typval_T], rettv: &mut typval_T) {
    get_user_input(argvars, rettv, false, true);
}
/// Port of `f_inputdialog()` — `input()` with a cancel value (argvars[2]).
pub fn f_inputdialog(argvars: &[typval_T], rettv: &mut typval_T) {
    get_user_input(argvars, rettv, true, false);
}
/// Port of `f_inputsave()` — typeahead stack push; nothing is buffered
/// standalone, so this is a no-op returning 0 (OK), as in C.
pub fn f_inputsave(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_inputrestore()` — typeahead stack pop; no-op returning 0 (OK).
pub fn f_inputrestore(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}

/// Port of `f_inputlist()` — print the `{textlist}` (a List of String lines, the
/// first being a header) and read the selected 1-based index from stdin,
/// returning 0 when the input is empty or not a number.
pub fn f_inputlist(argvars: &[typval_T], rettv: &mut typval_T) {
    use std::io::Write;
    if argvars[0].v_type != VAR_LIST {
        // c: semsg(_(e_listarg), "inputlist()");
        emsg("E686: Argument of inputlist() must be a List");
        return;
    }
    if let v_list(Some(l)) = &argvars[0].vval {
        for it in &l.borrow().lv_items {
            println!("{}", tv_get_string(&it.li_tv));
        }
    }
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let n = match std::io::stdin().read_line(&mut line) {
        Ok(n) if n > 0 => line.trim().parse::<varnumber_T>().unwrap_or(0),
        _ => 0,
    };
    *rettv = typval_T::from(n);
}

/// Port of `f_confirm()` — print `{msg}` and the `&`-accelerated `{choices}`
/// (split on `\n`, default "&OK") numbered from 1, then read the chosen number
/// from stdin. Empty input returns the `{default}` button (argvars[2], default
/// 1); EOF returns 0 (cancelled), as the editor's dialog does.
pub fn f_confirm(argvars: &[typval_T], rettv: &mut typval_T) {
    use std::io::Write;
    let message = tv_get_string(&argvars[0]);
    let buttons = if argvars.len() > 1 && argvars[1].v_type != VAR_UNKNOWN {
        tv_get_string(&argvars[1])
    } else {
        "&OK".to_string()
    };
    let def = if argvars.len() > 2 && argvars[2].v_type != VAR_UNKNOWN {
        tv_get_number(&argvars[2])
    } else {
        1
    };
    println!("{message}");
    for (i, b) in buttons.split('\n').enumerate() {
        // Drop the '&' accelerator markers (c: drops them from the label).
        let label: String = b.chars().filter(|&c| c != '&').collect();
        println!("{}) {}", i + 1, label);
    }
    print!("Type number and <Enter> (default {def}): ");
    let _ = std::io::stdout().flush();

    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) | Err(_) => *rettv = typval_T::from(0 as varnumber_T), // cancelled
        Ok(_) => {
            let t = line.trim();
            let choice = if t.is_empty() {
                def
            } else {
                t.parse::<varnumber_T>().unwrap_or(0)
            };
            *rettv = typval_T::from(choice);
        }
    }
}

// ── Syntax / spell / swap / region / timer / cursor-setter builtins ──
//
// A standalone interpreter has no syntax highlighter, spell checker, swap
// files, screen region, event-loop timers, or current window/buffer to move a
// cursor in. Each reduces to the value its C body produces with the subsystem
// inactive: no syntax id (0) / attribute (""), no swap file (""), no bad word
// (`["", ""]`), an empty result List, a cursor/position set that cannot apply
// (-1), or a timer that cannot be created without an event loop (-1).

// ── Highlight-group registry (EXTENSION — no `vendor/` counterpart) ─────────────
//
// A standalone interpreter has no UI, so vendor's highlight groups never exist and
// the C `:highlight` machinery (`syn_name2id`/`highlight_exists`/`syn_id2attr`)
// finds nothing. But a sourced colorscheme or vimrc DEFINES groups via
// `:highlight {group} …` and real scripts guard on `hlexists()`/`hlID()` before
// (re)defining or reading a group. We keep a minimal ordered registry so those
// reflect what the running script has actually defined; an embedding editor (see
// [`crate::fusevm_bridge`]) additionally receives each definition to map onto its
// own theme. Group name lookup is case-insensitive, matching Vim.

/// One `:highlight` definition: either a link (`hi link A B`) or the raw
/// `key=val` attributes (`ctermfg=…`, `guifg=…`, `cterm=…`, `gui=…`, …).
#[derive(Default, Clone)]
pub struct HlGroup {
    pub attrs: std::collections::HashMap<String, String>,
    pub link: Option<String>,
    pub cleared: bool,
}

#[derive(Default)]
pub struct HlRegistry {
    /// Group name by id-1 (Vim highlight ids are 1-based, allocation order).
    order: Vec<String>,
    /// Canonical (lowercase) name → definition.
    groups: std::collections::HashMap<String, HlGroup>,
}

thread_local! {
    pub static HL_GROUPS: std::cell::RefCell<HlRegistry> =
        std::cell::RefCell::new(HlRegistry::default());
}

/// Allocate (or find) the 1-based id for `name`, creating an empty slot the first
/// time a group is named — mirrors `syn_check_group()`.
fn hl_intern(reg: &mut HlRegistry, name: &str) -> varnumber_T {
    let key = name.to_ascii_lowercase();
    if let Some(pos) = reg.order.iter().position(|g| *g == key) {
        return (pos + 1) as varnumber_T;
    }
    reg.order.push(key.clone());
    reg.groups.entry(key).or_default();
    reg.order.len() as varnumber_T
}

/// Apply one `:highlight` command's argument text to the registry and return the
/// target group name (for the host hook), or `None` for an argument-less listing
/// query. Handles `hi[!] [default] {group} key=val…`, `hi link A B`,
/// `hi clear [group]`, and `hi default link A B`.
pub fn hl_define_from_args(args: &str) -> Option<String> {
    let args = args.trim();
    if args.is_empty() {
        return None; // bare `:highlight` — a listing query.
    }
    let mut toks = args.split_whitespace().peekable();
    // Skip a leading `default` keyword (does not change the target group).
    let mut first = *toks.peek()?;
    if first == "default" {
        toks.next();
        first = *toks.peek()?;
    }
    // `hi clear` / `hi clear {group}`.
    if first == "clear" {
        toks.next();
        return HL_GROUPS.with(|r| {
            let mut reg = r.borrow_mut();
            match toks.next() {
                Some(g) => {
                    hl_intern(&mut reg, g);
                    let key = g.to_ascii_lowercase();
                    if let Some(def) = reg.groups.get_mut(&key) {
                        *def = HlGroup {
                            cleared: true,
                            ..HlGroup::default()
                        };
                    }
                    Some(g.to_string())
                }
                None => {
                    // `hi clear` alone resets every group.
                    for def in reg.groups.values_mut() {
                        *def = HlGroup {
                            cleared: true,
                            ..HlGroup::default()
                        };
                    }
                    None
                }
            }
        });
    }
    // `hi link {from} {to}` / `hi! link …` / `hi default link …`.
    if first == "link" {
        toks.next();
        let from = toks.next()?;
        let to = toks.next()?;
        return HL_GROUPS.with(|r| {
            let mut reg = r.borrow_mut();
            hl_intern(&mut reg, from);
            hl_intern(&mut reg, to);
            let key = from.to_ascii_lowercase();
            let def = reg.groups.entry(key).or_default();
            def.link = Some(to.to_string());
            def.cleared = false;
            Some(from.to_string())
        });
    }
    // `hi {group} key=val …`.
    let group = toks.next()?.to_string();
    HL_GROUPS.with(|r| {
        let mut reg = r.borrow_mut();
        hl_intern(&mut reg, &group);
        let key = group.to_ascii_lowercase();
        let def = reg.groups.entry(key).or_default();
        def.cleared = false;
        def.link = None;
        for kv in toks {
            if let Some((k, v)) = kv.split_once('=') {
                def.attrs.insert(k.to_ascii_lowercase(), v.to_string());
            }
        }
        Some(group)
    })
}

/// A highlight group resolved through its `link` chain, for an embedding editor
/// to translate into its own theme. Colours are the raw Vim tokens (`#rrggbb`,
/// a cterm number, or a colour name like `DarkBlue`); `attrs` is the union of the
/// `gui=`/`cterm=` display-attribute lists (`bold`, `italic`, `underline`, …).
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct ResolvedHl {
    pub guifg: Option<String>,
    pub guibg: Option<String>,
    pub guisp: Option<String>,
    pub ctermfg: Option<String>,
    pub ctermbg: Option<String>,
    pub attrs: Vec<String>,
    pub cleared: bool,
}

/// Names of every highlight group named this session, in allocation order.
pub fn hl_names() -> Vec<String> {
    HL_GROUPS.with(|r| r.borrow().order.clone())
}

/// Resolve group `name` (following `link`s) to concrete colours + attributes for
/// an embedding editor. `None` if the group was never named; a `cleared` group
/// resolves to an all-empty `ResolvedHl` with `cleared = true`.
pub fn hl_resolved(name: &str) -> Option<ResolvedHl> {
    HL_GROUPS.with(|r| {
        let reg = r.borrow();
        let def = reg.groups.get(&name.to_ascii_lowercase())?;
        if def.cleared {
            return Some(ResolvedHl {
                cleared: true,
                ..ResolvedHl::default()
            });
        }
        let attr_list = hl_lookup(&reg, name, "gui", 0)
            .or_else(|| hl_lookup(&reg, name, "cterm", 0))
            .or_else(|| hl_lookup(&reg, name, "term", 0))
            .map(|list| {
                list.split(',')
                    .filter(|a| !a.eq_ignore_ascii_case("none") && !a.is_empty())
                    .map(|a| a.to_ascii_lowercase())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        // A colour token of "NONE" means "no colour" — treat as unset.
        let colour =
            |key: &str| hl_lookup(&reg, name, key, 0).filter(|v| !v.eq_ignore_ascii_case("none"));
        Some(ResolvedHl {
            guifg: colour("guifg"),
            guibg: colour("guibg"),
            guisp: colour("guisp"),
            ctermfg: colour("ctermfg"),
            ctermbg: colour("ctermbg"),
            attrs: attr_list,
            cleared: false,
        })
    })
}

/// Resolve a highlight `key` (following `link`s) to a string attribute value.
fn hl_lookup(reg: &HlRegistry, name: &str, key: &str, depth: u8) -> Option<String> {
    if depth > 16 {
        return None;
    }
    let def = reg.groups.get(&name.to_ascii_lowercase())?;
    if let Some(v) = def.attrs.get(key) {
        return Some(v.clone());
    }
    if let Some(link) = &def.link {
        return hl_lookup(reg, link, key, depth + 1);
    }
    None
}

/// 1-based id of highlight group `name`, or 0 if never named (`syn_name2id`).
pub fn hl_id(name: &str) -> varnumber_T {
    HL_GROUPS.with(|r| {
        let reg = r.borrow();
        reg.order
            .iter()
            .position(|g| *g == name.to_ascii_lowercase())
            .map(|p| (p + 1) as varnumber_T)
            .unwrap_or(0)
    })
}

/// Whether highlight group `name` exists (has been named and not cleared).
pub fn hl_exists(name: &str) -> bool {
    HL_GROUPS.with(|r| {
        r.borrow()
            .groups
            .get(&name.to_ascii_lowercase())
            .is_some_and(|d| !d.cleared)
    })
}

/// `synIDattr(id, what[, mode])` over the registry: resolve group id → name, then
/// the requested attribute. Colour queries (`fg`/`bg`/`sp`, with an optional `#`)
/// prefer the GUI value in "gui" mode and the cterm value otherwise; boolean
/// attributes (`bold`/`italic`/…) test the relevant `cterm=`/`gui=` list.
fn hl_synidattr(id: varnumber_T, what: &str, mode: &str) -> String {
    if id < 1 {
        return String::new();
    }
    HL_GROUPS.with(|r| {
        let reg = r.borrow();
        let Some(name) = reg.order.get((id - 1) as usize).cloned() else {
            return String::new();
        };
        let gui = mode == "gui" || mode.is_empty();
        let what = what.trim_end_matches('#');
        let colour = |g_key: &str, c_key: &str| -> String {
            let primary = if gui { g_key } else { c_key };
            hl_lookup(&reg, &name, primary, 0)
                .or_else(|| hl_lookup(&reg, &name, if gui { c_key } else { g_key }, 0))
                .unwrap_or_default()
        };
        match what {
            "name" => name,
            "fg" => colour("guifg", "ctermfg"),
            "bg" => colour("guibg", "ctermbg"),
            "sp" => colour("guisp", "ctermul"),
            "font" => hl_lookup(&reg, &name, "font", 0).unwrap_or_default(),
            // Boolean display attributes live in the `gui=`/`cterm=`/`term=` lists.
            "bold" | "italic" | "reverse" | "inverse" | "standout" | "underline" | "undercurl"
            | "underdouble" | "underdotted" | "underdashed" | "strikethrough" | "nocombine" => {
                let list_key = if gui { "gui" } else { "cterm" };
                let hit = hl_lookup(&reg, &name, list_key, 0)
                    .or_else(|| hl_lookup(&reg, &name, "term", 0))
                    .map(|list| {
                        // `reverse` and `inverse` are synonyms in the attr list.
                        let want = if what == "inverse" { "reverse" } else { what };
                        list.split(',').any(|a| a.eq_ignore_ascii_case(want))
                    })
                    .unwrap_or(false);
                if hit {
                    "1".into()
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        }
    })
}

/// Port of `f_synID()` — no syntax highlighter → id 0.
pub fn f_synID(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_synIDtrans()` — no syntax → translated id 0.
pub fn f_synIDtrans(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_synIDattr()`. There is no live syntax highlighter, so a `synID()`
/// result is 0 and yields "". But `synIDattr(hlID('Group'), …)` — reading a
/// script-defined group's colour/attributes — resolves through the highlight
/// registry (see [`HL_GROUPS`]).
pub fn f_synIDattr(argvars: &[typval_T], rettv: &mut typval_T) {
    let id = argvars
        .first()
        .map(|a| tv_get_number_chk(a, None))
        .unwrap_or(0);
    let what = argvars.get(1).map(tv_get_string).unwrap_or_default();
    let mode = argvars.get(2).map(tv_get_string).unwrap_or_default();
    *rettv = typval_T::from(hl_synidattr(id, &what, &mode));
}
/// Port of `f_synstack()` — `tv_list_set_ret(rettv, NULL)`, never filled with no
/// buffer → empty List.
pub fn f_synstack(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_list_alloc_ret(rettv, 0);
}
/// Port of `f_synconcealed()` — the unconditional `[concealed, text, matchid]`
/// triple; nothing is concealed standalone → `[0, '', 0]`.
pub fn f_synconcealed(_argvars: &[typval_T], rettv: &mut typval_T) {
    let l = tv_list_alloc_ret(rettv, 3);
    let mut lb = l.borrow_mut();
    tv_list_append_number(&mut lb, 0);
    tv_list_append_string(&mut lb, "");
    tv_list_append_number(&mut lb, 0);
}
/// Port of `f_changenr()` — `curbuf->b_u_seq_cur`; no undo history → 0.
pub fn f_changenr(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_swapname()` — no swap file → "" (the C NULL string).
pub fn f_swapname(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(String::new());
}
/// Port of `f_swapfilelist()`/`recover_names()` — no swap files → empty List.
pub fn f_swapfilelist(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_list_alloc_ret(rettv, 0);
}
/// Port of `f_spellbadword()` — no spell checker → `['', '']` (no bad word).
pub fn f_spellbadword(_argvars: &[typval_T], rettv: &mut typval_T) {
    let l = tv_list_alloc_ret(rettv, 2);
    let mut lb = l.borrow_mut();
    tv_list_append_string(&mut lb, "");
    tv_list_append_string(&mut lb, "");
}
/// Port of `f_spellsuggest()` — no spell checker → empty List.
pub fn f_spellsuggest(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_list_alloc_ret(rettv, 0);
}
/// Port of `f_getregion()` — no buffer/selection → empty List.
pub fn f_getregion(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: getregion(p1, p2 [, opts]) — text between two [buf, lnum, col, off]
    // positions. `type` "v" charwise (default), "V" linewise.
    let pos = |tv: &typval_T| -> (varnumber_T, varnumber_T) {
        match (tv.v_type, &tv.vval) {
            (VAR_LIST, v_list(Some(l))) => {
                let it = &l.borrow().lv_items;
                (
                    it.get(1).map_or(0, |x| tv_get_number(&x.li_tv)),
                    it.get(2).map_or(0, |x| tv_get_number(&x.li_tv)),
                )
            }
            _ => (0, 0),
        }
    };
    let (mut l1, mut c1) = pos(&argvars[0]);
    let (mut l2, mut c2) = pos(&argvars[1]);
    if (l1, c1) > (l2, c2) {
        std::mem::swap(&mut l1, &mut l2);
        std::mem::swap(&mut c1, &mut c2);
    }
    let linewise = argvars
        .get(2)
        .and_then(|o| match (o.v_type, &o.vval) {
            (VAR_DICT, v_dict(Some(d))) => tv_dict_find(&d.borrow(), "type").map(tv_get_string),
            _ => None,
        })
        .is_some_and(|t| t.starts_with('V'));
    let out = tv_list_alloc_ret(rettv, 0);
    let mut ob = out.borrow_mut();
    let lines = get_buffer_lines(l1, l2);
    for (i, line) in lines.iter().enumerate() {
        let lnum = l1 + i as varnumber_T;
        let piece = if linewise {
            line.clone()
        } else {
            let start = if lnum == l1 {
                (c1 as usize).saturating_sub(1)
            } else {
                0
            };
            let end = if lnum == l2 {
                (c2 as usize).min(line.len())
            } else {
                line.len()
            };
            line.get(start.min(line.len())..end.max(start.min(line.len())))
                .unwrap_or("")
                .to_string()
        };
        tv_list_append_string(&mut ob, &piece);
    }
}
/// Port of `f_getregionpos()` — no buffer/selection → empty List.
pub fn f_getregionpos(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_list_alloc_ret(rettv, 0);
}
/// Port of `f_matchbufline()` (buffer.c) — every match of `{pat}` in buffer
/// lines `{lnum}`..`{end}` as a List of `{lnum, byteidx, text}` (single buffer,
/// so `{buf}` is ignored).
pub fn f_matchbufline(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: `buf = tv_get_buf(&argvars[0], false); if (buf == NULL) { semsg(
    // _(e_invalid_buffer_name_str), ...); return; }` — a buffer that does not
    // exist is E158, not an empty result list.
    //
    // Standalone, the line store is the single current buffer that `getbufinfo()`
    // reports as `bufnr: 1`, and it is not on the `buf_T` list `tv_get_buf` walks
    // (that list is populated by an editor host). So the current buffer — `1`,
    // `''` or `'%'` — resolves here even with no host attached; every other
    // designator is E158, as in Vim.
    //
    // c: `tv_get_buf` does `buflist_findnr((int)tv->vval.v_number)` — the C
    // `(int)` cast truncates the 64-bit number to its low 32 bits, so e.g.
    // -9223372036854775807 (0x8000000000000001) designates buffer 1 (verified
    // against vim 9.2 and nvim 0.12: it passes the buffer check and fails later
    // on end_lnum). And 0 goes through `buflist_findnr`'s
    // `if (nr == 0) nr = curwin->w_alt_fnum;` — no alternate buffer standalone,
    // so 0 is E158 (`matchbufline(0, …)` is "E158: Invalid buffer name: 0" in
    // both oracles), NOT the current buffer.
    let designates_curbuf = match (&argvars[0].v_type, &argvars[0].vval) {
        (VAR_NUMBER, v_number(n)) => (*n as i32) == 1,
        (VAR_STRING, v_string(s)) => s.is_empty() || s == "%",
        _ => false,
    };
    if !designates_curbuf && tv_get_buf(&argvars[0], false).is_none() {
        crate::ported::message::semsg(&format!(
            "E158: Invalid buffer name: {}",
            tv_get_string(&argvars[0])
        ));
        tv_list_alloc_ret(rettv, 0);
        return;
    }
    let pat = tv_get_string(&argvars[1]);
    // c: `linenr_T slnum = tv_get_lnum_buf(&argvars[2], buf);` — linenr_T is a
    // 32-bit int, so the C assignment truncates a 64-bit lnum to its low 32 bits
    // (`matchbufline(1, 'a', 1, 9223372036854775807)` sees end_lnum == -1 in the
    // real binaries). Mirror the cast.
    let lnum = tv_get_lnum(&argvars[2]) as i32 as varnumber_T;
    // c: `if (slnum < 1) { semsg(_(e_invargval), "lnum"); return; }` and
    // `if (elnum < 1 || elnum < slnum) { semsg(_(e_invargval), "end_lnum"); return; }`
    // — line numbers are 1-based, so 0 or negative is an error, not an empty list.
    if lnum < 1 {
        crate::ported::message::semsg("E475: Invalid value for argument lnum");
        tv_list_alloc_ret(rettv, 0);
        return;
    }
    let end = tv_get_lnum(&argvars[3]) as i32 as varnumber_T;
    if end < 1 || end < lnum {
        crate::ported::message::semsg("E475: Invalid value for argument end_lnum");
        tv_list_alloc_ret(rettv, 0);
        return;
    }
    let ic = tv_get_number_chk(&get_option_value("ignorecase"), None) != 0;
    // c: optional {dict} with "submatches".
    let submatches = argvars.get(4).is_some_and(|d| match &d.vval {
        v_dict(Some(dd)) => {
            dd.borrow()
                .dv_hashtab
                .get("submatches")
                .map(tv_get_bool)
                .unwrap_or(0)
                != 0
        }
        _ => false,
    });
    let out = tv_list_alloc_ret(rettv, 0);
    for (i, line) in get_buffer_lines(lnum, end).iter().enumerate() {
        let ln = lnum + i as varnumber_T;
        // c: matchbuf = true → the dict uses "lnum".
        get_matches_in_str(line, &pat, ic, &out, ln, submatches, true);
    }
}
/// Port of `f_menu_get()` — no menus → empty List.
pub fn f_menu_get(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_list_alloc_ret(rettv, 0);
}
/// Port of `f_timer_info()` — no event-loop timers → empty List.
pub fn f_timer_info(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_list_alloc_ret(rettv, 0);
}
/// Port of `f_timer_start()` — no event loop to schedule on → -1 (the C error
/// default; a real timer id is otherwise returned).
pub fn f_timer_start(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
}
/// Port of `f_timer_stop()` — no timers → no-op (rettv stays 0).
pub fn f_timer_stop(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_timer_pause()` — no timers → no-op (rettv stays 0).
pub fn f_timer_pause(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_timer_stopall()` — no timers → no-op (rettv stays 0).
pub fn f_timer_stopall(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_setpos()`/`set_position(…,false)` — no buffer/window to set a
/// position in → -1 (the C error default).
pub fn f_setpos(argvars: &[typval_T], rettv: &mut typval_T) {
    let expr = tv_get_string(&argvars[0]);
    // c: the position List is [bufnum, lnum, col, off]; `.` sets the cursor and
    // `'m` sets mark m.
    if let (VAR_LIST, v_list(Some(l))) = (argvars[1].v_type, &argvars[1].vval) {
        let items = &l.borrow().lv_items;
        let lnum = items.get(1).map_or(0, |it| tv_get_number(&it.li_tv));
        let col = items.get(2).map_or(1, |it| tv_get_number(&it.li_tv));
        if expr == "." {
            set_cursorpos(lnum, col);
        } else if let Some(name) = expr.strip_prefix('\'').and_then(|r| r.chars().next()) {
            setmark(name, lnum, col);
        }
    }
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_setcharpos()`/`set_position(…,true)` — like `setpos()`.
pub fn f_setcharpos(argvars: &[typval_T], rettv: &mut typval_T) {
    f_setpos(argvars, rettv);
}
/// Port of `f_cursor()`/`set_cursorpos(…,false)` — move the cursor to `{lnum}`,
/// `{col}` (or a `[lnum, col, off]` List). Returns 0 on success.
pub fn f_cursor(argvars: &[typval_T], rettv: &mut typval_T) {
    let (lnum, col) = if argvars[0].v_type == VAR_LIST {
        if let (VAR_LIST, v_list(Some(l))) = (argvars[0].v_type, &argvars[0].vval) {
            let items = &l.borrow().lv_items;
            (
                items.first().map_or(1, |it| tv_get_number(&it.li_tv)),
                items.get(1).map_or(1, |it| tv_get_number(&it.li_tv)),
            )
        } else {
            (1, 1)
        }
    } else {
        (
            tv_get_number(&argvars[0]),
            argvars.get(1).map_or(1, tv_get_number),
        )
    };
    set_cursorpos(lnum, col);
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_setcursorcharpos()`/`set_cursorpos(…,true)` — like `cursor()`.
pub fn f_setcursorcharpos(argvars: &[typval_T], rettv: &mut typval_T) {
    f_cursor(argvars, rettv);
}
/// Port of `f_setcharsearch()` — sets the `f`/`t` search state we do not track
/// standalone → no-op (rettv stays 0, as the C sets no return value).
pub fn f_setcharsearch(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_settagstack()` — no window tag stack to mutate → accepted no-op
/// (0, the C success return).
pub fn f_settagstack(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}

// ── assert_*() — the Vim unit-testing framework (testing.c, not vendored) ──
//
// Each assert appends a failure message to `v:errors` (via the vendored
// `assert_error`, vendor/eval/vars.c:3360) and returns 1 on failure, 0 on
// success — so a script can run a batch of asserts and then inspect
// `v:errors`. Behaviour and message wording follow the spec documented in
// `vendor/eval.lua` (the implementations live in Neovim's `testing.c`, which is
// not part of the vendored eval tree). Values render with `string()`
// (`encode_tv2string`); a user `{msg}` renders with `:echo` rules
// (`encode_tv2echo`), matching the C `fill_assert_error`.

/// The assertion flavour selecting `fill_assert_error`'s wording.
#[derive(Clone, Copy, PartialEq)]
enum AssertType {
    Equal,
    NotEqual,
    Match,
    NotMatch,
    Other,
}

/// Port of `fill_assert_error()` (Neovim `testing.c`, not vendored) — build the
/// `v:errors` line: an optional `{msg}: ` prefix, then `Expected …`/`Pattern …`
/// per `atype`, the expected value (or `exp_str` literal), and for the
/// non-`NotEqual` forms the actual value.
fn fill_assert_error(
    opt_msg: Option<&typval_T>,
    exp_str: Option<&str>,
    exp_tv: &typval_T,
    got_tv: &typval_T,
    atype: AssertType,
) -> String {
    let mut s = String::new();
    if let Some(m) = opt_msg {
        if m.v_type != VAR_UNKNOWN {
            s.push_str(&encode_tv2echo(m));
            s.push_str(": ");
        }
    }
    s.push_str(match atype {
        AssertType::Match | AssertType::NotMatch => "Pattern ",
        AssertType::NotEqual => "Expected not equal to ",
        _ => "Expected ",
    });
    match exp_str {
        Some(e) => s.push_str(e),
        None => s.push_str(&encode_tv2string(exp_tv)),
    }
    match atype {
        AssertType::NotEqual => {}
        AssertType::Match => {
            s.push_str(" does not match ");
            s.push_str(&encode_tv2string(got_tv));
        }
        AssertType::NotMatch => {
            s.push_str(" does match ");
            s.push_str(&encode_tv2string(got_tv));
        }
        _ => {
            s.push_str(" but got ");
            s.push_str(&encode_tv2string(got_tv));
        }
    }
    s
}

/// Shared body of `assert_equal`/`assert_notequal` (`assert_equal_common`):
/// compare with `tv_equal` (case always matters, no coercion) and record a
/// failure when the result is not the asserted relation.
fn assert_equal_common(argvars: &[typval_T], rettv: &mut typval_T, want_equal: bool) {
    let equal = tv_equal(&argvars[0], &argvars[1], false);
    if equal != want_equal {
        let atype = if want_equal {
            AssertType::Equal
        } else {
            AssertType::NotEqual
        };
        let msg = fill_assert_error(argvars.get(2), None, &argvars[0], &argvars[1], atype);
        assert_error(&msg);
        *rettv = typval_T::from(1 as varnumber_T);
    } else {
        *rettv = typval_T::from(0 as varnumber_T);
    }
}

/// Port of `f_assert_equal()` — fail when `{expected}` and `{actual}` differ.
pub fn f_assert_equal(argvars: &[typval_T], rettv: &mut typval_T) {
    assert_equal_common(argvars, rettv, true);
}
/// Port of `f_assert_notequal()` — fail when `{expected}` and `{actual}` equal.
pub fn f_assert_notequal(argvars: &[typval_T], rettv: &mut typval_T) {
    assert_equal_common(argvars, rettv, false);
}

/// Shared body of `assert_true`/`assert_false` (`assert_bool`): pass when
/// `{actual}` is a non-zero Number / `v:true` (resp. zero / `v:false`); any
/// other type fails. Message uses the `"True"`/`"False"` literal.
fn assert_bool(argvars: &[typval_T], rettv: &mut typval_T, is_true: bool) {
    let v = &argvars[0];
    let ok = match v.v_type {
        VAR_NUMBER => (tv_get_number(v) != 0) == is_true,
        VAR_BOOL => (tv_get_bool(v) != 0) == is_true,
        _ => false,
    };
    if !ok {
        let lit = if is_true { "True" } else { "False" };
        let msg = fill_assert_error(argvars.get(1), Some(lit), v, v, AssertType::Other);
        assert_error(&msg);
        *rettv = typval_T::from(1 as varnumber_T);
    } else {
        *rettv = typval_T::from(0 as varnumber_T);
    }
}

/// Port of `f_assert_true()` — fail unless `{actual}` is TRUE.
pub fn f_assert_true(argvars: &[typval_T], rettv: &mut typval_T) {
    assert_bool(argvars, rettv, true);
}
/// Port of `f_assert_false()` — fail unless `{actual}` is FALSE.
pub fn f_assert_false(argvars: &[typval_T], rettv: &mut typval_T) {
    assert_bool(argvars, rettv, false);
}

/// Shared body of `assert_match`/`assert_notmatch` (`assert_match_common`):
/// match `{pattern}` against `{actual}` as a string with Vim 'magic' regex,
/// case-sensitive (`assert` ignores 'ignorecase').
fn assert_match_common(argvars: &[typval_T], rettv: &mut typval_T, want_match: bool) {
    let pat = tv_get_string(&argvars[0]);
    let actual = tv_get_string(&argvars[1]);
    let matched = regex_match(&pat, &actual, false);
    if matched != want_match {
        let atype = if want_match {
            AssertType::Match
        } else {
            AssertType::NotMatch
        };
        let msg = fill_assert_error(argvars.get(2), None, &argvars[0], &argvars[1], atype);
        assert_error(&msg);
        *rettv = typval_T::from(1 as varnumber_T);
    } else {
        *rettv = typval_T::from(0 as varnumber_T);
    }
}

/// Port of `f_assert_match()` — fail when `{pattern}` does not match `{actual}`.
pub fn f_assert_match(argvars: &[typval_T], rettv: &mut typval_T) {
    assert_match_common(argvars, rettv, true);
}
/// Port of `f_assert_notmatch()` — fail when `{pattern}` matches `{actual}`.
pub fn f_assert_notmatch(argvars: &[typval_T], rettv: &mut typval_T) {
    assert_match_common(argvars, rettv, false);
}

/// Port of `f_assert_report()` — append `{msg}` to `v:errors` unconditionally.
pub fn f_assert_report(argvars: &[typval_T], rettv: &mut typval_T) {
    assert_error(&tv_get_string(&argvars[0]));
    *rettv = typval_T::from(1 as varnumber_T);
}

/// Port of `f_assert_inrange()` — fail when `{actual}` is outside the inclusive
/// `[{lower}, {upper}]` range. Numbers and Floats compare by value.
pub fn f_assert_inrange(argvars: &[typval_T], rettv: &mut typval_T) {
    let as_f64 = |tv: &typval_T| -> f64 {
        if tv.v_type == VAR_FLOAT {
            tv_get_float(tv)
        } else {
            tv_get_number(tv) as f64
        }
    };
    let lower = as_f64(&argvars[0]);
    let upper = as_f64(&argvars[1]);
    let actual = as_f64(&argvars[2]);
    if actual < lower || actual > upper {
        let mut msg = String::new();
        if let Some(m) = argvars.get(3) {
            if m.v_type != VAR_UNKNOWN {
                msg.push_str(&encode_tv2echo(m));
                msg.push_str(": ");
            }
        }
        msg.push_str(&format!(
            "Expected range {} - {}, but got {}",
            encode_tv2string(&argvars[0]),
            encode_tv2string(&argvars[1]),
            encode_tv2string(&argvars[2]),
        ));
        assert_error(&msg);
        *rettv = typval_T::from(1 as varnumber_T);
    } else {
        *rettv = typval_T::from(0 as varnumber_T);
    }
}

/// Port of `f_assert_exception()` — fail when `v:exception` does not contain the
/// `{error}` string. Used inside a `:catch` to assert the thrown exception.
pub fn f_assert_exception(argvars: &[typval_T], rettv: &mut typval_T) {
    let error = tv_get_string(&argvars[0]);
    let exc = get_vim_var_str(VV_EXCEPTION);
    if exc.is_empty() {
        // c: "v:exception is not set" when nothing was caught.
        assert_error("v:exception is not set");
        *rettv = typval_T::from(1 as varnumber_T);
    } else if !exc.contains(&error) {
        let got = typval_T::from(exc);
        let msg = fill_assert_error(argvars.get(1), None, &argvars[0], &got, AssertType::Other);
        assert_error(&msg);
        *rettv = typval_T::from(1 as varnumber_T);
    } else {
        *rettv = typval_T::from(0 as varnumber_T);
    }
}

// ── OS interaction: system()/systemlist()/environ() (os/shell.c, os/env.c) ──
//
// Not part of the vendored eval tree (their home files are os/shell.c and
// os/env.c). Faithful standalone ports: run a command through the shell and
// capture its stdout, or read the process environment. `system()` sets
// `v:shell_error` to the command's exit status, as in Vim.

/// Run `{cmd}` (argvars[0]) through `sh -c`, writing `{input}` (argvars[1], if
/// any) to its stdin, and return the captured stdout bytes. Sets `v:shell_error`
/// to the exit status (-1 if the shell could not be run). stderr is inherited
/// (shown), as Vim does by default.
fn get_cmd_output(argvars: &[typval_T]) -> Vec<u8> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let cmd = tv_get_string(&argvars[0]);
    let input = if argvars.len() > 1 && argvars[1].v_type != VAR_UNKNOWN {
        Some(tv_get_string(&argvars[1]))
    } else {
        None
    };

    let mut command = Command::new("sh");
    command.arg("-c").arg(&cmd).stdout(Stdio::piped());
    command.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });

    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(_) => {
            set_vim_var_nr(VV_SHELL_ERROR, -1);
            return Vec::new();
        }
    };
    if let Some(text) = input {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
    }
    match child.wait_with_output() {
        Ok(out) => {
            set_vim_var_nr(
                VV_SHELL_ERROR,
                out.status.code().unwrap_or(-1) as varnumber_T,
            );
            out.stdout
        }
        Err(_) => {
            set_vim_var_nr(VV_SHELL_ERROR, -1);
            Vec::new()
        }
    }
}

/// Port of `f_system()` — run `{cmd}` and return its output as a String
/// (trailing newline preserved, as in Vim).
pub fn f_system(argvars: &[typval_T], rettv: &mut typval_T) {
    let out = get_cmd_output(argvars);
    *rettv = typval_T::from(String::from_utf8_lossy(&out).into_owned());
}

/// Port of `f_systemlist()` — like `system()` but the output is split into a
/// List of lines (a single trailing newline does not add an empty element).
pub fn f_systemlist(argvars: &[typval_T], rettv: &mut typval_T) {
    let out = String::from_utf8_lossy(&get_cmd_output(argvars)).into_owned();
    let l = tv_list_alloc_ret(rettv, 0);
    let mut lb = l.borrow_mut();
    let trimmed = out.strip_suffix('\n').unwrap_or(&out);
    if !trimmed.is_empty() || out.contains('\n') {
        for line in trimmed.split('\n') {
            tv_list_append_string(&mut lb, line);
        }
    }
}

/// Port of `f_environ()` — a Dict of every environment variable. Uses the
/// OS-native form and lossily decodes non-UTF-8 names/values.
pub fn f_environ(_argvars: &[typval_T], rettv: &mut typval_T) {
    let d = tv_dict_alloc_ret(rettv);
    let mut db = d.borrow_mut();
    for (k, v) in std::env::vars_os() {
        tv_dict_add_str(&mut db, &k.to_string_lossy(), &v.to_string_lossy());
    }
}

// ── Buffer / window / tabpage builtins (no buffers or windows standalone) ──
//
// A standalone interpreter has no buffer list, windows, or tab pages, so these
// reduce to the value their C bodies (vendor/eval/buffer.c, window.c) return when
// the looked-up buffer/window is absent: a missing buffer is -1 / 0 / "", a
// window measurement is -1, and there is one implicit window and tab page.

/// Port of `f_bufnr()` (buffer.c) — no such buffer → -1.
pub fn f_bufnr(_argvars: &[typval_T], rettv: &mut typval_T) {
    // Embedded: the host's current-buffer number; standalone: no buffers -> -1.
    let n = crate::fusevm_bridge::editor_buf_nr().unwrap_or(-1);
    *rettv = typval_T::from(n);
}
/// Port of `f_bufexists()` (buffer.c) — no buffers → 0.
pub fn f_bufexists(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_buflisted()` (buffer.c) — no buffers → 0.
pub fn f_buflisted(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_bufloaded()` (buffer.c) — no buffers → 0.
pub fn f_bufloaded(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_bufname()` (buffer.c) — no buffer → "" (the C NULL string).
pub fn f_bufname(_argvars: &[typval_T], rettv: &mut typval_T) {
    // Embedded: the host's current-buffer name; standalone: "" (C NULL string).
    let name = crate::fusevm_bridge::editor_buf_name().unwrap_or_default();
    *rettv = typval_T::from(name);
}
/// Port of `f_bufwinnr()`/`buf_win_common()` (buffer.c) — no window → -1.
pub fn f_bufwinnr(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
}
/// Port of `f_bufwinid()`/`buf_win_common()` (buffer.c) — no window → -1.
pub fn f_bufwinid(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
}
/// Port of `f_winnr()` (window.c) — the single implicit window → 1.
pub fn f_winnr(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(1 as varnumber_T);
}
/// Port of `f_winbufnr()` (window.c) — no buffer in the window → -1.
pub fn f_winbufnr(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
}
/// Port of `f_winwidth()` (window.c) — no measurable window → -1.
pub fn f_winwidth(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
}
/// Port of `f_winheight()` (window.c) — no measurable window → -1.
pub fn f_winheight(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
}
/// Port of `f_winlayout()` (window.c) — no window tree → empty List.
pub fn f_winlayout(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_list_alloc_ret(rettv, 0);
}
/// Port of `f_winline()` (window.c) — no screen → cursor window row 0.
pub fn f_winline(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_wincol()` (window.c) — no screen → cursor window column 0.
pub fn f_wincol(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_winrestcmd()` (window.c) — no windows to restore → "".
pub fn f_winrestcmd(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(String::new());
}
/// Port of `f_tabpagenr()` (window.c) — the single implicit tab page → 1.
pub fn f_tabpagenr(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(1 as varnumber_T);
}
/// Port of `f_tabpagewinnr()` (window.c) — one window in the tab page → 1.
pub fn f_tabpagewinnr(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(1 as varnumber_T);
}

// ── More buffer/window builtins (no buffer lines or real windows standalone) ──
//
// Faithful to the C "absent" returns (vendor/eval/buffer.c, window.c): reading a
// buffer line yields "" / []; a line-changing command FAILs with 1; window
// queries yield no id (0) / -1 / [] / [0,0]; GUI position is [-1,-1].

/// Port of `f_getline()` (buffer.c) — no buffer: "" for a single line,
/// `[]` for the two-arg (range) List form.
pub fn f_getline(argvars: &[typval_T], rettv: &mut typval_T) {
    let lnum = tv_get_lnum(&argvars[0]);
    if argvars.len() >= 2 && argvars[1].v_type != VAR_UNKNOWN {
        // c: two arguments → a List of lines lnum..=end.
        let end = tv_get_lnum(&argvars[1]);
        let lines = get_buffer_lines(lnum, end);
        let l = tv_list_alloc_ret(rettv, lines.len() as isize);
        let mut lb = l.borrow_mut();
        for line in lines {
            tv_list_append_string(&mut lb, &line);
        }
    } else {
        let s = get_buffer_lines(lnum, lnum)
            .into_iter()
            .next()
            .unwrap_or_default();
        *rettv = typval_T::from(s);
    }
}
/// Port of `f_getbufline()` (buffer.c) — buffer lines `{lnum}`..`{end}` as a
/// List. vimlrs has a single virtual buffer, so `{buf}` is ignored.
pub fn f_getbufline(argvars: &[typval_T], rettv: &mut typval_T) {
    let lnum = tv_get_lnum(&argvars[1]);
    let end = if argvars.len() >= 3 && argvars[2].v_type != VAR_UNKNOWN {
        tv_get_lnum(&argvars[2])
    } else {
        lnum
    };
    let lines = get_buffer_lines(lnum, end);
    let l = tv_list_alloc_ret(rettv, lines.len() as isize);
    let mut lb = l.borrow_mut();
    for line in lines {
        tv_list_append_string(&mut lb, &line);
    }
}
/// Port of `f_getbufoneline()` (buffer.c) — the single buffer line `{lnum}`.
pub fn f_getbufoneline(argvars: &[typval_T], rettv: &mut typval_T) {
    let lnum = tv_get_lnum(&argvars[1]);
    let s = get_buffer_lines(lnum, lnum)
        .into_iter()
        .next()
        .unwrap_or_default();
    *rettv = typval_T::from(s);
}
/// Port of `f_getbufinfo()` (buffer.c) — no buffers → empty List.
pub fn f_getbufinfo(_argvars: &[typval_T], rettv: &mut typval_T) {
    // vimlrs has a single virtual buffer (number 1); report it.
    let out = tv_list_alloc_ret(rettv, 1);
    let mut ob = out.borrow_mut();
    let d = tv_dict_alloc();
    {
        let mut db = d.borrow_mut();
        tv_dict_add_nr(&mut db, "bufnr", 1);
        tv_dict_add_str(&mut db, "name", "");
        tv_dict_add_nr(
            &mut db,
            "lnum",
            crate::fusevm_bridge::editor_curpos(CURPOS.with(|c| *c.borrow())).0,
        );
        tv_dict_add_nr(&mut db, "linecount", curbuf_len());
        tv_dict_add_nr(&mut db, "loaded", 1);
        tv_dict_add_nr(&mut db, "listed", 1);
        tv_dict_add_nr(&mut db, "hidden", 0);
        tv_dict_add_nr(&mut db, "changed", 0);
        tv_dict_add_nr(&mut db, "changedtick", 1);
        let empty_list = |db: &mut crate::ported::eval::typval_defs_h::dict_T, k: &str| {
            let l = tv_list_alloc(0);
            tv_dict_add_tv(
                db,
                k,
                typval_T {
                    v_type: VAR_LIST,
                    v_lock: VarLockStatus::VAR_UNLOCKED,
                    vval: v_list(Some(l)),
                },
            );
        };
        empty_list(&mut db, "windows");
        empty_list(&mut db, "popups");
    }
    tv_list_append_tv(&mut ob, match_dict_val(d));
}
/// Port of `f_setline()`/`set_buffer_lines()` (buffer.c) — replace line(s) from
/// `{lnum}` with `{text}` (a String or List). Returns 0 on success, 1 on error.
pub fn f_setline(argvars: &[typval_T], rettv: &mut typval_T) {
    let lnum = tv_get_lnum(&argvars[0]);
    let r = set_buffer_lines(lnum, tv_lines_arg(&argvars[1]), false);
    *rettv = typval_T::from(r);
}
/// Port of `f_setbufline()` (buffer.c) — like `setline()` (single buffer).
pub fn f_setbufline(argvars: &[typval_T], rettv: &mut typval_T) {
    let lnum = tv_get_lnum(&argvars[1]);
    let r = set_buffer_lines(lnum, tv_lines_arg(&argvars[2]), false);
    *rettv = typval_T::from(r);
}
/// Port of `f_append()` (buffer.c) — insert `{text}` after line `{lnum}` (0 =
/// before the first line). Returns 0 on success.
pub fn f_append(argvars: &[typval_T], rettv: &mut typval_T) {
    let lnum = tv_get_lnum(&argvars[0]);
    let r = set_buffer_lines(lnum, tv_lines_arg(&argvars[1]), true);
    *rettv = typval_T::from(r);
}
/// Port of `f_appendbufline()` (buffer.c) — like `append()` (single buffer).
pub fn f_appendbufline(argvars: &[typval_T], rettv: &mut typval_T) {
    let lnum = tv_get_lnum(&argvars[1]);
    let r = set_buffer_lines(lnum, tv_lines_arg(&argvars[2]), true);
    *rettv = typval_T::from(r);
}
/// Port of `f_deletebufline()` (buffer.c) — delete lines `{first}`..`{last}`
/// (single buffer). An emptied buffer keeps one empty line. Returns 0 on
/// success, 1 if the range is invalid.
pub fn f_deletebufline(argvars: &[typval_T], rettv: &mut typval_T) {
    let first = tv_get_lnum(&argvars[1]);
    let last = if argvars.len() >= 3 && argvars[2].v_type != VAR_UNKNOWN {
        tv_get_lnum(&argvars[2])
    } else {
        first
    };
    let len = curbuf_len();
    if first < 1 || first > len || last < first {
        *rettv = typval_T::from(1 as varnumber_T);
        return;
    }
    CURBUF.with(|b| {
        let mut b = b.borrow_mut();
        if b.is_empty() {
            b.push(String::new());
        }
        let lo = (first - 1) as usize;
        let hi = (last.min(b.len() as varnumber_T)) as usize;
        b.drain(lo..hi);
        if b.is_empty() {
            b.push(String::new());
        }
    });
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_getwininfo()` (window.c) — no windows → empty List.
pub fn f_getwininfo(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_list_alloc_ret(rettv, 0);
}
/// Port of `f_gettabinfo()` (window.c) — no tab pages → empty List.
pub fn f_gettabinfo(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_list_alloc_ret(rettv, 0);
}
/// Port of `f_getwinpos()` (window.c) — no GUI → `[-1, -1]`.
pub fn f_getwinpos(_argvars: &[typval_T], rettv: &mut typval_T) {
    let l = tv_list_alloc_ret(rettv, 2);
    let mut lb = l.borrow_mut();
    tv_list_append_number(&mut lb, -1);
    tv_list_append_number(&mut lb, -1);
}
/// Port of `f_getwinposx()` (window.c) — no GUI → -1.
pub fn f_getwinposx(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
}
/// Port of `f_getwinposy()` (window.c) — no GUI → -1.
pub fn f_getwinposy(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
}
/// Port of `f_win_getid()` (window.c) — no window → 0.
pub fn f_win_getid(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_win_id2win()` (window.c) — id not found → 0.
pub fn f_win_id2win(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_win_findbuf()` (window.c) — no windows → empty List.
pub fn f_win_findbuf(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_list_alloc_ret(rettv, 0);
}
/// Port of `f_win_gotoid()` (window.c) — no window to go to → 0 (FAIL).
pub fn f_win_gotoid(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_win_gettype()` (window.c) — id invalid (no window) → "unknown".
pub fn f_win_gettype(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from("unknown".to_string());
}
/// Port of `f_win_screenpos()` (window.c) — no window → `[0, 0]`.
pub fn f_win_screenpos(_argvars: &[typval_T], rettv: &mut typval_T) {
    let l = tv_list_alloc_ret(rettv, 2);
    let mut lb = l.borrow_mut();
    tv_list_append_number(&mut lb, 0);
    tv_list_append_number(&mut lb, 0);
}

// ── Window-view / prompt / server / context builtins (inactive standalone) ──

/// Port of `f_win_id2tabwin()` (window.c) — id not found → `[0, 0]`.
pub fn f_win_id2tabwin(_argvars: &[typval_T], rettv: &mut typval_T) {
    let l = tv_list_alloc_ret(rettv, 2);
    let mut lb = l.borrow_mut();
    tv_list_append_number(&mut lb, 0);
    tv_list_append_number(&mut lb, 0);
}
/// Port of `f_win_splitmove()` (window.c) — no window → -1 (FAIL).
pub fn f_win_splitmove(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
}
/// Port of `f_win_move_separator()` (window.c) — no window → 0 (false).
pub fn f_win_move_separator(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_win_move_statusline()` (window.c) — no window → 0 (false).
pub fn f_win_move_statusline(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_getcmdwintype()` (window.c) — not in the command-line window → "".
pub fn f_getcmdwintype(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(String::new());
}
/// Port of `f_winrestview()` (window.c) — no window to restore → no-op (0).
pub fn f_winrestview(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_winsaveview()` (window.c) — the view Dict for the implicit window
/// at the origin: line 1, everything else 0.
pub fn f_winsaveview(_argvars: &[typval_T], rettv: &mut typval_T) {
    let d = tv_dict_alloc_ret(rettv);
    let mut db = d.borrow_mut();
    tv_dict_add_nr(&mut db, "lnum", 1);
    tv_dict_add_nr(&mut db, "col", 0);
    tv_dict_add_nr(&mut db, "coladd", 0);
    tv_dict_add_nr(&mut db, "curswant", 0);
    tv_dict_add_nr(&mut db, "topline", 1);
    tv_dict_add_nr(&mut db, "topfill", 0);
    tv_dict_add_nr(&mut db, "leftcol", 0);
    tv_dict_add_nr(&mut db, "skipcol", 0);
}
/// Port of `f_bufload()` (buffer.c) — no buffers to load → no-op (0).
pub fn f_bufload(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_prompt_getinput()` (buffer.c) — no prompt buffer → "".
pub fn f_prompt_getinput(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(String::new());
}
/// Port of `f_prompt_setprompt()` (buffer.c) — no prompt buffer → no-op (0).
pub fn f_prompt_setprompt(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_prompt_setcallback()` (buffer.c) — no prompt buffer → no-op (0).
pub fn f_prompt_setcallback(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_prompt_setinterrupt()` (buffer.c) — no prompt buffer → no-op (0).
pub fn f_prompt_setinterrupt(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_interrupt()` (funcs.c) — sets `got_int`; the standalone
/// interpreter has no interactive interrupt to raise, so it is a no-op (0).
pub fn f_interrupt(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_debugbreak()` (funcs.c) — no process to signal → FAIL (0).
pub fn f_debugbreak(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_api_info()` (funcs.c) — no embedded API → empty Dict.
pub fn f_api_info(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_dict_alloc_ret(rettv);
}
/// Port of `f_swapinfo()`/`swapfile_dict()` (funcs.c) — no swap file to read →
/// `{error: 'Cannot open file'}`.
pub fn f_swapinfo(_argvars: &[typval_T], rettv: &mut typval_T) {
    let d = tv_dict_alloc_ret(rettv);
    tv_dict_add_str(&mut d.borrow_mut(), "error", "Cannot open file");
}
/// Port of `f_serverstart()` (funcs.c) — no server standalone → "" (the C NULL).
pub fn f_serverstart(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(String::new());
}
/// Port of `f_serverstop()` (funcs.c) — no server → no-op (0).
pub fn f_serverstop(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}

// ── Scoped variables / jobs / channels (no buffers, windows, or event loop) ──
//
// Scoped-var getters (vars.c `get_var_from`) return the {def} argument when the
// variable is absent (always, standalone), else ""; setters are no-ops. Jobs,
// channels, and sockets need an event loop the standalone interpreter does not
// run, so they fail (-1) or are no-ops (0); jobwait returns an empty List.

/// `{def}` argument at `idx`, or "" when absent — the `get_var_from` fallback.
fn get_var_from(argvars: &[typval_T], idx: usize) -> typval_T {
    match argvars.get(idx) {
        Some(d) if d.v_type != VAR_UNKNOWN => d.clone(),
        _ => typval_T::from(String::new()),
    }
}
/// Port of `f_getbufvar()` (vars.c) — no buffer → `{def}` (arg 2) or "".
pub fn f_getbufvar(argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = get_var_from(argvars, 2);
}
/// Port of `f_getwinvar()` (vars.c) — no window → `{def}` (arg 2) or "".
pub fn f_getwinvar(argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = get_var_from(argvars, 2);
}
/// Port of `f_gettabvar()` (vars.c) — no tab page → `{def}` (arg 2) or "".
pub fn f_gettabvar(argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = get_var_from(argvars, 2);
}
/// Port of `f_gettabwinvar()` (vars.c) — no tab/window → `{def}` (arg 3) or "".
pub fn f_gettabwinvar(argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = get_var_from(argvars, 3);
}
/// Port of `f_setbufvar()` (vars.c) — no buffer → no-op (0).
pub fn f_setbufvar(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_setwinvar()` (vars.c) — no window → no-op (0).
pub fn f_setwinvar(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_settabvar()` (vars.c) — no tab page → no-op (0).
pub fn f_settabvar(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_settabwinvar()` (vars.c) — no tab/window → no-op (0).
pub fn f_settabwinvar(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_jobstart()` (funcs.c) — no event loop to run the job → -1.
pub fn f_jobstart(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
}
/// Port of `f_jobpid()` (funcs.c) — no job → 0.
pub fn f_jobpid(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_jobstop()` (funcs.c) — no job to stop → 0.
pub fn f_jobstop(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_jobwait()` (funcs.c) — no jobs → empty List.
pub fn f_jobwait(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_list_alloc_ret(rettv, 0);
}
/// Port of `f_jobresize()` (funcs.c) — no job → 0.
pub fn f_jobresize(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_chanclose()` (funcs.c) — no channel → 0.
pub fn f_chanclose(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_chansend()` (funcs.c) — no channel → 0 bytes sent.
pub fn f_chansend(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_feedkeys()` (funcs.c) — no typeahead buffer → no-op (0).
pub fn f_feedkeys(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_wait()` (funcs.c) — no event loop → -1 (the C error default).
pub fn f_wait(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
}
/// Port of `f_sockconnect()` (funcs.c) — no event loop → 0 (no channel).
pub fn f_sockconnect(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_win_execute()` (window.c) — no window to run the command in → "".
pub fn f_win_execute(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(String::new());
}
/// Port of `f_bufadd()` (buffer.c) — no buffer list standalone → 0.
pub fn f_bufadd(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}

// ── Context stack / providers / RPC / misc (inactive standalone) ──

/// Port of `f_ctxget()` (funcs.c) — empty context stack → empty Dict.
pub fn f_ctxget(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_dict_alloc_ret(rettv);
}
/// Port of `f_ctxpop()` (funcs.c) — nothing to pop → no-op (0).
pub fn f_ctxpop(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_ctxpush()` (funcs.c) — no-op (0).
pub fn f_ctxpush(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_ctxset()` (funcs.c) — no-op (0).
pub fn f_ctxset(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_ctxsize()`/`ctx_size()` (funcs.c) — empty stack → 0.
pub fn f_ctxsize(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_islocked()` (`eval/funcs.c:3223`) — `1` if the variable named by
/// `argvars[0]` is `:lockvar`-locked, `0` if unlocked, `-1` if it does not
/// exist. Parses the name with [`get_lval`] (`GLV_NO_AUTOLOAD | GLV_READ_ONLY`)
/// and reads the lock via [`tv_islocked`].
///
/// RUST-PORT NOTE: `di_flags` is not modeled, so the C
/// `(di->di_flags & DI_FLAGS_LOCK)` half of the scalar check is elided — a
/// scalar `:lockvar` stores its lock on the value's `v_lock` (see
/// [`crate::ported::eval::typval::tv_item_lock`]), which `tv_islocked` reads.
pub fn f_islocked(argvars: &[typval_T], rettv: &mut typval_T) {
    use crate::ported::eval::typval::tv_islocked;
    use crate::ported::eval::vars::find_var;
    use crate::ported::eval::{get_lval, lval_T, FNE_CHECK_START, GLV_NO_AUTOLOAD, GLV_READ_ONLY};

    // c:3227 rettv->vval.v_number = -1;
    *rettv = typval_T::from(-1 as varnumber_T);

    let name = tv_get_string(&argvars[0]);
    let mut lv = lval_T::default();
    // c:3228 end = get_lval(name, NULL, &lv, false, false, GLV_NO_AUTOLOAD|GLV_READ_ONLY, FNE_CHECK_START);
    let end = get_lval(
        &name,
        None,
        &mut lv,
        false,
        false,
        GLV_NO_AUTOLOAD | GLV_READ_ONLY,
        FNE_CHECK_START,
    );

    // c:3233 if (end != NULL && lv.ll_name != NULL)
    if let (Some(end_off), Some(_)) = (end, lv.ll_name.as_ref()) {
        if end_off < name.len() {
            // c:3234 *end != NUL → invalid/trailing argument.
            let rest = &name[end_off..];
            if lv.ll_name_len == 0 {
                crate::ported::message::semsg(&format!("E475: Invalid argument: {rest}"));
            } else {
                crate::ported::message::semsg(&format!("E488: Trailing characters: {rest}"));
            }
        } else if matches!(lv.ll_tv, crate::ported::eval::LlTv::Null) {
            // c:3237 lv.ll_tv == NULL → a plain variable.
            if let Some(di_tv) = find_var(lv.ll_name.as_ref().unwrap(), true) {
                // c:3244 (di->di_flags & DI_FLAGS_LOCK) || tv_islocked(&di->di_tv);
                //        the DI_FLAGS_LOCK half is elided (di_flags not modeled).
                rettv.vval = v_number(tv_islocked(&di_tv) as varnumber_T);
            }
        } else if lv.ll_range {
            // c:3247
            emsg("E786: Range not allowed");
        } else if let Some(newkey) = lv.ll_newkey.as_ref() {
            // c:3249 semsg(e_dictkey, lv.ll_newkey);
            crate::ported::message::semsg(&format!(
                "E716: Key not present in Dictionary: {newkey}"
            ));
        } else if let (Some(l), Some(li)) = (lv.ll_list.as_ref(), lv.ll_li) {
            // c:3251 List item — tv_islocked(TV_LIST_ITEM_TV(lv.ll_li)).
            if let Some(item) = l.borrow().lv_items.get(li) {
                rettv.vval = v_number(tv_islocked(&item.li_tv) as varnumber_T);
            }
        } else if let (Some(d), Some(key)) = (lv.ll_dict.as_ref(), lv.ll_di.as_ref()) {
            // c:3254 Dictionary item — tv_islocked(&lv.ll_di->di_tv).
            if let Some(item) = d.borrow().dv_hashtab.get(key) {
                rettv.vval = v_number(tv_islocked(item) as varnumber_T);
            }
        }
    }
}
/// Port of `f_last_buffer_nr()` (buffer.c) — no buffers → 0.
pub fn f_last_buffer_nr(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_libcall()`/`libcall_common()` (funcs.c) — no dynamic library
/// loading → "".
pub fn f_libcall(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(String::new());
}
/// Port of `f_libcallnr()` (funcs.c) — no dynamic library loading → 0.
pub fn f_libcallnr(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
// ── msgpack: msgpackdump()/msgpackparse() — eval/funcs.c dispatch over the
//    encode.c (`encode_vim_to_msgpack`) / decode.c (`unpack_typval`) codecs.
//
//    Faithful port of the MessagePack value↔typval mapping for the lossless,
//    self-describing subset, matching msgpack-c's minimal-width packing so the
//    bytes are identical to Neovim's:
//      nil↔v:null  bool↔v:true/v:false  int↔Number  float64↔Float
//      str↔Dict keys (always STR on dump)  bin↔String/Blob (always BIN on dump)
//      array↔List  map(string keys)↔Dict
//    On dump a BIN/STR is read back by msgpackparse as a Blob/String, so (per
//    the C's documented limitation 3/4) String→Blob is intentionally lossy.
//
//    Architecture note: this crate stores VAR_STRING as a UTF-8 `String`, so the
//    raw msgpack byte stream is exact only in the Blob form (`msgpackdump(l,'B')`
//    / `msgpackparse(0z…)`). The readfile()-style list form reuses the project's
//    existing text convention (split/join on '\n', `from_utf8_lossy`) and so is
//    exact only for text/number payloads — the same fidelity as readfile() here.

/// Append the minimal-width MessagePack encoding of `tv` to `out`. Mirrors
/// `encode_vim_to_msgpack()` (encode.c) over msgpack-c's `msgpack_pack_*`.
/// Returns the Vim error string (E5004/E5005) for an unencodable value.
fn mpack_encode_tv(tv: &typval_T, out: &mut Vec<u8>) -> Result<(), &'static str> {
    match (tv.v_type, &tv.vval) {
        // c: every special (v:null / v:none) encodes as msgpack nil; v:true /
        // v:false → bool.
        (VAR_SPECIAL, _) => out.push(0xc0),
        (VAR_BOOL, v_bool(b)) => out.push(if *b == kBoolVarTrue { 0xc3 } else { 0xc2 }),
        (VAR_NUMBER, v_number(n)) => mpack_pack_int(*n, out),
        // c: msgpack_pack_double — Float is always float64.
        (VAR_FLOAT, v_float(f)) => {
            out.push(0xcb);
            out.extend_from_slice(&f.to_bits().to_be_bytes());
        }
        // c: dict keys / encode_vim_to_msgpack STR path. Strings dump as STR here
        // only when used as a map key; a standalone String dumps as BIN (limit 4).
        (VAR_STRING, v_string(s)) => mpack_pack_bin(s.as_bytes(), out),
        (VAR_BLOB, v_blob(b)) => {
            let bytes = b
                .as_ref()
                .map(|b| b.borrow().bv_ga.clone())
                .unwrap_or_default();
            mpack_pack_bin(&bytes, out);
        }
        (VAR_LIST, v_list(l)) => {
            let items: Vec<typval_T> = l
                .as_ref()
                .map(|l| {
                    l.borrow()
                        .lv_items
                        .iter()
                        .map(|it| it.li_tv.clone())
                        .collect()
                })
                .unwrap_or_default();
            mpack_pack_array_len(items.len(), out);
            for it in &items {
                mpack_encode_tv(it, out)?;
            }
        }
        (VAR_DICT, v_dict(d)) => {
            let pairs: Vec<(String, typval_T)> = d
                .as_ref()
                .map(|d| {
                    d.borrow()
                        .dv_hashtab
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect()
                })
                .unwrap_or_default();
            mpack_pack_map_len(pairs.len(), out);
            for (k, v) in &pairs {
                // c: keys are always dumped as STR strings (limitation 3).
                mpack_pack_str(k.as_bytes(), out);
                mpack_encode_tv(v, out)?;
            }
        }
        // c: E5004 — Funcref/Partial cannot be dumped.
        (VAR_FUNC, _) | (VAR_PARTIAL, _) => {
            return Err("E5004: Error while dumping: attempt to dump function reference")
        }
        _ => return Err("E5004: Error while dumping: attempt to dump unsupported type"),
    }
    Ok(())
}

/// `msgpack_pack_int64` — minimal signed/unsigned width (non-negative ≥128 use
/// the UINT family, negatives use the INT family), matching msgpack-c exactly.
fn mpack_pack_int(n: varnumber_T, out: &mut Vec<u8>) {
    if n >= 0 {
        let u = n as u64;
        if u < 0x80 {
            out.push(u as u8); // positive fixint
        } else if u <= 0xff {
            out.push(0xcc);
            out.push(u as u8);
        } else if u <= 0xffff {
            out.push(0xcd);
            out.extend_from_slice(&(u as u16).to_be_bytes());
        } else if u <= 0xffff_ffff {
            out.push(0xce);
            out.extend_from_slice(&(u as u32).to_be_bytes());
        } else {
            out.push(0xcf);
            out.extend_from_slice(&u.to_be_bytes());
        }
    } else if n >= -32 {
        out.push((n as i8) as u8); // negative fixint (0xe0..0xff)
    } else if n >= -128 {
        out.push(0xd0);
        out.push((n as i8) as u8);
    } else if n >= -32768 {
        out.push(0xd1);
        out.extend_from_slice(&(n as i16).to_be_bytes());
    } else if n >= -(1i64 << 31) {
        out.push(0xd2);
        out.extend_from_slice(&(n as i32).to_be_bytes());
    } else {
        out.push(0xd3);
        out.extend_from_slice(&n.to_be_bytes());
    }
}

fn mpack_pack_str(b: &[u8], out: &mut Vec<u8>) {
    let n = b.len();
    if n < 32 {
        out.push(0xa0 | n as u8);
    } else if n <= 0xff {
        out.push(0xd9);
        out.push(n as u8);
    } else if n <= 0xffff {
        out.push(0xda);
        out.extend_from_slice(&(n as u16).to_be_bytes());
    } else {
        out.push(0xdb);
        out.extend_from_slice(&(n as u32).to_be_bytes());
    }
    out.extend_from_slice(b);
}

fn mpack_pack_bin(b: &[u8], out: &mut Vec<u8>) {
    let n = b.len();
    if n <= 0xff {
        out.push(0xc4);
        out.push(n as u8);
    } else if n <= 0xffff {
        out.push(0xc5);
        out.extend_from_slice(&(n as u16).to_be_bytes());
    } else {
        out.push(0xc6);
        out.extend_from_slice(&(n as u32).to_be_bytes());
    }
    out.extend_from_slice(b);
}

fn mpack_pack_array_len(n: usize, out: &mut Vec<u8>) {
    if n < 16 {
        out.push(0x90 | n as u8);
    } else if n <= 0xffff {
        out.push(0xdc);
        out.extend_from_slice(&(n as u16).to_be_bytes());
    } else {
        out.push(0xdd);
        out.extend_from_slice(&(n as u32).to_be_bytes());
    }
}

fn mpack_pack_map_len(n: usize, out: &mut Vec<u8>) {
    if n < 16 {
        out.push(0x80 | n as u8);
    } else if n <= 0xffff {
        out.push(0xde);
        out.extend_from_slice(&(n as u16).to_be_bytes());
    } else {
        out.push(0xdf);
        out.extend_from_slice(&(n as u32).to_be_bytes());
    }
}

/// Collect the input byte stream of `msgpackparse()`: a Blob is taken verbatim;
/// a readfile()-style List is joined on '\n' (the project's text convention).
fn mpack_input_bytes(tv: &typval_T) -> Result<Vec<u8>, &'static str> {
    match (tv.v_type, &tv.vval) {
        (VAR_BLOB, v_blob(b)) => Ok(b
            .as_ref()
            .map(|b| b.borrow().bv_ga.clone())
            .unwrap_or_default()),
        (VAR_LIST, v_list(l)) => {
            let items: Vec<String> = l
                .as_ref()
                .map(|l| {
                    l.borrow()
                        .lv_items
                        .iter()
                        .map(|it| tv_get_string(&it.li_tv))
                        .collect()
                })
                .unwrap_or_default();
            Ok(items.join("\n").into_bytes())
        }
        _ => Err("E5070: msgpackparse() argument must be a List or Blob"),
    }
}

/// Port of `f_msgpackdump()` (funcs.c → encode.c). Encode a List of objects to
/// MessagePack. The default return is a readfile()-style List; when `{type}`
/// contains "B" a Blob is returned instead. `Funcref`s raise E5004.
pub fn f_msgpackdump(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: first argument must be a List of objects to dump.
    let items: Vec<typval_T> = match &argvars[0].vval {
        v_list(Some(l)) => l
            .borrow()
            .lv_items
            .iter()
            .map(|it| it.li_tv.clone())
            .collect(),
        _ => {
            emsg("E1211: List required for argument 1");
            tv_list_alloc_ret(rettv, 0);
            return;
        }
    };
    let want_blob = argvars.len() > 1 && tv_get_string(&argvars[1]).contains('B');
    let mut bytes = Vec::new();
    for it in &items {
        if let Err(e) = mpack_encode_tv(it, &mut bytes) {
            emsg(e);
            tv_list_alloc_ret(rettv, 0);
            return;
        }
    }
    if want_blob {
        let blob = tv_blob_alloc_ret(rettv);
        blob.borrow_mut().bv_ga = bytes;
        return;
    }
    // readfile()-style List: split the byte stream on '\n' (project convention).
    // An empty stream yields an empty List (as readfile() of an empty file does),
    // not a single empty line.
    let l = tv_list_alloc_ret(rettv, 0);
    if bytes.is_empty() {
        return;
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut lb = l.borrow_mut();
    for line in text.split('\n') {
        tv_list_append_string(&mut lb, line);
    }
}

/// Port of `f_msgpackparse()` (funcs.c → decode.c). Convert a readfile()-style
/// List or a Blob of MessagePack into a List of Vimscript objects.
pub fn f_msgpackparse(argvars: &[typval_T], rettv: &mut typval_T) {
    let bytes = match mpack_input_bytes(&argvars[0]) {
        Ok(b) => b,
        Err(e) => {
            emsg(e);
            tv_list_alloc_ret(rettv, 0);
            return;
        }
    };
    let l = tv_list_alloc_ret(rettv, 0);
    // Faithful decode.c path: unpack_typval advances the (data,size) cursor by
    // one top-level object per call (mpack_parse returns MPACK_OK after one).
    let mut data: &[u8] = &bytes;
    let mut size = bytes.len();
    while size > 0 {
        let mut item = typval_T::default();
        if crate::ported::eval::decode::unpack_typval(&mut data, &mut size, &mut item)
            != crate::ported::mpack::MPACK_OK
        {
            emsg("E5766: failed to parse msgpack string");
            break;
        }
        tv_list_append_tv(&mut l.borrow_mut(), item);
    }
}
/// Port of `f_rpcnotify()` (funcs.c) — no RPC channel → 0.
pub fn f_rpcnotify(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_rpcrequest()` (funcs.c) — no RPC channel → 0.
pub fn f_rpcrequest(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_rpcstart()` (funcs.c, deprecated) — no RPC → 0.
pub fn f_rpcstart(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_rpcstop()` (funcs.c) — no RPC → 0.
pub fn f_rpcstop(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_stdioopen()` (funcs.c) — no event loop → 0 (no channel).
pub fn f_stdioopen(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_submatch()` (funcs.c) — no active `:substitute` → "" (the List
/// form, `{list}` truthy, yields an empty List).
pub fn f_submatch(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: reg_submatch(no) — text of group `no` of the match a `:s//\=…/` /
    // substitute(…, '\=…') expression is currently replacing.
    let no = tv_get_number_chk(&argvars[0], None).max(0) as usize;
    if argvars.len() >= 2 && argvars[1].v_type != VAR_UNKNOWN && tv_get_bool(&argvars[1]) != 0 {
        // {list} form: the match text split into lines. With no active match
        // (called outside a `\=` expression) this is an empty List.
        let l = tv_list_alloc_ret(rettv, 0);
        if crate::viml_regex::has_submatch_context() {
            let mut lb = l.borrow_mut();
            for line in crate::viml_regex::current_submatch(no).split('\n') {
                tv_list_append_string(&mut lb, line);
            }
        }
    } else {
        *rettv = typval_T::from(crate::viml_regex::current_submatch(no));
    }
}
/// Port of `f_prompt_appendbuf()` (buffer.c) — no prompt buffer → no-op (0).
pub fn f_prompt_appendbuf(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_py3eval()` (funcs.c) — no Python provider → v:null.
pub fn f_py3eval(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_SPECIAL;
    rettv.vval = v_special(kSpecialVarNull);
}
/// Port of `f_perleval()` (funcs.c) — no Perl provider → v:null.
pub fn f_perleval(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_SPECIAL;
    rettv.vval = v_special(kSpecialVarNull);
}

// ── Final builtins: stdpath (XDG), GUI/provider/terminal absent ──

/// Port of `f_stdpath()` from `Src/eval/funcs.c` — the standard Nvim path of a
/// given kind, resolved from the XDG base-directory environment variables (with
/// the usual `~/.config`-style defaults) plus the `nvim` app subdirectory.
/// Port of `get_appname()` (Neovim `src/nvim/os/env.c`, home file not under the
/// vendored `vendor/eval/` tree). The application name used in the XDG paths:
/// `$NVIM_APPNAME` when set and non-empty, else "nvim".
fn get_appname() -> String {
    std::env::var("NVIM_APPNAME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "nvim".to_string())
}

/// Port of `get_xdg_var_list()` — `vendor/eval/funcs.c:7140`. Split the XDG
/// directory list in `$env` (or `default` when unset/empty) on the path
/// separator and append the appname to each entry. Used by stdpath()'s
/// `config_dirs`/`data_dirs`.
fn get_xdg_var_list(env: &str, default: &str) -> Vec<String> {
    let appname = get_appname();
    let val = std::env::var(env)
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default.to_string());
    val.split(':')
        .filter(|s| !s.is_empty())
        .map(|d| format!("{d}/{appname}"))
        .collect()
}

pub fn f_stdpath(argvars: &[typval_T], rettv: &mut typval_T) {
    let home = std::env::var("HOME").unwrap_or_default();
    let appname = get_appname();
    // c: stdpaths_get_xdg_var(xdg) then concat the appname.
    let xdg_home = |env: &str, default_rel: &str| -> String {
        let base = std::env::var(env)
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("{home}/{default_rel}"));
        format!("{base}/{appname}")
    };
    let kind = tv_get_string(&argvars[0]);
    match kind.as_str() {
        "config" => *rettv = typval_T::from(xdg_home("XDG_CONFIG_HOME", ".config")),
        "data" => *rettv = typval_T::from(xdg_home("XDG_DATA_HOME", ".local/share")),
        "cache" => *rettv = typval_T::from(xdg_home("XDG_CACHE_HOME", ".cache")),
        "state" => *rettv = typval_T::from(xdg_home("XDG_STATE_HOME", ".local/state")),
        "log" => {
            *rettv = typval_T::from(format!(
                "{}/logs",
                xdg_home("XDG_STATE_HOME", ".local/state")
            ))
        }
        "run" => {
            let run = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
            *rettv = typval_T::from(format!("{run}/{appname}"));
        }
        "config_dirs" | "data_dirs" => {
            let dirs = if kind == "config_dirs" {
                get_xdg_var_list("XDG_CONFIG_DIRS", "/etc/xdg")
            } else {
                get_xdg_var_list("XDG_DATA_DIRS", "/usr/local/share:/usr/share")
            };
            let l = tv_list_alloc_ret(rettv, dirs.len() as isize);
            let mut lb = l.borrow_mut();
            for d in &dirs {
                tv_list_append_string(&mut lb, d);
            }
        }
        _ => {
            emsg(&format!("E6100: \"{kind}\" is not a valid stdpath"));
            *rettv = typval_T::from(String::new());
        }
    }
}
/// Port of `f_keytrans()` (funcs.c) — translate key codes to a readable form;
/// plain text (the standalone case) passes through unchanged.
/// Port of `f_keytrans()` (`Src/eval/funcs.c`) — render a string in Vim's key
/// notation, i.e. `str2special_save(str, replace_spaces = true, replace_others)`
/// (`Src/message.c`), which maps each character through `get_special_key_name()`
/// (`Src/keycodes.c`) when it is a special key, a C0 control character, or a
/// space.
///
/// For the characters a *script* string can hold, that reduces to the rules
/// below (verified identical in Vim 9.2 and Neovim 0.12):
///
/// - `' '` → `<Space>`, `'<'` → `<lt>` (the `replace_spaces` / `replace_others`
///   cases; `|` and `\` are **not** replaced by this caller);
/// - a C0 control character → its table name (`<Tab>`, `<NL>`, `<CR>`, `<Esc>`)
///   or, with no table entry, `c + '@'` under the CTRL modifier: `0x01` → `<C-A>`,
///   `0x1f` → `<C-_>` (`get_special_key_name`: "if (table_idx < 0 && !vim_isprintc(c)
///   && c < ' ') { c += '@'; modifiers |= MOD_MASK_CTRL; }");
/// - everything else, including `0x7f` and multibyte text, passes through.
///
/// The `K_SPECIAL`-escaped terminal-key sequences that `str2special` also decodes
/// cannot occur here: they are produced by the terminal input layer, and a
/// standalone interpreter never sees one.
pub fn f_keytrans(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        out.push_str(&crate::ported::keycodes::get_special_key_name(c));
    }
    *rettv = typval_T::from(out);
}
/// Port of `f_luaeval()` (funcs.c) — no Lua provider standalone → v:null.
pub fn f_luaeval(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_SPECIAL;
    rettv.vval = v_special(kSpecialVarNull);
}
/// Port of `f_rubyeval()` (funcs.c) — no Ruby provider standalone → v:null.
pub fn f_rubyeval(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_SPECIAL;
    rettv.vval = v_special(kSpecialVarNull);
}
/// Port of `f_termopen()` (deprecated.c) — no terminal/event loop → -1.
pub fn f_termopen(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
}

/// Port of `has_wsl()` (funcs.c) — true under Windows Subsystem for Linux,
/// detected from the kernel release string (`microsoft`).
pub fn has_wsl() -> bool {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|s| s.to_lowercase().contains("microsoft"))
        .unwrap_or(false)
}

/// Port of `emsg_mpack_error()` (funcs.c) — report a msgpack decode error.
pub fn emsg_mpack_error(status: i32) {
    if status != 0 {
        emsg("E5004: Error while dumping or parsing msgpack");
    }
}

/// Port of `find_win_for_curbuf()` (buffer.c) — find a window showing the
/// current buffer; no windows standalone → no-op.
pub fn find_win_for_curbuf() {}

/// Port of `buf_win_common()` (buffer.c) — the shared body of `bufwinnr()`/
/// `bufwinid()`: no window shows the buffer → -1.
pub fn buf_win_common(_argvars: &[typval_T], rettv: &mut typval_T, _get_nr: bool) {
    *rettv = typval_T::from(-1 as varnumber_T);
}

// ════════════════════════════════════════════════════════════════════════════
// Round-1 builtin expansion. These builtins' C home files lie outside the
// vendored `vendor/eval/` tree (search.c, cmdhist.c, digraph.c, mbyte.c,
// testing.c, and the full eval/funcs.c table), so their `fn` names are recorded
// in `tests/data/fake_fn_allowlist.txt`. Each is a faithful port cited to its
// home file.
// ════════════════════════════════════════════════════════════════════════════

use crate::ported::eval::typval_defs_h::VarLockStatus;

// ── matchfuzzy()/matchfuzzypos() — Neovim fuzzy.c (fuzzy_match*, and the fzy
//    scoring engine it adapted from https://github.com/jhawthorn/fzy). Vim 9.2
//    ships the same algorithm: both oracles agree on every probed score
//    (895 for 'a'→'ab', 1895 for 'ba'→'bar', INT_MAX for a whole-string
//    match, -10 for 'a'→'bar'). ──

/// `FUZZY_MATCH_MAX_LEN` (fuzzy.h) — max characters that can be matched.
const FUZZY_MATCH_MAX_LEN: usize = 1024;
/// `FUZZY_SCORE_NONE = INT_MIN` (fuzzy.h) — invalid fuzzy score.
const FUZZY_SCORE_NONE: i32 = i32::MIN;

// c: fuzzy.c fzy scoring constants (score_t is double).
const FZY_SCORE_MAX: f64 = f64::INFINITY; // c: SCORE_MAX
const FZY_SCORE_MIN: f64 = f64::NEG_INFINITY; // c: SCORE_MIN
const FZY_SCORE_SCALE: f64 = 1000.0; // c: SCORE_SCALE
const SCORE_GAP_LEADING: f64 = -0.005;
const SCORE_GAP_TRAILING: f64 = -0.005;
const SCORE_GAP_INNER: f64 = -0.01;
const SCORE_MATCH_CONSECUTIVE: f64 = 1.0;
const SCORE_MATCH_SLASH: f64 = 0.9;
const SCORE_MATCH_WORD: f64 = 0.8;
const SCORE_MATCH_CAPITAL: f64 = 0.7;
const SCORE_MATCH_DOT: f64 = 0.6;

/// Port of `has_match()` (fuzzy.c) — do all needle chars occur in order in the
/// haystack? Both strings advance by `MB_PTR_ADV`/`utfc_ptr2len` cluster steps
/// (`utf_ptr2char` reads the base codepoint); the case rule is the C's exactly:
/// a needle char matches itself or its uppercase form (`mb_toupper`).
fn has_match(needle: &str, haystack: &str) -> bool {
    use crate::ported::mbyte::{mb_toupper, utfc_ptr2len};
    if needle.is_empty() {
        return false; // c: !*needle → FAIL
    }
    let hb = haystack.as_bytes();
    let mut h = 0usize;
    let nb = needle.as_bytes();
    let mut n = 0usize;
    while n < nb.len() {
        let n_char = needle[n..].chars().next().unwrap_or('\0');
        let n_upper = mb_toupper(n_char);
        let mut found = false;
        while h < hb.len() {
            let h_char = haystack[h..].chars().next().unwrap_or('\0');
            h += utfc_ptr2len(&hb[h..]).max(1) as usize;
            if n_char == h_char || n_upper == h_char {
                found = true;
                break;
            }
        }
        if !found {
            return false;
        }
        n += utfc_ptr2len(&nb[n..]).max(1) as usize;
    }
    true
}

/// Port of `compute_bonus_codepoint()` (fuzzy.c) — the positional bonus for a
/// match at a char preceded by `last_c`.
///
/// RUST-PORT NOTE: the C's `vim_iswordc(c)` follows 'iskeyword'
/// (`@,48-57,_,192-255` by default); `char::is_alphanumeric() || '_'` is the
/// standalone approximation, identical over ASCII.
fn compute_bonus_codepoint(last_c: char, c: char) -> f64 {
    if c.is_alphanumeric() || c == '_' {
        if last_c == '/' {
            return SCORE_MATCH_SLASH;
        }
        if last_c == '-' || last_c == '_' || last_c == ' ' {
            return SCORE_MATCH_WORD;
        }
        if last_c == '.' {
            return SCORE_MATCH_DOT;
        }
        if c.is_uppercase() && last_c.is_lowercase() {
            return SCORE_MATCH_CAPITAL;
        }
    }
    0.0
}

/// Port of `match_row()` (fuzzy.c) — fill one DP row of the fzy score
/// matrices. `D[j]`: best score ending with a match at column `j`; `M[j]`:
/// best score at column `j`.
#[allow(clippy::too_many_arguments)]
fn match_row(
    lower_needle: &[char],
    lower_haystack: &[char],
    match_bonus: &[f64],
    row: usize,
    curr_d: &mut [f64],
    curr_m: &mut [f64],
    last_d: &[f64],
    last_m: &[f64],
) {
    let n = lower_needle.len();
    let m = lower_haystack.len();
    let i = row;

    let mut prev_score = FZY_SCORE_MIN;
    let gap_score = if i == n - 1 {
        SCORE_GAP_TRAILING
    } else {
        SCORE_GAP_INNER
    };
    let mut prev_m = FZY_SCORE_MIN;
    let mut prev_d = FZY_SCORE_MIN;

    for j in 0..m {
        if lower_needle[i] == lower_haystack[j] {
            let mut score = FZY_SCORE_MIN;
            if i == 0 {
                score = (j as f64) * SCORE_GAP_LEADING + match_bonus[j];
            } else if j > 0 {
                // c: consecutive match, doesn't stack with match_bonus.
                score = (prev_m + match_bonus[j]).max(prev_d + SCORE_MATCH_CONSECUTIVE);
            }
            prev_d = last_d[j];
            prev_m = last_m[j];
            curr_d[j] = score;
            prev_score = score.max(prev_score + gap_score);
            curr_m[j] = prev_score;
        } else {
            prev_d = last_d[j];
            prev_m = last_m[j];
            curr_d[j] = FZY_SCORE_MIN;
            prev_score += gap_score;
            curr_m[j] = prev_score;
        }
    }
}

/// Port of `setup_match_struct()` (fuzzy.c) — the lowercased needle/haystack
/// codepoints (one per `MB_PTR_ADV` cluster, capped at `MATCH_MAX_LEN`) and
/// the per-position haystack bonus (the previous char seeds as '/').
fn setup_match_struct(needle: &str, haystack: &str) -> (Vec<char>, Vec<char>, Vec<f64>) {
    use crate::ported::mbyte::{mb_tolower, utfc_ptr2len};
    let mut lower_needle: Vec<char> = Vec::new();
    let nb = needle.as_bytes();
    let mut i = 0usize;
    while i < nb.len() && lower_needle.len() < FUZZY_MATCH_MAX_LEN {
        lower_needle.push(mb_tolower(needle[i..].chars().next().unwrap_or('\0')));
        i += utfc_ptr2len(&nb[i..]).max(1) as usize;
    }

    let mut lower_haystack: Vec<char> = Vec::new();
    let mut match_bonus: Vec<f64> = Vec::new();
    let hb = haystack.as_bytes();
    let mut prev_c = '/';
    i = 0;
    while i < hb.len() && lower_haystack.len() < FUZZY_MATCH_MAX_LEN {
        let c = haystack[i..].chars().next().unwrap_or('\0');
        lower_haystack.push(mb_tolower(c));
        match_bonus.push(compute_bonus_codepoint(prev_c, c));
        prev_c = c;
        i += utfc_ptr2len(&hb[i..]).max(1) as usize;
    }
    (lower_needle, lower_haystack, match_bonus)
}

/// Port of `match_positions()` (fuzzy.c) — the fzy DP over needle × haystack:
/// returns the match score (`SCORE_MAX` for a whole-string case-insensitive
/// match — the INT_MAX sentinel upstream) and writes the matched char
/// positions into `positions`.
fn match_positions(needle: &str, haystack: &str, positions: &mut [u32]) -> f64 {
    if needle.is_empty() {
        return FZY_SCORE_MIN;
    }

    let (lower_needle, lower_haystack, match_bonus) = setup_match_struct(needle, haystack);

    let n = lower_needle.len();
    let m = lower_haystack.len();

    if m > FUZZY_MATCH_MAX_LEN || n > m {
        // c: unreasonably large candidate — no score.
        return FZY_SCORE_MIN;
    }
    if n == m {
        // c: equal lengths + has_match precondition ⇒ the strings are equal
        // ignoring case — the SCORE_MAX shortcut (INT_MAX upstream). Checked
        // char-by-char because truncation can also make n == m.
        if lower_needle == lower_haystack {
            for (i, p) in positions.iter_mut().enumerate().take(n) {
                *p = i as u32;
            }
            return FZY_SCORE_MAX;
        }
    }

    // c: D[][] best score ending with a match here; M[][] best score here.
    let mut d = vec![FZY_SCORE_MIN; n * m];
    let mut mm = vec![FZY_SCORE_MIN; n * m];
    {
        let (d0, _) = d.split_at_mut(m);
        let (m0, _) = mm.split_at_mut(m);
        // c: match_row(&match, 0, D[0], M[0], D[0], M[0]) — row 0 never reads
        // last_D/last_M (i == 0 branch), so seed rows are fine.
        let seed = vec![FZY_SCORE_MIN; m];
        match_row(
            &lower_needle,
            &lower_haystack,
            &match_bonus,
            0,
            d0,
            m0,
            &seed,
            &seed,
        );
    }
    for i in 1..n {
        let (dprev, dcur) = d.split_at_mut(i * m);
        let (mprev, mcur) = mm.split_at_mut(i * m);
        match_row(
            &lower_needle,
            &lower_haystack,
            &match_bonus,
            i,
            &mut dcur[..m],
            &mut mcur[..m],
            &dprev[(i - 1) * m..],
            &mprev[(i - 1) * m..],
        );
    }

    // c: backtrace to find the positions of optimal matching.
    let mut match_required = false;
    let mut j = m as isize - 1;
    for i in (0..n).rev() {
        while j >= 0 {
            let ju = j as usize;
            if d[i * m + ju] != FZY_SCORE_MIN && (match_required || d[i * m + ju] == mm[i * m + ju])
            {
                // c: if this score was determined via SCORE_MATCH_CONSECUTIVE,
                // the previous character MUST be a match.
                match_required = i > 0
                    && ju > 0
                    && mm[i * m + ju] == d[(i - 1) * m + (ju - 1)] + SCORE_MATCH_CONSECUTIVE;
                if i < positions.len() {
                    positions[i] = ju as u32;
                }
                j -= 1;
                break;
            }
            j -= 1;
        }
    }

    mm[(n - 1) * m + (m - 1)]
}

/// Port of `fuzzy_match()` (fuzzy.c) — match `pat_arg` against `str`: each
/// space-separated word independently (all words at once with `matchseq`),
/// scores summed with INT_MAX/INT_MIN+1 saturation, matched char positions
/// appended to `matches`. Returns true when anything matched.
fn fuzzy_match(
    str_: &str,
    pat_arg: &str,
    matchseq: bool,
    out_score: &mut i32,
    matches: &mut [u32],
    max_matches: usize,
) -> bool {
    let mut complete = false;
    let mut num_matches = 0usize;

    *out_score = 0;

    let mut rest: &str = pat_arg;

    // c: try matching each word in "pat_arg" in "str".
    loop {
        let pat: &str;
        if matchseq {
            complete = true;
            pat = rest;
        } else {
            // c: extract one word from the pattern (separated by white space).
            rest = crate::ported::eval::skipwhite(rest);
            if rest.is_empty() {
                break;
            }
            let mut end = rest.len();
            let b = rest.as_bytes();
            let mut i = 0usize;
            while i < b.len() {
                let c = rest[i..].chars().next().unwrap_or('\0');
                if c == ' ' || c == '\t' {
                    end = i;
                    break;
                }
                i += crate::ported::mbyte::utfc_ptr2len(&b[i..]).max(1) as usize;
            }
            pat = &rest[..end];
            if end == rest.len() {
                complete = true; // c: processed all the words
            }
            rest = &rest[end..];
        }

        // c: match_positions() always writes pat_chars entries — bail if they
        // won't fit. `pat_chars = mb_charlen(pat)`.
        let mut pat_chars = crate::ported::mbyte::mb_charlen(pat) as usize;
        if pat_chars > max_matches {
            pat_chars = max_matches;
        }
        if num_matches > max_matches - pat_chars {
            num_matches = 0;
            *out_score = FUZZY_SCORE_NONE;
            break;
        }

        let mut score = FUZZY_SCORE_NONE;
        if has_match(pat, str_) {
            let fzy_score = match_positions(pat, str_, &mut matches[num_matches..]);
            if fzy_score != FZY_SCORE_MIN {
                score = if fzy_score == FZY_SCORE_MAX {
                    i32::MAX
                } else if fzy_score < 0.0 {
                    (fzy_score * FZY_SCORE_SCALE - 0.5).ceil() as i32
                } else {
                    (fzy_score * FZY_SCORE_SCALE + 0.5).floor() as i32
                };
            }
        }

        if score == FUZZY_SCORE_NONE {
            num_matches = 0;
            *out_score = FUZZY_SCORE_NONE;
            break;
        }

        // c: saturating accumulation across words.
        if score > 0 && *out_score > i32::MAX - score {
            *out_score = i32::MAX;
        } else if score < 0 && *out_score < i32::MIN + 1 - score {
            *out_score = i32::MIN + 1;
        } else {
            *out_score += score;
        }

        num_matches += pat_chars;

        if complete || num_matches >= max_matches {
            break;
        }
    }

    num_matches != 0
}

/// One scored entry of `fuzzy_match_in_list` (c: `fuzzyItem_T`).
struct FuzzyItem {
    /// c: `idx` — match order, the stable-sort key.
    idx: usize,
    /// Index of the item in the input list.
    item: usize,
    /// c: `score`.
    score: i32,
    /// c: `startpos` — `matches[0]`, used by the exact-match tiebreak.
    startpos: u32,
    /// c: `itemstr` — the matched text (for the exact-match tiebreak).
    itemstr: String,
    /// c: `lmatchpos` — the per-pattern-char matched positions.
    lmatchpos: Vec<u32>,
}

/// Port of `fuzzy_match_item_compare()` (fuzzy.c) — descending score; equal
/// scores put an exact substring match (the pattern verbatim at `startpos`)
/// first, then keep input order. The C indexes `itemstr + startpos` with a
/// *char* position — a byte/char conflation kept verbatim here.
fn fuzzy_match_item_compare(s1: &FuzzyItem, s2: &FuzzyItem, pat: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if s1.score != s2.score {
        return s2.score.cmp(&s1.score);
    }
    let exact = |s: &FuzzyItem| {
        s.itemstr
            .as_bytes()
            .get(s.startpos as usize..)
            .is_some_and(|rest| rest.starts_with(pat.as_bytes()))
    };
    let (e1, e2) = (exact(s1), exact(s2));
    if e1 == e2 {
        s1.idx.cmp(&s2.idx)
    } else if e2 {
        Ordering::Greater
    } else {
        Ordering::Less
    }
}

/// Port of `fuzzy_match_in_list()` (fuzzy.c) — score every String (or
/// Dict-`key`) item, honoring `max_matches` as a cap on the SCAN (the first N
/// matches in input order, sorted afterwards — not the top N by score), sort
/// with [`fuzzy_match_item_compare`], and fill `rettv`'s pre-shaped result.
#[allow(clippy::too_many_arguments)]
fn fuzzy_match_in_list(
    items: &[typval_T],
    pat: &str,
    matchseq: bool,
    key: Option<&str>,
    retmatchpos: bool,
    rettv: &mut typval_T,
    max_matches: i64,
) {
    let mut found: Vec<FuzzyItem> = Vec::new();
    let mut matches = vec![0u32; FUZZY_MATCH_MAX_LEN];

    for (item_idx, item) in items.iter().enumerate() {
        if max_matches > 0 && found.len() as i64 >= max_matches {
            break;
        }
        // c: the item itself (String) or the `key` field of a Dict item.
        let itemstr = match key {
            Some(k) => match (item.v_type, &item.vval) {
                (VAR_DICT, v_dict(Some(d))) => match tv_dict_find(&d.borrow(), k) {
                    Some(v) => tv_get_string(v),
                    None => continue,
                },
                _ => continue,
            },
            None => {
                if item.v_type == VAR_STRING {
                    tv_get_string(item)
                } else {
                    continue;
                }
            }
        };

        let mut score = 0i32;
        if fuzzy_match(
            &itemstr,
            pat,
            matchseq,
            &mut score,
            &mut matches,
            FUZZY_MATCH_MAX_LEN,
        ) {
            // c: copy the matching positions — one per non-whitespace pattern
            // char (every char with matchseq), walking the pattern by
            // `MB_PTR_ADV` cluster steps.
            let mut lmatchpos = Vec::new();
            if retmatchpos {
                let mut j = 0usize;
                let pb = pat.as_bytes();
                let mut i = 0usize;
                while i < pb.len() && j < FUZZY_MATCH_MAX_LEN {
                    let c = pat[i..].chars().next().unwrap_or('\0');
                    if !(c == ' ' || c == '\t') || matchseq {
                        lmatchpos.push(matches[j]);
                        j += 1;
                    }
                    i += crate::ported::mbyte::utfc_ptr2len(&pb[i..]).max(1) as usize;
                }
            }
            found.push(FuzzyItem {
                idx: found.len(),
                item: item_idx,
                score,
                startpos: matches[0],
                itemstr,
                lmatchpos,
            });
        }
    }

    if found.is_empty() {
        return;
    }
    // c: qsort(fuzzy_match_item_compare) — descending score with the
    // exact-match tiebreak.
    found.sort_by(|a, b| fuzzy_match_item_compare(a, b, pat));

    let mk = |l| typval_T {
        v_type: VAR_LIST,
        v_lock: VarLockStatus::VAR_UNLOCKED,
        vval: v_list(Some(l)),
    };
    if !retmatchpos {
        if let v_list(Some(out)) = &rettv.vval {
            let mut ob = out.borrow_mut();
            for f in &found {
                tv_list_append_tv(&mut ob, items[f.item].clone());
            }
        }
        return;
    }
    // c: matchfuzzypos() fills the three pre-created sub-lists.
    let matched = tv_list_alloc(0);
    let posl = tv_list_alloc(0);
    let scorel = tv_list_alloc(0);
    for f in &found {
        tv_list_append_tv(&mut matched.borrow_mut(), items[f.item].clone());
        let p = tv_list_alloc(0);
        for pos in &f.lmatchpos {
            tv_list_append_number(&mut p.borrow_mut(), *pos as varnumber_T);
        }
        tv_list_append_tv(&mut posl.borrow_mut(), mk(p));
        tv_list_append_number(&mut scorel.borrow_mut(), f.score as varnumber_T);
    }
    if let v_list(Some(outer)) = &rettv.vval {
        let mut ob = outer.borrow_mut();
        ob.lv_items.clear();
        tv_list_append_tv(&mut ob, mk(matched));
        tv_list_append_tv(&mut ob, mk(posl));
        tv_list_append_tv(&mut ob, mk(scorel));
    }
}

/// Port of `do_fuzzymatch()` (fuzzy.c) — the shared body of `matchfuzzy()`/
/// `matchfuzzypos()`: validate the arguments and options Dict, then delegate
/// to [`fuzzy_match_in_list`].
///
/// RUST-PORT NOTE: the `text_cb` Funcref option needs the Callback
/// infrastructure (`tv_dict_get_callback`) and is accepted but not consulted;
/// the `key` option covers the Dict-item form.
fn do_fuzzymatch(argvars: &[typval_T], rettv: &mut typval_T, retmatchpos: bool) {
    // c: validate and get the arguments.
    let items: Vec<typval_T> = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => l
            .borrow()
            .lv_items
            .iter()
            .map(|it| it.li_tv.clone())
            .collect(),
        _ => {
            crate::ported::message::semsg(&format!(
                "E686: Argument of {} must be a List",
                if retmatchpos {
                    "matchfuzzypos()"
                } else {
                    "matchfuzzy()"
                }
            ));
            return;
        }
    };
    if argvars[1].v_type != VAR_STRING {
        crate::ported::message::semsg(&format!(
            "E475: Invalid argument: {}",
            tv_get_string(&argvars[1])
        ));
        return;
    }
    let pat = tv_get_string(&argvars[1]);

    let mut key: Option<String> = None;
    let mut matchseq = false;
    let mut max_matches: i64 = 0;
    if argvars.len() >= 3 && argvars[2].v_type != VAR_UNKNOWN {
        // c: tv_check_for_nonnull_dict_arg(argvars, 2).
        let d = match (argvars[2].v_type, &argvars[2].vval) {
            (VAR_DICT, v_dict(Some(d))) => d.clone(),
            _ => {
                crate::ported::message::semsg("E1206: Dictionary required for argument 3");
                return;
            }
        };
        let d = d.borrow();
        if let Some(di) = tv_dict_find(&d, "key") {
            // c: a `key` must be a non-empty String.
            let bad = di.v_type != VAR_STRING || tv_get_string(di).is_empty();
            if bad {
                crate::ported::message::semsg(&format!(
                    "E475: Invalid value for argument key: {}",
                    tv_get_string(di)
                ));
                return;
            }
            key = Some(tv_get_string(di));
        }
        if let Some(di) = tv_dict_find(&d, "limit") {
            // c: `limit` must be a Number.
            if di.v_type != VAR_NUMBER {
                crate::ported::message::semsg("E475: Invalid value for argument limit");
                return;
            }
            max_matches = tv_get_number(di);
        }
        // c: `if (tv_dict_has_key(d, "matchseq")) matchseq = true;` — presence,
        // not value.
        matchseq = tv_dict_find(&d, "matchseq").is_some();
    }

    // c: tv_list_alloc_ret + (for matchfuzzypos) the three pre-created lists.
    let out = tv_list_alloc_ret(rettv, if retmatchpos { 3 } else { 0 });
    if retmatchpos {
        let mut ob = out.borrow_mut();
        for _ in 0..3 {
            tv_list_append_tv(
                &mut ob,
                typval_T {
                    v_type: VAR_LIST,
                    v_lock: VarLockStatus::VAR_UNLOCKED,
                    vval: v_list(Some(tv_list_alloc(0))),
                },
            );
        }
    }

    fuzzy_match_in_list(
        &items,
        &pat,
        matchseq,
        key.as_deref(),
        retmatchpos,
        rettv,
        max_matches,
    );
}

/// Port of `f_matchfuzzy()` (fuzzy.c) — fuzzy-filter a List by a pattern, best
/// matches first.
pub fn f_matchfuzzy(argvars: &[typval_T], rettv: &mut typval_T) {
    do_fuzzymatch(argvars, rettv, false);
}

/// Port of `f_matchfuzzypos()` (fuzzy.c) — like `matchfuzzy()` but returns
/// `[items, match-positions, scores]`.
pub fn f_matchfuzzypos(argvars: &[typval_T], rettv: &mut typval_T) {
    do_fuzzymatch(argvars, rettv, true);
}

// ── histadd()/histget()/histnr()/histdel() — Neovim cmdhist.c. ──

thread_local! {
    /// The five command-line history rings (`history[HIST_COUNT][]` in
    /// cmdhist.c): cmd, search, expr, input, debug. Index 0 is the oldest.
    static HISTORY: std::cell::RefCell<[Vec<String>; 5]> =
        const { std::cell::RefCell::new([Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()]) };
}

/// Port of `get_histtype()` (Neovim cmdhist.c) — map a history name (`":"`/
/// `"cmd"`, `"/"`/`"search"`, `"="`/`"expr"`, `"@"`/`"input"`, `">"`/`"debug"`)
/// to its ring index, or `None` for an invalid name. An empty name is the
/// command history.
fn get_histtype(name: &str) -> Option<usize> {
    match name {
        "" | ":" | "cmd" => Some(0),
        "/" | "?" | "search" => Some(1),
        "=" | "expr" => Some(2),
        "@" | "input" => Some(3),
        ">" | "debug" => Some(4),
        _ => None,
    }
}

/// Port of `f_histadd()` (Neovim cmdhist.c) — add `{item}` to history
/// `{history}` (de-duplicating). Returns 1 on success, 0 on failure.
pub fn f_histadd(argvars: &[typval_T], rettv: &mut typval_T) {
    let name = tv_get_string(&argvars[0]);
    let item = tv_get_string(&argvars[1]);
    let t = match get_histtype(&name) {
        Some(t) => t,
        None => return,
    };
    if item.is_empty() {
        return;
    }
    HISTORY.with(|h| {
        let mut h = h.borrow_mut();
        h[t].retain(|e| e != &item);
        h[t].push(item);
    });
    rettv.vval = v_number(1);
}

/// Port of `f_histget()` (Neovim cmdhist.c) — return the `{index}`-th entry of
/// history `{history}`. A positive index is the absolute 1-based entry number,
/// a negative index counts back from the newest (-1 = newest); omitted/0 means
/// the newest. Out of range yields "".
pub fn f_histget(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    let name = tv_get_string(&argvars[0]);
    let t = match get_histtype(&name) {
        Some(t) => t,
        None => {
            rettv.vval = v_string(String::new());
            return;
        }
    };
    let idx = if argvars.len() >= 2 {
        tv_get_number(&argvars[1])
    } else {
        0
    };
    let s = HISTORY.with(|h| {
        let h = h.borrow();
        let v = &h[t];
        let n = v.len() as varnumber_T;
        let pos: Option<usize> = if idx == 0 {
            v.len().checked_sub(1)
        } else if idx > 0 {
            if idx <= n {
                Some((idx - 1) as usize)
            } else {
                None
            }
        } else {
            let p = n + idx;
            if p >= 0 {
                Some(p as usize)
            } else {
                None
            }
        };
        pos.map(|i| v[i].clone()).unwrap_or_default()
    });
    rettv.vval = v_string(s);
}

/// Port of `f_histnr()` (Neovim cmdhist.c) — the number of the newest entry in
/// history `{history}` (here the entry count), or -1 for an invalid name.
pub fn f_histnr(argvars: &[typval_T], rettv: &mut typval_T) {
    let name = tv_get_string(&argvars[0]);
    match get_histtype(&name) {
        Some(t) => {
            let n = HISTORY.with(|h| h.borrow()[t].len());
            rettv.vval = v_number(n as varnumber_T);
        }
        None => rettv.vval = v_number(-1),
    }
}

/// Port of `f_histdel()` (Neovim cmdhist.c) — delete from history `{history}`:
/// with no `{item}` clear the whole ring; a Number deletes that indexed entry;
/// a String deletes every entry matching it as a pattern. Returns 1.
pub fn f_histdel(argvars: &[typval_T], rettv: &mut typval_T) {
    let name = tv_get_string(&argvars[0]);
    let t = match get_histtype(&name) {
        Some(t) => t,
        None => return,
    };
    HISTORY.with(|h| {
        let mut h = h.borrow_mut();
        if argvars.len() < 2 {
            h[t].clear();
        } else if argvars[1].v_type == VAR_NUMBER {
            let idx = tv_get_number(&argvars[1]);
            let n = h[t].len() as varnumber_T;
            let pos = if idx > 0 && idx <= n {
                Some((idx - 1) as usize)
            } else if idx < 0 && n + idx >= 0 {
                Some((n + idx) as usize)
            } else {
                None
            };
            if let Some(p) = pos {
                h[t].remove(p);
            }
        } else {
            let pat = tv_get_string(&argvars[1]);
            h[t].retain(|e| !regex_match(&pat, e, false));
        }
    });
    rettv.vval = v_number(1);
}

// ── digraph_get()/digraph_set()/digraph_getlist()/digraph_setlist() —
//    Neovim digraph.c. ──

thread_local! {
    /// User digraphs set via `digraph_set()` (keyed by the two-char trigger),
    /// layered over the small built-in table in `getexactdigraph()`.
    static USER_DIGRAPHS: std::cell::RefCell<std::collections::HashMap<String, String>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Port of `getexactdigraph()` (Neovim digraph.c) — resolve the two-character
/// trigger `c1c2` to its digraph string: a user digraph (from `digraph_set()`)
/// takes precedence over the built-in RFC-1345 subset; `None` if unknown.
fn getexactdigraph(c1: char, c2: char) -> Option<String> {
    let key: String = [c1, c2].iter().collect();
    if let Some(v) = USER_DIGRAPHS.with(|d| d.borrow().get(&key).cloned()) {
        return Some(v);
    }
    // c: a small slice of Neovim's built-in digraph table (digraphdefault[]).
    let builtin = match (c1, c2) {
        ('a', ':') => 'ä',
        ('o', ':') => 'ö',
        ('u', ':') => 'ü',
        ('e', '\'') => 'é',
        ('C', 'o') => '©',
        ('R', 'O') => '®',
        ('+', '-') => '±',
        ('-', '>') => '→',
        ('O', 'K') => '✓',
        _ => return None,
    };
    Some(builtin.to_string())
}

/// Port of `f_digraph_get()` (Neovim digraph.c) — the digraph string for the
/// two-character trigger `{chars}`, or "" if none is defined.
pub fn f_digraph_get(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    let chars = tv_get_string(&argvars[0]);
    let cc: Vec<char> = chars.chars().collect();
    if cc.len() != 2 {
        crate::ported::message::semsg(&format!(
            "E1214: Digraph must be just two characters: {chars}"
        ));
        rettv.vval = v_string(String::new());
        return;
    }
    rettv.vval = v_string(getexactdigraph(cc[0], cc[1]).unwrap_or_default());
}

/// Port of `f_digraph_set()` (Neovim digraph.c) — register the digraph
/// `{digraph}` for the two-character trigger `{chars}`. Returns v:true/v:false.
pub fn f_digraph_set(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_BOOL;
    let chars = tv_get_string(&argvars[0]);
    let digr = tv_get_string(&argvars[1]);
    if chars.chars().count() != 2 {
        crate::ported::message::semsg(&format!(
            "E1214: Digraph must be just two characters: {chars}"
        ));
        rettv.vval = v_bool(kBoolVarFalse);
        return;
    }
    USER_DIGRAPHS.with(|d| d.borrow_mut().insert(chars, digr));
    rettv.vval = v_bool(kBoolVarTrue);
}

/// Port of `f_digraph_getlist()` (Neovim digraph.c) — return the user digraphs
/// as a List of `[chars, digraph]` pairs (sorted by trigger for stability).
pub fn f_digraph_getlist(_argvars: &[typval_T], rettv: &mut typval_T) {
    let mut entries: Vec<(String, String)> = USER_DIGRAPHS.with(|d| {
        d.borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    });
    entries.sort();
    let out = tv_list_alloc_ret(rettv, entries.len() as isize);
    let mut ob = out.borrow_mut();
    for (chars, digr) in entries {
        let pair = tv_list_alloc(2);
        {
            let mut pb = pair.borrow_mut();
            tv_list_append_string(&mut pb, &chars);
            tv_list_append_string(&mut pb, &digr);
        }
        tv_list_append_tv(
            &mut ob,
            typval_T {
                v_type: VAR_LIST,
                v_lock: VarLockStatus::VAR_UNLOCKED,
                vval: v_list(Some(pair)),
            },
        );
    }
}

/// Port of `f_digraph_setlist()` (Neovim digraph.c) — register a List of
/// `[chars, digraph]` pairs. Returns v:true on success, v:false on a bad entry.
pub fn f_digraph_setlist(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_BOOL;
    rettv.vval = v_bool(kBoolVarFalse);
    let l = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => l.clone(),
        _ => {
            emsg("E714: List required");
            return;
        }
    };
    let mut pending: Vec<(String, String)> = Vec::new();
    for item in l.borrow().lv_items.iter() {
        let pair: Vec<String> = match (item.li_tv.v_type, &item.li_tv.vval) {
            (VAR_LIST, v_list(Some(inner))) => inner
                .borrow()
                .lv_items
                .iter()
                .map(|e| tv_get_string(&e.li_tv))
                .collect(),
            _ => {
                emsg("E1216: digraph_setlist() argument must be a list of lists with two items");
                return;
            }
        };
        if pair.len() != 2 || pair[0].chars().count() != 2 {
            emsg("E1216: digraph_setlist() argument must be a list of lists with two items");
            return;
        }
        pending.push((pair[0].clone(), pair[1].clone()));
    }
    USER_DIGRAPHS.with(|d| {
        let mut d = d.borrow_mut();
        for (k, v) in pending {
            d.insert(k, v);
        }
    });
    rettv.vval = v_bool(kBoolVarTrue);
}

// ── hostname()/iconv() — eval/funcs.c (full table) + Neovim mbyte.c. ──

/// Port of `f_hostname()` (Neovim eval/funcs.c, full table) — the system host
/// name (`os_get_hostname`).
pub fn f_hostname(_argvars: &[typval_T], rettv: &mut typval_T) {
    use nix::libc;
    rettv.v_type = VAR_STRING;
    // c: os_get_hostname() → gethostname(3) into a fixed buffer.
    let mut buf = [0u8; 256];
    let name = unsafe {
        if libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) == 0 {
            std::ffi::CStr::from_ptr(buf.as_ptr() as *const libc::c_char)
                .to_string_lossy()
                .into_owned()
        } else {
            String::new()
        }
    };
    rettv.vval = v_string(name);
}

/// Port of `f_iconv()` (Neovim eval/funcs.c → mbyte.c `string_convert`) —
/// convert `{expr}` from `{from}` to `{to}`. vimlrs holds strings as UTF-8
/// internally, so identity and UTF-8↔UTF-8 conversions return the input; an
/// unsupported pairing also returns the input unchanged (Vim's "no conversion"
/// fallback), with characters left as-is.
pub fn f_iconv(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    let s = tv_get_string(&argvars[0]);
    let canon = |e: &str| -> &'static str {
        match e.to_ascii_lowercase().as_str() {
            "utf-8" | "utf8" | "unicode" => "utf-8",
            "latin1" | "iso-8859-1" | "8bit-iso-8859-1" => "latin1",
            _ => "other",
        }
    };
    let from = canon(&tv_get_string(&argvars[1]));
    let to = canon(&tv_get_string(&argvars[2]));
    // c: same encoding (or both UTF-8 aliases) → no conversion needed.
    let out = if from == to {
        s
    } else if from == "latin1" && to == "utf-8" {
        // Latin-1 byte values map 1:1 onto the first 256 codepoints; our chars
        // already are those codepoints, so the text passes through.
        s
    } else if from == "utf-8" && to == "latin1" {
        // Representable codepoints (<= 0xFF) pass through; others become '?'.
        s.chars()
            .map(|c| if (c as u32) <= 0xFF { c } else { '?' })
            .collect()
    } else {
        s
    };
    rettv.vval = v_string(out);
}

// ── argc()/argv()/argidx() — eval/funcs.c (full table). Standalone, vimlrs has
//    no editor arglist, so the global argument list is the script file(s) named
//    on the command line — the natural counterpart of Vim's file arglist. The
//    CLI seeds it via `set_arglist()` before running. ──

thread_local! {
    /// The global argument list: the file argument(s) vimlrs was invoked with
    /// (`vimlrs a.vim b.vim` → `["a.vim", "b.vim"]`). Empty for REPL / `-e` /
    /// `-c`. This is Vim's unnamed, global arglist (`arglistid()` == 0).
    static ARGLIST: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Seed the global argument list from the command line (called by the CLI before
/// sourcing). `argv()`/`argc()` read it back from Vimscript.
pub fn set_arglist(args: &[String]) {
    ARGLIST.with(|a| *a.borrow_mut() = args.to_vec());
}

/// Port of `f_argc()` — Neovim `src/nvim/arglist.c` (home file not under the
/// vendored `vendor/eval/` tree, so allowlisted category-A). The size of the
/// global argument list (here the script files from the command line).
pub fn f_argc(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(ARGLIST.with(|a| a.borrow().len()) as varnumber_T);
}

/// Port of `f_argidx()` — Neovim `src/nvim/arglist.c` (not vendored). The current
/// index in the argument list. vimlrs sources every file in order rather than
/// tracking a "current" buffer, so the index rests at 0, as in a fresh Vim.
pub fn f_argidx(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(0);
}

/// Port of `f_argv()` — Neovim `src/nvim/arglist.c` (not vendored). With no
/// argument or `-1`, the whole
/// argument list as a List; with an index `N`, that entry as a String ("" when
/// out of range). The optional trailing window-id argument is accepted and
/// ignored (there is one global arglist).
pub fn f_argv(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: if (argvars[0].v_type != VAR_UNKNOWN) { idx = tv_get_number(...); ... }
    if !argvars.is_empty() && tv_get_number(&argvars[0]) != -1 {
        let idx = tv_get_number(&argvars[0]);
        rettv.v_type = VAR_STRING;
        let s = ARGLIST.with(|a| {
            let a = a.borrow();
            if idx >= 0 && (idx as usize) < a.len() {
                a[idx as usize].clone()
            } else {
                String::new()
            }
        });
        rettv.vval = v_string(s);
        return;
    }
    // No index (or -1): the whole list.
    let l = tv_list_alloc_ret(rettv, ARGLIST.with(|a| a.borrow().len()) as isize);
    let mut lb = l.borrow_mut();
    ARGLIST.with(|a| {
        for s in a.borrow().iter() {
            tv_list_append_string(&mut lb, s);
        }
    });
}

// ── assert_equalfile() — Neovim testing.c. ──

/// Port of `f_assert_equalfile()` (Neovim testing.c) — assert that two files
/// have identical contents. On mismatch (or a missing file) append an error to
/// `v:errors` and return 1; otherwise return 0.
pub fn f_assert_equalfile(argvars: &[typval_T], rettv: &mut typval_T) {
    let fname1 = tv_get_string(&argvars[0]);
    let fname2 = tv_get_string(&argvars[1]);
    let c1 = std::fs::read(&fname1);
    let c2 = std::fs::read(&fname2);
    let equal = matches!((&c1, &c2), (Ok(a), Ok(b)) if a == b);
    if equal {
        return;
    }
    // c: fill_assert_error builds "expected file ... to be equal to ...".
    let detail = match (c1.is_ok(), c2.is_ok()) {
        (false, _) => format!("E485: Can't read file {fname1}"),
        (_, false) => format!("E485: Can't read file {fname2}"),
        _ => format!("first file \"{fname1}\" differs from second file \"{fname2}\""),
    };
    let extra = if argvars.len() >= 3 {
        format!("{}: ", tv_get_string(&argvars[2]))
    } else {
        String::new()
    };
    assert_error(&format!("{extra}{detail}"));
    rettv.vval = v_number(1);
}

// ── arglistid() — arglist.c (not vendored); foldlevel() — fold.c. ──

/// Port of `f_arglistid()` — Neovim `src/nvim/arglist.c` (not vendored). The id
/// of the argument list of the (optionally specified) window. vimlrs runs with
/// one global, unnamed argument list, so the id is always 0.
pub fn f_arglistid(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(0);
}

/// Port of `f_foldlevel()` (Neovim fold.c) — the fold level at line `{lnum}`.
/// No editor windows/folds exist standalone, so every line is at level 0.
pub fn f_foldlevel(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(0);
}

// ════════════════════════════════════════════════════════════════════════════
// Round-2 builtin expansion. Match highlighting (window.c), sign definitions
// (sign.c), fold-close queries (fold.c) and mapping queries (mapping.c) — all
// outside the vendored vendor/eval/ tree, so recorded in the drift-gate
// allowlist. Standalone there are no windows/buffers, so the match and sign
// tables are pure in-memory bookkeeping and the editor-only queries return
// their documented "nothing here" values.
// ════════════════════════════════════════════════════════════════════════════

// ── matchadd()/matchaddpos()/matchdelete()/getmatches()/setmatches()/
//    clearmatches()/matcharg() — Neovim window.c (the w_match_head list). ──

thread_local! {
    /// The window match list (`w_match_head` in window.c), each entry a Dict
    /// `{group, pattern|pos…, priority, id}`. One global list standalone.
    static MATCHES: std::cell::RefCell<Vec<typval_T>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Counter for auto-assigned match ids (`-1` argument), like Vim's
    /// increasing default ids.
    static MATCH_LAST_ID: std::cell::Cell<varnumber_T> = const { std::cell::Cell::new(1000) };
}

/// Make a `VAR_DICT` typval owning `d`.
fn match_dict_val(
    d: std::rc::Rc<std::cell::RefCell<crate::ported::eval::typval_defs_h::dict_T>>,
) -> typval_T {
    typval_T {
        v_type: VAR_DICT,
        v_lock: VarLockStatus::VAR_UNLOCKED,
        vval: v_dict(Some(d)),
    }
}

/// Port of `f_matchadd()` (Neovim window.c `match_add`) — add a pattern match in
/// highlight `{group}`, returning its id (an explicit `{id}`, else an
/// auto-assigned one). Returns -1 on a bad id.
pub fn f_matchadd(argvars: &[typval_T], rettv: &mut typval_T) {
    let group = tv_get_string(&argvars[0]);
    let pattern = tv_get_string(&argvars[1]);
    let priority = if argvars.len() >= 3 {
        tv_get_number(&argvars[2])
    } else {
        10
    };
    let mut id = if argvars.len() >= 4 {
        tv_get_number(&argvars[3])
    } else {
        -1
    };
    if id == 0 || id < -1 {
        // c: emsg(_("E799: Invalid ID: %" PRId64)) — ids are >= 1, or -1 (auto).
        crate::ported::message::semsg(&format!("E799: Invalid ID: {id}"));
        rettv.vval = v_number(-1);
        return;
    }
    if id == -1 {
        id = MATCH_LAST_ID.with(|c| {
            let v = c.get() + 1;
            c.set(v);
            v
        });
    }
    let d = tv_dict_alloc();
    {
        let mut db = d.borrow_mut();
        tv_dict_add_str(&mut db, "group", &group);
        tv_dict_add_str(&mut db, "pattern", &pattern);
        tv_dict_add_nr(&mut db, "priority", priority);
        tv_dict_add_nr(&mut db, "id", id);
    }
    MATCHES.with(|m| m.borrow_mut().push(match_dict_val(d)));
    rettv.vval = v_number(id);
}

/// Port of `f_matchaddpos()` (Neovim window.c `matchaddpos`) — like
/// `matchadd()` but matches the line/column positions in `{pos}` instead of a
/// pattern. Returns the match id.
pub fn f_matchaddpos(argvars: &[typval_T], rettv: &mut typval_T) {
    let group = tv_get_string(&argvars[0]);
    let priority = if argvars.len() >= 3 {
        tv_get_number(&argvars[2])
    } else {
        10
    };
    let mut id = if argvars.len() >= 4 {
        tv_get_number(&argvars[3])
    } else {
        -1
    };
    if id == 0 || id < -1 {
        crate::ported::message::semsg(&format!("E799: Invalid ID: {id}"));
        rettv.vval = v_number(-1);
        return;
    }
    if id == -1 {
        id = MATCH_LAST_ID.with(|c| {
            let v = c.get() + 1;
            c.set(v);
            v
        });
    }
    let d = tv_dict_alloc();
    {
        let mut db = d.borrow_mut();
        tv_dict_add_str(&mut db, "group", &group);
        tv_dict_add_nr(&mut db, "priority", priority);
        tv_dict_add_nr(&mut db, "id", id);
        // c: store each position as pos1, pos2, … (verbatim list items).
        if let (VAR_LIST, v_list(Some(l))) = (argvars[1].v_type, &argvars[1].vval) {
            for (i, it) in l.borrow().lv_items.iter().enumerate() {
                tv_dict_add_tv(&mut db, &format!("pos{}", i + 1), it.li_tv.clone());
            }
        }
    }
    MATCHES.with(|m| m.borrow_mut().push(match_dict_val(d)));
    rettv.vval = v_number(id);
}

/// Port of `f_matchdelete()` (Neovim window.c `match_delete`) — delete the match
/// with id `{id}`. Returns 0 on success, -1 if there is no such match.
pub fn f_matchdelete(argvars: &[typval_T], rettv: &mut typval_T) {
    let id = tv_get_number(&argvars[0]);
    let removed = MATCHES.with(|m| {
        let mut m = m.borrow_mut();
        let before = m.len();
        m.retain(|tv| match (tv.v_type, &tv.vval) {
            (VAR_DICT, v_dict(Some(d))) => {
                tv_dict_find(&d.borrow(), "id").map(tv_get_number) != Some(id)
            }
            _ => true,
        });
        before != m.len()
    });
    if !removed {
        // c: emsg(_(e_no_match_id)) — E803.
        crate::ported::message::semsg(&format!("E803: ID not found: {id}"));
        rettv.vval = v_number(-1);
    }
}

/// Port of `f_getmatches()` (Neovim window.c) — the current match list as a List
/// of Dicts (a copy).
pub fn f_getmatches(_argvars: &[typval_T], rettv: &mut typval_T) {
    let snapshot = MATCHES.with(|m| m.borrow().clone());
    let out = tv_list_alloc_ret(rettv, snapshot.len() as isize);
    let mut ob = out.borrow_mut();
    for tv in snapshot {
        tv_list_append_tv(&mut ob, tv);
    }
}

/// Port of `f_setmatches()` (Neovim window.c) — replace the whole match list
/// with the List of Dicts `{list}`. Returns 0 on success, -1 on a type error.
pub fn f_setmatches(argvars: &[typval_T], rettv: &mut typval_T) {
    let l = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => l.clone(),
        _ => {
            emsg("E714: List required");
            rettv.vval = v_number(-1);
            return;
        }
    };
    let mut next: Vec<typval_T> = Vec::new();
    for it in l.borrow().lv_items.iter() {
        if it.li_tv.v_type != VAR_DICT {
            // c: emsg(_(e_dictreq)) — every list item must be a Dict.
            emsg("E715: Dictionary required");
            rettv.vval = v_number(-1);
            return;
        }
        next.push(it.li_tv.clone());
    }
    MATCHES.with(|m| *m.borrow_mut() = next);
}

/// Port of `f_clearmatches()` (Neovim window.c `clear_matches`) — remove all
/// matches.
pub fn f_clearmatches(_argvars: &[typval_T], _rettv: &mut typval_T) {
    MATCHES.with(|m| m.borrow_mut().clear());
}

/// Port of `f_matcharg()` (Neovim window.c) — the `{group, pattern}` of the
/// `:match`/`:2match`/`:3match` command number `{nr}`. None are set standalone,
/// so 1/2/3 yield `['', '']` and any other number an empty List.
pub fn f_matcharg(argvars: &[typval_T], rettv: &mut typval_T) {
    let nr = tv_get_number(&argvars[0]);
    let l = tv_list_alloc_ret(rettv, 2);
    if (1..=3).contains(&nr) {
        let mut lb = l.borrow_mut();
        tv_list_append_string(&mut lb, "");
        tv_list_append_string(&mut lb, "");
    }
}

// ── sign_define()/sign_getdefined()/sign_undefine() — Neovim sign.c. ──

thread_local! {
    /// Defined signs (`sign_define` table in sign.c): name → option Dict.
    static SIGNS: std::cell::RefCell<std::collections::BTreeMap<String, typval_T>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
}

/// Port of `f_sign_define()` (Neovim sign.c) — define sign `{name}` with the
/// optional attribute Dict. Returns 0 on success, -1 on error.
pub fn f_sign_define(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: the list form (sign_define([{dict}, …])) returns a List; here we port
    // the scalar form sign_define({name} [, {dict}]).
    if argvars[0].v_type == VAR_LIST {
        emsg("E1206: Dictionary required");
        rettv.vval = v_number(-1);
        return;
    }
    let name = tv_get_string(&argvars[0]);
    if name.is_empty() {
        rettv.vval = v_number(-1);
        return;
    }
    let opts = if argvars.len() >= 2 && argvars[1].v_type == VAR_DICT {
        argvars[1].clone()
    } else {
        let d = tv_dict_alloc();
        match_dict_val(d)
    };
    SIGNS.with(|s| s.borrow_mut().insert(name, opts));
}

/// Port of `f_sign_getdefined()` (Neovim sign.c) — the list of defined signs as
/// Dicts (each `{name}` merged with its attributes); with `{name}` just that
/// sign (or an empty List).
pub fn f_sign_getdefined(argvars: &[typval_T], rettv: &mut typval_T) {
    let want = if argvars.is_empty() {
        None
    } else {
        Some(tv_get_string(&argvars[0]))
    };
    let entries: Vec<(String, typval_T)> = SIGNS.with(|s| {
        s.borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    });
    let out = tv_list_alloc_ret(rettv, 0);
    let mut ob = out.borrow_mut();
    for (name, opts) in entries {
        if let Some(w) = &want {
            if w != &name {
                continue;
            }
        }
        let d = tv_dict_alloc();
        {
            let mut db = d.borrow_mut();
            tv_dict_add_str(&mut db, "name", &name);
            // c: copy the stored attributes alongside the name.
            if let (VAR_DICT, v_dict(Some(src))) = (opts.v_type, &opts.vval) {
                for (k, v) in src.borrow().dv_hashtab.iter() {
                    tv_dict_add_tv(&mut db, k, v.clone());
                }
            }
        }
        tv_list_append_tv(&mut ob, match_dict_val(d));
    }
}

/// Port of `f_sign_undefine()` (Neovim sign.c) — undefine sign `{name}`, or all
/// signs when called with no argument. Returns 0 on success.
pub fn f_sign_undefine(argvars: &[typval_T], rettv: &mut typval_T) {
    if argvars.is_empty() {
        SIGNS.with(|s| s.borrow_mut().clear());
        return;
    }
    let name = tv_get_string(&argvars[0]);
    let existed = SIGNS.with(|s| s.borrow_mut().remove(&name).is_some());
    if !existed {
        crate::ported::message::semsg(&format!("E155: Unknown sign: {name}"));
        rettv.vval = v_number(-1);
    }
}

// ── foldclosed()/foldclosedend() — Neovim fold.c. No folds standalone. ──

/// Port of `f_foldclosed()` (Neovim fold.c) — the first line of the closed fold
/// at `{lnum}`, or -1 when the line is not in a closed fold (always, standalone).
pub fn f_foldclosed(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(-1);
}

/// Port of `f_foldclosedend()` (Neovim fold.c) — the last line of the closed
/// fold at `{lnum}`, or -1 (always, standalone).
pub fn f_foldclosedend(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(-1);
}

// ── Real mapping subsystem (Neovim mapping.c). An in-memory mapping table
//    (`maphash`) populated programmatically through mapset() and queried by
//    maparg()/mapcheck()/maplist()/hasmapto(). No editor is needed — a mapping
//    is just data. ──

// Mode flag bits (internal; the dict `mode` key is the mode-char string). The
// exact values are not observable, only how modes intersect.
const MAP_NORMAL: i32 = 0x01;
const MAP_VISUAL: i32 = 0x02;
const MAP_SELECT: i32 = 0x04;
const MAP_OP_PENDING: i32 = 0x08;
const MAP_INSERT: i32 = 0x10;
const MAP_CMDLINE: i32 = 0x20;
const MAP_LANG: i32 = 0x40;
const MAP_TERMINAL: i32 = 0x80;
/// The `:map` default — normal, visual, select and operator-pending.
const MAP_DEFAULT: i32 = MAP_NORMAL | MAP_VISUAL | MAP_SELECT | MAP_OP_PENDING;

/// A single mapping (`mapblock_T` in mapping.c), reduced to the fields the
/// builtins expose. Keeps the C struct name.
#[derive(Clone)]
#[allow(non_camel_case_types)]
pub struct mapblock_T {
    pub lhs: String,
    pub rhs: String,
    pub mode: i32,
    pub noremap: bool,
    pub expr: bool,
    pub silent: bool,
    pub nowait: bool,
    pub buffer: bool,
    pub sid: varnumber_T,
    pub lnum: varnumber_T,
    pub desc: String,
}

thread_local! {
    /// The mapping table (`maphash[]` in mapping.c), a flat list standalone.
    static MAPHASH: std::cell::RefCell<Vec<mapblock_T>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Port of `mode_str2flags()` (Neovim mapping.c) — a mode-char string (`"n"`,
/// `"v"`, `"!"`, …) to its mode-flag bits; ""/" " is the `:map` default.
fn mode_str2flags(modechars: &str) -> i32 {
    if modechars.is_empty() || modechars == " " {
        return MAP_DEFAULT;
    }
    let mut f = 0;
    for c in modechars.chars() {
        f |= match c {
            'n' => MAP_NORMAL,
            'v' => MAP_VISUAL | MAP_SELECT,
            'x' => MAP_VISUAL,
            's' => MAP_SELECT,
            'o' => MAP_OP_PENDING,
            'i' => MAP_INSERT,
            'c' => MAP_CMDLINE,
            'l' => MAP_LANG,
            't' => MAP_TERMINAL,
            '!' => MAP_INSERT | MAP_CMDLINE,
            _ => 0,
        };
    }
    f
}

/// Port of `map_mode_to_chars()` (Neovim mapping.c) — mode bits to the
/// canonical mode-char `maparg()` reports (" " for the normal/visual/op-pending
/// default, "!" for insert+cmdline, "v" for visual+select).
fn map_mode_to_chars(mode: i32) -> String {
    if mode == MAP_DEFAULT {
        return " ".to_string();
    }
    if mode == MAP_INSERT | MAP_CMDLINE {
        return "!".to_string();
    }
    if mode == MAP_VISUAL | MAP_SELECT {
        return "v".to_string();
    }
    let mut s = String::new();
    for (bit, c) in [
        (MAP_NORMAL, 'n'),
        (MAP_VISUAL, 'x'),
        (MAP_SELECT, 's'),
        (MAP_OP_PENDING, 'o'),
        (MAP_INSERT, 'i'),
        (MAP_CMDLINE, 'c'),
        (MAP_LANG, 'l'),
        (MAP_TERMINAL, 't'),
    ] {
        if mode & bit != 0 {
            s.push(c);
        }
    }
    s
}

/// Port of `mapblock_fill_dict()` (Neovim mapping.c) — build the `maparg()`/
/// `maplist()` Dict describing one mapping.
fn mapblock_fill_dict(mb: &mapblock_T) -> typval_T {
    let d = tv_dict_alloc();
    {
        let mut db = d.borrow_mut();
        tv_dict_add_str(&mut db, "lhs", &mb.lhs);
        tv_dict_add_str(&mut db, "lhsraw", &mb.lhs);
        tv_dict_add_str(&mut db, "rhs", &mb.rhs);
        tv_dict_add_nr(&mut db, "noremap", mb.noremap as varnumber_T);
        tv_dict_add_nr(&mut db, "expr", mb.expr as varnumber_T);
        tv_dict_add_nr(&mut db, "silent", mb.silent as varnumber_T);
        tv_dict_add_nr(&mut db, "nowait", mb.nowait as varnumber_T);
        tv_dict_add_nr(&mut db, "buffer", mb.buffer as varnumber_T);
        tv_dict_add_nr(&mut db, "script", 0);
        tv_dict_add_nr(&mut db, "sid", mb.sid);
        tv_dict_add_nr(&mut db, "scriptversion", 1);
        tv_dict_add_nr(&mut db, "lnum", mb.lnum);
        tv_dict_add_str(&mut db, "mode", &map_mode_to_chars(mb.mode));
        tv_dict_add_nr(&mut db, "mode_bits", mb.mode as varnumber_T);
        tv_dict_add_str(&mut db, "desc", &mb.desc);
        tv_dict_add_nr(&mut db, "abbr", 0);
    }
    match_dict_val(d)
}

/// Port of `map_add()` (Neovim mapping.c) — insert `mb`, replacing any existing
/// mapping with the same lhs in overlapping modes.
fn map_add(mb: mapblock_T) {
    MAPHASH.with(|h| {
        let mut h = h.borrow_mut();
        h.retain(|e| !(e.lhs == mb.lhs && e.mode & mb.mode != 0));
        h.push(mb);
    });
}

/// Port of `getmaparg()` (Neovim mapping.c) — the mapping for `name` in `mode`
/// returned as the rhs String or (when `want_dict`) a Dict; empty if none.
fn getmaparg(name: &str, mode: i32, want_dict: bool, rettv: &mut typval_T) {
    let found = MAPHASH.with(|h| {
        h.borrow()
            .iter()
            .find(|e| e.lhs == name && e.mode & mode != 0)
            .cloned()
    });
    if want_dict {
        match found {
            Some(mb) => *rettv = mapblock_fill_dict(&mb),
            None => {
                let _ = tv_dict_alloc_ret(rettv);
            }
        }
    } else {
        rettv.v_type = VAR_STRING;
        rettv.vval = v_string(found.map(|m| m.rhs).unwrap_or_default());
    }
}

/// Port of `f_hasmapto()` (Neovim mapping.c) — whether some mapping in the given
/// mode maps *to* `{what}`.
pub fn f_hasmapto(argvars: &[typval_T], rettv: &mut typval_T) {
    let what = tv_get_string(&argvars[0]);
    let mode = mode_str2flags(&argvars.get(1).map(tv_get_string).unwrap_or_default());
    let found = MAPHASH.with(|h| {
        h.borrow()
            .iter()
            .any(|e| e.mode & mode != 0 && e.rhs.contains(&what))
    });
    rettv.vval = v_number(found as varnumber_T);
}

/// Port of `f_maparg()` (Neovim mapping.c) — the rhs that `{name}` maps to in
/// `{mode}` (a String, or a full Dict when the `{dict}` argument is truthy).
pub fn f_maparg(argvars: &[typval_T], rettv: &mut typval_T) {
    let name = tv_get_string(&argvars[0]);
    let mode = mode_str2flags(&argvars.get(1).map(tv_get_string).unwrap_or_default());
    let want_dict = argvars.len() >= 4 && tv_get_bool(&argvars[3]) != 0;
    getmaparg(&name, mode, want_dict, rettv);
}

/// Port of `f_mapcheck()` (Neovim mapping.c) — the rhs of a mapping whose lhs
/// `{name}` could begin (or that begins with `{name}`); "" if none.
pub fn f_mapcheck(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    let name = tv_get_string(&argvars[0]);
    let mode = mode_str2flags(&argvars.get(1).map(tv_get_string).unwrap_or_default());
    let rhs = MAPHASH.with(|h| {
        h.borrow()
            .iter()
            .find(|e| e.mode & mode != 0 && (e.lhs.starts_with(&name) || name.starts_with(&e.lhs)))
            .map(|e| e.rhs.clone())
    });
    rettv.vval = v_string(rhs.unwrap_or_default());
}

/// Port of `f_maplist()` (Neovim mapping.c) — every mapping as a List of Dicts.
pub fn f_maplist(_argvars: &[typval_T], rettv: &mut typval_T) {
    let maps = MAPHASH.with(|h| h.borrow().clone());
    let l = tv_list_alloc_ret(rettv, maps.len() as isize);
    let mut lb = l.borrow_mut();
    for mb in &maps {
        tv_list_append_tv(&mut lb, mapblock_fill_dict(mb));
    }
}

/// Port of `get_map_mode()` (Neovim mapping.c) — parse a `:map`-family command
/// word (e.g. `nmap`, `inoremap`, `vunmap`, `cmapclear`, `map!`) into its mode
/// bits and the action it requests: `(mode, is_unmap, is_clear, is_noremap)`.
/// Returns `None` if `cmd` is not a map-family command.
pub fn get_map_mode(cmd: &str) -> Option<(i32, bool, bool, bool)> {
    let (base, bang) = match cmd.strip_suffix('!') {
        Some(b) => (b, true),
        None => (cmd, false),
    };
    let (prefix, unmap, clear, noremap) = if let Some(p) = base.strip_suffix("mapclear") {
        (p, false, true, false)
    } else if let Some(p) = base.strip_suffix("noremap") {
        (p, false, false, true)
    } else if let Some(p) = base.strip_suffix("unmap") {
        (p, true, false, false)
    } else if let Some(p) = base.strip_suffix("map") {
        (p, false, false, false)
    } else {
        return None;
    };
    let mode = match prefix {
        "" => {
            if bang {
                MAP_INSERT | MAP_CMDLINE
            } else {
                MAP_DEFAULT
            }
        }
        "n" => MAP_NORMAL,
        "i" => MAP_INSERT,
        "v" => MAP_VISUAL | MAP_SELECT,
        "x" => MAP_VISUAL,
        "s" => MAP_SELECT,
        "o" => MAP_OP_PENDING,
        "c" => MAP_CMDLINE,
        "t" => MAP_TERMINAL,
        "l" => MAP_LANG,
        _ => return None,
    };
    Some((mode, unmap, clear, noremap))
}

/// Port of `do_map()` (Neovim mapping.c) — execute a parsed `:map`-family
/// command: clear the mode's mappings, remove the `{lhs}` mapping, or add a
/// `{lhs}` → `{rhs}` mapping (honoring the `<silent>`/`<expr>`/`<nowait>`/
/// `<buffer>` argument prefixes). `arg` is the text after the command word.
pub fn do_map(arg: &str, mode: i32, unmap: bool, clear: bool, noremap: bool) {
    if clear {
        MAPHASH.with(|h| h.borrow_mut().retain(|e| e.mode & mode == 0));
        return;
    }
    // c: consume the leading <silent>/<expr>/<nowait>/<buffer>/… map arguments.
    let mut rest = arg.trim_start();
    let (mut silent, mut expr, mut nowait, mut buffer) = (false, false, false, false);
    loop {
        let lower = rest.to_ascii_lowercase();
        if let Some(r) = lower.strip_prefix("<silent>") {
            silent = true;
            rest = &rest[rest.len() - r.len()..];
        } else if let Some(r) = lower.strip_prefix("<expr>") {
            expr = true;
            rest = &rest[rest.len() - r.len()..];
        } else if let Some(r) = lower.strip_prefix("<nowait>") {
            nowait = true;
            rest = &rest[rest.len() - r.len()..];
        } else if let Some(r) = lower.strip_prefix("<buffer>") {
            buffer = true;
            rest = &rest[rest.len() - r.len()..];
        } else if let Some(r) = lower
            .strip_prefix("<unique>")
            .or(lower.strip_prefix("<script>"))
        {
            rest = &rest[rest.len() - r.len()..];
        } else {
            break;
        }
        rest = rest.trim_start();
    }
    let (lhs, rhs) = match rest.find(char::is_whitespace) {
        Some(i) => (&rest[..i], rest[i..].trim()),
        None => (rest, ""),
    };
    if lhs.is_empty() {
        return;
    }
    if unmap {
        MAPHASH.with(|h| {
            h.borrow_mut()
                .retain(|e| !(e.lhs == lhs && e.mode & mode != 0))
        });
        return;
    }
    // c: `:map {lhs}` with no rhs lists the mapping — a no-op standalone.
    if rhs.is_empty() {
        return;
    }
    map_add(mapblock_T {
        lhs: lhs.to_string(),
        rhs: rhs.to_string(),
        mode,
        noremap,
        expr,
        silent,
        nowait,
        buffer,
        sid: 0,
        lnum: 0,
        desc: String::new(),
    });
}

// ── User-defined commands (Neovim usercmd.c). `:command Name {repl}` stores a
//    command; invoking `:Name args` runs the replacement with `<args>`/
//    `<q-args>`/`<f-args>`/`<bang>`/`<lt>` substituted. Self-contained — no
//    editor needed. ──

/// A user command (`ucmd_T` in usercmd.c), reduced to the fields needed to
/// store and expand it. Keeps the C struct name.
#[allow(non_camel_case_types)]
#[derive(Clone)]
pub struct ucmd_T {
    pub rep: String,
    pub nargs: char,
    pub bang: bool,
}

thread_local! {
    /// The user-command table (`ucmds` in usercmd.c), keyed by command name.
    static USER_COMMANDS: std::cell::RefCell<std::collections::BTreeMap<String, ucmd_T>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
}

/// Port of `uc_add_command()` (Neovim usercmd.c) — register user command
/// `name` with replacement `rep`.
fn uc_add_command(name: &str, rep: &str, nargs: char, bang: bool) {
    USER_COMMANDS.with(|c| {
        c.borrow_mut().insert(
            name.to_string(),
            ucmd_T {
                rep: rep.to_string(),
                nargs,
                bang,
            },
        );
    });
}

/// Port of `find_ucmd()` (Neovim usercmd.c) — resolve `name` to a user command:
/// an exact match, else a unique prefix match.
fn find_ucmd(name: &str) -> Option<ucmd_T> {
    USER_COMMANDS.with(|c| {
        let c = c.borrow();
        if let Some(uc) = c.get(name) {
            return Some(uc.clone());
        }
        let mut hits = c.iter().filter(|(k, _)| k.starts_with(name));
        match (hits.next(), hits.next()) {
            (Some((_, uc)), None) => Some(uc.clone()),
            _ => None,
        }
    })
}

/// Port of `uc_check_code()` (Neovim usercmd.c) — substitute the `<...>` codes
/// in a user command's replacement: `<args>` (verbatim), `<q-args>` (one quoted
/// String), `<f-args>` (comma-separated quoted args), `<bang>` (`!`/nothing),
/// `<lt>` (`<`). Unknown codes are copied through.
fn uc_check_code(rep: &str, args: &str, bang: bool) -> String {
    let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let chars: Vec<char> = rep.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            if let Some(off) = chars[i..].iter().position(|&c| c == '>') {
                let code: String = chars[i + 1..i + off].iter().collect();
                let repl = match code.to_ascii_lowercase().as_str() {
                    "args" => Some(args.to_string()),
                    "q-args" => Some(quote(args)),
                    "f-args" => Some(
                        args.split_whitespace()
                            .map(quote)
                            .collect::<Vec<_>>()
                            .join(", "),
                    ),
                    "bang" => Some(if bang { "!".to_string() } else { String::new() }),
                    "lt" => Some("<".to_string()),
                    _ => None,
                };
                if let Some(r) = repl {
                    out.push_str(&r);
                    i += off + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Port of `do_ucmd()` (Neovim usercmd.c) — expand user command `name`'s
/// replacement with `args`/`bang`, ready to run. `None` if no such command.
pub fn do_ucmd(name: &str, args: &str, bang: bool) -> Option<String> {
    let uc = find_ucmd(name)?;
    Some(uc_check_code(&uc.rep, args, bang && uc.bang))
}

/// Port of `ex_command()` (Neovim usercmd.c) — define a user command from a
/// `:command` argument: `[!] [-attrs] Name {replacement}`.
pub fn ex_command(arg: &str) {
    let mut s = arg.trim();
    // A leading `!` means `:command!` (redefine); we always overwrite anyway.
    s = s.strip_prefix('!').unwrap_or(s).trim_start();
    let mut nargs = '0';
    let mut bang = false;
    // c: parse the leading `-attr[=val]` command attributes.
    while let Some(r) = s.strip_prefix('-') {
        let end = r.find(char::is_whitespace).unwrap_or(r.len());
        let attr = r[..end].to_ascii_lowercase();
        if let Some(v) = attr.strip_prefix("nargs=") {
            nargs = v.chars().next().unwrap_or('0');
        } else if attr == "bang" {
            bang = true;
        }
        // -range/-count/-complete=/-buffer/… are accepted and ignored.
        s = r[end..].trim_start();
    }
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    let name = &s[..end];
    let rep = s[end..].trim_start();
    if name.is_empty() {
        return;
    }
    uc_add_command(name, rep, nargs, bang);
}

/// Port of `ex_delcommand()` (Neovim usercmd.c) — delete user command `name`.
pub fn ex_delcommand(arg: &str) {
    let name = arg.trim();
    USER_COMMANDS.with(|c| {
        c.borrow_mut().remove(name);
    });
}

// ── Autocommands (Neovim autocmd.c). `:autocmd {event} {pat} {cmd}` registers
//    a command to run on an event; `:doautocmd {event} [{pat}]` fires matching
//    ones. `:augroup` sets the active group. Self-contained — events do not
//    auto-fire without an editor, but :doautocmd triggers them. ──

/// One registered autocommand (`AutoCmd`/`AutoPat` in autocmd.c).
#[derive(Clone)]
pub struct AutoCmd {
    pub group: String,
    pub event: String,
    pub pat: String,
    pub cmd: String,
}

thread_local! {
    /// The autocommand list (`first_autopat[]` in autocmd.c).
    static AUTOCMDS: std::cell::RefCell<Vec<AutoCmd>> = const { std::cell::RefCell::new(Vec::new()) };
    /// The `:augroup` in effect (`current_augroup`), "" for the default group.
    static CURRENT_AUGROUP: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// Port of `match_file_pat()` (Neovim autocmd.c/fileio.c) — whether the file
/// glob `pat` (with `*`/`?`) matches `name`. Iterative wildcard match.
fn match_file_pat(pat: &str, name: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let s: Vec<char> = name.chars().collect();
    let (mut pi, mut si) = (0usize, 0usize);
    let (mut star, mut mark): (Option<usize>, usize) = (None, 0);
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = si;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Port of `do_augroup()` (Neovim autocmd.c) — set the active autocommand group;
/// `END`/`end` (or empty) returns to the default group.
pub fn do_augroup(name: &str) {
    let name = name.trim();
    let g = if name.is_empty() || name.eq_ignore_ascii_case("end") {
        String::new()
    } else {
        name.to_string()
    };
    CURRENT_AUGROUP.with(|c| *c.borrow_mut() = g);
}

/// Port of `do_autocmd()` (Neovim autocmd.c) — register (or, with `!`, replace/
/// remove) an autocommand from a `:autocmd` argument: `[!] {event} {pat}
/// [{cmd}]`. Comma-separated events register one entry each.
pub fn do_autocmd(arg: &str) {
    let mut s = arg.trim();
    let force = s.starts_with('!');
    s = s.strip_prefix('!').unwrap_or(s).trim_start();
    let group = CURRENT_AUGROUP.with(|c| c.borrow().clone());
    if s.is_empty() {
        // `:autocmd!` — remove every autocommand in the current group.
        if force {
            AUTOCMDS.with(|a| a.borrow_mut().retain(|ac| ac.group != group));
        }
        return;
    }
    let (events_tok, rest) = match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim_start()),
        None => (s, ""),
    };
    let events: Vec<String> = events_tok
        .split(',')
        .filter(|e| !e.is_empty())
        .map(|e| e.to_ascii_lowercase())
        .collect();
    if rest.is_empty() {
        // `:autocmd! {event}` — remove the event's autocommands in this group.
        if force {
            AUTOCMDS.with(|a| {
                a.borrow_mut()
                    .retain(|ac| !(ac.group == group && events.contains(&ac.event)))
            });
        }
        return;
    }
    let (pat, cmd) = match rest.find(char::is_whitespace) {
        Some(i) => (&rest[..i], rest[i..].trim()),
        None => (rest, ""),
    };
    if cmd.is_empty() {
        // `:autocmd! {event} {pat}` — remove that event+pattern in this group.
        if force {
            AUTOCMDS.with(|a| {
                a.borrow_mut().retain(|ac| {
                    !(ac.group == group && events.contains(&ac.event) && ac.pat == pat)
                })
            });
        }
        return;
    }
    AUTOCMDS.with(|a| {
        let mut a = a.borrow_mut();
        for event in &events {
            // c: `:autocmd!` replaces any existing event+pattern first.
            if force {
                a.retain(|ac| !(ac.group == group && &ac.event == event && ac.pat == pat));
            }
            a.push(AutoCmd {
                group: group.clone(),
                event: event.clone(),
                pat: pat.to_string(),
                cmd: cmd.to_string(),
            });
        }
    });
}

/// Port of `apply_autocmds()` (Neovim autocmd.c) — the commands of every
/// autocommand for `event` whose pattern matches `target`, in registration
/// order. The caller runs them.
pub fn apply_autocmds(event: &str, target: &str) -> Vec<String> {
    let evt = event.to_ascii_lowercase();
    AUTOCMDS.with(|a| {
        a.borrow()
            .iter()
            .filter(|ac| ac.event == evt && match_file_pat(&ac.pat, target))
            .map(|ac| ac.cmd.clone())
            .collect()
    })
}

/// Port of `do_doautocmd()` (Neovim autocmd.c) — parse a `:doautocmd` argument
/// (`[group] {event} [{target}]`) and return the matching autocommands to run.
pub fn do_doautocmd(arg: &str) -> Vec<String> {
    let s = arg.trim();
    let (event, target) = match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim()),
        None => (s, "*"),
    };
    if event.is_empty() {
        return Vec::new();
    }
    apply_autocmds(event, target)
}

/// Port of `au_exists()` (Neovim autocmd.c) — whether autocommands exist for an
/// `exists()` query of the form `#{event}` or `#{event}#{pat}`.
pub fn au_exists(arg: &str) -> bool {
    let mut parts = arg.splitn(2, '#');
    let event = parts.next().unwrap_or("").to_ascii_lowercase();
    let pat = parts.next();
    AUTOCMDS.with(|a| {
        #[allow(clippy::unnecessary_map_or)]
        a.borrow()
            .iter()
            .any(|ac| ac.event == event && pat.map_or(true, |p| ac.pat == p))
    })
}

// ── Ex commands with a line range that operate on the current buffer (Neovim
//    ex_cmds.c / ex_docmd.c): `:[range]d/s/g/v/m/t/j/y/pu`. Reached from the
//    parser for `:`-prefixed and `%`-prefixed lines (neither starts a valid
//    expression). ──

/// The outcome of `do_excmd`: a direct command that mutated the buffer, a
/// command the Ex layer does not recognize (run it as an ordinary statement),
/// or a `:global` that must run `cmd` on each of the matched lines.
pub enum ExCmdResult {
    Handled,
    NotEx,
    Global(Vec<varnumber_T>, String),
}

/// Parse one address (`.`, `$`, a Number, or empty = the cursor) with optional
/// `+N`/`-N` offsets. Returns `(lnum, bytes_consumed, had_address)`.
fn parse_addr(s: &str) -> (varnumber_T, usize, bool) {
    let b = s.as_bytes();
    let mut i = 0;
    let last = curbuf_len();
    let cur = crate::fusevm_bridge::editor_curpos(CURPOS.with(|c| *c.borrow())).0;
    let (mut lnum, had) = if i < b.len() && b[i] == b'.' {
        i += 1;
        (cur, true)
    } else if i < b.len() && b[i] == b'$' {
        i += 1;
        (last, true)
    } else if i + 1 < b.len() && b[i] == b'\'' {
        // c: a `'m` mark address.
        let name = b[i + 1] as char;
        i += 2;
        (getmark(name).map_or(0, |(l, _)| l), true)
    } else if i < b.len() && b[i].is_ascii_digit() {
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        (s[start..i].parse().unwrap_or(0), true)
    } else {
        (cur, false)
    };
    // Offsets: +N / -N (a bare +/- is ±1), possibly several.
    let mut had_off = false;
    while i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        let sign = if b[i] == b'+' { 1 } else { -1 };
        i += 1;
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let n: varnumber_T = if i > start {
            s[start..i].parse().unwrap_or(0)
        } else {
            1
        };
        lnum += sign * n;
        had_off = true;
    }
    (lnum, i, had || had_off)
}

/// Parse a leading line range (`%`, `N`, `N,M`, `.`, `$`, `.+1,$`, …). Returns
/// `(line1, line2, had_range, rest)`; with no range both lines are the cursor
/// line and `had_range` is false.
fn parse_line_range(s: &str) -> (varnumber_T, varnumber_T, bool, &str) {
    let s = s.trim_start();
    if let Some(rest) = s.strip_prefix('%') {
        return (1, curbuf_len(), true, rest.trim_start());
    }
    let cur = crate::fusevm_bridge::editor_curpos(CURPOS.with(|c| *c.borrow())).0;
    let (a1, c1, had1) = parse_addr(s);
    let mut rest = &s[c1..];
    if let Some(after) = rest.strip_prefix([',', ';']) {
        let (a2, c2, _) = parse_addr(after);
        rest = &after[c2..];
        let l1 = if had1 { a1 } else { cur };
        return (l1, a2, true, rest.trim_start());
    }
    if had1 {
        (a1, a1, true, rest.trim_start())
    } else {
        (cur, cur, false, rest.trim_start())
    }
}

/// Split `s` on its first character (the delimiter) into up to three unescaped
/// fields — used to parse `:s/pat/sub/flags` and `:g/pat/cmd`.
fn split_delim(s: &str, max: usize) -> Vec<String> {
    let mut chars = s.chars();
    let delim = match chars.next() {
        Some(d) => d,
        None => return Vec::new(),
    };
    let mut fields = vec![String::new()];
    let mut escaped = false;
    for c in chars {
        if escaped {
            if c != delim {
                fields.last_mut().unwrap().push('\\');
            }
            fields.last_mut().unwrap().push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == delim && fields.len() < max {
            fields.push(String::new());
        } else {
            fields.last_mut().unwrap().push(c);
        }
    }
    fields
}

/// Port of `do_excmd()` (Neovim ex_docmd.c, this subset) — run a `:[range]cmd`
/// line against the current buffer.
pub fn do_excmd(line: &str) -> ExCmdResult {
    let line = line.trim().strip_prefix(':').unwrap_or(line.trim());
    let (l1, l2, had_range, rest) = parse_line_range(line);
    // Command word: leading letters, then an optional `!`.
    let cmd_end = rest
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(rest.len());
    let cmd = &rest[..cmd_end];
    let bang = rest[cmd_end..].starts_with('!');
    let args = rest[cmd_end + if bang { 1 } else { 0 }..].trim();
    let len = curbuf_len();
    let (lo, hi) = (l1.clamp(1, len), l2.clamp(1, len));
    match cmd {
        "d" | "de" | "del" | "delete" => {
            CURBUF.with(|b| {
                let mut b = b.borrow_mut();
                if b.is_empty() {
                    b.push(String::new());
                }
                let (a, z) = ((lo - 1) as usize, (hi as usize).min(b.len()));
                if a < z {
                    b.drain(a..z);
                }
                if b.is_empty() {
                    b.push(String::new());
                }
            });
            set_cursorpos(lo, 1);
            ExCmdResult::Handled
        }
        "s" | "su" | "sub" | "substitute" => {
            let f = split_delim(args, 3);
            let (pat, sub, flags) = (
                f.first().cloned().unwrap_or_default(),
                f.get(1).cloned().unwrap_or_default(),
                f.get(2).cloned().unwrap_or_default(),
            );
            for lnum in lo..=hi {
                let cur = get_buffer_lines(lnum, lnum)
                    .into_iter()
                    .next()
                    .unwrap_or_default();
                let new = crate::viml_regex::regex_substitute(&cur, &pat, &sub, &flags);
                set_buffer_lines(lnum, vec![new], false);
            }
            set_cursorpos(hi, 1);
            ExCmdResult::Handled
        }
        "g" | "gl" | "global" | "v" | "vglobal" => {
            let invert = cmd.starts_with('v') || bang;
            let f = split_delim(args, 2);
            let pat = f.first().cloned().unwrap_or_default();
            let sub_cmd = f.get(1).cloned().unwrap_or_default();
            let ic = tv_get_number_chk(&get_option_value("ignorecase"), None) != 0;
            // :global defaults to the whole file when no range is given.
            let (gl, gh) = if had_range { (lo, hi) } else { (1, len) };
            let mut hits = Vec::new();
            for lnum in gl..=gh {
                let l = get_buffer_lines(lnum, lnum)
                    .into_iter()
                    .next()
                    .unwrap_or_default();
                let m = crate::viml_regex::regex_match(&pat, &l, ic);
                if m != invert {
                    hits.push(lnum);
                }
            }
            ExCmdResult::Global(hits, sub_cmd)
        }
        "m" | "mo" | "move" => {
            let (dest, _, _) = parse_addr(args.trim());
            ex_move(lo, hi, dest);
            ExCmdResult::Handled
        }
        "t" | "co" | "cop" | "copy" => {
            let (dest, _, _) = parse_addr(args.trim());
            ex_copy(lo, hi, dest);
            ExCmdResult::Handled
        }
        "j" | "jo" | "join" => {
            ex_join(lo, hi, bang);
            ExCmdResult::Handled
        }
        "y" | "ya" | "yank" => {
            let reg = args.chars().next().unwrap_or('"');
            let lines = get_buffer_lines(lo, hi);
            write_reg_contents_lst(reg, lines, MotionType::LineWise, false);
            ExCmdResult::Handled
        }
        "pu" | "put" => {
            let reg = args.chars().next().unwrap_or('"');
            if let Some(lines) = get_reg_contents(reg) {
                set_buffer_lines(hi, lines, true);
            }
            ExCmdResult::Handled
        }
        "r" | "re" | "read" => {
            // `:[line]r {file}` inserts a file's lines after the range; `:r !cmd`
            // inserts a shell command's output. Default position: the last line.
            let at = if had_range { hi } else { curbuf_len() };
            let lines = if let Some(shell) = args.strip_prefix('!') {
                shell_capture(shell.trim())
            } else {
                std::fs::read_to_string(args)
                    .map(|s| s.lines().map(str::to_string).collect())
                    .unwrap_or_default()
            };
            set_buffer_lines(at, lines, true);
            ExCmdResult::Handled
        }
        "w" | "wr" | "write" => {
            // `:[range]w[!] {file}` writes the buffer (or range) to a file;
            // `:w >>file` appends; `:w !cmd` pipes to a shell command.
            let (lo2, hi2) = if had_range { (lo, hi) } else { (1, len) };
            let body = get_buffer_lines(lo2, hi2).join("\n") + "\n";
            if let Some(shell) = args.strip_prefix('!') {
                shell_feed(shell.trim(), &body);
            } else if let Some(file) = args.strip_prefix(">>") {
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(file.trim())
                {
                    use std::io::Write;
                    let _ = f.write_all(body.as_bytes());
                }
            } else if !args.is_empty() {
                let _ = std::fs::write(args, body);
            }
            ExCmdResult::Handled
        }
        "e" | "ed" | "edit" => {
            // `:e {file}` replaces the buffer with the file's contents.
            if !args.is_empty() {
                if let Ok(s) = std::fs::read_to_string(args) {
                    let lines: Vec<String> = s.lines().map(str::to_string).collect();
                    CURBUF.with(|b| {
                        *b.borrow_mut() = if lines.is_empty() {
                            vec![String::new()]
                        } else {
                            lines
                        }
                    });
                    set_cursorpos(1, 1);
                }
            }
            ExCmdResult::Handled
        }
        "sort" | "sor" => {
            let (lo2, hi2) = if had_range { (lo, hi) } else { (1, len) };
            ex_sort(lo2, hi2, bang, args);
            ExCmdResult::Handled
        }
        "mark" | "ma" | "mar" | "k" => {
            // `:[line]mark x` / `:[line]k x` sets mark x at the range's last line.
            if let Some(name) = args.chars().next() {
                setmark(name, hi, 1);
            }
            ExCmdResult::Handled
        }
        "normal" | "norm" => {
            // `:[range]normal {keys}` runs the keys on each line (cursor at col
            // 1); without a range it runs once at the cursor. A leading space
            // after the command separates the keys verbatim.
            let keys = rest[cmd_end + if bang { 1 } else { 0 }..]
                .strip_prefix(' ')
                .unwrap_or(args);
            if had_range {
                for lnum in lo..=hi.min(curbuf_len()) {
                    set_cursorpos(lnum, 1);
                    do_normal(keys);
                }
            } else {
                do_normal(keys);
            }
            ExCmdResult::Handled
        }
        "echohl" | "echoh" => {
            // `:echohl {group}` — set the highlight id for subsequent `:echo`.
            // `ex_echohl` (Src/eval.c:6207) resolves the group via `syn_name2id`,
            // which is 0 standalone (no highlight groups), so this clears the id.
            crate::ported::eval::ex_echohl(args);
            ExCmdResult::Handled
        }
        // `:runtime[!] {file}...` (`ex_docmd.c` → `ex_runtime`) sources matching
        // files from the runtime search path — how ftplugins pull in a sibling
        // (`runtime! ftplugin/c.vim` from `cpp.vim`, so `b:undo_ftplugin` exists
        // before the `..=` that appends to it). Delegated to the crate-root
        // sourcing machinery (`do_runtime`); the runtime path is rediscovered
        // editor-less (see [`crate::fusevm_bridge::runtime_dirs`]).
        "runtime" | "ru" | "run" | "runt" | "runti" | "runtim" => {
            crate::fusevm_bridge::do_runtime(bang, args);
            ExCmdResult::Handled
        }
        // Screen/session commands with no observable effect on an editor-less
        // config load: `:redraw[!]`/`:redrawstatus`/`:redrawtabline` (repaint)
        // and `:redir` (message redirection — no message UI here). Recognized as
        // no-ops so a function body using them still defines instead of the line
        // aborting the `:function`.
        "redraw" | "redr" | "redra" | "redraws" | "redrawstatus" | "redrawt" | "redrawtabline"
        | "redir" | "redi" => ExCmdResult::Handled,
        // `:noh[lsearch]` (`ex_docmd.c` → `ex_nohlsearch`) turns off the current
        // search-match highlighting until the next search. There is no highlight
        // state editor-less, so it is a no-op — recognized so a config/syntax
        // line `nohlsearch` is handled instead of declining to `parse_expr`.
        "noh" | "nohl" | "nohls" | "nohlse" | "nohlsea" | "nohlsear" | "nohlsearc"
        | "nohlsearch" => ExCmdResult::Handled,
        // Fold-view commands (`ex_docmd.c` → `ex_fold`/`ex_foldopen`): `:fo[ld]`
        // creates a fold, `:foldo[pen][!]` opens folds, `:foldc[lose][!]` closes
        // them (the `!` opens/closes recursively). Folds live in a window's view
        // state, which a standalone eval engine has none of, so every form is a
        // no-op — recognized so a runtime line like `syntax/cdl.vim`'s `%foldo!`
        // (which real Vim runs silently once `foldmethod=expr` has made folds)
        // is Handled instead of declining to E492.
        "fold" | "fo" | "fol" | "foldopen" | "foldo" | "foldop" | "foldope" | "foldclose"
        | "foldc" | "foldcl" | "foldclo" | "foldclos" => ExCmdResult::Handled,
        // Buffer-list navigation (`:bnext`/`:bprevious`/`:bfirst`/`:blast`/
        // `:buffer`/`:bmodified`/`:ball`) switches the displayed buffer — a no-op
        // editor-less. (`:edit {file}` above still loads a file when given one.)
        "bnext" | "bn" | "bne" | "bprevious" | "bp" | "bprev" | "bNext" | "bN" | "bfirst"
        | "bf" | "blast" | "bl" | "buffer" | "bu" | "buf" | "bmodified" | "bm" | "bmod"
        | "ball" | "ba" => ExCmdResult::Handled,
        "delmarks" | "delm" => {
            if bang {
                MARKS.with(|m| m.borrow_mut().clear());
            } else {
                for name in args.chars().filter(|c| !c.is_whitespace()) {
                    MARKS.with(|m| {
                        m.borrow_mut().remove(&name);
                    });
                }
            }
            ExCmdResult::Handled
        }
        "p" | "pr" | "print" | "nu" | "number" => {
            let numbered = cmd.starts_with("nu") || cmd == "number";
            for lnum in lo..=hi {
                let l = get_buffer_lines(lnum, lnum)
                    .into_iter()
                    .next()
                    .unwrap_or_default();
                if numbered {
                    println!("{lnum:>3} {l}");
                } else {
                    println!("{l}");
                }
            }
            ExCmdResult::Handled
        }
        // The command word started with a non-letter Ex command (`>`/`<`/`!`)?
        // parse_addr already consumed any range, so try those directly.
        "" if rest.starts_with('>') || rest.starts_with('<') || rest.starts_with('!') => {
            let c = rest.chars().next().unwrap();
            if c == '!' {
                if had_range {
                    let body = get_buffer_lines(lo, hi).join("\n") + "\n";
                    let out = shell_filter(rest[1..].trim(), &body);
                    CURBUF.with(|b| {
                        let mut b = b.borrow_mut();
                        let (a, z) = ((lo - 1) as usize, (hi as usize).min(b.len()));
                        b.drain(a..z);
                        for (i, l) in out.into_iter().enumerate() {
                            b.insert(a + i, l);
                        }
                        if b.is_empty() {
                            b.push(String::new());
                        }
                    });
                    set_cursorpos(lo, 1);
                }
            } else {
                let count = rest.chars().take_while(|&x| x == c).count();
                ex_shift(lo, hi, c == '>', count);
            }
            ExCmdResult::Handled
        }
        // A bare line range with no command word (`:'>`, `:'<`, `:5`, `:.`) moves
        // the cursor to the (last) addressed line — `do_excmd()` runs the range's
        // implicit `:` which is "print"/goto in an editor. Editor-less, the
        // observable effect is the cursor move. Recognized so a mark-address line
        // like `'>` in a function body parses instead of aborting the `:function`.
        "" if had_range => {
            set_cursorpos(hi, 1);
            ExCmdResult::Handled
        }
        // Script-language interface commands (`:python`/`:py3`/`:ruby`/`:perl`/
        // `:lua`/`:tcl`/`:mzscheme` and variants) run an embedded interpreter that
        // is not compiled in (`has('ruby')` etc. are 0), so every form is a no-op
        // editor-less. Recognized here — via the same set the parser routes to
        // `Stmt::ExCmd` — so an executed interface line resolves to `Handled`
        // instead of `NotEx`, which would re-parse it back into an `ExCmd` and
        // recurse without bound (E169). The command word may carry a trailing
        // digit (`python3`, `py3`), so it is matched on `rest`, not the alpha `cmd`.
        _ if crate::viml_parser::is_script_lang_cmd(rest) => ExCmdResult::Handled,
        _ => ExCmdResult::NotEx,
    }
}

/// Port of `ex_sort()` (Neovim ex_cmds.c) — sort buffer lines `lo`..`hi`. Flags
/// (in `args` before an optional `/pat/`): `n` numeric, `i` ignore case, `u`
/// unique, `!` reverse; a `/pat/` sorts by the text matching after `pat`.
fn ex_sort(lo: varnumber_T, hi: varnumber_T, reverse: bool, args: &str) {
    let flags: String = args.chars().take_while(|c| *c != '/').collect();
    let numeric = flags.contains('n');
    let ignorecase = flags.contains('i');
    let unique = flags.contains('u');
    let pat: String = {
        let f = split_delim(args.trim_start_matches(|c: char| c != '/'), 2);
        f.first().cloned().unwrap_or_default()
    };
    let mut lines = get_buffer_lines(lo, hi);
    let key = |s: &str| -> String {
        if pat.is_empty() {
            s.to_string()
        } else {
            let (_, _, e) = crate::viml_regex::regex_matchstrpos(&pat, s, ignorecase);
            if e < 0 {
                s.to_string()
            } else {
                s[e as usize..].to_string()
            }
        }
    };
    if numeric {
        let num = |s: &str| -> i64 {
            let k = key(s);
            let t = k.trim_start();
            let digits: String = t
                .chars()
                .skip_while(|c| !c.is_ascii_digit() && *c != '-')
                .take_while(|c| c.is_ascii_digit() || *c == '-')
                .collect();
            digits.parse().unwrap_or(0)
        };
        lines.sort_by_key(|s| num(s));
    } else if ignorecase {
        lines.sort_by_key(|s| key(s).to_lowercase());
    } else {
        lines.sort_by_key(|s| key(s));
    }
    if reverse {
        lines.reverse();
    }
    if unique {
        lines.dedup();
    }
    CURBUF.with(|b| {
        let mut b = b.borrow_mut();
        let (a, z) = ((lo - 1) as usize, (hi as usize).min(b.len()));
        if a < z {
            b.drain(a..z);
            for (i, l) in lines.into_iter().enumerate() {
                b.insert(a + i, l);
            }
        }
        if b.is_empty() {
            b.push(String::new());
        }
    });
}

/// Port of `ex_operators()`/`shift_line()` (Neovim ex_cmds.c / ops.c) —
/// indent (`>`) or dedent (`<`) lines `lo`..`hi` by `count` × 'shiftwidth'.
fn ex_shift(lo: varnumber_T, hi: varnumber_T, indent: bool, count: usize) {
    let sw = {
        let t = tv_get_number_chk(&get_option_value("shiftwidth"), None);
        if t > 0 {
            t as usize
        } else {
            8
        }
    };
    let amount = sw * count.max(1);
    for lnum in lo..=hi {
        let line = get_buffer_lines(lnum, lnum)
            .into_iter()
            .next()
            .unwrap_or_default();
        let new = if indent {
            if line.trim().is_empty() {
                line
            } else {
                format!("{}{}", " ".repeat(amount), line)
            }
        } else {
            let drop = line.chars().take(amount).take_while(|c| *c == ' ').count();
            line[drop..].to_string()
        };
        set_buffer_lines(lnum, vec![new], false);
    }
}

/// Run a shell command and capture its stdout lines (`:r !cmd`).
fn shell_capture(cmd: &str) -> Vec<String> {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Pipe text to a shell command's stdin, discarding its output (`:w !cmd`).
fn shell_feed(cmd: &str, input: &str) {
    use std::io::Write;
    if let Ok(mut child) = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(input.as_bytes());
        }
        let _ = child.wait();
    }
}

/// Filter text through a shell command, returning its stdout lines (`:range!cmd`).
fn shell_filter(cmd: &str, input: &str) -> Vec<String> {
    use std::io::Write;
    let child = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(_) => return input.lines().map(str::to_string).collect(),
    };
    if let Some(stdin) = child.stdin.take() {
        let mut stdin = stdin;
        let _ = stdin.write_all(input.as_bytes());
    }
    child
        .wait_with_output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Port of `ex_move()` (Neovim ex_cmds.c) — move lines `lo`..`hi` to after
/// `dest` (0 = before the first line).
fn ex_move(lo: varnumber_T, hi: varnumber_T, dest: varnumber_T) {
    let moved = get_buffer_lines(lo, hi);
    CURBUF.with(|b| {
        let mut b = b.borrow_mut();
        let (a, z) = ((lo - 1) as usize, (hi as usize).min(b.len()));
        b.drain(a..z);
        // Adjust the destination for the removed block above it.
        let mut at = dest;
        if dest >= hi {
            at -= hi - lo + 1;
        } else if dest >= lo {
            at = lo - 1;
        }
        let pos = (at.max(0) as usize).min(b.len());
        for (i, l) in moved.into_iter().enumerate() {
            b.insert(pos + i, l);
        }
    });
}

/// Port of `ex_copy()` (Neovim ex_cmds.c) — copy lines `lo`..`hi` to after
/// `dest`.
fn ex_copy(lo: varnumber_T, hi: varnumber_T, dest: varnumber_T) {
    let copied = get_buffer_lines(lo, hi);
    set_buffer_lines(dest, copied, true);
}

/// Port of `ex_join()` (Neovim ex_cmds.c) — join lines `lo`..`hi` into one. With
/// `keep` (`:j!`) the lines are concatenated verbatim; otherwise leading
/// whitespace of joined-on lines is dropped and a single space inserted.
fn ex_join(lo: varnumber_T, hi: varnumber_T, keep: bool) {
    let hi = hi.max(lo + 1);
    let lines = get_buffer_lines(lo, hi);
    if lines.len() < 2 {
        return;
    }
    let mut joined = lines[0].clone();
    for l in &lines[1..] {
        if keep {
            joined.push_str(l);
        } else {
            if !joined.is_empty() && !joined.ends_with(' ') {
                joined.push(' ');
            }
            joined.push_str(l.trim_start());
        }
    }
    CURBUF.with(|b| {
        let mut b = b.borrow_mut();
        let (a, z) = ((lo - 1) as usize, (hi as usize).min(b.len()));
        if a < z {
            b.drain(a..z);
            b.insert(a, joined);
        }
    });
    set_cursorpos(lo, 1);
}

// ── Normal-mode commands (Neovim normal.c / ops.c), a bounded subset that runs
//    motions, deletes, yanks, puts and simple edits on the in-memory buffer.
//    No insert mode (i/a/o…), so editing keys map to their delete equivalents. ──

/// Character class for word motions: 0 blank, 1 keyword (alnum/`_`), 2 other.
fn char_class(c: char) -> u8 {
    if c.is_whitespace() {
        0
    } else if c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

/// Port of `do_normal()` / `normal_cmd()` (Neovim normal.c), bounded subset.
/// Runs the key sequence `keys` against the current buffer/cursor (ASCII; the
/// cursor column is treated as a character index).
pub fn do_normal(keys: &str) {
    let k: Vec<char> = keys.chars().collect();
    let nlines = || curbuf_len();
    let line_chars = |l: varnumber_T| -> Vec<char> {
        get_buffer_lines(l, l)
            .into_iter()
            .next()
            .unwrap_or_default()
            .chars()
            .collect()
    };
    let put_line = |l: varnumber_T, cs: &[char]| {
        set_buffer_lines(l, vec![cs.iter().collect()], false);
    };
    let cur = || {
        let __p = crate::fusevm_bridge::editor_curpos(CURPOS.with(|c| *c.borrow()));
        (__p.0, __p.1)
    };
    // Forward word motion (`w`): step to the next word's start.
    let word_fwd = |l0: varnumber_T, ci0: usize| -> (varnumber_T, usize) {
        let (mut l, mut ci) = (l0, ci0);
        let mut line = line_chars(l);
        if ci < line.len() {
            let cls = char_class(line[ci]);
            while ci < line.len() && char_class(line[ci]) == cls && cls != 0 {
                ci += 1;
            }
        }
        loop {
            while ci < line.len() && char_class(line[ci]) == 0 {
                ci += 1;
            }
            if ci < line.len() {
                break;
            }
            if l < nlines() {
                l += 1;
                line = line_chars(l);
                ci = 0;
            } else {
                ci = line.len();
                break;
            }
        }
        (l, ci)
    };
    // Word-end motion (`e`): step to the end of the next word.
    let word_end = |l0: varnumber_T, ci0: usize| -> (varnumber_T, usize) {
        let line = line_chars(l0);
        let mut ci = ci0 + 1;
        while ci < line.len() && char_class(line[ci]) == 0 {
            ci += 1;
        }
        if ci < line.len() {
            let cls = char_class(line[ci]);
            while ci + 1 < line.len() && char_class(line[ci + 1]) == cls {
                ci += 1;
            }
        }
        (l0, ci.min(line.len().saturating_sub(1)))
    };
    // Back word motion (`b`).
    let word_back = |l0: varnumber_T, ci0: usize| -> (varnumber_T, usize) {
        let line = line_chars(l0);
        let mut ci = ci0;
        if ci == 0 {
            return (l0, 0);
        }
        ci -= 1;
        while ci > 0 && char_class(line[ci]) == 0 {
            ci -= 1;
        }
        if ci < line.len() {
            let cls = char_class(line[ci]);
            while ci > 0 && char_class(line[ci - 1]) == cls {
                ci -= 1;
            }
        }
        (l0, ci)
    };

    let mut i = 0usize;
    while i < k.len() {
        // Leading count (a leading `0` is the start-of-line motion, not a count).
        let mut count: i64 = 0;
        let mut has_count = false;
        while i < k.len() && k[i].is_ascii_digit() && (k[i] != '0' || has_count) {
            count = count * 10 + (k[i] as i64 - '0' as i64);
            has_count = true;
            i += 1;
        }
        if i >= k.len() {
            break;
        }
        let cmd = k[i];
        i += 1;
        let n = if has_count { count.max(1) } else { 1 };
        let (l, ci) = {
            let (cl, cc) = cur();
            (cl, (cc - 1).max(0) as usize)
        };
        match cmd {
            'h' => set_cursorpos(l, (ci as varnumber_T + 1 - n).max(1)),
            'l' => set_cursorpos(l, ci as varnumber_T + 1 + n),
            '0' => set_cursorpos(l, 1),
            '^' => {
                let line = line_chars(l);
                let f = line.iter().position(|c| !c.is_whitespace()).unwrap_or(0);
                set_cursorpos(l, f as varnumber_T + 1);
            }
            '$' => {
                let len = line_chars(l).len().max(1);
                set_cursorpos(l, len as varnumber_T);
            }
            'j' => set_cursorpos((l + n).min(nlines()), ci as varnumber_T + 1),
            'k' => set_cursorpos((l - n).max(1), ci as varnumber_T + 1),
            'G' => set_cursorpos(if has_count { count } else { nlines() }, 1),
            '|' => set_cursorpos(l, n),
            'g' => {
                if i < k.len() && k[i] == 'g' {
                    i += 1;
                    set_cursorpos(if has_count { count } else { 1 }, 1);
                }
            }
            'w' => {
                let (mut tl, mut tc) = (l, ci);
                for _ in 0..n {
                    let (a, b) = word_fwd(tl, tc);
                    tl = a;
                    tc = b;
                }
                set_cursorpos(tl, tc as varnumber_T + 1);
            }
            'b' => {
                let (mut tl, mut tc) = (l, ci);
                for _ in 0..n {
                    let (a, b) = word_back(tl, tc);
                    tl = a;
                    tc = b;
                }
                set_cursorpos(tl, tc as varnumber_T + 1);
            }
            'e' => {
                let (mut tl, mut tc) = (l, ci);
                for _ in 0..n {
                    let (a, b) = word_end(tl, tc);
                    tl = a;
                    tc = b;
                }
                set_cursorpos(tl, tc as varnumber_T + 1);
            }
            'x' => {
                let mut line = line_chars(l);
                let end = (ci + n as usize).min(line.len());
                if ci < line.len() {
                    let removed: String = line[ci..end].iter().collect();
                    write_reg_contents_lst('"', vec![removed], MotionType::CharWise, false);
                    line.drain(ci..end);
                    put_line(l, &line);
                    set_cursorpos(
                        l,
                        (ci as varnumber_T + 1).min(line.len().max(1) as varnumber_T),
                    );
                }
            }
            'X' => {
                let mut line = line_chars(l);
                let start = ci.saturating_sub(n as usize);
                if ci > 0 {
                    line.drain(start..ci);
                    put_line(l, &line);
                    set_cursorpos(l, start as varnumber_T + 1);
                }
            }
            'D' | 'C' => {
                let mut line = line_chars(l);
                if ci < line.len() {
                    let removed: String = line[ci..].iter().collect();
                    write_reg_contents_lst('"', vec![removed], MotionType::CharWise, false);
                    line.truncate(ci);
                    put_line(l, &line);
                }
            }
            'd' | 'c' | 'y' => {
                let op = cmd;
                let m = if i < k.len() { k[i] } else { ' ' };
                i += 1;
                // Doubled operator (dd/cc/yy) → linewise on `n` lines.
                if m == op || (op == 'c' && m == 'c') {
                    let last = (l + n - 1).min(nlines());
                    let lines = get_buffer_lines(l, last);
                    write_reg_contents_lst('"', lines, MotionType::LineWise, false);
                    if op != 'y' {
                        CURBUF.with(|b| {
                            let mut b = b.borrow_mut();
                            let (a, z) = ((l - 1) as usize, (last as usize).min(b.len()));
                            b.drain(a..z);
                            if b.is_empty() {
                                b.push(String::new());
                            }
                        });
                        set_cursorpos(l, 1);
                    }
                } else {
                    // Charwise operator + motion on the current line.
                    let line = line_chars(l);
                    let target = match m {
                        '$' => line.len(),
                        '0' => 0,
                        'w' => word_fwd(l, ci).1.min(line.len()),
                        'e' => word_end(l, ci).1 + 1,
                        'l' => (ci + 1).min(line.len()),
                        'h' => ci.saturating_sub(1),
                        _ => ci,
                    };
                    let (a, z) = (ci.min(target), ci.max(target));
                    let removed: String = line[a..z.min(line.len())].iter().collect();
                    write_reg_contents_lst('"', vec![removed], MotionType::CharWise, false);
                    if op != 'y' {
                        let mut nl = line.clone();
                        nl.drain(a..z.min(nl.len()));
                        put_line(l, &nl);
                        set_cursorpos(l, a as varnumber_T + 1);
                    }
                }
            }
            'Y' => {
                let last = (l + n - 1).min(nlines());
                write_reg_contents_lst('"', get_buffer_lines(l, last), MotionType::LineWise, false);
            }
            'p' | 'P' => {
                let (mtype, _) = get_reg_type('"');
                if let Some(reg) = get_reg_contents('"') {
                    if mtype == MotionType::LineWise {
                        let at = if cmd == 'p' { l } else { l - 1 };
                        set_buffer_lines(at, reg.clone(), true);
                        set_cursorpos(at + 1, 1);
                    } else {
                        let mut line = line_chars(l);
                        let at = if cmd == 'p' {
                            (ci + 1).min(line.len())
                        } else {
                            ci
                        };
                        let text: Vec<char> = reg.join("").chars().collect();
                        for (j, c) in text.iter().enumerate() {
                            line.insert(at + j, *c);
                        }
                        put_line(l, &line);
                        set_cursorpos(l, (at + text.len()) as varnumber_T);
                    }
                }
            }
            'J' => {
                let last = (l + n.max(2) - 1).min(nlines());
                ex_join(l, last, false);
            }
            'r' => {
                if i < k.len() {
                    let rc = k[i];
                    i += 1;
                    let mut line = line_chars(l);
                    for j in 0..n as usize {
                        if ci + j < line.len() {
                            line[ci + j] = rc;
                        }
                    }
                    put_line(l, &line);
                }
            }
            '~' => {
                let mut line = line_chars(l);
                for j in 0..n as usize {
                    if ci + j < line.len() {
                        let c = line[ci + j];
                        line[ci + j] = if c.is_uppercase() {
                            c.to_ascii_lowercase()
                        } else {
                            c.to_ascii_uppercase()
                        };
                    }
                }
                put_line(l, &line);
                set_cursorpos(l, (ci + n as usize).min(line.len().max(1)) as varnumber_T);
            }
            _ => {}
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Round-3 builtin expansion. Command-line state (ex_getln.c), sign placement
// (sign.c), and a set of editor-feature queries whose answer is well-defined
// when no editor is attached (indent.c / fold.c / highlight.c / diff.c /
// search.c / popupmenu / cmdexpand). All outside the vendored vendor/eval/ tree,
// so recorded in the drift-gate allowlist.
// ════════════════════════════════════════════════════════════════════════════

// ── setcmdline()/getcmdline()/setcmdpos()/getcmdpos()/getcmdtype() —
//    Neovim ex_getln.c. Standalone we model a settable command-line buffer. ──

thread_local! {
    /// The command-line buffer state (`ccline` in ex_getln.c): `(line, pos,
    /// type)`. `pos` is the 1-based byte position of the cursor.
    ///
    /// Public so an embedding editor can publish its *real* command line here
    /// (see `fusevm_bridge::cmdline_host_publish`) — standalone, the builtins
    /// below are the only things that ever write it.
    pub static CMDLINE: std::cell::RefCell<(String, varnumber_T, String)> =
        const { std::cell::RefCell::new((String::new(), 0, String::new())) };
}

/// Port of `f_setcmdline()` (Neovim ex_getln.c) — replace the command-line
/// contents with `{str}` and, optionally, move the cursor to byte `{pos}`.
/// Returns 0 on success.
pub fn f_setcmdline(argvars: &[typval_T], rettv: &mut typval_T) {
    let line = tv_get_string(&argvars[0]);
    let pos = if argvars.len() >= 2 {
        tv_get_number(&argvars[1])
    } else {
        line.len() as varnumber_T + 1
    };
    CMDLINE.with(|c| {
        let mut c = c.borrow_mut();
        c.0 = line;
        c.1 = pos;
    });
    rettv.vval = v_number(0);
}

/// Port of `f_getcmdline()` (Neovim ex_getln.c) — the current command-line
/// contents ("" when no command line is active).
pub fn f_getcmdline(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(CMDLINE.with(|c| c.borrow().0.clone()));
}

/// Port of `f_setcmdpos()` (Neovim ex_getln.c) — set the cursor to byte
/// position `{pos}` (1-based) on the command line. Returns 0 on success.
pub fn f_setcmdpos(argvars: &[typval_T], rettv: &mut typval_T) {
    let pos = tv_get_number(&argvars[0]);
    CMDLINE.with(|c| c.borrow_mut().1 = pos);
    rettv.vval = v_number(0);
}

/// Port of `f_getcmdpos()` (Neovim ex_getln.c) — the 1-based byte position of
/// the cursor on the command line (0 when no command line is active).
pub fn f_getcmdpos(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(CMDLINE.with(|c| c.borrow().1));
}

/// Port of `f_getcmdtype()` (Neovim ex_getln.c) — the command-line type char
/// (`:`/`/`/`?`/…), "" when no command line is active.
pub fn f_getcmdtype(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(CMDLINE.with(|c| c.borrow().2.clone()));
}

// ── sign_place()/sign_getplaced()/sign_unplace()/sign_placelist()/
//    sign_unplacelist()/sign_jump() — Neovim sign.c (placed-sign list). ──

thread_local! {
    /// Placed signs (`buf_T.b_signlist` per buffer in sign.c): each a Dict
    /// `{id, group, name, bufnr, lnum, priority}`.
    static SIGNS_PLACED: std::cell::RefCell<Vec<typval_T>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Counter for auto-assigned placed-sign ids (`id == 0`).
    static SIGN_LAST_ID: std::cell::Cell<varnumber_T> = const { std::cell::Cell::new(0) };
}

/// Place one sign and return its id. Shared by `sign_place()`/`sign_placelist()`.
fn sign_place_one(
    id: varnumber_T,
    group: &str,
    name: &str,
    bufnr: varnumber_T,
    lnum: varnumber_T,
    priority: varnumber_T,
) -> varnumber_T {
    let id = if id == 0 {
        SIGN_LAST_ID.with(|c| {
            let v = c.get() + 1;
            c.set(v);
            v
        })
    } else {
        id
    };
    let d = tv_dict_alloc();
    {
        let mut db = d.borrow_mut();
        tv_dict_add_nr(&mut db, "id", id);
        tv_dict_add_str(&mut db, "group", group);
        tv_dict_add_str(&mut db, "name", name);
        tv_dict_add_nr(&mut db, "bufnr", bufnr);
        tv_dict_add_nr(&mut db, "lnum", lnum);
        tv_dict_add_nr(&mut db, "priority", priority);
    }
    SIGNS_PLACED.with(|s| s.borrow_mut().push(match_dict_val(d)));
    id
}

/// Read a Number field from a Dict typval, with a default.
fn sign_dict_nr(d: &typval_T, key: &str, def: varnumber_T) -> varnumber_T {
    match (d.v_type, &d.vval) {
        (VAR_DICT, v_dict(Some(dd))) => tv_dict_find(&dd.borrow(), key)
            .map(tv_get_number)
            .unwrap_or(def),
        _ => def,
    }
}

/// Read a String field from a Dict typval, with a default.
fn sign_dict_str(d: &typval_T, key: &str, def: &str) -> String {
    match (d.v_type, &d.vval) {
        (VAR_DICT, v_dict(Some(dd))) => tv_dict_find(&dd.borrow(), key)
            .map(tv_get_string)
            .unwrap_or_else(|| def.to_string()),
        _ => def.to_string(),
    }
}

/// Port of `f_sign_place()` (Neovim sign.c) — place sign `{name}` (group
/// `{group}`) in buffer `{buf}` at the line given by `{dict}.lnum`. `{id}` 0
/// auto-assigns. Returns the sign id, or -1 on error.
pub fn f_sign_place(argvars: &[typval_T], rettv: &mut typval_T) {
    let id = tv_get_number(&argvars[0]);
    let group = tv_get_string(&argvars[1]);
    let name = tv_get_string(&argvars[2]);
    let bufnr = if argvars[3].v_type == VAR_NUMBER {
        tv_get_number(&argvars[3])
    } else {
        1
    };
    let dict = argvars.get(4);
    let lnum = dict.map_or(1, |d| sign_dict_nr(d, "lnum", 1));
    let priority = dict.map_or(10, |d| sign_dict_nr(d, "priority", 10));
    rettv.vval = v_number(sign_place_one(id, &group, &name, bufnr, lnum, priority));
}

/// Port of `f_sign_placelist()` (Neovim sign.c) — place every sign in `{list}`
/// (each a Dict). Returns the List of assigned ids (-1 for a bad entry).
pub fn f_sign_placelist(argvars: &[typval_T], rettv: &mut typval_T) {
    let l = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => l.clone(),
        _ => {
            emsg("E714: List required");
            return;
        }
    };
    let items: Vec<typval_T> = l
        .borrow()
        .lv_items
        .iter()
        .map(|it| it.li_tv.clone())
        .collect();
    let out = tv_list_alloc_ret(rettv, items.len() as isize);
    let mut ob = out.borrow_mut();
    for it in items {
        if it.v_type != VAR_DICT {
            tv_list_append_number(&mut ob, -1);
            continue;
        }
        let id = sign_dict_nr(&it, "id", 0);
        let group = sign_dict_str(&it, "group", "");
        let name = sign_dict_str(&it, "name", "");
        let bufnr = sign_dict_nr(&it, "buffer", 1);
        let lnum = sign_dict_nr(&it, "lnum", 1);
        let priority = sign_dict_nr(&it, "priority", 10);
        tv_list_append_number(
            &mut ob,
            sign_place_one(id, &group, &name, bufnr, lnum, priority),
        );
    }
}

/// Port of `f_sign_getplaced()` (Neovim sign.c) — the placed signs as
/// `[{bufnr, signs: [...]}]`, grouped by buffer. An optional `{buf}` and a
/// `{dict}` with `group`/`id`/`lnum` narrow the result.
pub fn f_sign_getplaced(argvars: &[typval_T], rettv: &mut typval_T) {
    let want_buf = argvars
        .first()
        .filter(|t| t.v_type == VAR_NUMBER)
        .map(tv_get_number);
    let dict = argvars.get(1);
    let want_group = dict.and_then(|d| match (d.v_type, &d.vval) {
        (VAR_DICT, v_dict(Some(dd))) => tv_dict_find(&dd.borrow(), "group").map(tv_get_string),
        _ => None,
    });
    let placed: Vec<typval_T> = SIGNS_PLACED.with(|s| s.borrow().clone());
    // Group surviving signs by bufnr, preserving insertion order.
    let mut buffers: Vec<varnumber_T> = Vec::new();
    let mut grouped: std::collections::BTreeMap<varnumber_T, Vec<typval_T>> =
        std::collections::BTreeMap::new();
    for sg in placed {
        let bufnr = sign_dict_nr(&sg, "bufnr", 0);
        if let Some(wb) = want_buf {
            if wb != bufnr {
                continue;
            }
        }
        if let Some(wg) = &want_group {
            if *wg != sign_dict_str(&sg, "group", "") {
                continue;
            }
        }
        if !buffers.contains(&bufnr) {
            buffers.push(bufnr);
        }
        grouped.entry(bufnr).or_default().push(sg);
    }
    let out = tv_list_alloc_ret(rettv, buffers.len() as isize);
    let mut ob = out.borrow_mut();
    for bufnr in buffers {
        let signs = grouped.remove(&bufnr).unwrap_or_default();
        let entry = tv_dict_alloc();
        {
            let mut eb = entry.borrow_mut();
            tv_dict_add_nr(&mut eb, "bufnr", bufnr);
            let sl = tv_list_alloc(signs.len() as isize);
            {
                let mut slb = sl.borrow_mut();
                for sg in signs {
                    tv_list_append_tv(&mut slb, sg);
                }
            }
            tv_dict_add_tv(
                &mut eb,
                "signs",
                typval_T {
                    v_type: VAR_LIST,
                    v_lock: VarLockStatus::VAR_UNLOCKED,
                    vval: v_list(Some(sl)),
                },
            );
        }
        tv_list_append_tv(&mut ob, match_dict_val(entry));
    }
}

/// Port of `f_sign_unplace()` (Neovim sign.c) — remove placed signs in
/// `{group}` (""/"*" = all groups), optionally narrowed by `{dict}.id`/
/// `{dict}.buffer`. Returns 0 on success.
pub fn f_sign_unplace(argvars: &[typval_T], rettv: &mut typval_T) {
    let group = tv_get_string(&argvars[0]);
    let dict = argvars.get(1);
    let want_id = dict.map(|d| sign_dict_nr(d, "id", 0)).filter(|v| *v != 0);
    let want_buf = dict
        .map(|d| sign_dict_nr(d, "buffer", -1))
        .filter(|v| *v != -1);
    SIGNS_PLACED.with(|s| {
        s.borrow_mut().retain(|sg| {
            let g_ok = group.is_empty() || group == "*" || group == sign_dict_str(sg, "group", "");
            #[allow(clippy::unnecessary_map_or)]
            let id_ok = want_id.map_or(true, |w| w == sign_dict_nr(sg, "id", 0));
            #[allow(clippy::unnecessary_map_or)]
            let buf_ok = want_buf.map_or(true, |w| w == sign_dict_nr(sg, "bufnr", 0));
            // Keep the sign unless every selector matches.
            !(g_ok && id_ok && buf_ok)
        });
    });
    rettv.vval = v_number(0);
}

/// Port of `f_sign_unplacelist()` (Neovim sign.c) — unplace each sign described
/// in `{list}`. Returns a List of per-item results (0 on success).
pub fn f_sign_unplacelist(argvars: &[typval_T], rettv: &mut typval_T) {
    let l = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => l.clone(),
        _ => {
            emsg("E714: List required");
            return;
        }
    };
    let items: Vec<typval_T> = l
        .borrow()
        .lv_items
        .iter()
        .map(|it| it.li_tv.clone())
        .collect();
    let out = tv_list_alloc_ret(rettv, items.len() as isize);
    let mut ob = out.borrow_mut();
    for it in items {
        let group = sign_dict_str(&it, "group", "");
        let id = sign_dict_nr(&it, "id", 0);
        SIGNS_PLACED.with(|s| {
            s.borrow_mut().retain(|sg| {
                let g_ok = group.is_empty() || group == sign_dict_str(sg, "group", "");
                let id_ok = id == 0 || id == sign_dict_nr(sg, "id", 0);
                !(g_ok && id_ok)
            });
        });
        tv_list_append_number(&mut ob, 0);
    }
}

/// Port of `f_sign_jump()` (Neovim sign.c) — jump the cursor to sign `{id}`.
/// No editor cursor standalone, so it reports failure (-1).
pub fn f_sign_jump(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(-1);
}

// ── editor-feature queries with a well-defined "no editor" answer. ──

/// Port of `f_indent()` (Neovim indent.c) — the indent (leading-whitespace
/// screen width, Tabs expanded to 'tabstop') of line `{lnum}`, -1 if invalid.
pub fn f_indent(argvars: &[typval_T], rettv: &mut typval_T) {
    let lnum = tv_get_lnum(&argvars[0]);
    if lnum < 1 || lnum > curbuf_len() {
        rettv.vval = v_number(-1);
        return;
    }
    // c: the indent is the screen width of the leading whitespace; a Tab
    // advances to the next 'tabstop' (default 8) boundary.
    let line = get_buffer_lines(lnum, lnum)
        .into_iter()
        .next()
        .unwrap_or_default();
    let ts = {
        let t = tv_get_number_chk(&get_option_value("tabstop"), None);
        if t > 0 {
            t as varnumber_T
        } else {
            8
        }
    };
    let mut col: varnumber_T = 0;
    for c in line.chars() {
        match c {
            ' ' => col += 1,
            '\t' => col += ts - (col % ts),
            _ => break,
        }
    }
    rettv.vval = v_number(col);
}

/// Port of `f_foldtext()` (Neovim fold.c) — the text shown for the closed fold
/// on the current line. No folds standalone → "".
pub fn f_foldtext(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(String::new());
}

/// Port of `f_foldtextresult()` (Neovim fold.c) — the `'foldtext'` text for the
/// fold at `{lnum}`. No folds standalone → "".
pub fn f_foldtextresult(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(String::new());
}

/// Port of `f_highlight_exists()` (Neovim highlight_group.c) — whether highlight
/// group `{name}` is defined. Tracks groups defined by a sourced
/// colorscheme/vimrc via the highlight registry (see [`HL_GROUPS`]).
pub fn f_highlight_exists(argvars: &[typval_T], rettv: &mut typval_T) {
    let name = argvars.first().map(tv_get_string).unwrap_or_default();
    rettv.vval = v_number(hl_exists(&name) as varnumber_T);
}

/// Port of `f_diff_filler()` (Neovim diff.c) — the number of filler lines above
/// line `{lnum}`. No diff mode standalone → 0.
pub fn f_diff_filler(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(0);
}

/// Port of `f_hlID()` — `vendor/eval/funcs.c:2894`. The numeric ID of the highlight
/// group named `{name}`, or 0 when it does not exist. Standalone there are no
/// highlight groups, so `syn_name2id()` finds nothing → 0. `highlightID()` is the
/// deprecated alias and shares this implementation.
pub fn f_hlID(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: rettv->vval.v_number = syn_name2id(tv_get_string(&argvars[0]));
    let name = argvars.first().map(tv_get_string).unwrap_or_default();
    rettv.vval = v_number(hl_id(&name));
}

/// Port of `f_diff_hlID()` (Neovim diff.c) — the highlight ID for diff mode at
/// line `{lnum}` column `{col}`. With no diff change (no diff mode standalone)
/// the C returns 0.
pub fn f_diff_hlID(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(0);
}

/// Port of `f_virtcol2col()` (Neovim plines.c) — the byte index of the
/// character at virtual column `{virtcol}`. No buffer standalone → -1.
pub fn f_virtcol2col(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(-1);
}

/// Port of `f_wildtrigger()` (Neovim cmdexpand.c) — trigger wildcard completion
/// on the command line. No interactive command line standalone → no-op.
pub fn f_wildtrigger(_argvars: &[typval_T], _rettv: &mut typval_T) {}

/// Port of `f_searchcount()` (Neovim search.c) — the search-count dict
/// `{current, total, exact_match, incomplete, maxcount}`. No active search
/// standalone → all-zero counts (maxcount defaults to 99).
pub fn f_searchcount(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: the optional {options} Dict may override the pattern and maxcount.
    let mut pat = LAST_SEARCH.with(|p| p.borrow().clone());
    let mut maxcount: varnumber_T = 99;
    if let Some((VAR_DICT, v_dict(Some(opt)))) = argvars.first().map(|t| (t.v_type, &t.vval)) {
        let opt = opt.borrow();
        if let Some(p) = tv_dict_find(&opt, "pattern") {
            pat = tv_get_string(p);
        }
        if let Some(m) = tv_dict_find(&opt, "maxcount") {
            maxcount = tv_get_number(m);
        }
    }
    let (mut total, mut current, mut exact) = (0i64, 0i64, 0i64);
    if !pat.is_empty() {
        let ic = tv_get_number_chk(&get_option_value("ignorecase"), None) != 0;
        let (clnum, ccol) = CURPOS.with(|c| {
            let c = c.borrow();
            (c.0, c.1)
        });
        for (i, line) in get_buffer_lines(1, curbuf_len()).iter().enumerate() {
            let lnum = i as varnumber_T + 1;
            let mut from = 0usize;
            while let Some((s, e)) = line_match_from(&pat, line, from, ic) {
                total += 1;
                // The cursor is at/after this match → it is the current one.
                if lnum < clnum || (lnum == clnum && (s as varnumber_T) < ccol) {
                    current = total;
                }
                if lnum == clnum && (s as varnumber_T) == ccol - 1 {
                    exact = 1;
                    current = total;
                }
                from = if e > s { e } else { s + 1 };
            }
        }
    }
    let d = tv_dict_alloc_ret(rettv);
    let mut db = d.borrow_mut();
    tv_dict_add_nr(&mut db, "current", current);
    tv_dict_add_nr(&mut db, "total", total);
    tv_dict_add_nr(&mut db, "exact_match", exact);
    tv_dict_add_nr(&mut db, "incomplete", 0);
    tv_dict_add_nr(&mut db, "maxcount", maxcount);
}

/// Port of `f_complete_info()` (Neovim insexpand.c) — the insert-completion
/// state dict. No insert-mode completion standalone → an inactive snapshot.
pub fn f_complete_info(_argvars: &[typval_T], rettv: &mut typval_T) {
    let d = tv_dict_alloc_ret(rettv);
    let mut db = d.borrow_mut();
    tv_dict_add_str(&mut db, "mode", "");
    tv_dict_add_nr(&mut db, "pum_visible", 0);
    let items = tv_list_alloc(0);
    tv_dict_add_tv(
        &mut db,
        "items",
        typval_T {
            v_type: VAR_LIST,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_list(Some(items)),
        },
    );
    tv_dict_add_nr(&mut db, "selected", -1);
}

// ════════════════════════════════════════════════════════════════════════════
// Round-4 builtin expansion. The quickfix/location lists (quickfix.c) are real
// in-memory error lists standalone; getcompletion() does real environment/file
// completion; the remaining input/indent/completion/menu queries return their
// documented "no editor" values. All outside the vendored vendor/eval/ tree.
// ════════════════════════════════════════════════════════════════════════════

// ── getqflist()/setqflist()/getloclist()/setloclist() — Neovim quickfix.c. ──

thread_local! {
    /// The quickfix list (`qf_list` in quickfix.c): `(entries, title)`.
    static QFLIST: std::cell::RefCell<(Vec<typval_T>, String)> =
        const { std::cell::RefCell::new((Vec::new(), String::new())) };
    /// The location list (one window standalone): `(entries, title)`.
    static LOCLIST: std::cell::RefCell<(Vec<typval_T>, String)> =
        const { std::cell::RefCell::new((Vec::new(), String::new())) };
}

/// Build a normalized quickfix entry Dict from a user item (Neovim
/// `qf_add_entry` field population): the full `{bufnr, module, lnum, end_lnum,
/// col, end_col, vcol, nr, pattern, text, type, valid}` schema with defaults.
fn qf_add_entry(item: &typval_T) -> typval_T {
    let d = tv_dict_alloc();
    let bufnr = sign_dict_nr(item, "bufnr", 0);
    let lnum = sign_dict_nr(item, "lnum", 0);
    {
        let mut db = d.borrow_mut();
        tv_dict_add_nr(&mut db, "bufnr", bufnr);
        tv_dict_add_str(&mut db, "module", &sign_dict_str(item, "module", ""));
        tv_dict_add_nr(&mut db, "lnum", lnum);
        tv_dict_add_nr(&mut db, "end_lnum", sign_dict_nr(item, "end_lnum", 0));
        tv_dict_add_nr(&mut db, "col", sign_dict_nr(item, "col", 0));
        tv_dict_add_nr(&mut db, "end_col", sign_dict_nr(item, "end_col", 0));
        tv_dict_add_nr(&mut db, "vcol", sign_dict_nr(item, "vcol", 0));
        tv_dict_add_nr(&mut db, "nr", sign_dict_nr(item, "nr", 0));
        tv_dict_add_str(&mut db, "pattern", &sign_dict_str(item, "pattern", ""));
        tv_dict_add_str(&mut db, "text", &sign_dict_str(item, "text", ""));
        tv_dict_add_str(&mut db, "type", &sign_dict_str(item, "type", ""));
        // c: an entry is valid when it has a real buffer or line number.
        let valid = if bufnr > 0 || lnum > 0 { 1 } else { 0 };
        tv_dict_add_nr(&mut db, "valid", valid);
    }
    match_dict_val(d)
}

/// Apply a `setqflist()`/`setloclist()` operation to one stored list.
fn qf_set_list(
    store: &std::cell::RefCell<(Vec<typval_T>, String)>,
    list: &typval_T,
    action: &str,
    what: Option<&typval_T>,
) -> varnumber_T {
    let mut s = store.borrow_mut();
    if action == "f" {
        s.0.clear();
        s.1.clear();
        return 0;
    }
    let entries: Vec<typval_T> = match (list.v_type, &list.vval) {
        (VAR_LIST, v_list(Some(l))) => l
            .borrow()
            .lv_items
            .iter()
            .filter(|it| it.li_tv.v_type == VAR_DICT)
            .map(|it| qf_add_entry(&it.li_tv))
            .collect(),
        _ => {
            emsg("E714: List required");
            return -1;
        }
    };
    if action == "a" {
        s.0.extend(entries);
    } else {
        // c: ' ' (create) and 'r' (replace) both make this the list contents.
        s.0 = entries;
    }
    if let Some(w) = what {
        if let (VAR_DICT, v_dict(Some(wd))) = (w.v_type, &w.vval) {
            if let Some(t) = tv_dict_find(&wd.borrow(), "title") {
                s.1 = tv_get_string(t);
            }
        }
    }
    0
}

/// Read a stored quickfix/location list either as the entry List, or — when a
/// `{what}` Dict is given — as a Dict of the requested properties.
fn qf_get_list(
    store: &std::cell::RefCell<(Vec<typval_T>, String)>,
    what: Option<&typval_T>,
    rettv: &mut typval_T,
) {
    let s = store.borrow();
    match what {
        Some(w) if w.v_type == VAR_DICT => {
            let keys: Vec<String> = match &w.vval {
                v_dict(Some(wd)) => wd.borrow().dv_hashtab.keys().cloned().collect(),
                _ => Vec::new(),
            };
            let d = tv_dict_alloc_ret(rettv);
            let mut db = d.borrow_mut();
            for k in keys {
                match k.as_str() {
                    "title" => {
                        tv_dict_add_str(&mut db, "title", &s.1);
                    }
                    "nr" => {
                        tv_dict_add_nr(&mut db, "nr", 0);
                    }
                    "size" => {
                        tv_dict_add_nr(&mut db, "size", s.0.len() as varnumber_T);
                    }
                    "winid" => {
                        tv_dict_add_nr(&mut db, "winid", 0);
                    }
                    "items" => {
                        let l = tv_list_alloc(s.0.len() as isize);
                        {
                            let mut lb = l.borrow_mut();
                            for e in &s.0 {
                                tv_list_append_tv(&mut lb, e.clone());
                            }
                        }
                        tv_dict_add_tv(
                            &mut db,
                            "items",
                            typval_T {
                                v_type: VAR_LIST,
                                v_lock: VarLockStatus::VAR_UNLOCKED,
                                vval: v_list(Some(l)),
                            },
                        );
                    }
                    _ => {}
                }
            }
        }
        _ => {
            let l = tv_list_alloc_ret(rettv, s.0.len() as isize);
            let mut lb = l.borrow_mut();
            for e in &s.0 {
                tv_list_append_tv(&mut lb, e.clone());
            }
        }
    }
}

/// Port of `f_setqflist()` (Neovim quickfix.c) — set the quickfix list from a
/// List of entry Dicts. `{action}` ' '/'r' replace, 'a' append, 'f' free.
/// Returns 0 on success, -1 on error.
pub fn f_setqflist(argvars: &[typval_T], rettv: &mut typval_T) {
    let action = if argvars.len() >= 2 {
        tv_get_string(&argvars[1])
    } else {
        String::new()
    };
    rettv.vval = v_number(QFLIST.with(|q| qf_set_list(q, &argvars[0], &action, argvars.get(2))));
}

/// Port of `f_getqflist()` (Neovim quickfix.c) — the quickfix list as a List of
/// entry Dicts, or a properties Dict when a `{what}` argument is given.
pub fn f_getqflist(argvars: &[typval_T], rettv: &mut typval_T) {
    QFLIST.with(|q| qf_get_list(q, argvars.first(), rettv));
}

/// Port of `f_setloclist()` (Neovim quickfix.c) — like `setqflist()` for the
/// location list of window `{nr}` (one window standalone). Returns 0 / -1.
pub fn f_setloclist(argvars: &[typval_T], rettv: &mut typval_T) {
    let action = if argvars.len() >= 3 {
        tv_get_string(&argvars[2])
    } else {
        String::new()
    };
    rettv.vval = v_number(LOCLIST.with(|q| qf_set_list(q, &argvars[1], &action, argvars.get(3))));
}

/// Port of `f_getloclist()` (Neovim quickfix.c) — the location list of window
/// `{nr}` as a List of entry Dicts, or a `{what}` properties Dict.
pub fn f_getloclist(argvars: &[typval_T], rettv: &mut typval_T) {
    LOCLIST.with(|q| qf_get_list(q, argvars.get(1), rettv));
}

// ── getcompletion() — Neovim cmdexpand.c. Real environment/file completion. ──

/// Port of `f_getcompletion()` (Neovim cmdexpand.c) — completion candidates for
/// `{pat}` of `{type}`. `environment` matches env var names; `file`/`dir` walk
/// the filesystem; unsupported types yield an empty List.
pub fn f_getcompletion(argvars: &[typval_T], rettv: &mut typval_T) {
    let pat = tv_get_string(&argvars[0]);
    let typ = tv_get_string(&argvars[1]);
    let mut results: Vec<String> = match typ.as_str() {
        "environment" => std::env::vars()
            .map(|(k, _)| k)
            .filter(|k| k.starts_with(&pat))
            .collect(),
        "dir" | "file" | "file_in_path" | "buffer" => {
            // Split the pattern into a directory prefix and a leaf prefix.
            let (dir, leaf) = match pat.rfind('/') {
                Some(i) => (&pat[..=i], &pat[i + 1..]),
                None => ("", pat.as_str()),
            };
            let readdir = if dir.is_empty() { "." } else { dir };
            let mut out = Vec::new();
            if let Ok(rd) = std::fs::read_dir(readdir) {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if !name.starts_with(leaf) {
                        continue;
                    }
                    let is_dir = e.path().is_dir();
                    if typ == "dir" && !is_dir {
                        continue;
                    }
                    let mut full = format!("{dir}{name}");
                    if is_dir {
                        full.push('/');
                    }
                    out.push(full);
                }
            }
            out
        }
        _ => Vec::new(),
    };
    results.sort();
    let l = tv_list_alloc_ret(rettv, results.len() as isize);
    let mut lb = l.borrow_mut();
    for r in results {
        tv_list_append_string(&mut lb, &r);
    }
}

// ── input / indent / completion / menu queries with a defined standalone
//    answer (getchar.c / indent.c / insexpand.c / cmdexpand.c / menu.c). ──

/// Port of `f_getchar()` (Neovim getchar.c) — get a typed character. No input
/// standalone → 0 (also the non-blocking "nothing available" result).
pub fn f_getchar(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(0);
}

/// Port of `f_getcharstr()` (Neovim getchar.c) — like `getchar()` but a String;
/// no input standalone → "".
pub fn f_getcharstr(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(String::new());
}

/// Port of `f_getcharmod()` (Neovim getchar.c) — the modifier bitmask of the
/// last typed character. No input standalone → 0.
pub fn f_getcharmod(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(0);
}

/// Port of `f_getcmdprompt()` (Neovim ex_getln.c) — the `input()`/`:` prompt
/// text. None active standalone → "".
pub fn f_getcmdprompt(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(String::new());
}

/// Port of `f_getcmdscreenpos()` (Neovim ex_getln.c) — the screen position of
/// the command-line cursor. No command line active standalone → 0.
pub fn f_getcmdscreenpos(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(0);
}

/// Port of `f_getcmdcompltype()` (Neovim cmdexpand.c) — the completion type of
/// the current command line. None active standalone → "".
pub fn f_getcmdcompltype(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(String::new());
}

/// Port of `f_getcmdcomplpat()` (Neovim cmdexpand.c) — the completion pattern of
/// the current command line. None active standalone → "".
pub fn f_getcmdcomplpat(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(String::new());
}

/// Port of `f_cindent()` (Neovim indent.c) — the C-indent for line `{lnum}`. No
/// buffer standalone → -1.
pub fn f_cindent(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(-1);
}

/// Port of `f_lispindent()` (Neovim indent.c) — the Lisp-indent for line
/// `{lnum}`. No buffer standalone → -1.
pub fn f_lispindent(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(-1);
}

/// Port of `f_complete_add()` (Neovim insexpand.c) — add a match during insert
/// completion. Not in insert mode standalone → 0.
pub fn f_complete_add(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(0);
}

/// Port of `f_complete_check()` (Neovim insexpand.c) — whether completion was
/// interrupted by typed input. None standalone → 0.
pub fn f_complete_check(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(0);
}

/// Port of `f_cmdcomplete_info()` (Neovim cmdexpand.c) — command-line
/// completion state. None active standalone → an empty Dict.
pub fn f_cmdcomplete_info(_argvars: &[typval_T], rettv: &mut typval_T) {
    let _ = tv_dict_alloc_ret(rettv);
}

/// Port of `f_menu_info()` (Neovim menu.c) — info about menu `{name}`. No menus
/// standalone → an empty Dict.
pub fn f_menu_info(_argvars: &[typval_T], rettv: &mut typval_T) {
    let _ = tv_dict_alloc_ret(rettv);
}

/// Port of `f_test_garbagecollect_now()` (Neovim eval.c) — force a GC. vimlrs
/// uses Rust ownership, so there is nothing to collect → no-op.
pub fn f_test_garbagecollect_now(_argvars: &[typval_T], _rettv: &mut typval_T) {}

/// Port of `f_test_write_list_log()` (Neovim eval.c) — a debug hook that logs
/// list-allocation activity; no such log standalone → no-op.
pub fn f_test_write_list_log(_argvars: &[typval_T], _rettv: &mut typval_T) {}

// ════════════════════════════════════════════════════════════════════════════
// Round-5 builtin expansion — completing the eval.lua builtin table. Provider
// evals with no provider (eval/funcs.c), undo-file path computation (undofile.c),
// and the remaining mouse/screen/completion/command-name queries. All outside
// the vendored vendor/eval/ tree.
// ════════════════════════════════════════════════════════════════════════════

/// Port of `f_pyeval()` (Neovim if_py.c) — evaluate Python. No Python provider →
/// v:null (the same as `py3eval()`/`perleval()`).
pub fn f_pyeval(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_SPECIAL;
    rettv.vval = v_special(kSpecialVarNull);
}

/// Port of `f_pyxeval()` (Neovim if_pyx.c) — evaluate Python (2-or-3). No
/// provider → v:null.
pub fn f_pyxeval(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_SPECIAL;
    rettv.vval = v_special(kSpecialVarNull);
}

/// Port of `f_undofile()` (Neovim undo.c) — the undo-file path for `{name}`.
/// With the default `'undodir'` of ".", the undo file sits in the file's own
/// directory as `.{name}.un~`. An empty name yields "".
pub fn f_undofile(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    let name = tv_get_string(&argvars[0]);
    if name.is_empty() {
        rettv.vval = v_string(String::new());
        return;
    }
    let p = std::path::Path::new(&name);
    let fname = p
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let undoname = format!(".{fname}.un~");
    let result = match p.parent() {
        Some(par) if !par.as_os_str().is_empty() => {
            par.join(&undoname).to_string_lossy().into_owned()
        }
        _ => undoname,
    };
    rettv.vval = v_string(result);
}

/// Port of `f_undotree()` (Neovim undo.c) — the undo-tree state. No undo history
/// standalone → a synced tree with no entries.
pub fn f_undotree(_argvars: &[typval_T], rettv: &mut typval_T) {
    let d = tv_dict_alloc_ret(rettv);
    let mut db = d.borrow_mut();
    tv_dict_add_nr(&mut db, "seq_last", 0);
    tv_dict_add_nr(&mut db, "seq_cur", 0);
    tv_dict_add_nr(&mut db, "time_cur", 0);
    tv_dict_add_nr(&mut db, "save_last", 0);
    tv_dict_add_nr(&mut db, "save_cur", 0);
    tv_dict_add_nr(&mut db, "synced", 1);
    let entries = tv_list_alloc(0);
    tv_dict_add_tv(
        &mut db,
        "entries",
        typval_T {
            v_type: VAR_LIST,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_list(Some(entries)),
        },
    );
}

/// Port of `f_getmousepos()` (Neovim mouse.c) — the last mouse position. No
/// mouse standalone → an all-zero position dict.
pub fn f_getmousepos(_argvars: &[typval_T], rettv: &mut typval_T) {
    let d = tv_dict_alloc_ret(rettv);
    let mut db = d.borrow_mut();
    for k in [
        "screenrow",
        "screencol",
        "winid",
        "winrow",
        "wincol",
        "line",
        "column",
        "coladd",
    ] {
        tv_dict_add_nr(&mut db, k, 0);
    }
}

/// Port of `f_screenpos()` (Neovim screen.c) — the screen position of buffer
/// line/column `{lnum}`/`{col}` in window `{winid}`. No screen standalone → all
/// zeros.
pub fn f_screenpos(_argvars: &[typval_T], rettv: &mut typval_T) {
    let d = tv_dict_alloc_ret(rettv);
    let mut db = d.borrow_mut();
    for k in ["row", "col", "curscol", "endcol"] {
        tv_dict_add_nr(&mut db, k, 0);
    }
}

/// Port of `f_getcompletiontype()` (Neovim cmdexpand.c) — the completion type
/// that would apply to command-line `{pat}`. No active command line → "".
pub fn f_getcompletiontype(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(String::new());
}

/// Port of `f_mapset()` (Neovim mapping.c) — create/restore a mapping from a
/// `maparg()`-style Dict. Supports both the modern `mapset({dict})` (mode from
/// the dict's `mode`/`mode_bits`) and the older `mapset({mode}, {abbr},
/// {dict})` forms.
pub fn f_mapset(argvars: &[typval_T], _rettv: &mut typval_T) {
    // Locate the option Dict and the explicit mode (3-arg form) if present.
    let (mode_str, dict) = if argvars[0].v_type == VAR_DICT {
        (None, &argvars[0])
    } else {
        match argvars.get(2) {
            Some(d) => (Some(tv_get_string(&argvars[0])), d),
            None => return,
        }
    };
    let d = match (dict.v_type, &dict.vval) {
        (VAR_DICT, v_dict(Some(d))) => d.clone(),
        _ => {
            emsg("E715: Dictionary required");
            return;
        }
    };
    let db = d.borrow();
    let get_s = |k: &str| tv_dict_find(&db, k).map(tv_get_string).unwrap_or_default();
    let get_n = |k: &str| tv_dict_find(&db, k).map(tv_get_number).unwrap_or(0);
    let mode = match mode_str {
        Some(m) => mode_str2flags(&m),
        None => {
            // c: modern form takes the mode from the dict's `mode`/`mode_bits`.
            let mc = get_s("mode");
            if !mc.is_empty() {
                mode_str2flags(&mc)
            } else if tv_dict_find(&db, "mode_bits").is_some() {
                get_n("mode_bits") as i32
            } else {
                MAP_DEFAULT
            }
        }
    };
    let lhs = get_s("lhs");
    if lhs.is_empty() {
        return;
    }
    map_add(mapblock_T {
        lhs,
        rhs: get_s("rhs"),
        mode,
        noremap: get_n("noremap") != 0,
        expr: get_n("expr") != 0,
        silent: get_n("silent") != 0,
        nowait: get_n("nowait") != 0,
        buffer: get_n("buffer") != 0,
        sid: get_n("sid"),
        lnum: get_n("lnum"),
        desc: get_s("desc"),
    });
}

/// Port of `f_complete()` (Neovim insexpand.c) — set the insert-mode completion
/// matches. Only valid in insert mode → a no-op standalone.
pub fn f_complete(_argvars: &[typval_T], _rettv: &mut typval_T) {}

/// Port of `f_preinserted()` (Neovim insexpand.c) — the text pre-inserted by
/// completion. None standalone → "".
pub fn f_preinserted(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(String::new());
}

/// Port of `f_getscriptinfo()` (Neovim runtime.c) — info about sourced scripts.
/// The script registry is not introspectable standalone → an empty List.
pub fn f_getscriptinfo(_argvars: &[typval_T], rettv: &mut typval_T) {
    let _ = tv_list_alloc_ret(rettv, 0);
}

/// Port of `f_getstacktrace()` (Neovim userfunc.c) — the current call stack.
/// Not introspectable standalone → an empty List.
pub fn f_getstacktrace(_argvars: &[typval_T], rettv: &mut typval_T) {
    let _ = tv_list_alloc_ret(rettv, 0);
}

/// Port of `f_fullcommand()` (Neovim ex_docmd.c) — expand a (possibly
/// abbreviated) Ex command `{name}` to its full name, "" if it resolves to no
/// command. Resolves against a table of common commands using Vim's rule: the
/// input must be at least the command's minimum abbreviation and a prefix of
/// its full name.
pub fn f_fullcommand(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    let mut name = tv_get_string(&argvars[0]);
    // c: a leading range/`:`/bang is stripped before matching.
    name = name
        .trim_start_matches([':', ' '])
        .trim_end_matches('!')
        .to_string();
    // (minimum-abbreviation, full-name), in command-table precedence order.
    const CMDS: &[(&str, &str)] = &[
        ("printf", "printf"),
        ("e", "edit"),
        ("ec", "echo"),
        ("echom", "echomsg"),
        ("w", "write"),
        ("wq", "wq"),
        ("q", "quit"),
        ("qa", "quitall"),
        ("s", "substitute"),
        ("sp", "split"),
        ("se", "set"),
        ("so", "source"),
        ("g", "global"),
        ("norm", "normal"),
        ("vs", "vsplit"),
        ("b", "buffer"),
        ("bn", "bnext"),
        ("bp", "bprevious"),
        ("d", "delete"),
        ("y", "yank"),
        ("pu", "put"),
        ("co", "copy"),
        ("m", "move"),
        ("r", "read"),
        ("let", "let"),
        ("cal", "call"),
        ("fu", "function"),
        ("retu", "return"),
        ("if", "if"),
        ("for", "for"),
        ("wh", "while"),
        ("try", "try"),
        ("au", "autocmd"),
        ("com", "command"),
    ];
    let result = if name.is_empty() {
        String::new()
    } else {
        CMDS.iter()
            .find(|(min, full)| name.starts_with(min) && full.starts_with(&name))
            .map(|(_, full)| full.to_string())
            .unwrap_or_default()
    };
    rettv.vval = v_string(result);
}

// ─────────────────────────────────────────────────────────────────────────────
// Reference ports of the remaining portable `funcs.c` leaves.
//
// These mirror the C bodies faithfully. The runtime dispatches the matching
// builtins through the fusevm bytecode bridge and the folded strict `f_*`
// helpers above (`f_col`, `f_search`, `f_setpos`, `f_libcall`, …), so several of
// the functions below are dead-code references kept as the C spec alongside that
// synthesis (see PORT.md and the porting brief). C names are verbatim.
// ─────────────────────────────────────────────────────────────────────────────
#[allow(unused_imports)]
use crate::ported::eval::typval::{
    tv_check_for_dict_arg, tv_check_for_list_arg, tv_check_for_opt_dict_arg,
    tv_check_for_opt_number_arg, tv_check_for_string_or_list_arg, tv_copy,
    tv_dict_add_allocated_str, tv_dict_extend, tv_dict_get_bool, tv_dict_get_string,
    tv_dict_item_remove, tv_list_append_allocated_string, tv_list_append_owned_tv,
};

/// c: funcs.c:5585 — `search*()`/`searchpair*()` flag bits (`#define SP_*`).
const SP_NOMOVE: i32 = 0x01; // don't move cursor
const SP_REPEAT: i32 = 0x02; // repeat to find outer pair
const SP_RETCOUNT: i32 = 0x04; // return matchcount
const SP_SETPCMARK: i32 = 0x08; // set previous context mark
const SP_START: i32 = 0x10; // accept match at start position
const SP_SUBPAT: i32 = 0x20; // return nr of matching sub-pattern
const SP_END: i32 = 0x40; // leave cursor at end of match
const SP_COLUMN: i32 = 0x80; // start at cursor column

/// `MAXCOL` (`pos_defs.h`) — the maximum column number sentinel.
const MAXCOL: crate::ported::window::colnr_T = crate::ported::window::colnr_T::MAX;

/// `MAX_FUNC_ARGS = 20` (`eval/typval_defs.h:292`) — max function arguments.
const MAX_FUNC_ARGS: i32 = 20;

/// Port of `f_call()` from `Src/eval/funcs.c:547`.
///
/// "call(func, arglist [, dict])" — call `func` with the List `arglist`,
/// optionally bound to a `dict` as `self`.
///
/// RUST-PORT NOTE: the Lua-table callback branch (`nlua_is_table_from_lua` /
/// `nlua_register_table_as_callable`) needs the Lua VM (not modelled) and is
/// omitted; `func_unref`/`xfree(tofree)` cleanup is `Rc`/scope-managed.
pub fn f_call(argvars: &[typval_T], rettv: &mut typval_T) {
    // c:549 if (tv_check_for_list_arg(argvars, 1) == FAIL) return;
    if tv_check_for_list_arg(argvars, 1) == FAIL {
        return;
    }
    // c:552 if (argvars[1].vval.v_list == NULL) return;
    let arglist_null = !matches!(&argvars[1].vval, v_list(Some(_)));
    if arglist_null {
        return;
    }

    // c:558 resolve the function name / partial.
    let mut partial: Option<std::rc::Rc<crate::ported::eval::typval_defs_h::partial_T>> = None;
    let mut func: String = match (&argvars[0].v_type, &argvars[0].vval) {
        // c:560 VAR_FUNC → argvars[0].vval.v_string
        (VAR_FUNC, v_string(s)) => s.clone(),
        // c:562 VAR_PARTIAL → partial_name(partial)
        (VAR_PARTIAL, v_partial(Some(pt))) => {
            partial = Some(pt.clone());
            crate::ported::eval::partial_name(pt).to_string()
        }
        // c:571 else → tv_get_string(&argvars[0])
        _ => tv_get_string(&argvars[0]),
    };

    // c:574 if (func == NULL || *func == NUL) return;
    if func.is_empty() {
        return;
    }

    // c:576 if (argvars[0].v_type == VAR_STRING) trans_function_name(...).
    if argvars[0].v_type == VAR_STRING {
        use crate::ported::eval::userfunc::tfn::{TFN_INT, TFN_QUIET};
        let mut p: &str = &func;
        match crate::ported::eval::userfunc::trans_function_name(
            &mut p,
            false,
            TFN_INT | TFN_QUIET,
            None,
            None,
        ) {
            // c:583 if (tofree == NULL) { emsg_funcname(e_unknown_function_str, func); return; }
            None => {
                crate::ported::eval::userfunc::emsg_funcname("E117: Unknown function: %s", &func);
                return;
            }
            Some(tofree) => func = tofree,
        }
    }

    // c:591 dict_T *selfdict = NULL;
    let mut selfdict: Option<
        std::rc::Rc<std::cell::RefCell<crate::ported::eval::typval_defs_h::dict_T>>,
    > = None;
    if argvars[2].v_type != VAR_UNKNOWN {
        // c:594 if (tv_check_for_dict_arg(argvars, 2) == FAIL) goto done;
        if tv_check_for_dict_arg(argvars, 2) == FAIL {
            return;
        }
        if let v_dict(Some(d)) = &argvars[2].vval {
            selfdict = Some(d.clone());
        }
    }

    // c:600 func_call(func, &argvars[1], partial, selfdict, rettv);
    crate::ported::eval::userfunc::func_call(
        &func,
        &argvars[1],
        partial.as_ref(),
        selfdict.as_ref(),
        rettv,
    );
}

/// Port of `f_eval()` from `Src/eval/funcs.c:1233`.
///
/// "eval(string)" — evaluate `string` as a Vimscript expression.
///
/// RUST-PORT NOTE: `need_clr_eos`/`aborting()` editor state is not modelled; the
/// ported [`eval1`](crate::ported::eval::eval1) reference drives evaluation, with
/// `EVAL_STRING_HOOK` backing sub-expression evaluation as elsewhere.
pub fn f_eval(argvars: &[typval_T], rettv: &mut typval_T) {
    // c:1235 const char *s = tv_get_string_chk(&argvars[0]);
    let s0 = tv_get_string_chk(&argvars[0]);
    // c:1236 if (s != NULL) s = skipwhite(s);
    let expr = s0
        .as_deref()
        .map(|s| crate::ported::eval::skipwhite(s).to_string());

    // c:1240 if (s == NULL || eval1(&s, rettv, &EVALARG_EVALUATE) == FAIL)
    let mut ok = false;
    let mut trailing: Option<String> = None;
    if let Some(e) = &expr {
        let mut p: &str = e;
        let mut ea = crate::ported::eval::evalarg_T {
            eval_flags: crate::ported::eval::EVAL_EVALUATE,
        };
        if crate::ported::eval::eval1(&mut p, rettv, Some(&mut ea)) != FAIL {
            ok = true;
            // c:1248 else if (*s != NUL) semsg(_(e_trailing_arg), s);
            if !p.is_empty() {
                trailing = Some(p.to_string());
            }
        }
    }

    if !ok {
        // c:1241 semsg(_(e_invexpr2), expr_start); rettv = 0
        if let Some(e) = &expr {
            crate::ported::message::semsg(&format!("E15: Invalid expression: \"{e}\""));
        }
        rettv.v_type = VAR_NUMBER;
        rettv.vval = v_number(0);
    } else if let Some(t) = trailing {
        crate::ported::message::semsg(&format!("E488: Trailing characters: {t}"));
    }
}

/// Port of `common_function()` from `Src/eval/funcs.c:1654`.
///
/// The shared body of `function()` (`is_funcref == false`) and `funcref()`
/// (`is_funcref == true`): resolve a function name / partial into a `VAR_FUNC` or
/// a `VAR_PARTIAL` (when bound args, a `self` dict, or an existing partial are
/// involved).
///
/// RUST-PORT NOTE: the `nlua_is_table_from_lua` dance is omitted; `func_ref` /
/// `func_ptr_ref` are refcount no-ops; the resolved-`ufunc_T` `pt_func` field is
/// not modelled, so a funcref always binds by `pt_name`.
pub fn common_function(argvars: &[typval_T], rettv: &mut typval_T, is_funcref: bool) {
    use crate::ported::eval::typval_defs_h::{partial_T, VarLockStatus};
    use crate::ported::eval::userfunc::tfn::{TFN_INT, TFN_NO_AUTOLOAD, TFN_NO_DEREF, TFN_QUIET};

    let mut use_string = false; // c:1658
    let mut arg_pt: Option<std::rc::Rc<partial_T>> = None; // c:1659
    let mut trans_name: Option<String> = None; // c:1660

    // c:1662 resolve the source name / partial.
    let mut s: Option<String> = match (&argvars[0].v_type, &argvars[0].vval) {
        (VAR_FUNC, v_string(name)) => Some(name.clone()), // c:1664
        (VAR_PARTIAL, v_partial(Some(pt))) => {
            arg_pt = Some(pt.clone()); // c:1668
            Some(crate::ported::eval::partial_name(pt).to_string())
        }
        _ => {
            use_string = true; // c:1673
            Some(tv_get_string(&argvars[0]))
        }
    };

    // c:1676 if ((use_string && no AUTOLOAD_CHAR) || is_funcref) save_function_name(...).
    let has_autoload = s
        .as_deref()
        .is_some_and(|x| x.as_bytes().contains(&crate::ported::eval::AUTOLOAD_CHAR));
    // A script-local name (`s:F`, `<SID>F`) is resolved further down by
    // `get_scriptlocal_funcname`. It must not go through `save_function_name`
    // first: that calls `trans_function_name`, which in this port's single-script
    // model has no script id and so reports E81 for every `<SID>` name (c:2138
    // `current_sctx.sc_sid <= 0`). Skipping the pre-parse keeps `function('s:F')`
    // working while every other name still gets validated.
    let script_local = s
        .as_deref()
        .is_some_and(|x| x.starts_with("s:") || x.starts_with("<SID>"));
    if ((use_string && !has_autoload) || is_funcref) && !script_local {
        let orig = s.clone().unwrap_or_default();
        let mut name: &str = &orig;
        trans_name = crate::ported::eval::userfunc::save_function_name(
            &mut name,
            false,
            TFN_INT | TFN_QUIET | TFN_NO_AUTOLOAD | TFN_NO_DEREF,
            None,
        );
        // c:1681 if (*name != NUL) s = NULL;
        if !name.is_empty() {
            s = None;
        }
    }

    let s_empty = s.as_deref().is_none_or(str::is_empty);
    let s_digit = s
        .as_deref()
        .and_then(|x| x.bytes().next())
        .is_some_and(|b| b.is_ascii_digit());
    if !script_local && (s_empty || (use_string && s_digit) || (is_funcref && trans_name.is_none()))
    {
        // c:1685 semsg(_(e_invarg2), use_string ? tv_get_string(&argvars[0]) : s);
        let arg = if use_string {
            tv_get_string(&argvars[0])
        } else {
            s.clone().unwrap_or_default()
        };
        crate::ported::message::semsg(&format!("E475: Invalid argument: {arg}"));
        return;
    }

    // c:1688 else if (unknown function) semsg(E700).
    let name_exists = trans_name.as_deref().is_some_and(|tn| {
        if is_funcref {
            crate::ported::eval::userfunc::find_func(tn).is_some()
        } else {
            crate::ported::eval::userfunc::translated_function_exists(tn)
        }
    });
    if trans_name.is_some() && !name_exists {
        crate::ported::message::semsg(&format!(
            "E700: Unknown function: {}",
            s.clone().unwrap_or_default()
        ));
        return;
    }

    // c:1694 build the result.
    let sname = s.unwrap_or_default();
    // c:1697 expand s:/<SID> into <SNR>nr_.
    // c:1697 expand s:/<SID> into <SNR>nr_. This port has no script-id table, so
    // `get_scriptlocal_funcname` yields None and the *literal* name is kept —
    // which is how script-local functions are registered here, so `function('s:F')`
    // still resolves. (Defaulting to "" instead produced a Funcref to nothing.)
    let name: String = if sname.starts_with("s:") || sname.starts_with("<SID>") {
        crate::ported::eval::userfunc::get_scriptlocal_funcname(Some(&sname))
            .unwrap_or_else(|| sname.clone())
    } else {
        sname.clone()
    };

    // c:1707 figure out arg_idx / dict_idx.
    let mut dict_idx = 0usize;
    let mut arg_idx = 0usize;
    let mut list: Option<
        std::rc::Rc<std::cell::RefCell<crate::ported::eval::typval_defs_h::list_T>>,
    > = None;
    // c reads `argvars[1]`/`argvars[2]` freely because the C array is terminated
    // by a VAR_UNKNOWN entry; the Rust slice simply ends, so ask for the type
    // through `get` and treat "absent" as VAR_UNKNOWN.
    let argtype = |i: usize| argvars.get(i).map_or(VAR_UNKNOWN, |a| a.v_type);
    if argtype(1) != VAR_UNKNOWN {
        if argtype(2) != VAR_UNKNOWN {
            arg_idx = 1; // c:1710 function(name, [args], dict)
            dict_idx = 2;
        } else if argtype(1) == VAR_DICT {
            dict_idx = 1; // c:1713 function(name, dict)
        } else {
            arg_idx = 1; // c:1716 function(name, [args])
        }
        if dict_idx > 0 {
            // c:1719 if (tv_check_for_dict_arg(argvars, dict_idx) == FAIL) return;
            if tv_check_for_dict_arg(argvars, dict_idx) == FAIL {
                return;
            }
            if !matches!(&argvars[dict_idx].vval, v_dict(Some(_))) {
                dict_idx = 0; // c:1725 (NULL dict)
            }
        }
        if arg_idx > 0 {
            // c:1729 if (argvars[arg_idx].v_type != VAR_LIST) emsg(E923); return;
            match &argvars[arg_idx].vval {
                v_list(Some(l)) if argvars[arg_idx].v_type == VAR_LIST => {
                    let len = tv_list_len(&l.borrow());
                    if len == 0 {
                        arg_idx = 0; // c:1738 (empty arg list)
                    } else if len > MAX_FUNC_ARGS {
                        // c:1739 MAX_FUNC_ARGS → E118
                        crate::ported::eval::userfunc::emsg_funcname(
                            "E118: Too many arguments for function: %s",
                            &sname,
                        );
                        return;
                    } else {
                        list = Some(l.clone());
                    }
                }
                _ => {
                    crate::ported::message::emsg(
                        "E923: Second argument of function() must be a list or a dict",
                    );
                    return;
                }
            }
        }
    }

    // c:1748 build VAR_PARTIAL when there are bound args / dict / an arg_pt.
    let make_partial = dict_idx > 0 || arg_idx > 0 || arg_pt.is_some() || is_funcref;
    if make_partial {
        let mut pt = partial_T {
            pt_refcount: 1, // c:1782
            pt_name: String::new(),
            pt_argv: Vec::new(),
            pt_dict: None,
        };

        // c:1752 collect bound arguments: arg_pt's then the new list's.
        let arg_len = arg_pt.as_ref().map_or(0, |p| p.pt_argv.len());
        let lv_len = list
            .as_ref()
            .map_or(0, |l| tv_list_len(&l.borrow()) as usize);
        if arg_idx > 0 || arg_len > 0 {
            if let Some(ap) = &arg_pt {
                for i in 0..arg_len {
                    let mut dst = typval_T::default();
                    tv_copy(&ap.pt_argv[i], &mut dst); // c:1762
                    pt.pt_argv.push(dst);
                }
            }
            if lv_len > 0 {
                if let Some(l) = &list {
                    for it in &l.borrow().lv_items {
                        let mut dst = typval_T::default();
                        tv_copy(&it.li_tv, &mut dst); // c:1768
                        pt.pt_argv.push(dst);
                    }
                }
            }
        }

        // c:1774 bind the dict.
        if dict_idx > 0 {
            if let v_dict(Some(d)) = &argvars[dict_idx].vval {
                d.borrow_mut().dv_refcount += 1; // c:1777
                pt.pt_dict = Some(d.clone());
            }
        } else if let Some(ap) = &arg_pt {
            if let Some(d) = &ap.pt_dict {
                d.borrow_mut().dv_refcount += 1; // c:1784
                pt.pt_dict = Some(d.clone());
            }
        }

        // c:1789 bind the name (pt_func not modelled → always pt_name).
        pt.pt_name = match &arg_pt {
            Some(ap) if !ap.pt_name.is_empty() => ap.pt_name.clone(),
            _ => name,
        };
        crate::ported::eval::userfunc::func_ref();

        // c:1808 rettv->v_type = VAR_PARTIAL;
        rettv.v_type = VAR_PARTIAL;
        rettv.v_lock = VarLockStatus::VAR_UNLOCKED;
        rettv.vval = v_partial(Some(std::rc::Rc::new(pt)));
    } else {
        // c:1812 result is a VAR_FUNC.
        crate::ported::eval::userfunc::func_ref();
        rettv.v_type = VAR_FUNC;
        rettv.v_lock = VarLockStatus::VAR_UNLOCKED;
        rettv.vval = v_string(name);
    }
}

/// Port of `get_col()` from `Src/eval/funcs.c:712`.
///
/// Get the cursor/mark column (a byte index, or a character index when
/// `charcol`) for `col()`/`charcol()`.
///
/// RUST-PORT NOTE: `virtual_active()` (the 'virtualedit' state) is not modelled
/// and is treated as `false`, so the `coladd`/`win_chartabsize` fix-up branch is
/// unreachable but kept for fidelity via
/// [`plines::win_chartabsize`](crate::ported::plines::win_chartabsize).
pub fn get_col(argvars: &[typval_T], rettv: &mut typval_T, charcol: bool) {
    // c:714 argument type checks.
    if tv_check_for_string_or_list_arg(argvars, 0) == FAIL
        || tv_check_for_opt_number_arg(argvars, 1) == FAIL
    {
        return;
    }

    // c:719 win_T *wp = curwin;
    let mut wp = crate::ported::window::curwin.with(|c| c.borrow().clone());
    if argvars[1].v_type != VAR_UNKNOWN {
        // c:723 wp = win_id2wp_tp((int)tv_get_number(&argvars[1]), &tp);
        let mut tp = None;
        wp = crate::ported::eval::window::win_id2wp_tp(
            tv_get_number(&argvars[1]) as i32,
            Some(&mut tp),
        );
        if wp.is_none() || tp.is_none() {
            return; // c:725
        }
    }
    let wp = match wp {
        Some(w) => w,
        None => return,
    };

    // c:731 buf_T *bp = wp->w_buffer;
    let bp = match wp.borrow().w_buffer.clone() {
        Some(b) => b,
        None => return,
    };
    let mut col: crate::ported::window::colnr_T = 0; // c:732
    let mut fnum = bp.borrow().handle; // c:733 bp->b_fnum
                                       // c:734 pos_T *fp = var2fpos(&argvars[0], false, &fnum, charcol, wp);
    let fp = crate::ported::eval::var2fpos(&argvars[0], false, &mut fnum, charcol, &wp);
    if let Some(fp) = fp {
        if fnum == bp.borrow().handle {
            if fp.col == MAXCOL {
                // c:737 '> can be MAXCOL: use the line length.
                if fp.lnum <= bp.borrow().b_ml.ml_line_count {
                    col = crate::ported::buffer::ml_get_buf_len(&mut bp.borrow_mut(), fp.lnum) + 1;
                } else {
                    col = MAXCOL;
                }
            } else {
                col = fp.col + 1; // c:745
                                  // c:748 virtual_active() fix-up — not modelled (see note).
                if virtual_active() && wp.borrow().w_cursor == fp {
                    let line = crate::ported::buffer::ml_get_buf(
                        &mut bp.borrow_mut(),
                        wp.borrow().w_cursor.lnum,
                    );
                    let cur_col = wp.borrow().w_cursor.col;
                    let p = line.get(cur_col as usize..).unwrap_or("");
                    let _ = crate::ported::plines::win_chartabsize(&wp, p, 0);
                }
            }
        }
    }
    rettv.vval = v_number(col as varnumber_T); // c:762
}

/// Port of `virtual_active()` from `Src/misc2.c` — 'virtualedit' state.
///
/// RUST-PORT NOTE: 'virtualedit' is not modelled here → always inactive.
fn virtual_active() -> bool {
    false
}

/// Port of `set_position()` from `Src/eval/funcs.c:6442`.
///
/// Shared body of `setpos()` (`charpos == false`) and `setcharpos()` (`true`):
/// set the cursor (`.`) or a mark (`'x`) to the position List.
pub fn set_position(argvars: &[typval_T], rettv: &mut typval_T, charpos: bool) {
    let mut curswant: crate::ported::window::colnr_T = -1; // c:6444

    rettv.vval = v_number(-1); // c:6446
                               // c:6447 const char *name = tv_get_string_chk(argvars);
    let name = match tv_get_string_chk(&argvars[0]) {
        Some(n) => n,
        None => return, // c:6449
    };

    // c:6452 list2fpos(&argvars[1], &pos, &fnum, &curswant, charpos)
    let mut pos = crate::ported::window::pos_T::default();
    let mut fnum = 0i32;
    if crate::ported::eval::list2fpos(
        &argvars[1],
        &mut pos,
        Some(&mut fnum),
        Some(&mut curswant),
        charpos,
    ) != OK
    {
        return; // c:6455
    }

    // c:6458 if (pos.col != MAXCOL && --pos.col < 0) pos.col = 0;
    if pos.col != MAXCOL {
        pos.col -= 1;
        if pos.col < 0 {
            pos.col = 0;
        }
    }

    let nb = name.as_bytes();
    if nb == b"." {
        // c:6461 set cursor; "fnum" is ignored.
        if let Some(w) = crate::ported::window::curwin.with(|c| c.borrow().clone()) {
            w.borrow_mut().w_cursor = pos;
            if curswant >= 0 {
                // RUST-PORT NOTE: w_curswant/w_set_curswant not modelled.
            }
        }
        rettv.vval = v_number(0); // c:6469
    } else if nb.len() == 2 && nb[0] == b'\'' {
        // c:6470 set mark.
        if crate::ported::mark::setmark_pos(nb[1] as i32, &pos, fnum) == OK {
            rettv.vval = v_number(0); // c:6473
        }
    } else {
        // c:6476 emsg(_(e_invarg));
        crate::ported::message::emsg("E474: Invalid argument");
    }
}

/// Port of `get_search_arg()` from `Src/eval/funcs.c:5593`.
///
/// Parse the flags string of a `search*()` call, setting `'wrapscan'` (`p_ws`)
/// and OR-ing `SP_*` bits into `flagsp`. Returns the direction (`FORWARD` /
/// `BACKWARD`), or `0` on a bad flag.
pub fn get_search_arg(varp: &typval_T, flagsp: Option<&mut i32>) -> i32 {
    use crate::ported::search::{BACKWARD, FORWARD};
    let mut dir = FORWARD; // c:5595

    if varp.v_type == VAR_UNKNOWN {
        return FORWARD; // c:5598
    }
    // c:5602 const char *flags = tv_get_string_buf_chk(varp, nbuf);
    let flags = match tv_get_string_buf_chk(varp) {
        Some(f) => f,
        None => return 0, // c:5604 type error
    };

    let mut flagsp = flagsp;
    for &c in flags.as_bytes() {
        match c {
            b'b' => dir = BACKWARD, // c:5610
            b'w' => crate::ported::search::p_ws.with(|w| *w.borrow_mut() = true), // c:5612
            b'W' => crate::ported::search::p_ws.with(|w| *w.borrow_mut() = false), // c:5614
            _ => {
                let mut mask = 0; // c:5617
                if flagsp.is_some() {
                    mask = match c {
                        b'c' => SP_START,
                        b'e' => SP_END,
                        b'm' => SP_RETCOUNT,
                        b'n' => SP_NOMOVE,
                        b'p' => SP_SUBPAT,
                        b'r' => SP_REPEAT,
                        b's' => SP_SETPCMARK,
                        b'z' => SP_COLUMN,
                        _ => 0,
                    };
                }
                if mask == 0 {
                    // c:5637 semsg(_(e_invarg2), flags); dir = 0;
                    crate::ported::message::semsg(&format!("E475: Invalid argument: {flags}"));
                    dir = 0;
                } else if let Some(fp) = flagsp.as_deref_mut() {
                    *fp |= mask; // c:5641
                }
            }
        }
        if dir == 0 {
            break; // c:5645
        }
    }
    dir // c:5649
}

/// Port of `search_cmn()` from `Src/eval/funcs.c:5653`.
///
/// Shared by `search()` and `searchpos()`: run one search from the cursor,
/// updating `match_pos` and moving the cursor. Returns the match line (or the
/// sub-pattern number for `SP_SUBPAT`), or `0` on failure.
///
/// RUST-PORT NOTE: the profile time limit, `SEARCH_COL`, and `setpcmark`
/// side-effect are not modelled; matching goes through
/// [`search::searchit`](crate::ported::search::searchit).
pub fn search_cmn(
    argvars: &[typval_T],
    match_pos: Option<&mut crate::ported::window::pos_T>,
    flagsp: &mut i32,
) -> i32 {
    use crate::ported::search::{searchit, SEARCH_COL, SEARCH_END, SEARCH_KEEP, SEARCH_START};
    let save_p_ws = crate::ported::search::p_ws.with(|w| *w.borrow()); // c:5655
    let mut retval = 0; // c:5656
    let mut lnum_stop = 0; // c:5657
    let mut options = SEARCH_KEEP; // c:5659
    let mut use_skip = false; // c:5660

    // c:5662 const char *pat = tv_get_string(&argvars[0]);
    let pat = tv_get_string(&argvars[0]);
    // c:5663 int dir = get_search_arg(&argvars[1], flagsp);
    let dir = get_search_arg(&argvars[1], Some(flagsp));
    if dir == 0 {
        crate::ported::search::p_ws.with(|w| *w.borrow_mut() = save_p_ws);
        return retval; // c:5665 goto theend
    }
    let flags = *flagsp;
    if flags & SP_START != 0 {
        options |= SEARCH_START; // c:5669
    }
    if flags & SP_END != 0 {
        options |= SEARCH_END; // c:5672
    }
    if flags & SP_COLUMN != 0 {
        options |= SEARCH_COL; // c:5675
    }

    // c:5678 optional stop line / timeout / skip.
    if argvars[1].v_type != VAR_UNKNOWN && argvars[2].v_type != VAR_UNKNOWN {
        lnum_stop = tv_get_number_chk(&argvars[2], None) as crate::ported::window::linenr_T;
        if lnum_stop < 0 {
            crate::ported::search::p_ws.with(|w| *w.borrow_mut() = save_p_ws);
            return retval; // c:5683
        }
        if argvars[3].v_type != VAR_UNKNOWN {
            let time_limit = tv_get_number_chk(&argvars[3], None);
            if time_limit < 0 {
                crate::ported::search::p_ws.with(|w| *w.borrow_mut() = save_p_ws);
                return retval; // c:5689
            }
            use_skip = crate::ported::eval::eval_expr_valid_arg(&argvars[4]); // c:5691
        }
    }

    // c:5701 reject SP_REPEAT|SP_RETCOUNT and NOMOVE+SETPCMARK together.
    if (flags & (SP_REPEAT | SP_RETCOUNT)) != 0
        || ((flags & SP_NOMOVE != 0) && (flags & SP_SETPCMARK != 0))
    {
        crate::ported::message::semsg(&format!(
            "E475: Invalid argument: {}",
            tv_get_string(&argvars[1])
        ));
        crate::ported::search::p_ws.with(|w| *w.borrow_mut() = save_p_ws);
        return retval; // c:5706
    }

    // c:5710 save_cursor = pos = curwin->w_cursor;
    let curwin_rc = crate::ported::window::curwin.with(|c| c.borrow().clone());
    let save_cursor = curwin_rc
        .as_ref()
        .map_or(crate::ported::window::pos_T::default(), |w| {
            w.borrow().w_cursor
        });
    let mut pos = save_cursor;
    let mut firstpos = crate::ported::window::pos_T::default(); // c:5712
    let mut subpatnum;

    // c:5721 repeat until {skip} returns false.
    loop {
        subpatnum = searchit(&mut pos, dir, &pat, options, lnum_stop);
        // c:5726 finding the first match again means no match where {skip}==0.
        if firstpos.lnum != 0 && pos == firstpos {
            subpatnum = FAIL;
        }
        if subpatnum == FAIL || !use_skip {
            break; // c:5729
        }
        if firstpos.lnum == 0 {
            firstpos = pos; // c:5733
        }
        // c:5737 if the skip expression matches, ignore this match.
        if let Some(w) = &curwin_rc {
            let save_pos = w.borrow().w_cursor;
            w.borrow_mut().w_cursor = pos;
            let do_skip = crate::ported::eval::eval_expr_to_bool(&argvars[4]);
            w.borrow_mut().w_cursor = save_pos;
            if !do_skip {
                break; // c:5748
            }
        } else {
            break;
        }
        options &= !SEARCH_START; // c:5753
    }

    if subpatnum != FAIL {
        if flags & SP_SUBPAT != 0 {
            retval = subpatnum; // c:5758
        } else {
            retval = pos.lnum; // c:5761
        }
        // c:5763 SP_SETPCMARK → setpcmark() (not modelled).
        if let Some(w) = &curwin_rc {
            w.borrow_mut().w_cursor = pos; // c:5766
        }
        if let Some(mp) = match_pos {
            mp.lnum = pos.lnum; // c:5769
            mp.col = pos.col + 1;
        }
    }

    // c:5777 SP_NOMOVE → restore cursor.
    if flags & SP_NOMOVE != 0 {
        if let Some(w) = &curwin_rc {
            w.borrow_mut().w_cursor = save_cursor;
        }
    }
    crate::ported::search::p_ws.with(|w| *w.borrow_mut() = save_p_ws); // c:5783
    retval
}

/// Port of `searchpair_cmn()` from `Src/eval/funcs.c:6064`.
///
/// Shared by `searchpair()` and `searchpairpos()`: parse the start/middle/end
/// patterns and flags then delegate to
/// [`do_searchpair`](crate::ported::search::do_searchpair). Returns the match
/// line (via `do_searchpair`) or `0`.
pub fn searchpair_cmn(
    argvars: &[typval_T],
    match_pos: Option<&mut crate::ported::window::pos_T>,
) -> i32 {
    use crate::ported::search::do_searchpair;
    let save_p_ws = crate::ported::search::p_ws.with(|w| *w.borrow()); // c:6066
    let mut flags = 0; // c:6067
    let mut retval = 0; // c:6068
    let mut lnum_stop = 0; // c:6069
    let mut time_limit = 0i64; // c:6070

    // c:6074 the three patterns.
    let spat = tv_get_string_chk(&argvars[0]);
    let mpat = tv_get_string_buf_chk(&argvars[1]);
    let epat = tv_get_string_buf_chk(&argvars[2]);
    let (spat, mpat, epat) = match (spat, mpat, epat) {
        (Some(s), Some(m), Some(e)) => (s, m, e),
        _ => {
            crate::ported::search::p_ws.with(|w| *w.borrow_mut() = save_p_ws);
            return retval; // c:6080 type error
        }
    };

    // c:6083 int dir = get_search_arg(&argvars[3], &flags);
    let dir = get_search_arg(&argvars[3], Some(&mut flags));
    if dir == 0 {
        crate::ported::search::p_ws.with(|w| *w.borrow_mut() = save_p_ws);
        return retval; // c:6085
    }

    // c:6089 don't accept SP_END or SP_SUBPAT; NOMOVE/SETPCMARK mutually exclusive.
    if (flags & (SP_END | SP_SUBPAT)) != 0
        || ((flags & SP_NOMOVE != 0) && (flags & SP_SETPCMARK != 0))
    {
        crate::ported::message::semsg(&format!(
            "E475: Invalid argument: {}",
            tv_get_string(&argvars[3])
        ));
        crate::ported::search::p_ws.with(|w| *w.borrow_mut() = save_p_ws);
        return retval; // c:6093
    }

    // c:6096 'r' implies 'W'.
    if flags & SP_REPEAT != 0 {
        crate::ported::search::p_ws.with(|w| *w.borrow_mut() = false);
    }

    // c:6101 optional skip expression, stop line, timeout.
    let mut skip: Option<&typval_T> = None;
    if !(argvars[3].v_type == VAR_UNKNOWN || argvars[4].v_type == VAR_UNKNOWN) {
        skip = Some(&argvars[4]); // c:6108
        if argvars[5].v_type != VAR_UNKNOWN {
            lnum_stop = tv_get_number_chk(&argvars[5], None) as crate::ported::window::linenr_T;
            if lnum_stop < 0 {
                crate::ported::message::semsg(&format!(
                    "E475: Invalid argument: {}",
                    tv_get_string(&argvars[5])
                ));
                crate::ported::search::p_ws.with(|w| *w.borrow_mut() = save_p_ws);
                return retval; // c:6113
            }
            if argvars[6].v_type != VAR_UNKNOWN {
                time_limit = tv_get_number_chk(&argvars[6], None);
                if time_limit < 0 {
                    crate::ported::message::semsg(&format!(
                        "E475: Invalid argument: {}",
                        tv_get_string(&argvars[6])
                    ));
                    crate::ported::search::p_ws.with(|w| *w.borrow_mut() = save_p_ws);
                    return retval; // c:6120
                }
            }
        }
    }

    // c:6127 do_searchpair(...).
    retval = do_searchpair(
        &spat, &mpat, &epat, dir, skip, flags, match_pos, lnum_stop, time_limit,
    );
    crate::ported::search::p_ws.with(|w| *w.borrow_mut() = save_p_ws); // c:6131
    retval
}

/// Port of `libcall_common()` from `Src/eval/funcs.c:3940`.
///
/// Shared body of `libcall()` (`out_type == VAR_STRING`) and `libcallnr()`
/// (`out_type == VAR_NUMBER`): load a native library and call a function taking a
/// string/int and returning a string/int (via
/// [`os_libcall`](crate::ported::os::dl::os_libcall)).
///
/// RUST-PORT NOTE (signature): `out_type` is passed as the [`VarType`] enum.
/// `check_secure()` (the `'secure'`/sandbox gate) is not modelled and is elided.
pub fn libcall_common(
    argvars: &[typval_T],
    rettv: &mut typval_T,
    out_type: crate::ported::eval::typval_defs_h::VarType,
) {
    // c:3942 rettv->v_type = out_type; if (out_type != VAR_NUMBER) rettv->vval.v_string = NULL;
    rettv.v_type = out_type;
    if out_type != VAR_NUMBER {
        rettv.vval = v_string(String::new());
    }

    // c:3951 both libname and funcname must be strings.
    let (libname, funcname) = match (&argvars[0], &argvars[1]) {
        (a, b) if a.v_type == VAR_STRING && b.v_type == VAR_STRING => {
            (tv_get_string(a), tv_get_string(b))
        }
        _ => return, // c:3952
    };

    // c:3958 input variables.
    let in_type = argvars[2].v_type;
    let str_in = if in_type == VAR_STRING {
        Some(tv_get_string(&argvars[2]))
    } else {
        None
    };
    let int_in = tv_get_number(&argvars[2]) as i32; // c:3963

    // c:3969 os_libcall(...).
    let mut int_out = 0i32;
    let mut str_out: Option<String> = None;
    let success = if out_type == VAR_STRING {
        crate::ported::os::dl::os_libcall(
            Some(&libname),
            Some(&funcname),
            str_in.as_deref(),
            int_in,
            Some(&mut str_out),
            &mut int_out,
        )
    } else {
        crate::ported::os::dl::os_libcall(
            Some(&libname),
            Some(&funcname),
            str_in.as_deref(),
            int_in,
            None,
            &mut int_out,
        )
    };

    if !success {
        // c:3973 semsg(_(e_libcall), funcname);
        crate::ported::message::semsg(&format!("E364: Library call failed for \"{funcname}\""));
        return;
    }

    if out_type == VAR_NUMBER {
        rettv.vval = v_number(int_out as varnumber_T); // c:3978
    } else {
        rettv.vval = v_string(str_out.unwrap_or_default());
    }
}

/// Port of `create_environment()` from `Src/eval/funcs.c:3382`.
///
/// Build the child-process environment Dict from `job_env` (a user dict), the
/// inherited process environment (unless `clear_env`), and pty/`$NVIM` fix-ups.
///
/// RUST-PORT NOTE: `uv_os_environ`/`os_getenv` are replaced by `std::env`; the
/// Windows uppercase-dedup, `p_tgc` COLORTERM, and the `pty_ignored_env_vars` /
/// `required_env_vars` tables are modelled with the documented default sets.
pub fn create_environment(
    job_env: Option<&crate::ported::eval::typval_defs_h::dict_T>,
    clear_env: bool,
    pty: bool,
    set_nvim_addr: bool,
    pty_term_name: &str,
) -> std::rc::Rc<std::cell::RefCell<crate::ported::eval::typval_defs_h::dict_T>> {
    // c:3385 dict_T *env = tv_dict_alloc();
    let env = crate::ported::eval::typval::tv_dict_alloc();

    if !clear_env {
        // c:3388 uv_os_environ → std::env::vars().
        for (name, value) in std::env::vars() {
            tv_dict_add_str(&mut env.borrow_mut(), &name, &value); // c:3400
        }

        if pty {
            // c:3406 remove pty-ignored env vars.
            for var in PTY_IGNORED_ENV_VARS {
                tv_dict_item_remove(&mut env.borrow_mut(), var);
            }
            // c:3417 p_tgc → COLORTERM=truecolor (not modelled → skipped).
        }
    }

    if pty {
        // c:3427 set a sane $TERM.
        tv_dict_item_remove(&mut env.borrow_mut(), "TERM");
        tv_dict_add_str(&mut env.borrow_mut(), "TERM", pty_term_name);
    }

    if set_nvim_addr {
        // c:3437 $NVIM = v:servername.
        let nvim_addr = get_vim_var_str(crate::ported::eval::vars::vv::VV_SEND_SERVER);
        if !nvim_addr.is_empty() {
            tv_dict_item_remove(&mut env.borrow_mut(), "NVIM");
            tv_dict_add_str(&mut env.borrow_mut(), "NVIM", &nvim_addr);
        }
    }

    if let Some(je) = job_env {
        // c:3466 tv_dict_extend(env, job_env->di_tv.vval.v_dict, "force");
        tv_dict_extend(&mut env.borrow_mut(), je, "force");
    }

    if pty {
        // c:3470 ensure required env vars are present.
        for var in REQUIRED_ENV_VARS {
            let present = env.borrow().dv_hashtab.contains_key(*var);
            if !present {
                if let Ok(val) = std::env::var(var) {
                    tv_dict_add_allocated_str(&mut env.borrow_mut(), var, val);
                }
            }
        }
    }

    env // c:3485
}

/// c:funcs.c:3372 `pty_ignored_env_vars[]` — env vars stripped for a pty child.
const PTY_IGNORED_ENV_VARS: &[&str] = &["COLUMNS", "LINES", "TERMCAP", "COLORFGBG", "COLORTERM"];
/// c:funcs.c:3378 `required_env_vars[]` — env vars a pty child must have.
const REQUIRED_ENV_VARS: &[&str] = &["HOME"];

/// Port of `msgpackparse_unpack_blob()` from `Src/eval/funcs.c:4750`.
///
/// Decode a Blob of msgpack bytes into `ret_list` (one value per element).
pub fn msgpackparse_unpack_blob(
    blob: &crate::ported::eval::typval_defs_h::blob_T,
    ret_list: &mut crate::ported::eval::typval_defs_h::list_T,
) {
    // c:4753 const int len = tv_blob_len(blob); if (len == 0) return;
    let data_all = &blob.bv_ga;
    if data_all.is_empty() {
        return;
    }
    // c:4759 const char *data = blob->bv_ga.ga_data; size_t remaining = len;
    let mut data: &[u8] = data_all;
    let mut remaining = data_all.len();
    while remaining != 0 {
        let mut tv = typval_T::default();
        // c:4763 int status = unpack_typval(&data, &remaining, &tv);
        let status = crate::ported::eval::decode::unpack_typval(&mut data, &mut remaining, &mut tv);
        if status != crate::ported::mpack::MPACK_OK {
            // c:4765 emsg_mpack_error(status);
            crate::ported::message::emsg("E5071: Failed to parse msgpack");
            return;
        }
        // c:4769 tv_list_append_owned_tv(ret_list, tv);
        tv_list_append_owned_tv(ret_list, tv);
    }
}

/// Port of `msgpackparse_unpack_list()` from `Src/eval/funcs.c:4686`.
///
/// Decode a List of msgpack byte-string "lines" into `ret_list`.
///
/// RUST-PORT NOTE: Neovim streams the joined list bytes through a persistent
/// `mpack_parser_t` in `ARENA_BLOCK_SIZE` chunks. This reference reconstructs the
/// full joined byte stream via [`encode_read_from_list`] (the same primitive the
/// C uses to read the list) and then decodes value-by-value with
/// [`unpack_typval`], the same primitive the Blob path uses.
pub fn msgpackparse_unpack_list(
    list: &crate::ported::eval::typval_defs_h::list_T,
    ret_list: &mut crate::ported::eval::typval_defs_h::list_T,
) {
    use crate::ported::eval::encode::{encode_init_lrstate, encode_read_from_list};
    // c:4689 if (tv_list_len(list) == 0) return;
    if tv_list_len(list) == 0 {
        return;
    }
    // c:4692 first item must be a string.
    let first_is_string = list
        .lv_items
        .first()
        .is_some_and(|it| it.li_tv.v_type == VAR_STRING);
    if !first_is_string {
        crate::ported::message::semsg("E475: Invalid argument: List item is not a string");
        return;
    }

    // c:4697 read the whole joined byte stream from the list.
    const ARENA_BLOCK_SIZE: usize = 4096; // c: memory_defs.h
    let mut lrstate = encode_init_lrstate(list);
    let mut bytes: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; ARENA_BLOCK_SIZE];
    loop {
        let (status, read_bytes) = encode_read_from_list(&mut lrstate, list, &mut chunk);
        bytes.extend_from_slice(&chunk[..read_bytes]);
        if status == FAIL {
            crate::ported::message::semsg("E475: Invalid argument: List item is not a string");
            return;
        }
        if status == OK {
            break; // finished
        }
        // status == NOTDONE (2): keep reading.
    }

    // Decode value-by-value.
    let mut data: &[u8] = &bytes;
    let mut remaining = bytes.len();
    while remaining != 0 {
        let mut tv = typval_T::default();
        let status = crate::ported::eval::decode::unpack_typval(&mut data, &mut remaining, &mut tv);
        if status != crate::ported::mpack::MPACK_OK {
            crate::ported::message::emsg("E5071: Failed to parse msgpack");
            return;
        }
        tv_list_append_owned_tv(ret_list, tv);
    }
}

/// Port of `block_def2str()` from `Src/eval/funcs.c:2164`.
///
/// Render a `block_def` (start-padding spaces + the text + end-padding spaces) to
/// a String, for the `getregion()`/`getregionpos()` blockwise/charwise paths.
pub fn block_def2str(bd: &block_def) -> String {
    // c:2166 size = startspaces + endspaces + textlen;
    let mut ret = String::new();
    // c:2169 memset(' ', startspaces)
    for _ in 0..bd.startspaces.max(0) {
        ret.push(' ');
    }
    // c:2172 memmove(textstart, textlen)
    let tl = bd.textlen.max(0) as usize;
    if tl > 0 {
        let end = (bd.textstart_off as usize + tl).min(bd.line.len());
        let start = (bd.textstart_off as usize).min(end);
        ret.push_str(&bd.line[start..end]);
    }
    // c:2175 memset(' ', endspaces)
    for _ in 0..bd.endspaces.max(0) {
        ret.push(' ');
    }
    ret
}

/// Port of `struct block_def` (`Src/ops.c`) — the fields the region/block
/// builtins read.
///
/// RUST-PORT NOTE: the C `char *textstart` pointer into the buffer line becomes a
/// byte offset (`textstart_off`) into the owned `line` string.
#[derive(Default)]
pub struct block_def {
    pub startspaces: crate::ported::window::colnr_T,
    pub endspaces: crate::ported::window::colnr_T,
    pub textlen: crate::ported::window::colnr_T,
    pub textcol: crate::ported::window::colnr_T,
    pub textstart_off: crate::ported::window::colnr_T,
    pub is_oneChar: bool,
    pub start_vcol: crate::ported::window::colnr_T,
    pub end_vcol: crate::ported::window::colnr_T,
    pub start_char_vcols: crate::ported::window::colnr_T,
    /// The owning line text (`textstart` points inside this).
    pub line: String,
}

/// Port of `add_regionpos_range()` from `Src/eval/funcs.c:2355`.
///
/// Append one `[[fnum,lnum,col,coladd], [fnum,lnum,col,coladd]]` region range to
/// the `getregionpos()` result list.
pub fn add_regionpos_range(
    rettv: &mut typval_T,
    p1: crate::ported::window::pos_T,
    p2: crate::ported::window::pos_T,
) {
    // c:2357 fnum = curbuf->b_fnum.
    let fnum = crate::ported::buffer::curbuf
        .with(|c| c.borrow().clone())
        .map_or(0, |b| b.borrow().handle) as varnumber_T;

    let l1 = crate::ported::eval::typval::tv_list_alloc(2); // c:2357
    let l2 = crate::ported::eval::typval::tv_list_alloc(4); // c:2360
    let l3 = crate::ported::eval::typval::tv_list_alloc(4); // c:2363

    // c:2366 l2 = [fnum, p1.lnum, p1.col, p1.coladd]
    {
        let mut b = l2.borrow_mut();
        tv_list_append_number(&mut b, fnum);
        tv_list_append_number(&mut b, p1.lnum as varnumber_T);
        tv_list_append_number(&mut b, p1.col as varnumber_T);
        tv_list_append_number(&mut b, p1.coladd as varnumber_T);
    }
    // c:2371 l3 = [fnum, p2.lnum, p2.col, p2.coladd]
    {
        let mut b = l3.borrow_mut();
        tv_list_append_number(&mut b, fnum);
        tv_list_append_number(&mut b, p2.lnum as varnumber_T);
        tv_list_append_number(&mut b, p2.col as varnumber_T);
        tv_list_append_number(&mut b, p2.coladd as varnumber_T);
    }
    // c:2358 l1 = [l2, l3]; append l1 to rettv.
    tv_list_append_list(&mut l1.borrow_mut(), l2);
    tv_list_append_list(&mut l1.borrow_mut(), l3);
    if let v_list(Some(rl)) = &rettv.vval {
        tv_list_append_list(&mut rl.borrow_mut(), l1);
    }
}

/// Port of `getregionpos()` from `Src/eval/funcs.c:2180`.
///
/// Validate the `[lnum,col]` list arguments and `{type}` dict of
/// `getregion()`/`getregionpos()`, computing the sorted region endpoints
/// (`p1`/`p2`), `inclusive`, and the [`MotionType`] into `region_type`. Returns
/// [`OK`](crate::ported::eval_h::OK)/[`FAIL`](crate::ported::eval_h::FAIL).
///
/// RUST-PORT NOTE: charwise / linewise are faithful; the blockwise
/// `getvvcol`/`oparg`/`reset_lbr`/`virtual_op` machinery is simplified — the
/// `oap` out-param records only `start_vcol`/`end_vcol` via
/// [`plines::getvcol`](crate::ported::plines::getvcol). The exclusive-selection
/// `unadjust_for_sel_inner` back-up is approximated. `region_type` is
/// `Option<MotionType>` where `None` == `kMTUnknown`.
#[allow(clippy::too_many_arguments)]
pub fn getregionpos(
    argvars: &[typval_T],
    rettv: &mut typval_T,
    p1: &mut crate::ported::window::pos_T,
    p2: &mut crate::ported::window::pos_T,
    inclusive: &mut bool,
    region_type: &mut Option<MotionType>,
    oap: &mut oparg_T,
) -> i32 {
    // c:2182 tv_list_alloc_ret(rettv, kListLenMayKnow);
    tv_list_alloc_ret(rettv, 0);

    // c:2184 argument checks.
    if tv_check_for_list_arg(argvars, 0) == FAIL
        || tv_check_for_list_arg(argvars, 1) == FAIL
        || tv_check_for_opt_dict_arg(argvars, 2) == FAIL
    {
        return FAIL;
    }

    // c:2192 list2fpos both endpoints; buffers must match.
    let mut fnum1 = -1;
    let mut fnum2 = -1;
    if crate::ported::eval::list2fpos(&argvars[0], p1, Some(&mut fnum1), None, false) != OK
        || crate::ported::eval::list2fpos(&argvars[1], p2, Some(&mut fnum2), None, false) != OK
        || fnum1 != fnum2
    {
        return FAIL; // c:2197
    }

    // c:2200 selection exclusivity + type. `*p_sel == 'e'` → 'selection' is
    // "exclusive".
    let sel_excl = tv_get_string(&crate::ported::option::get_option_value("selection"))
        .as_bytes()
        .first()
        == Some(&b'e');
    let mut is_select_exclusive = sel_excl;
    let mut type_str = String::from("v"); // default_type
    if argvars[2].v_type == VAR_DICT {
        if let v_dict(Some(d)) = &argvars[2].vval {
            is_select_exclusive =
                tv_dict_get_bool(&d.borrow(), "exclusive", sel_excl as varnumber_T) != 0;
            if let Some(t) = tv_dict_get_string(&d.borrow(), "type") {
                type_str = t;
            }
        }
    }

    // c:2214 parse the type into region_type + block_width.
    let tb = type_str.as_bytes();
    let mut block_width = 0i32;
    if tb == b"v" {
        *region_type = Some(MotionType::CharWise); // c:2216
    } else if tb == b"V" {
        *region_type = Some(MotionType::LineWise); // c:2218
    } else if tb.first() == Some(&0x16) {
        // Ctrl-V — blockwise.
        let rest = &type_str[1..];
        if !rest.is_empty() {
            match rest.parse::<i32>() {
                Ok(w) if w > 0 => block_width = w,
                _ => {
                    crate::ported::message::semsg(&format!(
                        "E475: Invalid value for argument type: {type_str}"
                    ));
                    return FAIL;
                }
            }
        }
        *region_type = Some(MotionType::BlockWise(block_width)); // c:2226
    } else {
        crate::ported::message::semsg(&format!(
            "E475: Invalid value for argument type: {type_str}"
        ));
        return FAIL; // c:2229
    }

    // c:2232 buffer lookup.
    let findbuf = if fnum1 != 0 {
        crate::ported::buffer::buflist_findnr(fnum1)
    } else {
        crate::ported::buffer::curbuf.with(|c| c.borrow().clone())
    };
    let findbuf = match findbuf {
        Some(b) if b.borrow().b_ml.ml_mfp => b,
        _ => {
            crate::ported::message::emsg("E681: Buffer is not loaded");
            return FAIL; // c:2235
        }
    };

    // c:2239 line/column validation, inlined for p1 (c:2244) then p2 (c:2254),
    // exactly as the C does.
    let line_count = findbuf.borrow().b_ml.ml_line_count;
    // p1:
    if p1.lnum < 1 || p1.lnum > line_count {
        crate::ported::message::semsg(&format!("E966: Invalid line number: {}", p1.lnum));
        return FAIL;
    }
    let l1len = crate::ported::buffer::ml_get_buf_len(&mut findbuf.borrow_mut(), p1.lnum);
    if p1.col == MAXCOL {
        p1.col = l1len + 1;
    } else if p1.col < 1 || p1.col > l1len + 1 {
        crate::ported::message::semsg(&format!("E964: Invalid column number: {}", p1.col));
        return FAIL;
    }
    // p2:
    if p2.lnum < 1 || p2.lnum > line_count {
        crate::ported::message::semsg(&format!("E966: Invalid line number: {}", p2.lnum));
        return FAIL;
    }
    let l2len = crate::ported::buffer::ml_get_buf_len(&mut findbuf.borrow_mut(), p2.lnum);
    if p2.col == MAXCOL {
        p2.col = l2len + 1;
    } else if p2.col < 1 || p2.col > l2len + 1 {
        crate::ported::message::semsg(&format!("E964: Invalid column number: {}", p2.col));
        return FAIL;
    }

    // c:2266 curbuf = findbuf; (virtual_op not modelled).
    crate::ported::buffer::curbuf.with(|c| *c.borrow_mut() = Some(findbuf.clone()));

    // c:2271 adjustment: 0-based columns.
    p1.col -= 1;
    p2.col -= 1;

    // c:2274 swap so p1 <= p2.
    if !lt(*p1, *p2) {
        std::mem::swap(p1, p2);
    }

    match region_type {
        Some(MotionType::CharWise) => {
            // c:2281 exclusive selection back-up.
            if is_select_exclusive && *p1 != *p2 {
                // RUST-PORT NOTE: unadjust_for_sel_inner is approximated by
                // backing p2 up one column, clamping at column 0.
                if p2.col > 0 {
                    p2.col -= 1;
                    *inclusive = true;
                } else {
                    *inclusive = false;
                }
            }
            // c:2288 if inclusive and p2 on NUL → not inclusive.
            let line = crate::ported::buffer::ml_get_buf(&mut findbuf.borrow_mut(), p2.lnum);
            if *inclusive && line.as_bytes().get(p2.col as usize).is_none() {
                *inclusive = false;
            }
        }
        Some(MotionType::BlockWise(bw)) => {
            // c:2292 blockwise vcol computation (simplified).
            let wp = crate::ported::window::curwin
                .with(|c| c.borrow().clone())
                .expect("curwin");
            let l1 = crate::ported::buffer::ml_get_buf(&mut findbuf.borrow_mut(), p1.lnum);
            let l2 = crate::ported::buffer::ml_get_buf(&mut findbuf.borrow_mut(), p2.lnum);
            let sc1 = crate::ported::plines::getvcol(&wp, &l1, p1.col);
            let sc2 = crate::ported::plines::getvcol(&wp, &l2, p2.col);
            let ec1 = sc1;
            let ec2 = sc2;
            oap.motion_type = Some(MotionType::BlockWise(*bw));
            oap.inclusive = true;
            oap.op_type = 0; // OP_NOP
            oap.start = *p1;
            oap.end = *p2;
            oap.start_vcol = sc1.min(sc2);
            oap.end_vcol = if *bw > 0 {
                oap.start_vcol + *bw - 1
            } else {
                ec1.max(ec2)
            };
        }
        _ => {}
    }

    OK // c:2318
}

/// Port of `oparg_T` (`Src/normal_defs.h`) — the operator-argument fields the
/// region builtins read. RUST-PORT NOTE: only the blockwise vcol fields are
/// modelled here (see [`getregionpos`]).
#[derive(Default)]
pub struct oparg_T {
    pub motion_type: Option<MotionType>,
    pub inclusive: bool,
    pub op_type: i32,
    pub start: crate::ported::window::pos_T,
    pub end: crate::ported::window::pos_T,
    pub start_vcol: crate::ported::window::colnr_T,
    pub end_vcol: crate::ported::window::colnr_T,
}

/// Port of `lt()` from `Src/pos_defs.h` — position ordering (`a < b`).
fn lt(a: crate::ported::window::pos_T, b: crate::ported::window::pos_T) -> bool {
    if a.lnum != b.lnum {
        a.lnum < b.lnum
    } else if a.col != b.col {
        a.col < b.col
    } else {
        a.coladd < b.coladd
    }
}

/// "Run one Ex command line → captured message text" hook, installed by the
/// bridge/tests. Backs `execute_common()`, which in C runs the command via
/// `do_cmdline_cmd`/`do_cmdline` (ex_docmd.c) and reads the redirected output
/// out of `capture_ga`.
///
/// RUST-PORT NOTE: `do_cmdline` is not ported (the runtime path is the fusevm
/// bridge `b_execute`), and the message-redir plumbing that feeds `capture_ga`
/// during a command run has no standalone analog. This hook stands in for both:
/// the installer runs the command line and returns the text that `:redir` would
/// have captured. Unset → the capture is empty (analogous to `EVAL_STRING_HOOK`).
type DoCmdlineFn = fn(&str) -> String;

thread_local! {
    /// C global `int msg_silent` (`message.c`) — nesting count of `:silent`.
    /// RUST-PORT NOTE: a per-thread `Cell` for the C global (single-threaded eval).
    static msg_silent: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };

    /// C global `int msg_col` (`message.c`) — current message-area column.
    static msg_col: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };

    /// C global `bool emsg_noredir` (`message.c`) — errors bypass `:redir`.
    static emsg_noredir: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };

    /// C global `bool redir_off` (`message.c`) — message redirection paused.
    static redir_off: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };

    /// C global `garray_T *capture_ga` (`message.c`) — the active `execute()`
    /// capture buffer (NULL when not capturing).
    ///
    /// RUST-PORT NOTE: the `garray_T` byte buffer is modelled as a `String`;
    /// `None` is the C `NULL` (not capturing).
    static capture_ga: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };

    /// Bridge/test-installed command runner backing `execute_common()`.
    pub static DO_CMDLINE_HOOK: std::cell::RefCell<Option<DoCmdlineFn>> =
        const { std::cell::RefCell::new(None) };
}

/// Port of `execute_common()` from `Src/eval/funcs.c:1278`.
///
/// Shared body of `execute()` (and, in C, `win_execute()`): run a command (or a
/// list of command lines) with message output redirected into `capture_ga`, then
/// return the captured text as a String in `rettv`.
pub fn execute_common(argvars: &[typval_T], rettv: &mut typval_T, arg_off: usize) {
    // Local closure standing in for `do_cmdline_cmd`/`do_cmdline` (ex_docmd.c,
    // not ported): run the command line through the bridge-installed
    // `DO_CMDLINE_HOOK` and return the text `:redir` would have captured. Unset
    // → empty capture. Kept a closure (not a named fn) as it has no C name.
    let do_cmdline_via_hook = |cmd: &str| -> String {
        match DO_CMDLINE_HOOK.with(|h| *h.borrow()) {
            Some(run) => run(cmd),
            None => String::new(),
        }
    };
    let save_msg_silent = msg_silent.with(|v| v.get()); // c:1280
    let save_emsg_silent = emsg_silent.with(|v| v.get()); // c:1281
    let save_emsg_noredir = emsg_noredir.with(|v| v.get()); // c:1282
    let save_redir_off = redir_off.with(|v| v.get()); // c:1283
    let save_capture_ga = capture_ga.with(|v| v.borrow().clone()); // c:1284
    let save_msg_col = msg_col.with(|v| v.get()); // c:1285
    let mut echo_output = false; // c:1286

    // c:1288 check_secure() — 'secure'/sandbox not modelled (always allowed).

    if argvars[arg_off + 1].v_type != VAR_UNKNOWN {
        // c:1293 tv_get_string_buf_chk(&argvars[arg_off + 1], buf)
        let s = match tv_get_string_buf_chk(&argvars[arg_off + 1]) {
            Some(s) => s,
            // c:1295 if (s == NULL) return;
            None => return,
        };
        if s.is_empty() {
            // c:1299 if (*s == NUL) echo_output = true;
            echo_output = true;
        }
        if s.as_bytes().starts_with(b"silent") {
            // c:1301 strncmp(s, "silent", 6) == 0
            msg_silent.with(|v| v.set(v.get() + 1));
        }
        if s == "silent!" {
            // c:1304 strcmp(s, "silent!") == 0
            emsg_silent.with(|v| v.set(1));
            emsg_noredir.with(|v| v.set(true));
        }
    } else {
        // c:1308 msg_silent++;
        msg_silent.with(|v| v.set(v.get() + 1));
    }

    // c:1312 ga_init(&capture_local, ...); capture_ga = &capture_local;
    capture_ga.with(|v| *v.borrow_mut() = Some(String::new()));
    redir_off.with(|v| v.set(false)); // c:1313
    if !echo_output {
        // c:1315 msg_col = 0;  // prevent leading spaces
        msg_col.with(|v| v.set(0));
    }

    let output = if argvars[arg_off].v_type != VAR_LIST {
        // c:1319 do_cmdline_cmd(tv_get_string(&argvars[arg_off]));
        do_cmdline_via_hook(&tv_get_string(&argvars[arg_off]))
    } else {
        match &argvars[arg_off].vval {
            // c:1320 else if (argvars[arg_off].vval.v_list != NULL)
            v_list(Some(l)) => {
                // c:1322 tv_list_ref(list);
                tv_list_ref(&mut l.borrow_mut());
                // c:1323 GetListLineCookie / do_cmdline(get_list_line, ...)
                // RUST-PORT NOTE: the get_list_line cookie feeds one list item
                // per command line; modelled by joining the items with newlines
                // and running them through the single command hook.
                let joined = l
                    .borrow()
                    .lv_items
                    .iter()
                    .map(|it| tv_get_string(&it.li_tv))
                    .collect::<Vec<_>>()
                    .join("\n");
                let out = do_cmdline_via_hook(&joined);
                // c:1331 tv_list_unref(list); — refcount drops with the Rc.
                l.borrow_mut().lv_refcount -= 1;
                out
            }
            _ => String::new(),
        }
    };
    // The command run above appends to `capture_ga` via the redir path in C;
    // here the hook returns that text directly, so fold it into the buffer.
    capture_ga.with(|v| {
        if let Some(buf) = v.borrow_mut().as_mut() {
            buf.push_str(&output);
        }
    });

    msg_silent.with(|v| v.set(save_msg_silent)); // c:1333
    emsg_silent.with(|v| v.set(save_emsg_silent)); // c:1334
    emsg_noredir.with(|v| v.set(save_emsg_noredir)); // c:1335
    redir_off.with(|v| v.set(save_redir_off)); // c:1336
                                               // c:1337 "silent reg" or "silent echo x" leaves msg_col somewhere in line.
    if echo_output {
        // c:1341 msg_col = 0;
        msg_col.with(|v| v.set(0));
    } else {
        // c:1345 msg_col = save_msg_col;
        msg_col.with(|v| v.set(save_msg_col));
    }

    // c:1348 ga_append(capture_ga, NUL); — String is already NUL-free/terminated.
    let captured = capture_ga
        .with(|v| v.borrow_mut().take())
        .unwrap_or_default();
    rettv.v_type = VAR_STRING; // c:1349
    rettv.vval = v_string(captured); // c:1350

    // c:1352 capture_ga = save_capture_ga;
    capture_ga.with(|v| *v.borrow_mut() = save_capture_ga);
}

/// Port of `f_execute()` from `Src/eval/funcs.c:1357`.
///
/// "execute(command)" function.
pub fn f_execute(argvars: &[typval_T], rettv: &mut typval_T) {
    // c:1359 execute_common(argvars, rettv, 0);
    execute_common(argvars, rettv, 0);
}

/// Port of `screenchar_adjust()` from `Src/eval/funcs.c:5925`.
///
/// Look up the grid on top at screen coordinate (`row`, `col`) and make the
/// coordinates relative to that grid, as `screenchar()`/`screenattr()`/
/// `screenchars()`/`screenstring()` need before reading a cell.
///
/// RUST-PORT NOTE: `msg_scroll_flush()` (message.c) flushes pending scroll to
/// the live UI — there is no UI standalone, so it is a no-op here. With no
/// compositor, [`ui_comp_get_grid_at_coord`] returns `None` (see `grid.rs`); C
/// always gets a non-NULL grid (the fallback `default_grid`) and reads
/// `comp_row`/`comp_col` off it. With no grid the coordinates stay absolute and
/// callers treat the `None` grid as off-screen.
fn screenchar_adjust(grid: &mut Option<ScreenGrid>, row: &mut i32, col: &mut i32) {
    // c:5931 msg_scroll_flush(); — no live UI (no-op).

    // c:5933 *grid = ui_comp_get_grid_at_coord(*row, *col);
    *grid = ui_comp_get_grid_at_coord(*row, *col);

    // c:5936 Make `row` and `col` relative to the grid.
    if let Some(g) = grid {
        *row -= g.comp_row; // c:5937
        *col -= g.comp_col; // c:5938
    }
}

#[cfg(test)]
mod execute_screen_reference_tests {
    use super::*;
    use crate::ported::eval::typval_defs_h::listitem_T;

    #[test]
    fn execute_returns_hook_captured_text() {
        fn hook(cmd: &str) -> String {
            format!("ran:{cmd}")
        }
        let saved = DO_CMDLINE_HOOK.with(|h| *h.borrow());
        DO_CMDLINE_HOOK.with(|h| *h.borrow_mut() = Some(hook));
        // execute("echo x") — argvars[1] absent (VAR_UNKNOWN) → silent path.
        let args = vec![typval_T::from("echo x".to_string()), typval_T::default()];
        let mut rettv = typval_T::default();
        f_execute(&args, &mut rettv);
        match &rettv.vval {
            v_string(s) => assert_eq!(s, "ran:echo x"),
            _ => panic!("expected VAR_STRING capture"),
        }
        assert!(matches!(rettv.v_type, VAR_STRING));
        DO_CMDLINE_HOOK.with(|h| *h.borrow_mut() = saved);
    }

    #[test]
    fn execute_list_joins_lines() {
        fn hook(cmd: &str) -> String {
            cmd.to_string()
        }
        let saved = DO_CMDLINE_HOOK.with(|h| *h.borrow());
        DO_CMDLINE_HOOK.with(|h| *h.borrow_mut() = Some(hook));
        let mut list = list_T::default();
        list.lv_items.push(listitem_T {
            li_tv: typval_T::from("one".to_string()),
        });
        list.lv_items.push(listitem_T {
            li_tv: typval_T::from("two".to_string()),
        });
        list.lv_len = 2;
        let cmds = typval_T {
            v_type: VAR_LIST,
            vval: v_list(Some(std::rc::Rc::new(std::cell::RefCell::new(list)))),
            v_lock: Default::default(),
        };
        let args = vec![cmds, typval_T::default()];
        let mut rettv = typval_T::default();
        f_execute(&args, &mut rettv);
        match &rettv.vval {
            v_string(s) => assert_eq!(s, "one\ntwo"),
            _ => panic!("expected VAR_STRING capture"),
        }
        DO_CMDLINE_HOOK.with(|h| *h.borrow_mut() = saved);
    }

    #[test]
    fn execute_without_hook_is_empty() {
        let saved = DO_CMDLINE_HOOK.with(|h| *h.borrow());
        DO_CMDLINE_HOOK.with(|h| *h.borrow_mut() = None);
        let args = vec![typval_T::from("echo x".to_string()), typval_T::default()];
        let mut rettv = typval_T::default();
        f_execute(&args, &mut rettv);
        match &rettv.vval {
            v_string(s) => assert!(s.is_empty()),
            _ => panic!("expected VAR_STRING capture"),
        }
        DO_CMDLINE_HOOK.with(|h| *h.borrow_mut() = saved);
    }

    #[test]
    fn screenchar_adjust_no_grid_leaves_coords() {
        // No compositor → None grid → coords stay absolute (off-screen result).
        let mut grid: Option<ScreenGrid> = None;
        let mut row = 5;
        let mut col = 7;
        screenchar_adjust(&mut grid, &mut row, &mut col);
        assert!(grid.is_none());
        assert_eq!((row, col), (5, 7));
    }
}

#[cfg(test)]
mod phaseg_reference_tests {
    use super::*;
    use crate::ported::eval::typval::EVAL_STRING_HOOK;
    use crate::ported::eval::typval_defs_h::{blob_T, list_T, typval_T};

    #[test]
    fn f_eval_evaluates_number() {
        // Install a hook so any sub-expression string resolves; the ported eval1
        // parses a plain integer literal directly.
        fn hook(e: &str) -> Option<typval_T> {
            e.trim().parse::<i64>().ok().map(typval_T::from)
        }
        let saved = EVAL_STRING_HOOK.with(|h| *h.borrow());
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = Some(hook));
        let args = vec![typval_T::from("42".to_string())];
        let mut rettv = typval_T::default();
        f_eval(&args, &mut rettv);
        assert!(matches!(rettv.vval, v_number(42)));
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = saved);
    }

    #[test]
    fn get_search_arg_parses_flags() {
        use crate::ported::search::{BACKWARD, FORWARD};
        crate::ported::search::p_ws.with(|w| *w.borrow_mut() = true);
        // 'b' → BACKWARD; 'W' → wrapscan off; 'n'/'c' → SP_ flags.
        let mut flags = 0;
        let varp = typval_T::from("bWnc".to_string());
        let dir = get_search_arg(&varp, Some(&mut flags));
        assert_eq!(dir, BACKWARD);
        assert!(!crate::ported::search::p_ws.with(|w| *w.borrow()));
        assert_eq!(flags & SP_NOMOVE, SP_NOMOVE);
        assert_eq!(flags & SP_START, SP_START);

        // Unknown arg → FORWARD, no flags.
        let unk = typval_T::default();
        let mut f2 = 0;
        assert_eq!(get_search_arg(&unk, Some(&mut f2)), FORWARD);
        assert_eq!(f2, 0);
    }

    #[test]
    fn block_def2str_pads_and_copies() {
        let bd = block_def {
            startspaces: 2,
            endspaces: 1,
            textlen: 3,
            textstart_off: 1,
            line: "abcdef".to_string(),
            ..Default::default()
        };
        // 2 leading spaces + "bcd" + 1 trailing space.
        assert_eq!(block_def2str(&bd), "  bcd ");
    }

    #[test]
    fn msgpackparse_blob_roundtrip_fixint() {
        // msgpack: 0x01 (fixint 1), 0x02 (fixint 2).
        let mut blob = blob_T::default();
        blob.bv_ga = vec![0x01, 0x02];
        let mut out = list_T::default();
        msgpackparse_unpack_blob(&blob, &mut out);
        assert_eq!(out.lv_items.len(), 2);
        assert!(matches!(out.lv_items[0].li_tv.vval, v_number(1)));
        assert!(matches!(out.lv_items[1].li_tv.vval, v_number(2)));
    }

    #[test]
    fn create_environment_includes_and_overrides() {
        // A user job dict overriding an inherited var.
        std::env::set_var("VIMLRS_TEST_ENV", "inherited");
        let mut je = crate::ported::eval::typval_defs_h::dict_T::default();
        crate::ported::eval::typval::tv_dict_add_str(&mut je, "VIMLRS_TEST_ENV", "overridden");
        let env = create_environment(Some(&je), false, false, false, "");
        let got = env.borrow().dv_hashtab.get("VIMLRS_TEST_ENV").cloned();
        match got {
            Some(tv) => assert!(matches!(&tv.vval, v_string(s) if s == "overridden")),
            None => panic!("env var missing"),
        }
        std::env::remove_var("VIMLRS_TEST_ENV");
    }
}

#[cfg(test)]
mod helper_tests {
    use super::may_add_state_char;

    #[test]
    fn find_internal_func_lookup() {
        use super::find_internal_func;
        let add = find_internal_func("add").unwrap();
        assert_eq!((add.min_argc, add.max_argc, add.base_arg), (2, 2, 1));
        assert_eq!(find_internal_func("argc").unwrap().base_arg, 0); // no method base
        assert!(find_internal_func("not_a_builtin_xyz").is_none());
    }

    #[test]
    fn check_and_call_internal_method() {
        use super::{call_internal_method, check_internal_func};
        use crate::ported::eval::typval::CALL_FUNC_HOOK;
        use crate::ported::eval::typval_defs_h::{typval_T, typval_vval_union::v_number};
        use crate::ported::eval::userfunc::fcerr::*;
        // check_internal_func: arity + base index. "add" has base=1, args 2..2.
        assert_eq!(check_internal_func("add", 2), 1);
        assert_eq!(check_internal_func("add", 1), -1); // too few (E119)
                                                       // a non-method builtin (no base) → 0 (BASE_NONE)
                                                       // call_internal_method inserts base at position (base-1) and dispatches.
        fn hook(_c: &typval_T, args: &[typval_T]) -> Option<typval_T> {
            Some(typval_T::from(args.len() as i64))
        }
        let saved = CALL_FUNC_HOOK.with(|h| *h.borrow());
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = Some(hook));
        let mut rv = typval_T::from(0);
        // add(base, x): base + one arg = 2 args total
        assert_eq!(
            call_internal_method("add", &[typval_T::from(9)], &typval_T::from(1), &mut rv),
            FCERR_NONE
        );
        assert!(matches!(rv.vval, v_number(2)));
        // unknown builtin
        assert_eq!(
            call_internal_method("no_such_xyz", &[], &typval_T::from(0), &mut rv),
            FCERR_UNKNOWN
        );
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = saved);
    }

    #[test]
    fn call_internal_func_arity_and_dispatch() {
        use super::call_internal_func;
        use crate::ported::eval::typval::CALL_FUNC_HOOK;
        use crate::ported::eval::typval_defs_h::typval_T;
        use crate::ported::eval::userfunc::fcerr::*;
        fn hook(_c: &typval_T, args: &[typval_T]) -> Option<typval_T> {
            Some(typval_T::from(args.len() as i64))
        }
        let mut rv = typval_T::from(0);
        // unknown builtin
        assert_eq!(
            call_internal_func("no_such_builtin_xyz", &[], &mut rv),
            FCERR_UNKNOWN
        );
        // "add" requires exactly 2 args (arity checked before the hook)
        assert_eq!(
            call_internal_func("add", &[typval_T::from(1)], &mut rv),
            FCERR_TOOFEW
        );
        assert_eq!(
            call_internal_func(
                "add",
                &[typval_T::from(1), typval_T::from(2), typval_T::from(3)],
                &mut rv
            ),
            FCERR_TOOMANY
        );
        // correct arity, no hook installed → UNKNOWN; with hook → NONE
        let saved = CALL_FUNC_HOOK.with(|h| *h.borrow());
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = None);
        assert_eq!(
            call_internal_func("add", &[typval_T::from(1), typval_T::from(2)], &mut rv),
            FCERR_UNKNOWN
        );
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = Some(hook));
        assert_eq!(
            call_internal_func("add", &[typval_T::from(1), typval_T::from(2)], &mut rv),
            FCERR_NONE
        );
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = saved);
    }

    #[test]
    fn return_register_char_or_empty() {
        use super::return_register;
        use crate::ported::eval::typval_defs_h::{
            typval_T, typval_vval_union::v_string, VarType::VAR_STRING,
        };
        let mut tv = typval_T::from(0);
        return_register(b'a', &mut tv);
        assert!(matches!(&tv.vval, v_string(s) if s == "a"));
        assert_eq!(tv.v_type, VAR_STRING);
        return_register(0, &mut tv);
        assert!(matches!(&tv.vval, v_string(s) if s.is_empty()));
    }

    #[test]
    fn may_add_state_char_filters() {
        // No filter → always appended.
        let mut all = String::new();
        for c in ['m', 'o', 'x'] {
            may_add_state_char(&mut all, None, c);
        }
        assert_eq!(all, "mox");
        // Filter → only included chars kept, in call order.
        let mut some = String::new();
        for c in ['m', 'o', 'x'] {
            may_add_state_char(&mut some, Some("mx"), c);
        }
        assert_eq!(some, "mx");
    }

    #[test]
    fn tv_get_buf_number_name_and_specials() {
        use super::{get_buf_arg, tv_get_buf, tv_get_buf_from_arg};
        use crate::ported::buffer::{
            buflist_new, curbuf, firstbuf, lastbuf, top_file_num, BLN_LISTED,
        };
        use crate::ported::eval::typval_defs_h::{
            typval_T, typval_vval_union::v_float, VarLockStatus, VarType::VAR_FLOAT,
        };
        use crate::ported::message::{capture_errors_begin, capture_errors_take};
        use std::rc::Rc;

        // Reset the buffer-list thread_local state (Rust reuses test threads).
        firstbuf.with(|f| *f.borrow_mut() = None);
        lastbuf.with(|l| *l.borrow_mut() = None);
        curbuf.with(|c| *c.borrow_mut() = None);
        top_file_num.with(|t| t.set(1));

        let a = buflist_new(Some("/tmp/a".into()), None, 0, BLN_LISTED).unwrap();
        let b = buflist_new(Some("/tmp/b".into()), None, 0, BLN_LISTED).unwrap();
        curbuf.with(|c| *c.borrow_mut() = Some(a.clone()));

        // c:473 VAR_NUMBER → buflist_findnr
        assert!(Rc::ptr_eq(
            &tv_get_buf(&typval_T::from(1i64), false).unwrap(),
            &a
        ));
        // c:482 empty string → curbuf
        assert!(Rc::ptr_eq(
            &tv_get_buf(&typval_T::from(String::new()), false).unwrap(),
            &a
        ));
        // c:485 "$" → lastbuf
        assert!(Rc::ptr_eq(
            &tv_get_buf(&typval_T::from(String::from("$")), false).unwrap(),
            &b
        ));
        // number with no matching buffer → NULL
        assert!(tv_get_buf(&typval_T::from(999i64), false).is_none());

        // tv_get_buf_from_arg: a non-Number/String type (Float) is rejected by
        // tv_check_str_or_nr → NULL (and emits E805).
        let flt = typval_T {
            v_type: VAR_FLOAT,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_float(1.5),
        };
        capture_errors_begin();
        assert!(tv_get_buf_from_arg(&flt).is_none());
        assert!(capture_errors_take().iter().any(|e| e.starts_with("E805")));
        // Number arg still resolves through tv_get_buf_from_arg.
        assert!(Rc::ptr_eq(
            &tv_get_buf_from_arg(&typval_T::from(1i64)).unwrap(),
            &a
        ));

        // get_buf_arg on a missing buffer → NULL with E158.
        capture_errors_begin();
        assert!(get_buf_arg(&typval_T::from(999i64)).is_none());
        assert!(capture_errors_take()
            .iter()
            .any(|e| e.starts_with("E158: Invalid buffer name: 999")));
    }
}

#[cfg(test)]
mod islocked_reference_tests {
    use super::*;
    use crate::ported::eval::vars::{cmdidx_T, ex_let, ex_lockvar, exarg_T};

    fn let_eap(arg: &str) -> exarg_T {
        exarg_T {
            arg: arg.to_string(),
            cmdidx: cmdidx_T::CMD_let,
            ..Default::default()
        }
    }
    fn lock_eap(arg: &str, idx: cmdidx_T) -> exarg_T {
        exarg_T {
            arg: arg.to_string(),
            cmdidx: idx,
            ..Default::default()
        }
    }
    fn islocked(name: &str) -> varnumber_T {
        let args = vec![typval_T::from(name.to_string())];
        let mut rettv = typval_T::default();
        f_islocked(&args, &mut rettv);
        match rettv.vval {
            v_number(n) => n,
            _ => panic!("islocked() did not return a Number"),
        }
    }

    // Faithful `f_islocked` (eval/funcs.c:3223): 1 locked, 0 unlocked, -1 absent.
    #[test]
    fn islocked_locked_unlocked_missing() {
        // let g:il_x = 1 | lockvar g:il_x  → 1
        ex_let(&mut let_eap("g:il_x = 1"));
        ex_lockvar(&mut lock_eap("g:il_x", cmdidx_T::CMD_lockvar));
        assert_eq!(islocked("g:il_x"), 1);

        // let g:il_y = 1 (never locked) → 0
        ex_let(&mut let_eap("g:il_y = 1"));
        assert_eq!(islocked("g:il_y"), 0);

        // a name that does not exist → -1 (no error emitted)
        assert_eq!(islocked("g:il_nope"), -1);

        // unlockvar clears the lock → 0
        ex_lockvar(&mut lock_eap("g:il_x", cmdidx_T::CMD_unlockvar));
        assert_eq!(islocked("g:il_x"), 0);
    }

    // A locked container value is reported locked (tv_islocked descends into a
    // locked List/Dict), matching the C `di->di_tv` lock check.
    #[test]
    fn islocked_locked_container() {
        ex_let(&mut let_eap("g:il_l = [1, 2]"));
        ex_lockvar(&mut lock_eap("g:il_l", cmdidx_T::CMD_lockvar));
        assert_eq!(islocked("g:il_l"), 1);

        ex_let(&mut let_eap("g:il_l2 = [3, 4]"));
        assert_eq!(islocked("g:il_l2"), 0);
    }
}
