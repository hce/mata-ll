//! Type inference: function/clause checking, pattern checking, expression
//! inference, exhaustiveness, and FFI signature validation. Moved verbatim
//! out of the monolithic typechecker.rs; `use super::*` keeps every name
//! resolution identical.

use super::*;

impl Checker {
    // --- Exhaustiveness checking ---

    /// Check if a list of patterns exhaustively covers a data type.
    /// Returns a list of missing constructor names, or empty if exhaustive.
    /// When `scrutinee_ty` is provided, GADT constructors whose return type
    /// cannot unify with it are excluded (they are unreachable).
    pub(super) fn check_exhaustiveness(&self, patterns: &[&Pattern], scrutinee_ty: Option<&Ty>) -> Vec<String> {
        // Collect constructor names, unwrapping parens, checking for catch-alls
        let mut seen_constructors: Vec<String> = Vec::new();
        let mut type_name: Option<String> = None;
        let mut has_literal = false;

        for p in patterns {
            self.collect_pattern_info(p, &mut seen_constructors, &mut type_name, &mut has_literal);
        }

        // If any pattern is a catch-all (variable/wildcard found), it's exhaustive
        if seen_constructors.contains(&"*".to_string()) { return vec![]; }

        // If we have literals, we can't check exhaustiveness
        if has_literal { return vec![]; }

        // If we have no constructors, nothing to check
        let type_name = match type_name {
            Some(t) => t,
            None => return vec![],
        };

        // Find all constructors for this type, filtering out GADT-unreachable ones
        let all_constructors: Vec<String> = self.constructors.iter()
            .filter(|(_, info)| info.type_name == type_name)
            .filter(|(_, info)| {
                // If we have a scrutinee type, check if this constructor's
                // result type can unify with it (i.e., is reachable)
                if let Some(sty) = scrutinee_ty {
                    unify(&info.result_type, sty).is_ok()
                } else {
                    true
                }
            })
            .map(|(name, _)| name.clone())
            .collect();

        // Return missing ones. Constructor keys are internal (a shadowing
        // local constructor is registered under a mangled key); report the
        // source name the user wrote.
        all_constructors.into_iter()
            .filter(|c| !seen_constructors.contains(c))
            .map(|c| c.strip_suffix(super::SHADOW_SUFFIX).unwrap_or(&c).to_string())
            .collect()
    }

    /// Recursively collect pattern info, unwrapping Paren wrappers.
    pub(super) fn collect_pattern_info(
        &self,
        pattern: &Pattern,
        seen: &mut Vec<String>,
        type_name: &mut Option<String>,
        has_literal: &mut bool,
    ) {
        match pattern {
            Pattern::Var(_) | Pattern::Wildcard => {
                // Use a sentinel to indicate catch-all
                if !seen.contains(&"*".to_string()) {
                    seen.push("*".to_string());
                }
            }
            Pattern::Constructor { name, .. } => {
                // Track by registered key so a shadowing local constructor is
                // compared against its own type's variants, not the shadowed one.
                let key = self.resolve_con_name(name);
                if let Some(info) = self.constructors.get(key) {
                    *type_name = Some(info.type_name.clone());
                    if !seen.iter().any(|s| s == key) {
                        seen.push(key.to_string());
                    }
                }
            }
            Pattern::LitPat(_) => { *has_literal = true; }
            Pattern::Paren(inner) => {
                self.collect_pattern_info(inner, seen, type_name, has_literal);
            }
            Pattern::Tuple(_) => {
                // Tuples are always exhaustive (single constructor)
                if !seen.contains(&"*".to_string()) {
                    seen.push("*".to_string());
                }
            }
        }
    }

    // --- Function checking ---

