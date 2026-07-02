//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! EXTENSION — NO `vendor/` COUNTERPART. Recursive-descent parser building the
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

use crate::viml_ast::{ArithOp, Expr, ForVars, LetTarget, Stmt, UnaryOp, UnletArg};
use crate::viml_lexer::{lex, CaseFlag, CmpOp, Tok, Token, VimlError};

/// The small set of names Phase 3 recognizes as builtin function calls. The
/// full `funcs.c` table is ported in Phase 5.
pub const PHASE3_BUILTINS: &[&str] = &[
    "len",
    "type",
    "string",
    "empty",
    "abs",
    "str2nr",
    "str2float",
    "float2nr",
];

/// Parse one statement line into a [`Stmt`].
///
/// Inline trailing comments (`echo 1  " note`) are not stripped in Phase 3: a
/// `"` is a comment only where the command grammar expects end-of-command, but
/// in expression position it opens a string. Full-line comments are skipped by
/// the source splitter before this is called.
pub fn parse_stmt(line: &str) -> Result<Stmt, VimlError> {
    // Strip leading command modifiers (`silent`, `silent!`, `verbose 9`,
    // `noautocmd`, `keepjumps`, …). They change how a command runs, not what it
    // is, and real vimrcs use them constantly (`silent! colorscheme x`). A bare
    // modifier with no command (`silent`) becomes a no-op.
    let line = strip_command_modifiers(line.trim());
    if line.is_empty() {
        return Ok(Stmt::Expr(Expr::Number(0)));
    }
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
        "set" | "se" | "setlocal" | "setl" | "setglobal" | "setg" => {
            Ok(Stmt::Set(rest.to_string()))
        }
        "source" | "so" => Ok(Stmt::Source(rest.trim().to_string())),
        "unlet" | "unl" => {
            // `:unlet[!] x y …` — the optional `!` suppresses the missing-var
            // error. Each argument is a bare name or a List/Dict element target
            // (`l[i]` / `d.key`), matching `do_unlet_var()` (vendor/eval/vars.c).
            let args = rest.trim_start_matches('!').trim();
            split_unlet_args(args)
                .into_iter()
                .map(parse_unlet_arg)
                .collect::<Result<Vec<_>, _>>()
                .map(Stmt::Unlet)
        }
        "let" => parse_let(rest),
        // `:const {name} = {expr}` assigns like `:let`. RUST-PORT NOTE: Vim also
        // locks the variable (reassigning is E741); that immutability is not yet
        // enforced here — `:const` parses and assigns as `:let`.
        "const" | "cons" => parse_let(rest),
        "call" => Ok(Stmt::Call(parse_expr(rest)?)),
        "eval" => Ok(Stmt::Expr(parse_expr(rest)?)),
        "break" => Ok(Stmt::Break),
        "continue" | "cont" => Ok(Stmt::Continue),
        "finish" | "finis" | "fini" => Ok(Stmt::Finish),
        "return" => Ok(if rest.trim().is_empty() {
            Stmt::Return(None)
        } else {
            Stmt::Return(Some(parse_expr(rest)?))
        }),
        "throw" => Ok(Stmt::Throw(parse_expr(rest)?)),
        // `:command[!] …` defines a user command; `:delcommand` removes one.
        // (`command(`/`delcommand(` are not builtins, but guard anyway.)
        "command" | "comm" | "com" if !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::CommandDef(line[cmd.len()..].trim_start().to_string()))
        }
        "delcommand" | "delc" if !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::CommandDel(rest.to_string()))
        }
        // `:autocmd`/`:augroup`/`:doautocmd` (with abbreviations).
        "autocmd" | "autocm" | "autoc" | "auto" | "au" if !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::Autocmd(line[cmd.len()..].trim_start().to_string()))
        }
        "augroup" | "aug" if !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::Augroup(rest.to_string()))
        }
        "doautocmd" | "doau" | "doautoall" if !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::Doautocmd(rest.to_string()))
        }
        // `:map`-family commands (`nmap`/`inoremap`/`vunmap`/`mapclear`/`map!`).
        // The bare `map`/`unmap`/… forms collide with the `map()`/`filter()`
        // builtins, so a name immediately followed by `(` stays an expression.
        _ if is_map_command(cmd) && !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::Map(line.to_string()))
        }
        // `:colorscheme {name}` / `:colo` — the bare form (no name) is a query.
        "colorscheme" | "colo" | "colors" | "colorsc" | "colorsch" | "colorsche" | "colorschem"
            if !line[cmd.len()..].starts_with('(') =>
        {
            Ok(Stmt::Colorscheme(rest.trim().to_string()))
        }
        // `:highlight`/`:hi` — define or link a highlight group. `:hi` on its own
        // (or `:hi {group}` with no keys) is a listing query in real vim; we keep
        // the raw args and let the runtime decide.
        "highlight" | "hi" | "highligh" | "highlig" | "highli" | "highl" | "high" | "hig"
            if !line[cmd.len()..].starts_with('(') =>
        {
            Ok(Stmt::Highlight(line[cmd.len()..].trim_start().to_string()))
        }
        // `:syntax`/`:syn` and `:filetype`/`:filet` — recognized so real vimrc
        // files parse. Standalone they are no-ops (zemacs highlights and detects
        // filetypes itself); an embedding editor may hook them.
        "syntax" | "syn" | "synta" | "synt" if !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::Syntax(rest.trim().to_string()))
        }
        // `:filet` is the shortest form (`:file` is a different command).
        "filetype" | "filetyp" | "filety" | "filet" if !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::Filetype(rest.trim().to_string()))
        }
        // A `:`-prefixed line, or a `%`-prefixed line (`%s/…`), is an Ex command
        // with an optional line range. Neither can begin a valid expression
        // statement, so this is safe; unrecognized Ex commands fall back to
        // running as an ordinary statement at run time.
        _ if line.starts_with(':')
            || (line.starts_with('%')
                && line[1..].starts_with(|c: char| c.is_ascii_alphabetic())) =>
        {
            Ok(Stmt::ExCmd(line.to_string()))
        }
        // A command word starting with an uppercase letter is a user-command
        // invocation (`:Foo args`), resolved at run time. A name immediately
        // followed by `(` is a funcref call expression, not a command.
        _ if cmd.starts_with(|c: char| c.is_ascii_uppercase())
            && !line[cmd.len()..].starts_with('(') =>
        {
            Ok(Stmt::UserCmd(line.to_string()))
        }
        _ => Ok(Stmt::Expr(parse_expr(line)?)),
    }
}

