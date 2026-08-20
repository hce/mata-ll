//! Constant folding pass
//!
//! Runs after monomorphization on the TIR.  Reduces compile-time-known
//! arithmetic, comparisons, boolean logic, string concatenation, and
//! negation of literals to their result literals.
//!
//! After monomorphization, typeclass methods (==, <, >, <>, etc.) are
//! resolved to concrete functions like `eq_Int`, `ord_gt__Number`,
//! `semigroup_String`.  We recognize these App(App(Var(name), lhs), rhs)
//! patterns and fold them too.

use crate::tir::*;
use crate::types::Ty;

// ── Module / function / clause traversal ────────────────

pub fn fold_module(mut module: TModule) -> TModule {
    // Pass-order witness: folding recognizes mono's concrete method names
    // (`eq_Int`, …), so it must see monomorphized bodies.
    debug_assert_eq!(
        module.passes_run.last(),
        Some(&"mono"),
        "fold must run directly on mono's output"
    );
    module.passes_run.push("fold");
    module.functions = module.functions.into_iter().map(fold_function).collect();
    module.instance_fns = module.instance_fns.into_iter().map(fold_function).collect();
    module
}

fn fold_function(mut func: TFunction) -> TFunction {
    func.clauses = func.clauses.into_iter().map(fold_clause).collect();
    func
}

fn fold_clause(mut clause: TClause) -> TClause {
    clause.guards = clause.guards.into_iter().map(|g| TGuard {
        condition: fold_expr(g.condition),
        body: fold_expr(g.body),
    }).collect();
    clause.body = clause.body.take().map(fold_expr);
    clause.where_binds = clause.where_binds.into_iter().map(fold_local).collect();
    clause
}

fn fold_local(mut def: TLocalDef) -> TLocalDef {
    def.body = fold_expr(def.body);
    def
}

// ── Paren stripping ─────────────────────────────────────

/// Look through Paren wrappers to find the underlying literal, if any.
fn unwrap_lit(expr: &TExpr) -> Option<&TLiteral> {
    match &expr.kind {
        TExprKind::Lit(lit) => Some(lit),
        TExprKind::Paren(inner) => unwrap_lit(inner),
        _ => None,
    }
}

// ── Resolved typeclass method → operator mapping ────────

/// Map monomorphized typeclass method names back to foldable operators.
/// These match the names the monomorphizer generates and the codegen
/// recognizes in its App(App(Var(name), lhs), rhs) inlining path.
fn resolved_method_to_op(name: &str) -> Option<&'static str> {
    match name {
        // Eq instances
        "eq_Int" | "eq_Number" | "eq_String" | "eq_Bool" => Some("=="),
        // Ord instances
        "ord_lt__Int"  | "ord_lt__Number"  | "ord_lt__String"  => Some("<"),
        "ord_gt__Int"  | "ord_gt__Number"  | "ord_gt__String"  => Some(">"),
        "ord_le__Int"  | "ord_le__Number"  | "ord_le__String"  => Some("<="),
        "ord_ge__Int"  | "ord_ge__Number"  | "ord_ge__String"  => Some(">="),
        // Semigroup
        "semigroup_String" => Some("<>"),
        _ => None,
    }
}

// ── Expression traversal + folding ──────────────────────

/// Fold one expression: children first (post-order, through
/// `TExpr::map_children` — THE enumeration of a node's children, so a
/// variant this pass has no rewrite for is still descended into; a
/// hand-rolled walk here once ended in `_ => …` and never looked inside
/// `DictMethod`, `OutgoingCallback` or `FfiMaybeArg`), then the local
/// rewrite for the shapes that fold: an infix operator on literals, a
/// resolved typeclass method applied to literals, negation of a literal,
/// and `if` on a literal condition.
fn fold_expr(expr: TExpr) -> TExpr {
    let TExpr { kind, ty } = expr.map_children(&mut fold_expr);
    match kind {
        TExprKind::InfixApp { op, lhs, rhs } => {
            if let Some(folded) = try_fold_infix(&op, &lhs, &rhs, &ty) {
                return folded;
            }
            TExpr::new(TExprKind::InfixApp { op, lhs, rhs }, ty)
        }
        // Negating i64::MIN has no i64 result: like the binary folds, leave
        // it unfolded for the runtime (an unchecked `-n` panicked in debug
        // builds and wrapped in release, where an `Integer` promotes).
        TExprKind::Negate(inner) => match unwrap_lit(&inner) {
            Some(TLiteral::Integer(n)) if n.checked_neg().is_some() =>
                TExpr::new(TExprKind::Lit(TLiteral::Integer(-n)), ty),
            Some(TLiteral::Number(n)) =>
                TExpr::new(TExprKind::Lit(TLiteral::Number(-n)), ty),
            _ => TExpr::new(TExprKind::Negate(inner), ty),
        },
        TExprKind::If { cond, then_branch, else_branch } => {
            if let Some(TLiteral::Bool(b)) = unwrap_lit(&cond) {
                return if *b { *then_branch } else { *else_branch };
            }
            TExpr::new(TExprKind::If { cond, then_branch, else_branch }, ty)
        }
        TExprKind::App(f, a) => {
            // Recognize App(App(Var(method), lhs), rhs) from resolved typeclass methods
            if let TExprKind::App(ff, lhs) = &f.kind
                && let TExprKind::Var(name) = &ff.kind
                    && let Some(op) = resolved_method_to_op(name)
                        && let Some(folded) = try_fold_infix(op, lhs, &a, &ty) {
                            return folded;
                        }
            TExpr::new(TExprKind::App(f, a), ty)
        }
        other => TExpr::new(other, ty),
    }
}

