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
//! CONSUMER := foldl' F Z CHAIN
//!           | sum CHAIN               -- at Int/Number: acc + x natively
//!           | length CHAIN            -- counts; element values undemanded
//! CHAIN    := map G CHAIN | filter P CHAIN | take N CHAIN
//!           | enumFromTo_Int A B      -- the range source
//!           | <any list expression>   -- leaf source, walked cell by cell
//! ```
//!
//! and emits
//!
//! ```text
//! (function()
//!     local __mll_fu_f  = <F>          -- bound once, in the ORIGINAL
//!     local __mll_fu_a  = <Z>          -- left-to-right evaluation order
//!     local __mll_fu_s0 = <G or P>     -- of the nested call expressions
//!     local __mll_fu_k0 = <N>          -- a take stage's budget
//!     …
//!     local __mll_fu_i, __mll_fu_hi = <A>, <B>
//!     while true do
//!         if __mll_fu_k0 <= 0 then return __mll_fu_a end   -- take gates
//!         if __mll_fu_i > __mll_fu_hi then return __mll_fu_a end
//!         local __mll_fu_x = __mll_fu_i
//!         __mll_fu_k0 = __mll_fu_k0 - 1                    -- at its position
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
//! * STRICTNESS GATES — eager per-element evaluation is sound only when
//!   the lazy pipeline would have demanded exactly the same work, so
//!   EVERY function in the pipeline must be provably strict in the
//!   parameter that receives the element:
//!     - the fold F in BOTH parameters (then every `f acc x` forced the
//!       element, and foldl' itself seq'ed the accumulator per step);
//!     - every map's G and every filter's P in their one parameter. A
//!       spine-strict consumer runs every filter's P (inclusion must be
//!       decided per cell) and demands every map output on the survivor
//!       path, so with strict stage functions demand reaches the source
//!       unconditionally — without a stage's row, a lazy G or P
//!       (`const 1`, `\_ -> True`) would leave inner elements UNDEMANDED
//!       in the lazy pipeline while the loop computed them (a bottom the
//!       original never hit), so any unproven stage declines the fusion.
//!   A function value proves its row three ways: a NAMED function's
//!   demand row (module-level or where-bound, through the mono
//!   specialization's spec_origin), possibly partially applied (the row's
//!   tail covers the remaining parameters); a first-class OPERATOR
//!   (`(+)` and friends) at native scalar types; or a LAMBDA whose body
//!   syntactically forces the parameter on every path (the conservative
//!   `forces_param` walk — sections desugar to lambdas, so `(* 3)`
//!   qualifies).
//!   `take` has no function and needs no row; `length` demands no element
//!   values at all, so its extraction and stage handling differ (below).
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
//! * TAKE — a take stage owns a budget counter. It DECREMENTS at the
//!   stage's position (only elements that reach it — surviving the
//!   filters inside it — consume budget) but its exhaustion CHECK runs at
//!   the TOP of the loop, before the source is even advanced: when a
//!   take's budget is spent the lazy pipeline stops pulling entirely, so
//!   no cell is forced, no inner filter runs, and no further source
//!   element is touched. `take 0 (⊥ : …)` returns without forcing the
//!   head, exactly as the lazy `take` would. On a LEAF source the budgets
//!   are re-checked after the body too, before the tail advance: the
//!   element that spends the last budget must not force the next cell's
//!   suspension (`take 2 (filter odd (1 : 3 : ⊥))` never touches ⊥). The
//!   budget expression evaluates where the original `take n …` call did.
//! * LENGTH — `length` forces the spine, never the elements: a Map stage
//!   with no Filter outside it produces values nothing demands, so those
//!   stages are DROPPED from the loop entirely — the lazy pipeline
//!   passes their functions as unforced thunks and never applies them,
//!   so a dropped stage leaves no trace and needs no strictness gate
//!   (`length (map ⊥ xs)` fuses to a pure count, as GHC computes it).
//!   Elements are extracted UNFORCED unless a Filter remains (a strict
//!   P is then the demand, as in the lazy pipeline); with no remaining
//!   stage the loop just counts the source. The count starts at 0 and
//!   the result is a native Int.
//! * SUM — `sum` at Int/Number is the strict left fold with native `+`
//!   (GHC's list sum is the same left-associated walk; Int wraps like
//!   every native Int op, Number is IEEE addition in GHC's order). The
//!   zero literal matches the result type (0 / 0.0). Other element types
//!   (Integer reaches codegen as a named `add_Integer` fold instead)
//!   decline.
//! * NATIVE OPERANDS — every value the loop itself compares or operates
//!   on natively must be DELIVERED WHNF, because there is no callee left
//!   to force it: the range bounds, a take budget, and a Native step's
//!   initial accumulator bind through `site_forced_arg_ast` (a do-bound
//!   variable is otherwise a raw thunk — found by the fuzzer the moment
//!   sum/length made fused pipelines generatable). A Native step's
//!   ELEMENT operand is WHNF from the source (range integers, forced
//!   extraction) and through named map stages (the WHNF-return claim at
//!   these native-scalar types), but a lambda or local-function map can
//!   return a raw captured thunk, so the step forces after those.
//!   CallF needs none of this: F is gated strict in both parameters.
//! * CALLING CONVENTION — the N-ary eta-padding convention guarantees a
//!   function VALUE of type `a -> b` is callable flat with one argument
//!   (and `b -> a -> b` with two): emitted parameter lists are padded to
//!   the type's arrow count and call sites pass outstanding arguments in
//!   one flat call. The runtime generics (`map(f, xs)` calling
//!   `f(xs[1])`) already lean on exactly this. A partial application
//!   bound as a stage local is a closure the same convention covers.
//! * NAMES — Prelude names are not shadowable at top level, and
//!   `enumFromTo_Int` is compiler-namespaced, so only LOCAL shadowing is
//!   checked, on every matched name.
//! * RANGE EDGE — `while i <= hi … i = i + 1` reproduces
//!   `enumFromTo_Int`'s own arithmetic, including its behavior at
//!   maxBound (both diverge identically there — a pre-existing Prelude
//!   property fusion does not change).
//! * LEAF SOURCE — an arbitrary list expression is walked
//!   `__force`/`__mll_head`/`__mll_tail` like every runtime consumer;
//!   the element is forced at extraction exactly when the gated pipeline
//!   proves the lazy walk forced it (always for foldl'/sum; for length
//!   only when a filter remains). A bare `foldl' f z xs` with no stages
//!   has no intermediate structure to remove and stays on the general
//!   path.

