//! Port of `src/nvim/eval/funcs.c` (vendored at `csrc/eval/funcs.c`).
//!
//! Vimscript builtin functions. Each `f_<name>` matches the C signature
//! `void f_<name>(typval_T *argvars, typval_T *rettv, EvalFuncData fptr)`,
//! reduced to `(argvars, rettv)` (the `fptr` carries no data for these). As in
//! C, the caller (`call_func`) pre-initializes `rettv` to `VAR_NUMBER`/0 before
//! the call, so a numeric function only assigns `rettv->vval.v_number`; only
//! functions returning another type set `v_type`. Phase 3 ports a subset.
#![allow(non_snake_case)]

use crate::ported::eval::typval::{
    tv_blob_len, tv_get_bool, tv_dict_add_tv, tv_dict_find, tv_dict_len, tv_equal, tv_get_float,
    tv_get_number_chk, tv_get_string, tv_list_alloc_ret, tv_list_append_number,
    tv_list_append_string, tv_list_append_tv, tv_list_len,
};
use crate::ported::eval::typval_defs_h::{
    typval_T, typval_vval_union::*, varnumber_T, BoolVarValue::*, SpecialVarValue::*, VarType::*,
    VAR_TYPE_BLOB,
    VAR_TYPE_BOOL, VAR_TYPE_DICT, VAR_TYPE_FLOAT, VAR_TYPE_FUNC, VAR_TYPE_LIST, VAR_TYPE_NUMBER,
    VAR_TYPE_SPECIAL, VAR_TYPE_STRING,
};
use crate::ported::message::emsg;
use crate::ported::option::get_option_value;

