//! Port of the UTF-8 codec helpers from `src/nvim/mbyte.c` (vendored at
//! `vendor/mbyte.c`).
//!
//! Only the four routines the JSON decoder (`eval/decode.c`) needs are ported
//! here: [`utf_ptr2char`], [`utf_ptr2len`], [`utf_char2len`] and
//! [`utf_char2bytes`], plus the [`utf8len_tab`] lookup table they share. The
//! rest of `mbyte.c` (composing-char logic, iconv, screen-cell width) is a
//! separate concern and is not ported.
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
}
