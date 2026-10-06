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

use crate::viml_ast::{ArithOp, Block, Expr, ForVars, LetTarget, Stmt, UnaryOp, UnletArg};
use crate::viml_lexer::{lex, CaseFlag, CmpOp, InterpPart, Tok, Token, VimlError};
use std::cell::Cell;

thread_local! {
    /// Whether the code currently being parsed is vim9 (`:vim9script` script or a
    /// `def … enddef` body). In vim9, dict literals `{key: val}` use BARE literal
    /// keys (`{a: 1}` → key `"a"`), not the legacy expression-keyed form where the
    /// key is evaluated. Set for the parse duration by [`Vim9Guard`].
    static VIM9: Cell<bool> = const { Cell::new(false) };
    /// Whether the text being parsed is a COMMAND LINE (`:execute`, `execute()`,
    /// an autocommand or user-command body) rather than a sourced file. c:
    /// `do_cmdline()` (`ex_docmd.c:832-849`) reports `E600`/`E170`/`E171` for a
    /// conditional left open only when the lines came from `getsourceline` or
    /// `get_func_line`; for any other getline it rewinds the condition stack
    /// silently. So on a command line an unclosed `:if`/`:try` simply ends
    /// there, and an unclosed `:while`/`:for` runs its body once — nothing
    /// reaches the `:endwhile` that would jump back. Set by [`parse_cmdline`].
    static CMDLINE_EOF: Cell<bool> = const { Cell::new(false) };
    /// The command [`parse_stmt`] is reading, as `(start address, length)`,
    /// with the text from its start to the end of its source line.
    ///
    /// c: a lval's `ll_name` points INTO the command line, and
    /// `value_check_lock(…, lp->ll_name, TV_CSTRING)` prints from there to the
    /// line's NUL — past any `|` and the commands after it. Bar-splitting has
    /// already cut those off the text the parser sees, so the tail is kept here
    /// for [`text_to_eol`].
    static CMD_TAIL: std::cell::RefCell<Option<(usize, usize, String)>> =
        const { std::cell::RefCell::new(None) };
    /// Whether every line being parsed is an Ex command, as in vim: a sourced
    /// file, or the text `:execute` and friends run. c: `do_one_cmd`
    /// (`ex_docmd.c`) reads a line as `[range]{command}` and nothing else, so a
    /// leading number is a range (`30` goes to line 30, `1wincmd w` is a count)
    /// and a word from the command table is that command (`only`, `cd ~/`),
    /// never an expression. Off for the `eval_source` REPL, which evaluates bare
    /// expressions, so `3 + 4` there still yields 7. Set by [`with_ex_lines`].
    static EX_LINES: Cell<bool> = const { Cell::new(false) };
}

/// Byte offset of `part` (a subslice of `whole`) within `whole`.
fn slice_offset(whole: &str, part: &str) -> usize {
    part.as_ptr() as usize - whole.as_ptr() as usize
}

/// Run `f` with [`CMD_TAIL`] describing `cmd`, whose source line continues as
/// `tail` (which starts with `cmd`'s text).
fn with_cmd_tail<T>(cmd: &str, tail: &str, f: impl FnOnce() -> T) -> T {
    let entry = tail
        .starts_with(cmd)
        .then(|| (cmd.as_ptr() as usize, cmd.len(), tail.to_string()));
    let saved = CMD_TAIL.with(|t| std::mem::replace(&mut *t.borrow_mut(), entry));
    let out = f();
    CMD_TAIL.with(|t| *t.borrow_mut() = saved);
    out
}

/// c: `MAX_FUNC_ARGS` (`eval/typval_defs.h`) — the most arguments a call may
/// pass.
const MAX_FUNC_ARGS: usize = 20;

/// `arg` (a subslice of the command being parsed) extended to the end of its
/// source line, trimmed; just `arg` trimmed when no tail is known.
fn text_to_eol(arg: &str) -> String {
    CMD_TAIL.with(|t| {
        if let Some((start, len, tail)) = &*t.borrow() {
            let at = arg.as_ptr() as usize;
            if at >= *start && at + arg.len() <= start + len {
                if let Some(rest) = tail.get(at - start..) {
                    return rest.trim().to_string();
                }
            }
        }
        arg.trim().to_string()
    })
}

/// The command text vim appends to an `eap->errmsg` error (`E586`/`E587`).
///
/// c: `do_one_cmd` reports `errormsg` through `append_command(*cmdlinep)`, the
/// command as written: from just after the previous `|` (or the start of the
/// line) up to where `separate_nextcmd` cut it — the next `|`, or the `"` of a
/// trailing comment — so command modifiers and the blanks around the command
/// stay in (`if 1 | break | endif` reports `:  break `). `line` is the
/// modifier-stripped text, used only when no source segment is known.
fn errmsg_cmd_text(line: &str) -> String {
    let raw = CMD_TAIL.with(|t| {
        t.borrow()
            .as_ref()
            .and_then(|(_, len, tail)| tail.get(..*len).map(str::to_string))
    });
    let raw = raw.unwrap_or_else(|| line.to_string());
    match raw.find('"') {
        Some(q) => raw[..q].to_string(),
        None => raw,
    }
}

/// Run `f` with [`EX_LINES`] on, restoring the previous value afterwards.
pub fn with_ex_lines<T>(f: impl FnOnce() -> T) -> T {
    let saved = EX_LINES.with(|c| c.replace(true));
    let out = f();
    EX_LINES.with(|c| c.set(saved));
    out
}

/// A legacy-script line read as vim reads it (see [`EX_LINES`]). vim9 keeps
/// expression statements (`list->add(1)`), so it is excluded.
fn ex_lines_active() -> bool {
    EX_LINES.with(|c| c.get()) && !vim9_active()
}

/// Whether `word` names a built-in Ex command by `find_ex_command`'s rule
/// (`ex_docmd.c:3130-3137`): it is a prefix of some [`CMDNAMES`] entry.
///
/// [`CMDNAMES`]: crate::ported::eval::funcs::CMDNAMES
fn is_builtin_ex_command(word: &str) -> bool {
    !word.is_empty()
        && crate::ported::eval::funcs::CMDNAMES
            .iter()
            .any(|name| name.starts_with(word))
}

/// Whether `line`, read as an Ex command line (see [`EX_LINES`]), is an Ex
/// command rather than one of the statements vimlrs evaluates itself — the
/// case where running it again as a statement only yields the same command.
pub fn is_ex_command_line(line: &str) -> bool {
    with_ex_lines(|| matches!(parse_stmt(line), Ok(Stmt::ExCmd(_))))
}

