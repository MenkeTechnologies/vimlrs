//! Port of the UTF-8 codec helpers from `src/nvim/mbyte.c` (vendored at
//! `vendor/mbyte.c`).
//!
//! Ported here are the routines the rest of the crate reaches for: the codec
//! (`utf_ptr2char`, `utf_ptr2len`, `utf_char2len`, `utf_char2bytes` and the
//! `utf8len_tab` they share, which is what the JSON decoder in `eval/decode.c`
//! needs), the cluster walk (`utfc_ptr2len`, `utf_head_off`, `mb_charlen`), the
//! case folds, and `utf_printable` — the "can this be displayed as itself"
//! predicate that `vim_isprintc` consults above 0xFF, and so the reason
//! `echo nr2char(0x200b)` is `<200b>`.
//!
//! Screen-cell width is ported here too — `utf_char2cells`, `utf_ptr2cells` and
//! `mb_string2cells`, with vim's `doublewidth`/`emoji_wide`/`ambiguous` interval
//! tables, because Neovim's answer to the same question is a utf8proc property
//! query and utf8proc is not a dependency of this crate.
//!
//! The encoding-name tables, `enc_canonize()` and the conversion `iconv()` is
//! built on are ported at the end; a conversion that is not latin1/latin9 <->
//! UTF-8 goes through the C library's iconv(3), as in the C.
//!
//! RUST-PORT NOTE: C walks `const char *` pointers into a NUL-terminated buffer,
//! so reads past the last byte land on the terminating NUL (`0x00`). Here the
//! byte-slice ports read out-of-range indices as `0`, reproducing that exact
//! behaviour (a truncated multibyte tail fails the `& 0xC0 == 0x80` check and
//! the lead byte is returned unchanged, matching C).
#![allow(non_upper_case_globals, clippy::needless_range_loop)]

/// Port of `utf8len_tab[]` from `Src/mbyte.c:106` — byte length of a UTF-8
/// character keyed by its first byte. Illegal lead bytes and NUL map to 1.
pub const utf8len_tab: [u8; 256] = [
    // ?1 ?2 ?3 ?4 ?5 ?6 ?7 ?8 ?9 ?A ?B ?C ?D ?E ?F
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 0?
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 1?
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 2?
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 3?
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 4?
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 5?
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 6?
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 7?
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 8?
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 9?
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // A?
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // B?
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, // C?
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, // D?
    3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, // E?
    4, 4, 4, 4, 4, 4, 4, 4, 5, 5, 5, 5, 6, 6, 1, 1, // F?
];

/// Port of `utf_ptr2char()` from `Src/mbyte.c:668` — decode the UTF-8 character
/// at the start of `p`. Returns the first byte unchanged for an invalid or
/// truncated sequence.
pub fn utf_ptr2char(p: &[u8]) -> i32 {
    // c: uint8_t *p; read each byte, out-of-range reads yield the NUL terminator.
    let byte = |i: usize| -> u32 { p.get(i).copied().unwrap_or(0) as u32 };
    // c: #define S(s) ((uint32_t)0x80U << (s))
    let s = |sh: u32| -> u32 { 0x80u32.wrapping_shl(sh) };

    let v0 = byte(0);
    if v0 < 0x80 {
        // c: Be quick for ASCII.
        return v0 as i32;
    }

    let len = utf8len_tab[v0 as usize];
    if len < 2 {
        return v0 as i32;
    }

    // c: #define CHECK(v) if ((v & 0xC0) != 0x80) return v0;
    let v1 = byte(1);
    if (v1 & 0xC0) != 0x80 {
        return v0 as i32;
    }
    if len == 2 {
        return v0
            .wrapping_shl(6)
            .wrapping_add(v1)
            .wrapping_sub((0xC0u32 << 6).wrapping_add(s(0))) as i32;
    }

    let v2 = byte(2);
    if (v2 & 0xC0) != 0x80 {
        return v0 as i32;
    }
    if len == 3 {
        return v0
            .wrapping_shl(12)
            .wrapping_add(v1.wrapping_shl(6))
            .wrapping_add(v2)
            .wrapping_sub((0xE0u32 << 12).wrapping_add(s(6)).wrapping_add(s(0)))
            as i32;
    }

    let v3 = byte(3);
    if (v3 & 0xC0) != 0x80 {
        return v0 as i32;
    }
    if len == 4 {
        return v0
            .wrapping_shl(18)
            .wrapping_add(v1.wrapping_shl(12))
            .wrapping_add(v2.wrapping_shl(6))
            .wrapping_add(v3)
            .wrapping_sub(
                (0xF0u32 << 18)
                    .wrapping_add(s(12))
                    .wrapping_add(s(6))
                    .wrapping_add(s(0)),
            ) as i32;
    }

    let v4 = byte(4);
    if (v4 & 0xC0) != 0x80 {
        return v0 as i32;
    }
    if len == 5 {
        return v0
            .wrapping_shl(24)
            .wrapping_add(v1.wrapping_shl(18))
            .wrapping_add(v2.wrapping_shl(12))
            .wrapping_add(v3.wrapping_shl(6))
            .wrapping_add(v4)
            .wrapping_sub(
                (0xF8u32 << 24)
                    .wrapping_add(s(18))
                    .wrapping_add(s(12))
                    .wrapping_add(s(6))
                    .wrapping_add(s(0)),
            ) as i32;
    }

    let v5 = byte(5);
    if (v5 & 0xC0) != 0x80 {
        return v0 as i32;
    }
    // c: len == 6
    v0.wrapping_shl(30)
        .wrapping_add(v1.wrapping_shl(24))
        .wrapping_add(v2.wrapping_shl(18))
        .wrapping_add(v3.wrapping_shl(12))
        .wrapping_add(v4.wrapping_shl(6))
        .wrapping_add(v5)
        // c: - (0xFCU << 30) == - (S(24) + S(18) + S(12) + S(6) + S(0))
        .wrapping_sub(
            s(24)
                .wrapping_add(s(18))
                .wrapping_add(s(12))
                .wrapping_add(s(6))
                .wrapping_add(s(0)),
        ) as i32
}

/// Port of `utf_ptr2len()` from `Src/mbyte.c:916` — byte length of the UTF-8
/// character at `p`. Returns 0 for a leading NUL, 1 for an illegal/incomplete
/// sequence.
pub fn utf_ptr2len(p: &[u8]) -> i32 {
    let b0 = p.first().copied().unwrap_or(0);
    if b0 == 0 {
        // c: if (*p == NUL) return 0;
        return 0;
    }
    let len = utf8len_tab[b0 as usize] as i32;
    for i in 1..len {
        if (p.get(i as usize).copied().unwrap_or(0) & 0xc0) != 0x80 {
            return 1;
        }
    }
    len
}

/// Port of `utfc_ptr2len()` from `Src/mbyte.c:970` — byte length of the UTF-8
/// character at `p` *including* any following composing characters (the
/// character-plus-composing "cluster" that Vim treats as one unit in
/// `escape()`, `strcharpart()` skipcc, `slice()`, …).
///
/// RUST-PORT NOTE: the C's `utf_composinglike` uses the full grapheme tables;
/// consistent with the rest of the crate, the composing check is the
/// `utf_iscomposing` range approximation.
pub fn utfc_ptr2len(p: &[u8]) -> i32 {
    let b0 = p.first().copied().unwrap_or(0);
    // c: if (b0 == NUL) return 0;
    if b0 == 0 {
        return 0;
    }
    // c: if (b0 < 0x80 && p[1] < 0x80) return 1;  // be quick for ASCII
    if b0 < 0x80 && p.get(1).copied().unwrap_or(0) < 0x80 {
        return 1;
    }
    // c: len = utf_ptr2len(p); if (len == 1 && b0 >= 0x80) return 1;
    let mut len = utf_ptr2len(p);
    if len == 1 && b0 >= 0x80 {
        return 1;
    }
    // c: while (utf_composinglike(...)) len += utf_ptr2len(p + len);
    loop {
        let rest = &p[len as usize..];
        if rest.first().copied().unwrap_or(0) < 0x80 {
            return len;
        }
        let c = utf_ptr2char(rest);
        match char::from_u32(c as u32) {
            Some(ch) if crate::ported::strings::utf_iscomposing(ch) => {
                len += utf_ptr2len(rest);
            }
            _ => return len,
        }
    }
}

/// Port of `utf_head_off()` from `Src/mbyte.c:1763` — how many bytes the byte
/// at offset `i` sits PAST the start of its character cluster (base character
/// plus trailing composing characters). 0 when `i` is itself a cluster start.
///
/// RUST-PORT NOTE: the C consults utf8proc grapheme boundclasses; consistent
/// with the rest of the crate, the composing check is the `utf_iscomposing`
/// range approximation, and `p` is a byte offset into a valid UTF-8 buffer.
pub fn utf_head_off(base: &[u8], i: usize) -> usize {
    // c: if (*p < 0x80) return 0;  // be quick for ASCII
    if base.get(i).copied().unwrap_or(0) < 0x80 {
        return 0;
    }
    // c: move start to the first byte of this codepoint.
    let mut start = i;
    while start > 0 && (base[start] & 0xc0) == 0x80 && i - start < 6 {
        start -= 1;
    }
    // c: backtrack while the codepoint at `start` is a composing char, gluing
    // it to the preceding base character (the cluster the C's boundclass walk
    // finds).
    loop {
        let composing = utf_ptr2char(&base[start..])
            .try_into()
            .ok()
            .and_then(char::from_u32)
            .is_some_and(crate::ported::strings::utf_iscomposing);
        if !composing || start == 0 {
            break;
        }
        let mut prev = start - 1;
        while prev > 0 && (base[prev] & 0xc0) == 0x80 && start - prev < 6 {
            prev -= 1;
        }
        start = prev;
    }
    i - start
}