use crate::tir::*;
use crate::types::Ty;
use super::CodeGen;
use super::function::{FnSpillScope, VarsSnapshot};
use super::lua::{Block, Expr, FuncBody, Stmt};

/// One pipeline stage. Collected outermost-first, applied source-first.
enum Stage<'a> {
    Map(&'a TExpr),
    Filter(&'a TExpr),
    Take(&'a TExpr),
}

enum Source<'a> {
    Range(&'a TExpr, &'a TExpr),
    Leaf(&'a TExpr),
}

/// What consumes the surviving elements.
enum Consumer<'a> {
    /// `foldl' F Z` — F called per element (or a native operator step).
    Fold { f: &'a TExpr, z: &'a TExpr },
    /// `sum` at Int/Number — native `+`, zero literal by type.
    Sum { float: bool },
    /// `length` — counts; element values undemanded.
    Length,
}

/// The per-element accumulator step.
enum StepKind {
    /// `a = __mll_fu_f(a, x)`
    CallF,
    /// `a = a <op> x` — a native scalar operator fold (`(+)` at Int).
    Native(&'static str),
    /// `a = a + 1` — length's count; the element value is unused.
    Count,
}

/// A stage's compiled form, source-first order. A map's bool records
/// whether its RESULT is provably WHNF: a module-level/prelude function
/// (spine-headed by a non-local Var) is covered by the WHNF-return claim
/// at these native-scalar element types; a lambda or a local function
/// value can return a raw captured thunk, so a NATIVE step after it must
/// force (CallF's fold forces both parameters itself).
enum StageLocal {
    Filter(String),
    Map(String, bool),
    /// A take budget counter (checked at loop top, decremented in place).
    Take(String),
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

/// Operators whose native Lua emission forces both operands.
fn strict_scalar_op(op: &str) -> bool {
    matches!(op, "+" | "-" | "*" | "/" | "^" | "div" | "mod" | "rem" | "quot"
        | "==" | "/=" | "<" | ">" | "<=" | ">=")
}

/// Is `ty` a scalar the native operators handle directly?
fn native_scalar(ty: &Ty) -> bool {
    matches!(ty, Ty::Con(n) if n == "Int" || n == "Number" || n == "Double")
}

/// Peel `n` arrows off a function type; the argument types peeled.
fn arrow_args(ty: &Ty, n: usize) -> Option<Vec<&Ty>> {
    let mut out = Vec::new();
    let mut cur = ty;
    for _ in 0..n {
        let Ty::Arrow(a, b, _) = cur else { return None };
        out.push(a.as_ref());
        cur = b.as_ref();
    }
    Some(out)
}

impl CodeGen {
    /// The SOURCE name a call-head Var denotes: the mono specialization's
    /// origin (`foldl'_IntT…` -> `foldl'`) or the name itself.
    fn fn_origin<'a>(&'a self, name: &'a str) -> &'a str {
        self.spec_origins.get(name).map(String::as_str).unwrap_or(name)
    }

    /// The demand row of the named function `n` visible here, if any.
    fn name_strict_row(&self, n: &str) -> Option<&Vec<bool>> {
        self.local_strict_params.get(n).or_else(|| {
            if self.is_local_shadowed(n) {
                None
            } else {
                self.demand_info.strict_params.get(n)
            }
        })
    }

    /// Does evaluating `e` to WHNF force the variable `p` on every path?
    /// Conservative and syntactic: false when unsure. Used to judge a
    /// lambda's parameter strictness (sections desugar to lambdas).
    fn forces_param(&self, e: &TExpr, p: &str) -> bool {
        match &e.kind {
            TExprKind::Paren(i) | TExprKind::Negate(i) => self.forces_param(i, p),
            TExprKind::Var(n) => n == p,
            TExprKind::InfixApp { op, lhs, rhs } => {
                if strict_scalar_op(op) || op == "seq" {
                    self.forces_param(lhs, p) || self.forces_param(rhs, p)
                } else if matches!(op.as_str(), "&&" | "||" | "++") {
                    self.forces_param(lhs, p)
                } else {
                    false
                }
            }
            TExprKind::If { cond, then_branch, else_branch } => {
                self.forces_param(cond, p)
                    || (self.forces_param(then_branch, p)
                        && self.forces_param(else_branch, p))
            }
            // Branch bodies bind pattern variables; judging only the
            // scrutinee stays sound without scope bookkeeping.
            TExprKind::Case { scrutinee, .. } => self.forces_param(scrutinee, p),
            TExprKind::Let { binds, body } => {
                if binds.iter().any(|b| b.name == p) {
                    false // shadowed; the outer p is not this p
                } else {
                    self.forces_param(body, p)
                }
            }
            TExprKind::App(..) => {
                let (h, sp) = spine(e);
                if let TExprKind::Var(n) = &h.kind {
                    if n == p {
                        return true; // calling p forces p itself
                    }
                    // A saturated call of a function with a demand row:
                    // its strict positions are forced.
                    if let Some(row) = self.name_strict_row(n)
                        && row.len() == sp.len()
                    {
                        return sp
                            .iter()
                            .zip(row.iter())
                            .any(|(a, s)| *s && self.forces_param(a, p));
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// The strictness row of the function VALUE `e` at `arity` remaining
    /// parameters, when provable: a named function's demand row (its tail
    /// after any partially applied arguments), a first-class operator at
    /// native scalar types, or a lambda judged by `forces_param`.
    fn fn_value_strictness(&self, e: &TExpr, arity: usize) -> Option<Vec<bool>> {
        let e = strip_parens(e);
        if let TExprKind::Lambda { params, body } = &e.kind {
            if params.len() != arity {
                return None;
            }
            return Some(
                params.iter().map(|(p, _)| self.forces_param(body, p)).collect(),
            );
        }
        if let TExprKind::OpFunc(op) = &e.kind {
            if strict_scalar_op(op)
                && arrow_args(&e.ty, arity)
                    .is_some_and(|args| args.iter().all(|t| native_scalar(t)))
            {
                return Some(vec![true; arity]);
            }
            return None;
        }
        // A composed value `f . g` used directly as a stage: strict as a
        // unary value when both sides are (matching the demand analyzer's
        // composition rule for the applied spelling).
        if let TExprKind::InfixApp { op, lhs, rhs } = &e.kind
            && op == "."
            && arity == 1
        {
            return (self.fn_value_strictness(lhs, 1) == Some(vec![true])
                && self.fn_value_strictness(rhs, 1) == Some(vec![true]))
            .then(|| vec![true]);
        }
        let (h, sp) = spine(e);
        if let TExprKind::Var(n) = &h.kind
            && let Some(row) = self.name_strict_row(n)
            && row.len() == sp.len() + arity
        {
            return Some(row[sp.len()..].to_vec());
        }
        None
    }

    pub(super) fn try_fused_list_pipeline(
        &mut self,
        f: &TExpr,
        args: &[&TExpr],
    ) -> Option<Expr> {
        let TExprKind::Var(name) = &f.kind else { return None };
        if self.is_local_shadowed(name) {
            return None;
        }
        let consumer = match (self.fn_origin(name), args.len()) {
            ("foldl'", 3) => Consumer::Fold { f: args[0], z: args[1] },
            ("sum", 1) => {
                // The element/result type: sum :: [a] -> a.
                let Ty::Arrow(_, ret, _) = &f.ty else { return None };
                if !native_scalar(ret) {
                    return None;
                }
                Consumer::Sum {
                    float: !matches!(ret.as_ref(), Ty::Con(n) if n == "Int"),
                }
            }
            ("length", 1) => Consumer::Length,
            _ => return None,
        };

        // The fold function's step: a named/lambda value called per
        // element, or a native operator applied in place. Every gate
        // failure declines to the general path.
        let step = match &consumer {
            Consumer::Fold { f: fold_f, .. } => {
                let ff = strip_parens(fold_f);
                if let TExprKind::OpFunc(op) = &ff.kind
                    && matches!(op.as_str(), "+" | "-" | "*")
                    && arrow_args(&ff.ty, 2)
                        .is_some_and(|a| a.iter().all(|t| native_scalar(t)))
                {
                    // (+)/(-)/(*) at native scalars: strict both, and the
                    // step needs no function value at all.
                    match op.as_str() {
                        "+" => StepKind::Native("+"),
                        "-" => StepKind::Native("-"),
                        _ => StepKind::Native("*"),
                    }
                } else {
                    let row = self.fn_value_strictness(fold_f, 2)?;
                    if !(row[0] && row[1]) {
                        return None;
                    }
                    StepKind::CallF
                }
            }
            Consumer::Sum { .. } => StepKind::Native("+"),
            Consumer::Length => StepKind::Count,
        };

        // Collect stages outermost-first down to the source, gating every
        // map/filter function's strictness as they appear.
        let mut stages: Vec<Stage> = Vec::new();
        let mut chain = match &consumer {
            Consumer::Fold { .. } => args[2],
            _ => args[0],
        };
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
                    if n == "take" && sp.len() == 2 && !self.is_local_shadowed(n) =>
                {
                    stages.push(Stage::Take(sp[0]));
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

        // length demands no element values: a Map with no Filter outside
        // it is DROPPED from the loop entirely — the lazy pipeline passes
        // its function as an unforced thunk and never applies it, so the
        // dropped stage leaves no trace (not even an evaluation of its
        // function expression), and it needs no strictness gate.
        if matches!(consumer, Consumer::Length) {
            let mut seen_filter = false;
            let mut kept = Vec::new();
            for st in stages {
                match st {
                    Stage::Map(_) if !seen_filter => {}
                    Stage::Filter(_) => {
                        seen_filter = true;
                        kept.push(st);
                    }
                    other => kept.push(other),
                }
            }
            stages = kept;
            if stages.is_empty() && matches!(source, Source::Leaf(_)) {
                // A pure spine count of a leaf is what the runtime length
                // already does; nothing to deforest.
                return None;
            }
        }
        // Gate every REMAINING stage function (see SOUNDNESS: a lazy G or
        // P would leave elements the loop computes undemanded).
        for st in &stages {
            match st {
                Stage::Map(g) | Stage::Filter(g) => {
                    if self.fn_value_strictness(g, 1) != Some(vec![true]) {
                        return None;
                    }
                }
                Stage::Take(_) => {}
            }
        }
        // For foldl'/sum the gated pipeline forces every element; for
        // length only a remaining filter's strict P demands them.
        let force_elem = !matches!(consumer, Consumer::Length)
            || stages.iter().any(|s| matches!(s, Stage::Filter(_)));

        // ---- emission (an IIFE: its own Lua function scope) ----
        let scope = VarsSnapshot::capture(self);
        let spill = FnSpillScope::enter(self);
        let mut stmts: Vec<Stmt> = Vec::new();

        // Bind F, Z and the stage functions once, in the original
        // left-to-right evaluation order of the nested calls.
        match &consumer {
            Consumer::Fold { f: fold_f, z } => {
                if matches!(step, StepKind::CallF) {
                    let f_e = self.forced_ast(fold_f);
                    stmts.push(Stmt::Local(vec!["__mll_fu_f".into()], Some(f_e)));
                }
                // CallF's F forces the accumulator itself (row[0]); a
                // NATIVE step uses it raw in an operator, so it must
                // arrive WHNF (a do-bound `z` is a thunk otherwise).
                let z_e = if matches!(step, StepKind::CallF) {
                    self.arg_ast(z, true)
                } else {
                    self.site_forced_arg_ast(z)
                };
                stmts.push(Stmt::Local(vec!["__mll_fu_a".into()], Some(z_e)));
            }
            Consumer::Sum { float } => {
                let zero = if *float { "0.0" } else { "0" };
                stmts.push(Stmt::Local(
                    vec!["__mll_fu_a".into()],
                    Some(Expr::lit(zero)),
                ));
            }
            Consumer::Length => {
                stmts.push(Stmt::Local(
                    vec!["__mll_fu_a".into()],
                    Some(Expr::lit("0")),
                ));
            }
        }
        let mut stage_locals: Vec<StageLocal> = Vec::new();
        let mut take_counters: Vec<String> = Vec::new();
        for (i, st) in stages.iter().enumerate() {
            match st {
                Stage::Map(g) => {
                    let n = format!("__mll_fu_s{i}");
                    let whnf_result = {
                        let (h, _) = spine(strip_parens(g));
                        matches!(&h.kind, TExprKind::Var(v)
                            if !self.is_local_shadowed(v))
                    };
                    let fe = self.forced_ast(g);
                    stmts.push(Stmt::Local(vec![n.clone()], Some(fe)));
                    stage_locals.push(StageLocal::Map(n, whnf_result));
                }
                Stage::Filter(p) => {
                    let n = format!("__mll_fu_s{i}");
                    let fe = self.forced_ast(p);
                    stmts.push(Stmt::Local(vec![n.clone()], Some(fe)));
                    stage_locals.push(StageLocal::Filter(n));
                }
                Stage::Take(ne) => {
                    let n = format!("__mll_fu_k{i}");
                    // The budget is compared and decremented natively:
                    // it must arrive WHNF (a do-bound count is a thunk).
                    let e = self.site_forced_arg_ast(ne);
                    stmts.push(Stmt::Local(vec![n.clone()], Some(e)));
                    take_counters.push(n.clone());
                    stage_locals.push(StageLocal::Take(n));
                }
            }
        }

        // The per-element body: stages applied source-first (reverse of the
        // collected outermost-first order), the accumulator step innermost.
        // A map binds a fresh element local; a filter nests everything
        // after it; a take decrements its budget (exhaustion is checked at
        // the loop top).
        fn step_stmt(step: &StepKind, cur: Expr, cur_whnf: bool) -> Stmt {
            match step {
                StepKind::CallF => Stmt::Assign(
                    "__mll_fu_a".into(),
                    Expr::call_named(
                        "__mll_fu_f",
                        vec![Expr::name("__mll_fu_a"), cur],
                    ),
                ),
                // The native operator needs a WHNF operand; a lambda map
                // stage's result can be a raw captured thunk.
                StepKind::Native(op) => Stmt::Assign(
                    "__mll_fu_a".into(),
                    Expr::binop(
                        *op,
                        Expr::name("__mll_fu_a"),
                        if cur_whnf { cur } else { Expr::force(cur) },
                    ),
                ),
                StepKind::Count => Stmt::Assign(
                    "__mll_fu_a".into(),
                    Expr::binop("+", Expr::name("__mll_fu_a"), Expr::lit("1")),
                ),
            }
        }
        fn build_body(
            source_first: &[StageLocal],
            cur: Expr,
            cur_whnf: bool,
            next_x: &mut usize,
            step: &StepKind,
        ) -> Vec<Stmt> {
            match source_first.split_first() {
                None => vec![step_stmt(step, cur, cur_whnf)],
                Some((StageLocal::Filter(n), rest)) => vec![Stmt::If {
                    cond: Expr::call_named(n, vec![cur.clone()]),
                    then_b: Block(build_body(rest, cur, cur_whnf, next_x, step)),
                    elseifs: vec![],
                    else_b: None,
                }],
                Some((StageLocal::Map(n, whnf_result), rest)) => {
                    let xn = format!("__mll_fu_x{next_x}");
                    *next_x += 1;
                    let mut out = vec![Stmt::Local(
                        vec![xn.clone()],
                        Some(Expr::call_named(n, vec![cur])),
                    )];
                    out.extend(build_body(
                        rest,
                        Expr::name(xn),
                        *whnf_result,
                        next_x,
                        step,
                    ));
                    out
                }
                Some((StageLocal::Take(n), rest)) => {
                    let mut out = vec![Stmt::Assign(
                        n.clone(),
                        Expr::binop("-", Expr::name(n), Expr::lit("1")),
                    )];
                    out.extend(build_body(rest, cur, cur_whnf, next_x, step));
                    out
                }
            }
        }
        let source_first: Vec<StageLocal> =
            stage_locals.into_iter().rev().collect();
        let mut next_x = 0usize;
        // The initial element is WHNF for a range (a native integer) and
        // for a forced leaf extraction; length's unforced extraction never
        // feeds a Native step (its step is Count).
        let body = build_body(
            &source_first,
            Expr::name("__mll_fu_x"),
            true,
            &mut next_x,
            &step,
        );

        // Every take's exhaustion check runs FIRST, before the source is
        // advanced or a cell is forced: a spent budget means the lazy
        // pipeline stopped pulling entirely.
        let mut loop_head: Vec<Stmt> = take_counters
            .iter()
            .map(|k| Stmt::If {
                cond: Expr::binop("<=", Expr::name(k), Expr::lit("0")),
                then_b: Block(vec![Stmt::Return(Expr::name("__mll_fu_a"))]),
                elseifs: vec![],
                else_b: None,
            })
            .collect();

        // The source loop. Exit is `return` from inside — WhileTrue's
        // divergence contract — so no statement follows the loop.
        match source {
            Source::Range(a, b) => {
                // Compared and incremented natively: delivered WHNF (the
                // general enumFromTo_Int forces its bounds at entry).
                let a_e = self.site_forced_arg_ast(a);
                let b_e = self.site_forced_arg_ast(b);
                stmts.push(Stmt::Local(vec!["__mll_fu_i".into()], Some(a_e)));
                stmts.push(Stmt::Local(vec!["__mll_fu_hi".into()], Some(b_e)));
                let mut loop_body = std::mem::take(&mut loop_head);
                loop_body.push(Stmt::If {
                    cond: Expr::binop(
                        ">",
                        Expr::name("__mll_fu_i"),
                        Expr::name("__mll_fu_hi"),
                    ),
                    then_b: Block(vec![Stmt::Return(Expr::name("__mll_fu_a"))]),
                    elseifs: vec![],
                    else_b: None,
                });
                loop_body.push(Stmt::Local(
                    vec!["__mll_fu_x".into()],
                    Some(Expr::name("__mll_fu_i")),
                ));
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
                let mut loop_body = std::mem::take(&mut loop_head);
                loop_body.push(Stmt::If {
                    cond: Expr::binop(
                        "==",
                        Expr::name("__mll_fu_l"),
                        Expr::lit("nil"),
                    ),
                    then_b: Block(vec![Stmt::Return(Expr::name("__mll_fu_a"))]),
                    elseifs: vec![],
                    else_b: None,
                });
                // Forced exactly when the gated pipeline proves the lazy
                // walk forced it (see SOUNDNESS: LENGTH).
                let head = Expr::call_named(
                    "__mll_head",
                    vec![Expr::name("__mll_fu_l")],
                );
                let extract = if force_elem { Expr::force(head) } else { head };
                loop_body.push(Stmt::Local(
                    vec!["__mll_fu_x".into()],
                    Some(extract),
                ));
                loop_body.extend(body);
                // A budget spent by THIS element means the lazy pipeline
                // never pulls another cell: re-check before the tail
                // advance, which would force the next cell's suspension
                // (`take 2 (filter odd (1 : 3 : ⊥))` must not touch ⊥).
                // The range source needs no such check — `i + 1` cannot
                // bottom, and the next iteration's top check returns.
                for k in &take_counters {
                    loop_body.push(Stmt::If {
                        cond: Expr::binop("<=", Expr::name(k), Expr::lit("0")),
                        then_b: Block(vec![Stmt::Return(Expr::name(
                            "__mll_fu_a",
                        ))]),
                        elseifs: vec![],
                        else_b: None,
                    });
                }
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
