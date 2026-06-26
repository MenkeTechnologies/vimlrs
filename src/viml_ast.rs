//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! EXTENSION — NO `csrc/` COUNTERPART. Neovim's `eval.c` parses and evaluates
//! in one pass over the source string; there is no AST. This tree is net-new,
//! its shape dictated by the `eval1`…`eval7` precedence ladder so the compiler
//! can lower it to fusevm bytecode.
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use crate::viml_lexer::{CaseFlag, CmpOp};

/// A Vimscript expression node.
#[derive(Debug, Clone)]
pub enum Expr {
    /// Integer literal.
    Number(i64),
    /// Float literal.
    Float(f64),
    /// String literal (already unescaped).
    Str(String),
    /// List literal `[a, b, …]`.
    List(Vec<Expr>),
    /// Dict literal `{k: v, …}`.
    Dict(Vec<(Expr, Expr)>),
    /// Variable reference (possibly scoped).
    Var(String),
    /// Option reference `&name`.
    Option(String),
    /// Environment variable `$NAME`.
    Env(String),
    /// Register `@x`.
    Register(char),

    /// Unary leader: `!`, `-`, `+` (`eval7_leader`).
    Unary {
        /// Operator.
        op: UnaryOp,
        /// Operand.
        expr: Box<Expr>,
    },
    /// Arithmetic / concatenation (`eval5`/`eval6`).
    Arith {
        /// Operator.
        op: ArithOp,
        /// Left operand.
        lhs: Box<Expr>,
        /// Right operand.
        rhs: Box<Expr>,
    },
    /// Comparison (`eval4`) — carries the case flag and `is`/`isnot`.
    Compare {
        /// Relational operator.
        op: CmpOp,
        /// Case-sensitivity suffix.
        case: CaseFlag,
        /// Left operand.
        lhs: Box<Expr>,
        /// Right operand.
        rhs: Box<Expr>,
    },
    /// Logical AND `&&` (`eval3`) — short-circuits, yields 0/1.
    And(Box<Expr>, Box<Expr>),
    /// Logical OR `||` (`eval2`) — short-circuits, yields 0/1.
    Or(Box<Expr>, Box<Expr>),
    /// Ternary `cond ? a : b` (`eval1`).
    Ternary {
        /// Condition.
        cond: Box<Expr>,
        /// Truthy value.
        then: Box<Expr>,
        /// Falsy value.
        otherwise: Box<Expr>,
    },
    /// Falsy-coalesce `lhs ?? rhs` (`eval1`).
    Coalesce(Box<Expr>, Box<Expr>),

    /// Subscript `base[index]`.
    Index {
        /// Indexed value.
        base: Box<Expr>,
        /// Index expression.
        index: Box<Expr>,
    },
    /// Slice `base[from:to]`.
    Slice {
        /// Sliced value.
        base: Box<Expr>,
        /// Lower bound, or None.
        from: Option<Box<Expr>>,
        /// Upper bound, or None.
        to: Option<Box<Expr>>,
    },
    /// Dict member `base.key`.
    Member {
        /// Dict value.
        base: Box<Expr>,
        /// Literal key.
        key: String,
    },
    /// Function call `name(args)`.
    Call {
        /// Function name.
        name: String,
        /// Argument expressions.
        args: Vec<Expr>,
    },
    /// Method call `base->name(args)`.
    Method {
        /// Receiver (first argument).
        base: Box<Expr>,
        /// Method name.
        name: String,
        /// Remaining arguments.
        args: Vec<Expr>,
    },
}

/// Unary leader operators (`eval7_leader`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// `-` numeric negation.
    Neg,
    /// `+` numeric coercion.
    Plus,
    /// `!` logical not.
    Not,
}

/// Arithmetic and concatenation operators (`eval5`/`eval6`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Mod,
    /// `.` / `..`
    Concat,
}

