//! Expression-splitting pass: bound the syntactic nesting of emitted Lua.
//!
//! A Lua host refuses a chunk at LOAD time when one expression nests
//! deeper than its parser's recursion budget (`LUAI_MAXCCALLS`, 200, on
//! PUC Lua — reported as `C stack overflow`; `LJ_MAX_XLEVEL`, 200, on
//! LuaJIT — `chunk has too many syntax levels`) or when one function needs
//! more register slots than a frame holds (`LJ_MAX_SLOTS`, 250 —
//! `function or expression too complex`; `MAXREGS`, 255, on PUC). mata-ll
//! emits a function body as one expression tree wherever the source is
//! one, so a valid program with a long chain — `a ++ b ++ … ` over 65
//! operands, `p && q && …` over 100, `f . g . …` over 55, `f $ g $ … $ x`
//! over 150, an `if … else if …` ladder of 190 arms, 250 nested `case`s,
//! `x + (y + (…))` over 190 — compiled to Lua that no host would load.
//! The per-construct costs, measured on both hosts, are tabulated in
//! `codegen/nesting.rs`, which also measures the finished tree in test
//! builds; this pass is what keeps the tree under those budgets.
//!
//! The pass runs on the monomorphized, constant-folded, newtype-erased TIR
//! just before codegen. It estimates, per sub-expression, the syntax
//! levels and the register slots the emitter will spend on it (`Cost`,
//! an upper bound over the emission shapes), and wherever a child at a
//! hoistable position would exceed [`LEVEL_BUDGET`] or [`TEMP_BUDGET`]
//! it pulls that child out into a `let` binding and references it by
//! name. Codegen lowers a `Let`'s bindings to SIBLING `local` statements
//! (or to an IIFE whose bindings are siblings), so each extracted piece
//! is parsed at shallow depth and a chain of any length becomes a
//! sequence of bindings each within budget, every one referring to the
//! previous by name.
//!
//! Correctness rests on referential transparency plus how codegen emits a
//! `let` binding:
//!   * Only pure sub-expressions are extracted: first-order values (never
//!     an IO/LuaIO/ST action — those carry effect structure codegen
//!     pattern-matches), and function COMPOSITIONS, which are closures —
//!     building one evaluates nothing. Lambdas, partial applications and
//!     every other function value stay where they are.
//!   * Each extracted expression is referenced exactly once, so no work is
//!     duplicated or newly shared.
//!   * A `let` binding is emitted eagerly only where evaluating it early
//!     is unobservable — its body cannot bottom and runs no user code
//!     (`is_cheap_to_force`: concrete operands, non-trapping native
//!     operators, constructors), or the group's body provably forces it
//!     first (demand analysis, exact-first-force) — and is a memoizing
//!     thunk otherwise, forced at exactly the point its single reference
//!     is forced. So an extracted operand keeps its laziness whatever the
//!     position it came from: the lazy right operand of `&&`/`||`/`++`,
//!     the argument of a non-strict callee, an `if` branch. Short-circuit
//!     order is preserved exactly: a chunk of `q && r && …` that is
//!     thunked runs only when the `&&` before it reached it, and a chunk
//!     that is emitted eagerly is one whose operands are all provably
//!     total comparisons over values already in WHNF — evaluating those
//!     ahead of time computes the same booleans and observes nothing.
//!   * Extraction never crosses a binder: lambda bodies, `let` bindings
//!     and bodies and where-bindings each start a fresh binding scope, so
//!     an extracted piece only references names in scope where it lands.
//!     A conditional position that binds nothing — an `if` branch, a
//!     `case` branch whose pattern binds nothing, a guard — is NOT a
//!     boundary: the bindings extracted inside it float to the enclosing
//!     scope, because a thunked binding allocated on a path that never
//!     forces it is unobservable and an eager one is total. A `case`
//!     branch that binds pattern variables floats only the bindings that
//!     mention none of them (`mentions`); the rest stay in the branch's
//!     own scope. Floating is what flattens a ladder: each `if … else if`
//!     arm is the else branch of the one before it, and a binding kept
//!     inside the branch would still be parsed inside every outer arm.
//!   * The one rewrite that is not an extraction: a `>>` chain nested to
//!     the left past the budget is re-associated to the right
//!     (`rotate_then_chain`), which the bind-chain emitter writes as a
//!     statement sequence; at the action monads — the only ones whose
//!     `>>` reaches codegen as an operator — both associations run the
//!     same actions in the same order.
//!
//! For expressions within budget nothing changes, so existing byte-exact
//! output is unaffected: the budgets sit above anything hand-written (an
//! ordinary program in the test corpus emits at most ~50 levels and ~45
//! expression slots; only the deliberately deep stress cases come near
//! them) and below every host limit with the fixed wrapping codegen adds
//! around a body accounted for (`codegen/nesting.rs` measures the result
//! in test builds: the deepest emitted function of the corpus, a
//! 70-operand chain inside nested closures, reaches 104 of the 200
//! levels).

use crate::tir::*;
use crate::types::Ty;

