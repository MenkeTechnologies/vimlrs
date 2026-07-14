//! vimlrs — a faithful Rust port of the Vimscript (VimL) interpreter, compiled
//! to [`fusevm`] bytecode and run on its VM + Cranelift JIT.
//!
//! VimL is the **4th language frontend** on fusevm, after zshrs (zsh),
//! strykelang (stryke), and awkrs (awk). Like zshrs, it has **no local VM and
//! no local JIT**: source is lexed and parsed into an AST, lowered to fusevm
//! bytecode, and executed by fusevm.
//!
//! ## Two-zone layout (the zshrs porting discipline)
//!
//! - [`ported`] — strict 1:1 ports of the Neovim eval C source under `vendor/`.
//!   Exact C names, `// c:NNN` citations, no invented helpers/structs. See
//!   `docs/PORT.md`.
//! - **Crate-root carve-outs** — net-new synthesis with no C counterpart
//!   (Neovim's eval is a string-walking interpreter with no AST/bytecode):
//!   [`viml_lexer`], [`viml_ast`], [`viml_parser`], [`compile_viml`],
//!   [`fusevm_bridge`]. These mirror zshrs's `compile_zsh.rs`/`fusevm_bridge.rs`
//!   carve-outs and are clearly headed "EXTENSION — NO vendor/ counterpart".

// The `ported` zone keeps Vim's exact C identifiers (e.g. `ufunc_T`,
// `except_type_T`, `VAR_FLAVOUR_*`) for 1:1 fidelity, so the non-camel-case
// lint is intentionally disabled crate-wide.
#![allow(non_camel_case_types)]

pub mod ported;

// Synthesis carve-outs (no `vendor/` counterpart).
pub mod aot;
pub mod banner;
pub mod builtin_docs;
pub mod cli;
pub mod compile_viml;
pub mod dap;
pub mod fusevm_bridge;
pub mod fusevm_disasm;
/// Rust map API over the ported `hashtab_T` (see the module docs: the C has no
/// `contains_key`/`iter_mut`, so the adapter lives in the synthesis zone).
pub mod hashtab_map;
/// AOP command-intercept (before/after/around advice on user-function calls) —
/// a vimlrs/zshrs-original extension with no Vim counterpart.
pub mod intercepts;
pub mod lsp;
pub mod repl;
pub mod script_cache;
pub mod viml_ast;
pub mod viml_lexer;
pub mod viml_parser;
pub mod viml_regex;

pub use fusevm_bridge::{eval_expr, eval_source};
pub use ported::eval::typval_defs_h::typval_T;
