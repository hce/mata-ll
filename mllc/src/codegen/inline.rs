//! Call-site inlining: AST building under a parameter substitution.
//!
//! Every `*_subst` function here is the substituting twin of a plain
//! builder or predicate in thunks.rs / expr.rs — `expr_subst_ast` of
//! `expr_ast`, `arg_subst_ast` of `arg_ast`, `callee_subst_ast` of
//! `callee_ast`, `forced_subst_ast` of `forced_ast`,
//! `expr_subst_yields_whnf` of `expr_yields_whnf`,
//! `is_cheap_to_force_subst` of `is_cheap_to_force` — and must mirror its
//! twin's semantics arm for arm: a substituted parameter is emitted and
//! weighed exactly as the call-site expression it stands for.
//! `is_con_app` / `is_saturated_con_app` classify constructor applications,
//! whose special emission the substituting walk does not replicate and
//! inlining therefore avoids.

use crate::tir::*;
use super::CodeGen;
use super::lua::{Block, Expr, FuncBody, Item, Stmt};
use super::names::{is_builtin_op, sanitize_name};
use super::util::{count_arrows, expr_references_name};

impl CodeGen {
    /// Build an expression with parameter substitution for inlining.
    /// Only recurses into sub-expressions that might contain substitution
    /// variables; delegates to expr_ast for everything else.
    pub(super) fn expr_subst_ast(&mut self, expr: &TExpr, subst: &std::collections::HashMap<String, &TExpr>) -> Expr {
        // If no substitution vars appear in this expr, use normal expr_ast
        // (which handles cons, list literals, etc. correctly)
        let has_subst_vars = subst.keys().any(|k| expr_references_name(expr, k));
        if !has_subst_vars {
            return self.expr_ast(expr);
        }
        match &expr.kind {
            TExprKind::Var(name) => {
                if let Some(replacement) = subst.get(name.as_str()) {
                    self.expr_ast(replacement)
                } else {
                    self.expr_ast(expr)
                }
            }
            TExprKind::InfixApp { op, lhs, rhs } => {
                if op == "div" || op == "mod" || op == "quot" || op == "rem" {
                    // Runtime helpers, not inline float math / bare `%`:
                    // math.floor(a/0) yields inf (a float escaping into
                    // Integer) instead of raising, and float division is
                    // inexact past 2^53. __mll_div/__mll_mod raise a clear
                    // error on a zero divisor and use native integer floor
                    // division (Lua 5.3+ `//`) when the host has it. quot/rem
                    // truncate toward zero (remainder takes the dividend's sign).
                    let helper = match op.as_str() {
                        "div" => "__mll_div", "mod" => "__mll_mod",
                        "quot" => "__mll_quot", _ => "__mll_rem",
                    };
                    let l = self.forced_subst_ast(lhs, subst);
                    let r = self.forced_subst_ast(rhs, subst);
                    return Expr::call_named(helper, vec![l, r]);
                }
                if op == "++" {
                    let l = self.expr_subst_ast(lhs, subst);
                    let r = self.expr_subst_ast(rhs, subst);
                    return Expr::call_named("__mll_list_append", vec![l, Expr::inline_fn0(r)]);
                }
                if op == "!!" {
                    let l = self.expr_subst_ast(lhs, subst);
                    let r = self.expr_subst_ast(rhs, subst);
                    return Expr::call_named("__mll_list_index", vec![l, r]);
                }
                if op == "$" {
                    // return $ x / pure $ x in action context: just emit x
                    if matches!(&lhs.kind, TExprKind::Var(n) if n == "pure" || n == "return") {
                        return self.expr_subst_ast(rhs, subst);
                    }
                    let callee = self.callee_subst_ast(lhs, subst);
                    let arg = self.expr_subst_ast(rhs, subst);
                    return Expr::call(callee, vec![Expr::thunk(arg)]);
                }
                if op == "." {
                    // Mirror the expr_ast "." arm. Without this, `.` would
                    // fall into the builtin-op branch below (is_builtin_op
                    // lists it for cheapness) and be emitted as Lua infix
                    // `a . b`, which is not a Lua operator.
                    let f = self.callee_subst_ast(lhs, subst);
                    let g = self.callee_subst_ast(rhs, subst);
                    return Expr::paren(Expr::Func(
                        vec!["_x".into()],
                        FuncBody::Inline(vec![Stmt::Return(Expr::call(
                            f,
                            vec![Expr::call(g, vec![Expr::name("_x")])],
                        ))]),
                    ));
                }
                let lua_op = match op.as_str() {
                    "<>" => "..", "&&" => "and", "||" => "or", "/=" => "~=",
                    // "div"/"mod" never reach here: handled above via
                    // __mll_div/__mll_mod (zero-divisor check, exact // ).
                    other => other,
                };
                if is_builtin_op(op) {
                    let l = self.forced_subst_ast(lhs, subst);
                    let r = self.forced_subst_ast(rhs, subst);
                    Expr::paren(Expr::binop(lua_op, l, r))
                } else {
                    let sop = sanitize_name(op);
                    let fref = self.lua_ref(&sop);
                    let l = self.expr_subst_ast(lhs, subst);
                    let r = self.expr_subst_ast(rhs, subst);
                    Expr::call_named(&fref, vec![l, r])
                }
            }
            TExprKind::Paren(inner) => Expr::paren(self.expr_subst_ast(inner, subst)),
            TExprKind::Negate(inner) => Expr::paren(Expr::neg(self.expr_subst_ast(inner, subst))),
            TExprKind::Lambda { params, body } => {
                // Flatten nested lambdas and eta-pad to the type's full arrow
                // count, exactly like the expr_ast Lambda arm — the emitted
                // arity must match the N-ary calling convention.
                let (orig, inner_body) = Self::flatten_lambda(params, body);
                let ps = Self::lambda_param_names(&orig);
                let eta_count = count_arrows(&expr.ty).saturating_sub(ps.len());
                let eta_params: Vec<String> =
                    (0..eta_count).map(|i| format!("_eta{}", i)).collect();
                // Remove shadowed names from substitution
                let mut inner_subst = subst.clone();
                let saved_locals = self.local_vars.clone();
                let saved_concrete = self.concrete_vars.clone();
                // A lambda parameter is NOT guaranteed forced (it may receive a
                // thunk through a higher-order call), so drop it from
                // concrete_vars — a same-named outer binding may be concrete —
                // to force its uses in the body. See the expr_ast Lambda arm.
                for name in &orig {
                    inner_subst.remove(*name);
                }
                for p in &ps {
                    self.local_vars.insert(p.clone());
                    self.concrete_vars.remove(p);
                }
                let mut all_params = ps.clone();
                all_params.extend(eta_params.iter().cloned());
                let ret = if eta_count > 0 {
                    // The callee position needs a WHNF function value; when
                    // the emission already yields one, callee_subst_ast only
                    // adds the parens a bare fn literal needs to be called.
                    let callee = if self.expr_subst_yields_whnf(inner_body, &inner_subst) {
                        self.callee_subst_ast(inner_body, &inner_subst)
                    } else {
                        Expr::force(self.expr_subst_ast(inner_body, &inner_subst))
                    };
                    Expr::call(
                        callee,
                        eta_params.iter().map(|p| Expr::name(p.clone())).collect(),
                    )
                } else {
                    self.expr_subst_ast(inner_body, &inner_subst)
                };
                let out = Expr::Func(all_params, FuncBody::Block(Block(vec![Stmt::Return(ret)])));
                self.local_vars = saved_locals;
                self.concrete_vars = saved_concrete;
                out
            }
            TExprKind::App(_, _) => {
                // Collect the application chain, substituting as we go
                let mut args: Vec<&TExpr> = vec![];
                let mut f = expr;
                while let TExprKind::App(inner_f, inner_arg) = &f.kind {
                    args.push(inner_arg.as_ref());
                    f = inner_f.as_ref();
                }
                args.reverse();
                let callee = self.callee_subst_ast(f, subst);
                let mut cargs = Vec::new();
                for a in &args {
                    cargs.push(self.expr_subst_ast(a, subst));
                }
                Expr::call(callee, cargs)
            }
            TExprKind::If { cond, then_branch, else_branch } => {
                let cond_e = self.expr_subst_ast(cond, subst);
                let then_s = Stmt::Return(self.expr_subst_ast(then_branch, subst));
                let else_s = Stmt::Return(self.expr_subst_ast(else_branch, subst));
                Expr::call(
                    Expr::paren(Expr::Func(
                        vec![],
                        FuncBody::Block(Block(vec![Stmt::If {
                            cond: cond_e,
                            then_b: Block(vec![then_s]),
                            elseifs: vec![],
                            else_b: Some(Block(vec![else_s])),
                        }])),
                    )),
                    vec![],
                )
            }
            TExprKind::Tuple(elems) => {
                let mut items = Vec::new();
                for elem in elems {
                    // Tuple fields are lazy positions in the inlined path too:
                    // inlining `f x = (x, 1)` at `snd (f (error "boom"))` must
                    // not run the error. Same weighing as the expr_ast arm.
                    items.push(Item::Pos(self.arg_subst_ast(elem, subst)));
                }
                Expr::Table(items)
            }
            _ => self.expr_ast(expr),
        }
    }

