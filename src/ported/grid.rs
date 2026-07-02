//! Port of `src/nvim/grid_defs.h` (`ScreenGrid`) and the compositor query
//! `ui_comp_get_grid_at_coord()` from `src/nvim/ui_compositor.c` (both vendored
//! at `csrc/grid_defs.h` / `csrc/ui_compositor.c`).
//!
//! `screenchar()`/`screenattr()`/`screenchars()`/`screenstring()` read the
//! composed screen through `screenchar_adjust()`, which asks the compositor for
//! the grid on top at a screen coordinate. Only the two symbols the eval tree
//! reaches are ported here: the `ScreenGrid` record and the coordinate lookup.
//!
//! RUST-PORT NOTE: vimlrs has no live UI compositor. The compositor owns a
//! stack of layer grids plus `default_grid`, none of which exist standalone, so
//! `ui_comp_get_grid_at_coord()` has no grids to search and returns `None` (a
//! null grid). `screenchar_adjust()` (eval/funcs.rs) then yields the "no screen"
//! result (-1 / empty) for the `screen*()` builtins. The intrusive
//! `schar_T*`/`sattr_T*`/`size_t*` pointer arrays become owning `Vec`s.
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use crate::ported::window::{colnr_T, handle_T};

/// `typedef uint32_t schar_T` (`Src/types_defs.h:12`) — a composed screen cell.
pub type schar_T = u32;
/// `typedef int32_t sattr_T` (`Src/types_defs.h:13`) — a cell's highlight attr.
pub type sattr_T = i32;

/// Port of `struct ScreenGrid` from `Src/grid_defs.h:45`.
///
/// A rectangular block of the screen (the default grid, a window grid, or a
/// float). `screenchar_adjust()` reads `comp_row`/`comp_col` (grid origin) and
/// `rows`/`cols`; the `screen*()` builtins read `chars`/`attrs`/`line_offset`.
///
/// RUST-PORT NOTE: the C `schar_T *chars` / `sattr_T *attrs` / `colnr_T *vcols`
/// / `size_t *line_offset` / `int *dirty_col` pointer arrays are modelled as
/// owning `Vec`s. Since vimlrs never allocates a grid (see the module note), all
/// arrays stay empty; the fields exist for faithful field-name parity.
#[derive(Default)]
pub struct ScreenGrid {
    /// `handle_T handle` — the grid's UI handle. (c:46)
    pub handle: handle_T,

    /// `schar_T *chars` — composed characters. (c:48)
    pub chars: Vec<schar_T>,
    /// `sattr_T *attrs` — cell highlight attributes. (c:49)
    pub attrs: Vec<sattr_T>,
    /// `colnr_T *vcols` — virtual column of each cell. (c:50)
    pub vcols: Vec<colnr_T>,
    /// `size_t *line_offset` — start index of each row in `chars`/`attrs`. (c:51)
    pub line_offset: Vec<usize>,

    // c:55 last column that was drawn (only used when "throttled" is set).
    /// `int *dirty_col`. (c:55)
    pub dirty_col: Vec<i32>,

    // c:57 the size of the allocated grid.
    /// `int rows`. (c:58)
    pub rows: i32,
    /// `int cols`. (c:59)
    pub cols: i32,

    /// `bool valid` — grid state is valid, else needs redraw. (c:62)
    pub valid: bool,

    /// `bool throttled` — draw internally, don't send updates yet. (c:66)
    pub throttled: bool,

    /// `bool blending` — compositor blends the grid with the background. (c:69)
    pub blending: bool,

    /// `bool mouse_enabled` — the grid interacts with mouse events. (c:72)
    pub mouse_enabled: bool,

    /// `int zindex` — order in the stack of grids. (c:75)
    pub zindex: i32,

    // c:77 Below is state owned by the compositor.
    /// `int comp_row` — grid origin row on the composed screen. (c:81)
    pub comp_row: i32,
    /// `int comp_col` — grid origin column on the composed screen. (c:82)
    pub comp_col: i32,

    // c:84 Requested width/height upon resize.
    /// `int comp_width`. (c:87)
    pub comp_width: i32,
    /// `int comp_height`. (c:88)
    pub comp_height: i32,

    /// `size_t comp_index` — z-index within the compositor. (c:92)
    pub comp_index: usize,

    /// `bool comp_disabled` — compositor momentarily ignores the grid. (c:95)
    pub comp_disabled: bool,

    /// `bool pending_comp_index_update`. (c:98)
    pub pending_comp_index_update: bool,
}

/// Port of `ui_comp_get_grid_at_coord()` from `Src/ui_compositor.c:335`.
///
/// Compute which grid is on top at supplied screen coordinates. C walks the
/// compositor layer stack (top-down), then the windows of the current tab, and
/// finally falls back to `&default_grid`.
///
/// RUST-PORT NOTE: vimlrs has no compositor layer stack (`layers`), no window
/// grids, and no `default_grid`, so there is nothing to search — the faithful
/// result is "no grid on top", returned as `None`. This is the deviation that
/// makes `screenchar()`/`screenattr()`/`screenchars()`/`screenstring()` report
/// the off-screen result. The C loop structure is preserved as commentary.
pub fn ui_comp_get_grid_at_coord(_row: i32, _col: i32) -> Option<ScreenGrid> {
    // c:337 for (i = kv_size(layers) - 1; i > 0; i--) { grid = kv_A(layers, i);
    // c:339   if (row >= grid->comp_row && row < grid->comp_row + grid->rows
    // c:340       && col >= grid->comp_col && col < grid->comp_col + grid->cols)
    // c:341     return grid; }
    //   → no `layers` stack in vimlrs.
    // c:345 FOR_ALL_WINDOWS_IN_TAB(wp, curtab) { grid = &wp->w_grid_alloc; ... }
    //   → no window grids in vimlrs.
    // c:353 return &default_grid;  → no default_grid; the null result is None.
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_compositor_yields_no_grid() {
        // With no compositor/default grid, every coordinate is off-screen.
        assert!(ui_comp_get_grid_at_coord(0, 0).is_none());
        assert!(ui_comp_get_grid_at_coord(100, 100).is_none());
    }
}
