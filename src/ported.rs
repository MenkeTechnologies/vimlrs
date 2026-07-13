//! Strict 1:1 ports of the Neovim eval C source vendored under `vendor/`.
//!
//! Every file here mirrors a `vendor/` file, uses the exact C names, and cites its
//! source (PORT.md discipline, adapted from zshrs). Net-new synthesis (lexer,
//! parser, AST, bytecode compiler, fusevm bridge) lives in the crate-root
//! carve-out modules instead, never here.

// Faithful 1:1 C ports mirror Neovim's source structure line-for-line, so clippy
// style lints (auto-deref, map_or, range patterns, …) and the unused-assignment /
// unused-paren / private-interface warnings that would demand idiomatic-Rust
// rewrites are relaxed for this subtree — the port must stay a match of `vendor/`,
// not diverge to satisfy a style linter. Net-new synthesis modules keep full lints.
#![allow(clippy::all)]
#![allow(unused_assignments)]
#![allow(unused_parens)]
#![allow(private_interfaces)]
// Faithful C ports keep the upstream lowercase global names (p_ws, msg_silent,
// capture_ga, ...) and include reference ports no runtime path calls (the
// bytecode frontend supersedes them), so these lints don't apply to this subtree.
#![allow(non_upper_case_globals)]
#![allow(dead_code)]
#![allow(unused_variables)]

/// Port of `src/nvim/buffer.c` + `memline.c` (subset: the `buf_T` model, buffer
/// list `buflist_*`, and the `ml_*` line store behind `getbufline`/`bufnr`/…).
pub mod buffer;
/// Port of `src/nvim/charset.c` (extern: `vim_str2nr`).
pub mod charset;
/// Port of `src/nvim/eval.c` and its `eval/` subtree.
pub mod eval;
/// Port of `src/nvim/eval.h` (header types: `exprtype_T`, `OK`/`FAIL`).
pub mod eval_h;
/// Port of `src/nvim/ex_eval.c` (abort/exception state predicates).
pub mod ex_eval;
/// Port of `src/nvim/grid_defs.h` (ScreenGrid) + `ui_compositor.c` (empty standalone).
pub mod grid;
/// Port of `src/nvim/strings.c` (the Vimscript string builtins `f_string`,
/// `f_strlen`, `f_byteidx`, `f_tr`, …). Home file not under the vendored
/// `vendor/eval/` tree; see `tests/data/fake_fn_allowlist.txt`.
/// Port of `src/nvim/keycodes.c` (subset: `trans_special`/`find_special_key`
/// behind the `"\<Esc>"` string escape, and `get_special_key_name` behind
/// `keytrans()` — the character-valued keys only; see the module docs).
pub mod keycodes;
/// Port of `src/nvim/mark.c` (subset: the mark store behind setmark_pos/getpos).
pub mod mark;
/// Port of `src/nvim/mbyte.c` (subset: the UTF-8 codec helpers `utf_ptr2char`,
/// `utf_ptr2len`, `utf_char2len`, `utf_char2bytes` behind the JSON decoder).
pub mod mbyte;
/// Port of `src/nvim/message.c` (extern: `emsg`/`did_emsg`).
pub mod message;
/// Port of vendored libmpack (`src/mpack/{mpack_core,object,conv}.c`) — the
/// streaming msgpack token reader + parser node-stack driving `decode.c`'s
/// `unpack_typval`/`msgpackparse`.
pub mod mpack;
/// Port of `src/nvim/ops.c` (subset: the yank-register store for `getreg`/`setreg`).
pub mod ops;
/// Port of `src/nvim/option.c` (subset: the option table, `&opt`, `:set`).
pub mod option;
/// Port of `src/nvim/option.c` OptVal layer (`OptVal`/`OptValType`,
/// `get_option_value`/`set_option_value`, `tv_to_optval`) alongside `option`.
pub mod option_optval;
/// Port of `src/nvim/os/` (subset: `os/time.c`'s `os_hrtime`).
pub mod os;
/// Port of `src/nvim/path.c` (subset: the path-component helpers behind
/// `pathshorten()`).
pub mod path;
/// Port of `src/nvim/plines.c`+`indent.c` (subset: getvcol/win_chartabsize/tabstop_padding).
pub mod plines;
/// Port of `src/nvim/profile.c` (the `proftime_T` helpers backing `reltime()`).
pub mod profile;
/// Port of `src/nvim/search.c` (subset: searchit/do_searchpair reference).
pub mod search;
/// Port of `src/nvim/sha256.c` (FIPS-180-2 SHA-256, behind `sha256()`).
pub mod sha256;
pub mod strings;
/// Generated not-yet-ported surface: one stub per vendored C function
/// definition (real name + `vendor/<file>:<line>` citation). Regenerate with
/// `scripts/gen_port_stubs.sh`; ported functions drop out automatically.
pub mod stubs;
/// Port of `src/nvim/window.c` (subset: the `win_T`/`tabpage_T` model + window
/// list behind `win_id2win`/`win_findbuf`/`getwinvar`/…).
pub mod window;
