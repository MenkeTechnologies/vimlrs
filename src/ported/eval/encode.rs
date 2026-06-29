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
    typval_T, typval_vval_union::*, BoolVarValue::*, SpecialVarValue::*, VarType::*,
};

/// Render a finite float the way Vim does: C `printf("%g", f)` with `prec`
/// significant digits (default 6, trailing zeros stripped), choosing `%f` vs
/// `%e` form by the decimal exponent. The caller appends `.0` when there is no
/// `.`/`e`, so `1.0` prints as `1.0` and `0.1 + 0.2` prints as `0.3`.
pub(crate) fn vim_float_g(f: f64, prec: i32) -> String {
    if f == 0.0 {
        // c: printf("%g", …) keeps the sign of IEEE negative zero ("-0").
        return if f.is_sign_negative() { "-0" } else { "0" }.to_string();
    }
    let p = prec.max(1);
    let strip = |s: &str| -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s.to_string()
        }
    };
    // True base-10 exponent from %e form (before rounding).
    let e_str = format!("{f:e}");
    let exp: i32 = e_str[e_str.find('e').unwrap() + 1..].parse().unwrap_or(0);
    if exp < -4 || exp >= p {
        // %e form: P-1 fractional digits, C-style "e±NN" exponent.
        let m = format!("{:.*e}", (p - 1) as usize, f);
        let epos = m.find('e').unwrap();
        let mant = strip(&m[..epos]);
        let exn: i32 = m[epos + 1..].parse().unwrap_or(0);
        format!("{mant}e{}{:02}", if exn < 0 { '-' } else { '+' }, exn.abs())
    } else {
        let dec = (p - 1 - exp).max(0) as usize;
        strip(&format!("{f:.dec$}"))
    }
}

/// Port of `encode_blob_write()` from `Src/eval/encode.c:48`.
///
/// Append the raw bytes `buf` to blob `blob`, returning the number written
/// (used as the readfile/channel-output sink for Blob mode).
pub fn encode_blob_write(blob: &mut crate::ported::eval::typval_defs_h::blob_T, buf: &[u8]) -> i32 {
    blob.bv_ga.extend_from_slice(buf);
    buf.len() as i32
}

/// Port of `encode_vim_list_to_buf()` from `Src/eval/encode.c:213`.
///
/// Serialize a List of strings to the `writefile()` byte form: items joined by
/// `NL`, with each item's embedded `NL` mapped to `NUL`. Returns `None` (the C
/// `false`) if any item is not a String.
pub fn encode_vim_list_to_buf(
    list: &crate::ported::eval::typval_defs_h::list_T,
) -> Option<String> {
    let mut parts: Vec<String> = Vec::with_capacity(list.lv_items.len());
    for it in &list.lv_items {
        if it.li_tv.v_type != VAR_STRING {
            return None;
        }
        match &it.li_tv.vval {
            v_string(s) => parts.push(s.replace('\n', "\0")),
            _ => parts.push(String::new()),
        }
    }
    Some(parts.join("\n"))
}

/// Port of `ListReaderState` (`Src/eval/encode.h:28`) — position state for
/// reading a List's joined byte stream. RUST-PORT NOTE: the C holds the `list`
/// and a `listitem_T *li` pointer; here `li` is an item index and the list is
/// passed to [`encode_read_from_list`].
#[derive(Debug, Clone, Copy)]
pub struct ListReaderState {
    /// Index of the item currently being read.
    pub li: usize,
    /// Byte offset inside the current item's string.
    pub offset: usize,
    /// Byte length of the current item's string.
    pub li_length: usize,
}

/// Port of `encode_init_lrstate()` from `Src/eval/encode.c:1053`.
///
/// Initialize a [`ListReaderState`] at the start of `list`.
pub fn encode_init_lrstate(list: &crate::ported::eval::typval_defs_h::list_T) -> ListReaderState {
    let li_length = list.lv_items.first().map_or(0, |it| match &it.li_tv.vval {
        v_string(s) => s.len(),
        _ => 0,
    });
    ListReaderState { li: 0, offset: 0, li_length }
}