/// Syntax levels one extracted sub-expression may cost the host parser,
/// as `Cost::levels` estimates them. The budget is set from the measured
/// costs in `codegen/nesting.rs`: the hosts allow 200 levels; codegen
/// wraps a body in at most ~30 fixed levels (the chunk statement, the
/// function body, a thunk or action closure around a binding, a pattern
/// match's dispatch closure and condition nesting); and on PUC every
/// frame the HOST is nested in when it loads the chunk costs a level too,
/// so a `require` from inside a few `pcall`s must still fit. 96 leaves
/// that host margin at roughly a third of the limit, and is still twice
/// the deepest expression in the test corpus.
pub const LEVEL_BUDGET: usize = 96;

/// The floor of the register-slot budget one extracted sub-expression may
/// hold above a function's locals, as `Cost::temps` estimates them. The
/// budget itself is per clause (`Cx::for_clause`): the frame's slots less
/// the clause's own binders. A function at the emitter's local limit
/// (`LOCAL_LIMIT`, 180 plain locals plus the spill table and its
/// parameters) has about 50 of LuaJIT's 250 slots left for expression
/// temporaries; a nested call holds two slots per pending callee plus one
/// per argument already built, so 48 admits a dozen pending
/// three-argument calls or two dozen one-argument ones there, while a
/// function with few locals keeps most of the frame. A table constructor
/// is bounded by the host itself (fifty pending items are flushed) and is
/// not counted against this.
pub const TEMP_BUDGET: usize = 48;

/// Rewrite every function/CAF body so no emitted Lua expression exceeds
/// the budgets.
pub fn split_module(mut module: TModule) -> TModule {
    // Pass-order witness: split measures the FOLDED tree (folding shrinks
    // chains it would otherwise split).
    debug_assert_eq!(
        module.passes_run.last(),
        Some(&"newtype_erase"),
        "split must run on the folded, newtype-erased module"
    );
    module.passes_run.push("split");
    for f in module.functions.iter_mut() {
        split_function(f);
    }
    for f in module.instance_fns.iter_mut() {
        split_function(f);
    }
    module
}

fn split_function(f: &mut TFunction) {
    for clause in f.clauses.iter_mut() {
        let mut cx = Cx::for_clause(clause);
        // where-bindings are in scope for the body and guards; flatten each in
        // its own scope so extracted temps stay local to that binding.
        for wb in clause.where_binds.iter_mut() {
            let body = std::mem::replace(&mut wb.body, dummy());
            wb.body = flatten_scope(body, &mut cx);
        }
        for g in clause.guards.iter_mut() {
            let cond = std::mem::replace(&mut g.condition, dummy());
            g.condition = flatten_scope(cond, &mut cx);
            let body = std::mem::replace(&mut g.body, dummy());
            g.body = flatten_scope(body, &mut cx);
        }
        clause.body = clause.body.take().map(|b| flatten_scope(b, &mut cx));
    }
}

fn dummy() -> TExpr {
    TExpr::new(TExprKind::Lit(TLiteral::Unit), Ty::Unit)
}

/// The per-clause state of the walk: the counter naming the `__splitN`
/// bindings, and the slot budget one extracted expression may hold in
/// THIS clause's function — the frame's `FRAME_SLOTS` less the locals the
/// clause declares (`clause_binders`: parameters, where-bindings, every
/// `let` and pattern binder of the body — an upper bound on what codegen
/// declares in one Lua function, since a nested lambda's binders get a
/// frame of their own), capped at the emitter's spill limit, and never
/// below [`TEMP_BUDGET`]. A CAF or a small function keeps nearly the
/// whole frame for its expressions; one at the local limit keeps the
/// floor.
struct Cx {
    ctr: usize,
    temps: usize,
}

impl Cx {
    fn for_clause(clause: &TClause) -> Cx {
        let locals = clause_binders(clause).min(SPILLED_LOCALS);
        Cx { ctr: 0, temps: FRAME_SLOTS.saturating_sub(locals).max(TEMP_BUDGET) }
    }
}

/// The slots of a frame left to expressions and locals together: LuaJIT's
/// 250 (`LJ_MAX_SLOTS`) less a margin for the temporaries codegen adds
/// (match scrutinees, loop copies, the spill table) and the frame's own
/// bookkeeping — the same 240 `codegen/nesting.rs` checks the tree
/// against, less eight for those temporaries.
const FRAME_SLOTS: usize = 232;

/// The most plain locals one emitted function holds: the emitter's
/// `LOCAL_LIMIT` (180) plus the spill table, parameters and a few
/// temporaries; past the limit a binding is a slot of the `_v` table and
/// costs no register.
const SPILLED_LOCALS: usize = 190;

/// Every name a clause binds in its own Lua function (an upper bound: a
/// lambda's or local function's binders count too, though codegen gives
/// those a frame of their own).
fn clause_binders(clause: &TClause) -> usize {
    let mut n = 0usize;
    for p in &clause.patterns {
        n += p.bound_vars().len();
    }
    for wb in &clause.where_binds {
        n += 1;
        n += expr_binders(&wb.body);
    }
    for g in &clause.guards {
        n += expr_binders(&g.condition) + expr_binders(&g.body);
    }
    if let Some(b) = &clause.body {
        n += expr_binders(b);
    }
    n
}

fn expr_binders(e: &TExpr) -> usize {
    let mut n = match &e.kind {
        TExprKind::Lambda { params, .. } => params.len(),
        TExprKind::Let { binds, .. } => binds.len(),
        TExprKind::Case { branches, .. } => {
            branches.iter().map(|b| b.pattern.bound_vars().len()).sum::<usize>() + 1
        }
        _ => 0,
    };
    e.for_each_child(&mut |c| n += expr_binders(c));
    n
}

