//! Port of `src/nvim/strings.c` — the Vimscript string builtins.
//!
//! These are real Neovim functions whose C home is `strings.c` (not under
//! `eval/`, so not in the vendored `vendor/eval/` tree). Ported from
//! `~/forkedRepos/neovim/src/nvim/strings.c`; their names are recorded in
//! `tests/data/fake_fn_allowlist.txt` as category-A (real C, home file not
//! vendored) until `strings.c` itself is vendored.
#![allow(non_snake_case)]

use crate::ported::charset::{
    vim_str2nr, STR2NR_BIN, STR2NR_FORCE, STR2NR_HEX, STR2NR_OCT, STR2NR_OOCT, STR2NR_QUOTE,
};
use crate::ported::eval::encode::encode_tv2string;
use crate::ported::eval::typval::{
    tv_blob_alloc_ret, tv_get_number_chk, tv_get_string, tv_list_alloc_ret, tv_list_append_number,
    tv_list_append_tv,
};
use crate::ported::eval::typval_defs_h::{typval_T, typval_vval_union::*, varnumber_T, VarType::*};
use crate::ported::eval_h::{FAIL, OK};
use crate::ported::message::{emsg, semsg};
use crate::ported::option::get_option_value;

/// "string(expr)" function — the `string()` rendering of `expr`.
pub fn f_string(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: rettv->v_type = VAR_STRING; rettv->vval.v_string = encode_tv2string(...);
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(encode_tv2string(&argvars[0]));
}

