//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! EXTENSION — implements Vim's regex DIALECT, not a line-by-line port of
//! `regexp_bt.c`/`regexp_nfa.c` (which compile a pattern to a bytecode program
//! and match it with a backtracking / NFA VM). This is the vimlrs analogue of
//! the bytecode-compiler carve-out: a backtracking matcher over a parsed AST
//! that reproduces Vim's documented pattern behavior (`:help pattern`) in the
//! default **magic** mode. It backs `=~`/`!~`, `matchstr()`, `match()`,
//! `substitute()`, pattern `split()`, and `:catch /pat/`.
//!
//! Supported (magic mode): literals, `.`, `^`, `$`, `[...]`/`[^...]` with
//! ranges, the class atoms `\d \D \w \W \s \S \a \A \l \u \x \h \H \o \O`
//! (+negations) and the option-derived `\p \P \i \I \k \K` (default
//! `'isprint'`/`'isident'`/`'iskeyword'`; the uppercase forms exclude digits,
//! per `:help /\P`, and are NOT set-complements),
//! quantifiers `* \+ \? \= \{n,m}` and the non-greedy `\{-n,m}`, groups
//! `\(...\)` (capturing) and `\%(...\)` (non-capturing) with `\|` alternation,
//! word boundaries `\< \>`, and case control `\c`/`\C` plus the caller's
//! ignore-case flag. Backreferences (`\1`) are not yet handled.
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use std::cell::RefCell;

/// `\=`-replacement expression evaluator hook (`expr -> string`).
type SubstExprFn = fn(&str) -> String;

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

