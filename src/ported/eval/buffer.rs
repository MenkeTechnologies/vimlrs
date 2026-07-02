//! Port of `src/nvim/eval/buffer.c` (vendored at `vendor/eval/buffer.c`).
//!
//! The buffer-related builtin helper layer. The leaves whose dependencies are
//! satisfied by the vimlrs buffer model (`crate::ported::buffer`) and the
//! already-ported typval layer are ported here: `find_buffer` (the number/name
//! resolver behind `bufexists()`/`buffer()`), `get_buffer_lines` (behind
//! `getline()`/`getbufline()`), `get_buffer_info` (behind `getbufinfo()`),
//! `set_buffer_lines` + `buf_set_append_line` (behind `setbufline()`/
//! `appendbufline()`, editing the `Vec<String>` line store via `ml_append`/
//! `ml_replace`), and `getbufline` (behind `getbufline()`/`getbufoneline()`).
//!
//! RUST-PORT NOTE: these are faithful strict reference ports. The runtime path
//! for the buffer builtins is the fusevm bridge (`eval/funcs.rs`, which owns the
//! `CURBUF` line store); these mirror the C interpreter's own helpers verbatim
//! alongside it, so `#[allow(dead_code)]` applies.
#![allow(non_snake_case, dead_code, clippy::all)]

use std::cell::RefCell;
use std::rc::Rc;

use crate::ported::buffer::{
    bt_nofilename, buf_T, buf_get_changedtick, buflist_findlnum, buflist_findname_exp,
    buflist_findnr, colnr_T, curbuf, linenr_T, ml_append, ml_get, ml_get_buf, ml_get_buf_len,
    ml_replace,
};
use crate::ported::eval::funcs::{tv_get_buf, tv_get_buf_from_arg};
use crate::ported::eval::typval::{
    tv_dict_add_dict, tv_dict_add_list, tv_dict_add_nr, tv_dict_add_str, tv_get_lnum_buf,
    tv_list_alloc, tv_list_alloc_ret, tv_list_append_string, tv_list_len,
};
use crate::ported::eval::typval_defs_h::{dict_T, list_T, typval_T, typval_vval_union, VarType};
use crate::ported::eval::typval_tostring;
use crate::ported::eval_h::OK;
use crate::ported::message::did_emsg;

/// Port of `path_with_url()` from `Src/path.c` (not vendored). Deferred: URL
/// scheme detection isn't modeled, so the nofile/URL name fallback never fires
/// via this path.
fn path_with_url(_fname: &str) -> bool {
    false
}

/// Port of `bufIsChanged()` from `Src/undo.c` (not vendored). Deferred:
/// `file_ff_differs`/`bt_dontwrite`/`b_modified_was_set` are not modeled;
/// approximate with the `b_changed` flag.
fn bufIsChanged(buf: &buf_T) -> bool {
    buf.b_changed != 0
}

/// Port of `buf_has_signs()` from `Src/sign.c` (not vendored). Deferred: no
/// sign subsystem, so a buffer never has signs.
fn buf_has_signs(_buf: &buf_T) -> bool {
    false
}

/// Port of `u_savesub()` from `Src/undo.c:276` (not vendored). Deferred: the undo
/// tree is not modeled, so saving the line about to be replaced always succeeds.
fn u_savesub(_lnum: linenr_T) -> i32 {
    // c: return u_savecommon(curbuf, lnum - 1, lnum + 1, lnum + 1, false);
    OK
}

/// Port of `u_save()` from `Src/undo.c:254` (not vendored). Deferred: no undo
/// tree, so saving the lines between `top` and `bot` always succeeds.
fn u_save(_top: linenr_T, _bot: linenr_T) -> i32 {
    // c: return u_savecommon(curbuf, top, bot, 0, false);
    OK
}

/// Port of `inserted_bytes()` from `Src/change.c:458` (not vendored). Deferred:
/// no extmark/redraw bookkeeping, so signalling an in-place edit is a no-op.
fn inserted_bytes(_lnum: linenr_T, _start_col: colnr_T, _old_col: i32, _new_col: i32) {}

/// Port of `appended_lines_mark()` from `Src/change.c:484` (not vendored).
/// Deferred: no marks/folds/windows to adjust and no redraw, so a no-op.
fn appended_lines_mark(_lnum: linenr_T, _count: i32) {}

