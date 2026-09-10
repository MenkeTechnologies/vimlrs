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
//! Not ported: iconv, and screen-cell width (`utf_char2cells` and friends), whose
//! data is utf8proc's property tables.
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
