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
    ("verbose", "vbs", Kind::Number, 0, ""),
    ("clipboard", "cb", Kind::String, 0, ""),
    ("keymodel", "km", Kind::String, 0, ""),
    ("langmap", "lmap", Kind::String, 0, ""),
    ("spellfile", "spf", Kind::String, 0, ""),
    // 'spelllang' is 'en' under LC_ALL=C, de_DE.UTF-8 and ja_JP.UTF-8 alike —
    // unlike 'helplang', it is not derived from the locale.
    ("spelllang", "spl", Kind::String, 0, "en"),
    ("virtualedit", "ve", Kind::String, 0, ""),
];

/// Every option name and abbreviation in vim 9.2's `options[]`
/// (`optiondefs.h`), sorted bytewise for `binary_search`. Most are not in
/// [`OPTIONS`]: a `:set` of one of them is accepted and does nothing here,
/// while a name in neither table is `E518: Unknown option` (`do_set_option`'s
/// `findoption()` miss). The `t_xx` terminal codes are not listed; `:set` skips
/// them.
#[rustfmt::skip]
const VIM_OPTION_NAMES: &[&str] = &[
    "ac", "acd", "acl", "act", "ai", "akm", "al", "aleph", "allowrevins", "altkeymap", "ambiwidth",
    "ambw", "anti", "antialias", "ap", "ar", "arab", "arabic", "arabicshape", "ari", "arshape",
    "asd", "autochdir", "autocomplete", "autocompletedelay", "autocompletetimeout", "autoindent",
    "autoprint", "autoread", "autoshelldir", "autowrite", "autowriteall", "aw", "awa",
    "background", "backspace", "backup", "backupcopy", "backupdir", "backupext", "backupskip",
    "balloondelay", "ballooneval", "balloonevalterm", "balloonexpr", "bdir", "bdlay", "beautify",
    "belloff", "beval", "bevalterm", "bex", "bexpr", "bf", "bg", "bh", "bin", "binary", "biosk",
    "bioskey", "bk", "bkc", "bl", "bo", "bomb", "breakat", "breakindent", "breakindentopt", "bri",
    "briopt", "brk", "browsedir", "bs", "bsdir", "bsk", "bt", "bufhidden", "buflisted", "buftype",
    "casemap", "cb", "cc", "ccv", "cd", "cdh", "cdhome", "cdpath", "cedit", "cf", "cfc", "cfu",
    "ch", "charconvert", "chi", "chistory", "ci", "cia", "cin", "cindent", "cink", "cinkeys",
    "cino", "cinoptions", "cinscopedecls", "cinsd", "cinw", "cinwords", "clipboard", "clipmethod",
    "cm", "cmdheight", "cmdwinheight", "cmp", "cms", "co", "cocu", "cole", "colorcolumn",
    "columns", "com", "comments", "commentstring", "compatible", "complete", "completefunc",
    "completefuzzycollect", "completeitemalign", "completeopt", "completepopup", "completeslash",
    "completetimeout", "concealcursor", "conceallevel", "confirm", "consk", "conskey",
    "copyindent", "cot", "cp", "cpm", "cpo", "cpoptions", "cpp", "cpt", "crb", "cryptmethod",
    "cscopepathcomp", "cscopeprg", "cscopequickfix", "cscoperelative", "cscopetag",
    "cscopetagorder", "cscopeverbose", "csl", "cspc", "csprg", "csqf", "csre", "cst", "csto",
    "csverb", "cto", "cuc", "cul", "culopt", "cursorbind", "cursorcolumn", "cursorline",
    "cursorlineopt", "cwh", "debug", "deco", "def", "define", "delcombine", "dex", "dg", "dia",
    "dict", "dictionary", "diff", "diffanchors", "diffexpr", "diffopt", "digraph", "dip", "dir",
    "directory", "display", "dy", "ea", "ead", "eadirection", "eb", "ed", "edcompatible", "ef",
    "efm", "ei", "eiw", "ek", "emo", "emoji", "enc", "encoding", "endoffile", "endofline", "eof",
    "eol", "ep", "equalalways", "equalprg", "errorbells", "errorfile", "errorformat", "esckeys",
    "et", "eventignore", "eventignorewin", "ex", "expandtab", "exrc", "fcl", "fcs", "fdc", "fde",
    "fdi", "fdl", "fdls", "fdm", "fdn", "fdo", "fdt", "fen", "fenc", "fencs", "fex", "ff", "ffs",
    "ffu", "fic", "fileencoding", "fileencodings", "fileformat", "fileformats", "fileignorecase",
    "filetype", "fillchars", "findfunc", "fixendofline", "fixeol", "fk", "fkmap", "fl", "flash",
    "flp", "fml", "fmr", "fo", "foldclose", "foldcolumn", "foldenable", "foldexpr", "foldignore",
    "foldlevel", "foldlevelstart", "foldmarker", "foldmethod", "foldminlines", "foldnestmax",
    "foldopen", "foldtext", "formatexpr", "formatlistpat", "formatoptions", "formatprg", "fp",
    "fs", "fsync", "ft", "gcr", "gd", "gdefault", "gfm", "gfn", "gfs", "gfw", "ghr", "gli", "go",
    "gp", "gr", "graphic", "grepformat", "grepprg", "gtl", "gtt", "guicursor", "guifont",
    "guifontset", "guifontwide", "guiheadroom", "guiligatures", "guioptions", "guipty",
    "guitablabel", "guitabtooltip", "hardtabs", "helpfile", "helpheight", "helplang", "hf", "hh",
    "hi", "hid", "hidden", "highlight", "history", "hk", "hkmap", "hkmapp", "hkp", "hl", "hlg",
    "hls", "hlsearch", "ht", "ic", "icon", "iconstring", "ignorecase", "im", "imactivatefunc",
    "imactivatekey", "imaf", "imageprotocol", "imak", "imc", "imcmdline", "imd", "imdisable",
    "imi", "iminsert", "ims", "imsearch", "imsf", "imst", "imstatusfunc", "imstyle", "inc",
    "include", "includeexpr", "incsearch", "inde", "indentexpr", "indentkeys", "indk", "inex",
    "inf", "infercase", "insertmode", "ipc", "is", "isf", "isfname", "isi", "isident", "isk",
    "iskeyword", "isp", "isprint", "joinspaces", "jop", "js", "jumpoptions", "key", "keymap",
    "keymodel", "keyprotocol", "keywordprg", "km", "kmp", "kp", "kpc", "langmap", "langmenu",
    "langnoremap", "langremap", "laststatus", "lazyredraw", "lbr", "lcs", "lhi", "lhistory",
    "linebreak", "lines", "linespace", "lisp", "lispoptions", "lispwords", "list", "listchars",
    "lm", "lmap", "lnr", "loadplugins", "lop", "lpl", "lrm", "ls", "lsp", "luadll", "lw", "lz",
    "ma", "macatsui", "magic", "makeef", "makeencoding", "makeprg", "mat", "matchpairs",
    "matchtime", "maxcombine", "maxfuncdepth", "maxmapdepth", "maxmem", "maxmempattern",
    "maxmemtot", "maxsearchcount", "mco", "mef", "menc", "menuitems", "mesg", "messagesopt", "mfd",
    "mh", "mis", "mkspellmem", "ml", "mle", "mls", "mlst", "mm", "mmd", "mmp", "mmt", "mod",
    "modeline", "modelineexpr", "modelines", "modelinestrict", "modifiable", "modified", "mopt",
    "more", "mouse", "mousef", "mousefocus", "mousehide", "mousem", "mousemev", "mousemodel",
    "mousemoveevent", "mouses", "mouseshape", "mouset", "mousetime", "mp", "mps", "msc", "msm",
    "mzq", "mzquantum", "mzschemedll", "mzschemegcdll", "nf", "novice", "nrformats", "nu",
    "number", "numberwidth", "nuw", "odev", "oft", "ofu", "omnifunc", "open", "opendevice",
    "operatorfunc", "opfunc", "opt", "optimize", "osctimeoutlen", "osfiletype", "ost", "pa",
    "packpath", "para", "paragraphs", "paste", "pastetoggle", "patchexpr", "patchmode", "path",
    "pb", "pdev", "penc", "perldll", "pex", "pexpr", "pfn", "ph", "pheader", "pi", "pm", "pmbcs",
    "pmbfn", "pmw", "popt", "pp", "preserveindent", "previewheight", "previewpopup",
    "previewwindow", "printdevice", "printencoding", "printexpr", "printfont", "printheader",
    "printmbcharset", "printmbfont", "printoptions", "prompt", "pt", "pumborder", "pumheight",
    "pummaxwidth", "pumopt", "pumwidth", "pvh", "pvp", "pvw", "pw", "pythondll", "pythonhome",
    "pythonthreedll", "pythonthreehome", "pyx", "pyxversion", "qe", "qftf", "quickfixtextfunc",
    "quoteescape", "rdt", "re", "readonly", "redraw", "redrawtime", "regexpengine",
    "relativenumber", "remap", "renderoptions", "report", "restorescreen", "revins", "ri",
    "rightleft", "rightleftcmd", "rl", "rlc", "rnu", "ro", "rop", "rs", "rtp", "ru", "rubydll",
    "ruf", "ruler", "rulerformat", "runtimepath", "sb", "sbo", "sbr", "sc", "scb", "scf", "scl",
    "scr", "scroll", "scrollbind", "scrollfocus", "scrolljump", "scrolloff", "scrolloffpad",
    "scrollopt", "scs", "sect", "sections", "secure", "sel", "selection", "selectmode",
    "sessionoptions", "sft", "sh", "shcf", "shell", "shellcmdflag", "shellpipe", "shellquote",
    "shellredir", "shellslash", "shelltemp", "shelltype", "shellxescape", "shellxquote",
    "shiftround", "shiftwidth", "shm", "shortmess", "shortname", "showbreak", "showcmd",
    "showcmdloc", "showfulltag", "showmatch", "showmode", "showtabline", "showtabpanel", "shq",
    "si", "sidescroll", "sidescrolloff", "signcolumn", "siso", "sj", "slm", "sloc", "slow",
    "slowopen", "sm", "smartcase", "smartindent", "smarttab", "smc", "smd", "smoothscroll", "sms",
    "sn", "so", "softtabstop", "sol", "sop", "sourceany", "sp", "spc", "spell", "spellcapcheck",
    "spellfile", "spelllang", "spelloptions", "spellsuggest", "spf", "spk", "spl", "splitbelow",
    "splitkeep", "splitright", "spo", "spr", "sps", "sr", "srr", "ss", "ssl", "ssop", "st", "sta",
    "stal", "startofline", "statusline", "statuslineopt", "stl", "stlo", "stmp", "stpl", "sts",
    "su", "sua", "suffixes", "suffixesadd", "sw", "swapfile", "swapsync", "swb", "swf",
    "switchbuf", "sws", "sxe", "sxq", "syn", "synmaxcol", "syntax", "ta", "tabclose", "tabline",
    "tabpagemax", "tabpanel", "tabpanelopt", "tabstop", "tag", "tagbsearch", "tagcase", "tagfunc",
    "taglength", "tagrelative", "tags", "tagsecure", "tagstack", "tal", "tb", "tbi", "tbidi",
    "tbis", "tbs", "tc", "tcl", "tcldll", "tenc", "term", "termbidi", "termencoding",
    "termguicolors", "termresize", "termsync", "termwinkey", "termwinscroll", "termwinsize",
    "termwintype", "terse", "textauto", "textmode", "textwidth", "tf", "tfu", "tgc", "tgst",
    "thesaurus", "thesaurusfunc", "tildeop", "timeout", "timeoutlen", "title", "titlelen",
    "titleold", "titlestring", "tl", "tm", "to", "toolbar", "toolbariconsize", "top", "tpl",
    "tplo", "tpm", "tr", "trz", "ts", "tsc", "tsl", "tsr", "tsrfu", "tsy", "ttimeout",
    "ttimeoutlen", "ttm", "tty", "ttybuiltin", "ttyfast", "ttym", "ttymouse", "ttyscroll",
    "ttytype", "tw", "twk", "tws", "twsl", "twt", "tx", "uc", "udf", "udir", "ul", "undodir",
    "undofile", "undolevels", "undoreload", "updatecount", "updatetime", "ur", "ut",
    "varsofttabstop", "vartabstop", "vb", "vbs", "vdir", "ve", "verbose", "verbosefile", "vfile",
    "vi", "viewdir", "viewoptions", "vif", "viminfo", "viminfofile", "virtualedit", "visualbell",
    "vop", "vsts", "vts", "w1200", "w300", "w9600", "wa", "wak", "warn", "wb", "wc", "wcm", "wcr",
    "wd", "weirdinvert", "wfb", "wfh", "wfw", "wh", "whichwrap", "whl", "wi", "wic", "wig",
    "wildchar", "wildcharm", "wildignore", "wildignorecase", "wildmenu", "wildmode", "wildoptions",
    "wim", "winaltkeys", "wincolor", "window", "winfixbuf", "winfixheight", "winfixwidth",
    "winheight", "winhighlight", "winminheight", "winminwidth", "winptydll", "winwidth", "wiv",
    "wiw", "wlseat", "wlsteal", "wltimeoutlen", "wm", "wmh", "wmnu", "wmw", "wop", "wrap",
    "wrapmargin", "wrapscan", "write", "writeany", "writebackup", "writedelay", "ws", "wse", "wst",
    "wtm", "ww", "xtermcodes",
];

