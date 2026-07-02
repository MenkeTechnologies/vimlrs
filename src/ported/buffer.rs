//! Port of `src/nvim/buffer.c` + the line-store readers of `src/nvim/memline.c`
//! (vendored as `vendor/buffer.c` / `vendor/memline.c`, structs in
//! `vendor/buffer_defs.h` / `vendor/memline_defs.h`).
//!
//! The minimal buffer substrate the Vimscript eval layer reaches: the `buf_T`
//! model (only the fields eval reads), the global buffer list
//! (`firstbuf`/`lastbuf`/`curbuf`), and the `buflist_*` lookup/creation
//! functions plus the `ml_*` line accessors.
//!
//! RUST-PORT NOTE: three unavoidable deviations, each sanctioned by the port
//! plan and marked at the site:
//!   1. The intrusive `buf_T *b_next/b_prev` list and the `curbuf`/`firstbuf`/
//!      `lastbuf` file-statics become `thread_local!` `Rc<RefCell<buf_T>>`
//!      handles; `FOR_ALL_BUFFERS` walks the `b_next` chain inline.
//!   2. `memline_T`'s memfile block tree (`b_ml.ml_mfp` + the 128-way B-tree)
//!      is replaced by a `Vec<String>` line store; `ml_mfp` collapses to a
//!      `bool` ("buffer loaded"). `ml_get_buf`/`ml_append_buf`/`ml_replace_buf`/
//!      `ml_delete` operate on the Vec, not on `ml_get_buf_impl`/`ml_append_int`
//!      /`ml_replace_buf_len`/`ml_delete_int` (those stay in `vendor` for
//!      reference only).
//!   3. FileID / os_fileid / regexp / autocmd / option / window subsystems the
//!      leaves do not need are deferred: private extern-adapters carry the real
//!      C name with a `Deferred` note (the `vim_regcomp`-in-`ex_eval.rs`
//!      pattern), so name-drift stays honest.
#![allow(non_snake_case, non_upper_case_globals, dead_code, clippy::all)]

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use crate::ported::eval::typval::tv_dict_alloc;
use crate::ported::eval::typval_defs_h::{dict_T, varnumber_T};
use crate::ported::eval_h::{FAIL, OK};

/// `typedef int32_t linenr_T;` — a line number. (types_defs.h; not vendored.)
pub type linenr_T = i32;
/// `typedef int32_t colnr_T;` — a column number. (types_defs.h; not vendored.)
pub type colnr_T = i32;
/// `typedef int handle_T;` — a buffer/window handle. (types_defs.h; not vendored.)
pub type handle_T = i32;

/// `enum bln_values` — flags for [`buflist_new`]. (buffer.h:30; not vendored.)
pub const BLN_CURBUF: i32 = 1;
pub const BLN_LISTED: i32 = 2;
pub const BLN_DUMMY: i32 = 4;
pub const BLN_NEW: i32 = 8;
pub const BLN_NOOPT: i32 = 16;
pub const BLN_NOCURWIN: i32 = 128;

/// `#define BF_CHECK_RO`/`BF_NEVERLOADED`/`BF_DUMMY` — `b_flags` bits.
/// (`vendor/buffer_defs.h`.)
pub const BF_CHECK_RO: i32 = 0x02;
pub const BF_NEVERLOADED: i32 = 0x04;
pub const BF_DUMMY: i32 = 0x80;

/// `memline_T` — the contents of a buffer. (`vendor/memline_defs.h`.)
///
/// RUST-PORT NOTE: `ml_mfp` (the `memfile_T *`) becomes a `bool` "buffer
/// loaded" flag, and the block-tree line storage becomes `ml_lines`, a
/// `Vec<String>`. `ml_line_count` mirrors `ml_lines.len()` exactly as the C
/// field mirrors the tree's line count.
#[derive(Debug, Default)]
pub struct memline_T {
    /// `linenr_T ml_line_count` — number of lines in the buffer.
    pub ml_line_count: linenr_T,
    /// `memfile_T *ml_mfp` — modeled as a load flag (true == `ml_mfp != NULL`).
    pub ml_mfp: bool,
    /// `colnr_T ml_line_textlen` — length of the last cached line + NUL.
    pub ml_line_textlen: colnr_T,
    /// RUST-PORT NOTE: the line store replacing the memfile block tree.
    pub ml_lines: Vec<String>,
}

