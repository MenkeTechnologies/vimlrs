//! Port of `src/nvim/strings.c` — the Vimscript string builtins.
//!
//! These are real Neovim functions whose C home is `strings.c` (not under
//! `eval/`, so not in the vendored `csrc/eval/` tree). Ported from
//! `~/forkedRepos/neovim/src/nvim/strings.c`; their names are recorded in
//! `tests/data/fake_fn_allowlist.txt` as category-A (real C, home file not
//! vendored) until `strings.c` itself is vendored.
#![allow(non_snake_case)]

use crate::ported::charset::{vim_str2nr, STR2NR_ALL};
use crate::ported::eval::encode::encode_tv2string;
use crate::ported::eval::typval::{
    tv_get_number_chk, tv_get_string, tv_list_alloc_ret, tv_list_append_number,
};
use crate::ported::eval::typval_defs_h::{
    typval_T, typval_vval_union::*, varnumber_T, VarType::*,
};

/// "string(expr)" function — the `string()` rendering of `expr`.
pub fn f_string(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: rettv->v_type = VAR_STRING; rettv->vval.v_string = encode_tv2string(...);
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(encode_tv2string(&argvars[0]));
}

/// "str2nr()" function — parse the leading number in a string.
pub fn f_str2nr(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let mut n: varnumber_T = 0;
    vim_str2nr(&s, None, None, STR2NR_ALL, Some(&mut n), None, 0, false, None);
    rettv.vval = v_number(n);
}

/// Port of `f_strlen()` from `Src/strings.c` — byte length of a string.
pub fn f_strlen(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(tv_get_string(&argvars[0]).len() as varnumber_T);
}

/// Port of `f_tolower()` from `Src/strings.c`.
pub fn f_tolower(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(tv_get_string(&argvars[0]).to_lowercase());
}

/// Port of `f_toupper()` from `Src/strings.c`.
pub fn f_toupper(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(tv_get_string(&argvars[0]).to_uppercase());
}

/// Port of `f_strchars()` from `Src/strings.c` — character count.
pub fn f_strchars(argvars: &[typval_T], rettv: &mut typval_T) {
    rettv.vval = v_number(tv_get_string(&argvars[0]).chars().count() as varnumber_T);
}

/// Port of `f_strpart()` from `Src/strings.c` — byte substring
/// `strpart({src}, {start} [, {len}])`.
pub fn f_strpart(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let bytes = s.as_bytes();
    let mut start = tv_get_number_chk(&argvars[1], None);
    if start < 0 {
        start = 0;
    }
    let start = (start as usize).min(bytes.len());
    let end = if argvars.len() >= 3 {
        let len = tv_get_number_chk(&argvars[2], None).max(0) as usize;
        (start + len).min(bytes.len())
    } else {
        bytes.len()
    };
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(String::from_utf8_lossy(&bytes[start..end]).into_owned());
}

/// Port of `f_stridx()` from `Src/strings.c` — byte index of `{needle}` in
/// `{haystack}` (from optional `{start}`), or -1.
pub fn f_stridx(argvars: &[typval_T], rettv: &mut typval_T) {
    let hay = tv_get_string(&argvars[0]);
    let needle = tv_get_string(&argvars[1]);
    let start = argvars.get(2).map_or(0, |t| tv_get_number_chk(t, None).max(0) as usize);
    let idx = if start <= hay.len() {
        hay[start..].find(&needle).map(|i| (i + start) as varnumber_T)
    } else {
        None
    };
    rettv.vval = v_number(idx.unwrap_or(-1));
}

/// Port of `f_trim()` from `Src/strings.c` (subset) — trim whitespace (or the
/// characters in `{mask}`) from both ends.
pub fn f_trim(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    rettv.v_type = VAR_STRING;
    let trimmed = if argvars.len() >= 2 {
        let mask = tv_get_string(&argvars[1]);
        let m: Vec<char> = mask.chars().collect();
        s.trim_matches(|c| m.contains(&c)).to_string()
    } else {
        s.trim().to_string()
    };
    rettv.vval = v_string(trimmed);
}

