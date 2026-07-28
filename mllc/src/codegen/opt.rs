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

//! Pass 4 — the `__force`-collapse peephole, run through the annotation
//! engine (annot.rs): `__force(e)` where the analysis stamps `e` WHNF
//! rewrites to `e` (justification: inherit from `e`; `__force` is the
//! identity on a non-thunk, and `e` is still evaluated once, in place).
//! This subsumes the former force-of-known-WHNF-locals pass: the analysis
//! derives the same single-assignment name facts (same qualification, same
//! Raw poisoning, same shadowing rules — see annot.rs) and additionally
//! stamps forces of non-name WHNF expressions, so the dedicated pass was
//! deleted (2026-07-27; corpus-verified byte-identical output).
//!
//! Pass 5 — self-tail-call → loop conversion (tailloop.rs), the structured
//! tier's first pass, run through the annotation engine's structured-pass
//! form (annot.rs): a named function whose body ends in `return <self>(…)`
//! (in statement-tree tail position) becomes a `while true` loop that
//! updates its parameters with one simultaneous multiple assignment and
//! iterates. It runs AFTER the expression passes: their normalizations feed
//! it cleaner trees, and a structured rewrite invalidates every carried
//! stamp, so the engine handed to the refutation must be the one recomputed
//! over the final tree — `run_with` swaps engines when the pass rewrites.
//!
//! Pass 6 — IO self-loop conversion (ioloop.rs), the structured tier's
//! second pass: an IO/ST function in the two-level shape (build-time
//! dispatch returning per-branch action closures) whose branch closures
//! tail-recurse through `return __mll_run_tail(<self>(…))` becomes a
//! dispatch skeleton returning ONE closure that runs the whole self-loop as
//! a `while true` — no per-iteration closure allocation, no runner
//! dispatch. It runs after tailloop for the same recompute-on-rewrite
//! reasons, and because tailloop-converted pure helpers may sit inside the
//! bodies it splices.
//!
//! Pass 7 — direct-perform IO self-loop conversion (performloop.rs), the
//! structured tier's third pass: an IO/ST function that performs at call
//! time and recurses through `return __mll_run_tail(<self>(…))` at the
//! outer body's action tail (directly, or through the dispatch-IIFE /
//! action-closure tree the emitter also produces) becomes a `while true`
//! loop. The self call sits in the runner's argument position — not a Lua
//! tail call — so the unconverted shape pins one frame per step and
//! overflows at ~1e6 depth: this pass is a correctness fix, not just perf.
//! It runs after ioloop: the shapes are disjoint by gating (ioloop needs
//! branch-closure terminals, which performloop's terminal vocabulary
//! declines), and running it last keeps ioloop's claim on anything both
//! could ever match.
//!
//! Per-pass toggles: `MLL_OPT_DISABLE` (read per `run` call) is a
//! comma-separated list of pass names to skip — `parens`, `dead`, `iife`,
//! `force`, `tailloop`, `ioloop`, `performloop`. A debugging aid for
//! isolating a pass's effect; unset (the default) runs everything, and an
//! unrecognized name warns on stderr so a typo cannot silently disable
//! nothing. `CompileOptions::disable_opt_passes` carries the same list
//! per-compile (it overrides the environment variable when set), so a test
//! can pin unoptimized emission without mutating process-global state.

use super::annot;
use super::ioloop;
use super::lua::{Block, Expr, FuncBody, Item, Stmt};
use super::performloop;
use super::tailloop;

/// Which passes to skip; see the module comment.
#[derive(Default)]
pub(super) struct Disable {
    parens: bool,
    dead: bool,
    iife: bool,
    force: bool,
    tailloop: bool,
    ioloop: bool,
    performloop: bool,
}

impl Disable {
    /// Parse a skip list. `spec: Some(list)` uses the explicit list (the
    /// `CompileOptions::disable_opt_passes` path — per-compile, immune to
    /// process-global state); `None` falls back to the `MLL_OPT_DISABLE`
    /// environment variable. Same comma-separated vocabulary either way.
    fn from_spec(spec: Option<&str>) -> Disable {
        let mut d = Disable::default();
        let env;
        let list = match spec {
            Some(s) => s,
            None => match std::env::var("MLL_OPT_DISABLE") {
                Ok(v) => {
                    env = v;
                    &env
                }
                Err(_) => return d,
            },
        };
        for name in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            match name {
                "parens" => d.parens = true,
                "dead" => d.dead = true,
                "iife" => d.iife = true,
                "force" => d.force = true,
                "tailloop" => d.tailloop = true,
                "ioloop" => d.ioloop = true,
                "performloop" => d.performloop = true,
                other => eprintln!(
                    "warning: MLL_OPT_DISABLE: unknown pass name '{}' \
                     (known: parens, dead, iife, force, tailloop, ioloop, \
                     performloop)",
                    other
                ),
            }
        }
        d
    }
}

