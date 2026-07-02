//! Port of `src/nvim/eval/buffer.c` (vendored at `csrc/eval/buffer.c`).
//!
//! The buffer-related builtin helper layer. Only the leaves whose dependencies
//! are satisfied by the vimlrs buffer model (`crate::ported::buffer`) and the
//! already-ported typval layer are ported here: `find_buffer` (the number/name
//! resolver behind `bufexists()`/`buffer()`), `get_buffer_lines` (behind
//! `getline()`/`getbufline()`), and `get_buffer_info` (behind `getbufinfo()`).
//!
//! Deferred (kept as stubs in `stubs/buffer.rs`): `buf_set_append_line` and
//! `getbufline`, both of which route through `tv_get_buf`/`tv_get_buf_from_arg`
//! — those live in `eval/funcs.c` (mirror `eval/funcs.rs`) and are not yet
//! ported, so calling them faithfully is not yet possible.
#![allow(non_snake_case, dead_code, clippy::all)]

use std::cell::RefCell;
use std::rc::Rc;

use crate::ported::buffer::{
    bt_nofilename, buf_get_changedtick, buflist_findlnum, buflist_findname_exp, buflist_findnr,
    buf_T, curbuf, ml_get_buf, ml_get_buf_len,
};
use crate::ported::eval::typval::{
    tv_dict_add_dict, tv_dict_add_list, tv_dict_add_nr, tv_dict_add_str, tv_list_alloc,
    tv_list_alloc_ret, tv_list_append_string,
};
use crate::ported::eval::typval_defs_h::{
    dict_T, typval_T, typval_vval_union, VarType,
};

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

/// Port of `find_buffer()` from `csrc/eval/buffer.c:47`.
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

/// Port of `get_buffer_lines()` from `csrc/eval/buffer.c:678`.
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
    rettv.v_type = if retlist { VarType::VAR_LIST } else { VarType::VAR_STRING };
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

/// Port of `get_buffer_info()` from `csrc/eval/buffer.c:573`.
///
/// @return buffer options, variables and other attributes in a dictionary.
pub fn get_buffer_info(buf: &Rc<RefCell<buf_T>>) -> Rc<RefCell<dict_T>> {
    // c:575 dict_T *const dict = tv_dict_alloc();
    let dict = crate::ported::eval::typval::tv_dict_alloc();
    let is_curbuf =
        curbuf.with(|c| c.borrow().as_ref().map_or(false, |cb| Rc::ptr_eq(cb, buf)));
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
        let lnum = if is_curbuf { buflist_findlnum(&b) } else { buflist_findlnum(&b) };
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
        tv_dict_add_nr(&mut d, "hidden", (b.b_ml.ml_mfp && b.b_nwindows == 0) as i64);
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
}
