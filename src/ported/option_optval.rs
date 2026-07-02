//! Port of `src/nvim/option.c`'s **OptVal-typed** get/set layer (subset), the
//! machinery the Vimscript eval engine reaches through: `&opt` reads
//! (`eval_option` → `get_option_value` → `optval_as_tv`), `:let &opt = expr`
//! and `setbufvar(b, '&opt', v)` writes (`set_option_from_tv` → `tv_to_optval`
//! → `set_option_value_handle_tty`), and option-name resolution
//! (`find_option`, `find_option_var_end`).
//!
//! Vendored spec: `csrc/option.c` (a curated subset of upstream `option.c` +
//! `option_defs.h` + `option.h` + `types_defs.h`). `tv_to_optval`,
//! `optval_as_tv`, `set_option_from_tv` live upstream in `eval/vars.c`
//! (`csrc/eval/vars.c`) and `find_option_var_end` in `eval.c` (`csrc/eval.c`);
//! they are ported here because they are the OptVal↔typval boundary.
//!
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! RUST-PORT NOTE (relationship to the sibling `option.rs`): the reduced
//! `option.rs` ports the *untyped* `do_set` `:set` argument-string grammar plus
//! the host-editor mirror hook, storing every value as a `typval_T`. THIS module
//! ports the *OptVal-typed* layer that eval's expression paths use — the same
//! logical option store, but modeled with Neovim's real `OptVal` / `OptValType`
//! / `OptValData` value type and the `find_option` / `optval_from_varp` /
//! `set_option_value` call chain, so `&opt` reads and `:let &opt` writes go
//! through the faithful C control flow rather than string parsing. The two are
//! kept as separate value stores (each thread-local); the editor integration
//! wave unifies them onto the buffer/window option variables.
//!
//! RUST-PORT NOTE (reductions vs upstream):
//!   * `OptIndex` is upstream a generated enum (`kOpt<Name>`) indexing the
//!     `options[]` array built from `options.lua`; here it is a `usize` index
//!     into the reduced `options` table with `kOptInvalid = usize::MAX`.
//!   * `options[]` is the subset of number/string/boolean options eval reads
//!     (see the table below). `flags`/`scope_flags`/per-buffer-per-window scopes
//!     and the `did_set` side-effect callbacks (redraw, terminal, filetype
//!     autocommands) are NOT modeled — `set_option_value` validates then stores
//!     the value so `&opt` reads observe it, but applies no editor side effects.
//!   * The option value store — upstream the global option variables reached via
//!     `get_varp_scope` + a `void *varp` — is a thread-local `OptIndex → OptVal`
//!     map here; `optval_from_varp` reads it (its `varp` parameter is dropped)
//!     and `get_option_value` calls it directly (no `get_varp_scope`).
//!   * `find_option_hash` (the generated perfect hash) → a linear scan.
//!   * `optval_free` is a no-op (Rust drops the owned `String`); `set_option`
//!     (the validating setter with side effects) collapses into a store write.
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::LazyLock;

use crate::ported::eval::encode::encode_tv2string;
use crate::ported::eval::typval::{tv_get_bool_chk, tv_get_number_chk, tv_get_string_buf_chk};
use crate::ported::eval::typval_defs_h::{
    typval_T, typval_vval_union, varnumber_T, BoolVarValue, SpecialVarValue, VarLockStatus, VarType,
};
use crate::ported::message::{emsg, semsg};

// ── types_defs.h ─────────────────────────────────────────────────────────────

/// `typedef int64_t OptInt;` (`types_defs.h:57`).
pub type OptInt = i64;

/// `typedef enum { kNone = -1, kFalse = 0, kTrue = 1 } TriState;`
/// (`types_defs.h:46`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriState {
    /// `kNone = -1`.
    kNone = -1,
    /// `kFalse = 0`.
    kFalse = 0,
    /// `kTrue = 1`.
    kTrue = 1,
}

/// Port of `TRISTATE_FROM_INT(val)` (`types_defs.h:55`):
/// `((val) == 0 ? kFalse : ((val) >= 1 ? kTrue : kNone))`.
fn TRISTATE_FROM_INT(val: varnumber_T) -> TriState {
    // c:55
    if val == 0 {
        TriState::kFalse
    } else if val >= 1 {
        TriState::kTrue
    } else {
        TriState::kNone
    }
}

// ── option_defs.h ────────────────────────────────────────────────────────────