    pub(super) fn check_function(&mut self, name: &str, clauses: &[Clause], declared_ty: &Ty) -> Option<TFunction> {
        self.current_fn = Some(name.to_string());
        self.wanted.clear();
        self.binder_types.clear();
        let (fresh_ty, renames) = self.freshen_sig_type_mapped(declared_ty);

        // Re-express this function's declared constraints over the freshened
        // variable names, so a caller can match each constraint to the type it
        // instantiates the function at (and reject e.g. `print` of a function).
        if let Some(cs) = self.fn_constraints.get(name) {
            let renamed: Vec<TyConstraint> = cs.iter().map(|c| TyConstraint {
                class_name: c.class_name.clone(),
                type_var: renames.get(&c.type_var).cloned().unwrap_or_else(|| c.type_var.clone()),
            }).collect();
            self.fn_use_constraints.insert(name.to_string(), renamed);
        }

        // The declared context as (class, freshened variable) pairs. The variable
        // is the actual freshened signature TyVar (with its id), so it can be
        // resolved through the clause substitution at discharge time — a signature
        // variable often unifies with a fresh parameter variable, and a plain
        // name match would then miss it. Used to decide whether a wanted
        // constraint over a signature variable is provided by the context.
        let declared_cvars: Vec<(String, TyVar)> = {
            let name_to_var: HashMap<String, TyVar> = fresh_ty.free_vars()
                .into_iter().map(|v| (v.name.clone(), v)).collect();
            self.fn_constraints.get(name).cloned().unwrap_or_default().iter()
                .filter_map(|c| {
                    // A constraint var not in `renames` was already fresh in
                    // the signature (id != MAX) — freshen_sig_type_mapped left
                    // it alone. Instance-method signatures are like this:
                    // check_instance alpha-renames the instance's variables
                    // before specializing the method type, and declares the
                    // instance context over those same pre-freshened names.
                    let fresh_name = renames.get(&c.type_var).unwrap_or(&c.type_var);
                    let tv = name_to_var.get(fresh_name)?;
                    Some((c.class_name.clone(), tv.clone()))
                })
                .collect()
        };

        // Add self for recursion
        let self_scheme = self.generalize(&self.env.clone(), &fresh_ty);
        self.env.insert(name.to_string(), self_scheme);

        let mut tclauses = Vec::new();
        let mut overall_subst = Subst::empty();

        for (clause_idx, clause) in clauses.iter().enumerate() {
            let clause_ctx = if clauses.len() > 1 {
                format!("clause {} of '{}'", clause_idx + 1, name)
            } else {
                format!("definition of '{}'", name)
            };

            // Snapshot the wanted-constraint list: if the clause fails, its
            // substitution is discarded (only the error survives), so any
            // class constraints emitted while checking it reference type
            // variables whose determinations were lost with that substitution.
            // Discharging those orphaned constraints below would report them
            // as spuriously "ambiguous" (e.g. a scrutinee annotation that
            // fully pins a `decodeJSON` result no longer counts, because the
            // unification recording it died with the clause). Dropping them is
            // safe: the clause's own error is reported and fails compilation,
            // and once it is fixed the constraints are checked for real.
            let wanted_before = self.wanted.len();
            match self.check_clause(clause, &fresh_ty, &clause_ctx) {
                Ok((tc, clause_subst)) => {
                    tclauses.push(tc);
                    // Merge, don't just compose: each clause is checked against
                    // the same signature, so two clauses routinely bind the
                    // same signature variable to their own clause-local
                    // variables. Composition would keep the first clause's
                    // binding and silently drop the second's, severing the
                    // second clause's body types from the signature — its
                    // class constraints would escape the declared-context
                    // check, and the monomorphizer could never relate its
                    // types to a concrete instantiation. (GADT-style per-
                    // clause refinements conflict instead of unifying and
                    // deliberately stay clause-local — see Subst::merge.)
                    overall_subst = overall_subst.merge(&clause_subst);
                }
                Err(e) => {
                    self.wanted.truncate(wanted_before);
                    self.push_error_span(e, clause_ctx, clause.span);
                }
            }
        }

        // Apply the combined substitution to the function type and all clauses,
        // resolving type variables that were unified during clause checking.
        let final_ty = fresh_ty.apply_subst(&overall_subst);
        let tclauses: Vec<TClause> = tclauses.into_iter()
            .map(|c| c.apply_subst(&overall_subst))
            .collect();

        // Discharge wanted class constraints against the available instances.
        // has_instance defers on bare type variables (returns true), so a still-
        // polymorphic constraint is left for the caller; only a structurally
        // impossible one (a function, an action, an instance-less type) is
        // rejected — even when its components are not yet resolved, since e.g.
        // no function type has a Show instance regardless of its arg/result.
        let span = clauses.first().map(|c| c.span).unwrap_or_default();
        // A leftover constraint mentioning a variable that this binding is
        // genuinely polymorphic over (i.e. in its caller-visible type) is left
        // for the caller to discharge. A variable that is fixed by how some
        // value-level binder (parameter, let/where/lambda binding) is used is
        // likewise determined, even when it does not appear in the function's own
        // type. Only a variable that is neither — nothing in the definition or
        // its callers can ever fix it — makes the constraint ambiguous, so it is
        // rejected rather than silently dropped (matching Haskell 2010 / GHC).
        // This function's own signature-quantified variables (a leftover
        // constraint over one of these must be discharged by the declared
        // context) versus variables determined by a local binder.
        let type_vars = final_ty.free_vars();
        let mut determined = type_vars.clone();
        for bt in &self.binder_types {
            for v in bt.apply_subst(&overall_subst).free_vars() {
                if !determined.contains(&v) { determined.push(v); }
            }
        }
        for (class, cty) in std::mem::take(&mut self.wanted) {
            let rty = cty.apply_subst(&overall_subst);
            if !self.has_instance(&class, &rty) {
                // When the failure is an instance whose declared context is
                // unsatisfied, say WHICH context constraint failed and at
                // what type — "No instance for 'Show (Tree Blob)'" alone
                // hides that the real gap is 'Show Blob'.
                let ctx_note = self.context_failure_note(&class, &rty);
                self.push_error_span(
                    DiagnosticKind::NoInstance { class, ty: rty },
                    format!("definition of '{}'", name),
                    span,
                );
                if let (Some(note), Some(diag)) = (ctx_note, self.errors.last_mut()) {
                    diag.notes.push(note);
                }
            } else if is_structural_monad_class(&class) {
                // The monad-hierarchy classes are resolved structurally for IO
                // (mata-ll does not dictionary-pass them, and `has_instance`
                // reports no instance for IO on them), so an unresolved monad
                // variable from do-notation is IO by construction — neither a
                // value-level ambiguity nor a missing-context error.
            } else if rty.free_vars().iter().any(|v| !determined.contains(v)) {
                // A variable nothing determines: no instance can ever be chosen.
                self.push_error_span(
                    DiagnosticKind::AmbiguousType { class, ty: rty },
                    format!("definition of '{}'", name),
                    span,
                );
            } else {
                // Every variable is determined. A constraint that bottoms out at
                // one of this function's signature-quantified variables needs
                // evidence: a bare rigid variable has no instance, so the
                // declared context (or a superclass of it) must provide the
                // class. If it does not, reject and point at the missing
                // constraint — the reachable-but-unconstrained case GHC rejects.
                let mut required: Vec<(String, TyVar)> = Vec::new();
                self.collect_required_var_constraints(&class, &rty, &mut required);
                for (rc, v) in required {
                    if !type_vars.contains(&v) { continue; }
                    // Provided if some declared constraint, resolved through the
                    // clause substitution, lands on the same variable with a class
                    // that satisfies `rc` (directly or via a superclass).
                    let provided = declared_cvars.iter().any(|(dc, dv)| {
                        self.class_satisfies(dc, &rc)
                            && Ty::Var(dv.clone()).apply_subst(&overall_subst)
                                .free_vars().contains(&v)
                    });
                    if !provided {
                        self.push_error_span(
                            DiagnosticKind::MissingContextConstraint { class: rc, ty: Ty::Var(v) },
                            format!("definition of '{}'", name),
                            span,
                        );
                    }
                }
            }
        }

        // Check exhaustiveness of first argument patterns
        if !clauses.is_empty() && !clauses[0].patterns.is_empty() {
            let first_patterns: Vec<&Pattern> = clauses.iter()
                .map(|c| &c.patterns[0])
                .collect();
            // Extract the first argument type for GADT-aware exhaustiveness
            let first_arg_ty = if let Ty::Arrow(a, _) = &final_ty { Some(a.as_ref()) } else { None };
            let missing = self.check_exhaustiveness(&first_patterns, first_arg_ty);
            if !missing.is_empty() {
                self.push_error_span(
                    DiagnosticKind::NonExhaustive(format!(
                        "'{}': missing patterns for {}", name, missing.join(", ")
                    )),
                    format!("definition of '{}'", name),
                    clauses[0].span,
                );
            }
        }

        self.current_fn = None;

        if tclauses.is_empty() && !clauses.is_empty() {
            return None;
        }

        Some(TFunction {
            name: name.to_string(),
            ty: final_ty,
            clauses: tclauses,
            specialized: false,
            dict_params: vec![],
        })
    }

