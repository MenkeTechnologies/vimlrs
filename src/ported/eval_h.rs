//! Port of selected declarations from `src/nvim/eval.h` (vendored at
//! `csrc/eval.h`). Header-defined eval types live in the header port
//! (PORT.md Rule C).
#![allow(non_camel_case_types, non_upper_case_globals)]

/// `OK` / `FAIL` return codes (`src/nvim/vim_defs.h`, extern). Ported here as
/// the eval ports' shared success/failure constants.
pub const OK: i32 = 1;
/// `FAIL` — operation failed.
pub const FAIL: i32 = 0;

/// `typedef enum { … } exprtype_T;` — types for comparison expressions.
/// (eval.h c:118)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum exprtype_T {
    /// (unused sentinel)
    EXPR_UNKNOWN = 0,
    /// `==`
    EXPR_EQUAL,
    /// `!=`
    EXPR_NEQUAL,
    /// `>`
    EXPR_GREATER,
    /// `>=`
    EXPR_GEQUAL,
    /// `<`
    EXPR_SMALLER,
    /// `<=`
    EXPR_SEQUAL,
    /// `=~`
    EXPR_MATCH,
    /// `!~`
    EXPR_NOMATCH,
    /// `is`
    EXPR_IS,
    /// `isnot`
    EXPR_ISNOT,
}
