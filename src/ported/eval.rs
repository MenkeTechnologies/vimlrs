//! Port of `src/nvim/eval.c` (vendored at `csrc/eval.c`).
//!
//! Only the leaf operator helpers are ported here: `num_divide`,
//! `num_modulus`, and `typval_compare`. The `eval0`…`eval7` recursive
//! tree-walkers are NOT ported — they are the interpreter vimlrs replaces with
//! fusevm bytecode (the same way zshrs does not port `Src/exec.c::execlist`).
//! Their per-operator semantics are reconstructed in the synthesis bridge
//! (`fusevm_bridge.rs`), which cites the relevant `eval5`/`eval6`/`eval7_leader`
//! lines.
#![allow(non_snake_case)]

// The `eval/` subtree (ports of `csrc/eval/*.c` + the header).
/// Port of `eval/decode.c`.
pub mod decode;
/// Port of `eval/encode.c`.
pub mod encode;
/// Port of `eval/executor.c` (the `tv_op` compound-assignment operators).
pub mod executor;
/// Port of `eval/fs.c` (subset: the pure path-string builtins).
pub mod fs;
/// Port of `eval/funcs.c`.
pub mod funcs;
/// Generated builtin arg-count table (from `csrc/eval.lua`); see
/// `scripts/gen_builtin_argc.sh`.
pub mod funcs_argc;
/// Port of `eval/list.c` (the `count()` family; callback ops stay bridge-side).
pub mod list;
/// Port of `eval/typval.c`.
pub mod typval;
/// Port of `eval/typval_defs.h`.
pub mod typval_defs_h;
/// Port of `eval/userfunc.c` (function-name classification helpers).
pub mod userfunc;
/// Port of `eval/vars.c`.
pub mod vars;
/// Port of `eval/window.c` (window-lookup helper layer).
pub mod window;

use std::rc::Rc;

use crate::ported::eval::typval::{
    tv_blob_equal, tv_dict_equal, tv_equal, tv_get_float, tv_get_number_chk, tv_get_string,
    tv_list_equal,
};
use crate::ported::eval::typval_defs_h::{
    partial_T, typval_T, typval_vval_union::*, varnumber_T, VarLockStatus, VarType::*,
    VARNUMBER_MAX, VARNUMBER_MIN,
};
use crate::ported::eval_h::{exprtype_T, exprtype_T::*, FAIL, OK};
use crate::ported::message::emsg;

/// Port of `num_divide()` from `Src/eval.c:171`.
///
/// "n1" divided by "n2", taking care of dividing by zero and the
/// `VARNUMBER_MIN / -1` overflow.
pub fn num_divide(n1: varnumber_T, n2: varnumber_T) -> varnumber_T {
    let result: varnumber_T;
    if n2 == 0 {
        // c: give an error message?
        if n1 == 0 {
            result = VARNUMBER_MIN; // c: similar to NaN
        } else if n1 < 0 {
            result = -VARNUMBER_MAX;
        } else {
            result = VARNUMBER_MAX;
        }
    } else if n1 == VARNUMBER_MIN && n2 == -1 {
        // c: VARNUMBER_MIN / -1 overflows; clamp to MAX
        result = VARNUMBER_MAX;
    } else {
        result = n1 / n2;
    }
    result
}

/// Port of `string2float()` from `Src/eval.c:4575`.
///
/// Convert the leading numeric prefix of `text` to a float, the way C `strtod`
/// does (with explicit `inf`/`-inf`/`nan` handling). Returns the parsed value
/// and the number of bytes consumed (0 if no number leads `text`).
pub fn string2float(text: &str) -> (f64, usize) {
    // c: MS-Windows does not deal with "inf"/"nan" properly — handle explicitly.
    let starts = |kw: &str| text.len() >= kw.len() && text[..kw.len()].eq_ignore_ascii_case(kw);
    if starts("-inf") {
        return (f64::NEG_INFINITY, 4);
    }
    if starts("inf") {
        return (f64::INFINITY, 3);
    }
    if starts("nan") {
        return (f64::NAN, 3);
    }
    // c: strtod() also parses hex floats — "0x1f" → 31.0, "0x1.8p1" → 3.0.
    {
        let b = text.as_bytes();
        let mut k = 0;
        let neg = b.first() == Some(&b'-');
        if matches!(b.first(), Some(b'+' | b'-')) {
            k = 1;
        }
        if b.len() >= k + 2 && b[k] == b'0' && (b[k + 1] | 0x20) == b'x' {
            let hexval = |c: u8| (c as char).to_digit(16).unwrap() as f64;
            let mut j = k + 2;
            let mut mant = 0.0f64;
            let mut any = false;
            while j < b.len() && b[j].is_ascii_hexdigit() {
                mant = mant * 16.0 + hexval(b[j]);
                j += 1;
                any = true;
            }
            if j < b.len() && b[j] == b'.' {
                j += 1;
                let mut scale = 1.0 / 16.0;
                while j < b.len() && b[j].is_ascii_hexdigit() {
                    mant += hexval(b[j]) * scale;
                    scale /= 16.0;
                    j += 1;
                    any = true;
                }
            }
            if any {
                // Optional binary exponent `p[+/-]ddd`.
                let mut exp = 0i32;
                if j < b.len() && (b[j] | 0x20) == b'p' {
                    let mut e = j + 1;
                    let mut es = 1i32;
                    if e < b.len() && matches!(b[e], b'+' | b'-') {
                        if b[e] == b'-' {
                            es = -1;
                        }
                        e += 1;
                    }
                    let mut ev = 0i32;
                    let mut ed = false;
                    while e < b.len() && b[e].is_ascii_digit() {
                        ev = ev * 10 + (b[e] - b'0') as i32;
                        e += 1;
                        ed = true;
                    }
                    if ed {
                        exp = es * ev;
                        j = e;
                    }
                }
                let mut val = mant * 2f64.powi(exp);
                if neg {
                    val = -val;
                }
                return (val, j);
            }
        }
    }
    // c: *ret_value = strtod(text, &s); return s - text;
    // Scan the longest prefix that is a valid C float literal.
    let b = text.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let mut saw_digit = false;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
        saw_digit = true;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            saw_digit = true;
        }
    }
    if !saw_digit {
        return (0.0, 0);
    }
    // Optional exponent, but only when it is well-formed (else strtod stops).
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    (text[..i].parse::<f64>().unwrap_or(0.0), i)
}

/// Port of `num_modulus()` from `Src/eval.c:196`.
///
/// "n1" modulus "n2", taking care of dividing by zero.
pub fn num_modulus(n1: varnumber_T, n2: varnumber_T) -> varnumber_T {
    // c: return (n2 == 0) ? 0 : (n1 % n2);
    if n2 == 0 {
        0
    } else {
        n1 % n2
    }
}