/// The `:h :command-modifiers` (and their common abbreviations) that may prefix
/// any Ex command. They set execution context (silencing, verbosity, split
/// direction, …) and are stripped before the command is parsed.
const CMD_MODIFIERS: &[&str] = &[
    "silent",
    "sil",
    "unsilent",
    "uns",
    "verbose",
    "verb",
    "noautocmd",
    "noa",
    "keepmarks",
    "keepm",
    "keepjumps",
    "keepj",
    "keepalt",
    "keepa",
    "keeppatterns",
    "keepp",
    "lockmarks",
    "lockm",
    "noswapfile",
    "nos",
    "sandbox",
    "sandb",
    "browse",
    "bro",
    "confirm",
    "conf",
    "hide",
    "hid",
    "aboveleft",
    "abo",
    "belowright",
    "bel",
    "botright",
    "bo",
    "topleft",
    "to",
    "leftabove",
    "lefta",
    "rightbelow",
    "rightb",
    "vertical",
    "vert",
    "horizontal",
    "hor",
    "tab",
];

/// Strip a leading run of command modifiers from an Ex command line, returning
/// the remaining command text. Each modifier is a standalone word (optionally
/// with a `!`, e.g. `silent!`); `verbose`/`tab` may carry a numeric count
/// (`verbose 15 …`). A line that is only modifiers strips to empty.
fn strip_command_modifiers(mut line: &str) -> &str {
    loop {
        line = line.trim_start();
        let end = line
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(line.len());
        if end == 0 {
            break;
        }
        let word = &line[..end];
        if !CMD_MODIFIERS.contains(&word) {
            break;
        }
        let after = &line[end..];
        // The modifier must be a standalone token: the next char is `!`, a space,
        // or end-of-line. (`silentfoo` is an identifier, not the modifier.)
        let mut rest = match after.chars().next() {
            None => "",
            Some('!') => &after[1..],
            Some(c) if c.is_whitespace() => after,
            _ => break,
        };
        // `verbose`/`tab` take an optional numeric count.
        if matches!(word, "verbose" | "verb" | "tab") {
            let r = rest.trim_start();
            let ne = r.find(|c: char| !c.is_ascii_digit()).unwrap_or(r.len());
            if ne > 0 {
                rest = &r[ne..];
            }
        }
        line = rest;
    }
    line
}

/// Whether `cmd` is the (alphabetic) word of a `:map`-family command — any of
/// `[nivxsoctl]?(map|noremap|unmap|mapclear)`. The optional trailing `!` of
/// `map!`/`noremap!`/`unmap!` is not part of `cmd` (it is non-alphabetic), so a
/// bare `map`/`noremap`/`unmap`/`mapclear` also matches here.
fn is_map_command(cmd: &str) -> bool {
    let prefix = cmd
        .strip_suffix("mapclear")
        .or_else(|| cmd.strip_suffix("noremap"))
        .or_else(|| cmd.strip_suffix("unmap"))
        .or_else(|| cmd.strip_suffix("map"));
    matches!(
        prefix,
        Some("" | "n" | "i" | "v" | "x" | "s" | "o" | "c" | "t" | "l")
    )
}

/// Split a statement line into its leading command word (ASCII letters) and the
/// remaining text. A line starting with non-alphabetic text is a bare
/// expression (empty command word).
fn cmd_word(line: &str) -> (&str, &str) {
    let line = line.trim();
    let end = line
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(line.len());
    (&line[..end], line[end..].trim_start())
}

/// Whether `cmd` closes or continues a block (so it must be handled by the
/// block parser, never as a leaf statement).
fn is_block_terminator(cmd: &str) -> bool {
    matches!(
        cmd,
        "endif"
            | "elseif"
            | "else"
            | "endwhile"
            | "endwh"
            | "endfor"
            | "endfunction"
            | "endfunc"
            | "catch"
            | "finally"
            | "endtry"
    )
}

/// Parse a whole source block into a flat statement list with block structure
/// (the `:if`/`:while`/`:for`/`:function`/`:try` bodies nested inside).
pub fn parse_program(src: &str) -> Result<Vec<Stmt>, VimlError> {
    Ok(parse_program_lines(src)?
        .into_iter()
        .map(|(_, s)| s)
        .collect())
}

