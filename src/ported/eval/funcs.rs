//! Port of `src/nvim/eval/funcs.c` (vendored at `csrc/eval/funcs.c`).
//!
//! Vimscript builtin functions. Each `f_<name>` matches the C signature
//! `void f_<name>(typval_T *argvars, typval_T *rettv, EvalFuncData fptr)`,
//! reduced to `(argvars, rettv)` (the `fptr` carries no data for these). As in
//! C, the caller (`call_func`) pre-initializes `rettv` to `VAR_NUMBER`/0 before
//! the call, so a numeric function only assigns `rettv->vval.v_number`; only
//! functions returning another type set `v_type`. Phase 3 ports a subset.
#![allow(non_snake_case)]

use crate::ported::eval::encode::{encode_tv2echo, encode_tv2string};
use crate::ported::eval::list::FILTER_MAP_EVAL_HOOK;
use crate::ported::eval::typval::tv_equal;
use crate::ported::eval::typval::{
    callback_from_typval, tv_blob_get, tv_check_for_number_arg, tv_check_for_string_arg,
    tv_dict_watcher_add, tv_dict_watcher_remove, tv_get_number, tv_get_string_buf,
    tv_get_string_chk, Callback, CALL_FUNC_HOOK,
};
use crate::ported::eval::typval::{
    tv_blob_len, tv_dict_add_tv, tv_dict_find, tv_dict_len, tv_get_bool, tv_get_float,
    tv_get_number_chk, tv_get_string, tv_list_alloc_ret, tv_list_append_number,
    tv_list_append_string, tv_list_append_tv, tv_list_copy, tv_list_find_nr, tv_list_flatten,
    tv_list_len, tv_list_ref,
};
use crate::ported::eval::typval::{
    tv_dict_add_list, tv_dict_add_nr, tv_dict_add_str, tv_dict_alloc, tv_dict_alloc_ret,
    tv_list_alloc, tv_list_append_list,
};
use crate::ported::eval::typval_defs_h::{
    typval_T, typval_vval_union::*, varnumber_T, BoolVarValue::*, SpecialVarValue::*, VarType::*,
    VAR_TYPE_BLOB, VAR_TYPE_BOOL, VAR_TYPE_DICT, VAR_TYPE_FLOAT, VAR_TYPE_FUNC, VAR_TYPE_LIST,
    VAR_TYPE_NUMBER, VAR_TYPE_SPECIAL, VAR_TYPE_STRING,
};
use crate::ported::eval::vars::{
    assert_error, get_vim_var_str, set_vim_var_nr,
    vv::{VV_EXCEPTION, VV_REG, VV_SHELL_ERROR},
};
use crate::ported::eval_h::{FAIL, OK};
use crate::ported::message::emsg;
use crate::ported::ops::{
    format_reg_type, get_reg_contents, get_reg_type, get_yank_type, write_reg_contents_lst,
    MotionType,
};
use crate::ported::option::get_option_value;
use crate::ported::os::env::os_get_pid;
use crate::ported::os::time::{os_hrtime, os_localtime_r, os_strptime};
use crate::ported::profile::{
    profile_end, profile_msg, profile_signed, profile_start, profile_sub, proftime_T,
};
use crate::ported::sha256::sha256_bytes;
use crate::viml_regex::regex_match;
use crate::viml_regex::{regex_matchlist, regex_matchstrpos};

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
        (VAR_NUMBER, _) | (VAR_FLOAT, _) => v_number(tv_get_string(arg).len() as varnumber_T),
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
    rettv.vval = v_number(tv_get_float(&argvars[0]) as varnumber_T);
}

/// Port of `f_function()` from `Src/eval/funcs.c` — a Funcref to the named
/// function.
pub fn f_function(argvars: &[typval_T], rettv: &mut typval_T) {
    let name = tv_get_string(&argvars[0]);
    // c: with no extra args this is a plain Funcref.
    if argvars.len() < 2 {
        rettv.v_type = VAR_FUNC;
        rettv.vval = v_string(name);
        return;
    }
    // c: function(name, [args]) / function(name, {dict}) / both → a Partial.
    let mut pt_argv: Vec<typval_T> = Vec::new();
    let mut pt_dict = None;
    for a in &argvars[1..] {
        match (a.v_type, &a.vval) {
            (VAR_LIST, v_list(Some(l))) => {
                pt_argv = l
                    .borrow()
                    .lv_items
                    .iter()
                    .map(|it| it.li_tv.clone())
                    .collect();
            }
            (VAR_DICT, v_dict(Some(d))) => pt_dict = Some(d.clone()),
            _ => {}
        }
    }
    rettv.v_type = VAR_PARTIAL;
    rettv.vval = v_partial(Some(std::rc::Rc::new(
        crate::ported::eval::typval_defs_h::partial_T {
            pt_refcount: 1,
            pt_name: name,
            pt_argv,
            pt_dict,
        },
    )));
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
    let s = char::from_u32(n as u32)
        .map(String::from)
        .unwrap_or_default();
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(s);
}

/// Port of `f_repeat()` from `Src/eval/funcs.c` — repeat a String (or List)
/// `count` times.
pub fn f_repeat(argvars: &[typval_T], rettv: &mut typval_T) {
    let count = tv_get_number_chk(&argvars[1], None).max(0) as usize;
    if let (VAR_LIST, v_list(Some(l))) = (argvars[0].v_type, &argvars[0].vval) {
        let src = l.borrow();
        let out = tv_list_alloc_ret(rettv, (src.lv_len as usize * count) as isize);
        let mut ob = out.borrow_mut();
        for _ in 0..count {
            for it in &src.lv_items {
                tv_list_append_tv(&mut ob, it.li_tv.clone());
            }
        }
        return;
    }
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(tv_get_string(&argvars[0]).repeat(count));
}