/// Port of `utf_char2len()` from `Src/mbyte.c:1053` — number of bytes needed to
/// encode Unicode character `c`.
pub fn utf_char2len(c: i32) -> i32 {
    if c < 0x80 {
        1
    } else if c < 0x800 {
        2
    } else if c < 0x10000 {
        3
    } else if c < 0x200000 {
        4
    } else if c < 0x4000000 {
        5
    } else {
        6
    }
}

/// Port of `utf_char2bytes()` from `Src/mbyte.c:1076` — encode Unicode character
/// `c` as UTF-8 into `buf` (which must have room for 6 bytes), returning the
/// number of bytes written (1-6). Does not append a NUL.
pub fn utf_char2bytes(c: i32, buf: &mut [u8]) -> i32 {
    let u = c as u32;
    if c < 0x80 {
        // 7 bits
        buf[0] = c as u8;
        1
    } else if c < 0x800 {
        // 11 bits
        buf[0] = (0xc0 + (u >> 6)) as u8;
        buf[1] = (0x80 + (u & 0x3f)) as u8;
        2
    } else if c < 0x10000 {
        // 16 bits
        buf[0] = (0xe0 + (u >> 12)) as u8;
        buf[1] = (0x80 + ((u >> 6) & 0x3f)) as u8;
        buf[2] = (0x80 + (u & 0x3f)) as u8;
        3
    } else if c < 0x200000 {
        // 21 bits
        buf[0] = (0xf0 + (u >> 18)) as u8;
        buf[1] = (0x80 + ((u >> 12) & 0x3f)) as u8;
        buf[2] = (0x80 + ((u >> 6) & 0x3f)) as u8;
        buf[3] = (0x80 + (u & 0x3f)) as u8;
        4
    } else if c < 0x4000000 {
        // 26 bits
        buf[0] = (0xf8 + (u >> 24)) as u8;
        buf[1] = (0x80 + ((u >> 18) & 0x3f)) as u8;
        buf[2] = (0x80 + ((u >> 12) & 0x3f)) as u8;
        buf[3] = (0x80 + ((u >> 6) & 0x3f)) as u8;
        buf[4] = (0x80 + (u & 0x3f)) as u8;
        5
    } else {
        // 31 bits
        buf[0] = (0xfc + (u >> 30)) as u8;
        buf[1] = (0x80 + ((u >> 24) & 0x3f)) as u8;
        buf[2] = (0x80 + ((u >> 18) & 0x3f)) as u8;
        buf[3] = (0x80 + ((u >> 12) & 0x3f)) as u8;
        buf[4] = (0x80 + ((u >> 6) & 0x3f)) as u8;
        buf[5] = (0x80 + (u & 0x3f)) as u8;
        6
    }
}

/// Port of `mb_tolower()` from `Src/mbyte.c` — single-codepoint lowercase
/// (`utf_tolower`).
///
/// RUST-PORT NOTE: the C consults Vim's own case-folding tables, which hold
/// only SIMPLE (1:1) mappings. Rust's `to_lowercase` is the FULL mapping and
/// can expand one char into several; the simple lowercase mapping is its first
/// codepoint (`U+0130` lowercases to `i` in Vim, while the full mapping is
/// `i` + `U+0307`).
pub fn mb_tolower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Port of `mb_toupper()` from `Src/mbyte.c` — single-codepoint uppercase
/// (`utf_toupper`). Same simple-vs-full note as [`mb_tolower`], with the
/// opposite resolution: where the FULL uppercase mapping expands to more than
/// one codepoint there is no simple mapping at all, and Vim leaves the
/// character alone. `toupper('ß')` is `ß` and `toupper('ﬁ')` is `ﬁ` in vim
/// 9.2.0900; taking the first codepoint of the full mapping answered `S` and
/// `F`, which is not a case conversion of anything.
pub fn mb_toupper(c: char) -> char {
    let mut up = c.to_uppercase();
    match (up.next(), up.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}

/// Port of `utf8len_tab_zero[]` from `vendor/mbyte.c:127` — "Like utf8len_tab
/// above, but using a zero for illegal lead bytes." The two differ only on
/// `0x80`-`0xBF` (continuation bytes) and `0xFE`/`0xFF`, which cannot begin a
/// sequence at all.
pub const utf8len_tab_zero: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        // The lead-byte lengths are identical to `utf8len_tab` everywhere it
        // does not say 1 for a byte that cannot lead; deriving it keeps the two
        // tables from drifting apart.
        t[i] = if i < 0x80 {
            1
        } else if i < 0xc0 || i >= 0xfe {
            0
        } else {
            utf8len_tab[i]
        };
        i += 1;
    }
    t
};

/// Port of `utf_safe_read_char_adv()` from `vendor/mbyte.c:741` — read the
/// character at the start of `s` without ever looking past `s`.
///
/// Returns `(codepoint, bytes_consumed)`:
///
/// * `(0, 0)` at end of buffer (the C's "returns 0");
/// * `(-1, 0)` when the sequence there is illegal or truncated, leaving the
///   position unmoved — the case `utf_strnicmp` reacts to by falling back to a
///   bytewise comparison;
/// * otherwise the decoded code point and its length.
///
/// The C's `c != (uint8_t)**s` test is how it tells a decoded character from
/// `utf_ptr2char`'s failure return (which is the first byte), with U+00C3 —
/// the one non-ASCII character that equals the first byte of its own encoding —
/// checked explicitly.
pub fn utf_safe_read_char_adv(s: &[u8]) -> (i32, usize) {
    let Some(&first) = s.first() else {
        return (0, 0); // c: `if (*n == 0) return 0;`
    };
    let k = utf8len_tab_zero[first as usize] as usize;
    if k == 1 {
        // c: ASCII character or NUL.
        return (first as i32, 1);
    }
    if k <= s.len() {
        let c = utf_ptr2char(s);
        if c != first as i32 || (c == 0xc3 && s.get(1) == Some(&0x83)) {
            return (c, k);
        }
    }
    // c: byte sequence is incomplete or illegal.
    (-1, 0)
}

/// Port of `mb_charlen()` from `Src/mbyte.c:2236` — the number of characters
/// in `str`, counting a base char plus its composing chars as one
/// (`utfc_ptr2len` steps).
pub fn mb_charlen(s: &str) -> i32 {
    let b = s.as_bytes();
    let mut count = 0i32;
    let mut i = 0usize;
    // c: for (count = 0; *p != NUL; count++) { p += utfc_ptr2len(p); }
    while i < b.len() {
        i += utfc_ptr2len(&b[i..]).max(1) as usize;
        count += 1;
    }
    count
}

/// Port of `utf_printable()` from `vendor/mbyte.c:1202` (the portable,
/// non-`__SSE2__` arm — the SSE2 arm above it computes the same predicate with
/// `_mm_cmpgt_epi16` over the same bounds).
///
/// True for characters that can be displayed in a normal way. Only meaningful
/// for characters of 0x100 and above; [`crate::ported::charset::vim_isprintc`]
/// consults `g_chartab[]` below that.
///
/// Everything outside these nine intervals is printable — including U+110000
/// and up, which is why `echo list2str([0x110000])` writes its four raw bytes
/// rather than `<110000>`.
pub fn utf_printable(c: i32) -> bool {
    // c: sorted list of non-overlapping intervals; 0xd800-0xdfff is reserved
    // for UTF-16, actually illegal.
    const NONPRINT: &[(i32, i32)] = &[
        (0x070f, 0x070f),
        (0x180b, 0x180e),
        (0x200b, 0x200f),
        (0x202a, 0x202e),
        (0x2060, 0x206f),
        (0xd800, 0xdfff),
        (0xfeff, 0xfeff),
        (0xfff9, 0xfffb),
        (0xfffe, 0xffff),
    ];
    // c: intable() — binary search; a linear scan over nine entries is the same
    // predicate and reads as the interval list it is.
    !NONPRINT
        .iter()
        .any(|&(first, last)| c >= first && c <= last)
}

// ── screen-cell width (c: vim `mbyte.c:1374` `utf_char2cells()`, the three
// interval tables it searches, and the `setcellwidths()` override in front of
// them) ──
//
// RUST-PORT NOTE: these four tables come from VIM rather than from
// `vendor/mbyte.c`, for the reason [`EMOJI_ALL`] gives: Neovim deleted the
// interval tables in favour of a utf8proc property query
// (`prop_is_emojilike()`, `vendor/mbyte.c:444`) and utf8proc is not a
// dependency of this crate. The parity oracle is vim, so vim's tables are the
// spec — and they are verified against it, not assumed: `strwidth(nr2char(c))`
// over every codepoint from 0x80 to 0x10FFFF agrees range for range.