/// Flatten an expression that begins a fresh binding scope (a whole body, a
/// lambda body, a let binding, ...). Extracted bindings are collected here
/// and, if any, wrapped around the result in a single `Let` (sibling
/// bindings).
fn flatten_scope(e: TExpr, cx: &mut Cx) -> TExpr {
    let mut binds: Vec<TLocalDef> = Vec::new();
    let inner = flatten_expr(e, &mut binds, cx);
    wrap_let(binds, inner)
}

fn wrap_let(binds: Vec<TLocalDef>, body: TExpr) -> TExpr {
    if binds.is_empty() {
        body
    } else {
        let ty = body.ty.clone();
        TExpr::new(TExprKind::Let { binds, body: Box::new(body) }, ty)
    }
}

/// Flatten a conditional position — an `if` branch, a `case` branch body
/// or guard — that binds `bound` (a case branch's pattern variables; empty
/// for an `if` branch or a guard). Bindings extracted inside it float to
/// `outer` unless they mention a bound name or an earlier binding that had
/// to stay; those stay in a `Let` around the branch. Then the branch
/// itself, if still over budget and free of the bound names, is extracted
/// to `outer` as one more binding — this is where a ladder is cut.
fn hoist_branch(
    e: TExpr,
    bound: &[String],
    outer: &mut Vec<TLocalDef>,
    cx: &mut Cx,
) -> TExpr {
    let mut local: Vec<TLocalDef> = Vec::new();
    let inner = flatten_expr(e, &mut local, cx);
    let mut staying: Vec<TLocalDef> = Vec::new();
    let mut staying_names: Vec<String> = bound.to_vec();
    for b in local {
        if mentions(&b.body, &staying_names) {
            staying_names.push(b.name.clone());
            staying.push(b);
        } else {
            outer.push(b);
        }
    }
    let inner = wrap_let(staying, inner);
    if over_budget(&inner, cx) && is_extractable(&inner) && !mentions(&inner, bound) {
        extract(inner, outer, cx)
    } else {
        inner
    }
}