/// Port of `typval_compare()` from `Src/eval.c:6793`.
///
/// Compare `typ1` and `typ2` per `type`. On success, `typ1` is overwritten with
/// the `VAR_NUMBER` boolean result and `OK` is returned; incompatible operations
/// raise `emsg` and return `FAIL`.
pub fn typval_compare(typ1: &mut typval_T, typ2: &typval_T, r#type: exprtype_T, ic: bool) -> i32 {
    let mut n1: varnumber_T; // c: varnumber_T n1, n2;
    let n2: varnumber_T;
    let type_is = r#type == EXPR_IS || r#type == EXPR_ISNOT; // c:6797

    if type_is && typ1.v_type != typ2.v_type {
        // c: For "is" a different type always means false; "isnot" means true.
        n1 = (r#type == EXPR_ISNOT) as varnumber_T;
    } else if typ1.v_type == VAR_BLOB || typ2.v_type == VAR_BLOB {
        if type_is {
            // c: n1 = (typ1->v_type == typ2->v_type
            //          && typ1->vval.v_blob == typ2->vval.v_blob);
            n1 = (typ1.v_type == typ2.v_type
                && match (&typ1.vval, &typ2.vval) {
                    (v_blob(Some(x)), v_blob(Some(y))) => Rc::ptr_eq(x, y),
                    (v_blob(None), v_blob(None)) => true,
                    _ => false,
                }) as varnumber_T;
            if r#type == EXPR_ISNOT {
                n1 = (n1 == 0) as varnumber_T;
            }
        } else if typ1.v_type != typ2.v_type || (r#type != EXPR_EQUAL && r#type != EXPR_NEQUAL) {
            if typ1.v_type != typ2.v_type {
                emsg("E977: Can only compare Blob with Blob");
            } else {
                emsg("E978: Invalid operation for Blob");
            }
            return FAIL;
        } else {
            // c: n1 = tv_blob_equal(typ1->vval.v_blob, typ2->vval.v_blob);
            n1 = match (&typ1.vval, &typ2.vval) {
                (v_blob(Some(x)), v_blob(Some(y))) => tv_blob_equal(x, y),
                (v_blob(None), v_blob(None)) => true,
                _ => false,
            } as varnumber_T;
            if r#type == EXPR_NEQUAL {
                n1 = (n1 == 0) as varnumber_T;
            }
        }
    } else if typ1.v_type == VAR_LIST || typ2.v_type == VAR_LIST {
        if type_is {
            // c: typ1->vval.v_list == typ2->vval.v_list
            n1 = (typ1.v_type == typ2.v_type
                && match (&typ1.vval, &typ2.vval) {
                    (v_list(Some(x)), v_list(Some(y))) => Rc::ptr_eq(x, y),
                    (v_list(None), v_list(None)) => true,
                    _ => false,
                }) as varnumber_T;
            if r#type == EXPR_ISNOT {
                n1 = (n1 == 0) as varnumber_T;
            }
        } else if typ1.v_type != typ2.v_type || (r#type != EXPR_EQUAL && r#type != EXPR_NEQUAL) {
            if typ1.v_type != typ2.v_type {
                emsg("E691: Can only compare List with List");
            } else {
                emsg("E692: Invalid operation for List");
            }
            return FAIL;
        } else {
            // c: n1 = tv_list_equal(typ1->vval.v_list, typ2->vval.v_list, ic);
            n1 = match (&typ1.vval, &typ2.vval) {
                (v_list(Some(x)), v_list(Some(y))) => tv_list_equal(x, y, ic),
                (v_list(None), v_list(None)) => true,
                _ => false,
            } as varnumber_T;
            if r#type == EXPR_NEQUAL {
                n1 = (n1 == 0) as varnumber_T;
            }
        }
    } else if typ1.v_type == VAR_DICT || typ2.v_type == VAR_DICT {
        if type_is {
            // c: typ1->vval.v_dict == typ2->vval.v_dict
            n1 = (typ1.v_type == typ2.v_type
                && match (&typ1.vval, &typ2.vval) {
                    (v_dict(Some(x)), v_dict(Some(y))) => Rc::ptr_eq(x, y),
                    (v_dict(None), v_dict(None)) => true,
                    _ => false,
                }) as varnumber_T;
            if r#type == EXPR_ISNOT {
                n1 = (n1 == 0) as varnumber_T;
            }
        } else if typ1.v_type != typ2.v_type || (r#type != EXPR_EQUAL && r#type != EXPR_NEQUAL) {
            if typ1.v_type != typ2.v_type {
                emsg("E735: Can only compare Dictionary with Dictionary");
            } else {
                emsg("E736: Invalid operation for Dictionary");
            }
            return FAIL;
        } else {
            // c: n1 = tv_dict_equal(typ1->vval.v_dict, typ2->vval.v_dict, ic);
            n1 = match (&typ1.vval, &typ2.vval) {
                (v_dict(Some(x)), v_dict(Some(y))) => tv_dict_equal(x, y, ic),
                (v_dict(None), v_dict(None)) => true,
                _ => false,
            } as varnumber_T;
            if r#type == EXPR_NEQUAL {
                n1 = (n1 == 0) as varnumber_T;
            }
        }
    } else if matches!(typ1.v_type, VAR_FUNC | VAR_PARTIAL)
        || matches!(typ2.v_type, VAR_FUNC | VAR_PARTIAL)
    {
        if r#type != EXPR_EQUAL
            && r#type != EXPR_NEQUAL
            && r#type != EXPR_IS
            && r#type != EXPR_ISNOT
        {
            emsg("E694: Invalid operation for Funcrefs");
            return FAIL;
        }
        n1 = tv_equal(typ1, typ2, ic) as varnumber_T;
        if r#type == EXPR_NEQUAL || r#type == EXPR_ISNOT {
            n1 = (n1 == 0) as varnumber_T;
        }
    } else if (typ1.v_type == VAR_FLOAT || typ2.v_type == VAR_FLOAT)
        && r#type != EXPR_MATCH
        && r#type != EXPR_NOMATCH
    {
        // c: If one of the two variables is a float, compare as a float.
        let f1 = tv_get_float(typ1);
        let f2 = tv_get_float(typ2);
        n1 = match r#type {
            EXPR_IS | EXPR_EQUAL => (f1 == f2) as varnumber_T,
            EXPR_ISNOT | EXPR_NEQUAL => (f1 != f2) as varnumber_T,
            EXPR_GREATER => (f1 > f2) as varnumber_T,
            EXPR_GEQUAL => (f1 >= f2) as varnumber_T,
            EXPR_SMALLER => (f1 < f2) as varnumber_T,
            EXPR_SEQUAL => (f1 <= f2) as varnumber_T,
            _ => 0,
        };
    } else if (typ1.v_type == VAR_NUMBER || typ2.v_type == VAR_NUMBER)
        && r#type != EXPR_MATCH
        && r#type != EXPR_NOMATCH
    {
        // c: If one of the two variables is a number, compare as a number.
        n1 = tv_get_number_chk(typ1, None);
        n2 = tv_get_number_chk(typ2, None);
        n1 = match r#type {
            EXPR_IS | EXPR_EQUAL => (n1 == n2) as varnumber_T,
            EXPR_ISNOT | EXPR_NEQUAL => (n1 != n2) as varnumber_T,
            EXPR_GREATER => (n1 > n2) as varnumber_T,
            EXPR_GEQUAL => (n1 >= n2) as varnumber_T,
            EXPR_SMALLER => (n1 < n2) as varnumber_T,
            EXPR_SEQUAL => (n1 <= n2) as varnumber_T,
            _ => 0,
        };
    } else {
        let s1 = tv_get_string(typ1);
        let s2 = tv_get_string(typ2);
        // c: i = (type != MATCH && type != NOMATCH) ? mb_strcmp_ic(ic, s1, s2) : 0;
        let i: i32 = if r#type != EXPR_MATCH && r#type != EXPR_NOMATCH {
            mb_strcmp_ic(ic, &s1, &s2)
        } else {
            0
        };
        n1 = match r#type {
            EXPR_IS | EXPR_EQUAL => (i == 0) as varnumber_T,
            EXPR_ISNOT | EXPR_NEQUAL => (i != 0) as varnumber_T,
            EXPR_GREATER => (i > 0) as varnumber_T,
            EXPR_GEQUAL => (i >= 0) as varnumber_T,
            EXPR_SMALLER => (i < 0) as varnumber_T,
            EXPR_SEQUAL => (i <= 0) as varnumber_T,
            EXPR_MATCH | EXPR_NOMATCH => {
                let mut m = pattern_match(&s2, &s1, ic) as varnumber_T;
                if r#type == EXPR_NOMATCH {
                    m = (m == 0) as varnumber_T;
                }
                m
            }
            EXPR_UNKNOWN => 0,
        };
    }

    // c: tv_clear(typ1); typ1->v_type = VAR_NUMBER; typ1->vval.v_number = n1;
    *typ1 = typval_T {
        v_type: VAR_NUMBER,
        v_lock: VarLockStatus::VAR_UNLOCKED,
        vval: v_number(n1),
    };
    OK
}

