//! Port of `Src/keycodes.c` — the `<Key>` notation subsystem.
//!
//! Two directions, both reachable from VimL:
//!
//! - `trans_special` — `"\<Esc>"` in a double-quoted string becomes the ESC
//!   character (`eval_string`, `eval.c:3512`, case `'<'`).
//! - `get_special_key_name` — `keytrans()` renders a character back as
//!   `<Esc>` / `<C-A>` / `<Space>`.
//!
//! ## What this port covers, and why the rest cannot be
//!
//! Vim's key codes come in two flavours. A key that *is* a character — `<Esc>`,
//! `<Tab>`, `<CR>`, `<Space>`, `<C-A>`, `<S-a>`, `<Char-65>` — is one byte in the
//! string, and that is what this module translates. A key that is not a
//! character — `<Up>`, `<F1>`, `<BS>`, `<Del>`, `<Nul>`, `<C-@>` — is encoded as a
//! `K_SPECIAL` (0x80) escape *sequence* of raw bytes that is not valid UTF-8, and
//! vimlrs stores strings as Rust `String` (UTF-8 text). Those sequences therefore
//! cannot be represented, and `trans_special` returns [`None`] for them, leaving
//! the source text literal — exactly what vimlrs did for *every* key before this
//! port. The same applies to the `<M-…>`/`<A-…>` meta forms, which Vim 9.2 and
//! Neovim 0.12 do not even agree on (`"\<M-a>"` is one byte `0xE1` in Vim and a
//! four-byte `K_SPECIAL` sequence in Neovim), so there is no single behavior to
//! port. See BUGS.md.
//!
//! `key_names_table` itself is a *generated* C table
//! (`keycode_names.generated.h`), so it is not quotable from `vendor/`; the
//! character-valued entries below were confirmed against both Vim 9.2 and
//! Neovim 0.12, which agree on all of them.

/// `MOD_MASK_*` (`keycodes.h`) — only the bits this port can act on.
const MOD_MASK_SHIFT: i32 = 0x02;
const MOD_MASK_CTRL: i32 = 0x04;

/// Port of `name_to_mod_mask()` (`keycodes.c:...`) — modifier letter → mask.
///
/// The C table also carries `M`/`T`/`A`/`D` and the multi-click bits; they are
/// deliberately absent here, because a key carrying one of those cannot be
/// reduced to a character (see the module docs) and must stay literal.
pub fn name_to_mod_mask(c: u8) -> i32 {
    match c.to_ascii_uppercase() {
        b'C' => MOD_MASK_CTRL,
        b'S' => MOD_MASK_SHIFT,
        _ => 0,
    }
}

/// Port of `get_special_key_code()` (`keycodes.c:608`) — key name → character.
///
/// The C looks the name up in the generated `key_names_table`; this is that
/// table's character-valued subset (names are matched case-insensitively, as the
/// C hash does). A name with no character value — `Up`, `F1`, `BS`, `Del`, `Nul`
/// — returns [`None`] rather than a `K_SPECIAL` code.
pub fn get_special_key_code(name: &str) -> Option<char> {
    Some(match name.to_ascii_lowercase().as_str() {
        "tab" => '\t',
        "nl" | "newline" | "linefeed" => '\n',
        "cr" | "return" | "enter" => '\r',
        "esc" => '\x1b',
        "space" => ' ',
        "lt" => '<',
        "bslash" => '\\',
        "bar" => '|',
        _ => return None,
    })
}

