//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! Vim's regex dialect. The PARSER here is an extension — a recursive-descent
//! read of `:help pattern` in the default **magic** mode, with the other three
//! dialects translated into it — while the MATCHER is `viml_regex_nfa.rs`, a
//! port of `regexp_nfa.c` at vim 9.2 patch 1000 driven by this parser's AST.
//! It backs `=~`/`!~`, `matchstr()`, `match()`, `substitute()`, pattern
//! `split()`, and `:catch /pat/`.
//!
//! Two engines, because vim has two. `'regexpengine'` defaults to
//! "automatic": `nfa_regcomp()` is tried first and the backtracker
//! (`regexp_bt.c`) answers only when it bails out. [`Regex::find_from`] is
//! that dispatch and [`Regex::find_from_bt`] is the backtracker — a
//! continuation-passing matcher over the same AST, so a group's end is a
//! choice point like any other. Several answers are properties of WHICH
//! engine produced them; see the module docs of `viml_regex_nfa.rs`.
//!
//! Supported (magic mode): literals, `.`, `^`, `$`, `[...]`/`[^...]` with
//! ranges, the class atoms `\d \D \w \W \s \S \a \A \l \u \x \h \H \o \O`
//! (+negations) and the option-derived `\p \P \i \I \k \K` (default
//! `'isprint'`/`'isident'`/`'iskeyword'`; the uppercase forms exclude digits,
//! per `:help /\P`, and are NOT set-complements),
//! quantifiers `* \+ \? \= \{n,m}` and the non-greedy `\{-n,m}`, groups
//! `\(...\)` (capturing) and `\%(...\)` (non-capturing) with `\|` alternation,
//! backreferences `\1`..`\9`, word boundaries `\< \>`, the `\@` family
//! (`\@=` `\@!` `\@<=` `\@<!` `\@>`, incl. the `\@123<=` limit), and case
//! control `\c`/`\C` plus the caller's ignore-case flag.
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use crate::ported::mbyte::mb_get_class;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// The ported NFA engine (`regexp_nfa.c`). Declared as a child module rather
/// than a sibling so it can read the AST types above without widening their
/// visibility.
#[path = "viml_regex_nfa.rs"]
mod nfa;

/// `\=`-replacement expression evaluator hook (`expr -> string`).
type SubstExprFn = fn(&str) -> String;

/// A cache entry: the compiled pattern, and the diagnostic it raises (kept so a
/// cache hit still reports it).
type CachedRegex = (Rc<Regex>, Option<String>);

thread_local! {
    /// Hook (installed by the bridge) that evaluates a `\=`-prefixed substitute
    /// replacement *expression* to its string result. `submatch()` reads
    /// [`SUBMATCHES`] while this runs. `None` when no evaluator is wired (then a
    /// `\=` replacement falls back to literal text).
    pub static SUBST_EXPR_HOOK: RefCell<Option<SubstExprFn>> =
        const { RefCell::new(None) };
    /// Groups of the match currently being replaced (index 0 = whole match),
    /// exposed to a `\=` expression through `submatch()`.
    static SUBMATCHES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Compiled patterns, keyed by the pattern text — see [`Regex::compile`].
    /// The `Option<String>` is the diagnostic the pattern raises, kept so a
    /// cache hit still reports it.
    static REGEX_CACHE: RefCell<HashMap<String, CachedRegex>> =
        RefCell::new(HashMap::new());
}

/// `submatch({n})` — the text of group `n` of the match a `substitute(…, '\=…')`
/// expression is currently replacing (`""` when out of range).
pub fn current_submatch(n: usize) -> String {
    SUBMATCHES.with(|s| s.borrow().get(n).cloned().unwrap_or_default())
}

/// Whether a `\=` substitute expression is currently being evaluated (so
/// `submatch()` has a real match context). Outside one, `submatch(n, 1)` yields
/// an empty list rather than a one-empty-line list.
pub fn has_submatch_context() -> bool {
    SUBMATCHES.with(|s| !s.borrow().is_empty())
}

/// A character class: `[...]`/`[^...]` or a `\d`-style atom.
#[derive(Debug, Clone)]
struct Class {
    negated: bool,
    items: Vec<ClassItem>,
}

#[derive(Debug, Clone)]
enum ClassItem {
    Ch(char),
    Range(char, char),
    Digit,
    Word,
    Space,
    Alpha,
    Lower,
    Upper,
    Hex,
    /// `\h` — head-of-word char `[A-Za-z_]` (ASCII, `:help /\h`).
    Head,
    /// `\o` — octal digit `[0-7]`.
    Octal,
    /// `\p` — printable char (default `'isprint'` + Vim's multibyte width rule).
    Print,
    /// `\P` — `\p` but excluding digits (Vim's `\P` is NOT a negation of `\p`).
    PrintNoDigit,
    /// `\i` — identifier char (default `'isident'`); single-byte only.
    Ident,
    /// `\I` — `\i` but excluding digits.
    IdentNoDigit,
    /// `\k` — keyword char (default `'iskeyword'`); multibyte-aware.
    Keyword,
    /// `\K` — `\k` but excluding digits.
    KeywordNoDigit,
    /// `\f` — filename char (default `'isfname'`); multibyte-aware.
    Fname,
    /// `\F` — `\f` but excluding digits.
    FnameNoDigit,
    /// `[:alnum:]` — ASCII letters/digits `[0-9A-Za-z]` (no `_`, unlike `\w`).
    Alnum,
    /// `[:blank:]` — space or tab only (`:help /[:blank:]`).
    Blank,
    /// `[:cntrl:]` — ASCII control chars `0x00`–`0x1F` and DEL `0x7F`.
    Cntrl,
    /// `[:graph:]` — ASCII printable non-space `0x21`–`0x7E` (ASCII-only).
    Graph,
    /// `[:punct:]` — ASCII punctuation (`is_ascii_punctuation`, includes `_`).
    Punct,
    /// `[:lower:]` — Unicode lowercase (é/ÿ/я match; unlike ASCII-only `\l`). Vim
    /// classifies multibyte case via `utf_islower`; approximated with
    /// `char::is_lowercase`. Known divergence: titlecase digraphs (ǅ/ǈ/ǋ) match
    /// both `[:lower:]` and `[:upper:]` in Vim but neither `is_lowercase` nor
    /// `is_uppercase` in Rust.
    LowerU,
    /// `[:upper:]` — Unicode uppercase (À/Ω/Я match; unlike ASCII-only `\u`).
    /// `char::is_uppercase`; same titlecase divergence as `LowerU`.
    UpperU,
    /// `[:space:]` — POSIX whitespace: space, tab, nl, cr, vertical-tab, form-feed
    /// (`0x09`–`0x0D` + space). Wider than `\s` (space/tab) and includes `\x0B`,
    /// which Rust's `is_ascii_whitespace` omits.
    Whitespace,
}

impl ClassItem {
    /// Whether case-fold (`\c` / 'ignorecase') applies to this set member.
    /// Verified against vim 9.2 / nvim 0.12.3: folding rewrites LITERAL set
    /// members (`[abc]`, `[A-Z]`, `\ca`) so `\c` matches either case, but a
    /// case-*defined* predicate keeps its own definition — a lowercase char
    /// must NOT match `[[:upper:]]` / `\u` under `\c`. So only `Ch`/`Range`
    /// fold; every predicate variant (Upper/Lower/UpperU/LowerU/Digit/…) does
    /// not. Case-agnostic predicates (`\d \w \a \x`) are no-ops either way.
    fn folds_under_ic(&self) -> bool {
        matches!(self, ClassItem::Ch(_) | ClassItem::Range(_, _))
    }

    fn contains(&self, c: char) -> bool {
        match self {
            ClassItem::Ch(x) => *x == c,
            ClassItem::Range(a, b) => *a <= c && c <= *b,
            // Vim's `\d \w \a \l \u \x` are ASCII-only regardless of locale
            // (`:help /\a`): `\a`=[A-Za-z], `\w`=[0-9A-Za-z_], `\l`=[a-z],
            // `\u`=[A-Z]. Unicode-aware predicates would wrongly match é/Ω/４.
            // (Multibyte word chars only matter for `\<`/`\>` and `\k`, which go
            // through `is_word`/iskeyword, not these class atoms.)
            ClassItem::Digit => c.is_ascii_digit(),
            ClassItem::Word => c.is_ascii_alphanumeric() || c == '_',
            ClassItem::Space => c == ' ' || c == '\t',
            ClassItem::Alpha => c.is_ascii_alphabetic(),
            ClassItem::Lower => c.is_ascii_lowercase(),
            ClassItem::Upper => c.is_ascii_uppercase(),
            ClassItem::Hex => c.is_ascii_hexdigit(),
            // `\h` head-of-word: ASCII letter or `_`, no digits (unlike `\w`).
            ClassItem::Head => c.is_ascii_alphabetic() || c == '_',
            ClassItem::Octal => ('0'..='7').contains(&c),
            ClassItem::Print => is_printable(c),
            // Vim's `\P \I \K` mean "like the lowercase form but excluding
            // digits" (`:help /\P`), NOT the set-complement `\p`/`\i`/`\k`
            // negation. So these are positive predicates, not `negated` classes.
            ClassItem::PrintNoDigit => is_printable(c) && !c.is_ascii_digit(),
            ClassItem::Ident => is_ident_char(c),
            ClassItem::IdentNoDigit => is_ident_char(c) && !c.is_ascii_digit(),
            ClassItem::Keyword => is_keyword_char(c),
            ClassItem::KeywordNoDigit => is_keyword_char(c) && !c.is_ascii_digit(),
            ClassItem::Fname => is_fname_char(c),
            ClassItem::FnameNoDigit => is_fname_char(c) && !c.is_ascii_digit(),
            // POSIX bracket classes `[[:name:]]` (`:help /[:alpha:]`). ASCII-ness
            // matches Vim/nvim empirically: alnum/graph/punct are ASCII-only,
            // `[:print:]` reuses `is_printable` (multibyte-aware).
            ClassItem::Alnum => c.is_ascii_alphanumeric(),
            ClassItem::Blank => c == ' ' || c == '\t',
            ClassItem::Cntrl => c.is_ascii_control(),
            ClassItem::Graph => c.is_ascii_graphic(),
            ClassItem::Punct => c.is_ascii_punctuation(),
            ClassItem::LowerU => c.is_lowercase(),
            ClassItem::UpperU => c.is_uppercase(),
            // Explicit set: `\x0B` (vertical tab) is whitespace to Vim but not to
            // Rust's `char::is_ascii_whitespace`, so it can't be reused here.
            ClassItem::Whitespace => matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c'),
        }
    }
}

/// `\p` printable test — default `'isprint'` (`@,161-255`) plus Vim's multibyte
/// width rule. Empirically (nvim 0.12.3 / vim 9.2, default `'isprint'`): ASCII
/// `0x20`–`0x7E` is always printable; `0x7F`–`0x9F` (DEL + C1 controls) are not;
/// `0xA0` (NBSP) and every char above it — including all multibyte and combining
/// marks — are printable. Known divergence: Vim treats a few zero-width format
/// chars (U+200B ZWSP … U+200D ZWJ) as non-printable; this predicate does not.
fn is_printable(c: char) -> bool {
    let n = c as u32;
    (0x20..=0x7E).contains(&n) || n >= 0xA0
}

/// Single-byte membership shared by `\i` (default `'isident'`) and `\k` (default
/// `'iskeyword'`) — both default to `@,48-57,_,192-255`: ASCII letters/digits,
/// `_`, and bytes `0xC0`–`0xFF` (this range is numeric, so it includes
/// non-letters like `×` U+00D7). Verified against nvim 0.12.3 / vim 9.2. `\i`
/// uses exactly this (no multibyte membership); `\k` widens it below.
fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || matches!(c as u32, 0xC0..=0xFF)
}

/// `\k` keyword test — the `\i` single-byte set OR a multibyte word char. Vim
/// classifies multibyte (`> 0xFF`) via `utf_class`; this approximates that with
/// `char::is_alphanumeric` (verified: Ω/中/あ/é match `\k`, `←`/`∀`/`•`/spaces do
/// not). Known divergence: Vim's `utf_class` also flags emoji and combining
/// marks as keyword chars; `is_alphanumeric` does not.
fn is_keyword_char(c: char) -> bool {
    is_ident_char(c) || ((c as u32) > 0xFF && c.is_alphanumeric())
}

/// `\f` filename test — default `'isfname'`, which on Unix is
/// `@,48-57,/,.,-,_,+,,,#,$,%,~,=`: the `@` class (alphabetic, multibyte
/// included), digits, `_`, and that punctuation set. Enumerated char by char
/// against vim 9.2 over `0x20..=0x7E`, which yields exactly
/// `#$%+,-./0123456789=A-Z_a-z~`; `é` and `中` also match, `' '` does not.
fn is_fname_char(c: char) -> bool {
    is_keyword_char(c) || matches!(c, '#' | '$' | '%' | '+' | ',' | '-' | '.' | '/' | '=' | '~')
}

impl Class {
    /// c: `wants_nfa`, set by `nfa_regatom()` at `regexp_nfa.c:1858` and
    /// `:1871` — and only there. `[[:upper:]]` and `[[:lower:]]` do not work
    /// with characters above 8 bits in the backtracking engine, so a pattern
    /// containing one must not take the `\{n,m}` bail-out that hands the
    /// pattern to that engine.
    fn wants_nfa(&self) -> bool {
        self.items
            .iter()
            .any(|it| matches!(it, ClassItem::LowerU | ClassItem::UpperU))
    }

    fn matches(&self, c: char, ic: bool) -> bool {
        let hit = self.items.iter().any(|it| {
            if ic && it.folds_under_ic() {
                it.contains(c.to_ascii_lowercase()) || it.contains(c.to_ascii_uppercase())
            } else {
                it.contains(c)
            }
        });
        hit ^ self.negated
    }
}

/// The capture/`\zs`/`\ze` slots a match fills in: `1..=ngroups` are the groups,
/// then two working slots for the match-start and match-end marks.
type Groups = Vec<Option<(usize, usize)>>;

/// A match continuation: given where the piece just matched ended and the group
/// state it left, either accept (`Some(end)` — the whole match is done) or
/// reject (`None`) and make the caller offer its next alternative.
type Cont<'a> = &'a mut dyn FnMut(usize, &mut Groups) -> Option<usize>;

/// What a match is being run against: the subject and the case-folding flag.
///
/// Bundled because they are the two things every step of the continuation-passing
/// matcher needs and neither ever changes during one match.
#[derive(Clone, Copy)]
struct Subject<'a> {
    text: &'a [char],
    ic: bool,
}

#[derive(Debug, Clone)]
enum Node {
    Lit(char),
    Any,
    Bol,
    Eol,
    /// Word boundary: `true` = `\<` (start), `false` = `\>` (end).
    WordB(bool),
    /// `\zs` — zero-width; moves the start of the whole match to here.
    MatchStart,
    /// `\ze` — zero-width; moves the end of the whole match to here.
    MatchEnd,
    /// `\1`..`\9` — backreference to the text captured by that group.
    BackRef(usize),
    /// `\%[atoms]` — a sequence of optionally-matched atoms; matches the longest
    /// in-order prefix of them (greedy), including the empty match.
    OptSeq(Vec<Node>),
    Class(Class),
    /// Alternation of branches; `Some(idx)` = capturing group index.
    Group(Vec<Branch>, Option<usize>),
    /// `\@` family — the preceding atom wrapped in a lookaround assertion or an
    /// atomic group (c: `nfa_regpiece` `case Magic('@')`).
    Look(Box<Atom>, LookOp),
    /// Internal (match-time only): zero-width, succeeds only at this char index.
    /// Appended after a lookbehind's atom to force it to end exactly at the
    /// assertion position.
    CheckPos(usize),
    /// `\%C` — c: `NFA_ANY_COMPOSING`. Skips over a composing character when
    /// there is one at this position, and matches without consuming otherwise.
    AnyComposing,
    /// `\%^` / `\%$` — zero-width, start / end of the FILE (`:help /\%^`).
    /// This engine matches a single string, which is the whole "file", so they
    /// are the ends of the subject. `true` = start.
    FileEnd(bool),
    /// `\%23c` / `\%>23c` / `\%<23c` — zero-width, the byte column (1-based)
    /// the match must be at, before, or after (`:help /\%c`).
    Col(Cmp, u32),
}

/// How a `\%…c` atom compares the current column against its number.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Cmp {
    /// `\%23c` — exactly.
    Eq,
    /// `\%>23c` — after.
    Gt,
    /// `\%<23c` — before.
    Lt,
}

/// Which `\@` operator wraps the atom.
#[derive(Debug, Clone)]
enum LookOp {
    /// `\@=` (`true`) / `\@!` (`false`) — zero-width (negative) lookahead
    /// (c: `NFA_PREV_ATOM_NO_WIDTH` / `NFA_PREV_ATOM_NO_WIDTH_NEG`).
    Ahead(bool),
    /// `\@<=` (`true`) / `\@<!` (`false`) — zero-width (negative) lookbehind,
    /// with the optional `\@123<=` distance limit
    /// (c: `NFA_PREV_ATOM_JUST_BEFORE` / `NFA_PREV_ATOM_JUST_BEFORE_NEG`).
    Behind(bool, Option<u32>),
    /// `\@>` — match the atom like a standalone pattern, consuming it, with no
    /// backtracking into it (c: `NFA_PREV_ATOM_LIKE_PATTERN`).
    Atomic,
}

/// A quantified atom.
#[derive(Debug, Clone)]
struct Atom {
    node: Node,
    min: u32,
    max: u32,
    greedy: bool,
}

type Branch = Vec<Atom>;

/// A compiled Vim regex (top-level alternation of branches).
pub struct Regex {
    branches: Vec<Branch>,
    ngroups: usize,
    /// Forced case from `\c` (Some(true)) / `\C` (Some(false)); else None.
    forced_ic: Option<bool>,
    /// The pattern was invalid: the error has been reported and this regex matches
    /// nothing, so every caller falls back to its no-match result — which is what
    /// Vim's functions return once they have raised the error.
    dead: bool,
    /// The same pattern compiled for the ported NFA engine, or `None` when
    /// `nfa::Prog::compile` bailed. c: `vim_regcomp()` under the automatic
    /// engine — `nfa_regcomp()` first, `bt_regcomp()` when it returns NULL.
    nfa: Option<nfa::Prog>,
}

/// A successful match: char-index span plus per-group spans (index 0 = whole).
#[derive(Debug, Clone)]
pub struct Captures {
    /// `[whole, group1, group2, …]` — each `Some((start, end))` in char indices.
    pub groups: Vec<Option<(usize, usize)>>,
}

impl Captures {
    /// Char span of the whole match.
    pub fn whole(&self) -> (usize, usize) {
        self.groups[0].unwrap_or((0, 0))
    }
}

const INF: u32 = u32::MAX;

