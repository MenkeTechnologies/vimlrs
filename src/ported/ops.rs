//! Port of the yank-register store from `csrc/ops.c` — the `y_regs[NUM_REGISTERS]`
//! register array plus the get/set paths behind `:let @r`, `getreg()`,
//! `setreg()` and `getregtype()`.
//!
//! RUST-PORT NOTE: in current Neovim this code lives in `src/nvim/register.c`
//! (+ `register.h` inline helpers, `register_defs.h` enums) after the
//! ops.c/register.c split, and `MotionType` comes from `normal_defs.h`; it is
//! vendored under the historical `csrc/ops.c` name. This is the editor-layer
//! carve-out backing the `funcs.c` register builtins — a faithful in-memory
//! `yankreg_T` store (the standalone interpreter has no buffer/yank machinery).
//! Clipboard (`get_clipboard`/`set_clipboard`), the `'/'`/`'='`/`'#'` special
//! sinks (search pattern / expr register / altfile) and `get_spec_reg` special
//! registers have no substrate here and are honest-stubbed — `'*'`/`'+'` degrade
//! to plain in-memory registers because no UI clipboard provider exists.
#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]

use std::cell::{Cell, RefCell};

/// `colnr_T` (`pos_defs.h`) — a column/width number.
type colnr_T = i32;

// ── register_defs.h — register index layout ─────────────────────────────────
//
// c:      0 = register for latest (unnamed) yank
// c:   1..9 = registers '1' to '9', for deletes
// c: 10..35 = registers 'a' to 'z'
// c:     36 = delete register '-'
// c:     37 = selection register '*'
// c:     38 = clipboard register '+'
const DELETION_REGISTER: usize = 36;
#[allow(dead_code)]
const NUM_SAVED_REGISTERS: usize = 37;
const STAR_REGISTER: usize = 37;
const PLUS_REGISTER: usize = 38;
const NUM_REGISTERS: usize = 39;

/// `MotionType` (`normal_defs.h`) — how a register's text is pasted.
///
/// RUST-PORT NOTE: C's `MotionType` is a plain enum (`kMTCharWise`/`kMTLineWise`/
/// `kMTBlockWise`/`kMTUnknown`) with the block width carried separately in
/// `yankreg_T.y_width`. This carve-out folds the width into `BlockWise` because
/// that is the shape the `funcs.c` builtin bridge already consumes; the stored
/// `yankreg_T` keeps a separate `y_width` field mirroring C. `kMTUnknown` (the
/// auto-detect sentinel and the absent-register `get_reg_type` return) is
/// represented as `None` in the `Option<MotionType>` params/returns below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionType {
    /// `kMTCharWise`.
    CharWise,
    /// `kMTLineWise`.
    LineWise,
    /// `kMTBlockWise` (with its block width).
    BlockWise(colnr_T),
}

/// `yreg_mode_t` (`register_defs.h`) — modes for [`get_yank_register`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum yreg_mode_t {
    YREG_PASTE,
    YREG_YANK,
    YREG_PUT,
}
use yreg_mode_t::*;

/// `yankreg_T` (`register_defs.h`) — definition of one register.
///
/// RUST-PORT NOTE: `y_array` is `Option<Vec<String>>` — `None` mirrors the C
/// `NULL` "empty register" sentinel (distinct from a register holding one empty
/// line). `y_size` is kept in lockstep with `y_array`'s length to mirror the C
/// field set. `additional_data` (ShaDa) has no analog here and is dropped.
struct yankreg_T {
    /// Pointer to an array of Strings (`NULL` → `None`).
    y_array: Option<Vec<String>>,
    /// Number of lines in y_array.
    y_size: usize,
    /// Register type.
    y_type: MotionType,
    /// Register width (only valid for y_type == kMTBlockWise).
    y_width: colnr_T,
    /// Time when register was last modified.
    timestamp: i64,
}

impl Default for yankreg_T {
    fn default() -> Self {
        // c: `static yankreg_T y_regs[NUM_REGISTERS] = { 0 };` — zero-init means
        // NULL array, size 0 and y_type == kMTCharWise (0).
        yankreg_T {
            y_array: None,
            y_size: 0,
            y_type: MotionType::CharWise,
            y_width: 0,
            timestamp: 0,
        }
    }
}

