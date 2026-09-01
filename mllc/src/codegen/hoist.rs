//! Pass 7 — loop-invariant closure hoisting, the structured tier's third
//! pass (see opt.rs's pipeline comment and annot.rs's structured-tier
//! contract).
//!
//! A function literal evaluated at ITERATION LEVEL of a `while true` loop —
//! directly in a loop-body statement's expressions, not inside another
//! literal — allocates one closure per iteration. Under LuaJIT that is the
//! FNEW trace abort: a loop that would otherwise compile to machine code
//! falls back to the interpreter the moment an iteration creates a closure.
//! When every free name of such a literal resolves OUTSIDE the loop, the
//! closure is the same on every iteration in everything but identity, so the
//! pass hoists it once, immediately before the loop:
//!
//! ```text
//! while true do                 local _h0 = function() … end
//!     …                    →    while true do
//!     f(function() … end)           …
//! end                               f(_h0)
//!                               end
//! ```
//!
//! Correctness decisions:
//!
//! * CAPTURE — Lua closures capture VARIABLES (upvalue references), not
//!   values. The hoist is sound exactly when no free name of the literal is
//!   bound inside the loop body: every free name then resolves to the same
//!   variable instance at the hoist point as at the literal's position —
//!   assignments to those variables are seen by the hoisted closure exactly
//!   as the per-iteration one saw them. A name bound inside the loop (a
//!   per-iteration `local`, the `_w` copies) blocks: those are fresh
//!   variables each iteration, and moving the capture out of the loop would
//!   both change which instance is captured and leave the name out of scope.
//!   The blocked set is the loop body's WHOLE binder set (every `local` and
//!   `local function` name in its statement tree, nested loops included,
//!   literal interiors excluded — their binders do not scope outside) — a
//!   superset of the path-accurate set, which can only decline more.
//! * CREATION TIMING — building a closure is pure and total (an allocation,
//!   no user code runs), so creating it once before the loop instead of on
//!   the iterations/branches that reach the literal is unobservable, except
//!   as function identity. mata-ll exposes no function equality — Haskell
//!   has none, and the FFI marshals closures without comparing them — so
//!   one shared closure is indistinguishable from fresh ones. (`__thunk`
//!   CALLS are never hoisted — a thunk table is mutable memoization state
//!   and must stay per-iteration; hoisting the literal INSIDE a
//!   `__thunk(function() … end)` argument is fine and covered: the wrapper
//!   call stays in the loop, the closure is shared.)
//! * ITERATION LEVEL ONLY — literals inside nested literals are created per
//!   CALL of the enclosing closure, not per iteration; they are skipped
//!   (each literal body is its own hoist scope with its own loops). A
//!   nested `while true` is processed first, so a closure it hoists lands
//!   at the outer loop's iteration level and may hoist again.
//! * RAW POLICY — free names are computed over the structured vocabulary;
//!   any Raw fragment (statement or expression) inside a literal
//!   contributes its identifier TOKENS as free names (`opt::token_set` —
//!   rendered text cannot hide a reference from the tokenizer), which
//!   over-approximates and can only block. A Raw STATEMENT in the loop body
//!   itself could DECLARE a local the binder scan cannot see, so it blocks
//!   the whole loop.
//! * SCOPE PLUMBING — the pass is offered named functions
//!   (`Stmt::Function`); nested named functions were offered by their own
//!   scope walk and are skipped, while function LITERALS in the offered
//!   body (ioloop's `_lp` driver) are walked here as fresh hoist scopes —
//!   no other offer reaches them. Hoisted locals respect the emitter's
//!   per-function local budget (`count_locals` + parameters against
//!   `LOCAL_LIMIT`), declining once exhausted.

use std::collections::HashSet;

use super::annot::{self, ScopeView, is_plain_ident};
use super::lua::{Block, Expr, FnTarget, Item, Stmt};
use super::opt;
use super::tailloop::used_tokens;

pub(super) struct HoistClosures;