/// Rewrite a `\v` (very-magic) segment into the equivalent default-magic
/// pattern, so the magic-mode parser handles it unchanged. In very-magic mode
/// every ASCII punctuation char is an operator (no backslash needed) and a
/// backslash makes it literal — the inverse of magic mode for
/// `( ) | + ? = { } < >`. `\v` switches very-magic on for the rest of the
/// pattern; `\m` switches back. Character classes `[...]` are copied verbatim.
/// (Exotic `\v` atoms — `@`, `&`, `%[` — are left as-is; not yet modelled.)
/// Translate a pattern into the magic dialect the parser reads, and report a
/// misplaced multi that only the *translation* can see.
///
/// A bare `*` at the start of a branch is a literal star in magic (`match('a*b','*')`
/// finds it), but in nomagic the special star is written `\*` — and there it IS a
/// multi, so `\M\*` has nothing to repeat and Vim rejects it (E866). Both end up as
/// a magic `*` after translation, so the parser can no longer tell them apart; this is
/// the only place that still can.
/// Returns the translated pattern, a parallel "this character was written in
/// very magic" map (one entry per translated char), a parallel map from each
/// translated character back to the ORIGINAL pattern index that produced it,
/// and the diagnostic only the translation can see.
///
/// The map exists because Vim's *diagnostics* quote the pattern in the dialect
/// it was written in: an unclosed group is `E54: Unmatched \(` in magic and
/// `E54: Unmatched (` under `\v` (c: `EMSG_M_RET_NULL(e_unmatched_str_open, ...)`,
/// which formats the message with `""` when `reg_magic == MAGIC_ALL` and `"\\"`
/// otherwise). Translating everything into magic threw that away, so `\v(` was
/// reported with a backslash that is not in the user's pattern.
fn preprocess_magic(pat: &str) -> Preprocessed {
    // Nothing to do for the default (magic) dialect, which is what the parser
    // reads — but a `\m` still has to be REMOVED even when it only restores the
    // dialect the pattern already had, or the parser reads it as an atom. It was
    // missing from this guard, so `matchend('X', '.\m\%[ab]')` looked for a
    // literal `m` and answered -1 where vim answers 3.
    if !pat.contains("\\v") && !pat.contains("\\m") && !pat.contains("\\M") && !pat.contains("\\V")
    {
        let n = pat.chars().count();
        return (pat.to_string(), vec![false; n], (0..n).collect(), None);
    }
    let chars: Vec<char> = pat.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut mode = Dialect::Magic;
    // Whether an atom has been emitted in the current branch — a multi needs one.
    let mut atom_before = false;
    // The diagnostic AND where in the translated pattern it was seen. Vim reports
    // the first violation in pattern order, and this one is found in a pre-pass
    // that runs before the parser — so without a position it always won, and
    // `\M\1\(\*` reported "E866: Misplaced *" where vim reports the earlier
    // "E65: Illegal back reference". See `compile_uncached`.
    let mut err: Option<(usize, String)> = None;
    const OPS: &str = "(){}+?=|<>@";
    // Filled to `out`'s length after each iteration, with the mode that produced
    // those characters — cheaper and less error-prone than tagging every push.
    let mut vm: Vec<bool> = Vec::new();
    // Filled the same way, with the ORIGINAL index of the atom that produced
    // those translated characters. `seen_endbrace()` scans the pattern the user
    // wrote, not a translation of it, so the E65 escape hatch needs a way back.
    let mut omap: Vec<usize> = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        let start_i = i;
        let mark = |vm: &mut Vec<bool>, out: &String, mode: Dialect| {
            vm.resize(out.chars().count(), mode == Dialect::VeryMagic);
        };
        // A mode switch can appear anywhere in the pattern and applies from there on.
        if c == '\\' {
            match chars.get(i + 1) {
                Some('v') => {
                    mode = Dialect::VeryMagic;
                    i += 2;
                    continue;
                }
                Some('m') => {
                    mode = Dialect::Magic;
                    i += 2;
                    continue;
                }
                Some('M') => {
                    mode = Dialect::NoMagic;
                    i += 2;
                    continue;
                }
                Some('V') => {
                    mode = Dialect::VeryNoMagic;
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        match mode {
            Dialect::Magic => {
                // Already the dialect the parser reads: copy an escape pair whole so a
                // following char is never mistaken for a bare one.
                out.push(c);
                i += 1;
                if c == '\\' && i < chars.len() {
                    out.push(chars[i]);
                    i += 1;
                }
            }
            // `\M` (nomagic) and `\V` (very nomagic) differ from magic only in WHICH
            // characters are special (`:help /magic`): in nomagic `.` `*` `~` `[` are
            // literal and `\.` `\*` … are the special ones — the escaping is simply
            // swapped. Very nomagic swaps `^` and `$` as well, so `\V^` is a literal
            // caret. Everything else (`\(`, `\|`, `\zs`, `\d`, …) is identical in
            // all four, which is why translating into magic is enough for the parser.
            Dialect::NoMagic | Dialect::VeryNoMagic => {
                let swapped: &str = if mode == Dialect::NoMagic {
                    ".*~["
                } else {
                    ".*~[^$"
                };
                if c == '\\' {
                    match chars.get(i + 1) {
                        Some(&n) if swapped.contains(n) => {
                            // c: "E866: (NFA regexp) Misplaced *" — the nomagic special
                            // star is a multi, and there is nothing before it to repeat.
                            if n == '*' && !atom_before && err.is_none() {
                                err = Some((
                                    out.chars().count(),
                                    "E866: (NFA regexp) Misplaced *".to_string(),
                                ));
                            }
                            if n != '*' {
                                atom_before = true;
                            }
                            out.push(n); // `\.` in nomagic IS the magic `.`
                            i += 2;
                        }
                        Some(&n) => {
                            // `\|` and `\(` open a new branch: the multi rule restarts.
                            atom_before = !matches!(n, '|' | '(' | '&');
                            out.push('\\');
                            out.push(n);
                            i += 2;
                            // `\%[`, `\%(`, `\%d97` and `\_.`, `\_[a-z]`, `\_s` — the
                            // char after `%` or `_` belongs to the escape, so it must be
                            // copied raw. Literal-izing it turned `\M\_.` into `\_\.`
                            // (an "any char incl newline" followed by a literal dot),
                            // which matched nothing.
                            if n == '%' || n == '_' {
                                if let Some(&after) = chars.get(i) {
                                    // `\%(` opens a group — a new branch, so a multi
                                    // right after it again has nothing to repeat.
                                    if n == '%' {
                                        atom_before = after != '(';
                                    }
                                    out.push(after);
                                    i += 1;
                                }
                            }
                        }
                        None => {
                            out.push('\\');
                            i += 1;
                        }
                    }
                } else if swapped.contains(c) {
                    atom_before = true;
                    out.push('\\'); // a bare `.` in nomagic is a literal dot
                    out.push(c);
                    i += 1;
                } else {
                    atom_before = true;
                    out.push(c);
                    i += 1;
                }
            }
            Dialect::VeryMagic => {
                match c {
                    '\\' => {
                        if let Some(&n) = chars.get(i + 1) {
                            if OPS.contains(n) || n == '%' {
                                out.push(n); // `\(` → literal '(' (bare in magic)
                            } else {
                                out.push('\\'); // keep `\d`, `\zs`, `\1`, `\\`, …
                                out.push(n);
                            }
                            i += 2;
                        } else {
                            out.push('\\');
                            i += 1;
                        }
                    }
                    '[' => {
                        // Copy the class verbatim (internals are mode-independent).
                        out.push('[');
                        i += 1;
                        if chars.get(i) == Some(&'^') {
                            out.push('^');
                            i += 1;
                        }
                        if chars.get(i) == Some(&']') {
                            out.push(']');
                            i += 1;
                        }
                        while i < chars.len() && chars[i] != ']' {
                            if chars[i] == '\\' && i + 1 < chars.len() {
                                out.push('\\');
                                out.push(chars[i + 1]);
                                i += 2;
                            } else {
                                out.push(chars[i]);
                                i += 1;
                            }
                        }
                        if i < chars.len() {
                            out.push(']');
                            i += 1;
                        }
                    }
                    // A bare `%` is Magic('%') under `\v` (c: `peekchr()` lists `%`
                    // among the chars that are "magic only after \v"), so the WHOLE
                    // `\%…` atom family is spelled without the backslash: `%(`, `%[`,
                    // `%d97`, `%^`, `%$`, `%23c`, `%V`, `%#`. Only `%(` was being
                    // translated, so `\v%[ab]` was read as a literal `%` followed by a
                    // collection and `\v%d97` as a literal `%d97`.
                    //
                    // The character that follows belongs to the atom, and the ones the
                    // generic OPS rule below would rewrite (`(`, `<`, `>`) must be
                    // copied raw — `\v%<2c` must not become `\%\<2c`. Everything
                    // deeper (a `%(`'s branches, a `%[`'s atoms) is still very magic
                    // and is left to the loop.
                    '%' => {
                        out.push_str("\\%");
                        i += 1;
                        if let Some(&n) = chars.get(i) {
                            if OPS.contains(n) {
                                out.push(n);
                                i += 1;
                            }
                        }
                    }
                    // `\v(foo)@<=bar` — a bare `@` is the lookaround operator in
                    // very magic. Its optional decimal limit and operator chars
                    // (`=` `!` `>` `<=` `<!`) belong to the `\@` escape, so copy
                    // them raw — the generic OPS rule below would re-translate
                    // `<`/`=`/`>` into `\<`/`\=`/`\>` and break the parse.
                    '@' => {
                        out.push_str("\\@");
                        i += 1;
                        while chars.get(i).is_some_and(|ch| ch.is_ascii_digit()) {
                            out.push(chars[i]);
                            i += 1;
                        }
                        if chars.get(i) == Some(&'<') {
                            out.push('<');
                            i += 1;
                        }
                        if let Some(&opc) = chars.get(i) {
                            if matches!(opc, '=' | '!' | '>') {
                                out.push(opc);
                                i += 1;
                            }
                        }
                    }
                    _ if OPS.contains(c) => {
                        out.push('\\'); // operator → magic backslash form
                        out.push(c);
                        i += 1;
                    }
                    _ => {
                        out.push(c); // . * ^ $ ~ and word chars are the same in both
                        i += 1;
                    }
                }
            }
        }
        mark(&mut vm, &out, mode);
        omap.resize(out.chars().count(), start_i);
    }
    omap.resize(out.chars().count(), chars.len());
    (out, vm, omap, err)
}

/// What [`preprocess_magic`] hands the parser: the translated pattern, the
/// per-character "written in very magic" map, the translated-index →
/// original-index map, and the diagnostic only the translation can see.
type Preprocessed = (String, Vec<bool>, Vec<usize>, Option<(usize, String)>);

/// The four pattern dialects (`:help /magic`). The parser reads [`Dialect::Magic`];
/// `preprocess_magic` translates the other three into it.
#[derive(Clone, Copy, PartialEq)]
enum Dialect {
    VeryMagic,
    Magic,
    NoMagic,
    VeryNoMagic,
}

// ── parser (magic mode) ──

struct Parser {
    p: Vec<char>,
    /// Per-character "written in very magic", parallel to `p`. Only the
    /// unmatched-group diagnostics read it; see [`preprocess_magic`].
    vm: Vec<bool>,
    /// The pattern as the user wrote it, and the translated-index →
    /// original-index map. Only [`Parser::lookbehind_ahead`] reads them.
    orig: Vec<char>,
    omap: Vec<usize>,
    i: usize,
    ngroups: usize,
    forced_ic: Option<bool>,
    /// Groups that have been *closed* so far. A backreference is legal only once its
    /// group is complete: Vim rejects `\(a\1\)` (E65) while `\(\(a\)\2\)` is fine, so
    /// counting *opened* groups is not enough.
    closed: Vec<usize>,
    /// The first `E<nnn>` the pattern violates. Vim *rejects* an invalid pattern —
    /// a backreference to a group that does not exist, a quantifier on `\zs`, a
    /// quantifier on a quantifier, an unclosed `\(` — and every function that takes
    /// one raises the error rather than quietly finding nothing, which is what this
    /// engine used to do.
    err: Option<String>,
    /// Where in `p` the recorded violation was seen. Only used to order the
    /// parser's first error against the pre-pass one; see `compile_uncached`.
    err_pos: usize,
}

impl Parser {
    /// Record the first violation; later ones are noise once the pattern is invalid.
    fn fail(&mut self, msg: &str) {
        if self.err.is_none() {
            self.err = Some(msg.to_string());
            self.err_pos = self.i;
        }
    }

    fn peek(&self) -> Option<char> {
        self.p.get(self.i).copied()
    }
    fn peek2(&self) -> Option<char> {
        self.p.get(self.i + 1).copied()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.i += 1;
        }
        c
    }

    /// `branch \| branch \| …`.
    /// `star_lit` is c: `peekchr()`'s "`*` is not magic … after `\\(`, `\\|`, `\\&`"
    /// exception, carried in from whatever opened this branch. It holds for the
    /// first branch only when the opener made `prevchr` a MAGIC `(`; every later
    /// branch is preceded by a magic `\\|` or `\\&`, so it always holds there.
    fn alternation(&mut self, star_lit: bool) -> Vec<Branch> {
        let mut branches = vec![self.and_branch(star_lit)];
        while self.peek() == Some('\\') && self.peek2() == Some('|') {
            self.i += 2;
            branches.push(self.and_branch(true));
        }
        branches
    }

    /// `concat \& concat \& …` (c: `nfa_regbranch`). All `\&`-joined concats must
    /// match at the same position; the result is the LAST concat's match. Each
    /// leading concat is compiled to a zero-width positive lookahead
    /// (`\(concat\)\@=`) and the last concat is inlined as the consuming match, so
    /// `foo\&...` == `\(foo\)\@=...` (matchstr → 'foo').
    fn and_branch(&mut self, star_lit: bool) -> Branch {
        let first = self.concat(star_lit);
        if !(self.peek() == Some('\\') && self.peek2() == Some('&')) {
            return first;
        }
        let mut concats = vec![first];
        while self.peek() == Some('\\') && self.peek2() == Some('&') {
            self.i += 2;
            concats.push(self.concat(true));
        }
        // The last concat consumes; the earlier ones are zero-width AND assertions.
        let last = concats.pop().unwrap_or_default();
        let mut out: Branch = Vec::new();
        for c in concats {
            let group = Atom {
                node: Node::Group(vec![c], None),
                min: 1,
                max: 1,
                greedy: true,
            };
            out.push(Atom {
                node: Node::Look(Box::new(group), LookOp::Ahead(true)),
                min: 1,
                max: 1,
                greedy: true,
            });
        }
        out.extend(last);
        out
    }

    /// A sequence of quantified atoms, stopping at `\|`, `\)`, or end.
    fn concat(&mut self, star_lit: bool) -> Branch {
        let mut atoms = Vec::new();
        loop {
            match (self.peek(), self.peek2()) {
                (None, _) => break,
                (Some('\\'), Some('|')) | (Some('\\'), Some(')')) | (Some('\\'), Some('&')) => {
                    break
                }
                _ => {}
            }
            // c: "E866: (NFA regexp) Misplaced +" — a multi at the start of a branch has
            // nothing to repeat. (A bare `*` there is NOT an error: magic treats a
            // leading star as a literal, which is why `match('a*b', '*')` finds it.
            // The nomagic special star `\*` IS a multi and is caught in
            // `preprocess_magic`, which is the only place that can still tell them
            // apart.)
            if atoms.is_empty() && self.peek() == Some('\\') {
                if let Some(m) = self.peek2() {
                    if matches!(m, '+' | '=' | '?' | '{' | '@') {
                        self.fail(&format!("E866: (NFA regexp) Misplaced {m}"));
                        break;
                    }
                }
            }
            // c: `peekchr()` `case '*'` — a bare `*` is a literal at the start of a
            // branch only when `prevchr` is a MAGIC `(`, `&` or `|`. `\(`, `\|`,
            // `\&` and (very magic) `(` all produce one; `\%(` does NOT, because
            // its `(` is read by `getchr()` in a dialect where `(` is not magic, so
            // `\%(*a\)` is "E866: Misplaced *" while `\(*a\)` and `\v%(*a)` keep a
            // literal star. `preprocess_magic` folds `\v%(` into `\%(`, so the two
            // are told apart by the per-character very-magic provenance in `vm`.
            if atoms.is_empty() && !star_lit && self.peek() == Some('*') {
                self.fail("E866: (NFA regexp) Misplaced *");
                break;
            }
            match self.quantified(atoms.is_empty()) {
                Some(a) => atoms.push(a),
                None => break,
            }
        }
        atoms
    }

    /// An atom plus an optional quantifier. `at_start` enables `^` as anchor.
    fn quantified(&mut self, at_start: bool) -> Option<Atom> {
        // doc: `:help /star` — "Exception: When "*" is used at the start of the
        // pattern or just after "^" it matches the star character."
        //
        // The start-of-branch half of that rule already falls out of `atom()`:
        // with no atom to repeat, `*` reaches the literal arm. The `^` half did
        // not, and the star was attaching to the anchor as a quantifier — so
        // `^*` meant "zero or more starts of line", which matches the empty
        // string everywhere and made `match('ab', '^*a')` answer 0 where vim
        // answers -1, and `matchstr('*ab', '^*a')` answer 'a' where vim answers
        // '*a'. Both were found by `fuzz-parity --regex`.
        //
        // Only the BARE `^` anchor takes the exception: `\_^*` still repeats
        // (`match('a', '\_^*')` is 0 in vim), which is why the test is on the
        // source character rather than on the node that comes back.
        let bare_bol = at_start && self.peek() == Some('^');
        let node = self.atom(at_start)?;
        if bare_bol && self.peek() == Some('*') {
            // Leave the `*` for the next `atom()` call, which reads it as the
            // literal it is. A `\{2}` or `\+` here is NOT covered by the
            // exception and still applies to the anchor.
            return Some(Atom {
                node,
                min: 1,
                max: 1,
                greedy: true,
            });
        }
        // `\@` is a multi too (c: `nfa_regpiece` `case Magic('@')`): it wraps the
        // atom just parsed in a lookaround/atomic node instead of repeating it.
        if self.peek() == Some('\\') && self.peek2() == Some('@') {
            self.i += 2;
            let node = self.look(node);
            // c: "E871: (NFA regexp) Can't have a multi follow a multi" — `a\@!*`.
            if self.multi_follows() {
                self.fail("E871: (NFA regexp) Can't have a multi follow a multi");
            }
            return Some(Atom {
                node,
                min: 1,
                max: 1,
                greedy: true,
            });
        }
        let before = self.i;
        let (min, max, greedy) = self.quantifier();
        let quantified = self.i != before;
        if quantified {
            // c: "E888: (NFA regexp) cannot repeat \zs" — a zero-width match-bound
            // marker cannot be *repeated*. `\?`/`\=` (which only make it optional,
            // max 1) are accepted: `\zs\?` is fine in Vim, `\zs*` and `\zs\{2}`
            // are not.
            let repeats = max > 1;
            if repeats {
                match node {
                    Node::MatchStart => self.fail("E888: (NFA regexp) cannot repeat \\zs"),
                    Node::MatchEnd => self.fail("E888: (NFA regexp) cannot repeat \\ze"),
                    _ => {}
                }
            }
            // c: "E871: (NFA regexp) Can't have a multi follow a multi" — `a*\+`,
            // and `a*\@=` (the `\@` family is a multi as well).
            if self.multi_follows() {
                self.fail("E871: (NFA regexp) Can't have a multi follow a multi");
            }
        }
        Some(Atom {
            node,
            min,
            max,
            greedy,
        })
    }

    /// Whether the literal text `@<=` or `@<!` appears anywhere from the cursor to
    /// the end of the pattern — c: `regexp_bt.c` `seen_endbrace`'s "Trick", which
    /// scans `regparse` forward for exactly that byte sequence and, finding it,
    /// lets a backreference precede the group it names.
    fn lookbehind_ahead(&self) -> bool {
        // c: `seen_endbrace()` scans `regparse`, which points into the pattern
        // the user WROTE, for the three bytes "@<=" or "@<!". Scanning the
        // magic translation instead answers differently for a very-magic
        // pattern, where a real `\@<=` is written `@<=` (three bytes, no
        // backslash) but translates to `\@\<\=` — no three-byte run — so
        // `split('', '\v\(\1\(\@<=')` reported E65 where vim answers `[]`.
        let from = self
            .omap
            .get(self.i)
            .copied()
            .unwrap_or(self.orig.len())
            .min(self.orig.len());
        self.orig[from..]
            .windows(3)
            .any(|w| w[0] == '@' && w[1] == '<' && matches!(w[2], '=' | '!'))
    }

    /// The character c: `nfa_regatom` reports as "Misplaced" at the cursor: the
    /// multis `*` `\+` `\?` `\=` `\{` `\@`, plus `\|` `\&` `\)`, which are branch
    /// and group operators and equally cannot form an atom. Used where the C calls
    /// `nfa_regatom()` directly rather than through `nfa_regpiece`.
    fn misplaced_here(&self) -> Option<char> {
        if self.peek() == Some('*') {
            return Some('*');
        }
        if self.peek() == Some('\\') {
            if let Some(m) = self.peek2() {
                if matches!(m, '+' | '?' | '=' | '{' | '@' | '|' | '&' | ')') {
                    return Some(m);
                }
            }
        }
        None
    }

    /// Whether the cursor sits on another multi — `*`, `\+`, `\?`, `\=`, `\{`,
    /// or `\@` (c: `re_multi_type(peekchr()) != NOT_MULTI` after a piece).
    fn multi_follows(&self) -> bool {
        self.peek() == Some('*')
            || (self.peek() == Some('\\')
                && matches!(self.peek2(), Some('+' | '?' | '=' | '{' | '@')))
    }

    /// The operator suffix of a `\@` multi (the `\@` itself is consumed). Wraps
    /// `inner` per c: `nfa_regpiece` `case Magic('@')` — an optional decimal
    /// limit (`getdecchrs()`, meaningful only for `\@123<=`/`\@123<!`), then
    /// `=` / `!` / `>` / `<=` / `<!`. An unknown operator is E869.
    fn look(&mut self, inner: Node) -> Node {
        let nr = self.read_int();
        let op = match self.bump() {
            Some('=') => LookOp::Ahead(true),
            Some('!') => LookOp::Ahead(false),
            Some('>') => LookOp::Atomic,
            Some('<') => match self.bump() {
                Some('=') => LookOp::Behind(true, nr),
                Some('!') => LookOp::Behind(false, nr),
                other => return self.bad_look(inner, other),
            },
            other => return self.bad_look(inner, other),
        };
        Node::Look(
            Box::new(Atom {
                node: inner,
                min: 1,
                max: 1,
                greedy: true,
            }),
            op,
        )
    }

    /// c: "E869: (NFA) Unknown operator '\@x'" — the char after `\@` (or after
    /// `\@<`) is not one Vim knows. The regex is dead after this, so the
    /// returned node is never matched.
    fn bad_look(&mut self, inner: Node, got: Option<char>) -> Node {
        let bad = got.map(|c| c.to_string()).unwrap_or_default();
        self.fail(&format!("E869: (NFA) Unknown operator '\\@{bad}'"));
        inner
    }

    fn quantifier(&mut self) -> (u32, u32, bool) {
        match (self.peek(), self.peek2()) {
            (Some('*'), _) => {
                self.i += 1;
                (0, INF, true)
            }
            (Some('\\'), Some('+')) => {
                self.i += 2;
                (1, INF, true)
            }
            (Some('\\'), Some('?')) | (Some('\\'), Some('=')) => {
                self.i += 2;
                (0, 1, true)
            }
            (Some('\\'), Some('{')) => {
                self.i += 2;
                self.brace_count()
            }
            _ => (1, 1, true),
        }
    }

    /// `\{n,m}` / `\{n}` / `\{n,}` / `\{,m}` / `\{}` / `\{-…}` (non-greedy).
    fn brace_count(&mut self) -> (u32, u32, bool) {
        let mut greedy = true;
        if self.peek() == Some('-') {
            greedy = false;
            self.i += 1;
        }
        let lo = self.read_int();
        let mut hi = lo;
        if self.peek() == Some(',') {
            self.i += 1;
            hi = self.read_int_or(INF);
        }
        // closing `}` (Vim allows a trailing `\}` or bare `}`).
        if self.peek() == Some('\\') && self.peek2() == Some('}') {
            self.i += 2;
        } else if self.peek() == Some('}') {
            self.i += 1;
        }
        let lo = lo.unwrap_or(0);
        let hi = hi.unwrap_or(if lo == 0 { INF } else { lo });
        (lo, hi, greedy)
    }

    fn read_int(&mut self) -> Option<u32> {
        let start = self.i;
        let mut n = 0u32;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                n = n * 10 + (c as u32 - '0' as u32);
                self.i += 1;
            } else {
                break;
            }
        }
        if self.i > start {
            Some(n)
        } else {
            None
        }
    }
    fn read_int_or(&mut self, default: u32) -> Option<u32> {
        Some(self.read_int().unwrap_or(default))
    }

    fn atom(&mut self, at_start: bool) -> Option<Node> {
        let c = self.peek()?;
        match c {
            '.' => {
                self.i += 1;
                Some(Node::Any)
            }
            '^' if at_start => {
                self.i += 1;
                Some(Node::Bol)
            }
            '$' if self.is_eol_pos() => {
                self.i += 1;
                Some(Node::Eol)
            }
            // c: an *unterminated* collection is not a collection at all — Vim treats
            // the `[` as a literal character (`match('a[x', '[')` is 1, and the
            // pattern `[abc` looks for the literal text `[abc`). Only scan it as a
            // collection when a closing `]` actually follows.
            '[' if self.collection_closes() => {
                self.i += 1;
                Some(Node::Class(self.bracket()))
            }
            // `:help /~` — `~` matches the LAST GIVEN SUBSTITUTE STRING, and with
            // none it is "E33: No previous substitute regular expression". This
            // interpreter has no `:s` command, so there is never a previous one:
            // `match('a', '~')` is E33 in vim 9.2 and was -1 here, and
            // `substitute()` does NOT set it (verified — E33 still follows a
            // `substitute()` call in the same process). Under `\M`/`\V` a bare
            // `~` is a literal and `preprocess_magic` has already escaped it, so
            // only the magic and very-magic spellings reach this arm.
            '~' => {
                self.fail("E33: No previous substitute regular expression");
                self.i += 1;
                Some(Node::Lit('~'))
            }
            '\\' => self.escape(),
            _ => {
                self.i += 1;
                Some(Node::Lit(c))
            }
        }
    }

    /// `$` is the end anchor only at the end of a branch.
    fn is_eol_pos(&self) -> bool {
        matches!(
            (self.p.get(self.i + 1), self.p.get(self.i + 2)),
            (None, _) | (Some('\\'), Some('|')) | (Some('\\'), Some(')'))
        )
    }

    fn escape(&mut self) -> Option<Node> {
        self.i += 1; // past '\'
                     // A trailing lone backslash escapes nothing, and vim reads it as the
                     // character itself: `match('a\b', '\')` is 1 and `matchstr('a\b', '\')`
                     // is "\". Returning `None` here ended the branch instead, so the pattern
                     // became empty and matched at 0.
        let Some(c) = self.bump() else {
            return Some(Node::Lit('\\'));
        };
        self.escaped(c)
    }

    /// c: `classchars` (`regexp_bt.c:256`) — `".iIkKfFpPsSdDxXoOwWhHaAlLuU"`, the
    /// characters that spell a character class after a backslash. `\_x` is legal
    /// for exactly these (plus `^`, `$` and `[`, handled before the fallthrough);
    /// anything else is `E877`.
    const CLASSCHARS: &str = ".iIkKfFpPsSdDxXoOwWhHaAlLuU";

    /// The atom denoted by the character *after* a backslash (already consumed).
    /// Split out from [`Self::escape`] so `\_x` can ask for the same atom `\x`
    /// would produce and then wrap it (see the `'_'` arm).
    fn escaped(&mut self, c: char) -> Option<Node> {
        Some(match c {
            // `\_x` — "x, or a newline" (`:help /\_`). `\_.` is any character
            // including NL, `\_s`/`\_a`/`\_d`/… are the class plus NL, and
            // `\_[…]` is the collection plus NL. It applies to the *negated*
            // classes too (`\_S` is "non-white, or NL"), so this cannot be done by
            // adding NL to the class's item list — a negated class would then
            // exclude it. Model it as what it is: an alternation of the atom and a
            // literal newline.
            '_' => {
                // c: `nfa_regatom()`'s `case Magic('_')` accepts exactly four
                // shapes and rejects everything else. `\_^` and `\_$` are the
                // line anchors and get NO newline alternation — they `break`
                // before `extra = NFA_ADD_NL` is set. `\_[` jumps to the
                // collection label, and every other character falls through into
                // the `classchars` switch, whose `p == NULL` arm with
                // `extra == NFA_ADD_NL` is
                // `semsg(e_nfa_regexp_invalid_character_class_nr, c)`. `\_b`,
                // `\_z` and `\_(` are that error, not the letters they used to
                // match here.
                let inner = match self.peek() {
                    // c: `if (c == '^') { EMIT(NFA_BOL); break; }` — no `\n` arm.
                    Some('^') => {
                        self.i += 1;
                        return Some(Node::Bol);
                    }
                    // c: `if (c == '$') { EMIT(NFA_EOL); break; }`
                    Some('$') => {
                        self.i += 1;
                        return Some(Node::Eol);
                    }
                    // `\_[…]` — a bracket collection.
                    Some('[') => self.atom(false)?,
                    // `\_.` — the `.` atom does not reach `escape()`, so take it
                    // here. It is `classchars`' first entry.
                    Some('.') => {
                        self.i += 1;
                        Node::Any
                    }
                    // `\_s`, `\_d`, `\_S`, … — the escaped class atom that
                    // follows, but ONLY when it is one of `classchars`.
                    Some(c) if Self::CLASSCHARS.contains(c) => {
                        self.i += 1;
                        self.escaped(c)?
                    }
                    Some(c) => {
                        // c: `e_nfa_regexp_invalid_character_class_nr` prints the
                        // character's numeric value, not the character.
                        self.i += 1;
                        self.fail(&format!(
                            "E877: (NFA regexp) Invalid character class: {}",
                            c as u32
                        ));
                        return Some(Node::Lit(c));
                    }
                    // c: `c = no_Magic(getchr()); if (c == NUL)
                    // EMSG_RET_FAIL(_(e_nfa_regexp_end_encountered_prematurely));`
                    // — a trailing `\_` is an error, not an empty atom.
                    None => {
                        self.fail("E865: (NFA) Regexp end encountered prematurely");
                        return None;
                    }
                };
                Node::Group(
                    vec![
                        vec![Atom {
                            node: inner,
                            min: 1,
                            max: 1,
                            greedy: true,
                        }],
                        vec![Atom {
                            node: Node::Lit('\n'),
                            min: 1,
                            max: 1,
                            greedy: true,
                        }],
                    ],
                    None,
                )
            }
            '(' => {
                // `escape()` has already consumed the `\(`; the opener starts two
                // characters back, which is what the diagnostic quotes.
                let open = self.i.saturating_sub(2);
                // c: `nfa_reg` `if (regnpar >= NSUBEXP) EMSG_RET_FAIL(...)`, with
                // `regnpar` starting at 1 and `NSUBEXP` 10 — nine capture groups
                // are the limit, and the tenth `\(` is rejected outright. This
                // engine numbered them without bound.
                if self.ngroups >= 9 {
                    self.fail("E872: (NFA regexp) Too many '('");
                }
                let idx = self.ngroups + 1;
                self.ngroups = idx;
                let branches = self.alternation(true);
                self.close_group(open, false);
                self.closed.push(idx);
                Node::Group(branches, Some(idx))
            }
            '%' if self.peek() == Some('(') => {
                let open = self.i.saturating_sub(2);
                self.i += 1; // past '('
                             // Only the very-magic spelling `\v%(` makes that `(` magic.
                let branches = self.alternation(self.vm.get(open).copied().unwrap_or(false));
                self.close_group(open, true);
                Node::Group(branches, None)
            }
            // Codepoint atoms — `\%d123` (decimal), `\%o40` (octal), `\%xff` /
            // `\%u00e9` / `\%U0001f600` (hex). Each matches the single character
            // with that code, so `\%d97` is the literal `a`. The digit run is
            // capped at the width Vim allows for the radix, so `\%d97x` is `a`
            // followed by a literal `x`.
            // Codepoint atoms — c: `nfa_regatom` `case 'd'/'o'/'x'/'u'/'U'` under
            // `Magic('%')`, reading through `getdecchrs`/`getoctchrs`/`gethexchrs`.
            // Each accepts a different digit run: decimal is UNBOUNDED, octal stops
            // after three digits OR once the value reaches 040 (so `\%o1011` is `A`
            // then a literal `1`, and `\%o401` is a space then `1`), and the hex
            // forms take at most 2/4/8. All three fail on an empty run, and the
            // caller rejects a value above INT_MAX — both are E678, where the atom
            // used to fall back to the literal radix letter.
            '%' if matches!(self.peek(), Some('d' | 'o' | 'x' | 'u' | 'U')) => {
                let kind = self.peek().expect("peeked above");
                self.i += 1; // past the radix letter
                let n = self.radix_chrs(kind);
                // The `%s` is the backslash the user had to type — empty under `\v`.
                let bs = if self
                    .vm
                    .get(self.i.saturating_sub(3))
                    .copied()
                    .unwrap_or(false)
                {
                    ""
                } else {
                    "\\"
                };
                match n
                    .filter(|n| *n <= u64::from(i32::MAX as u32))
                    .and_then(|n| u32::try_from(n).ok())
                    .and_then(char::from_u32)
                {
                    Some(c) => Node::Lit(c),
                    None => {
                        self.fail(&format!("E678: Invalid character after {bs}%[dxouU]"));
                        Node::Lit(kind)
                    }
                }
            }
            // c: `case 'C': EMIT(NFA_ANY_COMPOSING)` — `\%C` consumes a
            // composing character when there is one here, so `a\%Cb` matches
            // the whole of "a" + U+0301 + "b".
            '%' if self.peek() == Some('C') => {
                self.i += 1;
                Node::AnyComposing
            }
            // `\%#` (cursor) and `\%V` (Visual area) are recognized atoms that no
            // string match can satisfy: there is no cursor and no Visual selection.
            '%' if matches!(self.peek(), Some('#' | 'V')) => {
                self.i += 1;
                Node::CheckPos(usize::MAX)
            }
            // `\%^` / `\%$` — the start and end of the FILE. In an engine that
            // matches one string, that string is the file.
            '%' if self.peek() == Some('^') => {
                self.i += 1;
                Node::FileEnd(true)
            }
            '%' if self.peek() == Some('$') => {
                self.i += 1;
                Node::FileEnd(false)
            }
            '%' if self.peek() == Some('[') => {
                // c: `nfa_regatom` `case '['` under `Magic('%')` — the body is a
                // `for (n = 0; (c = peekchr()) != ']'; ++n) nfa_regatom()` loop,
                // NOT `nfa_regpiece`, so a multi inside is never a quantifier and
                // always reaches nfa_regatom's "these should follow an atom" arm:
                // `\%[a*]`, `\%[a\+b]`, `\%[a\|b]` and `\%[a\)b]` are all E866.
                let open = self.i.saturating_sub(2);
                // The `%s` in E69/E70 is the backslash the user had to type, empty
                // under `\v` — the same dialect rule `close_group` applies.
                let bs = if self.vm.get(open).copied().unwrap_or(false) {
                    ""
                } else {
                    "\\"
                };
                self.i += 1; // past '['
                let mut nodes = Vec::new();
                while self.peek().is_some() && self.peek() != Some(']') {
                    if let Some(m) = self.misplaced_here() {
                        self.fail(&format!("E866: (NFA regexp) Misplaced {m}"));
                        break;
                    }
                    match self.atom(false) {
                        Some(n) => nodes.push(n),
                        None => break,
                    }
                }
                if self.peek() == Some(']') {
                    self.i += 1; // past ']'
                                 // c: "E70: Empty %s%%[]" — the `n == 0` check runs after the `]`.
                    if nodes.is_empty() {
                        self.fail(&format!("E70: Empty {bs}%[]"));
                    }
                } else if self.err.is_none() {
                    // c: "E69: Missing ] after %s%%[" — the loop ran off the end.
                    self.fail(&format!("E69: Missing ] after {bs}%["));
                }
                Node::OptSeq(nodes)
            }
            // c: `nfa_regatom` `case Magic('%')` `default:` — the position family
            // `\%23l` / `\%<23c` / `\%>23v`, its cursor-relative `\%.c` form, the
            // mark forms `\%'m` / `\%<'m`, and the E867 that catches everything
            // else. Only the `\%23c`/`\%<`/`\%>` spelling used to be read at all;
            // an unrecognized `\%x` quietly became a literal `%`.
            '%' => {
                let cmp = match self.peek() {
                    Some('>') => {
                        self.i += 1;
                        Cmp::Gt
                    }
                    Some('<') => {
                        self.i += 1;
                        Cmp::Lt
                    }
                    _ => Cmp::Eq,
                };
                // `\%.c` is "the cursor's column"; a number may not follow it.
                let cur = self.peek() == Some('.');
                if cur {
                    self.i += 1;
                }
                let mut n: u64 = 0;
                let mut got_digit = false;
                while let Some(d) = self.peek().and_then(|c| c.to_digit(10)) {
                    if cur {
                        let c = self.peek().expect("peeked above");
                        self.fail(&format!("E1204: No Number allowed after .: '\\%{c}'"));
                        break;
                    }
                    n = n.saturating_mul(10).saturating_add(u64::from(d));
                    self.i += 1;
                    got_digit = true;
                }
                let c = self.peek();
                match c {
                    Some(k @ ('l' | 'c' | 'v')) => {
                        self.i += 1;
                        if !cur && !got_digit {
                            self.fail(&format!("E1273: (NFA regexp) missing value in '\\%{k}'"));
                        }
                        // c: `limit` is INT_MAX, or INT_MAX / MB_MAXBYTES for `v`.
                        let limit = if k == 'v' {
                            u64::from(i32::MAX as u32) / 21
                        } else {
                            u64::from(i32::MAX as u32)
                        };
                        if n >= limit {
                            self.fail("E951: \\% value too large");
                        }
                        // A line number and a virtual column are properties of a
                        // BUFFER, and the cursor-relative forms of a cursor; vim
                        // answers -1 for `\%1l` against a string too, so a node that
                        // never matches is the faithful answer for those.
                        if k == 'c' && !cur {
                            Node::Col(cmp, u32::try_from(n).unwrap_or(u32::MAX))
                        } else {
                            Node::CheckPos(usize::MAX)
                        }
                    }
                    // `\%'m` / `\%<'m` — a mark, which a string match has none of.
                    Some('\'') if n == 0 && !got_digit => {
                        self.i += 1;
                        self.bump(); // the mark name belongs to the atom
                        Node::CheckPos(usize::MAX)
                    }
                    _ => {
                        // c: `semsg(e_nfa_regexp_unknown_operator_percent_chr,
                        // no_Magic(c))`. At the end of the pattern `c` is NUL, and
                        // the `%c` writes it into the middle of the message — so
                        // what vim prints stops there, WITHOUT the closing quote.
                        self.fail(&match c {
                            Some(c) => format!("E867: (NFA regexp) Unknown operator '\\%{c}'"),
                            None => "E867: (NFA regexp) Unknown operator '\\%".to_string(),
                        });
                        Node::Lit('%')
                    }
                }
            }
            // `\%[atoms]` — optional-sequence atom (matches a greedy prefix).
            '<' => Node::WordB(true),
            '>' => Node::WordB(false),
            // `\zs` / `\ze` — set the start / end of the matched text.
            'z' if self.peek() == Some('s') => {
                self.i += 1;
                Node::MatchStart
            }
            'z' if self.peek() == Some('e') => {
                self.i += 1;
                Node::MatchEnd
            }
            'd' => class_atom(false, ClassItem::Digit),
            'D' => class_atom(true, ClassItem::Digit),
            'w' => class_atom(false, ClassItem::Word),
            'W' => class_atom(true, ClassItem::Word),
            's' => class_atom(false, ClassItem::Space),
            'S' => class_atom(true, ClassItem::Space),
            'a' => class_atom(false, ClassItem::Alpha),
            'A' => class_atom(true, ClassItem::Alpha),
            'l' => class_atom(false, ClassItem::Lower),
            // c: `classchars` pairs `l`/`L` and `u`/`U` with `NFA_LOWER`/
            // `NFA_NLOWER` and `NFA_UPPER`/`NFA_NUPPER`, and the runtime arm is
            // a plain complement — `result = curc != NUL && !ri_lower(curc)` —
            // so these two ARE negations, unlike the `\P \I \K` forms above.
            'L' => class_atom(true, ClassItem::Lower),
            'u' => class_atom(false, ClassItem::Upper),
            'U' => class_atom(true, ClassItem::Upper),
            'x' => class_atom(false, ClassItem::Hex),
            'X' => class_atom(true, ClassItem::Hex),
            'h' => class_atom(false, ClassItem::Head),
            'H' => class_atom(true, ClassItem::Head),
            'o' => class_atom(false, ClassItem::Octal),
            'O' => class_atom(true, ClassItem::Octal),
            // `\P \I \K` are "excluding digits", not negations — see the
            // `PrintNoDigit`/… `ClassItem` predicates; they stay `negated=false`.
            'p' => class_atom(false, ClassItem::Print),
            'P' => class_atom(false, ClassItem::PrintNoDigit),
            'i' => class_atom(false, ClassItem::Ident),
            'I' => class_atom(false, ClassItem::IdentNoDigit),
            'k' => class_atom(false, ClassItem::Keyword),
            'K' => class_atom(false, ClassItem::KeywordNoDigit),
            'f' => class_atom(false, ClassItem::Fname),
            'F' => class_atom(false, ClassItem::FnameNoDigit),
            'c' => {
                self.forced_ic = Some(true);
                return self.atom(false);
            }
            'C' => {
                self.forced_ic = Some(false);
                return self.atom(false);
            }
            // `\1`..`\9` — backreference to a group that must already be open.
            // c: Vim rejects `\1` with no group, and a *forward* reference too.
            d @ '1'..='9' => {
                let n = d as usize - '0' as usize;
                // c: the group must be COMPLETE — `\(a\1\)` refers to a group that is
                // still open, which Vim rejects, and so is a forward reference.
                //
                // E65 is raised by the BACKTRACKING engine (`regexp_bt.c`
                // `seen_endbrace`), which the automatic engine falls back to when
                // the NFA compile fails — and that function carries an explicit
                // escape hatch: "Trick: check if \"@<=\" or \"@<!\" follows, in which
                // case the \1 can appear before the referenced match". It is a plain
                // forward scan of the remaining pattern TEXT, not a parse, so
                // `match('', '\1@<=')` is -1 in vim where this reported E65.
                if !self.closed.contains(&n) && !self.lookbehind_ahead() {
                    self.fail("E65: Illegal back reference");
                }
                Node::BackRef(n)
            }
            't' => Node::Lit('\t'),
            'n' => Node::Lit('\n'),
            'r' => Node::Lit('\r'),
            'e' => Node::Lit('\x1b'),
            other => Node::Lit(other),
        })
    }

    /// Consume the `\)` that closes a group opened at translated index `open`,
    /// or report the group as unmatched.
    ///
    /// The diagnostic quotes the opener the way the USER wrote it, which is what
    /// Vim does: `\%(` is its own message and its own E-number (`E53`, not
    /// `E54`), and under `\v` neither form carries a backslash — `\v(` is
    /// "E54: Unmatched (" and `\v%(` is "E53: Unmatched %(".
    fn close_group(&mut self, open: usize, noncapturing: bool) {
        if self.peek() == Some('\\') && self.peek2() == Some(')') {
            self.i += 2;
            return;
        }
        let bs = if self.vm.get(open).copied().unwrap_or(false) {
            ""
        } else {
            "\\"
        };
        if noncapturing {
            self.fail(&format!("E53: Unmatched {bs}%("));
        } else {
            self.fail(&format!("E54: Unmatched {bs}("));
        }
    }

    /// Whether the `[` at the cursor opens a collection that is actually closed.
    /// The first `]` may appear literally right after `[` or `[^` (`[]a]` is a
    /// collection holding `]` and `a`), so start looking past it.
    fn collection_closes(&self) -> bool {
        let mut j = self.i + 1;
        if self.p.get(j) == Some(&'^') {
            j += 1;
        }
        if self.p.get(j) == Some(&']') {
            j += 1;
        }
        while let Some(&c) = self.p.get(j) {
            match c {
                ']' => return true,
                '\\' => j += 2, // an escaped char inside the collection
                // `[:alpha:]`, `[=a=]` and `[.a.]` are single ITEMS inside a
                // collection and carry their own `]`, which does not close the
                // collection. Counting it as the closer made `[[=a=]` — which
                // vim reads as literal text, because nothing closes the outer
                // `[` — into a collection of `[ = a =`, so
                // `matchstr('aa', '[[=a=]')` answered 'a' where vim answers ''.
                '[' if matches!(self.p.get(j + 1), Some(':' | '=' | '.')) => {
                    let kind = self.p[j + 1];
                    let mut k = j + 2;
                    while self.p.get(k).is_some()
                        && !(self.p[k] == kind && self.p.get(k + 1) == Some(&']'))
                    {
                        k += 1;
                    }
                    // Unterminated item: the `[` is just a character again.
                    j = if self.p.get(k).is_some() {
                        k + 2
                    } else {
                        j + 1
                    };
                }
                _ => j += 1,
            }
        }
        false
    }

    /// c: `getdecchrs()` / `getoctchrs()` / `gethexchrs(2|4|8)` (regexp.c) —
    /// the digit run after a radix letter, with the width each one allows.
    /// Decimal is UNBOUNDED, octal stops after three digits OR once the value
    /// reaches `040`, and the hex forms take at most 2 / 4 / 8. All three
    /// return `-1` — `None` here — on an empty run.
    fn radix_chrs(&mut self, kind: char) -> Option<u64> {
        let radix = match kind {
            'd' => 10,
            'o' => 8,
            _ => 16,
        };
        let mut n: u64 = 0;
        let mut got = 0;
        loop {
            let room = match kind {
                'd' => true,
                'o' => got < 3 && n < 0o40,
                'x' => got < 2,
                'u' => got < 4,
                _ => got < 8,
            };
            if !room {
                break;
            }
            let Some(d) = self.peek().and_then(|c| c.to_digit(radix)) else {
                break;
            };
            n = n
                .saturating_mul(u64::from(radix))
                .saturating_add(u64::from(d));
            self.i += 1;
            got += 1;
        }
        (got > 0).then_some(n)
    }

    /// c: `coll_get_char()` (regexp_bt.c:2660) — "Get a number after a
    /// backslash that is inside []. When nothing is recognized return a
    /// backslash", leaving the radix letter unconsumed.
    fn coll_get_char(&mut self) -> char {
        let kind = self.p[self.i];
        self.i += 1;
        match self.radix_chrs(kind) {
            None => {
                self.i -= 1;
                '\\'
            }
            // c: `if (nr > INT_MAX) nr = INT_MAX`, then the caller rejects
            // INT_MAX as "E1301: Value too large"; a value that is not a
            // codepoint cannot match any character either way.
            Some(n) => u32::try_from(n)
                .ok()
                .and_then(char::from_u32)
                .unwrap_or('\u{10fffd}'),
        }
    }

    /// One set member written with a backslash. c: the `*regparse == '\\'` arm
    /// of the collection loop (`regexp_nfa.c:1937`): a backslash is an escape
    /// inside `[]` only when what follows is in `REGEXP_INRANGE` (`]^-n\`) or
    /// `REGEXP_ABBR` (`nrtebdoxuU`) — regexp.c:182. Anything else leaves the
    /// backslash an ordinary member of the set.
    ///
    /// Without this, `[1-\x43]` was the range `1` to `\` (0x31..0x5C) plus the
    /// literal `x`, `4`, `3`, so it matched `[` (0x5B) — `match('[',
    /// '[1-\x43]')` answered 0 where vim answers -1.
    fn coll_escape(&mut self) -> Option<char> {
        if self.peek() != Some('\\') {
            return None;
        }
        let n = self.peek2()?;
        if !matches!(
            n,
            ']' | '^' | '-' | 'n' | '\\' | 'r' | 't' | 'e' | 'b' | 'd' | 'o' | 'x' | 'u' | 'U'
        ) {
            return None;
        }
        self.i += 1; // past the backslash
        if matches!(n, 'd' | 'o' | 'x' | 'u' | 'U') {
            return Some(self.coll_get_char());
        }
        self.i += 1;
        // c: `if (*regparse == 'n') startc = (reg_string || …) ? NL : NFA_NEWL`
        // — this engine matches a string, so `reg_string` is TRUE and `\n`
        // inside a collection is the newline character; then
        // `backslash_trans()` for the rest, which maps `] ^ - \` to themselves.
        Some(match n {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'e' => '\x1b',
            'b' => '\x08',
            other => other,
        })
    }

    fn bracket(&mut self) -> Class {
        let mut negated = false;
        if self.peek() == Some('^') {
            negated = true;
            self.i += 1;
        }
        let mut items = Vec::new();
        // A `]` immediately after `[`/`[^` is a literal.
        if self.peek() == Some(']') {
            items.push(ClassItem::Ch(']'));
            self.i += 1;
        }
        while let Some(c) = self.peek() {
            if c == ']' {
                self.i += 1;
                break;
            }
            // POSIX bracket class `[:name:]` inside `[...]` (`:help /[:alpha:]`).
            if c == '[' && self.peek2() == Some(':') {
                if let Some(mut posix) = self.posix_class() {
                    items.append(&mut posix);
                    continue;
                }
            }
            // Equivalence class `[=a=]` and collating element `[.a.]`. Both are a
            // single ITEM that carries its own `]`, so the collection is not
            // closed by it — `[[=a=]]` is one collection holding the class, and
            // reading its `]` as the closer left a stray literal `]` in the
            // pattern (`matchstr('a]b', '[[=a=]]')` answered 'a]' where vim
            // answers 'a').
            //
            // c: `get_equi_class` / `get_coll_element` accept EXACTLY ONE
            // character between the delimiters — `if (p[l + 2] == '=' && p[l + 3]
            // == ']')` — so `[[=ab=]]` is no class at all and its `[` falls through
            // to a literal (`match('a', '[[=ab=]]')` is -1 in vim and was 0 here).
            // An equivalence class then expands through `nfa_emit_equi_class`'s
            // per-letter table, so `[[=a=]]` matches `á` `à` `â` `ä` as well as
            // `a`; a collating element `[.a.]` is just the character itself.
            if c == '[' && matches!(self.peek2(), Some('=' | '.')) {
                let kind = self.peek2().expect("peeked above");
                if let Some(&ch) = self.p.get(self.i + 2) {
                    if self.p.get(self.i + 3) == Some(&kind) && self.p.get(self.i + 4) == Some(&']')
                    {
                        match (kind == '=').then(|| equi_class(ch)).flatten() {
                            Some(set) => items.extend(set.iter().copied().map(ClassItem::Ch)),
                            None => items.push(ClassItem::Ch(ch)),
                        }
                        self.i += 5;
                        continue;
                    }
                }
            }
            let lo = match self.coll_escape() {
                Some(ch) => ch,
                None => {
                    self.i += 1;
                    c
                }
            };
            // Range `a-z` (not when `-` is last before `]`). c: the range test
            // runs BEFORE the backslash arm, so an escaped `\-` is a member and
            // never the operator.
            if self.peek() == Some('-') && self.peek2().is_some() && self.peek2() != Some(']') {
                self.i += 1;
                let hi = match self.coll_escape() {
                    Some(ch) => ch,
                    None => self.bump().expect("peek2 above"),
                };
                // c: "E944: Reverse range in character class" — `[z-a]`.
                if hi < lo {
                    self.fail("E944: Reverse range in character class");
                }
                items.push(ClassItem::Range(lo, hi));
            } else {
                items.push(ClassItem::Ch(lo));
            }
        }
        Class { negated, items }
    }

    /// Parse a POSIX bracket class `[:name:]` starting at `self.i` (which points at
    /// `[`, with `peek2() == ':'`). On a recognized name, consumes through the
    /// closing `:]` and returns its `ClassItem`(s); otherwise leaves `self.i` where
    /// it was and returns `None` so the `[` is treated as a literal (Vim behavior).
    fn posix_class(&mut self) -> Option<Vec<ClassItem>> {
        let mut j = self.i + 2; // skip `[` and `:`
        let mut name = String::new();
        while let Some(ch) = self.p.get(j).copied() {
            if ch == ':' {
                break;
            }
            if !ch.is_ascii_alphabetic() {
                return None;
            }
            name.push(ch);
            j += 1;
        }
        // Require the closing `:]`.
        if self.p.get(j).copied() != Some(':') || self.p.get(j + 1).copied() != Some(']') {
            return None;
        }
        let items = posix_class_items(&name)?;
        self.i = j + 2; // consume through `:]`
        Some(items)
    }
}