/// `cob_T` — saved context for [`change_other_buffer_prepare`] /
/// [`change_other_buffer_restore`] (`vendor/eval/buffer.c:37`).
///
/// RUST-PORT NOTE: the C struct saves `curwin` (`cob_curwin_save`), the autocmd
/// window state (`cob_aco`/`cob_using_aco`) and `VIsual_active`
/// (`cob_save_VIsual_active`). With no window / autocmd / VIsual model only the
/// current buffer needs saving, so the `ml_*` accessors (which key off `curbuf`)
/// act on the target buffer and are restored afterwards.
#[derive(Default)]
struct cob_T {
    cob_curbuf_save: Option<Rc<RefCell<buf_T>>>,
}

/// Port of `change_other_buffer_prepare()` from `vendor/eval/buffer.c:92`.
///
/// RUST-PORT NOTE: reduced to swapping `curbuf` to `buf` so the `ml_*` accessors
/// edit the right buffer. `find_win_for_curbuf()`, the `curwin` save,
/// `VIsual_active` reset and the `aucmd_prepbuf()` autocmd-window fallback (c:96,
/// c:100, c:98, c:104-110) are omitted — no window / VIsual / autocmd subsystem.
fn change_other_buffer_prepare(cob: &mut cob_T, buf: &Rc<RefCell<buf_T>>) {
    // c:94 CLEAR_POINTER(cob);
    // c:101 curbuf = buf;
    cob.cob_curbuf_save = curbuf.with(|c| c.borrow_mut().replace(buf.clone()));
}

/// Port of `change_other_buffer_restore()` from `vendor/eval/buffer.c:113`.
///
/// RUST-PORT NOTE: restores `curbuf`; the aucmd-window / `curwin` /
/// `VIsual_active` restore (c:115-123) is omitted per
/// [`change_other_buffer_prepare`].
fn change_other_buffer_restore(cob: &mut cob_T) {
    // c:118 curwin = cob->cob_curwin_save; curbuf = curwin->w_buffer;
    curbuf.with(|c| *c.borrow_mut() = cob.cob_curbuf_save.take());
}

/// Port of `find_buffer()` from `vendor/eval/buffer.c:47`.
///
/// Find a buffer by number or exact name.
pub fn find_buffer(avar: &typval_T) -> Option<Rc<RefCell<buf_T>>> {
    // c:49 buf_T *buf = NULL;
    let mut buf: Option<Rc<RefCell<buf_T>>> = None;

    // c:51 if (avar->v_type == VAR_NUMBER)
    if avar.v_type == VarType::VAR_NUMBER {
        if let typval_vval_union::v_number(n) = &avar.vval {
            // c:52 buf = buflist_findnr((int)avar->vval.v_number);
            buf = buflist_findnr(*n as i32);
        }
    } else if avar.v_type == VarType::VAR_STRING {
        // c:53 else if (avar->v_type == VAR_STRING && avar->vval.v_string != NULL)
        if let typval_vval_union::v_string(s) = &avar.vval {
            if !s.is_empty() {
                // c:54 buf = buflist_findname_exp(avar->vval.v_string);
                buf = buflist_findname_exp(s);
                // c:55 if (buf == NULL) — try a URL / "nofile" buffer name match.
                if buf.is_none() {
                    // c:58 FOR_ALL_BUFFERS(bp) — walk firstbuf via b_next.
                    let mut bp = crate::ported::buffer::firstbuf.with(|f| f.borrow().clone());
                    while let Some(b) = bp {
                        {
                            let bb = b.borrow();
                            // c:59 bp->b_fname != NULL
                            if let Some(fname) = bb.b_fname.as_deref() {
                                // c:60 (path_with_url(bp->b_fname) || bt_nofilename(bp))
                                //      && strcmp(bp->b_fname, avar->vval.v_string) == 0
                                if (path_with_url(fname) || bt_nofilename(&bb)) && fname == s {
                                    drop(bb);
                                    buf = Some(b);
                                    break;
                                }
                            }
                        }
                        let next = b.borrow().b_next.clone();
                        bp = next;
                    }
                }
            }
        }
    }
    // c:68 return buf;
    buf
}

