//! Kind inference and kind checking.
//!
//! Kinds classify types the way types classify values: a complete type
//! (`Integer`, `Maybe String`) has kind `Type`, a type constructor that still
//! needs arguments has an arrow kind (`Maybe : Type -> Type`,
//! `Either : Type -> Type -> Type`), and a constructor can take a
//! higher-kinded argument (`data Wrap f = Wrap (f Integer)` gives
//! `Wrap : (Type -> Type) -> Type`). mata-ll has no surface syntax for kinds
//! at all — every kind is inferred from use, and unconstrained kinds default
//! to `Type`, exactly GHC's Haskell-2010 kind defaulting.
//!
//! The machinery is a small unifier over `Kind` (which carries `Kind::Var`
//! for exactly this purpose), used in two modes:
//!
//! * INFERENCE (silent): `infer_declared_kinds` runs before pass 1 of
//!   `check_module` and computes the kind of every data type, newtype, type
//!   alias and type family in the merged module, handling mutual recursion by
//!   solving all their constraints against one shared substitution.
//!   `infer_class_kinds` (pass 1b) does the same for class declarations: it
//!   infers every class's type-variable kind together, from how the variable
//!   is used in the method signatures (in
//!   `class Foldable t where foldr :: (a -> b -> b) -> b -> t a -> b`, the
//!   use `t a` at a complete-type position forces `t : Type -> Type` — no
//!   annotation needed) AND from superclass agreement, order-independently.
//!   Failures are NOT reported here; the checking walk below re-finds them
//!   with the final kinds and better context.
//!
//! * CHECKING (reporting): pass 2b and pass 3 of `check_module` walk every
//!   type the user wrote — data fields, newtype bodies, alias bodies, class
//!   method signatures, instance heads and contexts, type-family equations,
//!   function/export signatures, and expression ascriptions — and report a
//!   diagnostic wherever a type is used at a kind it does not have.
//!
//! An instance head must have the kind the class variable was inferred at:
//! `instance Foldable []` is well-formed because `[] : Type -> Type` matches
//! Foldable's `t : Type -> Type`, while `instance Foldable Integer` and
//! `instance Show Maybe` are kind errors. Class kinds live in
//! `Checker::class_kinds` (builtin classes are seeded in `init_kinds`; user
//! classes are inferred by `infer_class_kinds`), parallel to the constructor
//! kind table `Checker::kinds`.
//!
//! PROMOTED KINDS (DataKinds): a parameterless, non-GADT data type promotes to
//! a real kind named after it — `data Nat = Z | S Nat` gives the kind `Nat`
//! (`Kind::Promoted("Nat")`), with `'Z :: Nat` and `'S :: Nat -> Nat`. Those
//! promoted constructor kinds are registered (`register_data_type` /
//! `promoted_constructor_kind`) BEFORE `infer_declared_kinds` runs, so an index
//! variable's kind is inferred from a promoted constructor in a GADT return
//! type: `n : Nat` from `VNil :: Vec 'Z a`. An index at the wrong promoted kind
//! (`Vec 'True`, `'True :: Bool`) is then a `Nat`-vs-`Bool` kind error, and a
//! natural-number type family is checked at `Nat -> … -> Nat`. Parameterised
//! and GADT data types keep the older `Type -> … -> Type` approximation for
//! their promoted constructors (promoting them precisely would need kind
//! polymorphism); and a non-GADT phantom parameter still defaults to `Type`
//! (no kind-signature syntax to say otherwise — pin the index with a GADT).

use super::*;

/// Kind-inference state for one declaration walk: the kinds assigned to the
/// declaration's type VARIABLES, and the kind-unification substitution the
/// walk accumulates. `report` selects between the two modes described in the
/// module header — a silent inference prepass, or a checking walk that pushes
/// diagnostics.
pub(super) struct KindCtx {
    /// Kind of each type variable in the scope currently being walked
    /// (a data declaration's parameters, one method signature's variables).
    /// A variable not yet seen gets a fresh kind variable on first use, so
    /// its kind is determined by how the signature uses it.
    vars: HashMap<String, Kind>,
    /// Kind-unification substitution: kind-variable id → kind.
    subst: HashMap<u32, Kind>,
    /// Next fresh kind-variable id.
    next: u32,
    /// When false, unification failures are silently ignored (inference-only
    /// prepass); when true, the walk pushes diagnostics for them.
    report: bool,
}

impl KindCtx {
    pub(super) fn new(report: bool) -> Self {
        KindCtx { vars: HashMap::new(), subst: HashMap::new(), next: 0, report }
    }

    fn fresh(&mut self) -> Kind {
        let id = self.next;
        self.next += 1;
        Kind::Var(id)
    }

