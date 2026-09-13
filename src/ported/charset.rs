//! Port of `vim_str2nr()`, the `STR2NR_*` flags and the `transchar` display
//! family from `vendor/charset.c`.
//!
//! `vim_str2nr` is the extern dependency `tv_get_number_chk()` calls. The
//! `transchar*` / `transstr*` group is the *display* transform: the rule that
//! turns a byte a terminal cannot show into the `^A` / `<ff>` / `<200b>` text
//! Vim actually writes. It is what `:echo` (via `msg_outtrans_len`, see
//! [`crate::ported::message`]) and `strtrans()` are both defined in terms of.
//!
//! Signatures mirror the C out-parameter form with `Option<&mut …>`, except that
//! the `transchar*` functions return an owned `Vec<u8>` instead of writing into
//! the file-static `transchar_charbuf[11]`: the C reuses one buffer to avoid an
//! allocation and every caller copies out of it before the next call, which is a
//! constraint a Rust return value does not have.
#![allow(non_upper_case_globals)]

use crate::ported::eval::typval_defs_h::{varnumber_T, VARNUMBER_MAX, VARNUMBER_MIN};
use crate::ported::mbyte::{
    mb_islower, mb_isupper, utf_char2cells, utf_class_tab, utf_printable, utf_ptr2cells,
    utf_ptr2char, utf_ptr2len, utfc_ptr2len,
};
use crate::vimstr::VimStr;

/// `STR2NR_BIN` — recognize a `0b`/`0B` binary prefix. (charset.h)
pub const STR2NR_BIN: i32 = 0x01;
/// `STR2NR_OCT` — recognize a leading-zero octal number.
pub const STR2NR_OCT: i32 = 0x02;
/// `STR2NR_HEX` — recognize a `0x`/`0X` hex prefix.
pub const STR2NR_HEX: i32 = 0x04;
/// `STR2NR_OOCT` — recognize a `0o`/`0O` octal prefix.
pub const STR2NR_OOCT: i32 = 0x08;
/// `STR2NR_QUOTE` — skip embedded `'` digit separators (`1'000` → 1000).
pub const STR2NR_QUOTE: i32 = 0x10;
/// `STR2NR_ALL` — recognize all of the above prefixes.
pub const STR2NR_ALL: i32 = STR2NR_BIN | STR2NR_OCT | STR2NR_HEX | STR2NR_OOCT;
/// `STR2NR_FORCE` — force the base selected by the radix bits in `what`
/// regardless of any prefix (set by `str2nr({expr}, {base})`). (charset.h)
pub const STR2NR_FORCE: i32 = 0x80;

