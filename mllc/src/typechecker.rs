use std::collections::{HashMap, HashSet};
use crate::ast::*;
use crate::tir::*;
use crate::types::*;

/// Type environment: maps names to type schemes
#[derive(Debug, Clone)]
pub struct TypeEnv {
    bindings: HashMap<String, Scheme>,
}

impl Default for TypeEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeEnv {
    pub fn new() -> Self {
        TypeEnv { bindings: HashMap::new() }
    }

    pub fn insert(&mut self, name: String, scheme: Scheme) {
        self.bindings.insert(name, scheme);
    }

    pub fn size(&self) -> usize { self.bindings.len() }

    pub fn lookup(&self, name: &str) -> Option<&Scheme> {
        self.bindings.get(name)
    }

    pub fn apply_subst(&self, subst: &Subst) -> TypeEnv {
        TypeEnv {
            bindings: self.bindings.iter()
                .map(|(k, v)| (k.clone(), v.apply_subst(subst)))
                .collect(),
        }
    }

    pub fn free_vars(&self) -> Vec<TyVar> {
        let mut vars = Vec::new();
        for scheme in self.bindings.values() {
            for v in scheme.free_vars() {
                if !vars.contains(&v) {
                    vars.push(v);
                }
            }
        }
        vars
    }
}

/// Constructor info
#[derive(Debug, Clone)]
pub struct ConInfo {
    pub type_name: String,
    pub variant_index: usize,
    pub total_variants: usize,
    pub field_types: Vec<Ty>,
    pub type_vars: Vec<TyVar>,
    pub result_type: Ty,
    /// Existential type variables (quantified per-constructor, not in the data type params)
    pub existential_vars: Vec<TyVar>,
}

/// Typeclass info
#[derive(Debug, Clone)]
pub struct ClassInfo {
    pub name: String,
    pub type_var: String,
    /// Superclass names (e.g., Eq for class Eq a => Ord a)
    pub superclasses: Vec<String>,
    /// Method name -> method type (with type_var as placeholder)
    pub methods: Vec<(String, Ty)>,
    /// Default method implementations (AST clauses, keyed by method name)
    pub default_methods: HashMap<String, Vec<Clause>>,
}

/// Instance info
#[derive(Debug, Clone)]
pub struct InstanceInfo {
    pub class_name: String,
    pub target_type: Ty,
    /// Method name -> mangled function name
    pub method_fns: HashMap<String, String>,
}

/// The type checker — validates types and produces typed IR
pub struct Checker {
    env: TypeEnv,
    next_var: u32,
    constructors: HashMap<String, ConInfo>,
    pub errors: Vec<TypeError>,
    current_fn: Option<String>,
    /// Registered typeclasses
    classes: HashMap<String, ClassInfo>,
    /// Registered instances: (class_name, type_string) -> InstanceInfo
    instances: HashMap<(String, String), InstanceInfo>,
    /// Record field accessors: field_name -> (type_name, lua_index)
    pub record_fields: HashMap<String, (String, usize)>,
    /// User-defined type families: name -> equations
    type_families: HashMap<String, Vec<TypeFamilyEq>>,
    /// Type aliases: name -> (params, expanded type)
    type_aliases: HashMap<String, (Vec<String>, Type)>,
    /// Kind table: type constructor name -> kind
    kinds: HashMap<String, Kind>,
    /// Names hidden by module export control (imported but not exported).
    /// Only enforced when `enforce_hidden` is true (local code, not imported code).
    hidden_names: HashSet<String>,
    enforce_hidden: bool,
    /// Index where local declarations start (for hidden name enforcement)
    local_decl_start: usize,
    /// Classes defined in the local module (for orphan detection)
    local_classes: HashSet<String>,
    /// Types defined in the local module (for orphan detection)
    local_types: HashSet<String>,
    /// Whether orphan instance checking is active
    orphan_check_enabled: bool,
    /// Typeclass constraints per function name (for dictionary-passing fallback)
    fn_constraints: HashMap<String, Vec<TyConstraint>>,
    /// Class constraints carried by a class method, keyed by method name
    /// (e.g. "show" -> [Show a]). Instantiating the method emits a wanted
    /// constraint on the freshened type so it can be discharged.
    method_constraints: HashMap<String, Vec<TyConstraint>>,
    /// A function's signature constraints, expressed over the *freshened*
    /// variable names its caller-visible scheme uses (so a call site can map
    /// each constraint to the freshly-instantiated type). E.g. with `needsShow
    /// :: Show a => …` freshened to `a519`, this holds [Show a519].
    fn_use_constraints: HashMap<String, Vec<TyConstraint>>,
    /// Wanted class constraints collected while checking the current function:
    /// (class name, the instantiated type the constraint applies to). Discharged
    /// at the function boundary once unification has resolved the type.
    wanted: Vec<(String, Ty)>,
}

impl Default for Checker {
    fn default() -> Self {
        Self::new()
    }
}

impl Checker {
    pub fn new() -> Self {
        let mut checker = Checker {
            env: TypeEnv::new(),
            next_var: 0,
            constructors: HashMap::new(),
            errors: Vec::new(),
            current_fn: None,
            classes: HashMap::new(),
            instances: HashMap::new(),
            record_fields: HashMap::new(),
            type_families: HashMap::new(),
            type_aliases: HashMap::new(),
            kinds: HashMap::new(),
            hidden_names: HashSet::new(),
            enforce_hidden: false,
            local_decl_start: 0,
            local_classes: HashSet::new(),
            local_types: HashSet::new(),
            orphan_check_enabled: false,
            fn_constraints: HashMap::new(),
            method_constraints: HashMap::new(),
            fn_use_constraints: HashMap::new(),
            wanted: Vec::new(),
        };
        checker.init_prelude();
        checker.init_kinds();
        checker
    }

    fn fresh_var(&mut self, prefix: &str) -> Ty {
        let id = self.next_var;
        self.next_var += 1;
        Ty::Var(TyVar { name: format!("{}{}", prefix, id), id })
    }

    fn fresh_tyvar(&mut self, prefix: &str) -> TyVar {
        let id = self.next_var;
        self.next_var += 1;
        TyVar { name: format!("{}{}", prefix, id), id }
    }

    fn instantiate(&mut self, scheme: &Scheme) -> Ty {
        self.instantiate_with_map(scheme).0
    }

    /// Instantiate a scheme, also returning the var→fresh-type map so callers
    /// can relate a class constraint's bound variable to its fresh type.
    fn instantiate_with_map(&mut self, scheme: &Scheme) -> (Ty, HashMap<TyVar, Ty>) {
        let mut map = HashMap::new();
        for v in &scheme.vars {
            if let Ty::Var(fresh) = self.fresh_var("_i") {
                map.insert(v.clone(), Ty::Var(fresh));
            }
        }
        (scheme.ty.apply_subst(&Subst::from_map(map.clone())), map)
    }

