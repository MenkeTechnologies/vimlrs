//! Port of `src/nvim/eval/encode.c` (vendored at `csrc/eval/encode.c`) — the
//! `string()` / `:echo` value-rendering entry points and the recursive
//! converter the `typval_encode.c.h` macro template generates.
//!
//! RUST-PORT NOTE: C generates `encode_vim_to_string`/`encode_vim_to_echo` by
//! instantiating the `typval_encode.c.h` template twice. The two instantiations
//! render identically for nested values (both quote nested strings); they differ
//! only at the outermost string, which the `encode_tv2*` wrappers handle. The
//! recursive walk is ported once as `encode_vim_to_string`; `encode_vim_to_echo`
//! delegates to it (the bodies the macro emits are equivalent).
#![allow(non_snake_case)]

use crate::ported::eval::typval_defs_h::{
    typval_T, typval_vval_union::*, BoolVarValue::*, VarType::*,
};

/// Port of `encode_tv2string()` from `Src/eval/encode.c:869`.
///
/// String representation of a value with quotes around strings (parseable back
/// by `eval()`). This is `string()`.
pub fn encode_tv2string(tv: &typval_T) -> String {
    // c: encode_vim_to_string(&ga, tv, ...)
    encode_vim_to_string(tv)
}

/// Port of `encode_tv2echo()` from `Src/eval/encode.c:893`.
///
/// String representation without quotes around the outermost string, as `:echo`
/// displays values.
pub fn encode_tv2echo(tv: &typval_T) -> String {
    // c: if (tv->v_type == VAR_STRING || tv->v_type == VAR_FUNC) { ga_concat(v_string) }
    match (tv.v_type, &tv.vval) {
        (VAR_STRING | VAR_FUNC, v_string(s)) => s.clone(),
        // c: else encode_vim_to_echo(&ga, tv, ...)
        _ => encode_vim_to_echo(tv),
    }
}

/// Port of the `encode_vim_to_string` instantiation of the `typval_encode.c.h`
/// template — recursive render with every string quoted.
pub fn encode_vim_to_string(tv: &typval_T) -> String {
    match (tv.v_type, &tv.vval) {
        // TYPVAL_ENCODE_CONV_NUMBER
        (VAR_NUMBER, v_number(n)) => n.to_string(),
        // TYPVAL_ENCODE_CONV_FLOAT — "%g", then append ".0" if no '.'/'e'.
        (VAR_FLOAT, v_float(f)) => conv_float(*f),
        // TYPVAL_ENCODE_CONV_STRING — single-quoted, embedded quotes doubled.
        (VAR_STRING, v_string(s)) => quote_string(s),
        // TYPVAL_ENCODE_CONV_FUNC_START — function('name').
        (VAR_FUNC, v_string(s)) => format!("function({})", quote_string(s)),
        (VAR_BOOL, v_bool(b)) => {
            if *b == kBoolVarTrue { "v:true" } else { "v:false" }.to_string()
        }
        (VAR_SPECIAL, _) => "v:null".to_string(),
        // TYPVAL_ENCODE_CONV_LIST_START / _BETWEEN_ITEMS / _END
        (VAR_LIST, v_list(l)) => match l {
            None => "[]".to_string(),
            Some(l) => {
                let l = l.borrow();
                let mut out = String::from("[");
                for (i, it) in l.lv_items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&encode_vim_to_string(&it.li_tv));
                }
                out.push(']');
                out
            }
        },
        // TYPVAL_ENCODE_CONV_DICT_START / _KEY / _AFTER_KEY / _BETWEEN_ITEMS / _END
        (VAR_DICT, v_dict(d)) => match d {
            None => "{}".to_string(),
            Some(d) => {
                let d = d.borrow();
                let mut out = String::from("{");
                for (i, (k, v)) in d.dv_hashtab.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&quote_string(k));
                    out.push_str(": ");
                    out.push_str(&encode_vim_to_string(v));
                }
                out.push('}');
                out
            }
        },
        // TYPVAL_ENCODE_CONV_BLOB — 0z followed by hex, grouped in 4-byte runs.
        (VAR_BLOB, v_blob(b)) => match b {
            None => "0z".to_string(),
            Some(b) => {
                let b = b.borrow();
                let mut out = String::from("0z");
                for (i, byte) in b.bv_ga.iter().enumerate() {
                    if i > 0 && i % 4 == 0 {
                        out.push('.');
                    }
                    out.push_str(&format!("{byte:02X}"));
                }
                out
            }
        },
        _ => String::new(),
    }
}

/// Port of the `encode_vim_to_echo` instantiation. Equivalent to
/// [`encode_vim_to_string`] for all nested values (see file-header note).
pub fn encode_vim_to_echo(tv: &typval_T) -> String {
    encode_vim_to_string(tv)
}

/// `TYPVAL_ENCODE_CONV_FLOAT` (`encode.c` / `typval_encode.c.h`): `%g`, with a
/// trailing `.0` when the result has no `.`/`e`/inf/nan, so `string(3.0)` is
/// `3.0` not `3`.
fn conv_float(f: f64) -> String {
    if f.is_infinite() {
        return if f < 0.0 { "-inf" } else { "inf" }.to_string();
    }
    if f.is_nan() {
        return "nan".to_string();
    }
    let s = format!("{f}"); // RUST-PORT NOTE: stands in for printf "%g".
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{s}.0")
    }
}

/// `TYPVAL_ENCODE_CONV_STRING`: wrap in single quotes, doubling embedded single
/// quotes (Vim's literal-string escaping).
fn quote_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push('\'');
        }
        out.push(ch);
    }
    out.push('\'');
    out
}