/// Port of `vim_str2nr()` from `vendor/charset.c:1219`.
///
/// Convert the leading numeric prefix of `start` to a number. An optional sign,
/// then a radix prefix selected by `what`, then the longest run of digits valid
/// in that radix. `prep` receives the detected base char (0/'b'/'o'/'x'), `len`
/// the number of consumed bytes, `nptr` the signed value, `unptr` the unsigned
/// magnitude. `maxlen == 0` means no limit. `strict`/`overflow` are accepted for
/// signature fidelity; overflow saturates here.
#[allow(clippy::too_many_arguments)]
pub fn vim_str2nr(
    start: &str,
    prep: Option<&mut i32>,
    len: Option<&mut i32>,
    what: i32,
    nptr: Option<&mut varnumber_T>,
    unptr: Option<&mut u64>,
    maxlen: i32,
    _strict: bool,
    _overflow: Option<&mut i32>,
) {
    let bytes = start.as_bytes();
    let mut ptr = 0usize; // c: const char *ptr = start;
    let mut negative = false; // c: bool negative = false;
    let cap = if maxlen <= 0 {
        bytes.len()
    } else {
        (maxlen as usize).min(bytes.len())
    };

    // c:1228 `const bool negative = (ptr[0] == '-');` then `c:1235 if (negative)
    // ptr++;` — a leading MINUS and nothing else. `+` is NOT a sign here, and
    // that is load-bearing rather than an oversight: this function is what
    // `tv_get_number` uses to coerce a String, so `'+7' + 0` is 0 in both
    // engines. `str2nr()` accepts `+` because `f_str2nr` strips the sign itself
    // (`vendor/.../strings.c`) before calling in — see `strings.rs:f_str2nr`.
    if ptr < cap && bytes[ptr] == b'-' {
        negative = true;
        ptr += 1;
    }

    // c: detect the base from the prefix
    let mut pre = 0u8; // c: int pre = 0;  // default decimal
    let mut base: u64 = 10;
    if (what & STR2NR_FORCE) != 0 {
        // c: STR2NR_FORCE — the radix bit in `what` dictates the base; a matching
        // prefix is consumed if present, but is not required.
        base = if what & STR2NR_HEX != 0 {
            16
        } else if what & (STR2NR_OCT | STR2NR_OOCT) != 0 {
            8
        } else if what & STR2NR_BIN != 0 {
            2
        } else {
            10
        };
        if ptr + 1 < cap && bytes[ptr] == b'0' {
            let c = bytes[ptr + 1];
            let pfx = (base == 16 && (c == b'x' || c == b'X'))
                || (base == 2 && (c == b'b' || c == b'B'))
                || (base == 8 && (c == b'o' || c == b'O'));
            if pfx {
                pre = c;
                ptr += 2;
            }
        }
    } else if ptr < cap && bytes[ptr] == b'0' && ptr + 1 < cap {
        match bytes[ptr + 1] {
            b'x' | b'X' if (what & STR2NR_HEX) != 0 => {
                pre = bytes[ptr + 1];
                base = 16;
                ptr += 2;
            }
            b'b' | b'B' if (what & STR2NR_BIN) != 0 => {
                pre = bytes[ptr + 1];
                base = 2;
                ptr += 2;
            }
            b'o' | b'O' if (what & STR2NR_OOCT) != 0 => {
                pre = bytes[ptr + 1];
                base = 8;
                ptr += 2;
            }
            b'0'..=b'7' if (what & STR2NR_OCT) != 0 => {
                pre = b'0';
                base = 8;
                // leading 0, digits start at ptr+1 conceptually; keep ptr on 0
                ptr += 1;
            }
            _ => {}
        }
    }

    // c: accumulate digits valid in `base`
    let mut un: u64 = 0; // c: uvarnumber_T un = 0;
    let mut saw_digit = false;
    let digit_val = |c: u8| -> Option<u64> {
        match c {
            b'0'..=b'9' => Some((c - b'0') as u64),
            b'a'..=b'f' if base == 16 => Some((c - b'a' + 10) as u64),
            b'A'..=b'F' if base == 16 => Some((c - b'A' + 10) as u64),
            _ => None,
        }
        .filter(|&d| d < base)
    };
    while ptr < cap {
        // c: with STR2NR_QUOTE, a `'` between two digits is a separator: skip it
        // only when the next char is itself a valid digit (a trailing `'` ends
        // the number).
        if (what & STR2NR_QUOTE) != 0
            && bytes[ptr] == b'\''
            && ptr + 1 < cap
            && digit_val(bytes[ptr + 1]).is_some()
        {
            ptr += 1;
            continue;
        }
        let Some(d) = digit_val(bytes[ptr]) else {
            break;
        };
        un = un.saturating_mul(base).saturating_add(d);
        saw_digit = true;
        ptr += 1;
    }
    let _ = saw_digit;

    if let Some(p) = prep {
        *p = pre as i32;
    }
    if let Some(l) = len {
        *l = ptr as i32;
    }
    if let Some(u) = unptr {
        *u = un;
    }
    if let Some(n) = nptr {
        // c: clamp the unsigned magnitude to the signed range before applying
        // the sign — negative overflow → VARNUMBER_MIN, positive → VARNUMBER_MAX.
        *n = if negative {
            if un > VARNUMBER_MAX as u64 {
                VARNUMBER_MIN
            } else {
                -(un as varnumber_T)
            }
        } else if un > VARNUMBER_MAX as u64 {
            VARNUMBER_MAX
        } else {
            un as varnumber_T
        };
    }
}