/// `struct file_buffer` (`buf_T`) — one file/buffer. (`vendor/buffer_defs.h`.)
///
/// Only the fields the eval layer reads are modeled; the rest of Neovim's
/// buf_T (windows, folds, marks, undo, syntax, per-buffer options beyond the
/// handful below) is omitted per the port plan.
#[derive(Debug, Default)]
pub struct buf_T {
    /// `handle_T handle` — the buffer number. `#define b_fnum handle`, so port
    /// sites that read C `b_fnum` read `.handle`.
    pub handle: handle_T,
    /// `memline_T b_ml` — associated memline (line store + line count).
    pub b_ml: memline_T,
    /// `buf_T *b_next` — next in the buffer list.
    pub b_next: Option<Rc<RefCell<buf_T>>>,
    /// `buf_T *b_prev` — previous in the buffer list (`Weak` to avoid a cycle).
    pub b_prev: Option<Weak<RefCell<buf_T>>>,
    /// `int b_nwindows` — number of windows open on this buffer.
    pub b_nwindows: i32,
    /// `int b_flags` — various `BF_` flags.
    pub b_flags: i32,
    /// `char *b_ffname` — full path file name (`None` for no name).
    pub b_ffname: Option<String>,
    /// `char *b_sfname` — short file name (`None` for no name).
    pub b_sfname: Option<String>,
    /// `char *b_fname` — current file name (points at ffname or sfname).
    pub b_fname: Option<String>,
    /// `int b_changed` — 'modified'.
    pub b_changed: i32,
    /// `changedtick_di.di_tv.vval.v_number` — b:changedtick, read by
    /// [`buf_get_changedtick`].
    pub changedtick: varnumber_T,
    /// `bool terminal` — modeled as "is a terminal buffer".
    pub terminal: bool,
    /// `time_t b_last_used` — time the buffer was last used.
    pub b_last_used: i64,
    /// `int b_p_bl` — 'buflisted'.
    pub b_p_bl: i32,
    /// `int b_p_ma` — 'modifiable'.
    pub b_p_ma: i32,
    /// `int b_p_ro` — 'readonly'.
    pub b_p_ro: i32,
    /// `char *b_p_bt` — 'buftype' (empty string == a normal buffer).
    pub b_p_bt: String,
    /// `bool b_modified_was_set` — did an explicit `:set modified`.
    pub b_modified_was_set: bool,
    /// `bool b_help` — true for a help-file buffer.
    pub b_help: bool,
    /// `dict_T *b_vars` — the b: scope Dict.
    pub b_vars: Option<Rc<RefCell<dict_T>>>,
}

thread_local! {
    /// `EXTERN buf_T *firstbuf` — first buffer in the list. (globals.h:394.)
    pub static firstbuf: RefCell<Option<Rc<RefCell<buf_T>>>> = const { RefCell::new(None) };
    /// `EXTERN buf_T *lastbuf` — last buffer in the list. (globals.h:395.)
    pub static lastbuf: RefCell<Option<Rc<RefCell<buf_T>>>> = const { RefCell::new(None) };
    /// `EXTERN buf_T *curbuf` — currently active buffer. (globals.h:396.)
    pub static curbuf: RefCell<Option<Rc<RefCell<buf_T>>>> = const { RefCell::new(None) };
    /// `static int top_file_num` — next free buffer number. (buffer.c.)
    /// RUST-PORT NOTE: `pub` (C's file-static) so the sibling eval test module
    /// can reset it for cross-test isolation of this thread_local.
    pub static top_file_num: Cell<i32> = const { Cell::new(1) };
}

// ---------------------------------------------------------------------------
// Extern-adapters for subsystems not vendored (real C names, deferred bodies).
// Same discipline as `vim_regcomp` in ex_eval.rs: the name traces to vendored
// vendor (so the drift gate recognizes it) but the body is a faithful stand-in.
// ---------------------------------------------------------------------------

