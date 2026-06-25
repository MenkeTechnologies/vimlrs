//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! EXTENSION — NO `csrc/` COUNTERPART. (PORT.md synthesis-layer carve-out, the
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
}

/// Token kinds recognized in a Vimscript expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    /// Integer literal, already parsed.
    Number(i64),
    /// Float literal.
    Float(f64),
    /// String literal, already unescaped.
    Str(String),
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
    /// `}`
    RBrace,
    /// `,`
    Comma,
    /// `=`
    Assign,
    /// End of input.
    Eof,
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
                });
                return Ok(out);
            }
            let kind = self.next_token()?;
            out.push(Token { kind, span });
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
            // `&&` is the logical-AND operator; only a lone `&` is an option sigil.
            b'&' if self.peek2() == b'&' => self.lex_operator(),
            b'&' => Ok(self.lex_sigil_name(Tok::Option as fn(String) -> Tok)),
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
                _ => {}
            }
        }
        while self.peek().is_ascii_digit() {
            self.pos += 1;
        }
        let mut is_float = false;
        if self.peek() == b'.' && self.peek2().is_ascii_digit() {
            is_float = true;
            self.pos += 1;
            while self.peek().is_ascii_digit() {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), b'e' | b'E') {
            let save = self.pos;
            self.pos += 1;
            if matches!(self.peek(), b'+' | b'-') {
                self.pos += 1;
            }
            if self.peek().is_ascii_digit() {
                is_float = true;
                while self.peek().is_ascii_digit() {
                    self.pos += 1;
                }
            } else {
                self.pos = save;
            }
        }
        let text = &self.s[start..self.pos];
        if is_float {
            Tok::Float(text.parse::<f64>().unwrap_or(0.0))
        } else {
            Tok::Number(text.parse::<i64>().unwrap_or(0))
        }
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
                b'\\' => {
                    self.pos += 1;
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
                        b'x' | b'X' => {
                            let mut n = 0u32;
                            for _ in 0..2 {
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
                        0 => return Err(VimlError::msg("E114: Missing quote")),
                        other => out.push(other as char),
                    }
                }
                _ => out.push(self.next_char()),
            }
        }
    }

    fn lex_ident(&mut self) -> Tok {
        let start = self.pos;
        while matches!(self.peek(), b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_') {
            self.pos += 1;
        }
        if self.peek() == b':' && (self.pos - start) <= 1 {
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
    fn numbers_and_floats() {
        assert_eq!(kinds("0xff"), vec![Tok::Number(255), Tok::Eof]);
        assert_eq!(kinds("3.14"), vec![Tok::Float(3.14), Tok::Eof]);
        assert_eq!(
            kinds("1 . 2"),
            vec![Tok::Number(1), Tok::Dot, Tok::Number(2), Tok::Eof]
        );
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
