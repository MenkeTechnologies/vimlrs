//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! EXTENSION — NO `csrc/` COUNTERPART. Recursive-descent parser building the
//! synthesis AST. The expression grammar transcribes the precedence ladder in
//! `eval.c` (`eval1`…`eval7`) but builds a tree instead of evaluating inline:
//!
//! ```text
//! eval1  expr2 ? expr1 : expr1   |  expr2 ?? expr1
//! eval2  expr3 || expr3
//! eval3  expr4 && expr4
//! eval4  expr5 (== != =~ !~ > >= < <= is isnot) expr5   (no assoc)
//! eval5  expr6 (+ - . ..) expr6
//! eval6  expr7 (* / %) expr7
//! eval7  (! - +)* primary subscripts
//! ```
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use crate::viml_ast::{ArithOp, Expr, ForVars, LetTarget, Stmt, UnaryOp};
use crate::viml_lexer::{lex, CaseFlag, CmpOp, Tok, Token, VimlError};

/// The small set of names Phase 3 recognizes as builtin function calls. The
/// full `funcs.c` table is ported in Phase 5.
pub const PHASE3_BUILTINS: &[&str] = &[
    "len", "type", "string", "empty", "abs", "str2nr", "str2float", "float2nr",
];

/// Parse one statement line into a [`Stmt`].
///
/// Inline trailing comments (`echo 1  " note`) are not stripped in Phase 3: a
/// `"` is a comment only where the command grammar expects end-of-command, but
/// in expression position it opens a string. Full-line comments are skipped by
/// the source splitter before this is called.
pub fn parse_stmt(line: &str) -> Result<Stmt, VimlError> {
    let line = line.trim();
    let cmd_end = line
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(line.len());
    let cmd = &line[..cmd_end];
    let rest = line[cmd_end..].trim_start();

    match cmd {
        "echo" => Ok(Stmt::Echo(parse_expr_list(rest)?)),
        "echon" => Ok(Stmt::Echon(parse_expr_list(rest)?)),
        "echomsg" | "echom" => Ok(Stmt::Echo(parse_expr_list(rest)?)),
        "execute" | "exe" => Ok(Stmt::Execute(parse_expr_list(rest)?)),
        "set" | "se" => Ok(Stmt::Set(rest.to_string())),
        "let" => parse_let(rest),
        "call" => Ok(Stmt::Call(parse_expr(rest)?)),
        "eval" => Ok(Stmt::Expr(parse_expr(rest)?)),
        "break" => Ok(Stmt::Break),
        "continue" | "cont" => Ok(Stmt::Continue),
        "return" => Ok(if rest.trim().is_empty() {
            Stmt::Return(None)
        } else {
            Stmt::Return(Some(parse_expr(rest)?))
        }),
        "throw" => Ok(Stmt::Throw(parse_expr(rest)?)),
        _ => Ok(Stmt::Expr(parse_expr(line)?)),
    }
}

/// Split a statement line into its leading command word (ASCII letters) and the
/// remaining text. A line starting with non-alphabetic text is a bare
/// expression (empty command word).
fn cmd_word(line: &str) -> (&str, &str) {
    let line = line.trim();
    let end = line.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(line.len());
    (&line[..end], line[end..].trim_start())
}

/// Whether `cmd` closes or continues a block (so it must be handled by the
/// block parser, never as a leaf statement).
fn is_block_terminator(cmd: &str) -> bool {
    matches!(
        cmd,
        "endif" | "elseif" | "else" | "endwhile" | "endwh" | "endfor" | "endfunction" | "endfunc"
            | "catch" | "finally" | "endtry"
    )
}

/// Parse a whole source block into a flat statement list with block structure
/// (the `:if`/`:while`/`:for`/`:function`/`:try` bodies nested inside).
pub fn parse_program(src: &str) -> Result<Vec<Stmt>, VimlError> {
    Ok(parse_program_lines(src)?.into_iter().map(|(_, s)| s).collect())
}