/// Port of `f_strridx()` from `Src/strings.c` — byte index of the LAST
/// occurrence of `{needle}` in `{haystack}`, or -1.
pub fn f_strridx(argvars: &[typval_T], rettv: &mut typval_T) {
    let hay = tv_get_string(&argvars[0]);
    let needle = tv_get_string(&argvars[1]);
    rettv.vval = v_number(hay.rfind(&needle).map_or(-1, |i| i as varnumber_T));
}

/// Port of `f_tr()` from `Src/strings.c` — translate characters of `{src}`
/// that appear in `{fromstr}` to the matching character of `{tostr}`.
pub fn f_tr(argvars: &[typval_T], rettv: &mut typval_T) {
    let src = tv_get_string(&argvars[0]);
    let from: Vec<char> = tv_get_string(&argvars[1]).chars().collect();
    let to: Vec<char> = tv_get_string(&argvars[2]).chars().collect();
    let out: String = src
        .chars()
        .map(|c| match from.iter().position(|&f| f == c) {
            Some(i) => to.get(i).copied().unwrap_or(c),
            None => c,
        })
        .collect();
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(out);
}

/// Port of `f_str2list()` from `Src/strings.c` — a List of the code points of
/// `{string}`.
pub fn f_str2list(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let l = tv_list_alloc_ret(rettv, s.chars().count() as isize);
    let mut lb = l.borrow_mut();
    for c in s.chars() {
        tv_list_append_number(&mut lb, c as varnumber_T);
    }
}

/// Port of `f_strgetchar()` from `Src/strings.c` — the decimal codepoint of the
/// `{index}`'th character (0-based) of `{str}`, or -1 if out of range.
pub fn f_strgetchar(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let idx = tv_get_number_chk(&argvars[1], None);
    rettv.vval = v_number(if idx < 0 {
        -1
    } else {
        s.chars().nth(idx as usize).map_or(-1, |c| c as varnumber_T)
    });
}

/// Port of `f_strcharpart()` from `Src/strings.c` — a substring of `{src}` by
/// CHARACTER index: `{start}` chars in, `{len}` chars long (to end if omitted).
/// A negative `{start}` counts toward `{len}` (matching Vim).
pub fn f_strcharpart(argvars: &[typval_T], rettv: &mut typval_T) {
    let chars: Vec<char> = tv_get_string(&argvars[0]).chars().collect();
    let mut start = tv_get_number_chk(&argvars[1], None);
    let has_len = argvars.len() >= 3;
    let mut len = if has_len {
        tv_get_number_chk(&argvars[2], None)
    } else {
        chars.len() as varnumber_T - start
    };
    if start < 0 {
        len += start; // chars before 0 are skipped but still consume {len}
        start = 0;
    }
    let start = (start as usize).min(chars.len());
    let len = len.max(0) as usize;
    let end = (start + len).min(chars.len());
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(chars[start..end].iter().collect());
}

/// Port of `f_byteidx()` from `Src/strings.c` — the byte index of the `{nr}`'th
/// character of `{expr}`. `nr == strcharlen` yields the byte length; `nr` past
/// the end yields -1.
pub fn f_byteidx(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let nr = tv_get_number_chk(&argvars[1], None);
    rettv.vval = v_number(if nr < 0 {
        -1
    } else {
        match s.char_indices().nth(nr as usize) {
            Some((b, _)) => b as varnumber_T,
            None if nr as usize == s.chars().count() => s.len() as varnumber_T,
            None => -1,
        }
    });
}

/// Port of `f_charidx()` from `Src/strings.c` — the character index of the byte
/// at `{idx}` in `{string}`, or -1 if `{idx}` is out of range.
pub fn f_charidx(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let idx = tv_get_number_chk(&argvars[1], None);
    rettv.vval = v_number(if idx < 0 || idx as usize >= s.len() {
        -1
    } else {
        s[..idx as usize].chars().count() as varnumber_T
    });
}

/// Port of `f_byteidxcomp()` from `Src/strings.c` — the byte index of the
/// `{nr}`'th character. Identical to `byteidx()` here: vimlrs does not track
/// composing characters separately, so each character is one index either way.
pub fn f_byteidxcomp(argvars: &[typval_T], rettv: &mut typval_T) {
    f_byteidx(argvars, rettv);
}

