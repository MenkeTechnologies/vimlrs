//! Vimscript List — port of `eval/list.c` and `list_T` (eval/typval_defs.h).
//!
//! Port status: **type scaffold only.** C `list_T` is a refcounted intrusive
//! doubly-linked list (`listitem_T` nodes, `lv_refcount`, `lv_watch`). The ~50
//! `tv_list_*` operations in `eval/list.c` are not yet ported; this defines the
//! handle so `Typval::List` type-checks and the port report can track the gap.

use std::cell::RefCell;
use std::rc::Rc;

use crate::typval::Typval;

/// Port of `list_T` (eval/typval_defs.h) — list value.
///
/// The faithful representation is an intrusive linked list with item watchers.
/// This placeholder stores items in a `Vec` until `eval/list.c` is ported.
#[derive(Debug, Default)]
pub struct List {
    pub items: Vec<Typval>,
}

/// Shared list reference — models C `list_T *` plus `lv_refcount`.
pub type ListRef = Rc<RefCell<List>>;

/// Port of `tv_list_equal()` (eval/typval.c). Not yet ported.
pub fn tv_list_equal(_l1: &Option<ListRef>, _l2: &Option<ListRef>, _ic: bool) -> bool {
    unimplemented!("tv_list_equal: port pending — see csrc/eval/typval.c")
}
