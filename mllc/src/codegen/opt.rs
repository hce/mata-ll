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

use super::lua::{Block, Expr, FuncBody, Item, Stmt};

/// Run all passes over the module body.
pub(super) fn run(stmts: &mut Vec<Stmt>) {
    normalize_parens_block(stmts);
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