thread_local! {
    /// `static yankreg_T y_regs[NUM_REGISTERS] = { 0 };`
    static Y_REGS: RefCell<Vec<yankreg_T>> =
        RefCell::new((0..NUM_REGISTERS).map(|_| yankreg_T::default()).collect());
    /// `static yankreg_T *y_previous = NULL;` — ptr to last written yankreg,
    /// stored as an index into `y_regs` (`None` == NULL).
    static Y_PREVIOUS: Cell<Option<usize>> = const { Cell::new(None) };
}

// ── register.h inline helpers ───────────────────────────────────────────────

/// Port of `op_reg_index()` from csrc/ops.c (register.h) — register name → index.
///
/// @return Index in y_regs array or -1 if register name was not recognized.
fn op_reg_index(regname: i32) -> i32 {
    if regname > 0 && (regname as u8 as char).is_ascii_digit() {
        regname - '0' as i32 // c: ascii_isdigit
    } else if regname > 0 && (regname as u8 as char).is_ascii_lowercase() {
        (regname - 'a' as i32) + 10 // c: CHAR_ORD_LOW(regname) + 10
    } else if regname > 0 && (regname as u8 as char).is_ascii_uppercase() {
        (regname - 'A' as i32) + 10 // c: CHAR_ORD_UP(regname) + 10
    } else if regname == '-' as i32 {
        DELETION_REGISTER as i32
    } else if regname == '*' as i32 {
        STAR_REGISTER as i32
    } else if regname == '+' as i32 {
        PLUS_REGISTER as i32
    } else {
        -1
    }
}

/// Port of `is_append_register()` from csrc/ops.c (register.h).
fn is_append_register(regname: i32) -> bool {
    regname > 0 && (regname as u8 as char).is_ascii_uppercase()
}

/// Port of `get_register_name()` from csrc/ops.c (register.h) — the character
/// name of the register with the given number.
#[allow(dead_code)]
fn get_register_name(num: i32) -> i32 {
    if num == -1 {
        '"' as i32
    } else if num < 10 {
        num + '0' as i32
    } else if num == DELETION_REGISTER as i32 {
        '-' as i32
    } else if num == STAR_REGISTER as i32 {
        '*' as i32
    } else if num == PLUS_REGISTER as i32 {
        '+' as i32
    } else {
        num + 'a' as i32 - 10
    }
}

/// Port of `reg_empty()` from csrc/ops.c (register.h) — whether register is empty.
#[allow(dead_code)]
fn reg_empty(reg: &yankreg_T) -> bool {
    reg.y_array.is_none()
        || reg.y_size == 0
        || (reg.y_size == 1
            && reg.y_type == MotionType::CharWise
            && reg
                .y_array
                .as_ref()
                .map(|a| a[0].is_empty())
                .unwrap_or(true))
}

// ── register.c ──────────────────────────────────────────────────────────────

/// Port of `get_y_register()` from csrc/ops.c — the register at index `reg`
/// (C returns `&y_regs[reg]`).
///
/// RUST-PORT NOTE: the C pointer into `y_regs` is represented by the register's
/// `y_regs` index throughout this carve-out (see [`get_yank_register`]), so this
/// accessor is the identity on the index. Kept for surface fidelity; the
/// standalone bridge has no ShaDa/API callers that consume it.
#[allow(dead_code)]
fn get_y_register(reg: usize) -> usize {
    reg // c: return &y_regs[reg];
}

/// Port of `get_y_previous()` from csrc/ops.c — the last written register index.
#[allow(dead_code)]
fn get_y_previous() -> Option<usize> {
    Y_PREVIOUS.with(|p| p.get())
}

