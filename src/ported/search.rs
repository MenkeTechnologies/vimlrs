//! Reference ports of the buffer-search core from `src/nvim/search.c` (not
//! vendored under `csrc/`; names are recognized via the allowlist).
//!
//! [`searchit`] and [`do_searchpair`] back the strict reference ports of
//! `search_cmn`/`searchpair_cmn` in
//! [`funcs`](crate::ported::eval::funcs). The runtime path (`f_search`,
//! `f_searchpair`, …) uses the fusevm-bridge-folded helpers already present in
//! `funcs.rs`; these are the faithful C-shaped spec alongside that synthesis,
//! clearly separated (see PORT.md).
//!
//! RUST-PORT NOTE: `do_searchpair` is defined in `funcs.c` (not `search.c`); it
//! is colocated here only to avoid a name clash with the pre-existing
//! bridge-folded `do_searchpair` in `funcs.rs`. Neovim's `searchit()` is ~500
//! lines driving the C `regexp.c` engine (`regmatch_T`/`vim_regexec_multi`) with
//! `'cpoptions'`, offsets, fuzzy matching, `SEARCH_MSG`, and the time limit. This
//! reference implements the core line scan over the current buffer using the
//! ported `viml_regex` engine instead; the differences are noted at each site.

#![allow(dead_code, non_snake_case)]

use crate::ported::buffer::{curbuf, ml_get};
use crate::ported::eval::eval_expr_to_bool;
use crate::ported::eval::typval_defs_h::typval_T;
use crate::ported::eval_h::FAIL;
use crate::ported::window::{colnr_T, curwin, linenr_T, pos_T};
use crate::viml_regex::Regex;
use std::cell::RefCell;

// c: vim_defs.h:18 — search directions.
pub const FORWARD: i32 = 1;
pub const BACKWARD: i32 = -1;

// c: search.h — search flag subset used by the eval search builtins.
pub const SEARCH_END: i32 = 0x40; // put cursor at end of match
pub const SEARCH_START: i32 = 0x100; // start search without col offset
pub const SEARCH_KEEP: i32 = 0x400; // keep previous search pattern
pub const SEARCH_COL: i32 = 0x1000; // start at specified column instead of zero

// c: search.h:58 — RE_SEARCH: save/use pat in/from search_pattern.
pub const RE_SEARCH: i32 = 0;

thread_local! {
    /// `EXTERN int p_ws;` (option_vars.h) — the `'wrapscan'` option. Defaults on.
    pub static p_ws: RefCell<bool> = const { RefCell::new(true) };
}

