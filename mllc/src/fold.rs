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
//!
//! On top of the per-expression folds, the pass propagates compile-time
//! constants ACROSS top-level bindings, to a fixpoint:
//!
//! - a top-level nullary binding whose body has folded to a literal (a
//!   literal CAF, `abc = 17`) substitutes into its use sites;
//! - a saturated call to a top-level function whose body is total
//!   arithmetic over its parameters (`def x = x + 1`), with every
//!   argument a literal, beta-reduces and folds (`def 5` → `6`).
//!
//! Each round can expose new folds for the next (`ghi = abc + def 5`
//! folds to `23` once `abc` has propagated and `def 5` has reduced, and
//! `ghi` is then itself a literal CAF for ITS users), so the module is
//! re-folded until nothing changes.  Termination: every rewrite either
//! replaces a non-literal node with a literal or drops nodes (the `if`
//! fold), and no rewrite introduces a non-literal node, so the count of
//! non-literal nodes strictly decreases on every changed round.
//!
//! Bottom-preservation is inherited from the literal folds: a call is
//! replaced only when the substituted body folds ALL THE WAY to a
//! literal, and the folds decline every partial case — a trapping
//! `div`/`mod` by literal zero, `i64` overflow, an `if` whose condition
//! did not fold — leaving the original expression for the runtime.
//! Candidate bodies are restricted to binder-free arithmetic shapes
//! (literals, parameters, operators, negation, parens, `if`), so
//! parameter substitution cannot capture, and a body that can reach
//! `error`/`undefined` (an App) is never a candidate in the first place.

use std::collections::HashMap;

use crate::tir::*;
use crate::types::Ty;

/// A literal-CAF value above this many bytes is NOT propagated: every
/// substitution duplicates the bytes at the use site, and a large string
/// constant is better shared through its (memoized) top-level binding.
/// Scalars always propagate.
const STR_PROPAGATE_MAX: usize = 40;

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
    loop {
        let mut folder = Folder {
            cafs: collect_literal_cafs(&module.functions),
            fns: collect_arith_fns(&module.functions),
            shadow: Vec::new(),
            changed: false,
        };
        module.functions = module
            .functions
            .into_iter()
            .map(|f| folder.fold_function(f))
            .collect();
        module.instance_fns = module
            .instance_fns
            .into_iter()
            .map(|f| folder.fold_function(f))
            .collect();
        if !folder.changed {
            break;
        }
    }
    module
}

/// Top-level literal CAFs: `name = <literal>`, no parameters, no guards,
/// no `where`, no dictionary parameters.  These substitute into use
/// sites.  `BigInteger` is excluded — its runtime value is CONSTRUCTED
/// (`__int_from_decimal`), so the shared top-level binding builds it
/// once while a propagated copy would rebuild it per use.  Long strings
/// are excluded for the duplication reason on `STR_PROPAGATE_MAX`.
fn collect_literal_cafs(functions: &[TFunction]) -> HashMap<String, TLiteral> {
    let mut cafs = HashMap::new();
    for f in functions {
        if !f.dict_params.is_empty() || f.clauses.len() != 1 {
            continue;
        }
        let c = &f.clauses[0];
        if !c.patterns.is_empty() || !c.guards.is_empty() || !c.where_binds.is_empty() {
            continue;
        }
        let Some(body) = &c.body else { continue };
        let Some(lit) = unwrap_lit(body) else { continue };
        match lit {
            TLiteral::BigInteger(_) => {}
            TLiteral::Str(s) if s.len() > STR_PROPAGATE_MAX => {}
            _ => {
                cafs.insert(f.name.clone(), lit.clone());
            }
        }
    }
    cafs
}