/// "str2nr()" function — parse the leading number in a string in the given
/// `{base}` (2, 8, 10 or 16; default 10). Port of `f_str2nr()` from
/// `Src/eval/funcs.c`: the base forces the radix (`STR2NR_FORCE`), an optional
/// `{quote}` arg permits `'` digit separators, and a leading sign is honored.
pub fn f_str2nr(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: base = argvars[1] (default 10); error on anything but 2/8/10/16.
    let base = argvars
        .get(1)
        .map(|t| tv_get_number_chk(t, None))
        .unwrap_or(10);
    if !matches!(base, 2 | 8 | 10 | 16) {
        // c: `emsg(_(e_invarg)); return;` — an unsupported base is an error, not
        // a silent 0.
        emsg("E474: Invalid argument");
        return;
    }
    // c: switch(base) { case 2: STR2NR_BIN|FORCE; case 8: STR2NR_OCT|OOCT|FORCE;
    //     case 16: STR2NR_HEX|FORCE; } (base 10 stays plain decimal, what == 0)
    let mut what = match base {
        2 => STR2NR_BIN | STR2NR_FORCE,
        8 => STR2NR_OCT | STR2NR_OOCT | STR2NR_FORCE,
        16 => STR2NR_HEX | STR2NR_FORCE,
        _ => 0,
    };
    // c: a truthy {quote} (argvars[2]) lets `'` separate digits (1'000 → 1000).
    if argvars
        .get(2)
        .is_some_and(|t| tv_get_number_chk(t, None) != 0)
    {
        what |= STR2NR_QUOTE;
    }
    // c: p = skipwhite(...); handle the leading sign before vim_str2nr.
    let s = tv_get_string(&argvars[0]);
    let p = s.trim_start();
    let (neg, p) = match p.strip_prefix('-') {
        Some(rest) => (true, rest.trim_start()),
        None => (false, p.strip_prefix('+').map(str::trim_start).unwrap_or(p)),
    };
    let mut n: varnumber_T = 0;
    vim_str2nr(p, None, None, what, Some(&mut n), None, 0, false, None);
    rettv.vval = v_number(if neg { -n } else { n });
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

/// Port of `f_strchars()` from `Src/strings.c` — character count. The optional
/// `{skipcc}` (argvars[1]); when truthy, composing characters are not counted
/// (the same folding `strcharlen()` always applies).
pub fn f_strchars(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    // c: skip_count_composing = (argvars[1] != UNKNOWN) ? tv_get_bool(...) : 0;
    let skipcc = argvars
        .get(1)
        .is_some_and(|t| tv_get_number_chk(t, None) != 0);
    let n = if skipcc {
        s.chars().filter(|c| !utf_iscomposing(*c)).count()
    } else {
        s.chars().count()
    };
    rettv.vval = v_number(n as varnumber_T);
}

/// Port of `f_strpart()` from `Src/strings.c` — byte substring
/// `strpart({src}, {start} [, {len}])`.
pub fn f_strpart(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let bytes = s.as_bytes();
    let slen = bytes.len() as varnumber_T;
    // c: nbyte = start; len = {len} present ? that : slen - nbyte.
    let mut nbyte = tv_get_number_chk(&argvars[1], None);
    // The C does all of this in `varnumber_T` (int64) and *relies on the
    // two's-complement wrap* at the extremes: with `start` = INT64_MIN,
    // `slen - nbyte` and the later `len += nbyte` each wrap once and cancel, so
    // `strpart('abc', -9223372036854775808)` is `'abc'` in Vim. Rust's `-`/`+`
    // panic there instead (debug overflow check), so spell the wrap out.
    let mut len = if argvars.len() >= 3 {
        tv_get_number_chk(&argvars[2], None)
    } else {
        slen.wrapping_sub(nbyte)
    };
    // c: a negative start clamps to 0 but folds its offset into the length, so
    // strpart('hello', -2, 3) keeps only the first character.
    if nbyte < 0 {
        len = len.wrapping_add(nbyte);
        nbyte = 0;
    } else if nbyte > slen {
        nbyte = slen;
    }
    if len < 0 {
        len = 0;
    } else if nbyte.saturating_add(len) > slen {
        len = slen - nbyte;
    }
    // c: with {chars} ({4}) set, reinterpret the byte-clamped {len} as a
    // character count and walk that many characters forward from `nbyte`
    // (the {start} offset itself stays byte-based, matching the C). The walk
    // is `utfc_ptr2len` — a base char plus its composing marks is ONE char
    // (`strpart('écombining', 0, 2, 0)` with decomposed é is `'éc'`).
    if argvars.len() >= 3 && argvars.get(3).is_some_and(|t| t.v_type != VAR_UNKNOWN) {
        let mut off = nbyte;
        while off < slen && len > 0 {
            off += crate::ported::mbyte::utfc_ptr2len(&bytes[off as usize..]).max(1) as varnumber_T;
            len -= 1;
        }
        len = off - nbyte;
    }
    let start = nbyte as usize;
    let end = start + len as usize;
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(String::from_utf8_lossy(&bytes[start..end]).into_owned());
}

/// Port of `f_stridx()` from `Src/strings.c` — byte index of `{needle}` in
/// `{haystack}` (from optional `{start}`), or -1.
pub fn f_stridx(argvars: &[typval_T], rettv: &mut typval_T) {
    let hay = tv_get_string(&argvars[0]);
    let needle = tv_get_string(&argvars[1]);
    let start = argvars
        .get(2)
        .map_or(0, |t| tv_get_number_chk(t, None).max(0) as usize);
    // c: `haystack += start_idx; pos = strstr(haystack, needle)` — a *byte*
    // offset and a byte-wise search, returning the offset from the start of the
    // haystack. Slicing the `str` (`hay[start..]`) panicked when `start` split a
    // multibyte char (`stridx('日本語', 'x', 1)`), so walk the bytes as C does.
    // An empty needle matches immediately, as `strstr` does.
    let (hb, nb) = (hay.as_bytes(), needle.as_bytes());
    let idx = if start > hb.len() {
        None
    } else if nb.is_empty() {
        Some(start as varnumber_T)
    } else {
        hb[start..]
            .windows(nb.len())
            .position(|w| w == nb)
            .map(|i| (i + start) as varnumber_T)
    };
    rettv.vval = v_number(idx.unwrap_or(-1));
}

/// Port of `f_trim()` from `Src/strings.c` — trim characters from the ends of
/// `{text}`.
///
/// With no `{mask}` (or an *empty* `{mask}`, which the C folds to NULL) any
/// character `<= ' '` or the non-breaking space 0xa0 is trimmed. With a mask, a
/// character is trimmed when its base codepoint (`utf_ptr2char`) equals the
/// base codepoint of any mask cluster, and the walk advances by whole
/// base-plus-composing clusters (`MB_PTR_ADV`). `{dir}` is truncated to `int`
/// exactly like the C's `(int)tv_get_number_chk(...)` — so an INT64-huge value
/// wraps rather than erroring — then anything outside 0..2 is E475.
pub fn f_trim(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(String::new());
    // c: `if (tv_check_for_opt_string_arg(argvars, 1) == FAIL) return;` — a
    // present, non-String {mask} is E1174.
    let has_mask = argvars.len() >= 2 && argvars[1].v_type != VAR_UNKNOWN;
    if has_mask && argvars[1].v_type != VAR_STRING {
        semsg("E1174: String required for argument 2");
        return;
    }
    // c: `mask = tv_get_string_buf_chk(...); if (*mask == NUL) mask = NULL;` —
    // an EMPTY mask falls back to the default whitespace set.
    let mask_str = if has_mask {
        Some(tv_get_string(&argvars[1])).filter(|m| !m.is_empty())
    } else {
        None
    };
    // Base codepoint of each mask cluster (c: `utf_ptr2char(p)` per
    // `MB_PTR_ADV` step).
    let mask_bases: Option<Vec<i32>> = mask_str.as_deref().map(|m| {
        let mb = m.as_bytes();
        let mut bases = Vec::new();
        let mut i = 0usize;
        while i < mb.len() {
            bases.push(crate::ported::mbyte::utf_ptr2char(&mb[i..]));
            i += crate::ported::mbyte::utfc_ptr2len(&mb[i..]).max(1) as usize;
        }
        bases
    });
    // c: {dir} is parsed only when {mask} is a String (matching the C's
    // `if (argvars[1].v_type == VAR_STRING)` nesting), `(int)`-truncated, then
    // range-checked: 0 = both ends, 1 = leading, 2 = trailing.
    let mut dir: i32 = 0;
    if has_mask && argvars.len() >= 3 && argvars[2].v_type != VAR_UNKNOWN {
        dir = tv_get_number_chk(&argvars[2], None) as i32;
        if !(0..=2).contains(&dir) {
            semsg(&format!(
                "E475: Invalid argument: {}",
                tv_get_string(&argvars[2])
            ));
            return;
        }
    }
    // c: `c1 > ' ' && c1 != 0xa0` is the KEEP condition for the default mask.
    let trims = |c1: i32| -> bool {
        match &mask_bases {
            None => c1 <= b' ' as i32 || c1 == 0xa0,
            Some(bases) => bases.contains(&c1),
        }
    };
    let bytes = s.as_bytes();
    // c: trim leading characters (dir 0 or 1) — `c1 = utf_ptr2char(head)` then
    // `MB_PTR_ADV(head)`: compare the cluster's base codepoint, advance by the
    // whole base-plus-composing cluster.
    let mut head = 0usize;
    if dir == 0 || dir == 1 {
        while head < bytes.len() {
            if !trims(crate::ported::mbyte::utf_ptr2char(&bytes[head..])) {
                break;
            }
            head += crate::ported::mbyte::utfc_ptr2len(&bytes[head..]).max(1) as usize;
        }
    }
    // c: trim trailing characters (dir 0 or 2) — `MB_PTR_BACK(head, prev)`
    // cluster by cluster from the end.
    let mut tail = bytes.len();
    if dir == 0 || dir == 2 {
        while tail > head {
            let mut prev = tail - 1;
            prev -= crate::ported::mbyte::utf_head_off(bytes, prev);
            if !trims(crate::ported::mbyte::utf_ptr2char(&bytes[prev..])) {
                break;
            }
            tail = prev;
        }
    }
    rettv.vval = v_string(s[head..tail].to_string());
}

/// Port of `f_strridx()` from `Src/strings.c` — byte index of the LAST
/// occurrence of `{needle}` in `{haystack}`, or -1. The optional third argument
/// is an upper limit for the match index; an empty needle matches past the end.
pub fn f_strridx(argvars: &[typval_T], rettv: &mut typval_T) {
    let hay = tv_get_string(&argvars[0]);
    let needle = tv_get_string(&argvars[1]);
    // c: rettv->vval.v_number = -1;
    rettv.v_type = VAR_NUMBER;
    rettv.vval = v_number(-1);
    let hb = hay.as_bytes();
    let nb = needle.as_bytes();
    // c: third argument — upper limit for the index; negative can never match.
    let end_idx: isize = if argvars.len() > 2 && argvars[2].v_type != VAR_UNKNOWN {
        let e = tv_get_number_chk(&argvars[2], None) as isize;
        if e < 0 {
            return;
        }
        e
    } else {
        hb.len() as isize
    };
    let lastmatch: Option<usize> = if nb.is_empty() {
        // c: empty string matches past the end — lastmatch = haystack + end_idx.
        Some(end_idx as usize)
    } else if nb.len() > hb.len() {
        // c: a needle longer than the haystack never matches (`strstr` → NULL).
        // Guard before the search below so its `hb[i..i + nb.len()]` slice — whose
        // range floors at 0 via `saturating_sub` — cannot index past the end.
        None
    } else {
        // c: for (rest = haystack; …) { rest = strstr(rest, needle); if (rest ==
        //    NULL || rest > haystack + end_idx) break; lastmatch = rest; }
        let mut found = None;
        let mut from = 0usize;
        loop {
            match (from..=hb.len().saturating_sub(nb.len())).find(|&i| hb[i..i + nb.len()] == *nb) {
                Some(pos) if (pos as isize) <= end_idx => {
                    found = Some(pos);
                    from = pos + 1;
                }
                _ => break,
            }
        }
        found
    };
    if let Some(i) = lastmatch {
        rettv.vval = v_number(i as varnumber_T);
    }
}

/// Port of `f_tr()` from `Src/strings.c` — translate characters of `{src}`
/// that appear in `{fromstr}` to the matching character of `{tostr}`.
pub fn f_tr(argvars: &[typval_T], rettv: &mut typval_T) {
    let src = tv_get_string(&argvars[0]);
    let fromstr = tv_get_string(&argvars[1]);
    let from: Vec<char> = fromstr.chars().collect();
    let to: Vec<char> = tv_get_string(&argvars[2]).chars().collect();
    rettv.v_type = VAR_STRING;
    // c: a {src} char found in {fromstr} at index i maps to {tostr}[i]; if {tostr}
    // has no such character the sets do not correspond — E475, returning "".
    let mut out = String::new();
    // c: `bool first = true;` — the length check below runs at most once.
    let mut first = true;
    for c in src.chars() {
        match from.iter().position(|&f| f == c) {
            Some(i) => match to.get(i) {
                Some(&t) => out.push(t),
                // c: `if (*p == NUL) { goto error; }  // tostr is shorter than fromstr.`
                None => {
                    semsg(&format!("E475: Invalid argument: {fromstr}"));
                    rettv.vval = v_string(String::new());
                    return;
                }
            },
            None => {
                // c: `if (first && cpstr == in_str) { … if (idx != 0) goto error; }`
                // — "Check that fromstr and tostr have the same number of
                // (multi-byte) characters. Done only once when a character of
                // in_str doesn't appear in fromstr." Without it, mismatched sets
                // went unreported whenever no input character happened to be
                // translated: `tr('-7', 'hello world', 'x')` returned '-7'
                // instead of raising E475.
                if first {
                    first = false;
                    if from.len() != to.len() {
                        semsg(&format!("E475: Invalid argument: {fromstr}"));
                        rettv.vval = v_string(String::new());
                        return;
                    }
                }
                out.push(c);
            }
        }
    }
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
///
/// The optional 4th arg `{skipcc}` selects how a "character" is measured: when
/// truthy, a base character plus its trailing composing characters count as one
/// character (c: `utfc_ptr2len`); otherwise each codepoint is its own character
/// (c: `utf_ptr2len`).
pub fn f_strcharpart(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let bytes = s.as_bytes();
    // c: `const size_t slen = strlen(p)` — everything below is the C's byte
    // walk over `int nbyte`/`int len` offsets, including its `(int)` truncating
    // casts (which is why an INT64-huge {start} wraps instead of erroring:
    // `strcharpart("a\\b", -9223372036854775807)` is `(int)` 1 → `'\b'`).
    let slen = bytes.len() as i64;
    // c: skipcc is read only when BOTH {len} and {skipcc} are present.
    let skipcc = argvars.get(2).is_some_and(|t| t.v_type != VAR_UNKNOWN)
        && argvars
            .get(3)
            .is_some_and(|t| t.v_type != VAR_UNKNOWN && tv_get_number_chk(t, None) != 0);
    // c: skipcc ? utfc_ptr2len (base + composing = one char) : utf_ptr2len.
    let step = |off: usize| -> i64 {
        let l = if skipcc {
            crate::ported::mbyte::utfc_ptr2len(&bytes[off..])
        } else {
            crate::ported::mbyte::utf_ptr2len(&bytes[off..])
        };
        l.max(1) as i64
    };
    let mut nbyte: i64 = 0;
    let mut nchar = tv_get_number_chk(&argvars[1], None);
    if nchar > 0 {
        // c: while (nchar > 0 && nbyte < slen) nbyte += utf*_ptr2len(p + nbyte);
        while nchar > 0 && nbyte < slen {
            nbyte += step(nbyte as usize);
            nchar -= 1;
        }
    } else {
        // c: `nbyte = (int)nchar` — truncate, do not clamp.
        nbyte = (nchar as i32) as i64;
    }
    let mut len: i64;
    if argvars.get(2).is_some_and(|t| t.v_type != VAR_UNKNOWN) {
        // c: `int charlen = (int)tv_get_number(&argvars[2])` — truncated too.
        let mut charlen = (tv_get_number_chk(&argvars[2], None) as i32) as i64;
        len = 0;
        while charlen > 0 && nbyte + len < slen {
            let off = nbyte + len;
            if off < 0 {
                // c: a char before the string counts as one byte of {len}.
                len += 1;
            } else {
                len += step(off as usize);
            }
            charlen -= 1;
        }
    } else {
        len = slen - nbyte; // c: default — all bytes that are available.
    }
    // c: only return the overlap between the specified part and the string.
    if nbyte < 0 {
        len += nbyte;
        nbyte = 0;
    } else if nbyte > slen {
        nbyte = slen;
    }
    if len < 0 {
        len = 0;
    } else if nbyte + len > slen {
        len = slen - nbyte;
    }
    rettv.v_type = VAR_STRING;
    let (a, b) = (nbyte as usize, (nbyte + len) as usize);
    rettv.vval = v_string(String::from_utf8_lossy(&bytes[a..b]).into_owned());
}

/// Port of `f_byteidx()` from `Src/strings.c` — the byte index of the `{nr}`'th
/// character of `{expr}`. `nr == strcharlen` yields the byte length; `nr` past
/// the end yields -1.
pub fn f_byteidx(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: byteidx() folds composing characters into the preceding base character.
    rettv.vval = v_number(byteidx_impl(argvars, true));
}

/// Shared core of byteidx()/byteidxcomp(): the byte index of the `{nr}`'th
/// character, or the string length when `{nr}` equals the character count, else
/// -1. When `skipcc`, a composing character is folded into the preceding base
/// character (byteidx); otherwise each codepoint is its own character
/// (byteidxcomp).
fn byteidx_impl(argvars: &[typval_T], skipcc: bool) -> varnumber_T {
    let s = tv_get_string(&argvars[0]);
    let nr = tv_get_number_chk(&argvars[1], None);
    if nr < 0 {
        return -1;
    }
    let nr = nr as usize;
    let mut count = 0usize;
    let mut prev = false; // a base char has been seen to fold a composing one into
    for (b, c) in s.char_indices() {
        let folds = skipcc && prev && utf_iscomposing(c);
        if !folds {
            if count == nr {
                return b as varnumber_T;
            }
            count += 1;
        }
        prev = true;
    }
    if nr == count {
        s.len() as varnumber_T
    } else {
        -1
    }
}

/// Port of `f_charidx()` from Neovim `src/nvim/strings.c` (home file not under
/// the vendored `vendor/eval/` tree). The character index of the byte at `{idx}`
/// in `{string}`, or -1 if `{idx}` is out of range.
///
/// SUBSET: the optional `{countcc}` (count composing chars) and `{utf16}`
/// arguments are not modelled — this counts every character (i.e. behaves as
/// `{countcc}` = 1). The core multibyte byte→char mapping is faithful.
pub fn f_charidx(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let idx = tv_get_number_chk(&argvars[1], None);
    if idx < 0 {
        rettv.vval = v_number(-1);
        return;
    }
    // c: {countcc} (argvars[2]) — default 0 folds composing characters into their
    // base character (so the index is of the base); 1 counts each separately.
    let countcc = argvars.len() >= 3
        && argvars[2].v_type != VAR_UNKNOWN
        && tv_get_number_chk(&argvars[2], None) != 0;
    // c: {utf16} (argvars[3]) — when truthy, {idx} is a UTF-16 code-unit index
    // into {string} instead of a byte index.
    let use_utf16 = argvars.len() >= 4
        && argvars[3].v_type != VAR_UNKNOWN
        && tv_get_number_chk(&argvars[3], None) != 0;

    // Split into index-units: a base character plus its trailing composing marks
    // when {countcc} is false, else each character alone. Record each unit's
    // start byte and the number of UTF-16 code units preceding it. The character
    // index is the unit's ordinal; an {idx} landing exactly at the end yields the
    // character count and one past the end -1 ("less than {idx} bytes").
    let mut units: Vec<(usize, varnumber_T)> = Vec::new();
    let mut u16_acc: varnumber_T = 0;
    let mut prev = false;
    for (b, c) in s.char_indices() {
        let folds = !countcc && prev && utf_iscomposing(c);
        prev = true;
        if !folds {
            units.push((b, u16_acc));
            u16_acc += utf_char2utf16len(c);
        }
    }
    let n = units.len() as varnumber_T;

    // Ordinal of the unit whose span (in bytes, or in UTF-16 units) contains
    // `idx`; the unit spans are contiguous and their keys strictly increasing, so
    // the containing unit is the last whose key is <= idx.
    let result = if use_utf16 {
        match idx {
            i if i == u16_acc => n,
            i if i > u16_acc => -1,
            i => units.iter().take_while(|(_, before)| *before <= i).count() as varnumber_T - 1,
        }
    } else {
        let strlen = s.len() as varnumber_T;
        match idx {
            i if i == strlen => n,
            i if i > strlen => -1,
            i => {
                units
                    .iter()
                    .take_while(|(sb, _)| (*sb as varnumber_T) <= i)
                    .count() as varnumber_T
                    - 1
            }
        }
    };
    rettv.vval = v_number(result);
}

/// Port of `f_byteidxcomp()` from `Src/strings.c` — the byte index of the
/// `{nr}`'th character. Identical to `byteidx()` here: vimlrs does not track
/// composing characters separately, so each character is one index either way.
pub fn f_byteidxcomp(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: byteidxcomp() counts each composing character separately.
    rettv.vval = v_number(byteidx_impl(argvars, false));
}

/// True for the common Unicode combining-mark ranges (`utf_iscomposing`), used
/// by `strcharlen()` to fold composing characters into their base character.
pub(crate) fn utf_iscomposing(c: char) -> bool {
    let u = c as u32;
    matches!(u,
        0x0300..=0x036F | 0x0483..=0x0489 | 0x0591..=0x05BD | 0x05BF
        | 0x0610..=0x061A | 0x064B..=0x065F | 0x0670 | 0x06D6..=0x06DC
        | 0x06DF..=0x06E4 | 0x0711 | 0x0730..=0x074A | 0x07A6..=0x07B0
        | 0x0816..=0x0823 | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF
        | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

/// Port of `f_strcharlen()` from `Src/strings.c` — the number of characters in
/// `{string}`, ignoring composing characters (each base+composing run counts
/// once). 0 on empty input.
pub fn f_strcharlen(argvars: &[typval_T], rettv: &mut typval_T) {
    let n = tv_get_string(&argvars[0])
        .chars()
        .filter(|c| !utf_iscomposing(*c))
        .count();
    rettv.vval = v_number(n as varnumber_T);
}

/// Port of `f_strtrans()` from `Src/strings.c` — translate unprintable
/// characters to their displayed form: control chars `0x00..0x1F` become `^@`…
/// `^_` (char + 0x40) and `0x7F` becomes `^?`; printable (incl. multibyte) is
/// kept. Matches `transchar` for the common case.
pub fn f_strtrans(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let mut u = c as u32;
        if u < 0x20 {
            // c: `transchar_nonprint()` — `if (c == NL) { c = NUL; }` ("we use
            // newline in place of a NUL"), so `strtrans("a\nb")` is `a^@b`.
            if u == 0x0a {
                u = 0;
            }
            out.push('^');
            out.push((u as u8 + 0x40) as char);
        } else if u == 0x7F {
            out.push_str("^?");
        } else {
            out.push(c);
        }
    }
    rettv.v_type = VAR_STRING;
    rettv.vval = v_string(out);
}

/// Port of `f_slice()` from `Src/strings.c` — `slice({expr}, {start} [, {end}])`,
/// like `expr[start : end]` but with an *exclusive* `{end}` and, for a String,
/// character (not byte) indices. Negative indices count from the end; `{end}` of
/// -1 omits the last item; an omitted `{end}` runs to the end. Returns an empty
/// value of the same type for an empty/invalid range.
pub fn f_slice(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: `if (check_can_index(&argvars[0], true, false) != OK) { return; }` — note
    // `verbose = false`: a Float/Bool/Special/Funcref is *silently* rejected and
    // the result stays the default Number 0 (`slice(v:true, 0)` is `0`, not the
    // stringified `'v:true'` this used to produce, and not an error either).
    if crate::ported::eval::check_can_index(&argvars[0], true, false) != OK {
        return;
    }
    // c: a Dict reaches `eval_index_inner`, whose range branch fails — silently,
    // again because `verbose` is false — leaving the copied value in place. So
    // slicing a Dict hands the Dict back unchanged rather than raising E731/E719.
    if argvars[0].v_type == VAR_DICT {
        *rettv = argvars[0].clone();
        return;
    }
    // c: a String goes through `eval_index_inner(…, exclusive = true)` →
    // `string_slice()` — CHARACTER indices with composing characters folded
    // into their base (`utfc_ptr2len`), end exclusive, raw (unclamped) bounds.
    if !matches!(argvars[0].v_type, VAR_LIST | VAR_BLOB) {
        let s = tv_get_string(&argvars[0]);
        let n1 = tv_get_number_chk(&argvars[1], None);
        let n2 = if argvars.len() >= 3 && argvars[2].v_type != VAR_UNKNOWN {
            tv_get_number_chk(&argvars[2], None)
        } else {
            crate::ported::eval::typval_defs_h::VARNUMBER_MAX
        };
        rettv.v_type = VAR_STRING;
        rettv.vval =
            v_string(crate::ported::eval::string_slice(&s, n1, n2, true).unwrap_or_default());
        return;
    }

    // c: Lists and Blobs run through `eval_index_inner` → the ported
    // `tv_list_slice_or_index`/`tv_blob_slice_or_index`, with the RAW bounds
    // (an out-of-range start yields an EMPTY result — `slice([1,2,3],-4,2)` is
    // `[]`, not a clamped `[1,2]`) and `n2` = VARNUMBER_MAX when omitted.
    let n1 = tv_get_number_chk(&argvars[1], None);
    let n2 = if argvars.len() >= 3 && argvars[2].v_type != VAR_UNKNOWN {
        tv_get_number_chk(&argvars[2], None)
    } else {
        crate::ported::eval::typval_defs_h::VARNUMBER_MAX
    };
    match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(l)) => match l.clone() {
            Some(l) => {
                *rettv = argvars[0].clone();
                let _ = crate::ported::eval::typval::tv_list_slice_or_index(
                    &l, true, n1, n2, true, rettv, false,
                );
            }
            None => {
                tv_list_alloc_ret(rettv, 0);
            }
        },
        (VAR_BLOB, v_blob(b)) => match b.clone() {
            Some(b) => {
                *rettv = argvars[0].clone();
                let _ = crate::ported::eval::typval::tv_blob_slice_or_index(
                    &b.borrow(),
                    true,
                    n1,
                    n2,
                    true,
                    rettv,
                );
            }
            None => {
                tv_blob_alloc_ret(rettv);
            }
        },
        // Strings (and Numbers-as-strings) returned above through the ported
        // `string_slice`; Dicts returned even earlier.
        _ => {}
    }
}

/// Port of `utf_char2cells()` (Neovim mbyte.c) — the display width of a single
/// character: 0 for a composing mark, 2 for an East-Asian-wide / emoji
/// character (the standard wide ranges), otherwise 1.
fn utf_char2cells(c: char) -> usize {
    if utf_iscomposing(c) {
        return 0;
    }
    let u = c as u32;
    // c: a `setcellwidths()` override (cw_value()) takes precedence over the
    // built-in width tables (Neovim mbyte.c).
    if let Some(w) = cw_value(u) {
        return w;
    }
    // c: an unprintable C1 character has no glyph and is shown as `<80>` — four
    // cells, and `strwidth()` counts them (`strwidth(nr2char(0x80))` is 4).
    if (0x80..=0x9f).contains(&u) {
        return 4;
    }
    let wide = matches!(u,
        0x1100..=0x115F | 0x2329 | 0x232A | 0x2E80..=0x303E | 0x3041..=0x33FF
        | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xA000..=0xA4CF | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1FAFF | 0x20000..=0x3FFFD);
    if wide {
        2
    } else {
        1
    }
}

// ── setcellwidths()/getcellwidths() — Neovim mbyte.c (home file not vendored).
// The user-defined cell-width override table installed by `setcellwidths()`.
// Each tuple is `(low, high, width)`: codepoints in `low..=high` display in
// `width` (1 or 2) cells. Consulted by `cw_value()` from `utf_char2cells()`.
thread_local! {
    static CW_TABLE: std::cell::RefCell<Vec<(u32, u32, u8)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Port of `cw_value()` (Neovim mbyte.c) — the display width override for
/// codepoint `c` from the `setcellwidths()` table, or `None` when no range
/// covers it (so the built-in width tables apply).
fn cw_value(c: u32) -> Option<usize> {
    CW_TABLE.with(|t| {
        t.borrow()
            .iter()
            .find(|(lo, hi, _)| c >= *lo && c <= *hi)
            .map(|(_, _, w)| *w as usize)
    })
}

/// Port of `f_setcellwidths()` (Neovim mbyte.c) — install a list of
/// `[low, high, width]` triples as the character cell-width override table.
/// Each entry must be a 3-Number List with `width` 1 or 2 and `low <= high`.
pub fn f_setcellwidths(argvars: &[typval_T], _rettv: &mut typval_T) {
    let l = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => l.clone(),
        // c: emsg(_(e_listreq)) — the argument must be a List.
        _ => {
            crate::ported::message::emsg("E1109: List required");
            return;
        }
    };
    let mut table: Vec<(u32, u32, u8)> = Vec::new();
    for item in l.borrow().lv_items.iter() {
        // c: each entry is itself a List of exactly three Numbers.
        let triple: Vec<varnumber_T> = match (item.li_tv.v_type, &item.li_tv.vval) {
            (VAR_LIST, v_list(Some(inner))) => inner
                .borrow()
                .lv_items
                .iter()
                .map(|e| crate::ported::eval::typval::tv_get_number(&e.li_tv))
                .collect(),
            _ => {
                crate::ported::message::emsg("E1110: List item is not a List");
                return;
            }
        };
        if triple.len() != 3 {
            crate::ported::message::emsg("E1111: List with three Numbers required");
            return;
        }
        let (lo, hi, w) = (triple[0], triple[1], triple[2]);
        if w != 1 && w != 2 {
            crate::ported::message::emsg("E1112: List item width must be 1 or 2");
            return;
        }
        if lo < 0 || hi < lo {
            crate::ported::message::emsg("E1113: Overlapping ranges for 0x...");
            return;
        }
        table.push((lo as u32, hi as u32, w as u8));
    }
    CW_TABLE.with(|t| *t.borrow_mut() = table);
}

/// Port of `f_getcellwidths()` (Neovim mbyte.c) — return the cell-width
/// override table as a List of `[low, high, width]` triples (insertion order).
pub fn f_getcellwidths(_argvars: &[typval_T], rettv: &mut typval_T) {
    let table = CW_TABLE.with(|t| t.borrow().clone());
    let out = tv_list_alloc_ret(rettv, table.len() as isize);
    let mut ob = out.borrow_mut();
    for (lo, hi, w) in table {
        let inner = crate::ported::eval::typval::tv_list_alloc(3);
        {
            let mut ib = inner.borrow_mut();
            tv_list_append_number(&mut ib, lo as varnumber_T);
            tv_list_append_number(&mut ib, hi as varnumber_T);
            tv_list_append_number(&mut ib, w as varnumber_T);
        }
        tv_list_append_tv(
            &mut ob,
            typval_T {
                v_type: VAR_LIST,
                v_lock: crate::ported::eval::typval_defs_h::VarLockStatus::VAR_UNLOCKED,
                vval: v_list(Some(inner)),
            },
        );
    }
}

/// Port of `f_strwidth()` from `Src/strings.c` — the number of display cells
/// `{string}` occupies (composing marks add 0, wide characters add 2).
pub fn f_strwidth(argvars: &[typval_T], rettv: &mut typval_T) {
    let w: usize = tv_get_string(&argvars[0]).chars().map(utf_char2cells).sum();
    rettv.vval = v_number(w as varnumber_T);
}

/// Port of `f_strdisplaywidth()` from `Src/strings.c` — like `strwidth()` but a
/// Tab advances to the next `'tabstop'` boundary. The optional `{col}` is the
/// starting screen column (so leading text affects Tab stops).
pub fn f_strdisplaywidth(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    // c: `col = (int)tv_get_number(&argvars[1])` — a truncating cast, and the
    // value may legally be INT_MAX (or negative).
    let col0: i32 = if argvars.len() >= 2 && argvars[1].v_type != VAR_UNKNOWN {
        tv_get_number_chk(&argvars[1], None) as i32
    } else {
        0
    };
    let ts: i64 = {
        let t = tv_get_number_chk(&get_option_value("tabstop"), None);
        if t > 0 {
            t
        } else {
            8
        }
    };
    // c: `linetabsize_col(col, s) - col`, where `linesize_fast` accumulates an
    // int64 `vcol` and CLAMPS the returned int at MAXCOL (0x7fffffff): a huge
    // starting {col} saturates immediately and the result is 0.
    const MAXCOL: i64 = 0x7fffffff; // enum { MAXCOL } — Src/pos_defs.h:19
    let mut vcol: i64 = col0 as i64;
    let mut vcol_arg: i64 = vcol;
    for c in s.chars() {
        let width: i64 = if c == '\t' {
            // c: tabstop_padding — `ts - (col % ts)`; Rust `%` truncates toward
            // zero exactly like the C's, negative columns included.
            ts - vcol_arg % ts
        } else if (c as u32) < 0x20 || c == '\x7f' {
            // c: a control character has no glyph — it *displays* as `^X` (`^J`,
            // `^?`), which is two cells. `strdisplaywidth` measures the display,
            // so it counts 2 where `strwidth` (which measures the text) counts 1.
            2
        } else {
            utf_char2cells(c) as i64
        };
        vcol += width;
        if vcol > MAXCOL {
            vcol_arg = MAXCOL;
            break;
        }
        vcol_arg = vcol;
    }
    rettv.vval = v_number(vcol_arg - col0 as i64);
}

/// Port of `f_charclass()` from `Src/strings.c` — the character class of the
/// first character of `{string}`: 0 blank, 1 punctuation, 2 word character, 3
/// emoji. 0 for an empty String.
pub fn f_charclass(argvars: &[typval_T], rettv: &mut typval_T) {
    let class = match tv_get_string(&argvars[0]).chars().next() {
        None => 0,
        Some(c) => utf_class_tab(c as u32),
    };
    rettv.vval = v_number(class);
}

/// Port of `utf_class_tab()` / `mb_get_class_tab()` (Neovim mbyte.c) — the
/// character class of codepoint `c`: 0 for blank/NUL, 1 for punctuation, 2 for
/// an alphanumeric word character, and >2 (a representative codepoint) for other
/// word characters such as CJK ideographs, Hangul and kana.
///
/// The Latin1 fast path uses the default `'iskeyword'` (`@,48-57,_,192-255`).
/// The emoji branch mirrors `prop_is_emojilike()` (`UTF8PROC_BOUNDCLASS_`
/// `EXTENDED_PICTOGRAPHIC`/`REGIONAL_INDICATOR`); utf8proc's property tables are
/// not vendored, so it is approximated by the common emoji blocks — the same
/// approximation `utf_char2cells()` uses for width. The interval table below is
/// transcribed verbatim from `utf_class_tab()` and binary-searched exactly as
/// the C does.
fn utf_class_tab(c: u32) -> varnumber_T {
    // First quick check for Latin1 characters, use 'iskeyword'.
    if c < 0x100 {
        if c == b' ' as u32 || c == b'\t' as u32 || c == 0 || c == 0xa0 {
            return 0; // blank
        }
        if (c as u8).is_ascii_alphanumeric() || c == b'_' as u32 || (0xc0..=0xff).contains(&c) {
            return 2; // word character
        }
        return 1; // punctuation
    }

    // emoji (utf8proc data not vendored — approximate the common blocks).
    if matches!(c, 0x1F1E6..=0x1F1FF | 0x1F300..=0x1FAFF | 0x2600..=0x27BF) {
        return 3;
    }

    // sorted list of non-overlapping intervals: (first, last, class).
    const CLASSES: &[(u32, u32, varnumber_T)] = &[
        (0x037e, 0x037e, 1), // Greek question mark
        (0x0387, 0x0387, 1), // Greek ano teleia
        (0x055a, 0x055f, 1), // Armenian punctuation
        (0x0589, 0x0589, 1), // Armenian full stop
        (0x05be, 0x05be, 1),
        (0x05c0, 0x05c0, 1),
        (0x05c3, 0x05c3, 1),
        (0x05f3, 0x05f4, 1),
        (0x060c, 0x060c, 1),
        (0x061b, 0x061b, 1),
        (0x061f, 0x061f, 1),
        (0x066a, 0x066d, 1),
        (0x06d4, 0x06d4, 1),
        (0x0700, 0x070d, 1), // Syriac punctuation
        (0x0964, 0x0965, 1),
        (0x0970, 0x0970, 1),
        (0x0df4, 0x0df4, 1),
        (0x0e4f, 0x0e4f, 1),
        (0x0e5a, 0x0e5b, 1),
        (0x0f04, 0x0f12, 1),
        (0x0f3a, 0x0f3d, 1),
        (0x0f85, 0x0f85, 1),
        (0x104a, 0x104f, 1), // Myanmar punctuation
        (0x10fb, 0x10fb, 1), // Georgian punctuation
        (0x1361, 0x1368, 1), // Ethiopic punctuation
        (0x166d, 0x166e, 1), // Canadian Syl. punctuation
        (0x1680, 0x1680, 0),
        (0x169b, 0x169c, 1),
        (0x16eb, 0x16ed, 1),
        (0x1735, 0x1736, 1),
        (0x17d4, 0x17dc, 1), // Khmer punctuation
        (0x1800, 0x180a, 1), // Mongolian punctuation
        (0x2000, 0x200b, 0), // spaces
        (0x200c, 0x2027, 1), // punctuation and symbols
        (0x2028, 0x2029, 0),
        (0x202a, 0x202e, 1), // punctuation and symbols
        (0x202f, 0x202f, 0),
        (0x2030, 0x205e, 1), // punctuation and symbols
        (0x205f, 0x205f, 0),
        (0x2060, 0x206f, 1),      // punctuation and symbols
        (0x2070, 0x207f, 0x2070), // superscript
        (0x2080, 0x2094, 0x2080), // subscript
        (0x20a0, 0x27ff, 1),      // all kinds of symbols
        (0x2800, 0x28ff, 0x2800), // braille
        (0x2900, 0x2998, 1),      // arrows, brackets, etc.
        (0x29d8, 0x29db, 1),
        (0x29fc, 0x29fd, 1),
        (0x2e00, 0x2e7f, 1), // supplemental punctuation
        (0x3000, 0x3000, 0), // ideographic space
        (0x3001, 0x3020, 1), // ideographic punctuation
        (0x3030, 0x3030, 1),
        (0x303d, 0x303d, 1),
        (0x3040, 0x309f, 0x3040), // Hiragana
        (0x30a0, 0x30ff, 0x30a0), // Katakana
        (0x3300, 0x9fff, 0x4e00), // CJK Ideographs
        (0xac00, 0xd7a3, 0xac00), // Hangul Syllables
        (0xf900, 0xfaff, 0x4e00), // CJK Ideographs
        (0xfd3e, 0xfd3f, 1),
        (0xfe30, 0xfe6b, 1),        // punctuation forms
        (0xff00, 0xff0f, 1),        // half/fullwidth ASCII
        (0xff1a, 0xff20, 1),        // half/fullwidth ASCII
        (0xff3b, 0xff40, 1),        // half/fullwidth ASCII
        (0xff5b, 0xff65, 1),        // half/fullwidth ASCII
        (0x1d000, 0x1d24f, 1),      // Musical notation
        (0x1d400, 0x1d7ff, 1),      // Mathematical Alphanumeric Symbols
        (0x1f000, 0x1f2ff, 1),      // Game pieces; enclosed characters
        (0x1f300, 0x1f9ff, 1),      // Many symbol blocks
        (0x20000, 0x2a6df, 0x4e00), // CJK Ideographs
        (0x2a700, 0x2b73f, 0x4e00), // CJK Ideographs
        (0x2b740, 0x2b81f, 0x4e00), // CJK Ideographs
        (0x2f800, 0x2fa1f, 0x4e00), // CJK Ideographs
    ];

    // binary search in table
    let mut bot: i64 = 0;
    let mut top: i64 = CLASSES.len() as i64 - 1;
    while top >= bot {
        let mid = ((bot + top) / 2) as usize;
        if CLASSES[mid].1 < c {
            bot = mid as i64 + 1;
        } else if CLASSES[mid].0 > c {
            top = mid as i64 - 1;
        } else {
            return CLASSES[mid].2;
        }
    }

    // most other characters are "word" characters
    2
}

/// UTF-16 code-unit length of a single character: 2 for an astral character
/// (a surrogate pair), else 1.
fn utf_char2utf16len(c: char) -> varnumber_T {
    if c as u32 > 0xFFFF {
        2
    } else {
        1
    }
}

/// Port of `f_strutf16len()` from `Src/strings.c` — the number of UTF-16 code
/// units in `{string}`. With `{countcc}` (arg 2) truthy composing marks are
/// counted; otherwise they are ignored.
pub fn f_strutf16len(argvars: &[typval_T], rettv: &mut typval_T) {
    let countcc = argvars.len() >= 2
        && argvars[1].v_type != VAR_UNKNOWN
        && tv_get_number_chk(&argvars[1], None) != 0;
    let n: varnumber_T = tv_get_string(&argvars[0])
        .chars()
        .filter(|c| countcc || !utf_iscomposing(*c))
        .map(utf_char2utf16len)
        .sum();
    rettv.vval = v_number(n);
}

/// Port of `f_utf16idx()` from `Src/strings.c` — the UTF-16 code-unit index of
/// the byte at `{idx}` in `{string}` (or the character at `{idx}` when
/// `{charidx}` (arg 4) is truthy). Composing marks are ignored unless
/// `{countcc}` (arg 3) is truthy. -1 if `{idx}` is past the end.
pub fn f_utf16idx(argvars: &[typval_T], rettv: &mut typval_T) {
    let s = tv_get_string(&argvars[0]);
    let idx = tv_get_number_chk(&argvars[1], None);
    if idx < 0 {
        rettv.vval = v_number(-1);
        return;
    }
    let countcc = argvars.len() >= 3
        && argvars[2].v_type != VAR_UNKNOWN
        && tv_get_number_chk(&argvars[2], None) != 0;
    let usecharidx = argvars.len() >= 4
        && argvars[3].v_type != VAR_UNKNOWN
        && tv_get_number_chk(&argvars[3], None) != 0;

    // Split the string into index-units. With {countcc} false a composing mark
    // folds into the preceding base character (contributing 0 UTF-16 units and
    // no new index), so a unit is a base char plus its trailing composing marks;
    // with {countcc} true every character is its own unit. For each unit record
    // its start byte and the number of UTF-16 code units in all preceding units.
    // The result is that "before" count for the unit whose span contains {idx}
    // (byte-index mode, or the {idx}'th unit in {charidx} mode); when {idx} lands
    // exactly at the end the total UTF-16 length is returned, and past the end -1.
    let mut units: Vec<(usize, varnumber_T)> = Vec::new();
    let mut u16_acc: varnumber_T = 0;
    let mut prev = false;
    for (b, c) in s.char_indices() {
        let folds = !countcc && prev && utf_iscomposing(c);
        prev = true;
        if !folds {
            units.push((b, u16_acc));
            u16_acc += utf_char2utf16len(c);
        }
    }
    let total_u16 = u16_acc;

    let result = if usecharidx {
        let n = units.len() as varnumber_T;
        match idx {
            i if i == n => total_u16,
            i if i > n => -1,
            i => units[i as usize].1,
        }
    } else {
        let strlen = s.len() as varnumber_T;
        match idx {
            i if i == strlen => total_u16,
            i if i > strlen => -1,
            i => units
                .iter()
                .take_while(|(sb, _)| (*sb as varnumber_T) <= i)
                .last()
                .map_or(0, |(_, before)| *before),
        }
    };
    rettv.vval = v_number(result);
}

// ── positional ($-style) printf format validation (Src/strings.c) ──

/// The `TYPE_*` enum from `Src/strings.c` — what a conversion specifier
/// consumes from the argument list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormatType {
    Unknown,
    Int,
    LongInt,
    LongLongInt,
    SignedSizeT,
    UnsignedInt,
    UnsignedLongInt,
    UnsignedLongLongInt,
    SizeT,
    Pointer,
    Percent,
    Char,
    String,
    Float,
}