/// Port of `get_buffer_lines()` from `vendor/eval/buffer.c:678`.
///
/// Get line or list of lines from buffer `buf` into `rettv`. When `retlist` is
/// true the lines are returned as a `VAR_LIST`, otherwise the single line
/// `start` as a `VAR_STRING`.
pub fn get_buffer_lines(
    buf: Option<&Rc<RefCell<buf_T>>>,
    mut start: i32,
    end: i32,
    retlist: bool,
    rettv: &mut typval_T,
) {
    // c:681 rettv->v_type = (retlist ? VAR_LIST : VAR_STRING);
    rettv.v_type = if retlist {
        VarType::VAR_LIST
    } else {
        VarType::VAR_STRING
    };
    rettv.vval = typval_vval_union::v_string(String::new());

    // c:684 if (buf == NULL || buf->b_ml.ml_mfp == NULL || start < 0 || end < start)
    let loaded = buf.map_or(false, |b| b.borrow().b_ml.ml_mfp);
    if buf.is_none() || !loaded || start < 0 || end < start {
        // c:685 if (retlist) tv_list_alloc_ret(rettv, 0);
        if retlist {
            tv_list_alloc_ret(rettv, 0);
        }
        return;
    }
    let buf = buf.unwrap();
    let line_count = buf.borrow().b_ml.ml_line_count;

    if retlist {
        // c:692 if (start < 1) start = 1;
        if start < 1 {
            start = 1;
        }
        // c:695 if (end > buf->b_ml.ml_line_count) end = buf->b_ml.ml_line_count;
        let end = end.min(line_count);
        // c:698 tv_list_alloc_ret(rettv, end - start + 1);
        let l = tv_list_alloc_ret(rettv, (end - start + 1) as isize);
        let mut start = start;
        // c:699 while (start <= end) { tv_list_append_string(...); start++; }
        while start <= end {
            let mut b = buf.borrow_mut();
            let line = ml_get_buf(&mut b, start);
            drop(b);
            tv_list_append_string(&mut l.borrow_mut(), &line);
            start += 1;
        }
    } else {
        // c:705 rettv->v_type = VAR_STRING;
        rettv.v_type = VarType::VAR_STRING;
        // c:706 rettv->vval.v_string = start in range ? xstrnsave(ml_get_buf(...)) : NULL;
        if start >= 1 && start <= line_count {
            let mut b = buf.borrow_mut();
            // xstrnsave(ml_get_buf(buf,start), ml_get_buf_len(buf,start)) copies the
            // whole line: ml_get_buf_len is exactly the returned line's byte length.
            let _ = ml_get_buf_len(&mut b, start);
            let line = ml_get_buf(&mut b, start);
            drop(b);
            rettv.vval = typval_vval_union::v_string(line);
        } else {
            rettv.vval = typval_vval_union::v_string(String::new());
        }
    }
}

