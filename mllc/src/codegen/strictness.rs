//! Strictness and cheapness predicates: when eager evaluation is sound.
//!
//! Two notions share one structural walk (`is_cheap_with`): `is_cheap`
//! ("small to duplicate or evaluate" — inlining and the call-site protocol)
//! and `is_cheap_to_force` ("evaluating now cannot force a suspended,
//! possibly-bottom computation" — a variable qualifies only when provably
//! WHNF via `concrete_vars`). `contains_trapping_op` keeps trapping
//! operators (integer `div`/`mod` on a zero divisor) out of eager positions.
//! `demanded_bindings` / `clause_demanded` implement let-to-case
//! eagerization: a binding provably demanded by the clause body may be
//! assigned strictly, decided per binding by `strict_binding_ok` together
//! with the free functions `strict_binding_safe` and `bare_var_alias`.

use crate::tir::*;
use super::CodeGen;
use super::names::{is_builtin_op, sanitize_name};
use super::util::{expr_references_name};

impl CodeGen {
    /// Structural cheapness walk: is `expr` a small expression (variables,
    /// literals, constructor applications, Lua-native operators over cheap
    /// operands, …) that can be evaluated eagerly without allocating a
    /// memoizing thunk — as opposed to an expensive computation such as a
    /// user function call, which stays lazy?
    ///
    /// `var_ok` decides whether a variable reference counts as cheap. The
    /// structural walk is shared between two notions of cheapness:
    /// - `is_cheap` (var_ok = always): "small to duplicate/evaluate" — used
    ///   for inlining decisions and the call-site protocol, where every
    ///   emission path forces variables anyway.
    /// - `is_cheap_to_force` (var_ok = provably WHNF): "safe to evaluate
    ///   eagerly" — evaluating the expression now cannot force a suspended
    ///   computation, so it cannot raise or diverge where GHC would not.
    ///
    /// `prim_ok` is the local-shadow question for a saturated primitive
    /// typeclass method application (`eq_Int a b`, `ord_gt__Int a b`, …):
    /// such an App emits as one native Lua operator over forced operands
    /// (try_primitive_method_app) — the App spelling of exactly the
    /// comparisons the InfixApp arm already counts cheap — but ONLY when
    /// no local binding shadows the method name; shadowed, it is an
    /// arbitrary call. `is_cheap_to_force` answers with
    /// `is_local_shadowed`, matching the emitter; `is_cheap` passes true
    /// (its consumers weigh size/duplication, where the pathological
    /// shadow only costs precision, and inline-candidate bodies at module
    /// scope have no locals to shadow with); the context-free floor
    /// passes false (the demand mirror must UNDER-claim eagerness — the
    /// drift bugs were all over-claims).
    pub(crate) fn is_cheap_with(
        expr: &TExpr,
        var_ok: &dyn Fn(&str) -> bool,
        prim_ok: &dyn Fn(&str) -> bool,
    ) -> bool {
        match &expr.kind {
            TExprKind::Lit(_) | TExprKind::Con(_)
            | TExprKind::Lambda { .. } | TExprKind::OpFunc(_) => true,
            // A dictionary-parameter method read: dictionaries are total
            // constructions (method tables of function values, never
            // thunked — DictCall emits its dict args eagerly on the same
            // ground), so the field read is WHNF-and-pure. Left lazy, the
            // thunk wrapper reached runtime helpers that call the method
            // value directly (`__mll_list_eq(elem_eq, …)` — a table where
            // a function is expected).
            TExprKind::DictAccess { .. } => true,
            TExprKind::Var(name) => var_ok(name),
            TExprKind::Paren(inner) | TExprKind::Negate(inner) => Self::is_cheap_with(inner, var_ok, prim_ok),
            TExprKind::Tuple(elems) => elems.iter().all(|e| Self::is_cheap_with(e, var_ok, prim_ok)),
            TExprKind::InfixApp { op, lhs, rhs } => {
                // `$` IS application: `f $ x` calls f, so it is neither small
                // to duplicate nor safe to evaluate eagerly — a possibly-⊥
                // application must stay lazy exactly like the App arm below
                // (an unused `where u = f $ 0` with a diverging `f` must
                // never run; GHC does not touch it). It is listed in
                // is_builtin_op for emission dispatch, which is a different
                // question from cheapness. The one cheap `$` shape mirrors
                // the App arm's constructor case: a constructor applied
                // through `$` (`Just $ x`) is just table creation.
                if op == "$" {
                    return Self::is_con_app(lhs)
                        && Self::is_cheap_with(lhs, var_ok, prim_ok)
                        && Self::is_cheap_with(rhs, var_ok, prim_ok);
                }
                // Builtin ops (arithmetic, comparison, concat) are cheap
                // if their operands are cheap. `.` stays: composition only
                // BUILDS a closure — no operand is called.
                is_builtin_op(op) && Self::is_cheap_with(lhs, var_ok, prim_ok) && Self::is_cheap_with(rhs, var_ok, prim_ok)
            }
            TExprKind::App(func, arg) => {
                // Constructor applications are cheap (just table creation).
                // General function applications are NOT cheap — the function
                // body might be expensive even if the args are cheap.
                if Self::is_con_app(expr) {
                    return Self::is_cheap_with(arg, var_ok, prim_ok)
                        && Self::is_cheap_with(func, var_ok, prim_ok);
                }
                // A saturated primitive typeclass method — the App spelling
                // (`ord_gt__Int x 5` for `x > 5` at Int) of the comparisons
                // the InfixApp arm above already counts cheap; it emits as
                // one native Lua operator over forced operands
                // (try_primitive_method_app), gated by `prim_ok` on the
                // same shadow question the emitter asks.
                let mut h: &TExpr = expr;
                let mut argc = 0usize;
                loop {
                    match &h.kind {
                        TExprKind::App(f2, _) => {
                            argc += 1;
                            h = f2.as_ref();
                        }
                        TExprKind::Paren(i) => h = i.as_ref(),
                        _ => break,
                    }
                }
                if argc == 2
                    && let TExprKind::Var(name) = &h.kind
                    && super::names::primitive_method_lua_op(name).is_some()
                    && prim_ok(name)
                    && let TExprKind::App(inner, arg1) = &func.kind
                {
                    // Exactly App(App(Var, arg1), arg): judge both operands.
                    let _ = inner;
                    return Self::is_cheap_with(arg1, var_ok, prim_ok)
                        && Self::is_cheap_with(arg, var_ok, prim_ok);
                }
                // Saturated Prelude `not` → `(operand == false)` native
                // boolean (try_native_not_app), same prim_ok shadow gate.
                if argc == 1
                    && let TExprKind::Var(name) = &h.kind
                    && name == "not"
                    && prim_ok(name)
                {
                    return Self::is_cheap_with(arg, var_ok, prim_ok);
                }
                false
            }
            TExprKind::If { cond, then_branch, else_branch } => {
                Self::is_cheap_with(cond, var_ok, prim_ok)
                    && Self::is_cheap_with(then_branch, var_ok, prim_ok)
                    && Self::is_cheap_with(else_branch, var_ok, prim_ok)
            }
            // Function calls, case, let — potentially expensive, thunk them
            _ => false,
        }
    }

