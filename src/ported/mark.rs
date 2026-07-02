//! Port of `src/nvim/mark.c` (not vendored under `csrc/`; the names appear as
//! calls in the vendored eval tree, so the drift gate recognizes them via the
//! allowlist / `nvim_c_functions.txt`).
//!
//! Only [`setmark_pos`] — the mark-setter behind `setpos()`/`setcharpos()` — is
//! ported, over a minimal in-process mark store.
//!
//! RUST-PORT NOTE: Neovim keeps marks in a rich set of per-buffer/global arrays
//! (`buf_T.b_namedm[26]`, the global `namedfm[]`, `b_visual`, `b_op_start`/`_end`,
//! `w_pcmark`, …) plus `fmark_T` view/timestamp metadata and `do_markset_autocmd`
//! notifications. None of that buffer/window substrate is modelled here, so the
//! store folds every mark into one `thread_local!` map keyed by `(fnum, mark
//! char)` holding just the `pos_T`. `setpcmark` and autocmd side effects are
//! elided. This is a faithful reference of the *dispatch* (which mark char is
//! valid for which category) rather than of the full storage layout.

#![allow(dead_code, non_snake_case)]

use crate::ported::buffer::buflist_findnr;
use crate::ported::eval_h::{FAIL, OK};
use crate::ported::window::{curwin, pos_T};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// The folded mark store: `(fnum, mark char) -> pos_T`. `fnum == 0` is used
    /// for the buffer-independent `'`/`` ` `` previous-context mark.
    static MARKS: RefCell<HashMap<(i32, u8), pos_T>> = RefCell::new(HashMap::new());
}

/// Port of `setmark_pos()` from `Src/mark.c:117`.
///
/// Set named mark `c` to position `pos` in buffer `fnum`. Returns
/// [`OK`](crate::ported::eval_h::OK)/[`FAIL`](crate::ported::eval_h::FAIL).
///
/// RUST-PORT NOTE (signature): the C `fmarkv_T *view_pt` view argument is dropped
/// (no view metadata is stored). See the module note for the storage deviation.
pub fn setmark_pos(c: i32, pos: &pos_T, fnum: i32) -> i32 {
    // c: Check for a special key (may cause islower() to crash).
    if c < 0 {
        // c:123
        return FAIL;
    }
    let cc = c as u8;

    // c: if (c == '\'' || c == '`') { … previous-context mark … return OK; }
    if cc == b'\'' || cc == b'`' {
        // c:128 if (pos == &curwin->w_cursor) setpcmark(); else curwin->w_pcmark = *pos;
        // RUST-PORT NOTE: `w_pcmark`/`setpcmark` are not modelled; store the
        // context mark under the buffer-independent key 0.
        let _ = curwin.with(|c| c.borrow().is_some());
        MARKS.with(|m| m.borrow_mut().insert((0, cc), *pos));
        return OK; // c:135
    }

    // c: Can't set a mark in a non-existent buffer.
    if buflist_findnr(fnum).is_none() {
        // c:139
        return FAIL; // c:141
    }

    // c: '"' last-cursor, '[' op-start, ']' op-end, '<'/'>' visual — all stored
    // as folded named marks (the dedicated buf_T fields are not modelled).
    // c:144/152/157/163 followed by do_markset_autocmd (elided).
    if cc == b'"' || cc == b'[' || cc == b']' || cc == b'<' || cc == b'>' {
        MARKS.with(|m| m.borrow_mut().insert((fnum, cc), *pos));
        return OK;
    }

    // c: if (ASCII_ISLOWER(c)) { i = c - 'a'; RESET_FMARK(buf->b_namedm+i, …); }
    if cc.is_ascii_lowercase() {
        // c:181
        MARKS.with(|m| m.borrow_mut().insert((fnum, cc), *pos));
        return OK; // c:184
    }
    // c: if (ASCII_ISUPPER(c) || ascii_isdigit(c)) { RESET_XFMARK(namedfm+i, …); }
    if cc.is_ascii_uppercase() || cc.is_ascii_digit() {
        // c:187 uppercase/numbered marks are global (fnum-tagged); folded here.
        MARKS.with(|m| m.borrow_mut().insert((0, cc), *pos));
        return OK; // c:196
    }

    // c:198 return FAIL;
    FAIL
}

#[cfg(test)]
mod tests {
    /// Look a stored mark up (test-only introspection helper — not a C function).
    fn mark_get_pos(fnum: i32, c: u8) -> Option<pos_T> {
        super::MARKS.with(|m| m.borrow().get(&(fnum, c)).copied())
    }
    use super::*;
    use crate::ported::buffer::{buflist_new, curbuf, firstbuf, lastbuf, top_file_num, BLN_LISTED};

    #[test]
    fn setmark_pos_lower_requires_buffer() {
        MARKS.with(|m| m.borrow_mut().clear());
        firstbuf.with(|f| *f.borrow_mut() = None);
        lastbuf.with(|l| *l.borrow_mut() = None);
        curbuf.with(|c| *c.borrow_mut() = None);
        top_file_num.with(|t| t.set(1));

        let pos = pos_T {
            lnum: 3,
            col: 5,
            coladd: 0,
        };
        // No such buffer → FAIL for a lowercase mark.
        assert_eq!(setmark_pos(b'a' as i32, &pos, 999), FAIL);

        // Register a buffer (gets fnum 1), then the mark is stored.
        let buf = buflist_new(Some("/tmp/mk".into()), None, 0, BLN_LISTED).unwrap();
        let fnum = buf.borrow().handle;
        curbuf.with(|c| *c.borrow_mut() = Some(buf.clone()));
        assert_eq!(setmark_pos(b'a' as i32, &pos, fnum), OK);
        assert_eq!(mark_get_pos(fnum, b'a'), Some(pos));
    }

    #[test]
    fn setmark_pos_rejects_negative_and_junk() {
        assert_eq!(setmark_pos(-1, &pos_T::default(), 0), FAIL);
        // '!' is neither special nor alnum → FAIL.
        assert_eq!(setmark_pos(b'!' as i32, &pos_T::default(), 0), FAIL);
    }

    #[test]
    fn setmark_pos_context_mark_no_buffer() {
        MARKS.with(|m| m.borrow_mut().clear());
        let pos = pos_T {
            lnum: 7,
            col: 1,
            coladd: 0,
        };
        // '`'/'\'' need no buffer.
        assert_eq!(setmark_pos(b'`' as i32, &pos, 0), OK);
        assert_eq!(mark_get_pos(0, b'`'), Some(pos));
    }
}
