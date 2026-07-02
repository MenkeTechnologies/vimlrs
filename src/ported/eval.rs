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
/// Port of `eval/buffer.c` (the buffer-introspection eval builtins).
pub mod buffer;
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

use std::cell::RefCell;
use std::rc::Rc;

use crate::ported::eval::typval::{
    tv_blob_copy, tv_blob_equal, tv_clear, tv_dict_equal, tv_equal, tv_get_float,
    tv_get_number_chk, tv_get_string, tv_get_string_chk, tv_list_equal, tv_list_find,
    tv_list_find_nr, tv_list_len, tv_list_watch_add,
};
use crate::ported::eval::typval_defs_h::{
    blob_T, dict_T, list_T, partial_T, typval_T, typval_vval_union::*, varnumber_T, VarLockStatus,
    VarType::*, VARNUMBER_MAX, VARNUMBER_MIN,
};
use crate::ported::eval::vars::skip_var_list;
use crate::ported::eval_h::{exprtype_T, exprtype_T::*, FAIL, OK};
use crate::ported::message::emsg;

// Editor-substrate imports for the position/index leaves ported below
// (`var2fpos`, `list2fpos`, `buf_byteidx_to_charidx`, `buf_charidx_to_byteidx`,
// `eval_for_line`). See the module-level docs in each substrate file.
use crate::ported::buffer::{buf_T, buflist_findnr, curbuf, ml_get_buf, ml_get_buf_len};
use crate::ported::mbyte::utf_ptr2len;
use crate::ported::window::{colnr_T, curwin, linenr_T, pos_T, win_T};

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

/// Port of `set_selfdict()` from `Src/eval.c:6014` — bind `selfdict` to the
/// Funcref/Partial in `rettv` (via [`make_partial`]) unless it is already an
/// explicitly-bound Partial. RUST-PORT NOTE: self-dict binding is not modeled
/// (see [`make_partial`]), so this is a no-op.
pub fn set_selfdict(
    rettv: &mut typval_T,
    selfdict: Option<&Rc<std::cell::RefCell<crate::ported::eval::typval_defs_h::dict_T>>>,
) {
    make_partial(selfdict, rettv);
}

/// Port of `make_partial()` from `Src/eval.c:3803` — turn a `dict.Func` access
/// into a Partial bound to `selfdict`. RUST-PORT NOTE: dict-function (`FC_DICT`)
/// tracking is not modeled (`UserFuncDef` has no `dict` attribute) and the
/// bridge's call path ignores a Partial's self dict, so self-dict binding is
/// absent — this is a no-op, leaving `rettv` as the plain Funcref (matching the
/// interpreter's behavior, where `dict.Func` is not self-bound).
pub fn make_partial(
    _selfdict: Option<&Rc<std::cell::RefCell<crate::ported::eval::typval_defs_h::dict_T>>>,
    _rettv: &mut typval_T,
) {
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
/// `Name(args)` call that skips the full expression parser.
///
/// RUST-PORT NOTE: the tree-walker fast path is not modeled standalone (the
/// bridge compiles calls), so this always declines with `NOTDONE`, i.e. every
/// caller ([`eval0_simple_funccal`]) falls through to the normal [`eval0`] path.
pub fn may_call_simple_func() -> i32 {
    NOTDONE
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

// ── evalarg_T lifecycle (Src/eval.c) ──
//
// RUST-PORT NOTE: the C `evalarg_T` (expression-evaluation context) belongs to
// the C tree-walker, which the fusevm carve-out replaces with its own parser
// state. It is never allocated standalone, so its setup/teardown is a no-op.
// (`lval_T` — the parsed assignment target — is a real port; see [`get_lval`]
// and [`clear_lval`] below.)

/// Port of `fill_evalarg_from_eap()` from `Src/eval.c:229` — populate an
/// `evalarg_T` from an `:`-command. The carve-out parser owns evaluation
/// context, so the C struct is unused → no-op.
pub fn fill_evalarg_from_eap() {}

/// Port of `clear_evalarg()` from `Src/eval.c:1754` — free an `evalarg_T`'s
/// owned strings; `Drop`-managed / struct unused → no-op.
pub fn clear_evalarg() {}

/// Port of `clear_lval()` from `Src/eval.c:1279` — free a parsed [`lval_T`].
///
/// RUST-PORT NOTE: `ll_exp_name`/`ll_newkey` are owned `Option<String>` freed by
/// `Drop`; clearing them mirrors the C `xfree()`s (the `Rc` handle fields drop
/// their references when `lp` itself is dropped, so they are left as-is).
pub fn clear_lval(lp: &mut lval_T) {
    // c: xfree(lp->ll_exp_name);
    lp.ll_exp_name = None;
    // c: xfree(lp->ll_newkey);
    lp.ll_newkey = None;
}

/// Port of `last_set_msg()` from `Src/eval.c:6345` — print where an option or
/// variable was last set (`:verbose set`). No interactive verbose output
/// standalone → no-op (mirrors the `list_*_vars` listing ports).
pub fn last_set_msg() {}

// ── editor-absent hooks (Src/eval.c) ──
//
// RUST-PORT NOTE: vimlrs is a standalone evaluator with no windows, fold engine,
// or interactive command line. These hooks into those subsystems are never
// reached, so they return the "nothing" value (faithful absence).

/// Port of `eval_foldexpr()` from `Src/eval.c:714` — evaluate a window's
/// `'foldexpr'` to a fold level. No windows/fold engine standalone → level 0.
pub fn eval_foldexpr() -> i32 {
    0
}

/// Port of `eval_foldtext()` from `Src/eval.c:765` — evaluate `'foldtext'` to the
/// folded-line display text. No fold engine standalone → empty.
pub fn eval_foldtext() -> String {
    String::new()
}

/// Port of `set_context_for_expression()` from `Src/eval.c:1571` — set the
/// command-line completion context for an expression. No interactive completion
/// standalone → no-op.
pub fn set_context_for_expression() {}

// ── jobs / channels (Src/eval.c) ──
//
// RUST-PORT NOTE: vimlrs has no event loop and no job/channel subsystem, so a
// job lookup never resolves and callback registration does nothing (faithful
// absence, same basis as the timer ports).

/// Port of `find_job()` from `Src/eval.c:6502` — look up a job/channel by id.
/// No jobs exist standalone → `None`.
pub fn find_job() -> Option<u64> {
    None
}

/// Port of `common_job_callbacks()` from `Src/eval.c:6478` — wire up a job's
/// stdout/stderr/exit callbacks. No job subsystem → nothing to register → false.
pub fn common_job_callbacks() -> bool {
    false
}

// ── timers (Src/eval.c) ──
//
// RUST-PORT NOTE: vimlrs has no event loop, so timers cannot be scheduled and
// their libuv callbacks never fire (the same basis as the `f_timer_*` builtins,
// which already return -1/empty). These are faithful "no event loop" no-ops,
// the same category as the prompt/provider/redir absence ports.

/// Port of `timer_start()` from `Src/eval.c:5069` — create and schedule a timer.
/// No event loop → cannot schedule → returns timer id `0` (failure).
pub fn timer_start(
    _timeout: i64,
    _repeat_count: i32,
    _callback: &crate::ported::eval::typval::Callback,
) -> u64 {
    0
}

/// Port of `timer_stop()` from `Src/eval.c:5091` — stop a timer; none exist, no-op.
pub fn timer_stop() {}

/// Port of `timer_decref()` from `Src/eval.c:5113` — drop a timer reference;
/// `Rc`/no-event-loop, no-op.
pub fn timer_decref() {}

/// Port of `timer_due_cb()` from `Src/eval.c:5019` — the libuv "timer fired"
/// callback; never fires without an event loop, no-op.
pub fn timer_due_cb() {}

/// Port of `timer_close_cb()` from `Src/eval.c:5104` — the libuv "timer closed"
/// callback; never fires, no-op.
pub fn timer_close_cb() {}

/// Port of `find_timer_by_nr()` from `Src/eval.c:4980` — look up a timer by id.
/// No timers exist → `None`.
pub fn find_timer_by_nr() -> Option<u64> {
    None
}

/// Port of `add_timer_info()` from `Src/eval.c:4985` — append one timer's info
/// dict to a List; no timers exist, no-op.
pub fn add_timer_info() {}

/// Port of `add_timer_info_all()` from `Src/eval.c:5007` — append every timer's
/// info; no timers exist, so the List stays empty, no-op.
pub fn add_timer_info_all() {}

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
            Some(b) if !exclusive && b < slen => {
                b + str[b..].chars().next().map_or(0, char::len_utf8)
            }
            Some(b) => b,
            None => return None,
        }
    };
    if start_byte >= slen || end_byte <= start_byte {
        return None;
    }
    Some(str[start_byte..end_byte].to_string())
}

/// Port of `eval7_leader()` from `Src/eval.c:2794`.
///
/// Apply the unary leader prefixes `leaders` (already parsed) to `rettv`, in
/// reverse order: `!` is logical-not (→ Number 0/1), `-` negates, `+` forces a
/// Number. With `numeric_only`, a `!` stops processing. Returns [`OK`]/[`FAIL`]
/// (non-numeric value).
pub fn eval7_leader(rettv: &mut typval_T, numeric_only: bool, leaders: &str) -> i32 {
    let is_float = rettv.v_type == VAR_FLOAT;
    let mut f = if is_float {
        crate::ported::eval::typval::tv_get_float(rettv)
    } else {
        0.0
    };
    let mut error = false;
    let mut val = if is_float {
        0
    } else {
        tv_get_number_chk(rettv, Some(&mut error))
    };
    if error {
        return FAIL;
    }
    let mut cur_float = is_float;
    for c in leaders.chars().rev() {
        match c {
            '!' => {
                if numeric_only {
                    break;
                }
                if cur_float {
                    val = i64::from(f == 0.0);
                    cur_float = false;
                } else {
                    val = i64::from(val == 0);
                }
            }
            '-' => {
                if cur_float {
                    f = -f;
                } else {
                    val = -val;
                }
            }
            _ => {} // '+' — no-op (forces Number)
        }
    }
    *rettv = if cur_float {
        typval_T {
            v_type: VAR_FLOAT,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_float(f),
        }
    } else {
        typval_T::from(val)
    };
    OK
}

// ── binary-operator helpers (Src/eval.c) ──
//
// The per-operator semantics behind `eval5`/`eval6` (`+`/`-`/`.`/`*`/`/`),
// alongside the already-ported `num_divide`/`num_modulus`. Each combines `tv2`
// into `tv1` in place.

/// Port of `eval_addblob()` from `Src/eval.c:2256` — concatenate Blobs
/// `tv1 ++ tv2` into `tv1`.
pub fn eval_addblob(tv1: &mut typval_T, tv2: &typval_T) {
    let bytes = |tv: &typval_T| -> Vec<u8> {
        match &tv.vval {
            v_blob(Some(b)) => b.borrow().bv_ga.clone(),
            _ => Vec::new(),
        }
    };
    let mut joined = bytes(tv1);
    joined.extend(bytes(tv2));
    let nb = crate::ported::eval::typval::tv_blob_alloc();
    nb.borrow_mut().bv_ga = joined;
    crate::ported::eval::typval::tv_blob_set_ret(tv1, nb);
}

/// Port of `eval_addlist()` from `Src/eval.c:2230` — copy List `tv1` and append
/// List `tv2`, storing the result in `tv1`. Returns [`OK`]/[`FAIL`].
pub fn eval_addlist(tv1: &mut typval_T, tv2: &typval_T) -> i32 {
    let l1 = match &tv1.vval {
        v_list(l) => l.clone(),
        _ => None,
    };
    let l2 = match &tv2.vval {
        v_list(l) => l.clone(),
        _ => None,
    };
    let mut var3 = typval_T::from(0);
    if crate::ported::eval::typval::tv_list_concat(l1.as_ref(), l2.as_ref(), &mut var3) == FAIL {
        return FAIL;
    }
    *tv1 = var3;
    OK
}

/// Port of `eval_concat_str()` from `Src/eval.c:2288` — concatenate the string
/// values of `tv1` and `tv2` into `tv1`. Returns [`OK`]/[`FAIL`] (type error).
pub fn eval_concat_str(tv1: &mut typval_T, tv2: &typval_T) -> i32 {
    let s2 = match crate::ported::eval::typval::tv_get_string_chk(tv2) {
        Some(s) => s,
        None => return FAIL,
    };
    if grow_string_tv(tv1, &s2) == OK {
        return OK;
    }
    let s1 = tv_get_string(tv1);
    *tv1 = typval_T::from(format!("{s1}{s2}"));
    OK
}

/// Port of `eval_addsub_number()` from `Src/eval.c:2316` — add (`op == b'+'`) or
/// subtract `tv2` from `tv1`, as Number or (if either is Float) Float, into
/// `tv1`. Returns [`OK`]/[`FAIL`] (non-numeric operand).
pub fn eval_addsub_number(tv1: &mut typval_T, tv2: &typval_T, op: u8) -> i32 {
    let f = tv1.v_type == VAR_FLOAT || tv2.v_type == VAR_FLOAT;
    let mut error = false;
    let n1 = if tv1.v_type == VAR_FLOAT {
        0
    } else {
        tv_get_number_chk(tv1, Some(&mut error))
    };
    if error {
        return FAIL;
    }
    let n2 = if tv2.v_type == VAR_FLOAT {
        0
    } else {
        tv_get_number_chk(tv2, Some(&mut error))
    };
    if error {
        return FAIL;
    }
    if f {
        let a = if tv1.v_type == VAR_FLOAT {
            crate::ported::eval::typval::tv_get_float(tv1)
        } else {
            n1 as f64
        };
        let b = if tv2.v_type == VAR_FLOAT {
            crate::ported::eval::typval::tv_get_float(tv2)
        } else {
            n2 as f64
        };
        *tv1 = typval_T {
            v_type: VAR_FLOAT,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_float(if op == b'+' { a + b } else { a - b }),
        };
    } else {
        *tv1 = typval_T::from(if op == b'+' { n1 + n2 } else { n1 - n2 });
    }
    OK
}

/// Port of `eval_multdiv_number()` from `Src/eval.c:2456` — multiply (`*`),
/// divide (`/`), or modulo (`%`) `tv1` by `tv2` into `tv1`. Float when either is
/// Float (`%` on a Float is the error `E804`); Number division/modulo go through
/// [`num_divide`]/[`num_modulus`]. Returns [`OK`]/[`FAIL`].
pub fn eval_multdiv_number(tv1: &mut typval_T, tv2: &typval_T, op: u8) -> i32 {
    let use_float = tv1.v_type == VAR_FLOAT || tv2.v_type == VAR_FLOAT;
    let mut error = false;
    let n1 = if tv1.v_type == VAR_FLOAT {
        0
    } else {
        tv_get_number_chk(tv1, Some(&mut error))
    };
    if error {
        return FAIL;
    }
    let n2 = if tv2.v_type == VAR_FLOAT {
        0
    } else {
        tv_get_number_chk(tv2, Some(&mut error))
    };
    if error {
        return FAIL;
    }
    if use_float {
        let f1 = if tv1.v_type == VAR_FLOAT {
            crate::ported::eval::typval::tv_get_float(tv1)
        } else {
            n1 as f64
        };
        let f2 = if tv2.v_type == VAR_FLOAT {
            crate::ported::eval::typval::tv_get_float(tv2)
        } else {
            n2 as f64
        };
        let r = match op {
            b'*' => f1 * f2,
            b'/' => {
                if f2 == 0.0 {
                    if f1 == 0.0 {
                        f64::NAN
                    } else if f1 > 0.0 {
                        f64::INFINITY
                    } else {
                        f64::NEG_INFINITY
                    }
                } else {
                    f1 / f2
                }
            }
            _ => {
                emsg("E804: Cannot use '%' with Float");
                return FAIL;
            }
        };
        *tv1 = typval_T {
            v_type: VAR_FLOAT,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_float(r),
        };
    } else {
        let r = match op {
            b'*' => n1.wrapping_mul(n2),
            b'/' => num_divide(n1, n2),
            _ => num_modulus(n1, n2),
        };
        *tv1 = typval_T::from(r);
    }
    OK
}

/// Port of `do_string_sub()` from `Src/eval.c:6377`.
///
/// Substitute `pat` with `sub` in `str` (the engine behind `substitute()`);
/// `flags` may contain `g`. A `\=expr` replacement is evaluated via the regex
/// engine's substitute-expression hook. RUST-PORT NOTE: the C `expr` typval and
/// `ret_len` out-param are folded into `crate::viml_regex::regex_substitute`.
pub fn do_string_sub(str: &str, pat: &str, sub: &str, flags: &str) -> String {
    crate::viml_regex::regex_substitute(str, pat, sub, flags)
}

/// Port of `make_expanded_name()` from `Src/eval.c:5708`.
///
/// Expand the `{expr}` curly braces in a variable/function name: evaluate the
/// expression between `expr_start` (the `{`) and `expr_end` (the `}`) in `name`,
/// splice its string value in, then recurse for any further `{…}`. Returns the
/// fully-expanded name, or `None` if an `{expr}` fails to evaluate.
pub fn make_expanded_name(name: &str, expr_start: usize, expr_end: usize) -> Option<String> {
    let val = eval_to_string(&name[expr_start + 1..expr_end])?;
    let expanded = format!("{}{}{}", &name[..expr_start], val, &name[expr_end + 1..]);
    // Recurse if the spliced-in result still contains a `{…}` group.
    match find_name_end(&expanded, 0) {
        (_, Some(es), Some(ee)) => make_expanded_name(&expanded, es, ee),
        _ => Some(expanded),
    }
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
        if bytes[p] == b':' && (p != 1 || !use_namespace || !b"bgstvw".contains(&bytes[0])) {
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
            if (len > 1 && b[p - 1] != b'}') || (len == 1 && !NAMESPACE_CHAR.contains(&b[0])) {
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

/// Port of `string_to_list()` from `Src/eval.c:4703`.
///
/// Split a string into a List of lines (used by `systemlist()`): NL separates
/// items and embedded NUL maps to NL. With `keepempty` false a single trailing
/// NL is dropped first, so it does not yield a trailing empty item.
pub fn string_to_list(
    s: &str,
    keepempty: bool,
) -> Rc<std::cell::RefCell<crate::ported::eval::typval_defs_h::list_T>> {
    let s = if !keepempty && s.ends_with('\n') {
        &s[..s.len() - 1]
    } else {
        s
    };
    let list = crate::ported::eval::typval::tv_list_alloc(-1);
    crate::ported::eval::encode::encode_list_write(&mut list.borrow_mut(), s);
    list
}

/// Port of `save_tv_as_string()` from `Src/eval.c:5143`.
///
/// Render `tv` as the byte string `writefile()`/channel-send would emit: a List
/// of strings is joined by NL (or CRLF when `crlf`), with a trailing separator
/// when `endnl` and each item's embedded NL mapped to NUL; a scalar is its
/// string value. Returns `None` for an Unknown value or a Number (the C treats
/// a Number as a buffer id, but no buffers exist standalone).
pub fn save_tv_as_string(tv: &typval_T, endnl: bool, crlf: bool) -> Option<String> {
    match tv.v_type {
        VAR_UNKNOWN | VAR_NUMBER => None,
        VAR_LIST => match &tv.vval {
            v_list(Some(l)) => {
                let l = l.borrow();
                let mut out = String::new();
                let n = l.lv_items.len();
                for (i, it) in l.lv_items.iter().enumerate() {
                    out.push_str(&tv_get_string(&it.li_tv).replace('\n', "\0"));
                    if endnl || i + 1 < n {
                        if crlf {
                            out.push('\r');
                        }
                        out.push('\n');
                    }
                }
                Some(out)
            }
            _ => Some(String::new()),
        },
        _ => Some(tv_get_string(tv)),
    }
}

/// Port of `os_can_exe()` from `src/nvim/os/fs.c` (extern; not vendored under
/// `csrc/`) — resolve `name` to an executable full path (a name containing a
/// path separator is checked directly, else searched on `$PATH`), or `None`.
///
/// RUST-PORT NOTE: the same leaf is ported module-private in
/// [`fs`](crate::ported::eval::fs) for `executable()`/`exepath()`; this copy
/// keeps identical logic so [`tv_to_argv`] can resolve its `argv[0]` (the two
/// should be deduplicated once one is made `pub`).
fn os_can_exe(name: &str) -> Option<String> {
    use std::path::Path;
    let is_exe = |p: &Path| -> bool {
        match std::fs::metadata(p) {
            Ok(m) if m.is_file() => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    m.permissions().mode() & 0o111 != 0
                }
                #[cfg(not(unix))]
                {
                    true
                }
            }
            _ => false,
        }
    };
    if name.contains('/') {
        return is_exe(Path::new(name)).then(|| name.to_string());
    }
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let cand = dir.join(name);
        if is_exe(&cand) {
            return Some(cand.to_string_lossy().into_owned());
        }
    }
    None
}

/// Port of `shell_build_argv()` from `src/nvim/os/shell.c` (extern; not vendored
/// under `csrc/`) — build the shell argv that runs command string `cmd`.
///
/// RUST-PORT NOTE: the C reads `'shell'`/`'shellcmdflag'`; standalone those
/// options are not modeled, so the POSIX default `sh -c <cmd>` is used.
fn shell_build_argv(cmd: &str) -> Vec<String> {
    vec!["sh".to_string(), "-c".to_string(), cmd.to_string()]
}

/// Port of `os_system()` from `src/nvim/os/shell.c` (extern; not vendored under
/// `csrc/`) — run `argv`, writing `input` to its stdin, and capture stdout.
/// Returns `(exit_status, output)` with `output` `None` when the command
/// produced no bytes, mirroring the C `*output == NULL` case.
///
/// RUST-PORT NOTE: the C drives its libuv process loop; here `std::process` runs
/// the child. stderr is inherited (shown), as Vim does by default. The status is
/// `-1` when the process could not be started.
fn os_system(argv: &[String], input: Option<&str>) -> (i32, Option<String>) {
    use std::io::Write;
    use std::process::{Command, Stdio};

    if argv.is_empty() {
        return (-1, None);
    }
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]).stdout(Stdio::piped());
    command.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });

    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(_) => return (-1, None),
    };
    if let Some(text) = input {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
    }
    match child.wait_with_output() {
        Ok(out) => {
            let status = out.status.code().unwrap_or(-1);
            if out.stdout.is_empty() {
                (status, None)
            } else {
                (
                    status,
                    Some(String::from_utf8_lossy(&out.stdout).into_owned()),
                )
            }
        }
        Err(_) => (-1, None),
    }
}

/// Builds a process argument vector from a Vimscript object (`typval_T`).
/// Port of `tv_to_argv()` from `csrc/eval.c:4644`.
///
/// @param cmd_tv      Vimscript object
/// @param cmd         Returns the command or executable name.
/// @param executable  Set to `false` if argv[0] is not executable.
///
/// @return  the shell argv when `cmd_tv` is a String; else the List's string
///          values with argv[0] resolved to a full path, or `None` on error.
pub fn tv_to_argv(
    cmd_tv: &typval_T,
    mut cmd: Option<&mut String>,
    mut executable: Option<&mut bool>,
) -> Option<Vec<String>> {
    use crate::ported::eval::typval::{tv_get_string_chk, tv_list_len};

    if cmd_tv.v_type == VAR_STRING {
        // c:4646 String => "shell semantics".
        let cmd_str = tv_get_string(cmd_tv);
        if let Some(c) = cmd.as_deref_mut() {
            *c = cmd_str.clone(); // c:4649
        }
        return Some(shell_build_argv(&cmd_str)); // c:4651
    }

    if cmd_tv.v_type != VAR_LIST {
        crate::ported::message::semsg(&format!(
            "E475: Invalid argument: {}",
            "expected String or List"
        )); // c:4655 e_invarg2
        return None;
    }

    let argl = match &cmd_tv.vval {
        v_list(Some(l)) => l.clone(),
        _ => return None,
    }; // c:4659
    let argc = tv_list_len(&argl.borrow()); // c:4660
    if argc == 0 {
        crate::ported::message::emsg("E474: Invalid argument"); // c:4662 e_invarg
        return None;
    }

    // c:4666 Resolve argv[0] to a full executable path.
    let arg0 = tv_get_string_chk(&argl.borrow().lv_items[0].li_tv);
    let exe_resolved = arg0.as_deref().and_then(os_can_exe);
    let exe_resolved = match (arg0.as_deref(), exe_resolved) {
        (Some(_), Some(r)) => r,
        (a0, _) => {
            // c:4668 !arg0 || !os_can_exe(...)
            if let (Some(a0), Some(exe)) = (a0, executable.as_deref_mut()) {
                crate::ported::message::semsg(&format!(
                    "E475: Invalid value for argument {}: {}",
                    "cmd",
                    format!("'{a0}' is not executable")
                )); // c:4672 e_invargNval
                *exe = false; // c:4673
            }
            return None;
        }
    };

    if let Some(c) = cmd.as_deref_mut() {
        *c = exe_resolved.clone(); // c:4679
    }

    // c:4682 Build the argument vector.
    let mut argv: Vec<String> = Vec::with_capacity(argc as usize);
    for item in argl.borrow().lv_items.iter() {
        match tv_get_string_chk(&item.li_tv) {
            Some(a) => argv.push(a), // c:4693
            None => return None,     // c:4688 emsg already done in tv_get_string_chk
        }
    }
    // c:4695 Replace argv[0] with the absolute path.
    if let Some(first) = argv.first_mut() {
        *first = exe_resolved;
    }

    Some(argv)
}