/// `typedef enum { kOptValTypeNil = -1, kOptValTypeBoolean, kOptValTypeNumber,
/// kOptValTypeString } OptValType;` (`option_defs.h:48`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptValType {
    /// `kOptValTypeNil = -1` — "no value" (invalid option / cleared value).
    kOptValTypeNil,
    /// `kOptValTypeBoolean` — a tri-state boolean option.
    kOptValTypeBoolean,
    /// `kOptValTypeNumber` — a numeric option.
    kOptValTypeNumber,
    /// `kOptValTypeString` — a string option.
    kOptValTypeString,
}

/// `typedef union { TriState boolean; OptInt number; String string; }
/// OptValData;` (`option_defs.h:66`) — the active member is selected by
/// `OptVal.type` (RUST-PORT NOTE: C union → tagged enum; the `kOptValTypeNil`
/// case carries no data, represented by `OptVal.data` being unused).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptValData {
    /// `TriState boolean` — for `kOptValTypeBoolean`.
    boolean(TriState),
    /// `OptInt number` — for `kOptValTypeNumber`.
    number(OptInt),
    /// `String string` — for `kOptValTypeString`.
    string(String),
    /// No active member (for `kOptValTypeNil`).
    nil,
}

/// `typedef struct { OptValType type; OptValData data; } OptVal;`
/// (`option_defs.h:74`). The C field `type` is a reserved word in Rust, so it
/// is `r#type`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptVal {
    /// `OptValType type` — value tag.
    pub r#type: OptValType,
    /// `OptValData data` — the value (active member per `type`).
    pub data: OptValData,
}

/// Port of `NIL_OPTVAL` (`option.h:53`):
/// `((OptVal) { .type = kOptValTypeNil })`.
fn NIL_OPTVAL() -> OptVal {
    // c:53
    OptVal {
        r#type: OptValType::kOptValTypeNil,
        data: OptValData::nil,
    }
}

/// Port of `BOOLEAN_OPTVAL(b)` (`option.h:54`):
/// `((OptVal) { .type = kOptValTypeBoolean, .data.boolean = b })`.
fn BOOLEAN_OPTVAL(b: TriState) -> OptVal {
    // c:54
    OptVal {
        r#type: OptValType::kOptValTypeBoolean,
        data: OptValData::boolean(b),
    }
}

/// Port of `NUMBER_OPTVAL(n)` (`option.h:55`):
/// `((OptVal) { .type = kOptValTypeNumber, .data.number = n })`.
fn NUMBER_OPTVAL(n: OptInt) -> OptVal {
    // c:55
    OptVal {
        r#type: OptValType::kOptValTypeNumber,
        data: OptValData::number(n),
    }
}

/// Port of `STRING_OPTVAL(s)` (`option.h:56`):
/// `((OptVal) { .type = kOptValTypeString, .data.string = s })`. The
/// `CSTR_AS_OPTVAL`/`CSTR_TO_OPTVAL` wrappers (`option.h:58`) collapse to this
/// here — the owned `String` is both the "as" (borrow) and "to" (copy) form.
fn STRING_OPTVAL(s: String) -> OptVal {
    // c:56
    OptVal {
        r#type: OptValType::kOptValTypeString,
        data: OptValData::string(s),
    }
}

/// `#define kOptFlagSecure (1 << 14)` (`option_defs.h:31`) — cannot change in
/// modeline or secure mode.
const kOptFlagSecure: u32 = 1 << 14;
/// `#define kOptFlagFunc (1 << 24)` (`option_defs.h:41`) — accept a function
/// reference or a lambda.
const kOptFlagFunc: u32 = 1 << 24;

// ── option.h ─────────────────────────────────────────────────────────────────

/// `OPT_GLOBAL = 0x01` (`option.h:26`) — use global value.
pub const OPT_GLOBAL: i32 = 0x01;
/// `OPT_LOCAL = 0x02` (`option.h:27`) — use local value.
pub const OPT_LOCAL: i32 = 0x02;

// ── the reduced options[] table ──────────────────────────────────────────────

/// `OptIndex` — index into the `options` table. Upstream a generated enum;
/// here a `usize` (see the file-header RUST-PORT NOTE).
pub type OptIndex = usize;
/// `kOptInvalid` — sentinel for "no such option".
pub const kOptInvalid: OptIndex = usize::MAX;