/// Like [`parse_program`] but pairs each TOP-LEVEL statement with its 1-based
/// source line (for the debugger's statement markers).
pub fn parse_program_lines(src: &str) -> Result<Vec<(u32, Stmt)>, VimlError> {
    let mut cur = Lines::new(src);
    let mut out = Vec::new();
    loop {
        cur.skip_blanks();
        let Some(line) = cur.peek() else { break };
        let (cmd, _) = cmd_word(&line);
        if is_block_terminator(cmd) {
            return Err(VimlError::msg(format!(
                "E580: `:{cmd}` without matching block opener"
            )));
        }
        let lineno = cur.line_no();
        for s in parse_one(&mut cur)? {
            out.push((lineno, s));
        }
    }
    Ok(out)
}

/// The result of [`parse_program_lines_tolerant`]: the top-level statements that
/// parsed (each with its 1-based source line), and `(line, message)` for each
/// statement that was skipped because it failed to parse.
pub type TolerantParse = (Vec<(u32, Stmt)>, Vec<(u32, String)>);

/// Like [`parse_program_lines`] but error-tolerant: a top-level statement (or
/// block) that fails to parse is skipped — its first logical line is dropped and
/// parsing resumes at the next — so one unsupported construct does not abort the
/// whole file. Returns the statements that parsed, paired with `(line, message)`
/// for each skipped one. Mirrors Vim reporting an error while sourcing a script
/// and continuing; used for best-effort config sourcing (e.g. a real `.vimrc`).
pub fn parse_program_lines_tolerant(src: &str) -> TolerantParse {
    let mut cur = Lines::new(src);
    let mut out = Vec::new();
    let mut errs = Vec::new();
    loop {
        cur.skip_blanks();
        let Some(line) = cur.peek() else { break };
        let lineno = cur.line_no();
        let snapshot = cur.i;
        let (cmd, _) = cmd_word(&line);
        if is_block_terminator(cmd) {
            // An orphaned terminator (e.g. left after skipping a broken opener):
            // report and step past it.
            errs.push((
                lineno,
                format!("E580: `:{cmd}` without matching block opener"),
            ));
            cur.i = snapshot + 1;
            continue;
        }
        match parse_one(&mut cur) {
            Ok(stmts) => {
                for s in stmts {
                    out.push((lineno, s));
                }
            }
            Err(e) => {
                errs.push((lineno, e.0));
                // Resume at the line after the one that began the failed parse.
                cur.i = snapshot + 1;
            }
        }
    }
    (out, errs)
}

/// Cursor over the LOGICAL lines of a source block. Physical lines whose first
/// non-blank char is `\` are joined onto the previous logical line (Vim's
/// line-continuation), so each entry carries its joined text plus the 1-based
/// source line where it began. `i` is the 0-based index of the next line.
struct Lines {
    lines: Vec<(u32, String)>,
    i: usize,
}

impl Lines {
    /// Build the logical-line list: first fold `\` continuation lines into the
    /// previous one (text after the `\` appended verbatim, as Vim does), then
    /// expand a one-line block (`if c | … | endif`) — a block-opener line with
    /// top-level `|` bars — into separate logical lines so the block parser
    /// handles it normally. Both keep the original 1-based source line number.
    fn new(src: &str) -> Self {
        // Pass 0: collapse heredoc assignments (`let x =<< [trim] [eval] END`)
        // into a single synthesized `let x = [...]` list-literal line. Body lines
        // are taken VERBATIM from the raw source — before continuation-folding or
        // bar-splitting — exactly as Vim's `ea_getline` feeds `heredoc_get()`
        // (vendor/eval/vars.c). Each body line becomes a single-quoted list item.
        let raw: Vec<&str> = src.lines().collect();
        let mut collapsed: Vec<(u32, String)> = Vec::new();
        let mut k = 0;
        while k < raw.len() {
            let lineno = (k + 1) as u32;
            if let Some((prefix, trim, _eval, marker)) = heredoc_opener(raw[k]) {
                // With `trim`, the end marker may be indented to match the `:let`
                // command line; record that indent so it can be skipped.
                let cmd_indent: String = raw[k].chars().take_while(|c| c.is_whitespace()).collect();
                let mut body: Vec<String> = Vec::new();
                let mut j = k + 1;
                while j < raw.len() {
                    let bl = raw[j];
                    let probe = if trim {
                        bl.strip_prefix(cmd_indent.as_str()).unwrap_or(bl)
                    } else {
                        bl
                    };
                    j += 1;
                    if probe == marker {
                        break;
                    }
                    body.push(bl.to_string());
                }
                // With `trim`, strip from every line the indent of the first
                // (non-blank) body line, matching char-for-char.
                if trim {
                    if let Some(first) = body.iter().find(|l| !l.trim().is_empty()) {
                        let ti: String = first.chars().take_while(|c| c.is_whitespace()).collect();
                        for l in body.iter_mut() {
                            let n: usize = l
                                .chars()
                                .zip(ti.chars())
                                .take_while(|(a, b)| a == b)
                                .map(|(a, _)| a.len_utf8())
                                .sum();
                            *l = l[n..].to_string();
                        }
                    }
                }
                let items: Vec<String> = body
                    .iter()
                    .map(|l| format!("'{}'", l.replace('\'', "''")))
                    .collect();
                collapsed.push((lineno, format!("{prefix}= [{}]", items.join(", "))));
                k = j;
                continue;
            }
            collapsed.push((lineno, raw[k].to_string()));
            k += 1;
        }
        // Pass 1: continuation join.
        let mut joined: Vec<(u32, String)> = Vec::new();
        for (lineno, raw) in collapsed {
            if let Some(rest) = raw.trim_start().strip_prefix('\\') {
                if let Some(last) = joined.last_mut() {
                    last.1.push_str(rest);
                    continue;
                }
            }
            joined.push((lineno, raw.to_string()));
        }
        // Pass 2: split `|`-separated commands into one logical line each, so a
        // block opener anywhere on the line (`let x=1 | if x | … | endif`) is
        // parsed as its own line. Blank/comment lines are kept whole.
        let mut lines: Vec<(u32, String)> = Vec::new();
        for (lineno, text) in joined {
            let trimmed = text.trim();
            if trimmed.is_empty() || trimmed.starts_with('"') {
                lines.push((lineno, text));
                continue;
            }
            let segs = split_commands(&text);
            if segs.len() > 1 {
                for seg in segs {
                    if !seg.trim().is_empty() {
                        lines.push((lineno, seg.to_string()));
                    }
                }
            } else {
                lines.push((lineno, text));
            }
        }
        Lines { lines, i: 0 }
    }