    /// Start walking a new variable scope (one constructor's fields, one
    /// method signature), seeded with the variables whose kinds are already
    /// fixed (a data type's parameters, the class variable). The
    /// substitution is deliberately kept — kinds solved so far stay solved.
    fn begin_scope(&mut self, seed: HashMap<String, Kind>) {
        self.vars = seed;
    }

    /// The kind of type variable `name` in the current scope, assigning a
    /// fresh kind variable on first use.
    fn var_kind(&mut self, name: &str) -> Kind {
        if let Some(k) = self.vars.get(name) {
            return k.clone();
        }
        let k = self.fresh();
        self.vars.insert(name.to_string(), k.clone());
        k
    }

    /// Resolve a kind through the substitution, recursively.
    fn zonk(&self, k: &Kind) -> Kind {
        match k {
            Kind::Var(id) => match self.subst.get(id) {
                Some(bound) => self.zonk(bound),
                None => k.clone(),
            },
            Kind::Arrow(a, b) => Kind::arrow(self.zonk(a), self.zonk(b)),
            Kind::Type | Kind::Symbol | Kind::Promoted(_) => k.clone(),
        }
    }

    /// Does kind variable `id` occur in `k` (already zonked)? Guards against
    /// building the infinite kind `k = k -> …` from a type applied to itself.
    fn occurs(id: u32, k: &Kind) -> bool {
        match k {
            Kind::Var(other) => *other == id,
            Kind::Arrow(a, b) => Self::occurs(id, a) || Self::occurs(id, b),
            Kind::Type | Kind::Symbol | Kind::Promoted(_) => false,
        }
    }

    /// Unify two kinds, extending the substitution. Errors carry no payload:
    /// the caller re-zonks both sides for the diagnostic it builds.
    fn unify(&mut self, a: &Kind, b: &Kind) -> Result<(), ()> {
        let a = self.zonk(a);
        let b = self.zonk(b);
        match (a, b) {
            (Kind::Type, Kind::Type) | (Kind::Symbol, Kind::Symbol) => Ok(()),
            // Two promoted kinds unify only when they name the same data type:
            // `Nat` and `Bool` are distinct kinds (this is what rejects a
            // `Bool`-tagged index where a `Nat` one is required).
            (Kind::Promoted(x), Kind::Promoted(y)) if x == y => Ok(()),
            (Kind::Var(i), k) | (k, Kind::Var(i)) => {
                if k == Kind::Var(i) {
                    Ok(())
                } else if Self::occurs(i, &k) {
                    Err(())
                } else {
                    self.subst.insert(i, k);
                    Ok(())
                }
            }
            (Kind::Arrow(a1, b1), Kind::Arrow(a2, b2)) => {
                self.unify(&a1, &a2)?;
                self.unify(&b1, &b2)
            }
            _ => Err(()),
        }
    }

    /// Zonk `k`, then default every kind variable still free to `Type` —
    /// Haskell-2010 kind defaulting. This is the only way a kind leaves a
    /// `KindCtx`: registered kinds and diagnostics never contain `Var`.
    fn default(&self, k: &Kind) -> Kind {
        match self.zonk(k) {
            Kind::Var(_) => Kind::Type,
            Kind::Arrow(a, b) => Kind::arrow(self.default(&a), self.default(&b)),
            // Type, Symbol, Promoted are already ground.
            other => other,
        }
    }
}

/// Peel parentheses off a type, for shape tests on what the user wrote.
fn strip_paren(ty: &Type) -> &Type {
    match ty {
        Type::Paren(inner) => strip_paren(inner),
        other => other,
    }
}