impl annot::StructuredPass for HoistClosures {
    fn request(
        &mut self,
        target: &FnTarget,
        params: &[String],
        body: &Block,
        _view: &ScopeView<'_>,
        _locals_in_scope: &HashSet<String>,
    ) -> Option<Block> {
        // One fresh-name pool for the whole request: tokens of the rendered
        // function cover every nested literal body, so a name fresh here is
        // fresh in any scope the walk hoists into.
        let mut used = used_tokens(target, params, &body.0);
        let mut new_body = body.clone();
        let mut any = false;
        hoist_fn_scope(params.len(), &mut new_body.0, &mut used, &mut any);
        if any { Some(new_body) } else { None }
    }
}

/// One function scope (the offered body, or a literal's body): its own
/// local budget, then the block walk.
fn hoist_fn_scope(
    param_count: usize,
    stmts: &mut Vec<Stmt>,
    used: &mut HashSet<String>,
    any: &mut bool,
) {
    let mut budget = super::CodeGen::LOCAL_LIMIT
        .saturating_sub(opt::count_locals(stmts) + param_count);
    hoist_block(stmts, used, &mut budget, any);
}

/// Walk a statement list of the current function scope: literal bodies are
/// fresh scopes, nested NAMED functions were offered separately, and every
/// `while true` first processes its own interior, then hoists its eligible
/// iteration-level literals to just before itself.
fn hoist_block(
    stmts: &mut Vec<Stmt>,
    used: &mut HashSet<String>,
    budget: &mut usize,
    any: &mut bool,
) {
    let mut i = 0;
    while i < stmts.len() {
        match &mut stmts[i] {
            // Own offer; its body is not this request's to rewrite.
            Stmt::Function { .. } => {
                i += 1;
                continue;
            }
            Stmt::WhileTrue(b) => {
                hoist_block(&mut b.0, used, budget, any);
                let hoisted = hoist_from_loop(&mut b.0, used, budget);
                let n = hoisted.len();
                if n > 0 {
                    *any = true;
                    for (j, s) in hoisted.into_iter().enumerate() {
                        stmts.insert(i + j, s);
                    }
                }
                i += n + 1;
                continue;
            }
            Stmt::If { then_b, elseifs, else_b, .. } => {
                hoist_block(&mut then_b.0, used, budget, any);
                for (_, b) in elseifs.iter_mut() {
                    hoist_block(&mut b.0, used, budget, any);
                }
                if let Some(b) = else_b {
                    hoist_block(&mut b.0, used, budget, any);
                }
            }
            Stmt::Do(b) => hoist_block(&mut b.0, used, budget, any),
            _ => {}
        }
        // Literal bodies anywhere in this statement's expressions are their
        // own hoist scopes (ioloop's `_lp` driver literal in particular).
        stmts[i].for_each_expr_mut(&mut |e| descend_literals(e, used, any));
        i += 1;
    }
}

fn descend_literals(e: &mut Expr, used: &mut HashSet<String>, any: &mut bool) {
    if let Expr::Func(params, body) = e {
        hoist_fn_scope(params.len(), body.stmts_mut(), used, any);
        return;
    }
    e.for_each_subexpr_mut(&mut |c| descend_literals(c, used, any));
}

/// Hoist the eligible iteration-level literals of one loop body; returns
/// the `local _hN = function … end` statements to insert before the loop.
fn hoist_from_loop(
    body: &mut Vec<Stmt>,
    used: &mut HashSet<String>,
    budget: &mut usize,
) -> Vec<Stmt> {
    // A Raw statement could declare a local the binder scan cannot see.
    if has_raw_stmt(body) {
        return Vec::new();
    }
    let mut bound = HashSet::new();
    binders_block(body, &mut bound);

    let mut hoisted = Vec::new();
    collect_block(body, &mut |e| {
        if *budget == 0 {
            return;
        }
        let Expr::Func(params, fb) = e else { unreachable!("collect offers literals only") };
        let free = free_names_func(params, fb.stmts());
        if !free.is_disjoint(&bound) {
            return;
        }
        let name = fresh_h(used);
        used.insert(name.clone());
        *budget -= 1;
        let lit = std::mem::replace(e, Expr::Name(name.clone()));
        hoisted.push(Stmt::Local(vec![name], Some(lit)));
    });
    hoisted
}

