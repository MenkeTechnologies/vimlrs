//! Port of `src/nvim/eval/window.c` (vendored at `csrc/eval/window.c`).
//!
//! The window-lookup helper layer behind the `win_*`/`tabpage*` builtins. A
//! standalone interpreter has no windows or tab pages, so every window lookup
//! finds nothing: a `win_T *` result is `None`, a window number / id is 0, a
//! "has winnr" / switch is false / FAIL, and the void state mutators are no-ops.
//! (RUST-PORT NOTE: the C `win_T *`/`tabpage_T *`/`switchwin_T *` pointers have
//! no standalone model; the signatures collapse to the lookup result.)
#![allow(non_snake_case)]

use crate::ported::eval::typval_defs_h::typval_T;
use crate::ported::eval_h::FAIL;

/// Port of `win_has_winnr()` — whether a window has a window number; no windows → false.
pub fn win_has_winnr() -> bool {
    false
}
/// Port of `win_getid()` — the id of a looked-up window; none → 0.
pub fn win_getid(_argvars: &[typval_T]) -> i32 {
    0
}
/// Port of `win_id2win()` — the window number for an id; not found → 0.
pub fn win_id2win(_argvars: &[typval_T]) -> i32 {
    0
}
/// Port of `win_id2wp()` — the window for an id; none → `None`.
pub fn win_id2wp(_id: i32) -> Option<()> {
    None
}
/// Port of `win_id2wp_tp()` — the window (and its tab) for an id; none → `None`.
pub fn win_id2wp_tp(_id: i32) -> Option<()> {
    None
}
/// Port of `find_win_by_nr()` — the window with a given number; none → `None`.
pub fn find_win_by_nr(_vp: &typval_T) -> Option<()> {
    None
}
/// Port of `find_win_by_nr_or_id()` — window by number or id; none → `None`.
pub fn find_win_by_nr_or_id(_vp: &typval_T) -> Option<()> {
    None
}
/// Port of `find_tabwin()` — window from a {win}/{tab} pair; none → `None`.
pub fn find_tabwin(_wvp: &typval_T, _tvp: &typval_T) -> Option<()> {
    None
}
/// Port of `get_winnr()` — the number of a window in a tab page; none → 0.
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
