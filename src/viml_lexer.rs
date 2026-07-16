//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! EXTENSION — NO `vendor/` COUNTERPART. (PORT.md synthesis-layer carve-out, the
//! vimlrs analogue of zshrs's crate-root `fusevm_bridge.rs`/`compile_zsh.rs`.)
//!
//! Neovim's `eval.c` has no separate lexer — `eval1`…`eval7` scan characters
//! off the source string inline while evaluating. The bytecode frontend needs a
//! real token stream, so this is net-new code, NOT a port. It is bound by the
//! "no fake C names" rule only in the negative sense: nothing here may claim to
//! be a port or carry a `// c:` citation it doesn't have. The token set is still
//! dictated by what `eval.c` recognizes (operator spellings, literal forms,
//! sigil-prefixed names).
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// A Vimscript evaluation error carrying a Vim-style message (`E…:` code +
/// text), raised by the synthesis lexer/parser/compiler before execution. (At
/// run time the ported eval engine signals via `emsg`/`did_emsg`.)
#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct VimlError(pub String);

impl VimlError {
    /// Construct from any message string.
    pub fn msg(m: impl Into<String>) -> Self {
        VimlError(m.into())
    }
}

/// A lexical token plus its byte offset in the source (for diagnostics).
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// The token kind/value.
    pub kind: Tok,
    /// Byte offset of the token start in the source line.
    pub span: usize,
    /// Byte offset just past the token end (for adjacency checks like `d.key`
    /// member access vs `a . b` concatenation).
    pub end: usize,
}

/// Token kinds recognized in a Vimscript expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    /// Integer literal, already parsed.
    Number(i64),
    /// Float literal.
    Float(f64),
    /// Blob literal `0z00112233` — the decoded bytes.
    Blob(Vec<u8>),
    /// String literal, already unescaped.
    Str(String),
    /// Interpolated string `$'…{expr}…'` / `$"…{expr}…"` — the ordered list of
    /// literal chunks and raw `{expr}` sources, lowered by the parser to a
    /// concat of the chunks with each expression echo-stringified.
    InterpStr(Vec<InterpPart>),
    /// Bare identifier or scoped name (`x`, `g:foo`, `v:true`).
    Ident(String),
    /// Option reference `&name`.
    Option(String),
    /// Environment variable `$NAME`.
    Env(String),
    /// Register `@x`.
    Register(char),

    /// `?`
    Question,
    /// `??`
    QuestionQuestion,
    /// `:`
    Colon,
    /// `||`
    OrOr,
    /// `&&`
    AndAnd,
    /// A comparison operator, with its case-sensitivity flag.
    Cmp(CmpOp, CaseFlag),
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `.`
    Dot,
    /// `..`
    DotDot,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `%`
    Percent,
    /// `!`
    Bang,
    /// `->`
    Arrow,
    /// `=>` (vim9 lambda arrow)
    FatArrow,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `{`
    LBrace,
    /// `#{` — opens a literal-key Dict (`#{a: 1}`, bare-word keys).
    HashBrace,
    /// `}`
    RBrace,
    /// `,`
    Comma,
    /// `=`
    Assign,
    /// Un-lexable trailing text inside a re-split Float literal (the exponent
    /// junk of `'a' .. 1.0e300` → `1 . 0` + `e300`): Vim's single-pass
    /// evaluator only reports it (E15) AFTER the operands to its left are
    /// evaluated — a List LHS raises E730 first — so the parser turns this into
    /// an expression that errors at RUN time, not a parse error.
    DeferredErr(String),
    /// End of input.
    Eof,
}

/// One piece of an interpolated string literal (`$'…'` / `$"…"`): either a
/// resolved literal chunk (escapes, `''`, `{{`/`}}` already applied) or the raw
/// source text of a `{expr}` region to be sub-parsed and echo-stringified.
#[derive(Debug, Clone, PartialEq)]
pub enum InterpPart {
    /// Literal text with all escapes already resolved.
    Lit(String),
    /// Raw expression source that appeared between `{` and `}`.
    Expr(String),
}