    /// "Small to duplicate/evaluate": `is_cheap_with` with every variable
    /// counted cheap (see there for the two notions).
    pub(super) fn is_cheap(expr: &TExpr) -> bool {
        Self::is_cheap_with(expr, &|_| true, &|_| true)
    }

    /// True when evaluating `expr` right now is *sound*, not just cheap:
    /// it cannot force a suspended (possibly bottom) computation.
    ///
    /// Cheap-eagerness (Faxén-style) is only valid for expressions that
    /// cannot fail or diverge. A bare `Var` does not qualify by itself: a
    /// variable can be bound to a thunk of `error`/an infinite loop, and
    /// expr_ast emits `__force(v)` for non-concrete variables — so eagerly
    /// evaluating `y + 1` forces `y` even though the binding was never
    /// demanded (GHC would never touch it). A variable only qualifies when
    /// its referent is provably WHNF, which is exactly the `concrete_vars`
    /// set: pattern-bound variables (forced by the match), demand-analysis
    /// strict parameters (forced at entry), do-block bind results, top-level
    /// functions, and prior bindings that were themselves assigned strictly
    /// under this same rule (so WHNF-ness propagates transitively through a
    /// binding group).
    pub(super) fn is_cheap_to_force(&self, expr: &TExpr) -> bool {
        // An Integer literal conversion emits as a read of its interned
        // `__mll_biglit[N]` CAF (see integer_lit_app) — a table index of an
        // immutable load-time value, as cheap and total as the literal it
        // denotes. Top-level check only: a conversion nested deeper inside
        // a non-cheap expression cannot rescue that expression anyway.
        if self.integer_lit_app(expr).is_some() {
            return true;
        }
        Self::is_cheap_with(
            expr,
            &|name| {
                // Prelude `otherwise` is the literal `true` — unless a local
                // binder shadows it, in which case it is an ordinary (possibly
                // thunked) variable like any other.
                (name == "otherwise" && !self.is_local_shadowed(name))
                    || self.concrete_vars.contains(&sanitize_name(name))
            },
            &|name| !self.is_local_shadowed(name),
        ) && !Self::contains_trapping_op(expr)
    }

