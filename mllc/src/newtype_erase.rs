//! Newtype constructor erasure (a TIR pass, after folding).
//!
//! A newtype is a zero-cost wrapper: at runtime a value of `newtype N = N Int`
//! IS the wrapped `Int`, and `N e` is exactly `e` — in GHC a newtype
//! constructor is transparent to evaluation, so `N ⊥` is ⊥, `seq (N x)`
//! forces `x`, and demanding `N e` to WHNF demands `e` to WHNF.
//!
//! Codegen used to emit a saturated `N e` as a call of an identity function
//! over a LAZILY suspended `e`: `__mll_fn[k](__thunk(e))`, which returns the
//! raw thunk. That broke the runtime's WHNF-return invariant (every compiled
//! function and thunk body returns a value, never a thunk — `__force`
//! unwraps exactly one level): `negate (N a) = N (negate a)` returned a
//! thunk, and any consumer that trusted the invariant saw a thunk table
//! where a number was due. Concrete-type call sites compensated through a
//! type gate (`ty_app_result_whnf` refuses the WHNF claim for a
//! newtype-typed result and forces), but two paths have no such gate:
//! a dictionary method called through a `Num a =>` dictionary (the result
//! type is a type variable, and the thunk body `return d.negate(y)` trusted
//! the callee), and a FIRST-CLASS constructor (`map N xs`), whose `f x`
//! call inside `map` returned the raw element thunk into a cons head —
//! `case map N (map (+1) xs) of (N a : _) -> a * 2` crashed on a table.
//!
//! This pass makes the source-level truth the IR truth: every saturated
//! `App(Con N, e)` becomes `e`, so codegen, the demand analyses, the
//! inliner and fusion all see the expression that is actually computed and
//! weigh its strictness, cheapness and WHNF-ness directly. The unapplied
//! constructor (`map N xs`, `N . f`) stays a function value, and codegen
//! emits it as `function(_v) return __force(_v) end` — the identity that
//! honours the WHNF-return invariant (see codegen/module.rs).
//!
//! Patterns get the same treatment: `N p` becomes `p`. A newtype pattern
//! is irrefutable in GHC — `case ⊥ of N _ -> 1` is 1 — but a constructor
//! pattern counts as one that inspects the scrutinee to every forcing
//! predicate (`TPattern::forces_scrutinee`, the demand analysis' entry
//! forces, the clause emitter), so `case lazy of N _ -> …` forced `lazy`.
//! With the wrapper gone, `N _` is `_` and `N (Just x)` is `Just x`: each
//! predicate sees exactly what GHC would match on.

use crate::tir::{TClause, TExpr, TExprKind, TFunction, TGuard, TLocalDef, TModule, TPattern};
use std::collections::HashSet;

/// Erase every saturated newtype constructor application in the module.
pub fn erase(mut module: TModule) -> TModule {
    debug_assert_eq!(
        module.passes_run.last(),
        Some(&"fold"),
        "newtype erasure runs on the folded module"
    );
    module.passes_run.push("newtype_erase");
    if module.newtypes.is_empty() {
        return module;
    }
    let ctors: HashSet<String> = module.newtypes.iter().cloned().collect();
    module.functions = module.functions.into_iter().map(|f| erase_function(&ctors, f)).collect();
    module.instance_fns = module.instance_fns.into_iter().map(|f| erase_function(&ctors, f)).collect();
    module
}

fn erase_function(ctors: &HashSet<String>, mut f: TFunction) -> TFunction {
    f.clauses = f.clauses.into_iter().map(|c| erase_clause(ctors, c)).collect();
    f
}

fn erase_clause(ctors: &HashSet<String>, mut c: TClause) -> TClause {
    c.patterns = c.patterns.into_iter().map(|p| erase_pattern(ctors, p)).collect();
    c.guards = c
        .guards
        .into_iter()
        .map(|g| TGuard { condition: erase_expr(ctors, g.condition), body: erase_expr(ctors, g.body) })
        .collect();
    c.body = c.body.take().map(|b| erase_expr(ctors, b));
    c.where_binds = c
        .where_binds
        .into_iter()
        .map(|wb| erase_local(ctors, wb))
        .collect();
    c
}

fn erase_local(ctors: &HashSet<String>, wb: TLocalDef) -> TLocalDef {
    TLocalDef {
        patterns: wb.patterns.into_iter().map(|p| erase_pattern(ctors, p)).collect(),
        body: erase_expr(ctors, wb.body),
        ..wb
    }
}

/// `N p` → `p`, through parentheses, as-patterns, tuples and other
/// constructors' arguments.
fn erase_pattern(ctors: &HashSet<String>, pat: TPattern) -> TPattern {
    match pat {
        TPattern::Constructor { name, mut args } if ctors.contains(&name) && args.len() == 1 => {
            erase_pattern(ctors, args.pop().unwrap())
        }
        TPattern::Constructor { name, args } => TPattern::Constructor {
            name,
            args: args.into_iter().map(|a| erase_pattern(ctors, a)).collect(),
        },
        TPattern::Paren(inner) => TPattern::Paren(Box::new(erase_pattern(ctors, *inner))),
        TPattern::As(n, inner) => TPattern::As(n, Box::new(erase_pattern(ctors, *inner))),
        TPattern::Tuple(ps) => TPattern::Tuple(ps.into_iter().map(|p| erase_pattern(ctors, p)).collect()),
        other => other,
    }
}

/// Post-order: children first, so a nested `N (M e)` collapses to `e`.
/// The result keeps the ARGUMENT's own type: that is the value computed,
/// and the type gates downstream (WHNF claims, FFI descriptors) must judge
/// what is emitted, not the erased wrapper.
fn erase_expr(ctors: &HashSet<String>, expr: TExpr) -> TExpr {
    let expr = expr.map_children(&mut |c| erase_expr(ctors, c));
    match expr.kind {
        TExprKind::App(func, arg)
            if matches!(&func.kind, TExprKind::Con(name) if ctors.contains(name)) =>
        {
            *arg
        }
        // The pattern positions map_children does not visit (it enumerates
        // child EXPRESSIONS): case alternatives and local bindings.
        TExprKind::Case { scrutinee, branches } => TExpr {
            kind: TExprKind::Case {
                scrutinee,
                branches: branches
                    .into_iter()
                    .map(|mut b| {
                        b.pattern = erase_pattern(ctors, b.pattern);
                        b
                    })
                    .collect(),
            },
            ty: expr.ty,
        },
        TExprKind::Let { binds, body } => TExpr {
            kind: TExprKind::Let {
                binds: binds.into_iter().map(|wb| erase_local_patterns(ctors, wb)).collect(),
                body,
            },
            ty: expr.ty,
        },
        kind => TExpr { kind, ty: expr.ty },
    }
}

/// A let-bound local whose BODY map_children already rewrote: only its
/// patterns are left.
fn erase_local_patterns(ctors: &HashSet<String>, mut wb: TLocalDef) -> TLocalDef {
    wb.patterns = wb.patterns.into_iter().map(|p| erase_pattern(ctors, p)).collect();
    wb
}