/// `_h0`, `_h1`, … — first index whose name no rendered token uses.
fn fresh_h(used: &HashSet<String>) -> String {
    (0..)
        .map(|i| format!("_h{}", i))
        .find(|c| !used.contains(c))
        .expect("some index is fresh")
}

/// Every iteration-level literal of the loop body: statement expressions
/// walked outside nested literals, nested `while true` interiors skipped
/// (already processed — anything a name bound there blocks here too).
fn collect_block(stmts: &mut [Stmt], f: &mut impl FnMut(&mut Expr)) {
    for s in stmts {
        match s {
            Stmt::WhileTrue(_) | Stmt::Function { .. } => {}
            Stmt::If { cond, then_b, elseifs, else_b } => {
                collect_expr(cond, f);
                collect_block(&mut then_b.0, f);
                for (c, b) in elseifs.iter_mut() {
                    collect_expr(c, f);
                    collect_block(&mut b.0, f);
                }
                if let Some(b) = else_b {
                    collect_block(&mut b.0, f);
                }
            }
            Stmt::Do(b) => collect_block(&mut b.0, f),
            other => other.for_each_expr_mut(&mut |e| collect_expr(e, f)),
        }
    }
}

fn collect_expr(e: &mut Expr, f: &mut impl FnMut(&mut Expr)) {
    if matches!(e, Expr::Func(..)) {
        f(e);
        return;
    }
    e.for_each_subexpr_mut(&mut |c| collect_expr(c, f));
}

fn has_raw_stmt(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| {
        matches!(s, Stmt::Raw(_)) || {
            let mut hit = false;
            s.for_each_block(&mut |b| hit = hit || has_raw_stmt(b));
            hit
        }
    })
}

/// Every name the loop body's statement tree binds: `local` declarations
/// and `local function` headers, sub-blocks (nested loops) included,
/// literal interiors excluded — their binders do not scope outside.
fn binders_block(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::Local(names, _) => out.extend(names.iter().cloned()),
            Stmt::Function { target: FnTarget::LocalFn(n), .. } => {
                out.insert(n.clone());
            }
            _ => {}
        }
        s.for_each_block(&mut |b| binders_block(b, out));
    }
}

// ---- Free names of a function literal ----

/// The free plain-identifier names of `function(params) body end`: every
/// name its body references that no parameter or in-scope `local` binds.
/// Opaque spellings — Raw fragments, composite `Name`s (`math.pi`,
/// `_v[2]`), index suffixes, rendered lvalues — contribute their identifier
/// tokens instead (minus the bound set), which over-approximates: a spare
/// name can only BLOCK a hoist. `Lit` content is never tokenized (a quoted
/// string is not a reference).
pub(super) fn free_names_func(params: &[String], body: &[Stmt]) -> HashSet<String> {
    let mut free = HashSet::new();
    let bound: HashSet<String> = params.iter().cloned().collect();
    free_block(body, &bound, &mut free);
    free
}

fn opaque_text(text: &str, bound: &HashSet<String>, free: &mut HashSet<String>) {
    let mut toks = HashSet::new();
    opt::token_set(text, &mut toks);
    free.extend(toks.into_iter().filter(|t| !bound.contains(t)));
}

