use std::collections::HashMap;
use std::fmt;

/// A typeclass constraint on a type variable, e.g. Show a
#[derive(Debug, Clone)]
pub struct TyConstraint {
    pub class_name: String,
    pub type_var: String,
}

/// Kind of a type expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// Regular types: Integer, String, Maybe Integer
    Type,
    /// Type-level string literals (used in FFI type families)
    Symbol,
    /// Function type constructor: Type -> Type (e.g., Maybe, [])
    Arrow(Box<Kind>, Box<Kind>),
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Kind::Type => write!(f, "Type"),
            Kind::Symbol => write!(f, "Symbol"),
            Kind::Arrow(a, b) => write!(f, "{} -> {}", a, b),
        }
    }
}

/// Internal type representation used by the type checker.
/// Separate from the AST's Type to allow for unification variables.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Ty {
    /// Concrete type: Integer, String, Bool, Number
    Con(String),
    /// Type variable (rigid or unification)
    Var(TyVar),
    /// Function type: a -> b
    Arrow(Box<Ty>, Box<Ty>),
    /// Type application: Maybe a, Tree Int
    App(Box<Ty>, Box<Ty>),
    /// List type: [a]
    List(Box<Ty>),
    /// IO type: IO a
    IO(Box<Ty>),
    /// Scoped Lua IO: LuaIO s a (s is a phantom scope variable)
    LuaIO(TyVar, Box<Ty>),
    /// Unit type: ()
    Unit,
    /// Rank-2 forall: forall s. ty
    Forall(TyVar, Box<Ty>),
    /// Rigid skolem constant — cannot unify with anything except itself.
    /// Created during rank-2 type checking to enforce polymorphism requirements.
    Skolem(String, u32),
    /// Tuple type: (a, b, c)
    Tuple(Vec<Ty>),
    /// Promoted data constructor (DataKinds): 'Empty, 'NonEmpty
    Promoted(String),
}

impl Ty {
    pub fn arrow(from: Ty, to: Ty) -> Ty {
        Ty::Arrow(Box::new(from), Box::new(to))
    }

    pub fn app(f: Ty, a: Ty) -> Ty {
        // Normalize: App(Con("[]"), a) → List(a), App(Con("IO"), a) → IO(a)
        if let Ty::Con(ref name) = f {
            match name.as_str() {
                "[]" => return Ty::List(Box::new(a)),
                "IO" => return Ty::IO(Box::new(a)),
                _ => {}
            }
        }
        Ty::App(Box::new(f), Box::new(a))
    }

    pub fn io(inner: Ty) -> Ty {
        Ty::IO(Box::new(inner))
    }

    pub fn lua_io(scope: TyVar, inner: Ty) -> Ty {
        Ty::LuaIO(scope, Box::new(inner))
    }

    pub fn list(inner: Ty) -> Ty {
        Ty::List(Box::new(inner))
    }

    /// Build a multi-argument function type: a -> b -> c -> ret
    pub fn fun(args: &[Ty], ret: Ty) -> Ty {
        args.iter().rev().fold(ret, |acc, arg| Ty::arrow(arg.clone(), acc))
    }

    /// Number of top-level arrows: `a -> b -> c` is 2, `Con` is 0.
    pub fn arrow_arity(&self) -> usize {
        match self {
            Ty::Arrow(_, rest) => 1 + rest.arrow_arity(),
            _ => 0,
        }
    }

    /// The final result type after peeling all top-level arrows.
    pub fn return_type(&self) -> &Ty {
        match self {
            Ty::Arrow(_, rest) => rest.return_type(),
            other => other,
        }
    }

    /// Split a function type into its argument types and final result.
    /// `a -> b -> c` becomes `([a, b], c)`.
    pub fn peel_arrows(&self) -> (Vec<&Ty>, &Ty) {
        let mut args = Vec::new();
        let mut cur = self;
        while let Ty::Arrow(a, b) = cur {
            args.push(a.as_ref());
            cur = b.as_ref();
        }
        (args, cur)
    }

