//! Monomorphization pass
//!
//! Walks the typed IR and collects all concrete type instantiations
//! of polymorphic functions. For each unique instantiation, generates
//! a specialized copy with a mangled name and rewrites call sites.

use std::collections::{HashMap, HashSet};
use crate::tir::*;
use crate::typechecker::{Checker, ClassInfo};
use crate::types::{Ty, TyVar, TyConstraint, Subst};

/// Format a "No instance" error, appending a mata-ll-specific hint when the
/// (method, type) pair is a known GHC-vs-mata-ll divergence.
fn no_instance_msg(method: &str, ty: &Ty) -> String {
    let mut msg = format!("No instance for '{}' on type '{}'", method, ty);
    let hint = match method {
        "<>" if matches!(ty, Ty::List(_)) =>
            Some("lists are concatenated with ++ in mata-ll; <> (Semigroup) is only defined for String"),
        ">" | "<" | ">=" | "<=" | "compare" | "max" | "min" if matches!(ty, Ty::Tuple(_)) =>
            Some("tuples have no Ord instance in mata-ll; compare their components individually"),
        _ => None,
    };
    if let Some(h) = hint {
        msg.push_str(&format!("\n  note: {}", h));
    }
    msg
}

/// A specialization demand: function name + concrete type arguments
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SpecKey {
    name: String,
    ty: String, // stringified type for hashing
}

pub struct Monomorphizer {
    /// Known polymorphic functions (name -> TFunction)
    poly_fns: HashMap<String, TFunction>,
    /// Set of prelude/built-in names (don't try to specialize these)
    builtins: HashSet<String>,
    /// Collected specializations: SpecKey -> mangled name
    specializations: HashMap<SpecKey, String>,
    /// Generated specialized functions
    generated: Vec<TFunction>,
    /// Typeclass method -> set of class names it belongs to
    class_methods: HashSet<String>,
    /// Methods where the class variable appears in the return type, not the first arg
    /// (e.g., Read's `read :: String -> a`, Enum's `toEnum :: Integer -> a`)
    return_type_methods: HashSet<String>,
    /// Instance resolution: (method_name, type_string) -> mangled function name
    instance_methods: HashMap<(String, String), String>,
    /// Errors collected during monomorphization
    pub errors: Vec<String>,
    /// Functions that use dictionary-passing instead of monomorphization
    dict_passing_fns: HashSet<String>,
    /// Typeclass constraints per function (from type signatures)
    fn_constraints: HashMap<String, Vec<TyConstraint>>,
    /// Class definitions (class_name -> ClassInfo)
    classes: HashMap<String, ClassInfo>,
    /// Method name -> class name (reverse lookup)
    method_to_class: HashMap<String, String>,
    /// Locally-bound names (lambda params, let binds, case pattern vars)
    /// that should NOT be resolved as typeclass methods
    locals: HashSet<String>,
}

impl Monomorphizer {
    pub fn new(checker: &Checker) -> Self {
        let mut builtins = HashSet::new();
        // Only mark as builtin functions that DON'T have user-defined instances
        for name in &[
            "putStrLn", "++", "$", ".", "id", "const",
            "flip", "not", "error", "sqrt", "otherwise", "max", "min",
            "+", "-", "*", "/", "==", "/=", "<", ">", "<=", ">=",
            "&&", "||", "mod", "div", "map", "filter", "foldl", "foldr",
            "True", "False", "Just", "Nothing",
            ":", "[]", "head", "tail", "take", "drop", "zipWith", "length", "reverse",
            "engage", "liftIO",
            "hmEmpty", "hmInsert", "hmLookup", "hmDelete",
            "hmSize", "hmKeys", "hmValues", "hmMember",
            "bsEmpty", "bsLength", "bsIndex", "bsSub", "bsSingleton",
            "bsConcat", "bsConcatList", "bsNull", "bsHead", "bsTail", "bsCons", "bsSnoc",
            "bsReplicate", "bsPack", "bsUnpack", "bsMap", "bsFoldl",
            "bsXor", "bsZipWith", "bsToString", "bsFromString",
            "bsGetU16LE", "bsGetU32LE", "bsGetI8", "bsGetI16LE", "bsPutI16LE",
            "runST", "newSTArray", "readSTArray", "writeSTArray",
            "modifySTArray", "stArrayLength", "newSTArrayFromList", "stArrayToList",
        ] {
            builtins.insert(name.to_string());
        }

        // Collect class method names and instance resolutions from the checker
        let mut class_methods = HashSet::new();
        let mut instance_methods = HashMap::new();

        for ((_class_name, _ty_str), inst) in checker.get_instances() {
            for (method_name, mangled) in &inst.method_fns {
                class_methods.insert(method_name.clone());
                // Key: (method_name, concrete_type_string)
                let ty_str = format!("{}", inst.target_type);
                instance_methods.insert((method_name.clone(), ty_str), mangled.clone());
            }
        }

        // Class methods with instances should NOT be in builtins
        // (they need to be resolved to instance methods)
        for m in &class_methods {
            builtins.remove(m);
        }
        // But also remove "show" from builtins since it's commonly a class method
        builtins.remove("show");

        // Build method -> class reverse lookup
        let mut method_to_class = HashMap::new();
        for (class_name, info) in checker.get_classes() {
            for (method_name, _) in &info.methods {
                method_to_class.insert(method_name.clone(), class_name.clone());
            }
        }

        // Build set of methods where class variable is in return position
        let mut return_type_methods = HashSet::new();
        for info in checker.get_classes().values() {
            let tv = &info.type_var;
            for (method_name, method_ty) in &info.methods {
                // Check if the class variable appears only in return position
                let first_arg_has_tv = match method_ty {
                    Ty::Arrow(a, _) => Self::ty_contains_var(a, tv),
                    _ => false,
                };
                if !first_arg_has_tv {
                    return_type_methods.insert(method_name.clone());
                }
            }
        }

        Monomorphizer {
            poly_fns: HashMap::new(),
            builtins,
            specializations: HashMap::new(),
            generated: Vec::new(),
            class_methods,
            return_type_methods,
            instance_methods,
            errors: Vec::new(),
            dict_passing_fns: HashSet::new(),
            fn_constraints: checker.get_fn_constraints().clone(),
            classes: checker.get_classes().clone(),
            method_to_class,
            locals: HashSet::new(),
        }
    }

    pub fn run(&mut self, module: TModule) -> TModule {
        // Collect polymorphic user-defined functions
        for func in &module.functions {
            if self.is_polymorphic(&func.ty) && !self.builtins.contains(&func.name) {
                self.poly_fns.insert(func.name.clone(), func.clone());
            }
        }
        // Polymorphic derived/instance methods (e.g. show_Box :: Box a -> String)
        // must be specializable per concrete type too, so `show` on a nested
        // value resolves to the concrete field show instead of the type-erased
        // runtime `show`. The generic copy still serves as the fallback.
        for func in &module.instance_fns {
            if self.is_polymorphic(&func.ty) && !self.builtins.contains(&func.name) {
                self.poly_fns.entry(func.name.clone()).or_insert_with(|| func.clone());
            }
        }

        // Monomorphize instance methods too
        let mut instance_fns: Vec<TFunction> = module.instance_fns.iter()
            .map(|f| self.mono_function(f.clone()))
            .collect();

        // Walk all functions to collect specialization demands
        let functions: Vec<TFunction> = module.functions.iter()
            .map(|f| self.mono_function(f.clone()))
            .collect();

        // Keep all original functions (including polymorphic ones — they serve
        // as fallbacks for calls inside other polymorphic contexts where types
        // aren't resolved). Append generated specializations after.
        let mut result_fns: Vec<TFunction> = functions;
        result_fns.append(&mut self.generated);

        // Rewrite dict-passing functions and their call sites
        if !self.dict_passing_fns.is_empty() {
            let dict_fns: Vec<String> = self.dict_passing_fns.iter().cloned().collect();
            for name in &dict_fns {
                if let Some(pos) = result_fns.iter().position(|f| f.name == *name) {
                    let mut func = result_fns[pos].clone();
                    self.rewrite_dict_passing_fn(&mut func);
                    result_fns[pos] = func;
                }
            }
            // Rewrite call sites in non-dict functions
            for func in &mut result_fns {
                if self.dict_passing_fns.contains(&func.name) { continue; }
                for clause in &mut func.clauses {
                    clause.body = self.rewrite_dict_call_sites(clause.body.clone());
                    clause.where_binds = clause.where_binds.iter().map(|wb| TLocalDef {
                        name: wb.name.clone(),
                        patterns: wb.patterns.clone(),
                        body: self.rewrite_dict_call_sites(wb.body.clone()),
                    }).collect();
                }
            }
            for func in &mut instance_fns {
                for clause in &mut func.clauses {
                    clause.body = self.rewrite_dict_call_sites(clause.body.clone());
                }
            }
        }

        TModule {
            data_defs: module.data_defs,
            functions: result_fns,
            instance_fns,
            has_main: module.has_main,
            exports: module.exports,
            record_accessors: module.record_accessors,
            newtypes: module.newtypes,
        }
    }