/// os_system wrapper. Handles `v:shell_error`.
/// Port of `get_system_output_as_rettv()` from `csrc/eval.c:4714`.
///
/// RUST-PORT NOTE: `check_secure()` (restricted mode), `:profile`, and the
/// `'verbose' > 3` echo are editor state not modeled standalone and are dropped.
/// The `USE_CRNL` <CR><NL> translation is Windows-only and omitted.
pub fn get_system_output_as_rettv(argvars: &[typval_T], rettv: &mut typval_T, retlist: bool) {
    use crate::ported::eval::typval::{tv_get_number, tv_list_ref};
    use crate::ported::eval::vars::{set_vim_var_nr, vv::VV_SHELL_ERROR};

    rettv.v_type = VAR_STRING; // c:4719
    rettv.vval = v_string(String::new()); // c:4720 v_string = NULL

    // c:4726 get input to the shell command (if any).
    let has_input = argvars.len() > 1 && argvars[1].v_type != VAR_UNKNOWN;
    let input = if has_input {
        match save_tv_as_string(&argvars[1], false, false) {
            Some(s) => Some(s),
            None => return, // c:4729 input_len < 0
        }
    } else {
        None
    };

    // c:4734 get shell command to execute.
    let mut executable = true;
    let argv = match tv_to_argv(&argvars[0], None, Some(&mut executable)) {
        Some(a) => a,
        None => {
            // c:4737 Already did emsg.
            if !executable {
                set_vim_var_nr(VV_SHELL_ERROR, -1); // c:4739
            }
            return;
        }
    };

    // c:4758 execute the command.
    let (status, res) = os_system(&argv, input.as_deref()); // c:4761

    set_vim_var_nr(VV_SHELL_ERROR, status as varnumber_T); // c:4769

    let res = match res {
        Some(r) => r,
        None => {
            // c:4771 res == NULL
            if retlist {
                crate::ported::eval::typval::tv_list_alloc_ret(rettv, 0); // c:4774
            } else {
                rettv.vval = v_string(String::new()); // c:4776 xstrdup("")
            }
            return;
        }
    };

    if retlist {
        let mut keepempty = false; // c:4782
        if argvars.len() > 2 && argvars[1].v_type != VAR_UNKNOWN && argvars[2].v_type != VAR_UNKNOWN
        {
            keepempty = tv_get_number(&argvars[2]) != 0; // c:4784
        }
        let list = string_to_list(&res, keepempty); // c:4786
        tv_list_ref(&mut list.borrow_mut()); // c:4787
        rettv.v_type = VAR_LIST; // c:4788
        rettv.vval = v_list(Some(list));
    } else {
        // c:4792 res may contain several NULs; replace with SOH (1) to avoid
        // truncation, like get_cmd_output().
        rettv.vval = v_string(res.replace('\0', "\u{1}")); // c:4794 memchrsub
    }
}