    /// Collect all free type variables
    pub fn free_vars(&self) -> Vec<TyVar> {
        match self {
            Ty::Con(_) | Ty::Unit | Ty::Promoted(_) | Ty::Skolem(..) => vec![],
            Ty::Var(v) => vec![v.clone()],
            Ty::Arrow(a, b) | Ty::App(a, b) => {
                let mut vars = a.free_vars();
                for v in b.free_vars() {
                    if !vars.contains(&v) {
                        vars.push(v);
                    }
                }
                vars
            }
            Ty::List(a) | Ty::IO(a) => a.free_vars(),
            Ty::LuaIO(s, a) => {
                let mut vars = vec![s.clone()];
                for v in a.free_vars() {
                    if !vars.contains(&v) { vars.push(v); }
                }
                vars
            }
            Ty::Forall(v, inner) => {
                inner.free_vars().into_iter().filter(|fv| fv != v).collect()
            }
            Ty::Tuple(elems) => {
                let mut vars = vec![];
                for e in elems {
                    for v in e.free_vars() {
                        if !vars.contains(&v) { vars.push(v); }
                    }
                }
                vars
            }
        }
    }

    /// Apply a substitution to this type
    pub fn apply_subst(&self, subst: &Subst) -> Ty {
        match self {
            Ty::Con(_) | Ty::Unit | Ty::Promoted(_) | Ty::Skolem(..) => self.clone(),
            Ty::Var(v) => {
                // Follow substitution chain iteratively to avoid stack overflow
                // from cyclic or long transitive mappings (e.g., a→b, b→c, c→Int)
                let mut current = v;
                let mut depth = 0;
                loop {
                    if let Some(ty) = subst.lookup(current) {
                        if let Ty::Var(next) = ty {
                            depth += 1;
                            if depth > 100 { return ty.clone(); }
                            current = next;
                        } else {
                            return ty.apply_subst(subst);
                        }
                    } else {
                        return Ty::Var(current.clone());
                    }
                }
            }
            Ty::Arrow(a, b) => Ty::arrow(a.apply_subst(subst), b.apply_subst(subst)),
            Ty::App(a, b) => Ty::app(a.apply_subst(subst), b.apply_subst(subst)),
            Ty::List(a) => Ty::list(a.apply_subst(subst)),
            Ty::IO(a) => Ty::io(a.apply_subst(subst)),
            Ty::LuaIO(s, a) => {
                let new_s = if let Some(Ty::Var(sv)) = subst.lookup(s) {
                    sv.clone()
                } else {
                    s.clone()
                };
                Ty::lua_io(new_s, a.apply_subst(subst))
            }
            Ty::Forall(v, inner) => {
                let mut restricted = subst.clone();
                restricted.remove(v);
                Ty::Forall(v.clone(), Box::new(inner.apply_subst(&restricted)))
            }
            Ty::Tuple(elems) => Ty::Tuple(elems.iter().map(|e| e.apply_subst(subst)).collect()),
        }
    }

    /// Check if a specific skolem occurs in this type (for escape check)
    pub fn contains_skolem(&self, name: &str, id: u32) -> bool {
        match self {
            Ty::Skolem(n, i) => n == name && *i == id,
            Ty::Con(_) | Ty::Unit | Ty::Promoted(_) | Ty::Var(_) => false,
            Ty::Arrow(a, b) | Ty::App(a, b) => a.contains_skolem(name, id) || b.contains_skolem(name, id),
            Ty::List(a) | Ty::IO(a) => a.contains_skolem(name, id),
            Ty::LuaIO(_, a) => a.contains_skolem(name, id),
            Ty::Forall(_, inner) => inner.contains_skolem(name, id),
            Ty::Tuple(elems) => elems.iter().any(|e| e.contains_skolem(name, id)),
        }
    }