/// Port of `doublewidth[]` from vim `mbyte.c:1379` — the sorted, non-overlapping
/// intervals of East Asian **Wide** and **Fullwidth** characters, generated
/// upstream by `runtime/tools/unicode.vim`.
const DOUBLEWIDTH: &[(u32, u32)] = &[
    (0x1100, 0x115f),
    (0x231a, 0x231b),
    (0x2329, 0x232a),
    (0x23e9, 0x23ec),
    (0x23f0, 0x23f0),
    (0x23f3, 0x23f3),
    (0x25fd, 0x25fe),
    (0x2614, 0x2615),
    (0x2630, 0x2637),
    (0x2648, 0x2653),
    (0x267f, 0x267f),
    (0x268a, 0x268f),
    (0x2693, 0x2693),
    (0x26a1, 0x26a1),
    (0x26aa, 0x26ab),
    (0x26bd, 0x26be),
    (0x26c4, 0x26c5),
    (0x26ce, 0x26ce),
    (0x26d4, 0x26d4),
    (0x26ea, 0x26ea),
    (0x26f2, 0x26f3),
    (0x26f5, 0x26f5),
    (0x26fa, 0x26fa),
    (0x26fd, 0x26fd),
    (0x2705, 0x2705),
    (0x270a, 0x270b),
    (0x2728, 0x2728),
    (0x274c, 0x274c),
    (0x274e, 0x274e),
    (0x2753, 0x2755),
    (0x2757, 0x2757),
    (0x2795, 0x2797),
    (0x27b0, 0x27b0),
    (0x27bf, 0x27bf),
    (0x2b1b, 0x2b1c),
    (0x2b50, 0x2b50),
    (0x2b55, 0x2b55),
    (0x2e80, 0x2e99),
    (0x2e9b, 0x2ef3),
    (0x2f00, 0x2fd5),
    (0x2ff0, 0x303e),
    (0x3041, 0x3096),
    (0x3099, 0x30ff),
    (0x3105, 0x312f),
    (0x3131, 0x318e),
    (0x3190, 0x31e5),
    (0x31ef, 0x321e),
    (0x3220, 0x3247),
    (0x3250, 0xa48c),
    (0xa490, 0xa4c6),
    (0xa960, 0xa97c),
    (0xac00, 0xd7a3),
    (0xf900, 0xfaff),
    (0xfe10, 0xfe19),
    (0xfe30, 0xfe52),
    (0xfe54, 0xfe66),
    (0xfe68, 0xfe6b),
    (0xff01, 0xff60),
    (0xffe0, 0xffe6),
    (0x16fe0, 0x16fe3),
    (0x16ff0, 0x16ff1),
    (0x17000, 0x187f7),
    (0x18800, 0x18cd5),
    (0x18cff, 0x18d08),
    (0x1aff0, 0x1aff3),
    (0x1aff5, 0x1affb),
    (0x1affd, 0x1affe),
    (0x1b000, 0x1b122),
    (0x1b132, 0x1b132),
    (0x1b150, 0x1b152),
    (0x1b155, 0x1b155),
    (0x1b164, 0x1b167),
    (0x1b170, 0x1b2fb),
    (0x1d300, 0x1d356),
    (0x1d360, 0x1d376),
    (0x1f004, 0x1f004),
    (0x1f0cf, 0x1f0cf),
    (0x1f18e, 0x1f18e),
    (0x1f191, 0x1f19a),
    (0x1f200, 0x1f202),
    (0x1f210, 0x1f23b),
    (0x1f240, 0x1f248),
    (0x1f250, 0x1f251),
    (0x1f260, 0x1f265),
    (0x1f300, 0x1f320),
    (0x1f32d, 0x1f335),
    (0x1f337, 0x1f37c),
    (0x1f37e, 0x1f393),
    (0x1f3a0, 0x1f3ca),
    (0x1f3cf, 0x1f3d3),
    (0x1f3e0, 0x1f3f0),
    (0x1f3f4, 0x1f3f4),
    (0x1f3f8, 0x1f43e),
    (0x1f440, 0x1f440),
    (0x1f442, 0x1f4fc),
    (0x1f4ff, 0x1f53d),
    (0x1f54b, 0x1f54e),
    (0x1f550, 0x1f567),
    (0x1f57a, 0x1f57a),
    (0x1f595, 0x1f596),
    (0x1f5a4, 0x1f5a4),
    (0x1f5fb, 0x1f64f),
    (0x1f680, 0x1f6c5),
    (0x1f6cc, 0x1f6cc),
    (0x1f6d0, 0x1f6d2),
    (0x1f6d5, 0x1f6d7),
    (0x1f6dc, 0x1f6df),
    (0x1f6eb, 0x1f6ec),
    (0x1f6f4, 0x1f6fc),
    (0x1f7e0, 0x1f7eb),
    (0x1f7f0, 0x1f7f0),
    (0x1f90c, 0x1f93a),
    (0x1f93c, 0x1f945),
    (0x1f947, 0x1f9ff),
    (0x1fa70, 0x1fa7c),
    (0x1fa80, 0x1fa89),
    (0x1fa8f, 0x1fac6),
    (0x1face, 0x1fadc),
    (0x1fadf, 0x1fae9),
    (0x1faf0, 0x1faf8),
    (0x20000, 0x2fffd),
    (0x30000, 0x3fffd),
];

/// Port of `emoji_wide[]` from vim `mbyte.c:1508` — Emoji that are not already
/// in [`DOUBLEWIDTH`] and are not East Asian Ambiguous, but which vim still
/// draws two cells wide when `'emoji'` is set.
const EMOJI_WIDE: &[(u32, u32)] = &[
    (0x23ed, 0x23ef),
    (0x23f1, 0x23f2),
    (0x23f8, 0x23fa),
    (0x24c2, 0x24c2),
    (0x261d, 0x261d),
    (0x26c8, 0x26c8),
    (0x26cf, 0x26cf),
    (0x26d1, 0x26d1),
    (0x26d3, 0x26d3),
    (0x26e9, 0x26e9),
    (0x26f0, 0x26f1),
    (0x26f7, 0x26f9),
    (0x270c, 0x270d),
    (0x2934, 0x2935),
    (0x1f170, 0x1f189),
    (0x1f1e6, 0x1f1ff),
    (0x1f321, 0x1f321),
    (0x1f324, 0x1f32c),
    (0x1f336, 0x1f336),
    (0x1f37d, 0x1f37d),
    (0x1f396, 0x1f397),
    (0x1f399, 0x1f39b),
    (0x1f39e, 0x1f39f),
    (0x1f3cb, 0x1f3ce),
    (0x1f3d4, 0x1f3df),
    (0x1f3f3, 0x1f3f5),
    (0x1f3f7, 0x1f3f7),
    (0x1f43f, 0x1f43f),
    (0x1f441, 0x1f441),
    (0x1f4fd, 0x1f4fd),
    (0x1f549, 0x1f54a),
    (0x1f56f, 0x1f570),
    (0x1f573, 0x1f579),
    (0x1f587, 0x1f587),
    (0x1f58a, 0x1f58d),
    (0x1f590, 0x1f590),
    (0x1f5a5, 0x1f5a5),
    (0x1f5a8, 0x1f5a8),
    (0x1f5b1, 0x1f5b2),
    (0x1f5bc, 0x1f5bc),
    (0x1f5c2, 0x1f5c4),
    (0x1f5d1, 0x1f5d3),
    (0x1f5dc, 0x1f5de),
    (0x1f5e1, 0x1f5e1),
    (0x1f5e3, 0x1f5e3),
    (0x1f5e8, 0x1f5e8),
    (0x1f5ef, 0x1f5ef),
    (0x1f5f3, 0x1f5f3),
    (0x1f5fa, 0x1f5fa),
    (0x1f6cb, 0x1f6cf),
    (0x1f6e0, 0x1f6e5),
    (0x1f6e9, 0x1f6e9),
    (0x1f6f0, 0x1f6f0),
    (0x1f6f3, 0x1f6f3),
];

/// c: the `#ifdef MACOS_X` tail of `emoji_wide[]` (vim `mbyte.c:1586`) — Apple's
/// SF Symbols 4, which live in Supplementary Private Use Area-B and are shipped
/// as glyphs in the default San Francisco fonts. The `#ifdef` is a `cfg` here
/// for the same reason it is an `#ifdef` there: on any other platform those
/// codepoints are ordinary private-use characters, one cell wide.
#[cfg(target_os = "macos")]
const EMOJI_WIDE_SF_SYMBOLS: &[(u32, u32)] = &[(0x100000, 0x1018c7)];
/// c: the `#ifdef MACOS_X` tail of `emoji_wide[]`, absent off macOS.
#[cfg(not(target_os = "macos"))]
const EMOJI_WIDE_SF_SYMBOLS: &[(u32, u32)] = &[];