/// Port of `f_split()` from `Src/eval/funcs.c`.
///
/// "split({str} [, {pat} [, {keepempty}]])" — split on the Vim regex `{pat}`
/// (default whitespace `\s\+`), dropping empty pieces unless `{keepempty}`.
pub fn f_split(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let keepempty = argvars
        .get(2)
        .is_some_and(|t| tv_get_number_chk(t, None) != 0);
    let pat = argvars.get(1).map(tv_get_string).filter(|p| !p.is_empty());
    let parts: Vec<String> = match pat {
        Some(p) => crate::viml_regex::regex_split(
            &s,
            &p,
            tv_get_bool(&get_option_value("ignorecase")) != 0,
            keepempty,
        ),
        None => s.split_whitespace().map(String::from).collect(),
    };
    let l = tv_list_alloc_ret(rettv, parts.len() as isize);
    let mut lb = l.borrow_mut();
    for p in &parts {
        tv_list_append_string(&mut lb, p);
    }
}

/// Port of `f_matchstr()` from `Src/eval/funcs.c` — the matched substring of the
/// Vim regex `{pat}` in `{expr}`, or "".
pub fn f_matchstr(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let pat = tv_get_string(&argvars[1]);
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(crate::viml_regex::regex_matchstr(
        &pat,
        &s,
        tv_get_bool(&get_option_value("ignorecase")) != 0,
    ));
}

/// Port of `f_match()` from `Src/eval/funcs.c` — the char index of the first
/// match of `{pat}` in `{expr}`, or -1.
pub fn f_match(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let pat = tv_get_string(&argvars[1]);
    rettv.vval = v_number(crate::viml_regex::regex_match_index(
        &pat,
        &s,
        tv_get_bool(&get_option_value("ignorecase")) != 0,
    ));
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

/// Port of `f_join()` from `Src/eval/funcs.c` — join a List with a separator
/// (default " ").
// `f_join` lives in its real home file, `src/ported/eval/typval.rs` (eval/typval.c).

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
    let l = tv_list_alloc_ret(rettv, 0);
    let mut lb = l.borrow_mut();
    if stride > 0 {
        let mut i = start;
        while i <= end {
            tv_list_append_number(&mut lb, i);
            i += stride;
        }
    } else if stride < 0 {
        let mut i = start;
        while i >= end {
            tv_list_append_number(&mut lb, i);
            i += stride;
        }
    }
}

/// Port of `f_add()` from `Src/eval/funcs.c` — append `{item}` to `{list}` and
/// return the list.
pub fn f_add(argvars: &[typval_T], rettv: &mut typval_T) {
    if let (VAR_LIST, v_list(Some(l))) = (argvars[0].v_type, &argvars[0].vval) {
        tv_list_append_tv(&mut l.borrow_mut(), argvars[1].clone());
        *rettv = argvars[0].clone();
    } else {
        emsg("E897: List or Blob required");
    }
}

/// Port of `f_reverse()` from `Src/eval/funcs.c` — reverse a List in place.
pub fn f_reverse(argvars: &[typval_T], rettv: &mut typval_T) {
    if let (VAR_LIST, v_list(Some(l))) = (argvars[0].v_type, &argvars[0].vval) {
        l.borrow_mut().lv_items.reverse();
        *rettv = argvars[0].clone();
    }
}

/// Port of `f_get()` from `Src/eval/funcs.c` — `get({list}, {idx} [, {def}])` /
/// `get({dict}, {key} [, {def}])`.
pub fn f_get(argvars: &[typval_T], rettv: &mut typval_T) {
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
        _ => None,
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

/// Port of `f_count()` from `Src/eval/funcs.c` (subset) — occurrences of
/// `{expr}` in a List.
// `f_count` lives in its real home file, `src/ported/eval/list.rs` (eval/list.c).

/// Port of `f_index()` from `Src/eval/funcs.c` (subset) — first index of
/// `{expr}` in a List, or -1.
pub fn f_index(argvars: &[typval_T], rettv: &mut typval_T) {
    let needle = &argvars[1];
    let idx = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => l
            .borrow()
            .lv_items
            .iter()
            .position(|it| crate::ported::eval::typval::tv_equal(&it.li_tv, needle, false))
            .map_or(-1, |i| i as varnumber_T),
        _ => -1,
    };
    rettv.vval = v_number(idx);
}

/// Port of `f_has()` from `Src/eval/funcs.c` (subset) — feature presence. Phase
/// 3 reports the always-true pseudo-features and `0` otherwise.
pub fn f_has(argvars: &[typval_T], rettv: &mut typval_T) {
    let feat = tv_get_string(&argvars[0]);
    let yes = matches!(feat.as_str(), "eval" | "float" | "vimlrs");
    rettv.vval = v_number(yes as varnumber_T);
}

