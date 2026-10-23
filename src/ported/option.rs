//! Port of `src/nvim/option.c` (subset) — the option table, `&opt` access, and
//! the `:set` command parser (`do_set`).
//!
//! Neovim's option machinery is large (hundreds of options, per-buffer/window
//! scopes, side effects). This ports the boolean, number and string options
//! whose default both reference engines agree on, plus the `do_set` argument
//! grammar (`set opt`, `set noopt`, `set opt!`, `set inv opt`, `set opt=val`,
//! `set opt?`, `set opt&`, `set opt<`) under `:set`, `:setlocal` and
//! `:setglobal`. Values live in `option_optval`'s store, which keeps a global
//! and a local value per option (see `option_optval::get_varp_scope`).
#![allow(non_snake_case, non_upper_case_globals)]

#[cfg(test)]
use crate::ported::eval::typval::tv_get_bool;
use crate::ported::eval::typval_defs_h::{typval_T, varnumber_T};
use crate::ported::option_optval::{
    self, find_option, get_varp_scope, kOptScopeBuf, kOptScopeGlobal, kOptScopeWin, options,
    optval_from_varp, set_option, OptValData, OptVarp, TriState, BOOLEAN_OPTVAL, NUMBER_OPTVAL,
    OPT_LOCAL, STRING_OPTVAL, TRISTATE_FROM_INT,
};
#[cfg(test)]
use std::collections::HashMap;

/// Option kind, for parsing `:set` values.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bool,
    Number,
    /// String options (`'shell'`, `'filetype'`, …) arrive with editor
    /// integration; the parse path already handles them.
    #[allow(dead_code)]
    String,
}

