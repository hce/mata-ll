//! Type- and TIR-shape helpers shared across the codegen module.
//!
//! Type side: `decompose_app` splits a type-application spine into head and
//! arguments, `con_name` reads a nullary type constructor, `subst_tyvars`
//! specialises field types, `count_arrows` counts top-level arrows (the
//! arity of the N-ary calling convention). TIR side: `expr_references_name`
//! finds uses of a name, `count_name_occurrences` weighs how often an
//! inlined body would re-emit a substituted argument (the inliner's
//! work-duplication measure), and `expr_evaluates_global_ref` decides whether a
//! top-level value binding would read another binding's slot eagerly at
//! module-load time — such a binding must be thunked so the read happens
//! after every slot is assigned.

use crate::tir::*;
use crate::types::Ty;

/// Decompose a type-application spine into its head constructor name and its
/// arguments in source order: `HashMap k v` → (Some("HashMap"), [k, v]),
/// `Maybe a` → (Some("Maybe"), [a]), `Foo` → (Some("Foo"), []). Non-Con heads
/// (type variables, functions, …) yield `(None, [])`.
pub(super) fn decompose_app(ty: &Ty) -> (Option<&str>, Vec<&Ty>) {
    let mut args: Vec<&Ty> = Vec::new();
    let mut cur = ty;
    loop {
        match cur {
            Ty::App(f, a) => {
                args.push(a.as_ref());
                cur = f.as_ref();
            }
            Ty::Con(name) => {
                args.reverse();
                return (Some(name.as_str()), args);
            }
            _ => return (None, Vec::new()),
        }
    }
}

/// The name of a nullary type constructor (`Con`), else None. Used to read a
/// HashMap's declared key type for the FFI-boundary key-type check.
pub(super) fn con_name(ty: &Ty) -> Option<&str> {
    match ty {
        Ty::Con(name) => Some(name.as_str()),
        _ => None,
    }
}

/// Substitute type variables (by name) throughout `ty`. Used to specialise a
/// LuaDict record's field types against the concrete arguments of the record
/// type at an FFI-result position.
pub(super) fn subst_tyvars(ty: &Ty, map: &std::collections::HashMap<String, Ty>) -> Ty {
    match ty {
        Ty::Var(v) => map.get(&v.name).cloned().unwrap_or_else(|| ty.clone()),
        Ty::App(f, a) => Ty::App(
            Box::new(subst_tyvars(f, map)),
            Box::new(subst_tyvars(a, map)),
        ),
        Ty::List(i) => Ty::List(Box::new(subst_tyvars(i, map))),
        Ty::Tuple(es) => Ty::Tuple(es.iter().map(|e| subst_tyvars(e, map)).collect()),
        Ty::IO(i) => Ty::IO(Box::new(subst_tyvars(i, map))),
        Ty::LuaIO(s, i) => Ty::LuaIO(s.clone(), Box::new(subst_tyvars(i, map))),
        other => other.clone(),
    }
}

/// Check if a TExpr references a given name anywhere
/// Would evaluating `expr` eagerly (at module-load time) read another
/// top-level binding's value? A top-level value binding with no params/where
/// has no locals, so every variable it mentions is a global reference.
/// References *inside a lambda* are safe — the closure reads them at call
/// time, after every slot is assigned — but a reference evaluated immediately
/// (a bare alias `y = x`, an operand `useX = x + 1`, a constructor field
/// `c = Just g`) is not: the referent's slot may still be nil when the eager
/// assignment runs. Such a binding must be thunked so the read is deferred to
/// first use.
pub(super) fn expr_evaluates_global_ref(expr: &TExpr) -> bool {
    match &expr.kind {
        TExprKind::Var(_) => true,
        // A lambda only captures its body; the reads fire at call time.
        TExprKind::Lambda { .. } => false,
        TExprKind::Con(_) | TExprKind::Lit(_) | TExprKind::OpFunc(_) => false,
        TExprKind::App(f, a) => expr_evaluates_global_ref(f) || expr_evaluates_global_ref(a),
        TExprKind::InfixApp { lhs, rhs, .. } => {
            expr_evaluates_global_ref(lhs) || expr_evaluates_global_ref(rhs)
        }
        TExprKind::Negate(e) | TExprKind::Paren(e) => expr_evaluates_global_ref(e),
        TExprKind::If { cond, then_branch, else_branch } => {
            expr_evaluates_global_ref(cond)
                || expr_evaluates_global_ref(then_branch)
                || expr_evaluates_global_ref(else_branch)
        }
        TExprKind::Tuple(elems) => elems.iter().any(expr_evaluates_global_ref),
        // Not reachable from the is_cheap eager path; thunk to be safe.
        _ => true,
    }
}