/// `typedef struct { char *fullname; char *shortname; uint32_t flags;
/// OptValType type; … OptVal def_val; } vimoption_T;` (`option_defs.h:167`),
/// reduced to the fields eval's OptVal path touches.
struct vimoption_T {
    /// `char *fullname` — full option name.
    fullname: &'static str,
    /// `char *shortname` — permissible abbreviation.
    shortname: &'static str,
    /// `uint32_t flags` — option flags (reduced; `kOptFlagFunc`/`kOptFlagSecure`
    /// are the only bits eval's OptVal path reads).
    flags: u32,
    /// `OptValType type` — option type.
    r#type: OptValType,
    /// `OptVal def_val` — default value.
    def_val: OptVal,
}

/// The `options[]` array (`option.c`, generated from `options.lua`). RUST-PORT
/// NOTE: the reduced subset of number/string/boolean options reachable via
/// `&opt` / `:let &opt` / `getbufvar(b, '&opt')` — no per-buffer/window scope,
/// no side-effect flags. A `LazyLock` stands in for the C file-static array; the
/// `b`/`n`/`s` row builders are local closures (not `fn` items) so they carry no
/// invented C names.
static options: LazyLock<Vec<vimoption_T>> = LazyLock::new(|| {
    let b = |fullname: &'static str, shortname: &'static str, def: TriState| vimoption_T {
        fullname,
        shortname,
        flags: 0,
        r#type: OptValType::kOptValTypeBoolean,
        def_val: BOOLEAN_OPTVAL(def),
    };
    let n = |fullname: &'static str, shortname: &'static str, def: OptInt| vimoption_T {
        fullname,
        shortname,
        flags: 0,
        r#type: OptValType::kOptValTypeNumber,
        def_val: NUMBER_OPTVAL(def),
    };
    let s = |fullname: &'static str, shortname: &'static str, def: &str| vimoption_T {
        fullname,
        shortname,
        flags: 0,
        r#type: OptValType::kOptValTypeString,
        def_val: STRING_OPTVAL(def.to_string()),
    };
    vec![
        // Boolean options.
        b("ignorecase", "ic", TriState::kFalse),
        b("smartcase", "scs", TriState::kFalse),
        b("magic", "magic", TriState::kTrue),
        b("expandtab", "et", TriState::kFalse),
        b("number", "nu", TriState::kFalse),
        b("relativenumber", "rnu", TriState::kFalse),
        b("wrap", "wrap", TriState::kTrue),
        b("hlsearch", "hls", TriState::kFalse),
        b("incsearch", "is", TriState::kFalse),
        b("autoindent", "ai", TriState::kFalse),
        // Number options.
        n("tabstop", "ts", 8),
        n("shiftwidth", "sw", 8),
        n("softtabstop", "sts", 0),
        n("textwidth", "tw", 0),
        n("scrolloff", "so", 0),
        // String options. RUST-PORT NOTE: 'filetype'/'syntax' fire autocommand
        // side effects upstream (not modeled here — value is stored only).
        s("filetype", "ft", ""),
        s("syntax", "syn", ""),
    ]
});

thread_local! {
    /// The option value store. RUST-PORT NOTE: stands in for the global option
    /// variables reached via `get_varp_scope`/`varp`; keyed by `OptIndex`,
    /// lazily seeded from `def_val` on read.
    static option_values: RefCell<HashMap<OptIndex, OptVal>> = RefCell::new(HashMap::new());
}

// ── option.c ports ───────────────────────────────────────────────────────────

/// Port of `find_option_len()` from `csrc/option.c` (upstream `option.c:3341`).
/// RUST-PORT NOTE: the generated perfect hash (`find_option_hash`) is a linear
/// scan over the reduced table, matching full name or abbreviation.
pub fn find_option_len(name: &str, len: usize) -> OptIndex {
    // c:3344 int index = find_option_hash(name, len);
    let name = &name[..len.min(name.len())];
    for (i, opt) in options.iter().enumerate() {
        if opt.fullname == name || opt.shortname == name {
            return i;
        }
    }
    // c:3345 return … : kOptInvalid;
    kOptInvalid
}

/// Port of `find_option()` from `csrc/option.c` (upstream `option.c:3353`) —
/// resolve an option name (or abbreviation) to its index.
pub fn find_option(name: &str) -> OptIndex {
    // c:3356 return find_option_len(name, strlen(name));
    find_option_len(name, name.len())
}