    /// Advance past blank lines and full-line `"` comments.
    fn skip_blanks(&mut self) {
        while let Some((_, l)) = self.lines.get(self.i) {
            let t = l.trim();
            if t.is_empty() || t.starts_with('"') {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<String> {
        self.lines.get(self.i).map(|(_, s)| s.clone())
    }

    fn bump(&mut self) {
        self.i += 1;
    }

    fn line_no(&self) -> u32 {
        self.lines.get(self.i).map(|(n, _)| *n).unwrap_or(0)
    }
}

/// Parse the statement(s) on the line at the cursor. A block opener yields one
/// `Stmt`; a leaf line yields one statement per `|`-separated command (Vim's
/// `do_one_cmd` bar split), so `let l = [1] | echo l` is two statements.
fn parse_one(cur: &mut Lines) -> Result<Vec<Stmt>, VimlError> {
    let line = cur.peek().expect("parse_one called at EOF");
    let (cmd, rest) = cmd_word(&line);
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
            for seg in split_commands(&line) {
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
                // Otherwise a `"` opens a string literal.
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

/// A parsed block body plus the terminator `(cmd, rest)` it stopped on
/// (`None` at EOF).
type ParseBlockResult = Result<(Vec<Stmt>, Option<(String, String)>), VimlError>;

/// Parse statements until a terminator in `terms`. Returns the body and the
/// terminator `(cmd, rest)` it stopped on (`None` at EOF).
fn parse_block(cur: &mut Lines, terms: &[&str]) -> ParseBlockResult {
    let mut stmts = Vec::new();
    loop {
        cur.skip_blanks();
        let Some(line) = cur.peek() else {
            return Ok((stmts, None));
        };
        let (cmd, rest) = cmd_word(&line);
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
    // Find the `)` that matches the parameter-list `(` — not merely the first
    // one, since a default value may itself contain parens (`a = abs(-7)`) or
    // brackets. Track nesting and skip quoted strings.
    let rparen = {
        let mut depth = 0i32;
        let mut quote: Option<u8> = None;
        let mut found = None;
        for (i, &b) in header.as_bytes().iter().enumerate().skip(lparen) {
            match quote {
                Some(q) => {
                    if b == q {
                        quote = None;
                    }
                }
                None => match b {
                    b'\'' | b'"' => quote = Some(b),
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            found = Some(i);
                            break;
                        }
                    }
                    _ => {}
                },
            }
        }
        found.ok_or_else(|| VimlError::msg("E125: Missing ')' in :function"))?
    };
    // Split the parameter list, separating optional `name = default` params
    // (`:help optional-function-argument`) into the name list plus a parallel
    // list of `(index, default expr)`.
    let mut args: Vec<String> = Vec::new();
    let mut defaults: Vec<(usize, Expr)> = Vec::new();
    for raw in split_top_commas(&header[lparen + 1..rparen]) {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        // `=` introduces a default, but `==`/`=~` etc. inside the default expr
        // must not be mistaken for it: only an unescaped top-level `=` that is
        // not part of a comparison operator separates name from default.
        match raw.find('=').filter(|&p| {
            raw[..p]
                .trim_end()
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == ':')
                && !matches!(raw.as_bytes().get(p + 1), Some(b'=') | Some(b'~'))
        }) {
            Some(p) => {
                defaults.push((args.len(), parse_expr(raw[p + 1..].trim())?));
                args.push(raw[..p].trim().to_string());
            }
            None => args.push(raw.to_string()),
        }
    }
    let (body, term) = parse_block(cur, &["endfunction", "endfunc"])?;
    if term.is_none() {
        return Err(VimlError::msg("E126: Missing :endfunction"));
    }
    Ok(Stmt::Function {
        name,
        args,
        defaults,
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
    let op = match rest.as_bytes()[..eq].last() {
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
    } else if lhs.ends_with(']') && lhs.contains('[') {
        // `base[index] = …` — split at the LAST top-level subscript (so nested
        // `d['a']['b']` parses `d['a']` as the base expression).
        let bytes = lhs.as_bytes();
        let mut depth = 0i32;
        let mut open = 0;
        for i in (0..lhs.len()).rev() {
            match bytes[i] {
                b']' => depth += 1,
                b'[' => {
                    depth -= 1;
                    if depth == 0 {
                        open = i;
                        break;
                    }
                }
                _ => {}
            }
        }
        let base_src = lhs[..open].trim();
        let index_src = &lhs[open + 1..lhs.len() - 1];
        // A top-level `:` inside the subscript marks a range assign `l[i:j]=…`
        // (split at the colon that is not nested in `[]`/`()` or a string).
        match split_top_colon(index_src) {
            Some((a, b)) => {
                let parse_opt = |s: &str| -> Result<Option<Box<Expr>>, VimlError> {
                    let s = s.trim();
                    Ok(if s.is_empty() {
                        None
                    } else {
                        Some(Box::new(parse_expr(s)?))
                    })
                };
                LetTarget::Range {
                    base: Box::new(parse_expr(base_src)?),
                    idx1: parse_opt(a)?,
                    idx2: parse_opt(b)?,
                }
            }
            None => LetTarget::Index {
                base: Box::new(parse_expr(base_src)?),
                index: Box::new(parse_expr(index_src)?),
            },
        }
    } else if !lhs.contains('[')
        && lhs.contains('.')
        && lhs.rsplit_once('.').is_some_and(|(_, k)| {
            !k.is_empty() && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
    {
        // `base.key = …` — split at the last `.` (nested `d.a.b` → base `d.a`).
        let (base, key) = lhs.rsplit_once('.').unwrap();
        LetTarget::Index {
            base: Box::new(parse_expr(base)?),
            index: Box::new(Expr::Str(key.to_string())),
        }
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

/// Split a `:unlet` argument list on top-level whitespace, keeping `[…]`
/// subscripts and quoted strings (e.g. `d['a b']`) intact. A plain
/// `split_whitespace()` would wrongly break `unlet d['a b']` in two.
fn split_unlet_args(s: &str) -> Vec<&str> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut start: Option<usize> = None;
    for (i, &c) in bytes.iter().enumerate() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                b'\'' | b'"' => quote = Some(c),
                b'[' | b'(' => depth += 1,
                b']' | b')' => depth -= 1,
                _ if c.is_ascii_whitespace() && depth == 0 => {
                    if let Some(st) = start.take() {
                        out.push(&s[st..i]);
                    }
                    continue;
                }
                _ => {}
            },
        }
        if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(st) = start {
        out.push(&s[st..]);
    }
    out
}

/// Split a function parameter list on its top-level commas, keeping commas
/// inside a default value's `[]`/`()`/`{}` or quoted string intact (so
/// `func F(l = [1, 2], d = {'a': 1})` splits into two params, not five).
fn split_top_commas(s: &str) -> Vec<&str> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut start = 0usize;
    for (i, &c) in bytes.iter().enumerate() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                b'\'' | b'"' => quote = Some(c),
                b'[' | b'(' | b'{' => depth += 1,
                b']' | b')' | b'}' => depth -= 1,
                b',' if depth == 0 => {
                    out.push(&s[start..i]);
                    start = i + 1;
                }
                _ => {}
            },
        }
    }
    out.push(&s[start..]);
    out
}

/// If `line` is a heredoc assignment opener (`let X =<< [trim] [eval] MARKER`),
/// return `(prefix, trim, eval, marker)` where `prefix` is the `:let` target
/// text before `=<<` (the caller appends `= [...]`). Mirrors the keyword scan
/// at the top of `heredoc_get()` (vendor/eval/vars.c).
fn heredoc_opener(line: &str) -> Option<(String, bool, bool, String)> {
    let (cmd, _) = cmd_word(line.trim_start());
    if !matches!(cmd, "let" | "const" | "cons") {
        return None;
    }
    let op = line.find("=<<")?;
    let prefix = line[..op].to_string();
    let mut rest = line[op + 3..].trim_start();
    let (mut trim, mut eval) = (false, false);
    loop {
        let kw = |r: &str, w: &str| -> bool {
            r.strip_prefix(w)
                .is_some_and(|t| t.is_empty() || t.starts_with(char::is_whitespace))
        };
        if kw(rest, "trim") {
            trim = true;
            rest = rest[4..].trim_start();
        } else if kw(rest, "eval") {
            eval = true;
            rest = rest[4..].trim_start();
        } else {
            break;
        }
    }
    let marker = rest.split_whitespace().next()?;
    if marker.is_empty() {
        return None;
    }
    Some((prefix, trim, eval, marker.to_string()))
}

/// Parse one `:unlet` argument into a bare name or a List/Dict element target.
/// Reuses the same `l[i]` / `d.key` shapes as `:let`'s `LetTarget::Index`; the
/// removal itself happens at runtime (mirroring `do_unlet_var()`).
fn parse_unlet_arg(arg: &str) -> Result<UnletArg, VimlError> {
    let arg = arg.trim();
    // `base[index]` — find the matching `[` for the trailing `]`.
    if arg.ends_with(']') && arg.contains('[') {
        let bytes = arg.as_bytes();
        let mut depth = 0i32;
        let mut open = 0;
        for i in (0..arg.len()).rev() {
            match bytes[i] {
                b']' => depth += 1,
                b'[' => {
                    depth -= 1;
                    if depth == 0 {
                        open = i;
                        break;
                    }
                }
                _ => {}
            }
        }
        let base_src = arg[..open].trim();
        let index_src = &arg[open + 1..arg.len() - 1];
        // A range subscript (`unlet l[i:j]`) is not yet supported; fall through
        // to treat the whole thing as a name so the runtime reports E108/E116.
        if split_top_colon(index_src).is_none() {
            return Ok(UnletArg::Item {
                base: Box::new(parse_expr(base_src)?),
                index: Box::new(parse_expr(index_src)?),
            });
        }
    } else if !arg.contains('[')
        && arg.contains('.')
        && arg.rsplit_once('.').is_some_and(|(_, k)| {
            !k.is_empty() && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
    {
        // `base.key` — split at the last `.` (nested `d.a.b` → base `d.a`).
        let (base, key) = arg.rsplit_once('.').unwrap();
        return Ok(UnletArg::Item {
            base: Box::new(parse_expr(base)?),
            index: Box::new(Expr::Str(key.to_string())),
        });
    }
    Ok(UnletArg::Name(arg.to_string()))
}

/// Split a subscript on its top-level `:` (the list-range separator). Returns
/// `(before, after)`, or `None` when there is no unnested `:` (a plain index).
/// `:` inside `[]`/`()` or a quoted string is ignored.
fn split_top_colon(s: &str) -> Option<(&str, &str)> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                b'\'' | b'"' => quote = Some(c),
                b'[' | b'(' => depth += 1,
                b']' | b')' => depth -= 1,
                b':' if depth == 0 => {
                    // Skip a scope-prefix colon (`s:`/`g:`/`a:`/…): a single scope
                    // letter at a token boundary, followed by an identifier char —
                    // that's a scoped variable index, not a range separator.
                    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
                    let scope_prefix = i >= 1
                        && matches!(
                            bytes[i - 1],
                            b's' | b'g' | b'b' | b'w' | b't' | b'l' | b'a' | b'v'
                        )
                        && (i == 1 || !is_ident(bytes[i - 2]))
                        && i + 1 < bytes.len()
                        && is_ident(bytes[i + 1]);
                    if !scope_prefix {
                        return Some((&s[..i], &s[i + 1..]));
                    }
                }
                _ => {}
            },
        }
        i += 1;
    }
    None
}

