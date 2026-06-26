//! Port of `src/nvim/eval/typval.c` (vendored at `csrc/eval/typval.c`).
//!
//! Vimscript value accessors and container operations. Function names,
//! signatures, and control flow match the C source (PORT.md Rules A/B/4).
#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]

use std::cell::RefCell;
use std::rc::Rc;

use crate::ported::charset::{vim_str2nr, STR2NR_ALL};
use crate::ported::eval::encode::{encode_tv2echo, encode_tv2string};
use crate::ported::eval::typval_defs_h::{
    blob_T, dict_T, float_T, list_T, listitem_T, typval_T, typval_vval_union::*, varnumber_T,
    BoolVarValue, BoolVarValue::*, SpecialVarValue::*, VarLockStatus, VarType::*,
};
use crate::ported::eval_h::{FAIL, OK};
use crate::ported::message::{emsg, semsg};

/// `static const char *const num_errors[]` from `Src/eval/typval.c` — the
/// "using a <type> as a Number" message for each non-numeric `VarType`, indexed
/// by `v_type`. (Indices follow the `VarType` declaration order, matching C.)
static num_errors: [&str; 11] = [
    "E685: Internal error",                 // VAR_UNKNOWN
    "",                                     // VAR_NUMBER (no error)
    "",                                     // VAR_STRING (no error)
    "E703: Using a Funcref as a Number",    // VAR_FUNC
    "E745: Using a List as a Number",       // VAR_LIST
    "E728: Using a Dictionary as a Number", // VAR_DICT
    "E805: Using a Float as a Number",      // VAR_FLOAT
    "",                                     // VAR_BOOL (no error)
    "",                                     // VAR_SPECIAL (no error)
    "E703: Using a Funcref as a Number",    // VAR_PARTIAL
    "E974: Using a Blob as a Number",       // VAR_BLOB
];

/// Port of `tv_get_number_chk()` from `Src/eval/typval.c`.
///
/// Get the number value of a Vimscript object. Float/List/Dict/Blob/Func raise
/// `emsg`; String is parsed with [`vim_str2nr`]; Bool/Special coerce.
pub fn tv_get_number_chk(tv: &typval_T, ret_error: Option<&mut bool>) -> varnumber_T {
    // c: switch (tv->v_type)
    match tv.v_type {
        // c: VAR_FUNC/VAR_PARTIAL/VAR_LIST/VAR_DICT/VAR_BLOB/VAR_FLOAT:
        //    emsg(_(num_errors[tv->v_type])); break;
        VAR_FUNC | VAR_PARTIAL | VAR_LIST | VAR_DICT | VAR_BLOB | VAR_FLOAT => {
            emsg(num_errors[tv.v_type as usize]);
        }
        VAR_NUMBER => {
            // c: return tv->vval.v_number;
            if let v_number(n) = &tv.vval {
                return *n;
            }
        }
        VAR_STRING => {
            // c: varnumber_T n = 0;
            //    if (tv->vval.v_string != NULL) vim_str2nr(..., STR2NR_ALL, &n, ...);
            //    return n;
            let mut n: varnumber_T = 0;
            if let v_string(s) = &tv.vval {
                vim_str2nr(s, None, None, STR2NR_ALL, Some(&mut n), None, 0, false, None);
            }
            return n;
        }
        VAR_BOOL => {
            // c: return tv->vval.v_bool == kBoolVarTrue ? 1 : 0;
            if let v_bool(b) = &tv.vval {
                return if *b == kBoolVarTrue { 1 } else { 0 };
            }
        }
        VAR_SPECIAL => {
            // c: return 0;
            return 0;
        }
        VAR_UNKNOWN => {
            // c: semsg(_(e_intern2), "tv_get_number(UNKNOWN)");
            semsg("E685: Internal error: tv_get_number(UNKNOWN)");
        }
    }
    // c: if (ret_error != NULL) *ret_error = true;
    //    return (ret_error == NULL ? -1 : 0);
    match ret_error {
        Some(e) => {
            *e = true;
            0
        }
        None => -1,
    }
}

/// Port of `tv_get_bool()` from `Src/eval/typval.c` — `tv_get_number_chk` with
/// no error sink.
pub fn tv_get_bool(tv: &typval_T) -> varnumber_T {
    tv_get_number_chk(tv, None)
}

/// Port of `tv_get_float_chk()` from `Src/eval/typval.c`.
///
/// Float → itself; Number → promoted; otherwise `emsg` and 0.0.
pub fn tv_get_float(tv: &typval_T) -> f64 {
    // c: switch (tv->v_type)
    match (tv.v_type, &tv.vval) {
        (VAR_NUMBER, v_number(n)) => *n as f64, // c: return (float_T)tv->vval.v_number;
        (VAR_FLOAT, v_float(f)) => *f,          // c: return tv->vval.v_float;
        _ => {
            // c: emsg(_("E808: Number or Float required"));
            emsg("E808: Number or Float required");
            0.0
        }
    }
}

/// Port of `tv_get_string_buf_chk()` from `Src/eval/typval.c`.
///
/// Number → decimal; Float → `%g`; String → itself; Bool → `v:false`/`v:true`;
/// Special → `v:null`. List/Dict/Blob/Func raise `emsg` and yield "". (We return
/// an owned `String`, so the C single-static-buffer caveat does not apply.)
pub fn tv_get_string_buf_chk(tv: &typval_T) -> Option<String> {
    match (tv.v_type, &tv.vval) {
        // c: snprintf(buf, NUMBUFLEN, "%" PRIdVARNUMBER, tv->vval.v_number);
        (VAR_NUMBER, v_number(n)) => Some(n.to_string()),
        // c: vim_snprintf(buf, NUMBUFLEN, "%g", tv->vval.v_float);
        // RUST-PORT NOTE: Rust has no printf-`%g`; default `f64` formatting is
        // the closest equivalent for the magnitudes the eval engine sees.
        (VAR_FLOAT, v_float(f)) => Some(if f.is_infinite() {
            if *f < 0.0 { "-inf" } else { "inf" }.to_string()
        } else if f.is_nan() {
            "nan".to_string()
        } else {
            format!("{f}")
        }),
        // c: return tv->vval.v_string == NULL ? "" : v_string;
        (VAR_STRING, v_string(s)) => Some(s.clone()),
        (VAR_FUNC, v_string(s)) => Some(s.clone()),
        // c: STRCPY(buf, encode_bool_var_names[tv->vval.v_bool]);
        (VAR_BOOL, v_bool(b)) => {
            Some(if *b == kBoolVarTrue { "v:true" } else { "v:false" }.to_string())
        }
        // c: STRCPY(buf, encode_special_var_names[tv->vval.v_special]);
        (VAR_SPECIAL, _) => Some("v:null".to_string()),
        // c: emsg(_(str_errors[tv->v_type])); return NULL;
        _ => {
            emsg("E730: Using a List/Dict/Funcref/Blob as a String");
            None
        }
    }
}

/// Port of `tv_get_string()` from `Src/eval/typval.c` — never-NULL convenience
/// over `tv_get_string_buf_chk` (NULL → "").
pub fn tv_get_string(tv: &typval_T) -> String {
    tv_get_string_buf_chk(tv).unwrap_or_default()
}

/// Port of `tv_equal()` from `Src/eval/typval.c` (the `ic == false` path).
///
/// Scalars compare within type; List/Dict/Blob compare structurally; Funcref by
/// name. Number and Float do NOT cross-compare equal here (that promotion lives
/// in `typval_compare`).
pub fn tv_equal(tv1: &typval_T, tv2: &typval_T, ic: bool) -> bool {
    // c: VAR_FUNC/VAR_PARTIAL compare via func_equal() even across the two types;
    // a NULL partial equals nothing.
    let is_func = |t| matches!(t, VAR_FUNC | VAR_PARTIAL);
    if is_func(tv1.v_type) && is_func(tv2.v_type) {
        if (tv1.v_type == VAR_PARTIAL && matches!(&tv1.vval, v_partial(None)))
            || (tv2.v_type == VAR_PARTIAL && matches!(&tv2.vval, v_partial(None)))
        {
            return false;
        }
        return crate::ported::eval::func_equal(tv1, tv2, ic);
    }
    match (&tv1.vval, &tv2.vval) {
        (v_number(a), v_number(b)) => a == b,
        (v_float(a), v_float(b)) => a == b,
        (v_string(a), v_string(b)) => {
            // VAR_FUNC and VAR_STRING both use v_string; compare only same type.
            tv1.v_type == tv2.v_type && a == b
        }
        (v_bool(a), v_bool(b)) => a == b,
        (v_special(a), v_special(b)) => a == b,
        (v_list(Some(a)), v_list(Some(b))) => tv_list_equal(a, b, ic),
        (v_list(None), v_list(None)) => true,
        (v_dict(Some(a)), v_dict(Some(b))) => tv_dict_equal(a, b, ic),
        (v_dict(None), v_dict(None)) => true,
        (v_blob(Some(a)), v_blob(Some(b))) => tv_blob_equal(a, b),
        (v_blob(None), v_blob(None)) => true,
        _ => false,
    }
}

// ── lists ──

/// Port of `tv_list_alloc()` from `Src/eval/typval.c` — allocate an empty list.
pub fn tv_list_alloc(_len: isize) -> Rc<RefCell<list_T>> {
    Rc::new(RefCell::new(list_T::default()))
}

/// Port of `tv_list_alloc_ret()` from `Src/eval/typval.c` — allocate a list and
/// set `rettv` to it (the `VAR_LIST`-returning builtins' entry point).
pub fn tv_list_alloc_ret(rettv: &mut typval_T, len: isize) -> Rc<RefCell<list_T>> {
    let l = tv_list_alloc(len);
    rettv.v_type = crate::ported::eval::typval_defs_h::VarType::VAR_LIST;
    rettv.vval = v_list(Some(l.clone()));
    l
}

/// Port of `tv_dict_alloc_ret()` from `Src/eval/typval.c` — allocate a dict and
/// set `rettv` to it.
pub fn tv_dict_alloc_ret(rettv: &mut typval_T) -> Rc<RefCell<dict_T>> {
    let d = tv_dict_alloc();
    rettv.v_type = crate::ported::eval::typval_defs_h::VarType::VAR_DICT;
    rettv.vval = v_dict(Some(d.clone()));
    d
}

/// Port of `tv_list_len()` from `Src/eval/typval.c`.
pub fn tv_list_len(l: &list_T) -> i32 {
    l.lv_len
}

/// Port of `tv_list_append_tv()` from `Src/eval/typval.c` — append a copy of
/// `tv` as the list's last item.
pub fn tv_list_append_tv(l: &mut list_T, tv: typval_T) {
    l.lv_items.push(listitem_T { li_tv: tv });
    l.lv_len = l.lv_items.len() as i32;
}

/// Port of `tv_list_append_string()` from `Src/eval/typval.c` — append a
/// `VAR_STRING` item.
pub fn tv_list_append_string(l: &mut list_T, s: &str) {
    tv_list_append_tv(
        l,
        typval_T {
            v_type: VAR_STRING,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_string(s.to_string()),
        },
    );
}

/// Port of `tv_list_append_number()` from `Src/eval/typval.c` — append a
/// `VAR_NUMBER` item.
pub fn tv_list_append_number(l: &mut list_T, n: varnumber_T) {
    tv_list_append_tv(
        l,
        typval_T {
            v_type: VAR_NUMBER,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_number(n),
        },
    );
}

/// Port of `tv_list_equal()` from `Src/eval/typval.c`.
pub fn tv_list_equal(l1: &Rc<RefCell<list_T>>, l2: &Rc<RefCell<list_T>>, ic: bool) -> bool {
    if Rc::ptr_eq(l1, l2) {
        // c: if (l1 == l2) return true;
        return true;
    }
    let (l1, l2) = (l1.borrow(), l2.borrow());
    if l1.lv_len != l2.lv_len {
        return false;
    }
    l1.lv_items
        .iter()
        .zip(l2.lv_items.iter())
        .all(|(a, b)| tv_equal(&a.li_tv, &b.li_tv, ic))
}

/// Port of `tv_list_uidx()` from `Src/eval/typval.h` (c:136) — normalize a
/// possibly-negative index into `0..lv_len`, or -1 if out of range.
pub fn tv_list_uidx(l: &list_T, n: i32) -> i32 {
    let mut n = n;
    // c: if (n < 0) n += tv_list_len(l);
    if n < 0 {
        n += tv_list_len(l);
    }
    // c: if (n < 0 || n >= tv_list_len(l)) return -1;
    if n < 0 || n >= tv_list_len(l) {
        return -1;
    }
    n
}

/// Port of `tv_list_find()` from `Src/eval/typval.c` (c:1612) — the item at
/// index `n` (negative counts from the end), or `None` if out of range. The C
/// linked-list walk and `lv_idx` cache are a perf detail; over the `Vec` model
/// this is a direct index after `tv_list_uidx`.
pub fn tv_list_find(l: &list_T, n: i32) -> Option<&listitem_T> {
    let n = tv_list_uidx(l, n);
    if n == -1 {
        return None;
    }
    l.lv_items.get(n as usize)
}

/// Port of `tv_list_find_nr()` from `Src/eval/typval.c` (c:1684) — `l[n]` as a
/// Number, or -1 (with `*ret_error = true`) when the index is out of range.
pub fn tv_list_find_nr(l: &list_T, n: i32, ret_error: Option<&mut bool>) -> varnumber_T {
    match tv_list_find(l, n) {
        None => {
            if let Some(e) = ret_error {
                *e = true;
            }
            -1
        }
        Some(li) => tv_get_number_chk(&li.li_tv, ret_error),
    }
}

/// Port of `tv_list_find_str()` from `Src/eval/typval.c` (c:1703) — `l[n]` as a
/// string, or `None` (with an `emsg`) when the index is out of range.
pub fn tv_list_find_str(l: &list_T, n: i32) -> Option<String> {
    match tv_list_find(l, n) {
        None => {
            semsg(&format!("E684: list index out of range: {n}"));
            None
        }
        Some(li) => Some(tv_get_string(&li.li_tv)),
    }
}

/// Port of `tv_list_reverse()` from `Src/eval/typval.c` (c:1581) — reverse the
/// list in place. (The C pointer swaps and `lv_idx` fix-up reduce to
/// `Vec::reverse` here.)
pub fn tv_list_reverse(l: &mut list_T) {
    if tv_list_len(l) <= 1 {
        return;
    }
    l.lv_items.reverse();
}

// ── dicts ──

/// Port of `tv_dict_alloc()` from `Src/eval/typval.c`.
pub fn tv_dict_alloc() -> Rc<RefCell<dict_T>> {
    Rc::new(RefCell::new(dict_T::default()))
}