/// Port of `find_option_end()` from `csrc/option.c` (upstream `option.c:1303`).
/// RUST-PORT NOTE: TTY/keycode handling (`find_tty_option_end`) is dropped (no
/// terminal); returns the byte length of the isolated option name (the C `end`
/// offset) and writes the resolved index to `opt_idxp`, or `None` if `arg` does
/// not start with an alphabetic character.
pub fn find_option_end(arg: &str, opt_idxp: &mut OptIndex) -> Option<usize> {
    let p = arg.as_bytes();

    // c:1315 if (!ASCII_ISALPHA(*p)) { *opt_idxp = kOptInvalid; return NULL; }
    if p.is_empty() || !p[0].is_ascii_alphabetic() {
        *opt_idxp = kOptInvalid;
        return None;
    }
    // c:1319 while (ASCII_ISALPHA(*p)) { p++; }
    let mut i = 0;
    while i < p.len() && p[i].is_ascii_alphabetic() {
        i += 1;
    }

    // c:1323 *opt_idxp = find_option_len(arg, (size_t)(p - arg));
    *opt_idxp = find_option_len(arg, i);
    // c:1324 return p;
    Some(i)
}

/// Port of `option_has_type()` from `csrc/option.c` (upstream `option.c`).
pub fn option_has_type(opt_idx: OptIndex, r#type: OptValType) -> bool {
    // c: return opt_idx != kOptInvalid && options[opt_idx].type == type;
    opt_idx != kOptInvalid && options[opt_idx].r#type == r#type
}

/// Port of `is_tty_option()` from `csrc/option.c` (upstream `option.c:3280`).
/// RUST-PORT NOTE: reduced to the recognizable TTY names (`t_*`, `term`,
/// `ttytype`) instead of parsing keycodes via `find_tty_option_end`.
pub fn is_tty_option(name: &str) -> bool {
    // c: return find_tty_option_end(name) != NULL;
    name == "term" || name == "ttytype" || name.starts_with("t_")
}

/// Port of `optval_free()` from `csrc/option.c` (upstream `option.c:3359`).
/// RUST-PORT NOTE: no-op — the owned `String` inside an `OptVal` is dropped by
/// Rust; retained as a named port so the `set_option_from_tv` call site matches
/// the C control flow.
pub fn optval_free(o: OptVal) {
    // c:3360 switch (o.type) { … api_free_string(o.data.string); … }
    let _ = o;
}

/// Port of `optval_copy()` from `csrc/option.c` (upstream `option.c:3377`).
/// RUST-PORT NOTE: `String` clone stands in for `copy_string`; scalar variants
/// return the value unchanged as in C.
pub fn optval_copy(o: OptVal) -> OptVal {
    // c:3379 switch (o.type)
    match o.r#type {
        // c:3380 kOptValTypeNil / kOptValTypeBoolean / kOptValTypeNumber: return o;
        OptValType::kOptValTypeNil
        | OptValType::kOptValTypeBoolean
        | OptValType::kOptValTypeNumber => o,
        // c:3384 kOptValTypeString: return STRING_OPTVAL(copy_string(o.data.string, NULL));
        OptValType::kOptValTypeString => {
            let OptValData::string(ref str_) = o.data else {
                return o;
            };
            STRING_OPTVAL(str_.clone())
        }
    }
}

/// Port of `optval_from_varp()` from `csrc/option.c` (upstream `option.c:3424`).
/// RUST-PORT NOTE: the `void *varp` dereference (`*(int*)varp` etc.) is replaced
/// by a read from the thread-local `option_values` store, falling back to the
/// option's `def_val`; the `varp` parameter and the `b_changed` special case are
/// dropped (no buffer). The `type` switch is preserved.
pub fn optval_from_varp(opt_idx: OptIndex) -> OptVal {
    // c:3433 OptValType type = option_get_type(opt_idx);
    let r#type = options[opt_idx].r#type;

    // c:3435 switch (type) — read the stored value (or the default) for opt_idx.
    let stored = option_values.with(|m| m.borrow().get(&opt_idx).cloned());
    match r#type {
        // c:3436 kOptValTypeNil: return NIL_OPTVAL;
        OptValType::kOptValTypeNil => NIL_OPTVAL(),
        // c:3438 kOptValTypeBoolean: return BOOLEAN_OPTVAL(TRISTATE_FROM_INT(*(int*)varp));
        OptValType::kOptValTypeBoolean
        // c:3440 kOptValTypeNumber: return NUMBER_OPTVAL(*(OptInt*)varp);
        | OptValType::kOptValTypeNumber
        // c:3442 kOptValTypeString: return STRING_OPTVAL(cstr_as_string(*(char**)varp));
        | OptValType::kOptValTypeString => stored.unwrap_or_else(|| options[opt_idx].def_val.clone()),
    }
}