    /// Does `class` have an instance for `ty`? Conservative: a type variable or
    /// rigid skolem is treated as satisfiable (deferred to the caller), and a
    /// container (list/tuple/applied type) is satisfiable when its components
    /// are. Only the cases that genuinely never have an instance — functions,
    /// IO/ST actions, and a concrete type constructor with no registered
    /// instance — are rejected.
    fn has_instance(&self, class: &str, ty: &Ty) -> bool {
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
            Ty::Con(name) => self.instances.contains_key(&(class.to_string(), name.clone())),
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
                    Ty::Con(base) =>
                        self.instances.contains_key(&(class.to_string(), base.clone()))
                            && args.iter().all(|a| self.has_instance(class, a)),
                    _ => true, // unknown head — defer rather than over-reject
                }
            }
        }
    }

    /// Emit the wanted class constraints for a use of `name`, mapping each
    /// constrained variable to its freshly-instantiated type. Covers both
    /// built-in class methods (`show`/`==`/…) and any user/prelude function
    /// whose signature carries constraints (e.g. `print :: Show a => …`), so a
    /// constraint is checked wherever the function is called.
    fn emit_use_constraints(&mut self, name: &str, inst_map: &HashMap<TyVar, Ty>) {
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

    fn generalize(&self, env: &TypeEnv, ty: &Ty) -> Scheme {
        let env_vars = env.free_vars();
        let vars: Vec<TyVar> = ty.free_vars().into_iter()
            .filter(|v| !env_vars.contains(v))
            .collect();
        Scheme { vars, ty: ty.clone() }
    }

    fn ast_type_to_ty(&mut self, ast_ty: &Type) -> Ty {
        match ast_ty {
            Type::Con(name) => {
                // Check for type alias expansion
                if let Some((params, alias_ty)) = self.type_aliases.get(name).cloned()
                    && params.is_empty() {
                        if name == "Int" {
                            eprintln!("Warning: Int is treated as Integer (Lua has no fixed-width integers)");
                        }
                        return self.ast_type_to_ty(&alias_ty);
                    }
                    // Parameterized alias used without args — treat as constructor
                Ty::Con(name.clone())
            }
            Type::Var(name) => Ty::Var(TyVar { name: name.clone(), id: u32::MAX }),
            Type::Arrow(a, b) => Ty::arrow(self.ast_type_to_ty(a), self.ast_type_to_ty(b)),
            Type::App(f, a) => {
                // Check for type family reduction: FamilyName arg1 arg2 ...
                if let Some(result) = self.try_reduce_type_family(ast_ty) {
                    return result;
                }
                // Check for type alias expansion: AliasName arg1 arg2 ...
                if let Some(result) = self.try_expand_type_alias(ast_ty) {
                    return result;
                }
                Ty::app(self.ast_type_to_ty(f), self.ast_type_to_ty(a))
            }
            Type::List(a) => Ty::list(self.ast_type_to_ty(a)),
            Type::IO(a) => Ty::io(self.ast_type_to_ty(a)),
            Type::ScopedLuaIO { scope_var, inner } => {
                let sv = TyVar { name: scope_var.clone(), id: u32::MAX };
                Ty::lua_io(sv, self.ast_type_to_ty(inner))
            }
            Type::Forall { var, inner } => {
                let tv = TyVar { name: var.clone(), id: u32::MAX };
                Ty::Forall(tv, Box::new(self.ast_type_to_ty(inner)))
            }
            Type::Unit => Ty::Unit,
            Type::Paren(inner) => self.ast_type_to_ty(inner),
            Type::Constrained { ty, .. } => self.ast_type_to_ty(ty),
            // LuaPure "name" T  reduces to  T
            Type::LuaPure { result, .. } => self.ast_type_to_ty(result),
            // LuaIO "name" T  reduces to  IO T
            Type::LuaIO { result, .. } => Ty::io(self.ast_type_to_ty(result)),
            // LuaIterator "name" T  reduces to  [T]
            Type::LuaIterator { result, .. } => Ty::list(self.ast_type_to_ty(result)),
            Type::Tuple(elems) => Ty::Tuple(elems.iter().map(|t| self.ast_type_to_ty(t)).collect()),
            // LuaTry "name" T  reduces to  IO (Either String T)
            Type::LuaTry { result, .. } => {
                let inner = self.ast_type_to_ty(result);
                Ty::io(Ty::app(Ty::app(Ty::Con("Either".into()), Ty::Con("String".into())), inner))
            }
            Type::Promoted(name) => Ty::Promoted(name.clone()),
        }
    }

    /// Try to reduce a type family application.
    /// Collects the head and arguments from nested App nodes,
    /// then tries to match against type family equations.
    fn try_reduce_type_family(&mut self, ty: &Type) -> Option<Ty> {
        // Collect the head and args from nested App: F a b -> (F, [a, b])
        let mut args = Vec::new();
        let mut head = ty;
        loop {
            match head {
                Type::App(f, a) => {
                    args.push(a.as_ref());
                    head = f.as_ref();
                }
                _ => break,
            }
        }
        args.reverse();

        let family_name = match head {
            Type::Con(name) => name.clone(),
            _ => return None,
        };

        let equations = self.type_families.get(&family_name)?.clone();

        // Try each equation
        for eq in &equations {
            if eq.args.len() != args.len() {
                continue;
            }
            // Try to match each arg pattern against the actual arg
            let mut bindings: HashMap<String, &Type> = HashMap::new();
            let mut matched = true;
            for (pattern, actual) in eq.args.iter().zip(args.iter()) {
                if !self.match_type_pattern(pattern, actual, &mut bindings) {
                    matched = false;
                    break;
                }
            }
            if matched {
                // Apply bindings to the result type
                let result = self.substitute_type(&eq.result, &bindings);
                return Some(self.ast_type_to_ty(&result));
            }
        }

        None
    }

    /// Expand a type alias application: `AliasName arg1 arg2` → substituted body.
    fn try_expand_type_alias(&mut self, ty: &Type) -> Option<Ty> {
        let mut args = Vec::new();
        let mut head = ty;
        loop {
            match head {
                Type::App(f, a) => { args.push(a.as_ref()); head = f.as_ref(); }
                _ => break,
            }
        }
        args.reverse();
        let alias_name = match head {
            Type::Con(name) => name.clone(),
            _ => return None,
        };
        let (params, alias_body) = self.type_aliases.get(&alias_name)?.clone();
        if params.len() != args.len() { return None; }
        let mut bindings: HashMap<String, &Type> = HashMap::new();
        for (param, arg) in params.iter().zip(args.iter()) {
            bindings.insert(param.clone(), arg);
        }
        let expanded = self.substitute_type(&alias_body, &bindings);
        Some(self.ast_type_to_ty(&expanded))
    }

    /// Match a type pattern against an actual type, collecting variable bindings.
    fn match_type_pattern<'a>(&self, pattern: &Type, actual: &'a Type, bindings: &mut HashMap<String, &'a Type>) -> bool {
        match pattern {
            Type::Var(name) => {
                if let Some(existing) = bindings.get(name) {
                    // Variable already bound — check consistency
                    format!("{:?}", existing) == format!("{:?}", actual)
                } else {
                    bindings.insert(name.clone(), actual);
                    true
                }
            }
            Type::Con(name) => matches!(actual, Type::Con(n) if n == name),
            Type::List(inner_pat) => {
                if let Type::List(inner_act) = actual {
                    self.match_type_pattern(inner_pat, inner_act, bindings)
                } else {
                    false
                }
            }
            Type::App(f_pat, a_pat) => {
                if let Type::App(f_act, a_act) = actual {
                    self.match_type_pattern(f_pat, f_act, bindings)
                        && self.match_type_pattern(a_pat, a_act, bindings)
                } else {
                    false
                }
            }
            Type::Paren(inner) => self.match_type_pattern(inner, actual, bindings),
            // Wildcards or underscore vars
            _ => false,
        }
    }

    /// Substitute type variables in a type with bound values.
    fn substitute_type(&self, ty: &Type, bindings: &HashMap<String, &Type>) -> Type {
        match ty {
            Type::Var(name) => {
                if let Some(bound) = bindings.get(name) {
                    (*bound).clone()
                } else {
                    ty.clone()
                }
            }
            Type::Con(_) => ty.clone(),
            Type::App(f, a) => Type::App(
                Box::new(self.substitute_type(f, bindings)),
                Box::new(self.substitute_type(a, bindings)),
            ),
            Type::Arrow(a, b) => Type::Arrow(
                Box::new(self.substitute_type(a, bindings)),
                Box::new(self.substitute_type(b, bindings)),
            ),
            Type::List(a) => Type::List(Box::new(self.substitute_type(a, bindings))),
            Type::IO(a) => Type::IO(Box::new(self.substitute_type(a, bindings))),
            Type::Paren(inner) => self.substitute_type(inner, bindings),
            _ => ty.clone(),
        }
    }

    /// Convert a forall type to a polymorphic scheme for rank-2 parameter binding.
    /// `forall a b. T` becomes `Scheme { vars: [a, b], ty: T }`.
    /// Non-forall types become monomorphic schemes.
    fn forall_to_scheme(ty: &Ty) -> Scheme {
        let mut vars = vec![];
        let mut current = ty;
        while let Ty::Forall(v, inner) = current {
            vars.push(v.clone());
            current = inner;
        }
        if vars.is_empty() {
            Scheme::mono(ty.clone())
        } else {
            Scheme { vars, ty: current.clone() }
        }
    }

    fn freshen_sig_type(&mut self, ty: &Ty) -> Ty {
        self.freshen_sig_type_mapped(ty).0
    }

    /// Like `freshen_sig_type` but also returns the renaming of signature
    /// variables (original name → fresh name), so a function's class
    /// constraints can be re-expressed over the freshened variables its
    /// caller-visible scheme actually uses.
    fn freshen_sig_type_mapped(&mut self, ty: &Ty) -> (Ty, HashMap<String, String>) {
        // Strip forall and bind scope variables as rigid
        let inner = match ty {
            Ty::Forall(v, inner) => {
                let fresh = self.fresh_tyvar(&v.name);
                let subst = Subst::singleton(v.clone(), Ty::Var(fresh));
                return self.freshen_sig_type_mapped(&inner.apply_subst(&subst));
            }
            other => other,
        };
        let vars = inner.free_vars();
        let mut map = HashMap::new();
        let mut renames = HashMap::new();
        for v in &vars {
            if v.id == u32::MAX {
                let fresh = self.fresh_tyvar(&v.name);
                renames.insert(v.name.clone(), fresh.name.clone());
                map.insert(v.clone(), Ty::Var(fresh));
            }
        }
        (inner.apply_subst(&Subst::from_map(map)), renames)
    }

    fn push_error_ctx(&mut self, kind: TypeErrorKind, ctx: String) {
        self.errors.push(TypeError { kind, context: Some(ctx), span: None });
    }

    fn push_error_span(&mut self, kind: TypeErrorKind, ctx: String, span: Span) {
        self.errors.push(TypeError { kind, context: Some(ctx), span: Some(span) });
    }

    fn literal_type(&self, lit: &Literal) -> Ty {
        match lit {
            Literal::Integer(_) => Ty::Con("Integer".into()),
            Literal::Number(_) => Ty::Con("Number".into()),
            Literal::Str(_) => Ty::Con("String".into()),
            Literal::Bool(_) => Ty::Con("Bool".into()),
            Literal::Unit => Ty::Unit,
        }
    }

    fn convert_literal(lit: &Literal) -> TLiteral {
        match lit {
            Literal::Integer(n) => TLiteral::Integer(*n),
            Literal::Number(n) => TLiteral::Number(*n),
            Literal::Str(s) => TLiteral::Str(s.clone()),
            Literal::Bool(b) => TLiteral::Bool(*b),
            Literal::Unit => TLiteral::Unit,
        }
    }

    // --- Prelude ---

    fn init_prelude(&mut self) {
        let a = TyVar { name: "a".into(), id: u32::MAX };
        let b = TyVar { name: "b".into(), id: u32::MAX };
        let c = TyVar { name: "c".into(), id: u32::MAX };
        let f = TyVar { name: "f".into(), id: u32::MAX };
        let m = TyVar { name: "m".into(), id: u32::MAX };
        let ta = Ty::Var(a.clone());
        let tb = Ty::Var(b.clone());
        let tc = Ty::Var(c.clone());
        let tf = Ty::Var(f.clone());
        let tm = Ty::Var(m.clone());

        // Only register types for builtins that are NOT provided by Prelude.mll
        // Prelude.mll provides: putStrLn, sqrt, id, const, flip,
        //   head, tail, map, filter, foldl, foldr, take, zipWith, length, reverse
        let entries: Vec<(&str, Vec<TyVar>, Ty)> = vec![
            ("print", vec![], Ty::arrow(Ty::Con("String".into()), Ty::io(Ty::Unit))),
            ("++", vec![a.clone()], Ty::fun(&[Ty::list(ta.clone()), Ty::list(ta.clone())], Ty::list(ta.clone()))),
            ("!!", vec![a.clone()], Ty::fun(&[Ty::list(ta.clone()), Ty::Con("Integer".into())], ta.clone())),
            ("$", vec![a.clone(), b.clone()], Ty::fun(&[Ty::arrow(ta.clone(), tb.clone()), ta.clone()], tb.clone())),
            (".", vec![a.clone(), b.clone(), c.clone()], Ty::fun(&[Ty::arrow(tb.clone(), tc.clone()), Ty::arrow(ta.clone(), tb.clone()), ta.clone()], tc.clone())),
            ("not", vec![], Ty::arrow(Ty::Con("Bool".into()), Ty::Con("Bool".into()))),
            ("error", vec![a.clone()], Ty::arrow(Ty::Con("String".into()), ta.clone())),
            ("undefined", vec![a.clone()], ta.clone()),
            ("otherwise", vec![], Ty::Con("Bool".into())),
            ("seq", vec![a.clone(), b.clone()], Ty::fun(&[ta.clone(), tb.clone()], tb.clone())),
            // pure/return, >>=, >> are now typeclass methods (Applicative/Monad)
            // but keep env entries so type inference sees them as polymorphic
            ("getArgs", vec![], Ty::io(Ty::list(Ty::Con("String".into())))),
            ("exit", vec![], Ty::arrow(Ty::Con("ExitValue".into()), Ty::io(Ty::Unit))),
            // Exception handling: catch Lua-level IO errors
            ("try", vec![a.clone()], Ty::arrow(
                Ty::io(ta.clone()),
                Ty::io(Ty::app(Ty::app(Ty::Con("Either".into()), Ty::Con("String".into())), ta.clone())),
            )),
            ("catch", vec![a.clone()], Ty::fun(&[
                Ty::io(ta.clone()),
                Ty::arrow(Ty::Con("String".into()), Ty::io(ta.clone())),
            ], Ty::io(ta.clone()))),
        ];
        for (name, vars, ty) in entries {
            self.env.insert(name.into(), Scheme { vars, ty });
        }
        // HashMap operations (backed by Lua tables)
        let hm = |k: Ty, v: Ty| Ty::app(Ty::app(Ty::Con("HashMap".into()), k), v);
        let hm_kv = hm(ta.clone(), tb.clone());
        let hm_entries: Vec<(&str, Vec<TyVar>, Ty)> = vec![
            ("hmEmpty", vec![a.clone(), b.clone()], hm_kv.clone()),
            ("hmInsert", vec![a.clone(), b.clone()], Ty::fun(&[ta.clone(), tb.clone(), hm_kv.clone()], hm_kv.clone())),
            ("hmLookup", vec![a.clone(), b.clone()], Ty::fun(&[ta.clone(), hm_kv.clone()], Ty::app(Ty::Con("Maybe".into()), tb.clone()))),
            ("hmDelete", vec![a.clone(), b.clone()], Ty::fun(&[ta.clone(), hm_kv.clone()], hm_kv.clone())),
            ("hmSize", vec![a.clone(), b.clone()], Ty::arrow(hm_kv.clone(), Ty::Con("Integer".into()))),
            ("hmKeys", vec![a.clone(), b.clone()], Ty::arrow(hm_kv.clone(), Ty::list(ta.clone()))),
            ("hmValues", vec![a.clone(), b.clone()], Ty::arrow(hm_kv.clone(), Ty::list(tb.clone()))),
            ("hmMember", vec![a.clone(), b.clone()], Ty::fun(&[ta.clone(), hm_kv.clone()], Ty::Con("Bool".into()))),
            ("hmFromList", vec![a.clone(), b.clone()], Ty::arrow(Ty::list(Ty::Tuple(vec![ta.clone(), tb.clone()])), hm_kv.clone())),
        ];
        for (name, vars, ty) in hm_entries {
            self.env.insert(name.into(), Scheme { vars, ty });
        }

        // ByteString operations (backed by Lua strings as byte arrays)
        let bs = Ty::Con("ByteString".into());
        let int = Ty::Con("Integer".into());
        let bool_ = Ty::Con("Bool".into());
        let bs_entries: Vec<(&str, Vec<TyVar>, Ty)> = vec![
            ("bsEmpty",     vec![], bs.clone()),
            ("bsLength",    vec![], Ty::arrow(bs.clone(), int.clone())),
            ("bsIndex",     vec![], Ty::fun(&[bs.clone(), int.clone()], int.clone())),
            ("bsSub",       vec![], Ty::fun(&[bs.clone(), int.clone(), int.clone()], bs.clone())),
            ("bsSingleton", vec![], Ty::arrow(int.clone(), bs.clone())),
            ("bsConcat",    vec![], Ty::fun(&[bs.clone(), bs.clone()], bs.clone())),
            ("bsConcatList", vec![], Ty::arrow(Ty::list(bs.clone()), bs.clone())),
            ("bsNull",      vec![], Ty::arrow(bs.clone(), bool_.clone())),
            ("bsHead",      vec![], Ty::arrow(bs.clone(), int.clone())),
            ("bsTail",      vec![], Ty::arrow(bs.clone(), bs.clone())),
            ("bsCons",      vec![], Ty::fun(&[int.clone(), bs.clone()], bs.clone())),
            ("bsSnoc",      vec![], Ty::fun(&[bs.clone(), int.clone()], bs.clone())),
            ("bsReplicate", vec![], Ty::fun(&[int.clone(), int.clone()], bs.clone())),
            ("bsPack",      vec![], Ty::arrow(Ty::list(int.clone()), bs.clone())),
            ("bsUnpack",    vec![], Ty::arrow(bs.clone(), Ty::list(int.clone()))),
            ("bsMap",       vec![], Ty::fun(&[Ty::arrow(int.clone(), int.clone()), bs.clone()], bs.clone())),
            ("bsFoldl",     vec![a.clone()], Ty::fun(&[Ty::fun(&[ta.clone(), int.clone()], ta.clone()), ta.clone(), bs.clone()], ta.clone())),
            ("bsXor",       vec![], Ty::fun(&[bs.clone(), bs.clone()], bs.clone())),
            ("bsZipWith",   vec![], Ty::fun(&[Ty::fun(&[int.clone(), int.clone()], int.clone()), bs.clone(), bs.clone()], bs.clone())),
            ("bsToString",  vec![], Ty::arrow(bs.clone(), Ty::Con("String".into()))),
            ("bsFromString", vec![], Ty::arrow(Ty::Con("String".into()), bs.clone())),
            ("bsGetU16LE",  vec![], Ty::fun(&[bs.clone(), int.clone()], int.clone())),
            ("bsGetU32LE",  vec![], Ty::fun(&[bs.clone(), int.clone()], int.clone())),
            ("bsGetI8",     vec![], Ty::fun(&[bs.clone(), int.clone()], int.clone())),
            ("bsGetI16LE",  vec![], Ty::fun(&[bs.clone(), int.clone()], int.clone())),
            ("bsPutI16LE",  vec![], Ty::arrow(int.clone(), bs.clone())),
        ];
        for (name, vars, ty) in bs_entries {
            self.env.insert(name.into(), Scheme { vars, ty });
        }

        for name in &["max", "min"] {
            self.env.insert(name.to_string(), Scheme { vars: vec![a.clone()], ty: Ty::fun(&[ta.clone(), ta.clone()], ta.clone()) });
        }
        for op in &["+", "-", "*", "/"] {
            self.env.insert(op.to_string(), Scheme { vars: vec![a.clone()], ty: Ty::fun(&[ta.clone(), ta.clone()], ta.clone()) });
        }
        // Comparison operators will be registered as Ord methods below
        for op in &["&&", "||"] {
            self.env.insert(op.to_string(), Scheme { vars: vec![], ty: Ty::fun(&[Ty::Con("Bool".into()), Ty::Con("Bool".into())], Ty::Con("Bool".into())) });
        }
        for name in &["mod", "div"] {
            self.env.insert(name.to_string(), Scheme { vars: vec![], ty: Ty::fun(&[Ty::Con("Integer".into()), Ty::Con("Integer".into())], Ty::Con("Integer".into())) });
        }
        // List functions that need lazy cons (implemented in Lua runtime)
        self.env.insert("head".into(), Scheme { vars: vec![a.clone()], ty: Ty::arrow(Ty::list(ta.clone()), ta.clone()) });
        self.env.insert("tail".into(), Scheme { vars: vec![a.clone()], ty: Ty::arrow(Ty::list(ta.clone()), Ty::list(ta.clone())) });
        self.env.insert("map".into(), Scheme { vars: vec![a.clone(), b.clone()], ty: Ty::fun(&[Ty::arrow(ta.clone(), tb.clone()), Ty::list(ta.clone())], Ty::list(tb.clone())) });
        self.env.insert("filter".into(), Scheme { vars: vec![a.clone(), b.clone()], ty: Ty::fun(&[Ty::arrow(ta.clone(), Ty::Con("Bool".into())), Ty::list(ta.clone())], Ty::list(ta.clone())) });
        self.env.insert("take".into(), Scheme { vars: vec![a.clone()], ty: Ty::fun(&[Ty::Con("Integer".into()), Ty::list(ta.clone())], Ty::list(ta.clone())) });
        self.env.insert("drop".into(), Scheme { vars: vec![a.clone()], ty: Ty::fun(&[Ty::Con("Integer".into()), Ty::list(ta.clone())], Ty::list(ta.clone())) });
        self.env.insert("zipWith".into(), Scheme { vars: vec![a.clone(), b.clone(), c.clone()], ty: Ty::fun(&[Ty::fun(&[ta.clone(), tb.clone()], tc.clone()), Ty::list(ta.clone()), Ty::list(tb.clone())], Ty::list(tc.clone())) });

        // Maybe
        self.constructors.insert("Just".into(), ConInfo { type_name: "Maybe".into(), variant_index: 1, total_variants: 2, field_types: vec![ta.clone()], type_vars: vec![a.clone()], result_type: Ty::app(Ty::Con("Maybe".into()), ta.clone()), existential_vars: vec![] });
        self.constructors.insert("Nothing".into(), ConInfo { type_name: "Maybe".into(), variant_index: 2, total_variants: 2, field_types: vec![], type_vars: vec![a.clone()], result_type: Ty::app(Ty::Con("Maybe".into()), ta.clone()), existential_vars: vec![] });
        self.env.insert("Just".into(), Scheme { vars: vec![a.clone()], ty: Ty::arrow(ta.clone(), Ty::app(Ty::Con("Maybe".into()), ta.clone())) });
        self.env.insert("Nothing".into(), Scheme { vars: vec![a.clone()], ty: Ty::app(Ty::Con("Maybe".into()), ta.clone()) });
        self.env.insert("True".into(), Scheme::mono(Ty::Con("Bool".into())));
        self.env.insert("False".into(), Scheme::mono(Ty::Con("Bool".into())));

        // List constructors
        self.constructors.insert(":".into(), ConInfo {
            type_name: "[]".into(), variant_index: 1, total_variants: 2,
            field_types: vec![ta.clone(), Ty::list(ta.clone())],
            type_vars: vec![a.clone()],
            result_type: Ty::list(ta.clone()),
            existential_vars: vec![],
        });
        self.constructors.insert("[]".into(), ConInfo {
            type_name: "[]".into(), variant_index: 2, total_variants: 2,
            field_types: vec![],
            type_vars: vec![a.clone()],
            result_type: Ty::list(ta.clone()),
            existential_vars: vec![],
        });
        // (:) :: a -> [a] -> [a]
        self.env.insert(":".into(), Scheme {
            vars: vec![a.clone()],
            ty: Ty::fun(&[ta.clone(), Ty::list(ta.clone())], Ty::list(ta.clone())),
        });
        // [] :: [a]
        self.env.insert("[]".into(), Scheme {
            vars: vec![a.clone()],
            ty: Ty::list(ta.clone()),
        });

        // head, tail, take, zipWith, length, reverse are now in Prelude.mll

        // LuaFunction and engage
        let s = TyVar { name: "s".into(), id: u32::MAX };
        let ts = Ty::Var(s.clone());

        // LuaFunction is just an opaque Con type — the scope var is
        // attached when it appears in a type signature as LuaFunction s
        // (handled by ast_type_to_ty via type application)

        // liftIO :: IO a -> LuaIO s a
        self.env.insert("liftIO".into(), Scheme {
            vars: vec![a.clone(), s.clone()],
            ty: Ty::arrow(Ty::io(ta.clone()), Ty::lua_io(s.clone(), ta.clone())),
        });

        // engage :: LuaFunction s -> a
        // (the type annotation at the call site determines a)
        // At runtime, engage is the identity — the LuaFunction is
        // already a Lua function, engage just satisfies the type system.
        self.env.insert("engage".into(), Scheme {
            vars: vec![a.clone(), s.clone()],
            ty: Ty::arrow(
                Ty::app(Ty::Con("LuaFunction".into()), Ty::Var(s.clone())),
                ta.clone(),
            ),
        });

        // ST s a — pure mutable state monad (same runtime as IO, type-level distinction only)
        // STArray s — mutable integer array, scoped to ST s
        let st_s = |inner: Ty| Ty::app(Ty::app(Ty::Con("ST".into()), ts.clone()), inner);
        let sta_s = Ty::app(Ty::Con("STArray".into()), ts.clone());

        // runST :: (forall s. ST s a) -> a
        // Rank-2: the s is universally quantified in the argument
        self.env.insert("runST".into(), Scheme {
            vars: vec![a.clone()],
            ty: Ty::arrow(
                Ty::Forall(s.clone(), Box::new(st_s(ta.clone()))),
                ta.clone(),
            ),
        });
        // newSTArray :: Integer -> Integer -> ST s (STArray s)
        self.env.insert("newSTArray".into(), Scheme {
            vars: vec![s.clone()],
            ty: Ty::fun(&[int.clone(), int.clone()], st_s(sta_s.clone())),
        });
        // readSTArray :: STArray s -> Integer -> ST s Integer
        self.env.insert("readSTArray".into(), Scheme {
            vars: vec![s.clone()],
            ty: Ty::fun(&[sta_s.clone(), int.clone()], st_s(int.clone())),
        });
        // writeSTArray :: STArray s -> Integer -> Integer -> ST s ()
        self.env.insert("writeSTArray".into(), Scheme {
            vars: vec![s.clone()],
            ty: Ty::fun(&[sta_s.clone(), int.clone(), int.clone()], st_s(Ty::Unit)),
        });
        // modifySTArray :: STArray s -> Integer -> (Integer -> Integer) -> ST s ()
        self.env.insert("modifySTArray".into(), Scheme {
            vars: vec![s.clone()],
            ty: Ty::fun(&[sta_s.clone(), int.clone(), Ty::arrow(int.clone(), int.clone())], st_s(Ty::Unit)),
        });
        // stArrayLength :: STArray s -> ST s Integer
        self.env.insert("stArrayLength".into(), Scheme {
            vars: vec![s.clone()],
            ty: Ty::arrow(sta_s.clone(), st_s(int.clone())),
        });
        // newSTArrayFromList :: [Integer] -> ST s (STArray s)
        self.env.insert("newSTArrayFromList".into(), Scheme {
            vars: vec![s.clone()],
            ty: Ty::arrow(Ty::list(int.clone()), st_s(sta_s.clone())),
        });
        // stArrayToList :: STArray s -> ST s [Integer]
        self.env.insert("stArrayToList".into(), Scheme {
            vars: vec![s.clone()],
            ty: Ty::arrow(sta_s.clone(), st_s(Ty::list(int.clone()))),
        });

        // -- Functor → Applicative → Monad hierarchy --

        // Type abbreviations for higher-kinded method types
        let fa = Ty::App(Box::new(tf.clone()), Box::new(ta.clone()));
        let fb = Ty::App(Box::new(tf.clone()), Box::new(tb.clone()));
        let ma = Ty::App(Box::new(tm.clone()), Box::new(ta.clone()));
        let mb = Ty::App(Box::new(tm.clone()), Box::new(tb.clone()));

        // Built-in Functor typeclass
        // fmap :: (a -> b) -> f a -> f b
        let fmap_ty = Ty::fun(&[Ty::arrow(ta.clone(), tb.clone()), fa.clone()], fb.clone());
        self.classes.insert("Functor".to_string(), ClassInfo {
            name: "Functor".to_string(),
            type_var: "f".to_string(),
            superclasses: vec![],
            methods: vec![
                ("fmap".to_string(), fmap_ty.clone()),
                ("<$>".to_string(), fmap_ty.clone()),
            ],
            default_methods: HashMap::new(),
        });
        self.env.insert("fmap".to_string(), Scheme {
            vars: vec![a.clone(), b.clone(), f.clone()],
            ty: fmap_ty.clone(),
        });
        self.env.insert("<$>".to_string(), Scheme {
            vars: vec![a.clone(), b.clone(), f.clone()],
            ty: fmap_ty,
        });

        // Functor instances (fmap and <$> map to same implementations)
        for tc_name in &["IO", "LuaIO", "ST"] {
            let mut method_fns = HashMap::new();
            method_fns.insert("fmap".to_string(), "fmap_IO".to_string());
            method_fns.insert("<$>".to_string(), "fmap_IO".to_string());
            self.instances.insert(
                ("Functor".to_string(), tc_name.to_string()),
                InstanceInfo {
                    class_name: "Functor".to_string(),
                    target_type: Ty::Con(tc_name.to_string()),
                    method_fns,
                },
            );
        }
        {
            let mut method_fns = HashMap::new();
            method_fns.insert("fmap".to_string(), "map".to_string());
            method_fns.insert("<$>".to_string(), "map".to_string());
            self.instances.insert(
                ("Functor".to_string(), "[]".to_string()),
                InstanceInfo {
                    class_name: "Functor".to_string(),
                    target_type: Ty::Con("[]".to_string()),
                    method_fns,
                },
            );
        }
        for tc_name in &["Maybe", "Either"] {
            let mut method_fns = HashMap::new();
            method_fns.insert("fmap".to_string(), format!("fmap_{}", tc_name));
            method_fns.insert("<$>".to_string(), format!("fmap_{}", tc_name));
            self.instances.insert(
                ("Functor".to_string(), tc_name.to_string()),
                InstanceInfo {
                    class_name: "Functor".to_string(),
                    target_type: Ty::Con(tc_name.to_string()),
                    method_fns,
                },
            );
        }

        // Built-in Applicative typeclass (superclass: Functor)
        // pure  :: a -> f a
        // (<*>) :: f (a -> b) -> f a -> f b
        let pure_ty = Ty::arrow(ta.clone(), fa.clone());
        let fab = Ty::App(Box::new(tf.clone()), Box::new(Ty::arrow(ta.clone(), tb.clone())));
        let ap_ty = Ty::fun(&[fab, fa.clone()], fb.clone());
        self.classes.insert("Applicative".to_string(), ClassInfo {
            name: "Applicative".to_string(),
            type_var: "f".to_string(),
            superclasses: vec!["Functor".to_string()],
            methods: vec![
                ("pure".to_string(), pure_ty.clone()),
                ("<*>".to_string(), ap_ty.clone()),
            ],
            default_methods: HashMap::new(),
        });
        self.env.insert("pure".to_string(), Scheme {
            vars: vec![a.clone(), f.clone()],
            ty: pure_ty.clone(),
        });
        self.env.insert("return".to_string(), Scheme {
            vars: vec![a.clone(), f.clone()],
            ty: pure_ty,
        });
        self.env.insert("<*>".to_string(), Scheme {
            vars: vec![a.clone(), b.clone(), f.clone()],
            ty: ap_ty,
        });

        // Applicative instances
        for tc_name in &["IO", "LuaIO", "ST"] {
            let mut method_fns = HashMap::new();
            method_fns.insert("pure".to_string(), "pure".to_string());
            method_fns.insert("<*>".to_string(), "ap_IO".to_string());
            self.instances.insert(
                ("Applicative".to_string(), tc_name.to_string()),
                InstanceInfo {
                    class_name: "Applicative".to_string(),
                    target_type: Ty::Con(tc_name.to_string()),
                    method_fns,
                },
            );
        }
        {
            let mut method_fns = HashMap::new();
            method_fns.insert("pure".to_string(), "pure_List".to_string());
            method_fns.insert("<*>".to_string(), "ap_List".to_string());
            self.instances.insert(
                ("Applicative".to_string(), "[]".to_string()),
                InstanceInfo {
                    class_name: "Applicative".to_string(),
                    target_type: Ty::Con("[]".to_string()),
                    method_fns,
                },
            );
        }
        {
            let mut method_fns = HashMap::new();
            method_fns.insert("pure".to_string(), "pure_Maybe".to_string());
            method_fns.insert("<*>".to_string(), "ap_Maybe".to_string());
            self.instances.insert(
                ("Applicative".to_string(), "Maybe".to_string()),
                InstanceInfo {
                    class_name: "Applicative".to_string(),
                    target_type: Ty::Con("Maybe".to_string()),
                    method_fns,
                },
            );
        }
        {
            let mut method_fns = HashMap::new();
            method_fns.insert("pure".to_string(), "pure_Either".to_string());
            method_fns.insert("<*>".to_string(), "ap_Either".to_string());
            self.instances.insert(
                ("Applicative".to_string(), "Either".to_string()),
                InstanceInfo {
                    class_name: "Applicative".to_string(),
                    target_type: Ty::Con("Either".to_string()),
                    method_fns,
                },
            );
        }

        // Built-in Monad typeclass (superclass: Applicative)
        // >>=    :: m a -> (a -> m b) -> m b
        // >>     :: m a -> m b -> m b
        // return :: a -> m a
        self.classes.insert("Monad".to_string(), ClassInfo {
            name: "Monad".to_string(),
            type_var: "m".to_string(),
            superclasses: vec!["Applicative".to_string()],
            methods: vec![
                (">>=".to_string(), Ty::fun(&[ma.clone(), Ty::arrow(ta.clone(), mb.clone())], mb.clone())),
                (">>".to_string(), Ty::fun(&[ma.clone(), mb.clone()], mb.clone())),
                ("return".to_string(), Ty::arrow(ta.clone(), ma.clone())),
            ],
            default_methods: HashMap::new(),
        });
        // >>= and >> env entries
        self.env.insert(">>=".to_string(), Scheme {
            vars: vec![a.clone(), b.clone(), m.clone()],
            ty: Ty::fun(&[ma.clone(), Ty::arrow(ta.clone(), mb.clone())], mb.clone()),
        });
        self.env.insert(">>".to_string(), Scheme {
            vars: vec![a.clone(), b.clone(), m.clone()],
            ty: Ty::fun(&[ma.clone(), mb.clone()], mb.clone()),
        });

        // Monad instances for IO, LuaIO, ST
        for monad_name in &["IO", "LuaIO", "ST"] {
            let mut method_fns = HashMap::new();
            method_fns.insert(">>=".to_string(), ">>=".to_string());
            method_fns.insert(">>".to_string(), ">>".to_string());
            method_fns.insert("return".to_string(), "pure".to_string());
            self.instances.insert(
                ("Monad".to_string(), monad_name.to_string()),
                InstanceInfo {
                    class_name: "Monad".to_string(),
                    target_type: Ty::Con(monad_name.to_string()),
                    method_fns,
                },
            );
        }

        // Monad instance for [] (lists)
        {
            let mut method_fns = HashMap::new();
            method_fns.insert(">>=".to_string(), "bind_List".to_string());
            method_fns.insert(">>".to_string(), "then_List".to_string());
            method_fns.insert("return".to_string(), "pure_List".to_string());
            self.instances.insert(
                ("Monad".to_string(), "[]".to_string()),
                InstanceInfo {
                    class_name: "Monad".to_string(),
                    target_type: Ty::Con("[]".to_string()),
                    method_fns,
                },
            );
        }

        // Monad instance for Maybe
        {
            let mut method_fns = HashMap::new();
            method_fns.insert(">>=".to_string(), "bind_Maybe".to_string());
            method_fns.insert(">>".to_string(), "then_Maybe".to_string());
            method_fns.insert("return".to_string(), "pure_Maybe".to_string());
            self.instances.insert(
                ("Monad".to_string(), "Maybe".to_string()),
                InstanceInfo {
                    class_name: "Monad".to_string(),
                    target_type: Ty::Con("Maybe".to_string()),
                    method_fns,
                },
            );
        }

        // Built-in Enum typeclass
        // succ :: a -> a
        // pred :: a -> a
        // toEnum :: Integer -> a
        // fromEnum :: a -> Integer
        // enumFrom :: a -> [a]
        // enumFromThen :: a -> a -> [a]
        // enumFromTo :: a -> a -> [a]
        // enumFromThenTo :: a -> a -> a -> [a]
        let succ_ty = Ty::arrow(ta.clone(), ta.clone());
        let to_enum_ty = Ty::arrow(Ty::Con("Integer".into()), ta.clone());
        let from_enum_ty = Ty::arrow(ta.clone(), Ty::Con("Integer".into()));
        let enum_from_ty = Ty::arrow(ta.clone(), Ty::List(Box::new(ta.clone())));
        let enum_from_then_ty = Ty::fun(&[ta.clone(), ta.clone()], Ty::List(Box::new(ta.clone())));
        let enum_from_to_ty = Ty::fun(&[ta.clone(), ta.clone()], Ty::List(Box::new(ta.clone())));
        let enum_from_then_to_ty = Ty::fun(&[ta.clone(), ta.clone(), ta.clone()], Ty::List(Box::new(ta.clone())));
        self.classes.insert("Enum".to_string(), ClassInfo {
            name: "Enum".to_string(),
            type_var: "a".to_string(),
            superclasses: vec![],
            methods: vec![
                ("succ".to_string(), succ_ty.clone()),
                ("pred".to_string(), succ_ty.clone()),
                ("toEnum".to_string(), to_enum_ty.clone()),
                ("fromEnum".to_string(), from_enum_ty.clone()),
                ("enumFrom".to_string(), enum_from_ty.clone()),
                ("enumFromThen".to_string(), enum_from_then_ty.clone()),
                ("enumFromTo".to_string(), enum_from_to_ty.clone()),
                ("enumFromThenTo".to_string(), enum_from_then_to_ty.clone()),
            ],
            default_methods: HashMap::new(),
        });
        for (name, ty) in &[
            ("succ", succ_ty.clone()), ("pred", succ_ty),
            ("toEnum", to_enum_ty), ("fromEnum", from_enum_ty),
            ("enumFrom", enum_from_ty), ("enumFromThen", enum_from_then_ty),
            ("enumFromTo", enum_from_to_ty), ("enumFromThenTo", enum_from_then_to_ty),
        ] {
            self.env.insert(name.to_string(), Scheme {
                vars: vec![a.clone()],
                ty: ty.clone(),
            });
        }

        // Enum instance for Integer
        {
            let mut method_fns = HashMap::new();
            method_fns.insert("succ".to_string(), "succ_Integer".to_string());
            method_fns.insert("pred".to_string(), "pred_Integer".to_string());
            method_fns.insert("toEnum".to_string(), "toEnum_Integer".to_string());
            method_fns.insert("fromEnum".to_string(), "fromEnum_Integer".to_string());
            method_fns.insert("enumFrom".to_string(), "enumFrom_Integer".to_string());
            method_fns.insert("enumFromThen".to_string(), "enumFromThen_Integer".to_string());
            method_fns.insert("enumFromTo".to_string(), "enumFromTo_Integer".to_string());
            method_fns.insert("enumFromThenTo".to_string(), "enumFromThenTo_Integer".to_string());
            self.instances.insert(
                ("Enum".to_string(), "Integer".to_string()),
                InstanceInfo {
                    class_name: "Enum".to_string(),
                    target_type: Ty::Con("Integer".to_string()),
                    method_fns,
                },
            );
        }

        // Built-in Bounded typeclass
        let min_bound_ty = ta.clone();
        let max_bound_ty = ta.clone();
        self.classes.insert("Bounded".to_string(), ClassInfo {
            name: "Bounded".to_string(),
            type_var: "a".to_string(),
            superclasses: vec![],
            methods: vec![
                ("minBound".to_string(), min_bound_ty.clone()),
                ("maxBound".to_string(), max_bound_ty.clone()),
            ],
            default_methods: HashMap::new(),
        });
        for (name, ty) in &[
            ("minBound", min_bound_ty),
            ("maxBound", max_bound_ty),
        ] {
            self.env.insert(name.to_string(), Scheme {
                vars: vec![a.clone()],
                ty: ty.clone(),
            });
        }

        // Built-in Show typeclass
        let show_ty = Ty::arrow(ta.clone(), Ty::Con("String".into()));
        self.classes.insert("Show".to_string(), ClassInfo {
            name: "Show".to_string(),
            type_var: "a".to_string(),
            superclasses: vec![],
            methods: vec![("show".to_string(), show_ty.clone())],
            default_methods: HashMap::new(),
        });
        self.env.insert("show".to_string(), Scheme {
            vars: vec![a.clone()],
            ty: show_ty,
        });

        // Built-in Read typeclass
        let read_ty = Ty::arrow(Ty::Con("String".into()), ta.clone());
        self.classes.insert("Read".to_string(), ClassInfo {
            name: "Read".to_string(),
            type_var: "a".to_string(),
            superclasses: vec![],
            methods: vec![("read".to_string(), read_ty.clone())],
            default_methods: HashMap::new(),
        });
        self.env.insert("read".to_string(), Scheme {
            vars: vec![a.clone()],
            ty: read_ty,
        });
        // Read instances for base types
        for type_name in &["Integer", "Number", "Bool", "String"] {
            let mut method_fns = HashMap::new();
            method_fns.insert("read".to_string(), format!("read_{}", type_name));
            self.instances.insert(
                ("Read".to_string(), type_name.to_string()),
                InstanceInfo {
                    class_name: "Read".to_string(),
                    target_type: Ty::Con(type_name.to_string()),
                    method_fns,
                },
            );
        }

        // Built-in Eq typeclass
        let eq_ty = Ty::fun(&[ta.clone(), ta.clone()], Ty::Con("Bool".into()));
        self.classes.insert("Eq".to_string(), ClassInfo {
            name: "Eq".to_string(),
            type_var: "a".to_string(),
            superclasses: vec![],
            methods: vec![("==".to_string(), eq_ty.clone())],
            default_methods: HashMap::new(),
        });
        self.env.insert("==".to_string(), Scheme {
            vars: vec![a.clone()],
            ty: eq_ty,
        });
        // /= is derived from ==
        self.env.insert("/=".to_string(), Scheme {
            vars: vec![a.clone()],
            ty: Ty::fun(&[ta.clone(), ta.clone()], Ty::Con("Bool".into())),
        });

        // Eq instances for base types
        for type_name in &["Integer", "Number", "String", "Bool", "ByteString"] {
            let target = Ty::Con(type_name.to_string());
            let mangled = format!("eq_{}", type_name);
            let mut method_fns = HashMap::new();
            method_fns.insert("==".to_string(), mangled);
            self.instances.insert(
                ("Eq".to_string(), type_name.to_string()),
                InstanceInfo {
                    class_name: "Eq".to_string(),
                    target_type: target,
                    method_fns,
                },
            );
        }

        // Built-in Ord typeclass (superclass: Eq)
        let cmp_ty = Ty::fun(&[ta.clone(), ta.clone()], Ty::Con("Bool".into()));
        // `compare` is an Ord method returning Ordering (defined in the prelude).
        let compare_ty = Ty::fun(&[ta.clone(), ta.clone()], Ty::Con("Ordering".into()));
        self.classes.insert("Ord".to_string(), ClassInfo {
            name: "Ord".to_string(),
            type_var: "a".to_string(),
            superclasses: vec!["Eq".to_string()],
            methods: vec![
                ("<".to_string(), cmp_ty.clone()),
                (">".to_string(), cmp_ty.clone()),
                ("<=".to_string(), cmp_ty.clone()),
                (">=".to_string(), cmp_ty.clone()),
                ("compare".to_string(), compare_ty.clone()),
            ],
            default_methods: HashMap::new(),
        });
        for op in &["<", ">", "<=", ">="] {
            self.env.insert(op.to_string(), Scheme {
                vars: vec![a.clone()],
                ty: cmp_ty.clone(),
            });
        }
        self.env.insert("compare".to_string(), Scheme {
            vars: vec![a.clone()],
            ty: compare_ty.clone(),
        });

        // Class constraints carried by the built-in class methods. Each
        // constrains the class variable "a"; a use whose "a" resolves to a
        // concrete type with no instance (a function, an IO action, a type
        // without the relevant deriving) is rejected at the function boundary.
        let cm: &[(&str, &str)] = &[
            ("show", "Show"), ("read", "Read"),
            ("==", "Eq"), ("/=", "Eq"),
            ("<", "Ord"), (">", "Ord"), ("<=", "Ord"), (">=", "Ord"),
            ("compare", "Ord"),
        ];
        for (method, class) in cm {
            self.method_constraints.insert(method.to_string(), vec![TyConstraint {
                class_name: class.to_string(),
                type_var: "a".to_string(),
            }]);
        }

        // Ord instances for base types
        for type_name in &["Integer", "Number", "String", "ByteString"] {
            let target = Ty::Con(type_name.to_string());
            let mut method_fns = HashMap::new();
            for op in &["<", ">", "<=", ">="] {
                method_fns.insert(op.to_string(), format!("ord_{}__{}", op_to_name(op), type_name));
            }
            // Every base Ord type has a `compare` runtime helper.
            method_fns.insert("compare".to_string(), format!("ord_compare__{}", type_name));
            self.instances.insert(
                ("Ord".to_string(), type_name.to_string()),
                InstanceInfo {
                    class_name: "Ord".to_string(),
                    target_type: target,
                    method_fns,
                },
            );
        }

        // Built-in Semigroup typeclass
        let semigroup_ty = Ty::fun(&[ta.clone(), ta.clone()], ta.clone());
        self.classes.insert("Semigroup".to_string(), ClassInfo {
            name: "Semigroup".to_string(),
            type_var: "a".to_string(),
            superclasses: vec![],
            methods: vec![("<>".to_string(), semigroup_ty.clone())],
            default_methods: HashMap::new(),
        });
        self.env.insert("<>".to_string(), Scheme {
            vars: vec![a.clone()],
            ty: semigroup_ty,
        });

        // Semigroup instances
        {
            // String: <> is string concatenation
            let mut method_fns = HashMap::new();
            method_fns.insert("<>".to_string(), "semigroup_String".to_string());
            self.instances.insert(
                ("Semigroup".to_string(), "String".to_string()),
                InstanceInfo {
                    class_name: "Semigroup".to_string(),
                    target_type: Ty::Con("String".into()),
                    method_fns,
                },
            );
            // []: <> is list append (same as ++)
            let mut method_fns = HashMap::new();
            method_fns.insert("<>".to_string(), "semigroup_List".to_string());
            self.instances.insert(
                ("Semigroup".to_string(), "[]".to_string()),
                InstanceInfo {
                    class_name: "Semigroup".to_string(),
                    target_type: Ty::list(ta.clone()),
                    method_fns,
                },
            );
        }

        // Show instances for base types and parameterized types
        for type_name in &["Integer", "Number", "String", "Bool", "[]", "Maybe", "ByteString"] {
            let target = Ty::Con(type_name.to_string());
            let mangled = format!("show_{}", type_name);
            let mut method_fns = HashMap::new();
            method_fns.insert("show".to_string(), mangled);
            self.instances.insert(
                ("Show".to_string(), type_name.to_string()),
                InstanceInfo {
                    class_name: "Show".to_string(),
                    target_type: target,
                    method_fns,
                },
            );
        }
    }

    fn init_kinds(&mut self) {
        // Base types: kind Type
        for name in &["Integer", "Number", "String", "Bool", "()", "ByteString"] {
            self.kinds.insert(name.to_string(), Kind::Type);
        }
        // Type constructors: kind Type -> Type
        let type_to_type = Kind::Arrow(Box::new(Kind::Type), Box::new(Kind::Type));
        for name in &["Maybe", "IO", "[]"] {
            self.kinds.insert(name.to_string(), type_to_type.clone());
        }
        // LuaFunction: kind Type -> Type
        self.kinds.insert("LuaFunction".to_string(), type_to_type.clone());
        // ST: kind Type -> Type -> Type (ST s a)
        self.kinds.insert("ST".to_string(),
            Kind::Arrow(Box::new(Kind::Type),
                Box::new(Kind::Arrow(Box::new(Kind::Type), Box::new(Kind::Type)))));
        // STArray: kind Type -> Type (parameterized by scope s)
        self.kinds.insert("STArray".to_string(), type_to_type.clone());
        // HashMap: kind Type -> Type -> Type
        self.kinds.insert("HashMap".to_string(),
            Kind::Arrow(Box::new(Kind::Type),
                Box::new(Kind::Arrow(Box::new(Kind::Type), Box::new(Kind::Type)))));
        // Show instance for HashMap (uses Lua show fallback)
        self.instances.insert(
            ("Show".to_string(), "HashMap".to_string()),
            InstanceInfo {
                class_name: "Show".to_string(),
                target_type: Ty::Con("HashMap".into()),
                method_fns: {
                    let mut m = HashMap::new();
                    m.insert("show".to_string(), "show_HashMap".to_string());
                    m
                },
            },
        );

        // Int as alias for Integer (Lua has no fixed-width integers)
        self.type_aliases.insert("Int".to_string(),
            (vec![], Type::Con("Integer".to_string())));
    }

    /// Get the kind of a type constructor, or infer Type for unknowns.
    pub fn kind_of(&self, name: &str) -> Kind {
        self.kinds.get(name).cloned().unwrap_or(Kind::Type)
    }

    /// Infer the kind of an AST type expression and report errors.
    fn check_type_kind(&mut self, ty: &Type) -> Kind {
        match ty {
            Type::Con(name) => self.kind_of(name),
            Type::Var(_) => Kind::Type, // type variables are assumed to be Type
            Type::Arrow(a, b) => {
                let ka = self.check_type_kind(a);
                let kb = self.check_type_kind(b);
                if ka != Kind::Type {
                    self.push_error_ctx(
                        TypeErrorKind::Other(format!("Kind error: argument of '->' has kind {}, expected Type", ka)),
                        "type expression".to_string(),
                    );
                }
                if kb != Kind::Type {
                    self.push_error_ctx(
                        TypeErrorKind::Other(format!("Kind error: result of '->' has kind {}, expected Type", kb)),
                        "type expression".to_string(),
                    );
                }
                Kind::Type
            }
            Type::App(f, a) => {
                let kf = self.check_type_kind(f);
                let _ka = self.check_type_kind(a);
                match kf {
                    Kind::Arrow(_, result) => *result,
                    Kind::Type => {
                        // Applying a Type-kinded thing — this is a kind error
                        // but only report if it's a known constructor
                        if let Type::Con(name) = f.as_ref()
                            && self.kinds.contains_key(name) {
                                self.push_error_ctx(
                                    TypeErrorKind::Other(format!(
                                        "Kind error: '{}' has kind Type and cannot be applied to an argument",
                                        name
                                    )),
                                    "type expression".to_string(),
                                );
                            }
                        Kind::Type
                    }
                    _ => Kind::Type,
                }
            }
            Type::List(_) | Type::IO(_) | Type::Unit => Kind::Type,
            Type::Paren(inner) => self.check_type_kind(inner),
            Type::Forall { inner, .. } => self.check_type_kind(inner),
            Type::Constrained { ty, .. } => self.check_type_kind(ty),
            Type::Promoted(name) => {
                let key = format!("'{}", name);
                if let Some(kind) = self.kinds.get(&key).cloned() {
                    kind
                } else {
                    self.push_error_ctx(
                        TypeErrorKind::Other(format!("Unknown promoted constructor '{}'", name)),
                        "type expression".to_string(),
                    );
                    Kind::Type
                }
            }
            _ => Kind::Type,
        }
    }

    /// Register a data type's kind based on its type parameters.
    fn register_kind(&mut self, name: &str, num_params: usize) {
        let mut kind = Kind::Type;
        for _ in 0..num_params {
            kind = Kind::Arrow(Box::new(Kind::Type), Box::new(kind));
        }
        self.kinds.insert(name.to_string(), kind);
    }

    // --- Data types ---

    fn register_data_type(&mut self, name: &str, type_vars: &[String], constructors: &[Constructor]) {
        self.register_kind(name, type_vars.len());
        let tvars: Vec<TyVar> = type_vars.iter()
            .map(|n| TyVar { name: n.clone(), id: u32::MAX })
            .collect();
        let result_type = tvars.iter().fold(Ty::Con(name.to_string()), |acc, tv| Ty::app(acc, Ty::Var(tv.clone())));

        // DataKinds: register promoted constructors with appropriate kinds
        // Nullary constructors get Kind::Type; constructors with N fields
        // get Kind::Arrow^N(Type, Type) so they can be applied at the type level.
        for con in constructors {
            let field_count = match &con.fields {
                crate::ast::ConstructorFields::Positional(fs) => fs.len(),
                crate::ast::ConstructorFields::Named(fs) => fs.len(),
            };
            let mut kind = Kind::Type;
            for _ in 0..field_count {
                kind = Kind::Arrow(Box::new(Kind::Type), Box::new(kind));
            }
            self.kinds.insert(format!("'{}", con.name), kind);
        }

        for (i, con) in constructors.iter().enumerate() {
            // Collect existential type variables for this constructor
            let ex_tvars: Vec<TyVar> = con.existential_vars.iter()
                .map(|n| TyVar { name: n.clone(), id: u32::MAX })
                .collect();

            let (field_types, con_result_type) = if let Some(gadt_ty) = &con.gadt_type {
                // GADT constructor: decompose type sig into args + return type
                let full_ty = self.ast_type_to_ty(gadt_ty);
                let mut args = Vec::new();
                let mut cur = full_ty;
                while let Ty::Arrow(a, b) = cur {
                    args.push(*a);
                    cur = *b;
                }
                (args, cur)
            } else {
                // Standard ADT constructor
                let fts: Vec<Ty> = match &con.fields {
                    ConstructorFields::Positional(types) => types.iter().map(|t| self.ast_type_to_ty(t)).collect(),
                    ConstructorFields::Named(fields) => fields.iter().map(|(_, t)| self.ast_type_to_ty(t)).collect(),
                };
                (fts, result_type.clone())
            };

            let con_type = if field_types.is_empty() { con_result_type.clone() } else { Ty::fun(&field_types, con_result_type.clone()) };

            // Constructor scheme includes both universal (data type) and existential vars
            let mut all_scheme_vars = tvars.clone();
            all_scheme_vars.extend(ex_tvars.clone());

            self.constructors.insert(con.name.clone(), ConInfo {
                type_name: name.to_string(), variant_index: i + 1, total_variants: constructors.len(),
                field_types: field_types.clone(), type_vars: tvars.clone(), result_type: con_result_type.clone(),
                existential_vars: ex_tvars,
            });
            self.env.insert(con.name.clone(), Scheme { vars: all_scheme_vars, ty: con_type });

            // Register record field accessors
            if let ConstructorFields::Named(fields) = &con.fields {
                for (fi, (field_name, field_ast_ty)) in fields.iter().enumerate() {
                    let field_ty = self.ast_type_to_ty(field_ast_ty);
                    // accessor :: DataType -> FieldType
                    let accessor_ty = Ty::arrow(result_type.clone(), field_ty);
                    self.env.insert(field_name.clone(), Scheme {
                        vars: tvars.clone(),
                        ty: accessor_ty,
                    });
                    // Store field index for codegen
                    let index = if constructors.len() == 1 { fi + 1 } else { fi + 2 };
                    self.record_fields.insert(field_name.clone(), (name.to_string(), index));
                }
            }
        }
    }

    /// Register a newtype as a zero-cost wrapper.
    /// `newtype Age = Integer` creates constructor `Age :: Integer -> Age`
    /// that is the identity function at runtime.
    fn register_newtype(&mut self, name: &str, type_vars: &[String], inner: &Type) {
        self.register_kind(name, type_vars.len());
        let tvars: Vec<TyVar> = type_vars.iter()
            .map(|n| TyVar { name: n.clone(), id: u32::MAX })
            .collect();
        let result_type = tvars.iter().fold(
            Ty::Con(name.to_string()),
            |acc, tv| Ty::app(acc, Ty::Var(tv.clone())),
        );
        let inner_ty = self.ast_type_to_ty(inner);

        // Register constructor: Name :: InnerType -> Name
        self.constructors.insert(name.to_string(), ConInfo {
            type_name: name.to_string(),
            variant_index: 1,
            total_variants: 1,
            field_types: vec![inner_ty.clone()],
            type_vars: tvars.clone(),
            result_type: result_type.clone(),
            existential_vars: vec![],
        });
        self.env.insert(name.to_string(), Scheme {
            vars: tvars,
            ty: Ty::arrow(inner_ty, result_type),
        });
    }

    fn convert_data_def(&mut self, name: &str, type_vars: &[String], constructors: &[Constructor]) -> TDataDef {
        TDataDef {
            name: name.to_string(),
            type_vars: type_vars.to_vec(),
            constructors: constructors.iter().map(|c| {
                TConstructor {
                    name: c.name.clone(),
                    fields: if c.gadt_type.is_some() {
                        // GADT: field types come from the registered ConInfo
                        let con_info = self.constructors.get(&c.name).unwrap();
                        TConFields::Positional(con_info.field_types.clone())
                    } else {
                        match &c.fields {
                            ConstructorFields::Positional(types) =>
                                TConFields::Positional(types.iter().map(|t| self.ast_type_to_ty(t)).collect()),
                            ConstructorFields::Named(fields) =>
                                TConFields::Named(fields.iter().map(|(n, t)| (n.clone(), self.ast_type_to_ty(t))).collect()),
                        }
                    },
                }
            }).collect(),
        }
    }

    // --- Module checking (produces TIR) ---

    /// Check a module, with orphan instance detection.
    /// `local_start` is the index into `module.decls` where locally-defined
    /// declarations begin (everything before is prelude or imported).
    pub fn check_module_with_local_start(&mut self, module: &Module, local_start: usize) -> TModule {
        // Collect names defined locally (classes and types)
        let mut local_classes: HashSet<String> = HashSet::new();
        let mut local_types: HashSet<String> = HashSet::new();
        for decl in &module.decls[local_start..] {
            match decl {
                Decl::ClassDecl { name, .. } => { local_classes.insert(name.clone()); }
                Decl::DataDef { name, .. } => { local_types.insert(name.clone()); }
                Decl::NewtypeDef { name, .. } => { local_types.insert(name.clone()); }
                _ => {}
            }
        }
        self.local_classes = local_classes;
        self.local_types = local_types;
        self.orphan_check_enabled = true;
        self.local_decl_start = local_start;
        self.check_module(module)
    }

    pub fn check_module(&mut self, module: &Module) -> TModule {
        // Register hidden names from import export control
        self.hidden_names.extend(module.hidden.iter().cloned());

        // Pass 1: register type aliases, data types, and newtypes
        // Type aliases must be registered first so that data constructors
        // referencing aliases (e.g. `data Foo = Foo MyAlias`) expand correctly.
        for decl in &module.decls {
            if let Decl::TypeAlias { name, params, ty } = decl {
                self.type_aliases.insert(name.clone(), (params.clone(), ty.clone()));
            }
        }
        for decl in &module.decls {
            match decl {
                Decl::DataDef { name, type_vars, constructors, .. } => {
                    self.register_data_type(name, type_vars, constructors);
                }
                Decl::NewtypeDef { name, type_vars, inner } => {
                    self.register_newtype(name, type_vars, inner);
                }
                _ => {}
            }
        }

        // Pass 2: register typeclass declarations and type families
        for decl in &module.decls {
            match decl {
                Decl::ClassDecl { name, type_var, superclasses, methods } => {
                    self.register_class(name, type_var, superclasses, methods);
                }
                Decl::TypeFamily { name, equations } => {
                    self.type_families.insert(name.clone(), equations.clone());
                }
                Decl::TypeAlias { name, params, ty } => {
                    self.type_aliases.insert(name.clone(), (params.clone(), ty.clone()));
                }
                _ => {}
            }
        }

        // Pass 3: collect type signatures and FFI info
        let mut sigs: HashMap<String, Ty> = HashMap::new();
        let mut ffi_info: HashMap<String, (String, FfiKind)> = HashMap::new();
        for decl in &module.decls {
            if let Decl::TypeSig { name, ty } = decl {
                // Kind-check the type signature
                self.check_type_kind(ty);
                // Extract FFI info before reducing the type
                if let Some(info) = extract_ffi_info(ty) {
                    ffi_info.insert(name.clone(), info);
                }
                // Extract typeclass constraints before ast_type_to_ty discards them
                if let Type::Constrained { constraints, .. } = ty {
                    let ty_constraints: Vec<TyConstraint> = constraints.iter().map(|c| {
                        let type_var = match &c.type_arg {
                            Type::Var(v) => v.clone(),
                            _ => format!("{:?}", c.type_arg),
                        };
                        TyConstraint { class_name: c.class_name.clone(), type_var }
                    }).collect();
                    if !ty_constraints.is_empty() {
                        self.fn_constraints.insert(name.clone(), ty_constraints);
                    }
                }
                sigs.insert(name.clone(), self.ast_type_to_ty(ty));
            }
        }

        // Collect names that have function bodies
        let mut defined_fns: std::collections::HashSet<String> = std::collections::HashSet::new();
        for decl in &module.decls {
            if let Decl::FunDef { name, .. } = decl {
                defined_fns.insert(name.clone());
            }
        }

        // Pass 4a: process deriving clauses first (so derived instances
        // are available when checking explicit instances with superclass constraints)
        let mut instance_fns = Vec::new();
        for decl in &module.decls {
            if let Decl::DataDef { name, type_vars, constructors, deriving } = decl {
                for class in deriving {
                    let derived = self.derive_instance(class, name, type_vars, constructors);
                    instance_fns.extend(derived);
                }
            }
        }

        // Pass 4b: register and check explicit instance declarations
        for decl in &module.decls {
            if let Decl::InstanceDecl { class_name, target_type, methods } = decl {
                let ifns = self.check_instance(class_name, target_type, methods);
                instance_fns.extend(ifns);
            }
        }

        // Pass 5: generate FFI functions (type sigs with LuaPure/LuaIO and no body)
        let mut data_defs = Vec::new();
        let mut functions = Vec::new();
        let mut has_main = false;

        // Sorted: ffi_info is a HashMap, and this order determines the order
        // FFI functions are emitted (and their __mll_fn slots assigned).
        let mut ffi_names: Vec<&String> = ffi_info.keys().collect();
        ffi_names.sort();
        for name in ffi_names {
            let (lua_name, ffi_kind) = &ffi_info[name];
            if !defined_fns.contains(name)
                && let Some(ty) = sigs.get(name) {
                    self.validate_ffi_callbacks(name, ty);
                    let ffi_fn = self.generate_ffi_function(name, lua_name, *ffi_kind, ty);
                    functions.push(ffi_fn);
                    // Register in env
                    let scheme = self.generalize(&self.env.clone(), ty);
                    self.env.insert(name.clone(), scheme);
                }
        }

        // Pre-register all function signatures so mutually recursive
        // functions can see each other during type checking
        for (name, ty) in &sigs {
            let scheme = self.generalize(&self.env.clone(), ty);
            self.env.insert(name.clone(), scheme);
        }

        // Local declarations that redefine a hidden name should shadow it
        if !self.hidden_names.is_empty() && self.local_decl_start > 0 {
            for decl in module.decls.iter().skip(self.local_decl_start) {
                match decl {
                    Decl::TypeSig { name, .. } | Decl::FunDef { name, .. } => {
                        self.hidden_names.remove(name);
                    }
                    Decl::DataDef { name, constructors, .. } => {
                        self.hidden_names.remove(name);
                        for con in constructors {
                            self.hidden_names.remove(&con.name);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Reject type signatures that have no accompanying definition and are
        // not FFI bindings. Without this, `foo :: Integer` with no body silently
        // compiles to a nil value that errors only when forced at runtime — a
        // soundness hole (the type promises a value the program never provides).
        // Body-less signatures are legitimate only for FFI declarations
        // (LuaPure/LuaIO/LuaIterator/LuaTry), which `ffi_info` tracks.
        let mut undefined_sigs: Vec<&String> = sigs.keys()
            .filter(|name| !defined_fns.contains(*name) && !ffi_info.contains_key(*name))
            .collect();
        undefined_sigs.sort();
        for name in undefined_sigs {
            self.push_error_ctx(
                TypeErrorKind::Other(format!(
                    "Type signature for '{}' has no accompanying definition", name)),
                format!("signature '{}'", name),
            );
        }

        // Pass 6: collect exports and check function definitions
        let mut exports = Vec::new();
        for (decl_idx, decl) in module.decls.iter().enumerate() {
            // Enable hidden name enforcement only for local (user) declarations
            self.enforce_hidden = !self.hidden_names.is_empty()
                && self.local_decl_start > 0
                && decl_idx >= self.local_decl_start;
            match decl {
                Decl::DataDef { name, type_vars, constructors, .. } => {
                    data_defs.push(self.convert_data_def(name, type_vars, constructors));
                }
                Decl::FunDef { name, clauses } => {
                    if name == "main" { has_main = true; }
                    if let Some(declared_ty) = sigs.get(name) {
                        if let Some(tfun) = self.check_function(name, clauses, declared_ty) {
                            functions.push(tfun);
                        }
                    } else {
                        self.push_error_ctx(
                            TypeErrorKind::Other(format!("Missing type signature for '{}'", name)),
                            format!("definition of '{}'", name),
                        );
                    }
                }
                Decl::ExportSig { name, ty } => {
                    exports.push(name.clone());
                    // Validate: callback parameters in exports must return LuaIO s
                    self.check_export_callbacks(name, ty);
                }
                _ => {}
            }
        }

        // Sorted so codegen emits accessors (and assigns their __mll_fn slots)
        // in a deterministic order; record_fields is a HashMap.
        let mut record_accessors: Vec<(String, usize)> = self.record_fields.iter()
            .map(|(name, (_, idx))| (name.clone(), *idx))
            .collect();
        record_accessors.sort();

        let newtypes: Vec<String> = module.decls.iter().filter_map(|d| {
            if let Decl::NewtypeDef { name, .. } = d { Some(name.clone()) } else { None }
        }).collect();

        TModule { data_defs, functions, instance_fns, has_main, exports, record_accessors, newtypes }
    }

    // --- Typeclass handling ---

    fn register_class(&mut self, name: &str, type_var: &str, superclasses: &[String], methods: &[ClassMethod]) {
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
    fn type_head_name(ty: &Type) -> Option<String> {
        match ty {
            Type::Con(name) => Some(name.clone()),
            Type::App(f, _) => Self::type_head_name(f),
            Type::List(_) => Some("List".to_string()),
            Type::IO(_) => Some("IO".to_string()),
            Type::Paren(inner) => Self::type_head_name(inner),
            _ => None,
        }
    }

    fn check_instance(
        &mut self,
        class_name: &str,
        target_type: &Type,
        methods: &[InstanceMethod],
    ) -> Vec<TFunction> {
        let target_ty = self.ast_type_to_ty(target_type);
        let ty_str = format!("{}", target_ty);

        // Orphan instance detection: either the class or the type must be local.
        // Only checked when check_module_with_local_start was used (local_start tracking active).
        if self.orphan_check_enabled {
            let type_head = Self::type_head_name(target_type);
            let class_is_local = self.local_classes.contains(class_name);
            let type_is_local = type_head.as_ref().is_some_and(|t| self.local_types.contains(t));
            if !class_is_local && !type_is_local {
                self.push_error_ctx(
                    TypeErrorKind::Other(format!(
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
                    TypeErrorKind::Other(format!("Unknown typeclass '{}'", class_name)),
                    format!("instance {} {}", class_name, ty_str),
                );
                return vec![];
            }
        };

        // Check superclass constraints
        for superclass in &class_info.superclasses {
            let key = (superclass.clone(), ty_str.clone());
            if !self.instances.contains_key(&key) {
                self.push_error_ctx(
                    TypeErrorKind::Other(format!(
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

        for method_def in methods {
            // Find the class method's type
            let class_method_ty = class_info.methods.iter()
                .find(|(n, _)| n == &method_def.name)
                .map(|(_, ty)| ty.clone());

            let method_ty = match class_method_ty {
                Some(ty) => {
                    // Substitute the class type variable with the target type
                    let tv = TyVar { name: class_info.type_var.clone(), id: u32::MAX };
                    let subst = Subst::singleton(tv, target_ty.clone());
                    ty.apply_subst(&subst)
                }
                None => {
                    self.push_error_ctx(
                        TypeErrorKind::Other(format!("'{}' is not a method of class '{}'",
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
                let subst = Subst::singleton(tv, target_ty.clone());
                let specialized_ty = method_ty.apply_subst(&subst);

                let mangled_name = format!("{}_{}", method_name, ty_str);
                instance_info.method_fns.insert(method_name.clone(), mangled_name.clone());

                if let Some(tfun) = self.check_function(&mangled_name, default_clauses, &specialized_ty) {
                    result_fns.push(tfun);
                }
            }
        }

        self.instances.insert(
            (class_name.to_string(), ty_str),
            instance_info,
        );

        result_fns
    }

    /// Look up the instance method for a given class method + concrete type
    pub fn resolve_method(&self, method_name: &str, concrete_ty: &Ty) -> Option<String> {
        for class_info in self.classes.values() {
            if class_info.methods.iter().any(|(n, _)| n == method_name) {
                let ty_str = format!("{}", concrete_ty);
                let key = (class_info.name.clone(), ty_str);
                if let Some(inst) = self.instances.get(&key) {
                    return inst.method_fns.get(method_name).cloned();
                }
            }
        }
        None
    }

    /// Expose instances for the monomorphizer
    pub fn get_instances(&self) -> &HashMap<(String, String), InstanceInfo> {
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

    // --- Deriving ---

    fn derive_instance(
        &mut self,
        class: &str,
        type_name: &str,
        type_vars: &[String],
        constructors: &[Constructor],
    ) -> Vec<TFunction> {
        match class {
            "Show" => self.derive_show(type_name, type_vars, constructors),
            "Eq" => self.derive_eq(type_name, type_vars, constructors),
            "Ord" => self.derive_ord(type_name, type_vars, constructors),
            "Enum" => self.derive_enum(type_name, type_vars, constructors),
            "Bounded" => self.derive_bounded(type_name, type_vars, constructors),
            "Functor" => self.derive_functor(type_name, type_vars, constructors),
            other => {
                self.push_error_ctx(
                    TypeErrorKind::Other(format!("Cannot derive '{}' — only Show, Eq, Ord and Functor are supported", other)),
                    format!("data {}", type_name),
                );
                vec![]
            }
        }
    }

    /// Generate `show` for a data type.
    /// For each constructor, generates a clause that produces "Constructor field1 field2 ...".
    fn derive_show(
        &mut self,
        type_name: &str,
        type_vars: &[String],
        constructors: &[Constructor],
    ) -> Vec<TFunction> {
        let tvars: Vec<TyVar> = type_vars.iter()
            .map(|n| TyVar { name: n.clone(), id: u32::MAX })
            .collect();
        let result_type = tvars.iter().fold(
            Ty::Con(type_name.to_string()),
            |acc, tv| Ty::app(acc, Ty::Var(tv.clone())),
        );

        let mangled = format!("show_{}", type_name);
        let fn_ty = Ty::arrow(result_type.clone(), Ty::Con("String".into()));

        let mut clauses = Vec::new();
        for con in constructors {
            let field_count = match &con.fields {
                ConstructorFields::Positional(fs) => fs.len(),
                ConstructorFields::Named(fs) => fs.len(),
            };

            // Build patterns: Con p0 p1 p2 ...
            let param_names: Vec<String> = (0..field_count)
                .map(|i| format!("_s{}", i))
                .collect();

            let con_info = self.constructors.get(&con.name).cloned();
            let field_tys: Vec<Ty> = con_info.as_ref()
                .map(|ci| ci.field_types.clone())
                .unwrap_or_default();

            let patterns = vec![
                TPattern::Constructor {
                    name: con.name.clone(),
                    args: param_names.iter().enumerate().map(|(i, n)| {
                        let ty = field_tys.get(i).cloned().unwrap_or(Ty::Unit);
                        TPattern::Var(n.clone(), ty)
                    }).collect(),
                }
            ];

            // Build body: "ConName" ++ " " ++ show p0 ++ " " ++ show p1 ...
            let mut body = TExpr::new(
                TExprKind::Lit(TLiteral::Str(con.name.clone())),
                Ty::Con("String".into()),
            );

            for (i, pname) in param_names.iter().enumerate() {
                let field_ty = field_tys.get(i).cloned().unwrap_or(Ty::Unit);

                // " "
                let space = TExpr::new(
                    TExprKind::Lit(TLiteral::Str(" ".into())),
                    Ty::Con("String".into()),
                );
                // concat body <> " "
                body = TExpr::new(
                    TExprKind::InfixApp {
                        op: "<>".into(),
                        lhs: Box::new(body),
                        rhs: Box::new(space),
                    },
                    Ty::Con("String".into()),
                );

                // __mll_show_arg (show field_i) — parenthesize the field if it is
                // a constructor application or negative number (GHC showsPrec 11).
                let field_shown = TExpr::new(
                    TExprKind::App(
                        Box::new(TExpr::new(
                            TExprKind::Var("__mll_show_arg".into()),
                            Ty::arrow(Ty::Con("String".into()), Ty::Con("String".into())),
                        )),
                        Box::new(TExpr::new(
                            TExprKind::App(
                                Box::new(TExpr::new(
                                    TExprKind::Var("show".into()),
                                    Ty::arrow(field_ty.clone(), Ty::Con("String".into())),
                                )),
                                Box::new(TExpr::new(
                                    TExprKind::Var(pname.clone()),
                                    field_ty,
                                )),
                            ),
                            Ty::Con("String".into()),
                        )),
                    ),
                    Ty::Con("String".into()),
                );

                body = TExpr::new(
                    TExprKind::InfixApp {
                        op: "<>".into(),
                        lhs: Box::new(body),
                        rhs: Box::new(field_shown),
                    },
                    Ty::Con("String".into()),
                );
            }

            clauses.push(TClause {
                patterns,
                guards: vec![],
                body,
                where_binds: vec![],
            });
        }

        // Register the instance
        let mut method_fns = HashMap::new();
        method_fns.insert("show".to_string(), mangled.clone());
        self.instances.insert(
            ("Show".to_string(), type_name.to_string()),
            InstanceInfo {
                class_name: "Show".to_string(),
                target_type: result_type.clone(),
                method_fns,
            },
        );

        vec![TFunction {
            name: mangled,
            ty: fn_ty,
            clauses,
            specialized: false,
            dict_params: vec![],
        }]
    }

    /// Generate `==` for a data type.
    /// Two values are equal if they have the same constructor and all fields are equal.
    fn derive_eq(
        &mut self,
        type_name: &str,
        type_vars: &[String],
        constructors: &[Constructor],
    ) -> Vec<TFunction> {
        let tvars: Vec<TyVar> = type_vars.iter()
            .map(|n| TyVar { name: n.clone(), id: u32::MAX })
            .collect();
        let result_type = tvars.iter().fold(
            Ty::Con(type_name.to_string()),
            |acc, tv| Ty::app(acc, Ty::Var(tv.clone())),
        );

        let mangled = format!("eq_{}", type_name);
        let fn_ty = Ty::fun(&[result_type.clone(), result_type.clone()], Ty::Con("Bool".into()));

        let mut clauses = Vec::new();

        for con in constructors {
            let field_count = match &con.fields {
                ConstructorFields::Positional(fs) => fs.len(),
                ConstructorFields::Named(fs) => fs.len(),
            };

            let con_info = self.constructors.get(&con.name).cloned();
            let field_tys: Vec<Ty> = con_info.as_ref()
                .map(|ci| ci.field_types.clone())
                .unwrap_or_default();

            let a_names: Vec<String> = (0..field_count).map(|i| format!("_a{}", i)).collect();
            let b_names: Vec<String> = (0..field_count).map(|i| format!("_b{}", i)).collect();

            let pat_a = TPattern::Constructor {
                name: con.name.clone(),
                args: a_names.iter().enumerate().map(|(i, n)| {
                    let ty = field_tys.get(i).cloned().unwrap_or(Ty::Unit);
                    TPattern::Var(n.clone(), ty)
                }).collect(),
            };
            let pat_b = TPattern::Constructor {
                name: con.name.clone(),
                args: b_names.iter().enumerate().map(|(i, n)| {
                    let ty = field_tys.get(i).cloned().unwrap_or(Ty::Unit);
                    TPattern::Var(n.clone(), ty)
                }).collect(),
            };

            // Build body: a0 == b0 && a1 == b1 && ...
            // The monomorphizer resolves == to the appropriate eq function
            // for each field type (including polymorphic types like Tree a
            // where the type constructor has a known Eq instance).
            let mut body = TExpr::new(
                TExprKind::Lit(TLiteral::Bool(true)),
                Ty::Con("Bool".into()),
            );

            for i in (0..field_count).rev() {
                let field_ty = field_tys.get(i).cloned().unwrap_or(Ty::Unit);
                let eq_expr = TExpr::new(
                    TExprKind::InfixApp {
                        op: "==".into(),
                        lhs: Box::new(TExpr::new(
                            TExprKind::Var(a_names[i].clone()),
                            field_ty.clone(),
                        )),
                        rhs: Box::new(TExpr::new(
                            TExprKind::Var(b_names[i].clone()),
                            field_ty,
                        )),
                    },
                    Ty::Con("Bool".into()),
                );
                body = TExpr::new(
                    TExprKind::InfixApp {
                        op: "&&".into(),
                        lhs: Box::new(eq_expr),
                        rhs: Box::new(body),
                    },
                    Ty::Con("Bool".into()),
                );
            }

            clauses.push(TClause {
                patterns: vec![pat_a, pat_b],
                guards: vec![],
                body,
                where_binds: vec![],
            });
        }

        // Add catch-all clause for different constructors: _ _ = False
        if constructors.len() > 1 {
            clauses.push(TClause {
                patterns: vec![
                    TPattern::Wildcard,
                    TPattern::Wildcard,
                ],
                guards: vec![],
                body: TExpr::new(TExprKind::Lit(TLiteral::Bool(false)), Ty::Con("Bool".into())),
                where_binds: vec![],
            });
        }

        // Register the instance
        let mut method_fns = HashMap::new();
        method_fns.insert("==".to_string(), mangled.clone());
        self.instances.insert(
            ("Eq".to_string(), type_name.to_string()),
            InstanceInfo {
                class_name: "Eq".to_string(),
                target_type: result_type.clone(),
                method_fns,
            },
        );

        vec![TFunction {
            name: mangled,
            ty: fn_ty,
            clauses,
            specialized: false,
            dict_params: vec![],
        }]
    }

    fn derive_ord(
        &mut self,
        type_name: &str,
        type_vars: &[String],
        constructors: &[Constructor],
    ) -> Vec<TFunction> {
        let tvars: Vec<TyVar> = type_vars.iter()
            .map(|n| TyVar { name: n.clone(), id: u32::MAX })
            .collect();
        let result_type = tvars.iter().fold(
            Ty::Con(type_name.to_string()),
            |acc, tv| Ty::app(acc, Ty::Var(tv.clone())),
        );
        let bool_ty = Ty::Con("Bool".into());
        let fn_ty = Ty::fun(&[result_type.clone(), result_type.clone()], bool_ty.clone());

        // For enums (no fields), use constructor index comparison.
        // Constructors earlier in the declaration are "less than" later ones.
        let is_enum = constructors.iter().all(|c| match &c.fields {
            ConstructorFields::Positional(fs) => fs.is_empty(),
            ConstructorFields::Named(fs) => fs.is_empty(),
        });

        let mut functions = Vec::new();

        for (op, op_name) in &[("<", "lt"), (">", "gt"), ("<=", "le"), (">=", "ge")] {
            let mangled = format!("ord_{}__{}", op_name, type_name);

            let clauses = if is_enum {
                // Enum: compare constructor indices
                // Generate clauses for each pair, with catch-all using index comparison
                let mut cls = Vec::new();
                for (i, con_a) in constructors.iter().enumerate() {
                    for (j, con_b) in constructors.iter().enumerate() {
                        let result = match *op {
                            "<" => i < j,
                            ">" => i > j,
                            "<=" => i <= j,
                            ">=" => i >= j,
                            _ => unreachable!(),
                        };
                        cls.push(TClause {
                            patterns: vec![
                                TPattern::Constructor { name: con_a.name.clone(), args: vec![] },
                                TPattern::Constructor { name: con_b.name.clone(), args: vec![] },
                            ],
                            guards: vec![],
                            body: TExpr::new(TExprKind::Lit(TLiteral::Bool(result)), bool_ty.clone()),
                            where_binds: vec![],
                        });
                    }
                }
                cls
            } else {
                // Non-enum: for now, just compare constructor indices
                // (field comparison would need recursive Ord instances)
                let mut cls = Vec::new();
                for (i, con_a) in constructors.iter().enumerate() {
                    let field_count_a = match &con_a.fields {
                        ConstructorFields::Positional(fs) => fs.len(),
                        ConstructorFields::Named(fs) => fs.len(),
                    };
                    for (j, con_b) in constructors.iter().enumerate() {
                        let field_count_b = match &con_b.fields {
                            ConstructorFields::Positional(fs) => fs.len(),
                            ConstructorFields::Named(fs) => fs.len(),
                        };
                        let a_args: Vec<TPattern> = (0..field_count_a)
                            .map(|k| TPattern::Var(format!("_a{}", k), Ty::Unit))
                            .collect();
                        let b_args: Vec<TPattern> = (0..field_count_b)
                            .map(|k| TPattern::Var(format!("_b{}", k), Ty::Unit))
                            .collect();
                        let result = match *op {
                            "<" => i < j,
                            ">" => i > j,
                            "<=" => i <= j,
                            ">=" => i >= j,
                            _ => unreachable!(),
                        };
                        cls.push(TClause {
                            patterns: vec![
                                TPattern::Constructor { name: con_a.name.clone(), args: a_args },
                                TPattern::Constructor { name: con_b.name.clone(), args: b_args },
                            ],
                            guards: vec![],
                            body: TExpr::new(TExprKind::Lit(TLiteral::Bool(result)), bool_ty.clone()),
                            where_binds: vec![],
                        });
                    }
                }
                cls
            };

            functions.push(TFunction {
                name: mangled.clone(),
                ty: fn_ty.clone(),
                clauses,
                specialized: false,
                dict_params: vec![],
            });
        }

        // compare: return LT/EQ/GT by constructor index. This mirrors the
        // index-only ordering used for the relational operators above (same
        // constructor compares EQ; field-level ordering is not derived).
        let ordering_ty = Ty::Con("Ordering".into());
        let compare_fn_ty = Ty::fun(&[result_type.clone(), result_type.clone()], ordering_ty.clone());
        let mut compare_clauses = Vec::new();
        for (i, con_a) in constructors.iter().enumerate() {
            let fc_a = match &con_a.fields {
                ConstructorFields::Positional(fs) => fs.len(),
                ConstructorFields::Named(fs) => fs.len(),
            };
            for (j, con_b) in constructors.iter().enumerate() {
                let fc_b = match &con_b.fields {
                    ConstructorFields::Positional(fs) => fs.len(),
                    ConstructorFields::Named(fs) => fs.len(),
                };
                let ord_con = if i < j { "LT" } else if i > j { "GT" } else { "EQ" };
                let a_args: Vec<TPattern> = (0..fc_a)
                    .map(|k| TPattern::Var(format!("_a{}", k), Ty::Unit)).collect();
                let b_args: Vec<TPattern> = (0..fc_b)
                    .map(|k| TPattern::Var(format!("_b{}", k), Ty::Unit)).collect();
                compare_clauses.push(TClause {
                    patterns: vec![
                        TPattern::Constructor { name: con_a.name.clone(), args: a_args },
                        TPattern::Constructor { name: con_b.name.clone(), args: b_args },
                    ],
                    guards: vec![],
                    body: TExpr::new(TExprKind::Con(ord_con.to_string()), ordering_ty.clone()),
                    where_binds: vec![],
                });
            }
        }
        functions.push(TFunction {
            name: format!("ord_compare__{}", type_name),
            ty: compare_fn_ty,
            clauses: compare_clauses,
            specialized: false,
            dict_params: vec![],
        });

        // Register the Ord instance
        let mut method_fns = HashMap::new();
        for (op, op_name) in &[("<", "lt"), (">", "gt"), ("<=", "le"), (">=", "ge")] {
            method_fns.insert(op.to_string(), format!("ord_{}__{}", op_name, type_name));
        }
        method_fns.insert("compare".to_string(), format!("ord_compare__{}", type_name));
        self.instances.insert(
            ("Ord".to_string(), type_name.to_string()),
            InstanceInfo {
                class_name: "Ord".to_string(),
                target_type: result_type,
                method_fns,
            },
        );

        functions
    }

    fn derive_enum(
        &mut self,
        type_name: &str,
        _type_vars: &[String],
        constructors: &[Constructor],
    ) -> Vec<TFunction> {
        // Enum can only be derived for simple enums (all constructors have 0 fields)
        let is_enum = constructors.iter().all(|c| match &c.fields {
            ConstructorFields::Positional(fs) => fs.is_empty(),
            ConstructorFields::Named(fs) => fs.is_empty(),
        });
        if !is_enum {
            self.push_error_ctx(
                TypeErrorKind::Other(format!("Cannot derive Enum for '{}' — constructors must have no fields", type_name)),
                format!("data {}", type_name),
            );
            return vec![];
        }

        let result_type = Ty::Con(type_name.to_string());
        let int_ty = Ty::Con("Integer".into());
        let list_ty = Ty::List(Box::new(result_type.clone()));
        let n = constructors.len();

        let mut functions = Vec::new();

        // fromEnum_T :: T -> Integer
        let from_name = format!("fromEnum_{}", type_name);
        {
            let clauses: Vec<TClause> = constructors.iter().enumerate().map(|(i, con)| {
                TClause {
                    patterns: vec![TPattern::Constructor { name: con.name.clone(), args: vec![] }],
                    guards: vec![],
                    body: TExpr::new(TExprKind::Lit(TLiteral::Integer(i as i64)), int_ty.clone()),
                    where_binds: vec![],
                }
            }).collect();
            functions.push(TFunction {
                name: from_name.clone(),
                ty: Ty::arrow(result_type.clone(), int_ty.clone()),
                clauses,
                specialized: false,
                dict_params: vec![],
            });
        }

        // toEnum_T :: Integer -> T
        let to_name = format!("toEnum_{}", type_name);
        {
            let mut clauses: Vec<TClause> = constructors.iter().enumerate().map(|(i, con)| {
                TClause {
                    patterns: vec![TPattern::LitPat(TLiteral::Integer(i as i64))],
                    guards: vec![],
                    body: TExpr::new(TExprKind::Con(con.name.clone()), result_type.clone()),
                    where_binds: vec![],
                }
            }).collect();
            // Error clause for out of range
            clauses.push(TClause {
                patterns: vec![TPattern::Wildcard],
                guards: vec![],
                body: TExpr::new(
                    TExprKind::App(
                        Box::new(TExpr::new(TExprKind::Var("error".into()), Ty::Unit)),
                        Box::new(TExpr::new(TExprKind::Lit(TLiteral::Str(
                            format!("toEnum: index out of range for {}", type_name)
                        )), Ty::Con("String".into()))),
                    ),
                    result_type.clone(),
                ),
                where_binds: vec![],
            });
            functions.push(TFunction {
                name: to_name.clone(),
                ty: Ty::arrow(int_ty.clone(), result_type.clone()),
                clauses,
                specialized: false,
                dict_params: vec![],
            });
        }

        // succ_T :: T -> T  (toEnum (fromEnum x + 1))
        let succ_name = format!("succ_{}", type_name);
        {
            let mut clauses: Vec<TClause> = Vec::new();
            for i in 0..n.saturating_sub(1) {
                clauses.push(TClause {
                    patterns: vec![TPattern::Constructor { name: constructors[i].name.clone(), args: vec![] }],
                    guards: vec![],
                    body: TExpr::new(TExprKind::Con(constructors[i+1].name.clone()), result_type.clone()),
                    where_binds: vec![],
                });
            }
            // succ of last = error
            clauses.push(TClause {
                patterns: vec![TPattern::Wildcard],
                guards: vec![],
                body: TExpr::new(
                    TExprKind::App(
                        Box::new(TExpr::new(TExprKind::Var("error".into()), Ty::Unit)),
                        Box::new(TExpr::new(TExprKind::Lit(TLiteral::Str(
                            format!("succ: already at maxBound for {}", type_name)
                        )), Ty::Con("String".into()))),
                    ),
                    result_type.clone(),
                ),
                where_binds: vec![],
            });
            functions.push(TFunction {
                name: succ_name.clone(),
                ty: Ty::arrow(result_type.clone(), result_type.clone()),
                clauses,
                specialized: false,
                dict_params: vec![],
            });
        }

        // pred_T :: T -> T
        let pred_name = format!("pred_{}", type_name);
        {
            let mut clauses: Vec<TClause> = Vec::new();
            for i in 1..n {
                clauses.push(TClause {
                    patterns: vec![TPattern::Constructor { name: constructors[i].name.clone(), args: vec![] }],
                    guards: vec![],
                    body: TExpr::new(TExprKind::Con(constructors[i-1].name.clone()), result_type.clone()),
                    where_binds: vec![],
                });
            }
            // pred of first = error
            clauses.push(TClause {
                patterns: vec![TPattern::Wildcard],
                guards: vec![],
                body: TExpr::new(
                    TExprKind::App(
                        Box::new(TExpr::new(TExprKind::Var("error".into()), Ty::Unit)),
                        Box::new(TExpr::new(TExprKind::Lit(TLiteral::Str(
                            format!("pred: already at minBound for {}", type_name)
                        )), Ty::Con("String".into()))),
                    ),
                    result_type.clone(),
                ),
                where_binds: vec![],
            });
            functions.push(TFunction {
                name: pred_name.clone(),
                ty: Ty::arrow(result_type.clone(), result_type.clone()),
                clauses,
                specialized: false,
                dict_params: vec![],
            });
        }

        // Range functions: use direct pattern matching for enumFromTo,
        // and delegate others through fromEnum/toEnum.

        // enumFromTo_T :: T -> T -> [T]
        // Generate explicit clauses: for each constructor i, check if fromEnum a <= fromEnum b
        let enum_from_to_name = format!("enumFromTo_{}", type_name);
        {
            // enumFromTo a b = if fromEnum a > fromEnum b then []
            //                  else a : enumFromTo (succ a) b
            // Generate as: single clause with if-expression
            let a_var = TExpr::new(TExprKind::Var("_a".into()), result_type.clone());
            let b_var = TExpr::new(TExprKind::Var("_b".into()), result_type.clone());
            let from_a = TExpr::new(TExprKind::App(
                Box::new(TExpr::new(TExprKind::Var(from_name.clone()), Ty::Unit)),
                Box::new(a_var.clone()),
            ), int_ty.clone());
            let from_b = TExpr::new(TExprKind::App(
                Box::new(TExpr::new(TExprKind::Var(from_name.clone()), Ty::Unit)),
                Box::new(b_var.clone()),
            ), int_ty.clone());
            // if fromEnum a > fromEnum b then []
            // else if fromEnum a == fromEnum b then [a]
            // else a : enumFromTo (succ a) b
            let cond_gt = TExpr::new(TExprKind::InfixApp {
                op: ">".into(),
                lhs: Box::new(from_a),
                rhs: Box::new(from_b.clone()),
            }, Ty::Con("Bool".into()));
            let nil = TExpr::new(TExprKind::Lit(TLiteral::Unit), list_ty.clone());
            let from_a2 = TExpr::new(TExprKind::App(
                Box::new(TExpr::new(TExprKind::Var(from_name.clone()), Ty::Unit)),
                Box::new(a_var.clone()),
            ), int_ty.clone());
            let cond_eq = TExpr::new(TExprKind::InfixApp {
                op: "==".into(),
                lhs: Box::new(from_a2),
                rhs: Box::new(from_b),
            }, Ty::Con("Bool".into()));
            let singleton = TExpr::new(TExprKind::InfixApp {
                op: ":".into(),
                lhs: Box::new(a_var.clone()),
                rhs: Box::new(nil.clone()),
            }, list_ty.clone());
            let succ_a = TExpr::new(TExprKind::App(
                Box::new(TExpr::new(TExprKind::Var(succ_name.clone()), Ty::Unit)),
                Box::new(a_var.clone()),
            ), result_type.clone());
            let recurse = TExpr::new(TExprKind::App(
                Box::new(TExpr::new(TExprKind::App(
                    Box::new(TExpr::new(TExprKind::Var(enum_from_to_name.clone()), Ty::Unit)),
                    Box::new(succ_a),
                ), Ty::Unit)),
                Box::new(b_var.clone()),
            ), list_ty.clone());
            let cons = TExpr::new(TExprKind::InfixApp {
                op: ":".into(),
                lhs: Box::new(a_var),
                rhs: Box::new(recurse),
            }, list_ty.clone());
            let inner_if = TExpr::new(TExprKind::If {
                cond: Box::new(cond_eq),
                then_branch: Box::new(singleton),
                else_branch: Box::new(cons),
            }, list_ty.clone());
            let body = TExpr::new(TExprKind::If {
                cond: Box::new(cond_gt),
                then_branch: Box::new(nil),
                else_branch: Box::new(inner_if),
            }, list_ty.clone());
            functions.push(TFunction {
                name: enum_from_to_name.clone(),
                ty: Ty::fun(&[result_type.clone(), result_type.clone()], list_ty.clone()),
                clauses: vec![TClause {
                    patterns: vec![
                        TPattern::Var("_a".into(), result_type.clone()),
                        TPattern::Var("_b".into(), result_type.clone()),
                    ],
                    guards: vec![], body, where_binds: vec![],
                }],
                specialized: false,
                dict_params: vec![],
            });
        }

        // enumFrom_T :: T -> [T]  =>  enumFromTo a (last constructor)
        let enum_from_name = format!("enumFrom_{}", type_name);
        {
            let a_var = TExpr::new(TExprKind::Var("_a".into()), result_type.clone());
            let last_con = TExpr::new(
                TExprKind::Con(constructors.last().unwrap().name.clone()),
                result_type.clone(),
            );
            let body = TExpr::new(TExprKind::App(
                Box::new(TExpr::new(TExprKind::App(
                    Box::new(TExpr::new(TExprKind::Var(enum_from_to_name.clone()), Ty::Unit)),
                    Box::new(a_var),
                ), Ty::Unit)),
                Box::new(last_con),
            ), list_ty.clone());
            functions.push(TFunction {
                name: enum_from_name.clone(),
                ty: Ty::arrow(result_type.clone(), list_ty.clone()),
                clauses: vec![TClause {
                    patterns: vec![TPattern::Var("_a".into(), result_type.clone())],
                    guards: vec![], body, where_binds: vec![],
                }],
                specialized: false,
                dict_params: vec![],
            });
        }

        // enumFromThen_T :: T -> T -> [T]
        // Generate: enumFromThen a b = go (fromEnum a) where step = fromEnum b - fromEnum a
        //   go i = if i < 0 || i >= n then [] else toEnum i : go (i + step)
        let enum_from_then_name = format!("enumFromThen_{}", type_name);
        {
            // Simpler: just generate explicit list since enum is finite
            // enumFromThen a b: start at fromEnum a, step by (fromEnum b - fromEnum a), stop at bounds
            let a_var = TExpr::new(TExprKind::Var("_a".into()), result_type.clone());
            let b_var = TExpr::new(TExprKind::Var("_b".into()), result_type.clone());
            let from_a = TExpr::new(TExprKind::App(
                Box::new(TExpr::new(TExprKind::Var(from_name.clone()), Ty::Unit)),
                Box::new(a_var),
            ), int_ty.clone());
            let from_b = TExpr::new(TExprKind::App(
                Box::new(TExpr::new(TExprKind::Var(from_name.clone()), Ty::Unit)),
                Box::new(b_var),
            ), int_ty.clone());
            // For finite enums, enumFromThen is rarely used; generate empty list as placeholder
            // A proper implementation would need a recursive helper with bounds checking
            let _ = (from_a, from_b);
            let body = TExpr::new(TExprKind::Lit(TLiteral::Unit), list_ty.clone());
            functions.push(TFunction {
                name: enum_from_then_name.clone(),
                ty: Ty::fun(&[result_type.clone(), result_type.clone()], list_ty.clone()),
                clauses: vec![TClause {
                    patterns: vec![
                        TPattern::Var("_a".into(), result_type.clone()),
                        TPattern::Var("_b".into(), result_type.clone()),
                    ],
                    guards: vec![], body, where_binds: vec![],
                }],
                specialized: false,
                dict_params: vec![],
            });
        }

        // enumFromThenTo_T :: T -> T -> T -> [T]
        let enum_from_then_to_name = format!("enumFromThenTo_{}", type_name);
        {
            let body = TExpr::new(TExprKind::Lit(TLiteral::Unit), list_ty.clone());
            functions.push(TFunction {
                name: enum_from_then_to_name.clone(),
                ty: Ty::fun(&[result_type.clone(), result_type.clone(), result_type.clone()], list_ty.clone()),
                clauses: vec![TClause {
                    patterns: vec![
                        TPattern::Var("_a".into(), result_type.clone()),
                        TPattern::Var("_b".into(), result_type.clone()),
                        TPattern::Var("_c".into(), result_type.clone()),
                    ],
                    guards: vec![], body, where_binds: vec![],
                }],
                specialized: false,
                dict_params: vec![],
            });
        }

        // Register the Enum instance
        let mut method_fns = HashMap::new();
        method_fns.insert("toEnum".to_string(), to_name);
        method_fns.insert("fromEnum".to_string(), from_name);
        method_fns.insert("succ".to_string(), succ_name);
        method_fns.insert("pred".to_string(), pred_name);
        method_fns.insert("enumFrom".to_string(), enum_from_name);
        method_fns.insert("enumFromThen".to_string(), enum_from_then_name);
        method_fns.insert("enumFromTo".to_string(), enum_from_to_name);
        method_fns.insert("enumFromThenTo".to_string(), enum_from_then_to_name);
        self.instances.insert(
            ("Enum".to_string(), type_name.to_string()),
            InstanceInfo {
                class_name: "Enum".to_string(),
                target_type: result_type,
                method_fns,
            },
        );

        functions
    }

    fn derive_bounded(
        &mut self,
        type_name: &str,
        _type_vars: &[String],
        constructors: &[Constructor],
    ) -> Vec<TFunction> {
        let is_enum = constructors.iter().all(|c| match &c.fields {
            ConstructorFields::Positional(fs) => fs.is_empty(),
            ConstructorFields::Named(fs) => fs.is_empty(),
        });
        if !is_enum || constructors.is_empty() {
            self.push_error_ctx(
                TypeErrorKind::Other(format!("Cannot derive Bounded for '{}' — must be a simple enum", type_name)),
                format!("data {}", type_name),
            );
            return vec![];
        }

        let result_type = Ty::Con(type_name.to_string());
        let mut functions = Vec::new();

        // minBound_T :: T
        let min_name = format!("minBound_{}", type_name);
        functions.push(TFunction {
            name: min_name.clone(),
            ty: result_type.clone(),
            clauses: vec![TClause {
                patterns: vec![],
                guards: vec![],
                body: TExpr::new(TExprKind::Con(constructors.first().unwrap().name.clone()), result_type.clone()),
                where_binds: vec![],
            }],
            specialized: false,
            dict_params: vec![],
        });

        // maxBound_T :: T
        let max_name = format!("maxBound_{}", type_name);
        functions.push(TFunction {
            name: max_name.clone(),
            ty: result_type.clone(),
            clauses: vec![TClause {
                patterns: vec![],
                guards: vec![],
                body: TExpr::new(TExprKind::Con(constructors.last().unwrap().name.clone()), result_type.clone()),
                where_binds: vec![],
            }],
            specialized: false,
            dict_params: vec![],
        });

        // Register Bounded instance
        let mut method_fns = HashMap::new();
        method_fns.insert("minBound".to_string(), min_name);
        method_fns.insert("maxBound".to_string(), max_name);
        self.instances.insert(
            ("Bounded".to_string(), type_name.to_string()),
            InstanceInfo {
                class_name: "Bounded".to_string(),
                target_type: result_type,
                method_fns,
            },
        );

        functions
    }

    /// Check if a type mentions a specific type variable name
    fn ty_mentions_var(ty: &Ty, var_name: &str) -> bool {
        match ty {
            Ty::Var(tv) => tv.name == var_name,
            Ty::Con(_) | Ty::Unit | Ty::Promoted(_) | Ty::Skolem(..) => false,
            Ty::Arrow(a, b) | Ty::App(a, b) => {
                Self::ty_mentions_var(a, var_name) || Self::ty_mentions_var(b, var_name)
            }
            Ty::List(a) | Ty::IO(a) => Self::ty_mentions_var(a, var_name),
            Ty::LuaIO(_, a) => Self::ty_mentions_var(a, var_name),
            Ty::Forall(_, a) => Self::ty_mentions_var(a, var_name),
            Ty::Tuple(elems) => elems.iter().any(|e| Self::ty_mentions_var(e, var_name)),
        }
    }

    /// Generate the expression to map a field value through the functor function.
    /// For a field of type `t` in a Functor-derived constructor:
    /// - If `t` doesn't mention the last type var: pass through unchanged
    /// - If `t` IS the last type var: apply `_f x`
    /// - Otherwise (e.g. `Tree a`, `[a]`): apply `resolved_fmap _f x`
    /// The fmap_name is resolved at derive time to avoid polymorphic resolution issues.
    fn functor_map_field(&self, field_ty: &Ty, last_var: &str, var_name: &str, b_ty: &Ty, self_fmap: &str) -> TExpr {
        if !Self::ty_mentions_var(field_ty, last_var) {
            // Field doesn't mention the functor parameter → pass through
            TExpr::new(TExprKind::Var(var_name.to_string()), field_ty.clone())
        } else if let Ty::Var(tv) = field_ty {
            if tv.name == last_var {
                // Field IS the functor parameter → apply _f
                TExpr::new(
                    TExprKind::App(
                        Box::new(TExpr::new(
                            TExprKind::Var("_f".to_string()),
                            Ty::arrow(field_ty.clone(), b_ty.clone()),
                        )),
                        Box::new(TExpr::new(
                            TExprKind::Var(var_name.to_string()),
                            field_ty.clone(),
                        )),
                    ),
                    b_ty.clone(),
                )
            } else {
                TExpr::new(TExprKind::Var(var_name.to_string()), field_ty.clone())
            }
        } else {
            // Complex type mentioning the var (e.g. Tree a, [a], Maybe a)
            // Resolve the fmap function name at derive time
            let fmap_resolved = self.resolve_functor_fmap(field_ty, self_fmap);
            let a_ty = Ty::Var(TyVar { name: last_var.to_string(), id: u32::MAX });
            let fmap_f = TExpr::new(
                TExprKind::App(
                    Box::new(TExpr::new(
                        TExprKind::Var(fmap_resolved),
                        Ty::arrow(Ty::arrow(a_ty.clone(), b_ty.clone()), Ty::arrow(field_ty.clone(), field_ty.clone())),
                    )),
                    Box::new(TExpr::new(
                        TExprKind::Var("_f".to_string()),
                        Ty::arrow(a_ty, b_ty.clone()),
                    )),
                ),
                Ty::arrow(field_ty.clone(), field_ty.clone()),
            );
            TExpr::new(
                TExprKind::App(
                    Box::new(fmap_f),
                    Box::new(TExpr::new(
                        TExprKind::Var(var_name.to_string()),
                        field_ty.clone(),
                    )),
                ),
                field_ty.clone(),
            )
        }
    }

    /// Resolve the concrete fmap function for a type constructor at derive time.
    /// Extracts the outermost type constructor and looks up the Functor instance.
    fn resolve_functor_fmap(&self, ty: &Ty, self_fmap: &str) -> String {
        let tc_name = match ty {
            Ty::List(_) => Some("[]".to_string()),
            Ty::IO(_) => Some("IO".to_string()),
            Ty::App(f, _) => {
                let mut head = f.as_ref();
                loop {
                    match head {
                        Ty::Con(name) => break Some(name.clone()),
                        Ty::App(inner, _) => head = inner.as_ref(),
                        _ => break None,
                    }
                }
            }
            _ => None,
        };
        if let Some(tc) = tc_name {
            let key = ("Functor".to_string(), tc);
            if let Some(inst) = self.instances.get(&key)
                && let Some(name) = inst.method_fns.get("fmap") {
                    return name.clone();
                }
            // Self-recursive: instance not yet registered, use self_fmap
            return self_fmap.to_string();
        }
        // Fallback: use generic fmap (will need monomorphizer resolution)
        "fmap".to_string()
    }

    /// Generate `fmap` for a data type.
    /// For `data T a = C1 f1 f2 | C2 g1`, generates:
    /// `fmap_T f (C1 x0 x1) = C1 (map x0) (map x1)`
    /// where `map` applies `f` to fields mentioning the last type variable.
    fn derive_functor(
        &mut self,
        type_name: &str,
        type_vars: &[String],
        constructors: &[Constructor],
    ) -> Vec<TFunction> {
        if type_vars.is_empty() {
            self.push_error_ctx(
                TypeErrorKind::Other(format!("Cannot derive Functor for '{}' — it has no type parameters", type_name)),
                format!("data {}", type_name),
            );
            return vec![];
        }

        let last_tv_name = type_vars.last().unwrap().clone();

        let tvars: Vec<TyVar> = type_vars.iter()
            .map(|n| TyVar { name: n.clone(), id: u32::MAX })
            .collect();

        let a_tv = TyVar { name: last_tv_name.clone(), id: u32::MAX };
        let b_tv = TyVar { name: "__b".to_string(), id: u32::MAX };

        // Build T a (input type)
        let input_type = tvars.iter().fold(
            Ty::Con(type_name.to_string()),
            |acc, tv| Ty::app(acc, Ty::Var(tv.clone())),
        );

        // Build T b (output type) — last type var replaced with __b
        let output_type = tvars.iter().fold(
            Ty::Con(type_name.to_string()),
            |acc, tv| {
                if tv.name == last_tv_name {
                    Ty::app(acc, Ty::Var(b_tv.clone()))
                } else {
                    Ty::app(acc, Ty::Var(tv.clone()))
                }
            },
        );

        let f_ty = Ty::arrow(Ty::Var(a_tv.clone()), Ty::Var(b_tv.clone()));
        let fn_ty = Ty::fun(&[f_ty.clone(), input_type.clone()], output_type.clone());

        let mangled = format!("fmap_{}", type_name);

        let mut clauses = Vec::new();
        for con in constructors {
            let field_count = match &con.fields {
                ConstructorFields::Positional(fs) => fs.len(),
                ConstructorFields::Named(fs) => fs.len(),
            };

            let con_info = self.constructors.get(&con.name).cloned();
            let field_tys: Vec<Ty> = con_info.as_ref()
                .map(|ci| ci.field_types.clone())
                .unwrap_or_default();

            let param_names: Vec<String> = (0..field_count)
                .map(|i| format!("_x{}", i))
                .collect();

            // Pattern: _f (Con x0 x1 ...)
            let patterns = vec![
                TPattern::Var("_f".to_string(), f_ty.clone()),
                TPattern::Constructor {
                    name: con.name.clone(),
                    args: param_names.iter().enumerate().map(|(i, n)| {
                        let ty = field_tys.get(i).cloned().unwrap_or(Ty::Unit);
                        TPattern::Var(n.clone(), ty)
                    }).collect(),
                },
            ];

            // Body: Con (mapped_x0) (mapped_x1) ...
            let mut body = TExpr::new(
                TExprKind::Con(con.name.clone()),
                output_type.clone(),
            );

            let b_ty_val = Ty::Var(b_tv.clone());
            for (i, pname) in param_names.iter().enumerate() {
                let field_ty = field_tys.get(i).cloned().unwrap_or(Ty::Unit);
                let mapped = self.functor_map_field(&field_ty, &last_tv_name, pname, &b_ty_val, &mangled);
                body = TExpr::new(
                    TExprKind::App(Box::new(body), Box::new(mapped)),
                    output_type.clone(),
                );
            }

            clauses.push(TClause {
                patterns,
                guards: vec![],
                body,
                where_binds: vec![],
            });
        }

        // Register the Functor instance
        let mut method_fns = HashMap::new();
        method_fns.insert("fmap".to_string(), mangled.clone());
        method_fns.insert("<$>".to_string(), mangled.clone());
        self.instances.insert(
            ("Functor".to_string(), type_name.to_string()),
            InstanceInfo {
                class_name: "Functor".to_string(),
                target_type: Ty::Con(type_name.to_string()),
                method_fns,
            },
        );

        vec![TFunction {
            name: mangled,
            ty: fn_ty,
            clauses,
            specialized: false,
            dict_params: vec![],
        }]
    }

    // --- Exhaustiveness checking ---

    /// Check if a list of patterns exhaustively covers a data type.
    /// Returns a list of missing constructor names, or empty if exhaustive.
    /// When `scrutinee_ty` is provided, GADT constructors whose return type
    /// cannot unify with it are excluded (they are unreachable).
    fn check_exhaustiveness(&self, patterns: &[&Pattern], scrutinee_ty: Option<&Ty>) -> Vec<String> {
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

        // Return missing ones
        all_constructors.into_iter()
            .filter(|c| !seen_constructors.contains(c))
            .collect()
    }

    /// Recursively collect pattern info, unwrapping Paren wrappers.
    fn collect_pattern_info(
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
                if let Some(info) = self.constructors.get(name) {
                    *type_name = Some(info.type_name.clone());
                    if !seen.contains(name) {
                        seen.push(name.clone());
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

    fn check_function(&mut self, name: &str, clauses: &[Clause], declared_ty: &Ty) -> Option<TFunction> {
        self.current_fn = Some(name.to_string());
        self.wanted.clear();
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

            match self.check_clause(clause, &fresh_ty, &clause_ctx) {
                Ok((tc, clause_subst)) => {
                    tclauses.push(tc);
                    overall_subst = overall_subst.compose(&clause_subst);
                }
                Err(e) => { self.push_error_span(e, clause_ctx, clause.span); }
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
        for (class, cty) in std::mem::take(&mut self.wanted) {
            let rty = cty.apply_subst(&overall_subst);
            if !self.has_instance(&class, &rty) {
                self.push_error_span(
                    TypeErrorKind::NoInstance { class, ty: rty },
                    format!("definition of '{}'", name),
                    span,
                );
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
                    TypeErrorKind::NonExhaustive(format!(
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

    fn check_clause(&mut self, clause: &Clause, fun_ty: &Ty, _ctx: &str) -> Result<(TClause, Subst), TypeErrorKind> {
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
                _ => return Err(TypeErrorKind::Other("Too many arguments".into())),
            }
        }

        let expected_ret = remaining_ty.apply_subst(&subst);

        // Pre-register where-bound names so they're in scope for the body
        for ld in &clause.where_binds {
            if ld.patterns.is_empty() {
                let fresh = self.fresh_var("_wh");
                local_env.insert(ld.name.clone(), Scheme::mono(fresh));
            } else {
                // Local function: assign a fresh type for each parameter + return
                let mut fn_ty = self.fresh_var("_wr");
                for _ in &ld.patterns {
                    let param_ty = self.fresh_var("_wp");
                    fn_ty = Ty::arrow(param_ty, fn_ty);
                }
                local_env.insert(ld.name.clone(), Scheme::mono(fn_ty));
            }
        }

        let mut tguards = Vec::new();
        let tbody;

        if !clause.guards.is_empty() {
            for guard in &clause.guards {
                let (tcond, cond_ty, s1) = self.infer_expr(&guard.condition, &local_env)?;
                let s2 = unify(&cond_ty.apply_subst(&s1), &Ty::Con("Bool".into()))?;
                let combined = s1.compose(&s2);
                subst = subst.compose(&combined);
                let (tbody_g, body_s) = self.check_expr_typed(&guard.body, &expected_ret, &local_env)?;
                subst = subst.compose(&body_s);
                tguards.push(TGuard { condition: tcond, body: tbody_g });
            }
            tbody = TExpr::new(TExprKind::Var("undefined".into()), expected_ret);
        } else {
            let (tb, body_s) = self.check_expr_typed(&clause.body, &expected_ret, &local_env)?;
            subst = subst.compose(&body_s);
            tbody = tb;
        }

        // Type-check where bindings fully, accumulating substitutions
        let mut twhere = Vec::new();
        for ld in &clause.where_binds {
            if ld.patterns.is_empty() {
                // Simple value binding: where x = expr
                let (texpr, inferred_ty, s) = self.infer_expr(&ld.body, &local_env).unwrap_or_else(|_| {
                    (TExpr::new(TExprKind::Var("error".into()), Ty::Unit), Ty::Unit, Subst::empty())
                });
                subst = subst.compose(&s);
                // Unify with the pre-registered fresh type
                if let Some(scheme) = local_env.lookup(&ld.name)
                    && let Ok(us) = unify(&scheme.ty.apply_subst(&subst), &inferred_ty.apply_subst(&subst)) {
                        subst = subst.compose(&us);
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
                    let (tp, ps) = self.check_pattern(pat, &param_ty, &mut fn_env)
                        .unwrap_or((TPattern::Wildcard, Subst::empty()));
                    where_subst = where_subst.compose(&ps);
                    param_tys.push(param_ty.apply_subst(&where_subst));
                    tpatterns.push(tp);
                }
                let (texpr, body_ty, bs) = self.infer_expr(&ld.body, &fn_env).unwrap_or_else(|_| {
                    (TExpr::new(TExprKind::Var("error".into()), Ty::Unit), Ty::Unit, Subst::empty())
                });
                where_subst = where_subst.compose(&bs);
                // Build the inferred function type and unify with pre-registered type
                let mut inferred_fn_ty = body_ty.apply_subst(&where_subst);
                for pty in param_tys.iter().rev() {
                    inferred_fn_ty = Ty::arrow(pty.apply_subst(&where_subst), inferred_fn_ty);
                }
                if let Some(scheme) = local_env.lookup(&ld.name)
                    && let Ok(us) = unify(&scheme.ty.apply_subst(&subst), &inferred_fn_ty.apply_subst(&subst)) {
                        subst = subst.compose(&us);
                    }
                twhere.push(TLocalDef {
                    name: ld.name.clone(),
                    patterns: tpatterns.into_iter().map(|p| p.apply_subst(&where_subst)).collect(),
                    body: texpr.apply_subst(&where_subst),
                });
            }
        }

        // Apply the accumulated substitution to the entire clause
        let raw_clause = TClause {
            patterns: tpatterns,
            guards: tguards,
            body: tbody,
            where_binds: twhere,
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
    fn infer_let_group(
        &mut self,
        binds: &[LocalDef],
        env: &TypeEnv,
        mut subst: Subst,
    ) -> Result<(Vec<TLocalDef>, TypeEnv, Subst), TypeErrorKind> {
        // Pre-register fresh monomorphic vars for the whole group so bindings
        // can see themselves and each other during inference.
        let mut rec_env = env.clone();
        let mut fresh_tys: Vec<Ty> = Vec::with_capacity(binds.len());
        for bind in binds {
            let fv = self.fresh_var("_let");
            fresh_tys.push(fv.clone());
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

    fn check_pattern(
        &mut self, pattern: &Pattern, expected: &Ty, env: &mut TypeEnv,
    ) -> Result<(TPattern, Subst), TypeErrorKind> {
        match pattern {
            Pattern::Var(name) => {
                // Rank-2: if expected type is forall-quantified, bind as polymorphic scheme
                let scheme = Self::forall_to_scheme(expected);
                env.insert(name.clone(), scheme);
                Ok((TPattern::Var(name.clone(), expected.clone()), Subst::empty()))
            }
            Pattern::Wildcard => Ok((TPattern::Wildcard, Subst::empty())),
            Pattern::LitPat(lit) => {
                let lit_ty = self.literal_type(lit);
                let s = unify(expected, &lit_ty)?;
                Ok((TPattern::LitPat(Self::convert_literal(lit)), s))
            }
            Pattern::Constructor { name, args } => {
                let con_info = self.constructors.get(name)
                    .ok_or_else(|| TypeErrorKind::UnboundConstructor(name.clone()))?.clone();

                if args.len() != con_info.field_types.len() {
                    return Err(TypeErrorKind::PatternArgCount {
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

                Ok((TPattern::Constructor { name: name.clone(), args: targs }, subst))
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

    fn infer_expr(&mut self, expr: &Expr, env: &TypeEnv) -> Result<(TExpr, Ty, Subst), TypeErrorKind> {
        match expr {
            Expr::Var(name) => {
                if self.enforce_hidden && self.hidden_names.contains(name) {
                    return Err(TypeErrorKind::Other(
                        format!("'{}' is not exported by its module", name)));
                }
                if let Some(scheme) = env.lookup(name) {
                    let scheme = scheme.clone();
                    let (ty, inst_map) = self.instantiate_with_map(&scheme);
                    self.emit_use_constraints(name, &inst_map);
                    Ok((TExpr::new(TExprKind::Var(name.clone()), ty.clone()), ty, Subst::empty()))
                } else {
                    Err(TypeErrorKind::UnboundVariable(name.clone()))
                }
            }
            Expr::Con(name) => {
                if let Some(scheme) = env.lookup(name) {
                    let ty = self.instantiate(scheme);
                    Ok((TExpr::new(TExprKind::Con(name.clone()), ty.clone()), ty, Subst::empty()))
                } else {
                    Err(TypeErrorKind::UnboundConstructor(name.clone()))
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
                                return Err(TypeErrorKind::Other(format!(
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
                        TypeErrorKind::NonExhaustive(format!(
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
                    Err(TypeErrorKind::UnboundVariable(format!("({})", op)))
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
                let con_info = self.constructors.get(constructor)
                    .ok_or_else(|| TypeErrorKind::UnboundConstructor(constructor.clone()))?.clone();

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
                        None => return Err(TypeErrorKind::Other(format!(
                            "Unknown field '{}' for constructor '{}'", name, constructor
                        ))),
                    }
                }

                // Check all fields are provided
                for (i, arg) in ordered_args.iter().enumerate() {
                    if arg.is_none() {
                        return Err(TypeErrorKind::Other(format!(
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
                    .ok_or_else(|| TypeErrorKind::Other(format!(
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
                        .ok_or_else(|| TypeErrorKind::Other(format!(
                            "Unknown record field '{}'", field_name
                        )))?.clone();
                    if field_tn != type_name {
                        return Err(TypeErrorKind::Other(format!(
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
    fn is_bind_chain(&self, expr: &Expr) -> bool {
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
    fn infer_bind_chain(&mut self, expr: &Expr, env: &TypeEnv) -> Result<(TExpr, Ty, Subst), TypeErrorKind> {
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

    fn check_expr_typed(&mut self, expr: &Expr, expected: &Ty, env: &TypeEnv) -> Result<(TExpr, Subst), TypeErrorKind> {
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
    fn generate_ffi_function(&mut self, name: &str, lua_name: &str, ffi_kind: FfiKind, ty: &Ty) -> TFunction {
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
        let call_args: Vec<TExpr> = params.iter()
            .map(|(n, t)| {
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
    fn check_export_callbacks(&mut self, name: &str, ty: &Type) {
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
    fn check_callback_param(&mut self, export_name: &str, param: &Type) {
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
                self.errors.push(TypeError::in_context(
                    TypeErrorKind::Other(format!(
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
    fn validate_ffi_callbacks(&mut self, name: &str, ty: &Ty) {
        let ctx = || format!("FFI declaration of '{}'", name);
        let mut err = |msg: String| {
            self.errors.push(TypeError::in_context(TypeErrorKind::Other(msg), ctx()));
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

/// Extract FFI info from an AST type.
/// Walks through Arrow types to find LuaPure/LuaIO at the return position.
/// Returns (lua_function_name, is_io).
/// Convert an operator symbol to a name safe for mangling.
/// Classes whose list/tuple/Maybe instances mata-ll synthesizes structurally
/// (mono generates them from the element instances). Ord is excluded — there is
/// no list/tuple/Maybe ordering — as are Read and user classes.
fn structural_container_class(class: &str) -> bool {
    matches!(class, "Show" | "Eq")
}

fn op_to_name(op: &str) -> &str {
    match op {
        "<" => "lt",
        ">" => "gt",
        "<=" => "le",
        ">=" => "ge",
        "==" => "eq",
        "/=" => "ne",
        _ => op,
    }
}

#[derive(Debug, Clone, Copy)]
enum FfiKind {
    Pure,
    IO,
    Iterator,
    Try,
}

fn extract_ffi_info(ty: &Type) -> Option<(String, FfiKind)> {
    match ty {
        Type::Arrow(_, b) => extract_ffi_info(b),
        Type::LuaPure { lua_name, .. } => Some((lua_name.clone(), FfiKind::Pure)),
        Type::LuaIO { lua_name, .. } => Some((lua_name.clone(), FfiKind::IO)),
        Type::LuaIterator { lua_name, .. } => Some((lua_name.clone(), FfiKind::Iterator)),
        Type::LuaTry { lua_name, .. } => Some((lua_name.clone(), FfiKind::Try)),
        Type::Paren(inner) => extract_ffi_info(inner),
        _ => None,
    }
}

/// Compute the OutgoingCallback flags for a callback parameter type.
/// Returns (arity, marshal_args, run_io, marshal_ret). A position is marshalled
/// across the boundary only when it is a `List` or nested `Arrow`; everything
/// else (foreign types, tuples, primitives, and the opaque polymorphic state)
/// is passed raw. Type-variable results are opaque state and returned raw.
fn outgoing_cb_flags(cb_ty: &Ty) -> (usize, Vec<bool>, bool, bool) {
    let (args, ret) = cb_ty.peel_arrows();
    let marshal_args = args.iter()
        .map(|a| matches!(a, Ty::List(_) | Ty::Arrow(_, _)))
        .collect();
    let (run_io, produced) = match ret {
        Ty::IO(inner) | Ty::LuaIO(_, inner) => (true, inner.as_ref()),
        other => (false, other),
    };
    let marshal_ret = !matches!(produced, Ty::Var(_));
    (args.len(), marshal_args, run_io, marshal_ret)
}

/// Free type variables a callback carries through its argument and result
/// *values*, excluding the phantom `LuaIO s` scope variable.
fn callback_value_vars(cb_ty: &Ty) -> Vec<TyVar> {
    let (args, ret) = cb_ty.peel_arrows();
    let mut vars = Vec::new();
    for a in &args {
        for v in a.free_vars() {
            if !vars.contains(&v) { vars.push(v); }
        }
    }
    let produced = match ret {
        Ty::IO(inner) | Ty::LuaIO(_, inner) => inner.as_ref(),
        other => other,
    };
    for v in produced.free_vars() {
        if !vars.contains(&v) { vars.push(v); }
    }
    vars
}