/// Render an AST type for a kind diagnostic, as the user wrote it (no alias
/// or type-family expansion — the message should show the offending source
/// text, not what it would have elaborated to).
pub(super) fn show_ast_type(ty: &Type) -> String {
    // An application or arrow nested in argument position needs parentheses.
    fn atom(t: &Type) -> String {
        match t {
            Type::App(..) | Type::Arrow(..) | Type::Constrained { .. }
            | Type::Forall { .. } | Type::ScopedLuaIO { .. } | Type::IO(_) =>
                format!("({})", show_ast_type(t)),
            _ => show_ast_type(t),
        }
    }
    match ty {
        Type::Con(name) => name.clone(),
        Type::Var(name) => name.clone(),
        Type::App(f, a) => format!("{} {}", show_ast_type(f), atom(a)),
        Type::Arrow(a, b, _) => match a.as_ref() {
            Type::Arrow(..) => format!("({}) -> {}", show_ast_type(a), show_ast_type(b)),
            _ => format!("{} -> {}", show_ast_type(a), show_ast_type(b)),
        },
        Type::List(a) => format!("[{}]", show_ast_type(a)),
        Type::IO(a) => format!("IO {}", atom(a)),
        Type::ScopedLuaIO { scope_var, inner } =>
            format!("LuaIO {} {}", scope_var, atom(inner)),
        Type::Forall { var, inner } => format!("forall {}. {}", var, show_ast_type(inner)),
        Type::Unit => "()".to_string(),
        Type::Paren(inner) => show_ast_type(inner),
        Type::Tuple(elems) => format!(
            "({})",
            elems.iter().map(show_ast_type).collect::<Vec<_>>().join(", ")
        ),
        Type::Constrained { ty, .. } => show_ast_type(ty),
        Type::Promoted(name) => format!("'{}", name),
        Type::LuaPure { lua_name, result } => format!("LuaPure \"{}\" {}", lua_name, atom(result)),
        Type::LuaIO { lua_name, result } => format!("LuaIO \"{}\" {}", lua_name, atom(result)),
        Type::LuaIterator { lua_name, result } => format!("LuaIterator \"{}\" {}", lua_name, atom(result)),
        Type::LuaTry { lua_name, result } => format!("LuaTry \"{}\" {}", lua_name, atom(result)),
        Type::LuaCatch { lua_name, result } => format!("LuaCatch \"{}\" {}", lua_name, atom(result)),
        Type::LuaIOCatch { lua_name, result } => format!("LuaIOCatch \"{}\" {}", lua_name, atom(result)),
    }
}

impl Checker {
    /// The kind a class's type variable was inferred at. Builtin classes are
    /// seeded in `init_kinds`, user classes in `register_class`; anything not
    /// in the table (an unknown class — reported elsewhere) defaults to
    /// `Type`, mirroring `kind_of`.
    pub(super) fn class_kind_of(&self, name: &str) -> Kind {
        self.class_kinds.get(name).cloned().unwrap_or(Kind::Type)
    }