/// `(canonical name, abbreviation, kind, number default, string default)` rows of
/// the supported option table — the subset of `options[]` (`option.c`) ported so
/// far. `Kind::Bool`/`Kind::Number` rows read the number default and ignore the
/// string one; `Kind::String` rows the reverse.
const OPTIONS: &[(&str, &str, Kind, varnumber_T, &str)] = &[
    ("ignorecase", "ic", Kind::Bool, 0, ""),
    ("smartcase", "scs", Kind::Bool, 0, ""),
    ("magic", "magic", Kind::Bool, 1, ""),
    ("expandtab", "et", Kind::Bool, 0, ""),
    ("number", "nu", Kind::Bool, 0, ""),
    ("relativenumber", "rnu", Kind::Bool, 0, ""),
    ("wrap", "wrap", Kind::Bool, 1, ""),
    ("hlsearch", "hls", Kind::Bool, 0, ""),
    ("incsearch", "is", Kind::Bool, 0, ""),
    ("autoindent", "ai", Kind::Bool, 0, ""),
    ("tabstop", "ts", Kind::Number, 8, ""),
    ("shiftwidth", "sw", Kind::Number, 8, ""),
    ("softtabstop", "sts", Kind::Number, 0, ""),
    ("textwidth", "tw", Kind::Number, 0, ""),
    ("scrolloff", "so", Kind::Number, 0, ""),
    // Comma-separated runtime search path. Editor-less, its stored value starts
    // empty (`&rtp` reads ""); `set rtp+=DIR` records the user's additions, which
    // `:runtime` searches on top of the discovered system runtime dirs (see
    // `crate::fusevm_bridge::runtime_dirs`).
    ("runtimepath", "rtp", Kind::String, 0, ""),
    // 'filetype'/'syntax' fire autocommand side effects upstream (not modeled —
    // the value is stored only). They are here so this table and
    // `option_optval::options` have the SAME membership: the two are consulted by
    // different callers (`&opt` reads and `:set` come here, `exists('&opt')` and
    // `:let &opt` go there), and a name present in one but not the other makes
    // the two answers contradict each other.
    ("filetype", "ft", Kind::String, 0, ""),
    ("syntax", "syn", Kind::String, 0, ""),
    // ── Engine-invariant defaults ────────────────────────────────────────────
    //
    // Each row below carries its real default rather than "". The bar for
    // membership is that `vim -N -es -u NONE -i NONE` and `nvim --clean
    // --headless` report the SAME value, so no engine-specific or startup-
    // specific state is baked in. `scripts/parity.sh` pins the oracle to `-N`
    // (Vim defaults), which is the state these were measured in and the state
    // the Neovim-derived engine ports.
    //
    // Deliberately EXCLUDED, and why:
    //
    //   * The 27 options where the two engines genuinely disagree —
    //     'cpoptions' (vim `aABceFsz` / nvim `aABceFs_`), 'formatoptions'
    //     (`tcq`/`tcqj`), 'history' (200/10000), 'shortmess', 'path', 'complete',
    //     'listchars', 'fillchars', 'laststatus', 'startofline', 'joinspaces',
    //     'hidden', 'autoread', 'background', 'mouse', 'display', 'switchbuf',
    //     'sidescroll', 'ttimeoutlen', 'diffopt', 'sessionoptions',
    //     'viewoptions', 'nrformats', 'commentstring', 'define', 'include',
    //     'esckeys'. There is no single value to seed.
    //
    //   * LOCALE-DERIVED values, which are not a property of the language at
    //     all. Measured on vim 9.2.0900:
    //       'helplang'      '' under LC_ALL=C, 'en' under en_US.UTF-8,
    //                       'de' under de_DE.UTF-8, 'ja' under ja_JP.UTF-8
    //                       — locale-derived in BOTH engines, so no constant is
    //                       ever right.
    //       'fileencodings' 'ucs-bom' under LC_ALL=C, 'ucs-bom,utf-8,default,
    //                       latin1' otherwise — locale-derived in vim, a
    //                       constant in nvim, so the two engines only "agree"
    //                       in a UTF-8 locale.
    //     'encoding' is the same shape: vim answers 'latin1' under LC_ALL=C and
    //     'utf-8' otherwise, while nvim answers 'utf-8' unconditionally. It
    //     stays seeded 'utf-8' because this crate ports the NEOVIM engine, where
    //     the value is locale-independent by construction; that makes the
    //     LC_ALL=C reading a vim/nvim split, not a gap here.
    ("encoding", "enc", Kind::String, 0, "utf-8"),
    ("buftype", "bt", Kind::String, 0, ""),
    ("fileformat", "ff", Kind::String, 0, "unix"),
    ("iskeyword", "isk", Kind::String, 0, "@,48-57,_,192-255"),
    ("isprint", "isp", Kind::String, 0, "@,161-255"),
    (
        "isfname",
        "isf",
        Kind::String,
        0,
        "@,48-57,/,.,-,_,+,,,#,$,%,~,=",
    ),
    ("autowrite", "aw", Kind::Bool, 0, ""),
    ("backup", "bk", Kind::Bool, 0, ""),
    ("binary", "bin", Kind::Bool, 0, ""),
    ("bomb", "bomb", Kind::Bool, 0, ""),
    ("cindent", "cin", Kind::Bool, 0, ""),
    ("compatible", "cp", Kind::Bool, 0, ""),
    ("confirm", "cf", Kind::Bool, 0, ""),
    ("copyindent", "ci", Kind::Bool, 0, ""),
    ("cursorline", "cul", Kind::Bool, 0, ""),
    ("digraph", "dg", Kind::Bool, 0, ""),
    ("endofline", "eol", Kind::Bool, 1, ""),
    ("equalalways", "ea", Kind::Bool, 1, ""),
    ("errorbells", "eb", Kind::Bool, 0, ""),
    ("gdefault", "gd", Kind::Bool, 0, ""),
    ("infercase", "inf", Kind::Bool, 0, ""),
    ("insertmode", "im", Kind::Bool, 0, ""),
    ("lisp", "lisp", Kind::Bool, 0, ""),
    ("list", "list", Kind::Bool, 0, ""),
    ("modeline", "ml", Kind::Bool, 1, ""),
    ("modifiable", "ma", Kind::Bool, 1, ""),
    ("more", "more", Kind::Bool, 1, ""),
    ("paste", "paste", Kind::Bool, 0, ""),
    ("preserveindent", "pi", Kind::Bool, 0, ""),
    ("readonly", "ro", Kind::Bool, 0, ""),
    ("ruler", "ru", Kind::Bool, 1, ""),
    ("shiftround", "sr", Kind::Bool, 0, ""),
    ("showcmd", "sc", Kind::Bool, 1, ""),
    ("showmatch", "sm", Kind::Bool, 0, ""),
    ("smartindent", "si", Kind::Bool, 0, ""),
    ("splitbelow", "sb", Kind::Bool, 0, ""),
    ("splitright", "spr", Kind::Bool, 0, ""),
    ("swapfile", "swf", Kind::Bool, 1, ""),
    ("tildeop", "top", Kind::Bool, 0, ""),
    ("title", "title", Kind::Bool, 0, ""),
    ("visualbell", "vb", Kind::Bool, 0, ""),
    ("warn", "warn", Kind::Bool, 1, ""),
    ("wrapscan", "ws", Kind::Bool, 1, ""),
    ("writebackup", "wb", Kind::Bool, 1, ""),
    ("maxfuncdepth", "mfd", Kind::Number, 100, ""),
    ("maxmapdepth", "mmd", Kind::Number, 1000, ""),
    ("redrawtime", "rdt", Kind::Number, 2000, ""),
    ("regexpengine", "re", Kind::Number, 0, ""),
    ("report", "report", Kind::Number, 2, ""),
    ("timeoutlen", "tm", Kind::Number, 1000, ""),
    ("undolevels", "ul", Kind::Number, 1000, ""),
    ("updatetime", "ut", Kind::Number, 4000, ""),
    ("wildchar", "wc", Kind::Number, 9, ""),
    ("ambiwidth", "ambw", Kind::String, 0, "single"),
    ("backspace", "bs", Kind::String, 0, "indent,eol,start"),
    ("breakat", "brk", Kind::String, 0, " \t!@*-+;:,./?"),
    ("fileformats", "ffs", Kind::String, 0, "unix,dos"),
    ("isident", "isi", Kind::String, 0, "@,48-57,_,192-255"),
    ("matchpairs", "mps", Kind::String, 0, "(:),{:},[:]"),
    ("selection", "sel", Kind::String, 0, "inclusive"),
    (
        "suffixes",
        "su",
        Kind::String,
        0,
        ".bak,~,.o,.h,.info,.swp,.obj",
    ),
    ("whichwrap", "ww", Kind::String, 0, "b,s"),
    ("wildmode", "wim", Kind::String, 0, "full"),
    ("emoji", "emo", Kind::Bool, 1, ""),
    ("exrc", "ex", Kind::Bool, 0, ""),
    ("icon", "icon", Kind::Bool, 0, ""),
    ("linebreak", "lbr", Kind::Bool, 0, ""),
    ("termguicolors", "tgc", Kind::Bool, 0, ""),
    ("ttyfast", "tf", Kind::Bool, 1, ""),
    ("undofile", "udf", Kind::Bool, 0, ""),
    ("cmdheight", "ch", Kind::Number, 1, ""),
    ("cmdwinheight", "cwh", Kind::Number, 7, ""),
    // The screen size: 80x24 in vim -es and nvim --headless alike. An embedding
    // editor reports its own through `install_screen_size_hook`.
    ("columns", "co", Kind::Number, 80, ""),
    ("lines", "lines", Kind::Number, 24, ""),
    ("conceallevel", "cole", Kind::Number, 0, ""),
    ("foldlevel", "fdl", Kind::Number, 0, ""),
    ("helpheight", "hh", Kind::Number, 20, ""),
    ("iminsert", "imi", Kind::Number, 0, ""),
    ("imsearch", "ims", Kind::Number, -1, ""),
    ("matchtime", "mat", Kind::Number, 5, ""),
    ("numberwidth", "nuw", Kind::Number, 4, ""),
    ("pumheight", "ph", Kind::Number, 0, ""),
    ("pumwidth", "pw", Kind::Number, 15, ""),
    ("scrolljump", "sj", Kind::Number, 1, ""),
    ("showtabline", "stal", Kind::Number, 1, ""),
    ("sidescrolloff", "siso", Kind::Number, 0, ""),
    ("synmaxcol", "smc", Kind::Number, 3000, ""),
    ("updatecount", "uc", Kind::Number, 200, ""),
    ("winheight", "wh", Kind::Number, 1, ""),
    ("winminheight", "wmh", Kind::Number, 1, ""),
    ("winwidth", "wiw", Kind::Number, 20, ""),
    ("wrapmargin", "wm", Kind::Number, 0, ""),
    ("writedelay", "wd", Kind::Number, 0, ""),
    ("clipboard", "cb", Kind::String, 0, ""),
    ("keymodel", "km", Kind::String, 0, ""),
    ("langmap", "lmap", Kind::String, 0, ""),
    ("spellfile", "spf", Kind::String, 0, ""),
    // 'spelllang' is 'en' under LC_ALL=C, de_DE.UTF-8 and ja_JP.UTF-8 alike —
    // unlike 'helplang', it is not derived from the locale.
    ("spelllang", "spl", Kind::String, 0, "en"),
    ("virtualedit", "ve", Kind::String, 0, ""),
];

