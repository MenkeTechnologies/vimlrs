//! Port of `src/nvim/window.c` (vendored subset at `csrc/window.c`) — only the
//! window / tab-page list model the eval window layer reaches: the `win_T` and
//! `tabpage_T` structs (fields eval reads), the global window/tab lists, and the
//! two lookup helpers `find_tabpage()` and `win_get_tabwin()`.
//!
//! RUST-PORT NOTE: Neovim's `win_T`/`tabpage_T` are heap objects wired into
//! intrusive doubly-linked lists via raw pointers (`w_next`/`w_prev`,
//! `tp_next`, plus the file-static roots `firstwin`/`lastwin`/`curwin`/
//! `first_tabpage`/`curtab`). Rust cannot express intrusive raw-pointer chains
//! safely, so the links become `Rc<RefCell<…>>` (forward, owning) and
//! `Weak<RefCell<…>>` (`w_prev`, back-link), and the file-static roots become
//! `thread_local!` cells. Pointer identity (`wp == curwin`, `tp == curtab`)
//! becomes [`Rc::ptr_eq`]. The deep window-management operations of `window.c`
//! (splits, frames, resize, layout) are not reachable from eval and are not
//! modelled here.
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use std::cell::RefCell;
use std::rc::{Rc, Weak};

// --- primitive C types (types_defs.h / pos_defs.h) --------------------------

/// `typedef int handle_T;` (types_defs.h:22) — opaque object id.
pub type handle_T = i32;
/// `typedef int32_t linenr_T;` (pos_defs.h) — line number type.
pub type linenr_T = i32;
/// `typedef int colnr_T;` (pos_defs.h) — column number type.
pub type colnr_T = i32;

/// `typedef struct { linenr_T lnum; colnr_T col; colnr_T coladd; } pos_T;`
/// (pos_defs.h:25) — position in file or buffer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct pos_T {
    /// line number
    pub lnum: linenr_T,
    /// column number
    pub col: colnr_T,
    pub coladd: colnr_T,
}

/// Subset of `WinConfig` (buffer_defs.h:1029) — only the two fields
/// [`win_has_winnr`](crate::ported::eval::window::win_has_winnr) reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct WinConfig {
    pub focusable: bool,
    pub hide: bool,
}

/// RUST-PORT NOTE: placeholder for the buffer agent's `buf_T`. The eval window
/// layer only reads `w_buffer->b_fnum` (in `win_findbuf`), so this stands in
/// with just that field until the central pass reconciles it with the real
/// `buf_T` from `buffer_defs.h`. `w_buffer` is modelled as an
/// `Option<Rc<RefCell<buf_T>>>` handle per the coordination note.
#[derive(Debug, Clone, Copy, Default)]
pub struct buf_T {
    /// `handle_T handle` — buffer number (`b_fnum` aliases `handle`).
    pub b_fnum: handle_T,
}

/// Port of `struct window_S` (`win_T`) from `csrc/buffer_defs.h:1102`.
/// Only the fields the eval layer reads are modelled (see module note).
#[derive(Default)]
pub struct win_T {
    /// `handle_T handle` — unique identifier for the window (the window id).
    pub handle: handle_T,
    /// `buf_T *w_buffer` — buffer this window is into.
    pub w_buffer: Option<Rc<RefCell<buf_T>>>,
    /// `win_T *w_prev` — link to previous window.
    pub w_prev: Option<Weak<RefCell<win_T>>>,
    /// `win_T *w_next` — link to next window.
    pub w_next: Option<Rc<RefCell<win_T>>>,
    /// `pos_T w_cursor` — cursor position in buffer.
    pub w_cursor: pos_T,
    /// `bool w_p_pvw` — 'previewwindow' (`w_onebuf_opt.wo_pvw`).
    pub w_p_pvw: bool,
    /// `bool w_floating` — whether the window is floating.
    pub w_floating: bool,
    /// `WinConfig w_config` — window configuration (float focus/hide).
    pub w_config: WinConfig,
}

/// Port of `struct tabpage_S` (`tabpage_T`) from `csrc/buffer_defs.h:840`.
/// Only the fields the eval layer reads are modelled (see module note).
#[derive(Default)]
pub struct tabpage_T {
    /// `handle_T handle` — unique identifier for the tab page.
    pub handle: handle_T,
    /// `tabpage_T *tp_next` — next tabpage or NULL.
    pub tp_next: Option<Rc<RefCell<tabpage_T>>>,
    /// `win_T *tp_curwin` — current window in this Tab page.
    pub tp_curwin: Option<Rc<RefCell<win_T>>>,
    /// `win_T *tp_prevwin` — previous window in this Tab page.
    pub tp_prevwin: Option<Rc<RefCell<win_T>>>,
    /// `win_T *tp_firstwin` — first window in this Tab page.
    pub tp_firstwin: Option<Rc<RefCell<win_T>>>,
    /// `win_T *tp_lastwin` — last window in this Tab page.
    pub tp_lastwin: Option<Rc<RefCell<win_T>>>,
}

// --- global window / tab lists (globals.h:355-390) --------------------------
//
// RUST-PORT NOTE: the C file-static roots become `thread_local!` cells holding
// the head/tail/current handles. `firstwin`/`lastwin`/`curwin` are the window
// list; `first_tabpage`/`curtab` are the tab-page list.