/// Like [`parse_program`] but pairs each TOP-LEVEL statement with its 1-based
/// source line (for the debugger's statement markers).
pub fn parse_program_lines(src: &str) -> Result<Vec<(u32, Stmt)>, VimlError> {
    let mut cur = Lines::new(src);
    let mut out = Vec::new();
    loop {
        cur.skip_blanks();
        let Some(line) = cur.peek() else { break };
        let (cmd, _) = cmd_word(line);
        if is_block_terminator(cmd) {
            return Err(VimlError::msg(format!("E580: `:{cmd}` without matching block opener")));
        }
        let lineno = cur.line_no();
        for s in parse_one(&mut cur)? {
            out.push((lineno, s));
        }
    }
    Ok(out)
}

/// Cursor over the physical lines of a source block. `i` is the 0-based index of
/// the next line; `line_no()` is its 1-based source line.
struct Lines<'a> {
    lines: Vec<&'a str>,
    i: usize,
}

impl<'a> Lines<'a> {
    fn new(src: &'a str) -> Self {
        Lines {
            lines: src.lines().collect(),
            i: 0,
        }
    }

    /// Advance past blank lines and full-line `"` comments.
    fn skip_blanks(&mut self) {
        while let Some(l) = self.lines.get(self.i) {
            let t = l.trim();
            if t.is_empty() || t.starts_with('"') {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<&'a str> {
        self.lines.get(self.i).copied()
    }

    fn bump(&mut self) {
        self.i += 1;
    }

    fn line_no(&self) -> u32 {
        self.i as u32 + 1
    }
}

/// Parse the statement(s) on the line at the cursor. A block opener yields one
/// `Stmt`; a leaf line yields one statement per `|`-separated command (Vim's
/// `do_one_cmd` bar split), so `let l = [1] | echo l` is two statements.
fn parse_one(cur: &mut Lines) -> Result<Vec<Stmt>, VimlError> {
    let line = cur.peek().expect("parse_one called at EOF");
    let (cmd, rest) = cmd_word(line);
    match cmd {
        "if" => {
            cur.bump();
            Ok(vec![parse_if(cur, rest)?])
        }
        "while" => {
            cur.bump();
            Ok(vec![parse_while(cur, rest)?])
        }
        "for" => {
            cur.bump();
            Ok(vec![parse_for(cur, rest)?])
        }
        "try" => {
            cur.bump();
            Ok(vec![parse_try(cur)?])
        }
        "function" | "func" => {
            cur.bump();
            Ok(vec![parse_function(cur, rest)?])
        }
        _ => {
            cur.bump();
            let mut out = Vec::new();
            for seg in split_commands(line) {
                if seg.trim().is_empty() {
                    continue;
                }
                out.push(parse_stmt(seg)?);
            }
            Ok(out)
        }
    }
}

/// Split a command line into its `|`-separated commands, the way Vim's
/// `do_one_cmd` does. A `|` ends a command except when it is inside a string,
/// part of the `||` operator, backslash-escaped, or after a `"` line comment.
fn split_commands(line: &str) -> Vec<&str> {
    let bytes = line.as_bytes();
    let mut segs = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut sq = false; // inside a single-quoted string ('' is an escaped quote)
    let mut dq = false; // inside a double-quoted string (\ escapes)
    while i < bytes.len() {
        let c = bytes[i];
        if sq {
            if c == b'\'' {
                if bytes.get(i + 1) == Some(&b'\'') {
                    i += 2;
                    continue;
                }
                sq = false;
            }
            i += 1;
            continue;
        }
        if dq {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'"' {
                dq = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'\'' => sq = true,
            b'"' => {
                // A `"` with only whitespace before it in this command is a
                // line comment → ignore the rest of the line.
                if line[start..i].trim().is_empty() {
                    let seg = &line[start..i];
                    if !seg.trim().is_empty() {
                        segs.push(seg);
                    }
                    return segs;
                }
                dq = true;
            }
            b'|' => {
                if bytes.get(i + 1) == Some(&b'|') {
                    i += 2; // `||` logical-or, not a separator
                    continue;
                }
                if i > 0 && bytes[i - 1] == b'\\' {
                    i += 1; // escaped bar
                    continue;
                }
                segs.push(&line[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    let tail = &line[start..];
    if !tail.trim().is_empty() {
        segs.push(tail);
    }
    segs
}

/// Parse statements until a terminator in `terms`. Returns the body and the
/// terminator `(cmd, rest)` it stopped on (`None` at EOF).
fn parse_block(cur: &mut Lines, terms: &[&str]) -> Result<(Vec<Stmt>, Option<(String, String)>), VimlError> {
    let mut stmts = Vec::new();
    loop {
        cur.skip_blanks();
        let Some(line) = cur.peek() else {
            return Ok((stmts, None));
        };
        let (cmd, rest) = cmd_word(line);
        if terms.contains(&cmd) {
            cur.bump();
            return Ok((stmts, Some((cmd.to_string(), rest.to_string()))));
        }
        if is_block_terminator(cmd) {
            return Err(VimlError::msg(format!("E580: unexpected `:{cmd}`")));
        }
        stmts.extend(parse_one(cur)?);
    }
}

const IF_TERMS: &[&str] = &["elseif", "else", "endif"];

fn parse_if(cur: &mut Lines, cond_str: &str) -> Result<Stmt, VimlError> {
    let mut arms = Vec::new();
    let mut else_body = None;
    let (body, mut term) = parse_block(cur, IF_TERMS)?;
    arms.push((parse_expr(cond_str)?, body));
    loop {
        match term {
            Some((ref c, ref rest)) if c == "elseif" => {
                let cond = parse_expr(rest)?;
                let (b, t) = parse_block(cur, IF_TERMS)?;
                arms.push((cond, b));
                term = t;
            }
            Some((ref c, _)) if c == "else" => {
                let (b, t) = parse_block(cur, &["endif"])?;
                else_body = Some(b);
                if t.is_none() {
                    return Err(VimlError::msg("E171: Missing :endif"));
                }
                break;
            }
            Some((ref c, _)) if c == "endif" => break,
            None => return Err(VimlError::msg("E171: Missing :endif")),
            Some((c, _)) => return Err(VimlError::msg(format!("E580: unexpected `:{c}` in :if"))),
        }
    }
    Ok(Stmt::If { arms, else_body })
}

fn parse_while(cur: &mut Lines, cond_str: &str) -> Result<Stmt, VimlError> {
    let cond = parse_expr(cond_str)?;
    let (body, term) = parse_block(cur, &["endwhile", "endwh"])?;
    if term.is_none() {
        return Err(VimlError::msg("E170: Missing :endwhile"));
    }
    Ok(Stmt::While { cond, body })
}

fn parse_for(cur: &mut Lines, header: &str) -> Result<Stmt, VimlError> {
    // `{var} in {expr}` — split on the first whitespace-delimited `in`.
    let idx = header
        .find(" in ")
        .ok_or_else(|| VimlError::msg("E690: Missing \"in\" after :for"))?;
    let var = header[..idx].trim();
    let vars = if let Some(inner) = var.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        ForVars::List(
            inner
                .split(',')
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty())
                .collect(),
        )
    } else {
        ForVars::One(var.to_string())
    };
    let iter = parse_expr(header[idx + 4..].trim())?;
    let (body, term) = parse_block(cur, &["endfor"])?;
    if term.is_none() {
        return Err(VimlError::msg("E170: Missing :endfor"));
    }
    Ok(Stmt::For { vars, iter, body })
}

fn parse_function(cur: &mut Lines, header: &str) -> Result<Stmt, VimlError> {
    // `[!] {name}({a}, {b}, …) [flags]`.
    let header = header.trim();
    let (bang, header) = match header.strip_prefix('!') {
        Some(rest) => (true, rest.trim()),
        None => (false, header),
    };
    let lparen = header
        .find('(')
        .ok_or_else(|| VimlError::msg("E124: Missing '(' in :function"))?;
    let name = header[..lparen].trim().to_string();
    let rparen = header[lparen..]
        .find(')')
        .map(|r| lparen + r)
        .ok_or_else(|| VimlError::msg("E125: Missing ')' in :function"))?;
    let args: Vec<String> = header[lparen + 1..rparen]
        .split(',')
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect();
    let (body, term) = parse_block(cur, &["endfunction", "endfunc"])?;
    if term.is_none() {
        return Err(VimlError::msg("E126: Missing :endfunction"));
    }
    Ok(Stmt::Function {
        name,
        args,
        body,
        bang,
    })
}

fn parse_try(cur: &mut Lines) -> Result<Stmt, VimlError> {
    const TRY_TERMS: &[&str] = &["catch", "finally", "endtry"];
    let (body, mut term) = parse_block(cur, TRY_TERMS)?;
    let mut catches = Vec::new();
    let mut finally = None;
    loop {
        match term {
            Some((ref c, ref rest)) if c == "catch" => {
                let pat = {
                    let r = rest.trim();
                    if r.is_empty() {
                        None
                    } else {
                        Some(r.trim_matches('/').to_string())
                    }
                };
                let (b, t) = parse_block(cur, TRY_TERMS)?;
                catches.push((pat, b));
                term = t;
            }
            Some((ref c, _)) if c == "finally" => {
                let (b, t) = parse_block(cur, &["endtry"])?;
                finally = Some(b);
                if t.is_none() {
                    return Err(VimlError::msg("E170: Missing :endtry"));
                }
                break;
            }
            Some((ref c, _)) if c == "endtry" => break,
            None => return Err(VimlError::msg("E170: Missing :endtry")),
            Some((c, _)) => return Err(VimlError::msg(format!("E580: unexpected `:{c}` in :try"))),
        }
    }
    Ok(Stmt::Try {
        body,
        catches,
        finally,
    })
}

fn parse_let(rest: &str) -> Result<Stmt, VimlError> {
    let eq = rest
        .find('=')
        .ok_or_else(|| VimlError::msg("E121: let requires '='"))?;
    // Compound assignment (`+= -= *= /= %= .=`, ex_let's `tv_op`): the char just
    // before `=` is the operator. A `:let` target never ends in one of these, so
    // its presence unambiguously marks a compound assign.
    let op = match rest[..eq].as_bytes().last() {
        Some(b'+') => Some(ArithOp::Add),
        Some(b'-') => Some(ArithOp::Sub),
        Some(b'*') => Some(ArithOp::Mul),
        Some(b'/') => Some(ArithOp::Div),
        Some(b'%') => Some(ArithOp::Mod),
        Some(b'.') => Some(ArithOp::Concat),
        _ => None,
    };
    let lhs_end = if op.is_some() { eq - 1 } else { eq };
    let lhs = rest[..lhs_end].trim();
    let rhs = rest[eq + 1..].trim();
    let target = if let Some(inner) = lhs.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        // `[a, b]` or `[a, b; rest]` list-unpack.
        let (head, rest_name) = match inner.split_once(';') {
            Some((h, r)) => (h, Some(r.trim().to_string())),
            None => (inner, None),
        };
        let names = head
            .split(',')
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .collect();
        LetTarget::List {
            names,
            rest: rest_name,
        }
    } else if let Some(name) = lhs.strip_prefix('&') {
        LetTarget::Option(name.to_string())
    } else if let Some(name) = lhs.strip_prefix('$') {
        LetTarget::Env(name.to_string())
    } else if let Some(reg) = lhs.strip_prefix('@') {
        LetTarget::Register(reg.chars().next().unwrap_or('"'))
    } else {
        LetTarget::Var(lhs.to_string())
    };
    // Plain `=` stores the RHS directly; `op=` desugars to `target = target op rhs`
    // (`tv_op` semantics), reusing the same store path so it stays JIT-eligible.
    let expr = match op {
        None => parse_expr(rhs)?,
        Some(op) => {
            let cur = let_target_expr(&target)?;
            Expr::Arith {
                op,
                lhs: Box::new(cur),
                rhs: Box::new(parse_expr(rhs)?),
            }
        }
    };
    Ok(Stmt::Let { target, expr })
}

/// The current value of a compound-assignment target, as an expression. List
/// unpack targets cannot take a compound operator (`E734`).
fn let_target_expr(target: &LetTarget) -> Result<Expr, VimlError> {
    Ok(match target {
        LetTarget::Var(n) => Expr::Var(n.clone()),
        LetTarget::Option(n) => Expr::Option(n.clone()),
        LetTarget::Env(n) => Expr::Env(n.clone()),
        LetTarget::Register(c) => Expr::Register(*c),
        LetTarget::List { .. } => {
            return Err(VimlError::msg("E734: Wrong variable type for +="))
        }
    })
}

fn parse_expr_list(src: &str) -> Result<Vec<Expr>, VimlError> {
    if src.trim().is_empty() {
        return Ok(Vec::new());
    }
    let toks = lex(src)?;
    let mut p = Parser::new(toks);
    let mut out = Vec::new();
    loop {
        out.push(p.eval1()?);
        if matches!(p.peek(), Tok::Eof) {
            break;
        }
    }
    Ok(out)
}

/// Parse a single expression string into an [`Expr`].
pub fn parse_expr(src: &str) -> Result<Expr, VimlError> {
    let toks = lex(src)?;
    let mut p = Parser::new(toks);
    let e = p.eval1()?;
    if !matches!(p.peek(), Tok::Eof) {
        return Err(VimlError::msg(
            "E15: Invalid expression: trailing tokens".to_string(),
        ));
    }
    Ok(e)
}

struct Parser {
    toks: Vec<Token>,
    i: usize,
}

impl Parser {
    fn new(toks: Vec<Token>) -> Self {
        Parser { toks, i: 0 }
    }

    fn peek(&self) -> &Tok {
        &self.toks[self.i].kind
    }

    fn advance(&mut self) -> Tok {
        let t = self.toks[self.i].kind.clone();
        if self.i + 1 < self.toks.len() {
            self.i += 1;
        }
        t
    }

    fn eat(&mut self, want: &Tok) -> Result<(), VimlError> {
        if self.peek() == want {
            self.advance();
            Ok(())
        } else {
            Err(VimlError::msg(format!(
                "E15: expected {want:?}, found {:?}",
                self.peek()
            )))
        }
    }

    fn eval1(&mut self) -> Result<Expr, VimlError> {
        let cond = self.eval2()?;
        match self.peek() {
            Tok::Question => {
                self.advance();
                let then = self.eval1()?;
                self.eat(&Tok::Colon)?;
                let otherwise = self.eval1()?;
                Ok(Expr::Ternary {
                    cond: Box::new(cond),
                    then: Box::new(then),
                    otherwise: Box::new(otherwise),
                })
            }
            Tok::QuestionQuestion => {
                self.advance();
                let rhs = self.eval1()?;
                Ok(Expr::Coalesce(Box::new(cond), Box::new(rhs)))
            }
            _ => Ok(cond),
        }
    }

    fn eval2(&mut self) -> Result<Expr, VimlError> {
        let mut lhs = self.eval3()?;
        while matches!(self.peek(), Tok::OrOr) {
            self.advance();
            let rhs = self.eval3()?;
            lhs = Expr::Or(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn eval3(&mut self) -> Result<Expr, VimlError> {
        let mut lhs = self.eval4()?;
        while matches!(self.peek(), Tok::AndAnd) {
            self.advance();
            let rhs = self.eval4()?;
            lhs = Expr::And(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn eval4(&mut self) -> Result<Expr, VimlError> {
        let lhs = self.eval5()?;
        let (op, case) = match self.peek() {
            Tok::Cmp(op, case) => (*op, *case),
            Tok::Ident(id) if id == "is" => (CmpOp::Is, CaseFlag::Default),
            Tok::Ident(id) if id == "isnot" => (CmpOp::IsNot, CaseFlag::Default),
            _ => return Ok(lhs),
        };
        self.advance();
        let rhs = self.eval5()?;
        Ok(Expr::Compare {
            op,
            case,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        })
    }

    fn eval5(&mut self) -> Result<Expr, VimlError> {
        let mut lhs = self.eval6()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => ArithOp::Add,
                Tok::Minus => ArithOp::Sub,
                Tok::Dot | Tok::DotDot => ArithOp::Concat,
                _ => break,
            };
            self.advance();
            let rhs = self.eval6()?;
            lhs = Expr::Arith {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn eval6(&mut self) -> Result<Expr, VimlError> {
        let mut lhs = self.eval7()?;
        loop {
            let op = match self.peek() {
                Tok::Star => ArithOp::Mul,
                Tok::Slash => ArithOp::Div,
                Tok::Percent => ArithOp::Mod,
                _ => break,
            };
            self.advance();
            let rhs = self.eval7()?;
            lhs = Expr::Arith {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn eval7(&mut self) -> Result<Expr, VimlError> {
        let mut leaders = Vec::new();
        loop {
            match self.peek() {
                Tok::Bang => {
                    self.advance();
                    leaders.push(UnaryOp::Not);
                }
                Tok::Minus => {
                    self.advance();
                    leaders.push(UnaryOp::Neg);
                }
                Tok::Plus => {
                    self.advance();
                    leaders.push(UnaryOp::Plus);
                }
                _ => break,
            }
        }
        let mut e = self.primary()?;
        e = self.postfix(e)?;
        for op in leaders.into_iter().rev() {
            e = Expr::Unary {
                op,
                expr: Box::new(e),
            };
        }
        Ok(e)
    }

    fn primary(&mut self) -> Result<Expr, VimlError> {
        match self.advance() {
            Tok::Number(n) => Ok(Expr::Number(n)),
            Tok::Float(f) => Ok(Expr::Float(f)),
            Tok::Str(s) => Ok(Expr::Str(s)),
            Tok::Option(o) => Ok(Expr::Option(o)),
            Tok::Env(e) => Ok(Expr::Env(e)),
            Tok::Register(r) => Ok(Expr::Register(r)),
            Tok::LParen => {
                let e = self.eval1()?;
                self.eat(&Tok::RParen)?;
                Ok(e)
            }
            Tok::LBracket => self.list_literal(),
            Tok::LBrace => self.dict_literal(),
            Tok::Ident(name) => {
                if matches!(self.peek(), Tok::LParen) {
                    self.advance();
                    let args = self.arg_list(&Tok::RParen)?;
                    Ok(Expr::Call { name, args })
                } else {
                    Ok(Expr::Var(name))
                }
            }
            other => Err(VimlError::msg(format!(
                "E15: Invalid expression: unexpected {other:?}"
            ))),
        }
    }

    /// Postfix subscripts: `[index]`, `[from:to]`, `->method()`. (`.name` dict
    /// member access is deferred — in Phase 3 `.` is concat; use `d['key']`.)
    fn postfix(&mut self, mut base: Expr) -> Result<Expr, VimlError> {
        loop {
            match self.peek() {
                Tok::LBracket => {
                    self.advance();
                    base = self.subscript(base)?;
                }
                Tok::Arrow => {
                    self.advance();
                    let name = match self.advance() {
                        Tok::Ident(n) => n,
                        other => {
                            return Err(VimlError::msg(format!(
                                "E15: expected method name after '->', found {other:?}"
                            )))
                        }
                    };
                    self.eat(&Tok::LParen)?;
                    let args = self.arg_list(&Tok::RParen)?;
                    base = Expr::Method {
                        base: Box::new(base),
                        name,
                        args,
                    };
                }
                _ => break,
            }
        }
        Ok(base)
    }

    fn subscript(&mut self, base: Expr) -> Result<Expr, VimlError> {
        if matches!(self.peek(), Tok::Colon) {
            self.advance();
            let to = if matches!(self.peek(), Tok::RBracket) {
                None
            } else {
                Some(Box::new(self.eval1()?))
            };
            self.eat(&Tok::RBracket)?;
            return Ok(Expr::Slice {
                base: Box::new(base),
                from: None,
                to,
            });
        }
        let first = self.eval1()?;
        if matches!(self.peek(), Tok::Colon) {
            self.advance();
            let to = if matches!(self.peek(), Tok::RBracket) {
                None
            } else {
                Some(Box::new(self.eval1()?))
            };
            self.eat(&Tok::RBracket)?;
            Ok(Expr::Slice {
                base: Box::new(base),
                from: Some(Box::new(first)),
                to,
            })
        } else {
            self.eat(&Tok::RBracket)?;
            Ok(Expr::Index {
                base: Box::new(base),
                index: Box::new(first),
            })
        }
    }

    fn list_literal(&mut self) -> Result<Expr, VimlError> {
        Ok(Expr::List(self.arg_list(&Tok::RBracket)?))
    }

    fn dict_literal(&mut self) -> Result<Expr, VimlError> {
        let mut pairs = Vec::new();
        if matches!(self.peek(), Tok::RBrace) {
            self.advance();
            return Ok(Expr::Dict(pairs));
        }
        loop {
            let key = self.eval1()?;
            self.eat(&Tok::Colon)?;
            let val = self.eval1()?;
            pairs.push((key, val));
            match self.advance() {
                Tok::Comma => {
                    if matches!(self.peek(), Tok::RBrace) {
                        self.advance();
                        break;
                    }
                }
                Tok::RBrace => break,
                other => {
                    return Err(VimlError::msg(format!(
                        "E15: expected ',' or '}}' in dict, found {other:?}"
                    )))
                }
            }
        }
        Ok(Expr::Dict(pairs))
    }

    fn arg_list(&mut self, close: &Tok) -> Result<Vec<Expr>, VimlError> {
        let mut args = Vec::new();
        if self.peek() == close {
            self.advance();
            return Ok(args);
        }
        loop {
            args.push(self.eval1()?);
            match self.advance() {
                Tok::Comma => {
                    if self.peek() == close {
                        self.advance();
                        break;
                    }
                }
                ref t if t == close => break,
                other => {
                    return Err(VimlError::msg(format!(
                        "E15: expected ',' or {close:?}, found {other:?}"
                    )))
                }
            }
        }
        Ok(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_add_mul() {
        match parse_expr("1 + 2 * 3").unwrap() {
            Expr::Arith {
                op: ArithOp::Add,
                rhs,
                ..
            } => assert!(matches!(
                *rhs,
                Expr::Arith {
                    op: ArithOp::Mul,
                    ..
                }
            )),
            e => panic!("bad tree: {e:?}"),
        }
    }

    #[test]
    fn collections_and_stmts() {
        assert!(matches!(parse_expr("[1, 2, 3]").unwrap(), Expr::List(_)));
        assert!(matches!(parse_expr("x[1:2]").unwrap(), Expr::Slice { .. }));
        assert!(matches!(parse_stmt("echo 1 + 1").unwrap(), Stmt::Echo(_)));
        assert!(matches!(parse_stmt("let x = 5").unwrap(), Stmt::Let { .. }));
    }
}
