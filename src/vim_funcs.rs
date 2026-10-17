//! EXTENSION — builtins that exist in Vim 9.x but not in Neovim, so they have no
//! counterpart under `vendor/` and live outside `src/ported/`.
//!
//! Each function is a port of the Vim C named in its doc comment
//! (`src/strings.c`, `src/misc2.c`, `src/evalfunc.c`), reduced to
//! `(argvars, rettv)` like the Neovim-derived `f_*` functions.
//!
//! Not ported: the wide `blob2str()`/`str2blob()` encodings (UTF-16, UCS-2,
//! UTF-32, UCS-4), whose C path converts a buffer containing NUL bytes and
//! needs a length-aware `string_convert()`; they report `E1515`/`E1516`.

use crate::ported::eval::typval::{
    tv_blob_alloc_ret, tv_check_for_blob_arg, tv_check_for_list_arg, tv_check_for_opt_dict_arg,
    tv_check_for_string_arg, tv_dict_get_string, tv_get_string_buf_chk, tv_list_alloc_ret,
    tv_list_append_string,
};
use crate::ported::eval::typval_defs_h::{typval_T, typval_vval_union::*, VarType::*};
use crate::ported::eval_h::FAIL;
use crate::ported::mbyte::{
    convert_setup_ext, enc_canon_props, enc_canonize, enc_skip, string_convert_ext, ConvType,
    ENC_2BYTE, ENC_2WORD, ENC_4BYTE,
};
use crate::ported::message::semsg;
use crate::vimstr::VimStr;

const NL: u8 = b'\n';
const BASE64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Port of `base64_encode()` (`src/misc2.c`).
fn base64_encode(src: &[u8]) -> Vec<u8> {
    let mut dst = Vec::with_capacity(src.len().div_ceil(3) * 4);
    for chunk in src.chunks(3) {
        let a = chunk[0] as u32;
        let b = chunk.get(1).copied().unwrap_or(0) as u32;
        let c = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (a << 16) | (b << 8) | c;
        dst.push(BASE64_TABLE[(triple >> 18) as usize & 0x3f]);
        dst.push(BASE64_TABLE[(triple >> 12) as usize & 0x3f]);
        dst.push(if chunk.len() > 1 {
            BASE64_TABLE[(triple >> 6) as usize & 0x3f]
        } else {
            b'='
        });
        dst.push(if chunk.len() > 2 {
            BASE64_TABLE[triple as usize & 0x3f]
        } else {
            b'='
        });
    }
    dst
}

/// `base64_dec_table[]` of `src/misc2.c`: 0xFF for a byte outside the alphabet,
/// and `=` decodes as 0.
fn base64_dec(c: u8) -> u32 {
    if c == b'=' {
        return 0;
    }
    BASE64_TABLE
        .iter()
        .position(|&t| t == c)
        .map_or(0xff, |i| i as u32)
}

/// Port of `base64_decode()` (`src/misc2.c`). `None` is the C's FAIL (`E475`
/// already reported). Padding is only trimmed from the end of the whole input,
/// so `=` inside the text decodes as zero bits, exactly as in the C.
fn base64_decode(data: &[u8]) -> Option<Vec<u8>> {
    let len = data.len();
    let mut out = Vec::new();
    if len == 0 {
        return Some(out);
    }
    let invalid = || {
        semsg(&format!(
            "E475: Invalid argument: {}",
            String::from_utf8_lossy(data)
        ));
        None
    };
    if len % 4 != 0 {
        return invalid();
    }
    let mut decoded_len = (len / 4) * 3;
    if data[len - 1] == b'=' {
        decoded_len -= 1;
    }
    if data[len - 2] == b'=' {
        decoded_len -= 1;
    }
    let mut j = 0;
    for q in data.chunks(4) {
        let s: Vec<u32> = q.iter().map(|&c| base64_dec(c)).collect();
        if s.contains(&0xff) {
            return invalid();
        }
        let triple = (s[0] << 18) | (s[1] << 12) | (s[2] << 6) | s[3];
        for shift in [16, 8, 0] {
            if j < decoded_len {
                out.push((triple >> shift) as u8);
                j += 1;
            }
        }
        if j == decoded_len
            && ((data[len - 2] == b'=' && s[1] & 0xf != 0)
                || (data[len - 1] == b'=' && s[2] & 0x3 != 0))
        {
            return invalid();
        }
    }
    Some(out)
}