    /// Infer the kind of a type expression, extending `kctx`'s substitution
    /// and reporting (in checking mode) every application that cannot be
    /// well-kinded. Returns the type's kind, which may still contain kind
    /// variables — callers default them before storing or displaying.
    fn infer_type_kind(&mut self, ty: &Type, kctx: &mut KindCtx, ctx: &str) -> Kind {
        match ty {
            Type::Con(name) => {
                // Type-level string literals parse as `Con "\"…\""`; they are
                // the (opaque) Symbol kind, consumed only by FFI forms.
                if name.starts_with('"') {
                    return Kind::Symbol;
                }
                if kctx.report {
                    self.check_con_defined(name, ctx);
                }
                if let Some(kind) = self.kinds.get(name) {
                    kind.clone()
                } else if let Some((params, _)) = self.type_aliases.get(name) {
                    // An alias registered outside this module's declarations
                    // (only the builtin `Int` today): its kind was never
                    // inferred, so approximate from its parameter count.
                    let mut k = Kind::Type;
                    for _ in 0..params.len() {
                        k = Kind::arrow(Kind::Type, k);
                    }
                    k
                } else {
                    // Undefined name — already reported by check_con_defined.
                    // A fresh kind variable lets the walk continue without a
                    // cascade of follow-on kind errors.
                    kctx.fresh()
                }
            }
            Type::Var(name) => kctx.var_kind(name),
            Type::App(f, a) => {
                let kf = self.infer_type_kind(f, kctx, ctx);
                let kf = kctx.zonk(&kf);
                let ka = self.infer_type_kind(a, kctx, ctx);
                match kf {
                    Kind::Arrow(dom, res) => {
                        if kctx.unify(&ka, &dom).is_err() && kctx.report {
                            self.push_error_ctx(
                                DiagnosticKind::KindArgMismatch {
                                    func: show_ast_type(f),
                                    arg: show_ast_type(a),
                                    expected: kctx.default(&dom),
                                    found: kctx.default(&ka),
                                },
                                ctx.to_string(),
                            );
                        }
                        *res
                    }
                    Kind::Var(_) => {
                        // The head's kind is not yet known (a type variable,
                        // or an undefined constructor): applying it teaches
                        // us it takes ka to some fresh result kind.
                        let res = kctx.fresh();
                        if kctx.unify(&kf, &Kind::arrow(ka, res.clone())).is_err()
                            && kctx.report
                        {
                            // Occurs check: only a type applied to itself
                            // (`t t`) can get here — the infinite kind
                            // `k = k -> …` has no finite solution.
                            self.push_error_ctx(
                                DiagnosticKind::Other(format!(
                                    "Kind error: '{}' would need the infinite kind 'k = k -> …': the type is applied to itself, so its kind would have to contain itself",
                                    show_ast_type(ty)
                                )),
                                ctx.to_string(),
                            );
                        }
                        res
                    }
                    Kind::Type | Kind::Symbol | Kind::Promoted(_) => {
                        if kctx.report {
                            // A VARIABLE head only reaches kind Type through
                            // another use in the same declaration — say so,
                            // rather than calling it a "complete type".
                            let is_var = matches!(strip_paren(f), Type::Var(_));
                            self.push_error_ctx(
                                DiagnosticKind::KindSaturatedApp {
                                    ty: show_ast_type(f),
                                    arg: show_ast_type(a),
                                    is_var,
                                    kind: kctx.default(&kf),
                                },
                                ctx.to_string(),
                            );
                        }
                        // Recover with a fresh kind so one bad application
                        // doesn't invalidate the whole surrounding type.
                        kctx.fresh()
                    }
                }
            }
            Type::Arrow(a, b, _) => {
                self.expect_type_kind(a, &Kind::Type, kctx, ctx);
                self.expect_type_kind(b, &Kind::Type, kctx, ctx);
                Kind::Type
            }
            Type::List(a) | Type::IO(a) => {
                self.expect_type_kind(a, &Kind::Type, kctx, ctx);
                Kind::Type
            }
            Type::Tuple(elems) => {
                for e in elems {
                    self.expect_type_kind(e, &Kind::Type, kctx, ctx);
                }
                Kind::Type
            }
            Type::ScopedLuaIO { scope_var, inner } => {
                // The phantom scope variable is an ordinary type of kind Type.
                let sk = kctx.var_kind(scope_var);
                let _ = kctx.unify(&sk, &Kind::Type);
                self.expect_type_kind(inner, &Kind::Type, kctx, ctx);
                Kind::Type
            }
            Type::LuaPure { result, .. }
            | Type::LuaIO { result, .. }
            | Type::LuaIterator { result, .. }
            | Type::LuaTry { result, .. }
            | Type::LuaCatch { result, .. }
            | Type::LuaIOCatch { result, .. } => {
                // Every FFI form reduces to a complete type built from its
                // result type, so the result itself must be one.
                self.expect_type_kind(result, &Kind::Type, kctx, ctx);
                Kind::Type
            }
            Type::Paren(inner) => self.infer_type_kind(inner, kctx, ctx),
            Type::Forall { var, inner } => {
                // Rank-2 foralls quantify FFI scope variables, kind Type.
                let vk = kctx.var_kind(var);
                let _ = kctx.unify(&vk, &Kind::Type);
                self.infer_type_kind(inner, kctx, ctx)
            }
            Type::Constrained { constraints, ty } => {
                // Each constraint uses its argument at the class variable's
                // kind: `Foldable t => …` forces `t : Type -> Type` even
                // before the body of the signature is walked. Unknown classes
                // are skipped — they are reported by the constraint checks.
                for c in constraints {
                    if self.classes.contains_key(&c.class_name) {
                        let ck = self.class_kind_of(&c.class_name);
                        self.expect_type_kind(&c.type_arg, &ck, kctx, ctx);
                    }
                }
                self.infer_type_kind(ty, kctx, ctx)
            }
            Type::Promoted(name) => {
                let key = format!("'{}", name);
                if let Some(kind) = self.kinds.get(&key).cloned() {
                    kind
                } else if kctx.report {
                    self.push_error_ctx(
                        DiagnosticKind::Other(format!("Unknown promoted constructor '{}'", name)),
                        ctx.to_string(),
                    );
                    Kind::Type
                } else {
                    // Silent prepass: promoted constructors of this module
                    // are registered later (pass 1); don't guess.
                    kctx.fresh()
                }
            }
            Type::Unit => Kind::Type,
        }
    }

    /// Infer `ty`'s kind and require it to be `expected`, reporting a
    /// mismatch (in checking mode) with the type rendered as written.
    fn expect_type_kind(&mut self, ty: &Type, expected: &Kind, kctx: &mut KindCtx, ctx: &str) {
        let found = self.infer_type_kind(ty, kctx, ctx);
        if kctx.unify(&found, expected).is_err() && kctx.report {
            self.push_error_ctx(
                DiagnosticKind::KindMismatch {
                    ty: show_ast_type(ty),
                    expected: kctx.default(expected),
                    found: kctx.default(&found),
                },
                ctx.to_string(),
            );
        }
    }