/// Port of `handle_get_buffer()` from `Src/map.c` (not vendored).
/// RUST-PORT NOTE: the `buffer_handles` pmap is not modeled; walk the buffer
/// list matching `b_fnum`, which is behaviourally identical.
fn handle_get_buffer(nr: handle_T) -> Option<Rc<RefCell<buf_T>>> {
    // c: FOR_ALL_BUFFERS(buf) — walk firstbuf via b_next.
    let mut cur = firstbuf.with(|f| f.borrow().clone());
    while let Some(buf) = cur {
        if buf.borrow().handle == nr {
            return Some(buf);
        }
        let next = buf.borrow().b_next.clone();
        cur = next;
    }
    None
}

/// Port of `path_fnamecmp()` from `Src/path.c` (not vendored).
/// RUST-PORT NOTE: platform 'fileignorecase' is not modeled; compare exactly.
fn path_fnamecmp(a: &str, b: &str) -> i32 {
    if a == b {
        0
    } else {
        1
    }
}

/// Port of `FullName_save()` from `Src/os/fs.c` (not vendored). Deferred: no
/// path expansion; return the name unchanged so name lookups still work.
fn FullName_save(fname: &str, _force: bool) -> Option<String> {
    Some(fname.to_string())
}

// ---------------------------------------------------------------------------
// buf_get_changedtick — Src/buffer.h:84 (inline)
// ---------------------------------------------------------------------------

/// Port of `buf_get_changedtick()` from `Src/buffer.h:84` (inline; not
/// vendored). Reads `changedtick_di.di_tv.vval.v_number`.
pub fn buf_get_changedtick(buf: &buf_T) -> varnumber_T {
    // c: return buf->changedtick_di.di_tv.vval.v_number;
    buf.changedtick
}

// ---------------------------------------------------------------------------
// memline.c line accessors — backed by the Vec<String> store (RUST-PORT NOTE)
// ---------------------------------------------------------------------------

/// Port of `ml_get_buf()` from `vendor/memline.c:19`.
///
/// RUST-PORT NOTE: replaces `ml_get_buf_impl()` (the block-tree walk). Returns
/// a copy of the line; C returns a pointer into the (read-only) data block.
/// Preserves the two C error paths: no memfile → `""`, invalid lnum → `"???"`.
pub fn ml_get_buf(buf: &mut buf_T, lnum: linenr_T) -> String {
    // c:1889 if (buf->b_ml.ml_mfp == NULL) { there are no lines }
    if !buf.b_ml.ml_mfp {
        buf.b_ml.ml_line_textlen = 1;
        return String::new();
    }
    // c:1893 if (lnum > buf->b_ml.ml_line_count) { invalid line number }
    if lnum > buf.b_ml.ml_line_count {
        buf.b_ml.ml_line_textlen = 4;
        return "???".to_string();
    }
    // c:1911 lnum = MAX(lnum, 1); pretend line 0 is line 1
    let lnum = lnum.max(1);
    let line = buf.b_ml.ml_lines[(lnum - 1) as usize].clone();
    buf.b_ml.ml_line_textlen = line.len() as colnr_T + 1;
    line
}

/// Port of `ml_get()` from `vendor/memline.c:13`. Line from `curbuf`.
pub fn ml_get(lnum: linenr_T) -> String {
    // c: return ml_get_buf_impl(curbuf, lnum, false);
    let cur = curbuf.with(|c| c.borrow().clone()).expect("curbuf is NULL");
    let mut b = cur.borrow_mut();
    ml_get_buf(&mut b, lnum)
}

/// Port of `ml_get_buf_len()` from `vendor/memline.c:36`.
pub fn ml_get_buf_len(buf: &mut buf_T, lnum: linenr_T) -> colnr_T {
    // c: const char *line = ml_get_buf(buf, lnum);
    let line = ml_get_buf(buf, lnum);
    // c: if (*line == NUL) { return 0; }
    if line.is_empty() {
        return 0;
    }
    // c: return buf->b_ml.ml_line_textlen - 1;
    buf.b_ml.ml_line_textlen - 1
}

