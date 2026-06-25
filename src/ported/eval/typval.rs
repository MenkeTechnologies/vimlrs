//! Port of `src/nvim/eval/typval.c` (vendored at `csrc/eval/typval.c`).
//!
//! Vimscript value accessors and container operations. Function names,
//! signatures, and control flow match the C source (PORT.md Rules A/B/4).
#![allow(non_snake_case, non_upper_case_globals)]

use std::cell::RefCell;
use std::rc::Rc;

use crate::ported::charset::{vim_str2nr, STR2NR_ALL};
use crate::ported::eval::typval_defs_h::{
    blob_T, dict_T, list_T, listitem_T, typval_T, typval_vval_union::*, varnumber_T, BoolVarValue::*,
    VarLockStatus, VarType::*,
};
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
pub fn tv_equal(tv1: &typval_T, tv2: &typval_T, _ic: bool) -> bool {
    match (&tv1.vval, &tv2.vval) {
        (v_number(a), v_number(b)) => a == b,
        (v_float(a), v_float(b)) => a == b,
        (v_string(a), v_string(b)) => {
            // VAR_FUNC and VAR_STRING both use v_string; compare only same type.
            tv1.v_type == tv2.v_type && a == b
        }
        (v_bool(a), v_bool(b)) => a == b,
        (v_special(a), v_special(b)) => a == b,
        (v_list(Some(a)), v_list(Some(b))) => tv_list_equal(a, b, _ic),
        (v_list(None), v_list(None)) => true,
        (v_dict(Some(a)), v_dict(Some(b))) => tv_dict_equal(a, b, _ic),
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
