//! Call-site inlining: an inline candidate's body emitted at a call site
//! with the call's arguments in place of its parameters.
//!
//! The substitution is applied at the TIR level (`subst_texpr`) and the
//! result goes through the ordinary emitter, so a substituted parameter is
//! emitted and weighed exactly as the call-site expression it stands for —
//! by construction, not by a mirrored walk. (An earlier version carried a
//! substituting twin of every builder and predicate — expr/arg/callee/
//! forced/yields-whnf/cheap-to-force — that had to mirror its plain twin
//! arm for arm; the `.` twin had drifted and its fallback arm dropped the
//! substitution.) `is_trivial_arg` is the sharing gate the call site
//! applies; `is_con_app` / `is_saturated_con_app` classify constructor
//! applications for the emitter.

use crate::tir::*;
use super::CodeGen;
use super::util::count_arrows;

impl CodeGen {
    /// The inlined body under `subst`, emitted by the ordinary emitter.
    pub(super) fn expr_subst_ast(&mut self, expr: &TExpr, subst: &std::collections::HashMap<String, &TExpr>) -> super::lua::Expr {
        let e = Self::subst_texpr(expr, subst);
        self.expr_ast(&e)
    }

    /// Would substituting `subst` into `body` CAPTURE a variable?
    /// `subst_texpr` is shadow-aware (a binder hides same-named entries of
    /// the map in its scope) but performs no alpha-renaming, so an argument
    /// expression whose variables collide with a binder inside the body
    /// would be captured: inlining `add x = \y -> x + y` at `add y`
    /// produced `\y -> y + y`. The check over-approximates on both sides
    /// (every variable occurrence in the argument, every binder in the
    /// body) — a false positive only declines an inline, and the call site
    /// falls back to the ordinary call.
    pub(super) fn subst_would_capture(
        body: &TExpr,
        subst: &std::collections::HashMap<String, &TExpr>,
    ) -> bool {
        // A parameter used as a backtick OPERATOR (`10 \`div\` x` under a
        // parameter named div) is not substitutable at all: an InfixApp op
        // is a string, not a Var node, so the substituted body would keep
        // the operator's builtin lowering instead of calling the argument.
        let mut op_uses = std::collections::HashSet::new();
        Self::collect_infix_op_uses(body, &mut op_uses);
        if subst.keys().any(|p| op_uses.contains(p)) {
            return true;
        }
        let mut binders = std::collections::HashSet::new();
        Self::collect_binders(body, &mut binders);
        if binders.is_empty() {
            return false;
        }
        let mut arg_vars = std::collections::HashSet::new();
        for arg in subst.values() {
            Self::collect_var_refs(arg, &mut arg_vars);
        }
        arg_vars.iter().any(|v| binders.contains(v))
    }

    /// Every identifier-shaped InfixApp operator used in `expr` (backtick
    /// operators — symbolic ones cannot collide with parameter names).
    fn collect_infix_op_uses(expr: &TExpr, out: &mut std::collections::HashSet<String>) {
        if let TExprKind::InfixApp { op, .. } = &expr.kind
            && op.starts_with(|c: char| c.is_alphabetic() || c == '_') {
                out.insert(op.clone());
            }
        expr.for_each_child(&mut |c| Self::collect_infix_op_uses(c, out));
    }

    /// Every variable occurrence in `expr` (an over-approximation of its
    /// free variables: bound occurrences are included, which only widens
    /// the capture check).
    fn collect_var_refs(expr: &TExpr, out: &mut std::collections::HashSet<String>) {
        if let TExprKind::Var(name) = &expr.kind {
            out.insert(name.clone());
        }
        expr.for_each_child(&mut |c| Self::collect_var_refs(c, out));
    }

    /// Every name bound ANYWHERE inside `expr` — lambda parameters, let
    /// binding names, their function-form parameters, and case pattern
    /// variables (the three TIR binder forms).
    fn collect_binders(expr: &TExpr, out: &mut std::collections::HashSet<String>) {
        match &expr.kind {
            TExprKind::Lambda { params, .. } => {
                for (name, _) in params {
                    out.insert(name.clone());
                }
            }
            TExprKind::Let { binds, .. } => {
                for b in binds {
                    out.insert(b.name.clone());
                    for p in &b.patterns {
                        for v in p.bound_vars() {
                            out.insert(v);
                        }
                    }
                }
            }
            TExprKind::Case { branches, .. } => {
                for b in branches {
                    for v in b.pattern.bound_vars() {
                        out.insert(v);
                    }
                }
            }
            _ => {}
        }
        expr.for_each_child(&mut |c| Self::collect_binders(c, out));
    }