/// Port of `ml_append_buf()` from `vendor/memline.c:156`.
///
/// RUST-PORT NOTE: replaces `ml_append_flush()`/`ml_append_int()`. Appends
/// `line` after `lnum` (0 == before the first line) in the Vec store and keeps
/// `ml_line_count` in sync. `len`/`newfile` are unused (no memfile).
pub fn ml_append_buf(
    buf: &mut buf_T,
    lnum: linenr_T,
    line: &str,
    _len: colnr_T,
    _newfile: bool,
) -> i32 {
    // c:158 if (buf->b_ml.ml_mfp == NULL) { return FAIL; }
    if !buf.b_ml.ml_mfp {
        return FAIL;
    }
    if lnum < 0 || lnum > buf.b_ml.ml_line_count {
        return FAIL;
    }
    buf.b_ml.ml_lines.insert(lnum as usize, line.to_string());
    buf.b_ml.ml_line_count += 1;
    OK
}

/// Port of `ml_append()` from `vendor/memline.c:141`. Appends into `curbuf`.
pub fn ml_append(lnum: linenr_T, line: &str, len: colnr_T, newfile: bool) -> i32 {
    let cur = curbuf.with(|c| c.borrow().clone()).expect("curbuf is NULL");
    let mut b = cur.borrow_mut();
    ml_append_buf(&mut b, lnum, line, len, newfile)
}

/// Port of `ml_replace_buf()` from `vendor/memline.c:172`.
///
/// RUST-PORT NOTE: replaces `ml_replace_buf_len()`. Replaces line `lnum`
/// in-place in the Vec store. `copy`/`noalloc` are unused (owned `String`).
pub fn ml_replace_buf(
    buf: &mut buf_T,
    lnum: linenr_T,
    line: &str,
    _copy: bool,
    _noalloc: bool,
) -> i32 {
    // c: if (line == NULL) return FAIL;  (line is always present here)
    if lnum < 1 || lnum > buf.b_ml.ml_line_count {
        return FAIL;
    }
    buf.b_ml.ml_lines[(lnum - 1) as usize] = line.to_string();
    OK
}

/// Port of `ml_replace()` from `vendor/memline.c:167`. Replaces in `curbuf`.
pub fn ml_replace(lnum: linenr_T, line: &str, copy: bool) -> i32 {
    let cur = curbuf.with(|c| c.borrow().clone()).expect("curbuf is NULL");
    let mut b = cur.borrow_mut();
    ml_replace_buf(&mut b, lnum, line, copy, false)
}

/// Port of `ml_delete_flags()` from `vendor/memline.c:185`.
///
/// RUST-PORT NOTE: replaces `ml_delete_int()`. Deletes line `lnum` from the
/// Vec store of `curbuf`.
pub fn ml_delete_flags(lnum: linenr_T, _flags: i32) -> i32 {
    let cur = curbuf.with(|c| c.borrow().clone()).expect("curbuf is NULL");
    let mut buf = cur.borrow_mut();
    // c:187 if (lnum < 1 || lnum > curbuf->b_ml.ml_line_count) return FAIL;
    if lnum < 1 || lnum > buf.b_ml.ml_line_count {
        return FAIL;
    }
    buf.b_ml.ml_lines.remove((lnum - 1) as usize);
    buf.b_ml.ml_line_count -= 1;
    OK
}

/// Port of `ml_delete()` from `vendor/memline.c:180`. Deletes from `curbuf`.
pub fn ml_delete(lnum: linenr_T) -> i32 {
    // c: return ml_delete_flags(lnum, 0);
    ml_delete_flags(lnum, 0)
}

// ---------------------------------------------------------------------------
// buffer.c — buffer-list lookups
// ---------------------------------------------------------------------------

/// Port of `otherfile_buf()` from `vendor/buffer.c:249`.
///
/// RUST-PORT NOTE: FileID/os_fileid comparison is deferred; only the name
/// compare (which C also does first, for files that don't exist yet) is kept.
fn otherfile_buf(buf: &buf_T, ffname: Option<&str>) -> bool {
    // c:252 no name is different
    let ffname = match ffname {
        Some(s) if !s.is_empty() => s,
        _ => return true,
    };
    let b_ffname = match buf.b_ffname.as_deref() {
        Some(s) => s,
        None => return true,
    };
    // c:255 if (path_fnamecmp(ffname, buf->b_ffname) == 0) return false;
    if path_fnamecmp(ffname, b_ffname) == 0 {
        return false;
    }
    // c: FileID dev/ino compare — deferred, assume different.
    true
}