fn free_block(stmts: &[Stmt], bound: &HashSet<String>, free: &mut HashSet<String>) {
    // Cloned per block: a `local` binds for the rest of its own block only.
    let mut bound = bound.clone();
    for s in stmts {
        match s {
            Stmt::Raw(t) => opaque_text(t, &bound, free),
            Stmt::Local(names, rhs) => {
                // Lua evaluates the initializer BEFORE the names bind.
                if let Some(e) = rhs {
                    free_expr(e, &bound, free);
                }
                bound.extend(names.iter().cloned());
            }
            Stmt::Assign(lhs, e) | Stmt::AssignIf { lhs, then_e: e, .. } => {
                if !bound.contains(lhs) {
                    opaque_text(lhs, &bound, free);
                }
                if let Stmt::AssignIf { cond, else_e, .. } = s {
                    free_expr(cond, &bound, free);
                    free_expr(else_e, &bound, free);
                }
                free_expr(e, &bound, free);
            }
            Stmt::MultiAssign(lhs, exprs) => {
                for l in lhs {
                    if !bound.contains(l) {
                        opaque_text(l, &bound, free);
                    }
                }
                for e in exprs {
                    free_expr(e, &bound, free);
                }
            }
            Stmt::Return(e) | Stmt::Expr(e) => free_expr(e, &bound, free),
            Stmt::If { cond, then_b, elseifs, else_b } => {
                free_expr(cond, &bound, free);
                free_block(&then_b.0, &bound, free);
                for (c, b) in elseifs {
                    free_expr(c, &bound, free);
                    free_block(&b.0, &bound, free);
                }
                if let Some(b) = else_b {
                    free_block(&b.0, &bound, free);
                }
            }
            Stmt::Do(b) | Stmt::WhileTrue(b) => free_block(&b.0, &bound, free),
            Stmt::Function { target, params, body } => {
                match target {
                    // `local function f` binds f, its own body included.
                    FnTarget::LocalFn(n) => {
                        bound.insert(n.clone());
                    }
                    // `f = function` assigns an existing variable.
                    FnTarget::Assigned(n) => {
                        if !bound.contains(n) {
                            opaque_text(n, &bound, free);
                        }
                    }
                    FnTarget::Slot(_) => {
                        if !bound.contains(super::lua::FN_TABLE) {
                            free.insert(super::lua::FN_TABLE.to_string());
                        }
                    }
                }
                let mut inner = bound.clone();
                inner.extend(params.iter().cloned());
                free_block(&body.0, &inner, free);
            }
            Stmt::ReturnTable(entries) => {
                for (_, e) in entries {
                    free_expr(e, &bound, free);
                }
            }
            Stmt::ReturnNone | Stmt::Goto(_) | Stmt::Label(_) => {}
        }
    }
}