/// Port of `tv_dict_len()` from `Src/eval/typval.c`.
pub fn tv_dict_len(d: &dict_T) -> i32 {
    d.dv_hashtab.len() as i32
}

/// Port of `tv_dict_find()` from `Src/eval/typval.c` — look up a key.
pub fn tv_dict_find<'d>(d: &'d dict_T, key: &str) -> Option<&'d typval_T> {
    d.dv_hashtab.get(key)
}

/// Port of `tv_dict_add_tv()` from `Src/eval/typval.c` — set a key's value.
pub fn tv_dict_add_tv(d: &mut dict_T, key: &str, tv: typval_T) {
    d.dv_hashtab.insert(key.to_string(), tv);
}

/// Port of `tv_dict_add()` from `Src/eval/typval.c` (c:2472) — add an item under
/// `key`, returning `FAIL` if the key already exists (no overwrite — unlike
/// [`tv_dict_add_tv`]). The dictitem here is the hashtab entry itself.
/// (`tv_dict_wrong_func_name` only guards `VAR_FUNC` keys and is omitted —
/// funcrefs-as-keys are not modeled.)
pub fn tv_dict_add(d: &mut dict_T, key: &str, tv: typval_T) -> i32 {
    if d.dv_hashtab.contains_key(key) {
        return FAIL;
    }
    d.dv_hashtab.insert(key.to_string(), tv);
    OK
}

/// Port of `tv_dict_add_nr()` from `Src/eval/typval.c` (c:2556).
pub fn tv_dict_add_nr(d: &mut dict_T, key: &str, nr: varnumber_T) -> i32 {
    tv_dict_add(
        d,
        key,
        typval_T { v_type: VAR_NUMBER, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v_number(nr) },
    )
}

/// Port of `tv_dict_add_float()` from `Src/eval/typval.c` (c:2569).
pub fn tv_dict_add_float(d: &mut dict_T, key: &str, nr: float_T) -> i32 {
    tv_dict_add(
        d,
        key,
        typval_T { v_type: VAR_FLOAT, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v_float(nr) },
    )
}

/// Port of `tv_dict_add_bool()` from `Src/eval/typval.c` (c:2593).
pub fn tv_dict_add_bool(d: &mut dict_T, key: &str, val: BoolVarValue) -> i32 {
    tv_dict_add(
        d,
        key,
        typval_T { v_type: VAR_BOOL, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v_bool(val) },
    )
}

/// Port of `tv_dict_add_str()` from `Src/eval/typval.c` (c:2616) — adds a
/// (copied) string entry. Delegates as in C through `tv_dict_add_str_len(…,-1)`.
pub fn tv_dict_add_str(d: &mut dict_T, key: &str, val: &str) -> i32 {
    tv_dict_add_allocated_str(d, key, val.to_string())
}

/// Port of `tv_dict_add_allocated_str()` from `Src/eval/typval.c` (c:2648) —
/// adds `val` as a `VAR_STRING` entry, taking ownership of the string.
pub fn tv_dict_add_allocated_str(d: &mut dict_T, key: &str, val: String) -> i32 {
    tv_dict_add(
        d,
        key,
        typval_T { v_type: VAR_STRING, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v_string(val) },
    )
}

/// Port of `tv_dict_has_key()` from `Src/eval/typval.c` (c:2270).
pub fn tv_dict_has_key(d: &dict_T, key: &str) -> bool {
    tv_dict_find(d, key).is_some()
}

/// Port of `tv_dict_get_number()` from `Src/eval/typval.c` (c:2299) — the Number
/// value of `key`, or 0 if absent.
pub fn tv_dict_get_number(d: &dict_T, key: &str) -> varnumber_T {
    tv_dict_get_number_def(d, key, 0)
}

/// Port of `tv_dict_get_number_def()` from `Src/eval/typval.c` (c:2312) — the
/// Number value of `key`, or `def` if absent.
pub fn tv_dict_get_number_def(d: &dict_T, key: &str, def: varnumber_T) -> varnumber_T {
    match tv_dict_find(d, key) {
        None => def,
        Some(di) => tv_get_number(di),
    }
}

/// Port of `tv_dict_get_bool()` from `Src/eval/typval.c` (c:2322) — the boolean
/// value of `key`, or `def` if absent.
pub fn tv_dict_get_bool(d: &dict_T, key: &str, def: varnumber_T) -> varnumber_T {
    match tv_dict_find(d, key) {
        None => def,
        Some(di) => tv_get_bool(di),
    }
}

/// Port of `tv_dict_get_string()` from `Src/eval/typval.c` (c:2367) — the string
/// value of `key` (numbers coerced), or `None` if the key does not exist.
pub fn tv_dict_get_string(d: &dict_T, key: &str) -> Option<String> {
    tv_dict_find(d, key).map(tv_get_string)
}

/// Port of `tv_get_number()` from `Src/eval/typval.c` (c:4188) — the Number
/// value of `tv`, errors reported via `emsg` (discarded here).
pub fn tv_get_number(tv: &typval_T) -> varnumber_T {
    let mut error = false;
    tv_get_number_chk(tv, Some(&mut error))
}

/// Port of `tv2bool()` from `Src/eval/typval.c` (c:4684) — truthiness of `tv`
/// (used by `:if`/`:while` and the logical operators).
pub fn tv2bool(tv: &typval_T) -> bool {
    match (tv.v_type, &tv.vval) {
        (VAR_NUMBER, v_number(n)) => *n != 0,
        (VAR_FLOAT, v_float(f)) => *f != 0.0,
        (VAR_FUNC, v_string(s)) | (VAR_STRING, v_string(s)) => !s.is_empty(),
        (VAR_LIST, v_list(l)) => l.as_ref().is_some_and(|l| l.borrow().lv_len > 0),
        (VAR_DICT, v_dict(d)) => d.as_ref().is_some_and(|d| !d.borrow().dv_hashtab.is_empty()),
        (VAR_BOOL, v_bool(b)) => *b == kBoolVarTrue,
        (VAR_SPECIAL, v_special(s)) => *s != kSpecialVarNull,
        (VAR_BLOB, v_blob(b)) => b.as_ref().is_some_and(|b| !b.borrow().bv_ga.is_empty()),
        // c: VAR_PARTIAL → v_partial != NULL.
        (VAR_PARTIAL, v_partial(p)) => p.is_some(),
        // VAR_UNKNOWN is falsy.
        _ => false,
    }
}

/// Port of `tv_copy()` from `Src/eval/typval.c` (c:3724) — copy `from` into `to`
/// with the lock cleared. Rc-backed compound values (List/Dict/Blob) clone by
/// reference-count bump (== `tv_list_ref` / `dv_refcount++` / `bv_refcount++`);
/// strings clone by value (== `xstrdup`).
pub fn tv_copy(from: &typval_T, to: &mut typval_T) {
    *to = from.clone();
    to.v_lock = VarLockStatus::VAR_UNLOCKED;
}

// ── argument / value type checks (Src/eval/typval.c) ──
//
// The C functions index a NUL-terminated `argvars` array, so an absent optional
// argument reads as the `VAR_UNKNOWN` sentinel. Over the `&[typval_T]` slice an
// absent argument is simply `idx >= len`, which these ports treat as
// `VAR_UNKNOWN` (via `args.get(idx)`).

/// Port of `tv_check_str_or_nr()` from `Src/eval/typval.c` (c:4051) — true if
/// `tv` is a Number or String; otherwise `emsg` and false.
pub fn tv_check_str_or_nr(tv: &typval_T) -> bool {
    match tv.v_type {
        VAR_NUMBER | VAR_STRING => true,
        VAR_FLOAT => {
            emsg("E805: Expected a Number or a String, Float found");
            false
        }
        VAR_PARTIAL | VAR_FUNC => {
            emsg("E703: Expected a Number or a String, Funcref found");
            false
        }
        VAR_LIST => {
            emsg("E745: Expected a Number or a String, List found");
            false
        }
        VAR_DICT => {
            emsg("E728: Expected a Number or a String, Dictionary found");
            false
        }
        VAR_BLOB => {
            emsg("E974: Expected a Number or a String, Blob found");
            false
        }
        VAR_BOOL => {
            emsg("E5299: Expected a Number or a String, Boolean found");
            false
        }
        VAR_SPECIAL => {
            emsg("E5300: Expected a Number or a String");
            false
        }
        VAR_UNKNOWN => {
            semsg("E685: Internal error: tv_check_str_or_nr(UNKNOWN)");
            false
        }
    }
}

/// Port of `tv_check_num()` from `Src/eval/typval.c` (c:4110) — true if `tv` is
/// a Number or coercible to one (Bool/Special/String); otherwise `emsg`/false.
pub fn tv_check_num(tv: &typval_T) -> bool {
    match tv.v_type {
        VAR_NUMBER | VAR_BOOL | VAR_SPECIAL | VAR_STRING => true,
        VAR_FUNC | VAR_PARTIAL => {
            emsg("E703: Using a Funcref as a Number");
            false
        }
        VAR_LIST => {
            emsg("E745: Using a List as a Number");
            false
        }
        VAR_DICT => {
            emsg("E728: Using a Dictionary as a Number");
            false
        }
        VAR_FLOAT => {
            emsg("E805: Using a Float as a Number");
            false
        }
        VAR_BLOB => {
            emsg("E974: Using a Blob as a Number");
            false
        }
        VAR_UNKNOWN => {
            emsg("E685: using an invalid value as a Number");
            false
        }
    }
}

/// Port of `tv_check_str()` from `Src/eval/typval.c` (c:4154) — true if `tv` is
/// a String or coercible to one (Number/Bool/Special/Float); else `emsg`/false.
pub fn tv_check_str(tv: &typval_T) -> bool {
    match tv.v_type {
        VAR_NUMBER | VAR_BOOL | VAR_SPECIAL | VAR_STRING | VAR_FLOAT => true,
        VAR_PARTIAL | VAR_FUNC => {
            emsg("E729: Using a Funcref as a String");
            false
        }
        VAR_LIST => {
            emsg("E730: Using a List as a String");
            false
        }
        VAR_DICT => {
            emsg("E731: Using a Dictionary as a String");
            false
        }
        VAR_BLOB => {
            emsg("E976: Using a Blob as a String");
            false
        }
        VAR_UNKNOWN => {
            emsg("E908: Using an invalid value as a String");
            false
        }
    }
}

/// Port of `tv_check_for_string_arg()` from `Src/eval/typval.c` (c:4345).
pub fn tv_check_for_string_arg(args: &[typval_T], idx: usize) -> i32 {
    if args.get(idx).map(|a| a.v_type) != Some(VAR_STRING) {
        semsg(&format!("E1174: String required for argument {}", idx + 1));
        return FAIL;
    }
    OK
}

/// Port of `tv_check_for_nonempty_string_arg()` from `Src/eval/typval.c` (c:4356).
pub fn tv_check_for_nonempty_string_arg(args: &[typval_T], idx: usize) -> i32 {
    if tv_check_for_string_arg(args, idx) == FAIL {
        return FAIL;
    }
    let empty = matches!(args.get(idx).map(|a| &a.vval), Some(v_string(s)) if s.is_empty());
    if empty {
        semsg(&format!("E1175: Non-empty string required for argument {}", idx + 1));
        return FAIL;
    }
    OK
}

/// Port of `tv_check_for_opt_string_arg()` from `Src/eval/typval.c` (c:4370).
pub fn tv_check_for_opt_string_arg(args: &[typval_T], idx: usize) -> i32 {
    if args.get(idx).map_or(VAR_UNKNOWN, |a| a.v_type) == VAR_UNKNOWN
        || tv_check_for_string_arg(args, idx) != FAIL
    {
        OK
    } else {
        FAIL
    }
}

/// Port of `tv_check_for_number_arg()` from `Src/eval/typval.c` (c:4378).
pub fn tv_check_for_number_arg(args: &[typval_T], idx: usize) -> i32 {
    if args.get(idx).map(|a| a.v_type) != Some(VAR_NUMBER) {
        semsg(&format!("E1210: Number required for argument {}", idx + 1));
        return FAIL;
    }
    OK
}

/// Port of `tv_check_for_opt_number_arg()` from `Src/eval/typval.c` (c:4392).
pub fn tv_check_for_opt_number_arg(args: &[typval_T], idx: usize) -> i32 {
    if args.get(idx).map_or(VAR_UNKNOWN, |a| a.v_type) == VAR_UNKNOWN
        || tv_check_for_number_arg(args, idx) != FAIL
    {
        OK
    } else {
        FAIL
    }
}

/// Port of `tv_check_for_float_or_nr_arg()` from `Src/eval/typval.c` (c:4400).
pub fn tv_check_for_float_or_nr_arg(args: &[typval_T], idx: usize) -> i32 {
    let t = args.get(idx).map(|a| a.v_type);
    if t != Some(VAR_FLOAT) && t != Some(VAR_NUMBER) {
        semsg(&format!("E1219: Float or Number required for argument {}", idx + 1));
        return FAIL;
    }
    OK
}

/// Port of `tv_check_for_bool_arg()` from `Src/eval/typval.c` (c:4408) — a Bool,
/// or a Number that is 0 or 1.
pub fn tv_check_for_bool_arg(args: &[typval_T], idx: usize) -> i32 {
    let ok = match args.get(idx) {
        Some(a) if a.v_type == VAR_BOOL => true,
        Some(a) if a.v_type == VAR_NUMBER => {
            matches!(&a.vval, v_number(n) if *n == 0 || *n == 1)
        }
        _ => false,
    };
    if !ok {
        semsg(&format!("E1212: Bool required for argument {}", idx + 1));
        return FAIL;
    }
    OK
}

/// Port of `tv_check_for_opt_bool_arg()` from `Src/eval/typval.c` (c:4426).
pub fn tv_check_for_opt_bool_arg(args: &[typval_T], idx: usize) -> i32 {
    if args.get(idx).map_or(VAR_UNKNOWN, |a| a.v_type) == VAR_UNKNOWN {
        return OK;
    }
    tv_check_for_bool_arg(args, idx)
}

/// Port of `tv_check_for_blob_arg()` from `Src/eval/typval.c` (c:4433).
pub fn tv_check_for_blob_arg(args: &[typval_T], idx: usize) -> i32 {
    if args.get(idx).map(|a| a.v_type) != Some(VAR_BLOB) {
        semsg(&format!("E1238: Blob required for argument {}", idx + 1));
        return FAIL;
    }
    OK
}

/// Port of `tv_check_for_list_arg()` from `Src/eval/typval.c` (c:4444).
pub fn tv_check_for_list_arg(args: &[typval_T], idx: usize) -> i32 {
    if args.get(idx).map(|a| a.v_type) != Some(VAR_LIST) {
        semsg(&format!("E1211: List required for argument {}", idx + 1));
        return FAIL;
    }
    OK
}

