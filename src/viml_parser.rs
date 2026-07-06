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
use crate::viml_lexer::{lex, CaseFlag, CmpOp, InterpPart, Tok, Token, VimlError};
use std::cell::Cell;

thread_local! {
    /// Whether the code currently being parsed is vim9 (`:vim9script` script or a
    /// `def … enddef` body). In vim9, dict literals `{key: val}` use BARE literal
    /// keys (`{a: 1}` → key `"a"`), not the legacy expression-keyed form where the
    /// key is evaluated. Set for the parse duration by [`Vim9Guard`].
    static VIM9: Cell<bool> = const { Cell::new(false) };
}

/// True when the parser is in a vim9 region (see [`VIM9`]).
fn vim9_active() -> bool {
    VIM9.with(|f| f.get())
}

/// RAII guard that switches [`VIM9`] for the duration of a parse (script body,
/// `def` body, or legacy `function` body) and restores the previous value on
/// drop, so nested regions (a legacy `:function` inside a `:vim9script`, or a
/// `:def` inside a legacy script) each parse in their own mode.
struct Vim9Guard(bool);

impl Vim9Guard {
    fn enter(on: bool) -> Self {
        Vim9Guard(VIM9.with(|f| f.replace(on)))
    }
}

impl Drop for Vim9Guard {
    fn drop(&mut self) {
        VIM9.with(|f| f.set(self.0));
    }
}

/// True when a script is a `:vim9script` — its first non-blank logical line's
/// command word is `vim9script`. Mirrors the detection in [`Lines::new`].
fn script_is_vim9(src: &str) -> bool {
    src.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .is_some_and(|l| l.split(char::is_whitespace).next() == Some("vim9script"))
}

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
    // `:vim9script [noclear]` — switches the script to vim9 mode (a no-op leaf;
    // the mode's parse effects are applied in `Lines::new`). Matched here because
    // its command word ends at the `9`, which `cmd_word` treats as a boundary.
    if line
        .split(|c: char| c.is_whitespace())
        .next()
        .is_some_and(|w| w == "vim9script")
    {
        return Ok(Stmt::Expr(Expr::Number(0)));
    }
    let cmd_end = line
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(line.len());
    let cmd = &line[..cmd_end];
    let rest = line[cmd_end..].trim_start();

    match cmd {
        "echo" | "ec" => Ok(Stmt::Echo(parse_expr_list(rest)?)),
        "echon" => Ok(Stmt::Echon(parse_expr_list(rest)?)),
        // `:echomsg`/`:echoerr` (Vim abbreviations `echom`, `echoe`/`echoer`)
        // both evaluate and print their expression list; they are modelled as
        // `:echo` here (same simplification already used for `:echomsg`). Adding
        // `:echoerr` stops an unrecognized `echoerr` in a function body from
        // falling through to `parse_expr` and aborting the `:function`.
        "echomsg" | "echom" | "echoerr" | "echoer" | "echoe" => {
            Ok(Stmt::Echo(parse_expr_list(rest)?))
        }
        // `:execute` accepts every prefix down to `:exe` (verified against Vim
        // 9.2: `exe`/`exec`/`execu`/`execut`/`execute` all run). Missing the
        // intermediate forms made `exec '…'` fall through to `parse_expr`, which
        // aborts the enclosing function definition and leaks its body to global
        // scope (E461 on `l:` vars).
        "execute" | "execut" | "execu" | "exec" | "exe" => {
            Ok(Stmt::Execute(parse_expr_list(rest)?))
        }
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
        // vim9 `:var {name}[: type] = {expr}` declare-and-assign like `:let`. The
        // `: type` annotation is parsed and discarded (checking/coercion deferred).
        // A type-only `:var x: number` (no initializer) default-inits to the
        // type's zero value ([`vim9_var_decl`]). RUST-PORT NOTE: real Vim rejects
        // `:var` in a legacy script (E1124); this leaf accepts it everywhere.
        "var" => parse_let(&vim9_var_decl(rest)),
        // `:final {name}[: type] = {expr}` — like `:var` but a value is REQUIRED
        // (real Vim: E1125 on a type-only `:final`), so no default-init here.
        "final" => parse_let(&strip_vim9_type(rest)),
        // A `:function` that reaches here (not caught as a block opener in
        // `parse_one` — e.g. behind a modifier, `silent function`) with no
        // parameter-list `(` is the listing/show command, not a definition:
        // no-op editor-less. (`:function {name}(…)` behind a modifier is not a
        // leaf and is left to error, matching the pre-existing limitation.)
        "function" | "fu" | "fun" | "func" | "funct" | "functi" | "functio"
            if !rest.contains('(') =>
        {
            Ok(Stmt::Expr(Expr::Number(0)))
        }
        // `:const {name} = {expr}` assigns like `:let`. RUST-PORT NOTE: Vim also
        // locks the variable (reassigning is E741); that immutability is not yet
        // enforced here — `:const` parses and assigns as `:let`.
        "const" | "cons" => parse_let(&strip_vim9_type(rest)),
        "call" => Ok(Stmt::Call(parse_expr(strip_legacy_trailing_comment(rest))?)),
        "eval" => Ok(Stmt::Expr(parse_expr(strip_legacy_trailing_comment(rest))?)),
        "break" => Ok(Stmt::Break),
        "continue" | "cont" => Ok(Stmt::Continue),
        "finish" | "finis" | "fini" => Ok(Stmt::Finish),
        "return" => Ok(if rest.trim().is_empty() {
            Stmt::Return(None)
        } else {
            Stmt::Return(Some(parse_expr(strip_legacy_trailing_comment(rest))?))
        }),
        "throw" => Ok(Stmt::Throw(parse_expr(strip_legacy_trailing_comment(
            rest,
        ))?)),
        // `:command[!] …` defines a user command; `:delcommand` removes one.
        // (`command(`/`delcommand(` are not builtins, but guard anyway.)
        "command" | "comm" | "com" if !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::CommandDef(line[cmd.len()..].trim_start().to_string()))
        }
        "delcommand" | "delc" if !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::CommandDel(rest.to_string()))
        }
        // `:delf[unction][!] {name}` removes a user function. The raw remainder
        // (the run-time handler splits off a leading `!`) carries the name; a
        // `delfunction(` form is an expression call, so guard on `(`.
        "delfunction" | "delfunctio" | "delfuncti" | "delfunct" | "delfunc" | "delfun" | "delf"
            if !line[cmd.len()..].starts_with('(') =>
        {
            Ok(Stmt::DelFunction(
                line[cmd.len()..].trim_start().to_string(),
            ))
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
        // `:normal[!] {keys}` runs normal-mode keys against the buffer, dispatched
        // by `do_excmd` at run time. Recognized even without a leading `:` so a
        // function body line like `normal! el` parses and the function defines —
        // otherwise it fell through to `parse_expr`, aborting the whole `:function`
        // and leaking its body to global scope.
        "normal" | "norm" if !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::ExCmd(line.to_string()))
        }
        // `:echohl {group}` sets the highlight group for later `:echo` output.
        // Its argument is a group NAME, not an expression, so it can't go through
        // the `:echo` path (that would evaluate the name as a variable). Routed to
        // `do_excmd`'s `ex_echohl` handler; recognized even bare so a function body
        // line like `echohl ErrorMsg` parses instead of aborting the `:function`.
        "echohl" | "echoh" if !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::ExCmd(line.to_string()))
        }
        // Screen/session/mark commands (`:redraw[!]`, `:redir`, `:runtime`,
        // `:mark`, `:nohlsearch`) — dispatched (or no-op'd) by `do_excmd`.
        // Recognized bare so a function body line like `redraw!` or `redir => x`
        // parses instead of being mis-read as an expression and aborting the
        // `:function`. `:noh[lsearch]` (`:h :noh`) clears search highlight — a
        // no-op editor-less; recognized so a config/syntax line `nohlsearch`
        // parses instead of falling through to `parse_expr` and erroring E121.
        "redraw" | "redr" | "redra" | "redraws" | "redrawstatus" | "redrawt" | "redrawtabline"
        | "redir" | "redi" | "runtime" | "ru" | "run" | "runt" | "runti" | "runtim" | "mark"
        | "ma" | "mar" | "noh" | "nohl" | "nohls" | "nohlse" | "nohlsea" | "nohlsear"
        | "nohlsearc" | "nohlsearch"
            if !line[cmd.len()..].starts_with('(') =>
        {
            Ok(Stmt::ExCmd(line.to_string()))
        }
        // Fold-view commands (`ex_docmd.c` → `ex_fold`/`ex_foldopen`): `:fo[ld]`
        // creates a fold, `:foldo[pen][!]` opens folds, `:foldc[lose][!]` closes
        // them. Folds are window-view state that a standalone eval engine does not
        // have, so `do_excmd` no-ops them — but they MUST parse as Ex commands so a
        // config/syntax line like `syntax/cdl.vim`'s `%foldo!` (whole-file range +
        // recursive open) is handled instead of falling through to `parse_expr`
        // (which would raise E121 / E492 on the fold word).
        "fold" | "fo" | "fol" | "foldopen" | "foldo" | "foldop" | "foldope" | "foldclose"
        | "foldc" | "foldcl" | "foldclo" | "foldclos"
            if !line[cmd.len()..].starts_with('(') =>
        {
            Ok(Stmt::ExCmd(line.to_string()))
        }
        // `:edit`/`:ed` (load a file / reload) and buffer-list navigation
        // (`:bnext`, `:bprevious`, `:bfirst`, `:blast`, `:buffer`, …) — dispatched
        // by `do_excmd`. Recognized bare so a body line like `edit #` or `bnext`
        // parses instead of `parse_expr` choking on it and aborting the function.
        "edit" | "ed" | "bnext" | "bn" | "bne" | "bprevious" | "bp" | "bprev" | "bNext" | "bN"
        | "bfirst" | "bf" | "blast" | "bl" | "buffer" | "bu" | "buf" | "bmodified" | "bm"
        | "bmod" | "ball" | "ba"
            if !line[cmd.len()..].starts_with('(') =>
        {
            Ok(Stmt::ExCmd(line.to_string()))
        }
        // A `:`-prefixed line, or a `%`-prefixed line (`%s/…`), is an Ex command
        // with an optional line range. Neither can begin a valid expression
        // statement, so this is safe; unrecognized Ex commands fall back to
        // running as an ordinary statement at run time.
        // A leading `!` is the `:!{cmd}` shell command (`:h :!`); `do_excmd`
        // treats a range-less one as a handled no-op. Recognized here so a bang
        // command — especially with shell redirection (`!mkdir … > /dev/null
        // 2>&1`) — parses as a statement instead of being mis-read as the `!`
        // (logical-not) expression, which aborts the enclosing `:function`.
        // A line beginning with `'` is a mark-address Ex command (`:'<`, `:'>`,
        // `:'<,'>s/…`) — never a bare string, which isn't a valid statement. Route
        // it to `do_excmd` so it parses; otherwise `parse_expr` reads it as an
        // unterminated string literal and aborts the enclosing `:function`.
        // A line beginning with an ASCII digit is a line-range Ex command: Vim's
        // `do_one_cmd` reads a leading number as the range's first address
        // (`1print`, `1,1fold`, `5`), so it is an Ex command, never an expression
        // statement (a bare number line moves the cursor, `:h {address}`).
        // `do_excmd`/`parse_line_range` already parse the range and dispatch (or
        // no-op) the command; routing here lets such a line in a `:function` body
        // parse instead of falling through to `parse_expr`, which chokes on the
        // trailing command word and aborts the whole definition.
        _ if line.starts_with(':')
            || line.starts_with('!')
            || line.starts_with('\'')
            || line.starts_with(|c: char| c.is_ascii_digit())
            || (line.starts_with('%')
                && line[1..].starts_with(|c: char| c.is_ascii_alphabetic())) =>
        {
            Ok(Stmt::ExCmd(line.to_string()))
        }
        // vim9 bare assignment to an already-declared variable: `name = expr`,
        // `name += expr`, `d[key] = expr`, `g:x = expr` — vim9 assigns without a
        // `:let`/`:var` keyword. Legacy vimscript requires `:let`, so this fires
        // only in a vim9 region; [`is_vim9_assignment`] rejects user commands
        // (`MyCmd key=val`) and comparisons. Kept ahead of the uppercase
        // user-command arm so a CamelCase script var (`Total = 0`) assigns.
        _ if vim9_active() && is_vim9_assignment(line) => parse_let(line),
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
        canon_block_kw(cmd),
        "endif"
            | "elseif"
            | "else"
            | "endwhile"
            | "endfor"
            | "endfunction"
            | "enddef"
            | "catch"
            | "finally"
            | "endtry"
    )
}

