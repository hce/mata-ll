use std::collections::{HashMap, HashSet};
use crate::ast::*;
use crate::tir::*;
use crate::types::*;

mod solve;
mod derive;
mod infer;
mod kind;
mod usage;

/// Fuel for type-alias expansion while resolving one top-level type (reset at
/// each depth-0 `ast_type_to_ty`). Charged by the size of every expanded alias
/// body, so the running total tracks the SIZE of the type being built —
/// mirroring the type-family reducer, which charges `ty_size_up_to` per
/// reduced type (`types.rs`). A self-doubling alias tower
/// (`type Pi a = P{i-1} (P{i-1} a)`) expands to a type whose size is
/// exponential in the number of levels, so it drains this budget and is
/// reported as non-terminating; ordinary alias use (a handful of levels, tens
/// of nodes) charges only a few hundred units and never comes close.
///
/// The value is calibrated empirically. Each charge unit here costs more real
/// work than a type-family reduction step (a full `substitute_type` clone of
/// the body plus the size walk), so the budget is set BELOW `TF_FUEL` (100k):
/// at 30k a doubling tower (P4 and up) trips in well under half a second on a
/// debug build, while a P3 tower (256 expanded nodes) and every realistic
/// signature still resolve. Even a deliberately large — but terminating —
/// hand-written alias would need tens of thousands of expanded nodes to trip,
/// which no hand-written type approaches; that is the safety margin.
const ALIAS_EXPAND_FUEL: u32 = 30_000;

/// One environment binding: the scheme plus its cached free-variable
/// footprint, computed once when the scheme enters the environment. The
/// caches exist so the environment-wide questions the checker asks per
/// statement — "is this variable free somewhere in the environment?"
/// (generalization) and "can this substitution change anything here?"
/// (`apply_subst`) — never have to re-walk every scheme's type. They cannot
/// go stale: the environment owns its entries (lookups hand out `&Scheme`),
/// so a scheme only ever changes by being re-inserted or rewritten by
/// `apply_subst`, both of which recompute the caches.
#[derive(Debug, Clone)]
struct EnvEntry {
    scheme: Scheme,
    /// Type variables free in the scheme (`ty`'s free vars minus the
    /// quantified `vars`).
    free_tvs: Vec<TyVar>,
    /// Rigid multiplicity ids free in the scheme (on `ty` but not
    /// quantified in `mult_vars`) — what `generalize` must not capture.
    free_rigids: Vec<u32>,
    /// EVERY unquantified multiplicity id on `ty`, flexible and rigid: the
    /// full set through which a substitution's multiplicity bindings could
    /// rewrite the scheme (`apply_subst` resolves flexible ids too).
    mult_ids: Vec<u32>,
}

impl EnvEntry {
    fn new(scheme: Scheme) -> EnvEntry {
        let free_tvs = scheme.free_vars();
        let mut all_rigids = Vec::new();
        scheme.ty.collect_rigid_mults(&mut all_rigids);
        let free_rigids: Vec<u32> = all_rigids.into_iter()
            .filter(|id| !scheme.mult_vars.contains(id))
            .collect();
        let mut all_mults = Vec::new();
        scheme.ty.collect_mult_ids(&mut all_mults);
        let mult_ids: Vec<u32> = all_mults.into_iter()
            .filter(|id| !scheme.mult_vars.contains(id))
            .collect();
        EnvEntry { scheme, free_tvs, free_rigids, mult_ids }
    }

    /// Can `subst` change this scheme? False guarantees `apply_subst` is the
    /// identity on it: quantified variables are restricted away by
    /// `Scheme::apply_subst`, so only the cached free footprint matters.
    fn affected_by(&self, subst: &Subst) -> bool {
        self.free_tvs.iter().any(|v| subst.binds_var(v))
            || self.mult_ids.iter().any(|id| subst.binds_mult(*id))
    }
}

/// Type environment: maps names to type schemes.
///
/// Alongside the bindings it maintains three MULTISETS (var → number of
/// entries whose cache mentions it) aggregating the per-entry caches. They
/// make `generalize`'s environment questions O(1) per variable and let a
/// substitution that touches nothing in the environment be recognized
/// without visiting any binding — previously each do-`let` re-walked every
/// scheme in scope (the whole Prelude plus all previous bindings), making
/// long do-blocks quadratic to typecheck. Counts (not sets) because two
/// entries can share a free variable and removing one must not forget the
/// other's claim.
#[derive(Debug, Clone)]
pub struct TypeEnv {
    bindings: HashMap<String, EnvEntry>,
    fv_counts: HashMap<TyVar, u32>,
    rigid_counts: HashMap<u32, u32>,
    mult_id_counts: HashMap<u32, u32>,
    /// Reverse indexes: variable → names of bindings whose cached footprint
    /// may mention it. STALE-TOLERANT: a rewritten/removed binding keeps its
    /// old index entries; `apply_subst_mut` re-checks each candidate against
    /// the live entry and a stale one simply drops (its bucket is consumed).
    /// These let a substitution rewrite exactly the bindings it affects
    /// instead of scanning the whole environment — the last Θ(env)-per-
    /// statement walk in the long-do-block shape.
    fv_index: HashMap<TyVar, Vec<String>>,
    mult_id_index: HashMap<u32, Vec<String>>,
}

impl Default for TypeEnv {
    fn default() -> Self {
        Self::new()
    }
}

fn count_add<K: std::hash::Hash + Eq + Clone>(counts: &mut HashMap<K, u32>, key: &K) {
    *counts.entry(key.clone()).or_insert(0) += 1;
}

fn count_sub<K: std::hash::Hash + Eq>(counts: &mut HashMap<K, u32>, key: &K) {
    if let Some(n) = counts.get_mut(key) {
        if *n <= 1 {
            counts.remove(key);
        } else {
            *n -= 1;
        }
    }
}

/// Add/remove one entry's cached footprint to/from the aggregate multisets.
/// Free functions (not methods) so callers can hold a mutable borrow of the
/// bindings map at the same time.
fn count_entry(
    entry: &EnvEntry,
    fv_counts: &mut HashMap<TyVar, u32>,
    rigid_counts: &mut HashMap<u32, u32>,
    mult_id_counts: &mut HashMap<u32, u32>,
) {
    for v in &entry.free_tvs { count_add(fv_counts, v); }
    for id in &entry.free_rigids { count_add(rigid_counts, id); }
    for id in &entry.mult_ids { count_add(mult_id_counts, id); }
}

fn uncount_entry(
    entry: &EnvEntry,
    fv_counts: &mut HashMap<TyVar, u32>,
    rigid_counts: &mut HashMap<u32, u32>,
    mult_id_counts: &mut HashMap<u32, u32>,
) {
    for v in &entry.free_tvs { count_sub(fv_counts, v); }
    for id in &entry.free_rigids { count_sub(rigid_counts, id); }
    for id in &entry.mult_ids { count_sub(mult_id_counts, id); }
}

impl TypeEnv {
    pub fn new() -> Self {
        TypeEnv {
            bindings: HashMap::new(),
            fv_counts: HashMap::new(),
            rigid_counts: HashMap::new(),
            mult_id_counts: HashMap::new(),
            fv_index: HashMap::new(),
            mult_id_index: HashMap::new(),
        }
    }

    /// Register `name` in the reverse indexes under every variable of
    /// `entry`'s footprint. (Old index entries for a replaced binding are
    /// left behind — see the stale-tolerance note on the fields.)
    fn index_entry(
        entry: &EnvEntry,
        name: &str,
        fv_index: &mut HashMap<TyVar, Vec<String>>,
        mult_id_index: &mut HashMap<u32, Vec<String>>,
    ) {
        for v in &entry.free_tvs {
            fv_index.entry(v.clone()).or_default().push(name.to_string());
        }
        for id in &entry.mult_ids {
            mult_id_index.entry(*id).or_default().push(name.to_string());
        }
    }

    pub fn insert(&mut self, name: String, scheme: Scheme) {
        let entry = EnvEntry::new(scheme);
        count_entry(&entry, &mut self.fv_counts, &mut self.rigid_counts, &mut self.mult_id_counts);
        Self::index_entry(&entry, &name, &mut self.fv_index, &mut self.mult_id_index);
        if let Some(old) = self.bindings.insert(name, entry) {
            uncount_entry(&old, &mut self.fv_counts, &mut self.rigid_counts, &mut self.mult_id_counts);
        }
    }

    /// Remove a binding, returning its scheme (used to take a `let` group's
    /// monomorphic pre-registrations back out before generalization).
    pub fn remove(&mut self, name: &str) -> Option<Scheme> {
        let entry = self.bindings.remove(name)?;
        uncount_entry(&entry, &mut self.fv_counts, &mut self.rigid_counts, &mut self.mult_id_counts);
        Some(entry.scheme)
    }

    pub fn size(&self) -> usize { self.bindings.len() }

    pub fn lookup(&self, name: &str) -> Option<&Scheme> {
        self.bindings.get(name).map(|e| &e.scheme)
    }

    /// Is the variable free in (some scheme of) this environment? O(1); the
    /// membership `generalize` needs.
    pub fn is_free_var(&self, v: &TyVar) -> bool {
        self.fv_counts.contains_key(v)
    }

    /// Is the rigid multiplicity id free in this environment? The
    /// multiplicity counterpart of `is_free_var`, consulted by `generalize`
    /// so an inner binding never captures an enclosing signature's `%m`.
    pub fn has_free_rigid_mult(&self, id: u32) -> bool {
        self.rigid_counts.contains_key(&id)
    }

    /// Can `subst` change anything in this environment? Checked against the
    /// aggregate multisets, so a substitution over variables the environment
    /// never mentions (the common case: a step's fresh variables) is
    /// recognized in O(|subst|) without visiting any binding.
    pub fn affected_by(&self, subst: &Subst) -> bool {
        subst.ty_domain().any(|v| self.fv_counts.contains_key(v))
            || subst.mult_domain().any(|id| self.mult_id_counts.contains_key(&id))
    }

    /// `self` after `subst`, borrowing unchanged: when the substitution
    /// cannot touch the environment this is `self` itself — no clone, no
    /// walk. The hot inference paths (one call per application node) go
    /// through here.
    pub fn applied<'a>(&'a self, subst: &Subst) -> std::borrow::Cow<'a, TypeEnv> {
        if self.affected_by(subst) {
            std::borrow::Cow::Owned(self.apply_subst(subst))
        } else {
            std::borrow::Cow::Borrowed(self)
        }
    }

    pub fn apply_subst(&self, subst: &Subst) -> TypeEnv {
        let mut out = TypeEnv::new();
        for (k, entry) in &self.bindings {
            let new_entry = if entry.affected_by(subst) {
                EnvEntry::new(entry.scheme.apply_subst(subst))
            } else {
                entry.clone()
            };
            count_entry(&new_entry, &mut out.fv_counts, &mut out.rigid_counts, &mut out.mult_id_counts);
            Self::index_entry(&new_entry, k, &mut out.fv_index, &mut out.mult_id_index);
            out.bindings.insert(k.clone(), new_entry);
        }
        out
    }

    /// Apply `subst` in place, rewriting ONLY the entries it can affect —
    /// found through the reverse indexes, so nothing else is even visited.
    /// When the aggregate check says the whole environment is untouched,
    /// this is O(|subst|). Same result as `apply_subst`, without rebuilding
    /// the map.
    pub fn apply_subst_mut(&mut self, subst: &Subst) {
        if !self.affected_by(subst) {
            return;
        }
        // Candidate bindings: consume the index buckets of every variable
        // the substitution binds. A name can sit in several buckets (and
        // stale duplicates exist by design), so dedup before rewriting.
        let mut cand: Vec<String> = Vec::new();
        for v in subst.ty_domain() {
            if let Some(names) = self.fv_index.remove(v) {
                cand.extend(names);
            }
        }
        for id in subst.mult_domain() {
            if let Some(names) = self.mult_id_index.remove(&id) {
                cand.extend(names);
            }
        }
        let mut seen: HashSet<String> = HashSet::with_capacity(cand.len());
        for name in cand {
            if !seen.insert(name.clone()) {
                continue;
            }
            let TypeEnv { bindings, fv_counts, rigid_counts, mult_id_counts, fv_index, mult_id_index } = self;
            let Some(entry) = bindings.get_mut(&name) else { continue };
            if !entry.affected_by(subst) {
                continue; // stale index entry; dropped with its bucket
            }
            uncount_entry(entry, fv_counts, rigid_counts, mult_id_counts);
            *entry = EnvEntry::new(entry.scheme.apply_subst(subst));
            count_entry(entry, fv_counts, rigid_counts, mult_id_counts);
            Self::index_entry(entry, &name, fv_index, mult_id_index);
        }
    }
}

/// The direction a value crosses the FFI boundary. Used by the export-type
/// whitelist (`ffi_marshallable` / `validate_top_level_callback`): an export
/// argument and a callback's result are `Import`; an export result and a
/// callback's argument are `Export`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FfiDir {
    /// Lua → mata-ll: an export ARGUMENT, or (after unwrapping its effect) a
    /// callback's own result. Decoded by the argument-decode path.
    Import,
    /// mata-ll → Lua: an export RESULT, or a callback's own argument. Marshalled
    /// by the result path.
    Export,
}

/// The scalar type names the FFI marshaller round-trips as bare Lua values
/// (derived from `codegen::Codegen::scalar_lua_type`: numbers, strings,
/// booleans). Keep in sync with that function.
fn ffi_scalar_name(n: &str) -> bool {
    matches!(n,
        "Int" | "Number" | "Double" | "Float"
        | "String" | "Char" | "ByteString" | "Bool")
}

/// Peel `App(App(Con(H), a), b)` into `(Some("H"), [a, b])`; the argument-source
/// order dual of `codegen::decompose_app`, so `HashMap k v` → `("HashMap",
/// [k, v])` and `Tree a` → `("Tree", [a])`. A non-`Con` head yields `None`.
fn decompose_ty_app(ty: &Ty) -> (Option<&str>, Vec<&Ty>) {
    let mut args: Vec<&Ty> = Vec::new();
    let mut head = ty;
    while let Ty::App(f, a) = head {
        args.push(a.as_ref());
        head = f.as_ref();
    }
    args.reverse();
    match head {
        Ty::Con(n) => (Some(n.as_str()), args),
        _ => (None, args),
    }
}

/// Rename EVERY free type variable in the given types to friendly single
/// letters (`a`, `b`, …), sharing one map so the same variable reads the same
/// on every side of the message. Unlike the diagnostic renderer's
/// `pretty_var_subst` — which preserves user-written names — this renames a
/// freshened user var (`a890`) too, because an export error's variables have no
/// meaningful source name to keep (the export is being rejected *for* being
/// polymorphic) and a leaked internal spelling is noise.
fn friendly_export_tys(tys: &[&Ty]) -> Vec<Ty> {
    let mut vars: Vec<TyVar> = Vec::new();
    for t in tys {
        for v in t.free_vars() {
            if !vars.contains(&v) { vars.push(v); }
        }
    }
    let mut map: HashMap<TyVar, Ty> = HashMap::new();
    for (i, v) in vars.iter().enumerate() {
        let letter = (b'a' + (i % 26) as u8) as char;
        let name = if i < 26 { letter.to_string() } else { format!("{}{}", letter, i / 26) };
        map.insert(v.clone(), Ty::Var(TyVar { name, id: v.id }));
    }
    let sub = Subst::from_map(map);
    tys.iter().map(|t| t.apply_subst(&sub)).collect()
}