/// Top-level beta-reduction candidates: one clause, all-`Var` patterns,
/// no guards, no `where`, no dictionary parameters, and a body that is
/// binder-free total-arithmetic shape over ONLY its own parameters (see
/// `arith_only_body`).  Stored as (parameter names, body clone); a
/// saturated call with all-literal arguments substitutes and folds.
fn collect_arith_fns(functions: &[TFunction]) -> HashMap<String, (Vec<String>, TExpr)> {
    let mut fns = HashMap::new();
    for f in functions {
        if !f.dict_params.is_empty() || f.clauses.len() != 1 {
            continue;
        }
        let c = &f.clauses[0];
        if c.patterns.is_empty() || !c.guards.is_empty() || !c.where_binds.is_empty() {
            continue;
        }
        let mut params = Vec::with_capacity(c.patterns.len());
        if !c.patterns.iter().all(|p| match p {
            TPattern::Var(n, _) => {
                params.push(n.clone());
                true
            }
            _ => false,
        }) {
            continue;
        }
        let Some(body) = &c.body else { continue };
        if arith_only_body(body, &params) {
            fns.insert(f.name.clone(), (params, body.clone()));
        }
    }
    fns
}

/// Is `expr` a binder-free arithmetic shape whose every variable is one
/// of `params`?  Only these shapes are substitution targets: they contain
/// no binder (no capture) and no call (no hidden work or bottom beyond
/// what the literal folds themselves decline).
fn arith_only_body(expr: &TExpr, params: &[String]) -> bool {
    match &expr.kind {
        TExprKind::Lit(_) => true,
        TExprKind::Var(n) => params.iter().any(|p| p == n),
        TExprKind::Paren(inner) | TExprKind::Negate(inner) => arith_only_body(inner, params),
        TExprKind::InfixApp { lhs, rhs, .. } => {
            arith_only_body(lhs, params) && arith_only_body(rhs, params)
        }
        TExprKind::If { cond, then_branch, else_branch } => {
            arith_only_body(cond, params)
                && arith_only_body(then_branch, params)
                && arith_only_body(else_branch, params)
        }
        _ => false,
    }
}