/// Port of `format_typeof()` from `Src/strings.c:864` — the [`FormatType`] of
/// the conversion at the start of `type_` (a length modifier `h`/`l`/`ll`/`z`
/// followed by the specifier char, with the `i`/`*`/`D`/`U`/`O` synonyms).
pub(crate) fn format_typeof(type_: &[u8]) -> FormatType {
    let mut i = 0usize;
    // allowed values: \0, h, l, L
    let mut length_modifier = 0u8;
    if matches!(type_.first(), Some(b'h' | b'l' | b'z')) {
        length_modifier = type_[0];
        i = 1;
        if length_modifier == b'l' && type_.get(1) == Some(&b'l') {
            // double l = long long
            length_modifier = b'L';
            i = 2;
        }
    }
    let mut fmt_spec = type_.get(i).copied().unwrap_or(0);
    // common synonyms:
    match fmt_spec {
        b'i' => fmt_spec = b'd',
        b'*' => {
            fmt_spec = b'd';
            length_modifier = b'h';
        }
        b'D' => {
            fmt_spec = b'd';
            length_modifier = b'l';
        }
        b'U' => {
            fmt_spec = b'u';
            length_modifier = b'l';
        }
        b'O' => {
            fmt_spec = b'o';
            length_modifier = b'l';
        }
        _ => {}
    }
    match fmt_spec {
        b'%' => FormatType::Percent,
        b'c' => FormatType::Char,
        b's' | b'S' => FormatType::String,
        b'p' => FormatType::Pointer,
        b'b' | b'B' => FormatType::UnsignedLongLongInt,
        b'd' => match length_modifier {
            0 | b'h' => FormatType::Int,
            b'l' => FormatType::LongInt,
            b'L' => FormatType::LongLongInt,
            b'z' => FormatType::SignedSizeT,
            _ => FormatType::Unknown,
        },
        b'u' | b'o' | b'x' | b'X' => match length_modifier {
            0 | b'h' => FormatType::UnsignedInt,
            b'l' => FormatType::UnsignedLongInt,
            b'L' => FormatType::UnsignedLongLongInt,
            b'z' => FormatType::SizeT,
            _ => FormatType::Unknown,
        },
        b'f' | b'F' | b'e' | b'E' | b'g' | b'G' => FormatType::Float,
        _ => FormatType::Unknown,
    }
}

