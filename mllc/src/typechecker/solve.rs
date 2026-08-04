//! Constraint solving: typeclass instance registration and lookup,
//! superclass entailment, wanted-constraint emission, and class/instance
//! declaration checking. Moved verbatim out of the monolithic
//! typechecker.rs; `use super::*` keeps every name resolution identical.

use super::*;

impl Checker {
    /// Register an instance under its structured head key, derived from the
    /// instance's target type — never from a Display string. Types with no
    /// instance head (functions, bare type variables) cannot carry instances;
    /// `check_instance` rejects those before reaching here, and the built-in /
    /// derived registrations always have a head by construction.
    pub(super) fn register_instance(&mut self, info: InstanceInfo) {
        if let Some(head) = InstHead::of(&info.target_type) {
            self.instances.insert((info.class_name.clone(), head), info);
        }
    }

    /// Does `class` have an instance for `ty`? Conservative: a type variable or
    /// rigid skolem is treated as satisfiable (deferred to the caller), and a
    /// container (list/tuple/applied type) is satisfiable when its components
    /// are. Only the cases that genuinely never have an instance — functions,
    /// IO/ST actions, and a concrete type constructor with no registered
    /// instance — are rejected.
    pub(super) fn has_instance(&self, class: &str, ty: &Ty) -> bool {
        match ty {
            // Polymorphic — not this definition's job to discharge.
            Ty::Var(_) => true,
            // A rank-2 sealing skolem defers to the enclosing context (the
            // caller's constraints discharge it). An EXISTENTIAL skolem
            // cannot defer: the concrete type was erased when the value was
            // packed into its constructor, so the only evidence that can
            // ever exist is what the constructor's declared context
            // (`forall a. Show a => …`) guarantees — exactly those classes
            // (and their superclasses), nothing more.
            Ty::Skolem(_, id) => match self.existential_skolems.get(id) {
                Some(info) => info.givens.iter().any(|g| self.class_satisfies(g, class)),
                None => true,
            },
            // No instance for functions or effectful actions, ever.
            Ty::Arrow(..) | Ty::Forall(_, _) | Ty::IO(_) | Ty::LuaIO(_, _) => false,
            Ty::Promoted(_) => false,
            Ty::Unit => true,
            // Lists/tuples are structural for Show and Eq (mono generates the
            // instance), but not for Ord — mata-ll has no list/tuple ordering.
            // A non-structural class can still have a registered list instance
            // (e.g. Monoid [a]); its declared context governs what the element
            // must provide, exactly like the Con-headed path below.
            Ty::List(elem) => {
                if structural_container_class(class) {
                    return self.has_instance(class, elem);
                }
                let Some(inst) = self.instances.get(&(class.to_string(), InstHead::List)) else {
                    return false;
                };
                match &inst.context {
                    Some(ctx) => match Self::match_instance_args(&inst.target_type, ty) {
                        Some(binds) => ctx.iter().all(|c| {
                            binds.get(&c.type_var)
                                .map(|t| self.has_instance(&c.class_name, t))
                                .unwrap_or(true)
                        }),
                        None => true,
                    },
                    None => self.has_instance(class, elem),
                }
            }
            Ty::Tuple(elems) =>
                structural_container_class(class) && elems.iter().all(|e| self.has_instance(class, e)),
            Ty::Con(_) => InstHead::of(ty)
                .is_some_and(|h| self.instances.contains_key(&(class.to_string(), h))),
            Ty::App(_, _) => {
                // Peel `T a b …` to its head constructor and argument types.
                let mut head = ty;
                let mut args: Vec<&Ty> = Vec::new();
                while let Ty::App(f, a) = head {
                    args.push(a.as_ref());
                    head = f.as_ref();
                }
                // A type-family application (`Rep a`, `Element [a]`) has no
                // instance of its OWN — the instance belongs to whatever the
                // family reduces to. If it reduces here (its arguments are
                // concrete enough), resolve on the reduct; if it is STUCK on a
                // rigid/flexible variable, defer exactly like a bare type
                // variable — the constraint (`GToJSON (Rep a)`) is carried by
                // the enclosing signature's context and discharged once
                // monomorphization pins `a` and the family reduces. Without
                // this, `has_instance` would peel to the family's head `Con`,
                // find no instance registered under it, and wrongly reject the
                // whole polymorphic definition (see the Generics substrate).
                if let Ty::Con(name) = head {
                    if self.is_type_family(name) {
                        return match self.reduce_family_ty(ty) {
                            Some(reduced) => self.has_instance(class, &reduced),
                            None => true,
                        };
                    }
                }
                match head {
                    // Maybe is structural for Show/Eq like lists are: its
                    // built-in Eq is NOT a registered instance, so a structural
                    // class checks the element directly. But this shortcut only
                    // applies when nothing is registered for `(class, Maybe)` —
                    // a user `instance C (Maybe a)` (or the builtin Show Maybe)
                    // must be honoured via the registry, like any other Con
                    // head, so fall through to the `Con(_)` arm below when one
                    // exists. (Before source-class methods emitted wanteds this
                    // path was never taken for a user class, so the omission
                    // stayed latent.)
                    Ty::Con(base) if base == "Maybe"
                        && !self.instances.contains_key(&(class.to_string(), InstHead::Con("Maybe".into()))) =>
                        structural_container_class(class) && args.iter().all(|a| self.has_instance(class, a)),
                    // Other type constructors need a registered instance. What
                    // the instance then demands of the type ARGUMENTS depends
                    // on its declared context: a user-written instance carries
                    // its exact context (`instance Show a => Show (Tree a)` at
                    // `Tree X` demands precisely `Show X`, and a context-free
                    // one demands nothing), while builtin/derived instances
                    // (context: None) keep the structural rule — every
                    // argument needs the class itself.
                    Ty::Con(_) => {
                        // A Con head always has an InstHead; mirror the old
                        // `is_some_and` (reject) if that ever changes.
                        let Some(h) = InstHead::of(head) else { return false };
                        let Some(inst) = self.instances.get(&(class.to_string(), h)) else {
                            return false;
                        };
                        match &inst.context {
                            Some(ctx) => match Self::match_instance_args(&inst.target_type, ty) {
                                Some(binds) => ctx.iter().all(|c| {
                                    binds.get(&c.type_var)
                                        .map(|t| self.has_instance(&c.class_name, t))
                                        // A context variable the use type does
                                        // not determine — defer, don't reject.
                                        .unwrap_or(true)
                                }),
                                // Argument spines don't line up — defer to the
                                // monomorphizer rather than over-reject.
                                None => true,
                            },
                            None => args.iter().all(|a| self.has_instance(class, a)),
                        }
                    }
                    _ => true, // unknown head — defer rather than over-reject
                }
            }
        }
    }

