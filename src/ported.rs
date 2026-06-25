//! Strict 1:1 ports of the Neovim eval C source vendored under `csrc/`.
//!
//! Every file here mirrors a `csrc/` file, uses the exact C names, and cites its
//! source (PORT.md discipline, adapted from zshrs). Net-new synthesis (lexer,
//! parser, AST, bytecode compiler, fusevm bridge) lives in the crate-root
//! carve-out modules instead, never here.

/// Port of `src/nvim/charset.c` (extern: `vim_str2nr`).
pub mod charset;
/// Port of `src/nvim/message.c` (extern: `emsg`/`did_emsg`).
pub mod message;
/// Port of `src/nvim/eval.h` (header types: `exprtype_T`, `OK`/`FAIL`).
pub mod eval_h;
/// Port of `src/nvim/eval.c` and its `eval/` subtree.
pub mod eval;
/// Port of `src/nvim/option.c` (subset: the option table, `&opt`, `:set`).
pub mod option;
/// Generated not-yet-ported surface: one stub per vendored C function
/// definition (real name + `csrc/<file>:<line>` citation). Regenerate with
/// `scripts/gen_port_stubs.sh`; ported functions drop out automatically.
pub mod stubs;