    /// Check if a type variable occurs in this type (for occurs check)
    pub fn occurs(&self, v: &TyVar) -> bool {
        match self {
            Ty::Con(_) | Ty::Unit | Ty::Promoted(_) | Ty::Skolem(..) => false,
            Ty::Var(w) => v == w,
            Ty::Arrow(a, b) | Ty::App(a, b) => a.occurs(v) || b.occurs(v),
            Ty::List(a) | Ty::IO(a) => a.occurs(v),
            Ty::LuaIO(s, a) => v == s || a.occurs(v),
            Ty::Forall(_, inner) => inner.occurs(v),
            Ty::Tuple(elems) => elems.iter().any(|e| e.occurs(v)),
        }
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ty::Con(name) => write!(f, "{}", name),
            Ty::Promoted(name) => write!(f, "'{}", name),
            Ty::Var(v) => write!(f, "{}", v),
            Ty::Arrow(a, b) => {
                match a.as_ref() {
                    Ty::Arrow(_, _) => write!(f, "({}) -> {}", a, b),
                    _ => write!(f, "{} -> {}", a, b),
                }
            }
            Ty::App(a, b) => {
                match b.as_ref() {
                    Ty::App(_, _) | Ty::Arrow(_, _) => write!(f, "{} ({})", a, b),
                    _ => write!(f, "{} {}", a, b),
                }
            }
            Ty::List(a) => write!(f, "[{}]", a),
            Ty::IO(a) => write!(f, "IO {}", a),
            Ty::LuaIO(s, a) => write!(f, "LuaIO {} {}", s, a),
            Ty::Forall(v, inner) => write!(f, "forall {}. {}", v, inner),
            Ty::Skolem(name, _) => write!(f, "{}", name),
            Ty::Unit => write!(f, "()"),
            Ty::Tuple(elems) => {
                write!(f, "(")?;
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", e)?;
                }
                write!(f, ")")
            }
        }
    }
}

/// The head constructor a typeclass instance attaches to — the *structured*
/// identity under which instances are registered by the typechecker and
/// resolved by the monomorphizer. mata-ll instances are per-head-constructor
/// (an `instance C (Pair a b)` is the one and only C instance for `Pair`), so
/// this key is exact. It replaces the old Display-string keys ("Pair a b",
/// "[a]", "[Integer]"), which required prefix probes and positional guessing
/// to relate a use-site type to its instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum InstHead {
    /// A named type constructor: `Integer`, `Maybe`, `Pair`, `IO`, `ST`, …
    Con(String),
    /// The list constructor `[]`.
    List,
    /// A tuple constructor of the given arity.
    Tuple(usize),
    /// The unit type `()`.
    Unit,
}

impl InstHead {
    /// The head constructor of a type, when it has one. `Maybe a`, `Pair x y`
    /// and bare `Pair` all yield `Con("Pair")`-style heads; sugared list/IO
    /// forms normalize to the same head as their constructor spelling
    /// (`[a]` and `[] a` are both `List`; `IO a` and the `IO` constructor are
    /// both `Con("IO")`). Variables, skolems, functions and promoted
    /// constructors have no instance head.
    pub fn of(ty: &Ty) -> Option<InstHead> {
        match ty {
            Ty::Con(n) if n == "[]" => Some(InstHead::List),
            Ty::Con(n) => Some(InstHead::Con(n.clone())),
            Ty::List(_) => Some(InstHead::List),
            Ty::IO(_) => Some(InstHead::Con("IO".into())),
            Ty::LuaIO(_, _) => Some(InstHead::Con("LuaIO".into())),
            Ty::Unit => Some(InstHead::Unit),
            Ty::Tuple(elems) => Some(InstHead::Tuple(elems.len())),
            Ty::App(f, _) => InstHead::of(f),
            Ty::Forall(_, inner) => InstHead::of(inner),
            Ty::Var(_) | Ty::Skolem(..) | Ty::Arrow(..) | Ty::Promoted(_) => None,
        }
    }
}

