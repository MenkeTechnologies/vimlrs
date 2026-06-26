//! Port of the register subsystem from `src/nvim/ops.c` (the `y_regs` yank-register
//! store + `get_reg_contents`/`write_reg_contents`/`get_reg_type`).
//!
//! RUST-PORT NOTE: `ops.c` is NOT vendored under `csrc/`; this is the editor-layer
//! carve-out backing the `funcs.c` register builtins — an in-memory register
//! store (the standalone interpreter has no buffer/yank machinery). The register
//! names and motion types match Vim; the function names match the `ops.c` symbols
//! the builtins call (`get_reg_contents`, `get_reg_type`, …).
#![allow(non_snake_case, non_upper_case_globals)]

use std::cell::RefCell;
use std::collections::HashMap;

/// `MotionType` (`ops_defs.h`) — how a register's text is pasted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionType {
    /// `kMTCharWise`.
    CharWise,
    /// `kMTLineWise`.
    LineWise,
    /// `kMTBlockWise` (with its block width).
    BlockWise(i32),
}

/// One yank register: its lines and motion type (`yankreg_T` subset).
struct Register {
    y_array: Vec<String>,
    y_type: MotionType,
}

thread_local! {
    /// `static yankreg_T y_regs[NUM_REGISTERS];` — the register store, keyed by
    /// register name (`"`, `a`-`z`, `0`-`9`, …).
    static Y_REGS: RefCell<HashMap<char, Register>> = RefCell::new(HashMap::new());
}

/// Port of `get_yank_type()` (ops.c) — parse a register-type option char into a
/// [`MotionType`] (+ block width). Returns `None` for an unrecognized char.
pub fn get_yank_type(c: u8, width: i32) -> Option<MotionType> {
    match c {
        b'c' | b'v' => Some(MotionType::CharWise),
        b'l' | b'V' => Some(MotionType::LineWise),
        b'b' | 0x16 => Some(MotionType::BlockWise(width)), // 0x16 = CTRL-V
        _ => None,
    }
}

/// Port of `write_reg_contents_lst()` (ops.c) — store `lines` into register
/// `name` with motion type `mtype`, appending when `append`.
pub fn write_reg_contents_lst(name: char, mut lines: Vec<String>, mtype: MotionType, append: bool) {
    Y_REGS.with(|r| {
        let mut regs = r.borrow_mut();
        if append {
            if let Some(reg) = regs.get_mut(&name) {
                // c: a charwise register continues on its last line (no newline
                // inserted); a linewise register gains new lines.
                if reg.y_type == MotionType::CharWise && !reg.y_array.is_empty() && !lines.is_empty() {
                    let head = lines.remove(0);
                    reg.y_array.last_mut().unwrap().push_str(&head);
                }
                reg.y_array.extend(lines);
                reg.y_type = mtype;
                return;
            }
        }
        regs.insert(name, Register { y_array: lines, y_type: mtype });
    });
}

/// Store a String value into a register (charwise unless a type is given), the
/// `write_reg_contents()` string path. The string's embedded `\n`s split lines.
pub fn write_reg_contents(name: char, value: &str, mtype: MotionType, append: bool) {
    let lines: Vec<String> = value.split('\n').map(str::to_string).collect();
    write_reg_contents_lst(name, lines, mtype, append);
}

/// Port of `get_reg_type()` (ops.c) — the motion type of register `name` (and a
/// block width). Absent register → CharWise.
pub fn get_reg_type(name: char) -> (MotionType, i32) {
    Y_REGS.with(|r| match r.borrow().get(&name).map(|reg| reg.y_type) {
        Some(MotionType::BlockWise(w)) => (MotionType::BlockWise(w), w),
        Some(t) => (t, 0),
        None => (MotionType::CharWise, 0),
    })
}

/// Port of `format_reg_type()` (ops.c) — the `getregtype()` code: `v` charwise,
/// `V` linewise, `<C-V>{width}` blockwise.
pub fn format_reg_type(mtype: MotionType, reglen: i32) -> String {
    match mtype {
        MotionType::CharWise => "v".to_string(),
        MotionType::LineWise => "V".to_string(),
        MotionType::BlockWise(_) => format!("\u{16}{}", reglen + 1),
    }
}

/// Port of `get_reg_contents()` (ops.c) — the register's lines, or `None` when
/// the register is unset (the C returns NULL). The caller forms the string
/// (lines joined by `\n`, with a trailing `\n` for a linewise register) or list.
pub fn get_reg_contents(name: char) -> Option<Vec<String>> {
    Y_REGS.with(|r| r.borrow().get(&name).map(|reg| reg.y_array.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charwise_and_linewise() {
        write_reg_contents('a', "hello", MotionType::CharWise, false);
        assert_eq!(get_reg_contents('a'), Some(vec!["hello".to_string()]));
        assert_eq!(format_reg_type(get_reg_type('a').0, 0), "v");
        assert_eq!(get_reg_contents('q'), None); // unset register

        write_reg_contents_lst('b', vec!["x".into(), "y".into()], MotionType::LineWise, false);
        assert_eq!(get_reg_contents('b'), Some(vec!["x".to_string(), "y".to_string()]));
        assert_eq!(format_reg_type(get_reg_type('b').0, 0), "V");

        // append (linewise adds lines)
        write_reg_contents_lst('b', vec!["z".into()], MotionType::LineWise, true);
        assert_eq!(get_reg_contents('b'), Some(vec!["x".to_string(), "y".to_string(), "z".to_string()]));
    }
}