    /// Check if a parameterized instance exists (e.g., Show [a] for Show [Integer])
    /// Find a parameterized instance for a method on a concrete type.
    /// E.g., find show_PureMap for show on PureMap String Integer.
    fn resolve_parameterized_instance(&self, method: &str, concrete_ty: &Ty) -> Option<String> {
        let base = match concrete_ty {
            Ty::List(_) => "[]",
            Ty::IO(_) => "IO",
            Ty::LuaIO(_, _) => "LuaIO",
            Ty::App(f, _) => {
                let mut head = f.as_ref();
                loop {
                    match head {
                        Ty::Con(name) => break name.as_str(),
                        Ty::App(inner, _) => head = inner.as_ref(),
                        _ => return None,
                    }
                }
            }
            _ => return None,
        };
        // Look for exact base or parameterized key (e.g. "PureMap k v")
        if let Some(mangled) = self.instance_methods.get(&(method.to_string(), base.to_string())) {
            return Some(mangled.clone());
        }
        for ((m, t), mangled) in &self.instance_methods {
            if m == method && t.starts_with(&format!("{} ", base)) {
                return Some(mangled.clone());
            }
        }
        None
    }

    /// Extract a type constructor from anywhere in a type.
    /// Used for higher-kinded class methods (Functor, Applicative) where
    /// the class variable is a type constructor (f, m) applied to arguments.
    fn extract_type_constructor(ty: &Ty) -> Option<String> {
        match ty {
            Ty::IO(_) => Some("IO".to_string()),
            Ty::LuaIO(_, _) => Some("LuaIO".to_string()),
            Ty::List(_) => Some("[]".to_string()),
            Ty::App(f, _) => {
                let mut head = f.as_ref();
                loop {
                    match head {
                        Ty::Con(name) => return Some(name.clone()),
                        Ty::App(inner, _) => head = inner.as_ref(),
                        _ => return None,
                    }
                }
            }
            Ty::Arrow(from, to) => {
                // Prefer return type: for methods like pure/return, the
                // class variable (m) is in the return position, not the argument.
                Self::extract_type_constructor(to)
                    .or_else(|| Self::extract_type_constructor(from))
            }
            _ => None,
        }
    }

    fn is_polymorphic(&self, ty: &Ty) -> bool {
        !ty.free_vars().is_empty()
    }

    /// Check if a type is an IO/ST/LuaIO action (monads handled by codegen's bind chain).
    fn is_action_monad(ty: &Ty) -> bool {
        matches!(ty, Ty::IO(_) | Ty::LuaIO(_, _))
            || matches!(ty, Ty::App(f, _) if matches!(f.as_ref(),
                Ty::App(c, _) if matches!(c.as_ref(), Ty::Con(n) if n == "ST")))
    }

    /// Extract the type of the first argument from a function type
    fn ty_contains_var(ty: &Ty, var: &str) -> bool {
        match ty {
            Ty::Var(v) => v.name == var,
            Ty::Arrow(a, b) => Self::ty_contains_var(a, var) || Self::ty_contains_var(b, var),
            Ty::App(f, a) => Self::ty_contains_var(f, var) || Self::ty_contains_var(a, var),
            Ty::List(t) | Ty::IO(t) => Self::ty_contains_var(t, var),
            Ty::Tuple(ts) => ts.iter().any(|t| Self::ty_contains_var(t, var)),
            _ => false,
        }
    }

    fn first_arg_type(&self, ty: &Ty) -> Option<Ty> {
        match ty {
            Ty::Arrow(a, _) => Some(*a.clone()),
            _ => None,
        }
    }

    fn return_type(&self, ty: &Ty) -> Option<Ty> {
        match ty {
            Ty::Arrow(_, b) => {
                // Get the final return type (innermost non-arrow)
                let mut current = b.as_ref();
                while let Ty::Arrow(_, next) = current {
                    current = next.as_ref();
                }
                Some(current.clone())
            }
            _ => None,
        }
    }

    fn mangle_name(&mut self, name: &str, ty: &Ty) -> String {
        let ty_str = self.ty_to_suffix(ty);
        format!("{}_{}", name, ty_str)
    }

    /// Specialize a derived `show` for a parameterized user data type at a
    /// concrete argument type. The generic `show_Box :: Box a -> String` shows
    /// its fields with the type-erased runtime `show` (which can't recover
    /// constructor names, so nested values print as tuples). Cloning it per
    /// concrete type and re-running monomorphization on the body resolves each
    /// field's `show` to the concrete field type — recursively. Returns `base`
    /// unchanged when there is nothing to specialize (monomorphic type, still
    /// polymorphic, or the polymorphic-recursion guard trips).
    fn specialize_derived_show(&mut self, base: &str, arg_ty: &Ty) -> String {
        if self.is_polymorphic(arg_ty) || !self.poly_fns.contains_key(base) {
            return base.to_string();
        }
        let fn_ty = Ty::arrow(arg_ty.clone(), Ty::Con("String".into()));
        let key = SpecKey { name: base.to_string(), ty: format!("{}", fn_ty) };
        if let Some(existing) = self.specializations.get(&key) {
            return existing.clone();
        }
        if self.specializations.keys().filter(|k| k.name == base).count() > 16 {
            return base.to_string();
        }
        let mangled = self.mangle_name(base, &fn_ty);
        // Insert before generating the body so a recursive field of the same
        // type resolves to this same specialization instead of looping.
        self.specializations.insert(key, mangled.clone());
        if let Some(poly_fn) = self.poly_fns.get(base).cloned() {
            let subst = Self::compute_body_subst(&poly_fn, &fn_ty);
            let mut spec_fn = poly_fn.clone();
            spec_fn.name = mangled.clone();
            spec_fn.ty = fn_ty;
            spec_fn.clauses = spec_fn.clauses.into_iter()
                .map(|c| c.apply_subst(&subst))
                .collect();
            spec_fn.specialized = true;
            spec_fn = self.mono_function(spec_fn);
            self.generated.push(spec_fn);
        }
        mangled
    }

    fn ty_to_suffix(&self, ty: &Ty) -> String {
        match ty {
            Ty::Con(name) => name.clone(),
            Ty::Var(v) => format!("v{}", v.name),
            Ty::Arrow(a, b) => format!("{}T{}", self.ty_to_suffix(a), self.ty_to_suffix(b)),
            Ty::App(a, b) => format!("{}A{}", self.ty_to_suffix(a), self.ty_to_suffix(b)),
            Ty::List(a) => format!("L{}", self.ty_to_suffix(a)),
            Ty::IO(a) => format!("IO{}", self.ty_to_suffix(a)),
            Ty::LuaIO(s, a) => format!("LIO{}_{}", s.name, self.ty_to_suffix(a)),
            Ty::Forall(_, inner) => self.ty_to_suffix(inner),
            Ty::Skolem(name, id) => format!("SK{}_{}", name, id),
            Ty::Unit => "Unit".to_string(),
            Ty::Tuple(elems) => format!("Tup{}", elems.iter().map(|e| self.ty_to_suffix(e)).collect::<Vec<_>>().join("_")),
            Ty::Promoted(name) => format!("P{}", name),
        }
    }

    fn mono_function(&mut self, mut func: TFunction) -> TFunction {
        func.clauses = func.clauses.into_iter()
            .map(|c| self.mono_clause(c))
            .collect();
        func
    }

    fn mono_clause(&mut self, mut clause: TClause) -> TClause {
        // Add clause pattern-bound variables to locals
        let mut pat_vars = Vec::new();
        for pat in &clause.patterns {
            pat_vars.extend(Self::pattern_vars(pat));
        }
        let new_vars: Vec<_> = pat_vars.iter()
            .filter(|n| !self.locals.contains(*n))
            .cloned().collect();
        for name in &pat_vars {
            self.locals.insert(name.clone());
        }
        clause.body = self.mono_expr(clause.body);
        clause.guards = clause.guards.into_iter().map(|g| TGuard {
            condition: self.mono_expr(g.condition),
            body: self.mono_expr(g.body),
        }).collect();
        clause.where_binds = clause.where_binds.into_iter().map(|ld| TLocalDef {
            name: ld.name,
            patterns: ld.patterns,
            body: self.mono_expr(ld.body),
        }).collect();
        for name in &new_vars {
            self.locals.remove(name);
        }
        clause
    }