/// Port of `valid_yank_reg()` from csrc/ops.c — whether `regname` names a valid
/// yank register. `writing` allows only writable registers.
///
/// @note There is no check for 0 (default register); caller must do that. The
/// black hole register '_' is regarded as valid.
fn valid_yank_reg(regname: i32, writing: bool) -> bool {
    (regname > 0 && regname < 256 && (regname as u8 as char).is_ascii_alphanumeric())
        || (!writing && b"/#.%:=".contains(&(regname as u8)) && regname > 0 && regname < 256)
        || regname == '"' as i32
        || regname == '-' as i32
        || regname == '_' as i32
        || regname == '*' as i32
        || regname == '+' as i32
}

/// Port of `op_reg_get()` from csrc/ops.c — the register with name `name`, or
/// `None` when the name has no `y_regs` slot (C returns NULL).
///
/// RUST-PORT NOTE: C returns `const yankreg_T *`; this returns the resolved
/// `y_regs` index (this carve-out's pointer representation, see
/// [`get_yank_register`]). Unlike [`get_yank_register`], it does not fall back to
/// register 0 — an unrecognized name yields `None`, mirroring the C `NULL`.
#[allow(dead_code)]
fn op_reg_get(name: char) -> Option<usize> {
    let i = op_reg_index(name as i32); // c: int i = op_reg_index(name);
    if i == -1 {
        return None; // c: return NULL;
    }
    Some(i as usize) // c: return &y_regs[i];
}

/// Port of `get_yank_register()` from csrc/ops.c — resolve `regname`+`mode` to a
/// `y_regs` index (mirrors returning `&y_regs[i]`), updating `y_previous` on
/// yank.
///
/// RUST-PORT NOTE: `get_clipboard` is unavailable (no UI provider) so it is
/// treated as always returning false; the `YREG_PUT` empty-`'*'`/`'+'`
/// `empty_reg` fast-path is therefore skipped and those registers fall through
/// to their normal in-memory index.
fn get_yank_register(regname: i32, mode: yreg_mode_t) -> usize {
    // c: get_clipboard(...) is a no-op here → skip the clipboard fast-paths.
    if mode != YREG_YANK
        && (regname == 0 || regname == '"' as i32 || regname == '*' as i32 || regname == '+' as i32)
    {
        if let Some(prev) = Y_PREVIOUS.with(|p| p.get()) {
            // in case clipboard not available, paste from previous used register
            return prev;
        }
    }

    let mut i = op_reg_index(regname);
    // when not 0-9, a-z, A-Z or '-'/'+'/'*': use register 0
    if i == -1 {
        i = 0;
    }
    let i = i as usize;

    if mode == YREG_YANK {
        // remember the written register for unnamed paste
        Y_PREVIOUS.with(|p| p.set(Some(i)));
    }
    i
}

/// Port of `free_register()` from csrc/ops.c — clear a register's contents.
fn free_register(reg: &mut yankreg_T) {
    // c: XFREE_CLEAR(reg->y_array); (additional_data has no analog here)
    reg.y_array = None;
    reg.y_size = 0;
}

/// Port of `format_reg_type()` from csrc/ops.c — the `getregtype()` code:
/// `v` charwise, `V` linewise, `<C-V>{width}` blockwise, `""` for unknown.
///
/// RUST-PORT NOTE: C writes into `buf` and returns the length; here we return the
/// `String` directly. `reg_width` maps to the [`MotionType::BlockWise`] payload,
/// with the caller-supplied `reg_width` used as the width so both the folded and
/// the separate widths agree.
pub fn format_reg_type(reg_type: MotionType, reg_width: colnr_T) -> String {
    match reg_type {
        MotionType::LineWise => "V".to_string(),
        MotionType::CharWise => "v".to_string(),
        // c: CTRL_V_STR "%" PRIdCOLNR, reg_width + 1  (CTRL_V_STR == "\x16")
        MotionType::BlockWise(_) => format!("\u{16}{}", reg_width + 1),
    }
}