    /// `arg_ast` for a lazy position inside an inlined (substituted) body —
    /// today that is exactly a tuple field of an inlined function. The same
    /// eager-vs-lazy weighing as arg_ast, with substituted parameters weighed
    /// as the call-site expressions they stand for.
    pub(super) fn arg_subst_ast(&mut self, expr: &TExpr, subst: &std::collections::HashMap<String, &TExpr>) -> Expr {
        let stripped = {
            let mut e = expr;
            while let TExprKind::Paren(inner) = &e.kind { e = inner.as_ref(); }
            e
        };
        // A bare substituted parameter IS the call-site argument: weigh that
        // argument exactly as a non-inlined call would.
        if let TExprKind::Var(name) = &stripped.kind
            && let Some(rep) = subst.get(name.as_str()) {
                return self.arg_ast(rep, false);
            }
        // No substituted parameter occurs: ordinary weighing.
        if !subst.keys().any(|k| expr_references_name(expr, k)) {
            return self.arg_ast(expr, false);
        }
        // A tuple is WHNF-total — emit it directly, never as a whole-tuple
        // thunk, and let its Tuple arm weigh each field (same rule as arg_ast).
        if matches!(&stripped.kind, TExprKind::Tuple(_)) {
            return self.expr_subst_ast(stripped, subst);
        }
        // Mixed structure over substituted parameters: eager only when the
        // structure is total AND every substituted parameter it leans on is
        // itself provably total; otherwise the laziness weight is maximal
        // (the expression may be ⊥) and it is suspended.
        if self.is_cheap_to_force_subst(expr, subst) {
            self.expr_subst_ast(expr, subst)
        } else {
            let e = self.expr_subst_ast(expr, subst);
            Expr::thunk(e)
        }
    }