/// Port of `set_buffer_lines()` from `vendor/eval/buffer.c:126`.
///
/// Set line or list of lines in buffer `buf` to `lines`. Any type is allowed and
/// converted to a string. `rettv->vval.v_number` is left 0 (OK) or set 1 (FAIL).
///
/// RUST-PORT NOTE: the undo-sync fast path (`u_sync_once`/`u_sync`, c:182-186) is
/// omitted — `u_sync_once` is 0 standalone. `u_savesub`/`u_save` are deferred
/// undo no-ops that report success; `inserted_bytes`/`appended_lines_mark` are
/// deferred redraw/mark no-ops; the `curwin->w_cursor` adjust, `check_cursor_col`
/// and `update_topline` window fix-ups (c:201-203, c:224-233) are omitted — no
/// windows. Editing is via `ml_replace`/`ml_append` on the `curbuf` line store,
/// exactly as C, with `curbuf` swapped to `buf` for a non-current target.
pub fn set_buffer_lines(
    buf: &Rc<RefCell<buf_T>>,
    lnum_arg: linenr_T,
    append: bool,
    lines: &typval_T,
    rettv: &mut typval_T,
) {
    // c:130 linenr_T lnum = lnum_arg + (append ? 1 : 0);
    let mut lnum = lnum_arg + if append { 1 } else { 0 };
    // c:131 int added = 0;
    let mut added: i32 = 0;

    // c:136 const bool is_curbuf = buf == curbuf;
    let is_curbuf = curbuf.with(|c| c.borrow().as_ref().map_or(false, |cb| Rc::ptr_eq(cb, buf)));
    // c:137 if (buf == NULL || (!is_curbuf && buf->b_ml.ml_mfp == NULL) || lnum < 1)
    // (buf is a live handle here, so the NULL leg never fires.)
    if (!is_curbuf && !buf.borrow().b_ml.ml_mfp) || lnum < 1 {
        // c:138 rettv->vval.v_number = 1;  // FAIL
        rettv.vval = typval_vval_union::v_number(1);
        return;
    }

    // c:143 cob_T cob;
    let mut cob = cob_T::default();
    // c:145 if (!is_curbuf) change_other_buffer_prepare(&cob, buf);
    if !is_curbuf {
        // c:146 change_other_buffer_prepare(&cob, buf);
        change_other_buffer_prepare(&mut cob, buf);
    }

    // From here `curbuf == buf`; the `ml_*` accessors operate on it. Read the
    // live line count fresh each time to avoid holding a borrow across an edit.
    let line_count = || {
        curbuf.with(|c| {
            c.borrow()
                .as_ref()
                .map_or(0, |b| b.borrow().b_ml.ml_line_count)
        })
    };

    // c:149 linenr_T append_lnum;
    let append_lnum = if append {
        // c:152 append_lnum = lnum - 1;
        lnum - 1
    } else {
        // c:156 append_lnum = curbuf->b_ml.ml_line_count;
        line_count()
    };

    // c:157 list_T *l = NULL; listitem_T *li = NULL; char *line = NULL;
    let l: Option<Rc<RefCell<list_T>>> = if lines.v_type == VarType::VAR_LIST {
        if let typval_vval_union::v_list(x) = &lines.vval {
            x.clone()
        } else {
            None
        }
    } else {
        None
    };
    // `li` is an index into the list Vec, mirroring the `listitem_T *li` cursor.
    let mut li: usize = 0;
    let mut line: Option<String>;

    'cleanup: {
        // c:162 if (lines->v_type == VAR_LIST)
        if lines.v_type == VarType::VAR_LIST {
            // c:164 if (l == NULL || tv_list_len(l) == 0) goto cleanup;  (success)
            match &l {
                None => break 'cleanup,
                Some(lst) if tv_list_len(&lst.borrow()) == 0 => break 'cleanup,
                _ => {}
            }
            // c:168 li = tv_list_first(l);  → index 0, filled at the loop head.
            line = None;
        } else {
            // c:170 line = typval_tostring(lines, false);
            line = Some(typval_tostring(Some(lines), false));
        }

        // c:174 while (true)
        loop {
            // c:175 if (lines->v_type == VAR_LIST)  // get next string
            if lines.v_type == VarType::VAR_LIST {
                let next = {
                    let lst = l.as_ref().unwrap().borrow();
                    // c:177 if (li == NULL) break;
                    if li >= lst.lv_items.len() {
                        None
                    } else {
                        // c:181 line = typval_tostring(TV_LIST_ITEM_TV(li), false);
                        Some(typval_tostring(Some(&lst.lv_items[li].li_tv), false))
                    }
                };
                match next {
                    None => break,
                    Some(s) => {
                        line = Some(s);
                        // c:182 li = TV_LIST_ITEM_NEXT(l, li);
                        li += 1;
                    }
                }
            }

            // c:185 rettv->vval.v_number = 1;  // FAIL
            rettv.vval = typval_vval_union::v_number(1);
            // c:186 if (line == NULL || lnum > curbuf->b_ml.ml_line_count + 1) break;
            // RUST-PORT NOTE: `typval_tostring` never yields NULL here.
            let count = line_count();
            if lnum > count + 1 {
                break;
            }

            // c:192-196 undo-sync fast path (`u_sync_once`) omitted — see the note.

            let ln = line.as_deref().unwrap();
            // c:197 if (!append && lnum <= curbuf->b_ml.ml_line_count)
            if !append && lnum <= count {
                // c:199 int old_len = (int)strlen(ml_get(lnum));
                let old_len = ml_get(lnum).len() as i32;
                // c:200 if (u_savesub(lnum) == OK && ml_replace(lnum, line, true) == OK)
                if u_savesub(lnum) == OK && ml_replace(lnum, ln, true) == OK {
                    // c:202 inserted_bytes(lnum, 0, old_len, (int)strlen(line));
                    inserted_bytes(lnum, 0, old_len, ln.len() as i32);
                    // c:203-205 cursor-col fix-up omitted (no windows).
                    // c:206 rettv->vval.v_number = 0;  // OK
                    rettv.vval = typval_vval_union::v_number(0);
                }
            } else if added > 0 || u_save(lnum - 1, lnum) == OK {
                // c:208 } else if (added > 0 || u_save(lnum - 1, lnum) == OK)
                // c:210 added++;
                added += 1;
                // c:211 if (ml_append(lnum - 1, line, 0, false) == OK)
                if ml_append(lnum - 1, ln, 0, false) == OK {
                    // c:212 rettv->vval.v_number = 0;  // OK
                    rettv.vval = typval_vval_union::v_number(0);
                }
            }

            // c:216 if (l == NULL) break;  // only one string argument
            if l.is_none() {
                break;
            }
            // c:219 lnum++;
            lnum += 1;
        }
    }

    // c:223 if (added > 0)
    if added > 0 {
        // c:224 appended_lines_mark(append_lnum, added);
        appended_lines_mark(append_lnum, added);
        // c:226-238 window cursor adjust / check_cursor_col / update_topline
        // omitted (no windows).
    }

    // c:240 cleanup:
    if !is_curbuf {
        // c:242 change_other_buffer_restore(&cob);
        change_other_buffer_restore(&mut cob);
    }
}

