/// Constant folding pass
///
/// Runs after monomorphization on the TIR.  Reduces compile-time-known
/// arithmetic, comparisons, boolean logic, string concatenation, and
/// negation of literals to their result literals.
///
/// After monomorphization, typeclass methods (==, <, >, <>, etc.) are
/// resolved to concrete functions like `eq_Integer`, `ord_gt__Number`,
/// `semigroup_String`.  We recognize these App(App(Var(name), lhs), rhs)
/// patterns and fold them too.

use crate::tir::*;
use crate::types::Ty;

// ── Module / function / clause traversal ────────────────

pub fn fold_module(mut module: TModule) -> TModule {
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
    clause.body = fold_expr(clause.body);
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
        "eq_Integer" | "eq_Number" | "eq_String" | "eq_Bool" => Some("=="),
        // Ord instances
        "ord_lt__Integer"  | "ord_lt__Number"  | "ord_lt__String"  => Some("<"),
        "ord_gt__Integer"  | "ord_gt__Number"  | "ord_gt__String"  => Some(">"),
        "ord_le__Integer"  | "ord_le__Number"  | "ord_le__String"  => Some("<="),
        "ord_ge__Integer"  | "ord_ge__Number"  | "ord_ge__String"  => Some(">="),
        // Semigroup
        "semigroup_String" => Some("<>"),
        _ => None,
    }
}

// ── Expression traversal + folding ──────────────────────

fn fold_expr(expr: TExpr) -> TExpr {
    let ty = expr.ty;
    match expr.kind {
        TExprKind::InfixApp { op, lhs, rhs } => {
            let lhs = fold_expr(*lhs);
            let rhs = fold_expr(*rhs);
            if let Some(folded) = try_fold_infix(&op, &lhs, &rhs, &ty) {
                return folded;
            }
            TExpr::new(TExprKind::InfixApp {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            }, ty)
        }
        TExprKind::Negate(inner) => {
            let inner = fold_expr(*inner);
            match unwrap_lit(&inner) {
                Some(TLiteral::Integer(n)) =>
                    TExpr::new(TExprKind::Lit(TLiteral::Integer(-n)), ty),
                Some(TLiteral::Number(n)) =>
                    TExpr::new(TExprKind::Lit(TLiteral::Number(-n)), ty),
                _ => TExpr::new(TExprKind::Negate(Box::new(inner)), ty),
            }
        }
        TExprKind::If { cond, then_branch, else_branch } => {
            let cond = fold_expr(*cond);
            let then_branch = fold_expr(*then_branch);
            let else_branch = fold_expr(*else_branch);
            if let Some(TLiteral::Bool(b)) = unwrap_lit(&cond) {
                return if *b { then_branch } else { else_branch };
            }
            TExpr::new(TExprKind::If {
                cond: Box::new(cond),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            }, ty)
        }
        TExprKind::Paren(inner) => {
            let inner = fold_expr(*inner);
            TExpr::new(TExprKind::Paren(Box::new(inner)), ty)
        }
        TExprKind::App(f, a) => {
            let f = fold_expr(*f);
            let a = fold_expr(*a);
            // Recognize App(App(Var(method), lhs), rhs) from resolved typeclass methods
            if let TExprKind::App(ff, lhs) = &f.kind {
                if let TExprKind::Var(name) = &ff.kind {
                    if let Some(op) = resolved_method_to_op(name) {
                        if let Some(folded) = try_fold_infix(op, lhs, &a, &ty) {
                            return folded;
                        }
                    }
                }
            }
            TExpr::new(TExprKind::App(Box::new(f), Box::new(a)), ty)
        }
        TExprKind::Lambda { params, body } => {
            TExpr::new(TExprKind::Lambda {
                params,
                body: Box::new(fold_expr(*body)),
            }, ty)
        }
        TExprKind::Case { scrutinee, branches } => {
            TExpr::new(TExprKind::Case {
                scrutinee: Box::new(fold_expr(*scrutinee)),
                branches: branches.into_iter().map(|b| TCaseBranch {
                    pattern: b.pattern,
                    guards: b.guards.into_iter().map(|g| TGuard {
                        condition: fold_expr(g.condition),
                        body: fold_expr(g.body),
                    }).collect(),
                    body: fold_expr(b.body),
                }).collect(),
            }, ty)
        }
        TExprKind::Let { binds, body } => {
            TExpr::new(TExprKind::Let {
                binds: binds.into_iter().map(fold_local).collect(),
                body: Box::new(fold_expr(*body)),
            }, ty)
        }
        TExprKind::Tuple(elems) => {
            TExpr::new(TExprKind::Tuple(
                elems.into_iter().map(fold_expr).collect(),
            ), ty)
        }
        TExprKind::SpecCall { original, specialized, args } => {
            TExpr::new(TExprKind::SpecCall {
                original,
                specialized,
                args: args.into_iter().map(fold_expr).collect(),
            }, ty)
        }
        TExprKind::DictCall { func_name, dict_args, value_args } => {
            TExpr::new(TExprKind::DictCall {
                func_name,
                dict_args: dict_args.into_iter().map(fold_expr).collect(),
                value_args: value_args.into_iter().map(fold_expr).collect(),
            }, ty)
        }
        TExprKind::RecordUpdate { record, updates, num_fields } => {
            TExpr::new(TExprKind::RecordUpdate {
                record: Box::new(fold_expr(*record)),
                updates: updates.into_iter().map(|(name, idx, val)| {
                    (name, idx, fold_expr(val))
                }).collect(),
                num_fields,
            }, ty)
        }
        // Leaves: Var, Con, Lit, OpFunc, DictAccess — nothing to fold
        _ => TExpr::new(expr.kind, ty),
    }
}

// ── Constant folding logic ──────────────────────────────

fn try_fold_infix(op: &str, lhs: &TExpr, rhs: &TExpr, ty: &Ty) -> Option<TExpr> {
    match (unwrap_lit(lhs), unwrap_lit(rhs)) {
        // Integer op Integer
        (Some(TLiteral::Integer(a)), Some(TLiteral::Integer(b))) => {
            fold_int_int(op, *a, *b, ty)
        }
        // Number op Number
        (Some(TLiteral::Number(a)), Some(TLiteral::Number(b))) => {
            fold_num_num(op, *a, *b, ty)
        }
        // String <> String
        (Some(TLiteral::Str(a)), Some(TLiteral::Str(b))) if op == "<>" => {
            Some(TExpr::new(TExprKind::Lit(TLiteral::Str(format!("{}{}", a, b))), ty.clone()))
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
        "div" => if b != 0 { lit(a.div_euclid(b)) } else { None },
        "mod" => if b != 0 { lit(a.rem_euclid(b)) } else { None },
        "=="  => cmp(a == b),
        "/="  => cmp(a != b),
        "<"   => cmp(a < b),
        ">"   => cmp(a > b),
        "<="  => cmp(a <= b),
        ">="  => cmp(a >= b),
        _     => None,
    }
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
