use std::collections::{HashMap, HashSet};
use crate::ast::*;
use crate::tir::*;
use crate::types::*;

mod solve;
mod derive;
mod infer;

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
    /// The instance's declared context, over the same type-variable names as
    /// `target_type`: `instance (Show a, Eq a) => C (T a)` stores
    /// [Show a, Eq a]. `Some(ctx)` (user-written instances) is exact — a use
    /// of the instance at a concrete type must satisfy exactly these
    /// constraints, an empty context requiring nothing. `None` (builtin and
    /// derived instances, which have no declared context) falls back to the
    /// structural rule: every type argument needs the class itself.
    pub context: Option<Vec<TyConstraint>>,
}

/// The type checker — validates types and produces typed IR
pub struct Checker {
    env: TypeEnv,
    next_var: u32,
    constructors: HashMap<String, ConInfo>,
    pub errors: Vec<Diagnostic>,
    current_fn: Option<String>,
    /// Registered typeclasses
    classes: HashMap<String, ClassInfo>,
    /// Registered instances, keyed by the *structured* head constructor of the
    /// instance's target type (see `InstHead`). One instance per (class, head):
    /// this is the canonical instance identity shared with the monomorphizer.
    instances: HashMap<(String, InstHead), InstanceInfo>,
    /// Record field accessors: field_name -> (type_name, lua_index)
    pub record_fields: HashMap<String, (String, usize)>,
    /// Type names that derive `LuaDict` (validated in `derive_luadict`): their
    /// constructor emits a name-keyed Lua table rather than a positional one.
    luadict_types: HashSet<String>,
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
    /// Types assigned to value-level binders (parameters, lambda/case/do binders,
    /// let/where bindings) seen while checking the current function. Their type
    /// variables count as *determined by the program*: a leftover class
    /// constraint over such a variable is not ambiguous even when the variable
    /// does not appear in the function's own type (it is fixed by how the binder
    /// is used). Collected here because the architecture does not otherwise scope
    /// local constraints. Cleared per function alongside `wanted`.
    binder_types: Vec<Ty>,
    /// Types that will have a FromJSON instance by the end of the module —
    /// every `deriving (FromJSON)` and every explicit `instance FromJSON T`,
    /// collected before deriving runs. Lets a derived decoder reference the
    /// decoder of a type declared later in the module (mutual recursion), whose
    /// instance is not registered yet when the earlier derive is generated.
    fromjson_types: HashSet<String>,
    /// Same prescan for ToJSON: types that will have a ToJSON instance by the
    /// end of the module, so a derived encoder can reference the encoder of a
    /// type declared later (mutual recursion).
    tojson_types: HashSet<String>,
    /// Constructor keys (into `constructors`/`env`) declared by the *local*
    /// module (decl index >= `local_decl_start`), as opposed to builtins, the
    /// Prelude and imports. Drives duplicate-vs-shadowing decisions.
    local_con_keys: HashSet<String>,
    /// Source name -> mangled key, for local constructors that shadow a
    /// non-local (builtin/Prelude/import) constructor of the same name.
    /// The non-local constructor keeps its plain name — codegen relies on the
    /// builtin names (`Just`, `Nothing`, `:`, `[]`) for the Maybe/list
    /// representations — and the local one is registered, referenced and
    /// emitted under the mangled key instead. `resolve_con_name` applies the
    /// rename to every constructor reference in local code, so the local
    /// definition consistently shadows the imported one (as in GHC, where a
    /// local declaration shadows an implicitly imported name).
    local_con_renames: HashMap<String, String>,
    /// True while the declaration currently being processed is local (its decl
    /// index is >= `local_decl_start`). Set by every decl-processing pass;
    /// determines how `resolve_con_name` resolves constructor references.
    checking_local: bool,
}