/// Map a POSIX/Vim bracket-class name to its `ClassItem` predicate(s). Covers the
/// standard POSIX set plus Vim's extras (`tab/escape/backspace/return/ident/
/// keyword`); the single-char extras become literal `Ch` items. `[:fname:]` is
/// intentionally unmapped (returns `None`): `'isfname'` is platform-dependent,
/// like `\f`, so the token falls through to a literal. Unknown names → `None`.
fn posix_class_items(name: &str) -> Option<Vec<ClassItem>> {
    let item = match name {
        "alnum" => ClassItem::Alnum,
        "alpha" => ClassItem::Alpha,
        "blank" => ClassItem::Blank,
        "cntrl" => ClassItem::Cntrl,
        "digit" => ClassItem::Digit,
        "graph" => ClassItem::Graph,
        "lower" => ClassItem::LowerU,
        "print" => ClassItem::Print,
        "punct" => ClassItem::Punct,
        "space" => ClassItem::Whitespace,
        "upper" => ClassItem::UpperU,
        "xdigit" => ClassItem::Hex,
        "tab" => ClassItem::Ch('\t'),
        "escape" => ClassItem::Ch('\x1b'),
        "backspace" => ClassItem::Ch('\x08'),
        "return" => ClassItem::Ch('\r'),
        "ident" => ClassItem::Ident,
        "keyword" => ClassItem::Keyword,
        _ => return None,
    };
    Some(vec![item])
}