/// Port of `findoption()` (`option.c`) — resolve an option name or abbreviation
/// to its `OPTIONS` row.
fn findoption(
    name: &str,
) -> Option<&'static (&'static str, &'static str, Kind, varnumber_T, &'static str)> {
    OPTIONS
        .iter()
        .find(|(n, abbr, _, _, _)| *n == name || *abbr == name)
}

/// Port of `get_option_value()` (`option.c`) reduced — the value of `&name` (or
/// its abbreviation): `get_varp()`'s value, the local one where the option has
/// one that is set (see [`option_optval::get_varp_scope`]). Unknown options
/// yield "" (the empty string). The value store is `option_optval`'s.
pub fn get_option_value(name: &str) -> typval_T {
    let Some((canon, _, kind, _, _)) = findoption(name) else {
        return typval_T::from(String::new());
    };
    if let Some((lines, columns)) = SCREEN_SIZE_HOOK.with(|h| h.borrow().as_ref().map(|f| f())) {
        match *canon {
            "columns" => return typval_T::from(columns),
            "lines" => return typval_T::from(lines),
            _ => {}
        }
    }
    if let Kind::String = kind {
        if let Some(v) = BUF_OPTION_HOOK.with(|h| h.borrow().as_ref().and_then(|f| f(canon))) {
            return typval_T::from(v);
        }
    }
    let opt_idx = option_optval::find_option(canon);
    option_optval::optval_as_tv(option_optval::get_option_value(opt_idx, 0), true)
}