    fn pattern_vars(pat: &TPattern) -> Vec<String> {
        match pat {
            TPattern::Var(name, _) => vec![name.clone()],
            TPattern::Wildcard | TPattern::LitPat(_) => vec![],
            TPattern::Constructor { args, .. } => {
                args.iter().flat_map(Self::pattern_vars).collect()
            }
            TPattern::Paren(p) => Self::pattern_vars(p),
            TPattern::Tuple(ps) => ps.iter().flat_map(Self::pattern_vars).collect(),
        }
    }

    fn mono_expr(&mut self, expr: TExpr) -> TExpr {
        // Iterative right-spine processing for bind chains
        enum SpineFrame {
            Bind { ty: Ty, op: String, lhs: TExpr, lambda_ty: Ty, params: Vec<(String, Ty)> },
            Seq { ty: Ty, op: String, lhs: TExpr },
            Let { ty: Ty, binds: Vec<TLocalDef> },
        }

        let mut spine: Vec<SpineFrame> = Vec::new();
        let mut current = expr;

        loop {
            match &current.kind {
                TExprKind::InfixApp { op, lhs, .. } if (op == ">>=" || op == ">>") && Self::is_action_monad(&lhs.ty) => {
                    let ty = current.ty.clone();
                    if let TExprKind::InfixApp { op, lhs, rhs } = current.kind {
                        let rhs_ty = rhs.ty.clone();
                        if op == ">>="
                            && let TExprKind::Lambda { params, body } = rhs.kind {
                                let lhs = self.mono_expr(*lhs);
                                spine.push(SpineFrame::Bind {
                                    ty, op,
                                    lhs,
                                    lambda_ty: rhs_ty,
                                    params,
                                });
                                current = *body;
                                continue;
                            }
                        let lhs = self.mono_expr(*lhs);
                        spine.push(SpineFrame::Seq { ty, op, lhs });
                        current = *rhs;
                        continue;
                    }
                    unreachable!();
                }
                TExprKind::Let { .. } if !spine.is_empty() => {
                    let ty = current.ty.clone();
                    if let TExprKind::Let { binds, body } = current.kind {
                        let binds = binds.into_iter().map(|b| TLocalDef {
                            name: b.name,
                            patterns: b.patterns,
                            body: self.mono_expr(b.body),
                        }).collect();
                        spine.push(SpineFrame::Let { ty, binds });
                        current = *body;
                        continue;
                    }
                    unreachable!();
                }
                _ => break,
            }
        }

        // Process terminal node normally
        let mut result = self.mono_expr_node(current);

        // Reconstruct spine bottom-up
        for frame in spine.into_iter().rev() {
            result = match frame {
                SpineFrame::Bind { ty, op, lhs, lambda_ty, params } => TExpr {
                    kind: TExprKind::InfixApp {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(TExpr {
                            kind: TExprKind::Lambda { params, body: Box::new(result) },
                            ty: lambda_ty,
                        }),
                    },
                    ty,
                },
                SpineFrame::Seq { ty, op, lhs } => TExpr {
                    kind: TExprKind::InfixApp { op, lhs: Box::new(lhs), rhs: Box::new(result) },
                    ty,
                },
                SpineFrame::Let { ty, binds } => TExpr {
                    kind: TExprKind::Let { binds, body: Box::new(result) },
                    ty,
                },
            };
        }
        result
    }