/// The characters an equivalence class `[=c=]` matches, or `None` when `c` has no
/// equivalents and stands for itself — c: `nfa_emit_equi_class` (`regexp_nfa.c`),
/// one entry per `case … return OK` arm of its `switch (c)`. In the C the `case`
/// labels of an arm and its `EMIT2` list are the same set of code points, so one
/// list serves both roles: `c` selects the group it appears in, and that whole
/// group is what the class matches. The C's fall-through `EMIT2(c)` is the `None`
/// here.
///
/// The C guards the table with `enc_utf8 || 'latin1' || 'iso-8859-15'`; this
/// engine is always UTF-8, so the guard always holds.
fn equi_class(c: char) -> Option<&'static [char]> {
    static GROUPS: &[&[char]] = &[
        &[
            'A', '\u{c0}', '\u{c1}', '\u{c2}', '\u{c3}', '\u{c4}', '\u{c5}', '\u{100}', '\u{102}',
            '\u{104}', '\u{1cd}', '\u{1de}', '\u{1e0}', '\u{1fa}', '\u{200}', '\u{202}', '\u{226}',
            '\u{23a}', '\u{1e00}', '\u{1ea0}', '\u{1ea2}', '\u{1ea4}', '\u{1ea6}', '\u{1ea8}',
            '\u{1eaa}', '\u{1eac}', '\u{1eae}', '\u{1eb0}', '\u{1eb2}', '\u{1eb4}', '\u{1eb6}',
        ],
        &[
            'B', '\u{181}', '\u{243}', '\u{1e02}', '\u{1e04}', '\u{1e06}',
        ],
        &[
            'C', '\u{c7}', '\u{106}', '\u{108}', '\u{10a}', '\u{10c}', '\u{187}', '\u{23b}',
            '\u{1e08}', '\u{a792}',
        ],
        &[
            'D', '\u{10e}', '\u{110}', '\u{18a}', '\u{1e0a}', '\u{1e0c}', '\u{1e0e}', '\u{1e10}',
            '\u{1e12}',
        ],
        &[
            'E', '\u{c8}', '\u{c9}', '\u{ca}', '\u{cb}', '\u{112}', '\u{114}', '\u{116}',
            '\u{118}', '\u{11a}', '\u{204}', '\u{206}', '\u{228}', '\u{246}', '\u{1e14}',
            '\u{1e16}', '\u{1e18}', '\u{1e1a}', '\u{1e1c}', '\u{1eb8}', '\u{1eba}', '\u{1ebc}',
            '\u{1ebe}', '\u{1ec0}', '\u{1ec2}', '\u{1ec4}', '\u{1ec6}',
        ],
        &['F', '\u{191}', '\u{1e1e}', '\u{a798}'],
        &[
            'G', '\u{11c}', '\u{11e}', '\u{120}', '\u{122}', '\u{193}', '\u{1e4}', '\u{1e6}',
            '\u{1f4}', '\u{1e20}', '\u{a7a0}',
        ],
        &[
            'H', '\u{124}', '\u{126}', '\u{21e}', '\u{1e22}', '\u{1e24}', '\u{1e26}', '\u{1e28}',
            '\u{1e2a}', '\u{2c67}',
        ],
        &[
            'I', '\u{cc}', '\u{cd}', '\u{ce}', '\u{cf}', '\u{128}', '\u{12a}', '\u{12c}',
            '\u{12e}', '\u{130}', '\u{197}', '\u{1cf}', '\u{208}', '\u{20a}', '\u{1e2c}',
            '\u{1e2e}', '\u{1ec8}', '\u{1eca}',
        ],
        &['J', '\u{134}', '\u{248}'],
        &[
            'K', '\u{136}', '\u{198}', '\u{1e8}', '\u{1e30}', '\u{1e32}', '\u{1e34}', '\u{2c69}',
            '\u{a740}',
        ],
        &[
            'L', '\u{139}', '\u{13b}', '\u{13d}', '\u{13f}', '\u{141}', '\u{23d}', '\u{1e36}',
            '\u{1e38}', '\u{1e3a}', '\u{1e3c}', '\u{2c60}',
        ],
        &['M', '\u{1e3e}', '\u{1e40}', '\u{1e42}'],
        &[
            'N', '\u{d1}', '\u{143}', '\u{145}', '\u{147}', '\u{1f8}', '\u{1e44}', '\u{1e46}',
            '\u{1e48}', '\u{1e4a}', '\u{a7a4}',
        ],
        &[
            'O', '\u{d2}', '\u{d3}', '\u{d4}', '\u{d5}', '\u{d6}', '\u{d8}', '\u{14c}', '\u{14e}',
            '\u{150}', '\u{19f}', '\u{1a0}', '\u{1d1}', '\u{1ea}', '\u{1ec}', '\u{1fe}', '\u{20c}',
            '\u{20e}', '\u{22a}', '\u{22c}', '\u{22e}', '\u{230}', '\u{1e4c}', '\u{1e4e}',
            '\u{1e50}', '\u{1e52}', '\u{1ecc}', '\u{1ece}', '\u{1ed0}', '\u{1ed2}', '\u{1ed4}',
            '\u{1ed6}', '\u{1ed8}', '\u{1eda}', '\u{1edc}', '\u{1ede}', '\u{1ee0}', '\u{1ee2}',
        ],
        &['P', '\u{1a4}', '\u{1e54}', '\u{1e56}', '\u{2c63}'],
        &['Q', '\u{24a}'],
        &[
            'R', '\u{154}', '\u{156}', '\u{158}', '\u{210}', '\u{212}', '\u{24c}', '\u{1e58}',
            '\u{1e5a}', '\u{1e5c}', '\u{1e5e}', '\u{2c64}', '\u{a7a6}',
        ],
        &[
            'S', '\u{15a}', '\u{15c}', '\u{15e}', '\u{160}', '\u{218}', '\u{1e60}', '\u{1e62}',
            '\u{1e64}', '\u{1e66}', '\u{1e68}', '\u{2c7e}', '\u{a7a8}',
        ],
        &[
            'T', '\u{162}', '\u{164}', '\u{166}', '\u{1ac}', '\u{1ae}', '\u{21a}', '\u{23e}',
            '\u{1e6a}', '\u{1e6c}', '\u{1e6e}', '\u{1e70}',
        ],
        &[
            'U', '\u{d9}', '\u{da}', '\u{dc}', '\u{db}', '\u{168}', '\u{16a}', '\u{16c}',
            '\u{16e}', '\u{170}', '\u{172}', '\u{1af}', '\u{1d3}', '\u{1d5}', '\u{1d7}', '\u{1d9}',
            '\u{1db}', '\u{214}', '\u{216}', '\u{244}', '\u{1e72}', '\u{1e74}', '\u{1e76}',
            '\u{1e78}', '\u{1e7a}', '\u{1ee4}', '\u{1ee6}', '\u{1ee8}', '\u{1eea}', '\u{1eec}',
            '\u{1eee}', '\u{1ef0}',
        ],
        &['V', '\u{1b2}', '\u{1e7c}', '\u{1e7e}'],
        &[
            'W', '\u{174}', '\u{1e80}', '\u{1e82}', '\u{1e84}', '\u{1e86}', '\u{1e88}',
        ],
        &['X', '\u{1e8a}', '\u{1e8c}'],
        &[
            'Y', '\u{dd}', '\u{176}', '\u{178}', '\u{1b3}', '\u{232}', '\u{24e}', '\u{1e8e}',
            '\u{1ef2}', '\u{1ef4}', '\u{1ef6}', '\u{1ef8}',
        ],
        &[
            'Z', '\u{179}', '\u{17b}', '\u{17d}', '\u{1b5}', '\u{1e90}', '\u{1e92}', '\u{1e94}',
            '\u{2c6b}',
        ],
        &[
            'a', '\u{e0}', '\u{e1}', '\u{e2}', '\u{e3}', '\u{e4}', '\u{e5}', '\u{101}', '\u{103}',
            '\u{105}', '\u{1ce}', '\u{1df}', '\u{1e1}', '\u{1fb}', '\u{201}', '\u{203}', '\u{227}',
            '\u{1d8f}', '\u{1e01}', '\u{1e9a}', '\u{1ea1}', '\u{1ea3}', '\u{1ea5}', '\u{1ea7}',
            '\u{1ea9}', '\u{1eab}', '\u{1ead}', '\u{1eaf}', '\u{1eb1}', '\u{1eb3}', '\u{1eb5}',
            '\u{1eb7}', '\u{2c65}',
        ],
        &[
            'b', '\u{180}', '\u{253}', '\u{1d6c}', '\u{1d80}', '\u{1e03}', '\u{1e05}', '\u{1e07}',
        ],
        &[
            'c', '\u{e7}', '\u{107}', '\u{109}', '\u{10b}', '\u{10d}', '\u{188}', '\u{23c}',
            '\u{1e09}', '\u{a793}', '\u{a794}',
        ],
        &[
            'd', '\u{10f}', '\u{111}', '\u{257}', '\u{1d6d}', '\u{1d81}', '\u{1d91}', '\u{1e0b}',
            '\u{1e0d}', '\u{1e0f}', '\u{1e11}', '\u{1e13}',
        ],
        &[
            'e', '\u{e8}', '\u{e9}', '\u{ea}', '\u{eb}', '\u{113}', '\u{115}', '\u{117}',
            '\u{119}', '\u{11b}', '\u{205}', '\u{207}', '\u{229}', '\u{247}', '\u{1d92}',
            '\u{1e15}', '\u{1e17}', '\u{1e19}', '\u{1e1b}', '\u{1e1d}', '\u{1eb9}', '\u{1ebb}',
            '\u{1ebd}', '\u{1ebf}', '\u{1ec1}', '\u{1ec3}', '\u{1ec5}', '\u{1ec7}',
        ],
        &[
            'f', '\u{192}', '\u{1d6e}', '\u{1d82}', '\u{1e1f}', '\u{a799}',
        ],
        &[
            'g', '\u{11d}', '\u{11f}', '\u{121}', '\u{123}', '\u{1e5}', '\u{1e7}', '\u{1f5}',
            '\u{260}', '\u{1d83}', '\u{1e21}', '\u{a7a1}',
        ],
        &[
            'h', '\u{125}', '\u{127}', '\u{21f}', '\u{1e23}', '\u{1e25}', '\u{1e27}', '\u{1e29}',
            '\u{1e2b}', '\u{1e96}', '\u{2c68}', '\u{a795}',
        ],
        &[
            'i', '\u{ec}', '\u{ed}', '\u{ee}', '\u{ef}', '\u{129}', '\u{12b}', '\u{12d}',
            '\u{12f}', '\u{1d0}', '\u{209}', '\u{20b}', '\u{268}', '\u{1d96}', '\u{1e2d}',
            '\u{1e2f}', '\u{1ec9}', '\u{1ecb}',
        ],
        &['j', '\u{135}', '\u{1f0}', '\u{249}'],
        &[
            'k', '\u{137}', '\u{199}', '\u{1e9}', '\u{1d84}', '\u{1e31}', '\u{1e33}', '\u{1e35}',
            '\u{2c6a}', '\u{a741}',
        ],
        &[
            'l', '\u{13a}', '\u{13c}', '\u{13e}', '\u{140}', '\u{142}', '\u{19a}', '\u{1e37}',
            '\u{1e39}', '\u{1e3b}', '\u{1e3d}', '\u{2c61}',
        ],
        &['m', '\u{1d6f}', '\u{1e3f}', '\u{1e41}', '\u{1e43}'],
        &[
            'n', '\u{f1}', '\u{144}', '\u{146}', '\u{148}', '\u{149}', '\u{1f9}', '\u{1d70}',
            '\u{1d87}', '\u{1e45}', '\u{1e47}', '\u{1e49}', '\u{1e4b}', '\u{a7a5}',
        ],
        &[
            'o', '\u{f2}', '\u{f3}', '\u{f4}', '\u{f5}', '\u{f6}', '\u{f8}', '\u{14d}', '\u{14f}',
            '\u{151}', '\u{1a1}', '\u{1d2}', '\u{1eb}', '\u{1ed}', '\u{1ff}', '\u{20d}', '\u{20f}',
            '\u{22b}', '\u{22d}', '\u{22f}', '\u{231}', '\u{275}', '\u{1e4d}', '\u{1e4f}',
            '\u{1e51}', '\u{1e53}', '\u{1ecd}', '\u{1ecf}', '\u{1ed1}', '\u{1ed3}', '\u{1ed5}',
            '\u{1ed7}', '\u{1ed9}', '\u{1edb}', '\u{1edd}', '\u{1edf}', '\u{1ee1}', '\u{1ee3}',
        ],
        &[
            'p', '\u{1a5}', '\u{1d71}', '\u{1d7d}', '\u{1d88}', '\u{1e55}', '\u{1e57}',
        ],
        &['q', '\u{24b}', '\u{2a0}'],
        &[
            'r', '\u{155}', '\u{157}', '\u{159}', '\u{211}', '\u{213}', '\u{24d}', '\u{27d}',
            '\u{1d72}', '\u{1d73}', '\u{1d89}', '\u{1e59}', '\u{1e5b}', '\u{1e5d}', '\u{1e5f}',
            '\u{a7a7}',
        ],
        &[
            's', '\u{15b}', '\u{15d}', '\u{15f}', '\u{161}', '\u{219}', '\u{23f}', '\u{1d74}',
            '\u{1d8a}', '\u{1e61}', '\u{1e63}', '\u{1e65}', '\u{1e67}', '\u{1e69}', '\u{a7a9}',
        ],
        &[
            't', '\u{163}', '\u{165}', '\u{167}', '\u{1ab}', '\u{1ad}', '\u{21b}', '\u{288}',
            '\u{1d75}', '\u{1e6b}', '\u{1e6d}', '\u{1e6f}', '\u{1e71}', '\u{1e97}', '\u{2c66}',
        ],
        &[
            'u', '\u{f9}', '\u{fa}', '\u{fb}', '\u{fc}', '\u{169}', '\u{16b}', '\u{16d}',
            '\u{16f}', '\u{171}', '\u{173}', '\u{1b0}', '\u{1d4}', '\u{1d6}', '\u{1d8}', '\u{1da}',
            '\u{1dc}', '\u{215}', '\u{217}', '\u{289}', '\u{1d7e}', '\u{1d99}', '\u{1e73}',
            '\u{1e75}', '\u{1e77}', '\u{1e79}', '\u{1e7b}', '\u{1ee5}', '\u{1ee7}', '\u{1ee9}',
            '\u{1eeb}', '\u{1eed}', '\u{1eef}', '\u{1ef1}',
        ],
        &['v', '\u{28b}', '\u{1d8c}', '\u{1e7d}', '\u{1e7f}'],
        &[
            'w', '\u{175}', '\u{1e81}', '\u{1e83}', '\u{1e85}', '\u{1e87}', '\u{1e89}', '\u{1e98}',
        ],
        &['x', '\u{1e8b}', '\u{1e8d}'],
        &[
            'y', '\u{fd}', '\u{ff}', '\u{177}', '\u{1b4}', '\u{233}', '\u{24f}', '\u{1e8f}',
            '\u{1e99}', '\u{1ef3}', '\u{1ef5}', '\u{1ef7}', '\u{1ef9}',
        ],
        &[
            'z', '\u{17a}', '\u{17c}', '\u{17e}', '\u{1b6}', '\u{1d76}', '\u{1d8e}', '\u{1e91}',
            '\u{1e93}', '\u{1e95}', '\u{2c6c}',
        ],
    ];
    GROUPS.iter().copied().find(|g| g.contains(&c))
}