/// Port of `tv_check_for_dict_arg()` from `Src/eval/typval.c` (c:4455).
pub fn tv_check_for_dict_arg(args: &[typval_T], idx: usize) -> i32 {
    if args.get(idx).map(|a| a.v_type) != Some(VAR_DICT) {
        semsg(&format!("E1206: Dictionary required for argument {}", idx + 1));
        return FAIL;
    }
    OK
}

/// Port of `tv_check_for_opt_dict_arg()` from `Src/eval/typval.c` (c:4478).
pub fn tv_check_for_opt_dict_arg(args: &[typval_T], idx: usize) -> i32 {
    if args.get(idx).map_or(VAR_UNKNOWN, |a| a.v_type) == VAR_UNKNOWN
        || tv_check_for_dict_arg(args, idx) != FAIL
    {
        OK
    } else {
        FAIL
    }
}

/// Port of `tv_check_for_string_or_number_arg()` from `Src/eval/typval.c` (c:4489).
pub fn tv_check_for_string_or_number_arg(args: &[typval_T], idx: usize) -> i32 {
    let t = args.get(idx).map(|a| a.v_type);
    if t != Some(VAR_STRING) && t != Some(VAR_NUMBER) {
        semsg(&format!("E1220: String or Number required for argument {}", idx + 1));
        return FAIL;
    }
    OK
}

/// Port of `tv_check_for_buffer_arg()` from `Src/eval/typval.c` (c:4501) — a
/// buffer number is a Number or a String.
pub fn tv_check_for_buffer_arg(args: &[typval_T], idx: usize) -> i32 {
    tv_check_for_string_or_number_arg(args, idx)
}

/// Port of `tv_check_for_lnum_arg()` from `Src/eval/typval.c` (c:4509) — a line
/// number is a Number or a String.
pub fn tv_check_for_lnum_arg(args: &[typval_T], idx: usize) -> i32 {
    tv_check_for_string_or_number_arg(args, idx)
}

/// Port of `tv_check_for_string_or_list_arg()` from `Src/eval/typval.c` (c:4516).
pub fn tv_check_for_string_or_list_arg(args: &[typval_T], idx: usize) -> i32 {
    let t = args.get(idx).map(|a| a.v_type);
    if t != Some(VAR_STRING) && t != Some(VAR_LIST) {
        semsg(&format!("E1222: String or List required for argument {}", idx + 1));
        return FAIL;
    }
    OK
}

/// Port of `tv_check_for_string_or_list_or_blob_arg()` from `Src/eval/typval.c`
/// (c:4527).
pub fn tv_check_for_string_or_list_or_blob_arg(args: &[typval_T], idx: usize) -> i32 {
    let t = args.get(idx).map(|a| a.v_type);
    if t != Some(VAR_STRING) && t != Some(VAR_LIST) && t != Some(VAR_BLOB) {
        semsg(&format!("E1252: String, List or Blob required for argument {}", idx + 1));
        return FAIL;
    }
    OK
}

/// Port of `tv_check_for_opt_string_or_list_arg()` from `Src/eval/typval.c`
/// (c:4540).
pub fn tv_check_for_opt_string_or_list_arg(args: &[typval_T], idx: usize) -> i32 {
    if args.get(idx).map_or(VAR_UNKNOWN, |a| a.v_type) == VAR_UNKNOWN
        || tv_check_for_string_or_list_arg(args, idx) != FAIL
    {
        OK
    } else {
        FAIL
    }
}

/// Port of `tv_check_for_string_or_func_arg()` from `Src/eval/typval.c` (c:4549).
pub fn tv_check_for_string_or_func_arg(args: &[typval_T], idx: usize) -> i32 {
    let t = args.get(idx).map(|a| a.v_type);
    if t != Some(VAR_PARTIAL) && t != Some(VAR_FUNC) && t != Some(VAR_STRING) {
        semsg(&format!("E1256: String or function required for argument {}", idx + 1));
        return FAIL;
    }
    OK
}

/// Port of `tv_check_for_list_or_blob_arg()` from `Src/eval/typval.c` (c:4562).
pub fn tv_check_for_list_or_blob_arg(args: &[typval_T], idx: usize) -> i32 {
    let t = args.get(idx).map(|a| a.v_type);
    if t != Some(VAR_LIST) && t != Some(VAR_BLOB) {
        semsg(&format!("E1226: List or Blob required for argument {}", idx + 1));
        return FAIL;
    }
    OK
}

/// Port of `tv_dict_equal()` from `Src/eval/typval.c`.
pub fn tv_dict_equal(d1: &Rc<RefCell<dict_T>>, d2: &Rc<RefCell<dict_T>>, ic: bool) -> bool {
    if Rc::ptr_eq(d1, d2) {
        return true;
    }
    let (d1, d2) = (d1.borrow(), d2.borrow());
    if d1.dv_hashtab.len() != d2.dv_hashtab.len() {
        return false;
    }
    d1.dv_hashtab
        .iter()
        .all(|(k, v)| d2.dv_hashtab.get(k).is_some_and(|w| tv_equal(v, w, ic)))
}

// ── blobs ──

/// Port of `tv_blob_alloc()` from `Src/eval/typval.c`.
pub fn tv_blob_alloc() -> Rc<RefCell<blob_T>> {
    Rc::new(RefCell::new(blob_T::default()))
}

/// Port of `tv_blob_len()` from `Src/eval/typval.c`.
pub fn tv_blob_len(b: &blob_T) -> i32 {
    b.bv_ga.len() as i32
}

/// Port of `tv_blob_equal()` from `Src/eval/typval.c`.
pub fn tv_blob_equal(b1: &Rc<RefCell<blob_T>>, b2: &Rc<RefCell<blob_T>>) -> bool {
    if Rc::ptr_eq(b1, b2) {
        return true;
    }
    b1.borrow().bv_ga == b2.borrow().bv_ga
}

/// Port of `tv_blob_get()` from `Src/eval/typval.h` (h:263) — the byte at `idx`.
pub fn tv_blob_get(b: &blob_T, idx: i32) -> u8 {
    b.bv_ga[idx as usize]
}

/// Port of `tv_blob_set()` from `Src/eval/typval.h` (h:274) — store `c` at `idx`.
pub fn tv_blob_set(blob: &mut blob_T, idx: i32, c: u8) {
    blob.bv_ga[idx as usize] = c;
}

/// Port of `tv_blob_set_ret()` from `Src/eval/typval.h` (h:235) — point `tv` at
/// blob `b` (the C `bv_refcount++` is the `Rc` clone the caller hands in).
pub fn tv_blob_set_ret(tv: &mut typval_T, b: Rc<RefCell<blob_T>>) {
    tv.v_type = VAR_BLOB;
    tv.vval = v_blob(Some(b));
}

/// Port of `tv_blob_alloc_ret()` from `Src/eval/typval.c` (c:3374) — allocate a
/// blob and set `ret_tv` to it.
pub fn tv_blob_alloc_ret(ret_tv: &mut typval_T) -> Rc<RefCell<blob_T>> {
    let b = tv_blob_alloc();
    tv_blob_set_ret(ret_tv, b.clone());
    b
}

/// Port of `tv_blob_copy()` from `Src/eval/typval.c` (c:3386) — deep-copy the
/// bytes of `from` (NULL → an empty/NULL blob) into `to`.
pub fn tv_blob_copy(from: Option<&Rc<RefCell<blob_T>>>, to: &mut typval_T) {
    to.v_type = VAR_BLOB;
    to.v_lock = VarLockStatus::VAR_UNLOCKED;
    match from {
        None => to.vval = v_blob(None),
        Some(from) => {
            let b = tv_blob_alloc_ret(to);
            // c: xmemdup(from->bv_ga.ga_data, len); ga_len = ga_maxlen = len;
            b.borrow_mut().bv_ga = from.borrow().bv_ga.clone();
        }
    }
}

/// Port of `tv_blob_set_range()` from `Src/eval/typval.c` (c:3075) — set bytes
/// `n1..=n2` of `dest` from `src`; `FAIL` if the byte counts differ. (`src` is
/// the blob directly here, not the wrapping typval.)
pub fn tv_blob_set_range(dest: &mut blob_T, n1: varnumber_T, n2: varnumber_T, src: &blob_T) -> i32 {
    if n2 - n1 + 1 != tv_blob_len(src) as varnumber_T {
        emsg("E972: Blob value does not have the right number of bytes");
        return FAIL;
    }
    let mut ir = 0;
    for il in n1..=n2 {
        tv_blob_set(dest, il as i32, tv_blob_get(src, ir));
        ir += 1;
    }
    OK
}

/// Port of `tv_blob_set_append()` from `Src/eval/typval.c` (c:3090) — store
/// `byte` at `idx`, appending one byte when `idx` is exactly the current length.
pub fn tv_blob_set_append(blob: &mut blob_T, idx: i32, byte: u8) {
    let ga_len = blob.bv_ga.len() as i32;
    // c: setting a byte beyond the end (other than appending one) is ignored.
    if idx <= ga_len {
        if idx == ga_len {
            blob.bv_ga.push(0);
        }
        tv_blob_set(blob, idx, byte);
    }
}

/// Port of `f_blob2list()` from `Src/eval/typval.c:3165`.
///
/// "blob2list()" function — a List of the blob's byte values.
pub fn f_blob2list(argvars: &[typval_T], rettv: &mut typval_T) {
    let l = tv_list_alloc_ret(rettv, 0);
    if tv_check_for_blob_arg(argvars, 0) == FAIL {
        return;
    }
    if let v_blob(Some(blob)) = &argvars[0].vval {
        let blob = blob.borrow();
        let mut lb = l.borrow_mut();
        for i in 0..tv_blob_len(&blob) {
            tv_list_append_number(&mut lb, tv_blob_get(&blob, i) as varnumber_T);
        }
    }
}

/// Port of `f_list2blob()` from `Src/eval/typval.c:3181`.
///
/// "list2blob()" function — a Blob from a List of byte values (`0..=255`);
/// `E1239` on an out-of-range value.
pub fn f_list2blob(argvars: &[typval_T], rettv: &mut typval_T) {
    let blob = tv_blob_alloc_ret(rettv);
    if tv_check_for_list_arg(argvars, 0) == FAIL {
        return;
    }
    if let v_list(Some(l)) = &argvars[0].vval {
        let lb = l.borrow();
        let mut bb = blob.borrow_mut();
        for li in &lb.lv_items {
            let mut error = false;
            let n = tv_get_number_chk(&li.li_tv, Some(&mut error));
            if error || n < 0 || n > 255 {
                if !error {
                    emsg(&format!("E1239: Invalid value for blob: 0x{n:X}"));
                }
                bb.bv_ga.clear();
                return;
            }
            bb.bv_ga.push(n as u8);
        }
    }
}

/// Port of `tv_blob_check_index()` from `Src/eval/typval.c:3049`.
///
/// Check that `n1` is a valid index for assigning into a blob of length
/// `bloblen` (`0..=bloblen`); `E979` unless `quiet`.
pub fn tv_blob_check_index(bloblen: i32, n1: varnumber_T, quiet: bool) -> i32 {
    // c: if (n1 < 0 || n1 > bloblen) { if (!quiet) semsg(e_blobidx, n1); return FAIL; }
    if n1 < 0 || n1 > bloblen as varnumber_T {
        if !quiet {
            emsg(&format!("E979: Blob index out of range: {n1}"));
        }
        return FAIL;
    }
    OK
}

/// Port of `tv_blob_check_range()` from `Src/eval/typval.c:3061`.
///
/// Check that `[n1, n2]` is a valid range within a blob of length `bloblen`;
/// `E979` (on `n2`) unless `quiet`.
pub fn tv_blob_check_range(bloblen: i32, n1: varnumber_T, n2: varnumber_T, quiet: bool) -> i32 {
    // c: if (n2 < 0 || n2 >= bloblen || n2 < n1) { if (!quiet) semsg(e_blobidx, n2); return FAIL; }
    if n2 < 0 || n2 >= bloblen as varnumber_T || n2 < n1 {
        if !quiet {
            emsg(&format!("E979: Blob index out of range: {n2}"));
        }
        return FAIL;
    }
    OK
}

/// Port of `tv_blob_index()` from `Src/eval/typval.c` — index a blob, yielding
/// the byte at `idx` as a Number in `rettv` (which holds the blob; `blob` is the
/// same value, read from to avoid borrowing `rettv` twice). `E979` out of range.
fn tv_blob_index(blob: &blob_T, len: i32, mut idx: varnumber_T, rettv: &mut typval_T) -> i32 {
    // c: if (idx < 0) idx = len + idx;
    if idx < 0 {
        idx = len as varnumber_T + idx;
    }
    if idx < len as varnumber_T && idx >= 0 {
        let v = tv_blob_get(blob, idx as i32) as varnumber_T;
        tv_clear(rettv);
        rettv.v_type = VAR_NUMBER;
        rettv.vval = v_number(v);
    } else {
        emsg(&format!("E979: Blob index out of range: {idx}"));
        return FAIL;
    }
    OK
}

/// Port of `tv_blob_slice()` from `Src/eval/typval.c` — slice a blob into a new
/// sub-blob in `rettv`; out-of-range indices yield an empty (NULL) blob.
fn tv_blob_slice(
    blob: &blob_T,
    len: i32,
    mut n1: varnumber_T,
    mut n2: varnumber_T,
    exclusive: bool,
    rettv: &mut typval_T,
) -> i32 {
    let len = len as varnumber_T;
    // c: clamp n1/n2 (negative from end; n2 past end → last index, exclusive-aware).
    if n1 < 0 {
        n1 = len + n1;
        if n1 < 0 {
            n1 = 0;
        }
    }
    if n2 < 0 {
        n2 = len + n2;
    } else if n2 >= len {
        n2 = len - if exclusive { 0 } else { 1 };
    }
    if exclusive {
        n2 -= 1;
    }
    if n1 >= len || n2 < 0 || n1 > n2 {
        tv_clear(rettv);
        rettv.v_type = VAR_BLOB;
        rettv.vval = v_blob(None);
    } else {
        let new_blob = tv_blob_alloc();
        new_blob.borrow_mut().bv_ga = (n1..=n2).map(|i| tv_blob_get(blob, i as i32)).collect();
        tv_clear(rettv);
        tv_blob_set_ret(rettv, new_blob);
    }
    OK
}

/// Port of `tv_blob_slice_or_index()` from `Src/eval/typval.c:3036`.
///
/// Dispatch a blob subscript to a single-byte index or a sub-blob slice.
pub fn tv_blob_slice_or_index(
    blob: &blob_T,
    is_range: bool,
    n1: varnumber_T,
    n2: varnumber_T,
    exclusive: bool,
    rettv: &mut typval_T,
) -> i32 {
    // c: len = tv_blob_len(rettv->vval.v_blob);  (== `blob`)
    let len = tv_blob_len(blob);
    if is_range {
        tv_blob_slice(blob, len, n1, n2, exclusive, rettv)
    } else {
        tv_blob_index(blob, len, n1, rettv)
    }
}