/// Port of `extract_modifiers()` (`keycodes.c`) — fold a modifier into the key
/// itself where Vim does.
///
/// Returns `None` when a modifier survives that this port cannot represent (a
/// CTRL on a key outside `?`..`_`/alpha, or a SHIFT on a non-letter): such a key
/// is a `K_SPECIAL` sequence in Vim.
fn extract_modifiers(mut key: char, modifiers: i32) -> Option<char> {
    let mut modifiers = modifiers;

    // c: `if ((modifiers & MOD_MASK_SHIFT) && ASCII_ISALPHA(key)) { key =
    // TOUPPER_ASC(key); if (!(modifiers & MOD_MASK_CTRL)) { modifiers &=
    // ~MOD_MASK_SHIFT; } }` — `<S-a>` is simply `A`.
    if modifiers & MOD_MASK_SHIFT != 0 && key.is_ascii_alphabetic() {
        key = key.to_ascii_uppercase();
        if modifiers & MOD_MASK_CTRL == 0 {
            modifiers &= !MOD_MASK_SHIFT;
        }
    }

    // c: `if (simplify && (modifiers & MOD_MASK_CTRL) && ((key >= '?' && key <=
    // '_') || ASCII_ISALPHA(key))) { key = CTRL_CHR(key); modifiers &=
    // ~MOD_MASK_CTRL; }` — `<C-A>` is 0x01, `<C-?>` is 0x7f.
    if modifiers & MOD_MASK_CTRL != 0 && ((('?'..='_').contains(&key)) || key.is_ascii_alphabetic())
    {
        // c: CTRL_CHR(x) == TOUPPER_ASC(x) ^ 0x40 (a macro in `ascii.h`, so it
        // stays an expression here rather than becoming an invented helper fn).
        let code = (key.to_ascii_uppercase() as u32) ^ 0x40;
        // c: `if (key == NUL) { key = K_ZERO; }` — `<C-@>` becomes the K_ZERO
        // *special* key, which has no character form; leave it literal.
        if code == 0 {
            return None;
        }
        key = char::from_u32(code)?;
        modifiers &= !MOD_MASK_CTRL;
    }

    // Anything still carrying a modifier is a K_SPECIAL key in Vim.
    (modifiers == 0).then_some(key)
}

/// Port of `find_special_key()` (`keycodes.c:...`) — parse the `<…>` at the start
/// of `src` and return the character it denotes, plus the byte length consumed
/// (including both angle brackets).
///
/// [`None`] means "not a key vimlrs can represent" — an unknown name, a
/// `K_SPECIAL` key, or a surviving modifier — and the caller must copy the text
/// through literally, which is what Vim's own callers do for an unmatched `<`.
pub fn find_special_key(src: &str) -> Option<(char, usize)> {
    let bytes = src.as_bytes();
    if bytes.first() != Some(&b'<') {
        return None;
    }
    // c: scan for the matching '>' across the modifier list and the key name.
    let close = src.find('>')?;
    let body = &src[1..close];
    if body.is_empty() {
        return None;
    }
    let consumed = close + 1;

    // c: `<Char-123>` / `<Char-0x7f>` — the key is spelled as a number.
    if let Some(num) = body
        .strip_prefix("Char-")
        .or_else(|| body.strip_prefix("char-"))
        .or_else(|| body.strip_prefix("CHAR-"))
    {
        let n = if let Some(hex) = num.strip_prefix("0x").or_else(|| num.strip_prefix("0X")) {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            num.parse::<u32>().ok()?
        };
        return char::from_u32(n).map(|c| (c, consumed));
    }

    // c: the modifier list is everything up to the LAST '-'; what follows is the
    // key (a single character, or a name from key_names_table).
    let mut modifiers = 0;
    let (mods, name) = match body.rfind('-') {
        Some(last_dash) => (&body[..last_dash], &body[last_dash + 1..]),
        None => ("", body),
    };
    for m in mods.bytes() {
        if m == b'-' {
            continue;
        }
        // c: `bit = name_to_mod_mask(*bp); if (bit == 0x0) break;` — an illegal
        // (or unrepresentable) modifier name means this is not our key.
        let bit = name_to_mod_mask(m);
        if bit == 0 {
            return None;
        }
        modifiers |= bit;
    }
    if name.is_empty() {
        return None;
    }

    // c: with a modifier present, a single-character name is that character
    // (`<C-a>`); otherwise the name goes through get_special_key_code (`<Esc>`).
    let mut chars = name.chars();
    let first = chars.next()?;
    let key = if modifiers != 0 && chars.next().is_none() {
        first
    } else {
        get_special_key_code(name)?
    };

    extract_modifiers(key, modifiers).map(|k| (k, consumed))
}

/// Port of `trans_special()` (`keycodes.c:364`) — translate the `<…>` key
/// notation at the start of `src` into the character it denotes.
///
/// Returns the character and the number of source bytes consumed, or [`None`]
/// when the text is not a representable key (the caller then copies it
/// literally). The C's `escape_ks` / `did_simplify` outputs are not modelled:
/// both concern `K_SPECIAL` byte sequences, which this port never produces.
pub fn trans_special(src: &str) -> Option<(char, usize)> {
    find_special_key(src)
}