/// Port of `f_exists()` from `Src/eval/funcs.c` (subset) — whether a variable
/// exists (the `*func`/`:cmd`/option forms arrive with their ports).
pub fn f_exists(argvars: &[typval_T], rettv: &mut typval_T) {
    let name = tv_get_string(&argvars[0]);
    // c: a leading '#' queries autocommands — `#{event}` or `#{event}#{pat}`.
    let present = if let Some(au) = name.strip_prefix('#') {
        au_exists(au)
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
    let bytes: Vec<char> = fmt.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    let mut arg = 1usize;
    while i < bytes.len() {
        if bytes[i] != '%' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        i += 1; // past '%'
                // Flags.
        let mut left = false;
        let mut zero = false;
        let mut plus = false;
        let mut space = false;
        while i < bytes.len() && matches!(bytes[i], '-' | '0' | '+' | ' ' | '#') {
            match bytes[i] {
                '-' => left = true,
                '0' => zero = true,
                '+' => plus = true,
                ' ' => space = true,
                _ => {}
            }
            i += 1;
        }
        // Width.
        let mut width = 0usize;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            width = width * 10 + (bytes[i] as usize - '0' as usize);
            i += 1;
        }
        // Precision.
        let mut prec: Option<usize> = None;
        if i < bytes.len() && bytes[i] == '.' {
            i += 1;
            let mut p = 0usize;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                p = p * 10 + (bytes[i] as usize - '0' as usize);
                i += 1;
            }
            prec = Some(p);
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
        let cur = argvars.get(arg);
        let core = match conv {
            'd' | 'i' => cur.map_or(0, |t| tv_get_number_chk(t, None)).to_string(),
            's' => {
                let mut s = cur.map(tv_get_string).unwrap_or_default();
                if let Some(p) = prec {
                    s.truncate(p);
                }
                s
            }
            'f' => format!("{:.*}", prec.unwrap_or(6), cur.map_or(0.0, tv_get_float)),
            'x' => format!("{:x}", cur.map_or(0, |t| tv_get_number_chk(t, None))),
            'X' => format!("{:X}", cur.map_or(0, |t| tv_get_number_chk(t, None))),
            'o' => format!("{:o}", cur.map_or(0, |t| tv_get_number_chk(t, None))),
            'b' | 'B' => format!("{:b}", cur.map_or(0, |t| tv_get_number_chk(t, None))),
            'u' => (cur.map_or(0, |t| tv_get_number_chk(t, None)) as u64).to_string(),
            'c' => char::from_u32(cur.map_or(0, |t| tv_get_number_chk(t, None)) as u32)
                .unwrap_or('\u{0}')
                .to_string(),
            'g' | 'G' => {
                // C `%g`: `prec` significant digits (default 6), trailing zeros
                // stripped, `%e`/`%f` chosen by exponent.
                let v = cur.map_or(0.0, tv_get_float);
                let s = if v.is_infinite() {
                    if v < 0.0 { "-inf" } else { "inf" }.to_string()
                } else if v.is_nan() {
                    "nan".to_string()
                } else {
                    crate::ported::eval::encode::vim_float_g(v, prec.unwrap_or(6) as i32)
                };
                if conv == 'G' {
                    s.to_uppercase()
                } else {
                    s
                }
            }
            'e' | 'E' => {
                let s = format!("{:.*e}", prec.unwrap_or(6), cur.map_or(0.0, tv_get_float));
                // Rust emits "1e2"; C/Vim emit "1.000000e+02" — add sign + 2-digit exp.
                let s = if let Some(ep) = s.find('e') {
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
                };
                s
            }
            other => {
                out.push('%');
                out.push(other);
                continue;
            }
        };
        arg += 1;
        // For signed numeric conversions the `+`/space flag forces a sign on
        // non-negative values; split it off `core` so zero-padding lands between
        // the sign and the digits (`%+05d` of 7 → `+0007`).
        let signed = matches!(conv, 'd' | 'i' | 'f' | 'F' | 'e' | 'E' | 'g' | 'G');
        let (sign, core) = if signed {
            if let Some(rest) = core.strip_prefix('-') {
                ("-", rest.to_string())
            } else if plus {
                ("+", core)
            } else if space {
                (" ", core)
            } else {
                ("", core)
            }
        } else {
            ("", core)
        };
        // Pad to width (width counts the sign).
        let len = sign.len() + core.chars().count();
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

/// Port of `f_insert()` from `Src/eval/funcs.c` — insert `{item}` at `{idx}`
/// (default 0) in `{list}`, returning the list.
pub fn f_insert(argvars: &[typval_T], rettv: &mut typval_T) {
    if let (VAR_LIST, v_list(Some(l))) = (argvars[0].v_type, &argvars[0].vval) {
        let mut lb = l.borrow_mut();
        let len = lb.lv_len as varnumber_T;
        let mut idx = argvars.get(2).map_or(0, |t| tv_get_number_chk(t, None));
        if idx < 0 {
            idx += len;
        }
        let idx = (idx.max(0) as usize).min(lb.lv_items.len());
        lb.lv_items.insert(
            idx,
            crate::ported::eval::typval_defs_h::listitem_T {
                li_tv: argvars[1].clone(),
            },
        );
        lb.lv_len = lb.lv_items.len() as i32;
    }
    *rettv = argvars[0].clone();
}

/// Port of `f_remove()` from `Src/eval/funcs.c` (subset) — remove and return an
/// item from a `{list}` by index, or a value from a `{dict}` by key.
// `f_remove` lives in its real home file, `src/ported/eval/list.rs` (eval/list.c).

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

/// Port of `f_uniq()` from `Src/eval/funcs.c` (subset) — remove adjacent
/// duplicate items from a `{list}`, returning it.
// `f_sort`/`f_uniq` live in their real home file, `src/ported/eval/typval.rs`.

// ── batch 4: regex-list, more string, list helpers (Src/eval/funcs.c) ──

/// Port of `f_matchlist()` from `Src/eval/funcs.c` — `[whole, sub1, …]` of the
/// first match of `{pat}` in `{expr}`.
pub fn f_matchlist(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let pat = tv_get_string(&argvars[1]);
    let parts = crate::viml_regex::regex_matchlist(
        &pat,
        &s,
        tv_get_bool(&get_option_value("ignorecase")) != 0,
    );
    let l = tv_list_alloc_ret(rettv, parts.len() as isize);
    let mut lb = l.borrow_mut();
    for p in &parts {
        tv_list_append_string(&mut lb, p);
    }
}

/// Port of `f_matchend()` from `Src/eval/funcs.c` — char index just past the
/// first match of `{pat}`, or -1.
pub fn f_matchend(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let pat = tv_get_string(&argvars[1]);
    rettv.vval = v_number(crate::viml_regex::regex_matchend(
        &pat,
        &s,
        tv_get_bool(&get_option_value("ignorecase")) != 0,
    ));
}

/// Port of `f_escape()` from `Src/eval/funcs.c` — prefix each character of
/// `{string}` that occurs in `{chars}` with a backslash.
pub fn f_escape(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let chars: Vec<char> = tv_get_string(&argvars[1]).chars().collect();
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if chars.contains(&c) {
            out.push('\\');
        }
        out.push(c);
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
            if let Some(c) = char::from_u32(tv_get_number_chk(&it.li_tv, None) as u32) {
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
    let s = tv_get_string(&argvars[0]);
    let pat = tv_get_string(&argvars[1]);
    let (m, start, end) = crate::viml_regex::regex_matchstrpos(
        &pat,
        &s,
        tv_get_bool(&get_option_value("ignorecase")) != 0,
    );
    let l = tv_list_alloc_ret(rettv, 3);
    let mut lb = l.borrow_mut();
    tv_list_append_string(&mut lb, &m);
    tv_list_append_number(&mut lb, start);
    tv_list_append_number(&mut lb, end);
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
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
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

    // Build the lines + the default motion type from the value's shape.
    let (lines, default_type) = match (contents.v_type, &contents.vval) {
        (VAR_LIST, v_list(Some(l))) => (
            l.borrow()
                .lv_items
                .iter()
                .map(|it| tv_get_string(&it.li_tv))
                .collect::<Vec<_>>(),
            MotionType::LineWise,
        ),
        _ => {
            let s = tv_get_string(&contents);
            // A trailing newline makes a string register linewise (Vim).
            if let Some(stripped) = s.strip_suffix('\n') {
                (
                    stripped.split('\n').map(str::to_string).collect(),
                    MotionType::LineWise,
                )
            } else {
                (
                    s.split('\n').map(str::to_string).collect(),
                    MotionType::CharWise,
                )
            }
        }
    };
    write_reg_contents_lst(regname, lines, yank_type.unwrap_or(default_type), append);
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
    f_function(argvars, rettv);
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

/// Port of `f_indexof()` from `Src/eval/funcs.c` — the index of the first
/// List/Blob item for which `{expr}` (string or funcref, `v:key`/`v:val`) is
/// true, or -1. An optional `{startidx:n}` dict starts the scan later.
pub fn f_indexof(argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
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
    let expr = &argvars[1];
    let test = |idx: i64, item: &typval_T| -> bool {
        let key = typval_T::from(idx);
        FILTER_MAP_EVAL_HOOK
            .with(|h| *h.borrow())
            .and_then(|f| f(expr, &key, item))
            .map(|r| tv_get_number(&r) != 0)
            .unwrap_or(false)
    };
    match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => {
            let items: Vec<typval_T> = l
                .borrow()
                .lv_items
                .iter()
                .map(|it| it.li_tv.clone())
                .collect();
            let start = if startidx < 0 {
                (items.len() as i64 + startidx).max(0)
            } else {
                startidx
            };
            for (i, item) in items.iter().enumerate().skip(start as usize) {
                if test(i as i64, item) {
                    *rettv = typval_T::from(i as varnumber_T);
                    return;
                }
            }
        }
        (VAR_BLOB, v_blob(Some(b))) => {
            let bytes = b.borrow().bv_ga.clone();
            let start = if startidx < 0 {
                (bytes.len() as i64 + startidx).max(0)
            } else {
                startidx
            };
            for (i, byte) in bytes.iter().enumerate().skip(start as usize) {
                if test(i as i64, &typval_T::from(*byte as varnumber_T)) {
                    *rettv = typval_T::from(i as varnumber_T);
                    return;
                }
            }
        }
        _ => {}
    }
}

// ── pattern / option / editor-absent builtins (funcs.c) ──────────────────────

/// Port of `f_matchstrlist()` from `Src/eval/funcs.c` — for each String in a
/// List, the first match of `{pat}`: `{idx, byteidx, text [, submatches]}`.
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
    let mk = |t, v| typval_T {
        v_type: t,
        v_lock: crate::ported::eval::typval_defs_h::VarLockStatus::VAR_UNLOCKED,
        vval: v,
    };
    for (idx, item) in items.iter().enumerate() {
        let s = tv_get_string(item);
        let (text, cstart, _) = regex_matchstrpos(&pat, &s, ic);
        if cstart < 0 {
            continue;
        }
        // c: byteidx is a BYTE offset; regex returns a char index.
        let byteidx: usize = s.chars().take(cstart as usize).map(char::len_utf8).sum();
        let d = tv_dict_alloc();
        tv_dict_add_tv(
            &mut d.borrow_mut(),
            "idx",
            typval_T::from(idx as varnumber_T),
        );
        tv_dict_add_tv(
            &mut d.borrow_mut(),
            "byteidx",
            typval_T::from(byteidx as varnumber_T),
        );
        tv_dict_add_tv(&mut d.borrow_mut(), "text", typval_T::from(text));
        if submatches {
            // c: always the 9 \1..\9 backrefs, "" for groups that didn't match.
            let groups = regex_matchlist(&pat, &s, ic);
            let sub = tv_list_alloc(0);
            for i in 1..=9 {
                tv_list_append_string(
                    &mut sub.borrow_mut(),
                    groups.get(i).map_or("", |g| g.as_str()),
                );
            }
            tv_dict_add_tv(
                &mut d.borrow_mut(),
                "submatches",
                mk(VAR_LIST, v_list(Some(sub))),
            );
        }
        tv_list_append_tv(&mut l.borrow_mut(), mk(VAR_DICT, v_dict(Some(d))));
    }
}

/// Port of `f_fnameescape()` from `Src/eval/funcs.c` — backslash-escape the
/// characters special to a `:` command's filename argument.
pub fn f_fnameescape(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: PATH_ESC_CHARS " \t\n*?[{`$\\%#'\"|!<".
    const ESC: &[u8] = b" \t\n*?[{`$\\%#'\"|!<";
    let name = tv_get_string(&argvars[0]);
    let mut out = String::with_capacity(name.len() + 2);
    for (i, b) in name.bytes().enumerate() {
        // A leading '+' or '>' is also escaped (would start a different arg).
        if ESC.contains(&b) || (i == 0 && (b == b'+' || b == b'>')) {
            out.push('\\');
        }
        out.push(b as char);
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
/// Port of `f_hlexists()` — no highlight groups → 0.
pub fn f_hlexists(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
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
    CURBUF.with(|b| b.borrow().len().max(1) as varnumber_T)
}

/// Port of `tv_get_lnum()` (Neovim eval/typval.c) — resolve a line-number
/// argument: a Number, or a String like `.` (cursor), `$` (last line), `w0`/
/// `w$` (window top/bottom = first/last here), or a `'m` mark (0, no marks).
fn tv_get_lnum(tv: &typval_T) -> varnumber_T {
    if tv.v_type == VAR_STRING {
        let s = tv_get_string(tv);
        match s.as_str() {
            "." => CURPOS.with(|c| c.borrow().0),
            "$" | "w$" => curbuf_len(),
            "w0" => 1,
            _ if s.starts_with('\'') => 0,
            _ => s.parse().unwrap_or(0),
        }
    } else {
        tv_get_number(tv)
    }
}

/// Port of `get_buffer_lines()` (Neovim buffer.c) — the current buffer's lines
/// from `start` to `end` (1-based, inclusive, clamped to the buffer).
fn get_buffer_lines(start: varnumber_T, end: varnumber_T) -> Vec<String> {
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
    CURPOS.with(|p| {
        let mut p = p.borrow_mut();
        *p = (l, c, c);
    });
}

/// Port of `f_getpos()`/`getpos_both(…,false,false)` — the position of `{expr}`
/// as `[bufnum, lnum, col, off]`; `.` is the cursor, marks are `[0,0,0,0]`.
pub fn f_getpos(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let (lnum, col) = if s == "." {
        CURPOS.with(|c| {
            let c = c.borrow();
            (c.0, c.1)
        })
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
    let (lnum, col, curswant) = CURPOS.with(|c| *c.borrow());
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
    let (lnum, ccol) = CURPOS.with(|c| {
        let c = c.borrow();
        (c.0, c.1)
    });
    let col = match s.as_str() {
        "." => ccol,
        "$" => get_buffer_lines(lnum, lnum)
            .first()
            .map_or(1, |l| l.len() as varnumber_T + 1),
        _ if s.starts_with('\'') => 0,
        _ => 0,
    };
    *rettv = typval_T::from(col);
}
/// Port of `f_charcol()`/`get_col(…,true)` — like `col()` but a character index.
pub fn f_charcol(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let (lnum, ccol) = CURPOS.with(|c| {
        let c = c.borrow();
        (c.0, c.1)
    });
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
    if argvars.len() > 1 && tv_get_bool(&argvars[1]) != 0 {
        let l = tv_list_alloc_ret(rettv, 2);
        let mut lb = l.borrow_mut();
        tv_list_append_number(&mut lb, 0);
        tv_list_append_number(&mut lb, 0);
    } else {
        *rettv = typval_T::from(0 as varnumber_T);
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
    let d = tv_dict_alloc_ret(rettv);
    let mut db = d.borrow_mut();
    for key in [
        "bytes",
        "chars",
        "words",
        "cursor_bytes",
        "cursor_chars",
        "cursor_words",
    ] {
        tv_dict_add_nr(&mut db, key, 0);
    }
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
    tv_list_alloc_ret(rettv, 0);
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
    let (clnum, ccol) = CURPOS.with(|c| {
        let c = c.borrow();
        (c.0, c.1)
    });
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
    let (clnum, ccol) = CURPOS.with(|c| {
        let c = c.borrow();
        (c.0, c.1)
    });
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

/// Port of `f_synID()` — no syntax highlighter → id 0.
pub fn f_synID(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_synIDtrans()` — no syntax → translated id 0.
pub fn f_synIDtrans(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_synIDattr()` — no syntax → "" (the C NULL string).
pub fn f_synIDattr(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(String::new());
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
pub fn f_getregion(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_list_alloc_ret(rettv, 0);
}
/// Port of `f_getregionpos()` — no buffer/selection → empty List.
pub fn f_getregionpos(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_list_alloc_ret(rettv, 0);
}
/// Port of `f_matchbufline()` — no buffer → empty List.
pub fn f_matchbufline(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_list_alloc_ret(rettv, 0);
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
    // c: `.` sets the cursor from [bufnum, lnum, col, off]; marks are accepted.
    if expr == "." {
        if let (VAR_LIST, v_list(Some(l))) = (argvars[1].v_type, &argvars[1].vval) {
            let items = &l.borrow().lv_items;
            let lnum = items.get(1).map_or(0, |it| tv_get_number(&it.li_tv));
            let col = items.get(2).map_or(1, |it| tv_get_number(&it.li_tv));
            set_cursorpos(lnum, col);
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
// `assert_error`, csrc/eval/vars.c:3360) and returns 1 on failure, 0 on
// success — so a script can run a batch of asserts and then inspect
// `v:errors`. Behaviour and message wording follow the spec documented in
// `csrc/eval.lua` (the implementations live in Neovim's `testing.c`, which is
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
// reduce to the value their C bodies (csrc/eval/buffer.c, window.c) return when
// the looked-up buffer/window is absent: a missing buffer is -1 / 0 / "", a
// window measurement is -1, and there is one implicit window and tab page.

/// Port of `f_bufnr()` (buffer.c) — no such buffer → -1.
pub fn f_bufnr(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
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
    *rettv = typval_T::from(String::new());
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
// Faithful to the C "absent" returns (csrc/eval/buffer.c, window.c): reading a
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
    tv_list_alloc_ret(rettv, 0);
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
/// Port of `f_islocked()` (funcs.c) — variable locks are not modeled → 0
/// (not locked).
pub fn f_islocked(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
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
/// Port of `f_msgpackdump()` (funcs.c) — nothing to encode → empty List.
pub fn f_msgpackdump(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_list_alloc_ret(rettv, 0);
}
/// Port of `f_msgpackparse()` (funcs.c) — nothing to parse → empty List.
pub fn f_msgpackparse(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_list_alloc_ret(rettv, 0);
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
pub fn f_stdpath(argvars: &[typval_T], rettv: &mut typval_T) {
    let home = std::env::var("HOME").unwrap_or_default();
    let xdg_home = |env: &str, default_rel: &str| -> String {
        let base = std::env::var(env)
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("{home}/{default_rel}"));
        format!("{base}/nvim")
    };
    let xdg_dirs = |env: &str, default: &str| -> Vec<String> {
        let val = std::env::var(env)
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| default.to_string());
        val.split(':')
            .filter(|s| !s.is_empty())
            .map(|d| format!("{d}/nvim"))
            .collect()
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
            *rettv = typval_T::from(format!("{run}/nvim"));
        }
        "config_dirs" | "data_dirs" => {
            let dirs = if kind == "config_dirs" {
                xdg_dirs("XDG_CONFIG_DIRS", "/etc/xdg")
            } else {
                xdg_dirs("XDG_DATA_DIRS", "/usr/local/share:/usr/share")
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
pub fn f_keytrans(argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(tv_get_string(&argvars[0]));
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
// vendored `csrc/eval/` tree (search.c, cmdhist.c, digraph.c, mbyte.c,
// testing.c, and the full eval/funcs.c table), so their `fn` names are recorded
// in `tests/data/fake_fn_allowlist.txt`. Each is a faithful port cited to its
// home file.
// ════════════════════════════════════════════════════════════════════════════

use crate::ported::eval::typval_defs_h::VarLockStatus;

// ── matchfuzzy()/matchfuzzypos() — Neovim search.c (fuzzy_match*). ──

const FUZZY_SEQUENTIAL_BONUS: i32 = 15;
const FUZZY_SEPARATOR_BONUS: i32 = 30;
const FUZZY_CAMEL_BONUS: i32 = 30;
const FUZZY_FIRST_LETTER_BONUS: i32 = 15;
const FUZZY_LEADING_LETTER_PENALTY: i32 = -5;
const FUZZY_MAX_LEADING_LETTER_PENALTY: i32 = -15;
const FUZZY_UNMATCHED_LETTER_PENALTY: i32 = -1;
const FUZZY_RECURSION_LIMIT: i32 = 10;
const FUZZY_MAX_MATCHES: usize = 256;

/// Port of `fuzzy_match_compute_score()` (Neovim search.c) — score a completed
/// set of match positions: base 100, a clamped leading-letter penalty, an
/// unmatched-letter penalty, plus sequential/camel/separator/first-letter
/// bonuses per matched char.
fn fuzzy_match_compute_score(str: &[char], matches: &[usize], camelcase: bool) -> i32 {
    let mut score = 100;
    // c: leading-letter penalty, clamped to MAX_LEADING_LETTER_PENALTY.
    let mut penalty = FUZZY_LEADING_LETTER_PENALTY * matches[0] as i32;
    if penalty < FUZZY_MAX_LEADING_LETTER_PENALTY {
        penalty = FUZZY_MAX_LEADING_LETTER_PENALTY;
    }
    score += penalty;
    // c: unmatched-letter penalty.
    let unmatched = str.len() as i32 - matches.len() as i32;
    score += FUZZY_UNMATCHED_LETTER_PENALTY * unmatched;
    // c: ordering bonuses.
    for i in 0..matches.len() {
        let curr_idx = matches[i];
        if i > 0 && curr_idx == matches[i - 1] + 1 {
            score += FUZZY_SEQUENTIAL_BONUS;
        }
        if curr_idx > 0 {
            let neighbor = str[curr_idx - 1];
            let curr = str[curr_idx];
            if camelcase && neighbor.is_lowercase() && curr.is_uppercase() {
                score += FUZZY_CAMEL_BONUS;
            }
            if neighbor == '/' || neighbor == '\\' || neighbor == ' ' || neighbor == '_' {
                score += FUZZY_SEPARATOR_BONUS;
            }
        } else {
            score += FUZZY_FIRST_LETTER_BONUS;
        }
    }
    score
}

/// Port of `fuzzy_match_recursive()` (Neovim search.c) — recursively match the
/// remaining `fuzpat` against `str_rem` (whose first char is at absolute
/// `str_idx` in `str_full`), accumulating matched positions into `matches`.
/// Returns the best score when the whole pattern matches.
#[allow(clippy::too_many_arguments)]
fn fuzzy_match_recursive(
    fuzpat: &[char],
    str_rem: &[char],
    str_idx: usize,
    str_full: &[char],
    camelcase: bool,
    matches: &mut Vec<usize>,
    recursion: &mut i32,
) -> Option<i32> {
    *recursion += 1;
    if *recursion >= FUZZY_RECURSION_LIMIT {
        return None;
    }
    if fuzpat.is_empty() || str_rem.is_empty() {
        return None;
    }
    let mut recursive_best: Option<(i32, Vec<usize>)> = None;
    let mut fp = 0usize;
    let mut sp = 0usize;
    while fp < fuzpat.len() && sp < str_rem.len() {
        let c1 = fuzpat[fp];
        let c2 = str_rem[sp];
        // c: case-insensitive compare (mb_tolower).
        if c1.to_lowercase().eq(c2.to_lowercase()) {
            if matches.len() >= FUZZY_MAX_MATCHES {
                return None;
            }
            // c: recursive call that "skips" this match (copy-on-write matches).
            let mut rec_matches = matches.clone();
            if let Some(rscore) = fuzzy_match_recursive(
                &fuzpat[fp..],
                &str_rem[sp + 1..],
                str_idx + sp + 1,
                str_full,
                camelcase,
                &mut rec_matches,
                recursion,
            ) {
                #[allow(clippy::unnecessary_map_or)]
                if recursive_best.as_ref().map_or(true, |(bs, _)| rscore > *bs) {
                    recursive_best = Some((rscore, rec_matches));
                }
            }
            matches.push(str_idx + sp);
            fp += 1;
        }
        sp += 1;
    }
    let matched = fp >= fuzpat.len();
    let this_score = if matched {
        fuzzy_match_compute_score(str_full, matches, camelcase)
    } else {
        0
    };
    if let Some((rscore, rmatches)) = recursive_best {
        if !matched || rscore > this_score {
            *matches = rmatches;
            return Some(rscore);
        }
    }
    if matched {
        return Some(this_score);
    }
    None
}

/// Port of `fuzzy_match()` (Neovim search.c) — match `pat` against `str`. With
/// `matchseq` the pattern matches as a single sequence; otherwise it is split
/// on spaces into words, each matched independently and the scores summed.
/// Returns the total score and all matched char positions.
fn fuzzy_match(
    str: &[char],
    pat: &str,
    matchseq: bool,
    camelcase: bool,
) -> Option<(i32, Vec<usize>)> {
    let words: Vec<Vec<char>> = if matchseq {
        vec![pat.chars().collect()]
    } else {
        pat.split(' ')
            .filter(|w| !w.is_empty())
            .map(|w| w.chars().collect())
            .collect()
    };
    if words.is_empty() {
        return None;
    }
    let mut total = 0i32;
    let mut all: Vec<usize> = Vec::new();
    for w in &words {
        let mut matches: Vec<usize> = Vec::new();
        let mut recursion = 0i32;
        match fuzzy_match_recursive(w, str, 0, str, camelcase, &mut matches, &mut recursion) {
            Some(score) => {
                total += score;
                all.extend_from_slice(&matches);
            }
            None => return None,
        }
    }
    Some((total, all))
}

/// Port of `do_fuzzymatch()` (Neovim search.c) — the shared body of
/// `matchfuzzy()`/`matchfuzzypos()`. Scores every list item (a String, or a
/// Dict field named by the `key` option), sorts by descending score (stable on
/// input order for ties), applies the `limit`/`matchseq`/`camelcase` options,
/// and returns either the filtered items or `[items, positions, scores]`.
fn do_fuzzymatch(argvars: &[typval_T], rettv: &mut typval_T, return_pos: bool) {
    let l = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => l.clone(),
        _ => {
            emsg("E714: List required");
            return;
        }
    };
    let pat = tv_get_string(&argvars[1]);
    // c: third argument is an optional options Dict.
    let mut key: Option<String> = None;
    let mut matchseq = false;
    let mut camelcase = true;
    let mut limit: i64 = 0;
    if argvars.len() >= 3 {
        if let (VAR_DICT, v_dict(Some(d))) = (argvars[2].v_type, &argvars[2].vval) {
            let d = d.borrow();
            if let Some(k) = tv_dict_find(&d, "key") {
                key = Some(tv_get_string(k));
            }
            if let Some(v) = tv_dict_find(&d, "matchseq") {
                matchseq = tv_get_number(v) != 0;
            }
            if let Some(v) = tv_dict_find(&d, "camelcase") {
                camelcase = tv_get_bool(v) != 0;
            }
            if let Some(v) = tv_dict_find(&d, "limit") {
                limit = tv_get_number(v);
            }
        }
    }
    let items: Vec<typval_T> = l
        .borrow()
        .lv_items
        .iter()
        .map(|it| it.li_tv.clone())
        .collect();
    let mut scored: Vec<(usize, i32, Vec<usize>)> = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        // c: extract the text to match — the item itself (String) or item[key].
        let text = match &key {
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
        let chars: Vec<char> = text.chars().collect();
        if let Some((score, positions)) = fuzzy_match(&chars, &pat, matchseq, camelcase) {
            scored.push((idx, score, positions));
        }
    }
    // c: sort by descending score; stable so equal scores keep input order.
    scored.sort_by_key(|x| std::cmp::Reverse(x.1));
    if limit > 0 && scored.len() > limit as usize {
        scored.truncate(limit as usize);
    }
    if !return_pos {
        let out = tv_list_alloc_ret(rettv, scored.len() as isize);
        let mut ob = out.borrow_mut();
        for (idx, _, _) in &scored {
            tv_list_append_tv(&mut ob, items[*idx].clone());
        }
        return;
    }
    // c: matchfuzzypos() returns [matched_items, positions, scores].
    let outer = tv_list_alloc_ret(rettv, 3);
    let matched = tv_list_alloc(0);
    let posl = tv_list_alloc(0);
    let scorel = tv_list_alloc(0);
    for (idx, score, positions) in &scored {
        tv_list_append_tv(&mut matched.borrow_mut(), items[*idx].clone());
        let p = tv_list_alloc(0);
        for pos in positions {
            tv_list_append_number(&mut p.borrow_mut(), *pos as varnumber_T);
        }
        tv_list_append_tv(
            &mut posl.borrow_mut(),
            typval_T {
                v_type: VAR_LIST,
                v_lock: VarLockStatus::VAR_UNLOCKED,
                vval: v_list(Some(p)),
            },
        );
        tv_list_append_number(&mut scorel.borrow_mut(), *score as varnumber_T);
    }
    let mk = |l| typval_T {
        v_type: VAR_LIST,
        v_lock: VarLockStatus::VAR_UNLOCKED,
        vval: v_list(Some(l)),
    };
    let mut ob = outer.borrow_mut();
    tv_list_append_tv(&mut ob, mk(matched));
    tv_list_append_tv(&mut ob, mk(posl));
    tv_list_append_tv(&mut ob, mk(scorel));
}

/// Port of `f_matchfuzzy()` (Neovim search.c) — fuzzy-filter a List by a
/// pattern, best matches first.
pub fn f_matchfuzzy(argvars: &[typval_T], rettv: &mut typval_T) {
    do_fuzzymatch(argvars, rettv, false);
}

/// Port of `f_matchfuzzypos()` (Neovim search.c) — like `matchfuzzy()` but
/// returns `[items, match-positions, scores]`.
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

// ── argc()/argv()/argidx() — eval/funcs.c (full table). vimlrs runs a single
//    script with no editor argument list, so the arglist is empty. ──

/// Port of `f_argc()` (Neovim eval/funcs.c) — the size of the argument list (0,
/// no editor arglist when standalone).
pub fn f_argc(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(0);
}

/// Port of `f_argidx()` (Neovim eval/funcs.c) — the current index in the
/// argument list (0 when standalone).
pub fn f_argidx(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(0);
}

/// Port of `f_argv()` (Neovim eval/funcs.c) — with no/`-1` index, the whole
/// (empty) argument list as a List; with an index, that entry as a String ("").
pub fn f_argv(argvars: &[typval_T], rettv: &mut typval_T) {
    if argvars.is_empty() || tv_get_number(&argvars[0]) == -1 {
        let _ = tv_list_alloc_ret(rettv, 0);
        return;
    }
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(String::new());
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

// ── arglistid() — eval/funcs.c (full table); foldlevel() — fold.c. ──

/// Port of `f_arglistid()` (Neovim eval/funcs.c) — the id of the argument list
/// of the (optionally specified) window. vimlrs runs a single script with one
/// global, unnamed argument list, so the id is always 0.
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
// outside the vendored csrc/eval/ tree, so recorded in the drift-gate
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
    let cur = CURPOS.with(|c| c.borrow().0);
    let (mut lnum, had) = if i < b.len() && b[i] == b'.' {
        i += 1;
        (cur, true)
    } else if i < b.len() && b[i] == b'$' {
        i += 1;
        (last, true)
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
    let cur = CURPOS.with(|c| c.borrow().0);
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
        _ => ExCmdResult::NotEx,
    }
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

// ════════════════════════════════════════════════════════════════════════════
// Round-3 builtin expansion. Command-line state (ex_getln.c), sign placement
// (sign.c), and a set of editor-feature queries whose answer is well-defined
// when no editor is attached (indent.c / fold.c / highlight.c / diff.c /
// search.c / popupmenu / cmdexpand). All outside the vendored csrc/eval/ tree,
// so recorded in the drift-gate allowlist.
// ════════════════════════════════════════════════════════════════════════════

// ── setcmdline()/getcmdline()/setcmdpos()/getcmdpos()/getcmdtype() —
//    Neovim ex_getln.c. Standalone we model a settable command-line buffer. ──

thread_local! {
    /// The command-line buffer state (`ccline` in ex_getln.c): `(line, pos,
    /// type)`. `pos` is the 1-based byte position of the cursor.
    static CMDLINE: std::cell::RefCell<(String, varnumber_T, String)> =
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
/// group `{name}` is defined. No highlight groups standalone → 0.
pub fn f_highlight_exists(_argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(0);
}

/// Port of `f_diff_filler()` (Neovim diff.c) — the number of filler lines above
/// line `{lnum}`. No diff mode standalone → 0.
pub fn f_diff_filler(_argvars: &[typval_T], rettv: &mut typval_T) {
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
// documented "no editor" values. All outside the vendored csrc/eval/ tree.
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
// the vendored csrc/eval/ tree.
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
