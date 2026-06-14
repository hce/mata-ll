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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Rank-2 forall: forall s. ty (limited to scope variables)
    Forall(TyVar, Box<Ty>),
    /// Tuple type: (a, b, c)
    Tuple(Vec<Ty>),
}

impl Ty {
    pub fn arrow(from: Ty, to: Ty) -> Ty {
        Ty::Arrow(Box::new(from), Box::new(to))
    }

    pub fn app(f: Ty, a: Ty) -> Ty {
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

    /// Collect all free type variables
    pub fn free_vars(&self) -> Vec<TyVar> {
        match self {
            Ty::Con(_) | Ty::Unit => vec![],
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
        stacker::maybe_grow(8 * 1024 * 1024, 8 * 1024 * 1024, || self.apply_subst_inner(subst))
    }

    fn apply_subst_inner(&self, subst: &Subst) -> Ty {
        match self {
            Ty::Con(_) | Ty::Unit => self.clone(),
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

    /// Check if a type variable occurs in this type (for occurs check)
    pub fn occurs(&self, v: &TyVar) -> bool {
        match self {
            Ty::Con(_) | Ty::Unit => false,
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
}

/// Unification: find a substitution that makes two types equal
pub fn unify(t1: &Ty, t2: &Ty) -> Result<Subst, TypeErrorKind> {
    stacker::maybe_grow(8 * 1024 * 1024, 8 * 1024 * 1024, || unify_inner(t1, t2))
}

fn unify_inner(t1: &Ty, t2: &Ty) -> Result<Subst, TypeErrorKind> {
    match (t1, t2) {
        (Ty::Con(a), Ty::Con(b)) if a == b => Ok(Subst::empty()),
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
        (Ty::Forall(v, inner), t) | (t, Ty::Forall(v, inner)) => {
            // The forall-bound variable is already a rigid skolem (id=MAX).
            // Unify the body directly — the variable will unify with whatever
            // the concrete type provides, enforcing that it can't escape.
            unify(inner, t)
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
    OccursCheck(TyVar, Ty),
    UnboundVariable(String),
    UnboundConstructor(String),
    PatternArgCount { constructor: String, expected: usize, got: usize },
    NonExhaustive(String),
    TypeSigMismatch { name: String, declared: Ty, inferred: Ty },
    Other(String),
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            TypeErrorKind::Mismatch(a, b) =>
                write!(f, "Cannot unify '{}' with '{}'", a, b)?,
            TypeErrorKind::OccursCheck(v, ty) =>
                write!(f, "Infinite type: {} occurs in {}", v, ty)?,
            TypeErrorKind::UnboundVariable(name) =>
                write!(f, "Unbound variable: {}", name)?,
            TypeErrorKind::UnboundConstructor(name) =>
                write!(f, "Unknown constructor: {}", name)?,
            TypeErrorKind::PatternArgCount { constructor, expected, got } =>
                write!(f, "Constructor {} expects {} args, got {}",
                    constructor, expected, got)?,
            TypeErrorKind::NonExhaustive(name) =>
                write!(f, "Non-exhaustive patterns in {}", name)?,
            TypeErrorKind::TypeSigMismatch { name, declared, inferred } =>
                write!(f, "Type signature for '{}' doesn't match: declared {}, inferred {}",
                    name, declared, inferred)?,
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
        Ok(())
    }
}