/// Port of `get_option_value()` from `csrc/option.c` (upstream `option.c:3630`).
/// RUST-PORT NOTE: `get_varp_scope` is collapsed — the value is read straight
/// from `optval_from_varp(opt_idx)`.
pub fn get_option_value(opt_idx: OptIndex, opt_flags: i32) -> OptVal {
    let _ = opt_flags;
    // c:3632 if (opt_idx == kOptInvalid) { return NIL_OPTVAL; }
    if opt_idx == kOptInvalid {
        return NIL_OPTVAL();
    }

    // c:3636 vimoption_T *opt = &options[opt_idx];
    // c:3637 void *varp = get_varp_scope(opt, opt_flags);
    // c:3639 return optval_copy(optval_from_varp(opt_idx, varp));
    optval_copy(optval_from_varp(opt_idx))
}

/// Port of `set_option_value()` from `csrc/option.c` (upstream `option.c:4116`).
/// RUST-PORT NOTE: the `sandbox` counter is not modeled standalone (see
/// `vars.c`'s `check_secure` port), so the `kOptFlagSecure` guard is inert here;
/// the validating `set_option` (with `did_set` side effects) collapses to a
/// store write. Returns `Some(msg)` on error, `None` on success.
pub fn set_option_value(opt_idx: OptIndex, value: OptVal, opt_flags: i32) -> Option<String> {
    // c:4118 assert(opt_idx != kOptInvalid);
    assert!(opt_idx != kOptInvalid);
    let _ = opt_flags;

    // c:4121 uint32_t flags = options[opt_idx].flags;
    let flags = options[opt_idx].flags;

    // c:4124 if (sandbox > 0 && (flags & kOptFlagSecure)) return _(e_sandbox);
    // RUST-PORT NOTE: sandbox == 0 here, so this never fires.
    let _ = (flags, kOptFlagSecure);

    // c:4128 return set_option(opt_idx, optval_copy(value), …);
    // RUST-PORT NOTE: store the value (no side effects).
    option_values.with(|m| {
        m.borrow_mut().insert(opt_idx, optval_copy(value));
    });
    None
}

/// Port of `set_option_value_handle_tty()` from `csrc/option.c` (upstream
/// `option.c:4152`). Returns `Some(msg)` on error, `None` on success.
pub fn set_option_value_handle_tty(
    name: &str,
    opt_idx: OptIndex,
    value: OptVal,
    opt_flags: i32,
) -> Option<String> {
    // c:4158 if (opt_idx == kOptInvalid) {
    if opt_idx == kOptInvalid {
        // c:4159 if (is_tty_option(name)) return NULL;  // Fail silently.
        if is_tty_option(name) {
            return None;
        }
        // c:4163 snprintf(errbuf, …, _(e_unknown_option2), name); return errbuf;
        return Some(format!("E355: Unknown option: {name}")); // e_unknown_option2
    }

    // c:4167 return set_option_value(opt_idx, value, opt_flags);
    set_option_value(opt_idx, value, opt_flags)
}

// ── eval/vars.c ports (the OptVal↔typval boundary) ───────────────────────────