/// Port of `ambiguous[]` from vim `mbyte.c:1171` — East Asian **Ambiguous**
/// characters, which are two cells wide only when `'ambiwidth'` is `"double"`.
const AMBIGUOUS: &[(u32, u32)] = &[
    (0x00a1, 0x00a1),
    (0x00a4, 0x00a4),
    (0x00a7, 0x00a8),
    (0x00aa, 0x00aa),
    (0x00ad, 0x00ae),
    (0x00b0, 0x00b4),
    (0x00b6, 0x00ba),
    (0x00bc, 0x00bf),
    (0x00c6, 0x00c6),
    (0x00d0, 0x00d0),
    (0x00d7, 0x00d8),
    (0x00de, 0x00e1),
    (0x00e6, 0x00e6),
    (0x00e8, 0x00ea),
    (0x00ec, 0x00ed),
    (0x00f0, 0x00f0),
    (0x00f2, 0x00f3),
    (0x00f7, 0x00fa),
    (0x00fc, 0x00fc),
    (0x00fe, 0x00fe),
    (0x0101, 0x0101),
    (0x0111, 0x0111),
    (0x0113, 0x0113),
    (0x011b, 0x011b),
    (0x0126, 0x0127),
    (0x012b, 0x012b),
    (0x0131, 0x0133),
    (0x0138, 0x0138),
    (0x013f, 0x0142),
    (0x0144, 0x0144),
    (0x0148, 0x014b),
    (0x014d, 0x014d),
    (0x0152, 0x0153),
    (0x0166, 0x0167),
    (0x016b, 0x016b),
    (0x01ce, 0x01ce),
    (0x01d0, 0x01d0),
    (0x01d2, 0x01d2),
    (0x01d4, 0x01d4),
    (0x01d6, 0x01d6),
    (0x01d8, 0x01d8),
    (0x01da, 0x01da),
    (0x01dc, 0x01dc),
    (0x0251, 0x0251),
    (0x0261, 0x0261),
    (0x02c4, 0x02c4),
    (0x02c7, 0x02c7),
    (0x02c9, 0x02cb),
    (0x02cd, 0x02cd),
    (0x02d0, 0x02d0),
    (0x02d8, 0x02db),
    (0x02dd, 0x02dd),
    (0x02df, 0x02df),
    (0x0300, 0x036f),
    (0x0391, 0x03a1),
    (0x03a3, 0x03a9),
    (0x03b1, 0x03c1),
    (0x03c3, 0x03c9),
    (0x0401, 0x0401),
    (0x0410, 0x044f),
    (0x0451, 0x0451),
    (0x2010, 0x2010),
    (0x2013, 0x2016),
    (0x2018, 0x2019),
    (0x201c, 0x201d),
    (0x2020, 0x2022),
    (0x2024, 0x2027),
    (0x2030, 0x2030),
    (0x2032, 0x2033),
    (0x2035, 0x2035),
    (0x203b, 0x203b),
    (0x203e, 0x203e),
    (0x2074, 0x2074),
    (0x207f, 0x207f),
    (0x2081, 0x2084),
    (0x20ac, 0x20ac),
    (0x2103, 0x2103),
    (0x2105, 0x2105),
    (0x2109, 0x2109),
    (0x2113, 0x2113),
    (0x2116, 0x2116),
    (0x2121, 0x2122),
    (0x2126, 0x2126),
    (0x212b, 0x212b),
    (0x2153, 0x2154),
    (0x215b, 0x215e),
    (0x2160, 0x216b),
    (0x2170, 0x2179),
    (0x2189, 0x2189),
    (0x2190, 0x2199),
    (0x21b8, 0x21b9),
    (0x21d2, 0x21d2),
    (0x21d4, 0x21d4),
    (0x21e7, 0x21e7),
    (0x2200, 0x2200),
    (0x2202, 0x2203),
    (0x2207, 0x2208),
    (0x220b, 0x220b),
    (0x220f, 0x220f),
    (0x2211, 0x2211),
    (0x2215, 0x2215),
    (0x221a, 0x221a),
    (0x221d, 0x2220),
    (0x2223, 0x2223),
    (0x2225, 0x2225),
    (0x2227, 0x222c),
    (0x222e, 0x222e),
    (0x2234, 0x2237),
    (0x223c, 0x223d),
    (0x2248, 0x2248),
    (0x224c, 0x224c),
    (0x2252, 0x2252),
    (0x2260, 0x2261),
    (0x2264, 0x2267),
    (0x226a, 0x226b),
    (0x226e, 0x226f),
    (0x2282, 0x2283),
    (0x2286, 0x2287),
    (0x2295, 0x2295),
    (0x2299, 0x2299),
    (0x22a5, 0x22a5),
    (0x22bf, 0x22bf),
    (0x2312, 0x2312),
    (0x2460, 0x24e9),
    (0x24eb, 0x254b),
    (0x2550, 0x2573),
    (0x2580, 0x258f),
    (0x2592, 0x2595),
    (0x25a0, 0x25a1),
    (0x25a3, 0x25a9),
    (0x25b2, 0x25b3),
    (0x25b6, 0x25b7),
    (0x25bc, 0x25bd),
    (0x25c0, 0x25c1),
    (0x25c6, 0x25c8),
    (0x25cb, 0x25cb),
    (0x25ce, 0x25d1),
    (0x25e2, 0x25e5),
    (0x25ef, 0x25ef),
    (0x2605, 0x2606),
    (0x2609, 0x2609),
    (0x260e, 0x260f),
    (0x261c, 0x261c),
    (0x261e, 0x261e),
    (0x2640, 0x2640),
    (0x2642, 0x2642),
    (0x2660, 0x2661),
    (0x2663, 0x2665),
    (0x2667, 0x266a),
    (0x266c, 0x266d),
    (0x266f, 0x266f),
    (0x269e, 0x269f),
    (0x26bf, 0x26bf),
    (0x26c6, 0x26cd),
    (0x26cf, 0x26d3),
    (0x26d5, 0x26e1),
    (0x26e3, 0x26e3),
    (0x26e8, 0x26e9),
    (0x26eb, 0x26f1),
    (0x26f4, 0x26f4),
    (0x26f6, 0x26f9),
    (0x26fb, 0x26fc),
    (0x26fe, 0x26ff),
    (0x273d, 0x273d),
    (0x2776, 0x277f),
    (0x2b56, 0x2b59),
    (0x3248, 0x324f),
    (0xe000, 0xf8ff),
    (0xfe00, 0xfe0f),
    (0xfffd, 0xfffd),
    (0x1f100, 0x1f10a),
    (0x1f110, 0x1f12d),
    (0x1f130, 0x1f169),
    (0x1f170, 0x1f18d),
    (0x1f18f, 0x1f190),
    (0x1f19b, 0x1f1ac),
    (0xe0100, 0xe01ef),
    (0xf0000, 0xffffd),
    (0x100000, 0x10fffd),
];

/// Port of `utf_char2cells()` from vim `mbyte.c:1374` — the number of screen
/// cells character `c` occupies: 2 for a double-width character, 4 or 6 for an
/// unprintable one (shown as `<xx>` / `<xxxx>`), otherwise 1.
///
/// Only correct for characters at 0x80 and above; below that the answer is
/// `g_chartab[]`'s ([`crate::ported::charset::char2cells`]).
///
/// RUST-PORT NOTE: the C's `intable()` helper is vim-only — a `fn` by that name
/// has no counterpart in the vendored Neovim C — so its body, the binary search
/// over a sorted non-overlapping interval list, is bound here as a closure and
/// called four times exactly as the C calls `intable()` four times.
pub fn utf_char2cells(c: i32) -> i32 {
    // c: `intable(table, sizeof(table), c)`, vim `mbyte.c:1141`.
    let intable = |table: &[(u32, u32)], c: i32| -> bool {
        // c: first quick check for Latin1 etc. characters
        if table.is_empty() || c < table[0].0 as i32 {
            return false;
        }
        // c: binary search in table
        let (mut bot, mut top) = (0i32, table.len() as i32 - 1);
        while top >= bot {
            let mid = (bot + top) / 2;
            if table[mid as usize].1 < c as u32 {
                bot = mid + 1;
            } else if table[mid as usize].0 > c as u32 {
                top = mid - 1;
            } else {
                return true;
            }
        }
        false
    };

    // c: FEAT_EVAL — "Use the value from setcellwidths() at 0x80 and higher,
    // unless the character is not printable."
    if c >= 0x80 && crate::ported::charset::vim_isprintc(c) {
        if let Some(n) = crate::ported::strings::cw_value(c as u32) {
            return n as i32;
        }
    }

    if c >= 0x100 {
        if !utf_printable(c) {
            return 6; // c: unprintable, displays <xxxx>
        }
        if intable(DOUBLEWIDTH, c) {
            return 2;
        }
        // c: `p_emoji` — the `'emoji'` option, on by default. Read through the
        // option table on every call so `:set noemoji` takes effect at once.
        let p_emoji = crate::ported::eval::typval::tv_get_number_chk(
            &crate::ported::option::get_option_value("emoji"),
            None,
        ) != 0;
        if p_emoji && (intable(EMOJI_WIDE, c) || intable(EMOJI_WIDE_SF_SYMBOLS, c)) {
            return 2;
        }
    }
    // c: Characters below 0x100 are influenced by 'isprint' option
    else if c >= 0x80 && !crate::ported::charset::vim_isprintc(c) {
        return 4; // c: unprintable, displays <xx>
    }

    // c: `*p_ambw == 'd'` — `'ambiwidth'` is `"single"` or `"double"`, and only
    // its first byte is tested, exactly as the C tests it.
    let p_ambw_double = crate::ported::eval::typval::tv_get_string(
        &crate::ported::option::get_option_value("ambiwidth"),
    )
    .starts_with('d');
    if c >= 0x80 && p_ambw_double && intable(AMBIGUOUS, c) {
        return 2;
    }

    1
}

/// Port of `utf_ptr2cells()` from vim `mbyte.c:1633` — the cells the character
/// at the start of `p` occupies. An illegal byte displays as `<xx>`, four cells.
pub fn utf_ptr2cells(p: &[u8]) -> i32 {
    // c: Need to convert to a character number.
    if p.first().copied().unwrap_or(0) >= 0x80 {
        let c = utf_ptr2char(p);
        let len = utf_ptr2len(p);
        // c: An illegal byte is displayed as <xx>.
        if len == 1 || c == 0 {
            return 4;
        }
        // c: If the char is ASCII it must be an overlong sequence.
        if c < 0x80 {
            return if crate::ported::charset::vim_isprintc(c) {
                crate::ported::charset::char2cells(c)
            } else {
                4
            };
        }
        return utf_char2cells(c);
    }
    1
}

/// Port of `mb_string2cells()` from `vendor/mbyte.c:628` (vim's `vim_strsize()`,
/// `charset.c:731`) — the cells a whole NUL-terminated string occupies.
///
/// The walk advances by `utfc_ptr2len`, so a composing mark is stepped over with
/// the base character it belongs to and contributes nothing of its own — which
/// is why `strwidth("é")` is 1.
pub fn mb_string2cells(s: &[u8]) -> usize {
    let mut clen = 0usize;
    let mut p = 0usize;
    while p < s.len() {
        clen += utf_ptr2cells(&s[p..]) as usize;
        p += utfc_ptr2len(&s[p..]).max(1) as usize;
    }
    clen
}