impl fmt::Display for InstHead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstHead::Con(n) => write!(f, "{}", n),
            InstHead::List => write!(f, "[]"),
            InstHead::Tuple(n) => write!(f, "({})", ",".repeat(n.saturating_sub(1))),
            InstHead::Unit => write!(f, "()"),
        }
    }
}

/// Type variable identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TyVar {
    pub name: String,
    pub id: u32,
}

impl fmt::Display for TyVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

/// A type scheme: forall a b. constraint => type
/// Used for polymorphic bindings.
#[derive(Debug, Clone)]
pub struct Scheme {
    pub vars: Vec<TyVar>,
    pub ty: Ty,
}

impl Scheme {
    pub fn mono(ty: Ty) -> Scheme {
        Scheme { vars: vec![], ty }
    }

    pub fn apply_subst(&self, subst: &Subst) -> Scheme {
        // Don't substitute bound variables
        let mut restricted = subst.clone();
        for v in &self.vars {
            restricted.remove(v);
        }
        Scheme {
            vars: self.vars.clone(),
            ty: self.ty.apply_subst(&restricted),
        }
    }

    pub fn free_vars(&self) -> Vec<TyVar> {
        self.ty.free_vars()
            .into_iter()
            .filter(|v| !self.vars.contains(v))
            .collect()
    }
}

impl fmt::Display for Scheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.vars.is_empty() {
            write!(f, "{}", self.ty)
        } else {
            let vars: Vec<String> = self.vars.iter().map(|v| v.name.clone()).collect();
            write!(f, "forall {}. {}", vars.join(" "), self.ty)
        }
    }
}

/// Substitution: mapping from type variables to types
#[derive(Debug, Clone)]
pub struct Subst {
    map: HashMap<TyVar, Ty>,
}

impl Subst {
    pub fn empty() -> Subst {
        Subst { map: HashMap::new() }
    }

    pub fn from_map(map: HashMap<TyVar, Ty>) -> Subst {
        Subst { map }
    }

    pub fn singleton(v: TyVar, ty: Ty) -> Subst {
        let mut map = HashMap::new();
        map.insert(v, ty);
        Subst { map }
    }

    pub fn lookup(&self, v: &TyVar) -> Option<&Ty> {
        self.map.get(v)
    }

    pub fn remove(&mut self, v: &TyVar) {
        self.map.remove(v);
    }

    pub fn size(&self) -> usize {
        self.map.len()
    }

    /// Compose two substitutions: apply self first, then other
    /// (other ∘ self)(t) = other(self(t))
    pub fn compose(&self, other: &Subst) -> Subst {
        let mut result: HashMap<TyVar, Ty> = self.map
            .iter()
            .map(|(k, v)| (k.clone(), v.apply_subst(other)))
            .collect();
        for (k, v) in &other.map {
            result.entry(k.clone()).or_insert_with(|| v.clone());
        }
        Subst { map: result }
    }

    /// Merge two INDEPENDENT substitutions (e.g. from two clauses of the same
    /// function, each checked against the same signature). Plain `compose`
    /// keeps `self`'s binding when both bind the same variable and silently
    /// DROPS `other`'s — severing everything reachable only through the
    /// dropped binding (a later clause's body types lose their link to the
    /// signature, so their class constraints and instantiations can no longer
    /// be related to it). Merging instead unifies the two images when they
    /// are compatible, so both bindings hold in the result.
    ///
    /// When the images genuinely conflict, `self`'s binding is kept — that is
    /// deliberate, not a fallback: GADT-style clauses refine the same
    /// signature variable to *different* concrete types per clause
    /// (`action RedL` binds c := 'Red, `action GreenL` binds c := 'Green),
    /// and those refinements must stay clause-local. A conflicting image is
    /// always such a concrete refinement (two free clause variables can never
    /// conflict), so nothing is severed by keeping it out of the shared
    /// substitution.
    pub fn merge(&self, other: &Subst) -> Subst {
        let mut result = self.compose(other);
        for (v, t_other) in &other.map {
            if self.map.contains_key(v) {
                let img_self = Ty::Var(v.clone()).apply_subst(&result);
                let img_other = t_other.apply_subst(&result);
                if let Ok(mgu) = unify(&img_self, &img_other) {
                    result = result.compose(&mgu);
                }
            }
        }
        result
    }
}