    fn mono_expr_node(&mut self, expr: TExpr) -> TExpr {
        let ty = expr.ty.clone();
        let kind = match expr.kind {
            TExprKind::Var(ref name) => {
                // 1. Check for typeclass method resolution
                // Skip if the name is a locally-bound variable (parameter, let, case pattern)
                if self.class_methods.contains(name) && !self.locals.contains(name) && !self.is_polymorphic(&ty) {
                    // For methods where the class variable is in the return position,
                    // resolve using the return type
                    if self.return_type_methods.contains(name) {
                        // For nullary methods (minBound, maxBound), the type IS the result
                        let ret_ty = self.return_type(&ty).unwrap_or_else(|| ty.clone());
                        let ret_str = format!("{}", ret_ty);
                        let key = (name.clone(), ret_str);
                        if let Some(mangled) = self.instance_methods.get(&key).cloned() {
                            return TExpr { kind: TExprKind::Var(mangled), ty };
                        }
                        // Parameterized lookup on return type (e.g. ST s [ByteString] → "ST")
                        if let Some(mangled) = self.resolve_parameterized_instance(name, &ret_ty) {
                            return TExpr { kind: TExprKind::Var(mangled), ty };
                        }
                    }
                    // Standard: use first argument type
                    if let Some(arg_ty) = self.first_arg_type(&ty) {
                        let ty_str = format!("{}", arg_ty);
                        let key = (name.clone(), ty_str.clone());
                        if let Some(mangled) = self.instance_methods.get(&key).cloned() {
                            return TExpr { kind: TExprKind::Var(mangled), ty };
                        } else if let Some(mangled) = self.resolve_parameterized_instance(name, &arg_ty) {
                            if name == "show" {
                                if let Some(specialized) = self.generate_container_show(&arg_ty) {
                                    return TExpr { kind: TExprKind::Var(specialized), ty };
                                }
                                let specialized = self.specialize_derived_show(&mangled, &arg_ty);
                                return TExpr { kind: TExprKind::Var(specialized), ty };
                            }
                            return TExpr { kind: TExprKind::Var(mangled), ty };
                        } else if let Ty::Tuple(elem_tys) = &arg_ty
                            && name == "show" {
                                let mangled = self.generate_tuple_show(elem_tys);
                                return TExpr { kind: TExprKind::Var(mangled), ty };
                            }
                    }
                    // Fallback: for higher-kinded methods (Functor/Applicative),
                    // extract the type constructor from any position in the type
                    if let Some(tc) = Self::extract_type_constructor(&ty) {
                        let key = (name.clone(), tc);
                        if let Some(mangled) = self.instance_methods.get(&key).cloned() {
                            return TExpr { kind: TExprKind::Var(mangled), ty };
                        }
                    }
                    // No resolution found for a class method on a concrete type
                    if let Some(arg_ty) = self.first_arg_type(&ty) {
                        self.errors.push(no_instance_msg(&name, &arg_ty));
                    }
                }
                // 2. Check for polymorphic function specialization
                // Handle calls inside specializations: if the type is still
                // polymorphic but we have specialization(s) for this function
                // name, use the most recent one (the recursive/sibling call
                // shares the same concrete type as the enclosing specialization)
                if self.poly_fns.contains_key(name) && !self.locals.contains(name) && !self.dict_passing_fns.contains(name) && self.is_polymorphic(&ty) {
                    let specs: Vec<_> = self.specializations.iter()
                        .filter(|(k, _)| k.name == *name)
                        .map(|(_, v)| v.clone())
                        .collect();
                    if !specs.is_empty() {
                        return TExpr { kind: TExprKind::Var(specs.last().unwrap().clone()), ty };
                    }
                }
                if self.poly_fns.contains_key(name) && !self.locals.contains(name) && !self.dict_passing_fns.contains(name) && !self.is_polymorphic(&ty) {
                    let key = SpecKey { name: name.clone(), ty: format!("{}", ty) };
                    let mangled = if let Some(existing) = self.specializations.get(&key) {
                        existing.clone()
                    } else {
                        // Check for polymorphic recursion (too many specializations)
                        let spec_count = self.specializations.keys()
                            .filter(|k| k.name == *name)
                            .count();
                        if spec_count > 16 {
                            // Switch to dictionary-passing for this function
                            self.dict_passing_fns.insert(name.clone());
                            self.specializations.retain(|k, _| k.name != *name);
                            self.generated.retain(|f| !f.name.starts_with(&format!("{}_", name)));
                            return TExpr { kind: expr.kind, ty };
                        }
                        let mangled = self.mangle_name(name, &ty);
                        self.specializations.insert(key, mangled.clone());
                        if let Some(poly_fn) = self.poly_fns.get(name).cloned() {
                            let mut spec_fn = poly_fn.clone();
                            spec_fn.name = mangled.clone();
                            // Apply type substitution to body for correct method resolution
                            let subst = Self::compute_body_subst(&poly_fn, &ty);
                            spec_fn.ty = ty.clone();
                            spec_fn.clauses = spec_fn.clauses.into_iter()
                                .map(|c| c.apply_subst(&subst))
                                .collect();
                            spec_fn.specialized = true;
                            spec_fn = self.mono_function(spec_fn);
                            self.generated.push(spec_fn);
                        }
                        mangled
                    };
                    TExprKind::Var(mangled)
                } else {
                    expr.kind
                }
            }
            TExprKind::App(func, arg) => {
                // Check for class method application: describe arg
                // where describe is a class method and arg has a concrete type
                // Skip if the name is a locally-bound variable (parameter, let, case pattern)
                if let TExprKind::Var(ref fname) = func.kind
                    && self.class_methods.contains(fname) && !self.locals.contains(fname) {
                        let arg_ty = &arg.ty;
                        if !self.is_polymorphic(arg_ty) {
                            let mut resolved = None;
                            // For methods where the class variable is in the return
                            // position (Read, toEnum), resolve against the result type
                            if self.return_type_methods.contains(fname) {
                                // For methods where the class variable is in the return
                                // position (pure, return, toEnum), resolve using result
                                // type — the arg type is the wrapped value, not the container.
                                if !self.is_polymorphic(&ty) {
                                    let ret_str = format!("{}", ty);
                                    let ret_key = (fname.clone(), ret_str);
                                    resolved = self.instance_methods.get(&ret_key).cloned();
                                }
                                // Parameterized lookup on result type
                                // (handles ST s a where s is still a type variable)
                                if resolved.is_none() {
                                    resolved = self.resolve_parameterized_instance(fname, &ty);
                                }
                                // Do NOT fall through to arg_ty resolution for
                                // return-type methods — the arg type would incorrectly
                                // match the inner container (e.g. [a] inside ST s [a])
                            } else if resolved.is_none() {
                                let ty_str = format!("{}", arg_ty);
                                let key = (fname.clone(), ty_str);
                                resolved = self.instance_methods.get(&key).cloned()
                                    .or_else(|| self.resolve_parameterized_instance(fname, arg_ty));
                            }
                            // For show on containers/tuples, generate specialized instances
                            if fname == "show" {
                                if let Ty::Tuple(elem_tys) = arg_ty {
                                    resolved = Some(self.generate_tuple_show(elem_tys));
                                } else if resolved.is_some() {
                                    if let Some(specialized) = self.generate_container_show(arg_ty) {
                                        resolved = Some(specialized);
                                    } else if let Some(base) = resolved.clone() {
                                        resolved = Some(self.specialize_derived_show(&base, arg_ty));
                                    }
                                }
                            }
                            // Fallback: extract type constructor from result type
                            // (for higher-kinded methods like fmap, pure)
                            if resolved.is_none()
                                && let Some(tc) = Self::extract_type_constructor(&ty) {
                                    let key2 = (fname.clone(), tc.clone());
                                    resolved = self.instance_methods.get(&key2).cloned();
                                }
                            if let Some(mangled) = resolved {
                                let mono_arg = self.mono_expr(*arg);
                                return TExpr {
                                    kind: TExprKind::App(
                                        Box::new(TExpr::new(TExprKind::Var(mangled), func.ty.clone())),
                                        Box::new(mono_arg),
                                    ),
                                    ty,
                                };
                            }
                        }
                    }
                TExprKind::App(
                    Box::new(self.mono_expr(*func)),
                    Box::new(self.mono_expr(*arg)),
                )
            }
            TExprKind::InfixApp { op, lhs, rhs } => {
                // Check for typeclass method resolution on infix operators
                if (self.class_methods.contains(&op) || (op == "/=" && self.class_methods.contains("==")))
                    && !self.locals.contains(&op) {
                    #[allow(unused_assignments)]
                    let mut resolved: Option<String> = None;

                    // For /=, resolve via == (the registered method) and wrap in not later
                    let lookup_op = if op == "/=" { "==".to_string() } else { op.clone() };

                    if !self.is_polymorphic(&lhs.ty) {
                        let ty_str = format!("{}", lhs.ty);
                        let key = (lookup_op.clone(), ty_str.clone());

                        // 1. Direct instance lookup
                        resolved = self.instance_methods.get(&key).cloned();

                        // 2. Specialized eq/ne generation for containers and tuples
                        if resolved.is_none() && (op == "==" || op == "/=") {
                            if let Ty::List(elem_ty) = &lhs.ty {
                                resolved = Some(self.generate_list_eq(elem_ty));
                            } else if Self::is_maybe_type(&lhs.ty) {
                                let inner_ty = Self::maybe_inner_type(&lhs.ty).unwrap();
                                resolved = Some(self.generate_maybe_eq(&inner_ty));
                            } else if let Ty::Tuple(elem_tys) = &lhs.ty {
                                resolved = Some(self.generate_tuple_eq(elem_tys));
                            }
                        }

                        // 3. Parameterized instance resolution (extract type constructor from LHS)
                        if resolved.is_none() {
                            resolved = self.resolve_parameterized_instance(&lookup_op, &lhs.ty);
                        }

                        // 4. Higher-kinded fallback: extract type constructor from result type
                        if resolved.is_none()
                            && let Some(tc) = Self::extract_type_constructor(&ty) {
                                resolved = self.instance_methods.get(&(lookup_op.clone(), tc)).cloned();
                            }
                    } else {
                        // Polymorphic type: try to resolve by type constructor if known.
                        // This handles derived instance methods (e.g. eq_Tree) where
                        // field types like `Tree a` are polymorphic but the type
                        // constructor has a registered instance.
                        if op == "==" || op == "/=" {
                            if let Ty::List(elem_ty) = &lhs.ty {
                                resolved = Some(self.generate_list_eq(elem_ty));
                            } else if Self::is_maybe_type(&lhs.ty) {
                                let inner_ty = Self::maybe_inner_type(&lhs.ty).unwrap();
                                resolved = Some(self.generate_maybe_eq(&inner_ty));
                            } else if let Ty::Tuple(elem_tys) = &lhs.ty {
                                resolved = Some(self.generate_tuple_eq(elem_tys));
                            }
                        }
                        if resolved.is_none() {
                            let ty_str = format!("{}", lhs.ty);
                            let key = (lookup_op.clone(), ty_str);
                            resolved = self.instance_methods.get(&key).cloned();
                        }
                        if resolved.is_none()
                            && let Some(tc) = Self::extract_type_constructor(&lhs.ty) {
                                resolved = self.instance_methods.get(&(lookup_op, tc)).cloned();
                            }
                    }

                    if let Some(mangled) = resolved {
                        // For /= operators, wrap in not
                        if op == "/=" {
                            let mono_lhs = self.mono_expr(*lhs);
                            let mono_rhs = self.mono_expr(*rhs);
                            let eq_call = TExpr {
                                kind: TExprKind::App(
                                    Box::new(TExpr::new(
                                        TExprKind::App(
                                            Box::new(TExpr::new(TExprKind::Var(mangled), Ty::Unit)),
                                            Box::new(mono_lhs),
                                        ),
                                        Ty::Unit,
                                    )),
                                    Box::new(mono_rhs),
                                ),
                                ty: ty.clone(),
                            };
                            return TExpr {
                                kind: TExprKind::App(
                                    Box::new(TExpr::new(TExprKind::Var("not_".to_string()), Ty::Unit)),
                                    Box::new(eq_call),
                                ),
                                ty,
                            };
                        }
                        // If the mangled name matches the operator, keep as InfixApp
                        // (e.g. IO monad's >>= stays >>=)
                        if mangled == op {
                            // fall through to normal InfixApp handling below
                        } else {
                            let mono_lhs = self.mono_expr(*lhs);
                            let mono_rhs = self.mono_expr(*rhs);
                            return TExpr {
                                kind: TExprKind::App(
                                    Box::new(TExpr::new(
                                        TExprKind::App(
                                            Box::new(TExpr::new(TExprKind::Var(mangled), Ty::Unit)),
                                            Box::new(mono_lhs),
                                        ),
                                        Ty::Unit,
                                    )),
                                    Box::new(mono_rhs),
                                ),
                                ty,
                            };
                        }
                    } else if !self.is_polymorphic(&lhs.ty) {
                        // No resolution found for a class method on a concrete type
                        self.errors.push(no_instance_msg(&op, &lhs.ty));
                    }
                }
                TExprKind::InfixApp {
                    op,
                    lhs: Box::new(self.mono_expr(*lhs)),
                    rhs: Box::new(self.mono_expr(*rhs)),
                }
            }
            TExprKind::Negate(inner) => TExprKind::Negate(Box::new(self.mono_expr(*inner))),
            TExprKind::Lambda { params, body } => {
                let saved_locals: Vec<_> = params.iter()
                    .filter(|(name, _)| !self.locals.contains(name))
                    .map(|(name, _)| name.clone())
                    .collect();
                for (name, _) in &params {
                    self.locals.insert(name.clone());
                }
                let body = Box::new(self.mono_expr(*body));
                for name in &saved_locals {
                    self.locals.remove(name);
                }
                TExprKind::Lambda { params, body }
            }
            TExprKind::If { cond, then_branch, else_branch } => {
                TExprKind::If {
                    cond: Box::new(self.mono_expr(*cond)),
                    then_branch: Box::new(self.mono_expr(*then_branch)),
                    else_branch: Box::new(self.mono_expr(*else_branch)),
                }
            }
            TExprKind::Case { scrutinee, branches } => {
                TExprKind::Case {
                    scrutinee: Box::new(self.mono_expr(*scrutinee)),
                    branches: branches.into_iter().map(|b| {
                        let pat_vars = Self::pattern_vars(&b.pattern);
                        let new_vars: Vec<_> = pat_vars.iter()
                            .filter(|n| !self.locals.contains(*n))
                            .cloned().collect();
                        for name in &pat_vars {
                            self.locals.insert(name.clone());
                        }
                        let result = TCaseBranch {
                            pattern: b.pattern,
                            guards: b.guards,
                            body: self.mono_expr(b.body),
                        };
                        for name in &new_vars {
                            self.locals.remove(name);
                        }
                        result
                    }).collect(),
                }
            }
            TExprKind::Let { binds, body } => {
                let new_names: Vec<_> = binds.iter()
                    .filter(|b| !self.locals.contains(&b.name))
                    .map(|b| b.name.clone())
                    .collect();
                for b in &binds {
                    self.locals.insert(b.name.clone());
                }
                let binds = binds.into_iter().map(|b| TLocalDef {
                    name: b.name, patterns: b.patterns,
                    body: self.mono_expr(b.body),
                }).collect();
                let body = Box::new(self.mono_expr(*body));
                for name in &new_names {
                    self.locals.remove(name);
                }
                TExprKind::Let { binds, body }
            }
            TExprKind::Paren(inner) => TExprKind::Paren(Box::new(self.mono_expr(*inner))),
            TExprKind::Tuple(elems) => TExprKind::Tuple(elems.into_iter().map(|e| self.mono_expr(e)).collect()),
            TExprKind::RecordUpdate { record, updates, num_fields } => TExprKind::RecordUpdate {
                record: Box::new(self.mono_expr(*record)),
                updates: updates.into_iter().map(|(n, idx, e)| (n, idx, self.mono_expr(e))).collect(),
                num_fields,
            },
            TExprKind::OutgoingCallback { callee, arity, marshal_args, run_io, marshal_ret } =>
                TExprKind::OutgoingCallback {
                    callee: Box::new(self.mono_expr(*callee)),
                    arity, marshal_args, run_io, marshal_ret,
                },
            other => other,
        };
        TExpr { kind, ty }
    }