/// Run all passes over the module body. `opt_disable`: see
/// `Disable::from_spec`.
pub(super) fn run(stmts: &mut Vec<Stmt>, opt_disable: Option<&str>) {
    let _ = run_with(stmts, &Disable::from_spec(opt_disable));
}

/// Run the enabled passes; returns the annotation engine whose mirror is
/// valid over the FINAL tree (the force-collapse engine, or the recomputed
/// engine of a structured rewrite that came after it), plus whether the
/// force pass ran — that decides the refutation's residual-force check.
fn run_with(stmts: &mut Vec<Stmt>, d: &Disable) -> (Option<annot::Engine>, bool) {
    if !d.parens {
        normalize_parens_block(stmts);
    }
    if !d.dead {
        dead_branch_block(stmts);
    }
    if !d.iife {
        flatten_iife_block(stmts);
    }
    let mut engine = if !d.force {
        Some(annot::Engine::run_pass(stmts, &mut ForceCollapse))
    } else {
        None
    };
    if !d.tailloop {
        // Structured tier, after the expression passes (see the module
        // comment). A structured rewrite invalidates every stamp carried so
        // far, so when the pass rewrites, its freshly recomputed engine
        // replaces the force pass's; when it rewrites nothing the earlier
        // engine still mirrors the tree.
        if let Some(fresh) = annot::Engine::run_structured(stmts, &mut tailloop::TailLoop) {
            engine = Some(fresh);
        }
    }
    if !d.ioloop {
        // Structured tier, pass 6 (see the module comment). Same engine
        // discipline as tailloop: a rewrite invalidates everything, so the
        // recomputed engine replaces whichever one came before.
        if let Some(fresh) = annot::Engine::run_structured(stmts, &mut ioloop::IoLoop) {
            engine = Some(fresh);
        }
    }
    if !d.performloop {
        // Structured tier, pass 7 (see the module comment). Same engine
        // discipline; runs after ioloop so the two-level conversions keep
        // their claim.
        if let Some(fresh) =
            annot::Engine::run_structured(stmts, &mut performloop::PerformLoop)
        {
            engine = Some(fresh);
        }
    }
    (engine, !d.force)
}

/// Test-build entry (see verify::check_stamps): run the passes exactly as
/// `run` would, then refute the carried stamps against a fresh analysis of
/// the final tree. Empty means clean.
pub(super) fn run_refuted(stmts: &mut Vec<Stmt>) -> Vec<String> {
    match run_with(stmts, &Disable::from_spec(None)) {
        (Some(engine), force_ran) => engine.refute(stmts, force_ran),
        // With every engine-run pass disabled there are no carried stamps
        // and no collapse obligation; a fresh analysis refuting itself
        // checks nothing.
        (None, _) => Vec::new(),
    }
}

/// The `__force`-collapse peephole (see the module comment): `__force(e)`
/// with a WHNF-stamped `e` becomes `e`, inheriting `e`'s stamps.
struct ForceCollapse;