    pub(super) fn check_clause(&mut self, clause: &Clause, fun_ty: &Ty, ctx: &str) -> Result<(TClause, Subst), DiagnosticKind> {
        let mut local_env = self.env.clone();
        let mut remaining_ty = fun_ty.clone();
        let mut subst = Subst::empty();
        let mut tpatterns = Vec::new();

        for pattern in &clause.patterns {
            match &remaining_ty {
                Ty::Arrow(arg_ty, ret_ty) => {
                    let arg_ty = arg_ty.apply_subst(&subst);
                    let (tp, pat_subst) = self.check_pattern(pattern, &arg_ty, &mut local_env)?;
                    subst = subst.compose(&pat_subst);
                    remaining_ty = *ret_ty.clone();
                    tpatterns.push(tp);
                }
                _ => return Err(DiagnosticKind::Other("Too many arguments".into())),
            }
        }

        let expected_ret = remaining_ty.apply_subst(&subst);

        // Pre-register where-bound names so they're in scope for the body
        for ld in &clause.where_binds {
            if ld.patterns.is_empty() {
                let fresh = self.fresh_var("_wh");
                // A where value binder's type is determined by its body/uses.
                self.binder_types.push(fresh.clone());
                local_env.insert(ld.name.clone(), Scheme::mono(fresh));
            } else {
                // Local function: assign a fresh type for each parameter + return
                let mut fn_ty = self.fresh_var("_wr");
                for _ in &ld.patterns {
                    let param_ty = self.fresh_var("_wp");
                    fn_ty = Ty::arrow(param_ty, fn_ty);
                }
                // The whole local-function type (params + result) is determined
                // by its uses; record it so constraints from its body are not
                // spuriously flagged as ambiguous.
                self.binder_types.push(fn_ty.clone());
                local_env.insert(ld.name.clone(), Scheme::mono(fn_ty));
            }
        }

        let mut tguards = Vec::new();
        let tbody;

        if !clause.guards.is_empty() {
            for guard in &clause.guards {
                // Check the condition and body against the environment with the
                // accumulated substitution applied, so a binding used in both the
                // condition and the body (e.g. a `where` value under `| p x = … x`)
                // keeps a single, consistent instantiation. Without this the two
                // uses instantiate independently and one side's resolution is lost
                // when the substitutions compose, leaving a type variable — and any
                // class constraint on it — spuriously unresolved.
                let cond_env = local_env.apply_subst(&subst);
                let (tcond, cond_ty, s1) = self.infer_expr(&guard.condition, &cond_env)?;
                let s2 = unify(&cond_ty.apply_subst(&s1), &Ty::Con("Bool".into()))?;
                let combined = s1.compose(&s2);
                subst = subst.compose(&combined);
                let body_env = local_env.apply_subst(&subst);
                let ret = expected_ret.apply_subst(&subst);
                let (tbody_g, body_s) = self.check_expr_typed(&guard.body, &ret, &body_env)?;
                subst = subst.compose(&body_s);
                tguards.push(TGuard { condition: tcond, body: tbody_g });
            }
            tbody = TExpr::new(TExprKind::Var("undefined".into()), expected_ret);
        } else {
            let body_env = local_env.apply_subst(&subst);
            let ret = expected_ret.apply_subst(&subst);
            let (tb, body_s) = self.check_expr_typed(&clause.body, &ret, &body_env)?;
            subst = subst.compose(&body_s);
            tbody = tb;
        }

        // Type-check where bindings fully, accumulating substitutions
        let mut twhere = Vec::new();
        for ld in &clause.where_binds {
            if ld.patterns.is_empty() {
                // Simple value binding: where x = expr
                // On failure, record the error and continue with a placeholder:
                // the error makes compilation fail before codegen, and carrying
                // on lets one pass report errors in later bindings too.
                let mut binding_errored = false;
                // On failure the binding's substitution is lost (we continue
                // with Subst::empty()), so class constraints emitted while
                // inferring its body reference variables whose determinations
                // are gone — discharging them would report spurious
                // ambiguities on top of the real error. Drop them; they are
                // re-checked for real once the reported error is fixed.
                let wanted_before = self.wanted.len();
                let (texpr, inferred_ty, s) = self.infer_expr(&ld.body, &local_env).unwrap_or_else(|e| {
                    self.wanted.truncate(wanted_before);
                    self.push_error_span(
                        e,
                        format!("the where-binding '{}' ({})", ld.name, ctx),
                        clause.span,
                    );
                    binding_errored = true;
                    (TExpr::new(TExprKind::Var("error".into()), Ty::Unit), Ty::Unit, Subst::empty())
                });
                subst = subst.compose(&s);
                // Unify with the pre-registered fresh type. That fresh type has
                // absorbed how the clause body USES the binding, so a failure
                // here means the binding's definition doesn't match its use —
                // a real type error that must be reported, not dropped (unless
                // the body already failed above, where a second message about
                // the Unit placeholder would only be noise).
                if let Some(scheme) = local_env.lookup(&ld.name) {
                    match unify(&scheme.ty.apply_subst(&subst), &inferred_ty.apply_subst(&subst)) {
                        Ok(us) => subst = subst.compose(&us),
                        Err(e) => if !binding_errored {
                            self.push_error_span(
                                e,
                                format!("the where-binding '{}' ({})", ld.name, ctx),
                                clause.span,
                            );
                        }
                    }
                }
                twhere.push(TLocalDef {
                    name: ld.name.clone(),
                    patterns: vec![],
                    body: texpr,
                });
            } else {
                // Local function: where go acc [] = ...
                let mut fn_env = local_env.clone();
                let mut param_tys = Vec::new();
                let mut tpatterns = Vec::new();
                let mut where_subst = Subst::empty();
                for pat in &ld.patterns {
                    let param_ty = self.fresh_var("_w");
                    // On failure, record the error and continue with a wildcard
                    // placeholder: the error makes compilation fail before
                    // codegen, and carrying on lets one pass report errors in
                    // later patterns and bindings too.
                    let (tp, ps) = self.check_pattern(pat, &param_ty, &mut fn_env).unwrap_or_else(|e| {
                        self.push_error_span(
                            e,
                            format!("a pattern of the where-binding '{}' ({})", ld.name, ctx),
                            clause.span,
                        );
                        (TPattern::Wildcard, Subst::empty())
                    });
                    where_subst = where_subst.compose(&ps);
                    param_tys.push(param_ty.apply_subst(&where_subst));
                    tpatterns.push(tp);
                }
                // Same recovery as the value-binding case above: record, then
                // continue with a placeholder that can never reach codegen.
                let mut binding_errored = false;
                // As in the value-binding case: the failed body's substitution
                // is lost, so drop the class constraints it emitted rather
                // than report them as spuriously ambiguous.
                let wanted_before = self.wanted.len();
                let (texpr, body_ty, bs) = self.infer_expr(&ld.body, &fn_env).unwrap_or_else(|e| {
                    self.wanted.truncate(wanted_before);
                    self.push_error_span(
                        e,
                        format!("the where-binding '{}' ({})", ld.name, ctx),
                        clause.span,
                    );
                    binding_errored = true;
                    (TExpr::new(TExprKind::Var("error".into()), Ty::Unit), Ty::Unit, Subst::empty())
                });
                where_subst = where_subst.compose(&bs);
                // Propagate the local-function body's unifications to the outer
                // substitution. Without this, the resolutions that fix a where
                // function's parameter types (and any class-method type variables
                // in its body) are visible only inside the emitted term, leaving
                // those variables spuriously unresolved at the function boundary.
                subst = subst.compose(&where_subst);
                // Build the inferred function type and unify with pre-registered type
                let mut inferred_fn_ty = body_ty.apply_subst(&where_subst);
                for pty in param_tys.iter().rev() {
                    inferred_fn_ty = Ty::arrow(pty.apply_subst(&where_subst), inferred_fn_ty);
                }
                // As in the value-binding case: the pre-registered type carries
                // how the clause body uses this local function, so a unification
                // failure is a definition-vs-use type error and must be recorded
                // (suppressed only when the body already failed, to avoid a
                // second message about the placeholder).
                if let Some(scheme) = local_env.lookup(&ld.name) {
                    match unify(&scheme.ty.apply_subst(&subst), &inferred_fn_ty.apply_subst(&subst)) {
                        Ok(us) => subst = subst.compose(&us),
                        Err(e) => if !binding_errored {
                            self.push_error_span(
                                e,
                                format!("the where-binding '{}' ({})", ld.name, ctx),
                                clause.span,
                            );
                        }
                    }
                }
                twhere.push(TLocalDef {
                    name: ld.name.clone(),
                    patterns: tpatterns.into_iter().map(|p| p.apply_subst(&where_subst)).collect(),
                    body: texpr.apply_subst(&where_subst),
                });
            }
        }

        // Apply the accumulated substitution to the entire clause,
        // keeping the source clause's location so downstream passes (the
        // monomorphizer) can locate their diagnostics.
        let raw_clause = TClause {
            patterns: tpatterns,
            guards: tguards,
            body: tbody,
            where_binds: twhere,
            span: Some(clause.span),
        };
        Ok((raw_clause.apply_subst(&subst), subst))
    }