/// The two bytes after `t_` of every termcap OPTION in vim 9.2's `options[]`
/// (`optiondefs.h`'s `p_term()` rows), sorted bytewise. Any other `t_xx` is a
/// key code (`find_key_option`).
#[rustfmt::skip]
const P_TERM_CODES: &[&[u8]] = &[
    b"8b", b"8f", b"8u", b"AB", b"AF", b"AL", b"AU", b"BD", b"BE", b"BS", b"CF", b"CS", b"CV",
    b"Ce", b"Co", b"Cs", b"DL", b"Ds", b"EC", b"EI", b"ES", b"GP", b"IE", b"IS", b"RB", b"RC",
    b"RF", b"RI", b"RK", b"RS", b"RT", b"RV", b"Ri", b"SC", b"SH", b"SI", b"SR", b"ST", b"Sb",
    b"Sf", b"Si", b"TE", b"TI", b"Te", b"Ts", b"Us", b"VS", b"WP", b"WS", b"XM", b"ZH", b"ZR",
    b"al", b"bc", b"cd", b"ce", b"cl", b"cm", b"cs", b"da", b"db", b"dl", b"ds", b"fd", b"fe",
    b"fs", b"ke", b"ks", b"le", b"mb", b"md", b"me", b"mr", b"ms", b"nd", b"op", b"se", b"so",
    b"sr", b"te", b"ti", b"ts", b"u7", b"ue", b"us", b"ut", b"vb", b"ve", b"vi", b"vs", b"xn",
    b"xo", b"xs",
];