impl annot::ExprPass for ForceCollapse {
    fn request(&mut self, e: &Expr, stamps: &annot::StampView<'_>) -> Option<annot::Request> {
        let Expr::Call(f, args) = e else { return None };
        if !matches!(f.as_ref(), Expr::Name(n) if n == "__force") || args.len() != 1 {
            return None;
        }
        if stamps.child(1)?.stamp().is_whnf() {
            Some(annot::Request::ReplaceWithChild(1))
        } else {
            None
        }
    }
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

/// Every path through the statement ends in `return` or a raised error —
/// control cannot pass it and continue in the enclosing block.
pub(super) fn stmt_diverges(s: &Stmt) -> bool {
    match s {
        Stmt::Return(_) | Stmt::ReturnNone => true,
        // The vocabulary has no `break`, so a `while true` body can only be
        // left through `return` or a raise: control never passes the loop.
        Stmt::WhileTrue(_) => true,
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

pub(super) fn block_diverges(b: &Block) -> bool {
    b.0.last().is_some_and(stmt_diverges)
}

fn dead_branch_block(stmts: &mut Vec<Stmt>) {
    // Bottom-up: children first, then this block's own rewrites.
    for s in stmts.iter_mut() {
        dead_branch_stmt(s);
    }
    // Straight-line reasoning does not hold in a block with goto labels (a
    // "dead" label may be a live jump target), so skip the block-level
    // rewrites there. In the current pipeline this cannot fire — labels only
    // exist after the tail-loop pass, which runs later — but the pass must
    // stay correct under reordering.
    if stmts.iter().any(|s| matches!(s, Stmt::Goto(_) | Stmt::Label(_))) {
        return;
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
        Stmt::Local(_, None) | Stmt::ReturnNone | Stmt::Goto(_) | Stmt::Label(_) => {}
        Stmt::MultiAssign(_, exprs) => {
            for e in exprs {
                expr_bodies(e);
            }
        }
        Stmt::AssignIf { cond, then_e, else_e, .. } => {
            expr_bodies(cond);
            expr_bodies(then_e);
            expr_bodies(else_e);
        }
        Stmt::Do(b) | Stmt::WhileTrue(b) => dead_branch_block(&mut b.0),
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
pub(super) fn single_return_callee(f: &Expr) -> bool {
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
        // Multi-assignment: the last RHS spreads its values over the
        // remaining lvalues (multi-value position), every other one
        // truncates.
        Stmt::MultiAssign(_, exprs) => {
            let last = exprs.len().saturating_sub(1);
            for (i, e) in exprs.iter_mut().enumerate() {
                normalize_expr(e, if i == last { Ctx::DelimLast } else { Ctx::Delim });
            }
        }
        Stmt::ReturnNone | Stmt::Goto(_) | Stmt::Label(_) => {}
        Stmt::Do(b) | Stmt::WhileTrue(b) => normalize_parens_block_ret(&mut b.0, ret_ctx),
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
pub(super) fn token_set(text: &str, out: &mut std::collections::HashSet<String>) {
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
pub(super) fn count_locals(stmts: &[Stmt]) -> usize {
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
            Stmt::Do(b) | Stmt::WhileTrue(b) => n += count_locals(&b.0),
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
            Stmt::Do(b) | Stmt::WhileTrue(b) => flatten_scope(&mut b.0, budget),
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
        Stmt::Raw(_) | Stmt::Local(_, None) | Stmt::ReturnNone | Stmt::Goto(_)
        | Stmt::Label(_) => {}
        Stmt::Local(_, Some(e)) | Stmt::Assign(_, e) | Stmt::Return(e) | Stmt::Expr(e) => expr(e),
        Stmt::MultiAssign(_, exprs) => {
            for e in exprs {
                expr(e);
            }
        }
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
        Stmt::Do(_) | Stmt::WhileTrue(_) | Stmt::Function { .. } => {}
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
        // A bare return yields zero values — there is nothing to rewrite
        // into an assignment, and treating it as tolerated fall-through
        // would silently swallow the return.
        Stmt::ReturnNone => false,
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn force_collapse_toggle() {
        let make = || vec![Stmt::Return(Expr::force(Expr::lit("42")))];
        let mut on = make();
        run_with(&mut on, &Disable::default());
        assert!(matches!(&on[0], Stmt::Return(Expr::Lit(s)) if s == "42"));
        let mut off = make();
        run_with(&mut off, &Disable { force: true, ..Disable::default() });
        assert!(
            matches!(&off[0], Stmt::Return(Expr::Call(..))),
            "disabled peephole must leave __force in place"
        );
    }

    /// The former force-of-WHNF-locals shape: a single-assignment local
    /// bound to a WHNF producer collapses its later forces.
    #[test]
    fn force_collapse_subsumes_whnf_locals() {
        let mut stmts = vec![
            Stmt::Local(vec!["x".into()], Some(Expr::force(Expr::name("y")))),
            Stmt::Return(Expr::force(Expr::name("x"))),
        ];
        run_with(&mut stmts, &Disable::default());
        assert!(matches!(&stmts[1], Stmt::Return(Expr::Name(n)) if n == "x"));
    }

    /// A force the analysis cannot justify stays.
    #[test]
    fn force_of_unknown_stays() {
        let mut stmts = vec![Stmt::Return(Expr::force(Expr::call_named("f", vec![])))];
        run_with(&mut stmts, &Disable::default());
        assert!(matches!(&stmts[0], Stmt::Return(Expr::Call(..))));
    }

    /// The ioloop toggle: disabled leaves the two-level shape (dispatch
    /// returning branch action closures); enabled (the default) converts
    /// it and the refutation stays clean.
    #[test]
    fn ioloop_toggle() {
        let make = || {
            vec![
                Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
                Stmt::Function {
                    header: "__mll_fn[1] = function(n)".into(),
                    body: Block(vec![Stmt::If {
                        cond: Expr::name("n"),
                        then_b: Block(vec![Stmt::Return(Expr::Func(
                            vec![],
                            FuncBody::Block(Block(vec![Stmt::Return(Expr::lit("nil"))])),
                        ))]),
                        elseifs: vec![],
                        else_b: Some(Block(vec![Stmt::Return(Expr::Func(
                            vec![],
                            FuncBody::Block(Block(vec![Stmt::Return(Expr::call_named(
                                "__mll_run_tail",
                                vec![Expr::call_named("__mll_fn[1]", vec![Expr::lit("1")])],
                            ))])),
                        ))])),
                    }]),
                },
            ]
        };
        let mut on = make();
        let (engine, _) = run_with(&mut on, &Disable::default());
        let Stmt::Function { body, .. } = &on[1] else { panic!("shape") };
        assert!(
            matches!(&body.0[0], Stmt::Local(names, _) if names == &vec!["_lp".to_string()]),
            "enabled pass must convert"
        );
        assert!(engine.expect("engine").refute(&on, true).is_empty());

        let mut off = make();
        run_with(&mut off, &Disable { ioloop: true, ..Disable::default() });
        let Stmt::Function { body, .. } = &off[1] else { panic!("shape") };
        assert!(
            matches!(&body.0[0], Stmt::If { .. }),
            "disabled pass must leave the two-level shape"
        );
    }

    /// The performloop toggle: disabled leaves the direct-perform
    /// recursion (the runner-argument self call); enabled (the default)
    /// converts it and the refutation stays clean.
    #[test]
    fn performloop_toggle() {
        let make = || {
            vec![
                Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
                Stmt::Function {
                    header: "__mll_fn[1] = function(n)".into(),
                    body: Block(vec![Stmt::If {
                        cond: Expr::name("n"),
                        then_b: Block(vec![Stmt::Return(Expr::lit("nil"))]),
                        elseifs: vec![],
                        else_b: Some(Block(vec![Stmt::Return(Expr::call_named(
                            "__mll_run_tail",
                            vec![Expr::call_named("__mll_fn[1]", vec![Expr::lit("1")])],
                        ))])),
                    }]),
                },
            ]
        };
        let mut on = make();
        let (engine, _) = run_with(&mut on, &Disable::default());
        let Stmt::Function { body, .. } = &on[1] else { panic!("shape") };
        assert!(matches!(body.0[0], Stmt::WhileTrue(_)), "enabled pass must convert");
        assert!(engine.expect("engine").refute(&on, true).is_empty());

        let mut off = make();
        run_with(&mut off, &Disable { performloop: true, ..Disable::default() });
        let Stmt::Function { body, .. } = &off[1] else { panic!("shape") };
        assert!(
            matches!(&body.0[0], Stmt::If { .. }),
            "disabled pass must leave the direct-perform recursion"
        );
    }

    /// The tailloop toggle: disabled leaves the recursive call; enabled
    /// (the default) converts it and the refutation stays clean.
    #[test]
    fn tailloop_toggle() {
        let make = || {
            vec![Stmt::Function {
                header: "local function f(x)".into(),
                body: Block(vec![Stmt::If {
                    cond: Expr::name("x"),
                    then_b: Block(vec![Stmt::Return(Expr::lit("0"))]),
                    elseifs: vec![],
                    else_b: Some(Block(vec![Stmt::Return(Expr::call_named(
                        "f",
                        vec![Expr::lit("1")],
                    ))])),
                }]),
            }]
        };
        let mut on = make();
        let (engine, _) = run_with(&mut on, &Disable::default());
        let Stmt::Function { body, .. } = &on[0] else { panic!("shape") };
        assert!(matches!(body.0[0], Stmt::WhileTrue(_)), "enabled pass must convert");
        assert!(engine.expect("engine").refute(&on, true).is_empty());

        let mut off = make();
        run_with(&mut off, &Disable { tailloop: true, ..Disable::default() });
        let Stmt::Function { body, .. } = &off[0] else { panic!("shape") };
        assert!(
            matches!(body.0[0], Stmt::If { .. }),
            "disabled pass must leave the recursion"
        );
    }

    /// run_refuted on a clean pipeline reports nothing.
    #[test]
    fn refutation_clean_after_passes() {
        let mut stmts = vec![
            Stmt::Local(vec!["x".into()], Some(Expr::force(Expr::name("y")))),
            Stmt::Return(Expr::force(Expr::name("x"))),
        ];
        assert!(run_refuted(&mut stmts).is_empty());
    }
}