// ── copy / extend / concat / slice / flatten / items (Src/eval/typval.c) ──
//
// The C linked-list walk + `copyID` cycle detection + `vimconv` reduce over the
// `Vec`/`Rc` model: a shallow copy is `tv_copy` per item, a deep copy delegates
// to `var_item_copy` (matching `f_copy`/`f_deepcopy`; `copyID` cycle detection is
// not modeled, so self-referential containers are unsupported, as in the
// existing `var_item_copy`).

/// Port of `tv_list_copy()` from `Src/eval/typval.c` (c:591) — a new list with
/// each item shallow- (`deep=false`) or deep-copied (`deep=true`).
pub fn tv_list_copy(orig: &Rc<RefCell<list_T>>, deep: bool) -> Rc<RefCell<list_T>> {
    let items: Vec<typval_T> = orig
        .borrow()
        .lv_items
        .iter()
        .map(|it| {
            if deep {
                crate::ported::eval::funcs::var_item_copy(&it.li_tv)
            } else {
                { let mut t = it.li_tv.clone(); t.v_lock = VarLockStatus::VAR_UNLOCKED; t }
            }
        })
        .collect();
    let copy = tv_list_alloc(items.len() as isize);
    {
        let mut c = copy.borrow_mut();
        for tv in items {
            tv_list_append_tv(&mut c, tv);
        }
    }
    copy
}

/// Port of `tv_dict_copy()` from `Src/eval/typval.c` (c:2838) — a new dict with
/// each value shallow- or deep-copied.
pub fn tv_dict_copy(orig: &Rc<RefCell<dict_T>>, deep: bool) -> Rc<RefCell<dict_T>> {
    let pairs: Vec<(String, typval_T)> = orig
        .borrow()
        .dv_hashtab
        .iter()
        .map(|(k, v)| {
            let nv = if deep {
                crate::ported::eval::funcs::var_item_copy(v)
            } else {
                { let mut t = v.clone(); t.v_lock = VarLockStatus::VAR_UNLOCKED; t }
            };
            (k.clone(), nv)
        })
        .collect();
    let copy = tv_dict_alloc();
    {
        let mut c = copy.borrow_mut();
        for (k, v) in pairs {
            tv_dict_add(&mut c, &k, v);
        }
    }
    copy
}

/// Port of `tv_list_extend()` from `Src/eval/typval.c` (c:868) — append (a copy
/// of) every item of `l2` to `l1`, before index `bef` (or at the end when
/// `None`). The C self-extend guard is unnecessary: `l1` and `l2` are distinct
/// borrows here.
pub fn tv_list_extend(l1: &mut list_T, l2: &list_T, bef: Option<usize>) {
    let add: Vec<typval_T> = l2.lv_items.iter().map(|it| { let mut t = it.li_tv.clone(); t.v_lock = VarLockStatus::VAR_UNLOCKED; t }).collect();
    match bef {
        None => {
            for tv in add {
                tv_list_append_tv(l1, tv);
            }
        }
        Some(mut i) => {
            i = i.min(l1.lv_items.len());
            for tv in add {
                l1.lv_items.insert(i, listitem_T { li_tv: tv });
                i += 1;
            }
            l1.lv_len = l1.lv_items.len() as i32;
        }
    }
}

/// Port of `tv_list_concat()` from `Src/eval/typval.c` (c:896) — set `tv` to a
/// new list that is `l1` followed by `l2` (either may be a NULL list).
pub fn tv_list_concat(
    l1: Option<&Rc<RefCell<list_T>>>,
    l2: Option<&Rc<RefCell<list_T>>>,
    tv: &mut typval_T,
) -> i32 {
    tv.v_type = VAR_BLOB; // placeholder; set below
    let l = match (l1, l2) {
        (None, None) => None,
        (None, Some(l2)) => Some(tv_list_copy(l2, false)),
        (Some(l1), l2) => {
            let copy = tv_list_copy(l1, false);
            if let Some(l2) = l2 {
                tv_list_extend(&mut copy.borrow_mut(), &l2.borrow(), None);
            }
            Some(copy)
        }
    };
    tv.v_type = VAR_LIST;
    tv.v_lock = VarLockStatus::VAR_UNLOCKED;
    tv.vval = v_list(l);
    OK
}

/// Port of `tv_list_slice()` from `Src/eval/typval.c` (c:921) — a new list of
/// items `n1..=n2` (caller-validated indices).
pub fn tv_list_slice(ol: &list_T, n1: varnumber_T, n2: varnumber_T) -> Rc<RefCell<list_T>> {
    let l = tv_list_alloc((n2 - n1 + 1) as isize);
    {
        let mut lb = l.borrow_mut();
        let mut i = n1;
        while i <= n2 {
            if let Some(it) = ol.lv_items.get(i as usize) {
                tv_list_append_tv(&mut lb, { let mut t = it.li_tv.clone(); t.v_lock = VarLockStatus::VAR_UNLOCKED; t });
            }
            i += 1;
        }
    }
    l
}

/// Port of `tv_list_flatten()` from `Src/eval/typval.c` (c:752) — replace nested
/// List items (up to `maxdepth`) with their contents, in place.
pub fn tv_list_flatten(list: &mut list_T, maxitems: i64, maxdepth: i64) {
    if maxdepth == 0 {
        return;
    }
    let mut i = 0usize;
    let mut done: i64 = 0;
    while i < list.lv_items.len() && done < maxitems {
        let inner = match (list.lv_items[i].li_tv.v_type, &list.lv_items[i].li_tv.vval) {
            (VAR_LIST, v_list(Some(inner))) => Some(inner.clone()),
            _ => None,
        };
        if let Some(inner) = inner {
            let mut sub: Vec<typval_T> =
                inner.borrow().lv_items.iter().map(|it| { let mut t = it.li_tv.clone(); t.v_lock = VarLockStatus::VAR_UNLOCKED; t }).collect();
            if maxdepth > 0 {
                let tmp = tv_list_alloc(0);
                let items: Vec<listitem_T> = sub.into_iter().map(|tv| listitem_T { li_tv: tv }).collect();
                let n = items.len() as i32;
                {
                    let mut t = tmp.borrow_mut();
                    t.lv_items = items;
                    t.lv_len = n;
                }
                let inner_len = inner.borrow().lv_len as i64;
                tv_list_flatten(&mut tmp.borrow_mut(), inner_len, maxdepth - 1);
                sub = tmp.borrow().lv_items.iter().map(|it| it.li_tv.clone()).collect();
            }
            list.lv_items.remove(i);
            for tv in sub {
                list.lv_items.insert(i, listitem_T { li_tv: tv });
                i += 1;
            }
            list.lv_len = list.lv_items.len() as i32;
        } else {
            i += 1;
        }
        done += 1;
    }
}

/// Port of `tv_dict_alloc_lock()` from `Src/eval/typval.c` (c:3232).
pub fn tv_dict_alloc_lock(lock: VarLockStatus) -> Rc<RefCell<dict_T>> {
    let d = tv_dict_alloc();
    d.borrow_mut().dv_lock = lock;
    d
}

/// Port of `enum DictListType` from `Src/eval/typval.c` — what `tv_dict2list`
/// extracts.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DictListType {
    kDict2ListKeys,
    kDict2ListValues,
    kDict2ListItems,
}
use DictListType::*;

/// Port of `tv_dict2list()` from `Src/eval/typval.c` (c:3258) — turn a Dict into
/// a List of keys, values, or `[key, value]` pairs.
pub fn tv_dict2list(argvars: &[typval_T], rettv: &mut typval_T, what: DictListType) {
    if tv_check_for_dict_arg(argvars, 0) == FAIL {
        tv_list_alloc_ret(rettv, 0);
        return;
    }
    let (VAR_DICT, v_dict(d)) = (argvars[0].v_type, &argvars[0].vval) else {
        tv_list_alloc_ret(rettv, 0);
        return;
    };
    let Some(d) = d else {
        tv_list_alloc_ret(rettv, 0);
        return;
    };
    let pairs: Vec<(String, typval_T)> =
        d.borrow().dv_hashtab.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let out = tv_list_alloc_ret(rettv, pairs.len() as isize);
    let mut ob = out.borrow_mut();
    for (k, v) in pairs {
        match what {
            kDict2ListKeys => tv_list_append_string(&mut ob, &k),
            kDict2ListValues => tv_list_append_tv(&mut ob, { let mut t = v.clone(); t.v_lock = VarLockStatus::VAR_UNLOCKED; t }),
            kDict2ListItems => {
                let sub = tv_list_alloc(2);
                {
                    let mut sb = sub.borrow_mut();
                    tv_list_append_string(&mut sb, &k);
                    tv_list_append_tv(&mut sb, v);
                }
                tv_list_append_list(&mut ob, sub);
            }
        }
    }
}

/// Port of `tv_blob2items()` from `Src/eval/typval.c` (c:798) — a Blob as a List
/// of `[index, byte]` pairs.
pub fn tv_blob2items(argvars: &[typval_T], rettv: &mut typval_T) {
    let bytes: Vec<u8> = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_BLOB, v_blob(Some(b))) => b.borrow().bv_ga.clone(),
        _ => Vec::new(),
    };
    let out = tv_list_alloc_ret(rettv, bytes.len() as isize);
    let mut ob = out.borrow_mut();
    for (i, byte) in bytes.iter().enumerate() {
        let l2 = tv_list_alloc(2);
        {
            let mut lb = l2.borrow_mut();
            tv_list_append_number(&mut lb, i as varnumber_T);
            tv_list_append_number(&mut lb, *byte as varnumber_T);
        }
        tv_list_append_list(&mut ob, l2);
    }
}

/// Port of `tv_list2items()` from `Src/eval/typval.c` (c:820) — a List as a List
/// of `[index, value]` pairs.
pub fn tv_list2items(argvars: &[typval_T], rettv: &mut typval_T) {
    let items: Vec<typval_T> = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => l.borrow().lv_items.iter().map(|it| it.li_tv.clone()).collect(),
        _ => Vec::new(),
    };
    let out = tv_list_alloc_ret(rettv, items.len() as isize);
    let mut ob = out.borrow_mut();
    for (idx, tv) in items.into_iter().enumerate() {
        let l2 = tv_list_alloc(2);
        {
            let mut lb = l2.borrow_mut();
            tv_list_append_number(&mut lb, idx as varnumber_T);
            tv_list_append_tv(&mut lb, tv);
        }
        tv_list_append_list(&mut ob, l2);
    }
}

/// Port of `tv_dict2items()` from `Src/eval/typval.c` (c:813).
pub fn tv_dict2items(argvars: &[typval_T], rettv: &mut typval_T) {
    tv_dict2list(argvars, rettv, kDict2ListItems);
}

/// Port of `tv_string2items()` from `Src/eval/typval.c` (c:841) — a String as a
/// List of `[char-index, character]` pairs.
pub fn tv_string2items(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_STRING, v_string(s)) => s.clone(),
        _ => String::new(),
    };
    let out = tv_list_alloc_ret(rettv, 0);
    let mut ob = out.borrow_mut();
    for (idx, ch) in s.chars().enumerate() {
        let l2 = tv_list_alloc(2);
        {
            let mut lb = l2.borrow_mut();
            tv_list_append_number(&mut lb, idx as varnumber_T);
            tv_list_append_string(&mut lb, &ch.to_string());
        }
        tv_list_append_list(&mut ob, l2);
    }
}

/// Port of `tv_dict_set_keys_readonly()` from `Src/eval/typval.c` (c:2896) —
/// mark every key read-only. No-op: per-item `di_flags` (RO/FIXED) are not
/// modeled here.
pub fn tv_dict_set_keys_readonly(_dict: &mut dict_T) {}

// ── reference counting / freeing (Rc-managed) ──
//
// The C reference counting and `xfree` chains are handled by `Rc<RefCell<…>>`
// here: dropping the last `Rc` frees. These ports keep the C names and update
// the (now vestigial) `*_refcount` fields, but actual lifetime is the `Rc`'s.

/// Port of `tv_list_ref()` — increment the reference count. (Appending an `Rc`
/// clone is itself the reference, so callers that push need not also call this.)
pub fn tv_list_ref(l: &mut list_T) {
    l.lv_refcount += 1;
}

/// Port of `tv_list_unref()` from `Src/eval/typval.c` (c:329) — decrement;
/// the `Rc` frees the list when the last reference drops.
pub fn tv_list_unref(l: &mut list_T) {
    l.lv_refcount -= 1;
}

/// Port of `tv_list_free_contents()` from `Src/eval/typval.c` (c:270) — clear
/// every item (each value's `Rc`s drop here).
pub fn tv_list_free_contents(l: &mut list_T) {
    l.lv_items.clear();
    l.lv_len = 0;
}

/// Port of `tv_list_free_list()` from `Src/eval/typval.c` (c:290) — free the
/// list struct itself. No-op: the `Rc` frees it (no GC list to unlink).
pub fn tv_list_free_list(_l: &mut list_T) {}

/// Port of `tv_list_free()` from `Src/eval/typval.c` (c:313) — free a list and
/// its items.
pub fn tv_list_free(l: &mut list_T) {
    tv_list_free_contents(l);
    tv_list_free_list(l);
}

/// Port of `tv_dict_unref()` from `Src/eval/typval.c` (c:2233).
pub fn tv_dict_unref(d: &mut dict_T) {
    d.dv_refcount -= 1;
}

/// Port of `tv_dict_free_contents()` from `Src/eval/typval.c` (c:2164).
pub fn tv_dict_free_contents(d: &mut dict_T) {
    d.dv_hashtab.clear();
}

/// Port of `tv_dict_free()` from `Src/eval/typval.c` (c:2217).
pub fn tv_dict_free(d: &mut dict_T) {
    tv_dict_free_contents(d);
}

/// Port of `tv_blob_unref()` from `Src/eval/typval.c` — decrement; `Rc` frees.
pub fn tv_blob_unref(b: &mut blob_T) {
    b.bv_refcount -= 1;
}

/// Port of `tv_blob_free()` from `Src/eval/typval.c` — free a blob (`Rc`-managed).
pub fn tv_blob_free(b: &mut blob_T) {
    b.bv_ga.clear();
}

// ── list append / dict ops ──

/// Port of `tv_list_append_list()` from `Src/eval/typval.c` (c:500) — append
/// `itemlist` to `l` as a single List item (the `Rc` clone is the reference).
pub fn tv_list_append_list(l: &mut list_T, itemlist: Rc<RefCell<list_T>>) {
    tv_list_append_tv(
        l,
        typval_T { v_type: VAR_LIST, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v_list(Some(itemlist)) },
    );
}

