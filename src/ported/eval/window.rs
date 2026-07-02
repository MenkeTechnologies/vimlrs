//! Port of `src/nvim/eval/window.c` (vendored at `csrc/eval/window.c`).
//!
//! The window-lookup helper layer behind the `win_*`/`tabpage*` builtins. The
//! window/tab-page list itself is modelled in [`crate::ported::window`]; these
//! helpers walk it. When no windows/tabs are registered every lookup naturally
//! finds nothing (empty lists → `None`/0), which matches a standalone eval.
//!
//! RUST-PORT NOTE: the C `win_T *`/`tabpage_T *` pointers become
//! `Option<Rc<RefCell<win_T>>>`/`Option<Rc<RefCell<tabpage_T>>>`, pointer
//! identity becomes [`Rc::ptr_eq`], and the `tabpage_T **tpp` out-parameter of
//! [`win_id2wp_tp`] becomes `Option<&mut Option<Rc<RefCell<tabpage_T>>>>`. The
//! deep window-management ops (`switch_win`, `get_win_info`, `win_execute_*`,
//! frame layout) are not reachable from a standalone eval and remain honest
//! stubs below.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use std::rc::Rc;

use crate::ported::eval::typval::{
    tv_get_number, tv_get_number_chk, tv_list_alloc_ret, tv_list_append_number,
};
use crate::ported::eval::typval_defs_h::{list_T, typval_T, varnumber_T, VarType::VAR_UNKNOWN};
use crate::ported::eval_h::FAIL;
use crate::ported::message::emsg;
use crate::ported::window::{
    curtab, curwin, find_tabpage, first_tabpage, firstwin, handle_T, tabpage_T, win_T,
    win_get_tabwin,
};

/// `LOWEST_WIN_ID = 1000` — lowest number used for a window ID (`csrc/window.h:35`).
const LOWEST_WIN_ID: i32 = 1000;

/// `e_invalwindow[] = N_("E957: Invalid window number")` (`errors.h:181`; that
/// file is not yet vendored under `csrc/`).
const e_invalwindow: &str = "E957: Invalid window number";

/// Port of `win_has_winnr()` from `csrc/eval/window.c:42`.
pub fn win_has_winnr(
    wp: &Rc<std::cell::RefCell<win_T>>,
    tp: &Rc<std::cell::RefCell<tabpage_T>>,
) -> bool {
    // c:44 return (wp == (tp == curtab ? curwin : tp->tp_curwin))
    let twin = if curtab.with(|c| c.borrow().as_ref().is_some_and(|ct| Rc::ptr_eq(ct, tp))) {
        curwin.with(|c| c.borrow().clone())
    } else {
        tp.borrow().tp_curwin.clone()
    };
    twin.as_ref().is_some_and(|t| Rc::ptr_eq(t, wp))
        // c:45-46 || (!wp->w_config.hide && wp->w_config.focusable)
        || (!wp.borrow().w_config.hide && wp.borrow().w_config.focusable)
}

/// Port of `win_getid()` from `csrc/eval/window.c:49`.
pub fn win_getid(argvars: &[typval_T]) -> i32 {
    if argvars.first().map_or(true, |t| t.v_type == VAR_UNKNOWN) {
        // c:50
        return curwin.with(|c| c.borrow().as_ref().map_or(0, |w| w.borrow().handle));
        // c:51
    }
    let mut winnr = tv_get_number(&argvars[0]) as i32; // c:53
    if winnr <= 0 {
        // c:55
        return 0; // c:56
    }

    let tp: Option<Rc<std::cell::RefCell<tabpage_T>>>;
    let mut wp: Option<Rc<std::cell::RefCell<win_T>>>;
    if argvars.get(1).map_or(true, |t| t.v_type == VAR_UNKNOWN) {
        // c:60
        tp = curtab.with(|c| c.borrow().clone()); // c:61
        wp = firstwin.with(|c| c.borrow().clone()); // c:62
    } else {
        let mut tabnr = tv_get_number(&argvars[1]) as i32; // c:64
                                                           // c:65 FOR_ALL_TABS(tp2): first matching --tabnr == 0
        let mut found: Option<Rc<std::cell::RefCell<tabpage_T>>> = None;
        let mut tp2 = first_tabpage.with(|c| c.borrow().clone());
        while let Some(t) = tp2.clone() {
            tabnr -= 1;
            if tabnr == 0 {
                // c:66
                found = Some(t.clone()); // c:67
                break; // c:68
            }
            tp2 = t.borrow().tp_next.clone();
        }
        tp = found;
        if tp.is_none() {
            // c:71
            return -1; // c:72
        }
        let tp_rc = tp.clone().unwrap();
        if curtab.with(|c| c.borrow().as_ref().is_some_and(|ct| Rc::ptr_eq(ct, &tp_rc))) {
            // c:74
            wp = firstwin.with(|c| c.borrow().clone()); // c:75
        } else {
            wp = tp_rc.borrow().tp_firstwin.clone(); // c:77
        }
    }
    // c:80 for (; wp != NULL; wp = wp->w_next)
    if let Some(tp_rc) = tp {
        while let Some(w) = wp.clone() {
            winnr -= win_has_winnr(&w, &tp_rc) as i32; // c:81
            if winnr == 0 {
                return w.borrow().handle; // c:82
            }
            wp = w.borrow().w_next.clone();
        }
    }
    0 // c:85
}