/// Rebuild `e`, hoisting any deep pure child into `binds` (post-order, so a
/// binding that references an earlier temp appears after it).
fn flatten_expr(e: TExpr, binds: &mut Vec<TLocalDef>, cx: &mut Cx) -> TExpr {
    let ty = e.ty.clone();
    let kind = match e.kind {
        // Leaves.
        k @ (TExprKind::Var(_)
        | TExprKind::Con(_)
        | TExprKind::Lit(_)
        | TExprKind::OpFunc(_)
        | TExprKind::DictAccess { .. }) => k,

        TExprKind::Paren(inner) => {
            TExprKind::Paren(Box::new(flatten_expr(*inner, binds, cx)))
        }
        // A constructed dictionary's method: recurse into the dictionary
        // expression but never hoist it (it carries dictionary structure).
        TExprKind::DictMethod { dict, method_name, nullary } => TExprKind::DictMethod {
            dict: Box::new(flatten_expr(*dict, binds, cx)),
            method_name,
            nullary,
        },
        TExprKind::Negate(inner) => {
            TExprKind::Negate(Box::new(hoist(*inner, binds, cx)))
        }
        TExprKind::App(f, a) => {
            // Recurse into the callee spine (so deep args nested inside it are
            // handled) but never hoist the callee itself: codegen pattern-matches
            // application spines (cons, seq, method inlining, ...) and a bare
            // Var head would defeat that.
            let f = flatten_expr(*f, binds, cx);
            let a = hoist(*a, binds, cx);
            TExprKind::App(Box::new(f), Box::new(a))
        }
        TExprKind::InfixApp { op, lhs, rhs } => {
            if is_action_op(&op) {
                // IO bind / sequencing: leave the shape intact (the
                // operands carry effect structure), just recurse so nested
                // pure sub-expressions inside are still split. The one
                // reshaping is a `>>` chain nested to the LEFT past the
                // budget, which is re-associated to the right (see
                // `rotate_then_chain`).
                let (lhs, rhs) = if op == ">>" && over_budget(&lhs, cx) {
                    rotate_then_chain(*lhs, *rhs)
                } else {
                    (*lhs, *rhs)
                };
                let lhs = flatten_expr(lhs, binds, cx);
                let rhs = flatten_expr(rhs, binds, cx);
                TExprKind::InfixApp { op, lhs: Box::new(lhs), rhs: Box::new(rhs) }
            } else {
                // Every other operator: both operands are hoist points.
                // `f $ x` is the application `f x` (the callee spine on the
                // left is never a value, so only `x` can be extracted);
                // `f . g` extracts a deep side that is itself a
                // composition (a closure value, see `is_extractable`);
                // a native or user operator extracts a deep operand of
                // value type. Laziness is codegen's binding rule, not a
                // per-operator question here (see the module comment).
                let lhs = hoist(*lhs, binds, cx);
                let rhs = hoist(*rhs, binds, cx);
                TExprKind::InfixApp { op, lhs: Box::new(lhs), rhs: Box::new(rhs) }
            }
        }
        TExprKind::If { cond, then_branch, else_branch } => {
            // cond is always evaluated: hoist into the current scope. The
            // branches are conditional positions binding nothing.
            let cond = hoist(*cond, binds, cx);
            let then_branch = hoist_branch(*then_branch, &[], binds, cx);
            let else_branch = hoist_branch(*else_branch, &[], binds, cx);
            TExprKind::If {
                cond: Box::new(cond),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            }
        }
        TExprKind::Case { scrutinee, branches } => {
            // The scrutinee is always evaluated (matched).
            let scrutinee = hoist(*scrutinee, binds, cx);
            let branches = branches
                .into_iter()
                .map(|mut b| {
                    // A branch binds its pattern variables: what mentions
                    // them stays inside the branch, the rest floats.
                    let bound = b.pattern.bound_vars();
                    for g in b.guards.iter_mut() {
                        let c = std::mem::replace(&mut g.condition, dummy());
                        g.condition = hoist_branch(c, &bound, binds, cx);
                        let bd = std::mem::replace(&mut g.body, dummy());
                        g.body = hoist_branch(bd, &bound, binds, cx);
                    }
                    b.body = b.body.take().map(|bb| hoist_branch(bb, &bound, binds, cx));
                    b
                })
                .collect();
            TExprKind::Case { scrutinee: Box::new(scrutinee), branches }
        }
        TExprKind::Lambda { params, body } => {
            // Cannot hoist across the parameter binder: fresh scope for the
            // body (and a thunk floated out of a lambda would be shared
            // across its calls — a change of sharing GHC makes only under
            // full laziness).
            TExprKind::Lambda { params, body: Box::new(flatten_scope(*body, cx)) }
        }
        TExprKind::Let { binds: mut lbinds, body } => {
            for b in lbinds.iter_mut() {
                let bd = std::mem::replace(&mut b.body, dummy());
                b.body = flatten_scope(bd, cx);
            }
            TExprKind::Let { binds: lbinds, body: Box::new(flatten_scope(*body, cx)) }
        }
        TExprKind::SpecCall { original, specialized, args } => {
            let args = args.into_iter().map(|a| hoist(a, binds, cx)).collect();
            TExprKind::SpecCall { original, specialized, args }
        }
        TExprKind::Tuple(elems) => {
            let elems = elems.into_iter().map(|a| hoist(a, binds, cx)).collect();
            TExprKind::Tuple(elems)
        }
        TExprKind::DictCall { func_name, dict_args, value_args } => {
            let dict_args = dict_args.into_iter().map(|a| flatten_expr(a, binds, cx)).collect();
            let value_args = value_args.into_iter().map(|a| hoist(a, binds, cx)).collect();
            TExprKind::DictCall { func_name, dict_args, value_args }
        }
        TExprKind::RecordUpdate { record, updates, num_fields } => {
            let record = hoist(*record, binds, cx);
            let updates = updates
                .into_iter()
                .map(|(n, i, v)| (n, i, hoist(v, binds, cx)))
                .collect();
            TExprKind::RecordUpdate { record: Box::new(record), updates, num_fields }
        }
        // A callback lowered out to Lua: recurse into the callee in its own
        // scope but do not hoist it (it carries closure/marshalling structure).
        TExprKind::OutgoingCallback {
            callee, arity, run_io,
        } => TExprKind::OutgoingCallback {
            callee: Box::new(flatten_scope(*callee, cx)),
            arity, run_io,
        },
        // An optional FFI argument: recurse but never hoist the wrapper itself
        // — it must stay directly inside its SpecCall argument list.
        TExprKind::FfiMaybeArg { value } => TExprKind::FfiMaybeArg {
            value: Box::new(flatten_scope(*value, cx)),
        },
    };
    TExpr::new(kind, ty)
}

/// Flatten a child expression and, if it is still over budget and safe to
/// name, pull it into a fresh `let` binding, returning a reference to it.
fn hoist(child: TExpr, binds: &mut Vec<TLocalDef>, cx: &mut Cx) -> TExpr {
    let child = flatten_expr(child, binds, cx);
    if over_budget(&child, cx) && is_extractable(&child) {
        extract(child, binds, cx)
    } else {
        child
    }
}

fn extract(child: TExpr, binds: &mut Vec<TLocalDef>, cx: &mut Cx) -> TExpr {
    let name = format!("__split{}", cx.ctr);
    cx.ctr += 1;
    let ty = child.ty.clone();
    binds.push(TLocalDef { name: name.clone(), patterns: Vec::new(), body: child });
    TExpr::new(TExprKind::Var(name), ty)
}

fn over_budget(e: &TExpr, cx: &Cx) -> bool {
    let c = cost(e);
    c.levels > LEVEL_BUDGET || c.temps > cx.temps
}