/// Whether `line`, after any range, names a built-in Ex command. One that
/// neither vimlrs nor its host runs is then skipped, as vim runs it without a
/// word (`only`, `argglobal`, `%argdel` print nothing under `vim -es`).
pub fn names_builtin_ex_command(line: &str) -> bool {
    let rest = line.trim_start_matches(|c: char| {
        c.is_ascii_digit() || matches!(c, ':' | '%' | '$' | '.' | ',' | ';' | '+' | '-' | ' ')
    });
    let end = rest
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(rest.len());
    is_builtin_ex_command(&rest[..end])
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

/// The window modifiers vim accepts before a command (`:vertical resize 30`,
/// `:tab split`, `:topleft copen`).
const WINDOW_MODIFIERS: &[&str] = &[
    "vertical",
    "vert",
    "horizontal",
    "hor",
    "tab",
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
];

/// The window modifiers in `line`'s leading modifier run, each followed by a
/// space (`"vertical "` for `silent! vert 1resize 30`), with `tab`'s count.
fn window_modifiers(line: &str) -> String {
    let line = line.trim();
    let prefix = &line[..line.len() - strip_command_modifiers(line).len()];
    let mut out = String::new();
    let mut keep_count = false;
    for token in prefix.split_whitespace() {
        let word = token.trim_end_matches('!');
        if WINDOW_MODIFIERS.contains(&word)
            || (keep_count && word.bytes().all(|b| b.is_ascii_digit()))
        {
            out.push_str(token);
            out.push(' ');
        }
        keep_count = word == "tab";
    }
    out
}

/// Put `placement` back in front of the Ex command `stmt` runs. vimlrs has no
/// windows, so a window modifier only means something to the embedding host
/// that receives the command line (`:vertical 1resize 30` sizes a width).
fn place(stmt: Stmt, placement: &str) -> Stmt {
    match stmt {
        Stmt::ExCmd(cmd) => Stmt::ExCmd(format!("{placement}{cmd}")),
        Stmt::Silent { bang, stmt } => Stmt::Silent {
            bang,
            stmt: Box::new(place(*stmt, placement)),
        },
        other => other,
    }
}

/// Parse one statement line into a [`Stmt`].
///
/// Inline trailing comments (`echo 1  " note`) are not stripped in Phase 3: a
/// `"` is a comment only where the command grammar expects end-of-command, but
/// in expression position it opens a string. Full-line comments are skipped by
/// the source splitter before this is called.
pub fn parse_stmt(line: &str) -> Result<Stmt, VimlError> {
    let placement = window_modifiers(line);
    let stmt = parse_stmt_unplaced(line)?;
    Ok(if placement.is_empty() {
        stmt
    } else {
        place(stmt, &placement)
    })
}

/// [`parse_stmt`] before the window modifiers are put back.
fn parse_stmt_unplaced(line: &str) -> Result<Stmt, VimlError> {
    // Strip leading command modifiers (`silent`, `silent!`, `verbose 9`,
    // `noautocmd`, `keepjumps`, …). They change how a command runs, not what it
    // is, and real vimrcs use them constantly (`silent! colorscheme x`). A bare
    // modifier with no command (`silent`) becomes a no-op.
    // `silent!` is not just noise: it suppresses the error message of the command
    // it prefixes (`emsg_silent`), which is why `silent! call Foo()` is everywhere
    // in real vimrcs. Keep that one bit and strip the rest of the modifiers.
    let silent = leading_silent(line.trim());
    let line = strip_command_modifiers(line.trim());
    if line.is_empty() {
        return Ok(Stmt::Expr(Expr::Number(0)));
    }
    if let Some(bang) = silent {
        // Re-parse the remainder as the wrapped command.
        let inner = parse_stmt(line)?;
        return Ok(Stmt::Silent {
            bang,
            stmt: Box::new(inner),
        });
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

    match ex_full_name(cmd) {
        "echo" => Ok(Stmt::Echo(parse_expr_list(rest)?)),
        "echon" => Ok(Stmt::Echon(parse_expr_list(rest)?)),
        // `:echomsg` (`echom`) evaluates and prints its expression list; it is
        // modelled as `:echo` (there is no message history to add it to).
        "echomsg" => Ok(Stmt::Echo(parse_expr_list(rest)?)),
        // `:echoerr` (`echoe`/`echoer`) reports its arguments as an ERROR — see
        // `Stmt::EchoErr`.
        "echoerr" => Ok(Stmt::EchoErr(parse_expr_list(rest)?)),
        // `:execute` accepts every prefix down to `:exe` (verified against Vim
        // 9.2: `exe`/`exec`/`execu`/`execut`/`execute` all run). Missing the
        // intermediate forms made `exec '…'` fall through to `parse_expr`, which
        // aborts the enclosing function definition and leaks its body to global
        // scope (E461 on `l:` vars).
        "execute" => Ok(Stmt::Execute(parse_expr_list(rest)?)),
        "set" => Ok(Stmt::Set(rest.to_string())),
        "setlocal" => Ok(Stmt::Setlocal(rest.to_string())),
        "setglobal" => Ok(Stmt::Setglobal(rest.to_string())),
        "source" => Ok(Stmt::Source(rest.trim().to_string())),
        "unlet" => {
            // `:unlet[!] x y …` — the optional `!` suppresses the missing-var
            // error. Each argument is a bare name or a List/Dict element target
            // (`l[i]` / `d.key`), matching `do_unlet_var()` (vendor/eval/vars.c).
            let bang = rest.starts_with('!');
            let args = rest.trim_start_matches('!').trim();
            split_unlet_args(args)
                .into_iter()
                .map(parse_unlet_arg)
                .collect::<Result<Vec<_>, _>>()
                .map(|args| Stmt::Unlet { args, bang })
        }
        // `:lockvar[!] [depth] {name}…` / `:unlockvar[!] …` — `ex_lockvar`
        // (vendor/eval/vars.c): `!` locks/unlocks all levels (`DICT_MAXNEST`), an
        // optional leading number is the explicit depth, and the default is 2.
        "lockvar" | "unlockvar" => parse_lockvar(rest, !cmd.starts_with("un")),
        "let" => parse_legacy_let(rest),
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
        "function" if !rest.contains('(') => Ok(Stmt::Expr(Expr::Number(0))),
        // `:const {name} = {expr}` / `:const [a, b; rest] = {expr}` — `ex_let`
        // with `is_const`. `set_var_const` refuses a name that already exists
        // (E995) and locks a new one with `DICT_MAXNEST` depth, which is why a
        // later `:let` of it is E741. Any other target shape is left to `:let`.
        "const" => match parse_let(&strip_vim9_type(rest))? {
            Stmt::Let {
                target: target @ (LetTarget::Var(_) | LetTarget::List { .. }),
                expr,
            } if !matches!(expr, Expr::Arith { mod_op: true, .. }) => {
                Ok(Stmt::Const { target, expr })
            }
            other => Ok(other),
        },
        // c: `ex_call` resolves the name with `trans_function_name`, which hands
        // `get_func_tv` an allocated, NUL-TERMINATED copy — so the E116 it may
        // report names the function and nothing else, unlike the same call written
        // in an expression. Verified against vim 9.2: `call type([1] . '')` is
        // `E116: Invalid arguments for function type`, while `echo type([1] . '')`
        // is `E116: … function type([1] . '')`. Only the OUTERMOST call is
        // renamed; one nested in an argument was still read from the source.
        "call" => {
            // An argument list that does not parse is reported when the `:call`
            // runs (see `parse_cmd_expr`): `call len(1 2)` is E116 there.
            let mut e = parse_cmd_expr_for(strip_legacy_trailing_comment(rest), true)?;
            if let Expr::Call { emsg_name, .. } = &mut e {
                *emsg_name = None;
            }
            Ok(Stmt::Call(e))
        }
        // `:defer Func(args)` takes a call with no `:call` in front of it, so the
        // expression is parsed exactly as `:call`'s is.
        "defer" => Ok(Stmt::Defer(parse_expr(strip_legacy_trailing_comment(
            rest,
        ))?)),
        "eval" => Ok(Stmt::Expr(parse_cmd_expr(strip_legacy_trailing_comment(
            rest,
        ))?)),
        // Abbreviations, as explicit sets rather than prefix tests, because the
        // shortest accepted prefix is not the shortest unique one and the
        // one-shorter spelling usually resolves to a DIFFERENT command. Read out
        // of vim 9.2.0900 with `fullcommand()`: `ret`→retab (not return),
        // `bre`→brewind (not break), `fin`→find (not finish), `co`→copy (not
        // continue). Getting this wrong is silent: `retu 42` parsed as an
        // expression statement, which made the whole enclosing `:function`
        // fail to parse and `E117: Unknown function` fire at the call site.
        "break" => Ok(Stmt::Break(errmsg_cmd_text(line))),
        "continue" => Ok(Stmt::Continue(errmsg_cmd_text(line))),
        "finish" => Ok(Stmt::Finish),
        "return" => Ok(if rest.trim().is_empty() {
            Stmt::Return(None)
        } else {
            Stmt::Return(Some(parse_cmd_expr(strip_legacy_trailing_comment(rest))?))
        }),
        "throw" => Ok(Stmt::Throw(parse_cmd_expr(strip_legacy_trailing_comment(
            rest,
        ))?)),
        // `:command[!] …` defines a user command; `:delcommand` removes one.
        // (`command(`/`delcommand(` are not builtins, but guard anyway.)
        "command" if !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::CommandDef(line[cmd.len()..].trim_start().to_string()))
        }
        "delcommand" if !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::CommandDel(rest.to_string()))
        }
        // `:delf[unction][!] {name}` removes a user function. The raw remainder
        // (the run-time handler splits off a leading `!`) carries the name; a
        // `delfunction(` form is an expression call, so guard on `(`.
        "delfunction" if !line[cmd.len()..].starts_with('(') => Ok(Stmt::DelFunction(
            line[cmd.len()..].trim_start().to_string(),
        )),
        // `:autocmd`/`:augroup`/`:doautocmd` (with abbreviations).
        "autocmd" if !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::Autocmd(line[cmd.len()..].trim_start().to_string()))
        }
        "augroup" if !line[cmd.len()..].starts_with('(') => Ok(Stmt::Augroup(rest.to_string())),
        "doautocmd" | "doautoall" if !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::Doautocmd(rest.to_string()))
        }
        // `:map`-family commands (`nmap`/`inoremap`/`vunmap`/`mapclear`/`map!`).
        // The bare `map`/`unmap`/… forms collide with the `map()`/`filter()`
        // builtins, so a name immediately followed by `(` stays an expression.
        _ if is_map_command(cmd) && !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::Map(line.to_string()))
        }
        // `:colorscheme {name}` / `:colo` — the bare form (no name) is a query.
        "colorscheme" if !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::Colorscheme(rest.trim().to_string()))
        }
        // `:highlight`/`:hi` — define or link a highlight group. `:hi` on its own
        // (or `:hi {group}` with no keys) is a listing query in real vim; we keep
        // the raw args and let the runtime decide.
        "highlight" if !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::Highlight(line[cmd.len()..].trim_start().to_string()))
        }
        // `:syntax`/`:syn` and `:filetype`/`:filet` — recognized so real vimrc
        // files parse. Standalone they are no-ops (zmax highlights and detects
        // filetypes itself); an embedding editor may hook them.
        "syntax" if !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::Syntax(rest.trim().to_string()))
        }
        // `:filet` is the shortest form (`:file` is a different command).
        "filetype" if !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::Filetype(rest.trim().to_string()))
        }
        // `:normal[!] {keys}` runs normal-mode keys against the buffer, dispatched
        // by `do_excmd` at run time. Recognized even without a leading `:` so a
        // function body line like `normal! el` parses and the function defines —
        // otherwise it fell through to `parse_expr`, aborting the whole `:function`
        // and leaking its body to global scope.
        "normal" if !line[cmd.len()..].starts_with('(') => Ok(Stmt::ExCmd(line.to_string())),
        // `:echohl {group}` sets the highlight group for later `:echo` output.
        // Its argument is a group NAME, not an expression, so it can't go through
        // the `:echo` path (that would evaluate the name as a variable). Routed to
        // `do_excmd`'s `ex_echohl` handler; recognized even bare so a function body
        // line like `echohl ErrorMsg` parses instead of aborting the `:function`.
        "echohl" if !line[cmd.len()..].starts_with('(') => Ok(Stmt::ExCmd(line.to_string())),
        // Screen/session/mark commands (`:redraw[!]`, `:redir`, `:runtime`,
        // `:mark`, `:nohlsearch`) — dispatched (or no-op'd) by `do_excmd`.
        // Recognized bare so a function body line like `redraw!` or `redir => x`
        // parses instead of being mis-read as an expression and aborting the
        // `:function`. `:noh[lsearch]` (`:h :noh`) clears search highlight — a
        // no-op editor-less; recognized so a config/syntax line `nohlsearch`
        // parses instead of falling through to `parse_expr` and erroring E121.
        "redraw" | "redrawstatus" | "redrawtabline" | "redir" | "runtime" | "mark"
        | "nohlsearch"
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
        "fold" | "foldopen" | "foldclose" if !line[cmd.len()..].starts_with('(') => {
            Ok(Stmt::ExCmd(line.to_string()))
        }
        // `:edit`/`:ed` (load a file / reload) and buffer-list navigation
        // (`:bnext`, `:bprevious`, `:bfirst`, `:blast`, `:buffer`, …) — dispatched
        // by `do_excmd`. Recognized bare so a body line like `edit #` or `bnext`
        // parses instead of `parse_expr` choking on it and aborting the function.
        "edit" | "bnext" | "bprevious" | "bNext" | "bfirst" | "blast" | "buffer" | "bmodified"
        | "ball"
            if !line[cmd.len()..].starts_with('(') =>
        {
            Ok(Stmt::ExCmd(line.to_string()))
        }
        // Script-language interface commands (`:python`/`:py3`/`:ruby`/`:perl`/
        // `:lua`/`:tcl`/`:mzscheme` and their `do`/`file` variants) run an embedded
        // interpreter. Editor-less — with the interface uncompiled (`has('ruby')`
        // etc. are 0) — they are never executed, but they MUST parse as Ex commands
        // (no-op'd by `do_excmd`) so a `:ruby …` line inside a not-taken branch
        // (ftplugin/ruby.vim's `if has('ruby') && has('win32')`) does not fail to
        // parse and drop its whole enclosing `:if` block via the tolerant parser.
        _ if is_script_lang_cmd(line) => Ok(Stmt::ExCmd(line.to_string())),
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
        // A line beginning with an ASCII digit is *usually* a line-range Ex
        // command: Vim's `do_one_cmd` reads a leading number as the range's first
        // address (`1print`, `1,1fold`), so a body line like `1,1print` must
        // route to `do_excmd` instead of falling through to `parse_expr`, which
        // chokes on the trailing command word and aborts the enclosing
        // `:function`. But vimlrs's eval entrypoint also evaluates a bare
        // top-level expression, and `3 + 4` / `2 * 8` parse as *complete*
        // expressions. Prefer the expression reading whenever the whole line
        // parses as one (no trailing tokens); only a line the expression grammar
        // rejects — a range comma or a trailing command word (`1,1print`,
        // `1print`) — falls through to `do_excmd`.
        // Read as vim reads a script or command line (see [`EX_LINES`]), the
        // number is always a range.
        _ if ex_lines_active() && line.starts_with(|c: char| c.is_ascii_digit()) => {
            Ok(Stmt::ExCmd(line.to_string()))
        }
        _ if line.starts_with(|c: char| c.is_ascii_digit()) => match parse_expr(line) {
            Ok(e) => Ok(Stmt::Expr(e)),
            Err(_) => Ok(Stmt::ExCmd(line.to_string())),
        },
        // A `:`-prefixed line, a `%`-prefixed line (`%s/…`), a bang (`:!{cmd}`),
        // or a mark-address line (`'<,'>s/…`) is an Ex command with an optional
        // range — none can begin a valid expression statement, so routing them to
        // `do_excmd` is safe.
        _ if line.starts_with(':')
            || line.starts_with('!')
            || line.starts_with('\'')
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
        // In a script or command line (see [`EX_LINES`]), a line opening with a
        // range address (`$argadd f`, `.,$d`, `+3`) or a word from the command
        // table (`only`, `cd ~/`, `argglobal`, `wincmd t`) is that Ex command.
        // `word(` stays a function call.
        _ if ex_lines_active()
            && (line.starts_with(['$', '.', ',', ';', '+', '-'])
                || (cmd.starts_with(|c: char| c.is_ascii_lowercase())
                    && !line[cmd.len()..].starts_with('(')
                    && is_builtin_ex_command(cmd))) =>
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