thread_local! {
    /// `EXTERN win_T *firstwin;` (globals.h:358) — first window.
    pub static firstwin: RefCell<Option<Rc<RefCell<win_T>>>> = const { RefCell::new(None) };
    /// `EXTERN win_T *lastwin;` (globals.h:359) — last window.
    pub static lastwin: RefCell<Option<Rc<RefCell<win_T>>>> = const { RefCell::new(None) };
    /// `EXTERN win_T *curwin;` (globals.h:375) — currently active window.
    pub static curwin: RefCell<Option<Rc<RefCell<win_T>>>> = const { RefCell::new(None) };
    /// `EXTERN tabpage_T *first_tabpage;` (globals.h:384) — first tab page.
    pub static first_tabpage: RefCell<Option<Rc<RefCell<tabpage_T>>>> = const { RefCell::new(None) };
    /// `EXTERN tabpage_T *curtab;` (globals.h:385) — current tab page.
    pub static curtab: RefCell<Option<Rc<RefCell<tabpage_T>>>> = const { RefCell::new(None) };
}

/// Port of `find_tabpage()` from `csrc/window.c:34`.
/// Find tab page "n" (first one is 1).  Returns NULL when not found.
pub fn find_tabpage(n: i32) -> Option<Rc<RefCell<tabpage_T>>> {
    let mut i = 1; // c:37

    if n == 0 {
        // c:39
        return curtab.with(|c| c.borrow().clone()); // c:40 return curtab
    }

    // c:43 for (tp = first_tabpage; tp != NULL && i != n; tp = tp->tp_next) { i++; }
    let mut tp = first_tabpage.with(|c| c.borrow().clone());
    while let Some(cur) = tp.clone() {
        if i == n {
            break;
        }
        i += 1; // c:44
        tp = cur.borrow().tp_next.clone();
    }
    tp // c:46
}

/// Port of `win_get_tabwin()` from `csrc/window.c:49`.
/// Set `*tabnr`/`*winnr` to the tab-page/window numbers of window id `id`.
///
/// RUST-PORT NOTE: the C out-parameters `int *tabnr`/`int *winnr` become
/// `&mut i32` (the C→Rust out-param map).
pub fn win_get_tabwin(id: handle_T, tabnr: &mut i32, winnr: &mut i32) {
    *tabnr = 0; // c:51
    *winnr = 0; // c:52

    let mut tnum = 1; // c:54
    let mut wnum = 1; // c:55
    // c:56 FOR_ALL_TABS(tp)
    let mut tp = first_tabpage.with(|c| c.borrow().clone());
    while let Some(tp_rc) = tp.clone() {
        // c:57 FOR_ALL_WINDOWS_IN_TAB(wp, tp): head is firstwin for curtab else tp_firstwin
        let is_curtab = curtab.with(|c| c.borrow().as_ref().is_some_and(|ct| Rc::ptr_eq(ct, &tp_rc)));
        let mut wp = if is_curtab {
            firstwin.with(|c| c.borrow().clone())
        } else {
            tp_rc.borrow().tp_firstwin.clone()
        };
        while let Some(wp_rc) = wp.clone() {
            if wp_rc.borrow().handle == id {
                // c:58
                if crate::ported::eval::window::win_has_winnr(&wp_rc, &tp_rc) {
                    // c:59
                    *winnr = wnum; // c:60
                    *tabnr = tnum; // c:61
                }
                return; // c:63
            }
            wnum += crate::ported::eval::window::win_has_winnr(&wp_rc, &tp_rc) as i32; // c:65
            wp = wp_rc.borrow().w_next.clone();
        }
        tnum += 1; // c:67
        wnum = 1; // c:68
        tp = tp_rc.borrow().tp_next.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-tab, two-window layout: firstwin(handle 1000) -> win(handle 1001).
    /// Returns (tab, w0, w1).
    fn build_two_windows() -> (Rc<RefCell<tabpage_T>>, Rc<RefCell<win_T>>, Rc<RefCell<win_T>>) {
        let w0 = Rc::new(RefCell::new(win_T {
            handle: 1000,
            w_config: WinConfig { focusable: true, hide: false },
            ..Default::default()
        }));
        let w1 = Rc::new(RefCell::new(win_T {
            handle: 1001,
            w_config: WinConfig { focusable: true, hide: false },
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
        lastwin.with(|c| *c.borrow_mut() = Some(w1.clone()));
        curwin.with(|c| *c.borrow_mut() = Some(w0.clone()));
        first_tabpage.with(|c| *c.borrow_mut() = Some(tab.clone()));
        curtab.with(|c| *c.borrow_mut() = Some(tab.clone()));
        (tab, w0, w1)
    }

    #[test]
    fn find_tabpage_number_and_zero() {
        let (tab, _w0, _w1) = build_two_windows();
        // n == 0 → curtab
        let cur = find_tabpage(0).unwrap();
        assert!(Rc::ptr_eq(&cur, &tab));
        // n == 1 → first (and only) tab
        let first = find_tabpage(1).unwrap();
        assert!(Rc::ptr_eq(&first, &tab));
        // out of range → None
        assert!(find_tabpage(2).is_none());
    }

    #[test]
    fn win_get_tabwin_finds_and_misses() {
        let (_tab, _w0, _w1) = build_two_windows();
        let mut tabnr = -1;
        let mut winnr = -1;
        win_get_tabwin(1001, &mut tabnr, &mut winnr);
        assert_eq!((tabnr, winnr), (1, 2));

        // unknown id → both stay 0
        let mut tabnr2 = -1;
        let mut winnr2 = -1;
        win_get_tabwin(9999, &mut tabnr2, &mut winnr2);
        assert_eq!((tabnr2, winnr2), (0, 0));
    }
}