// ─── the display transform: g_chartab[] and the transchar family ─────────────

/// `CT_CELL_MASK` (`vendor/charset.c:51`) — mask: nr of display cells (1, 2 or 4).
const CT_CELL_MASK: u8 = 0x07;
/// `CT_PRINT_CHAR` (`vendor/charset.c:52`) — flag: set for printable chars.
const CT_PRINT_CHAR: u8 = 0x10;
/// `CT_FNAME_CHAR` (`vendor/charset.c:54`) — flag: set for file name chars.
const CT_FNAME_CHAR: u8 = 0x40;

/// Port of the `global` half of `buf_init_chartab()` (`vendor/charset.c:87`) —
/// `g_chartab[]`, the per-byte "how wide is it / is it printable" table.
///
/// The C then walks 'isident', 'isprint', 'isfname' and 'iskeyword' through
/// `parse_isopt()`. With Vim's DEFAULT option values that pass cannot change a
/// single bit this port reads: only the `var == p_isp` arm touches
/// `CT_PRINT_CHAR`/`CT_CELL_MASK`, it is guarded by `if (c < ' ' || c > '~')`,
/// and the default `'isprint'` is `"@,161-255"` — 161-255 are already printable
/// from the loop below, and the `@` (isalpha over 1-255) adds only ASCII letters
/// and Latin-1 letters, all of which are printable already. The other three arms
/// set `CT_ID_CHAR`/`CT_FNAME_CHAR` and `buf->b_chartab`, which nothing here
/// reads. So the table is computed once, from the defaults; vimlrs has no
/// `:set isprint` to invalidate it.
///
/// `dy_flags & kOptDyFlagUhex` is likewise the default (`'display'` has no
/// `"uhex"`), so an unprintable byte is 2 cells (`^X`), not 4 (`<xx>`).
fn buf_init_chartab() -> [u8; 256] {
    let mut tab = [0u8; 256];
    // c: from <Space> to '~' is 1 (printable), others are 2 (not printable).
    let mut c = 0usize;
    while c < b' ' as usize {
        tab[c] = 2;
        c += 1;
    }
    while c <= b'~' as usize {
        tab[c] = 1 + CT_PRINT_CHAR;
        c += 1;
    }
    while c < 256 {
        tab[c] = if c >= 0xa0 {
            // c: UTF-8: bytes 0xa0 - 0xff are printable (latin1). Also assume
            // that every multi-byte char is a filename character.
            (CT_PRINT_CHAR | CT_FNAME_CHAR) + 1
        } else {
            2
        };
        c += 1;
    }
    tab
}

/// `g_chartab[256]` (`vendor/charset.c:48`). The C fills it from
/// `init_chartab()` at startup; here it is the constant the defaults produce.
static G_CHARTAB: std::sync::LazyLock<[u8; 256]> = std::sync::LazyLock::new(buf_init_chartab);

/// `'iskeyword'`'s default value for a normal buffer (`option_vars.h`'s
/// `p_isk` initialiser): the `@` alpha class, the digits, `_`, and the Latin-1
/// letters.
const P_ISK_DEFAULT: &str = "@,48-57,_,192-255";

