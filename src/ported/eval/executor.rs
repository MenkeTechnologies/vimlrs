//! Port of `src/nvim/eval/executor.c` (vendored at `csrc/eval/executor.c`).
//!
//! The compound-assignment operators (`tv_op` family) behind `:let x += y`,
//! `-= *= /= %=` and `.=`. `eexe_mod_op` mutates the left operand in place,
//! reproducing Vim's reference-type semantics (a `List`/`Blob` `+=` extends the
//! existing value, so aliases observe the change).
#![allow(non_snake_case, non_upper_case_globals)]

use crate::ported::eval::typval::{
    tv_blob_len, tv_clear, tv_get_number, tv_get_string, tv_list_copy, tv_list_extend, tv_list_ref,
};
use crate::ported::eval::typval_defs_h::{typval_T, typval_vval_union::*, VarType::*};
use crate::ported::eval::{num_divide, num_modulus};
use crate::ported::eval_h::{FAIL, OK};
use crate::ported::message::semsg;

/// Port of `tv_op_blob()` from `Src/eval/executor.c:20` — "blob1 += blob2".
fn tv_op_blob(tv1: &mut typval_T, tv2: &typval_T, op: char) -> i32 {
    if op != '+' || tv2.v_type != VAR_BLOB {
        return FAIL;
    }
    // Blob += Blob
    let b2 = match &tv2.vval {
        v_blob(Some(b)) => b.clone(),
        _ => return OK, // tv2 blob is NULL → nothing to append
    };
    match &tv1.vval {
        v_blob(Some(b1)) => {
            // c: ga_grow + memmove b2 onto the end of b1.
            // Snapshot first to stay borrow-safe when b1 IS b2 (`b += b`).
            let appended = b2.borrow().bv_ga.clone();
            if tv_blob_len(&b2.borrow()) > 0 {
                b1.borrow_mut().bv_ga.extend_from_slice(&appended);
            }
        }
        _ => {
            // c: tv1 blob is NULL → share tv2's blob.
            b2.borrow_mut().bv_refcount += 1;
            tv1.vval = v_blob(Some(b2));
        }
    }
    OK
}

/// Port of `tv_op_list()` from `Src/eval/executor.c:54` — "list1 += list2".
fn tv_op_list(tv1: &mut typval_T, tv2: &typval_T, op: char) -> i32 {
    if op != '+' || tv2.v_type != VAR_LIST {
        return FAIL;
    }
    let l2 = match &tv2.vval {
        v_list(Some(l)) => l.clone(),
        _ => return OK, // tv2 list is NULL → nothing to extend
    };
    match &tv1.vval {
        v_list(Some(l1)) => {
            // c: tv_list_extend(l1, l2, NULL). Snapshot l2 when it IS l1
            // (`l += l`) to avoid a double borrow of the same RefCell.
            let snapshot = tv_list_copy(&l2, false);
            tv_list_extend(&mut l1.borrow_mut(), &snapshot.borrow(), None);
        }
        _ => {
            // c: tv1 list is NULL → share tv2's list.
            tv_list_ref(&mut l2.borrow_mut());
            tv1.vval = v_list(Some(l2));
        }
    }
    OK
}

/// Port of `tv_op_number()` from `Src/eval/executor.c:80` — `nr op= nr/float`.
fn tv_op_number(tv1: &mut typval_T, tv2: &typval_T, op: char) -> i32 {
    let n = tv_get_number(tv1);
    if let (VAR_FLOAT, v_float(f2)) = (tv2.v_type, &tv2.vval) {
        if op == '%' {
            return FAIL;
        }
        let mut f = n as f64;
        match op {
            '+' => f += f2,
            '-' => f -= f2,
            '*' => f *= f2,
            '/' => f /= f2,
            _ => {}
        }
        tv_clear(tv1);
        tv1.v_type = VAR_FLOAT;
        tv1.vval = v_float(f);
    } else {
        let n2 = tv_get_number(tv2);
        let r = match op {
            '+' => n + n2,
            '-' => n - n2,
            '*' => n * n2,
            '/' => num_divide(n, n2),
            '%' => num_modulus(n, n2),
            _ => n,
        };
        tv_clear(tv1);
        tv1.v_type = VAR_NUMBER;
        tv1.vval = v_number(r);
    }
    OK
}

/// Port of `tv_op_string()` from `Src/eval/executor.c:125` — "str1 .= str2".
fn tv_op_string(tv1: &mut typval_T, tv2: &typval_T, _op: char) -> i32 {
    if tv2.v_type == VAR_FLOAT {
        return FAIL;
    }
    // c: s = concat_str(tv_get_string(tv1), tv_get_string_buf(tv2)).
    let s = format!("{}{}", tv_get_string(tv1), tv_get_string(tv2));
    tv_clear(tv1);
    tv1.v_type = VAR_STRING;
    tv1.vval = v_string(s);
    OK
}

/// Port of `tv_op_nr_or_string()` from `Src/eval/executor.c:151`.
fn tv_op_nr_or_string(tv1: &mut typval_T, tv2: &typval_T, op: char) -> i32 {
    if tv2.v_type == VAR_LIST {
        return FAIL;
    }
    if matches!(op, '+' | '-' | '*' | '/' | '%') {
        return tv_op_number(tv1, tv2, op);
    }
    tv_op_string(tv1, tv2, op)
}