fn free_expr(e: &Expr, bound: &HashSet<String>, free: &mut HashSet<String>) {
    match e {
        Expr::Name(s) => {
            if is_plain_ident(s) {
                if !bound.contains(s) {
                    free.insert(s.clone());
                }
            } else {
                opaque_text(s, bound, free);
            }
        }
        Expr::Lit(_) => {}
        Expr::Raw(t) => opaque_text(t, bound, free),
        Expr::Paren(inner) | Expr::Neg(inner) => free_expr(inner, bound, free),
        Expr::Call(f, args) => {
            free_expr(f, bound, free);
            for a in args {
                free_expr(a, bound, free);
            }
        }
        // The method name is a field, not a variable.
        Expr::Method(recv, _, args) => {
            free_expr(recv, bound, free);
            for a in args {
                free_expr(a, bound, free);
            }
        }
        Expr::Index(base, suffix) => {
            free_expr(base, bound, free);
            opaque_text(suffix, bound, free);
        }
        Expr::Binop(_, l, r) => {
            free_expr(l, bound, free);
            free_expr(r, bound, free);
        }
        Expr::Table(items) | Expr::TableSpaced(items) => {
            for item in items {
                match item {
                    Item::Pos(e) => free_expr(e, bound, free),
                    // The key is a field name, not a variable.
                    Item::KV(_, e) => free_expr(e, bound, free),
                }
            }
        }
        Expr::Func(params, body) => {
            let mut inner = bound.clone();
            inner.extend(params.iter().cloned());
            free_block(body.stmts(), &inner, free);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::annot::Engine;
    use crate::codegen::lua::FuncBody;

    /// Run the pass over a module and return the rendered output.
    fn hoisted(mut stmts: Vec<Stmt>) -> (String, bool) {
        let rewrote = Engine::run_structured(&mut stmts, &mut HoistClosures).is_some();
        let mut out = String::new();
        Block(stmts).render(0, &mut out);
        (out, rewrote)
    }

    /// A closure literal calling `g(x)` — `x` free.
    fn lit_calling(free_name: &str) -> Expr {
        Expr::Func(
            vec![],
            FuncBody::Block(Block(vec![Stmt::Return(Expr::call_named(
                "g",
                vec![Expr::name(free_name)],
            ))])),
        )
    }

    /// `local function f(x) while true do return h(<lit>) end end` — the
    /// literal at iteration level, its free names chosen by the caller.
    fn loop_with_literal(lit: Expr, pre_loop: Vec<Stmt>, loop_head: Vec<Stmt>) -> Vec<Stmt> {
        let mut body = pre_loop;
        let mut inner = loop_head;
        inner.push(Stmt::Return(Expr::call_named("h", vec![lit])));
        body.push(Stmt::WhileTrue(Block(inner)));
        vec![Stmt::Function {
            target: FnTarget::LocalFn("f".into()),
            params: vec!["x".into()],
            body: Block(body),
        }]
    }

    #[test]
    fn invariant_literal_hoists_before_the_loop() {
        // The literal's one free name is the enclosing parameter — bound
        // outside the loop, so the closure is the same every iteration.
        let (out, rewrote) = hoisted(loop_with_literal(lit_calling("x"), vec![], vec![]));
        assert!(rewrote);
        assert!(
            out.contains("local _h0 = function"),
            "invariant literal must hoist: {out}"
        );
        assert!(out.contains("return h(_h0)"), "use site reads the local: {out}");
        // The hoist sits before the loop, not inside it.
        let hoist_at = out.find("local _h0").unwrap();
        let loop_at = out.find("while true do").unwrap();
        assert!(hoist_at < loop_at, "hoist must precede the loop: {out}");
    }

    #[test]
    fn loop_bound_capture_blocks_the_hoist() {
        // `local i = x` inside the loop: a fresh variable every iteration —
        // a closure capturing it must stay where it is.
        let (out, rewrote) = hoisted(loop_with_literal(
            lit_calling("i"),
            vec![],
            vec![Stmt::Local(vec!["i".into()], Some(Expr::name("x")))],
        ));
        assert!(!rewrote, "per-iteration capture must not hoist: {out}");
    }

    #[test]
    fn raw_statement_in_the_loop_blocks_every_hoist() {
        // A Raw statement could declare a local the binder scan cannot see.
        let (out, rewrote) = hoisted(loop_with_literal(
            lit_calling("x"),
            vec![],
            vec![Stmt::Raw("local x = 1".into())],
        ));
        assert!(!rewrote, "Raw in the loop body must block: {out}");
    }

    #[test]
    fn nested_literal_is_not_iteration_level() {
        // The candidate literal contains another literal; only the OUTER
        // one is created per iteration, and it hoists as a whole.
        let inner = lit_calling("x");
        let outer = Expr::Func(
            vec![],
            FuncBody::Block(Block(vec![Stmt::Return(Expr::call_named(
                "h",
                vec![inner],
            ))])),
        );
        let (out, rewrote) = hoisted(loop_with_literal(outer, vec![], vec![]));
        assert!(rewrote);
        // Exactly one hoist: the outer literal, inner riding along.
        assert!(out.contains("local _h0 = function"), "{out}");
        assert!(!out.contains("_h1"), "inner literal must ride along: {out}");
    }

    #[test]
    fn thunk_wrapper_stays_per_iteration() {
        // `__thunk(function() return g(x) end)`: the closure hoists, the
        // mutable thunk table must stay inside the loop.
        let lit = lit_calling("x");
        let stmts = vec![Stmt::Function {
            target: FnTarget::LocalFn("f".into()),
            params: vec!["x".into()],
            body: Block(vec![Stmt::WhileTrue(Block(vec![Stmt::Return(
                Expr::call_named("h", vec![Expr::thunk(lit)]),
            )]))]),
        }];
        let (out, rewrote) = hoisted(stmts);
        assert!(rewrote);
        assert!(out.contains("local _h0 = function"), "{out}");
        assert!(
            out.contains("h(__thunk(_h0))"),
            "the __thunk call must stay at the use site: {out}"
        );
    }
}
