//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! EXTENSION — NO `csrc/` COUNTERPART. Lowers the synthesis AST to a
//! `fusevm::Chunk`. Neovim has no bytecode compiler; this is the net-new piece
//! that makes VimL run on fusevm (the role zshrs's `compile_zsh.rs` plays for
//! zsh). Each expression compiles to a sequence leaving one value on the stack;
//! faithful VimL semantics are never inlined here — every operator routes to a
//! `VIML_*` builtin whose handler calls the canonical ports.
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use fusevm::{ChunkBuilder, Op, Value};
use serde::{Deserialize, Serialize};

use crate::fusevm_bridge as h;
use crate::viml_ast::{ArithOp, Expr, ForVars, LetTarget, Stmt, UnaryOp};
use crate::viml_lexer::{CmpOp, VimlError};

/// A compiled user function: its name, parameter names, and body chunk.
#[derive(Serialize, Deserialize, Clone)]
pub struct UserFuncDef {
    /// Function name (possibly scoped).
    pub name: String,
    /// Parameter names (without the `a:` prefix).
    pub params: Vec<String>,
    /// `function!` — replace an existing definition.
    pub bang: bool,
    /// Compiled function body.
    pub chunk: fusevm::Chunk,
}

/// A compiled program: the top-level `main` chunk plus the user functions it
/// defines. Serialized as a unit into the rkyv script cache so a cache hit
/// restores both (functions and all).
#[derive(Serialize, Deserialize)]
pub struct CompiledProgram {
    /// Top-level statements.
    pub main: fusevm::Chunk,
    /// User functions defined at the top level.
    pub funcs: Vec<UserFuncDef>,
}

/// Compile a program: top-level statements into `main`, `:function` definitions
/// into `funcs`.
pub fn compile_program(stmts: &[Stmt]) -> Result<CompiledProgram, VimlError> {
    // Exceptions are global: if anything in the program throws or `:try`s, every
    // compilation unit emits unwind checks (so a throw can propagate through a
    // function call into a caller's `:try`).
    let exc = uses_exceptions(stmts);
    let mut funcs = Vec::new();
    let mut top = Vec::new();
    for s in stmts {
        if let Stmt::Function {
            name,
            args,
            body,
            bang,
        } = s
        {
            funcs.push(UserFuncDef {
                name: name.clone(),
                params: args.clone(),
                bang: *bang,
                chunk: compile_function_body(body, exc)?,
            });
        } else {
            top.push(s.clone());
        }
    }
    let mut c = Compiler::new(false, exc);
    // Slot provably-Number top-level locals so a script-level numeric loop
    // JIT-traces too. Sound: `slot_plan` bails on function calls/dynamic and
    // drops any bare name whose `g:`-alias is referenced (a bare script-level
    // name IS `g:name`). Disabled when exceptions add per-statement unwinds.
    if !exc {
        (c.slots, c.int_slots) = slot_plan(&top, false);
    }
    c.unwind.push(Vec::new());
    c.compile_stmts(&top)?;
    let frame = c.unwind.pop().expect("top unwind frame");
    let report = c.b.current_pos();
    for j in frame {
        c.b.patch_jump(j, report);
    }
    if exc {
        // Any exception that reached the top uncaught is reported here.
        c.emit(Op::CallBuiltin(h::VIML_REPORT_UNCAUGHT, 0));
        c.emit(Op::Pop);
    }
    Ok(CompiledProgram {
        main: c.b.build(),
        funcs,
    })
}