    /// The decimal string of an Integer-literal conversion: `expr` (parens
    /// stripped) is the typechecker's `fromInteger_Integer` applied to one
    /// integer literal, and the name still refers to the runtime conversion
    /// (no local binder or user definition has taken it — either would make
    /// this an ordinary call whose eager evaluation could run user code).
    /// Both the emission intercept (the biglit-pool read) and
    /// `is_cheap_to_force` key on this one judgment.
    pub(super) fn integer_lit_app(&self, expr: &TExpr) -> Option<String> {
        let mut e = expr;
        while let TExprKind::Paren(p) = &e.kind {
            e = p.as_ref();
        }
        let TExprKind::App(func, arg) = &e.kind else { return None };
        let mut f = func.as_ref();
        while let TExprKind::Paren(p) = &f.kind {
            f = p.as_ref();
        }
        let TExprKind::Var(name) = &f.kind else { return None };
        if name != "fromInteger_Integer"
            || self.is_local_shadowed(name)
            || self.module_fn_names.contains(name.as_str())
        {
            return None;
        }
        let mut a = arg.as_ref();
        while let TExprKind::Paren(p) = &a.kind {
            a = p.as_ref();
        }
        match &a.kind {
            TExprKind::Lit(TLiteral::Integer(n)) => Some(n.to_string()),
            // Already pooled by literal_ast on its own, but routing the
            // whole conversion through one slot skips the wrapper call too.
            TExprKind::Lit(TLiteral::BigInteger(s)) => Some(s.clone()),
            _ => None,
        }
    }