/// Normalize a command word to its canonical block-control keyword, resolving
/// Vim's command abbreviations (`fu`→`function`, `endw`→`endwhile`, `en`→`endif`,
/// `cat`→`catch`, …). The abbreviation sets match Vim 9.2's `fullcommand()`
/// exactly, including the gaps where a prefix resolves to a *different* command
/// (`fo`→`fold`, `tr`→`trewind`, `final`→`final`, `i`→`insert`) — so these are
/// explicit sets, never prefix tests. A word that is not a block keyword is
/// returned unchanged. Every block-structure decision routes through this so the
/// openers and terminators accept the same forms Vim does; missing one made an
/// abbreviated `endw`/`endf` leak out as an undefined-variable expression and
/// desync the enclosing block.
fn canon_block_kw(cmd: &str) -> &str {
    match cmd {
        "fu" | "fun" | "func" | "funct" | "functi" | "functio" | "function" => "function",
        "endf" | "endfu" | "endfun" | "endfunc" | "endfunct" | "endfuncti" | "endfunctio"
        | "endfunction" => "endfunction",
        "wh" | "whi" | "whil" | "while" => "while",
        "endw" | "endwh" | "endwhi" | "endwhil" | "endwhile" => "endwhile",
        "for" => "for",
        "endfo" | "endfor" => "endfor",
        "if" => "if",
        "elsei" | "elseif" => "elseif",
        "el" | "els" | "else" => "else",
        "en" | "end" | "endi" | "endif" => "endif",
        "try" => "try",
        "cat" | "catc" | "catch" => "catch",
        "fina" | "finall" | "finally" => "finally",
        "endt" | "endtr" | "endtry" => "endtry",
        other => other,
    }
}

/// Whether `cmd` OPENS a multi-line block: the legacy `:if`/`:while`/`:for`/
/// `:function`/`:try` (routed through `canon_block_kw` so every Vim abbreviation
/// `fu`/`wh`/… counts) plus the vim9 definition blocks `:def`/`:enum`/`:class`/
/// `:interface`. Used by the tolerant parser to consume a whole block as one
/// unit when its body fails to parse.
fn is_block_opener(cmd: &str) -> bool {
    matches!(
        canon_block_kw(cmd),
        "if" | "while" | "for" | "function" | "try" | "def" | "enum" | "class" | "interface"
    )
}

/// The command word that determines a line's block role, seen through any
/// leading command modifiers (`silent`/`verbose`/…) and a vim9 `export` marker.
/// `export def`/`export function` open a block exactly as the bare forms do, so
/// the tolerant parser must classify them as openers when it skips a body that
/// failed to parse — otherwise the raw command word is `export` (not a block
/// keyword) and the block's inner statements leak out to run at the top level.
fn block_cmd_word(line: &str) -> &str {
    let (cmd, rest) = cmd_word(strip_command_modifiers(line));
    if cmd == "export" {
        cmd_word(rest).0
    } else {
        cmd
    }
}