/// Port of `get_special_key_name()` (`keycodes.c:263`) — the `<…>` notation for a
/// character, as `keytrans()` prints it.
///
/// Covers the inverse of `trans_special`: the table names, and the C0 controls
/// that `get_special_key_name` renders under the CTRL modifier (c: "if (table_idx
/// < 0 && !vim_isprintc(c) && c < ' ') { c += '@'; modifiers |= MOD_MASK_CTRL; }").
/// A character with no `<…>` form is returned as itself.
pub fn get_special_key_name(c: char) -> String {
    match c {
        ' ' => "<Space>".into(),
        '<' => "<lt>".into(),
        '\t' => "<Tab>".into(),
        '\n' => "<NL>".into(),
        '\r' => "<CR>".into(),
        '\x1b' => "<Esc>".into(),
        // c: c += '@'; modifiers |= MOD_MASK_CTRL  →  0x01 is <C-A>, 0x1f is <C-_>.
        c if (c as u32) < 0x20 => format!("<C-{}>", (c as u8 + b'@') as char),
        c => c.to_string(),
    }
}

/// `KS_EXTRA` (vim 9.2 `keymap.h:61`): the first termcap byte of a key that
/// has no two-character termcap name.
const KS_EXTRA: u8 = 253;
/// `KE_TAB` (vim 9.2 `keymap.h:215`): the unshifted TAB key, `K_TAB`.
const KE_TAB: u8 = 54;

/// The rows of `key_names_table[]` (vim 9.2 `misc2.c:921`) whose key is a
/// `TERMCAP2KEY(a, b)` code — the keys a `t_xx` option names — keyed by the
/// two termcap bytes, in the table's order. Alternative names (`is_alt`) are
/// left out: `find_special_key_in_table()` never returns one.
const key_names_table: &[([u8; 2], &str)] = &[
    (*b"kb", "BS"),
    (*b"kD", "Del"),
    (*b"kd", "Down"),
    (*b"@7", "End"),
    (*b"k1", "F1"),
    (*b"k;", "F10"),
    (*b"F1", "F11"),
    (*b"F2", "F12"),
    (*b"F3", "F13"),
    (*b"F4", "F14"),
    (*b"F5", "F15"),
    (*b"F6", "F16"),
    (*b"F7", "F17"),
    (*b"F8", "F18"),
    (*b"F9", "F19"),
    (*b"k2", "F2"),
    (*b"FA", "F20"),
    (*b"FB", "F21"),
    (*b"FC", "F22"),
    (*b"FD", "F23"),
    (*b"FE", "F24"),
    (*b"FF", "F25"),
    (*b"FG", "F26"),
    (*b"FH", "F27"),
    (*b"FI", "F28"),
    (*b"FJ", "F29"),
    (*b"k3", "F3"),
    (*b"FK", "F30"),
    (*b"FL", "F31"),
    (*b"FM", "F32"),
    (*b"FN", "F33"),
    (*b"FO", "F34"),
    (*b"FP", "F35"),
    (*b"FQ", "F36"),
    (*b"FR", "F37"),
    (*b"k4", "F4"),
    (*b"k5", "F5"),
    (*b"k6", "F6"),
    (*b"k7", "F7"),
    (*b"k8", "F8"),
    (*b"k9", "F9"),
    (*b"%1", "Help"),
    (*b"kh", "Home"),
    (*b"kI", "Insert"),
    (*b"KC", "k0"),
    (*b"KD", "k1"),
    (*b"KE", "k2"),
    (*b"KF", "k3"),
    (*b"KG", "k4"),
    (*b"KH", "k5"),
    (*b"KI", "k6"),
    (*b"KJ", "k7"),
    (*b"KK", "k8"),
    (*b"KL", "k9"),
    (*b"K8", "kDivide"),
    (*b"K4", "kEnd"),
    (*b"KA", "kEnter"),
    (*b"K1", "kHome"),
    (*b"K7", "kMinus"),
    (*b"K9", "kMultiply"),
    (*b"K5", "kPageDown"),
    (*b"K3", "kPageUp"),
    (*b"K6", "kPlus"),
    (*b"KB", "kPoint"),
    (*b"kl", "Left"),
    (*b"kN", "PageDown"),
    (*b"kP", "PageUp"),
    (*b"PE", "PasteEnd"),
    (*b"PS", "PasteStart"),
    (*b"kr", "Right"),
    ([KS_EXTRA, KE_TAB], "Tab"),
    (*b"&8", "Undo"),
    (*b"ku", "Up"),
];

