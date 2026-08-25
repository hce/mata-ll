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
    pub(super) fn is_cheap_with(expr: &TExpr, var_ok: &dyn Fn(&str) -> bool) -> bool {
        match &expr.kind {
            TExprKind::Lit(_) | TExprKind::Con(_)
            | TExprKind::Lambda { .. } | TExprKind::OpFunc(_) => true,
            TExprKind::Var(name) => var_ok(name),
            TExprKind::Paren(inner) | TExprKind::Negate(inner) => Self::is_cheap_with(inner, var_ok),
            TExprKind::Tuple(elems) => elems.iter().all(|e| Self::is_cheap_with(e, var_ok)),
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
                        && Self::is_cheap_with(lhs, var_ok)
                        && Self::is_cheap_with(rhs, var_ok);
                }
                // Builtin ops (arithmetic, comparison, concat) are cheap
                // if their operands are cheap. `.` stays: composition only
                // BUILDS a closure — no operand is called.
                is_builtin_op(op) && Self::is_cheap_with(lhs, var_ok) && Self::is_cheap_with(rhs, var_ok)
            }
            TExprKind::App(func, arg) => {
                // Constructor applications are cheap (just table creation).
                // General function applications are NOT cheap — the function
                // body might be expensive even if the args are cheap.
                if Self::is_con_app(expr) {
                    Self::is_cheap_with(arg, var_ok) && Self::is_cheap_with(func, var_ok)
                } else {
                    false
                }
            }
            TExprKind::If { cond, then_branch, else_branch } => {
                Self::is_cheap_with(cond, var_ok)
                    && Self::is_cheap_with(then_branch, var_ok)
                    && Self::is_cheap_with(else_branch, var_ok)
            }
            // Function calls, case, let — potentially expensive, thunk them
            _ => false,
        }
    }

    /// "Small to duplicate/evaluate": `is_cheap_with` with every variable
    /// counted cheap (see there for the two notions).
    pub(super) fn is_cheap(expr: &TExpr) -> bool {
        Self::is_cheap_with(expr, &|_| true)
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
        Self::is_cheap_with(expr, &|name| {
            // Prelude `otherwise` is the literal `true` — unless a local
            // binder shadows it, in which case it is an ordinary (possibly
            // thunked) variable like any other.
            (name == "otherwise" && !self.is_local_shadowed(name))
                || self.concrete_vars.contains(&sanitize_name(name))
        }) && !Self::contains_trapping_op(expr)
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
                matches!(op.as_str(), "div" | "mod" | "quot" | "rem" | "%")
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