thread_local! {
    /// The `t_xx` values `:set` stored, by the two bytes after `t_` — the
    /// termcap options' values and the key codes `add_termcode()` adds. Silent
    /// Ex mode starts with none.
    static TERM_CODES: std::cell::RefCell<HashMap<Vec<u8>, Vec<u8>>> =
        std::cell::RefCell::new(HashMap::new());
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
/// The `P_COMMA`/`P_ONECOMMA`/`P_NODUP`/`P_FLAGLIST`/`P_EXPAND`/`P_COLON` bits of the
/// string options in [`OPTIONS`], from vim's `optiondefs.h`. Only `:set`'s
/// `+=`/`^=`/`-=` and its value copy read them.
const P_COMMA: u32 = 0x1;
const P_ONECOMMA: u32 = 0x2 | P_COMMA;
const P_NODUP: u32 = 0x4;
const P_FLAGLIST: u32 = 0x8;
const P_EXPAND: u32 = 0x10;
const P_COLON: u32 = 0x20;
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
    ("wildmode", P_ONECOMMA | P_NODUP | P_COLON),
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
/// RUST-PORT NOTE: a vim option this crate's reduced table does not hold
/// ([`VIM_OPTION_NAMES`]) is skipped silently — a plugin's `set cpo&vim` must
/// keep running — and a `t_xx` terminal code likewise; any other name is
/// `E518`. `set all`, `set all&` and `set termcap` do nothing. A query (`opt?`, a bare
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
        let find_dup = |origval: &str, newval: &str, comma_list: bool| -> Option<usize> {
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
        // c: stropt_handle_keymatch() (vim 9.2) — a P_COMMA|P_COLON option
        // takes an operand that holds a `key:value` item or several items one
        // item at a time: a `key:` item replaces (`+=`/`^=`) or removes (`-=`)
        // every item with that key, any other item is added unless present, or
        // removed. A single plain item takes the ordinary path below.
        let keymatched = (comma_list
            && flags & P_COLON != 0
            && (newval.contains(':') || newval.contains(',')))
        .then(|| {
            // c: find_key_item() — the first item starting with `key` (which
            // includes its ':'), as (offset, length up to the next comma).
            let find_key_item = |src: &str, from: usize, key: &str| -> Option<(usize, usize)> {
                let s = src.as_bytes();
                (from..s.len())
                    .find(|&p| {
                        (p == from || s[p - 1] == b',') && s[p..].starts_with(key.as_bytes())
                    })
                    .map(|p| {
                        (
                            p,
                            s[p..]
                                .iter()
                                .position(|&c| c == b',')
                                .unwrap_or(s.len() - p),
                        )
                    })
            };
            // c: remove_comma_item() — with the comma after it, or the one
            // before it when it is the last item.
            let remove_comma_item = |out: &mut String, item: usize, len: usize| {
                if out.as_bytes().get(item + len) == Some(&b',') {
                    out.replace_range(item..item + len + 1, "");
                } else if item > 0 && out.as_bytes()[item - 1] == b',' {
                    out.replace_range(item - 1..item + len, "");
                } else {
                    out.truncate(item);
                }
            };
            // c: remove_key_item() — every item with `key` except the one at
            // `skip`.
            let remove_key_item = |out: &mut String, key: &str, skip: Option<usize>| {
                while let Some((mut found, mut len)) = find_key_item(out, 0, key) {
                    if Some(found) == skip {
                        let next = found
                            + len
                            + usize::from(out.as_bytes().get(found + len) == Some(&b','));
                        match find_key_item(out, next, key) {
                            Some((f, l)) => (found, len) = (f, l),
                            None => break,
                        }
                    }
                    remove_comma_item(out, found, len);
                }
            };
            let mut out = origval.to_string();
            for item in newval.split(',').filter(|i| !i.is_empty()) {
                // c: append_item() / prepend_item()
                let add = |out: &mut String| {
                    *out = match (out.is_empty(), op) {
                        (true, _) => item.to_string(),
                        (false, b'^') => format!("{item},{out}"),
                        (false, _) => format!("{out},{item}"),
                    }
                };
                let Some(colon) = item.find(':') else {
                    match (op, find_dup(&out, item, true)) {
                        (b'-', Some(s)) => remove_comma_item(&mut out, s, item.len()),
                        (b'+' | b'^', None) => add(&mut out),
                        _ => {}
                    }
                    continue;
                };
                let key = &item[..=colon];
                match (op, find_key_item(&out, 0, key)) {
                    (b'-', _) => remove_key_item(&mut out, key, None),
                    // c: "Exact duplicate: keep it in place, but remove other
                    // items with the same key."
                    (_, Some((s, len))) if &out[s..s + len] == item => {
                        remove_key_item(&mut out, key, Some(s));
                    }
                    (_, found) => {
                        if found.is_some() {
                            remove_key_item(&mut out, key, None);
                        }
                        add(&mut out);
                    }
                }
            }
            out
        });
        let dup = if keymatched.is_none() && (op == b'-' || flags & P_NODUP != 0) {
            find_dup(origval, &newval, comma_list)
        } else {
            None
        };
        let mut out = match (keymatched, op) {
            (Some(k), _) => k,
            // c: "do not add if already there"
            (None, b'+' | b'^') if dup.is_some() => origval.to_string(),
            (None, b'+' | b'^') => {
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
            (None, _) => match dup {
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
    // The `opt?` values the argument being processed showed (c: `did_show`).
    let shown: std::cell::RefCell<Vec<(String, bool)>> = std::cell::RefCell::new(Vec::new());
    let mut did_show = false;
    // c: do_set_option() for a `t_xx` name `findoption()` does not hold here: a
    // termcap option (`t_AB`, a string option with no value in silent Ex mode)
    // or a key code (`TERMCAP2KEY` of the two bytes, `find_key_option`). Asking
    // for a key code that is not set is `E846`; `t_xx=value` sets either
    // (`add_termcode`). `value` is the text after `=`/`:`, when there is one.
    let set_term_code = |name: &[u8], value: Option<&[u8]>, query: bool| -> Option<String> {
        let termcap_opt = P_TERM_CODES.binary_search(&name).is_ok();
        if let Some(v) = value {
            // c: stropt_copy_value() — up to a blank, a backslash escaping the
            // next character.
            let mut val: Vec<u8> = Vec::new();
            let mut k = 0;
            while k < v.len() && v[k] != b' ' && v[k] != b'\t' {
                if v[k] == b'\\' && k + 1 < v.len() {
                    k += 1;
                }
                val.push(v[k]);
                k += 1;
            }
            // An empty key code is none: after `set t_xy=`, asking for it is E846
            // (measured on vim 9.2).
            TERM_CODES.with(|t| {
                let mut t = t.borrow_mut();
                if val.is_empty() && !termcap_opt {
                    t.remove(name);
                } else {
                    t.insert(name.to_vec(), val);
                }
            });
            return None;
        }
        if !query {
            return None;
        }
        let code = TERM_CODES.with(|t| t.borrow().get(name).cloned());
        let shown_code = |c: &[u8]| {
            let mut out = crate::vimstr::VimStr::new();
            crate::ported::message::msg_outtrans(c, &mut out);
            String::from_utf8_lossy(out.as_bytes()).into_owned()
        };
        let label = String::from_utf8_lossy(name);
        if termcap_opt {
            // c: showoneopt() of the string option.
            let text = format!("  t_{label}={}", shown_code(code.as_deref().unwrap_or(b"")));
            shown.borrow_mut().push((text, true));
            return None;
        }
        let Some(code) = code else {
            return Some("E846: Key code not set".to_string());
        };
        // c: show_one_termcode(). Not an informative message, so it is written
        // at once rather than buffered.
        let text =
            crate::ported::keycodes::show_one_termcode([name[0], name[1]], &shown_code(&code));
        shown.borrow_mut().push((text, false));
        None
    };
    let do_set_option = |arg: &str| -> Option<String> {
        const E474: &str = "E474: Invalid argument";
        const E488: &str = "E488: Trailing characters";
        const E521: &str = "E521: Number required after =";
        let white = |c: u8| c == b' ' || c == b'\t';
        let at = |s: &[u8], k: usize| s.get(k).copied().unwrap_or(0);

        // c: `:set all` / `:set all&` / `:set termcap` list (or reset) every
        // option before any name is looked up; neither is modelled.
        if (arg.starts_with("all") && !at(arg.as_bytes(), 3).is_ascii_alphabetic())
            || arg.starts_with("termcap")
        {
            return None;
        }
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
        // c: a `t_xx` name is four BYTES, the two after `t_` anything — so it can
        // end inside a multibyte character (`t_aé`), which no table here holds.
        let found = arg
            .get(..name_len)
            .and_then(findoption)
            .filter(|_| arg.is_char_boundary(name_len));
        let Some((canon, _, kind, _, _)) = found else {
            if s.starts_with(b"t_") && name_len == 4 {
                if prefix != 0 {
                    return Some(E474.to_string());
                }
                let value = matches!(nextchar, b'=' | b':').then(|| &s[len + 1..]);
                let query = nextchar == b'?' || !b"=:&<!".contains(&nextchar);
                if let Some(e) = set_term_code(&s[2..4], value, query) {
                    return Some(e);
                }
                if query && nextchar != b'?' && nextchar != 0 && !white(afterchar) {
                    return Some(E488.to_string());
                }
                return None;
            }
            let name = &arg[..name_len];
            if VIM_OPTION_NAMES.binary_search(&name).is_err() {
                return Some("E518: Unknown option".to_string());
            }
            // c: "hidden option: skip" — 'w300', 'w1200' and 'w9600' have no
            // value in any build; only asking for one is an error.
            if matches!(name, "w300" | "w1200" | "w9600") && !b"=:!&<".contains(&nextchar) {
                return Some("E519: Option not supported".to_string());
            }
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
            // c: showoneopt() — shown before the trailing-text check below, so
            // `set ts!` displays `tabstop=8` and then reports E488.
            shown
                .borrow_mut()
                .push((showoneopt(canon, *kind, opt_idx, varp, opt_flags), true));
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
                    let mut origval = match optval_from_varp(opt_idx, origvarp).data {
                        OptValData::string(s) => s,
                        _ => String::new(),
                    };
                    // c: opt_backspace_nr2str() — a Vim 5.x numeric 'backspace'
                    // is replaced by its words before the new value is made, so
                    // `set bs=3 | set bs=03` keeps "indent,eol,nostop".
                    if *canon == "backspace" && origval.starts_with(|c: char| c.is_ascii_digit()) {
                        let digits = origval.bytes().take_while(u8::is_ascii_digit).count();
                        origval = match origval[..digits].parse::<u32>() {
                            Ok(0) => "",
                            Ok(1) => "indent,eol",
                            Ok(2) => "indent,eol,start",
                            Ok(3) => "indent,eol,nostop",
                            _ => origval.as_str(),
                        }
                        .to_string();
                        set_option(opt_idx, STRING_OPTVAL(origval.clone()), opt_flags);
                    }
                    // c: stropt_copy_value() — a backslash escapes the next
                    // character, except before a file-name character of a P_EXPAND
                    // option (`set rtp=c:\dir`).
                    let mut raw = &arg.as_bytes()[p + 1..];
                    // c: opt_whichwrap_nr2str() — a Vim 3.0 numeric 'whichwrap'
                    // (`set ww=31`) is copied as its flag list; whatever follows
                    // the digits is skipped with the argument.
                    let ww_words;
                    if *canon == "whichwrap" && raw.first().is_some_and(u8::is_ascii_digit) {
                        let digits = raw.iter().take_while(|c| c.is_ascii_digit()).count();
                        let i: u64 = std::str::from_utf8(&raw[..digits])
                            .ok()
                            .and_then(|d| d.parse().ok())
                            .unwrap_or(u64::MAX);
                        let flags = [(1, "b"), (2, "s"), (4, "h,l"), (8, "<,>"), (16, "[,]")];
                        ww_words = flags
                            .iter()
                            .filter(|(bit, _)| i & bit != 0)
                            .map(|(_, w)| *w)
                            .collect::<Vec<_>>()
                            .join(",");
                        raw = ww_words.as_bytes();
                    }
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
        // c: each value goes on the message line as it is shown — the first
        // after `gotocmdline()`, each next one after `msg_putchar('\n')`.
        for (text, buffered) in shown.borrow_mut().drain(..) {
            crate::fusevm_bridge::set_show_option(&text, !did_show, buffered);
            did_show = true;
        }
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
    // c: do_set() `theend:` — "After displaying option values in silent mode."
    if did_show {
        crate::fusevm_bridge::set_show_end();
    }
}

/// Port of `showoneopt()` (vim `option.c:6462`) — the text `:set opt?` shows for
/// one option: `no`/`  ` and the name for a boolean, `  name=value` otherwise,
/// with the value as `option_value2string()` renders it (`'wildchar'` as a key
/// name, a `P_EXPAND` value with `$HOME` as `~`, unprintable characters `^X`).
fn showoneopt(canon: &str, kind: Kind, opt_idx: usize, varp: OptVarp, opt_flags: i32) -> String {
    use crate::ported::eval::typval::{tv_get_number_chk, tv_get_string};
    let val = if opt_flags == 0 {
        get_option_value(canon)
    } else {
        option_optval::optval_as_tv(optval_from_varp(opt_idx, varp), true)
    };
    if kind == Kind::Bool {
        let on = tv_get_number_chk(&val, None) != 0;
        return format!("{}{canon}", if on { "  " } else { "no" });
    }
    let mut value = tv_get_string(&val);
    if matches!(canon, "wildchar" | "wildcharm") {
        // c: wc_use_keyname() — a key with a `<Name>` is shown by it, any other
        // character through transchar().
        let n = tv_get_number_chk(&val, None);
        let key =
            crate::ported::keycodes::get_special_key_name(char::from_u32(n as u32).unwrap_or('\0'));
        value = if key.starts_with('<') && !key.starts_with("<C-") {
            key
        } else {
            String::from_utf8_lossy(&crate::ported::charset::transchar_buf(n as i32)).into_owned()
        };
    }
    let flags = STRING_FLAGS
        .iter()
        .find(|(n, _)| *n == canon)
        .map_or(0, |(_, f)| *f);
    if flags & P_EXPAND != 0 {
        // c: home_replace() — `$HOME` starting an item is shown as `~`.
        if let Some(home) = std::env::var("HOME").ok().filter(|h| !h.is_empty()) {
            value = value
                .split(',')
                .map(|item| match item.strip_prefix(home.as_str()) {
                    Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("~{rest}"),
                    _ => item.to_string(),
                })
                .collect::<Vec<_>>()
                .join(",");
        }
    }
    let mut shown = crate::vimstr::VimStr::new();
    crate::ported::message::msg_outtrans(value.as_bytes(), &mut shown);
    format!("  {canon}={}", String::from_utf8_lossy(shown.as_bytes()))
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

    /// `VIM_OPTION_NAMES` is searched with `binary_search`, which answers
    /// "absent" for a present name once the slice is out of order — and an
    /// absent vim option is `E518` on every `:set` of it. Every name of this
    /// port's own table must be in it too, since both are vim's.
    #[test]
    fn vim_option_names_sorted_and_cover_table() {
        assert!(
            VIM_OPTION_NAMES.windows(2).all(|w| w[0] < w[1]),
            "VIM_OPTION_NAMES must be sorted bytewise without duplicates"
        );
        for (full, abbr, ..) in OPTIONS {
            for key in [full, abbr] {
                assert!(
                    VIM_OPTION_NAMES.binary_search(key).is_ok(),
                    "{key:?} is in OPTIONS but not in VIM_OPTION_NAMES"
                );
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