fn class_atom(negated: bool, item: ClassItem) -> Node {
    Node::Class(Class {
        negated,
        items: vec![item],
    })
}

impl Regex {
    /// Compile a Vim magic-mode pattern, reusing the compiled form when this
    /// pattern has been seen before. Always succeeds (a malformed tail is
    /// treated literally, as Vim is lenient).
    ///
    /// The parse is a pure function of the pattern text — `preprocess_magic`
    /// and `Parser` read no option and no other state, and the case rule
    /// (`ic`) is a MATCH-time argument, not a compile-time one — so the result
    /// can be memoised without changing any answer. What is not pure is the
    /// diagnostic: an invalid pattern is an error every VimL function that
    /// takes one raises EVERY time it is called, so the message is cached
    /// beside the compiled form and re-raised on each hit.
    ///
    /// This matters because nothing above this layer caches: `'x' =~ 'pat'`
    /// inside a loop re-parsed the pattern on every iteration.
    pub fn compile(pat: &str) -> Rc<Regex> {
        // Bounded so a script that builds patterns from data (`'\<' . word .
        // '\>'` over a word list) cannot grow the map without limit. Clearing
        // wholesale rather than evicting one entry keeps this to two lines; the
        // pattern set of a real script is far below the bound, so the clear is
        // not a path that repeats.
        const MAX_CACHED: usize = 512;
        if let Some((re, err)) = REGEX_CACHE.with(|c| c.borrow().get(pat).cloned()) {
            if let Some(msg) = err {
                crate::ported::message::emsg(&msg);
            }
            return re;
        }
        let (re, err) = Regex::compile_uncached(pat);
        let re = Rc::new(re);
        REGEX_CACHE.with(|c| {
            let mut c = c.borrow_mut();
            if c.len() >= MAX_CACHED {
                c.clear();
            }
            c.insert(pat.to_string(), (re.clone(), err.clone()));
        });
        if let Some(msg) = err {
            crate::ported::message::emsg(&msg);
        }
        re
    }

    /// The parse itself, plus the diagnostic it would raise. Split out of
    /// [`Self::compile`] so the cache can hold both and replay the second.
    fn compile_uncached(pat: &str) -> (Regex, Option<String>) {
        let (tpat, vm, omap, pre_err) = preprocess_magic(pat);
        let mut parser = Parser {
            p: tpat.chars().collect(),
            vm,
            orig: pat.chars().collect(),
            omap,
            i: 0,
            ngroups: 0,
            forced_ic: None,
            closed: Vec::new(),
            err: None,
            err_pos: 0,
        };
        let branches = parser.alternation(true);
        // `concat` stops at a `\)`, so anything left over at the top level is a `\)`
        // that never had a `\(` — c: "E55: Unmatched \)".
        if parser.i < parser.p.len() {
            // Same dialect rule as the opener: `\v)` is "E55: Unmatched )".
            let bs = if parser.vm.get(parser.i).copied().unwrap_or(false) {
                ""
            } else {
                "\\"
            };
            parser.fail(&format!("E55: Unmatched {bs})"));
        }
        // An invalid pattern is an *error* in Vim, raised by every function that
        // takes one (`match()`, `substitute()`, `split()`, …) — not a pattern that
        // quietly matches nothing, which is what this engine used to do. Report it
        // and hand back a regex that matches nothing, so each caller falls through to
        // the result it returns once the error has been raised.
        // Vim reports the FIRST violation in pattern order. The pre-pass sees one
        // the parser cannot (a nomagic `\*` with nothing to repeat, which the
        // translation turns into an ordinary magic `*`), but it is not privileged:
        // when the parser found something earlier in the pattern, that is the
        // error Vim raises. `\M\1\(\*` is E65, `\M\*\1` is E866, and
        // `\M\ze\{2}\(\*` is E888 — all three verified against vim 9.2.
        if let Some((pos, msg)) = pre_err {
            if parser.err.is_none() || pos < parser.err_pos {
                parser.err = Some(msg);
                parser.err_pos = pos;
            }
        }
        if let Some(msg) = parser.err {
            return (
                Regex {
                    branches: Vec::new(),
                    ngroups: 0,
                    forced_ic: None,
                    dead: true,
                    nfa: None,
                },
                Some(msg),
            );
        }
        let nfa = nfa::Prog::compile(&branches, parser.ngroups);
        (
            Regex {
                branches,
                ngroups: parser.ngroups,
                forced_ic: parser.forced_ic,
                dead: false,
                nfa,
            },
            None,
        )
    }

    fn effective_ic(&self, ic: bool) -> bool {
        self.forced_ic.unwrap_or(ic)
    }

    /// First match at or after each start position (leftmost). Returns char
    /// spans (whole + groups).
    pub fn find(&self, text: &[char], ic: bool) -> Option<Captures> {
        self.find_from(text, ic, 0)
    }

    /// Leftmost match whose start is at or after char index `from`. `^`/`\<`
    /// still anchor to the absolute string start (this is Vim's `startcol`
    /// search, used by `match()`/`matchstr()` with a `{count}` argument).
    pub fn find_from(&self, text: &[char], ic: bool, from: usize) -> Option<Captures> {
        // An invalid pattern matches nothing (the error was already reported at
        // compile time) — an empty branch list would otherwise match the empty string.
        if self.dead {
            return None;
        }
        let ic = self.effective_ic(ic);
        let start = from.min(text.len());
        // c: `vim_regexec_both()` under `AUTOMATIC_ENGINE` — the NFA engine
        // answers unless it reports `NFA_TOO_EXPENSIVE`, in which case the
        // backtracker below is retried with the same pattern.
        if let Some(prog) = &self.nfa {
            match nfa::exec(prog, text, ic, start) {
                nfa::Outcome::Match(m) => {
                    let mut groups = m.subs;
                    groups.resize(self.ngroups + 1, None);
                    return Some(Captures { groups });
                }
                nfa::Outcome::NoMatch => return None,
                // c: `addstate()` raises the message and returns NULL, which
                // becomes `NFA_TOO_EXPENSIVE` — so vim reports it AND still
                // answers, from the backtracker.
                nfa::Outcome::Error(msg) => crate::ported::message::emsg(msg),
                nfa::Outcome::TooExpensive => {}
            }
        }
        self.find_from_bt(text, ic, start)
    }

