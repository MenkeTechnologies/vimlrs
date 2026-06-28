//! Port of `src/nvim/eval/typval_defs.h` (vendored at `csrc/eval/typval_defs.h`).
//!
//! Header-defined Vimscript value types. Names, field names, and enum members
//! match the C source exactly (PORT.md Rule A / Rule C — header types live in
//! the header port). C lower-case / mixed-case type names are kept verbatim, so
//! the usual Rust casing lints are disabled for this file.
//!
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! RUST-PORT NOTE (not from C): C models lists/dicts/blobs as intrusive,
//! reference-counted heap objects reached through raw pointers
//! (`list_T *`, `lv_first`/`lv_last` item chain). The single-threaded eval
//! engine's observable semantics are reproduced with `Rc<RefCell<…>>` handles
//! (the `Rc` IS the refcount the `is`/`isnot` operators compare) and a `Vec`
//! item store in place of the `li_next`/`li_prev` chain. The C `lv_refcount` /
//! `dv_refcount` / `bv_refcount` fields are retained for fidelity even though
//! the `Rc` does the counting.
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
#![allow(non_camel_case_types, non_upper_case_globals)]

use std::cell::RefCell;
use std::rc::Rc;

/// `typedef int64_t varnumber_T;` — type used for VAR_NUMBER values. (c:14)
pub type varnumber_T = i64;

/// `typedef double float_T;` (`src/nvim/types_defs.h`); the eval engine's
/// VAR_FLOAT storage type.
pub type float_T = f64;

/// `VARNUMBER_MAX` — maximal `varnumber_T`. (c:42)
pub const VARNUMBER_MAX: varnumber_T = i64::MAX;
/// `VARNUMBER_MIN` — minimal `varnumber_T`. (c:46)
pub const VARNUMBER_MIN: varnumber_T = i64::MIN;

/// Bool variable values. (c:88 `typedef enum { kBoolVarFalse, kBoolVarTrue }`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoolVarValue {
    /// `v:false`.
    kBoolVarFalse,
    /// `v:true`.
    kBoolVarTrue,
}

/// Special variable values. (c:94 `typedef enum { kSpecialVarNull }`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialVarValue {
    /// `v:null`.
    kSpecialVarNull,
    /// `v:none` (an "absent" value, distinct from `v:null` only in rendering).
    kSpecialVarNone,
}

/// Variable lock status for `typval_T.v_lock`. (c:99)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VarLockStatus {
    /// Not locked.
    #[default]
    VAR_UNLOCKED,
    /// User lock, can be unlocked.
    VAR_LOCKED,
    /// Locked forever.
    VAR_FIXED,
}

/// Vimscript variable types, for `typval_T.v_type`. (c:106)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarType {
    /// Unknown (unspecified) value.
    VAR_UNKNOWN,
    /// Number, `.v_number` is used.
    VAR_NUMBER,
    /// String, `.v_string` is used.
    VAR_STRING,
    /// Function reference, `.v_string` is used as function name.
    VAR_FUNC,
    /// List, `.v_list` is used.
    VAR_LIST,
    /// Dict, `.v_dict` is used.
    VAR_DICT,
    /// Floating-point value, `.v_float` is used.
    VAR_FLOAT,
    /// true, false.
    VAR_BOOL,
    /// Special value (null), `.v_special` is used.
    VAR_SPECIAL,
    /// Partial, `.v_partial` is used.
    VAR_PARTIAL,
    /// Blob, `.v_blob` is used.
    VAR_BLOB,
}

// Type values for type(). (c:121)
/// `VAR_TYPE_NUMBER`. (c:122)
pub const VAR_TYPE_NUMBER: varnumber_T = 0;
/// `VAR_TYPE_STRING`. (c:123)
pub const VAR_TYPE_STRING: varnumber_T = 1;
/// `VAR_TYPE_FUNC`. (c:124)
pub const VAR_TYPE_FUNC: varnumber_T = 2;
/// `VAR_TYPE_LIST`. (c:125)
pub const VAR_TYPE_LIST: varnumber_T = 3;
/// `VAR_TYPE_DICT`. (c:126)
pub const VAR_TYPE_DICT: varnumber_T = 4;
/// `VAR_TYPE_FLOAT`. (c:127)
pub const VAR_TYPE_FLOAT: varnumber_T = 5;
/// `VAR_TYPE_BOOL`. (c:128)
pub const VAR_TYPE_BOOL: varnumber_T = 6;
/// `VAR_TYPE_SPECIAL`. (c:129)
pub const VAR_TYPE_SPECIAL: varnumber_T = 7;
/// `VAR_TYPE_BLOB`. (c:130)
pub const VAR_TYPE_BLOB: varnumber_T = 10;

/// `union typval_vval_union` — the active value member selected by
/// `typval_T.v_type`. (c:137) Member names match the C union fields; a NULL C
/// pointer (`v_string`/`v_list`/… may be NULL) is `None`/empty here.
#[derive(Debug, Clone)]
pub enum typval_vval_union {
    /// `varnumber_T v_number` — for VAR_NUMBER.
    v_number(varnumber_T),
    /// `BoolVarValue v_bool` — for VAR_BOOL.
    v_bool(BoolVarValue),
    /// `SpecialVarValue v_special` — for VAR_SPECIAL.
    v_special(SpecialVarValue),
    /// `float_T v_float` — for VAR_FLOAT.
    v_float(float_T),
    /// `char *v_string` — for VAR_STRING and VAR_FUNC (can be NULL).
    v_string(String),
    /// `list_T *v_list` — for VAR_LIST (can be NULL).
    v_list(Option<Rc<RefCell<list_T>>>),
    /// `dict_T *v_dict` — for VAR_DICT (can be NULL).
    v_dict(Option<Rc<RefCell<dict_T>>>),
    /// `blob_T *v_blob` — for VAR_BLOB (can be NULL).
    v_blob(Option<Rc<RefCell<blob_T>>>),
    /// `partial_T *v_partial` — for VAR_PARTIAL (can be NULL).
    v_partial(Option<Rc<partial_T>>),
    /// Placeholder active member for VAR_UNKNOWN (`TV_INITIAL_VALUE`).
    v_unknown,
}

