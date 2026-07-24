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

//! Pass 4 — force of known-WHNF locals. A safety net under the emitter's
//! `concrete_vars` tracking: a single-assignment local whose one
//! value-producing site is WHNF by construction (a function/table literal,
//! a literal token, a cons cell, a `__force` result — including the
//! `p = __force(p)` entry rebind) never holds a thunk, so a later
//! `__force(name)` is the identity and rewrites to `name`. Closure capture
//! is safe — the local never holds a thunk regardless of when the closure
//! runs — but a name mentioned in any Raw text, bound more than once, or
//! shadowed by a nested parameter is left alone.
//!
//! Pass 3 — IIFE flattening. The expression walk emits `case`/`let`/`if`
//! in value position as immediately-invoked function literals; in return
//! position and in straight-line value bindings the closure allocation
//! (and, on LuaJIT, the trace break) buys nothing. Two shapes splice:
//! `return (function(p…) body end)(a…)` becomes `local p = a; …body` —
//! Lua guarantees `return` is last in its block, so the spliced locals
//! leak nowhere, and the body's own returns already meant "return from
//! the enclosing function". `local x = (function() … end)()` (and the
//! assignment form) splices its body prefix and turns the tail return —
//! or a tail `if` whose every arm returns or raises — into assignments
//! to `x`. Both bail on a name collision (a conservative identifier-token
//! scan over rendered text, so `Raw` content is covered) and stop when
//! the enclosing function's local count would approach Lua's 200-local
//! limit, which the emitter's own `_v` spill decided before this pass ran.

use super::lua::{Block, Expr, FuncBody, Item, Stmt};

