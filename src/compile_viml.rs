//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! EXTENSION — NO `vendor/` COUNTERPART. Lowers the synthesis AST to a
//! `fusevm::Chunk`. Neovim has no bytecode compiler; this is the net-new piece
//! that makes VimL run on fusevm (the role zshrs's `compile_zsh.rs` plays for
//! zsh). Each expression compiles to a sequence leaving one value on the stack;
//! faithful VimL semantics are never inlined here — every operator routes to a
//! `VIML_*` builtin whose handler calls the canonical ports.
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use fusevm::{ChunkBuilder, Op, Value};
use serde::{Deserialize, Serialize};

use crate::fusevm_bridge as h;
use crate::viml_ast::{ArithOp, Block, Expr, ForVars, LetTarget, Stmt, UnaryOp, UnletArg};
use crate::viml_lexer::{CaseFlag, CmpOp, VimlError};

/// A compiled user function: its name, parameter names, and body chunk.
#[derive(Serialize, Deserialize, Clone)]
pub struct UserFuncDef {
    /// Function name (possibly scoped).
    pub name: String,
    /// Parameter names (without the `a:` prefix).
    pub params: Vec<String>,
    /// Compiled default-value expressions for optional parameters, as
    /// `(param index, chunk)`. Each chunk leaves the default on the VM stack and
    /// is run at call time (in the partially-bound `a:` scope) when the argument
    /// is omitted.
    pub defaults: Vec<(usize, fusevm::Chunk)>,
    /// `function!` — replace an existing definition.
    pub bang: bool,
    /// `true` for a vim9 `:def` — bare names in the body that are not locals or
    /// parameters resolve to script-scope vars/functions (which vimlrs keeps in
    /// the global dict). `false` for a legacy `:function`.
    pub vim9: bool,
    /// c: `FC_DICT` — the function takes a `self` dict, either from the `dict`
    /// attribute or implicitly from the `:function d.key()` form. Reading such a
    /// function out of a Dict binds it to that Dict (c: `make_partial`,
    /// userfunc.c:3805, whose only gate is `fp->uf_flags & FC_DICT`).
    pub dict: bool,
    /// c: `FC_ABORT` — declared `abort`. The body was compiled to stop at its
    /// first error, and the error stays visible to the caller (see the
    /// `did_emsg` restore in `fusevm_bridge::in_callee`).
    pub abort: bool,
    /// Compiled function body.
    pub chunk: fusevm::Chunk,
    /// c: `FC_CLOSURE` — declared `closure`. Such a function reads the
    /// variables of the activation that defined it (`register_closure`).
    #[serde(default)]
    pub closure: bool,
    /// c: `uf_scoped` — the activation a `closure` function was defined in,
    /// recorded when its `:function` line runs. Runtime state, never cached.
    #[serde(skip)]
    pub scoped: Option<std::rc::Rc<crate::ported::eval::vars::ScopedFunccal>>,
}

/// A compiled program: the top-level `main` chunk plus the user functions it
/// defines. Serialized as a unit into the rkyv script cache so a cache hit
/// restores both (functions and all).
#[derive(Serialize, Deserialize)]
pub struct CompiledProgram {
    /// Top-level statements.
    pub main: fusevm::Chunk,
    /// Functions with no `:function` line of their own to reach, so there is
    /// nothing to defer: the anonymous bodies behind `{args -> body}` lambdas
    /// and behind `:function d.key()`. Registered unconditionally at load.
    pub funcs: Vec<UserFuncDef>,
    /// Every NAMED `:function` definition — whether at script level or nested in
    /// an `:if`/`:while`/`:for`/`:try` — *staged* into the runtime's pending
    /// registry and inserted into the live `FUNCTIONS` table only when its
    /// `:function` line actually executes.
    ///
    /// That is what `:function` is: an ordinary command, not a declaration. It
    /// makes the idempotent `if !exists('*F') | function F() … | endif` guard
    /// define `F` only on the first source, and it makes a forward reference
    /// fail — `function('Later')` written above `:function Later()` is
    /// `E700: Unknown function: Later` in vim, and was accepted here while
    /// script-level definitions were hoisted into [`funcs`](Self::funcs).
    /// Faithful to userfunc.c: `:function` inside `if`/`while`/`for`/`try` is
    /// legal (those only adjust `indent`, userfunc.c:2485-2494) and the def
    /// executes when control flow reaches it.
    pub deferred_funcs: Vec<UserFuncDef>,
}

impl CompiledProgram {
    /// Every user function the program compiled, whichever list it landed in.
    ///
    /// Callers that want to *inspect* the program (dump bytecode, report
    /// execution tiers, find a body in a test) want all of them; only the
    /// runtime loader cares about the registered/staged distinction.
    pub fn all_funcs(&self) -> impl Iterator<Item = &UserFuncDef> {
        self.funcs.iter().chain(self.deferred_funcs.iter())
    }
}

thread_local! {
    /// Anonymous functions generated from `{args -> body}` lambdas during the
    /// current compile. Accumulated as expressions compile (including inside
    /// `:function` bodies), then folded into [`CompiledProgram::funcs`].
    static LAMBDA_FUNCS: std::cell::RefCell<Vec<UserFuncDef>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// Counter for unique `<lambda>N` names within a compile.
    static LAMBDA_COUNTER: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Counter for the generated names of `:function d.key()` bodies (c: `func_nr`).
    static DICT_FUNC_COUNTER: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Functions whose `:function` sits inside a script-level control-flow block
    /// (`:if`/`:while`/`:for`/`:try`). Accumulated as the block body compiles,
    /// then moved into [`CompiledProgram::deferred_funcs`]. They are registered
    /// at run time when their emitted define-op executes, not at load — see
    /// [`CompiledProgram::deferred_funcs`].
    static DEFERRED_FUNCS: std::cell::RefCell<Vec<UserFuncDef>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// Whether the compile in progress is a debug (`--dap`) build, so every
    /// [`Compiler`] it creates emits a `SET_LINENO` marker before each
    /// statement. Set for the duration of [`compile_program_debug`] only.
    ///
    /// A flag rather than a `Compiler::new` parameter because the compilers that
    /// need it are not all reachable from one call: `compile_program_inner`
    /// builds the top-level one, `compile_function_body` builds one per
    /// `:function` body, and lambda bodies build their own from inside
    /// expression compilation. Marking only the top-level chunk is what left
    /// function bodies unbreakable.
    static DEBUG_MARKERS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Stable, content-derived staging key for a deferred (block-level) `:function`.
/// The compiler emits this key as a string constant ahead of the runtime
/// define-op; the bridge stages the def under the same key. Content-addressed so
/// the key is identical across a recompile or a script-cache hit (survives
/// caching) and cannot collide across independently-compiled programs — the
/// runtime pending registry is a global thread-local shared by every sourced
/// script and nested function body. `DefaultHasher::new()` uses a fixed seed, so
/// the digest is deterministic across processes. The name prefix guarantees two
/// distinct functions never share a key even under a (astronomically unlikely)
/// digest collision.
pub fn deferred_key(def: &UserFuncDef) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    // Hash the serialized def (name + params + defaults + bang + body chunk).
    // bincode is already a dependency (the script cache uses it); an encode
    // failure is impossible for these owned, plain-data types.
    bincode::serialize(def).unwrap_or_default().hash(&mut h);
    format!("{}#{:016x}", def.name, h.finish())
}

/// Collect the function-scope variable names referenced in `e` that are not in
/// `bound` — the names `check_vars` tests when a lambda is created, to decide
/// whether it is a closure. A nested lambda's own params extend `bound` for
/// its body. Function-call names are not variables and are not collected.
fn collect_free_vars(
    e: &Expr,
    bound: &mut Vec<String>,
    out: &mut std::collections::BTreeSet<String>,
) {
    match e {
        Expr::Var(n) => {
            // c: `check_vars` counts only a name whose scope is the running
            // function's locals or arguments: bare names, `l:` and `a:`. The
            // other scopes (`g:`/`b:`/`w:`/`t:`/`v:`/`s:`) are the same from
            // inside the lambda.
            let capturable = !n.contains(':') || n.starts_with("a:") || n.starts_with("l:");
            if capturable && !bound.contains(n) {
                out.insert(n.clone());
            }
        }
        Expr::Lambda { params, body, .. } => {
            let base = bound.len();
            bound.extend(params.iter().cloned());
            collect_free_vars(body, bound, out);
            bound.truncate(base);
        }
        Expr::List(xs) => xs.iter().for_each(|x| collect_free_vars(x, bound, out)),
        Expr::Dict(ps) => ps.iter().for_each(|(k, v)| {
            collect_free_vars(k, bound, out);
            collect_free_vars(v, bound, out);
        }),
        Expr::Unary { expr, .. } => collect_free_vars(expr, bound, out),
        Expr::Arith { lhs, rhs, .. } | Expr::Compare { lhs, rhs, .. } => {
            collect_free_vars(lhs, bound, out);
            collect_free_vars(rhs, bound, out);
        }
        Expr::And(a, b) | Expr::Or(a, b) | Expr::Coalesce(a, b) => {
            collect_free_vars(a, bound, out);
            collect_free_vars(b, bound, out);
        }
        Expr::Ternary {
            cond,
            then,
            otherwise,
        } => {
            collect_free_vars(cond, bound, out);
            collect_free_vars(then, bound, out);
            collect_free_vars(otherwise, bound, out);
        }
        Expr::Index { base, index } => {
            collect_free_vars(base, bound, out);
            collect_free_vars(index, bound, out);
        }
        Expr::Slice { base, from, to } => {
            collect_free_vars(base, bound, out);
            if let Some(f) = from {
                collect_free_vars(f, bound, out);
            }
            if let Some(t) = to {
                collect_free_vars(t, bound, out);
            }
        }
        Expr::Member { base, .. } => collect_free_vars(base, bound, out),
        Expr::MemberCall { base, args, .. } => {
            collect_free_vars(base, bound, out);
            args.iter().for_each(|a| collect_free_vars(a, bound, out));
        }
        Expr::Interp(segs) => segs.iter().for_each(|s| collect_free_vars(s, bound, out)),
        Expr::Call { args, .. } => args.iter().for_each(|a| collect_free_vars(a, bound, out)),
        Expr::CallExpr { callee, args } => {
            collect_free_vars(callee, bound, out);
            args.iter().for_each(|a| collect_free_vars(a, bound, out));
        }
        Expr::Method { base, args, .. } => {
            collect_free_vars(base, bound, out);
            args.iter().for_each(|a| collect_free_vars(a, bound, out));
        }
        Expr::ScriptErrorGuard { inner, .. } => collect_free_vars(inner, bound, out),
        // Literals, sigil-scoped refs and deferred errors capture nothing.
        Expr::Number(_)
        | Expr::Float(_)
        | Expr::Str(_)
        | Expr::Bytes(_)
        | Expr::NullFunc
        | Expr::Option(_)
        | Expr::Env(_)
        | Expr::Register(_)
        | Expr::ScriptError(_) => {}
    }
}

/// Compile a `:function` definition's fields into a [`UserFuncDef`]. Shared by
/// the top-level collection in [`compile_program`] and the block-level deferred
/// path in [`Compiler::stmt`], so both register byte-identical defs.
fn build_user_func_def(
    name: &str,
    args: &[String],
    defaults: &[(usize, Expr)],
    body: &[(u32, Stmt)],
    flags: FuncFlags,
    def_line: u32,
    exc: bool,
) -> Result<UserFuncDef, VimlError> {
    let defaults = defaults
        .iter()
        .map(|(i, e)| Ok((*i, compile_expr_only(e)?)))
        .collect::<Result<Vec<_>, VimlError>>()?;
    Ok(UserFuncDef {
        name: name.to_string(),
        params: args.to_vec(),
        defaults,
        bang: flags.bang,
        vim9: flags.vim9,
        dict: flags.dict,
        abort: flags.abort,
        chunk: compile_function_body(body, exc, def_line, flags.abort, flags.vim9, flags.closure)?,
        closure: flags.closure,
        scoped: None,
    })
}

/// The definition-site flags a `:function` carries into its registry entry.
/// Grouped so [`build_user_func_def`] takes one parameter for the three rather
/// than three positional `bool`s a call site can silently transpose.
#[derive(Clone, Copy)]
struct FuncFlags {
    /// `function!` — replace an existing definition.
    bang: bool,
    /// vim9 `:def` — bare names in the body resolve to script scope.
    vim9: bool,
    /// c: `FC_DICT` — the function takes a `self` dict.
    dict: bool,
    /// c: `FC_ABORT` — the `abort` attribute.
    abort: bool,
    /// c: `FC_CLOSURE` — the `closure` attribute.
    closure: bool,
}

/// Split a `:function` name that targets a Dict key (`d.key`, `g:d.key`,
/// `d.a.b`) into the container expression and the final key. Returns `None` for
/// an ordinary function name — including an autoload name (`foo#bar`), which
/// has no dot, and a name whose dots are all part of a scope prefix.
fn dict_func_target(name: &str) -> Option<(Expr, String)> {
    let (head, key) = name.rsplit_once('.')?;
    if head.is_empty() || key.is_empty() {
        return None;
    }
    // An indexed container (`s:m[k].name`) is an expression in its own right:
    // c: `trans_function_name` hands it to `get_lval`, which evaluates `[k]`.
    if head.contains('[') {
        return crate::viml_parser::parse_expr(head)
            .ok()
            .map(|base| (base, key.to_string()));
    }
    // The container is everything before the last dot, itself possibly a chain.
    let mut base = match head.split_once('.') {
        None => Expr::Var(head.to_string()),
        Some(_) => {
            let mut parts = head.split('.');
            let mut e = Expr::Var(parts.next()?.to_string());
            for p in parts {
                e = Expr::Member {
                    base: Box::new(e),
                    key: p.to_string(),
                };
            }
            e
        }
    };
    // A bare `d` inside a `:function d.key()` names the script/global variable
    // `d`, which `Expr::Var` already resolves.
    if let Expr::Var(v) = &base {
        if v.is_empty() {
            return None;
        }
        base = Expr::Var(v.clone());
    }
    Some((base, key.to_string()))
}

/// The next generated name for a `:function d.key()` body. c: `func_nr`
/// (`userfunc.c`) — a decimal counter starting at 1, which is why vim prints
/// `function('1')` for the first such definition in a session.
fn next_dict_func_name() -> String {
    DICT_FUNC_COUNTER.with(|c| {
        let n = c.get() + 1;
        c.set(n);
        n.to_string()
    })
}

/// A fresh unique anonymous-function name, `<lambda>N`.
///
/// c: `get_lambda_name()` (`userfunc.c:269`) is `"<lambda>%d", ++lambda_no` — the
/// counter is *pre*-incremented, so the first lambda in a script is
/// `<lambda>1`, which is what `string({x -> x})` prints in vim.
fn next_lambda_name() -> String {
    LAMBDA_COUNTER.with(|c| {
        let n = c.get() + 1;
        c.set(n);
        format!("<lambda>{n}")
    })
}

/// Compile a program: top-level statements into `main`, `:function` definitions
/// into `funcs`.
pub fn compile_program(stmts: &[(u32, Stmt)]) -> Result<CompiledProgram, VimlError> {
    compile_program_inner(stmts, true, false, false)
}

/// Compile a program that runs INSIDE another one — an `execute()` command
/// string, an `eval()` expression, a nested `:source`.
///
/// Identical to [`compile_program`] except that the command-name tag is always
/// emitted. `compile_program` emits it only when the program itself uses
/// exceptions, on the reasoning that nothing else can observe it — which is true
/// of a whole script and false of a nested one: the CALLER's `:try` observes the
/// tag of a command that ran inside `execute()`, even though the executed string
/// contains no `:try` of its own. Without this,
/// `try | echo execute('echo {}+1') | catch | echo v:exception | endtry` reported
/// `Vim(let):E728` — whatever tag the enclosing statement had last set — where
/// both vim 9.2 and nvim 0.12 report `Vim(echo):E728`.
pub fn compile_program_nested(stmts: &[(u32, Stmt)]) -> Result<CompiledProgram, VimlError> {
    compile_program_inner(stmts, true, true, false)
}

/// [`compile_program_nested`] for the text of an `:execute` run from a function
/// body. c: `ex_execute` hands `do_cmdline` the function's own `getline`, so
/// `ex_return`'s `getline_equal(…, get_func_line)` holds and `:return` there
/// returns from the FUNCTION (`exe 'return ' . x`). `execute()`, autocommands
/// and user commands pass no getline and stay [`compile_program_nested`].
/// `VIML_EXEC_RETURNED` modes: look at the flag, set it, or read and clear it.
pub const EXEC_RETURNED_PEEK: i64 = 0;
/// See [`EXEC_RETURNED_PEEK`].
pub const EXEC_RETURNED_SET: i64 = 1;
/// See [`EXEC_RETURNED_PEEK`].
pub const EXEC_RETURNED_TAKE: i64 = 2;

pub fn compile_program_nested_in_function(
    stmts: &[(u32, Stmt)],
) -> Result<CompiledProgram, VimlError> {
    compile_program_inner(stmts, true, true, true)
}

/// Compile a program that is only *part* of a script — one statement of a
/// tolerant, statement-at-a-time source (`fusevm_bridge::source_tolerant`).
///
/// Identical to [`compile_program`] except that top-level locals are never
/// slotted. Slotting is what lets a script-level numeric loop lower to native
/// ops, and it is sound only when the whole script is in front of the compiler:
/// a bare `let A = 1` compiled alone looks like a write nobody reads, so the
/// slot absorbs it and `g:A` is never written — the next statement's `echo A`
/// then raised E121 where vim prints 1.
pub fn compile_script_stmt(stmts: &[(u32, Stmt)]) -> Result<CompiledProgram, VimlError> {
    compile_program_inner(stmts, false, false, false)
}

fn compile_program_inner(
    stmts: &[(u32, Stmt)],
    slot_top: bool,
    nested: bool,
    func_cmdline: bool,
) -> Result<CompiledProgram, VimlError> {
    // Exceptions are global: if anything in the program throws or `:try`s, every
    // compilation unit emits unwind checks (so a throw can propagate through a
    // function call into a caller's `:try`). A NESTED program counts as using
    // them whatever it contains, because the program that ran it may be inside a
    // `:try` that observes what happens here.
    let exc = nested || uses_exceptions(stmts);
    LAMBDA_FUNCS.with(|f| f.borrow_mut().clear());
    DEFERRED_FUNCS.with(|f| f.borrow_mut().clear());
    // LAMBDA_COUNTER is NOT reset here. c: `lambda_no` is a `static int` inside
    // `get_lambda_name()` (userfunc.c:271) that is only ever incremented — one
    // counter for the life of the process. Resetting it per compile made every
    // NESTED compile (`eval()`, an expression string handed to `map()`,
    // `:execute`) restart at `<lambda>1` and REGISTER OVER the outer script's
    // `<lambda>1`:
    //
    //   let A = {x -> x * 2}
    //   let B = eval('{x -> x + 100}')
    //   echo A(5)                       " 105 here, 10 in vim
    let mut funcs = Vec::new();
    let mut top: Block = Vec::new();
    for (line, s) in stmts {
        if let Stmt::Function {
            name,
            args,
            defaults,
            body,
            bang,
            vim9,
            dict: _,
            abort,
            closure,
        } = s
        {
            // `:function d.key()` defines an ANONYMOUS function and stores a
            // reference to it in the Dict — `string(d.key)` in vim is
            // `function('1', {…})`, never `function('d.key')`. So the body is
            // registered under a generated numeric name (vim's `func_nr`
            // counter, which also starts at 1) and the definition line becomes
            // the assignment it implies.
            match dict_func_target(name) {
                Some((base, key)) => {
                    let anon = next_dict_func_name();
                    // c: `ex_function` sets `FC_DICT` for the `d.key()` form
                    // whether or not `dict` was written (`fudi.fd_dict != NULL`).
                    // Verified against vim 9.2: `function d.nodict()` with no
                    // attribute still gives `string(d.nodict)` ==
                    // `function('1', {…})`.
                    let flags = FuncFlags {
                        bang: *bang,
                        vim9: *vim9,
                        dict: true,
                        abort: *abort,
                        closure: *closure,
                    };
                    funcs.push(build_user_func_def(
                        &anon, args, defaults, body, flags, *line, exc,
                    )?);
                    top.push((
                        *line,
                        Stmt::Let {
                            target: LetTarget::Index {
                                base: Box::new(base),
                                index: Box::new(Expr::Str(key)),
                                // Synthesized by the `:function d.key()` desugar.
                                src: None,
                            },
                            // NOT `function('1')`: vim rejects a numeric name there
                            // (E129/E475 — verified), even though its own numbered
                            // functions are named exactly that. The Funcref value is
                            // built directly, through the same `\x01func\x01`
                            // sentinel `Expr::NullFunc` uses.
                            expr: Expr::Str(format!("\u{1}func\u{1}{anon}")),
                        },
                    ));
                }
                // A plain `:function F()` at script level. Vim reads the whole
                // script but EXECUTES it line by line, and `:function` is an
                // ordinary command: `F` does not exist until its `:function`
                // line has run. So this is not hoisted — it goes back into the
                // statement stream and `Compiler::stmt`'s `Stmt::Function` arm
                // stages it into `deferred_funcs` with a register-on-reach
                // `VIML_DEFINE_FUNC`, exactly as a `:function` nested in an
                // `:if` already was.
                //
                // Hoisting made a forward reference succeed:
                // `let F = function('Later')` written above `:function Later()`
                // was accepted, where vim raises
                // `Vim(let):E700: Unknown function: Later` (verified).
                None => top.push((*line, s.clone())),
            }
        } else {
            top.push((*line, s.clone()));
        }
    }
    let mut c = Compiler::new(false, exc);
    c.func_cmdline = func_cmdline;
    // Slot provably-Number top-level locals so a script-level numeric loop
    // JIT-traces too. Sound: `slot_plan` bails on function calls/dynamic and
    // drops any bare name whose `g:`-alias is referenced (a bare script-level
    // name IS `g:name`). Disabled when exceptions add per-statement unwinds.
    if !exc && slot_top {
        (c.slots, c.int_slots) = slot_plan(&top, false);
    }
    c.unwind.push(Vec::new());
    c.compile_stmts(&top)?;
    let frame = c.unwind.pop().expect("top unwind frame");
    let report = c.b.current_pos();
    for j in frame {
        c.b.patch_jump(j, report);
    }
    // `:finish` jumps to the end of the top-level script.
    for j in std::mem::take(&mut c.finishes) {
        c.b.patch_jump(j, report);
    }
    if exc {
        // Any exception that reached the top uncaught is reported here.
        c.emit(Op::CallBuiltin(h::VIML_REPORT_UNCAUGHT, 0));
        c.emit(Op::Pop);
    }
    // Fold in any anonymous functions generated from lambdas (top-level and
    // inside function bodies).
    funcs.extend(LAMBDA_FUNCS.with(|f| std::mem::take(&mut *f.borrow_mut())));
    // Block-level `:function` defs collected while compiling `top` — staged for
    // conditional, run-when-reached registration.
    let deferred_funcs = DEFERRED_FUNCS.with(|f| std::mem::take(&mut *f.borrow_mut()));
    Ok(CompiledProgram {
        main: c.b.build(),
        funcs,
        deferred_funcs,
    })
}

/// Compile a user function body to its own chunk. `:return` jumps to the end;
/// with no explicit return the caller defaults the result to `0`. A pending
/// exception unwinds to the same end (the call returns with it still pending).
fn compile_function_body(
    body: &[(u32, Stmt)],
    exc: bool,
    def_line: u32,
    abort: bool,
    vim9: bool,
    closure: bool,
) -> Result<fusevm::Chunk, VimlError> {
    let mut c = Compiler::new(true, exc);
    // vim numbers a function body's lines from 1 at the first line AFTER the
    // `:function`, so `v:throwpoint` for a throw on the body's third line is
    // `…function F, line 3` and not the file line. The parser records absolute
    // file lines; `line_base` turns them into that relative numbering.
    c.line_base = def_line;
    // Slot-allocate provably-Number locals so a numeric loop body lowers to
    // native ops the JIT can trace. (Exceptions add per-statement unwind
    // CallBuiltins that would break a native loop, so only when `!exc`.)
    // A vim9 `:def` is excluded: there, a bare name that the body never declares
    // with `var` IS the script-level variable, and slotting it would turn every
    // assignment into a frame-local write the script never sees (`def Bump() |
    // counter = counter + 1 | enddef` left `counter` at 0 while vim 9.2 reports
    // 3 after three calls). The parser lowers `var x = 1` and `x = 1` to the same
    // `Stmt::Let`, so the body alone cannot tell a declaration from an
    // assignment — until it can, a def body keeps its names dict-backed and the
    // `b_setvar` script-scope fallback resolves them at run time.
    // A closure (a lambda, or a `:function … closure`) is excluded for the same
    // reason: a bare name it never assigns may be its defining function's.
    if !exc && !vim9 && !closure {
        (c.slots, c.int_slots) = slot_plan(body, true);
    }
    // c: `ex_docmd.c:647-651` resets `did_emsg` after every command of a function
    // body — `&& !func_has_abort(real_cookie)`. Without the `abort` attribute the
    // flag never survives one command, so nothing in a plain body is ever skipped;
    // WITH it the flag persists and `ea.skip` (c:2027-2031) drops the rest of the
    // body. Compiling the whole body at conditional depth 1 is exactly that: every
    // statement gets the skip test and they all land at the body end.
    if abort {
        c.cond_enter();
    }
    c.unwind.push(Vec::new());
    c.compile_stmts(body)?;
    let frame = c.unwind.pop().expect("fn unwind frame");
    if abort {
        c.cond_leave();
    }
    let end = c.b.current_pos();
    for j in std::mem::take(&mut c.returns) {
        c.b.patch_jump(j, end);
    }
    for j in std::mem::take(&mut c.finishes) {
        c.b.patch_jump(j, end);
    }
    for j in frame {
        c.b.patch_jump(j, end);
    }
    Ok(c.b.build())
}