    /// When `class ty` has no instance BECAUSE a registered instance's
    /// declared context is unsatisfied, explain which context constraint
    /// failed — recursing when the failure is itself context-shaped, so
    /// `Show (Tree (Tree Blob))` bottoms out at `Blob`. Returns None when the
    /// failure is not context-shaped (no registered head instance, a function
    /// type, …); the plain "No instance" message already covers those.
    pub(super) fn context_failure_note(&self, class: &str, ty: &Ty) -> Option<String> {
        if !matches!(ty, Ty::App(_, _)) {
            return None;
        }
        let inst = InstHead::of(ty)
            .and_then(|h| self.instances.get(&(class.to_string(), h)))?;
        let ctx = inst.context.as_ref()?;
        let binds = Self::match_instance_args(&inst.target_type, ty)?;
        // A compound type in constraint position reads wrong without parens
        // ("Show Tree a" vs "Show (Tree a)").
        let paren = |t: &Ty| match t {
            Ty::Arrow(..) | Ty::App(_, _) | Ty::IO(_) | Ty::LuaIO(_, _) =>
                format!("({})", t),
            _ => format!("{}", t),
        };
        for c in ctx {
            let Some(bound) = binds.get(&c.type_var) else { continue };
            if self.has_instance(&c.class_name, bound) {
                continue;
            }
            let ctx_str = ctx.iter()
                .map(|c| format!("{} {}", c.class_name, c.type_var))
                .collect::<Vec<_>>().join(", ");
            let here = format!(
                "there is an instance '({}) => {} {}', but using it at '{}' needs '{} {}'",
                ctx_str, inst.class_name, paren(&inst.target_type), ty,
                c.class_name, paren(bound));
            return Some(match self.context_failure_note(&c.class_name, bound) {
                Some(deeper) => format!("{}; {}", here, deeper),
                None => format!("{}, and there is no instance '{} {}'",
                    here, c.class_name, paren(bound)),
            });
        }
        None
    }