/// Unification: find a substitution that makes two types equal
pub fn unify(t1: &Ty, t2: &Ty) -> Result<Subst, TypeErrorKind> {
    match (t1, t2) {
        (Ty::Con(a), Ty::Con(b)) if a == b => Ok(Subst::empty()),
        (Ty::Promoted(a), Ty::Promoted(b)) if a == b => Ok(Subst::empty()),
        (Ty::Unit, Ty::Unit) => Ok(Subst::empty()),

        (Ty::Var(v), t) | (t, Ty::Var(v)) => {
            if t == &Ty::Var(v.clone()) {
                Ok(Subst::empty())
            } else if t.occurs(v) {
                Err(TypeErrorKind::OccursCheck(v.clone(), t.clone()))
            } else {
                Ok(Subst::singleton(v.clone(), t.clone()))
            }
        }

        (Ty::Arrow(a1, b1), Ty::Arrow(a2, b2)) => {
            let s1 = unify(a1, a2)?;
            let s2 = unify(&b1.apply_subst(&s1), &b2.apply_subst(&s1))?;
            Ok(s1.compose(&s2))
        }

        (Ty::App(a1, b1), Ty::App(a2, b2)) => {
            let s1 = unify(a1, a2)?;
            let s2 = unify(&b1.apply_subst(&s1), &b2.apply_subst(&s1))?;
            Ok(s1.compose(&s2))
        }

        (Ty::List(a), Ty::List(b)) => unify(a, b),
        (Ty::IO(a), Ty::IO(b)) => unify(a, b),
        (Ty::LuaIO(s1, a), Ty::LuaIO(s2, b)) => {
            let s = unify(&Ty::Var(s1.clone()), &Ty::Var(s2.clone()))?;
            let s2 = unify(&a.apply_subst(&s), &b.apply_subst(&s))?;
            Ok(s.compose(&s2))
        }

        (Ty::Tuple(a), Ty::Tuple(b)) if a.len() == b.len() => {
            let mut s = Subst::empty();
            for (ea, eb) in a.iter().zip(b.iter()) {
                let si = unify(&ea.apply_subst(&s), &eb.apply_subst(&s))?;
                s = s.compose(&si);
            }
            Ok(s)
        }

        // Allow App(f, a) to unify with List(b) by treating [] as App(Con("[]"), ...)
        (Ty::App(f, a), Ty::List(b)) | (Ty::List(b), Ty::App(f, a)) => {
            let s1 = unify(f, &Ty::Con("[]".into()))?;
            let s2 = unify(&a.apply_subst(&s1), &b.apply_subst(&s1))?;
            Ok(s1.compose(&s2))
        }

        // Allow App(m, a) to unify with IO(b) by treating IO as App(Con("IO"), ...)
        (Ty::App(f, a), Ty::IO(b)) | (Ty::IO(b), Ty::App(f, a)) => {
            let s1 = unify(f, &Ty::Con("IO".into()))?;
            let s2 = unify(&a.apply_subst(&s1), &b.apply_subst(&s1))?;
            Ok(s1.compose(&s2))
        }

        // Allow App(m, a) to unify with LuaIO(s, b) by treating LuaIO as App(App(Con("LuaIO"), s), ...)
        (Ty::App(f, a), Ty::LuaIO(s, b)) | (Ty::LuaIO(s, b), Ty::App(f, a)) => {
            let lua_io_s = Ty::App(Box::new(Ty::Con("LuaIO".into())), Box::new(Ty::Var(s.clone())));
            let s1 = unify(f, &lua_io_s)?;
            let s2 = unify(&a.apply_subst(&s1), &b.apply_subst(&s1))?;
            Ok(s1.compose(&s2))
        }

        // Forall: instantiate the quantified variable and unify the body
        (Ty::Forall(_v, inner), t) | (t, Ty::Forall(_v, inner)) => {
            // The forall-bound variable is already a rigid skolem (id=MAX).
            // Unify the body directly — the variable will unify with whatever
            // the concrete type provides, enforcing that it can't escape.
            unify(inner, t)
        }

        // Skolem: rigid type constant, only unifies with itself
        (Ty::Skolem(a, i), Ty::Skolem(b, j)) if a == b && i == j => Ok(Subst::empty()),
        (Ty::Skolem(..), _t) | (_t, Ty::Skolem(..)) => {
            Err(TypeErrorKind::RigidMismatch(t1.clone(), t2.clone()))
        }

        _ => Err(TypeErrorKind::Mismatch(t1.clone(), t2.clone())),
    }
}