/// The relational families recognized in `eval4` (`eval.c`). These map onto the
/// ported `exprtype_T` in the bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    /// `==`
    Equal,
    /// `!=`
    NotEqual,
    /// `=~`
    Match,
    /// `!~`
    NoMatch,
    /// `>`
    Greater,
    /// `>=`
    GreaterEqual,
    /// `<`
    Less,
    /// `<=`
    LessEqual,
    /// `is`
    Is,
    /// `isnot`
    IsNot,
}

/// Case-sensitivity suffix on a comparison (`==#` match-case, `==?` ignore-case,
/// bare `==` follows `'ignorecase'`). Mirrors the `ic` derivation in `eval4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseFlag {
    /// No suffix — follows `'ignorecase'` (default match-case here).
    Default,
    /// `#` — always match case.
    MatchCase,
    /// `?` — always ignore case.
    IgnoreCase,
}

/// Lex a Vimscript expression string into a token stream (ending in [`Tok::Eof`]).
pub fn lex(src: &str) -> Result<Vec<Token>, VimlError> {
    Lexer::new(src).run()
}

/// Lex as much of `src` as tokenizes, stopping (without error) at the first
/// byte that does not start a token. Returns the tokens lexed so far (ending in
/// [`Tok::Eof`] whose span is the stop offset) plus that stop offset.
///
/// `eval()` needs this: the C `f_eval` runs `eval1()` on the string, which
/// consumes one leading expression and never looks at what follows — so
/// `eval("a'quote")` is the variable `a` with trailing text, not a lex error.
/// Lexing the whole string up front turned such trailing text into E115/E15
/// before the leading expression was ever evaluated.
pub fn lex_prefix(src: &str) -> (Vec<Token>, usize) {
    let mut lx = Lexer::new(src);
    let mut out = Vec::new();
    loop {
        lx.skip_ws();
        let span = lx.pos;
        if lx.pos >= lx.src.len() {
            out.push(Token {
                kind: Tok::Eof,
                span,
                end: span,
            });
            return (out, span);
        }
        match lx.next_token() {
            Ok(kind) => out.push(Token {
                kind,
                span,
                end: lx.pos,
            }),
            Err(_) => {
                out.push(Token {
                    kind: Tok::Eof,
                    span,
                    end: span,
                });
                return (out, span);
            }
        }
    }
}