/// The current value of a compound-assignment target, as an expression. List
/// unpack targets cannot take a compound operator (`E734`).
fn let_target_expr(target: &LetTarget) -> Result<Expr, VimlError> {
    Ok(match target {
        LetTarget::Var(n) => Expr::Var(n.clone()),
        LetTarget::Option(n) => Expr::Option(n.clone()),
        LetTarget::Env(n) => Expr::Env(n.clone()),
        LetTarget::Register(c) => Expr::Register(*c),
        LetTarget::Index { base, index } => Expr::Index {
            base: base.clone(),
            index: index.clone(),
        },
        LetTarget::List { .. } | LetTarget::Range { .. } => {
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
            // Blob literal `0z…` desugars to `list2blob([byte, …])`, reusing the
            // ported list2blob builtin to build the Blob value.
            Tok::Blob(bytes) => Ok(Expr::Call {
                name: "list2blob".to_string(),
                args: vec![Expr::List(
                    bytes.into_iter().map(|b| Expr::Number(b as i64)).collect(),
                )],
            }),
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
            Tok::LBrace => {
                if self.at_lambda() {
                    self.lambda()
                } else {
                    self.dict_literal()
                }
            }
            Tok::HashBrace => self.literal_dict(),
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

    /// True when the `(` at the current position directly abuts the previous
    /// token (no whitespace), i.e. it is a call applied to the preceding value
    /// rather than a separate parenthesised argument.
    fn lparen_abuts_prev(&self) -> bool {
        let i = self.i;
        i > 0
            && self.toks.get(i).is_some_and(|t| t.kind == Tok::LParen)
            && self.toks[i].span == self.toks[i - 1].end
    }

    /// True when the `Tok::Dot` at the current position is a dict member access
    /// `d.key` rather than the `..`-style concat operator: it must directly abut
    /// the base (no space before) and be immediately followed by a bare name (no
    /// space after). `a . b` (spaced) stays concatenation.
    fn at_member_dot(&self) -> bool {
        let i = self.i;
        if i == 0 || i + 1 >= self.toks.len() {
            return false;
        }
        let dot = &self.toks[i];
        if dot.kind != Tok::Dot {
            return false;
        }
        let prev = &self.toks[i - 1];
        let next = &self.toks[i + 1];
        // `.name(` is concatenation with a function call (`a().b(x)` / `s.f(x)`),
        // not a member call — legacy Vimscript has no direct `dict.key(args)`
        // call syntax (that is vim9). So only treat `.name` as a member read when
        // the name is NOT immediately followed by '('.
        let followed_by_call =
            matches!(self.toks.get(i + 2), Some(t) if t.kind == Tok::LParen && t.span == next.end);
        matches!(next.kind, Tok::Ident(_))
            && dot.span == prev.end // no space before the dot
            && next.span == dot.end // no space after the dot
            && !followed_by_call
    }

    /// Postfix subscripts: `[index]`, `[from:to]`, `.name` dict member access,
    /// and `->method()`. A no-space `d.key` is a member read (a string subscript);
    /// a spaced `a . b` is left to `eval6` as concatenation.
    fn postfix(&mut self, mut base: Expr) -> Result<Expr, VimlError> {
        loop {
            // `d.key` member read — but not on a numeric literal (`1.foo` is concat).
            if self.at_member_dot() && !matches!(base, Expr::Number(_) | Expr::Float(_)) {
                self.advance(); // consume the dot
                if let Tok::Ident(key) = self.advance() {
                    base = Expr::Index {
                        base: Box::new(base),
                        index: Box::new(Expr::Str(key)),
                    };
                    continue;
                }
            }
            // `expr(args)` — call a funcref-valued expression directly. Only when
            // `(` abuts the base (no space): a bare `name(...)` is already a Call
            // from `primary`, so an abutting `(` here always follows another
            // postfix result (`function('x')(a)`, `funcs[0](a)`); a spaced
            // `echo F (1)` stays two arguments.
            if matches!(self.peek(), Tok::LParen) && self.lparen_abuts_prev() {
                self.advance(); // consume '('
                let args = self.arg_list(&Tok::RParen)?;
                base = Expr::CallExpr {
                    callee: Box::new(base),
                    args,
                };
                continue;
            }
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

    /// Lookahead (just past the opening `{`) deciding lambda vs dict: a lambda
    /// is `{ -> …}` or `{ ident (, ident)* -> …}` — a top-level `->` reached
    /// through only bare names and commas. Anything else (a `:` key, a string
    /// key, `}`) is a dict.
    fn at_lambda(&self) -> bool {
        let mut j = self.i;
        if matches!(self.toks.get(j).map(|t| &t.kind), Some(Tok::Arrow)) {
            return true; // {-> body}
        }
        loop {
            if !matches!(self.toks.get(j).map(|t| &t.kind), Some(Tok::Ident(_))) {
                return false;
            }
            j += 1;
            match self.toks.get(j).map(|t| &t.kind) {
                Some(Tok::Arrow) => return true,
                Some(Tok::Comma) => j += 1,
                _ => return false,
            }
        }
    }

    /// Parse a lambda `{params -> body}` (the opening `{` already consumed).
    fn lambda(&mut self) -> Result<Expr, VimlError> {
        let mut params = Vec::new();
        if !matches!(self.peek(), Tok::Arrow) {
            loop {
                match self.advance() {
                    Tok::Ident(n) => params.push(n),
                    other => {
                        return Err(VimlError::msg(format!(
                            "E15: expected lambda parameter, found {other:?}"
                        )))
                    }
                }
                match self.peek() {
                    Tok::Comma => {
                        self.advance();
                    }
                    Tok::Arrow => break,
                    other => {
                        return Err(VimlError::msg(format!(
                            "E15: expected ',' or '->' in lambda, found {other:?}"
                        )))
                    }
                }
            }
        }
        self.eat(&Tok::Arrow)?;
        let body = self.eval1()?;
        self.eat(&Tok::RBrace)?;
        Ok(Expr::Lambda {
            params,
            body: Box::new(body),
        })
    }

    /// Parse a literal-key Dict `#{key: val, …}` (the opening `#{` consumed):
    /// each key is a bare word (or number) used as a String. Because a single
    /// scope-like char absorbs its `:` in the lexer (`a:`), a key Ident ending
    /// in `:` already includes the separator; otherwise a `:` token follows.
    fn literal_dict(&mut self) -> Result<Expr, VimlError> {
        let mut pairs = Vec::new();
        if matches!(self.peek(), Tok::RBrace) {
            self.advance();
            return Ok(Expr::Dict(pairs));
        }
        loop {
            let raw = match self.advance() {
                Tok::Ident(s) => s,
                Tok::Number(n) => n.to_string(),
                Tok::Str(s) => s,
                other => {
                    return Err(VimlError::msg(format!(
                        "E15: expected literal Dict key, found {other:?}"
                    )))
                }
            };
            let (key, val) = if let Some(stripped) = raw.strip_suffix(':') {
                // `a:` — a scope-letter key absorbed its `:`; the value follows.
                (stripped.to_string(), self.eval1()?)
            } else if let Some(c) = raw.find(':') {
                // `a:1` — a scope-letter key merged with a glued simple value
                // (the lexer can only glue a bareword/number after the `:`); split
                // and parse that fragment as the value.
                (raw[..c].to_string(), parse_expr(&raw[c + 1..])?)
            } else {
                self.eat(&Tok::Colon)?;
                (raw, self.eval1()?)
            };
            pairs.push((Expr::Str(key), val));
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
                        "E15: expected ',' or '}}' in #{{}}, found {other:?}"
                    )))
                }
            }
        }
        Ok(Expr::Dict(pairs))
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
    fn command_modifiers_are_stripped() {
        // `silent!` + a real command → the command survives.
        assert!(matches!(
            parse_stmt("silent! colorscheme molokai").unwrap(),
            Stmt::Colorscheme(n) if n == "molokai"
        ));
        // Stacked modifiers, a `verbose` count, and abbreviations.
        assert!(matches!(
            parse_stmt("silent noautocmd verbose 9 set number").unwrap(),
            Stmt::Set(a) if a == "number"
        ));
        // A bare modifier is a no-op, not an error.
        assert!(matches!(parse_stmt("silent").unwrap(), Stmt::Expr(_)));
        // A longer identifier that merely starts with a modifier word is NOT a
        // modifier (`silentfoo` is a user command, not `silent` + `foo`).
        assert!(matches!(
            parse_stmt("Silentcmd arg").unwrap(),
            Stmt::UserCmd(_)
        ));
    }

    #[test]
    fn tolerant_parse_skips_bad_statements() {
        // Line 2 is an unterminated string (a parse error). Tolerant parsing must
        // still yield the good statements on lines 1 and 3.
        let src = "set number\nlet x = \"oops\ncolorscheme molokai\n";
        let (stmts, errs) = parse_program_lines_tolerant(src);
        assert_eq!(errs.len(), 1, "one statement skipped");
        assert!(stmts
            .iter()
            .any(|(_, s)| matches!(s, Stmt::Set(a) if a == "number")));
        assert!(stmts
            .iter()
            .any(|(_, s)| matches!(s, Stmt::Colorscheme(n) if n == "molokai")));
    }

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
    fn dict_member_dot_vs_concat() {
        // `d.key` (no spaces) → a string subscript (member read).
        match parse_expr("d.key").unwrap() {
            Expr::Index { index, .. } => {
                assert!(matches!(*index, Expr::Str(ref s) if s == "key"))
            }
            e => panic!("expected member Index, got {e:?}"),
        }
        // Nested `d.a.b` → chained subscripts.
        assert!(matches!(parse_expr("d.a.b").unwrap(), Expr::Index { .. }));
        // `a . b` (spaced) stays concatenation.
        assert!(matches!(
            parse_expr("a . b").unwrap(),
            Expr::Arith {
                op: ArithOp::Concat,
                ..
            }
        ));
        // `'x' .. 'y'` stays concatenation.
        assert!(matches!(
            parse_expr("'x' .. 'y'").unwrap(),
            Expr::Arith {
                op: ArithOp::Concat,
                ..
            }
        ));
    }

    #[test]
    fn literal_key_dict() {
        // `#{a: 1}` is a Dict with the bare word as a String key.
        match parse_expr("#{a: 1, name: 'x'}").unwrap() {
            Expr::Dict(pairs) => {
                assert_eq!(pairs.len(), 2);
                assert!(matches!(&pairs[0].0, Expr::Str(s) if s == "a"));
                assert!(matches!(&pairs[1].0, Expr::Str(s) if s == "name"));
            }
            e => panic!("expected Dict, got {e:?}"),
        }
        assert!(matches!(parse_expr("#{}").unwrap(), Expr::Dict(_)));
    }

    #[test]
    fn one_line_blocks() {
        // `if … | … | endif` on one line parses as a full If block.
        assert!(matches!(
            parse_program("if 1 | echo 'y' | endif").unwrap().as_slice(),
            [Stmt::If { .. }]
        ));
        // A leaf command then a one-line block: two statements.
        match parse_program("let x = 5 | if x > 3 | echo 'big' | endif")
            .unwrap()
            .as_slice()
        {
            [Stmt::Let { .. }, Stmt::If { .. }] => {}
            s => panic!("expected [Let, If], got {s:?}"),
        }
        // A `for` one-liner.
        assert!(matches!(
            parse_program("for i in [1] | echo i | endfor")
                .unwrap()
                .as_slice(),
            [Stmt::For { .. }]
        ));
        // A plain bar line (no block) is still two leaf statements.
        assert_eq!(parse_program("let a = 1 | echo a").unwrap().len(), 2);
    }

    #[test]
    fn line_continuation() {
        // A `\` continuation line joins onto the previous logical line.
        let prog = parse_program("let x = [1,\n      \\ 2,\n      \\ 3]").unwrap();
        match &prog[0] {
            Stmt::Let {
                expr: Expr::List(items),
                ..
            } => assert_eq!(items.len(), 3),
            s => panic!("expected 3-item list, got {s:?}"),
        }
        // Line numbers: the statement after a 2-physical-line logical line keeps
        // its real physical number.
        let lines = parse_program_lines("let a = 1\nlet b = [10,\n  \\ 20]\nlet c = 3").unwrap();
        assert_eq!(lines[0].0, 1);
        assert_eq!(lines[1].0, 2); // the [10, 20] let starts on physical line 2
        assert_eq!(lines[2].0, 4); // `let c` is on physical line 4
    }

    #[test]
    fn lambda_vs_dict() {
        // `{x -> …}` and `{-> …}` are lambdas.
        assert!(matches!(
            parse_expr("{x -> x + 1}").unwrap(),
            Expr::Lambda { .. }
        ));
        assert!(matches!(
            parse_expr("{-> 42}").unwrap(),
            Expr::Lambda { .. }
        ));
        match parse_expr("{a, b -> a - b}").unwrap() {
            Expr::Lambda { params, .. } => assert_eq!(params, vec!["a", "b"]),
            e => panic!("expected lambda, got {e:?}"),
        }
        // `{...}` with string keys and `{}` are dicts (not lambdas).
        assert!(matches!(parse_expr("{'a': 1}").unwrap(), Expr::Dict(_)));
        assert!(matches!(
            parse_expr("{'k': v, 'j': w}").unwrap(),
            Expr::Dict(_)
        ));
        assert!(matches!(parse_expr("{}").unwrap(), Expr::Dict(_)));
    }

    #[test]
    fn collections_and_stmts() {
        assert!(matches!(parse_expr("[1, 2, 3]").unwrap(), Expr::List(_)));
        assert!(matches!(parse_expr("x[1:2]").unwrap(), Expr::Slice { .. }));
        assert!(matches!(parse_stmt("echo 1 + 1").unwrap(), Stmt::Echo(_)));
        assert!(matches!(parse_stmt("let x = 5").unwrap(), Stmt::Let { .. }));
    }
}