/// Type error with optional context and source location
#[derive(Debug)]
pub struct TypeError {
    pub kind: TypeErrorKind,
    pub context: Option<String>,
    pub span: Option<crate::ast::Span>,
}

impl TypeError {
    pub fn new(kind: TypeErrorKind) -> Self {
        TypeError { kind, context: None, span: None }
    }

    pub fn in_context(kind: TypeErrorKind, ctx: impl Into<String>) -> Self {
        TypeError { kind, context: Some(ctx.into()), span: None }
    }
}

#[derive(Debug)]
pub enum TypeErrorKind {
    Mismatch(Ty, Ty),
    RigidMismatch(Ty, Ty),
    OccursCheck(TyVar, Ty),
    UnboundVariable(String),
    UnboundConstructor(String),
    PatternArgCount { constructor: String, expected: usize, got: usize },
    NonExhaustive(String),
    TypeSigMismatch { name: String, declared: Ty, inferred: Ty },
    /// A class constraint with no matching instance, e.g. `Show (a -> b)`.
    NoInstance { class: String, ty: Ty },
    /// A class constraint left over with a type variable that nothing in the
    /// definition determines, e.g. `show Nothing` (the element of the `Maybe`
    /// is never fixed). No instance can be chosen, so it is rejected.
    AmbiguousType { class: String, ty: Ty },
    /// A class method is used on a signature-quantified type variable whose
    /// class the function's declared context does not provide, e.g.
    /// `poly :: a -> String; poly x = show x` (needs `Show a`, none declared).
    /// There is no instance for a bare rigid variable, so the evidence is
    /// missing and it must be rejected.
    MissingContextConstraint { class: String, ty: Ty },
    /// A type reference names a type that was never defined: no builtin,
    /// data/newtype declaration, type alias, or type family has this name.
    /// Rejected at the reference so it cannot flow downstream as an opaque
    /// type and resurface as a misleading error (e.g. a missing Show
    /// instance on a type that does not exist).
    UnknownType(String),
    Other(String),
}

/// Render the i-th friendly type-variable name: a, b, … z, a1, b1, …
fn pretty_var_name(i: usize) -> String {
    let letter = (b'a' + (i % 26) as u8) as char;
    let suffix = i / 26;
    if suffix == 0 { letter.to_string() } else { format!("{}{}", letter, suffix) }
}

