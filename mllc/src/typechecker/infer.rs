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
                    self.unify(&info.result_type, sty).is_ok()
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
        self.pattern_skolems.clear();
        // The caller-visible signature, with each universally-quantified
        // variable renamed to a fresh FLEXIBLE unification variable. Patterns
        // and every downstream pass work on this, exactly as before — crucially,
        // a GADT constructor pattern may still refine a signature variable to a
        // concrete type-index within a clause (`s := 'Empty`), which the flexible
        // reading makes an ordinary local unification.
        let (fresh_ty, renames) = self.freshen_sig_type_mapped(declared_ty);
        // For SOUNDNESS: while checking a clause BODY, any signature variable a
        // pattern did NOT already pin is skolemized (made rigid) so the body
        // cannot narrow it — that is what rejects a body more general than the
        // signature (`f :: a -> Int` / `f x = x`). `sig_skolems` maps each fresh
        // signature variable to its own rigid skolem, `demote` maps that skolem
        // back once the body checks, and each skolem is registered with the
        // classes the declared context provides so a `Monad m =>`-constrained
        // variable is still discharged by the context while a bare one has no
        // evidence. GADT refinement happens BEFORE this (at pattern time), so a
        // refined variable is concrete by the time the body is checked and is
        // never skolemized. See `skolemize_sig_body_vars` / `check_clause`.
        let declared_constraints = self.fn_constraints.get(name).cloned().unwrap_or_default();
        let (sig_skolems, demote) =
            self.skolemize_sig_body_vars(&fresh_ty, &declared_constraints);

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
            match self.check_clause(clause, &fresh_ty, &sig_skolems, &clause_ctx) {
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
                    // Locate the error at the offending statement captured while
                    // checking the body, falling back to the clause head for a
                    // shape error that crossed no statement marker (e.g. an
                    // argument-pattern mismatch).
                    let span = self.error_span.take().unwrap_or(clause.span);
                    self.push_error_span(e, clause_ctx, span);
                }
            }
        }

        // (The function type and clauses are finalised below, after numeric
        // defaulting has had a chance to extend `overall_subst`.)

        // The signature skolems are demoted back to their flexible variables at
        // each consumption point below (`Ty::demote_skolems`), once the body has
        // been checked rigidly against them: `final_ty` and the TIR clauses
        // (after `overall_subst` is applied), and each wanted constraint (after
        // resolving it through `overall_subst`, since a clause may have bound a
        // fresh unification variable TO a skolem). Everything downstream then
        // works on ordinary `Ty::Var`s, exactly as before the skolem check: a
        // wanted left on a bare, unconstrained signature variable flows into the
        // `MissingContextConstraint` path (asking for the missing `=>` context),
        // while a body that FORCED a signature variable to a concrete type
        // already failed at unification. Numeric defaulting reads `self.wanted`
        // with skolems still present, which is correct — a skolem has no free
        // variables and is never a defaulting candidate (a signature variable
        // must not be defaulted).

        // Record this function's declared constraints over the FINAL type's
        // variable names. Clause checking may unify a freshened signature
        // variable with a fresh unification variable, and it is THAT
        // variable's name the generalized function type carries — so neither
        // the source-name nor the freshened-sig-name spelling of the
        // constraints reliably matches the type. The dictionary-passing
        // rewrite needs the spelling that does.
        if let Some(cs) = self.fn_use_constraints.get(name).cloned() {
            let fresh_vars: HashMap<String, TyVar> = fresh_ty.free_vars()
                .into_iter().map(|v| (v.name.clone(), v)).collect();
            let renamed: Vec<TyConstraint> = cs.into_iter().map(|c| {
                let final_name = fresh_vars.get(&c.type_var)
                    // Resolve through the clause substitution, then demote any
                    // body skolem back to its flexible variable (a declared sig
                    // variable resolves to its skolem, which is not a `Ty::Var`).
                    .map(|tv| Ty::Var(tv.clone()).apply_subst(&overall_subst).demote_skolems(&demote))
                    .and_then(|t| if let Ty::Var(v) = t { Some(v.name) } else { None });
                TyConstraint {
                    class_name: c.class_name,
                    type_var: final_name.unwrap_or(c.type_var),
                }
            }).collect();
            self.fn_dict_constraints.insert(name.to_string(), renamed);
        }
        // ---- Numeric defaulting (mata-ll's `default (Int, Number)`; GHC's
        // Haskell-2010 rule is `default (Integer, Double)`, but mata-ll has no
        // Integer, so `Int` is the first candidate) ----
        // Resolve type variables that appear ONLY in the wanted constraints of
        // this binding (never in its signature type) and are constrained solely
        // by standard classes including at least one numeric class. Without this,
        // an in-expression literal like `show 5` would be reported ambiguous.
        // The chosen default (Int first, then Number for Double) is folded
        // into `overall_subst` BEFORE it is applied to the TIR clauses, so the
        // literal nodes carry the concrete type codegen ultimately emits.
        {
            let default_subst = self.compute_numeric_defaults(&fresh_ty, &overall_subst);
            if !default_subst.is_type_empty() {
                overall_subst = overall_subst.merge(&default_subst);
            }
        }
        let final_ty = fresh_ty.apply_subst(&overall_subst).demote_skolems(&demote);
        let tclauses: Vec<TClause> = tclauses.into_iter()
            .map(|c| {
                let mut c = c.apply_subst(&overall_subst);
                if !demote.is_empty() { c.demote_skolems(&demote); }
                c
            })
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
        // A set, not a Vec: with one binder per statement (a long do-block of
        // `let`s), the membership probes below are per-wanted-constraint ×
        // per-binder, and Vec::contains made that quadratic.
        let mut determined: HashSet<TyVar> = type_vars.iter().cloned().collect();
        for bt in &self.binder_types {
            // Demote any body skolem back to its flexible variable first: a
            // parameter bound at a signature-variable type resolves through
            // `overall_subst` to that variable's skolem, whose `free_vars` are
            // empty — without demotion it would drop from `determined`.
            for v in bt.apply_subst(&overall_subst).demote_skolems(&demote).free_vars() {
                determined.insert(v);
            }
        }
        for (class, cty) in std::mem::take(&mut self.wanted) {
            let rty = cty.apply_subst(&overall_subst).demote_skolems(&demote);
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
                                // The clause substitution may have bound this
                                // declared variable to its body skolem; demote it
                                // back so it matches the (already demoted) `v`.
                                .demote_skolems(&demote)
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
            let first_arg_ty = if let Ty::Arrow(a, _, _) = &final_ty { Some(a.as_ref()) } else { None };
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

        let tfun = TFunction {
            name: name.to_string(),
            ty: final_ty,
            clauses: tclauses,
            specialized: false,
            dict_params: vec![],
            derived_strict: false,
        };

        // Affine-usage check (linear types): with every multiplicity now
        // resolved on the final types, enforce that anything bound at a `%1`
        // arrow is used at most once (typechecker/usage.rs). A function that
        // never touches a `%1` type has nothing tracked, so this is a cheap
        // no-op walk for ordinary code.
        self.check_function_usage(&tfun);

        Some(tfun)
    }

    /// Haskell 2010 / GHC numeric defaulting for this binding.
    ///
    /// Considers each type variable that occurs in a leftover wanted constraint
    /// (after `subst`). A variable is a defaulting candidate when, GHC-style,
    /// every constraint on it has the bare variable as its head (`C v`, not
    /// `C (f v)`), every such class is a *standard* class, at least one is a
    /// numeric class, and the variable does NOT appear in this binding's own
    /// (signature) type — i.e. it is ambiguous, resolvable only by defaulting.
    /// For each candidate the default types are tried in order — `Int`,
    /// then `Number` (GHC's `Double`) — and the first that satisfies every
    /// constraint on the variable is chosen. Returns the substitution to fold
    /// into `overall_subst`.
    fn compute_numeric_defaults(&self, fresh_ty: &Ty, subst: &Subst) -> Subst {
        // GHC reduces each wanted constraint to head-normal form — per-variable
        // pieces `C v` — before defaulting: `Eq [(a, b)]` becomes `Eq a` and
        // `Eq b`, so `a` and `b` each default independently. `collect_required_
        // var_constraints` performs exactly that structural reduction (the same
        // one `has_instance` uses). A free variable a constraint cannot push all
        // the way down to (a non-structural head with no matching context) is
        // "blocked": it is entangled in a constraint we cannot simplify, so —
        // matching GHC — it is not defaulted.
        struct VarInfo {
            // Residual per-variable classes (GHC's HNF), for the standard/numeric
            // eligibility test.
            classes: Vec<String>,
            // Every original constraint in which the variable is free, used to
            // verify a candidate default actually satisfies the WHOLE constraint
            // (e.g. `Show (a -> a -> a)` — reducible to no residual, yet never
            // satisfiable — must veto defaulting `a`).
            full: Vec<(String, Ty)>,
        }
        let mut groups: HashMap<TyVar, VarInfo> = HashMap::new();
        for (class, cty) in &self.wanted {
            let rty = cty.apply_subst(subst);
            let fvs = rty.free_vars();
            if fvs.is_empty() { continue; }
            for v in &fvs {
                let e = groups.entry(v.clone()).or_insert(VarInfo { classes: Vec::new(), full: Vec::new() });
                e.full.push((class.clone(), rty.clone()));
            }
            // Reduce to the residual per-variable class constraints (GHC's HNF).
            // A structural constraint with no residual on a variable (e.g.
            // `Monoid [a]`, whose instance needs nothing of `a`) contributes no
            // class — it neither constrains nor blocks that variable's default.
            let mut reduced: Vec<(String, TyVar)> = Vec::new();
            self.collect_required_var_constraints(class, &rty, &mut reduced);
            for (rc, rv) in reduced {
                let e = groups.entry(rv).or_insert(VarInfo { classes: Vec::new(), full: Vec::new() });
                if !e.classes.contains(&rc) { e.classes.push(rc); }
            }
        }

        let sig_vars = fresh_ty.apply_subst(subst).free_vars();
        // Accumulate the chosen defaults as a plain map. Each candidate
        // variable is a distinct map key and every image is a ground default
        // type (`Int`/`Number`), so folding candidates in with `merge` (as
        // this used to) can never rewrite or conflict with an earlier
        // binding — but it re-walked the whole accumulated substitution per
        // candidate, which was quadratic in the number of defaulted
        // literals. Direct insertion builds the identical substitution.
        let mut out: HashMap<TyVar, Ty> = HashMap::new();
        for (v, info) in &groups {
            // A variable that survives in the binding's own type is genuinely
            // polymorphic — the caller (or its declared context) discharges it.
            if sig_vars.contains(v) { continue; }
            // All residual classes standard, at least one numeric (GHC's rule).
            // A user (non-standard) class on the variable blocks defaulting
            // exactly as in GHC — such a use is genuinely ambiguous.
            if !info.classes.iter().all(|c| Self::is_standard_class(c)) { continue; }
            if !info.classes.iter().any(|c| Self::is_numeric_class(c)) { continue; }
            // Try the default types in order; pick the first for which EVERY
            // original constraint on the variable has an instance.
            for cand in &["Int", "Number"] {
                let ct = Ty::Con((*cand).to_string());
                let sub = Subst::singleton(v.clone(), ct.clone());
                if info.full.iter().all(|(class, fty)| self.has_instance(class, &fty.apply_subst(&sub))) {
                    out.insert(v.clone(), ct);
                    break;
                }
            }
        }
        Subst::from_map(out)
    }

    fn is_numeric_class(c: &str) -> bool {
        matches!(c, "Num" | "Real" | "Integral" | "Fractional" | "RealFrac" | "Floating" | "RealFloat")
    }

    fn is_standard_class(c: &str) -> bool {
        Self::is_numeric_class(c)
            || matches!(c, "Eq" | "Ord" | "Show" | "Read" | "Enum" | "Bounded")
    }

    pub(super) fn check_clause(&mut self, clause: &Clause, fun_ty: &Ty, sig_skolems: &HashMap<TyVar, Ty>, ctx: &str) -> Result<(TClause, Subst), DiagnosticKind> {
        // Start each clause with no captured statement span: any error raised
        // while checking this clause records the offending statement's span via
        // the `Spanned` markers, which the caller uses to locate the diagnostic
        // instead of the clause head.
        self.error_span = None;
        let mut local_env = self.env.clone();
        let mut remaining_ty = fun_ty.clone();
        let mut subst = Subst::empty();
        let mut tpatterns = Vec::new();
        // Existential skolems minted anywhere in this clause (argument
        // patterns or nested matches in the body) must not survive into the
        // function's own type; snapshot to know which ones are ours.
        let skolems_before = self.pattern_skolems.len();

        for pattern in &clause.patterns {
            match &remaining_ty {
                Ty::Arrow(arg_ty, ret_ty, _) => {
                    let arg_ty = arg_ty.apply_subst(&subst);
                    let (tp, pat_subst) = self.check_pattern(pattern, &arg_ty, &mut local_env)?;
                    subst = subst.compose(&pat_subst);
                    remaining_ty = *ret_ty.clone();
                    tpatterns.push(tp);
                }
                _ => return Err(DiagnosticKind::Other("Too many arguments".into())),
            }
        }

        // Skolemize every signature variable the PATTERNS did not already pin,
        // for the duration of the body check. A GADT constructor pattern may
        // have refined a signature variable to a concrete type-index (`s :=
        // 'Empty`), resolved through `subst`; that variable is now concrete and
        // is left alone. Every still-free signature variable becomes its rigid
        // skolem so the body cannot narrow it — the soundness check. The map is
        // applied to the body's expected return type AND to the local
        // environment (parameters bound at signature-variable types), so the
        // body sees one consistent, rigid view; `check_function` demotes the
        // skolems back afterwards.
        let body_skolems: Subst = {
            let mut map = HashMap::new();
            for (v, sk) in sig_skolems {
                if Ty::Var(v.clone()).apply_subst(&subst) == Ty::Var(v.clone()) {
                    map.insert(v.clone(), sk.clone());
                }
            }
            Subst::from_map(map)
        };
        subst = subst.compose(&body_skolems);

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
                let cond_env = local_env.applied(&subst);
                let (tcond, cond_ty, s1) = self.infer_expr(&guard.condition, &cond_env)?;
                let s2 = self.unify(&cond_ty.apply_subst(&s1), &Ty::Con("Bool".into()))?;
                let combined = s1.compose(&s2);
                subst = subst.compose(&combined);
                let body_env = local_env.applied(&subst);
                let ret = expected_ret.apply_subst(&subst);
                let (tbody_g, body_s) = self.check_expr_typed(&guard.body, &ret, &body_env)?;
                subst = subst.compose(&body_s);
                tguards.push(TGuard { condition: tcond, body: tbody_g });
            }
            tbody = TExpr::new(TExprKind::Var("undefined".into()), expected_ret);
        } else {
            let body_env = local_env.applied(&subst);
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
                    match self.unify(&scheme.ty.apply_subst(&subst), &inferred_ty.apply_subst(&subst)) {
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
                // A where-function's patterns can unpack an existential too;
                // its skolems must not survive into the function's own type
                // (checked below, once that type is assembled).
                let where_skolems_before = self.pattern_skolems.len();
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
                    match self.unify(&scheme.ty.apply_subst(&subst), &inferred_fn_ty.apply_subst(&subst)) {
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
                // A skolem this where-function unpacked must not appear in
                // its own type: mata-ll where-bindings are monomorphic, so
                // every CALL of the function shares one type — a where-fn
                // returning its unpacked existential (`unpack (Foo x) = x`)
                // would claim two calls on two different boxes yield the
                // SAME hidden type, which is false (and, with an Eq-style
                // constrained existential, exploitable). GHC rejects this
                // the same way.
                self.check_existential_escape(
                    &inferred_fn_ty.apply_subst(&subst), where_skolems_before)?;
                twhere.push(TLocalDef {
                    name: ld.name.clone(),
                    patterns: tpatterns.into_iter().map(|p| p.apply_subst(&where_subst)).collect(),
                    body: texpr.apply_subst(&where_subst),
                });
            }
        }

        // An existential skolem unpacked by this clause (in an argument
        // pattern or a nested match) must not appear in the function's own
        // type: the function type is what callers see, and the hidden type
        // must stay hidden. This catches both the direct leak (`unFoo (Foo
        // x) = x` — the return type IS the skolem) and the indirect one
        // through a parameter (`f g s = case s of Foo x -> g x` — g's type
        // becomes `skolem -> r`).
        self.check_existential_escape(&fun_ty.apply_subst(&subst), skolems_before)?;

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
    ///
    /// The environment is taken BY VALUE and threaded in place: a long
    /// do-block calls this once per `let` statement, and rebuilding the whole
    /// environment (clone + substitute every scheme in scope) each time made
    /// typechecking quadratic in the number of statements. The caller's
    /// invariant — the incoming environment is already substituted up to the
    /// incoming `subst` (every path composes a substitution into `subst` and
    /// applies it to the live environment at the same time) — lets this
    /// function apply only the NEW substitutions it produces.
    pub(super) fn infer_let_group(
        &mut self,
        binds: &[LocalDef],
        mut env: TypeEnv,
        mut subst: AccSubst,
    ) -> Result<(Vec<TLocalDef>, TypeEnv, AccSubst), DiagnosticKind> {
        // Pre-register fresh monomorphic vars for the whole group so bindings
        // can see themselves and each other during inference. A shadowed outer
        // scheme is saved so it can come back for the generalization step —
        // generalization is over the OUTER environment, which still contains
        // it.
        let mut fresh_tys: Vec<Ty> = Vec::with_capacity(binds.len());
        let mut shadowed: Vec<Option<Scheme>> = Vec::with_capacity(binds.len());
        for bind in binds {
            let fv = self.fresh_var("_let");
            fresh_tys.push(fv.clone());
            // A let binder's type is determined by its body/uses; record it so a
            // class constraint over its variable is not flagged as ambiguous.
            self.binder_types.push(fv.clone());
            shadowed.push(env.remove(&bind.name));
            env.insert(bind.name.clone(), Scheme::mono(fv));
        }

        // Infer each body in the recursive environment and unify its type with
        // the pre-registered variable, keeping the environment substituted as
        // bindings resolve (`apply_subst_mut` touches only affected schemes).
        let mut tbinds = Vec::new();
        for (i, bind) in binds.iter().enumerate() {
            let (te, bind_ty, s) = self.infer_expr(&bind.body, &env)?;
            subst.compose_with(&s);
            env.apply_subst_mut(&s);
            let us = self.unify(&fresh_tys[i].apply_subst(&subst), &bind_ty.apply_subst(&subst))?;
            subst.compose_with(&us);
            env.apply_subst_mut(&us);
            tbinds.push(TLocalDef { name: bind.name.clone(), patterns: vec![], body: te });
        }

        // Take the group's monomorphic pre-registrations back out, restoring
        // any shadowed outer schemes: generalization must see exactly the
        // outer environment. A restored scheme sat outside the environment
        // while the group was inferred, so it catches up on the accumulated
        // substitution here (applying the already-seen prefix again is a
        // no-op: applications resolve variables fully, so they are
        // idempotent).
        for (bind, old) in binds.iter().zip(shadowed) {
            env.remove(&bind.name);
            if let Some(old_scheme) = old {
                env.insert(bind.name.clone(), old_scheme.apply_subst(&subst));
            }
        }

        // Generalize each binding over the outer environment (excluding the
        // group's own monomorphic vars), then extend the environment.
        //
        // Monomorphism restriction: a type variable still carrying an unresolved
        // class constraint is NOT generalized — it stays a shared monomorphic
        // variable so its constraints remain connected to how the binding is
        // used. This matters for numeric literals: `let t = Leaf 1` has type
        // `Tree a` with a pending `Num a`; generalizing `a` would sever that
        // `Num a` from the `Eq (Tree a)` a later `t == …` emits, and neither
        // fragment could then be defaulted. Keeping `a` monomorphic lets the two
        // constraints meet at the enclosing binding's defaulting step. This also
        // brings `let` into line with mata-ll's already-monomorphic `where`
        // bindings, and matches GHC's monomorphism restriction.
        //
        // Whether a candidate variable is constraint-bound is checked lazily
        // per candidate, scanning the wanted constraints newest-first — a
        // do-`let`'s pending constraint (its literal's `Num`) is the one just
        // emitted, so the common case stops after a step or two instead of
        // materializing every wanted constraint's variables each time.
        let is_constrained = |checker: &Self, v: &TyVar| {
            checker.wanted.iter().rev()
                .any(|(_, cty)| cty.apply_subst(&subst).free_vars().contains(v))
        };
        // Generalize ALL binds against the outer environment BEFORE inserting
        // any of the new schemes: a sibling's scheme is not part of the outer
        // environment and must not influence what a binding may quantify.
        let mut schemes: Vec<Scheme> = Vec::with_capacity(binds.len());
        for fresh in &fresh_tys {
            let bind_ty = fresh.apply_subst(&subst);
            let mut scheme = self.generalize(&env, &bind_ty);
            scheme.vars.retain(|v| !is_constrained(self, v));
            schemes.push(scheme);
        }
        for (bind, scheme) in binds.iter().zip(schemes) {
            env.insert(bind.name.clone(), scheme);
        }

        Ok((tbinds, env, subst))
    }

    /// Reject a type that mentions an existential skolem minted after
    /// `since` (a snapshot of `pattern_skolems.len()`). Called on any type
    /// that outlives the pattern match which unpacked the existential — the
    /// result type of a case expression, or the whole function type of a
    /// clause (which also catches leaks into parameter types, e.g. an outer
    /// lambda-bound function unified against `skolem -> r`). The concrete
    /// type was erased at packing time, so nothing outside the match may
    /// name it.
    fn check_existential_escape(&self, ty: &Ty, since: usize) -> Result<(), DiagnosticKind> {
        for (sk_name, sk_id) in &self.pattern_skolems[since..] {
            if ty.contains_skolem(sk_name, *sk_id) {
                let con = self.existential_skolems.get(sk_id)
                    .map(|i| i.con.clone())
                    .unwrap_or_default();
                return Err(DiagnosticKind::ExistentialEscape {
                    var: sk_name.clone(),
                    con,
                    ty: ty.clone(),
                });
            }
        }
        Ok(())
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
                let s = self.unify(expected, &lit_ty)?;
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
                // Existential type variables become fresh RIGID skolem
                // constants, scoped to this pattern-match branch: the
                // concrete type was erased when the value was packed, so the
                // body may not assume anything about it — a skolem unifies
                // only with itself, never with a concrete type. Provenance is
                // recorded so (a) `has_instance` satisfies a wanted class
                // constraint on the skolem from exactly the constructor's
                // declared context and nothing else, (b) the escape checks in
                // `check_clause` / `Expr::Case` know which skolems this
                // pattern introduced, and (c) diagnostics can say where the
                // hidden type came from.
                for tv in &con_info.existential_vars {
                    let sk_id = self.next_var;
                    self.next_var += 1;
                    tv_map.insert(tv.clone(), Ty::Skolem(tv.name.clone(), sk_id));
                    self.pattern_skolems.push((tv.name.clone(), sk_id));
                    let givens = con_info.existential_constraints.iter()
                        .filter(|c| c.type_var == tv.name)
                        .map(|c| c.class_name.clone())
                        .collect();
                    self.existential_skolems.insert(sk_id, ExSkolemInfo {
                        var: tv.name.clone(),
                        con: name.clone(),
                        givens,
                        origin: SkolemOrigin::Existential,
                    });
                }
                let tv_subst = Subst::from_map(tv_map);
                let result_ty = con_info.result_type.apply_subst(&tv_subst);
                let mut subst = self.unify(expected, &result_ty)?;

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
                let s = self.unify(expected, &tuple_ty)?;
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

    /// Depth-guard wrapper around `infer_expr_inner`: expression inference
    /// recurses along the desugared expression structure, whose depth is not
    /// bounded by the parser (a 10,000-element list literal is flat source
    /// but a 10,000-deep cons chain here). Past the limit the walk stops with
    /// a clean diagnostic BEFORE recursing deeper, so it can never overflow
    /// the native stack. See `crate::MAX_NESTING_DEPTH` /
    /// `crate::COMPILER_STACK_SIZE` for how limit and stack are calibrated.
    pub(super) fn infer_expr(&mut self, expr: &Expr, env: &TypeEnv) -> Result<(TExpr, Ty, Subst), DiagnosticKind> {
        if self.expr_depth >= crate::MAX_NESTING_DEPTH {
            return Err(Self::expr_too_deep());
        }
        self.expr_depth += 1;
        let r = self.infer_expr_inner(expr, env);
        self.expr_depth -= 1;
        r
    }

    /// The "expression nested too deeply" diagnostic shared by `infer_expr`
    /// and `check_expr_typed`.
    fn expr_too_deep() -> DiagnosticKind {
        DiagnosticKind::Other(format!(
            "expression nested too deeply (limit {}): the compiler walks \
             expressions with bounded recursion so it can report this error \
             instead of crashing; note that a long chain also counts — a list \
             literal nests one level per element, an operator chain one level \
             per operand — so split the expression into smaller definitions",
            crate::MAX_NESTING_DEPTH
        ))
    }

    /// Latch a statement-boundary expression's span as the error location, for
    /// a failure that surfaces at a PARENT unification (reconciling a branch's
    /// type against the case/if result) rather than inside the branch's own
    /// inference — where the `Spanned` arm's latch would already have fired.
    /// No-op unless `e` is a `Spanned` marker and no inner statement has claimed
    /// the location first.
    fn latch_stmt_span(&mut self, e: &Expr) {
        if self.error_span.is_none()
            && let Expr::Spanned(sp, _) = e {
            self.error_span = Some(*sp);
        }
    }

    fn infer_expr_inner(&mut self, expr: &Expr, env: &TypeEnv) -> Result<(TExpr, Ty, Subst), DiagnosticKind> {
        match expr {
            // A transparent statement-boundary marker: infer the inner
            // expression and, if it fails, latch this statement's span as the
            // error location — but only if a deeper (inner) statement has not
            // already claimed it, so the innermost failing statement wins. The
            // marker is erased: the inner expression's typed IR is returned.
            Expr::Spanned(span, inner) => {
                let r = self.infer_expr(inner, env);
                if r.is_err() && self.error_span.is_none() {
                    self.error_span = Some(*span);
                }
                r
            }
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
                } else if let Some(con) = self.existential_fields.get(name) {
                    // The field exists, but its type is existential — a
                    // selector would hand the hidden type to any caller,
                    // outside every match scope.
                    Err(DiagnosticKind::Other(format!(
                        "Field '{}' of constructor '{}' has an existential type, so it has no selector function: the field's type was erased when the value was packed, and a selector would carry it out of the pattern match that is the only place it may be used. Pattern-match the constructor positionally instead ('case v of {} x -> …')",
                        name, con, con)))
                } else {
                    Err(DiagnosticKind::UnboundVariable(name.clone()))
                }
            }
            Expr::Con(name) => {
                // Resolve to the registered key (shadowing, see check_pattern);
                // the TIR carries the key so codegen picks the right tag.
                let con_key = self.resolve_con_name(name).to_string();
                if let Some(scheme) = env.lookup(&con_key) {
                    let scheme = scheme.clone();
                    let (ty, inst_map) = self.instantiate_with_map(&scheme);
                    // Packing a value into a constrained existential
                    // constructor (`forall a. Show a => Showable a`) must
                    // prove the declared context for the packed type HERE:
                    // construction is the only moment the concrete type is
                    // still known — after it, only the constraint's evidence
                    // survives. Emit each declared constraint as a wanted on
                    // the instantiation of its existential variable.
                    if let Some(ci) = self.constructors.get(&con_key) {
                        for c in &ci.existential_constraints {
                            if let Some(t) = inst_map.iter()
                                .find(|(v, _)| v.name == c.type_var)
                                .map(|(_, t)| t.clone()) {
                                self.wanted.push((c.class_name.clone(), t));
                            }
                        }
                    }
                    Ok((TExpr::new(TExprKind::Con(con_key), ty.clone()), ty, Subst::empty()))
                } else {
                    Err(DiagnosticKind::UnboundConstructor(name.clone()))
                }
            }
            Expr::Lit(lit) => {
                // Numeric literals are polymorphic (GHC): an integer literal is
                // `Num a => a` (via `fromInteger`), a fractional literal is
                // `Fractional a => a` (via `fromRational`). We give the literal a
                // fresh type variable and emit the corresponding wanted; the
                // variable is later unified with the surrounding concrete type,
                // or resolved by defaulting (Int, then Number) if it stays
                // free. The TIR keeps the raw literal — at a concrete Int or
                // Number type `fromInteger`/`fromRational` is the identity and is
                // erased in codegen; only a user Num type materialises the call
                // (see the monomorphizer's Lit handling).
                match lit {
                    Literal::Integer(_) => {
                        let ty = self.fresh_var("_lit");
                        if let Ty::Var(v) = &ty {
                            self.wanted.push(("Num".to_string(), Ty::Var(v.clone())));
                        }
                        Ok((TExpr::new(TExprKind::Lit(Self::convert_literal(lit)), ty.clone()), ty, Subst::empty()))
                    }
                    Literal::Number(_) => {
                        let ty = self.fresh_var("_lit");
                        if let Ty::Var(v) = &ty {
                            self.wanted.push(("Fractional".to_string(), Ty::Var(v.clone())));
                        }
                        Ok((TExpr::new(TExprKind::Lit(Self::convert_literal(lit)), ty.clone()), ty, Subst::empty()))
                    }
                    _ => {
                        let ty = self.literal_type(lit);
                        Ok((TExpr::new(TExprKind::Lit(Self::convert_literal(lit)), ty.clone()), ty, Subst::empty()))
                    }
                }
            }
            Expr::App(func, arg) => {
                let (tf, func_ty, s1) = self.infer_expr(func, env)?;
                let env2 = env.applied(&s1);
                let (ta, arg_ty, s2) = self.infer_expr(arg, &env2)?;
                let ret_ty = self.fresh_var("_r");
                let func_ty = func_ty.apply_subst(&s2);
                // The expected arrow at an application carries a FLEXIBLE
                // multiplicity: it adopts the applied function's own (so a
                // `%1` function can be applied like any other), and the
                // linear-usage pass later reads the resolved multiplicity to
                // decide how the application charges the argument (usage.rs).
                let app_mult = self.fresh_mult();

                // Rank-2: if the function expects a forall-quantified argument,
                // skolemize the quantified variable and check the argument against it
                let s3 = if let Ty::Arrow(ref param_ty, ref func_ret, _) = func_ty {
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
                        let s_arg = self.unify(&arg_ty, &skolem_body)?;
                        // Connect return type
                        let s_ret = self.unify(&ret_ty, &func_ret.apply_subst(&s_arg))?;
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
                        self.unify(&func_ty, &Ty::arrow_m(arg_ty, ret_ty.clone(), app_mult))?
                    }
                } else {
                    self.unify(&func_ty, &Ty::arrow_m(arg_ty, ret_ty.clone(), app_mult))?
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
                    // Each lambda arrow gets a fresh multiplicity variable so
                    // the lambda can be used at a `%1` type: checking it
                    // against `a %1 -> b` binds the variable to One, and the
                    // linear-usage pass then reads the resolved arrow to know
                    // the binder must be consumed exactly once (usage.rs).
                    let mult = self.fresh_mult();
                    if param != "_" {
                        local_env.insert(param.clone(), Scheme::mono(param_ty.clone()));
                    }
                    param_info.push((param.clone(), param_ty, mult));
                }
                let (tbody, body_ty, subst) = self.infer_expr(body, &local_env)?;
                let func_ty = param_info.iter().rev().fold(body_ty, |acc, (_, pt, m)| {
                    Ty::arrow_m(pt.apply_subst(&subst), acc, *m)
                });
                let typed_params: Vec<(String, Ty)> = param_info.iter()
                    .map(|(n, t, _)| (n.clone(), t.apply_subst(&subst)))
                    .collect();
                Ok((
                    TExpr::new(TExprKind::Lambda { params: typed_params, body: Box::new(tbody) }, func_ty.clone()),
                    func_ty, subst,
                ))
            }
            Expr::If { cond, then_branch, else_branch } => {
                let (tc, cond_ty, s1) = self.infer_expr(cond, env)?;
                let sb = self.unify(&cond_ty, &Ty::Con("Bool".into()))?;
                let s1 = s1.compose(&sb);
                let env2 = env.applied(&s1);
                let (tt, then_ty, s2) = self.infer_expr(then_branch, &env2)?;
                let env3 = env2.applied(&s2);
                let (te, else_ty, s3) = self.infer_expr(else_branch, &env3)?;
                // then/else agree cleanly on their own; a mismatch reconciling
                // them belongs at the else branch, not the clause head.
                let s4 = self.unify(&then_ty.apply_subst(&s3), &else_ty)
                    .inspect_err(|_| self.latch_stmt_span(else_branch))?;
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
                    // Any existential skolem this branch's pattern mints must
                    // stay inside the branch; snapshot to check that below.
                    let skolems_before = self.pattern_skolems.len();
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
                            let genv = branch_env.applied(&subst);
                            let (tcond, cond_ty, gs1) = self.infer_expr(&guard.condition, &genv)?;
                            let gs2 = self.unify(&cond_ty.apply_subst(&gs1), &Ty::Con("Bool".into()))?;
                            subst = subst.compose(&gs1).compose(&gs2);
                            let genv2 = branch_env.applied(&subst);
                            let (tgbody, gbody_ty, gbs) = self.infer_expr(&guard.body, &genv2)?;
                            subst = subst.compose(&gbs);
                            let gu = self.unify(&result_ty.apply_subst(&subst), &gbody_ty)
                                .inspect_err(|_| self.latch_stmt_span(&guard.body))?;
                            subst = subst.compose(&gu);
                            tguards.push(TGuard { condition: tcond, body: tgbody });
                        }
                    }

                    let (tb, body_ty, body_subst) = self.infer_expr(&branch.body, &branch_env)?;
                    subst = subst.compose(&body_subst);
                    // The branch body inferred cleanly; a mismatch here is the
                    // branch's type disagreeing with the case result, so locate
                    // it at this branch, not the clause head.
                    let s = self.unify(&result_ty.apply_subst(&subst), &body_ty)
                        .inspect_err(|_| self.latch_stmt_span(&branch.body))?;
                    subst = subst.compose(&s);
                    // The case's result outlives the branch, so an unpacked
                    // existential's skolem must not appear in it (e.g.
                    // `case s of Foo x -> x`). Leaks into longer-lived types
                    // that are not the result (an outer binder's type) are
                    // caught by the same check at the clause boundary.
                    self.check_existential_escape(&result_ty.apply_subst(&subst), skolems_before)?;
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
            Expr::Let { .. } => {
                // Process the whole spine of directly nested `let`s
                // ITERATIVELY, threading one owned environment and one
                // accumulated substitution along it. A do-block whose
                // statements are all `let`s desugars to one nested `Expr::Let`
                // per statement; recursing per level cloned the environment
                // and re-composed the inner substitution at every step, which
                // made such blocks quadratic to typecheck (and burned nesting
                // depth linearly). This mirrors `infer_bind_chain`'s iterative
                // handling of `>>=` chains, including how it threads the
                // accumulated substitution through `infer_let_group`.
                let mut env_acc = env.clone();
                let mut subst = AccSubst::new();
                let mut tgroups: Vec<Vec<TLocalDef>> = Vec::new();
                let mut cur: &Expr = expr;
                while let Expr::Let { binds, body } = cur {
                    let (tbinds, new_env, new_subst) =
                        self.infer_let_group(binds, env_acc, subst)?;
                    env_acc = new_env;
                    subst = new_subst;
                    tgroups.push(tbinds);
                    cur = body;
                }
                let (tbody, body_ty, s) = self.infer_expr(cur, &env_acc)?;
                subst.compose_with(&s);
                // Rebuild the nested TIR lets bottom-up; every level of the
                // spine has the innermost body's type.
                let mut te = tbody;
                for tbinds in tgroups.into_iter().rev() {
                    te = TExpr::new(
                        TExprKind::Let { binds: tbinds, body: Box::new(te) },
                        body_ty.clone(),
                    );
                }
                Ok((te, body_ty, subst.into_subst()))
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
                // The ascribed type is user-written type syntax like any
                // signature: it must be a well-kinded complete type
                // (`x :: Maybe` or `xs :: [] Int Int` is a kind
                // error, not a unification puzzle).
                self.check_type_kind(declared_ty, "a type ascription");
                let expected = self.ast_type_to_ty(declared_ty);
                let expected = self.freshen_sig_type(&expected);
                let (te, inferred, subst) = self.infer_expr(inner, env)?;
                let s = self.unify(&inferred, &expected)?;
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
                    if let Some(con) = self.existential_fields.get(field_name) {
                        // The new value's type would have to match a type
                        // that was erased when the record was packed —
                        // unknowable. Rebuild the record with its
                        // constructor instead.
                        return Err(DiagnosticKind::Other(format!(
                            "Field '{}' of constructor '{}' has an existential type and cannot be record-updated: the type the new value must have was erased when the record was packed, so there is nothing to check the new value against. Rebuild the value with '{}' instead",
                            field_name, con, con)));
                    }
                    let env2 = env.applied(&subst);
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
                    let env2 = env.applied(&subst);
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
                    // Non-lambda RHS: a first-class use of the operator —
                    // `m >>= f` applies a continuation FUNCTION, `a >> b`
                    // sequences two action expressions. Either way the whole
                    // InfixApp is the chain's TERMINAL, typed below by the
                    // ordinary infix rule (the same one that types it at top
                    // level). It must NOT be flattened into one more
                    // statement: doing so made the rhs the terminal, so a
                    // final `step 1 >>= print` after another do-statement
                    // unified `print`'s function type with the do-block's
                    // monad ("Cannot unify 'IO a' with 'b -> IO ()'"), and a
                    // final `a >> b` forced `>>`'s second argument against a
                    // synthetic continuation arrow ("'IO a' with 'b -> c'").
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

        // Process each statement iteratively. The substitution accumulates
        // once per statement, so it uses the indexed accumulator (AccSubst):
        // composing through the plain representation re-walks the whole
        // accumulated map each step, which is quadratic over a long chain.
        let mut local_env = env.clone();
        let mut subst = AccSubst::new();
        // Collect typed results to reconstruct bottom-up
        struct TypedBind {
            op: String,
            lhs_te: TExpr,
            param: String,
            param_ty: Ty,
            /// The `m b` result type of this bind (the type of the whole
            /// `lhs >>= \param -> rest`). Unified with the continuation's
            /// type in the backward pass below.
            result_ty: Ty,
        }
        enum TypedStmt {
            Bind(Box<TypedBind>),
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
                    subst.compose_with(&s_op);
                    local_env.apply_subst_mut(&s_op);

                    // Infer lhs type
                    let (tlhs, lhs_ty, s_lhs) = self.infer_expr(lhs, &local_env)?;
                    subst.compose_with(&s_lhs);
                    local_env.apply_subst_mut(&s_lhs);

                    // Unify: op_ty ~ lhs_ty -> (param_ty -> result_ty) -> result_ty
                    let param_ty = self.fresh_var("_bp");
                    let result_ty = self.fresh_var("_br");
                    let op_ty = op_ty.apply_subst(&s_lhs);
                    let expected_op = Ty::arrow(lhs_ty, Ty::arrow(
                        Ty::arrow(param_ty.clone(), result_ty.clone()),
                        result_ty.clone(),
                    ));
                    let s_unify = self.unify(&op_ty, &expected_op)?;
                    subst.compose_with(&s_unify);
                    local_env.apply_subst_mut(&s_unify);
                    let bound_ty = param_ty.apply_subst(&s_unify);

                    // Bind parameter
                    if *param != "_" {
                        local_env.insert(param.to_string(), Scheme::mono(bound_ty.clone()));
                    }

                    typed_stmts.push(TypedStmt::Bind(Box::new(TypedBind {
                        op: op.to_string(),
                        lhs_te: tlhs,
                        param: param.to_string(),
                        param_ty: bound_ty,
                        result_ty,
                    })));
                }
                BindStmt::Let { binds } => {
                    let (tbinds, new_env, new_subst) =
                        self.infer_let_group(binds, local_env, subst)?;
                    subst = new_subst;
                    local_env = new_env;
                    typed_stmts.push(TypedStmt::Let(tbinds));
                }
            }
        }

        // Type-check the terminal expression
        let (te_terminal, terminal_ty, s_term) = self.infer_expr(current, &local_env)?;
        subst.compose_with(&s_term);

        // Backward pass: unify each bind's result type with the type of its
        // continuation (the rest of the chain). For `lhs >>= \p -> rest` the
        // bind's `m b` result IS the type of `rest`, so the do-block's monad
        // flows both ways along the chain. Without this, a bind whose lhs
        // doesn't pin the monad by itself (e.g. `x <- fmap f (pure 1)`) stays
        // polymorphic when later statements are the only thing that determine
        // the monad, and monomorphization can't resolve the class methods.
        // The non-chain (short do-block) path gets this unification for free
        // from the general App rule; this restores it for the iterative path.
        // A `let` statement is transparent here: `let ... in rest` has the
        // type of `rest`.
        let mut cont_ty = terminal_ty.clone();
        for tstmt in typed_stmts.iter().rev() {
            if let TypedStmt::Bind(tb) = tstmt {
                let rt = tb.result_ty.apply_subst(&subst);
                let s = self.unify(&rt, &cont_ty.apply_subst(&subst))?;
                subst.compose_with(&s);
                cont_ty = rt.apply_subst(&s);
            }
        }

        // Reconstruct the nested TExpr bottom-up, applying the final
        // substitution to all stored expressions so that type variables
        // resolved later in the chain are propagated back.
        let mut result_te = te_terminal.apply_subst(&subst);
        let mut result_ty = terminal_ty.apply_subst(&subst);

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

        Ok((result_te, result_ty, subst.into_subst()))
    }

    // Depth-guard wrapper; see `infer_expr`. Bidirectional checking recurses
    // into itself (lambda bodies, branches) without passing through
    // `infer_expr`, so it maintains the same counter.
    pub(super) fn check_expr_typed(&mut self, expr: &Expr, expected: &Ty, env: &TypeEnv) -> Result<(TExpr, Subst), DiagnosticKind> {
        if self.expr_depth >= crate::MAX_NESTING_DEPTH {
            return Err(Self::expr_too_deep());
        }
        self.expr_depth += 1;
        let r = self.check_expr_typed_inner(expr, expected, env);
        self.expr_depth -= 1;
        r
    }

    pub(super) fn check_expr_typed_inner(&mut self, expr: &Expr, expected: &Ty, env: &TypeEnv) -> Result<(TExpr, Subst), DiagnosticKind> {
        let (te, inferred, subst) = self.infer_expr(expr, env)?;
        let s = self.unify(&inferred.apply_subst(&subst), &expected.apply_subst(&subst))?;
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
        while let Ty::Arrow(a, b, _) = current {
            arg_types.push(*a);
            current = *b;
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
            derived_strict: false,
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
        // positional arguments — see OutgoingCallback. Only the arity and
        // IO-ness are fixed here; the boundary conversions are derived at
        // codegen time from the monomorphized callback type.
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
                if matches!(t, Ty::Arrow(..)) {
                    let (arity, run_io) = outgoing_cb_flags(t);
                    TExpr::new(
                        TExprKind::OutgoingCallback {
                            callee: Box::new(var),
                            arity, run_io,
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
            derived_strict: false,
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
        while let Type::Arrow(param, ret, _) = current {
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
        if let Type::Arrow(..) = p {
            // Find the ultimate return type of this callback
            let mut ret = p;
            while let Type::Arrow(_, r, _) = ret {
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

        for cb in arg_tys.iter().filter(|t| matches!(t, Ty::Arrow(..))) {
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