pub(super) fn expr_references_name(expr: &TExpr, name: &str) -> bool {
    match &expr.kind {
        TExprKind::Var(n) => n == name,
        TExprKind::Con(_) | TExprKind::Lit(_) | TExprKind::OpFunc(_) => false,
        TExprKind::App(f, a) => expr_references_name(f, name) || expr_references_name(a, name),
        TExprKind::InfixApp { lhs, rhs, .. } => {
            expr_references_name(lhs, name) || expr_references_name(rhs, name)
        }
        TExprKind::Negate(e) | TExprKind::Paren(e) => expr_references_name(e, name),
        TExprKind::Lambda { body, .. } => expr_references_name(body, name),
        TExprKind::If { cond, then_branch, else_branch } => {
            expr_references_name(cond, name) ||
            expr_references_name(then_branch, name) ||
            expr_references_name(else_branch, name)
        }
        TExprKind::Case { scrutinee, branches } => {
            expr_references_name(scrutinee, name) ||
            branches.iter().any(|b| {
                b.guards.iter().any(|g|
                    expr_references_name(&g.condition, name) || expr_references_name(&g.body, name))
                || expr_references_name(&b.body, name)
            })
        }
        TExprKind::Let { binds, body } => {
            binds.iter().any(|b| expr_references_name(&b.body, name)) ||
            expr_references_name(body, name)
        }
        TExprKind::SpecCall { args, .. } => args.iter().any(|a| expr_references_name(a, name)),
        TExprKind::Tuple(elems) => elems.iter().any(|e| expr_references_name(e, name)),
        TExprKind::DictAccess { .. } => false,
        TExprKind::DictMethod { dict, .. } => expr_references_name(dict, name),
        TExprKind::DictCall { dict_args, value_args, .. } => {
            dict_args.iter().any(|a| expr_references_name(a, name)) ||
            value_args.iter().any(|a| expr_references_name(a, name))
        }
        TExprKind::RecordUpdate { record, updates, .. } => {
            expr_references_name(record, name) ||
            updates.iter().any(|(_, _, e)| expr_references_name(e, name))
        }
        TExprKind::OutgoingCallback { callee, .. } => expr_references_name(callee, name),
        TExprKind::FfiMaybeArg { value } => expr_references_name(value, name),
    }
}

/// How many times an inlined body would EMIT (and so evaluate) an argument
/// substituted for `name` — the inliner's work-duplication measure, not a
/// plain syntactic count:
///
/// - `if`/`case` alternatives are exclusive at runtime, so alternatives
///   contribute the MAXIMUM of their counts, not the sum (GHC's occurrence
///   analyser makes the same one-branch allowance);
/// - an occurrence under a lambda counts double: the lambda may be called
///   any number of times, so a non-trivial argument substituted there is
///   re-evaluated per call even when it occurs just once syntactically
///   (GHC only inlines into a lambda when the argument is work-free);
/// - a lambda whose own parameter rebinds `name` shadows it — inner
///   occurrences are the lambda's parameter, never substituted (mirrors
///   the `inner_subst.remove` in expr_subst_ast's Lambda arm);
/// - `let` is counted as a plain sum without modelling its shadowing:
///   over-counting only declines an inline, never mis-emits (and is_cheap
///   keeps `let`/`case` out of inline-candidate bodies anyway).
pub(super) fn count_name_occurrences(expr: &TExpr, name: &str) -> usize {
    match &expr.kind {
        TExprKind::Var(n) => usize::from(n == name),
        TExprKind::Con(_) | TExprKind::Lit(_) | TExprKind::OpFunc(_)
        | TExprKind::DictAccess { .. } => 0,
        TExprKind::App(f, a) => {
            count_name_occurrences(f, name) + count_name_occurrences(a, name)
        }
        TExprKind::InfixApp { lhs, rhs, .. } => {
            count_name_occurrences(lhs, name) + count_name_occurrences(rhs, name)
        }
        TExprKind::Negate(e) | TExprKind::Paren(e) => count_name_occurrences(e, name),
        TExprKind::Lambda { params, body } => {
            if params.iter().any(|(p, _)| p == name) {
                0
            } else {
                2 * count_name_occurrences(body, name)
            }
        }
        TExprKind::If { cond, then_branch, else_branch } => {
            count_name_occurrences(cond, name)
                + count_name_occurrences(then_branch, name)
                    .max(count_name_occurrences(else_branch, name))
        }
        TExprKind::Case { scrutinee, branches } => {
            count_name_occurrences(scrutinee, name)
                + branches
                    .iter()
                    .map(|b| {
                        b.guards
                            .iter()
                            .map(|g| {
                                count_name_occurrences(&g.condition, name)
                                    + count_name_occurrences(&g.body, name)
                            })
                            .sum::<usize>()
                            + count_name_occurrences(&b.body, name)
                    })
                    .max()
                    .unwrap_or(0)
        }
        TExprKind::Let { binds, body } => {
            binds
                .iter()
                .map(|b| count_name_occurrences(&b.body, name))
                .sum::<usize>()
                + count_name_occurrences(body, name)
        }
        TExprKind::SpecCall { args, .. } => {
            args.iter().map(|a| count_name_occurrences(a, name)).sum()
        }
        TExprKind::Tuple(elems) => {
            elems.iter().map(|e| count_name_occurrences(e, name)).sum()
        }
        TExprKind::DictMethod { dict, .. } => count_name_occurrences(dict, name),
        TExprKind::DictCall { dict_args, value_args, .. } => {
            dict_args
                .iter()
                .chain(value_args.iter())
                .map(|a| count_name_occurrences(a, name))
                .sum()
        }
        TExprKind::RecordUpdate { record, updates, .. } => {
            count_name_occurrences(record, name)
                + updates
                    .iter()
                    .map(|(_, _, e)| count_name_occurrences(e, name))
                    .sum::<usize>()
        }
        TExprKind::OutgoingCallback { callee, .. } => count_name_occurrences(callee, name),
        TExprKind::FfiMaybeArg { value } => count_name_occurrences(value, name),
    }
}

/// Count how many arrows are at the top level of a type.
/// Arrow(a, Arrow(b, c)) = 2, Arrow(a, b) = 1, Con(_) = 0
pub(super) fn count_arrows(ty: &Ty) -> usize {
    match ty {
        Ty::Arrow(_, rest, _) => 1 + count_arrows(rest),
        _ => 0,
    }
}
