//! Whole-program analyses run before emission.
//!
//! `analyze_call_sites` determines, per function, which parameter positions
//! receive cheap (non-thunk) arguments at every call site — such parameters
//! never need `__force` at entry. `mark_hidden_call_site` accounts for the
//! extra argument that `$` and `.` emission introduces, so that position is
//! never judged always-cheap. `find_inline_candidates` selects small pure
//! functions (single clause, simple patterns, no guards, no where bindings,
//! cheap body, not self-recursive, no constructor applications) for
//! call-site inlining by inline.rs.

use crate::tir::*;
use super::CodeGen;
use super::util::{expr_references_name};

impl CodeGen {
    /// Whole-program call-site analysis. For each function, determine which
    /// parameter positions always receive cheap (non-thunk) arguments.
    pub(super) fn analyze_call_sites(&mut self, module: &TModule) {
        // Initialize: for each function, track (ever_thunked, ever_called) per param
        let mut ever_thunked: std::collections::HashMap<String, Vec<bool>> = std::collections::HashMap::new();
        let mut ever_called: std::collections::HashMap<String, Vec<bool>> = std::collections::HashMap::new();
        for func in module.functions.iter().chain(module.instance_fns.iter()) {
            let num_params = func.clauses.iter().map(|c| c.patterns.len()).max().unwrap_or(0);
            if num_params > 0 {
                ever_thunked.insert(func.name.clone(), vec![false; num_params]);
                ever_called.insert(func.name.clone(), vec![false; num_params]);
            }
        }
        // Scan all function bodies (and where-clause bodies) for call sites
        for func in module.functions.iter().chain(module.instance_fns.iter()) {
            for clause in &func.clauses {
                self.scan_call_sites(&clause.body, &mut ever_thunked, &mut ever_called);
                // Guard conditions and bodies are call sites too — without
                // scanning them, a recursive call that appears only inside a
                // guard (e.g. `f n | ... = f (g n)`) is missed, and the
                // parameter is wrongly judged always-cheap (concrete) while the
                // actual emission thunks the argument.
                for g in &clause.guards {
                    self.scan_call_sites(&g.condition, &mut ever_thunked, &mut ever_called);
                    self.scan_call_sites(&g.body, &mut ever_thunked, &mut ever_called);
                }
                for wb in &clause.where_binds {
                    self.scan_call_sites(&wb.body, &mut ever_thunked, &mut ever_called);
                }
            }
        }
        // A param is always-cheap only if it was called at least once and
        // never received a thunk at any call site.
        for (name, thunked) in &ever_thunked {
            if let Some(called) = ever_called.get(name) {
                let cheap: Vec<bool> = thunked.iter().zip(called.iter())
                    .map(|(t, c)| *c && !*t)
                    .collect();
                self.params_always_cheap.insert(name.clone(), cheap);
            }
        }
    }