/// Port of `buflist_findname_file_id()` from `vendor/buffer.c:235`.
fn buflist_findname_file_id(ffname: &str) -> Option<Rc<RefCell<buf_T>>> {
    // c:238 FOR_ALL_BUFFERS_BACKWARDS(buf) — walk lastbuf via b_prev.
    let mut cur = lastbuf.with(|l| l.borrow().clone());
    while let Some(buf) = cur {
        {
            let b = buf.borrow();
            // c:239 if ((buf->b_flags & BF_DUMMY) == 0 && !otherfile_buf(...))
            if (b.b_flags & BF_DUMMY) == 0 && !otherfile_buf(&b, Some(ffname)) {
                drop(b);
                return Some(buf);
            }
        }
        let prev = buf.borrow().b_prev.as_ref().and_then(|w| w.upgrade());
        cur = prev;
    }
    None
}

/// Port of `buflist_findname()` from `vendor/buffer.c:228`.
pub fn buflist_findname(ffname: &str) -> Option<Rc<RefCell<buf_T>>> {
    // c: FileID file_id; ... return buflist_findname_file_id(ffname, ...);
    buflist_findname_file_id(ffname)
}

/// Port of `buflist_findname_exp()` from `vendor/buffer.c:208`.
pub fn buflist_findname_exp(fname: &str) -> Option<Rc<RefCell<buf_T>>> {
    // c:213 char *ffname = FullName_save(fname, ...);
    match FullName_save(fname, true) {
        // c:224 if (ffname != NULL) { buf = buflist_findname(ffname); ... }
        Some(ffname) => buflist_findname(&ffname),
        None => None,
    }
}

/// Port of `buflist_findnr()` from `vendor/buffer.c:393`.
pub fn buflist_findnr(mut nr: i32) -> Option<Rc<RefCell<buf_T>>> {
    // c:395 if (nr == 0) { nr = curwin->w_alt_fnum; }
    // RUST-PORT NOTE: no windows, so w_alt_fnum is 0 and nr stays 0.
    if nr == 0 {
        nr = 0;
    }
    // c:398 return handle_get_buffer((handle_T)nr);
    handle_get_buffer(nr as handle_T)
}

/// Port of `buflist_findpat()` from `vendor/buffer.c:290`.
///
/// RUST-PORT NOTE: the four-attempt `vim_regcomp`/`buflist_match`/
/// `file_pat_to_reg_pat` regexp search is deferred (the regexp engine is not
/// vendored). The `%`/`#` special-buffer fast path is faithful; a plain
/// pattern falls back to a substring match on the buffer name, honestly
/// approximate. Returns the fnum of the found buffer, or `< 0` on error.
pub fn buflist_findpat(pattern: &str, _unlisted: bool, _diffmode: bool, _curtab_only: bool) -> i32 {
    let mut r#match: i32 = -1;

    // c:296 if (pattern_end == pattern + 1 && (*pattern == '%' || '#'))
    if pattern.len() == 1 && (pattern == "%" || pattern == "#") {
        // c:297 match = *pattern == '%' ? curbuf->b_fnum : curwin->w_alt_fnum;
        // RUST-PORT NOTE: no alternate window, so '#' resolves to 0/-1.
        if pattern == "%" {
            r#match = curbuf.with(|c| c.borrow().as_ref().map_or(-1, |b| b.borrow().handle));
        }
    } else {
        // RUST-PORT NOTE: deferred regexp — substring match on b_ffname/b_sfname/
        // b_fname of the first matching listed buffer.
        let mut cur = lastbuf.with(|l| l.borrow().clone());
        while let Some(buf) = cur {
            {
                let b = buf.borrow();
                let hit = [&b.b_ffname, &b.b_sfname, &b.b_fname]
                    .iter()
                    .filter_map(|n| n.as_deref())
                    .any(|n| n.contains(pattern));
                if b.b_p_bl != 0 && hit {
                    if r#match >= 0 {
                        // c: already found a match
                        return -2;
                    }
                    r#match = b.handle;
                }
            }
            let prev = buf.borrow().b_prev.as_ref().and_then(|w| w.upgrade());
            cur = prev;
        }
    }
    r#match
}