/// Port of `tv_to_optval()` from `csrc/eval/vars.c` (upstream `vars.c:3196`) —
/// convert a `typval_T` to the `OptVal` for option `opt_idx`. Sets `*error` on a
/// type error.
fn tv_to_optval(tv: &typval_T, opt_idx: OptIndex, option: &str, error: &mut bool) -> OptVal {
    // c:3198 OptVal value = NIL_OPTVAL;
    let mut value = NIL_OPTVAL();
    // c:3200 bool err = false;
    let mut err = false;
    // c:3201 const bool is_tty_opt = is_tty_option(option);
    let is_tty_opt = is_tty_option(option);
    // c:3202-3204
    let option_has_bool = !is_tty_opt && option_has_type(opt_idx, OptValType::kOptValTypeBoolean);
    let option_has_num = !is_tty_opt && option_has_type(opt_idx, OptValType::kOptValTypeNumber);
    let option_has_str = is_tty_opt || option_has_type(opt_idx, OptValType::kOptValTypeString);

    // c:3206 if (!is_tty_opt && (get_option(opt_idx)->flags & kOptFlagFunc) && tv_is_func(*tv)) {
    // RUST-PORT NOTE: tv_is_func (typval.h:426) inlined: VAR_FUNC || VAR_PARTIAL.
    let tv_is_func = matches!(tv.v_type, VarType::VAR_FUNC | VarType::VAR_PARTIAL);
    if !is_tty_opt && (options[opt_idx].flags & kOptFlagFunc) != 0 && tv_is_func {
        // c:3210 char *strval = encode_tv2string(tv, NULL);
        let strval = encode_tv2string(tv);
        // c:3211 err = strval == NULL;  (never NULL here)
        // c:3212 value = CSTR_AS_OPTVAL(strval);
        value = STRING_OPTVAL(strval);
    } else if option_has_bool || option_has_num {
        // c:3214 varnumber_T n = option_has_num ? tv_get_number_chk : tv_get_bool_chk;
        let n = if option_has_num {
            tv_get_number_chk(tv, Some(&mut err))
        } else {
            tv_get_bool_chk(tv, Some(&mut err))
        };
        // c:3217 if (!err && tv->v_type == VAR_STRING && n == 0) {
        if !err && tv.v_type == VarType::VAR_STRING && n == 0 {
            // c:3218-3220 check the string is all zeros (an actual number)
            let sbytes: &[u8] = match &tv.vval {
                typval_vval_union::v_string(sv) => sv.as_bytes(),
                _ => &[],
            };
            let mut idx = 0usize;
            while idx < sbytes.len() && sbytes[idx] == b'0' {
                idx += 1;
            }
            if idx == 0 || idx < sbytes.len() {
                // c:3222 There's another character after zeros or the string is empty.
                err = true;
                // c:3224 semsg(_("E521: Number required: &%s = '%s'"), option, …);
                let sstr = match &tv.vval {
                    typval_vval_union::v_string(sv) => sv.as_str(),
                    _ => "",
                };
                semsg(&format!("E521: Number required: &{option} = '{sstr}'"));
            }
        }
        // c:3228 value = option_has_num ? NUMBER_OPTVAL((OptInt)n)
        //                              : BOOLEAN_OPTVAL(TRISTATE_FROM_INT(n));
        value = if option_has_num {
            NUMBER_OPTVAL(n as OptInt)
        } else {
            BOOLEAN_OPTVAL(TRISTATE_FROM_INT(n))
        };
    } else if option_has_str {
        // c:3231 if (tv->v_type != VAR_BOOL && tv->v_type != VAR_SPECIAL) {
        if tv.v_type != VarType::VAR_BOOL && tv.v_type != VarType::VAR_SPECIAL {
            // c:3232 const char *strval = tv_get_string_buf_chk(tv, nbuf);
            let strval = tv_get_string_buf_chk(tv);
            // c:3233 err = strval == NULL;
            err = strval.is_none();
            // c:3234 value = CSTR_TO_OPTVAL(strval);
            value = STRING_OPTVAL(strval.unwrap_or_default());
        } else if !is_tty_opt {
            // c:3236 err = true; emsg(_(e_string_required));
            err = true;
            emsg("E928: String required"); // e_string_required
        }
    } else {
        // c:3240 abort();  // This should never happen.
        unreachable!("tv_to_optval: option has no known type");
    }

    // c:3243 if (error != NULL) { *error = err; }
    *error = err;
    // c:3246 return value;
    value
}

/// Port of `optval_as_tv()` from `csrc/eval/vars.c` (upstream `vars.c:3256`) —
/// convert an `OptVal` to a `typval_T`. `numbool` renders booleans as numbers
/// (for backwards compatibility).
pub fn optval_as_tv(value: OptVal, numbool: bool) -> typval_T {
    // c:3258 typval_T rettv = { .v_type = VAR_SPECIAL, .vval.v_special = kSpecialVarNull };
    let mut rettv = typval_T {
        v_type: VarType::VAR_SPECIAL,
        v_lock: VarLockStatus::VAR_UNLOCKED,
        vval: typval_vval_union::v_special(SpecialVarValue::kSpecialVarNull),
    };

    // c:3260 switch (value.type)
    match value.r#type {
        // c:3261 kOptValTypeNil: break;
        OptValType::kOptValTypeNil => {}
        // c:3263 kOptValTypeBoolean:
        OptValType::kOptValTypeBoolean => {
            let boolean = match value.data {
                OptValData::boolean(t) => t,
                _ => TriState::kNone,
            };
            if numbool {
                // c:3265 rettv.v_type = VAR_NUMBER; rettv.vval.v_number = value.data.boolean;
                rettv.v_type = VarType::VAR_NUMBER;
                rettv.vval = typval_vval_union::v_number(boolean as varnumber_T);
            } else if boolean != TriState::kNone {
                // c:3268 rettv.v_type = VAR_BOOL; rettv.vval.v_bool = value.data.boolean == kTrue;
                rettv.v_type = VarType::VAR_BOOL;
                rettv.vval = typval_vval_union::v_bool(if boolean == TriState::kTrue {
                    BoolVarValue::kBoolVarTrue
                } else {
                    BoolVarValue::kBoolVarFalse
                });
            }
            // c:3271 break;  // return v:null for None boolean value.
        }
        // c:3272 kOptValTypeNumber:
        OptValType::kOptValTypeNumber => {
            let number = match value.data {
                OptValData::number(n) => n,
                _ => 0,
            };
            // c:3273 rettv.v_type = VAR_NUMBER; rettv.vval.v_number = value.data.number;
            rettv.v_type = VarType::VAR_NUMBER;
            rettv.vval = typval_vval_union::v_number(number as varnumber_T);
        }
        // c:3276 kOptValTypeString:
        OptValType::kOptValTypeString => {
            let string = match value.data {
                OptValData::string(s) => s,
                _ => String::new(),
            };
            // c:3277 rettv.v_type = VAR_STRING; rettv.vval.v_string = value.data.string.data;
            rettv.v_type = VarType::VAR_STRING;
            rettv.vval = typval_vval_union::v_string(string);
        }
    }

    // c:3282 return rettv;
    rettv
}