    pub(super) fn scan_call_sites(&self, expr: &TExpr,
        ever_thunked: &mut std::collections::HashMap<String, Vec<bool>>,
        ever_called: &mut std::collections::HashMap<String, Vec<bool>>,
    ) {
        // Iterative right-spine walk for bind chains
        let mut expr = expr;
        loop {
            match &expr.kind {
                TExprKind::InfixApp { op, lhs, rhs } if op == ">>=" || op == ">>" => {
                    self.scan_call_sites(lhs, ever_thunked, ever_called);
                    if let TExprKind::Lambda { body, .. } = &rhs.kind {
                        expr = body;
                        continue;
                    }
                    expr = rhs;
                    continue;
                }
                TExprKind::Let { binds, body } => {
                    for bind in binds { self.scan_call_sites(&bind.body, ever_thunked, ever_called); }
                    expr = body;
                    continue;
                }
                _ => break,
            }
        }
        match &expr.kind {
            TExprKind::App(_, _) => {
                let mut args: Vec<&TExpr> = vec![];
                let mut f = expr;
                while let TExprKind::App(inner_f, inner_arg) = &f.kind {
                    args.push(inner_arg.as_ref());
                    f = inner_f.as_ref();
                }
                args.reverse();
                if let TExprKind::Var(name) = &f.kind
                    && let Some(thunked) = ever_thunked.get_mut(name.as_str()) {
                        let called = ever_called.get_mut(name.as_str()).unwrap();
                        for (i, arg) in args.iter().enumerate() {
                            if i < thunked.len() {
                                called[i] = true;
                                // A parameter is judged "always cheap" (the
                                // callee then skips forcing it and treats it as
                                // a value) only when EVERY call site passes an
                                // argument that arg_ast is guaranteed to
                                // evaluate eagerly regardless of context. That
                                // guarantee is the *context-free floor* of
                                // is_cheap_to_force: cheap structure built
                                // without leaning on any variable's WHNF-ness
                                // (var_ok = false) and free of trapping ops.
                                // arg_ast's eager set (strict OR
                                // is_cheap_to_force) is a superset of this, so
                                // whenever the callee assumes a value one was
                                // passed. Any other argument may be thunked by
                                // arg_ast, so mark the position thunked here.
                                if !Self::is_cheap_with(arg, &|_| false)
                                    || Self::contains_trapping_op(arg)
                                {
                                    thunked[i] = true;
                                }
                            }
                        }
                    }
                for arg in &args {
                    self.scan_call_sites(arg, ever_thunked, ever_called);
                }
                if !matches!(&f.kind, TExprKind::Var(_) | TExprKind::Con(_)) {
                    self.scan_call_sites(f, ever_thunked, ever_called);
                }
            }
            TExprKind::InfixApp { op, lhs, rhs } if op == "$" || op == "." => {
                // These operators emit hidden call sites (see their expr_ast
                // arms) that the generic recursion below cannot see:
                //   f $ x  calls f with a thunked x appended to f's spine args;
                //   f . g  builds a closure that calls g with the closure's raw
                //          parameter (possibly a thunk, when the composition is
                //          applied to a non-cheap argument) and calls f with
                //          g's result (which may itself be a raw thunk, e.g.
                //          when g just returns its lazy parameter).
                // Without registering them, a parameter that is cheap at every
                // direct call site but receives a thunk here is wrongly judged
                // always-cheap and emitted without __force. Mark one extra,
                // conservatively thunked argument position on each operand's
                // application spine.
                Self::mark_hidden_call_site(lhs, ever_thunked, ever_called);
                if op == "." {
                    Self::mark_hidden_call_site(rhs, ever_thunked, ever_called);
                }
                self.scan_call_sites(lhs, ever_thunked, ever_called);
                self.scan_call_sites(rhs, ever_thunked, ever_called);
            }
            TExprKind::InfixApp { lhs, rhs, .. } => {
                self.scan_call_sites(lhs, ever_thunked, ever_called);
                self.scan_call_sites(rhs, ever_thunked, ever_called);
            }
            TExprKind::Lambda { body, .. } => self.scan_call_sites(body, ever_thunked, ever_called),
            TExprKind::If { cond, then_branch, else_branch } => {
                self.scan_call_sites(cond, ever_thunked, ever_called);
                self.scan_call_sites(then_branch, ever_thunked, ever_called);
                self.scan_call_sites(else_branch, ever_thunked, ever_called);
            }
            TExprKind::Let { binds, body } => {
                for bind in binds { self.scan_call_sites(&bind.body, ever_thunked, ever_called); }
                self.scan_call_sites(body, ever_thunked, ever_called);
            }
            TExprKind::Case { scrutinee, branches } => {
                self.scan_call_sites(scrutinee, ever_thunked, ever_called);
                for b in branches {
                    for g in &b.guards {
                        self.scan_call_sites(&g.condition, ever_thunked, ever_called);
                        self.scan_call_sites(&g.body, ever_thunked, ever_called);
                    }
                    self.scan_call_sites(&b.body, ever_thunked, ever_called);
                }
            }
            TExprKind::Paren(inner) | TExprKind::Negate(inner) => self.scan_call_sites(inner, ever_thunked, ever_called),
            TExprKind::Tuple(elems) => { for e in elems { self.scan_call_sites(e, ever_thunked, ever_called); } }
            TExprKind::SpecCall { args, .. } => { for a in args { self.scan_call_sites(a, ever_thunked, ever_called); } }
            TExprKind::OutgoingCallback { callee, .. } => self.scan_call_sites(callee, ever_thunked, ever_called),
            TExprKind::FfiMaybeArg { value } => self.scan_call_sites(value, ever_thunked, ever_called),
            _ => {}
        }
    }

