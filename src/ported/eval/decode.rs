//! Port of `src/nvim/eval/decode.c` (vendored at `csrc/eval/decode.c`).
//!
//! The JSON decode entry point. C `decode.c` parses with an explicit
//! value/container stack machine; the equivalent JSON grammar is implemented by
//! the [`crate::viml_json`] carve-out (recursive descent, identical `typval_T`
//! result), the way the eval engine delegates regex to `viml_regex` — see that
//! module's header.

use crate::ported::eval::typval_defs_h::typval_T;

/// Port of `json_decode_string()` from `Src/eval/decode.c:619` — parse a JSON
/// document into a value. Returns `None` on malformed input (the C path sets an
/// error and returns FAIL).
pub fn json_decode_string(buf: &str) -> Option<typval_T> {
    crate::viml_json::decode(buf)
}