/// `(a >> b) >> c` as `a >> (b >> c)`, over the whole left spine of `>>`
/// nodes (parens seen through; a `>>=` in the spine is an atom — moving a
/// statement under its lambda could capture a name). `>>` is `infixl 1`,
/// so `a >> b >> c >> …` arrives left-nested, and codegen's bind chain
/// walks the RIGHT spine as statements while a left operand that is
/// itself a chain runs as a nested runner closure — one per operand. After
/// monomorphization an InfixApp `>>` exists only at the action monads (IO,
/// LuaIO, ST — `mono::is_action_monad`; every other instance is a call to
/// its `then_*` method), and for those `>>` is sequencing: both shapes run
/// `a`, then `b`, then `c`, discard the same results and propagate the
/// same exception, so the rotation is exact. Only a spine past the budget
/// is rotated, so a short chain keeps its emission.
fn rotate_then_chain(lhs: TExpr, rhs: TExpr) -> (TExpr, TExpr) {
    // The spine's atoms, leftmost first.
    let mut atoms: Vec<TExpr> = Vec::new();
    let mut node = lhs;
    loop {
        match node.kind {
            TExprKind::Paren(inner) => node = *inner,
            TExprKind::InfixApp { op, lhs, rhs } if op == ">>" => {
                atoms.push(*rhs);
                node = *lhs;
            }
            kind => {
                node.kind = kind;
                break;
            }
        }
    }
    let leftmost = node;
    // `atoms` holds the right operands outermost-first, i.e. the chain's
    // order reversed; rebuild from the original right operand inwards.
    let mut acc = rhs;
    for a in atoms {
        let ty = acc.ty.clone();
        acc = TExpr::new(
            TExprKind::InfixApp { op: ">>".into(), lhs: Box::new(a), rhs: Box::new(acc) },
            ty,
        );
    }
    (leftmost, acc)
}

/// The action-sequencing operators, whose operand shape codegen's bind
/// chain depends on.
fn is_action_op(op: &str) -> bool {
    matches!(op, ">>=" | ">>")
}

/// Whether an expression may be named by a `let` binding without changing
/// semantics or breaking a codegen shape assumption: a pure first-order
/// value, or a composition whose result is one (a closure — naming it
/// evaluates nothing, and a let-bound function is called like any local).
fn is_extractable(e: &TExpr) -> bool {
    // Naming a bare reference or literal is pointless (and Vars/Cons may be part
    // of an application spine handled specially by codegen).
    if matches!(
        e.kind,
        TExprKind::Var(_) | TExprKind::Con(_) | TExprKind::Lit(_) | TExprKind::OpFunc(_)
    ) {
        return false;
    }
    if is_value_type(&e.ty) {
        return true;
    }
    if let TExprKind::InfixApp { op, .. } = &e.kind
        && op == "."
    {
        let mut res = &e.ty;
        while let Ty::Arrow(_, r, _) = res {
            res = r.as_ref();
        }
        return is_value_type(res);
    }
    false
}

/// Pure, first-order value types — never functions, effects (IO/LuaIO/ST) or
/// unresolved type variables (which could stand for any of those).
fn is_value_type(ty: &Ty) -> bool {
    match ty {
        Ty::Con(_) | Ty::List(_) | Ty::Tuple(_) | Ty::Unit | Ty::Promoted(_) => true,
        Ty::App(_, _) => !is_st_type(ty),
        Ty::Arrow(..)
        | Ty::IO(_)
        | Ty::LuaIO(_, _)
        | Ty::Forall(_, _)
        | Ty::Skolem(_, _)
        | Ty::Var(_) => false,
    }
}

/// `ST s a` — an effectful (mutable) action, threaded as a closure by codegen.
fn is_st_type(ty: &Ty) -> bool {
    match ty {
        Ty::App(f, _) => match f.as_ref() {
            Ty::App(c, _) => matches!(c.as_ref(), Ty::Con(n) if n == "ST"),
            _ => false,
        },
        _ => false,
    }
}

/// Does `e` mention any of `names` as a variable? Over-approximate — a
/// rebinding of the name inside `e` still counts — which only keeps a
/// binding from floating.
fn mentions(e: &TExpr, names: &[String]) -> bool {
    if names.is_empty() {
        return false;
    }
    let mut found = false;
    fn walk(e: &TExpr, names: &[String], found: &mut bool) {
        if *found {
            return;
        }
        if let TExprKind::Var(n) = &e.kind
            && names.iter().any(|m| m == n)
        {
            *found = true;
            return;
        }
        e.for_each_child(&mut |c| walk(c, names, found));
    }
    walk(e, names, &mut found);
    found
}

/// The host-parser cost of the Lua codegen emits for an expression: the
/// syntax levels it nests and the register slots its evaluation holds
/// above the function's locals. Upper bounds over the shapes the emitter
/// chooses, in the units `codegen/nesting.rs` measures, per node:
///
///   * a call argument opens one level, and the argument may be
///     suspended in a thunk (`__thunk(function() return … end)`: a call
///     argument, a statement and a return operand — three more) or
///     captured by a partial application's closure; a pending call holds
///     its callee and frame link (two slots) plus every argument already
///     built, and a thunk starts a fresh frame — the estimate takes the
///     un-suspended shape, which is the one that accumulates;
///   * a native operator's right operand is a nested `subexpr` under its
///     own grouping parens, and a non-WHNF operand is wrapped in
///     `__force(…)`: three levels, one slot held for the left value; the
///     left operand is read by the same `subexpr` (its parens cost one);
///   * `++` suspends its right operand (`__mll_list_append(l, function()
///     return r end)`: three levels, a fresh frame); a cons cell suspends
///     its tail the same way (`cons_cost`), and a list literal of eight
///     or more elements is built flat by one statement per element
///     (`flat_list_literal`, the emitter's own test); `!!`, `seq` and
///     every user operator are calls;
///   * composition nests the inner side as an immediately-invoked
///     closure (`f((function(_x) return g(_x) end)(_x))`: four levels);
///   * `if`, `case` and `let` in value position are immediately-invoked
///     closures around statements (`(function() if c then return a else
///     return b end end)()`: four levels to a branch, a match's dispatch
///     and condition nesting two more, a thunked binding three more); a
///     lambda is a function literal (statement and return operand, plus
///     the parens a callee literal needs); a tuple is a table whose items
///     may be thunked; a record update an invoked closure of thunked
///     assignments; an outgoing callback a marshalling wrapper.
///
/// A TIR `Paren` is emitted as a Lua paren (shed later where the position
/// proves it redundant, kept as a binop operand) and costs one level.
#[derive(Clone, Copy, Default)]
struct Cost {
    levels: usize,
    temps: usize,
}

