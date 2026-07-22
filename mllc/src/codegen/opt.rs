//! AST optimization passes over the finished statement list.
//!
//! `run` is the single entry point, called by `generate` (mod.rs) on the
//! whole module body after `module_stmts` and before printing — so
//! `ondemand_prelude`, which scans the printed body, automatically shrinks
//! the prelude for anything a pass deletes. Every pass recurses into
//! `Stmt::Function` and `FuncBody` bodies.
//!
//! Raw policy: `Expr::Raw` / `Stmt::Raw` are opaque atoms. No pass rewrites
//! inside them; passes that only rearrange structure move them verbatim,
//! and a paren around a Raw child is never dropped (the text could be a
//! multi-return host call whose truncation the paren enforces).
//!
//! Pass 1 — paren normalization. Grouping in the emitted tree is explicit
//! (`Paren` nodes, never synthesized by the printer), so emission sites
//! wrap defensively and the result carries redundant parens. This pass
//! drops a `Paren` exactly where the enclosing position proves it
//! redundant (see `Ctx`). The semantic payoff is in return position: Lua's
//! `return f(x)` is a proper tail call but `return (f(x))` is not — it
//! truncates to one value and pays a stack frame — so a redundant paren in
//! a thunk body turns deep lazy force chains into stack overflows. The
//! paren around a call is ONLY dropped where truncation is preserved: at
//! single-value positions (which truncate by themselves), and at
//! multi-value positions (return, last argument, last table item) only
//! when the callee provably returns one value (`single_return_callee`) —
//! an unrecognized name can be a multi-returning host function, and there
//! `(f(x))` is Lua's truncation operator.

//! Pass 2 — dead-branch and wrapper cleanup. Four rewrites, applied
//! bottom-up per block: (1) an `elseif true` arm (the `otherwise` guard)
//! becomes `else`, later arms are dead; a whole `if true` becomes a `do`
//! block (NOT a splice — its locals must stay scoped away from following
//! statements). (2) A two-arm chain whose second condition is the first
//! with one top-level ==/~= swapped becomes if/else — sound because the
//! complement evaluates exactly the subexpressions the first condition
//! already evaluated (thunk forces are memoized). (3) Statements after a
//! diverging statement (every path returns or raises) are dead and
//! dropped — this kills the non-exhaustive fall-off after an exhaustive
//! chain. A `Stmt::Raw` is never treated as diverging. (4) A `do` block
//! in final position splices into its parent: nothing follows, so its
//! locals leak nowhere observable.

use super::lua::{Block, Expr, FuncBody, Item, Stmt};

/// Run all passes over the module body.
pub(super) fn run(stmts: &mut Vec<Stmt>) {
    normalize_parens_block(stmts);
    dead_branch_block(stmts);
}

fn is_true_lit(e: &Expr) -> bool {
    matches!(e, Expr::Lit(s) if s == "true")
}

/// Render an expression to text for syntactic comparison (comparison only —
/// never used to rewrite).
fn render_str(e: &Expr) -> String {
    let mut s = String::new();
    e.render(0, &mut s);
    s
}

/// `b` is `a` with the one top-level `==`/`~=` swapped.
fn complement_conds(a: &Expr, b: &Expr) -> bool {
    let (Expr::Binop(op1, l1, r1), Expr::Binop(op2, l2, r2)) = (a, b) else {
        return false;
    };
    let swapped = (op1 == "==" && op2 == "~=") || (op1 == "~=" && op2 == "==");
    swapped && render_str(l1) == render_str(l2) && render_str(r1) == render_str(r2)
}

/// Every path through the statement ends in `return` or a raised error.
fn stmt_diverges(s: &Stmt) -> bool {
    match s {
        Stmt::Return(_) => true,
        Stmt::Expr(Expr::Call(f, _)) => matches!(f.as_ref(), Expr::Name(n) if n == "error"),
        Stmt::If { then_b, elseifs, else_b: Some(else_b), .. } => {
            block_diverges(then_b)
                && elseifs.iter().all(|(_, b)| block_diverges(b))
                && block_diverges(else_b)
        }
        Stmt::Do(b) => block_diverges(b),
        _ => false,
    }
}

fn block_diverges(b: &Block) -> bool {
    b.0.last().is_some_and(stmt_diverges)
}