/// Port of `get_reg_type()` from csrc/ops.c — the motion type of register
/// `regname` (and, for a blockwise register, its width).
///
/// RUST-PORT NOTE: C returns `kMTUnknown` for the special/absent-register cases;
/// this carve-out returns `(CharWise, 0)` to preserve the existing builtin-bridge
/// contract (`getregtype()` of an absent register yields `"v"` in vimlrs). The
/// special read-only registers (`%`, `#`, `=`, `:`, `/`, `.`, CTRL-{F,P,W,A},
/// `_`) all report charwise, matching C.
pub fn get_reg_type(regname: char) -> (MotionType, colnr_T) {
    let regname = regname as i32;
    match regname {
        // c: '%' '#' '=' ':' '/' '.' Ctrl_F Ctrl_P Ctrl_W Ctrl_A '_' → kMTCharWise
        0x25 | 0x23 | 0x3d | 0x3a | 0x2f | 0x2e | 0x06 | 0x10 | 0x17 | 0x01 | 0x5f => {
            return (MotionType::CharWise, 0);
        }
        _ => {}
    }

    if regname != 0 && !valid_yank_reg(regname, false) {
        return (MotionType::CharWise, 0); // c: return kMTUnknown;
    }

    let i = get_yank_register(regname, YREG_PASTE);
    Y_REGS.with(|r| {
        let regs = r.borrow();
        let reg = &regs[i];
        if reg.y_array.is_some() {
            if reg.y_type == MotionType::BlockWise(reg.y_width) {
                return (reg.y_type, reg.y_width);
            }
            return (reg.y_type, 0);
        }
        (MotionType::CharWise, 0) // c: return kMTUnknown;
    })
}

/// Port of `get_reg_wrap_one_line()` from csrc/ops.c — wrap the single line `s`
/// for [`get_reg_contents`].
///
/// RUST-PORT NOTE: C returns `void *` — either the bare `char *s` (no `kGRegList`)
/// or a one-element `list_T`. This carve-out always yields the list shape (the
/// `kGRegList` branch): [`get_reg_contents`] returns the register's lines and the
/// `funcs.c` bridge forms the joined string itself, so the bare-string branch has
/// no consumer here.
fn get_reg_wrap_one_line(s: &str) -> Vec<String> {
    // c: list_T *list = tv_list_alloc(1); tv_list_append_allocated_string(list, s);
    vec![s.to_string()]
}

/// Port of `get_reg_contents()` from csrc/ops.c — the lines held by register
/// `regname`, or `None` when it is unset (C returns NULL).
///
/// RUST-PORT NOTE: this returns the `kGRegList` shape (the register's lines); the
/// `funcs.c` builtins (`f_getreg`/`f_getreginfo`) form the joined string / list
/// themselves. The `'='` expression register and `get_spec_reg` special
/// registers have no substrate here and yield `None`.
pub fn get_reg_contents(regname: char) -> Option<Vec<String>> {
    let mut regname = regname as i32;

    // c: Don't allow using an expression register inside an expression; the
    // expr register has no substrate here → NULL.
    if regname == '=' as i32 {
        return None;
    }

    if regname == '@' as i32 {
        // "@@" is used for unnamed register
        regname = '"' as i32;
    }

    // check for valid regname
    if regname != 0 && !valid_yank_reg(regname, false) {
        return None;
    }

    // c: get_spec_reg(regname, &retval, &allocated, false) — only the
    // substrate-free black hole is ported; the file/altfile/expr/cmdline/search/
    // insert/cursor special registers need buffer machinery absent here, so they
    // are honest-stubbed (treated as "not a special register").
    if regname == '_' as i32 {
        // c: case '_': *argp = ""; return true; → get_reg_wrap_one_line("").
        return Some(get_reg_wrap_one_line(""));
    }

    let i = get_yank_register(regname, YREG_PUT);
    Y_REGS.with(|r| r.borrow()[i].y_array.clone())
}

/// Port of `init_write_reg()` from csrc/ops.c — validate `name`, snapshot
/// `y_previous`, resolve the target register index and (unless appending) clear
/// it. Returns `(reg index, old_y_previous)` or `None` on an invalid name.
fn init_write_reg(name: i32, must_append: bool) -> Option<(usize, Option<usize>)> {
    if !valid_yank_reg(name, true) {
        // c: emsg_invreg(name); — reported by the builtin bridge, not here.
        return None;
    }

    // Don't want to change the current (unnamed) register.
    let old_y_previous = Y_PREVIOUS.with(|p| p.get());

    let reg = get_yank_register(name, YREG_YANK);
    if !is_append_register(name) && !must_append {
        Y_REGS.with(|r| free_register(&mut r.borrow_mut()[reg]));
    }
    Some((reg, old_y_previous))
}