// ── Constant folding logic ──────────────────────────────

fn try_fold_infix(op: &str, lhs: &TExpr, rhs: &TExpr, ty: &Ty) -> Option<TExpr> {
    match (unwrap_lit(lhs), unwrap_lit(rhs)) {
        // Int op Int
        (Some(TLiteral::Integer(a)), Some(TLiteral::Integer(b))) => {
            fold_int_int(op, *a, *b, ty)
        }
        // Number op Number
        (Some(TLiteral::Number(a)), Some(TLiteral::Number(b))) => {
            fold_num_num(op, *a, *b, ty)
        }
        // String <> String
        (Some(TLiteral::Str(a)), Some(TLiteral::Str(b))) if op == "<>" => {
            let mut joined = a.clone();
            joined.extend_from_slice(b);
            Some(TExpr::new(TExprKind::Lit(TLiteral::Str(joined)), ty.clone()))
        }
        // Bool && Bool, Bool || Bool, Bool == Bool
        (Some(TLiteral::Bool(a)), Some(TLiteral::Bool(b))) => {
            fold_bool_bool(op, *a, *b, ty)
        }
        _ => None,
    }
}

fn fold_int_int(op: &str, a: i64, b: i64, ty: &Ty) -> Option<TExpr> {
    let lit = |v: i64| Some(TExpr::new(TExprKind::Lit(TLiteral::Integer(v)), ty.clone()));
    let cmp = |v: bool| Some(TExpr::new(TExprKind::Lit(TLiteral::Bool(v)), ty.clone()));
    match op {
        "+"   => a.checked_add(b).and_then(lit),
        "-"   => a.checked_sub(b).and_then(lit),
        "*"   => a.checked_mul(b).and_then(lit),
        // Haskell (and the emitted Lua runtime) use FLOOR semantics: `div`
        // rounds the quotient toward negative infinity and `mod` takes the
        // sign of the DIVISOR (7 `div` (-2) = -4, 7 `mod` (-2) = -1). Rust's
        // div_euclid/rem_euclid are Euclidean (mod always >= 0), which
        // disagrees for a negative divisor — folding with them would make a
        // constant expression evaluate differently from the identical
        // expression computed at runtime. Zero divisors (and the lone
        // i64::MIN / -1 overflow) are left unfolded so the runtime raises.
        "div" => if b != 0 { floor_div(a, b).and_then(lit) } else { None },
        "mod" => if b != 0 { floor_mod(a, b).and_then(lit) } else { None },
        "=="  => cmp(a == b),
        "/="  => cmp(a != b),
        "<"   => cmp(a < b),
        ">"   => cmp(a > b),
        "<="  => cmp(a <= b),
        ">="  => cmp(a >= b),
        _     => None,
    }
}

/// Floor division, matching Haskell's `div` and the Lua runtime (`//` /
/// math.floor(a/b)): truncate toward negative infinity, so a quotient with a
/// fractional part and mixed-sign operands rounds DOWN one more than Rust's
/// truncating `/`. None on the single overflowing case (i64::MIN div -1).
fn floor_div(a: i64, b: i64) -> Option<i64> {
    let q = a.checked_div(b)?;
    let r = a % b; // safe: checked_div succeeded, so b != 0 and no overflow
    Some(if r != 0 && ((r < 0) != (b < 0)) { q - 1 } else { q })
}

/// Floor modulo, matching Haskell's `mod` and Lua's `%`: the result has the
/// sign of the divisor and satisfies a == b * floor_div(a,b) + floor_mod(a,b).
fn floor_mod(a: i64, b: i64) -> Option<i64> {
    let r = a.checked_rem(b)?;
    Some(if r != 0 && ((r < 0) != (b < 0)) { r + b } else { r })
}

fn fold_num_num(op: &str, a: f64, b: f64, ty: &Ty) -> Option<TExpr> {
    let lit = |v: f64| Some(TExpr::new(TExprKind::Lit(TLiteral::Number(v)), ty.clone()));
    let cmp = |v: bool| Some(TExpr::new(TExprKind::Lit(TLiteral::Bool(v)), ty.clone()));
    match op {
        "+"  => lit(a + b),
        "-"  => lit(a - b),
        "*"  => lit(a * b),
        "/"  => if b != 0.0 { lit(a / b) } else { None },
        "^"  => lit(a.powf(b)),
        "==" => cmp(a == b),
        "/=" => cmp(a != b),
        "<"  => cmp(a < b),
        ">"  => cmp(a > b),
        "<=" => cmp(a <= b),
        ">=" => cmp(a >= b),
        _    => None,
    }
}

fn fold_bool_bool(op: &str, a: bool, b: bool, ty: &Ty) -> Option<TExpr> {
    let lit = |v: bool| Some(TExpr::new(TExprKind::Lit(TLiteral::Bool(v)), ty.clone()));
    match op {
        "&&" => lit(a && b),
        "||" => lit(a || b),
        "==" => lit(a == b),
        "/=" => lit(a != b),
        _    => None,
    }
}