/// Port of `win_id2tabwin()` from `csrc/eval/window.c:89`.
pub fn win_id2tabwin(argvars: &[typval_T], rettv: &mut typval_T) {
    let id = tv_get_number(&argvars[0]) as handle_T; // c:91

    let mut winnr = 1; // c:93
    let mut tabnr = 1; // c:94
    win_get_tabwin(id, &mut tabnr, &mut winnr); // c:95

    let list = tv_list_alloc_ret(rettv, 2); // c:97
    tv_list_append_number(&mut list.borrow_mut(), tabnr as varnumber_T); // c:98
    tv_list_append_number(&mut list.borrow_mut(), winnr as varnumber_T); // c:99
}

/// Port of `win_id2wp()` from `csrc/eval/window.c:102`.
pub fn win_id2wp(id: i32) -> Option<Rc<std::cell::RefCell<win_T>>> {
    win_id2wp_tp(id, None) // c:104
}

/// Port of `win_id2wp_tp()` from `csrc/eval/window.c:109`.
/// Return the window and tab pointer of window "id".
/// Returns NULL when not found.
///
/// RUST-PORT NOTE: the C `tabpage_T **tpp` out-parameter becomes
/// `Option<&mut Option<Rc<RefCell<tabpage_T>>>>`; `*tpp = tp` (only on success)
/// becomes `*out = Some(tp_rc)` inside the match, matching `if (tpp != NULL)`.
pub fn win_id2wp_tp(
    id: i32,
    tpp: Option<&mut Option<Rc<std::cell::RefCell<tabpage_T>>>>,
) -> Option<Rc<std::cell::RefCell<win_T>>> {
    // c:111 FOR_ALL_TAB_WINDOWS(tp, wp)
    let mut tp = first_tabpage.with(|c| c.borrow().clone());
    while let Some(tp_rc) = tp.clone() {
        let is_curtab =
            curtab.with(|c| c.borrow().as_ref().is_some_and(|ct| Rc::ptr_eq(ct, &tp_rc)));
        let mut wp = if is_curtab {
            firstwin.with(|c| c.borrow().clone())
        } else {
            tp_rc.borrow().tp_firstwin.clone()
        };
        while let Some(w) = wp.clone() {
            if w.borrow().handle == id {
                // c:112
                if let Some(out) = tpp {
                    *out = Some(tp_rc.clone()); // c:113-115 *tpp = tp
                }
                return Some(w); // c:116
            }
            wp = w.borrow().w_next.clone();
        }
        tp = tp_rc.borrow().tp_next.clone();
    }

    None // c:120 return NULL
}

/// Port of `win_id2win()` from `csrc/eval/window.c:123`.
pub fn win_id2win(argvars: &[typval_T]) -> i32 {
    let mut nr = 1; // c:125
    let id = tv_get_number(&argvars[0]) as i32; // c:126

    // c:128 FOR_ALL_WINDOWS_IN_TAB(wp, curtab): head is firstwin (tp == curtab)
    if let Some(tp_rc) = curtab.with(|c| c.borrow().clone()) {
        let mut wp = firstwin.with(|c| c.borrow().clone());
        while let Some(w) = wp.clone() {
            if w.borrow().handle == id {
                // c:129
                return if win_has_winnr(&w, &tp_rc) { nr } else { 0 }; // c:130
            }
            nr += win_has_winnr(&w, &tp_rc) as i32; // c:132
            wp = w.borrow().w_next.clone();
        }
    }
    0 // c:134
}