/// Input to [`str_to_reg`]: a byte string (`const char *str` + `len`) or a list
/// of lines (`char **strings`). RUST-PORT NOTE: this replaces the C
/// `const char *str` + `bool str_list` union.
enum reg_input<'a> {
    Str(&'a [u8]),
    List(&'a [String]),
}

/// Port of `str_to_reg()` from csrc/ops.c — put a string (or list of strings)
/// into register `y_ptr`, appending when the register already holds charwise
/// text. `yank_type == None` mirrors `kMTUnknown` (auto-detect).
fn str_to_reg(
    y_ptr: &mut yankreg_T,
    yank_type: Option<MotionType>,
    input: reg_input,
    blocklen: colnr_T,
) {
    // c: `bool str_list` is represented by the `reg_input` variant below.
    if y_ptr.y_array.is_none() {
        // NULL means empty register
        y_ptr.y_size = 0;
    }

    // c: if (yank_type == kMTUnknown) auto-detect from a trailing NL/CAR.
    let mut yank_type = match yank_type {
        Some(t) => t,
        None => {
            let linewise = match &input {
                reg_input::List(_) => true,
                reg_input::Str(s) => {
                    let len = s.len();
                    len > 0 && (s[len - 1] == b'\n' || s[len - 1] == b'\r')
                }
            };
            if linewise {
                MotionType::LineWise
            } else {
                MotionType::CharWise
            }
        }
    };
    let is_block = matches!(yank_type, MotionType::BlockWise(_));

    let mut extraline = false; // extra line at the end
    let mut append = false; // append to last line in register

    // Count the number of lines within the string (C computes `newlines` to size
    // the array up-front; the Rust Vec grows on demand, so we only need the
    // `extraline`/`append` flags the count loop also sets).
    match &input {
        reg_input::List(_) => {}
        reg_input::Str(s) => {
            let len = s.len();
            if yank_type == MotionType::CharWise || len == 0 || s[len - 1] != b'\n' {
                extraline = true; // count extra newline at the end
            }
            if y_ptr.y_size > 0 && y_ptr.y_type == MotionType::CharWise {
                append = true;
            }
        }
    }

    // Grow / take the register array; `lnum` == current line count == pp.len().
    let mut pp: Vec<String> = y_ptr.y_array.take().unwrap_or_default();
    let mut lnum = y_ptr.y_size;

    // If called with `blocklen < 0`, we have to update the yank reg's width.
    let mut maxlen: usize = 0;

    match input {
        reg_input::List(strings) => {
            for ss in strings {
                // c: pp[lnum] = cstr_to_string(*ss);
                pp.push(ss.clone());
                if is_block {
                    // RUST-PORT NOTE: mb_string2cells() (mbyte.c) approximated by
                    // char count — no display-cell metrics in the standalone.
                    let charlen = ss.chars().count();
                    maxlen = maxlen.max(charlen);
                }
                lnum += 1;
            }
        }
        reg_input::Str(s) => {
            let end = s.len();
            let mut start = 0usize;
            // c: for (start=str; start < end + extraline; start += line_len+1)
            while start < end + extraline as usize {
                let mut charlen: i32 = 0;
                // find the end of the line
                let mut line_end = start;
                while line_end < end {
                    if s[line_end] == b'\n' {
                        break;
                    }
                    if is_block {
                        // RUST-PORT NOTE: utf_ptr2cells_len() approximated as 1
                        // cell per char (no display-cell metrics here).
                        charlen += 1;
                    }
                    if s[line_end] == 0 {
                        line_end += 1; // registers can have NUL chars
                    } else {
                        // c: utf_ptr2len_len() — advance one UTF-8 char.
                        // RUST-PORT NOTE: mbyte.c helper inlined as a UTF-8
                        // lead-byte length decode, clamped to the remaining bytes.
                        let b = s[line_end];
                        let n = if b < 0x80 {
                            1
                        } else if b >> 5 == 0b110 {
                            2
                        } else if b >> 4 == 0b1110 {
                            3
                        } else if b >> 3 == 0b11110 {
                            4
                        } else {
                            1
                        };
                        line_end += n.min(end - line_end).max(1);
                    }
                }
                maxlen = maxlen.max(charlen as usize);

                // When appending, copy the previous line and free it after.
                let mut buf: Vec<u8> = if append {
                    lnum -= 1;
                    std::mem::take(&mut pp[lnum]).into_bytes()
                } else {
                    Vec::new()
                };
                buf.extend_from_slice(&s[start..line_end]);
                // Convert NULs to '\n' to prevent truncation.
                for b in buf.iter_mut() {
                    if *b == 0 {
                        *b = b'\n';
                    }
                }
                let line = String::from_utf8(buf)
                    .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
                if append {
                    pp[lnum] = line;
                    append = false; // only first line is appended
                } else {
                    pp.push(line);
                }
                lnum += 1;
                start = line_end + 1; // c: start += line_len + 1
            }
        }
    }

    // Without any lines make the register empty.
    if lnum == 0 {
        y_ptr.y_array = None;
        y_ptr.y_size = 0;
        return;
    }

    if is_block {
        // c: y_width = (blocklen == -1 ? maxlen - 1 : blocklen);
        y_ptr.y_width = if blocklen == -1 {
            maxlen as colnr_T - 1
        } else {
            blocklen
        };
        yank_type = MotionType::BlockWise(y_ptr.y_width);
    } else {
        y_ptr.y_width = 0;
    }
    y_ptr.y_type = yank_type;
    y_ptr.y_size = lnum;
    // c: y_ptr->timestamp = os_time();  (RUST-PORT NOTE: os_time() inlined.)
    y_ptr.timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    y_ptr.y_array = Some(pp);
}

/// Port of `finish_write_reg()` from csrc/ops.c — restore `y_previous` for a
/// non-`"` register.
///
/// RUST-PORT NOTE: `set_clipboard(name, reg)` is a no-op (no UI provider).
fn finish_write_reg(name: i32, old_y_previous: Option<usize>) {
    // ':let @" = "val"' should change the meaning of the "" register
    if name != '"' as i32 {
        Y_PREVIOUS.with(|p| p.set(old_y_previous));
    }
}

/// Port of `write_reg_contents_ex()` from csrc/ops.c — store `str` in register
/// `name`. `yank_type == None` mirrors `kMTUnknown` (auto-detect from a trailing
/// NL/CAR).
///
/// RUST-PORT NOTE: the `'/'` (search pattern), `'#'` (altfile) and `'='` (expr
/// register) sinks have no substrate in the standalone interpreter and are
/// honest-stubbed (no-op).
fn write_reg_contents_ex(
    name: i32,
    str: &[u8],
    must_append: bool,
    yank_type: Option<MotionType>,
    block_len: colnr_T,
) {
    // c: '/' set_last_search_pat, '#' altfile, '=' expr_line — no substrate here.
    if name == '/' as i32 || name == '#' as i32 || name == '=' as i32 {
        return;
    }

    if name == '_' as i32 {
        // black hole: nothing to do
        return;
    }

    let (reg, old_y_previous) = match init_write_reg(name, must_append) {
        Some(v) => v,
        None => return,
    };
    Y_REGS.with(|r| {
        str_to_reg(
            &mut r.borrow_mut()[reg],
            yank_type,
            reg_input::Str(str),
            block_len,
        )
    });
    finish_write_reg(name, old_y_previous);
}

/// Port of `write_reg_contents()` from csrc/ops.c — store `value` in register
/// `name`.
///
/// RUST-PORT NOTE: the C `write_reg_contents(name, str, len, must_append)` passes
/// `kMTUnknown`; this carve-out threads the caller's [`MotionType`] through so
/// the builtin bridge can force a type (`Some(mtype)`), matching how the reduced
/// module behaved.
pub fn write_reg_contents(name: char, value: &str, mtype: MotionType, append: bool) {
    // Split the bridge MotionType into (type, block_len); the folded BlockWise
    // width is passed as `block_len` so both widths agree.
    let (yt, block_len) = match mtype {
        MotionType::BlockWise(w) => (Some(MotionType::BlockWise(w)), w),
        other => (Some(other), 0),
    };
    write_reg_contents_ex(name as i32, value.as_bytes(), append, yt, block_len);
}

/// Port of `write_reg_contents_lst()` from csrc/ops.c — store `lines` in register
/// `name`.
///
/// RUST-PORT NOTE: arg order/shape is adapted to the `funcs.c` bridge — the
/// motion type (`mtype`, block width folded in) comes before `append`, and the
/// lines are already split. The `'/'`/`'='`/`'#'` single-line special cases
/// route to [`write_reg_contents_ex`] (all no-ops here); the black-hole `'_'` is
/// a no-op.
pub fn write_reg_contents_lst(name: char, lines: Vec<String>, mtype: MotionType, append: bool) {
    let name = name as i32;
    let (yt, block_len) = match mtype {
        MotionType::BlockWise(w) => (Some(MotionType::BlockWise(w)), w),
        other => (Some(other), 0),
    };

    if name == '/' as i32 || name == '=' as i32 || name == '#' as i32 {
        // c: single-line-only registers; join is irrelevant since the sinks are
        // no-ops here.
        let s = lines.first().cloned().unwrap_or_default();
        write_reg_contents_ex(name, s.as_bytes(), append, yt, block_len);
        return;
    }

    // black hole: nothing to do
    if name == '_' as i32 {
        return;
    }

    let (reg, old_y_previous) = match init_write_reg(name, append) {
        Some(v) => v,
        None => return,
    };
    Y_REGS.with(|r| {
        str_to_reg(
            &mut r.borrow_mut()[reg],
            yt,
            reg_input::List(&lines),
            block_len,
        )
    });
    finish_write_reg(name, old_y_previous);
}

/// Port of `get_yank_type()` from `Src/eval/funcs.c` — parse a register-type
/// option char into a [`MotionType`] (+ block width). Returns `None` for an
/// unrecognized char.
///
/// RUST-PORT NOTE: the C `get_yank_type(char **pp, MotionType*, int*)` advances a
/// cursor and parses a trailing width; this reduced form takes the type char and
/// a width and returns the resolved [`MotionType`] for the builtin bridge.
pub fn get_yank_type(c: u8, width: colnr_T) -> Option<MotionType> {
    match c {
        b'v' | b'c' => Some(MotionType::CharWise),
        b'V' | b'l' => Some(MotionType::LineWise),
        b'b' | 0x16 => Some(MotionType::BlockWise(width)), // 0x16 = CTRL-V
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset() {
        Y_REGS
            .with(|r| *r.borrow_mut() = (0..NUM_REGISTERS).map(|_| yankreg_T::default()).collect());
        Y_PREVIOUS.with(|p| p.set(None));
    }

    #[test]
    fn charwise_and_linewise() {
        reset();
        write_reg_contents('a', "hello", MotionType::CharWise, false);
        assert_eq!(get_reg_contents('a'), Some(vec!["hello".to_string()]));
        assert_eq!(format_reg_type(get_reg_type('a').0, 0), "v");
        assert_eq!(get_reg_contents('q'), None); // unset register

        write_reg_contents_lst(
            'b',
            vec!["x".into(), "y".into()],
            MotionType::LineWise,
            false,
        );
        assert_eq!(
            get_reg_contents('b'),
            Some(vec!["x".to_string(), "y".to_string()])
        );
        assert_eq!(format_reg_type(get_reg_type('b').0, 0), "V");

        // append (linewise adds lines)
        write_reg_contents_lst('b', vec!["z".into()], MotionType::LineWise, true);
        assert_eq!(
            get_reg_contents('b'),
            Some(vec!["x".to_string(), "y".to_string(), "z".to_string()])
        );
    }

    /// `:let @a = 'x'` then `getreg('a')` must round-trip.
    #[test]
    fn let_register_roundtrip() {
        reset();
        // setreg('a', 'x') → charwise single line.
        write_reg_contents_lst('a', vec!["x".into()], MotionType::CharWise, false);
        assert_eq!(get_reg_contents('a'), Some(vec!["x".to_string()]));
        assert_eq!(get_reg_type('a').0, MotionType::CharWise);
    }

    /// Charwise append merges the first appended line onto the register's last
    /// line (str_to_reg string path), matching Vim.
    #[test]
    fn charwise_append_merges() {
        reset();
        write_reg_contents('a', "foo", MotionType::CharWise, false);
        write_reg_contents('a', "bar", MotionType::CharWise, true);
        assert_eq!(get_reg_contents('a'), Some(vec!["foobar".to_string()]));
    }

    /// A non-append write replaces the register (init_write_reg frees it first).
    #[test]
    fn non_append_replaces() {
        reset();
        write_reg_contents_lst(
            'c',
            vec!["one".into(), "two".into()],
            MotionType::LineWise,
            false,
        );
        write_reg_contents_lst('c', vec!["only".into()], MotionType::LineWise, false);
        assert_eq!(get_reg_contents('c'), Some(vec!["only".to_string()]));
    }

    /// Uppercase register name forces append even with `append == false`
    /// (is_append_register).
    #[test]
    fn uppercase_appends() {
        reset();
        write_reg_contents_lst('d', vec!["a".into()], MotionType::LineWise, false);
        // 'D' targets the same register index as 'd' and appends.
        write_reg_contents_lst('D', vec!["b".into()], MotionType::LineWise, false);
        assert_eq!(
            get_reg_contents('d'),
            Some(vec!["a".to_string(), "b".to_string()])
        );
    }

    /// Blockwise registers report `<C-V>{width}` via getregtype.
    #[test]
    fn blockwise_regtype() {
        reset();
        write_reg_contents_lst(
            'e',
            vec!["ab".into(), "cd".into()],
            MotionType::BlockWise(1),
            false,
        );
        let (t, w) = get_reg_type('e');
        assert_eq!(t, MotionType::BlockWise(1));
        assert_eq!(format_reg_type(t, w), "\u{16}2");
    }

    /// Black hole register discards writes; reads always yield "" (one empty
    /// line) via the ported `get_spec_reg` '_' case, never the written text.
    #[test]
    fn black_hole_discards() {
        reset();
        write_reg_contents_lst('_', vec!["gone".into()], MotionType::CharWise, false);
        assert_eq!(get_reg_contents('_'), Some(vec![String::new()]));
        // The write did not leak into register 0 (op_reg_index('_') == -1 → 0).
        assert_eq!(get_reg_contents('0'), None);
    }

    /// `op_reg_get` resolves a name to its `y_regs` index and rejects names with
    /// no slot (unlike `get_yank_register`, no fall-through to register 0).
    #[test]
    fn op_reg_get_index() {
        reset();
        assert_eq!(op_reg_get('a'), Some(10)); // 'a' → 10
        assert_eq!(op_reg_get('0'), Some(0));
        assert_eq!(op_reg_get('-'), Some(DELETION_REGISTER));
        assert_eq!(op_reg_get('*'), Some(STAR_REGISTER));
        assert_eq!(op_reg_get('"'), None); // no op_reg_index slot → NULL
        assert_eq!(op_reg_get('_'), None); // black hole is not in y_regs
                                           // get_y_register is the identity on the index (pointer→index deviation).
        assert_eq!(get_y_register(10), 10);
    }

    /// `get_reg_wrap_one_line` yields the one-element list shape, and the black
    /// hole read routes through it.
    #[test]
    fn wrap_one_line() {
        reset();
        assert_eq!(get_reg_wrap_one_line("x"), vec!["x".to_string()]);
        assert_eq!(get_reg_contents('_'), Some(vec![String::new()]));
    }

    /// `getreg('@')` and the unnamed register `"` are the same store.
    #[test]
    fn unnamed_alias() {
        reset();
        write_reg_contents_lst('"', vec!["u".into()], MotionType::CharWise, false);
        assert_eq!(get_reg_contents('@'), Some(vec!["u".to_string()]));
        assert_eq!(get_reg_contents('"'), Some(vec!["u".to_string()]));
    }
}