/// Port of `tv_list_append_dict()` from `Src/eval/typval.c` (c:515) — append
/// `dict` to `l` as a single Dict item.
pub fn tv_list_append_dict(l: &mut list_T, dict: Rc<RefCell<dict_T>>) {
    tv_list_append_tv(
        l,
        typval_T { v_type: VAR_DICT, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v_dict(Some(dict)) },
    );
}

/// Port of `tv_list_append_allocated_string()` from `Src/eval/typval.c` (c:555)
/// — append `str` as a String item, taking ownership.
pub fn tv_list_append_allocated_string(l: &mut list_T, str: String) {
    tv_list_append_tv(
        l,
        typval_T { v_type: VAR_STRING, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v_string(str) },
    );
}

/// Port of `tv_dict_clear()` from `Src/eval/typval.c` (c:2700) — remove every
/// entry, leaving a valid empty Dict.
pub fn tv_dict_clear(d: &mut dict_T) {
    d.dv_hashtab.clear();
}

/// Port of `tv_dict_extend()` from `Src/eval/typval.c` (c:2723) — merge `d2`
/// into `d1` per `action`: `"error"`/`e` (duplicate key → E737), `"force"`/`f`
/// (d2 overrides), other/`"keep"` (duplicate d2 keys ignored). (The `move`
/// optimization, watchers and scope-name validation are not modeled.)
pub fn tv_dict_extend(d1: &mut dict_T, d2: &dict_T, action: &str) {
    let act = action.as_bytes().first().copied().unwrap_or(b'f');
    let pairs: Vec<_> = d2.dv_hashtab.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (k, v) in pairs {
        if d1.dv_hashtab.contains_key(&k) {
            match act {
                b'e' => {
                    semsg(&format!("E737: Key already exists: {k}"));
                    break;
                }
                b'f' => {
                    d1.dv_hashtab.insert(k, v);
                }
                _ => {} // keep: ignore duplicate
            }
        } else {
            d1.dv_hashtab.insert(k, v);
        }
    }
}

/// Port of `tv_dict_add_list()` from `Src/eval/typval.c` (c:2489) — add `list`
/// under `key` as a List entry; `FAIL` if the key exists.
pub fn tv_dict_add_list(d: &mut dict_T, key: &str, list: Rc<RefCell<list_T>>) -> i32 {
    tv_dict_add(
        d,
        key,
        typval_T { v_type: VAR_LIST, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v_list(Some(list)) },
    )
}

/// Port of `tv_dict_add_dict()` from `Src/eval/typval.c` (c:2532) — add `dict`
/// under `key` as a Dict entry; `FAIL` if the key exists.
pub fn tv_dict_add_dict(d: &mut dict_T, key: &str, dict: Rc<RefCell<dict_T>>) -> i32 {
    tv_dict_add(
        d,
        key,
        typval_T { v_type: VAR_DICT, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v_dict(Some(dict)) },
    )
}

/// Port of `tv_dict_add_str_len()` from `Src/eval/typval.c` (c:2632) — add the
/// first `len` bytes of `val` (or all of it when `len < 0`) under `key`.
pub fn tv_dict_add_str_len(d: &mut dict_T, key: &str, val: &str, len: i32) -> i32 {
    let s = if len < 0 {
        val.to_string()
    } else {
        val.chars().take(len as usize).collect()
    };
    tv_dict_add_allocated_str(d, key, s)
}

/// Port of `tv_dict_get_string_buf()` from `Src/eval/typval.c` (c:2387) — the
/// string value of `key` (numbers coerced), or `None` if the key is absent.
pub fn tv_dict_get_string_buf(d: &dict_T, key: &str) -> Option<String> {
    tv_dict_find(d, key).map(tv_get_string)
}

/// Port of `tv_dict_get_string_buf_chk()` from `Src/eval/typval.c` (c:2409) —
/// `def` when the key is absent, `None` on a type error, the string otherwise.
pub fn tv_dict_get_string_buf_chk(d: &dict_T, key: &str, def: Option<String>) -> Option<String> {
    match tv_dict_find(d, key) {
        None => def,
        Some(di) => tv_get_string_buf_chk(di),
    }
}

/// Port of `tv_dict_get_tv()` from `Src/eval/typval.c` (c:2282) — copy `key`'s
/// value into `rettv`; `OK` on success, `FAIL` if the key is absent.
pub fn tv_dict_get_tv(d: &dict_T, key: &str, rettv: &mut typval_T) -> i32 {
    match tv_dict_find(d, key) {
        None => FAIL,
        Some(di) => {
            let di = di.clone();
            tv_copy(&di, rettv);
            OK
        }
    }
}

/// Port of `tv_dict_to_env()` from `Src/eval/typval.c` (c:2334) — render the
/// dict as `KEY=VALUE` environment strings.
pub fn tv_dict_to_env(denv: &dict_T) -> Vec<String> {
    denv.dv_hashtab.iter().map(|(k, v)| format!("{k}={}", tv_get_string(v))).collect()
}

// ── clear / free / get ──

/// Port of `tv_clear()` from `Src/eval/typval.c` (c:3655) — free the value held
/// by `tv` and reset it to an unlocked `VAR_UNKNOWN` (compound `Rc`s drop here).
pub fn tv_clear(tv: &mut typval_T) {
    if tv.v_type == VAR_UNKNOWN {
        return;
    }
    *tv = typval_T { v_type: VAR_UNKNOWN, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v_number(0) };
}

/// Port of `tv_free()` from `Src/eval/typval.c` (c:3677) — free `tv` and the
/// value inside it. (`Rc`-managed: dropping the value releases it.)
pub fn tv_free(tv: &mut typval_T) {
    tv_clear(tv);
}

/// Port of `tv_islocked()` from `Src/eval/typval.c` (c:3859) — true if the value
/// is locked itself or refers to a locked List/Dict container.
pub fn tv_islocked(tv: &typval_T) -> bool {
    if tv.v_lock == VarLockStatus::VAR_LOCKED {
        return true;
    }
    match (tv.v_type, &tv.vval) {
        (VAR_LIST, v_list(Some(l))) => l.borrow().lv_lock == VarLockStatus::VAR_LOCKED,
        (VAR_DICT, v_dict(Some(d))) => d.borrow().dv_lock == VarLockStatus::VAR_LOCKED,
        _ => false,
    }
}

/// `TV_TRANSLATE` from `Src/eval/typval.h:436` — a `name_len` sentinel: the name
/// is a message id to translate (identity here).
pub const TV_TRANSLATE: usize = usize::MAX;
/// `TV_CSTRING` from `Src/eval/typval.h:441` — a `name_len` sentinel: the name is
/// a NUL-terminated C string (use its whole length).
pub const TV_CSTRING: usize = usize::MAX - 1;

/// `DICT_MAXNEST` from `Src/eval/typval.c:113` — the recursion cap for (un)lock.
const DICT_MAXNEST: i32 = 100;

/// Port of `value_check_lock()` from `Src/eval/typval.c:3917`.
///
/// If `lock` makes the value read-only, emit `E741`/`E742` (naming the value when
/// `name` is given, per the C `%.*s`) and return true.
pub fn value_check_lock(lock: VarLockStatus, name: Option<&str>, name_len: usize) -> bool {
    // The `%.*s` shows `name_len` bytes of the name; the sentinels mean "all".
    let shown = |n: &str| -> String {
        if name_len == TV_TRANSLATE || name_len == TV_CSTRING {
            n.to_string()
        } else {
            n.get(..name_len).unwrap_or(n).to_string()
        }
    };
    let msg = match lock {
        VarLockStatus::VAR_UNLOCKED => return false,
        VarLockStatus::VAR_LOCKED => match name {
            None => "E741: Value is locked".to_string(),
            Some(n) => format!("E741: Value is locked: {}", shown(n)),
        },
        VarLockStatus::VAR_FIXED => match name {
            None => "E742: Cannot change value".to_string(),
            Some(n) => format!("E742: Cannot change value of {}", shown(n)),
        },
    };
    emsg(&msg);
    true
}

/// Port of `tv_check_lock()` from `Src/eval/typval.c:3888`.
///
/// Check both the value's own lock and (for a container) its contents' lock.
pub fn tv_check_lock(tv: &typval_T, name: Option<&str>, name_len: usize) -> bool {
    let lock = match (tv.v_type, &tv.vval) {
        (VAR_BLOB, v_blob(Some(b))) => b.borrow().bv_lock,
        (VAR_LIST, v_list(Some(l))) => l.borrow().lv_lock,
        (VAR_DICT, v_dict(Some(d))) => d.borrow().dv_lock,
        _ => VarLockStatus::VAR_UNLOCKED,
    };
    value_check_lock(tv.v_lock, name, name_len)
        || (lock != VarLockStatus::VAR_UNLOCKED && value_check_lock(lock, name, name_len))
}

thread_local! {
    /// `tv_item_lock`'s `static int recurse` (the (un)lock nesting guard).
    static TV_ITEM_LOCK_RECURSE: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
}

/// Port of `tv_item_lock()` from `Src/eval/typval.c:3777`.
///
/// Lock (or unlock) `tv` and, to `deep` levels (`< 0` = all), its contents. The C
/// `check_refcount` skips shared containers; the `*_refcount` fields are retained
/// for fidelity. The `CHANGE_LOCK` macro is inlined (`VAR_FIXED` never changes).
pub fn tv_item_lock(tv: &mut typval_T, deep: i32, lock: bool, check_refcount: bool) {
    // c: if (recurse >= DICT_MAXNEST) { emsg(e_variable_nested_too_deep_for_unlock); return; }
    if TV_ITEM_LOCK_RECURSE.with(|r| r.get()) >= DICT_MAXNEST {
        emsg("E743: Variable nested too deep for (un)lock");
        return;
    }
    if deep == 0 {
        return;
    }
    TV_ITEM_LOCK_RECURSE.with(|r| r.set(r.get() + 1));

    let change_lock = |var: VarLockStatus| -> VarLockStatus {
        match var {
            VarLockStatus::VAR_FIXED => VarLockStatus::VAR_FIXED,
            _ => {
                if lock {
                    VarLockStatus::VAR_LOCKED
                } else {
                    VarLockStatus::VAR_UNLOCKED
                }
            }
        }
    };
    tv.v_lock = change_lock(tv.v_lock);

    match (tv.v_type, &tv.vval) {
        (VAR_BLOB, v_blob(Some(b))) => {
            let mut bb = b.borrow_mut();
            if !(check_refcount && bb.bv_refcount > 1) {
                bb.bv_lock = change_lock(bb.bv_lock);
            }
        }
        (VAR_LIST, v_list(Some(l))) => {
            let recurse = {
                let mut lb = l.borrow_mut();
                if !(check_refcount && lb.lv_refcount > 1) {
                    lb.lv_lock = change_lock(lb.lv_lock);
                    deep < 0 || deep > 1
                } else {
                    false
                }
            };
            if recurse {
                let mut lb = l.borrow_mut();
                for li in lb.lv_items.iter_mut() {
                    tv_item_lock(&mut li.li_tv, deep - 1, lock, check_refcount);
                }
            }
        }
        (VAR_DICT, v_dict(Some(d))) => {
            let recurse = {
                let mut db = d.borrow_mut();
                if !(check_refcount && db.dv_refcount > 1) {
                    db.dv_lock = change_lock(db.dv_lock);
                    deep < 0 || deep > 1
                } else {
                    false
                }
            };
            if recurse {
                let mut db = d.borrow_mut();
                for (_k, v) in db.dv_hashtab.iter_mut() {
                    tv_item_lock(v, deep - 1, lock, check_refcount);
                }
            }
        }
        _ => {}
    }
    TV_ITEM_LOCK_RECURSE.with(|r| r.set(r.get() - 1));
}

/// Port of `tv_get_bool_chk()` from `Src/eval/typval.c` (c:4248) — alias for
/// `tv_get_number_chk` (Bool is a thin wrapper over Number).
pub fn tv_get_bool_chk(tv: &typval_T, ret_error: Option<&mut bool>) -> varnumber_T {
    tv_get_number_chk(tv, ret_error)
}

/// Port of `tv_get_string_chk()` from `Src/eval/typval.c` (c:4628) — the string
/// value, or `None` on a type error. (Our owned `String` avoids the C single
/// static buffer; `tv_get_string_buf_chk` is the same here.)
pub fn tv_get_string_chk(tv: &typval_T) -> Option<String> {
    tv_get_string_buf_chk(tv)
}

/// Port of `tv_get_string_buf()` from `Src/eval/typval.c` (c:4673) — like
/// `tv_get_string_chk` but a type error yields "" instead of `None`.
pub fn tv_get_string_buf(tv: &typval_T) -> String {
    tv_get_string_buf_chk(tv).unwrap_or_default()
}

/// Port of `tv_list_remove()` from `Src/eval/typval.c:1127`.
///
/// `remove()` on a List: drop the item at `argvars[1]` (returning it), or the
/// range `[argvars[1], argvars[2]]` (returning a new List). The C linked-list
/// `tv_list_drop_items`/`tv_list_move_items` reduce to `Vec::remove`/`drain`.
pub fn tv_list_remove(argvars: &[typval_T], rettv: &mut typval_T, arg_errmsg: &str) {
    let l = match &argvars[0].vval {
        v_list(Some(l)) => l.clone(),
        _ => return,
    };
    // c: value_check_lock(tv_list_locked(l), arg_errmsg, TV_TRANSLATE)
    if value_check_lock(l.borrow().lv_lock, Some(arg_errmsg), TV_TRANSLATE) {
        return;
    }
    let mut error = false;
    let idx = tv_get_number_chk(&argvars[1], Some(&mut error));
    if error {
        return;
    }
    let len = tv_list_len(&l.borrow());
    // c: item = tv_list_find(l, idx); NULL → E684.
    if tv_list_find(&l.borrow(), idx as i32).is_none() {
        emsg(&format!("E684: List index out of range: {idx}"));
        return;
    }
    let start = if idx < 0 { len as varnumber_T + idx } else { idx } as usize;

    if argvars.len() < 3 {
        // c: remove one item, return its value.
        let mut lb = l.borrow_mut();
        let it = lb.lv_items.remove(start);
        lb.lv_len = lb.lv_items.len() as i32;
        *rettv = it.li_tv;
    } else {
        // c: remove a range, return a List with the values.
        let mut error2 = false;
        let end = tv_get_number_chk(&argvars[2], Some(&mut error2));
        if error2 {
            return;
        }
        if tv_list_find(&l.borrow(), end as i32).is_none() {
            emsg(&format!("E684: List index out of range: {end}"));
            return;
        }
        let endi = if end < 0 { len as varnumber_T + end } else { end } as usize;
        // c: "item2" must be at or after "item" (forward walk) → else E16.
        if endi < start {
            emsg("E16: Invalid range");
            return;
        }
        let out = tv_list_alloc_ret(rettv, (endi - start + 1) as isize);
        let drained: Vec<listitem_T> = {
            let mut lb = l.borrow_mut();
            let drained = lb.lv_items.drain(start..=endi).collect::<Vec<_>>();
            lb.lv_len = lb.lv_items.len() as i32;
            drained
        };
        let mut ob = out.borrow_mut();
        ob.lv_len = drained.len() as i32;
        ob.lv_items = drained;
    }
}