/// Port of `f_len()` from `Src/eval/funcs.c`.
///
/// "len()" function — length of a String/List/Dict/Blob (or the decimal width
/// of a Number). `rettv` is pre-set to `VAR_NUMBER`.
pub fn f_len(argvars: &[typval_T], rettv: &mut typval_T) {
    let arg = &argvars[0];
    // c: switch (argvars[0].v_type) { ... rettv->vval.v_number = ...; }
    rettv.vval = match (arg.v_type, &arg.vval) {
        (VAR_STRING, v_string(s)) => v_number(s.chars().count() as varnumber_T),
        (VAR_NUMBER, _) | (VAR_FLOAT, _) => {
            v_number(tv_get_string(arg).chars().count() as varnumber_T)
        }
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
        (VAR_DICT, v_dict(d)) => d.as_ref().map_or(true, |d| d.borrow().dv_hashtab.is_empty()),
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
    let s = tv_get_string(&argvars[0]);
    rettv.v_type = VAR_FLOAT;
    rettv.vval = v_float(s.trim().parse::<f64>().unwrap_or(0.0));
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
    rettv.v_type = VAR_FUNC;
    rettv.vval = v_string(tv_get_string(&argvars[0]));
}




/// Port of `f_char2nr()` from `Src/eval/funcs.c` — code point of the first char.
pub fn f_char2nr(argvars: &[typval_T], rettv: &mut typval_T) {
    let n = tv_get_string(&argvars[0]).chars().next().map_or(0, |c| c as varnumber_T);
    rettv.vval = v_number(n);
}

/// Port of `f_nr2char()` from `Src/eval/funcs.c` — char for a code point.
pub fn f_nr2char(argvars: &[typval_T], rettv: &mut typval_T) {
    let n = tv_get_number_chk(&argvars[0], None);
    let s = char::from_u32(n as u32).map(String::from).unwrap_or_default();
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
    let keepempty = argvars.get(2).map_or(false, |t| tv_get_number_chk(t, None) != 0);
    let pat = argvars.get(1).map(tv_get_string).filter(|p| !p.is_empty());
    let parts: Vec<String> = match pat {
        Some(p) => crate::viml_regex::regex_split(&s, &p, tv_get_bool(&get_option_value("ignorecase")) != 0, keepempty),
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
    rettv.vval = v_string(crate::viml_regex::regex_matchstr(&pat, &s, tv_get_bool(&get_option_value("ignorecase")) != 0));
}

/// Port of `f_match()` from `Src/eval/funcs.c` — the char index of the first
/// match of `{pat}` in `{expr}`, or -1.
pub fn f_match(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let pat = tv_get_string(&argvars[1]);
    rettv.vval = v_number(crate::viml_regex::regex_match_index(&pat, &s, tv_get_bool(&get_option_value("ignorecase")) != 0));
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
pub fn f_join(argvars: &[typval_T], rettv: &mut typval_T) {
    let sep = if argvars.len() >= 2 {
        tv_get_string(&argvars[1])
    } else {
        " ".to_string()
    };
    rettv.v_type = VAR_STRING;
    let out = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => l
            .borrow()
            .lv_items
            .iter()
            .map(|it| tv_get_string(&it.li_tv))
            .collect::<Vec<_>>()
            .join(&sep),
        _ => String::new(),
    };
    rettv.vval = v_string(out);
}

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

/// Port of `max_min()` from `Src/eval/funcs.c` (List subset) — the shared
/// `max`/`min` body; `domax` picks the direction. Empty → 0.
fn max_min(argvars: &[typval_T], rettv: &mut typval_T, domax: bool) {
    let n = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => {
            let l = l.borrow();
            let mut it = l.lv_items.iter();
            match it.next() {
                Some(first) => it.fold(tv_get_number_chk(&first.li_tv, None), |acc, x| {
                    let v = tv_get_number_chk(&x.li_tv, None);
                    if domax {
                        acc.max(v)
                    } else {
                        acc.min(v)
                    }
                }),
                None => 0,
            }
        }
        _ => 0,
    };
    rettv.vval = v_number(n);
}

/// Port of `f_count()` from `Src/eval/funcs.c` (subset) — occurrences of
/// `{expr}` in a List.
pub fn f_count(argvars: &[typval_T], rettv: &mut typval_T) {
    let needle = &argvars[1];
    let n = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => l
            .borrow()
            .lv_items
            .iter()
            .filter(|it| crate::ported::eval::typval::tv_equal(&it.li_tv, needle, false))
            .count() as varnumber_T,
        _ => 0,
    };
    rettv.vval = v_number(n);
}

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
        while i < bytes.len() && matches!(bytes[i], '-' | '0' | '+' | ' ' | '#') {
            match bytes[i] {
                '-' => left = true,
                '0' => zero = true,
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
            other => {
                out.push('%');
                out.push(other);
                continue;
            }
        };
        arg += 1;
        // Pad to width.
        if core.chars().count() >= width {
            out.push_str(&core);
        } else {
            let pad = width - core.chars().count();
            if left {
                out.push_str(&core);
                out.extend(std::iter::repeat(' ').take(pad));
            } else if zero && conv != 's' {
                out.extend(std::iter::repeat('0').take(pad));
                out.push_str(&core);
            } else {
                out.extend(std::iter::repeat(' ').take(pad));
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
    rettv.vval = v_number(tv_get_number_chk(&argvars[0], None) & tv_get_number_chk(&argvars[1], None));
}
/// Port of `f_or()` from `Src/eval/funcs.c` — bitwise OR.
pub fn f_or(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(tv_get_number_chk(&argvars[0], None) | tv_get_number_chk(&argvars[1], None));
}
/// Port of `f_xor()` from `Src/eval/funcs.c` — bitwise XOR.
pub fn f_xor(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(tv_get_number_chk(&argvars[0], None) ^ tv_get_number_chk(&argvars[1], None));
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
pub fn f_remove(argvars: &[typval_T], rettv: &mut typval_T) {
    match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => {
            let mut lb = l.borrow_mut();
            let len = lb.lv_len as varnumber_T;
            let mut idx = tv_get_number_chk(&argvars[1], None);
            if idx < 0 {
                idx += len;
            }
            if idx >= 0 && (idx as usize) < lb.lv_items.len() {
                let it = lb.lv_items.remove(idx as usize);
                lb.lv_len = lb.lv_items.len() as i32;
                *rettv = it.li_tv;
            }
        }
        (VAR_DICT, v_dict(Some(d))) => {
            let key = tv_get_string(&argvars[1]);
            if let Some(v) = d.borrow_mut().dv_hashtab.shift_remove(&key) {
                *rettv = v;
            }
        }
        _ => {}
    }
}

/// Port of `f_extend()` from `Src/eval/funcs.c` (subset) — append `{expr2}`'s
/// items to a `{list}`, or merge a `{dict}`'s entries, returning the first.
pub fn f_extend(argvars: &[typval_T], rettv: &mut typval_T) {
    match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l1))) => {
            if let (VAR_LIST, v_list(Some(l2))) = (argvars[1].v_type, &argvars[1].vval) {
                let add: Vec<_> = l2.borrow().lv_items.iter().map(|it| it.li_tv.clone()).collect();
                let mut lb = l1.borrow_mut();
                for tv in add {
                    tv_list_append_tv(&mut lb, tv);
                }
            }
        }
        (VAR_DICT, v_dict(Some(d1))) => {
            if let (VAR_DICT, v_dict(Some(d2))) = (argvars[1].v_type, &argvars[1].vval) {
                let pairs: Vec<_> = d2
                    .borrow()
                    .dv_hashtab
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let mut db = d1.borrow_mut();
                for (k, v) in pairs {
                    tv_dict_add_tv(&mut db, &k, v);
                }
            }
        }
        _ => {}
    }
    *rettv = argvars[0].clone();
}