    /// Generate a specialized show for a container type (List, Maybe, etc.)
    /// Returns the mangled name if generated, None if not applicable.
    /// Resolve the eq function for a type, generating specialized eq if needed.
    fn resolve_elem_eq(&mut self, ty: &Ty) -> String {
        // Type variables: use generic raw-equality function
        if matches!(ty, Ty::Var(_)) {
            return "__mll_eq".to_string();
        }
        // Check if we already have an eq for this type
        if let Some(existing) = self.instance_methods.get(&("==".to_string(), format!("{}", ty))).cloned() {
            return existing;
        }
        // Recursively generate specialized eq for containers/tuples
        if let Ty::List(elem_ty) = ty {
            return self.generate_list_eq(elem_ty);
        }
        if let Ty::Tuple(elem_tys) = ty {
            return self.generate_tuple_eq(elem_tys);
        }
        if Self::is_maybe_type(ty)
            && let Some(inner_ty) = Self::maybe_inner_type(ty) {
                return self.generate_maybe_eq(&inner_ty);
            }
        // Try type constructor for parameterized types
        if let Some(tc) = Self::extract_type_constructor(ty)
            && let Some(existing) = self.instance_methods.get(&("==".to_string(), tc)).cloned() {
                return existing;
            }
        "__mll_eq".to_string()
    }

    /// Generate a specialized eq function for a tuple type.
    fn generate_tuple_eq(&mut self, elem_tys: &[Ty]) -> String {
        let tuple_ty = Ty::Tuple(elem_tys.to_vec());
        let mangled = format!("eq_{}", self.ty_to_suffix(&tuple_ty));

        let key = ("==".to_string(), format!("{}", tuple_ty));
        if let Some(existing) = self.instance_methods.get(&key) {
            return existing.clone();
        }
        self.instance_methods.insert(key, mangled.clone());

        // Resolve eq for each element type (recursively generates specialized eq)
        let elem_tys_owned: Vec<Ty> = elem_tys.to_vec();
        let mut elem_eq_names = Vec::new();
        for et in &elem_tys_owned {
            elem_eq_names.push(self.resolve_elem_eq(et));
        }

        // Build body: eq_E1(a[1], b[1]) and eq_E2(a[2], b[2]) and ...
        let bool_ty = Ty::Con("Bool".to_string());
        let a = "_a".to_string();
        let b = "_b".to_string();

        // Encode element eq names in the SpecCall
        let eq_spec = format!("__mll_tuple_eq:{}:{}", elem_tys.len(),
            elem_eq_names.join(","));

        let body = TExpr::new(
            TExprKind::SpecCall {
                original: mangled.clone(),
                specialized: eq_spec,
                args: vec![
                    TExpr::new(TExprKind::Var(a.clone()), tuple_ty.clone()),
                    TExpr::new(TExprKind::Var(b.clone()), tuple_ty.clone()),
                ],
            },
            bool_ty.clone(),
        );

        let func = TFunction {
            name: mangled.clone(),
            ty: Ty::fun(&[tuple_ty.clone(), tuple_ty], bool_ty),
            clauses: vec![TClause {
                patterns: vec![
                    TPattern::Var(a, Ty::Unit),
                    TPattern::Var(b, Ty::Unit),
                ],
                guards: vec![],
                body,
                where_binds: vec![],
            }],
            specialized: true,
            dict_params: vec![],
        };
        self.generated.push(func);
        mangled
    }

    fn generate_list_eq(&mut self, elem_ty: &Ty) -> String {
        let list_ty = Ty::List(Box::new(elem_ty.clone()));
        let mangled = format!("eq_{}", self.ty_to_suffix(&list_ty));

        let key = ("==".to_string(), format!("{}", list_ty));
        if let Some(existing) = self.instance_methods.get(&key) {
            return existing.clone();
        }
        self.instance_methods.insert(key, mangled.clone());

        let elem_ty_owned = elem_ty.clone();
        let elem_eq = self.resolve_elem_eq(&elem_ty_owned);

        let bool_ty = Ty::Con("Bool".to_string());
        let body = TExpr::new(
            TExprKind::SpecCall {
                original: mangled.clone(),
                specialized: format!("__mll_list_eq:{}", elem_eq),
                args: vec![
                    TExpr::new(TExprKind::Var("_a".into()), list_ty.clone()),
                    TExpr::new(TExprKind::Var("_b".into()), list_ty.clone()),
                ],
            },
            bool_ty.clone(),
        );

        let func = TFunction {
            name: mangled.clone(),
            ty: Ty::fun(&[list_ty.clone(), list_ty], bool_ty),
            clauses: vec![TClause {
                patterns: vec![
                    TPattern::Var("_a".into(), Ty::Unit),
                    TPattern::Var("_b".into(), Ty::Unit),
                ],
                guards: vec![], body, where_binds: vec![],
            }],
            specialized: true,
            dict_params: vec![],
        };
        self.generated.push(func);
        mangled
    }

    fn is_maybe_type(ty: &Ty) -> bool {
        matches!(ty, Ty::App(f, _) if matches!(f.as_ref(), Ty::Con(n) if n == "Maybe"))
    }

    fn maybe_inner_type(ty: &Ty) -> Option<Ty> {
        if let Ty::App(f, inner) = ty
            && matches!(f.as_ref(), Ty::Con(n) if n == "Maybe") {
                return Some(*inner.clone());
            }
        None
    }

    fn generate_maybe_eq(&mut self, inner_ty: &Ty) -> String {
        let maybe_ty = Ty::app(Ty::Con("Maybe".into()), inner_ty.clone());
        let mangled = format!("eq_{}", self.ty_to_suffix(&maybe_ty));

        let key = ("==".to_string(), format!("{}", maybe_ty));
        if let Some(existing) = self.instance_methods.get(&key) {
            return existing.clone();
        }
        self.instance_methods.insert(key, mangled.clone());

        let inner_ty_owned = inner_ty.clone();
        let elem_eq = self.resolve_elem_eq(&inner_ty_owned);

        let bool_ty = Ty::Con("Bool".to_string());
        let body = TExpr::new(
            TExprKind::SpecCall {
                original: mangled.clone(),
                specialized: format!("__mll_maybe_eq:{}", elem_eq),
                args: vec![
                    TExpr::new(TExprKind::Var("_a".into()), maybe_ty.clone()),
                    TExpr::new(TExprKind::Var("_b".into()), maybe_ty.clone()),
                ],
            },
            bool_ty.clone(),
        );

        let func = TFunction {
            name: mangled.clone(),
            ty: Ty::fun(&[maybe_ty.clone(), maybe_ty], bool_ty),
            clauses: vec![TClause {
                patterns: vec![
                    TPattern::Var("_a".into(), Ty::Unit),
                    TPattern::Var("_b".into(), Ty::Unit),
                ],
                guards: vec![], body, where_binds: vec![],
            }],
            specialized: true,
            dict_params: vec![],
        };
        self.generated.push(func);
        mangled
    }

