//! Port of `src/nvim/eval/list.c` (vendored at `csrc/eval/list.c`).
//!
//! List/Dict/String builtins whose bodies live in `list.c`. The callback-driven
//! ones (`map`/`filter`/`foreach`) are orchestrated bridge-side; the pure
//! counting helpers are ported here.
#![allow(non_snake_case)]

use crate::ported::eval::typval::{
    tv_dict_copy, tv_dict_extend, tv_equal, tv_get_number_chk, tv_get_string_chk, tv_list_copy,
    tv_list_extend, tv_list_find, tv_list_len,
};
use crate::ported::eval::typval_defs_h::{
    dict_T, list_T, typval_T, typval_vval_union::*, varnumber_T, VarType::*,
};
use crate::ported::message::emsg;

/// Port of `count_string()` from `Src/eval/list.c:459`.
///
/// Count non-overlapping occurrences of `needle` in `haystack` (case-insensitive
/// when `ic`). The C advances by the needle length on a match and one char on a
/// miss; non-overlapping find/skip is equivalent.
fn count_string(haystack: &str, needle: &str, ic: bool) -> varnumber_T {
    // c: if (p == NULL || needle == NULL || *needle == NUL) return 0;
    if needle.is_empty() {
        return 0;
    }
    let mut n: varnumber_T = 0;
    if ic {
        // c: mb_strnicmp at each position — case-fold both, then count.
        let hay = haystack.to_lowercase();
        let need = needle.to_lowercase();
        let mut rest = hay.as_str();
        while let Some(pos) = rest.find(&need) {
            n += 1;
            rest = &rest[pos + need.len()..];
        }
    } else {
        // c: while ((next = strstr(p, needle)) != NULL) { n++; p = next + needlelen; }
        let mut rest = haystack;
        while let Some(pos) = rest.find(needle) {
            n += 1;
            rest = &rest[pos + needle.len()..];
        }
    }
    n
}

/// Port of `count_list()` from `Src/eval/list.c:492`.
///
/// Count items equal to `needle`, starting at index `idx`.
fn count_list(l: &list_T, needle: &typval_T, idx: i64, ic: bool) -> varnumber_T {
    // c: if (tv_list_len(l) == 0) return 0;
    if tv_list_len(l) == 0 {
        return 0;
    }
    // c: li = tv_list_find(l, idx); if (li == NULL) { semsg(e_list_index_out_of_range_nr, idx); return 0; }
    if tv_list_find(l, idx as i32).is_none() {
        emsg(&format!("E684: List index out of range: {idx}"));
        return 0;
    }
    let start = if idx < 0 { tv_list_len(l) as i64 + idx } else { idx } as usize;
    // c: for (; li != NULL; li = NEXT) if (tv_equal(li, needle, ic)) n++;
    let mut n: varnumber_T = 0;
    for li in l.lv_items.iter().skip(start) {
        if tv_equal(&li.li_tv, needle, ic) {
            n += 1;
        }
    }
    n
}

/// Port of `count_dict()` from `Src/eval/list.c:518`.
///
/// Count values equal to `needle`.
fn count_dict(d: &dict_T, needle: &typval_T, ic: bool) -> varnumber_T {
    // c: TV_DICT_ITER(d, di, { if (tv_equal(&di->di_tv, needle, ic)) n++; });
    let mut n: varnumber_T = 0;
    for (_k, v) in d.dv_hashtab.iter() {
        if tv_equal(v, needle, ic) {
            n += 1;
        }
    }
    n
}

/// Port of `f_count()` from `Src/eval/list.c:536`.
///
/// "count()" function — occurrences of `argvars[1]` in a String/List/Dict, with
/// optional case-insensitivity `argvars[2]` and (List only) start index
/// `argvars[3]`.
pub fn f_count(argvars: &[typval_T], rettv: &mut typval_T) {
    let mut n: varnumber_T = 0;
    let mut ic = 0;
    let mut error = false;

    // c: if (argvars[2].v_type != VAR_UNKNOWN) ic = tv_get_number_chk(&argvars[2], &error);
    if argvars.len() > 2 {
        ic = tv_get_number_chk(&argvars[2], Some(&mut error));
    }

    if !error {
        match (argvars[0].v_type, &argvars[0].vval) {
            (VAR_STRING, v_string(s)) => {
                let needle = tv_get_string_chk(&argvars[1]).unwrap_or_default();
                n = count_string(s, &needle, ic != 0);
            }
            (VAR_LIST, v_list(Some(l))) => {
                // c: idx defaults 0; set from argvars[3] only when [2] and [3] are present.
                let mut idx = 0i64;
                if argvars.len() > 3 {
                    idx = tv_get_number_chk(&argvars[3], Some(&mut error));
                }
                if !error {
                    n = count_list(&l.borrow(), &argvars[1], idx, ic != 0);
                }
            }
            (VAR_DICT, v_dict(Some(d))) => {
                // c: a start index makes no sense for a Dict → E474.
                if argvars.len() > 3 {
                    emsg("E474: Invalid argument");
                } else {
                    n = count_dict(&d.borrow(), &argvars[1], ic != 0);
                }
            }
            (VAR_LIST, _) | (VAR_DICT, _) => {} // NULL list/dict → 0
            _ => {
                emsg("E706: Argument of count() must be a List, String or Dictionary");
            }
        }
    }
    rettv.vval = v_number(n);
}