/// Port of `tv_dict_remove()` from `Src/eval/typval.c:3344`.
///
/// `remove()` on a Dict: drop and return the value at key `argvars[1]`. The
/// `di_flags` (ro/fixed) checks and watchers are not modeled.
pub fn tv_dict_remove(argvars: &[typval_T], rettv: &mut typval_T, arg_errmsg: &str) {
    // c: if (argvars[2] != UNKNOWN) semsg(e_toomanyarg, "remove()");
    if argvars.len() > 2 {
        emsg("E118: Too many arguments for function: remove()");
        return;
    }
    let d = match &argvars[0].vval {
        v_dict(Some(d)) => d.clone(),
        _ => return,
    };
    // c: value_check_lock(d->dv_lock, arg_errmsg, TV_TRANSLATE)
    if value_check_lock(d.borrow().dv_lock, Some(arg_errmsg), TV_TRANSLATE) {
        return;
    }
    let key = match tv_get_string_chk(&argvars[1]) {
        Some(k) => k,
        None => return,
    };
    // c: di = tv_dict_find(d, key); NULL → E716; else remove + return.
    let removed = d.borrow_mut().dv_hashtab.shift_remove(&key);
    match removed {
        Some(v) => {
            // c: tv_dict_watcher_notify(d, key, NULL, &oldtv) on removal.
            tv_dict_watcher_notify(&d, &key, None, Some(&v));
            *rettv = v;
        }
        None => emsg(&format!("E716: Key not present in Dictionary: \"{key}\"")),
    }
}

/// Port of `tv_blob_remove()` from `Src/eval/typval.c` — `remove()` on a Blob:
/// drop the byte at `argvars[1]` (returning it), or the range `[argvars[1],
/// argvars[2]]` (returning a new Blob).
pub fn tv_blob_remove(argvars: &[typval_T], rettv: &mut typval_T, arg_errmsg: &str) {
    let b = match &argvars[0].vval {
        v_blob(Some(b)) => b.clone(),
        _ => return,
    };
    // c: value_check_lock(b->bv_lock, arg_errmsg, TV_TRANSLATE)
    if value_check_lock(b.borrow().bv_lock, Some(arg_errmsg), TV_TRANSLATE) {
        return;
    }
    let mut error = false;
    let mut idx = tv_get_number_chk(&argvars[1], Some(&mut error));
    if error {
        return;
    }
    let len = tv_blob_len(&b.borrow());
    // c: if (idx < 0) idx = len + idx;
    if idx < 0 {
        idx += len as varnumber_T;
    }
    if idx < 0 || idx >= len as varnumber_T {
        emsg(&format!("E979: Blob index out of range: {idx}"));
        return;
    }
    let idx = idx as usize;

    if argvars.len() < 3 {
        // c: remove one byte, return its value.
        let mut bb = b.borrow_mut();
        let v = bb.bv_ga[idx] as varnumber_T;
        bb.bv_ga.remove(idx);
        rettv.vval = v_number(v);
    } else {
        // c: remove a range, return a Blob with the values.
        let mut error2 = false;
        let mut end = tv_get_number_chk(&argvars[2], Some(&mut error2));
        if error2 {
            return;
        }
        if end < 0 {
            end += len as varnumber_T;
        }
        if end >= len as varnumber_T || idx as varnumber_T > end {
            emsg(&format!("E979: Blob index out of range: {end}"));
            return;
        }
        let end = end as usize;
        let new_blob = tv_blob_alloc();
        {
            let mut bb = b.borrow_mut();
            new_blob.borrow_mut().bv_ga = bb.bv_ga.drain(idx..=end).collect();
        }
        tv_blob_set_ret(rettv, new_blob);
    }
}

/// Port of `list_join_inner()` from `Src/eval/typval.c:992`.
///
/// Stringify each item with `encode_tv2echo` and concatenate into `gap` with
/// `sep` between. The C pre-sizes via a per-item garray (`Join`); the result is
/// identical built directly.
fn list_join_inner(gap: &mut String, l: &list_T, sep: &str) -> i32 {
    let mut first = true;
    for item in &l.lv_items {
        if first {
            first = false;
        } else {
            gap.push_str(sep);
        }
        gap.push_str(&encode_tv2echo(&item.li_tv));
    }
    OK
}

/// Port of `tv_list_join()` from `Src/eval/typval.c:1051`.
///
/// Join a List into `gap` using `sep`.
pub fn tv_list_join(gap: &mut String, l: &list_T, sep: &str) -> i32 {
    // c: if (!tv_list_len(l)) return OK;
    if tv_list_len(l) == 0 {
        return OK;
    }
    list_join_inner(gap, l, sep)
}

/// Port of `f_join()` from `Src/eval/typval.c:1072`.
///
/// "join({list} [, {sep}])" — join a List into a String (items rendered as by
/// `:echo`, so nested Lists/Dicts render structurally, unlike `tv_get_string`).
pub fn f_join(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: if (argvars[0].v_type != VAR_LIST) { emsg(e_listreq); return; }
    if argvars[0].v_type != VAR_LIST {
        emsg("E714: List required");
        return;
    }
    rettv.v_type = VAR_STRING;
    // c: sep defaults to " "; a type error in {sep} yields a NULL string.
    let sep = if argvars.len() < 2 {
        " ".to_string()
    } else {
        match tv_get_string_chk(&argvars[1]) {
            Some(s) => s,
            None => {
                rettv.vval = v_string(String::new());
                return;
            }
        }
    };
    let mut ga = String::new();
    if let v_list(Some(l)) = &argvars[0].vval {
        tv_list_join(&mut ga, &l.borrow(), &sep);
    }
    rettv.vval = v_string(ga);
}

/// Port of `tv_list_slice_or_index()` from `Src/eval/typval.c:932`.
///
/// Subscript a List (`rettv` holds it): a single index → that item; a range →
/// a sub-List. `E684` for an out-of-range single index (when `verbose`).
pub fn tv_list_slice_or_index(
    list: &Rc<RefCell<list_T>>,
    range: bool,
    n1_arg: varnumber_T,
    n2_arg: varnumber_T,
    exclusive: bool,
    rettv: &mut typval_T,
    verbose: bool,
) -> i32 {
    let len = tv_list_len(&list.borrow());
    let mut n1 = n1_arg;
    let mut n2 = n2_arg;

    if n1 < 0 {
        n1 = len as varnumber_T + n1;
    }
    if n1 < 0 || n1 >= len as varnumber_T {
        // c: a range tolerates invalid bounds (→ empty); an index is an error.
        if !range {
            if verbose {
                emsg(&format!("E684: List index out of range: {n1_arg}"));
            }
            return FAIL;
        }
        n1 = len as varnumber_T;
    }
    if range {
        if n2 < 0 {
            n2 = len as varnumber_T + n2;
        } else if n2 >= len as varnumber_T {
            n2 = len as varnumber_T - if exclusive { 0 } else { 1 };
        }
        if exclusive {
            n2 -= 1;
        }
        if n2 < 0 || n2 + 1 < n1 {
            n2 = -1;
        }
        let l = tv_list_slice(&list.borrow(), n1, n2);
        tv_clear(rettv);
        rettv.v_type = VAR_LIST;
        rettv.vval = v_list(Some(l));
    } else {
        // c: copy the item out before clearing rettv (which may free the list).
        let mut var1 = typval_T {
            v_type: VAR_UNKNOWN,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_number(0),
        };
        if let Some(li) = tv_list_find(&list.borrow(), n1 as i32) {
            tv_copy(&li.li_tv, &mut var1);
        }
        tv_clear(rettv);
        *rettv = var1;
    }
    OK
}

/// Per-sort comparison configuration. Port of `sortinfo_T` from
/// `Src/eval/typval.c:46`. Partials / `selfdict` are not modeled.
#[derive(Default)]
pub struct sortinfo_T {
    pub item_compare_ic: bool,
    pub item_compare_lc: bool,
    pub item_compare_numeric: bool,
    pub item_compare_numbers: bool,
    pub item_compare_float: bool,
    pub item_compare_func: Option<String>,
    pub item_compare_func_err: std::cell::Cell<bool>,
}

thread_local! {
    /// Bridge-installed funcref comparator for `sort()`/`uniq()` with a `{func}`
    /// argument: `(name, a, b) -> Some(cmp)`, or `None` on a call/type error.
    /// The value layer can't call user functions itself (that lives in the
    /// bridge), so the bridge installs this hook in `install()`.
    pub static SORT_FUNCREF_HOOK: std::cell::RefCell<Option<fn(&str, &typval_T, &typval_T) -> Option<varnumber_T>>> =
        const { std::cell::RefCell::new(None) };

    /// Generic "call a Funcref/Partial typval with args → result" hook, installed
    /// by the bridge (the value layer can't call user functions itself). Used by
    /// `reduce()`. The first argument is the callee typval (VAR_FUNC name or
    /// VAR_PARTIAL), so bound partial args are honored. `None` on a call error.
    pub static CALL_FUNC_HOOK: std::cell::RefCell<Option<fn(&typval_T, &[typval_T]) -> Option<typval_T>>> =
        const { std::cell::RefCell::new(None) };
}