    fn generate_container_show(&mut self, ty: &Ty) -> Option<String> {
        match ty {
            Ty::List(elem_ty) =>
                Some(self.generate_threaded_show(ty, elem_ty, "__mll_show_list")),
            // Maybe is type-directed: Just x and x share a runtime rep, so the
            // element show plus the type recover the structure (nil == Nothing).
            Ty::App(f, elem_ty) if matches!(f.as_ref(), Ty::Con(n) if n == "Maybe") =>
                Some(self.generate_threaded_show(ty, elem_ty, "__mll_show_maybe")),
            _ => None,
        }
    }

    /// Generate a `show` wrapper that threads the element show into a runtime
    /// helper (`__mll_show_list` / `__mll_show_maybe`). Shared by the container
    /// arms above so list and Maybe stay in lockstep.
    fn generate_threaded_show(&mut self, ty: &Ty, elem_ty: &Ty, runtime_fn: &str) -> String {
        let mangled = format!("show_{}", self.ty_to_suffix(ty));
        let key = ("show".to_string(), format!("{}", ty));
        if let Some(existing) = self.instance_methods.get(&key) {
            return existing.clone();
        }
        self.instance_methods.insert(key, mangled.clone());

        // Resolve show for the element type
        let elem_show = self.resolve_show_for(elem_ty);

        let str_ty = Ty::Con("String".to_string());
        let param = "_x".to_string();
        let body = TExpr::new(
            TExprKind::SpecCall {
                original: mangled.clone(),
                specialized: format!("{}:{}", runtime_fn, elem_show),
                args: vec![TExpr::new(TExprKind::Var(param.clone()), ty.clone())],
            },
            str_ty.clone(),
        );
        let func = TFunction {
            name: mangled.clone(),
            ty: Ty::arrow(ty.clone(), str_ty),
            clauses: vec![TClause {
                patterns: vec![TPattern::Var(param, Ty::Unit)],
                guards: vec![],
                body,
                where_binds: vec![],
            }],
            specialized: true,
            dict_params: vec![],
        };
        self.generated.push(func);
        mangled
    }

    /// Resolve the show function name for a given type.
    fn resolve_show_for(&mut self, ty: &Ty) -> String {
        let ty_str = format!("{}", ty);
        let key = ("show".to_string(), ty_str);
        if let Some(mangled) = self.instance_methods.get(&key) {
            return mangled.clone();
        }
        if let Ty::Tuple(elems) = ty {
            return self.generate_tuple_show(elems);
        }
        if let Ty::List(_) = ty
            && let Some(mangled) = self.generate_container_show(ty) {
                return mangled;
            }
        if self.resolve_parameterized_instance("show", ty).is_some() {
            // Has a generic instance — generate container show
            if let Some(mangled) = self.generate_container_show(ty) {
                return mangled;
            }
        }
        // Fallback to generic runtime show
        "show".to_string()
    }

    /// Generate a specialized show function for a tuple type.
    /// show_(Integer, String) produces: function(t) return "(" .. show_Integer(t[1]) .. ", " .. show_String(t[2]) .. ")" end
    fn generate_tuple_show(&mut self, elem_tys: &[Ty]) -> String {
        let tuple_ty = Ty::Tuple(elem_tys.to_vec());
        let mangled = format!("show_{}", self.ty_to_suffix(&tuple_ty));

        // Check if already generated
        let key = ("show".to_string(), format!("{}", tuple_ty));
        if let Some(existing) = self.instance_methods.get(&key) {
            return existing.clone();
        }
        self.instance_methods.insert(key, mangled.clone());

        // Resolve show for each element type
        let mut elem_show_names = Vec::new();
        for et in elem_tys {
            let show_name = if let Some(resolved) = self.instance_methods.get(&("show".to_string(), format!("{}", et))) {
                resolved.clone()
            } else {
                // Fallback to generic show for unknown types
                "show".to_string()
            };
            elem_show_names.push(show_name);
        }

        // Build body: "(" ++ show_E1(t[1]) ++ ", " ++ show_E2(t[2]) ++ ... ++ ")"
        // We generate this as a chain of InfixApp(++, ...)
        let param_name = "_t".to_string();
        let str_ty = Ty::Con("String".to_string());

        let mut parts: Vec<TExpr> = vec![
            TExpr::new(TExprKind::Lit(TLiteral::Str("(".to_string())), str_ty.clone()),
        ];
        for (i, show_fn) in elem_show_names.iter().enumerate() {
            if i > 0 {
                parts.push(TExpr::new(TExprKind::Lit(TLiteral::Str(", ".to_string())), str_ty.clone()));
            }
            // show_Elem(t[i+1]) — represented as App(Var(show_fn), SpecCall to access field)
            let field_access = TExpr::new(
                TExprKind::SpecCall {
                    original: format!("_t_{}", i),
                    specialized: format!("__mll_tup_get:{}", i + 1),
                    args: vec![TExpr::new(TExprKind::Var(param_name.clone()), tuple_ty.clone())],
                },
                elem_tys[i].clone(),
            );
            let show_call = TExpr::new(
                TExprKind::App(
                    Box::new(TExpr::new(TExprKind::Var(show_fn.clone()), Ty::arrow(elem_tys[i].clone(), str_ty.clone()))),
                    Box::new(field_access),
                ),
                str_ty.clone(),
            );
            parts.push(show_call);
        }
        parts.push(TExpr::new(TExprKind::Lit(TLiteral::Str(")".to_string())), str_ty.clone()));

        // Chain with ++
        let body = parts.into_iter().reduce(|acc, part| {
            TExpr::new(
                TExprKind::InfixApp {
                    op: "<>".to_string(),
                    lhs: Box::new(acc),
                    rhs: Box::new(part),
                },
                str_ty.clone(),
            )
        }).unwrap();

        let func = TFunction {
            name: mangled.clone(),
            ty: Ty::arrow(tuple_ty, str_ty),
            clauses: vec![TClause {
                patterns: vec![TPattern::Var(param_name, Ty::Unit)],
                guards: vec![],
                body,
                where_binds: vec![],
            }],
            specialized: true,
            dict_params: vec![],
        };
        self.generated.push(func);
        mangled
    }

    // --- Dictionary-passing support for polymorphic recursion ---

    /// Compute a substitution mapping ALL free type vars in a function body
    /// to concrete types, using the function type signature as source of truth.
    fn compute_body_subst(poly_fn: &TFunction, concrete_ty: &Ty) -> Subst {
        // Step 1: match function type against concrete type for name-based mappings
        let mut name_map: HashMap<String, Ty> = HashMap::new();
        Self::collect_subst_by_name(&poly_fn.ty, concrete_ty, &mut name_map);

        // Step 2: collect ALL free type vars from body
        let mut all_vars: Vec<TyVar> = Vec::new();
        for clause in &poly_fn.clauses {
            Self::collect_clause_vars(clause, &mut all_vars);
        }

        // Step 3: map each body var to concrete type
        let mut map: HashMap<TyVar, Ty> = HashMap::new();
        Self::collect_subst_exact(&poly_fn.ty, concrete_ty, &mut map);
        for var in &all_vars {
            if map.contains_key(var) { continue; }
            if let Some(concrete) = name_map.get(&var.name) {
                map.insert(var.clone(), concrete.clone());
                continue;
            }
            // Single type parameter: all unmapped vars get the same concrete type
            if name_map.len() == 1 {
                map.insert(var.clone(), name_map.values().next().unwrap().clone());
            }
        }
        Subst::from_map(map)
    }

    fn collect_subst_by_name(pattern: &Ty, concrete: &Ty, map: &mut HashMap<String, Ty>) {
        match (pattern, concrete) {
            (Ty::Var(v), _) => { map.insert(v.name.clone(), concrete.clone()); }
            (Ty::Arrow(pa, pb), Ty::Arrow(ca, cb)) |
            (Ty::App(pa, pb), Ty::App(ca, cb)) => {
                Self::collect_subst_by_name(pa, ca, map);
                Self::collect_subst_by_name(pb, cb, map);
            }
            (Ty::List(pa), Ty::List(ca)) |
            (Ty::IO(pa), Ty::IO(ca)) => Self::collect_subst_by_name(pa, ca, map),
            (Ty::Tuple(ps), Ty::Tuple(cs)) if ps.len() == cs.len() => {
                for (p, c) in ps.iter().zip(cs.iter()) {
                    Self::collect_subst_by_name(p, c, map);
                }
            }
            (Ty::Forall(_, pi), _) => Self::collect_subst_by_name(pi, concrete, map),
            _ => {}
        }
    }