/// Port of `encode_read_from_list()` from `Src/eval/encode.c:257`.
///
/// Read up to `buf.len()` bytes of `list`'s joined byte form into `buf` (items
/// separated by `NL`, embedded `NL` → `NUL`), advancing `state`. Returns
/// `(status, read_bytes)` where status is [`OK`](crate::ported::eval_h::OK)
/// (finished), `2` (NOTDONE — more remains), or
/// [`FAIL`](crate::ported::eval_h::FAIL) (a non-String item).
pub fn encode_read_from_list(
    state: &mut ListReaderState,
    list: &crate::ported::eval::typval_defs_h::list_T,
    buf: &mut [u8],
) -> (i32, usize) {
    use crate::ported::eval_h::{FAIL, OK};
    const NOTDONE: i32 = 2; // c: Src/macros_defs.h
    let nbuf = buf.len();
    let mut p = 0;
    while p < nbuf {
        if let Some(bytes) = list.lv_items.get(state.li).and_then(|it| match &it.li_tv.vval {
            v_string(s) => Some(s.as_bytes()),
            _ => None,
        }) {
            while state.offset < state.li_length && p < nbuf {
                let ch = bytes[state.offset];
                state.offset += 1;
                buf[p] = if ch == b'\n' { 0 } else { ch };
                p += 1;
            }
        }
        if p < nbuf {
            state.li += 1;
            if state.li >= list.lv_items.len() {
                return (OK, p);
            }
            buf[p] = b'\n';
            p += 1;
            match list.lv_items.get(state.li).map(|it| &it.li_tv) {
                Some(tv) if tv.v_type == VAR_STRING => {
                    state.offset = 0;
                    state.li_length = match &tv.vval {
                        v_string(s) => s.len(),
                        _ => 0,
                    };
                }
                _ => return (FAIL, p),
            }
        }
    }
    let more = state.offset < state.li_length || state.li + 1 < list.lv_items.len();
    (if more { NOTDONE } else { OK }, nbuf)
}

/// Port of `encode_list_write()` from `Src/eval/encode.c:56`.
///
/// Append the lines of `buf` to `list`, splitting on `NL` and mapping embedded
/// `NUL` → `NL` (the `readfile()`/channel-output representation). The first
/// line continues the list's last item (so streamed chunks join), and a buffer
/// ending in `NL` yields a trailing empty item. RUST-PORT NOTE: the C's NULL
/// (never-set) string item is an empty string here.
pub fn encode_list_write(list: &mut crate::ported::eval::typval_defs_h::list_T, buf: &str) {
    use crate::ported::eval::typval::tv_list_append_string;
    use crate::ported::eval::typval_defs_h::typval_vval_union::v_string;
    if buf.is_empty() {
        return;
    }
    let mut segments = buf.split('\n');
    // Continue the last existing list item with the first (partial) line.
    if !list.lv_items.is_empty() {
        if let Some(first) = segments.next() {
            let chunk = first.replace('\0', "\n");
            if let v_string(s) = &mut list.lv_items.last_mut().unwrap().li_tv.vval {
                s.push_str(&chunk);
            }
        }
    }
    for seg in segments {
        tv_list_append_string(list, &seg.replace('\0', "\n"));
    }
}

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
        // TYPVAL_ENCODE_CONV_FLOAT — "%g", then append ".0" if no '.'/'e' (so
        // string(3.0) is "3.0", not "3"). RUST-PORT NOTE: `{f}` stands in for
        // printf "%g".
        (VAR_FLOAT, v_float(f)) => {
            if f.is_infinite() {
                if *f < 0.0 { "-inf" } else { "inf" }.to_string()
            } else if f.is_nan() {
                "nan".to_string()
            } else {
                let s = vim_float_g(*f, 6);
                if s.contains(['.', 'e', 'E']) {
                    s
                } else {
                    format!("{s}.0")
                }
            }
        }
        // TYPVAL_ENCODE_CONV_STRING — single-quoted, embedded quotes doubled.
        (VAR_STRING, v_string(s)) => format!("'{}'", s.replace('\'', "''")),
        // TYPVAL_ENCODE_CONV_FUNC_START — function('name').
        (VAR_FUNC, v_string(s)) => format!("function('{}')", s.replace('\'', "''")),
        // A Partial — function('name'[, [args]]).
        (VAR_PARTIAL, v_partial(Some(p))) => {
            let name = p.pt_name.replace('\'', "''");
            if p.pt_argv.is_empty() {
                format!("function('{name}')")
            } else {
                let args: Vec<String> = p.pt_argv.iter().map(encode_tv2string).collect();
                format!("function('{name}', [{}])", args.join(", "))
            }
        }
        (VAR_BOOL, v_bool(b)) => if *b == kBoolVarTrue {
            "v:true"
        } else {
            "v:false"
        }
        .to_string(),
        (VAR_SPECIAL, v_special(kSpecialVarNone)) => "v:none".to_string(),
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
                    out.push_str(&format!("'{}'", k.replace('\'', "''")));
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