impl Cost {
    const LEAF: Cost = Cost { levels: 1, temps: 1 };

    fn above(self, levels: usize, temps: usize) -> Cost {
        Cost { levels: self.levels + levels, temps: self.temps + temps }
    }

    fn max(self, other: Cost) -> Cost {
        Cost { levels: self.levels.max(other.levels), temps: self.temps.max(other.temps) }
    }

    /// A fresh function frame: the levels carry on, the slots start over.
    fn framed(self) -> Cost {
        Cost { levels: self.levels, temps: 1 }
    }
}

/// Levels a call argument costs, suspended or captured (see `Cost`).
const ARG_LEVELS: usize = 4;
/// Levels a native operator's right operand costs (operand, paren, force).
const OPERAND_LEVELS: usize = 3;
/// Levels composition costs its inner side (the invoked closure).
const COMPOSE_LEVELS: usize = 4;
/// Levels an `if`/`case` branch costs under the value-position closure.
const BRANCH_LEVELS: usize = 6;
/// Levels a `let` binding costs under the value-position closure, thunked.
const LET_BIND_LEVELS: usize = 6;

fn cost(e: &TExpr) -> Cost {
    match &e.kind {
        TExprKind::Var(_)
        | TExprKind::Con(_)
        | TExprKind::Lit(_)
        | TExprKind::OpFunc(_)
        | TExprKind::DictAccess { .. } => Cost::LEAF,
        TExprKind::DictMethod { dict, .. } => cost(dict).above(1, 0),
        TExprKind::Paren(inner) => cost(inner).above(1, 0),
        TExprKind::Negate(inner) => cost(inner).above(OPERAND_LEVELS, 0),
        TExprKind::App(..) => {
            // A list literal of eight or more elements is emitted flat
            // (one cons statement per element inside an invoked closure):
            // each element sits at the closure's level, nothing nests.
            if let Some(elems) = crate::codegen::flat_list_literal(e) {
                let mut c = Cost::LEAF;
                for el in elems {
                    c = c.max(cost(el).above(ARG_LEVELS, 0));
                }
                return c.framed();
            }
            let mut head = e;
            let mut args: Vec<&TExpr> = Vec::new();
            while let TExprKind::App(f, a) = &head.kind {
                args.push(a.as_ref());
                head = f.as_ref();
            }
            args.reverse();
            // Any other saturated cons is a cell with a suspended tail.
            if matches!(&head.kind, TExprKind::Con(n) if n == ":") && args.len() == 2 {
                return cons_cost(args[0], args[1]);
            }
            // A resolved primitive method applied to its two operands
            // (`semigroup_String s t`, `ord_lt__Int a b` — the App
            // spelling of `<>`, `<`, …) inlines to the native operator
            // (`try_primitive_method_app`), with the operator's cost.
            if let TExprKind::Var(name) = &head.kind
                && args.len() == 2
                && crate::codegen::primitive_method_lua_op(name).is_some()
            {
                return native_op_cost(args[0], args[1]);
            }
            // One flat call over the spine's arguments: the head's own
            // cost (a non-variable head is a parenthesized callee), and
            // argument `i` built above the callee, its link and the `i`
            // arguments before it.
            call_cost(cost(head).above(1, 0), &args)
        }
        TExprKind::SpecCall { args, .. } => {
            call_cost(Cost::LEAF, &args.iter().collect::<Vec<_>>())
        }
        TExprKind::DictCall { dict_args, value_args, .. } => {
            let args: Vec<&TExpr> = dict_args.iter().chain(value_args.iter()).collect();
            call_cost(Cost::LEAF, &args)
        }
        TExprKind::InfixApp { op, lhs, rhs } => match op.as_str() {
            "." => cost(lhs).above(COMPOSE_LEVELS, 0).max(cost(rhs).above(COMPOSE_LEVELS, 0)).framed(),
            "$" => call_cost(cost(lhs).above(1, 0), &[rhs.as_ref()]),
            "++" => cost(lhs).above(ARG_LEVELS, 2).max(cost(rhs).above(ARG_LEVELS, 0).framed()),
            ":" => cons_cost(lhs, rhs),
            ">>=" | ">>" => {
                // A bind chain's RIGHT spine is emitted as a statement
                // sequence (`bind_chain_block` walks it iteratively; a
                // bind's lambda is spliced as the statements that follow),
                // so the continuation costs nothing per statement; the
                // left operand runs as a statement of its own, nested in
                // a runner closure when it is itself a chain.
                let rest = match &rhs.kind {
                    TExprKind::Lambda { body, .. } if op == ">>=" => cost(body),
                    _ => cost(rhs),
                };
                cost(lhs).above(ARG_LEVELS, 0).framed().max(rest)
            }
            "&&" | "||" => cost(lhs).above(1, 0).max(cost(rhs).above(OPERAND_LEVELS, 0)),
            _ if crate::codegen::is_builtin_op(op) => native_op_cost(lhs, rhs),
            _ => call_cost(Cost::LEAF, &[lhs.as_ref(), rhs.as_ref()]),
        },
        TExprKind::If { cond, then_branch, else_branch } => cost(cond)
            .above(2, 0)
            .max(cost(then_branch).above(BRANCH_LEVELS, 0))
            .max(cost(else_branch).above(BRANCH_LEVELS, 0)),
        TExprKind::Case { scrutinee, branches } => {
            let mut c = cost(scrutinee).above(ARG_LEVELS, 0);
            for b in branches {
                if let Some(bb) = &b.body {
                    c = c.max(cost(bb).above(BRANCH_LEVELS, 0));
                }
                for g in &b.guards {
                    c = c.max(cost(&g.condition).above(BRANCH_LEVELS, 0));
                    c = c.max(cost(&g.body).above(BRANCH_LEVELS, 0));
                }
            }
            c
        }
        TExprKind::Let { binds, body } => {
            let mut c = cost(body).above(BRANCH_LEVELS, 0);
            for b in binds {
                c = c.max(cost(&b.body).above(LET_BIND_LEVELS, 0));
            }
            c
        }
        TExprKind::Lambda { body, .. } => cost(body).above(3, 0).framed(),
        TExprKind::Tuple(elems) => {
            let mut c = Cost::LEAF;
            for (i, el) in elems.iter().enumerate() {
                c = c.max(cost(el).above(ARG_LEVELS, 1 + (i % 50)));
            }
            c
        }
        TExprKind::RecordUpdate { record, updates, .. } => {
            let mut c = cost(record).above(ARG_LEVELS, 0);
            for (_, _, v) in updates {
                c = c.max(cost(v).above(ARG_LEVELS + 3, 0));
            }
            c.framed()
        }
        TExprKind::OutgoingCallback { callee, .. } => cost(callee).above(BRANCH_LEVELS, 0).framed(),
        TExprKind::FfiMaybeArg { value } => cost(value).above(1, 0),
    }
}