/// Port of `win_findbuf()` from `csrc/eval/window.c:137`.
///
/// RUST-PORT NOTE: C dereferences `wp->w_buffer` directly (always non-NULL);
/// the placeholder `w_buffer` is an `Option`, so a window with no buffer simply
/// never matches.
pub fn win_findbuf(argvars: &[typval_T], list: &mut list_T) {
    let bufnr = tv_get_number(&argvars[0]) as i32; // c:139

    // c:141 FOR_ALL_TAB_WINDOWS(tp, wp)
    let mut tp = first_tabpage.with(|c| c.borrow().clone());
    while let Some(tp_rc) = tp.clone() {
        let is_curtab =
            curtab.with(|c| c.borrow().as_ref().is_some_and(|ct| Rc::ptr_eq(ct, &tp_rc)));
        let mut wp = if is_curtab {
            firstwin.with(|c| c.borrow().clone())
        } else {
            tp_rc.borrow().tp_firstwin.clone()
        };
        while let Some(w) = wp.clone() {
            let matches = w
                .borrow()
                .w_buffer
                .as_ref()
                .is_some_and(|b| b.borrow().handle == bufnr);
            if matches {
                // c:142
                tv_list_append_number(list, w.borrow().handle as varnumber_T); // c:143
            }
            wp = w.borrow().w_next.clone();
        }
        tp = tp_rc.borrow().tp_next.clone();
    }
}

/// Port of `find_win_by_nr()` from `csrc/eval/window.c:153`.
/// Find window specified by "vp" in tabpage "tp".
///
/// @param tp  NULL for current tab page
/// @return  current window if "vp" is number zero.
///          NULL if not found.
pub fn find_win_by_nr(
    vp: &typval_T,
    tp: Option<Rc<std::cell::RefCell<tabpage_T>>>,
) -> Option<Rc<std::cell::RefCell<win_T>>> {
    let mut nr = tv_get_number_chk(vp, None) as i32; // c:155

    if nr < 0 {
        // c:157
        return None; // c:158
    }

    if nr == 0 {
        // c:161
        return curwin.with(|c| c.borrow().clone()); // c:162
    }

    // c:166 This method accepts NULL as an alias for curtab.
    let tp = match tp {
        Some(tp) => Some(tp),
        None => curtab.with(|c| c.borrow().clone()), // c:167
    };

    // c:170 FOR_ALL_WINDOWS_IN_TAB(wp, tp)
    if let Some(tp_rc) = tp {
        let is_curtab =
            curtab.with(|c| c.borrow().as_ref().is_some_and(|ct| Rc::ptr_eq(ct, &tp_rc)));
        let mut wp = if is_curtab {
            firstwin.with(|c| c.borrow().clone())
        } else {
            tp_rc.borrow().tp_firstwin.clone()
        };
        while let Some(w) = wp.clone() {
            if nr >= LOWEST_WIN_ID {
                // c:171
                if w.borrow().handle == nr {
                    // c:172
                    return Some(w); // c:173
                }
            } else {
                nr -= 1;
                if nr <= 0 {
                    // c:175
                    return Some(w); // c:176
                }
            }
            wp = w.borrow().w_next.clone();
        }
    }
    None // c:179
}

/// Port of `find_win_by_nr_or_id()` from `csrc/eval/window.c:185`.
/// Find a window: When using a Window ID in any tab page, when using a number
/// in the current tab page.
/// Returns NULL when not found.
pub fn find_win_by_nr_or_id(vp: &typval_T) -> Option<Rc<std::cell::RefCell<win_T>>> {
    let nr = tv_get_number_chk(vp, None) as i32; // c:187

    if nr >= LOWEST_WIN_ID {
        // c:189
        return win_id2wp(tv_get_number(vp) as i32); // c:190
    }

    find_win_by_nr(vp, None) // c:193
}