    /// `is_cheap_to_force` under an inlining substitution: a substituted
    /// variable is exactly as safe to force as the call-site expression it
    /// stands for.
    pub(super) fn is_cheap_to_force_subst(&self, expr: &TExpr, subst: &std::collections::HashMap<String, &TExpr>) -> bool {
        Self::is_cheap_with(expr, &|name| match subst.get(name) {
            Some(rep) => self.is_cheap_to_force(rep),
            None => name == "otherwise" || self.concrete_vars.contains(&sanitize_name(name)),
        }) && !Self::contains_trapping_op(expr)
    }

    /// Check if an expression contains a function call where the function
    /// is NOT a known top-level/prelude name. Such calls could be to
    /// arbitrary function parameters and may be expensive.
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

    /// Substituting counterpart of callee_ast for the inline path. The callee
    /// may be a substitution variable whose *replacement* is a function
    /// literal (e.g. a section passed to an inlined higher-order function),
    /// so the literal check resolves through the substitution first.
    pub(super) fn callee_subst_ast(
        &mut self,
        f: &TExpr,
        subst: &std::collections::HashMap<String, &TExpr>,
    ) -> Expr {
        let resolved: &TExpr = match &f.kind {
            TExprKind::Var(name) => subst.get(name.as_str()).copied().unwrap_or(f),
            _ => f,
        };
        let needs_wrap = Self::is_bare_fn_literal(resolved);
        let e = self.expr_subst_ast(f, subst);
        if needs_wrap { Expr::paren(e) } else { e }
    }

    /// Substituting counterpart of expr_yields_whnf: WHNF-ness of the
    /// expr_subst_ast emission. The arms mirror expr_subst_ast exactly; note
    /// its App arm always emits a generic call (no accessor / primitive-op
    /// inline), so App is never WHNF here, and a substituted variable stands
    /// for its replacement, emitted by plain expr_ast.
    pub(super) fn expr_subst_yields_whnf(
        &self,
        expr: &TExpr,
        subst: &std::collections::HashMap<String, &TExpr>,
    ) -> bool {
        if !subst.keys().any(|k| expr_references_name(expr, k)) {
            // expr_subst_ast delegates wholesale to expr_ast in this case.
            return self.expr_yields_whnf(expr);
        }
        match &expr.kind {
            TExprKind::Var(name) => match subst.get(name.as_str()) {
                Some(repl) => self.expr_yields_whnf(repl),
                None => true, // expr_ast Var arm — see expr_yields_whnf
            },
            TExprKind::Lit(_) | TExprKind::Negate(_) | TExprKind::Con(_)
            | TExprKind::Tuple(_) | TExprKind::Lambda { .. } => true,
            TExprKind::Paren(inner) => self.expr_subst_yields_whnf(inner, subst),
            TExprKind::InfixApp { op, .. } => Self::infix_yields_whnf(op),
            _ => false,
        }
    }

    /// Substituting counterpart of forced_ast, for the inline path. A
    /// substituted parameter is emitted as its call-site replacement,
    /// weighed exactly like forced_ast would weigh it directly — this is
    /// what used to blindly emit `__force(<replacement>)` and produced the
    /// hot-loop `__force(__force(ch))` doubles.
    pub(super) fn forced_subst_ast(
        &mut self,
        expr: &TExpr,
        subst: &std::collections::HashMap<String, &TExpr>,
    ) -> Expr {
        if self.expr_subst_yields_whnf(expr, subst) {
            self.expr_subst_ast(expr, subst)
        } else {
            let e = self.expr_subst_ast(expr, subst);
            Expr::force(e)
        }
    }
}