    /// True when `expr` (already known to be structurally cheap) contains a
    /// built-in operator that can *trap* at runtime — integer `div`/`mod`/`%`
    /// raise a Lua error on a zero divisor (`__mll_div`/`__mll_mod` check the
    /// divisor and raise "divide by zero" explicitly). Such an expression can be
    /// ⊥ even though every operand is a plain value, so it is never safe to
    /// evaluate eagerly in a non-strict position: bottom always weighs
    /// maximally on the laziness side (see the weighing in arg_ast). Float `/`
    /// is deliberately excluded — `1/0` is `inf`, matching Haskell's `Double`,
    /// not an error.
    pub(super) fn contains_trapping_op(expr: &TExpr) -> bool {
        match &expr.kind {
            TExprKind::InfixApp { op, lhs, rhs } => {
                // The integer division family's ONE trap is the zero
                // divisor (Lua's own integer division wraps MIN/-1
                // silently, and __mll_div/__mll_mod raise only on zero) —
                // so a NONZERO integer-literal divisor cannot trap, and
                // `i mod 2000 + 1` is as safe to evaluate eagerly as
                // `i + 1`. This mirrors the emission rule that lowers
                // mod-by-nonzero-literal to native `%`; without it every
                // such expression was thunked in a lazy argument position
                // (a closure per loop iteration on the hm_churn lookup
                // path).
                let trapping_divisor = matches!(op.as_str(),
                        "div" | "mod" | "quot" | "rem" | "%")
                    && {
                        let mut r = rhs.as_ref();
                        while let TExprKind::Paren(p) = &r.kind {
                            r = p.as_ref();
                        }
                        !matches!(&r.kind,
                            TExprKind::Lit(TLiteral::Integer(n)) if *n != 0)
                    };
                trapping_divisor
                    || Self::contains_trapping_op(lhs)
                    || Self::contains_trapping_op(rhs)
            }
            TExprKind::Paren(inner) | TExprKind::Negate(inner) => Self::contains_trapping_op(inner),
            TExprKind::Tuple(elems) => elems.iter().any(Self::contains_trapping_op),
            TExprKind::App(func, arg) => {
                Self::contains_trapping_op(func) || Self::contains_trapping_op(arg)
            }
            TExprKind::If { cond, then_branch, else_branch } => {
                Self::contains_trapping_op(cond)
                    || Self::contains_trapping_op(then_branch)
                    || Self::contains_trapping_op(else_branch)
            }
            _ => false,
        }
    }

    /// Names of value bindings in `binds` that are provably demanded when
    /// the binding group's body is evaluated (seeded with the body's demand
    /// set, closed transitively through the RHSes of demanded siblings).
    ///
    /// A demanded binding will be forced anyway, so evaluating it eagerly at
    /// binding time is sound even when it can raise or diverge — the same
    /// bottom merely surfaces at binding time instead of at first use. This
    /// is the eagerization GHC's own demand analysis performs (let-to-case),
    /// and it is what keeps hot-loop bindings strict without the unsound
    /// "every Var is cheap" rule (see is_cheap_to_force).
    pub(super) fn demanded_bindings(
        &self,
        binds: &[TLocalDef],
        seed: crate::demand::DemandMap,
    ) -> std::collections::HashSet<String> {
        let inlined = |n: &str| self.inline_fns.contains_key(n);
        let mut demanded = seed;
        // Re-walk a binding when its own demand deepens (a later sibling can
        // raise e.g. a Head demand to an element demand). Terminates: demands
        // only deepen and the lattice is finite for a finite program.
        let mut walked: std::collections::HashMap<String, crate::demand::Demand> =
            std::collections::HashMap::new();
        loop {
            let mut changed = false;
            for b in binds {
                if b.patterns.is_empty()
                    && let Some(d) = demanded.get(&b.name).cloned() {
                        let redo = match walked.get(&b.name) {
                            Some(prev) => !prev.subsumes(&d),
                            None => true,
                        };
                        if redo {
                            walked.insert(b.name.clone(), d.clone());
                            let m = crate::demand::demanded_map(
                                &b.body,
                                &self.demand_info.rows,
                                &self.local_demand_rows,
                                &inlined,
                                &|n| self.is_local_shadowed(n),
                                &d,
                            );
                            crate::demand::map_join(&mut demanded, m);
                            changed = true;
                        }
                    }
            }
            if !changed {
                break;
            }
        }
        demanded.into_keys().collect()
    }

    /// The structured local-function rows in scope for `clause`: the
    /// current scope's rows with every where-bound NAME shadowed first (a
    /// sibling value binding must not inherit an enclosing scope's row —
    /// same discipline where_binds_stmts applies to local_strict_params),
    /// extended with the rows of the clause's own where-bound function
    /// groups (see demand::local_fn_rows).
    pub(super) fn clause_local_rows(
        &self,
        clause: &TClause,
    ) -> std::collections::HashMap<String, crate::demand::LocalRows> {
        let inlined = |n: &str| self.inline_fns.contains_key(n);
        let mut env = self.local_demand_rows.clone();
        for b in &clause.where_binds {
            env.remove(&b.name);
        }
        env.extend(crate::demand::local_fn_rows(
            &self.demand_info.rows,
            &inlined,
            &|n| self.is_local_shadowed(n),
            clause,
        ));
        env
    }