    /// The backtracking engine's leftmost search. c: `bt_regexec_both()`, which
    /// vim reaches when the NFA engine bailed out.
    fn find_from_bt(&self, text: &[char], ic: bool, from: usize) -> Option<Captures> {
        let mut start = from;
        loop {
            // Two extra trailing slots hold the `\zs`/`\ze` positions, if any.
            let mut groups = vec![None; self.ngroups + 3];
            if let Some(end) = self.match_alt(&self.branches, text, start, &mut groups, ic) {
                // `\zs` moves the reported start, `\ze` the reported end.
                let s = match groups[self.ngroups + 1] {
                    Some((zp, _)) => zp,
                    None => start,
                };
                let e = match groups[self.ngroups + 2] {
                    Some((ep, _)) => ep,
                    None => end,
                };
                groups[0] = Some((s, e));
                // Drop the working slots so matchlist() sees only real groups.
                groups.truncate(self.ngroups + 1);
                return Some(Captures { groups });
            }
            if start >= text.len() {
                return None;
            }
            // The scan advances one *character* — `MB_PTR_ADV` / `utfc_ptr2len`,
            // base codepoint plus its composing marks — so a match can never
            // BEGIN on a mark that belongs to the character before it:
            // `match("éx", '\W')` is -1 in both engines, not 1. `from`
            // itself is always tried, which is why a subject that *opens* with a
            // composing mark still matches it at 0.
            start = cluster_end(text, start);
        }
    }

    /// Whether the pattern matches anywhere in `text`.
    pub fn is_match(&self, text: &[char], ic: bool) -> bool {
        self.find(text, ic).is_some()
    }

    /// Try each branch of an alternation at `pos`; first to match wins.
    fn match_alt(
        &self,
        branches: &[Branch],
        text: &[char],
        pos: usize,
        groups: &mut Vec<Option<(usize, usize)>>,
        ic: bool,
    ) -> Option<usize> {
        for b in branches {
            if let Some(end) = self.match_atoms(b, text, pos, groups, ic) {
                return Some(end);
            }
        }
        None
    }

    /// Match `atoms` in sequence at `pos`, returning where the whole sequence
    /// ended — the entry point every caller uses.
    ///
    /// The work is done by [`Self::atoms_k`], which is continuation-passing. The
    /// engine used to be written as "each piece returns ONE end position, the
    /// next piece starts there", and that shape cannot express backtracking INTO
    /// a group: `\(\_[a-z]\?\)[a-z]` against "12c3" let the group take the `c`,
    /// found no letter after it, and gave up — where vim retries with the group
    /// matching empty and answers `c`. The group's end position is a choice
    /// point like any other, so the continuation has to be able to reject it and
    /// ask for the next one.
    fn match_atoms(
        &self,
        atoms: &[Atom],
        text: &[char],
        pos: usize,
        groups: &mut Groups,
        ic: bool,
    ) -> Option<usize> {
        self.atoms_k(atoms, Subject { text, ic }, pos, groups, &mut |p, _g| {
            Some(p)
        })
    }

    /// `atoms` in sequence, handing each way the sequence can end to `k`.
    ///
    /// `k` returns `Some(end)` to accept that way and finish, or `None` to reject
    /// it and make this function try the next one. The final continuation simply
    /// accepts, so a plain `match_atoms` still takes the first (leftmost,
    /// greediest) path — the behaviour every existing case was recorded under.
    fn atoms_k(
        &self,
        atoms: &[Atom],
        subj: Subject,
        pos: usize,
        groups: &mut Groups,
        k: Cont,
    ) -> Option<usize> {
        let Some((atom, rest)) = atoms.split_first() else {
            return k(pos, groups);
        };
        self.rep_k(atom, subj, pos, 0, groups, &mut |p, g| {
            self.atoms_k(rest, subj, p, g, k)
        })
    }

    /// One quantified atom: try each legal repetition count, in the order the
    /// quantifier asks for, handing the position after it to `k`.
    ///
    /// Greedy tries "one more" before stopping; non-greedy (`\{-…}`) stops
    /// before trying one more. That ordering is the whole difference between the
    /// two, and it is now expressed once, here, instead of by materialising every
    /// reachable position and walking the list forwards or backwards.
    fn rep_k(
        &self,
        atom: &Atom,
        subj: Subject,
        pos: usize,
        count: u32,
        groups: &mut Groups,
        k: Cont,
    ) -> Option<usize> {
        // Restoring `groups` around a rejected alternative is what keeps a trial
        // repetition from leaving its captures (and its `\zs`/`\ze` marks)
        // behind: `matchend('ng', '\(.\ze=\)\?')` is 0 in vim, and was 1 here
        // because the failed attempt's `\ze` write outlived it.
        let tracks = node_writes(&atom.node);
        let saved = if tracks { Some(groups.clone()) } else { None };
        let restore = |groups: &mut Groups, saved: &Option<Groups>| {
            if let Some(s) = saved {
                groups.clone_from(s);
            }
        };

        let stop = |groups: &mut Groups, k: Cont| -> Option<usize> {
            if count < atom.min {
                return None;
            }
            k(pos, groups)
        };
        let more = |groups: &mut Groups, k: Cont| -> Option<usize> {
            if count >= atom.max {
                return None;
            }
            self.one_k(&atom.node, subj, pos, groups, &mut |np, g| {
                // A zero-width repetition never advances, so asking for another
                // one after it would not terminate. It still COUNTS: an empty
                // match satisfies the minimum, which is why
                // `match('aaa', '\%(\.\?\)\{2}')` is 0 in vim. So take as many
                // as `min` demands and then continue, instead of recursing.
                if np == pos {
                    let taken = count + 1;
                    if taken < atom.min {
                        return self.rep_k(atom, subj, np, taken, g, k);
                    }
                    return k(np, g);
                }
                self.rep_k(atom, subj, np, count + 1, g, k)
            })
        };

        if atom.greedy {
            if let Some(end) = more(groups, k) {
                return Some(end);
            }
            restore(groups, &saved);
            if let Some(end) = stop(groups, k) {
                return Some(end);
            }
        } else {
            if let Some(end) = stop(groups, k) {
                return Some(end);
            }
            restore(groups, &saved);
            if let Some(end) = more(groups, k) {
                return Some(end);
            }
        }
        restore(groups, &saved);
        None
    }

    /// One occurrence of one node, handing each end position it can produce to
    /// `k`.
    ///
    /// Only a group has more than one: every other node either matches at
    /// exactly one place or not at all, so it delegates to [`Self::match_one`]
    /// and calls `k` once.
    fn one_k(
        &self,
        node: &Node,
        subj: Subject,
        pos: usize,
        groups: &mut Groups,
        k: Cont,
    ) -> Option<usize> {
        let Node::Group(branches, capidx) = node else {
            let end = self.match_one(node, subj.text, pos, groups, subj.ic)?;
            return k(end, groups);
        };
        // A group that can write nothing needs no snapshot between branches —
        // `\%(ab\|cd\)` is a plain alternation, and cloning the slot vector for
        // every attempt at one was pure overhead.
        let saved = node_writes(node).then(|| groups.clone());
        for b in branches {
            if let Some(end) = self.atoms_k(b, subj, pos, groups, &mut |end, g| {
                let prev = capidx.map(|i| (i, g[i]));
                if let Some(i) = capidx {
                    g[*i] = Some((pos, end));
                }
                match k(end, g) {
                    Some(r) => Some(r),
                    None => {
                        if let Some((i, old)) = prev {
                            g[i] = old;
                        }
                        None
                    }
                }
            }) {
                return Some(end);
            }
            if let Some(sv) = &saved {
                groups.clone_from(sv);
            }
        }
        None
    }

    /// Match a single occurrence of one node at `pos`; returns the new position.
    fn match_one(
        &self,
        node: &Node,
        text: &[char],
        pos: usize,
        groups: &mut Vec<Option<(usize, usize)>>,
        ic: bool,
    ) -> Option<usize> {
        match node {
            Node::Lit(c) => {
                let ch = *text.get(pos)?;
                if char_eq(ch, *c, ic) {
                    Some(cluster_end(text, pos))
                } else {
                    None
                }
            }
            Node::Any => {
                let ch = *text.get(pos)?;
                if ch != '\n' {
                    Some(cluster_end(text, pos))
                } else {
                    None
                }
            }
            Node::Class(cl) => {
                let ch = *text.get(pos)?;
                if cl.matches(ch, ic) {
                    Some(cluster_end(text, pos))
                } else {
                    None
                }
            }
            Node::Bol => (pos == 0).then_some(pos),
            Node::Eol => (pos == text.len()).then_some(pos),
            // c: `\zs`/`\ze` are zero-width and record where the reported match
            // should start/end (slots reserved just past the capture groups).
            Node::MatchStart => {
                let slot = self.ngroups + 1;
                if let Some(cell) = groups.get_mut(slot) {
                    *cell = Some((pos, pos));
                }
                Some(pos)
            }
            Node::MatchEnd => {
                let slot = self.ngroups + 2;
                if let Some(cell) = groups.get_mut(slot) {
                    *cell = Some((pos, pos));
                }
                Some(pos)
            }
            Node::WordB(start) => {
                // c: `case BOW:` / `case EOW:` in `regmatch()`, regexp_bt.c:3517
                // and :3539 — both engines share these two predicates.
                let ok = if *start {
                    bow_matches(text, pos)
                } else {
                    eow_matches(text, pos)
                };
                ok.then_some(pos)
            }
            Node::OptSeq(nodes) => {
                // Greedily match the longest in-order prefix of the atoms; each
                // is optional, so stop at the first that does not match.
                let mut p = pos;
                for n in nodes {
                    match self.match_one(n, text, p, groups, ic) {
                        Some(next) => p = next,
                        None => break,
                    }
                }
                Some(p)
            }
            Node::BackRef(n) => {
                // Match the exact text previously captured by group `n`. An unset
                // group (it didn't participate) matches the empty string.
                match groups.get(*n).copied().flatten() {
                    Some((s, e)) => {
                        let len = e - s;
                        if pos + len <= text.len()
                            && (0..len).all(|k| char_eq(text[pos + k], text[s + k], ic))
                        {
                            Some(pos + len)
                        } else {
                            None
                        }
                    }
                    None => Some(pos),
                }
            }
            Node::Group(branches, capidx) => {
                let end = self.match_alt(branches, text, pos, groups, ic)?;
                if let Some(idx) = capidx {
                    groups[*idx] = Some((pos, end));
                }
                Some(end)
            }
            Node::Look(atom, op) => {
                let one = std::slice::from_ref(atom.as_ref());
                match op {
                    // c: NFA_PREV_ATOM_NO_WIDTH(_NEG) — zero-width probe of the
                    // atom at `pos`, consuming nothing. Groups captured inside a
                    // *successful* positive lookahead are kept:
                    // matchlist('foobar','foo\(bar\)\@=') == ['foo','bar',…].
                    LookOp::Ahead(want) => {
                        let mut probe = groups.clone();
                        let hit = self.match_atoms(one, text, pos, &mut probe, ic).is_some();
                        if hit != *want {
                            return None;
                        }
                        if hit {
                            *groups = probe;
                        }
                        Some(pos)
                    }
                    // c: NFA_PREV_ATOM_JUST_BEFORE(_NEG) — the atom must match
                    // ending exactly at `pos`; `\@123<=` bounds how far back the
                    // attempt may start (C counts bytes, this engine's unit is
                    // chars). The FARTHEST start that matches wins the captures:
                    // matchlist('aaab','\(a*\)\@<=b')[1] == 'aaa' in both oracles.
                    LookOp::Behind(want, limit) => {
                        let min_start = limit.map_or(0, |l| pos.saturating_sub(l as usize));
                        let branch = [
                            (**atom).clone(),
                            Atom {
                                node: Node::CheckPos(pos),
                                min: 1,
                                max: 1,
                                greedy: true,
                            },
                        ];
                        let mut hit = false;
                        for s in min_start..=pos {
                            let mut probe = groups.clone();
                            if self.match_atoms(&branch, text, s, &mut probe, ic).is_some() {
                                if *want {
                                    *groups = probe;
                                }
                                hit = true;
                                break;
                            }
                        }
                        (hit == *want).then_some(pos)
                    }
                    // c: NFA_PREV_ATOM_LIKE_PATTERN (`\@>`) — match the atom like
                    // a standalone pattern and CONSUME it, never backtracking into
                    // it. `match_one` commits to the first way its operand matches,
                    // so a plain match is exactly that.
                    LookOp::Atomic => self.match_atoms(one, text, pos, groups, ic),
                }
            }
            // c: `case NFA_ANY_COMPOSING` in `nfa_regmatch()` — "On a composing
            // character skip over it.  Otherwise do nothing.  Always matches."
            Node::AnyComposing => Some(
                match text
                    .get(pos)
                    .is_some_and(|c| crate::ported::strings::utf_iscomposing(*c))
                {
                    true => pos + 1,
                    false => pos,
                },
            ),
            Node::CheckPos(target) => (pos == *target).then_some(pos),
            Node::FileEnd(start) => {
                if *start { pos == 0 } else { pos == text.len() }.then_some(pos)
            }
            // c: `\%23c` is a BYTE column and 1-based, so the first character of
            // the subject is column 1 — `match('a', '\%1c')` is 0 in vim.
            Node::Col(cmp, n) => {
                let col = text[..pos].iter().map(|c| c.len_utf8()).sum::<usize>() as u64 + 1;
                let n = *n as u64;
                match cmp {
                    Cmp::Eq => col == n,
                    Cmp::Gt => col > n,
                    Cmp::Lt => col < n,
                }
                .then_some(pos)
            }
        }
    }
}

/// Whether matching this node can WRITE to the group/`\zs`/`\ze` slots.
///
/// Only these need the snapshot/restore dance in [`Regex::match_atoms`]; a
/// literal, a class or `.` cannot record anything, and the overwhelming majority
/// of quantified atoms are one of those.
fn node_writes(n: &Node) -> bool {
    match n {
        Node::Group(branches, cap) => {
            cap.is_some()
                || branches
                    .iter()
                    .any(|b| b.iter().any(|a| node_writes(&a.node)))
        }
        Node::MatchStart | Node::MatchEnd => true,
        Node::Look(a, _) => node_writes(&a.node),
        Node::OptSeq(ns) => ns.iter().any(node_writes),
        Node::Lit(_)
        | Node::Any
        | Node::Bol
        | Node::Eol
        | Node::WordB(_)
        | Node::BackRef(_)
        | Node::Class(_)
        | Node::AnyComposing
        | Node::CheckPos(_)
        | Node::FileEnd(_)
        | Node::Col(..) => false,
    }
}

/// One character *including its composing marks* — the end index of the cluster
/// starting at `pos`.
///
/// A matching atom in Vim consumes a whole character as `mb_ptr2len`/`utfc_ptr2len`
/// measures it, which is the base codepoint plus any combining marks that follow.
/// Advancing a single `char` instead split `é` (`e` + U+0301) down the middle, so
/// `matchstr("é…", '\l')` returned the bare `e` where Vim returns `é`.
fn cluster_end(text: &[char], pos: usize) -> usize {
    let mut end = pos + 1;
    while text
        .get(end)
        .is_some_and(|c| crate::ported::strings::utf_iscomposing(*c))
    {
        end += 1;
    }
    end
}