/// Port of `format_typename()` from `Src/strings.c:978` — the human name of a
/// conversion's type, used by the E1502/E1504 messages.
fn format_typename(type_: &[u8]) -> &'static str {
    match format_typeof(type_) {
        FormatType::Int => "int",
        FormatType::LongInt => "long int",
        FormatType::LongLongInt => "long long int",
        FormatType::SignedSizeT => "signed size_t",
        FormatType::UnsignedInt => "unsigned int",
        FormatType::UnsignedLongInt => "unsigned long int",
        FormatType::UnsignedLongLongInt => "unsigned long long int",
        FormatType::SizeT => "size_t",
        FormatType::Pointer => "pointer",
        FormatType::Percent => "percent",
        FormatType::Char => "char",
        FormatType::String => "string",
        FormatType::Float => "float",
        FormatType::Unknown => "unknown",
    }
}

/// Port of `adjust_types()` from `Src/strings.c:1013` — record that positional
/// argument `arg` (1-based) is consumed by the conversion at byte offset
/// `type_off` of `fmt`, erroring when the same slot is reused with a different
/// type (E1504), a `*` field-width slot is reused as a non-int (E1502), or the
/// index is not positive (E1505). `ap_types` holds the offset of each slot's
/// first use (the C's `const char **ap_types`).
fn adjust_types(ap_types: &mut Vec<Option<usize>>, arg: i32, fmt: &str, type_off: usize) -> i32 {
    if arg <= 0 {
        semsg(&format!(
            "E1505: Invalid format specifier: {}",
            &fmt[type_off..]
        ));
        return FAIL;
    }
    let idx = arg as usize - 1;
    if ap_types.len() <= idx {
        ap_types.resize(idx + 1, None);
    }
    if let Some(old_off) = ap_types[idx] {
        let old = &fmt.as_bytes()[old_off..];
        let new = &fmt.as_bytes()[type_off..];
        if old[0] == b'*' || new[0] == b'*' {
            let pt = if new[0] == b'*' { old } else { new };
            if pt[0] != b'*' && !matches!(pt[0], b'd' | b'i') {
                semsg(&format!(
                    "E1502: Positional argument {arg} used as field width reused as \
                     different type: {}/{}",
                    format_typename(old),
                    format_typename(new)
                ));
                return FAIL;
            }
        } else if format_typeof(new) != format_typeof(old) {
            semsg(&format!(
                "E1504: Positional argument {arg} type used inconsistently: {}/{}",
                format_typename(new),
                format_typename(old)
            ));
            return FAIL;
        }
    }
    ap_types[idx] = Some(type_off);
    OK
}