    /// Infer a `let` / do-`let` binding group as **mutually recursive**, then
    /// generalize each binding over the outer environment (let-polymorphism).
    ///
    /// Every name is pre-registered with a fresh monomorphic type variable
    /// before any body is inferred, so a binding may reference itself and its
    /// siblings (e.g. `fib = [1,1] ++ zipWith (+) fib (drop 1 fib)`). This
    /// mirrors the recursive `where`/top-level handling; the difference is the
    /// final generalization step, which `where` omits but `let` needs to keep
    /// `let i = \x -> x in (i 1, i True)` working.
    ///
    /// Returns the typed bindings, the environment extended with the
    /// generalized schemes (for the body / subsequent statements), and the
    /// accumulated substitution. Only value bindings (no parameters) are
    /// supported, matching the rest of the `let` pipeline; bindings with
    /// parameters are inferred as-is and will surface the same errors as before.
    pub(super) fn infer_let_group(
        &mut self,
        binds: &[LocalDef],
        env: &TypeEnv,
        mut subst: Subst,
    ) -> Result<(Vec<TLocalDef>, TypeEnv, Subst), DiagnosticKind> {
        // Pre-register fresh monomorphic vars for the whole group so bindings
        // can see themselves and each other during inference.
        let mut rec_env = env.clone();
        let mut fresh_tys: Vec<Ty> = Vec::with_capacity(binds.len());
        for bind in binds {
            let fv = self.fresh_var("_let");
            fresh_tys.push(fv.clone());
            // A let binder's type is determined by its body/uses; record it so a
            // class constraint over its variable is not flagged as ambiguous.
            self.binder_types.push(fv.clone());
            rec_env.insert(bind.name.clone(), Scheme::mono(fv));
        }

        // Infer each body in the recursive environment and unify its type with
        // the pre-registered variable.
        let mut tbinds = Vec::new();
        for (i, bind) in binds.iter().enumerate() {
            let env_i = rec_env.apply_subst(&subst);
            let (te, bind_ty, s) = self.infer_expr(&bind.body, &env_i)?;
            subst = subst.compose(&s);
            let us = unify(&fresh_tys[i].apply_subst(&subst), &bind_ty.apply_subst(&subst))?;
            subst = subst.compose(&us);
            tbinds.push(TLocalDef { name: bind.name.clone(), patterns: vec![], body: te });
        }

        // Generalize each binding over the outer environment (excluding the
        // group's own monomorphic vars), then extend the environment.
        let outer_env = env.apply_subst(&subst);
        let mut out_env = outer_env.clone();
        for (i, bind) in binds.iter().enumerate() {
            let scheme = self.generalize(&outer_env, &fresh_tys[i].apply_subst(&subst));
            out_env.insert(bind.name.clone(), scheme);
        }

        Ok((tbinds, out_env, subst))
    }

    // --- Pattern checking (returns typed pattern) ---

    pub(super) fn check_pattern(
        &mut self, pattern: &Pattern, expected: &Ty, env: &mut TypeEnv,
    ) -> Result<(TPattern, Subst), DiagnosticKind> {
        match pattern {
            Pattern::Var(name) => {
                // Rank-2: if expected type is forall-quantified, bind as polymorphic scheme
                let scheme = Self::forall_to_scheme(expected);
                env.insert(name.clone(), scheme);
                // This binder's type variables are determined by how it is used,
                // so record it for the ambiguity check at the function boundary.
                self.binder_types.push(expected.clone());
                Ok((TPattern::Var(name.clone(), expected.clone()), Subst::empty()))
            }
            Pattern::Wildcard => Ok((TPattern::Wildcard, Subst::empty())),
            Pattern::LitPat(lit) => {
                let lit_ty = self.literal_type(lit);
                let s = unify(expected, &lit_ty)?;
                Ok((TPattern::LitPat(Self::convert_literal(lit)), s))
            }
            Pattern::Constructor { name, args } => {
                // Resolve the source name to its registered key (a local
                // constructor shadowing a Prelude/import one lives under a
                // mangled key); diagnostics keep the source name.
                let con_key = self.resolve_con_name(name).to_string();
                let con_info = self.constructors.get(&con_key)
                    .ok_or_else(|| DiagnosticKind::UnboundConstructor(name.clone()))?.clone();

                if args.len() != con_info.field_types.len() {
                    return Err(DiagnosticKind::PatternArgCount {
                        constructor: name.clone(), expected: con_info.field_types.len(), got: args.len(),
                    });
                }

                let mut tv_map = HashMap::new();
                for tv in &con_info.type_vars {
                    if let Ty::Var(fresh) = self.fresh_var("_p") {
                        tv_map.insert(tv.clone(), Ty::Var(fresh));
                    }
                }
                // Existential type variables get fresh (skolem-like) variables
                // that are local to this pattern match branch
                for tv in &con_info.existential_vars {
                    if let Ty::Var(fresh) = self.fresh_var("_ex") {
                        tv_map.insert(tv.clone(), Ty::Var(fresh));
                    }
                }
                let tv_subst = Subst::from_map(tv_map);
                let result_ty = con_info.result_type.apply_subst(&tv_subst);
                let mut subst = unify(expected, &result_ty)?;

                let mut targs = Vec::new();
                for (arg_pat, field_ty) in args.iter().zip(&con_info.field_types) {
                    let expected_field = field_ty.apply_subst(&tv_subst).apply_subst(&subst);
                    let (tp, s) = self.check_pattern(arg_pat, &expected_field, env)?;
                    subst = subst.compose(&s);
                    targs.push(tp);
                }

                Ok((TPattern::Constructor { name: con_key, args: targs }, subst))
            }
            Pattern::Paren(inner) => self.check_pattern(inner, expected, env),
            Pattern::Tuple(pats) => {
                // Expect a Tuple type with matching arity
                let elem_types: Vec<Ty> = pats.iter().map(|_| self.fresh_var("_t")).collect();
                let tuple_ty = Ty::Tuple(elem_types.clone());
                let s = unify(expected, &tuple_ty)?;
                let mut subst = s;
                let mut tpats = Vec::new();
                for (p, et) in pats.iter().zip(elem_types.iter()) {
                    let et_resolved = et.apply_subst(&subst);
                    let (tp, ps) = self.check_pattern(p, &et_resolved, env)?;
                    subst = subst.compose(&ps);
                    tpats.push(tp);
                }
                Ok((TPattern::Tuple(tpats), subst))
            }
        }
    }

    // --- Expression inference (returns typed expr) ---