    /// Register the hidden call site that `$` and `.` emission creates for an
    /// operand (see the scan_call_sites `$`/`.` arm): the operand's spine
    /// callee receives one extra argument, in the position right after its
    /// explicit spine arguments, and that argument may be a thunk — so the
    /// position must never be judged always-cheap.
    pub(super) fn mark_hidden_call_site(
        operand: &TExpr,
        ever_thunked: &mut std::collections::HashMap<String, Vec<bool>>,
        ever_called: &mut std::collections::HashMap<String, Vec<bool>>,
    ) {
        let mut extra_pos = 0usize;
        let mut f = operand;
        loop {
            match &f.kind {
                TExprKind::Paren(inner) => f = inner.as_ref(),
                TExprKind::App(inner_f, _) => {
                    extra_pos += 1;
                    f = inner_f.as_ref();
                }
                _ => break,
            }
        }
        if let TExprKind::Var(name) = &f.kind
            && let Some(thunked) = ever_thunked.get_mut(name.as_str()) {
                let called = ever_called.get_mut(name.as_str()).unwrap();
                if extra_pos < thunked.len() {
                    called[extra_pos] = true;
                    thunked[extra_pos] = true;
                }
            }
    }

    /// Identify small pure functions eligible for inlining at call sites.
    /// Criteria: single clause, all-simple patterns, no guards, no where bindings,
    /// body is cheap, and not self-recursive.
    pub(super) fn find_inline_candidates(&mut self, module: &TModule) {
        for func in module.functions.iter().chain(module.instance_fns.iter()) {
            if func.clauses.len() != 1 { continue; }
            let clause = &func.clauses[0];
            if !clause.guards.is_empty() || !clause.where_binds.is_empty() { continue; }
            if clause.patterns.is_empty() { continue; } // value binding, not a function
            let all_simple = clause.patterns.iter().all(|p| matches!(p, TPattern::Var(_, _)));
            if !all_simple { continue; }
            if !Self::is_cheap(&clause.body) { continue; }
            if expr_references_name(&clause.body, &func.name) { continue; } // recursive
            // Only inline bodies that are arithmetic/comparison expressions,
            // not constructor applications (which need special expr_ast handling)
            if Self::body_has_constructors(&clause.body) { continue; }
            let params: Vec<String> = clause.patterns.iter().map(|p| {
                if let TPattern::Var(name, _) = p { name.clone() } else { unreachable!() }
            }).collect();
            self.inline_fns.insert(func.name.clone(), (params, clause.body.clone()));
        }
    }

    /// Check if an expression contains constructor applications (Con nodes).
    /// These need special handling in expr_ast (e.g. : → __mll_cons) that
    /// expr_subst_ast doesn't replicate, so we skip inlining for them.
    pub(super) fn body_has_constructors(expr: &TExpr) -> bool {
        match &expr.kind {
            TExprKind::Con(_) => true,
            TExprKind::App(f, a) => Self::body_has_constructors(f) || Self::body_has_constructors(a),
            TExprKind::InfixApp { lhs, rhs, .. } => Self::body_has_constructors(lhs) || Self::body_has_constructors(rhs),
            TExprKind::Paren(inner) | TExprKind::Negate(inner) => Self::body_has_constructors(inner),
            TExprKind::Tuple(elems) => elems.iter().any(Self::body_has_constructors),
            _ => false,
        }
    }
}
