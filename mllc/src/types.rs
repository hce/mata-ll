use std::collections::{HashMap, HashSet};
use std::fmt;

/// A typeclass constraint on a type variable, e.g. Show a
#[derive(Debug, Clone)]
pub struct TyConstraint {
    pub class_name: String,
    pub type_var: String,
}

/// Kind of a type expression. Kinds classify types the way types classify
/// values: a complete type (`Int`, `Maybe String`) has kind `Type`, and a
/// type constructor that still needs arguments has an arrow kind (`Maybe` is
/// `Type -> Type`, `Either` is `Type -> Type -> Type`). Kinds are written the
/// way GHC writes them (`Type`, `Type -> Type`); mata-ll has no surface
/// syntax for kind annotations — every kind is inferred (see
/// typechecker/kind.rs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// Regular types: Int, String, Maybe Int
    Type,
    /// Type-level string literals (used in FFI type families)
    Symbol,
    /// Function type constructor: Type -> Type (e.g., Maybe, [])
    Arrow(Box<Kind>, Box<Kind>),
    /// The kind a monomorphic data type promotes to under DataKinds: `data Nat
    /// = Z | S Nat` gives the kind `Nat`, inhabited by the promoted
    /// constructors `'Z :: Nat` and `'S :: Nat -> Nat`. The string is the data
    /// type's name. (Only parameterless, non-GADT, non-existential data types
    /// promote to a real kind — that keeps promotion monomorphic, with no kind
    /// polymorphism; every other data type keeps the `Type` approximation.)
    Promoted(String),
    /// A kind-unification variable, used only DURING kind inference
    /// (typechecker/kind.rs). Every variable left unconstrained when a
    /// declaration has been fully walked is defaulted to `Type` — exactly
    /// GHC's Haskell-2010 kind defaulting — so no `Var` ever survives into
    /// the registered kind tables or into a diagnostic.
    Var(u32),
}

impl Kind {
    /// Build `k1 -> k2`.
    pub fn arrow(from: Kind, to: Kind) -> Kind {
        Kind::Arrow(Box::new(from), Box::new(to))
    }

    /// Number of arguments this kind still expects: `Type` is 0,
    /// `Type -> Type` is 1, `(Type -> Type) -> Type` is 1.
    pub fn arity(&self) -> usize {
        match self {
            Kind::Arrow(_, rest) => 1 + rest.arity(),
            _ => 0,
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Kind::Type => write!(f, "Type"),
            Kind::Symbol => write!(f, "Symbol"),
            // Arrow kinds are right-associative like arrow types, so a
            // higher-kinded argument needs parentheses: (Type -> Type) -> Type.
            Kind::Arrow(a, b) => match a.as_ref() {
                Kind::Arrow(..) => write!(f, "({}) -> {}", a, b),
                _ => write!(f, "{} -> {}", a, b),
            },
            Kind::Promoted(name) => write!(f, "{}", name),
            Kind::Var(id) => write!(f, "k{}", id),
        }
    }
}

/// Multiplicity of a function arrow (linear types): how often the function
/// may USE the argument this arrow binds. `a -> b` is `a %Many -> b` (no
/// restriction, every plain arrow); `a %1 -> b` promises the argument is
/// consumed EXACTLY once (linear — GHC's LinearTypes semantics: a second
/// use is an error and so is dropping it; see typechecker/usage.rs for the
/// enforcement and its boundary). Multiplicity is a type-CHECKING discipline only: it is never
/// consulted after type checking, so monomorphization, codegen and the
/// emitted Lua are identical with or without `%1` annotations.
///
/// `Var` is a multiplicity unification variable. Arrows the inference engine
/// invents (the expected arrow at an application, a lambda's own arrows) get
/// a fresh one so they adopt whichever multiplicity the program unifies them
/// with — that is how a lambda checked against a `%1` parameter learns its
/// binder is linear. A variable left unconstrained means no `%1` annotation
/// ever reached the arrow, and every consumer treats it as `Many`.
///
/// `Rigid` is a NAMED multiplicity variable from a signature (`a %m -> b`):
/// the multiplicity-polymorphism counterpart of a signature type variable.
/// Inside the definition that declared it, it is rigid — it unifies only
/// with itself, so the body cannot silently specialize `%m` to `One` or
/// `Many` behind the callers' backs, and the usage checker treats a binder
/// bound at a `%m` arrow pessimistically (a caller may instantiate `m` to
/// `1`). At every USE of the definition, `Scheme` instantiation replaces it
/// with a fresh flexible `Var`, which then adopts whatever the call site
/// provides — that is how `apply :: (a %m -> b) -> a %m -> b` keeps a `%1`
/// argument linear while staying usable with unrestricted functions.
///
/// Equality and hashing are deliberately multiplicity-BLIND (see the manual
/// impls below): multiplicity must never change a type's identity, so every
/// existing `Ty` comparison, map key and cache behaves exactly as it did
/// before multiplicities existed. Only `unify` (which handles the slot
/// explicitly) and the linear-usage checker ever look at it.
#[derive(Debug, Clone, Copy)]
pub enum Mult {
    /// `a %1 -> b`: the argument must be consumed exactly once (linear).
    One,
    /// `a -> b` / `a %Many -> b`: unrestricted (GHC's ω).
    Many,
    /// A multiplicity unification variable (ids from `Checker::fresh_mult`,
    /// a namespace separate from type-variable ids).
    Var(u32),
    /// A named signature multiplicity variable (`a %m -> b`), rigid inside
    /// its own definition and freshened to a flexible `Var` at each use (ids
    /// share the `fresh_mult` counter, so an id is only ever one flavor).
    Rigid(u32),
}

