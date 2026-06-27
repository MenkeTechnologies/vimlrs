//! Port of `src/nvim/eval/list.c` (vendored at `csrc/eval/list.c`).
//!
//! List/Dict/String builtins whose bodies live in `list.c`, including the
//! callback-driven `map`/`filter`/`mapnew`/`foreach` (the per-item evaluation
//! crosses into the bridge via `FILTER_MAP_EVAL_HOOK`/`FILTER_MAP_CMD_HOOK`).
#![allow(non_snake_case, non_camel_case_types)]

use std::cell::RefCell;
use std::rc::Rc;

use crate::ported::eval::typval::{
    tv_blob_alloc, tv_blob_get, tv_blob_len, tv_blob_remove, tv_blob_set, tv_dict_add_tv,
    tv_dict_alloc_ret, tv_dict_copy, tv_dict_extend, tv_dict_remove, tv_equal, tv_get_number_chk,
    tv_get_string, tv_get_string_chk, tv_list_alloc_ret, tv_list_copy, tv_list_extend,
    tv_list_find, tv_list_len, tv_list_remove, value_check_lock, TV_TRANSLATE,
};
use crate::ported::eval::typval_defs_h::{
    blob_T, dict_T, list_T, listitem_T, typval_T, typval_vval_union::*, varnumber_T, VarLockStatus,
    VarType::*,
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
    let start = if idx < 0 {
        tv_list_len(l) as i64 + idx
    } else {
        idx
    } as usize;
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
    let l1 = if is_new {
        tv_list_copy(&l1_orig, false)
    } else {
        l1_orig
    };

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
            Some(if before < 0 {
                (len + before) as usize
            } else {
                before as usize
            })
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
    let d1 = if is_new {
        tv_dict_copy(&d1_orig, false)
    } else {
        d1_orig
    };

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
        emsg(&format!(
            "E712: Argument of {name}() must be a List or Dictionary"
        ));
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

// ── map() / filter() / mapnew() / foreach() (Src/eval/list.c) ──

/// Port of `filtermap_T` from `Src/eval/list.c:16`.
#[derive(Clone, Copy, PartialEq)]
pub enum filtermap_T {
    FILTERMAP_FILTER,
    FILTERMAP_MAP,
    FILTERMAP_MAPNEW,
    FILTERMAP_FOREACH,
}
use filtermap_T::*;

thread_local! {
    /// Per-item evaluator for map/filter/foreach: set `v:key`/`v:val`, then eval
    /// the expr (string) or call the funcref → result. Installed by the bridge
    /// in `install()` (the value layer cannot evaluate expressions itself).
    pub static FILTER_MAP_EVAL_HOOK: RefCell<Option<fn(&typval_T, &typval_T, &typval_T) -> Option<typval_T>>> =
        const { RefCell::new(None) };
    /// `foreach()` with a String runs it as a command line (`do_cmdline_cmd`).
    /// Installed by the bridge.
    pub static FILTER_MAP_CMD_HOOK: RefCell<Option<fn(&str, &typval_T, &typval_T) -> bool>> =
        const { RefCell::new(None) };
}

/// Port of `filter_map_one()` from `Src/eval/list.c:37`.
///
/// Apply `expr` to one item (`v:val`); returns `(newtv, rem)` or `None` on
/// failure. For `filter()`, `rem` says drop the item. `foreach()` with a String
/// runs it as a command.
fn filter_map_one(
    tv: &typval_T,
    key: &typval_T,
    expr: &typval_T,
    filtermap: filtermap_T,
) -> Option<(typval_T, bool)> {
    // c: foreach() is not limited to an expression — a String is a command.
    if filtermap == FILTERMAP_FOREACH && expr.v_type == VAR_STRING {
        let hook = FILTER_MAP_CMD_HOOK.with(|h| *h.borrow());
        let ok = hook.is_some_and(|f| f(&tv_get_string(expr), key, tv));
        let unknown = typval_T {
            v_type: VAR_UNKNOWN,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_number(0),
        };
        return if ok { Some((unknown, false)) } else { None };
    }
    let hook = FILTER_MAP_EVAL_HOOK.with(|h| *h.borrow());
    let newtv = hook.and_then(|f| f(expr, key, tv))?;
    let mut rem = false;
    if filtermap == FILTERMAP_FILTER {
        // c: filter() removes the item when the expr is zero.
        let mut error = false;
        rem = tv_get_number_chk(&newtv, Some(&mut error)) == 0;
        if error {
            return None;
        }
    }
    Some((newtv, rem))
}

/// Port of `filter_map_list()` from `Src/eval/list.c:272`.
fn filter_map_list(
    l: &Rc<RefCell<list_T>>,
    filtermap: filtermap_T,
    arg_errmsg: &str,
    expr: &typval_T,
    rettv: &mut typval_T,
) {
    let nr_tv = |n: varnumber_T| typval_T {
        v_type: VAR_NUMBER,
        v_lock: VarLockStatus::VAR_UNLOCKED,
        vval: v_number(n),
    };
    if filtermap == FILTERMAP_FILTER
        && value_check_lock(l.borrow().lv_lock, Some(arg_errmsg), TV_TRANSLATE)
    {
        return;
    }
    let items: Vec<typval_T> = l
        .borrow()
        .lv_items
        .iter()
        .map(|it| it.li_tv.clone())
        .collect();
    let mut out: Vec<listitem_T> = Vec::with_capacity(items.len());
    let mut i = 0;
    let mut failed = false;
    while i < items.len() {
        let key = nr_tv(i as varnumber_T);
        match filter_map_one(&items[i], &key, expr, filtermap) {
            None => {
                failed = true;
                break;
            }
            Some((newtv, rem)) => match filtermap {
                FILTERMAP_MAP | FILTERMAP_MAPNEW => out.push(listitem_T { li_tv: newtv }),
                FILTERMAP_FILTER => {
                    if !rem {
                        out.push(listitem_T {
                            li_tv: items[i].clone(),
                        });
                    }
                }
                FILTERMAP_FOREACH => {}
            },
        }
        i += 1;
    }
    // c: on failure the loop stops, leaving later items unprocessed/original.
    if failed && matches!(filtermap, FILTERMAP_MAP | FILTERMAP_FILTER) {
        for it in items.iter().skip(i) {
            out.push(listitem_T { li_tv: it.clone() });
        }
    }
    match filtermap {
        FILTERMAP_MAP | FILTERMAP_FILTER => {
            let mut lb = l.borrow_mut();
            lb.lv_len = out.len() as i32;
            lb.lv_items = out;
        }
        FILTERMAP_MAPNEW => {
            let nl = tv_list_alloc_ret(rettv, out.len() as isize);
            let mut nb = nl.borrow_mut();
            nb.lv_len = out.len() as i32;
            nb.lv_items = out;
        }
        FILTERMAP_FOREACH => {}
    }
}

/// Port of `filter_map_dict()` from `Src/eval/list.c:83`.
fn filter_map_dict(
    d: &Rc<RefCell<dict_T>>,
    filtermap: filtermap_T,
    arg_errmsg: &str,
    expr: &typval_T,
    rettv: &mut typval_T,
) {
    if filtermap == FILTERMAP_FILTER
        && value_check_lock(d.borrow().dv_lock, Some(arg_errmsg), TV_TRANSLATE)
    {
        return;
    }
    let str_tv = |s: String| typval_T {
        v_type: VAR_STRING,
        v_lock: VarLockStatus::VAR_UNLOCKED,
        vval: v_string(s),
    };
    let d_ret = if filtermap == FILTERMAP_MAPNEW {
        Some(tv_dict_alloc_ret(rettv))
    } else {
        None
    };
    let keys: Vec<String> = d.borrow().dv_hashtab.keys().cloned().collect();
    for k in keys {
        let val = match d.borrow().dv_hashtab.get(&k) {
            Some(v) => v.clone(),
            None => continue,
        };
        let key = str_tv(k.clone());
        match filter_map_one(&val, &key, expr, filtermap) {
            None => break,
            Some((newtv, rem)) => match filtermap {
                FILTERMAP_MAP => {
                    d.borrow_mut().dv_hashtab.insert(k, newtv);
                }
                FILTERMAP_MAPNEW => {
                    if let Some(dr) = &d_ret {
                        tv_dict_add_tv(&mut dr.borrow_mut(), &k, newtv);
                    }
                }
                FILTERMAP_FILTER => {
                    if rem {
                        d.borrow_mut().dv_hashtab.shift_remove(&k);
                    }
                }
                FILTERMAP_FOREACH => {}
            },
        }
    }
}

/// Port of `filter_map_blob()` from `Src/eval/list.c:149`.
fn filter_map_blob(
    b: &Rc<RefCell<blob_T>>,
    filtermap: filtermap_T,
    arg_errmsg: &str,
    expr: &typval_T,
    rettv: &mut typval_T,
) {
    if filtermap == FILTERMAP_FILTER
        && value_check_lock(b.borrow().bv_lock, Some(arg_errmsg), TV_TRANSLATE)
    {
        return;
    }
    // mapnew() works on (and returns) a copy.
    let nr_tv = |n: varnumber_T| typval_T {
        v_type: VAR_NUMBER,
        v_lock: VarLockStatus::VAR_UNLOCKED,
        vval: v_number(n),
    };
    let b_ret = if filtermap == FILTERMAP_MAPNEW {
        // c: tv_blob_copy(b, rettv); b_ret = rettv->vval.v_blob;
        let nb = tv_blob_alloc();
        nb.borrow_mut().bv_ga = b.borrow().bv_ga.clone();
        rettv.v_type = VAR_BLOB;
        rettv.vval = v_blob(Some(nb.clone()));
        nb
    } else {
        b.clone()
    };
    let len = tv_blob_len(&b.borrow());
    let mut i = 0i32;
    let mut removed = 0i32;
    while i < len {
        let val = tv_blob_get(&b.borrow(), i) as varnumber_T;
        let tv = nr_tv(val);
        let key = nr_tv(i as varnumber_T);
        match filter_map_one(&tv, &key, expr, filtermap) {
            None => break,
            Some((newtv, rem)) => {
                if filtermap != FILTERMAP_FOREACH {
                    if newtv.v_type != VAR_NUMBER && newtv.v_type != VAR_BOOL {
                        emsg("E978: Invalid operation for Blob");
                        break;
                    }
                    if filtermap != FILTERMAP_FILTER {
                        let n = tv_get_number_chk(&newtv, None);
                        if n != val {
                            tv_blob_set(&mut b_ret.borrow_mut(), i - removed, n as u8);
                        }
                    } else if rem {
                        b.borrow_mut().bv_ga.remove((i - removed) as usize);
                        removed += 1;
                    }
                }
            }
        }
        i += 1;
    }
}

/// Port of `filter_map_string()` from `Src/eval/list.c:216`.
fn filter_map_string(str: &str, filtermap: filtermap_T, expr: &typval_T, rettv: &mut typval_T) {
    let nr_tv = |n: varnumber_T| typval_T {
        v_type: VAR_NUMBER,
        v_lock: VarLockStatus::VAR_UNLOCKED,
        vval: v_number(n),
    };
    let str_tv = |s: String| typval_T {
        v_type: VAR_STRING,
        v_lock: VarLockStatus::VAR_UNLOCKED,
        vval: v_string(s),
    };
    let mut ga = String::new();
    for (idx, ch) in str.chars().enumerate() {
        let tv = str_tv(ch.to_string());
        let key = nr_tv(idx as varnumber_T);
        match filter_map_one(&tv, &key, expr, filtermap) {
            None => break,
            Some((newtv, rem)) => {
                if filtermap == FILTERMAP_MAP || filtermap == FILTERMAP_MAPNEW {
                    if newtv.v_type != VAR_STRING {
                        emsg("E928: String required");
                        break;
                    }
                    ga.push_str(&tv_get_string(&newtv));
                } else if filtermap == FILTERMAP_FOREACH || !rem {
                    ga.push(ch);
                }
            }
        }
    }
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(ga);
}

/// Port of `filter_map()` from `Src/eval/list.c:336` — the shared dispatcher.
fn filter_map(argvars: &[typval_T], rettv: &mut typval_T, filtermap: filtermap_T) {
    let func_name = match filtermap {
        FILTERMAP_MAP => "map()",
        FILTERMAP_MAPNEW => "mapnew()",
        FILTERMAP_FILTER => "filter()",
        FILTERMAP_FOREACH => "foreach()",
    };
    let arg_errmsg = match filtermap {
        FILTERMAP_MAP => "map() argument",
        FILTERMAP_MAPNEW => "mapnew() argument",
        FILTERMAP_FILTER => "filter() argument",
        FILTERMAP_FOREACH => "foreach() argument",
    };
    // c: map/filter/foreach return the first argument (not mapnew, not a String).
    if filtermap != FILTERMAP_MAPNEW && argvars[0].v_type != VAR_STRING {
        *rettv = argvars[0].clone();
    }
    if !matches!(
        argvars[0].v_type,
        VAR_BLOB | VAR_LIST | VAR_DICT | VAR_STRING
    ) {
        emsg(&format!(
            "E1250: Argument of {func_name} must be a List, String, Dictionary or Blob"
        ));
        return;
    }
    if argvars.len() < 2 || argvars[1].v_type == VAR_UNKNOWN {
        return;
    }
    let expr = &argvars[1];
    match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_DICT, v_dict(Some(d))) => filter_map_dict(d, filtermap, arg_errmsg, expr, rettv),
        (VAR_BLOB, v_blob(Some(b))) => filter_map_blob(b, filtermap, arg_errmsg, expr, rettv),
        (VAR_STRING, _) => filter_map_string(&tv_get_string(&argvars[0]), filtermap, expr, rettv),
        (VAR_LIST, v_list(Some(l))) => filter_map_list(l, filtermap, arg_errmsg, expr, rettv),
        _ => {} // NULL container
    }
}