struct Lexer<'a> {
    src: &'a [u8],
    s: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn new(s: &'a str) -> Self {
        Lexer {
            src: s.as_bytes(),
            s,
            pos: 0,
        }
    }

    fn peek(&self) -> u8 {
        self.src.get(self.pos).copied().unwrap_or(0)
    }

    fn peek2(&self) -> u8 {
        self.src.get(self.pos + 1).copied().unwrap_or(0)
    }

    fn run(mut self) -> Result<Vec<Token>, VimlError> {
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            let span = self.pos;
            if self.pos >= self.src.len() {
                out.push(Token {
                    kind: Tok::Eof,
                    span,
                    end: span,
                });
                return Ok(out);
            }
            let kind = self.next_token()?;
            out.push(Token {
                kind,
                span,
                end: self.pos,
            });
        }
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), b' ' | b'\t' | b'\r' | b'\n') {
            self.pos += 1;
        }
    }

    fn next_token(&mut self) -> Result<Tok, VimlError> {
        let c = self.peek();
        match c {
            b'0'..=b'9' => Ok(self.lex_number()),
            b'\'' => self.lex_single_string(),
            b'"' => self.lex_double_string(),
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => Ok(self.lex_ident()),
            // `<SID>name` / `<SNR>123_name` are script-local function names, not a
            // `<` comparison: Vim accepts them wherever a function name is expected
            // (`:call <SID>Foo()`, `<SID>Foo(...)`, `function('<SID>Foo')`). The
            // `<SID>`/`<SNR>` marker is case-insensitive (userfunc.c STRNICMP), so
            // scan the whole marker plus the following name as one identifier and
            // let the parser treat it as a `Tok::Ident` (call or funcref name).
            b'<' if self.at_scriptid_name() => Ok(self.lex_scriptid_name()),
            // `#{` opens a literal-key Dict.
            b'#' if self.peek2() == b'{' => {
                self.pos += 2;
                Ok(Tok::HashBrace)
            }
            // `&&` is the logical-AND operator; only a lone `&` is an option sigil.
            b'&' if self.peek2() == b'&' => self.lex_operator(),
            b'&' => Ok(self.lex_sigil_name(Tok::Option as fn(String) -> Tok)),
            // `$'…'` / `$"…"` is an interpolated string (vim9 and legacy); a `$`
            // followed by a name is an environment-variable reference.
            b'$' if matches!(self.peek2(), b'\'' | b'"') => self.lex_interp_string(),
            b'$' => Ok(self.lex_sigil_name(Tok::Env as fn(String) -> Tok)),
            b'@' => {
                self.pos += 1;
                let r = self.peek() as char;
                if self.peek() != 0 {
                    self.pos += 1;
                }
                Ok(Tok::Register(r))
            }
            _ => self.lex_operator(),
        }
    }

    fn lex_number(&mut self) -> Tok {
        let start = self.pos;
        if self.peek() == b'0' {
            match self.peek2() {
                b'x' | b'X' => return self.lex_radix(16),
                b'b' | b'B' => return self.lex_radix(2),
                b'o' | b'O' => return self.lex_radix(8),
                b'z' | b'Z' => return self.lex_blob(),
                _ => {}
            }
        }
        while self.peek().is_ascii_digit() {
            self.pos += 1;
        }
        // c: eval_number() — the whole `.{digits}` / exponent scan below can
        // still be REJECTED (`get_float = false`), in which case the token is
        // only the leading integer (`vim_str2nr` re-reads from the start).
        let int_end = self.pos;
        let mut is_float = false;
        if self.peek() == b'.' && self.peek2().is_ascii_digit() {
            is_float = true;
            self.pos += 1;
            while self.peek().is_ascii_digit() {
                self.pos += 1;
            }
            // An exponent is only part of the literal after a `.{digits}`
            // fraction: Vim/Neovim's float grammar is
            // `[0-9]+\.[0-9]+([eE][+-]?[0-9]+)?`, so a dotless `1e100` is the
            // Number `1` followed by the name `e100` (an error at parse time),
            // never a float.
            if matches!(self.peek(), b'e' | b'E') {
                self.pos += 1;
                if matches!(self.peek(), b'+' | b'-') {
                    self.pos += 1;
                }
                if self.peek().is_ascii_digit() {
                    while self.peek().is_ascii_digit() {
                        self.pos += 1;
                    }
                } else {
                    // c: `if (!ascii_isdigit(*p)) get_float = false;` — a bare
                    // `1.5e`/`1.5e+` is NOT a float at all, just the Number 1.
                    is_float = false;
                }
            }
            // c: `if (ASCII_ISALPHA(*p) || *p == '.') get_float = false;` — a
            // trailing name char or second dot rejects the float wholesale:
            // `1.2.3` is `1 . 2 . 3` (three concatenated Numbers), `1.5x` the
            // Number 1 followed by `.5x`.
            if self.peek().is_ascii_alphabetic() || self.peek() == b'.' {
                is_float = false;
            }
            if !is_float {
                self.pos = int_end;
            }
        }
        let text = &self.s[start..self.pos];
        if is_float {
            return Tok::Float(text.parse::<f64>().unwrap_or(0.0));
        }
        // Vim octal literal: a leading `0` followed only by octal digits (`010`
        // == 8). A `8`/`9` anywhere (`08`, `0129`) keeps it decimal, matching
        // vim_str2nr's STR2NR_OCT detection in eval_number() (Src/eval.c).
        if text.len() > 1
            && text.starts_with('0')
            && text.bytes().all(|b| (b'0'..=b'7').contains(&b))
        {
            return Tok::Number(Self::saturating_literal(i64::from_str_radix(text, 8)));
        }
        Tok::Number(Self::saturating_literal(text.parse::<i64>()))
    }

    /// A too-large integer literal **saturates** at `VARNUMBER_MAX`, it does not
    /// become 0: `vim_str2nr` stops accumulating once the value would overflow and
    /// hands back the maximum, so Vim reads `9223372036854775808` as
    /// `9223372036854775807`. Returning 0 turned an out-of-range index into a valid
    /// one (`insert([1], 9, -9223372036854775808)` inserted at 0 instead of raising
    /// E684).
    fn saturating_literal(parsed: Result<i64, std::num::ParseIntError>) -> i64 {
        parsed.unwrap_or(i64::MAX)
    }

    /// Lex a Blob literal `0z` followed by an even number of hex digits (Vim
    /// also allows a `.` separating byte groups, e.g. `0z00.11`). Port of the
    /// `0z` branch of `eval_number()` (`Src/eval.c`).
    fn lex_blob(&mut self) -> Tok {
        self.pos += 2; // skip "0z"
        let mut bytes = Vec::new();
        loop {
            let hi = self.peek();
            if hi == b'.' {
                self.pos += 1;
                continue;
            }
            if !(hi as char).is_ascii_hexdigit() {
                break;
            }
            let lo = self.peek2();
            if !(lo as char).is_ascii_hexdigit() {
                // odd trailing nibble — stop (Vim requires pairs)
                break;
            }
            let s = &self.s[self.pos..self.pos + 2];
            bytes.push(u8::from_str_radix(s, 16).unwrap_or(0));
            self.pos += 2;
        }
        Tok::Blob(bytes)
    }

    fn lex_radix(&mut self, radix: u32) -> Tok {
        let start = self.pos;
        self.pos += 2;
        let digits_start = self.pos;
        while (self.peek() as char).is_digit(radix) {
            self.pos += 1;
        }
        let digits = &self.s[digits_start..self.pos];
        match i64::from_str_radix(digits, radix) {
            Ok(n) => Tok::Number(n),
            // An empty digit run is not a radix literal at all (rewind and let
            // `0` stand); a digit run that overflows saturates, as in decimal.
            Err(_) if !digits.is_empty() => Tok::Number(i64::MAX),
            Err(_) => {
                self.pos = start + 1;
                Tok::Number(0)
            }
        }
    }

    fn lex_single_string(&mut self) -> Result<Tok, VimlError> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            match self.peek() {
                0 => return Err(VimlError::msg("E115: Missing quote")),
                b'\'' => {
                    if self.peek2() == b'\'' {
                        out.push('\'');
                        self.pos += 2;
                    } else {
                        self.pos += 1;
                        return Ok(Tok::Str(out));
                    }
                }
                _ => out.push(self.next_char()),
            }
        }
    }

    fn lex_double_string(&mut self) -> Result<Tok, VimlError> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            match self.peek() {
                0 => return Err(VimlError::msg("E114: Missing quote")),
                b'"' => {
                    self.pos += 1;
                    return Ok(Tok::Str(out));
                }
                b'\\' => self.push_double_escape(&mut out)?,
                _ => out.push(self.next_char()),
            }
        }
    }

    /// Decode one backslash escape in a double-quoted string body (the leading
    /// `\` is at the current position) and push the resulting char(s) to `out`.
    /// Shared by [`Self::lex_double_string`] and the double-quote interpolated
    /// string body, so both honour the identical escape set.
    fn push_double_escape(&mut self, out: &mut String) -> Result<(), VimlError> {
        self.pos += 1; // consume the backslash
        let e = self.peek();
        self.pos += 1;
        match e {
            b'n' => out.push('\n'),
            b't' => out.push('\t'),
            b'r' => out.push('\r'),
            b'e' => out.push('\x1b'),
            b'b' => out.push('\x08'),
            b'\\' => out.push('\\'),
            b'"' => out.push('"'),
            b'0'..=b'7' => {
                let mut n = (e - b'0') as u32;
                for _ in 0..2 {
                    let d = self.peek();
                    if (b'0'..=b'7').contains(&d) {
                        n = n * 8 + (d - b'0') as u32;
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                if let Some(ch) = char::from_u32(n) {
                    out.push(ch);
                }
            }
            // c (eval.c:3590): `\x`/`\X` take 2 hex digits and store the raw
            // byte; `\u` takes 4 and `\U` 8, storing the codepoint as UTF-8.
            // Fewer digits is fine (`"\u41"` is `A`); *no* hex digit at all means
            // the escape is not one, and the letter is emitted literally
            // (`"a\uZZb"` is `auZZb`) — which is what the fallback arm does.
            b'x' | b'X' | b'u' | b'U' if (self.peek() as char).is_ascii_hexdigit() => {
                let maxlen = match e {
                    b'x' | b'X' => 2,
                    b'u' => 4,
                    _ => 8,
                };
                let mut n = 0u32;
                for _ in 0..maxlen {
                    let d = self.peek();
                    if (d as char).is_ascii_hexdigit() {
                        n = n * 16 + (d as char).to_digit(16).unwrap();
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                if let Some(ch) = char::from_u32(n) {
                    out.push(ch);
                }
            }
            // `\<Esc>`, `\<C-A>`, `\<Space>`, … — the special-key escape
            // (c: eval_string case '<' → trans_special). A key with no character
            // form (`\<Up>`, `\<F1>`: K_SPECIAL byte sequences) is not
            // translated and the `<` stays literal, as it did before.
            b'<' => {
                let rest = &self.s[self.pos - 1..];
                match crate::ported::keycodes::trans_special(rest) {
                    Some((c, used)) => {
                        out.push(c);
                        self.pos += used - 1; // the '<' was already consumed
                    }
                    None => out.push('<'),
                }
            }
            0 => return Err(VimlError::msg("E114: Missing quote")),
            other => out.push(other as char),
        }
        Ok(())
    }

    /// Lex an interpolated string `$'…'` (literal body, `''`→`'`, no escapes) or
    /// `$"…"` (double-quote body, backslash escapes apply). In both, `{expr}`
    /// marks an embedded expression, `{{`/`}}` (and, for `$"…"`, `\{`/`\}`) are
    /// literal braces, and a lone unmatched `}` is E1278. The `$` and opening
    /// quote are at the current position. Faithful to Vim 9.2's interpolated
    /// string (`:help interpolated-string`), which works in both vim9 and legacy.
    fn lex_interp_string(&mut self) -> Result<Tok, VimlError> {
        let double = self.peek2() == b'"';
        self.pos += 2; // skip `$` and the opening quote
        let quote = if double { b'"' } else { b'\'' };
        let mut parts: Vec<InterpPart> = Vec::new();
        let mut lit = String::new();
        loop {
            let c = self.peek();
            if c == 0 {
                return Err(VimlError::msg(if double {
                    "E114: Missing quote"
                } else {
                    "E115: Missing quote"
                }));
            }
            if double && c == b'\\' {
                // `\{`/`\}` fall through to the escape's default arm (literal
                // brace), so an escaped brace never opens an expression.
                self.push_double_escape(&mut lit)?;
                continue;
            }
            if c == quote {
                // A doubled `''` inside a `$'…'` body is a literal quote.
                if !double && self.peek2() == b'\'' {
                    lit.push('\'');
                    self.pos += 2;
                    continue;
                }
                self.pos += 1; // closing quote
                break;
            }
            match c {
                b'{' => {
                    if self.peek2() == b'{' {
                        lit.push('{');
                        self.pos += 2;
                    } else {
                        self.pos += 1; // consume `{`
                        if !lit.is_empty() {
                            parts.push(InterpPart::Lit(std::mem::take(&mut lit)));
                        }
                        parts.push(InterpPart::Expr(self.scan_interp_expr()?));
                    }
                }
                b'}' => {
                    if self.peek2() == b'}' {
                        lit.push('}');
                        self.pos += 2;
                    } else {
                        return Err(VimlError::msg("E1278: Stray '}' without a matching '{'"));
                    }
                }
                _ => lit.push(self.next_char()),
            }
        }
        if !lit.is_empty() {
            parts.push(InterpPart::Lit(lit));
        }
        Ok(Tok::InterpStr(parts))
    }

    /// Scan the source of one `{expr}` region: the opening `{` is already
    /// consumed; return the raw text up to (and consuming) the matching `}`.
    /// Nested `{…}` (dict literals) are depth-counted, and string literals
    /// `'…'`/`"…"` are skipped whole so a brace or quote inside them neither
    /// closes the expression nor the enclosing string. Unbalanced → E1279.
    fn scan_interp_expr(&mut self) -> Result<String, VimlError> {
        let start = self.pos;
        let mut depth = 1u32;
        loop {
            match self.peek() {
                0 => return Err(VimlError::msg("E1279: Missing '}'")),
                b'\'' => self.skip_sq_in_expr(),
                b'"' => self.skip_dq_in_expr(),
                b'{' => {
                    depth += 1;
                    self.pos += 1;
                }
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        let expr = self.s[start..self.pos].to_string();
                        self.pos += 1; // consume the matching `}`
                        return Ok(expr);
                    }
                    self.pos += 1;
                }
                _ => {
                    self.next_char();
                }
            }
        }
    }

    /// Skip a single-quoted string body inside an interpolation expression (the
    /// opening `'` is at the current position); `''` is an embedded quote.
    fn skip_sq_in_expr(&mut self) {
        self.pos += 1; // opening `'`
        loop {
            match self.peek() {
                0 => return,
                b'\'' => {
                    if self.peek2() == b'\'' {
                        self.pos += 2;
                    } else {
                        self.pos += 1;
                        return;
                    }
                }
                _ => {
                    self.next_char();
                }
            }
        }
    }

    /// Skip a double-quoted string body inside an interpolation expression (the
    /// opening `"` is at the current position); a `\` escapes the next byte.
    fn skip_dq_in_expr(&mut self) {
        self.pos += 1; // opening `"`
        loop {
            match self.peek() {
                0 => return,
                b'\\' => {
                    self.pos += 1;
                    if self.peek() != 0 {
                        self.next_char();
                    }
                }
                b'"' => {
                    self.pos += 1;
                    return;
                }
                _ => {
                    self.next_char();
                }
            }
        }
    }

    /// Whether the bytes at the cursor begin a script-local function name marker
    /// (`<SID>` or `<SNR>`, case-insensitive). Called only when `peek()` is `<`.
    fn at_scriptid_name(&self) -> bool {
        self.src
            .get(self.pos..self.pos + 5)
            .is_some_and(|m| m.eq_ignore_ascii_case(b"<SID>") || m.eq_ignore_ascii_case(b"<SNR>"))
    }

    /// Scan a `<SID>`/`<SNR>` script-local function name into a single
    /// `Tok::Ident` carrying the literal marker plus the following name (for
    /// `<SNR>` this includes the `123_` script-id prefix, which is ordinary
    /// name-tail bytes). The marker is preserved verbatim; the registry resolves
    /// `<SID>Foo` against the `func <SID>Foo()` definition stored under the same
    /// literal name.
    fn lex_scriptid_name(&mut self) -> Tok {
        let start = self.pos;
        self.pos += 5; // past `<SID>` / `<SNR>`
        while matches!(self.peek(), b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_') {
            self.pos += 1;
        }
        Tok::Ident(self.s[start..self.pos].to_string())
    }

    fn lex_ident(&mut self) -> Tok {
        let start = self.pos;
        while matches!(self.peek(), b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_') {
            self.pos += 1;
        }
        // A leading single-letter scope prefix (`a:`/`b:`/`g:`/`l:`/`s:`/`t:`/
        // `v:`/`w:`) absorbs its `:` and the name after it. Only the real scope
        // letters do this — otherwise `z:1` (a no-space ternary `?z:1`) or a
        // literal-Dict key `#{z:1}` would wrongly merge.
        if self.peek() == b':'
            && (self.pos - start) == 1
            && matches!(
                self.src[start],
                b'a' | b'b' | b'g' | b'l' | b's' | b't' | b'v' | b'w'
            )
        {
            self.pos += 1;
            while matches!(self.peek(), b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_') {
                self.pos += 1;
            }
        }
        while self.peek() == b'#'
            && matches!(self.peek2(), b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
        {
            self.pos += 1;
            while matches!(self.peek(), b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_') {
                self.pos += 1;
            }
        }
        Tok::Ident(self.s[start..self.pos].to_string())
    }

    fn lex_sigil_name(&mut self, ctor: fn(String) -> Tok) -> Tok {
        self.pos += 1;
        let start = self.pos;
        if matches!(self.peek(), b'l' | b'g') && self.peek2() == b':' {
            self.pos += 2;
        }
        while matches!(self.peek(), b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_') {
            self.pos += 1;
        }
        ctor(self.s[start..self.pos].to_string())
    }

    fn lex_operator(&mut self) -> Result<Tok, VimlError> {
        let c = self.peek();
        let c2 = self.peek2();
        macro_rules! cmp {
            ($op:expr, $len:expr) => {{
                self.pos += $len;
                let flag = match self.peek() {
                    b'#' => {
                        self.pos += 1;
                        CaseFlag::MatchCase
                    }
                    b'?' => {
                        self.pos += 1;
                        CaseFlag::IgnoreCase
                    }
                    _ => CaseFlag::Default,
                };
                return Ok(Tok::Cmp($op, flag));
            }};
        }
        match (c, c2) {
            (b'?', b'?') => {
                self.pos += 2;
                Ok(Tok::QuestionQuestion)
            }
            (b'?', _) => {
                self.pos += 1;
                Ok(Tok::Question)
            }
            (b':', _) => {
                self.pos += 1;
                Ok(Tok::Colon)
            }
            (b'|', b'|') => {
                self.pos += 2;
                Ok(Tok::OrOr)
            }
            (b'&', b'&') => {
                self.pos += 2;
                Ok(Tok::AndAnd)
            }
            (b'=', b'=') => cmp!(CmpOp::Equal, 2),
            (b'=', b'~') => cmp!(CmpOp::Match, 2),
            // `=>` is the vim9 lambda arrow (`(a, b) => a + b`).
            (b'=', b'>') => {
                self.pos += 2;
                Ok(Tok::FatArrow)
            }
            (b'=', _) => {
                self.pos += 1;
                Ok(Tok::Assign)
            }
            (b'!', b'=') => cmp!(CmpOp::NotEqual, 2),
            (b'!', b'~') => cmp!(CmpOp::NoMatch, 2),
            (b'!', _) => {
                self.pos += 1;
                Ok(Tok::Bang)
            }
            (b'>', b'=') => cmp!(CmpOp::GreaterEqual, 2),
            (b'>', _) => cmp!(CmpOp::Greater, 1),
            (b'<', b'=') => cmp!(CmpOp::LessEqual, 2),
            (b'<', _) => cmp!(CmpOp::Less, 1),
            (b'-', b'>') => {
                self.pos += 2;
                Ok(Tok::Arrow)
            }
            (b'+', _) => {
                self.pos += 1;
                Ok(Tok::Plus)
            }
            (b'-', _) => {
                self.pos += 1;
                Ok(Tok::Minus)
            }
            (b'.', b'.') => {
                self.pos += 2;
                Ok(Tok::DotDot)
            }
            (b'.', _) => {
                self.pos += 1;
                Ok(Tok::Dot)
            }
            (b'*', _) => {
                self.pos += 1;
                Ok(Tok::Star)
            }
            (b'/', _) => {
                self.pos += 1;
                Ok(Tok::Slash)
            }
            (b'%', _) => {
                self.pos += 1;
                Ok(Tok::Percent)
            }
            (b'(', _) => {
                self.pos += 1;
                Ok(Tok::LParen)
            }
            (b')', _) => {
                self.pos += 1;
                Ok(Tok::RParen)
            }
            (b'[', _) => {
                self.pos += 1;
                Ok(Tok::LBracket)
            }
            (b']', _) => {
                self.pos += 1;
                Ok(Tok::RBracket)
            }
            (b'{', _) => {
                self.pos += 1;
                Ok(Tok::LBrace)
            }
            (b'}', _) => {
                self.pos += 1;
                Ok(Tok::RBrace)
            }
            (b',', _) => {
                self.pos += 1;
                Ok(Tok::Comma)
            }
            _ => Err(VimlError::msg(format!(
                "E15: Invalid expression: unexpected '{}'",
                c as char
            ))),
        }
    }

    fn next_char(&mut self) -> char {
        let ch = self.s[self.pos..].chars().next().unwrap_or('\0');
        self.pos += ch.len_utf8();
        ch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<Tok> {
        lex(src).unwrap().into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn blob_literals() {
        assert_eq!(
            kinds("0z00112233"),
            vec![Tok::Blob(vec![0, 17, 34, 51]), Tok::Eof]
        );
        assert_eq!(
            kinds("0zDEADBEEF"),
            vec![Tok::Blob(vec![0xde, 0xad, 0xbe, 0xef]), Tok::Eof]
        );
        assert_eq!(kinds("0z00.11"), vec![Tok::Blob(vec![0, 17]), Tok::Eof]);
        assert_eq!(kinds("0z"), vec![Tok::Blob(vec![]), Tok::Eof]);
    }

    #[test]
    // `3.14` here is a lexer fixture, not an attempt to express π.
    #[allow(clippy::approx_constant)]
    fn numbers_and_floats() {
        assert_eq!(kinds("0xff"), vec![Tok::Number(255), Tok::Eof]);
        assert_eq!(kinds("3.14"), vec![Tok::Float(3.14), Tok::Eof]);
        assert_eq!(
            kinds("1 . 2"),
            vec![Tok::Number(1), Tok::Dot, Tok::Number(2), Tok::Eof]
        );
    }

    #[test]
    fn octal_literals() {
        // Leading 0 + only octal digits → octal (Vim semantics).
        assert_eq!(kinds("010"), vec![Tok::Number(8), Tok::Eof]);
        assert_eq!(kinds("0777"), vec![Tok::Number(511), Tok::Eof]);
        assert_eq!(kinds("017"), vec![Tok::Number(15), Tok::Eof]);
        // A 8/9 digit makes it decimal; bare 0 stays 0; floats untouched.
        assert_eq!(kinds("08"), vec![Tok::Number(8), Tok::Eof]);
        assert_eq!(kinds("0129"), vec![Tok::Number(129), Tok::Eof]);
        assert_eq!(kinds("0"), vec![Tok::Number(0), Tok::Eof]);
        assert_eq!(kinds("0.5"), vec![Tok::Float(0.5), Tok::Eof]);
    }

    #[test]
    fn strings_and_ops() {
        assert_eq!(kinds("'a''b'"), vec![Tok::Str("a'b".into()), Tok::Eof]);
        assert_eq!(
            kinds("==#"),
            vec![Tok::Cmp(CmpOp::Equal, CaseFlag::MatchCase), Tok::Eof]
        );
    }
}