/// Port of `f_base64_encode()` (`src/evalfunc.c`).
pub fn f_base64_encode(argvars: &[typval_T], rettv: &mut typval_T) {
    if tv_check_for_blob_arg(argvars, 0) == FAIL {
        return;
    }
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(VimStr::new());
    if let v_blob(Some(b)) = &argvars[0].vval {
        rettv.vval = v_string(base64_encode(&b.borrow().bv_ga).into());
    }
}

/// Port of `f_base64_decode()` (`src/evalfunc.c`).
pub fn f_base64_decode(argvars: &[typval_T], rettv: &mut typval_T) {
    if tv_check_for_string_arg(argvars, 0) == FAIL {
        return;
    }
    let blob = tv_blob_alloc_ret(rettv);
    if let Some(s) = tv_get_string_buf_chk(&argvars[0]) {
        if let Some(bytes) = base64_decode(s.as_bytes()) {
            blob.borrow_mut().bv_ga = bytes;
        }
    }
}

/// Port of `uri_encode()` (`src/strings.c`): everything but ASCII
/// alphanumerics and `-_.~` becomes `%XX`.
fn uri_encode(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for &c in s {
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~') {
            out.push(c);
        } else {
            out.extend_from_slice(format!("%{c:02X}").as_bytes());
        }
    }
    out
}

/// Port of `uri_decode()` (`src/strings.c`). A `%` with fewer than two bytes
/// after it keeps the `%` and the next byte; a `%` followed by a non-hex pair
/// is copied through with its two bytes.
fn uri_decode(s: &[u8]) -> Vec<u8> {
    let hex = |b: u8| (b as char).to_digit(16);
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] != b'%' {
            out.push(s[i]);
            i += 1;
        } else if i + 2 >= s.len() {
            out.push(s[i]);
            i += 1;
            if i < s.len() {
                out.push(s[i]);
                i += 1;
            }
        } else if let (Some(h), Some(l)) = (hex(s[i + 1]), hex(s[i + 2])) {
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.extend_from_slice(&s[i..i + 3]);
            i += 3;
        }
    }
    out
}

/// Port of `f_uriencode()` (`src/strings.c`).
pub fn f_uriencode(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(VimStr::new());
    if tv_check_for_string_arg(argvars, 0) == FAIL {
        return;
    }
    if let Some(s) = tv_get_string_buf_chk(&argvars[0]) {
        rettv.vval = v_string(uri_encode(s.as_bytes()).into());
    }
}

/// Port of `f_uridecode()` (`src/strings.c`).
pub fn f_uridecode(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(VimStr::new());
    if tv_check_for_string_arg(argvars, 0) == FAIL {
        return;
    }
    if let Some(s) = tv_get_string_buf_chk(&argvars[0]) {
        rettv.vval = v_string(uri_decode(s.as_bytes()).into());
    }
}

/// Port of `normalize_encoding_name()` (`src/strings.c`): lower case, `_` to
/// `-`, and a hyphen after `ucs`/`utf` when a digit follows (`utf16le` to
/// `utf-16le`).
fn normalize_encoding_name(enc: &str) -> String {
    let mut p: Vec<u8> = enc
        .bytes()
        .map(|c| if c == b'_' { b'-' } else { c.to_ascii_lowercase() })
        .collect();
    let digit_after_prefix = (p.starts_with(b"ucs") || p.starts_with(b"utf"))
        && p.get(3).is_some_and(u8::is_ascii_digit)
        && p.get(4) != Some(&b'-');
    if digit_after_prefix {
        p.insert(3, b'-');
    }
    String::from_utf8_lossy(&p).into_owned()
}

/// The `{options}` Dict's `encoding` as `(canonical, iconv-normalized)`.
fn encoding_option(arg: Option<&typval_T>) -> Option<(String, String)> {
    let Some(typval_T {
        vval: v_dict(Some(d)),
        ..
    }) = arg
    else {
        return None;
    };
    let enc = tv_dict_get_string(&d.borrow(), "encoding")?;
    let skipped = enc_skip(&enc);
    Some((enc_canonize(skipped), normalize_encoding_name(skipped)))
}

/// Port of `convert_string()` (`src/strings.c`) with `vc_fail` set; `None` is
/// the C's FAIL.
fn convert_string(s: &[u8], from: &str, to: &str) -> Option<Vec<u8>> {
    let mut vc = convert_setup_ext(&enc_canonize(from), &enc_canonize(to))?;
    vc.vc_fail = true;
    if vc.vc_type == ConvType::CONV_NONE {
        return Some(s.to_vec());
    }
    string_convert_ext(&vc, s)
}

