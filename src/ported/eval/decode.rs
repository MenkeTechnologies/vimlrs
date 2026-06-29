//! Port of `src/nvim/eval/decode.c` (vendored at `csrc/eval/decode.c`).
//!
//! The JSON decode entry point. C `decode.c` parses with an explicit
//! value/container stack machine; the equivalent JSON grammar is implemented by
//! the [`crate::viml_json`] carve-out (recursive descent, identical `typval_T`
//! result), the way the eval engine delegates regex to `viml_regex` — see that
//! module's header.

use crate::ported::eval::typval_defs_h::typval_T;

/// Port of `typval_parser_error_free()` from `Src/eval/decode.c:1016` — free an
/// mpack parser's pending error state. RUST-PORT NOTE: JSON parsing here is
/// [`json_decode_string`] (a Rust `Result`, not the C `mpack_parser_t`), so
/// there is no C parser error struct to free → no-op.
pub fn typval_parser_error_free() {}

/// Port of `json_decode_string()` from `Src/eval/decode.c:619` — parse a JSON
/// document into a value. Returns `None` on malformed input (the C path sets an
/// error and returns FAIL).
pub fn json_decode_string(buf: &str) -> Option<typval_T> {
    crate::viml_json::decode(buf)
}