/// Port of `emoji_all[]` from vim `mbyte.c:2863` (v9.2.1000) — the sorted,
/// non-overlapping intervals of every Emoji codepoint, generated upstream by
/// `runtime/tools/unicode.vim` from unicode.org's emoji-list. `0x00a9` and
/// `0x00ae` are excluded there because they are latin1.
///
/// RUST-PORT NOTE: this one table comes from VIM rather than from
/// `vendor/mbyte.c`, and deliberately. Neovim replaced the interval table with
/// a utf8proc property query (`prop_is_emojilike()`, `vendor/mbyte.c:444`),
/// and utf8proc is not a dependency of this crate. The parity oracle is vim
/// 9.2.1000, so vim's table is the spec that makes `charclass()` and every
/// `\<`/`\>` answer agree with it: verified codepoint by codepoint over
/// `1..=0x2fb00` against the oracle's own `charclass()`.
const EMOJI_ALL: &[(u32, u32)] = &[
    (0x203c, 0x203c),
    (0x2049, 0x2049),
    (0x2122, 0x2122),
    (0x2139, 0x2139),
    (0x2194, 0x2199),
    (0x21a9, 0x21aa),
    (0x231a, 0x231b),
    (0x2328, 0x2328),
    (0x23cf, 0x23cf),
    (0x23e9, 0x23f3),
    (0x23f8, 0x23fa),
    (0x24c2, 0x24c2),
    (0x25aa, 0x25ab),
    (0x25b6, 0x25b6),
    (0x25c0, 0x25c0),
    (0x25fb, 0x25fe),
    (0x2600, 0x2604),
    (0x260e, 0x260e),
    (0x2611, 0x2611),
    (0x2614, 0x2615),
    (0x2618, 0x2618),
    (0x261d, 0x261d),
    (0x2620, 0x2620),
    (0x2622, 0x2623),
    (0x2626, 0x2626),
    (0x262a, 0x262a),
    (0x262e, 0x262f),
    (0x2638, 0x263a),
    (0x2640, 0x2640),
    (0x2642, 0x2642),
    (0x2648, 0x2653),
    (0x265f, 0x2660),
    (0x2663, 0x2663),
    (0x2665, 0x2666),
    (0x2668, 0x2668),
    (0x267b, 0x267b),
    (0x267e, 0x267f),
    (0x2692, 0x2697),
    (0x2699, 0x2699),
    (0x269b, 0x269c),
    (0x26a0, 0x26a1),
    (0x26a7, 0x26a7),
    (0x26aa, 0x26ab),
    (0x26b0, 0x26b1),
    (0x26bd, 0x26be),
    (0x26c4, 0x26c5),
    (0x26c8, 0x26c8),
    (0x26ce, 0x26cf),
    (0x26d1, 0x26d1),
    (0x26d3, 0x26d4),
    (0x26e9, 0x26ea),
    (0x26f0, 0x26f5),
    (0x26f7, 0x26fa),
    (0x26fd, 0x26fd),
    (0x2702, 0x2702),
    (0x2705, 0x2705),
    (0x2708, 0x270d),
    (0x270f, 0x270f),
    (0x2712, 0x2712),
    (0x2714, 0x2714),
    (0x2716, 0x2716),
    (0x271d, 0x271d),
    (0x2721, 0x2721),
    (0x2728, 0x2728),
    (0x2733, 0x2734),
    (0x2744, 0x2744),
    (0x2747, 0x2747),
    (0x274c, 0x274c),
    (0x274e, 0x274e),
    (0x2753, 0x2755),
    (0x2757, 0x2757),
    (0x2763, 0x2764),
    (0x2795, 0x2797),
    (0x27a1, 0x27a1),
    (0x27b0, 0x27b0),
    (0x27bf, 0x27bf),
    (0x2934, 0x2935),
    (0x2b05, 0x2b07),
    (0x2b1b, 0x2b1c),
    (0x2b50, 0x2b50),
    (0x2b55, 0x2b55),
    (0x3030, 0x3030),
    (0x303d, 0x303d),
    (0x3297, 0x3297),
    (0x3299, 0x3299),
    (0x1f004, 0x1f004),
    (0x1f0cf, 0x1f0cf),
    (0x1f170, 0x1f171),
    (0x1f17e, 0x1f17f),
    (0x1f18e, 0x1f18e),
    (0x1f191, 0x1f19a),
    (0x1f1e6, 0x1f1ff),
    (0x1f201, 0x1f202),
    (0x1f21a, 0x1f21a),
    (0x1f22f, 0x1f22f),
    (0x1f232, 0x1f23a),
    (0x1f250, 0x1f251),
    (0x1f300, 0x1f321),
    (0x1f324, 0x1f393),
    (0x1f396, 0x1f397),
    (0x1f399, 0x1f39b),
    (0x1f39e, 0x1f3f0),
    (0x1f3f3, 0x1f3f5),
    (0x1f3f7, 0x1f4fd),
    (0x1f4ff, 0x1f53d),
    (0x1f549, 0x1f54e),
    (0x1f550, 0x1f567),
    (0x1f56f, 0x1f570),
    (0x1f573, 0x1f57a),
    (0x1f587, 0x1f587),
    (0x1f58a, 0x1f58d),
    (0x1f590, 0x1f590),
    (0x1f595, 0x1f596),
    (0x1f5a4, 0x1f5a5),
    (0x1f5a8, 0x1f5a8),
    (0x1f5b1, 0x1f5b2),
    (0x1f5bc, 0x1f5bc),
    (0x1f5c2, 0x1f5c4),
    (0x1f5d1, 0x1f5d3),
    (0x1f5dc, 0x1f5de),
    (0x1f5e1, 0x1f5e1),
    (0x1f5e3, 0x1f5e3),
    (0x1f5e8, 0x1f5e8),
    (0x1f5ef, 0x1f5ef),
    (0x1f5f3, 0x1f5f3),
    (0x1f5fa, 0x1f64f),
    (0x1f680, 0x1f6c5),
    (0x1f6cb, 0x1f6d2),
    (0x1f6d5, 0x1f6d7),
    (0x1f6dc, 0x1f6e5),
    (0x1f6e9, 0x1f6e9),
    (0x1f6eb, 0x1f6ec),
    (0x1f6f0, 0x1f6f0),
    (0x1f6f3, 0x1f6fc),
    (0x1f7e0, 0x1f7eb),
    (0x1f7f0, 0x1f7f0),
    (0x1f90c, 0x1f93a),
    (0x1f93c, 0x1f945),
    (0x1f947, 0x1f9ff),
    (0x1fa70, 0x1fa7c),
    (0x1fa80, 0x1fa88),
    (0x1fa90, 0x1fabd),
    (0x1fabf, 0x1fac5),
    (0x1face, 0x1fadb),
    (0x1fae0, 0x1fae8),
    (0x1faf0, 0x1faf8),
];