    /// Demand seed for a clause's where bindings: the variables the emitted
    /// code for the clause body (or its guards) forces when evaluated. The
    /// result demand is the current function's (deep only when the
    /// whole-program analysis proved every call site applies it).
    ///
    /// Computes the clause's local rows itself (rather than reading
    /// local_demand_rows) because every caller evaluates this seed BEFORE
    /// where_binds_stmts opens the clause's where scope.
    pub(super) fn clause_demanded(&self, clause: &TClause) -> crate::demand::DemandMap {
        let inlined = |n: &str| self.inline_fns.contains_key(n);
        let locals = self.clause_local_rows(clause);
        // The seed is computed BEFORE where_binds_stmts registers the
        // clause's where names as locals, so the ambient predicate alone
        // misses them (and the clause's own pattern/inner binders): extend
        // it with the clause-level shadow set — the structured twin of the
        // boolean analysis's own_shadowed (see demand::clause_shadow_set).
        let own = crate::demand::clause_shadow_set(clause, &locals);
        let shadowed = |n: &str| own.contains(n) || self.is_local_shadowed(n);
        if clause.guards.is_empty() {
            crate::demand::demanded_map(
                clause.plain_body(),
                &self.demand_info.rows,
                &locals,
                &inlined,
                &shadowed,
                &self.cur_result_demand,
            )
        } else {
            crate::demand::demanded_map_guards(
                &clause.guards,
                &self.demand_info.rows,
                &locals,
                &inlined,
                &shadowed,
                &self.cur_result_demand,
            )
        }
    }

    /// Whether a value binding may be assigned strictly (evaluated eagerly):
    /// either evaluating it cannot force a suspended computation at all
    /// (is_cheap_to_force), or the binding is provably demanded by the
    /// group's body — eager evaluation then only reorders a force that
    /// happens regardless — and its RHS is structurally cheap (so we still
    /// never eagerly run an expensive computation at binding time).
    pub(super) fn strict_binding_ok(
        &self,
        bind: &TLocalDef,
        demanded: &std::collections::HashSet<String>,
    ) -> bool {
        self.is_cheap_to_force(&bind.body)
            || (demanded.contains(&bind.name) && Self::is_cheap(&bind.body))
    }