fn char_eq(a: char, b: char, ic: bool) -> bool {
    if ic {
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

/// Port of `reg_prev_class()` from `regexp.c:1349` (vim 9.2.1000) — the
/// character class of the character *before* `input`, or `-1` at the start of
/// the line.
///
/// RUST-PORT NOTE: the C is `mb_get_class_buf(rex.input - 1 - mb_head_off(...))`
/// — the byte arithmetic that steps a `char_u *` back one whole character.
/// Both engines here index a decoded `&[char]`, so the previous character is
/// `text[input - 1]`.
fn reg_prev_class(text: &[char], input: usize) -> i32 {
    if input > 0 {
        mb_get_class(text[input - 1])
    } else {
        -1
    }
}

/// The class of the character at `input`, or `0` past the end of the subject —
/// the C reads the line's NUL terminator there, and `mb_get_class_buf()`
/// answers `0` for NUL.
fn reg_this_class(text: &[char], input: usize) -> i32 {
    text.get(input).copied().map_or(0, mb_get_class)
}

/// `\<` — port of the `has_mbyte` arm of `case BOW:` (regexp_bt.c:3517, and
/// `case NFA_BOW:` at regexp_nfa.c:6321, which is the same test).
///
/// A word begins where the character under the cursor is a word character
/// (class 2 or more) and the character before it is in a *different* class.
/// That is not the same question as "the previous character is not a word
/// character": `mb_get_class_buf()` gives CJK ideographs, Hiragana, Katakana,
/// Hangul, braille, superscripts and subscripts each their own class number, so
/// `\<` fires between `語` and `a` in `日本語abc` — vim's
/// `substitute('日本語abc', '\<', 'X', 'g')` is `X日本語Xabc`. A single
/// is-word boolean cannot see that boundary.
fn bow_matches(text: &[char], input: usize) -> bool {
    // c: if (curc == NUL) result = FALSE;
    if input >= text.len() {
        return false;
    }
    // c: this_class = mb_get_class_buf(rex.input, rex.reg_buf);
    let this_class = reg_this_class(text, input);
    // c: if (this_class <= 1) result = FALSE;  // not on a word at all
    if this_class <= 1 {
        return false;
    }
    // c: else if (reg_prev_class() == this_class) result = FALSE;
    //    // previous char is in same word
    reg_prev_class(text, input) != this_class
}

/// `\>` — port of the `has_mbyte` arm of `case EOW:` (regexp_bt.c:3539, and
/// `case NFA_EOW:` at regexp_nfa.c:6348).
///
/// A word ends where the preceding character is a word character and the
/// character under the cursor — the line's NUL at end of subject, whose class
/// is `0` — is in a different class. See [`bow_matches`] for why the test is
/// class equality rather than a boolean.
fn eow_matches(text: &[char], input: usize) -> bool {
    // c: if (rex.input == rex.line) result = FALSE;  // can't match at start
    if input == 0 {
        return false;
    }
    let this_class = reg_this_class(text, input);
    let prev_class = reg_prev_class(text, input);
    // c: if (this_class == prev_class || prev_class == 0 || prev_class == 1)
    //        result = FALSE;
    this_class != prev_class && prev_class != 0 && prev_class != 1
}

// ── high-level entry points (used by ops.rs / builtins) ──

/// `subject =~ pattern`: whether the pattern matches anywhere in the subject.
pub fn regex_match(pat: &str, subject: &str, ic: bool) -> bool {
    let chars: Vec<char> = subject.chars().collect();
    Regex::compile(pat).is_match(&chars, ic)
}

/// The first matched substring (`matchstr`), or "" if no match.
pub fn regex_matchstr(pat: &str, subject: &str, ic: bool) -> String {
    let chars: Vec<char> = subject.chars().collect();
    match Regex::compile(pat).find(&chars, ic) {
        Some(caps) => {
            let (s, e) = caps.whole();
            chars[s..e].iter().collect()
        }
        None => String::new(),
    }
}

/// The `nth` (1-based) match of `pat` in `subject` whose start is at/after char
/// index `from`. Returns `(start, end, [whole, \1..\9])` in char indices (the
/// group list padded to 10, trailing empties), or `None`. `^`/`\<` anchor to the
/// absolute string start. Backs `match()`/`matchstr()`/… `{start}`/`{count}`.
pub fn regex_search_nth(
    pat: &str,
    subject: &str,
    ic: bool,
    from: usize,
    nth: i64,
) -> Option<(i64, i64, Vec<String>)> {
    let chars: Vec<char> = subject.chars().collect();
    let (s, e, spans) = regex_search_nth_chars(pat, &chars, ic, from, nth)?;
    let groups = spans
        .iter()
        .map(|g| match g {
            Some((gs, ge)) => chars[*gs..*ge].iter().collect(),
            None => String::new(),
        })
        .collect();
    Some((s, e, groups))
}

/// `(start, end, [whole, \1..\9])` in char indices — [`regex_search_nth_chars`]'s
/// result. A group that did not participate is `None`.
pub type CharSpans = (i64, i64, Vec<Option<(usize, usize)>>);

/// [`regex_search_nth`] over an already-decoded subject, reporting CHAR SPANS
/// rather than materialised group text.
///
/// The span form is what a caller needs when the subject's characters do not
/// map one-for-one onto the bytes they came from — `find_some_match()`'s
/// `{start}` chop, whose first "character" can be the orphan trailing byte of a
/// multi-byte sequence. Such a caller has to slice the ORIGINAL bytes, so it
/// cannot use text this function rebuilt from `char`s.
pub fn regex_search_nth_chars(
    pat: &str,
    chars: &[char],
    ic: bool,
    from: usize,
    nth: i64,
) -> Option<CharSpans> {
    let re = Regex::compile(pat);
    let mut pos = from.min(chars.len());
    let mut remaining = nth.max(1);
    loop {
        let caps = re.find_from(chars, ic, pos)?;
        let (s, e) = caps.whole();
        remaining -= 1;
        if remaining <= 0 {
            let mut groups: Vec<Option<(usize, usize)>> = caps
                .groups
                .iter()
                .map(|g| match g {
                    // A group's span can come back inverted when `\zs` inside it moved
                    // the match start past where the group closed — Vim rejects such a
                    // pattern outright (E888), but the matcher must not panic on the
                    // slice while getting there.
                    Some((gs, ge)) if gs <= ge => Some((*gs, *ge)),
                    _ => None,
                })
                .collect();
            groups.resize(10, None);
            return Some((s as i64, e as i64, groups));
        }
        // c (funcs.c:4190): the next {count} search starts ONE CHARACTER past the
        // START of this match, not past its end —
        //   `startcol = startp[0] + utfc_ptr2len(startp[0]) - str`
        // so e.g. `matchstr('hello world', '\w\+', 0, 2)` == 'ello', not 'world'.
        // `utfc_ptr2len` is a base codepoint plus its composing marks, so the
        // step is the whole cluster, not `s + 1`; that also covers the
        // zero-width case (a cluster is at least one char, so it progresses).
        pos = cluster_end(chars, s);
        if pos > chars.len() {
            return None;
        }
    }
}

/// The char index of the first match (`match`), or -1.
pub fn regex_match_index(pat: &str, subject: &str, ic: bool) -> i64 {
    let chars: Vec<char> = subject.chars().collect();
    Regex::compile(pat)
        .find(&chars, ic)
        .map_or(-1, |c| c.whole().0 as i64)
}

/// The char index just after the first match (`matchend`), or -1.
pub fn regex_matchend(pat: &str, subject: &str, ic: bool) -> i64 {
    let chars: Vec<char> = subject.chars().collect();
    Regex::compile(pat)
        .find(&chars, ic)
        .map_or(-1, |c| c.whole().1 as i64)
}

/// `matchstrpos`: `(matched substring, start char index, end char index)`, or
/// `("", -1, -1)` if there is no match.
pub fn regex_matchstrpos(pat: &str, subject: &str, ic: bool) -> (String, i64, i64) {
    let chars: Vec<char> = subject.chars().collect();
    match Regex::compile(pat).find(&chars, ic) {
        Some(caps) => {
            let (s, e) = caps.whole();
            (chars[s..e].iter().collect(), s as i64, e as i64)
        }
        None => (String::new(), -1, -1),
    }
}

/// `matchlist`: `[whole, submatch1, …]` (empty strings for groups that didn't
/// participate), or an empty list if there is no match.
pub fn regex_matchlist(pat: &str, subject: &str, ic: bool) -> Vec<String> {
    let chars: Vec<char> = subject.chars().collect();
    match Regex::compile(pat).find(&chars, ic) {
        Some(caps) => {
            // Vim's matchlist() always returns the whole match plus the nine
            // `\1`..`\9` submatch slots (NSUBEXP == 10), trailing empties kept.
            let mut out: Vec<String> = caps
                .groups
                .iter()
                .map(|g| match g {
                    Some((s, e)) => chars[*s..*e].iter().collect(),
                    None => String::new(),
                })
                .collect();
            out.resize(10, String::new());
            out
        }
        None => Vec::new(),
    }
}

/// `substitute({str}, {pat}, {sub}, {flags})` — replace the first match, or all
/// with the `g` flag. `\0`/`&` is the whole match; `\1`..`\9` are groups.
pub fn regex_substitute(subject: &str, pat: &str, sub: &str, flags: &str) -> String {
    let chars: Vec<char> = subject.chars().collect();
    let re = Regex::compile(pat);
    // c: `int do_all = (flags[0] == 'g');` (`do_string_sub`, eval.c) — the FIRST
    // character and nothing else. Reading it as "contains a g" made
    // `substitute('aaa','a','X','xg')` and `substitute('aaa','a','X','combining')`
    // replace every match where Vim replaces one; `'g '`, `'gzz'` and `'ggg'`
    // are global in both, which is what tells the two readings apart.
    let global = flags.starts_with('g');
    // `substitute()` has NO `i` flag: `do_string_sub` sets `regmatch.rm_ic = p_ic`
    // from the `'ignorecase'` OPTION and never looks at `flags` again. Measured,
    // `substitute('AAA','a','X','i')` is `'AAA'` in Vim and was `'XAA'` here.
    // Case-insensitivity comes from the pattern's own `\c` (handled by the regex
    // compiler) or from `'ignorecase'`, which this path does not consult yet —
    // Vim's default is `noignorecase`, so `false` is its default behaviour.
    let ic = false;
    // A `\=`-prefixed replacement is a Vim expression evaluated per match (with
    // `submatch()` available), not literal text.
    let sub_expr = sub.strip_prefix("\\=");
    let mut out = String::new();
    // Faithful port of `do_string_sub` (eval.c:6398). `tail` is the current
    // search origin; `zero_width` remembers the position of the last empty match
    // that was substituted, so a fresh empty match at that same spot is skipped
    // (copy one char, advance) rather than emitting a duplicate replacement. This
    // is Vim's "skip empty match except for first match" rule — e.g.
    // `substitute("aaa","a*","X","g")` is `X`, not `XX`.
    let mut tail = 0usize;
    let mut zero_width: Option<usize> = None;
    while let Some(caps) = re.find_from(&chars, ic, tail.min(chars.len())) {
        // Find the next match at or after `tail`. This goes through the same
        // entry point every other caller uses, so `substitute()` gets whichever
        // engine answered for `match()` — it used to run its own copy of the
        // scan against the backtracker, which is how
        // `substitute('a', '\]\@!\~\{-}\|\h\{,3}', 'X', '')` came out
        // `'Xa'` while `matchstr()` on the same pattern already said `'a'`.
        // `find_from` also applies the `\zs`/`\ze` adjustment, which is why
        // the two working slots are no longer handled here.
        let (s, e) = caps.whole();
        let groups = caps.groups;
        // c: `if (regmatch.startp[0] == regmatch.endp[0])` — empty match. Skip it
        // only when it lands on the same position as the previous empty match.
        if s == e {
            if zero_width == Some(s) {
                if tail < chars.len() {
                    out.push(chars[tail]);
                    tail += 1;
                    continue;
                }
                break;
            }
            zero_width = Some(s);
        }
        out.extend(&chars[tail..s]);
        if let Some(expr) = sub_expr {
            // Populate submatch() context, then evaluate the replacement expr.
            let subs: Vec<String> = groups
                .iter()
                .map(|g| match g {
                    Some((a, b)) => chars[*a..*b].iter().collect(),
                    None => String::new(),
                })
                .collect();
            SUBMATCHES.with(|m| *m.borrow_mut() = subs);
            // Copy the fn pointer out before calling it — the evaluator re-enters
            // install(), which borrows SUBST_EXPR_HOOK mutably.
            let hook = SUBST_EXPR_HOOK.with(|h| *h.borrow());
            let rep = hook.map(|f| f(expr)).unwrap_or_default();
            out.push_str(&rep);
        } else {
            out.push_str(&expand_sub(sub, &chars, &groups));
        }
        // c: `tail = regmatch.endp[0]; if (*tail == NUL) break;`
        tail = e;
        if tail >= chars.len() {
            break;
        }
        if !global {
            break;
        }
    }
    out.extend(&chars[tail.min(chars.len())..]);
    out
}

/// Case-folding state for substitute replacements: `\u`/`\l` upper/lower the
/// next output char only; `\U`/`\L` hold until `\e`/`\E`.
#[derive(Default)]
struct SubCase {
    one_shot: Option<bool>, // Some(true)=upper, Some(false)=lower — next char only
    sustained: Option<bool>,
}

impl SubCase {
    /// Push one logical char through the active case transform.
    ///
    /// `mb_toupper`/`mb_tolower` (the SIMPLE 1:1 mapping Vim's tables hold), not
    /// `char::to_uppercase()` (the FULL Unicode mapping, which can expand one
    /// char into several). Measured against vim 9.2.0900:
    /// `substitute('straße', '\(.*\)', '\U\1', '')` is `STRAßE` there and was
    /// `STRASSE` here. Same distinction as `strings.rs`'s `f_toupper`.
    fn push(&mut self, out: &mut String, c: char) {
        let upper = self.one_shot.take().or(self.sustained);
        match upper {
            Some(true) => out.push(crate::ported::mbyte::mb_toupper(c)),
            Some(false) => out.push(crate::ported::mbyte::mb_tolower(c)),
            None => out.push(c),
        }
    }
    fn push_str(&mut self, out: &mut String, s: impl IntoIterator<Item = char>) {
        for c in s {
            self.push(out, c);
        }
    }
}

/// Expand a substitute replacement: `\0`/`&` → whole match, `\1`..`\9` → group,
/// `\\` → `\`, `\n` → newline (0x0a), `\r` → carriage return, `\t` → tab,
/// `\b` → backspace (0x08),
/// `\u`/`\l`/`\U`/`\L`/`\e`/`\E` → case folding (matching Vim's `vim_regsub`).
fn expand_sub(sub: &str, chars: &[char], groups: &[Option<(usize, usize)>]) -> String {
    let s: Vec<char> = sub.chars().collect();
    let mut out = String::new();
    let mut cs = SubCase::default();
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            '&' => {
                if let Some((a, b)) = groups.first().copied().flatten() {
                    cs.push_str(&mut out, chars[a..b].iter().copied());
                }
                i += 1;
            }
            '\\' if i + 1 < s.len() => {
                let n = s[i + 1];
                match n {
                    '0'..='9' => {
                        let g = n as usize - '0' as usize;
                        if let Some(Some((a, b))) = groups.get(g) {
                            cs.push_str(&mut out, chars[*a..*b].iter().copied());
                        }
                    }
                    // c: `vim_regsub_both()` (`Src/nvim/regexp.c:2295-2307`) —
                    //   case 'r': c = CAR;  case 'n': c = NL;
                    //   case 't': c = TAB;  case 'b': c = Ctrl_H;
                    // `\n` is a NEWLINE (0x0a) here, not a NUL. The widespread
                    // "`\n` inserts a NUL" line is about `:s` in a BUFFER, where
                    // a NL byte in stored line text stands for a NUL — it is the
                    // storage convention, not this expansion. Measured: both
                    // `vim` and `nvim` answer
                    // `str2list(substitute('x','x','a\nb','')) == [97, 10, 98]`.
                    // What made it look like a NUL is that `strtrans()` DISPLAYS
                    // 0x0a as `^@` (`transchar_nonprint`: "we use newline in
                    // place of a NUL").
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    'b' => out.push('\u{8}'),
                    '\\' => cs.push(&mut out, '\\'),
                    '&' => cs.push(&mut out, '&'),
                    'u' => cs.one_shot = Some(true),
                    'l' => cs.one_shot = Some(false),
                    'U' => cs.sustained = Some(true),
                    'L' => cs.sustained = Some(false),
                    'e' | 'E' => {
                        cs.sustained = None;
                        cs.one_shot = None;
                    }
                    other => cs.push(&mut out, other),
                }
                i += 2;
            }
            c => {
                cs.push(&mut out, c);
                i += 1;
            }
        }
    }
    out
}