fn dead_branch_block(stmts: &mut Vec<Stmt>) {
    // Bottom-up: children first, then this block's own rewrites.
    for s in stmts.iter_mut() {
        dead_branch_stmt(s);
    }
    // (3) Drop statements after the first diverging one.
    if let Some(i) = stmts.iter().position(stmt_diverges)
        && i + 1 < stmts.len()
    {
        stmts.truncate(i + 1);
    }
    // (4) Splice a final `do` block into this one.
    if matches!(stmts.last(), Some(Stmt::Do(_))) {
        let Some(Stmt::Do(Block(inner))) = stmts.pop() else { unreachable!() };
        stmts.extend(inner);
    }
}

fn dead_branch_stmt(stmt: &mut Stmt) {
    // Recurse into nested function-literal bodies inside expressions.
    fn expr_bodies(e: &mut Expr) {
        match e {
            Expr::Name(_) | Expr::Lit(_) | Expr::Raw(_) => {}
            Expr::Paren(e) | Expr::Neg(e) => expr_bodies(e),
            Expr::Call(f, args) => {
                expr_bodies(f);
                for a in args {
                    expr_bodies(a);
                }
            }
            Expr::Method(recv, _, args) => {
                expr_bodies(recv);
                for a in args {
                    expr_bodies(a);
                }
            }
            Expr::Index(base, _) => expr_bodies(base),
            Expr::Binop(_, l, r) => {
                expr_bodies(l);
                expr_bodies(r);
            }
            Expr::Table(items) | Expr::TableSpaced(items) => {
                for item in items {
                    match item {
                        Item::Pos(e) | Item::KV(_, e) => expr_bodies(e),
                    }
                }
            }
            Expr::Func(_, body) => match body {
                FuncBody::Inline(stmts) => dead_branch_block(stmts),
                FuncBody::Block(Block(stmts)) => dead_branch_block(stmts),
            },
        }
    }

    match stmt {
        Stmt::Raw(_) => {}
        Stmt::Local(_, Some(e)) | Stmt::Assign(_, e) | Stmt::Return(e) | Stmt::Expr(e) => {
            expr_bodies(e)
        }
        Stmt::Local(_, None) => {}
        Stmt::AssignIf { cond, then_e, else_e, .. } => {
            expr_bodies(cond);
            expr_bodies(then_e);
            expr_bodies(else_e);
        }
        Stmt::Do(b) => dead_branch_block(&mut b.0),
        Stmt::Function { body, .. } => dead_branch_block(&mut body.0),
        Stmt::ReturnTable(entries) => {
            for (_, e) in entries {
                expr_bodies(e);
            }
        }
        Stmt::If { cond, then_b, elseifs, else_b } => {
            expr_bodies(cond);
            dead_branch_block(&mut then_b.0);
            for (c, b) in elseifs.iter_mut() {
                expr_bodies(c);
                dead_branch_block(&mut b.0);
            }
            if let Some(b) = else_b.as_mut() {
                dead_branch_block(&mut b.0);
            }
            // (1) `elseif true` becomes `else`; later arms and the old
            // `else` are unreachable.
            if let Some(i) = elseifs.iter().position(|(c, _)| is_true_lit(c)) {
                let (_, b) = elseifs.swap_remove(i);
                elseifs.truncate(i);
                *else_b = Some(b);
            }
            // A whole `if true` keeps only its first arm, as a `do` block
            // so its locals stay scoped; a final-position `do` is spliced
            // by the parent block's rewrite (4).
            if is_true_lit(cond) {
                let then_b = std::mem::replace(then_b, Block(Vec::new()));
                *stmt = Stmt::Do(then_b);
                return;
            }
            // (2) Complement collapse: `if C … elseif ¬C …` → if/else.
            if elseifs.len() == 1 && else_b.is_none() && complement_conds(cond, &elseifs[0].0) {
                let (_, b) = elseifs.pop().expect("one elseif");
                *else_b = Some(b);
            }
        }
    }
}

