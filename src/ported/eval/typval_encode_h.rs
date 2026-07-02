//! Port of `src/nvim/eval/typval_encode.h` (vendored at
//! `csrc/eval/typval_encode.h`) — the `MPConvStack` conversion-state types shared
//! by the `typval_encode.c.h` template, plus the minimal `klib/kvec.h`
//! (vendored at `csrc/klib/kvec.h`) vector this header instantiates.
//!
//! RUST-PORT NOTE: vimlrs's `encode.rs` renders values by direct recursion, so
//! nothing constructs an `MPConvStack` at runtime — these types exist as a
//! faithful reference so `conv_error()` (`encode.c:113`), which walks a stack to
//! build an object-path error message, can be ported verbatim. The pointer
//! fields of `MPConvStackVal` (`hashitem_T *hi`, `listitem_T *li`,
//! `typval_T *arg`/`*argv`, `dict_T **dictp`) become indices / owned handles
//! because the port's `list_T`/`dict_T` are `Vec`/`IndexMap`-backed rather than
//! intrusive linked structures (see `typval_defs_h`).
#![allow(non_snake_case, non_camel_case_types, dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use crate::ported::eval::typval_defs_h::{dict_T, list_T, partial_T, typval_T};

/// `kvec_withinit_t(type, INIT_SIZE)` from `klib/kvec.h:150` (vendored at
/// `csrc/klib/kvec.h`).
///
/// RUST-PORT NOTE: the C small-vector-optimized kvec (an `INIT_SIZE`-slot inline
/// array that spills to a heap `items` pointer once it grows) collapses to a
/// plain `Vec<T>` here — identical push / index / size semantics, no manual
/// `capacity`/`size` bookkeeping.
#[derive(Debug)]
pub struct kvec_withinit_t<T> {
    /// `type *items` — the element storage (`.size`/`.capacity` are the `Vec`'s).
    pub items: Vec<T>,
}

// A hand-written `Default` (the `derive` would spuriously require `T: Default`).
impl<T> Default for kvec_withinit_t<T> {
    fn default() -> Self {
        kvec_withinit_t { items: Vec::new() }
    }
}

/// Port of the `kv_size(v)` macro from `klib/kvec.h:68` — number of elements.
pub fn kv_size<T>(v: &kvec_withinit_t<T>) -> usize {
    v.items.len()
}

/// Port of the `kv_A(v, i)` macro from `klib/kvec.h:66` — element access.
pub fn kv_A<T>(v: &kvec_withinit_t<T>, i: usize) -> &T {
    &v.items[i]
}

/// Port of the `kvi_push(v, x)` macro from `klib/kvec.h:253` — append an element.
pub fn kvi_push<T>(v: &mut kvec_withinit_t<T>, x: T) {
    v.items.push(x);
}

/// Port of `MPConvStackValType` from `typval_encode.h:18` — type of a stack
/// entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MPConvStackValType {
    /// Convert `dict_T *dictionary`.
    kMPConvDict,
    /// Convert `list_T *list`.
    kMPConvList,
    /// Convert mapping represented as a `list_T*` of pairs.
    kMPConvPairs,
    /// Convert `partial_T* partial`.
    kMPConvPartial,
    /// Convert argc/argv pair coming from a partial.
    kMPConvPartialList,
}

/// Port of `MPConvPartialStage` from `typval_encode.h:27` — stage at which a
/// partial is being converted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MPConvPartialStage {
    /// About to convert arguments.
    kMPConvPartialArgs,
    /// About to convert self dictionary.
    kMPConvPartialSelf,
    /// Already converted everything.
    kMPConvPartialEnd,
}

/// Port of the `union data` of `MPConvStackVal` (`typval_encode.h:38`).
///
/// RUST-PORT NOTE: the C `union` (a single overlaid storage read according to
/// the sibling `type` tag) becomes a Rust `enum` whose variants keep the C union
/// member names (`d`/`l`/`p`/`a`) and sub-field names verbatim.
#[derive(Debug)]
pub enum MPConvStackValData {
    /// State of dictionary conversion (`kMPConvDict`).
    d {
        /// `dict_T *dict` — currently converted dictionary.
        dict: Rc<RefCell<dict_T>>,
        /// `dict_T **dictp` — location where that dictionary is stored. RUST-PORT
        /// NOTE: the C double-indirection (`&tv->vval.v_dict`, deref'd back to
        /// `dict` by the converter) collapses to the pointed-to dict handle here;
        /// only the not-ported converter reads it.
        dictp: Rc<RefCell<dict_T>>,
        /// `hashitem_T *hi` — currently converted dictionary item. RUST-PORT
        /// NOTE: an index of the *next* entry into the ordered `dv_hashtab`
        /// (`None` == the C `NULL`, i.e. not yet advanced).
        hi: Option<usize>,
        /// `size_t todo` — amount of items left to process.
        todo: usize,
    },
    /// State of list or generic mapping conversion (`kMPConvList`/`kMPConvPairs`).
    l {
        /// `list_T *list` — currently converted list.
        list: Rc<RefCell<list_T>>,
        /// `listitem_T *li` — currently converted list item. RUST-PORT NOTE: an
        /// index of the *next* item into `lv_items` (`None` == the C `NULL`,
        /// i.e. past the end).
        li: Option<usize>,
    },
    /// State of partial conversion (`kMPConvPartial`).
    p {
        /// `MPConvPartialStage stage` — stage at which the partial is converted.
        stage: MPConvPartialStage,
        /// `partial_T *pt` — currently converted partial.
        pt: Option<Rc<partial_T>>,
    },
    /// State of a partial's argument-list conversion (`kMPConvPartialList`).
    a {
        /// `typval_T *arg` — currently converted argument. RUST-PORT NOTE: the
        /// current index into `argv` (the C pointer difference `arg - argv`).
        arg: usize,
        /// `typval_T *argv` — start of the argument list.
        argv: Vec<typval_T>,
        /// `size_t todo` — number of items left to process.
        todo: usize,
    },
}

/// Port of `MPConvStackVal` from `typval_encode.h:34` — one entry of the
/// Vimscript-to-messagepack conversion stack.
#[derive(Debug)]
pub struct MPConvStackVal {
    /// `MPConvStackValType type` — type of the stack entry.
    pub r#type: MPConvStackValType,
    /// `typval_T *tv` — currently converted `typval_T` (`None` == the C `NULL`,
    /// as for a `kMPConvPartialList` entry).
    pub tv: Option<typval_T>,
    /// `int saved_copyID` — copyID item used to have.
    pub saved_copyID: i32,
    /// `union { … } data` — data to convert.
    pub data: MPConvStackValData,
}

/// Port of `MPConvStack` from `typval_encode.h:64` — the stack used to convert
/// Vimscript values to messagepack (`kvec_withinit_t(MPConvStackVal, 8)`).
pub type MPConvStack = kvec_withinit_t<MPConvStackVal>;