/// Build a substitution that renames internal unification variables (names
/// starting with '_', e.g. `_i700`) to friendly letters `a, b, c, …`. The map
/// is shared across all the given types so the same variable reads the same on
/// every side of the message. User-written variable names (which don't start
/// with '_') are left untouched, and friendly letters skip any they already use.
fn pretty_var_subst(tys: &[&Ty]) -> Subst {
    let mut vars: Vec<TyVar> = Vec::new();
    for t in tys {
        for v in t.free_vars() {
            if !vars.contains(&v) { vars.push(v); }
        }
    }
    let mut used: std::collections::HashSet<String> = vars
        .iter()
        .filter(|v| !v.name.starts_with('_'))
        .map(|v| v.name.clone())
        .collect();
    let mut map = HashMap::new();
    let mut counter = 0usize;
    for v in &vars {
        if !v.name.starts_with('_') { continue; }
        let name = loop {
            let candidate = pretty_var_name(counter);
            counter += 1;
            if !used.contains(&candidate) { break candidate; }
        };
        used.insert(name.clone());
        map.insert(v.clone(), Ty::Var(TyVar { name, id: v.id }));
    }
    Subst::from_map(map)
}

/// True when one side is `String` and the other is a list — mata-ll's most
/// common confusion, since String is not `[Char]` here.
fn is_string_list_mismatch(a: &Ty, b: &Ty) -> bool {
    let is_str = |t: &Ty| matches!(t, Ty::Con(n) if n == "String");
    let is_list = |t: &Ty| matches!(t, Ty::List(_));
    (is_str(a) && is_list(b)) || (is_list(a) && is_str(b))
}