/// Compile a user function body to its own chunk. `:return` jumps to the end;
/// with no explicit return the caller defaults the result to `0`. A pending
/// exception unwinds to the same end (the call returns with it still pending).
fn compile_function_body(body: &[Stmt], exc: bool) -> Result<fusevm::Chunk, VimlError> {
    let mut c = Compiler::new(true, exc);
    // Slot-allocate provably-Number locals so a numeric loop body lowers to
    // native ops the JIT can trace. (Exceptions add per-statement unwind
    // CallBuiltins that would break a native loop, so only when `!exc`.)
    if !exc {
        (c.slots, c.int_slots) = slot_plan(body, true);
    }
    c.unwind.push(Vec::new());
    c.compile_stmts(body)?;
    let frame = c.unwind.pop().expect("fn unwind frame");
    let end = c.b.current_pos();
    for j in std::mem::take(&mut c.returns) {
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

/// Whether any statement (recursively) uses `:try` or `:throw`.
fn uses_exceptions(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s {
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

/// Debug build: emit a `SET_LINENO` marker (source line → the DAP `check_line`
/// hook) before each statement so the debugger can pause at breakpoints. Used
/// only under `--dap`; the normal `compile_program` carries no markers.
pub fn compile_program_debug(stmts: &[(u32, Stmt)]) -> Result<fusevm::Chunk, VimlError> {
    // Debug (DAP) chunks don't carry exception-unwind checks; `:try` stepping
    // is a later refinement.
    let mut c = Compiler::new(false, false);
    for (line, s) in stmts {
        // `:function` defs carry no top-level bytecode; their markers/bodies are
        // compiled separately, so skip them in the debug main chunk.
        if matches!(s, Stmt::Function { .. }) {
            continue;
        }
        c.emit(Op::LoadInt(*line as i64));
        c.emit(Op::CallBuiltin(h::VIML_SET_LINENO, 1));
        c.emit(Op::Pop);
        c.stmt(s)?;
    }
    Ok(c.b.build())
}

struct Compiler {
    b: ChunkBuilder,
    /// Stack of enclosing loops; `break`/`continue` record jump sites here.
    loops: Vec<LoopCtx>,
    /// Counter for unique hidden `:for` iterator/index variable names.
    hidden: u32,
    /// Whether we are compiling inside a function body (`:return` is valid).
    in_function: bool,
    /// `:return` jump sites in a function body, patched to the body end.
    returns: Vec<usize>,
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

fn slot_plan(stmts: &[Stmt], in_function: bool) -> SlotPlan {
    use std::collections::{HashMap, HashSet};

    fn is_bare(name: &str) -> bool {
        !name.is_empty()
            && !name.contains(':')
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    }

    // The function-local slot key for a name, or None if it lives in another
    // scope. In a function, `l:name` IS bare `name` (legacy VimL has no closures),
    // so both share a slot; every other prefix (`g:`/`s:`/`a:`/`b:`/`w:`/`t:`/
    // `v:`) is a distinct dict-backed store and can't be slotted.
    fn slot_key(name: &str, in_function: bool) -> Option<&str> {
        if is_bare(name) {
            Some(name)
        } else if in_function {
            name.strip_prefix("l:").filter(|r| is_bare(r))
        } else {
            None
        }
    }

    // Builtins that look a variable up BY NAME — they can observe even an `l:`
    // slot, so a chunk that calls one must not slot.
    fn introspects(name: &str) -> bool {
        matches!(name, "exists" | "eval" | "execute" | "call")
    }

    struct Ctx<'a> {
        bail: &'a mut bool,
        assigns: &'a mut HashMap<String, Vec<Expr>>,
        disq: &'a mut HashSet<String>,
        in_function: bool,
    }

    fn walk_expr(e: &Expr, cx: &mut Ctx) {
        match e {
            // A callee runs in its own frame and cannot see this function's
            // `l:` locals (legacy VimL has no closures), so slotting survives
            // user/value-builtin calls inside a function. At SCRIPT scope a bare
            // var IS `g:`, which a callee can read — bail. Name-introspecting
            // builtins bail in either scope.
            Expr::Call { name, args } => {
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
            Expr::List(items) => items.iter().for_each(|i| walk_expr(i, cx)),
            Expr::Dict(pairs) => pairs.iter().for_each(|(k, v)| {
                walk_expr(k, cx);
                walk_expr(v, cx);
            }),
            _ => {}
        }
    }

    fn walk(stmts: &[Stmt], cx: &mut Ctx) {
        for s in stmts {
            if *cx.bail {
                return;
            }
            match s {
                Stmt::Function { .. } | Stmt::Execute(_) | Stmt::Set(_) | Stmt::Try { .. } => {
                    *cx.bail = true
                }
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
                    cx.assigns.entry(key).or_default().push(Expr::Number(0));
                    walk(body, cx);
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
                        ForVars::List(ns) => ns.iter().for_each(|n| disq_var(n)),
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
                    }
                }
                Stmt::Let { .. } => *cx.bail = true, // non-bare target: be safe
                Stmt::Echo(es) | Stmt::Echon(es) => es.iter().for_each(|e| walk_expr(e, cx)),
                Stmt::Call(e) | Stmt::Expr(e) | Stmt::Throw(e) => walk_expr(e, cx),
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
        },
    );
    if bail || assigns.is_empty() {
        return (HashMap::new(), HashSet::new());
    }

    // A tree is a Number (`is_int=false`) / an Integer (`is_int=true`) when every
    // leaf is a matching literal or a (still-candidate) slot var of that kind.
    // `+ - * / %` of Numbers are Numbers; only `/`,`%` and Float leaves break
    // integer-ness. Concat is a string op — never numeric.
    fn rhs_kind(e: &Expr, set: &HashSet<String>, is_int: bool, in_function: bool) -> bool {
        match e {
            Expr::Number(_) => true,
            Expr::Float(_) => !is_int,
            Expr::Var(n) => slot_key(n, in_function).is_some_and(|k| set.contains(k)),
            Expr::Arith { op, lhs, rhs } => {
                !matches!(op, ArithOp::Concat)
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
            Expr::Call { name, args } if bitwise_native_op(name, args.len()).is_some() => {
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
            _ => {}
        }
    }
    fn scoped_s(stmts: &[Stmt], in_function: bool, out: &mut HashSet<String>) {
        for s in stmts {
            match s {
                Stmt::Let {
                    target: LetTarget::Var(n),
                    expr,
                } => {
                    scoped_var(n, in_function, out);
                    scoped_e(expr, in_function, out);
                }
                Stmt::Echo(es) | Stmt::Echon(es) => {
                    es.iter().for_each(|e| scoped_e(e, in_function, out))
                }
                Stmt::Call(e) | Stmt::Expr(e) | Stmt::Throw(e) => scoped_e(e, in_function, out),
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
            hidden: 0,
            in_function,
            returns: Vec::new(),
            exc,
            unwind: Vec::new(),
            slots: std::collections::HashMap::new(),
            int_slots: std::collections::HashSet::new(),
        }
    }

    /// Compile a statement sequence, emitting an unwind check after each
    /// statement when exceptions are in play (so a pending exception jumps to
    /// the innermost boundary).
    fn compile_stmts(&mut self, stmts: &[Stmt]) -> Result<(), VimlError> {
        for s in stmts {
            self.stmt(s)?;
            if self.exc {
                self.emit(Op::CallBuiltin(h::VIML_CHECK_EXC, 0));
                let j = self.emit(Op::JumpIfTrue(0));
                if let Some(frame) = self.unwind.last_mut() {
                    frame.push(j);
                }
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

const LINE: u32 = 1;

impl Compiler {
    fn emit(&mut self, op: Op) -> usize {
        self.b.emit(op, LINE)
    }

    fn load_str(&mut self, s: &str) {
        let idx = self.b.add_constant(Value::str(s));
        self.emit(Op::LoadConst(idx));
    }

    fn argc(n: usize) -> Result<u8, VimlError> {
        u8::try_from(n).map_err(|_| VimlError::msg("E118: Too many arguments (Phase 3 limit 255)"))
    }

    fn stmt(&mut self, s: &Stmt) -> Result<(), VimlError> {
        match s {
            Stmt::Echo(args) => self.echo(args, h::VIML_ECHO),
            Stmt::Echon(args) => self.echo(args, h::VIML_ECHON),
            Stmt::Let { target, expr } => self.let_stmt(target, expr),
            Stmt::Call(e) => {
                self.expr(e)?;
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::Expr(e) => {
                self.expr(e)?;
                self.emit(Op::CallBuiltin(h::VIML_SET_RESULT, 1));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::If { arms, else_body } => self.if_stmt(arms, else_body),
            Stmt::While { cond, body } => self.while_stmt(cond, body),
            Stmt::For { vars, iter, body } => self.for_stmt(vars, iter, body),
            Stmt::Execute(args) => {
                for a in args {
                    self.expr(a)?;
                }
                self.emit(Op::CallBuiltin(h::VIML_EXEC_STMT, Self::argc(args.len())?));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::Set(args) => {
                self.load_str(args);
                self.emit(Op::CallBuiltin(h::VIML_SET, 1));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::Break => {
                let j = self.emit(Op::Jump(0));
                self.loops
                    .last_mut()
                    .ok_or_else(|| VimlError::msg("E587: :break without :while or :for"))?
                    .breaks
                    .push(j);
                Ok(())
            }
            Stmt::Continue => {
                let j = self.emit(Op::Jump(0));
                self.loops
                    .last_mut()
                    .ok_or_else(|| VimlError::msg("E586: :continue without :while or :for"))?
                    .continues
                    .push(j);
                Ok(())
            }
            Stmt::Return(expr) => {
                if !self.in_function {
                    return Err(VimlError::msg("E133: :return not inside a function"));
                }
                match expr {
                    Some(e) => self.expr(e)?,
                    None => {
                        self.emit(Op::LoadInt(0)); // `:return` with no expr → 0
                    }
                }
                self.emit(Op::CallBuiltin(h::VIML_SET_RETURN, 1));
                self.emit(Op::Pop);
                let j = self.emit(Op::Jump(0));
                self.returns.push(j);
                Ok(())
            }
            Stmt::Function { .. } => Err(VimlError::msg(
                "E120: nested :function is not supported (define at script level)",
            )),
            Stmt::Throw(e) => {
                self.expr(e)?;
                self.emit(Op::CallBuiltin(h::VIML_THROW, 1));
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::Try {
                body,
                catches,
                finally,
            } => self.try_stmt(body, catches, finally),
        }
    }

    /// `:try … :catch … :finally … :endtry`. The protected body's unwind checks
    /// jump to the catch dispatch; matched catches clear the pending exception;
    /// the finally body always runs; any still-pending exception propagates to
    /// the enclosing boundary.
    fn try_stmt(
        &mut self,
        body: &[Stmt],
        catches: &[(Option<String>, Vec<Stmt>)],
        finally: &Option<Vec<Stmt>>,
    ) -> Result<(), VimlError> {
        // Protected body — its unwind frame targets the catch dispatch.
        self.unwind.push(Vec::new());
        self.compile_stmts(body)?;
        let body_frame = self.unwind.pop().expect("try body frame");
        let j_normal = self.emit(Op::Jump(0)); // normal completion → finally

        let catch_dispatch = self.b.current_pos();
        for j in body_frame {
            self.b.patch_jump(j, catch_dispatch);
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

        let finally_start = self.b.current_pos();
        if let Some(j) = prev_no_match {
            self.b.patch_jump(j, finally_start); // no catch matched → finally
        }
        for j in to_finally {
            self.b.patch_jump(j, finally_start);
        }
        if let Some(fbody) = finally {
            self.compile_stmts(fbody)?;
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
        Ok(())
    }

    /// `:if`/`:elseif`/`:else`/`:endif` — a chain of `cond → body` arms.
    fn if_stmt(
        &mut self,
        arms: &[(Expr, Vec<Stmt>)],
        else_body: &Option<Vec<Stmt>>,
    ) -> Result<(), VimlError> {
        let mut end_jumps = Vec::new();
        for (cond, body) in arms {
            self.cond(cond)?;
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

    /// `:while {cond} … :endwhile`.
    fn while_stmt(&mut self, cond: &Expr, body: &[Stmt]) -> Result<(), VimlError> {
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
        self.cond(cond)?;
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
        if let Expr::Call { name, args } = iter {
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
        body: &[Stmt],
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

    fn for_stmt(&mut self, vars: &ForVars, iter: &Expr, body: &[Stmt]) -> Result<(), VimlError> {
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
        self.expr(iter)?;
        self.set_var(&list_var);
        self.emit(Op::LoadInt(0));
        self.set_var(&idx_var);

        let l_cond = self.b.current_pos();
        // if !(idx < len(list)) jump end
        self.get_var(&idx_var);
        self.get_var(&list_var);
        self.emit(Op::CallBuiltin(h::VIML_FN_LEN, 1));
        self.emit(Op::CallBuiltin(h::cmp_id(CmpOp::Less, false), 2));
        self.emit(Op::CallBuiltin(h::VIML_TRUTHY, 1));
        let jf = self.emit(Op::JumpIfFalse(0));

        // item = list[idx]; bind it to the loop variable(s).
        self.get_var(&list_var);
        self.get_var(&idx_var);
        self.emit(Op::CallBuiltin(h::VIML_INDEX, 2));
        match vars {
            ForVars::One(name) => self.set_var(name),
            ForVars::List(names) => {
                // Unpack each item (itself a list) into the names.
                self.set_var(&item_var);
                for (i, name) in names.iter().enumerate() {
                    self.get_var(&item_var);
                    self.emit(Op::LoadInt(i as i64));
                    self.emit(Op::CallBuiltin(h::VIML_INDEX, 2));
                    self.set_var(name);
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
        for j in ctx.breaks {
            self.b.patch_jump(j, l_end);
        }
        for j in ctx.continues {
            self.b.patch_jump(j, l_incr);
        }
        Ok(())
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

    fn echo(&mut self, args: &[Expr], id: u16) -> Result<(), VimlError> {
        for a in args {
            self.expr(a)?;
        }
        let n = Self::argc(args.len())?;
        self.emit(Op::CallBuiltin(id, n));
        self.emit(Op::Pop);
        Ok(())
    }

    fn let_stmt(&mut self, target: &LetTarget, expr: &Expr) -> Result<(), VimlError> {
        match target {
            LetTarget::Var(name) => {
                self.expr(expr)?;
                self.set_var(name);
                Ok(())
            }
            LetTarget::Env(name) => {
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
                Ok(())
            }
            LetTarget::Index { base, index } => {
                // `let base[index] = value` — push value, base, index; the bridge
                // sets base[index] = value (and fires Dict watchers). `base` is an
                // expression, so nested `d['a']['b']` resolves the inner container
                // (a shared Rc, so the mutation propagates).
                self.expr(expr)?;
                self.expr(base)?;
                self.expr(index)?;
                self.emit(Op::CallBuiltin(h::VIML_SETINDEX, 3));
                self.emit(Op::Pop);
                Ok(())
            }
            LetTarget::Range { base, idx1, idx2 } => {
                // `let base[idx1:idx2] = list` — push the source list, base, idx1
                // (default 0), idx2 (Undef → "to the end"); the bridge assigns
                // the range in place via tv_list_assign_range.
                self.expr(expr)?;
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
                self.emit(Op::CallBuiltin(h::VIML_SETRANGE, 4));
                self.emit(Op::Pop);
                Ok(())
            }
            LetTarget::Option(_) | LetTarget::Register(_) => Err(VimlError::msg(
                "E15: :let on options/registers arrives with the option-table port",
            )),
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
            Expr::Arith { op, lhs, rhs } => {
                !matches!(op, ArithOp::Concat) && self.expr_is_num(lhs) && self.expr_is_num(rhs)
            }
            Expr::Unary {
                op: UnaryOp::Neg | UnaryOp::Plus,
                expr,
            } => self.expr_is_num(expr),
            // Bitwise builtins of integer args yield an Integer (so also a Number).
            Expr::Call { name, args } if bitwise_native_op(name, args.len()).is_some() => {
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
            Expr::Arith { op, lhs, rhs } => {
                !matches!(op, ArithOp::Concat) && self.expr_is_int(lhs) && self.expr_is_int(rhs)
            }
            Expr::Unary {
                op: UnaryOp::Neg | UnaryOp::Plus,
                expr,
            } => self.expr_is_int(expr),
            // Bitwise builtins yield an Integer when every argument is an Integer.
            Expr::Call { name, args } if bitwise_native_op(name, args.len()).is_some() => {
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
                for it in items {
                    self.expr(it)?;
                }
                let n = Self::argc(items.len())?;
                self.emit(Op::CallBuiltin(h::VIML_MAKE_LIST, n));
            }
            Expr::Dict(pairs) => {
                for (k, v) in pairs {
                    self.expr(k)?;
                    self.expr(v)?;
                }
                let n = Self::argc(pairs.len() * 2)?;
                self.emit(Op::CallBuiltin(h::VIML_MAKE_DICT, n));
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
            Expr::Arith { op, lhs, rhs } => {
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
                self.expr(lhs)?;
                self.expr(rhs)?;
                let id = match op {
                    ArithOp::Add => h::VIML_ADD,
                    ArithOp::Sub => h::VIML_SUB,
                    ArithOp::Mul => h::VIML_MUL,
                    ArithOp::Div => h::VIML_DIV,
                    ArithOp::Mod => h::VIML_MOD,
                    ArithOp::Concat => h::VIML_CONCAT,
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
                self.emit(Op::CallBuiltin(h::cmp_id(*op, h::ic_flag(*case)), 2));
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
                // `base.key` Dict member read — identical to `base['key']`.
                self.expr(base)?;
                self.load_str(key);
                self.emit(Op::CallBuiltin(h::VIML_INDEX, 2));
            }
            Expr::Call { name, args } => {
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
                match builtin_fn_id(name) {
                    Some(id) => {
                        for a in args {
                            self.expr(a)?;
                        }
                        self.emit(Op::CallBuiltin(id, Self::argc(args.len())?));
                    }
                    // Unknown name → user-defined function call (resolved by name
                    // at runtime). Stack: [name, arg0, …, argN].
                    None => {
                        self.load_str(name);
                        for a in args {
                            self.expr(a)?;
                        }
                        self.emit(Op::CallBuiltin(h::VIML_CALL_USER, Self::argc(args.len())?));
                        self.emit_call_unwind_check();
                    }
                }
            }
            Expr::Method { base, name, args } => match builtin_fn_id(name) {
                Some(id) => {
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
            },
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
        self.emit(Op::CallBuiltin(h::VIML_TRUTHY, 1));
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

fn builtin_fn_id(name: &str) -> Option<u16> {
    Some(match name {
        "len" => h::VIML_FN_LEN,
        "type" => h::VIML_FN_TYPE,
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
        _ => return None,
    })
}
