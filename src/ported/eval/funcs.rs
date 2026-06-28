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
    let present = crate::ported::eval::vars::eval_variable(&name).is_some();
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

/// Port of `f_getpos()`/`getpos_both(…,false,false)` — no cursor → `[0,0,0,0]`.
pub fn f_getpos(_argvars: &[typval_T], rettv: &mut typval_T) {
    getpos_both(rettv, false);
}
/// Port of `f_getcharpos()`/`getpos_both(…,false,true)` — no cursor → `[0,0,0,0]`.
pub fn f_getcharpos(_argvars: &[typval_T], rettv: &mut typval_T) {
    getpos_both(rettv, false);
}
/// Port of `f_getcurpos()`/`getpos_both(…,true,false)` — no cursor →
/// `[0,0,0,0,0]` (the 5th element is `curswant`).
pub fn f_getcurpos(_argvars: &[typval_T], rettv: &mut typval_T) {
    getpos_both(rettv, true);
}
/// Port of `f_getcursorcharpos()`/`getpos_both(…,true,true)` — `[0,0,0,0,0]`.
pub fn f_getcursorcharpos(_argvars: &[typval_T], rettv: &mut typval_T) {
    getpos_both(rettv, true);
}
/// Port of `f_col()`/`get_col(…,false)` — no cursor column → 0.
pub fn f_col(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_charcol()`/`get_col(…,true)` — no cursor column → 0.
pub fn f_charcol(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_line()` — no cursor line → 0.
pub fn f_line(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
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
/// Port of `f_line2byte()` — no buffer → -1.
pub fn f_line2byte(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
}
/// Port of `f_byte2line()` — no buffer → -1.
pub fn f_byte2line(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
}
/// Port of `f_nextnonblank()` — no buffer lines → 0.
pub fn f_nextnonblank(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_prevnonblank()` — no buffer lines → 0.
pub fn f_prevnonblank(_argvars: &[typval_T], rettv: &mut typval_T) {
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

/// Port of `f_search()`/`search_cmn()` — pattern not found (no buffer) → 0.
pub fn f_search(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_searchpos()` — not found → `[0, 0]`.
pub fn f_searchpos(_argvars: &[typval_T], rettv: &mut typval_T) {
    let l = tv_list_alloc_ret(rettv, 2);
    let mut lb = l.borrow_mut();
    tv_list_append_number(&mut lb, 0);
    tv_list_append_number(&mut lb, 0);
}
/// Port of `f_searchpair()`/`searchpair_cmn()` — not found → 0.
pub fn f_searchpair(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(0 as varnumber_T);
}
/// Port of `f_searchpairpos()` — not found → `[0, 0]`.
pub fn f_searchpairpos(_argvars: &[typval_T], rettv: &mut typval_T) {
    let l = tv_list_alloc_ret(rettv, 2);
    let mut lb = l.borrow_mut();
    tv_list_append_number(&mut lb, 0);
    tv_list_append_number(&mut lb, 0);
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
pub fn f_setpos(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
}
/// Port of `f_setcharpos()`/`set_position(…,true)` — no buffer/window → -1.
pub fn f_setcharpos(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
}
/// Port of `f_cursor()`/`set_cursorpos(…,false)` — no window to move → -1.
pub fn f_cursor(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
}
/// Port of `f_setcursorcharpos()`/`set_cursorpos(…,true)` — no window → -1.
pub fn f_setcursorcharpos(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(-1 as varnumber_T);
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
    if argvars.len() >= 2 && argvars[1].v_type != VAR_UNKNOWN {
        tv_list_alloc_ret(rettv, 0);
    } else {
        *rettv = typval_T::from(String::new());
    }
}
/// Port of `f_getbufline()`/`getbufline()` (buffer.c) — always a List → empty.
pub fn f_getbufline(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_list_alloc_ret(rettv, 0);
}
/// Port of `f_getbufoneline()` (buffer.c) — no buffer line → "".
pub fn f_getbufoneline(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(String::new());
}
/// Port of `f_getbufinfo()` (buffer.c) — no buffers → empty List.
pub fn f_getbufinfo(_argvars: &[typval_T], rettv: &mut typval_T) {
    tv_list_alloc_ret(rettv, 0);
}
/// Port of `f_setline()`/`set_buffer_lines()` (buffer.c) — no buffer → 1 (FAIL).
pub fn f_setline(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(1 as varnumber_T);
}
/// Port of `f_setbufline()` (buffer.c) — no buffer → 1 (FAIL).
pub fn f_setbufline(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(1 as varnumber_T);
}
/// Port of `f_append()` (buffer.c) — no buffer → 1 (FAIL).
pub fn f_append(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(1 as varnumber_T);
}
/// Port of `f_appendbufline()` (buffer.c) — no buffer → 1 (FAIL).
pub fn f_appendbufline(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(1 as varnumber_T);
}
/// Port of `f_deletebufline()` (buffer.c) — no buffer → 1 (FAIL default).
pub fn f_deletebufline(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = typval_T::from(1 as varnumber_T);
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

/// Port of `f_do_searchpair()`/`do_searchpair()` helper (funcs.c) — searching
/// for a matching pair needs a buffer; standalone has none → not found (0).
pub fn do_searchpair() -> varnumber_T {
    0
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