/// Port of `set_option_from_tv()` from `csrc/eval/vars.c` (upstream
/// `vars.c:3286`) — set option `varname` to the value of `varp` (as used by
/// `setbufvar`/`setwinvar` with a `&`-prefixed name).
pub fn set_option_from_tv(varname: &str, varp: &typval_T) {
    // c:3288 OptIndex opt_idx = find_option(varname);
    let opt_idx = find_option(varname);
    // c:3289 if (opt_idx == kOptInvalid) { semsg(_(e_unknown_option2), varname); return; }
    if opt_idx == kOptInvalid {
        semsg(&format!("E355: Unknown option: {varname}")); // e_unknown_option2
        return;
    }

    // c:3294 bool error = false;
    let mut error = false;
    // c:3295 OptVal value = tv_to_optval(varp, opt_idx, varname, &error);
    let value = tv_to_optval(varp, opt_idx, varname, &mut error);

    // c:3297 if (!error) {
    if !error {
        // c:3298 const char *errmsg = set_option_value_handle_tty(varname, opt_idx, value, OPT_LOCAL);
        let errmsg = set_option_value_handle_tty(varname, opt_idx, value.clone(), OPT_LOCAL);
        // c:3300 if (errmsg) { emsg(errmsg); }
        if let Some(errmsg) = errmsg {
            emsg(&errmsg);
        }
    }
    // c:3304 optval_free(value);
    optval_free(value);
}