/// Run all passes over the module body.
pub(super) fn run(stmts: &mut Vec<Stmt>) {
    normalize_parens_block(stmts);
    dead_branch_block(stmts);
    flatten_iife_block(stmts);
    force_whnf_block(stmts);
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
    while let Expr::Paren(inner) = slot {
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

// ---- Pass 3: IIFE flattening ----

/// Identifier-shaped tokens of rendered Lua text. Conservative by design:
/// rendering includes `Raw` content, so an identifier hidden in a Raw
/// fragment is seen and blocks a splice like any other.
fn token_set(text: &str, out: &mut std::collections::HashSet<String>) {
    let mut cur = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_alphanumeric() || c == '_' {
            cur.push(c);
        } else if !cur.is_empty() {
            let tok = std::mem::take(&mut cur);
            if !tok.starts_with(|c: char| c.is_ascii_digit()) {
                out.insert(tok);
            }
        }
    }
}

fn expr_tokens(e: &Expr) -> std::collections::HashSet<String> {
    let mut s = String::new();
    e.render(0, &mut s);
    let mut out = std::collections::HashSet::new();
    token_set(&s, &mut out);
    out
}

fn stmts_tokens(stmts: &[Stmt]) -> std::collections::HashSet<String> {
    let mut s = String::new();
    for st in stmts {
        st.render_line(0, &mut s);
    }
    let mut out = std::collections::HashSet::new();
    token_set(&s, &mut out);
    out
}

/// Local declarations belonging to this function scope (sub-blocks
/// included, nested function literals not — they are their own scope).
/// Branch locals are summed, not maxed, so the count over-approximates
/// "active at once" — declining a splice is always safe.
fn count_locals(stmts: &[Stmt]) -> usize {
    let mut n = 0;
    for s in stmts {
        match s {
            Stmt::Local(names, _) => n += names.len(),
            Stmt::Function { header, .. } => {
                if header.starts_with("local ") {
                    n += 1;
                }
            }
            Stmt::If { then_b, elseifs, else_b, .. } => {
                n += count_locals(&then_b.0);
                for (_, b) in elseifs {
                    n += count_locals(&b.0);
                }
                if let Some(b) = else_b {
                    n += count_locals(&b.0);
                }
            }
            Stmt::Do(b) => n += count_locals(&b.0),
            _ => {}
        }
    }
    n
}

/// Entry per function scope: budget the splices against Lua's local limit
/// (the emitter's `_v` spill was decided before this pass ran, so the pass
/// must never push a function past the limit on its own).
fn flatten_iife_block(stmts: &mut Vec<Stmt>) {
    let mut budget = super::CodeGen::LOCAL_LIMIT.saturating_sub(count_locals(stmts));
    flatten_scope(stmts, &mut budget);
}

fn flatten_scope(stmts: &mut Vec<Stmt>, budget: &mut usize) {
    let mut i = 0;
    while i < stmts.len() {
        if i == stmts.len() - 1 && try_splice_return(stmts, i, budget) {
            // Reprocess from the splice point: the spliced body may itself
            // end in a return-position IIFE.
            continue;
        }
        if try_splice_value(stmts, i, budget) {
            continue;
        }
        match &mut stmts[i] {
            Stmt::If { then_b, elseifs, else_b, .. } => {
                flatten_scope(&mut then_b.0, budget);
                for (_, b) in elseifs.iter_mut() {
                    flatten_scope(&mut b.0, budget);
                }
                if let Some(b) = else_b.as_mut() {
                    flatten_scope(&mut b.0, budget);
                }
            }
            Stmt::Do(b) => flatten_scope(&mut b.0, budget),
            Stmt::Function { body, .. } => flatten_iife_block(&mut body.0),
            _ => {}
        }
        // Function literals in this statement's expressions are their own
        // scopes (this covers thunk bodies and IIFEs that stayed IIFEs).
        stmt_expr_funcs(&mut stmts[i]);
        i += 1;
    }
}

/// Visit the statement's expressions (not its sub-blocks — those belong to
/// the enclosing scope and are handled by `flatten_scope`) and process
/// every function-literal body as a fresh scope.
fn stmt_expr_funcs(stmt: &mut Stmt) {
    fn expr(e: &mut Expr) {
        match e {
            Expr::Name(_) | Expr::Lit(_) | Expr::Raw(_) => {}
            Expr::Paren(e) | Expr::Neg(e) => expr(e),
            Expr::Call(f, args) => {
                expr(f);
                for a in args {
                    expr(a);
                }
            }
            Expr::Method(recv, _, args) => {
                expr(recv);
                for a in args {
                    expr(a);
                }
            }
            Expr::Index(base, _) => expr(base),
            Expr::Binop(_, l, r) => {
                expr(l);
                expr(r);
            }
            Expr::Table(items) | Expr::TableSpaced(items) => {
                for item in items {
                    match item {
                        Item::Pos(e) | Item::KV(_, e) => expr(e),
                    }
                }
            }
            Expr::Func(_, body) => match body {
                FuncBody::Inline(stmts) => flatten_iife_block(stmts),
                FuncBody::Block(Block(stmts)) => flatten_iife_block(stmts),
            },
        }
    }
    match stmt {
        Stmt::Raw(_) | Stmt::Local(_, None) => {}
        Stmt::Local(_, Some(e)) | Stmt::Assign(_, e) | Stmt::Return(e) | Stmt::Expr(e) => expr(e),
        Stmt::AssignIf { cond, then_e, else_e, .. } => {
            expr(cond);
            expr(then_e);
            expr(else_e);
        }
        Stmt::If { cond, elseifs, .. } => {
            expr(cond);
            for (c, _) in elseifs {
                expr(c);
            }
        }
        Stmt::Do(_) | Stmt::Function { .. } => {}
        Stmt::ReturnTable(entries) => {
            for (_, e) in entries {
                expr(e);
            }
        }
    }
}

fn iife_parts(e: &Expr) -> Option<(&Vec<String>, &FuncBody, &Vec<Expr>)> {
    let Expr::Call(f, args) = e else { return None };
    let Expr::Paren(pf) = f.as_ref() else { return None };
    let Expr::Func(params, body) = pf.as_ref() else { return None };
    if params.len() != args.len() {
        return None;
    }
    Some((params, body, args))
}

fn body_stmts(body: FuncBody) -> Vec<Stmt> {
    match body {
        FuncBody::Inline(s) => s,
        FuncBody::Block(Block(s)) => s,
    }
}

fn body_stmts_ref(body: &FuncBody) -> &Vec<Stmt> {
    match body {
        FuncBody::Inline(s) => s,
        FuncBody::Block(Block(s)) => s,
    }
}

/// Shape 1: `return (function(p…) body end)(a…)` as the block's last
/// statement becomes `local p = a; … body`. `return` is last in its block
/// by Lua's grammar, so the spliced locals leak nowhere; the body's own
/// returns already meant "return from the enclosing function", and a
/// proper-tail-call body statement stays one.
fn try_splice_return(stmts: &mut Vec<Stmt>, i: usize, budget: &mut usize) -> bool {
    {
        let Stmt::Return(e) = &stmts[i] else { return false };
        let Some((params, body, args)) = iife_parts(e) else { return false };
        // The IIFE evaluated every argument before binding any parameter;
        // the splice binds sequentially, so a later argument must not
        // mention an earlier parameter's name.
        for (k, a) in args.iter().enumerate() {
            if k > 0 {
                let toks = expr_tokens(a);
                if params[..k].iter().any(|p| toks.contains(p)) {
                    return false;
                }
            }
        }
        let cost = params.len() + count_locals(body_stmts_ref(body));
        if cost > *budget {
            return false;
        }
        *budget -= cost;
    }
    let Stmt::Return(Expr::Call(f, args)) = stmts.remove(i) else { unreachable!() };
    let Expr::Paren(pf) = *f else { unreachable!() };
    let Expr::Func(params, body) = *pf else { unreachable!() };
    let mut spliced = Vec::new();
    for (p, a) in params.into_iter().zip(args) {
        spliced.push(Stmt::Local(vec![p], Some(a)));
    }
    spliced.extend(body_stmts(body));
    stmts.splice(i..i, spliced);
    true
}

/// Would rewriting this tail statement's returns into assignments to `lhs`
/// preserve behavior? `strict` requires every path to return or raise (the
/// assignment form must not fall through and keep the lvalue's old value);
/// the fresh-local form tolerates fall-through, which leaves the same nil
/// the fallen-through IIFE returned.
fn tail_rewrite_ok(s: &Stmt, strict: bool) -> bool {
    match s {
        Stmt::Return(_) => true,
        Stmt::Expr(Expr::Call(f, _)) => matches!(f.as_ref(), Expr::Name(n) if n == "error"),
        Stmt::If { then_b, elseifs, else_b, .. } => {
            let arm = |b: &Block| match b.0.last() {
                Some(s) => tail_rewrite_ok(s, strict),
                None => !strict,
            };
            arm(then_b)
                && elseifs.iter().all(|(_, b)| arm(b))
                && match else_b {
                    Some(b) => arm(b),
                    None => !strict,
                }
        }
        _ => !strict && !matches!(s, Stmt::Raw(_)),
    }
}

fn rewrite_tail_returns(s: &mut Stmt, lhs: &str) {
    match s {
        Stmt::Return(e) => {
            let e = std::mem::replace(e, Expr::Lit(String::new()));
            *s = Stmt::Assign(lhs.to_string(), e);
        }
        Stmt::If { then_b, elseifs, else_b, .. } => {
            let arm = |b: &mut Block| {
                if let Some(last) = b.0.last_mut() {
                    rewrite_tail_returns(last, lhs);
                }
            };
            arm(then_b);
            for (_, b) in elseifs.iter_mut() {
                arm(b);
            }
            if let Some(b) = else_b.as_mut() {
                arm(b);
            }
        }
        _ => {}
    }
}

/// Shape 2: `local x = (function() body end)()` / `x = (function() … end)()`
/// where the body is straight-line (`local`/assignment/effect statements)
/// up to a tail `return e` — or a tail `if` whose arms end in returns —
/// splices the prefix and assigns the tail value(s) to `x` directly. The
/// spliced locals become visible to the REST of the enclosing block, so any
/// occurrence of their names in the following statements bails the splice.
fn try_splice_value(stmts: &mut Vec<Stmt>, i: usize, budget: &mut usize) -> bool {
    enum Target {
        Fresh(String),
        Lvalue(String),
    }
    let target;
    let body_ref;
    match &stmts[i] {
        Stmt::Local(names, Some(e)) if names.len() == 1 => {
            let Some((params, body, _)) = iife_parts(e) else { return false };
            if !params.is_empty() {
                return false;
            }
            target = Target::Fresh(names[0].clone());
            body_ref = body_stmts_ref(body);
        }
        Stmt::Assign(lhs, e) => {
            let Some((params, body, _)) = iife_parts(e) else { return false };
            if !params.is_empty() {
                return false;
            }
            target = Target::Lvalue(lhs.clone());
            body_ref = body_stmts_ref(body);
        }
        _ => return false,
    }
    let Some((tail, prefix)) = body_ref.split_last() else { return false };
    if !prefix.iter().all(|s| {
        matches!(s, Stmt::Local(_, _) | Stmt::Assign(..) | Stmt::Expr(_))
    }) {
        return false;
    }
    let strict = matches!(target, Target::Lvalue(_));
    let tail_if = match tail {
        Stmt::Return(_) => false,
        Stmt::If { .. } if tail_rewrite_ok(tail, strict) => true,
        _ => return false,
    };
    // The fresh-local If form declares `local x` BEFORE the body runs, so
    // a body that reads an outer `x` would see the new nil local: bail.
    let lhs_name = match &target {
        Target::Fresh(n) | Target::Lvalue(n) => n.clone(),
    };
    if tail_if
        && matches!(target, Target::Fresh(_))
        && stmts_tokens(body_ref).contains(&lhs_name)
    {
        return false;
    }
    // Introduced names must not appear in the rest of the enclosing block
    // (they would shadow whatever those statements meant to reference).
    let mut introduced = Vec::new();
    for s in prefix {
        if let Stmt::Local(names, _) = s {
            introduced.extend(names.iter().cloned());
        }
    }
    if !introduced.is_empty() {
        let suffix_toks = stmts_tokens(&stmts[i + 1..]);
        if introduced.iter().any(|n| suffix_toks.contains(n)) {
            return false;
        }
    }
    let cost = introduced.len();
    if cost > *budget {
        return false;
    }
    *budget -= cost;

    let removed = stmts.remove(i);
    let (e, target) = match removed {
        Stmt::Local(_, Some(e)) => (e, target),
        Stmt::Assign(_, e) => (e, target),
        _ => unreachable!(),
    };
    let Expr::Call(f, _) = e else { unreachable!() };
    let Expr::Paren(pf) = *f else { unreachable!() };
    let Expr::Func(_, body) = *pf else { unreachable!() };
    let mut body = body_stmts(body);
    let mut spliced = Vec::new();
    let mut tail = body.pop().expect("checked non-empty");
    match (&target, tail_if) {
        (Target::Fresh(n), false) => {
            spliced.extend(body);
            let Stmt::Return(v) = tail else { unreachable!() };
            spliced.push(Stmt::Local(vec![n.clone()], Some(v)));
        }
        (Target::Fresh(n), true) => {
            spliced.push(Stmt::Local(vec![n.clone()], None));
            spliced.extend(body);
            rewrite_tail_returns(&mut tail, n);
            spliced.push(tail);
        }
        (Target::Lvalue(l), false) => {
            spliced.extend(body);
            let Stmt::Return(v) = tail else { unreachable!() };
            spliced.push(Stmt::Assign(l.clone(), v));
        }
        (Target::Lvalue(l), true) => {
            spliced.extend(body);
            rewrite_tail_returns(&mut tail, l);
            spliced.push(tail);
        }
    }
    stmts.splice(i..i, spliced);
    true
}

// ---- Pass 4: force of known-WHNF locals ----

/// Per-function-scope facts for the force pass, collected over the whole
/// scope subtree (nested function literals included, because their params
/// and locals shadow, and their bodies may capture this scope's names).
#[derive(Default)]
struct ScopeFacts {
    /// name -> number of binding sites: `local` names, assignment lvalues
    /// (plain identifiers), `AssignIf` lvalues, tokens of `Function`
    /// headers (covers both the declared name and its parameter names),
    /// and nested function-literal parameters.
    binds: std::collections::HashMap<String, usize>,
    /// name -> number of assignment sites (subset of binds).
    assigns: std::collections::HashMap<String, usize>,
    /// Names mentioned inside any Raw text in the scope: poisoned — the
    /// pass cannot see what the Raw does with them.
    raw_mentions: std::collections::HashSet<String>,
}

impl ScopeFacts {
    fn bind(&mut self, n: &str) {
        *self.binds.entry(n.to_string()).or_insert(0) += 1;
    }
    fn assign(&mut self, n: &str) {
        self.bind(n);
        *self.assigns.entry(n.to_string()).or_insert(0) += 1;
    }
    fn raw(&mut self, text: &str) {
        token_set(text, &mut self.raw_mentions);
    }
}

fn is_plain_ident(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with(|c: char| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn collect_facts_block(stmts: &[Stmt], f: &mut ScopeFacts) {
    for s in stmts {
        collect_facts_stmt(s, f);
    }
}

fn collect_facts_stmt(s: &Stmt, f: &mut ScopeFacts) {
    match s {
        Stmt::Raw(t) => f.raw(t),
        Stmt::Local(names, init) => {
            for n in names {
                f.bind(n);
            }
            if let Some(e) = init {
                collect_facts_expr(e, f);
            }
        }
        Stmt::Assign(lhs, e) => {
            if is_plain_ident(lhs) {
                f.assign(lhs);
            }
            collect_facts_expr(e, f);
        }
        Stmt::AssignIf { lhs, cond, then_e, else_e } => {
            if is_plain_ident(lhs) {
                f.assign(lhs);
            }
            collect_facts_expr(cond, f);
            collect_facts_expr(then_e, f);
            collect_facts_expr(else_e, f);
        }
        Stmt::Return(e) | Stmt::Expr(e) => collect_facts_expr(e, f),
        Stmt::If { cond, then_b, elseifs, else_b } => {
            collect_facts_expr(cond, f);
            collect_facts_block(&then_b.0, f);
            for (c, b) in elseifs {
                collect_facts_expr(c, f);
                collect_facts_block(&b.0, f);
            }
            if let Some(b) = else_b {
                collect_facts_block(&b.0, f);
            }
        }
        Stmt::Do(b) => collect_facts_block(&b.0, f),
        Stmt::Function { header, body } => {
            // The header's tokens include the declared name and every
            // parameter name — each is a binding site.
            let mut toks = std::collections::HashSet::new();
            token_set(header, &mut toks);
            for t in toks {
                f.bind(&t);
            }
            collect_facts_block(&body.0, f);
        }
        Stmt::ReturnTable(entries) => {
            for (_, e) in entries {
                collect_facts_expr(e, f);
            }
        }
    }
}

fn collect_facts_expr(e: &Expr, f: &mut ScopeFacts) {
    match e {
        Expr::Name(_) | Expr::Lit(_) => {}
        Expr::Raw(t) => f.raw(t),
        Expr::Paren(e) | Expr::Neg(e) => collect_facts_expr(e, f),
        Expr::Call(c, args) => {
            collect_facts_expr(c, f);
            for a in args {
                collect_facts_expr(a, f);
            }
        }
        Expr::Method(recv, _, args) => {
            collect_facts_expr(recv, f);
            for a in args {
                collect_facts_expr(a, f);
            }
        }
        Expr::Index(base, _) => collect_facts_expr(base, f),
        Expr::Binop(_, l, r) => {
            collect_facts_expr(l, f);
            collect_facts_expr(r, f);
        }
        Expr::Table(items) | Expr::TableSpaced(items) => {
            for item in items {
                match item {
                    Item::Pos(e) | Item::KV(_, e) => collect_facts_expr(e, f),
                }
            }
        }
        Expr::Func(params, body) => {
            for p in params {
                f.bind(p);
            }
            collect_facts_block(body_stmts_ref_fb(body), f);
        }
    }
}

fn body_stmts_ref_fb(body: &FuncBody) -> &Vec<Stmt> {
    match body {
        FuncBody::Inline(s) => s,
        FuncBody::Block(Block(s)) => s,
    }
}

/// An emission that yields WHNF by construction: a function or table
/// literal, a rendered literal token, a cons cell, or a `__force` result
/// (which includes the `p = __force(p)` entry rebind).
fn whnf_by_construction(e: &Expr) -> bool {
    let mut e = e;
    while let Expr::Paren(inner) = e {
        e = inner;
    }
    match e {
        Expr::Func(..) | Expr::Lit(_) | Expr::Table(_) | Expr::TableSpaced(_) => true,
        Expr::Call(f, _) => matches!(
            f.as_ref(),
            Expr::Name(n) if n == "__force" || n == "__mll_cons" || n == "__mll_lazy_cons"
        ),
        _ => false,
    }
}

/// Entry per function scope.
fn force_whnf_block(stmts: &mut [Stmt]) {
    let mut facts = ScopeFacts::default();
    collect_facts_block(stmts, &mut facts);
    let active = std::collections::HashSet::new();
    force_whnf_scope(stmts, &facts, &active);
}

/// A name qualifies once its single value-producing site has run: a sole
/// `local n = <whnf>` (one binding site), or a sole assignment `n = <whnf>`
/// whose one OTHER binding site is the parameter/forward declaration it
/// rebinds (two binding sites, one assignment).
fn whnf_binding_qualifies(facts: &ScopeFacts, n: &str, is_assign: bool) -> bool {
    if facts.raw_mentions.contains(n) {
        return false;
    }
    let binds = facts.binds.get(n).copied().unwrap_or(0);
    let assigns = facts.assigns.get(n).copied().unwrap_or(0);
    if is_assign {
        binds == 2 && assigns == 1
    } else {
        binds == 1 && assigns == 0
    }
}

fn force_whnf_scope(
    stmts: &mut [Stmt],
    facts: &ScopeFacts,
    active: &std::collections::HashSet<String>,
) {
    let mut active = active.clone();
    for s in stmts.iter_mut() {
        // Rewrite uses with the actives established by PRECEDING statements,
        // then consider this statement's own binding.
        match s {
            Stmt::Raw(_) => {}
            Stmt::Local(_, Some(e)) | Stmt::Assign(_, e) | Stmt::Return(e) | Stmt::Expr(e) => {
                rewrite_forces_expr(e, facts, &active)
            }
            Stmt::Local(_, None) => {}
            Stmt::AssignIf { cond, then_e, else_e, .. } => {
                rewrite_forces_expr(cond, facts, &active);
                rewrite_forces_expr(then_e, facts, &active);
                rewrite_forces_expr(else_e, facts, &active);
            }
            Stmt::If { cond, then_b, elseifs, else_b } => {
                rewrite_forces_expr(cond, facts, &active);
                force_whnf_scope(&mut then_b.0, facts, &active);
                for (c, b) in elseifs.iter_mut() {
                    rewrite_forces_expr(c, facts, &active);
                    force_whnf_scope(&mut b.0, facts, &active);
                }
                if let Some(b) = else_b.as_mut() {
                    force_whnf_scope(&mut b.0, facts, &active);
                }
            }
            Stmt::Do(b) => force_whnf_scope(&mut b.0, facts, &active),
            // A named nested function is its own scope.
            Stmt::Function { body, .. } => force_whnf_block(&mut body.0),
            Stmt::ReturnTable(entries) => {
                for (_, e) in entries {
                    rewrite_forces_expr(e, facts, &active);
                }
            }
        }
        match s {
            Stmt::Local(names, Some(rhs)) if names.len() == 1 => {
                if whnf_by_construction(rhs) && whnf_binding_qualifies(facts, &names[0], false) {
                    active.insert(names[0].clone());
                }
            }
            Stmt::Assign(lhs, rhs) if is_plain_ident(lhs)
                && whnf_by_construction(rhs) && whnf_binding_qualifies(facts, lhs, true) => {
                    active.insert(lhs.clone());
                }
            _ => {}
        }
    }
}

/// Replace `__force(n)` by `n` for active names. Descends into nested
/// function literals — closure capture is safe: the single-assignment local
/// never holds a thunk regardless of when the closure runs — but drops any
/// active name shadowed by the literal's parameters first.
fn rewrite_forces_expr(
    e: &mut Expr,
    facts: &ScopeFacts,
    active: &std::collections::HashSet<String>,
) {
    if let Expr::Call(f, args) = e
        && matches!(f.as_ref(), Expr::Name(n) if n == "__force")
        && args.len() == 1
        && let Expr::Name(n) = &args[0]
        && active.contains(n)
    {
        *e = Expr::Name(n.clone());
        return;
    }
    match e {
        Expr::Name(_) | Expr::Lit(_) | Expr::Raw(_) => {}
        Expr::Paren(e) | Expr::Neg(e) => rewrite_forces_expr(e, facts, active),
        Expr::Call(f, args) => {
            rewrite_forces_expr(f, facts, active);
            for a in args {
                rewrite_forces_expr(a, facts, active);
            }
        }
        Expr::Method(recv, _, args) => {
            rewrite_forces_expr(recv, facts, active);
            for a in args {
                rewrite_forces_expr(a, facts, active);
            }
        }
        Expr::Index(base, _) => rewrite_forces_expr(base, facts, active),
        Expr::Binop(_, l, r) => {
            rewrite_forces_expr(l, facts, active);
            rewrite_forces_expr(r, facts, active);
        }
        Expr::Table(items) | Expr::TableSpaced(items) => {
            for item in items {
                match item {
                    Item::Pos(e) | Item::KV(_, e) => rewrite_forces_expr(e, facts, active),
                }
            }
        }
        Expr::Func(params, body) => {
            // Uses inside the literal see this scope's actives minus its
            // own parameters; the literal's BODY statements still belong
            // to a fresh scope for new bindings, which force_whnf_block
            // handles when the walk reaches it via flatten/facts — here we
            // only rewrite captured uses.
            let mut inner = active.clone();
            for p in params {
                inner.remove(p);
            }
            let stmts: &mut Vec<Stmt> = match body {
                FuncBody::Inline(s) => s,
                FuncBody::Block(Block(s)) => s,
            };
            force_whnf_scope(stmts, facts, &inner);
        }
    }
}