/// Port of `find_tabwin()` from `csrc/eval/window.c:197`.
/// Find window specified by "wvp" in tabpage "tvp".
pub fn find_tabwin(wvp: &typval_T, tvp: &typval_T) -> Option<Rc<std::cell::RefCell<win_T>>> {
    let mut wp: Option<Rc<std::cell::RefCell<win_T>>> = None; // c:199
    let mut tp: Option<Rc<std::cell::RefCell<tabpage_T>>> = None; // c:200

    if wvp.v_type != VAR_UNKNOWN {
        // c:202
        if tvp.v_type != VAR_UNKNOWN {
            // c:203
            let n = tv_get_number(tvp) as i32; // c:204
            if n >= 0 {
                // c:205
                tp = find_tabpage(n); // c:206
            }
        } else {
            tp = curtab.with(|c| c.borrow().clone()); // c:209
        }

        if tp.is_some() {
            // c:212
            wp = find_win_by_nr(wvp, tp); // c:213
        }
    } else {
        wp = curwin.with(|c| c.borrow().clone()); // c:216
    }

    wp // c:219
}

/// Port of `get_optional_window()` from `csrc/eval/funcs.c:769` (that file's
/// mirror `eval/funcs.rs` is owned by the funcs agent; this window-lookup helper
/// is placed with the other window helpers it delegates to). `idx` selects the
/// argument to interpret as a window.
///
/// RUST-PORT NOTE: `int idx` becomes `usize` (it only ever indexes `argvars`).
pub fn get_optional_window(
    argvars: &[typval_T],
    idx: usize,
) -> Option<Rc<std::cell::RefCell<win_T>>> {
    if argvars.get(idx).map_or(true, |t| t.v_type == VAR_UNKNOWN) {
        // c:771
        return curwin.with(|c| c.borrow().clone()); // c:772
    }

    let win = find_win_by_nr_or_id(&argvars[idx]); // c:775
    if win.is_none() {
        // c:776
        emsg(e_invalwindow); // c:777
        return None; // c:778
    }
    win // c:780
}

// --- honest stubs: deep window ops not reachable from a standalone eval ------
// (RUST-PORT NOTE: these need the full window-management substrate — frames,
// resize, autocmds, the ex command layer — that a standalone interpreter does
// not model; they stay no-op/FAIL as documented until that substrate exists.)