/// The full name of the Ex command `word` abbreviates — c: `find_ex_command`'s
/// walk of `cmdnames[]` (`ex_docmd.c:3130-3137`), shared with `fullcommand()`
/// and `exists(':cmd')` through [`CMDNAMES`]: the FIRST entry that starts with
/// the word wins, so `ech` is `:echo`, `cal` is `:call`, `con` is `:continue`
/// and `co` is `:copy`.
///
/// The table is Neovim's, which has no vim9 `:var`/`:final`; their vim
/// spellings (`va`, `var`, `final`) are matched first.
///
/// RUST-PORT NOTE: a one-letter word is left alone. vim reads `e` as `:edit`
/// and `b` as `:buffer`, but this crate's expression entry point still takes
/// a one-letter line as a bare expression.
///
/// [`CMDNAMES`]: crate::ported::eval::funcs::CMDNAMES
fn ex_full_name(word: &str) -> &str {
    if word.len() < 2 {
        return word;
    }
    if word == "va" || word == "var" {
        return "var";
    }
    if word == "final" {
        return "final";
    }
    crate::ported::eval::funcs::CMDNAMES
        .iter()
        .find(|n| n.starts_with(word))
        .copied()
        .unwrap_or(word)
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
/// `Some(bang)` when the line's leading modifier run contains `silent` — `bang` is
/// true for `silent!`. (`:silent` silences the command's output; the bang also
/// silences its errors.)
fn leading_silent(mut line: &str) -> Option<bool> {
    loop {
        line = line.trim_start();
        let end = line
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(line.len());
        if end == 0 {
            return None;
        }
        let word = &line[..end];
        if !CMD_MODIFIERS.contains(&word) {
            return None;
        }
        let after = &line[end..];
        let bang = after.starts_with('!');
        if word == "silent" {
            return Some(bang);
        }
        match after.chars().next() {
            Some('!') => line = &after[1..],
            Some(c) if c.is_whitespace() => line = after,
            _ => return None,
        }
    }
}

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

/// Whether `line` invokes one of Vim's script-language interface Ex commands —
/// `:python`/`:py`, `:python3`/`:py3`, `:pythonx`/`:pyx`, `:perl`, `:ruby`,
/// `:lua`, `:tcl`, `:mzscheme`/`:mz`, and their `do`/`file` variants. The command
/// name may carry a trailing digit (`python3`, `py3`), which `cmd_word` splits
/// off, so it is matched on the leading alphanumeric run instead. A name directly
/// followed by `(` is a funcref call expression, not a command. Shared with
/// `do_excmd` so the runtime no-ops exactly the set the parser routes to `ExCmd`.
pub(crate) fn is_script_lang_cmd(line: &str) -> bool {
    let line = line.trim_start();
    let end = line
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(line.len());
    if line[end..].starts_with('(') {
        return false;
    }
    // A short name (`py`/`mz`) or a full one directly followed by an assignment
    // operator (`py = 7`, `lua += 1`) is a vim9 assignment to a same-named
    // variable, not the interface command — real Vim reassigns it. Decline so the
    // vim9-assignment arm handles it. (`=<<` heredoc-assign counts; `==`/`=~`
    // comparisons do not — a bare `cmd == …` is not a valid statement anyway.)
    let after = line[end..].trim_start();
    let is_assign = matches!(
        after.as_bytes().first(),
        Some(b'+' | b'-' | b'*' | b'/' | b'%')
    ) && after[1..].starts_with('=')
        || after.starts_with("..=")
        || after.starts_with(".=")
        || (after.starts_with('=') && !after.starts_with("==") && !after.starts_with("=~"));
    if is_assign {
        return false;
    }
    matches!(
        &line[..end],
        "python"
            | "py"
            | "pydo"
            | "pyfile"
            | "python3"
            | "py3"
            | "py3do"
            | "py3file"
            | "pythonx"
            | "pyx"
            | "pyxdo"
            | "pyxfile"
            | "perl"
            | "perldo"
            | "ruby"
            | "rubydo"
            | "rubyfile"
            | "lua"
            | "luado"
            | "luafile"
            | "tcl"
            | "tcldo"
            | "tclfile"
            | "mzscheme"
            | "mz"
            | "mzfile"
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
/// The `E5xx/E6xx: :<kw> without :<opener>: <line>` message a block terminator
/// gets when there is no block for it to close.
///
/// Each keyword has its OWN code, and they are not consecutive — measured, one
/// probe per keyword, identically in vim 9.2.0900 and nvim 0.12.4:
///
/// ```text
/// endif      E580: :endif without :if: endif
/// else       E581: :else without :if: else
/// elseif 1   E582: :elseif without :if: elseif 1
/// endwhile   E588: :endwhile without :while: endwhile
/// endfor     E588: :endfor without :for: endfor
/// endtry     E602: :endtry without :try: endtry
/// catch      E603: :catch without :try: catch
/// finally    E606: :finally without :try: finally
/// ```
///
/// The argument is the whole command line, not just the keyword. A single E580
/// for all of them — which is what this parser used to raise, with its own
/// wording — is only ever right for `:endif`.
fn e_unmatched_block(cmd: &str, line: &str) -> VimlError {
    let (code, opener) = match canon_block_kw(cmd) {
        "endif" => ("E580", ":if"),
        "else" => ("E581", ":if"),
        "elseif" => ("E582", ":if"),
        "endwhile" => ("E588", ":while"),
        "endfor" => ("E588", ":for"),
        "endtry" => ("E602", ":try"),
        "catch" => ("E603", ":try"),
        "finally" => ("E606", ":try"),
        "endfunction" => ("E193", ":function"),
        "enddef" => ("E193", ":def"),
        _ => ("E580", ":if"),
    };
    let kw = canon_block_kw(cmd);
    if kw == "endfunction" || kw == "enddef" {
        // c: `E193: %s not inside a function` (`errors.h`), which names the
        // KEYWORD rather than the opener and takes no command line.
        return VimlError::msg(format!("{code}: {kw} not inside a function"));
    }
    VimlError::msg(format!("{code}: :{kw} without {opener}: {}", line.trim()))
}

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
///
/// Every statement — top level and nested — is paired with its 1-based source
/// line. The line is not debug-only bookkeeping: the compiler writes it into
/// `fusevm::Chunk::lines`, which is what `v:throwpoint` reports and what the
/// debugger's statement markers read.
pub fn parse_program(src: &str) -> Result<Block, VimlError> {
    // Inline `rust { ... }` FFI blocks are rewritten to `call __rust_compile(...)`
    // (and their exported names recorded for builtin override) before lexing.
    let desugared = crate::rust_ffi::desugar(src);
    let src = desugared.as_str();
    let _vim9 = Vim9Guard::enter(script_is_vim9(src));
    let mut cur = Lines::new(src);
    let mut out = Vec::new();
    loop {
        cur.skip_blanks();
        let Some(line) = cur.peek() else { break };
        let (cmd, _) = cmd_word(&line);
        if is_block_terminator(cmd) {
            return Err(e_unmatched_block(cmd, &line));
        }
        let lineno = cur.line_no();
        for s in parse_one(&mut cur)? {
            out.push((lineno, s));
        }
    }
    Ok(group_by_line(out))
}

/// Parse a COMMAND LINE — the text `:execute` / `execute()` run — where end of
/// input closes any conditional still open instead of erroring (see
/// [`CMDLINE_EOF`]). Otherwise identical to [`parse_program`].
pub fn parse_cmdline(src: &str) -> Result<Block, VimlError> {
    let saved = CMDLINE_EOF.with(|f| f.replace(true));
    let r = parse_program(src);
    CMDLINE_EOF.with(|f| f.set(saved));
    r
}

/// True when end of input legitimately closes an open conditional: a command
/// line, not a sourced file (see [`CMDLINE_EOF`]).
fn eof_closes_block() -> bool {
    CMDLINE_EOF.with(|f| f.get())
}

/// The body of a `:while`/`:for` that a command line left open. It runs once,
/// because nothing reaches the `:endwhile`/`:endfor` that loops back; modelled
/// as the body followed by `:break`.
fn run_once(mut body: Block, line: u32) -> Block {
    body.push((line, Stmt::Break(String::new())));
    body
}

/// Wrap each run of statements that share a source line — i.e. the `|`-separated
/// commands of one command line — in [`Stmt::LineGroup`], so the compiler can
/// abandon the rest of the line when one of them errors, as Vim does. Runs of one
/// are left alone.
fn group_by_line(stmts: Vec<(u32, Stmt)>) -> Vec<(u32, Stmt)> {
    let mut out: Vec<(u32, Stmt)> = Vec::with_capacity(stmts.len());
    let mut i = 0;
    while i < stmts.len() {
        let line = stmts[i].0;
        let end = stmts[i..]
            .iter()
            .position(|(l, _)| *l != line)
            .map_or(stmts.len(), |n| i + n);
        if end - i > 1 {
            let group: Vec<Stmt> = stmts[i..end].iter().map(|(_, s)| s.clone()).collect();
            out.push((line, Stmt::LineGroup(group)));
        } else {
            out.push(stmts[i].clone());
        }
        i = end;
    }
    out
}

/// The result of [`parse_program_lines_tolerant`]: the top-level statements that
/// parsed (each with its 1-based source line), and `(line, message)` for each
/// statement that was skipped because it failed to parse.
pub type TolerantParse = (Vec<(u32, Stmt)>, Vec<(u32, String)>);

/// Like [`parse_program`] but error-tolerant: a top-level statement (or
/// block) that fails to parse is skipped — its first logical line is dropped and
/// parsing resumes at the next — so one unsupported construct does not abort the
/// whole file. Returns the statements that parsed, paired with `(line, message)`
/// for each skipped one. Mirrors Vim reporting an error while sourcing a script
/// and continuing; used for best-effort config sourcing (e.g. a real `.vimrc`).
pub fn parse_program_lines_tolerant(src: &str) -> TolerantParse {
    // Desugar inline `rust { ... }` FFI blocks before lexing (see
    // `parse_program`).
    let desugared = crate::rust_ffi::desugar(src);
    let src = desugared.as_str();
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
    /// Parallel to `lines`: for a segment that pass 2 cut out of a `|`-separated
    /// source line, the text from the segment's start to the end of that line
    /// (see [`CMD_TAIL`]). `None` when the logical line is the whole line.
    tails: Vec<Option<String>>,
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
            // Script-language heredoc (`:python3 << EOF … EOF`, and the same for
            // `:python`/`:perl`/`:ruby`/`:lua`/`:tcl`/`:mzscheme`). Vim's
            // `script_get()` (ex_docmd.c) sees the command arg begin with `<<`
            // and calls `heredoc_get(…, script_get=true)`, which reads every
            // subsequent line into the embedded interpreter until the end marker
            // — a missing marker defaults to `.` (vendor/eval/vars.c:791). The
            // interface is uncompiled here, so the body is never run; skip it to
            // the marker so its lines are NOT mis-parsed as vimscript. The opener
            // line itself stays as a no-op'd Ex command.
            if let Some((trim, marker)) = script_lang_heredoc_marker(raw[k]) {
                let cmd_indent: String = raw[k].chars().take_while(|c| c.is_whitespace()).collect();
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
                }
                collapsed.push((lineno, raw[k].to_string()));
                k = j;
                continue;
            }
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
        let mut tails: Vec<Option<String>> = Vec::new();
        for (lineno, text) in joined {
            let trimmed = text.trim();
            if trimmed.is_empty() || trimmed.starts_with('"') {
                lines.push((lineno, text));
                tails.push(None);
                continue;
            }
            // Commands whose argument absorbs a trailing `|` (`:autocmd`,
            // `:command`, `:normal`, `:global`/`:vglobal`) must not be bar-split:
            // the `|` is part of their text. `autocmd … exe '…' | e` is one
            // autocmd, not an autocmd followed by a stray `e` statement.
            let (lead, _) = cmd_word(strip_command_modifiers(trimmed));
            if cmd_takes_bar_arg(lead) {
                lines.push((lineno, text));
                tails.push(None);
                continue;
            }
            let segs = split_commands(&text);
            if segs.len() > 1 {
                for seg in segs {
                    if !seg.trim().is_empty() {
                        lines.push((lineno, seg.to_string()));
                        tails.push(Some(text[slice_offset(&text, seg)..].to_string()));
                    }
                }
            } else {
                lines.push((lineno, text));
                tails.push(None);
            }
        }
        Lines { lines, tails, i: 0 }
    }

    /// The text from the start of the segment at the cursor (`line`) to the end
    /// of its source line — see [`CMD_TAIL`]. A block header quotes it in a
    /// parse error: `if 1 + | echo 'a' | endif` is `E15: Invalid expression:
    /// "| echo 'a' | endif"`, because vim's scan ran into the `|`.
    fn line_tail(&self, line: &str) -> String {
        self.tails
            .get(self.i)
            .cloned()
            .flatten()
            .unwrap_or_else(|| line.to_string())
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

    /// The source line of the segment just consumed (the `:endtry`, say).
    fn prev_line_no(&self) -> u32 {
        self.lines
            .get(self.i.saturating_sub(1))
            .map(|(n, _)| *n)
            .unwrap_or(0)
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
            let tail = cur.line_tail(&line);
            cur.bump();
            Ok(vec![with_cmd_tail(&line, &tail, || parse_if(cur, rest))?])
        }
        "while" => {
            let tail = cur.line_tail(&line);
            cur.bump();
            Ok(vec![with_cmd_tail(&line, &tail, || {
                parse_while(cur, rest)
            })?])
        }
        "for" => {
            let tail = cur.line_tail(&line);
            cur.bump();
            Ok(vec![with_cmd_tail(&line, &tail, || parse_for(cur, rest))?])
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
            let tail = cur
                .tails
                .get(cur.i)
                .cloned()
                .flatten()
                .unwrap_or_else(|| line.clone());
            cur.bump();
            // Commands that absorb a trailing `|` (`:autocmd`, `:command`,
            // `:normal`, `:global`) are parsed whole — splitting them would break
            // off part of their argument (e.g. `autocmd … exe '…' | e`).
            if cmd_takes_bar_arg(cmd_word(strip_command_modifiers(line.trim())).0) {
                return Ok(vec![with_cmd_tail(&line, &tail, || parse_stmt(&line))?]);
            }
            let mut out = Vec::new();
            for seg in split_commands(&line) {
                if seg.trim().is_empty() {
                    continue;
                }
                let seg_tail = tail.get(slice_offset(&line, seg)..).unwrap_or(seg);
                out.push(with_cmd_tail(seg, seg_tail, || parse_stmt(seg))?);
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
            // `:Intercept before|after|around <pat> { code }` — AOP command-
            // intercept extension (vimlrs/zshrs-original). The advice `{ code }`
            // may contain `|`-separated statements, so the whole line is one
            // command (like a user command defined without `-bar`).
            | "Intercept"
    )
}

/// When `b[i..]` opens an interpolated string (`$'…'` or `$"…"`), the index just
/// past its closing quote (or the end of `b`). A plain quote scan misreads one:
/// in `$'{"x'y"}'` the `'` inside the `{expr}` is the expression's, not the
/// literal's, and the `"` after it is not a comment. The rules are the lexer's
/// (`lex_interp_string` / `scan_interp_expr`): `''` (or `\` in `$"…"`) escapes,
/// `{{` is a literal brace, and a `{expr}` region runs to its matching `}` with
/// nested braces counted and `'…'`/`"…"` strings skipped whole.
fn skip_interp_string(b: &[u8], i: usize) -> Option<usize> {
    let quote = *b.get(i + 1)?;
    if b[i] != b'$' || (quote != b'\'' && quote != b'"') {
        return None;
    }
    let double = quote == b'"';
    let mut p = i + 2;
    while p < b.len() {
        match b[p] {
            b'\\' if double => p += 2,
            c if c == quote => {
                if !double && b.get(p + 1) == Some(&b'\'') {
                    p += 2;
                } else {
                    return Some(p + 1);
                }
            }
            b'{' if b.get(p + 1) == Some(&b'{') => p += 2,
            b'{' => {
                let mut depth = 0u32;
                while p < b.len() {
                    match b[p] {
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        b'\'' => {
                            p += 1;
                            while p < b.len() && !(b[p] == b'\'' && b.get(p + 1) != Some(&b'\'')) {
                                p += if b[p] == b'\'' { 2 } else { 1 };
                            }
                        }
                        b'"' => {
                            p += 1;
                            while p < b.len() && b[p] != b'"' {
                                p += if b[p] == b'\\' { 2 } else { 1 };
                            }
                        }
                        _ => {}
                    }
                    p += 1;
                }
                p += 1;
            }
            _ => p += 1,
        }
    }
    Some(b.len())
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
        // `:wincmd {arg}` reads the next character as its argument, even a `|`
        // (`wincmd _ | wincmd |`), and only then looks for a separator.
        if i == start {
            if let Some(end) = wincmd_arg_end(&line[start..]) {
                i = start + end;
                continue;
            }
        }
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
        if let Some(end) = skip_interp_string(bytes, i) {
            i = end;
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

/// The byte offset just past `:[count]wincmd {arg}`'s argument when `seg`
/// is that command: vim's `ex_wincmd` takes one character (two after `g` or
/// CTRL-G) and only then calls `check_nextcmd`, so a `|` argument is not a
/// separator.
fn wincmd_arg_end(seg: &str) -> Option<usize> {
    let body = seg.trim_start_matches([' ', '\t', ':']);
    let body = body.trim_start_matches(|c: char| c.is_ascii_digit());
    let word_end = body
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(body.len());
    if !matches!(&body[..word_end], "winc" | "wincm" | "wincmd") {
        return None;
    }
    let after = &body[word_end..];
    let arg = after.trim_start_matches([' ', '\t']);
    let mut chars = arg.char_indices();
    let (_, first) = chars.next()?;
    let len = if first == 'g' || first == '\x07' {
        chars
            .next()
            .map_or(first.len_utf8(), |(i, c)| i + c.len_utf8())
    } else {
        first.len_utf8()
    };
    Some(seg.len() - arg.len() + len)
}

/// A parsed block body plus the terminator `(cmd, rest)` it stopped on
/// (`None` at EOF).
type ParseBlockResult = Result<(Block, Option<(String, String)>), VimlError>;

/// Parse statements until a terminator in `terms`. Returns the body and the
/// terminator `(cmd, rest)` it stopped on (`None` at EOF).
fn parse_block(cur: &mut Lines, terms: &[&str]) -> ParseBlockResult {
    // Collected with their source lines so a `|`-separated run inside a block body
    // is grouped exactly as it is at top level (see `group_by_line`).
    let mut stmts: Vec<(u32, Stmt)> = Vec::new();
    loop {
        cur.skip_blanks();
        let Some(line) = cur.peek() else {
            return Ok((group_by_line(stmts), None));
        };
        let (cmd, rest) = cmd_word(&line);
        // Compare (and hand back) the canonical keyword so an abbreviated
        // terminator (`endw`, `endf`, `en`, `cat`, …) closes its block.
        let ck = canon_block_kw(cmd);
        if terms.contains(&ck) {
            cur.bump();
            return Ok((
                group_by_line(stmts),
                Some((ck.to_string(), rest.to_string())),
            ));
        }
        if is_block_terminator(cmd) {
            return Err(e_unmatched_block(cmd, &line));
        }
        let lineno = cur.line_no();
        for s in parse_one(cur)? {
            stmts.push((lineno, s));
        }
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
        if let Some(end) = skip_interp_string(b, i) {
            i = end;
            continue;
        }
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
            // `@"` is the unnamed register, not a comment: the character after
            // `@` is a register name whatever it is (`call F(@")`).
            b'@' => i += 2,
            _ => i += 1,
        }
    }
    s.trim_end()
}

fn parse_if(cur: &mut Lines, cond_str: &str) -> Result<Stmt, VimlError> {
    let mut arms = Vec::new();
    let mut else_body = None;
    let (body, mut term) = parse_block(cur, IF_TERMS)?;
    arms.push((
        parse_cmd_expr(strip_legacy_trailing_comment(cond_str))?,
        body,
    ));
    loop {
        match term {
            Some((ref c, ref rest)) if c == "elseif" => {
                let cond = parse_cmd_expr(strip_legacy_trailing_comment(rest))?;
                let (b, t) = parse_block(cur, IF_TERMS)?;
                arms.push((cond, b));
                term = t;
            }
            Some((ref c, _)) if c == "else" => {
                let (b, t) = parse_block(cur, &["endif"])?;
                else_body = Some(b);
                if t.is_none() && !eof_closes_block() {
                    return Err(VimlError::msg("E171: Missing :endif"));
                }
                break;
            }
            Some((ref c, _)) if c == "endif" => break,
            None if eof_closes_block() => break,
            None => return Err(VimlError::msg("E171: Missing :endif")),
            Some((ref c, ref rest)) => return Err(e_unmatched_block(c, &format!("{c} {rest}"))),
        }
    }
    Ok(Stmt::If { arms, else_body })
}

fn parse_while(cur: &mut Lines, cond_str: &str) -> Result<Stmt, VimlError> {
    let cond = parse_cmd_expr(strip_legacy_trailing_comment(cond_str))?;
    let (body, term) = parse_block(cur, &["endwhile"])?;
    if term.is_none() {
        if eof_closes_block() {
            let body = run_once(body, cur.prev_line_no());
            return Ok(Stmt::While { cond, body });
        }
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
        // c: `skip_var_list` — the same `[a, b; rest]` shape `:let` takes.
        let (head, rest) = match inner.split_once(';') {
            Some((h, r)) => (h, Some(r.trim().to_string())),
            None => (inner, None),
        };
        ForVars::List {
            names: head
                .split(',')
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty())
                .collect(),
            rest,
        }
    } else {
        ForVars::One(var.to_string())
    };
    let iter = parse_cmd_expr(strip_legacy_trailing_comment(header[idx + 4..].trim()))?;
    let (body, term) = parse_block(cur, &["endfor"])?;
    if term.is_none() {
        if eof_closes_block() {
            let body = run_once(body, cur.prev_line_no());
            return Ok(Stmt::For { vars, iter, body });
        }
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
/// Byte length of a leading SCOPE prefix (`g:`, `b:`, `w:`, `t:`, `s:`, `l:`,
/// `a:`, `v:`), or 0.
///
/// Its colon is not a vim9 type annotation: `const g:c = [1]` declares the
/// global `g:c`, and reading the `g:` colon as `name: type` truncated the name to
/// `g` — the value landed in a variable called `g` and the intended one was never
/// defined (`islocked('g:c')` answered -1).
fn scope_prefix_len(s: &str) -> usize {
    let b = s.as_bytes();
    // A vim9 type annotation requires white space after its colon (`var s:
    // string`, vim's E1069 without it), and a scope prefix never has any
    // (`g:c`). That is the whole discriminator — without it `var s: string`
    // read `s:` as the script scope and declared nothing at all.
    let is_scope = b.len() > 2
        && b[1] == b':'
        && !b[2].is_ascii_whitespace()
        && matches!(b[0], b'g' | b'b' | b'w' | b't' | b's' | b'l' | b'a' | b'v');
    usize::from(is_scope) * 2
}

/// [`find_top_colon`] applied past any leading scope prefix.
fn find_type_colon(s: &str) -> Option<usize> {
    let skip = scope_prefix_len(s);
    find_top_colon(&s[skip..]).map(|p| p + skip)
}

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
    match find_type_colon(decl) {
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
    let Some(p) = find_type_colon(trimmed) else {
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
        .ok_or_else(|| VimlError::msg(format!("E124: Missing '(': {header}")))?;
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
        found.ok_or_else(|| {
            VimlError::msg(format!("E125: Illegal argument: {}", &header[lparen + 1..]))
        })?
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
    // c: `ex_function` reads the attributes that follow the parameter list —
    // `range`, `abort`, `dict`, `closure` (userfunc.c) — setting `FC_RANGE`,
    // `FC_ABORT`, `FC_DICT`, `FC_CLOSURE`. `dict`, `abort` and `closure` are
    // observable and are recorded; `range` is accepted and ignored (this port
    // has no `:{range}call`).
    let attrs = &header[rparen + 1..];
    let dict = attrs.split_whitespace().any(|w| w == "dict");
    let abort = attrs.split_whitespace().any(|w| w == "abort");
    let closure = attrs.split_whitespace().any(|w| w == "closure");
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
        dict,
        abort,
        closure,
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
    // The `:def` header line — already consumed by `parse_one`, so it is the
    // line *before* the cursor.
    let def_line = cur.prev_line_no();
    let header = header.trim();
    // A leading `!` (`def!`) is accepted and discarded — vim9 `def` always
    // (re)defines, so there is no bang distinction as for legacy `:function`.
    let header = header.strip_prefix('!').map_or(header, str::trim_start);
    let lparen = header
        .find('(')
        .ok_or_else(|| VimlError::msg(format!("E124: Missing '(': {header}")))?;
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
        found.ok_or_else(|| {
            VimlError::msg(format!("E125: Illegal argument: {}", &header[lparen + 1..]))
        })?
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
    // These are synthetic, so they carry the `:def` header's own line — a body
    // line number reported for one would point at a statement nobody wrote.
    let mut body: Block = Vec::with_capacity(body_stmts.len() + args.len());
    for p in &args {
        if is_plain_ident(p) {
            body.push((
                def_line,
                Stmt::Let {
                    target: LetTarget::Var(p.clone()),
                    expr: Expr::Var(format!("a:{p}")),
                },
            ));
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
        // A vim9 `:def` has no `dict` attribute — vim9 has no `self`-dict
        // functions, so `FC_DICT` is never set for one.
        dict: false,
        // c: a vim9 `:def` body always aborts on error — `ex_docmd.c:647`'s
        // reset is for legacy function lines only.
        abort: true,
        closure: false,
    })
}

fn parse_try(cur: &mut Lines) -> Result<Stmt, VimlError> {
    const TRY_TERMS: &[&str] = &["catch", "finally", "endtry"];
    // The `:try` keyword's own source line — if `:endtry` is on the same one, the
    // whole construct was a `|`-separated one-liner, which Vim handles differently
    // (an error abandons the line and is therefore NOT caught). See `Stmt::Try`.
    let open_line = cur.line_no();
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
                if t.is_none() && !eof_closes_block() {
                    return Err(VimlError::msg("E600: Missing :endtry"));
                }
                break;
            }
            Some((ref c, _)) if c == "endtry" => break,
            None if eof_closes_block() => break,
            None => return Err(VimlError::msg("E600: Missing :endtry")),
            Some((ref c, ref rest)) => return Err(e_unmatched_block(c, &format!("{c} {rest}"))),
        }
    }
    // `line_no()` now points just past `:endtry`; the construct was a one-liner if
    // nothing advanced to a new source line.
    let inline = cur.prev_line_no() == open_line;
    Ok(Stmt::Try {
        body,
        catches,
        finally,
        inline,
    })
}

/// The names of a `:let {var-name} …` listing (`list_arg_vars()`,
/// `vars.c:1210`), each paired with the expression that reads it. `None` when
/// there is nothing to list by name: no argument, a bare scope (`g:`), or an
/// argument that is not a variable name (`$ENV`, `&opt`, `@r`, `[a, b]`).
fn let_list(rest: &str) -> Option<Stmt> {
    // Split on whitespace outside brackets, so `l[0]` and `d['k']` stay whole.
    let mut names = Vec::new();
    let (mut depth, mut start, mut quote) = (0i32, None::<usize>, None::<u8>);
    for (i, &b) in rest.as_bytes().iter().enumerate() {
        match (quote, b) {
            (Some(q), _) if b == q => quote = None,
            (Some(_), _) => {}
            (None, b'\'' | b'"') => quote = Some(b),
            (None, b'[' | b'(' | b'{') => depth += 1,
            (None, b']' | b')' | b'}') => depth -= 1,
            (None, b' ' | b'\t') if depth == 0 => {
                if let Some(s) = start.take() {
                    names.push(&rest[s..i]);
                }
                continue;
            }
            _ => {}
        }
        start.get_or_insert(i);
    }
    if let Some(s) = start {
        names.push(&rest[s..]);
    }
    if names.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(names.len());
    // c: `get_name_len(&arg, &tofree, true, true)` reports
    // `E15: Invalid expression: "%s"` over the REST OF THE COMMAND LINE when
    // the argument does not start a name (`let &ts`, `let y 'str'`), and
    // `list_arg_vars` stops there with no next command.
    let invalid =
        |at: &str| Expr::ScriptError(format!("E15: Invalid expression: \"{}\"", text_to_eol(at)));
    for name in names {
        let first = *name.as_bytes().first()?;
        let bare_scope = name.len() == 2 && name.as_bytes()[1] == b':';
        if bare_scope {
            return None;
        }
        if !is_let_name_byte(first) {
            out.push((String::new(), invalid(name)));
            break;
        }
        let name_end = name
            .bytes()
            .position(|b| !is_let_name_byte(b))
            .unwrap_or(name.len());
        // `get_name_len` takes a leading digit as a name character too, and no
        // variable is called `3`.
        if first.is_ascii_digit() {
            let head = &name[..name_end];
            out.push((
                String::new(),
                Expr::ScriptError(format!("E121: Undefined variable: {head}")),
            ));
            break;
        }
        let tail = &name[name_end..];
        let subscript = tail.starts_with('[')
            || tail
                .strip_prefix('.')
                .and_then(|k| k.bytes().next())
                .is_some_and(is_let_name_byte);
        if tail.is_empty() || subscript {
            out.push((name.to_string(), parse_expr(name).ok()?));
        } else {
            // `let y z<`: `y` and `z` are listed, then the `<` is E15.
            let head = &name[..name_end];
            out.push((head.to_string(), parse_expr(head).ok()?));
            out.push((String::new(), invalid(tail)));
            break;
        }
    }
    Some(Stmt::LetList(out))
}

/// A byte `get_name_len()` takes as part of a variable name (`eval_isnamec`,
/// without the curly braces this reader does not expand).
fn is_let_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'#')
}

/// Where a `:let` target ends — `skip_var_list()` (`vendor/eval/vars.c`): a
/// `[a, b; c]` list, or one name with its `{}`/`[]` parts and `.key`s.
fn let_target_end(s: &str) -> usize {
    let b = s.as_bytes();
    let mut i = 0;
    // `&opt` / `&l:opt`, `$ENV`, `@r` — the sigil and its name.
    match b.first() {
        Some(b'@') => return 2.min(b.len()),
        Some(b'&' | b'$') => i = 1,
        _ => {}
    }
    let mut depth = 0i32;
    let mut quote = None::<u8>;
    while i < b.len() {
        let c = b[i];
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None if depth > 0 => match c {
                b'\'' | b'"' => quote = Some(c),
                b'[' | b'{' => depth += 1,
                b']' | b'}' => depth -= 1,
                _ => {}
            },
            None => match c {
                b'[' | b'{' => depth += 1,
                // `.key`, but not the `.` of `.=` / `..=`.
                b'.' if b.get(i + 1).copied().is_some_and(is_let_name_byte) => {}
                _ if is_let_name_byte(c) => {}
                _ => break,
            },
        }
        i += 1;
    }
    i
}