/// Port of `item_compare()` from `Src/eval/typval.c` — the default comparison.
fn item_compare(tv1: &typval_T, tv2: &typval_T, info: &sortinfo_T) -> i32 {
    if info.item_compare_numbers {
        let v1 = tv_get_number(tv1);
        let v2 = tv_get_number(tv2);
        return if v1 == v2 { 0 } else if v1 > v2 { 1 } else { -1 };
    }
    if info.item_compare_float {
        let v1 = tv_get_float(tv1);
        let v2 = tv_get_float(tv2);
        return if v1 == v2 { 0 } else if v1 > v2 { 1 } else { -1 };
    }
    // c: a String uses its raw value; other types render via encode_tv2string;
    //    a String vs a non-String uses "'" (Vim's documented quirk).
    let p1 = if tv1.v_type == VAR_STRING {
        if tv2.v_type != VAR_STRING || info.item_compare_numeric { "'".to_string() } else { tv_get_string(tv1) }
    } else {
        encode_tv2string(tv1)
    };
    let p2 = if tv2.v_type == VAR_STRING {
        if tv1.v_type != VAR_STRING || info.item_compare_numeric { "'".to_string() } else { tv_get_string(tv2) }
    } else {
        encode_tv2string(tv2)
    };
    if !info.item_compare_numeric {
        // c: lc → strcoll (locale; approximated by byte order); ic → STRICMP; else strcmp.
        let ord = if info.item_compare_lc {
            p1.cmp(&p2)
        } else if info.item_compare_ic {
            p1.to_lowercase().cmp(&p2.to_lowercase())
        } else {
            p1.cmp(&p2)
        };
        match ord {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    } else {
        // c: n1 = strtod(p1); n2 = strtod(p2);
        let cs1 = std::ffi::CString::new(p1).unwrap_or_default();
        let cs2 = std::ffi::CString::new(p2).unwrap_or_default();
        let n1 = unsafe { nix::libc::strtod(cs1.as_ptr(), std::ptr::null_mut()) };
        let n2 = unsafe { nix::libc::strtod(cs2.as_ptr(), std::ptr::null_mut()) };
        if n1 == n2 { 0 } else if n1 > n2 { 1 } else { -1 }
    }
}

// The `keep_zero` index tiebreak the C uses for qsort stability is unnecessary —
// the Rust sort below is already stable; these wrappers mirror the C names.
fn item_compare_keeping_zero(tv1: &typval_T, tv2: &typval_T, info: &sortinfo_T) -> i32 {
    item_compare(tv1, tv2, info)
}
fn item_compare_not_keeping_zero(tv1: &typval_T, tv2: &typval_T, info: &sortinfo_T) -> i32 {
    item_compare(tv1, tv2, info)
}

/// Port of `item_compare2()` — comparison via a user `{func}` (the bridge hook).
fn item_compare2(tv1: &typval_T, tv2: &typval_T, info: &sortinfo_T) -> i32 {
    // c: shortcut after a previous failure; compare all equal.
    if info.item_compare_func_err.get() {
        return 0;
    }
    let name = match &info.item_compare_func {
        Some(n) => n,
        None => return 0,
    };
    // Copy the fn pointer out before calling it — the nested user-function run
    // re-enters install(), which borrows SORT_FUNCREF_HOOK mutably.
    let hook = SORT_FUNCREF_HOOK.with(|h| *h.borrow());
    let res = hook.and_then(|f| f(name, tv1, tv2));
    match res {
        Some(n) => {
            if n > 0 {
                1
            } else if n < 0 {
                -1
            } else {
                0
            }
        }
        None => {
            // c: ITEM_COMPARE_FAIL — record the error, compare equal henceforth.
            info.item_compare_func_err.set(true);
            0
        }
    }
}
fn item_compare2_keeping_zero(tv1: &typval_T, tv2: &typval_T, info: &sortinfo_T) -> i32 {
    item_compare2(tv1, tv2, info)
}
fn item_compare2_not_keeping_zero(tv1: &typval_T, tv2: &typval_T, info: &sortinfo_T) -> i32 {
    item_compare2(tv1, tv2, info)
}

/// Port of `parse_sort_uniq_args()` from `Src/eval/typval.c:1422`.
fn parse_sort_uniq_args(argvars: &[typval_T], info: &mut sortinfo_T) -> i32 {
    if argvars.len() < 2 {
        return OK;
    }
    let a1 = &argvars[1];
    // c: {func} as VAR_FUNC; VAR_PARTIAL not modeled.
    if a1.v_type == VAR_FUNC {
        info.item_compare_func = Some(tv_get_string(a1));
    } else {
        let mut error = false;
        let nr = tv_get_number_chk(a1, Some(&mut error)) as i32;
        if error {
            return FAIL;
        }
        if nr == 1 {
            info.item_compare_ic = true;
        } else if a1.v_type != VAR_NUMBER {
            info.item_compare_func = Some(tv_get_string(a1));
        } else if nr != 0 {
            emsg("E474: Invalid argument");
            return FAIL;
        }
        if let Some(f) = info.item_compare_func.clone() {
            match f.as_str() {
                "" => info.item_compare_func = None, // empty → default sort
                "n" => {
                    info.item_compare_func = None;
                    info.item_compare_numeric = true;
                }
                "N" => {
                    info.item_compare_func = None;
                    info.item_compare_numbers = true;
                }
                "f" => {
                    info.item_compare_func = None;
                    info.item_compare_float = true;
                }
                "i" => {
                    info.item_compare_func = None;
                    info.item_compare_ic = true;
                }
                "l" => {
                    info.item_compare_func = None;
                    info.item_compare_lc = true;
                }
                _ => {}
            }
        }
    }
    if argvars.len() > 2 {
        // c: optional {dict} (selfdict) — validated, but unused (partials unmodeled).
        if tv_check_for_dict_arg(argvars, 2) == FAIL {
            return FAIL;
        }
    }
    OK
}

/// Port of `do_sort()` from `Src/eval/typval.c:1349`. Uses Rust's stable sort,
/// so the C index tiebreak for stability is unnecessary.
fn do_sort(l: &Rc<RefCell<list_T>>, info: &sortinfo_T) {
    let has_func = info.item_compare_func.is_some();
    info.item_compare_func_err.set(false);
    let mut lb = l.borrow_mut();
    let original = lb.lv_items.clone();
    let mut items = std::mem::take(&mut lb.lv_items);
    items.sort_by(|a, b| {
        let r = if has_func {
            item_compare2_not_keeping_zero(&a.li_tv, &b.li_tv, info)
        } else {
            item_compare_not_keeping_zero(&a.li_tv, &b.li_tv, info)
        };
        r.cmp(&0)
    });
    if info.item_compare_func_err.get() {
        // c: on a compare-func error the list is left as it was.
        lb.lv_items = original;
        emsg("E702: Sort compare function failed");
    } else {
        lb.lv_items = items;
    }
    lb.lv_len = lb.lv_items.len() as i32;
}

/// Port of `do_uniq()` from `Src/eval/typval.c:1390` — drop adjacent equal items.
fn do_uniq(l: &Rc<RefCell<list_T>>, info: &sortinfo_T) {
    let has_func = info.item_compare_func.is_some();
    info.item_compare_func_err.set(false);
    let mut lb = l.borrow_mut();
    let items = std::mem::take(&mut lb.lv_items);
    let mut out: Vec<listitem_T> = Vec::with_capacity(items.len());
    let mut i = 0;
    while i < items.len() {
        let dup = if let Some(prev) = out.last() {
            let r = if has_func {
                item_compare2_keeping_zero(&prev.li_tv, &items[i].li_tv, info)
            } else {
                item_compare_keeping_zero(&prev.li_tv, &items[i].li_tv, info)
            };
            if info.item_compare_func_err.get() {
                emsg("E882: Uniq compare function failed");
                break;
            }
            r == 0
        } else {
            false
        };
        if !dup {
            out.push(items[i].clone());
        }
        i += 1;
    }
    // c: on a compare-func error it stops; keep the not-yet-processed items.
    while i < items.len() {
        out.push(items[i].clone());
        i += 1;
    }
    lb.lv_items = out;
    lb.lv_len = lb.lv_items.len() as i32;
}

/// Port of `do_sort_uniq()` from `Src/eval/typval.c` — shared `sort()`/`uniq()`.
fn do_sort_uniq(argvars: &[typval_T], rettv: &mut typval_T, sort: bool) {
    // c: if (argvars[0].v_type != VAR_LIST) semsg(e_listarg, ...);
    if argvars[0].v_type != VAR_LIST {
        emsg(&format!(
            "E686: Argument of {} must be a List",
            if sort { "sort()" } else { "uniq()" }
        ));
        return;
    }
    let l = match &argvars[0].vval {
        v_list(Some(l)) => l.clone(),
        _ => {
            rettv.v_type = VAR_LIST;
            rettv.vval = v_list(None);
            return;
        }
    };
    let arg_errmsg = if sort { "sort() argument" } else { "uniq() argument" };
    if value_check_lock(l.borrow().lv_lock, Some(arg_errmsg), TV_TRANSLATE) {
        return;
    }
    // c: tv_list_set_ret(rettv, l);
    rettv.v_type = VAR_LIST;
    rettv.vval = v_list(Some(l.clone()));
    if tv_list_len(&l.borrow()) <= 1 {
        return; // short list sorts pretty quickly
    }
    let mut info = sortinfo_T::default();
    if parse_sort_uniq_args(argvars, &mut info) == FAIL {
        return;
    }
    if sort {
        do_sort(&l, &info);
    } else {
        do_uniq(&l, &info);
    }
}

/// Port of `f_sort()` from `Src/eval/typval.c`.
pub fn f_sort(argvars: &[typval_T], rettv: &mut typval_T) {
    do_sort_uniq(argvars, rettv, true);
}

/// Port of `f_uniq()` from `Src/eval/typval.c`.
pub fn f_uniq(argvars: &[typval_T], rettv: &mut typval_T) {
    do_sort_uniq(argvars, rettv, false);
}

// ── Callback (Src/eval/typval.c) ──
//
// `Callback` in C is a union of {funcref name, partial_T*, LuaRef} + a type tag.
// vimlrs models neither partials nor Lua, so a `Callback` here is funcref-or-none
// (the same adaptation as modeling funcrefs as `VAR_FUNC` strings). The C
// `func_ref`/`func_unref` refcounting is vestigial (functions live in the bridge
// `FUNCTIONS` registry), so copy is a clone and free is a reset.

/// Port of `Callback` from `Src/eval/typval_defs.h:64` (funcref-only model).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Callback {
    /// `kCallbackNone`.
    #[default]
    None,
    /// `kCallbackFuncref` — the function name.
    Funcref(String),
}

/// Port of `callback_copy()` from `Src/eval/typval.c`.
pub fn callback_copy(dest: &mut Callback, src: &Callback) {
    // c: dup the funcref name + func_ref() (refcount vestigial here).
    *dest = src.clone();
}

/// Port of `callback_free()` from `Src/eval/typval.c`.
pub fn callback_free(callback: &mut Callback) {
    // c: func_unref + free, then type = kCallbackNone.
    *callback = Callback::None;
}

/// Port of `callback_put()` from `Src/eval/typval.c` — write a `Callback` into a
/// `typval_T` (a Funcref, or v:null for None).
pub fn callback_put(cb: &Callback, tv: &mut typval_T) {
    match cb {
        Callback::Funcref(name) => {
            tv.v_type = VAR_FUNC;
            tv.vval = v_string(name.clone());
        }
        Callback::None => {
            tv.v_type = VAR_SPECIAL;
            tv.vval =
                v_special(crate::ported::eval::typval_defs_h::SpecialVarValue::kSpecialVarNull);
        }
    }
}

/// Port of `callback_to_string()` from `Src/eval/typval.c`.
pub fn callback_to_string(cb: &Callback) -> String {
    // c: snprintf("<vim function: %s>", funcref); None → "".
    match cb {
        Callback::Funcref(name) => format!("<vim function: {name}>"),
        Callback::None => String::new(),
    }
}

/// Port of `tv_callback_equal()` from `Src/eval/typval.c`.
pub fn tv_callback_equal(cb1: &Callback, cb2: &Callback) -> bool {
    // c: same type, and equal funcref names.
    cb1 == cb2
}

/// Port of `tv_dict_get_callback()` from `Src/eval/typval.c` — read the
/// `Callback` at `key`. Returns `false` (with `E6000`) if the value is present
/// but is not a function/function-name; a missing key yields `true` + `None`.
pub fn tv_dict_get_callback(d: &dict_T, key: &str, result: &mut Callback) -> bool {
    *result = Callback::None;
    let tv = match tv_dict_find(d, key) {
        Some(tv) => tv,
        None => return true,
    };
    // c: callback_from_typval — VAR_FUNC / VAR_STRING name → a funcref Callback.
    match tv.v_type {
        VAR_FUNC | VAR_STRING => {
            *result = Callback::Funcref(tv_get_string(tv));
            true
        }
        _ => {
            emsg("E6000: Argument is not a function or function name");
            false
        }
    }
}

/// Port of `callback_from_typval()` from `Src/eval/eval.c` — read a `Callback`
/// from a typval (a Funcref/name; `v:none`/`v:null` → None; else fail).
pub fn callback_from_typval(callback: &mut Callback, tv: &typval_T) -> bool {
    match tv.v_type {
        VAR_FUNC => {
            *callback = Callback::Funcref(tv_get_string(tv));
            true
        }
        VAR_STRING => {
            let s = tv_get_string(tv);
            *callback = if s.is_empty() { Callback::None } else { Callback::Funcref(s) };
            true
        }
        VAR_SPECIAL => {
            *callback = Callback::None;
            true
        }
        _ => false,
    }
}

// ── Dict watchers (Src/eval/typval.c) ──
//
// The C `DictWatcher` lives in a per-dict QUEUE (`dict->watchers`); here it is a
// `Vec<DictWatcher>` on `dict_T.dv_watchers`. `busy`/`needs_free` (re-entrancy
// bookkeeping) are unneeded: `tv_dict_watcher_notify` copies the matching
// callbacks out before invoking them, so the dict is never borrowed during a call.

/// Port of `DictWatcher` from `Src/eval/typval_defs.h` (funcref-only Callback).
#[derive(Clone, Debug, Default)]
pub struct DictWatcher {
    pub callback: Callback,
    pub key_pattern: String,
}

/// Port of `tv_dict_watcher_add()` from `Src/eval/typval.c`.
pub fn tv_dict_watcher_add(dict: &Rc<RefCell<dict_T>>, key_pattern: &str, callback: Callback) {
    dict.borrow_mut()
        .dv_watchers
        .push(DictWatcher { callback, key_pattern: key_pattern.to_string() });
}

/// Port of `tv_dict_watcher_matches()` from `Src/eval/typval.c` — a trailing `*`
/// is a prefix match, else an exact match.
fn tv_dict_watcher_matches(watcher: &DictWatcher, key: &str) -> bool {
    let p = &watcher.key_pattern;
    if let Some(prefix) = p.strip_suffix('*') {
        key.starts_with(prefix)
    } else {
        key == p
    }
}

/// Port of `tv_dict_watcher_free()` from `Src/eval/typval.c` — drop the watcher
/// (its `Callback`/pattern are freed by Rust's drop).
pub fn tv_dict_watcher_free(_watcher: DictWatcher) {}

/// Port of `tv_dict_watcher_remove()` from `Src/eval/typval.c` — remove the
/// watcher whose callback + key pattern match; `false` if none found.
pub fn tv_dict_watcher_remove(
    dict: &Rc<RefCell<dict_T>>,
    key_pattern: &str,
    callback: &Callback,
) -> bool {
    let mut d = dict.borrow_mut();
    if let Some(i) = d
        .dv_watchers
        .iter()
        .position(|w| tv_callback_equal(&w.callback, callback) && w.key_pattern == key_pattern)
    {
        let w = d.dv_watchers.remove(i);
        tv_dict_watcher_free(w);
        true
    } else {
        false
    }
}