/// Port of the `p_isk` arm of `parse_isopt()` (`vendor/charset.c:157`) —
/// `b_chartab[]`, the per-byte "is this a keyword character" bitmap that
/// `'iskeyword'` fills.
///
/// The C parses `var` into single characters, ranges (`192-255`) and the
/// `@` shorthand, which stands for "1 through 255, but only where
/// `mb_islower()` or `mb_isupper()` says the byte is a letter" — the `@` is
/// why `µ` (0xb5) is a keyword character while `ª` (0xaa) is not, even though
/// neither is in the `192-255` range. The `^` negation prefix, the other three
/// option arms (`p_isi`/`p_isp`/`p_isf`) and the error returns are not reached
/// by the default value and are not ported here; vimlrs has no `:set
/// iskeyword` to re-run this with.
fn parse_isopt(var: &str) -> [bool; 256] {
    let mut tab = [false; 256];
    for part in var.split(',') {
        let mut chars = part.chars();
        let Some(c) = chars.next() else { continue };
        // c: the `c == '@'` arm — a single '@' (not "@-@").
        let (mut c, c2, do_isalpha) = if part == "@" {
            (1u32, 255u32, true)
        } else {
            let first = c as u32;
            match part.split_once('-') {
                // c: `c2` is read the same way as `c`; both are decimal when
                // they start with a digit.
                Some((lo, hi)) if !lo.is_empty() && !hi.is_empty() => (
                    lo.parse::<u32>().unwrap_or(first),
                    hi.parse::<u32>()
                        .unwrap_or_else(|_| hi.chars().next().map_or(first, |ch| ch as u32)),
                    false,
                ),
                _ => {
                    let n = part.parse::<u32>().unwrap_or(first);
                    (n, n, false)
                }
            }
        };

        while c <= c2 && c < 256 {
            // c: use the MB_ functions here, because isalpha() doesn't work
            // properly when 'encoding' is "latin1" and the locale is "C".
            let ch = char::from_u32(c).unwrap_or('\0');
            if !do_isalpha || mb_islower(ch) || mb_isupper(ch) {
                // c: (var == p_isk) — SET_CHARTAB(buf, c).
                tab[c as usize] = true;
            }
            c += 1;
        }
    }
    tab
}

/// `b_chartab[]` (`vendor/buffer_defs.h`) for the one buffer vimlrs has, filled
/// from the default `'iskeyword'`.
static B_CHARTAB: std::sync::LazyLock<[bool; 256]> =
    std::sync::LazyLock::new(|| parse_isopt(P_ISK_DEFAULT));

/// Port of `vim_iswordc_tab()` from `vendor/charset.c:804` — is `c` a keyword
/// character?
///
/// Below `0x100` this is `'iskeyword'`; at or above it the answer is
/// [`crate::ported::mbyte::utf_class_tab`]'s built-in rules, where any class of
/// `2` or more is a word character.
pub fn vim_iswordc_tab(c: i32) -> bool {
    if c >= 0x100 {
        utf_class_tab(c) >= 2
    } else {
        c > 0 && B_CHARTAB[c as usize]
    }
}

/// Port of `vim_iswordc()` from `vendor/charset.c:792` — [`vim_iswordc_tab`]
/// for the current buffer.
pub fn vim_iswordc(c: i32) -> bool {
    vim_iswordc_tab(c)
}

/// Port of `vim_isprintc()` from `vendor/charset.c:891` — is `c` a character
/// that can be shown as itself?
///
/// Note `c > 0`: NUL is NOT printable, and `c >= 0x100` defers to
/// `utf_printable()`, so U+200B (ZERO WIDTH SPACE) is unprintable and echoes as
/// `<200b>` while U+110000 is printable and echoes as its four raw bytes.
pub fn vim_isprintc(c: i32) -> bool {
    if c >= 0x100 {
        return utf_printable(c);
    }
    c > 0 && (G_CHARTAB[c as usize] & CT_PRINT_CHAR) != 0
}

/// Port of `byte2cells()` from `vendor/charset.c:694` — display cells for one
/// byte. 0 for a byte >= 0x80, because there the width depends on the bytes
/// that follow.
pub fn byte2cells(b: i32) -> i32 {
    if b >= 0x80 {
        return 0;
    }
    (G_CHARTAB[b as usize] & CT_CELL_MASK) as i32
}