/// The `MOD_MASK_SHIFT` rows of `modifier_keys_table[]` (vim 9.2
/// `misc2.c:826`) whose shifted key is a two-character termcap code: the
/// shifted key and the key without the modifier. The `MOD_MASK_CTRL` rows and
/// the `KS_EXTRA` shifted keys cannot be named by a `t_xx` option.
const modifier_keys_table: &[([u8; 2], [u8; 2])] = &[
    (*b"&9", *b"@1"),
    (*b"&0", *b"@2"),
    (*b"*1", *b"@4"),
    (*b"*2", *b"@5"),
    (*b"*3", *b"@6"),
    (*b"*4", *b"kD"),
    (*b"*5", *b"kL"),
    (*b"*7", *b"@7"),
    (*b"*9", *b"@9"),
    (*b"*0", *b"@0"),
    (*b"#1", *b"%1"),
    (*b"#2", *b"kh"),
    (*b"#3", *b"kI"),
    (*b"#4", *b"kl"),
    (*b"%a", *b"%3"),
    (*b"%b", *b"%4"),
    (*b"%c", *b"%5"),
    (*b"%d", *b"%7"),
    (*b"%e", *b"%8"),
    (*b"%f", *b"%9"),
    (*b"%g", *b"%0"),
    (*b"%h", *b"&3"),
    (*b"%i", *b"kr"),
    (*b"%j", *b"&5"),
    (*b"!1", *b"&6"),
    (*b"!2", *b"&7"),
    (*b"!3", *b"&8"),
    (*b"kB", [KS_EXTRA, KE_TAB]),
];

/// Port of `find_special_key_in_table()` (vim 9.2 `misc2.c:1682`) for a
/// termcap key `TERMCAP2KEY(c[0], c[1])`: the key's name in
/// [`key_names_table`].
pub fn find_special_key_in_table(c: [u8; 2]) -> Option<&'static str> {
    key_names_table
        .iter()
        .find(|(key, _)| *key == c)
        .map(|&(_, name)| name)
}

/// Port of `show_one_termcode()` (vim 9.2 `term.c:7580`): the `t_xx` line of
/// `:set t_xx?` for key code `name` — `t_xx`, the key's
/// `get_special_key_name(TERMCAP2KEY(name[0], name[1]), 0)` when that is not a
/// bare `<t_xx>`, padded to 17 columns (at least one blank), then `code` as
/// `msg_outtrans()` shows it.
///
/// `get_special_key_name()` is reduced to what a two-character termcap key can
/// reach: a shifted key is translated to its unshifted one through
/// [`modifier_keys_table`] with `S-` in front, then named from
/// [`key_names_table`], else spelled `t_xx`.
pub fn show_one_termcode(name: [u8; 2], code: &str) -> String {
    // c: `if (name[0] > '~')` — blanks for a code without a `t_xx` name.
    let mut buf = if name[0] > b'~' {
        "     ".to_string()
    } else {
        format!("t_{} ", String::from_utf8_lossy(&name))
    };
    let (modifiers, c) = modifier_keys_table
        .iter()
        .find(|(shifted, _)| *shifted == name)
        .map_or(("", name), |&(_, unshifted)| ("S-", unshifted));
    match find_special_key_in_table(c) {
        Some(key) => buf.push_str(&format!("<{modifiers}{key}>")),
        // c: `if (p[1] != 't')` — a bare `<t_xx>` is not shown.
        None if !modifiers.is_empty() => {
            buf.push_str(&format!("<{modifiers}t_{}>", String::from_utf8_lossy(&c)))
        }
        None => {}
    }
    // c: do IObuff[len++] = ' '; while (len < 17);
    loop {
        buf.push(' ');
        if buf.len() >= 17 {
            break;
        }
    }
    buf + code
}