/// Port of `buflist_nr2name()` from `vendor/buffer.c:402`.
///
/// RUST-PORT NOTE: `home_replace_save()` (the `~/` shortener) is deferred; the
/// stored name is returned as-is.
pub fn buflist_nr2name(n: i32, fullname: bool, _helptail: bool) -> Option<String> {
    // c:404 buf_T *buf = buflist_findnr(n); if (buf == NULL) return NULL;
    let buf = buflist_findnr(n)?;
    let b = buf.borrow();
    // c:407 return home_replace_save(..., fullname ? buf->b_ffname : buf->b_fname);
    if fullname {
        b.b_ffname.clone()
    } else {
        b.b_fname.clone()
    }
}

/// Port of `buflist_findfmark()` from `vendor/buffer.c:413`.
///
/// RUST-PORT NOTE: `fmark_T` is not modeled; `buflist_findlnum` reads only
/// `->mark.lnum`, so this collapses to that line number. With no per-window
/// `wininfo`, `find_wininfo` finds nothing and C returns `no_position`, whose
/// `mark.lnum` is 1.
fn buflist_findfmark(_buf: &buf_T) -> linenr_T {
    // c: static fmark_T no_position = { { 1, 0, 0 }, ... }; return &no_position;
    1
}

/// Port of `buflist_findlnum()` from `vendor/buffer.c:422`.
pub fn buflist_findlnum(buf: &buf_T) -> linenr_T {
    // c: return buflist_findfmark(buf)->mark.lnum;
    buflist_findfmark(buf)
}

/// Port of `buflist_name_nr()` from `vendor/buffer.c:429`.
///
/// Returns `Some((fname, lnum))` on success, `None` (== FAIL) otherwise.
/// RUST-PORT NOTE: the C out-params `char **fname`/`linenr_T *lnum` become the
/// returned tuple.
pub fn buflist_name_nr(fnum: i32) -> Option<(String, linenr_T)> {
    // c:431 buf_T *buf = buflist_findnr(fnum);
    let buf = buflist_findnr(fnum)?;
    let b = buf.borrow();
    // c:432 if (buf == NULL || buf->b_fname == NULL) return FAIL;
    let fname = b.b_fname.clone()?;
    // c:437 *lnum = buflist_findlnum(buf);
    let lnum = buflist_findlnum(&b);
    Some((fname, lnum))
}

// ---------------------------------------------------------------------------
// buffer.c — buffer creation
// ---------------------------------------------------------------------------