/// Port of `searchit()` from `Src/search.c:576` (core line scan).
///
/// Search the current buffer starting at `pos` in direction `dir` for `pat`.
/// Honors `'wrapscan'` (`p_ws`), the `stop_lnum` limit, `SEARCH_START` (accept a
/// match at the start position) and `SEARCH_END` (leave the position at the end
/// of the match). On success writes the 1-based line / 0-based byte column into
/// `pos` and returns a sub-pattern number (`1` for the whole pattern); on no
/// match returns [`FAIL`](crate::ported::eval_h::FAIL).
///
/// RUST-PORT NOTE (signature): the C `searchit(win_T*, buf_T*, pos_T*, pos_T*
/// end_pos, dir, char *pat, size_t patlen, long count, options, pat_use,
/// searchit_arg_T*)` is reduced to the args the eval callers vary; `win`/`buf`
/// are `curwin`/`curbuf`, `end_pos`/`pat_use`/`count`/the time limit are dropped,
/// and matching uses `viml_regex` (char-index spans) instead of `regexp.c`.
pub fn searchit(pos: &mut pos_T, dir: i32, pat: &str, options: i32, stop_lnum: linenr_T) -> i32 {
    // RUST-PORT NOTE: byte<->char column mapping for the reduced String line
    // model (no C counterpart — local closures, not synthesis-adapter fns).
    let char_byte_offsets = |line: &str| -> Vec<colnr_T> {
        let mut v: Vec<colnr_T> = line.char_indices().map(|(i, _)| i as colnr_T).collect();
        v.push(line.len() as colnr_T);
        v
    };
    let byte_to_char = |byte_of_char: &[colnr_T], col: colnr_T| -> usize {
        match byte_of_char.iter().position(|&b| b >= col) {
            Some(i) => i,
            None => byte_of_char.len().saturating_sub(1),
        }
    };
    if pat.is_empty() {
        return FAIL;
    }
    let re = Regex::compile(pat);
    // curbuf->b_ml.ml_line_count
    let line_count = curbuf
        .with(|c| c.borrow().clone())
        .map_or(0, |b| b.borrow().b_ml.ml_line_count);
    if line_count == 0 {
        return FAIL;
    }
    let ic = crate::ported::eval::typval::tv_get_number(&crate::ported::option::get_option_value(
        "ignorecase",
    )) != 0;

    let accept_start = (options & SEARCH_START) != 0;
    let want_end = (options & SEARCH_END) != 0;

    let start_lnum = pos.lnum.clamp(1, line_count);
    let start_col = pos.col.max(0);

    // Two passes: from the start line to the buffer edge, then (if 'wrapscan')
    // wrap around and cover the remaining lines up to and including the start.
    let mut passes = 0;
    let mut lnum = start_lnum;
    loop {
        let line = ml_get(lnum);
        let chars: Vec<char> = line.chars().collect();
        // Byte<->char index maps for this line.
        let byte_of_char = char_byte_offsets(&line);

        if dir == FORWARD {
            // First column to accept a match at, in char index.
            let from_char = if lnum == start_lnum && passes == 0 {
                let sc = byte_to_char(&byte_of_char, start_col);
                if accept_start {
                    sc
                } else {
                    sc + 1
                }
            } else {
                0
            };
            if from_char <= chars.len() {
                if let Some(cap) = re.find_from(&chars, ic, from_char) {
                    let (s, e) = cap.whole();
                    let col_char = if want_end { e.saturating_sub(1) } else { s };
                    pos.lnum = lnum;
                    pos.col = byte_of_char.get(col_char).copied().unwrap_or(0);
                    return 1;
                }
            }
        } else {
            // Backward: find the last match strictly before start_col on the
            // start line, or the last match anywhere on later lines.
            let upto_char = if lnum == start_lnum && passes == 0 {
                let sc = byte_to_char(&byte_of_char, start_col);
                if accept_start {
                    sc + 1
                } else {
                    sc
                }
            } else {
                chars.len() + 1
            };
            let mut best: Option<(usize, usize)> = None;
            let mut from = 0;
            while from <= chars.len() {
                match re.find_from(&chars, ic, from) {
                    Some(cap) => {
                        let (s, e) = cap.whole();
                        if s < upto_char {
                            best = Some((s, e));
                            from = s + 1; // keep scanning for a later match
                        } else {
                            break;
                        }
                    }
                    None => break,
                }
            }
            if let Some((s, e)) = best {
                let col_char = if want_end { e.saturating_sub(1) } else { s };
                pos.lnum = lnum;
                pos.col = byte_of_char.get(col_char).copied().unwrap_or(0);
                return 1;
            }
        }

        // Advance to the next line in `dir`, applying stop_lnum and wrapscan.
        if dir == FORWARD {
            if stop_lnum != 0 && lnum >= stop_lnum {
                if passes == 0 && p_ws.with(|w| *w.borrow()) {
                    // fall through to wrap
                } else {
                    return FAIL;
                }
            }
            lnum += 1;
            if lnum > line_count {
                if passes == 0 && p_ws.with(|w| *w.borrow()) {
                    passes = 1;
                    lnum = 1;
                } else {
                    return FAIL;
                }
            }
            if passes == 1 && lnum > start_lnum {
                return FAIL;
            }
        } else {
            if stop_lnum != 0 && lnum <= stop_lnum {
                if passes == 0 && p_ws.with(|w| *w.borrow()) {
                    // fall through to wrap
                } else {
                    return FAIL;
                }
            }
            lnum -= 1;
            if lnum < 1 {
                if passes == 0 && p_ws.with(|w| *w.borrow()) {
                    passes = 1;
                    lnum = line_count;
                } else {
                    return FAIL;
                }
            }
            if passes == 1 && lnum < start_lnum {
                return FAIL;
            }
        }
    }
}