/// Split `subject` on matches of `pat` (pattern `split()`). Internal empty
/// pieces (from adjacent separators) are kept, matching Vim — only a leading or
/// trailing empty item is dropped, and only when `keepempty` is false.
pub fn regex_split(subject: &str, pat: &str, ic: bool, keepempty: bool) -> Vec<String> {
    // Faithful port of `f_split()` (eval/funcs.c). At each step search for the
    // next separator at/after the current position; the text before it becomes
    // an item. The `col` offset keeps a zero-width separator (e.g. `\zs`, the
    // "split into characters" idiom) from getting stuck at the same spot, so it
    // advances one character per item.
    let chars: Vec<char> = subject.chars().collect();
    let n = chars.len();
    let re = Regex::compile(pat);
    let eic = re.effective_ic(ic);

    // Find the first match at or after `from`, through the same entry point
    // every other caller uses so `split()` gets whichever engine answered for
    // `match()`. Returns the separator span, `\zs`/`\ze`-adjusted like Vim's
    // startp/endp.
    let find_from = |from: usize| -> Option<(usize, usize)> {
        re.find_from(&chars, eic, from.min(n)).map(|c| c.whole())
    };

    let mut out: Vec<String> = Vec::new();
    let mut str = 0usize; // start of the current item
    let mut col = 0usize; // search offset, 1 right after a zero-width match
    loop {
        let at_end = str >= n;
        // c: `while (*str != NUL || keepempty)` — stop unless a trailing empty
        // item is wanted.
        if at_end && !keepempty {
            break;
        }
        // c: match = (*str == NUL) ? false : vim_regexec_nl(..., str, col).
        let m = if at_end { None } else { find_from(str + col) };
        let (startp, endp) = m.unwrap_or((n, n));
        let end = if m.is_some() { startp } else { n };
        // c: keep this item unless it is an omitted leading/trailing empty; an
        // internal empty between two real (non-zero-width) separators survives.
        if keepempty || end > str || (!out.is_empty() && !at_end && m.is_some() && end < endp) {
            out.push(chars[str..end].iter().collect());
        }
        if m.is_none() {
            break;
        }
        // c: advance past the match; on a zero-width match step one *character* so
        // the next search makes progress — and a character is a base codepoint
        // plus its composing marks (`mb_ptr2len`), so `split(s, '\zs')` keeps
        // `é` (e + U+0301) whole instead of splitting the accent off.
        col = if endp > str {
            0
        } else {
            cluster_end(&chars, str) - str
        };
        str = endp;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every value here is the answer of vim 9.2 patch 1000, measured through
    /// `scripts/parity.sh`'s pinned entry point; the corresponding parity cases
    /// are `regex_nfa_thread_order`, `regex_composing_atoms`,
    /// `regex_backref_engine_order` and `regex_collection_escapes`.
    #[test]
    fn nfa_thread_order_decides_alternation_priority() {
        // A zero-width ANCHOR branch loses to a longer branch at the same
        // start, because `^`/`\<` compile to a transition added to the CURRENT
        // thread list while `a*` is still running.
        assert_eq!(regex_matchstr("^\\|a*", "aa", false), "aa");
        assert_eq!(regex_matchstr("\\<\\|a*", "aa", false), "aa");
        assert_eq!(regex_matchstr("$\\|a*", "aa", false), "aa");
        // An empty GROUP does not: it reaches NFA_MATCH at column 0.
        assert_eq!(regex_matchstr("\\%(\\)\\|a*", "aa", false), "");
        assert_eq!(regex_matchstr("a\\{0}\\|a*", "aa", false), "");
        // And it is not "longest wins" either.
        assert_eq!(regex_matchstr("a\\|ab", "ab", false), "a");
        // The same rule seen through substitute() and split(), which now go
        // through the same engine entry as matchstr().
        assert_eq!(
            regex_substitute("a", "\\]\\@!\\~\\{-}\\|\\h\\{,3}", "X", ""),
            "X"
        );
        assert_eq!(
            regex_split("aXa", "X\\|a*", false, false),
            Vec::<String>::new()
        );
    }

    #[test]
    fn composing_characters_are_word_chars_and_percent_c_consumes_them() {
        // c: `utf_class_buf()` returns 2 ("word") for any codepoint >= 0x100
        // that is not in its punctuation table, combining marks included.
        assert_eq!(regex_match_index("\\<", "\u{301}b", false), 0);
        assert_eq!(
            regex_split("\u{301}b", "\\<", false, false),
            vec!["\u{301}b"]
        );
        // c: `NFA_ANY_COMPOSING` — `\%C` skips one composing character when
        // there is one here and matches without consuming otherwise.
        assert_eq!(regex_matchstr("a\\%Cb", "a\u{301}b", false), "a\u{301}b");
        assert_eq!(regex_matchstr("a\\%C", "a\u{301}b", false), "a\u{301}");
        // `\%C` at a position with no composing character matches without
        // consuming; the `a` that follows then cannot end the match in front
        // of U+0301, so there is no match at all.
        assert_eq!(regex_matchstr("\\%Ca", "a\u{301}b", false), "");
        // A literal consumes only the base character, and NFA_MATCH refuses to
        // end in front of a composing mark — so `a` alone cannot match "a" +
        // U+0301, while `.` takes the whole cluster.
        assert_eq!(regex_matchstr("a", "a\u{301}b", false), "");
        assert_eq!(regex_matchstr(".", "a\u{301}b", false), "a\u{301}");
    }

    #[test]
    fn collection_backslash_escapes() {
        // c: REGEXP_INRANGE "]^-n\" + REGEXP_ABBR "nrtebdoxuU" (regexp.c:182).
        // `[1-\x43]` is the range 1..C, NOT 1..'\' — `[` (0x5B) is outside it.
        assert!(!regex_match("[1-\\x43]", "[", false));
        assert!(regex_match("[1-\\x43]", "A", false));
        assert!(regex_match("[1-\\x5b]", "[", false));
        assert!(regex_match("[\\d97]", "a", false));
        assert!(regex_match("[\\o141]", "a", false));
        assert!(regex_match("[\\x61-\\x63]", "a", false));
        assert!(!regex_match("[\\x61-\\x63]", "z", false));
        assert!(regex_match("[\\t]", "\t", false));
        assert!(regex_match("[\\r]", "\r", false));
        assert!(!regex_match("[\\r]", "r", false));
        // An escaped `-` is a member, never the range operator.
        assert!(regex_match("[a\\-z]", "-", false));
        assert!(!regex_match("[a\\-z]", "b", false));
        // c: `coll_get_char()` — "When nothing is recognized return a
        // backslash", leaving the radix letter unconsumed, so `[1-\xzz]`
        // is the range 1..'\' plus the literals `z`, `z`.
        assert!(regex_match("[1-\\xzz]", "\\", false));
        // A backslash before anything outside those two sets is not an escape.
        assert!(regex_match("[\\q]", "q", false));
        assert!(regex_match("[\\q]", "\\", false));
    }

    #[test]
    fn basic_atoms_and_anchors() {
        assert!(regex_match("foo", "a foo b", false));
        assert!(regex_match("^foo", "foobar", false));
        assert!(!regex_match("^foo", "a foo", false));
        assert!(regex_match("bar$", "foobar", false));
        assert!(regex_match("f.o", "fxo", false));
    }

    /// `\L`, `\U` and `\X` are the NEGATED forms of `\l`, `\u` and `\x`, not
    /// missing atoms. c: `regexp_bt.c:256`
    /// `classchars = ".iIkKfFpPsSdDxXoOwWhHaAlLuU"` pairs each lowercase
    /// spelling with the uppercase one, and `nfa_classcodes` maps those three
    /// pairs to `NFA_LOWER`/`NFA_NLOWER`, `NFA_UPPER`/`NFA_NUPPER` and
    /// `NFA_HEX`/`NFA_NHEX`, whose runtime arms are plain complements that
    /// still require a character — `result = curc != NUL && !ri_lower(curc)`
    /// (`regexp_nfa.c:6781`, `:6731`). All three used to fall through to the
    /// literal-character arm, so `match('A', '\L')` answered `-1` where
    /// vim 9.2.1000 answers `0`.
    ///
    /// Every expectation was read off vim 9.2.1000. Found by counting: the
    /// fuzz grammar's class-atom list held fifteen of `classchars`' twenty-six
    /// entries, and these three were among the eleven with a count of zero.
    /// With them added the table is complete — every letter in `classchars` is
    /// now an arm of `Parser::atom`.
    #[test]
    fn negated_case_class_atoms() {
        // Non-lowercase / non-uppercase, one character wide.
        assert_eq!(regex_matchstr("\\L", "A", false), "A");
        assert_eq!(regex_matchstr("\\L", "a", false), "");
        assert_eq!(regex_matchstr("\\U", "a", false), "a");
        assert_eq!(regex_matchstr("\\U", "A", false), "");
        // Digits are neither, so both accept them.
        assert_eq!(regex_matchstr("\\L\\+", "aB3", false), "B3");
        assert_eq!(regex_matchstr("\\U\\+", "aB3", false), "a");
        // `curc != NUL`: an empty subject matches neither, unlike a `\%^`-style
        // zero-width atom.
        assert!(!regex_match("\\L", "", false));
        assert!(!regex_match("\\U", "", false));
        // The complement is over the same predicate `\l`/`\u` use, so a
        // non-ASCII letter that is neither is accepted by both.
        assert_eq!(regex_matchstr("\\L\\+", "éA", false), "éA");
        assert_eq!(regex_matchstr("\\U\\+", "éa", false), "éa");
        // Very-magic spells them the same way — the class atoms keep their
        // backslash under `\v`.
        assert_eq!(regex_matchstr("\\v\\U+", "aBc", false), "a");
        assert_eq!(regex_matchstr("\\v\\L+", "aBc", false), "B");
        // And they reach `substitute()` like any other atom.
        assert_eq!(regex_substitute("aB", "\\L", "X", "g"), "aX");
        assert_eq!(regex_substitute("aB", "\\U", "Y", "g"), "YB");
        // `\X` — non-hex-digit, the third pair `classchars` names and the one
        // the fuzzer hit first, as `match('\zs', '\X')` (the subject is the
        // three characters `\`, `z`, `s`; the backslash is not a hex digit).
        assert_eq!(regex_matchstr("\\X", "\\zs", false), "\\");
        assert_eq!(regex_matchstr("\\X", "abcdef0", false), "");
        assert_eq!(regex_matchstr("\\X\\+", "0aFzy1", false), "zy");
        assert!(!regex_match("\\X", "", false));
    }

    #[test]
    fn quantifiers_and_classes() {
        assert!(regex_match("ab*c", "ac", false));
        assert!(regex_match("ab*c", "abbbc", false));
        assert!(regex_match("a\\+", "aaa", false));
        assert!(regex_match("\\d\\+", "x42y", false));
        assert!(regex_match("[a-c]\\{2}", "xbcx", false));
        assert!(!regex_match("[^0-9]", "5", false));
    }

    /// `substitute()`'s `{flags}` is read by `do_string_sub` as
    /// `int do_all = (flags[0] == 'g')` (eval.c) — the FIRST character, and no
    /// other flag exists. Reading it as "contains a g" replaced every match for
    /// `'xg'` and for any word ending in g (`'combining'`), and an invented `i`
    /// flag made `substitute('AAA','a','X','i')` answer `'XAA'` where Vim leaves
    /// `'AAA'` untouched: case-insensitivity comes from the pattern's `\c` or
    /// from `'ignorecase'`, never from these flags.
    ///
    /// Every expectation was read off `vim 9.2`. Found by `fuzz-parity`
    /// (seed 91117) as `substitute('0x1f','\_.','X','écombining')`.
    #[test]
    fn substitute_flags_are_read_as_vim_reads_them() {
        // Global iff the flags START with `g` — trailing junk does not matter.
        for f in ["g", "gg", "ggg", "gx", "g ", "gzz"] {
            assert_eq!(regex_substitute("aaa", "a", "X", f), "XXX", "flags {f:?}");
        }
        // A `g` anywhere but first is not the flag.
        for f in ["", "xg", " g", "ag", "&g", "G", "combining", "é"] {
            assert_eq!(regex_substitute("aaa", "a", "X", f), "Xaa", "flags {f:?}");
        }
        // There is no `i` flag: these leave an uppercase subject alone.
        for f in ["i", "gi", "ig"] {
            assert_eq!(regex_substitute("AAA", "a", "X", f), "AAA", "flags {f:?}");
        }
        // The pattern's own `\c` still forces a case-insensitive match, and the
        // leading `g` still governs how many are replaced.
        assert_eq!(regex_substitute("AAA", "\\ca", "X", "g"), "XXX");
        assert_eq!(regex_substitute("AAA", "\\ca", "X", ""), "XAA");
    }

    #[test]
    fn groups_alt_wordbound_case() {
        assert!(regex_match("\\(foo\\|bar\\)", "a bar", false));
        assert!(regex_match("\\<word\\>", "a word here", false));
        assert!(!regex_match("\\<ord\\>", "word", false));
        assert!(regex_match("FOO", "foo", true)); // ignore case
        assert!(regex_match("\\cfoo", "FOO", false)); // \c forces ic
    }

    #[test]
    fn matchstr_and_substitute() {
        assert_eq!(regex_matchstr("\\d\\+", "ab123cd", false), "123");
        assert_eq!(regex_substitute("foobar", "o", "0", ""), "f0obar");
        assert_eq!(regex_substitute("foobar", "o", "0", "g"), "f00bar");
        assert_eq!(
            regex_substitute("2024-06", "\\(\\d\\+\\)-\\(\\d\\+\\)", "\\2/\\1", ""),
            "06/2024"
        );
    }

    // `\zs` moves the start of the replaced region: only text after `\zs` is
    // substituted. Verified against vim 9.2 / nvim.
    #[test]
    fn substitute_zs_region() {
        // `\zs` narrows the replaced region to the text after it.
        assert_eq!(regex_substitute("foobar", "foo\\zsbar", "X", ""), "fooX");
        assert_eq!(regex_substitute("abc", "a\\zsb", "X", ""), "aXc");
        // Leftmost real match wins; the reported start is the `\zs` position.
        assert_eq!(regex_substitute("foobar", "o\\zsbar", "X", ""), "fooX");
        // Trailing `\zs` — a zero-width match after the anchor text inserts.
        assert_eq!(regex_substitute("abc", "abc\\zs", "X", ""), "abcX");
        // `&` in the replacement is the narrowed region, not the whole match.
        assert_eq!(
            regex_substitute("foobar", "foo\\zsbar", "[&]", ""),
            "foo[bar]"
        );
        // Global `\zs` narrowing.
        assert_eq!(regex_substitute("axbxc", "\\zsx", "-", "g"), "a-b-c");
    }

    // `\&` — concat/AND: all `\&`-joined branches must match at the position and
    // the LAST branch is the result (`foo\&...` == `\(foo\)\@=...`). Verified
    // against vim 9.2.
    #[test]
    fn regex_and_branch() {
        // All branches match at pos 0; the last (`...`) is what is returned.
        assert_eq!(regex_matchstr("foo\\&...", "foobar", false), "foo");
        // Order of branches does not change the "last wins" rule.
        assert_eq!(regex_matchstr("...\\&foo", "foobar", false), "foo");
        // A failing leading branch fails the whole match.
        assert_eq!(regex_matchstr("x\\&foo", "foobar", false), "");
        // The last branch may consume more than a leading one.
        assert_eq!(regex_matchstr("fo\\&..o", "foobar", false), "foo");
        // substitute() replaces the last branch's span.
        assert_eq!(regex_substitute("foobar", "foo\\&...", "X", ""), "Xbar");
        // Exact equivalence to the positive-lookahead form.
        assert_eq!(
            regex_matchstr("foo\\&...", "foobar", false),
            regex_matchstr("\\(foo\\)\\@=...", "foobar", false)
        );
    }

    #[test]
    fn split_on_pattern() {
        assert_eq!(
            regex_split("a1b2c", "\\d", false, false),
            vec!["a", "b", "c"]
        );
    }

    // Every expectation below was verified against BOTH vim 9.2 and nvim (they
    // agree on all of them) — see BUGS.md R12-10.
    #[test]
    fn lookahead() {
        // `a\@!` matches zero-width wherever `a` does not follow.
        assert_eq!(
            regex_split("3.5e2", "a\\@!", false, false),
            vec!["3", ".", "5", "e", "2"]
        );
        assert_eq!(
            regex_substitute("*.[]^$\\", "a\\@!", "X", "abc"),
            "X*.[]^$\\"
        );
        assert_eq!(regex_match_index("b\\@!", "abc", false), 0);
        assert_eq!(regex_match_index("b\\@!", "bbc", false), 2);
        assert_eq!(regex_matchstr("foo\\(bar\\)\\@=", "foobar", false), "foo");
        assert_eq!(regex_match_index("foo\\(bar\\)\\@!", "foobaz", false), 0);
        assert_eq!(regex_match_index("foo\\(bar\\)\\@!", "foobar", false), -1);
        // Groups captured inside a successful positive lookahead are kept.
        assert_eq!(
            regex_matchlist("foo\\(bar\\)\\@=", "foobar", false)[..2],
            ["foo".to_string(), "bar".to_string()]
        );
        assert_eq!(regex_substitute("abc", "x\\@!", "-", "g"), "-a-b-c-");
        assert_eq!(regex_substitute("abc", "x\\@=", "-", "g"), "abc");
        assert_eq!(regex_split("ab", "b\\@=", false, false), vec!["a", "b"]);
    }

    #[test]
    fn lookbehind() {
        assert_eq!(regex_matchstr("\\(foo\\)\\@<=bar", "foobar", false), "bar");
        assert_eq!(regex_match_index("\\(foo\\)\\@<=bar", "zzzbar", false), -1);
        assert_eq!(regex_match_index("\\(foo\\)\\@<!bar", "foobar", false), -1);
        assert_eq!(regex_match_index("\\(foo\\)\\@<!bar", "zzzbar", false), 3);
        // The lookbehind may match empty, and the FARTHEST start wins captures.
        assert_eq!(regex_match_index("\\(a*\\)\\@<=b", "b", false), 0);
        assert_eq!(
            regex_matchlist("\\(a*\\)\\@<=b", "aaab", false)[..2],
            ["b".to_string(), "aaa".to_string()]
        );
        // `\@123<=` bounds how far back the attempt may start.
        assert_eq!(regex_matchstr("\\(foo\\)\\@2<=bar", "foobar", false), "");
        assert_eq!(regex_matchstr("\\(foo\\)\\@3<=bar", "foobar", false), "bar");
        assert_eq!(regex_substitute("aXbXc", "X\\@<!.", "-", "g"), "--b-c");
        assert_eq!(regex_substitute("aaa", "\\(a\\)\\@<=a", "X", "g"), "aXX");
    }

    #[test]
    fn atomic_group() {
        // `\@>` consumes like a standalone pattern with no backtracking into it.
        assert_eq!(regex_matchstr("\\(a*\\)\\@>b", "aaab", false), "aaab");
        assert_eq!(regex_match_index("\\(a*\\)\\@>a", "aaa", false), -1);
    }

    #[test]
    fn look_very_magic() {
        assert_eq!(regex_matchstr("\\v(foo)@<=bar", "foobar", false), "bar");
        assert_eq!(regex_match_index("\\vfoo(bar)@!", "foobaz", false), 0);
        assert_eq!(regex_matchstr("\\v(foo)@3<=bar", "foobar", false), "bar");
        assert_eq!(
            regex_split("3.5e2", "\\va@!", false, false),
            vec!["3", ".", "5", "e", "2"]
        );
    }

    #[test]
    fn look_errors() {
        // A dead (rejected) pattern matches nothing: `\@` with nothing before it
        // is E866, a multi on either side of `\@` is E871, `\@x` is E869.
        assert_eq!(regex_match_index("\\@!", "x", false), -1);
        assert_eq!(regex_match_index("a\\@!*", "x", false), -1);
        assert_eq!(regex_match_index("a*\\@=", "x", false), -1);
        assert_eq!(regex_match_index("a\\@x", "x", false), -1);
        assert_eq!(regex_match_index("a\\@<x", "x", false), -1);
        assert_eq!(regex_match_index("\\(a\\)\\@=\\{2}", "x", false), -1);
    }

    /// `\<` compares CHARACTER CLASSES, not a yes/no word predicate: a boundary
    /// falls wherever two adjacent characters have different non-punctuation
    /// classes. Every expectation is vim 9.2.1000's own answer.
    #[test]
    fn word_boundaries_follow_character_class() {
        // The case that opened R43-O1: 語 is class 0x4e00 and `a` is class 2, so
        // the script change IS a word start.
        assert_eq!(
            regex_substitute("日本語abc", "\\<", "X", "g"),
            "X日本語Xabc"
        );
        assert_eq!(
            regex_substitute("abc日本語", "\\<", "X", "g"),
            "XabcX日本語"
        );
        // Hiragana, katakana, CJK and Hangul are four DIFFERENT classes, so a
        // boundary falls between each pair.
        assert_eq!(
            regex_substitute("あアa漢한", "\\<", "X", "g"),
            "XあXアXaX漢X한"
        );
        assert_eq!(
            regex_substitute("あアa漢한", "\\>", "X", "g"),
            "あXアXaX漢X한X"
        );
        // Emoji are class 3 — `emoji_all`, checked before the interval table.
        assert_eq!(regex_substitute("a😀b", "\\<", "X", "g"), "XaX😀Xb");
        // Subscripts are class 0x2080, but SUPERSCRIPT TWO is U+00B2 — below
        // 0x100, so it is `'iskeyword'`'s answer (punctuation), not the table's.
        assert_eq!(regex_substitute("x₂y", "\\<", "X", "g"), "XxX₂Xy");
        assert_eq!(regex_substitute("x²y", "\\<", "X", "g"), "Xx²Xy");
        // Latin-1 letters share class 2 with ASCII, so no boundary inside a word.
        assert_eq!(regex_substitute("áb", "\\<", "X", "g"), "Xáb");
        // µ (0xb5) is a keyword character via the `@` alpha class; ª (0xaa) is not.
        assert_eq!(regex_substitute("µa", "\\<", "X", "g"), "Xµa");
        assert_eq!(regex_substitute("ªa", "\\<", "X", "g"), "ªXa");
        // ASCII words are unchanged by all of this.
        assert_eq!(regex_substitute("foo bar", "\\<", "X", "g"), "Xfoo Xbar");
        assert_eq!(regex_substitute("foo bar", "\\>", "X", "g"), "fooX barX");
        // `\>` cannot match at position 0 (`rex.input == rex.line`), and `\<`
        // cannot match at end of subject (`curc == NUL`).
        assert_eq!(regex_substitute("", "\\<", "X", "g"), "");
        assert_eq!(regex_substitute("", "\\>", "X", "g"), "");
        // split() rides on the same predicate.
        assert_eq!(
            regex_split("日本語abc", "\\<", false, false),
            vec!["日本語", "abc"]
        );
    }

    /// `\_x` is legal for `classchars`, `^`, `$` and `[` — and an error for
    /// everything else. A rejected pattern matches nothing here.
    #[test]
    fn underscore_atom_rejects_non_class_chars() {
        // c: `e_nfa_regexp_invalid_character_class_nr` — `\_b` used to match `b`.
        assert_eq!(regex_match_index("\\_b", "abc", false), -1);
        assert_eq!(regex_match_index("\\_z", "abc", false), -1);
        assert_eq!(regex_match_index("\\_1", "abc", false), -1);
        assert_eq!(regex_match_index("\\_(a\\)", "abc", false), -1);
        assert_eq!(regex_match_index("\\_é", "abc", false), -1);
        // c: `if (c == NUL) EMSG_RET_FAIL(e_nfa_regexp_end_encountered_prematurely)`
        assert_eq!(regex_match_index("\\_", "abc", false), -1);
        // The legal shapes still work: a class plus newline, and a collection.
        assert_eq!(regex_matchstr("\\_.\\+", "a\nb", false), "a\nb");
        assert_eq!(regex_matchstr("\\_s", "a\nb", false), "\n");
        assert_eq!(regex_matchstr("\\_[ab]\\+", "a\nb", false), "a\nb");
        assert_eq!(regex_matchstr("\\_W", "abc", false), "");
        // `\_^` and `\_$` are the plain anchors — the C breaks out before
        // `extra = NFA_ADD_NL`, so they carry NO newline alternation and cannot
        // match in the middle of a one-line subject.
        assert_eq!(regex_matchstr("\\_^a", "abc", false), "a");
        assert_eq!(regex_matchstr("c\\_$", "abc", false), "c");
        assert_eq!(regex_matchstr("a\\_^b", "abc", false), "");
    }
}
