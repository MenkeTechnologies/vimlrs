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
//! - [`ported`] — strict 1:1 ports of the Neovim eval C source under `csrc/`.
//!   Exact C names, `// c:NNN` citations, no invented helpers/structs. See
//!   `docs/PORT.md`.
//! - **Crate-root carve-outs** — net-new synthesis with no C counterpart
//!   (Neovim's eval is a string-walking interpreter with no AST/bytecode):
//!   [`viml_lexer`], [`viml_ast`], [`viml_parser`], [`compile_viml`],
//!   [`fusevm_bridge`]. These mirror zshrs's `compile_zsh.rs`/`fusevm_bridge.rs`
//!   carve-outs and are clearly headed "EXTENSION — NO csrc/ counterpart".

// The `ported` zone keeps Vim's exact C identifiers (e.g. `ufunc_T`,
// `except_type_T`, `VAR_FLAVOUR_*`) for 1:1 fidelity, so the non-camel-case
// lint is intentionally disabled crate-wide.
#![allow(non_camel_case_types)]

pub mod ported;

// Synthesis carve-outs (no `csrc/` counterpart).
pub mod aot;
pub mod cli;
pub mod compile_viml;
pub mod dap;
pub mod fusevm_bridge;
pub mod fusevm_disasm;
pub mod lsp;
pub mod script_cache;
pub mod viml_ast;
pub mod viml_json;
pub mod viml_lexer;
pub mod viml_parser;
pub mod viml_regex;

pub use fusevm_bridge::{eval_expr, eval_source};
pub use ported::eval::typval_defs_h::typval_T;