/// Port of `buf_set_append_line()` from `vendor/eval/buffer.c:257`.
///
/// Set (`append == false`, behind `setbufline()`) or append (`append == true`,
/// behind `appendbufline()`) lines to a buffer. `rettv->vval.v_number` receives
/// 0 (OK) / 1 (FAIL); the caller initialises `rettv` to a `VAR_NUMBER`.
pub fn buf_set_append_line(argvars: &[typval_T], rettv: &mut typval_T, append: bool) {
    // c:259 const int did_emsg_before = did_emsg;
    let did_emsg_before = did_emsg.with(|d| d.get());
    // c:260 buf_T *const buf = tv_get_buf(&argvars[0], false);
    let buf = tv_get_buf(&argvars[0], false);
    // c:261 if (buf == NULL)
    match buf {
        None => {
            // c:262 rettv->vval.v_number = 1;  // FAIL
            rettv.vval = typval_vval_union::v_number(1);
        }
        Some(buf) => {
            // c:264 const linenr_T lnum = tv_get_lnum_buf(&argvars[1], buf);
            // RUST-PORT NOTE: the substrate `tv_get_lnum_buf` ignores `buf`
            // (its `"$"` last-line special is unmodeled), so `None` is passed.
            let lnum = tv_get_lnum_buf(&argvars[1], None) as linenr_T;
            // c:265 if (did_emsg == did_emsg_before)
            if did_emsg.with(|d| d.get()) == did_emsg_before {
                // c:266 set_buffer_lines(buf, lnum, append, &argvars[2], rettv);
                set_buffer_lines(&buf, lnum, append, &argvars[2], rettv);
            }
        }
    }
}

/// Port of `getbufline()` from `vendor/eval/buffer.c:715`.
///
/// `retlist` true → `getbufline()` (a List); false → `getbufoneline()` (the
/// single line as a String).
pub fn getbufline(argvars: &[typval_T], rettv: &mut typval_T, retlist: bool) {
    // c:717 linenr_T lnum = 1;
    let mut lnum: linenr_T = 1;
    // c:718 linenr_T end = 1;
    let mut end: linenr_T = 1;
    // c:719 const int did_emsg_before = did_emsg;
    let did_emsg_before = did_emsg.with(|d| d.get());
    // c:720 buf_T *const buf = tv_get_buf_from_arg(&argvars[0]);
    let buf = tv_get_buf_from_arg(&argvars[0]);
    // c:721 if (buf != NULL)
    if buf.is_some() {
        // c:722 lnum = tv_get_lnum_buf(&argvars[1], buf);
        // RUST-PORT NOTE: substrate `tv_get_lnum_buf` ignores `buf` (no `"$"`).
        lnum = tv_get_lnum_buf(&argvars[1], None) as linenr_T;
        // c:723 if (did_emsg > did_emsg_before) return;
        if did_emsg.with(|d| d.get()) > did_emsg_before {
            return;
        }
        // c:726 end = (argvars[2].v_type == VAR_UNKNOWN ? lnum
        //                                               : tv_get_lnum_buf(&argvars[2], buf));
        end = if argvars.get(2).map_or(VarType::VAR_UNKNOWN, |a| a.v_type) == VarType::VAR_UNKNOWN {
            lnum
        } else {
            tv_get_lnum_buf(&argvars[2], None) as linenr_T
        };
    }

    // c:731 get_buffer_lines(buf, lnum, end, retlist, rettv);
    get_buffer_lines(buf.as_ref(), lnum, end, retlist, rettv);
}