    /// Kind-check a standalone type that must be a complete type (kind Type):
    /// a function/export signature, a newtype body, an ascription. Each call
    /// is its own variable scope — the same variable name in two different
    /// signatures is two different variables. Also rejects references to type
    /// names that were never defined (via `check_con_defined`).
    pub(super) fn check_type_kind(&mut self, ty: &Type, ctx: &str) {
        let mut kctx = KindCtx::new(true);
        self.expect_type_kind(ty, &Kind::Type, &mut kctx, ctx);
        self.check_family_saturation(ty, ctx);
    }

    /// Reject UNSATURATED type-family applications anywhere in `ty`. A type
    /// family is not a first-class type constructor: it is a compile-time
    /// function over types, evaluable only when it has all its arguments, so
    /// it cannot be passed unapplied where a type constructor is expected
    /// (`data Wrap f = Wrap (f Integer)` with `Wrap SomeFamily`). GHC
    /// rejects partial family application for the same reason; silently
    /// accepting it left the application stuck forever and produced baffling
    /// downstream errors (or none at all).
    pub(super) fn check_family_saturation(&mut self, ty: &Type, ctx: &str) {
        // Walk the application spine to count how many arguments the head
        // receives, checking each argument subtree along the way.
        let mut head = strip_paren(ty);
        let mut nargs = 0usize;
        while let Type::App(f, a) = head {
            self.check_family_saturation(a, ctx);
            nargs += 1;
            head = strip_paren(f);
        }
        match head {
            Type::Con(name) => {
                if let Some(eqs) = self.type_families.get(name) {
                    let arity = eqs.first().map(|e| e.args.len()).unwrap_or(0);
                    if nargs < arity {
                        let name = name.clone();
                        self.push_error_ctx(
                            DiagnosticKind::Other(format!(
                                "Type family '{}' is applied to {} of its {} argument{} here. A type family is not a first-class type constructor — it is a compile-time function over types that can only be evaluated once fully applied — so it cannot be passed unapplied where a type constructor is expected. GHC rejects unsaturated type families for the same reason",
                                name, nargs, arity, if arity == 1 { "" } else { "s" }
                            )),
                            ctx.to_string(),
                        );
                    }
                }
            }
            Type::Arrow(a, b, _) => {
                self.check_family_saturation(a, ctx);
                self.check_family_saturation(b, ctx);
            }
            Type::List(a) | Type::IO(a) => self.check_family_saturation(a, ctx),
            Type::ScopedLuaIO { inner, .. } => self.check_family_saturation(inner, ctx),
            Type::Forall { inner, .. } => self.check_family_saturation(inner, ctx),
            Type::Constrained { ty, .. } => self.check_family_saturation(ty, ctx),
            Type::Tuple(elems) => {
                for e in elems {
                    self.check_family_saturation(e, ctx);
                }
            }
            Type::LuaPure { result, .. }
            | Type::LuaIO { result, .. }
            | Type::LuaIterator { result, .. }
            | Type::LuaTry { result, .. }
            | Type::LuaCatch { result, .. }
            | Type::LuaIOCatch { result, .. } => self.check_family_saturation(result, ctx),
            _ => {}
        }
    }

    /// Kind-check the field types of one data constructor. All fields share
    /// one scope so the constructor's existential variables are used
    /// consistently across its fields; the data type's parameters come in
    /// pre-seeded with their inferred kinds.
    pub(super) fn check_constructor_kinds(
        &mut self,
        field_types: &[&Type],
        params: HashMap<String, Kind>,
        ctx: &str,
    ) {
        let mut kctx = KindCtx::new(true);
        kctx.begin_scope(params);
        for ft in field_types {
            self.expect_type_kind(ft, &Kind::Type, &mut kctx, ctx);
            self.check_family_saturation(ft, ctx);
        }
    }

    /// Kind-check one class method signature, with the class variable
    /// pre-seeded at the class's inferred kind — so a method that uses the
    /// variable inconsistently with its siblings (`t a` in one method, bare
    /// `t` in another) is reported at the deviating method.
    pub(super) fn check_class_method_kind(
        &mut self,
        class_name: &str,
        class_var: &str,
        method_ty: &Type,
        ctx: &str,
    ) {
        let mut kctx = KindCtx::new(true);
        let mut seed = HashMap::new();
        seed.insert(class_var.to_string(), self.class_kind_of(class_name));
        kctx.begin_scope(seed);
        self.expect_type_kind(method_ty, &Kind::Type, &mut kctx, ctx);
    }