/// Port of `char2cells()` from `vendor/charset.c:711` — display cells for one
/// character. Below 0x80 the answer is `g_chartab[]` (a control character is two
/// cells, `^X`); at or above it, [`utf_char2cells`].
///
/// The C's leading `IS_SPECIAL(c)` arm is for the negative `K_*` key codes that
/// only the terminal input layer produces; nothing in this crate passes one.
pub fn char2cells(c: i32) -> i32 {
    if c >= 0x80 {
        // c: UTF-8: above 0x80 need to check the value
        return utf_char2cells(c);
    }
    (G_CHARTAB[(c & 0xff) as usize] & CT_CELL_MASK) as i32
}

/// Port of `ptr2cells()` from `vendor/charset.c:730` — display cells for the
/// character at the start of `p`. A TAB counts as the two cells of `^I`; it is
/// [`win_chartabsize`] that knows a TAB pads to the next `'tabstop'`.
pub fn ptr2cells(p: &[u8]) -> i32 {
    let b = p.first().copied().unwrap_or(0);
    // c: For UTF-8 we need to look at more bytes if the first byte is >= 0x80.
    if b >= 0x80 {
        return utf_ptr2cells(p);
    }
    (G_CHARTAB[b as usize] & CT_CELL_MASK) as i32
}

/// Port of `tabstop_padding()` (vim `indent.c`) for the no-`'vartabstop'` case
/// the C's `RET_WIN_BUF_CHARTABSIZE` macro spells out: `ts - (col % ts)`, the
/// cells a TAB at virtual column `col` takes to reach the next tab stop.
fn tabstop_padding(col: i64, ts: i64) -> i64 {
    let ts = if ts <= 0 { 8 } else { ts };
    ts - col % ts
}

/// Port of `win_chartabsize()` from `vendor/plines.c:48` — the cells the
/// character at `p` takes when it starts at virtual column `col`.
///
/// The window is the C's `wp`; a string match has none, and every option the
/// macro consults (`'list'`, `'listchars'`) is at its default, so the TAB arm is
/// taken and the padding comes from `'tabstop'`.
pub fn win_chartabsize(p: &[u8], col: i64) -> i64 {
    // c: if (*p == TAB && (!wp->w_p_list || wp->w_p_lcs_chars.tab1))
    if p.first() == Some(&b'\t') {
        // c: `buf->b_p_ts` — read through the option table on every call, so
        // `:set ts=4` takes effect at once.
        let ts = crate::ported::eval::typval::tv_get_number_chk(
            &crate::ported::option::get_option_value("tabstop"),
            None,
        );
        return tabstop_padding(col, ts);
    }
    ptr2cells(p) as i64
}

/// Port of `linesize_fast()` from `vendor/plines.c:473` (vim's
/// `win_linetabsize_cts()`, `charset.c:913`) — the shared loop under every
/// "how wide is this line" question: walk the clusters, adding
/// [`win_chartabsize`] of each, and stop at byte `len` or at `MAXCOL`.
///
/// The `vcol`/`vcol_arg` pair is the C's: `vcol` is an `int64_t` that may
/// overflow past `MAXCOL`, `vcol_arg` is the clamped `int` that is returned and
/// that the next character's tab stop is measured from.
fn linesize_fast(line: &[u8], vcol_arg: i64, len: usize) -> i64 {
    const MAXCOL: i64 = 0x7fff_ffff; // c: enum { MAXCOL } — `vendor/pos_defs.h:19`
    let mut vcol_arg = vcol_arg;
    let mut vcol: i64 = vcol_arg;
    let mut p = 0usize;
    while p < len && p < line.len() {
        vcol += win_chartabsize(&line[p..], vcol_arg);
        p += crate::ported::mbyte::utfc_ptr2len(&line[p..]).max(1) as usize;
        if vcol > MAXCOL {
            vcol_arg = MAXCOL;
            break;
        }
        vcol_arg = vcol;
    }
    vcol_arg
}