impl TypeError {
    /// An optional mata-ll-specific explanation appended below the error.
    fn hint(&self) -> Option<&'static str> {
        match &self.kind {
            TypeErrorKind::Mismatch(a, b) | TypeErrorKind::RigidMismatch(a, b)
                if is_string_list_mismatch(a, b) =>
                Some("in mata-ll String is not a list of characters — it is an opaque \
                      type that does not unify with [a]. A String cannot be passed where \
                      a list is expected, and list functions (++, map, length, …) do not \
                      accept it."),
            TypeErrorKind::NoInstance { class, ty }
                if class == "Ord" && matches!(ty, Ty::Tuple(_) | Ty::List(_) | Ty::App(_, _)) =>
                Some("mata-ll has no Ord instance for tuples, lists, or Maybe; compare their \
                      components individually."),
            TypeErrorKind::NoInstance { ty, .. }
                if matches!(ty, Ty::Arrow(_, _) | Ty::IO(_) | Ty::LuaIO(_, _)) =>
                Some("functions and IO actions have no Show/Eq/Ord instance — there is no \
                      way to render or compare them."),
            TypeErrorKind::AmbiguousType { .. } =>
                Some("add a type annotation to pin the type down, e.g. \
                      `show (Nothing :: Maybe Integer)`. GHC rejects this the same way."),
            TypeErrorKind::MissingContextConstraint { .. } =>
                Some("a bare polymorphic variable has no instance unless the signature \
                      requires one. GHC reports this as \"add (C a) to the context\"."),
            TypeErrorKind::UnknownType(name) => match name.as_str() {
                "Boolean" =>
                    Some("the boolean type is spelled 'Bool', as in Haskell."),
                "Char" =>
                    Some("mata-ll has no Char type — String is opaque, not [Char]. \
                          Individual characters are Integer byte codes; see strByte and \
                          strChar in LString."),
                "Double" | "Float" =>
                    Some("the floating-point type is spelled 'Number' (Lua's number type); \
                          GHC's Double and Float do not exist in mata-ll."),
                _ => None,
            },
            _ => None,
        }
    }
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            TypeErrorKind::Mismatch(a, b) => {
                let s = pretty_var_subst(&[a, b]);
                write!(f, "Cannot unify '{}' with '{}'", a.apply_subst(&s), b.apply_subst(&s))?
            }
            TypeErrorKind::RigidMismatch(a, b) => {
                let s = pretty_var_subst(&[a, b]);
                write!(f, "Cannot match '{}' with '{}': rank-2 polymorphism requires a polymorphic argument",
                    a.apply_subst(&s), b.apply_subst(&s))?
            }
            TypeErrorKind::OccursCheck(v, ty) => {
                let vt = Ty::Var(v.clone());
                let s = pretty_var_subst(&[&vt, ty]);
                write!(f, "Infinite type: {} occurs in {}", vt.apply_subst(&s), ty.apply_subst(&s))?
            }
            TypeErrorKind::UnboundVariable(name) =>
                write!(f, "Unbound variable: {}", name)?,
            TypeErrorKind::UnboundConstructor(name) =>
                write!(f, "Unknown constructor: {}", name)?,
            TypeErrorKind::PatternArgCount { constructor, expected, got } =>
                write!(f, "Constructor {} expects {} args, got {}",
                    constructor, expected, got)?,
            TypeErrorKind::NonExhaustive(name) =>
                write!(f, "Non-exhaustive patterns in {}", name)?,
            TypeErrorKind::TypeSigMismatch { name, declared, inferred } => {
                let s = pretty_var_subst(&[declared, inferred]);
                write!(f, "Type signature for '{}' doesn't match: declared {}, inferred {}",
                    name, declared.apply_subst(&s), inferred.apply_subst(&s))?
            }
            TypeErrorKind::NoInstance { class, ty } => {
                let s = pretty_var_subst(&[ty]);
                let rendered = ty.apply_subst(&s);
                let shown = match &rendered {
                    Ty::Arrow(_, _) | Ty::App(_, _) | Ty::IO(_) | Ty::LuaIO(_, _) =>
                        format!("({})", rendered),
                    _ => format!("{}", rendered),
                };
                write!(f, "No instance for '{} {}'", class, shown)?
            }
            TypeErrorKind::AmbiguousType { class, ty } => {
                let s = pretty_var_subst(&[ty]);
                let rendered = ty.apply_subst(&s);
                let shown = match &rendered {
                    Ty::Arrow(_, _) | Ty::App(_, _) | Ty::IO(_) | Ty::LuaIO(_, _) =>
                        format!("({})", rendered),
                    _ => format!("{}", rendered),
                };
                write!(f, "Ambiguous type: nothing here determines the type '{}', so no '{}' instance can be chosen for it",
                    shown, class)?
            }
            TypeErrorKind::MissingContextConstraint { class, ty } => {
                // Show the signature variable's written name: a freshened rigid
                // variable is `<name><id>` (e.g. `a519`), so trim the trailing id
                // digits back to what the user wrote (`a`).
                let v = match ty {
                    Ty::Var(tv) => tv.name.trim_end_matches(|c: char| c.is_ascii_digit()).to_string(),
                    other => format!("{}", other),
                };
                let v = if v.is_empty() { "a".to_string() } else { v };
                write!(f, "No instance for '{} {}': the type variable '{}' is only as general as the signature says, and the signature does not require '{} {}'. Add it to the context, e.g. '({} {}) => …'",
                    class, v, v, class, v, class, v)?
            }
            TypeErrorKind::UnknownType(name) =>
                write!(f, "Unknown type '{}': nothing in this program or its imports defines a type with this name — it is not a builtin, and no data, newtype, type alias, or type family declaration for it is in scope", name)?,
            TypeErrorKind::Other(msg) => write!(f, "{}", msg)?,
        }
        if let Some(ctx) = &self.context {
            if let Some(span) = &self.span {
                write!(f, "\n  at {}:{}, in {}", span.line, span.col, ctx)?;
            } else {
                write!(f, "\n  in {}", ctx)?;
            }
        } else if let Some(span) = &self.span {
            write!(f, "\n  at {}:{}", span.line, span.col)?;
        }
        if let Some(hint) = self.hint() {
            write!(f, "\n  note: {}", hint)?;
        }
        Ok(())
    }
}