/// The `note:` line explaining why `culprit` cannot cross in direction `dir`.
fn export_ffi_note(culprit: &Ty, dir: FfiDir) -> String {
    let (head, _) = decompose_ty_app(culprit);
    match culprit {
        Ty::Var(_) | Ty::Skolem(..) =>
            "Lua has no representation for a polymorphic value; give the export a \
             concrete type.".to_string(),
        Ty::IO(_) | Ty::LuaIO(_, _) if dir == FfiDir::Import =>
            "a Lua caller cannot supply an IO/LuaIO action; only a callback (a \
             function returning LuaIO) may cross inward.".to_string(),
        Ty::Arrow(..) =>
            "a callback is only marshalled as a DIRECT top-level argument of the \
             export (returning LuaIO); a function nested in a container/result, or \
             a callback that itself takes a callback, is not.".to_string(),
        _ if matches!(head, Some("ST") | Some("STArray") | Some("STRef")) =>
            "an ST handle is region-scoped (it must not outlive its runST) and has \
             no Lua representation.".to_string(),
        // A plain user/prelude `data` type (Either/Ordering/ExitValue or a
        // user ADT) reached here has real constructors but no designed FFI
        // shape — it would cross only as its internal `{tag, fields…}` table,
        // which the host cannot interpret. Say so and point at the fixes.
        _ if head.is_some() =>
            "this type has real constructors but no FFI representation: it would \
             cross only as mata-ll's internal `{tag, ...}` tagged table, which has \
             no meaning to a Lua host. To carry structured data, use a `LuaDict` \
             record (a name-keyed table); for a dynamic scalar, use `Any`; or \
             encode the value as a scalar or a list. A newtype over a marshallable \
             type crosses transparently; a plain `data` type does not."
                .to_string(),
        _ =>
            "only scalars, (), lists, tuples, Maybe, HashMap, LuaDict records, \
             newtypes over marshallable types, Any, and a top-level callback may \
             cross the FFI boundary. Either crosses only as a LuaTry/LuaIOCatch \
             result.".to_string(),
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
    /// Class constraints declared on the existential variables
    /// (`forall a. Show a => MkBox a`): the classes the constructor
    /// GUARANTEES for each hidden type. Enforced in both directions —
    /// construction emits a wanted constraint (the packed type must have the
    /// instance), and unpacking makes exactly these classes available on the
    /// skolem that replaces the hidden variable.
    pub existential_constraints: Vec<TyConstraint>,
}

/// Provenance of a skolem constant minted for an EXISTENTIAL type variable
/// when its constructor was unpacked in a pattern. Skolems without an entry
/// here are rank-2 sealing skolems (their constraints are the caller's to
/// discharge); existential skolems carry exactly the classes the constructor
/// declared (`givens`) and are used to build precise diagnostics.
#[derive(Debug, Clone)]
pub(super) struct ExSkolemInfo {
    /// The source-level variable name (`a` in `forall a. …`).
    pub var: String,
    /// The constructor whose pattern match introduced the skolem (for an
    /// existential), or a phrase naming the function signature (for a
    /// signature skolem — see `origin`).
    pub con: String,
    /// Class names the declared context guarantees for it (a constructor's
    /// existential context, or a function signature's `=>` context).
    pub givens: Vec<String>,
    /// Where this skolem came from, governing the provenance note wording.
    pub origin: SkolemOrigin,
}

/// Why a skolem constant was minted, for diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum SkolemOrigin {
    /// An existential type variable erased when a constructor was packed.
    Existential,
    /// A universally-quantified variable of a function's own signature,
    /// rigid while its body is checked. `fn_name` is the function being
    /// checked.
    Signature { fn_name: String },
    /// A variable bound by a higher-rank argument type (`f :: (forall a. …) ->
    /// …`), rigid while the *argument* is checked against it. The argument must
    /// work for every `a`, so any class constraint its body demands of `a` is
    /// unsatisfiable — the value is not polymorphic enough.
    Rank2Arg,
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
    /// Current recursion depth of expression inference (`infer_expr` /
    /// `check_expr_typed`), bounded by `crate::MAX_NESTING_DEPTH`. The
    /// desugared AST can nest far deeper than the source text (a list
    /// literal becomes a cons chain, one level per element), so the parser's
    /// own depth guard does not bound this walk — it needs its own.
    pub(super) expr_depth: usize,
    /// Span of the innermost statement-boundary expression (an `Expr::Spanned`
    /// marker: a do-statement, let/where binding body, case-branch or guard
    /// body) whose checking failed. Set as the error propagates OUT through the
    /// `Spanned` arm of `infer_expr_inner`; the deepest marker records its span
    /// first and outer ones leave it (set-if-`None`), so a diagnostic lands on
    /// the offending statement's line, not the clause head. A clause-level
    /// handler `take()`s it, falling back to the clause span when no statement
    /// marker was crossed. Reset per clause in `check_clause`.
    pub(super) error_span: Option<crate::ast::Span>,
    /// Current recursion depth of `ast_type_to_ty`, bounded by
    /// `crate::MAX_NESTING_DEPTH`. Type-alias and type-family expansion can
    /// deepen a type far beyond its written form (and a self-referential
    /// alias would recurse forever), so the guard counts recursion here, not
    /// source syntax.
    type_depth: usize,
    /// Fuel for type-alias expansion within resolving ONE top-level type,
    /// reset each time `ast_type_to_ty` is entered at depth 0. The depth
    /// guard alone cannot bound alias expansion: a self-doubling tower
    /// (`type Pi a = P{i-1} (P{i-1} a)`) expands to a type whose SIZE is
    /// exponential in the number of levels while its DEPTH stays small (P8
    /// has depth ~256 but ~2^256 nodes), so it would grind for ages —
    /// exponential WORK, not deep recursion. Charged by the size of each
    /// expanded alias body (see `charge_alias_expansion`), mirroring the
    /// type-family reducer's size-charged fuel (`types::ty_size_up_to`), so
    /// an exponential tower exhausts it almost immediately while ordinary
    /// (few-level) alias use is unaffected.
    alias_fuel: u32,
    /// True once the alias-expansion-too-large diagnostic has been pushed for
    /// the current module, so the same error is not reported repeatedly.
    alias_reported: bool,
    env: TypeEnv,
    next_var: u32,
    /// Counter for multiplicity unification variables (`Mult::Var`), a
    /// namespace separate from type-variable ids so minting one never
    /// perturbs type-variable numbering (and therefore diagnostics) in
    /// programs that never mention `%1`.
    next_mult: u32,
    /// Named multiplicity variables of the signature currently being
    /// converted by `ast_type_to_ty`: source name → rigid id, so every `%m`
    /// of one signature is the same `Mult::Rigid`. Reset at each top-level
    /// conversion (`type_depth == 0`).
    sig_mult_vars: HashMap<String, u32>,
    constructors: HashMap<String, ConInfo>,
    pub errors: Vec<Diagnostic>,
    current_fn: Option<String>,
    /// Registered typeclasses
    classes: HashMap<String, ClassInfo>,
    /// Registered instances, keyed by the *structured* head constructor of the
    /// instance's target type (see `InstHead`). One instance per (class, head):
    /// this is the canonical instance identity shared with the monomorphizer.
    instances: HashMap<(String, InstHead), InstanceInfo>,
    /// (class, head) keys of the SOURCE `instance` declarations already
    /// checked, to reject a second declaration for the same head. Instance
    /// dispatch keys on the head constructor alone (`InstHead`), so two
    /// declarations that share a head — a genuine duplicate (`instance Greet
    /// Int` twice) or two argument-specialized heads (`Pretty [Int]`
    /// and `Pretty [Bool]`, both `List`) — would silently mis-dispatch. Caught
    /// at declaration instead, like GHC's duplicate/overlapping-instance error.
    checked_instance_heads: HashSet<(String, InstHead)>,
    /// Record field accessors: field_name -> (type_name, lua_index)
    pub record_fields: HashMap<String, (String, usize)>,
    /// Type names that derive `LuaDict` (validated in `derive_luadict`): their
    /// constructor emits a name-keyed Lua table rather than a positional one.
    luadict_types: HashSet<String>,
    /// Type names declared with `newtype` (registered by `register_newtype`).
    /// A newtype is transparent — codegen represents the value AS its single
    /// underlying field with no wrapper — so it is FFI-marshallable exactly when
    /// that field is, unlike a `data` type of identical shape (which would only
    /// cross as an internal tagged table). The FFI boundary check
    /// (`ffi_marshallable`) needs this to tell the two apart precisely, since a
    /// newtype and a single-constructor-single-field `data` are otherwise
    /// indistinguishable in `constructors`.
    newtype_types: HashSet<String>,
    /// User-defined type families: name -> equations (AST form). Reduced
    /// eagerly on CONCRETE arguments during `ast_type_to_ty`.
    type_families: HashMap<String, Vec<TypeFamilyEq>>,
    /// The same families lowered to `Ty` form for reduction DURING
    /// unification (symbolic reduction over type variables). Built once by
    /// `build_ty_families` after pass 2 registers `type_families`, and handed
    /// to the unifier via `Checker::unify`. Empty for programs with no
    /// families, in which case unification takes its plain syntactic path.
    ty_families: TyFamilies,
    /// While true, `try_reduce_type_family` does not reduce — used to lower a
    /// family's own equations to raw `Ty` form (for `ty_families`) without the
    /// eager AST reduction firing on them.
    tf_lowering: bool,
    /// True once a divergent family has been reported, so the same divergence
    /// is not reported repeatedly.
    tf_reported_divergence: bool,
    /// Type aliases: name -> (params, expanded type)
    type_aliases: HashMap<String, (Vec<String>, Type)>,
    /// Kind table: type constructor name -> kind. Builtins are seeded by
    /// `init_kinds`; everything a module declares (data, newtype, alias,
    /// type family) is inferred by `infer_declared_kinds` (typechecker/kind.rs)
    /// before pass 1 registers anything else.
    kinds: HashMap<String, Kind>,
    /// Names of data types that promote to a REAL kind under DataKinds: the
    /// parameterless, non-GADT, non-existential ones (so their promoted kind is
    /// monomorphic — `Nat`, `Color`, plus the builtin `Bool`). A promoted
    /// constructor of such a type gets a `Kind::Promoted` result kind, and its
    /// field types are promoted to kinds through this same set. Every other
    /// data type keeps the historical `Type -> … -> Type` approximation for its
    /// promoted constructors (promoting them would need kind polymorphism).
    /// Computed at the start of `check_module`; seeded with `Bool`.
    promotable_kinds: HashSet<String>,
    /// Class-variable kind table: class name -> the kind its type variable
    /// was inferred at (Show's `a` is Type, Foldable's `t` is Type -> Type).
    /// The class-side counterpart of `kinds`: builtin classes are seeded in
    /// `init_kinds`, user classes inferred order-independently from their
    /// method signatures and superclasses by `infer_class_kinds` (pass 1b,
    /// before pass 2 registers the classes). Instance heads are checked
    /// against this.
    class_kinds: HashMap<String, Kind>,
    /// Names hidden by module export control (imported but not exported).
    /// Only enforced when `enforce_hidden` is true (local code, not imported code).
    hidden_names: HashSet<String>,
    enforce_hidden: bool,
    /// Index where local declarations start (for hidden name enforcement)
    local_decl_start: usize,
    /// Number of leading declarations that belong to the implicit Prelude
    /// (indices `0..prelude_decl_count` of the merged module; imports follow,
    /// then local code). Set by `set_prelude_decl_count` before checking.
    /// Errors produced while checking these declarations are tagged
    /// `Diagnostic::baseline`: the Prelude alone always compiles, so such an
    /// error means the user's program interfered with it (e.g. redefined a
    /// Prelude name) and the caller reports THAT instead of the Prelude line.
    prelude_decl_count: usize,
    /// True while the declaration currently being processed is one of the
    /// Prelude's own (decl index < `prelude_decl_count`). Maintained alongside
    /// `checking_local` by every decl-processing pass.
    checking_prelude: bool,
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
    /// The declared constraints of each checked function, re-expressed a
    /// second time over the FINAL (post-solve, generalized) type's variable
    /// names — the names that appear on the TFunction handed to the
    /// monomorphizer. The dictionary-passing rewrite matches these against
    /// that type; both earlier spellings (source names, freshened sig names)
    /// can disagree with it after unification.
    pub(super) fn_dict_constraints: HashMap<String, Vec<TyConstraint>>,
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
    /// Skolems minted for existential type variables while checking the
    /// current function, as (name, id). `check_pattern` appends; the escape
    /// checks in `check_clause` and `Expr::Case` snapshot the length around a
    /// pattern to know which skolems that pattern introduced. Cleared per
    /// function alongside `wanted`.
    pattern_skolems: Vec<(String, u32)>,
    /// Provenance and declared givens for every existential skolem ever
    /// minted, keyed by skolem id (ids come from `next_var`, so they are
    /// unique program-wide). Consulted by `has_instance` (a wanted on an
    /// existential skolem is satisfied only by the constructor's declared
    /// constraints) and by diagnostic enrichment.
    existential_skolems: HashMap<u32, ExSkolemInfo>,
    /// Record fields whose type mentions their constructor's existential
    /// variables, mapped to the constructor name. Such a field has no usable
    /// selector (the selector's result type would BE the hidden type, walking
    /// it straight out of any match scope) and cannot be record-updated (the
    /// new value's type cannot be checked against a type that was erased).
    /// Registered instead of a selector so both uses get a real explanation
    /// rather than "unbound variable". Same restriction as GHC.
    existential_fields: HashMap<String, String>,
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
            expr_depth: 0,
            error_span: None,
            type_depth: 0,
            alias_fuel: 0,
            alias_reported: false,
            env: TypeEnv::new(),
            next_var: 0,
            next_mult: 0,
            sig_mult_vars: HashMap::new(),
            constructors: HashMap::new(),
            errors: Vec::new(),
            current_fn: None,
            classes: HashMap::new(),
            instances: HashMap::new(),
            checked_instance_heads: HashSet::new(),
            record_fields: HashMap::new(),
            luadict_types: HashSet::new(),
            newtype_types: HashSet::new(),
            type_families: HashMap::new(),
            ty_families: TyFamilies::new(),
            tf_lowering: false,
            tf_reported_divergence: false,
            type_aliases: HashMap::new(),
            kinds: HashMap::new(),
            promotable_kinds: HashSet::from(["Bool".to_string()]),
            class_kinds: HashMap::new(),
            hidden_names: HashSet::new(),
            enforce_hidden: false,
            local_decl_start: 0,
            prelude_decl_count: 0,
            checking_prelude: false,
            local_classes: HashSet::new(),
            local_types: HashSet::new(),
            orphan_check_enabled: false,
            fn_constraints: HashMap::new(),
            method_constraints: HashMap::new(),
            fn_use_constraints: HashMap::new(),
            fn_dict_constraints: HashMap::new(),
            wanted: Vec::new(),
            binder_types: Vec::new(),
            fromjson_types: HashSet::new(),
            tojson_types: HashSet::new(),
            local_con_keys: HashSet::new(),
            local_con_renames: HashMap::new(),
            checking_local: false,
            pattern_skolems: Vec::new(),
            existential_skolems: HashMap::new(),
            existential_fields: HashMap::new(),
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

    /// A fresh multiplicity variable, for arrows the inference engine invents
    /// (the expected arrow at an application, a lambda's own arrows). It
    /// unifies with whichever multiplicity the program provides; left
    /// unconstrained it behaves as `Many` everywhere.
    fn fresh_mult(&mut self) -> Mult {
        let id = self.next_mult;
        self.next_mult += 1;
        Mult::Var(id)
    }

    fn instantiate(&mut self, scheme: &Scheme) -> Ty {
        self.instantiate_with_map(scheme).0
    }

    /// Instantiate a scheme, also returning the var→fresh-type map so callers
    /// can relate a class constraint's bound variable to its fresh type.
    /// Quantified multiplicity variables (`Scheme::mult_vars`) are freshened
    /// too: each use of a multiplicity-polymorphic function gets its own
    /// flexible `Mult::Var` per quantified `%m`, which then unifies with
    /// whatever the call site provides — `One` from a `%1` context, `Many`
    /// from an unrestricted one.
    fn instantiate_with_map(&mut self, scheme: &Scheme) -> (Ty, HashMap<TyVar, Ty>) {
        let mut map = HashMap::new();
        for v in &scheme.vars {
            if let Ty::Var(fresh) = self.fresh_var("_i") {
                map.insert(v.clone(), Ty::Var(fresh));
            }
        }
        let mut mults = HashMap::new();
        for id in &scheme.mult_vars {
            mults.insert(*id, self.fresh_mult());
        }
        (scheme.ty.apply_subst(&Subst::from_parts(map.clone(), mults)), map)
    }


    fn generalize(&self, env: &TypeEnv, ty: &Ty) -> Scheme {
        // Environment membership is answered by the environment's aggregate
        // free-variable multisets (O(1) per variable) rather than by
        // re-walking every scheme in scope, which made long do-blocks
        // quadratic to typecheck.
        let vars: Vec<TyVar> = ty.free_vars().into_iter()
            .filter(|v| !env.is_free_var(v))
            .collect();
        // Multiplicity polymorphism: quantify the RIGID multiplicity
        // variables (a signature's `%m`), except those an enclosing binder's
        // type still mentions — a local alias of a `%m`-typed value must keep
        // sharing the enclosing signature's variable, or a use of the alias
        // could re-instantiate it and claim a multiplicity the value does not
        // have. Flexible `Mult::Var`s are deliberately NOT quantified (see
        // `Scheme::mult_vars`).
        let mut ty_mults = Vec::new();
        ty.collect_rigid_mults(&mut ty_mults);
        let mult_vars: Vec<u32> = ty_mults.into_iter()
            .filter(|id| !env.has_free_rigid_mult(*id))
            .collect();
        Scheme { vars, mult_vars, ty: ty.clone() }
    }

    /// Depth-guard wrapper around `ast_type_to_ty_inner`: past the limit it
    /// reports a clean "type nested too deeply" diagnostic (once) and returns
    /// a placeholder instead of recursing further — the errors list is
    /// non-empty, so compilation stops after this pass. Checked BEFORE
    /// descending, so the walk itself can never overflow the native stack.
    fn ast_type_to_ty(&mut self, ast_ty: &Type) -> Ty {
        // Fresh alias-expansion fuel per top-level type resolution, so a big
        // program with many small types never accumulates a false trip, while
        // any single type that expands exponentially exhausts it (see
        // `charge_alias_expansion` / `ALIAS_EXPAND_FUEL`).
        if self.type_depth == 0 {
            self.alias_fuel = ALIAS_EXPAND_FUEL;
            // Named multiplicity variables are scoped to one signature: each
            // top-level type conversion starts a fresh name→id map, so `%m`
            // in two different signatures is two different variables.
            self.sig_mult_vars.clear();
        }
        if self.type_depth >= crate::MAX_NESTING_DEPTH {
            let already_reported = self.errors.iter().any(|e| {
                matches!(&e.kind, DiagnosticKind::Other(m) if m.starts_with("type nested too deeply"))
            });
            if !already_reported {
                let mut diag = Diagnostic::new(DiagnosticKind::Other(format!(
                    "type nested too deeply (limit {})",
                    crate::MAX_NESTING_DEPTH
                )));
                diag.notes.push(
                    "the compiler resolves types (including type-alias and \
                     type-family expansion) with bounded recursion so it can \
                     report this error instead of crashing; a self-referential \
                     alias like `type A = [A]` also ends up here, because its \
                     expansion never terminates"
                        .to_string(),
                );
                self.errors.push(diag);
            }
            return Ty::Unit;
        }
        self.type_depth += 1;
        let ty = self.ast_type_to_ty_inner(ast_ty);
        self.type_depth -= 1;
        ty
    }

    /// Charge alias-expansion fuel by the size of a freshly expanded alias
    /// body, BEFORE recursing into it. Returns `true` while fuel remains and
    /// `false` once it is exhausted — in which case a clean "type alias
    /// expansion did not terminate" diagnostic is pushed (once) and the caller
    /// must stop expanding (return a placeholder) rather than build the rest of
    /// the exponentially large type. Charging by size (mirroring the
    /// type-family reducer, which charges `ty_size_up_to` per reduced type)
    /// means a self-doubling tower — which produces exponentially many, and
    /// exponentially larger, expansions — drains the budget almost at once.
    fn charge_alias_expansion(&mut self, expanded: &Type) -> bool {
        let cost = ast_type_size_up_to(expanded, self.alias_fuel);
        if cost >= self.alias_fuel {
            self.alias_fuel = 0;
            if !self.alias_reported {
                self.alias_reported = true;
                let ctx = match &self.current_fn {
                    Some(name) => format!("the type signature of '{}'", name),
                    None => "a type signature".to_string(),
                };
                let mut diag = Diagnostic::new(DiagnosticKind::Other(
                    "type alias expansion did not terminate: expanding the type \
                     aliases in this signature produced a type too large to \
                     represent (it exceeded the alias-expansion size limit), so \
                     the aliases appear to grow without bound"
                        .to_string(),
                ));
                diag.context = Some(ctx);
                diag.notes.push(
                    "a self-referential alias (`type A = [A]`) or a doubling \
                     tower (`type Pi a = P(i-1) (P(i-1) a)`, where each level \
                     doubles the expanded size) has no finite normal form; \
                     mata-ll bounds alias expansion by the size of the result \
                     so it reports this instead of looping"
                        .to_string(),
                );
                self.errors.push(diag);
            }
            return false;
        }
        self.alias_fuel -= cost;
        true
    }

    fn ast_type_to_ty_inner(&mut self, ast_ty: &Type) -> Ty {
        match ast_ty {
            Type::Con(name) => {
                // Check for type alias expansion
                if let Some((params, alias_ty)) = self.type_aliases.get(name).cloned()
                    && params.is_empty() {
                        // Charge by the size of the alias body before expanding
                        // — a nullary self-referential alias (`type A = [A]`)
                        // grows without bound and must be reported, not looped.
                        if !self.charge_alias_expansion(&alias_ty) {
                            return Ty::Unit;
                        }
                        return self.ast_type_to_ty(&alias_ty);
                    }
                    // Parameterized alias used without args — treat as constructor
                Ty::Con(name.clone())
            }
            Type::Var(name) => Ty::Var(TyVar { name: name.clone(), id: u32::MAX }),
            Type::Arrow(a, b, m) => {
                // Multiplicity annotations: `%1`/`%Many` are the constants; a
                // named variable (`a %m -> b`) resolves to ONE rigid
                // multiplicity variable per distinct name within the type
                // currently being converted (`sig_mult_vars`, reset at each
                // top-level entry) — the multiplicity counterpart of a
                // signature type variable.
                let mult = match m {
                    MultAnn::One => Mult::One,
                    MultAnn::Many => Mult::Many,
                    MultAnn::Var(name) => {
                        if let Some(id) = self.sig_mult_vars.get(name) {
                            Mult::Rigid(*id)
                        } else {
                            let Mult::Var(id) = self.fresh_mult() else { unreachable!() };
                            self.sig_mult_vars.insert(name.clone(), id);
                            Mult::Rigid(id)
                        }
                    }
                };
                Ty::arrow_m(self.ast_type_to_ty(a), self.ast_type_to_ty(b), mult)
            }
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
            // LuaIterator "name" [E]  reduces to  [E]: the type argument names
            // the RESULT of collecting the iterator, always an explicit list,
            // and each step yields one `E` (decoded at the call site, see
            // codegen `__mll_iter:`). The parser has already checked the list
            // shape, so reduce it as-is.
            Type::LuaIterator { result, .. } => self.ast_type_to_ty(result),
            Type::Tuple(elems) => Ty::Tuple(elems.iter().map(|t| self.ast_type_to_ty(t)).collect()),
            // LuaTry "name" (Either String T)  reduces to  IO (Either String T)
            // (the parser has already checked the `Either String a` shape).
            Type::LuaTry { result, .. } => Ty::io(self.ast_type_to_ty(result)),
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
        // While lowering a family's own equations to raw `Ty` form for
        // `ty_families`, do not reduce — the equations must stay unreduced so
        // the unifier's normalizer can reduce them (with fuel) later.
        if self.tf_lowering {
            return None;
        }
        // Collect the head and args from nested App: F a b -> (F, [a, b])
        let mut args = Vec::new();
        let mut head = ty;
        while let Type::App(f, a) = head {
            args.push(a.as_ref());
            head = f.as_ref();
        }
        args.reverse();

        let family_name = match head {
            Type::Con(name) => name.clone(),
            _ => return None,
        };

        if !self.type_families.contains_key(&family_name) {
            return None;
        }

        // Delegate to the ONE Ty-level reduction engine (tf_reduce_head via
        // reduce_type_families) instead of a second, AST-level matcher. The
        // AST matcher was a duplicated engine with its own gaps — it had no
        // promoted-constructor case, so `F 'Z` failed its specific clause
        // and fell through to a catch-all — and no apartness rule. Lower the
        // whole application to raw `Ty` form (no eager reduction) and let
        // the shared iterative, fuel-bounded, apartness-checking normalizer
        // do the reduction.
        //
        // Families are registered across several passes, so the lowered
        // `ty_families` may lag `type_families`; rebuild when they disagree
        // so this eager path never reduces against a stale set.
        if self.ty_families.len() != self.type_families.len() {
            self.build_ty_families();
        }
        let saved = self.tf_lowering;
        self.tf_lowering = true;
        let raw = self.ast_type_to_ty(ty);
        self.tf_lowering = saved;
        Some(match reduce_type_families(&raw, &self.ty_families) {
            Ok(reduced) => reduced,
            Err(_diverged) => {
                if !self.tf_reported_divergence {
                    self.tf_reported_divergence = true;
                    self.push_error_ctx(
                        DiagnosticKind::TypeFamilyDivergence(family_name.clone()),
                        format!("the type family '{}'", family_name),
                    );
                }
                raw
            }
        })
    }

    /// Lower every registered type family's equations to raw `Ty` form (no
    /// reduction, so a family application in a result stays a `Con`-headed
    /// application the unifier's normalizer can reduce later) and store them
    /// in `ty_families`. Run once after pass 2 registers the families, before
    /// any unification can see a family-typed signature.
    fn build_ty_families(&mut self) {
        if self.type_families.is_empty() {
            return;
        }
        let families = self.type_families.clone();
        self.tf_lowering = true;
        for (name, eqs) in &families {
            let lowered: Vec<(Vec<Ty>, Ty)> = eqs
                .iter()
                .map(|eq| {
                    let pats = eq.args.iter().map(|p| self.ast_type_to_ty(p)).collect();
                    let result = self.ast_type_to_ty(&eq.result);
                    (pats, result)
                })
                .collect();
            self.ty_families.insert(name.clone(), lowered);
        }
        self.tf_lowering = false;
    }

    /// Unification that reduces closed type families (from `ty_families`)
    /// while matching — the checker's standard entry point, replacing the bare
    /// `unify` free function at call sites that may see family-typed values.
    pub(super) fn unify(&self, t1: &Ty, t2: &Ty) -> Result<Subst, DiagnosticKind> {
        unify_tf(t1, t2, &self.ty_families)
    }

    /// Expand a type alias application: `AliasName arg1 arg2` → substituted body.
    fn try_expand_type_alias(&mut self, ty: &Type) -> Option<Ty> {
        let mut args = Vec::new();
        let mut head = ty;
        while let Type::App(f, a) = head {
            args.push(a.as_ref());
            head = f.as_ref();
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
        // Charge by the size of the expanded body before recursing into it. A
        // doubling tower expands to an exponentially large type; charging per
        // expansion drains the fuel and trips the diagnostic long before the
        // full type is built (which would otherwise take exponential time).
        if !self.charge_alias_expansion(&expanded) {
            return Some(Ty::Unit);
        }
        Some(self.ast_type_to_ty(&expanded))
    }

    /// Match a type pattern against an actual type, collecting variable bindings.
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
            Type::Arrow(a, b, m) => Type::Arrow(
                Box::new(self.substitute_type(a, bindings)),
                Box::new(self.substitute_type(b, bindings)),
                m.clone(),
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
            Scheme { vars, mult_vars: vec![], ty: current.clone() }
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

    /// Mint the rigid skolems used to check a function's clause BODIES against
    /// its signature, so a body more general than the declared type is rejected
    /// (`f :: a -> Int` / `f x = x` may not narrow `a` to `Int`).
    ///
    /// `fresh_ty` is the already-freshened signature (its universally-quantified
    /// variables are fresh FLEXIBLE `Ty::Var`s). This mints one fresh rigid
    /// skolem per such variable and returns:
    ///   - `sig_skolems`: flexible signature variable → its skolem. `check_clause`
    ///     substitutes these into the body's EXPECTED types AFTER pattern
    ///     checking — but only for a variable a pattern did not already pin, so
    ///     a GADT match that refines a signature variable to a concrete index
    ///     (`s := 'Empty`) is untouched and the skolem never appears.
    ///   - `demote`: skolem id → the flexible variable, applied once the body
    ///     checks (`Ty::demote_skolems`) so every downstream pass and the caller-
    ///     visible type see ordinary `Ty::Var`s. Kept as a plain map (not a
    ///     `Subst` field) so the substitution carried on every hot inference
    ///     frame stays small.
    ///
    /// Each skolem is registered in `existential_skolems` with the classes the
    /// declared context guarantees for its variable, so `has_instance` and the
    /// wanted-constraint discharge treat a `Monad m =>`-provided skolem as
    /// satisfied while a bare, unconstrained signature skolem has no instance.
    fn skolemize_sig_body_vars(
        &mut self,
        fresh_ty: &Ty,
        declared_constraints: &[TyConstraint],
    ) -> (HashMap<TyVar, Ty>, HashMap<u32, Ty>) {
        // The declared context re-expressed over the FRESHENED variable names,
        // so a class can be matched to the fresh variable that carries it. A
        // constraint whose variable is not freshened (an instance-method
        // signature, already alpha-renamed) matches by its own name.
        let free: Vec<TyVar> = fresh_ty.free_vars();
        let mut givens_for: HashMap<String, Vec<String>> = HashMap::new();
        for c in declared_constraints {
            // `c.type_var` here is the SOURCE name; the freshened variable keeps
            // the source name as a prefix, so match by trimming the id suffix is
            // fragile. Instead map by exact source name against the fresh var's
            // name prefix below.
            givens_for.entry(c.type_var.clone()).or_default().push(c.class_name.clone());
        }
        // Recover each fresh variable's SOURCE name. `freshen_sig_type_mapped`
        // builds a fresh name as `<source_name><id>` via `fresh_tyvar`, so the
        // source name is the fresh name with its trailing id digits removed.
        let source_name = |fresh: &TyVar| -> String {
            fresh.name.trim_end_matches(|ch: char| ch.is_ascii_digit()).to_string()
        };

        let fn_name = self.current_fn.clone().unwrap_or_else(|| "this binding".to_string());
        let mut sig_skolems: HashMap<TyVar, Ty> = HashMap::new();
        let mut demote: HashMap<u32, Ty> = HashMap::new();
        for v in &free {
            let sk_id = self.next_var;
            self.next_var += 1;
            // Name the skolem by the SOURCE variable (`a`, not the freshened
            // `a1050`) so a rigidity diagnostic prints the name the user wrote.
            let sname = source_name(v);
            sig_skolems.insert(v.clone(), Ty::Skolem(sname.clone(), sk_id));
            demote.insert(sk_id, Ty::Var(v.clone()));
            let givens = givens_for.get(&sname).cloned().unwrap_or_default();
            self.existential_skolems.insert(sk_id, ExSkolemInfo {
                var: sname.clone(),
                con: format!("the signature of '{}'", fn_name),
                givens,
                origin: SkolemOrigin::Signature { fn_name: fn_name.clone() },
            });
        }
        (sig_skolems, demote)
    }

    fn push_error_ctx(&mut self, kind: DiagnosticKind, ctx: String) {
        let baseline = self.checking_prelude;
        let notes = self.existential_provenance_notes(&kind);
        self.errors.push(Diagnostic { kind, context: Some(ctx), span: None, file: None, notes, baseline });
    }

    fn push_error_span(&mut self, kind: DiagnosticKind, ctx: String, span: Span) {
        let baseline = self.checking_prelude;
        let notes = self.existential_provenance_notes(&kind);
        self.errors.push(Diagnostic { kind, context: Some(ctx), span: Some(span), file: None, notes, baseline });
    }

    /// Reject exports whose declared type uses something the FFI marshaller
    /// cannot correctly move across the boundary. Runs on each export's FINAL
    /// resolved type — the same `export_types` the code generator marshals from
    /// — so the whitelist is derived from what `ffi_arg_marshal_desc` /
    /// `ffi_decode_desc_inner` / the deep-force fallback actually handle, and a
    /// silently-wrong `"false"`/`__mll_to_lua` conversion never reaches codegen.
    fn validate_export_types(&mut self, exports: &[String], functions: &[TFunction], constrained: &[String]) {
        let tys: HashMap<&str, &Ty> = functions.iter()
            .filter(|f| exports.contains(&f.name))
            .map(|f| (f.name.as_str(), &f.ty))
            .collect();
        // Deterministic diagnostics: follow the source export order.
        for name in exports {
            // Already rejected for a class constraint — its type variable would
            // be reported a second time here.
            if constrained.contains(name) { continue; }
            let Some(&ty) = tys.get(name.as_str()) else { continue };
            // Peel a leading rank-2 forall (the LuaIO scope variable etc.).
            let mut t = ty;
            while let Ty::Forall(_, inner) = t { t = inner; }
            // An export `A1 -> … -> An -> R` decodes each Ai (Lua→mata-ll, the
            // IMPORT direction) and marshals R (mata-ll→Lua, the EXPORT
            // direction). A top-level argument that is itself a function is a
            // CALLBACK — the one arrow position codegen fully marshals — so it
            // is validated by `validate_top_level_callback`; every other arrow
            // (nested in a container, in the result, or a callback's own
            // argument) is rejected by `ffi_marshallable`.
            let (arg_tys, res) = t.peel_arrows();
            for (i, a) in arg_tys.iter().enumerate() {
                let pos = format!("argument {}", i + 1);
                if matches!(a, Ty::Arrow(..)) {
                    self.validate_top_level_callback(name, &pos, a);
                } else if let Err((culprit, cdir)) =
                    self.ffi_marshallable(a, FfiDir::Import, &mut Vec::new())
                {
                    self.push_export_ffi_error(name, &pos, a, &culprit, cdir);
                }
            }
            if let Err((culprit, cdir)) =
                self.ffi_marshallable(res, FfiDir::Export, &mut Vec::new())
            {
                self.push_export_ffi_error(name, "the result", res, &culprit, cdir);
            }
        }
    }

    /// Whether `ty` can cross the FFI boundary in direction `dir`. Returns the
    /// first offending sub-type (and the direction it was reached in) so the
    /// diagnostic can name the exact culprit.
    ///
    /// A type may cross ONLY if it has DEFINED marshalling behavior — a shape
    /// the host is meant to see, not an internal representation that happens to
    /// be a table. The allowed set:
    ///   - scalars (Int/Number/Double/Float/String/Char/ByteString/Bool),
    ///     `()`, and the opaque `LuaUserData` interop handle;
    ///   - `[a]` iff `a` allowed; tuples iff every element allowed;
    ///   - `HashMap k v` iff `k` is a scalar Lua key and `v` is allowed;
    ///   - `Maybe a` iff `a` allowed (designed: nil ↔ Nothing);
    ///   - `Any` — the dynamic boundary type, allowed by name (its runtime
    ///     conversion is defined by codegen);
    ///   - a `LuaDict` record iff every declared field is allowed (designed: a
    ///     name-keyed table);
    ///   - a NEWTYPE iff its single underlying field is allowed — a newtype is
    ///     transparent (codegen represents the value AS the field, no wrapper),
    ///     so it inherits the field's representation (this is what keeps
    ///     `newtype FileHandle = FileHandle LuaUserData` and the whole `LIO`
    ///     file API crossing);
    ///   - `IO a` / `LuaIO _ a` in EXPORT (result) position iff `a` is allowed.
    ///
    /// A recursive newtype/record re-entry is cycle-guarded and passes as opaque.
    ///
    /// Everything else is REJECTED. In particular a plain user `data` type with
    /// real constructors — a multi-constructor and/or multi-field ADT — is
    /// refused EVEN WHEN its fields would each marshal, because it would cross
    /// only as its internal `{tag, fields…}` table, which has no host-facing
    /// meaning. This is where prelude ADTs `Either`/`Ordering`/`ExitValue` land
    /// when used outside their one designed context (the `Either String a` of a
    /// `LuaTry`/`LuaIOCatch` result is peeled and validated by its caller, not
    /// here). Also rejected: a bare type variable (no runtime rep for a
    /// polymorphic value), a region-scoped `ST`/`STArray`/`STRef` handle, `IO`/
    /// `LuaIO` in IMPORT (argument) position, and any unknown constructor.
    ///
    /// A FUNCTION type is rejected here in EVERY position. Codegen only fully
    /// marshals a callback when it is a DIRECT top-level export argument (the
    /// branch emitting `__mll_wrap_callback_in`); a function nested inside a
    /// container/result — or a callback's own function-typed argument — is
    /// passed opaque (`"false"` descriptor) and would leak. So the only accepted
    /// arrow position is handled separately by `validate_top_level_callback`,
    /// and this recursive check treats any arrow it reaches as unmarshallable.
    fn ffi_marshallable(&self, ty: &Ty, dir: FfiDir, visited: &mut Vec<String>)
        -> Result<(), (Ty, FfiDir)>
    {
        self.ffi_marshallable_allowing(ty, dir, &[], visited)
    }

    /// As `ffi_marshallable`, but a bare type variable in `opaque` is accepted
    /// rather than rejected. This is the ONE designed exception: the threaded
    /// STATE of a polymorphic outgoing-callback FFI (the fold pattern) crosses
    /// Lua opaquely, and `validate_ffi_callbacks` already enforces that it is a
    /// single shared variable threaded soundly through the callback's
    /// accumulator, the callback's result, an FFI argument, and the FFI return.
    /// So when validating such an import, those state variables are whitelisted;
    /// every OTHER type variable (and every other position) is checked as usual.
    /// Exports and non-stateful imports pass an empty `opaque`.
    fn ffi_marshallable_allowing(&self, ty: &Ty, dir: FfiDir, opaque: &[TyVar], visited: &mut Vec<String>)
        -> Result<(), (Ty, FfiDir)>
    {
        match ty {
            Ty::Forall(_, inner) => self.ffi_marshallable_allowing(inner, dir, opaque, visited),
            Ty::Unit => Ok(()),
            Ty::Con(n) if ffi_scalar_name(n) || n == "LuaUserData" => Ok(()),
            // A whitelisted opaque state variable (see the doc above) round-trips.
            Ty::Var(v) if opaque.contains(v) => Ok(()),
            Ty::Var(_) | Ty::Skolem(..) | Ty::Promoted(_) => Err((ty.clone(), dir)),
            Ty::List(a) => self.ffi_marshallable_allowing(a, dir, opaque, visited),
            Ty::Tuple(es) => {
                for e in es { self.ffi_marshallable_allowing(e, dir, opaque, visited)?; }
                Ok(())
            }
            // An action can be a RESULT (the export performs it and marshals the
            // yielded value) but never an ARGUMENT — Lua has nothing to hand in.
            Ty::IO(a) | Ty::LuaIO(_, a) => match dir {
                FfiDir::Export => self.ffi_marshallable_allowing(a, FfiDir::Export, opaque, visited),
                FfiDir::Import => Err((ty.clone(), dir)),
            },
            // A function reached in any recursive position (nested in a
            // container, in result position, or as a callback's own argument)
            // is NOT marshalled by codegen — reject it. The one accepted arrow,
            // a top-level export-argument callback, never reaches here.
            Ty::Arrow(..) => Err((ty.clone(), dir)),
            Ty::App(..) | Ty::Con(_) => {
                let (head, args) = decompose_ty_app(ty);
                match head {
                    Some("HashMap") if args.len() == 2 => {
                        // Keys are Lua table keys — must be a scalar; values
                        // marshal by the value type in the same direction.
                        if !matches!(args[0], Ty::Con(n) if ffi_scalar_name(n)) {
                            return Err((ty.clone(), dir));
                        }
                        self.ffi_marshallable_allowing(args[1], dir, opaque, visited)
                    }
                    // `Maybe a` has a designed shape at the boundary — `nil` for
                    // Nothing, the marshalled payload for `Just` — so it crosses
                    // iff its type argument crosses in the same direction.
                    Some("Maybe") if args.len() == 1 => {
                        self.ffi_marshallable_allowing(args[0], dir, opaque, visited)
                    }
                    // `Any` is the dynamic FFI boundary type: it has a defined
                    // runtime conversion (supplied by codegen), so it crosses in
                    // both directions regardless of what value it wraps.
                    Some("Any") => Ok(()),
                    Some(name) => {
                        // A recursive type re-entered (a newtype/record whose
                        // field mentions itself): the marshaller treats the
                        // re-entry as opaque and it round-trips, so accept and stop.
                        if visited.iter().any(|v| v == name) {
                            return Ok(());
                        }
                        // Only types with a DEFINED marshalling shape may cross.
                        // Dispatch on the designed representation, not on "does
                        // every field happen to marshal":
                        //   - a newtype is transparent (codegen represents the
                        //     value AS its single field with no wrapper), so it
                        //     crosses iff that field crosses;
                        //   - a LuaDict record crosses as a name-keyed table, so
                        //     it crosses iff every declared field crosses;
                        //   - any other user `data` type — a plain ADT with real
                        //     constructors — would cross only as its internal
                        //     `{tag, fields…}` table, an implementation detail
                        //     with no host-facing meaning, so it is REJECTED even
                        //     when its fields would each marshal. (This is where
                        //     `Either`/`Ordering`/`ExitValue` land outside their
                        //     designed contexts — a plain ADT is a plain ADT.)
                        // No constructors ⇒ an abstract/handle constructor (ST,
                        // STArray, STRef, an unknown type, …) with no marshalling
                        // ⇒ reject.
                        let is_newtype = self.newtype_types.contains(name);
                        let is_luadict = self.luadict_types.contains(name);
                        if !is_newtype && !is_luadict {
                            return Err((ty.clone(), dir));
                        }
                        let cons: Vec<ConInfo> = self.constructors.values()
                            .filter(|c| c.type_name == name)
                            .cloned()
                            .collect();
                        if cons.is_empty() {
                            return Err((ty.clone(), dir));
                        }
                        visited.push(name.to_string());
                        for con in &cons {
                            let mut smap: HashMap<TyVar, Ty> = HashMap::new();
                            for (tv, a) in con.type_vars.iter().zip(args.iter()) {
                                smap.insert(tv.clone(), (*a).clone());
                            }
                            let sub = Subst::from_map(smap);
                            for fty in &con.field_types {
                                let fty = fty.apply_subst(&sub);
                                if let Err(e) = self.ffi_marshallable_allowing(&fty, dir, opaque, visited) {
                                    visited.pop();
                                    return Err(e);
                                }
                            }
                        }
                        visited.pop();
                        Ok(())
                    }
                    None => Err((ty.clone(), dir)),
                }
            }
        }
    }

    /// Validate the ONE arrow position codegen fully marshals: a direct
    /// top-level export argument that is a callback. The codegen (the export
    /// loop's `if matches!(ty, Ty::Arrow(..))` branch, which emits
    /// `__mll_wrap_callback_in`) marshals each callback argument OUT
    /// (mata-ll→Lua, the EXPORT direction) with `ffi_arg_marshal_desc` and
    /// decodes the callback's result IN (Lua→mata-ll, the IMPORT direction)
    /// after unwrapping its `LuaIO`/`IO` — but it hard-codes the opaque
    /// `"false"` descriptor for a callback argument that is ITSELF a function.
    /// So we accept this callback iff none of its arguments is a function and
    /// every argument (Export) and its unwrapped result (Import) marshal. (The
    /// separate pre-existing rule that the result be an action lives in
    /// `check_export_callbacks`.)
    fn validate_top_level_callback(&mut self, name: &str, position: &str, cb_ty: &Ty) {
        let (cb_args, cb_ret) = cb_ty.peel_arrows();
        for a in &cb_args {
            let cb_pos = format!("{} (a callback argument)", position);
            if matches!(a, Ty::Arrow(..)) {
                // A callback taking a callback: codegen passes the inner
                // function opaque, so reject it (name it via the shared helper).
                self.push_export_ffi_error(name, &cb_pos, cb_ty, a, FfiDir::Export);
            } else if let Err((culprit, cdir)) =
                self.ffi_marshallable(a, FfiDir::Export, &mut Vec::new())
            {
                self.push_export_ffi_error(name, &cb_pos, cb_ty, &culprit, cdir);
            }
        }
        // The callback's result crosses back IN; codegen unwraps its LuaIO/IO
        // and decodes the payload, so validate the payload in Import direction.
        let payload = match cb_ret {
            Ty::IO(a) | Ty::LuaIO(_, a) => a.as_ref(),
            other => other,
        };
        if let Err((culprit, cdir)) =
            self.ffi_marshallable(payload, FfiDir::Import, &mut Vec::new())
        {
            let cb_pos = format!("{} (the callback result)", position);
            self.push_export_ffi_error(name, &cb_pos, cb_ty, &culprit, cdir);
        }
    }

    /// Build the export-boundary diagnostic: name the binder, the position
    /// (argument N / the result), the whole position type and the offending
    /// sub-type, and the crossing direction — with a `note:` explaining WHY the
    /// culprit cannot cross.
    fn push_export_ffi_error(&mut self, name: &str, position: &str, whole: &Ty, culprit: &Ty, dir: FfiDir) {
        let dir_phrase = match dir {
            FfiDir::Import => "cross into mata-ll from Lua (argument direction)",
            FfiDir::Export => "cross out to Lua from mata-ll (result direction)",
        };
        // Rename internal/freshened type variables (e.g. `a890`, `_r7`) to
        // friendly letters, sharing one map so `whole` and `culprit` agree.
        let (whole, culprit) = {
            let pair = friendly_export_tys(&[whole, culprit]);
            (pair[0].clone(), pair[1].clone())
        };
        let nested = if whole == culprit {
            String::new()
        } else {
            format!(" (inside '{}')", whole)
        };
        self.push_error_ctx(
            DiagnosticKind::Other(format!(
                "Export '{}': {} has type '{}', which cannot {} — the type '{}'{} has no FFI marshalling.",
                name, position, whole, dir_phrase, culprit, nested
            )),
            format!("export declaration of '{}'", name),
        );
        let note = export_ffi_note(&culprit, dir);
        if let Some(diag) = self.errors.last_mut() {
            diag.notes.push(note);
        }
    }

    /// Validate the signature of an FFI IMPORT — a `LuaPure`/`LuaIO`/`LuaTry`/
    /// `LuaIOCatch` (or `LuaCatch`/`LuaIterator`) declaration mata-ll calls INTO
    /// Lua with. This is the mirror of `validate_export_types`: an import
    /// `f :: A1 -> … -> An -> R` marshals each `Ai` OUT to Lua (mata-ll→Lua, the
    /// EXPORT direction) and decodes `R` back IN (Lua→mata-ll, the IMPORT
    /// direction), so the arguments and the result are checked in the OPPOSITE
    /// directions to an export. `ty` is the FINAL resolved type (the `LuaIO`/…
    /// wrappers already reduced to `IO`/the raw result), `ffi_kind` says which
    /// FFI form it is so the `Try`/`IOCatch` `Either String _` layer is peeled
    /// exactly as codegen's `ffi_catch_decode_desc` does.
    ///
    /// A function-typed argument is an OUTGOING CALLBACK (mata-ll passes it to
    /// the host, which calls it): its own arguments come IN from Lua (Import) and
    /// its result goes back OUT (Export) — the exact dual of an export-argument
    /// callback — so it is validated with the directions swapped.
    fn validate_ffi_import_types(&mut self, name: &str, ffi_kind: FfiKind, ty: &Ty) {
        let mut t = ty;
        while let Ty::Forall(_, inner) = t { t = inner; }
        let (arg_tys, res) = t.peel_arrows();

        // The threaded-state fold pattern (see `validate_ffi_callbacks`): a
        // polymorphic outgoing callback threads an OPAQUE state variable that
        // round-trips through Lua untouched. `validate_ffi_callbacks` already
        // enforces the soundness of that variable (one shared variable across
        // the callback's accumulator/result and the FFI's initial-state
        // argument/return), so here we whitelist those variables and let every
        // OTHER type still be structurally checked. A callback whose value
        // variables are empty is concrete and contributes none.
        let mut opaque: Vec<TyVar> = Vec::new();
        for cb in arg_tys.iter().filter(|t| matches!(t, Ty::Arrow(..))) {
            for v in callback_value_vars(cb) {
                if !opaque.contains(&v) { opaque.push(v); }
            }
        }

        for (i, a) in arg_tys.iter().enumerate() {
            let pos = format!("argument {}", i + 1);
            if matches!(a, Ty::Arrow(..)) {
                self.validate_ffi_import_callback(name, &pos, a, &opaque);
            } else if let Err((culprit, cdir)) =
                self.ffi_marshallable_allowing(a, FfiDir::Export, &opaque, &mut Vec::new())
            {
                self.push_import_ffi_error(name, &pos, a, &culprit, cdir);
            }
        }

        // The result crosses IN from Lua. For a plain Pure/IO import that is the
        // whole result (its `IO` peeled here, so a bare `IO a` result validates
        // `a`); for a Try/IOCatch import the `pcall` wrapper builds the
        // `Either String _` tags itself, so only the inner payload is decoded —
        // peel `IO`/`Either String` exactly like `ffi_catch_decode_desc`.
        let payload = match ffi_kind {
            FfiKind::Try | FfiKind::Catch | FfiKind::IOCatch => {
                let inner = match res {
                    Ty::IO(a) | Ty::LuaIO(_, a) => a.as_ref(),
                    other => other,
                };
                let (head, args) = decompose_ty_app(inner);
                match head {
                    Some("Either") if args.len() == 2 => args[1],
                    // Not the expected `Either String a` shape: validate the
                    // whole inner result rather than silently skipping it.
                    _ => inner,
                }
            }
            // A plain IO/LuaIO result: decode the yielded value.
            _ => match res {
                Ty::IO(a) | Ty::LuaIO(_, a) => a.as_ref(),
                other => other,
            },
        };
        if let Err((culprit, cdir)) =
            self.ffi_marshallable_allowing(payload, FfiDir::Import, &opaque, &mut Vec::new())
        {
            self.push_import_ffi_error(name, "the result", payload, &culprit, cdir);
        }
    }

    /// Validate an OUTGOING callback argument of an FFI import: mata-ll hands
    /// this function to the host, which invokes it, so its arguments arrive IN
    /// from Lua (Import) and its result is sent back OUT (Export) — the mirror
    /// of `validate_top_level_callback`. A callback nested one level deeper (a
    /// callback that itself takes a callback) is passed opaque by codegen, so it
    /// is rejected. `opaque` is the threaded-state whitelist (see the caller).
    fn validate_ffi_import_callback(&mut self, name: &str, position: &str, cb_ty: &Ty, opaque: &[TyVar]) {
        let (cb_args, cb_ret) = cb_ty.peel_arrows();
        for a in &cb_args {
            let cb_pos = format!("{} (a callback argument)", position);
            if matches!(a, Ty::Arrow(..)) {
                self.push_import_ffi_error(name, &cb_pos, cb_ty, a, FfiDir::Import);
            } else if let Err((culprit, cdir)) =
                self.ffi_marshallable_allowing(a, FfiDir::Import, opaque, &mut Vec::new())
            {
                self.push_import_ffi_error(name, &cb_pos, cb_ty, &culprit, cdir);
            }
        }
        // The callback's result goes back OUT to the host; unwrap its effect.
        let payload = match cb_ret {
            Ty::IO(a) | Ty::LuaIO(_, a) => a.as_ref(),
            other => other,
        };
        if let Err((culprit, cdir)) =
            self.ffi_marshallable_allowing(payload, FfiDir::Export, opaque, &mut Vec::new())
        {
            let cb_pos = format!("{} (the callback result)", position);
            self.push_import_ffi_error(name, &cb_pos, cb_ty, &culprit, cdir);
        }
    }

    /// The import-boundary analogue of `push_export_ffi_error`: same shape and
    /// same `note:` guidance, phrased for an FFI import (a Lua call) rather than
    /// an export.
    fn push_import_ffi_error(&mut self, name: &str, position: &str, whole: &Ty, culprit: &Ty, dir: FfiDir) {
        let dir_phrase = match dir {
            FfiDir::Import => "cross into mata-ll from Lua (the host's return value)",
            FfiDir::Export => "cross out to Lua from mata-ll (an argument to the host)",
        };
        let (whole, culprit) = {
            let pair = friendly_export_tys(&[whole, culprit]);
            (pair[0].clone(), pair[1].clone())
        };
        let nested = if whole == culprit {
            String::new()
        } else {
            format!(" (inside '{}')", whole)
        };
        self.push_error_ctx(
            DiagnosticKind::Other(format!(
                "FFI import '{}': {} has type '{}', which cannot {} — the type '{}'{} has no FFI marshalling.",
                name, position, whole, dir_phrase, culprit, nested
            )),
            format!("FFI declaration of '{}'", name),
        );
        let note = export_ffi_note(&culprit, dir);
        if let Some(diag) = self.errors.last_mut() {
            diag.notes.push(note);
        }
    }

    /// Provenance notes for every existential skolem a diagnostic's types
    /// mention. A skolem prints as a plain type-variable name ('a'), which is
    /// baffling in a message like "Cannot match 'a' with 'Int'" or
    /// "No instance for 'Num a'" unless the reader is told that 'a' is the
    /// type a constructor hid — so every error path (push_error_ctx/_span)
    /// attaches where the skolem came from and what the constructor
    /// guarantees for it. Rank-2 sealing skolems have no entry in
    /// `existential_skolems` and get no note.
    fn existential_provenance_notes(&self, kind: &DiagnosticKind) -> Vec<String> {
        let mut sks: Vec<(String, u32)> = Vec::new();
        match kind {
            DiagnosticKind::Mismatch(a, b) | DiagnosticKind::RigidMismatch(a, b) => {
                a.collect_skolems(&mut sks);
                b.collect_skolems(&mut sks);
            }
            DiagnosticKind::OccursCheck(_, t)
            | DiagnosticKind::NoInstance { ty: t, .. }
            | DiagnosticKind::AmbiguousType { ty: t, .. }
            | DiagnosticKind::MissingContextConstraint { ty: t, .. } => t.collect_skolems(&mut sks),
            DiagnosticKind::TypeSigMismatch { declared, inferred, .. } => {
                declared.collect_skolems(&mut sks);
                inferred.collect_skolems(&mut sks);
            }
            // ExistentialEscape already names the constructor in its message.
            _ => {}
        }
        let mut notes = Vec::new();
        for (_, id) in sks {
            if let Some(info) = self.existential_skolems.get(&id) {
                // A signature skolem: a universally-quantified variable of the
                // function's OWN declared type, rigid while its body is checked.
                // The caller picks its concrete type, so the body may not assume
                // any particular one.
                if let SkolemOrigin::Signature { fn_name } = &info.origin {
                    let guaranteed = if info.givens.is_empty() {
                        "the signature places no constraints on it, so the body cannot assume it supports any operation".to_string()
                    } else {
                        format!("the only thing the body may assume about it is the declared context ({})",
                            info.givens.join(", "))
                    };
                    let note = format!(
                        "'{}' is a rigid type variable from the signature of '{}': the caller chooses what concrete type it is, so inside '{}' it stands for some unknown type and cannot be matched with a specific one — {}",
                        info.var, fn_name, fn_name, guaranteed);
                    if !notes.contains(&note) { notes.push(note); }
                    continue;
                }
                if info.origin == SkolemOrigin::Rank2Arg {
                    let note = format!(
                        "'{}' is bound by a higher-rank argument type (forall {}. …): the value must work for EVERY type, so it cannot demand any class instance for '{}' — a lambda like `\\x -> x + 1` (needing `Num`) or one calling `show x` (needing `Show`) is not polymorphic enough. Pass a value that uses '{}' only as an opaque token.",
                        info.var, info.var, info.var, info.var);
                    if !notes.contains(&note) { notes.push(note); }
                    continue;
                }
                let guaranteed = if info.givens.is_empty() {
                    "the constructor declares no constraints for it, so nothing at all is known about it".to_string()
                } else {
                    format!("the only thing known about it is the constructor's declared context ({})",
                        info.givens.join(", "))
                };
                let note = format!(
                    "'{}' is the existential type hidden by constructor '{}': the concrete type was erased when the value was packed, so inside the match '{}' stands for some unknown, rigid type — {}",
                    info.var, info.con, info.var, guaranteed);
                if notes.contains(&note) {
                    // Two DISTINCT skolems that print identically: the same
                    // constructor unpacked twice. Saying "cannot match 'a'
                    // with 'a'" is baffling without this.
                    notes.push(format!(
                        "the two '{}'s are different types: every unpacking of '{}' hides its own concrete type, so values from two separate matches cannot be assumed to share one",
                        info.var, info.con));
                } else {
                    notes.push(note);
                }
            }
        }
        notes
    }

    fn literal_type(&self, lit: &Literal) -> Ty {
        match lit {
            Literal::Integer(_) => Ty::Con("Int".into()),
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
        //   head, tail, map, filter, take, zipWith, length, reverse
        // (foldr/foldl are Foldable class methods, registered below)
        let entries: Vec<(&str, Vec<TyVar>, Ty)> = vec![
            ("print", vec![], Ty::arrow(Ty::Con("String".into()), Ty::io(Ty::Unit))),
            ("++", vec![a.clone()], Ty::fun(&[Ty::list(ta.clone()), Ty::list(ta.clone())], Ty::list(ta.clone()))),
            ("!!", vec![a.clone()], Ty::fun(&[Ty::list(ta.clone()), Ty::Con("Int".into())], ta.clone())),
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
            self.env.insert(name.into(), Scheme { vars, mult_vars: vec![], ty });
        }
        // HashMap operations (backed by Lua tables)
        let hm = |k: Ty, v: Ty| Ty::app(Ty::app(Ty::Con("HashMap".into()), k), v);
        let hm_kv = hm(ta.clone(), tb.clone());
        let hm_entries: Vec<(&str, Vec<TyVar>, Ty)> = vec![
            ("hmEmpty", vec![a.clone(), b.clone()], hm_kv.clone()),
            ("hmInsert", vec![a.clone(), b.clone()], Ty::fun(&[ta.clone(), tb.clone(), hm_kv.clone()], hm_kv.clone())),
            ("hmLookup", vec![a.clone(), b.clone()], Ty::fun(&[ta.clone(), hm_kv.clone()], Ty::app(Ty::Con("Maybe".into()), tb.clone()))),
            ("hmDelete", vec![a.clone(), b.clone()], Ty::fun(&[ta.clone(), hm_kv.clone()], hm_kv.clone())),
            ("hmSize", vec![a.clone(), b.clone()], Ty::arrow(hm_kv.clone(), Ty::Con("Int".into()))),
            ("hmKeys", vec![a.clone(), b.clone()], Ty::arrow(hm_kv.clone(), Ty::list(ta.clone()))),
            ("hmValues", vec![a.clone(), b.clone()], Ty::arrow(hm_kv.clone(), Ty::list(tb.clone()))),
            ("hmMember", vec![a.clone(), b.clone()], Ty::fun(&[ta.clone(), hm_kv.clone()], Ty::Con("Bool".into()))),
            ("hmFromList", vec![a.clone(), b.clone()], Ty::arrow(Ty::list(Ty::Tuple(vec![ta.clone(), tb.clone()])), hm_kv.clone())),
            ("hmToList", vec![a.clone(), b.clone()], Ty::arrow(hm_kv.clone(), Ty::list(Ty::Tuple(vec![ta.clone(), tb.clone()])))),
        ];
        for (name, vars, ty) in hm_entries {
            self.env.insert(name.into(), Scheme { vars, mult_vars: vec![], ty });
        }

        // ByteString operations (backed by Lua strings as byte arrays)
        let bs = Ty::Con("ByteString".into());
        let int = Ty::Con("Int".into());
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
            self.env.insert(name.into(), Scheme { vars, mult_vars: vec![], ty });
        }

        for name in &["max", "min"] {
            self.env.insert(name.to_string(), Scheme { vars: vec![a.clone()], mult_vars: vec![], ty: Ty::fun(&[ta.clone(), ta.clone()], ta.clone()) });
        }
        // Arithmetic operators are the methods of the numeric classes
        // registered further below (Num for + - *, Fractional for /). Their
        // env schemes are `forall a. a -> a -> a`; the Num/Fractional class
        // constraint on `a` is attached via `method_constraints` in the
        // numeric-class registration block, exactly like `==`/`show`.
        for op in &["+", "-", "*", "/"] {
            self.env.insert(op.to_string(), Scheme { vars: vec![a.clone()], mult_vars: vec![], ty: Ty::fun(&[ta.clone(), ta.clone()], ta.clone()) });
        }
        // Comparison operators will be registered as Ord methods below
        for op in &["&&", "||"] {
            self.env.insert(op.to_string(), Scheme { vars: vec![], mult_vars: vec![], ty: Ty::fun(&[Ty::Con("Bool".into()), Ty::Con("Bool".into())], Ty::Con("Bool".into())) });
        }
        // div/mod/quot/rem are the Integral class methods (`forall a. a->a->a`,
        // constrained to Integral below). Previously monomorphic Int->Int.
        for name in &["mod", "div", "quot", "rem"] {
            self.env.insert(name.to_string(), Scheme { vars: vec![a.clone()], mult_vars: vec![], ty: Ty::fun(&[ta.clone(), ta.clone()], ta.clone()) });
        }
        // List functions that need lazy cons (implemented in Lua runtime)
        self.env.insert("head".into(), Scheme { vars: vec![a.clone()], mult_vars: vec![], ty: Ty::arrow(Ty::list(ta.clone()), ta.clone()) });
        self.env.insert("tail".into(), Scheme { vars: vec![a.clone()], mult_vars: vec![], ty: Ty::arrow(Ty::list(ta.clone()), Ty::list(ta.clone())) });
        self.env.insert("map".into(), Scheme { vars: vec![a.clone(), b.clone()], mult_vars: vec![], ty: Ty::fun(&[Ty::arrow(ta.clone(), tb.clone()), Ty::list(ta.clone())], Ty::list(tb.clone())) });
        self.env.insert("filter".into(), Scheme { vars: vec![a.clone(), b.clone()], mult_vars: vec![], ty: Ty::fun(&[Ty::arrow(ta.clone(), Ty::Con("Bool".into())), Ty::list(ta.clone())], Ty::list(ta.clone())) });
        self.env.insert("take".into(), Scheme { vars: vec![a.clone()], mult_vars: vec![], ty: Ty::fun(&[Ty::Con("Int".into()), Ty::list(ta.clone())], Ty::list(ta.clone())) });
        self.env.insert("drop".into(), Scheme { vars: vec![a.clone()], mult_vars: vec![], ty: Ty::fun(&[Ty::Con("Int".into()), Ty::list(ta.clone())], Ty::list(ta.clone())) });
        self.env.insert("zipWith".into(), Scheme { vars: vec![a.clone(), b.clone(), c.clone()], mult_vars: vec![], ty: Ty::fun(&[Ty::fun(&[ta.clone(), tb.clone()], tc.clone()), Ty::list(ta.clone()), Ty::list(tb.clone())], Ty::list(tc.clone())) });

        // Maybe
        self.constructors.insert("Just".into(), ConInfo { type_name: "Maybe".into(), variant_index: 1, total_variants: 2, field_types: vec![ta.clone()], type_vars: vec![a.clone()], result_type: Ty::app(Ty::Con("Maybe".into()), ta.clone()), existential_vars: vec![], existential_constraints: vec![] });
        self.constructors.insert("Nothing".into(), ConInfo { type_name: "Maybe".into(), variant_index: 2, total_variants: 2, field_types: vec![], type_vars: vec![a.clone()], result_type: Ty::app(Ty::Con("Maybe".into()), ta.clone()), existential_vars: vec![], existential_constraints: vec![] });
        self.env.insert("Just".into(), Scheme { vars: vec![a.clone()], mult_vars: vec![], ty: Ty::arrow(ta.clone(), Ty::app(Ty::Con("Maybe".into()), ta.clone())) });
        self.env.insert("Nothing".into(), Scheme { vars: vec![a.clone()], mult_vars: vec![], ty: Ty::app(Ty::Con("Maybe".into()), ta.clone()) });
        self.env.insert("True".into(), Scheme::mono(Ty::Con("Bool".into())));
        self.env.insert("False".into(), Scheme::mono(Ty::Con("Bool".into())));

        // List constructors
        self.constructors.insert(":".into(), ConInfo {
            type_name: "[]".into(), variant_index: 1, total_variants: 2,
            field_types: vec![ta.clone(), Ty::list(ta.clone())],
            type_vars: vec![a.clone()],
            result_type: Ty::list(ta.clone()),
            existential_vars: vec![],
            existential_constraints: vec![],
        });
        self.constructors.insert("[]".into(), ConInfo {
            type_name: "[]".into(), variant_index: 2, total_variants: 2,
            field_types: vec![],
            type_vars: vec![a.clone()],
            result_type: Ty::list(ta.clone()),
            existential_vars: vec![],
            existential_constraints: vec![],
        });
        // (:) :: a -> [a] -> [a]
        self.env.insert(":".into(), Scheme {
            vars: vec![a.clone()],
            mult_vars: vec![],
            ty: Ty::fun(&[ta.clone(), Ty::list(ta.clone())], Ty::list(ta.clone())),
        });
        // [] :: [a]
        self.env.insert("[]".into(), Scheme {
            vars: vec![a.clone()],
            mult_vars: vec![],
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
            mult_vars: vec![],
            ty: Ty::arrow(Ty::io(ta.clone()), Ty::lua_io(s.clone(), ta.clone())),
        });

        // engage :: LuaFunction s -> a
        // (the type annotation at the call site determines a)
        // At runtime, engage is the identity — the LuaFunction is
        // already a Lua function, engage just satisfies the type system.
        self.env.insert("engage".into(), Scheme {
            vars: vec![a.clone(), s.clone()],
            mult_vars: vec![],
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
            mult_vars: vec![],
            ty: Ty::arrow(
                Ty::Forall(s.clone(), Box::new(st_s(ta.clone()))),
                ta.clone(),
            ),
        });
        // newSTArray :: Int -> Int -> ST s (STArray s)
        self.env.insert("newSTArray".into(), Scheme {
            vars: vec![s.clone()],
            mult_vars: vec![],
            ty: Ty::fun(&[int.clone(), int.clone()], st_s(sta_s.clone())),
        });
        // readSTArray :: STArray s -> Int -> ST s Int
        self.env.insert("readSTArray".into(), Scheme {
            vars: vec![s.clone()],
            mult_vars: vec![],
            ty: Ty::fun(&[sta_s.clone(), int.clone()], st_s(int.clone())),
        });
        // writeSTArray :: STArray s -> Int -> Int -> ST s ()
        self.env.insert("writeSTArray".into(), Scheme {
            vars: vec![s.clone()],
            mult_vars: vec![],
            ty: Ty::fun(&[sta_s.clone(), int.clone(), int.clone()], st_s(Ty::Unit)),
        });
        // modifySTArray :: STArray s -> Int -> (Int -> Int) -> ST s ()
        self.env.insert("modifySTArray".into(), Scheme {
            vars: vec![s.clone()],
            mult_vars: vec![],
            ty: Ty::fun(&[sta_s.clone(), int.clone(), Ty::arrow(int.clone(), int.clone())], st_s(Ty::Unit)),
        });
        // stArrayLength :: STArray s -> ST s Int
        self.env.insert("stArrayLength".into(), Scheme {
            vars: vec![s.clone()],
            mult_vars: vec![],
            ty: Ty::arrow(sta_s.clone(), st_s(int.clone())),
        });
        // newSTArrayFromList :: [Int] -> ST s (STArray s)
        self.env.insert("newSTArrayFromList".into(), Scheme {
            vars: vec![s.clone()],
            mult_vars: vec![],
            ty: Ty::arrow(Ty::list(int.clone()), st_s(sta_s.clone())),
        });
        // stArrayToList :: STArray s -> ST s [Int]
        self.env.insert("stArrayToList".into(), Scheme {
            vars: vec![s.clone()],
            mult_vars: vec![],
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
            mult_vars: vec![],
            ty: fmap_ty.clone(),
        });
        self.env.insert("<$>".to_string(), Scheme {
            vars: vec![a.clone(), b.clone(), f.clone()],
            mult_vars: vec![],
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
                // Empty context, NOT None: a higher-kinded instance demands
                // nothing of the constructor's own type arguments, so the
                // structural fallback rule in `has_instance` (meant for
                // Show/Eq-style element checking) must not apply. Without
                // this, a wanted like `Functor (Either String)` — where the
                // class variable binds to a partially-applied constructor —
                // would wrongly require `Functor String`.
                context: Some(vec![]),
            });
        }

        // Built-in Applicative typeclass (superclass: Functor)
        // pure   :: a -> f a
        // (<*>)  :: f (a -> b) -> f a -> f b
        // liftA2 :: (a -> b -> c) -> f a -> f b -> f c
        // liftA2 is a real method (as in GHC), not sugar for <$>/<*>: the
        // <$>/<*> chain routes a FUNCTION through the applicative (an
        // `f (b -> c)` intermediate), and the type-erased IO runtime cannot
        // represent an action whose result is itself a Lua function
        // (__mll_run could not tell it from an unrun action). liftA2 keeps
        // only fully-applied values in the container, so generic Applicative
        // code (traverse) works at IO too.
        let pure_ty = Ty::arrow(ta.clone(), fa.clone());
        let fab = Ty::App(Box::new(tf.clone()), Box::new(Ty::arrow(ta.clone(), tb.clone())));
        let ap_ty = Ty::fun(&[fab, fa.clone()], fb.clone());
        let fc = Ty::App(Box::new(tf.clone()), Box::new(tc.clone()));
        let lifta2_ty = Ty::fun(
            &[Ty::fun(&[ta.clone(), tb.clone()], tc.clone()), fa.clone(), fb.clone()],
            fc,
        );
        self.classes.insert("Applicative".to_string(), ClassInfo {
            name: "Applicative".to_string(),
            type_var: "f".to_string(),
            superclasses: vec!["Functor".to_string()],
            methods: vec![
                ("pure".to_string(), pure_ty.clone()),
                ("<*>".to_string(), ap_ty.clone()),
                ("liftA2".to_string(), lifta2_ty.clone()),
            ],
            default_methods: HashMap::new(),
        });
        self.env.insert("pure".to_string(), Scheme {
            vars: vec![a.clone(), f.clone()],
            mult_vars: vec![],
            ty: pure_ty.clone(),
        });
        self.env.insert("return".to_string(), Scheme {
            vars: vec![a.clone(), f.clone()],
            mult_vars: vec![],
            ty: pure_ty,
        });
        self.env.insert("<*>".to_string(), Scheme {
            vars: vec![a.clone(), b.clone(), f.clone()],
            mult_vars: vec![],
            ty: ap_ty,
        });
        self.env.insert("liftA2".to_string(), Scheme {
            vars: vec![a.clone(), b.clone(), c.clone(), f.clone()],
            mult_vars: vec![],
            ty: lifta2_ty,
        });

        // Applicative instances
        for tc_name in &["IO", "LuaIO", "ST"] {
            let mut method_fns = HashMap::new();
            method_fns.insert("pure".to_string(), "pure".to_string());
            method_fns.insert("<*>".to_string(), "ap_IO".to_string());
            method_fns.insert("liftA2".to_string(), "liftA2_IO".to_string());
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
            method_fns.insert("liftA2".to_string(), "liftA2_List".to_string());
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
            method_fns.insert("liftA2".to_string(), "liftA2_Maybe".to_string());
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
            method_fns.insert("liftA2".to_string(), "liftA2_Either".to_string());
            self.register_instance(InstanceInfo {
                class_name: "Applicative".to_string(),
                target_type: Ty::Con("Either".to_string()),
                method_fns,
                // Empty context, not None — see the Functor Either instance.
                context: Some(vec![]),
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
            mult_vars: vec![],
            ty: Ty::fun(&[ma.clone(), Ty::arrow(ta.clone(), mb.clone())], mb.clone()),
        });
        self.env.insert(">>".to_string(), Scheme {
            vars: vec![a.clone(), b.clone(), m.clone()],
            mult_vars: vec![],
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

        // Built-in Foldable typeclass
        // foldr :: (a -> b -> b) -> b -> t a -> b
        // foldl :: (b -> a -> b) -> b -> t a -> b
        // The remaining GHC Foldable vocabulary (length, null, elem, sum,
        // product, maximum, minimum, foldMap, toList) is defined generically
        // over these two methods in the Prelude / Data.Foldable.
        let t = TyVar { name: "t".into(), id: u32::MAX };
        let tt = Ty::Var(t.clone());
        let ta_in_t = Ty::App(Box::new(tt.clone()), Box::new(ta.clone()));
        let foldr_ty = Ty::fun(
            &[Ty::fun(&[ta.clone(), tb.clone()], tb.clone()), tb.clone(), ta_in_t.clone()],
            tb.clone(),
        );
        let foldl_ty = Ty::fun(
            &[Ty::fun(&[tb.clone(), ta.clone()], tb.clone()), tb.clone(), ta_in_t.clone()],
            tb.clone(),
        );
        self.classes.insert("Foldable".to_string(), ClassInfo {
            name: "Foldable".to_string(),
            type_var: "t".to_string(),
            superclasses: vec![],
            methods: vec![
                ("foldr".to_string(), foldr_ty.clone()),
                ("foldl".to_string(), foldl_ty.clone()),
            ],
            default_methods: HashMap::new(),
        });
        self.env.insert("foldr".to_string(), Scheme {
            vars: vec![a.clone(), b.clone(), t.clone()],
            mult_vars: vec![],
            ty: foldr_ty,
        });
        self.env.insert("foldl".to_string(), Scheme {
            vars: vec![a.clone(), b.clone(), t.clone()],
            mult_vars: vec![],
            ty: foldl_ty,
        });
        // Emit wanted constraints at use sites so a fold over a type without
        // a Foldable instance — or an ambiguous one like `Right 5` with an
        // undetermined Left type — is a compile error with the annotation
        // hint, not a deferred dispatch that fails at runtime.
        for method in &["foldr", "foldl"] {
            self.method_constraints.insert(method.to_string(), vec![TyConstraint {
                class_name: "Foldable".to_string(),
                type_var: "t".to_string(),
            }]);
        }

        // The Foldable instances for [], Maybe and Either (folds over Right,
        // like GHC) are ordinary `instance Foldable …` declarations in
        // Prelude.mll — the kind system checks their heads against the class
        // variable's Type -> Type kind like any user instance. Tuples
        // deliberately have no instance: the class variable has kind
        // Type -> Type and mata-ll has no partially-applied tuple constructor
        // (consistent with tuples having no Ord instance either).

        // Built-in Traversable typeclass (superclasses: Functor, Foldable)
        // traverse :: Applicative f => (a -> f b) -> t a -> f (t b)
        // sequenceA is defined in the Prelude as `traverse (\x -> x)`.
        let tb_in_t = Ty::App(Box::new(tt.clone()), Box::new(tb.clone()));
        let traverse_ty = Ty::fun(
            &[Ty::arrow(ta.clone(), fb.clone()), ta_in_t.clone()],
            Ty::App(Box::new(tf.clone()), Box::new(tb_in_t)),
        );
        self.classes.insert("Traversable".to_string(), ClassInfo {
            name: "Traversable".to_string(),
            type_var: "t".to_string(),
            superclasses: vec!["Functor".to_string(), "Foldable".to_string()],
            methods: vec![("traverse".to_string(), traverse_ty.clone())],
            default_methods: HashMap::new(),
        });
        self.env.insert("traverse".to_string(), Scheme {
            vars: vec![a.clone(), b.clone(), f.clone(), t.clone()],
            mult_vars: vec![],
            ty: traverse_ty,
        });
        self.method_constraints.insert("traverse".to_string(), vec![
            TyConstraint { class_name: "Traversable".to_string(), type_var: "t".to_string() },
            TyConstraint { class_name: "Applicative".to_string(), type_var: "f".to_string() },
        ]);
        // Like Foldable, the Traversable instances for [], Maybe and Either
        // live in Prelude.mll as ordinary `instance Traversable …`
        // declarations.

        // Built-in Enum typeclass
        // succ :: a -> a
        // pred :: a -> a
        // toEnum :: Int -> a
        // fromEnum :: a -> Int
        // enumFrom :: a -> [a]
        // enumFromThen :: a -> a -> [a]
        // enumFromTo :: a -> a -> [a]
        // enumFromThenTo :: a -> a -> a -> [a]
        let succ_ty = Ty::arrow(ta.clone(), ta.clone());
        let to_enum_ty = Ty::arrow(Ty::Con("Int".into()), ta.clone());
        let from_enum_ty = Ty::arrow(ta.clone(), Ty::Con("Int".into()));
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
                mult_vars: vec![],
                ty: ty.clone(),
            });
        }

        // Enum instance for Int
        {
            let mut method_fns = HashMap::new();
            method_fns.insert("succ".to_string(), "succ_Int".to_string());
            method_fns.insert("pred".to_string(), "pred_Int".to_string());
            method_fns.insert("toEnum".to_string(), "toEnum_Int".to_string());
            method_fns.insert("fromEnum".to_string(), "fromEnum_Int".to_string());
            method_fns.insert("enumFrom".to_string(), "enumFrom_Int".to_string());
            method_fns.insert("enumFromThen".to_string(), "enumFromThen_Int".to_string());
            method_fns.insert("enumFromTo".to_string(), "enumFromTo_Int".to_string());
            method_fns.insert("enumFromThenTo".to_string(), "enumFromThenTo_Int".to_string());
            self.register_instance(InstanceInfo {
                class_name: "Enum".to_string(),
                target_type: Ty::Con("Int".to_string()),
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
                mult_vars: vec![],
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
            mult_vars: vec![],
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
            mult_vars: vec![],
            ty: read_ty,
        });
        // Read instances for base types
        for type_name in &["Int", "Number", "Bool", "String"] {
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
            mult_vars: vec![],
            ty: eq_ty,
        });
        // /= is derived from ==
        self.env.insert("/=".to_string(), Scheme {
            vars: vec![a.clone()],
            mult_vars: vec![],
            ty: Ty::fun(&[ta.clone(), ta.clone()], Ty::Con("Bool".into())),
        });

        // Eq instances for base types
        for type_name in &["Int", "Number", "String", "Bool", "ByteString"] {
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
                mult_vars: vec![],
                ty: cmp_ty.clone(),
            });
        }
        self.env.insert("compare".to_string(), Scheme {
            vars: vec![a.clone()],
            mult_vars: vec![],
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
        for type_name in &["Int", "Number", "String", "ByteString"] {
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

        // ===================================================================
        // Numeric class hierarchy: Num, Fractional, Real, Integral.
        // Registered built-in (like Eq/Ord) rather than as source classes,
        // because the operator methods (+ - * / div mod quot rem) must map to
        // THEMSELVES in the Int/Number instances so the monomorphizer keeps
        // them as inline InfixApp (byte-identical concrete arithmetic) — a
        // self-reference no source `instance` body can express. The named
        // methods (negate/abs/signum/fromInteger/recip/fromRational/toInteger/
        // quotRem/divMod) dispatch to small runtime helpers emitted on demand.
        // ===================================================================
        {
            let bin = Ty::fun(&[ta.clone(), ta.clone()], ta.clone());
            let un = Ty::arrow(ta.clone(), ta.clone());
            let from_integer_ty = Ty::arrow(Ty::Con("Int".into()), ta.clone());
            // No Rational type in mata-ll: a fractional literal is a Number
            // (f64) at the source level, so `fromRational` takes that same
            // representation. Documented as the single numeric-tower deviation.
            let from_rational_ty = Ty::arrow(Ty::Con("Number".into()), ta.clone());
            let pair = Ty::Tuple(vec![ta.clone(), ta.clone()]);
            let to_pair = Ty::fun(&[ta.clone(), ta.clone()], pair);

            // ---- class Num a ----
            self.classes.insert("Num".to_string(), ClassInfo {
                name: "Num".to_string(),
                type_var: "a".to_string(),
                superclasses: vec![],
                methods: vec![
                    ("+".to_string(), bin.clone()),
                    ("-".to_string(), bin.clone()),
                    ("*".to_string(), bin.clone()),
                    ("negate".to_string(), un.clone()),
                    ("abs".to_string(), un.clone()),
                    ("signum".to_string(), un.clone()),
                    ("fromInteger".to_string(), from_integer_ty.clone()),
                ],
                default_methods: HashMap::new(),
            });
            // ---- class Num a => Fractional a ----
            self.classes.insert("Fractional".to_string(), ClassInfo {
                name: "Fractional".to_string(),
                type_var: "a".to_string(),
                superclasses: vec!["Num".to_string()],
                methods: vec![
                    ("/".to_string(), bin.clone()),
                    ("recip".to_string(), un.clone()),
                    ("fromRational".to_string(), from_rational_ty.clone()),
                ],
                default_methods: HashMap::new(),
            });
            // ---- class (Num a, Ord a) => Real a ----
            // GHC's `Real` has `toRational :: a -> Rational`; mata-ll has no
            // Rational, so the class is a superclass marker with no methods.
            self.classes.insert("Real".to_string(), ClassInfo {
                name: "Real".to_string(),
                type_var: "a".to_string(),
                superclasses: vec!["Num".to_string(), "Ord".to_string()],
                methods: vec![],
                default_methods: HashMap::new(),
            });
            // ---- class (Real a, Enum a) => Integral a ----
            self.classes.insert("Integral".to_string(), ClassInfo {
                name: "Integral".to_string(),
                type_var: "a".to_string(),
                superclasses: vec!["Real".to_string(), "Enum".to_string()],
                methods: vec![
                    ("quot".to_string(), bin.clone()),
                    ("rem".to_string(), bin.clone()),
                    ("div".to_string(), bin.clone()),
                    ("mod".to_string(), bin.clone()),
                    ("quotRem".to_string(), to_pair.clone()),
                    ("divMod".to_string(), to_pair.clone()),
                    // GHC's `toInteger :: a -> Integer` is deliberately absent:
                    // mata-ll has no arbitrary-precision Integer, so a method
                    // whose whole purpose is to escape to bignum cannot be
                    // honoured. Using `toInteger` is rejected with a note
                    // pointing at `Int` (see Diagnostic::hint).
                ],
                default_methods: HashMap::new(),
            });

            // Env schemes for the NAMED methods (the operators are already in
            // env from the arithmetic block above).
            let named: &[(&str, &Ty)] = &[
                ("negate", &un), ("abs", &un), ("signum", &un),
                ("fromInteger", &from_integer_ty),
                ("recip", &un), ("fromRational", &from_rational_ty),
                ("quotRem", &to_pair), ("divMod", &to_pair),
            ];
            for (name, ty) in named {
                self.env.insert(name.to_string(), Scheme {
                    vars: vec![a.clone()], mult_vars: vec![], ty: (*ty).clone(),
                });
            }

            // Per-method class constraints (drives wanted emission on use).
            let ncm: &[(&str, &str)] = &[
                ("+", "Num"), ("-", "Num"), ("*", "Num"),
                ("negate", "Num"), ("abs", "Num"), ("signum", "Num"),
                ("fromInteger", "Num"),
                ("/", "Fractional"), ("recip", "Fractional"), ("fromRational", "Fractional"),
                ("quot", "Integral"), ("rem", "Integral"), ("div", "Integral"), ("mod", "Integral"),
                ("quotRem", "Integral"), ("divMod", "Integral"),
            ];
            for (method, class) in ncm {
                self.method_constraints.insert(method.to_string(), vec![TyConstraint {
                    class_name: class.to_string(),
                    type_var: "a".to_string(),
                }]);
            }

            // ---- instances ----
            // Operators self-map (mono keeps them inline); named methods point
            // at runtime helpers. See codegen PRELUDE for the helper bodies.
            let mk = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
                pairs.iter().map(|(m, f)| (m.to_string(), f.to_string())).collect()
            };
            // Num Int
            self.register_instance(InstanceInfo {
                class_name: "Num".to_string(),
                target_type: Ty::Con("Int".to_string()),
                method_fns: mk(&[("+", "+"), ("-", "-"), ("*", "*"),
                    ("negate", "negate_Int"), ("abs", "abs_Int"),
                    ("signum", "signum_Int"), ("fromInteger", "fromInteger_Int")]),
                context: None,
            });
            // Num Number
            self.register_instance(InstanceInfo {
                class_name: "Num".to_string(),
                target_type: Ty::Con("Number".to_string()),
                method_fns: mk(&[("+", "+"), ("-", "-"), ("*", "*"),
                    ("negate", "negate_Number"), ("abs", "abs_Number"),
                    ("signum", "signum_Number"), ("fromInteger", "fromInteger_Number")]),
                context: None,
            });
            // Fractional Number (Integer is deliberately NOT Fractional, as GHC)
            self.register_instance(InstanceInfo {
                class_name: "Fractional".to_string(),
                target_type: Ty::Con("Number".to_string()),
                method_fns: mk(&[("/", "/"), ("recip", "recip_Number"),
                    ("fromRational", "fromRational_Number")]),
                context: None,
            });
            // Real Int / Real Number (no methods; just evidence Ord+Num).
            self.register_instance(InstanceInfo {
                class_name: "Real".to_string(),
                target_type: Ty::Con("Int".to_string()),
                method_fns: HashMap::new(),
                context: None,
            });
            self.register_instance(InstanceInfo {
                class_name: "Real".to_string(),
                target_type: Ty::Con("Number".to_string()),
                method_fns: HashMap::new(),
                context: None,
            });
            // Integral Integer (Number is NOT Integral, as GHC)
            self.register_instance(InstanceInfo {
                class_name: "Integral".to_string(),
                target_type: Ty::Con("Int".to_string()),
                method_fns: mk(&[("div", "div"), ("mod", "mod"), ("quot", "quot"), ("rem", "rem"),
                    ("quotRem", "quotRem_Int"), ("divMod", "divMod_Int")]),
                context: None,
            });
        }

        // The Semigroup and Monoid CLASS declarations are now ordinary source
        // classes in lib/Prelude.mll (`class Semigroup a where (<>) :: …` and
        // `class Semigroup a => Monoid a where { mempty; mappend }`). Their
        // method env entries and their per-method class constraints —
        // including the `mempty` ambiguity check — are synthesized by
        // `register_class` exactly as for any user class, so nothing about
        // them needs to be hard-registered here anymore. Only the runtime
        // string-concatenation primitive their String instances call stays a
        // builtin (below), because Lua `..` has no source-level spelling.

        // `semigroup_String` is the runtime string-concatenation primitive
        // (Lua `..`, defined in codegen's preamble and inlined at call sites).
        // mata-ll String is opaque — unlike GHC's `[Char]` it has no `++` — so
        // this is the ONLY way to concatenate two Strings, and the Prelude's
        // `instance Semigroup String` / `instance Monoid String` bodies call
        // it by name. Registering it in the environment makes those source
        // instance bodies type-check; codegen already knows the name. (The
        // list instances use the ordinary `++` operator instead, so no such
        // primitive is exposed for lists.)
        self.env.insert("semigroup_String".to_string(), Scheme {
            vars: vec![],
            mult_vars: vec![],
            ty: Ty::fun(&[Ty::Con("String".into()), Ty::Con("String".into())], Ty::Con("String".into())),
        });

        // The Semigroup/Monoid classes and their String/[a] instances all
        // live in lib/Prelude.mll now. The deliberate mata-ll divergence —
        // `<>` on a concrete list type is rejected in favour of `++` — lives
        // in the monomorphizer's dispatch (`resolve_at_type`), keyed on the
        // class name from the (now source) class registration, so it is
        // unaffected by the move. `mappend` still dispatches on lists (its
        // instance body is `xs ++ ys`), and an undetermined `mempty` is an
        // ambiguity error via the constraint `register_class` synthesizes for
        // it, exactly as for the builtin `mempty` before.

        // Show instances for base types and parameterized types
        for type_name in &["Int", "Number", "String", "Bool", "[]", "Maybe", "ByteString"] {
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
        for name in &["Int", "Number", "String", "Bool", "()", "ByteString", "LuaUserData"] {
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

        // Builtin CLASS-variable kinds (see `class_kinds`). The container
        // classes apply their variable to an element type in every method
        // (`fmap :: (a -> b) -> f a -> f b`), so their variable is
        // Type -> Type; every other builtin class (Show, Eq, Ord, Enum,
        // Bounded, Read, Semigroup, Monoid) constrains complete types and
        // defaults to Type via `class_kind_of`, so only the higher-kinded
        // ones need an entry. User-declared classes get their kind inferred
        // from their method signatures and superclasses by `infer_class_kinds`.
        for name in &["Functor", "Applicative", "Monad", "Foldable", "Traversable"] {
            self.class_kinds.insert(name.to_string(), type_to_type.clone());
        }
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

        // `Int` is the canonical builtin fixed-width integer (registered with
        // kind Type above). There is deliberately no `Int` type and no
        // `Int`/`Int` alias: mata-ll's integer is 64-bit and wrapping, so
        // it carries GHC's `Int` name, and `Integer` (arbitrary precision) is
        // rejected with a note (see Diagnostic::hint) rather than silently
        // aliased — the alias would be the soundness back door.

        // The builtin `Bool` promotes (DataKinds) like any parameterless data
        // type: `'True`/`'False` have kind `Bool`. `Bool` is already in
        // `promotable_kinds` (seeded in `new`); register the promoted
        // constructor kinds so a `Bool`-kinded index is recognized (and a
        // `Bool` tag used where a `Nat` is expected is a clear kind error).
        self.kinds.insert("'True".to_string(), Kind::Promoted("Bool".to_string()));
        self.kinds.insert("'False".to_string(), Kind::Promoted("Bool".to_string()));
    }

    /// The kind a data type's promoted constructor `con` receives: its result
    /// is the promoted kind `Promoted(data_type)`, preceded by one arrow per
    /// field, each field type promoted to a kind via `promote_field_kind`. So
    /// `Z` (of `Nat`) gives `Nat`, and `S Nat` gives `Nat -> Nat`. Only called
    /// for promotable (parameterless, non-GADT) data types.
    fn promoted_constructor_kind(&self, con: &Constructor, data_type: &str) -> Kind {
        let field_types: Vec<&Type> = match &con.fields {
            ConstructorFields::Positional(tys) => tys.iter().collect(),
            ConstructorFields::Named(fields) => fields.iter().map(|f| &f.ty).collect(),
        };
        let mut kind = Kind::Promoted(data_type.to_string());
        for ft in field_types.iter().rev() {
            kind = Kind::arrow(self.promote_field_kind(ft), kind);
        }
        kind
    }

    /// Promote a constructor field TYPE to the KIND it contributes to the
    /// promoted constructor. A field whose type is itself a promotable data
    /// type (`Nat` in `S Nat`) promotes to that type's kind (`Nat`); anything
    /// else (a builtin like `Int`, a compound type) is approximated as
    /// `Type` — such a field is not a usable type-level index anyway, and the
    /// constructor's RESULT kind (what indexing checks against) is still exact.
    fn promote_field_kind(&self, ty: &Type) -> Kind {
        match ty {
            Type::Con(name) if self.promotable_kinds.contains(name) =>
                Kind::Promoted(name.clone()),
            Type::Paren(inner) => self.promote_field_kind(inner),
            _ => Kind::Type,
        }
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
        } else if let Some(ci) = self.constructors.get(name) {
            // An UN-TICKED promoted pun: no TYPE with this name exists, but a
            // data CONSTRUCTOR does (`Vec (S Z)` written for `Vec ('S 'Z)`).
            // Point at the tick instead of a bare unknown-type error.
            let type_name = ci.type_name.clone();
            let baseline = self.checking_prelude;
            self.errors.push(Diagnostic {
                kind: DiagnosticKind::Other(format!(
                    "Unknown type '{}' — but a data constructor of that name exists (declared by 'data {}'). To use a constructor at the type level it must be PROMOTED, written with a leading tick: '{}",
                    name, type_name, name
                )),
                context: Some(ctx.to_string()),
                span: None,
                file: None,
                notes: vec![format!(
                    "GHC (with DataKinds) accepts the un-ticked pun when the name is unambiguous; mata-ll always requires the tick."
                )],
                baseline,
            });
        } else {
            self.push_error_ctx(DiagnosticKind::UnknownType(name.to_string()), ctx.to_string());
        }
    }

    // (Kind inference and the kind-checking walks live in typechecker/kind.rs:
    // `check_type_kind` and its siblings replace the old per-declaration
    // arity-based checks that lived here.)

    /// Fallback kind registration for a data type: `Type -> … -> Type` from
    /// its parameter count. `infer_declared_kinds` (pass 1a) has already
    /// registered the real, inferred kind for every declaration in the
    /// module — which may be higher-kinded (`data Wrap f = Wrap (f Int)`
    /// gives `(Type -> Type) -> Type`) — so this only fills the gap if a
    /// registration path was somehow not covered by that pass, and never
    /// overwrites an inferred kind.
    fn register_kind(&mut self, name: &str, num_params: usize) {
        if self.kinds.contains_key(name) {
            return;
        }
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

    /// Decompose a GADT constructor signature into (context constraints,
    /// field types, result type, existential vars): peels explicit `forall`s
    /// and an outer context (`MkBox :: forall a. Show a => a -> Box`), splits
    /// the arrow spine, and computes which signature variables are
    /// EXISTENTIAL — the ones that occur in the fields but not in the result
    /// type. GADT syntax declares existentials implicitly this way; the data
    /// header's parameter names are only arity markers, so a field variable
    /// that happens to share a header name but does not reach the result is
    /// existential all the same.
    fn analyze_gadt_signature(&mut self, gadt_ty: &Type) -> (Vec<Constraint>, Vec<Ty>, Ty, Vec<TyVar>) {
        let mut core = gadt_ty;
        let mut constraints: Vec<Constraint> = Vec::new();
        loop {
            match core {
                Type::Forall { inner, .. } => core = inner,
                Type::Paren(inner) => core = inner,
                Type::Constrained { constraints: cs, ty } => {
                    constraints.extend(cs.iter().cloned());
                    core = ty;
                }
                _ => break,
            }
        }
        let full_ty = self.ast_type_to_ty(core);
        let mut args = Vec::new();
        let mut cur = full_ty;
        while let Ty::Arrow(a, b, _) = cur {
            args.push(*a);
            cur = *b;
        }
        let result_vars = cur.free_vars();
        let mut ex_vars: Vec<TyVar> = Vec::new();
        for ft in &args {
            for v in ft.free_vars() {
                if !result_vars.contains(&v) && !ex_vars.contains(&v) {
                    ex_vars.push(v);
                }
            }
        }
        (constraints, args, cur, ex_vars)
    }

    fn register_data_type(&mut self, name: &str, type_vars: &[String], constructors: &[Constructor]) {
        self.register_kind(name, type_vars.len());
        let tvars: Vec<TyVar> = type_vars.iter()
            .map(|n| TyVar { name: n.clone(), id: u32::MAX })
            .collect();
        let result_type = tvars.iter().fold(Ty::Con(name.to_string()), |acc, tv| Ty::app(acc, Ty::Var(tv.clone())));

        // DataKinds: register the kinds of this type's promoted constructors.
        // A promotable data type (parameterless, non-GADT, non-existential —
        // see `promotable_kinds`) promotes to a REAL kind named after it, so
        // its constructors get promoted kinds ending in that kind (`'Z :: Nat`,
        // `'S :: Nat -> Nat`). Every other data type keeps the historical
        // approximation: each promoted constructor with N fields gets
        // `Type -> … -> Type` (promoting it precisely would need kind
        // polymorphism, which mata-ll does not have).
        let promotable = self.promotable_kinds.contains(name);
        for con in constructors {
            let kind = if promotable {
                self.promoted_constructor_kind(con, name)
            } else {
                let field_count = match &con.fields {
                    crate::ast::ConstructorFields::Positional(fs) => fs.len(),
                    crate::ast::ConstructorFields::Named(fs) => fs.len(),
                };
                let mut kind = Kind::Type;
                for _ in 0..field_count {
                    kind = Kind::Arrow(Box::new(Kind::Type), Box::new(kind));
                }
                kind
            };
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
            let mut ex_tvars: Vec<TyVar> = con.existential_vars.iter()
                .map(|n| TyVar { name: n.clone(), id: u32::MAX })
                .collect();

            // The constraints declared on those variables (`forall a. Show a
            // => …`). Only the Haskell-2010 form `C a` over a variable bound
            // by THIS constructor's forall can mean anything here; pass 2b
            // rejects everything else, so a malformed constraint is dropped
            // rather than half-registered.
            let mut ex_constraints: Vec<TyConstraint> = con.existential_constraints.iter()
                .filter_map(|c| match &c.type_arg {
                    Type::Var(v) if con.existential_vars.contains(v) =>
                        Some(TyConstraint { class_name: c.class_name.clone(), type_var: v.clone() }),
                    _ => None,
                })
                .collect();

            let (field_types, con_result_type) = if let Some(gadt_ty) = &con.gadt_type {
                // GADT constructor: decompose type sig into args + return
                // type, collecting the implicitly-declared existentials
                // (field variables that do not reach the result type) and
                // any context constraints on them. Malformed constraints
                // (unknown class, non-existential variable) are reported in
                // pass 2b, like the `forall`-syntax ones.
                let (gadt_constraints, args, result, gadt_ex) =
                    self.analyze_gadt_signature(gadt_ty);
                for v in gadt_ex {
                    if !ex_tvars.contains(&v) {
                        ex_tvars.push(v);
                    }
                }
                for c in &gadt_constraints {
                    if let Type::Var(v) = &c.type_arg
                        && ex_tvars.iter().any(|t| &t.name == v) {
                            ex_constraints.push(TyConstraint {
                                class_name: c.class_name.clone(),
                                type_var: v.clone(),
                            });
                        }
                }
                (args, result)
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
                existential_constraints: ex_constraints,
            });
            self.env.insert(con_key, Scheme { vars: all_scheme_vars, mult_vars: vec![], ty: con_type });

            // Register record field accessors
            if let ConstructorFields::Named(fields) = &con.fields {
                for (fi, field) in fields.iter().enumerate() {
                    let field_ty = self.ast_type_to_ty(&field.ty);
                    // A field whose type mentions an existential variable
                    // gets NO selector: `getIt :: Foo -> a` would hand the
                    // hidden type to any caller, outside any match scope —
                    // the selector is the escape hole in function form.
                    // Record it so uses of the name (and record updates of
                    // the field) can explain this instead of claiming the
                    // name is unbound. Construction and pattern matching
                    // still work (both go through the constructor itself).
                    if field_ty.free_vars().iter()
                        .any(|v| con.existential_vars.contains(&v.name)) {
                        self.existential_fields.insert(field.name.clone(), con.name.clone());
                        let index = if constructors.len() == 1 { fi + 1 } else { fi + 2 };
                        self.record_fields.insert(field.name.clone(), (name.to_string(), index));
                        continue;
                    }
                    // accessor :: DataType -> FieldType
                    // Note: the accessor is always named after the Haskell field
                    // name; an `as "key"` rename affects only the LuaDict table key.
                    let accessor_ty = Ty::arrow(result_type.clone(), field_ty);
                    self.env.insert(field.name.clone(), Scheme {
                        vars: tvars.clone(),
                        mult_vars: vec![],
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
    /// `newtype Age = Int` creates constructor `Age :: Int -> Age`
    /// that is the identity function at runtime.
    fn register_newtype(&mut self, name: &str, type_vars: &[String], inner: &Type) {
        self.register_kind(name, type_vars.len());
        // Record that this name is a newtype (a transparent, zero-cost wrapper)
        // so the FFI boundary check can distinguish it from a same-shaped `data`
        // type: a newtype crosses AS its single field, a `data` type would only
        // cross as an internal tagged table.
        self.newtype_types.insert(name.to_string());
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
            existential_constraints: vec![],
        });
        self.env.insert(con_key, Scheme {
            vars: tvars,
            mult_vars: vec![],
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

    /// True when `name` is bound in the initial global environment — a
    /// compiler builtin (`error`, `seq`, `map`, `hmInsert`, …) or a builtin
    /// class method (`show`, `fmap`, `>>=`, …). Only meaningful on a fresh
    /// checker, before `check_module` has inserted source-level bindings.
    pub fn is_builtin(&self, name: &str) -> bool {
        self.env.lookup(name).is_some()
    }

    /// Declare how many leading declarations of the module about to be checked
    /// belong to the implicit Prelude (see `prelude_decl_count`). Errors
    /// raised inside that region are tagged [`Diagnostic::baseline`].
    pub fn set_prelude_decl_count(&mut self, count: usize) {
        self.prelude_decl_count = count;
    }

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

        // Register type families and aliases and lower the families to `Ty`
        // form BEFORE anything converts a type: the eager (concrete) family
        // reduction in `ast_type_to_ty` now goes through the shared iterative
        // normalizer, which needs `ty_families` populated. (Both are also
        // re-registered in passes 1/2 — idempotent — where they logically
        // belong; this early pass only makes reduction available from the very
        // first `ast_type_to_ty`, e.g. a data field of family type in pass 1.)
        for decl in &module.decls {
            match decl {
                Decl::TypeFamily { name, equations } => {
                    self.type_families.insert(name.clone(), equations.clone());
                }
                Decl::TypeAlias { name, params, ty } => {
                    self.type_aliases.insert(name.clone(), (params.clone(), ty.clone()));
                }
                _ => {}
            }
        }
        self.build_ty_families();

        // Determine which data types promote to a REAL kind (DataKinds): the
        // parameterless, non-GADT, non-existential ones — so the promoted kind
        // is monomorphic (`Nat`, `Color`, …). Do this BEFORE `infer_declared_kinds`
        // and pass 1, and register the promoted constructor kinds now, so the
        // kind-inference prepass can infer an index variable's kind from a
        // promoted constructor in a GADT return type (`n : Nat` from `Vec 'Z a`).
        for decl in &module.decls {
            if let Decl::DataDef { name, type_vars, constructors, .. } = decl {
                let promotable = type_vars.is_empty()
                    && constructors.iter().all(|c|
                        c.gadt_type.is_none() && c.existential_vars.is_empty());
                if promotable {
                    self.promotable_kinds.insert(name.clone());
                }
            }
        }
        for decl in &module.decls {
            if let Decl::DataDef { name, constructors, .. } = decl {
                if self.promotable_kinds.contains(name) {
                    for con in constructors {
                        let kind = self.promoted_constructor_kind(con, name);
                        self.kinds.insert(format!("'{}", con.name), kind);
                    }
                }
            }
        }

        // Pass 1a: infer the kind of everything the module declares at the
        // type level (data, newtype, alias, type family), solving all their
        // constraints together so mutual recursion and cross-references work.
        // Must run before pass 1: registration converts field types with
        // these kinds in place, and every later kind check reads this table.
        // Silent — ill-kinded declarations are reported by pass 2b.
        self.infer_declared_kinds(&module.decls);

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
            self.checking_prelude = decl_idx < self.prelude_decl_count;
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
        self.checking_prelude = false;

        // Pass 1b: infer the type-variable kind of every class the module
        // declares, order-independently (a superclass declared later still
        // constrains its subclass — see `infer_class_kinds`). Runs after
        // pass 1 so method signatures can look up data-type kinds, and
        // before pass 2 so `register_class` and every instance-head kind
        // check read the finalized `class_kinds` table. Silent — an
        // inconsistent class is reported by pass 2b.
        self.infer_class_kinds(&module.decls);

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
                Decl::DataDef { name, type_vars, constructors, .. } => {
                    let ctx = format!("the definition of data type '{}'", name);
                    for con in constructors {
                        // Validate the constraints on this constructor's
                        // existential variables (`forall a. Show a => …`).
                        // register_data_type (pass 1, before classes existed)
                        // silently dropped anything malformed; report it here
                        // so a typo'd class or a constraint on the wrong
                        // variable cannot silently become "no constraint".
                        for c in &con.existential_constraints {
                            match &c.type_arg {
                                Type::Var(v) if con.existential_vars.contains(v) => {
                                    if !self.classes.contains_key(&c.class_name) {
                                        self.push_error_ctx(
                                            DiagnosticKind::Other(format!(
                                                "Unknown typeclass '{}' in the context of constructor '{}': the constraint names the class the packed value must have an instance of, so it must be a class that is in scope",
                                                c.class_name, con.name)),
                                            ctx.clone(),
                                        );
                                    }
                                }
                                Type::Var(v) => {
                                    self.push_error_ctx(
                                        DiagnosticKind::Other(format!(
                                            "Constraint '{} {}' on constructor '{}' does not mention any of its existentially quantified variables: only the variables bound by this constructor's 'forall' can carry a constraint here",
                                            c.class_name, v, con.name)),
                                        ctx.clone(),
                                    );
                                }
                                other => {
                                    let shown = self.ast_type_to_ty(other);
                                    self.push_error_ctx(
                                        DiagnosticKind::Other(format!(
                                            "Constraint '{} {}' on constructor '{}' must apply the class to a plain type variable bound by the 'forall' (the Haskell 2010 form 'C a')",
                                            c.class_name, shown, con.name)),
                                        ctx.clone(),
                                    );
                                }
                            }
                        }
                        if let Some(gadt_ty) = &con.gadt_type {
                            // A GADT signature's context is subject to the
                            // same rules as a forall-constructor's: each
                            // constraint must name a known class and apply
                            // it to one of the constructor's EXISTENTIAL
                            // variables (a variable that reaches the result
                            // type is the caller's, and its constraints
                            // belong on the functions that use the type).
                            let (gadt_constraints, _, _, ex_vars) =
                                self.analyze_gadt_signature(gadt_ty);
                            for c in &gadt_constraints {
                                match &c.type_arg {
                                    Type::Var(v) if ex_vars.iter().any(|t| &t.name == v) => {
                                        if !self.classes.contains_key(&c.class_name) {
                                            self.push_error_ctx(
                                                DiagnosticKind::Other(format!(
                                                    "Unknown typeclass '{}' in the context of constructor '{}': the constraint names the class the packed value must have an instance of, so it must be a class that is in scope",
                                                    c.class_name, con.name)),
                                                ctx.clone(),
                                            );
                                        }
                                    }
                                    Type::Var(v) => {
                                        self.push_error_ctx(
                                            DiagnosticKind::Other(format!(
                                                "Constraint '{} {}' on constructor '{}' does not mention any of its existential variables ('{}' reaches the constructor's result type, so it is chosen by the caller, not hidden by the constructor)",
                                                c.class_name, v, con.name, v)),
                                            ctx.clone(),
                                        );
                                    }
                                    other => {
                                        let shown = self.ast_type_to_ty(other);
                                        self.push_error_ctx(
                                            DiagnosticKind::Other(format!(
                                                "Constraint '{} {}' on constructor '{}' must apply the class to a plain type variable (the Haskell 2010 form 'C a')",
                                                c.class_name, shown, con.name)),
                                            ctx.clone(),
                                        );
                                    }
                                }
                            }
                            // A GADT signature scopes its own type variables
                            // (the header parameters are arity markers), so
                            // it is checked as a standalone complete type.
                            self.check_type_kind(gadt_ty, &ctx);
                            continue;
                        }
                        // All of one constructor's fields share a scope: the
                        // data parameters come in at their inferred kinds
                        // (higher-kinded parameters included), and the
                        // constructor's existential variables must be used
                        // at one consistent kind across its fields.
                        let field_types: Vec<&Type> = match &con.fields {
                            ConstructorFields::Positional(tys) => tys.iter().collect(),
                            ConstructorFields::Named(fields) =>
                                fields.iter().map(|f| &f.ty).collect(),
                        };
                        let params = self.param_kind_seed(name, type_vars);
                        self.check_constructor_kinds(&field_types, params, &ctx);
                    }
                }
                Decl::NewtypeDef { name, type_vars, inner } => {
                    let params = self.param_kind_seed(name, type_vars);
                    self.check_constructor_kinds(
                        &[inner],
                        params,
                        &format!("the definition of newtype '{}'", name),
                    );
                }
                Decl::TypeAlias { name, params, ty } => {
                    self.check_alias_kinds(
                        name,
                        params,
                        ty,
                        &format!("the definition of type alias '{}'", name),
                    );
                }
                Decl::ClassDecl { name, type_var, methods, .. } => {
                    // Each method is checked with the class variable at the
                    // class's inferred kind, so a method that disagrees with
                    // its siblings is the one that gets the error.
                    for method in methods {
                        self.check_class_method_kind(
                            name,
                            type_var,
                            &method.ty,
                            &format!("the signature of method '{}' in class '{}'", method.name, name),
                        );
                    }
                }
                Decl::InstanceDecl { class_name, target_type, context, .. } => {
                    // The head must have the class variable's kind, and the
                    // context must use the head's variables consistently.
                    self.check_instance_kind(class_name, target_type, context);
                }
                Decl::TypeFamily { name, equations } => {
                    self.check_family_kinds(
                        name,
                        equations,
                        &format!("the definition of type family '{}'", name),
                    );
                }
                _ => {}
            }
        }

        // Pass 3: collect type signatures and FFI info
        let mut sigs: HashMap<String, Ty> = HashMap::new();
        let mut ffi_info: HashMap<String, (String, FfiKind)> = HashMap::new();
        for decl in &module.decls {
            // An export signature is a type the user wrote too; it must be a
            // well-kinded complete type just like an ordinary signature.
            if let Decl::ExportSig { name, ty } = decl {
                self.check_type_kind(ty, &format!("the export signature for '{}'", name));
            }
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
            self.checking_prelude = decl_idx < self.prelude_decl_count;
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
        self.checking_prelude = false;

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
            self.checking_prelude = decl_idx < self.prelude_decl_count;
            if let Decl::InstanceDecl { class_name, target_type, context, methods } = decl {
                let ifns = self.check_instance(class_name, target_type, context, methods);
                instance_fns.extend(ifns);
            }
        }
        self.checking_local = false;
        self.checking_prelude = false;

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
                    // Reject any import argument/result whose type has no defined
                    // FFI marshalling (the import mirror of validate_export_types:
                    // a plain `data` ADT would leak as an internal tagged table).
                    self.validate_ffi_import_types(name, *ffi_kind, ty);
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
        // not FFI bindings. Without this, `foo :: Int` with no body silently
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
        // Exports already rejected for carrying a class constraint — the
        // structural boundary check is skipped for them (their type variable
        // would otherwise be reported a second time).
        let mut constrained_exports: Vec<String> = Vec::new();
        for (decl_idx, decl) in module.decls.iter().enumerate() {
            // Enable hidden name enforcement only for local (user) declarations
            self.enforce_hidden = !self.hidden_names.is_empty()
                && self.local_decl_start > 0
                && decl_idx >= self.local_decl_start;
            // Constructor references in local bodies resolve to local
            // (possibly shadowing) constructors; non-local bodies keep seeing
            // the constructors of their own scope.
            self.checking_local = decl_idx >= self.local_decl_start;
            self.checking_prelude = decl_idx < self.prelude_decl_count;
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
                    // A class constraint on the export (`export f :: Num a => …`)
                    // would require passing a dictionary across the boundary,
                    // which has no Lua representation. Reject it here where the
                    // declared context is visible (the resolved type has the
                    // context stripped). Peel a leading forall/parens first.
                    let mut ctx_ty = ty;
                    while let Type::Forall { inner, .. } | Type::Paren(inner) = ctx_ty {
                        ctx_ty = inner;
                    }
                    if let Type::Constrained { constraints, .. } = ctx_ty
                        && !constraints.is_empty()
                    {
                        constrained_exports.push(name.clone());
                        self.push_error_ctx(
                            DiagnosticKind::Other(format!(
                                "Export '{}' has a class constraint in its type, but a \
                                 typeclass dictionary cannot cross the FFI boundary.",
                                name
                            )),
                            format!("export declaration of '{}'", name),
                        );
                        if let Some(diag) = self.errors.last_mut() {
                            diag.notes.push(
                                "give the export a concrete, unconstrained type — Lua \
                                 has no representation for a class dictionary.".to_string());
                        }
                    }
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
        self.checking_prelude = false;

        // Reject exports whose signature uses a type that cannot cross the FFI
        // boundary (a polymorphic value, a constrained/dictionary type, a
        // region-scoped ST handle, an inbound IO action, …). Runs on the FINAL
        // resolved function types — exactly the `export_types` codegen marshals
        // from — so the error is raised before codegen ever emits a broken
        // (undefined-at-the-boundary) conversion.
        self.validate_export_types(&exports, &functions, &constrained_exports);

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

        TModule { data_defs, dropped_data_defs: vec![], functions, instance_fns, has_main, exports, record_accessors, newtypes }
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
        Type::Arrow(_, b, _) => extract_ffi_info(b),
        Type::LuaPure { lua_name, .. } => Some((lua_name.clone(), FfiKind::Pure)),
        Type::LuaIO { lua_name, .. } => Some((lua_name.clone(), FfiKind::IO)),
        Type::LuaIterator { lua_name, .. } => Some((lua_name.clone(), FfiKind::Iterator)),
        Type::LuaTry { lua_name, .. } => Some((lua_name.clone(), FfiKind::Try)),
        Type::LuaCatch { lua_name, .. } => Some((lua_name.clone(), FfiKind::Catch)),
        Type::LuaIOCatch { lua_name, .. } => Some((lua_name.clone(), FfiKind::IOCatch)),
        Type::Paren(inner) => extract_ffi_info(inner),
        // Peel a class-constraint context (`LuaDict b => … -> LuaIO …`) and a
        // rank-N `forall`, exactly as `ast_type_to_ty` does: the FFI form that
        // makes this a body-less import sits at the tail of the arrow chain,
        // under any qualifier. Without this a constrained FFI import (its
        // constraint bounds a marshalled argument, e.g. `LuaDict b => … [b] …`)
        // is not recognised and is rejected as a signature with no definition.
        Type::Constrained { ty, .. } => extract_ffi_info(ty),
        Type::Forall { inner, .. } => extract_ffi_info(inner),
        _ => None,
    }
}

/// Is `ty` a saturated `Maybe a`? Used to spot optional FFI parameters in a
/// declared FFI signature (SPEC "Optional parameters").
fn is_maybe_ty(ty: &Ty) -> bool {
    matches!(ty, Ty::App(f, _) if matches!(f.as_ref(), Ty::Con(c) if c == "Maybe"))
}

/// Compute the OutgoingCallback shape for a callback parameter type.
/// Returns (arity, run_io). WHAT to convert at each boundary position is not
/// decided here: it is derived at codegen time from the callback's
/// monomorphized type, so a polymorphic accumulator instantiated at a
/// structured type is converted identically at the FFI edge and the callback
/// edge (see TExprKind::OutgoingCallback). Arity and IO-ness never change
/// under instantiation, so they may safely be read off the declared type.
fn outgoing_cb_flags(cb_ty: &Ty) -> (usize, bool) {
    let (args, ret) = cb_ty.peel_arrows();
    let run_io = matches!(ret, Ty::IO(_) | Ty::LuaIO(_, _));
    (args.len(), run_io)
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

/// Node count of an AST `Type`, but stops counting at `cap` (returns `cap`
/// then) so a runaway (exponentially expanding) type is never walked in full.
/// The AST analogue of `types::ty_size_up_to`; used to charge type-alias
/// expansion fuel by the size of each expanded body (see
/// `Checker::charge_alias_expansion`). A cost of at least 1 per expansion is
/// guaranteed (`Con`/`Var`/`Unit` count 1), so exponentially many expansions
/// exhaust the budget even when each individual body is small.
fn ast_type_size_up_to(ty: &Type, cap: u32) -> u32 {
    fn go(ty: &Type, cap: u32, acc: &mut u32) {
        if *acc >= cap { return; }
        *acc += 1;
        match ty {
            Type::App(a, b) | Type::Arrow(a, b, _) => { go(a, cap, acc); go(b, cap, acc); }
            Type::List(a) | Type::IO(a) => go(a, cap, acc),
            Type::Paren(inner) => go(inner, cap, acc),
            Type::ScopedLuaIO { inner, .. } => go(inner, cap, acc),
            Type::Forall { inner, .. } => go(inner, cap, acc),
            Type::LuaPure { result, .. }
            | Type::LuaIO { result, .. }
            | Type::LuaIterator { result, .. }
            | Type::LuaTry { result, .. }
            | Type::LuaCatch { result, .. }
            | Type::LuaIOCatch { result, .. } => go(result, cap, acc),
            Type::Constrained { constraints, ty } => {
                for c in constraints { go(&c.type_arg, cap, acc); if *acc >= cap { return; } }
                go(ty, cap, acc);
            }
            Type::Tuple(elems) => {
                for e in elems { go(e, cap, acc); if *acc >= cap { break; } }
            }
            Type::Con(_) | Type::Var(_) | Type::Unit | Type::Promoted(_) => {}
        }
    }
    let mut acc = 0;
    go(ty, cap, &mut acc);
    acc
}