fn is_wide_encoding(enc: &str) -> bool {
    enc_canon_props(enc) & (ENC_2BYTE | ENC_4BYTE | ENC_2WORD) != 0
}

/// Port of `string_from_blob()` (`src/strings.c`): the bytes up to the next NL,
/// a NUL byte standing for a NL character. Returns the string and the index of
/// the next byte to process.
fn string_from_blob(blob: &[u8], start: usize) -> (Vec<u8>, usize) {
    let rest = &blob[start..];
    let nl = rest.iter().position(|&b| b == NL);
    let line = &rest[..nl.unwrap_or(rest.len())];
    let s = line.iter().map(|&b| if b == 0 { NL } else { b }).collect();
    (s, start + line.len() + usize::from(nl.is_some()))
}

/// Port of `f_blob2str()` (`src/strings.c`): a List of the lines of a Blob,
/// converted from `{options}.encoding` to UTF-8 (`'encoding'` is always UTF-8
/// here, so every result is validated unless the encoding is `none`).
pub fn f_blob2str(argvars: &[typval_T], rettv: &mut typval_T) {
    if tv_check_for_blob_arg(argvars, 0) == FAIL || tv_check_for_opt_dict_arg(argvars, 1) == FAIL {
        return;
    }
    let list = tv_list_alloc_ret(rettv, 0);
    let v_blob(Some(blob)) = &argvars[0].vval else {
        return;
    };
    let blob = blob.borrow().bv_ga.clone();
    let mut validate_utf8 = true;
    let mut from = encoding_option(argvars.get(1));
    if from.as_ref().is_some_and(|(canon, _)| canon == "none") {
        validate_utf8 = false;
        from = None;
    }
    if let Some((canon, _)) = &from {
        if is_wide_encoding(canon) {
            semsg(&format!("E1515: Unable to convert from '{canon}' encoding"));
            return;
        }
    }
    let mut lb = list.borrow_mut();
    let mut idx = 0;
    while idx < blob.len() {
        let (mut line, next) = string_from_blob(&blob, idx);
        idx = next;
        if let Some((canon, raw)) = &from {
            match convert_string(&line, raw, "utf-8") {
                Some(c) => line = c,
                None => {
                    semsg(&format!("E1515: Unable to convert from '{canon}' encoding"));
                    return;
                }
            }
        }
        if validate_utf8 && std::str::from_utf8(&line).is_err() {
            semsg("E1515: Unable to convert from 'utf-8' encoding");
            return;
        }
        tv_list_append_string(&mut lb, VimStr::from(line));
    }
    // c: a Blob ending in a newline gets one more empty string.
    if blob.last() == Some(&NL) {
        tv_list_append_string(&mut lb, "");
    }
}

/// Port of `f_str2blob()` (`src/strings.c`): the String items of a List joined
/// by NL (NL inside an item becomes NUL), converted to `{options}.encoding`.
/// Non-String items are skipped.
pub fn f_str2blob(argvars: &[typval_T], rettv: &mut typval_T) {
    if tv_check_for_list_arg(argvars, 0) == FAIL || tv_check_for_opt_dict_arg(argvars, 1) == FAIL {
        return;
    }
    let blob = tv_blob_alloc_ret(rettv);
    let v_list(Some(list)) = &argvars[0].vval else {
        return;
    };
    let to = encoding_option(argvars.get(1));
    if let Some((canon, _)) = &to {
        if is_wide_encoding(canon) {
            semsg(&format!("E1516: Unable to convert to '{canon}' encoding"));
            return;
        }
    }
    let mut out = blob.borrow_mut();
    for (n, li) in list.borrow().lv_items.iter().enumerate() {
        let v_string(s) = &li.li_tv.vval else {
            continue;
        };
        if li.li_tv.v_type != VAR_STRING {
            continue;
        }
        let mut bytes = s.as_bytes().to_vec();
        if let Some((canon, raw)) = &to {
            match convert_string(&bytes, "utf-8", raw) {
                Some(c) => bytes = c,
                None => {
                    semsg(&format!("E1516: Unable to convert to '{canon}' encoding"));
                    return;
                }
            }
        }
        if n > 0 {
            out.bv_ga.push(NL);
        }
        out.bv_ga
            .extend(bytes.iter().map(|&b| if b == NL { 0 } else { b }));
    }
}