/// The host callback behind [`BUF_OPTION_HOOK`]: a string option's full name
/// to the host buffer's value, `None` when the host does not own it.
pub type BufOptionHook = Box<dyn Fn(&str) -> Option<String>>;

thread_local! {
    /// Host hook giving the current buffer's value of a string option the
    /// embedding editor owns (by full name), what `&opt` reads before this
    /// table: a session tests `if &buftype ==# 'terminal'` after the host opened
    /// the buffer. EXTENSION — installed by
    /// [`crate::fusevm_bridge::install_buf_option_hook`]; `None` falls through.
    pub static BUF_OPTION_HOOK: std::cell::RefCell<Option<BufOptionHook>> =
        const { std::cell::RefCell::new(None) };

    /// Host hook giving the embedding editor's screen `(lines, columns)`, what
    /// `&lines` / `&columns` read: a `:mksession` script sizes its windows from
    /// them (`exe 'vert 1resize ' . ((&columns * 30 + 99) / 199)`). EXTENSION —
    /// installed by [`crate::fusevm_bridge::install_screen_size_hook`]; unset, the
    /// table's 80x24 stands.
    #[allow(clippy::type_complexity)]
    pub static SCREEN_SIZE_HOOK: std::cell::RefCell<Option<Box<dyn Fn() -> (varnumber_T, varnumber_T)>>> =
        const { std::cell::RefCell::new(None) };
}

thread_local! {
    /// Host hook fired with the raw `:set` argument string whenever `:set` runs,
    /// so an embedding editor (zmax) can mirror the option onto its own live
    /// config. EXTENSION — no `vendor/` counterpart; the analogue of Vim's
    /// option-change side-effect callbacks (`did_set_*`). The installer lives in
    /// the crate-root carve-out [`crate::fusevm_bridge::install_set_hook`] (net-new
    /// synthesis does not belong under `src/ported/`); unset by default (no-op).
    pub static SET_HOST_HOOK: std::cell::RefCell<Option<Box<dyn Fn(&str)>>> =
        const { std::cell::RefCell::new(None) };
}
/// The `P_COMMA`/`P_ONECOMMA`/`P_NODUP`/`P_FLAGLIST`/`P_EXPAND` bits of the
/// string options in [`OPTIONS`], from vim's `optiondefs.h`. Only `:set`'s
/// `+=`/`^=`/`-=` and its value copy read them.
const P_COMMA: u32 = 0x1;
const P_ONECOMMA: u32 = 0x2 | P_COMMA;
const P_NODUP: u32 = 0x4;
const P_FLAGLIST: u32 = 0x8;
const P_EXPAND: u32 = 0x10;
const STRING_FLAGS: &[(&str, u32)] = &[
    ("runtimepath", P_EXPAND | P_ONECOMMA | P_NODUP),
    ("iskeyword", P_COMMA | P_NODUP),
    ("isprint", P_COMMA | P_NODUP),
    ("isfname", P_COMMA | P_NODUP),
    ("isident", P_COMMA | P_NODUP),
    ("backspace", P_ONECOMMA | P_NODUP),
    ("breakat", P_FLAGLIST),
    ("fileformats", P_ONECOMMA | P_NODUP),
    ("matchpairs", P_ONECOMMA | P_NODUP),
    ("suffixes", P_ONECOMMA | P_NODUP),
    ("whichwrap", P_ONECOMMA | P_FLAGLIST),
    ("wildmode", P_ONECOMMA | P_NODUP),
    ("clipboard", P_ONECOMMA | P_NODUP),
    ("keymodel", P_ONECOMMA | P_NODUP),
    ("langmap", P_ONECOMMA | P_NODUP),
    ("spellfile", P_EXPAND | P_ONECOMMA),
    ("spelllang", P_EXPAND | P_ONECOMMA),
    ("virtualedit", P_ONECOMMA | P_NODUP),
];

