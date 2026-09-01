//! List-pipeline fusion: `foldl' f z (map g (filter p (enumFromTo_Int a b)))`
//! and its relatives emit ONE loop with no intermediate lists.
//!
//! The lazy pipeline pays, per element and per stage: a cons cell, a
//! suspended-tail carrier, a cell force, and a tail dispatch. The
//! handwritten twin is a single loop — that gap (60-160x on the
//! list_pipeline benchmark after every allocation-level optimization) is
//! the intermediate STRUCTURE itself, and only deforestation removes it.
//!
//! `try_fused_list_pipeline` (called from the App arm of the expression
//! walk, like the hmLookup case fusion) matches
//!
//! ```text
//! foldl' F Z CHAIN
//! CHAIN := map G CHAIN | filter P CHAIN
//!        | enumFromTo_Int A B          -- the range source
//!        | <any list expression>       -- leaf source, walked cell by cell
//! ```
//!
//! and emits
//!
//! ```text
//! (function()
//!     local __mll_fu_f  = <F>          -- bound once, in the ORIGINAL
//!     local __mll_fu_a  = <Z>          -- left-to-right evaluation order
//!     local __mll_fu_s0 = <G or P>     -- of the nested call expressions
//!     …
//!     local __mll_fu_i, __mll_fu_hi = <A>, <B>
//!     while true do
//!         if __mll_fu_i > __mll_fu_hi then return __mll_fu_a end
//!         local __mll_fu_x = __mll_fu_i
//!         local __mll_fu_x0 = __mll_fu_s1(__mll_fu_x)      -- map
//!         if __mll_fu_s0(__mll_fu_x0) then                 -- filter
//!             __mll_fu_a = __mll_fu_f(__mll_fu_a, __mll_fu_x0)
//!         end
//!         __mll_fu_i = __mll_fu_i + 1
//!     end
//! end)()
//! ```
//!
//! The loop honors `WhileTrue`'s contract (exit through `return`, never a
//! condition or `break` — `stmt_diverges` relies on it), and the whole
//! emission is structured statements: no `Raw`, so every later pass
//! (paren, dead-branch, hoist) analyzes it like any other tree.
//!
//! SOUNDNESS.
//!
//! * WHEN IT RUNS — the IIFE evaluates where the original call expression
//!   stood; a lazy position still suspends the whole expression, so
//!   nothing runs earlier than the original call would have.
//! * STRICTNESS GATE — the fold function F must be provably strict in
//!   BOTH parameters (a named function's demand row, module-level or
//!   where-bound). Then every `f acc x` the original fold performed
//!   forced the element, so computing each surviving element eagerly in
//!   the loop forces exactly what the lazy pipeline forced; the
//!   accumulator was seq'ed per step by foldl' itself. Without the row,
//!   elements would have to stay suspended per cell — decline to the
//!   general path.
//! * ORDER — elements flow left to right, and per element the stages run
//!   source-outward (transform, then test, then fold), exactly the order
//!   the demand-driven walk forced them; a filter guards only the stages
//!   outside it, so `map h (filter p …)` applies h to survivors only,
//!   as written. Bottoms inside the F/Z/stage EXPRESSIONS surface in the
//!   original left-to-right evaluation order of the nested calls; the
//!   eager Z can surface a Z-bottom before a first-element bottom where
//!   the lazy fold interleaved them — both runs raise, and which bottom
//!   wins is imprecise-exception latitude (the stance every
//!   eager-argument rule in this backend takes).
//! * CALLING CONVENTION — the N-ary eta-padding convention guarantees a
//!   function VALUE of type `a -> b` is callable flat with one argument
//!   (and `b -> a -> b` with two): emitted parameter lists are padded to
//!   the type's arrow count and call sites pass outstanding arguments in
//!   one flat call. The runtime generics (`map(f, xs)` calling
//!   `f(xs[1])`) already lean on exactly this.
//! * NAMES — Prelude names are not shadowable at top level, and
//!   `enumFromTo_Int` is compiler-namespaced, so only LOCAL shadowing is
//!   checked, on every matched name.
//! * RANGE EDGE — `while i <= hi … i = i + 1` reproduces
//!   `enumFromTo_Int`'s own arithmetic, including its behavior at
//!   maxBound (both diverge identically there — a pre-existing Prelude
//!   property fusion does not change).
//! * LEAF SOURCE — an arbitrary list expression is walked
//!   `__force`/`__mll_head`/`__mll_tail` like every runtime consumer;
//!   the element is forced at extraction (F is element-strict, so the
//!   lazy fold forced it too). A bare `foldl' f z xs` with no stages has
//!   no intermediate structure to remove and stays on the general path.