/// Whether `cmd` is a *final* block terminator — the keyword that closes a block
/// for good (`:endif`/`:endwhile`/`:endfor`/`:endfunction`/`:endtry`, and the
/// vim9 `:enddef`/`:endenum`/`:endclass`/`:endinterface`). The mid-block
/// continuations (`:else`/`:elseif`/`:catch`/`:finally`) are NOT finals: they
/// neither open nor close a block, so they leave nesting depth unchanged when
/// balancing openers against terminators.
fn is_final_block_terminator(cmd: &str) -> bool {
    matches!(
        canon_block_kw(cmd),
        "endif"
            | "endwhile"
            | "endfor"
            | "endfunction"
            | "enddef"
            | "endtry"
            | "endenum"
            | "endclass"
            | "endinterface"
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
    let _vim9 = Vim9Guard::enter(script_is_vim9(src));
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
    let _vim9 = Vim9Guard::enter(script_is_vim9(src));
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
                if is_block_opener(block_cmd_word(&line)) {
                    // A block whose body failed to parse is consumed WHOLE, up to
                    // its matching terminator — exactly as Vim reads a block to
                    // its end regardless of body contents. This keeps the inner
                    // statements from leaking out to run at the top level (a
                    // function's `while` loop must never execute at script scope).
                    cur.skip_block_from(snapshot);
                } else {
                    // Resume at the line after the one that began the failed parse.
                    cur.i = snapshot + 1;
                }
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
        // Pass 1: continuation join. A leading `\` joins verbatim (legacy
        // `line-continuation`) everywhere. In a vim9 region — a script whose first
        // command is `:vim9script`, or the body of any `def … enddef` (vim9
        // functions are vim9 even inside a legacy script) — vim9 AUTOMATIC line
        // continuation also applies (`:help vim9-line-continuation`): an
        // expression joins the next physical line when it has unclosed
        // `[]`/`{}`/`()`, ends with a trailing binary operator, or the next line
        // begins with a binary operator / method `->` / member `.` / closing
        // bracket / command-`|`. vim9 `#` comments are dropped in the region.
        let script_vim9 = collapsed
            .iter()
            .map(|(_, l)| l.trim())
            .find(|t| !t.is_empty() && !t.starts_with('"'))
            .is_some_and(|t| t.split(char::is_whitespace).next() == Some("vim9script"));
        let mut joined: Vec<(u32, String)> = Vec::new();
        let mut in_def: u32 = 0; // depth of open `def … enddef`
        let mut open_depth: i32 = 0; // unclosed brackets of the current logical line
        for (lineno, raw) in collapsed {
            let trimmed = raw.trim_start();
            // Legacy `\` continuation: always joins the text after the `\`.
            if let Some(rest) = trimmed.strip_prefix('\\') {
                if let Some(last) = joined.last_mut() {
                    last.1.push_str(rest);
                    if script_vim9 || in_def > 0 {
                        open_depth = vim9_bracket_depth(&last.1);
                    }
                    continue;
                }
            }
            let fw = cmd_word(&raw).0;
            let is_enddef = fw == "enddef";
            let active_vim9 = script_vim9 || in_def > 0;
            // vim9 `#` comment: a line that is only a comment is dropped — skipped
            // silently while accumulating a bracketed expression, else emitted as
            // an empty line for `skip_blanks` to pass over.
            if active_vim9 && !trimmed.is_empty() && strip_vim9_comment(&raw).trim().is_empty() {
                if open_depth <= 0 {
                    joined.push((lineno, String::new()));
                }
                continue;
            }
            if active_vim9 && !is_enddef {
                if let Some(last) = joined.last_mut() {
                    let join = open_depth > 0
                        || vim9_trailing_continues(&last.1)
                        || vim9_leading_continues(trimmed)
                        || (trimmed.starts_with(':') && vim9_open_ternary(&last.1));
                    if join {
                        last.1.push(' ');
                        last.1.push_str(strip_vim9_comment(&raw).trim_start());
                        open_depth = vim9_bracket_depth(&last.1);
                        continue;
                    }
                }
            }
            // Start a new logical line. A vim9-region line (or a `def` opener,
            // whose body region begins here) has its trailing `#` comment stripped.
            let text = if active_vim9 || fw == "def" {
                strip_vim9_comment(&raw).to_string()
            } else {
                raw.to_string()
            };
            joined.push((lineno, text));
            if fw == "def" {
                in_def += 1;
            } else if is_enddef && in_def > 0 {
                in_def -= 1;
            }
            open_depth = if script_vim9 || in_def > 0 {
                vim9_bracket_depth(&joined.last().expect("just pushed").1)
            } else {
                0
            };
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
            // Commands whose argument absorbs a trailing `|` (`:autocmd`,
            // `:command`, `:normal`, `:global`/`:vglobal`) must not be bar-split:
            // the `|` is part of their text. `autocmd … exe '…' | e` is one
            // autocmd, not an autocmd followed by a stray `e` statement.
            let (lead, _) = cmd_word(strip_command_modifiers(trimmed));
            if cmd_takes_bar_arg(lead) {
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

    /// Advance the cursor past the whole block that began at logical-line index
    /// `start` (a block-opener line), leaving `i` just after the matching final
    /// terminator. The tolerant parser calls this when a block's body fails to
    /// parse, so the block is consumed as ONE unit — mirroring Vim, which reads
    /// a `:function … :endfunction` (and every `:while`/`:if`/`:for`/`:try`) to
    /// its terminator regardless of body contents. Without it, a broken body
    /// leaks its inner statements out to run at the top level (e.g. a function's
    /// `while` loop executing unbounded at script scope). Nested blocks are
    /// balanced by depth; an unterminated block consumes to end-of-input.
    fn skip_block_from(&mut self, start: usize) {
        let mut depth = 0i32;
        let mut j = start;
        while j < self.lines.len() {
            let cmd = block_cmd_word(&self.lines[j].1);
            if is_block_opener(cmd) {
                depth += 1;
            } else if is_final_block_terminator(cmd) {
                depth -= 1;
                if depth == 0 {
                    self.i = j + 1;
                    return;
                }
            }
            j += 1;
        }
        self.i = self.lines.len();
    }
}

/// Parse the statement(s) on the line at the cursor. A block opener yields one
/// `Stmt`; a leaf line yields one statement per `|`-separated command (Vim's
/// `do_one_cmd` bar split), so `let l = [1] | echo l` is two statements.
fn parse_one(cur: &mut Lines) -> Result<Vec<Stmt>, VimlError> {
    let line = cur.peek().expect("parse_one called at EOF");
    let (cmd, rest) = cmd_word(&line);
    // vim9 `export` marks the following definition (`def`/`var`/`const`/`final`/
    // `class`/`interface`/`enum`) visible to importers. Editor-less the marker has
    // no runtime effect, so strip it and re-dispatch on the real definition. Without
    // this, `export def Foo()` would fall through to the `_` command arm and its body
    // would run as top-level statements (E117/E121 on every body line).
    if cmd == "export" {
        let stripped = rest.trim_start().to_string();
        cur.lines[cur.i].1 = stripped;
        return parse_one(cur);
    }
    // Route block openers through `canon_block_kw` so every Vim abbreviation
    // (`fu`/`fun`/`func` for `:function`, `wh` for `:while`, …) opens the block.
    match canon_block_kw(cmd) {
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
        "function" => {
            cur.bump();
            // Only `[!] {name}({params})` is a definition. `:function` (list all),
            // `:function {name}` (show one), and `:function /{pat}` (list matching)
            // have no parameter-list `(` and must NOT open a block — a bare
            // `function` inside a body (e.g. `silent function`) would otherwise be
            // taken as a nested `:function` with a missing `(` (E124) and abort the
            // enclosing definition. Editor-less the listing has no output → no-op.
            let hdr = rest
                .trim_start()
                .strip_prefix('!')
                .unwrap_or(rest)
                .trim_start();
            if !hdr.starts_with('/') && hdr.contains('(') {
                Ok(vec![parse_function(cur, rest)?])
            } else {
                Ok(vec![Stmt::Expr(Expr::Number(0))])
            }
        }
        "def" => {
            cur.bump();
            // Only `[!] {name}({params})` is a vim9 definition; a bare `def`
            // (list functions) or `def {name}` (show one) has no `(` and is a
            // listing/query — a no-op editor-less, matching `:function`.
            let hdr = rest
                .trim_start()
                .strip_prefix('!')
                .unwrap_or(rest)
                .trim_start();
            if hdr.contains('(') {
                Ok(vec![parse_def(cur, rest)?])
            } else {
                Ok(vec![Stmt::Expr(Expr::Number(0))])
            }
        }
        _ => {
            cur.bump();
            // Commands that absorb a trailing `|` (`:autocmd`, `:command`,
            // `:normal`, `:global`) are parsed whole — splitting them would break
            // off part of their argument (e.g. `autocmd … exe '…' | e`).
            if cmd_takes_bar_arg(cmd_word(strip_command_modifiers(line.trim())).0) {
                return Ok(vec![parse_stmt(&line)?]);
            }
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
/// Ex commands whose argument text includes any trailing `|` — they lack Vim's
/// `EX_TRLBAR` flag, so a `|` after them is part of the command, not a command
/// separator. Abbreviation sets match Vim 9.2 `fullcommand()`. Used by the
/// logical-line splitter (Pass 2) to keep such a line whole.
fn cmd_takes_bar_arg(cmd: &str) -> bool {
    matches!(
        cmd,
        "au" | "aut"
            | "auto"
            | "autoc"
            | "autocm"
            | "autocmd"
            | "com"
            | "comm"
            | "command"
            | "norm"
            | "norma"
            | "normal"
            | "g"
            | "gl"
            | "glo"
            | "glob"
            | "globa"
            | "global"
            | "v"
            | "vg"
            | "vgl"
            | "vglo"
            | "vglob"
            | "vgloba"
            | "vglobal"
    )
}

fn split_commands(line: &str) -> Vec<&str> {
    let bytes = line.as_bytes();
    let mut segs = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut sq = false; // inside a single-quoted string ('' is an escaped quote)
    let mut dq = false; // inside a double-quoted string (\ escapes)
                        // A `:sy[ntax]` command carries `/…/`-delimited regex patterns whose bars are
                        // literal alternation, not command separators: `syn match Foo /\v(N|G|E)/`.
                        // Vim's syntax parser skips each pattern via `skip_regexp` before it looks for
                        // a trailing `|`, so a `|` inside `/…/` never ends the command. Track a slash
                        // pattern the same way sq/dq strings are tracked (`\` escapes, `/` closes) so
                        // inner bars stay literal. (`'…'`/`"…"` delimiters are already covered by the
                        // sq/dq handling.) Scoped to `:syntax` so real division — `let x = 4/2 | …` —
                        // still splits on the bar.
    let is_syntax_cmd = matches!(
        cmd_word(strip_command_modifiers(line.trim())).0,
        "sy" | "syn" | "synt" | "synta" | "syntax"
    );
    let mut slash = false; // inside a `/…/` :syntax pattern (\ escapes, / closes)
    while i < bytes.len() {
        let c = bytes[i];
        if slash {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'/' {
                slash = false;
            }
            i += 1;
            continue;
        }
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
            b'/' if is_syntax_cmd => slash = true,
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
        // Compare (and hand back) the canonical keyword so an abbreviated
        // terminator (`endw`, `endf`, `en`, `cat`, …) closes its block.
        let ck = canon_block_kw(cmd);
        if terms.contains(&ck) {
            cur.bump();
            return Ok((stmts, Some((ck.to_string(), rest.to_string()))));
        }
        if is_block_terminator(cmd) {
            return Err(VimlError::msg(format!("E580: unexpected `:{cmd}`")));
        }
        stmts.extend(parse_one(cur)?);
    }
}

const IF_TERMS: &[&str] = &["elseif", "else", "endif"];

/// Strip a legacy trailing `"` line comment from a single-expression command
/// argument (`:if`/`:elseif`/`:while`/`:for`/`:return`/`:eval`/`:call`/`:throw`
/// and a `:let` RHS). In legacy Vim script a double-quoted string never spans a
/// line, so a top-level `"` whose body does not close before end-of-line cannot
/// be a string operand — it opens a comment (`:help :comment`), which these
/// commands accept after their expression. Binary-verified against Vim 9.2:
/// `if 1 " c`, `elseif c ==? 'f' " c`, `let x = 1 " c` all succeed, while
/// `echo 1 " c` errors — so this is applied only to the single-expression
/// commands, never to `:echo` (which parses a whole expression list). A `"`
/// inside a complete `'…'`/`"…"` string, or one that *does* close, is a real
/// operand and left intact — so for a well-formed argument this only trims
/// trailing whitespace, never altering an expression that already parses. This
/// makes a trailing-comment line parse in the strict fast path (so the enclosing
/// `:function` body is captured and the function is defined) instead of failing
/// with E114 and dropping to the tolerant fallback.
fn strip_legacy_trailing_comment(s: &str) -> &str {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\'' => {
                // Single-quoted string: `''` is an escaped quote.
                i += 1;
                while i < b.len() {
                    if b[i] == b'\'' {
                        if b.get(i + 1) == Some(&b'\'') {
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            b'"' => {
                // Does this `"` open a string that closes before EOL (`\` escapes)?
                let mut j = i + 1;
                let mut closed = false;
                while j < b.len() {
                    match b[j] {
                        b'\\' => j += 2,
                        b'"' => {
                            closed = true;
                            j += 1;
                            break;
                        }
                        _ => j += 1,
                    }
                }
                if closed {
                    i = j; // a real string operand — skip past it
                } else {
                    return s[..i].trim_end(); // unterminated → start of comment
                }
            }
            _ => i += 1,
        }
    }
    s.trim_end()
}

fn parse_if(cur: &mut Lines, cond_str: &str) -> Result<Stmt, VimlError> {
    let mut arms = Vec::new();
    let mut else_body = None;
    let (body, mut term) = parse_block(cur, IF_TERMS)?;
    arms.push((parse_expr(strip_legacy_trailing_comment(cond_str))?, body));
    loop {
        match term {
            Some((ref c, ref rest)) if c == "elseif" => {
                let cond = parse_expr(strip_legacy_trailing_comment(rest))?;
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
    let cond = parse_expr(strip_legacy_trailing_comment(cond_str))?;
    let (body, term) = parse_block(cur, &["endwhile"])?;
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
    let iter = parse_expr(strip_legacy_trailing_comment(header[idx + 4..].trim()))?;
    let (body, term) = parse_block(cur, &["endfor"])?;
    if term.is_none() {
        return Err(VimlError::msg("E170: Missing :endfor"));
    }
    Ok(Stmt::For { vars, iter, body })
}

/// Return the code portion of a vim9 line, dropping a trailing `#` comment. In
/// vim9script `#` (at the start of a token — line start or preceded by
/// whitespace) is the comment leader and `"` delimits a string (unlike legacy,
/// where `"` starts the comment). `#` mid-token (`autoload#name`) is not a
/// comment. Skips `'…'` (`''` escapes) and `"…"` (`\` escapes) string bodies.
fn strip_vim9_comment(s: &str) -> &str {
    let b = s.as_bytes();
    let mut i = 0;
    let mut sq = false;
    let mut dq = false;
    let mut prev_ws = true; // start of line counts as preceded-by-whitespace
    while i < b.len() {
        let c = b[i];
        if sq {
            if c == b'\'' {
                if b.get(i + 1) == Some(&b'\'') {
                    i += 2;
                    continue;
                }
                sq = false;
            }
            prev_ws = false;
            i += 1;
            continue;
        }
        if dq {
            if c == b'\\' {
                i += 2;
                prev_ws = false;
                continue;
            }
            if c == b'"' {
                dq = false;
            }
            prev_ws = false;
            i += 1;
            continue;
        }
        match c {
            b'\'' => {
                sq = true;
                prev_ws = false;
            }
            b'"' => {
                dq = true;
                prev_ws = false;
            }
            b'#' if prev_ws => return &s[..i],
            _ => prev_ws = c == b' ' || c == b'\t',
        }
        i += 1;
    }
    s
}

/// Net unclosed-bracket depth of a vim9 code fragment (`([{` add, `)]}`
/// subtract), skipping string and `#`-comment bytes. A positive result means the
/// expression is unterminated and continues on the next physical line — vim9's
/// automatic line continuation inside `[]`/`{}`/`()` (`:help
/// vim9-line-continuation`), the fix for the earlier `[`-continuation recursion.
fn vim9_bracket_depth(s: &str) -> i32 {
    let code = strip_vim9_comment(s);
    let b = code.as_bytes();
    let mut i = 0;
    let mut depth = 0i32;
    let mut sq = false;
    let mut dq = false;
    while i < b.len() {
        let c = b[i];
        if sq {
            if c == b'\'' {
                if b.get(i + 1) == Some(&b'\'') {
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
            b'"' => dq = true,
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    depth
}

/// Whether a vim9 logical line ends mid-expression with a trailing binary
/// operator (or an assignment `=` whose RHS is on the next line), so the next
/// physical line continues it. Trailing single `.` is excluded (`.member`
/// continuation is leading, and `edit .` legitimately ends in `.`).
fn vim9_trailing_continues(s: &str) -> bool {
    let code = strip_vim9_comment(s).trim_end();
    for op in ["..", "&&", "||", "==", "!=", ">=", "<=", "=~", "!~", "->"] {
        if code.ends_with(op) {
            return true;
        }
    }
    // `<`/`>` are deliberately excluded: they clash with vim9 generic type
    // syntax (`list<number>`, `dict<string>`) that ends a `def`/`var` line.
    matches!(
        code.as_bytes().last(),
        Some(b'+' | b'-' | b'*' | b'%' | b'?' | b'&' | b'|' | b'=')
    )
}

/// Whether a vim9 physical line begins with a continuation leader — a binary
/// operator, method `->`, member `.`, closing bracket, or command-list `|` — so
/// it continues the previous line (`:help vim9-line-continuation`). A leading `:`
/// is NOT handled here (it introduces a range); the ternary `:` case is gated on
/// [`vim9_open_ternary`] by the caller.
fn vim9_leading_continues(trimmed: &str) -> bool {
    for op in ["->", "..", "&&", "||", "==", "!=", ">=", "<=", "=~", "!~"] {
        if trimmed.starts_with(op) {
            return true;
        }
    }
    // `<`/`>` are excluded (they clash with vim9 `<type>` syntax); `>=`/`<=` are
    // still matched by the multi-char loop above.
    matches!(
        trimmed.as_bytes().first(),
        Some(b'+' | b'-' | b'*' | b'/' | b'%' | b'.' | b'?' | b')' | b']' | b'}' | b'|' | b'&')
    )
}

/// Whether `s` has an unmatched top-level `?` (an open ternary), so a following
/// line that begins with `:` is the ternary's else-branch continuation rather
/// than a range. Scope colons (`g:`/`a:`/…) can undercount this; that rare miss
/// is accepted for the bounded slice.
fn vim9_open_ternary(s: &str) -> bool {
    let code = strip_vim9_comment(s);
    let b = code.as_bytes();
    let mut i = 0;
    let mut sq = false;
    let mut dq = false;
    let mut depth = 0i32;
    let mut q = 0i32;
    while i < b.len() {
        let c = b[i];
        if sq {
            if c == b'\'' {
                if b.get(i + 1) == Some(&b'\'') {
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
            b'"' => dq = true,
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'?' if depth == 0 => q += 1,
            b':' if depth == 0 => q -= 1,
            _ => {}
        }
        i += 1;
    }
    q > 0
}

/// Byte offset of the top-level assignment `=` in `s` (not part of `==`/`!=`/
/// `<=`/`>=`/`=~`), skipping strings and bracketed groups. Used to split a vim9
/// `def` parameter or `var` declaration into its LHS and initializer.
fn find_top_eq(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    for (i, &c) in b.iter().enumerate() {
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
                b'=' if depth == 0
                    && !matches!(b.get(i.wrapping_sub(1)), Some(b'!' | b'<' | b'>' | b'='))
                    && !matches!(b.get(i + 1), Some(b'=' | b'~')) =>
                {
                    return Some(i);
                }
                _ => {}
            },
        }
    }
    None
}

/// Byte offset of the first top-level `:` in `s` (a vim9 `name: type`
/// annotation separator), skipping strings and bracketed groups.
fn find_top_colon(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    for (i, &c) in b.iter().enumerate() {
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
                b':' if depth == 0 => return Some(i),
                _ => {}
            },
        }
    }
    None
}

/// Whether `s` is a plain identifier (`[A-Za-z_][A-Za-z0-9_]*`) — a vim9 `def`
/// parameter that gets a bare-name `let` prologue (excludes `...` varargs).
fn is_plain_ident(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        && !s.as_bytes()[0].is_ascii_digit()
}

/// Strip a vim9 `: type` annotation from a `var`/`final`/`const` declaration's
/// LHS, leaving `NAME = expr` for [`parse_let`]. `var x: number = 5` → `x = 5`.
/// The vim9 type system (checking/coercion, default init for a type-only
/// `var x: number`) is deferred; the annotation is parsed and discarded.
fn strip_vim9_type(rest: &str) -> String {
    let Some(eq) = find_top_eq(rest) else {
        return rest.to_string();
    };
    let (decl, tail) = (rest[..eq].trim_end(), &rest[eq..]);
    // A list-unpack LHS (`[a, b]`) has no scalar `: type`; leave it untouched.
    if decl.starts_with('[') {
        return rest.to_string();
    }
    match find_top_colon(decl) {
        Some(p) => format!("{} {}", decl[..p].trim_end(), tail),
        None => rest.to_string(),
    }
}

/// vim9 `:var` declaration LHS → a `:let`-compatible string for [`parse_let`].
/// With an initializer (`var x: T = e`) the `: T` annotation is stripped
/// ([`strip_vim9_type`]). A type-only declaration (`var x: T`, no `=`)
/// default-inits to `T`'s zero value (`:help vim9-declaration`), producing
/// `x = <default>`. Defaults are binary-verified against Vim 9.2:
/// `string ''`, `number 0`, `float 0.0`, `bool v:false`, `list []`, `dict {}`,
/// `blob 0z`, `any 0`. Opaque types real Vim rejects or has no literal for
/// (`func` → E1017, `job`/`channel`/class names) pass through unchanged so the
/// existing error path is preserved.
fn vim9_var_decl(rest: &str) -> String {
    if find_top_eq(rest).is_some() {
        return strip_vim9_type(rest);
    }
    let trimmed = rest.trim();
    // A list-unpack LHS (`[a, b]`) is never a type-only scalar declaration.
    if trimmed.starts_with('[') {
        return rest.to_string();
    }
    let Some(p) = find_top_colon(trimmed) else {
        return rest.to_string();
    };
    let name = trimmed[..p].trim_end();
    // Outermost type constructor: the leading identifier of the annotation,
    // up to `<` (list<…>/dict<…>), `(` (func(…)), whitespace, or a `#` comment.
    let outer: String = trimmed[p + 1..]
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    let default = match outer.as_str() {
        "string" => "''",
        "number" => "0",
        "float" => "0.0",
        "bool" => "v:false",
        "list" => "[]",
        "dict" => "{}",
        "blob" => "0z",
        "any" => "0",
        _ => return rest.to_string(),
    };
    format!("{name} = {default}")
}

/// True when `line` (in a vim9 region) is a bare assignment to an already-declared
/// variable — `name = expr`, `name += expr`, `d[key] = expr`, `g:x = expr`,
/// `obj.field = expr`, etc. vim9 assigns without a `:let`/`:var` keyword
/// (`:help vim9-declaration`); legacy vimscript requires `:let`, so the caller
/// gates this on [`vim9_active`]. Detection scans a valid lvalue (identifier with
/// optional scope `:`, chained `[...]` subscripts and `.field` members) and
/// requires an assignment operator (`= += -= *= /= %= ..=`) as the next token —
/// a space before the operator (as in a user command `MyCmd key=val`) or a `==`/
/// `=~` comparison is rejected. Routed through [`parse_let`], which handles every
/// assignment operator and lvalue form.
fn is_vim9_assignment(line: &str) -> bool {
    let b = line.as_bytes();
    // An lvalue starts with an identifier char, or `[` for a `[a, b] = …` /
    // `[a, b; rest] = …` list-unpack assignment; anything else is not an
    // assignment. The scan loop below skips the balanced bracket group, and the
    // trailing operator check confirms a top-level `=` follows.
    match b.first() {
        Some(&c) if c.is_ascii_alphabetic() || c == b'_' || c == b'[' => {}
        _ => return false,
    }
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            c if c.is_ascii_alphanumeric() || c == b'_' => i += 1,
            b':' => i += 1, // scope separator: g:, b:, s:, …
            b'[' => {
                // Skip a bracketed subscript, tracking nesting and strings.
                let mut depth = 0i32;
                let mut quote: Option<u8> = None;
                while i < b.len() {
                    let c = b[i];
                    i += 1;
                    match quote {
                        Some(q) => {
                            if c == q {
                                quote = None;
                            }
                        }
                        None => match c {
                            b'\'' | b'"' => quote = Some(c),
                            b'[' => depth += 1,
                            b']' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        },
                    }
                }
            }
            // `.field` member access (not the `..` concat / `..=` operator).
            b'.' if b
                .get(i + 1)
                .is_some_and(|&c| c.is_ascii_alphabetic() || c == b'_') =>
            {
                i += 1
            }
            _ => break,
        }
    }
    let op = line[i..].trim_start().as_bytes();
    match op.first() {
        Some(b'=') => !matches!(op.get(1), Some(b'=') | Some(b'~')),
        Some(b'+') | Some(b'-') | Some(b'*') | Some(b'%') | Some(b'/') => op.get(1) == Some(&b'='),
        Some(b'.') => op.get(1) == Some(&b'.') && op.get(2) == Some(&b'='),
        _ => false,
    }
}

fn parse_function(cur: &mut Lines, header: &str) -> Result<Stmt, VimlError> {
    // A legacy `:function … endfunction` body is legacy even inside a
    // `:vim9script`: its dict literals use expression keys, not vim9 bare keys.
    let _vim9 = Vim9Guard::enter(false);
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
    let (body, term) = parse_block(cur, &["endfunction"])?;
    if term.is_none() {
        return Err(VimlError::msg("E126: Missing :endfunction"));
    }
    Ok(Stmt::Function {
        name,
        args,
        defaults,
        body,
        bang,
        // Legacy `:function`: bare names in the body do NOT see script-scope
        // vars (that requires an explicit `s:`/`g:` prefix).
        vim9: false,
    })
}

/// Parse a vim9 `def[!] {name}({params}): {rettype} … enddef` definition.
///
/// Unlike legacy `:function`, vim9 parameters are BARE names: inside the body `x`
/// refers to the parameter `x` directly (no `a:` prefix). This is realized by
/// synthesizing a `let {p} = a:{p}` prologue for each plain-identifier
/// parameter — the identical mechanism the `{args -> body}` lambda compiler uses
/// (compile_viml.rs) — so params still bind through the existing `a:` call
/// machinery yet resolve as bare locals when the body runs.
///
/// Parameter `: type` annotations and the `: rettype` return type are PARSED and
/// discarded (the vim9 type system is deferred). Optional `param = default`
/// values are honored via the shared [`Stmt::Function`] `defaults` path. The body
/// uses vim9 automatic line continuation, joined in [`Lines::new`]. `...` varargs
/// collection and full type checking are deferred.
fn parse_def(cur: &mut Lines, header: &str) -> Result<Stmt, VimlError> {
    // A `def … enddef` body is vim9 even inside a legacy script: its parameter
    // defaults and body statements parse with vim9 bare-key dict semantics.
    let _vim9 = Vim9Guard::enter(true);
    let header = header.trim();
    // A leading `!` (`def!`) is accepted and discarded — vim9 `def` always
    // (re)defines, so there is no bang distinction as for legacy `:function`.
    let header = header.strip_prefix('!').map_or(header, str::trim_start);
    let lparen = header
        .find('(')
        .ok_or_else(|| VimlError::msg("E1055: Missing '(' in :def"))?;
    let name = header[..lparen].trim().to_string();
    // Match the parameter-list `)` (tracking nesting, skipping quoted strings) —
    // a default value may contain its own parens/brackets.
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
        found.ok_or_else(|| VimlError::msg("E1055: Missing ')' in :def"))?
    };
    // Text after `)` is `: rettype` (or nothing) — parsed and ignored.
    let mut args: Vec<String> = Vec::new();
    let mut defaults: Vec<(usize, Expr)> = Vec::new();
    for raw in split_top_commas(&header[lparen + 1..rparen]) {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        if raw.starts_with("...") {
            // Varargs boundary: record the marker so fixed params before it bind
            // correctly; collecting the rest into a typed list is deferred.
            args.push("...".to_string());
            continue;
        }
        // `name[: type][ = default]` — split off the default, then the type.
        let (decl, default) = match find_top_eq(raw) {
            Some(p) => (raw[..p].trim(), Some(raw[p + 1..].trim())),
            None => (raw, None),
        };
        let pname = match find_top_colon(decl) {
            Some(p) => decl[..p].trim(),
            None => decl,
        };
        if let Some(d) = default {
            defaults.push((args.len(), parse_expr(d)?));
        }
        args.push(pname.to_string());
    }
    let (body_stmts, term) = parse_block(cur, &["enddef"])?;
    if term.is_none() {
        return Err(VimlError::msg("E1057: Missing :enddef"));
    }
    // Prepend `let {p} = a:{p}` so each bare parameter name resolves in the body.
    let mut body: Vec<Stmt> = Vec::with_capacity(body_stmts.len() + args.len());
    for p in &args {
        if is_plain_ident(p) {
            body.push(Stmt::Let {
                target: LetTarget::Var(p.clone()),
                expr: Expr::Var(format!("a:{p}")),
            });
        }
    }
    body.extend(body_stmts);
    Ok(Stmt::Function {
        name,
        args,
        defaults,
        body,
        // vim9 `def` always (re)defines; there is no "already defined" error as
        // for legacy `:function` without `!`.
        bang: true,
        // vim9 `def`: bare names in the body resolve to script-scope
        // vars/functions when they are not locals or parameters.
        vim9: true,
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
    let Some(eq) = rest.find('=') else {
        // No `=`: `:let` (list all variables) or `:let {var}` (show one) — a
        // listing/show command, not an assignment. Editor-less it has no
        // observable output, so it is a no-op; erroring here would abort an
        // enclosing `:function` whose body lists variables (`silent let`).
        return Ok(Stmt::Expr(Expr::Number(0)));
    };
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
    // vim9 string concat-assign is `..=` (two dots); legacy is `.=` (one). The
    // op char sits at `eq - 1`; for `..=` a second dot precedes it, so strip both.
    let lhs_end = match op {
        Some(ArithOp::Concat) if eq >= 2 && rest.as_bytes()[eq - 2] == b'.' => eq - 2,
        Some(_) => eq - 1,
        None => eq,
    };
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
    let rhs = strip_legacy_trailing_comment(rhs);
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
/// at the top of `heredoc_get()` (vendor/eval/vars.c). vim9 declaration keywords
/// (`var`/`final`/`const`) also open a heredoc (`:help vim9` uses the same
/// `=<<` list-assignment form as legacy `:let`).
fn heredoc_opener(line: &str) -> Option<(String, bool, bool, String)> {
    let (cmd, _) = cmd_word(line.trim_start());
    if !matches!(cmd, "let" | "const" | "cons" | "var" | "final") {
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
    let mut p = Parser::new(toks, src);
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
    let mut p = Parser::new(toks, src);
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
    /// The source string the tokens were lexed from. Token spans are byte
    /// offsets into this; used to extract the exact text of a vim9 bare dict key
    /// (`{a-b: 1}` → `"a-b"`), which does not survive tokenization intact.
    src: String,
    /// Count of bracket-nested sub-expressions currently open (`(expr)`, list
    /// elements, call arguments, dict values). Vim caps this at
    /// [`Self::EXPR_MAX_DEPTH`] and raises `E1169` once it is reached — verified
    /// against `/opt/homebrew/bin/vim`: 999 nested `(` succeed, 1000 → E1169.
    /// Ternary branches, unary leaders and left-associative chains (`.`, `+`,
    /// `[idx]`) do NOT bump this — vim accepts those to great depth (500000
    /// nested `-` succeed), matching a loop/tail-recursive parse here.
    depth: u32,
}

impl Parser {
    /// Vim's expression-nesting limit (`E1169: Expression too recursive`). The
    /// error fires when the 1000th bracket opens, so 999 levels are accepted.
    const EXPR_MAX_DEPTH: u32 = 1000;

    fn new(toks: Vec<Token>, src: &str) -> Self {
        Parser {
            toks,
            i: 0,
            src: src.to_string(),
            depth: 0,
        }
    }

    /// Parse a bracket-nested sub-expression, bumping the recursion counter so a
    /// pathologically deep `((((…))))` / `[[[…]]]` / `f(f(f(…)))` / `{a:{a:…}}`
    /// raises Vim's `E1169` instead of overflowing the native stack. Guards only
    /// the bracket-open recursion points (paren, list/call element, dict value)
    /// so it matches vim, which does not count ternary/unary/concat depth.
    fn nested_eval1(&mut self) -> Result<Expr, VimlError> {
        self.depth += 1;
        if self.depth >= Self::EXPR_MAX_DEPTH {
            self.depth -= 1;
            let tail = self.src.get(self.toks[self.i].span..).unwrap_or("");
            return Err(VimlError::msg(format!(
                "E1169: Expression too recursive: {tail}"
            )));
        }
        let r = self.eval1();
        self.depth -= 1;
        r
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
            Tok::InterpStr(parts) => self.lower_interp(parts),
            Tok::Option(o) => Ok(Expr::Option(o)),
            Tok::Env(e) => Ok(Expr::Env(e)),
            Tok::Register(r) => Ok(Expr::Register(r)),
            Tok::LParen => {
                // vim9 lambda `(params) => body` (opening `(` already consumed);
                // otherwise an ordinary parenthesised expression.
                if self.at_vim9_lambda() {
                    return self.vim9_lambda();
                }
                let e = self.nested_eval1()?;
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
                } else if vim9_active() {
                    // vim9 keyword literals (`vim9.txt`): in a `:vim9script` script
                    // or a `def…enddef` body, bare `true`/`false`/`null` are the
                    // boolean/special constants — equal to `v:true`/`v:false`/`v:null`
                    // (binary-verified: `true == v:true`, `type(true) == 6`,
                    // `type(null) == 7`). In legacy mode they stay ordinary names
                    // (bare `true` → E121 undefined variable), so this only fires
                    // under `vim9_active()`.
                    // The `null_*` names are vim9 predefined constants
                    // (`vim9.txt`, "Predefined variables"). Each is the null
                    // value of its type; oracle-verified against Vim 9.2 they
                    // are observably the empty literal of that type
                    // (`type(null_string) == 1 && null_string == ''`,
                    // `type(null_list) == 3 && string(null_list) == '[]'`,
                    // `null_blob` → `0z`, `null_function`/`null_partial` →
                    // `function('')`). `null_channel`/`null_job` need channel
                    // and job value types vimlrs does not have, so they stay
                    // ordinary names rather than being faked.
                    match name.as_str() {
                        "true" => Ok(Expr::Var("v:true".to_string())),
                        "false" => Ok(Expr::Var("v:false".to_string())),
                        "null" => Ok(Expr::Var("v:null".to_string())),
                        "null_string" => Ok(Expr::Str(String::new())),
                        "null_list" => Ok(Expr::List(Vec::new())),
                        "null_dict" => Ok(Expr::Dict(Vec::new())),
                        "null_blob" => Ok(Expr::Call {
                            name: "list2blob".to_string(),
                            args: vec![Expr::List(Vec::new())],
                        }),
                        "null_function" | "null_partial" => Ok(Expr::Call {
                            name: "function".to_string(),
                            args: vec![Expr::Str(String::new())],
                        }),
                        _ => Ok(Expr::Var(name)),
                    }
                } else {
                    Ok(Expr::Var(name))
                }
            }
            other => Err(VimlError::msg(format!(
                "E15: Invalid expression: unexpected {other:?}"
            ))),
        }
    }

    /// Lower an interpolated string's raw parts into an [`Expr::Interp`]: each
    /// literal chunk becomes an `Expr::Str`, each `{expr}` region is sub-parsed.
    /// A blank/empty region (`{ }`, `{}`) sub-parses to an E15 error, matching
    /// Vim (an empty interpolation expression is invalid). The compiler
    /// echo-stringifies and concatenates the segments.
    fn lower_interp(&mut self, parts: Vec<InterpPart>) -> Result<Expr, VimlError> {
        let mut segs = Vec::with_capacity(parts.len());
        for part in parts {
            match part {
                InterpPart::Lit(s) => segs.push(Expr::Str(s)),
                InterpPart::Expr(src) => segs.push(parse_expr(&src)?),
            }
        }
        Ok(Expr::Interp(segs))
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
            // `d.key` member read. Skipped for statically-known non-Dict bases —
            // number/float (`1.foo`), string (`'('.name`) and list literals can
            // never be a Dict at runtime, so their `.name` is unambiguously
            // concat and is left to `eval6`. Leaving it there is also what makes
            // the concat RHS bind trailing subscripts correctly (`'('.p[0]` →
            // `'(' . p[0]`, not `('(' . p)[0]`): eval6 re-parses the RHS as a
            // fresh postfix chain, matching vim's `handle_subscript`, which only
            // consumes `.name` when `rettv->v_type == VAR_DICT`.
            if self.at_member_dot()
                && !matches!(
                    base,
                    Expr::Number(_) | Expr::Float(_) | Expr::Str(_) | Expr::List(_)
                )
            {
                self.advance(); // consume the dot
                if let Tok::Ident(key) = self.advance() {
                    // Syntactically identical to string concat (`a.b`): whether
                    // this is a Dict subscript or `.`-concat is decided by the
                    // runtime type of `base` (see the `Expr::Member` lowering in
                    // `compile_viml.rs`). Carry the literal key; it doubles as the
                    // bare variable name for the concat RHS (`a.b` → `a . b`).
                    base = Expr::Member {
                        base: Box::new(base),
                        key,
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

    /// Lookahead (opening `(` already consumed) distinguishing a vim9 lambda
    /// `(params) => body` — optionally `(params): rettype => body` — from an
    /// ordinary parenthesised expression. Scans to the matching `)`, then requires
    /// a `=>` next, allowing an optional `: rettype` annotation in between. Only
    /// fires in a vim9 region; legacy scripts have no `=>` lambda.
    fn at_vim9_lambda(&self) -> bool {
        if !vim9_active() {
            return false;
        }
        let mut j = self.i;
        let mut depth = 1i32;
        while depth > 0 {
            match self.toks.get(j).map(|t| &t.kind) {
                None | Some(Tok::Eof) => return false,
                Some(Tok::LParen) | Some(Tok::LBracket) | Some(Tok::LBrace) => depth += 1,
                Some(Tok::RParen) | Some(Tok::RBracket) | Some(Tok::RBrace) => depth -= 1,
                _ => {}
            }
            j += 1;
        }
        // `j` is now just past the matching `)`.
        match self.toks.get(j).map(|t| &t.kind) {
            Some(Tok::FatArrow) => true,
            // `(params): rettype => body` — a top-level `=>` must follow the `:`.
            Some(Tok::Colon) => {
                let mut k = j + 1;
                let mut d = 0i32;
                while let Some(t) = self.toks.get(k) {
                    match &t.kind {
                        Tok::FatArrow if d == 0 => return true,
                        Tok::LParen | Tok::LBracket | Tok::LBrace => d += 1,
                        Tok::RParen | Tok::RBracket | Tok::RBrace => {
                            if d == 0 {
                                return false;
                            }
                            d -= 1;
                        }
                        Tok::Comma if d == 0 => return false,
                        Tok::Eof => return false,
                        _ => {}
                    }
                    k += 1;
                }
                false
            }
            _ => false,
        }
    }

    /// Parse a vim9 lambda `(params) => body` (opening `(` already consumed).
    /// Parameter type annotations (`x: type`) and an optional return type
    /// (`): type =>`) are accepted and discarded — only the names bind, reusing the
    /// same `Expr::Lambda` representation as the legacy `{params -> body}` form.
    fn vim9_lambda(&mut self) -> Result<Expr, VimlError> {
        let mut params = Vec::new();
        if !matches!(self.peek(), Tok::RParen) {
            loop {
                match self.advance() {
                    // A scope-letter parameter (`a`, `b`, `s`, `g`, `l`, `t`, `v`,
                    // `w`) followed by a `: type` annotation is lexed with the colon
                    // absorbed into the identifier (`a: number` → `Ident("a:")`,
                    // `a:list` → `Ident("a:list")`). Split the name off at that colon
                    // and skip the remaining type tokens; the standalone-colon case
                    // (`n: number`) is handled just below.
                    Tok::Ident(n) => match n.find(':') {
                        Some(pos) => {
                            params.push(n[..pos].to_string());
                            self.skip_type();
                        }
                        None => params.push(n),
                    },
                    other => {
                        return Err(VimlError::msg(format!(
                            "E15: expected lambda parameter, found {other:?}"
                        )))
                    }
                }
                if matches!(self.peek(), Tok::Colon) {
                    self.advance();
                    self.skip_type();
                }
                match self.peek() {
                    Tok::Comma => {
                        self.advance();
                    }
                    Tok::RParen => break,
                    other => {
                        return Err(VimlError::msg(format!(
                            "E15: expected ',' or ')' in lambda, found {other:?}"
                        )))
                    }
                }
            }
        }
        self.eat(&Tok::RParen)?;
        if matches!(self.peek(), Tok::Colon) {
            self.advance();
            self.skip_type();
        }
        self.eat(&Tok::FatArrow)?;
        let body = self.eval1()?;
        Ok(Expr::Lambda {
            params,
            body: Box::new(body),
        })
    }

    /// Skip a vim9 type expression in a lambda signature, consuming tokens up to a
    /// top-level `,`, `)`, or `=>` (its terminators in param / return position).
    /// `<…>` (`list<number>`), `(…)` (`func(...)`) and `[…]` nest.
    fn skip_type(&mut self) {
        let mut angle = 0i32;
        let mut paren = 0i32;
        loop {
            match self.peek() {
                Tok::Cmp(CmpOp::Less, _) => {
                    angle += 1;
                    self.advance();
                }
                Tok::Cmp(CmpOp::Greater, _) if angle > 0 => {
                    angle -= 1;
                    self.advance();
                }
                Tok::LParen | Tok::LBracket => {
                    paren += 1;
                    self.advance();
                }
                Tok::RParen | Tok::RBracket if paren > 0 => {
                    paren -= 1;
                    self.advance();
                }
                Tok::Comma | Tok::RParen | Tok::FatArrow if angle == 0 && paren == 0 => break,
                Tok::Eof => break,
                _ => {
                    self.advance();
                }
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
                (stripped.to_string(), self.nested_eval1()?)
            } else if let Some(c) = raw.find(':') {
                // `a:1` — a scope-letter key merged with a glued simple value
                // (the lexer can only glue a bareword/number after the `:`); split
                // and parse that fragment as the value.
                (raw[..c].to_string(), parse_expr(&raw[c + 1..])?)
            } else {
                self.eat(&Tok::Colon)?;
                (raw, self.nested_eval1()?)
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
        // In a vim9 region `{key: val}` uses BARE literal keys — the key text is
        // taken verbatim, not evaluated as a variable (`:help vim9-scriptlocal`,
        // vim9.txt "the {} form uses literal keys"). Legacy scripts keep the
        // expression-keyed form.
        let vim9 = vim9_active();
        loop {
            let key = if vim9 {
                self.vim9_dict_key()?
            } else {
                let k = self.eval1()?;
                self.eat(&Tok::Colon)?;
                k
            };
            let val = self.nested_eval1()?;
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

    /// Parse a vim9 dict key and its trailing `:`, leaving the cursor at the
    /// value. Three key forms, matching Vim 9.2:
    ///  * `{'k': v}` / `{"k": v}` — a quoted string key.
    ///  * `{[expr]: v}` — a bracketed computed key; `expr` is evaluated and its
    ///    string form is the key.
    ///  * `{a: 1}`, `{a-b: 1}`, `{007: 1}` — a BARE key: a run of
    ///    `[A-Za-z0-9_-]` used verbatim as a string (leading zeros kept). The
    ///    tokens (`Ident`/`Number`/`Minus`, or a scope-letter `Ident` that
    ///    absorbed the `:`) do not preserve the key intact, so it is read from
    ///    the source between the key start and the `:`.
    fn vim9_dict_key(&mut self) -> Result<Expr, VimlError> {
        if let Tok::Str(s) = self.peek().clone() {
            self.advance();
            self.eat(&Tok::Colon)?;
            return Ok(Expr::Str(s));
        }
        if matches!(self.peek(), Tok::LBracket) {
            self.advance();
            let key = self.eval1()?;
            self.eat(&Tok::RBracket)?;
            self.eat(&Tok::Colon)?;
            return Ok(key);
        }
        let start = self.toks[self.i].span;
        let bytes = self.src.as_bytes();
        let mut p = start;
        while p < bytes.len()
            && (bytes[p].is_ascii_alphanumeric() || bytes[p] == b'_' || bytes[p] == b'-')
        {
            p += 1;
        }
        if p == start {
            return Err(VimlError::msg(format!(
                "E15: expected literal Dict key, found {:?}",
                self.peek()
            )));
        }
        let key = self.src[start..p].to_string();
        // The `:` separator follows the key (optional intervening whitespace is
        // tolerated as a benign superset; strict Vim rejects it with E1068).
        let mut colon = p;
        while colon < bytes.len() && (bytes[colon] == b' ' || bytes[colon] == b'\t') {
            colon += 1;
        }
        if colon >= bytes.len() || bytes[colon] != b':' {
            return Err(VimlError::msg(
                "E720: Missing colon in Dictionary".to_string(),
            ));
        }
        // Advance past every token up to and including the `:` at offset `colon`
        // (a standalone `Colon`, or one absorbed into a scope-letter `Ident`),
        // leaving the cursor at the value.
        while self.toks[self.i].span <= colon && !matches!(self.peek(), Tok::Eof) {
            self.advance();
        }
        Ok(Expr::Str(key))
    }

    fn arg_list(&mut self, close: &Tok) -> Result<Vec<Expr>, VimlError> {
        let mut args = Vec::new();
        if self.peek() == close {
            self.advance();
            return Ok(args);
        }
        loop {
            args.push(self.nested_eval1()?);
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
        // `d.key` (no spaces) → a runtime-dispatched Member (Dict subscript vs
        // string concat, decided by the runtime type of the base).
        match parse_expr("d.key").unwrap() {
            Expr::Member { key, .. } => assert_eq!(key, "key"),
            e => panic!("expected Member, got {e:?}"),
        }
        // Nested `d.a.b` → chained Members.
        assert!(matches!(parse_expr("d.a.b").unwrap(), Expr::Member { .. }));
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
