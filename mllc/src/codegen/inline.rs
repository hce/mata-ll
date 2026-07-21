//! Call-site inlining: emission under a parameter substitution.
//!
//! Every `*_subst` function here is the substituting twin of a plain
//! emission or predicate in thunks.rs / expr.rs — `gen_expr_subst` of
//! `gen_expr`, `gen_arg_subst` of `gen_arg`, `gen_callee_subst` of
//! `gen_callee`, `gen_forced_subst` of `gen_forced`,
//! `gen_expr_subst_yields_whnf` of `gen_expr_yields_whnf`,
//! `is_cheap_to_force_subst` of `is_cheap_to_force` — and must mirror its
//! twin's semantics arm for arm: a substituted parameter is emitted and
//! weighed exactly as the call-site expression it stands for.
//! `is_con_app` / `is_saturated_con_app` classify constructor applications,
//! whose special emission the substituting walk does not replicate and
//! inlining therefore avoids.

use crate::tir::*;
use super::CodeGen;
use super::names::{is_builtin_op, sanitize_name};
use super::util::{count_arrows, expr_references_name};

impl CodeGen {
    /// Emit an expression with parameter substitution for inlining.
    /// Only recurses into sub-expressions that might contain substitution
    /// variables; delegates to gen_expr for everything else.
    pub(super) fn gen_expr_subst(&mut self, expr: &TExpr, subst: &std::collections::HashMap<String, &TExpr>) {
        // If no substitution vars appear in this expr, use normal gen_expr
        // (which handles cons, list literals, etc. correctly)
        let has_subst_vars = subst.keys().any(|k| expr_references_name(expr, k));
        if !has_subst_vars {
            self.gen_expr(expr);
            return;
        }
        match &expr.kind {
            TExprKind::Var(name) => {
                if let Some(replacement) = subst.get(name.as_str()) {
                    self.gen_expr(replacement);
                } else {
                    self.gen_expr(expr);
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
                    self.emit(match op.as_str() {
                        "div" => "__mll_div(", "mod" => "__mll_mod(",
                        "quot" => "__mll_quot(", _ => "__mll_rem(",
                    });
                    self.gen_forced_subst(lhs, subst);
                    self.emit(", ");
                    self.gen_forced_subst(rhs, subst);
                    self.emit(")");
                    return;
                }
                if op == "++" {
                    self.emit("__mll_list_append(");
                    self.gen_expr_subst(lhs, subst);
                    self.emit(", function() return ");
                    self.gen_expr_subst(rhs, subst);
                    self.emit(" end)");
                    return;
                }
                if op == "!!" {
                    self.emit("__mll_list_index(");
                    self.gen_expr_subst(lhs, subst);
                    self.emit(", ");
                    self.gen_expr_subst(rhs, subst);
                    self.emit(")");
                    return;
                }
                if op == "$" {
                    // return $ x / pure $ x in action context: just emit x
                    if matches!(&lhs.kind, TExprKind::Var(n) if n == "pure" || n == "return") {
                        self.gen_expr_subst(rhs, subst);
                        return;
                    }
                    self.gen_callee_subst(lhs, subst);
                    self.emit("(__thunk(function() return ");
                    self.gen_expr_subst(rhs, subst);
                    self.emit(" end))");
                    return;
                }
                if op == "." {
                    // Mirror the gen_expr "." arm. Without this, `.` would
                    // fall into the builtin-op branch below (is_builtin_op
                    // lists it for cheapness) and be emitted as Lua infix
                    // `a . b`, which is not a Lua operator.
                    self.emit("(function(_x) return ");
                    self.gen_callee_subst(lhs, subst);
                    self.emit("(");
                    self.gen_callee_subst(rhs, subst);
                    self.emit("(_x)) end)");
                    return;
                }
                let lua_op = match op.as_str() {
                    "<>" => "..", "&&" => "and", "||" => "or", "/=" => "~=",
                    // "div"/"mod" never reach here: handled above via
                    // __mll_div/__mll_mod (zero-divisor check, exact // ).
                    other => other,
                };
                if is_builtin_op(op) {
                    self.emit("(");
                    self.gen_forced_subst(lhs, subst);
                    self.emit(&format!(" {} ", lua_op));
                    self.gen_forced_subst(rhs, subst);
                    self.emit(")");
                } else {
                    let sop = sanitize_name(op);
                    self.emit(&self.lua_ref(&sop)); self.emit("(");
                    self.gen_expr_subst(lhs, subst); self.emit(", ");
                    self.gen_expr_subst(rhs, subst); self.emit(")");
                }
            }
            TExprKind::Paren(inner) => {
                self.emit("(");
                self.gen_expr_subst(inner, subst);
                self.emit(")");
            }
            TExprKind::Negate(inner) => {
                self.emit("(-");
                self.gen_expr_subst(inner, subst);
                self.emit(")");
            }
            TExprKind::Lambda { params, body } => {
                // Flatten nested lambdas and eta-pad to the type's full arrow
                // count, exactly like the gen_expr Lambda arm — the emitted
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
                // to force its uses in the body. See the gen_expr Lambda arm.
                for name in &orig {
                    inner_subst.remove(*name);
                }
                for p in &ps {
                    self.local_vars.insert(p.clone());
                    self.concrete_vars.remove(p);
                }
                let mut all_params = ps.clone();
                all_params.extend(eta_params.iter().cloned());
                self.emit(&format!("function({})\n", all_params.join(", ")));
                self.indent += 1;
                self.emit_indent(); self.emit("return ");
                if eta_count > 0 {
                    // The callee position needs a WHNF function value; when
                    // the emission already yields one, gen_callee_subst only
                    // adds the parens a bare fn literal needs to be called.
                    if self.gen_expr_subst_yields_whnf(inner_body, &inner_subst) {
                        self.gen_callee_subst(inner_body, &inner_subst);
                    } else {
                        self.emit("__force(");
                        self.gen_expr_subst(inner_body, &inner_subst);
                        self.emit(")");
                    }
                    self.emit(&format!("({})", eta_params.join(", ")));
                } else {
                    self.gen_expr_subst(inner_body, &inner_subst);
                }
                self.emit("\n");
                self.indent -= 1;
                self.emit_indent(); self.emit("end");
                self.local_vars = saved_locals;
                self.concrete_vars = saved_concrete;
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
                self.gen_callee_subst(f, subst);
                self.emit("(");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 { self.emit(", "); }
                    self.gen_expr_subst(a, subst);
                }
                self.emit(")");
            }
            TExprKind::If { cond, then_branch, else_branch } => {
                self.emit("(function()\n");
                self.indent += 1;
                self.emit_indent(); self.emit("if ");
                self.gen_expr_subst(cond, subst);
                self.emit(" then\n");
                self.indent += 1;
                self.emit_indent(); self.emit("return ");
                self.gen_expr_subst(then_branch, subst);
                self.emit("\n");
                self.indent -= 1;
                self.emit_indent(); self.emit("else\n");
                self.indent += 1;
                self.emit_indent(); self.emit("return ");
                self.gen_expr_subst(else_branch, subst);
                self.emit("\n");
                self.indent -= 1;
                self.emit_indent(); self.emit("end\n");
                self.indent -= 1;
                self.emit_indent(); self.emit("end)()");
            }
            TExprKind::Tuple(elems) => {
                self.emit("{");
                for (i, elem) in elems.iter().enumerate() {
                    if i > 0 { self.emit(", "); }
                    // Tuple fields are lazy positions in the inlined path too:
                    // inlining `f x = (x, 1)` at `snd (f (error "boom"))` must
                    // not run the error. Same weighing as the gen_expr arm.
                    self.gen_arg_subst(elem, subst);
                }
                self.emit("}");
            }
            _ => self.gen_expr(expr),
        }
    }

    /// `gen_arg` for a lazy position inside an inlined (substituted) body —
    /// today that is exactly a tuple field of an inlined function. The same
    /// eager-vs-lazy weighing as gen_arg, with substituted parameters weighed
    /// as the call-site expressions they stand for.
    pub(super) fn gen_arg_subst(&mut self, expr: &TExpr, subst: &std::collections::HashMap<String, &TExpr>) {
        let stripped = {
            let mut e = expr;
            while let TExprKind::Paren(inner) = &e.kind { e = inner.as_ref(); }
            e
        };
        // A bare substituted parameter IS the call-site argument: weigh that
        // argument exactly as a non-inlined call would.
        if let TExprKind::Var(name) = &stripped.kind
            && let Some(rep) = subst.get(name.as_str()) {
                self.gen_arg(rep, false);
                return;
            }
        // No substituted parameter occurs: ordinary weighing.
        if !subst.keys().any(|k| expr_references_name(expr, k)) {
            self.gen_arg(expr, false);
            return;
        }
        // A tuple is WHNF-total — emit it directly, never as a whole-tuple
        // thunk, and let its Tuple arm weigh each field (same rule as gen_arg).
        if matches!(&stripped.kind, TExprKind::Tuple(_)) {
            self.gen_expr_subst(stripped, subst);
            return;
        }
        // Mixed structure over substituted parameters: eager only when the
        // structure is total AND every substituted parameter it leans on is
        // itself provably total; otherwise the laziness weight is maximal
        // (the expression may be ⊥) and it is suspended.
        if self.is_cheap_to_force_subst(expr, subst) {
            self.gen_expr_subst(expr, subst);
        } else {
            self.emit("__thunk(function() return ");
            self.gen_expr_subst(expr, subst);
            self.emit(" end)");
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

    /// Substituting counterpart of gen_callee for the inline path. The callee
    /// may be a substitution variable whose *replacement* is a function
    /// literal (e.g. a section passed to an inlined higher-order function),
    /// so the literal check resolves through the substitution first.
    pub(super) fn gen_callee_subst(
        &mut self,
        f: &TExpr,
        subst: &std::collections::HashMap<String, &TExpr>,
    ) {
        let resolved: &TExpr = match &f.kind {
            TExprKind::Var(name) => subst.get(name.as_str()).copied().unwrap_or(f),
            _ => f,
        };
        let needs_wrap = Self::is_bare_fn_literal(resolved);
        if needs_wrap { self.emit("("); }
        self.gen_expr_subst(f, subst);
        if needs_wrap { self.emit(")"); }
    }

    /// Substituting counterpart of gen_expr_yields_whnf: WHNF-ness of the
    /// gen_expr_subst emission. The arms mirror gen_expr_subst exactly; note
    /// its App arm always emits a generic call (no accessor / primitive-op
    /// inline), so App is never WHNF here, and a substituted variable stands
    /// for its replacement, emitted by plain gen_expr.
    pub(super) fn gen_expr_subst_yields_whnf(
        &self,
        expr: &TExpr,
        subst: &std::collections::HashMap<String, &TExpr>,
    ) -> bool {
        if !subst.keys().any(|k| expr_references_name(expr, k)) {
            // gen_expr_subst delegates wholesale to gen_expr in this case.
            return self.gen_expr_yields_whnf(expr);
        }
        match &expr.kind {
            TExprKind::Var(name) => match subst.get(name.as_str()) {
                Some(repl) => self.gen_expr_yields_whnf(repl),
                None => true, // gen_expr Var arm — see gen_expr_yields_whnf
            },
            TExprKind::Lit(_) | TExprKind::Negate(_) | TExprKind::Con(_)
            | TExprKind::Tuple(_) | TExprKind::Lambda { .. } => true,
            TExprKind::Paren(inner) => self.gen_expr_subst_yields_whnf(inner, subst),
            TExprKind::InfixApp { op, .. } => Self::infix_yields_whnf(op),
            _ => false,
        }
    }

    /// Substituting counterpart of gen_forced, for the inline path. A
    /// substituted parameter is emitted as its call-site replacement,
    /// weighed exactly like gen_forced would weigh it directly — this is
    /// what used to blindly emit `__force(<replacement>)` and produced the
    /// hot-loop `__force(__force(ch))` doubles.
    pub(super) fn gen_forced_subst(
        &mut self,
        expr: &TExpr,
        subst: &std::collections::HashMap<String, &TExpr>,
    ) {
        if self.gen_expr_subst_yields_whnf(expr, subst) {
            self.gen_expr_subst(expr, subst);
        } else {
            self.emit("__force(");
            self.gen_expr_subst(expr, subst);
            self.emit(")");
        }
    }
}