/// Port of `find_option_var_end()` from `csrc/eval.c` (upstream `eval.c:6297`) —
/// isolate the option name after a `&`/`+` sigil, decoding a leading `g:`/`l:`
/// scope prefix. RUST-PORT NOTE: C takes `const char **arg` (advanced past the
/// prefix) and returns the `end` pointer; here `arg` is the full sigil-prefixed
/// string and the result is `(name, opt_idx, opt_flags)` — `name` is `None`
/// when no option is found (the C `end == NULL` case).
pub fn find_option_var_end(arg: &str) -> (Option<String>, OptIndex, i32) {
    let bytes = arg.as_bytes();
    // c:6302 p++;  (skip the '&' / '+' sigil)
    let mut p = 1usize;
    let mut opt_flags;

    // c:6303 if (*p == 'g' && p[1] == ':') { *opt_flags = OPT_GLOBAL; p += 2; }
    if p + 1 < bytes.len() && bytes[p] == b'g' && bytes[p + 1] == b':' {
        opt_flags = OPT_GLOBAL;
        p += 2;
    } else if p + 1 < bytes.len() && bytes[p] == b'l' && bytes[p + 1] == b':' {
        // c:6306 else if (*p == 'l' && p[1] == ':') { *opt_flags = OPT_LOCAL; p += 2; }
        opt_flags = OPT_LOCAL;
        p += 2;
    } else {
        // c:6310 *opt_flags = 0;
        opt_flags = 0;
    }

    // c:6313 const char *end = find_option_end(p, opt_idxp);
    let mut opt_idx = kOptInvalid;
    let end = find_option_end(&arg[p..], &mut opt_idx);
    // c:6314 *arg = end == NULL ? *arg : p;  (caller-side pointer advance)
    // c:6315 return end;
    match end {
        Some(len) => (Some(arg[p..p + len].to_string()), opt_idx, opt_flags),
        None => (None, opt_idx, opt_flags),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset_store() {
        option_values.with(|m| m.borrow_mut().clear());
    }

    #[test]
    fn find_option_resolves_name_and_abbrev() {
        assert_eq!(find_option("ignorecase"), find_option("ic"));
        assert_ne!(find_option("ignorecase"), kOptInvalid);
        assert_eq!(find_option("nosuchopt"), kOptInvalid);
    }

    #[test]
    fn get_default_and_set_number() {
        reset_store();
        let ts = find_option("tabstop");
        // Default value is 8.
        let v = get_option_value(ts, 0);
        assert_eq!(v.data, OptValData::number(8));
        // Set to 4 and read back.
        assert_eq!(set_option_value(ts, NUMBER_OPTVAL(4), 0), None);
        assert_eq!(get_option_value(ts, 0).data, OptValData::number(4));
    }

    #[test]
    fn tv_to_optval_number_and_bool_and_string() {
        reset_store();
        // Number option from a VAR_NUMBER.
        let ts = find_option("tabstop");
        let mut err = false;
        let ov = tv_to_optval(&typval_T::from(2 as varnumber_T), ts, "tabstop", &mut err);
        assert!(!err);
        assert_eq!(ov.data, OptValData::number(2));

        // Boolean option from a VAR_NUMBER (1 → kTrue).
        let ic = find_option("ignorecase");
        let ov = tv_to_optval(
            &typval_T::from(1 as varnumber_T),
            ic,
            "ignorecase",
            &mut err,
        );
        assert!(!err);
        assert_eq!(ov.data, OptValData::boolean(TriState::kTrue));

        // String option from a VAR_STRING.
        let ft = find_option("filetype");
        let ov = tv_to_optval(
            &typval_T::from("rust".to_string()),
            ft,
            "filetype",
            &mut err,
        );
        assert!(!err);
        assert_eq!(ov.data, OptValData::string("rust".to_string()));
    }

    #[test]
    fn tv_to_optval_e521_on_nonnumeric_string() {
        // A non-numeric string for a number option is an error (E521).
        let ts = find_option("tabstop");
        let mut err = false;
        let _ = tv_to_optval(&typval_T::from("abc".to_string()), ts, "tabstop", &mut err);
        assert!(err);
    }

    #[test]
    fn optval_as_tv_bool_numbool_and_native() {
        // numbool=true → VAR_NUMBER carrying the tri-state int.
        let tv = optval_as_tv(BOOLEAN_OPTVAL(TriState::kTrue), true);
        assert_eq!(tv.v_type, VarType::VAR_NUMBER);
        assert!(matches!(tv.vval, typval_vval_union::v_number(1)));

        // numbool=false → VAR_BOOL.
        let tv = optval_as_tv(BOOLEAN_OPTVAL(TriState::kTrue), false);
        assert_eq!(tv.v_type, VarType::VAR_BOOL);
        assert!(matches!(
            tv.vval,
            typval_vval_union::v_bool(BoolVarValue::kBoolVarTrue)
        ));

        // None boolean → v:null (VAR_SPECIAL) when not numbool.
        let tv = optval_as_tv(BOOLEAN_OPTVAL(TriState::kNone), false);
        assert_eq!(tv.v_type, VarType::VAR_SPECIAL);
    }

    #[test]
    fn set_option_from_tv_roundtrip_and_unknown() {
        reset_store();
        // Set 'shiftwidth' via a &-style typval write, then read as a typval.
        set_option_from_tv("shiftwidth", &typval_T::from(3 as varnumber_T));
        let sw = find_option("shiftwidth");
        let tv = optval_as_tv(get_option_value(sw, 0), true);
        assert!(matches!(tv.vval, typval_vval_union::v_number(3)));

        // Unknown option name resolves to kOptInvalid.
        assert_eq!(find_option("definitelynotanoption"), kOptInvalid);
    }

    #[test]
    fn find_option_var_end_decodes_scope_prefix() {
        // "&ic" → name "ic", flags 0.
        let (name, idx, flags) = find_option_var_end("&ic");
        assert_eq!(name.as_deref(), Some("ic"));
        assert_ne!(idx, kOptInvalid);
        assert_eq!(flags, 0);

        // "&g:number" → name "number", OPT_GLOBAL.
        let (name, _idx, flags) = find_option_var_end("&g:number");
        assert_eq!(name.as_deref(), Some("number"));
        assert_eq!(flags, OPT_GLOBAL);

        // "&l:wrap" → name "wrap", OPT_LOCAL.
        let (name, _idx, flags) = find_option_var_end("&l:wrap");
        assert_eq!(name.as_deref(), Some("wrap"));
        assert_eq!(flags, OPT_LOCAL);
    }
}