/// Port of `buflist_new()` from `vendor/buffer.c:12`.
///
/// RUST-PORT NOTE: reduced to the "allocate a new buffer structure" path — the
/// heart of the C function ("This is the ONLY place where a new buffer
/// structure is allocated"). The deferred branches are, verbatim from C and
/// each requiring an unvendored subsystem: the `buflist_findname_file_id`
/// name-reuse update (marks/options/autocmds), the `BLN_CURBUF` curbuf reuse
/// (`buf_freeall`), `buf_copy_options`, the `wininfo` push, and the
/// `EVENT_BUFNEW`/`EVENT_BUFADD` autocmds. The list linking, fnum assignment,
/// changedtick init and `b_p_bl`/`b_flags` init are faithful.
pub fn buflist_new(
    ffname_arg: Option<String>,
    sfname_arg: Option<String>,
    lnum: linenr_T,
    flags: i32,
) -> Option<Rc<RefCell<buf_T>>> {
    let ffname = ffname_arg;
    let sfname = sfname_arg;

    // c:1993 buf = xcalloc(1, sizeof(buf_T));
    let buf = Rc::new(RefCell::new(buf_T::default()));
    {
        let mut b = buf.borrow_mut();
        // c:1995 buf->b_vars = tv_dict_alloc(); init_var_dict(...);
        b.b_vars = Some(tv_dict_alloc());
        // c:1997 buf_init_changedtick(buf);  (changedtick starts at 0)
        b.changedtick = 0;

        // c:2000 if (ffname != NULL) { buf->b_ffname = ffname; buf->b_sfname = ... }
        if let Some(ref f) = ffname {
            b.b_ffname = Some(f.clone());
            b.b_sfname = sfname.clone().or_else(|| Some(f.clone()));
        }

        // c:2027 put new buffer at the end of the buffer list
        // (unconditional here: the BLN_CURBUF reuse branch is deferred.)
        // buf->b_next = NULL; link into firstbuf/lastbuf.
        let last = lastbuf.with(|l| l.borrow().clone());
        match last {
            None => {
                // c:2029 firstbuf == NULL → buffer list is empty
                b.b_prev = None;
                firstbuf.with(|f| *f.borrow_mut() = Some(buf.clone()));
            }
            Some(ref last_rc) => {
                // c:2033 lastbuf->b_next = buf; buf->b_prev = lastbuf;
                last_rc.borrow_mut().b_next = Some(buf.clone());
                b.b_prev = Some(Rc::downgrade(last_rc));
            }
        }
        lastbuf.with(|l| *l.borrow_mut() = Some(buf.clone()));

        // c:2038 buf->b_fnum = top_file_num++;
        let n = top_file_num.with(|t| {
            let v = t.get();
            t.set(v + 1);
            v
        });
        b.handle = n;

        // c:2058 buf->b_fname = buf->b_sfname;
        b.b_fname = b.b_sfname.clone();
        // c:2062 buf->b_u_synced = true;
        // c:2063 buf->b_flags = BF_CHECK_RO | BF_NEVERLOADED;
        b.b_flags = BF_CHECK_RO | BF_NEVERLOADED;
        // c:2064 if (flags & BLN_DUMMY) buf->b_flags |= BF_DUMMY;
        if flags & BLN_DUMMY != 0 {
            b.b_flags |= BF_DUMMY;
        }
        // c:2069 buf->b_p_bl = (flags & BLN_LISTED) ? true : false;
        b.b_p_bl = if flags & BLN_LISTED != 0 { 1 } else { 0 };
        // 'buftype' defaults to empty (a normal buffer); 'modifiable' on.
        b.b_p_ma = 1;
        // The line-mark for the new window (wininfo) is deferred; lnum recorded.
        let _ = lnum;
    }

    Some(buf)
}