/// Port of `do_searchpair()` from `Src/eval/funcs.c:6172`.
///
/// Search for a start/middle/end triple, honoring nesting. Returns `1`/`-1` for
/// found and `0` for not found (the C `retval`), writing the match into
/// `match_pos` when non-`None`.
///
/// RUST-PORT NOTE: instead of building the C `pat2`/`pat3` alternation and
/// inspecting `regmatch` submatches to classify each hit, this scans with
/// `viml_regex` and tests the three patterns directly at the current position to
/// decide whether a hit is a start (open), end (close) or middle. Nesting,
/// `foundpos` de-duplication, the `{skip}` expression and `stop_lnum` mirror the
/// C loop. The time limit is not enforced.
#[allow(clippy::too_many_arguments)]
pub fn do_searchpair(
    spat: &str,
    mpat: &str,
    epat: &str,
    dir: i32,
    skip: Option<&typval_T>,
    flags: i32,
    match_pos: Option<&mut pos_T>,
    lnum_stop: linenr_T,
    _time_limit: i64,
) -> i32 {
    // RUST-PORT NOTE: byte<->char column mapping for the reduced String line
    // model (no C counterpart — local closures, not synthesis-adapter fns).
    let char_byte_offsets = |line: &str| -> Vec<colnr_T> {
        let mut v: Vec<colnr_T> = line.char_indices().map(|(i, _)| i as colnr_T).collect();
        v.push(line.len() as colnr_T);
        v
    };
    let byte_to_char = |byte_of_char: &[colnr_T], col: colnr_T| -> usize {
        match byte_of_char.iter().position(|&b| b >= col) {
            Some(i) => i,
            None => byte_of_char.len().saturating_sub(1),
        }
    };
    // RUST-PORT NOTE: set curwin->w_cursor (local closure, not a named fn).
    let set_cursor = |pos: pos_T| {
        if let Some(w) = curwin.with(|c| c.borrow().clone()) {
            w.borrow_mut().w_cursor = pos;
        }
    };
    let mut retval = 0; // c:6178 default: FAIL
    let mut nest = 1; // c:6179

    let use_skip = skip.is_some_and(crate::ported::eval::eval_expr_valid_arg);
    let mut options = SEARCH_KEEP;
    if (flags & 0x10) != 0 {
        // c:6206 SP_START → SEARCH_START
        options |= SEARCH_START;
    }

    // c:6211 save_cursor = curwin->w_cursor; pos = curwin->w_cursor;
    let save_cursor = curwin
        .with(|c| c.borrow().clone())
        .map_or(pos_T::default(), |w| w.borrow().w_cursor);
    let mut pos = save_cursor;
    let mut firstpos = pos_T::default(); // c:6213 clearpos(&firstpos)
    let mut foundpos = pos_T::default(); // c:6215 clearpos(&foundpos)

    let re_open = Regex::compile(if dir == BACKWARD { epat } else { spat });
    let re_close = Regex::compile(if dir == BACKWARD { spat } else { epat });
    let re_mid = if mpat.is_empty() {
        None
    } else {
        Some(Regex::compile(mpat))
    };
    let ic = crate::ported::eval::typval::tv_get_number(&crate::ported::option::get_option_value(
        "ignorecase",
    )) != 0;
    let combined = if mpat.is_empty() {
        format!("\\({spat}\\)\\|\\({epat}\\)")
    } else {
        format!("\\({spat}\\)\\|\\({epat}\\)\\|\\({mpat}\\)")
    };

    loop {
        // c:6231 n = searchit(...)
        let n = searchit(&mut pos, dir, &combined, options, lnum_stop);
        if n == FAIL || (firstpos.lnum != 0 && pos == firstpos) {
            // c:6233 didn't find it or found the first match again: FAIL
            break;
        }
        if firstpos.lnum == 0 {
            firstpos = pos; // c:6238
        }
        if pos == foundpos {
            // c:6240 same position again → advance one char and retry
            if dir == BACKWARD {
                if pos.col > 0 {
                    pos.col -= 1;
                } else if pos.lnum > 1 {
                    pos.lnum -= 1;
                    pos.col = ml_get(pos.lnum).len() as colnr_T;
                }
            } else {
                pos.col += 1;
            }
        }
        foundpos = pos; // c:6250

        options &= !SEARCH_START; // c:6253 clear the start flag

        // c:6256 if the skip pattern matches, ignore this match.
        if use_skip {
            let save = save_cursor;
            set_cursor(pos);
            let do_skip = skip.map(eval_expr_to_bool).unwrap_or(false);
            set_cursor(save);
            if do_skip {
                continue;
            }
        }

        // Classify the hit at `pos` (open/close/middle).
        let line = ml_get(pos.lnum);
        let chars: Vec<char> = line.chars().collect();
        let byte_of_char = char_byte_offsets(&line);
        let at = byte_to_char(&byte_of_char, pos.col);
        let is_close = re_close
            .find_from(&chars, ic, at)
            .is_some_and(|c| c.whole().0 == at);
        let is_open = re_open
            .find_from(&chars, ic, at)
            .is_some_and(|c| c.whole().0 == at);
        let is_mid = re_mid
            .as_ref()
            .and_then(|r| r.find_from(&chars, ic, at))
            .is_some_and(|c| c.whole().0 == at);

        if is_close && !is_open {
            // closing the pair
            nest -= 1;
            if nest == 0 {
                retval = 1; // c: found the end
                break;
            }
        } else if is_open {
            nest += 1; // opening a nested pair
        } else if is_mid && nest == 1 {
            retval = 1; // a middle at the top level matches
            break;
        }
    }

    if retval != 0 {
        if let Some(mp) = match_pos {
            *mp = pos;
        }
        if (flags & 0x01) != 0 {
            // c: SP_NOMOVE → restore cursor
            set_cursor(save_cursor);
        } else {
            set_cursor(pos);
        }
    } else {
        set_cursor(save_cursor);
    }

    retval
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::buffer::{buflist_new, curbuf, firstbuf, lastbuf, top_file_num, BLN_LISTED};

    fn setup_buffer(lines: &[&str]) {
        firstbuf.with(|f| *f.borrow_mut() = None);
        lastbuf.with(|l| *l.borrow_mut() = None);
        curbuf.with(|c| *c.borrow_mut() = None);
        top_file_num.with(|t| t.set(1));
        let buf = buflist_new(Some("/tmp/s".into()), None, 0, BLN_LISTED).unwrap();
        {
            let mut b = buf.borrow_mut();
            b.b_ml.ml_lines = lines.iter().map(|s| s.to_string()).collect();
            b.b_ml.ml_line_count = lines.len() as linenr_T;
            b.b_ml.ml_mfp = true;
        }
        curbuf.with(|c| *c.borrow_mut() = Some(buf));
    }

    #[test]
    fn searchit_forward_finds_next() {
        setup_buffer(&["foo bar", "baz foo", "qux"]);
        p_ws.with(|w| *w.borrow_mut() = false);
        let mut pos = pos_T {
            lnum: 1,
            col: 0,
            coladd: 0,
        };
        // From (1,0) forward for "foo" with SEARCH_START off → skips col 0, wraps
        // to line 2.
        let r = searchit(&mut pos, FORWARD, "foo", 0, 0);
        assert_eq!(r, 1);
        assert_eq!((pos.lnum, pos.col), (2, 4));
    }

    #[test]
    fn searchit_forward_accept_start() {
        setup_buffer(&["foo bar"]);
        p_ws.with(|w| *w.borrow_mut() = false);
        let mut pos = pos_T {
            lnum: 1,
            col: 0,
            coladd: 0,
        };
        // SEARCH_START accepts a match at the start position.
        let r = searchit(&mut pos, FORWARD, "foo", SEARCH_START, 0);
        assert_eq!(r, 1);
        assert_eq!((pos.lnum, pos.col), (1, 0));
    }

    #[test]
    fn searchit_no_match_returns_fail() {
        setup_buffer(&["abc", "def"]);
        p_ws.with(|w| *w.borrow_mut() = true);
        let mut pos = pos_T {
            lnum: 1,
            col: 0,
            coladd: 0,
        };
        assert_eq!(searchit(&mut pos, FORWARD, "zzz", 0, 0), FAIL);
    }

    #[test]
    fn searchit_backward_finds_prev() {
        setup_buffer(&["foo", "bar", "foo x"]);
        p_ws.with(|w| *w.borrow_mut() = false);
        let mut pos = pos_T {
            lnum: 3,
            col: 4,
            coladd: 0,
        };
        // Backward from (3,4) for "foo" → the "foo" at start of line 3.
        let r = searchit(&mut pos, BACKWARD, "foo", 0, 0);
        assert_eq!(r, 1);
        assert_eq!((pos.lnum, pos.col), (3, 0));
    }
}