/// Port of `tv_dict_watcher_notify()` from `Src/eval/typval.c` — call every
/// watcher whose pattern matches `key` with `(dict, key, {new, old})`.
pub fn tv_dict_watcher_notify(
    dict: &Rc<RefCell<dict_T>>,
    key: &str,
    newtv: Option<&typval_T>,
    oldtv: Option<&typval_T>,
) {
    // Copy matching callbacks out so the dict isn't borrowed during the calls
    // (the C uses watcher->busy for the same re-entrancy safety).
    let matching: Vec<Callback> = dict
        .borrow()
        .dv_watchers
        .iter()
        .filter(|w| tv_dict_watcher_matches(w, key))
        .map(|w| w.callback.clone())
        .collect();
    if matching.is_empty() {
        return;
    }
    // c: argv[2] is a dict {"new": newtv, "old": oldtv} (each only if present).
    let change = tv_dict_alloc();
    if let Some(n) = newtv {
        tv_dict_add_tv(&mut change.borrow_mut(), "new", n.clone());
    }
    if let Some(o) = oldtv {
        if o.v_type != VAR_UNKNOWN {
            tv_dict_add_tv(&mut change.borrow_mut(), "old", o.clone());
        }
    }
    let mk = |t, v| typval_T { v_type: t, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v };
    let dict_tv = mk(VAR_DICT, v_dict(Some(dict.clone())));
    let key_tv = mk(VAR_STRING, v_string(key.to_string()));
    let change_tv = mk(VAR_DICT, v_dict(Some(change)));
    let hook = CALL_FUNC_HOOK.with(|h| *h.borrow());
    for cb in matching {
        if let Callback::Funcref(name) = cb {
            if let Some(f) = hook {
                let func_tv = mk(VAR_FUNC, v_string(name.clone()));
                let _ = f(&func_tv, &[dict_tv.clone(), key_tv.clone(), change_tv.clone()]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nr(n: varnumber_T) -> typval_T {
        typval_T { v_type: VAR_NUMBER, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v_number(n) }
    }

    #[test]
    fn dict_add_fails_on_existing_key_but_add_tv_overwrites() {
        let d = tv_dict_alloc();
        let mut db = d.borrow_mut();
        // tv_dict_add: first insert OK, duplicate FAIL with the original kept.
        assert_eq!(tv_dict_add_nr(&mut db, "a", 1), OK);
        assert_eq!(tv_dict_add_nr(&mut db, "a", 2), FAIL);
        assert_eq!(tv_dict_get_number(&db, "a"), 1);
        // tv_dict_add_tv overwrites unconditionally.
        tv_dict_add_tv(&mut db, "a", nr(9));
        assert_eq!(tv_dict_get_number(&db, "a"), 9);
    }

    #[test]
    fn dict_typed_getters_and_defaults() {
        let d = tv_dict_alloc();
        let mut db = d.borrow_mut();
        tv_dict_add_str(&mut db, "s", "hi");
        tv_dict_add_bool(&mut db, "b", kBoolVarTrue);
        assert!(tv_dict_has_key(&db, "s"));
        assert!(!tv_dict_has_key(&db, "missing"));
        assert_eq!(tv_dict_get_string(&db, "s"), Some("hi".to_string()));
        assert_eq!(tv_dict_get_string(&db, "missing"), None);
        assert_eq!(tv_dict_get_number_def(&db, "missing", 7), 7);
        assert_eq!(tv_dict_get_bool(&db, "b", 0), 1);
        assert_eq!(tv_dict_get_bool(&db, "missing", 0), 0);
    }

    #[test]
    fn list_find_uidx_reverse_and_copy() {
        let l = tv_list_alloc(0);
        {
            let mut lb = l.borrow_mut();
            tv_list_append_number(&mut lb, 10);
            tv_list_append_number(&mut lb, 20);
            tv_list_append_number(&mut lb, 30);
        }
        let lb = l.borrow();
        // uidx: negative counts from the end, out-of-range -> -1.
        assert_eq!(tv_list_uidx(&lb, 0), 0);
        assert_eq!(tv_list_uidx(&lb, -1), 2);
        assert_eq!(tv_list_uidx(&lb, 3), -1);
        assert_eq!(tv_list_uidx(&lb, -4), -1);
        // find_nr by index (incl. negative).
        assert_eq!(tv_list_find_nr(&lb, 1, None), 20);
        assert_eq!(tv_list_find_nr(&lb, -1, None), 30);
        let mut err = false;
        assert_eq!(tv_list_find_nr(&lb, 9, Some(&mut err)), -1);
        assert!(err);
        // find_str coerces; out-of-range -> None.
        assert_eq!(tv_list_find_str(&lb, 0), Some("10".to_string()));
        assert_eq!(tv_list_find_str(&lb, 9), None);
        drop(lb);
        // reverse in place.
        tv_list_reverse(&mut l.borrow_mut());
        assert_eq!(tv_list_find_nr(&l.borrow(), 0, None), 30);
        assert_eq!(tv_list_find_nr(&l.borrow(), 2, None), 10);
        // tv_copy clears the lock and shares the Rc (refcount bump).
        let src = typval_T {
            v_type: VAR_LIST,
            v_lock: VarLockStatus::VAR_LOCKED,
            vval: v_list(Some(l.clone())),
        };
        let mut dst = nr(0);
        tv_copy(&src, &mut dst);
        assert_eq!(dst.v_type, VAR_LIST);
        assert_eq!(dst.v_lock, VarLockStatus::VAR_UNLOCKED);
        if let v_list(Some(d)) = &dst.vval {
            assert!(Rc::ptr_eq(d, &l));
        } else {
            panic!("expected shared list");
        }
    }

    fn str_tv(s: &str) -> typval_T {
        typval_T {
            v_type: VAR_STRING,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_string(s.to_string()),
        }
    }

    fn blob_tv() -> typval_T {
        typval_T {
            v_type: VAR_BLOB,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_blob(Some(tv_blob_alloc())),
        }
    }

    #[test]
    fn callback_family() {
        let func = |n: &str| typval_T {
            v_type: VAR_FUNC,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_string(n.to_string()),
        };
        // copy / to_string / equal.
        let mut c = Callback::None;
        callback_copy(&mut c, &Callback::Funcref("Foo".to_string()));
        assert_eq!(c, Callback::Funcref("Foo".to_string()));
        assert_eq!(callback_to_string(&c), "<vim function: Foo>");
        assert!(tv_callback_equal(&c, &Callback::Funcref("Foo".to_string())));
        assert!(!tv_callback_equal(&c, &Callback::Funcref("Bar".to_string())));
        // put → a VAR_FUNC typval.
        let mut tv = nr(0);
        callback_put(&c, &mut tv);
        assert!(matches!((tv.v_type, &tv.vval), (VAR_FUNC, v_string(s)) if s == "Foo"));
        // free → None.
        callback_free(&mut c);
        assert_eq!(c, Callback::None);
        // tv_dict_get_callback: present func key → Funcref; missing → None (ok).
        let d = tv_dict_alloc();
        tv_dict_add_tv(&mut d.borrow_mut(), "cb", func("Handler"));
        let mut r = Callback::None;
        assert!(tv_dict_get_callback(&d.borrow(), "cb", &mut r));
        assert_eq!(r, Callback::Funcref("Handler".to_string()));
        let mut r2 = Callback::Funcref("x".to_string());
        assert!(tv_dict_get_callback(&d.borrow(), "nope", &mut r2));
        assert_eq!(r2, Callback::None);
    }

    #[test]
    fn lock_family() {
        use VarLockStatus::*;
        // value_check_lock: unlocked → false (no error); locked/fixed → true.
        assert!(!value_check_lock(VAR_UNLOCKED, None, TV_TRANSLATE));
        assert!(value_check_lock(VAR_LOCKED, None, TV_TRANSLATE));
        assert!(value_check_lock(VAR_FIXED, Some("x"), TV_TRANSLATE));

        // tv_item_lock deep-locks a list and its items; tv_check_lock detects it.
        let l = tv_list_alloc(0);
        {
            let mut lb = l.borrow_mut();
            lb.lv_items = vec![listitem_T { li_tv: nr(1) }, listitem_T { li_tv: nr(2) }];
            lb.lv_len = 2;
        }
        let mut tv = typval_T {
            v_type: VAR_LIST,
            v_lock: VAR_UNLOCKED,
            vval: v_list(Some(l.clone())),
        };
        tv_item_lock(&mut tv, -1, true, false);
        assert_eq!(tv.v_lock, VAR_LOCKED);
        assert_eq!(l.borrow().lv_lock, VAR_LOCKED);
        assert_eq!(l.borrow().lv_items[0].li_tv.v_lock, VAR_LOCKED); // deep
        assert!(tv_check_lock(&tv, None, TV_TRANSLATE));

        // ...and unlock again.
        tv_item_lock(&mut tv, -1, false, false);
        assert_eq!(l.borrow().lv_lock, VAR_UNLOCKED);
        assert!(!tv_check_lock(&tv, None, TV_TRANSLATE));

        // VAR_FIXED never changes.
        let mut fixed = nr(1);
        fixed.v_lock = VAR_FIXED;
        tv_item_lock(&mut fixed, 1, false, false);
        assert_eq!(fixed.v_lock, VAR_FIXED);
    }

    #[test]
    fn blob_index_and_slice() {
        let mk = || {
            let b = tv_blob_alloc();
            b.borrow_mut().bv_ga = vec![10, 20, 30, 40];
            typval_T {
                v_type: VAR_BLOB,
                v_lock: VarLockStatus::VAR_UNLOCKED,
                vval: v_blob(Some(b)),
            }
        };

        // index b[2] -> 30
        let mut rt = mk();
        let b = match &rt.vval {
            v_blob(Some(b)) => b.clone(),
            _ => unreachable!(),
        };
        assert_eq!(tv_blob_slice_or_index(&b.borrow(), false, 2, 0, false, &mut rt), OK);
        assert!(matches!((rt.v_type, &rt.vval), (VAR_NUMBER, v_number(30))));

        // negative index b[-1] -> 40
        let mut rt = mk();
        let b = match &rt.vval {
            v_blob(Some(b)) => b.clone(),
            _ => unreachable!(),
        };
        assert_eq!(tv_blob_slice_or_index(&b.borrow(), false, -1, 0, false, &mut rt), OK);
        assert!(matches!((rt.v_type, &rt.vval), (VAR_NUMBER, v_number(40))));

        // out of range b[10] -> E979 / FAIL
        let mut rt = mk();
        let b = match &rt.vval {
            v_blob(Some(b)) => b.clone(),
            _ => unreachable!(),
        };
        assert_eq!(tv_blob_slice_or_index(&b.borrow(), false, 10, 0, false, &mut rt), FAIL);

        // slice b[1:2] (inclusive) -> [20, 30]
        let mut rt = mk();
        let b = match &rt.vval {
            v_blob(Some(b)) => b.clone(),
            _ => unreachable!(),
        };
        assert_eq!(tv_blob_slice_or_index(&b.borrow(), true, 1, 2, false, &mut rt), OK);
        match (&rt.v_type, &rt.vval) {
            (VAR_BLOB, v_blob(Some(nb))) => assert_eq!(nb.borrow().bv_ga, vec![20, 30]),
            _ => panic!("expected a blob slice"),
        }
    }

    #[test]
    fn arg_type_checks_required_and_optional() {
        let args = [nr(5), str_tv("hi")];
        // required: right type OK, wrong type FAIL.
        assert_eq!(tv_check_for_number_arg(&args, 0), OK);
        assert_eq!(tv_check_for_string_arg(&args, 1), OK);
        assert_eq!(tv_check_for_string_arg(&args, 0), FAIL);
        assert_eq!(tv_check_for_number_arg(&args, 1), FAIL);
        // missing required arg (idx past end == VAR_UNKNOWN sentinel) -> FAIL.
        assert_eq!(tv_check_for_string_arg(&args, 5), FAIL);
        // optional: absent (past end) -> OK; present wrong type -> FAIL.
        assert_eq!(tv_check_for_opt_string_arg(&args, 5), OK);
        assert_eq!(tv_check_for_opt_string_arg(&args, 1), OK);
        assert_eq!(tv_check_for_opt_number_arg(&args, 1), FAIL);
        // bool: a Number 0/1 passes, other numbers fail.
        assert_eq!(tv_check_for_bool_arg(&[nr(1)], 0), OK);
        assert_eq!(tv_check_for_bool_arg(&[nr(0)], 0), OK);
        assert_eq!(tv_check_for_bool_arg(&[nr(2)], 0), FAIL);
        // non-empty string.
        assert_eq!(tv_check_for_nonempty_string_arg(&[str_tv("x")], 0), OK);
        assert_eq!(tv_check_for_nonempty_string_arg(&[str_tv("")], 0), FAIL);
        // single-value checks.
        assert!(tv_check_str_or_nr(&nr(1)));
        assert!(tv_check_str(&nr(1)));
        assert!(tv_check_num(&str_tv("3")));
        // "or" arg checks: accept either type, reject the rest.
        assert_eq!(tv_check_for_string_or_number_arg(&[nr(1)], 0), OK);
        assert_eq!(tv_check_for_string_or_number_arg(&[str_tv("x")], 0), OK);
        assert_eq!(tv_check_for_string_or_number_arg(&[blob_tv()], 0), FAIL);
        assert_eq!(tv_check_for_buffer_arg(&[nr(2)], 0), OK);
        assert_eq!(tv_check_for_lnum_arg(&[str_tv("$")], 0), OK);
        assert_eq!(tv_check_for_list_or_blob_arg(&[blob_tv()], 0), OK);
        assert_eq!(tv_check_for_list_or_blob_arg(&[nr(1)], 0), FAIL);
        // optional or-list: absent OK, present-string OK, present-number FAIL.
        assert_eq!(tv_check_for_opt_string_or_list_arg(&[str_tv("x")], 5), OK);
        assert_eq!(tv_check_for_opt_string_or_list_arg(&[str_tv("x")], 0), OK);
        assert_eq!(tv_check_for_opt_string_or_list_arg(&[nr(1)], 0), FAIL);
    }

    #[test]
    fn blob_get_set_copy_and_ranges() {
        let b = tv_blob_alloc();
        {
            let mut bb = b.borrow_mut();
            // set_append grows by one when idx == len; ignores idx past end+1.
            tv_blob_set_append(&mut bb, 0, 0xde);
            tv_blob_set_append(&mut bb, 1, 0xad);
            tv_blob_set_append(&mut bb, 2, 0xbe);
            tv_blob_set_append(&mut bb, 9, 0xff); // idx > len -> ignored
            assert_eq!(tv_blob_len(&bb), 3);
            assert_eq!(tv_blob_get(&bb, 0), 0xde);
            tv_blob_set(&mut bb, 0, 0x00);
            assert_eq!(tv_blob_get(&bb, 0), 0x00);
        }
        // copy duplicates the bytes into a fresh blob (not the same Rc).
        let mut dst = nr(0);
        tv_blob_copy(Some(&b), &mut dst);
        if let v_blob(Some(d)) = &dst.vval {
            assert!(!Rc::ptr_eq(d, &b));
            assert_eq!(d.borrow().bv_ga, b.borrow().bv_ga);
        } else {
            panic!("expected blob");
        }
        // NULL source -> empty/NULL blob.
        let mut dst2 = nr(0);
        tv_blob_copy(None, &mut dst2);
        assert!(matches!(dst2.vval, v_blob(None)));
        // set_range: length must match, else FAIL.
        let src = tv_blob_alloc();
        src.borrow_mut().bv_ga = vec![1, 2];
        assert_eq!(tv_blob_set_range(&mut b.borrow_mut(), 0, 1, &src.borrow()), OK);
        assert_eq!(b.borrow().bv_ga[..2], [1, 2]);
        assert_eq!(tv_blob_set_range(&mut b.borrow_mut(), 0, 2, &src.borrow()), FAIL);
    }

    #[test]
    fn dict_extend_clear_env_and_tv_clear() {
        let d1 = tv_dict_alloc();
        let d2 = tv_dict_alloc();
        {
            let mut a = d1.borrow_mut();
            tv_dict_add_nr(&mut a, "x", 1);
            let mut b = d2.borrow_mut();
            tv_dict_add_nr(&mut b, "x", 2);
            tv_dict_add_str(&mut b, "y", "hi");
        }
        // keep: existing key kept; new key added.
        tv_dict_extend(&mut d1.borrow_mut(), &d2.borrow(), "keep");
        assert_eq!(tv_dict_get_number(&d1.borrow(), "x"), 1);
        assert!(tv_dict_has_key(&d1.borrow(), "y"));
        // force: existing key overridden.
        tv_dict_extend(&mut d1.borrow_mut(), &d2.borrow(), "force");
        assert_eq!(tv_dict_get_number(&d1.borrow(), "x"), 2);
        // to_env renders KEY=VALUE.
        let env = tv_dict_to_env(&d1.borrow());
        assert!(env.contains(&"y=hi".to_string()));
        // clear empties it.
        tv_dict_clear(&mut d1.borrow_mut());
        assert_eq!(tv_dict_len(&d1.borrow()), 0);
        // tv_clear resets to VAR_UNKNOWN.
        let mut t = str_tv("gone");
        tv_clear(&mut t);
        assert_eq!(t.v_type, VAR_UNKNOWN);
        // islocked reflects the container lock.
        let l = tv_list_alloc(0);
        l.borrow_mut().lv_lock = VarLockStatus::VAR_LOCKED;
        let tv = typval_T {
            v_type: VAR_LIST,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_list(Some(l)),
        };
        assert!(tv_islocked(&tv));
    }

    #[test]
    fn tv2bool_matches_vim_truthiness() {
        assert!(!tv2bool(&nr(0)));
        assert!(tv2bool(&nr(5)));
        assert!(!tv2bool(&typval_T {
            v_type: VAR_STRING,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_string(String::new()),
        }));
        assert!(tv2bool(&typval_T {
            v_type: VAR_STRING,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_string("x".to_string()),
        }));
        // Empty list is falsy; a one-item list is truthy.
        let l = tv_list_alloc(0);
        assert!(!tv2bool(&typval_T {
            v_type: VAR_LIST,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_list(Some(l.clone())),
        }));
        tv_list_append_number(&mut l.borrow_mut(), 1);
        assert!(tv2bool(&typval_T {
            v_type: VAR_LIST,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_list(Some(l)),
        }));
    }
}