/// Port of `buflist_add()` from `vendor/buffer.c:198`.
pub fn buflist_add(fname: Option<String>, flags: i32) -> i32 {
    // c:200 buf_T *buf = buflist_new(fname, NULL, 0, flags);
    match buflist_new(fname, None, 0, flags) {
        // c:201 if (buf != NULL) return buf->b_fnum;
        Some(buf) => buf.borrow().handle,
        // c:204 return 0;
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// buffer.c — 'buftype' predicates
// ---------------------------------------------------------------------------

/// Port of `bt_prompt()` from `vendor/buffer.c:443`.
pub fn bt_prompt(buf: &buf_T) -> bool {
    // c: return buf != NULL && buf->b_p_bt[0] == 'p';
    buf.b_p_bt.as_bytes().first() == Some(&b'p')
}

/// Port of `bt_normal()` from `vendor/buffer.c:449`.
pub fn bt_normal(buf: &buf_T) -> bool {
    // c: return buf != NULL && buf->b_p_bt[0] == NUL;
    buf.b_p_bt.is_empty()
}

/// Port of `bt_quickfix()` from `vendor/buffer.c:455`.
pub fn bt_quickfix(buf: &buf_T) -> bool {
    // c: return buf != NULL && buf->b_p_bt[0] == 'q';
    buf.b_p_bt.as_bytes().first() == Some(&b'q')
}

/// Port of `bt_nofilename()` from `vendor/buffer.c:461`.
pub fn bt_nofilename(buf: &buf_T) -> bool {
    // c: (b_p_bt[0]=='n' && b_p_bt[2]=='f') || b_p_bt[0]=='a' || terminal || 'p'
    let bt = buf.b_p_bt.as_bytes();
    (bt.first() == Some(&b'n') && bt.get(2) == Some(&b'f'))
        || bt.first() == Some(&b'a')
        || buf.terminal
        || bt.first() == Some(&b'p')
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reset the buffer-list thread_local state between cases (Rust reuses test
    // threads, and these globals persist per-thread). No C counterpart — the C
    // globals live for the process; tests need a reset seam.
    fn reset() {
        firstbuf.with(|f| *f.borrow_mut() = None);
        lastbuf.with(|l| *l.borrow_mut() = None);
        curbuf.with(|c| *c.borrow_mut() = None);
        top_file_num.with(|t| t.set(1));
    }

    fn load(buf: &Rc<RefCell<buf_T>>, lines: &[&str]) {
        let mut b = buf.borrow_mut();
        b.b_ml.ml_mfp = true;
        b.b_ml.ml_lines = lines.iter().map(|s| s.to_string()).collect();
        b.b_ml.ml_line_count = lines.len() as linenr_T;
    }

    #[test]
    fn buflist_new_links_and_numbers() {
        reset();
        let a = buflist_new(Some("/tmp/a".into()), None, 0, BLN_LISTED).unwrap();
        let b = buflist_new(Some("/tmp/b".into()), None, 0, BLN_LISTED).unwrap();
        // fnums are sequential from 1
        assert_eq!(a.borrow().handle, 1);
        assert_eq!(b.borrow().handle, 2);
        // list links: first->next == b, b->prev == a
        assert_eq!(a.borrow().b_next.as_ref().unwrap().borrow().handle, 2);
        assert_eq!(
            b.borrow()
                .b_prev
                .as_ref()
                .unwrap()
                .upgrade()
                .unwrap()
                .borrow()
                .handle,
            1
        );
        // 'buflisted' set from BLN_LISTED
        assert_eq!(a.borrow().b_p_bl, 1);
    }

    #[test]
    fn buflist_findnr_and_name() {
        reset();
        let _a = buflist_new(Some("/tmp/a".into()), None, 0, BLN_LISTED).unwrap();
        let b = buflist_new(Some("/tmp/b".into()), None, 0, BLN_LISTED).unwrap();
        // found by number
        assert!(Rc::ptr_eq(&buflist_findnr(2).unwrap(), &b));
        // missing number → None
        assert!(buflist_findnr(99).is_none());
        // name lookup finds the buffer (exact path match)
        assert!(Rc::ptr_eq(&buflist_findname("/tmp/b").unwrap(), &b));
        // buflist_name_nr returns fname + lnum(1)
        let (fname, lnum) = buflist_name_nr(1).unwrap();
        assert_eq!(fname, "/tmp/a");
        assert_eq!(lnum, 1);
    }

    #[test]
    fn ml_get_and_line_count() {
        reset();
        let buf = buflist_new(Some("/tmp/c".into()), None, 0, BLN_LISTED).unwrap();
        load(&buf, &["first", "second", "third"]);
        {
            let mut b = buf.borrow_mut();
            assert_eq!(b.b_ml.ml_line_count, 3);
            assert_eq!(ml_get_buf(&mut b, 1), "first");
            assert_eq!(ml_get_buf(&mut b, 3), "third");
            // ml_get_buf_len excludes the NUL
            assert_eq!(ml_get_buf_len(&mut b, 2), "second".len() as colnr_T);
            // invalid lnum → "???"
            assert_eq!(ml_get_buf(&mut b, 9), "???");
        }
    }

    #[test]
    fn ml_append_replace_delete() {
        reset();
        let buf = buflist_new(Some("/tmp/d".into()), None, 0, BLN_LISTED).unwrap();
        load(&buf, &["one", "two"]);
        curbuf.with(|c| *c.borrow_mut() = Some(buf.clone()));
        // append after line 2
        assert_eq!(ml_append(2, "three", 0, false), OK);
        // replace line 1
        assert_eq!(ml_replace(1, "ONE", true), OK);
        // delete line 2 ("two")
        assert_eq!(ml_delete(2), OK);
        {
            let mut b = buf.borrow_mut();
            assert_eq!(b.b_ml.ml_line_count, 2);
            assert_eq!(ml_get_buf(&mut b, 1), "ONE");
            assert_eq!(ml_get_buf(&mut b, 2), "three");
        }
        // out-of-range delete fails
        assert_eq!(ml_delete(9), FAIL);
    }

    #[test]
    fn buftype_predicates() {
        let mut b = buf_T::default();
        assert!(bt_normal(&b));
        b.b_p_bt = "prompt".into();
        assert!(bt_prompt(&b));
        assert!(bt_nofilename(&b));
        b.b_p_bt = "quickfix".into();
        assert!(bt_quickfix(&b));
        assert!(!bt_normal(&b));
    }
}
