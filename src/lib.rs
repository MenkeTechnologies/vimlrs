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
//!   [`fusevm_bridge`], [`vimstr`]. These mirror zshrs's
//!   `compile_zsh.rs`/`fusevm_bridge.rs` carve-outs and are clearly headed
//!   "EXTENSION — NO vendor/ counterpart". [`vimstr`] is the odd one out: it is
//!   a *representation* rather than a stage — the byte string backing a
//!   `VAR_STRING`, which the C reaches by dereferencing a `char_u *` and so has
//!   no named counterpart to port.

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
// Outer docs here would merge with dap_client.rs's own `//!` block and make
// rustdoc resolve that block's links against the crate root; the module
// documents itself.
pub mod dap_client;
pub mod embed;
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
/// VimL wiring for inline Rust FFI (`rust { ... }` blocks): the RustSugar config,
/// the pre-lex desugar, and the exported-name set for builtin override.
pub mod rust_ffi;
pub mod script_cache;
pub mod tiers;
pub mod viml_ast;
pub mod viml_lexer;
pub mod viml_parser;
pub mod viml_regex;
// Outer docs here would merge with vimstr.rs's own `//!` block, resolving its
// links against the crate root; the module documents itself.
pub mod vim_funcs;
pub mod vimstr;

pub use embed::run_program;
pub use fusevm_bridge::{eval_expr, eval_source};
pub use ported::eval::typval_defs_h::typval_T;

/// Interpreter-thread stack size. The parser, bytecode compiler and value `Drop`
/// all walk the expression tree recursively, so a pathologically deep-but-valid
/// script (`let x = ` + 500000 `-`, which real vim evaluates without error) would
/// overflow the default 8 MiB main-thread stack and abort with SIGABRT. Vim
/// tolerates such depth because its C frames are tiny; a debug Rust build's
/// frames are far larger, so the interpreter runs on a thread with a 1 GiB stack
/// (virtual reservation, pages commit only when touched). This keeps vimlrs
/// matching vim's behaviour — succeed on deep linear chains, and raise `E1169` on
/// deep *bracket* nesting via the parser's depth guard — instead of crashing.
/// Verified: 500000 nested `-` succeed; 1000 nested `(` raise E1169 like vim.
/// Used by the binary's worker thread and by [`run_program`].
pub const WORKER_STACK: usize = 1 << 30;