/// Port of `eval_index()` from `Src/eval.c:3092`.
///
/// Apply one subscript `subscript` (`.name`, `[expr]`, or `[a:b]`) to `rettv`.
/// Index expressions are evaluated via `EVAL_STRING_HOOK` and applied with
/// [`eval_index_inner`]. Returns [`OK`]/[`FAIL`]. RUST-PORT NOTE: the C parses
/// the subscript off the expression; here one isolated subscript is passed in.
pub fn eval_index(rettv: &mut typval_T, subscript: &str, verbose: bool) -> i32 {
    if check_can_index(rettv, true, verbose) != OK {
        return FAIL;
    }
    let eval = |e: &str| -> Option<typval_T> {
        crate::ported::eval::typval::EVAL_STRING_HOOK
            .with(|h| *h.borrow())
            .and_then(|f| f(e))
    };
    if let Some(key) = subscript.strip_prefix('.') {
        return eval_index_inner(rettv, false, None, None, false, Some(key), verbose);
    }
    let inner = match subscript
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
    {
        Some(i) => i,
        None => return FAIL,
    };
    // Find a top-level ':' (range), balancing brackets and skipping strings.
    let colon = {
        let b = inner.as_bytes();
        let (mut depth, mut i, mut pos) = (0i32, 0usize, None);
        while i < b.len() {
            match b[i] {
                b'\'' => {
                    i += 1;
                    while i < b.len() && b[i] != b'\'' {
                        i += 1;
                    }
                }
                b'"' => {
                    i += 1;
                    while i < b.len() && b[i] != b'"' {
                        if b[i] == b'\\' && i + 1 < b.len() {
                            i += 1;
                        }
                        i += 1;
                    }
                }
                b'[' | b'(' | b'{' => depth += 1,
                b']' | b')' | b'}' => depth -= 1,
                b':' if depth == 0 => {
                    pos = Some(i);
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        pos
    };
    let parse_side = |s: &str| -> Result<Option<typval_T>, ()> {
        let s = s.trim();
        if s.is_empty() {
            Ok(None)
        } else {
            eval(s).map(Some).ok_or(())
        }
    };
    match colon {
        None => {
            let n1 = match eval(inner.trim()) {
                Some(v) => v,
                None => return FAIL,
            };
            eval_index_inner(rettv, false, Some(&n1), None, false, None, verbose)
        }
        Some(c) => {
            let (Ok(v1), Ok(v2)) = (parse_side(&inner[..c]), parse_side(&inner[c + 1..])) else {
                return FAIL;
            };
            eval_index_inner(rettv, true, v1.as_ref(), v2.as_ref(), false, None, verbose)
        }
    }
}

/// Port of `eval_index_inner()` from `Src/eval.c:3237`.
///
/// Apply a subscript (`var1`) or slice (`var1:var2`, `exclusive` for `slice()`)
/// — or a Dict key — to `rettv`, in place. RUST-PORT NOTE: unlike the C's
/// byte-indexed String path, the String case is character-indexed (via the
/// ported `string_slice`/`char_from_string`), matching the interpreter's
/// char-based string subscripting. Returns [`OK`]/[`FAIL`].
#[allow(clippy::too_many_arguments)]
pub fn eval_index_inner(
    rettv: &mut typval_T,
    is_range: bool,
    var1: Option<&typval_T>,
    var2: Option<&typval_T>,
    exclusive: bool,
    key: Option<&str>,
    verbose: bool,
) -> i32 {
    let n1 = match var1 {
        Some(v) if rettv.v_type != VAR_DICT => tv_get_number_chk(v, None),
        _ => 0,
    };
    let n2 = if is_range {
        if rettv.v_type == VAR_DICT {
            if verbose {
                emsg("E719: Cannot slice a Dictionary");
            }
            return FAIL;
        }
        var2.map_or(VARNUMBER_MAX, |t| tv_get_number_chk(t, None))
    } else {
        0
    };

    match rettv.v_type {
        VAR_BOOL | VAR_SPECIAL | VAR_FUNC | VAR_FLOAT | VAR_PARTIAL | VAR_UNKNOWN => OK,
        VAR_NUMBER | VAR_STRING => {
            let s = tv_get_string(rettv);
            let v = if is_range {
                string_slice(&s, n1, n2, exclusive)
            } else {
                char_from_string(&s, n1)
            };
            *rettv = typval_T::from(v.unwrap_or_default());
            OK
        }
        VAR_BLOB => {
            let b = match &rettv.vval {
                v_blob(Some(b)) => b.clone(),
                _ => return OK,
            };
            let bb = b.borrow();
            crate::ported::eval::typval::tv_blob_slice_or_index(
                &bb, is_range, n1, n2, exclusive, rettv,
            )
        }
        VAR_LIST => {
            let l = match &rettv.vval {
                v_list(Some(l)) => l.clone(),
                _ => return OK,
            };
            crate::ported::eval::typval::tv_list_slice_or_index(
                &l, is_range, n1, n2, exclusive, rettv, verbose,
            )
        }
        VAR_DICT => {
            let d = match &rettv.vval {
                v_dict(Some(d)) => d.clone(),
                _ => return FAIL,
            };
            let k = match key
                .map(String::from)
                .or_else(|| var1.and_then(crate::ported::eval::typval::tv_get_string_chk))
            {
                Some(k) => k,
                None => return FAIL,
            };
            let found = crate::ported::eval::typval::tv_dict_find(&d.borrow(), &k).cloned();
            match found {
                Some(v) if !tv_is_luafunc(&v) => {
                    *rettv = v;
                    OK
                }
                _ => {
                    if verbose {
                        emsg(&format!("E716: Key not present in Dictionary: \"{k}\""));
                    }
                    FAIL
                }
            }
        }
    }
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

// ── string-expression evaluation entry points (Src/eval.c) ──
//
// RUST-PORT NOTE: the C `eval0`…`eval7` recursive tree-walker is replaced by the
// fusevm carve-out; these high-level wrappers (which in C drive `eval0`) instead
// compile-and-run the expression string through the bridge's `EVAL_STRING_HOOK`,
// producing the same result. `None` on a parse/eval error.

/// Port of `eval_expr_string()` from `Src/eval.c:367` — evaluate the string in
/// `expr` as an expression, storing the result in `rettv`. Returns
/// [`OK`]/[`FAIL`].
pub fn eval_expr_string(expr: &typval_T, rettv: &mut typval_T) -> i32 {
    let s = tv_get_string(expr);
    match crate::ported::eval::typval::EVAL_STRING_HOOK
        .with(|h| *h.borrow())
        .and_then(|f| f(s.trim_start()))
    {
        Some(result) => {
            *rettv = result;
            OK
        }
        None => FAIL,
    }
}

/// Port of `eval_to_string_eap()` from `Src/eval.c:510` — evaluate expression
/// string `arg` and render the result with [`typval2string`] (`join_list`
/// controls List rendering). RUST-PORT NOTE: the C `exarg_T` and
/// `use_simple_function` are not modeled.
pub fn eval_to_string_eap(arg: &str, join_list: bool) -> Option<String> {
    crate::ported::eval::typval::EVAL_STRING_HOOK
        .with(|h| *h.borrow())
        .and_then(|f| f(arg))
        .map(|tv| typval2string(&tv, join_list))
}

/// Port of `eval_to_string()` from `Src/eval.c:531` — evaluate expression string
/// `arg` and return the result rendered as a String, or `None` on error.
pub fn eval_to_string(arg: &str) -> Option<String> {
    eval_to_string_eap(arg, false)
}

/// Port of `eval_to_string_safe()` from `Src/eval.c:540` — [`eval_to_string`]
/// without the caller's local variables and under textlock. RUST-PORT NOTE: the
/// sandbox/textlock/funccal save-restore are not modeled standalone.
pub fn eval_to_string_safe(arg: &str) -> Option<String> {
    eval_to_string_eap(arg, false)
}

/// Port of `eval_to_string_skip()` from `Src/eval.c:433` — evaluate `arg` to a
/// String, or `None` when `skip` (parse-only, no evaluation).
pub fn eval_to_string_skip(arg: &str, skip: bool) -> Option<String> {
    if skip {
        None
    } else {
        eval_to_string_eap(arg, false)
    }
}

/// Port of `eval_expr_ext()` from `Src/eval.c:598` — evaluate expression string
/// `arg`, returning the resulting value or `None`. RUST-PORT NOTE: the C
/// `exarg_T`/`use_simple_function` are not modeled (same as [`eval_expr`]).
pub fn eval_expr_ext(arg: &str) -> Option<typval_T> {
    eval_expr(arg)
}

/// Port of `eval_to_number()` from `Src/eval.c:563` — evaluate expression string
/// `arg` and return the result as a Number (`-1` on error).
pub fn eval_to_number(arg: &str) -> varnumber_T {
    crate::ported::eval::typval::EVAL_STRING_HOOK
        .with(|h| *h.borrow())
        .and_then(|f| f(arg))
        .map_or(-1, |tv| tv_get_number_chk(&tv, None))
}

/// Port of `eval_expr()` from `Src/eval.c:593` — evaluate expression string
/// `arg` and return the resulting value, or `None` on error.
pub fn eval_expr(arg: &str) -> Option<typval_T> {
    crate::ported::eval::typval::EVAL_STRING_HOOK
        .with(|h| *h.borrow())
        .and_then(|f| f(arg))
}

/// Port of `eval_expr_typval()` from `Src/eval.c:395` — evaluate `expr` (a
/// Partial, a Funcref/name when `want_func`, else an expression string) with
/// `argv`, into `rettv`. Returns [`OK`]/[`FAIL`].
pub fn eval_expr_typval(
    expr: &typval_T,
    want_func: bool,
    argv: &[typval_T],
    rettv: &mut typval_T,
) -> i32 {
    match expr.v_type {
        VAR_PARTIAL => eval_expr_partial(expr, argv, rettv),
        VAR_FUNC => eval_expr_func(expr, argv, rettv),
        _ if want_func => eval_expr_func(expr, argv, rettv),
        _ => eval_expr_string(expr, rettv),
    }
}

/// Port of `eval_expr_to_bool()` from `Src/eval.c:411` — like [`eval_to_bool`]
/// but from a typval (string, Funcref, or Partial). `false` on error.
pub fn eval_expr_to_bool(expr: &typval_T) -> bool {
    let mut rettv = typval_T::from(0);
    if eval_expr_typval(expr, false, &[], &mut rettv) == FAIL {
        return false;
    }
    tv_get_number_chk(&rettv, None) != 0
}

/// Port of `eval_to_bool()` from `Src/eval.c:249` — evaluate expression string
/// `arg` and return whether the result is non-zero. RUST-PORT NOTE: the C
/// `skip`/`use_simple_function`/error out-param are not modeled; a parse/eval
/// error yields `false`.
pub fn eval_to_bool(arg: &str) -> bool {
    crate::ported::eval::typval::EVAL_STRING_HOOK
        .with(|h| *h.borrow())
        .and_then(|f| f(arg))
        .is_some_and(|tv| tv_get_number_chk(&tv, None) != 0)
}

/// Port of `eval1_emsg()` from `Src/eval.c:281` — evaluate one expression string
/// `arg` into `rettv`, giving an error message on failure. Returns [`OK`]/[`FAIL`].
pub fn eval1_emsg(arg: &str, rettv: &mut typval_T) -> i32 {
    match crate::ported::eval::typval::EVAL_STRING_HOOK
        .with(|h| *h.borrow())
        .and_then(|f| f(arg))
    {
        Some(result) => {
            *rettv = result;
            OK
        }
        None => FAIL,
    }
}

/// Port of `handle_subscript()` from `Src/eval.c:5931`.
///
/// Apply a chain of subscripts to `rettv` in order — `.key`/`[idx]`/`[a:b]`
/// indexing ([`eval_index`]), `(args)` Funcref calls ([`call_func_rettv`]), and
/// `->method(args)` method calls ([`eval_method`]). Returns [`OK`]/[`FAIL`].
/// RUST-PORT NOTE: the C scans the chain off the expression; here the already
/// split subscript strings are passed in.
pub fn handle_subscript(rettv: &mut typval_T, subscripts: &[&str], verbose: bool) -> i32 {
    for sub in subscripts {
        let r = if let Some(m) = sub.strip_prefix("->") {
            // `->method(args)` — split the method name from its argument text.
            match m.find('(') {
                Some(open) if m.ends_with(')') => {
                    let base = rettv.clone();
                    eval_method(&m[..open], &m[open + 1..m.len() - 1], &base, rettv)
                }
                _ => FAIL,
            }
        } else if let Some(a) = sub.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
            call_func_rettv(rettv, a)
        } else {
            eval_index(rettv, sub, verbose)
        };
        if r == FAIL {
            return FAIL;
        }
    }
    OK
}

/// Port of `call_func_rettv()` from `Src/eval.c:2853`.
///
/// Call the Funcref/Partial value currently in `rettv` with the argument text
/// `args`, replacing `rettv` with the result. Returns [`OK`]/[`FAIL`]. RUST-PORT
/// NOTE: the C parses `(args)` off the expression and threads a self-dict; here
/// the isolated argument text is passed in and dispatch goes through
/// [`eval_expr_func`] (which honors a Partial's bound args, not its self dict).
pub fn call_func_rettv(rettv: &mut typval_T, args: &str) -> i32 {
    let argvars = match crate::ported::eval::userfunc::get_func_arguments(args) {
        Some(a) => a,
        None => return FAIL,
    };
    let callee = rettv.clone();
    eval_expr_func(&callee, &argvars, rettv)
}

/// Port of `eval_func()` from `Src/eval.c:1698`.
///
/// Evaluate a function or method call: with a `basetv` it is a method call
/// ([`eval_method`]), otherwise a plain call
/// ([`get_func_tv`](crate::ported::eval::userfunc::get_func_tv)). Returns
/// [`OK`]/[`FAIL`]. RUST-PORT NOTE: the C resolves the name and parses the call
/// off the expression; here the name and isolated argument text are passed in.
pub fn eval_func(name: &str, args: &str, basetv: Option<&typval_T>, rettv: &mut typval_T) -> i32 {
    match basetv {
        Some(base) => eval_method(name, args, base, rettv),
        None => crate::ported::eval::userfunc::get_func_tv(name, args, rettv),
    }
}

/// Port of `eval_method()` from `Src/eval.c:2955`.
///
/// Dispatch a method call `base->method(args)`: a builtin via
/// [`call_internal_method`](crate::ported::eval::funcs::call_internal_method)
/// (which inserts the base at the builtin's method-base position), else a user
/// function with the base prepended as the first argument. Returns [`OK`]/[`FAIL`].
/// RUST-PORT NOTE: the C parses `->method(args)` off the expression; here the
/// method name and isolated argument text are passed in.
pub fn eval_method(method: &str, args: &str, basetv: &typval_T, rettv: &mut typval_T) -> i32 {
    use crate::ported::eval::userfunc::fcerr::*;
    let argvars = match crate::ported::eval::userfunc::get_func_arguments(args) {
        Some(a) => a,
        None => return FAIL,
    };
    match crate::ported::eval::funcs::call_internal_method(method, &argvars, basetv, rettv) {
        FCERR_NONE => OK,
        FCERR_UNKNOWN => {
            // Not a builtin → user function: the base is the first argument.
            let mut full = Vec::with_capacity(argvars.len() + 1);
            full.push(basetv.clone());
            full.extend(argvars);
            crate::ported::eval::userfunc::call_func(method, &full, rettv)
        }
        _ => FAIL,
    }
}

/// Evaluate "->method()" when the method is a lambda: `base->{...}(args)`.
/// Port of `eval_lambda()` from `csrc/eval.c:2914`.
///
/// `*arg` points to the `-` of the `->`; on OK it is advanced to after the `)`.
/// `rettv` holds the base value on entry and the call result on return.
///
/// RUST-PORT NOTE: the C `call_func_rettv(arg, evalarg, rettv, evaluate, NULL,
/// &base, NULL)` threads the parse pointer through the callee; here the callee is
/// the [`get_lambda_tv`](crate::ported::eval::userfunc::get_lambda_tv) result
/// (always a `VAR_PARTIAL`), the parenthesised argument text is isolated and run
/// through [`get_func_arguments`](crate::ported::eval::userfunc::get_func_arguments),
/// the base is prepended (method-base semantics), and the call is dispatched via
/// [`eval_expr_partial`]. In skip mode (`!evaluate`) the arguments are only
/// consumed, matching the C fast path.
pub fn eval_lambda(
    arg: &mut &str,
    rettv: &mut typval_T,
    mut evalarg: Option<&mut evalarg_T>,
    verbose: bool,
) -> i32 {
    let evaluate = evalarg
        .as_deref()
        .map(|e| e.eval_flags & EVAL_EVALUATE != 0)
        .unwrap_or(false); // c:2918
                           // c:2920 Skip over the ->.
    {
        let s = *arg;
        *arg = &s[2..];
    }
    let mut base = rettv.clone(); // c:2921 typval_T base = *rettv
    *rettv = typval_T::default(); // c:2922 rettv->v_type = VAR_UNKNOWN

    let mut ret = crate::ported::eval::userfunc::get_lambda_tv(arg, rettv, evalarg.as_deref()); // c:2924
    if ret != OK {
        return FAIL; // c:2926
    } else if (*arg).as_bytes().first() != Some(&b'(') {
        // c:2927 **arg != '('
        if verbose {
            if skipwhite(*arg).as_bytes().first() == Some(&b'(') {
                emsg("E274: No white space allowed before parenthesis"); // c:2930 e_nowhitespace
            } else {
                crate::ported::message::semsg(&format!("E107: Missing parentheses: {}", "lambda"));
                // c:2932 e_missingparen
            }
        }
        crate::ported::eval::typval::tv_clear(rettv);
        ret = FAIL; // c:2936
    } else {
        // c:2938 call_func_rettv(arg, evalarg, rettv, evaluate, NULL, &base, NULL)
        // Isolate the balanced "( … )" argument text, advancing *arg past ')'.
        let src = *arg;
        let b = src.as_bytes();
        let (mut depth, mut i, mut endp) = (0i32, 0usize, None);
        while i < b.len() {
            match b[i] {
                b'\'' => {
                    i += 1;
                    while i < b.len() && b[i] != b'\'' {
                        i += 1;
                    }
                }
                b'"' => {
                    i += 1;
                    while i < b.len() && b[i] != b'"' {
                        if b[i] == b'\\' && i + 1 < b.len() {
                            i += 1;
                        }
                        i += 1;
                    }
                }
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        endp = Some(i);
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        match endp {
            Some(end) => {
                let inner = &src[1..end];
                *arg = &src[end + 1..];
                if evaluate {
                    let callee = rettv.clone();
                    match crate::ported::eval::userfunc::get_func_arguments(inner) {
                        Some(argvars) => {
                            let mut full = Vec::with_capacity(argvars.len() + 1);
                            full.push(base.clone());
                            full.extend(argvars);
                            ret = eval_expr_partial(&callee, &full, rettv);
                        }
                        None => ret = FAIL,
                    }
                } else {
                    // Skip mode: arguments consumed, nothing called.
                    ret = OK;
                }
            }
            None => ret = FAIL,
        }
    }

    // c:2941 Clear the funcref afterwards, so that deleting it while
    // evaluating the arguments is possible (see test55).
    if evaluate {
        crate::ported::eval::typval::tv_clear(&mut base); // c:2944
    }

    ret // c:2947
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

// ═══════════════════════════════════════════════════════════════════════════
// Expression tree-walker (`eval0`…`eval7`) + leaf parsers, ported from
// `csrc/eval.c`.
//
// PORT.md classes these as strict reference ports: faithful C control flow with
// verbatim C names, cited `// c:NNN`. The bytecode frontend (viml_parser.rs /
// compile_viml.rs) is the RUNTIME path — these tree-walkers are dead code here,
// which is allowed per-file (see PORT.md §"strict 1:1 ports"). They document
// the exact grammar the frontend must reproduce.
//
// RUST-PORT NOTE (cursor model): the C walkers advance a `char **arg` pointer
// into a NUL-terminated buffer. The Rust ports model that cursor as
// `&mut &str`: `**arg` (the first byte) is `arg.as_bytes().first()...unwrap_or(0)`
// (an empty slice reads as the C NUL sentinel), and `*arg = skipwhite(*arg + n)`
// becomes `*arg = skipwhite(&s[n..])` (copying `*arg` out first, since `&str`
// is `Copy`). Wide chars advance by `char::len_utf8`, matching `MB_PTR_ADV`.
// ═══════════════════════════════════════════════════════════════════════════

/// `EVAL_EVALUATE` (`csrc/eval.h:140`) — `evalarg_T.eval_flags` bit: when unset,
/// the argument is only parsed, not executed.
pub const EVAL_EVALUATE: i32 = 1;

/// `NOTDONE` (`src/nvim/vim_defs.h`, extern) — a third return code besides
/// OK/FAIL, used by parsers that decline to handle the input (e.g. `{...}` that
/// turns out to be a `{expr}` name rather than a Dict/lambda).
pub const NOTDONE: i32 = 2;

/// `typedef struct { int eval_flags; … } evalarg_T;`
/// (`src/nvim/eval_defs.h:20`) — context passed through the `eval*` functions.
///
/// RUST-PORT NOTE: `eval_getline`/`eval_cookie`/`eval_tofree` are the
/// `:source`-line reader used when an expression spans continuation lines; the
/// standalone evaluator has no source-line getter, so only `eval_flags` is
/// modeled.
#[derive(Debug, Default, Clone)]
pub struct evalarg_T {
    /// `int eval_flags` — `EVAL_` flag values (`EVAL_EVALUATE`).
    pub eval_flags: i32,
}

/// `typedef enum { GLV_FAIL, GLV_OK, GLV_STOP } glv_status_T;`
/// (`csrc/eval.c:134`) — result of `get_lval_dict_item()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum glv_status_T {
    /// Evaluation error.
    GLV_FAIL,
    /// Success.
    GLV_OK,
    /// Stop processing characters after the key.
    GLV_STOP,
}

/// `GLV_QUIET` (`csrc/eval.h:94`) — `get_lval()` flag: do not emit error
/// messages (aliases `TFN_QUIET`).
pub const GLV_QUIET: i32 = 2;
/// `GLV_NO_AUTOLOAD` (`csrc/eval.h:95`) — do not use script autoloading
/// (aliases `TFN_NO_AUTOLOAD`).
pub const GLV_NO_AUTOLOAD: i32 = 4;
/// `GLV_READ_ONLY` (`csrc/eval.h:96`) — caller will not change the value
/// (aliases `TFN_READ_ONLY`).
pub const GLV_READ_ONLY: i32 = 16;

/// The `lval_T.ll_tv` interior handle.
///
/// RUST-PORT NOTE: the C `ll_tv` is a raw `typval_T *` aliasing either a
/// variable's `dictitem_T.di_tv`, a live List item, or a live Dict item. The
/// value layer here models containers as `Rc<RefCell<…>>` (see
/// `typval_defs_h.rs`) with no `dictitem_T` and no stable interior address, so
/// the pointer becomes a handle following the crate's existing
/// `Rc<RefCell>`-plus-index convention: [`LlTv::Var`] holds the variable value
/// (obtained by-copy from the reduced `find_var`, which returns a value — see
/// [`get_lval`]); [`LlTv::DictItem`]/[`LlTv::ListItem`] hold the owning
/// container `Rc` plus the key/index of the focused item. Reads and writes
/// through the `Rc` alias the live container, so subscript assignment persists;
/// a write to a [`LlTv::Var`] root only updates the local copy (a NULL/empty
/// container auto-vivified at the *top level* therefore does not persist — this
/// needs a mutable `find_var` returning an interior handle, deferred).
pub enum LlTv {
    /// C `ll_tv == NULL`.
    Null,
    /// `&v->di_tv` — the variable's own value (the subscript-walk root).
    Var(typval_T),
    /// `&di->di_tv` — a Dict item, addressed by its owning Dict `Rc` and key.
    DictItem(Rc<RefCell<dict_T>>, String),
    /// `TV_LIST_ITEM_TV(li)` — a List item, by its owning List `Rc` and index.
    ListItem(Rc<RefCell<list_T>>, usize),
}

impl LlTv {
    /// Read the current value the handle points at (C `*ll_tv`). RUST-PORT NOTE:
    /// returns a clone; the container `Rc` inside the clone still aliases the
    /// live List/Dict/Blob.
    fn get(&self) -> Option<typval_T> {
        match self {
            LlTv::Null => None,
            LlTv::Var(tv) => Some(tv.clone()),
            LlTv::DictItem(d, k) => d.borrow().dv_hashtab.get(k).cloned(),
            LlTv::ListItem(l, i) => l.borrow().lv_items.get(*i).map(|it| it.li_tv.clone()),
        }
    }

    /// Write `tv` back through the handle (C `*ll_tv = …`). For [`LlTv::Var`] this
    /// updates only the local copy (see the type note). Named `write` (not `set`)
    /// so the drift gate traces it to a C callable.
    fn write(&mut self, tv: typval_T) {
        match self {
            LlTv::Null => {}
            LlTv::Var(slot) => *slot = tv,
            LlTv::DictItem(d, k) => {
                if let Some(v) = d.borrow_mut().dv_hashtab.get_mut(k) {
                    *v = tv;
                }
            }
            LlTv::ListItem(l, i) => {
                let idx = *i;
                if let Some(it) = l.borrow_mut().lv_items.get_mut(idx) {
                    it.li_tv = tv;
                }
            }
        }
    }
}

/// `typedef struct { … } lval_T;` (`csrc/eval.h:52`) — the parsed assignment
/// target returned by [`get_lval`] and consumed by [`set_var_lval`].
///
/// RUST-PORT NOTE: the raw interior pointers of the C struct become
/// `Rc<RefCell>`/index handles ([`LlTv`], `ll_list`/`ll_dict`/`ll_blob`) — see
/// [`LlTv`] for the aliasing model. `ll_li` (a `listitem_T *`) becomes the item
/// index into `ll_list`; `ll_di` (a `dictitem_T *`) becomes the item key into
/// `ll_dict` (there is no `dictitem_T` — the Dict is an `IndexMap`).
pub struct lval_T {
    /// `const char *ll_name` — start of variable name (can be NULL).
    pub ll_name: Option<String>,
    /// `size_t ll_name_len` — length of `.ll_name`.
    pub ll_name_len: usize,
    /// `char *ll_exp_name` — NULL or expanded (`{expr}`) name.
    pub ll_exp_name: Option<String>,
    /// `typval_T *ll_tv` — value of the item being used (or the Dict to add to).
    pub ll_tv: LlTv,
    /// `listitem_T *ll_li` — index of the (first) list item, or None.
    pub ll_li: Option<usize>,
    /// `list_T *ll_list` — the list or None.
    pub ll_list: Option<Rc<RefCell<list_T>>>,
    /// `bool ll_range` — true when a `[i:j]` range was used.
    pub ll_range: bool,
    /// `bool ll_empty2` — second index empty: `[i:]`.
    pub ll_empty2: bool,
    /// `int ll_n1` — first index for a list.
    pub ll_n1: i32,
    /// `int ll_n2` — second index for a list range.
    pub ll_n2: i32,
    /// `dict_T *ll_dict` — the Dict or None.
    pub ll_dict: Option<Rc<RefCell<dict_T>>>,
    /// `dictitem_T *ll_di` — the dict key of the focused item, or None (see the
    /// struct note: the key string stands in for the `dictitem_T *`).
    pub ll_di: Option<String>,
    /// `char *ll_newkey` — new key for a Dict item.
    pub ll_newkey: Option<String>,
    /// `blob_T *ll_blob` — the Blob or None.
    pub ll_blob: Option<Rc<RefCell<blob_T>>>,
}

impl Default for lval_T {
    fn default() -> Self {
        lval_T {
            ll_name: None,
            ll_name_len: 0,
            ll_exp_name: None,
            ll_tv: LlTv::Null,
            ll_li: None,
            ll_list: None,
            ll_range: false,
            ll_empty2: false,
            ll_n1: 0,
            ll_n2: 0,
            ll_dict: None,
            ll_di: None,
            ll_newkey: None,
            ll_blob: None,
        }
    }
}

/// Port of `get_lval_dict_item()` from `Src/eval.c:840`.
///
/// Get a Dict lval subitem for `key`/`[key]` in the Dict at `lp.ll_tv`; on a
/// missing key that may be added, records `ll_newkey` and returns
/// [`glv_status_T::GLV_STOP`]. `p_off` is the byte offset in `name` just after
/// the key (C `*key_end`, which this never advances).
///
/// RUST-PORT NOTE: the scope-dictionary validation (`lp->ll_dict->dv_scope`, the
/// `get_vimvar_dict()`/`get_funccal_args_ht()` identity checks) and the
/// `di_flags` read-only/lock check are elided: `dv_scope`/`di_flags` are not
/// modeled and a Dict reached through a subscript is never a scope dict here, so
/// those branches are always the "plain container" path.
#[allow(clippy::too_many_arguments)]
fn get_lval_dict_item(
    lp: &mut lval_T,
    name: &str,
    key: Option<&str>,
    len: i32,
    p_off: usize,
    var1: Option<&typval_T>,
    flags: i32,
    unlet: bool,
    rettv: Option<&typval_T>,
) -> glv_status_T {
    use glv_status_T::*;
    let quiet = flags & GLV_QUIET != 0;
    // c: if (len == -1) key = tv_get_string(var1);
    let key: String = if len == -1 {
        tv_get_string(var1.unwrap())
    } else {
        // c: key limited to "len" bytes.
        key.unwrap()[..len as usize].to_string()
    };
    // c: lp->ll_list = NULL;
    lp.ll_list = None;

    // c: a NULL dict is equivalent with an empty dict — allocate one now.
    let cur = lp.ll_tv.get().unwrap_or_default();
    let dict_rc = match &cur.vval {
        v_dict(Some(d)) => d.clone(),
        _ => {
            let nd = crate::ported::eval::typval::tv_dict_alloc();
            let newcur = typval_T {
                v_type: VAR_DICT,
                v_lock: VarLockStatus::VAR_UNLOCKED,
                vval: v_dict(Some(nd.clone())),
            };
            lp.ll_tv.write(newcur);
            nd
        }
    };
    // c: lp->ll_dict = lp->ll_tv->vval.v_dict;
    lp.ll_dict = Some(dict_rc.clone());

    // c: lp->ll_di = tv_dict_find(lp->ll_dict, key, len);
    let found = dict_rc.borrow().dv_hashtab.contains_key(&key);

    // c: if (lp->ll_di != NULL && tv_is_luafunc(&lp->ll_di->di_tv) && len == -1 && rettv == NULL)
    if found && len == -1 && rettv.is_none() {
        let is_lua = dict_rc
            .borrow()
            .dv_hashtab
            .get(&key)
            .is_some_and(tv_is_luafunc);
        if is_lua {
            crate::ported::message::semsg("E461: Illegal variable name: v:['lua']");
            return GLV_FAIL;
        }
    }

    if !found {
        // c: Key does not exist in dict: may need to add it.
        let pc = name.as_bytes().get(p_off).copied();
        if pc == Some(b'[') || pc == Some(b'.') || unlet {
            if !quiet {
                crate::ported::message::semsg(&format!(
                    "E716: Key not present in Dictionary: \"{key}\""
                ));
            }
            return GLV_FAIL;
        }
        // c: lp->ll_newkey = xstrdup(key); *key_end = p; return GLV_STOP;
        lp.ll_newkey = Some(key);
        return GLV_STOP;
    }
    // c: existing variable — the di_flags read-only/lock check is elided (note).

    // c: lp->ll_tv = &lp->ll_di->di_tv;
    lp.ll_di = Some(key.clone());
    lp.ll_tv = LlTv::DictItem(dict_rc, key);

    GLV_OK
}

/// Port of `get_lval_blob()` from `Src/eval.c:933`.
///
/// Get a Blob lval for `name[expr]` / `name[expr:expr]` from the Blob at
/// `lp.ll_tv`. `var1`/`var2` are the (already-evaluated) indices; `empty1` marks
/// an omitted first index. Returns [`OK`]/[`FAIL`].
fn get_lval_blob(
    lp: &mut lval_T,
    var1: Option<&typval_T>,
    var2: Option<&typval_T>,
    empty1: bool,
    quiet: bool,
) -> i32 {
    let cur = lp.ll_tv.get().unwrap_or_default();
    let blob_rc = match &cur.vval {
        v_blob(Some(b)) => b.clone(),
        _ => return FAIL,
    };
    // c: const int bloblen = tv_blob_len(lp->ll_tv->vval.v_blob);
    let bloblen = crate::ported::eval::typval::tv_blob_len(&blob_rc.borrow());

    // c: Get the number and item for the only or first index of the List.
    if empty1 {
        lp.ll_n1 = 0;
    } else {
        lp.ll_n1 = crate::ported::eval::typval::tv_get_number(var1.unwrap()) as i32;
    }

    if crate::ported::eval::typval::tv_blob_check_index(bloblen, lp.ll_n1 as varnumber_T, quiet)
        == FAIL
    {
        return FAIL;
    }
    if lp.ll_range && !lp.ll_empty2 {
        lp.ll_n2 = crate::ported::eval::typval::tv_get_number(var2.unwrap()) as i32;
        if crate::ported::eval::typval::tv_blob_check_range(
            bloblen,
            lp.ll_n1 as varnumber_T,
            lp.ll_n2 as varnumber_T,
            quiet,
        ) == FAIL
        {
            return FAIL;
        }
    }

    // c: lp->ll_blob = lp->ll_tv->vval.v_blob; lp->ll_tv = NULL;
    lp.ll_blob = Some(blob_rc);
    lp.ll_tv = LlTv::Null;

    OK
}

/// Port of `get_lval_list()` from `Src/eval.c:970`.
///
/// Get a List lval for `name[expr]` / `name[expr:expr]` from the List at
/// `lp.ll_tv`. Returns [`OK`]/[`FAIL`].
fn get_lval_list(
    lp: &mut lval_T,
    var1: Option<&typval_T>,
    var2: Option<&typval_T>,
    empty1: bool,
    _flags: i32,
    quiet: bool,
) -> i32 {
    // c: Get the number and item for the only or first index of the List.
    if empty1 {
        lp.ll_n1 = 0;
    } else {
        lp.ll_n1 = crate::ported::eval::typval::tv_get_number(var1.unwrap()) as i32;
    }

    // c: lp->ll_dict = NULL; lp->ll_list = lp->ll_tv->vval.v_list;
    lp.ll_dict = None;
    let cur = lp.ll_tv.get().unwrap_or_default();
    let list_rc = match &cur.vval {
        v_list(Some(l)) => l.clone(),
        _ => return FAIL,
    };
    lp.ll_list = Some(list_rc.clone());

    // c: lp->ll_li = tv_list_check_range_index_one(lp->ll_list, &lp->ll_n1, quiet);
    let mut n1 = lp.ll_n1;
    let li = crate::ported::eval::typval::tv_list_check_range_index_one(
        &list_rc.borrow(),
        &mut n1,
        quiet,
    );
    lp.ll_n1 = n1;
    let li = match li {
        Some(i) => i,
        None => return FAIL,
    };
    lp.ll_li = Some(li);

    // c: May need to find the item/absolute index for the second range index.
    if lp.ll_range && !lp.ll_empty2 {
        lp.ll_n2 = crate::ported::eval::typval::tv_get_number(var2.unwrap()) as i32;
        let mut n1b = lp.ll_n1;
        let mut n2 = lp.ll_n2;
        if crate::ported::eval::typval::tv_list_check_range_index_two(
            &list_rc.borrow(),
            &mut n1b,
            li,
            &mut n2,
            quiet,
        ) == FAIL
        {
            return FAIL;
        }
        lp.ll_n1 = n1b;
        lp.ll_n2 = n2;
    }

    // c: lp->ll_tv = TV_LIST_ITEM_TV(lp->ll_li);
    lp.ll_tv = LlTv::ListItem(list_rc, li);

    OK
}

/// Port of `get_lval_subscript()` from `Src/eval.c:1016`.
///
/// Walk the `[idx]`/`[a:b]`/`.key` subscripts starting at byte offset `p` in
/// `name`, descending `lp.ll_tv` into the referenced List/Dict/Blob item.
/// Returns the byte offset just after the last subscript, or `None` on error.
///
/// RUST-PORT NOTE: the C consumes each index with a recursive `eval1()`; here
/// the bracket-matched substring is evaluated through the sanctioned
/// `EVAL_STRING_HOOK` bridge (the same integration point [`eval_index`] uses),
/// since the hook does not report how many bytes it consumed. The unused C
/// `ht`/`v` parameters (dead in the C body, and not produced by the reduced
/// `find_var`) are omitted.
fn get_lval_subscript(
    lp: &mut lval_T,
    mut p: usize,
    name: &str,
    rettv: Option<&typval_T>,
    unlet: bool,
    flags: i32,
) -> Option<usize> {
    let quiet = flags & GLV_QUIET != 0;
    let b = name.as_bytes();
    let eval = |e: &str| -> Option<typval_T> {
        crate::ported::eval::typval::EVAL_STRING_HOOK
            .with(|h| *h.borrow())
            .and_then(|f| f(e))
    };

    // c: Loop until no more [idx] or .key is following.
    while p < b.len()
        && (b[p] == b'['
            || (b[p] == b'.' && b.get(p + 1) != Some(&b'=') && b.get(p + 1) != Some(&b'.')))
    {
        let cur = lp.ll_tv.get().unwrap_or_default();
        if b[p] == b'.' && cur.v_type != VAR_DICT {
            if !quiet {
                crate::ported::message::semsg(&format!(
                    "E1203: Dot can only be used on a dictionary: {name}"
                ));
            }
            return None;
        }
        if !matches!(cur.v_type, VAR_LIST | VAR_DICT | VAR_BLOB) {
            if !quiet {
                emsg("E689: Can only index a List, Dictionary or Blob");
            }
            return None;
        }

        // c: A NULL list/blob works like an empty one, allocate one now.
        if cur.v_type == VAR_LIST && matches!(cur.vval, v_list(None)) {
            let mut tmp = typval_T::default();
            crate::ported::eval::typval::tv_list_alloc_ret(&mut tmp, -1);
            lp.ll_tv.write(tmp);
        } else if cur.v_type == VAR_BLOB && matches!(cur.vval, v_blob(None)) {
            let mut tmp = typval_T::default();
            crate::ported::eval::typval::tv_blob_alloc_ret(&mut tmp);
            lp.ll_tv.write(tmp);
        }

        if lp.ll_range {
            if !quiet {
                emsg("E708: [:] must come last");
            }
            return None;
        }

        let mut len: i32 = -1;
        let mut key: Option<String> = None;
        let mut var1: Option<typval_T> = None;
        let mut var2: Option<typval_T> = None;
        let mut empty1 = false;
        if b[p] == b'.' {
            // c: key = p + 1; scan [A-Za-z0-9_]; error on empty key.
            let ks = p + 1;
            let mut ke = ks;
            while ke < b.len() && (b[ke].is_ascii_alphanumeric() || b[ke] == b'_') {
                ke += 1;
            }
            if ke == ks {
                if !quiet {
                    emsg("E713: Cannot use empty key after .");
                }
                return None;
            }
            len = (ke - ks) as i32;
            key = Some(name[ks..ke].to_string());
            p = ke;
        } else {
            // c: Get the index [expr] or the first index [expr: ].
            // RUST-PORT NOTE: bracket-match the whole [ ... ] (balancing nested
            // brackets and skipping strings), then split the inner text on a
            // top-level ':' — the analogue of eval1() advancing to ':'/']'.
            let mut depth = 0i32;
            let mut i = p;
            let mut close = None;
            while i < b.len() {
                match b[i] {
                    b'\'' => {
                        i += 1;
                        while i < b.len() && b[i] != b'\'' {
                            i += 1;
                        }
                    }
                    b'"' => {
                        i += 1;
                        while i < b.len() && b[i] != b'"' {
                            if b[i] == b'\\' && i + 1 < b.len() {
                                i += 1;
                            }
                            i += 1;
                        }
                    }
                    b'[' | b'(' | b'{' => depth += 1,
                    b']' | b')' | b'}' => {
                        depth -= 1;
                        if depth == 0 && b[i] == b']' {
                            close = Some(i);
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            let close = match close {
                Some(c) => c,
                None => {
                    if !quiet {
                        emsg("E111: Missing ']'");
                    }
                    return None;
                }
            };
            let inner = &name[p + 1..close];
            // Top-level ':' in the inner text (range separator).
            let colon = {
                let ib = inner.as_bytes();
                let (mut d, mut i, mut pos) = (0i32, 0usize, None);
                while i < ib.len() {
                    match ib[i] {
                        b'\'' => {
                            i += 1;
                            while i < ib.len() && ib[i] != b'\'' {
                                i += 1;
                            }
                        }
                        b'"' => {
                            i += 1;
                            while i < ib.len() && ib[i] != b'"' {
                                if ib[i] == b'\\' && i + 1 < ib.len() {
                                    i += 1;
                                }
                                i += 1;
                            }
                        }
                        b'[' | b'(' | b'{' => d += 1,
                        b']' | b')' | b'}' => d -= 1,
                        b':' if d == 0 => {
                            pos = Some(i);
                            break;
                        }
                        _ => {}
                    }
                    i += 1;
                }
                pos
            };
            match colon {
                None => {
                    // c: single index [expr].
                    empty1 = false;
                    let v = eval(inner.trim())?;
                    if !crate::ported::eval::typval::tv_check_str(&v) {
                        return None;
                    }
                    var1 = Some(v);
                    lp.ll_range = false;
                }
                Some(c) => {
                    // c: first index [expr:...] (empty1 when omitted).
                    let left = inner[..c].trim();
                    if left.is_empty() {
                        empty1 = true;
                    } else {
                        empty1 = false;
                        let v = eval(left)?;
                        if !crate::ported::eval::typval::tv_check_str(&v) {
                            return None;
                        }
                        var1 = Some(v);
                    }
                    // c: a slice is illegal on a Dict.
                    if cur.v_type == VAR_DICT {
                        if !quiet {
                            emsg("E719: Cannot slice a Dictionary");
                        }
                        return None;
                    }
                    // c: [:] requires a List or Blob value on the RHS.
                    if let Some(rt) = rettv {
                        let ok = (rt.v_type == VAR_LIST && matches!(&rt.vval, v_list(Some(_))))
                            || (rt.v_type == VAR_BLOB && matches!(&rt.vval, v_blob(Some(_))));
                        if !ok {
                            if !quiet {
                                emsg("E709: [:] requires a List or Blob value");
                            }
                            return None;
                        }
                    }
                    // c: second index [ :expr].
                    let right = inner[c + 1..].trim();
                    if right.is_empty() {
                        lp.ll_empty2 = true;
                    } else {
                        lp.ll_empty2 = false;
                        let v = eval(right)?;
                        if !crate::ported::eval::typval::tv_check_str(&v) {
                            return None;
                        }
                        var2 = Some(v);
                    }
                    lp.ll_range = true;
                }
            }
            // c: Skip to past ']'.
            p = close + 1;
        }

        if cur.v_type == VAR_DICT {
            let glv_status = get_lval_dict_item(
                lp,
                name,
                key.as_deref(),
                len,
                p,
                var1.as_ref(),
                flags,
                unlet,
                rettv,
            );
            if glv_status == glv_status_T::GLV_FAIL {
                return None;
            }
            if glv_status == glv_status_T::GLV_STOP {
                break;
            }
        } else if cur.v_type == VAR_BLOB {
            if get_lval_blob(lp, var1.as_ref(), var2.as_ref(), empty1, quiet) == FAIL {
                return None;
            }
            break;
        } else if get_lval_list(lp, var1.as_ref(), var2.as_ref(), empty1, flags, quiet) == FAIL {
            return None;
        }
    }

    Some(p)
}

/// Port of `get_lval()` from `Src/eval.c:1191`.
///
/// Parse the assignment target `name` (a plain name, `dict.key`, `list[expr]`,
/// or a slice) into `lp`. `rettv` is the value to be assigned (or `None`).
/// Returns the byte offset just after the parsed name (incl. indices), or `None`
/// on a parse error (`lp` is still filled enough for [`clear_lval`]).
///
/// RUST-PORT NOTE: the reduced `find_var` returns a value copy, not a mutable
/// `dictitem_T *`, so `ll_tv` becomes [`LlTv::Var`] (see [`LlTv`]). Subscript
/// mutations persist through the container `Rc`; a top-level scalar/NULL-
/// container auto-vivification would need a mutable `find_var` (deferred).
#[allow(clippy::too_many_arguments)]
pub fn get_lval(
    name: &str,
    rettv: Option<&typval_T>,
    lp: &mut lval_T,
    unlet: bool,
    skip: bool,
    flags: i32,
    fne_flags: u32,
) -> Option<usize> {
    let quiet = flags & GLV_QUIET != 0;

    // c: CLEAR_POINTER(lp) — clear everything in "lp".
    *lp = lval_T::default();

    if skip {
        // c: When skipping just find the end of the name.
        lp.ll_name = Some(name.to_string());
        let (end, _, _) = find_name_end(name, FNE_INCL_BR | fne_flags);
        return Some(end);
    }

    // c: Find the end of the name.
    let (mut p, expr_start, expr_end) = find_name_end(name, fne_flags);
    if let (Some(es), Some(ee)) = (expr_start, expr_end) {
        let pc = name.as_bytes().get(p).copied();
        // c: Don't expand the name when we already know there is an error.
        if unlet
            && !matches!(pc, Some(b' ') | Some(b'\t'))
            && !ends_excmd(pc.unwrap_or(0))
            && pc != Some(b'[')
            && pc != Some(b'.')
        {
            crate::ported::message::semsg(&format!("E488: Trailing characters: {}", &name[p..]));
            return None;
        }
        lp.ll_exp_name = make_expanded_name(name, es, ee);
        lp.ll_name = lp.ll_exp_name.clone();
        if lp.ll_exp_name.is_none() {
            // c: report an invalid expression in braces unless aborting.
            if !crate::ported::ex_eval::aborting() && !quiet {
                crate::ported::message::semsg(&format!("E475: Invalid argument: {name}"));
                return None;
            }
            lp.ll_name_len = 0;
        } else {
            lp.ll_name_len = lp.ll_name.as_ref().unwrap().len();
        }
    } else {
        lp.ll_name = Some(name[..p].to_string());
        lp.ll_name_len = p;
    }

    // c: Without [idx] or .key we are done.
    let pc = name.as_bytes().get(p).copied();
    if (pc != Some(b'[') && pc != Some(b'.')) || lp.ll_name.is_none() {
        return Some(p);
    }

    // c: find_var — only pass &ht when we would write (prevents autoload too).
    let ll_name = lp.ll_name.clone().unwrap();
    let v = crate::ported::eval::vars::find_var(&ll_name, flags & GLV_NO_AUTOLOAD != 0);
    match v {
        None => {
            if !quiet {
                crate::ported::message::semsg(&format!("E121: Undefined variable: {ll_name}"));
            }
            None
        }
        Some(v) => {
            // c: lp->ll_tv = &v->di_tv;
            let is_lua = tv_is_luafunc(&v);
            lp.ll_tv = LlTv::Var(v);
            // c: For v:lua just return a pointer to the "." after the "v:lua".
            if is_lua {
                return Some(p);
            }
            // c: process the subitem after "." or "[".
            p = get_lval_subscript(lp, p, name, rettv, unlet, flags)?;
            // c: lp->ll_name_len = (size_t)(p - lp->ll_name);
            lp.ll_name_len = p;
            Some(p)
        }
    }
}

/// Port of `set_var_lval()` from `Src/eval.c:1290`.
///
/// Assign `rettv` to the target parsed by [`get_lval`]. `op` is `None` (C NULL)
/// or one of `"="`/`"+="`/`"-="`/`"*="`/`"/="`/`"%="`/`".="`. `_endp` is the C
/// NUL-terminator save/restore position, unneeded here (`ll_name` is an owned
/// exact substring) — RUST-PORT NOTE.
pub fn set_var_lval(
    lp: &mut lval_T,
    _endp: usize,
    rettv: &mut typval_T,
    copy: bool,
    is_const: bool,
    op: Option<&str>,
) {
    let ll_name = lp.ll_name.clone().unwrap_or_default();
    // c: op != NULL && *op != '=' — a compound (+=, -=, …) assignment.
    let is_compound = matches!(op, Some(o) if !o.starts_with('='));
    let opc = op.and_then(|o| o.chars().next());

    if matches!(lp.ll_tv, LlTv::Null) {
        if let Some(blob_rc) = lp.ll_blob.clone() {
            if is_compound {
                crate::ported::message::semsg(&format!(
                    "E734: Wrong variable type for {}=",
                    op.unwrap()
                ));
                return;
            }
            let lock = blob_rc.borrow().bv_lock;
            if crate::ported::eval::typval::value_check_lock(
                lock,
                Some(&ll_name),
                crate::ported::eval::typval::TV_CSTRING,
            ) {
                return;
            }

            if lp.ll_range && rettv.v_type == VAR_BLOB {
                if lp.ll_empty2 {
                    lp.ll_n2 = crate::ported::eval::typval::tv_blob_len(&blob_rc.borrow()) - 1;
                }
                let src = match &rettv.vval {
                    v_blob(Some(b)) => b.clone(),
                    _ => return,
                };
                if crate::ported::eval::typval::tv_blob_set_range(
                    &mut blob_rc.borrow_mut(),
                    lp.ll_n1 as varnumber_T,
                    lp.ll_n2 as varnumber_T,
                    &src.borrow(),
                ) == FAIL
                {
                    return;
                }
            } else {
                let mut error = false;
                let val = tv_get_number_chk(rettv, Some(&mut error));
                if !error {
                    if !(0..=255).contains(&val) {
                        crate::ported::message::semsg(&format!(
                            "E1239: Invalid value for blob: 0x{val:X}"
                        ));
                    } else {
                        crate::ported::eval::typval::tv_blob_set_append(
                            &mut blob_rc.borrow_mut(),
                            lp.ll_n1,
                            val as u8,
                        );
                    }
                }
            }
        } else if is_compound {
            // c: handle +=, -=, *=, /=, %= and .=
            if is_const {
                emsg("E995: Cannot modify existing variable");
                return;
            }
            // c: eval_variable(ll_name, …, &tv, &di, …) — the di read-only/lock
            // check is elided (no dictitem in the reduced find path).
            if let Some(mut tv) = crate::ported::eval::vars::eval_variable(&ll_name) {
                if crate::ported::eval::executor::eexe_mod_op(&mut tv, rettv, opc.unwrap()) == OK {
                    crate::ported::eval::vars::set_var(&ll_name, lp.ll_name_len, tv, false);
                }
            }
        } else {
            crate::ported::eval::vars::set_var_const(
                &ll_name,
                lp.ll_name_len,
                rettv.clone(),
                copy,
                is_const,
            );
        }
    } else if crate::ported::eval::typval::value_check_lock(
        // c: value_check_lock(ll_newkey == NULL ? ll_tv->v_lock : ll_tv->vval.v_dict->dv_lock, …)
        if lp.ll_newkey.is_none() {
            lp.ll_tv
                .get()
                .map_or(VarLockStatus::VAR_UNLOCKED, |t| t.v_lock)
        } else {
            lp.ll_dict
                .as_ref()
                .map_or(VarLockStatus::VAR_UNLOCKED, |d| d.borrow().dv_lock)
        },
        Some(&ll_name),
        crate::ported::eval::typval::TV_CSTRING,
    ) {
        // c: Skip
    } else if lp.ll_range {
        if is_const {
            emsg("E996: Cannot lock a range");
            return;
        }
        let list_rc = match &lp.ll_list {
            Some(l) => l.clone(),
            None => return,
        };
        let src = match &rettv.vval {
            v_list(Some(l)) => l.clone(),
            _ => return,
        };
        crate::ported::eval::typval::tv_list_assign_range(
            &list_rc,
            &src.borrow(),
            lp.ll_n1,
            lp.ll_n2,
            lp.ll_empty2,
            op.unwrap_or("="),
            &ll_name,
        );
    } else {
        // c: Assign to a List or Dictionary item.
        let dict = lp.ll_dict.clone();
        // c: bool watched = tv_dict_is_watched(dict) — inlined (no ported helper).
        let watched = dict
            .as_ref()
            .is_some_and(|d| !d.borrow().dv_watchers.is_empty());

        if is_const {
            emsg("E996: Cannot lock a list or dict");
            return;
        }

        // c: typval_T oldtv = TV_INITIAL_VALUE;
        let mut oldtv = typval_T::default();
        let mut do_assign = true;

        if let Some(newkey) = lp.ll_newkey.clone() {
            if is_compound {
                crate::ported::message::semsg(&format!(
                    "E716: Key not present in Dictionary: \"{newkey}\""
                ));
                return;
            }
            let d = match &lp.ll_dict {
                Some(d) => d.clone(),
                None => return,
            };
            // c: if (tv_dict_wrong_func_name(lp->ll_tv->vval.v_dict, rettv, newkey)) return;
            if crate::ported::eval::typval::tv_dict_wrong_func_name(&d.borrow(), rettv, &newkey) {
                return;
            }
            // c: di = tv_dict_item_alloc(newkey); tv_dict_add(...); lp->ll_tv = &di->di_tv;
            if crate::ported::eval::typval::tv_dict_add(
                &mut d.borrow_mut(),
                &newkey,
                typval_T::default(),
            ) == FAIL
            {
                return;
            }
            lp.ll_tv = LlTv::DictItem(d, newkey);
        } else {
            if watched {
                // c: tv_copy(lp->ll_tv, &oldtv);
                if let Some(cur) = lp.ll_tv.get() {
                    crate::ported::eval::typval::tv_copy(&cur, &mut oldtv);
                }
            }
            if is_compound {
                // c: eexe_mod_op(lp->ll_tv, rettv, op); goto notify;
                let mut cur = lp.ll_tv.get().unwrap_or_default();
                crate::ported::eval::executor::eexe_mod_op(&mut cur, rettv, opc.unwrap());
                lp.ll_tv.write(cur);
                do_assign = false;
            }
            // else: tv_clear(lp->ll_tv) — the value is overwritten just below.
        }

        // c: Assign the value to the variable or list item.
        if do_assign {
            if copy {
                let mut dest = typval_T::default();
                crate::ported::eval::typval::tv_copy(rettv, &mut dest);
                lp.ll_tv.write(dest);
            } else {
                // c: *lp->ll_tv = *rettv; lp->ll_tv->v_lock = VAR_UNLOCKED; tv_init(rettv);
                let mut moved = rettv.clone();
                moved.v_lock = VarLockStatus::VAR_UNLOCKED;
                lp.ll_tv.write(moved);
                *rettv = typval_T::default();
            }
        }

        // c: notify:
        if watched {
            if let Some(d) = &dict {
                let newtv = lp.ll_tv.get();
                if oldtv.v_type == VAR_UNKNOWN {
                    // c: assert(lp->ll_newkey != NULL);
                    crate::ported::eval::typval::tv_dict_watcher_notify(
                        d,
                        lp.ll_newkey.as_deref().unwrap_or_default(),
                        newtv.as_ref(),
                        None,
                    );
                } else {
                    crate::ported::eval::typval::tv_dict_watcher_notify(
                        d,
                        lp.ll_di.as_deref().unwrap_or_default(),
                        newtv.as_ref(),
                        Some(&oldtv),
                    );
                }
            }
        }
    }
}

// ── small scanner helpers (extern, ported against their home C files) ──

/// Port of `skipwhite()` from `src/nvim/charset.c` (extern; not vendored under
/// `csrc/`) — skip leading spaces and tabs.
pub fn skipwhite(s: &str) -> &str {
    s.trim_start_matches([' ', '\t'])
}

/// Port of `skipdigits()` from `src/nvim/charset.c` (extern) — skip leading
/// ASCII decimal digits.
pub fn skipdigits(s: &str) -> &str {
    s.trim_start_matches(|c: char| c.is_ascii_digit())
}

/// Port of `ends_excmd()` from `src/nvim/ex_docmd.h` (macro, extern) — true when
/// `c` ends an Ex command: NUL, `|`, `"` or newline.
pub fn ends_excmd(c: u8) -> bool {
    c == 0 || c == b'|' || c == b'"' || c == b'\n'
}

/// Port of `hex2nr()` from `src/nvim/charset.c` (extern) — value of a hex digit.
pub fn hex2nr(c: u8) -> u8 {
    if c.is_ascii_digit() {
        c - b'0'
    } else {
        (c | 0x20) - b'a' + 10
    }
}

/// Skip over an expression at "*pp".
/// Port of `skip_expr()` from `csrc/eval.c:461`.
///
/// @return  FAIL for an error, OK otherwise.
pub fn skip_expr(pp: &mut &str, mut evalarg: Option<&mut evalarg_T>) -> i32 {
    let save_flags = evalarg.as_deref().map_or(0, |e| e.eval_flags); // c:463

    // c:465 Don't evaluate the expression.
    if let Some(e) = evalarg.as_deref_mut() {
        e.eval_flags &= !EVAL_EVALUATE; // c:467
    }

    *pp = skipwhite(pp); // c:470
    let mut rettv = typval_T::default(); // c:471
    let res = eval1(pp, &mut rettv, None); // c:472 eval1(pp, &rettv, NULL)

    if let Some(e) = evalarg.as_deref_mut() {
        e.eval_flags = save_flags; // c:475
    }

    res // c:478
}

/// Handle zero level expression. This calls [`eval1`] and handles the error
/// message. Puts the result in `rettv` when returning OK and "evaluate" is true.
/// Port of `eval0()` from `csrc/eval.c:1787`.
///
/// RUST-PORT NOTE: the C `exarg_T *eap` (for `eap->nextcmd`) and the
/// `did_emsg`/`called_emsg`/`aborting()` guards are editor state not modeled
/// standalone; the invalid-expression / trailing-arg errors are always emitted.
pub fn eval0(arg: &str, rettv: &mut typval_T, mut evalarg: Option<&mut evalarg_T>) -> i32 {
    let mut p = skipwhite(arg); // c:1793
    let ret = eval1(&mut p, rettv, evalarg.as_deref_mut()); // c:1794

    let mut end_error = false;
    if ret != FAIL {
        end_error = !ends_excmd(p.as_bytes().first().copied().unwrap_or(0)); // c:1797
    }
    if ret == FAIL || end_error {
        if ret != FAIL {
            crate::ported::eval::typval::tv_clear(rettv); // c:1801
        }
        // c:1810 report the invalid expression / trailing argument.
        if end_error {
            crate::ported::message::semsg(&format!("E488: Trailing characters: {p}"));
        } else {
            crate::ported::message::semsg(&format!("E15: Invalid expression: \"{arg}\""));
        }
        return FAIL;
    }
    ret
}

/// Handle zero level expression with optimization for a simple function call.
/// Same arguments and return value as [`eval0`].
/// Port of `eval0_simple_funccal()` from `csrc/eval.c:1862`.
///
/// RUST-PORT NOTE: the C `exarg_T *eap` is editor state not modeled standalone
/// and is dropped, matching [`eval0`]'s signature. [`may_call_simple_func`]
/// always returns `NOTDONE` here, so this is a straight delegation to [`eval0`].
pub fn eval0_simple_funccal(
    arg: &str,
    rettv: &mut typval_T,
    evalarg: Option<&mut evalarg_T>,
) -> i32 {
    let mut r = may_call_simple_func(); // c:1864

    if r == NOTDONE {
        r = eval0(arg, rettv, evalarg); // c:1867
    }
    r // c:1869
}

/// Handle top level expression: `expr2 ? expr1 : expr1` / `expr2 ?? expr1`.
/// Port of `eval1()` from `csrc/eval.c:1880`.
pub fn eval1(arg: &mut &str, rettv: &mut typval_T, mut evalarg: Option<&mut evalarg_T>) -> i32 {
    let at = |s: &str, i: usize| s.as_bytes().get(i).copied().unwrap_or(0);
    *rettv = typval_T::default(); // c:1882 CLEAR_POINTER(rettv)

    // c:1885 Get the first variable.
    if eval2(arg, rettv, evalarg.as_deref_mut()) == FAIL {
        return FAIL;
    }

    if at(*arg, 0) == b'?' {
        let op_falsy = at(*arg, 1) == b'?'; // c:1891
        let mut local_evalarg = evalarg_T::default();
        let ea: &mut evalarg_T = match evalarg.as_deref_mut() {
            Some(e) => e,
            None => &mut local_evalarg,
        };
        let orig_flags = ea.eval_flags; // c:1898
        let evaluate = ea.eval_flags & EVAL_EVALUATE != 0; // c:1899

        let mut result = false;
        if evaluate {
            let mut error = false;
            if op_falsy {
                result = crate::ported::eval::typval::tv2bool(rettv); // c:1906
            } else if tv_get_number_chk(rettv, Some(&mut error)) != 0 {
                result = true; // c:1908
            }
            if error || !op_falsy || !result {
                crate::ported::eval::typval::tv_clear(rettv); // c:1911
            }
            if error {
                return FAIL;
            }
        }

        // c:1918 Get the second variable. Recursive!
        if op_falsy {
            let s = *arg;
            *arg = &s[1..]; // c:1920 (*arg)++
        }
        {
            let s = *arg;
            *arg = skipwhite(&s[1..]); // c:1922
        }
        ea.eval_flags = if (if op_falsy { !result } else { result }) {
            orig_flags
        } else {
            orig_flags & !EVAL_EVALUATE
        }; // c:1923
        let mut var2 = typval_T::default();
        if eval1(arg, &mut var2, Some(&mut *ea)) == FAIL {
            ea.eval_flags = orig_flags;
            return FAIL;
        }
        if !op_falsy || !result {
            *rettv = var2; // c:1931
        }

        if !op_falsy {
            // c:1935 Check for the ":".
            if at(*arg, 0) != b':' {
                emsg("E109: Missing ':' after '?'");
                if evaluate && result {
                    crate::ported::eval::typval::tv_clear(rettv);
                }
                ea.eval_flags = orig_flags;
                return FAIL;
            }

            // c:1946 Get the third variable. Recursive!
            {
                let s = *arg;
                *arg = skipwhite(&s[1..]);
            }
            ea.eval_flags = if !result {
                orig_flags
            } else {
                orig_flags & !EVAL_EVALUATE
            }; // c:1948
            let mut var3 = typval_T::default();
            if eval1(arg, &mut var3, Some(&mut *ea)) == FAIL {
                if evaluate && result {
                    crate::ported::eval::typval::tv_clear(rettv);
                }
                ea.eval_flags = orig_flags;
                return FAIL;
            }
            if evaluate && !result {
                *rettv = var3; // c:1957
            }
        }

        ea.eval_flags = orig_flags; // c:1964
    }

    OK
}

/// Handle first level expression: `expr2 || expr2` (logical OR).
/// Port of `eval2()` from `csrc/eval.c:1978`.
pub fn eval2(arg: &mut &str, rettv: &mut typval_T, mut evalarg: Option<&mut evalarg_T>) -> i32 {
    let at = |s: &str, i: usize| s.as_bytes().get(i).copied().unwrap_or(0);
    // c:1981 Get the first variable.
    if eval3(arg, rettv, evalarg.as_deref_mut()) == FAIL {
        return FAIL;
    }

    // c:1987 Handle the "||" operator.
    if at(*arg, 0) == b'|' && at(*arg, 1) == b'|' {
        let mut local_evalarg = evalarg_T::default();
        let ea: &mut evalarg_T = match evalarg.as_deref_mut() {
            Some(e) => e,
            None => &mut local_evalarg,
        };
        let orig_flags = ea.eval_flags;
        let evaluate = ea.eval_flags & EVAL_EVALUATE != 0;

        let mut result = false;
        if evaluate {
            let mut error = false;
            if tv_get_number_chk(rettv, Some(&mut error)) != 0 {
                result = true;
            }
            crate::ported::eval::typval::tv_clear(rettv);
            if error {
                return FAIL;
            }
        }

        // c:2011 Repeat until there is no following "||".
        while at(*arg, 0) == b'|' && at(*arg, 1) == b'|' {
            {
                let s = *arg;
                *arg = skipwhite(&s[2..]); // c:2013
            }
            ea.eval_flags = if !result {
                orig_flags
            } else {
                orig_flags & !EVAL_EVALUATE
            };
            let mut var2 = typval_T::default();
            if eval3(arg, &mut var2, Some(&mut *ea)) == FAIL {
                return FAIL;
            }

            // c:2020 Compute the result.
            if evaluate && !result {
                let mut error = false;
                if tv_get_number_chk(&var2, Some(&mut error)) != 0 {
                    result = true;
                }
                crate::ported::eval::typval::tv_clear(&mut var2);
                if error {
                    return FAIL;
                }
            }
            if evaluate {
                *rettv = typval_T::from(result as varnumber_T); // c:2032
            }
        }

        ea.eval_flags = orig_flags;
    }

    OK
}

/// Handle second level expression: `expr3 && expr3` (logical AND).
/// Port of `eval3()` from `csrc/eval.c:2056`.
pub fn eval3(arg: &mut &str, rettv: &mut typval_T, mut evalarg: Option<&mut evalarg_T>) -> i32 {
    let at = |s: &str, i: usize| s.as_bytes().get(i).copied().unwrap_or(0);
    // c:2059 Get the first variable.
    if eval4(arg, rettv, evalarg.as_deref_mut()) == FAIL {
        return FAIL;
    }

    // c:2065 Handle the "&&" operator.
    if at(*arg, 0) == b'&' && at(*arg, 1) == b'&' {
        let mut local_evalarg = evalarg_T::default();
        let ea: &mut evalarg_T = match evalarg.as_deref_mut() {
            Some(e) => e,
            None => &mut local_evalarg,
        };
        let orig_flags = ea.eval_flags;
        let evaluate = ea.eval_flags & EVAL_EVALUATE != 0;

        let mut result = true;
        if evaluate {
            let mut error = false;
            if tv_get_number_chk(rettv, Some(&mut error)) == 0 {
                result = false;
            }
            crate::ported::eval::typval::tv_clear(rettv);
            if error {
                return FAIL;
            }
        }

        // c:2089 Repeat until there is no following "&&".
        while at(*arg, 0) == b'&' && at(*arg, 1) == b'&' {
            {
                let s = *arg;
                *arg = skipwhite(&s[2..]);
            }
            ea.eval_flags = if result {
                orig_flags
            } else {
                orig_flags & !EVAL_EVALUATE
            };
            let mut var2 = typval_T::default();
            if eval4(arg, &mut var2, Some(&mut *ea)) == FAIL {
                return FAIL;
            }

            // c:2098 Compute the result.
            if evaluate && result {
                let mut error = false;
                if tv_get_number_chk(&var2, Some(&mut error)) == 0 {
                    result = false;
                }
                crate::ported::eval::typval::tv_clear(&mut var2);
                if error {
                    return FAIL;
                }
            }
            if evaluate {
                *rettv = typval_T::from(result as varnumber_T);
            }
        }

        ea.eval_flags = orig_flags;
    }

    OK
}

/// Handle third level expression: the comparison operators
/// (`==` `!=` `>` `>=` `<` `<=` `=~` `!~` `is` `isnot`).
/// Port of `eval4()` from `csrc/eval.c:2143`.
pub fn eval4(arg: &mut &str, rettv: &mut typval_T, mut evalarg: Option<&mut evalarg_T>) -> i32 {
    let at = |s: &str, i: usize| s.as_bytes().get(i).copied().unwrap_or(0);
    let mut r#type = EXPR_UNKNOWN; // c:2146
    let mut len = 2usize; // c:2147

    // c:2150 Get the first variable.
    if eval5(arg, rettv, evalarg.as_deref_mut()) == FAIL {
        return FAIL;
    }

    let p = *arg; // c:2154
    match at(p, 0) {
        b'=' => {
            if at(p, 1) == b'=' {
                r#type = EXPR_EQUAL;
            } else if at(p, 1) == b'~' {
                r#type = EXPR_MATCH;
            }
        }
        b'!' => {
            if at(p, 1) == b'=' {
                r#type = EXPR_NEQUAL;
            } else if at(p, 1) == b'~' {
                r#type = EXPR_NOMATCH;
            }
        }
        b'>' => {
            if at(p, 1) != b'=' {
                r#type = EXPR_GREATER;
                len = 1;
            } else {
                r#type = EXPR_GEQUAL;
            }
        }
        b'<' => {
            if at(p, 1) != b'=' {
                r#type = EXPR_SMALLER;
                len = 1;
            } else {
                r#type = EXPR_SEQUAL;
            }
        }
        b'i' => {
            if at(p, 1) == b's' {
                if at(p, 2) == b'n' && at(p, 3) == b'o' && at(p, 4) == b't' {
                    len = 5; // c:2189
                }
                let c = at(p, len);
                if !c.is_ascii_alphanumeric() && c != b'_' {
                    r#type = if len == 2 { EXPR_IS } else { EXPR_ISNOT }; // c:2192
                }
            }
        }
        _ => {}
    }

    // c:2199 If there is a comparative operator, use it.
    if r#type != EXPR_UNKNOWN {
        let ic;
        // c:2202 extra `?` = ignore case, extra `#` = match case, else 'ignorecase'.
        if at(p, len) == b'?' {
            ic = true;
            len += 1;
        } else if at(p, len) == b'#' {
            ic = false;
            len += 1;
        } else {
            ic = crate::ported::eval::typval::tv_get_bool(
                &crate::ported::option::get_option_value("ignorecase"),
            ) != 0; // c:2209 p_ic
        }

        // c:2213 Get the second variable.
        *arg = skipwhite(&p[len..]);
        let mut var2 = typval_T::default();
        if eval5(arg, &mut var2, evalarg.as_deref_mut()) == FAIL {
            crate::ported::eval::typval::tv_clear(rettv);
            return FAIL;
        }
        let evaluate = evalarg
            .as_deref()
            .map_or(false, |e| e.eval_flags & EVAL_EVALUATE != 0);
        if evaluate {
            let ret = typval_compare(rettv, &var2, r#type, ic); // c:2219
            return ret;
        }
    }

    OK
}

/// Handle fourth level expression: `+` (number add / list-blob concat), `-`
/// (subtract), `.`/`..` (string concat). Port of `eval5()` from
/// `csrc/eval.c:2389`. Arithmetic is delegated to the leaf helpers
/// [`eval_concat_str`]/[`eval_addblob`]/[`eval_addlist`]/[`eval_addsub_number`].
pub fn eval5(arg: &mut &str, rettv: &mut typval_T, mut evalarg: Option<&mut evalarg_T>) -> i32 {
    let at = |s: &str, i: usize| s.as_bytes().get(i).copied().unwrap_or(0);
    // c:2392 Get the first variable.
    if eval6(arg, rettv, evalarg.as_deref_mut(), false) == FAIL {
        return FAIL;
    }

    // c:2397 Repeat computing, until no '+', '-' or '.' is following.
    loop {
        let op = at(*arg, 0);
        let concat = op == b'.';
        if op != b'+' && op != b'-' && !concat {
            break;
        }

        let evaluate = evalarg
            .as_deref()
            .map_or(false, |e| e.eval_flags & EVAL_EVALUATE != 0); // c:2404
        if (op != b'+' || (rettv.v_type != VAR_LIST && rettv.v_type != VAR_BLOB))
            && (op == b'.' || rettv.v_type != VAR_FLOAT)
            && evaluate
        {
            // c:2414 Check the first operand's type before evaluating the 2nd.
            if (op == b'.' && !crate::ported::eval::typval::tv_check_str(rettv))
                || (op != b'.' && !crate::ported::eval::typval::tv_check_num(rettv))
            {
                crate::ported::eval::typval::tv_clear(rettv);
                return FAIL;
            }
        }

        // c:2420 Get the second variable.
        {
            let s = *arg;
            if op == b'.' && at(s, 1) == b'.' {
                *arg = &s[1..]; // c:2422 ..string concatenation
            }
        }
        {
            let s = *arg;
            *arg = skipwhite(&s[1..]); // c:2424
        }
        let mut var2 = typval_T::default();
        if eval6(arg, &mut var2, evalarg.as_deref_mut(), op == b'.') == FAIL {
            crate::ported::eval::typval::tv_clear(rettv);
            return FAIL;
        }

        if evaluate {
            // c:2432 Compute the result.
            if op == b'.' {
                if eval_concat_str(rettv, &var2) == FAIL {
                    return FAIL;
                }
            } else if op == b'+' && rettv.v_type == VAR_BLOB && var2.v_type == VAR_BLOB {
                eval_addblob(rettv, &var2);
            } else if op == b'+' && rettv.v_type == VAR_LIST && var2.v_type == VAR_LIST {
                if eval_addlist(rettv, &var2) == FAIL {
                    return FAIL;
                }
            } else if eval_addsub_number(rettv, &var2, op) == FAIL {
                return FAIL;
            }
        }
    }
    OK
}

/// Handle fifth level expression: `*` (multiply), `/` (divide), `%` (modulo).
/// Port of `eval6()` from `csrc/eval.c:2545`. Arithmetic delegates to
/// [`eval_multdiv_number`].
pub fn eval6(
    arg: &mut &str,
    rettv: &mut typval_T,
    mut evalarg: Option<&mut evalarg_T>,
    want_string: bool,
) -> i32 {
    let at = |s: &str, i: usize| s.as_bytes().get(i).copied().unwrap_or(0);
    // c:2548 Get the first variable.
    if eval7(arg, rettv, evalarg.as_deref_mut(), want_string) == FAIL {
        return FAIL;
    }

    // c:2553 Repeat computing, until no '*', '/' or '%' is following.
    loop {
        let op = at(*arg, 0);
        if op != b'*' && op != b'/' && op != b'%' {
            break;
        }

        let evaluate = evalarg
            .as_deref()
            .map_or(false, |e| e.eval_flags & EVAL_EVALUATE != 0); // c:2559

        // c:2561 Get the second variable.
        {
            let s = *arg;
            *arg = skipwhite(&s[1..]);
        }
        let mut var2 = typval_T::default();
        if eval7(arg, &mut var2, evalarg.as_deref_mut(), false) == FAIL {
            return FAIL;
        }

        if evaluate {
            // c:2569 Compute the result.
            if eval_multdiv_number(rettv, &var2, op) == FAIL {
                return FAIL;
            }
        }
    }

    OK
}

/// Handle sixth level expression: constants, variables, function calls, nested
/// expressions, Lists/Dicts, options, environment vars, registers, plus leading
/// `!`/`-`/`+` and trailing subscripts/method-calls. Port of `eval7()` from
/// `csrc/eval.c:2608`.
///
/// RUST-PORT NOTE: several `eval7` branches call editor/userfunc subsystems that
/// the standalone evaluator resolves through already-ported adapters or leaves
/// deferred: the register case routes through [`get_reg_contents`]
/// (crate::ported::eval::…), variable lookup through
/// [`eval_variable`](crate::ported::eval::vars::eval_variable), function calls
/// and method calls through the ported [`eval_func`]/[`handle_subscript`]
/// (with the `(args)`/`[idx]`/`.key`/`->m()` chain scanned off the cursor here),
/// the recursion counter is dropped, and lambda `{a -> expr}` (C `get_lambda_tv`,
/// userfunc) and curly-brace `{expr}` names are deferred.
pub fn eval7(
    arg: &mut &str,
    rettv: &mut typval_T,
    mut evalarg: Option<&mut evalarg_T>,
    want_string: bool,
) -> i32 {
    let at = |s: &str, i: usize| s.as_bytes().get(i).copied().unwrap_or(0);
    let evaluate = evalarg
        .as_deref()
        .map_or(false, |e| e.eval_flags & EVAL_EVALUATE != 0); // c:2610
    let verbose = true;

    rettv.v_type = VAR_UNKNOWN; // c:2616
    rettv.vval = crate::ported::eval::typval_defs_h::typval_vval_union::v_unknown;

    // c:2618 Skip '!', '-' and '+' characters. They are handled later.
    let start_leader = *arg;
    let mut p: &str = *arg;
    while matches!(at(p, 0), b'!' | b'-' | b'+') {
        let s = p;
        p = skipwhite(&s[1..]);
    }
    let leaders = &start_leader[..start_leader.len() - p.len()]; // c:2623 [start,end)

    // Balanced-group finder for scanning subscript/call chains off the cursor.
    let find_close = |s: &str, open: u8, close: u8| -> Option<usize> {
        let b = s.as_bytes();
        let mut depth = 0i32;
        let mut i = 0usize;
        while i < b.len() {
            let c = b[i];
            if c == b'\'' {
                i += 1;
                while i < b.len() && b[i] != b'\'' {
                    i += 1;
                }
            } else if c == b'"' {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' && i + 1 < b.len() {
                        i += 1;
                    }
                    i += 1;
                }
            } else if c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    };

    let mut ret;
    let mut did_numeric = false;
    match at(p, 0) {
        // c:2641 Number constant.
        b'0'..=b'9' => {
            let mut pp = p;
            ret = eval_number(&mut pp, rettv, evaluate, want_string);
            p = pp;
            did_numeric = true;
            // c:2655 Apply prefixed "-" and "+" now (matters when "->" follows).
            if ret == OK && evaluate && !leaders.is_empty() {
                eval7_leader(rettv, true, leaders);
            }
        }
        // c:2661 String constant.
        b'"' => {
            let mut pp = p;
            ret = eval_string(&mut pp, rettv, evaluate, false);
            p = pp;
        }
        // c:2666 Literal string constant.
        b'\'' => {
            let mut pp = p;
            ret = eval_lit_string(&mut pp, rettv, evaluate, false);
            p = pp;
        }
        // c:2671 List.
        b'[' => {
            let mut pp = p;
            ret = eval_list(&mut pp, rettv, evalarg.as_deref_mut());
            p = pp;
        }
        // c:2676 Literal Dictionary: #{...}
        b'#' => {
            let mut pp = p;
            ret = eval_lit_dict(&mut pp, rettv, evalarg.as_deref_mut());
            p = pp;
        }
        // c:2682 Lambda / Dictionary.
        b'{' => {
            // RUST-PORT NOTE: get_lambda_tv() (userfunc) is deferred; a plain
            // Dict or a NOTDONE {expr}-name falls through to name resolution.
            let mut pp = p;
            ret = eval_dict(&mut pp, rettv, evalarg.as_deref_mut(), false);
            p = pp;
        }
        // c:2690 Option value: &name
        b'&' => {
            let mut pp = p;
            ret = eval_option(&mut pp, rettv, evaluate);
            p = pp;
        }
        // c:2695 Environment variable / interpolated string.
        b'$' => {
            let mut pp = p;
            if at(pp, 1) == b'"' || at(pp, 1) == b'\'' {
                ret = eval_interp_string(&mut pp, rettv, evaluate, evalarg.as_deref_mut());
            } else {
                ret = eval_env_var(&mut pp, rettv, evaluate);
            }
            p = pp;
        }
        // c:2704 Register contents: @r.
        b'@' => {
            {
                let s = p;
                p = &s[1..]; // (*arg)++
            }
            if evaluate {
                let name = at(p, 0) as char;
                let s = crate::ported::ops::get_reg_contents(name)
                    .map(|v| v.join("\n"))
                    .unwrap_or_default();
                *rettv = typval_T::from(s);
            }
            if at(p, 0) != 0 {
                let s = p;
                p = &s[1..];
            }
            ret = OK;
        }
        // c:2716 nested expression: (expression).
        b'(' => {
            {
                let s = p;
                p = skipwhite(&s[1..]);
            }
            let mut pp = p;
            ret = eval1(&mut pp, rettv, evalarg.as_deref_mut()); // recursive!
            p = pp;
            if at(p, 0) == b')' {
                let s = p;
                p = &s[1..];
            } else if ret == OK {
                emsg("E110: Missing ')'");
                crate::ported::eval::typval::tv_clear(rettv);
                ret = FAIL;
            }
        }
        _ => {
            ret = NOTDONE; // c:2730
        }
    }

    if ret == NOTDONE {
        // c:2734 Must be a variable or function name.
        let name_at = p;
        let len = get_name_len(name_at); // c:2739 (RUST-PORT: no curly/alias)
        if len <= 0 {
            ret = FAIL; // c:2745
        } else {
            let name = &name_at[..len as usize];
            p = &name_at[len as usize..];
            let after = skipwhite(p);
            if at(after, 0) == b'(' {
                // c:2748 "name(..."  recursive!
                if let Some(cl) = find_close(after, b'(', b')') {
                    let inner = &after[1..cl];
                    ret = eval_func(name, inner, None, rettv);
                    let s = after;
                    p = &s[cl + 1..];
                } else {
                    ret = FAIL;
                }
            } else if evaluate {
                // c:2753 get value of variable
                match crate::ported::eval::vars::eval_variable(name) {
                    Some(v) => {
                        *rettv = v;
                        ret = OK;
                    }
                    None => {
                        crate::ported::message::semsg(&format!("E121: Undefined variable: {name}"));
                        ret = FAIL;
                    }
                }
            } else {
                ret = OK; // c:2765 skip the name
            }
        }
    }

    {
        let s = p;
        p = skipwhite(s); // c:2771
    }

    // c:2775 Handle following '[', '(', '.' and '->' subscripts/method-calls.
    if ret == OK {
        let mut subs: Vec<String> = Vec::new();
        loop {
            let c0 = at(p, 0);
            if c0 == b'[' {
                if let Some(cl) = find_close(p, b'[', b']') {
                    subs.push(p[..=cl].to_string());
                    let s = p;
                    p = &s[cl + 1..];
                } else {
                    break;
                }
            } else if c0 == b'.' && eval_isdictc(at(p, 1)) {
                let mut e = 1usize;
                while eval_isdictc(at(p, e)) {
                    e += 1;
                }
                subs.push(p[..e].to_string());
                let s = p;
                p = &s[e..];
            } else if c0 == b'(' {
                if let Some(cl) = find_close(p, b'(', b')') {
                    subs.push(p[..=cl].to_string());
                    let s = p;
                    p = &s[cl + 1..];
                } else {
                    break;
                }
            } else if c0 == b'-' && at(p, 1) == b'>' {
                let mut e = 2usize;
                while eval_isnamec(at(p, e)) {
                    e += 1;
                }
                if at(p, e) == b'(' {
                    if let Some(cl) = find_close(&p[e..], b'(', b')') {
                        let total = e + cl + 1;
                        subs.push(p[..total].to_string());
                        let s = p;
                        p = &s[total..];
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        if !subs.is_empty() {
            let refs: Vec<&str> = subs.iter().map(|x| x.as_str()).collect();
            ret = handle_subscript(rettv, &refs, verbose);
        }
    }

    // c:2779 Apply logical NOT and unary '-', right to left (final pass).
    if ret == OK && evaluate && !leaders.is_empty() {
        // The number branch already ran the numeric (trailing +/-) pass; only
        // the '!' and leaders left of it remain for the non-numeric pass.
        let rem: &str = if did_numeric {
            match leaders.rfind('!') {
                Some(i) => &leaders[..=i],
                None => "",
            }
        } else {
            leaders
        };
        if !rem.is_empty() {
            eval7_leader(rettv, false, rem);
        }
    }

    *arg = p;
    ret
}

/// Allocate a variable for a number constant. Also deals with "0z" for a Blob.
/// Port of `eval_number()` from `csrc/eval.c:3424`.
pub fn eval_number(arg: &mut &str, rettv: &mut typval_T, evaluate: bool, want_string: bool) -> i32 {
    let src = *arg;
    let at = |s: &str, i: usize| s.as_bytes().get(i).copied().unwrap_or(0);
    let mut p = skipdigits(&src[1..]); // c:3426
    let mut get_float = false;

    // c:3434 Accept a float when the format matches; not after the "." operator.
    if !want_string && at(p, 0) == b'.' && at(p, 1).is_ascii_digit() {
        get_float = true;
        p = skipdigits(&p[2..]);
        if at(p, 0) == b'e' || at(p, 0) == b'E' {
            let mut q = &p[1..];
            if at(q, 0) == b'-' || at(q, 0) == b'+' {
                q = &q[1..];
            }
            if !at(q, 0).is_ascii_digit() {
                get_float = false;
            } else {
                p = skipdigits(&q[1..]);
            }
        }
        if at(p, 0).is_ascii_alphabetic() || at(p, 0) == b'.' {
            get_float = false; // c:3448
        }
    }
    if get_float {
        let (f, consumed) = string2float(src); // c:3454
        *arg = &src[consumed..];
        if evaluate {
            *rettv = typval_T {
                v_type: VAR_FLOAT,
                v_lock: VarLockStatus::VAR_UNLOCKED,
                vval: v_float(f),
            };
        }
    } else if at(src, 0) == b'0' && (at(src, 1) == b'z' || at(src, 1) == b'Z') {
        // c:3459 Blob constant: 0z0123456789abcdef
        let blob = if evaluate {
            Some(crate::ported::eval::typval::tv_blob_alloc())
        } else {
            None
        };
        let b = src.as_bytes();
        let mut bp = 2usize; // c: bp = *arg + 2
        while bp < b.len() && b[bp].is_ascii_hexdigit() {
            if !(bp + 1 < b.len() && b[bp + 1].is_ascii_hexdigit()) {
                if blob.is_some() {
                    emsg("E973: Blob literal should have an even number of hex characters");
                }
                return FAIL;
            }
            if let Some(ref bl) = blob {
                bl.borrow_mut()
                    .bv_ga
                    .push((hex2nr(b[bp]) << 4) + hex2nr(b[bp + 1]));
            }
            if bp + 3 < b.len() && b[bp + 2] == b'.' && b[bp + 3].is_ascii_hexdigit() {
                bp += 1; // c:3479
            }
            bp += 2;
        }
        if let Some(bl) = blob {
            crate::ported::eval::typval::tv_blob_set_ret(rettv, bl);
        }
        *arg = &src[bp..];
    } else {
        // c:3487 decimal, hex or octal number
        let mut len = 0i32;
        let mut n: varnumber_T = 0;
        crate::ported::charset::vim_str2nr(
            src,
            None,
            Some(&mut len),
            crate::ported::charset::STR2NR_ALL,
            Some(&mut n),
            None,
            0,
            true,
            None,
        );
        if len == 0 {
            if evaluate {
                crate::ported::message::semsg(&format!("E15: Invalid expression: \"{src}\""));
            }
            return FAIL;
        }
        *arg = &src[len as usize..];
        if evaluate {
            *rettv = typval_T::from(n);
        }
    }
    OK
}

/// Evaluate a `"string"` constant. When "interpolate" is true reduce `{{`→`{`,
/// `}}`→`}` and stop at a single `{`. Port of `eval_string()` from
/// `csrc/eval.c:3512`.
///
/// RUST-PORT NOTE: the C two-pass (measure, then copy) is folded into one pass
/// building a `String` (byte values 0x80–0xFF become the matching Latin-1
/// codepoint, consistent with the crate's String-as-byte-string model). The
/// `\<Key>` special-key escape (C `trans_special`/`find_special_key`, the
/// keycodes subsystem) is deferred: the `<` is copied literally.
pub fn eval_string(arg: &mut &str, rettv: &mut typval_T, evaluate: bool, interpolate: bool) -> i32 {
    let src = *arg;
    let b = src.as_bytes();
    let off = if interpolate { 0usize } else { 1 };
    let mut out = String::new();
    let mut i = off;
    let mut term: u8 = 0;

    while i < b.len() {
        let c = b[i];
        if c == b'"' {
            term = b'"'; // c:3520 found the closing quote
            break;
        }
        if c == b'\\' && i + 1 < b.len() {
            i += 1;
            let nc = b[i];
            match nc {
                b'b' => {
                    out.push('\u{08}');
                    i += 1;
                } // c:3576 BS
                b'e' => {
                    out.push('\u{1b}');
                    i += 1;
                } // ESC
                b'f' => {
                    out.push('\u{0c}');
                    i += 1;
                } // FF
                b'n' => {
                    out.push('\n');
                    i += 1;
                } // NL
                b'r' => {
                    out.push('\r');
                    i += 1;
                } // CAR
                b't' => {
                    out.push('\t');
                    i += 1;
                } // TAB
                // c:3589 hex "\x1", unicode "#", "\U..."
                b'X' | b'x' | b'u' | b'U' => {
                    if i + 1 < b.len() && b[i + 1].is_ascii_hexdigit() {
                        let up = nc & !0x20; // toupper
                        let mut n = if up == b'X' {
                            2
                        } else if nc == b'u' {
                            4
                        } else {
                            8
                        };
                        let mut nr: u32 = 0;
                        while n > 0 && i + 1 < b.len() && b[i + 1].is_ascii_hexdigit() {
                            i += 1;
                            nr = (nr << 4) + hex2nr(b[i]) as u32;
                            n -= 1;
                        }
                        i += 1;
                        if up != b'X' {
                            if let Some(ch) = char::from_u32(nr) {
                                out.push(ch); // c:3613 utf_char2bytes
                            }
                        } else {
                            out.push(char::from(nr as u8)); // c:3615
                        }
                    }
                }
                // c:3620 octal "\1", "\12", "\123"
                b'0'..=b'7' => {
                    let mut val = (b[i] - b'0') as u32;
                    i += 1;
                    if i < b.len() && (b'0'..=b'7').contains(&b[i]) {
                        val = (val << 3) + (b[i] - b'0') as u32;
                        i += 1;
                        if i < b.len() && (b'0'..=b'7').contains(&b[i]) {
                            val = (val << 3) + (b[i] - b'0') as u32;
                            i += 1;
                        }
                    }
                    out.push(char::from(val as u8));
                }
                // c:3640 special key "\<C-W>" — deferred (keycodes subsystem)
                b'<' => {
                    out.push('<');
                    i += 1;
                }
                _ => {
                    // c:3659 mb_copy_char
                    let ch = src[i..].chars().next().unwrap();
                    out.push(ch);
                    i += ch.len_utf8();
                }
            }
            continue;
        } else if interpolate && (c == b'{' || c == b'}') {
            if c == b'{' && b.get(i + 1) != Some(&b'{') {
                term = b'{'; // c:3664 start of expression
                break;
            }
            i += 1; // c:3667 reduce "{{"→"{", "}}"→"}"
            if i < b.len() {
                let ch = src[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
            }
            continue;
        } else {
            let ch = src[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }

    if term != b'"' && !(interpolate && term == b'{') {
        crate::ported::message::semsg(&format!("E114: Missing quote: {src}")); // c:3556
        return FAIL;
    }

    if !evaluate {
        *arg = &src[i + off..]; // c:3562
        return OK;
    }

    *rettv = typval_T::from(out); // c:3568
    let mut end = i;
    if term == b'"' && !interpolate {
        end += 1; // c:3674
    }
    *arg = &src[end..];
    OK
}

/// Evaluate a `'str''ing'` literal-string constant. When "interpolate" is true
/// reduce `{{`→`{` and stop at a single `{`. Port of `eval_lit_string()` from
/// `csrc/eval.c:3686`.
pub fn eval_lit_string(
    arg: &mut &str,
    rettv: &mut typval_T,
    evaluate: bool,
    interpolate: bool,
) -> i32 {
    let src = *arg;
    let b = src.as_bytes();
    let off = if interpolate { 0usize } else { 1 };
    let mut out = String::new();
    let mut i = off;
    let mut term: u8 = 0;

    while i < b.len() {
        let c = b[i];
        if c == b'\'' {
            if b.get(i + 1) != Some(&b'\'') {
                term = b'\''; // c:3695
                break;
            }
            out.push('\''); // c:3698 '' → '
            i += 2;
            continue;
        } else if interpolate && c == b'{' {
            if b.get(i + 1) != Some(&b'{') {
                term = b'{'; // c:3703
                break;
            }
            out.push('{'); // c:3705 {{ → {
            i += 2;
            continue;
        } else if interpolate && c == b'}' {
            i += 1; // c:3708
            if b.get(i) != Some(&b'}') {
                crate::ported::message::semsg(&format!(
                    "E1278: Stray '}}' without a matching '{{': {src}"
                ));
                return FAIL;
            }
            out.push('}');
            i += 1;
            continue;
        } else {
            let ch = src[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }

    if term != b'\'' && !(interpolate && term == b'{') {
        crate::ported::message::semsg(&format!("E115: Missing quote: {src}")); // c:3719
        return FAIL;
    }

    if !evaluate {
        *arg = &src[i + off..]; // c:3725
        return OK;
    }

    *rettv = typval_T::from(out); // c:3732
    *arg = &src[i + off..]; // c:3750
    OK
}

/// Evaluate a single/double quoted string that may contain `{expr}` groups.
/// "arg" points to the `$`. Port of `eval_interp_string()` from
/// `csrc/eval.c:3759`.
///
/// RUST-PORT NOTE: the C `eval_one_expr_in_str()` helper (which parses one
/// expression from the string, evaluates it, and appends its string form) is
/// inlined here as a direct [`eval1`] over the `{…}` body, which is why this
/// port carries the `evalarg` the C signature threads through the helper.
pub fn eval_interp_string(
    arg: &mut &str,
    rettv: &mut typval_T,
    evaluate: bool,
    mut evalarg: Option<&mut evalarg_T>,
) -> i32 {
    let at = |s: &str, i: usize| s.as_bytes().get(i).copied().unwrap_or(0);
    let mut ga = String::new();

    // c:3767 *arg is on the '$', move to the first string char, then past quote.
    {
        let s = *arg;
        *arg = &s[1..];
    }
    let quote = at(*arg, 0);
    {
        let s = *arg;
        *arg = &s[1..];
    }

    let mut ret;
    loop {
        let mut tv = typval_T::default();
        // c:3775 Get the string up to the matching quote or a single '{'.
        if quote == b'"' {
            ret = eval_string(arg, &mut tv, evaluate, true);
        } else {
            ret = eval_lit_string(arg, &mut tv, evaluate, true);
        }
        if ret == FAIL {
            break;
        }
        if evaluate {
            ga.push_str(&crate::ported::eval::typval::tv_get_string(&tv)); // c:3784
        }

        if at(*arg, 0) != b'{' {
            // c:3788 found terminating quote
            let s = *arg;
            *arg = &s[1..];
            break;
        }

        // c:3793 eval_one_expr_in_str: at '{', evaluate the body up to '}'.
        {
            let s = *arg;
            *arg = &s[1..];
        }
        let mut etv = typval_T::default();
        if eval1(arg, &mut etv, evalarg.as_deref_mut()) == FAIL {
            ret = FAIL;
            break;
        }
        {
            let s = *arg;
            *arg = skipwhite(s);
        }
        if at(*arg, 0) != b'}' {
            crate::ported::message::semsg(&format!("E1279: Missing '}}': {}", *arg));
            ret = FAIL;
            break;
        }
        {
            let s = *arg;
            *arg = &s[1..];
        }
        if evaluate {
            ga.push_str(&crate::ported::eval::typval::tv_get_string(&etv));
        }
    }

    // c:3801 Always returns OK with whatever was collected.
    let _ = ret;
    *rettv = typval_T::from(ga);
    OK
}

/// Allocate a variable for a List and fill it from `*arg` (points to the `[`).
/// Port of `eval_list()` from `csrc/eval.c:3857`.
pub fn eval_list(arg: &mut &str, rettv: &mut typval_T, mut evalarg: Option<&mut evalarg_T>) -> i32 {
    let at = |s: &str, i: usize| s.as_bytes().get(i).copied().unwrap_or(0);
    let evaluate = evalarg
        .as_deref()
        .map_or(false, |e| e.eval_flags & EVAL_EVALUATE != 0); // c:3859
    let l = if evaluate {
        Some(crate::ported::eval::typval::tv_list_alloc(-1))
    } else {
        None
    };

    {
        let s = *arg;
        *arg = skipwhite(&s[1..]); // c:3866
    }
    while at(*arg, 0) != b']' && at(*arg, 0) != 0 {
        let mut tv = typval_T::default();
        if eval1(arg, &mut tv, evalarg.as_deref_mut()) == FAIL {
            // c:3870 failret (Rc drops the partial list)
            return FAIL;
        }
        if evaluate {
            tv.v_lock = VarLockStatus::VAR_UNLOCKED;
            if let Some(ref l) = l {
                crate::ported::eval::typval::tv_list_append_owned_tv(&mut l.borrow_mut(), tv);
            }
        }

        // c:3878 the comma must come after the value
        let had_comma = at(*arg, 0) == b',';
        if had_comma {
            let s = *arg;
            *arg = skipwhite(&s[1..]);
        }

        if at(*arg, 0) == b']' {
            break;
        }
        if !had_comma {
            crate::ported::message::semsg(&format!("E696: Missing comma in List: {}", *arg));
            return FAIL;
        }
    }

    if at(*arg, 0) != b']' {
        crate::ported::message::semsg(&format!("E697: Missing end of List ']': {}", *arg)); // c:3894
        return FAIL;
    }

    {
        let s = *arg;
        *arg = skipwhite(&s[1..]); // c:3902
    }
    if evaluate {
        if let Some(l) = l {
            *rettv = typval_T {
                v_type: VAR_LIST,
                v_lock: VarLockStatus::VAR_UNLOCKED,
                vval: v_list(Some(l)),
            };
        }
    }
    OK
}

/// Allocate a variable for a Dictionary and fill it from `*arg` (points to the
/// `{` or, for a literal `#{...}` dict, the char after `#`). Returns NOTDONE for
/// a curly-braces `{expr}` name. Port of `eval_dict()` from `csrc/eval.c:4444`.
pub fn eval_dict(
    arg: &mut &str,
    rettv: &mut typval_T,
    mut evalarg: Option<&mut evalarg_T>,
    literal: bool,
) -> i32 {
    let at = |s: &str, i: usize| s.as_bytes().get(i).copied().unwrap_or(0);
    let evaluate = evalarg
        .as_deref()
        .map_or(false, |e| e.eval_flags & EVAL_EVALUATE != 0); // c:4446
    let src = *arg;

    // c:4452 First check it's not a curly-braces expression {expr} (no eval).
    let curly = skipwhite(&src[1..]);
    if at(curly, 0) != b'}' && !literal {
        let mut cp = curly;
        let mut tv = typval_T::default();
        if eval1(&mut cp, &mut tv, None) == OK && at(skipwhite(cp), 0) == b'}' {
            return NOTDONE; // c:4462
        }
    }

    let d = if evaluate {
        Some(crate::ported::eval::typval::tv_dict_alloc())
    } else {
        None
    };

    {
        let s = *arg;
        *arg = skipwhite(&s[1..]); // c:4473
    }
    while at(*arg, 0) != b'}' && at(*arg, 0) != 0 {
        let mut tvkey = typval_T::default();
        // c:4475 recursive! (literal key vs an expression key)
        let keyok = if literal {
            match get_literal_key(*arg) {
                Some((k, rest)) => {
                    tvkey = typval_T::from(k);
                    *arg = rest;
                    true
                }
                None => false,
            }
        } else {
            eval1(arg, &mut tvkey, evalarg.as_deref_mut()) == OK
        };
        if !keyok {
            return FAIL;
        }
        if at(*arg, 0) != b':' {
            crate::ported::message::semsg(&format!("E720: Missing colon in Dictionary: {}", *arg));
            return FAIL;
        }
        let key = if evaluate {
            match crate::ported::eval::typval::tv_get_string_buf_chk(&tvkey) {
                Some(k) => k,
                None => return FAIL, // c:4487 errmsg already given
            }
        } else {
            String::new()
        };

        {
            let s = *arg;
            *arg = skipwhite(&s[1..]); // c:4494
        }
        let mut tv = typval_T::default();
        if eval1(arg, &mut tv, evalarg.as_deref_mut()) == FAIL {
            return FAIL;
        }
        if evaluate {
            if let Some(ref d) = d {
                if crate::ported::eval::typval::tv_dict_find(&d.borrow(), &key).is_some() {
                    crate::ported::message::semsg(&format!(
                        "E721: Duplicate key in Dictionary: \"{key}\""
                    )); // c:4502
                    return FAIL;
                }
                tv.v_lock = VarLockStatus::VAR_UNLOCKED;
                crate::ported::eval::typval::tv_dict_add(&mut d.borrow_mut(), &key, tv);
            }
        }

        // c:4516 the comma must come after the value
        let had_comma = at(*arg, 0) == b',';
        if had_comma {
            let s = *arg;
            *arg = skipwhite(&s[1..]);
        }
        if at(*arg, 0) == b'}' {
            break;
        }
        if !had_comma {
            crate::ported::message::semsg(&format!("E722: Missing comma in Dictionary: {}", *arg));
            return FAIL;
        }
    }

    if at(*arg, 0) != b'}' {
        crate::ported::message::semsg(&format!("E723: Missing end of Dictionary '}}': {}", *arg)); // c:4532
        return FAIL;
    }

    {
        let s = *arg;
        *arg = skipwhite(&s[1..]); // c:4540
    }
    if evaluate {
        if let Some(d) = d {
            *rettv = typval_T {
                v_type: VAR_DICT,
                v_lock: VarLockStatus::VAR_UNLOCKED,
                vval: v_dict(Some(d)),
            };
        }
    }
    OK
}

/// Evaluate a literal dictionary `#{key: val, …}`. "*arg" points to the `#`.
/// Returns NOTDONE for `{expr}`. Port of `eval_lit_dict()` from
/// `csrc/eval.c:4552`.
pub fn eval_lit_dict(arg: &mut &str, rettv: &mut typval_T, evalarg: Option<&mut evalarg_T>) -> i32 {
    let at = |s: &str, i: usize| s.as_bytes().get(i).copied().unwrap_or(0);
    if at(*arg, 1) == b'{' {
        {
            let s = *arg;
            *arg = &s[1..]; // c:4557 (*arg)++
        }
        eval_dict(arg, rettv, evalarg, true)
    } else {
        NOTDONE // c:4560
    }
}

/// Get the value of an environment variable. "arg" points to the `$`.
/// Port of `eval_env_var()` from `csrc/eval.c:4603`.
///
/// RUST-PORT NOTE: `vim_getenv()` is `std::env::var`; the `expand_env_save`
/// fallback for `$VIM`/`${HOME}` (the runtime-path expansion subsystem) is
/// deferred, so an unset variable is simply the empty string.
pub fn eval_env_var(arg: &mut &str, rettv: &mut typval_T, evaluate: bool) -> i32 {
    {
        let s = *arg;
        *arg = &s[1..]; // c:4605 (*arg)++
    }
    let name_full = *arg;
    let len = get_env_len(name_full) as usize; // c:4607

    if evaluate && len == 0 {
        return FAIL; // c:4611 Invalid empty name.
    }
    if evaluate {
        let name = &name_full[..len];
        let string = std::env::var(name).unwrap_or_default(); // c:4616 vim_getenv
        *rettv = typval_T::from(string);
    }
    *arg = &name_full[len..];
    OK
}

/// Evaluate an option value `&name`. "*arg" points to the `&`.
/// Port of `eval_option()` from `csrc/eval.c:3371`.
///
/// RUST-PORT NOTE: `find_option_var_end()`/`OptIndex`/`is_tty_option()` (the
/// option-table subsystem) are not modeled; the option name is scanned inline
/// after any `&+`/`&<`/`&g:`/`&l:` prefix and looked up via
/// [`get_option_value`](crate::ported::option::get_option_value), which returns
/// an empty value for an unknown name (the C `E113` check is deferred).
pub fn eval_option(arg: &mut &str, rettv: &mut typval_T, evaluate: bool) -> i32 {
    let src = *arg;
    let b = src.as_bytes();
    let mut i = 1usize; // past '&'
    if b.get(i) == Some(&b'+') || b.get(i) == Some(&b'<') {
        i += 1; // has("+opt") / &<opt
    }
    if (b.get(i) == Some(&b'g') || b.get(i) == Some(&b'l')) && b.get(i + 1) == Some(&b':') {
        i += 2; // &g: / &l: scope prefix
    }
    let name_start = i;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    if i == name_start {
        if evaluate {
            crate::ported::message::semsg(&format!("E112: Option name missing: {src}"));
            // c:3383
        }
        return FAIL;
    }
    let name = &src[name_start..i];
    *arg = &src[i..];
    if evaluate {
        *rettv = crate::ported::option::get_option_value(name); // c:3407
    }
    OK
}

/// `OPT_GLOBAL` (`src/nvim/option.h:26`) — `find_option_var_end` scope flag: use
/// the option's global value.
pub const OPT_GLOBAL: i32 = 0x01;
/// `OPT_LOCAL` (`src/nvim/option.h:27`) — scope flag: use the option's local value.
pub const OPT_LOCAL: i32 = 0x02;

/// Skip over the name of an option variable: `&option`, `&g:option` or
/// `&l:option`.
/// Port of `find_option_var_end()` from `csrc/eval.c:6297`.
///
/// `*arg` points to the `&`/`+` on entry; on a found name it is advanced to the
/// option name (past any `g:`/`l:` scope prefix) and `opt_flags` is set to
/// [`OPT_GLOBAL`]/[`OPT_LOCAL`]/`0`. Returns the option name length (the offset of
/// the char after the name in the advanced `*arg`), or `None` when no name found.
///
/// RUST-PORT NOTE: the C tail `find_option_end()` also sets an `OptIndex`; the
/// option-index table is not modeled standalone (see [`eval_option`]), so the
/// ASCII-alpha name span is scanned inline and no index is returned.
pub fn find_option_var_end(arg: &mut &str, opt_flags: &mut i32) -> Option<usize> {
    let src = *arg;
    let b = src.as_bytes();

    let mut p = 1usize; // c:6302 p++ (past '&'/'+')
    if b.get(p) == Some(&b'g') && b.get(p + 1) == Some(&b':') {
        *opt_flags = OPT_GLOBAL; // c:6304
        p += 2;
    } else if b.get(p) == Some(&b'l') && b.get(p + 1) == Some(&b':') {
        *opt_flags = OPT_LOCAL; // c:6307
        p += 2;
    } else {
        *opt_flags = 0; // c:6310
    }

    // c:6313 end = find_option_end(p, opt_idxp): the name is the ASCII-alpha run.
    let name_start = p;
    while p < b.len() && b[p].is_ascii_alphabetic() {
        p += 1;
    }
    if p == name_start {
        return None; // c:6314 end == NULL → *arg unchanged, return NULL
    }
    *arg = &src[name_start..]; // c:6314 *arg = p
    Some(p - name_start)
}

/// Writes "<sourcing_name>:<sourcing_lnum>".
/// Port of `eval_fmt_source_name_line()` from `csrc/eval.c:6659`.
///
/// RUST-PORT NOTE: the C writes into a caller buffer; here the formatted string
/// is returned. There is no execution stack standalone (no `SOURCING_NAME`), so
/// this always yields `"?"`.
pub fn eval_fmt_source_name_line() -> String {
    // c:6661 SOURCING_NAME is NULL standalone.
    "?".to_string() // c:6664
}

/// `typedef struct { … } forinfo_T;` — info used by a ":for" loop
/// (`csrc/eval.c:123`).
///
/// RUST-PORT NOTE: the C `listwatch_T fi_lw` field is omitted. List watchers
/// are not modeled anywhere in this port (`tv_list_watch_add`/`tv_list_watch_*`
/// are no-ops in `eval/typval.rs`), and `fi_lw.lw_item` is a raw `listitem_T *`
/// cursor into the watched list — not expressible over the `Rc<RefCell<list_T>>`
/// + `Vec<listitem_T>` representation. `fi_list` holds the list handle instead;
/// `next_for_item()` (deferred) iterates it.
#[derive(Debug, Default)]
pub struct forinfo_T {
    /// `int fi_semicolon` — true if ending in '; var]'.
    pub fi_semicolon: i32,
    /// `int fi_varcount` — nr of variables in the list.
    pub fi_varcount: i32,
    /// `list_T *fi_list` — list being used.
    pub fi_list: Option<Rc<RefCell<list_T>>>,
    /// `int fi_bi` — index of blob.
    pub fi_bi: i32,
    /// `blob_T *fi_blob` — blob being used.
    pub fi_blob: Option<Rc<RefCell<blob_T>>>,
    /// `char *fi_string` — copy of string being used.
    pub fi_string: Option<String>,
    /// `int fi_byte_idx` — byte index in fi_string.
    pub fi_byte_idx: i32,
}

/// Port of `eval_for_line()` from `csrc/eval.c:1435`.
///
/// Set up a ":for" loop iterator from `arg` (`for {var} in {expr}`): parse the
/// loop-variable lvalue, require the "in" keyword, then evaluate the source
/// expression into the iterator (List / Blob / String). `*errp` is cleared on a
/// successfully parsed expression.
///
/// RUST-PORT NOTE: the C `exarg_T *eap` argument is dropped — the ported `eval0`
/// signature has no `eap`. `evalarg` is passed by `&mut` (C: pointer). The
/// C `emsg_skip++/--` bracketing (which suppresses errors while only parsing)
/// is not modeled: `emsg_skip` is a global editor counter with no standalone
/// analog (matching `ex_let_vars`'s handling in `eval/vars.rs`).
pub fn eval_for_line(arg: &str, errp: &mut bool, evalarg: &mut evalarg_T) -> forinfo_T {
    let mut fi = forinfo_T::default(); // c:1437 xcalloc
    let mut tv = typval_T::default(); // c:1438
    let skip = evalarg.eval_flags & EVAL_EVALUATE == 0; // c:1440

    *errp = true; // c:1442 Default: there is an error.

    // c:1444 expr = skip_var_list(arg, &fi->fi_varcount, &fi->fi_semicolon, false);
    let (consumed, varcount, semicolon) = match skip_var_list(arg, false) {
        Some(t) => t,
        None => return fi, // c:1446
    };
    fi.fi_varcount = varcount;
    fi.fi_semicolon = i32::from(semicolon);
    let expr = &arg[consumed..];

    let expr = skipwhite(expr); // c:1449
    let eb = expr.as_bytes();
    // c:1450 expr[0] != 'i' || expr[1] != 'n' || !(expr[2] == NUL || ascii_iswhite(expr[2]))
    let c2 = eb.get(2).copied().unwrap_or(0);
    if eb.first().copied().unwrap_or(0) != b'i'
        || eb.get(1).copied().unwrap_or(0) != b'n'
        || !(c2 == 0 || c2 == b' ' || c2 == b'\t')
    {
        emsg("E690: Missing \"in\" after :for"); // c:1452
        return fi;
    }

    // c:1456 if (skip) emsg_skip++;  — RUST-PORT NOTE: emsg_skip not modeled.
    let expr = skipwhite(&expr[2..]); // c:1459
    if eval0(expr, &mut tv, Some(evalarg)) == OK {
        // c:1460
        *errp = false; // c:1461
        if !skip {
            // c:1462
            match tv.v_type {
                VAR_LIST => {
                    // c:1463
                    let l = if let v_list(l) = &tv.vval {
                        l.clone()
                    } else {
                        None
                    };
                    match l {
                        // c:1465 a null list is like an empty list: do nothing
                        None => tv_clear(&mut tv), // c:1467
                        Some(lst) => {
                            // c:1469 No need to increment the refcount, it's already
                            // set for the list being used in "tv".
                            tv_list_watch_add(&mut lst.borrow_mut()); // c:1472 (no-op)
                            fi.fi_list = Some(lst); // c:1471
                        }
                    }
                }
                VAR_BLOB => {
                    // c:1475
                    fi.fi_bi = 0; // c:1476
                    let b = if let v_blob(b) = &tv.vval {
                        b.clone()
                    } else {
                        None
                    };
                    if let Some(bb) = b {
                        // c:1477
                        // c:1480 Make a copy, so that the iteration still works when
                        // the blob is changed.
                        let mut btv = typval_T::default(); // c:1478
                        tv_blob_copy(Some(&bb), &mut btv); // c:1482
                        if let v_blob(nb) = btv.vval {
                            fi.fi_blob = nb; // c:1483
                        }
                    }
                    tv_clear(&mut tv); // c:1485
                }
                VAR_STRING => {
                    // c:1486
                    fi.fi_byte_idx = 0; // c:1487
                    let s = if let v_string(s) = &tv.vval {
                        Some(s.clone())
                    } else {
                        None
                    };
                    fi.fi_string = s; // c:1488
                                      // c:1489 tv.vval.v_string = NULL — steal the string so `tv_clear`
                                      // (elsewhere) would not double-free it.
                    tv.vval = v_unknown;
                    tv.v_type = VAR_UNKNOWN;
                    if fi.fi_string.is_none() {
                        // c:1490
                        fi.fi_string = Some(String::new()); // c:1491 xstrdup("")
                    }
                }
                _ => {
                    // c:1493
                    emsg("E1098: String, List or Blob required"); // c:1494
                    tv_clear(&mut tv); // c:1495
                }
            }
        }
    }
    // c:1499 if (skip) emsg_skip--;  — RUST-PORT NOTE: emsg_skip not modeled.

    fi // c:1503
}

/// Port of `buf_byteidx_to_charidx()` from `csrc/eval.c:5228`.
///
/// Convert byte index `byteidx` of line `lnum` in `buf` to a character index
/// (both zero-based). Works only for loaded buffers; returns -1 on failure.
///
/// RUST-PORT NOTE: `buf_T *buf` (nullable) becomes `Option<&Rc<RefCell<buf_T>>>`.
/// C's `utfc_ptr2len` (which folds trailing composing characters into one
/// grapheme) collapses to [`utf_ptr2len`] here — the composing-character /
/// grapheme machinery (`utf_composinglike`, `GraphemeState`) is not yet ported
/// in `mbyte.rs`, so combining sequences count per code point.
pub fn buf_byteidx_to_charidx(
    buf: Option<&Rc<RefCell<buf_T>>>,
    mut lnum: linenr_T,
    byteidx: i32,
) -> i32 {
    // c:5230 if (buf == NULL || buf->b_ml.ml_mfp == NULL) return -1;
    let buf = match buf {
        Some(b) if b.borrow().b_ml.ml_mfp => b,
        _ => return -1,
    };
    let mut b = buf.borrow_mut();

    // c:5234 if (lnum > buf->b_ml.ml_line_count) lnum = buf->b_ml.ml_line_count;
    if lnum > b.b_ml.ml_line_count {
        lnum = b.b_ml.ml_line_count;
    }

    let str = ml_get_buf(&mut b, lnum); // c:5238
    let sb = str.as_bytes();

    // c:5240 if (*str == NUL) return 0;
    if sb.is_empty() {
        return 0;
    }

    // c:5245 count the number of characters
    let mut t = 0usize; // c: t = str
    let mut count = 0i32;
    // c:5247 for (count = 0; *t != NUL && t <= str + byteidx; count++) t += utfc_ptr2len(t);
    while t < sb.len() && (t as i32) <= byteidx {
        count += 1;
        t += utf_ptr2len(&sb[t..]) as usize;
    }

    // c:5251 In insert mode, when the cursor is at the end of a non-empty line,
    // byteidx points to the NUL past the end of the string: add one.
    // c:5254 if (*t == NUL && byteidx != 0 && t == str + byteidx) count++;
    if t >= sb.len() && byteidx != 0 && t as i32 == byteidx {
        count += 1;
    }

    count - 1 // c:5258
}

/// Port of `buf_charidx_to_byteidx()` from `csrc/eval.c:5266`.
///
/// Convert character index `charidx` of line `lnum` in `buf` to a byte index
/// (both zero-based). Works only for loaded buffers; returns -1 on failure.
///
/// RUST-PORT NOTE: nullable `buf_T *` → `Option<&Rc<RefCell<buf_T>>>`, and
/// `utfc_ptr2len` collapses to [`utf_ptr2len`] (see `buf_byteidx_to_charidx`).
pub fn buf_charidx_to_byteidx(
    buf: Option<&Rc<RefCell<buf_T>>>,
    mut lnum: linenr_T,
    mut charidx: i32,
) -> i32 {
    // c:5268 if (buf == NULL || buf->b_ml.ml_mfp == NULL) return -1;
    let buf = match buf {
        Some(b) if b.borrow().b_ml.ml_mfp => b,
        _ => return -1,
    };
    let mut b = buf.borrow_mut();

    // c:5272 if (lnum > buf->b_ml.ml_line_count) lnum = buf->b_ml.ml_line_count;
    if lnum > b.b_ml.ml_line_count {
        lnum = b.b_ml.ml_line_count;
    }

    let str = ml_get_buf(&mut b, lnum); // c:5276
    let sb = str.as_bytes();

    // c:5279 Convert the character offset to a byte offset
    let mut t = 0usize; // c: t = str
                        // c:5280 while (*t != NUL && --charidx > 0) t += utfc_ptr2len(t);
    while t < sb.len() && {
        charidx -= 1;
        charidx > 0
    } {
        t += utf_ptr2len(&sb[t..]) as usize;
    }

    t as i32 // c:5284 return (int)(t - str);
}

/// Port of `var2fpos()` from `csrc/eval.c:5299`.
///
/// Translate a Vimscript object into a buffer/window position. Accepts a
/// `VAR_LIST` (`[lnum, col, coladd]`) or a `VAR_STRING` name (`"."`, `"v"`,
/// `"$"`, `"'m"`, `"w0"`, `"w$"`). Returns the resolved position or `None`
/// (C `NULL`) on error; `ret_fnum` receives the file number for global marks.
///
/// RUST-PORT NOTE: C returns a pointer to a `static pos_T`; here the position
/// is returned by value as `Option<pos_T>`. `win_T *wp` becomes
/// `&Rc<RefCell<win_T>>`, and `wp->w_buffer` (a raw always-non-NULL pointer in
/// C) is modeled as an `Option`, so a bufferless window yields `None`.
///
/// DEFERRED: the `"'m"` named-mark case (`mark_get`/`fmark_T`/`kMarkAll`) and
/// the `"w0"`/`"w$"` visible-line cases (`update_topline`/`validate_botline_win`
/// and the `w_topline`/`w_botline` fields) need the mark and screen/topline
/// subsystems, which are not modeled — those names fall through to `None`. The
/// `"v"` Visual-start case has no `VIsual_active`/`VIsual` globals here, so it
/// resolves to the cursor (the C fallback when Visual is inactive).
pub fn var2fpos(
    tv: &typval_T,
    dollar_lnum: bool,
    ret_fnum: &mut i32,
    charcol: bool,
    wp: &Rc<RefCell<win_T>>,
) -> Option<pos_T> {
    let mut pos = pos_T::default(); // c:5303 static pos_T pos;

    // c:5305 buf_T *bp = wp->w_buffer;
    let bp = wp.borrow().w_buffer.clone()?;

    // c:5307 Argument can be [lnum, col, coladd].
    if tv.v_type == VAR_LIST {
        let mut error = false; // c:5309

        // c:5311 list_T *l = tv->vval.v_list;
        let l = match &tv.vval {
            v_list(Some(l)) => l.clone(),
            _ => return None, // c:5313 (NULL list)
        };
        let lb = l.borrow();

        // c:5317 Get the line number.
        pos.lnum = tv_list_find_nr(&lb, 0, Some(&mut error)) as linenr_T;
        if error || pos.lnum <= 0 || pos.lnum > bp.borrow().b_ml.ml_line_count {
            // c:5319 Invalid line number.
            return None;
        }

        // c:5323 Get the column number.
        pos.col = tv_list_find_nr(&lb, 1, Some(&mut error)) as colnr_T;
        if error {
            return None; // c:5326
        }
        let len: i32;
        if charcol {
            // c:5330 len = mb_charlen(ml_get_buf(bp, pos.lnum));
            // RUST-PORT NOTE: mb_charlen() is not yet ported in mbyte.rs; inline
            // its `utf_ptr2len` code-point walk (composing chars per code point).
            let line = ml_get_buf(&mut bp.borrow_mut(), pos.lnum);
            let lbytes = line.as_bytes();
            let mut i = 0usize;
            let mut cnt = 0i32;
            while i < lbytes.len() {
                cnt += 1;
                i += utf_ptr2len(&lbytes[i..]) as usize;
            }
            len = cnt;
        } else {
            // c:5332 len = ml_get_buf_len(bp, pos.lnum);
            len = ml_get_buf_len(&mut bp.borrow_mut(), pos.lnum);
        }

        // c:5335 We accept "$" for the column number: last column.
        // c:5336 listitem_T *li = tv_list_find(l, 1);
        let is_dollar = match tv_list_find(&lb, 1) {
            Some(li) => matches!(&li.li_tv.vval, v_string(s) if s == "$"),
            None => false,
        };
        if is_dollar {
            pos.col = len + 1; // c:5340
        }

        // c:5343 Accept a position up to the NUL after the line.
        if pos.col == 0 || pos.col > len + 1 {
            // c:5344 Invalid column number.
            return None;
        }
        pos.col -= 1; // c:5348

        // c:5350 Get the virtual offset.  Defaults to zero.
        pos.coladd = tv_list_find_nr(&lb, 2, Some(&mut error)) as colnr_T;
        if error {
            pos.coladd = 0; // c:5353
        }

        return Some(pos); // c:5356
    }

    // c:5359 const char *const name = tv_get_string_chk(tv);
    let name = tv_get_string_chk(tv)?;
    let nb = name.as_bytes();

    pos.lnum = 0; // c:5364
    if nb.first() == Some(&b'.') {
        // c:5365 cursor
        pos = wp.borrow().w_cursor; // c:5367
    } else if nb.first() == Some(&b'v') && nb.get(1).copied().unwrap_or(0) == 0 {
        // c:5368 Visual start.
        // RUST-PORT NOTE: no VIsual_active/VIsual globals — resolve to the
        // cursor (the C branch taken when Visual mode is inactive). c:5373
        pos = wp.borrow().w_cursor;
    } else if nb.first() == Some(&b'\'') {
        // c:5375 mark — DEFERRED: the mark subsystem (mark_get/fmark_T/kMarkAll)
        // is not modeled; leave pos.lnum == 0 so this falls through to `None`.
    }
    if pos.lnum != 0 {
        // c:5386
        if charcol {
            // c:5388 pos.col = buf_byteidx_to_charidx(bp, pos.lnum, pos.col);
            pos.col = buf_byteidx_to_charidx(Some(&bp), pos.lnum, pos.col);
        }
        return Some(pos); // c:5390
    }

    pos.coladd = 0; // c:5393

    if nb.first() == Some(&b'w') && dollar_lnum {
        // c:5395 "w0"/"w$" — DEFERRED: update_topline/validate_botline_win and
        // the w_topline/w_botline fields (screen/topline subsystem) are not
        // modeled; fall through to `None`.
    } else if nb.first() == Some(&b'$') {
        // c:5413 last column or line
        if dollar_lnum {
            pos.lnum = bp.borrow().b_ml.ml_line_count; // c:5415
            pos.col = 0; // c:5416
        } else {
            let cursor_lnum = wp.borrow().w_cursor.lnum;
            pos.lnum = cursor_lnum; // c:5418
            if charcol {
                // c:5420 pos.col = mb_charlen(ml_get_buf(bp, wp->w_cursor.lnum));
                // RUST-PORT NOTE: inline mb_charlen (not ported); see above.
                let line = ml_get_buf(&mut bp.borrow_mut(), cursor_lnum);
                let lbytes = line.as_bytes();
                let mut i = 0usize;
                let mut cnt = 0i32;
                while i < lbytes.len() {
                    cnt += 1;
                    i += utf_ptr2len(&lbytes[i..]) as usize;
                }
                pos.col = cnt;
            } else {
                // c:5422 pos.col = ml_get_buf_len(bp, wp->w_cursor.lnum);
                pos.col = ml_get_buf_len(&mut bp.borrow_mut(), cursor_lnum);
            }
        }
        return Some(pos); // c:5425
    }
    None // c:5427
}

/// Port of `list2fpos()` from `csrc/eval.c:5440`.
///
/// Convert list in `arg` into position `posp` and optional file number `fnump`.
/// When `fnump` is `None` there is no file number, only 3 items:
/// `[lnum, col, off]`. The column is passed on as-is (the caller may decrement
/// it). Returns `FAIL` when conversion is not possible (does not validate the
/// resulting position). If `charcol`, the column is a character index.
///
/// RUST-PORT NOTE: the out-parameters `int *fnump`/`colnr_T *curswantp` become
/// `Option<&mut …>`; C's `curbuf->b_fnum` reads the `.handle` alias.
pub fn list2fpos(
    arg: &typval_T,
    posp: &mut pos_T,
    mut fnump: Option<&mut i32>,
    curswantp: Option<&mut colnr_T>,
    charcol: bool,
) -> i32 {
    // c:5444 List must be: [fnum, lnum, col, coladd, curswant], where "fnum" is
    // only there when "fnump" isn't NULL; "coladd"/"curswant" are optional.
    let l = match &arg.vval {
        v_list(Some(l)) if arg.v_type == VAR_LIST => l.clone(),
        _ => return FAIL, // c:5450
    };
    {
        let lb = l.borrow();
        let n = tv_list_len(&lb);
        // c:5448 tv_list_len(l) < (fnump == NULL ? 2 : 3) || > (fnump == NULL ? 4 : 5)
        let (lo, hi) = if fnump.is_none() { (2, 4) } else { (3, 5) };
        if n < lo || n > hi {
            return FAIL;
        }
    }

    let lb = l.borrow();
    let mut i = 0; // c:5453
    if let Some(fp) = fnump.as_deref_mut() {
        let mut n = tv_list_find_nr(&lb, i, None) as i32; // c:5456 fnum
        i += 1;
        if n < 0 {
            return FAIL; // c:5458
        }
        if n == 0 {
            // c:5460 Current buffer.
            n = curbuf
                .with(|c| c.borrow().clone())
                .map(|b| b.borrow().handle)
                .unwrap_or(0); // c:5461 curbuf->b_fnum
        }
        *fp = n; // c:5463
    }

    let mut n = tv_list_find_nr(&lb, i, None) as i32; // c:5466 lnum
    i += 1;
    if n < 0 {
        return FAIL; // c:5468
    }
    posp.lnum = n; // c:5470

    n = tv_list_find_nr(&lb, i, None) as i32; // c:5472 col
    i += 1;
    if n < 0 {
        return FAIL; // c:5474
    }
    // c:5476 If character position is specified, convert to byte position; if the
    // line number is zero use the cursor line.
    if charcol {
        // c:5480 Get the text for the specified line in a loaded buffer.
        let fnum = match fnump.as_deref() {
            None => curbuf
                .with(|c| c.borrow().clone())
                .map(|b| b.borrow().handle)
                .unwrap_or(0),
            Some(fp) => *fp,
        };
        let buf = buflist_findnr(fnum);
        // c:5481 if (buf == NULL || buf->b_ml.ml_mfp == NULL) return FAIL;
        match &buf {
            Some(b) if b.borrow().b_ml.ml_mfp => {}
            _ => return FAIL,
        }
        let lnum = if posp.lnum == 0 {
            // c:5485 curwin->w_cursor.lnum
            curwin
                .with(|c| c.borrow().clone())
                .map(|w| w.borrow().w_cursor.lnum)
                .unwrap_or(0)
        } else {
            posp.lnum
        };
        n = buf_charidx_to_byteidx(buf.as_ref(), lnum, n) + 1; // c:5484
    }
    posp.col = n; // c:5488

    n = tv_list_find_nr(&lb, i, None) as i32; // c:5490 off
    if n < 0 {
        posp.coladd = 0; // c:5492
    } else {
        posp.coladd = n; // c:5494
    }

    if let Some(cw) = curswantp {
        // c:5498 *curswantp = tv_list_find_nr(l, i + 1, NULL);
        *cw = tv_list_find_nr(&lb, i + 1, None) as colnr_T;
    }

    OK // c:5501
}

#[cfg(test)]
mod tests {
    use super::string2float;

    #[test]
    fn handle_subscript_chain() {
        use super::handle_subscript;
        use crate::ported::eval::typval::EVAL_STRING_HOOK;
        use crate::ported::eval_h::OK;
        fn hook(e: &str) -> Option<typval_T> {
            e.trim().parse::<i64>().ok().map(typval_T::from)
        }
        let saved = EVAL_STRING_HOOK.with(|h| *h.borrow());
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = Some(hook));
        // "abcdef"[1:4][1] → "bcde"[1] → "c"
        let mut s = typval_T::from("abcdef".to_string());
        assert_eq!(handle_subscript(&mut s, &["[1:4]", "[1]"], false), OK);
        assert!(matches!(&s.vval, v_string(t) if t == "c"));
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = saved);
    }

    #[test]
    fn eval_index_subscripts() {
        use super::eval_index;
        use crate::ported::eval::typval::EVAL_STRING_HOOK;
        use crate::ported::eval_h::OK;
        fn hook(e: &str) -> Option<typval_T> {
            e.trim().parse::<i64>().ok().map(typval_T::from)
        }
        let saved = EVAL_STRING_HOOK.with(|h| *h.borrow());
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = Some(hook));
        // string[1] (char-based)
        let mut s = typval_T::from("héllo".to_string());
        assert_eq!(eval_index(&mut s, "[1]", false), OK);
        assert!(matches!(&s.vval, v_string(t) if t == "é"));
        // string[1:3] inclusive
        let mut s2 = typval_T::from("abcdef".to_string());
        eval_index(&mut s2, "[1:3]", false);
        assert!(matches!(&s2.vval, v_string(t) if t == "bcd"));
        // open-ended [2:]
        let mut s3 = typval_T::from("abcdef".to_string());
        eval_index(&mut s3, "[2:]", false);
        assert!(matches!(&s3.vval, v_string(t) if t == "cdef"));
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = saved);
    }

    #[test]
    fn eval_index_inner_types() {
        use super::eval_index_inner;
        use crate::ported::eval::typval::{tv_dict_add_nr, tv_list_alloc, tv_list_append_number};
        use crate::ported::eval::typval_defs_h::{
            dict_T, typval_vval_union::v_dict, VarLockStatus, VarType::VAR_DICT,
        };
        use crate::ported::eval_h::{FAIL, OK};
        use std::{cell::RefCell, rc::Rc};
        // string subscript [1] is char-based
        let mut s = typval_T::from("héllo".to_string());
        assert_eq!(
            eval_index_inner(
                &mut s,
                false,
                Some(&typval_T::from(1)),
                None,
                false,
                None,
                false
            ),
            OK
        );
        assert!(matches!(&s.vval, v_string(t) if t == "é"));
        // string slice [1:3] inclusive
        let mut s2 = typval_T::from("abcdef".to_string());
        eval_index_inner(
            &mut s2,
            true,
            Some(&typval_T::from(1)),
            Some(&typval_T::from(3)),
            false,
            None,
            false,
        );
        assert!(matches!(&s2.vval, v_string(t) if t == "bcd"));
        // list index [2]
        let l = tv_list_alloc(-1);
        for n in [10, 20, 30] {
            tv_list_append_number(&mut l.borrow_mut(), n);
        }
        let mut lv = typval_T {
            v_type: crate::ported::eval::typval_defs_h::VarType::VAR_LIST,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: crate::ported::eval::typval_defs_h::typval_vval_union::v_list(Some(l)),
        };
        eval_index_inner(
            &mut lv,
            false,
            Some(&typval_T::from(2)),
            None,
            false,
            None,
            false,
        );
        assert!(matches!(lv.vval, v_number(30)));
        // dict key
        let mut d = dict_T::default();
        tv_dict_add_nr(&mut d, "k", 7);
        let mut dv = typval_T {
            v_type: VAR_DICT,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_dict(Some(Rc::new(RefCell::new(d)))),
        };
        assert_eq!(
            eval_index_inner(&mut dv, false, None, None, false, Some("k"), false),
            OK
        );
        assert!(matches!(dv.vval, v_number(7)));
        // missing key → FAIL; re-make a dict for the missing-key case
        let mut d2 = dict_T::default();
        tv_dict_add_nr(&mut d2, "k", 7);
        let mut dv2 = typval_T {
            v_type: VAR_DICT,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_dict(Some(Rc::new(RefCell::new(d2)))),
        };
        assert_eq!(
            eval_index_inner(&mut dv2, false, None, None, false, Some("nope"), false),
            FAIL
        );
    }
    use super::{
        char_from_string, char_idx2byte, find_name_end, get_literal_key, is_luafunc, string_slice,
        to_name_end, tv_is_luafunc, var_flavour, var_flavour_T::*,
    };
    use crate::ported::eval::typval_defs_h::{
        typval_T, typval_vval_union::*, VarLockStatus, VarType::*,
    };
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
        assert_eq!(
            string_slice("abcdef", 1, -1, false).as_deref(),
            Some("bcdef")
        );
        // empty result
        assert_eq!(string_slice("abc", 2, 1, false), None);
    }

    #[test]
    fn eval7_leader_unary() {
        use super::eval7_leader;
        let not = |n: i64, l: &str| {
            let mut tv = typval_T::from(n);
            eval7_leader(&mut tv, false, l);
            match tv.vval {
                v_number(x) => x,
                _ => -999,
            }
        };
        assert_eq!(not(5, "!"), 0); // truthy → 0
        assert_eq!(not(0, "!"), 1); // falsy → 1
        assert_eq!(not(5, "-"), -5); // negate
        assert_eq!(not(5, "--"), 5); // double negate
        assert_eq!(not(5, "!-"), 0); // - then ! : -5 truthy → 0
                                     // numeric_only stops at '!', leaving the value
        let mut tv = typval_T::from(5);
        eval7_leader(&mut tv, true, "!");
        assert!(matches!(tv.vval, v_number(5)));
    }

    #[test]
    fn binary_op_helpers() {
        use super::{eval_addsub_number, eval_concat_str};
        use crate::ported::eval_h::OK;
        let mut a = typval_T::from(3);
        assert_eq!(eval_addsub_number(&mut a, &typval_T::from(4), b'+'), OK);
        assert!(matches!(a.vval, v_number(7)));
        let mut b = typval_T::from(10);
        eval_addsub_number(&mut b, &typval_T::from(4), b'-');
        assert!(matches!(b.vval, v_number(6)));
        // float promotion: 3 + 1.5 = 4.5
        let mut c = typval_T::from(3);
        let f = typval_T {
            v_type: VAR_FLOAT,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_float(1.5),
        };
        eval_addsub_number(&mut c, &f, b'+');
        assert!(matches!(c.vval, v_float(x) if (x - 4.5).abs() < 1e-9));
        // string concat
        let mut s = typval_T::from("foo".to_string());
        assert_eq!(
            eval_concat_str(&mut s, &typval_T::from("bar".to_string())),
            OK
        );
        assert!(matches!(&s.vval, v_string(t) if t == "foobar"));
        // multdiv: number * and /
        use super::eval_multdiv_number;
        let mut m = typval_T::from(6);
        eval_multdiv_number(&mut m, &typval_T::from(7), b'*');
        assert!(matches!(m.vval, v_number(42)));
        let mut d = typval_T::from(17);
        eval_multdiv_number(&mut d, &typval_T::from(5), b'/');
        assert!(matches!(d.vval, v_number(3)));
        let mut md = typval_T::from(17);
        eval_multdiv_number(&mut md, &typval_T::from(5), b'%');
        assert!(matches!(md.vval, v_number(2)));
    }

    #[test]
    fn do_string_sub_basic() {
        use super::do_string_sub;
        assert_eq!(do_string_sub("hello", "l", "L", "g"), "heLLo");
        assert_eq!(do_string_sub("hello", "l", "L", ""), "heLlo");
        assert_eq!(do_string_sub("abc", "x", "y", "g"), "abc");
    }

    #[test]
    fn make_expanded_name_curly() {
        use super::make_expanded_name;
        use crate::ported::eval::typval::EVAL_STRING_HOOK;
        fn hook(e: &str) -> Option<typval_T> {
            match e {
                "x" => Some(typval_T::from("BAR".to_string())),
                "1" => Some(typval_T::from("ONE".to_string())),
                _ => None,
            }
        }
        let saved = EVAL_STRING_HOOK.with(|h| *h.borrow());
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = Some(hook));
        // foo{x}baz → fooBARbaz
        assert_eq!(
            make_expanded_name("foo{x}baz", 3, 5).as_deref(),
            Some("fooBARbaz")
        );
        // two groups expand recursively
        assert_eq!(
            make_expanded_name("a{x}b{1}c", 1, 3).as_deref(),
            Some("aBARbONEc")
        );
        // a failing expr → None
        assert_eq!(make_expanded_name("p{bad}q", 1, 5), None);
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = saved);
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
        assert_eq!(
            get_literal_key("a-b : x").unwrap(),
            ("a-b".to_string(), ": x")
        );
        assert!(get_literal_key(":bad").is_none());
    }

    #[test]
    fn var_flavour_classes() {
        assert_eq!(var_flavour("FOO"), VAR_FLAVOUR_SHADA);
        assert_eq!(var_flavour("Foo"), VAR_FLAVOUR_SESSION);
        assert_eq!(var_flavour("foo"), VAR_FLAVOUR_DEFAULT);
    }

    #[test]
    fn string_to_list_splits() {
        use super::string_to_list;
        use crate::ported::eval::typval_defs_h::typval_vval_union::v_string;
        let lines = |s: &str, keep: bool| -> Vec<String> {
            string_to_list(s, keep)
                .borrow()
                .lv_items
                .iter()
                .map(|it| match &it.li_tv.vval {
                    v_string(s) => s.clone(),
                    _ => String::new(),
                })
                .collect()
        };
        // trailing NL dropped unless keepempty
        assert_eq!(
            lines("a\nb\n", false),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(
            lines("a\nb\n", true),
            vec!["a".to_string(), "b".to_string(), String::new()]
        );
        assert_eq!(lines("a\nb", false), vec!["a".to_string(), "b".to_string()]);
        // embedded NUL → NL within an item
        assert_eq!(lines("x\0y", false), vec!["x\ny".to_string()]);
    }

    #[test]
    fn save_tv_as_string_modes() {
        use super::save_tv_as_string;
        use crate::ported::eval::typval::{tv_list_alloc, tv_list_append_string};
        use crate::ported::eval::typval_defs_h::{
            typval_T, typval_vval_union::v_list, VarLockStatus, VarType::VAR_LIST,
        };
        // scalar string
        assert_eq!(
            save_tv_as_string(&typval_T::from("hi".to_string()), false, false).as_deref(),
            Some("hi")
        );
        // Unknown / Number → None
        assert_eq!(save_tv_as_string(&typval_T::from(5), false, false), None);
        // list: items separated by NL, trailing NL only with endnl
        let l = tv_list_alloc(-1);
        tv_list_append_string(&mut l.borrow_mut(), "a");
        tv_list_append_string(&mut l.borrow_mut(), "b");
        let lv = typval_T {
            v_type: VAR_LIST,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_list(Some(l)),
        };
        assert_eq!(
            save_tv_as_string(&lv, false, false).as_deref(),
            Some("a\nb")
        );
        assert_eq!(
            save_tv_as_string(&lv, true, false).as_deref(),
            Some("a\nb\n")
        );
    }

    #[test]
    fn check_can_index_by_type() {
        use super::check_can_index;
        use crate::ported::eval_h::{FAIL, OK};
        let tv = |t, v| typval_T {
            v_type: t,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v,
        };
        // indexable
        assert_eq!(
            check_can_index(&tv(VAR_STRING, v_string("x".into())), true, false),
            OK
        );
        assert_eq!(
            check_can_index(&tv(VAR_NUMBER, v_number(1)), true, false),
            OK
        );
        // not indexable
        assert_eq!(
            check_can_index(&tv(VAR_FLOAT, v_float(1.0)), true, false),
            FAIL
        );
        assert_eq!(
            check_can_index(
                &tv(
                    VAR_BOOL,
                    v_bool(crate::ported::eval::typval_defs_h::BoolVarValue::kBoolVarTrue)
                ),
                true,
                false
            ),
            FAIL
        );
        assert_eq!(
            check_can_index(&tv(VAR_FUNC, v_string("F".into())), true, false),
            FAIL
        );
        // unknown: FAIL only when evaluating
        assert_eq!(
            check_can_index(&tv(VAR_UNKNOWN, v_number(0)), true, false),
            FAIL
        );
        assert_eq!(
            check_can_index(&tv(VAR_UNKNOWN, v_number(0)), false, false),
            OK
        );
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
    fn eval_string_wrappers_via_hook() {
        use super::{eval_expr, eval_to_number, eval_to_string};
        use crate::ported::eval::typval::EVAL_STRING_HOOK;
        fn hook(expr: &str) -> Option<typval_T> {
            match expr {
                "42" => Some(typval_T::from(42)),
                "str" => Some(typval_T::from("hi".to_string())),
                _ => None,
            }
        }
        let saved = EVAL_STRING_HOOK.with(|h| *h.borrow());
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = Some(hook));
        assert_eq!(eval_to_number("42"), 42);
        assert_eq!(eval_to_string("str").as_deref(), Some("hi"));
        assert!(eval_expr("42").is_some());
        // parse/eval error → the C error sentinels
        assert_eq!(eval_to_number("bad"), -1);
        assert_eq!(eval_to_string("bad"), None);
        assert!(eval_expr("bad").is_none());
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = saved);
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
        let f = typval_T {
            v_type: VAR_FUNC,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_string("F".into()),
        };
        let mut rv = typval_T::from(-1);
        assert_eq!(eval_expr_func(&f, &[typval_T::from(1)], &mut rv), OK);
        assert!(matches!(rv.vval, v_number(1)));
        // Empty name → FAIL.
        let empty = typval_T {
            v_type: VAR_FUNC,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_string(String::new()),
        };
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
        assert_eq!(
            callback_call_retnr(&Callback::Funcref("F".into()), &[typval_T::from(9)]),
            1
        );
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

    fn ev() -> super::evalarg_T {
        super::evalarg_T {
            eval_flags: super::EVAL_EVALUATE,
        }
    }

    #[test]
    fn eval0_arithmetic_precedence() {
        use super::eval0;
        use crate::ported::eval_h::OK;
        let run = |src: &str| -> typval_T {
            let mut tv = typval_T::default();
            let mut e = ev();
            assert_eq!(eval0(src, &mut tv, Some(&mut e)), OK, "eval0 {src}");
            tv
        };
        // precedence: * binds tighter than +
        assert!(matches!(run("1 + 2 * 3").vval, v_number(7)));
        // nested parens
        assert!(matches!(run("2 * (3 + 4)").vval, v_number(14)));
        // subtraction / division / modulo
        assert!(matches!(run("17 - 5").vval, v_number(12)));
        assert!(matches!(run("17 / 5").vval, v_number(3)));
        assert!(matches!(run("17 % 5").vval, v_number(2)));
    }

    #[test]
    fn eval0_comparisons_and_logic() {
        use super::eval0;
        let n = |src: &str| -> i64 {
            let mut tv = typval_T::default();
            let mut e = ev();
            super::eval0(src, &mut tv, Some(&mut e));
            match tv.vval {
                v_number(x) => x,
                _ => -999,
            }
        };
        let _ = eval0;
        assert_eq!(n("1 < 2"), 1);
        assert_eq!(n("2 <= 2"), 1);
        assert_eq!(n("3 > 5"), 0);
        assert_eq!(n("1 == 1"), 1);
        assert_eq!(n("1 != 1"), 0);
        // logical && / || short-circuit result
        assert_eq!(n("1 && 0"), 0);
        assert_eq!(n("0 || 5"), 1);
        // ternary
        assert_eq!(n("1 ? 10 : 20"), 10);
        assert_eq!(n("0 ? 10 : 20"), 20);
        // falsy operator
        assert_eq!(n("0 ?? 7"), 7);
        // unary leaders
        assert_eq!(n("!0"), 1);
        assert_eq!(n("!5"), 0);
        assert_eq!(n("--3"), 3);
    }

    #[test]
    fn eval0_string_concat() {
        use super::eval0;
        let mut tv = typval_T::default();
        let mut e = ev();
        eval0(r#"'ab' . 'cd'"#, &mut tv, Some(&mut e));
        assert!(matches!(&tv.vval, v_string(s) if s == "abcd"));
        // .. operator too
        let mut tv2 = typval_T::default();
        let mut e2 = ev();
        eval0(r#"'x' .. 'y'"#, &mut tv2, Some(&mut e2));
        assert!(matches!(&tv2.vval, v_string(s) if s == "xy"));
    }

    #[test]
    fn eval_number_forms() {
        use super::eval_number;
        use crate::ported::eval_h::OK;
        // decimal
        let mut s = "42 rest";
        let mut tv = typval_T::default();
        assert_eq!(eval_number(&mut s, &mut tv, true, false), OK);
        assert!(matches!(tv.vval, v_number(42)));
        assert_eq!(s, " rest");
        // hex
        let mut s = "0x1f";
        let mut tv = typval_T::default();
        eval_number(&mut s, &mut tv, true, false);
        assert!(matches!(tv.vval, v_number(31)));
        // float
        let mut s = "3.5";
        let mut tv = typval_T::default();
        eval_number(&mut s, &mut tv, true, false);
        assert!(matches!(tv.vval, v_float(x) if (x - 3.5).abs() < 1e-9));
        // blob 0z...
        let mut s = "0zDEADBEEF";
        let mut tv = typval_T::default();
        eval_number(&mut s, &mut tv, true, false);
        match &tv.vval {
            v_blob(Some(b)) => assert_eq!(b.borrow().bv_ga, vec![0xDE, 0xAD, 0xBE, 0xEF]),
            _ => panic!("expected blob"),
        }
    }

    #[test]
    fn eval_string_escapes() {
        use super::eval_string;
        let s = |src: &str| -> String {
            let mut a = src;
            let mut tv = typval_T::default();
            eval_string(&mut a, &mut tv, true, false);
            match tv.vval {
                v_string(x) => x,
                _ => String::new(),
            }
        };
        assert_eq!(s(r#""a\tb""#), "a\tb");
        assert_eq!(s(r#""x\ny""#), "x\ny");
        assert_eq!(s(r#""\x41""#), "A"); // hex escape
        assert_eq!(s(r#""\101""#), "A"); // octal escape
        assert_eq!(s(r#""plain""#), "plain");
    }

    #[test]
    fn eval_lit_string_reduces_quotes() {
        use super::eval_lit_string;
        let mut a = r#"'it''s'"#;
        let mut tv = typval_T::default();
        eval_lit_string(&mut a, &mut tv, true, false);
        assert!(matches!(&tv.vval, v_string(x) if x == "it's"));
    }

    #[test]
    fn eval_list_and_dict_literals() {
        use super::{eval_dict, eval_list};
        use crate::ported::eval::typval::tv_dict_find;
        // list
        let mut a = "[1, 2, 3]";
        let mut tv = typval_T::default();
        let mut e = ev();
        eval_list(&mut a, &mut tv, Some(&mut e));
        match &tv.vval {
            v_list(Some(l)) => assert_eq!(l.borrow().lv_items.len(), 3),
            _ => panic!("expected list"),
        }
        // dict
        let mut a = r#"{'a': 1, 'b': 2}"#;
        let mut tv = typval_T::default();
        let mut e = ev();
        eval_dict(&mut a, &mut tv, Some(&mut e), false);
        match &tv.vval {
            v_dict(Some(d)) => {
                let d = d.borrow();
                assert!(matches!(tv_dict_find(&d, "a"), Some(v) if matches!(v.vval, v_number(1))));
                assert!(matches!(tv_dict_find(&d, "b"), Some(v) if matches!(v.vval, v_number(2))));
            }
            _ => panic!("expected dict"),
        }
    }

    #[test]
    fn eval7_full_stack_number_with_subscript() {
        use super::eval0;
        use crate::ported::eval::typval::EVAL_STRING_HOOK;
        // The index sub-expression ("1") is evaluated through EVAL_STRING_HOOK,
        // the bridge integration point the ported eval helpers delegate to (see
        // eval_index / handle_subscript). Install a literal hook like the sibling
        // tests so the eval0..eval7 + handle_subscript + eval_index chain runs.
        fn hook(expr: &str) -> Option<typval_T> {
            match expr.trim() {
                "1" => Some(typval_T::from(1)),
                _ => None,
            }
        }
        let saved = EVAL_STRING_HOOK.with(|h| *h.borrow());
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = Some(hook));
        // list index through the whole eval0..eval7 + handle_subscript chain
        let mut tv = typval_T::default();
        let mut e = ev();
        eval0("[10, 20, 30][1]", &mut tv, Some(&mut e));
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = saved);
        assert!(matches!(tv.vval, v_number(20)));
    }

    // ── lval machinery (get_lval / get_lval_* / set_var_lval / clear_lval) ──

    #[test]
    fn get_lval_and_set_dict_item() {
        use super::{get_lval, lval_T, set_var_lval, LlTv};
        use crate::ported::eval::typval::tv_dict_alloc;
        use crate::ported::eval::typval_defs_h::typval_vval_union::{v_dict, v_number};
        use crate::ported::eval::typval_defs_h::{typval_T, VarLockStatus, VarType::VAR_DICT};
        use crate::ported::eval::vars::set_var;
        // g:gld = {"a": 1}
        let d = tv_dict_alloc();
        d.borrow_mut()
            .dv_hashtab
            .insert("a".to_string(), typval_T::from(1));
        set_var(
            "g:gld",
            0,
            typval_T {
                v_type: VAR_DICT,
                v_lock: VarLockStatus::VAR_UNLOCKED,
                vval: v_dict(Some(d.clone())),
            },
            false,
        );
        // Parse g:gld.a — an existing Dict item.
        let mut lp = lval_T::default();
        let end = get_lval("g:gld.a", None, &mut lp, false, false, 0, 0);
        assert!(end.is_some());
        assert!(matches!(lp.ll_tv, LlTv::DictItem(_, ref k) if k == "a"));
        // Assign 99; the write persists through the aliased Rc.
        let mut rettv = typval_T::from(99);
        set_var_lval(&mut lp, 0, &mut rettv, true, false, Some("="));
        assert!(
            matches!(d.borrow().dv_hashtab.get("a"), Some(t) if matches!(t.vval, v_number(99)))
        );
    }

    #[test]
    fn get_lval_adds_new_dict_key() {
        use super::{get_lval, lval_T, set_var_lval};
        use crate::ported::eval::typval::tv_dict_alloc;
        use crate::ported::eval::typval_defs_h::typval_vval_union::{v_dict, v_number};
        use crate::ported::eval::typval_defs_h::{typval_T, VarLockStatus, VarType::VAR_DICT};
        use crate::ported::eval::vars::set_var;
        let d = tv_dict_alloc();
        set_var(
            "g:gldnew",
            0,
            typval_T {
                v_type: VAR_DICT,
                v_lock: VarLockStatus::VAR_UNLOCKED,
                vval: v_dict(Some(d.clone())),
            },
            false,
        );
        // g:gldnew.fresh — a non-existing key records ll_newkey (GLV_STOP path).
        let mut lp = lval_T::default();
        let end = get_lval("g:gldnew.fresh", None, &mut lp, false, false, 0, 0);
        assert!(end.is_some());
        assert_eq!(lp.ll_newkey.as_deref(), Some("fresh"));
        let mut rettv = typval_T::from(7);
        set_var_lval(&mut lp, 0, &mut rettv, true, false, Some("="));
        assert!(
            matches!(d.borrow().dv_hashtab.get("fresh"), Some(t) if matches!(t.vval, v_number(7)))
        );
    }

    #[test]
    fn get_lval_and_set_list_item() {
        use super::{get_lval, lval_T, set_var_lval};
        use crate::ported::eval::typval::{tv_list_alloc, tv_list_append_number, EVAL_STRING_HOOK};
        use crate::ported::eval::typval_defs_h::typval_vval_union::{v_list, v_number};
        use crate::ported::eval::typval_defs_h::{typval_T, VarLockStatus, VarType::VAR_LIST};
        use crate::ported::eval::vars::set_var;
        fn hook(e: &str) -> Option<typval_T> {
            e.trim().parse::<i64>().ok().map(typval_T::from)
        }
        let saved = EVAL_STRING_HOOK.with(|h| *h.borrow());
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = Some(hook));
        let l = tv_list_alloc(-1);
        for n in [10i64, 20, 30] {
            tv_list_append_number(&mut l.borrow_mut(), n);
        }
        set_var(
            "g:gllst",
            0,
            typval_T {
                v_type: VAR_LIST,
                v_lock: VarLockStatus::VAR_UNLOCKED,
                vval: v_list(Some(l.clone())),
            },
            false,
        );
        // g:gllst[1] — the index sub-expression is evaluated via EVAL_STRING_HOOK.
        let mut lp = lval_T::default();
        let end = get_lval("g:gllst[1]", None, &mut lp, false, false, 0, 0);
        assert!(end.is_some());
        assert_eq!(lp.ll_n1, 1);
        let mut rettv = typval_T::from(99);
        set_var_lval(&mut lp, 0, &mut rettv, true, false, Some("="));
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = saved);
        assert!(matches!(l.borrow().lv_items[1].li_tv.vval, v_number(99)));
    }

    #[test]
    fn get_lval_and_set_blob_byte() {
        use super::{get_lval, lval_T, set_var_lval};
        use crate::ported::eval::typval::{tv_blob_alloc, EVAL_STRING_HOOK};
        use crate::ported::eval::typval_defs_h::typval_vval_union::v_blob;
        use crate::ported::eval::typval_defs_h::{typval_T, VarLockStatus, VarType::VAR_BLOB};
        use crate::ported::eval::vars::set_var;
        fn hook(e: &str) -> Option<typval_T> {
            e.trim().parse::<i64>().ok().map(typval_T::from)
        }
        let saved = EVAL_STRING_HOOK.with(|h| *h.borrow());
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = Some(hook));
        let b = tv_blob_alloc();
        b.borrow_mut().bv_ga = vec![1u8, 2, 3];
        set_var(
            "g:glblb",
            0,
            typval_T {
                v_type: VAR_BLOB,
                v_lock: VarLockStatus::VAR_UNLOCKED,
                vval: v_blob(Some(b.clone())),
            },
            false,
        );
        let mut lp = lval_T::default();
        let end = get_lval("g:glblb[0]", None, &mut lp, false, false, 0, 0);
        assert!(end.is_some());
        assert!(lp.ll_blob.is_some());
        let mut rettv = typval_T::from(255);
        set_var_lval(&mut lp, 0, &mut rettv, true, false, Some("="));
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = saved);
        assert_eq!(b.borrow().bv_ga[0], 255);
    }

    #[test]
    fn clear_lval_frees_names() {
        use super::{clear_lval, lval_T};
        let mut lp = lval_T::default();
        lp.ll_exp_name = Some("expanded".to_string());
        lp.ll_newkey = Some("k".to_string());
        clear_lval(&mut lp);
        assert!(lp.ll_exp_name.is_none());
        assert!(lp.ll_newkey.is_none());
    }

    #[test]
    fn skip_expr_advances_and_restores_flags() {
        use super::{evalarg_T, skip_expr, EVAL_EVALUATE};
        use crate::ported::eval_h::OK;
        // No evalarg: the expression is skipped and *pp advances past it.
        let mut p: &str = "1 + 2 | rest";
        assert_eq!(skip_expr(&mut p, None), OK);
        assert!(p.starts_with('|'), "pp not advanced past expr: {p:?}");
        // With an evalarg the eval_flags are cleared while skipping and restored.
        let mut ea = evalarg_T {
            eval_flags: EVAL_EVALUATE,
        };
        let mut q: &str = "3 * 4";
        assert_eq!(skip_expr(&mut q, Some(&mut ea)), OK);
        assert_eq!(ea.eval_flags, EVAL_EVALUATE);
    }

    #[test]
    fn eval0_simple_funccal_falls_through_to_eval0() {
        use super::{eval0_simple_funccal, evalarg_T, EVAL_EVALUATE};
        use crate::ported::eval_h::OK;
        // may_call_simple_func() declines (NOTDONE), so eval0 evaluates "1 + 2".
        let mut ea = evalarg_T {
            eval_flags: EVAL_EVALUATE,
        };
        let mut rv = typval_T::default();
        assert_eq!(eval0_simple_funccal("1 + 2", &mut rv, Some(&mut ea)), OK);
        assert!(matches!(rv.vval, v_number(3)));
    }

    #[test]
    fn find_option_var_end_scopes() {
        use super::{find_option_var_end, OPT_GLOBAL, OPT_LOCAL};
        let mut flags = -1;
        let mut a: &str = "&g:number";
        assert_eq!(find_option_var_end(&mut a, &mut flags), Some(6));
        assert_eq!(a, "number");
        assert_eq!(flags, OPT_GLOBAL);

        let mut b: &str = "&l:ai=1";
        assert_eq!(find_option_var_end(&mut b, &mut flags), Some(2));
        assert_eq!(b, "ai=1"); // *arg at the name; char after name is '='
        assert_eq!(flags, OPT_LOCAL);

        let mut c: &str = "&wrap";
        assert_eq!(find_option_var_end(&mut c, &mut flags), Some(4));
        assert_eq!(c, "wrap");
        assert_eq!(flags, 0);

        // No option name after '&': NULL, *arg unchanged.
        let mut d: &str = "&123";
        assert_eq!(find_option_var_end(&mut d, &mut flags), None);
        assert_eq!(d, "&123");
    }

    #[test]
    fn eval_fmt_source_name_line_no_stack() {
        use super::eval_fmt_source_name_line;
        assert_eq!(eval_fmt_source_name_line(), "?");
    }

    #[test]
    fn eval_lambda_prepends_base_and_calls() {
        use super::{eval_lambda, evalarg_T, EVAL_EVALUATE};
        use crate::ported::eval::typval::{CALL_FUNC_HOOK, EVAL_STRING_HOOK};
        use crate::ported::eval_h::OK;
        fn eval_hook(e: &str) -> Option<typval_T> {
            e.trim().parse::<i64>().ok().map(typval_T::from)
        }
        // Sum the argument values so we can observe the base was prepended.
        fn call_hook(_c: &typval_T, args: &[typval_T]) -> Option<typval_T> {
            let sum: i64 = args
                .iter()
                .map(|a| match a.vval {
                    v_number(n) => n,
                    _ => 0,
                })
                .sum();
            Some(typval_T::from(sum))
        }
        let saved_e = EVAL_STRING_HOOK.with(|h| *h.borrow());
        let saved_c = CALL_FUNC_HOOK.with(|h| *h.borrow());
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = Some(eval_hook));
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = Some(call_hook));

        let mut ea = evalarg_T {
            eval_flags: EVAL_EVALUATE,
        };
        // base = 7; base->{x, y -> x}(2, 3) → call with [7, 2, 3] → 12.
        let mut rv = typval_T::from(7);
        let mut arg: &str = "->{x, y -> x}(2, 3) tail";
        assert_eq!(eval_lambda(&mut arg, &mut rv, Some(&mut ea), true), OK);
        assert!(matches!(rv.vval, v_number(12)), "got {:?}", rv.vval);
        assert_eq!(arg, " tail");

        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = saved_e);
        CALL_FUNC_HOOK.with(|h| *h.borrow_mut() = saved_c);
    }

    #[test]
    fn tv_to_argv_string_uses_shell() {
        use super::tv_to_argv;
        // A String uses shell semantics: sh -c <cmd>.
        let cmd = typval_T::from("echo hi".to_string());
        let mut name = String::new();
        let argv = tv_to_argv(&cmd, Some(&mut name), None).expect("argv");
        assert_eq!(
            argv,
            vec!["sh".to_string(), "-c".to_string(), "echo hi".to_string()]
        );
        assert_eq!(name, "echo hi");
    }

    #[cfg(unix)]
    #[test]
    fn tv_to_argv_list_resolves_argv0() {
        use super::tv_to_argv;
        use crate::ported::eval::typval::{tv_list_alloc, tv_list_append_string};
        let l = tv_list_alloc(-1);
        {
            let mut lb = l.borrow_mut();
            tv_list_append_string(&mut lb, "/bin/echo");
            tv_list_append_string(&mut lb, "hi");
        }
        let cmd = typval_T {
            v_type: VAR_LIST,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_list(Some(l)),
        };
        let mut exe = true;
        let argv = tv_to_argv(&cmd, None, Some(&mut exe)).expect("argv");
        assert!(exe);
        assert_eq!(argv[0], "/bin/echo"); // path-containing arg0 resolves to itself
        assert_eq!(argv[1], "hi");
    }

    #[cfg(unix)]
    #[test]
    fn get_system_output_as_rettv_captures_stdout() {
        use super::get_system_output_as_rettv;
        // system("printf hi") → "hi" (no trailing newline from printf).
        let argvars = vec![typval_T::from("printf hi".to_string())];
        let mut rv = typval_T::default();
        get_system_output_as_rettv(&argvars, &mut rv, false);
        assert!(
            matches!(&rv.vval, v_string(s) if s == "hi"),
            "got {:?}",
            rv.vval
        );
    }

    // ── position/index leaves: var2fpos / list2fpos / byteidx conversions ──

    use crate::ported::buffer::{buflist_new, curbuf, firstbuf, lastbuf, top_file_num, BLN_LISTED};
    use crate::ported::eval_h::{FAIL, OK};
    use crate::ported::window::{curwin, win_T};
    use std::cell::RefCell;

    /// Reset the buffer + window thread_local roots (Rust reuses test threads).
    fn reset_editor() {
        firstbuf.with(|f| *f.borrow_mut() = None);
        lastbuf.with(|l| *l.borrow_mut() = None);
        curbuf.with(|c| *c.borrow_mut() = None);
        top_file_num.with(|t| t.set(1));
        curwin.with(|c| *c.borrow_mut() = None);
    }

    /// Load `lines` into a buffer as a loaded memline.
    fn load_lines(buf: &Rc<RefCell<crate::ported::buffer::buf_T>>, lines: &[&str]) {
        let mut b = buf.borrow_mut();
        b.b_ml.ml_mfp = true;
        b.b_ml.ml_lines = lines.iter().map(|s| s.to_string()).collect();
        b.b_ml.ml_line_count = lines.len() as i32;
    }

    /// Build a window over `buf` with the cursor at `(lnum, col)` and make it
    /// `curwin`.
    fn make_curwin(
        buf: &Rc<RefCell<crate::ported::buffer::buf_T>>,
        lnum: i32,
        col: i32,
    ) -> Rc<RefCell<win_T>> {
        let w = Rc::new(RefCell::new(win_T {
            w_buffer: Some(buf.clone()),
            ..Default::default()
        }));
        w.borrow_mut().w_cursor = super::pos_T {
            lnum,
            col,
            coladd: 0,
        };
        curwin.with(|c| *c.borrow_mut() = Some(w.clone()));
        w
    }

    #[test]
    fn byteidx_charidx_conversions_utf8() {
        use super::{buf_byteidx_to_charidx, buf_charidx_to_byteidx};
        reset_editor();
        // "héllo" = h(1) é(2) l l o → 6 bytes, 5 code points.
        let buf = buflist_new(Some("/tmp/mb".into()), None, 0, BLN_LISTED).unwrap();
        load_lines(&buf, &["héllo"]);

        // byte 3 (start of first 'l') → char index 2.
        assert_eq!(buf_byteidx_to_charidx(Some(&buf), 1, 3), 2);
        // char index 3 → byte offset 3 (per the C --charidx>0 walk).
        assert_eq!(buf_charidx_to_byteidx(Some(&buf), 1, 3), 3);

        // NULL buffer / unloaded → -1.
        assert_eq!(buf_byteidx_to_charidx(None, 1, 0), -1);
        assert_eq!(buf_charidx_to_byteidx(None, 1, 0), -1);

        // Empty line → 0.
        let buf2 = buflist_new(Some("/tmp/empty".into()), None, 0, BLN_LISTED).unwrap();
        load_lines(&buf2, &[""]);
        assert_eq!(buf_byteidx_to_charidx(Some(&buf2), 1, 5), 0);
    }

    #[test]
    fn var2fpos_list_and_names() {
        use super::var2fpos;
        use crate::ported::eval::typval::{tv_list_alloc, tv_list_append_number};
        reset_editor();
        let buf = buflist_new(Some("/tmp/vf".into()), None, 0, BLN_LISTED).unwrap();
        load_lines(&buf, &["hello", "world"]);
        let wp = make_curwin(&buf, 2, 3);
        let mut fnum = 0;

        // [lnum, col] list: col is 1-based on input, decremented on output.
        let l = tv_list_alloc(-1);
        {
            let mut lb = l.borrow_mut();
            tv_list_append_number(&mut lb, 1);
            tv_list_append_number(&mut lb, 2);
        }
        let tv = typval_T {
            v_type: VAR_LIST,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_list(Some(l)),
        };
        let p = var2fpos(&tv, false, &mut fnum, false, &wp).expect("list pos");
        assert_eq!(p.lnum, 1);
        assert_eq!(p.col, 1);

        // "." → cursor position (2, 3).
        let dot = typval_T::from(".".to_string());
        let p = var2fpos(&dot, false, &mut fnum, false, &wp).expect("cursor");
        assert_eq!(p.lnum, 2);
        assert_eq!(p.col, 3);

        // "$" with dollar_lnum → last line, col 0.
        let dollar = typval_T::from("$".to_string());
        let p = var2fpos(&dollar, true, &mut fnum, false, &wp).expect("last line");
        assert_eq!(p.lnum, 2);
        assert_eq!(p.col, 0);

        // "$" without dollar_lnum → cursor line, last byte column.
        let p = var2fpos(&dollar, false, &mut fnum, false, &wp).expect("last col");
        assert_eq!(p.lnum, 2);
        assert_eq!(p.col, 5); // len("world")

        // deferred named-mark case falls through to None.
        let mark = typval_T::from("'a".to_string());
        assert!(var2fpos(&mark, false, &mut fnum, false, &wp).is_none());
    }

    #[test]
    fn list2fpos_with_and_without_fnum() {
        use super::list2fpos;
        use crate::ported::eval::typval::{tv_list_alloc, tv_list_append_number};
        reset_editor();
        let buf = buflist_new(Some("/tmp/l2".into()), None, 0, BLN_LISTED).unwrap();
        load_lines(&buf, &["a", "b"]);
        curbuf.with(|c| *c.borrow_mut() = Some(buf.clone()));

        // No fnum: [lnum, col, off].
        let l = tv_list_alloc(-1);
        {
            let mut lb = l.borrow_mut();
            tv_list_append_number(&mut lb, 1);
            tv_list_append_number(&mut lb, 2);
            tv_list_append_number(&mut lb, 3);
        }
        let arg = typval_T {
            v_type: VAR_LIST,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_list(Some(l)),
        };
        let mut posp = super::pos_T::default();
        assert_eq!(list2fpos(&arg, &mut posp, None, None, false), OK);
        assert_eq!(posp.lnum, 1);
        assert_eq!(posp.col, 2);
        assert_eq!(posp.coladd, 3);

        // With fnum 0 → resolves to curbuf handle.
        let l2 = tv_list_alloc(-1);
        {
            let mut lb = l2.borrow_mut();
            tv_list_append_number(&mut lb, 0);
            tv_list_append_number(&mut lb, 5);
            tv_list_append_number(&mut lb, 2);
        }
        let arg2 = typval_T {
            v_type: VAR_LIST,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_list(Some(l2)),
        };
        let mut posp2 = super::pos_T::default();
        let mut fnum = -1;
        assert_eq!(
            list2fpos(&arg2, &mut posp2, Some(&mut fnum), None, false),
            OK
        );
        let curbuf_handle = curbuf
            .with(|c| c.borrow().clone())
            .map(|b| b.borrow().handle)
            .unwrap();
        assert_eq!(fnum, curbuf_handle);
        assert_eq!(posp2.lnum, 5);
        assert_eq!(posp2.col, 2);

        // Too-short list → FAIL.
        let short = tv_list_alloc(-1);
        tv_list_append_number(&mut short.borrow_mut(), 1);
        let args = typval_T {
            v_type: VAR_LIST,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_list(Some(short)),
        };
        let mut p3 = super::pos_T::default();
        assert_eq!(list2fpos(&args, &mut p3, None, None, false), FAIL);
    }

    #[test]
    fn eval_for_line_list_and_errors() {
        use super::{eval_for_line, evalarg_T};
        use crate::ported::eval::typval::EVAL_STRING_HOOK;
        // Install the eval0 sub-expression hook (task convention): number
        // literals resolve to themselves.
        fn hook(expr: &str) -> Option<typval_T> {
            expr.trim().parse::<i64>().ok().map(typval_T::from)
        }
        let saved = EVAL_STRING_HOOK.with(|h| *h.borrow());
        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = Some(hook));

        // `for x in [1, 2, 3]` → fi_list with 3 items, errp cleared.
        let mut errp = true;
        let mut ea = evalarg_T {
            eval_flags: super::EVAL_EVALUATE,
        };
        let fi = eval_for_line("x in [1, 2, 3]", &mut errp, &mut ea);
        assert!(!errp);
        assert_eq!(fi.fi_varcount, 1);
        assert_eq!(fi.fi_semicolon, 0);
        let l = fi.fi_list.expect("fi_list");
        assert_eq!(crate::ported::eval::typval::tv_list_len(&l.borrow()), 3);

        // Missing "in" → E690, errp stays true, no list.
        let mut errp2 = true;
        let mut ea2 = evalarg_T {
            eval_flags: super::EVAL_EVALUATE,
        };
        let fi2 = eval_for_line("x [1]", &mut errp2, &mut ea2);
        assert!(errp2);
        assert!(fi2.fi_list.is_none());

        // String source → fi_string set.
        let mut errp3 = true;
        let mut ea3 = evalarg_T {
            eval_flags: super::EVAL_EVALUATE,
        };
        let fi3 = eval_for_line("c in \"abc\"", &mut errp3, &mut ea3);
        assert!(!errp3);
        assert_eq!(fi3.fi_string.as_deref(), Some("abc"));

        EVAL_STRING_HOOK.with(|h| *h.borrow_mut() = saved);
    }
}