    pub(super) fn infer_expr(&mut self, expr: &Expr, env: &TypeEnv) -> Result<(TExpr, Ty, Subst), DiagnosticKind> {
        match expr {
            Expr::Var(name) => {
                if self.enforce_hidden && self.hidden_names.contains(name) {
                    return Err(DiagnosticKind::Other(
                        format!("'{}' is not exported by its module", name)));
                }
                if let Some(scheme) = env.lookup(name) {
                    let scheme = scheme.clone();
                    let (ty, inst_map) = self.instantiate_with_map(&scheme);
                    self.emit_use_constraints(name, &inst_map);
                    Ok((TExpr::new(TExprKind::Var(name.clone()), ty.clone()), ty, Subst::empty()))
                } else {
                    Err(DiagnosticKind::UnboundVariable(name.clone()))
                }
            }
            Expr::Con(name) => {
                // Resolve to the registered key (shadowing, see check_pattern);
                // the TIR carries the key so codegen picks the right tag.
                let con_key = self.resolve_con_name(name).to_string();
                if let Some(scheme) = env.lookup(&con_key) {
                    let ty = self.instantiate(scheme);
                    Ok((TExpr::new(TExprKind::Con(con_key), ty.clone()), ty, Subst::empty()))
                } else {
                    Err(DiagnosticKind::UnboundConstructor(name.clone()))
                }
            }
            Expr::Lit(lit) => {
                let ty = self.literal_type(lit);
                Ok((TExpr::new(TExprKind::Lit(Self::convert_literal(lit)), ty.clone()), ty, Subst::empty()))
            }
            Expr::App(func, arg) => {
                let (tf, func_ty, s1) = self.infer_expr(func, env)?;
                let env2 = env.apply_subst(&s1);
                let (ta, arg_ty, s2) = self.infer_expr(arg, &env2)?;
                let ret_ty = self.fresh_var("_r");
                let func_ty = func_ty.apply_subst(&s2);

                // Rank-2: if the function expects a forall-quantified argument,
                // skolemize the quantified variable and check the argument against it
                let s3 = if let Ty::Arrow(ref param_ty, ref func_ret) = func_ty {
                    if let Ty::Forall(..) = **param_ty {
                        // Collect all forall-bound variables (handles forall a b. T)
                        let mut skolems = vec![];
                        let mut current: &Ty = param_ty;
                        let mut vars = vec![];
                        while let Ty::Forall(v, inner) = current {
                            vars.push(v.clone());
                            current = inner;
                        }
                        // Create skolems for each bound variable, substitute into body
                        let mut skolem_body = current.clone();
                        for var in &vars {
                            let sk_id = self.next_var;
                            self.next_var += 1;
                            skolems.push((var.name.clone(), sk_id));
                            let sk = Ty::Skolem(var.name.clone(), sk_id);
                            skolem_body = skolem_body.apply_subst(
                                &Subst::singleton(var.clone(), sk),
                            );
                        }
                        // Directly check the argument against the skolemized param type
                        let s_arg = unify(&arg_ty, &skolem_body)?;
                        // Connect return type
                        let s_ret = unify(&ret_ty, &func_ret.apply_subst(&s_arg))?;
                        let combined = s_arg.compose(&s_ret);
                        // Escape check: skolems must not appear in the return type
                        let final_ret = ret_ty.apply_subst(&combined);
                        for (sk_name, sk_id) in &skolems {
                            if final_ret.contains_skolem(sk_name, *sk_id) {
                                return Err(DiagnosticKind::Other(format!(
                                    "Rigid type variable '{}' escapes its scope", sk_name
                                )));
                            }
                        }
                        combined
                    } else {
                        unify(&func_ty, &Ty::arrow(arg_ty, ret_ty.clone()))?
                    }
                } else {
                    unify(&func_ty, &Ty::arrow(arg_ty, ret_ty.clone()))?
                };

                let final_ty = ret_ty.apply_subst(&s3);
                Ok((
                    TExpr::new(TExprKind::App(Box::new(tf), Box::new(ta)), final_ty.clone()),
                    final_ty,
                    s1.compose(&s2).compose(&s3),
                ))
            }
            Expr::InfixApp { op, lhs, rhs } => {
                // Detect bind chains (from do-blocks) and process iteratively
                // to avoid stack overflow on deeply nested expressions.
                if (op == ">>=" || op == ">>") && self.is_bind_chain(rhs) {
                    return self.infer_bind_chain(expr, env);
                }

                // Desugar to App(App(op, lhs), rhs) for type inference
                let op_expr = if env.lookup(op).is_some() {
                    Expr::Var(op.clone())
                } else {
                    Expr::OpFunc(op.clone())
                };
                let desugared = Expr::App(
                    Box::new(Expr::App(Box::new(op_expr), Box::new(*lhs.clone()))),
                    Box::new(*rhs.clone()),
                );
                let (te, ty, subst) = self.infer_expr(&desugared, env)?;
                // Reconstruct as InfixApp in the TIR for codegen
                if let TExprKind::App(f, rhs_t) = te.kind
                    && let TExprKind::App(_, lhs_t) = f.kind {
                        return Ok((
                            TExpr::new(TExprKind::InfixApp {
                                op: op.clone(), lhs: lhs_t, rhs: rhs_t,
                            }, ty.clone()),
                            ty, subst,
                        ));
                    }
                // Fallback: just return the desugared form
                let (te2, ty2, subst2) = self.infer_expr(&desugared, env)?;
                Ok((te2, ty2, subst2))
            }
            Expr::Negate(inner) => {
                let (te, ty, s) = self.infer_expr(inner, env)?;
                Ok((TExpr::new(TExprKind::Negate(Box::new(te)), ty.clone()), ty, s))
            }
            Expr::Lambda { params, body } => {
                let mut local_env = env.clone();
                let mut param_info = Vec::new();
                for param in params {
                    let param_ty = self.fresh_var("_l");
                    if param != "_" {
                        local_env.insert(param.clone(), Scheme::mono(param_ty.clone()));
                    }
                    param_info.push((param.clone(), param_ty));
                }
                let (tbody, body_ty, subst) = self.infer_expr(body, &local_env)?;
                let func_ty = param_info.iter().rev().fold(body_ty, |acc, (_, pt)| {
                    Ty::arrow(pt.apply_subst(&subst), acc)
                });
                let typed_params: Vec<(String, Ty)> = param_info.iter()
                    .map(|(n, t)| (n.clone(), t.apply_subst(&subst)))
                    .collect();
                Ok((
                    TExpr::new(TExprKind::Lambda { params: typed_params, body: Box::new(tbody) }, func_ty.clone()),
                    func_ty, subst,
                ))
            }
            Expr::If { cond, then_branch, else_branch } => {
                let (tc, cond_ty, s1) = self.infer_expr(cond, env)?;
                let sb = unify(&cond_ty, &Ty::Con("Bool".into()))?;
                let s1 = s1.compose(&sb);
                let env2 = env.apply_subst(&s1);
                let (tt, then_ty, s2) = self.infer_expr(then_branch, &env2)?;
                let env3 = env2.apply_subst(&s2);
                let (te, else_ty, s3) = self.infer_expr(else_branch, &env3)?;
                let s4 = unify(&then_ty.apply_subst(&s3), &else_ty)?;
                let final_ty = then_ty.apply_subst(&s3).apply_subst(&s4);
                Ok((
                    TExpr::new(TExprKind::If {
                        cond: Box::new(tc), then_branch: Box::new(tt), else_branch: Box::new(te),
                    }, final_ty.clone()),
                    final_ty, s1.compose(&s2).compose(&s3).compose(&s4),
                ))
            }
            Expr::Case { scrutinee, branches } => {
                let (ts, scrut_ty, s1) = self.infer_expr(scrutinee, env)?;
                let result_ty = self.fresh_var("_c");
                let mut subst = s1;
                let mut tbranches = Vec::new();

                for branch in branches {
                    let mut branch_env = env.apply_subst(&subst);
                    let scrut_ty = scrut_ty.apply_subst(&subst);
                    let (tp, pat_subst) = self.check_pattern(&branch.pattern, &scrut_ty, &mut branch_env)?;
                    subst = subst.compose(&pat_subst);

                    // A branch may carry guards (`pat | g1 -> e1 | g2 -> e2`).
                    // Each guard condition must be Bool and each guard body must
                    // agree with the case result type, exactly as for function
                    // clause guards. When guards are present the branch body is
                    // the synthetic `undefined` fallthrough produced by the parser.
                    let mut tguards = Vec::new();
                    if !branch.guards.is_empty() {
                        for guard in &branch.guards {
                            let genv = branch_env.apply_subst(&subst);
                            let (tcond, cond_ty, gs1) = self.infer_expr(&guard.condition, &genv)?;
                            let gs2 = unify(&cond_ty.apply_subst(&gs1), &Ty::Con("Bool".into()))?;
                            subst = subst.compose(&gs1).compose(&gs2);
                            let genv2 = branch_env.apply_subst(&subst);
                            let (tgbody, gbody_ty, gbs) = self.infer_expr(&guard.body, &genv2)?;
                            subst = subst.compose(&gbs);
                            let gu = unify(&result_ty.apply_subst(&subst), &gbody_ty)?;
                            subst = subst.compose(&gu);
                            tguards.push(TGuard { condition: tcond, body: tgbody });
                        }
                    }

                    let (tb, body_ty, body_subst) = self.infer_expr(&branch.body, &branch_env)?;
                    subst = subst.compose(&body_subst);
                    let s = unify(&result_ty.apply_subst(&subst), &body_ty)?;
                    subst = subst.compose(&s);
                    tbranches.push(TCaseBranch { pattern: tp, guards: tguards, body: tb });
                }

                // Check exhaustiveness of case patterns
                let case_patterns: Vec<&Pattern> = branches.iter()
                    .map(|b| &b.pattern)
                    .collect();
                let resolved_scrut_ty = scrut_ty.apply_subst(&subst);
                let missing = self.check_exhaustiveness(&case_patterns, Some(&resolved_scrut_ty));
                if !missing.is_empty() {
                    let fn_name = self.current_fn.clone().unwrap_or_else(|| "<expr>".into());
                    self.push_error_ctx(
                        DiagnosticKind::NonExhaustive(format!(
                            "case expression in '{}': missing patterns for {}",
                            fn_name, missing.join(", ")
                        )),
                        format!("definition of '{}'", fn_name),
                    );
                }

                let final_ty = result_ty.apply_subst(&subst);
                Ok((
                    TExpr::new(TExprKind::Case { scrutinee: Box::new(ts), branches: tbranches }, final_ty.clone()),
                    final_ty, subst,
                ))
            }
            Expr::Let { binds, body } => {
                let (tbinds, body_env, subst) =
                    self.infer_let_group(binds, env, Subst::empty())?;
                let (tbody, body_ty, s) = self.infer_expr(body, &body_env)?;
                Ok((
                    TExpr::new(TExprKind::Let { binds: tbinds, body: Box::new(tbody) }, body_ty.clone()),
                    body_ty, subst.compose(&s),
                ))
            }
            Expr::Do(_) => unreachable!("Do should be desugared to >>= before type checking"),
            Expr::Paren(inner) => {
                let (te, ty, s) = self.infer_expr(inner, env)?;
                Ok((TExpr::new(TExprKind::Paren(Box::new(te)), ty.clone()), ty, s))
            }
            Expr::OpFunc(op) => {
                if let Some(scheme) = env.lookup(op) {
                    let ty = self.instantiate(scheme);
                    Ok((TExpr::new(TExprKind::OpFunc(op.clone()), ty.clone()), ty, Subst::empty()))
                } else {
                    Err(DiagnosticKind::UnboundVariable(format!("({})", op)))
                }
            }
            Expr::Ascription(inner, declared_ty) => {
                let expected = self.ast_type_to_ty(declared_ty);
                let expected = self.freshen_sig_type(&expected);
                let (te, inferred, subst) = self.infer_expr(inner, env)?;
                let s = unify(&inferred, &expected)?;
                let final_ty = inferred.apply_subst(&s);
                let full_subst = subst.compose(&s);
                let resolved_te = te.apply_subst(&full_subst);
                Ok((resolved_te, final_ty, full_subst))
            }
            Expr::RecordCon { constructor, fields } => {
                // Desugar to positional application by reordering fields
                // to match the data declaration order
                let con_info = self.constructors.get(self.resolve_con_name(constructor))
                    .ok_or_else(|| DiagnosticKind::UnboundConstructor(constructor.clone()))?.clone();

                // Collect field names with their index from the record_fields table
                let mut field_order: Vec<(String, usize)> = Vec::new();
                for (field_name, (type_name, idx)) in &self.record_fields {
                    if *type_name == con_info.type_name {
                        field_order.push((field_name.clone(), *idx));
                    }
                }
                // Sort by index to get declaration order
                field_order.sort_by_key(|(_, idx)| *idx);

                // Build positional arguments in declaration order
                let num_fields = field_order.len();
                let mut ordered_args: Vec<Option<&Expr>> = vec![None; num_fields];
                for (name, val) in fields {
                    let pos = field_order.iter().position(|(n, _)| n == name);
                    match pos {
                        Some(i) => ordered_args[i] = Some(val),
                        None => return Err(DiagnosticKind::Other(format!(
                            "Unknown field '{}' for constructor '{}'", name, constructor
                        ))),
                    }
                }

                // Check all fields are provided
                for (i, arg) in ordered_args.iter().enumerate() {
                    if arg.is_none() {
                        return Err(DiagnosticKind::Other(format!(
                            "Missing field '{}' in constructor '{}'",
                            field_order[i].0, constructor
                        )));
                    }
                }

                // Desugar to App(App(Con(name), arg1), arg2) ...
                let desugared = ordered_args.iter().fold(
                    Expr::Con(constructor.clone()),
                    |acc, arg| Expr::App(Box::new(acc), Box::new(arg.unwrap().clone())),
                );
                self.infer_expr(&desugared, env)
            }
            Expr::RecordUpdate { expr, updates } => {
                // Infer the record expression type
                let (rec_te, rec_ty, mut subst) = self.infer_expr(expr, env)?;

                // Determine the type name from the first update field
                let first_field = &updates[0].0;
                let (type_name, _) = self.record_fields.get(first_field)
                    .ok_or_else(|| DiagnosticKind::Other(format!(
                        "Unknown record field '{}'", first_field
                    )))?.clone();

                // Collect all fields for this type with their indices
                let mut all_fields: Vec<(String, usize)> = Vec::new();
                for (field_name, (tn, idx)) in &self.record_fields {
                    if *tn == type_name {
                        all_fields.push((field_name.clone(), *idx));
                    }
                }
                let num_fields = all_fields.len();

                // Process update expressions and verify fields
                let mut typed_updates = Vec::new();
                for (field_name, field_expr) in updates {
                    let (field_tn, field_idx) = self.record_fields.get(field_name)
                        .ok_or_else(|| DiagnosticKind::Other(format!(
                            "Unknown record field '{}'", field_name
                        )))?.clone();
                    if field_tn != type_name {
                        return Err(DiagnosticKind::Other(format!(
                            "Field '{}' belongs to type '{}', not '{}'",
                            field_name, field_tn, type_name
                        )));
                    }
                    let env2 = env.apply_subst(&subst);
                    let (te, _ty, s) = self.infer_expr(field_expr, &env2)?;
                    subst = subst.compose(&s);
                    typed_updates.push((field_name.clone(), field_idx, te));
                }

                let result_ty = rec_ty.apply_subst(&subst);
                Ok((TExpr::new(TExprKind::RecordUpdate {
                    record: Box::new(rec_te),
                    updates: typed_updates,
                    num_fields,
                }, result_ty.clone()), result_ty, subst))
            }
            Expr::Tuple(elems) => {
                let mut telems = Vec::new();
                let mut elem_types = Vec::new();
                let mut subst = Subst::empty();
                for e in elems {
                    let env2 = env.apply_subst(&subst);
                    let (te, ty, s) = self.infer_expr(e, &env2)?;
                    subst = subst.compose(&s);
                    elem_types.push(ty);
                    telems.push(te);
                }
                let tuple_ty = Ty::Tuple(elem_types);
                Ok((TExpr::new(TExprKind::Tuple(telems), tuple_ty.clone()), tuple_ty, subst))
            }
        }
    }