    fn collect_subst_exact(pattern: &Ty, concrete: &Ty, map: &mut HashMap<TyVar, Ty>) {
        match (pattern, concrete) {
            (Ty::Var(v), _) => { map.insert(v.clone(), concrete.clone()); }
            (Ty::Arrow(pa, pb), Ty::Arrow(ca, cb)) |
            (Ty::App(pa, pb), Ty::App(ca, cb)) => {
                Self::collect_subst_exact(pa, ca, map);
                Self::collect_subst_exact(pb, cb, map);
            }
            (Ty::List(pa), Ty::List(ca)) |
            (Ty::IO(pa), Ty::IO(ca)) => Self::collect_subst_exact(pa, ca, map),
            (Ty::Tuple(ps), Ty::Tuple(cs)) if ps.len() == cs.len() => {
                for (p, c) in ps.iter().zip(cs.iter()) {
                    Self::collect_subst_exact(p, c, map);
                }
            }
            (Ty::Forall(_, pi), _) => Self::collect_subst_exact(pi, concrete, map),
            _ => {}
        }
    }

    fn collect_clause_vars(clause: &TClause, vars: &mut Vec<TyVar>) {
        for p in &clause.patterns { Self::collect_pattern_vars(p, vars); }
        Self::collect_expr_vars(&clause.body, vars);
        for g in &clause.guards {
            Self::collect_expr_vars(&g.condition, vars);
            Self::collect_expr_vars(&g.body, vars);
        }
        for wb in &clause.where_binds { Self::collect_expr_vars(&wb.body, vars); }
    }

    fn collect_pattern_vars(pat: &TPattern, vars: &mut Vec<TyVar>) {
        match pat {
            TPattern::Var(_, ty) => {
                for v in ty.free_vars() { if !vars.contains(&v) { vars.push(v); } }
            }
            TPattern::Constructor { args, .. } => {
                for a in args { Self::collect_pattern_vars(a, vars); }
            }
            TPattern::Paren(p) => Self::collect_pattern_vars(p, vars),
            TPattern::Tuple(ps) => { for p in ps { Self::collect_pattern_vars(p, vars); } }
            _ => {}
        }
    }

    fn collect_expr_vars(expr: &TExpr, vars: &mut Vec<TyVar>) {
        for v in expr.ty.free_vars() { if !vars.contains(&v) { vars.push(v); } }
        match &expr.kind {
            TExprKind::App(f, a) => { Self::collect_expr_vars(f, vars); Self::collect_expr_vars(a, vars); }
            TExprKind::InfixApp { lhs, rhs, .. } => { Self::collect_expr_vars(lhs, vars); Self::collect_expr_vars(rhs, vars); }
            TExprKind::Lambda { body, .. } => Self::collect_expr_vars(body, vars),
            TExprKind::If { cond, then_branch, else_branch } => {
                Self::collect_expr_vars(cond, vars);
                Self::collect_expr_vars(then_branch, vars);
                Self::collect_expr_vars(else_branch, vars);
            }
            TExprKind::Case { scrutinee, branches } => {
                Self::collect_expr_vars(scrutinee, vars);
                for b in branches { Self::collect_expr_vars(&b.body, vars); }
            }
            TExprKind::Let { binds, body } => {
                for b in binds { Self::collect_expr_vars(&b.body, vars); }
                Self::collect_expr_vars(body, vars);
            }
            TExprKind::Negate(e) | TExprKind::Paren(e) => Self::collect_expr_vars(e, vars),
            TExprKind::Tuple(es) => { for e in es { Self::collect_expr_vars(e, vars); } }
            TExprKind::RecordUpdate { record, updates, .. } => {
                Self::collect_expr_vars(record, vars);
                for (_, _, e) in updates { Self::collect_expr_vars(e, vars); }
            }
            TExprKind::OutgoingCallback { callee, .. } => Self::collect_expr_vars(callee, vars),
            _ => {}
        }
    }

    /// Rewrite a function to use dictionary-passing.
    fn rewrite_dict_passing_fn(&self, func: &mut TFunction) {
        let constraints = match self.fn_constraints.get(&func.name) {
            Some(cs) => cs.clone(),
            None => return,
        };
        let dict_params: Vec<(String, String)> = constraints.iter().map(|c| {
            (c.class_name.clone(), format!("__dict_{}", c.class_name))
        }).collect();
        func.dict_params = dict_params.clone();

        let class_to_dict: HashMap<String, String> = dict_params.iter()
            .map(|(cls, param)| (cls.clone(), param.clone()))
            .collect();

        let func_name = func.name.clone();
        for clause in &mut func.clauses {
            clause.body = self.rewrite_dict_expr(clause.body.clone(), &func_name, &class_to_dict);
            clause.guards = clause.guards.iter().map(|g| TGuard {
                condition: self.rewrite_dict_expr(g.condition.clone(), &func_name, &class_to_dict),
                body: self.rewrite_dict_expr(g.body.clone(), &func_name, &class_to_dict),
            }).collect();
            clause.where_binds = clause.where_binds.iter().map(|wb| TLocalDef {
                name: wb.name.clone(),
                patterns: wb.patterns.clone(),
                body: self.rewrite_dict_expr(wb.body.clone(), &func_name, &class_to_dict),
            }).collect();
        }
    }

    /// Rewrite an expression for dictionary-passing.
    fn rewrite_dict_expr(&self, expr: TExpr, func_name: &str, class_to_dict: &HashMap<String, String>) -> TExpr {
        let ty = expr.ty.clone();
        let kind = match expr.kind {
            TExprKind::Var(ref name) => {
                if let Some(class_name) = self.method_to_class.get(name)
                    && let Some(dict_param) = class_to_dict.get(class_name)
                        && self.is_polymorphic(&ty) {
                            return TExpr {
                                kind: TExprKind::DictAccess {
                                    dict_param: dict_param.clone(),
                                    method_name: name.clone(),
                                },
                                ty,
                            };
                        }
                return expr;
            }
            TExprKind::App(_, _) => {
                let (head, _) = Self::collect_app_chain(&expr);
                if let TExprKind::Var(ref call_name) = head.kind
                    && call_name == func_name {
                        let (_, args) = Self::collect_app_chain(&expr);
                        let dict_args: Vec<TExpr> = class_to_dict.values().map(|dp| {
                            TExpr::new(TExprKind::Var(dp.clone()), Ty::Unit)
                        }).collect();
                        let value_args: Vec<TExpr> = args.into_iter()
                            .map(|a| self.rewrite_dict_expr(a, func_name, class_to_dict))
                            .collect();
                        return TExpr {
                            kind: TExprKind::DictCall {
                                func_name: func_name.to_string(),
                                dict_args,
                                value_args,
                            },
                            ty,
                        };
                    }
                if let TExprKind::App(func, arg) = expr.kind {
                    TExprKind::App(
                        Box::new(self.rewrite_dict_expr(*func, func_name, class_to_dict)),
                        Box::new(self.rewrite_dict_expr(*arg, func_name, class_to_dict)),
                    )
                } else { unreachable!() }
            }
            TExprKind::InfixApp { op, lhs, rhs } => {
                if let Some(class_name) = self.method_to_class.get(&op)
                    && let Some(dict_param) = class_to_dict.get(class_name)
                        && self.is_polymorphic(&lhs.ty) {
                            let dict_access = TExpr::new(
                                TExprKind::DictAccess { dict_param: dict_param.clone(), method_name: op.clone() },
                                Ty::Unit,
                            );
                            let lhs = self.rewrite_dict_expr(*lhs, func_name, class_to_dict);
                            let rhs = self.rewrite_dict_expr(*rhs, func_name, class_to_dict);
                            let app1 = TExpr::new(TExprKind::App(Box::new(dict_access), Box::new(lhs)), Ty::Unit);
                            return TExpr::new(TExprKind::App(Box::new(app1), Box::new(rhs)), ty);
                        }
                TExprKind::InfixApp {
                    op,
                    lhs: Box::new(self.rewrite_dict_expr(*lhs, func_name, class_to_dict)),
                    rhs: Box::new(self.rewrite_dict_expr(*rhs, func_name, class_to_dict)),
                }
            }
            TExprKind::Lambda { params, body } => TExprKind::Lambda {
                params, body: Box::new(self.rewrite_dict_expr(*body, func_name, class_to_dict)),
            },
            TExprKind::If { cond, then_branch, else_branch } => TExprKind::If {
                cond: Box::new(self.rewrite_dict_expr(*cond, func_name, class_to_dict)),
                then_branch: Box::new(self.rewrite_dict_expr(*then_branch, func_name, class_to_dict)),
                else_branch: Box::new(self.rewrite_dict_expr(*else_branch, func_name, class_to_dict)),
            },
            TExprKind::Case { scrutinee, branches } => TExprKind::Case {
                scrutinee: Box::new(self.rewrite_dict_expr(*scrutinee, func_name, class_to_dict)),
                branches: branches.into_iter().map(|b| TCaseBranch {
                    pattern: b.pattern,
                    guards: b.guards.into_iter().map(|g| TGuard {
                        condition: self.rewrite_dict_expr(g.condition, func_name, class_to_dict),
                        body: self.rewrite_dict_expr(g.body, func_name, class_to_dict),
                    }).collect(),
                    body: self.rewrite_dict_expr(b.body, func_name, class_to_dict),
                }).collect(),
            },
            TExprKind::Let { binds, body } => TExprKind::Let {
                binds: binds.into_iter().map(|b| TLocalDef {
                    name: b.name, patterns: b.patterns,
                    body: self.rewrite_dict_expr(b.body, func_name, class_to_dict),
                }).collect(),
                body: Box::new(self.rewrite_dict_expr(*body, func_name, class_to_dict)),
            },
            TExprKind::Negate(e) => TExprKind::Negate(Box::new(self.rewrite_dict_expr(*e, func_name, class_to_dict))),
            TExprKind::Paren(e) => TExprKind::Paren(Box::new(self.rewrite_dict_expr(*e, func_name, class_to_dict))),
            TExprKind::Tuple(es) => TExprKind::Tuple(es.into_iter().map(|e| self.rewrite_dict_expr(e, func_name, class_to_dict)).collect()),
            TExprKind::RecordUpdate { record, updates, num_fields } => TExprKind::RecordUpdate {
                record: Box::new(self.rewrite_dict_expr(*record, func_name, class_to_dict)),
                updates: updates.into_iter().map(|(n, idx, e)| (n, idx, self.rewrite_dict_expr(e, func_name, class_to_dict))).collect(),
                num_fields,
            },
            other => other,
        };
        TExpr { kind, ty }
    }

