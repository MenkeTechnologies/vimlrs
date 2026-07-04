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
        return; // c: emsg(e_invarg); leaves rettv at 0
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
    let mut len = if argvars.len() >= 3 {
        tv_get_number_chk(&argvars[2], None)
    } else {
        slen - nbyte
    };
    // c: a negative start clamps to 0 but folds its offset into the length, so
    // strpart('hello', -2, 3) keeps only the first character.
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
    // c: with {chars} ({4}) set, reinterpret the byte-clamped {len} as a
    // character count and walk that many characters forward from `nbyte`
    // (the {start} offset itself stays byte-based, matching the C).
    if argvars.len() >= 3 && argvars.get(3).is_some_and(|t| t.v_type != VAR_UNKNOWN) {
        let mut off = nbyte;
        while off < slen && len > 0 {
            off += crate::ported::mbyte::utf_ptr2len(&bytes[off as usize..]) as varnumber_T;
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
    let idx = if start <= hay.len() {
        hay[start..]
            .find(&needle)
            .map(|i| (i + start) as varnumber_T)
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
    // c: {mask} (default = isspace() set); an explicit empty mask trims nothing.
    let has_mask = argvars.len() >= 2 && argvars[1].v_type != VAR_UNKNOWN;
    let mask: Vec<char> = if has_mask {
        tv_get_string(&argvars[1]).chars().collect()
    } else {
        vec![' ', '\t', '\r', '\n', '\u{0b}', '\u{0c}']
    };
    let pred = |c: char| mask.contains(&c);
    // c: {dir} — 0 = both ends (default), 1 = leading only, 2 = trailing only.
    let dir = argvars
        .get(2)
        .filter(|t| t.v_type != VAR_UNKNOWN)
        .map_or(0, |t| tv_get_number_chk(t, None));
    let trimmed = match dir {
        1 => s.trim_start_matches(pred),
        2 => s.trim_end_matches(pred),
        _ => s.trim_matches(pred),
    };
    rettv.vval = v_string(trimmed.to_string());
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
    for c in src.chars() {
        match from.iter().position(|&f| f == c) {
            Some(i) => match to.get(i) {
                Some(&t) => out.push(t),
                None => {
                    crate::ported::message::semsg(&format!("E475: Invalid argument: {fromstr}"));
                    rettv.vval = v_string(String::new());
                    return;
                }
            },
            None => out.push(c),
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
    if idx < 0 || idx as usize >= s.len() {
        rettv.vval = v_number(-1);
        return;
    }
    let idx = idx as usize;
    // c: {countcc} (argvars[2]) — default 0 folds composing characters into their
    // base character (so the index is of the base); 1 counts each separately.
    let countcc = argvars
        .get(2)
        .is_some_and(|t| tv_get_number_chk(t, None) != 0);
    // Character index of the character whose byte span contains byte `idx`, via
    // char boundaries (a non-boundary `idx` maps to its containing char and never
    // panics). With folding, a composing char does not advance the index.
    let mut ci: varnumber_T = -1;
    let mut prev = false;
    for (b, c) in s.char_indices() {
        if b > idx {
            break;
        }
        let folds = !countcc && prev && utf_iscomposing(c);
        if !folds {
            ci += 1;
        }
        prev = true;
    }
    rettv.vval = v_number(ci);
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
        let u = c as u32;
        if u < 0x20 {
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
    // Length of the sliced value, by type.
    let len: varnumber_T = match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(Some(l))) => l.borrow().lv_items.len() as varnumber_T,
        (VAR_BLOB, v_blob(Some(b))) => b.borrow().bv_ga.len() as varnumber_T,
        (VAR_LIST, _) | (VAR_BLOB, _) => 0,
        _ => tv_get_string(&argvars[0]).chars().count() as varnumber_T,
    };

    let clamp = |mut i: varnumber_T| -> varnumber_T {
        if i < 0 {
            i += len;
        }
        i.clamp(0, len)
    };
    let s = clamp(tv_get_number_chk(&argvars[1], None));
    let has_end = argvars.len() >= 3 && argvars[2].v_type != VAR_UNKNOWN;
    let mut e = if has_end {
        clamp(tv_get_number_chk(&argvars[2], None))
    } else {
        len
    };
    if e < s {
        e = s;
    }
    let (s, e) = (s as usize, e as usize);

    match (argvars[0].v_type, &argvars[0].vval) {
        (VAR_LIST, v_list(l)) => {
            let out = tv_list_alloc_ret(rettv, 0);
            if let Some(l) = l {
                let lb = l.borrow();
                let mut ob = out.borrow_mut();
                for it in &lb.lv_items[s..e] {
                    tv_list_append_tv(&mut ob, it.li_tv.clone());
                }
            }
        }
        (VAR_BLOB, v_blob(b)) => {
            let out = tv_blob_alloc_ret(rettv);
            if let Some(b) = b {
                out.borrow_mut()
                    .bv_ga
                    .extend_from_slice(&b.borrow().bv_ga[s..e]);
            }
        }
        _ => {
            let chars: Vec<char> = tv_get_string(&argvars[0]).chars().collect();
            rettv.v_type = VAR_STRING;
            rettv.vval = v_string(chars[s..e].iter().collect());
        }
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
    let col0 = if argvars.len() >= 2 {
        tv_get_number_chk(&argvars[1], None).max(0) as usize
    } else {
        0
    };
    let ts = {
        let t = tv_get_number_chk(&get_option_value("tabstop"), None);
        if t > 0 {
            t as usize
        } else {
            8
        }
    };
    let mut col = col0;
    for c in s.chars() {
        if c == '\t' {
            col += ts - (col % ts);
        } else {
            col += utf_char2cells(c);
        }
    }
    rettv.vval = v_number((col - col0) as varnumber_T);
}

/// Port of `f_charclass()` from `Src/strings.c` — the character class of the
/// first character of `{string}`: 0 blank, 1 punctuation, 2 word character, 3
/// emoji. 0 for an empty String.
pub fn f_charclass(argvars: &[typval_T], rettv: &mut typval_T) {
    let class = match tv_get_string(&argvars[0]).chars().next() {
        None => 0,
        Some(c) if c == ' ' || c == '\t' || c == '\0' => 0,
        Some(c) if matches!(c as u32, 0x1F300..=0x1FAFF | 0x2600..=0x27BF) => 3,
        Some(c) if c == '_' || c.is_alphanumeric() => 2,
        Some(_) => 1,
    };
    rettv.vval = v_number(class as varnumber_T);
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
    let charidx = argvars.len() >= 4
        && argvars[3].v_type != VAR_UNKNOWN
        && tv_get_number_chk(&argvars[3], None) != 0;
    let target = idx as usize;

    let mut byte_pos = 0usize;
    let mut char_pos = 0usize;
    let mut u16_pos: varnumber_T = 0;
    for c in s.chars() {
        let pos = if charidx { char_pos } else { byte_pos };
        if pos >= target {
            rettv.vval = v_number(u16_pos);
            return;
        }
        if countcc || !utf_iscomposing(c) {
            u16_pos += utf_char2utf16len(c);
            char_pos += 1;
        }
        byte_pos += c.len_utf8();
    }
    // idx exactly at the end → the total length; past the end → -1.
    let end = if charidx { char_pos } else { byte_pos };
    rettv.vval = v_number(if target == end { u16_pos } else { -1 });
}