/// Assignment target for `:let`.
#[derive(Debug, Clone)]
pub enum LetTarget {
    /// `let x = …` / `let g:x = …`.
    Var(String),
    /// `let &opt = …`.
    Option(String),
    /// `let $ENV = …`.
    Env(String),
    /// `let @x = …`.
    Register(char),
    /// `let base[index] = …` / `let base.key = …` — index/member assignment.
    /// `base` is the container expression (so nesting like `d['a']['b']` works).
    Index {
        /// The container expression.
        base: Box<Expr>,
        /// The index/key expression.
        index: Box<Expr>,
    },
    /// `let [a, b] = list` / `let [a, b; rest] = list` — list-unpack.
    List {
        /// Leading target names.
        names: Vec<String>,
        /// Trailing `; rest` name, if present (gets the remaining items).
        rest: Option<String>,
    },
    /// `let base[idx1:idx2] = list` — list range assignment. Omitted `idx1`
    /// defaults to 0; omitted `idx2` means "to the end".
    Range {
        /// The container expression.
        base: Box<Expr>,
        /// The first index (`None` → from the start).
        idx1: Option<Box<Expr>>,
        /// The last index (`None` → to the end).
        idx2: Option<Box<Expr>>,
    },
}

/// `:for` loop variable: a single name, or a `[a, b]` unpack of each item.
#[derive(Debug, Clone)]
pub enum ForVars {
    /// `:for x in …`.
    One(String),
    /// `:for [a, b] in …` — each item is unpacked into these names.
    List(Vec<String>),
}

/// A Vimscript statement (one ex-command's worth of work).
#[derive(Debug, Clone)]
pub enum Stmt {
    /// `:echo expr …`.
    Echo(Vec<Expr>),
    /// `:echon expr …`.
    Echon(Vec<Expr>),
    /// `:let target = expr`.
    Let {
        /// Assignment target.
        target: LetTarget,
        /// Value expression.
        expr: Expr,
    },
    /// `:call funcref(args)`.
    Call(Expr),
    /// A bare expression (REPL / `-e`).
    Expr(Expr),

    /// `:if … :elseif … :else … :endif`. Each arm is `(condition, body)`; the
    /// optional trailing `else` body has no condition.
    If {
        /// `if` / `elseif` arms in source order.
        arms: Vec<(Expr, Vec<Stmt>)>,
        /// `else` body, if present.
        else_body: Option<Vec<Stmt>>,
    },
    /// `:while {cond} … :endwhile`.
    While {
        /// Loop condition.
        cond: Expr,
        /// Loop body.
        body: Vec<Stmt>,
    },
    /// `:for {var} in {expr} … :endfor` (list iteration).
    For {
        /// Loop variable(s) — a single name or a `[a, b]` unpack.
        vars: ForVars,
        /// Iterable expression (a List in Phase 3 of this port).
        iter: Expr,
        /// Loop body.
        body: Vec<Stmt>,
    },
    /// `:break`.
    Break,
    /// `:continue`.
    Continue,
    /// `:return [expr]`.
    Return(Option<Expr>),
    /// `:function {name}(args) … :endfunction`.
    Function {
        /// Function name (may be scoped / `s:` / autoload).
        name: String,
        /// Parameter names (without the `a:` prefix).
        args: Vec<String>,
        /// Function body.
        body: Vec<Stmt>,
        /// `function!` — replace an existing definition.
        bang: bool,
    },
    /// `:try … :catch {pat} … :finally … :endtry`.
    Try {
        /// Protected body.
        body: Vec<Stmt>,
        /// `catch` clauses: `(optional /pattern/, body)`.
        catches: Vec<(Option<String>, Vec<Stmt>)>,
        /// `finally` body, always run.
        finally: Option<Vec<Stmt>>,
    },
    /// `:throw {expr}`.
    Throw(Expr),
    /// `:execute expr …` — concatenate the values (space-separated) and run the
    /// result as an ex command line.
    Execute(Vec<Expr>),
    /// `:set {args}` — set options (the raw argument text).
    Set(String),
}
