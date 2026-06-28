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

/// Port of `next_for_item()` from `Src/eval.c` — advance a `:for` iterator; the
/// bridge drives `:for`, so this path is unused → false (stop).
pub fn next_for_item() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::string2float;

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