/// Port of `tv_op_float()` from `Src/eval/executor.c:167` — `f1 op= f2`.
fn tv_op_float(tv1: &mut typval_T, tv2: &typval_T, op: char) -> i32 {
    if op == '%'
        || op == '.'
        || !matches!(tv2.v_type, VAR_FLOAT | VAR_NUMBER | VAR_STRING)
    {
        return FAIL;
    }
    let f = match (tv2.v_type, &tv2.vval) {
        (VAR_FLOAT, v_float(f)) => *f,
        _ => tv_get_number(tv2) as f64,
    };
    if let v_float(f1) = &mut tv1.vval {
        match op {
            '+' => *f1 += f,
            '-' => *f1 -= f,
            '*' => *f1 *= f,
            '/' => *f1 /= f,
            _ => {}
        }
    }
    OK
}

/// Port of `eexe_mod_op()` from `Src/eval/executor.c:201` — apply `tv1 op= tv2`
/// in place (`+= -= *= /= %= .=`). Returns OK or FAIL (with E734 on a bad combo).
pub fn eexe_mod_op(tv1: &mut typval_T, tv2: &typval_T, op: char) -> i32 {
    // c: e_letwrong — "E734: Wrong variable type for %s=".
    let letwrong = || semsg(&format!("E734: Wrong variable type for {op}="));
    // Can't do anything with a Funcref or Dict on the right; v:true/null only
    // work with "..=".
    if tv2.v_type == VAR_FUNC
        || tv2.v_type == VAR_DICT
        || ((tv2.v_type == VAR_BOOL || tv2.v_type == VAR_SPECIAL) && op == '.')
    {
        letwrong();
        return FAIL;
    }

    let retval = match tv1.v_type {
        VAR_DICT | VAR_FUNC | VAR_PARTIAL | VAR_BOOL | VAR_SPECIAL => FAIL,
        VAR_BLOB => tv_op_blob(tv1, tv2, op),
        VAR_LIST => tv_op_list(tv1, tv2, op),
        VAR_NUMBER | VAR_STRING => tv_op_nr_or_string(tv1, tv2, op),
        VAR_FLOAT => tv_op_float(tv1, tv2, op),
        VAR_UNKNOWN => FAIL,
    };

    if retval != OK {
        letwrong();
    }
    retval
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::eval::typval::{tv_list_alloc, tv_list_append_number};
    use crate::ported::eval::typval_defs_h::{blob_T, list_T, VarLockStatus};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn num(n: i64) -> typval_T {
        typval_T { v_type: VAR_NUMBER, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v_number(n) }
    }
    fn s(t: &str) -> typval_T {
        typval_T { v_type: VAR_STRING, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v_string(t.to_string()) }
    }

    #[test]
    fn number_and_string_ops() {
        let mut a = num(5);
        assert_eq!(eexe_mod_op(&mut a, &num(3), '+'), OK);
        assert!(matches!(a.vval, v_number(8)));
        assert_eq!(eexe_mod_op(&mut a, &num(2), '*'), OK);
        assert!(matches!(a.vval, v_number(16)));
        let mut t = s("foo");
        assert_eq!(eexe_mod_op(&mut t, &s("bar"), '.'), OK);
        assert!(matches!(&t.vval, v_string(x) if x == "foobar"));
        // number .= number → string concat ("12".concat is via tv_op_string).
        let mut n = num(1);
        assert_eq!(eexe_mod_op(&mut n, &num(2), '.'), OK);
        assert!(matches!(&n.vval, v_string(x) if x == "12"));
    }

    #[test]
    fn list_extends_in_place_for_aliases() {
        let l = tv_list_alloc(0);
        tv_list_append_number(&mut l.borrow_mut(), 1);
        let a = typval_T { v_type: VAR_LIST, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v_list(Some(l.clone())) };
        let b = a.clone(); // alias (shared Rc)
        let src = tv_list_alloc(0);
        tv_list_append_number(&mut src.borrow_mut(), 2);
        let src_tv = typval_T { v_type: VAR_LIST, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v_list(Some(src)) };
        let mut a = a;
        assert_eq!(eexe_mod_op(&mut a, &src_tv, '+'), OK);
        // The alias `b` observes the in-place extension.
        if let v_list(Some(bl)) = &b.vval {
            assert_eq!(bl.borrow().lv_items.len(), 2);
        } else {
            panic!("b not a list");
        }
    }

    #[test]
    fn blob_appends_in_place() {
        let mk = |bytes: &[u8]| {
            let b = Rc::new(RefCell::new(blob_T::default()));
            b.borrow_mut().bv_ga = bytes.to_vec();
            typval_T { v_type: VAR_BLOB, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v_blob(Some(b)) }
        };
        let mut a = mk(&[0x01]);
        assert_eq!(eexe_mod_op(&mut a, &mk(&[0x02, 0x03]), '+'), OK);
        if let v_blob(Some(b)) = &a.vval {
            assert_eq!(b.borrow().bv_ga, vec![0x01, 0x02, 0x03]);
        } else {
            panic!("not a blob");
        }
    }

    #[test]
    fn rejects_bad_combinations() {
        // dict on the right → E734/FAIL.
        let d = typval_T { v_type: VAR_DICT, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v_dict(None) };
        let mut a = num(1);
        assert_eq!(eexe_mod_op(&mut a, &d, '+'), FAIL);
        // %= with a float operand → FAIL.
        let mut n = num(10);
        let f = typval_T { v_type: VAR_FLOAT, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v_float(3.0) };
        assert_eq!(eexe_mod_op(&mut n, &f, '%'), FAIL);
        // list += non-list → FAIL.
        let mut l = typval_T { v_type: VAR_LIST, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v_list(Some(tv_list_alloc(0))) };
        assert_eq!(eexe_mod_op(&mut l, &num(1), '+'), FAIL);
        let _ = list_T::default();
    }
}