/// Port of the `classes[]` interval table inside `utf_class_buf()`
/// (vim `mbyte.c:3034`, v9.2.1000; identical to `utf_class_tab()`'s table at
/// `vendor/mbyte.c:1235`) — a sorted list of non-overlapping intervals mapping
/// a codepoint to its character class.
///
/// A class of `0` is whitespace and `1` is punctuation; anything else is a word
/// character, and the VALUE distinguishes *which kind* of word character. That
/// is the whole point of the table: Hiragana is `0x3040`, Katakana `0x30a0`,
/// CJK Ideographs `0x4e00`, Hangul `0xac00`, braille `0x2800`, superscripts
/// `0x2070`, subscripts `0x2080`, and everything else that is a word character
/// is `2`. `\<` fires wherever two adjacent characters have *different*
/// non-punctuation classes, which is why vim finds a word boundary between
/// `語` and `a` in `日本語abc`.
const CLASSES: &[(u32, u32, u32)] = &[
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

/// Port of `utf_class_tab()` from `vendor/mbyte.c:1227` (vim's
/// `utf_class_buf()`, `mbyte.c:3026`) — the character class of a Unicode
/// codepoint.
///
/// * `0` — white space
/// * `1` — punctuation
/// * `2` or bigger — some class of word character
///
/// Below `0x100` the answer comes from `'iskeyword'`
/// ([`crate::ported::charset::vim_iswordc_tab`]); above it, emoji answer `3`
/// and the rest is a binary search of [`CLASSES`], defaulting to `2` — "most
/// other characters are word characters".
pub fn utf_class_tab(c: i32) -> i32 {
    // c: First quick check for Latin1 characters, use 'iskeyword'.
    if c < 0x100 {
        if c == b' ' as i32 || c == b'\t' as i32 || c == 0 || c == 0xa0 {
            return 0; // c: blank
        }
        if crate::ported::charset::vim_iswordc_tab(c) {
            return 2; // c: word character
        }
        return 1; // c: punctuation
    }

    // c: emoji — `intable(emoji_all, sizeof(emoji_all), c)`, vim `mbyte.c:3124`.
    // Its body is the same binary search over a sorted, non-overlapping
    // interval list that the `classes[]` search below is, written out here
    // rather than factored into a helper: `intable()` is vim-only, so a `fn`
    // by that name has no counterpart in the vendored Neovim C.
    {
        let mut bot = 0i32;
        let mut top = EMOJI_ALL.len() as i32 - 1;
        // c: first quick check for Latin1 etc. characters
        if c >= EMOJI_ALL[0].0 as i32 {
            while top >= bot {
                let mid = (bot + top) / 2;
                if EMOJI_ALL[mid as usize].1 < c as u32 {
                    bot = mid + 1;
                } else if EMOJI_ALL[mid as usize].0 > c as u32 {
                    top = mid - 1;
                } else {
                    return 3;
                }
            }
        }
    }

    // c: binary search in table
    let mut bot = 0i32;
    let mut top = CLASSES.len() as i32 - 1;
    while top >= bot {
        let mid = (bot + top) / 2;
        let (first, last, cls) = CLASSES[mid as usize];
        if last < c as u32 {
            bot = mid + 1;
        } else if first > c as u32 {
            top = mid - 1;
        } else {
            return cls as i32;
        }
    }

    // c: most other characters are "word" characters
    2
}

/// Port of `utf_class()` from `vendor/mbyte.c:1222` — [`utf_class_tab`] for the
/// current buffer, which here is the only buffer.
pub fn utf_class(c: i32) -> i32 {
    utf_class_tab(c)
}

/// Port of `mb_get_class_tab()` from `vendor/mbyte.c:429` (vim's
/// `mb_get_class_buf()`, `mbyte.c:838`) — the class of the character a pointer
/// points at.
///
/// RUST-PORT NOTE: the C takes a `char_u *` into the line and branches on
/// `MB_BYTE2LEN(p[0]) == 1`, i.e. on whether the byte is ASCII. This crate's
/// regex engines walk a decoded `&[char]`, so the argument is the character
/// itself. The two branches agree: the single-byte arm is reachable only for
/// `c < 0x80`, where its three tests (NUL/white → 0, `vim_iswordc` → 2,
/// else 1) are exactly [`utf_class_tab`]'s own `c < 0x100` arm.
pub fn mb_get_class_tab(c: char) -> i32 {
    utf_class_tab(c as i32)
}

/// Port of `mb_get_class()` from `vendor/mbyte.c:423` — [`mb_get_class_tab`]
/// for the current buffer.
pub fn mb_get_class(c: char) -> i32 {
    mb_get_class_tab(c)
}

/// Port of `mb_islower()` from `vendor/mbyte.c:1417` — "a" has an uppercase
/// equivalent, so it is a lowercase letter.
pub fn mb_islower(c: char) -> bool {
    mb_toupper(c) != c
}

/// Port of `mb_isupper()` from `vendor/mbyte.c:1443` — the mirror of
/// [`mb_islower`].
pub fn mb_isupper(c: char) -> bool {
    mb_tolower(c) != c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_roundtrip() {
        assert_eq!(utf_ptr2char(b"A"), 0x41);
        assert_eq!(utf_char2len(0x41), 1);
        let mut b = [0u8; 6];
        assert_eq!(utf_char2bytes(0x41, &mut b), 1);
        assert_eq!(b[0], b'A');
    }

    #[test]
    fn multibyte_roundtrip() {
        // U+00E9 é (2 bytes), U+20AC € (3 bytes), U+1F600 😀 (4 bytes).
        for &ch in &[0xE9, 0x20AC, 0x1F600] {
            let mut b = [0u8; 6];
            let n = utf_char2bytes(ch, &mut b);
            assert_eq!(n, utf_char2len(ch));
            assert_eq!(utf_ptr2char(&b[..n as usize]), ch);
            assert_eq!(utf_ptr2len(&b[..n as usize]), n);
        }
    }

    #[test]
    fn truncated_returns_lead_byte() {
        // Lead byte of a 3-byte sequence with no continuation → returns lead byte.
        assert_eq!(utf_ptr2char(&[0xE2]), 0xE2);
    }

    /// `utf_class_tab()` at the interval EDGES — the places a fencepost in the
    /// binary search or a mistyped bound would show. Every expectation is vim
    /// 9.2.1000's own `charclass()`.
    #[test]
    fn utf_class_tab_interval_edges() {
        // Below 0x100 the answer is 'iskeyword', not the table.
        assert_eq!(utf_class_tab(0x20), 0); // space
        assert_eq!(utf_class_tab(0x09), 0); // tab
        assert_eq!(utf_class_tab(0xa0), 0); // NBSP
        assert_eq!(utf_class_tab('a' as i32), 2);
        assert_eq!(utf_class_tab('_' as i32), 2);
        assert_eq!(utf_class_tab('!' as i32), 1);
        // The `@` alpha class: µ has an uppercase mapping, ª does not.
        assert_eq!(utf_class_tab(0xb5), 2);
        assert_eq!(utf_class_tab(0xaa), 1);
        assert_eq!(utf_class_tab(0xc0), 2); // start of the 192-255 range
        assert_eq!(utf_class_tab(0xff), 2); // end of it
                                            // Script blocks carry their own class number, which is what makes `\<`
                                            // fire between two scripts.
        assert_eq!(utf_class_tab(0x3040), 0x3040); // hiragana, first
        assert_eq!(utf_class_tab(0x309f), 0x3040); // hiragana, last
        assert_eq!(utf_class_tab(0x30a0), 0x30a0); // katakana, first
        assert_eq!(utf_class_tab(0x30ff), 0x30a0); // katakana, last
        assert_eq!(utf_class_tab(0x4e00), 0x4e00); // CJK (inside 0x3300-0x9fff)
        assert_eq!(utf_class_tab(0xac00), 0xac00); // Hangul, first
        assert_eq!(utf_class_tab(0xd7a3), 0xac00); // Hangul, last
        assert_eq!(utf_class_tab(0x2800), 0x2800); // braille, first
        assert_eq!(utf_class_tab(0x28ff), 0x2800); // braille, last
        assert_eq!(utf_class_tab(0x2070), 0x2070); // superscript
        assert_eq!(utf_class_tab(0x2080), 0x2080); // subscript
                                                   // Punctuation and whitespace intervals.
        assert_eq!(utf_class_tab(0x3000), 0); // ideographic space
        assert_eq!(utf_class_tab(0x3001), 1); // ideographic punctuation
        assert_eq!(utf_class_tab(0x2028), 0); // line separator
                                              // Emoji answer 3, and the check runs BEFORE the interval table — U+1F600
                                              // is inside `{0x1f300, 0x1f9ff, 1}`, which would call it punctuation.
        assert_eq!(utf_class_tab(0x1f600), 3);
        assert_eq!(utf_class_tab(0x203c), 3); // first `emoji_all` interval
        assert_eq!(utf_class_tab(0x1faf8), 3); // last one
                                               // 0x00a9 and 0x00ae are excluded from `emoji_all` as latin1.
        assert_eq!(utf_class_tab(0xa9), 1);
        // Everything not in either table is a word character.
        assert_eq!(utf_class_tab(0x0301), 2); // COMBINING ACUTE ACCENT
        assert_eq!(utf_class_tab(0x0391), 2); // GREEK CAPITAL ALPHA
        assert_eq!(utf_class_tab(0x2fa1f), 0x4e00); // last CJK interval, last cp
        assert_eq!(utf_class_tab(0x2fa20), 2); // one past it
    }
}

// ── encoding names and conversion (`vendor/mbyte.c:146-2850`) ──────────────
//
// The canonical-name table, the alias table, `enc_canonize()`, and the
// converter `iconv()` (the Vimscript function) is built on: an internal
// latin1/latin9 ↔ UTF-8 conversion, everything else through the C library's
// iconv(3) exactly as the C calls it.

/// `ENC_*` property bits of an encoding (`mbyte_defs.h`).
pub const ENC_8BIT: i32 = 0x01;
pub const ENC_DBCS: i32 = 0x02;
pub const ENC_UNICODE: i32 = 0x04;
pub const ENC_ENDIAN_B: i32 = 0x10;
pub const ENC_ENDIAN_L: i32 = 0x20;
pub const ENC_2BYTE: i32 = 0x40;
pub const ENC_4BYTE: i32 = 0x80;
pub const ENC_2WORD: i32 = 0x100;
pub const ENC_LATIN1: i32 = 0x200;
pub const ENC_LATIN9: i32 = 0x400;
pub const ENC_MACROMAN: i32 = 0x800;

/// Port of `enc_canon_table[]` (`vendor/mbyte.c:151`): canonical name and
/// properties. The codepage column is Windows-only and not carried.
const enc_canon_table: &[(&str, i32)] = &[
    ("latin1", ENC_8BIT + ENC_LATIN1),
    ("iso-8859-2", ENC_8BIT),
    ("iso-8859-3", ENC_8BIT),
    ("iso-8859-4", ENC_8BIT),
    ("iso-8859-5", ENC_8BIT),
    ("iso-8859-6", ENC_8BIT),
    ("iso-8859-7", ENC_8BIT),
    ("iso-8859-8", ENC_8BIT),
    ("iso-8859-9", ENC_8BIT),
    ("iso-8859-10", ENC_8BIT),
    ("iso-8859-11", ENC_8BIT),
    ("iso-8859-13", ENC_8BIT),
    ("iso-8859-14", ENC_8BIT),
    ("iso-8859-15", ENC_8BIT + ENC_LATIN9),
    ("koi8-r", ENC_8BIT),
    ("koi8-u", ENC_8BIT),
    ("utf-8", ENC_UNICODE),
    ("ucs-2", ENC_UNICODE + ENC_ENDIAN_B + ENC_2BYTE),
    ("ucs-2le", ENC_UNICODE + ENC_ENDIAN_L + ENC_2BYTE),
    ("utf-16", ENC_UNICODE + ENC_ENDIAN_B + ENC_2WORD),
    ("utf-16le", ENC_UNICODE + ENC_ENDIAN_L + ENC_2WORD),
    ("ucs-4", ENC_UNICODE + ENC_ENDIAN_B + ENC_4BYTE),
    ("ucs-4le", ENC_UNICODE + ENC_ENDIAN_L + ENC_4BYTE),
    ("debug", ENC_DBCS),
    ("euc-jp", ENC_DBCS),
    ("sjis", ENC_DBCS),
    ("euc-kr", ENC_DBCS),
    ("euc-cn", ENC_DBCS),
    ("euc-tw", ENC_DBCS),
    ("big5", ENC_DBCS),
    ("cp437", ENC_8BIT),
    ("cp737", ENC_8BIT),
    ("cp775", ENC_8BIT),
    ("cp850", ENC_8BIT),
    ("cp852", ENC_8BIT),
    ("cp855", ENC_8BIT),
    ("cp857", ENC_8BIT),
    ("cp860", ENC_8BIT),
    ("cp861", ENC_8BIT),
    ("cp862", ENC_8BIT),
    ("cp863", ENC_8BIT),
    ("cp865", ENC_8BIT),
    ("cp866", ENC_8BIT),
    ("cp869", ENC_8BIT),
    ("cp874", ENC_8BIT),
    ("cp932", ENC_DBCS),
    ("cp936", ENC_DBCS),
    ("cp949", ENC_DBCS),
    ("cp950", ENC_DBCS),
    ("cp1250", ENC_8BIT),
    ("cp1251", ENC_8BIT),
    ("cp1253", ENC_8BIT),
    ("cp1254", ENC_8BIT),
    ("cp1255", ENC_8BIT),
    ("cp1256", ENC_8BIT),
    ("cp1257", ENC_8BIT),
    ("cp1258", ENC_8BIT),
    ("macroman", ENC_8BIT + ENC_MACROMAN),
    ("hp-roman8", ENC_8BIT),
];

/// Port of `enc_alias_table[]` (`vendor/mbyte.c:274`): alias → index into
/// [`enc_canon_table`].
const enc_alias_table: &[(&str, usize)] = &[
    ("ansi", 0),
    ("iso-8859-1", 0),
    ("latin2", 1),
    ("latin3", 2),
    ("latin4", 3),
    ("cyrillic", 4),
    ("arabic", 5),
    ("greek", 6),
    ("hebrew", 7),
    ("latin5", 8),
    ("turkish", 8),
    ("latin6", 9),
    ("nordic", 9),
    ("thai", 10),
    ("latin7", 11),
    ("latin8", 12),
    ("latin9", 13),
    ("utf8", 16),
    ("unicode", 17),
    ("ucs2", 17),
    ("ucs2be", 17),
    ("ucs-2be", 17),
    ("ucs2le", 18),
    ("utf16", 19),
    ("utf16be", 19),
    ("utf-16be", 19),
    ("utf16le", 20),
    ("ucs4", 21),
    ("ucs4be", 21),
    ("ucs-4be", 21),
    ("ucs4le", 22),
    ("utf32", 21),
    ("utf-32", 21),
    ("utf32be", 21),
    ("utf-32be", 21),
    ("utf32le", 22),
    ("utf-32le", 22),
    ("932", 45),
    ("949", 47),
    ("936", 46),
    ("gbk", 46),
    ("950", 48),
    ("eucjp", 24),
    ("unix-jis", 24),
    ("ujis", 24),
    ("shift-jis", 25),
    ("pck", 25),
    ("euckr", 26),
    ("5601", 26),
    ("euccn", 27),
    ("gb2312", 27),
    ("euctw", 28),
    ("japan", 24),
    ("korea", 26),
    ("prc", 27),
    ("zh-cn", 27),
    ("chinese", 27),
    ("zh-tw", 28),
    ("taiwan", 28),
    ("cp950", 29),
    ("950", 29),
    ("mac", 57),
    ("mac-roman", 57),
];

/// Port of `enc_canon_search()` (`vendor/mbyte.c:353`).
fn enc_canon_search(name: &str) -> Option<usize> {
    enc_canon_table.iter().position(|&(n, _)| n == name)
}

/// Port of `enc_canon_props()` (`vendor/mbyte.c:366`): the properties of a
/// canonical encoding name, 0 when it is not known.
pub fn enc_canon_props(name: &str) -> i32 {
    if let Some(i) = enc_canon_search(name) {
        enc_canon_table[i].1
    } else if name.starts_with("2byte-") {
        ENC_DBCS
    } else if name.starts_with("8bit-") || name.starts_with("iso-8859-") {
        ENC_8BIT
    } else {
        0
    }
}

/// Port of `enc_skip()` (`vendor/mbyte.c:2313`): skip vim's `2byte-`/`8bit-`
/// head of an encoding name.
pub fn enc_skip(p: &str) -> &str {
    p.strip_prefix("2byte-")
        .or_else(|| p.strip_prefix("8bit-"))
        .unwrap_or(p)
}

/// Port of `enc_alias_search()` (`vendor/mbyte.c:2389`).
fn enc_alias_search(name: &str) -> Option<usize> {
    enc_alias_table
        .iter()
        .find(|&&(n, _)| n == name)
        .map(|&(_, i)| i)
}

/// Port of `enc_canonize()` (`vendor/mbyte.c:2329`): the canonical name for
/// `enc` — lower case, `_` → `-`, `2byte-`/`8bit-` and `microsoft-` dropped,
/// `iso8859n` → `iso-8859-n`, `latin-N` → `latinN`, an alias resolved. A name
/// that is not recognized comes back with only those edits.
pub fn enc_canonize(enc: &str) -> String {
    if enc == "default" {
        // c: `fenc_default`, set from `enc_locale()` by `set_init_1()`.
        return enc_locale().unwrap_or_else(|| "latin1".to_string());
    }
    let r: String = enc
        .chars()
        .map(|c| {
            if c == '_' {
                '-'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect();
    let skipped = r.len() - enc_skip(&r).len();
    let (head, p) = r.split_at(skipped);
    let mut p = p.to_string();
    if let Some(rest) = p.strip_prefix("microsoft-cp") {
        p = format!("cp{rest}");
    }
    if let Some(rest) = p.strip_prefix("iso8859") {
        p = format!("iso-8859{rest}");
    }
    if p.starts_with("iso-8859") && p.as_bytes().get(8) != Some(&b'-') {
        p = format!("iso-8859-{}", &p[8..]);
    }
    if let Some(rest) = p.strip_prefix("latin-") {
        p = format!("latin{rest}");
    }
    if enc_canon_search(&p).is_some() {
        p
    } else if let Some(i) = enc_alias_search(&p) {
        enc_canon_table[i].0.to_string()
    } else {
        format!("{head}{p}")
    }
}

/// Port of `enc_locale()` (`vendor/mbyte.c:2405`): the canonical encoding of
/// the current locale, from `nl_langinfo(CODESET)` or else the locale name.
pub fn enc_locale() -> Option<String> {
    // SAFETY: nl_langinfo/setlocale return pointers to static NUL-terminated
    // strings (or NULL), read immediately.
    let cstr = |p: *const libc::c_char| -> Option<String> {
        if p.is_null() {
            return None;
        }
        let s = unsafe { std::ffi::CStr::from_ptr(p) }
            .to_string_lossy()
            .into_owned();
        (!s.is_empty()).then_some(s)
    };
    let s = cstr(unsafe { libc::nl_langinfo(libc::CODESET) })
        .or_else(|| cstr(unsafe { libc::setlocale(libc::LC_CTYPE, std::ptr::null()) }))
        .or_else(|| {
            ["LC_ALL", "LC_CTYPE", "LANG"]
                .iter()
                .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        })?;
    let b = s.as_bytes();
    let mut buf = String::new();
    let copy_from = match s.find('.') {
        // c: "ja_JP.EUC" → "euc-jp" and the like.
        Some(dot)
            if dot > 2
                && b.get(dot + 1..dot + 4)
                    .is_some_and(|e| e.eq_ignore_ascii_case(b"EUC"))
                && !b
                    .get(dot + 4)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'-')
                && b[dot - 3] == b'_' =>
        {
            buf.push_str("euc-");
            for c in [b[dot - 2], b[dot - 1]] {
                if c.is_ascii_alphanumeric() {
                    buf.push(c.to_ascii_lowercase() as char);
                }
            }
            None
        }
        Some(dot) => Some(dot + 1),
        None => Some(0),
    };
    if let Some(from) = copy_from {
        for &c in b[from..].iter().take(49) {
            match c {
                b'_' | b'-' => buf.push('-'),
                c if c.is_ascii_alphanumeric() => buf.push(c.to_ascii_lowercase() as char),
                _ => break,
            }
        }
    }
    Some(enc_canonize(&buf))
}

/// An iconv(3) conversion descriptor.
#[allow(non_camel_case_types)]
pub type iconv_t = *mut libc::c_void;

// iconv(3) itself: part of the C library on glibc and musl, `libiconv` on
// macOS (the system copy in /usr/lib).
#[cfg_attr(target_os = "macos", link(name = "iconv"))]
extern "C" {
    fn iconv_open(tocode: *const libc::c_char, fromcode: *const libc::c_char) -> iconv_t;
    fn iconv(
        cd: iconv_t,
        inbuf: *mut *mut libc::c_char,
        inbytesleft: *mut libc::size_t,
        outbuf: *mut *mut libc::c_char,
        outbytesleft: *mut libc::size_t,
    ) -> libc::size_t;
    fn iconv_close(cd: iconv_t) -> libc::c_int;
}

/// `vimconv_T.vc_type` (`mbyte_defs.h`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConvType {
    CONV_NONE,
    CONV_TO_UTF8,
    CONV_9_TO_UTF8,
    CONV_TO_LATIN1,
    CONV_TO_LATIN9,
    CONV_ICONV,
}

/// Port of `vimconv_T` (`mbyte_defs.h`): one prepared conversion. The iconv
/// descriptor is closed when the value is dropped (the C's
/// `convert_setup(&vimconv, NULL, NULL)`).
pub struct vimconv_T {
    pub vc_type: ConvType,
    pub vc_fail: bool,
    vc_fd: Option<iconv_t>,
}

impl Drop for vimconv_T {
    fn drop(&mut self) {
        if let Some(fd) = self.vc_fd.take() {
            // SAFETY: `fd` came from a successful iconv_open and is closed once.
            unsafe { iconv_close(fd) };
        }
    }
}

thread_local! {
    /// c: `my_iconv_open`'s `static WorkingStatus iconv_working`:
    /// `None` unknown, `Some(true)` working, `Some(false)` broken.
    static iconv_working: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Port of `my_iconv_open()` (`vendor/mbyte.c:2472`): `iconv_open()`, plus a
/// one-time probe for the broken glibc iconv whose output pointer comes back
/// NULL.
fn my_iconv_open(to: &str, from: &str) -> Option<iconv_t> {
    if iconv_working.with(|w| w.get()) == Some(false) {
        return None;
    }
    let to = std::ffi::CString::new(enc_skip(to)).ok()?;
    let from = std::ffi::CString::new(enc_skip(from)).ok()?;
    // SAFETY: both arguments are valid NUL-terminated strings.
    let fd = unsafe { iconv_open(to.as_ptr(), from.as_ptr()) };
    if fd as isize == -1 {
        return None;
    }
    if iconv_working.with(|w| w.get()).is_none() {
        let mut tobuf = [0 as libc::c_char; 400];
        let mut p = tobuf.as_mut_ptr();
        let mut tolen: libc::size_t = tobuf.len();
        // SAFETY: a reset call (NULL input) writing into a 400-byte buffer.
        unsafe {
            iconv(
                fd,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut p,
                &mut tolen,
            )
        };
        if p.is_null() {
            iconv_working.with(|w| w.set(Some(false)));
            // SAFETY: `fd` is open and is not used again.
            unsafe { iconv_close(fd) };
            return None;
        }
        iconv_working.with(|w| w.set(Some(true)));
    }
    Some(fd)
}

/// Port of `convert_setup_ext()` (`vendor/mbyte.c:2624`) with both
/// `*_unicode_is_utf8` true, i.e. `convert_setup()`: prepare a conversion
/// between two canonical names. `None` is the C's FAIL (no way to convert).
pub fn convert_setup_ext(from: &str, to: &str) -> Option<vimconv_T> {
    let mut vc = vimconv_T {
        vc_type: ConvType::CONV_NONE,
        vc_fail: false,
        vc_fd: None,
    };
    // c: no conversion when one of the names is empty or they are equal.
    if from.is_empty() || to.is_empty() || from == to {
        return Some(vc);
    }
    let from_prop = enc_canon_props(from);
    let to_prop = enc_canon_props(to);
    let from_is_utf8 = from_prop & ENC_UNICODE != 0;
    let to_is_utf8 = to_prop & ENC_UNICODE != 0;
    vc.vc_type = if from_prop & ENC_LATIN1 != 0 && to_is_utf8 {
        ConvType::CONV_TO_UTF8
    } else if from_prop & ENC_LATIN9 != 0 && to_is_utf8 {
        ConvType::CONV_9_TO_UTF8
    } else if from_is_utf8 && to_prop & ENC_LATIN1 != 0 {
        ConvType::CONV_TO_LATIN1
    } else if from_is_utf8 && to_prop & ENC_LATIN9 != 0 {
        ConvType::CONV_TO_LATIN9
    } else {
        vc.vc_fd = my_iconv_open(
            if to_is_utf8 { "utf-8" } else { to },
            if from_is_utf8 { "utf-8" } else { from },
        );
        if vc.vc_fd.is_none() {
            return None;
        }
        ConvType::CONV_ICONV
    };
    Some(vc)
}

/// Port of `iconv_string()` (`vendor/mbyte.c:2509`) without the incomplete-
/// tail handling (`unconvlenp` is always NULL for `iconv()`): convert with
/// iconv(3), writing `?` (two for a double-width character) for what cannot be
/// converted and skipping it. `None` when the conversion fails outright.
fn iconv_string(vcp: &vimconv_T, s: &[u8]) -> Option<Vec<u8>> {
    let fd = vcp.vc_fd?;
    let mut result: Vec<u8> = Vec::new();
    let mut from = 0usize;
    let mut done = 0usize;
    let mut len = 0usize;
    let mut e2big = false;
    loop {
        if len == 0 || e2big {
            // c: allocate enough room for most conversions; grow on E2BIG.
            len += (s.len() - from) * 2 + 40;
            result.resize(len, 0);
        }
        let mut inp = s[from..].as_ptr() as *mut libc::c_char;
        let mut inlen: libc::size_t = s.len() - from;
        let mut outp = result[done..].as_mut_ptr() as *mut libc::c_char;
        let mut outlen: libc::size_t = len - done - 2;
        // SAFETY: the in/out pointers and lengths describe live buffers.
        let r = unsafe { iconv(fd, &mut inp, &mut inlen, &mut outp, &mut outlen) };
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        from = s.len() - inlen;
        let mut to = len - 2 - outlen;
        if r != usize::MAX {
            result.truncate(to);
            return Some(result);
        }
        e2big = errno == libc::E2BIG;
        if !vcp.vc_fail && (errno == libc::EILSEQ || errno == libc::EINVAL) {
            // c: can't convert: insert a '?' and skip a character.
            result[to] = b'?';
            to += 1;
            if utf_ptr2cells(&s[from..]) > 1 {
                result[to] = b'?';
                to += 1;
            }
            from += utfc_ptr2len_len(&s[from..]).max(1) as usize;
        } else if !e2big {
            return None;
        }
        done = to;
    }
}

/// Port of `utf_ptr2len_len()` (`vendor/mbyte.c:946`): like `utf_ptr2len` but
/// never reads past `p.len()`; an incomplete sequence still reports the length
/// its lead byte announces.
pub fn utf_ptr2len_len(p: &[u8]) -> i32 {
    let len = utf8len_tab[p.first().copied().unwrap_or(0) as usize] as usize;
    if len == 1 {
        return 1;
    }
    let m = len.min(p.len());
    if p[1..m].iter().any(|&b| b & 0xc0 != 0x80) {
        return 1;
    }
    len as i32
}

/// Port of `utfc_ptr2len_len()` (`vendor/mbyte.c:1008`): `utfc_ptr2len`
/// within `p.len()` bytes. Uses the crate's `utf_iscomposing` approximation of
/// `utf_composinglike`, as `utfc_ptr2len` does.
pub fn utfc_ptr2len_len(p: &[u8]) -> i32 {
    let size = p.len();
    if size < 1 || p[0] == 0 {
        return 0;
    }
    if p[0] < 0x80 && (size == 1 || p[1] < 0x80) {
        return 1;
    }
    let mut len = utf_ptr2len_len(p) as usize;
    if (len == 1 && p[0] >= 0x80) || len > size {
        return 1;
    }
    while len < size {
        if p[len] < 0x80 {
            break;
        }
        let next = utf_ptr2len_len(&p[len..]) as usize;
        if next > size - len {
            break;
        }
        match char::from_u32(utf_ptr2char(&p[len..]) as u32) {
            Some(ch) if crate::ported::strings::utf_iscomposing(ch) => len += next,
            _ => break,
        }
    }
    len as i32
}

/// Port of `string_convert_ext()` (`vendor/mbyte.c:2698`) with
/// `unconvlenp == NULL`, i.e. `string_convert()`. `None` is the C's NULL
/// (an illegal byte for the internal UTF-8 → latin conversions, or an iconv
/// failure).
pub fn string_convert_ext(vcp: &vimconv_T, ptr: &[u8]) -> Option<Vec<u8>> {
    // c: `lenp == NULL` — the text is a C string.
    let len = ptr.iter().position(|&b| b == 0).unwrap_or(ptr.len());
    let ptr = &ptr[..len];
    if len == 0 {
        return Some(Vec::new());
    }
    let mut d: Vec<u8> = Vec::with_capacity(len * 2);
    match vcp.vc_type {
        ConvType::CONV_TO_UTF8 => {
            for &c in ptr {
                if c < 0x80 {
                    d.push(c);
                } else {
                    d.push(0xc0 + (c >> 6));
                    d.push(0x80 + (c & 0x3f));
                }
            }
        }
        ConvType::CONV_9_TO_UTF8 => {
            for &b in ptr {
                let c: i32 = match b {
                    0xa4 => 0x20ac,
                    0xa6 => 0x0160,
                    0xa8 => 0x0161,
                    0xb4 => 0x017d,
                    0xb8 => 0x017e,
                    0xbc => 0x0152,
                    0xbd => 0x0153,
                    0xbe => 0x0178,
                    b => b as i32,
                };
                let mut buf = [0u8; 6];
                let n = utf_char2bytes(c, &mut buf) as usize;
                d.extend_from_slice(&buf[..n]);
            }
        }
        ConvType::CONV_TO_LATIN1 | ConvType::CONV_TO_LATIN9 => {
            let mut i = 0usize;
            while i < len {
                let l = utf_ptr2len_len(&ptr[i..]) as usize;
                if l == 1 {
                    if utf8len_tab_zero[ptr[i] as usize] == 0 {
                        // c: illegal utf-8 byte cannot be converted.
                        return None;
                    }
                    d.push(ptr[i]);
                } else {
                    let mut c = utf_ptr2char(&ptr[i..]);
                    if vcp.vc_type == ConvType::CONV_TO_LATIN9 {
                        c = match c {
                            0x20ac => 0xa4,
                            0x0160 => 0xa6,
                            0x0161 => 0xa8,
                            0x017d => 0xb4,
                            0x017e => 0xb8,
                            0x0152 => 0xbc,
                            0x0153 => 0xbd,
                            0x0178 => 0xbe,
                            0xa4 | 0xa6 | 0xa8 | 0xb4 | 0xb8 | 0xbc | 0xbd | 0xbe => 0x100,
                            c => c,
                        };
                    }
                    let composing = char::from_u32(c as u32)
                        .is_some_and(crate::ported::strings::utf_iscomposing);
                    if !composing {
                        if c < 0x100 {
                            d.push(c as u8);
                        } else if vcp.vc_fail {
                            return None;
                        } else {
                            d.push(0xbf);
                            if utf_char2cells(c) > 1 {
                                d.push(b'?');
                            }
                        }
                    }
                }
                i += l;
            }
        }
        ConvType::CONV_ICONV => return iconv_string(vcp, ptr),
        ConvType::CONV_NONE => return None,
    }
    Some(d)
}