/// Port of `extend_list()` from `Src/eval/list.c:649`.
///
/// Append `argvars[1]`'s items to the `argvars[0]` list (at index `argvars[2]`,
/// if given), returning the result. `is_new` (extendnew) works on a copy. Lock
/// checks are skipped (locks are unmodeled).
fn extend_list(argvars: &[typval_T], is_new: bool, rettv: &mut typval_T) {
    let mut error = false;
    // c: l1 = argvars[0].vval.v_list;  (is_new → a fresh copy)
    let l1_orig = match &argvars[0].vval {
        v_list(Some(l)) => l.clone(),
        _ => return,
    };
    let l1 = if is_new { tv_list_copy(&l1_orig, false) } else { l1_orig };

    // c: before-index (argvars[2]): NULL ⇒ append; `before == len` ⇒ append;
    //    out of range ⇒ E684.
    let bef: Option<usize> = if argvars.len() > 2 {
        let before = tv_get_number_chk(&argvars[2], Some(&mut error)) as i32;
        if error {
            return;
        }
        let len = tv_list_len(&l1.borrow());
        if before == len {
            None
        } else if tv_list_find(&l1.borrow(), before).is_none() {
            emsg(&format!("E684: List index out of range: {before}"));
            return;
        } else {
            Some(if before < 0 { (len + before) as usize } else { before as usize })
        }
    } else {
        None
    };

    // c: tv_list_extend(l1, l2, item). Snapshot l2 first so `extend(a, a)` does
    // not alias l1's RefCell.
    if let v_list(Some(l2)) = &argvars[1].vval {
        let snap = tv_list_copy(l2, false);
        tv_list_extend(&mut l1.borrow_mut(), &snap.borrow(), bef);
    }

    if is_new {
        rettv.v_type = VAR_LIST;
        rettv.vval = v_list(Some(l1));
    } else {
        *rettv = argvars[0].clone();
    }
}

/// Port of `extend_dict()` from `Src/eval/list.c:578`.
///
/// Merge `argvars[1]`'s entries into the `argvars[0]` dict using action
/// `argvars[2]` (`keep`/`force`/`error`, default `force`), returning the result.
fn extend_dict(argvars: &[typval_T], is_new: bool, rettv: &mut typval_T) {
    let d1_orig = match &argvars[0].vval {
        v_dict(Some(d)) => d.clone(),
        _ => return,
    };
    // c: d2 == NULL → do nothing, tv_copy(argvars[0], rettv).
    let d2 = match &argvars[1].vval {
        v_dict(Some(d)) => d.clone(),
        _ => {
            *rettv = argvars[0].clone();
            return;
        }
    };
    let d1 = if is_new { tv_dict_copy(&d1_orig, false) } else { d1_orig };

    // c: action default "force"; validate against {keep,force,error} → E475.
    let mut action = String::from("force");
    if argvars.len() > 2 {
        match tv_get_string_chk(&argvars[2]) {
            None => return, // type error; message already given
            Some(a) => {
                if a != "keep" && a != "force" && a != "error" {
                    emsg(&format!("E475: Invalid argument: {a}"));
                    return;
                }
                action = a;
            }
        }
    }

    // c: tv_dict_extend(d1, d2, action). Snapshot d2 (aliasing-safe).
    let snap = tv_dict_copy(&d2, false);
    tv_dict_extend(&mut d1.borrow_mut(), &snap.borrow(), &action);

    if is_new {
        rettv.v_type = VAR_DICT;
        rettv.vval = v_dict(Some(d1));
    } else {
        *rettv = argvars[0].clone();
    }
}

/// Port of `extend()` from `Src/eval/list.c:707` — the `extend`/`extendnew`
/// dispatcher.
fn extend(argvars: &[typval_T], rettv: &mut typval_T, is_new: bool) {
    if argvars[0].v_type == VAR_LIST && argvars[1].v_type == VAR_LIST {
        extend_list(argvars, is_new, rettv);
    } else if argvars[0].v_type == VAR_DICT && argvars[1].v_type == VAR_DICT {
        extend_dict(argvars, is_new, rettv);
    } else {
        // c: semsg(e_listdictarg, is_new ? "extendnew()" : "extend()");
        let name = if is_new { "extendnew" } else { "extend" };
        emsg(&format!("E712: Argument of {name}() must be a List or Dictionary"));
    }
}

/// Port of `f_extend()` from `Src/eval/list.c:720`.
pub fn f_extend(argvars: &[typval_T], rettv: &mut typval_T) {
    extend(argvars, rettv, false);
}

/// Port of `f_extendnew()` from `Src/eval/list.c:728`.
pub fn f_extendnew(argvars: &[typval_T], rettv: &mut typval_T) {
    extend(argvars, rettv, true);
}