/// Port of `do_set()` (vim `option.c`) — parse and apply a `:set` argument
/// string, one `do_set_option()` per argument: `opt`, `noopt`, `invopt`,
/// `opt!`, `opt&`, `opt&vim`, `opt<`, `opt?`, `opt=val`/`opt:val` and the
/// `+=`/`^=`/`-=` operators. `opt_flags` is `ex_set()`'s: 0 for `:set`,
/// `OPT_LOCAL` for `:setlocal`, `OPT_GLOBAL` for `:setglobal`.
///
/// The first error is reported as `{msg}: {argument}` and ends the command.
///
/// RUST-PORT NOTE: an option name this crate's reduced table does not know is
/// skipped silently instead of `E518` — most of vim's options are not in it,
/// and a plugin's `set cpo&vim` must keep running. A query (`opt?`, a bare
/// number/string option) shows nothing: the `-es` message stream it writes to
/// is not modelled.
pub fn do_set(args: &str, opt_flags: i32) {
    // Mirror the whole `:set` line to the host editor first (if a hook is
    // installed), then keep vimlrs' own option table in sync below so `&opt`
    // reads inside vimscript still see the value.
    SET_HOST_HOOK.with(|h| {
        if let Some(f) = h.borrow().as_ref() {
            f(args);
        }
    });
    // c: the `+=`/`^=`/`-=` half of `stropt_get_newval()`: `find_dup_item()`,
    // `stropt_concat_with_comma()`, `stropt_remove_val()` and
    // `stropt_remove_dupflags()`.
    let stropt_newval = |origval: &str, newval: String, op: u8, flags: u32| -> String {
        if op == 0 {
            return newval;
        }
        let comma_list = flags & P_COMMA != 0;
        // c: find_dup_item() — an item that starts the value or follows an
        // unescaped comma, and ends at a comma or the end.
        let find_dup = || -> Option<usize> {
            let o = origval.as_bytes();
            let n = newval.as_bytes();
            let mut bs = 0;
            for s in 0..o.len() {
                if (!comma_list || s == 0 || (o[s - 1] == b',' && bs & 1 == 0))
                    && o[s..].starts_with(n)
                    && (!comma_list || s + n.len() == o.len() || o[s + n.len()] == b',')
                {
                    return Some(s);
                }
                if (s > 1 && o[s - 1] == b'\\' && o[s - 2] != b',') || (s == 1 && o[0] == b'\\') {
                    bs += 1;
                } else {
                    bs = 0;
                }
            }
            None
        };
        let dup = if op == b'-' || flags & P_NODUP != 0 {
            find_dup()
        } else {
            None
        };
        let mut out = match op {
            // c: "do not add if already there"
            b'+' | b'^' if dup.is_some() => origval.to_string(),
            b'+' | b'^' => {
                let comma = comma_list && !origval.is_empty() && !newval.is_empty();
                let mut orig = origval;
                // c: "strip a trailing comma, would get 2"
                if op == b'+'
                    && comma
                    && flags & P_ONECOMMA == P_ONECOMMA
                    && orig.len() > 1
                    && orig.ends_with(',')
                    && orig.as_bytes()[orig.len() - 2] != b'\\'
                {
                    orig = &orig[..orig.len() - 1];
                }
                let sep = if comma { "," } else { "" };
                if op == b'+' {
                    format!("{orig}{sep}{newval}")
                } else {
                    format!("{newval}{sep}{orig}")
                }
            }
            // c: stropt_remove_val()
            _ => match dup {
                Some(s) if !newval.is_empty() => {
                    let (mut from, mut to) = (s, s + newval.len());
                    if comma_list {
                        if s == 0 {
                            if origval.as_bytes().get(to) == Some(&b',') {
                                to += 1;
                            }
                        } else {
                            from -= 1;
                        }
                    }
                    format!("{}{}", &origval[..from], &origval[to..])
                }
                _ => origval.to_string(),
            },
        };
        // c: stropt_remove_dupflags()
        if flags & P_FLAGLIST != 0 {
            let mut v: Vec<u8> = out.into_bytes();
            let mut s = 0;
            while s < v.len() {
                let dup_after = |v: &[u8], from: usize, c: u8| v[from..].contains(&c);
                if flags & P_ONECOMMA == P_ONECOMMA {
                    if v[s] != b',' && v.get(s + 1) == Some(&b',') && dup_after(&v, s + 2, v[s]) {
                        v.drain(s..s + 2);
                        continue;
                    }
                } else if (!comma_list || v[s] != b',') && dup_after(&v, s + 1, v[s]) {
                    v.remove(s);
                    continue;
                }
                s += 1;
            }
            out = String::from_utf8_lossy(&v).into_owned();
        }
        out
    };
    // c: `do_set_option()` — one argument, `arg` running to the end of the line.
    let do_set_option = |arg: &str| -> Option<String> {
        const E474: &str = "E474: Invalid argument";
        const E488: &str = "E488: Trailing characters";
        const E521: &str = "E521: Number required after =";
        let white = |c: u8| c == b' ' || c == b'\t';
        let at = |s: &[u8], k: usize| s.get(k).copied().unwrap_or(0);

        // c: get_option_prefix()
        let (prefix, arg) = if arg.starts_with("no") && !arg.starts_with("novice") {
            (b'n', &arg[2..])
        } else if let Some(rest) = arg.strip_prefix("inv") {
            (b'i', rest)
        } else {
            (0, arg)
        };
        let s = arg.as_bytes();
        // c: parse_option_name() — `t_xx` is four characters, any other name the run
        // of alphanumerics and `_`.
        let mut len = if s.starts_with(b"t_") && s.len() >= 4 {
            4
        } else {
            s.iter()
                .take_while(|c| c.is_ascii_alphanumeric() || **c == b'_')
                .count()
        };
        let name_len = len;
        let afterchar = at(s, len);
        while white(at(s, len)) {
            len += 1;
        }
        // c: get_opt_op()
        let op = match (at(s, len), at(s, len + 1)) {
            (c @ (b'+' | b'^' | b'-'), b'=') => {
                len += 1;
                c
            }
            _ => 0,
        };
        let nextchar = at(s, len);
        let Some((canon, _, kind, _, _)) = findoption(&arg[..name_len]) else {
            return None;
        };
        let opt_idx = find_option(canon);
        let varp = get_varp_scope(opt_idx, opt_flags);
        let mut p = len; // c: `arg += len` — at nextchar
        if b"?=:!&<".contains(&nextchar) && nextchar != 0 {
            // c: "opt&vim" / "opt&vi"
            if nextchar == b'&' && at(s, p + 1) == b'v' && at(s, p + 2) == b'i' {
                p += if at(s, p + 3) == b'm' { 3 } else { 2 };
            }
            if b"?!&<".contains(&nextchar) && at(s, p + 1) != 0 && !white(at(s, p + 1)) {
                return Some(E488.to_string());
            }
        }
        // c: "print value" — `opt?`, or a bare number/string option.
        if nextchar == b'?' || (prefix == 0 && !b"=:&<".contains(&nextchar) && *kind != Kind::Bool)
        {
            if nextchar != b'?' && nextchar != 0 && !white(afterchar) {
                return Some(E488.to_string());
            }
            return None;
        }
        let cur = optval_from_varp(opt_idx, varp);
        let def_val = options[opt_idx].def_val.clone();
        // c: `opt<` copies the global value; `:setlocal` of a global-local option
        // instead unsets the local value (`NO_LOCAL_UNDOLEVEL`, -1, ''), so
        // `get_varp()` goes back to the global one.
        let scope = options[opt_idx].scope_flags;
        let global_local =
            scope & kOptScopeGlobal != 0 && scope & (kOptScopeBuf | kOptScopeWin) != 0;
        let global = || {
            if global_local && opt_flags == OPT_LOCAL {
                crate::ported::option_optval::NIL_OPTVAL()
            } else {
                optval_from_varp(opt_idx, OptVarp::Global)
            }
        };
        let value = match kind {
            // c: do_set_option_bool()
            Kind::Bool => {
                if nextchar == b'=' || nextchar == b':' {
                    return Some(E474.to_string());
                }
                let on = matches!(cur.data, OptValData::boolean(TriState::kTrue));
                match nextchar {
                    b'!' => BOOLEAN_OPTVAL(TRISTATE_FROM_INT(i64::from(!on))),
                    b'&' => def_val,
                    b'<' => global(),
                    _ => {
                        if nextchar != 0 && !white(afterchar) {
                            return Some(E488.to_string());
                        }
                        match prefix {
                            b'i' => BOOLEAN_OPTVAL(TRISTATE_FROM_INT(i64::from(!on))),
                            b'n' => BOOLEAN_OPTVAL(TriState::kFalse),
                            _ => BOOLEAN_OPTVAL(TriState::kTrue),
                        }
                    }
                }
            }
            _ if !b"=:&<".contains(&nextchar) || nextchar == 0 || prefix != 0 => {
                return Some(E474.to_string());
            }
            // c: do_set_option_numeric()
            Kind::Number => match nextchar {
                b'&' => def_val,
                b'<' => global(),
                _ => {
                    let v = &arg[p + 1..];
                    let vb = v.as_bytes();
                    if !(at(vb, 0) == b'-' || at(vb, 0).is_ascii_digit()) {
                        return Some(E521.to_string());
                    }
                    // c: "Allow negative (for 'undolevels'), octal and hex numbers."
                    let mut n_len = 0i32;
                    let mut n: varnumber_T = 0;
                    crate::ported::charset::vim_str2nr(
                        v,
                        None,
                        Some(&mut n_len),
                        crate::ported::charset::STR2NR_ALL,
                        Some(&mut n),
                        None,
                        0,
                        true,
                        None,
                    );
                    let n_len = n_len as usize;
                    if n_len == 0 || (n_len < vb.len() && !white(vb[n_len])) {
                        return Some(E521.to_string());
                    }
                    let old = match cur.data {
                        OptValData::number(c) => c,
                        _ => 0,
                    };
                    NUMBER_OPTVAL(match op {
                        b'+' => old.wrapping_add(n),
                        b'^' => old.wrapping_mul(n),
                        b'-' => old.wrapping_sub(n),
                        _ => n,
                    })
                }
            },
            // c: do_set_option_string() / stropt_get_newval()
            Kind::String => match nextchar {
                b'&' => def_val,
                b'<' => global(),
                _ => {
                    let flags = STRING_FLAGS
                        .iter()
                        .find(|(n, _)| *n == *canon)
                        .map_or(0, |(_, f)| *f);
                    // c: a `:set` of a global-local string option works on the
                    // global value (the local one is emptied by `set_option`);
                    // `:setlocal` on its local value, or the global one while the
                    // local one is unset.
                    let origvarp = match opt_flags {
                        0 if global_local => OptVarp::Global,
                        OPT_LOCAL if global_local => get_varp_scope(opt_idx, 0),
                        _ => varp,
                    };
                    let origval = match optval_from_varp(opt_idx, origvarp).data {
                        OptValData::string(s) => s,
                        _ => String::new(),
                    };
                    // c: stropt_copy_value() — a backslash escapes the next
                    // character, except before a file-name character of a P_EXPAND
                    // option (`set rtp=c:\dir`).
                    let raw = &arg.as_bytes()[p + 1..];
                    let mut newval: Vec<u8> = Vec::new();
                    let mut k = 0usize;
                    while k < raw.len() && !white(raw[k]) {
                        if raw[k] == b'\\' && k + 1 < raw.len() {
                            let c = raw[k + 1];
                            let keep = flags & P_EXPAND != 0
                                && (c.is_ascii_alphanumeric() || b"/.-_+,#$%~=".contains(&c))
                                && c != b'\\';
                            if !keep {
                                k += 1;
                            }
                        }
                        newval.push(raw[k]);
                        k += 1;
                    }
                    let newval = String::from_utf8_lossy(&newval).into_owned();
                    STRING_OPTVAL(stropt_newval(&origval, newval, op, flags))
                }
            },
        };
        set_option(opt_idx, value, opt_flags)
    };
    let b = args.as_bytes();
    let white = |c: u8| c == b' ' || c == b'\t';
    let mut i = 0usize;
    while i < b.len() && white(b[i]) {
        i += 1;
    }
    while i < b.len() {
        let start = i;
        let errmsg = do_set_option(&args[i..]);
        // c: "Advance to next argument: skip until a blank found, taking care of
        // backslashes; skip blanks."
        while i < b.len() && !white(b[i]) {
            if b[i] == b'\\' && i + 1 < b.len() {
                i += 1;
            }
            i += 1;
        }
        while i < b.len() && white(b[i]) {
            i += 1;
        }
        // c: the message quotes `startarg` up to the NEXT argument, so the
        // blanks after the failing one are part of it (`E521: … tw=x `).
        if let Some(msg) = errmsg {
            crate::ported::message::emsg(&format!("{msg}: {}", &args[start..i]));
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_host_hook_fires_with_raw_args() {
        use std::cell::RefCell;
        thread_local! { static SEEN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) }; }
        super::SET_HOST_HOOK.with(|h| {
            *h.borrow_mut() = Some(Box::new(|a: &str| {
                SEEN.with(|s| s.borrow_mut().push(a.to_string()))
            }));
        });
        super::do_set("number tw=80", 0);
        SEEN.with(|s| assert_eq!(s.borrow().as_slice(), &["number tw=80".to_string()]));
        // and vimlrs' own option table still tracks it (dual-write):
        assert!(super::findoption("tw").is_some());
    }

    /// No option name or abbreviation may appear twice in `OPTIONS`.
    ///
    /// `findoption` is a linear scan that returns the FIRST row whose full name
    /// or abbreviation matches, so a duplicate would silently shadow a later row
    /// and make one option unreachable — the same failure mode the builtin-id
    /// collision guard in `tests/opcodes.rs` exists for. An abbreviation that
    /// collides with another option's FULL name is the dangerous shape (`&list`
    /// vs a hypothetical `li` abbreviation), so both namespaces are checked in
    /// one pass rather than separately.
    #[test]
    fn option_names_are_unique() {
        let mut seen: HashMap<&str, &str> = HashMap::new();
        for (full, abbr, ..) in OPTIONS {
            for key in [full, abbr] {
                if let Some(prev) = seen.insert(key, full) {
                    if prev != *full {
                        panic!("option key {key:?} is claimed by both {prev:?} and {full:?}");
                    }
                }
            }
        }
    }

    /// `OPTIONS` and `option_optval::options` must describe the same options.
    ///
    /// The two tables are consulted by DIFFERENT callers: `&opt` reads and
    /// `:set` resolve through this file's `findoption`, while `exists('&opt')`
    /// and `:let &opt` resolve through `option_optval::find_option`. A name in
    /// one table but not the other therefore makes the two answers contradict —
    /// which is exactly what shipped before this test existed: `exists('&rtp')`
    /// answered 0 while `&rtp` resolved, because 'runtimepath' was in this table
    /// only. Real vim answers 1 and a value for every option in either list.
    ///
    /// Names, abbreviations, kinds AND defaults are all compared, so a row added
    /// to one side with a different default cannot pass either.
    #[test]
    fn option_tables_agree() {
        let mine: Vec<(&str, &str, &str, String)> = OPTIONS
            .iter()
            .map(|(full, abbr, kind, num, s)| {
                let (tag, def) = match kind {
                    Kind::Bool => ("bool", num.to_string()),
                    Kind::Number => ("number", num.to_string()),
                    Kind::String => ("string", (*s).to_string()),
                };
                (*full, *abbr, tag, def)
            })
            .collect();
        // Read straight off `option_optval::options` — the C's `options[]`. A
        // helper function on that side would have been an invented name under
        // `src/ported/`, which `tests/ported_fn_names_match_c.rs` rejects (and
        // correctly: the C has one option table, so it needs no such accessor).
        // The comparison lives here, in a `#[cfg(test)]` fn the gate exempts.
        use crate::ported::option_optval::{OptValData, OptValType, TriState};
        let mut mine = mine;
        let mut theirs: Vec<(&str, &str, &str, String)> = crate::ported::option_optval::options
            .iter()
            .map(|o| {
                let (tag, def) = match (&o.r#type, &o.def_val.data) {
                    (OptValType::kOptValTypeBoolean, OptValData::boolean(b)) => (
                        "bool",
                        match b {
                            TriState::kTrue => "1".to_string(),
                            _ => "0".to_string(),
                        },
                    ),
                    (OptValType::kOptValTypeNumber, OptValData::number(n)) => {
                        ("number", n.to_string())
                    }
                    (OptValType::kOptValTypeString, OptValData::string(s)) => ("string", s.clone()),
                    _ => ("nil", String::new()),
                };
                (o.fullname, o.shortname, tag, def)
            })
            .collect();
        mine.sort();
        theirs.sort();
        assert_eq!(
            mine, theirs,
            "src/ported/option.rs OPTIONS and src/ported/option_optval.rs options \
             have drifted — every option must be in both, with the same \
             abbreviation, kind and default"
        );
    }

    #[test]
    fn set_and_get_bool_and_number() {
        let ic = || tv_get_bool(&get_option_value("ignorecase")) != 0;
        do_set("ignorecase", 0);
        assert!(ic());
        do_set("noic", 0); // abbreviation + no-prefix
        assert!(!ic());
        do_set("ic!", 0); // toggle
        assert!(ic());
        do_set("tabstop=4", 0);
        assert_eq!(
            crate::ported::eval::typval::tv_get_number_chk(&get_option_value("ts"), None),
            4
        );
    }
}