    /// Kind-check an instance declaration: the head's kind must be exactly
    /// the class variable's kind (`instance Foldable []` needs
    /// `[] : Type -> Type`; `instance Show (Tree a)` needs the applied head
    /// at kind Type), and the context's constraints must use the head's
    /// variables at their class's kind.
    pub(super) fn check_instance_kind(
        &mut self,
        class_name: &str,
        target_type: &Type,
        context: &[Constraint],
    ) {
        let ctx = format!("the instance declaration 'instance {} {}'",
            class_name, show_ast_type(target_type));
        let mut kctx = KindCtx::new(true);
        let found = self.infer_type_kind(target_type, &mut kctx, &ctx);
        // Unknown class: reported by check_instance; no kind to check against.
        if let Some(class_info) = self.classes.get(class_name) {
            let class_var = class_info.type_var.clone();
            let expected = self.class_kind_of(class_name);
            if kctx.unify(&found, &expected).is_err() {
                self.push_error_ctx(
                    DiagnosticKind::InstanceKindMismatch {
                        class: class_name.to_string(),
                        class_var,
                        target: show_ast_type(target_type),
                        expected: kctx.default(&expected),
                        found: kctx.default(&found),
                    },
                    ctx.clone(),
                );
            }
        }
        // The context shares the head's variable scope: in
        // `instance (Show a) => C (T a)`, the `a` being constrained is the
        // head's `a`, at whatever kind the head fixed for it.
        for c in context {
            if self.classes.contains_key(&c.class_name) {
                let ck = self.class_kind_of(&c.class_name);
                self.expect_type_kind(&c.type_arg, &ck, &mut kctx, &ctx);
            }
        }
    }

    /// Kind-check a type alias declaration's body, with the parameters
    /// pre-seeded at the kinds `infer_declared_kinds` gave them.
    pub(super) fn check_alias_kinds(&mut self, name: &str, params: &[String], ty: &Type, ctx: &str) {
        let mut kctx = KindCtx::new(true);
        kctx.begin_scope(self.param_kind_seed(name, params));
        // No top-level expectation: an alias may abbreviate a constructor of
        // any kind (its own kind was inferred as params -> body kind).
        self.infer_type_kind(ty, &mut kctx, ctx);
        self.check_family_saturation(ty, ctx);
    }

    /// Kind-check a type family's equations AGAINST THE FAMILY'S OWN KIND
    /// (as inferred by the silent prepass — pinned by the first equation
    /// that constrains each position). Each equation is its own variable
    /// scope. Every pattern must sit at the family's argument kind and every
    /// result at its result kind, so an ill-kinded equation (`Mix 'Z = …;
    /// Mix 'True = …` mixing Nat and Bool patterns) is an error AT THE
    /// DEFINITION — even when the bad equation is never used. Merely walking
    /// the patterns (the old behavior) reported nothing until a USE site
    /// tripped over the family's inferred kind, blaming the user's code for
    /// the library's ill-formed definition.
    pub(super) fn check_family_kinds(&mut self, name: &str, equations: &[TypeFamilyEq], ctx: &str) {
        let family_kind = self.kinds.get(name).cloned().unwrap_or(Kind::Type);
        for eq in equations {
            let mut kctx = KindCtx::new(true);
            let mut kind = family_kind.clone();
            for arg in &eq.args {
                let ka = self.infer_type_kind(arg, &mut kctx, ctx);
                match kind {
                    Kind::Arrow(dom, rest) => {
                        if kctx.unify(&ka, &dom).is_err() {
                            self.push_error_ctx(
                                DiagnosticKind::KindArgMismatch {
                                    func: name.to_string(),
                                    arg: show_ast_type(arg),
                                    expected: kctx.default(&dom),
                                    found: kctx.default(&ka),
                                },
                                ctx.to_string(),
                            );
                        }
                        kind = *rest;
                    }
                    other => kind = other,
                }
            }
            self.check_family_saturation(&eq.result, ctx);
            let kr = self.infer_type_kind(&eq.result, &mut kctx, ctx);
            if kctx.unify(&kr, &kind).is_err() {
                self.push_error_ctx(
                    DiagnosticKind::Other(format!(
                        "Kind error: this equation's result '{}' has kind {}, but the type family '{}' returns kind {} (pinned by its other equations)",
                        show_ast_type(&eq.result),
                        kctx.default(&kr),
                        name,
                        kctx.default(&kind),
                    )),
                    ctx.to_string(),
                );
            }
        }
    }

    /// The seed map for a data/newtype/alias declaration's parameters: each
    /// parameter name mapped to the kind the registered constructor kind
    /// gives it (the i-th arrow argument of `kinds[name]`). Falls back to
    /// Type if the registered kind is missing or shorter than the parameter
    /// list (both would be a compiler bug, not a user error).
    pub(super) fn param_kind_seed(&self, name: &str, params: &[String]) -> HashMap<String, Kind> {
        let mut seed = HashMap::new();
        let mut kind = self.kinds.get(name).cloned().unwrap_or(Kind::Type);
        for p in params {
            let (dom, rest) = match kind {
                Kind::Arrow(dom, rest) => (*dom, *rest),
                _ => (Kind::Type, Kind::Type),
            };
            seed.insert(p.clone(), dom);
            kind = rest;
        }
        seed
    }