impl Class {
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
fn preprocess_magic(pat: &str) -> (String, Option<String>) {
    // Nothing to do for the default (magic) dialect, which is what the parser reads.
    if !pat.contains("\\v") && !pat.contains("\\M") && !pat.contains("\\V") {
        return (pat.to_string(), None);
    }
    let chars: Vec<char> = pat.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut mode = Dialect::Magic;
    // Whether an atom has been emitted in the current branch — a multi needs one.
    let mut atom_before = false;
    let mut err: Option<String> = None;
    const OPS: &str = "(){}+?=|<>";
    while i < chars.len() {
        let c = chars[i];
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
                                err = Some("E866: (NFA regexp) Misplaced *".to_string());
                            }
                            if n != '*' {
                                atom_before = true;
                            }
                            out.push(n); // `\.` in nomagic IS the magic `.`
                            i += 2;
                        }
                        Some(&n) => {
                            // `\|` and `\(` open a new branch: the multi rule restarts.
                            atom_before = !matches!(n, '|' | '(');
                            out.push('\\');
                            out.push(n);
                            i += 2;
                            // `\%[`, `\%(`, `\%d97` — the char after `%` belongs to the
                            // escape and must not be literal-ized on the next pass.
                            if n == '%' {
                                if let Some(&after) = chars.get(i) {
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
                            if OPS.contains(n) {
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
                    '%' if chars.get(i + 1) == Some(&'(') => {
                        out.push_str("\\%("); // non-capturing group
                        i += 2;
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
    }
    (out, err)
}

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
}

impl Parser {
    /// Record the first violation; later ones are noise once the pattern is invalid.
    fn fail(&mut self, msg: &str) {
        if self.err.is_none() {
            self.err = Some(msg.to_string());
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
    fn alternation(&mut self) -> Vec<Branch> {
        let mut branches = vec![self.concat()];
        while self.peek() == Some('\\') && self.peek2() == Some('|') {
            self.i += 2;
            branches.push(self.concat());
        }
        branches
    }

    /// A sequence of quantified atoms, stopping at `\|`, `\)`, or end.
    fn concat(&mut self) -> Branch {
        let mut atoms = Vec::new();
        loop {
            match (self.peek(), self.peek2()) {
                (None, _) => break,
                (Some('\\'), Some('|')) | (Some('\\'), Some(')')) => break,
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
                    if matches!(m, '+' | '=' | '?' | '{') {
                        self.fail(&format!("E866: (NFA regexp) Misplaced {m}"));
                        break;
                    }
                }
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
        let node = self.atom(at_start)?;
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
            // c: "E871: (NFA regexp) Can't have a multi follow a multi" — `a*\+`.
            let after = self.i;
            let (_, _, _) = self.quantifier();
            if self.i != after {
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
        let c = self.bump()?;
        self.escaped(c)
    }

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
                let inner = match self.peek() {
                    // `\_.` — the `.` atom does not reach `escape()`, so take it here.
                    Some('.') => {
                        self.i += 1;
                        Node::Any
                    }
                    // `\_[…]` — a bracket collection.
                    Some('[') => self.atom(false)?,
                    // `\_s`, `\_d`, `\_S`, … — the escaped class atom that follows.
                    Some(_) => {
                        let c = self.bump()?;
                        self.escaped(c)?
                    }
                    None => return None,
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
                let idx = self.ngroups + 1;
                self.ngroups = idx;
                let branches = self.alternation();
                self.close_group();
                self.closed.push(idx);
                Node::Group(branches, Some(idx))
            }
            '%' if self.peek() == Some('(') => {
                self.i += 1; // past '('
                let branches = self.alternation();
                self.close_group();
                Node::Group(branches, None)
            }
            // Codepoint atoms — `\%d123` (decimal), `\%o40` (octal), `\%xff` /
            // `\%u00e9` / `\%U0001f600` (hex). Each matches the single character
            // with that code, so `\%d97` is the literal `a`. The digit run is
            // capped at the width Vim allows for the radix, so `\%d97x` is `a`
            // followed by a literal `x`.
            '%' if matches!(self.peek(), Some('d' | 'o' | 'x' | 'u' | 'U')) => {
                let kind = self.peek().expect("peeked above");
                self.i += 1; // past the radix letter
                let (radix, maxlen) = match kind {
                    'd' => (10, 10),
                    'o' => (8, 4),
                    'x' => (16, 2),
                    'u' => (16, 4),
                    _ => (16, 8),
                };
                let mut n: u32 = 0;
                let mut got = 0;
                while got < maxlen {
                    let Some(c) = self.peek() else { break };
                    let Some(d) = c.to_digit(radix) else { break };
                    n = n.saturating_mul(radix).saturating_add(d);
                    self.i += 1;
                    got += 1;
                }
                // No digits at all: Vim treats the atom as the literal letter.
                match (got > 0).then(|| char::from_u32(n)).flatten() {
                    Some(c) => Node::Lit(c),
                    None => Node::Lit(kind),
                }
            }
            // `\%[atoms]` — optional-sequence atom (matches a greedy prefix).
            '%' if self.peek() == Some('[') => {
                self.i += 1; // past '['
                let mut nodes = Vec::new();
                while self.peek().is_some() && self.peek() != Some(']') {
                    match self.atom(false) {
                        Some(n) => nodes.push(n),
                        None => break,
                    }
                }
                if self.peek() == Some(']') {
                    self.i += 1; // past ']'
                }
                Node::OptSeq(nodes)
            }
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
            'u' => class_atom(false, ClassItem::Upper),
            'x' => class_atom(false, ClassItem::Hex),
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
                if !self.closed.contains(&n) {
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

    fn close_group(&mut self) {
        if self.peek() == Some('\\') && self.peek2() == Some(')') {
            self.i += 2;
        } else {
            // c: "E54: Unmatched \(" — the group was never closed.
            self.fail("E54: Unmatched \\(");
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
                _ => j += 1,
            }
        }
        false
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
            self.i += 1;
            // Range `a-z` (not when `-` is last before `]`).
            if self.peek() == Some('-') && self.peek2().is_some() && self.peek2() != Some(']') {
                self.i += 1;
                let hi = self.bump().unwrap();
                // c: "E944: Reverse range in character class" — `[z-a]`.
                if hi < c {
                    self.fail("E944: Reverse range in character class");
                }
                items.push(ClassItem::Range(c, hi));
            } else {
                items.push(ClassItem::Ch(c));
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

fn class_atom(negated: bool, item: ClassItem) -> Node {
    Node::Class(Class {
        negated,
        items: vec![item],
    })
}

impl Regex {
    /// Compile a Vim magic-mode pattern. Always succeeds (a malformed tail is
    /// treated literally, as Vim is lenient).
    pub fn compile(pat: &str) -> Regex {
        let (pat, pre_err) = preprocess_magic(pat);
        let mut parser = Parser {
            p: pat.chars().collect(),
            i: 0,
            ngroups: 0,
            forced_ic: None,
            closed: Vec::new(),
            err: None,
        };
        if let Some(e) = pre_err {
            parser.fail(&e);
        }
        let branches = parser.alternation();
        // `concat` stops at a `\)`, so anything left over at the top level is a `\)`
        // that never had a `\(` — c: "E55: Unmatched \)".
        if parser.i < parser.p.len() {
            parser.fail("E55: Unmatched \\)");
        }
        // An invalid pattern is an *error* in Vim, raised by every function that
        // takes one (`match()`, `substitute()`, `split()`, …) — not a pattern that
        // quietly matches nothing, which is what this engine used to do. Report it
        // and hand back a regex that matches nothing, so each caller falls through to
        // the result it returns once the error has been raised.
        if let Some(msg) = parser.err {
            crate::ported::message::emsg(&msg);
            return Regex {
                branches: Vec::new(),
                ngroups: 0,
                forced_ic: None,
                dead: true,
            };
        }
        Regex {
            branches,
            ngroups: parser.ngroups,
            forced_ic: parser.forced_ic,
            dead: false,
        }
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
        for start in from..=text.len() {
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
        }
        None
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

    fn match_atoms(
        &self,
        atoms: &[Atom],
        text: &[char],
        pos: usize,
        groups: &mut Vec<Option<(usize, usize)>>,
        ic: bool,
    ) -> Option<usize> {
        let Some((atom, rest)) = atoms.split_first() else {
            return Some(pos);
        };
        // Reachable positions after matching `atom` 0,1,2,… times (greedy run).
        let mut positions = vec![pos];
        let mut cur = pos;
        let mut count = 0u32;
        while count < atom.max {
            match self.match_one(&atom.node, text, cur, groups, ic) {
                Some(next) if next > cur => {
                    positions.push(next);
                    cur = next;
                    count += 1;
                }
                // Zero-width match. Repeating it is still legal — an empty match can
                // be taken as many times as `min` demands, it simply never advances —
                // so satisfy `min` here and then stop, because iterating further would
                // loop forever without moving. Counting it only *once* meant a group
                // that can match empty could never reach a `min` above 1:
                // `match('aaa', '\%(\.\?\)\{2}')` is 0 in Vim (an empty match at 0)
                // and was -1 here.
                Some(next) if next == cur => {
                    while count < atom.min {
                        positions.push(cur);
                        count += 1;
                    }
                    break;
                }
                _ => break,
            }
        }
        let max_k = positions.len() - 1;
        if (max_k as u32) < atom.min {
            return None;
        }
        let order: Vec<usize> = if atom.greedy {
            (atom.min as usize..=max_k).rev().collect()
        } else {
            (atom.min as usize..=max_k).collect()
        };
        for k in order {
            if let Some(end) = self.match_atoms(rest, text, positions[k], groups, ic) {
                return Some(end);
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
                let before = pos > 0 && is_word(text[pos - 1]);
                let after = pos < text.len() && is_word(text[pos]);
                let ok = if *start {
                    !before && after
                } else {
                    before && !after
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
        }
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

/// Word-character test for `\<`/`\>` boundaries. Unlike the `\w` class atom
/// (ASCII-only), Vim's word boundaries follow `'iskeyword'`, whose default
/// (`@,48-57,_,192-255`) plus `utf_class` classification treats multibyte
/// letters/digits (é, Ω, ４, ñ) as keyword chars — verified against nvim/vim:
/// `matchstr('!é', '\<.')` == 'é'. So this stays Unicode-aware on purpose.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
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
    let re = Regex::compile(pat);
    let mut pos = from.min(chars.len());
    let mut remaining = nth.max(1);
    loop {
        let caps = re.find_from(&chars, ic, pos)?;
        let (s, e) = caps.whole();
        remaining -= 1;
        if remaining <= 0 {
            let mut groups: Vec<String> = caps
                .groups
                .iter()
                .map(|g| match g {
                    // A group's span can come back inverted when `\zs` inside it moved
                    // the match start past where the group closed — Vim rejects such a
                    // pattern outright (E888), but the matcher must not panic on the
                    // slice while getting there.
                    Some((gs, ge)) if gs <= ge => chars[*gs..*ge].iter().collect(),
                    _ => String::new(),
                })
                .collect();
            groups.resize(10, String::new());
            return Some((s as i64, e as i64, groups));
        }
        // Advance past this match to find the next; step one char on a
        // zero-width match so the search makes progress.
        pos = if e > s { e } else { s + 1 };
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
    let global = flags.contains('g');
    let ic = flags.contains('i');
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
    loop {
        // Find the next match at or after `tail`.
        let mut found = None;
        for start in tail..=chars.len() {
            let mut groups = vec![None; re.ngroups + 1];
            if let Some(end) = re.match_alt(
                &re.branches,
                &chars,
                start,
                &mut groups,
                re.effective_ic(ic),
            ) {
                groups[0] = Some((start, end));
                found = Some((start, end, groups));
                break;
            }
        }
        let Some((s, e, groups)) = found else {
            break;
        };
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
    fn push(&mut self, out: &mut String, c: char) {
        let upper = self.one_shot.take().or(self.sustained);
        match upper {
            Some(true) => out.extend(c.to_uppercase()),
            Some(false) => out.extend(c.to_lowercase()),
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
/// `\\` → `\`, `\n` → NUL (0x00), `\r` → carriage return, `\t` → tab,
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
                    // Vim's `vim_regsub` replacement quirk: `\n` inserts a NUL
                    // (0x00), NOT a newline; `\r` inserts a carriage return
                    // (0x0d); `\t` a tab. (The PATTERN side is the opposite — there
                    // `\n` means newline.)
                    'n' => out.push('\0'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
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

    // Find the first match at or after `from` (match_alt is anchored, so scan).
    // Returns the separator span (`\zs`/`\ze`-adjusted, like Vim's startp/endp).
    let find_from = |from: usize| -> Option<(usize, usize)> {
        let mut p = from;
        while p <= n {
            let mut groups = vec![None; re.ngroups + 3];
            if let Some(end) = re.match_alt(&re.branches, &chars, p, &mut groups, eic) {
                let startp = groups[re.ngroups + 1].map_or(p, |(zp, _)| zp);
                let endp = groups[re.ngroups + 2].map_or(end, |(ep, _)| ep);
                return Some((startp, endp));
            }
            p += 1;
        }
        None
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

    #[test]
    fn basic_atoms_and_anchors() {
        assert!(regex_match("foo", "a foo b", false));
        assert!(regex_match("^foo", "foobar", false));
        assert!(!regex_match("^foo", "a foo", false));
        assert!(regex_match("bar$", "foobar", false));
        assert!(regex_match("f.o", "fxo", false));
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

    #[test]
    fn split_on_pattern() {
        assert_eq!(
            regex_split("a1b2c", "\\d", false, false),
            vec!["a", "b", "c"]
        );
    }
}