    /// Bindings of a let/where group that may be evaluated eagerly with
    /// EXACT bottom identity — the generalization of
    /// try_let_seq_strict_ast's seq shape to first-force chains.
    /// strict_binding_ok's demanded+cheap rule reorders only bottom-free
    /// computation; this set admits EXPENSIVE RHSes (persistent-map
    /// writes, Integer arithmetic) by proving the lazy program forces the
    /// binding before any other bottom could surface, so evaluating the
    /// RHS at binding time runs the same computation at the same program
    /// point and bottom identity is preserved exactly (the discipline the
    /// general Let arm documents: never surface a different bottom than
    /// unoptimized GHC, the goldens oracle).
    ///
    /// The chain: the group body's evaluation must force a candidate as
    /// its FIRST possibly-bottoming act (walking case/if scrutinees and
    /// the strict positions of entry-forcing callees —
    /// demand::entry_forced_mask, a stronger contract than the strictness
    /// rows, whose "forced during the run" admits `f a = case loop of _ ->
    /// a`); that binding's RHS may then force a sibling the same way, and
    /// so on. Membership is independent of whether an intermediate link is
    /// itself emitted eagerly: a thunked link's RHS still runs, unchanged,
    /// at the moment the proven first force reaches it — eagerization only
    /// ever moves a force earlier past provably-bottom-free work (sibling
    /// thunk allocations, the group's own declarations).
    ///
    /// Callers must still apply strict_binding_safe (a forward reference
    /// reads a nil slot regardless of demand). All three Let emitters
    /// consult this set — the value Let arm (expr.rs), the where path
    /// (function.rs), and bind_chain_block's Let arm (action.rs, which
    /// clause bodies route through): in each, the group body's emission
    /// is the very next thing after the binding group, so the anchor
    /// argument is the same. A body that continues an ACTION chain
    /// (`>>=`/`>>`) declines in the walker (InfixApp is not an anchor),
    /// and action-typed anchors decline on their type.
    pub(super) fn exact_demanded_bindings(
        &self,
        binds: &[TLocalDef],
        body: &TExpr,
    ) -> std::collections::HashSet<String> {
        use std::collections::HashSet;
        // Candidates: uniquely named value bindings whose name is not
        // rebound by any inner construct in the walk (a rebound name would
        // make the walker's Var test resolve to the wrong binder) and
        // whose RHS is not an action value (those emit as re-performable
        // closures, never eager values).
        let mut rebound = HashSet::new();
        crate::demand::collect_rebound_names(body, &mut rebound);
        for b in binds {
            crate::demand::collect_rebound_names(&b.body, &mut rebound);
        }
        let mut counts: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for b in binds {
            *counts.entry(b.name.as_str()).or_default() += 1;
        }
        let candidates: HashSet<&str> = binds
            .iter()
            .filter(|b| {
                b.patterns.is_empty()
                    && counts[b.name.as_str()] == 1
                    && !rebound.contains(&b.name)
                    && !Self::is_nullary_action_type(&b.body.ty)
            })
            .map(|b| b.name.as_str())
            .collect();
        if candidates.is_empty() {
            return HashSet::new();
        }
        let mut exact: HashSet<String> = HashSet::new();
        let mut frontier = self.first_forced_candidate(body, &candidates, false);
        while let Some(x) = frontier {
            if !exact.insert(x.clone()) {
                break; // self-sustaining chain: stop
            }
            let bind = binds
                .iter()
                .find(|b| b.name == x)
                .expect("chain names come from candidates, which come from binds");
            // The RHS walk starts from "this binding is being forced":
            // a bare-variable alias RHS is a valid link here (forcing the
            // alias forces the referent with nothing in between).
            frontier = self.first_forced_candidate(&bind.body, &candidates, true);
        }
        exact
    }

    /// The candidate binding that evaluating `e` forces before any
    /// possible bottom — None when the first possibly-bottoming act cannot
    /// be pinned on one candidate. Walks case/if scrutinees (a case whose
    /// first branch is irrefutable never forces its scrutinee, so it does
    /// not anchor) and saturated calls to entry-forcing runtime callees.
    /// Every sibling argument of a carrier position must be
    /// bottom-free-to-force AND reference no candidate: all argument
    /// expressions run before the call, and a candidate's slot is still
    /// nil/unforced at that point (a stale concreteness claim from an
    /// outer same-named binding must not count it cheap).
    ///
    /// `whnf_taken` says the context takes `e`'s WHNF at this very point
    /// (a scrutinee, an entry-forced argument, a chained RHS). A bare Var
    /// answers only under it: the group body being a bare variable proves
    /// nothing — the emitted IIFE returns that thunk to a consumer that
    /// may force it arbitrarily later.
    fn first_forced_candidate(
        &self,
        e: &TExpr,
        candidates: &std::collections::HashSet<&str>,
        whnf_taken: bool,
    ) -> Option<String> {
        let mut e = e;
        while let TExprKind::Paren(p) = &e.kind {
            e = p.as_ref();
        }
        if Self::is_nullary_action_type(&e.ty) {
            return None; // suspended: evaluating it performs nothing now
        }
        match &e.kind {
            TExprKind::Var(n) if whnf_taken => {
                candidates.get(n.as_str()).map(|s| s.to_string())
            }
            TExprKind::Case { scrutinee, branches } => {
                if branches.first().is_some_and(|b| b.pattern.forces_scrutinee()) {
                    self.first_forced_candidate(scrutinee, candidates, true)
                } else {
                    None
                }
            }
            TExprKind::If { cond, .. } => {
                self.first_forced_candidate(cond, candidates, true)
            }
            TExprKind::App(_, _) => {
                // Flatten the spine, looking through parens at the head.
                let mut args_rev: Vec<&TExpr> = Vec::new();
                let mut f = e;
                loop {
                    match &f.kind {
                        TExprKind::App(func, arg) => {
                            args_rev.push(arg.as_ref());
                            f = func.as_ref();
                        }
                        TExprKind::Paren(i) => f = i.as_ref(),
                        _ => break,
                    }
                }
                let TExprKind::Var(g) = &f.kind else { return None };
                if self.is_local_shadowed(g) {
                    return None; // a local binding, not the runtime callee
                }
                let mask = crate::demand::entry_forced_mask(g)?;
                if mask.len() != args_rev.len() {
                    // Partial application runs nothing; over-application
                    // is a different emitted shape. Neither anchors.
                    return None;
                }
                let args: Vec<&TExpr> = args_rev.into_iter().rev().collect();
                // Exactly one argument may carry the chain, in an
                // entry-forced position; every other argument must be
                // bottom-free (their site evaluation runs before the
                // call, and the callee's entry force of an already-WHNF
                // value cannot bottom either).
                let mut carrier: Option<usize> = None;
                for (i, a) in args.iter().enumerate() {
                    if self.is_cheap_to_force(a)
                        && candidates.iter().all(|c| !expr_references_name(a, c))
                    {
                        continue;
                    }
                    if carrier.is_some() {
                        return None;
                    }
                    carrier = Some(i);
                }
                let j = carrier?;
                if !mask[j] {
                    return None;
                }
                self.first_forced_candidate(args[j], candidates, true)
            }
            _ => None,
        }
    }
}