/// Port of `mb_strcmp_ic()` (`Src/nvim/strings.c`, extern) reduced to byte/UTF-8
/// comparison with optional case-fold. Returns <0/0/>0 like `strcmp`.
fn mb_strcmp_ic(ic: bool, s1: &str, s2: &str) -> i32 {
    let (a, b) = if ic {
        (s1.to_lowercase(), s2.to_lowercase())
    } else {
        (s1.to_string(), s2.to_string())
    };
    match a.cmp(&b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// Port of `pattern_match()` (`Src/nvim/regexp.c`) — `vim_regexec` over the
/// subject. Delegates to the VimL regex engine ([`crate::viml_regex`]), the way
/// other ports delegate to fusevm; that engine implements Vim's pattern dialect
/// (it replaces the C engine's bytecode-program matcher).
fn pattern_match(pat: &str, subject: &str, ic: bool) -> bool {
    // `'ignorecase'` makes a plain `=~` match case-insensitively.
    let ic = ic
        || crate::ported::eval::typval::tv_get_bool(&crate::ported::option::get_option_value(
            "ignorecase",
        )) != 0;
    crate::viml_regex::regex_match(pat, subject, ic)
}

/// `AUTOLOAD_CHAR` from `Src/eval.h` (c:136) — the `#` separator in autoload
/// names (`foo#bar#baz`).
pub const AUTOLOAD_CHAR: u8 = b'#';

/// Port of `eval_isnamec1()` from `Src/eval.c` (c:5761) — true if `c` may begin
/// a variable or function name (a letter or `_`).
pub fn eval_isnamec1(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

/// Port of `eval_isnamec()` from `Src/eval.c` (c:5754) — true if `c` may appear
/// within a variable or function name (`[A-Za-z0-9_:#]`).
pub fn eval_isnamec(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b':' || c == AUTOLOAD_CHAR
}

/// Port of `eval_isdictc()` from `Src/eval.c` (c:5768) — true if `c` may appear
/// in an unquoted dictionary key (`[A-Za-z0-9_]`).
pub fn eval_isdictc(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Port of `partial_free()` from `Src/eval.c:3824` — free a closure and its
/// bound argv/self-dict. RUST-PORT NOTE: the `partial_T` and everything it owns
/// are `Rc`/`Drop`-managed, so reclamation is automatic → no-op.
pub fn partial_free(_pt: &partial_T) {}

/// Port of `partial_unref()` from `Src/eval.c:3842` — decrement a closure's
/// reference count, freeing at zero. The `Rc` does this on clone/drop, so the
/// explicit unref is a no-op (mirrors [`func_unref`]).
pub fn partial_unref(_pt: Option<&Rc<partial_T>>) {}

/// Port of `typval_tostring()` from `Src/eval.c:7001`.
///
/// A human-readable string for `arg`: `"(does not exist)"` for `None`; the raw
/// contents of a String when `quotes` is false; otherwise the `string()`
/// encoding (quoted strings, `[...]`/`{...}` for containers).
pub fn typval_tostring(arg: Option<&typval_T>, quotes: bool) -> String {
    match arg {
        None => "(does not exist)".to_string(),
        Some(tv) => {
            if !quotes && tv.v_type == VAR_STRING {
                match &tv.vval {
                    v_string(s) => s.clone(),
                    _ => String::new(),
                }
            } else {
                crate::ported::eval::encode::encode_tv2string(tv)
            }
        }
    }
}

/// Port of `partial_name()` from `Src/eval.c:3810` — the function name a partial
/// resolves to. RUST-PORT NOTE: `pt_func` (the resolved `ufunc_T`) is not
/// modeled, so only the `pt_name` branch applies; an empty name yields `""`.
pub fn partial_name(pt: &partial_T) -> &str {
    &pt.pt_name
}

/// Port of `func_equal()` from `Src/eval.c:3911` — compare two Funcref/Partial
/// values: equal when their names, `self` dicts, and bound argument lists all
/// match (NULL/empty name and NULL/empty dict treated alike).
pub fn func_equal(tv1: &typval_T, tv2: &typval_T, ic: bool) -> bool {
    // c: empty and NULL function name considered the same.
    let name = |tv: &typval_T| -> String {
        match (tv.v_type, &tv.vval) {
            (VAR_FUNC, v_string(s)) => s.clone(),
            (VAR_PARTIAL, v_partial(Some(p))) => partial_name(p).to_string(),
            _ => String::new(),
        }
    };
    let s1 = name(tv1);
    let s2 = name(tv2);
    // c: if (s1 == NULL || s2 == NULL) { if (s1 != s2) return false; } else strcmp.
    if s1 != s2 {
        return false;
    }

    // c: empty dict and NULL dict is different — both NULL-equivalent here means equal.
    let dict = |tv: &typval_T| -> Option<Rc<std::cell::RefCell<crate::ported::eval::typval_defs_h::dict_T>>> {
        match (tv.v_type, &tv.vval) {
            (VAR_PARTIAL, v_partial(Some(p))) => p.pt_dict.clone(),
            _ => None,
        }
    };
    match (dict(tv1), dict(tv2)) {
        (None, None) => {}
        (Some(d1), Some(d2)) => {
            if !tv_dict_equal(&d1, &d2, ic) {
                return false;
            }
        }
        _ => return false,
    }

    // c: empty list and no list considered the same — compare bound args pairwise.
    let argv = |tv: &typval_T| -> Vec<typval_T> {
        match (tv.v_type, &tv.vval) {
            (VAR_PARTIAL, v_partial(Some(p))) => p.pt_argv.clone(),
            _ => Vec::new(),
        }
    };
    let a1 = argv(tv1);
    let a2 = argv(tv2);
    if a1.len() != a2.len() {
        return false;
    }
    a1.iter().zip(a2.iter()).all(|(x, y)| tv_equal(x, y, ic))
}

// ── eval.c misc helpers (init/clear are no-ops; renderers + arg validation) ──

/// Port of `eval_init()` from `Src/eval.c` — global eval state is initialised
/// lazily by the value/var layers, so there is nothing to do here.
pub fn eval_init() {}

/// Port of `eval_clear()` from `Src/eval.c` — teardown of eval globals; the
/// `Rc`-managed value layer needs no explicit clear.
pub fn eval_clear() {}

/// Port of `eval_expr_valid_arg()` from `Src/eval.c` — true when `tv` is usable
/// as an expression argument (to `map()`/`sort()`/…): not Unknown, and not an
/// empty/NULL String.
pub fn eval_expr_valid_arg(tv: &typval_T) -> bool {
    tv.v_type != VAR_UNKNOWN
        && (tv.v_type != VAR_STRING || matches!(&tv.vval, v_string(s) if !s.is_empty()))
}

/// Port of `typval2string()` from `Src/eval.c` — render `tv` to a String: a List
/// is newline-joined (with a trailing newline) when `join_list`, a List/Dict
/// otherwise uses `string()`, and a scalar uses its string value.
pub fn typval2string(tv: &typval_T, join_list: bool) -> String {
    use crate::ported::eval::encode::encode_tv2string;
    use crate::ported::eval::typval::tv_list_join;
    if join_list && tv.v_type == VAR_LIST {
        let mut out = String::new();
        if let v_list(Some(l)) = &tv.vval {
            let lb = l.borrow();
            tv_list_join(&mut out, &lb, "\n");
            if lb.lv_len > 0 {
                out.push('\n');
            }
        }
        out
    } else if tv.v_type == VAR_LIST || tv.v_type == VAR_DICT {
        encode_tv2string(tv)
    } else {
        tv_get_string(tv)
    }
}

/// Port of `restore_v_event()` from `Src/eval.c` — restore the saved `v:event`
/// dict; `v:event` is not populated standalone, so this is a no-op.
pub fn restore_v_event() {}

/// Port of `get_v_event()` from `Src/eval.c:145`.
///
/// Return the `v:event` dictionary (into which event data is written before
/// firing an autocommand). RUST-PORT NOTE: standalone never populates `v:event`
/// (no event data), so the C save-and-clear of an in-use dict never triggers —
/// consistent with the no-op [`restore_v_event`] — and this just returns the
/// (empty) dict.
pub fn get_v_event() -> Option<Rc<std::cell::RefCell<crate::ported::eval::typval_defs_h::dict_T>>> {
    crate::ported::eval::vars::get_vim_var_dict(crate::ported::eval::vars::vv::VV_EVENT)
}

/// Port of `get_copyID()` from `Src/eval.c` — the GC mark-and-sweep copy-id
/// counter. The `Rc`-managed value layer needs no copy-id pass → 0.
pub fn get_copyID() -> i32 {
    0
}

/// Port of `get_callback_depth()` from `Src/eval.c` — current callback nesting
/// depth; not separately tracked → 0.
pub fn get_callback_depth() -> i32 {
    0
}

// ── eval.c name scanners + leaf no-ops ──

/// True for an identifier byte (`vim_isIDc`): ASCII alphanumeric or `_`.
fn vim_isIDc(c: u8) -> bool {
    c == b'_' || c.is_ascii_alphanumeric()
}

/// Port of `get_env_len()` from `Src/eval.c` — the length of the environment
/// variable name at the start of `s` (run of identifier chars), or 0.
pub fn get_env_len(s: &str) -> i32 {
    s.bytes().take_while(|&c| vim_isIDc(c)).count() as i32
}

/// Port of `get_id_len()` from `Src/eval.c` — the length of the variable name at
/// the start of `s`, honouring that a single namespace char before `:` (e.g.
/// `s:`) is part of the name but other `xx:` is not.
pub fn get_id_len(s: &str) -> i32 {
    let b = s.as_bytes();
    let mut p = 0;
    while p < b.len() && eval_isnamec(b[p]) {
        if b[p] == b':' {
            let len = p;
            let is_ns = len == 1 && b"abglstvw".contains(&b[0]);
            if len > 1 || !is_ns {
                break;
            }
        }
        p += 1;
    }
    p as i32
}

/// Port of `skip_luafunc_name()` from `Src/eval.c` — index past a `v:lua`
/// function name (`A-Za-z0-9_.'`).
pub fn skip_luafunc_name(s: &str) -> usize {
    s.bytes()
        .take_while(|&c| c.is_ascii_alphanumeric() || c == b'_' || c == b'.' || c == b'\'')
        .count()
}

/// Port of `check_luafunc_name()` from `Src/eval.c` — the length of a valid
/// `v:lua` function name in `str` when terminated by `(` (if `paren`) or end of
/// string, else 0.
pub fn check_luafunc_name(str: &str, paren: bool) -> i32 {
    let end = skip_luafunc_name(str);
    let term_ok = if paren {
        str.as_bytes().get(end) == Some(&b'(')
    } else {
        end == str.len()
    };
    if term_ok {
        end as i32
    } else {
        0
    }
}

/// Port of `get_echo_hl_id()` from `Src/eval.c` — the highlight id for `:echohl`;
/// no highlight groups standalone → 0.
pub fn get_echo_hl_id() -> i32 {
    0
}

/// Port of `may_call_simple_func()` from `Src/eval.c` — the fast path for a bare
/// function call; unused here (the bridge calls), so → FAIL (use the normal path).
pub fn may_call_simple_func() -> i32 {
    FAIL
}

/// Port of `free_for_info()` from `Src/eval.c` — free a `:for` loop iterator;
/// `Rc`-managed, no-op.
pub fn free_for_info() {}

/// Port of `timer_stop_all()` from `Src/eval.c` — stop all timers; none exist
/// standalone (no event loop), no-op.
pub fn timer_stop_all() {}

/// Port of `timer_teardown()` from `Src/eval.c` — timer-subsystem teardown; no-op.
pub fn timer_teardown() {}

/// Port of `set_argv_var()` from `Src/eval.c` — set `v:argv` from the process
/// arguments. The standalone interpreter exposes its CLI args here.
pub fn set_argv_var(argv: &[String]) {
    use crate::ported::eval::typval::{tv_list_alloc, tv_list_append_string};
    let l = tv_list_alloc(argv.len() as isize);
    {
        let mut lb = l.borrow_mut();
        for a in argv {
            tv_list_append_string(&mut lb, a);
        }
    }
    crate::ported::eval::vars::set_vim_var_list(crate::ported::eval::vars::vv::VV_ARGV, Some(l));
}

/// Port of `get_name_len()` from `Src/eval.c` — the length of the variable/
/// function name at the start of `s` (subset: no curly-brace expansion).
pub fn get_name_len(s: &str) -> i32 {
    get_id_len(s)
}

/// Port of `eval_clear`-adjacent `garbage_collect()` from `Src/eval.c` — the
/// mark-and-sweep pass; the `Rc` value layer frees eagerly, so → false (nothing
/// freed).
pub fn garbage_collect(_testing: bool) -> bool {
    false
}

/// Port of `free_unref_items()` from `Src/eval.c` — free items unreferenced
/// after a GC mark pass; the `Rc` layer frees eagerly → nothing freed (0).
pub fn free_unref_items(_copy_id: i32) -> i32 {
    0
}

/// Port of `set_ref_in_list_items()`/GC marker (`Src/eval.c`) — mark a list
/// reachable during GC; no mark pass under `Rc`, no-op returning false.
pub fn set_ref_in_list_items() -> bool {
    false
}

/// Port of `set_ref_in_item()` (`Src/eval.c`) — GC marker; no-op (false).
pub fn set_ref_in_item() -> bool {
    false
}

/// Port of `set_ref_in_ht()` (`Src/eval.c`) — GC marker over a hashtab; no-op.
pub fn set_ref_in_ht() -> bool {
    false
}

/// Port of `set_ref_in_item_dict()` (`Src/eval.c:4304`) — GC marker that walks a
/// Dict's items; no mark pass under `Rc`, no-op returning false (no abort).
pub fn set_ref_in_item_dict() -> bool {
    false
}

/// Port of `set_ref_in_item_list()` (`Src/eval.c:4334`) — GC marker over a
/// List's items; no-op (false).
pub fn set_ref_in_item_list() -> bool {
    false
}

/// Port of `set_ref_in_item_partial()` (`Src/eval.c:4357`) — GC marker over a
/// partial's bound argv/self-dict; no-op (false).
pub fn set_ref_in_item_partial() -> bool {
    false
}

/// Port of `set_ref_in_callback()` (`Src/eval.c:4943`) — GC marker over a
/// `Callback`'s referenced function/partial; no-op (false).
pub fn set_ref_in_callback() -> bool {
    false
}

/// Port of `set_ref_in_callback_reader()` (`Src/eval.c:4964`) — GC marker over a
/// `CallbackReader`'s buffers and callback; no-op (false).
pub fn set_ref_in_callback_reader() -> bool {
    false
}

/// Port of `eval_has_provider()` from `Src/eval.c` — whether a `has('python3')`-
/// style provider feature is available; no providers standalone → false.
pub fn eval_has_provider(_feat: &str, _throw_if_fast: bool) -> bool {
    false
}

/// Port of `invoke_prompt_interrupt()` from `Src/eval.c` — fire a prompt
/// buffer's interrupt callback; no prompt buffer standalone → false.
pub fn invoke_prompt_interrupt() -> bool {
    false
}

/// Port of `eval_call_provider()` from `Src/eval.c:6537`.
///
/// Call a language-host provider (`perl`/`python`/…) method. RUST-PORT NOTE: no
/// providers exist standalone (see [`eval_has_provider`]), so this always takes
/// the C "no provider" path — emit `E319` and return Number `0`.
pub fn eval_call_provider(
    provider: &str,
    _method: &str,
    _arguments: Option<Rc<std::cell::RefCell<crate::ported::eval::typval_defs_h::list_T>>>,
    _discard: bool,
) -> typval_T {
    crate::ported::message::semsg(&format!(
        "E319: No \"{provider}\" provider found. Run \":checkhealth vim.provider\""
    ));
    typval_T::from(0)
}

/// Port of `prompt_get_input()` from `Src/eval.c:6669` — the current user input
/// in a prompt buffer. No prompt buffer standalone → `None`.
pub fn prompt_get_input() -> Option<String> {
    None
}

/// Port of `prompt_trim_scrollback()` from `Src/eval.c:6693` — trim a prompt
/// buffer's scrollback; no prompt buffer standalone, no-op.
pub fn prompt_trim_scrollback() {}

/// Port of `prompt_invoke_callback()` from `Src/eval.c:6727` — run a prompt
/// buffer's callback on an entered line. No prompt buffer standalone, no-op
/// (mirrors [`invoke_prompt_interrupt`]).
pub fn prompt_invoke_callback() {}

/// Port of `script_host_eval()` from `Src/eval.c:6519` — evaluate an expression
/// via a language-host provider (`perl`/`python`/…). No providers standalone
/// (see [`eval_has_provider`]), no-op.
pub fn script_host_eval() {}

/// Port of `next_for_item()` from `Src/eval.c` — advance a `:for` iterator; the
/// bridge drives `:for`, so this path is unused → false (stop).
pub fn next_for_item() -> bool {
    false
}

/// `static char * const namespace_char = "abglstvw";` (`Src/eval.c:115`) — the
/// single-letter scope prefixes (`a:`/`b:`/`g:`/`l:`/`s:`/`t:`/`v:`/`w:`) that
/// make a trailing `:` part of a name rather than a slice separator.
pub const NAMESPACE_CHAR: &[u8] = b"abglstvw";

/// Port of `char_idx2byte()` from `Src/eval.c:5863`.
///
/// Byte offset of character index `idx` in `str`. A negative `idx` counts from
/// the end (`-1` is the last character). Returns `Some(str.len())` when `idx`
/// runs past the end, and `None` when a negative `idx` runs past the start
/// (the C `-1` "before the start" sentinel).
///
/// RUST-PORT NOTE: the C walks `utfc_ptr2len`, which groups composing marks
/// into one index; consistent with the rest of the crate, a Rust `char`
/// (one Unicode scalar) is the unit instead.
pub fn char_idx2byte(str: &str, idx: varnumber_T) -> Option<usize> {
    if idx >= 0 {
        // Forward: skip `idx` characters, clamping at the end.
        match str.char_indices().nth(idx as usize) {
            Some((b, _)) => Some(b),
            None => Some(str.len()),
        }
    } else {
        // Backward: -1 is the last char. `nchar` chars from the end.
        let nchar = (-idx) as usize;
        let total = str.chars().count();
        if nchar > total {
            None
        } else {
            Some(
                str.char_indices()
                    .nth(total - nchar)
                    .map_or(str.len(), |(b, _)| b),
            )
        }
    }
}

/// Port of `char_from_string()` from `Src/eval.c:5825`.
///
/// The single character at character index `index` in `str` (a negative index
/// counts from the end, like a List). Returns `None` (the C `NULL`, i.e. the
/// empty string at the call site) when the index is out of range.
pub fn char_from_string(str: &str, index: varnumber_T) -> Option<String> {
    let chars: Vec<char> = str.chars().collect();
    let nchar = if index < 0 {
        let n = chars.len() as varnumber_T + index;
        if n < 0 {
            // c: unlike a List, an out-of-range index is the empty string.
            return None;
        }
        n
    } else {
        index
    };
    chars.get(nchar as usize).map(|c| c.to_string())
}

/// Port of `string_slice()` from `Src/eval.c:5893`.
///
/// The slice `str[first : last]` by character indices (composing characters
/// included). `exclusive` is true for `slice()`, false for the `[a:b]`
/// subscript (where `last` is inclusive). Returns `None` when the result is
/// empty (the C `NULL`).
pub fn string_slice(
    str: &str,
    first: varnumber_T,
    last: varnumber_T,
    exclusive: bool,
) -> Option<String> {
    let slen = str.len();
    // c: first index very negative → clamp to zero.
    let start_byte = char_idx2byte(str, first).unwrap_or(0);
    let end_byte = if (last == -1 && !exclusive) || last == VARNUMBER_MAX {
        slen
    } else {
        match char_idx2byte(str, last) {
            // c: inclusive subscript end → step past that character.
            Some(b) if !exclusive && b < slen => b + str[b..].chars().next().map_or(0, char::len_utf8),
            Some(b) => b,
            None => return None,
        }
    };
    if start_byte >= slen || end_byte <= start_byte {
        return None;
    }
    Some(str[start_byte..end_byte].to_string())
}

/// Port of `to_name_end()` from `Src/eval.c:805`.
///
/// Byte offset of the end of the name starting at `arg` (`""` start → `0`).
/// With `use_namespace`, a `b:`/`g:`/`s:`/`t:`/`v:`/`w:` prefix keeps its `:`
/// as part of the name; any other `:` terminates it (so `[n:]` slices work).
pub fn to_name_end(arg: &str, use_namespace: bool) -> usize {
    let bytes = arg.as_bytes();
    // c: quick check for a valid starting character.
    if bytes.is_empty() || !eval_isnamec1(bytes[0]) {
        return 0;
    }
    let mut p = 1;
    while p < bytes.len() && eval_isnamec(bytes[p]) {
        if bytes[p] == b':'
            && (p != 1 || !use_namespace || !b"bgstvw".contains(&bytes[0]))
        {
            break;
        }
        p += 1;
    }
    p
}

/// `FNE_INCL_BR` (`Src/eval.h`) — `find_name_end` should fold a trailing
/// `[idx]`/`.key` subscript into the name span.
pub const FNE_INCL_BR: u32 = 1;
/// `FNE_CHECK_START` (`Src/eval.h`) — require a valid name-starting character.
pub const FNE_CHECK_START: u32 = 2;

/// Port of `find_name_end()` from `Src/eval.c:5620`.
///
/// Byte offset of the end of the variable/function name at `arg`, honoring
/// `{expr}` curly nesting, `[idx]` bracket nesting, quoted strings, and the
/// namespace `:` rule. Returns `(end, expr_start, expr_end)` where the latter
/// two are the byte offsets of the first `{`…`}` curly group (each `None` if
/// no curly is present), mirroring the C out-parameters.
///
/// `flags`: [`FNE_INCL_BR`] also folds `[`/`.key` subscripts into the name span;
/// [`FNE_CHECK_START`] requires a valid starting character.
pub fn find_name_end(arg: &str, flags: u32) -> (usize, Option<usize>, Option<usize>) {
    let b = arg.as_bytes();
    let mut expr_start: Option<usize> = None;
    let mut expr_end: Option<usize> = None;

    // c: quick check for a valid starting character.
    if (flags & FNE_CHECK_START) != 0
        && !(b.first().is_some_and(|&c| eval_isnamec1(c)) || b.first() == Some(&b'{'))
    {
        return (0, None, None);
    }

    let mut mb_nest = 0i32;
    let mut br_nest = 0i32;
    let mut p = 0usize;
    while p < b.len() {
        let c = b[p];
        let cont = eval_isnamec(c)
            || c == b'{'
            || ((flags & FNE_INCL_BR) != 0
                && (c == b'[' || (c == b'.' && b.get(p + 1).is_some_and(|&d| eval_isdictc(d)))))
            || mb_nest != 0
            || br_nest != 0;
        if !cont {
            break;
        }
        if c == b'\'' {
            // skip over 'string' to avoid counting [ and ] inside it.
            p += 1;
            while p < b.len() && b[p] != b'\'' {
                p += 1;
            }
            if p >= b.len() {
                break;
            }
        } else if c == b'"' {
            // skip over "str\"ing" to avoid counting [ and ] inside it.
            p += 1;
            while p < b.len() && b[p] != b'"' {
                if b[p] == b'\\' && p + 1 < b.len() {
                    p += 1;
                }
                p += 1;
            }
            if p >= b.len() {
                break;
            }
        } else if br_nest == 0 && mb_nest == 0 && c == b':' {
            // "s:" starts "s:var", but "n:" does not (used in slice "[n:]").
            let len = p;
            if (len > 1 && b[p - 1] != b'}')
                || (len == 1 && !NAMESPACE_CHAR.contains(&b[0]))
            {
                break;
            }
        }

        if mb_nest == 0 {
            if c == b'[' {
                br_nest += 1;
            } else if c == b']' {
                br_nest -= 1;
            }
        }
        if br_nest == 0 {
            if c == b'{' {
                mb_nest += 1;
                if expr_start.is_none() {
                    expr_start = Some(p);
                }
            } else if c == b'}' {
                mb_nest -= 1;
                if mb_nest == 0 && expr_end.is_none() {
                    expr_end = Some(p);
                }
            }
        }
        p += 1;
    }
    (p, expr_start, expr_end)
}

/// Port of `get_literal_key()` from `Src/eval.c:4422`.
///
/// Parse a literal `#{}` dictionary key (`[A-Za-z0-9_-]+`) at the start of
/// `arg`. Returns `Some((key, rest))` where `rest` is `arg` past the key with
/// leading whitespace skipped, or `None` (the C `FAIL`) if `arg` does not start
/// with a key character.
pub fn get_literal_key(arg: &str) -> Option<(String, &str)> {
    let b = arg.as_bytes();
    let is_key = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'-';
    if b.is_empty() || !is_key(b[0]) {
        return None;
    }
    let end = b.iter().position(|&c| !is_key(c)).unwrap_or(b.len());
    let key = arg[..end].to_string();
    let rest = arg[end..].trim_start_matches([' ', '\t']);
    Some((key, rest))
}

/// Port of `check_can_index()` from `Src/eval.c:3181`.
///
/// Whether `rettv` may be subscripted/sliced: Funcref/Partial, Float,
/// Bool/Special (and an `evaluate`d Unknown) cannot and yield [`FAIL`]; String,
/// Number, List, Dict, Blob (and a non-evaluated Unknown) yield [`OK`]. Emits
/// the matching error when `verbose`.
pub fn check_can_index(rettv: &typval_T, evaluate: bool, verbose: bool) -> i32 {
    match rettv.v_type {
        VAR_FUNC | VAR_PARTIAL => {
            if verbose {
                emsg("E695: Cannot index a Funcref");
            }
            FAIL
        }
        VAR_FLOAT => {
            if verbose {
                emsg("E806: Using a Float as a String");
            }
            FAIL
        }
        VAR_BOOL | VAR_SPECIAL => {
            if verbose {
                emsg("E909: Cannot index a special variable");
            }
            FAIL
        }
        VAR_UNKNOWN => {
            if evaluate {
                emsg("E909: Cannot index a special variable");
                FAIL
            } else {
                OK
            }
        }
        VAR_STRING | VAR_NUMBER | VAR_LIST | VAR_DICT | VAR_BLOB => OK,
    }
}

/// Port of `grow_string_tv()` from `Src/eval.c:2272`.
///
/// Append `s2` to the String value in `tv1` in place, returning [`OK`]; returns
/// [`FAIL`] (leaving `tv1` untouched) when `tv1` is not a String.
pub fn grow_string_tv(tv1: &mut typval_T, s2: &str) -> i32 {
    match (tv1.v_type, &mut tv1.vval) {
        (VAR_STRING, v_string(s)) => {
            s.push_str(s2);
            OK
        }
        _ => FAIL,
    }
}

/// `var_flavour_T` (`Src/eval.c`) — how a global variable name participates in
/// session/ShaDa save: `:mksession` (`SESSION`), ShaDa file (`SHADA`), or
/// neither (`DEFAULT`).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum var_flavour_T {
    /// Lowercased name — not saved.
    VAR_FLAVOUR_DEFAULT,
    /// Mixed-case starting uppercase — saved to a session file.
    VAR_FLAVOUR_SESSION,
    /// All-uppercase — saved to the ShaDa file.
    VAR_FLAVOUR_SHADA,
}

/// Port of `var_flavour()` from `Src/eval.c:6318`.
///
/// `ALLCAPS` → ShaDa, `Mixed`/`Capitalized` → session, anything else → default.
pub fn var_flavour(varname: &str) -> var_flavour_T {
    use var_flavour_T::*;
    let mut chars = varname.chars();
    match chars.next() {
        Some(first) if first.is_ascii_uppercase() => {
            if chars.any(|c| c.is_ascii_lowercase()) {
                VAR_FLAVOUR_SESSION
            } else {
                VAR_FLAVOUR_SHADA
            }
        }
        _ => VAR_FLAVOUR_DEFAULT,
    }
}

/// Port of `eval_expr_partial()` from `Src/eval.c:319`.
///
/// Evaluate `expr` (a Partial) by calling it with `argv`; the partial's bound
/// arguments are honored by the bridge hook. Returns [`OK`]/[`FAIL`].
pub fn eval_expr_partial(expr: &typval_T, argv: &[typval_T], rettv: &mut typval_T) -> i32 {
    let name = match &expr.vval {
        v_partial(Some(p)) => partial_name(p).to_string(),
        _ => return FAIL,
    };
    if name.is_empty() {
        return FAIL;
    }
    match crate::ported::eval::typval::CALL_FUNC_HOOK
        .with(|h| *h.borrow())
        .and_then(|f| f(expr, argv))
    {
        Some(result) => {
            *rettv = result;
            OK
        }
        None => FAIL,
    }
}

/// Port of `eval_expr_func()` from `Src/eval.c:345`.
///
/// Evaluate `expr` (a Funcref or a function-name String) by calling it with
/// `argv`. Returns [`OK`]/[`FAIL`].
pub fn eval_expr_func(expr: &typval_T, argv: &[typval_T], rettv: &mut typval_T) -> i32 {
    let name = match (expr.v_type, &expr.vval) {
        (VAR_FUNC, v_string(s)) => s.clone(),
        _ => tv_get_string(expr),
    };
    if name.is_empty() {
        return FAIL;
    }
    match crate::ported::eval::typval::CALL_FUNC_HOOK
        .with(|h| *h.borrow())
        .and_then(|f| f(expr, argv))
    {
        Some(result) => {
            *rettv = result;
            OK
        }
        None => FAIL,
    }
}

/// Port of `call_vim_function()` from `Src/eval.c:627`.
///
/// Call Vimscript function `func` with arguments `argv`, storing the result in
/// `rettv`; returns [`OK`]/[`FAIL`]. RUST-PORT NOTE: dispatch goes through the
/// bridge's `CALL_FUNC_HOOK`; the `v:lua.` prefix and `funcexe` line range are
/// not modeled.
pub fn call_vim_function(func: &str, argv: &[typval_T], rettv: &mut typval_T) -> i32 {
    let callee = typval_T {
        v_type: VAR_FUNC,
        v_lock: VarLockStatus::VAR_UNLOCKED,
        vval: v_string(func.to_string()),
    };
    match crate::ported::eval::typval::CALL_FUNC_HOOK
        .with(|h| *h.borrow())
        .and_then(|f| f(&callee, argv))
    {
        Some(result) => {
            *rettv = result;
            OK
        }
        None => FAIL,
    }
}

/// Port of `call_func_retstr()` from `Src/eval.c:670`.
///
/// Call `func` and return its result as a String, or `None` (the C `NULL`) when
/// the call fails.
pub fn call_func_retstr(func: &str, argv: &[typval_T]) -> Option<String> {
    let mut rettv = typval_T::from(0);
    if call_vim_function(func, argv, &mut rettv) == FAIL {
        return None;
    }
    Some(tv_get_string(&rettv))
}

/// Port of `call_func_retlist()` from `Src/eval.c:694`.
///
/// Call `func` and return its result as a List, or `None` when the call fails
/// or the result is not a List.
pub fn call_func_retlist(
    func: &str,
    argv: &[typval_T],
) -> Option<Rc<std::cell::RefCell<crate::ported::eval::typval_defs_h::list_T>>> {
    let mut rettv = typval_T::from(0);
    if call_vim_function(func, argv, &mut rettv) == FAIL {
        return None;
    }
    match (rettv.v_type, rettv.vval) {
        (VAR_LIST, v_list(Some(l))) => Some(l),
        _ => None,
    }
}

/// Port of `callback_call()` from `Src/eval.c:4888`.
///
/// Invoke `callback` with `argvars`, storing the result in `rettv`; returns
/// whether the call happened. A `None` callback does nothing. RUST-PORT NOTE:
/// the actual dispatch goes through the bridge's `CALL_FUNC_HOOK` (the value
/// layer can't call user functions itself); the C `funcexe` first/last-line
/// range and recursion guard are not modeled.
pub fn callback_call(
    callback: &crate::ported::eval::typval::Callback,
    argvars: &[typval_T],
    rettv: &mut typval_T,
) -> bool {
    use crate::ported::eval::typval::Callback;
    match callback {
        Callback::None => false,
        Callback::Funcref(name) => {
            let callee = typval_T {
                v_type: VAR_FUNC,
                v_lock: VarLockStatus::VAR_UNLOCKED,
                vval: v_string(name.clone()),
            };
            match crate::ported::eval::typval::CALL_FUNC_HOOK
                .with(|h| *h.borrow())
                .and_then(|f| f(&callee, argvars))
            {
                Some(result) => {
                    *rettv = result;
                    true
                }
                None => false,
            }
        }
    }
}

/// Port of `is_luafunc()` from `Src/eval.c:5787`.
///
/// True when `partial` is the special `v:lua` partial used to call Lua
/// functions — identified by object identity with `get_vim_var_partial(VV_LUA)`.
/// RUST-PORT NOTE: the standalone interpreter has no Lua provider, so unless a
/// value *is* the `v:lua` partial this is false.
pub fn is_luafunc(partial: &Rc<partial_T>) -> bool {
    crate::ported::eval::vars::get_vim_var_partial(crate::ported::eval::vars::vv::VV_LUA)
        .is_some_and(|lua| Rc::ptr_eq(&lua, partial))
}

/// Port of `tv_is_luafunc()` from `Src/eval.c:5794`.
///
/// True when `tv` is the `v:lua` Funcref value (a partial that [`is_luafunc`]).
pub fn tv_is_luafunc(tv: &typval_T) -> bool {
    matches!((tv.v_type, &tv.vval), (VAR_PARTIAL, v_partial(Some(p))) if is_luafunc(p))
}

#[cfg(test)]
mod tests {
    use super::string2float;
    use super::{
        char_from_string, char_idx2byte, find_name_end, get_literal_key, is_luafunc, string_slice,
        to_name_end, tv_is_luafunc, var_flavour, var_flavour_T::*,
    };
    use crate::ported::eval::typval_defs_h::{typval_T, typval_vval_union::*, VarLockStatus, VarType::*};
    use std::rc::Rc;

    #[test]
    fn char_idx2byte_ascii_and_neg() {
        assert_eq!(char_idx2byte("hello", 0), Some(0));
        assert_eq!(char_idx2byte("hello", 4), Some(4));
        assert_eq!(char_idx2byte("hello", 5), Some(5)); // past end → len
        assert_eq!(char_idx2byte("hello", -1), Some(4)); // last char
        assert_eq!(char_idx2byte("hello", -5), Some(0));
        assert_eq!(char_idx2byte("hello", -6), None); // before start
    }

    #[test]
    fn char_from_string_unicode() {
        assert_eq!(char_from_string("héllo", 1).as_deref(), Some("é"));
        assert_eq!(char_from_string("héllo", -1).as_deref(), Some("o"));
        assert_eq!(char_from_string("ab", 5), None);
        assert_eq!(char_from_string("ab", -3), None);
    }

    #[test]
    fn string_slice_inclusive_and_exclusive() {
        // subscript [1:3] is inclusive of index 3
        assert_eq!(string_slice("abcdef", 1, 3, false).as_deref(), Some("bcd"));
        // slice() is exclusive of the end
        assert_eq!(string_slice("abcdef", 1, 3, true).as_deref(), Some("bc"));
        // [1:] to the end
        assert_eq!(string_slice("abcdef", 1, -1, false).as_deref(), Some("bcdef"));
        // empty result
        assert_eq!(string_slice("abc", 2, 1, false), None);
    }

    #[test]
    fn to_name_end_namespace() {
        assert_eq!(to_name_end("foo + 1", true), 3);
        assert_eq!(&"s:var = 1"[..to_name_end("s:var = 1", true)], "s:var");
        // "n:" is not a namespace → name ends before the colon
        assert_eq!(&"n:]"[..to_name_end("n:]", true)], "n");
        assert_eq!(to_name_end("123", true), 0); // invalid start
    }

    #[test]
    fn find_name_end_curly_and_bracket() {
        let (end, s, e) = find_name_end("foo{bar}baz rest", 0);
        assert_eq!(&"foo{bar}baz rest"[..end], "foo{bar}baz");
        assert_eq!((s, e), (Some(3), Some(7)));
        // without FNE_INCL_BR the scan stops at '['
        let (end2, _, _) = find_name_end("foo[0]", 0);
        assert_eq!(&"foo[0]"[..end2], "foo");
        // FNE_INCL_BR (bit0) folds the [idx] subscript into the name span
        let (end2b, _, _) = find_name_end("foo[0]", 1);
        assert_eq!(&"foo[0]"[..end2b], "foo[0]");
        let (end3, s3, _) = find_name_end("plain", 0);
        assert_eq!((end3, s3), (5, None));
    }

    #[test]
    fn get_literal_key_basic() {
        assert_eq!(get_literal_key("key: 1").unwrap().0, "key");
        assert_eq!(get_literal_key("a-b : x").unwrap(), ("a-b".to_string(), ": x"));
        assert!(get_literal_key(":bad").is_none());
    }

    #[test]
    fn var_flavour_classes() {
        assert_eq!(var_flavour("FOO"), VAR_FLAVOUR_SHADA);
        assert_eq!(var_flavour("Foo"), VAR_FLAVOUR_SESSION);
        assert_eq!(var_flavour("foo"), VAR_FLAVOUR_DEFAULT);
    }

    #[test]
    fn check_can_index_by_type() {
        use super::check_can_index;
        use crate::ported::eval_h::{FAIL, OK};
        let tv = |t, v| typval_T { v_type: t, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v };
        // indexable
        assert_eq!(check_can_index(&tv(VAR_STRING, v_string("x".into())), true, false), OK);
        assert_eq!(check_can_index(&tv(VAR_NUMBER, v_number(1)), true, false), OK);
        // not indexable
        assert_eq!(check_can_index(&tv(VAR_FLOAT, v_float(1.0)), true, false), FAIL);
        assert_eq!(check_can_index(&tv(VAR_BOOL, v_bool(crate::ported::eval::typval_defs_h::BoolVarValue::kBoolVarTrue)), true, false), FAIL);
        assert_eq!(check_can_index(&tv(VAR_FUNC, v_string("F".into())), true, false), FAIL);
        // unknown: FAIL only when evaluating
        assert_eq!(check_can_index(&tv(VAR_UNKNOWN, v_number(0)), true, false), FAIL);
        assert_eq!(check_can_index(&tv(VAR_UNKNOWN, v_number(0)), false, false), OK);
    }

    #[test]
    fn grow_string_tv_appends() {
        use super::grow_string_tv;
        use crate::ported::eval_h::{FAIL, OK};
        let mut s = typval_T {
            v_type: VAR_STRING,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_string("foo".to_string()),
        };
        assert_eq!(grow_string_tv(&mut s, "bar"), OK);
        assert!(matches!(&s.vval, v_string(t) if t == "foobar"));
        // a non-string is rejected, left untouched
        let mut n = typval_T {
            v_type: VAR_NUMBER,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_number(1),
        };
        assert_eq!(grow_string_tv(&mut n, "x"), FAIL);
    }

    #[test]
    fn typval_tostring_modes() {
        use super::typval_tostring;
        let s = typval_T {
            v_type: VAR_STRING,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_string("hi".to_string()),
        };
        // unquoted string → raw; quoted → string()-encoded with quotes
        assert_eq!(typval_tostring(Some(&s), false), "hi");
        assert_eq!(typval_tostring(Some(&s), true), "'hi'");
        // missing value
        assert_eq!(typval_tostring(None, false), "(does not exist)");
        // a number is the same with or without quotes
        let n = typval_T {
            v_type: VAR_NUMBER,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_number(42),
        };
        assert_eq!(typval_tostring(Some(&n), false), "42");
    }

    #[test]
    fn eval_expr_func_and_partial() {
        use super::{eval_expr_func, eval_expr_partial};
        use crate::ported::eval::typval::CALL_FUNC_HOOK;
        use crate::ported::eval::typval_defs_h::partial_T;
        use crate::ported::eval_h::{FAIL, OK};
        fn hook(_c: &typval_T, args: &[typval_T]) -> Option<typval_T> {
            Some(typval_T::from(args.len() as i64))
        }
        let saved = CALL_FUNC_HOOK.with(|h| *h.borrow());
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = Some(hook));
        // Funcref by name.
        let f = typval_T { v_type: VAR_FUNC, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v_string("F".into()) };
        let mut rv = typval_T::from(-1);
        assert_eq!(eval_expr_func(&f, &[typval_T::from(1)], &mut rv), OK);
        assert!(matches!(rv.vval, v_number(1)));
        // Empty name → FAIL.
        let empty = typval_T { v_type: VAR_FUNC, v_lock: VarLockStatus::VAR_UNLOCKED, vval: v_string(String::new()) };
        assert_eq!(eval_expr_func(&empty, &[], &mut rv), FAIL);
        // Partial.
        let p = typval_T {
            v_type: VAR_PARTIAL,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_partial(Some(Rc::new(partial_T {
                pt_refcount: 1,
                pt_name: "P".into(),
                pt_argv: vec![],
                pt_dict: None,
            }))),
        };
        let mut rv2 = typval_T::from(-1);
        assert_eq!(eval_expr_partial(&p, &[], &mut rv2), OK);
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = saved);
    }

    #[test]
    fn call_func_helpers_via_hook() {
        use super::{call_func_retlist, call_func_retstr, call_vim_function};
        use crate::ported::eval::typval::CALL_FUNC_HOOK;
        use crate::ported::eval_h::FAIL;
        fn hook_str(_c: &typval_T, _args: &[typval_T]) -> Option<typval_T> {
            Some(typval_T::from("hi".to_string()))
        }
        let saved = CALL_FUNC_HOOK.with(|h| *h.borrow());
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = None);
        // No hook → every form fails.
        let mut rv = typval_T::from(0);
        assert_eq!(call_vim_function("F", &[], &mut rv), FAIL);
        assert_eq!(call_func_retstr("F", &[]), None);
        assert!(call_func_retlist("F", &[]).is_none());
        // String-returning hook: retstr gets it, retlist rejects a non-List.
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = Some(hook_str));
        assert_eq!(call_func_retstr("F", &[]).as_deref(), Some("hi"));
        assert!(call_func_retlist("F", &[]).is_none());
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = saved);
    }

    #[test]
    fn callback_call_via_hook() {
        use super::callback_call;
        use crate::ported::eval::typval::{Callback, CALL_FUNC_HOOK};
        use crate::ported::eval::userfunc::callback_call_retnr;
        fn hook(_callee: &typval_T, args: &[typval_T]) -> Option<typval_T> {
            Some(typval_T::from(args.len() as i64))
        }
        let saved = CALL_FUNC_HOOK.with(|h| *h.borrow());
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = None);
        // None callback never calls; no hook → a Funcref can't call either.
        let mut rv = typval_T::from(0);
        assert!(!callback_call(&Callback::None, &[], &mut rv));
        assert!(!callback_call(&Callback::Funcref("F".into()), &[], &mut rv));
        assert_eq!(callback_call_retnr(&Callback::Funcref("F".into()), &[]), -2);
        // Install the hook and dispatch.
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = Some(hook));
        let mut rv2 = typval_T::from(0);
        assert!(callback_call(
            &Callback::Funcref("F".into()),
            &[typval_T::from(1), typval_T::from(2)],
            &mut rv2
        ));
        assert!(matches!(rv2.vval, v_number(2)));
        assert_eq!(callback_call_retnr(&Callback::Funcref("F".into()), &[typval_T::from(9)]), 1);
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = saved);
    }

    #[test]
    fn luafunc_predicates() {
        // A non-partial value is never the v:lua funcref.
        let s = typval_T {
            v_type: VAR_STRING,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_string("x".to_string()),
        };
        assert!(!tv_is_luafunc(&s));
        // A freshly-built, unrelated partial is not the v:lua identity.
        let other = Rc::new(crate::ported::eval::typval_defs_h::partial_T {
            pt_refcount: 1,
            pt_name: String::new(),
            pt_argv: Vec::new(),
            pt_dict: None,
        });
        assert!(!is_luafunc(&other));
    }

    #[test]
    // `3.14` here is a parse fixture, not an attempt to express π.
    #[allow(clippy::approx_constant)]
    fn string2float_leading_prefix() {
        assert_eq!(string2float("3.14"), (3.14, 4));
        assert_eq!(string2float("3.14abc"), (3.14, 4));
        assert_eq!(string2float("2.5e3xyz"), (2500.0, 5));
        assert_eq!(string2float(".5"), (0.5, 2));
        assert_eq!(string2float("42"), (42.0, 2));
        // No leading number consumes nothing.
        assert_eq!(string2float("abc"), (0.0, 0));
        // inf/nan keywords.
        assert_eq!(string2float("inf"), (f64::INFINITY, 3));
        assert_eq!(string2float("-inf"), (f64::NEG_INFINITY, 4));
        assert!(string2float("nan").0.is_nan());
        // A bare exponent marker is not consumed (strtod stops before "e").
        assert_eq!(string2float("5e"), (5.0, 1));
    }
}