/// Legacy `:let`. c: `ex_let` decides between assigning and LISTING by the text
/// right after the target (`skip_var_list` + `skipwhite`): an `=`, an `op=` or
/// `..=` assigns, anything else lists — so `let y <<= 2` lists `y` and reports
/// E15 for `<<= 2`, and `let [a] < 1` is E474.
fn parse_legacy_let(rest: &str) -> Result<Stmt, VimlError> {
    let s = rest.trim_start();
    let after = s[let_target_end(s)..].trim_start().as_bytes();
    let assigns = match after {
        [b'=', ..] | [b'.', b'.', b'=', ..] => true,
        [op, b'=', ..] => b"+-*/%.".contains(op),
        _ => false,
    };
    if assigns {
        return parse_let(rest);
    }
    if s.starts_with('[') {
        return Ok(Stmt::Expr(Expr::ScriptError(
            "E474: Invalid argument".to_string(),
        )));
    }
    // `:let` alone and the whole-scope forms (`:let g:`) list a hashtable in its
    // iteration order, which is not modelled — those stay a no-op.
    Ok(let_list(strip_legacy_trailing_comment(rest)).unwrap_or(Stmt::Expr(Expr::Number(0))))
}

fn parse_let(rest: &str) -> Result<Stmt, VimlError> {
    let Some(eq) = rest.find('=') else {
        // No `=`: `:let {var-name} …` lists the named variables. `:let` alone
        // and the whole-scope forms (`:let g:`) list a hashtable in its
        // iteration order, which is not modelled — those stay a no-op, as does
        // anything this reader does not recognise as a variable name.
        return Ok(
            let_list(strip_legacy_trailing_comment(rest)).unwrap_or(Stmt::Expr(Expr::Number(0)))
        );
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
        let names = split_top_commas(head)
            .into_iter()
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .collect();
        LetTarget::List {
            names,
            rest: rest_name,
        }
    } else {
        let_target(lhs, rest)?
    };
    // Plain `=` stores the RHS directly; `op=` desugars to `target = target op rhs`
    // (`tv_op` semantics), reusing the same store path so it stays JIT-eligible.
    let rhs = strip_legacy_trailing_comment(rhs);
    let expr = match op {
        None => parse_cmd_expr(rhs)?,
        Some(op) => {
            let cur = let_target_expr(&target)?;
            let rhs = parse_cmd_expr(rhs)?;
            // c: `ex_let` evaluates `{expr}` BEFORE it reads the target, so an
            // expression that does not parse is the only error
            // (`let nosuch += * 2` is E15, not E121).
            if let Expr::ScriptError(_) = rhs {
                return Ok(Stmt::Let { target, expr: rhs });
            }
            Expr::Arith {
                op,
                lhs: Box::new(cur),
                rhs: Box::new(rhs),
                // c: `ex_let_one` applies the operator with `eexe_mod_op`, not with
                // the expression operator — see `Expr::Arith::mod_op`.
                mod_op: true,
            }
        }
    };
    Ok(Stmt::Let { target, expr })
}

/// The target of a `:let` that is not a list: `&opt`, `$ENV`, `@r`,
/// `base[idx]`, `base[a:b]`, `base.key`, or a variable. `rest` is the `:let`
/// argument from the target on, which subscripted targets quote in E741.
/// c: `ex_let_one()`, also called for each item of `let [a, b] = …`.
pub(crate) fn let_target(lhs: &str, rest: &str) -> Result<LetTarget, VimlError> {
    Ok(if let Some(name) = lhs.strip_prefix('&') {
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
                    src: Some(text_to_eol(rest)),
                }
            }
            None => LetTarget::Index {
                base: Box::new(parse_expr(base_src)?),
                index: Box::new(parse_expr(index_src)?),
                src: Some(text_to_eol(rest)),
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
            src: Some(text_to_eol(rest)),
        }
    } else {
        LetTarget::Var(lhs.to_string())
    })
}

/// Split a `:unlet` argument list on top-level whitespace, keeping `[…]`
/// subscripts and quoted strings (e.g. `d['a b']`) intact. A plain
/// `split_whitespace()` would wrongly break `unlet d['a b']` in two.
/// `:lockvar`/`:unlockvar` argument text → [`Stmt::LockVar`].
///
/// Only the `!` is split off here; the depth digits and the name list are left
/// for the ported `ex_lockvar`, which already implements both.
fn parse_lockvar(rest: &str, lock: bool) -> Result<Stmt, VimlError> {
    let t = rest.trim_start();
    let bang = t.starts_with('!');
    let arg = t.trim_start_matches('!').trim().to_string();
    if arg.is_empty() {
        return Err(VimlError::msg("E471: Argument required"));
    }
    Ok(Stmt::LockVar { arg, bang, lock })
}

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