/// What the position enclosing an expression slot proves about a `Paren`
/// child there.
#[derive(Clone, Copy, PartialEq)]
enum Ctx {
    /// Grouping matters (binop/neg operand, the child of a `Paren`): only a
    /// self-delimiting child may shed its parens.
    Grouped,
    /// Delimited single-value position (if/elseif condition, single-lvalue
    /// assignment RHS, non-last call argument, keyed table value): the
    /// position truncates to one value by itself, so `Binop`/`Neg` and any
    /// call may shed.
    Delim,
    /// Delimited multi-value position (return operand, last call argument,
    /// last positional table item, multi-lvalue RHS): a call spreads its
    /// values here, so a call sheds only with a single-return callee.
    DelimLast,
    /// Lua prefixexp position (index base, callee, method receiver): the
    /// child must remain a prefixexp, so only prefixexp shapes shed. A call
    /// is adjusted to one value by the grammar here, so truncation is
    /// preserved without the paren.
    Prefix,
}

/// Callees whose calls provably return exactly one value: the runtime
/// helpers (all single-return except the excluded forwarders), the show
/// family, and compiled-function slots. Everything else — host FFI names
/// in particular — may multi-return.
fn single_return_callee(f: &Expr) -> bool {
    match f {
        Expr::Name(n) => {
            n == "__force"
                || n == "__thunk"
                || (n.starts_with("__mll_")
                    // Multi-value by design (FFI result spreading) or
                    // forwarding a callee's returns verbatim.
                    && n != "__mll_opt_tail"
                    && n != "__mll_run_tail"
                    && n != "__mll_seq")
                || n.starts_with("show")
        }
        Expr::Index(base, _) => matches!(base.as_ref(), Expr::Name(b) if b == "__mll_fn"),
        _ => false,
    }
}

/// May a `Paren` around `inner` be dropped at a `ctx` position?
fn paren_redundant(inner: &Expr, ctx: Ctx) -> bool {
    match ctx {
        Ctx::Grouped => matches!(
            inner,
            Expr::Name(_) | Expr::Lit(_) | Expr::Index(..) | Expr::Paren(_)
        ),
        Ctx::Delim => matches!(
            inner,
            Expr::Name(_)
                | Expr::Lit(_)
                | Expr::Index(..)
                | Expr::Paren(_)
                | Expr::Binop(..)
                | Expr::Neg(_)
                | Expr::Call(..)
                | Expr::Method(..)
                | Expr::Func(..)
                | Expr::Table(_)
                | Expr::TableSpaced(_)
        ),
        Ctx::DelimLast => match inner {
            Expr::Name(_)
            | Expr::Lit(_)
            | Expr::Index(..)
            | Expr::Paren(_)
            | Expr::Binop(..)
            | Expr::Neg(_)
            | Expr::Func(..)
            | Expr::Table(_)
            | Expr::TableSpaced(_) => true,
            Expr::Call(f, _) => single_return_callee(f),
            _ => false,
        },
        // A bare literal/table/function is not a prefixexp — `(5):m()`
        // needs its parens even though `5` is self-delimiting.
        Ctx::Prefix => matches!(
            inner,
            Expr::Name(_) | Expr::Index(..) | Expr::Paren(_) | Expr::Call(..) | Expr::Method(..)
        ),
    }
}