/// Port of `f_filter()` from `Src/eval/list.c:405`.
pub fn f_filter(argvars: &[typval_T], rettv: &mut typval_T) {
    filter_map(argvars, rettv, FILTERMAP_FILTER);
}
/// Port of `f_map()` from `Src/eval/list.c:411`.
pub fn f_map(argvars: &[typval_T], rettv: &mut typval_T) {
    filter_map(argvars, rettv, FILTERMAP_MAP);
}
/// Port of `f_mapnew()` from `Src/eval/list.c:417`.
pub fn f_mapnew(argvars: &[typval_T], rettv: &mut typval_T) {
    filter_map(argvars, rettv, FILTERMAP_MAPNEW);
}
/// Port of `f_foreach()` from `Src/eval/list.c:423`.
pub fn f_foreach(argvars: &[typval_T], rettv: &mut typval_T) {
    filter_map(argvars, rettv, FILTERMAP_FOREACH);
}

/// Port of `f_remove()` from `Src/eval/list.c:810`.
///
/// "remove()" function — drop an item/range from a List/Dict/Blob, returning it.
pub fn f_remove(argvars: &[typval_T], rettv: &mut typval_T) {
    let arg_errmsg = "remove() argument";
    // c: dispatch by container type; otherwise E896.
    match argvars[0].v_type {
        VAR_DICT => tv_dict_remove(argvars, rettv, arg_errmsg),
        VAR_BLOB => tv_blob_remove(argvars, rettv, arg_errmsg),
        VAR_LIST => tv_list_remove(argvars, rettv, arg_errmsg),
        _ => emsg("E896: Argument of remove() must be a List, Dictionary or Blob"),
    }
}