    /// Infer the type-variable kind of EVERY class the module declares —
    /// BEFORE pass 2 registers them — solving all their constraints against
    /// one shared substitution so the result does not depend on declaration
    /// order.
    ///
    /// A class variable's kind is pinned by two things: how the variable is
    /// used in the method signatures (`foldr :: … -> t a -> b` forces
    /// `t : Type -> Type`), and its superclasses (`class Super t => Sub t`
    /// makes Sub's `t` share Super's kind — the constraint applies the same
    /// variable). The subtle case is a subclass whose OWN methods never pin
    /// the variable's kind (they may not even mention it), so the kind is
    /// knowable only through a superclass: if that superclass is declared
    /// LATER in the module, a naive per-class pass would not yet know its
    /// kind and would wrongly default the subclass to `Type`. Giving every
    /// class a provisional kind variable up front and unifying against the
    /// shared substitution fixes both the forward and backward reference,
    /// exactly as `infer_declared_kinds` does for data types.
    ///
    /// Silent by design: a genuinely conflicting class declaration keeps its
    /// first-solved kind here, and the checking walk in pass 2b (seeded with
    /// this result) reports the deviating method where the user can see it.
    pub(super) fn infer_class_kinds(&mut self, decls: &[Decl]) {
        let mut kctx = KindCtx::new(false);

        // Step 1: a provisional kind variable for every class this module
        // declares, registered so cross-references (a superclass declared
        // either before OR after) resolve to the same shared variable.
        let mut class_kv: HashMap<String, Kind> = HashMap::new();
        let mut declared: Vec<String> = Vec::new();
        for decl in decls {
            if let Decl::ClassDecl { name, .. } = decl {
                let kv = kctx.fresh();
                class_kv.insert(name.clone(), kv.clone());
                self.class_kinds.insert(name.clone(), kv);
                declared.push(name.clone());
            }
        }

        // Step 2: walk every class against the shared substitution.
        for decl in decls {
            if let Decl::ClassDecl { name, type_var, superclasses, methods } = decl {
                let kv = class_kv[name].clone();
                // A superclass constrains the SAME variable, so their kinds
                // must agree. Its kind is the provisional variable when the
                // superclass is declared in this module (order-independent),
                // otherwise the finalized/builtin kind.
                for sup in superclasses {
                    let sk = class_kv.get(sup)
                        .cloned()
                        .unwrap_or_else(|| self.class_kind_of(sup));
                    let _ = kctx.unify(&kv, &sk);
                }
                for method in methods {
                    // Each method signature is its own scope for ITS
                    // variables, but the class variable's kind is shared
                    // across all of them and across the superclasses.
                    let mut seed = HashMap::new();
                    seed.insert(type_var.to_string(), kv.clone());
                    kctx.begin_scope(seed);
                    let mk = self.infer_type_kind(&method.ty, &mut kctx, "");
                    let _ = kctx.unify(&mk, &Kind::Type);
                }
            }
        }

        // Step 3: default what nothing constrained and finalize the table.
        for name in declared {
            if let Some(kind) = self.class_kinds.get(&name).cloned() {
                self.class_kinds.insert(name, kctx.default(&kind));
            }
        }
    }