/// Port of `win_has_winnr()`-adjacent `get_winnr()` — number of a window in a
/// tab page; no full winnr() substrate → 0.
pub fn get_winnr(_argvar: &typval_T) -> i32 {
    0
}
/// Port of `switch_win()` — temporarily switch to a window; none to switch → FAIL.
pub fn switch_win() -> i32 {
    FAIL
}
/// Port of `switch_win_noblock()` — as [`switch_win`] without autocmd blocking → FAIL.
pub fn switch_win_noblock() -> i32 {
    FAIL
}
/// Port of `restore_win()` — restore after [`switch_win`]; no-op.
pub fn restore_win() {}
/// Port of `restore_win_noblock()` — restore after [`switch_win_noblock`]; no-op.
pub fn restore_win_noblock() {}
/// Port of `get_win_info()` — fill a window-info Dict; no window → no-op.
pub fn get_win_info() {}
/// Port of `get_tabpage_info()` — fill a tab-page-info Dict; no-op.
pub fn get_tabpage_info() {}
/// Port of `get_framelayout()` — append a window-frame layout to a List; no-op.
pub fn get_framelayout() {}
/// Port of `win_execute_before()` — set up `win_execute()`; no window → no-op.
pub fn win_execute_before() {}
/// Port of `win_execute_after()` — tear down `win_execute()`; no-op.
pub fn win_execute_after() {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::eval::typval_defs_h::{
        typval_vval_union::v_number, VarLockStatus, VarType::VAR_NUMBER,
    };
    use crate::ported::window::{buf_T, tabpage_T, win_T, WinConfig};
    use std::cell::RefCell;
    // (win_T / tabpage_T / Rc also reachable via `super::*`; re-imported for clarity.)

    fn num(n: varnumber_T) -> typval_T {
        typval_T {
            v_type: VAR_NUMBER,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: v_number(n),
        }
    }

    /// Install a one-tab, two-window layout into the thread-local lists and
    /// return the two windows. w0 handle 1000 (buffer 7), w1 handle 1001
    /// (buffer 9).
    fn setup() -> (Rc<RefCell<win_T>>, Rc<RefCell<win_T>>) {
        let b7 = Rc::new(RefCell::new(buf_T { handle: 7, ..Default::default() }));
        let b9 = Rc::new(RefCell::new(buf_T { handle: 9, ..Default::default() }));
        let w0 = Rc::new(RefCell::new(win_T {
            handle: 1000,
            w_buffer: Some(b7),
            w_config: WinConfig {
                focusable: true,
                hide: false,
            },
            ..Default::default()
        }));
        let w1 = Rc::new(RefCell::new(win_T {
            handle: 1001,
            w_buffer: Some(b9),
            w_config: WinConfig {
                focusable: true,
                hide: false,
            },
            ..Default::default()
        }));
        w0.borrow_mut().w_next = Some(w1.clone());
        w1.borrow_mut().w_prev = Some(Rc::downgrade(&w0));
        let tab = Rc::new(RefCell::new(tabpage_T {
            handle: 1,
            tp_firstwin: Some(w0.clone()),
            tp_lastwin: Some(w1.clone()),
            tp_curwin: Some(w0.clone()),
            ..Default::default()
        }));
        firstwin.with(|c| *c.borrow_mut() = Some(w0.clone()));
        curwin.with(|c| *c.borrow_mut() = Some(w0.clone()));
        first_tabpage.with(|c| *c.borrow_mut() = Some(tab.clone()));
        curtab.with(|c| *c.borrow_mut() = Some(tab.clone()));
        (w0, w1)
    }

    #[test]
    fn win_id2wp_and_win_id2win() {
        let (w0, w1) = setup();
        assert!(Rc::ptr_eq(&win_id2wp(1000).unwrap(), &w0));
        assert!(Rc::ptr_eq(&win_id2wp(1001).unwrap(), &w1));
        assert!(win_id2wp(4242).is_none());

        // win_id2win returns the 1-based winnr in curtab, 0 if unknown.
        assert_eq!(win_id2win(&[num(1000)]), 1);
        assert_eq!(win_id2win(&[num(1001)]), 2);
        assert_eq!(win_id2win(&[num(4242)]), 0);
    }

    #[test]
    fn win_id2wp_tp_sets_tab() {
        let (_w0, w1) = setup();
        let mut tab_out: Option<Rc<RefCell<tabpage_T>>> = None;
        let wp = win_id2wp_tp(1001, Some(&mut tab_out)).unwrap();
        assert!(Rc::ptr_eq(&wp, &w1));
        assert_eq!(tab_out.unwrap().borrow().handle, 1);
    }

    #[test]
    fn find_win_by_nr_or_id_number_vs_id() {
        let (w0, w1) = setup();
        // nr 0 → curwin (w0)
        assert!(Rc::ptr_eq(&find_win_by_nr_or_id(&num(0)).unwrap(), &w0));
        // nr 2 (< LOWEST_WIN_ID) → 2nd window by count
        assert!(Rc::ptr_eq(&find_win_by_nr_or_id(&num(2)).unwrap(), &w1));
        // id >= LOWEST_WIN_ID → by handle
        assert!(Rc::ptr_eq(&find_win_by_nr_or_id(&num(1000)).unwrap(), &w0));
        // negative → None
        assert!(find_win_by_nr_or_id(&num(-3)).is_none());
    }

    #[test]
    fn win_findbuf_collects_matching_windows() {
        setup();
        let mut list = list_T::default();
        win_findbuf(&[num(9)], &mut list);
        assert_eq!(list.lv_len, 1);
        // the sole match is window 1001 (buffer 9)
        assert_eq!(super::tv_get_number(&list.lv_items[0].li_tv), 1001);
    }

    #[test]
    fn get_optional_window_default_and_lookup() {
        let (w0, w1) = setup();
        // absent arg → curwin
        assert!(Rc::ptr_eq(&get_optional_window(&[], 0).unwrap(), &w0));
        // explicit id → that window
        assert!(Rc::ptr_eq(
            &get_optional_window(&[num(1001)], 0).unwrap(),
            &w1
        ));
        // bad id → None
        assert!(get_optional_window(&[num(4242)], 0).is_none());
    }

    #[test]
    fn win_id2tabwin_returns_pair() {
        setup();
        let mut rettv = typval_T::default();
        win_id2tabwin(&[num(1001)], &mut rettv);
        // rettv is a 2-element list [tabnr, winnr] = [1, 2]
        if let crate::ported::eval::typval_defs_h::typval_vval_union::v_list(Some(l)) = &rettv.vval
        {
            let l = l.borrow();
            assert_eq!(l.lv_len, 2);
            assert_eq!(super::tv_get_number(&l.lv_items[0].li_tv), 1);
            assert_eq!(super::tv_get_number(&l.lv_items[1].li_tv), 2);
        } else {
            panic!("expected a VAR_LIST rettv");
        }
    }
}