/// If `line` invokes a plain script-language interface command whose argument
/// begins with `<<`, return `(trim, marker)` for the heredoc that follows.
/// Mirrors Vim's `script_get()` (ex_docmd.c): only the bare interpreter
/// commands (`:python`/`:py`, `:python3`/`:py3`, `:pythonx`/`:pyx`, `:perl`,
/// `:ruby`, `:lua`, `:tcl`, `:mzscheme`/`:mz`) read an embedded script — the
/// `do`/`file` variants take an expression / filename, not a heredoc. The arg
/// (leading whitespace skipped) must start with `<<`; after it, `heredoc_get`
/// (vendor/eval/vars.c) reads the optional `trim`/`eval` words then the marker
/// word, defaulting to `.` when the marker is missing (script_get case).
fn script_lang_heredoc_marker(line: &str) -> Option<(bool, String)> {
    // The interpreter name may carry a trailing digit (`python3`, `py3`), which
    // `cmd_word` (ASCII-letter split) would truncate — split on the alphanumeric
    // run instead, matching `is_script_lang_cmd`.
    let line = line.trim_start();
    let end = line
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(line.len());
    let (cmd, rest) = (&line[..end], line[end..].trim_start());
    if !matches!(
        cmd,
        "python"
            | "py"
            | "python3"
            | "py3"
            | "pythonx"
            | "pyx"
            | "perl"
            | "ruby"
            | "lua"
            | "tcl"
            | "mzscheme"
            | "mz"
    ) {
        return None;
    }
    let mut r = rest.strip_prefix("<<")?.trim_start();
    let mut trim = false;
    loop {
        let kw = |s: &str, w: &str| -> bool {
            s.strip_prefix(w)
                .is_some_and(|t| t.is_empty() || t.starts_with(char::is_whitespace))
        };
        if kw(r, "trim") {
            trim = true;
            r = r[4..].trim_start();
        } else if kw(r, "eval") {
            r = r[4..].trim_start();
        } else {
            break;
        }
    }
    // The marker is the next word; a missing or comment-only remainder means the
    // `.` default (only reached for script_get, which this always is).
    let marker = match r.split_whitespace().next() {
        Some(m) if !m.starts_with('"') => m.to_string(),
        _ => ".".to_string(),
    };
    Some((trim, marker))
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
        // A top-level `:` makes it a range, `unlet l[i:j]`.
        let base = Box::new(parse_expr(base_src)?);
        return Ok(match split_top_colon(index_src) {
            Some((a, b)) => {
                let parse_opt = |s: &str| -> Result<Option<Box<Expr>>, VimlError> {
                    let s = s.trim();
                    Ok(if s.is_empty() {
                        None
                    } else {
                        Some(Box::new(parse_expr(s)?))
                    })
                };
                UnletArg::Range {
                    base,
                    idx1: parse_opt(a)?,
                    idx2: parse_opt(b)?,
                    src: arg.to_string(),
                }
            }
            None => UnletArg::Item {
                base,
                index: Box::new(parse_expr(index_src)?),
                src: arg.to_string(),
            },
        });
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
            src: arg.to_string(),
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
        LetTarget::Index { base, index, .. } => Expr::Index {
            base: base.clone(),
            index: index.clone(),
        },
        LetTarget::List { .. } | LetTarget::Range { .. } => {
            return Err(VimlError::msg("E734: Wrong variable type for +="))
        }
    })
}

/// The `{expr}` of a `:let`, `:return`, `:throw`, `:eval`, and the condition or
/// list of `:if`/`:elseif`/`:while`/`:for`. c: each runs `eval0()` (or
/// `eval_to_bool()`) on it when the command EXECUTES, so text that does not parse is an error at run time, not a reason
/// to reject the whole script: `let x = 1 +` reports `E15: Invalid expression:
/// "1 +"` (eval0's own message, over the whole expression to the end of the
/// line), `let x = (1` the level's `E110`, `let x = 1 2` `E488: Trailing
/// characters: 2` — and the variable is not assigned. The error is an
/// [`Expr::ScriptError`]: the `:let` store's failure guard skips the
/// assignment, and a failed `:if`/`:while`/`:for` header skips the construct,
/// exactly as a run-time error there does. In code that is not executed (a
/// false `:if` branch) nothing is reported, as vim's `emsg_skip` has it.
///
/// A curly-brace name still fails the parse, for the reason given at
/// [`Parser::operand_invexpr`].
fn parse_cmd_expr(src: &str) -> Result<Expr, VimlError> {
    parse_cmd_expr_for(src, false)
}

/// [`parse_cmd_expr`], with `call_cmd` set when `src` is the argument of `:call`
/// (see [`Parser::call_cmd`]).
fn parse_cmd_expr_for(src: &str, call_cmd: bool) -> Result<Expr, VimlError> {
    let (toks, _, lex_err) = crate::viml_lexer::lex_prefix(src);
    let mut p = Parser::new(toks, src);
    p.call_cmd = call_cmd;
    p.lex_err = lex_err;
    let e = match p.eval1() {
        Ok(e) => p.guard_deferred(e),
        Err(err) => {
            return match p.operand_invexpr(src, &err, 0) {
                Some(msg) => Ok(Expr::ScriptError(msg)),
                None => Err(err),
            }
        }
    };
    if matches!(p.peek(), Tok::Eof) && p.lex_err.is_none() {
        return Ok(e);
    }
    // c: `eval0`: `if (!ends_excmd(*p)) semsg(_(e_trailing_arg), p)` — the
    // expression is not evaluated, so its own errors never show. Text the lexer
    // could not read (`1 ~ 2`, `1 'abc`) is trailing text all the same.
    let at = p.toks.get(p.i).map_or(src.len(), |t| t.span);
    Ok(Expr::ScriptError(format!(
        "E488: Trailing characters: {}",
        text_to_eol(&src[at..])
    )))
}

fn parse_expr_list(src: &str) -> Result<Vec<Expr>, VimlError> {
    if src.trim().is_empty() {
        return Ok(Vec::new());
    }
    // Lex only as far as the text tokenizes: an argument that does not lex (an
    // unterminated string) is reached and reported in its turn, after the
    // arguments ahead of it ran — `echo 1 printf('%E')'` prints `1`, the E766,
    // and only then `E115: Missing single quote`.
    let (toks, _, lex_err) = crate::viml_lexer::lex_prefix(src);
    let mut p = Parser::new(toks, src);
    p.lex_err = lex_err;
    let mut out = Vec::new();
    loop {
        if matches!(p.peek(), Tok::Eof) {
            if let Some(err) = &p.lex_err {
                // An argument the lexer stopped on: its E15 quotes the text to
                // the end of the line, past any `|` (`echo 1 ~ 2 | echo 3`).
                let at = p.toks.get(p.i).map_or(src.len(), |t| t.span);
                let msg = match p.operand_invexpr(src, err, at) {
                    Some(m) if err.0.starts_with("E15: ") => m,
                    _ => err.0.clone(),
                };
                out.push(Expr::ScriptError(msg));
            }
            break;
        }
        let arg_at = p.peek_span().unwrap_or(src.len());
        let e = match p.eval1() {
            Ok(e) => e,
            Err(err) => match p.operand_invexpr(src, &err, arg_at) {
                // c (`ex_echo`, `ex_execute`): each argument is parsed AND
                // evaluated before the next is read, so the arguments ahead of
                // a malformed one are already printed when its E15 reports, and
                // the error aborts the rest of the command line. A runtime
                // error operand reproduces that ordering.
                Some(msg) => {
                    out.push(Expr::ScriptError(msg));
                    return Ok(out);
                }
                None => return Err(err),
            },
        };
        out.push(p.guard_deferred(e));
    }
    Ok(out)
}

/// Parse a single expression string into an [`Expr`].
/// Parse the *leading* expression of `src` and report where it stopped (a byte
/// offset into `src`).
///
/// `eval()` needs this: the C's `f_eval` runs `eval1()` on the string, **evaluates
/// the result**, and only then complains about whatever is left over
/// (`E488: Trailing characters`). Parsing the whole string up front instead
/// reported a parse error (E15) for text Vim would have evaluated first —
/// `eval("nl\nhere")` is E121 (undefined variable `nl`) in Vim, not E15.
pub fn parse_expr_prefix(src: &str) -> Result<(Expr, usize), VimlError> {
    // Lex only as far as the source tokenizes: text past the leading expression
    // is `eval()`'s business (E488 trailing / never reached), not a lex error —
    // `eval("a'quote")` is the variable `a` (E121 when undefined), and
    // `eval("tab\\there")` is the variable `tab`, exactly as the C `eval1()`
    // stops after one expression (verified against vim 9.2 / nvim 0.12).
    let (toks, lex_stop, lex_err) = crate::viml_lexer::lex_prefix(src);
    let mut p = Parser::new(toks, src);
    p.lex_err = lex_err;
    let e = p.eval1()?;
    let e = p.guard_deferred(e);
    // c: `eval1()` leaves `*arg` just past the last character it consumed — a
    // level that looks ahead for an operator and finds none does not advance
    // over the blanks it skipped — so the leftover text keeps its leading
    // whitespace (`eval('1 2')` is `E488: Trailing characters:  2`).
    let rest_at = p.i.checked_sub(1).map_or(lex_stop, |last| p.toks[last].end);
    Ok((e, rest_at))
}

pub fn parse_expr(src: &str) -> Result<Expr, VimlError> {
    let toks = lex(src)?;
    let mut p = Parser::new(toks, src);
    let e = p.eval1()?;
    let e = p.guard_deferred(e);
    if !matches!(p.peek(), Tok::Eof) {
        // c: `E15: Invalid expression: "%s"` over the text still unread
        // (`vendor/eval.c:383`), not a description of the parser's state.
        return Err(p.invexpr());
    }
    Ok(e)
}

impl Parser {
    /// Byte offset of the current token, or `None` at `Eof`.
    fn peek_span(&self) -> Option<usize> {
        let t = self.toks.get(self.i)?;
        (!matches!(t.kind, Tok::Eof)).then_some(t.span)
    }

    /// The source text from the current token to the end.
    ///
    /// Every one of vim's parse diagnostics that takes an argument quotes
    /// exactly this — the text still unread at the point the parser gave up.
    /// `{'a' 1}` is `E720: Missing colon in Dictionary: 1}`, `[1 2]` is
    /// `E696: Missing comma in List: 2]`, and running out of input gives the
    /// same messages with an EMPTY argument (`E696: Missing comma in List: `),
    /// trailing space and all. A `Debug`-formatted token is not a substitute:
    /// it is Rust's name for the token, not vim's view of the source.
    fn rest(&self) -> &str {
        let at = self.peek_span().unwrap_or(self.src.len());
        self.src.get(at..).unwrap_or("")
    }

    /// The source from `at` to the end of the expression buffer — the text a
    /// `%s`-over-a-source-pointer diagnostic carries. See [`Parser::clip`].
    fn src_from(&self, at: usize) -> String {
        let end = self.clip.last().copied().unwrap_or(self.src.len());
        self.src.get(at..end.max(at)).unwrap_or("").to_string()
    }

    /// The span of the `}` that closes the lambda body starting at the current
    /// token, found in the TOKEN stream before the body is parsed so every
    /// diagnostic built inside it can be clipped as it is built.
    fn lambda_body_end(&self) -> Option<usize> {
        let mut depth = 0u32;
        for t in &self.toks[self.i..] {
            match t.kind {
                Tok::LBrace | Tok::HashBrace => depth += 1,
                Tok::RBrace if depth == 0 => return Some(t.span),
                Tok::RBrace => depth -= 1,
                _ => {}
            }
        }
        None
    }

    /// `E15: Invalid expression: "%s"` over [`Self::rest`] — `e_invexpr2`
    /// (Neovim `errors.h:37`), quotes included.
    ///
    /// At end of input this is [`VimlError::silent`] instead, because the C only
    /// reports when there is something left to point at (`vendor/eval.c:5604`)
    /// and leaves the empty case to the caller's own E15 over the whole
    /// expression. That is why `eval('1 +')` prints ONE E15 and `eval(']')`
    /// prints two.
    fn invexpr(&self) -> VimlError {
        let rest = self.rest();
        if rest.is_empty() {
            // Out of tokens with an operand still expected: if the token stream
            // ended because the LEXER gave up (an unterminated quote), that is
            // the diagnostic vim reports here.
            return self.lex_err.clone().unwrap_or_else(VimlError::silent);
        }
        VimlError::msg(format!("E15: Invalid expression: \"{rest}\""))
    }

    /// The diagnostic an `:echo`/`:execute` argument that failed to PARSE
    /// reports at run time, or `None` to leave it a parse failure.
    ///
    /// c (`ex_echo`, `ex_execute`): each argument is parsed and evaluated before
    /// the next is read, so a malformed one reports when execution reaches it —
    /// after the arguments ahead of it printed — and abandons the rest of the
    /// line. Its message is whatever the failing level said (`E110`, `E696`,
    /// `E697`, `E15` over the unread text), or, when that level said nothing,
    /// `ex_echo`'s own `semsg(_(e_invexpr2), p)` with `p` where THIS argument
    /// began (`echo 'a' 1 +` is `E15: Invalid expression: "1 +"`).
    ///
    /// The quoted text runs to the end of the LINE, because vim never cut it at
    /// the `|`: `echo 1 + * 2 | echo 3` is `"* 2 | echo 3"`, and a parse that
    /// ran out at the `|` is vim's scan stopping ON it (`"| echo 3"`).
    ///
    /// `None` for an argument holding a curly-brace name (`g:a_{x}`): this
    /// parser cannot read those yet, and reporting its failure would put an
    /// error on valid vim source — the reason `source_tolerant` keeps parse
    /// errors quiet at all.
    ///
    /// `orig` is the caller's slice of the command line (the same text as
    /// `self.src`): `text_to_eol` can only see past a `|` from a pointer into
    /// the line itself, not from this parser's owned copy.
    fn operand_invexpr(&self, orig: &str, err: &VimlError, arg_at: usize) -> Option<String> {
        let arg = orig.get(arg_at..).unwrap_or("");
        let b = arg.as_bytes();
        let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b':';
        let curly = (1..b.len())
            .any(|k| (b[k] == b'{' && ident(b[k - 1])) || (b[k - 1] == b'}' && ident(b[k])));
        if curly {
            return None;
        }
        // The current token's span: at the `Eof` the lexer stopped on, that is
        // where the text it could not read begins (`~ 3`), not the end.
        let at = self.toks.get(self.i).map_or(orig.len(), |t| t.span);
        let rest = orig.get(at..).unwrap_or("");
        if err.is_silent() {
            let past = text_to_eol(orig.get(orig.len()..).unwrap_or(""));
            let quoted = if rest.is_empty() && !past.is_empty() {
                past
            } else {
                text_to_eol(arg)
            };
            return (!quoted.is_empty()).then(|| format!("E15: Invalid expression: \"{quoted}\""));
        }
        if err.0.starts_with("E15: Invalid expression: \"") && !rest.is_empty() {
            return Some(format!(
                "E15: Invalid expression: \"{}\"",
                text_to_eol(rest)
            ));
        }
        Some(err.0.clone())
    }