/// Port of `f_copy()` from `Src/eval/funcs.c` — a shallow copy of `{expr}`.
pub fn f_copy(argvars: &[typval_T], rettv: &mut typval_T) {
    match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => {
            let items: Vec<_> = l.borrow().lv_items.iter().map(|it| it.li_tv.clone()).collect();
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
    use crate::ported::eval::typval::{tv_blob2items, tv_dict2items, tv_list2items, tv_string2items};
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
pub fn f_uniq(argvars: &[typval_T], rettv: &mut typval_T) {
    if let (VAR_LIST, v_list(Some(l))) = (argvars[0].v_type, &argvars[0].vval) {
        let mut lb = l.borrow_mut();
        lb.lv_items.dedup_by(|a, b| tv_equal(&a.li_tv, &b.li_tv, false));
        lb.lv_len = lb.lv_items.len() as i32;
    }
    *rettv = argvars[0].clone();
}

// ── batch 4: regex-list, more string, list helpers (Src/eval/funcs.c) ──

/// Port of `f_matchlist()` from `Src/eval/funcs.c` — `[whole, sub1, …]` of the
/// first match of `{pat}` in `{expr}`.
pub fn f_matchlist(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let pat = tv_get_string(&argvars[1]);
    let parts = crate::viml_regex::regex_matchlist(&pat, &s, tv_get_bool(&get_option_value("ignorecase")) != 0);
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
    rettv.vval = v_number(crate::viml_regex::regex_matchend(&pat, &s, tv_get_bool(&get_option_value("ignorecase")) != 0));
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

/// Port of `f_flatten()` from `Src/eval/funcs.c` — flatten a nested List up to
/// `{maxdepth}` (default: fully), mutating it in place and returning it.
pub fn f_flatten(argvars: &[typval_T], rettv: &mut typval_T) {
    let maxdepth = argvars.get(1).map_or(varnumber_T::MAX, |t| tv_get_number_chk(t, None));
    if let (VAR_LIST, v_list(Some(l))) = (argvars[0].v_type, &argvars[0].vval) {
        // Iterative DFS (a private recursive helper would be a non-C name).
        let top: Vec<typval_T> = l.borrow().lv_items.iter().map(|it| it.li_tv.clone()).collect();
        let mut stack: Vec<(typval_T, varnumber_T)> =
            top.into_iter().rev().map(|tv| (tv, maxdepth)).collect();
        let mut out = Vec::new();
        while let Some((tv, depth)) = stack.pop() {
            if depth > 0 {
                if let (VAR_LIST, v_list(Some(inner))) = (tv.v_type, &tv.vval) {
                    let items: Vec<typval_T> =
                        inner.borrow().lv_items.iter().map(|it| it.li_tv.clone()).collect();
                    for it in items.into_iter().rev() {
                        stack.push((it, depth - 1));
                    }
                    continue;
                }
            }
            out.push(tv);
        }
        let mut lb = l.borrow_mut();
        lb.lv_len = out.len() as i32;
        lb.lv_items = out
            .into_iter()
            .map(|li_tv| crate::ported::eval::typval_defs_h::listitem_T { li_tv })
            .collect();
    }
    *rettv = argvars[0].clone();
}

// ── batch 5: deepcopy + more float math (Src/eval/funcs.c) ──

/// Port of `var_item_copy()` from `Src/eval/typval.c` — a deep copy of `from`
/// (Lists/Dicts copied recursively into fresh handles).
pub(crate) fn var_item_copy(from: &typval_T) -> typval_T {
    match (from.v_type, &from.vval) {
        (VAR_LIST, v_list(Some(l))) => {
            let items: Vec<typval_T> =
                l.borrow().lv_items.iter().map(|it| var_item_copy(&it.li_tv)).collect();
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

/// Port of `f_extendnew()` from `Src/eval/list.c` — like `extend()` but returns
/// a NEW List/Dict, leaving the arguments unchanged. (c: `extend(..., true)`.)
pub fn f_extendnew(argvars: &[typval_T], rettv: &mut typval_T) {
    match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l1))) => {
            let mut items: Vec<_> =
                l1.borrow().lv_items.iter().map(|it| it.li_tv.clone()).collect();
            if let (VAR_LIST, v_list(Some(l2))) = (argvars[1].v_type, &argvars[1].vval) {
                items.extend(l2.borrow().lv_items.iter().map(|it| it.li_tv.clone()));
            }
            let out = tv_list_alloc_ret(rettv, items.len() as isize);
            let mut ob = out.borrow_mut();
            for tv in items {
                tv_list_append_tv(&mut ob, tv);
            }
        }
        (VAR_DICT, v_dict(Some(d1))) => {
            let mut pairs: Vec<_> = d1
                .borrow()
                .dv_hashtab
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            if let (VAR_DICT, v_dict(Some(d2))) = (argvars[1].v_type, &argvars[1].vval) {
                pairs.extend(d2.borrow().dv_hashtab.iter().map(|(k, v)| (k.clone(), v.clone())));
            }
            let out = crate::ported::eval::typval::tv_dict_alloc_ret(rettv);
            let mut ob = out.borrow_mut();
            for (k, v) in pairs {
                tv_dict_add_tv(&mut ob, &k, v); // later (expr2) keys override
            }
        }
        _ => *rettv = argvars[0].clone(),
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