/// Shed every redundant paren layer at this slot, then normalize the
/// children of whatever remains.
fn normalize_expr(slot: &mut Expr, ctx: Ctx) {
    loop {
        let Expr::Paren(inner) = slot else { break };
        if !paren_redundant(inner, ctx) {
            break;
        }
        // Replace the slot by its unwrapped child.
        let inner = std::mem::replace(inner.as_mut(), Expr::Lit(String::new()));
        *slot = inner;
    }
    match slot {
        Expr::Name(_) | Expr::Lit(_) | Expr::Raw(_) => {}
        Expr::Paren(e) => normalize_expr(e, Ctx::Grouped),
        Expr::Call(f, args) => {
            // A thunk body's return value has exactly one consumer: the
            // `local val = x[1]()` line in `__force`, which truncates to
            // one value. Its return position is therefore single-value
            // like any other delimited slot, and a paren around ANY call
            // there may shed — this is where deep lazy force chains
            // regain proper tail calls.
            if let Expr::Name(n) = f.as_ref()
                && n == "__thunk"
                && args.len() == 1
                && let Expr::Func(_, body) = &mut args[0]
            {
                match body {
                    FuncBody::Inline(stmts) => normalize_parens_block_ret(stmts, Ctx::Delim),
                    FuncBody::Block(Block(stmts)) => normalize_parens_block_ret(stmts, Ctx::Delim),
                }
                return;
            }
            normalize_expr(f, Ctx::Prefix);
            let last = args.len().saturating_sub(1);
            for (i, a) in args.iter_mut().enumerate() {
                normalize_expr(a, if i == last { Ctx::DelimLast } else { Ctx::Delim });
            }
        }
        Expr::Method(recv, _, args) => {
            normalize_expr(recv, Ctx::Prefix);
            let last = args.len().saturating_sub(1);
            for (i, a) in args.iter_mut().enumerate() {
                normalize_expr(a, if i == last { Ctx::DelimLast } else { Ctx::Delim });
            }
        }
        Expr::Index(base, _) => normalize_expr(base, Ctx::Prefix),
        Expr::Binop(_, l, r) => {
            normalize_expr(l, Ctx::Grouped);
            normalize_expr(r, Ctx::Grouped);
        }
        Expr::Neg(e) => normalize_expr(e, Ctx::Grouped),
        Expr::Table(items) | Expr::TableSpaced(items) => normalize_items(items),
        Expr::Func(_, body) => match body {
            FuncBody::Inline(stmts) => normalize_parens_block(stmts),
            FuncBody::Block(Block(stmts)) => normalize_parens_block(stmts),
        },
    }
}

fn normalize_items(items: &mut [Item]) {
    let last = items.len().saturating_sub(1);
    for (i, item) in items.iter_mut().enumerate() {
        match item {
            // Only the LAST positional item spreads a call's values into
            // the table; every other item truncates.
            Item::Pos(e) => {
                normalize_expr(e, if i == last { Ctx::DelimLast } else { Ctx::Delim })
            }
            Item::KV(_, e) => normalize_expr(e, Ctx::Delim),
        }
    }
}

fn normalize_parens_block(stmts: &mut [Stmt]) {
    normalize_parens_block_ret(stmts, Ctx::DelimLast);
}

/// `ret_ctx` is the context of this block's `return` operands — `DelimLast`
/// for a real function body, `Delim` inside a thunk body (see the `__thunk`
/// rule in `normalize_expr`). It propagates through `If`/`Do` sub-blocks
/// (their returns belong to the same function) and resets at any nested
/// function literal or named function.
fn normalize_parens_block_ret(stmts: &mut [Stmt], ret_ctx: Ctx) {
    for s in stmts {
        normalize_parens_stmt(s, ret_ctx);
    }
}

fn normalize_parens_stmt(stmt: &mut Stmt, ret_ctx: Ctx) {
    match stmt {
        Stmt::Raw(_) => {}
        Stmt::Local(names, init) => {
            if let Some(e) = init {
                let ctx = if names.len() == 1 { Ctx::Delim } else { Ctx::DelimLast };
                normalize_expr(e, ctx);
            }
        }
        Stmt::Assign(_, e) => normalize_expr(e, Ctx::Delim),
        Stmt::Return(e) => normalize_expr(e, ret_ctx),
        // Statement position discards the value; the expression itself must
        // stay a call, which the emitter guarantees and no rule here breaks
        // (a call never sheds into a non-call).
        Stmt::Expr(e) => normalize_expr(e, Ctx::Delim),
        Stmt::If { cond, then_b, elseifs, else_b } => {
            normalize_expr(cond, Ctx::Delim);
            normalize_parens_block_ret(&mut then_b.0, ret_ctx);
            for (c, b) in elseifs {
                normalize_expr(c, Ctx::Delim);
                normalize_parens_block_ret(&mut b.0, ret_ctx);
            }
            if let Some(b) = else_b {
                normalize_parens_block_ret(&mut b.0, ret_ctx);
            }
        }
        Stmt::AssignIf { cond, then_e, else_e, .. } => {
            normalize_expr(cond, Ctx::Delim);
            normalize_expr(then_e, Ctx::Delim);
            normalize_expr(else_e, Ctx::Delim);
        }
        Stmt::Do(b) => normalize_parens_block_ret(&mut b.0, ret_ctx),
        Stmt::Function { body, .. } => normalize_parens_block(&mut body.0),
        Stmt::ReturnTable(entries) => {
            for (_, e) in entries {
                normalize_expr(e, Ctx::Delim);
            }
        }
    }
}