// Multiplicity-blind identity: any Mult equals any Mult, and hashing adds
// nothing. This keeps `Ty`'s derived Eq/Hash exactly as they were before the
// multiplicity slot existed — a `%1` arrow and a plain arrow are the SAME
// type for lookup/caching purposes, and only unification distinguishes them.
impl PartialEq for Mult {
    fn eq(&self, _other: &Mult) -> bool {
        true
    }
}
impl Eq for Mult {}
impl std::hash::Hash for Mult {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

/// Internal type representation used by the type checker.
/// Separate from the AST's Type to allow for unification variables.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Ty {
    /// Concrete type: Int, String, Bool, Number
    Con(String),
    /// Type variable (rigid or unification)
    Var(TyVar),
    /// Function type: a -> b (the `Mult` is the arrow's multiplicity;
    /// plain `->` is `Mult::Many`)
    Arrow(Box<Ty>, Box<Ty>, Mult),
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
        Ty::Arrow(Box::new(from), Box::new(to), Mult::Many)
    }

    /// Build an arrow with an explicit multiplicity (`%1`, or a fresh
    /// multiplicity variable for inference-invented arrows).
    pub fn arrow_m(from: Ty, to: Ty, mult: Mult) -> Ty {
        Ty::Arrow(Box::new(from), Box::new(to), mult)
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
            Ty::Arrow(_, rest, _) => 1 + rest.arrow_arity(),
            _ => 0,
        }
    }

    /// The final result type after peeling all top-level arrows.
    pub fn return_type(&self) -> &Ty {
        match self {
            Ty::Arrow(_, rest, _) => rest.return_type(),
            other => other,
        }
    }

    /// Split a function type into its argument types and final result.
    /// `a -> b -> c` becomes `([a, b], c)`.
    pub fn peel_arrows(&self) -> (Vec<&Ty>, &Ty) {
        let mut args = Vec::new();
        let mut cur = self;
        while let Ty::Arrow(a, b, _) = cur {
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
            Ty::Arrow(a, b, _) | Ty::App(a, b) => {
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

    /// Collect the ids of every rigid multiplicity variable (`Mult::Rigid`)
    /// on this type's arrows. These are what `Checker::generalize` quantifies
    /// (`Scheme::mult_vars`) and instantiation freshens; flexible `Mult::Var`s
    /// are never quantified — a definition is only checked against the
    /// polymorphic reading of a multiplicity when the variable was rigid
    /// while its body was checked (see `Mult`).
    pub fn collect_rigid_mults(&self, out: &mut Vec<u32>) {
        match self {
            Ty::Con(_) | Ty::Unit | Ty::Promoted(_) | Ty::Skolem(..) | Ty::Var(_) => {}
            Ty::Arrow(a, b, m) => {
                if let Mult::Rigid(id) = m
                    && !out.contains(id) {
                        out.push(*id);
                    }
                a.collect_rigid_mults(out);
                b.collect_rigid_mults(out);
            }
            Ty::App(a, b) => {
                a.collect_rigid_mults(out);
                b.collect_rigid_mults(out);
            }
            Ty::List(a) | Ty::IO(a) | Ty::LuaIO(_, a) | Ty::Forall(_, a) =>
                a.collect_rigid_mults(out),
            Ty::Tuple(elems) => for e in elems { e.collect_rigid_mults(out); },
        }
    }

    /// Collect the ids of EVERY multiplicity variable on this type's arrows —
    /// flexible (`Mult::Var`) and rigid (`Mult::Rigid`) alike. This is the
    /// "could a substitution's multiplicity bindings rewrite this type?"
    /// footprint: `apply_subst` resolves both kinds through `resolve_mult`, so
    /// both belong in the cache the environment uses to skip untouched
    /// schemes (`TypeEnv`). Contrast `collect_rigid_mults`, which answers the
    /// narrower generalization question.
    pub fn collect_mult_ids(&self, out: &mut Vec<u32>) {
        match self {
            Ty::Con(_) | Ty::Unit | Ty::Promoted(_) | Ty::Skolem(..) | Ty::Var(_) => {}
            Ty::Arrow(a, b, m) => {
                if let Mult::Var(id) | Mult::Rigid(id) = m
                    && !out.contains(id) {
                        out.push(*id);
                    }
                a.collect_mult_ids(out);
                b.collect_mult_ids(out);
            }
            Ty::App(a, b) => {
                a.collect_mult_ids(out);
                b.collect_mult_ids(out);
            }
            Ty::List(a) | Ty::IO(a) | Ty::LuaIO(_, a) | Ty::Forall(_, a) =>
                a.collect_mult_ids(out),
            Ty::Tuple(elems) => for e in elems { e.collect_mult_ids(out); },
        }
    }

    /// Conservative "could `subst` change this type at all?" test: true when
    /// the type mentions any variable, LuaIO scope, or arrow multiplicity in
    /// the substitution's domain. Used to skip the clone-heavy `apply_subst`
    /// when it would be an identity — false here GUARANTEES identity, while
    /// a true may still be an identity (e.g. a `Forall` shadowing the bound
    /// variable), which merely falls back to the full application.
    pub fn mentions_subst(&self, subst: &Subst) -> bool {
        if subst.map.is_empty() && subst.mults.is_empty() {
            return false;
        }
        match self {
            Ty::Con(_) | Ty::Unit | Ty::Promoted(_) | Ty::Skolem(..) => false,
            Ty::Var(v) => subst.map.contains_key(v),
            Ty::Arrow(a, b, m) => {
                (matches!(m, Mult::Var(id) | Mult::Rigid(id) if subst.mults.contains_key(id)))
                    || a.mentions_subst(subst) || b.mentions_subst(subst)
            }
            Ty::App(a, b) => a.mentions_subst(subst) || b.mentions_subst(subst),
            Ty::List(a) | Ty::IO(a) => a.mentions_subst(subst),
            Ty::LuaIO(s, a) => subst.map.contains_key(s) || a.mentions_subst(subst),
            // Conservative: the bound variable shadows a binding of the same
            // name, but treating that rare case as "mentioned" only costs a
            // redundant full application (which restricts correctly).
            Ty::Forall(v, inner) => subst.map.contains_key(v) || inner.mentions_subst(subst),
            Ty::Tuple(elems) => elems.iter().any(|e| e.mentions_subst(subst)),
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
            Ty::Arrow(a, b, m) => Ty::arrow_m(
                a.apply_subst(subst), b.apply_subst(subst), subst.resolve_mult(*m)),
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

    /// Replace signature skolem constants by the types they demote to, keyed
    /// by skolem id. Used ONCE per function, after its body has been checked
    /// rigidly against the skolemized signature, to turn each signature skolem
    /// back into the fresh flexible variable the caller-visible type and every
    /// downstream pass expect. A skolem id not in the map is left rigid (an
    /// existential skolem, which must never be demoted). Kept off the hot
    /// `Subst` path so the substitution type carried on every inference frame
    /// stays small.
    pub fn demote_skolems(&self, demote: &HashMap<u32, Ty>) -> Ty {
        match self {
            Ty::Skolem(_, id) => match demote.get(id) {
                Some(t) => t.clone(),
                None => self.clone(),
            },
            Ty::Con(_) | Ty::Unit | Ty::Promoted(_) | Ty::Var(_) => self.clone(),
            Ty::Arrow(a, b, m) =>
                Ty::Arrow(Box::new(a.demote_skolems(demote)), Box::new(b.demote_skolems(demote)), *m),
            Ty::App(a, b) => Ty::app(a.demote_skolems(demote), b.demote_skolems(demote)),
            Ty::List(a) => Ty::list(a.demote_skolems(demote)),
            Ty::IO(a) => Ty::io(a.demote_skolems(demote)),
            Ty::LuaIO(s, a) => Ty::lua_io(s.clone(), a.demote_skolems(demote)),
            Ty::Forall(v, inner) => Ty::Forall(v.clone(), Box::new(inner.demote_skolems(demote))),
            Ty::Tuple(elems) => Ty::Tuple(elems.iter().map(|e| e.demote_skolems(demote)).collect()),
        }
    }

    /// Check if a specific skolem occurs in this type (for escape check)
    pub fn contains_skolem(&self, name: &str, id: u32) -> bool {
        match self {
            Ty::Skolem(n, i) => n == name && *i == id,
            Ty::Con(_) | Ty::Unit | Ty::Promoted(_) | Ty::Var(_) => false,
            Ty::Arrow(a, b, _) | Ty::App(a, b) => a.contains_skolem(name, id) || b.contains_skolem(name, id),
            Ty::List(a) | Ty::IO(a) => a.contains_skolem(name, id),
            Ty::LuaIO(_, a) => a.contains_skolem(name, id),
            Ty::Forall(_, inner) => inner.contains_skolem(name, id),
            Ty::Tuple(elems) => elems.iter().any(|e| e.contains_skolem(name, id)),
        }
    }

    /// Collect every distinct skolem constant occurring in this type, as
    /// (name, id) pairs. Used to attach provenance notes to diagnostics that
    /// mention a skolem (existential unpacking vs rank-2 sealing).
    pub fn collect_skolems(&self, out: &mut Vec<(String, u32)>) {
        match self {
            Ty::Skolem(n, i) => {
                if !out.iter().any(|(on, oi)| on == n && oi == i) {
                    out.push((n.clone(), *i));
                }
            }
            Ty::Con(_) | Ty::Unit | Ty::Promoted(_) | Ty::Var(_) => {}
            Ty::Arrow(a, b, _) | Ty::App(a, b) => {
                a.collect_skolems(out);
                b.collect_skolems(out);
            }
            Ty::List(a) | Ty::IO(a) | Ty::LuaIO(_, a) | Ty::Forall(_, a) => a.collect_skolems(out),
            Ty::Tuple(elems) => for e in elems { e.collect_skolems(out); },
        }
    }

    /// Check if a type variable occurs in this type (for occurs check)
    pub fn occurs(&self, v: &TyVar) -> bool {
        match self {
            Ty::Con(_) | Ty::Unit | Ty::Promoted(_) | Ty::Skolem(..) => false,
            Ty::Var(w) => v == w,
            Ty::Arrow(a, b, _) | Ty::App(a, b) => a.occurs(v) || b.occurs(v),
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
            Ty::Arrow(a, b, m) => {
                // A `%1` arrow renders its annotation; `Many` and an
                // unconstrained multiplicity variable render as the plain
                // arrow they behave as. A rigid multiplicity variable renders
                // as the conventional `%m` (its source name is not carried).
                let arrow = match m {
                    Mult::One => "%1 ->",
                    Mult::Rigid(_) => "%m ->",
                    _ => "->",
                };
                match a.as_ref() {
                    Ty::Arrow(..) => write!(f, "({}) {} {}", a, arrow, b),
                    _ => write!(f, "{} {} {}", a, arrow, b),
                }
            }
            Ty::App(a, b) => {
                match b.as_ref() {
                    Ty::App(_, _) | Ty::Arrow(..) => write!(f, "{} ({})", a, b),
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
/// "[a]", "[Int]"), which required prefix probes and positional guessing
/// to relate a use-site type to its instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum InstHead {
    /// A named type constructor: `Int`, `Maybe`, `Pair`, `IO`, `ST`, …
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
#[derive(Debug, Clone, Eq)]
pub struct TyVar {
    pub name: String,
    pub id: u32,
}

/// Identity is (name, id), as the old derives had it — but compare the id
/// first (a fresh variable's id is unique, so almost every inequality is
/// decided by one integer compare) and hash only the id plus a two-byte
/// digest of the name instead of siphashing the whole string. Type variables
/// key the checker's hottest maps (substitutions, environment footprints);
/// full string hashing was a measurable share of typechecking long
/// functions. Equal variables have equal ids and names, so they hash equal;
/// user-written variables (which all share `id: u32::MAX`) stay spread by
/// the name digest.
impl PartialEq for TyVar {
    fn eq(&self, other: &TyVar) -> bool {
        self.id == other.id && self.name == other.name
    }
}

impl std::hash::Hash for TyVar {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u32(self.id);
        state.write_u8(self.name.len() as u8);
        state.write_u8(self.name.as_bytes().first().copied().unwrap_or(0));
    }
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
    /// Quantified multiplicity variables: the `Mult::Rigid` ids of `ty` that
    /// belong to this scheme (multiplicity polymorphism, `a %m -> b`).
    /// Instantiation replaces each with a fresh flexible `Mult::Var`, so
    /// every use of the scheme picks its own multiplicity — exactly parallel
    /// to `vars`. Only RIGID multiplicities are ever quantified: a flexible
    /// `Mult::Var` left on a type was not checked polymorphically (its
    /// definition's usage accounting read it as `Many`), so freshening it
    /// per use would let a call site claim `%1` behavior the definition was
    /// never held to.
    pub mult_vars: Vec<u32>,
    pub ty: Ty,
}

impl Scheme {
    pub fn mono(ty: Ty) -> Scheme {
        Scheme { vars: vec![], mult_vars: vec![], ty }
    }

    pub fn apply_subst(&self, subst: &Subst) -> Scheme {
        // Don't substitute bound variables. Cloning the whole substitution
        // just to restrict it is expensive when the substitution is large
        // (it deep-clones every image), so only do it when the substitution
        // actually binds one of this scheme's bound variables.
        let needs_restrict = self.vars.iter().any(|v| subst.map.contains_key(v))
            || self.mult_vars.iter().any(|id| subst.mults.contains_key(id));
        let ty = if needs_restrict {
            let mut restricted = subst.clone();
            for v in &self.vars {
                restricted.remove(v);
            }
            for id in &self.mult_vars {
                restricted.remove_mult(*id);
            }
            self.ty.apply_subst(&restricted)
        } else {
            self.ty.apply_subst(subst)
        };
        Scheme {
            vars: self.vars.clone(),
            mult_vars: self.mult_vars.clone(),
            ty,
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

/// Substitution: mapping from type variables to types. Multiplicity
/// variables (see `Mult`) live in their own map: they are a separate
/// namespace from type variables and are only ever read back through
/// `resolve_mult` when a substitution is applied to an arrow type.
#[derive(Debug, Clone)]
pub struct Subst {
    map: HashMap<TyVar, Ty>,
    mults: HashMap<u32, Mult>,
}

impl Subst {
    pub fn empty() -> Subst {
        Subst { map: HashMap::new(), mults: HashMap::new() }
    }

    pub fn from_map(map: HashMap<TyVar, Ty>) -> Subst {
        Subst { map, mults: HashMap::new() }
    }

    /// A substitution over type variables AND multiplicity variables at once
    /// (scheme instantiation renames both namespaces in one application).
    pub fn from_parts(map: HashMap<TyVar, Ty>, mults: HashMap<u32, Mult>) -> Subst {
        Subst { map, mults }
    }

    /// True when this substitution binds no type variables (multiplicity-only
    /// substitutions still count as empty here; callers use it to skip a no-op
    /// merge of type bindings).
    pub fn is_type_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn singleton(v: TyVar, ty: Ty) -> Subst {
        let mut map = HashMap::new();
        map.insert(v, ty);
        Subst { map, mults: HashMap::new() }
    }

    /// A substitution binding one multiplicity variable.
    pub fn mult_singleton(id: u32, m: Mult) -> Subst {
        let mut mults = HashMap::new();
        mults.insert(id, m);
        Subst { map: HashMap::new(), mults }
    }

    /// Resolve a multiplicity through this substitution, following variable
    /// chains (with the same defensive depth cap as type-variable chains).
    /// `Rigid` ids are looked up too: unification never binds a rigid
    /// variable (see `unify_mult`), so the only bindings keyed by one are the
    /// fresh-variable renamings scheme instantiation builds — everywhere else
    /// a rigid variable resolves to itself.
    pub fn resolve_mult(&self, m: Mult) -> Mult {
        let mut cur = m;
        let mut depth = 0;
        while let Mult::Var(id) | Mult::Rigid(id) = cur {
            match self.mults.get(&id) {
                Some(next) => {
                    // A self-mapping or an over-long chain ends the walk.
                    if matches!(next, Mult::Var(nid) | Mult::Rigid(nid) if *nid == id) { break; }
                    depth += 1;
                    if depth > 100 { break; }
                    cur = *next;
                }
                None => break,
            }
        }
        cur
    }

    pub fn lookup(&self, v: &TyVar) -> Option<&Ty> {
        self.map.get(v)
    }

    pub fn remove(&mut self, v: &TyVar) {
        self.map.remove(v);
    }

    /// Drop any binding for one multiplicity-variable id (the multiplicity
    /// counterpart of `remove`, used by `Scheme::apply_subst` to keep a
    /// substitution away from the scheme's bound multiplicity variables).
    pub fn remove_mult(&mut self, id: u32) {
        self.mults.remove(&id);
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
        // Multiplicity bindings compose the same way: resolve self's images
        // through other, then take other's bindings for anything self left
        // unbound.
        let mut mults: HashMap<u32, Mult> = self.mults
            .iter()
            .map(|(k, v)| (*k, other.resolve_mult(*v)))
            .collect();
        for (k, v) in &other.mults {
            mults.entry(*k).or_insert(*v);
        }
        Subst { map: result, mults }
    }

    /// In-place `compose`: exactly `*self = self.compose(other)`, without
    /// rebuilding the map. `compose` clones every entry of `self` (key and
    /// image) to apply `other` to the images; on the accumulate-in-a-loop
    /// pattern (`subst = subst.compose(&s)` once per do-statement) that walk
    /// makes a long function quadratic in its length. Here an image is
    /// rewritten only when it actually mentions `other`'s domain
    /// (`Ty::mentions_subst`); an untouched image — the overwhelming case,
    /// since `other` binds this step's fresh variables — is left alone, and
    /// no keys are recloned. The resulting map is identical to `compose`'s:
    /// images stay flattened, so variable chains stay short.
    pub fn compose_with(&mut self, other: &Subst) {
        if other.map.is_empty() && other.mults.is_empty() {
            return;
        }
        for v in self.map.values_mut() {
            if v.mentions_subst(other) {
                *v = v.apply_subst(other);
            }
        }
        for (k, v) in &other.map {
            self.map.entry(k.clone()).or_insert_with(|| v.clone());
        }
        if !other.mults.is_empty() {
            for m in self.mults.values_mut() {
                *m = other.resolve_mult(*m);
            }
        }
        for (k, v) in &other.mults {
            self.mults.entry(*k).or_insert(*v);
        }
    }

    /// Does this substitution bind the given type variable?
    pub fn binds_var(&self, v: &TyVar) -> bool {
        self.map.contains_key(v)
    }

    /// Does this substitution bind the given multiplicity variable id?
    pub fn binds_mult(&self, id: u32) -> bool {
        self.mults.contains_key(&id)
    }

    /// The type variables this substitution binds.
    pub fn ty_domain(&self) -> impl Iterator<Item = &TyVar> {
        self.map.keys()
    }

    /// The multiplicity-variable ids this substitution binds.
    pub fn mult_domain(&self) -> impl Iterator<Item = u32> + '_ {
        self.mults.keys().copied()
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

/// A substitution that ACCUMULATES over a long sequence of composition steps
/// (one per do-block statement), with a reverse index over its images so each
/// step no longer walks the whole map.
///
/// `Subst::compose` (and the in-place `compose_with`) must rewrite every
/// stored image through the incoming substitution to keep images flattened —
/// an O(accumulated size) walk per step, which made typechecking a long
/// do-block quadratic in its length even after the walk stopped allocating.
/// Here each index maps a variable to the entry keys whose images might
/// mention it, so a step rewrites exactly the images the incoming
/// substitution can change (usually none: it binds the step's fresh
/// variables) and the composed result is IDENTICAL to `Subst::compose`'s —
/// images stay flattened, chains stay short.
///
/// Index entries are stale-tolerant: a rewritten image keeps its old index
/// entries (they are re-checked with `mentions_subst` and skipped when they
/// no longer apply, each at most once, since processing an incoming variable
/// consumes its index bucket).
#[derive(Debug, Clone)]
pub struct AccSubst {
    subst: Subst,
    /// type variable -> keys of `subst.map` whose image may mention it
    /// (free type variables and LuaIO scopes).
    var_index: HashMap<TyVar, Vec<TyVar>>,
    /// multiplicity id -> keys of `subst.map` whose image may carry it on an
    /// arrow (flexible or rigid: `apply_subst` resolves both).
    mult_index: HashMap<u32, Vec<TyVar>>,
    /// multiplicity id -> keys of `subst.mults` whose VALUE may reference it.
    mult_val_index: HashMap<u32, Vec<u32>>,
}

impl std::ops::Deref for AccSubst {
    type Target = Subst;
    fn deref(&self) -> &Subst { &self.subst }
}

impl Default for AccSubst {
    fn default() -> Self { Self::new() }
}

impl AccSubst {
    pub fn new() -> AccSubst {
        AccSubst {
            subst: Subst::empty(),
            var_index: HashMap::new(),
            mult_index: HashMap::new(),
            mult_val_index: HashMap::new(),
        }
    }

    /// The plain substitution, for handing the accumulated result onward.
    pub fn into_subst(self) -> Subst {
        self.subst
    }

    /// Record where image `img` (stored under key `k`) must be revisited: at
    /// every type variable it mentions and every arrow multiplicity it
    /// carries. A `Forall`-bound variable needs no entry — `apply_subst`
    /// restricts it away, so a binding of it cannot rewrite the image — and
    /// `free_vars` already excludes it.
    fn index_image(&mut self, k: &TyVar, img: &Ty) {
        for v in img.free_vars() {
            self.var_index.entry(v).or_default().push(k.clone());
        }
        let mut mults = Vec::new();
        img.collect_mult_ids(&mut mults);
        for id in mults {
            self.mult_index.entry(id).or_default().push(k.clone());
        }
    }

    fn index_mult_value(&mut self, k: u32, m: Mult) {
        if let Mult::Var(id) | Mult::Rigid(id) = m {
            self.mult_val_index.entry(id).or_default().push(k);
        }
    }

    /// `self = other ∘ self`, exactly as `Subst::compose` computes it, but
    /// touching only the entries `other` can affect (found via the indexes)
    /// instead of walking the whole accumulated map.
    pub fn compose_with(&mut self, other: &Subst) {
        if other.map.is_empty() && other.mults.is_empty() {
            return;
        }
        // Images that may mention other's domain: consume the index buckets
        // of every incoming variable. A key can sit in several buckets, so
        // dedup before rewriting.
        let mut cand: Vec<TyVar> = Vec::new();
        for w in other.map.keys() {
            if let Some(ks) = self.var_index.remove(w) {
                cand.extend(ks);
            }
        }
        for id in other.mults.keys() {
            if let Some(ks) = self.mult_index.remove(id) {
                cand.extend(ks);
            }
        }
        if !cand.is_empty() {
            let mut seen: HashSet<TyVar> = HashSet::with_capacity(cand.len());
            for k in cand {
                if !seen.insert(k.clone()) {
                    continue;
                }
                let Some(img) = self.subst.map.get(&k) else { continue };
                if !img.mentions_subst(other) {
                    continue; // stale index entry; drops here, checked once
                }
                let new_img = img.apply_subst(other);
                self.index_image(&k, &new_img);
                self.subst.map.insert(k, new_img);
            }
        }
        // Multiplicity values that may reference other's domain.
        let mut mcand: Vec<u32> = Vec::new();
        for id in other.mults.keys() {
            if let Some(ks) = self.mult_val_index.remove(id) {
                mcand.extend(ks);
            }
        }
        if !mcand.is_empty() {
            mcand.sort_unstable();
            mcand.dedup();
            for k in mcand {
                if let Some(m) = self.subst.mults.get(&k).copied() {
                    let resolved = other.resolve_mult(m);
                    self.index_mult_value(k, resolved);
                    self.subst.mults.insert(k, resolved);
                }
            }
        }
        // Adopt other's bindings for anything self leaves unbound (compose
        // keeps self's binding on conflict). Adopted images are stored AS IS,
        // exactly as `compose` stores them.
        for (k, v) in &other.map {
            if !self.subst.map.contains_key(k) {
                self.index_image(k, v);
                self.subst.map.insert(k.clone(), v.clone());
            }
        }
        for (k, v) in &other.mults {
            if !self.subst.mults.contains_key(k) {
                self.index_mult_value(*k, *v);
                self.subst.mults.insert(*k, *v);
            }
        }
    }
}

/// Closed type-family equations in `Ty` form, used to reduce family
/// applications DURING unification (not just at AST-conversion time). Each
/// family maps to its equations in declaration order; an equation is
/// `(argument patterns, result)`, both already lowered to `Ty` with the
/// equation's pattern variables as `Ty::Var`. Built once by the typechecker
/// after type families are registered (see `Checker::build_ty_families`); the
/// unifier holds a borrow of it.
#[derive(Debug, Clone, Default)]
pub struct TyFamilies {
    eqs: HashMap<String, Vec<(Vec<Ty>, Ty)>>,
}

impl TyFamilies {
    pub fn new() -> TyFamilies {
        TyFamilies { eqs: HashMap::new() }
    }
    pub fn insert(&mut self, name: String, equations: Vec<(Vec<Ty>, Ty)>) {
        self.eqs.insert(name, equations);
    }
    pub fn is_empty(&self) -> bool {
        self.eqs.is_empty()
    }
    pub fn len(&self) -> usize {
        self.eqs.len()
    }
    fn get(&self, name: &str) -> Option<&Vec<(Vec<Ty>, Ty)>> {
        self.eqs.get(name)
    }
    fn contains(&self, name: &str) -> bool {
        self.eqs.contains_key(name)
    }
}

/// Reduction fuel for one `tf_normalize` call: an upper bound on the number of
/// family-reduction steps, so a non-terminating family (`Loop x = Loop x`)
/// is reported as a divergence instead of looping (or overflowing the stack)
/// forever. Real programs reduce a small, bounded number of steps.
const TF_FUEL: u32 = 100_000;

/// Peel an application spine `F a1 a2 … an` into `(F, [a1, …, an])`.
fn peel_app(ty: &Ty) -> (&Ty, Vec<&Ty>) {
    let mut head = ty;
    let mut args = Vec::new();
    while let Ty::App(f, a) = head {
        args.push(a.as_ref());
        head = f.as_ref();
    }
    args.reverse();
    (head, args)
}

/// Is `ty` an application headed by a type family that did NOT reduce — i.e.
/// a STUCK family application? (Its outermost head is a family name, but no
/// equation matched, typically because a scrutinee position holds a type
/// variable.) Such an application is irreducible for now and must NOT be
/// unified structurally: a family is not assumed injective, so `F a ~ F b`
/// may not conclude `a ~ b`. It behaves like an opaque, rigid-ish type that
/// only unifies with a syntactically identical one (or binds a variable).
fn is_stuck_family_app(ty: &Ty, fams: &TyFamilies) -> bool {
    let (head, _args) = peel_app(ty);
    matches!(head, Ty::Con(name) if fams.contains(name))
}

/// Match a family-equation pattern against an actual `Ty` argument, binding
/// the pattern's variables (by NAME — an equation's variables are its own
/// fresh names). A pattern constructor/promoted/app position requires the
/// actual to have that exact shape: a `Ty::Var` in the actual where the
/// pattern expects `'Z` or `'S _` does NOT match, which is exactly how a
/// closed family gets "stuck" on an unknown type variable rather than
/// committing to an equation. Non-linear patterns (a variable used twice)
/// must bind consistently.
fn tf_match(pat: &Ty, actual: &Ty, binds: &mut HashMap<String, Ty>) -> bool {
    match pat {
        Ty::Var(v) => match binds.get(&v.name) {
            Some(prev) => prev == actual,
            None => {
                binds.insert(v.name.clone(), actual.clone());
                true
            }
        },
        Ty::Con(n) => matches!(actual, Ty::Con(m) if m == n),
        Ty::Promoted(n) => matches!(actual, Ty::Promoted(m) if m == n),
        Ty::Unit => matches!(actual, Ty::Unit),
        Ty::App(pf, pa) => matches!(actual, Ty::App(af, aa)
            if tf_match(pf, af, binds) && tf_match(pa, aa, binds)),
        Ty::List(pe) => matches!(actual, Ty::List(ae) if tf_match(pe, ae, binds)),
        Ty::IO(pe) => matches!(actual, Ty::IO(ae) if tf_match(pe, ae, binds)),
        Ty::Tuple(ps) => matches!(actual, Ty::Tuple(as_)
            if ps.len() == as_.len()
                && ps.iter().zip(as_).all(|(p, a)| tf_match(p, a, binds))),
        // Arrows, foralls, skolems etc. in a family pattern are not supported
        // as matchable shapes — treat as non-matching (stuck).
        _ => false,
    }
}

/// Substitute an equation's matched pattern variables into its result type.
fn tf_subst(ty: &Ty, binds: &HashMap<String, Ty>) -> Ty {
    match ty {
        Ty::Var(v) => binds.get(&v.name).cloned().unwrap_or_else(|| ty.clone()),
        Ty::App(f, a) => Ty::app(tf_subst(f, binds), tf_subst(a, binds)),
        Ty::Arrow(a, b, m) => Ty::arrow_m(tf_subst(a, binds), tf_subst(b, binds), *m),
        Ty::List(e) => Ty::list(tf_subst(e, binds)),
        Ty::IO(e) => Ty::io(tf_subst(e, binds)),
        Ty::LuaIO(s, e) => Ty::lua_io(s.clone(), tf_subst(e, binds)),
        Ty::Tuple(es) => Ty::Tuple(es.iter().map(|e| tf_subst(e, binds)).collect()),
        Ty::Forall(v, inner) => Ty::Forall(v.clone(), Box::new(tf_subst(inner, binds))),
        Ty::Con(_) | Ty::Promoted(_) | Ty::Unit | Ty::Skolem(..) => ty.clone(),
    }
}

/// Could `pat` and `actual` be made EQUAL by some substitution of their
/// variables? The negation is GHC's *apartness*: a closed-family clause may
/// only be skipped in favor of a later one when the argument can never come
/// to match it. Variables on either side are flexible and bind through one
/// shared map (pattern variables are renamed apart first — an equation's
/// names are its own and may collide with the argument's). A family
/// application on the ARGUMENT side is a wildcard: it could reduce to
/// anything later, so it is never apart from anything. Conservative in the
/// stuck direction — when unsure, the reduction stays stuck rather than
/// committing to a possibly-wrong later clause.
fn tf_maybe_unifiable(pat: &Ty, actual: &Ty, binds: &mut HashMap<String, Ty>, fams: &TyFamilies) -> bool {
    // Resolve a variable through the binding map (one step at a time).
    fn walk(t: &Ty, binds: &HashMap<String, Ty>) -> Ty {
        let mut cur = t.clone();
        while let Ty::Var(v) = &cur {
            match binds.get(&v.name) {
                Some(next) if next != &cur => cur = next.clone(),
                _ => break,
            }
        }
        cur
    }
    let a = walk(pat, binds);
    let b = walk(actual, binds);
    if is_stuck_family_app(&b, fams) || is_stuck_family_app(&a, fams) {
        return true;
    }
    match (&a, &b) {
        // A skolem is a rigid but UNKNOWN type — the caller chooses it. For
        // apartness it behaves exactly like a variable: it could later turn out
        // to be `'Z` (or anything), so it is never apart from a pattern, and a
        // closed family applied to it must stay STUCK rather than fall through
        // to a catch-all clause. (A concrete argument would have reduced an
        // earlier clause; a skolem cannot, so the family is irreducible on it.)
        (Ty::Skolem(..), _) | (_, Ty::Skolem(..)) => true,
        (Ty::Var(v), other) | (other, Ty::Var(v)) => {
            // Bind (no occurs check: an occurs failure would make them
            // apart, but claiming "maybe unifiable" only errs toward stuck).
            if other == &Ty::Var(v.clone()) {
                return true;
            }
            binds.insert(v.name.clone(), other.clone());
            true
        }
        (Ty::Con(x), Ty::Con(y)) => x == y,
        (Ty::Promoted(x), Ty::Promoted(y)) => x == y,
        (Ty::Unit, Ty::Unit) => true,
        (Ty::App(f1, a1), Ty::App(f2, a2)) => {
            tf_maybe_unifiable(f1, f2, binds, fams) && tf_maybe_unifiable(a1, a2, binds, fams)
        }
        (Ty::List(x), Ty::List(y)) | (Ty::IO(x), Ty::IO(y)) => {
            tf_maybe_unifiable(x, y, binds, fams)
        }
        (Ty::Tuple(xs), Ty::Tuple(ys)) if xs.len() == ys.len() => {
            xs.iter().zip(ys).all(|(x, y)| tf_maybe_unifiable(x, y, binds, fams))
        }
        (Ty::Arrow(f1, a1, _), Ty::Arrow(f2, a2, _)) => {
            tf_maybe_unifiable(f1, f2, binds, fams) && tf_maybe_unifiable(a1, a2, binds, fams)
        }
        // Different shapes (constructor vs list, promoted vs con, …): apart.
        _ => false,
    }
}

/// Rename an equation pattern's variables so they can never collide with the
/// argument's variables inside the shared apartness binding map.
fn tf_rename_pat_vars(ty: &Ty) -> Ty {
    match ty {
        Ty::Var(v) => Ty::Var(TyVar { name: format!("__tfpat_{}", v.name), id: v.id }),
        Ty::App(f, a) => Ty::app(tf_rename_pat_vars(f), tf_rename_pat_vars(a)),
        Ty::Arrow(a, b, m) => Ty::arrow_m(tf_rename_pat_vars(a), tf_rename_pat_vars(b), *m),
        Ty::List(e) => Ty::list(tf_rename_pat_vars(e)),
        Ty::IO(e) => Ty::io(tf_rename_pat_vars(e)),
        Ty::LuaIO(s, e) => Ty::lua_io(s.clone(), tf_rename_pat_vars(e)),
        Ty::Tuple(es) => Ty::Tuple(es.iter().map(tf_rename_pat_vars).collect()),
        Ty::Forall(v, inner) => Ty::Forall(v.clone(), Box::new(tf_rename_pat_vars(inner))),
        _ => ty.clone(),
    }
}

/// Try to reduce ONE outermost step of a family application. Returns the
/// reduced type, or `None` when `ty` is not a (saturated) family application
/// or is stuck (no equation matched, or the matching equation cannot FIRE).
///
/// GHC closed-family semantics: equations are tried top-to-bottom, and the
/// first whose pattern MATCHES fires only if the argument is *apart* from
/// every earlier equation's pattern — i.e. no substitution of the argument's
/// variables could make an earlier equation apply. A symbolic argument that
/// merely fails to match an earlier, more specific clause (`IsZero n` against
/// `IsZero 'Z`) is NOT apart from it, so the application stays STUCK instead
/// of wrongly committing to the catch-all. For ground arguments matching and
/// unifiability coincide, so ground reductions are unchanged.
fn tf_reduce_head(ty: &Ty, fams: &TyFamilies) -> Option<Ty> {
    let (head, args) = peel_app(ty);
    let Ty::Con(name) = head else { return None };
    let equations = fams.get(name)?;
    for (i, (pats, result)) in equations.iter().enumerate() {
        if pats.len() != args.len() {
            continue;
        }
        let mut binds: HashMap<String, Ty> = HashMap::new();
        if pats.iter().zip(&args).all(|(p, a)| tf_match(p, a, &mut binds)) {
            // Apartness against every EARLIER equation: if some earlier
            // pattern could still come to match the argument under a
            // substitution, this clause must not fire — stuck.
            for (ppats, _) in &equations[..i] {
                if ppats.len() != args.len() {
                    continue;
                }
                let mut ubinds: HashMap<String, Ty> = HashMap::new();
                if ppats.iter().zip(&args).all(|(p, a)| {
                    tf_maybe_unifiable(&tf_rename_pat_vars(p), a, &mut ubinds, fams)
                }) {
                    return None;
                }
            }
            return Some(tf_subst(result, &binds));
        }
    }
    None
}

/// Normalize the SUB-TERMS of `ty` (recursively) without touching `ty`'s own
/// head — so an inner family application is reduced before an outer one tries
/// to match on it (`Plus (Plus 'Z 'Z) m` needs the inner reduced first). The
/// recursion depth here is bounded by the type's structural nesting, never by
/// the number of reduction steps.
fn tf_normalize_children(ty: &Ty, fams: &TyFamilies, fuel: &mut u32) -> Result<Ty, DiagnosticKind> {
    Ok(match ty {
        Ty::App(f, a) =>
            Ty::app(tf_normalize(f, fams, fuel)?, tf_normalize(a, fams, fuel)?),
        Ty::Arrow(a, b, m) =>
            Ty::arrow_m(tf_normalize(a, fams, fuel)?, tf_normalize(b, fams, fuel)?, *m),
        Ty::List(e) => Ty::list(tf_normalize(e, fams, fuel)?),
        Ty::IO(e) => Ty::io(tf_normalize(e, fams, fuel)?),
        Ty::LuaIO(s, e) => Ty::lua_io(s.clone(), tf_normalize(e, fams, fuel)?),
        Ty::Tuple(es) => Ty::Tuple(
            es.iter().map(|e| tf_normalize(e, fams, fuel)).collect::<Result<_, _>>()?,
        ),
        Ty::Forall(v, inner) =>
            Ty::Forall(v.clone(), Box::new(tf_normalize(inner, fams, fuel)?)),
        other => other.clone(),
    })
}

/// Reduce every closed-type-family application in `ty` to normal form: first
/// its sub-terms, then its head repeatedly, until nothing reduces (a fixpoint)
/// or `fuel` is exhausted (divergence). A family application that gets stuck on
/// a type variable is left in place — a normal, deferred outcome, not an error.
///
/// The head-reduction fixpoint is an ITERATIVE loop, not recursion: a
/// non-terminating family (`Loop x = Loop x`) must burn fuel and report a
/// divergence, never grow the call stack until it overflows.
fn tf_normalize(ty: &Ty, fams: &TyFamilies, fuel: &mut u32) -> Result<Ty, DiagnosticKind> {
    let mut cur = tf_normalize_children(ty, fams, fuel)?;
    loop {
        match tf_reduce_head(&cur, fams) {
            None => return Ok(cur),
            Some(reduced) => {
                // Charge fuel by the SIZE of the reduced type, not a flat 1 per
                // step. A GROWING family (`Grow x = Grow (Maybe x)`) enlarges
                // its argument every step, so a flat per-step budget lets it
                // build an enormous type before the step limit — O(fuel^2) work,
                // effectively a hang. Charging by size bounds total work and
                // reports the divergence quickly, the same as a same-size loop.
                let cost = ty_size_up_to(&reduced, *fuel);
                if cost >= *fuel {
                    let (head, _) = peel_app(&cur);
                    let name = match head { Ty::Con(n) => n.clone(), _ => "?".to_string() };
                    return Err(DiagnosticKind::TypeFamilyDivergence(name));
                }
                *fuel -= cost;
                // The reduced result may expose new inner family apps.
                cur = tf_normalize_children(&reduced, fams, fuel)?;
            }
        }
    }
}

/// Count the nodes in `ty`, stopping as soon as the count reaches `cap`
/// (returns `cap` then). Used to charge type-family reduction fuel by the size
/// of each reduced type without ever walking a runaway (growing) type in full.
fn ty_size_up_to(ty: &Ty, cap: u32) -> u32 {
    fn go(ty: &Ty, cap: u32, acc: &mut u32) {
        if *acc >= cap { return; }
        *acc += 1;
        match ty {
            Ty::Arrow(a, b, _) | Ty::App(a, b) => { go(a, cap, acc); go(b, cap, acc); }
            Ty::List(a) | Ty::IO(a) | Ty::LuaIO(_, a) | Ty::Forall(_, a) => go(a, cap, acc),
            Ty::Tuple(es) => for e in es { go(e, cap, acc); if *acc >= cap { break; } },
            _ => {}
        }
    }
    let mut acc = 0;
    go(ty, cap, &mut acc);
    acc
}

/// Reduce every closed-type-family application in `ty` to normal form using
/// `fams`. Public entry point for the typechecker's eager (concrete)
/// reduction, which shares this ITERATIVE, fuel-bounded normalizer with the
/// unifier so a divergent family reports an error instead of overflowing the
/// stack. Returns the normal form, or a `TypeFamilyDivergence` error.
pub fn reduce_type_families(ty: &Ty, fams: &TyFamilies) -> Result<Ty, DiagnosticKind> {
    if fams.is_empty() {
        return Ok(ty.clone());
    }
    let mut fuel = TF_FUEL;
    tf_normalize(ty, fams, &mut fuel)
}

/// Unification with no type families in scope — the fast path used by
/// `Subst::merge` and preserved for callers that never see family types.
pub fn unify(t1: &Ty, t2: &Ty) -> Result<Subst, DiagnosticKind> {
    unify_tf(t1, t2, &TyFamilies::new())
}

/// Unification that reduces closed-type-family applications to normal form on
/// both sides before matching, so length arithmetic like `Plus 'Z m ~ m` and
/// `Plus ('S n) m ~ 'S (Plus n m)` succeeds. When `fams` is empty this is the
/// plain syntactic unifier (no normalization overhead).
pub fn unify_tf(t1: &Ty, t2: &Ty, fams: &TyFamilies) -> Result<Subst, DiagnosticKind> {
    if fams.is_empty() {
        return unify_inner(t1, t2, fams);
    }
    let mut fuel = TF_FUEL;
    let n1 = tf_normalize(t1, fams, &mut fuel)?;
    let n2 = tf_normalize(t2, fams, &mut fuel)?;
    unify_inner(&n1, &n2, fams)
}

/// Unify two arrow multiplicities. Equal constants unify; a flexible
/// multiplicity variable binds to the other side (or trivially to itself);
/// `One` vs `Many` is a genuine mismatch (`None` — the caller builds the
/// diagnostic, which needs the full arrow types for a readable message).
/// A RIGID variable (a signature's `%m`) unifies only with itself: inside
/// the definition that declared it, `m` stands for whichever multiplicity a
/// caller will pick, so the body may not pin it to `One`, `Many`, or a
/// different rigid variable — the same reason a rigid type variable rejects
/// concrete types. (A flexible variable binding TO a rigid one is fine; that
/// is how an inference-invented arrow adopts the signature's `%m`.)
fn unify_mult(m1: Mult, m2: Mult) -> Option<Subst> {
    match (m1, m2) {
        (Mult::One, Mult::One) | (Mult::Many, Mult::Many) => Some(Subst::empty()),
        (Mult::Var(v), Mult::Var(w)) if v == w => Some(Subst::empty()),
        (Mult::Rigid(v), Mult::Rigid(w)) if v == w => Some(Subst::empty()),
        (Mult::Var(v), m) | (m, Mult::Var(v)) => Some(Subst::mult_singleton(v, m)),
        _ => None,
    }
}

/// The structural core of unification. `t1`/`t2` are already in
/// family-normal-form when `fams` is non-empty; recursive calls go back
/// through `unify_tf` so a substitution that exposes a new reduction is
/// re-normalized.
fn unify_inner(t1: &Ty, t2: &Ty, fams: &TyFamilies) -> Result<Subst, DiagnosticKind> {
    match (t1, t2) {
        (Ty::Con(a), Ty::Con(b)) if a == b => Ok(Subst::empty()),
        (Ty::Promoted(a), Ty::Promoted(b)) if a == b => Ok(Subst::empty()),
        (Ty::Unit, Ty::Unit) => Ok(Subst::empty()),

        // Two FRESH flexible variables (user-written variables carry
        // `id: u32::MAX` and keep the old left-binds-first behavior below):
        // either binding direction is a most general unifier, so bind the
        // YOUNGER (higher id) to the older. Direction matters for compile
        // time, not meaning: inference threads one variable through a long
        // chain of statements (e.g. the shared `Num` variable of a do-block
        // full of numeric `let`s), and binding old := new moves that chain's
        // representative every statement — re-pointing every accumulated
        // substitution image that mentions it, which is quadratic in program
        // length. Binding new := old keeps the representative stable, so
        // each statement touches only its own bindings.
        (Ty::Var(v), Ty::Var(w))
            if v != w && v.id != u32::MAX && w.id != u32::MAX =>
        {
            if v.id > w.id {
                Ok(Subst::singleton(v.clone(), Ty::Var(w.clone())))
            } else {
                Ok(Subst::singleton(w.clone(), Ty::Var(v.clone())))
            }
        }

        (Ty::Var(v), t) | (t, Ty::Var(v)) => {
            if t == &Ty::Var(v.clone()) {
                Ok(Subst::empty())
            } else if t.occurs(v) {
                Err(DiagnosticKind::OccursCheck(v.clone(), t.clone()))
            } else {
                Ok(Subst::singleton(v.clone(), t.clone()))
            }
        }

        // Stuck (irreducible) type-family applications. After normalization
        // these did not reduce — a scrutinee is a type variable no equation
        // matches yet. A family is NOT assumed injective, so we must not unify
        // two family applications structurally (that would wrongly conclude
        // `a ~ b` from `F a ~ F b`). Two stuck applications unify only when
        // they are syntactically identical (e.g. `Plus n m ~ Plus n m`, which
        // is exactly what a length-tracking recursion produces); anything else
        // is left as an unprovable equality and rejected. A stuck app versus a
        // type variable was already handled by the `Var` arm above (which
        // binds it, with the ordinary occurs check).
        (a, b) if is_stuck_family_app(a, fams) || is_stuck_family_app(b, fams) => {
            if a == b {
                Ok(Subst::empty())
            } else {
                Err(DiagnosticKind::Mismatch(a.clone(), b.clone()))
            }
        }

        (Ty::Arrow(a1, b1, m1), Ty::Arrow(a2, b2, m2)) => {
            // Multiplicities unify invariantly, exactly like GHC's linear
            // arrows: `%1` and a plain `->` are different arrows, and a
            // variable adopts whichever side is concrete. Rejecting
            // Many-into-One is what keeps the linear promise sound — an
            // unrestricted function flowing into a `%1`-typed position could
            // duplicate (or drop) the argument the `%1` type claims is
            // consumed exactly once.
            let s0 = unify_mult(*m1, *m2)
                .ok_or_else(|| DiagnosticKind::MultiplicityMismatch(t1.clone(), t2.clone()))?;
            let s1 = unify_tf(&a1.apply_subst(&s0), &a2.apply_subst(&s0), fams)?;
            let s01 = s0.compose(&s1);
            let s2 = unify_tf(&b1.apply_subst(&s01), &b2.apply_subst(&s01), fams)?;
            Ok(s01.compose(&s2))
        }

        (Ty::App(a1, b1), Ty::App(a2, b2)) => {
            let s1 = unify_tf(a1, a2, fams)?;
            let s2 = unify_tf(&b1.apply_subst(&s1), &b2.apply_subst(&s1), fams)?;
            Ok(s1.compose(&s2))
        }

        (Ty::List(a), Ty::List(b)) => unify_tf(a, b, fams),
        (Ty::IO(a), Ty::IO(b)) => unify_tf(a, b, fams),
        (Ty::LuaIO(s1, a), Ty::LuaIO(s2, b)) => {
            let s = unify_tf(&Ty::Var(s1.clone()), &Ty::Var(s2.clone()), fams)?;
            let s2 = unify_tf(&a.apply_subst(&s), &b.apply_subst(&s), fams)?;
            Ok(s.compose(&s2))
        }

        (Ty::Tuple(a), Ty::Tuple(b)) if a.len() == b.len() => {
            let mut s = Subst::empty();
            for (ea, eb) in a.iter().zip(b.iter()) {
                let si = unify_tf(&ea.apply_subst(&s), &eb.apply_subst(&s), fams)?;
                s = s.compose(&si);
            }
            Ok(s)
        }

        // Allow App(f, a) to unify with List(b) by treating [] as App(Con("[]"), ...)
        (Ty::App(f, a), Ty::List(b)) | (Ty::List(b), Ty::App(f, a)) => {
            let s1 = unify_tf(f, &Ty::Con("[]".into()), fams)?;
            let s2 = unify_tf(&a.apply_subst(&s1), &b.apply_subst(&s1), fams)?;
            Ok(s1.compose(&s2))
        }

        // Allow App(m, a) to unify with IO(b) by treating IO as App(Con("IO"), ...)
        (Ty::App(f, a), Ty::IO(b)) | (Ty::IO(b), Ty::App(f, a)) => {
            let s1 = unify_tf(f, &Ty::Con("IO".into()), fams)?;
            let s2 = unify_tf(&a.apply_subst(&s1), &b.apply_subst(&s1), fams)?;
            Ok(s1.compose(&s2))
        }

        // Allow App(m, a) to unify with LuaIO(s, b) by treating LuaIO as App(App(Con("LuaIO"), s), ...)
        (Ty::App(f, a), Ty::LuaIO(s, b)) | (Ty::LuaIO(s, b), Ty::App(f, a)) => {
            let lua_io_s = Ty::App(Box::new(Ty::Con("LuaIO".into())), Box::new(Ty::Var(s.clone())));
            let s1 = unify_tf(f, &lua_io_s, fams)?;
            let s2 = unify_tf(&a.apply_subst(&s1), &b.apply_subst(&s1), fams)?;
            Ok(s1.compose(&s2))
        }

        // Forall: instantiate the quantified variable and unify the body
        (Ty::Forall(_v, inner), t) | (t, Ty::Forall(_v, inner)) => {
            // The forall-bound variable is already a rigid skolem (id=MAX).
            // Unify the body directly — the variable will unify with whatever
            // the concrete type provides, enforcing that it can't escape.
            unify_tf(inner, t, fams)
        }

        // Skolem: rigid type constant, only unifies with itself
        (Ty::Skolem(a, i), Ty::Skolem(b, j)) if a == b && i == j => Ok(Subst::empty()),
        (Ty::Skolem(..), _t) | (_t, Ty::Skolem(..)) => {
            Err(DiagnosticKind::RigidMismatch(t1.clone(), t2.clone()))
        }

        _ => Err(DiagnosticKind::Mismatch(t1.clone(), t2.clone())),
    }
}

/// One structured compiler diagnostic, shared by the parser, the typechecker
/// and the monomorphizer: what went wrong (`kind`), where in the source
/// (`span`), inside which definition (`context`), plus optional mata-ll
/// specific `note:` lines explaining a deviation from GHC.
#[derive(Debug)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub context: Option<String>,
    pub span: Option<crate::ast::Span>,
    /// The file the span refers to, when known. Rendered as `at file:line:col`
    /// instead of the bare `at line:col`. Only set where the compiler actually
    /// knows the file (the CLI passes the source path via
    /// `CompileOptions::source_name`); `None` keeps the historical rendering.
    pub file: Option<String>,
    /// Extra explanatory lines rendered as `note: …` after the location.
    /// (Type-error kinds additionally derive a builtin note via `hint()`.)
    pub notes: Vec<String>,
    /// True when the error was produced while checking the implicit Prelude's
    /// own declarations rather than the user's code (or their imports). Such
    /// an error can only be caused by the user's program interfering with the
    /// Prelude — the Prelude alone always compiles — so `lib.rs` replaces
    /// these with a diagnosis of the interference (e.g. a redefined Prelude
    /// name) instead of showing errors at source lines the user never wrote.
    pub baseline: bool,
}

impl Diagnostic {
    pub fn new(kind: DiagnosticKind) -> Self {
        Diagnostic { kind, context: None, span: None, file: None, notes: Vec::new(), baseline: false }
    }

    pub fn in_context(kind: DiagnosticKind, ctx: impl Into<String>) -> Self {
        Diagnostic { kind, context: Some(ctx.into()), span: None, file: None, notes: Vec::new(), baseline: false }
    }

    /// A parse error at a known source location. Rendered inline as
    /// `"{msg} at {line}:{col}"` — the parser's historical format.
    pub fn parse_at(msg: impl Into<String>, span: crate::ast::Span) -> Self {
        Diagnostic {
            kind: DiagnosticKind::Parse(msg.into()),
            context: None,
            span: Some(span),
            file: None,
            notes: Vec::new(),
            baseline: false,
        }
    }

}

#[derive(Debug)]
pub enum DiagnosticKind {
    Mismatch(Ty, Ty),
    RigidMismatch(Ty, Ty),
    /// Two function types whose shapes line up but whose arrows disagree
    /// about multiplicity: one side is `a %1 -> b` (argument consumed
    /// exactly once) and the other a plain `a -> b` (no restriction). They
    /// are not interchangeable — letting an unrestricted function stand
    /// where a `%1` one is required would let it use an argument twice (or
    /// drop it) that the `%1` type promises is consumed exactly once.
    MultiplicityMismatch(Ty, Ty),
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
    /// An existentially quantified type variable, skolemized when its data
    /// constructor was unpacked in a pattern, occurs in a type that outlives
    /// the pattern match (the match's result type, or the enclosing
    /// function's type). The concrete type was erased when the value was
    /// packed, so nothing outside the match may see or name it.
    ExistentialEscape { var: String, con: String, ty: Ty },
    /// A type reference names a type that was never defined: no builtin,
    /// data/newtype declaration, type alias, or type family has this name.
    /// Rejected at the reference so it cannot flow downstream as an opaque
    /// type and resurface as a misleading error (e.g. a missing Show
    /// instance on a type that does not exist).
    UnknownType(String),
    /// A type expression sits in a position that requires a different kind
    /// than the one it has — a bare `Maybe` (kind `Type -> Type`) where a
    /// complete type is required, or a type variable used both applied
    /// (`t a`) and bare (`t`) in the same signature. The strings are the
    /// type as the user wrote it; the kinds are fully defaulted.
    KindMismatch { ty: String, expected: Kind, found: Kind },
    /// A complete type (kind `Type`) is applied to a type argument, e.g.
    /// `Maybe Int Bool` (the inner `Maybe Int` is already complete,
    /// so the application to `Bool` is meaningless) or `Int a`.
    /// `is_var` selects the wording: applying a type VARIABLE that another
    /// use in the same declaration already fixed at kind Type is a
    /// two-kinds-for-one-variable conflict, not a saturated constructor.
    KindSaturatedApp { ty: String, arg: String, is_var: bool, kind: Kind },
    /// A type application whose argument has the wrong kind for the
    /// constructor's parameter, e.g. `HashMap Maybe Int`: HashMap's
    /// first parameter must be a complete type, but `Maybe` still needs an
    /// argument.
    KindArgMismatch { func: String, arg: String, expected: Kind, found: Kind },
    /// An instance head whose kind does not match the class variable's kind:
    /// `instance Foldable Int` (Foldable's methods apply the class
    /// variable to an element type, so an instance must supply a
    /// `Type -> Type` constructor) or `instance Show Maybe` (Show constrains
    /// complete types).
    InstanceKindMismatch { class: String, class_var: String, target: String, expected: Kind, found: Kind },
    /// A closed type family did not terminate while reducing (e.g.
    /// `type family Loop x where Loop x = Loop x`): reduction hit its step
    /// bound. The string is the family's name. Reported instead of looping
    /// (or overflowing the stack) forever.
    TypeFamilyDivergence(String),
    /// A syntax error. The message is rendered verbatim, with the span (when
    /// present) appended inline as ` at line:col` — the parser's historical
    /// format, unlike type errors which put the location on its own line.
    Parse(String),
    Other(String),
}

/// Quote a rendered type for a message, but not a promoted constructor: its
/// leading tick already sets it off, so `'True` prints as `'True`, not the
/// doubled `''True'`.
fn quote_ty(s: &str) -> String {
    if s.starts_with('\'') { s.to_string() } else { format!("'{}'", s) }
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
/// common confusion, since String is not `[Char]` here. A still-polymorphic
/// container shape `t a` counts as list-like too: it is what a
/// Foldable-generic function (length, null, sum, elem, …) expects, and
/// passing a String there is the same confusion.
fn is_string_list_mismatch(a: &Ty, b: &Ty) -> bool {
    let is_str = |t: &Ty| matches!(t, Ty::Con(n) if n == "String");
    let is_listish = |t: &Ty| matches!(t, Ty::List(_))
        || matches!(t, Ty::App(f, _) if matches!(f.as_ref(), Ty::Var(_)));
    (is_str(a) && is_listish(b)) || (is_listish(a) && is_str(b))
}

impl Diagnostic {
    /// An optional mata-ll-specific explanation appended below the error.
    fn hint(&self) -> Option<&'static str> {
        match &self.kind {
            DiagnosticKind::Mismatch(a, b) | DiagnosticKind::RigidMismatch(a, b)
                if is_string_list_mismatch(a, b) =>
                Some("in mata-ll String is opaque — it is the Lua string (a byte array), \
                      NOT [Char] — so it does not unify with [a], and list operations \
                      (++, map, length, intercalate, …) do not accept it. To join Strings \
                      use the Semigroup operator (<>): `\"a\" <> \"b\"`; to join a list of \
                      Strings, fold it with <> or use mconcat (there is no ++/intercalate \
                      for String). See HASKDIFF.md, \"Strings and ByteStrings\"."),
            DiagnosticKind::NoInstance { class, ty }
                if class == "Ord" && matches!(ty, Ty::Tuple(_) | Ty::List(_) | Ty::App(_, _)) =>
                Some("mata-ll has no Ord instance for tuples, lists, or Maybe; compare their \
                      components individually."),
            DiagnosticKind::NoInstance { ty: Ty::Arrow(..) | Ty::IO(_) | Ty::LuaIO(_, _), .. } =>
                Some("functions and IO actions have no Show/Eq/Ord instance — there is no \
                      way to render or compare them."),
            DiagnosticKind::MultiplicityMismatch(..) =>
                Some("to pass a '%1' function where an ordinary one is expected, \
                      wrap it in a lambda: '\\x -> f x' makes no single-use promise. \
                      The other direction (an ordinary function where '%1' is \
                      required) is rejected outright, because the function might \
                      use its argument more than once. GHC's LinearTypes behave \
                      the same way."),
            DiagnosticKind::AmbiguousType { .. } =>
                Some("add a type annotation to pin the type down, e.g. \
                      `show (Nothing :: Maybe Int)`. GHC rejects this the same way."),
            DiagnosticKind::MissingContextConstraint { .. } =>
                Some("a bare polymorphic variable has no instance unless the signature \
                      requires one. GHC reports this as \"add (C a) to the context\"."),
            DiagnosticKind::ExistentialEscape { .. } =>
                Some("the concrete type was erased when the value was packed into the \
                      constructor, so no code outside the match can know what it is. Use \
                      the value inside the match (e.g. apply the functions packed \
                      alongside it), or repack it into the existential before returning."),
            DiagnosticKind::InstanceKindMismatch { expected, found, .. } => match (expected, found) {
                (Kind::Arrow(..), Kind::Type) =>
                    Some("a class over containers takes the bare, unapplied constructor \
                          as its instance head: write 'instance C []', not \
                          'instance C [a]' (and 'instance C Maybe', not \
                          'instance C (Maybe a)'). The class variable stands for the \
                          container itself; the element type stays polymorphic."),
                (Kind::Type, Kind::Arrow(..)) =>
                    Some("this class constrains complete types, so the instance head \
                          must apply the constructor to type arguments, e.g. \
                          'instance C (T a)'. GHC reports this as \"Expecting one \
                          more argument\"."),
                _ => None,
            },
            DiagnosticKind::KindMismatch { expected: Kind::Type, found: Kind::Arrow(..), .. } =>
                Some("mata-ll infers every kind from how types are used — there are \
                      no kind annotations — so the fix is to apply the constructor \
                      to its missing argument(s). GHC reports this as \"Expecting \
                      one more argument\"."),
            DiagnosticKind::UnknownType(name) => match name.as_str() {
                "Boolean" =>
                    Some("the boolean type is spelled 'Bool', as in Haskell."),
                "Char" =>
                    Some("mata-ll has no Char type — String is opaque, not [Char]. \
                          Individual characters are Int byte codes; see strByte and \
                          strChar in LString."),
                "Double" | "Float" =>
                    Some("the floating-point type is spelled 'Number' (Lua's number type); \
                          GHC's Double and Float do not exist in mata-ll."),
                "Int" =>
                    Some("mata-ll has no arbitrary-precision Integer. The fixed-width \
                          integer type is spelled 'Int' — 64-bit and wrapping, exactly \
                          like GHC's Int. Use Int; there is no bignum type."),
                _ => None,
            },
            DiagnosticKind::UnboundVariable(name) if name == "toInteger" =>
                Some("mata-ll has no arbitrary-precision Integer, so 'toInteger' (which \
                      would convert to it) does not exist. The integer type is already \
                      'Int'; there is nothing to convert to."),
            _ => None,
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            DiagnosticKind::Mismatch(a, b) => {
                let s = pretty_var_subst(&[a, b]);
                write!(f, "Cannot unify '{}' with '{}'", a.apply_subst(&s), b.apply_subst(&s))?
            }
            DiagnosticKind::RigidMismatch(a, b) => {
                let s = pretty_var_subst(&[a, b]);
                write!(f, "Cannot match '{}' with '{}': a rigid type variable stands for one specific type that is not visible here, so it cannot be assumed to be any particular type",
                    a.apply_subst(&s), b.apply_subst(&s))?
            }
            DiagnosticKind::MultiplicityMismatch(a, b) => {
                let s = pretty_var_subst(&[a, b]);
                let mut rigids = Vec::new();
                a.collect_rigid_mults(&mut rigids);
                b.collect_rigid_mults(&mut rigids);
                write!(f, "Cannot match '{}' with '{}': the arrows disagree about how often the function may use its argument — '%1 ->' promises the argument is consumed exactly once, while a plain '->' makes no such promise, so the two function types are not interchangeable",
                    a.apply_subst(&s), b.apply_subst(&s))?;
                if !rigids.is_empty() {
                    write!(f, "\nnote: '%m ->' is a multiplicity VARIABLE from a signature — the caller chooses whether it is '%1' or unrestricted, so inside this definition it cannot be assumed to be either one")?
                }
            }
            DiagnosticKind::OccursCheck(v, ty) => {
                let vt = Ty::Var(v.clone());
                let s = pretty_var_subst(&[&vt, ty]);
                write!(f, "Infinite type: {} occurs in {}", vt.apply_subst(&s), ty.apply_subst(&s))?
            }
            DiagnosticKind::UnboundVariable(name) =>
                write!(f, "Unbound variable: {}", name)?,
            DiagnosticKind::UnboundConstructor(name) =>
                write!(f, "Unknown constructor: {}", name)?,
            DiagnosticKind::PatternArgCount { constructor, expected, got } =>
                write!(f, "Constructor {} expects {} args, got {}",
                    constructor, expected, got)?,
            DiagnosticKind::NonExhaustive(name) =>
                write!(f, "Non-exhaustive patterns in {}", name)?,
            DiagnosticKind::TypeSigMismatch { name, declared, inferred } => {
                let s = pretty_var_subst(&[declared, inferred]);
                write!(f, "Type signature for '{}' doesn't match: declared {}, inferred {}",
                    name, declared.apply_subst(&s), inferred.apply_subst(&s))?
            }
            DiagnosticKind::NoInstance { class, ty } => {
                let s = pretty_var_subst(&[ty]);
                let rendered = ty.apply_subst(&s);
                let shown = match &rendered {
                    Ty::Arrow(..) | Ty::App(_, _) | Ty::IO(_) | Ty::LuaIO(_, _) =>
                        format!("({})", rendered),
                    _ => format!("{}", rendered),
                };
                write!(f, "No instance for '{} {}'", class, shown)?
            }
            DiagnosticKind::AmbiguousType { class, ty } => {
                let s = pretty_var_subst(&[ty]);
                let rendered = ty.apply_subst(&s);
                let shown = match &rendered {
                    Ty::Arrow(..) | Ty::App(_, _) | Ty::IO(_) | Ty::LuaIO(_, _) =>
                        format!("({})", rendered),
                    _ => format!("{}", rendered),
                };
                write!(f, "Ambiguous type: nothing here determines the type '{}', so no '{}' instance can be chosen for it",
                    shown, class)?
            }
            DiagnosticKind::MissingContextConstraint { class, ty } => {
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
            DiagnosticKind::ExistentialEscape { var, con, ty } => {
                let s = pretty_var_subst(&[ty]);
                write!(f, "Existential type variable '{}' escapes its scope: '{}' is hidden by constructor '{}' and exists only inside the pattern match that unpacks it, but here it leaks into the type '{}', which outlives the match",
                    var, var, con, ty.apply_subst(&s))?
            }
            DiagnosticKind::UnknownType(name) =>
                write!(f, "Unknown type '{}': nothing in this program or its imports defines a type with this name — it is not a builtin, and no data, newtype, type alias, or type family declaration for it is in scope", name)?,
            DiagnosticKind::TypeFamilyDivergence(name) =>
                write!(f, "Type family '{}' did not terminate: reducing an application of it exceeded the reduction step limit, so it appears to be non-terminating (e.g. an equation whose result reduces to itself)", name)?,
            DiagnosticKind::KindMismatch { ty, expected, found } => {
                // Tailor the two common shapes: an unsaturated constructor
                // where a complete type belongs, and the reverse.
                match (expected, found) {
                    (Kind::Type, Kind::Arrow(..)) => {
                        let n = found.arity();
                        write!(f, "Kind error: '{}' has kind {} — it is a type constructor that still needs {} more type argument{} before it is a complete type — but it is used here where a complete type (kind Type) is required",
                            ty, found, n, if n == 1 { "" } else { "s" })?
                    }
                    (Kind::Arrow(..), Kind::Type) =>
                        write!(f, "Kind error: '{}' has kind Type (a complete type), but it is used here as a type constructor of kind {} — something that must still be applied to type arguments",
                            ty, expected)?,
                    _ =>
                        write!(f, "Kind error: '{}' has kind {}, but its position here requires kind {}",
                            ty, found, expected)?,
                }
            }
            DiagnosticKind::KindSaturatedApp { ty, arg, is_var, kind } => {
                if *is_var {
                    write!(f, "Kind error: the type variable '{}' is applied to the type argument '{}' here, but its use elsewhere in this declaration makes it a complete type (kind Type) — a single type variable cannot be used at two different kinds",
                        ty, arg)?
                } else {
                    write!(f, "Kind error: '{}' is applied to the type argument '{}', but '{}' has kind {} — it is already a complete type and takes no type arguments",
                        ty, arg, ty, kind)?
                }
            }
            DiagnosticKind::KindArgMismatch { func, arg, expected, found } =>
                // `arg` may be a promoted constructor (`'True`), whose leading
                // tick already delimits it — avoid the doubled `''True'`.
                write!(f, "Kind error: in this type application, '{}' needs an argument of kind {}, but {} has kind {}",
                    func, expected, quote_ty(arg), found)?,
            DiagnosticKind::InstanceKindMismatch { class, class_var, target, expected, found } =>
                write!(f, "Kind error: 'instance {} {}' is ill-kinded: the methods of class '{}' use its type variable '{}' at kind {}, but '{}' has kind {}",
                    class, target, class, class_var, expected, target, found)?,
            DiagnosticKind::Parse(msg) => {
                // Parse errors keep their historical inline rendering:
                // `Expected X, found Y at 3:7`.
                write!(f, "{}", msg)?;
                if let Some(span) = &self.span {
                    write!(f, " at {}:{}", span.line, span.col)?;
                }
                for note in &self.notes {
                    write!(f, "\n  note: {}", note)?;
                }
                return Ok(());
            }
            DiagnosticKind::Other(msg) => write!(f, "{}", msg)?,
        }
        // `at [file:]line:col` — the file prefix appears only when known.
        let loc = |span: &crate::ast::Span| match &self.file {
            Some(file) => format!("{}:{}:{}", file, span.line, span.col),
            None => format!("{}:{}", span.line, span.col),
        };
        if let Some(ctx) = &self.context {
            if let Some(span) = &self.span {
                write!(f, "\n  at {}, in {}", loc(span), ctx)?;
            } else {
                write!(f, "\n  in {}", ctx)?;
            }
        } else if let Some(span) = &self.span {
            write!(f, "\n  at {}", loc(span))?;
        }
        if let Some(hint) = self.hint() {
            write!(f, "\n  note: {}", hint)?;
        }
        for note in &self.notes {
            write!(f, "\n  note: {}", note)?;
        }
        Ok(())
    }
}