/// Port of `encode_tv2json()` from `Src/eval/encode.c:921` — the `json_encode()`
/// rendering of a value.
pub fn encode_tv2json(tv: &typval_T) -> String {
    encode_vim_to_json(tv)
}

/// Port of `convert_to_json_string()` from `Src/eval/encode.c:621` — a
/// double-quoted, JSON-escaped string.
fn convert_to_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Port of the `encode_vim_to_json` instantiation of the encode template — JSON
/// render. Strings/keys are double-quoted+escaped, `v:true`/`v:false`/`v:null`
/// become `true`/`false`/`null`.
pub fn encode_vim_to_json(tv: &typval_T) -> String {
    match (tv.v_type, &tv.vval) {
        (VAR_NUMBER, v_number(n)) => n.to_string(),
        (VAR_FLOAT, v_float(f)) => {
            if f.is_finite() {
                let s = vim_float_g(*f, 6);
                if s.contains(['.', 'e', 'E']) {
                    s
                } else {
                    format!("{s}.0")
                }
            } else {
                "null".to_string() // JSON has no NaN/Inf
            }
        }
        (VAR_STRING, v_string(s)) => convert_to_json_string(s),
        (VAR_BOOL, v_bool(b)) => if *b == kBoolVarTrue { "true" } else { "false" }.to_string(),
        (VAR_SPECIAL, _) => "null".to_string(),
        (VAR_LIST, v_list(l)) => match l {
            None => "[]".to_string(),
            Some(l) => {
                let l = l.borrow();
                let mut out = String::from("[");
                for (i, it) in l.lv_items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&encode_vim_to_json(&it.li_tv));
                }
                out.push(']');
                out
            }
        },
        (VAR_DICT, v_dict(d)) => match d {
            None => "{}".to_string(),
            Some(d) => {
                let d = d.borrow();
                let mut out = String::from("{");
                for (i, (k, v)) in d.dv_hashtab.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&convert_to_json_string(k));
                    out.push(':');
                    out.push_str(&encode_vim_to_json(v));
                }
                out.push('}');
                out
            }
        },
        _ => "null".to_string(),
    }
}

#[cfg(test)]
mod encode_io_tests {
    use super::{encode_blob_write, encode_vim_list_to_buf};
    use crate::ported::eval::typval::{tv_list_append_number, tv_list_append_string};
    use crate::ported::eval::typval_defs_h::{blob_T, list_T};

    #[test]
    fn blob_write_appends_bytes() {
        let mut b = blob_T::default();
        assert_eq!(encode_blob_write(&mut b, &[1, 2, 3]), 3);
        assert_eq!(encode_blob_write(&mut b, &[4]), 1);
        assert_eq!(b.bv_ga, vec![1, 2, 3, 4]);
    }

    #[test]
    fn read_from_list_matches_to_buf() {
        use super::{encode_init_lrstate, encode_read_from_list, encode_vim_list_to_buf};
        use crate::ported::eval_h::OK;
        let mut l = list_T::default();
        tv_list_append_string(&mut l, "a");
        tv_list_append_string(&mut l, "x\ny"); // embedded NL
        tv_list_append_string(&mut l, "b");
        let expected = encode_vim_list_to_buf(&l).unwrap();
        // full read into a big buffer reproduces encode_vim_list_to_buf exactly
        let mut st = encode_init_lrstate(&l);
        let mut buf = vec![0u8; 64];
        let (status, n) = encode_read_from_list(&mut st, &l, &mut buf);
        assert_eq!(status, OK);
        assert_eq!(&buf[..n], expected.as_bytes());
        // a too-small buffer reports NOTDONE (2)
        let mut st2 = encode_init_lrstate(&l);
        let mut small = vec![0u8; 2];
        let (status2, n2) = encode_read_from_list(&mut st2, &l, &mut small);
        assert_eq!(status2, 2);
        assert_eq!(n2, 2);
    }

    #[test]
    fn vim_list_to_buf_joins() {
        let mut l = list_T::default();
        tv_list_append_string(&mut l, "a");
        tv_list_append_string(&mut l, "b");
        assert_eq!(encode_vim_list_to_buf(&l).as_deref(), Some("a\nb"));
        // embedded NL within an item → NUL
        let mut l2 = list_T::default();
        tv_list_append_string(&mut l2, "x\ny");
        assert_eq!(encode_vim_list_to_buf(&l2), Some("x\0y".to_string()));
        // a non-string item → None
        let mut l3 = list_T::default();
        tv_list_append_string(&mut l3, "ok");
        tv_list_append_number(&mut l3, 7);
        assert_eq!(encode_vim_list_to_buf(&l3), None);
    }
}