    /// Decompose nested App into (head_function, [arg1, arg2, ...])
    fn collect_app_chain(expr: &TExpr) -> (&TExpr, Vec<TExpr>) {
        let mut args = Vec::new();
        let mut e = expr;
        while let TExprKind::App(f, a) = &e.kind {
            args.push(a.as_ref().clone());
            e = f.as_ref();
        }
        args.reverse();
        (e, args)
    }

    /// Rewrite call sites to dict-passing functions.
    fn rewrite_dict_call_sites(&self, expr: TExpr) -> TExpr {
        let ty = expr.ty.clone();
        match expr.kind {
            TExprKind::App(_, _) => {
                let (head, _) = Self::collect_app_chain(&expr);
                if let TExprKind::Var(ref call_name) = head.kind
                    && self.dict_passing_fns.contains(call_name) && !self.is_polymorphic(&ty)
                        && let Some(constraints) = self.fn_constraints.get(call_name).cloned() {
                            let (_head, args) = Self::collect_app_chain(&expr);
                            let poly_fn_ty = self.poly_fns.get(call_name).map(|f| &f.ty);
                            let dict_args: Vec<TExpr> = constraints.iter().map(|c| {
                                let concrete = self.resolve_constraint_type(
                                    &c.type_var, poly_fn_ty, &args);
                                self.build_concrete_dict(&c.class_name, &concrete)
                            }).collect();
                            let value_args: Vec<TExpr> = args.into_iter()
                                .map(|a| self.rewrite_dict_call_sites(a))
                                .collect();
                            return TExpr {
                                kind: TExprKind::DictCall {
                                    func_name: call_name.clone(),
                                    dict_args,
                                    value_args,
                                },
                                ty,
                            };
                        }
                if let TExprKind::App(func, arg) = expr.kind {
                    TExpr {
                        kind: TExprKind::App(
                            Box::new(self.rewrite_dict_call_sites(*func)),
                            Box::new(self.rewrite_dict_call_sites(*arg)),
                        ),
                        ty,
                    }
                } else { unreachable!() }
            }
            TExprKind::InfixApp { op, lhs, rhs } => TExpr {
                kind: TExprKind::InfixApp {
                    op,
                    lhs: Box::new(self.rewrite_dict_call_sites(*lhs)),
                    rhs: Box::new(self.rewrite_dict_call_sites(*rhs)),
                }, ty,
            },
            TExprKind::Lambda { params, body } => TExpr {
                kind: TExprKind::Lambda { params, body: Box::new(self.rewrite_dict_call_sites(*body)) }, ty,
            },
            TExprKind::If { cond, then_branch, else_branch } => TExpr {
                kind: TExprKind::If {
                    cond: Box::new(self.rewrite_dict_call_sites(*cond)),
                    then_branch: Box::new(self.rewrite_dict_call_sites(*then_branch)),
                    else_branch: Box::new(self.rewrite_dict_call_sites(*else_branch)),
                }, ty,
            },
            TExprKind::Case { scrutinee, branches } => TExpr {
                kind: TExprKind::Case {
                    scrutinee: Box::new(self.rewrite_dict_call_sites(*scrutinee)),
                    branches: branches.into_iter().map(|b| TCaseBranch {
                        pattern: b.pattern, guards: b.guards,
                        body: self.rewrite_dict_call_sites(b.body),
                    }).collect(),
                }, ty,
            },
            TExprKind::Let { binds, body } => TExpr {
                kind: TExprKind::Let {
                    binds: binds.into_iter().map(|b| TLocalDef {
                        name: b.name, patterns: b.patterns,
                        body: self.rewrite_dict_call_sites(b.body),
                    }).collect(),
                    body: Box::new(self.rewrite_dict_call_sites(*body)),
                }, ty,
            },
            TExprKind::Negate(e) => TExpr {
                kind: TExprKind::Negate(Box::new(self.rewrite_dict_call_sites(*e))), ty,
            },
            TExprKind::Paren(e) => TExpr {
                kind: TExprKind::Paren(Box::new(self.rewrite_dict_call_sites(*e))), ty,
            },
            _ => expr,
        }
    }

    fn resolve_constraint_type(&self, type_var: &str, poly_fn_ty: Option<&Ty>, args: &[TExpr]) -> Ty {
        if let Some(fn_ty) = poly_fn_ty {
            let mut subst = HashMap::new();
            Self::match_fn_args(fn_ty, args, &mut subst);
            if let Some(ty) = subst.get(type_var) { return ty.clone(); }
        }
        if !args.is_empty()
            && let Some(inner) = Self::extract_inner_type(&args[0].ty) { return inner; }
        Ty::Con("_".into())
    }

    fn match_fn_args(fn_ty: &Ty, args: &[TExpr], subst: &mut HashMap<String, Ty>) {
        let mut param_ty = fn_ty;
        for arg in args {
            if let Ty::Arrow(from, to) = param_ty {
                Self::collect_subst_by_name(from, &arg.ty, subst);
                param_ty = to;
            }
        }
    }

    fn extract_inner_type(ty: &Ty) -> Option<Ty> {
        match ty {
            Ty::App(_, inner) => Some(*inner.clone()),
            Ty::List(inner) => Some(*inner.clone()),
            _ => None,
        }
    }

    fn build_concrete_dict(&self, class_name: &str, concrete_ty: &Ty) -> TExpr {
        let class_info = match self.classes.get(class_name) {
            Some(ci) => ci,
            None => return TExpr::new(TExprKind::Lit(TLiteral::Unit), Ty::Unit),
        };
        let ty_str = format!("{}", concrete_ty);
        let mut method_impls = Vec::new();
        for (method_name, _) in &class_info.methods {
            let key = (method_name.clone(), ty_str.clone());
            let impl_name = self.instance_methods.get(&key)
                .cloned()
                .or_else(|| self.resolve_parameterized_instance(method_name, concrete_ty))
                .unwrap_or_else(|| method_name.clone());
            method_impls.push(format!("{}={}", method_name, impl_name));
        }
        let spec = format!("__mll_dict:{}:{}", class_name, method_impls.join(","));
        TExpr::new(
            TExprKind::SpecCall {
                original: format!("__dict_{}", class_name),
                specialized: spec,
                args: vec![],
            },
            Ty::Unit,
        )
    }
}
