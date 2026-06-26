//! Port of `src/nvim/eval/fs.c` (vendored at `csrc/eval/fs.c`).
//!
//! Filesystem-related Vimscript builtins. Only the pure path-string builtins are
//! ported here; the ones that touch the filesystem or editor state are stubbed.
#![allow(non_snake_case)]

use crate::ported::eval::typval::{tv_get_number, tv_get_string_chk};
use crate::ported::eval::typval_defs_h::{typval_T, typval_vval_union::*, VarType::*};
use crate::ported::path::shorten_dir_len;

/// Port of `f_pathshorten()` from `Src/eval/fs.c` (`pathshorten()`).
///
/// "pathshorten({path} [, {len}])" — shorten directory names in a path to `len`
/// characters each (default 1), keeping the final component.
pub fn f_pathshorten(argvars: &[typval_T], rettv: &mut typval_T) {
    // c: trim_len = (argvars[1] == UNKNOWN) ? 1 : max(1, tv_get_number(&argvars[1]));
    let mut trim_len = 1;
    if argvars.len() > 1 {
        trim_len = tv_get_number(&argvars[1]) as i32;
        if trim_len < 1 {
            trim_len = 1;
        }
    }
    rettv.v_type = VAR_STRING;
    // c: p = tv_get_string_chk(&argvars[0]); if (p == NULL) v_string = NULL;
    //    else { v_string = xstrdup(p); shorten_dir_len(v_string, trim_len); }
    match tv_get_string_chk(&argvars[0]) {
        Some(p) => rettv.vval = v_string(shorten_dir_len(&p, trim_len)),
        None => rettv.vval = v_string(String::new()),
    }
}
