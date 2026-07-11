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
            Ty::Var(_) | Ty::Skolem(..) => true,
            // No instance for functions or effectful actions, ever.
            Ty::Arrow(_, _) | Ty::Forall(_, _) | Ty::IO(_) | Ty::LuaIO(_, _) => false,
            Ty::Promoted(_) => false,
            Ty::Unit => true,
            // Lists/tuples are structural for Show and Eq (mono generates the
            // instance), but not for Ord — mata-ll has no list/tuple ordering.
            Ty::List(elem) => structural_container_class(class) && self.has_instance(class, elem),
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
                match head {
                    // Maybe is structural for Show/Eq like lists are; its built-in
                    // Eq isn't a registered instance, so check it the same way.
                    Ty::Con(base) if base == "Maybe" =>
                        structural_container_class(class) && args.iter().all(|a| self.has_instance(class, a)),
                    // Other type constructors need a registered (derived) instance.
                    Ty::Con(_) =>
                        InstHead::of(head)
                            .is_some_and(|h| self.instances.contains_key(&(class.to_string(), h)))
                            && args.iter().all(|a| self.has_instance(class, a)),
                    _ => true, // unknown head — defer rather than over-reject
                }
            }
        }
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
                    Ty::Con(_) if InstHead::of(head).is_some_and(|h|
                        self.instances.contains_key(&(class.to_string(), h))) =>
                        for a in args { self.collect_required_var_constraints(class, a, out); },
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
        let tv = TyVar { name: type_var.to_string(), id: u32::MAX };
        let mut method_types = Vec::new();
        let mut default_methods = HashMap::new();

        for method in methods {
            let ty = self.ast_type_to_ty(&method.ty);
            method_types.push((method.name.clone(), ty.clone()));

            // Register class method in env as polymorphic
            self.env.insert(method.name.clone(), Scheme {
                vars: vec![tv.clone()],
                ty,
            });

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
    /// e.g. `Maybe a` -> "Maybe", `Integer` -> "Integer", `[a]` -> "List"
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

    pub(super) fn check_instance(
        &mut self,
        class_name: &str,
        target_type: &Type,
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

        // Orphan instance detection: either the class or the type must be local.
        // Only checked when check_module_with_local_start was used (local_start tracking active).
        if self.orphan_check_enabled {
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

        let mut instance_info = InstanceInfo {
            class_name: class_name.to_string(),
            target_type: target_ty.clone(),
            method_fns: HashMap::new(),
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
        let fresh_target_ty = {
            let mut renames: HashMap<TyVar, Ty> = HashMap::new();
            for v in target_ty.free_vars() {
                let fresh = self.fresh_var("_inst");
                renames.insert(v, fresh);
            }
            target_ty.apply_subst(&Subst::from_map(renames))
        };

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

            // Generate mangled name: show_Integer, show_Bool, etc.
            let mangled_name = format!("{}_{}", method_def.name, ty_str);
            instance_info.method_fns.insert(method_def.name.clone(), mangled_name.clone());

            // Type-check the instance method against the specialized type
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

    /// Expose class definitions for the monomorphizer
    pub fn get_classes(&self) -> &HashMap<String, ClassInfo> {
        &self.classes
    }
}
