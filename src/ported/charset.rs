//! Port of `vim_str2nr()` and the `STR2NR_*` flags from `src/nvim/charset.c`.
//!
//! `charset.c` is not vendored under `csrc/` (only the eval tree is); this is
//! the extern dependency `tv_get_number_chk()` calls, ported against its home
//! file (PORT.md Rule 9). The signature mirrors the C out-parameter form with
//! `Option<&mut …>`.
#![allow(non_upper_case_globals)]

use crate::ported::eval::typval_defs_h::varnumber_T;

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

/// Port of `vim_str2nr()` from `Src/nvim/charset.c`.
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

    // c: leading sign
    if ptr < cap && (bytes[ptr] == b'-' || bytes[ptr] == b'+') {
        negative = bytes[ptr] == b'-';
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
        *n = if negative {
            (un as varnumber_T).wrapping_neg()
        } else {
            un as varnumber_T
        };
    }
}