/// Port of `get_buffer_info()` from `vendor/eval/buffer.c:573`.
///
/// @return buffer options, variables and other attributes in a dictionary.
pub fn get_buffer_info(buf: &Rc<RefCell<buf_T>>) -> Rc<RefCell<dict_T>> {
    // c:575 dict_T *const dict = tv_dict_alloc();
    let dict = crate::ported::eval::typval::tv_dict_alloc();
    let is_curbuf = curbuf.with(|c| c.borrow().as_ref().map_or(false, |cb| Rc::ptr_eq(cb, buf)));
    let b = buf.borrow();

    {
        let mut d = dict.borrow_mut();
        // c:577 tv_dict_add_nr(dict, S_LEN("bufnr"), buf->b_fnum);
        tv_dict_add_nr(&mut d, "bufnr", b.handle as i64);
        // c:578 tv_dict_add_str(dict, "name", buf->b_ffname != NULL ? buf->b_ffname : "");
        tv_dict_add_str(&mut d, "name", b.b_ffname.as_deref().unwrap_or(""));
        // c:579 tv_dict_add_nr(dict, "lnum", buf == curbuf ? curwin->w_cursor.lnum
        //                                                   : buflist_findlnum(buf));
        // RUST-PORT NOTE: no window/cursor model — both branches yield the
        // wininfo mark lnum (1).
        let lnum = if is_curbuf {
            buflist_findlnum(&b)
        } else {
            buflist_findlnum(&b)
        };
        tv_dict_add_nr(&mut d, "lnum", lnum as i64);
        // c:581 tv_dict_add_nr(dict, "linecount", buf->b_ml.ml_line_count);
        tv_dict_add_nr(&mut d, "linecount", b.b_ml.ml_line_count as i64);
        // c:582 tv_dict_add_nr(dict, "loaded", buf->b_ml.ml_mfp != NULL);
        tv_dict_add_nr(&mut d, "loaded", b.b_ml.ml_mfp as i64);
        // c:583 tv_dict_add_nr(dict, "listed", buf->b_p_bl);
        tv_dict_add_nr(&mut d, "listed", b.b_p_bl as i64);
        // c:584 tv_dict_add_nr(dict, "changed", bufIsChanged(buf));
        tv_dict_add_nr(&mut d, "changed", bufIsChanged(&b) as i64);
        // c:585 tv_dict_add_nr(dict, "changedtick", buf_get_changedtick(buf));
        tv_dict_add_nr(&mut d, "changedtick", buf_get_changedtick(&b));
        // c:586 tv_dict_add_nr(dict, "hidden", ml_mfp != NULL && b_nwindows == 0);
        tv_dict_add_nr(
            &mut d,
            "hidden",
            (b.b_ml.ml_mfp && b.b_nwindows == 0) as i64,
        );
        // c:587 tv_dict_add_nr(dict, "command", buf == cmdwin_buf);
        // RUST-PORT NOTE: no command-line window, so cmdwin_buf is NULL → 0.
        tv_dict_add_nr(&mut d, "command", 0);

        // c:590 tv_dict_add_dict(dict, "variables", buf->b_vars);
        if let Some(vars) = b.b_vars.clone() {
            tv_dict_add_dict(&mut d, "variables", vars);
        }
    }

    // c:593 list_T *const windows = tv_list_alloc(kListLenMayKnow);
    // RUST-PORT NOTE: no windows, so the list stays empty.
    let windows = tv_list_alloc(0);
    // c:599 tv_dict_add_list(dict, "windows", windows);
    tv_dict_add_list(&mut dict.borrow_mut(), "windows", windows);

    // c:601 if (buf_has_signs(buf)) { tv_dict_add_list(dict, "signs", ...); }
    // Deferred: buf_has_signs() is always false.
    let _ = buf_has_signs(&b);

    // c:606 tv_dict_add_nr(dict, "lastused", buf->b_last_used);
    tv_dict_add_nr(&mut dict.borrow_mut(), "lastused", b.b_last_used);

    // c:608 return dict;
    dict
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::buffer::{buflist_new, firstbuf, lastbuf, top_file_num, BLN_LISTED};

    // Reset the buffer-list thread_local state (Rust reuses test threads).
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
        b.b_ml.ml_line_count = lines.len() as i32;
    }

    #[test]
    fn find_buffer_by_number_and_name() {
        reset();
        let a = buflist_new(Some("/tmp/a".into()), None, 0, BLN_LISTED).unwrap();
        // by number
        let tv = typval_T::from(1i64);
        assert!(Rc::ptr_eq(&find_buffer(&tv).unwrap(), &a));
        // by name (exact path)
        let tv = typval_T::from(String::from("/tmp/a"));
        assert!(Rc::ptr_eq(&find_buffer(&tv).unwrap(), &a));
        // missing number → None
        let tv = typval_T::from(42i64);
        assert!(find_buffer(&tv).is_none());
    }

    #[test]
    fn get_buffer_lines_list_and_single() {
        reset();
        let buf = buflist_new(Some("/tmp/b".into()), None, 0, BLN_LISTED).unwrap();
        load(&buf, &["alpha", "beta", "gamma"]);

        let as_str = |tv: &typval_T| match &tv.vval {
            typval_vval_union::v_string(s) => s.clone(),
            _ => panic!("expected VAR_STRING"),
        };

        // list form: lines 1..3
        let mut rettv = typval_T::default();
        get_buffer_lines(Some(&buf), 1, 3, true, &mut rettv);
        assert_eq!(rettv.v_type, VarType::VAR_LIST);
        if let typval_vval_union::v_list(Some(l)) = &rettv.vval {
            let l = l.borrow();
            assert_eq!(l.lv_len, 3);
            assert_eq!(as_str(&l.lv_items[0].li_tv), "alpha");
            assert_eq!(as_str(&l.lv_items[2].li_tv), "gamma");
        } else {
            panic!("expected VAR_LIST");
        }

        // single form: line 2
        let mut rettv = typval_T::default();
        get_buffer_lines(Some(&buf), 2, 2, false, &mut rettv);
        assert_eq!(rettv.v_type, VarType::VAR_STRING);
        assert_eq!(as_str(&rettv), "beta");

        // out-of-range single → empty string
        let mut rettv = typval_T::default();
        get_buffer_lines(Some(&buf), 9, 9, false, &mut rettv);
        assert_eq!(as_str(&rettv), "");
    }

    #[test]
    fn get_buffer_info_fields() {
        reset();
        let buf = buflist_new(Some("/tmp/c".into()), None, 0, BLN_LISTED).unwrap();
        load(&buf, &["one", "two"]);

        let d = get_buffer_info(&buf);
        let d = d.borrow();
        let nr = |k: &str| match d.dv_hashtab.get(k).map(|tv| &tv.vval) {
            Some(typval_vval_union::v_number(n)) => *n,
            _ => panic!("missing/!nr {k}"),
        };
        assert_eq!(nr("bufnr"), 1);
        assert_eq!(nr("linecount"), 2);
        assert_eq!(nr("loaded"), 1);
        assert_eq!(nr("listed"), 1);
        assert_eq!(nr("changed"), 0);
        assert_eq!(nr("lnum"), 1);
        // name is the ffname
        match d.dv_hashtab.get("name").map(|tv| &tv.vval) {
            Some(typval_vval_union::v_string(s)) => assert_eq!(s, "/tmp/c"),
            _ => panic!("missing name"),
        }
        // variables dict present
        assert!(matches!(
            d.dv_hashtab.get("variables").map(|tv| &tv.vval),
            Some(typval_vval_union::v_dict(Some(_)))
        ));
    }

    use crate::ported::eval::typval_defs_h::listitem_T;

    fn lines_of(buf: &Rc<RefCell<buf_T>>) -> Vec<String> {
        buf.borrow().b_ml.ml_lines.clone()
    }

    fn nr(rettv: &typval_T) -> i64 {
        match &rettv.vval {
            typval_vval_union::v_number(n) => *n,
            _ => panic!("expected VAR_NUMBER"),
        }
    }

    fn list_tv(items: &[&str]) -> typval_T {
        let l = Rc::new(RefCell::new(list_T::default()));
        {
            let mut lb = l.borrow_mut();
            for it in items {
                lb.lv_items.push(listitem_T {
                    li_tv: typval_T::from(it.to_string()),
                });
            }
            lb.lv_len = items.len() as i32;
        }
        typval_T {
            v_type: VarType::VAR_LIST,
            vval: typval_vval_union::v_list(Some(l)),
            ..Default::default()
        }
    }

    #[test]
    fn buf_set_append_line_appends_and_sets() {
        reset();
        // append (appendbufline): insert "X" below line 2 of a non-current buf.
        let buf = buflist_new(Some("/tmp/ap".into()), None, 0, BLN_LISTED).unwrap();
        load(&buf, &["a", "b", "c"]);
        let mut rettv = typval_T::from(0i64);
        let args = [
            typval_T::from(buf.borrow().handle as i64),
            typval_T::from(2i64),
            typval_T::from(String::from("X")),
        ];
        buf_set_append_line(&args, &mut rettv, true);
        assert_eq!(nr(&rettv), 0); // OK
        assert_eq!(lines_of(&buf), vec!["a", "b", "X", "c"]);
        // curbuf restored to None (was None before the change).
        assert!(curbuf.with(|c| c.borrow().is_none()));

        // set (setbufline): replace line 1 with "ONE".
        let mut rettv = typval_T::from(0i64);
        let args = [
            typval_T::from(buf.borrow().handle as i64),
            typval_T::from(1i64),
            typval_T::from(String::from("ONE")),
        ];
        buf_set_append_line(&args, &mut rettv, false);
        assert_eq!(nr(&rettv), 0);
        assert_eq!(lines_of(&buf), vec!["ONE", "b", "X", "c"]);
    }

    #[test]
    fn buf_set_append_line_list_and_missing_buffer() {
        reset();
        let buf = buflist_new(Some("/tmp/lst".into()), None, 0, BLN_LISTED).unwrap();
        load(&buf, &["a", "b"]);
        // append a two-element list below line 1.
        let mut rettv = typval_T::from(0i64);
        let args = [
            typval_T::from(buf.borrow().handle as i64),
            typval_T::from(1i64),
            list_tv(&["x", "y"]),
        ];
        buf_set_append_line(&args, &mut rettv, true);
        assert_eq!(nr(&rettv), 0);
        assert_eq!(lines_of(&buf), vec!["a", "x", "y", "b"]);

        // unknown buffer number → FAIL (1).
        let mut rettv = typval_T::from(0i64);
        let args = [
            typval_T::from(999i64),
            typval_T::from(1i64),
            typval_T::from(String::from("z")),
        ];
        buf_set_append_line(&args, &mut rettv, true);
        assert_eq!(nr(&rettv), 1);
    }

    #[test]
    fn getbufline_list_and_oneline() {
        reset();
        let buf = buflist_new(Some("/tmp/gl".into()), None, 0, BLN_LISTED).unwrap();
        load(&buf, &["one", "two", "three"]);
        let h = buf.borrow().handle as i64;

        let as_str = |tv: &typval_T| match &tv.vval {
            typval_vval_union::v_string(s) => s.clone(),
            _ => panic!("expected VAR_STRING"),
        };

        // getbufline(buf, 1, 3) → List of all three lines.
        let mut rettv = typval_T::default();
        let args = [
            typval_T::from(h),
            typval_T::from(1i64),
            typval_T::from(3i64),
        ];
        getbufline(&args, &mut rettv, true);
        assert_eq!(rettv.v_type, VarType::VAR_LIST);
        if let typval_vval_union::v_list(Some(l)) = &rettv.vval {
            let l = l.borrow();
            assert_eq!(l.lv_len, 3);
            assert_eq!(as_str(&l.lv_items[0].li_tv), "one");
            assert_eq!(as_str(&l.lv_items[2].li_tv), "three");
        } else {
            panic!("expected VAR_LIST");
        }

        // getbufoneline(buf, 2) → the single String "two" (argvars[2] absent).
        let mut rettv = typval_T::default();
        let args = [typval_T::from(h), typval_T::from(2i64)];
        getbufline(&args, &mut rettv, false);
        assert_eq!(rettv.v_type, VarType::VAR_STRING);
        assert_eq!(as_str(&rettv), "two");

        // unknown buffer → empty List for the retlist form.
        let mut rettv = typval_T::default();
        let args = [
            typval_T::from(4242i64),
            typval_T::from(1i64),
            typval_T::from(1i64),
        ];
        getbufline(&args, &mut rettv, true);
        assert_eq!(rettv.v_type, VarType::VAR_LIST);
        if let typval_vval_union::v_list(Some(l)) = &rettv.vval {
            assert_eq!(l.borrow().lv_len, 0);
        } else {
            panic!("expected empty VAR_LIST");
        }
    }
}