use crate::tir::*;
use super::CodeGen;
use super::function::{FnSpillScope, VarsSnapshot};
use super::lua::{Block, Expr, FuncBody, Stmt};

/// One pipeline stage. Collected outermost-first, applied source-first.
enum Stage<'a> {
    Map(&'a TExpr),
    Filter(&'a TExpr),
}

enum Source<'a> {
    Range(&'a TExpr, &'a TExpr),
    Leaf(&'a TExpr),
}

fn strip_parens(mut e: &TExpr) -> &TExpr {
    while let TExprKind::Paren(p) = &e.kind {
        e = p.as_ref();
    }
    e
}

/// `(head, args)` of an application spine, args in application order.
fn spine(e: &TExpr) -> (&TExpr, Vec<&TExpr>) {
    let mut args = Vec::new();
    let mut h = e;
    while let TExprKind::App(f, a) = &h.kind {
        args.push(a.as_ref());
        h = f.as_ref();
    }
    args.reverse();
    (strip_parens(h), args)
}

impl CodeGen {
    /// The SOURCE name a call-head Var denotes: the mono specialization's
    /// origin (`foldl'_IntT…` -> `foldl'`) or the name itself.
    fn fn_origin<'a>(&'a self, name: &'a str) -> &'a str {
        self.spec_origins.get(name).map(String::as_str).unwrap_or(name)
    }

    pub(super) fn try_fused_list_pipeline(
        &mut self,
        f: &TExpr,
        args: &[&TExpr],
    ) -> Option<Expr> {
        let TExprKind::Var(name) = &f.kind else { return None };
        if self.fn_origin(name) != "foldl'"
            || self.is_local_shadowed(name)
            || args.len() != 3
        {
            return None;
        }
        let (fold_f, z) = (args[0], args[1]);

        // The fold function: a named function whose demand row proves both
        // parameters strict.
        let strict2 = matches!(&strip_parens(fold_f).kind, TExprKind::Var(n)
            if self.local_strict_params.get(n)
                .or_else(|| if self.is_local_shadowed(n) { None }
                          else { self.demand_info.strict_params.get(n) })
                .is_some_and(|row| row.len() >= 2 && row[0] && row[1]));
        if !strict2 {
            return None;
        }

        // Collect stages outermost-first down to the source.
        let mut stages: Vec<Stage> = Vec::new();
        let mut chain = args[2];
        let source = loop {
            let c = strip_parens(chain);
            let (h, sp) = spine(c);
            match &h.kind {
                TExprKind::Var(n)
                    if n == "map" && sp.len() == 2 && !self.is_local_shadowed(n) =>
                {
                    stages.push(Stage::Map(sp[0]));
                    chain = sp[1];
                }
                TExprKind::Var(n)
                    if n == "filter" && sp.len() == 2 && !self.is_local_shadowed(n) =>
                {
                    stages.push(Stage::Filter(sp[0]));
                    chain = sp[1];
                }
                TExprKind::Var(n)
                    if n == "enumFromTo_Int"
                        && sp.len() == 2
                        && !self.is_local_shadowed(n) =>
                {
                    break Source::Range(sp[0], sp[1]);
                }
                _ => break Source::Leaf(c),
            }
        };
        if stages.is_empty() && matches!(source, Source::Leaf(_)) {
            return None;
        }

        // ---- emission (an IIFE: its own Lua function scope) ----
        let scope = VarsSnapshot::capture(self);
        let spill = FnSpillScope::enter(self);
        let mut stmts: Vec<Stmt> = Vec::new();

        // Bind F, Z and the stage functions once, in the original
        // left-to-right evaluation order of the nested calls.
        let f_e = self.forced_ast(fold_f);
        stmts.push(Stmt::Local(vec!["__mll_fu_f".into()], Some(f_e)));
        let z_e = self.arg_ast(z, true);
        stmts.push(Stmt::Local(vec!["__mll_fu_a".into()], Some(z_e)));
        let mut stage_locals: Vec<(bool, String)> = Vec::new(); // (is_filter, name)
        for (i, st) in stages.iter().enumerate() {
            let n = format!("__mll_fu_s{i}");
            let (is_filter, fe) = match st {
                Stage::Map(g) => (false, self.forced_ast(g)),
                Stage::Filter(p) => (true, self.forced_ast(p)),
            };
            stmts.push(Stmt::Local(vec![n.clone()], Some(fe)));
            stage_locals.push((is_filter, n));
        }

        // The per-element body: stages applied source-first (reverse of the
        // collected outermost-first order), the fold step innermost. A map
        // binds a fresh element local; a filter nests everything after it.
        fn build_body(
            source_first: &[(bool, String)],
            cur: Expr,
            next_x: &mut usize,
        ) -> Vec<Stmt> {
            match source_first.split_first() {
                None => vec![Stmt::Assign(
                    "__mll_fu_a".into(),
                    Expr::call_named(
                        "__mll_fu_f",
                        vec![Expr::name("__mll_fu_a"), cur],
                    ),
                )],
                Some(((true, n), rest)) => vec![Stmt::If {
                    cond: Expr::call_named(n, vec![cur.clone()]),
                    then_b: Block(build_body(rest, cur, next_x)),
                    elseifs: vec![],
                    else_b: None,
                }],
                Some(((false, n), rest)) => {
                    let xn = format!("__mll_fu_x{next_x}");
                    *next_x += 1;
                    let mut out = vec![Stmt::Local(
                        vec![xn.clone()],
                        Some(Expr::call_named(n, vec![cur])),
                    )];
                    out.extend(build_body(rest, Expr::name(xn), next_x));
                    out
                }
            }
        }
        let source_first: Vec<(bool, String)> =
            stage_locals.iter().rev().cloned().collect();
        let mut next_x = 0usize;
        let body = build_body(&source_first, Expr::name("__mll_fu_x"), &mut next_x);

        // The source loop. Exit is `return` from inside — WhileTrue's
        // divergence contract — so no statement follows the loop.
        match source {
            Source::Range(a, b) => {
                let a_e = self.arg_ast(a, true);
                let b_e = self.arg_ast(b, true);
                stmts.push(Stmt::Local(vec!["__mll_fu_i".into()], Some(a_e)));
                stmts.push(Stmt::Local(vec!["__mll_fu_hi".into()], Some(b_e)));
                let mut loop_body = vec![
                    Stmt::If {
                        cond: Expr::binop(
                            ">",
                            Expr::name("__mll_fu_i"),
                            Expr::name("__mll_fu_hi"),
                        ),
                        then_b: Block(vec![Stmt::Return(Expr::name("__mll_fu_a"))]),
                        elseifs: vec![],
                        else_b: None,
                    },
                    Stmt::Local(
                        vec!["__mll_fu_x".into()],
                        Some(Expr::name("__mll_fu_i")),
                    ),
                ];
                loop_body.extend(body);
                loop_body.push(Stmt::Assign(
                    "__mll_fu_i".into(),
                    Expr::binop("+", Expr::name("__mll_fu_i"), Expr::lit("1")),
                ));
                stmts.push(Stmt::WhileTrue(Block(loop_body)));
            }
            Source::Leaf(l) => {
                let l_e = self.forced_ast(l);
                stmts.push(Stmt::Local(vec!["__mll_fu_l".into()], Some(l_e)));
                let mut loop_body = vec![
                    Stmt::If {
                        cond: Expr::binop(
                            "==",
                            Expr::name("__mll_fu_l"),
                            Expr::lit("nil"),
                        ),
                        then_b: Block(vec![Stmt::Return(Expr::name("__mll_fu_a"))]),
                        elseifs: vec![],
                        else_b: None,
                    },
                    // F is element-strict, so the lazy fold forced every
                    // element it folded; force at extraction.
                    Stmt::Local(
                        vec!["__mll_fu_x".into()],
                        Some(Expr::force(Expr::call_named(
                            "__mll_head",
                            vec![Expr::name("__mll_fu_l")],
                        ))),
                    ),
                ];
                loop_body.extend(body);
                loop_body.push(Stmt::Assign(
                    "__mll_fu_l".into(),
                    Expr::call_named("__mll_tail", vec![Expr::name("__mll_fu_l")]),
                ));
                stmts.push(Stmt::WhileTrue(Block(loop_body)));
            }
        }

        spill.exit(self);
        scope.restore(self);
        Some(Expr::call(
            Expr::paren(Expr::Func(vec![], FuncBody::Block(Block(stmts)))),
            vec![],
        ))
    }
}