    /// Check if an expression is part of a bind chain (from do-block desugaring).
    pub(super) fn is_bind_chain(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Lambda { body, .. } => {
                match body.as_ref() {
                    Expr::InfixApp { op, .. } if op == ">>=" || op == ">>" => true,
                    Expr::Let { body, .. } => matches!(body.as_ref(),
                        Expr::InfixApp { op, .. } if op == ">>=" || op == ">>"),
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// Type-check a bind chain iteratively. Flattens the right-spine of
    /// InfixApp(>>=)/InfixApp(>>)/Let into a heap-allocated Vec and processes
    /// each statement in a loop, avoiding deep stack recursion.
    pub(super) fn infer_bind_chain(&mut self, expr: &Expr, env: &TypeEnv) -> Result<(TExpr, Ty, Subst), DiagnosticKind> {
        // Flatten the bind chain into a list of statements
        enum BindStmt<'a> {
            Bind { op: &'a str, lhs: &'a Expr, param: &'a str, },
            Let { binds: &'a [LocalDef] },
        }

        let mut stmts: Vec<BindStmt> = Vec::new();
        let mut current = expr;

        loop {
            match current {
                Expr::InfixApp { op, lhs, rhs } if op == ">>=" || op == ">>" => {
                    if let Expr::Lambda { params, body } = rhs.as_ref() {
                        stmts.push(BindStmt::Bind { op, lhs, param: &params[0] });
                        current = body;
                        continue;
                    }
                    // >>= without Lambda rhs — not a bind chain continuation
                    stmts.push(BindStmt::Bind { op, lhs, param: "_" });
                    current = rhs;
                    break;
                }
                Expr::Let { binds, body } => {
                    stmts.push(BindStmt::Let { binds });
                    current = body;
                    continue;
                }
                _ => break,
            }
        }

        // Process each statement iteratively
        let mut local_env = env.clone();
        let mut subst = Subst::empty();
        // Collect typed results to reconstruct bottom-up
        struct TypedBind {
            op: String,
            lhs_te: TExpr,
            param: String,
            param_ty: Ty,
        }
        enum TypedStmt {
            Bind(TypedBind),
            Let(Vec<TLocalDef>),
        }
        let mut typed_stmts: Vec<TypedStmt> = Vec::new();

        for stmt in &stmts {
            match stmt {
                BindStmt::Bind { op, lhs, param } => {
                    // Type-check: lhs >>= \param -> rest
                    // Desugar the single InfixApp to App(App(op, lhs), rhs_placeholder)
                    let op_expr = if local_env.lookup(op).is_some() {
                        Expr::Var(op.to_string())
                    } else {
                        Expr::OpFunc(op.to_string())
                    };
                    // Infer op type
                    let (_top, op_ty, s_op) = self.infer_expr(&op_expr, &local_env)?;
                    subst = subst.compose(&s_op);
                    local_env = local_env.apply_subst(&s_op);

                    // Infer lhs type
                    let (tlhs, lhs_ty, s_lhs) = self.infer_expr(lhs, &local_env)?;
                    subst = subst.compose(&s_lhs);
                    local_env = local_env.apply_subst(&s_lhs);

                    // Unify: op_ty ~ lhs_ty -> (param_ty -> result_ty) -> result_ty
                    let param_ty = self.fresh_var("_bp");
                    let result_ty = self.fresh_var("_br");
                    let op_ty = op_ty.apply_subst(&s_lhs);
                    let expected_op = Ty::arrow(lhs_ty, Ty::arrow(
                        Ty::arrow(param_ty.clone(), result_ty.clone()),
                        result_ty.clone(),
                    ));
                    let s_unify = unify(&op_ty, &expected_op)?;
                    subst = subst.compose(&s_unify);
                    local_env = local_env.apply_subst(&s_unify);
                    let bound_ty = param_ty.apply_subst(&s_unify);

                    // Bind parameter
                    if *param != "_" {
                        local_env.insert(param.to_string(), Scheme::mono(bound_ty.clone()));
                    }

                    typed_stmts.push(TypedStmt::Bind(TypedBind {
                        op: op.to_string(),
                        lhs_te: tlhs,
                        param: param.to_string(),
                        param_ty: bound_ty,
                    }));
                }
                BindStmt::Let { binds } => {
                    let (tbinds, new_env, new_subst) =
                        self.infer_let_group(binds, &local_env, subst)?;
                    subst = new_subst;
                    local_env = new_env;
                    typed_stmts.push(TypedStmt::Let(tbinds));
                }
            }
        }

        // Type-check the terminal expression
        let (te_terminal, terminal_ty, s_term) = self.infer_expr(current, &local_env)?;
        subst = subst.compose(&s_term);

        // Reconstruct the nested TExpr bottom-up, applying the final
        // substitution to all stored expressions so that type variables
        // resolved later in the chain are propagated back.
        let mut result_te = te_terminal.apply_subst(&subst);
        let mut result_ty = terminal_ty;

        for tstmt in typed_stmts.into_iter().rev() {
            match tstmt {
                TypedStmt::Bind(tb) => {
                    let param_ty = tb.param_ty.apply_subst(&subst);
                    let lambda_ty = Ty::arrow(param_ty.clone(), result_ty.clone());
                    let lambda = TExpr::new(
                        TExprKind::Lambda {
                            params: vec![(tb.param, param_ty)],
                            body: Box::new(result_te),
                        },
                        lambda_ty.clone(),
                    );
                    let infix_ty = result_ty.clone();
                    result_te = TExpr::new(
                        TExprKind::InfixApp {
                            op: tb.op,
                            lhs: Box::new(tb.lhs_te.apply_subst(&subst)),
                            rhs: Box::new(lambda),
                        },
                        infix_ty.clone(),
                    );
                    result_ty = infix_ty;
                }
                TypedStmt::Let(binds) => {
                    let let_ty = result_ty.clone();
                    let binds = binds.into_iter().map(|b| TLocalDef {
                        name: b.name,
                        patterns: b.patterns.into_iter().map(|p| p.apply_subst(&subst)).collect(),
                        body: b.body.apply_subst(&subst),
                    }).collect();
                    result_te = TExpr::new(
                        TExprKind::Let { binds, body: Box::new(result_te) },
                        let_ty,
                    );
                }
            }
        }

        Ok((result_te, result_ty, subst))
    }

    pub(super) fn check_expr_typed(&mut self, expr: &Expr, expected: &Ty, env: &TypeEnv) -> Result<(TExpr, Subst), DiagnosticKind> {
        let (te, inferred, subst) = self.infer_expr(expr, env)?;
        let s = unify(&inferred.apply_subst(&subst), &expected.apply_subst(&subst))?;
        let final_ty = inferred.apply_subst(&subst).apply_subst(&s);
        // Apply the full composed substitution to inner types so that type
        // variables resolved by the expected type (e.g. monad type from function
        // sig) propagate into sub-expressions through variable chains
        let full_subst = subst.compose(&s);
        let resolved_te = te.apply_subst(&full_subst);
        Ok((TExpr { kind: resolved_te.kind, ty: final_ty }, full_subst))
    }

    /// Generate a TIR function for an FFI declaration.
    /// The function body calls the named Lua function directly.
    pub(super) fn generate_ffi_function(&mut self, name: &str, lua_name: &str, ffi_kind: FfiKind, ty: &Ty) -> TFunction {
        // Count argument types from the function type
        let mut arg_types = Vec::new();
        let mut current = ty.clone();
        loop {
            match current {
                Ty::Arrow(a, b) => {
                    arg_types.push(*a);
                    current = *b;
                }
                _ => break,
            }
        }
        let ret_ty = current;

        // Zero-arg Pure FFI: constant access (e.g., math.pi), not a function call.
        // Zero-arg IO FFI still needs to call the function (e.g., io.flush()).
        if arg_types.is_empty() && matches!(ffi_kind, FfiKind::Pure) {
            let body = TExpr::new(
                TExprKind::SpecCall {
                    original: name.to_string(),
                    specialized: format!("__mll_const:{}", lua_name),
                    args: vec![],
                },
                ret_ty.clone(),
            );
            return TFunction {
                name: name.to_string(),
                ty: ty.clone(),
                clauses: vec![TClause {
                    span: None,
                    patterns: vec![],
                    guards: vec![],
                    body,
                    where_binds: vec![],
                }],
                specialized: false,
            dict_params: vec![],
            };
        }

        // Generate parameter names and patterns
        let params: Vec<(String, Ty)> = arg_types.iter().enumerate()
            .map(|(i, t)| (format!("_ffi{}", i), t.clone()))
            .collect();

        let patterns: Vec<TPattern> = params.iter()
            .map(|(n, t)| TPattern::Var(n.clone(), t.clone()))
            .collect();

        // Build the call expression: lua_func(_ffi0, _ffi1, ...). Function-typed
        // (callback) parameters are wrapped so the Lua host can call them with
        // positional arguments — see OutgoingCallback. Flags are computed here,
        // before monomorphization, while the threaded state is still a type var.
        //
        // A parameter *declared* `Maybe a` is an optional Lua parameter (see
        // SPEC "Optional parameters"): it is marked with FfiMaybeArg so codegen
        // unwraps `Just x` to `x`, passes `Nothing` as nil, and truly omits the
        // trailing run of nil optionals from the call. The mark is decided
        // here, from the declared signature, so a *polymorphic* FFI parameter
        // later instantiated at `Maybe` keeps its raw-value behavior. The
        // receiver of a method-call FFI (arg 0 of a `:method` name) is never
        // optional — there is no call without a receiver.
        let is_method_call = lua_name.starts_with(':');
        let call_args: Vec<TExpr> = params.iter().enumerate()
            .map(|(i, (n, t))| {
                let var = TExpr::new(TExprKind::Var(n.clone()), t.clone());
                if matches!(t, Ty::Arrow(_, _)) {
                    let (arity, marshal_args, run_io, marshal_ret) = outgoing_cb_flags(t);
                    TExpr::new(
                        TExprKind::OutgoingCallback {
                            callee: Box::new(var),
                            arity, marshal_args, run_io, marshal_ret,
                        },
                        t.clone(),
                    )
                } else if is_maybe_ty(t) && !(is_method_call && i == 0) {
                    TExpr::new(
                        TExprKind::FfiMaybeArg { value: Box::new(var) },
                        t.clone(),
                    )
                } else {
                    var
                }
            })
            .collect();

        // Check if return type is a tuple (multi-return from Lua)
        let tuple_arity = match &ret_ty {
            Ty::Tuple(elems) => Some(elems.len()),
            // IO (Tuple ...) — unwrap IO wrapper
            Ty::App(io, inner) if matches!(io.as_ref(), Ty::Con(c) if c == "IO") => {
                if let Ty::Tuple(elems) = inner.as_ref() { Some(elems.len()) } else { None }
            }
            _ => None,
        };

        let specialized = match ffi_kind {
            FfiKind::Iterator => format!("__mll_iter:{}", lua_name),
            FfiKind::Try => format!("__mll_try:{}", lua_name),
            FfiKind::Catch => format!("__mll_pcall:{}", lua_name),
            FfiKind::IOCatch => format!("__mll_iopcall:{}", lua_name),
            FfiKind::IO if tuple_arity.is_some() => {
                format!("__mll_io_tup:{}:{}", tuple_arity.unwrap(), lua_name)
            }
            FfiKind::IO => format!("__mll_io:{}", lua_name),
            _ if tuple_arity.is_some() => {
                format!("__mll_tup_ret:{}:{}", tuple_arity.unwrap(), lua_name)
            }
            _ => lua_name.to_string(),
        };

        let body = TExpr::new(
            TExprKind::SpecCall {
                original: name.to_string(),
                specialized,
                args: call_args,
            },
            ret_ty.clone(),
        );

        TFunction {
            name: name.to_string(),
            ty: ty.clone(),
            clauses: vec![TClause {
                span: None,
                patterns,
                guards: vec![],
                body,
                where_binds: vec![],
            }],
            specialized: false,
            dict_params: vec![],
        }
    }

    /// Check that function-typed parameters in an export signature return LuaIO.
    /// Lua functions are untrusted and assumed effectful, so callback parameters
    /// must have their return type in `LuaIO s a` form.
    pub(super) fn check_export_callbacks(&mut self, name: &str, ty: &Type) {
        // Walk the arrow chain to find parameters
        let mut current = ty;
        // Skip forall
        if let Type::Forall { inner, .. } = current {
            current = inner;
        }
        // Walk arrow parameters
        while let Type::Arrow(param, ret) = current {
            self.check_callback_param(name, param);
            current = ret;
        }
    }

    /// If a parameter is a function type, check its return type ends in LuaIO/ScopedLuaIO.
    pub(super) fn check_callback_param(&mut self, export_name: &str, param: &Type) {
        // Unwrap parens
        let p = match param {
            Type::Paren(inner) => inner.as_ref(),
            _ => param,
        };
        // Check if this parameter is a function type
        if let Type::Arrow(_, _) = p {
            // Find the ultimate return type of this callback
            let mut ret = p;
            while let Type::Arrow(_, r) = ret {
                ret = r;
            }
            // Unwrap parens on return type
            let ret = match ret {
                Type::Paren(inner) => inner.as_ref(),
                _ => ret,
            };
            // Must be ScopedLuaIO or an IO-like type
            let is_lua_io = match ret {
                Type::ScopedLuaIO { .. } => true,
                Type::App(outer, _) => {
                    if let Type::App(con, _) = outer.as_ref() {
                        matches!(con.as_ref(), Type::Con(c) if c == "LuaIO")
                    } else {
                        false
                    }
                }
                _ => false,
            };
            if !is_lua_io {
                self.errors.push(Diagnostic::in_context(
                    DiagnosticKind::Other(format!(
                        "Export '{}': callback parameter must return LuaIO s a. \
                         Lua functions are untrusted and assumed effectful.",
                        export_name
                    )),
                    format!("export declaration of '{}'", export_name),
                ));
            }
        }
    }

    /// Strict soundness check for FFI declarations that pass an mata-ll callback
    /// out to a Lua function and thread a polymorphic state through it (the
    /// fold pattern). The state must round-trip through Lua opaquely, which is
    /// only sound when it is one shared polymorphic type variable across the
    /// callback's accumulator argument, the callback's result, the FFI's
    /// initial-state argument, and the FFI's return. Concrete callbacks (no
    /// type variables, e.g. `String -> String`) thread no opaque state and are
    /// allowed without further checks.
    pub(super) fn validate_ffi_callbacks(&mut self, name: &str, ty: &Ty) {
        let ctx = || format!("FFI declaration of '{}'", name);
        let mut err = |msg: String| {
            self.errors.push(Diagnostic::in_context(DiagnosticKind::Other(msg), ctx()));
        };

        let (arg_tys, ret) = ty.peel_arrows();
        // The FFI's value-return type (peel an IO/LuaIO effect wrapper).
        let ffi_value_ret = match ret {
            Ty::IO(inner) | Ty::LuaIO(_, inner) => inner.as_ref(),
            other => other,
        };

        for cb in arg_tys.iter().filter(|t| matches!(t, Ty::Arrow(_, _))) {
            if callback_value_vars(cb).is_empty() {
                continue; // concrete callback: no opaque state to keep sound
            }
            // Polymorphic (stateful) callback: enforce the shared-variable rule.
            let s = match ffi_value_ret {
                Ty::Var(v) => v.clone(),
                _ => {
                    err(format!(
                        "FFI '{}' passes a polymorphic callback, so its return type \
                         must be the threaded state (a type variable); found a concrete type.",
                        name));
                    return;
                }
            };
            let (cb_args, cb_ret) = cb.peel_arrows();
            // Result must be `S` (pure) or `LuaIO s S` (effectful). `IO S` is
            // rejected: effectful callbacks are standardized on `LuaIO s acc`.
            let result_ok = match cb_ret {
                Ty::Var(v) => *v == s,
                Ty::LuaIO(_, inner) => matches!(inner.as_ref(), Ty::Var(v) if *v == s),
                _ => false,
            };
            if !result_ok {
                err(format!(
                    "FFI '{}': callback must return the threaded state '{}' (pure) \
                     or 'LuaIO s {}' (effectful).",
                    name, s.name, s.name));
            }
            // The callback must take the state as an accumulator argument.
            if !cb_args.iter().any(|a| matches!(a, Ty::Var(v) if *v == s)) {
                err(format!(
                    "FFI '{}': callback must take the threaded state '{}' as an argument.",
                    name, s.name));
            }
            // The FFI must take the initial state as a direct argument.
            if !arg_tys.iter().any(|a| matches!(a, Ty::Var(v) if *v == s)) {
                err(format!(
                    "FFI '{}': must take the initial state '{}' as an argument.",
                    name, s.name));
            }
        }
    }
}