    /// The expression with every substituted parameter replaced by its
    /// call-site expression, at the TIR level, so the ordinary emitter sees
    /// (and weighs) the call-site expression exactly where the parameter
    /// stood. Shadow-aware: a lambda parameter, a let name (or a let-bound
    /// function's own parameters, in its body) or a case pattern variable
    /// that rebinds a substituted name hides it in its scope. NOT
    /// capture-avoiding: the caller must decline substitutions whose
    /// argument variables collide with body binders (subst_would_capture)
    /// — there is no alpha-renaming here.
    fn subst_texpr(expr: &TExpr, subst: &std::collections::HashMap<String, &TExpr>) -> TExpr {
        if subst.is_empty() {
            return expr.clone();
        }
        let hide = |names: &[String]| -> std::collections::HashMap<String, &TExpr> {
            let mut inner = subst.clone();
            for n in names { inner.remove(n); }
            inner
        };
        match &expr.kind {
            TExprKind::Var(name) => match subst.get(name.as_str()) {
                Some(rep) => (*rep).clone(),
                None => expr.clone(),
            },
            TExprKind::Lambda { params, body } => {
                let names: Vec<String> = params.iter().map(|(n, _)| n.clone()).collect();
                let inner = hide(&names);
                TExpr::new(
                    TExprKind::Lambda {
                        params: params.clone(),
                        body: Box::new(Self::subst_texpr(body, &inner)),
                    },
                    expr.ty.clone(),
                )
            }
            TExprKind::Let { binds, body } => {
                let names: Vec<String> = binds.iter().map(|b| b.name.clone()).collect();
                let inner = hide(&names);
                let binds = binds.iter().map(|b| {
                    let mut own: Vec<String> = names.clone();
                    for p in &b.patterns { own.extend(p.bound_vars()); }
                    let bind_subst = hide(&own);
                    TLocalDef {
                        name: b.name.clone(),
                        patterns: b.patterns.clone(),
                        body: Self::subst_texpr(&b.body, &bind_subst),
                    }
                }).collect();
                TExpr::new(
                    TExprKind::Let { binds, body: Box::new(Self::subst_texpr(body, &inner)) },
                    expr.ty.clone(),
                )
            }
            TExprKind::Case { scrutinee, branches } => {
                let branches = branches.iter().map(|b| {
                    let inner = hide(&b.pattern.bound_vars());
                    TCaseBranch {
                        pattern: b.pattern.clone(),
                        guards: b.guards.iter().map(|g| TGuard {
                            condition: Self::subst_texpr(&g.condition, &inner),
                            body: Self::subst_texpr(&g.body, &inner),
                        }).collect(),
                        body: b.body.as_ref().map(|e| Self::subst_texpr(e, &inner)),
                    }
                }).collect();
                TExpr::new(
                    TExprKind::Case {
                        scrutinee: Box::new(Self::subst_texpr(scrutinee, subst)),
                        branches,
                    },
                    expr.ty.clone(),
                )
            }
            _ => expr.clone().map_children(&mut |c| Self::subst_texpr(&c, subst)),
        }
    }

    /// Whether a call-site argument is trivial to EMIT more than once: its
    /// duplicated emission duplicates no work. This is what admits an
    /// argument into a multiply-occurring (or under-lambda) parameter of an
    /// inlined body — the same work-free test GHC's inliner applies before
    /// substituting at several occurrences. Qualifying shapes:
    /// - a literal;
    /// - a variable — its emission is a name read or `__force(name)`, and
    ///   thunks memoize (runtime.lua `__force`), so a second force returns
    ///   the cached value;
    /// - a bare constructor or operator function — a value reference;
    /// - parens/negation over any of those (one duplicated Lua `-`).
    ///
    /// Anything else (calls, operators over non-trivial operands, tuples —
    /// each emission allocates) is declined; the call site then falls back
    /// to the ordinary call, which shares the argument by construction.
    pub(super) fn is_trivial_arg(expr: &TExpr) -> bool {
        match &expr.kind {
            TExprKind::Lit(_) | TExprKind::Var(_) | TExprKind::Con(_)
            | TExprKind::OpFunc(_) => true,
            TExprKind::Paren(e) | TExprKind::Negate(e) => Self::is_trivial_arg(e),
            _ => false,
        }
    }

    /// Check if an expression is a constructor application (Con applied to args)
    pub(super) fn is_con_app(expr: &TExpr) -> bool {
        match &expr.kind {
            TExprKind::Con(_) => true,
            TExprKind::App(func, _) => Self::is_con_app(func),
            _ => false,
        }
    }

    /// Check if an expression is a SATURATED constructor application: the head
    /// is statically a data constructor and the applied argument count equals
    /// the constructor's arity. The infix cons `x : xs` always qualifies (both
    /// operands are present by construction). For a prefix spine, saturation
    /// is proved against the Con head's own instantiated type — its arrow
    /// count IS the constructor's arity (the typechecker stamps every Con node
    /// with its constructor scheme) — plus no arrows remaining on the result.
    /// A partial application (`Just` as a value, a `Cons x` awaiting its
    /// tail) fails the arity test: it is a closure, not a construction, and
    /// must not be treated as one. An unprovable head type fails conservatively.
    pub(super) fn is_saturated_con_app(expr: &TExpr) -> bool {
        match &expr.kind {
            TExprKind::InfixApp { op, .. } => op == ":",
            TExprKind::App(..) => {
                let mut argc = 0usize;
                let mut f = expr;
                while let TExprKind::App(inner, _) = &f.kind {
                    argc += 1;
                    f = inner.as_ref();
                }
                matches!(&f.kind, TExprKind::Con(_))
                    && count_arrows(&expr.ty) == 0
                    && count_arrows(&f.ty) == argc
            }
            _ => false,
        }
    }

}