    /// Match a use type against a registered instance's target type,
    /// positionally: `Tree Int` against `Tree a` yields {a: Int}.
    /// Only plain-variable instance arguments bind anything. Returns None when
    /// the two argument spines have different lengths — the caller then defers
    /// rather than guessing.
    fn match_instance_args(inst_target: &Ty, use_ty: &Ty) -> Option<HashMap<String, Ty>> {
        fn peel(ty: &Ty) -> Vec<&Ty> {
            let mut head = ty;
            let mut args = Vec::new();
            while let Ty::App(f, a) = head {
                args.push(a.as_ref());
                head = f.as_ref();
            }
            args.reverse();
            args
        }
        let inst_args = peel(inst_target);
        let use_args = peel(use_ty);
        if inst_args.len() != use_args.len() {
            return None;
        }
        let mut binds = HashMap::new();
        for (ia, ua) in inst_args.iter().zip(use_args.iter()) {
            if let Ty::Var(v) = ia {
                binds.insert(v.name.clone(), (*ua).clone());
            }
        }
        Some(binds)
    }

    /// Collect the type-variable leaves a `class ty` constraint ultimately needs
    /// an instance for, mirroring `has_instance`'s structural recursion (a
    /// list/tuple/Maybe of `a` needs `class a`; a derived `T a` needs `class a`).
    /// Only variable leaves are collected; concrete constructors are assumed
    /// resolved (they passed `has_instance`). Skolems are left rigid/deferred.
    pub(super) fn collect_required_var_constraints(&self, class: &str, ty: &Ty, out: &mut Vec<(String, TyVar)>) {
        match ty {
            Ty::Var(v) => out.push((class.to_string(), v.clone())),
            Ty::List(elem) if structural_container_class(class) =>
                self.collect_required_var_constraints(class, elem, out),
            Ty::Tuple(elems) if structural_container_class(class) =>
                for e in elems { self.collect_required_var_constraints(class, e, out); },
            Ty::App(_, _) => {
                let mut head = ty;
                let mut args: Vec<&Ty> = Vec::new();
                while let Ty::App(f, a) = head { args.push(a.as_ref()); head = f.as_ref(); }
                match head {
                    Ty::Con(base) if base == "Maybe" && structural_container_class(class) =>
                        for a in args { self.collect_required_var_constraints(class, a, out); },
                    Ty::Con(_) => {
                        let Some(inst) = InstHead::of(head)
                            .and_then(|h| self.instances.get(&(class.to_string(), h)))
                        else { return };
                        // Mirror has_instance: a declared context is exact
                        // (each context constraint recurses at the type its
                        // variable is bound to), builtin/derived instances
                        // stay structural (the class itself on every argument).
                        match inst.context.clone() {
                            Some(ctx) => {
                                let Some(binds) = Self::match_instance_args(&inst.target_type, ty)
                                else { return };
                                for c in &ctx {
                                    if let Some(bound) = binds.get(&c.type_var) {
                                        self.collect_required_var_constraints(
                                            &c.class_name, bound, out);
                                    }
                                }
                            }
                            None =>
                                for a in args { self.collect_required_var_constraints(class, a, out); },
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// True when a `declared` class provides `wanted`: they are the same class,
    /// or `wanted` is a transitive superclass of `declared`.
    pub(super) fn class_satisfies(&self, declared: &str, wanted: &str) -> bool {
        if declared == wanted { return true; }
        let mut stack = vec![declared.to_string()];
        let mut seen: HashSet<String> = HashSet::new();
        while let Some(c) = stack.pop() {
            if !seen.insert(c.clone()) { continue; }
            if let Some(info) = self.classes.get(&c) {
                for sup in &info.superclasses {
                    if sup == wanted { return true; }
                    stack.push(sup.clone());
                }
            }
        }
        false
    }

    /// Emit the wanted class constraints for a use of `name`, mapping each
    /// constrained variable to its freshly-instantiated type. Covers both
    /// built-in class methods (`show`/`==`/…) and any user/prelude function
    /// whose signature carries constraints (e.g. `print :: Show a => …`), so a
    /// constraint is checked wherever the function is called.
    pub(super) fn emit_use_constraints(&mut self, name: &str, inst_map: &HashMap<TyVar, Ty>) {
        let mut constraints: Vec<TyConstraint> = Vec::new();
        if let Some(cs) = self.method_constraints.get(name) {
            constraints.extend(cs.iter().cloned());
        }
        if let Some(cs) = self.fn_use_constraints.get(name) {
            constraints.extend(cs.iter().cloned());
        }
        for c in &constraints {
            if let Some(fresh) = inst_map.iter()
                .find(|(v, _)| v.name == c.type_var)
                .map(|(_, t)| t.clone())
            {
                self.wanted.push((c.class_name.clone(), fresh));
            }
        }
    }

    // --- Typeclass handling ---

    pub(super) fn register_class(&mut self, name: &str, type_var: &str, superclasses: &[String], methods: &[ClassMethod]) {
        // The class variable's kind was already inferred, order-independently
        // and with superclass agreement, by `infer_class_kinds` (pass 1b)
        // and lives in `class_kinds`. `class Foldable t where foldr :: … ->
        // t a -> b` gives `t : Type -> Type` from the use `t a`, with no
        // annotation. Instance heads are later checked against this kind.
        let tv = TyVar { name: type_var.to_string(), id: u32::MAX };
        let mut method_types = Vec::new();
        let mut default_methods = HashMap::new();

        for method in methods {
            let ty = self.ast_type_to_ty(&method.ty);
            method_types.push((method.name.clone(), ty.clone()));

            // Register class method in env as polymorphic. Quantify EVERY
            // type variable of the signature — the class variable AND the
            // method's own variables (`myfmap :: (a -> b) -> f a -> f b`
            // quantifies a, b, f). Quantifying only the class variable left
            // a and b as shared free variables, so two uses of the method in
            // one definition had to agree on them: `myfmap (+1) (Just 1)`
            // then `myfmap not (Just True)` failed with a false
            // "Cannot unify Int with Bool". Each occurrence must get a
            // fresh instantiation of the full scheme, as GHC does.
            let mut qvars: Vec<TyVar> = ty.free_vars();
            if !qvars.iter().any(|v| v.name == tv.name) {
                qvars.push(tv.clone());
            }
            // Quantify the method's rigid multiplicity variables (`consume ::
            // a %m -> b` in a class) exactly like its type variables, so each
            // use — and each instance — instantiates them independently.
            let mut qmults = Vec::new();
            ty.collect_rigid_mults(&mut qmults);
            self.env.insert(method.name.clone(), Scheme {
                vars: qvars,
                mult_vars: qmults,
                ty: ty.clone(),
            });

            // Synthesize the class constraint carried by this method, exactly
            // as the builtin classes register it by hand (method_constraints
            // for show/==/foldr/mempty/…): a use of the method emits a wanted
            // `ClassName classVar`, mapped through the instantiation at the use
            // site. This is what makes an *undetermined* use — a
            // return-position-only method such as a nullary `def :: a`, whose
            // class variable no argument pins — a compile-time ambiguity error
            // (via check_function's discharge) instead of a runtime crash, and
            // a use at a concrete instance-less type a compile-time NoInstance.
            //
            // Scoped precisely to avoid over-constraining: emit ONLY when the
            // method's signature actually mentions the class variable. When it
            // does not (a degenerate `foo :: Int`), the wanted's variable
            // would be pinned by nothing and *every* use would be ambiguous —
            // so we leave such a method exactly as it compiles today rather
            // than newly rejecting it. When the variable appears in an
            // ARGUMENT (`op :: t a -> Int`), the constraint is still
            // emitted but the discharge machinery leaves it satisfied: the
            // argument's type is a `binder_type`, so the variable is
            // "determined" and never reported as ambiguous — the same reason
            // `show x` resolves silently while `show (read s)` does not.
            let mentions_class_var = ty.free_vars().iter().any(|v| v.name == type_var);
            if mentions_class_var {
                self.method_constraints
                    .entry(method.name.clone())
                    .or_insert_with(|| vec![TyConstraint {
                        class_name: name.to_string(),
                        type_var: type_var.to_string(),
                    }]);
            }

            // Store default implementation if present
            if let Some(clauses) = &method.default_clauses {
                default_methods.insert(method.name.clone(), clauses.clone());
            }
        }

        self.classes.insert(name.to_string(), ClassInfo {
            name: name.to_string(),
            type_var: type_var.to_string(),
            superclasses: superclasses.to_vec(),
            methods: method_types,
            default_methods,
        });
    }

    /// Extract the head type constructor name from a Type.
    /// e.g. `Maybe a` -> "Maybe", `Int` -> "Int", `[a]` -> "List"
    pub(super) fn type_head_name(ty: &Type) -> Option<String> {
        match ty {
            Type::Con(name) => Some(name.clone()),
            Type::App(f, _) => Self::type_head_name(f),
            Type::List(_) => Some("List".to_string()),
            Type::IO(_) => Some("IO".to_string()),
            Type::Paren(inner) => Self::type_head_name(inner),
            _ => None,
        }
    }

    /// Convert an instance's declared context to TyConstraints over the same
    /// variable names as the (unfreshened) target type. Only the Haskell-2010
    /// form `C tyvar` is accepted, and the variable must be bound by the
    /// instance head; anything else yields an error (when `report` is set —
    /// the silent pre-registration pass leaves reporting to `check_instance`)
    /// and is dropped, so downstream passes only ever see well-formed context
    /// entries.
    fn convert_instance_context(
        &mut self,
        class_name: &str,
        target_ty: &Ty,
        context: &[Constraint],
        report: bool,
    ) -> Vec<TyConstraint> {
        let ty_str = format!("{}", target_ty);
        let head_vars: HashSet<String> =
            target_ty.free_vars().into_iter().map(|v| v.name).collect();
        let mut out = Vec::new();
        for c in context {
            let ctx_str = format!("instance {} {}", class_name, ty_str);
            if !self.classes.contains_key(&c.class_name) {
                if report {
                    self.push_error_ctx(
                        DiagnosticKind::Other(format!(
                            "Unknown typeclass '{}' in instance context", c.class_name)),
                        ctx_str,
                    );
                }
                continue;
            }
            let var = match &c.type_arg {
                Type::Var(v) => v.clone(),
                other => {
                    if report {
                        let shown = match self.ast_type_to_ty(other) {
                            t @ (Ty::Arrow(..) | Ty::App(_, _) | Ty::IO(_) | Ty::LuaIO(_, _)) =>
                                format!("({})", t),
                            t => format!("{}", t),
                        };
                        self.push_error_ctx(
                            DiagnosticKind::Other(format!(
                                "Constraint '{} {}' in the instance context must apply the class to a plain type variable: the context names which of the instance head's type variables need their own instance, so a compound type has nothing to attach to here\nnote: GHC accepts compound context types with FlexibleContexts; mata-ll supports only the Haskell 2010 form `C a`.",
                                c.class_name, shown)),
                            ctx_str,
                        );
                    }
                    continue;
                }
            };
            if !head_vars.contains(&var) {
                if report {
                    self.push_error_ctx(
                        DiagnosticKind::Other(format!(
                            "Constraint '{} {}' in the instance context mentions type variable '{}', which does not appear in the instance head '{}': a use of the instance could never determine what type '{}' is, so the constraint could never be satisfied",
                            c.class_name, var, var, ty_str, var)),
                        ctx_str,
                    );
                }
                continue;
            }
            out.push(TyConstraint { class_name: c.class_name.clone(), type_var: var });
        }
        out
    }

    /// Register an instance's identity — its (class, head) key, its context
    /// and the mangled names of the methods it will provide — BEFORE any
    /// instance method body is type-checked. Instances are globally visible in
    /// Haskell, so a method body may use its own instance (a recursive `show`
    /// on `Tree a`) or one declared later in the module; checking bodies
    /// against an incomplete instance table would reject those. Mangled method
    /// names are deterministic (`method_typestr`), so the full mapping is
    /// known before the bodies are checked; `check_instance` later re-registers
    /// the identical info. Silent by design: everything invalid (unknown
    /// class, headless target type, ill-formed context) is skipped here and
    /// reported by `check_instance`.
    pub(super) fn preregister_instance(
        &mut self,
        class_name: &str,
        target_type: &Type,
        context: &[Constraint],
        methods: &[InstanceMethod],
    ) {
        let target_ty = self.ast_type_to_ty(target_type);
        if InstHead::of(&target_ty).is_none() {
            return;
        }
        let Some(class_info) = self.classes.get(class_name).cloned() else { return; };
        let ty_str = format!("{}", target_ty);
        let provided: HashSet<&str> = methods.iter().map(|m| m.name.as_str()).collect();
        let mut method_fns = HashMap::new();
        for (method_name, _) in &class_info.methods {
            if provided.contains(method_name.as_str())
                || class_info.default_methods.contains_key(method_name)
            {
                method_fns.insert(method_name.clone(), format!("{}_{}", method_name, ty_str));
            }
        }
        let ctx = self.convert_instance_context(class_name, &target_ty, context, false);
        self.register_instance(InstanceInfo {
            class_name: class_name.to_string(),
            target_type: target_ty,
            method_fns,
            context: Some(ctx),
        });
    }

    /// Is `ty` a valid instance HEAD — a type constructor applied only to
    /// DISTINCT type variables (`Int`, `[a]`, `Maybe a`, `(a, b)`, `Pair a
    /// b`)? Not `[Int]`, `Maybe Bool`, `Pair a a`. Dispatch keys on the
    /// head constructor alone, so anything more specific is ambiguous with the
    /// general head and is rejected.
    fn instance_head_general(ty: &Ty) -> bool {
        let args: Vec<&Ty> = match ty {
            Ty::List(e) => vec![e.as_ref()],
            Ty::Tuple(es) => es.iter().collect(),
            Ty::IO(e) => vec![e.as_ref()],
            Ty::App(_, _) => {
                let mut a = Vec::new();
                let mut cur = ty;
                while let Ty::App(f, x) = cur { a.push(x.as_ref()); cur = f.as_ref(); }
                a
            }
            // Con (nullary like Int), Unit, LuaIO, etc.: no ordinary type
            // arguments to constrain here — accept.
            _ => return true,
        };
        let mut seen = HashSet::new();
        args.iter().all(|arg| matches!(arg, Ty::Var(v) if seen.insert(v.name.clone())))
    }

    pub(super) fn check_instance(
        &mut self,
        class_name: &str,
        target_type: &Type,
        context: &[Constraint],
        methods: &[InstanceMethod],
    ) -> Vec<TFunction> {
        let target_ty = self.ast_type_to_ty(target_type);
        let ty_str = format!("{}", target_ty);

        // An instance must attach to a head constructor (a named type, a list,
        // a tuple, or ()). Function types, bare type variables, etc. have no
        // instance head — nothing could ever be resolved to such an instance.
        let target_head = match InstHead::of(&target_ty) {
            Some(h) => h,
            None => {
                self.push_error_ctx(
                    DiagnosticKind::Other(format!(
                        "Cannot define an instance for '{}': an instance must be \
                         for a named type constructor, a list, a tuple, or ()",
                        ty_str
                    )),
                    format!("instance {} {}", class_name, ty_str),
                );
                return vec![];
            }
        };

        // Instance dispatch keys on the head constructor alone (InstHead), so
        // the head's type arguments must be DISTINCT type variables — an
        // argument-specialized head like `Pretty [Int]` or `Pretty (Pair
        // Int Int)` shares its head (`List`, `Pair`) with `Pretty [a]`
        // / `Pretty (Pair a b)` and every other argument choice, and would
        // silently mis-dispatch (`pretty [True]` running the `[Int]` body).
        // Reject it — GHC needs FlexibleInstances/OverlappingInstances for such
        // heads, which mata-ll does not have.
        if !Self::instance_head_general(&target_ty) {
            self.push_error_ctx(
                DiagnosticKind::Other(format!(
                    "Instance head '{}' is too specific: an instance must be for a \
                     type constructor applied to DISTINCT type variables (e.g. \
                     '[a]', 'Maybe a', 'Pair a b'), not to concrete or repeated \
                     type arguments\nnote: mata-ll resolves an instance by its head \
                     constructor only, so '{}' would be indistinguishable from every \
                     other argument choice at the same head and could silently \
                     mis-dispatch. GHC accepts this only with FlexibleInstances / \
                     OverlappingInstances, which mata-ll does not support.",
                    ty_str, ty_str
                )),
                format!("instance {} {}", class_name, ty_str),
            );
            return vec![];
        }

        // A second SOURCE instance for the same (class, head) — a duplicate or
        // an overlap that shares a head — is a compile error (GHC rejects
        // duplicate/overlapping instances). Registration is last-writer-wins, so
        // without this the later declaration would silently shadow the earlier.
        // Only local/imported instances are tracked: a user instance that
        // duplicates a Prelude one (e.g. `instance Foldable []`) shares the
        // Prelude's non-local class AND type, so the orphan check below already
        // rejects it — reporting it as a duplicate too would be redundant.
        if !self.checking_prelude
            && !self.checked_instance_heads.insert((class_name.to_string(), target_head.clone()))
        {
            self.push_error_ctx(
                DiagnosticKind::Other(format!(
                    "Duplicate instance '{} {}': an instance for this class and head \
                     constructor is already declared\nnote: mata-ll allows one \
                     instance per (class, head constructor); two declarations that \
                     share a head — a repeat, or overlapping heads like '[a]' and \
                     '[Int]' — would mis-dispatch. Remove or merge one.",
                    class_name, ty_str
                )),
                format!("instance {} {}", class_name, ty_str),
            );
            return vec![];
        }

        // Orphan instance detection: either the class or the type must be local.
        // Only the MAIN module (`checking_local`) is checked. Imported modules —
        // the stdlib (JSON, Data.Generics, …) and user libraries alike — are
        // trusted to be coherent, exactly as the implicit Prelude is: mata-ll
        // compiles the whole program together, so there is no cross-build
        // incoherence for the orphan rule to guard against in library code, and
        // a stdlib module legitimately declares instances for builtin types it
        // codes against (`instance ToJSON Int`, the generic combinator
        // instances). The check still fires for the top-level program, where it
        // catches a user adding a rogue instance for a class and type they own
        // neither of. `local_classes`/`local_types` are the main module's own
        // declarations (see `check_module_with_local_start`).
        if self.orphan_check_enabled && self.checking_local {
            let type_head = Self::type_head_name(target_type);
            let class_is_local = self.local_classes.contains(class_name);
            let type_is_local = type_head.as_ref().is_some_and(|t| self.local_types.contains(t));
            if !class_is_local && !type_is_local {
                self.push_error_ctx(
                    DiagnosticKind::Other(format!(
                        "Orphan instance: neither class '{}' nor type '{}' is defined in this module",
                        class_name, ty_str
                    )),
                    format!("instance {} {}", class_name, ty_str),
                );
            }
        }

        let class_info = match self.classes.get(class_name) {
            Some(ci) => ci.clone(),
            None => {
                self.push_error_ctx(
                    DiagnosticKind::Other(format!("Unknown typeclass '{}'", class_name)),
                    format!("instance {} {}", class_name, ty_str),
                );
                return vec![];
            }
        };

        // Check superclass constraints
        for superclass in &class_info.superclasses {
            let key = (superclass.clone(), target_head.clone());
            if !self.instances.contains_key(&key) {
                self.push_error_ctx(
                    DiagnosticKind::Other(format!(
                        "No instance of superclass '{}' for type '{}' (required by '{}')",
                        superclass, ty_str, class_name
                    )),
                    format!("instance {} {}", class_name, ty_str),
                );
            }
        }

        // Validate and convert the declared context (reporting any ill-formed
        // entry); the well-formed part is registered on the instance so a use
        // site knows what to demand of the concrete type arguments.
        let ctx = self.convert_instance_context(class_name, &target_ty, context, true);

        let mut instance_info = InstanceInfo {
            class_name: class_name.to_string(),
            target_type: target_ty.clone(),
            method_fns: HashMap::new(),
            context: Some(ctx.clone()),
        };

        let mut result_fns = Vec::new();
        let provided_methods: std::collections::HashSet<String> =
            methods.iter().map(|m| m.name.clone()).collect();

        // Substituting the class variable with the target type must not capture:
        // for `instance C [a]`, the target type's own `a` is a different variable
        // from the class's `a`, but both are spelled TyVar { name: "a", id: MAX }.
        // Substituting `a := [a]` directly makes apply_subst chase its own output
        // forever (a → [a] → [[a]] → …) and overflow the stack. Alpha-rename the
        // target type's variables to fresh ones first; only this freshened copy
        // is used to specialize method types — instance registration and error
        // messages keep the user's spelling.
        let mut renames: HashMap<TyVar, Ty> = HashMap::new();
        for v in target_ty.free_vars() {
            let fresh = self.fresh_var("_inst");
            renames.insert(v, fresh);
        }
        let fresh_target_ty = target_ty.apply_subst(&Subst::from_map(renames.clone()));

        // The declared context, re-expressed over the freshened instance
        // variables the specialized method types use. Registered as each
        // method's declared function context (fn_constraints) — exactly the
        // mechanism a constrained top-level function (`f :: Show a => …`)
        // uses — so a method body may use the context's class methods on the
        // instance's type variables, and check_function discharges those
        // wanteds against the declared context instead of rejecting them.
        let ctx_fresh: Vec<TyConstraint> = ctx.iter().filter_map(|c| {
            let (_, fresh) = renames.iter().find(|(v, _)| v.name == c.type_var)?;
            let Ty::Var(fv) = fresh else { return None };
            Some(TyConstraint { class_name: c.class_name.clone(), type_var: fv.name.clone() })
        }).collect();

        for method_def in methods {
            // Find the class method's type
            let class_method_ty = class_info.methods.iter()
                .find(|(n, _)| n == &method_def.name)
                .map(|(_, ty)| ty.clone());

            let method_ty = match class_method_ty {
                Some(ty) => {
                    // Substitute the class type variable with the target type
                    let tv = TyVar { name: class_info.type_var.clone(), id: u32::MAX };
                    let subst = Subst::singleton(tv, fresh_target_ty.clone());
                    ty.apply_subst(&subst)
                }
                None => {
                    self.push_error_ctx(
                        DiagnosticKind::Other(format!("'{}' is not a method of class '{}'",
                            method_def.name, class_name)),
                        format!("instance {} {}", class_name, ty_str),
                    );
                    continue;
                }
            };

            // Generate mangled name: show_Int, show_Bool, etc.
            let mangled_name = format!("{}_{}", method_def.name, ty_str);
            instance_info.method_fns.insert(method_def.name.clone(), mangled_name.clone());

            // Type-check the instance method against the specialized type,
            // with the instance's declared context in scope for its body.
            if !ctx_fresh.is_empty() {
                self.fn_constraints.insert(mangled_name.clone(), ctx_fresh.clone());
            }
            if let Some(tfun) = self.check_function(&mangled_name, &method_def.clauses, &method_ty) {
                result_fns.push(tfun);
            }
        }

        // Fill in default method implementations for any methods not provided by the instance
        for (method_name, method_ty) in &class_info.methods {
            if provided_methods.contains(method_name) {
                continue;
            }
            if let Some(default_clauses) = class_info.default_methods.get(method_name) {
                let tv = TyVar { name: class_info.type_var.clone(), id: u32::MAX };
                let subst = Subst::singleton(tv, fresh_target_ty.clone());
                let specialized_ty = method_ty.apply_subst(&subst);

                let mangled_name = format!("{}_{}", method_name, ty_str);
                instance_info.method_fns.insert(method_name.clone(), mangled_name.clone());

                // A default body is checked at this instance's type, so it
                // gets the same declared context as an explicit method body.
                if !ctx_fresh.is_empty() {
                    self.fn_constraints.insert(mangled_name.clone(), ctx_fresh.clone());
                }
                if let Some(tfun) = self.check_function(&mangled_name, default_clauses, &specialized_ty) {
                    result_fns.push(tfun);
                }
            }
        }

        self.register_instance(instance_info);

        result_fns
    }

    /// Expose instances for the monomorphizer
    pub fn get_instances(&self) -> &HashMap<(String, InstHead), InstanceInfo> {
        &self.instances
    }

    /// Expose typeclass constraints per function for dictionary-passing fallback
    pub fn get_fn_constraints(&self) -> &HashMap<String, Vec<TyConstraint>> {
        &self.fn_constraints
    }

    /// The declared constraints re-expressed over each checked function's
    /// FINAL type variable names (see check_function) — the names on the
    /// TFunction the monomorphizer sees. The dictionary-passing rewrite
    /// matches constraint variables against that type, so it must use these;
    /// the source-name spelling in `fn_constraints` never matches it.
    pub fn get_fn_dict_constraints(&self) -> &HashMap<String, Vec<TyConstraint>> {
        &self.fn_dict_constraints
    }

    /// Expose class definitions for the monomorphizer
    pub fn get_classes(&self) -> &HashMap<String, ClassInfo> {
        &self.classes
    }

    /// The structured argument type of each function's constraints (parallel to
    /// `get_fn_constraints`), so dictionary passing can handle a compound
    /// constraint like `GEncode (Rep a)` whose `type_var` string is opaque.
    pub fn get_fn_constraint_args(&self) -> &HashMap<String, Vec<(String, Ty)>> {
        &self.fn_constraint_args
    }

    /// Expose the lowered closed-type-family equations so monomorphization can
    /// reduce a family application (`Rep T`) to its concrete representation
    /// before dispatching a class method on it (the Generics substrate leaves
    /// `Rep a` in a method's dispatch type until `a` is pinned here).
    pub fn get_ty_families(&self) -> &crate::types::TyFamilies {
        &self.ty_families
    }
}