/// Suffix appended to a local constructor's key when it shadows a non-local
/// constructor of the same name. Never shown to users: diagnostics strip it,
/// derived Show prints the source name, and JSON codecs key by the source name.
pub(crate) const SHADOW_SUFFIX: &str = "__mll_shadow";

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
            luadict_types: HashSet::new(),
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
            binder_types: Vec::new(),
            fromjson_types: HashSet::new(),
            tojson_types: HashSet::new(),
            local_con_keys: HashSet::new(),
            local_con_renames: HashMap::new(),
            checking_local: false,
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
            // LuaCatch "name" (Either String T)  reduces to  Either String T
            // (the parser has already checked the `Either String a` shape).
            Type::LuaCatch { result, .. } => self.ast_type_to_ty(result),
            // LuaIOCatch "name" (Either String T)  reduces to  IO (Either String T)
            Type::LuaIOCatch { result, .. } => Ty::io(self.ast_type_to_ty(result)),
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
        // Every node that can contain a nested type is handled explicitly. A
        // catch-all `_ => ty.clone()` here is a trap: it silently drops the
        // substitution for any unlisted variant, so an alias parameter buried
        // inside e.g. a tuple leaks through unsubstituted and later collides
        // with same-named variables from other expansions.
        match ty {
            Type::Var(name) => {
                if let Some(bound) = bindings.get(name) {
                    (*bound).clone()
                } else {
                    ty.clone()
                }
            }
            Type::Con(_) | Type::Unit | Type::Promoted(_) => ty.clone(),
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
            Type::Tuple(elems) => Type::Tuple(
                elems.iter().map(|e| self.substitute_type(e, bindings)).collect(),
            ),
            Type::ScopedLuaIO { scope_var, inner } => {
                // Rename the scope variable too if the alias maps it to one.
                let new_scope = match bindings.get(scope_var) {
                    Some(Type::Var(n)) => n.clone(),
                    _ => scope_var.clone(),
                };
                Type::ScopedLuaIO {
                    scope_var: new_scope,
                    inner: Box::new(self.substitute_type(inner, bindings)),
                }
            }
            Type::Forall { var, inner } => {
                // The bound variable shadows any alias parameter of the same
                // name, so drop it from the bindings while descending.
                if bindings.contains_key(var) {
                    let mut inner_bindings = bindings.clone();
                    inner_bindings.remove(var);
                    Type::Forall {
                        var: var.clone(),
                        inner: Box::new(self.substitute_type(inner, &inner_bindings)),
                    }
                } else {
                    Type::Forall {
                        var: var.clone(),
                        inner: Box::new(self.substitute_type(inner, bindings)),
                    }
                }
            }
            Type::LuaPure { lua_name, result } => Type::LuaPure {
                lua_name: lua_name.clone(),
                result: Box::new(self.substitute_type(result, bindings)),
            },
            Type::LuaIO { lua_name, result } => Type::LuaIO {
                lua_name: lua_name.clone(),
                result: Box::new(self.substitute_type(result, bindings)),
            },
            Type::LuaIterator { lua_name, result } => Type::LuaIterator {
                lua_name: lua_name.clone(),
                result: Box::new(self.substitute_type(result, bindings)),
            },
            Type::LuaTry { lua_name, result } => Type::LuaTry {
                lua_name: lua_name.clone(),
                result: Box::new(self.substitute_type(result, bindings)),
            },
            Type::LuaCatch { lua_name, result } => Type::LuaCatch {
                lua_name: lua_name.clone(),
                result: Box::new(self.substitute_type(result, bindings)),
            },
            Type::LuaIOCatch { lua_name, result } => Type::LuaIOCatch {
                lua_name: lua_name.clone(),
                result: Box::new(self.substitute_type(result, bindings)),
            },
            Type::Constrained { constraints, ty } => Type::Constrained {
                constraints: constraints.iter().map(|c| Constraint {
                    class_name: c.class_name.clone(),
                    type_arg: self.substitute_type(&c.type_arg, bindings),
                }).collect(),
                ty: Box::new(self.substitute_type(ty, bindings)),
            },
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

    fn push_error_ctx(&mut self, kind: DiagnosticKind, ctx: String) {
        self.errors.push(Diagnostic { kind, context: Some(ctx), span: None, notes: Vec::new() });
    }

    fn push_error_span(&mut self, kind: DiagnosticKind, ctx: String, span: Span) {
        self.errors.push(Diagnostic { kind, context: Some(ctx), span: Some(span), notes: Vec::new() });
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
            ("hmToList", vec![a.clone(), b.clone()], Ty::arrow(hm_kv.clone(), Ty::list(Ty::Tuple(vec![ta.clone(), tb.clone()])))),
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
            self.register_instance(InstanceInfo {
                class_name: "Functor".to_string(),
                target_type: Ty::Con(tc_name.to_string()),
                method_fns,
                context: None,
            });
        }
        {
            let mut method_fns = HashMap::new();
            method_fns.insert("fmap".to_string(), "map".to_string());
            method_fns.insert("<$>".to_string(), "map".to_string());
            self.register_instance(InstanceInfo {
                class_name: "Functor".to_string(),
                target_type: Ty::Con("[]".to_string()),
                method_fns,
                context: None,
            });
        }
        for tc_name in &["Maybe", "Either"] {
            let mut method_fns = HashMap::new();
            method_fns.insert("fmap".to_string(), format!("fmap_{}", tc_name));
            method_fns.insert("<$>".to_string(), format!("fmap_{}", tc_name));
            self.register_instance(InstanceInfo {
                class_name: "Functor".to_string(),
                target_type: Ty::Con(tc_name.to_string()),
                method_fns,
                context: None,
            });
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
            self.register_instance(InstanceInfo {
                class_name: "Applicative".to_string(),
                target_type: Ty::Con(tc_name.to_string()),
                method_fns,
                context: None,
            });
        }
        {
            let mut method_fns = HashMap::new();
            method_fns.insert("pure".to_string(), "pure_List".to_string());
            method_fns.insert("<*>".to_string(), "ap_List".to_string());
            self.register_instance(InstanceInfo {
                class_name: "Applicative".to_string(),
                target_type: Ty::Con("[]".to_string()),
                method_fns,
                context: None,
            });
        }
        {
            let mut method_fns = HashMap::new();
            method_fns.insert("pure".to_string(), "pure_Maybe".to_string());
            method_fns.insert("<*>".to_string(), "ap_Maybe".to_string());
            self.register_instance(InstanceInfo {
                class_name: "Applicative".to_string(),
                target_type: Ty::Con("Maybe".to_string()),
                method_fns,
                context: None,
            });
        }
        {
            let mut method_fns = HashMap::new();
            method_fns.insert("pure".to_string(), "pure_Either".to_string());
            method_fns.insert("<*>".to_string(), "ap_Either".to_string());
            self.register_instance(InstanceInfo {
                class_name: "Applicative".to_string(),
                target_type: Ty::Con("Either".to_string()),
                method_fns,
                context: None,
            });
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
            self.register_instance(InstanceInfo {
                class_name: "Monad".to_string(),
                target_type: Ty::Con(monad_name.to_string()),
                method_fns,
                context: None,
            });
        }

        // Monad instance for [] (lists)
        {
            let mut method_fns = HashMap::new();
            method_fns.insert(">>=".to_string(), "bind_List".to_string());
            method_fns.insert(">>".to_string(), "then_List".to_string());
            method_fns.insert("return".to_string(), "pure_List".to_string());
            self.register_instance(InstanceInfo {
                class_name: "Monad".to_string(),
                target_type: Ty::Con("[]".to_string()),
                method_fns,
                context: None,
            });
        }

        // Monad instance for Maybe
        {
            let mut method_fns = HashMap::new();
            method_fns.insert(">>=".to_string(), "bind_Maybe".to_string());
            method_fns.insert(">>".to_string(), "then_Maybe".to_string());
            method_fns.insert("return".to_string(), "pure_Maybe".to_string());
            self.register_instance(InstanceInfo {
                class_name: "Monad".to_string(),
                target_type: Ty::Con("Maybe".to_string()),
                method_fns,
                context: None,
            });
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
            self.register_instance(InstanceInfo {
                class_name: "Enum".to_string(),
                target_type: Ty::Con("Integer".to_string()),
                method_fns,
                context: None,
            });
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
            self.register_instance(InstanceInfo {
                class_name: "Read".to_string(),
                target_type: Ty::Con(type_name.to_string()),
                method_fns,
                context: None,
            });
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
            self.register_instance(InstanceInfo {
                class_name: "Eq".to_string(),
                target_type: target,
                method_fns,
                context: None,
            });
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
            self.register_instance(InstanceInfo {
                class_name: "Ord".to_string(),
                target_type: target,
                method_fns,
                context: None,
            });
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
            self.register_instance(InstanceInfo {
                class_name: "Semigroup".to_string(),
                target_type: Ty::Con("String".into()),
                method_fns,
                context: None,
            });
            // []: <> is list append (same as ++)
            let mut method_fns = HashMap::new();
            method_fns.insert("<>".to_string(), "semigroup_List".to_string());
            self.register_instance(InstanceInfo {
                class_name: "Semigroup".to_string(),
                target_type: Ty::list(ta.clone()),
                method_fns,
                context: None,
            });
        }

        // Show instances for base types and parameterized types
        for type_name in &["Integer", "Number", "String", "Bool", "[]", "Maybe", "ByteString"] {
            let target = Ty::Con(type_name.to_string());
            let mangled = format!("show_{}", type_name);
            let mut method_fns = HashMap::new();
            method_fns.insert("show".to_string(), mangled);
            self.register_instance(InstanceInfo {
                class_name: "Show".to_string(),
                target_type: target,
                method_fns,
                context: None,
            });
        }

        // `()` is a base type like any other and carries the GHC base
        // instances Show/Eq/Ord. It is registered separately from the loops
        // above because its instance key is the type string "()" (matching
        // `format!("{}", Ty::Unit)`) while its mangled runtime names must be
        // identifier-safe (`show_Unit`, not `show_()`). Runtime rep is nil,
        // so eq/ord are trivial (nil == nil; compare is always EQ).
        {
            let mut method_fns = HashMap::new();
            method_fns.insert("show".to_string(), "show_Unit".to_string());
            self.register_instance(InstanceInfo {
                class_name: "Show".to_string(),
                target_type: Ty::Unit,
                method_fns,
                context: None,
            });
            let mut method_fns = HashMap::new();
            method_fns.insert("==".to_string(), "eq_Unit".to_string());
            self.register_instance(InstanceInfo {
                class_name: "Eq".to_string(),
                target_type: Ty::Unit,
                method_fns,
                context: None,
            });
            let mut method_fns = HashMap::new();
            for op in &["<", ">", "<=", ">="] {
                method_fns.insert(op.to_string(), format!("ord_{}__Unit", op_to_name(op)));
            }
            method_fns.insert("compare".to_string(), "ord_compare__Unit".to_string());
            self.register_instance(InstanceInfo {
                class_name: "Ord".to_string(),
                target_type: Ty::Unit,
                method_fns,
                context: None,
            });
        }
    }

    fn init_kinds(&mut self) {
        // Base types: kind Type
        // LuaUserData is the opaque builtin for Lua userdata values crossing
        // the FFI boundary (e.g. lib/LIO.mll's FileHandle wraps one); it must
        // be registered here like every other builtin so that references to
        // it pass the unknown-type check.
        for name in &["Integer", "Number", "String", "Bool", "()", "ByteString", "LuaUserData"] {
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
        self.register_instance(InstanceInfo {
            class_name: "Show".to_string(),
            target_type: Ty::Con("HashMap".into()),
            method_fns: {
                let mut m = HashMap::new();
                m.insert("show".to_string(), "show_HashMap".to_string());
                m
            },
            context: None,
        });

        // Int as alias for Integer (Lua has no fixed-width integers)
        self.type_aliases.insert("Int".to_string(),
            (vec![], Type::Con("Integer".to_string())));
    }

    /// Get the kind of a type constructor, or infer Type for unknowns.
    pub fn kind_of(&self, name: &str) -> Kind {
        self.kinds.get(name).cloned().unwrap_or(Kind::Type)
    }

    /// Check that a type-constructor reference names a type that exists.
    ///
    /// Everything a type reference can legitimately name is registered by the
    /// time declaration types are validated: builtins (init_kinds), data types
    /// and newtypes (pass 1), type aliases and type families (pass 2). A name
    /// found in none of those tables cannot be given any meaning — if it were
    /// let through it would flow downstream as an opaque type and surface as a
    /// misleading error later (e.g. "no Show instance for 'Boolean'" when the
    /// real problem is that 'Boolean' does not exist).
    fn check_con_defined(&mut self, name: &str, ctx: &str) {
        if self.kinds.contains_key(name)
            || self.type_aliases.contains_key(name)
            || self.type_families.contains_key(name)
            // Type-level string literals (LuaImport names etc.) parse as
            // `Con "\"…\""`; they are names, not type constructors.
            || name.starts_with('"')
        {
            return;
        }
        if self.classes.contains_key(name) {
            self.push_error_ctx(
                DiagnosticKind::Other(format!(
                    "'{}' is a typeclass, not a type: a class describes operations a type supports and cannot itself stand where a type is expected",
                    name
                )),
                ctx.to_string(),
            );
        } else {
            self.push_error_ctx(DiagnosticKind::UnknownType(name.to_string()), ctx.to_string());
        }
    }

    /// Infer the kind of an AST type expression and report errors, including
    /// references to type names that were never defined. `ctx` names the
    /// declaration being checked for the error message.
    fn check_type_kind(&mut self, ty: &Type, ctx: &str) -> Kind {
        match ty {
            Type::Con(name) => {
                self.check_con_defined(name, ctx);
                self.kind_of(name)
            }
            Type::Var(_) => Kind::Type, // type variables are assumed to be Type
            Type::Arrow(a, b) => {
                let ka = self.check_type_kind(a, ctx);
                let kb = self.check_type_kind(b, ctx);
                if ka != Kind::Type {
                    self.push_error_ctx(
                        DiagnosticKind::Other(format!("Kind error: argument of '->' has kind {}, expected Type", ka)),
                        ctx.to_string(),
                    );
                }
                if kb != Kind::Type {
                    self.push_error_ctx(
                        DiagnosticKind::Other(format!("Kind error: result of '->' has kind {}, expected Type", kb)),
                        ctx.to_string(),
                    );
                }
                Kind::Type
            }
            Type::App(f, a) => {
                let kf = self.check_type_kind(f, ctx);
                let _ka = self.check_type_kind(a, ctx);
                match kf {
                    Kind::Arrow(_, result) => *result,
                    Kind::Type => {
                        // Applying a Type-kinded thing — this is a kind error
                        // but only report if it's a known constructor
                        if let Type::Con(name) = f.as_ref()
                            && self.kinds.contains_key(name) {
                                self.push_error_ctx(
                                    DiagnosticKind::Other(format!(
                                        "Kind error: '{}' has kind Type and cannot be applied to an argument",
                                        name
                                    )),
                                    ctx.to_string(),
                                );
                            }
                        Kind::Type
                    }
                    _ => Kind::Type,
                }
            }
            Type::List(a) | Type::IO(a) => {
                self.check_type_kind(a, ctx);
                Kind::Type
            }
            Type::Unit => Kind::Type,
            Type::Tuple(elems) => {
                for e in elems {
                    self.check_type_kind(e, ctx);
                }
                Kind::Type
            }
            Type::ScopedLuaIO { inner, .. } => {
                self.check_type_kind(inner, ctx);
                Kind::Type
            }
            Type::LuaPure { result, .. }
            | Type::LuaIO { result, .. }
            | Type::LuaIterator { result, .. }
            | Type::LuaTry { result, .. }
            | Type::LuaCatch { result, .. }
            | Type::LuaIOCatch { result, .. } => {
                self.check_type_kind(result, ctx);
                Kind::Type
            }
            Type::Paren(inner) => self.check_type_kind(inner, ctx),
            Type::Forall { inner, .. } => self.check_type_kind(inner, ctx),
            Type::Constrained { ty, .. } => self.check_type_kind(ty, ctx),
            Type::Promoted(name) => {
                let key = format!("'{}", name);
                if let Some(kind) = self.kinds.get(&key).cloned() {
                    kind
                } else {
                    self.push_error_ctx(
                        DiagnosticKind::Other(format!("Unknown promoted constructor '{}'", name)),
                        ctx.to_string(),
                    );
                    Kind::Type
                }
            }
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

    /// Resolve a constructor reference (from a pattern, expression, derived
    /// instance or data-def conversion) to the key it is registered under.
    /// In local code a name that shadows a non-local constructor resolves to
    /// the local (mangled) key; everywhere else — non-local code, or names the
    /// local module does not redefine — the source name is the key. This is
    /// the single point that keeps the typechecker, the derived instances and
    /// codegen's tag table all agreeing on which constructor a name means.
    pub(super) fn resolve_con_name<'a>(&'a self, name: &'a str) -> &'a str {
        if self.checking_local
            && let Some(key) = self.local_con_renames.get(name) {
                return key;
            }
        name
    }

    /// Claim `con_name` (constructor `variant_index` of `total_variants` of
    /// `type_name`, being registered by the declaration currently processed —
    /// local iff `checking_local`) in the flat constructor namespace. Returns
    /// the key to register it under, or `None` (with a diagnostic pushed) when
    /// the name genuinely duplicates an existing same-scope constructor.
    ///
    /// Policy, mirroring GHC's scoping as closely as the flattened-namespace
    /// architecture allows:
    /// - two constructors of the same name in the same scope: error (GHC:
    ///   "Multiple declarations of ...") — except that a *non-local*
    ///   re-registration of the identical constructor (same type, same
    ///   position) is benign and accepted: a diamond import merges the same
    ///   module's declarations twice;
    /// - a local constructor whose name a builtin/Prelude/import already uses:
    ///   the local one shadows it (GHC: a local definition shadows an
    ///   implicitly imported name) — it gets a mangled key, and
    ///   `resolve_con_name` routes local references to it;
    /// - two non-local constructors (an import against the Prelude or another
    ///   import): error, consistent with the loud import-collision policy for
    ///   functions in `check_import_collisions`.
    ///
    /// Silently keeping both under one name is never an option: the
    /// typechecker's map is last-writer-wins but codegen's tag table scans
    /// first-match, so the two phases would disagree on the constructor's tag
    /// and the program would misbehave at runtime with no diagnostic.
    fn claim_constructor_name(
        &mut self,
        con_name: &str,
        type_name: &str,
        variant_index: usize,
        total_variants: usize,
    ) -> Option<String> {
        let duplicate_of = |checker: &mut Self, other_type: &str, other_is_local: bool| {
            let where_other = if other_is_local { "in this module" } else { "by the Prelude or an import" };
            let note = if other_is_local {
                "GHC rejects this too (\"Multiple declarations\"): all constructors declared in one module share a single namespace, so a use of the name would be ambiguous. Rename one of the constructors.".to_string()
            } else {
                format!(
                    "mata-ll merges the Prelude and every import into a single namespace, so two imported types cannot both declare a constructor named '{}'. (A constructor declared in your own file may shadow an imported one — this error is only for two imported/Prelude declarations.) Rename the constructor in one of the imported modules.",
                    con_name,
                )
            };
            checker.push_error_ctx(
                DiagnosticKind::Other(format!(
                    "Duplicate data constructor '{}': it is already declared by '{}' {}\nnote: {}",
                    con_name, other_type, where_other, note,
                )),
                format!("data {}", type_name),
            );
        };

        if self.checking_local {
            // A previous local declaration already shadows a non-local `con_name`.
            if let Some(key) = self.local_con_renames.get(con_name).cloned() {
                let other = self.constructors.get(&key).map(|c| c.type_name.clone()).unwrap_or_default();
                duplicate_of(self, &other, true);
                return None;
            }
            let existing_type = self.constructors.get(con_name).map(|c| c.type_name.clone());
            match existing_type {
                Some(other) if self.local_con_keys.contains(con_name) => {
                    duplicate_of(self, &other, true);
                    None
                }
                Some(_) => {
                    // Shadow the non-local constructor: the local one lives
                    // under a mangled key, the non-local keeps its name.
                    let key = format!("{}{}", con_name, SHADOW_SUFFIX);
                    if let Some(clash) = self.constructors.get(&key).map(|c| c.type_name.clone()) {
                        // Pathological: a constructor was literally named like
                        // the mangled key. Refuse loudly rather than alias.
                        let clash_local = self.local_con_keys.contains(&key);
                        duplicate_of(self, &clash, clash_local);
                        return None;
                    }
                    self.local_con_renames.insert(con_name.to_string(), key.clone());
                    self.local_con_keys.insert(key.clone());
                    Some(key)
                }
                None => {
                    self.local_con_keys.insert(con_name.to_string());
                    Some(con_name.to_string())
                }
            }
        } else if let Some(existing) = self.constructors.get(con_name) {
            if existing.type_name == type_name
                && existing.variant_index == variant_index
                && existing.total_variants == total_variants {
                // The identical constructor registered again: a diamond import
                // merges the same module's declarations more than once.
                Some(con_name.to_string())
            } else {
                let other = existing.type_name.clone();
                duplicate_of(self, &other, false);
                None
            }
        } else {
            Some(con_name.to_string())
        }
    }

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
            // Claim the name in the flat constructor namespace: `con_key` is
            // the key the constructor is registered (and code-generated)
            // under — the plain name, or a mangled key when a local
            // constructor shadows a Prelude/import one. A genuine same-scope
            // duplicate was reported by the claim; skip it so it cannot
            // clobber the existing registration with conflicting tags.
            let Some(con_key) = self.claim_constructor_name(&con.name, name, i + 1, constructors.len()) else { continue };

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
                    ConstructorFields::Named(fields) => fields.iter().map(|f| self.ast_type_to_ty(&f.ty)).collect(),
                };
                (fts, result_type.clone())
            };

            let con_type = if field_types.is_empty() { con_result_type.clone() } else { Ty::fun(&field_types, con_result_type.clone()) };

            // Constructor scheme includes both universal (data type) and existential vars
            let mut all_scheme_vars = tvars.clone();
            all_scheme_vars.extend(ex_tvars.clone());

            self.constructors.insert(con_key.clone(), ConInfo {
                type_name: name.to_string(), variant_index: i + 1, total_variants: constructors.len(),
                field_types: field_types.clone(), type_vars: tvars.clone(), result_type: con_result_type.clone(),
                existential_vars: ex_tvars,
            });
            self.env.insert(con_key, Scheme { vars: all_scheme_vars, ty: con_type });

            // Register record field accessors
            if let ConstructorFields::Named(fields) = &con.fields {
                for (fi, field) in fields.iter().enumerate() {
                    let field_ty = self.ast_type_to_ty(&field.ty);
                    // accessor :: DataType -> FieldType
                    // Note: the accessor is always named after the Haskell field
                    // name; an `as "key"` rename affects only the LuaDict table key.
                    let accessor_ty = Ty::arrow(result_type.clone(), field_ty);
                    self.env.insert(field.name.clone(), Scheme {
                        vars: tvars.clone(),
                        ty: accessor_ty,
                    });
                    // Store field index for codegen
                    let index = if constructors.len() == 1 { fi + 1 } else { fi + 2 };
                    self.record_fields.insert(field.name.clone(), (name.to_string(), index));
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
        // The constructor shares the flat namespace with data constructors,
        // so it goes through the same duplicate/shadowing claim.
        let Some(con_key) = self.claim_constructor_name(name, name, 1, 1) else { return };
        self.constructors.insert(con_key.clone(), ConInfo {
            type_name: name.to_string(),
            variant_index: 1,
            total_variants: 1,
            field_types: vec![inner_ty.clone()],
            type_vars: tvars.clone(),
            result_type: result_type.clone(),
            existential_vars: vec![],
        });
        self.env.insert(con_key, Scheme {
            vars: tvars,
            ty: Ty::arrow(inner_ty, result_type),
        });
    }

    fn convert_data_def(&mut self, name: &str, type_vars: &[String], constructors: &[Constructor]) -> TDataDef {
        TDataDef {
            name: name.to_string(),
            type_vars: type_vars.to_vec(),
            is_luadict: self.luadict_types.contains(name),
            constructors: constructors.iter().map(|c| {
                TConstructor {
                    // The TIR (and thus codegen) name is the registered key:
                    // mangled when this local constructor shadows a non-local
                    // one, the plain name otherwise. Every reference site
                    // resolves the same way, so tags stay consistent.
                    name: self.resolve_con_name(&c.name).to_string(),
                    external_name: c.external_name.clone(),
                    fields: if c.gadt_type.is_some() {
                        // GADT: field types come from the registered ConInfo
                        let con_info = self.constructors.get(self.resolve_con_name(&c.name)).unwrap();
                        TConFields::Positional(con_info.field_types.clone())
                    } else {
                        match &c.fields {
                            ConstructorFields::Positional(types) =>
                                TConFields::Positional(types.iter().map(|t| self.ast_type_to_ty(t)).collect()),
                            ConstructorFields::Named(fields) =>
                                TConFields::Named(fields.iter().map(|f| TRecordField {
                                    name: f.name.clone(),
                                    external_key: f.external_key.clone(),
                                    ty: self.ast_type_to_ty(&f.ty),
                                }).collect()),
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
        for (decl_idx, decl) in module.decls.iter().enumerate() {
            // Constructor names claimed by a local declaration shadow
            // non-local (Prelude/import) ones; same-scope duplicates error.
            self.checking_local = decl_idx >= self.local_decl_start;
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
        self.checking_local = false;

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

        // Pass 2b: validate every type reference in declarations. All names a
        // type reference can legitimately use are registered by now (builtins,
        // data/newtypes from pass 1, aliases and families from pass 2), so a
        // name in none of those tables is undefined and is rejected here.
        // This must run before deriving (pass 4a) and instance checking so an
        // undefined type is reported as "unknown type" instead of surfacing
        // later as a misleading missing-instance error.
        for decl in &module.decls {
            match decl {
                Decl::DataDef { name, constructors, .. } => {
                    let ctx = format!("the definition of data type '{}'", name);
                    for con in constructors {
                        if let Some(gadt_ty) = &con.gadt_type {
                            self.check_type_kind(gadt_ty, &ctx);
                            continue;
                        }
                        match &con.fields {
                            ConstructorFields::Positional(tys) => {
                                for t in tys {
                                    self.check_type_kind(t, &ctx);
                                }
                            }
                            ConstructorFields::Named(fields) => {
                                for field in fields {
                                    self.check_type_kind(&field.ty, &ctx);
                                }
                            }
                        }
                    }
                }
                Decl::NewtypeDef { name, inner, .. } => {
                    self.check_type_kind(inner, &format!("the definition of newtype '{}'", name));
                }
                Decl::TypeAlias { name, ty, .. } => {
                    self.check_type_kind(ty, &format!("the definition of type alias '{}'", name));
                }
                Decl::ClassDecl { name, methods, .. } => {
                    for method in methods {
                        self.check_type_kind(
                            &method.ty,
                            &format!("the signature of method '{}' in class '{}'", method.name, name),
                        );
                    }
                }
                Decl::InstanceDecl { class_name, target_type, .. } => {
                    self.check_type_kind(
                        target_type,
                        &format!("the instance declaration 'instance {} …'", class_name),
                    );
                }
                Decl::TypeFamily { name, equations } => {
                    let ctx = format!("the definition of type family '{}'", name);
                    for eq in equations {
                        for arg in &eq.args {
                            self.check_type_kind(arg, &ctx);
                        }
                        self.check_type_kind(&eq.result, &ctx);
                    }
                }
                _ => {}
            }
        }

        // Pass 3: collect type signatures and FFI info
        let mut sigs: HashMap<String, Ty> = HashMap::new();
        let mut ffi_info: HashMap<String, (String, FfiKind)> = HashMap::new();
        for decl in &module.decls {
            if let Decl::TypeSig { name, ty } = decl {
                // Kind-check the type signature (also rejects unknown type names)
                self.check_type_kind(ty, &format!("the type signature for '{}'", name));
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

        // Pre-register all function signatures BEFORE deriving and instance
        // checking. Mutually recursive functions need to see each other, and
        // instance method bodies (pass 4b) are type-checked before function
        // definitions (pass 6), so without this an instance method could not
        // call any top-level function — e.g. a FromJSON instance written in
        // terms of the JSON module's decoder combinators.
        for (name, ty) in &sigs {
            let scheme = self.generalize(&self.env.clone(), ty);
            self.env.insert(name.clone(), scheme);
        }

        // Collect the sets of types that will carry a ToJSON / FromJSON
        // instance once the whole module is processed (see `fromjson_types`
        // and `tojson_types`).
        self.fromjson_types.clear();
        self.tojson_types.clear();
        for decl in &module.decls {
            match decl {
                Decl::DataDef { name, deriving, .. } => {
                    if deriving.iter().any(|c| c == "FromJSON") {
                        self.fromjson_types.insert(name.clone());
                    }
                    if deriving.iter().any(|c| c == "ToJSON") {
                        self.tojson_types.insert(name.clone());
                    }
                }
                Decl::InstanceDecl { class_name, target_type, .. }
                    if class_name == "FromJSON" || class_name == "ToJSON" => {
                    if let Some(head) = Self::type_head_name(target_type) {
                        if class_name == "FromJSON" {
                            self.fromjson_types.insert(head);
                        } else {
                            self.tojson_types.insert(head);
                        }
                    }
                }
                _ => {}
            }
        }

        // Pass 4a: process deriving clauses first (so derived instances
        // are available when checking explicit instances with superclass constraints)
        let mut instance_fns = Vec::new();
        for (decl_idx, decl) in module.decls.iter().enumerate() {
            // Derived instances build TIR directly; constructor references in
            // them must resolve in the scope of the data type they derive for
            // (a local shadowing constructor resolves to its mangled key).
            self.checking_local = decl_idx >= self.local_decl_start;
            if let Decl::DataDef { name, type_vars, constructors, deriving } = decl {
                // A field-key rename (`field as "key" :: T`) gives the field
                // one shared EXTERNAL name: the key in the runtime Lua table
                // that `deriving (LuaDict)` lays the record out as, and the
                // JSON object key of a derived ToJSON/FromJSON codec. Without
                // any of those derivings the record never crosses a boundary
                // that keys by name, so the rename would be silently
                // meaningless. Reject it instead.
                if !deriving.iter().any(|c| c == "LuaDict" || c == "ToJSON" || c == "FromJSON") {
                    for con in constructors {
                        if let ConstructorFields::Named(fields) = &con.fields {
                            for field in fields {
                                if let Some(key) = &field.external_key {
                                    self.push_error_ctx(
                                        DiagnosticKind::Other(format!(
                                            "Field '{}' of '{}' is renamed with `as \"{}\"`, but '{}' derives none of LuaDict, ToJSON or FromJSON: the rename only changes the field's external name — the key in the runtime Lua table of a LuaDict record and the JSON object key of a derived ToJSON/FromJSON codec — and without one of those derivings there is nothing the rename could apply to\nnote: `as` field renaming is a mata-ll extension with no GHC equivalent; add `deriving (LuaDict)`, `deriving (ToJSON)` or `deriving (FromJSON)`, or drop the rename.",
                                            field.name, name, key, name,
                                        )),
                                        format!("data {}", name),
                                    );
                                }
                            }
                        }
                    }
                }
                // A constructor rename (`Con field-types as "name"`) gives
                // the constructor an external TAG. Two derivings give that tag
                // a meaning: a ToJSON/FromJSON codec writes and reads it to
                // tell the constructors of a sum type apart at the JSON
                // boundary; and `deriving (LuaDict)` on an all-nullary sum type
                // makes the tag the constructor's runtime string at the Lua
                // boundary. Absent both, nothing names the constructor
                // externally (an ADT with fields is a positional integer tag,
                // not a name), so the rename would be silently meaningless —
                // reject it instead.
                let is_luadict_enum = deriving.iter().any(|c| c == "LuaDict")
                    && constructors.iter().all(|c| match &c.fields {
                        ConstructorFields::Positional(fs) => fs.is_empty(),
                        ConstructorFields::Named(fs) => fs.is_empty(),
                    });
                if !deriving.iter().any(|c| c == "ToJSON" || c == "FromJSON") && !is_luadict_enum {
                    for con in constructors {
                        if let Some(ext) = &con.external_name {
                            self.push_error_ctx(
                                DiagnosticKind::Other(format!(
                                    "Constructor '{}' of '{}' is renamed with `as \"{}\"`, but '{}' derives neither ToJSON nor FromJSON, nor is it an all-nullary type deriving LuaDict: the rename only changes the constructor's external tag — the string a derived JSON codec, or a LuaDict string-enum, uses to tell the constructors apart — and without one of those there is nothing the rename could apply to\nnote: `as` constructor renaming is a mata-ll extension with no GHC equivalent; on a type with fields it never affects the Lua side (at the Lua boundary such a constructor is a positional integer tag, not a name). Add `deriving (ToJSON)` or `deriving (FromJSON)`, or make an all-nullary sum type `deriving (LuaDict)` so its constructors become Lua strings, or drop the rename.",
                                    con.name, name, ext, name,
                                )),
                                format!("data {}", name),
                            );
                        }
                    }
                }
                for class in deriving {
                    let derived = self.derive_instance(class, name, type_vars, constructors);
                    instance_fns.extend(derived);
                }
            }
        }

        self.checking_local = false;

        // Pass 4b: register and check explicit instance declarations.
        // Registration runs over ALL instance decls before any method body is
        // checked: instances are globally visible, so a body may use its own
        // instance (`show l` on the sub-`Tree a` inside `instance Show a =>
        // Show (Tree a)`) or one declared later in the module.
        for decl in module.decls.iter() {
            if let Decl::InstanceDecl { class_name, target_type, context, methods } = decl {
                self.preregister_instance(class_name, target_type, context, methods);
            }
        }
        for (decl_idx, decl) in module.decls.iter().enumerate() {
            // Instance method bodies reference constructors; resolve them in
            // the scope of the declaring module (shadowing, see pass 1).
            self.checking_local = decl_idx >= self.local_decl_start;
            if let Decl::InstanceDecl { class_name, target_type, context, methods } = decl {
                let ifns = self.check_instance(class_name, target_type, context, methods);
                instance_fns.extend(ifns);
            }
        }
        self.checking_local = false;

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
                DiagnosticKind::Other(format!(
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
            // Constructor references in local bodies resolve to local
            // (possibly shadowing) constructors; non-local bodies keep seeing
            // the constructors of their own scope.
            self.checking_local = decl_idx >= self.local_decl_start;
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
                            DiagnosticKind::Other(format!("Missing type signature for '{}'", name)),
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

        self.checking_local = false;

        // The newtype list carries the *registered* constructor keys: a local
        // newtype whose constructor shadows a non-local constructor is known
        // to codegen (which elides it as an identity function) only under its
        // mangled key.
        let newtypes: Vec<String> = module.decls.iter().enumerate().filter_map(|(decl_idx, d)| {
            if let Decl::NewtypeDef { name, .. } = d {
                if decl_idx >= self.local_decl_start
                    && let Some(key) = self.local_con_renames.get(name) {
                        return Some(key.clone());
                    }
                Some(name.clone())
            } else { None }
        }).collect();

        TModule { data_defs, functions, instance_fns, has_main, exports, record_accessors, newtypes }
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

/// The monad-hierarchy classes, which mata-ll resolves structurally for IO
/// rather than by dictionary passing. A leftover constraint from one of these
/// (e.g. the `Applicative f` of a `when`/`return` in an IO do-block) is IO by
/// construction, not a genuine value-level ambiguity, so it is not flagged.
fn is_structural_monad_class(class: &str) -> bool {
    matches!(class, "Functor" | "Applicative" | "Monad")
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
    /// `LuaCatch`: pure call under `pcall`, result `Either String a`.
    Catch,
    /// `LuaIOCatch`: IO action under `pcall`, result `IO (Either String a)`.
    IOCatch,
}

fn extract_ffi_info(ty: &Type) -> Option<(String, FfiKind)> {
    match ty {
        Type::Arrow(_, b) => extract_ffi_info(b),
        Type::LuaPure { lua_name, .. } => Some((lua_name.clone(), FfiKind::Pure)),
        Type::LuaIO { lua_name, .. } => Some((lua_name.clone(), FfiKind::IO)),
        Type::LuaIterator { lua_name, .. } => Some((lua_name.clone(), FfiKind::Iterator)),
        Type::LuaTry { lua_name, .. } => Some((lua_name.clone(), FfiKind::Try)),
        Type::LuaCatch { lua_name, .. } => Some((lua_name.clone(), FfiKind::Catch)),
        Type::LuaIOCatch { lua_name, .. } => Some((lua_name.clone(), FfiKind::IOCatch)),
        Type::Paren(inner) => extract_ffi_info(inner),
        _ => None,
    }
}

/// Is `ty` a saturated `Maybe a`? Used to spot optional FFI parameters in a
/// declared FFI signature (SPEC "Optional parameters").
fn is_maybe_ty(ty: &Ty) -> bool {
    matches!(ty, Ty::App(f, _) if matches!(f.as_ref(), Ty::Con(c) if c == "Maybe"))
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
