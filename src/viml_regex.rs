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
//! ranges, the class atoms `\d \D \w \W \s \S \a \l \u \x` (+negations),
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
}

impl ClassItem {
    fn contains(&self, c: char) -> bool {
        match self {
            ClassItem::Ch(x) => *x == c,
            ClassItem::Range(a, b) => *a <= c && c <= *b,
            ClassItem::Digit => c.is_ascii_digit(),
            ClassItem::Word => c.is_alphanumeric() || c == '_',
            ClassItem::Space => c == ' ' || c == '\t',
            ClassItem::Alpha => c.is_alphabetic(),
            ClassItem::Lower => c.is_lowercase(),
            ClassItem::Upper => c.is_uppercase(),
            ClassItem::Hex => c.is_ascii_hexdigit(),
        }
    }
}

impl Class {
    fn matches(&self, c: char, ic: bool) -> bool {
        let hit = self.items.iter().any(|it| {
            if ic {
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
fn preprocess_magic(pat: &str) -> String {
    if !pat.contains("\\v") {
        return pat.to_string();
    }
    let chars: Vec<char> = pat.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut very = false;
    const OPS: &str = "(){}+?=|<>";
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            match chars[i + 1] {
                'v' => {
                    very = true;
                    i += 2;
                    continue;
                }
                'm' => {
                    very = false;
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        if !very {
            out.push(c);
            i += 1;
            continue;
        }
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
    out
}

// ── parser (magic mode) ──

struct Parser {
    p: Vec<char>,
    i: usize,
    ngroups: usize,
    forced_ic: Option<bool>,
}

impl Parser {
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
        let (min, max, greedy) = self.quantifier();
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
            '[' => {
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
        Some(match c {
            '(' => {
                let idx = self.ngroups + 1;
                self.ngroups = idx;
                let branches = self.alternation();
                self.close_group();
                Node::Group(branches, Some(idx))
            }
            '%' if self.peek() == Some('(') => {
                self.i += 1; // past '('
                let branches = self.alternation();
                self.close_group();
                Node::Group(branches, None)
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
            'c' => {
                self.forced_ic = Some(true);
                return self.atom(false);
            }
            'C' => {
                self.forced_ic = Some(false);
                return self.atom(false);
            }
            // `\1`..`\9` — backreference to a previously captured group.
            d @ '1'..='9' => Node::BackRef(d as usize - '0' as usize),
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
        }
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
            self.i += 1;
            // Range `a-z` (not when `-` is last before `]`).
            if self.peek() == Some('-') && self.peek2().is_some() && self.peek2() != Some(']') {
                self.i += 1;
                let hi = self.bump().unwrap();
                items.push(ClassItem::Range(c, hi));
            } else {
                items.push(ClassItem::Ch(c));
            }
        }
        Class { negated, items }
    }
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
        let pat = preprocess_magic(pat);
        let mut parser = Parser {
            p: pat.chars().collect(),
            i: 0,
            ngroups: 0,
            forced_ic: None,
        };
        let branches = parser.alternation();
        Regex {
            branches,
            ngroups: parser.ngroups,
            forced_ic: parser.forced_ic,
        }
    }

    fn effective_ic(&self, ic: bool) -> bool {
        self.forced_ic.unwrap_or(ic)
    }

    /// First match at or after each start position (leftmost). Returns char
    /// spans (whole + groups).
    pub fn find(&self, text: &[char], ic: bool) -> Option<Captures> {
        let ic = self.effective_ic(ic);
        for start in 0..=text.len() {
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
                // Zero-width match: count it once, then stop (avoid looping).
                Some(next) if next == cur && count < atom.min => {
                    positions.push(next);
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
                    Some(pos + 1)
                } else {
                    None
                }
            }
            Node::Any => {
                let ch = *text.get(pos)?;
                if ch != '\n' {
                    Some(pos + 1)
                } else {
                    None
                }
            }
            Node::Class(cl) => {
                let ch = *text.get(pos)?;
                if cl.matches(ch, ic) {
                    Some(pos + 1)
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

fn char_eq(a: char, b: char, ic: bool) -> bool {
    if ic {
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

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
    let mut pos = 0usize;
    loop {
        // Find the next match at or after `pos`.
        let mut found = None;
        for start in pos..=chars.len() {
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
        out.extend(&chars[pos..s]);
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
        // Advance; guard zero-width matches by emitting one char.
        if e > s {
            pos = e;
        } else if e < chars.len() {
            out.push(chars[e]);
            pos = e + 1;
        } else {
            break;
        }
        if !global {
            break;
        }
    }
    out.extend(&chars[pos.min(chars.len())..]);
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
/// `\\` → `\`, `\n`/`\t`/`\r` → control chars, `\u`/`\l`/`\U`/`\L`/`\e`/`\E` →
/// case folding (matching Vim's `vim_regsub` behaviour).
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
                    'n' => out.push('\n'),
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
        // c: advance past the match; on a zero-width match step one char so the
        // next search makes progress.
        col = if endp > str { 0 } else { 1 };
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