/// Port of `linetabsize_col()` from `vendor/plines.c:63` — the screen width of
/// `s` when it starts at virtual column `startvcol`. This is what
/// `strdisplaywidth({string}, {col})` measures, minus `{col}`.
pub fn linetabsize_col(startvcol: i64, s: &[u8]) -> i64 {
    linesize_fast(s, startvcol, usize::MAX)
}

/// Port of `win_linetabsize()` from vim `charset.c:841` (`vendor/plines.c:79`
/// names it at the call site) — the screen width of the first `len` bytes of
/// `line`, which is what `\%23v` compares against.
///
/// RUST-PORT NOTE: the C reaches `win_lbr_chartabsize()`, which for a window
/// with `'linebreak'`, `'breakindent'` and `'showbreak'` all off reduces to
/// `win_nolbr_chartabsize()`, and that in turn to [`win_chartabsize`] plus one
/// cell for a double-width character sitting in the window's last column
/// (`in_win_border()`). A string match has no window, `w_width` is 0, and
/// `in_win_border()` answers FALSE for such a window by its own first test — so
/// the border arm is unreachable here, not skipped.
pub fn win_linetabsize(line: &[u8], len: usize) -> i64 {
    linesize_fast(line, 0, len)
}

/// Port of `nr2hex()` from `vendor/charset.c:674` — the lower 4 bits of `n` as a
/// hex character. Lower case, as the C says, "to avoid the confusion of <F1>
/// being 0xf1 or function key 1".
fn nr2hex(n: u32) -> u8 {
    if (n & 0xf) <= 9 {
        (n & 0xf) as u8 + b'0'
    } else {
        (n & 0xf) as u8 - 10 + b'a'
    }
}

/// Port of `transchar_hex()` from `vendor/charset.c:634` — a non-printable
/// character as `<ff>` / `<200b>` / `<110000>`. Appends to `buf`; the C's return
/// value is the number of bytes written, which `transstr_len()` uses to size its
/// allocation.
pub fn transchar_hex(buf: &mut Vec<u8>, c: i32) -> usize {
    let start = buf.len();
    let u = c as u32;
    buf.push(b'<');
    if c > 0xFF {
        if c > 0xFFFF {
            buf.push(nr2hex(u >> 20));
            buf.push(nr2hex(u >> 16));
        }
        buf.push(nr2hex(u >> 12));
        buf.push(nr2hex(u >> 8));
    }
    buf.push(nr2hex(u >> 4));
    buf.push(nr2hex(u));
    buf.push(b'>');
    buf.len() - start
}

/// Port of `transchar_nonprint()` from `vendor/charset.c:604` — a non-printable
/// character as the 2..4 printable ones Vim shows.
///
/// `buf` is the C's `const buf_T *`, used only to ask whether `'fileformat'` is
/// `mac` (where a CR stands in for a NL). Every caller in this port passes the
/// C's `NULL`, which skips that arm — vimlrs has no buffer with a fileformat.
/// The `c == NL → NUL` rewrite is kept: it is why `strtrans("a\nb")` is `a^@b`.
pub fn transchar_nonprint(charbuf: &mut Vec<u8>, mut c: i32) {
    // c: if (c == NL) { c = NUL; }  // we use newline in place of a NUL
    if c == 0x0a {
        c = 0;
    }
    debug_assert!(c <= 0xff);
    // c: `dy_flags & kOptDyFlagUhex` is the default (off) — see buf_init_chartab.
    if c > 0x7f {
        transchar_hex(charbuf, c);
    } else {
        // c: 0x00 - 0x1f and 0x7f; DEL displayed as ^?
        charbuf.push(b'^');
        charbuf.push((c ^ 0x40) as u8);
    }
}