/// Whether `binds[i]` may be emitted as a *strict* (immediately-evaluated,
/// non-thunk) assignment without reading a still-`nil` sibling.
///
/// A `let`/`where` group is mutually recursive: all names are forward-declared,
/// then assigned in source order. A strict assignment evaluates its RHS at the
/// point it runs, so it may only read siblings whose assignment has already
/// executed — i.e. names at an *earlier* position. A reference to itself or to a
/// later binding (index `>= i`) would read `nil`, so such a binding must be
/// emitted lazily (as a thunk) instead.
///
/// A `Lambda` body is the exception: its body runs when the function is *called*,
/// by which time every assignment in the group has completed, so a function
/// value is always safe to bind strictly regardless of forward references (this
/// is what makes mutually-recursive local functions work).
pub(super) fn strict_binding_safe(binds: &[TLocalDef], i: usize) -> bool {
    if matches!(binds[i].body.kind, TExprKind::Lambda { .. }) {
        return true;
    }
    // Not-yet-assigned siblings are those at position i (self) and beyond.
    !binds[i..].iter().any(|b| expr_references_name(&binds[i].body, &b.name))
}

/// If the lazy binding `binds[i]` is a pure alias — its RHS is exactly a bare
/// variable (after stripping parens, as arg_ast does) — return that variable
/// expression so the emitter assigns the raw reference instead of wrapping a
/// fresh thunk around a force of it. The variable already denotes a
/// thunk-or-value (the same rule arg_ast applies to bare-variable arguments):
/// `x = y` then shares y's thunk, so laziness is preserved (nothing is forced
/// at binding time), a single force memoizes for both names, and the extra
/// thunk allocation plus force indirection disappear. This is GHC semantics —
/// `let x = y` makes x and y the same lazy value.
///
/// Only fires when the raw read is sound: the reference is read at assignment
/// time, so the RHS must not be a self or forward sibling reference (those
/// slots are still nil — the same condition strict_binding_safe checks; for a
/// bare variable it degenerates to exactly that name test). A self/forward
/// alias stays thunked, which keeps `let x = x` a proper ⊥ instead of nil.
/// Anything that is not a bare variable — calls, constructor applications,
/// operators — is out of scope here and keeps its thunk.
pub(super) fn bare_var_alias(binds: &[TLocalDef], i: usize) -> Option<&TExpr> {
    let mut e = &binds[i].body;
    while let TExprKind::Paren(inner) = &e.kind {
        e = inner.as_ref();
    }
    if matches!(e.kind, TExprKind::Var(_)) && strict_binding_safe(binds, i) {
        Some(e)
    } else {
        None
    }
}