/// Port of `format_overflow_error()` from `Src/strings.c:1066` — E1510 naming
/// the digit run that exceeded [`MAX_ALLOWED_STRING_WIDTH`].
fn format_overflow_error(fmt: &str, pstart: usize) {
    let b = fmt.as_bytes();
    let mut p = pstart;
    while p < b.len() && b[p].is_ascii_digit() {
        p += 1;
    }
    semsg(&format!("E1510: Value too large: {}", &fmt[pstart..p]));
}

/// `MAX_ALLOWED_STRING_WIDTH` from `Src/strings.c` — 1 MiB.
const MAX_ALLOWED_STRING_WIDTH: u32 = 1048576;

/// Port of `get_unsigned_int()` from `Src/strings.c:1084` — parse the digit
/// run at `*p` into `uj`, capping at [`MAX_ALLOWED_STRING_WIDTH`] (E1510 when
/// `overflow_err`, i.e. the typval `printf()` path). NOTE (c): the first byte
/// is consumed unconditionally — `%$d` reads `'$' - '0'` and overflows, which
/// is why its error is E1510 with an empty digit run.
fn get_unsigned_int(
    fmt: &str,
    pstart: usize,
    p: &mut usize,
    uj: &mut u32,
    overflow_err: bool,
) -> i32 {
    let b = fmt.as_bytes();
    // c: `*uj = (unsigned)(**p - '0')` — int subtraction cast to unsigned, so a
    // non-digit first byte ('$' in `%$d`) wraps huge and trips the overflow check.
    *uj = (b[*p] as i32 - b'0' as i32) as u32;
    *p += 1;
    while *p < b.len() && b[*p].is_ascii_digit() && *uj < MAX_ALLOWED_STRING_WIDTH {
        *uj = 10 * *uj + (b[*p] - b'0') as u32;
        *p += 1;
    }
    if *uj > MAX_ALLOWED_STRING_WIDTH {
        if overflow_err {
            format_overflow_error(fmt, pstart);
            return FAIL;
        }
        *uj = MAX_ALLOWED_STRING_WIDTH;
    }
    OK
}

