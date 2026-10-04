//! Port of `src/nvim/ex_session.c` (subset, `vendor/ex_session.c`): the part
//! of `:mksession` the Vimscript engine owns — the global variables saved
//! under 'sessionoptions' "globals". An embedding editor writes the rest of
//! the session file and asks for these lines.

use crate::ported::eval::typval::{tv_get_float, tv_get_string};
use crate::ported::eval::typval_defs_h::VarType;
use crate::ported::eval::var_flavour_T::VAR_FLAVOUR_SESSION;

/// Port of `store_session_globals()` (ex_session.c:534): a `let` line for
/// each Number, String and Float global whose name has the session flavour
/// (starts with an uppercase letter and holds a lowercase one). The C writes
/// to the session file; this returns the lines in `g:`'s iteration order.
pub fn store_session_globals() -> Vec<String> {
    let mut lines = Vec::new();
    crate::ported::eval::vars::globvardict.with(|d| {
        for (key, tv) in d.borrow().dv_hashtab.iter() {
            if crate::ported::eval::var_flavour(key) != VAR_FLAVOUR_SESSION {
                continue;
            }
            match tv.v_type {
                VarType::VAR_NUMBER | VarType::VAR_STRING => {
                    // c:540-547 `vim_strsave_escaped(…, "\\\"\n\r")`, then a LF
                    // and CR become `\n` and `\r`.
                    let mut p = String::new();
                    for c in tv_get_string(tv).chars() {
                        match c {
                            '\\' | '"' => {
                                p.push('\\');
                                p.push(c);
                            }
                            '\n' => p.push_str("\\n"),
                            '\r' => p.push_str("\\r"),
                            c => p.push(c),
                        }
                    }
                    // c:548-552: a String is quoted, a Number padded with spaces.
                    let q = if tv.v_type == VarType::VAR_STRING { '"' } else { ' ' };
                    lines.push(format!("let {key} = {q}{p}{q}"));
                }
                VarType::VAR_FLOAT => {
                    // c:567-575 `"let %s = %c%f"`, the sign split off.
                    let f = tv_get_float(tv);
                    let sign = if f < 0.0 { '-' } else { ' ' };
                    lines.push(format!("let {key} = {sign}{:.6}", f.abs()));
                }
                _ => {}
            }
        }
    });
    lines
}