/// A native Lua operator over forced operands (`operator_infix_ast`,
/// `try_primitive_method_app`): the right operand is a nested `subexpr`
/// under its grouping paren and a possible `__force`, holding the left
/// value in one slot; the left operand is read by the same `subexpr`.
fn native_op_cost(lhs: &TExpr, rhs: &TExpr) -> Cost {
    cost(lhs).above(1, 0).max(cost(rhs).above(OPERAND_LEVELS, 1))
}

/// A cons cell (`cons_infix_ast`): the head is a lazy argument, the tail a
/// suspended closure (`__mll_lazy_cons(h, function() return t end)`:
/// argument, statement, return operand) unless it is a bare reference —
/// three levels per cell and a fresh frame each. Thunk lifting usually
/// makes the tail a top-level function, which nests nothing; the estimate
/// takes the shape that remains when a cell captures too much to lift.
fn cons_cost(head: &TExpr, tail: &TExpr) -> Cost {
    cost(head).above(ARG_LEVELS, 0).framed().max(cost(tail).above(3, 0).framed())
}

/// One Lua call: the callee's cost, and argument `i` opening a level above
/// the call with the callee, its frame link and the `i` earlier arguments
/// held in slots.
fn call_cost(head: Cost, args: &[&TExpr]) -> Cost {
    let mut c = head;
    for (i, a) in args.iter().enumerate() {
        c = c.max(cost(a).above(ARG_LEVELS, 2 + i));
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int() -> Ty {
        Ty::Con("Int".into())
    }

    fn var(n: &str) -> TExpr {
        TExpr::new(TExprKind::Var(n.into()), int())
    }

    fn lit(i: i64) -> TExpr {
        TExpr::new(TExprKind::Lit(TLiteral::Integer(i)), int())
    }

    fn infix(op: &str, l: TExpr, r: TExpr) -> TExpr {
        TExpr::new(TExprKind::InfixApp { op: op.into(), lhs: Box::new(l), rhs: Box::new(r) }, int())
    }

    fn app(f: TExpr, a: TExpr) -> TExpr {
        TExpr::new(TExprKind::App(Box::new(f), Box::new(a)), int())
    }

    /// A cut is made at the first child found over budget, so a binding's
    /// body may exceed the budget by the cost of its own root node.
    fn within_cut_bound(e: &TExpr) -> bool {
        let c = cost(e);
        c.levels <= LEVEL_BUDGET + BRANCH_LEVELS && c.temps <= TEMP_BUDGET + 4
    }

    /// A right-nested `+` chain costs three levels and one slot per
    /// operand; a left-nested one costs one level and holds no slots.
    #[test]
    fn operand_chains() {
        let mut r = lit(0);
        for i in 0..10 {
            r = infix("+", lit(i), r);
        }
        let c = cost(&r);
        assert_eq!(c.levels, 1 + 10 * OPERAND_LEVELS);
        assert_eq!(c.temps, 11);
        let mut l = lit(0);
        for i in 0..10 {
            l = infix("+", l, lit(i));
        }
        let c = cost(&l);
        // The innermost right operand's three, then one paren per level.
        assert_eq!(c.levels, 1 + OPERAND_LEVELS + 9);
        assert_eq!(c.temps, 2);
    }

    /// A call spine is one flat call: arguments add slots, not levels,
    /// and a nested call charges the callee, the link and the earlier
    /// arguments.
    #[test]
    fn call_spines() {
        let wide = app(app(app(var("f"), lit(1)), lit(2)), lit(3));
        let c = cost(&wide);
        assert_eq!(c.levels, 1 + ARG_LEVELS);
        assert_eq!(c.temps, 1 + 2 + 2);
        let mut nested = lit(0);
        for _ in 0..5 {
            nested = app(app(var("g"), lit(1)), nested);
        }
        let c = cost(&nested);
        assert_eq!(c.levels, 1 + 5 * ARG_LEVELS);
        assert_eq!(c.temps, 1 + 5 * 3);
    }

    /// A deep right operand of `&&` is extracted even though it is cheap
    /// (its binding stays lazy by codegen's rule), and the chain becomes
    /// bindings each within budget.
    #[test]
    fn deep_and_chain_is_cut_into_bindings() {
        // On the compiler's stack: a 200-operand TIR chain is recursive
        // data, and libtest's thread is far smaller than the compiler's.
        crate::with_compiler_stack(deep_and_chain_is_cut_into_bindings_body);
    }

    fn deep_and_chain_is_cut_into_bindings_body() {
        let bool_ty = Ty::Con("Bool".into());
        let cmp = |i: i64| TExpr::new(
            TExprKind::InfixApp { op: "<".into(), lhs: Box::new(var("n")), rhs: Box::new(lit(i)) },
            bool_ty.clone(),
        );
        let mut chain = cmp(200);
        for i in (1..200).rev() {
            chain = TExpr::new(
                TExprKind::InfixApp { op: "&&".into(), lhs: Box::new(cmp(i)), rhs: Box::new(chain) },
                bool_ty.clone(),
            );
        }
        let mut cx = Cx { ctr: 0, temps: TEMP_BUDGET };
        let out = flatten_scope(chain, &mut cx);
        let TExprKind::Let { binds, body } = &out.kind else { panic!("expected a Let") };
        assert!(binds.len() >= 4, "200 operands need several cuts: {}", binds.len());
        assert!(!over_budget(body, &cx));
        for b in binds {
            assert!(within_cut_bound(&b.body), "binding {} is over budget", b.name);
        }
    }

    /// An `if … else if` ladder: every arm floats out of the branch it sits
    /// in, so the cut bindings are siblings of the outermost `if`.
    #[test]
    fn if_ladder_floats_out_of_its_branches() {
        crate::with_compiler_stack(if_ladder_floats_out_of_its_branches_body);
    }

    fn if_ladder_floats_out_of_its_branches_body() {
        let bool_ty = Ty::Con("Bool".into());
        let cmp = |i: i64| TExpr::new(
            TExprKind::InfixApp { op: "==".into(), lhs: Box::new(var("n")), rhs: Box::new(lit(i)) },
            bool_ty.clone(),
        );
        let mut ladder = lit(0);
        for i in (0..300).rev() {
            ladder = TExpr::new(
                TExprKind::If {
                    cond: Box::new(cmp(i)),
                    then_branch: Box::new(lit(i * 2)),
                    else_branch: Box::new(ladder),
                },
                int(),
            );
        }
        let mut cx = Cx { ctr: 0, temps: TEMP_BUDGET };
        let out = flatten_scope(ladder, &mut cx);
        let TExprKind::Let { binds, body } = &out.kind else { panic!("expected a Let") };
        assert!(binds.len() >= 10, "300 arms need many cuts: {}", binds.len());
        assert!(!over_budget(body, &cx));
        for b in binds {
            assert!(within_cut_bound(&b.body), "binding {} is over budget", b.name);
            // A floated arm references the previous cut by name only.
            assert!(!matches!(b.body.kind, TExprKind::Let { .. }));
        }
    }

    /// `((a >> b) >> c) >> d` becomes `a >> (b >> (c >> d))`.
    #[test]
    fn then_chain_rotates_to_the_right() {
        let io = Ty::IO(Box::new(Ty::Unit));
        let act = |n: &str| TExpr::new(TExprKind::Var(n.into()), io.clone());
        let then = |l: TExpr, r: TExpr| {
            let ty = r.ty.clone();
            TExpr::new(TExprKind::InfixApp { op: ">>".into(), lhs: Box::new(l), rhs: Box::new(r) }, ty)
        };
        let paren = |e: TExpr| { let ty = e.ty.clone(); TExpr::new(TExprKind::Paren(Box::new(e)), ty) };
        let lhs = paren(then(paren(then(act("a"), act("b"))), act("c")));
        let (l, r) = rotate_then_chain(lhs, act("d"));
        let name = |e: &TExpr| match &e.kind { TExprKind::Var(n) => n.clone(), _ => "?".into() };
        assert_eq!(name(&l), "a");
        let TExprKind::InfixApp { lhs: b, rhs: r2, .. } = &r.kind else { panic!() };
        assert_eq!(name(b), "b");
        let TExprKind::InfixApp { lhs: c, rhs: d, .. } = &r2.kind else { panic!() };
        assert_eq!(name(c), "c");
        assert_eq!(name(d), "d");
    }

    /// A branch that mentions its pattern variable keeps its bindings.
    #[test]
    fn mentions_sees_through_children() {
        let e = infix("+", app(var("f"), var("x")), lit(1));
        assert!(mentions(&e, &["x".into()]));
        assert!(!mentions(&e, &["y".into()]));
    }
}
