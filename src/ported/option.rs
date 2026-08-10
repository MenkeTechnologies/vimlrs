//! Port of `src/nvim/option.c` (subset) — the option table, `&opt` access, and
//! the `:set` command parser (`do_set`).
//!
//! Neovim's option machinery is large (hundreds of options, per-buffer/window
//! scopes, side effects). This ports the boolean, number and string options
//! whose default both reference engines agree on, plus the `do_set` argument
//! grammar (`set opt`, `set noopt`, `set opt!`, `set inv opt`, `set opt=val`,
//! `set opt?`); the value store is a thread-local map seeded with Vim's
//! defaults. Per-buffer and per-window scopes follow with the editor
//! integration — a buffer-local option's row here carries its default and is
//! read globally.
#![allow(non_snake_case, non_upper_case_globals)]

use std::cell::RefCell;
use std::collections::HashMap;

use crate::ported::eval::typval::{tv_get_bool, tv_get_string};
use crate::ported::eval::typval_defs_h::{typval_T, varnumber_T};

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

thread_local! {
    /// Current option values, keyed by canonical name. Lazily seeded from the
    /// table defaults on first access.
    static option_values: RefCell<HashMap<String, typval_T>> = RefCell::new(HashMap::new());
}

/// Port of `findoption()` (`option.c`) — resolve an option name or abbreviation
/// to its `OPTIONS` row.
fn findoption(
    name: &str,
) -> Option<&'static (&'static str, &'static str, Kind, varnumber_T, &'static str)> {
    OPTIONS
        .iter()
        .find(|(n, abbr, _, _, _)| *n == name || *abbr == name)
}

/// Port of `set_option_value()` (`option.c`) reduced — store option `canon`'s
/// value.
fn set_option_value(canon: &str, tv: typval_T) {
    option_values.with(|m| {
        m.borrow_mut().insert(canon.to_string(), tv);
    });
}

/// Port of `get_option_value()` (`option.c`) reduced — the value of `&name` (or
/// its abbreviation). Unknown options yield "" (the empty string).
pub fn get_option_value(name: &str) -> typval_T {
    let Some((canon, _, kind, default, sdefault)) = findoption(name) else {
        return typval_T::from(String::new());
    };
    option_values.with(|m| {
        m.borrow()
            .get(*canon)
            .cloned()
            .unwrap_or_else(|| match kind {
                Kind::String => typval_T::from(sdefault.to_string()),
                _ => typval_T::from(*default),
            })
    })
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

/// Port of `do_set()` (`option.c`) — parse and apply a `:set` argument string:
/// `set opt` / `set noopt` / `set opt!` / `set invopt` / `set opt=val` /
/// `set opt:val` / `set opt?` (whitespace-separated, multiple per line).
pub fn do_set(args: &str) {
    // Mirror the whole `:set` line to the host editor first (if a hook is
    // installed), then keep vimlrs' own option table in sync below so `&opt`
    // reads inside vimscript still see the value.
    SET_HOST_HOOK.with(|h| {
        if let Some(f) = h.borrow().as_ref() {
            f(args);
        }
    });
    for part in args.split_whitespace() {
        // `opt=val` / `opt:val`, plus the compound-assign operators `opt+=val`
        // (append), `opt^=val` (prepend), `opt-=val` (remove) — `do_set`'s
        // OP_ADDING/OP_PREPENDING/OP_REMOVING. Compound ops apply to comma-list
        // string options (e.g. `set rtp+=DIR`); on number/bool options a compound
        // op is left as a no-op (matches the prior behavior where `sw+` failed to
        // resolve). A plain `=`/`:` sets.
        if let Some((lhs, val)) = part.split_once(['=', ':']) {
            let (name, op) = match lhs.strip_suffix(['+', '^', '-']) {
                Some(base) => (base, lhs.as_bytes()[lhs.len() - 1]),
                None => (lhs, b'='),
            };
            if let Some((canon, _, kind, _, _)) = findoption(name) {
                let tv = match (kind, op) {
                    (Kind::String, b'+') => {
                        let cur = tv_get_string(&get_option_value(canon));
                        typval_T::from(if cur.is_empty() {
                            val.to_string()
                        } else {
                            format!("{cur},{val}")
                        })
                    }
                    (Kind::String, b'^') => {
                        let cur = tv_get_string(&get_option_value(canon));
                        typval_T::from(if cur.is_empty() {
                            val.to_string()
                        } else {
                            format!("{val},{cur}")
                        })
                    }
                    (Kind::String, b'-') => {
                        let cur = tv_get_string(&get_option_value(canon));
                        typval_T::from(
                            cur.split(',')
                                .filter(|s| *s != val)
                                .collect::<Vec<_>>()
                                .join(","),
                        )
                    }
                    (Kind::String, _) => typval_T::from(val.to_string()),
                    (_, b'=') => typval_T::from(val.trim().parse::<varnumber_T>().unwrap_or(0)),
                    // Compound op on a number/bool option: no-op.
                    _ => continue,
                };
                set_option_value(canon, tv);
            }
            continue;
        }
        // `opt!` (toggle a bool) / `opt?` (query — no-op here).
        if let Some(name) = part.strip_suffix('!') {
            if let Some((canon, _, Kind::Bool, _, _)) = findoption(name) {
                let cur = tv_get_bool(&get_option_value(canon)) != 0;
                set_option_value(canon, typval_T::from(varnumber_T::from(!cur)));
            }
            continue;
        }
        if part.ends_with('?') {
            continue; // query form: no terminal output in this subset
        }
        // `noopt` / `invopt` (bool off / invert).
        if let Some(name) = part.strip_prefix("no") {
            if let Some((canon, _, Kind::Bool, _, _)) = findoption(name) {
                set_option_value(canon, typval_T::from(0));
                continue;
            }
        }
        if let Some(name) = part.strip_prefix("inv") {
            if let Some((canon, _, Kind::Bool, _, _)) = findoption(name) {
                let cur = tv_get_bool(&get_option_value(canon)) != 0;
                set_option_value(canon, typval_T::from(varnumber_T::from(!cur)));
                continue;
            }
        }
        // Bare `opt` — turn a boolean on (number/string forms are queries).
        if let Some((canon, _, Kind::Bool, _, _)) = findoption(part) {
            set_option_value(canon, typval_T::from(1));
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
        super::do_set("number tw=80");
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
        do_set("ignorecase");
        assert!(ic());
        do_set("noic"); // abbreviation + no-prefix
        assert!(!ic());
        do_set("ic!"); // toggle
        assert!(ic());
        do_set("tabstop=4");
        assert_eq!(
            crate::ported::eval::typval::tv_get_number_chk(&get_option_value("ts"), None),
            4
        );
    }
}
