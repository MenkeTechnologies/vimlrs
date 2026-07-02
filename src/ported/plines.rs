//! Port of `src/nvim/plines.c` (not vendored under `vendor/`; names appear as calls
//! in the vendored eval tree, so the drift gate recognizes them via the
//! allowlist).
//!
//! Minimal cell-width / virtual-column helpers over the line store, backing the
//! `virtual_active` branch of [`get_col`](crate::ported::eval::funcs::get_col)
//! and `virtcol()`-style queries.
//!
//! RUST-PORT NOTE: Neovim's charsize path (`CharsizeArg`, `init_charsize_arg`,
//! `linesize_fast`/`linesize_regular`, the marktree inline-virtual-text scan,
//! `'linebreak'`/`'breakindent'`/`'showbreak'`, `'list'`/`'listchars'`) is not
//! modelled. This port keeps only tab expansion via the `'tabstop'` option and a
//! one-cell-per-character width for everything else (double-width CJK is folded
//! to the byte scan; not distinguished). `b_p_ts`/`b_p_vts_array` are read from
//! the global `'tabstop'` option rather than a per-buffer field.

#![allow(dead_code, non_snake_case)]

use crate::ported::eval::typval::tv_get_number;
use crate::ported::mbyte::{utf_ptr2char, utf_ptr2len};
use crate::ported::option::get_option_value;
use crate::ported::window::{colnr_T, win_T};
use std::cell::RefCell;
use std::rc::Rc;

const TAB: u8 = b'\t';

/// Port of `tabstop_padding()` from `Src/indent.c` (fixed-tabstop case only).
///
/// Number of screen cells a `<Tab>` at virtual column `col` occupies for tab
/// width `ts`. RUST-PORT NOTE: the variable-tabstop array (`b_p_vts_array`) is
/// not modelled.
pub fn tabstop_padding(col: colnr_T, ts: i32) -> i32 {
    let ts = if ts <= 0 { 8 } else { ts };
    ts - (col % ts)
}

/// Port of `win_chartabsize()` from `Src/plines.c:48`.
///
/// Number of cells the character at byte string `p` occupies when displayed at
/// virtual column `col` in window `wp`. A `<Tab>` expands to the next tab stop;
/// everything else is one cell (see the module note).
pub fn win_chartabsize(_wp: &Rc<RefCell<win_T>>, p: &str, col: colnr_T) -> i32 {
    // c: if (*p == TAB && ...) return tabstop_padding(col, buf->b_p_ts, ...);
    if p.as_bytes().first() == Some(&TAB) {
        // 'tabstop' (b_p_ts); read from the global option, default 8.
        let ts = tv_get_number(&get_option_value("tabstop")) as i32;
        return tabstop_padding(col, ts);
    }
    // c: return ptr2cells(p);
    ptr2cells(p)
}

/// Port of `ptr2cells()` from `Src/mbyte.c` (simplified).
///
/// Screen cells for the character at `p` (1 for a normal char). RUST-PORT NOTE:
/// wide (CJK) glyphs are not distinguished — always 1.
pub fn ptr2cells(p: &str) -> i32 {
    let _ = p;
    1
}

/// Port of `getvcol()` from `Src/plines.c` (byte-position → virtual column).
///
/// Walk `line` up to byte position `col`, tab-expanding, and return the virtual
/// column of the character at that position (the "start" vcol). RUST-PORT NOTE
/// (signature): the C fills `colnr_T *start/*cursor/*end` out-params for a window
/// + `pos_T`; here it takes the raw `line` and target byte `col` and returns the
/// start vcol, which is all the eval callers need.
pub fn getvcol(wp: &Rc<RefCell<win_T>>, line: &str, col: colnr_T) -> colnr_T {
    let bytes = line.as_bytes();
    let mut vcol: colnr_T = 0;
    let mut i = 0usize;
    while i < bytes.len() && (i as colnr_T) < col {
        // width of the char starting at byte i
        let seg = &bytes[i..];
        let clen = utf_ptr2len(seg).max(1) as usize;
        let _ = utf_ptr2char(seg); // keep parity with the C decode step
        let segstr = std::str::from_utf8(seg).unwrap_or("");
        vcol += win_chartabsize(wp, segstr, vcol);
        i += clen;
    }
    vcol
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabstop_padding_advances_to_next_stop() {
        // ts = 8: at col 0 a tab fills 8; at col 3 it fills 5.
        assert_eq!(tabstop_padding(0, 8), 8);
        assert_eq!(tabstop_padding(3, 8), 5);
        assert_eq!(tabstop_padding(8, 8), 8);
        assert_eq!(tabstop_padding(0, 0), 8); // 0 → default 8
    }
}