    /// Consume `want`, or fail with vim's own message for that construct —
    /// `msg` with a single `%s` receiving [`Self::rest`].
    fn eat_or(&mut self, want: &Tok, msg: &str) -> Result<(), VimlError> {
        if self.peek() == want {
            self.advance();
            return Ok(());
        }
        Err(VimlError::msg(msg.replace("%s", self.rest())))
    }
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
    /// Deferred E15s produced by [`Self::resplit_float_token`] during this
    /// parse, in source order. Vim's `eval_number()` FAIL is a *parse* failure
    /// that aborts `eval1` at the junk's position: everything textually after
    /// it is dead, and the expression fails even when the junk sits in a
    /// ternary/`&&`/`||` branch evaluation never reaches. The ternary and
    /// short-circuit parse levels check for new entries after parsing an
    /// operand (see `eval1`/`eval2`/`eval3`), and the expression roots wrap
    /// the tree in [`Expr::ScriptErrorGuard`] as a last resort.
    deferred_e15: Vec<String>,
    /// The lex error that ended a `lex_prefix` scan, if any. Reported only when
    /// the parser runs out of tokens with an operand still expected — see
    /// `lex_prefix` and [`Self::invexpr`].
    lex_err: Option<VimlError>,
    /// Where the expression buffer being parsed ENDS, innermost last.
    ///
    /// c: `get_lambda_tv` (`Src/nvim/eval/userfunc.c`) stores the lambda's BODY
    /// TEXT for the function it generates, so a diagnostic raised inside a
    /// lambda quotes only up to that body's end — while the same call written
    /// outside one quotes to the end of the enclosing eval buffer. That is
    /// visible in `E116`, whose text is `%s` over a pointer INTO the source
    /// (see `Expr::Call::emsg_name`):
    ///
    /// ```vim
    /// echo map([1], {i,v -> strlen([1] . '')})
    /// " E116: Invalid arguments for function strlen([1] . '')
    /// ```
    ///
    /// — no trailing `})`. Empty at the top level, where the buffer ends with
    /// the source.
    clip: Vec<usize>,
    /// Parsing the argument of `:call`, whose OUTERMOST function name
    /// `trans_function_name` hands `get_func_tv` as an allocated, NUL-terminated
    /// copy — so its E116/E740 names the function alone, where the same call in
    /// an expression quotes the source from the name on.
    call_cmd: bool,
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
            deferred_e15: Vec::new(),
            lex_err: None,
            clip: Vec::new(),
            call_cmd: false,
        }
    }

    /// Wrap a completely parsed expression in [`Expr::ScriptErrorGuard`] when a
    /// re-split float left a deferred E15 anywhere in it (see the field doc).
    /// Redundant when a ternary/`&&`/`||` level already handled the junk (the
    /// raise yields to any earlier error), but guarantees the expression can
    /// never silently succeed, as Vim's parse failure never does.
    fn guard_deferred(&mut self, e: Expr) -> Expr {
        if self.deferred_e15.is_empty() {
            return e;
        }
        let msg = std::mem::take(&mut self.deferred_e15).swap_remove(0);
        Expr::ScriptErrorGuard {
            inner: Box::new(e),
            msg,
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

    /// Port of the `want_string` arm of `eval_number()` (`Src/eval.c:3434`):
    /// after a `.`/`..` concat, "Don't look for a float after the '.' operator"
    /// — the "." belongs to concatenation. The lexer tokenized context-free, so
    /// the Float token at the cursor is split back into its leading integer
    /// (with `vim_str2nr`'s octal rule) and the re-lexed remainder: `1.5` →
    /// `1` `.` `5`, and `1.0e300` → `1` `.` `0` `e300` (an E15 downstream,
    /// exactly as Vim reports for `'a' .. -1.0e300`).
    fn resplit_float_token(&mut self) {
        let (span, end) = (self.toks[self.i].span, self.toks[self.i].end);
        let Some(text) = self.src.get(span..end) else {
            return;
        };
        let digits = text.bytes().take_while(|b| b.is_ascii_digit()).count();
        if digits == 0 || digits >= text.len() {
            return;
        }
        let int_text = &text[..digits];
        // Same integer rules as the lexer: octal for an all-octal 0-leading
        // literal, saturating at VARNUMBER_MAX.
        let n = if int_text.len() > 1
            && int_text.starts_with('0')
            && int_text.bytes().all(|b| (b'0'..=b'7').contains(&b))
        {
            i64::from_str_radix(int_text, 8).unwrap_or(i64::MAX)
        } else {
            int_text.parse::<i64>().unwrap_or(i64::MAX)
        };
        let rem_start = span + digits;
        let mut spliced: Vec<Token> = Vec::new();
        if let Some(rem_src) = self.src.get(rem_start..end) {
            if let Ok(rem) = lex(rem_src) {
                for mut t in rem {
                    if t.kind == Tok::Eof {
                        break;
                    }
                    t.span += rem_start;
                    t.end += rem_start;
                    spliced.push(t);
                }
            }
        }
        // A remainder with an exponent (`.0e300`) does not re-lex into a plain
        // `.` + fraction — Vim stops scanning at the `e` and reports E15, but
        // only at RUN time and only after the operands to its left evaluated
        // (a List LHS is E730 first). Replace everything past the `.` with a
        // deferred-error operand carrying Vim's message.
        if spliced.len() > 2 {
            let frac_start = rem_start + 1; // just past the '.'
            let tail = self.src.get(frac_start..).unwrap_or("").to_string();
            // c: `e_invexpr2` is `E15: Invalid expression: "%s"` — with QUOTES.
            let msg = format!("E15: Invalid expression: \"{tail}\"");
            // Record the parse-level failure for the enclosing ternary/`&&`/
            // `||` levels and the expression root: Vim's eval_number() FAIL
            // fails the whole eval1 even in a branch that is never evaluated
            // (`(1 ? 350.0 : (-2147483648 .. 1.0e308))` is E15 in both
            // oracles, not 350.0).
            // The message pushed here is the one the SKIPPED positions want —
            // see `whole_expr_e15`. The token below keeps the position-specific
            // one, which is what an evaluated position reports.
            self.deferred_e15.push(self.whole_expr_e15());
            spliced.truncate(1); // keep the '.'
            spliced.push(Token {
                kind: Tok::DeferredErr(msg),
                span: frac_start,
                end,
            });
        }
        self.toks[self.i] = Token {
            kind: Tok::Number(n),
            span,
            end: rem_start,
        };
        for (k, t) in spliced.into_iter().enumerate() {
            self.toks.insert(self.i + 1 + k, t);
        }
    }

    /// `E15: Invalid expression: "%s"` over the WHOLE expression — `eval0`'s
    /// own fallback (`vendor/eval.c`, `eval0_retarg`):
    ///
    /// ```c
    /// if (!aborting() && did_emsg == did_emsg_before
    ///                 && called_emsg == called_emsg_before)
    ///     semsg(_(e_invalid_expression_str), arg);
    /// ```
    ///
    /// `arg` is where eval0 STARTED, not where the failure was. It fires only
    /// when the failing level emitted nothing itself, and `eval_number()`'s own
    /// `semsg` is guarded by `if (evaluate)` — so a number-literal failure in a
    /// branch evaluation never reaches reports the whole expression, while the
    /// same failure in an evaluated branch reports from the literal onwards:
    ///
    /// ```text
    /// echo 1 ? 12abc : 3      E15: Invalid expression: "12abc : 3"
    /// echo 0 ? 12abc : 3      E15: Invalid expression: "0 ? 12abc : 3"
    /// echo 0 && 12abc         E15: Invalid expression: "0 && 12abc"
    /// echo 0 ? 'a' . 1.0e300 : 3   E15: Invalid expression: "0 ? 'a' . 1.0e300 : 3"
    /// ```
    fn whole_expr_e15(&self) -> String {
        format!("E15: Invalid expression: \"{}\"", self.src)
    }

    /// Port of `vim_str2nr()`'s `strict` reject (`vendor/charset.c:1368`):
    ///
    /// ```c
    /// // Check for an alphanumeric character immediately following, that is
    /// // most likely a typo.
    /// if (strict && ptr - start != maxlen && ASCII_ISALNUM(*ptr)) {
    ///   return;   // leaves *len == 0
    /// }
    /// ```
    ///
    /// `eval_number()` passes `strict = TRUE` and turns that zero length into
    /// `semsg(_(e_invalid_expression_str), *arg)` + FAIL, so a number with a
    /// letter or digit glued to it is not "a number then a name" — it is a
    /// parse failure at the number:
    ///
    /// ```text
    ///     echo 12abc      vim  E15: Invalid expression: "12abc"
    ///     echo 1e5        vim  E15: Invalid expression: "1e5"
    ///     echo 0xg        vim  E15: Invalid expression: "0xg"
    ///     echo 007a       vim  E15: Invalid expression: "007a"
    /// ```
    ///
    /// while `_` is not alphanumeric and so still terminates the literal
    /// normally (`echo 1_000` prints `1`, then E121 for `_000`).
    ///
    /// Without this the port read `12abc` as `12` followed by the name `abc`
    /// and answered E121, and `1e5`/`1e-10` as `1` followed by `e5`/`e`.
    ///
    /// The failure is modelled with the same deferred machinery as a re-split
    /// float (see [`Self::resplit_float_token`]) because it is the same shape:
    /// it aborts `eval1` from the literal's position, so the text after it is
    /// dead, and it fails the expression even inside a branch that is never
    /// evaluated.
    fn check_number_junk(&mut self) {
        if !matches!(self.peek(), Tok::Number(_)) {
            return;
        }
        let (span, end) = (self.toks[self.i].span, self.toks[self.i].end);
        if !self
            .src
            .as_bytes()
            .get(end)
            .is_some_and(u8::is_ascii_alphanumeric)
        {
            return;
        }
        let tail = self.src.get(span..).unwrap_or("").to_string();
        self.deferred_e15.push(self.whole_expr_e15());
        self.toks[self.i].kind = Tok::DeferredErr(format!("E15: Invalid expression: \"{tail}\""));
        // The glued run lexed as its own Ident/Number token(s) (`12abc` is
        // `12` + `abc`). Vim never sees them — `eval_number` FAILed on the
        // whole literal — so drop everything the run covers, and only that:
        // a bracket or operator after it still has to close its enclosing
        // list/call (`[1, 12abc]` reports E15, not "missing ]").
        let run_end = span
            + self.src[span..]
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric())
                .count();
        while self.i + 1 < self.toks.len() {
            let t = &self.toks[self.i + 1];
            // `Tok::Eof` sits at the end of the source and so is inside the run
            // whenever the run reaches it; removing it leaves `advance()` stuck
            // on the last token and the parse never terminates.
            if t.kind == Tok::Eof || t.span < end || t.end > run_end {
                break;
            }
            self.toks.remove(self.i + 1);
        }
    }

    fn eat(&mut self, want: &Tok) -> Result<(), VimlError> {
        if self.peek() == want {
            self.advance();
            Ok(())
        } else {
            Err(self.invexpr())
        }
    }

    /// `{cond} ? {then}` with no `:` — c: `eval1()` (eval.c) parses the
    /// then-branch, *then* checks for the colon and `emsg(_(e_missing_colon_
    /// after_questionmark))`. So E109 is a **run-time** error tagged with the
    /// ex-command (`Vim(echo):E109: …`), not a parse failure, and the truthy
    /// branch's side effects have already happened when it fires. Verified
    /// against vim 9.2 with a counter-bumping function:
    ///
    /// ```text
    /// try | echo 1 ? Bump() | catch | endtry   " Vim(echo):E109: …, g:n == 1
    /// try | echo 0 ? Bump() | catch | endtry   " Vim(echo):E109: …, g:n == 1
    /// ```
    ///
    /// [`Expr::ScriptErrorGuard`] is exactly that "evaluate, discard, raise"
    /// shape, so the truthy arm gets one wrapped around the parsed then-branch
    /// and the falsy arm gets the bare [`Expr::ScriptError`] — no side effect
    /// on a branch vim never evaluates.
    ///
    /// Returning a node rather than `Err` matters beyond the message: a parse
    /// error here is swallowed whole by the tolerant source path
    /// (`fusevm_bridge::source_tolerant`), which is why vimlrs used to accept
    /// `echo 1 ? 'a'` *silently* — more permissive than the reference.
    fn missing_colon(cond: Expr, then: Expr) -> Expr {
        const E109: &str = "E109: Missing ':' after '?'";
        Expr::Ternary {
            cond: Box::new(cond),
            then: Box::new(Expr::ScriptErrorGuard {
                inner: Box::new(then),
                msg: E109.to_string(),
            }),
            otherwise: Box::new(Expr::ScriptError(E109.to_string())),
        }
    }

    fn eval1(&mut self) -> Result<Expr, VimlError> {
        let cond = self.eval2()?;
        match self.peek() {
            Tok::Question => {
                self.advance();
                let junk_before_then = self.deferred_e15.len();
                let then = self.eval1()?;
                let then_junk = self.deferred_e15.len() > junk_before_then;
                if self.peek() != &Tok::Colon {
                    return Ok(Self::missing_colon(cond, then));
                }
                self.advance();
                let junk_before_else = self.deferred_e15.len();
                let otherwise = self.eval1()?;
                let else_junk = self.deferred_e15.len() > junk_before_else;
                // c: eval1 parses BOTH branches left to right, and a re-split
                // float's junk is a parse FAILURE at its position: junk in the
                // then-branch kills the else-branch as dead text (`(0 ? junk :
                // matchbufline(0x1f,…))` is E15 in both oracles — matchbufline
                // never runs); junk in the else-branch reports only after the
                // then-branch evaluated.
                let (then, otherwise) = if then_junk {
                    let msg = self.deferred_e15[junk_before_then].clone();
                    (then, Expr::ScriptError(msg))
                } else if else_junk {
                    let msg = self.deferred_e15[junk_before_else].clone();
                    (
                        Expr::ScriptErrorGuard {
                            inner: Box::new(then),
                            msg,
                        },
                        otherwise,
                    )
                } else {
                    (then, otherwise)
                };
                Ok(Expr::Ternary {
                    cond: Box::new(cond),
                    then: Box::new(then),
                    otherwise: Box::new(otherwise),
                })
            }
            Tok::QuestionQuestion => {
                self.advance();
                let junk_before = self.deferred_e15.len();
                let rhs = self.eval1()?;
                let node = Expr::Coalesce(Box::new(cond), Box::new(rhs));
                Ok(self.guard_skipped_junk(node, junk_before))
            }
            _ => Ok(cond),
        }
    }

    /// A short-circuit node whose right operand grew junk while parsing: in
    /// Vim the junk is a parse failure at its position, so it reports right
    /// after this node even when short-circuiting skipped the operand
    /// (`((0 && junk) + f())` is E15 before `f()` ever runs). When the operand
    /// DID evaluate, its inline [`Expr::ScriptError`] already raised and this
    /// wrapper's raise yields to it.
    fn guard_skipped_junk(&mut self, node: Expr, junk_before: usize) -> Expr {
        if self.deferred_e15.len() <= junk_before {
            return node;
        }
        let msg = self.deferred_e15[junk_before].clone();
        Expr::ScriptErrorGuard {
            inner: Box::new(node),
            msg,
        }
    }

    fn eval2(&mut self) -> Result<Expr, VimlError> {
        let mut lhs = self.eval3()?;
        while matches!(self.peek(), Tok::OrOr) {
            self.advance();
            let junk_before = self.deferred_e15.len();
            let rhs = self.eval3()?;
            let node = Expr::Or(Box::new(lhs), Box::new(rhs));
            lhs = self.guard_skipped_junk(node, junk_before);
        }
        Ok(lhs)
    }

    fn eval3(&mut self) -> Result<Expr, VimlError> {
        let mut lhs = self.eval4()?;
        while matches!(self.peek(), Tok::AndAnd) {
            self.advance();
            let junk_before = self.deferred_e15.len();
            let rhs = self.eval4()?;
            let node = Expr::And(Box::new(lhs), Box::new(rhs));
            lhs = self.guard_skipped_junk(node, junk_before);
        }
        Ok(lhs)
    }

    fn eval4(&mut self) -> Result<Expr, VimlError> {
        let lhs = self.eval_shift()?;
        let (op, case) = match self.peek() {
            Tok::Cmp(op, case) => (*op, *case),
            Tok::Ident(id) if id == "is" => (CmpOp::Is, CaseFlag::Default),
            Tok::Ident(id) if id == "isnot" => (CmpOp::IsNot, CaseFlag::Default),
            _ => return Ok(lhs),
        };
        self.advance();
        let rhs = self.eval_shift()?;
        Ok(Expr::Compare {
            op,
            case,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        })
    }

    /// Bitwise shifts, the level vim 9.2 inserts between the comparisons and
    /// `+`/`-`/`.`: `var1 << var2`, `var1 >> var2`, left-associative.
    ///
    /// c: vim `eval.c` `eval5()` — Neovim has no such level (`1 << 2` is E15
    /// there), so this follows vim. Both operands are evaluated at run time by
    /// `eval_shift_number()`; see `b_check_lhs_shift` / `b_shift`.
    fn eval_shift(&mut self) -> Result<Expr, VimlError> {
        let mut lhs = self.eval5()?;
        loop {
            let op = match self.peek() {
                Tok::ShiftL => ArithOp::ShiftL,
                Tok::ShiftR => ArithOp::ShiftR,
                _ => break,
            };
            self.advance();
            let rhs = self.eval5()?;
            lhs = Expr::Arith {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                mod_op: false,
            };
        }
        Ok(lhs)
    }

    fn eval5(&mut self) -> Result<Expr, VimlError> {
        // c: `eval6(arg, rettv, evalarg, false)` — only the operand AFTER a
        // `.`/`..` concat is parsed with want_string.
        let mut lhs = self.eval6(false)?;
        loop {
            let op = match self.peek() {
                Tok::Plus => ArithOp::Add,
                Tok::Minus => ArithOp::Sub,
                Tok::Dot | Tok::DotDot => ArithOp::Concat,
                _ => break,
            };
            self.advance();
            // c: `eval6(arg, &var2, evalarg, op == '.')` — after a concat, a
            // "." is string concatenation, never a float decimal point
            // (`'a' . 1.5` is `'a' . 1 . 5` → `'a15'`).
            let rhs = self.eval6(op == ArithOp::Concat)?;
            lhs = Expr::Arith {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                mod_op: false,
            };
        }
        Ok(lhs)
    }

    fn eval6(&mut self, want_string: bool) -> Result<Expr, VimlError> {
        let mut lhs = self.eval7(want_string)?;
        loop {
            let op = match self.peek() {
                Tok::Star => ArithOp::Mul,
                Tok::Slash => ArithOp::Div,
                Tok::Percent => ArithOp::Mod,
                _ => break,
            };
            self.advance();
            // c: only the FIRST operand of eval6 inherits want_string.
            let rhs = self.eval7(false)?;
            lhs = Expr::Arith {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                mod_op: false,
            };
        }
        Ok(lhs)
    }

    fn eval7(&mut self, want_string: bool) -> Result<Expr, VimlError> {
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
        // c: eval_number(arg, rettv, evaluate, want_string) — after a concat
        // operator a "." is never a float decimal point. The lexer tokenized
        // context-free, so re-split a Float token here: `1.5` becomes the
        // Number 1 with `.5` re-lexed (a concat of 5), exactly the C's scan.
        if want_string {
            if let Tok::Float(_) = self.peek() {
                self.resplit_float_token();
            }
        }
        self.check_number_junk();
        let number_literal = matches!(self.peek(), Tok::Number(_) | Tok::Float(_));
        let mut e = self.primary()?;
        // c: the number case of eval7 — "Apply prefixed "-" and "+" now.
        // Matters especially when "->" follows." `eval7_leader(…, numeric_only
        // = true, …)` walks the leaders right to left and stops at the first
        // `!`, so `-1->abs()` is `abs(-1)` and `!-1->abs()` is `!abs(-1)`.
        if number_literal {
            while let Some(&op) = leaders.last() {
                match op {
                    UnaryOp::Not => break,
                    UnaryOp::Neg => {
                        e = Expr::Unary {
                            op,
                            expr: Box::new(e),
                        }
                    }
                    // `+` changes nothing about a number it is applied to.
                    UnaryOp::Plus => {}
                }
                leaders.pop();
            }
        }
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
        // The source offset of the token about to be consumed. For a call this
        // is where the C's `name` pointer would aim — see `Expr::Call::emsg_name`.
        let at = self.peek_span();
        let at_tok = self.i;
        match self.advance() {
            Tok::Number(n) => Ok(Expr::Number(n)),
            Tok::Float(f) => Ok(Expr::Float(f)),
            // Junk inside a re-split float (see `resplit_float_token`) — an
            // error Vim raises when the expression RUNS, not when it parses.
            Tok::DeferredErr(msg) => Ok(Expr::ScriptError(msg)),
            // Blob literal `0z…` desugars to `list2blob([byte, …])`, reusing the
            // ported list2blob builtin to build the Blob value.
            Tok::Blob(bytes) => Ok(Expr::Call {
                name: "list2blob".to_string(),
                args: vec![Expr::List(
                    bytes.into_iter().map(|b| Expr::Number(b as i64)).collect(),
                )],
                emsg_name: None,
            }),
            Tok::Str(s) => Ok(Expr::Str(s)),
            Tok::BStr(b) => Ok(Expr::Bytes(b)),
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
                // c: `E110: Missing ')'`, with no argument.
                self.eat_or(&Tok::RParen, "E110: Missing ')'")?;
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
            // c: eval7's default arm takes any text `get_name_len()` measures as a
            // name, and `get_id_len()` keeps a `:` at the very start (its
            // "xx:" check only fires once `len >= 1`). So `:` and `:abc` are
            // variable names: `echo :abc` is `E121: Undefined variable: :abc`
            // and `echo 0 ?: 1` skips the name `:` and then misses the `:`, E109.
            Tok::Colon => {
                let start = at.unwrap_or(0);
                let len =
                    crate::ported::eval::get_id_len(self.src.get(start..).unwrap_or(":")) as usize;
                let end = start + len.max(1);
                // The name ends inside the lexer's tokens only at a boundary
                // the lexer also drew; consume every token it covers.
                while self
                    .toks
                    .get(self.i)
                    .is_some_and(|t| !matches!(t.kind, Tok::Eof) && t.span >= start && t.end <= end)
                {
                    self.i += 1;
                }
                let name = self.src[start..end].to_string();
                self.name_operand(name, at, at_tok)
            }
            Tok::Ident(name) => self.name_operand(name, at, at_tok),
            // c (`eval9` → `eval_leader`/default): the token that cannot start an
            // operand is still unread when E15 quotes the rest, so it heads the
            // quoted text (`echo 1 + * 2` is `"* 2"`, `echo )` is `")"`).
            _ => {
                self.i = at_tok;
                Err(self.invexpr())
            }
        }
    }

    /// The operand that is a variable or function name: a call when `(` follows,
    /// otherwise a variable (or a vim9 keyword literal).
    fn name_operand(
        &mut self,
        name: String,
        at: Option<usize>,
        at_tok: usize,
    ) -> Result<Expr, VimlError> {
        {
            if matches!(self.peek(), Tok::LParen) {
                self.advance();
                // c: `emsg_funcname(…, name)` prints `name` up to its NUL:
                // the rest of the expression text, or the bare name for
                // `:call`'s outermost function.
                let shown = match at {
                    Some(s) if !(self.call_cmd && at_tok == 0) => self.src_from(s),
                    _ => name.clone(),
                };
                let args = self.arg_list(
                    &Tok::RParen,
                    &format!("E116: Invalid arguments for function {shown}"),
                    "E15: Invalid expression: \"%s\"",
                )?;
                Ok(Expr::Call {
                    name,
                    args,
                    emsg_name: at.map(|s| self.src_from(s)),
                })
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
                        emsg_name: None,
                    }),
                    "null_function" | "null_partial" => Ok(Expr::NullFunc),
                    _ => Ok(Expr::Var(name)),
                }
            } else {
                Ok(Expr::Var(name))
            }
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
                InterpPart::Lit(b) => segs.push(match String::from_utf8(b) {
                    Ok(s) => Expr::Str(s),
                    Err(e) => Expr::Bytes(e.into_bytes()),
                }),
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

    /// True when the `Tok::LBracket` at the current position directly abuts the
    /// previous token, i.e. it opens an index/slice subscript of the expression
    /// just parsed rather than starting a separate List argument.
    ///
    /// `vendor/eval.c:5964` — `handle_subscript()` only consumes `[` when
    /// `!ascii_iswhite(*(*arg - 1))`, so `echo l [0]` is two `:echo` arguments
    /// (a List and a List) while `echo l[0]` is one element. The same guard
    /// already exists for `(` ([`Self::lparen_abuts_prev`]) and for `.name`
    /// ([`Self::at_member_dot`]); `->` is the one form the C exempts.
    fn lbracket_abuts_prev(&self) -> bool {
        let i = self.i;
        i > 0
            && self.toks.get(i).is_some_and(|t| t.kind == Tok::LBracket)
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
        matches!(next.kind, Tok::Ident(_))
            && dot.span == prev.end // no space before the dot
            && next.span == dot.end // no space after the dot
    }

    /// True when the member dot at the current position is followed by
    /// `name(` — `d.get()`. Which of the two it means (call the funcref in the
    /// Dict, or concatenate with the result of calling `name`) is a run-time
    /// question, so this only reports the shape; see [`Expr::MemberCall`].
    fn at_member_call(&self) -> bool {
        let next_end = match self.toks.get(self.i + 1) {
            Some(t) => t.end,
            None => return false,
        };
        matches!(self.toks.get(self.i + 2), Some(t) if t.kind == Tok::LParen && t.span == next_end)
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
                let is_call = self.at_member_call();
                self.advance(); // consume the dot
                if let Tok::Ident(key) = self.advance() {
                    if is_call {
                        self.advance(); // consume '('
                        let args = self.arg_list(
                            &Tok::RParen,
                            &format!("E116: Invalid arguments for function {key}"),
                            "E15: Invalid expression: \"%s\"",
                        )?;
                        base = Expr::MemberCall {
                            base: Box::new(base),
                            key,
                            args,
                        };
                        continue;
                    }
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
                let args = self.arg_list(
                    &Tok::RParen,
                    "E116: Invalid arguments for function",
                    "E15: Invalid expression: \"%s\"",
                )?;
                base = Expr::CallExpr {
                    callee: Box::new(base),
                    args,
                };
                continue;
            }
            match self.peek() {
                // Only when the `[` abuts the base — see [`Self::lbracket_abuts_prev`].
                Tok::LBracket if self.lbracket_abuts_prev() => {
                    self.advance();
                    base = self.subscript(base)?;
                }
                Tok::Arrow => {
                    self.advance();
                    // c: `eval_lambda()` — `expr->{args -> body}(more)` calls the
                    // lambda with `expr` prepended to the arguments.
                    if matches!(self.peek(), Tok::LBrace) {
                        self.advance();
                        if !self.at_lambda() {
                            return Err(self.invexpr());
                        }
                        let callee = self.lambda()?;
                        if !matches!(self.peek(), Tok::LParen) {
                            // c: `semsg(_(e_missingparen), "lambda")`.
                            return Err(VimlError::msg("E107: Missing parentheses: lambda"));
                        }
                        if !self.lparen_abuts_prev() {
                            // c: `emsg(_(e_nowhitespace))`.
                            return Err(VimlError::msg(
                                "E274: No white space allowed before parenthesis",
                            ));
                        }
                        self.advance();
                        let mut args = vec![base];
                        args.extend(self.arg_list(
                            &Tok::RParen,
                            "E116: Invalid arguments for function",
                            "E15: Invalid expression: \"%s\"",
                        )?);
                        base = Expr::CallExpr {
                            callee: Box::new(callee),
                            args,
                        };
                        continue;
                    }
                    let name_at = self.peek_span();
                    let name = match self.advance() {
                        Tok::Ident(n) => n,
                        _ => return Err(self.invexpr()),
                    };
                    // c: `eval_method` hands `call_func_rettv` a pointer into
                    // the source, so E116/E740 quote from the name on.
                    let shown = name_at.map_or_else(|| name.clone(), |s| self.src_from(s));
                    self.eat(&Tok::LParen)?;
                    let args = self.arg_list(
                        &Tok::RParen,
                        &format!("E116: Invalid arguments for function {shown}"),
                        "E15: Invalid expression: \"%s\"",
                    )?;
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

    /// Port of `eval_list()` (`vendor/eval.c:3857`), opening `[` consumed.
    ///
    /// The loop stops at `]` OR the end of the text, so the two ways a list can
    /// run out are different errors: after an item with no comma it is E696
    /// (`[1`), but after a comma — or before any item — the loop never runs
    /// again and the end check reports E697 (`[1,`, `[`). An item that fails
    /// with nothing to say (`[1 +`) stays silent, for the caller's E15.
    fn list_literal(&mut self) -> Result<Expr, VimlError> {
        let mut items = Vec::new();
        // The end of the TOKENS is the end of the text only when the lexer did
        // not stop early: text it could not read (`[1, 'a`) is still an item
        // for `eval1()`, whose error (E115) is the one reported.
        while !matches!(self.peek(), Tok::RBracket)
            && !(matches!(self.peek(), Tok::Eof) && self.lex_err.is_none())
        {
            items.push(self.nested_eval1()?);
            // c: the comma must come after the value
            let had_comma = *self.peek() == Tok::Comma;
            if had_comma {
                self.advance();
            }
            if *self.peek() == Tok::RBracket {
                break;
            }
            if !had_comma {
                return Err(VimlError::msg(format!(
                    "E696: Missing comma in List: {}",
                    self.rest()
                )));
            }
        }
        if *self.peek() != Tok::RBracket {
            return Err(VimlError::msg(format!(
                "E697: Missing end of List ']': {}",
                self.rest()
            )));
        }
        self.advance();
        Ok(Expr::List(items))
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
            vim9: true,
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
                // `list<list<number>>` closes two levels with one `>>` token.
                Tok::ShiftR if angle > 1 => {
                    angle -= 2;
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
        self.eat_or(&Tok::Arrow, "E451: Expected }: %s")?;
        // c: the body's TEXT is what `get_lambda_tv` stores for the generated
        // function, so everything parsed inside it quotes only up to the `}`.
        let clipped = self
            .lambda_body_end()
            .inspect(|e| self.clip.push(*e))
            .is_some();
        let body = self.eval1();
        if clipped {
            self.clip.pop();
        }
        let body = body?;
        // c: `E451: Expected }: %s`.
        self.eat_or(&Tok::RBrace, "E451: Expected }: %s")?;
        Ok(Expr::Lambda {
            params,
            body: Box::new(body),
            vim9: vim9_active(),
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
                // c: `E720: Missing colon in Dictionary: %s`.
                self.eat_or(&Tok::Colon, "E720: Missing colon in Dictionary: %s")?;
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
                _ => {
                    return Err(VimlError::msg(format!(
                        "E722: Missing comma in Dictionary: {}",
                        self.rest()
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
        // c: eval.c:4500 — `tv_dict_find` before adding each item raises E721 on a
        // repeated key. Keys that are constants (a string or number literal)
        // resolve to their final string form at parse time, so the duplicate can
        // be caught here; a computed key (variable/expression) is left for run
        // time, matching Vim's evaluate-then-check order for those.
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        loop {
            let key = if vim9 {
                self.vim9_dict_key()?
            } else {
                let k = self.eval1()?;
                // c: `E720: Missing colon in Dictionary: %s`.
                self.eat_or(&Tok::Colon, "E720: Missing colon in Dictionary: %s")?;
                k
            };
            // c: eval.c:4502 semsg(e_duplicate_key_in_dictionary_str, key).
            if let Some(k) = const_dict_key(&key) {
                if !seen.insert(k.clone()) {
                    return Err(VimlError::msg(format!(
                        "E721: Duplicate key in Dictionary: \"{k}\""
                    )));
                }
            }
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
                _ => {
                    return Err(VimlError::msg(format!(
                        "E722: Missing comma in Dictionary: {}",
                        self.rest()
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
            return Err(VimlError::msg(format!(
                "E720: Missing colon in Dictionary: {}",
                self.rest()
            )));
        }
        // Advance past every token up to and including the `:` at offset `colon`
        // (a standalone `Colon`, or one absorbed into a scope-letter `Ident`),
        // leaving the cursor at the value.
        while self.toks[self.i].span <= colon && !matches!(self.peek(), Tok::Eof) {
            self.advance();
        }
        Ok(Expr::Str(key))
    }

    /// Comma-separated operands up to `close`.
    ///
    /// The two failure messages are the CALLER's to choose, because vim uses
    /// different ones for the same loop and even for the same caller:
    ///
    /// * `ran_out` — the input ended before `close`. `[1` and `[1,2` are
    ///   `E696: Missing comma in List: ` (empty argument); `f(`, `f(1` and
    ///   `f(1,` are `E116: Invalid arguments for function f`.
    /// * `junk` — a token that is neither a separator nor `close`. `[1 2]` is
    ///   `E696: Missing comma in List: 2]`, while `string(1e3)` — where `e3` is
    ///   junk abutting a number — is `E15: Invalid expression: …`, NOT E116.
    ///
    /// A `%s` in either receives the source still unread.
    fn arg_list(&mut self, close: &Tok, ran_out: &str, junk: &str) -> Result<Vec<Expr>, VimlError> {
        let mut args = Vec::new();
        if self.peek() == close {
            self.advance();
            return Ok(args);
        }
        loop {
            // c: `get_func_arguments` stops at a `,` where an argument should
            // start, and the list is then not closed by `)`: E116, not a
            // complaint about the comma (`assert_equal(, x)`).
            if *close == Tok::RParen && matches!(self.peek(), Tok::Comma) {
                return Err(VimlError::msg(ran_out.replace("%s", "")));
            }
            // An operand that simply RAN OUT (`f(`, `f(1,`) is the same
            // unterminated-list failure as a missing separator, and vim reports
            // it with the same message — `E116: Invalid arguments for function
            // f`, not a generic E15. An operand that failed with something to
            // say keeps its own message.
            match self.nested_eval1() {
                Ok(e) => args.push(e),
                Err(e) if e.is_silent() => {
                    return Err(VimlError::msg(ran_out.replace("%s", self.rest())))
                }
                Err(e) => return Err(e),
            }
            // c: `get_func_arguments` reads at most `MAX_FUNC_ARGS` (20); a list
            // that is not closed by then fails, and `get_func_tv` reports it as
            // E740 rather than E116 because `argcount == MAX_FUNC_ARGS`.
            if args.len() == MAX_FUNC_ARGS && !matches!(self.peek(), Tok::RParen) {
                if let Some(name) = ran_out.strip_prefix("E116: Invalid arguments for function ") {
                    return Err(VimlError::msg(format!(
                        "E740: Too many arguments for function {name}"
                    )));
                }
            }
            // The rest is read BEFORE the separator is consumed: vim quotes the
            // offending token itself (`[1 2]` is `…: 2]`, not `…: ]`).
            let rest = self.rest().to_string();
            match self.advance() {
                Tok::Comma => {
                    if self.peek() == close {
                        self.advance();
                        break;
                    }
                }
                ref t if t == close => break,
                // c: `get_func_arguments` stops at anything but a `,` after an
                // argument, and a list not then closed by `)` is `ret = FAIL` —
                // E116 for the function, never a message about the text itself
                // (`len(1 2)`).
                _ if rest.is_empty() || *close == Tok::RParen => {
                    return Err(VimlError::msg(ran_out.replace("%s", "")))
                }
                _ => return Err(VimlError::msg(junk.replace("%s", &rest))),
            }
        }
        Ok(args)
    }
}

/// The final Dict-key string for a *constant* key expression, or `None` for a
/// computed key. Mirrors `tv_get_string_buf_chk` on the evaluated key: a string
/// literal is its own value; a Number is its decimal form (`{1: v}` keys on
/// "1"). A Float key is a Vim error (E806), not a duplicate concern, so it is
/// left out. Non-constant keys can only be compared after evaluation, so they
/// are deferred to run time (`None`).
fn const_dict_key(key: &Expr) -> Option<String> {
    match key {
        Expr::Str(s) => Some(s.clone()),
        Expr::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `:wincmd`'s argument is the next character even when it is `|` — every
    /// `:mksession` writes `wincmd _ | wincmd |` — and a later `|` separates.
    #[test]
    fn wincmd_takes_a_bar_as_its_argument() {
        assert_eq!(
            split_commands("wincmd _ | wincmd |"),
            ["wincmd _ ", " wincmd |"]
        );
        assert_eq!(
            split_commands("2wincmd | | echo 1"),
            ["2wincmd | ", " echo 1"]
        );
        assert_eq!(split_commands("winc gf | echo 1"), ["winc gf ", " echo 1"]);
        assert_eq!(split_commands("echo 1 | echo 2"), ["echo 1 ", " echo 2"]);
    }

    #[test]
    fn command_modifiers_are_stripped() {
        // `silent!` is NOT merely stripped: it wraps the command, because the bang
        // suppresses the command's error message (`emsg_silent`). The command
        // itself survives inside the wrapper.
        let Stmt::Silent { bang, stmt } = parse_stmt("silent! colorscheme molokai").unwrap() else {
            panic!("`silent!` must wrap the command it silences");
        };
        assert!(bang, "the bang is what also silences the command's errors");
        assert!(matches!(*stmt, Stmt::Colorscheme(n) if n == "molokai"));
        // `:silent` without the bang still wraps (it silences output, not errors).
        let Stmt::Silent { bang, .. } = parse_stmt("silent echo 'x'").unwrap() else {
            panic!("`silent` must wrap the command it silences");
        };
        assert!(!bang);
        // Stacked modifiers, a `verbose` count, and abbreviations: the non-`silent`
        // modifiers are still stripped, and the `silent` wraps what is left.
        let Stmt::Silent { bang, stmt } =
            parse_stmt("silent noautocmd verbose 9 set number").unwrap()
        else {
            panic!("a leading `silent` must wrap the command");
        };
        assert!(!bang);
        assert!(matches!(*stmt, Stmt::Set(a) if a == "number"));
        // A modifier run with no `silent` is stripped away entirely, as before.
        assert!(matches!(
            parse_stmt("noautocmd verbose 9 set number").unwrap(),
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
        // Line 2 is a `:let` whose target does not parse. Tolerant parsing must
        // still yield the good statements on lines 1 and 3.
        let src = "set number\nlet bad[1 +] = 0\ncolorscheme molokai\n";
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
            [(1, Stmt::If { .. })]
        ));
        // A leaf command then a one-line block: two statements — and because they
        // are on ONE line, they form a `LineGroup`, so an error in the first
        // abandons the second (Vim's do_cmdline abandons the rest of the line).
        match parse_program("let x = 5 | if x > 3 | echo 'big' | endif")
            .unwrap()
            .as_slice()
        {
            [(1, Stmt::LineGroup(g))] => {
                assert!(matches!(g.as_slice(), [Stmt::Let { .. }, Stmt::If { .. }]));
            }
            s => panic!("expected [LineGroup([Let, If])], got {s:?}"),
        }
        // A `for` one-liner.
        assert!(matches!(
            parse_program("for i in [1] | echo i | endfor")
                .unwrap()
                .as_slice(),
            [(1, Stmt::For { .. })]
        ));
        // A plain bar line is one `LineGroup` holding the two leaf statements.
        match parse_program("let a = 1 | echo a").unwrap().as_slice() {
            [(1, Stmt::LineGroup(g))] => assert_eq!(g.len(), 2),
            s => panic!("expected [LineGroup(2)], got {s:?}"),
        }
    }

    #[test]
    fn line_continuation() {
        // A `\` continuation line joins onto the previous logical line.
        let prog = parse_program("let x = [1,\n      \\ 2,\n      \\ 3]").unwrap();
        match &prog[0].1 {
            Stmt::Let {
                expr: Expr::List(items),
                ..
            } => assert_eq!(items.len(), 3),
            s => panic!("expected 3-item list, got {s:?}"),
        }
        // Line numbers: the statement after a 2-physical-line logical line keeps
        // its real physical number.
        let lines = parse_program("let a = 1\nlet b = [10,\n  \\ 20]\nlet c = 3").unwrap();
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
    fn dict_duplicate_key_e721() {
        // c: eval.c:4502 — a repeated constant key raises E721 naming the key.
        let e = parse_expr("{'a': 1, 'a': 2}").unwrap_err();
        assert_eq!(e.to_string(), "E721: Duplicate key in Dictionary: \"a\"");
        // A Number key keys on its decimal string, so `1` and `'1'` collide.
        assert!(parse_expr("{1: 2, '1': 3}").is_err());
        // Distinct keys are fine, including a repeated *value*.
        assert!(parse_expr("{'a': 1, 'b': 1}").is_ok());
        // A non-constant (computed) key is deferred to run time — not flagged here.
        assert!(parse_expr("{k: 1, 'k': 2}").is_ok());
    }

    #[test]
    fn collections_and_stmts() {
        assert!(matches!(parse_expr("[1, 2, 3]").unwrap(), Expr::List(_)));
        assert!(matches!(parse_expr("x[1:2]").unwrap(), Expr::Slice { .. }));
        assert!(matches!(parse_stmt("echo 1 + 1").unwrap(), Stmt::Echo(_)));
        assert!(matches!(parse_stmt("let x = 5").unwrap(), Stmt::Let { .. }));
    }

    /// The lines a `:mksession` / vim-session script is made of. In a sourced
    /// file each is an Ex command, as vim's `do_one_cmd` reads it; outside one
    /// (the expression REPL) the old reading stands.
    #[test]
    fn script_lines_are_ex_commands() {
        let ex = |line: &str| with_ex_lines(|| matches!(parse_stmt(line), Ok(Stmt::ExCmd(_))));
        for line in [
            "only",
            "cd ~/",
            "cd /tmp/x",
            "argglobal",
            "%argdel",
            "$argadd .zshrc",
            "wincmd t",
            "1wincmd w",
            "tabnext 1",
            "30",
            "407",
            ".,$d",
            "+3",
        ] {
            assert!(ex(line), "`{line}` in a script is an Ex command");
        }
        // `silent only`: the modifier wraps the command, it does not hide it.
        let wrapped = with_ex_lines(|| parse_stmt("silent only").unwrap());
        assert!(
            matches!(&wrapped, Stmt::Silent { stmt, .. } if matches!(**stmt, Stmt::ExCmd(_))),
            "{wrapped:?}"
        );

        // Statements vimlrs evaluates keep their own parse, and a call stays a
        // call even when the name is a command word (`list(…)` is not `:list`).
        assert!(with_ex_lines(|| matches!(
            parse_stmt("let s:l = 1").unwrap(),
            Stmt::Let { .. }
        )));
        assert!(with_ex_lines(|| matches!(
            parse_stmt("set so=0").unwrap(),
            Stmt::Set(_)
        )));
        assert!(!ex("list(1)"), "a word followed by `(` is a call");
        assert!(!ex("undefinedword"), "not in the command table");

        // Outside a script, a number and a bare word are expressions as before.
        assert!(matches!(parse_stmt("30").unwrap(), Stmt::Expr(_)));
        assert!(matches!(parse_stmt("3 + 4").unwrap(), Stmt::Expr(_)));
    }

    /// Which command lines the runtime may skip silently (built-in, as vim runs
    /// them without a word) and which are E492 (no such command).
    #[test]
    fn builtin_ex_command_names_see_past_the_range() {
        for line in ["only", "%argdel", "$argadd f", "1wincmd w", ".,$d", "argg"] {
            assert!(names_builtin_ex_command(line), "{line}");
        }
        for line in ["%foobar", "!ls", "zzz"] {
            assert!(!names_builtin_ex_command(line), "{line}");
        }
    }
}