/// Port of `parse_fmt_types()` from `Src/strings.c:1101` — the pre-pass over a
/// `printf()` format that validates `$`-style (positional) conversions before
/// anything is formatted. `argc` is the number of arguments supplied after the
/// format (the C's `tvs` array length). Errors, in the C's order:
///
/// - E1505 — `0` flag on a positional index (`%01$d`), a `$` after a width or
///   precision digit run, or a `%*N` width not followed by `$`.
/// - E1510 — a width/precision/index digit run over 1 MiB.
/// - E1500 — positional and non-positional conversions mixed (also raised for
///   an *unknown* specifier carrying a positional index).
/// - E1502/E1504 — a slot reused with an incompatible type (`adjust_types`).
/// - E1501 — a slot the format never uses (`%2$d` with no `%1$…`).
/// - E1503 — a slot past the supplied arguments.
///
/// Returns FAIL after emitting the message; the caller renders nothing (the
/// C's `vim_vsnprintf_typval` returns 0 → `printf()` yields an empty string).
pub fn parse_fmt_types(fmt: &str, argc: usize) -> i32 {
    let b = fmt.as_bytes();
    let mut ap_types: Vec<Option<usize>> = Vec::new();
    let mut any_pos = false;
    let mut any_arg = false;
    let mut p = 0usize;

    // c: CHECK_POS_ARG — mixing is detected the moment both kinds have been seen.
    macro_rules! check_pos_arg {
        () => {
            if any_pos && any_arg {
                semsg(&format!(
                    "E1500: Cannot mix positional and non-positional arguments: {fmt}"
                ));
                return FAIL;
            }
        };
    }

    while p < b.len() {
        if b[p] != b'%' {
            p += 1;
            continue;
        }
        let pstart = p + 1;
        p += 1;
        // variable for positional arg
        let mut pos_arg: i32 = -1;

        // First check to see if we find a positional argument specifier
        let mut ptype = p;
        while ptype < b.len() && b[ptype].is_ascii_digit() {
            ptype += 1;
        }
        if b.get(ptype) == Some(&b'$') {
            if b[p] == b'0' {
                // 0 flag at the wrong place
                semsg(&format!("E1505: Invalid format specifier: {fmt}"));
                return FAIL;
            }
            // Positional argument
            let mut uj = 0u32;
            if get_unsigned_int(fmt, pstart, &mut p, &mut uj, true) == FAIL {
                return FAIL;
            }
            pos_arg = uj as i32;
            any_pos = true;
            check_pos_arg!();
            p += 1; // past '$'
        }

        // parse flags
        while matches!(b.get(p), Some(b'0' | b'-' | b'+' | b' ' | b'#' | b'\'')) {
            p += 1;
        }

        // parse field width
        let mut arg_off = p;
        if b.get(p) == Some(&b'*') {
            p += 1;
            if b.get(p).is_some_and(u8::is_ascii_digit) {
                // Positional argument field width
                let mut uj = 0u32;
                if get_unsigned_int(fmt, arg_off + 1, &mut p, &mut uj, true) == FAIL {
                    return FAIL;
                }
                if b.get(p) != Some(&b'$') {
                    semsg(&format!("E1505: Invalid format specifier: {fmt}"));
                    return FAIL;
                }
                p += 1;
                any_pos = true;
                check_pos_arg!();
                if adjust_types(&mut ap_types, uj as i32, fmt, arg_off) == FAIL {
                    return FAIL;
                }
            } else {
                any_arg = true;
                check_pos_arg!();
            }
        } else if b.get(p).is_some_and(u8::is_ascii_digit) {
            let digstart = p;
            let mut uj = 0u32;
            if get_unsigned_int(fmt, digstart, &mut p, &mut uj, true) == FAIL {
                return FAIL;
            }
            if b.get(p) == Some(&b'$') {
                semsg(&format!("E1505: Invalid format specifier: {fmt}"));
                return FAIL;
            }
        }

        // parse precision
        if b.get(p) == Some(&b'.') {
            p += 1;
            arg_off = p;
            if b.get(p) == Some(&b'*') {
                p += 1;
                if b.get(p).is_some_and(u8::is_ascii_digit) {
                    let mut uj = 0u32;
                    if get_unsigned_int(fmt, arg_off + 1, &mut p, &mut uj, true) == FAIL {
                        return FAIL;
                    }
                    if b.get(p) != Some(&b'$') {
                        semsg(&format!("E1505: Invalid format specifier: {fmt}"));
                        return FAIL;
                    }
                    any_pos = true;
                    check_pos_arg!();
                    p += 1;
                    if adjust_types(&mut ap_types, uj as i32, fmt, arg_off) == FAIL {
                        return FAIL;
                    }
                } else {
                    any_arg = true;
                    check_pos_arg!();
                }
            } else if b.get(p).is_some_and(u8::is_ascii_digit) {
                let digstart = p;
                let mut uj = 0u32;
                if get_unsigned_int(fmt, digstart, &mut p, &mut uj, true) == FAIL {
                    return FAIL;
                }
                if b.get(p) == Some(&b'$') {
                    semsg(&format!("E1505: Invalid format specifier: {fmt}"));
                    return FAIL;
                }
            }
        }

        if pos_arg != -1 {
            any_pos = true;
            check_pos_arg!();
            ptype = p;
        }

        // parse 'h', 'l', 'll' and 'z' length modifiers
        if matches!(b.get(p), Some(b'h' | b'l' | b'z')) {
            let length_modifier = b[p];
            p += 1;
            if length_modifier == b'l' && b.get(p) == Some(&b'l') {
                // double l = long long
                p += 1;
            }
        }

        match b.get(p) {
            // Check for known format specifiers. % is special!
            Some(
                b'i' | b'*' | b'd' | b'u' | b'o' | b'D' | b'U' | b'O' | b'x' | b'X' | b'b' | b'B'
                | b'c' | b's' | b'S' | b'p' | b'f' | b'F' | b'e' | b'E' | b'g' | b'G',
            ) => {
                if pos_arg != -1 {
                    if adjust_types(&mut ap_types, pos_arg, fmt, ptype) == FAIL {
                        return FAIL;
                    }
                } else {
                    any_arg = true;
                    check_pos_arg!();
                }
            }
            _ => {
                if pos_arg != -1 {
                    semsg(&format!(
                        "E1500: Cannot mix positional and non-positional arguments: {fmt}"
                    ));
                    return FAIL;
                }
            }
        }

        if p < b.len() {
            p += 1; // step over the just processed conversion specifier
        }
    }

    // c: an unused slot is E1501; a used slot past the supplied arguments
    // (tvs[idx].v_type == VAR_UNKNOWN) is E1503 — checked per index, ascending.
    for idx in 0..ap_types.len() {
        if ap_types[idx].is_none() {
            semsg(&format!(
                "E1501: format argument {} unused in $-style format: {fmt}",
                idx + 1
            ));
            return FAIL;
        }
        if idx >= argc {
            semsg(&format!(
                "E1503: Positional argument {} out of bounds: {fmt}",
                idx + 1
            ));
            return FAIL;
        }
    }

    OK
}