/// Port of `transchar_buf()` from `vendor/charset.c:541` — translate one
/// *character* into printable text, leaving printable ASCII intact.
///
/// The C opens with an `IS_SPECIAL(c)` arm (`(c) < 0`, `vendor/keycodes.h:25`)
/// that emits a `~@` prefix and continues with `K_SECOND(c)`. It is NOT ported:
/// no caller here can reach it — `c` arrives either from `utf_ptr2char()` or
/// from a byte, and is never negative — and `K_SECOND` resolves against the
/// `K_SPECIAL`/`KS_ZERO` key-code constants, which this port has no producer
/// for (see `crate::ported::keycodes`: "K_SPECIAL byte sequences, which this
/// port never produces"). A guessed stand-in would be a wrong answer sitting
/// behind an unreachable branch.
///
/// `chartab_initialized` is true by the time anything calls this (the C's own
/// `!chartab_initialized` disjunct is a pre-`init_chartab()` fallback), so the
/// printable test is exactly `(c <= 0xFF) && vim_isprintc(c)`.
pub fn transchar_buf(c: i32) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    debug_assert!(
        c >= 0,
        "IS_SPECIAL(c) arm is not ported — see the doc above"
    );
    if c <= 0xFF && vim_isprintc(c) {
        out.push(c as u8);
    } else if c <= 0xFF {
        transchar_nonprint(&mut out, c);
    } else {
        transchar_hex(&mut out, c);
    }
    out
}

/// Port of `transchar_byte_buf()` from `vendor/charset.c:585` — like
/// [`transchar_buf`] but called with a BYTE.
///
/// This is the difference that makes `echo list2str([-1])` print `<ff>`: a lone
/// byte >= 0x80 is an illegal UTF-8 byte, so it goes straight to hex instead of
/// being asked whether the *character* 0xff is printable (it is — `<ff>` is
/// still what Vim shows, because the byte is not a character here).
pub fn transchar_byte_buf(c: i32) -> Vec<u8> {
    if c >= 0x80 {
        let mut out = Vec::with_capacity(4);
        transchar_nonprint(&mut out, c);
        return out;
    }
    transchar_buf(c)
}

/// Port of `transstr_buf()` from `vendor/charset.c:351` — replace special
/// characters in `s` with printable ones.
///
/// The C's `buf`/`buflen` are the caller-sized output `transstr()` allocates
/// after measuring with `transstr_len()`; a growable `Vec` replaces both, so the
/// "exceeded buf size" breaks have no counterpart and `transstr_len` has no
/// caller left to serve.
pub fn transstr_buf(s: &[u8], untab: bool) -> VimStr {
    let mut out = VimStr::new();
    let mut p = 0usize;
    // c: `while (… && *p != NUL && …)` — the C walks a `char *`, so a NUL ends
    // the walk. `strtrans(list2str([65, 0, 66]))` is `A`, not `A^@B`.
    while p < s.len() && s[p] != 0 {
        let l = utfc_ptr2len(&s[p..]) as usize;
        if l > 1 {
            if vim_isprintc(utf_ptr2char(&s[p..])) {
                out.push_bytes(&s[p..p + l]);
            } else {
                // c: an unprintable cluster is hexed one character at a time, so
                // a base + composing pair becomes two `<….>` groups.
                let mut off = 0usize;
                while off < l {
                    let c = utf_ptr2char(&s[p + off..]);
                    let mut hexbuf = Vec::with_capacity(8);
                    transchar_hex(&mut hexbuf, c);
                    out.push_bytes(&hexbuf);
                    off += utf_ptr2len(&s[p + off..]).max(1) as usize;
                }
            }
            p += l;
        } else if s[p] == b'\t' && !untab {
            out.push_bytes(&s[p..p + 1]);
            p += 1;
        } else {
            out.push_bytes(&transchar_byte_buf(s[p] as i32));
            p += 1;
        }
    }
    out
}

/// Port of `transstr()` from `vendor/charset.c:406` — copy `s`, replacing
/// special characters with printable ones. What `strtrans()` is.
pub fn transstr(s: &[u8], untab: bool) -> VimStr {
    transstr_buf(s, untab)
}