    /// Infer the kinds of every data type, newtype, type alias and type
    /// family declared in the module — BEFORE pass 1 registers anything.
    ///
    /// Every declaration first gets a provisional kind built from fresh kind
    /// variables (`data T f a` gets `k1 -> k2 -> Type`), registered in
    /// `self.kinds` so that the constraint walk can look up mutually
    /// recursive references. Then every field/body/equation is walked against
    /// one shared substitution, so a use in one declaration can determine a
    /// parameter kind in another. Finally the still-unconstrained kind
    /// variables default to Type and the solved kinds replace the
    /// provisional entries.
    ///
    /// Silent by design: an ill-kinded declaration keeps its first-solved
    /// kind here, and the checking walks in pass 2b (seeded with these final
    /// kinds) report the conflict where the user can see it.
    pub(super) fn infer_declared_kinds(&mut self, decls: &[Decl]) {
        let mut kctx = KindCtx::new(false);

        // Step 1: provisional kinds for everything this module declares.
        let mut declared: Vec<String> = Vec::new();
        for decl in decls {
            match decl {
                Decl::DataDef { name, type_vars, .. }
                | Decl::NewtypeDef { name, type_vars, .. } => {
                    let mut kind = Kind::Type;
                    for _ in 0..type_vars.len() {
                        kind = Kind::arrow(kctx.fresh(), kind);
                    }
                    self.kinds.insert(name.clone(), kind);
                    declared.push(name.clone());
                }
                Decl::TypeAlias { name, params, .. } => {
                    // params -> (body kind); the body kind is solved by the
                    // constraint walk (an alias may abbreviate a constructor).
                    let mut kind = kctx.fresh();
                    for _ in 0..params.len() {
                        kind = Kind::arrow(kctx.fresh(), kind);
                    }
                    self.kinds.insert(name.clone(), kind);
                    declared.push(name.clone());
                }
                Decl::TypeFamily { name, equations } => {
                    let arity = equations.first().map(|eq| eq.args.len()).unwrap_or(0);
                    let mut kind = kctx.fresh();
                    for _ in 0..arity {
                        kind = Kind::arrow(kctx.fresh(), kind);
                    }
                    self.kinds.insert(name.clone(), kind);
                    declared.push(name.clone());
                }
                _ => {}
            }
        }

        // Step 2: walk every declared type against the shared substitution.
        for decl in decls {
            match decl {
                Decl::DataDef { name, type_vars, constructors, .. } => {
                    for con in constructors {
                        if let Some(gadt_ty) = &con.gadt_type {
                            // A GADT signature scopes its own variables (the
                            // header's parameter names are arity markers);
                            // the result type's application spine constrains
                            // the data type's parameter kinds through the
                            // provisional kind.
                            kctx.begin_scope(HashMap::new());
                            let kg = self.infer_type_kind(gadt_ty, &mut kctx, "");
                            let _ = kctx.unify(&kg, &Kind::Type);
                            continue;
                        }
                        // Ordinary constructor: fields share one scope with
                        // the data parameters seeded to the provisional
                        // parameter kinds; existential variables get fresh
                        // kinds on first use.
                        kctx.begin_scope(self.param_kind_seed(name, type_vars));
                        let field_types: Vec<&Type> = match &con.fields {
                            ConstructorFields::Positional(tys) => tys.iter().collect(),
                            ConstructorFields::Named(fields) => fields.iter().map(|f| &f.ty).collect(),
                        };
                        for ft in field_types {
                            let kf = self.infer_type_kind(ft, &mut kctx, "");
                            let _ = kctx.unify(&kf, &Kind::Type);
                        }
                    }
                }
                Decl::NewtypeDef { name, type_vars, inner } => {
                    kctx.begin_scope(self.param_kind_seed(name, type_vars));
                    let ki = self.infer_type_kind(inner, &mut kctx, "");
                    let _ = kctx.unify(&ki, &Kind::Type);
                }
                Decl::TypeAlias { name, params, ty } => {
                    kctx.begin_scope(self.param_kind_seed(name, params));
                    let kb = self.infer_type_kind(ty, &mut kctx, "");
                    // The alias's registered result kind IS its body's kind.
                    let mut result = self.kinds.get(name).cloned().unwrap_or(Kind::Type);
                    for _ in 0..params.len() {
                        result = match result {
                            Kind::Arrow(_, rest) => *rest,
                            other => other,
                        };
                    }
                    let _ = kctx.unify(&kb, &result);
                }
                Decl::TypeFamily { name, equations } => {
                    // Each equation constrains the family's argument and
                    // result kinds; pattern variables are scoped per equation.
                    let family_kind = self.kinds.get(name).cloned().unwrap_or(Kind::Type);
                    for eq in equations {
                        kctx.begin_scope(HashMap::new());
                        let mut kind = family_kind.clone();
                        for arg in &eq.args {
                            let ka = self.infer_type_kind(arg, &mut kctx, "");
                            if let Kind::Arrow(dom, rest) = kind {
                                let _ = kctx.unify(&ka, &dom);
                                kind = *rest;
                            }
                        }
                        let kr = self.infer_type_kind(&eq.result, &mut kctx, "");
                        let _ = kctx.unify(&kr, &kind);
                    }
                }
                _ => {}
            }
        }

        // Step 3: default what nothing constrained and finalize the tables.
        // An unconstrained parameter (a phantom, never used in a field)
        // defaults to `Type`, matching GHC without a kind signature — so a
        // promoted tag of a non-`Type` kind cannot be an index of a phantom
        // parameter (pin the index via a GADT constructor return type instead;
        // see the note in CAVEATS/HASKDIFF and the `datakinds.mll` pattern).
        for name in declared {
            if let Some(kind) = self.kinds.get(&name).cloned() {
                self.kinds.insert(name, kctx.default(&kind));
            }
        }
    }
}