/// Compile a single expression to a chunk that leaves its value on the VM stack
/// (no result-capture builtin). A pure-numeric expression therefore lowers to a
/// fully native-op chunk (`LoadInt`/`Add`/…), which fusevm's JIT compiles to
/// machine code; the value is read from `VMResult::Ok`.
pub fn compile_expr_only(e: &Expr) -> Result<fusevm::Chunk, VimlError> {
    let mut c = Compiler::new(false, false);
    c.expr(e)?;
    Ok(c.b.build())
}

/// Whether the line `s` runs an `:execute` itself — not in a nested block, whose
/// own lines are checked where they are compiled.
fn runs_execute(s: &Stmt) -> bool {
    match s {
        Stmt::Execute(_) => true,
        Stmt::LineGroup(stmts) => stmts.iter().any(runs_execute),
        Stmt::Silent { stmt, .. } => runs_execute(stmt),
        _ => false,
    }
}

/// Whether any statement (recursively) uses `:try` or `:throw`.
fn uses_exceptions(stmts: &[(u32, Stmt)]) -> bool {
    stmts.iter().any(|(_, s)| match s {
        Stmt::Throw(_) | Stmt::Try { .. } => true,
        Stmt::If { arms, else_body } => {
            arms.iter().any(|(_, b)| uses_exceptions(b))
                || else_body.as_deref().is_some_and(uses_exceptions)
        }
        Stmt::While { body, .. } | Stmt::For { body, .. } | Stmt::Function { body, .. } => {
            uses_exceptions(body)
        }
        _ => false,
    })
}

/// Debug build: [`compile_program`] with a `SET_LINENO` marker (source line →
/// the DAP `check_line` hook) emitted before every statement, so the debugger
/// can pause at breakpoints. Used only under `--dap`; the normal
/// `compile_program` carries no markers.
///
/// This is the ordinary compile with one flag set, not a parallel one. It used
/// to be a separate top-level loop that both `continue`d over every
/// `Stmt::Function` and returned a bare `Chunk`, discarding `funcs` /
/// `deferred_funcs` — so under `--dap` no user function was ever defined:
///
/// ```text
/// function! Add(a, b)
///   return a:a + a:b
/// endfunction
/// echo Add(2, 3)
/// echo "done"
/// ```
///
/// printed `5` / `done` in vim (and in `viml` without `--dap`), but only `done`
/// under `--dap` — the `Add(2, 3)` call resolved to nothing. Markers now come
/// from `compile_stmts`, the one place every statement is compiled,
/// so bodies nested in `:if`/`:while`/`:for` and inside `:function` bodies carry
/// them too — which is what makes a breakpoint inside a function reachable.
pub fn compile_program_debug(stmts: &[(u32, Stmt)]) -> Result<CompiledProgram, VimlError> {
    DEBUG_MARKERS.with(|d| d.set(true));
    let r = compile_program(stmts);
    DEBUG_MARKERS.with(|d| d.set(false));
    r
}

struct Compiler {
    b: ChunkBuilder,
    /// Stack of enclosing loops; `break`/`continue` record jump sites here.
    loops: Vec<LoopCtx>,
    /// Enclosing `:try`s of this body, innermost last (see [`FinCtx`]).
    fin: Vec<FinCtx>,
    /// Counter for unique hidden `:for` iterator/index variable names.
    hidden: u32,
    /// Whether we are compiling inside a function body (`:return` is valid).
    in_function: bool,
    /// `:return` jump sites in a function body, patched to the body end.
    returns: Vec<usize>,
    /// `:finish` jump sites, patched to the end of the current chunk (stops
    /// sourcing the rest of the script/file).
    finishes: Vec<usize>,
    /// The program is the text of an `:execute` run from a function body, so a
    /// `:return` in it returns from that function (see
    /// [`compile_program_nested_in_function`]).
    func_cmdline: bool,
    /// Whether the program uses exceptions (`:try`/`:throw`). When set, a
    /// per-statement unwind check is emitted after every statement.
    exc: bool,
    /// Stack of pending exception-unwind jump sites, one frame per exception
    /// boundary (function body, `:try` body, top level); top is innermost.
    unwind: Vec<Vec<usize>>,
    /// Bare locals proven always-Number, mapped to fusevm slot indices. Their
    /// reads/writes lower to native `Op::GetSlot`/`SetSlot` (instead of the
    /// `VIML_GETVAR`/`SETVAR` builtins) so a numeric loop body is CallBuiltin-
    /// free and the tracing JIT can compile it. `int_slots` is the subset proven
    /// always-Integer (the rest may hold Float) — used to keep `range()` bounds
    /// integer, while native `+`/`-`/`*`/compares accept either (fusevm's
    /// `arith_int_fast` promotes int↔float exactly like VimL).
    slots: std::collections::HashMap<String, u16>,
    int_slots: std::collections::HashSet<String>,
    /// The source line of the statement being compiled — written into
    /// `fusevm::Chunk::lines` by [`Compiler::emit`] for every op it produces.
    ///
    /// Inside a function body it is the line *relative to the `:function`*
    /// (`base_line` subtracted), because that is what vim reports:
    /// `v:throwpoint` for a throw on the third line of a body is
    /// `…function F, line 3`, not the file line.
    cur_line: u32,
    /// Subtracted from every statement's absolute source line to produce
    /// [`Compiler::cur_line`]. Zero for a script chunk (absolute file lines) and
    /// the `:function` header's line for a body chunk.
    line_base: u32,
    /// Nesting depth of open `:if`/`:while`/`:for` blocks — this port's stand-in
    /// for `cstack.cs_idx >= 0`.
    ///
    /// c: `do_cmdline` clears `did_emsg` between command lines only while the
    /// condition stack is EMPTY (`ex_docmd.c:448-454`). Inside a conditional the
    /// flag persists, and `ea.skip` (`ex_docmd.c:2027-2031`) then skips every
    /// following command until the outermost conditional closes. So a check is
    /// needed exactly at depth > 0, and the resume point is where the depth
    /// returns to 0.
    cond_depth: u32,
    /// Jump sites of the per-statement `did_emsg` skip test, patched to the end
    /// of the OUTERMOST enclosing conditional (where the C's reset happens).
    aborts: Vec<usize>,
    /// Index of the `Op::Jump` reserved at the head of the outermost conditional,
    /// so a baseline op can be spliced in front of it if the block turns out to
    /// need one. See [`Compiler::cond_leave`].
    cond_head: Option<usize>,
    /// Count of `Op::CallBuiltin`s emitted so far. Every VimL diagnostic is
    /// raised from a builtin (`emsg()` has no native-op caller), so a statement
    /// that did not move this counter cannot have errored and needs no skip test
    /// — which is what keeps a slotted numeric loop body CallBuiltin-free and
    /// therefore JIT-traceable.
    calls: u32,
    /// Debug (`--dap`) build: emit a `SET_LINENO` marker before each statement.
    /// Copied from [`DEBUG_MARKERS`] at construction so every chunk of the
    /// program — main, `:function` bodies, lambda bodies — is marked alike.
    dbg: bool,
}

/// Decide which bare function-local variables can live in fusevm slots.
///
/// Sound & conservative: returns empty (so nothing is slotted and behaviour is
/// unchanged) unless the whole body is free of anything that could reach a
/// variable by name dynamically — function/method calls (the callee may read a
/// global), `:execute`/`:set`, nested `:function`, `:try`, `:for`, or any
/// `:let` target other than a bare name. A name is slotted only if *every*
/// assignment to it provably evaluates to a Number (fixed-point over the set,
/// so `let s = s + i` keeps `s` a slot only while `i` is one too).
type SlotPlan = (
    std::collections::HashMap<String, u16>,
    std::collections::HashSet<String>,
);