/// `struct partial_S { … } partial_T` — a Funcref with bound arguments and/or a
/// `self` dict (`function(name, [args])` / `function(name, dict)`). `pt_func`
/// (the resolved `ufunc_T`) is not modeled — `pt_name` is used to look the
/// function up at call time.
#[derive(Debug)]
pub struct partial_T {
    /// `int pt_refcount` — reference count (vestigial; `Rc`-managed).
    pub pt_refcount: i32,
    /// `char *pt_name` — the function name.
    pub pt_name: String,
    /// `typval_T *pt_argv` (with `pt_argc`) — the bound leading arguments.
    pub pt_argv: Vec<typval_T>,
    /// `dict_T *pt_dict` — the `self` dict, if bound.
    pub pt_dict: Option<Rc<RefCell<dict_T>>>,
}

/// `typedef struct { VarType v_type; VarLockStatus v_lock; union … vval; }
/// typval_T;` — a single Vimscript value. (c:134)
#[derive(Debug, Clone)]
pub struct typval_T {
    /// Variable type.
    pub v_type: VarType,
    /// Variable lock status.
    pub v_lock: VarLockStatus,
    /// Actual value (active member selected by `v_type`).
    pub vval: typval_vval_union,
}

impl Default for typval_T {
    /// `TV_INITIAL_VALUE` (c:150): `{ .v_type = VAR_UNKNOWN, .v_lock =
    /// VAR_UNLOCKED }`.
    fn default() -> Self {
        typval_T {
            v_type: VarType::VAR_UNKNOWN,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: typval_vval_union::v_unknown,
        }
    }
}

impl From<varnumber_T> for typval_T {
    /// A `VAR_NUMBER` value (the C `rettv->v_type = VAR_NUMBER; rettv->vval
    /// .v_number = n;` pattern, as a Rust constructor).
    fn from(n: varnumber_T) -> Self {
        typval_T {
            v_type: VarType::VAR_NUMBER,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: typval_vval_union::v_number(n),
        }
    }
}

impl From<String> for typval_T {
    /// A `VAR_STRING` value.
    fn from(s: String) -> Self {
        typval_T {
            v_type: VarType::VAR_STRING,
            v_lock: VarLockStatus::VAR_UNLOCKED,
            vval: typval_vval_union::v_string(s),
        }
    }
}

/// `struct listitem_S { listitem_T *li_next; listitem_T *li_prev; typval_T
/// li_tv; }` — an item of a list. (c:167) The `li_next`/`li_prev` chain is
/// replaced by the owning `Vec` (see file-header RUST-PORT NOTE); `li_tv` is
/// kept verbatim.
#[derive(Debug, Clone)]
pub struct listitem_T {
    /// Item value.
    pub li_tv: typval_T,
}

/// `struct listvar_S { … }` — info about a list. (c:183)
#[derive(Debug, Default)]
pub struct list_T {
    /// Items (`lv_first`…`lv_last` chain, stored as a `Vec` per the port note).
    pub lv_items: Vec<listitem_T>,
    /// `int lv_len` — number of items. (c:192)
    pub lv_len: i32,
    /// `int lv_refcount` — reference count. (c:191)
    pub lv_refcount: i32,
    /// `VarLockStatus lv_lock`. (c:195)
    pub lv_lock: VarLockStatus,
}

/// `struct dictvar_S { … }` — a Dictionary. (c:252)
///
/// The `dv_hashtab` of `dictitem_T` is stored as an insertion-ordered map
/// (Vim's hashtab iteration order is unspecified; insertion order is what
/// users observe and is deterministic for `string()`/`:echo`).
#[derive(Debug, Default)]
pub struct dict_T {
    /// `hashtab_T dv_hashtab` contents: key → value. (c:258)
    pub dv_hashtab: indexmap::IndexMap<String, typval_T>,
    /// `int dv_refcount` — reference count. (c:256)
    pub dv_refcount: i32,
    /// `VarLockStatus dv_lock` — whole-dict lock. (c:253)
    pub dv_lock: VarLockStatus,
    /// `QUEUE watchers` — registered `dictwatcheradd()` watchers. (c:259)
    pub dv_watchers: Vec<crate::ported::eval::typval::DictWatcher>,
}

/// `struct blobvar_S { garray_T bv_ga; int bv_refcount; VarLockStatus bv_lock;
/// }` — a Blob. (c:268)
#[derive(Debug, Default)]
pub struct blob_T {
    /// `garray_T bv_ga` — the byte data. (c:269)
    pub bv_ga: Vec<u8>,
    /// `int bv_refcount` — reference count. (c:270)
    pub bv_refcount: i32,
    /// `VarLockStatus bv_lock`. (c:271)
    pub bv_lock: VarLockStatus,
}