/// Substitute literals for parameters in an `arith_only_body` clone.
/// No binders can occur (checked by `arith_only_body`), so the walk is
/// capture-free by construction.
fn subst_literals(expr: TExpr, env: &HashMap<&str, &TLiteral>) -> TExpr {
    let TExpr { kind, ty } = expr;
    let kind = match kind {
        TExprKind::Var(n) => match env.get(n.as_str()) {
            Some(lit) => TExprKind::Lit((*lit).clone()),
            None => TExprKind::Var(n),
        },
        TExprKind::Paren(inner) => TExprKind::Paren(Box::new(subst_literals(*inner, env))),
        TExprKind::Negate(inner) => TExprKind::Negate(Box::new(subst_literals(*inner, env))),
        TExprKind::InfixApp { op, lhs, rhs } => TExprKind::InfixApp {
            op,
            lhs: Box::new(subst_literals(*lhs, env)),
            rhs: Box::new(subst_literals(*rhs, env)),
        },
        TExprKind::If { cond, then_branch, else_branch } => TExprKind::If {
            cond: Box::new(subst_literals(*cond, env)),
            then_branch: Box::new(subst_literals(*then_branch, env)),
            else_branch: Box::new(subst_literals(*else_branch, env)),
        },
        other => other,
    };
    TExpr::new(kind, ty)
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

/// One folding round's state: the cross-binding environments (collected
/// from the module BEFORE the round, so a chain `a = 1; b = a + 1;
/// c = b + 1` converges over rounds, one link per round), the local
/// shadow stack, and the change marker driving the fixpoint.
struct Folder {
    cafs: HashMap<String, TLiteral>,
    fns: HashMap<String, (Vec<String>, TExpr)>,
    /// Names bound by enclosing local binders (clause parameters, `where`
    /// binds and their parameters, lambda parameters, `let` binds, `case`
    /// patterns).  A shadowed name is an ordinary local — never the
    /// top-level CAF or function of the same name (the same masking rule
    /// codegen's `is_local_shadowed` and the demand analyses apply).
    shadow: Vec<String>,
    changed: bool,
}

impl Folder {
    fn is_shadowed(&self, name: &str) -> bool {
        self.shadow.iter().any(|s| s == name)
    }

    fn fold_function(&mut self, mut func: TFunction) -> TFunction {
        func.clauses = func
            .clauses
            .into_iter()
            .map(|c| self.fold_clause(c))
            .collect();
        func
    }

    fn fold_clause(&mut self, mut clause: TClause) -> TClause {
        let mark = self.shadow.len();
        for p in &clause.patterns {
            p.for_each_var(&mut |v| self.shadow.push(v.to_string()));
        }
        for wb in &clause.where_binds {
            self.shadow.push(wb.name.clone());
        }
        clause.guards = clause
            .guards
            .into_iter()
            .map(|g| TGuard {
                condition: self.fold_expr(g.condition),
                body: self.fold_expr(g.body),
            })
            .collect();
        clause.body = clause.body.take().map(|b| self.fold_expr(b));
        clause.where_binds = clause
            .where_binds
            .into_iter()
            .map(|wb| self.fold_local(wb))
            .collect();
        self.shadow.truncate(mark);
        clause
    }

    fn fold_local(&mut self, mut def: TLocalDef) -> TLocalDef {
        let mark = self.shadow.len();
        for p in &def.patterns {
            p.for_each_var(&mut |v| self.shadow.push(v.to_string()));
        }
        def.body = self.fold_expr(def.body);
        self.shadow.truncate(mark);
        def
    }

    /// Fold one expression: children first (post-order), then the local
    /// rewrite for the shapes that fold: an infix operator on literals, a
    /// resolved typeclass method applied to literals, negation of a
    /// literal, `if` on a literal condition, a literal-CAF reference, and
    /// a saturated arithmetic call on literals.
    ///
    /// The binder-introducing variants (`Lambda`, `Let`, `Case`) are
    /// descended by hand with shadow-stack management; every other
    /// variant descends through `TExpr::map_children` — THE enumeration
    /// of a node's children, so a variant this pass has no rewrite for is
    /// still folded inside (a hand-rolled walk here once ended in
    /// `_ => …` and never looked inside `DictMethod`, `OutgoingCallback`
    /// or `FfiMaybeArg`).
    fn fold_expr(&mut self, expr: TExpr) -> TExpr {
        // Binder scopes first: their children are NOT uniform — names bound
        // here must mask the cross-binding environments in the subtree.
        match expr.kind {
            TExprKind::Lambda { params, body } => {
                let mark = self.shadow.len();
                self.shadow.extend(params.iter().map(|(p, _)| p.clone()));
                let body = Box::new(self.fold_expr(*body));
                self.shadow.truncate(mark);
                return TExpr::new(TExprKind::Lambda { params, body }, expr.ty);
            }
            TExprKind::Let { binds, body } => {
                // `let` is recursive: every bind's name is in scope in every
                // bind's body and in the let-body.
                let mark = self.shadow.len();
                self.shadow.extend(binds.iter().map(|b| b.name.clone()));
                let binds = binds
                    .into_iter()
                    .map(|b| self.fold_local(b))
                    .collect();
                let body = Box::new(self.fold_expr(*body));
                self.shadow.truncate(mark);
                return TExpr::new(TExprKind::Let { binds, body }, expr.ty);
            }
            TExprKind::Case { scrutinee, branches } => {
                let scrutinee = Box::new(self.fold_expr(*scrutinee));
                let branches = branches
                    .into_iter()
                    .map(|mut b| {
                        let mark = self.shadow.len();
                        b.pattern
                            .for_each_var(&mut |v| self.shadow.push(v.to_string()));
                        b.guards = b
                            .guards
                            .into_iter()
                            .map(|g| TGuard {
                                condition: self.fold_expr(g.condition),
                                body: self.fold_expr(g.body),
                            })
                            .collect();
                        b.body = b.body.take().map(|bb| self.fold_expr(bb));
                        self.shadow.truncate(mark);
                        b
                    })
                    .collect();
                return TExpr::new(TExprKind::Case { scrutinee, branches }, expr.ty);
            }
            _ => {}
        }

        let TExpr { kind, ty } = expr.map_children(&mut |c| self.fold_expr(c));
        match kind {
            TExprKind::Var(name) => {
                if !self.is_shadowed(&name)
                    && let Some(lit) = self.cafs.get(&name) {
                        self.changed = true;
                        return TExpr::new(TExprKind::Lit(lit.clone()), ty);
                    }
                TExpr::new(TExprKind::Var(name), ty)
            }
            TExprKind::InfixApp { op, lhs, rhs } => {
                if let Some(folded) = try_fold_infix(&op, &lhs, &rhs, &ty) {
                    self.changed = true;
                    return folded;
                }
                TExpr::new(TExprKind::InfixApp { op, lhs, rhs }, ty)
            }
            // Negating i64::MIN has no i64 result: like the binary folds, leave
            // it unfolded for the runtime (an unchecked `-n` panicked in debug
            // builds and wrapped in release, where an `Integer` promotes).
            TExprKind::Negate(inner) => match unwrap_lit(&inner) {
                Some(TLiteral::Integer(n)) if n.checked_neg().is_some() => {
                    self.changed = true;
                    TExpr::new(TExprKind::Lit(TLiteral::Integer(-n)), ty)
                }
                Some(TLiteral::Number(n)) => {
                    self.changed = true;
                    TExpr::new(TExprKind::Lit(TLiteral::Number(-n)), ty)
                }
                _ => TExpr::new(TExprKind::Negate(inner), ty),
            },
            TExprKind::If { cond, then_branch, else_branch } => {
                if let Some(TLiteral::Bool(b)) = unwrap_lit(&cond) {
                    self.changed = true;
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
                                self.changed = true;
                                return folded;
                            }
                let app = TExpr::new(TExprKind::App(f, a), ty);
                if let Some(folded) = self.try_fold_call(&app) {
                    self.changed = true;
                    return folded;
                }
                app
            }
            other => TExpr::new(other, ty),
        }
    }

    /// Beta-reduce-and-fold a saturated call to an arithmetic candidate
    /// with all-literal arguments.  The call is replaced ONLY when the
    /// substituted body folds all the way to a single literal — anything
    /// the folds decline (a trapping divisor, overflow, an unfolded `if`)
    /// keeps the original call, so every runtime-observable behavior
    /// (including bottom) is preserved.
    fn try_fold_call(&mut self, expr: &TExpr) -> Option<TExpr> {
        // Unwind the application spine: App(App(Var f, a1), a2) → f, [a1, a2].
        let mut args_rev = Vec::new();
        let mut head = expr;
        while let TExprKind::App(f, a) = &head.kind {
            args_rev.push(a.as_ref());
            head = f;
        }
        let TExprKind::Var(name) = &head.kind else { return None };
        if self.is_shadowed(name) {
            return None;
        }
        let (params, body) = self.fns.get(name.as_str())?;
        if args_rev.len() != params.len() {
            return None;
        }
        let mut env: HashMap<&str, &TLiteral> = HashMap::new();
        for (p, a) in params.iter().zip(args_rev.iter().rev()) {
            env.insert(p.as_str(), unwrap_lit(a)?);
        }
        // The fold below is SPECULATIVE: when it stops short of a single
        // literal the call is kept, and any interior folds it performed on
        // the discarded clone must not count as progress — a permanently
        // declined call (`(x + 1) div y` with y = 0) would otherwise mark
        // every round changed and the fixpoint would never terminate.
        let saved_changed = self.changed;
        let folded = self.fold_expr(subst_literals(body.clone(), &env));
        self.changed = saved_changed;
        match &folded.kind {
            // The call's result type, not the body clone's (they agree
            // after monomorphization; keep the use-site annotation).
            TExprKind::Lit(lit) => Some(TExpr::new(
                TExprKind::Lit(lit.clone()),
                expr.ty.clone(),
            )),
            _ => None,
        }
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