fn slot_plan(stmts: &[(u32, Stmt)], in_function: bool) -> SlotPlan {
    use std::collections::{HashMap, HashSet};

    fn is_bare(name: &str) -> bool {
        !name.is_empty()
            && !name.contains(':')
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    }

    // The function-local slot key for a name, or None if it lives in another
    // scope. In a function, `l:name` IS bare `name`, so both share a slot; every
    // other prefix (`g:`/`s:`/`a:`/`b:`/`w:`/`t:`/
    // `v:`) is a distinct dict-backed store and can't be slotted.
    fn slot_key(name: &str, in_function: bool) -> Option<&str> {
        // A script-level name IS `g:name`, and one with an uppercase first
        // letter outlives the script — `:mksession` saves the mixed-case ones
        // and ShaDa the all-caps ones (`var_flavour`) — so it must be stored.
        if is_bare(name)
            && !in_function
            && crate::ported::eval::var_flavour(name)
                != crate::ported::eval::var_flavour_T::VAR_FLAVOUR_DEFAULT
        {
            None
        } else if is_bare(name) {
            Some(name)
        } else if in_function {
            name.strip_prefix("l:").filter(|r| is_bare(r))
        } else {
            None
        }
    }

    // Builtins that look a variable up BY NAME — they can observe even an `l:`
    // slot, so a chunk that calls one must not slot. That includes every
    // builtin that evaluates a String argument as an expression or a command
    // in the caller's scope: `map([1], 'v:val * k')` reads the local `k`.
    fn introspects(name: &str) -> bool {
        matches!(
            name,
            "exists"
                | "eval"
                | "execute"
                | "call"
                | "map"
                | "mapnew"
                | "filter"
                | "foreach"
                | "substitute"
                | "searchpair"
                | "searchpairpos"
        )
    }

    /// A bare SCOPE DICT — `l:`, `g:`, `b:`, `w:`, `t:`, `s:`, `a:`, `v:`.
    ///
    /// Reading one hands the script every variable in that scope at once
    /// (`keys(l:)`, `string(l:)`, `get(l:, 'x')`), so a slotted local would be
    /// MISSING from an answer vim gives in full. A slot has no name and lives
    /// outside the scope dict, so the only sound response is not to slot at all.
    fn is_scope_dict(name: &str) -> bool {
        matches!(name, "l:" | "g:" | "b:" | "w:" | "t:" | "s:" | "a:" | "v:")
    }

    struct Ctx<'a> {
        bail: &'a mut bool,
        assigns: &'a mut HashMap<String, Vec<Expr>>,
        disq: &'a mut HashSet<String>,
        in_function: bool,
        /// Slot keys definitely assigned before the point being walked. A slot
        /// always holds a Number, so a name read before its first assignment —
        /// `let n += 1` with no `n`, a global the script never assigned — must
        /// stay in its dict, where the read is `E121` (or finds the variable).
        init: Vec<String>,
    }

    fn walk_expr(e: &Expr, cx: &mut Ctx) {
        match e {
            // A callee runs in its own frame and cannot see this function's
            // `l:` locals — only a closure defined in THIS body can, and a
            // lambda or a `:function` here already bails — so slotting survives
            // user/value-builtin calls inside a function. At SCRIPT scope a bare
            // var IS `g:`, which a callee can read — bail. Name-introspecting
            // builtins bail in either scope.
            Expr::Call { name, args, .. } => {
                if !cx.in_function || introspects(name) {
                    *cx.bail = true;
                } else {
                    args.iter().for_each(|a| walk_expr(a, cx));
                }
            }
            Expr::Method { base, name, args } => {
                if !cx.in_function || introspects(name) {
                    *cx.bail = true;
                } else {
                    walk_expr(base, cx);
                    args.iter().for_each(|a| walk_expr(a, cx));
                }
            }
            Expr::Arith { lhs, rhs, .. } | Expr::Compare { lhs, rhs, .. } => {
                walk_expr(lhs, cx);
                walk_expr(rhs, cx);
            }
            Expr::Unary { expr, .. } => walk_expr(expr, cx),
            Expr::And(a, b) | Expr::Or(a, b) | Expr::Coalesce(a, b) => {
                walk_expr(a, cx);
                walk_expr(b, cx);
            }
            Expr::Ternary {
                cond,
                then,
                otherwise,
            } => {
                walk_expr(cond, cx);
                walk_expr(then, cx);
                walk_expr(otherwise, cx);
            }
            Expr::Index { base, index } => {
                walk_expr(base, cx);
                walk_expr(index, cx);
            }
            Expr::Slice { base, from, to } => {
                walk_expr(base, cx);
                if let Some(f) = from {
                    walk_expr(f, cx);
                }
                if let Some(t) = to {
                    walk_expr(t, cx);
                }
            }
            Expr::Var(name) if is_scope_dict(name) => *cx.bail = true,
            Expr::Var(name) => {
                if let Some(key) = slot_key(name, cx.in_function) {
                    if !cx.init.iter().any(|k| k == key) {
                        cx.disq.insert(key.to_string());
                    }
                }
            }
            // A lambda may be a closure over this function's locals, reading
            // and writing them by name after the fact: a slot is invisible to it.
            Expr::Lambda { .. } => *cx.bail = true,
            // A call through a value, or a Dict member's function: the same rule
            // as a named call, and the operands may themselves hold one
            // (`eval('{-> q}')()` reads `q` by name through the inner call).
            Expr::CallExpr { callee, args } => {
                if !cx.in_function {
                    *cx.bail = true;
                } else {
                    walk_expr(callee, cx);
                    args.iter().for_each(|a| walk_expr(a, cx));
                }
            }
            Expr::MemberCall { base, args, .. } => {
                if !cx.in_function {
                    *cx.bail = true;
                } else {
                    walk_expr(base, cx);
                    args.iter().for_each(|a| walk_expr(a, cx));
                }
            }
            Expr::Member { base, .. } => walk_expr(base, cx),
            Expr::Interp(segs) => segs.iter().for_each(|s| walk_expr(s, cx)),
            Expr::ScriptErrorGuard { inner, .. } => walk_expr(inner, cx),
            Expr::List(items) => items.iter().for_each(|i| walk_expr(i, cx)),
            Expr::Dict(pairs) => pairs.iter().for_each(|(k, v)| {
                walk_expr(k, cx);
                walk_expr(v, cx);
            }),
            _ => {}
        }
    }

    fn walk(stmts: &[(u32, Stmt)], cx: &mut Ctx) {
        // An assignment makes its name initialized for the rest of THIS block
        // only: after a loop or a conditional arm it may not have run.
        let init_depth = cx.init.len();
        walk_block(stmts, cx);
        cx.init.truncate(init_depth);
    }

    fn walk_block(stmts: &[(u32, Stmt)], cx: &mut Ctx) {
        for (_, s) in stmts {
            if *cx.bail {
                return;
            }
            walk_stmt(s, cx);
        }
    }

    fn walk_stmt(s: &Stmt, cx: &mut Ctx) {
        {
            match s {
                Stmt::Function { .. }
                | Stmt::Execute(_)
                | Stmt::Set(_)
                | Stmt::Map(_)
                | Stmt::CommandDef(_)
                | Stmt::CommandDel(_)
                | Stmt::DelFunction(_)
                | Stmt::UserCmd(_)
                | Stmt::Autocmd(_)
                | Stmt::Augroup(_)
                | Stmt::Doautocmd(_)
                | Stmt::ExCmd(_)
                | Stmt::Try { .. } => *cx.bail = true,
                // `for VAR in range(...)` keeps its var slottable (range yields
                // Numbers) — bare or, in a function, `l:`-scoped; recurse the body.
                Stmt::For {
                    vars: ForVars::One(name),
                    iter,
                    body,
                } if slot_key(name, cx.in_function).is_some()
                    && matches!(iter, Expr::Call { name: f, .. } if f == "range") =>
                {
                    if let Expr::Call { args, .. } = iter {
                        args.iter().for_each(|a| walk_expr(a, cx));
                    }
                    let key = slot_key(name, cx.in_function).unwrap().to_string();
                    cx.assigns
                        .entry(key.clone())
                        .or_default()
                        .push(Expr::Number(0));
                    let init_depth = cx.init.len();
                    cx.init.push(key);
                    walk(body, cx);
                    cx.init.truncate(init_depth);
                }
                // Any other for-loop: the loop var(s) take non-Number values —
                // disqualify them (by slot key) — but DON'T bail; sibling numeric
                // loops can still slot.
                Stmt::For { vars, iter, body } => {
                    walk_expr(iter, cx);
                    let mut disq_var = |n: &str| {
                        cx.disq
                            .insert(slot_key(n, cx.in_function).unwrap_or(n).to_string());
                    };
                    match vars {
                        ForVars::One(n) => disq_var(n),
                        ForVars::List { names, rest } => {
                            names.iter().chain(rest).for_each(|n| disq_var(n))
                        }
                    }
                    walk(body, cx);
                }
                Stmt::Let {
                    target: LetTarget::Var(name),
                    expr,
                } => {
                    walk_expr(expr, cx);
                    if let Some(key) = slot_key(name, cx.in_function) {
                        cx.assigns
                            .entry(key.to_string())
                            .or_default()
                            .push(expr.clone());
                        cx.init.push(key.to_string());
                    }
                }
                Stmt::Let { .. } => *cx.bail = true, // non-bare target: be safe
                // `:lockvar`/`:unlockvar` names a variable by string at run time,
                // which a slot has no name for — keep everything in `g:`.
                Stmt::LockVar { .. } => *cx.bail = true,
                // `:const` locks what it assigns, by name, at run time.
                Stmt::Const { .. } => *cx.bail = true,
                // `:unlet` removes a variable by NAME at run time (`do_unlet`), which
                // a slot does not have: `let d = 5 | unlet d` must find `d`.
                Stmt::Unlet { .. } => *cx.bail = true,
                // The commands of a `|`-separated line run in order in this same
                // block, and `:silent` wraps one command: both are walked through.
                Stmt::LineGroup(inner) => {
                    for s in inner {
                        if *cx.bail {
                            return;
                        }
                        walk_stmt(s, cx);
                    }
                }
                Stmt::Silent { stmt, .. } => walk_stmt(stmt, cx),
                Stmt::Echo(es) | Stmt::Echon(es) | Stmt::EchoErr(es) => {
                    es.iter().for_each(|e| walk_expr(e, cx))
                }
                Stmt::LetList(vs) => vs.iter().for_each(|(_, e)| walk_expr(e, cx)),
                // `:defer`'s arguments are evaluated where they are written, so
                // they are walked like any other statement's expression.
                Stmt::Call(e) | Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Defer(e) => walk_expr(e, cx),
                Stmt::Return(Some(e)) => walk_expr(e, cx),
                Stmt::While { cond, body } => {
                    walk_expr(cond, cx);
                    walk(body, cx);
                }
                Stmt::If { arms, else_body } => {
                    for (c, b) in arms {
                        walk_expr(c, cx);
                        walk(b, cx);
                    }
                    if let Some(b) = else_body {
                        walk(b, cx);
                    }
                }
                _ => {}
            }
        }
    }

    let mut assigns: HashMap<String, Vec<Expr>> = HashMap::new();
    let mut bail = false;
    let mut disq: HashSet<String> = HashSet::new();
    walk(
        stmts,
        &mut Ctx {
            bail: &mut bail,
            assigns: &mut assigns,
            disq: &mut disq,
            in_function,
            init: Vec::new(),
        },
    );
    if bail || assigns.is_empty() {
        return (HashMap::new(), HashSet::new());
    }

    // A tree is a Number (`is_int=false`) / an Integer (`is_int=true`) when every
    // leaf is a matching literal or a (still-candidate) slot var of that kind.
    // `+ - * / %` of Numbers are Numbers; only `/`,`%` and Float leaves break
    // integer-ness. Concat is a string op — never numeric. A shift can fail on
    // Number operands (`1 << -1`, E1283), so it is never proved numeric either.
    fn rhs_kind(e: &Expr, set: &HashSet<String>, is_int: bool, in_function: bool) -> bool {
        match e {
            Expr::Number(_) => true,
            Expr::Float(_) => !is_int,
            Expr::Var(n) => slot_key(n, in_function).is_some_and(|k| set.contains(k)),
            Expr::Arith { op, lhs, rhs, .. } => {
                !matches!(op, ArithOp::Concat | ArithOp::ShiftL | ArithOp::ShiftR)
                    && rhs_kind(lhs, set, is_int, in_function)
                    && rhs_kind(rhs, set, is_int, in_function)
            }
            Expr::Unary {
                op: UnaryOp::Neg | UnaryOp::Plus,
                expr,
            } => rhs_kind(expr, set, is_int, in_function),
            // Logical-not yields Integer 0/1 when its operand is integer.
            Expr::Unary {
                op: UnaryOp::Not,
                expr,
            } => rhs_kind(expr, set, true, in_function),
            // The bitwise builtins always yield an Integer (so valid in either
            // pass) when every argument is itself provably integer.
            Expr::Call { name, args, .. } if bitwise_native_op(name, args.len()).is_some() => {
                args.iter().all(|a| rhs_kind(a, set, true, in_function))
            }
            // A ternary's kind is its branches' kind (the test is irrelevant).
            Expr::Ternary {
                then, otherwise, ..
            } => {
                rhs_kind(then, set, is_int, in_function)
                    && rhs_kind(otherwise, set, is_int, in_function)
            }
            // A comparison reifies to Integer 0/1 when both operands are numeric
            // (so it lowers natively); valid in either pass.
            Expr::Compare { op, lhs, rhs, .. } if Compiler::native_cmp(*op).is_some() => {
                rhs_kind(lhs, set, false, in_function) && rhs_kind(rhs, set, false, in_function)
            }
            _ => false,
        }
    }

    // Fixed-point over the candidate set for a given kind (numeric, or integer).
    let fixed_point = |is_int: bool| -> HashSet<String> {
        let mut set: HashSet<String> = assigns.keys().cloned().collect();
        loop {
            let mut changed = false;
            for name in set.iter().cloned().collect::<Vec<_>>() {
                if !assigns[&name]
                    .iter()
                    .all(|rhs| rhs_kind(rhs, &set, is_int, in_function))
                {
                    set.remove(&name);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        set
    };
    // `num` = slottable (always a Number); `int_only` ⊆ `num` = always Integer.
    let mut num = fixed_point(false);
    let int_only = fixed_point(true);

    // A bare name at script level IS `g:name`; in a function it IS `l:name`.
    // If any scoped alias of a candidate is referenced, slotting it would
    // desync the dict-backed form — drop those candidates.
    // `l:` in a function names the slot itself, not a separate store, so it is
    // not a disqualifying alias there; every other prefix still is.
    fn scoped_var(n: &str, in_function: bool, out: &mut HashSet<String>) {
        if let Some((pre, suf)) = n.rsplit_once(':') {
            if !(in_function && pre == "l") {
                out.insert(suf.to_string());
            }
        }
    }
    fn scoped_e(e: &Expr, in_function: bool, out: &mut HashSet<String>) {
        match e {
            Expr::Var(n) => scoped_var(n, in_function, out),
            Expr::Arith { lhs, rhs, .. } | Expr::Compare { lhs, rhs, .. } => {
                scoped_e(lhs, in_function, out);
                scoped_e(rhs, in_function, out);
            }
            Expr::Unary { expr, .. } => scoped_e(expr, in_function, out),
            Expr::And(a, b) | Expr::Or(a, b) | Expr::Coalesce(a, b) => {
                scoped_e(a, in_function, out);
                scoped_e(b, in_function, out);
            }
            Expr::Ternary {
                cond,
                then,
                otherwise,
            } => {
                scoped_e(cond, in_function, out);
                scoped_e(then, in_function, out);
                scoped_e(otherwise, in_function, out);
            }
            Expr::Index { base, index } => {
                scoped_e(base, in_function, out);
                scoped_e(index, in_function, out);
            }
            Expr::Slice { base, from, to } => {
                scoped_e(base, in_function, out);
                if let Some(f) = from {
                    scoped_e(f, in_function, out);
                }
                if let Some(t) = to {
                    scoped_e(t, in_function, out);
                }
            }
            Expr::List(xs) => xs.iter().for_each(|x| scoped_e(x, in_function, out)),
            Expr::Dict(ps) => ps.iter().for_each(|(k, v)| {
                scoped_e(k, in_function, out);
                scoped_e(v, in_function, out);
            }),
            Expr::Call { args, .. } => args.iter().for_each(|a| scoped_e(a, in_function, out)),
            Expr::Interp(segs) => segs.iter().for_each(|s| scoped_e(s, in_function, out)),
            _ => {}
        }
    }
    fn scoped_s(stmts: &[(u32, Stmt)], in_function: bool, out: &mut HashSet<String>) {
        for (_, s) in stmts {
            match s {
                Stmt::Let {
                    target: LetTarget::Var(n),
                    expr,
                } => {
                    scoped_var(n, in_function, out);
                    scoped_e(expr, in_function, out);
                }
                Stmt::Echo(es) | Stmt::Echon(es) | Stmt::EchoErr(es) => {
                    es.iter().for_each(|e| scoped_e(e, in_function, out))
                }
                Stmt::LetList(vs) => vs.iter().for_each(|(_, e)| scoped_e(e, in_function, out)),
                Stmt::Call(e) | Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Defer(e) => {
                    scoped_e(e, in_function, out)
                }
                Stmt::Return(Some(e)) => scoped_e(e, in_function, out),
                Stmt::While { cond, body } => {
                    scoped_e(cond, in_function, out);
                    scoped_s(body, in_function, out);
                }
                Stmt::For { iter, body, .. } => {
                    scoped_e(iter, in_function, out);
                    scoped_s(body, in_function, out);
                }
                Stmt::If { arms, else_body } => {
                    for (c, b) in arms {
                        scoped_e(c, in_function, out);
                        scoped_s(b, in_function, out);
                    }
                    if let Some(b) = else_body {
                        scoped_s(b, in_function, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut scoped = HashSet::new();
    scoped_s(stmts, in_function, &mut scoped);
    num.retain(|n| !scoped.contains(n) && !disq.contains(n));

    let mut names: Vec<String> = num.iter().cloned().collect();
    names.sort();
    let slots: HashMap<String, u16> = names
        .into_iter()
        .enumerate()
        .map(|(i, n)| (n, i as u16))
        .collect();
    // Integer subset, restricted to the names that actually got slotted.
    let int_slots: HashSet<String> = int_only
        .into_iter()
        .filter(|n| slots.contains_key(n))
        .collect();
    (slots, int_slots)
}

impl Compiler {
    fn new(in_function: bool, exc: bool) -> Self {
        Compiler {
            b: ChunkBuilder::new(),
            loops: Vec::new(),
            fin: Vec::new(),
            hidden: 0,
            in_function,
            returns: Vec::new(),
            finishes: Vec::new(),
            func_cmdline: false,
            exc,
            unwind: Vec::new(),
            slots: std::collections::HashMap::new(),
            int_slots: std::collections::HashSet::new(),
            cur_line: 0,
            line_base: 0,
            cond_depth: 0,
            aborts: Vec::new(),
            cond_head: None,
            calls: 0,
            dbg: DEBUG_MARKERS.with(|d| d.get()),
        }
    }

    /// Compile a statement sequence, emitting an unwind check after each
    /// statement when exceptions are in play (so a pending exception jumps to
    /// the innermost boundary).
    /// The ex-command name a statement raises errors under — Vim tags an
    /// error-turned-exception with it (`Vim(echo):E730: …`).
    ///
    /// Block openers are named too. Verified against vim 9.2:
    ///
    /// ```text
    /// try | if [][0] | endif | catch | echo v:exception | endtry
    /// " Vim(if):E684: List index out of range: 0
    /// ```
    ///
    /// and the same for `:while` and `:for`. Before they were listed, the tag on
    /// such an error was whatever the *previous* statement had set — `Vim:` at
    /// the top of a script, `Vim(echo)` after an `:echo`.
    ///
    /// The marker this drives is also where the statement's source line is
    /// recorded (`fusevm_bridge::b_set_cmdname`), so a statement kind missing
    /// here would report the previous statement's line in `v:throwpoint`.
    fn stmt_cmdname(s: &Stmt) -> Option<&'static str> {
        Some(match s {
            Stmt::Echo(_) => "echo",
            Stmt::LetList(_) => "let",
            Stmt::Echon(_) => "echon",
            Stmt::EchoErr(_) => "echoerr",
            Stmt::Let { .. } => "let",
            Stmt::Const { .. } => "const",
            Stmt::Call(_) => "call",
            Stmt::Defer(_) => "defer",
            Stmt::Return(_) => "return",
            Stmt::Break(_) => "break",
            Stmt::Continue(_) => "continue",
            Stmt::Throw(_) => "throw",
            Stmt::Execute(_) => "execute",
            Stmt::Unlet { .. } => "unlet",
            Stmt::LockVar { lock: true, .. } => "lockvar",
            Stmt::LockVar { lock: false, .. } => "unlockvar",
            Stmt::If { .. } => "if",
            Stmt::While { .. } => "while",
            Stmt::For { .. } => "for",
            Stmt::Try { .. } => "try",
            Stmt::Source(_) => "source",
            Stmt::Set(_) => "set",
            Stmt::Function { .. } => "function",
            Stmt::DelFunction(_) => "delfunction",
            Stmt::Finish => "finish",
            Stmt::CommandDef(_) => "command",
            Stmt::CommandDel(_) => "delcommand",
            Stmt::Autocmd(_) => "autocmd",
            Stmt::Augroup(_) => "augroup",
            Stmt::Doautocmd(_) => "doautocmd",
            Stmt::Colorscheme(_) => "colorscheme",
            Stmt::Highlight(_) => "highlight",
            Stmt::Syntax(_) => "syntax",
            Stmt::Filetype(_) => "filetype",
            // `:silent CMD` is a modifier, not a command: vim tags an error
            // inside it with the command it modifies (`silent echo [][0]` is
            // `Vim(echo):E684`, verified), so look through it.
            Stmt::Silent { stmt, .. } => return Self::stmt_cmdname(stmt),
            _ => return None,
        })
    }

    /// Which rule decides whether this command sets `eap->nextcmd`, and so
    /// whether a failure of its own drops the rest of a `|`-separated line —
    /// see `fusevm_bridge::b_line_abort`. `:silent` is a modifier, not a
    /// command, so it is looked through exactly as `stmt_cmdname` does.
    fn nextcmd_rule(s: &Stmt) -> i64 {
        match s {
            // c: `ex_call` is the one that sets `eap->nextcmd` only when the
            // call SUCCEEDED, so a failed call drops the rest of the line even
            // though `get_func_arguments` had consumed the `(…)`.
            Stmt::Call(_) => 2,
            Stmt::Silent { stmt, .. } => Self::nextcmd_rule(stmt),
            // Everything else — `:echo` included (`vendor/eval.c:6187` sets it
            // from wherever the argument loop stopped) — drops the line only
            // when the PARSE aborted mid-expression.
            _ => 1,
        }
    }

    fn compile_stmts(&mut self, stmts: &[(u32, Stmt)]) -> Result<(), VimlError> {
        for (line, s) in stmts {
            // The line every op emitted for this statement is tagged with. It
            // costs no bytecode: `fusevm::ChunkBuilder::emit` already takes a
            // line and the chunk already keeps the vector.
            self.cur_line = line.saturating_sub(self.line_base);
            // Debug (`--dap`) build only: hand the debugger the statement's
            // ABSOLUTE file line before it runs — `cur_line` is relative inside a
            // function body, but a DAP client sets breakpoints on file lines.
            if self.dbg {
                self.emit(Op::LoadInt(*line as i64));
                self.emit(Op::CallBuiltin(h::VIML_SET_LINENO, 1));
                self.emit(Op::Pop);
            }
            // Only programs that use exceptions can observe the tag, and they are
            // the only ones that pay for it.
            if self.exc {
                if let Some(cmd) = Self::stmt_cmdname(s) {
                    self.load_str(cmd);
                    self.emit(Op::CallBuiltin(h::VIML_SET_CMDNAME, 1));
                    self.emit(Op::Pop);
                }
            }
            let calls_before = self.calls;
            self.stmt(s)?;
            if (self.in_function || self.func_cmdline) && runs_execute(s) {
                self.exec_return_check();
            }
            if self.exc {
                self.emit(Op::CallBuiltin(h::VIML_CHECK_EXC, 0));
                let j = self.emit(Op::JumpIfTrue(0));
                if let Some(frame) = self.unwind.last_mut() {
                    frame.push(j);
                }
            }
            // c: `ea.skip` (`ex_docmd.c:2027-2031`) — every command after one that
            // reported an error is skipped while `did_emsg` holds, and inside a
            // conditional nothing resets it. Emitted only when the statement
            // actually reached a builtin: `emsg()` is unreachable from the native
            // ops, so a fully-lowered numeric statement cannot have set the flag.
            // The exception check above runs first, so a pending throw still
            // unwinds through its own frame rather than taking this exit.
            if self.calls > calls_before {
                self.emit_block_abort_check(0);
            }
        }
        Ok(())
    }
}

/// Pending `break`/`continue` jump sites for one enclosing loop, patched when
/// the loop's bytecode is finished.
#[derive(Default)]
struct LoopCtx {
    breaks: Vec<usize>,
    continues: Vec<usize>,
}

/// A `:try` being compiled, for the `:return`/`:break`/`:continue` that leave it.
///
/// c: `ex_return`, `ex_break` and `ex_continue` do not jump: they mark the
/// try-conditionals they cross `CSF_PENDING` (`cleanup_conditionals`), and each
/// `:finally` on the way runs before `ex_endtry` resumes the pending command
/// (`rewind_conditionals`). Every `:endtry` crossed also drops the try level —
/// a `:return` out of a `:try` that skipped that left every later error
/// turned into an exception.
#[derive(Default)]
struct FinCtx {
    /// `loops.len()` when the `:try` opened: a `:break` crosses this `:try` only
    /// when its loop is outside it.
    loop_depth: usize,
    /// Jumps from a leaving command to the try's pending entry.
    sites: Vec<usize>,
    /// Which pending kinds (`EXIT_*`) were recorded.
    kinds: Vec<i64>,
}

/// Pending-command codes for `VIML_PENDING_PUSH`. 0 is "nothing pending".
const EXIT_RETURN: i64 = 1;
const EXIT_BREAK: i64 = 2;
const EXIT_CONTINUE: i64 = 3;

impl Compiler {
    fn emit(&mut self, op: Op) -> usize {
        if matches!(op, Op::CallBuiltin(..)) {
            self.calls += 1;
        }
        self.b.emit(op, self.cur_line)
    }

    fn load_str(&mut self, s: &str) {
        let idx = self.b.add_constant(Value::str(s));
        self.emit(Op::LoadConst(idx));
    }

    /// A string literal whose bytes are not valid UTF-8 (`"\xc3"` is the single
    /// byte `c3` in vim). A bytecode constant is a `fusevm::Value` and
    /// `Value::Str` is an `Arc<String>`, so the bytes cannot be one; the hex
    /// form can, and [`h::VIML_BYTES`] turns it back at run time.
    fn load_bytes(&mut self, bytes: &[u8]) {
        let mut hex = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            hex.push_str(&format!("{b:02x}"));
        }
        self.load_str(&hex);
        self.emit(Op::CallBuiltin(h::VIML_BYTES, 1));
    }

    /// The `u8` operand of a `CallBuiltin`/`Call` opcode.
    ///
    /// Overflowing it is a bytecode-encoding limit, but the CONDITION — a call
    /// with more arguments than can be passed — is one both engines already
    /// name: `E740: Too many arguments for function %s` (measured, a 300-argument
    /// call to a `...` function). Reporting `E118` for it was wrong twice over:
    /// E118 is "too many arguments for function: %s" (the arity check against a
    /// declared signature, which this is not) and the text was this crate's own
    /// phase numbering, which no vim ever printed.
    fn argc(n: usize) -> Result<u8, VimlError> {
        u8::try_from(n)
            .map_err(|_| VimlError::msg("E740: Too many arguments for function".to_string()))
    }

    fn stmt(&mut self, s: &Stmt) -> Result<(), VimlError> {
        match s {
            Stmt::Echo(args) => self.echo(args, h::VIML_ECHO),
            Stmt::Echon(args) => self.echo(args, h::VIML_ECHON),
            Stmt::EchoErr(args) => self.echoerr(args),
            Stmt::LetList(vars) => self.let_list(vars),
            Stmt::Let { target, expr } => self.let_stmt(target, expr),
            Stmt::Const { target, expr } => self.const_stmt(target, expr),
            Stmt::Call(e) => {
                // Mark the error count first, exactly as `Stmt::Expr` and `:echo`
                // do: `:call` is its own ex-command, so a deferred `VIML_RAISE`
                // inside it (a wrong-arity builtin, say) must compare against
                // THIS command's snapshot. Without the mark it compared against
                // whichever statement last took one, so `call abs()` stayed
                // silent after any earlier error had bumped the counter —
                // `assert_fails('call abs()', 'E119')` passed alone and failed
                // when a passing `assert_fails` ran before it.
                self.emit(Op::CallBuiltin(h::VIML_ERR_MARK, 0));
                self.emit(Op::Pop);
                self.expr(e)?;
                self.emit(Op::Pop);
                Ok(())
            }
            // `:defer Func(args)` — evaluate the arguments *here* and stash the
            // call for the frame's exit, rather than emitting the call itself.
            // vim evaluates a deferred call's arguments at the `:defer`, so the
            // argument expressions compile inline exactly as `:call`'s would; only
            // the invocation is postponed.
            Stmt::Defer(e) => {
                let Expr::Call { name, args, .. } = e else {
                    return Err(VimlError(
                        // c: `:defer` takes a call and nothing else; both engines answer
                        // `E129: Function name required` (measured on `defer 5`).
                        "E129: Function name required".to_string(),
                    ));
                };
                self.load_str(name);
                for a in args {
                    self.expr(a)?;
                }
                // +1 for the name pushed under the arguments.
                self.emit(Op::CallBuiltin(h::VIML_DEFER, args.len() as u8 + 1));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::Expr(e) => {
                // Mark the error count first (as `:echo` does) so a deferred
                // `VIML_RAISE` inside the expression can tell whether an
                // earlier operand already errored — Vim's single-pass eval
                // aborts on the first error, so only the first is reported.
                self.emit(Op::CallBuiltin(h::VIML_ERR_MARK, 0));
                self.emit(Op::Pop);
                self.expr(e)?;
                self.emit(Op::CallBuiltin(h::VIML_SET_RESULT, 1));
                self.emit(Op::Pop);
                Ok(())
            }
            // The three cstack-pushing block commands. `cond_enter`/`cond_leave`
            // bracket them so an error inside skips to the end of the OUTERMOST
            // one — see [`Compiler::cond_depth`].
            Stmt::If { arms, else_body } => {
                self.cond_enter();
                let r = self.if_stmt(arms, else_body);
                self.cond_leave();
                r
            }
            Stmt::While { cond, body } => {
                self.cond_enter();
                let r = self.while_stmt(cond, body);
                self.cond_leave();
                r
            }
            Stmt::For { vars, iter, body } => {
                self.cond_enter();
                let r = self.for_stmt(vars, iter, body);
                self.cond_leave();
                r
            }

            Stmt::Execute(args) => {
                // c: `ex_execute()` — an argument whose `eval1()` fails ends the
                // command (`ret = FAIL; break;`) and nothing is executed:
                // `execute 'echo' nosuch` is the E121 alone. Each failure path
                // drops the value it left and the arguments already on the stack.
                let mut failed = Vec::new();
                for (i, a) in args.iter().enumerate() {
                    self.emit(Op::CallBuiltin(h::VIML_ERR_MARK, 0));
                    self.emit(Op::Pop);
                    self.expr(a)?;
                    self.emit(Op::CallBuiltin(h::VIML_ERR_SINCE, 0));
                    let ok = self.emit(Op::JumpIfFalse(0));
                    for _ in 0..=i {
                        self.emit(Op::Pop);
                    }
                    failed.push(self.emit(Op::Jump(0)));
                    let here = self.b.current_pos();
                    self.b.patch_jump(ok, here);
                }
                // From a function body, the text may `:return` from the function;
                // `compile_stmts` acts on that at the end of the line.
                let id = if self.in_function || self.func_cmdline {
                    h::VIML_EXEC_STMT_FN
                } else {
                    h::VIML_EXEC_STMT
                };
                self.emit(Op::CallBuiltin(id, Self::argc(args.len())?));
                self.emit(Op::Pop);
                let end = self.b.current_pos();
                for j in failed {
                    self.b.patch_jump(j, end);
                }
                Ok(())
            }
            Stmt::Set(args) => {
                self.load_str(args);
                self.emit(Op::CallBuiltin(h::VIML_SET, 1));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::Map(line) => {
                self.load_str(line);
                self.emit(Op::CallBuiltin(h::VIML_MAP, 1));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::CommandDef(args) => {
                self.load_str(args);
                self.emit(Op::CallBuiltin(h::VIML_COMMAND, 1));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::CommandDel(name) => {
                self.load_str(name);
                self.emit(Op::CallBuiltin(h::VIML_DELCOMMAND, 1));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::DelFunction(arg) => {
                self.load_str(arg);
                self.emit(Op::CallBuiltin(h::VIML_DELFUNCTION, 1));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::UserCmd(line) => {
                self.load_str(line);
                self.emit(Op::CallBuiltin(h::VIML_USERCMD, 1));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::Autocmd(args) => {
                self.load_str(args);
                self.emit(Op::CallBuiltin(h::VIML_AUTOCMD, 1));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::Augroup(name) => {
                self.load_str(name);
                self.emit(Op::CallBuiltin(h::VIML_AUGROUP, 1));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::Doautocmd(args) => {
                self.load_str(args);
                self.emit(Op::CallBuiltin(h::VIML_DOAUTOCMD, 1));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::ExCmd(line) => {
                self.load_str(line);
                self.emit(Op::CallBuiltin(h::VIML_EXCMD, 1));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::Colorscheme(name) => {
                self.load_str(name);
                self.emit(Op::CallBuiltin(h::VIML_COLORSCHEME, 1));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::Highlight(args) => {
                self.load_str(args);
                self.emit(Op::CallBuiltin(h::VIML_HIGHLIGHT, 1));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::Syntax(args) => {
                self.load_str(args);
                self.emit(Op::CallBuiltin(h::VIML_SYNTAX, 1));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::Filetype(args) => {
                self.load_str(args);
                self.emit(Op::CallBuiltin(h::VIML_FILETYPE, 1));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::Source(path) => {
                self.load_str(path);
                self.emit(Op::CallBuiltin(h::VIML_SOURCE, 1));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::Unlet { args, bang } => {
                // c: `ex_unletlock` — once an argument has failed (`error =
                // true`), the rest are only parsed, never unlet, so
                // `unlet nosuch m[0]` reports E108 alone. The error count at the
                // start stays on the stack while the arguments run.
                let several = args.len() > 1;
                if several {
                    self.emit(Op::CallBuiltin(h::VIML_ERR_COUNT, 0));
                }
                let mut skips = Vec::new();
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.emit(Op::Dup);
                        self.emit(Op::CallBuiltin(h::VIML_ERRS_AFTER, 1));
                        skips.push(self.emit(Op::JumpIfTrue(0)));
                    }
                    match arg {
                        // c: `do_unlet(lp->ll_name, lp->ll_name_len, eap->forceit)`
                        // — `forceit` reaches the leaf, where it decides between
                        // "silently OK" and E108.
                        UnletArg::Name(name) => {
                            self.load_str(name);
                            self.emit(Op::LoadInt(*bang as i64));
                            self.emit(Op::CallBuiltin(h::VIML_UNLET, 2));
                        }
                        // `unlet base[index]` / `unlet base.key` — push the
                        // container then the index; the bridge removes the
                        // element in place (mirroring `do_unlet_var()`).
                        UnletArg::Item { base, index, src } => {
                            self.expr(base)?;
                            self.expr(index)?;
                            self.load_str(src);
                            self.emit(Op::CallBuiltin(h::VIML_UNLET_INDEX, 3));
                        }
                        // `unlet base[i:j]` — the container, both indexes (0 for
                        // an omitted one), which were omitted, and the text.
                        UnletArg::Range {
                            base,
                            idx1,
                            idx2,
                            src,
                        } => {
                            self.expr(base)?;
                            for idx in [idx1, idx2] {
                                match idx {
                                    Some(e) => self.expr(e)?,
                                    None => {
                                        self.emit(Op::LoadInt(0));
                                    }
                                }
                            }
                            let empty = (idx1.is_none() as i64) | ((idx2.is_none() as i64) << 1);
                            self.emit(Op::LoadInt(empty));
                            self.load_str(src);
                            self.emit(Op::CallBuiltin(h::VIML_UNLET_RANGE, 5));
                        }
                    }
                    self.emit(Op::Pop);
                }
                let end = self.b.current_pos();
                for j in skips {
                    self.b.patch_jump(j, end);
                }
                if several {
                    self.emit(Op::Pop);
                }
                Ok(())
            }
            // `:lockvar`/`:unlockvar` — the raw argument plus the two flags; the
            // bridge rebuilds the `exarg_T` the ported `ex_lockvar` parses.
            Stmt::LockVar { arg, bang, lock } => {
                self.load_str(arg);
                self.load_str(if *bang { "!" } else { "" });
                self.load_str(if *lock { "lock" } else { "unlock" });
                self.emit(Op::CallBuiltin(h::VIML_LOCKVAR, 3));
                self.emit(Op::Pop);
                Ok(())
            }
            // c: `ex_break` with no loop on the condition stack sets
            // `eap->errmsg`, which `do_one_cmd` reports at RUN time with the
            // command text appended; the script carries on after it.
            Stmt::Break(text) => {
                if self.loops.is_empty() {
                    self.raise_cmd(&format!("E587: :break without :while or :for: {text}"));
                    return Ok(());
                }
                self.exit_jump(EXIT_BREAK);
                Ok(())
            }
            Stmt::Finish => {
                // `:finish` stops sourcing the rest of the script/file: jump to
                // the end of the current chunk (patched in compile_program /
                // compile_function_body). Unwinds cleanly out of :if / :while.
                let j = self.emit(Op::Jump(0));
                self.finishes.push(j);
                Ok(())
            }
            Stmt::Continue(text) => {
                if self.loops.is_empty() {
                    self.raise_cmd(&format!("E586: :continue without :while or :for: {text}"));
                    return Ok(());
                }
                self.exit_jump(EXIT_CONTINUE);
                Ok(())
            }
            Stmt::Return(expr) => {
                // c: `ex_return` outside a function is `emsg(e_return_not_inside_function)`
                // at run time, before the argument is evaluated, and without
                // the command text; the script carries on after it.
                if !self.in_function && !self.func_cmdline {
                    self.raise_cmd("E133: :return not inside a function");
                    return Ok(());
                }
                // c: `ex_return` returns the evaluated value only when `eval0()`
                // succeeded; on FAIL it still returns, but through
                // `do_return(…, NULL)` — i.e. with the value 0. Mark the
                // evaluator's failure count so `VIML_SET_RETURN` can tell
                // (`function F() | return [1] . 'x' | endfunction` yields 0 in both
                // engines, not the recovered `'0x'`).
                self.emit(Op::CallBuiltin(h::VIML_ERR_MARK, 0));
                self.emit(Op::Pop);
                match expr {
                    Some(e) => self.expr(e)?,
                    None => {
                        self.emit(Op::LoadInt(0)); // `:return` with no expr → 0
                    }
                }
                self.emit(Op::CallBuiltin(h::VIML_SET_RETURN, 1));
                self.emit(Op::Pop);
                if !self.in_function {
                    // An `:execute` of the function: the value is in the
                    // function's return slot; flag it for the `:execute` that
                    // ran this text and stop running the text.
                    self.emit(Op::LoadInt(EXEC_RETURNED_SET));
                    self.emit(Op::CallBuiltin(h::VIML_EXEC_RETURNED, 1));
                    self.emit(Op::Pop);
                    let j = self.emit(Op::Jump(0));
                    self.finishes.push(j);
                    return Ok(());
                }
                self.exit_jump(EXIT_RETURN);
                Ok(())
            }
            Stmt::Function {
                name,
                args,
                defaults,
                body,
                bang,
                vim9,
                dict,
                abort,
                closure,
            } => {
                // A `:function` reached HERE (in `stmt`, not `compile_program`'s
                // top-level loop) is nested inside a control-flow block and/or
                // another function's body. Vim treats BOTH as legal: reading a
                // function body, `:if`/`:while`/`:for`/`:try` only adjust `indent`
                // (userfunc.c:2485-2494) and an inner `:function …(` bumps the
                // function-nesting counter and is defined when the enclosing code
                // runs (userfunc.c:2496-2511) — the inner def is registered when
                // the outer function executes, NOT at parse time. (Vim's E120 is
                // "Using <SID> not in a script context", userfunc.c:1631 — never
                // "nested :function"; the only nesting error is E1058 "Function
                // nesting too deep" at MAX_FUNC_NESTING, out of scope here.)
                // Register when this line executes — not at compile time — so a
                // guarded idempotent definition (`if !exists('*F') | function
                // F() … | endif`, whether at script level or inside an init
                // function) defines `F` only on the first pass. The compiled def
                // is staged into the program's `deferred_funcs`; the runtime
                // define-op inserts it into the live registry, keyed by a
                // content-stable staging key.
                // `:function d.key()` here is the same anonymous numbered function
                // plus Dict assignment as at script level (see
                // `compile_program_inner`), defined when the line runs: a
                // constructor function that builds `let c = {}` and then
                // `function c.next() dict` must find its own local `c`.
                let target = dict_func_target(name);
                let flags = FuncFlags {
                    bang: *bang,
                    vim9: *vim9,
                    dict: *dict || target.is_some(),
                    abort: *abort,
                    closure: *closure,
                };
                let anon = target.as_ref().map(|_| next_dict_func_name());
                let def = build_user_func_def(
                    anon.as_deref().unwrap_or(name),
                    args,
                    defaults,
                    body,
                    flags,
                    self.cur_line,
                    self.exc,
                )?;
                let key = deferred_key(&def);
                DEFERRED_FUNCS.with(|f| f.borrow_mut().push(def));
                self.load_str(&key);
                self.emit(Op::CallBuiltin(h::VIML_DEFINE_FUNC, 1));
                self.emit(Op::Pop);
                if let (Some((base, key)), Some(anon)) = (target, anon) {
                    self.stmt(&Stmt::Let {
                        target: LetTarget::Index {
                            base: Box::new(base),
                            index: Box::new(Expr::Str(key)),
                            src: None,
                        },
                        expr: Expr::Str(format!("\u{1}func\u{1}{anon}")),
                    })?;
                }
                Ok(())
            }
            // c: `do_cmdline` abandons the REST OF THE COMMAND LINE when a command
            // errors — the `|`-separated commands after the failing one do not run,
            // and execution resumes at the next line:
            //
            //   echo 'a' | echo [1] . 'x' | echo 'never'
            //   → prints `a`, reports E730, never prints `never`.
            //
            // Each command marks the error count, and if it rose, the group jumps to
            // its end. (This is also why an *error* inside a one-line
            // `try | … | catch | … | endtry` is not caught in Vim: the abandoned
            // line takes the `:catch` with it. That refinement is not modelled —
            // see BUGS.md.)
            Stmt::LineGroup(stmts) => {
                let mut to_end = Vec::new();
                for (i, inner) in stmts.iter().enumerate() {
                    // Debug (`--dap`) build only: each bar-separated command is a
                    // separate stop, because vim's debugger is command-oriented
                    // rather than line-oriented. Measured on `VIM - Vi IMproved
                    // 9.2 (2026 Feb 14, compiled Aug 02 2026 19:00:41)`:
                    //
                    //   >step
                    //   line 1: let a = 1 | let b = 2
                    //   >step
                    //   line 1: let b = 2
                    //
                    // Two stops, one line. The group is a single statement to
                    // `compile_stmts`, which therefore marks only its first
                    // command — so `i > 0` picks up exactly the ones it missed,
                    // on the same (absolute) line. `cur_line` is body-relative,
                    // so `line_base` goes back on.
                    if self.dbg && i > 0 {
                        let abs = (self.cur_line + self.line_base) as i64;
                        self.emit(Op::LoadInt(abs));
                        self.emit(Op::CallBuiltin(h::VIML_SET_LINENO, 1));
                        self.emit(Op::Pop);
                    }
                    self.emit(Op::CallBuiltin(h::VIML_ERR_MARK, 0));
                    self.emit(Op::Pop);
                    // Each bar-separated command on the line is its own
                    // ex-command and tags its own errors — the group shares a
                    // source line but not a command name.
                    if self.exc {
                        if let Some(cmd) = Self::stmt_cmdname(inner) {
                            self.load_str(cmd);
                            self.emit(Op::CallBuiltin(h::VIML_SET_CMDNAME, 1));
                            self.emit(Op::Pop);
                        }
                    }
                    self.stmt(inner)?;
                    // The last command has no successors to abandon.
                    if i + 1 < stmts.len() {
                        // Not `ERR_SINCE`: a `:silent!` error is not *reported*, and
                        // Vim carries on with the rest of the line after one. A hard
                        // failure abandons the line even when silenced.
                        // The command's `eap->nextcmd` rule — see
                        // `fusevm_bridge::b_line_abort`.
                        self.emit(Op::LoadInt(Self::nextcmd_rule(inner)));
                        self.emit(Op::CallBuiltin(h::VIML_LINE_ABORT, 1));
                        to_end.push(self.emit(Op::JumpIfTrue(0)));
                    }
                }
                let end = self.b.current_pos();
                for j in to_end {
                    self.b.patch_jump(j, end);
                }
                Ok(())
            }
            // c: `:silent!` raises `emsg_silent` for the duration of the command, so
            // the error is raised (and still aborts the command) but not reported.
            Stmt::Silent { bang, stmt } => {
                let flag = i64::from(*bang);
                self.emit(Op::LoadInt(flag));
                self.emit(Op::CallBuiltin(h::VIML_SILENT_ENTER, 1));
                self.emit(Op::Pop);
                self.stmt(stmt)?;
                self.emit(Op::LoadInt(flag));
                self.emit(Op::CallBuiltin(h::VIML_SILENT_LEAVE, 1));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::Throw(e) => {
                // c: `ex_throw` evaluates the argument with `eval0()` FIRST and
                // only throws when that succeeded — an error while evaluating it
                // is the outcome, not the value. Mark the error count so
                // `VIML_THROW` can tell (vim 9.2: `throw [][0]` gives
                // `Vim(throw):E684`, not a thrown `v:null`).
                self.emit(Op::CallBuiltin(h::VIML_ERR_MARK, 0));
                self.emit(Op::Pop);
                self.expr(e)?;
                self.emit(Op::CallBuiltin(h::VIML_THROW, 1));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::Try {
                body,
                catches,
                finally,
                inline,
            } => self.try_stmt(body, catches, finally, *inline),
        }
    }

    /// `:try … :catch … :finally … :endtry`. The protected body's unwind checks
    /// jump to the catch dispatch; matched catches clear the pending exception;
    /// the finally body always runs; any still-pending exception propagates to
    /// the enclosing boundary.
    fn try_stmt(
        &mut self,
        body: &[(u32, Stmt)],
        catches: &[(Option<String>, Block)],
        finally: &Option<Block>,
        inline: bool,
    ) -> Result<(), VimlError> {
        // c: `:try` raises `trylevel`, which is what makes an error inside the body
        // a catchable exception rather than a printed message (`cause_errthrow`).
        self.emit(Op::CallBuiltin(h::VIML_TRY_ENTER, 0));
        self.emit(Op::Pop);
        self.fin.push(FinCtx {
            loop_depth: self.loops.len(),
            ..FinCtx::default()
        });
        // Protected body — its unwind frame targets the catch dispatch.
        self.unwind.push(Vec::new());
        self.compile_stmts(body)?;
        let body_frame = self.unwind.pop().expect("try body frame");
        let j_normal = self.emit(Op::Jump(0)); // normal completion → finally

        let catch_dispatch = self.b.current_pos();
        for j in body_frame {
            self.b.patch_jump(j, catch_dispatch);
        }
        // A one-line `try | … | catch | … | endtry`: an ERROR abandons the command
        // line, which takes the `:catch` with it — so the error is not caught here
        // and propagates to an enclosing handler. An explicit `:throw` on the same
        // line still reaches the catch. Skipping straight to the finally leaves the
        // exception pending, and the propagation check after the finally carries it
        // outward.
        let mut skip_catches = None;
        if inline {
            self.emit(Op::CallBuiltin(h::VIML_EXC_IS_HARD, 0));
            skip_catches = Some(self.emit(Op::JumpIfTrue(0)));
        }

        // Catch arms. `to_finally` collects every jump that should land at the
        // finally block (caught-and-done, or re-thrown from a catch body).
        let mut to_finally = vec![j_normal];
        let mut prev_no_match: Option<usize> = None;
        for (pat, cbody) in catches {
            if let Some(j) = prev_no_match.take() {
                let here = self.b.current_pos();
                self.b.patch_jump(j, here);
            }
            // Empty string = catch-all.
            self.load_str(pat.as_deref().unwrap_or(""));
            self.emit(Op::CallBuiltin(h::VIML_CATCH_MATCH, 1));
            let jf = self.emit(Op::JumpIfFalse(0));
            self.unwind.push(Vec::new());
            self.compile_stmts(cbody)?;
            let cframe = self.unwind.pop().expect("catch body frame");
            to_finally.push(self.emit(Op::Jump(0)));
            to_finally.extend(cframe); // a re-throw in the catch body → finally
            prev_no_match = Some(jf);
        }

        // Every path out of the body — normal completion, a matched catch, and "no
        // catch matched" — converges here, so this is where the try level drops
        // again. An error raised inside a `:catch`/`:finally` body is therefore
        // only catchable by an *enclosing* `:try`, as in Vim.
        // The `:finally` is not inside its own `:try`: a `:return` there leaves
        // outward. With a command pending, every other way in records "none".
        let fin = self.fin.pop().expect("try fin ctx");
        let finally_start = self.b.current_pos();
        if let Some(j) = skip_catches {
            self.b.patch_jump(j, finally_start);
        }
        if !fin.sites.is_empty() {
            self.emit(Op::LoadInt(0));
            self.emit(Op::CallBuiltin(h::VIML_PENDING_PUSH, 1));
            self.emit(Op::Pop);
            let pending_entry = self.b.current_pos();
            for j in &fin.sites {
                self.b.patch_jump(*j, pending_entry);
            }
        }
        self.emit(Op::CallBuiltin(h::VIML_TRY_LEAVE, 0));
        self.emit(Op::Pop);
        if let Some(j) = prev_no_match {
            self.b.patch_jump(j, finally_start); // no catch matched → finally
        }
        for j in to_finally {
            self.b.patch_jump(j, finally_start);
        }
        if let Some(fbody) = finally {
            self.compile_stmts(fbody)?;
        }
        // c: `ex_endtry` resumes the command the `:finally` interrupted.
        let mut resume = Vec::new();
        if !fin.sites.is_empty() {
            for &kind in &fin.kinds {
                self.emit(Op::LoadInt(kind));
                self.emit(Op::CallBuiltin(h::VIML_PENDING_TAKE_IF, 1));
                resume.push((kind, self.emit(Op::JumpIfTrue(0))));
            }
            self.emit(Op::LoadInt(0));
            self.emit(Op::CallBuiltin(h::VIML_PENDING_TAKE_IF, 1));
            self.emit(Op::Pop);
        }
        // After finally: if an exception is still pending, propagate it to the
        // enclosing boundary (the try's own frame is already popped).
        if self.exc {
            self.emit(Op::CallBuiltin(h::VIML_CHECK_EXC, 0));
            let j = self.emit(Op::JumpIfTrue(0));
            if let Some(frame) = self.unwind.last_mut() {
                frame.push(j);
            }
        }
        if !resume.is_empty() {
            let done = self.emit(Op::Jump(0));
            for (kind, j) in resume {
                let here = self.b.current_pos();
                self.b.patch_jump(j, here);
                self.exit_jump(kind);
            }
            let here = self.b.current_pos();
            self.b.patch_jump(done, here);
        }
        Ok(())
    }

    /// `:if`/`:elseif`/`:else`/`:endif` — a chain of `cond → body` arms.
    fn if_stmt(
        &mut self,
        arms: &[(Expr, Block)],
        else_body: &Option<Block>,
    ) -> Result<(), VimlError> {
        let mut end_jumps = Vec::new();
        for (cond, body) in arms {
            let calls_before = self.calls;
            self.mark_parse_error(cond);
            self.cond(cond)?;
            // c: `ex_if` reads `CHECK_SKIP` (`vendor/ex_eval.c:865`, the macro at
            // c:80-85) — an error raised WHILE evaluating the condition sets
            // `did_emsg`, so neither the `:if` body nor the `:else` runs; c:1590
            // spells the same test out again for `ex_else`. The condition value is
            // on the stack, hence `drop = 1`.
            if self.calls > calls_before {
                self.emit_block_abort_check(1);
            }
            let jf = self.emit(Op::JumpIfFalse(0));
            self.compile_stmts(body)?;
            end_jumps.push(self.emit(Op::Jump(0)));
            let next = self.b.current_pos();
            self.b.patch_jump(jf, next);
        }
        if let Some(body) = else_body {
            self.compile_stmts(body)?;
        }
        let end = self.b.current_pos();
        for j in end_jumps {
            self.b.patch_jump(j, end);
        }
        Ok(())
    }

    /// A block header whose text did not parse ([`Expr::ScriptError`], see
    /// `viml_parser::parse_cmd_expr`) reports unconditionally: `VIML_RAISE` stays
    /// quiet when an error was already counted since the last mark (a deferred
    /// error to the right of a failed operand), and with no mark set by this
    /// command that baseline is whatever an earlier statement left.
    fn mark_parse_error(&mut self, e: &Expr) {
        if matches!(e, Expr::ScriptError(_)) {
            self.emit(Op::CallBuiltin(h::VIML_ERR_MARK, 0));
            self.emit(Op::Pop);
        }
    }

    /// `:while {cond} … :endwhile`.
    fn while_stmt(&mut self, cond: &Expr, body: &[(u32, Stmt)]) -> Result<(), VimlError> {
        // Loop rotation: enter at the test, put the body first, and make the
        // condition the CONDITIONAL BACKEDGE (`JumpIfTrue` back to the body).
        // This is semantically identical to a top-tested `while` (the initial
        // jump checks the condition before the first iteration), but the only
        // backward branch is the test itself — the shape fusevm's tracing JIT
        // records (no mid-body forward side-exit to abort the trace).
        let to_test = self.emit(Op::Jump(0));
        let l_body = self.b.current_pos();
        self.loops.push(LoopCtx::default());
        self.compile_stmts(body)?;
        let ctx = self.loops.pop().expect("loop ctx");
        let l_test = self.b.current_pos();
        self.b.patch_jump(to_test, l_test);
        let calls_before = self.calls;
        self.mark_parse_error(cond);
        self.cond(cond)?;
        // c: `ex_while` passes `skip = CHECK_SKIP` to `eval_to_bool`
        // (`vendor/ex_eval.c:1007-1009`) and only activates the loop when
        // `!skip && !error && result` (c:1042). An error in the condition
        // therefore leaves the loop inactive — on the first pass AND on the
        // backedge, which is the second half of why a failing body runs the loop
        // exactly once.
        if self.calls > calls_before {
            self.emit_block_abort_check(1);
        }
        self.emit(Op::JumpIfTrue(l_body));
        let l_end = self.b.current_pos();
        for j in ctx.breaks {
            self.b.patch_jump(j, l_end);
        }
        for j in ctx.continues {
            self.b.patch_jump(j, l_test);
        }
        Ok(())
    }

    /// `:for {var} in {list} … :endfor`. Compiled as an index loop over the
    /// evaluated list, using hidden globals for the list + index (control-char
    /// names that cannot collide with user variables).
    /// Allocate a fresh hidden fusevm slot (after the named slots).
    fn alloc_slot(&mut self) -> u16 {
        let idx = self.slots.len() as u16;
        self.slots.insert(format!("\u{1}slot_{idx}"), idx);
        idx
    }

    /// `range(...)` arguments if `iter` is a `range()` call with 1–3 args, else
    /// `None`. Bounds need not be provably integer — `for_range_native` coerces a
    /// non-int start/bound with `tv_get_number` (exactly as `f_range` does), so a
    /// dynamic bound like `range(a:n)` or `range(len(x))` still runs natively.
    fn range_native_args<'a>(&self, iter: &'a Expr) -> Option<&'a [Expr]> {
        if let Expr::Call { name, args, .. } = iter {
            if name == "range" && (1..=3).contains(&args.len()) {
                return Some(args);
            }
        }
        None
    }

    /// Emit `for VAR in range(...)` as a native integer counter loop (rotated
    /// for the tracing JIT). `range()` is evaluated once: the bound is hoisted
    /// into a hidden slot, as Vim materializes the list a single time.
    fn for_range_native(
        &mut self,
        slot: u16,
        args: &[Expr],
        step: i64,
        body: &[(u32, Stmt)],
    ) -> Result<(), VimlError> {
        // 1 arg: `0 .. n-1` (test `i < n`). 2+ args: `a .. b` inclusive (`i <= b`).
        let (start, bound, cmp) = if args.len() == 1 {
            (None, &args[0], Op::NumLt)
        } else {
            (Some(&args[0]), &args[1], Op::NumLe)
        };
        // Coerce a non-literal-int start/bound to an integer once (range() does
        // tv_get_number on its args); the coercion is in the loop prologue, so
        // the traced body stays CallBuiltin-free.
        match start {
            None => {
                self.emit(Op::LoadInt(0));
            }
            Some(e) => {
                self.expr(e)?;
                if !self.expr_is_int(e) {
                    self.emit(Op::CallBuiltin(h::VIML_TONUMBER, 1));
                }
            }
        }
        self.emit(Op::SetSlot(slot)); // i = start
        let bound_slot = self.alloc_slot();
        self.expr(bound)?;
        if !self.expr_is_int(bound) {
            self.emit(Op::CallBuiltin(h::VIML_TONUMBER, 1));
        }
        self.emit(Op::SetSlot(bound_slot)); // bound = <expr> (once)

        let to_test = self.emit(Op::Jump(0));
        let l_body = self.b.current_pos();
        self.loops.push(LoopCtx::default());
        self.compile_stmts(body)?;
        let ctx = self.loops.pop().expect("loop ctx");
        let l_incr = self.b.current_pos(); // continue target
        self.emit(Op::GetSlot(slot));
        self.emit(Op::LoadInt(step));
        self.emit(Op::Add);
        self.emit(Op::SetSlot(slot)); // i += step
        let l_test = self.b.current_pos();
        self.b.patch_jump(to_test, l_test);
        self.emit(Op::GetSlot(slot));
        self.emit(Op::GetSlot(bound_slot));
        self.emit(cmp);
        self.emit(Op::JumpIfTrue(l_body)); // backedge = the loop test
        let l_end = self.b.current_pos();
        for j in ctx.breaks {
            self.b.patch_jump(j, l_end);
        }
        for j in ctx.continues {
            self.b.patch_jump(j, l_incr);
        }
        Ok(())
    }

    fn for_stmt(
        &mut self,
        vars: &ForVars,
        iter: &Expr,
        body: &[(u32, Stmt)],
    ) -> Result<(), VimlError> {
        // Native fast path: `for VAR in range(...)` with a slotted VAR and
        // integer bounds compiles to a native counter loop — no list is
        // materialized, the body is CallBuiltin-free, and the loop is rotated
        // so fusevm's tracing JIT compiles it. Matches Vim's `range()`: 1 arg →
        // `0..n-1`; 2 args → `a..b` inclusive; 3 args → step (positive literal).
        if let ForVars::One(name) = vars {
            if let Some(&slot) = self.slots.get(self.slot_key(name)) {
                if let Some(args) = self.range_native_args(iter) {
                    // step must be a positive literal so the compare direction
                    // is known at compile time; anything else falls through.
                    let step = match args.get(2) {
                        None => Some(1),
                        Some(Expr::Number(s)) if *s > 0 => Some(*s),
                        _ => None,
                    };
                    if let Some(step) = step {
                        return self.for_range_native(slot, args, step, body);
                    }
                }
            }
        }
        let n = self.hidden;
        self.hidden += 1;
        let list_var = format!("\u{1}for_list_{n}");
        let idx_var = format!("\u{1}for_idx_{n}");
        let item_var = format!("\u{1}for_item_{n}");

        // list = <iter>;  idx = 0
        let calls_before = self.calls;
        self.emit(Op::CallBuiltin(h::VIML_ARGS_BEGIN, 0));
        self.emit(Op::Pop);
        self.mark_parse_error(iter);
        self.expr(iter)?;
        // c: `eval_for_line` returns at once when `eval1()` FAILED — there is no
        // value to type-check, so `for x in nosuch` is E121 alone, not also
        // E1098. The loop then walks an empty List.
        self.emit(Op::CallBuiltin(h::VIML_ARGS_FAILED, 0));
        let evaluated = self.emit(Op::JumpIfFalse(0));
        self.emit(Op::Pop);
        self.emit(Op::CallBuiltin(h::VIML_MAKE_LIST, 0));
        let to_store = self.emit(Op::Jump(0));
        let check_at = self.b.current_pos();
        self.b.patch_jump(evaluated, check_at);
        // c: `eval_for_line`'s type switch — a String is walked one character
        // (with its composing marks) at a time, not one byte; any other type
        // than String/List/Blob is E1098 and leaves nothing to walk.
        self.emit(Op::CallBuiltin(h::VIML_FOR_ITEMS, 1));
        let store_at = self.b.current_pos();
        self.b.patch_jump(to_store, store_at);
        // c: `ex_while`'s `:for` arm calls `eval_for_line` and then only advances
        // when `!error && fi != NULL && !skip` (`vendor/ex_eval.c:1021-1030`) —
        // an error while evaluating the list leaves the loop inactive. vim
        // materializes the list ONCE, so this test is likewise outside the loop.
        if self.calls > calls_before {
            self.emit_block_abort_check(1);
        }
        self.set_var(&list_var);
        self.emit(Op::LoadInt(0));
        self.set_var(&idx_var);

        let l_cond = self.b.current_pos();
        // if !(idx < len(list)) jump end
        self.get_var(&idx_var);
        self.get_var(&list_var);
        self.emit(Op::CallBuiltin(h::VIML_FN_LEN, 1));
        self.emit(Op::CallBuiltin(
            h::cmp_id(CmpOp::Less, CaseFlag::MatchCase),
            2,
        ));
        self.emit(Op::CallBuiltin(h::VIML_TRUTHY, 1));
        let jf = self.emit(Op::JumpIfFalse(0));

        // item = list[idx]; bind it to the loop variable(s).
        self.get_var(&list_var);
        self.get_var(&idx_var);
        self.emit(Op::CallBuiltin(h::VIML_INDEX, 2));
        let mut unpack_failed = None;
        match vars {
            ForVars::One(name) => self.set_var(name),
            ForVars::List { names, rest } => {
                // c: `next_for_item` is `ex_let_vars(...) == OK`, so an item
                // that is not a List, or has the wrong number of elements for
                // the targets, reports E714/E687/E688 exactly as `:let [a, b] =`
                // does and ENDS the loop — `ex_while` treats a FALSE
                // `next_for_item` like the end of the list.
                self.set_var(&item_var);
                self.get_var(&item_var);
                self.emit(Op::LoadInt(names.len() as i64 + i64::from(rest.is_some())));
                self.emit(Op::LoadInt(i64::from(rest.is_some())));
                self.emit(Op::CallBuiltin(h::VIML_UNPACK_CHECK, 3));
                unpack_failed = Some(self.emit(Op::JumpIfFalse(0)));
                for (i, name) in names.iter().enumerate() {
                    self.get_var(&item_var);
                    self.emit(Op::LoadInt(i as i64));
                    self.emit(Op::CallBuiltin(h::VIML_INDEX, 2));
                    self.set_var(name);
                }
                if let Some(r) = rest {
                    self.get_var(&item_var);
                    self.emit(Op::LoadInt(names.len() as i64)); // from
                    self.emit(Op::LoadUndef); // to = end
                    self.emit(Op::CallBuiltin(h::VIML_SLICE, 3));
                    self.set_var(r);
                }
            }
        }

        self.loops.push(LoopCtx::default());
        self.compile_stmts(body)?;
        let ctx = self.loops.pop().expect("loop ctx");

        // idx += 1  (continue target)
        let l_incr = self.b.current_pos();
        self.get_var(&idx_var);
        self.emit(Op::LoadInt(1));
        self.emit(Op::CallBuiltin(h::VIML_ADD, 2));
        self.set_var(&idx_var);
        self.emit(Op::Jump(l_cond));

        let l_end = self.b.current_pos();
        self.b.patch_jump(jf, l_end);
        if let Some(j) = unpack_failed {
            self.b.patch_jump(j, l_end);
        }
        for j in ctx.breaks {
            self.b.patch_jump(j, l_end);
        }
        for j in ctx.continues {
            self.b.patch_jump(j, l_incr);
        }
        Ok(())
    }

    /// Open a `:if`/`:while`/`:for` block.
    ///
    /// c: `do_cmdline` clears `did_emsg` before this command line, because the
    /// condition stack is still empty (`ex_docmd.c:448-454`) — so an error from an
    /// EARLIER statement must not skip anything inside this block. Establishing
    /// that baseline needs a builtin call, and a builtin call in the chunk is
    /// exactly what stops a slotted numeric loop being JIT-compiled. So the
    /// outermost block only reserves a native `Op::Jump` here; [`Compiler::cond_leave`]
    /// decides, once it knows whether anything inside can error at all, whether to
    /// give it a destination that sets the baseline first.
    fn cond_enter(&mut self) {
        if self.cond_depth == 0 {
            self.cond_head = Some(self.emit(Op::Jump(0)));
        }
        self.cond_depth += 1;
    }

    /// Close it. When the outermost one closes the condition stack is empty again
    /// and the C's next `did_emsg = false` is due, so this is where every skip test
    /// collected inside lands.
    fn cond_leave(&mut self) {
        self.cond_depth -= 1;
        if self.cond_depth != 0 {
            return;
        }
        let head = self.cond_head.take().expect("conditional head");
        let body = head + 1;
        let aborts = std::mem::take(&mut self.aborts);
        if aborts.is_empty() {
            // Nothing in the block reached a builtin, so nothing in it could have
            // called `emsg()`: no baseline is needed and the reserved jump just
            // falls through to the block.
            self.b.patch_jump(head, body);
            return;
        }
        // Something can error. Land the skip jumps at the end of the block, then
        // append the baseline op OUT OF LINE and point the reserved jump at it —
        // the block itself stays exactly as compiled.
        let end = self.b.current_pos();
        for j in aborts {
            self.b.patch_jump(j, end);
        }
        let over = self.emit(Op::Jump(0));
        let prologue = self.b.current_pos();
        self.emit(Op::CallBuiltin(h::VIML_BLOCK_ENTER, 0));
        self.emit(Op::Pop);
        self.emit(Op::Jump(body));
        let after = self.b.current_pos();
        self.b.patch_jump(over, after);
        self.b.patch_jump(head, prologue);
    }

    /// The `did_emsg` half of `ea.skip` (`ex_docmd.c:2027-2031`), as a jump out of
    /// the enclosing conditional. `drop` is the number of stack values the jump
    /// has to discard (1 when the test sits on top of an already-evaluated loop
    /// or `:if` condition, 0 between statements).
    ///
    /// No-op at depth 0: there the C would have reset `did_emsg` before the next
    /// command line, so nothing is skipped.
    fn emit_block_abort_check(&mut self, drop: usize) {
        if self.cond_depth == 0 {
            return;
        }
        self.emit(Op::CallBuiltin(h::VIML_BLOCK_ABORT, 0));
        if drop == 0 {
            let j = self.emit(Op::JumpIfTrue(0));
            self.aborts.push(j);
            return;
        }
        let cont = self.emit(Op::JumpIfFalse(0));
        for _ in 0..drop {
            self.emit(Op::Pop);
        }
        let j = self.emit(Op::Jump(0));
        self.aborts.push(j);
        let here = self.b.current_pos();
        self.b.patch_jump(cont, here);
    }

    /// After a user-function call leaves its result on the stack, check whether
    /// the call raised an exception; if so, drop the (default) result and unwind
    /// to the enclosing boundary — so the throw aborts the surrounding command
    /// instead of letting it consume a bogus value. (No-op without exceptions.)
    fn emit_call_unwind_check(&mut self) {
        if !self.exc {
            return;
        }
        // Stack: [result]. → [result, pending].
        self.emit(Op::CallBuiltin(h::VIML_CHECK_EXC, 0));
        let cont = self.emit(Op::JumpIfFalse(0)); // not pending → keep result, continue
        self.emit(Op::Pop); // pending → drop the result before unwinding
        let j = self.emit(Op::Jump(0));
        if let Some(frame) = self.unwind.last_mut() {
            frame.push(j);
        }
        let here = self.b.current_pos();
        self.b.patch_jump(cont, here);
    }

    /// Emit a get of a (possibly scoped) variable by name. A slotted local
    /// reads natively via `Op::GetSlot`.
    /// The slot key for a variable: in a function, `l:name` is bare `name` (same
    /// storage), so both reach the same slot. Other scopes pass through unchanged
    /// and miss `self.slots`, falling back to the dict-backed builtin path.
    fn slot_key<'a>(&self, name: &'a str) -> &'a str {
        if self.in_function {
            if let Some(rest) = name.strip_prefix("l:") {
                return rest;
            }
        }
        name
    }

    fn get_var(&mut self, name: &str) {
        if let Some(&slot) = self.slots.get(self.slot_key(name)) {
            self.emit(Op::GetSlot(slot));
            return;
        }
        self.load_str(name);
        self.emit(Op::CallBuiltin(h::VIML_GETVAR, 1));
    }

    /// Emit a set of a variable from the value on top of the stack, leaving the
    /// stack balanced. A slotted local writes natively via `Op::SetSlot` (which
    /// consumes the value).
    fn set_var(&mut self, name: &str) {
        if let Some(&slot) = self.slots.get(self.slot_key(name)) {
            self.emit(Op::SetSlot(slot));
            return;
        }
        self.load_str(name);
        self.emit(Op::CallBuiltin(h::VIML_SETVAR, 2));
        self.emit(Op::Pop);
    }

    /// `:echo` / `:echon`, compiled ARGUMENT BY ARGUMENT.
    ///
    /// c: `ex_echo`'s loop (`vendor/eval.c:6139-6186`) evaluates one argument and
    /// writes it before touching the next, so an error raised while evaluating
    /// argument N lands after argument N-1 has already appeared:
    ///
    /// ```vim
    /// echo 'A =' strlen([1]) 'B'
    /// " A =
    /// " E730: Using a List as a String 0 B
    /// ```
    ///
    /// Evaluating everything and writing once — what this did before — put the
    /// error text first. `eval1() == FAIL` for an argument `break`s the loop
    /// (c:6146-6155), so a failing argument also drops the ones after it, and the
    /// mark is retaken per argument because that is the granularity the C tests at.
    fn echo(&mut self, args: &[Expr], id: u16) -> Result<(), VimlError> {
        // `:echo` with no arguments has no loop turn at all; the single-call form
        // still carries the empty-message conventions the capture sinks expect.
        if args.is_empty() {
            self.emit(Op::CallBuiltin(h::VIML_ERR_MARK, 0));
            self.emit(Op::Pop);
            self.emit(Op::CallBuiltin(id, 0));
            self.emit(Op::Pop);
            return Ok(());
        }
        let newline = i64::from(id == h::VIML_ECHO);
        let mut to_end = Vec::new();
        for (i, a) in args.iter().enumerate() {
            // Snapshot did_emsg/EVAL_FAIL before THIS argument, so the test below
            // asks about this argument's `eval1()` and not a previous one's.
            self.emit(Op::CallBuiltin(h::VIML_ERR_MARK, 0));
            self.emit(Op::Pop);
            self.expr(a)?;
            // c:6146 `if (eval1(&arg, &rettv, &evalarg) == FAIL) { … break; }`.
            // `EVAL_FAIL`, not `did_emsg`: a builtin that reports an error and
            // still yields a value keeps its `:echo` (`echo str2nr('0x1f', 0)`
            // prints the E474 AND `0`), while a `:silent!` failure prints nothing
            // even though it was never reported.
            self.emit(Op::CallBuiltin(h::VIML_ERR_SINCE, 0));
            let ok = self.emit(Op::JumpIfFalse(0));
            self.emit(Op::Pop); // drop the recovered value the failed eval left
            to_end.push(self.emit(Op::Jump(0)));
            let here = self.b.current_pos();
            self.b.patch_jump(ok, here);
            self.emit(Op::LoadInt(i64::from(i == 0)));
            self.emit(Op::LoadInt(newline));
            self.emit(Op::CallBuiltin(h::VIML_ECHO_ARG, 3));
            self.emit(Op::Pop);
        }
        let end = self.b.current_pos();
        for j in to_end {
            self.b.patch_jump(j, end);
        }
        self.emit(Op::LoadInt(newline));
        self.emit(Op::CallBuiltin(h::VIML_ECHO_END, 1));
        self.emit(Op::Pop);
        Ok(())
    }

    /// `:echoerr {expr} …` — `ex_execute()` (`vendor/eval.c`) with `CMD_echoerr`:
    /// each argument is evaluated and appended to ONE message, a space between
    /// two once the message is not empty, a String as `:echo` shows it and any
    /// other type as `string()` does. The first argument that fails to evaluate
    /// ends the command with nothing reported but its own error (c: `ret = FAIL;
    /// break;`); otherwise a non-empty message goes to `emsg_multiline()`.
    /// Stack while it runs: the message so far, then the argument.
    fn echoerr(&mut self, args: &[Expr]) -> Result<(), VimlError> {
        self.load_str("");
        let mut failed = Vec::new();
        for a in args {
            self.emit(Op::CallBuiltin(h::VIML_ERR_MARK, 0));
            self.emit(Op::Pop);
            self.expr(a)?;
            self.emit(Op::CallBuiltin(h::VIML_ERR_SINCE, 0));
            let ok = self.emit(Op::JumpIfFalse(0));
            self.emit(Op::Pop); // the value the failed evaluation left
            self.emit(Op::Pop); // the message so far
            failed.push(self.emit(Op::Jump(0)));
            let here = self.b.current_pos();
            self.b.patch_jump(ok, here);
            self.emit(Op::CallBuiltin(h::VIML_ECHOERR_ARG, 2));
        }
        self.emit(Op::CallBuiltin(h::VIML_ECHOERR_END, 1));
        self.emit(Op::Pop);
        let end = self.b.current_pos();
        for j in failed {
            self.b.patch_jump(j, end);
        }
        Ok(())
    }

    /// `:let {var-name} …` — `list_arg_vars()` (`vars.c:1210`): read each name
    /// and list it. The first name that fails to evaluate sets `error`, and every
    /// later one is then only skipped over (c:1219), so nothing after it prints.
    fn let_list(&mut self, vars: &[(String, Expr)]) -> Result<(), VimlError> {
        let mut to_end = Vec::new();
        for (name, e) in vars {
            self.emit(Op::CallBuiltin(h::VIML_ERR_MARK, 0));
            self.emit(Op::Pop);
            self.expr(e)?;
            self.emit(Op::CallBuiltin(h::VIML_ERR_SINCE, 0));
            let ok = self.emit(Op::JumpIfFalse(0));
            self.emit(Op::Pop); // drop the recovered value the failed read left
            to_end.push(self.emit(Op::Jump(0)));
            let here = self.b.current_pos();
            self.b.patch_jump(ok, here);
            self.load_str(name);
            self.emit(Op::CallBuiltin(h::VIML_LET_LIST_ONE, 2));
            self.emit(Op::Pop);
        }
        let end = self.b.current_pos();
        for j in to_end {
            self.b.patch_jump(j, end);
        }
        Ok(())
    }

    /// Whether evaluating `expr` could raise a Vim error. A literal cannot; an
    /// expression the compiler has already proved numeric cannot either (that is the
    /// same judgement the native-arithmetic fast path relies on).
    fn expr_can_error(expr: &Expr) -> bool {
        !matches!(
            expr,
            Expr::Number(_) | Expr::Float(_) | Expr::Str(_) | Expr::Bytes(_) | Expr::NullFunc
        )
    }

    /// The operator of a COMPOUND `:let` (`+= -= *= /= %= .=`), or `None` for a
    /// plain `=`. See `Expr::Arith::mod_op`.
    fn let_compound_op(expr: &Expr) -> Option<char> {
        match expr {
            Expr::Arith {
                op, mod_op: true, ..
            } => Some(match op {
                ArithOp::Add => '+',
                ArithOp::Sub => '-',
                ArithOp::Mul => '*',
                ArithOp::Div => '/',
                ArithOp::Mod => '%',
                ArithOp::Concat => '.',
                // Never a compound operator: legacy vim has no `<<=`/`>>=`.
                ArithOp::ShiftL | ArithOp::ShiftR => return None,
            }),
            _ => None,
        }
    }

    /// The same, restricted to the arithmetic operators — the set
    /// `vim_strchr("+-*/%", *op)` picks out in `ex_let_env` and
    /// `ex_let_register`.
    fn arith_compound_op(expr: &Expr) -> Option<char> {
        Self::let_compound_op(expr).filter(|c| *c != '.')
    }

    /// After a line that ran an `:execute` from a function: if the text
    /// `:return`ed, return now. c: `ex_return` only marks the function returned
    /// (`do_return`), and `do_cmdline` sees it when it fetches the next line —
    /// so the rest of the `|`-separated line still runs (`exe 'return' | echo
    /// 'x'` prints `x`). Text that an `:execute` ran ends instead, leaving the
    /// flag for the function.
    fn exec_return_check(&mut self) {
        let mode = if self.in_function {
            EXEC_RETURNED_TAKE
        } else {
            EXEC_RETURNED_PEEK
        };
        self.emit(Op::LoadInt(mode));
        self.emit(Op::CallBuiltin(h::VIML_EXEC_RETURNED, 1));
        let skip = self.emit(Op::JumpIfFalse(0));
        if self.in_function {
            self.exit_jump(EXIT_RETURN);
        } else {
            let j = self.emit(Op::Jump(0));
            self.finishes.push(j);
        }
        let here = self.b.current_pos();
        self.b.patch_jump(skip, here);
    }

    /// Leave by `:return` (`EXIT_RETURN`), `:break` or `:continue`. Inside a
    /// `:try` the command crosses, record it as pending there and go to that
    /// try's `:finally`; otherwise jump straight to the target.
    fn exit_jump(&mut self, kind: i64) {
        let loops = self.loops.len();
        let crosses = self
            .fin
            .last()
            .is_some_and(|f| kind == EXIT_RETURN || f.loop_depth == loops);
        if crosses {
            self.emit(Op::LoadInt(kind));
            self.emit(Op::CallBuiltin(h::VIML_PENDING_PUSH, 1));
            self.emit(Op::Pop);
            let j = self.emit(Op::Jump(0));
            let f = self.fin.last_mut().expect("crossed :try");
            f.sites.push(j);
            if !f.kinds.contains(&kind) {
                f.kinds.push(kind);
            }
            return;
        }
        let j = self.emit(Op::Jump(0));
        match kind {
            EXIT_RETURN => self.returns.push(j),
            EXIT_BREAK => self.loops.last_mut().expect("in a loop").breaks.push(j),
            _ => self.loops.last_mut().expect("in a loop").continues.push(j),
        }
    }

    /// Report `msg` as the current command's error at run time (`emsg()` from
    /// inside an Ex command: tagged `Vim(cmd):` inside `:try`, and it abandons
    /// the rest of a `|`-separated line because it is reported).
    /// The error mark is set first: `VIML_RAISE` stays quiet when an earlier
    /// error is still above the mark, which is right for an expression and wrong
    /// for a command of its own.
    fn raise_cmd(&mut self, msg: &str) {
        self.emit(Op::CallBuiltin(h::VIML_ERR_MARK, 0));
        self.emit(Op::Pop);
        self.load_str(msg);
        self.emit(Op::CallBuiltin(h::VIML_RAISE_CMD, 1));
        self.emit(Op::Pop);
    }

    /// c: `semsg(_(e_letwrong), op)` — "E734: Wrong variable type for %s=".
    fn raise_letwrong(&mut self, op: char) {
        self.load_str(&format!("E734: Wrong variable type for {op}="));
        // c: reported by `ex_let_env`/`ex_let_register`/`ex_let_option` with the
        // argument already parsed, so it does not abandon the rest of the line.
        self.emit(Op::CallBuiltin(h::VIML_RAISE_CMD, 1));
        self.emit(Op::Pop);
    }

    /// `:let {name} = {expr}` for a plain variable target.
    fn let_var(&mut self, name: &str, expr: &Expr) -> Result<(), VimlError> {
        // Vim abandons a command whose expression raised an error, so a
        // failed `:let` leaves the variable ALONE:
        //
        //   let g:v = 'orig'
        //   silent! let g:v = [1] . 'x'   " E730
        //   echo g:v                      " still 'orig' in Vim
        //
        // Without this the recovered value ('0x') was stored and the script
        // carried on with corrupted data. An expression that cannot raise
        // (a literal, or arithmetic the compiler already proved numeric)
        // skips the guard, so `let i = i + 1` keeps its native fast path.
        // `expr_is_num` is the same judgement the native-arithmetic fast
        // path already relies on: an expression it proves numeric is
        // compiled to raw ops that cannot raise, so guarding it would only
        // put `CallBuiltin`s back into loop bodies the JIT needs to trace.
        if Self::expr_can_error(expr) && !self.expr_is_num(expr) {
            self.emit(Op::CallBuiltin(h::VIML_ERR_MARK, 0));
            self.emit(Op::Pop);
            self.expr(expr)?;
            self.emit(Op::CallBuiltin(h::VIML_ERR_SINCE, 0));
            let j_failed = self.emit(Op::JumpIfTrue(0));
            self.set_var(name);
            let j_done = self.emit(Op::Jump(0));
            // Failed: drop the recovered value, leave the variable as it was.
            let here = self.b.current_pos();
            self.b.patch_jump(j_failed, here);
            self.emit(Op::Pop);
            let after = self.b.current_pos();
            self.b.patch_jump(j_done, after);
        } else {
            self.expr(expr)?;
            self.set_var(name);
        }
        Ok(())
    }

    /// `:const` — `ex_let` with `is_const` (`vendor/eval/vars.c:916`). The value is
    /// evaluated once; a failed evaluation assigns nothing, as for `:let`. Then
    /// each target, in order, goes through `set_var_const` (c:2848): a name that
    /// already exists is E995 and stops the assignment there, a new one is
    /// bound and locked (`tv_item_lock(…, DICT_MAXNEST, …)`, i.e. `:lockvar!`).
    fn const_stmt(&mut self, target: &LetTarget, expr: &Expr) -> Result<(), VimlError> {
        let n = self.hidden;
        self.hidden += 1;
        let tmp = format!("\u{1}const_{n}");
        self.emit(Op::CallBuiltin(h::VIML_ERR_MARK, 0));
        self.emit(Op::Pop);
        self.expr(expr)?;
        self.emit(Op::CallBuiltin(h::VIML_ERR_SINCE, 0));
        let j_failed = self.emit(Op::JumpIfTrue(0));
        self.set_var(&tmp);
        let mut to_end = Vec::new();
        // (name, how to read its value out of `tmp`)
        let mut binds: Vec<(&str, Option<(i64, bool)>)> = Vec::new();
        match target {
            LetTarget::Var(name) => binds.push((name, None)),
            LetTarget::List { names, rest } => {
                // c: `ex_let_vars` checks the target count before binding.
                self.get_var(&tmp);
                self.emit(Op::LoadInt(names.len() as i64 + i64::from(rest.is_some())));
                self.emit(Op::LoadInt(i64::from(rest.is_some())));
                self.emit(Op::CallBuiltin(h::VIML_UNPACK_CHECK, 3));
                to_end.push(self.emit(Op::JumpIfFalse(0)));
                for (i, name) in names.iter().enumerate() {
                    binds.push((name, Some((i as i64, false))));
                }
                if let Some(r) = rest {
                    binds.push((r, Some((names.len() as i64, true))));
                }
            }
            _ => unreachable!("the parser builds Stmt::Const only for a name or a list"),
        }
        for (name, part) in binds {
            self.load_str(name);
            self.emit(Op::CallBuiltin(h::VIML_CONST_FREE, 1));
            to_end.push(self.emit(Op::JumpIfFalse(0)));
            self.get_var(&tmp);
            match part {
                None => {}
                Some((i, false)) => {
                    self.emit(Op::LoadInt(i));
                    self.emit(Op::CallBuiltin(h::VIML_INDEX, 2));
                }
                Some((from, true)) => {
                    self.emit(Op::LoadInt(from));
                    self.emit(Op::LoadUndef);
                    self.emit(Op::CallBuiltin(h::VIML_SLICE, 3));
                }
            }
            self.set_var(name);
            self.stmt(&Stmt::LockVar {
                arg: name.to_string(),
                bang: true,
                lock: true,
            })?;
        }
        let j_done = self.emit(Op::Jump(0));
        // Failed evaluation: drop the recovered value, bind nothing.
        let failed = self.b.current_pos();
        self.b.patch_jump(j_failed, failed);
        self.emit(Op::Pop);
        let end = self.b.current_pos();
        self.b.patch_jump(j_done, end);
        for j in to_end {
            self.b.patch_jump(j, end);
        }
        Ok(())
    }

    fn let_stmt(&mut self, target: &LetTarget, expr: &Expr) -> Result<(), VimlError> {
        match target {
            // A SLOTTED local cannot be locked: `slot_plan` refuses to slot anything
            // in a body that contains `:lockvar` at all (see its `Stmt::LockVar`
            // bail), and a fusevm slot has no lock state to begin with. Skipping the
            // check there is what keeps `let s += i` a bare `Op::Add` and the loop
            // JIT-traceable.
            LetTarget::Var(name)
                if Self::let_compound_op(expr).is_some()
                    && !self.slots.contains_key(self.slot_key(name)) =>
            {
                // c: `ex_let_one` → `set_var_lval`, whose `value_check_lock` runs
                // BEFORE `eexe_mod_op`. Order matters: `eexe_mod_op` extends a List
                // IN PLACE, so checking afterwards (which is where the plain `=`
                // path checks) would report E741 on an already-mutated list.
                self.load_str(name);
                self.emit(Op::CallBuiltin(h::VIML_VAR_LOCKED, 1));
                let locked = self.emit(Op::JumpIfTrue(0));
                self.let_var(name, expr)?;
                let end = self.b.current_pos();
                self.b.patch_jump(locked, end);
                Ok(())
            }
            LetTarget::Var(name) => self.let_var(name, expr),
            LetTarget::Env(name) => {
                // c: `ex_let_env` (`vendor/eval/vars.c:1316`) — an ARITHMETIC
                // compound on an environment variable is `e_letwrong` before
                // anything is evaluated; only `.=` is defined for one (c:1326).
                if let Some(op) = Self::arith_compound_op(expr) {
                    self.raise_letwrong(op);
                    return Ok(());
                }
                self.expr(expr)?;
                self.load_str(name);
                self.emit(Op::CallBuiltin(h::VIML_SETENV, 2));
                self.emit(Op::Pop);
                Ok(())
            }
            LetTarget::List { names, rest } => {
                // `:let [a, b; rest] = expr` — evaluate once into a hidden temp,
                // then index each name and slice the remainder.
                let n = self.hidden;
                self.hidden += 1;
                let tmp = format!("\u{1}unpack_{n}");
                self.expr(expr)?;
                self.set_var(&tmp);
                // The C checks the target count against the list BEFORE it
                // assigns anything (`ex_let_vars`, vars.c:1036-1051). Lowering
                // straight to index/slice skipped that: too few items surfaced
                // as `VIML_INDEX`'s E684 rather than E688, and too many were
                // accepted silently where vim raises E687.
                self.get_var(&tmp);
                self.emit(Op::LoadInt(names.len() as i64 + i64::from(rest.is_some())));
                self.emit(Op::LoadInt(i64::from(rest.is_some())));
                self.emit(Op::CallBuiltin(h::VIML_UNPACK_CHECK, 3));
                // A failed count leaves EVERY name untouched, which is what
                // `exists()` reports afterwards — the C returns FAIL before its
                // assignment loop, so jumping past the stores is the same shape.
                let jf = self.emit(Op::JumpIfFalse(0));
                for (i, name) in names.iter().enumerate() {
                    self.get_var(&tmp);
                    self.emit(Op::LoadInt(i as i64));
                    self.emit(Op::CallBuiltin(h::VIML_INDEX, 2));
                    self.set_var(name);
                }
                if let Some(r) = rest {
                    self.get_var(&tmp);
                    self.emit(Op::LoadInt(names.len() as i64)); // from
                    self.emit(Op::LoadUndef); // to = end
                    self.emit(Op::CallBuiltin(h::VIML_SLICE, 3));
                    self.set_var(r);
                }
                let after = self.b.current_pos();
                self.b.patch_jump(jf, after);
                Ok(())
            }
            LetTarget::Index { base, index, src } => {
                // `let base[index] = value` — push value, base, index; the bridge
                // sets base[index] = value (and fires Dict watchers). `base` is an
                // expression, so nested `d['a']['b']` resolves the inner container
                // (a shared Rc, so the mutation propagates).
                // The error count before the lval is resolved rides along: an
                // error there (`E121` for an undefined base) ends the `:let`
                // in `get_lval`, before any write is attempted.
                self.expr(expr)?;
                self.emit(Op::CallBuiltin(h::VIML_ERR_COUNT, 0));
                self.expr(base)?;
                self.expr(index)?;
                self.load_str(src.as_deref().unwrap_or(""));
                self.emit(Op::CallBuiltin(h::VIML_SETINDEX, 5));
                self.emit(Op::Pop);
                Ok(())
            }
            LetTarget::Range {
                base,
                idx1,
                idx2,
                src,
            } => {
                // `let base[idx1:idx2] = list` — push the source list, base, idx1
                // (default 0), idx2 (Undef → "to the end"); the bridge assigns
                // the range in place via tv_list_assign_range.
                self.expr(expr)?;
                self.emit(Op::CallBuiltin(h::VIML_ERR_COUNT, 0));
                self.expr(base)?;
                match idx1 {
                    Some(e) => self.expr(e)?,
                    None => {
                        self.emit(Op::LoadInt(0));
                    }
                }
                match idx2 {
                    Some(e) => self.expr(e)?,
                    None => {
                        self.emit(Op::LoadUndef);
                    }
                }
                self.load_str(src.as_deref().unwrap_or(""));
                self.emit(Op::CallBuiltin(h::VIML_SETRANGE, 6));
                self.emit(Op::Pop);
                Ok(())
            }
            LetTarget::Register(c) => {
                // c: `ex_let_register` (`vendor/eval/vars.c:1457`) — same rule as
                // for an environment variable: `+-*/%` is `e_letwrong`, `.=` reads
                // the register and appends (c:1465).
                if let Some(op) = Self::arith_compound_op(expr) {
                    self.raise_letwrong(op);
                    return Ok(());
                }
                // `:let @r = expr` → setreg(r, expr). Push the register name then
                // the value (the `f_setreg(argvars)` order).
                self.load_str(&c.to_string());
                self.expr(expr)?;
                self.emit(Op::CallBuiltin(h::VIML_SETREG, 2));
                self.emit(Op::Pop);
                Ok(())
            }
            LetTarget::Option(name) => {
                // c: `ex_let_option` (`vendor/eval/vars.c:1379-1384`) — `.=` on a
                // number option and `+=`/`-=`/… on a string one are `e_letwrong`,
                // and the option keeps its value. The option's type is only known
                // at run time, so the test is too.
                let mut skip = None;
                if let Some(op) = Self::let_compound_op(expr) {
                    self.load_str(name);
                    self.load_str(&op.to_string());
                    self.emit(Op::CallBuiltin(h::VIML_OPT_OP_BAD, 2));
                    skip = Some(self.emit(Op::JumpIfTrue(0)));
                }
                // `:let &opt = expr` → set the option. Push the option name then
                // the value; the bridge applies it via `option::do_set`.
                self.load_str(name);
                self.expr(expr)?;
                self.emit(Op::CallBuiltin(h::VIML_SETOPT, 2));
                self.emit(Op::Pop);
                if let Some(j) = skip {
                    let end = self.b.current_pos();
                    self.b.patch_jump(j, end);
                }
                Ok(())
            }
        }
    }

    /// Conservative static type inference: `true` only when `e` provably
    /// evaluates to a VimL Number (never Float/String/List/…), so its `+`/`-`/`*`
    /// may lower to native `Op::Add`/`Sub`/`Mul`. Integer literals are Numbers;
    /// `+ - * / %` of Numbers are Numbers (`/`,`%` are integer ops in VimL);
    /// unary `-`/`+` of a Number is a Number. Anything else is rejected, so the
    /// dynamic builtin path is used and correctness is never at risk.
    /// `true` if `e` provably evaluates to a Number (Integer OR Float) — so its
    /// `+`/`-`/`*` and comparisons may lower to native ops (fusevm promotes
    /// int↔float exactly like VimL).
    fn expr_is_num(&self, e: &Expr) -> bool {
        match e {
            Expr::Number(_) | Expr::Float(_) => true,
            Expr::Var(name) => self.slots.contains_key(self.slot_key(name)), // slotted ⇒ Number
            Expr::Arith { op, lhs, rhs, .. } => {
                !matches!(op, ArithOp::Concat | ArithOp::ShiftL | ArithOp::ShiftR)
                    && self.expr_is_num(lhs)
                    && self.expr_is_num(rhs)
            }
            Expr::Unary {
                op: UnaryOp::Neg | UnaryOp::Plus,
                expr,
            } => self.expr_is_num(expr),
            // Bitwise builtins of integer args yield an Integer (so also a Number).
            Expr::Call { name, args, .. } if bitwise_native_op(name, args.len()).is_some() => {
                args.iter().all(|a| self.expr_is_int(a))
            }
            // A ternary is a Number when both branches are (the test is irrelevant
            // to the result type).
            Expr::Ternary {
                then, otherwise, ..
            } => self.expr_is_num(then) && self.expr_is_num(otherwise),
            // A native-lowered comparison reifies to Number 0/1.
            Expr::Compare { op, lhs, rhs, .. } => {
                Self::native_cmp(*op).is_some() && self.expr_is_num(lhs) && self.expr_is_num(rhs)
            }
            // Logical-not of an Integer reifies to 0/1 (also a Number).
            Expr::Unary {
                op: UnaryOp::Not,
                expr,
            } => self.expr_is_int(expr),
            _ => false,
        }
    }

    /// `true` if `e` provably evaluates to an Integer — required for `range()`
    /// bounds (Vim's `range()` rejects Floats) and the native counter.
    fn expr_is_int(&self, e: &Expr) -> bool {
        match e {
            Expr::Number(_) => true,
            Expr::Var(name) => self.int_slots.contains(self.slot_key(name)),
            Expr::Arith { op, lhs, rhs, .. } => {
                !matches!(op, ArithOp::Concat | ArithOp::ShiftL | ArithOp::ShiftR)
                    && self.expr_is_int(lhs)
                    && self.expr_is_int(rhs)
            }
            Expr::Unary {
                op: UnaryOp::Neg | UnaryOp::Plus,
                expr,
            } => self.expr_is_int(expr),
            // Bitwise builtins yield an Integer when every argument is an Integer.
            Expr::Call { name, args, .. } if bitwise_native_op(name, args.len()).is_some() => {
                args.iter().all(|a| self.expr_is_int(a))
            }
            // A ternary is an Integer when both branches are.
            Expr::Ternary {
                then, otherwise, ..
            } => self.expr_is_int(then) && self.expr_is_int(otherwise),
            // A native-lowered comparison yields Integer 0/1.
            Expr::Compare { op, lhs, rhs, .. } => {
                Self::native_cmp(*op).is_some() && self.expr_is_num(lhs) && self.expr_is_num(rhs)
            }
            // Logical-not of an Integer yields Integer 0/1.
            Expr::Unary {
                op: UnaryOp::Not,
                expr,
            } => self.expr_is_int(expr),
            _ => false,
        }
    }

    /// fusevm-native comparison op for an integer compare, or `None` for the
    /// dynamic ops (`=~`/`!~`/`is`/`isnot`) that have no numeric form. The
    /// result is a `Value::Bool` — correct only when consumed by a jump
    /// (condition position), so this is used solely for `:if`/`:while` tests.
    fn native_cmp(op: CmpOp) -> Option<Op> {
        Some(match op {
            CmpOp::Equal => Op::NumEq,
            CmpOp::NotEqual => Op::NumNe,
            CmpOp::Less => Op::NumLt,
            CmpOp::LessEqual => Op::NumLe,
            CmpOp::Greater => Op::NumGt,
            CmpOp::GreaterEqual => Op::NumGe,
            _ => return None,
        })
    }

    /// Emit a condition that leaves a truthiness flag on the stack for a
    /// following `JumpIf*`. An integer comparison lowers to a native compare op
    /// (no `VIML_TRUTHY` builtin), keeping a numeric loop/if test JIT-eligible;
    /// anything else falls back to the dynamic `expr` + `VIML_TRUTHY` path.
    fn cond(&mut self, e: &Expr) -> Result<(), VimlError> {
        match e {
            // Integer/float comparison → native compare op (Bool consumed by the
            // following jump, never reified).
            Expr::Compare { op, lhs, rhs, .. }
                if Self::native_cmp(*op).is_some()
                    && self.expr_is_num(lhs)
                    && self.expr_is_num(rhs) =>
            {
                let nop = Self::native_cmp(*op).unwrap();
                self.expr(lhs)?;
                self.expr(rhs)?;
                self.emit(nop);
                Ok(())
            }
            // `a && b` — short-circuit, leaving one truthiness flag. Stays
            // CallBuiltin-free when both arms are native, so a compound loop
            // condition still traces.
            Expr::And(a, b) => {
                self.cond(a)?;
                let to_false = self.emit(Op::JumpIfFalse(0)); // a false → result false
                self.cond(b)?;
                let to_end = self.emit(Op::Jump(0));
                let l_false = self.b.current_pos();
                self.b.patch_jump(to_false, l_false);
                self.emit(Op::LoadFalse);
                let l_end = self.b.current_pos();
                self.b.patch_jump(to_end, l_end);
                Ok(())
            }
            // `a || b` — short-circuit.
            Expr::Or(a, b) => {
                self.cond(a)?;
                let to_true = self.emit(Op::JumpIfTrue(0)); // a true → result true
                self.cond(b)?;
                let to_end = self.emit(Op::Jump(0));
                let l_true = self.b.current_pos();
                self.b.patch_jump(to_true, l_true);
                self.emit(Op::LoadTrue);
                let l_end = self.b.current_pos();
                self.b.patch_jump(to_end, l_end);
                Ok(())
            }
            _ => {
                self.expr(e)?;
                self.emit(Op::CallBuiltin(h::VIML_TRUTHY, 1));
                Ok(())
            }
        }
    }

    fn expr(&mut self, e: &Expr) -> Result<(), VimlError> {
        match e {
            Expr::Number(n) => {
                self.emit(Op::LoadInt(*n));
            }
            Expr::Float(f) => {
                self.emit(Op::LoadFloat(*f));
            }
            Expr::Str(s) => self.load_str(s),
            Expr::Bytes(b) => self.load_bytes(b),
            // A parse-position error Vim reports only when the expression runs
            // (same rationale as the wrong-argc call → `VIML_RAISE` lowering).
            Expr::ScriptError(msg) => {
                self.load_str(msg);
                self.emit(Op::CallBuiltin(h::VIML_RAISE, 1));
            }
            // c: a parse failure inside eval1 fails the WHOLE expression even
            // when it sits in a branch evaluation never reaches — so evaluate
            // the tree (its own errors report first; `VIML_RAISE` yields to
            // any earlier error), drop the value, and raise the deferred E15.
            // The result is the C's `rettv->vval.v_number = 0`.
            Expr::ScriptErrorGuard { inner, msg } => {
                self.expr(inner)?;
                self.emit(Op::Pop);
                self.load_str(msg);
                self.emit(Op::CallBuiltin(h::VIML_RAISE, 1));
            }
            // A Funcref is carried as a tagged string in the VM (see
            // `tv_to_value`); the null one is that tag with an empty name.
            Expr::NullFunc => self.load_str("\u{1}func\u{1}"),
            Expr::Interp(segs) => {
                // Echo-stringify each segment (`VIML_STR_INTERP`) and concatenate
                // left to right; an empty interpolation is the empty string.
                if segs.is_empty() {
                    self.load_str("");
                } else {
                    for (i, seg) in segs.iter().enumerate() {
                        self.expr(seg)?;
                        self.emit(Op::CallBuiltin(h::VIML_STR_INTERP, 1));
                        if i > 0 {
                            self.emit(Op::CallBuiltin(h::VIML_CONCAT, 2));
                        }
                    }
                }
            }
            Expr::Var(name) => {
                self.get_var(name);
            }
            Expr::Option(name) => {
                self.load_str(name);
                self.emit(Op::CallBuiltin(h::VIML_GETOPT, 1));
            }
            Expr::Env(name) => {
                self.load_str(name);
                self.emit(Op::CallBuiltin(h::VIML_GETENV, 1));
            }
            Expr::Register(r) => {
                self.load_str(&r.to_string());
                self.emit(Op::CallBuiltin(h::VIML_GETREG, 1));
            }
            Expr::List(items) => {
                // A single `VIML_MAKE_LIST` op carries the element count in a `u8`
                // (max 255). VimL puts no size limit on a List literal, so for a
                // longer literal build it in chunks of 255 and concatenate the
                // chunks with `+` (`VIML_ADD` List+List concat) — an identical
                // List, no size cap. (Vim corpus e.g. clojurecomplete.vim's ~700
                // element completion list.)
                const MAX: usize = u8::MAX as usize;
                if items.len() <= MAX {
                    for it in items {
                        self.expr(it)?;
                    }
                    self.emit(Op::CallBuiltin(h::VIML_MAKE_LIST, items.len() as u8));
                } else {
                    let mut chunks = items.chunks(MAX);
                    let first = chunks.next().unwrap();
                    for it in first {
                        self.expr(it)?;
                    }
                    self.emit(Op::CallBuiltin(h::VIML_MAKE_LIST, first.len() as u8));
                    for chunk in chunks {
                        for it in chunk {
                            self.expr(it)?;
                        }
                        self.emit(Op::CallBuiltin(h::VIML_MAKE_LIST, chunk.len() as u8));
                        self.emit(Op::CallBuiltin(h::VIML_ADD, 2));
                    }
                }
            }
            Expr::Dict(pairs) => {
                // A single `VIML_MAKE_DICT` op carries the slot count (2 per pair)
                // in a `u8` (max 255), so a literal caps at 127 pairs. VimL puts no
                // size limit on a Dict literal, so for a longer literal build it in
                // chunks of 127 pairs and merge the chunks with `extend()` (in-place
                // merge returning the first dict) — an identical Dict, no size cap.
                // (Vim corpus e.g. colors/lists/default.vim's 788-entry v:colornames
                // extend.)
                const MAX_PAIRS: usize = (u8::MAX as usize) / 2; // 127
                if pairs.len() <= MAX_PAIRS {
                    for (k, v) in pairs {
                        self.expr(k)?;
                        self.expr(v)?;
                    }
                    self.emit(Op::CallBuiltin(h::VIML_MAKE_DICT, (pairs.len() * 2) as u8));
                } else {
                    let mut chunks = pairs.chunks(MAX_PAIRS);
                    let first = chunks.next().unwrap();
                    for (k, v) in first {
                        self.expr(k)?;
                        self.expr(v)?;
                    }
                    self.emit(Op::CallBuiltin(h::VIML_MAKE_DICT, (first.len() * 2) as u8));
                    for chunk in chunks {
                        for (k, v) in chunk {
                            self.expr(k)?;
                            self.expr(v)?;
                        }
                        self.emit(Op::CallBuiltin(h::VIML_MAKE_DICT, (chunk.len() * 2) as u8));
                        self.emit(Op::CallBuiltin(h::VIML_FN_EXTEND, 2));
                    }
                }
            }
            Expr::Lambda { params, body, vim9 } => {
                // c: `get_lambda_tv` — an anonymous function `<lambda>N` whose
                // body is `return {expr}`. Its parameters are bound as bare
                // locals (so the body names them without `a:`).
                let name = next_lambda_name();
                // A lambda body is one expression on one line, so every
                // statement of the synthesized body reports line 1 — which is
                // what vim reports for a throw inside `{x -> …}`.
                let mut stmts: Block = params
                    .iter()
                    .map(|p| {
                        (
                            1,
                            Stmt::Let {
                                target: LetTarget::Var(p.clone()),
                                expr: Expr::Var(format!("a:{p}")),
                            },
                        )
                    })
                    .collect();
                stmts.push((1, Stmt::Return(Some((**body).clone()))));
                let chunk = compile_function_body(&stmts, self.exc, 0, false, *vim9, true)?;
                LAMBDA_FUNCS.with(|f| {
                    f.borrow_mut().push(UserFuncDef {
                        name: name.clone(),
                        params: params.clone(),
                        defaults: Vec::new(),
                        bang: true,
                        vim9: *vim9,
                        // c: `get_lambda_tv` builds the ufunc with
                        // `FC_LAMBDA|FC_CLOSURE`, never `FC_DICT` — a lambda
                        // stored in a Dict is not bound to it.
                        // c: `get_lambda_tv` sets no `FC_ABORT` either.
                        abort: false,
                        dict: false,
                        // Whether THIS evaluation is a closure is decided when
                        // it runs, and carried by the value (`VIML_MAKE_LAMBDA`).
                        closure: false,
                        scoped: None,
                        chunk,
                    })
                });
                // c: while the body is parsed, `check_vars` sets `eval_lavars`
                // for every name it reads that is a local or an argument of the
                // current function; `VIML_MAKE_LAMBDA` makes that test against
                // the names collected here.
                let mut names = std::collections::BTreeSet::new();
                collect_free_vars(body, &mut Vec::new(), &mut names);
                self.load_str(&name);
                let n = names.len().min(u8::MAX as usize - 1);
                for v in names.iter().take(n) {
                    self.load_str(v);
                }
                self.emit(Op::CallBuiltin(h::VIML_MAKE_LAMBDA, (n + 1) as u8));
            }
            Expr::Unary { op, expr } => {
                // Native numeric negation → `Op::Negate` (Int wrapping-negates,
                // Float negates — exactly VimL), so `-x` keeps a loop JIT-able.
                if matches!(op, UnaryOp::Neg) && self.expr_is_num(expr) {
                    self.expr(expr)?;
                    self.emit(Op::Negate);
                    return Ok(());
                }
                // Native logical-not of an integer: `!x` == `x == 0`, reified to
                // VimL's Number 0/1 with a branch (all JIT-lowerable), so `!flag`
                // / `!(i % 2)` keep a loop traceable. (Restricted to Integer
                // operands; a Float would diverge from Vim's E805.)
                if matches!(op, UnaryOp::Not) && self.expr_is_int(expr) {
                    self.expr(expr)?;
                    self.emit(Op::LoadInt(0));
                    self.emit(Op::NumEq);
                    let jf = self.emit(Op::JumpIfFalse(0));
                    self.emit(Op::LoadInt(1));
                    let jend = self.emit(Op::Jump(0));
                    let lfalse = self.b.current_pos();
                    self.b.patch_jump(jf, lfalse);
                    self.emit(Op::LoadInt(0));
                    let lend = self.b.current_pos();
                    self.b.patch_jump(jend, lend);
                    return Ok(());
                }
                self.expr(expr)?;
                let id = match op {
                    UnaryOp::Neg => h::VIML_NEG,
                    UnaryOp::Plus => h::VIML_UPLUS,
                    UnaryOp::Not => h::VIML_NOT,
                };
                self.emit(Op::CallBuiltin(id, 1));
            }
            Expr::Arith {
                op,
                lhs,
                rhs,
                mod_op,
            } => {
                // JIT fast path: integer `+`/`-`/`*` lower to fusevm-NATIVE ops
                // (`Op::Add`/`Sub`/`Mul`) so the chunk stays eligible for the
                // 3-tier Cranelift JIT. Sound because `Value::Int` <-> Number
                // typval is transparent at the VM-stack boundary (fusevm_bridge
                // `tv_to_value`/`value_to_tv`), and i64 wrap matches VimL's
                // `varnumber_T` arithmetic. `/`/`%` keep the builtin (VimL's
                // div-by-zero semantics differ from `sdiv`/`srem` traps);
                // `Concat` is a string op; non-int operands keep the dynamic
                // builtin (`b_add` etc.) which is also the JIT deopt fallback.
                let native = match op {
                    ArithOp::Add => Some(Op::Add),
                    ArithOp::Sub => Some(Op::Sub),
                    ArithOp::Mul => Some(Op::Mul),
                    _ => None,
                };
                if let Some(nop) = native {
                    if self.expr_is_num(lhs) && self.expr_is_num(rhs) {
                        self.expr(lhs)?;
                        self.expr(rhs)?;
                        self.emit(nop);
                        return Ok(());
                    }
                }
                // Native `%` for INTEGER operands only: fusevm `Op::Mod` is
                // `(y==0)?0:x%y`, identical to the `num_modulus` port, and Rust
                // `%` is C-truncated like VimL. Floats diverge (VimL errors on
                // `%` with a Float), so they keep the builtin. (`/` always stays
                // on the builtin — fusevm `Op::Div` is float division, unlike
                // VimL's integer `/`.)
                if matches!(op, ArithOp::Mod) && self.expr_is_int(lhs) && self.expr_is_int(rhs) {
                    self.expr(lhs)?;
                    self.expr(rhs)?;
                    self.emit(Op::Mod);
                    return Ok(());
                }
                // c: `ex_let_one` applies a COMPOUND assignment with `eexe_mod_op`
                // (`vendor/eval/executor.c:201`), which is a type table rather than
                // the coercing expression operator: `let d = {} | let d += [1]` is
                // `E734: Wrong variable type for +=` in vim, not E745. The native
                // paths above stay — they are only taken when both operands are
                // provably Number, and `eexe_mod_op` agrees with them there.
                if *mod_op {
                    // c: `ex_let_one` reads the current value with `eval_variable`
                    // and RETURNS if that failed — `let g:nosuch += 1` is E121 alone,
                    // never also E734. Same for a right-hand side that failed.
                    self.emit(Op::CallBuiltin(h::VIML_ARGS_BEGIN, 0));
                    self.emit(Op::Pop);
                    self.expr(lhs)?;
                    self.expr(rhs)?;
                    self.emit(Op::CallBuiltin(h::VIML_ARGS_FAILED, 0));
                    let ok = self.emit(Op::JumpIfFalse(0));
                    self.emit(Op::Pop); // rhs
                    self.emit(Op::Pop); // lhs
                                        // The `:let` store is skipped by its own failure guard, so the
                                        // value left here is never written anywhere.
                    self.emit(Op::LoadInt(0));
                    let to_end = self.emit(Op::Jump(0));
                    let here = self.b.current_pos();
                    self.b.patch_jump(ok, here);
                    self.load_str(match op {
                        ArithOp::Add => "+",
                        ArithOp::Sub => "-",
                        ArithOp::Mul => "*",
                        ArithOp::Div => "/",
                        ArithOp::Mod => "%",
                        ArithOp::Concat => ".",
                        ArithOp::ShiftL => "<<",
                        ArithOp::ShiftR => ">>",
                    });
                    self.emit(Op::CallBuiltin(h::VIML_MOD_OP, 3));
                    let end = self.b.current_pos();
                    self.b.patch_jump(to_end, end);
                    return Ok(());
                }
                self.expr(lhs)?;
                // c: eval5 (c:2405) type-checks the LEFT operand of `+`, `-` and `.`
                // *before it even parses the right one*, "to avoid side effects after
                // an error" — so `0z - remove(d, k)` reports the Blob (E974) and never
                // runs the removal. `*`, `/` and `%` do NOT do this: the C evaluates
                // their right operand first too, so they already agree.
                //
                // A statically-numeric left operand can never fail either check
                // (a Number passes both tv_check_num and tv_check_str), so the check
                // is skipped there and `i + 1` keeps its native-arithmetic fast path.
                // c: vim `eval5()` checks a shift's left operand the same way, and
                // there a Float passes `expr_is_num` but must still fail (E1282).
                if !self.expr_is_num(lhs) || matches!(op, ArithOp::ShiftL | ArithOp::ShiftR) {
                    let chk = match op {
                        ArithOp::Add => Some(h::VIML_CHECK_LHS_ADD),
                        ArithOp::Sub => Some(h::VIML_CHECK_LHS_SUB),
                        ArithOp::Concat => Some(h::VIML_CHECK_LHS_CONCAT),
                        ArithOp::ShiftL | ArithOp::ShiftR => Some(h::VIML_CHECK_LHS_SHIFT),
                        _ => None,
                    };
                    if let Some(chk) = chk {
                        self.emit(Op::CallBuiltin(chk, 1));
                        // c: eval5 returns FAIL before it parses the right
                        // operand, so a refused left operand skips it and its
                        // side effects (`[] - remove(l, 0)` leaves `l` alone).
                        self.emit(Op::CallBuiltin(h::VIML_LHS_CHECK_FAILED, 0));
                        let to_rhs = self.emit(Op::JumpIfFalse(0));
                        self.emit(Op::LoadInt(0));
                        let to_op = self.emit(Op::Jump(0));
                        let rhs_at = self.b.current_pos();
                        self.b.patch_jump(to_rhs, rhs_at);
                        self.expr(rhs)?;
                        let op_at = self.b.current_pos();
                        self.b.patch_jump(to_op, op_at);
                    } else {
                        self.expr(rhs)?;
                    }
                } else {
                    self.expr(rhs)?;
                }
                let id = match op {
                    ArithOp::Add => h::VIML_ADD,
                    ArithOp::Sub => h::VIML_SUB,
                    ArithOp::Mul => h::VIML_MUL,
                    ArithOp::Div => h::VIML_DIV,
                    ArithOp::Mod => h::VIML_MOD,
                    ArithOp::Concat => h::VIML_CONCAT,
                    ArithOp::ShiftL => h::VIML_SHIFT_L,
                    ArithOp::ShiftR => h::VIML_SHIFT_R,
                };
                self.emit(Op::CallBuiltin(id, 2));
            }
            Expr::Compare { op, case, lhs, rhs } => {
                // Value-position compare of numeric operands → native compare
                // (`cond()`) reified to VimL's Number 0/1 with a tiny branch (all
                // JIT-lowerable ops), so `let s += i > 5` keeps a loop traceable.
                // The case flag is irrelevant for numbers. Non-numeric operands
                // (or `is`/`isnot`) keep the builtin, which yields 0/1 directly.
                if Self::native_cmp(*op).is_some() && self.expr_is_num(lhs) && self.expr_is_num(rhs)
                {
                    self.cond(e)?; // native compare → Bool on the stack
                    let jf = self.emit(Op::JumpIfFalse(0));
                    self.emit(Op::LoadInt(1));
                    let jend = self.emit(Op::Jump(0));
                    let lfalse = self.b.current_pos();
                    self.b.patch_jump(jf, lfalse);
                    self.emit(Op::LoadInt(0));
                    let lend = self.b.current_pos();
                    self.b.patch_jump(jend, lend);
                    return Ok(());
                }
                self.expr(lhs)?;
                self.expr(rhs)?;
                self.emit(Op::CallBuiltin(h::cmp_id(*op, *case), 2));
            }
            Expr::And(a, b) => self.logical_and(a, b)?,
            Expr::Or(a, b) => self.logical_or(a, b)?,
            Expr::Ternary {
                cond,
                then,
                otherwise,
            } => self.ternary(cond, then, otherwise)?,
            Expr::Coalesce(a, b) => self.coalesce(a, b)?,
            Expr::Index { base, index } => {
                self.expr(base)?;
                self.expr(index)?;
                self.emit(Op::CallBuiltin(h::VIML_INDEX, 2));
            }
            Expr::Slice { base, from, to } => {
                self.expr(base)?;
                self.opt_bound(from)?;
                self.opt_bound(to)?;
                self.emit(Op::CallBuiltin(h::VIML_SLICE, 3));
            }
            Expr::Member { base, key } => {
                // A no-space `base.key` is syntactically ambiguous: it is a Dict
                // subscript `base['key']` when `base` is a Dict at runtime, and
                // string concatenation `base . key` (a bare variable read) in
                // every other case. Vim decides by runtime type, so lower to a
                // type test that dispatches at execution time. `base` is
                // evaluated exactly ONCE (Dup the value), so a side-effecting base
                // fires once and chains like `a.b.c` do not blow up.
                self.expr(base)?; // [base]
                self.emit(Op::Dup); // [base, base]
                self.emit(Op::CallBuiltin(h::VIML_IS_DICT, 1)); // [base, bool]
                let jf = self.emit(Op::JumpIfFalse(0)); // pops bool → [base]
                                                        // Dict branch: subscript with the literal key.
                self.load_str(key); // [base, "key"]
                self.emit(Op::CallBuiltin(h::VIML_INDEX, 2)); // [value]
                let jend = self.emit(Op::Jump(0));
                // Concat branch: `base . <var named key>` — matches spaced `a . b`.
                let lconcat = self.b.current_pos();
                self.b.patch_jump(jf, lconcat);
                self.get_var(key); // [base, varval]
                self.emit(Op::CallBuiltin(h::VIML_CONCAT, 2)); // [result]
                let lend = self.b.current_pos();
                self.b.patch_jump(jend, lend);
            }
            Expr::Call {
                name,
                args,
                emsg_name,
            } => {
                // JIT fast path: the bitwise builtins lower to fusevm-NATIVE ops
                // when every argument is provably integer, so bit-manipulation
                // loops stay JIT-eligible. `f_and` is `a & b` over `tv_get_number`,
                // and fusevm `Op::BitAnd` is `to_int() & to_int()` — identical for
                // Int operands. Non-int args keep the builtin (the deopt fallback).
                if let Some(nop) = bitwise_native_op(name, args.len()) {
                    if args.iter().all(|a| self.expr_is_int(a)) {
                        for a in args {
                            self.expr(a)?;
                        }
                        self.emit(nop);
                        return Ok(());
                    }
                }
                // A `rust { ... }` block's exported functions are callable by
                // bareword and SHADOW a Vim builtin of the same name (e.g. an
                // exported `add` overrides the list `add()` builtin), mirroring
                // how a PHP `rust` export shadows the standard library. Route
                // such names through the runtime call path so the FFI fallback
                // in `b_call_user` resolves them; a user `:function` still wins
                // (it is looked up before the FFI registry there).
                let id = builtin_fn_id(name).filter(|_| !crate::rust_ffi::is_ffi_export(name));
                // A wrong argument count is an error Vim raises when the command
                // RUNS, not when the script loads: rejecting it at compile time made
                // an *unreachable* bad call abort the whole script (`if 0 | echo
                // strlen('a','b') | endif` loads fine in Vim). And it raises the
                // count error only AFTER evaluating the arguments — `call_func`
                // (`vendor/eval/userfunc.c:580`) runs after `get_func_arguments`
                // (c:559), so the arguments' side effects happen and an argument
                // that FAILED pre-empts the count error with E116. Verified against
                // vim 9.2: `echo strlen(Side(1), Side(2))` prints both of `Side`'s
                // messages and then E118.
                let argc_err = id.and_then(|_| builtin_argc_error(name, args.len()));
                // c: `get_func_tv` calls `call_func` only when `get_func_arguments`
                // returned OK, and reports `E116: Invalid arguments for function %s`
                // otherwise (`vendor/eval/userfunc.c:559-588`) — a SECOND diagnostic
                // after the one the argument itself raised:
                //
                //   echo type([1] . '')
                //   " E730: Using a List as a String
                //   " E116: Invalid arguments for function type([1] . '')
                //
                // Skipped when no argument could fail at all, so a literal call keeps
                // its original ops.
                let guarded = args.iter().any(Self::expr_can_error);
                if guarded {
                    self.emit(Op::CallBuiltin(h::VIML_ARGS_BEGIN, 0));
                    self.emit(Op::Pop);
                }
                // A user-function call pushes its name below the arguments, so the
                // failure path has one more stack value to discard.
                let extra = usize::from(id.is_none());
                if id.is_none() {
                    self.load_str(name);
                }
                for a in args {
                    self.expr(a)?;
                }
                let mut to_end = None;
                if guarded {
                    self.emit(Op::CallBuiltin(h::VIML_ARGS_FAILED, 0));
                    let ok = self.emit(Op::JumpIfFalse(0));
                    for _ in 0..args.len() + extra {
                        self.emit(Op::Pop);
                    }
                    // c:587 formats the message over the `name` POINTER, which aims
                    // into the source — see `Expr::Call::emsg_name`.
                    self.load_str(emsg_name.as_deref().unwrap_or(name));
                    // The bare name too: the runtime needs it to tell a call of a
                    // Funcref-valued VARIABLE from a call of a function.
                    self.load_str(name);
                    self.emit(Op::CallBuiltin(h::VIML_ARGS_E116, 2));
                    to_end = Some(self.emit(Op::Jump(0)));
                    let here = self.b.current_pos();
                    self.b.patch_jump(ok, here);
                }
                let n = Self::argc(args.len())?;
                match (argc_err, id) {
                    // c: `call_func` rejects the count with `FCERR_TOOMANY` /
                    // `FCERR_TOOFEW` (`vendor/eval/userfunc.c:1625-1628`) once the
                    // arguments are on the stack — so they are discarded here.
                    (Some(msg), _) => {
                        for _ in 0..args.len() + extra {
                            self.emit(Op::Pop);
                        }
                        self.load_str(&msg);
                        self.emit(Op::CallBuiltin(h::VIML_RAISE, 1));
                    }
                    (None, Some(id)) => {
                        self.emit(Op::CallBuiltin(id, n));
                    }
                    // Unknown name → user-defined function call (resolved by name
                    // at runtime). Stack: [name, arg0, …, argN].
                    (None, None) => {
                        self.emit(Op::CallBuiltin(h::VIML_CALL_USER, n));
                        self.emit_call_unwind_check();
                    }
                }
                if let Some(j) = to_end {
                    let end = self.b.current_pos();
                    self.b.patch_jump(j, end);
                }
            }
            // `expr(args)` — evaluate the callee to a Funcref/Partial, push the
            // args, then call the value. Stack: [funcref, arg0, …, argN].
            // `base.name(args)` — the same runtime Dict test as `Expr::Member`,
            // with the call applied inside each branch: a Dict base calls the
            // funcref at `base['name']`, anything else concatenates `base` with
            // the result of calling `name(args)` (`substitute(…).submatch(0)`).
            Expr::MemberCall { base, key, args } => {
                self.expr(base)?; // [base]
                self.emit(Op::Dup); // [base, base]
                self.emit(Op::CallBuiltin(h::VIML_IS_DICT, 1)); // [base, bool]
                let jf = self.emit(Op::JumpIfFalse(0)); // pops bool → [base]
                                                        // Dict branch: call the funcref stored under the
                                                        // key, with the Dict bound as `self`.
                self.load_str(key); // [base, "key"]
                for a in args {
                    self.expr(a)?;
                }
                self.emit(Op::CallBuiltin(
                    h::VIML_CALL_MEMBER,
                    Self::argc(args.len())?,
                ));
                self.emit_call_unwind_check();
                let jend = self.emit(Op::Jump(0));
                // Concat branch: `base . key(args)`.
                let lconcat = self.b.current_pos();
                self.b.patch_jump(jf, lconcat);
                self.expr(&Expr::Call {
                    name: key.clone(),
                    args: args.clone(),
                    emsg_name: None,
                })?; // [base, value]
                self.emit(Op::CallBuiltin(h::VIML_CONCAT, 2)); // [result]
                let lend = self.b.current_pos();
                self.b.patch_jump(jend, lend);
            }
            // `d['key'](args)` / `l[0](args)` — the subscript-then-call form.
            // Routed through `VIML_CALL_MEMBER` so a Dict base binds `self`, the
            // same way `d.key(args)` does (c: `handle_subscript` sets `selfdict`
            // from the Dict it just indexed).
            Expr::CallExpr { callee, args } if matches!(**callee, Expr::Index { .. }) => {
                let Expr::Index { base, index } = &**callee else {
                    unreachable!("guarded by the match arm")
                };
                self.expr(base)?;
                self.expr(index)?;
                for a in args {
                    self.expr(a)?;
                }
                self.emit(Op::CallBuiltin(
                    h::VIML_CALL_MEMBER,
                    Self::argc(args.len())?,
                ));
                self.emit_call_unwind_check();
            }
            Expr::CallExpr { callee, args } => {
                self.expr(callee)?;
                for a in args {
                    self.expr(a)?;
                }
                self.emit(Op::CallBuiltin(
                    h::VIML_CALL_FUNCREF,
                    Self::argc(args.len())?,
                ));
                self.emit_call_unwind_check();
            }
            Expr::Method { base, name, args } => {
                match builtin_fn_id(name).filter(|_| !crate::rust_ffi::is_ffi_export(name)) {
                    Some(id) => {
                        // See the note on the plain-call path: a mis-arity call raises at
                        // runtime, not at compile time.
                        if let Some(msg) = builtin_argc_error(name, args.len() + 1) {
                            self.load_str(&msg);
                            self.emit(Op::CallBuiltin(h::VIML_RAISE, 1));
                            return Ok(());
                        }
                        self.expr(base)?;
                        for a in args {
                            self.expr(a)?;
                        }
                        self.emit(Op::CallBuiltin(id, Self::argc(args.len() + 1)?));
                    }
                    None => {
                        self.load_str(name);
                        self.expr(base)?;
                        for a in args {
                            self.expr(a)?;
                        }
                        self.emit(Op::CallBuiltin(
                            h::VIML_CALL_USER,
                            Self::argc(args.len() + 1)?,
                        ));
                        self.emit_call_unwind_check();
                    }
                }
            }
        }
        Ok(())
    }

    fn opt_bound(&mut self, b: &Option<Box<Expr>>) -> Result<(), VimlError> {
        match b {
            Some(e) => self.expr(e),
            None => {
                self.emit(Op::LoadUndef);
                Ok(())
            }
        }
    }

    fn logical_and(&mut self, a: &Expr, b: &Expr) -> Result<(), VimlError> {
        self.expr(a)?;
        self.emit(Op::CallBuiltin(h::VIML_TRUTHY, 1));
        let jf = self.emit(Op::JumpIfFalse(0));
        self.expr(b)?;
        self.emit(Op::CallBuiltin(h::VIML_BOOLNUM, 1));
        let jend = self.emit(Op::Jump(0));
        let lfalse = self.b.current_pos();
        self.emit(Op::LoadInt(0));
        let lend = self.b.current_pos();
        self.b.patch_jump(jf, lfalse);
        self.b.patch_jump(jend, lend);
        Ok(())
    }

    fn logical_or(&mut self, a: &Expr, b: &Expr) -> Result<(), VimlError> {
        self.expr(a)?;
        self.emit(Op::CallBuiltin(h::VIML_TRUTHY, 1));
        let jt = self.emit(Op::JumpIfTrue(0));
        self.expr(b)?;
        self.emit(Op::CallBuiltin(h::VIML_BOOLNUM, 1));
        let jend = self.emit(Op::Jump(0));
        let ltrue = self.b.current_pos();
        self.emit(Op::LoadInt(1));
        let lend = self.b.current_pos();
        self.b.patch_jump(jt, ltrue);
        self.b.patch_jump(jend, lend);
        Ok(())
    }

    fn ternary(&mut self, cond: &Expr, then: &Expr, otherwise: &Expr) -> Result<(), VimlError> {
        // Lower the test through `cond()` (native compare / short-circuit `&&`/`||`)
        // so a numeric ternary like `i % 2 == 0 ? i : 0` stays CallBuiltin-free and
        // keeps an enclosing loop trace-eligible; non-native tests fall back to
        // `VIML_TRUTHY` inside `cond()`.
        self.cond(cond)?;
        let jf = self.emit(Op::JumpIfFalse(0));
        self.expr(then)?;
        let jend = self.emit(Op::Jump(0));
        let lelse = self.b.current_pos();
        self.expr(otherwise)?;
        let lend = self.b.current_pos();
        self.b.patch_jump(jf, lelse);
        self.b.patch_jump(jend, lend);
        Ok(())
    }

    fn coalesce(&mut self, a: &Expr, b: &Expr) -> Result<(), VimlError> {
        self.expr(a)?;
        self.emit(Op::Dup);
        self.emit(Op::CallBuiltin(h::VIML_TV2BOOL, 1));
        let jf = self.emit(Op::JumpIfFalse(0));
        let jend = self.emit(Op::Jump(0));
        let lelse = self.b.current_pos();
        self.emit(Op::Pop);
        self.expr(b)?;
        let lend = self.b.current_pos();
        self.b.patch_jump(jf, lelse);
        self.b.patch_jump(jend, lend);
        Ok(())
    }
}

/// Map a builtin function name to its `VIML_FN_*` id, or `None` if it is not a
/// builtin (then it is compiled as a user-function call, resolved at runtime).
/// The fusevm-native op for a VimL bitwise builtin (`and`/`or`/`xor`/`invert`)
/// at the given arity, or `None`. `f_and`=`a&b`/etc. over `tv_get_number`, and
/// the fusevm ops are `to_int()`-based — identical for provably-integer operands,
/// which is the only case the caller lowers natively (else the builtin is kept).
fn bitwise_native_op(name: &str, argc: usize) -> Option<Op> {
    match (name, argc) {
        ("and", 2) => Some(Op::BitAnd),
        ("or", 2) => Some(Op::BitOr),
        ("xor", 2) => Some(Op::BitXor),
        ("invert", 1) => Some(Op::BitNot),
        _ => None,
    }
}

/// The accepted `(min, max)` argument count for a builtin, or `None` when the name is
/// not in the generated table (vimlrs-only builtins, left unchecked).
///
/// Vim checks this when it *parses the expression*, i.e. when the command runs — so a
/// mis-arity call compiles to a runtime raise (see `VIML_RAISE`) rather than being
/// rejected at compile time, which would make an unreachable bad call abort the whole
/// script. The check still guards the leaf `f_*` from indexing a short `argvars[]`.
/// Accepted argument counts for builtins vimlrs implements that Neovim's
/// `eval.lua` — the metadata `funcs_argc.rs` is GENERATED from — does not carry,
/// because Neovim does not have the function. Consulted only when the generated
/// table has no entry for the name, so it can never contradict it.
const EXTRA_BUILTIN_ARGC: &[(&str, u8, u8)] = &[
    // `typename({expr})` is Vim-only (vim9 type introspection). Vim raises E119
    // for `typename()` and E118 for a second argument.
    ("typename", 1, 1),
];

pub(crate) fn builtin_argc_range(name: &str) -> Option<(u8, u8)> {
    use crate::ported::eval::funcs_argc::BUILTIN_ARGC;
    BUILTIN_ARGC
        .binary_search_by(|(n, _, _)| (*n).cmp(name))
        .ok()
        .map(|i| (BUILTIN_ARGC[i].1, BUILTIN_ARGC[i].2))
        .or_else(|| {
            EXTRA_BUILTIN_ARGC
                .iter()
                .find(|(n, _, _)| *n == name)
                .map(|&(_, lo, hi)| (lo, hi))
        })
}

/// The Vim error a builtin call with `argc` arguments would raise, or `None`
/// when the count is acceptable (or the name is unknown). Shared by the
/// compile-time check and the runtime `call()`/funcref dispatch so both report
/// E118/E119 instead of letting a leaf `f_*` panic on a short slice.
pub(crate) fn builtin_argc_error(name: &str, argc: usize) -> Option<String> {
    let (min, max) = builtin_argc_range(name)?;
    if argc < min as usize {
        Some(format!("E119: Not enough arguments for function: {name}"))
    } else if argc > max as usize {
        Some(format!("E118: Too many arguments for function: {name}"))
    } else {
        None
    }
}

pub(crate) fn builtin_fn_id(name: &str) -> Option<u16> {
    Some(match name {
        "len" => h::VIML_FN_LEN,
        "type" => h::VIML_FN_TYPE,
        "typename" => h::VIML_FN_TYPENAME,
        "string" => h::VIML_FN_STRING,
        "empty" => h::VIML_FN_EMPTY,
        "abs" => h::VIML_FN_ABS,
        "str2nr" => h::VIML_FN_STR2NR,
        "str2float" => h::VIML_FN_STR2FLOAT,
        "float2nr" => h::VIML_FN_FLOAT2NR,
        "strlen" => h::VIML_FN_STRLEN,
        "tolower" => h::VIML_FN_TOLOWER,
        "toupper" => h::VIML_FN_TOUPPER,
        "char2nr" => h::VIML_FN_CHAR2NR,
        "nr2char" => h::VIML_FN_NR2CHAR,
        "repeat" => h::VIML_FN_REPEAT,
        "split" => h::VIML_FN_SPLIT,
        "join" => h::VIML_FN_JOIN,
        "range" => h::VIML_FN_RANGE,
        "add" => h::VIML_FN_ADD,
        "reverse" => h::VIML_FN_REVERSE,
        "get" => h::VIML_FN_GET,
        "has_key" => h::VIML_FN_HAS_KEY,
        "keys" => h::VIML_FN_KEYS,
        "values" => h::VIML_FN_VALUES,
        "max" => h::VIML_FN_MAX,
        "min" => h::VIML_FN_MIN,
        "count" => h::VIML_FN_COUNT,
        "index" => h::VIML_FN_INDEX,
        "has" => h::VIML_FN_HAS,
        "exists" => h::VIML_FN_EXISTS,
        "printf" => h::VIML_FN_PRINTF,
        "map" => h::VIML_FN_MAP,
        "filter" => h::VIML_FN_FILTER,
        "mapnew" => h::VIML_FN_MAPNEW,
        "foreach" => h::VIML_FN_FOREACH,
        "dictwatcheradd" => h::VIML_FN_DICTWATCHERADD,
        "dictwatcherdel" => h::VIML_FN_DICTWATCHERDEL,
        "sort" => h::VIML_FN_SORT,
        "call" => h::VIML_FN_CALL,
        "function" => h::VIML_FN_FUNCTION,
        "submatch" => h::VIML_FN_SUBMATCH,
        "json_encode" => h::VIML_FN_JSON_ENCODE,
        "json_decode" => h::VIML_FN_JSON_DECODE,
        "strgetchar" => h::VIML_FN_STRGETCHAR,
        "strcharpart" => h::VIML_FN_STRCHARPART,
        "byteidx" => h::VIML_FN_BYTEIDX,
        "charidx" => h::VIML_FN_CHARIDX,
        "matchstrpos" => h::VIML_FN_MATCHSTRPOS,
        "extendnew" => h::VIML_FN_EXTENDNEW,
        "getenv" => h::VIML_FN_GETENV,
        "setenv" => h::VIML_FN_SETENV,
        "shellescape" => h::VIML_FN_SHELLESCAPE,
        "isinf" => h::VIML_FN_ISINF,
        "isnan" => h::VIML_FN_ISNAN,
        "getpid" => h::VIML_FN_GETPID,
        "localtime" => h::VIML_FN_LOCALTIME,
        // AOP command-intercept extension (vimlrs/zshrs-original; no Vim fn).
        "intercept" => h::VIML_FN_INTERCEPT,
        "intercept_proceed" => h::VIML_FN_INTERCEPT_PROCEED,
        "soundfold" => h::VIML_FN_SOUNDFOLD,
        "byteidxcomp" => h::VIML_FN_BYTEIDXCOMP,
        "reltime" => h::VIML_FN_RELTIME,
        "reltimestr" => h::VIML_FN_RELTIMESTR,
        "reltimefloat" => h::VIML_FN_RELTIMEFLOAT,
        "rand" => h::VIML_FN_RAND,
        "srand" => h::VIML_FN_SRAND,
        "strftime" => h::VIML_FN_STRFTIME,
        "strptime" => h::VIML_FN_STRPTIME,
        "pathshorten" => h::VIML_FN_PATHSHORTEN,
        "isabsolutepath" => h::VIML_FN_ISABSOLUTEPATH,
        "simplify" => h::VIML_FN_SIMPLIFY,
        "filereadable" => h::VIML_FN_FILEREADABLE,
        "filewritable" => h::VIML_FN_FILEWRITABLE,
        "isdirectory" => h::VIML_FN_ISDIRECTORY,
        "getfsize" => h::VIML_FN_GETFSIZE,
        "getftype" => h::VIML_FN_GETFTYPE,
        "getftime" => h::VIML_FN_GETFTIME,
        "getfperm" => h::VIML_FN_GETFPERM,
        "setfperm" => h::VIML_FN_SETFPERM,
        "getcwd" => h::VIML_FN_GETCWD,
        "chdir" => h::VIML_FN_CHDIR,
        "executable" => h::VIML_FN_EXECUTABLE,
        "exepath" => h::VIML_FN_EXEPATH,
        "tempname" => h::VIML_FN_TEMPNAME,
        "mkdir" => h::VIML_FN_MKDIR,
        "delete" => h::VIML_FN_DELETE,
        "rename" => h::VIML_FN_RENAME,
        "readfile" => h::VIML_FN_READFILE,
        "writefile" => h::VIML_FN_WRITEFILE,
        "fnamemodify" => h::VIML_FN_FNAMEMODIFY,
        "filecopy" => h::VIML_FN_FILECOPY,
        "haslocaldir" => h::VIML_FN_HASLOCALDIR,
        "resolve" => h::VIML_FN_RESOLVE,
        "glob2regpat" => h::VIML_FN_GLOB2REGPAT,
        "readdir" => h::VIML_FN_READDIR,
        "readblob" => h::VIML_FN_READBLOB,
        "getreg" => h::VIML_FN_GETREG,
        "getregtype" => h::VIML_FN_GETREGTYPE,
        "getreginfo" => h::VIML_FN_GETREGINFO,
        "setreg" => h::VIML_FN_SETREG,
        "reg_recording" => h::VIML_FN_REG_RECORDING,
        "reg_executing" => h::VIML_FN_REG_EXECUTING,
        "reg_recorded" => h::VIML_FN_REG_RECORDED,
        "gettext" => h::VIML_FN_GETTEXT,
        "garbagecollect" => h::VIML_FN_GARBAGECOLLECT,
        "funcref" => h::VIML_FN_FUNCREF,
        "id" => h::VIML_FN_ID,
        "indexof" => h::VIML_FN_INDEXOF,
        "matchstrlist" => h::VIML_FN_MATCHSTRLIST,
        "fnameescape" => h::VIML_FN_FNAMEESCAPE,
        "shiftwidth" => h::VIML_FN_SHIFTWIDTH,
        "mode" => h::VIML_FN_MODE,
        "state" => h::VIML_FN_STATE,
        "visualmode" => h::VIML_FN_VISUALMODE,
        "pumvisible" => h::VIML_FN_PUMVISIBLE,
        "wildmenumode" => h::VIML_FN_WILDMENUMODE,
        "did_filetype" => h::VIML_FN_DID_FILETYPE,
        "eventhandler" => h::VIML_FN_EVENTHANDLER,
        "hlexists" => h::VIML_FN_HLEXISTS,
        "windowsversion" => h::VIML_FN_WINDOWSVERSION,
        "getfontname" => h::VIML_FN_GETFONTNAME,
        "foreground" => h::VIML_FN_FOREGROUND,
        "prompt_getprompt" => h::VIML_FN_PROMPT_GETPROMPT,
        "pum_getpos" => h::VIML_FN_PUM_GETPOS,
        "serverlist" => h::VIML_FN_SERVERLIST,
        "getpos" => h::VIML_FN_GETPOS,
        "getcharpos" => h::VIML_FN_GETCHARPOS,
        "getcurpos" => h::VIML_FN_GETCURPOS,
        "getcursorcharpos" => h::VIML_FN_GETCURSORCHARPOS,
        "col" => h::VIML_FN_COL,
        "charcol" => h::VIML_FN_CHARCOL,
        "line" => h::VIML_FN_LINE,
        "virtcol" => h::VIML_FN_VIRTCOL,
        "screenrow" => h::VIML_FN_SCREENROW,
        "screencol" => h::VIML_FN_SCREENCOL,
        "screenchar" => h::VIML_FN_SCREENCHAR,
        "screenattr" => h::VIML_FN_SCREENATTR,
        "screenchars" => h::VIML_FN_SCREENCHARS,
        "screenstring" => h::VIML_FN_SCREENSTRING,
        "line2byte" => h::VIML_FN_LINE2BYTE,
        "byte2line" => h::VIML_FN_BYTE2LINE,
        "nextnonblank" => h::VIML_FN_NEXTNONBLANK,
        "prevnonblank" => h::VIML_FN_PREVNONBLANK,
        "wordcount" => h::VIML_FN_WORDCOUNT,
        "getjumplist" => h::VIML_FN_GETJUMPLIST,
        "getchangelist" => h::VIML_FN_GETCHANGELIST,
        "getmarklist" => h::VIML_FN_GETMARKLIST,
        "gettagstack" => h::VIML_FN_GETTAGSTACK,
        "tagfiles" => h::VIML_FN_TAGFILES,
        "taglist" => h::VIML_FN_TAGLIST,
        "tabpagebuflist" => h::VIML_FN_TABPAGEBUFLIST,
        "search" => h::VIML_FN_SEARCH,
        "searchpos" => h::VIML_FN_SEARCHPOS,
        "searchpair" => h::VIML_FN_SEARCHPAIR,
        "searchpairpos" => h::VIML_FN_SEARCHPAIRPOS,
        "searchdecl" => h::VIML_FN_SEARCHDECL,
        "getcharsearch" => h::VIML_FN_GETCHARSEARCH,
        "input" => h::VIML_FN_INPUT,
        "inputsecret" => h::VIML_FN_INPUTSECRET,
        "inputdialog" => h::VIML_FN_INPUTDIALOG,
        "inputlist" => h::VIML_FN_INPUTLIST,
        "inputsave" => h::VIML_FN_INPUTSAVE,
        "inputrestore" => h::VIML_FN_INPUTRESTORE,
        "confirm" => h::VIML_FN_CONFIRM,
        "synID" => h::VIML_FN_SYNID,
        "synIDtrans" => h::VIML_FN_SYNIDTRANS,
        "synIDattr" => h::VIML_FN_SYNIDATTR,
        "synstack" => h::VIML_FN_SYNSTACK,
        "synconcealed" => h::VIML_FN_SYNCONCEALED,
        "changenr" => h::VIML_FN_CHANGENR,
        "swapname" => h::VIML_FN_SWAPNAME,
        "swapfilelist" => h::VIML_FN_SWAPFILELIST,
        "spellbadword" => h::VIML_FN_SPELLBADWORD,
        "spellsuggest" => h::VIML_FN_SPELLSUGGEST,
        "getregion" => h::VIML_FN_GETREGION,
        "getregionpos" => h::VIML_FN_GETREGIONPOS,
        "matchbufline" => h::VIML_FN_MATCHBUFLINE,
        "menu_get" => h::VIML_FN_MENU_GET,
        "timer_info" => h::VIML_FN_TIMER_INFO,
        "timer_start" => h::VIML_FN_TIMER_START,
        "timer_stop" => h::VIML_FN_TIMER_STOP,
        "timer_pause" => h::VIML_FN_TIMER_PAUSE,
        "timer_stopall" => h::VIML_FN_TIMER_STOPALL,
        "setpos" => h::VIML_FN_SETPOS,
        "setcharpos" => h::VIML_FN_SETCHARPOS,
        "cursor" => h::VIML_FN_CURSOR,
        "setcursorcharpos" => h::VIML_FN_SETCURSORCHARPOS,
        "setcharsearch" => h::VIML_FN_SETCHARSEARCH,
        "settagstack" => h::VIML_FN_SETTAGSTACK,
        "assert_equal" => h::VIML_FN_ASSERT_EQUAL,
        "assert_notequal" => h::VIML_FN_ASSERT_NOTEQUAL,
        "assert_true" => h::VIML_FN_ASSERT_TRUE,
        "assert_false" => h::VIML_FN_ASSERT_FALSE,
        "assert_match" => h::VIML_FN_ASSERT_MATCH,
        "assert_notmatch" => h::VIML_FN_ASSERT_NOTMATCH,
        "assert_report" => h::VIML_FN_ASSERT_REPORT,
        "assert_inrange" => h::VIML_FN_ASSERT_INRANGE,
        "assert_exception" => h::VIML_FN_ASSERT_EXCEPTION,
        "assert_fails" => h::VIML_FN_ASSERT_FAILS,
        "system" => h::VIML_FN_SYSTEM,
        "systemlist" => h::VIML_FN_SYSTEMLIST,
        "environ" => h::VIML_FN_ENVIRON,
        "slice" => h::VIML_FN_SLICE,
        "strcharlen" => h::VIML_FN_STRCHARLEN,
        "strtrans" => h::VIML_FN_STRTRANS,
        "strwidth" => h::VIML_FN_STRWIDTH,
        "strdisplaywidth" => h::VIML_FN_STRDISPLAYWIDTH,
        "charclass" => h::VIML_FN_CHARCLASS,
        "glob" => h::VIML_FN_GLOB,
        "globpath" => h::VIML_FN_GLOBPATH,
        "strutf16len" => h::VIML_FN_STRUTF16LEN,
        "utf16idx" => h::VIML_FN_UTF16IDX,
        "bufnr" => h::VIML_FN_BUFNR,
        "bufexists" => h::VIML_FN_BUFEXISTS,
        "buflisted" => h::VIML_FN_BUFLISTED,
        "bufloaded" => h::VIML_FN_BUFLOADED,
        "bufname" => h::VIML_FN_BUFNAME,
        "bufwinnr" => h::VIML_FN_BUFWINNR,
        "bufwinid" => h::VIML_FN_BUFWINID,
        "winnr" => h::VIML_FN_WINNR,
        "winbufnr" => h::VIML_FN_WINBUFNR,
        "winwidth" => h::VIML_FN_WINWIDTH,
        "winheight" => h::VIML_FN_WINHEIGHT,
        "winlayout" => h::VIML_FN_WINLAYOUT,
        "winline" => h::VIML_FN_WINLINE,
        "wincol" => h::VIML_FN_WINCOL,
        "winrestcmd" => h::VIML_FN_WINRESTCMD,
        "tabpagenr" => h::VIML_FN_TABPAGENR,
        "tabpagewinnr" => h::VIML_FN_TABPAGEWINNR,
        "getline" => h::VIML_FN_GETLINE,
        "getbufline" => h::VIML_FN_GETBUFLINE,
        "getbufoneline" => h::VIML_FN_GETBUFONELINE,
        "getbufinfo" => h::VIML_FN_GETBUFINFO,
        "setline" => h::VIML_FN_SETLINE,
        "setbufline" => h::VIML_FN_SETBUFLINE,
        "append" => h::VIML_FN_APPEND,
        "appendbufline" => h::VIML_FN_APPENDBUFLINE,
        "deletebufline" => h::VIML_FN_DELETEBUFLINE,
        "getwininfo" => h::VIML_FN_GETWININFO,
        "gettabinfo" => h::VIML_FN_GETTABINFO,
        "getwinpos" => h::VIML_FN_GETWINPOS,
        "getwinposx" => h::VIML_FN_GETWINPOSX,
        "getwinposy" => h::VIML_FN_GETWINPOSY,
        "win_getid" => h::VIML_FN_WIN_GETID,
        "win_id2win" => h::VIML_FN_WIN_ID2WIN,
        "win_findbuf" => h::VIML_FN_WIN_FINDBUF,
        "win_gotoid" => h::VIML_FN_WIN_GOTOID,
        "win_gettype" => h::VIML_FN_WIN_GETTYPE,
        "win_screenpos" => h::VIML_FN_WIN_SCREENPOS,
        "expand" => h::VIML_FN_EXPAND,
        "expandcmd" => h::VIML_FN_EXPANDCMD,
        "win_id2tabwin" => h::VIML_FN_WIN_ID2TABWIN,
        "win_splitmove" => h::VIML_FN_WIN_SPLITMOVE,
        "win_move_separator" => h::VIML_FN_WIN_MOVE_SEPARATOR,
        "win_move_statusline" => h::VIML_FN_WIN_MOVE_STATUSLINE,
        "getcmdwintype" => h::VIML_FN_GETCMDWINTYPE,
        "winrestview" => h::VIML_FN_WINRESTVIEW,
        "winsaveview" => h::VIML_FN_WINSAVEVIEW,
        "bufload" => h::VIML_FN_BUFLOAD,
        "prompt_getinput" => h::VIML_FN_PROMPT_GETINPUT,
        "prompt_setprompt" => h::VIML_FN_PROMPT_SETPROMPT,
        "prompt_setcallback" => h::VIML_FN_PROMPT_SETCALLBACK,
        "prompt_setinterrupt" => h::VIML_FN_PROMPT_SETINTERRUPT,
        "interrupt" => h::VIML_FN_INTERRUPT,
        "debugbreak" => h::VIML_FN_DEBUGBREAK,
        "api_info" => h::VIML_FN_API_INFO,
        "swapinfo" => h::VIML_FN_SWAPINFO,
        "serverstart" => h::VIML_FN_SERVERSTART,
        "serverstop" => h::VIML_FN_SERVERSTOP,
        "getbufvar" => h::VIML_FN_GETBUFVAR,
        "getwinvar" => h::VIML_FN_GETWINVAR,
        "gettabvar" => h::VIML_FN_GETTABVAR,
        "gettabwinvar" => h::VIML_FN_GETTABWINVAR,
        "setbufvar" => h::VIML_FN_SETBUFVAR,
        "setwinvar" => h::VIML_FN_SETWINVAR,
        "settabvar" => h::VIML_FN_SETTABVAR,
        "settabwinvar" => h::VIML_FN_SETTABWINVAR,
        "jobstart" => h::VIML_FN_JOBSTART,
        "jobpid" => h::VIML_FN_JOBPID,
        "jobstop" => h::VIML_FN_JOBSTOP,
        "jobwait" => h::VIML_FN_JOBWAIT,
        "jobresize" => h::VIML_FN_JOBRESIZE,
        "chanclose" => h::VIML_FN_CHANCLOSE,
        "chansend" => h::VIML_FN_CHANSEND,
        "feedkeys" => h::VIML_FN_FEEDKEYS,
        "wait" => h::VIML_FN_WAIT,
        "sockconnect" => h::VIML_FN_SOCKCONNECT,
        "win_execute" => h::VIML_FN_WIN_EXECUTE,
        "bufadd" => h::VIML_FN_BUFADD,
        "ctxget" => h::VIML_FN_CTXGET,
        "ctxpop" => h::VIML_FN_CTXPOP,
        "ctxpush" => h::VIML_FN_CTXPUSH,
        "ctxset" => h::VIML_FN_CTXSET,
        "ctxsize" => h::VIML_FN_CTXSIZE,
        "islocked" => h::VIML_FN_ISLOCKED,
        "last_buffer_nr" => h::VIML_FN_LAST_BUFFER_NR,
        "libcall" => h::VIML_FN_LIBCALL,
        "libcallnr" => h::VIML_FN_LIBCALLNR,
        "msgpackdump" => h::VIML_FN_MSGPACKDUMP,
        "msgpackparse" => h::VIML_FN_MSGPACKPARSE,
        "rpcnotify" => h::VIML_FN_RPCNOTIFY,
        "rpcrequest" => h::VIML_FN_RPCREQUEST,
        "rpcstart" => h::VIML_FN_RPCSTART,
        "rpcstop" => h::VIML_FN_RPCSTOP,
        "stdioopen" => h::VIML_FN_STDIOOPEN,
        "prompt_appendbuf" => h::VIML_FN_PROMPT_APPENDBUF,
        "py3eval" => h::VIML_FN_PY3EVAL,
        "perleval" => h::VIML_FN_PERLEVAL,
        "stdpath" => h::VIML_FN_STDPATH,
        "keytrans" => h::VIML_FN_KEYTRANS,
        "luaeval" => h::VIML_FN_LUAEVAL,
        "rubyeval" => h::VIML_FN_RUBYEVAL,
        "termopen" => h::VIML_FN_TERMOPEN,
        "browse" => h::VIML_FN_BROWSE,
        "browsedir" => h::VIML_FN_BROWSEDIR,
        "finddir" => h::VIML_FN_FINDDIR,
        "findfile" => h::VIML_FN_FINDFILE,
        "flattennew" => h::VIML_FN_FLATTENNEW,
        "sha256" => h::VIML_FN_SHA256,
        "blob2list" => h::VIML_FN_BLOB2LIST,
        "list2blob" => h::VIML_FN_LIST2BLOB,
        "sqrt" => h::VIML_FN_SQRT,
        "floor" => h::VIML_FN_FLOOR,
        "ceil" => h::VIML_FN_CEIL,
        "round" => h::VIML_FN_ROUND,
        "trunc" => h::VIML_FN_TRUNC,
        "log" => h::VIML_FN_LOG,
        "exp" => h::VIML_FN_EXP,
        "sin" => h::VIML_FN_SIN,
        "cos" => h::VIML_FN_COS,
        "pow" => h::VIML_FN_POW,
        "and" => h::VIML_FN_AND,
        "or" => h::VIML_FN_OR,
        "xor" => h::VIML_FN_XOR,
        "invert" => h::VIML_FN_INVERT,
        "strchars" => h::VIML_FN_STRCHARS,
        "strpart" => h::VIML_FN_STRPART,
        "stridx" => h::VIML_FN_STRIDX,
        "trim" => h::VIML_FN_TRIM,
        "insert" => h::VIML_FN_INSERT,
        "remove" => h::VIML_FN_REMOVE,
        "extend" => h::VIML_FN_EXTEND,
        "copy" => h::VIML_FN_COPY,
        "items" => h::VIML_FN_ITEMS,
        "uniq" => h::VIML_FN_UNIQ,
        "matchstr" => h::VIML_FN_MATCHSTR,
        "match" => h::VIML_FN_MATCH,
        "substitute" => h::VIML_FN_SUBSTITUTE,
        "matchlist" => h::VIML_FN_MATCHLIST,
        "matchend" => h::VIML_FN_MATCHEND,
        "strridx" => h::VIML_FN_STRRIDX,
        "escape" => h::VIML_FN_ESCAPE,
        "tr" => h::VIML_FN_TR,
        "str2list" => h::VIML_FN_STR2LIST,
        "list2str" => h::VIML_FN_LIST2STR,
        "flatten" => h::VIML_FN_FLATTEN,
        "reduce" => h::VIML_FN_REDUCE,
        "eval" => h::VIML_FN_EVAL,
        "execute" => h::VIML_FN_EXECUTE,
        "deepcopy" => h::VIML_FN_DEEPCOPY,
        "fmod" => h::VIML_FN_FMOD,
        "atan2" => h::VIML_FN_ATAN2,
        "tan" => h::VIML_FN_TAN,
        "atan" => h::VIML_FN_ATAN,
        "asin" => h::VIML_FN_ASIN,
        "acos" => h::VIML_FN_ACOS,
        "sinh" => h::VIML_FN_SINH,
        "cosh" => h::VIML_FN_COSH,
        "tanh" => h::VIML_FN_TANH,
        "log10" => h::VIML_FN_LOG10,
        "matchfuzzy" => h::VIML_FN_MATCHFUZZY,
        "matchfuzzypos" => h::VIML_FN_MATCHFUZZYPOS,
        "histadd" => h::VIML_FN_HISTADD,
        "histget" => h::VIML_FN_HISTGET,
        "histnr" => h::VIML_FN_HISTNR,
        "histdel" => h::VIML_FN_HISTDEL,
        "digraph_get" => h::VIML_FN_DIGRAPH_GET,
        "digraph_set" => h::VIML_FN_DIGRAPH_SET,
        "digraph_getlist" => h::VIML_FN_DIGRAPH_GETLIST,
        "digraph_setlist" => h::VIML_FN_DIGRAPH_SETLIST,
        "setcellwidths" => h::VIML_FN_SETCELLWIDTHS,
        "getcellwidths" => h::VIML_FN_GETCELLWIDTHS,
        "hostname" => h::VIML_FN_HOSTNAME,
        "iconv" => h::VIML_FN_ICONV,
        "argc" => h::VIML_FN_ARGC,
        "argidx" => h::VIML_FN_ARGIDX,
        "argv" => h::VIML_FN_ARGV,
        "assert_equalfile" => h::VIML_FN_ASSERT_EQUALFILE,
        "arglistid" => h::VIML_FN_ARGLISTID,
        "foldlevel" => h::VIML_FN_FOLDLEVEL,
        "matchadd" => h::VIML_FN_MATCHADD,
        "matchaddpos" => h::VIML_FN_MATCHADDPOS,
        "matchdelete" => h::VIML_FN_MATCHDELETE,
        "getmatches" => h::VIML_FN_GETMATCHES,
        "setmatches" => h::VIML_FN_SETMATCHES,
        "clearmatches" => h::VIML_FN_CLEARMATCHES,
        "matcharg" => h::VIML_FN_MATCHARG,
        "sign_define" => h::VIML_FN_SIGN_DEFINE,
        "sign_getdefined" => h::VIML_FN_SIGN_GETDEFINED,
        "sign_undefine" => h::VIML_FN_SIGN_UNDEFINE,
        "foldclosed" => h::VIML_FN_FOLDCLOSED,
        "foldclosedend" => h::VIML_FN_FOLDCLOSEDEND,
        "hasmapto" => h::VIML_FN_HASMAPTO,
        "maparg" => h::VIML_FN_MAPARG,
        "mapcheck" => h::VIML_FN_MAPCHECK,
        "maplist" => h::VIML_FN_MAPLIST,
        // c: deprecated.c aliases — the same EvalFuncDef as their modern name.
        "highlightID" => h::VIML_FN_HLID,
        "buffer_exists" => h::VIML_FN_BUFEXISTS,
        "buffer_name" => h::VIML_FN_BUFNAME,
        "buffer_number" => h::VIML_FN_BUFNR,
        "file_readable" => h::VIML_FN_FILEREADABLE,
        "setcmdline" => h::VIML_FN_SETCMDLINE,
        "getcmdline" => h::VIML_FN_GETCMDLINE,
        "setcmdpos" => h::VIML_FN_SETCMDPOS,
        "getcmdpos" => h::VIML_FN_GETCMDPOS,
        "getcmdtype" => h::VIML_FN_GETCMDTYPE,
        "sign_place" => h::VIML_FN_SIGN_PLACE,
        "sign_getplaced" => h::VIML_FN_SIGN_GETPLACED,
        "sign_unplace" => h::VIML_FN_SIGN_UNPLACE,
        "sign_placelist" => h::VIML_FN_SIGN_PLACELIST,
        "sign_unplacelist" => h::VIML_FN_SIGN_UNPLACELIST,
        "sign_jump" => h::VIML_FN_SIGN_JUMP,
        "indent" => h::VIML_FN_INDENT,
        "foldtext" => h::VIML_FN_FOLDTEXT,
        "foldtextresult" => h::VIML_FN_FOLDTEXTRESULT,
        "highlight_exists" => h::VIML_FN_HIGHLIGHT_EXISTS,
        "hlID" => h::VIML_FN_HLID,
        "diff_hlID" => h::VIML_FN_DIFF_HLID,
        "diff_filler" => h::VIML_FN_DIFF_FILLER,
        "virtcol2col" => h::VIML_FN_VIRTCOL2COL,
        "wildtrigger" => h::VIML_FN_WILDTRIGGER,
        "searchcount" => h::VIML_FN_SEARCHCOUNT,
        "complete_info" => h::VIML_FN_COMPLETE_INFO,
        "setqflist" => h::VIML_FN_SETQFLIST,
        "getqflist" => h::VIML_FN_GETQFLIST,
        "setloclist" => h::VIML_FN_SETLOCLIST,
        "getloclist" => h::VIML_FN_GETLOCLIST,
        "getcompletion" => h::VIML_FN_GETCOMPLETION,
        "getchar" => h::VIML_FN_GETCHAR,
        "getcharstr" => h::VIML_FN_GETCHARSTR,
        "getcharmod" => h::VIML_FN_GETCHARMOD,
        "getcmdprompt" => h::VIML_FN_GETCMDPROMPT,
        "getcmdscreenpos" => h::VIML_FN_GETCMDSCREENPOS,
        "getcmdcompltype" => h::VIML_FN_GETCMDCOMPLTYPE,
        "getcmdcomplpat" => h::VIML_FN_GETCMDCOMPLPAT,
        "cindent" => h::VIML_FN_CINDENT,
        "lispindent" => h::VIML_FN_LISPINDENT,
        "complete_add" => h::VIML_FN_COMPLETE_ADD,
        "complete_check" => h::VIML_FN_COMPLETE_CHECK,
        "cmdcomplete_info" => h::VIML_FN_CMDCOMPLETE_INFO,
        "menu_info" => h::VIML_FN_MENU_INFO,
        "test_garbagecollect_now" => h::VIML_FN_TEST_GARBAGECOLLECT_NOW,
        "test_write_list_log" => h::VIML_FN_TEST_WRITE_LIST_LOG,
        "pyeval" => h::VIML_FN_PYEVAL,
        "pyxeval" => h::VIML_FN_PYXEVAL,
        "undofile" => h::VIML_FN_UNDOFILE,
        "undotree" => h::VIML_FN_UNDOTREE,
        "getmousepos" => h::VIML_FN_GETMOUSEPOS,
        "screenpos" => h::VIML_FN_SCREENPOS,
        "getcompletiontype" => h::VIML_FN_GETCOMPLETIONTYPE,
        "mapset" => h::VIML_FN_MAPSET,
        "complete" => h::VIML_FN_COMPLETE,
        "preinserted" => h::VIML_FN_PREINSERTED,
        "getscriptinfo" => h::VIML_FN_GETSCRIPTINFO,
        "getstacktrace" => h::VIML_FN_GETSTACKTRACE,
        "fullcommand" => h::VIML_FN_FULLCOMMAND,
        "assert_beeps" => h::VIML_FN_ASSERT_BEEPS,
        "assert_nobeep" => h::VIML_FN_ASSERT_NOBEEP,
        // c: deprecated.c aliases for the channel functions.
        "jobsend" => h::VIML_FN_CHANSEND,
        "jobclose" => h::VIML_FN_CHANCLOSE,
        _ => return None,
    })
}
