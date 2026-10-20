//! Monomorphization pass
//!
//! Walks the typed IR and collects all concrete type instantiations
//! of polymorphic functions. For each unique instantiation, generates
//! a specialized copy with a mangled name and rewrites call sites.

use std::collections::{HashMap, HashSet};
use crate::ast::Span;
use crate::tir::*;
use crate::typechecker::{Checker, ClassInfo, InstanceInfo};
use crate::types::{Diagnostic, DiagnosticKind, InstHead, Ty, TyVar, TyConstraint, Subst, TyFamilies};

/// Per-function cap on generated specializations: a function whose
/// specializations already outnumber it trips the cap at its next distinct
/// instantiation (see `Monomorphizer::trip_spec_cap`) — 17 copies are kept
/// and the 18th instantiation trips. Guards polymorphic recursion —
/// each specialization demanding another at a bigger type — by switching a
/// constrained function to dictionary passing and a constraint-free one to
/// its shared generic copy.
const SPEC_LIMIT: usize = 16;

/// The message and optional note of a "No instance" error. The note carries a
/// mata-ll-specific hint when the (method, type) pair is a known
/// GHC-vs-mata-ll divergence.
fn no_instance_parts(method: &str, ty: &Ty) -> (String, Option<&'static str>) {
    let msg = format!("No instance for '{}' on type '{}'", method, ty);
    let hint = match method {
        ">" | "<" | ">=" | "<=" | "compare" | "max" | "min" if matches!(ty, Ty::Tuple(_)) =>
            Some("tuples have no Ord instance in mata-ll; compare their components individually"),
        _ => None,
    };
    (msg, hint)
}

/// A specialization demand: function name + the concrete type it is
/// instantiated at. Keyed on the structured type — never its Display string —
/// so two distinct types can never collide into one specialization.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SpecKey {
    name: String,
    ty: Ty,
}

/// What a class-method use lowers to — the ONE decision for every form a
/// method takes (a named use, an infix operator on either operand, a
/// first-class operator or section, prefix negation, an overloaded
/// literal), made by `dispatch_method` for the main walk and the
/// dictionary rewrite alike. The invariant it keeps: a use whose
/// class-variable binding contains a type variable or an existential
/// skolem is dispatched through EVIDENCE (a dictionary parameter, a
/// captured existential dictionary, or a dictionary composed from them)
/// whenever evidence is in scope; a native or specialized lowering is
/// chosen only for a binding that needs none (`needs_evidence`). Before
/// this, each syntactic
/// form had its own copy of the decision and each copy missed cases —
/// structural operators over a polymorphic element resolved to a
/// type-erased shape implementation, a first-class `(==)` and a prefix
/// `-x` kept the native Lua operator, a literal at an existential type
/// stayed a machine number, an operator whose class variable sits in the
/// right operand was never rewritten (T5, X1, X2, L14).
#[derive(Debug)]
enum MethodUse {
    /// The implementation IS the operator (the builtin `+` at Int/Number,
    /// IO's `>>=`): the node stays as written and codegen emits it
    /// natively — byte-identical to the pre-overloading compiler.
    Native,
    /// A named implementation function: a specialized instance method, a
    /// generated structural implementation, or (for a binding that will
    /// never see evidence) the type-erased generic copy.
    Impl(String),
    /// A dictionary read — a parameter access, a captured existential
    /// dictionary, or a method of a composed dictionary — applied to the
    /// method's own extra dictionaries when the method carries
    /// method-level constraints (`traverse`'s `Applicative f`).
    Evidence(TExprKind),
    /// No decision here: the binding is polymorphic and the evidence
    /// arrives with the dictionary rewrite of this body (or, in a generic
    /// copy, never — the node keeps codegen's type-erased lowering).
    Deferred,
    /// The class variable is bound to this concrete type, and no instance
    /// exists for it.
    Missing(Ty),
}

/// The dictionary evidence a dictionary-form body carries: its own
/// dictionary parameters, as the precise `(class, variable) -> parameter`
/// map a method use at exactly that variable resolves through (`env`) and
/// the per-class fallback `class -> parameter` (the FIRST constraint of the
/// class, for a use whose variable name the body no longer matches). Both
/// views are minted by `dict_param_tables`. The main walk carries the EMPTY
/// environment: there, evidence comes only from the captured dictionaries
/// of an enclosing existential match (`Monomorphizer::skolem_dicts`), and
/// `dict_evidence_for` answers "no" for every type variable.
#[derive(Debug, Clone, Default)]
struct DictEnv {
    class_to_dict: HashMap<String, String>,
    env: HashMap<(String, String), String>,
}

/// A constrained existential constructor (`data Box = forall a. Show a =>
/// MkBox a`), as the monomorphizer sees it. GHC's representation is the
/// model: the constructor CAPTURES the class dictionaries for its hidden
/// type at pack time — one hidden trailing field per (class, variable) in
/// `captures` — and a match binds them back, so a method used at the hidden
/// type (a skolem inside the match) dispatches through the captured
/// dictionary. Without the capture the solver's "satisfied" for `Show a`
/// had no runtime evidence, and `show x` on the unpacked value fell to the
/// type-erased runtime `show` (a user ADT printed as a tuple).
#[derive(Debug, Clone)]
struct ExCtor {
    /// The declared field types, over the existential variables by NAME.
    field_types: Vec<Ty>,
    /// The dictionaries captured, in order: the superclass closure of the
    /// declared context (`superclass_closure` — an `Ord a` given also
    /// answers `==`), as (class, existential variable).
    captures: Vec<(String, String)>,
}

/// The transitive superclass closure of a constraint list — THE order every
/// dictionary convention of this module follows, for a dictionary-passing
/// function's parameters, a context instance's dictionary-form parameters
/// (and the sub-dictionaries its constructed dictionary closes over) and a
/// constrained existential constructor's captured fields alike. Each
/// declared constraint is followed by its superclasses, depth-first in the
/// class's declaration order: `Sortable a` under `class Eq a => Ord a` and
/// `class Ord a => Sortable a` gives `[Sortable a, Ord a, Eq a]`, and `Monad
/// m` gives `[Monad m, Applicative m, Functor m]`. Deduplicated on (class,
/// variable, argument), so a superclass that is also declared explicitly
/// keeps the position it first appeared at: `(Eq a, Ord a)` is `[Eq a, Ord
/// a]`, `(Ord a, Eq a)` is `[Ord a, Eq a]` — one dictionary per class and
/// variable, never two. The structured argument of a compound constraint
/// (`GEncode (Rep a)`) travels with the superclasses it introduces, so
/// `fn_constraint_args` stays parallel.
///
/// The typechecker discharges a superclass demand (`Eq a` from a declared
/// `Ord a`) by entailment, `class_satisfies`; that step leaves no runtime
/// evidence, so the dictionary-passing fallback must carry the superclass
/// dictionaries itself — exactly what GHC's dictionary layout provides by
/// storing them inside the subclass dictionary. mata-ll passes them as
/// separate parameters instead, which keeps a dictionary one class's
/// method table (`SpecKind::Dict`/`DictCtor`, `CodeGen::dict_field`) and
/// lets a method use resolve its (class, variable) in one lookup (T1).
fn superclass_closure(
    classes: &HashMap<String, ClassInfo>,
    declared: Vec<(TyConstraint, Option<Ty>)>,
) -> Vec<(TyConstraint, Option<Ty>)> {
    let mut out: Vec<(TyConstraint, Option<Ty>)> = Vec::new();
    for (c, arg) in declared {
        let mut stack = vec![c.class_name.clone()];
        while let Some(cls) = stack.pop() {
            if out.iter().any(|(k, a)| k.class_name == cls && k.type_var == c.type_var && *a == arg) {
                continue;
            }
            out.push((TyConstraint { class_name: cls.clone(), type_var: c.type_var.clone() }, arg.clone()));
            if let Some(ci) = classes.get(&cls) {
                for sup in ci.superclasses.iter().rev() {
                    stack.push(sup.clone());
                }
            }
        }
    }
    out
}

pub struct Monomorphizer {
    /// Constrained existential constructors by name (see `ExCtor`).
    ex_ctors: HashMap<String, ExCtor>,
    /// Captured dictionaries in scope, innermost last: (class, skolem id)
    /// -> the pattern-bound variable holding the dictionary (pushed by the
    /// pattern rewrite for the clause/branch/binding it scopes over).
    skolem_dicts: Vec<((String, u32), String)>,
    /// Counter for fresh dictionary binder names.
    ex_binder_counter: usize,
    /// Known polymorphic functions (name -> TFunction)
    poly_fns: HashMap<String, TFunction>,
    /// Set of prelude/built-in names (don't try to specialize these)
    builtins: HashSet<String>,
    /// The functions whose ONE generic copy is shared by every use instead
    /// of a specialization per instantiation — with one exception,
    /// `specialization_wanted`: a use whose instantiation WIDENS the
    /// function's arity gets its own copy, keyed by the use's arrow shape
    /// (`arity_shape`), so there is one such copy per shape, not per type.
    /// Members: the `builtins` that DO have a mata-ll body in this module
    /// (`id`, `const`, `flip`, `reverse`, …), and every constraint-free
    /// function that tripped the specialization cap (`trip_spec_cap`) — it
    /// has no dictionary form to switch to, so its retained generic copy is
    /// what serves its uses from then on, exactly like a builtin's.
    ///
    /// Codegen emits one Lua function per definition, with as many parameters
    /// as the DECLARED type has arrows, and every call site passes the
    /// arguments its own instantiated type calls for, in one flat call (the
    /// N-ary convention). The two agree for every other function because
    /// specialization gives each instantiation its own copy — but a shared
    /// `const :: a -> b -> a` used at `a := Int -> Int` is a two-parameter
    /// Lua function handed three arguments, and Lua discards the third; and
    /// the body's own calls count arrows from the type they can see, so a
    /// shared `apply f x = f x` at `b := Int -> Int` calls a two-parameter
    /// `f` with one argument.
    arity_only_fns: HashSet<String>,
    /// Collected specializations: SpecKey -> mangled name
    specializations: HashMap<SpecKey, String>,
    /// Generated specialized functions
    generated: Vec<TFunction>,
    /// Instance dispatch table, shared with the typechecker's canonical
    /// registry: (method_name, instance head) -> implementation function.
    /// The head is the structured identity of the instance's target type
    /// (see `InstHead`) — instance selection never looks at Display strings.
    instance_methods: HashMap<(String, InstHead), String>,
    /// Full instance records (target type + declared context), keyed like
    /// `instance_methods`. The dictionary-passing rewrite reads the context
    /// to build a dictionary for a structured type (`[a]`, `T a`) out of the
    /// dictionaries its element types demand.
    instances: HashMap<(String, InstHead), InstanceInfo>,
    /// How to DESCRIBE each instance method function in diagnostics:
    /// mangled fn name -> a user-facing phrase ("the derived Show instance
    /// for 'W'"). Errors inside instance methods used to leak the internal
    /// mangled name ("in definition of 'ord_compare__W'"), which the user
    /// never wrote.
    instance_fn_ctx: HashMap<String, String>,
    /// Dictionary-form methods generated for parameterized instances:
    /// (class, head) -> [(method, dictform function name)] in
    /// class-declaration order. A dictionary-form method takes its
    /// instance context's dictionaries as leading parameters, so the
    /// dictionary for `[a]` can be constructed at runtime from the
    /// dictionary for `a` (GHC's dictionary elaboration, scoped to the
    /// polymorphic-recursion path that needs it).
    dictform_impls: HashMap<(String, InstHead), Vec<(String, String, usize)>>,
    /// Implementations *generated by this pass* for structural types, memoized
    /// per concrete type: (method_name, type) -> generated function name.
    /// E.g. ("show", [Int]) -> show_LInteger, ("==", (Int, String)) ->
    /// eq_TupInteger_String. Structurally keyed like everything else.
    generated_impls: HashMap<(String, Ty), String>,
    /// Every mangled name this pass has handed out, with the (base, type) it
    /// stands for. `ty_to_suffix` is readable but NOT injective — `(A_B, C)`
    /// and `(A, B_C)` both suffix to "TupA_B_C" — so two distinct types could
    /// silently share one generated function name (one clobbers the other in
    /// the emitted Lua). Genuine collisions get a deterministic "__2"/"__3"…
    /// disambiguator; non-colliding names are unchanged.
    mangled_names: HashMap<String, (String, Ty)>,
    /// Errors collected during monomorphization, located at the clause
    /// being processed when they arose (see `cur_ctx`/`cur_span`).
    pub errors: Vec<Diagnostic>,
    /// Diagnoses of BACKEND limitations, keyed by the function whose body
    /// they sit in, reported only if that function survives dead-code
    /// elimination (see `compile_impl`). The one producer today is the
    /// first-class-dict error: a still-polymorphic reference to a
    /// dict-passing function is uncompilable ONLY where it is emitted, and
    /// the GENERIC original that holds it is usually dead once every real
    /// use specialized — erroring eagerly rejected GHC-legal programs (G7:
    /// `pick True h = h; pick False _ = gg` used only at Int). Unlike type
    /// errors, which hold whether or not the code is reachable, an
    /// unemittable expression in unemitted code limits nothing.
    pub deferred_errors: Vec<(String, Diagnostic)>,
    /// The function whose clauses `rewrite_non_dict_fn` is currently
    /// rewriting — the reachability key for `deferred_errors`.
    cur_rewrite_fn: Option<String>,
    /// The enclosing definition currently being monomorphized, phrased like
    /// the typechecker's contexts ("definition of 'main'", "clause 2 of 'f'").
    cur_ctx: Option<String>,
    /// Source span of the clause currently being monomorphized. `None` inside
    /// compiler-synthesized clauses (derived instances, generated impls).
    cur_span: Option<Span>,
    /// Constrained functions that use dictionary-passing instead of
    /// monomorphization. A constraint-free function that trips the
    /// specialization cap has no dictionary form; it joins
    /// `arity_only_fns` instead.
    dict_passing_fns: HashSet<String>,
    /// Distinct instantiation count at the moment each function tripped the
    /// specialization cap (see `trip_spec_cap`), constrained or not — the
    /// record that a function HAS tripped — reported by the first-class-use
    /// diagnostic so the cause is legible at the use site.
    dict_passing_counts: HashMap<String, usize>,
    /// Specializations deleted when their function tripped the specialization
    /// limit: purged mangled name -> the function that now serves its uses:
    /// a constraint-free function's generic copy, or, for an
    /// arity-widening use, the arrow-shape copy regenerated at the trip
    /// (`widening_copy`). Call sites rewritten to a purged name
    /// BEFORE the limit tripped are redirected in `run` — otherwise they
    /// reference functions that no longer exist. A purged copy of a
    /// CONSTRAINED function is not redirected: its name keeps a function
    /// of its type (`ground_forms`), because the dictionary-passing
    /// original cannot stand in for it wherever the name is not a call
    /// the rewrite reaches. A dictionary table is never redirected
    /// either: a ground type's dictionary is decided only after the last
    /// trip (`dict_demands`), so no table ever names a purged copy.
    purged_specs: HashMap<String, String>,
    /// The names under which a dictionary-passing function's VALUE at a
    /// ground type exists as a plain function: a copy that was emitted
    /// before its function tripped the cap (kept — it is correct code for
    /// its type), or the stub `dict_stub` generates under the name the
    /// copy had or would have had. A reference to such a name is right in
    /// every position — a call, a first-class use, the element function a
    /// structural thread carries by name (`SpecKind::ListEq` & co.) —
    /// which a reference to the bare dictionary-passing function is not:
    /// only a call site the rewrite reaches gets its dictionaries.
    ground_forms: HashSet<String>,
    /// The dictionaries of GROUND types the rewrite passes demanded
    /// (`build_dict_expr`), in first-demand order, each with the
    /// diagnostic context of the site that first demanded it: (class,
    /// type, context, span). A ground type needs no evidence from the
    /// demanding scope, so its dictionary is decided in one place, after
    /// the fixpoint of `run` is final (`materialize_dicts`); until then
    /// the site holds a `SpecKind::DictDemand` placeholder. Deciding it
    /// at the site froze a specialization name that a later trip of its
    /// instance method purged: `revert_purged` then renamed the entry to
    /// the method's original — by then its dictionary form, taking a
    /// context dictionary first — so every table built before the trip
    /// called the method with its arguments shifted by one (T13: `print
    /// (Box v)` at 21 types under `instance Show a => Show (Box a)` died
    /// on the first line).
    dict_demands: Vec<(String, Ty, Option<String>, Option<Span>)>,
    /// The (class, type) pairs in `dict_demands`, for its dedup.
    dict_demanded: HashSet<(String, Ty)>,
    /// The decided table per demand. Rebuilt by every pass of `run`'s
    /// fixpoint, so the decisions put in (`fill_dict_demands`) are those
    /// of the last pass — the one that tripped nothing.
    dict_tables: HashMap<(String, Ty), TExpr>,
    /// Typeclass constraints per function: the superclass closure of the
    /// declared context, in `superclass_closure` order — one entry per
    /// dictionary the function's dictionary-passing form takes, which is
    /// the one convention every call site and body rewrite is built from.
    fn_constraints: HashMap<String, Vec<TyConstraint>>,
    /// Class definitions (class_name -> ClassInfo)
    classes: HashMap<String, ClassInfo>,
    /// Method name -> class name (reverse lookup)
    method_to_class: HashMap<String, String>,
    /// The constraints a class method carries BEYOND its class's own
    /// (`traverse :: (Traversable t, Applicative f) => …` carries
    /// `Applicative f`), as their superclass closure — one dictionary
    /// each, in this order. GHC's dictionary layout: the method's entry in
    /// the class dictionary is a function of those dictionaries first.
    /// Here, the method's dictionary form takes them after the instance
    /// context's dictionaries (`ensure_dictform`), and every
    /// dictionary-routed use applies the entry to the dictionaries built
    /// for the method's own variables at that use (`dict_method_use`).
    /// Only methods with such constraints are present.
    method_dict_constraints: HashMap<String, Vec<TyConstraint>>,
    /// Locally-bound names (lambda params, let binds, case pattern vars)
    /// that should NOT be resolved as typeclass methods
    locals: HashSet<String>,
    /// Stack of (function name, mangled name) for specializations currently
    /// being generated. A still-polymorphic recursive call inside a
    /// specialization resolves to the top-most matching entry — deterministic
    /// and correct under nested specialization (unlike picking an arbitrary
    /// entry from the `specializations` HashMap).
    gen_stack: Vec<(String, String)>,
    /// The (class, dict param) pairs of the function currently being rewritten
    /// for dictionary passing, in constraint DECLARATION order. `class_to_dict`
    /// carries the same pairs as a lookup map; this Vec is the ordered view for
    /// the one place that must enumerate them (the no-recorded-constraints
    /// fallback in `rewrite_dict_expr`) — enumerating the HashMap there would
    /// pass dictionaries in nondeterministic order. Saved/restored around each
    /// rewrite scope because dictionary construction nests (`build_dict_expr`
    /// can enter `build_concrete_dict`, which rewrites with its own context).
    cur_dict_params: Vec<(String, String)>,
    /// Per-function constraint argument types (parallel to `fn_constraints`),
    /// so dictionary passing can resolve a COMPOUND constraint (`GEncode (Rep
    /// a)`) whose `type_var` string is opaque — see the checker field of the
    /// same name.
    fn_constraint_args: HashMap<String, Vec<(String, Ty)>>,
    /// The current dictionary-passing function's `(class, argument type,
    /// parameter name)` triples, so a method use inside it whose class-variable
    /// binding matches a compound constraint routes to that parameter instead
    /// of rebuilding the dictionary. Saved/restored around each rewrite scope.
    cur_dict_by_arg: Vec<(String, Ty, String)>,
    /// True while `ensure_dictform` monomorphizes an instance-method body: a
    /// still-polymorphic call must then keep its NAME (for the dictionary
    /// rewrite) instead of falling back to an arbitrary existing
    /// specialization of it.
    in_dictform: bool,
    /// Closed-type-family equations (borrowed from the checker) so a family
    /// application in a method's dispatch type is reduced to its concrete form
    /// before instance selection. The Generics `Rep` family is the caller:
    /// `gToJSON :: Rep a -> Json` dispatches on `Rep T`, which must reduce to
    /// the synthesized representation before its `GToJSON` instance is found.
    ty_families: TyFamilies,
}

impl Monomorphizer {
    pub fn new(checker: &Checker) -> Self {
        let mut builtins = HashSet::new();
        // Only mark as builtin functions that DON'T have user-defined instances
        for name in &[
            "putStrLn", "++", "$", ".", "id", "const",
            "flip", "not", "error", "sqrt", "otherwise",
            "+", "-", "*", "/", "==", "/=", "<", ">", "<=", ">=",
            // foldr/foldl are Foldable class methods and length carries a
            // Foldable constraint (all dispatched/specialized), so none of
            // them belong here.
            "&&", "||", "mod", "div", "quot", "rem", "map", "filter",
            "True", "False", "Just", "Nothing",
            ":", "[]", "head", "tail", "take", "drop", "zipWith", "reverse",
            "engage", "liftIO",
            "hmEmpty", "hmInsert", "hmLookup", "hmDelete",
            "hmSize", "hmKeys", "hmValues", "hmMember", "hmToList",
            "bsEmpty", "bsLength", "bsIndex", "bsSub", "bsSingleton",
            "bsConcat", "bsConcatList", "bsNull", "bsHead", "bsTail", "bsCons", "bsSnoc",
            "bsReplicate", "bsPack", "bsUnpack", "bsMap", "bsFoldl",
            "bsXor", "bsZipWith", "bsToString", "bsFromString",
            "bsGetU16LE", "bsGetU32LE", "bsGetI8", "bsGetI16LE", "bsPutI16LE",
            "runST",
        ] {
            builtins.insert(name.to_string());
        }
        for i in crate::intrinsics::ST_INTRINSICS {
            builtins.insert(i.source.to_string());
        }

        // Adopt the typechecker's canonical instance registry: it is already
        // keyed by the structured head of each instance's target type, which
        // is exactly the identity instance dispatch needs.
        let mut class_methods = HashSet::new();
        let mut instance_methods = HashMap::new();
        let mut instances = HashMap::new();

        let mut instance_fn_ctx = HashMap::new();
        for ((class_name, head), inst) in checker.get_instances() {
            instances.insert((class_name.clone(), head.clone()), inst.clone());
            for (method_name, mangled) in &inst.method_fns {
                class_methods.insert(method_name.clone());
                instance_methods.insert((method_name.clone(), head.clone()), mangled.clone());
                if mangled != method_name {
                    // A derived instance comes from the deriving clause,
                    // which is where the user can act on the error.
                    let phrase = if inst.derived {
                        format!(
                            "the derived {} instance for '{}' (from its deriving clause)",
                            class_name, inst.target_type
                        )
                    } else {
                        format!("the {} instance for '{}'", class_name, inst.target_type)
                    };
                    instance_fn_ctx.insert(mangled.clone(), phrase);
                }
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

        // A method's own constraints beyond its class's (see the field):
        // the checker records every constraint a method use emits,
        // including the class's own `C classvar`; drop that one and close
        // the rest over superclasses, the convention every dictionary
        // list follows.
        let mut method_dict_constraints: HashMap<String, Vec<TyConstraint>> = HashMap::new();
        for (method, cs) in checker.get_method_constraints() {
            let Some(class_name) = method_to_class.get(method) else { continue };
            let Some(info) = checker.get_classes().get(class_name) else { continue };
            let extra: Vec<(TyConstraint, Option<Ty>)> = cs.iter()
                .filter(|c| !(c.class_name == *class_name && c.type_var == info.type_var))
                .map(|c| (c.clone(), None))
                .collect();
            if extra.is_empty() {
                continue;
            }
            let closed = superclass_closure(checker.get_classes(), extra)
                .into_iter().map(|(c, _)| c).collect();
            method_dict_constraints.insert(method.clone(), closed);
        }

        // Constrained existential constructors: the dictionaries each one
        // captures — the superclass closure of its declared context.
        let classes_ref = checker.get_classes();
        let mut ex_ctors: HashMap<String, ExCtor> = HashMap::new();
        for (cname, info) in checker.get_constructors() {
            if info.existential_constraints.is_empty() {
                continue;
            }
            let captures = superclass_closure(
                classes_ref,
                info.existential_constraints.iter().map(|c| (c.clone(), None)).collect(),
            ).into_iter().map(|(c, _)| (c.class_name, c.type_var)).collect();
            ex_ctors.insert(cname.clone(), ExCtor {
                field_types: info.field_types.clone(),
                captures,
            });
        }

        // Every constraint list a dictionary convention is built from is
        // the SUPERCLASS CLOSURE of what was declared (see
        // `superclass_closure`): a function's context (`fn_constraints`,
        // with `fn_constraint_args` kept parallel) and a context instance's
        // context (`instances`). The checker discharges `Eq a` from a
        // declared `Ord a` by entailment (`class_satisfies`), which leaves
        // no runtime evidence; the dictionary-passing fallback must carry
        // the `Eq` dictionary itself, or `x == y` under `Ord a` reaches
        // codegen as a raw operator — Lua's `==`, table identity (T1).
        // Prefer the final-name spelling of each function's constraints
        // (the names its checked type actually uses); fall back to the
        // source-name record for functions the checker did not re-express
        // (none today, defensively).
        let mut fn_constraints = checker.get_fn_constraints();
        for (name, use_cs) in checker.get_fn_dict_constraints() {
            fn_constraints.insert(name, use_cs);
        }
        let mut fn_constraint_args = checker.get_fn_constraint_args();
        for (name, cs) in fn_constraints.iter_mut() {
            let args = fn_constraint_args.get(name).cloned().unwrap_or_default();
            // The argument types are parallel to the constraints by
            // declaration order; a (defensive) length mismatch leaves the
            // arguments unexpanded rather than pairing them wrongly.
            let paired: Vec<(TyConstraint, Option<Ty>)> = if args.len() == cs.len() {
                cs.iter().cloned().zip(args.into_iter().map(|(_, t)| Some(t))).collect()
            } else {
                cs.iter().cloned().map(|c| (c, None)).collect()
            };
            let closed = superclass_closure(classes_ref, paired);
            if closed.iter().all(|(_, arg)| arg.is_some()) {
                fn_constraint_args.insert(
                    name.clone(),
                    closed.iter()
                        .map(|(c, arg)| (c.class_name.clone(), arg.clone().unwrap()))
                        .collect(),
                );
            }
            *cs = closed.into_iter().map(|(c, _)| c).collect();
        }
        for inst in instances.values_mut() {
            if let Some(ctx) = inst.context.take() {
                inst.context = Some(
                    superclass_closure(classes_ref, ctx.into_iter().map(|c| (c, None)).collect())
                        .into_iter().map(|(c, _)| c).collect(),
                );
            }
        }

        Monomorphizer {
            ex_ctors,
            skolem_dicts: Vec::new(),
            ex_binder_counter: 0,
            poly_fns: HashMap::new(),
            builtins,
            arity_only_fns: HashSet::new(),
            specializations: HashMap::new(),
            generated: Vec::new(),
            instance_methods,
            instances,
            instance_fn_ctx,
            dictform_impls: HashMap::new(),
            generated_impls: HashMap::new(),
            mangled_names: HashMap::new(),
            errors: Vec::new(),
            deferred_errors: Vec::new(),
            cur_rewrite_fn: None,
            cur_ctx: None,
            cur_span: None,
            dict_passing_fns: HashSet::new(),
            dict_passing_counts: HashMap::new(),
            purged_specs: HashMap::new(),
            ground_forms: HashSet::new(),
            dict_demands: Vec::new(),
            dict_demanded: HashSet::new(),
            dict_tables: HashMap::new(),
            gen_stack: Vec::new(),
            cur_dict_params: Vec::new(),
            fn_constraints,
            classes: checker.get_classes().clone(),
            method_to_class,
            method_dict_constraints,
            locals: HashSet::new(),
            fn_constraint_args,
            cur_dict_by_arg: Vec::new(),
            in_dictform: false,
            ty_families: checker.get_ty_families().clone(),
        }
    }

    /// Does the concrete/stuck type `bound` match a constraint argument pattern
    /// `pat` — structurally, with a type VARIABLE on either side matching
    /// anything? `Rep V ~ Rep a` matches (same family head, one variable arg);
    /// `Rep V ~ Maybe a` does not. Used to route a method use to the dictionary
    /// parameter standing for the compound constraint it discharges.
    fn arg_matches(pat: &Ty, bound: &Ty) -> bool {
        match (pat, bound) {
            (Ty::Var(_), _) | (_, Ty::Var(_)) => true,
            (Ty::Con(a), Ty::Con(b)) => a == b,
            (Ty::App(a1, b1), Ty::App(a2, b2)) =>
                Self::arg_matches(a1, a2) && Self::arg_matches(b1, b2),
            (Ty::List(a), Ty::List(b)) | (Ty::IO(a), Ty::IO(b)) => Self::arg_matches(a, b),
            (Ty::Tuple(a), Ty::Tuple(b)) =>
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| Self::arg_matches(x, y)),
            (Ty::Unit, Ty::Unit) => true,
            _ => false,
        }
    }

    /// Reduce every closed-type-family application in `ty` to normal form,
    /// returning the reduct only when it changed. A method's dispatch type may
    /// still carry a family application (`Rep T`) once its class variable is
    /// pinned to a concrete type; reducing it exposes the real representation
    /// head the instance is registered under. A stuck or divergent application
    /// is left as-is (`None`).
    fn reduce_families(&self, ty: &Ty) -> Option<Ty> {
        match crate::types::reduce_type_families(ty, &self.ty_families) {
            Ok(reduced) if &reduced != ty => Some(reduced),
            _ => None,
        }
    }

    pub fn run(&mut self, mut module: TModule) -> TModule {
        // A constrained existential constructor carries its captured
        // dictionaries as hidden TRAILING fields: extend the data
        // definitions so codegen's constructor functions take them and the
        // declared fields keep their positions.
        for def in module.data_defs.iter_mut().chain(module.dropped_data_defs.iter_mut()) {
            for con in def.constructors.iter_mut() {
                let Some(ex) = self.ex_ctors.get(&con.name) else { continue };
                match &mut con.fields {
                    TConFields::Positional(tys) => {
                        for _ in &ex.captures {
                            tys.push(Ty::Unit);
                        }
                    }
                    TConFields::Named(fs) => {
                        for (i, (cls, var)) in ex.captures.iter().enumerate() {
                            let name = format!("__exdict_{}_{}_{}", cls, var, i);
                            fs.push(TRecordField { key: name.clone(), name, ty: Ty::Unit });
                        }
                    }
                }
            }
        }
        // Collect polymorphic user-defined functions
        for func in &module.functions {
            if self.is_polymorphic(&func.ty) {
                if self.builtins.contains(&func.name) {
                    // A builtin stays shared; only an arity-widening use of it
                    // is specialized (see `arity_only_fns`).
                    self.arity_only_fns.insert(func.name.clone());
                }
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

        // Monomorphize instance methods too. The module is owned: its
        // function vectors move through mono_function (they were once
        // cloned out of the module first, once per function).
        let mut instance_fns: Vec<TFunction> = std::mem::take(&mut module.instance_fns)
            .into_iter()
            .map(|f| self.mono_function(f))
            .collect();

        // Walk all functions to collect specialization demands
        let functions: Vec<TFunction> = std::mem::take(&mut module.functions)
            .into_iter()
            .map(|f| self.mono_function(f))
            .collect();

        // Keep all original functions (including polymorphic ones — they serve
        // as fallbacks for calls inside other polymorphic contexts where types
        // aren't resolved). Append generated specializations after.
        let mut result_fns: Vec<TFunction> = functions;
        result_fns.append(&mut self.generated);

        // Redirect references to purged specializations. When a function
        // trips the specialization limit mid-pass, call sites monomorphized
        // EARLIER already reference its (now deleted) specializations; point
        // them at what now serves them (`purged_specs`): the original
        // function, so the dictionary-passing rewrite below — or the
        // retained generic copy — handles them, or the arrow-shape copy
        // regenerated at the trip for an arity-widening use.
        if !self.purged_specs.is_empty() {
            for func in result_fns.iter_mut().chain(instance_fns.iter_mut()) {
                for clause in &mut func.clauses {
                    clause.map_exprs(&mut |e| self.revert_purged(e));
                }
            }
        }

        // The dictionary phase: rewrite dict-passing functions and their
        // call sites, and decide the dictionary of every ground type a
        // rewrite demanded. A fixpoint over the whole phase: rewriting one
        // body can DISCOVER further dict-passing functions — a sibling
        // constrained call (gdTag calling gdConBody) marks its callee —
        // and deciding a dictionary can trip a fresh specialization cap
        // (flagging an instance method mid-phase, purging its copies).
        // Each newly flagged function needs its own body rewrite AND a
        // fresh call-site pass over everything already processed, and
        // every ground dictionary is decided again: the sites hold
        // placeholders until the phase ends, so no table can hold a name a
        // later trip purges (T13). A pass that changes nothing — no new
        // flag, no trip — is the last; its decisions stand. (A trip is
        // counted in `dict_passing_counts` for constrained and
        // constraint-free functions alike, so the state covers the purge
        // of a shared function's copies too.)
        if !self.dict_passing_fns.is_empty() || !self.dict_demands.is_empty() {
            let mut rewritten: HashSet<String> = HashSet::new();
            loop {
                // (a) Rewrite the bodies of newly flagged dict-passing
                // functions (worklist: a rewrite can flag more). An instance
                // METHOD that tripped the cap lives in instance_fns, not
                // result_fns — search both. Sorted: rewriting a dict-passing
                // function can GENERATE new functions (dictionary-form
                // instance methods, via build_dict_expr → ensure_dictform),
                // which are appended to the module in the order they are
                // first demanded. Iterating the HashSet directly would make
                // that order — and the emitted __mll_fn slot numbering —
                // vary from run to run.
                loop {
                    let mut dict_fns: Vec<String> = self.dict_passing_fns.iter()
                        .filter(|n| !rewritten.contains(*n))
                        .cloned().collect();
                    if dict_fns.is_empty() { break; }
                    dict_fns.sort();
                    for name in &dict_fns {
                        rewritten.insert(name.clone());
                        // result_fns / instance_fns are locals, so the
                        // rewrite works in place (no clone-and-write-back).
                        if let Some(func) = result_fns.iter_mut().find(|f| f.name == *name) {
                            self.rewrite_dict_passing_fn(func);
                        } else if let Some(func) = instance_fns.iter_mut().find(|f| f.name == *name) {
                            self.rewrite_dict_passing_fn(func);
                        }
                    }
                }
                // What (b)–(d) may change; a flag (a) discovers is served by
                // its own worklist, and (b)–(d) run after it in any case.
                let before = self.dictionary_phase_state();
                // (b) Revert purged references and rewrite call sites in
                // non-dict functions. Idempotent per function: DictCall nodes
                // pass through untouched, so re-running after a new flag only
                // converts the calls to the newly flagged names.
                // `TClause::map_exprs` covers body, guards AND where-binding
                // bodies — the previous inlined copies of this loop skipped
                // the guards, so a dict-passing call in a guard condition was
                // never rewritten.
                for func in &mut result_fns {
                    self.rewrite_non_dict_fn(func);
                }
                for func in &mut instance_fns {
                    self.rewrite_non_dict_fn(func);
                }
                // (c) The rewrites may have generated new functions:
                // dictionary-form instance methods and arrow-shape copies.
                // Their bodies never went through the loops above, so give
                // each batch the same treatment before emitting.
                self.settle_generated(&mut result_fns);
                // (d) Decide the dictionary of every ground type demanded so
                // far, from the current set of dictionary-passing functions.
                // This is where the specializations a static dictionary
                // names are generated — and where an instance method can
                // trip its cap: then the state changed, and the next pass
                // decides every table again. Deciding generates functions
                // (dictionary forms, specializations) whose settling can
                // demand further dictionaries, so decide and settle until
                // every demand has its table.
                self.materialize_dicts(&mut result_fns);
                if self.dictionary_phase_state() == before {
                    break;
                }
            }
            // The fixpoint is final: put the decided tables in.
            for func in result_fns.iter_mut().chain(instance_fns.iter_mut()) {
                for clause in &mut func.clauses {
                    clause.map_exprs(&mut |e| self.fill_dict_demands(e));
                }
            }
            self.dict_tables.clear();
        }

        debug_assert!(
            module.passes_run.is_empty(),
            "mono must run first on the typechecker's TModule (pass-order witness)"
        );
        TModule {
            data_defs: module.data_defs,
            dropped_data_defs: module.dropped_data_defs,
            functions: result_fns,
            instance_fns,
            has_main: module.has_main,
            exports: module.exports,
            source_names: module.source_names,
            record_accessors: module.record_accessors,
            newtypes: module.newtypes,
            newtype_types: module.newtype_types,
            passes_run: vec!["mono"],
        }
    }

    /// What a pass of the dictionary phase can change in a way that
    /// invalidates an earlier product: the set of dictionary-passing
    /// functions (a new flag makes a bare call of it a call without its
    /// dictionaries) and the set of functions that tripped the cap (a trip
    /// purges copies and redirects their uses — for constrained and
    /// constraint-free functions alike, `dict_passing_counts` records
    /// both). Each grows monotonically and is bounded by the module's
    /// functions, so the phase's loop terminates.
    fn dictionary_phase_state(&self) -> (usize, usize) {
        (self.dict_passing_fns.len(), self.dict_passing_counts.len())
    }

    /// Move every function generated so far into `result_fns`, each given
    /// the call-site half of the dictionary phase first (its body never
    /// went through the rewrite loops of `run`). A rewrite can generate
    /// more, so this drains in batches until nothing is left.
    fn settle_generated(&mut self, result_fns: &mut Vec<TFunction>) {
        while !self.generated.is_empty() {
            let mut batch = std::mem::take(&mut self.generated);
            for func in &mut batch {
                self.rewrite_non_dict_fn(func);
            }
            result_fns.append(&mut batch);
        }
    }

    /// Step (d) of `run`'s fixpoint: decide the table of every ground
    /// dictionary demanded (`dict_demands`), in demand order, from the
    /// CURRENT set of dictionary-passing functions — the same decision
    /// `decide_dict` makes for a type with evidence in scope, with no
    /// evidence (a ground type needs none), under the diagnostic context
    /// of the site that first demanded it. Deciding one can demand more:
    /// the context dictionaries of a composed instance dictionary, and
    /// whatever the functions it generated demand once settled. Every
    /// pass starts over, so a table decided before a trip — naming a
    /// copy the trip purged — is replaced by the decision of the pass that
    /// trips nothing, the last.
    fn materialize_dicts(&mut self, result_fns: &mut Vec<TFunction>) {
        self.dict_tables.clear();
        let mut decided = 0;
        loop {
            while decided < self.dict_demands.len() {
                let (class, ty, ctx, span) = self.dict_demands[decided].clone();
                decided += 1;
                let saved_ctx = std::mem::replace(&mut self.cur_ctx, ctx);
                let saved_span = std::mem::replace(&mut self.cur_span, span);
                let table = self.decide_dict(&class, &ty, &DictEnv::default());
                self.cur_ctx = saved_ctx;
                self.cur_span = saved_span;
                self.dict_tables.insert((class, ty), table);
            }
            self.settle_generated(result_fns);
            if decided == self.dict_demands.len() {
                break;
            }
        }
    }

    /// Replace every `DictDemand` placeholder in `expr` by its decided
    /// table — recursively, as a composed table's context dictionaries are
    /// demands of their own (each for a proper part of the type, so the
    /// recursion bottoms out). Runs once, after the last pass of `run`'s
    /// fixpoint decided every demand.
    fn fill_dict_demands(&self, expr: TExpr) -> TExpr {
        if let TExprKind::SpecCall { specialized: SpecKind::DictDemand { class, ty }, .. } = &expr.kind {
            let table = self.dict_tables.get(&(class.clone(), ty.clone()))
                .unwrap_or_else(|| panic!(
                    "internal: the '{class}' dictionary for '{ty}' was demanded but never decided"))
                .clone();
            debug_assert!(
                Self::dict_table_names(&table).iter().all(|n| !self.purged_specs.contains_key(*n)),
                "the '{class}' dictionary for '{ty}' names a purged specialization"
            );
            return self.fill_dict_demands(table);
        }
        expr.map_children(&mut |c| self.fill_dict_demands(c))
    }

    /// The implementation names a decided dictionary table holds (its own
    /// entries only; the context dictionaries it is applied to are
    /// demands, filled separately).
    fn dict_table_names(table: &TExpr) -> Vec<&str> {
        match &table.kind {
            TExprKind::SpecCall { specialized: SpecKind::Dict { methods, .. }, .. } =>
                methods.iter().map(|(_, n)| n.as_str()).collect(),
            TExprKind::SpecCall { specialized: SpecKind::DictCtor { methods, .. }, .. } =>
                methods.iter().map(|(_, n, _)| n.as_str()).collect(),
            _ => Vec::new(),
        }
    }

    /// The call-site half of the dictionary phase for a function that is NOT
    /// itself dictionary-passing: revert references to purged
    /// specializations, then rewrite calls to dict-passing functions.
    /// Idempotent per function (DictCall nodes pass through untouched), so a
    /// re-run after a new flag only converts the calls to the newly flagged
    /// names. `TClause::map_exprs` covers body, guards AND where-binding
    /// bodies — an earlier inlined copy of this loop skipped the guards, so a
    /// dict-passing call in a guard condition was never rewritten.
    fn rewrite_non_dict_fn(&mut self, func: &mut TFunction) {
        if self.dict_passing_fns.contains(&func.name) {
            return;
        }
        let saved_fn = self.cur_rewrite_fn.replace(func.name.clone());
        for clause in &mut func.clauses {
            // Location for any diagnostic the rewrite pushes (a first-class
            // use of a dict-passing function at a polymorphic type).
            let saved_ctx = self.cur_ctx.replace(
                format!("definition of '{}'", func.name));
            let saved_span = std::mem::replace(&mut self.cur_span, clause.span);
            self.map_clause_exprs_scoped(clause, &mut |m, e| {
                let e = m.revert_purged(e);
                m.rewrite_dict_call_sites(e)
            });
            self.cur_ctx = saved_ctx;
            self.cur_span = saved_span;
        }
        self.cur_rewrite_fn = saved_fn;
    }

    /// The type the CLASS VARIABLE of `method` is bound to at a use site,
    /// found by structurally matching the method's declared type against the
    /// type it is used at. E.g. `fromJSON :: Json -> Either String a` used at
    /// `Json -> Either String Person` binds a := Person.
    ///
    /// This is the typechecker's instance evidence: `use_ty` is the
    /// instantiated method type the typechecker recorded on the TIR node (the
    /// same instantiation its `wanted` constraint was solved at), and the
    /// declared method signature comes from the typechecker's class registry.
    /// The match recovers the exact substitution the checker made, wherever
    /// the class variable sits — first argument, return type, nested, or
    /// higher-kinded (`m a` against `IO b`/`[b]`/`LuaIO s b` via the sugared
    /// cases in `collect_subst_by_name`).
    fn class_var_binding(&self, method: &str, use_ty: &Ty) -> Option<Ty> {
        let class_name = self.method_to_class.get(method)?;
        let info = self.classes.get(class_name)?;
        let (_, decl_ty) = info.methods.iter().find(|(n, _)| n == method)?;
        // A type-family application in the DECLARED method type (`Rep a` in
        // `from :: a -> Rep a`) is not structurally invertible: the family is
        // not injective, so matching its argument against the concrete type
        // cannot determine the class variable, and doing so would bind it to a
        // piece of the family's REDUCT (`a := Sum U1 U1` from `Rep a ~ Sum U1
        // (Sum U1 U1)`). Blank those positions before matching; the class
        // variable is recovered from an injective occurrence elsewhere (the
        // plain `a` argument of `from`, the plain `a` result of `to`).
        let decl_ty = self.blank_family_apps(decl_ty);
        let mut bindings: HashMap<String, Ty> = HashMap::new();
        Self::collect_subst_by_name(&decl_ty, use_ty, &mut bindings);
        bindings.get(&info.type_var).cloned()
    }

    /// Replace every type-family application in `ty` with an opaque marker so
    /// it matches nothing structurally (see `class_var_binding`). Recurses into
    /// every type former; a family-headed `App` spine becomes the marker whole.
    fn blank_family_apps(&self, ty: &Ty) -> Ty {
        let (head, _) = {
            let mut h = ty;
            let mut n = 0;
            while let Ty::App(f, _) = h { h = f.as_ref(); n += 1; }
            (h, n)
        };
        if let Ty::Con(name) = head {
            if self.ty_families.contains(name) {
                return Ty::Con("__mll_opaque_family__".into());
            }
        }
        match ty {
            Ty::App(f, a) => Ty::App(Box::new(self.blank_family_apps(f)), Box::new(self.blank_family_apps(a))),
            Ty::Arrow(a, b, m) => Ty::Arrow(Box::new(self.blank_family_apps(a)), Box::new(self.blank_family_apps(b)), *m),
            Ty::List(a) => Ty::List(Box::new(self.blank_family_apps(a))),
            Ty::IO(a) => Ty::IO(Box::new(self.blank_family_apps(a))),
            Ty::LuaIO(s, a) => Ty::LuaIO(s.clone(), Box::new(self.blank_family_apps(a))),
            Ty::Tuple(es) => Ty::Tuple(es.iter().map(|e| self.blank_family_apps(e)).collect()),
            Ty::Forall(v, i) => Ty::Forall(v.clone(), Box::new(self.blank_family_apps(i))),
            other => other.clone(),
        }
    }

    /// THE dispatch decision for a class-method use — see `MethodUse`.
    /// `use_ty` is the instantiated method type the typechecker recorded
    /// on the node (for an operator, `lhs -> rhs -> result`; for prefix
    /// negation, `t -> t`; for a literal, `Integer -> t` or `Number ->
    /// t`); `ev` is the dictionary environment of the enclosing
    /// dictionary-form body, empty in the main walk. Called from every
    /// arm of `mono_expr_node` and `rewrite_dict_expr` where a method can
    /// appear, so the two passes make one decision:
    ///
    ///   1. the class-variable binding is read off `use_ty`
    ///      (`class_var_binding`) and family-reduced;
    ///   2. a binding that needs no evidence (`needs_evidence`: fully
    ///      concrete, or a head whose instance demands nothing of the
    ///      variable parts — `ST s`, `Either e`) resolves to its
    ///      implementation (`resolve_at_type`): `Native` when the
    ///      implementation is the operator itself, `Impl` otherwise,
    ///      `Missing` with no instance;
    ///   3. a binding over type variables or existential skolems is
    ///      `Evidence` when every dictionary it needs is in scope
    ///      (`dict_evidence_for`): the parameter, the captured binder, or
    ///      a dictionary composed from them (`dict_method_use`);
    ///   4. with no evidence in scope, inside the main walk of a body that
    ///      is about to be rewritten to dictionary form (`in_dictform`)
    ///      the use is `Deferred` to that rewrite — the node must stay as
    ///      written so the rewrite finds it;
    ///   5. otherwise no evidence will ever come (the retained generic
    ///      copy of a polymorphic function, a rank-2 skolem): the
    ///      type-erased best effort, `erased_method_impl`, or `Deferred`
    ///      to codegen's erased lowering.
    fn dispatch_method(&mut self, method: &str, use_ty: &Ty, ev: &DictEnv) -> MethodUse {
        let Some(class) = self.method_to_class.get(method).cloned() else {
            return MethodUse::Deferred;
        };
        let Some(binding) = self.class_var_binding(method, use_ty) else {
            // The binding could not be recovered (an unusual method
            // shape): the enclosing body's parameter for the class, if it
            // has one, else nothing to decide on.
            return match ev.class_to_dict.get(&class) {
                Some(p) => MethodUse::Evidence(TExprKind::DictAccess {
                    dict_param: p.clone(),
                    method_name: method.to_string(),
                }),
                None => MethodUse::Deferred,
            };
        };
        // The binding is read from the RAW method type (its injective
        // positions — `class_var_binding` blanks family apps), then the
        // binding itself is reduced: `f := Rep T` becomes the concrete
        // representation the instance is registered under. Specialization
        // of a parameterized instance body, in contrast, must match
        // against a use type whose families are ALSO reduced, so `Sum a b`
        // lines up with the real representation instead of the un-reduced
        // `Rep T`.
        let binding = self.reduce_families(&binding).unwrap_or(binding);
        if !self.needs_evidence(&class, &binding) {
            let reduced_use_ty = self.reduce_families(use_ty).unwrap_or_else(|| use_ty.clone());
            return match self.resolve_at_type(method, &binding, &reduced_use_ty) {
                Some(name) if name == method => MethodUse::Native,
                Some(name) => MethodUse::Impl(name),
                None => MethodUse::Missing(binding),
            };
        }
        if self.dict_evidence_for(&class, &binding, ev) {
            return MethodUse::Evidence(self.dict_method_use(&class, method, use_ty, ev));
        }
        if self.in_dictform {
            return MethodUse::Deferred;
        }
        match self.erased_method_impl(method, &binding) {
            Some(name) if name == method => MethodUse::Native,
            Some(name) => MethodUse::Impl(name),
            None => MethodUse::Deferred,
        }
    }

    /// Does a `class` method at binding `ty` need runtime evidence — is
    /// its instance, or something that instance's context demands, still
    /// polymorphic? A ground binding needs none; a type variable or an
    /// existential skolem always does; a structural shape of Eq/Ord/Show
    /// needs what its elements need; a type with an instance for its head
    /// needs what the instance's context demands of the bound arguments
    /// (`ST s`, `Either e` and `LuaIO s` under Monad demand nothing of
    /// their variable argument, so the head decides and the use lowers
    /// natively — the same decision the specializing path always made
    /// for them; a derived `Eq (Either a b)` at a class variable demands
    /// `Eq a`); a builtin instance registered without a context (the
    /// structural rule: every argument needs the class) needs evidence
    /// for every argument that is not ground.
    fn needs_evidence(&self, class: &str, ty: &Ty) -> bool {
        if ty.free_vars().is_empty() && !Self::contains_skolem(ty) {
            return false;
        }
        match ty {
            Ty::Var(_) | Ty::Skolem(..) => true,
            _ => {
                if Self::is_structural_class(class)
                    && let Some((_, elems)) = Self::structural_shape(ty)
                {
                    return elems.iter().any(|e| self.needs_evidence(class, e));
                }
                let Some(head) = InstHead::of(ty) else { return true };
                let Some(inst) = self.instances.get(&(class.to_string(), head)) else {
                    return true;
                };
                match &inst.context {
                    Some(ctx) => {
                        let mut bind: HashMap<String, Ty> = HashMap::new();
                        Self::collect_subst_by_name(&inst.target_type, ty, &mut bind);
                        ctx.iter().any(|c| match bind.get(&c.type_var) {
                            Some(sub) => self.needs_evidence(&c.class_name, sub),
                            None => true,
                        })
                    }
                    None => true,
                }
            }
        }
    }

    /// Is every dictionary a `class` dictionary for `ty` would be composed
    /// from in scope? The side-effect-free twin of `build_dict_expr`'s
    /// decision tree, so a use is routed through evidence exactly when the
    /// evidence can be built: a type variable needs the body's parameter
    /// for (class, variable) — or the body's parameter for the class — an
    /// existential skolem needs a captured dictionary for the class, a
    /// ground type is always buildable (a static dictionary), a structural
    /// shape of Eq/Ord/Show needs its elements', and a parameterized
    /// instance needs what its context demands of the bound arguments.
    fn dict_evidence_for(&self, class: &str, ty: &Ty, ev: &DictEnv) -> bool {
        match ty {
            Ty::Var(v) => ev.env.contains_key(&(class.to_string(), v.name.clone()))
                || ev.class_to_dict.contains_key(class),
            Ty::Skolem(_, id) => self.skolem_dict_for(class, *id).is_some(),
            _ if ty.free_vars().is_empty() && !Self::contains_skolem(ty) => true,
            _ => {
                // A compound binding (`Rep a`) that IS one of the enclosing
                // function's constraints (`GEncode (Rep a)`) has the
                // dictionary PARAMETER standing for it (`dict_method_use`).
                if self.cur_dict_by_arg.iter()
                    .any(|(cls, arg, _)| cls == class && Self::arg_matches(arg, ty))
                {
                    return true;
                }
                if Self::is_structural_class(class)
                    && let Some((_, elems)) = Self::structural_shape(ty)
                {
                    return elems.iter().all(|e| self.dict_evidence_for(class, e, ev));
                }
                let Some(head) = InstHead::of(ty) else { return false };
                let Some(inst) = self.instances.get(&(class.to_string(), head)) else {
                    return false;
                };
                match &inst.context {
                    Some(ctx) => {
                        let mut bind: HashMap<String, Ty> = HashMap::new();
                        Self::collect_subst_by_name(&inst.target_type, ty, &mut bind);
                        ctx.iter().all(|c| match bind.get(&c.type_var) {
                            Some(sub) => self.dict_evidence_for(&c.class_name, sub, ev),
                            None => false,
                        })
                    }
                    // A builtin instance registered without a context: its
                    // head-keyed static dictionary.
                    None => true,
                }
            }
        }
    }

    /// The type-erased best effort for a method use whose binding is still
    /// polymorphic and will never see evidence (step 5 of
    /// `dispatch_method`): the structural shape's own implementation with
    /// erased element dispatch (`__mll_eq`/`__mll_cmp` for the element,
    /// which is right for scalars) — `==` on a `[a]`, `compare` on a
    /// `Maybe a` — or the instance for the binding's head constructor
    /// (`==` on a `Tree a` field inside the generic derived `eq_Tree`, whose
    /// instance choice depends only on the head). `None` leaves the node as
    /// written, codegen's erased lowering.
    fn erased_method_impl(&mut self, method: &str, binding: &Ty) -> Option<String> {
        match method {
            "==" => if let Some(name) = self.structural_eq_impl(binding) {
                return Some(name);
            },
            "/=" => if let Some(name) = self.structural_ne_impl(binding) {
                return Some(name);
            },
            "compare" | "<" | "<=" | ">" | ">=" | "max" | "min" =>
                if let Some(name) = self.structural_ord_impl(method, binding) {
                    return Some(name);
                },
            _ => {}
        }
        let head = InstHead::of(binding)?;
        self.instance_methods.get(&(method.to_string(), head)).cloned()
    }

    /// Resolve the implementation of `method` for a CONCRETE class-variable
    /// binding. `use_ty` is the full method type at the use site, needed to
    /// specialize a parameterized instance's still-polymorphic method body.
    ///
    /// Resolution order: implementations this pass already generated for the
    /// exact type; structural implementations for compiler-owned shapes
    /// (lists, tuples, Maybe); then the instance registry by head constructor.
    fn resolve_at_type(&mut self, method: &str, binding: &Ty, use_ty: &Ty) -> Option<String> {
        if let Some(existing) = self.generated_impls.get(&(method.to_string(), binding.clone())) {
            return Some(existing.clone());
        }
        match method {
            "==" => {
                if let Some(name) = self.structural_eq_impl(binding) {
                    return Some(name);
                }
            }
            "/=" => {
                if let Some(name) = self.structural_ne_impl(binding) {
                    return Some(name);
                }
            }
            "show" => {
                if let Ty::Tuple(elem_tys) = binding {
                    return Some(self.generate_tuple_show(&elem_tys.clone()));
                }
                if let Some(name) = self.generate_container_show(binding) {
                    return Some(name);
                }
            }
            "showsPrec" => {
                if let Some(name) = self.structural_shows_prec_impl(binding) {
                    return Some(name);
                }
            }
            "compare" | "<" | "<=" | ">" | ">=" | "max" | "min" => {
                if let Some(name) = self.structural_ord_impl(method, binding) {
                    return Some(name);
                }
            }
            // Hashable's hidden methods at a structural key shape: the
            // generated encoder, and the A16 compare the writes stamp on
            // the handle (the scalar instances map them to the runtime
            // scalar encoder and the scalar `compare` below).
            "__hashKey" => {
                if Self::is_structural_key(binding) {
                    return Some(self.structural_key_enc(binding));
                }
            }
            "__hashCompare" => {
                if let Some(name) = self.structural_ord_impl("compare", binding) {
                    return Some(name);
                }
            }
            _ => {}
        }
        let head = InstHead::of(binding)?;
        let mangled = self.instance_methods.get(&(method.to_string(), head))?.clone();
        // A parameterized instance's method is itself polymorphic (e.g.
        // ord_compare__Pair, a derived show_Box); its body resolves class
        // methods on FIELDS only once specialized at this use's concrete type.
        if method == "show" {
            Some(self.specialize_derived_show(&mangled, binding))
        } else if mangled != method {
            Some(self.specialize_instance_fn(&mangled, use_ty))
        } else {
            // Resolves to itself (e.g. IO's `>>=`): nothing to rewrite.
            Some(mangled)
        }
    }

    /// The structural `==` for a compiler-owned shape — list, Maybe, tuple —
    /// generated (or memoized) by element type; `None` for every other type.
    /// Shared by the concrete resolver and the polymorphic-binding operator
    /// dispatch, which once carried the same three-arm block each.
    /// The eight hm* operations whose behavior depends on the KEY layout
    /// (A17). `hmSize` counts entries either way and `hmEmpty` is the shared
    /// empty table (the first insert stamps the flavor), so neither is here.
    fn hm_key_op(name: &str) -> Option<&'static str> {
        ["hmInsert", "hmLookup", "hmDelete", "hmMember", "hmFromList",
         "hmKeys", "hmValues", "hmToList"]
            .into_iter()
            .find(|op| *op == name)
    }

    /// The key type of an hm operation, read off its INSTANTIATED type: the
    /// first parameter for the keyed ops, the pair's first component for
    /// `hmFromList`, the map type's first argument for the iteration ops.
    fn hm_key_type(op: &str, fn_ty: &Ty) -> Option<Ty> {
        let Ty::Arrow(first, ..) = fn_ty else { return None };
        match op {
            "hmInsert" | "hmLookup" | "hmDelete" | "hmMember" => Some((**first).clone()),
            "hmFromList" => match first.as_ref() {
                Ty::List(pair) => match pair.as_ref() {
                    Ty::Tuple(kv) if kv.len() == 2 => Some(kv[0].clone()),
                    _ => None,
                },
                _ => None,
            },
            _ => match first.as_ref() {
                // HashMap k v = App(App(Con "HashMap", k), v)
                Ty::App(hk, _) => match hk.as_ref() {
                    Ty::App(h, k) if matches!(h.as_ref(), Ty::Con(n) if n == "HashMap") =>
                        Some((**k).clone()),
                    _ => None,
                },
                _ => None,
            },
        }
    }

    /// A key type the scalar hashmap path cannot serve: its Lua value is a
    /// table (identity semantics), so it goes through the encoded-entry
    /// variants. Exactly the shapes the structural solve admits.
    fn is_structural_key(ty: &Ty) -> bool {
        matches!(ty, Ty::List(_) | Ty::Tuple(_)) || Self::is_maybe_type(ty)
    }

    /// The injective string encoder for structural key type `ty` (A17):
    /// composites thread element encoders exactly as ListEq threads element
    /// eqs; scalars are the runtime `__mll_key_scalar`. Memoized as the
    /// implementation of Hashable's `__hashKey` at `ty`, so a static
    /// Hashable dictionary for the type finds it.
    fn structural_key_enc(&mut self, ty: &Ty) -> String {
        match ty {
            Ty::List(elem) => {
                let list_ty = ty.clone();
                let elem = (**elem).clone();
                self.synthetic_spec_fn("__hashKey", "hashkey_", &list_ty, &["_k"],
                    Ty::Con("String".into()),
                    move |slf| SpecKind::KeyEncList(slf.elem_key_enc(&elem)))
            }
            m if Self::is_maybe_type(m) => {
                let inner = Self::maybe_inner_type(m).unwrap();
                let maybe_ty = m.clone();
                self.synthetic_spec_fn("__hashKey", "hashkey_", &maybe_ty, &["_k"],
                    Ty::Con("String".into()),
                    move |slf| SpecKind::KeyEncMaybe(slf.elem_key_enc(&inner)))
            }
            Ty::Tuple(elems) => {
                let tuple_ty = ty.clone();
                let elems = elems.clone();
                self.synthetic_spec_fn("__hashKey", "hashkey_", &tuple_ty, &["_k"],
                    Ty::Con("String".into()),
                    move |slf| {
                        let names: Vec<String> =
                            elems.iter().map(|e| slf.elem_key_enc(e)).collect();
                        SpecKind::KeyEncTuple(names)
                    })
            }
            _ => "__mll_key_scalar".to_string(),
        }
    }

    /// The element encoder of a composite key: the shape's own for a
    /// nested shape, the scalar encoder otherwise — including an ERASED
    /// element (a type variable no evidence reaches: a literal-keyed
    /// `hmFromList [((0, 1), …)]` whose element variables the checker
    /// never fixed), the same degradation the erased `__mll_eq`/`__mll_cmp`
    /// fallbacks define for erased scalars. An erased element in a body
    /// that HAS evidence never comes here: `hm_op_use` routes such a key
    /// through its composed Hashable dictionary.
    fn elem_key_enc(&mut self, ty: &Ty) -> String {
        if Self::is_structural_key(ty) {
            self.structural_key_enc(ty)
        } else {
            "__mll_key_scalar".to_string()
        }
    }

    /// The hm* operations with no key argument: they enumerate a map and
    /// need no evidence — the handle carries the flavor and the key order.
    fn hm_iteration_op(op: &str) -> bool {
        matches!(op, "hmKeys" | "hmValues" | "hmToList")
    }

    /// The ONE decision for an hm* use at key type `key_ty` — the HashMap
    /// analog of `dispatch_method`, made by the main walk and the
    /// dictionary rewrite alike. `None` keeps the node as written: the
    /// scalar runtime family (a scalar key, or an erased key type nothing
    /// will ever give evidence for — the type-erased degradation, whose
    /// runtime tripwire rejects a table key loudly), or, inside a body
    /// about to be rewritten to dictionary form, a deferral to that
    /// rewrite. Otherwise:
    ///
    ///   - a keyless enumerator at a structural shape or an erased key
    ///     takes the threaded wrapper (`hm_threaded_impl`), typed for the
    ///     shape, flavor-from-the-handle for the erased key;
    ///   - a keyed operation whose key type needs evidence (a type
    ///     variable or an existential skolem anywhere in it) and has it in
    ///     scope takes the operation's dictionary form applied to the key
    ///     type's Hashable dictionary (the parameter, a captured
    ///     existential dictionary, or a dictionary composed over the
    ///     elements' — `build_dict_expr`), which reads the encoder and the
    ///     compare off it at run time;
    ///   - a keyed operation at a structural shape that needs no evidence
    ///     takes the threaded wrapper with the shape's generated encoder.
    ///
    /// Before this, a keyed operation at an erased key type went to a
    /// runtime family that guessed the flavor per key and encoded a table
    /// key with a TYPE-ERASED walk over its raw slots — an encoding that
    /// agreed with the typed one for tuples of scalars only, so a map
    /// written by a dictionary-form body (the Data.Map wrappers past the
    /// specialization limit) could not be read by a specialized one, and
    /// a lazy list key's generator slot reached the scalar encoder (H2,
    /// H3).
    fn hm_op_use(&mut self, op: &str, key_ty: &Ty, use_ty: &Ty, ev: &DictEnv) -> Option<TExpr> {
        let wrapper = |slf: &mut Self| {
            let mangled = slf.hm_threaded_impl(op, key_ty, use_ty);
            Some(TExpr::new(TExprKind::Var(mangled), use_ty.clone()))
        };
        if Self::hm_iteration_op(op) {
            if Self::is_structural_key(key_ty) || matches!(key_ty, Ty::Var(_) | Ty::Skolem(..)) {
                return wrapper(self);
            }
            return None;
        }
        if self.needs_evidence("Hashable", key_ty) {
            if self.dict_evidence_for("Hashable", key_ty, ev) {
                let dict = self.build_dict_expr("Hashable", key_ty, ev);
                let dictform = self.hm_dictform(op);
                let head = TExpr::new(
                    TExprKind::Var(dictform),
                    Ty::fun(&[Ty::Unit], use_ty.clone()),
                );
                return Some(TExpr::new(
                    TExprKind::App(Box::new(head), Box::new(dict)),
                    use_ty.clone(),
                ));
            }
            if self.in_dictform {
                return None;
            }
        }
        if Self::is_structural_key(key_ty) {
            return wrapper(self);
        }
        None
    }

    /// The dictionary form of keyed hm operation `op`, generated once (it
    /// is generic over the key type): `op·dictform(__d0, _h0, …)` calls the
    /// runtime's dictionary-routed `__mll_hmd_<op>` with the Hashable
    /// dictionary's encoder (and, for the writes, its compare — the
    /// handle is stamped with it) ahead of the value parameters. A use is
    /// this function applied to the dictionary, so a saturated call
    /// flattens to one N-ary call and a first-class use is an ordinary
    /// partial application.
    fn hm_dictform(&mut self, op: &str) -> String {
        let n_params = match op {
            "hmInsert" => 3,
            "hmLookup" | "hmDelete" | "hmMember" => 2,
            "hmFromList" => 1,
            other => unreachable!("hm_dictform: {other} is not a keyed hm operation"),
        };
        let params: Vec<String> = (0..n_params).map(|i| format!("_h{i}")).collect();
        let param_refs: Vec<&str> = params.iter().map(String::as_str).collect();
        let key = format!("{op}__dictform");
        let base = format!("{op}_dictform");
        let suffix = op.strip_prefix("hm").expect("hm op name");
        let runtime = format!("__mll_hmd_{}{}", suffix[..1].to_lowercase(), &suffix[1..]);
        let stamps = matches!(op, "hmInsert" | "hmFromList");
        let params_owned = params.clone();
        self.structural_dictform((&key, &Ty::Unit), &base, 1, &param_refs, Ty::Unit, move |dicts| {
            let var = |n: &str| TExpr::new(TExprKind::Var(n.to_string()), Ty::Unit);
            let app = |f: TExpr, a: TExpr| TExpr::new(
                TExprKind::App(Box::new(f), Box::new(a)), Ty::Unit);
            let mut call = app(var(&runtime), Self::dictform_elem_method(&dicts[0], "__hashKey"));
            if stamps {
                call = app(call, Self::dictform_elem_method(&dicts[0], "__hashCompare"));
            }
            for p in &params_owned {
                call = app(call, var(p));
            }
            call
        })
    }

    /// The threaded wrapper for hm operation `op` at key type `key_ty` — a
    /// structural shape, or (for the keyless enumerators only) an erased
    /// key — declared at the use's full instantiated type (so the N-ary
    /// convention holds at every call and partial application). The keyed
    /// ops carry the shape's encoder, the writes also its A16 structural
    /// compare (stamped on the handle, so the enumerators come back in
    /// true Ord order from any body); the enumerators carry nothing.
    fn hm_threaded_impl(&mut self, op: &str, key_ty: &Ty, use_ty: &Ty) -> String {
        // Erased element variables reach this path (see `elem_key_enc`);
        // canonicalize so every use of one shape shares one wrapper,
        // exactly as the arity-widening specializations do.
        let use_ty = &if self.is_polymorphic(use_ty) {
            Self::canonicalize_vars(use_ty)
        } else {
            use_ty.clone()
        };
        let key_ty = &if self.is_polymorphic(key_ty) {
            Self::canonicalize_vars(key_ty)
        } else {
            key_ty.clone()
        };
        let mangled = match self.memo_or_mangle(op, &format!("{}_", op), use_ty) {
            Ok(existing) => return existing,
            Err(fresh) => fresh,
        };
        // An erased key (a type variable or skolem no evidence reaches)
        // only ever comes here for an enumerator (see `hm_op_use`): the
        // runtime reads the flavor off the handle.
        let erased = matches!(key_ty, Ty::Var(_) | Ty::Skolem(..));
        let (enc, cmp) = if erased || Self::hm_iteration_op(op) {
            (None, None)
        } else if matches!(op, "hmInsert" | "hmFromList") {
            (Some(self.structural_key_enc(key_ty)), self.structural_ord_impl("compare", key_ty))
        } else {
            (Some(self.structural_key_enc(key_ty)), None)
        };
        // Parameter types come from peeling the instantiated arrows — hm
        // signatures are heterogeneous, so synthetic_spec_fn's same-type
        // parameter list does not fit here.
        let mut param_tys = Vec::new();
        let mut rest = use_ty;
        while let Ty::Arrow(a, b, _) = rest {
            param_tys.push((**a).clone());
            rest = b;
        }
        let result_ty = rest.clone();
        let param_names: Vec<String> =
            (0..param_tys.len()).map(|i| format!("_h{}", i)).collect();
        let args: Vec<TExpr> = param_names.iter().zip(&param_tys)
            .map(|(p, t)| TExpr::new(TExprKind::Var(p.clone()), t.clone()))
            .collect();
        let body = TExpr::new(
            TExprKind::SpecCall {
                original: mangled.clone(),
                specialized: SpecKind::HmOp { op: op.to_string(), enc, cmp, erased },
                args,
            },
            result_ty.clone(),
        );
        let func = TFunction {
            name: mangled.clone(),
            ty: use_ty.clone(),
            clauses: vec![TClause {
                span: None,
                patterns: param_names.iter()
                    .map(|p| TPattern::Var(p.clone(), Ty::Unit))
                    .collect(),
                guards: vec![],
                body: Some(body),
                where_binds: vec![],
            }],
            specialized: true,
            spec_origin: None,
            dict_params: vec![],
            derived_strict: false,
        };
        self.generated.push(func);
        mangled
    }

    /// The structural Ord family for a compiler-owned shape (A16), the
    /// compare analog of `structural_eq_impl`: `compare` is generated per
    /// shape (lexicographic lists, Nothing < Just, element-wise tuples),
    /// and every other Ord method derives from it (`OrdFromCmp` — the
    /// Ordering result is its constructor index, so the operators are
    /// integer tests on the compare result, and max/min select an operand).
    fn structural_ord_impl(&mut self, method: &str, binding: &Ty) -> Option<String> {
        let cmp = match binding {
            Ty::List(elem_ty) => self.generate_list_cmp(&elem_ty.clone()),
            m if Self::is_maybe_type(m) => {
                let inner = Self::maybe_inner_type(m).unwrap();
                self.generate_maybe_cmp(&inner)
            }
            Ty::Tuple(elem_tys) => self.generate_tuple_cmp(&elem_tys.clone()),
            _ => return None,
        };
        if method == "compare" {
            return Some(cmp);
        }
        let (base, result_ty) = match method {
            "<" => ("ord_lt_", Ty::Con("Bool".into())),
            "<=" => ("ord_le_", Ty::Con("Bool".into())),
            ">" => ("ord_gt_", Ty::Con("Bool".into())),
            ">=" => ("ord_ge_", Ty::Con("Bool".into())),
            "max" => ("ord_max_", binding.clone()),
            "min" => ("ord_min_", binding.clone()),
            _ => return None,
        };
        let op = method.to_string();
        Some(self.synthetic_spec_fn(method, base, binding, &["_a", "_b"], result_ty,
            move |_| SpecKind::OrdFromCmp { op, cmp }))
    }

    fn generate_list_cmp(&mut self, elem_ty: &Ty) -> String {
        let list_ty = Ty::List(Box::new(elem_ty.clone()));
        let elem_ty = elem_ty.clone();
        self.synthetic_spec_fn("compare", "ord_compare_", &list_ty, &["_a", "_b"],
            Ty::Con("Ordering".into()),
            move |slf| SpecKind::ListCmp(slf.resolve_elem_cmp(&elem_ty)))
    }

    fn generate_maybe_cmp(&mut self, inner_ty: &Ty) -> String {
        let maybe_ty = Ty::app(Ty::Con("Maybe".into()), inner_ty.clone());
        let inner_ty = inner_ty.clone();
        self.synthetic_spec_fn("compare", "ord_compare_", &maybe_ty, &["_a", "_b"],
            Ty::Con("Ordering".into()),
            move |slf| SpecKind::MaybeCmp(slf.resolve_elem_cmp(&inner_ty)))
    }

    fn generate_tuple_cmp(&mut self, elem_tys: &[Ty]) -> String {
        let tuple_ty = Ty::Tuple(elem_tys.to_vec());
        let elem_tys = elem_tys.to_vec();
        self.synthetic_spec_fn("compare", "ord_compare_", &tuple_ty, &["_a", "_b"],
            Ty::Con("Ordering".into()),
            move |slf| {
                let names: Vec<String> = elem_tys.iter()
                    .map(|et| slf.resolve_elem_cmp(et))
                    .collect();
                SpecKind::TupleCmp(names)
            })
    }

    /// Element comparator, mirroring `resolve_elem_eq`: the concrete
    /// `compare` where the type resolves one, `__mll_cmp` (the type-erased
    /// runtime fallback) at a genuinely polymorphic position.
    fn resolve_elem_cmp(&mut self, ty: &Ty) -> String {
        if matches!(ty, Ty::Var(_)) {
            return "__mll_cmp".to_string();
        }
        let cmp_use_ty = Ty::fun(&[ty.clone(), ty.clone()], Ty::Con("Ordering".into()));
        match self.resolve_at_type("compare", ty, &cmp_use_ty) {
            Some(name) => name,
            None => "__mll_cmp".to_string(),
        }
    }

    fn structural_eq_impl(&mut self, binding: &Ty) -> Option<String> {
        if let Ty::List(elem_ty) = binding {
            return Some(self.generate_list_eq(&elem_ty.clone()));
        }
        if Self::is_maybe_type(binding) {
            let inner = Self::maybe_inner_type(binding).unwrap();
            return Some(self.generate_maybe_eq(&inner));
        }
        if let Ty::Tuple(elem_tys) = binding {
            return Some(self.generate_tuple_eq(&elem_tys.clone()));
        }
        None
    }

    /// The structural `/=` for a compiler-owned shape: GHC's class default
    /// (`not (a == b)`) over the structural `==` of the same shape, one
    /// generated function per shape (`SpecKind::NotEq`).
    fn structural_ne_impl(&mut self, binding: &Ty) -> Option<String> {
        if !(matches!(binding, Ty::List(_) | Ty::Tuple(_)) || Self::is_maybe_type(binding)) {
            return None;
        }
        let eq = self.structural_eq_impl(binding)?;
        Some(self.synthetic_spec_fn("/=", "ne", binding, &["_a", "_b"], Ty::Con("Bool".into()),
            move |_| SpecKind::NotEq(eq)))
    }

    /// The structural `showsPrec` for a compiler-owned shape (list, Maybe,
    /// tuple): the runtime precedence rule (`__mll_shows_prec`) over the
    /// shape's typed `show` — the same rule every builtin and derived
    /// instance's showsPrec applies, so `showsPrec 11 (Just 1)` is
    /// "(Just 1)" and a list or tuple is never parenthesized.
    fn structural_shows_prec_impl(&mut self, binding: &Ty) -> Option<String> {
        if !(matches!(binding, Ty::List(_) | Ty::Tuple(_)) || Self::is_maybe_type(binding)) {
            return None;
        }
        let show_use_ty = Ty::arrow(binding.clone(), Ty::Con("String".into()));
        let show = self.resolve_at_type("show", binding, &show_use_ty)?;
        let str_ty = Ty::Con("String".into());
        Some(self.synthetic_spec_fn_typed(
            "showsPrec",
            "showsPrec",
            binding,
            &[("_d", Ty::Con("Int".into())), ("_x", binding.clone()), ("_s", str_ty.clone())],
            str_ty,
            move |_| SpecKind::ShowsPrecOf(show),
        ))
    }

    /// True if the type contains a rigid skolem anywhere — dispatch on those
    /// is deferred, exactly like a type variable (rank-2 bodies).
    fn contains_skolem(ty: &Ty) -> bool {
        match ty {
            Ty::Skolem(..) => true,
            Ty::Con(_) | Ty::Var(_) | Ty::Unit | Ty::Promoted(_) => false,
            Ty::Arrow(a, b, _) | Ty::App(a, b) => Self::contains_skolem(a) || Self::contains_skolem(b),
            Ty::List(a) | Ty::IO(a) | Ty::LuaIO(_, a) | Ty::Forall(_, a) => Self::contains_skolem(a),
            Ty::Tuple(elems) => elems.iter().any(Self::contains_skolem),
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

    /// Produce the unique function name for `base` at `ty`. The readable
    /// `ty_to_suffix` encoding is kept, but uniqueness is guaranteed through
    /// `mangled_names`: when a DIFFERENT (base, type) pair already owns the
    /// candidate name, a deterministic "__2"/"__3"… disambiguator is appended
    /// (generation order is deterministic, so so is the numbering). That
    /// table knows the names THIS pass handed out; what keeps a generated
    /// name apart from every name of the program is its spelling
    /// (`ast::minted`: `count·IntTInt`), which no source name has.
    fn mangle_name(&mut self, base: &str, ty: &Ty) -> String {
        let stem = crate::ast::minted(base, &self.ty_to_suffix(ty));
        let mut candidate = stem.clone();
        let mut k = 1u32;
        loop {
            match self.mangled_names.get(&candidate) {
                None => {
                    self.mangled_names
                        .insert(candidate.clone(), (base.to_string(), ty.clone()));
                    return candidate;
                }
                Some((b, t)) if b == base && t == ty => return candidate,
                Some(_) => {
                    k += 1;
                    candidate = crate::ast::minted(&format!("{}_", stem), &k.to_string());
                }
            }
        }
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
        if self.is_polymorphic(arg_ty) {
            return base.to_string();
        }
        let fn_ty = Ty::arrow(arg_ty.clone(), Ty::Con("String".into()));
        self.specialize_instance_fn(base, &fn_ty)
    }

    /// Specialize a still-polymorphic instance method (derived or user-written,
    /// e.g. `eq_Box`, `ord_compare__Pair`) at the concrete function type it is
    /// used at. Instance resolution alone only picks the FUNCTION — for a
    /// parameterized instance its body still contains class methods at the
    /// instance's type VARIABLES (`compare _a0 _b0` at type `a`), which the
    /// polymorphic original can never resolve. Cloning it per concrete use
    /// type and re-running monomorphization resolves each field's method at
    /// its concrete type — recursively. Returns `base` unchanged when there is
    /// nothing to specialize (not polymorphic, use type not concrete, or the
    /// polymorphic-recursion guard trips).
    fn specialize_instance_fn(&mut self, base: &str, fn_ty: &Ty) -> String {
        if self.is_polymorphic(fn_ty) || !self.poly_fns.contains_key(base) {
            return base.to_string();
        }
        // Past the cap, the shared form serves every caller: the composed
        // dictionary form of a constrained method, the generic copy of a
        // constraint-free one. Returning the raw original once the cap
        // tripped WITHOUT the switch (the old behavior) left the later
        // types calling a body whose still-polymorphic method uses resolve
        // to nothing at runtime — a generic function (gSum over the C1 of
        // each deriving constructor, gFields over each S1 field, …) died
        // with a nil call as soon as the program passed the cap.
        if self.arity_only_fns.contains(base) {
            return base.to_string();
        }
        // A dictionary-passing method at a ground type is its stub: a
        // plain function of that type, right wherever the resolved name
        // lands — a structural thread carries it by NAME, which no
        // call-site rewrite reaches, and the bare dictionary form there
        // took the element as its dictionary (`[Box 1] == [Box 1]` after
        // `==` at Box tripped: "attempt to index a nil value").
        if self.dict_passing_fns.contains(base) {
            return self.ground_form_or_base(base, fn_ty);
        }
        if let Some(spec) = self.specialize_at(base, fn_ty) {
            return spec;
        }
        // The request tripped the cap — this instantiation is the one that
        // crossed it — or found the shape copies exhausted: whatever
        // serves the method now.
        self.ground_form_or_base(base, fn_ty)
    }

    /// The name a dictionary-passing method resolves to at `fn_ty`: its
    /// stub (`dict_stub`) at a ground type; the bare name at a type over
    /// an existential's hidden type, whose dictionary is the captured
    /// binder — in scope at the use site only, where the call-site
    /// rewrite builds the call — and for a shared, constraint-free
    /// function, whose generic copy is a plain function already.
    fn ground_form_or_base(&mut self, base: &str, fn_ty: &Ty) -> String {
        if self.dict_passing_fns.contains(base) && !Self::contains_skolem(fn_ty) {
            return self.dict_stub(base, fn_ty);
        }
        base.to_string()
    }

    /// The value of dictionary-passing `base` at ground `spec_ty` as a
    /// named function — the name its specialization at that type had or
    /// would have had (`mangle_name` is stable per (base, type)):
    /// `base·ty(p1, …, pn) = base(dicts…, p1, …, pn)`, the dictionaries
    /// demanded for the type (decided after `run`'s fixpoint). Generated
    /// once per name; a name under which the specialization itself was
    /// emitted before the trip is that copy (`ground_forms`).
    fn dict_stub(&mut self, base: &str, spec_ty: &Ty) -> String {
        let mangled = self.mangle_name(base, spec_ty);
        if !self.ground_forms.insert(mangled.clone()) {
            return mangled;
        }
        let mut params: Vec<(String, Ty)> = Vec::new();
        let mut rest = spec_ty;
        while let Ty::Arrow(from, to, _) = rest {
            params.push((format!("_a{}", params.len()), (**from).clone()));
            rest = to;
        }
        let args: Vec<TExpr> = params.iter()
            .map(|(p, pty)| TExpr::new(TExprKind::Var(p.clone()), pty.clone()))
            .collect();
        let body = self.dictify_use(base, args, rest.clone());
        self.generated.push(TFunction {
            name: mangled.clone(),
            ty: spec_ty.clone(),
            clauses: vec![TClause {
                span: None,
                patterns: params.iter()
                    .map(|(p, _)| TPattern::Var(p.clone(), Ty::Unit))
                    .collect(),
                guards: vec![],
                body: Some(body),
                where_binds: vec![],
            }],
            specialized: true,
            spec_origin: None,
            dict_params: vec![],
            derived_strict: false,
        });
        mangled
    }

    /// The specialization of `name` at `spec_ty`: the existing one, or a
    /// freshly generated copy — `None` when the request trips the cap
    /// (`trip_spec_cap`), in which case the use stays by name. The one path
    /// every specialization request takes (a use in the main walk, an
    /// instance method, the regeneration of a widening copy at a trip), so
    /// the insert-before-generate discipline — a recursive use inside the
    /// body finds this same copy instead of looping — lives in one place.
    fn specialize_at(&mut self, name: &str, spec_ty: &Ty) -> Option<String> {
        let key = SpecKey { name: name.to_string(), ty: spec_ty.clone() };
        if let Some(existing) = self.specializations.get(&key) {
            return Some(existing.clone());
        }
        if self.trip_spec_cap(name) {
            return None;
        }
        let mangled = self.mangle_name(name, spec_ty);
        // `mangle_name` is stable per (base, type): a copy regenerated at
        // the key of a purged one gets the purged name back, and the
        // references made to that name before the trip are right for it
        // (same key, same code) — they must stay, not be redirected.
        self.purged_specs.remove(&mangled);
        self.specializations.insert(key, mangled.clone());
        self.generate_specialization(name, &mangled, spec_ty);
        Some(mangled)
    }

    /// True when `name` has just tripped the per-function specialization
    /// cap: more than `SPEC_LIMIT` specializations exist and another
    /// distinct instantiation is wanted — a polymorphic-recursion symptom (each
    /// specialization demands another at a bigger type) or simply one
    /// genuine instantiation too many. Purges exactly the specializations
    /// OF THIS function — identified by their recorded mangled names, never
    /// by name prefix, which would also delete specializations of a sibling
    /// `foo_bar` when `foo` trips the limit — and switches the function to
    /// the form that serves any number of instantiations: dictionary
    /// passing for a constrained function; for a constraint-free one, which
    /// has no dictionary form, its retained generic copy, shared the way a
    /// prelude builtin's is (`arity_only_fns`).
    ///
    /// Neither shared form can serve a use that widens the function's
    /// arity (see `arity_only_fns`), so every purged widening
    /// specialization is replaced by a copy at its arrow shape
    /// (`widening_copy`) — one per shape, not per type — and a call site
    /// already rewritten to a purged name is pointed, in `run` via
    /// `purged_specs`, at whatever now serves it: the generic copy, or
    /// that shape copy. A purged copy of a CONSTRAINED function keeps its
    /// name instead: the copy itself where it was already emitted, the
    /// stub of its type (`dict_stub`) where it was still pending or under
    /// generation — a reference to a dictionary-passing function is right
    /// only at a call site the rewrite reaches, and the references made
    /// before the trip sit anywhere (a structural thread's element
    /// function, a first-class use), so the name must stay a function of
    /// the copy's type.
    ///
    /// A function that is already shared or dictionary-passing (a builtin,
    /// or past its trip) can only grow shape copies, and more than
    /// `SPEC_LIMIT` distinct widening shapes means a polymorphic recursion
    /// that widens the arity at every level: no finite set of copies serves
    /// it and nothing is left to fall back to, so it is reported (once) and
    /// no further copy is generated — the trip cannot loop.
    fn trip_spec_cap(&mut self, name: &str) -> bool {
        if self.arity_only_fns.contains(name) || self.dict_passing_fns.contains(name) {
            // Already shared: the only copies it can still want are the
            // arrow-shape copies of its widening uses (counted apart from
            // any specialization that predates a flag without a trip).
            let shapes = self.specializations.keys()
                .filter(|k| k.name == name && self.arity_widening(name, &k.ty))
                .count();
            if shapes <= SPEC_LIMIT {
                return false;
            }
            self.push_arity_shape_limit_error(name);
            return true;
        }
        let count = self.specializations.keys().filter(|k| k.name == name).count();
        if count <= SPEC_LIMIT {
            return false;
        }
        // The count at trip time — the existing copies plus the
        // instantiation that crossed the cap — for the first-class-use
        // diagnostic: it tells the user WHICH way the cap was crossed — a
        // runaway ladder (polymorphic recursion) or simply an 18th genuine
        // instantiation — and that adding an instantiation elsewhere is
        // what broke this unrelated-looking use site.
        self.dict_passing_counts.insert(name.to_string(), count + 1);
        let shared = self.fn_constraints.get(name).is_none();
        if shared {
            self.arity_only_fns.insert(name.to_string());
        } else {
            self.dict_passing_fns.insert(name.to_string());
        }
        let mut purged: Vec<(SpecKey, String)> = self.specializations.iter()
            .filter(|(k, _)| k.name == name)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        self.specializations.retain(|k, _| k.name != name);
        let purged_names: HashSet<String> = purged.iter().map(|(_, v)| v.clone()).collect();
        // A copy still pending (or under generation, on `gen_stack`: its
        // generation drops it) is gone; one already emitted stays — it is
        // correct code for its type, and `run` rewrites its call sites
        // like any function's.
        let mut gone: HashSet<String> = self.generated.iter()
            .filter(|f| purged_names.contains(&f.name))
            .map(|f| f.name.clone())
            .collect();
        gone.extend(self.gen_stack.iter()
            .filter(|(_, m)| purged_names.contains(m))
            .map(|(_, m)| m.clone()));
        self.generated.retain(|f| !purged_names.contains(&f.name));
        // Regenerated in a fixed order: the shape copies and stubs are
        // emitted in the order they are generated, and the HashMap's order
        // is not stable.
        purged.sort_by(|a, b| a.1.cmp(&b.1));
        for (key, mangled) in purged {
            if self.arity_widening(name, &key.ty) {
                let target = self.widening_copy(name, &key.ty).unwrap_or_else(|| name.to_string());
                if target != mangled {
                    self.purged_specs.insert(mangled, target);
                }
            } else if shared {
                self.purged_specs.insert(mangled, name.to_string());
            } else if gone.contains(&mangled) {
                // A constrained function's copy keeps its name: the stub
                // of its type, which passes the dictionaries — every
                // reference made before the trip stays right, in
                // whatever position it sits (`ground_forms`).
                self.dict_stub(name, &key.ty);
            } else {
                self.ground_forms.insert(mangled);
            }
        }
        true
    }

    /// The copy that serves a use of shared or dictionary-passing `name` at
    /// arity-widening `use_ty`: the copy at the use's arrow shape
    /// (`arity_shape`), generated on first demand. For a shared function it
    /// is an ordinary specialization at the shape. For a dictionary-passing
    /// function it is a dictionary-passing function in its own right at the
    /// shape — the original's constraints over the original's variables,
    /// which the shape keeps — so `run`'s worklist rewrites its body to
    /// dictionary form exactly as it does the original's, and a call site
    /// builds the same dictionaries for it (`dictify_use`), saturating its
    /// own arrow count: the one dictionary form at the declared arrow count
    /// cannot serve a widening use any more than a generic copy can (see
    /// `arity_only_fns`). `None` when the shape copies are exhausted
    /// (`trip_spec_cap`).
    fn widening_copy(&mut self, name: &str, use_ty: &Ty) -> Option<String> {
        let shape = self.arity_shape(name, use_ty);
        if !self.dict_passing_fns.contains(name) {
            return self.specialize_at(name, &shape);
        }
        let key = SpecKey { name: name.to_string(), ty: shape.clone() };
        if let Some(existing) = self.specializations.get(&key) {
            return Some(existing.clone());
        }
        if self.trip_spec_cap(name) {
            return None;
        }
        let poly_fn = self.poly_fns.get(name)?.clone();
        let mangled = self.mangle_name(name, &shape);
        self.purged_specs.remove(&mangled);
        self.specializations.insert(key, mangled.clone());
        let subst = Self::compute_body_subst(&poly_fn, &shape);
        let mut copy = poly_fn;
        copy.name = mangled.clone();
        copy.ty = shape;
        copy.clauses = copy.clauses.into_iter()
            .map(|c| c.apply_subst(&subst))
            .collect();
        copy.specialized = true;
        copy.spec_origin = Some(name.to_string());
        if let Some(cs) = self.fn_constraints.get(name).cloned() {
            self.fn_constraints.insert(mangled.clone(), cs);
        }
        if let Some(args) = self.fn_constraint_args.get(name).cloned() {
            self.fn_constraint_args.insert(mangled.clone(), args);
        }
        self.dict_passing_fns.insert(mangled.clone());
        // Its body is not monomorphized here: the dictionary rewrite starts
        // from the pristine copy registered in `poly_fns`, as for every
        // dictionary-passing function. The placeholder pushed to `generated`
        // is what `run`'s worklist finds by name and rewrites in place.
        self.poly_fns.insert(mangled.clone(), copy.clone());
        self.generated.push(copy);
        Some(mangled)
    }

    /// Report, once per function, that `name` wants more than `SPEC_LIMIT`
    /// arrow-shape copies (see `trip_spec_cap`).
    fn push_arity_shape_limit_error(&mut self, name: &str) {
        let msg = format!(
            "'{}' is used at more than {} distinct function-type shapes",
            name, SPEC_LIMIT);
        if self.errors.iter().any(|e| matches!(&e.kind, DiagnosticKind::Other(m) if *m == msg)) {
            return;
        }
        self.errors.push(Diagnostic {
            kind: DiagnosticKind::Other(msg),
            context: self.cur_ctx.clone(),
            span: self.cur_span,
            file: None,
            notes: vec![
                format!(
                    "a use that instantiates a result type variable of '{}' to a \
                     function type needs a copy of it with that many parameters \
                     (mata-ll compiles every function as one Lua function with \
                     one parameter per arrow of its type, and GHC's curried \
                     calls need no such copy); one copy per distinct shape is \
                     kept, up to the specialization limit of {}. Past it there \
                     is nothing to fall back to: the shared generic copy cannot \
                     take the extra arguments.",
                    name, SPEC_LIMIT),
                "this many shapes is almost always a polymorphic recursion whose \
                 result is a function at every level. Restructure the \
                 recursion so its arity does not grow, or wrap the widened \
                 uses in a function with a fixed type.".to_string(),
            ],
            baseline: false, excerpt: None, names: None,
        });
    }

    /// Clone the polymorphic original of `name` at `fn_ty`, substitute its
    /// body types, and monomorphize it under an accurate generation stack (a
    /// still-polymorphic recursive call inside the body then resolves to THIS
    /// specialization). The `mangled` name must already be registered in
    /// `specializations` so recursive uses find it instead of looping.
    fn generate_specialization(&mut self, name: &str, mangled: &str, fn_ty: &Ty) {
        let Some(poly_fn) = self.poly_fns.get(name).cloned() else { return };
        let subst = Self::compute_body_subst(&poly_fn, fn_ty);
        let mut spec_fn = poly_fn;
        spec_fn.name = mangled.to_string();
        spec_fn.ty = fn_ty.clone();
        spec_fn.clauses = spec_fn.clauses.into_iter()
            .map(|c| c.apply_subst(&subst))
            .collect();
        spec_fn.specialized = true;
        spec_fn.spec_origin = Some(name.to_string());
        let tripped_before = self.dict_passing_counts.contains_key(name);
        self.gen_stack.push((name.to_string(), mangled.to_string()));
        let spec_fn = self.mono_function(spec_fn);
        self.gen_stack.pop();
        // The function tripped the cap WHILE this copy was being generated
        // (a polymorphic-recursion ladder trips from inside its own
        // generation): the copy was purged, every reference to it is
        // redirected in `run`, and the shared form — or an arrow-shape copy
        // regenerated under this same name — serves its uses. Emitting it
        // would leave a dead function, or a second definition under the
        // regenerated copy's name.
        if !tripped_before && self.dict_passing_counts.contains_key(name) {
            return;
        }
        self.generated.push(spec_fn);
    }

    fn ty_to_suffix(&self, ty: &Ty) -> String {
        match ty {
            Ty::Con(name) => name.clone(),
            Ty::Var(v) => format!("v{}", v.name),
            Ty::Arrow(a, b, _) => format!("{}T{}", self.ty_to_suffix(a), self.ty_to_suffix(b)),
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

    /// Record a "No instance" error at the clause currently being processed.
    fn push_no_instance(&mut self, method: &str, ty: &Ty) {
        let (msg, hint) = no_instance_parts(method, ty);
        self.errors.push(Diagnostic {
            kind: DiagnosticKind::Other(msg),
            context: self.cur_ctx.clone(),
            span: self.cur_span,
            file: None,
            notes: hint.map(str::to_string).into_iter().collect(),
            baseline: false, excerpt: None, names: None,
        });
    }

    fn mono_function(&mut self, mut func: TFunction) -> TFunction {
        // Name errors after the function the user wrote: a specialization
        // being generated (top of `gen_stack`) reports as its original.
        let display_name = self.gen_stack.last()
            .filter(|(_, mangled)| *mangled == func.name)
            .map(|(base, _)| base.clone())
            .unwrap_or_else(|| func.name.clone());
        let multi_clause = func.clauses.len() > 1;
        // An instance method reports as the instance the user wrote (or the
        // deriving clause that generated it) — never its internal mangled
        // name, which appears nowhere in the source.
        let described = self.instance_fn_ctx.get(&display_name).cloned();
        func.clauses = func.clauses.into_iter().enumerate()
            .map(|(idx, c)| {
                let ctx = match (&described, multi_clause) {
                    (Some(d), _) => d.clone(),
                    (None, true) => format!("clause {} of '{}'", idx + 1, display_name),
                    (None, false) => format!("definition of '{}'", display_name),
                };
                // Save/restore: specialization generation re-enters
                // mono_function from inside an enclosing clause.
                let saved_ctx = self.cur_ctx.replace(ctx);
                let saved_span = std::mem::replace(&mut self.cur_span, c.span);
                let result = self.mono_clause(c);
                self.cur_ctx = saved_ctx;
                self.cur_span = saved_span;
                result
            })
            .collect();
        func
    }

    /// Run `f` with `names` in the local scope, then take out the ones
    /// that were not local before. The ONE shadowing discipline for every
    /// binder — clause patterns, lambda parameters, case patterns, let/
    /// where names and their own parameters, do-block binders: a name in
    /// `locals` is never resolved as (or specialized from) a top-level
    /// polymorphic function or class method of the same name. Binder
    /// sites once each hand-rolled this, and the do-block spine and the
    /// where-bindings had none, so `reverse <- getLine` or a where-bound
    /// `reverse` was rewritten into a specialization of the GLOBAL.
    fn with_locals<T>(
        &mut self,
        names: impl IntoIterator<Item = String>,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let added: Vec<String> = names.into_iter().filter(|n| self.locals.insert(n.clone())).collect();
        let result = f(self);
        for name in &added {
            self.locals.remove(name);
        }
        result
    }

    /// Apply `f` to every top-level expression of `clause` — the same
    /// coverage list as [`TClause::map_exprs`] — with the clause's binders in
    /// `locals`: pattern variables and where-bound names over everything,
    /// each where-binding's own parameters over its body only. The
    /// dictionary rewrite passes walk clauses OUTSIDE `mono_clause`, so they
    /// must reestablish the scope themselves; through the scope-blind
    /// `map_exprs`, `apply gg = gg 1 2` had its PARAMETER call rewritten
    /// into a DictCall on the dict-passing global `gg` (G1).
    fn map_clause_exprs_scoped(
        &mut self,
        clause: &mut TClause,
        f: &mut impl FnMut(&mut Self, TExpr) -> TExpr,
    ) {
        fn take(e: &mut TExpr) -> TExpr {
            std::mem::replace(e, TExpr::new(TExprKind::Lit(TLiteral::Unit), Ty::Unit))
        }
        // The clause's existential captures scope over the same expressions
        // as its binders (the patterns were extended by the main walk; this
        // re-reads their dictionary binders, see `ex_rewrite_pattern`), so
        // a method use the rewrite reaches at a hidden type — one the main
        // walk deferred because its binding ALSO contains this body's class
        // variable — finds the captured dictionary.
        let mut caps = Vec::new();
        clause.patterns = self.ex_rewrite_patterns(std::mem::take(&mut clause.patterns), &mut caps);
        let scope: Vec<String> = clause.patterns.iter().flat_map(TPattern::bound_vars)
            .chain(clause.where_binds.iter().map(|ld| ld.name.clone()))
            .collect();
        self.with_ex_scope(caps, |m| m.with_locals(scope, |m| {
            for g in &mut clause.guards {
                g.condition = f(m, take(&mut g.condition));
                g.body = f(m, take(&mut g.body));
            }
            clause.body = clause.body.take().map(|b| f(m, b));
            for wb in &mut clause.where_binds {
                let params: Vec<String> =
                    wb.patterns.iter().flat_map(TPattern::bound_vars).collect();
                let body = take(&mut wb.body);
                wb.body = m.with_locals(params, |m| f(m, body));
            }
        }));
    }

    /// One level of binder-scoped descent for the dictionary rewrite passes:
    /// rebuild `expr`'s children with `f`, entering lambda parameters, let
    /// group names (each binding's own parameters over its body), and case
    /// pattern variables into `locals` around the children they scope over.
    /// `mono_expr` carries its own (spine-unrolled) copy of this discipline;
    /// the rewrites walk finished TIR after it and previously descended with
    /// the scope-blind `TExpr::map_children`, so a local named like a
    /// dict-passing global was rewritten as the global (G1).
    fn map_children_scoped(
        &mut self,
        expr: TExpr,
        f: &mut impl FnMut(&mut Self, TExpr) -> TExpr,
    ) -> TExpr {
        let ty = expr.ty;
        let kind = match expr.kind {
            TExprKind::Lambda { params, body } => {
                let names: Vec<String> = params.iter().map(|(n, _)| n.clone()).collect();
                let body = self.with_locals(names, |m| f(m, *body));
                TExprKind::Lambda { params, body: Box::new(body) }
            }
            TExprKind::Let { binds, body } => {
                // The group's names scope over every binding body and the
                // let body — one recursive scope, like a where group.
                let group: Vec<String> = binds.iter().map(|b| b.name.clone()).collect();
                self.with_locals(group, |m| {
                    let binds = binds.into_iter().map(|ld| {
                        let mut caps = Vec::new();
                        let patterns = m.ex_rewrite_patterns(ld.patterns, &mut caps);
                        let params: Vec<String> =
                            patterns.iter().flat_map(TPattern::bound_vars).collect();
                        let body = m.with_ex_scope(caps, |m| m.with_locals(params, |m| f(m, ld.body)));
                        TLocalDef { name: ld.name, patterns, body }
                    }).collect();
                    let body = f(m, *body);
                    TExprKind::Let { binds, body: Box::new(body) }
                })
            }
            TExprKind::Case { scrutinee, branches } => {
                let scrutinee = f(self, *scrutinee);
                let branches = branches.into_iter().map(|br| {
                    let mut caps = Vec::new();
                    let pattern = self.ex_rewrite_pattern(br.pattern, &mut caps);
                    let names = pattern.bound_vars();
                    self.with_ex_scope(caps, |m| m.with_locals(names, |m| TCaseBranch {
                        guards: br.guards.into_iter().map(|g| TGuard {
                            condition: f(m, g.condition),
                            body: f(m, g.body),
                        }).collect(),
                        body: br.body.map(|b| f(m, b)),
                        pattern,
                    }))
                }).collect();
                TExprKind::Case { scrutinee: Box::new(scrutinee), branches }
            }
            kind => return TExpr { kind, ty }.map_children(&mut |c| f(self, c)),
        };
        TExpr { kind, ty }
    }

    /// Monomorphize a local function/value binding: its own parameters are
    /// local inside its body (the binding's NAME is put in scope by the
    /// caller, for the whole recursive group).
    fn mono_local_def(&mut self, ld: TLocalDef) -> TLocalDef {
        let mut caps = Vec::new();
        let patterns = self.ex_rewrite_patterns(ld.patterns, &mut caps);
        let params: Vec<String> = patterns.iter().flat_map(TPattern::bound_vars).collect();
        let body = self.with_ex_scope(caps, |m| m.with_locals(params, |m| m.mono_expr(ld.body)));
        TLocalDef { name: ld.name, patterns, body }
    }

    fn mono_clause(&mut self, mut clause: TClause) -> TClause {
        // An existential constructor pattern binds its captured
        // dictionaries too (hidden trailing binders); they scope over
        // exactly what the pattern's own variables scope over.
        let mut caps = Vec::new();
        clause.patterns = self.ex_rewrite_patterns(std::mem::take(&mut clause.patterns), &mut caps);
        // Clause pattern variables and where-bound names are local in the
        // body, the guards and the where bodies (a where group is one
        // recursive scope, like a let group).
        let scope: Vec<String> = clause.patterns.iter().flat_map(TPattern::bound_vars)
            .chain(clause.where_binds.iter().map(|ld| ld.name.clone()))
            .collect();
        self.with_ex_scope(caps, |m| m.with_locals(scope, |m| {
            clause.body = clause.body.take().map(|b| m.mono_expr(b));
            clause.guards = clause.guards.into_iter().map(|g| TGuard {
                condition: m.mono_expr(g.condition),
                body: m.mono_expr(g.body),
            }).collect();
            clause.where_binds = clause.where_binds.into_iter()
                .map(|ld| m.mono_local_def(ld)).collect();
            clause
        }))
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
        // The binders the spine introduces (a bind's lambda parameters, a
        // let group's names) scope over everything after them — the later
        // statements' left-hand sides mono'd while walking down, and the
        // terminal — so they enter `locals` as the walk passes them and
        // leave together after the terminal (with_locals, unrolled across
        // the loop). Each let binding's own parameters are scoped by
        // mono_local_def.
        let mut spine_locals: Vec<String> = Vec::new();

        loop {
            match &current.kind {
                TExprKind::InfixApp { op, lhs, .. } if (op == ">>=" || op == ">>") && Self::is_action_monad(&lhs.ty) => {
                    let ty = current.ty.clone();
                    if let TExprKind::InfixApp { op, lhs, rhs } = current.kind {
                        let rhs_ty = rhs.ty.clone();
                        if op == ">>="
                            && let TExprKind::Lambda { params, body } = rhs.kind {
                                let lhs = self.mono_expr(*lhs);
                                for (name, _) in &params {
                                    if self.locals.insert(name.clone()) {
                                        spine_locals.push(name.clone());
                                    }
                                }
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
                        // Names first (a let group is one recursive scope),
                        // then the bodies.
                        for b in &binds {
                            if self.locals.insert(b.name.clone()) {
                                spine_locals.push(b.name.clone());
                            }
                        }
                        let binds = binds.into_iter().map(|b| self.mono_local_def(b)).collect();
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
        for name in &spine_locals {
            self.locals.remove(name);
        }

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
                // 1. Typeclass method dispatch, driven by the instantiated
                // method type the typechecker recorded on this node.
                // Skip if the name is a locally-bound variable (parameter, let,
                // case pattern).
                if self.method_to_class.contains_key(name) && !self.locals.contains(name) {
                    match self.dispatch_method(name, &ty, &DictEnv::default()) {
                        MethodUse::Impl(mangled) => {
                            return TExpr { kind: TExprKind::Var(mangled), ty };
                        }
                        // A method at an EXISTENTIAL type (a skolem bound
                        // by an enclosing constructor match): the
                        // dictionary the constructor captured for it.
                        MethodUse::Evidence(kind) => return TExpr { kind, ty },
                        MethodUse::Missing(binding) => self.push_no_instance(name, &binding),
                        MethodUse::Native | MethodUse::Deferred => {}
                    }
                }
                // A constrained polymorphic function used FIRST-CLASS at an
                // existential type: no specialization can be compiled for a
                // skolem, so the use takes dictionary passing with the
                // captured dictionaries (eta-expanded over its parameters).
                if self.ex_dict_call_wanted(name, &ty) {
                    let name = name.clone();
                    self.dict_passing_fns.insert(name.clone());
                    return self.dictify_use(&name, Vec::new(), ty);
                }
                // 2. Check for polymorphic function specialization
                // Handle calls inside specializations: if the type is still
                // polymorphic but we have specialization(s) for this function
                // name, use the most recent one (the recursive/sibling call
                // shares the same concrete type as the enclosing specialization)
                // An `arity_only_fns` builtin is excluded: its specializations
                // exist only to fix an arity, so redirecting a
                // still-polymorphic reference to an arbitrary one of them
                // would hand the use a function of the WRONG arity. The
                // shared generic copy is what such a reference wants.
                // HashMap ops (A17): an hm* use whose KEY type the scalar
                // hashmap path cannot serve — a structural shape (the Lua
                // table would be keyed by identity, and the runtime
                // tripwire would reject it) or a type that needs Hashable
                // evidence — is rewritten by `hm_op_use`, the one decision
                // the dictionary rewrite makes too. The main walk carries
                // no dictionary environment: evidence here is a captured
                // existential dictionary, and a body about to go
                // dictionary-form keeps the node for the rewrite.
                if !self.locals.contains(name)
                    && let Some(op) = Self::hm_key_op(name)
                    && let Some(key_ty) = Self::hm_key_type(op, &ty)
                    && let Some(rewritten) = self.hm_op_use(op, &key_ty, &ty, &DictEnv::default())
                {
                    return rewritten;
                }
                // A use of a DICTIONARY-PASSING function that widens its
                // arity: the one dictionary form is emitted at the declared
                // arrow count, which cannot serve it (see `arity_only_fns`),
                // so the use takes the dictionary-passing copy at its arrow
                // shape, which the dictionary rewrite then calls like any
                // other dictionary-passing function — at its own arrow
                // count. The copy's own body re-monomorphizes here too; a
                // recursive use at the same shape finds the same copy.
                if self.dict_passing_fns.contains(name)
                    && !self.locals.contains(name)
                    && self.poly_fns.contains_key(name)
                    && self.arity_widening(name, &ty)
                {
                    let name = name.clone();
                    if let Some(copy) = self.widening_copy(&name, &ty) {
                        return TExpr { kind: TExprKind::Var(copy), ty };
                    }
                }
                let poly_use = self.poly_fns.contains_key(name)
                    && !self.locals.contains(name)
                    && !self.dict_passing_fns.contains(name);
                let residual = self.is_polymorphic(&ty);
                // A still-polymorphic use whose residual variables are all
                // DEAD (see `residual_vars_dead`) specializes exactly like
                // a concrete one, at the canonicalized type, below. One
                // whose residual variables are LIVE — a variable the
                // context constrains never got instantiated — cannot be
                // specialized: the copy would need a dictionary nobody can
                // name. Inside a specialization of `name` such a use is
                // the recursive/sibling call sharing the enclosing copy's
                // type (top-most on the generation stack); inside a
                // DICTIONARY-FORM body it stays by name, and the dictionary
                // rewrite that follows routes it through the context's
                // dictionaries; anywhere else it stays on the generic copy,
                // whose type-erased dispatch is the only implementation
                // that fits every instantiation. It is never pointed at an
                // arbitrary sibling specialization: the old fallback took
                // the lexically-smallest one, which welded one use's type
                // into another's — `f (1 :: Int) []` with `f :: Show a =>
                // a -> b -> String` ran the String or Number copy of `f`
                // (`"1"` shown as `1.0`, or "attempt to get length of a
                // number"), and gdTag-for-every-type once called
                // gdConBody-for-BlueC. A dead residual variable (the `b`
                // there) used to be what sent a use down that fallback.
                if poly_use && residual && !self.residual_vars_dead(name, &ty) {
                    // A WIDENING use is excluded for the same reason the
                    // arity-only builtins are: the enclosing specialization
                    // was compiled at its own arrow count, which is the
                    // wrong one for this use.
                    if !self.arity_only_fns.contains(name)
                        && !self.arity_widening(name, &ty)
                        && let Some((_, mangled)) = self.gen_stack.iter().rev()
                            .find(|(n, _)| n == name)
                    {
                        return TExpr { kind: TExprKind::Var(mangled.clone()), ty };
                    }
                    return TExpr { kind: expr.kind, ty };
                }
                // A concrete use specializes at its type. A still-polymorphic
                // use with only dead residual variables specializes at the
                // CANONICALIZED type (variables renamed in first-occurrence
                // order), so every use of one shape shares one copy: the
                // dead variables do not reach the body, so the copy compiled
                // for `Int -> [_w0] -> String` is the right code for every
                // list type in that slot. Keying on the use type as written
                // would mint a copy per fresh unification variable.
                // The arity-WIDENING use of a shared builtin is the shape
                // that first demanded this: its leftover variables are dead,
                // but the ARROW STRUCTURE is the whole point of the copy,
                // and it is concrete even when an element type is not.
                // Refuted by backend_fuzz index 66: `(flip const True (\v ->
                // False)) []` — the dead `[]` element type kept `flip` on
                // the generic copy, whose body calls `f(a, b)` flat, while
                // the first-class `const` was (rightly) widened to three
                // parameters: f consumed its eta slot as nil and flip's real
                // result was applied to a boolean. The copy and the widened
                // value must agree, so the copy must exist. Not just the
                // shared builtins: a USER polymorphic function hits the same
                // hole (backend_fuzz index 123 — `fuzzTwice` at `a := t1 ->
                // t2 -> R` with a dead variable left in the use type kept
                // the generic copy, whose body applies `f` one argument at a
                // time, against a 3-parameter flattened lambda argument: the
                // flat lambda ran on nils and the caller applied its table
                // result). And a constrained function is no different once
                // its residual variables are known dead: `f (T 1) []` with
                // `f :: Show a => a -> b -> String` must run `show` at `T`,
                // which only a copy specialized at `T -> [_w0] -> String`
                // does — the generic copy's erased `show` printed `(1)`.
                // A shared function (`arity_only_fns`) is specialized ONLY
                // by a widening use, and that copy is keyed by the use's
                // arrow shape rather than its type (`arity_shape`): the
                // shape is all the copy exists to fix, so one copy serves
                // every use of that shape and the copies stay few — a
                // constraint-free function past the cap would otherwise
                // start a second climb toward it.
                if poly_use && self.specialization_wanted(name, &ty) {
                    let spec_ty = if self.arity_only_fns.contains(name) {
                        self.arity_shape(name, &ty)
                    } else if residual {
                        Self::canonicalize_vars(&ty)
                    } else {
                        ty.clone()
                    };
                    match self.specialize_at(name, &spec_ty) {
                        Some(mangled) => TExprKind::Var(mangled),
                        // The specialization cap tripped on this very use.
                        // The shared form the trip switched the function
                        // to serves it — run()'s worklist routes a
                        // constrained one through dictionary passing, a
                        // constraint-free one is its generic copy — unless
                        // this use widens the arity, which only an
                        // arrow-shape copy serves; when those are
                        // exhausted too the trip reported it and the
                        // reference stays by name.
                        None => {
                            if self.arity_widening(name, &ty) {
                                let name = name.clone();
                                if let Some(copy) = self.widening_copy(&name, &ty) {
                                    return TExpr { kind: TExprKind::Var(copy), ty };
                                }
                            }
                            return TExpr { kind: expr.kind, ty };
                        }
                    }
                } else {
                    expr.kind
                }
            }
            TExprKind::App(func, arg) => {
                let whole = TExpr { kind: TExprKind::App(func, arg), ty: ty.clone() };
                // A SATURATED constrained-existential constructor: pack the
                // dictionaries for the hidden type as hidden trailing
                // arguments (see ExCtor). Checked on the un-rewritten spine
                // so an infeasible pack leaves the node untouched.
                if let Some(ex_name) = self.ex_saturated_ctor(&whole)
                    && let Some(packed) = self.ex_pack_saturated(ex_name, whole.clone(), true, &DictEnv::default())
                {
                    return packed;
                }
                // A constrained polymorphic function applied at an
                // existential type: dictionary passing with the captured
                // dictionaries (a specialization cannot be compiled for a
                // skolem — its body would have nothing to dispatch on).
                if let Some(call_name) = Self::app_head_name(&whole)
                    && self.ex_dict_call_wanted(&call_name, &ty)
                {
                    let n_args = Self::app_chain_len(&whole);
                    if self.dictify_feasible(&call_name, n_args, &ty) {
                        self.dict_passing_fns.insert(call_name.clone());
                        let (_head, args) = Self::split_app_chain(whole);
                        let args: Vec<TExpr> = args.into_iter().map(|a| self.mono_expr(a)).collect();
                        return self.dictify_use(&call_name, args, ty);
                    }
                }
                let TExprKind::App(func, arg) = whole.kind else { unreachable!() };
                // A class method in head position is dispatched by the Var
                // case when `func` is monomorphized — the method node's own
                // type is the full instantiated method type, which determines
                // the class-variable binding regardless of where the class
                // variable sits (argument, result, nested, higher-kinded).
                TExprKind::App(
                    Box::new(self.mono_expr(*func)),
                    Box::new(self.mono_expr(*arg)),
                )
            }
            // A constrained-existential constructor used UNSATURATED (`map
            // MkBox xs`, `MkBox . f`): eta-expand over its declared fields so
            // the saturated application inside can pack the dictionaries
            // (the field types are read off this node's instantiated type).
            TExprKind::Con(ref name) if self.ex_ctors.contains_key(name) => {
                let name = name.clone();
                match self.ex_eta_expand(&name, ty.clone(), &DictEnv::default()) {
                    Some(kind) => kind,
                    None => TExprKind::Con(name),
                }
            }
            TExprKind::OpFunc(ref op) => {
                // A FIRST-CLASS section of a class-method operator
                // (`foldr (<>) mempty`, `zipWith (<+>) xs ys`): dispatch it
                // exactly like an infix use, from the operator type the
                // checker recorded on this node. Without this, codegen's
                // OpFunc fallback hard-codes a Lua operator — `<>` becomes
                // string concatenation regardless of the instance, and a
                // user-defined operator symbol would be emitted verbatim (a
                // Lua syntax error). Only a successful resolution rewrites;
                // deferred/missing uses keep the old fallback.
                let op = op.clone();
                if self.method_to_class.contains_key(&op) && !self.locals.contains(&op) {
                    match self.dispatch_method(&op, &ty, &DictEnv::default()) {
                        // A real instance function (a user Num type, `<>`,
                        // …) or a dictionary read becomes the value; a
                        // NATIVE operator (`+` at Int/Number) stays an
                        // OpFunc so codegen emits the operator lambda —
                        // byte-identical to before the operator became a
                        // class method, and never a reference to a
                        // nonexistent `_plus_`-style global.
                        MethodUse::Impl(mangled) => return TExpr { kind: TExprKind::Var(mangled), ty },
                        MethodUse::Evidence(kind) => return TExpr { kind, ty },
                        MethodUse::Missing(binding) => self.push_no_instance(&op, &binding),
                        MethodUse::Native | MethodUse::Deferred => {}
                    }
                }
                expr.kind
            }
            TExprKind::InfixApp { op, lhs, rhs } => {
                // Typeclass method dispatch on infix operators. The TIR drops
                // the operator's own instantiated type, but it is exactly
                // `lhs.ty -> rhs.ty -> result_ty` — reconstruct it and
                // dispatch the same way as a named method use.
                if self.method_to_class.contains_key(&op) && !self.locals.contains(&op) {
                    let use_ty = Ty::fun(&[lhs.ty.clone(), rhs.ty.clone()], ty.clone());
                    // The method as an expression: the implementation
                    // name, or a dictionary read at an existential skolem
                    // with a captured dictionary. A NATIVE operator (IO's
                    // `>>=`, `+` at Int) stays an InfixApp for the normal
                    // handling below.
                    let method_expr: Option<TExpr> = match self.dispatch_method(&op, &use_ty, &DictEnv::default()) {
                        MethodUse::Impl(mangled) => Some(TExpr::new(TExprKind::Var(mangled), Ty::Unit)),
                        MethodUse::Evidence(kind) => Some(TExpr::new(kind, use_ty.clone())),
                        MethodUse::Missing(binding) => {
                            self.push_no_instance(&op, &binding);
                            None
                        }
                        MethodUse::Native | MethodUse::Deferred => None,
                    };
                    if let Some(method_expr) = method_expr {
                        let mono_lhs = self.mono_expr(*lhs);
                        let mono_rhs = self.mono_expr(*rhs);
                        return Self::method_operator_app(method_expr, mono_lhs, mono_rhs, ty);
                    }
                }
                // A non-class-method operator that names a polymorphic function
                // (e.g. the Prelude `(^)`): route it through application so it
                // SPECIALIZES at the operand types. Left as an InfixApp it would
                // call the generic copy, whose inner Num ops and literals stay
                // polymorphic (inline `*`, a machine `1` for `x ^ 0`) — wrong at
                // Integer. As an App, mono specializes the body so `*` becomes
                // mul_Integer and `1` becomes fromInteger_Integer.
                if !self.locals.contains(&op) && self.poly_fns.contains_key(&op) {
                    let op_ty = Ty::fun(&[lhs.ty.clone(), rhs.ty.clone()], ty.clone());
                    let partial_ty = Ty::fun(std::slice::from_ref(&rhs.ty), ty.clone());
                    let app = TExpr {
                        kind: TExprKind::App(
                            Box::new(TExpr::new(
                                TExprKind::App(
                                    Box::new(TExpr::new(TExprKind::Var(op.clone()), op_ty)),
                                    lhs,
                                ),
                                partial_ty,
                            )),
                            rhs,
                        ),
                        ty,
                    };
                    return self.mono_expr(app);
                }
                TExprKind::InfixApp {
                    op,
                    lhs: Box::new(self.mono_expr(*lhs)),
                    rhs: Box::new(self.mono_expr(*rhs)),
                }
            }
            TExprKind::Negate(inner) => {
                // Prefix minus is the `negate` method at `t -> t`: Int and
                // Number keep Lua's unary `-`; every other Num type (Integer,
                // a user instance, a dictionary-bound or existential type)
                // has no `__unm`, so it dispatches like any method — the
                // same decision the InfixApp arm makes for `+ - *`.
                let inner = self.mono_expr(*inner);
                match self.negate_use(&ty, inner, &DictEnv::default()) {
                    Ok(e) => return e,
                    Err(inner) => TExprKind::Negate(Box::new(inner)),
                }
            }
            TExprKind::Lambda { params, body } => {
                let names: Vec<String> = params.iter().map(|(n, _)| n.clone()).collect();
                let body = self.with_locals(names, |m| Box::new(m.mono_expr(*body)));
                TExprKind::Lambda { params, body }
            }
            TExprKind::Case { scrutinee, branches } => {
                TExprKind::Case {
                    scrutinee: Box::new(self.mono_expr(*scrutinee)),
                    branches: branches.into_iter().map(|b| {
                        // An existential constructor pattern binds its
                        // captured dictionaries over the guards and body.
                        let mut caps = Vec::new();
                        let pattern = self.ex_rewrite_pattern(b.pattern, &mut caps);
                        let pat_vars = pattern.bound_vars();
                        self.with_ex_scope(caps, |m| m.with_locals(pat_vars, |m| TCaseBranch {
                            pattern,
                            guards: b.guards.into_iter().map(|g| TGuard {
                                condition: m.mono_expr(g.condition),
                                body: m.mono_expr(g.body),
                            }).collect(),
                            body: b.body.map(|bb| m.mono_expr(bb)),
                        }))
                    }).collect(),
                }
            }
            TExprKind::Let { binds, body } => {
                let names: Vec<String> = binds.iter().map(|b| b.name.clone()).collect();
                self.with_locals(names, |m| {
                    let binds = binds.into_iter().map(|b| m.mono_local_def(b)).collect();
                    let body = Box::new(m.mono_expr(*body));
                    TExprKind::Let { binds, body }
                })
            }
            // Numeric-literal overloading, GHC-faithful: `fromInteger (n ::
            // Integer)` / `fromRational (r :: Number)` through the one
            // dispatch decision (see `literal_use`). At Int/Number the
            // conversion is the identity and the raw literal is emitted
            // (byte-identical to the pre-overloading compiler).
            TExprKind::Lit(lit @ (TLiteral::Integer(_) | TLiteral::Number(_) | TLiteral::BigInteger(_))) => {
                match self.literal_use(lit, &ty, &DictEnv::default()) {
                    Ok(e) => return e,
                    Err(lit) => TExprKind::Lit(lit),
                }
            }
            // Everything else — If, Paren, Tuple, RecordUpdate, SpecCall and
            // dictionary nodes, callbacks, non-numeric literals — is pure
            // structural descent: no dispatch decision and no scope to track.
            other => {
                return TExpr { kind: other, ty }
                    .map_children(&mut |c| self.mono_expr(c));
            }
        };
        TExpr { kind, ty }
    }

    /// A method applied infix: `method lhs rhs` as two Apps (the shape
    /// codegen's N-ary call protocol flattens), the one builder both passes
    /// use for an operator that dispatched to a function or a dictionary
    /// read.
    fn method_operator_app(method_expr: TExpr, lhs: TExpr, rhs: TExpr, ty: Ty) -> TExpr {
        TExpr {
            kind: TExprKind::App(
                Box::new(TExpr::new(
                    TExprKind::App(Box::new(method_expr), Box::new(lhs)),
                    Ty::Unit,
                )),
                Box::new(rhs),
            ),
            ty,
        }
    }

    /// Prefix negation at type `ty` with the already-processed operand:
    /// the `negate` method dispatched at `ty -> ty` (see `dispatch_method`)
    /// applied to the operand, or the operand handed back (`Err`) when the
    /// node stays Lua's unary minus — the native Int/Number negate, or a
    /// deferred decision. The builtin Int/Number instances name their
    /// `negate` the runtime helpers `negate_Int`/`negate_Number`, which a
    /// first-class `negate` calls; the PREFIX form at those types is the
    /// operator itself.
    fn negate_use(&mut self, ty: &Ty, inner: TExpr, ev: &DictEnv) -> Result<TExpr, TExpr> {
        let use_ty = Ty::arrow(ty.clone(), ty.clone());
        let neg = match self.dispatch_method("negate", &use_ty, ev) {
            MethodUse::Impl(name) if name != "negate_Int" && name != "negate_Number" =>
                TExpr::new(TExprKind::Var(name), Ty::Unit),
            MethodUse::Evidence(kind) => TExpr::new(kind, use_ty),
            MethodUse::Missing(binding) => {
                self.push_no_instance("negate", &binding);
                return Err(inner);
            }
            MethodUse::Impl(_) | MethodUse::Native | MethodUse::Deferred => return Err(inner),
        };
        Ok(TExpr {
            kind: TExprKind::App(Box::new(neg), Box::new(inner)),
            ty: ty.clone(),
        })
    }

    /// The overloaded lowering of a numeric literal at type `ty` — GHC's
    /// `fromInteger (n :: Integer)` for an integer literal, `fromRational
    /// (r :: Number)` for a fractional one — through `dispatch_method` at
    /// `Integer -> ty` / `Number -> ty`:
    ///   * Int/Number: the builtin conversions are the identity
    ///     passthroughs (`fromInteger_Int`, `fromInteger_Number`,
    ///     `fromRational_Number`), so the literal stays raw (`Err`) — the
    ///     documented host representation;
    ///   * Integer: `fromInteger_Integer n` (machine -> bignum);
    ///   * a user Num type: `fromInteger_T (fromInteger_Integer n)` — the
    ///     user's `fromInteger` receives an actual Integer, as in GHC;
    ///   * a dictionary-bound or existential type: `<dict>.fromInteger`
    ///     over the same Integer argument;
    ///   * a deferred decision: raw (`Err`), the dictionary rewrite of
    ///     the body comes back to it.
    /// An integer literal bound for a real fromInteger must arrive EXACT:
    /// emitted as a bare Lua numeral it is read back by the host, and a
    /// doubles-only host (LuaJIT) rounds anything past 2^53 —
    /// `9223372036854775807 :: Integer` became 2^63 before the bignum ever
    /// saw it. Past 2^53 the Integer argument is the decimal-string
    /// BigInteger literal, which codegen emits through the exact
    /// `__mll_biglit` pool on every host; a BigInteger literal (> i64) is
    /// already that.
    fn literal_use(&mut self, lit: TLiteral, ty: &Ty, ev: &DictEnv) -> Result<TExpr, TLiteral> {
        let integer = Ty::Con("Integer".into());
        let number = Ty::Con("Number".into());
        let (method, src_ty) = match &lit {
            TLiteral::Integer(_) | TLiteral::BigInteger(_) => ("fromInteger", integer.clone()),
            TLiteral::Number(_) => ("fromRational", number.clone()),
            _ => return Err(lit),
        };
        let use_ty = Ty::arrow(src_ty, ty.clone());
        let conv = match self.dispatch_method(method, &use_ty, ev) {
            MethodUse::Impl(name) => match name.as_str() {
                "fromInteger_Int" | "fromInteger_Number" | "fromRational_Number" => return Err(lit),
                _ => TExpr::new(TExprKind::Var(name), Ty::Unit),
            },
            MethodUse::Evidence(kind) => TExpr::new(kind, use_ty),
            MethodUse::Missing(binding) => {
                self.push_no_instance(method, &binding);
                return Err(lit);
            }
            MethodUse::Native | MethodUse::Deferred => return Err(lit),
        };
        let to_integer = |raw: TExpr| TExpr::new(
            TExprKind::App(
                Box::new(TExpr::new(TExprKind::Var("fromInteger_Integer".into()), Ty::Unit)),
                Box::new(raw),
            ),
            integer.clone(),
        );
        let lifts = matches!(&conv.kind, TExprKind::Var(n) if n == "fromInteger_Integer");
        let arg = match lit {
            TLiteral::Integer(n) if n.unsigned_abs() > (1u64 << 53) =>
                TExpr::new(TExprKind::Lit(TLiteral::BigInteger(n.to_string())), integer.clone()),
            TLiteral::Integer(n) => {
                let raw = TExpr::new(TExprKind::Lit(TLiteral::Integer(n)), Ty::Con("Int".into()));
                // At Integer the conversion IS the lift: one call, not two.
                if lifts {
                    return Ok(TExpr { kind: to_integer(raw).kind, ty: ty.clone() });
                }
                to_integer(raw)
            }
            TLiteral::BigInteger(s) =>
                TExpr::new(TExprKind::Lit(TLiteral::BigInteger(s)), integer.clone()),
            TLiteral::Number(x) => TExpr::new(TExprKind::Lit(TLiteral::Number(x)), number.clone()),
            _ => unreachable!(),
        };
        if lifts {
            // A big literal at Integer is already an Integer value (codegen
            // emits `__int_from_decimal`).
            return Ok(TExpr { kind: arg.kind, ty: ty.clone() });
        }
        Ok(TExpr {
            kind: TExprKind::App(Box::new(conv), Box::new(arg)),
            ty: ty.clone(),
        })
    }

    /// Resolve the eq function for an element type, generating specialized eq
    /// for structural shapes if needed. Type variables use the generic
    /// raw-equality runtime function.
    fn resolve_elem_eq(&mut self, ty: &Ty) -> String {
        if matches!(ty, Ty::Var(_)) {
            return "__mll_eq".to_string();
        }
        let eq_use_ty = Ty::fun(&[ty.clone(), ty.clone()], Ty::Con("Bool".into()));
        match self.resolve_at_type("==", ty, &eq_use_ty) {
            Some(name) => name,
            None => "__mll_eq".to_string(),
        }
    }

    /// Build and register a synthetic runtime-backed function: a single
    /// clause whose body is one `SpecCall` handing the work to a runtime
    /// helper together with the resolved element implementations (see
    /// codegen's `spec_call_ast` for the decode side of the spec string).
    /// Shared by the structural eq/show generators, which used to be four
    /// near-identical copies of this scaffolding. Memoized per
    /// `(method, ty)` in `generated_impls` — the memo is inserted BEFORE
    /// `build_spec` runs, so a recursive element type re-entering its own
    /// generator terminates on the memo hit.
    fn synthetic_spec_fn(
        &mut self,
        method: &str,
        base: &str,
        ty: &Ty,
        param_names: &[&str],
        result_ty: Ty,
        build_spec: impl FnOnce(&mut Self) -> SpecKind,
    ) -> String {
        let params: Vec<(&str, Ty)> = param_names.iter().map(|p| (*p, ty.clone())).collect();
        self.synthetic_spec_fn_typed(method, base, ty, &params, result_ty, build_spec)
    }

    /// `synthetic_spec_fn` with one type per parameter, for a helper whose
    /// parameters are not all the shape itself (`showsPrec`'s precedence
    /// and tail).
    fn synthetic_spec_fn_typed(
        &mut self,
        method: &str,
        base: &str,
        ty: &Ty,
        params: &[(&str, Ty)],
        result_ty: Ty,
        build_spec: impl FnOnce(&mut Self) -> SpecKind,
    ) -> String {
        let mangled = match self.memo_or_mangle(method, base, ty) {
            Ok(existing) => return existing,
            Err(fresh) => fresh,
        };

        let spec = build_spec(self);
        let args: Vec<TExpr> = params.iter()
            .map(|(p, pty)| TExpr::new(TExprKind::Var((*p).to_string()), pty.clone()))
            .collect();
        let body = TExpr::new(
            TExprKind::SpecCall {
                original: mangled.clone(),
                specialized: spec,
                args,
            },
            result_ty.clone(),
        );
        let param_tys: Vec<Ty> = params.iter().map(|(_, pty)| pty.clone()).collect();
        let func = TFunction {
            name: mangled.clone(),
            ty: Ty::fun(&param_tys, result_ty),
            clauses: vec![TClause {
                span: None,
                patterns: params.iter()
                    .map(|(p, _)| TPattern::Var((*p).to_string(), Ty::Unit))
                    .collect(),
                guards: vec![],
                body: Some(body),
                where_binds: vec![],
            }],
            specialized: true,
            spec_origin: None,
            dict_params: vec![],
            derived_strict: false,
        };
        self.generated.push(func);
        mangled
    }

    /// The memoization every generated implementation goes through: the name
    /// already generated for (`method`, `ty`) as `Ok`, or — recorded first,
    /// so a recursive demand for the same shape while the body is being
    /// built finds it — the fresh mangled name as `Err`.
    fn memo_or_mangle(&mut self, method: &str, base: &str, ty: &Ty) -> Result<String, String> {
        let key = (method.to_string(), ty.clone());
        if let Some(existing) = self.generated_impls.get(&key) {
            return Ok(existing.clone());
        }
        let mangled = self.mangle_name(base, ty);
        self.generated_impls.insert(key, mangled.clone());
        Err(mangled)
    }

    /// Generate a specialized eq function for a tuple type.
    fn generate_tuple_eq(&mut self, elem_tys: &[Ty]) -> String {
        let tuple_ty = Ty::Tuple(elem_tys.to_vec());
        let elem_tys = elem_tys.to_vec();
        self.synthetic_spec_fn("==", "eq", &tuple_ty, &["_a", "_b"], Ty::Con("Bool".into()),
            move |slf| {
                // Resolve eq per element (recursively generating nested
                // specialized eqs) and encode the names in the SpecCall.
                let names: Vec<String> = elem_tys.iter()
                    .map(|et| slf.resolve_elem_eq(et))
                    .collect();
                SpecKind::TupleEq(names)
            })
    }

    fn generate_list_eq(&mut self, elem_ty: &Ty) -> String {
        let list_ty = Ty::List(Box::new(elem_ty.clone()));
        let elem_ty = elem_ty.clone();
        self.synthetic_spec_fn("==", "eq", &list_ty, &["_a", "_b"], Ty::Con("Bool".into()),
            move |slf| SpecKind::ListEq(slf.resolve_elem_eq(&elem_ty)))
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
        let inner_ty = inner_ty.clone();
        self.synthetic_spec_fn("==", "eq", &maybe_ty, &["_a", "_b"], Ty::Con("Bool".into()),
            move |slf| SpecKind::MaybeEq(slf.resolve_elem_eq(&inner_ty)))
    }

    fn generate_container_show(&mut self, ty: &Ty) -> Option<String> {
        match ty {
            // A list shows its elements at precedence 0: the element show.
            Ty::List(elem_ty) =>
                Some(self.generate_threaded_show(ty, elem_ty, false, SpecKind::ShowList)),
            // Maybe is type-directed: Just x and x share a runtime rep, so the
            // element show plus the type recover the structure (nil == Nothing).
            // `Just x` shows its payload at precedence 11 through the
            // payload type's own showsPrec (GHC's derived `Show (Maybe a)`).
            Ty::App(f, elem_ty) if matches!(f.as_ref(), Ty::Con(n) if n == "Maybe") =>
                Some(self.generate_threaded_show(ty, elem_ty, true, SpecKind::ShowMaybe)),
            _ => None,
        }
    }

    /// Generate a `show` wrapper that threads the element show (or, with
    /// `prec`, the element showsPrec) into a runtime helper (`make` is
    /// `SpecKind::ShowList` / `SpecKind::ShowMaybe`). Shared by the
    /// container arms above so list and Maybe stay in lockstep.
    fn generate_threaded_show(
        &mut self,
        ty: &Ty,
        elem_ty: &Ty,
        prec: bool,
        make: fn(String) -> SpecKind,
    ) -> String {
        let elem_ty = elem_ty.clone();
        self.synthetic_spec_fn("show", "show", ty, &["_x"], Ty::Con("String".into()),
            move |slf| make(if prec {
                slf.resolve_shows_prec_for(&elem_ty)
            } else {
                slf.resolve_show_for(&elem_ty)
            }))
    }

    /// Resolve the show function name for a given type.
    fn resolve_show_for(&mut self, ty: &Ty) -> String {
        let show_use_ty = Ty::arrow(ty.clone(), Ty::Con("String".into()));
        match self.resolve_at_type("show", ty, &show_use_ty) {
            Some(name) => name,
            // Fallback to generic runtime show (primitives, or genuinely
            // unresolved/polymorphic types).
            None => "show".to_string(),
        }
    }

    /// Resolve the showsPrec function name for a given type — the
    /// precedence twin of `resolve_show_for`, falling back to the
    /// type-erased runtime `showsPrec` (the precedence rule over the
    /// generic show).
    fn resolve_shows_prec_for(&mut self, ty: &Ty) -> String {
        let str_ty = Ty::Con("String".into());
        let use_ty = Ty::fun(&[Ty::Con("Int".into()), ty.clone(), str_ty.clone()], str_ty);
        match self.resolve_at_type("showsPrec", ty, &use_ty) {
            Some(name) => name,
            None => "showsPrec".to_string(),
        }
    }

    /// Generate a specialized show function for a tuple type.
    /// show_(Int, String) produces: function(t) return "(" .. show_Int(t[1]) .. ", " .. show_String(t[2]) .. ")" end
    fn generate_tuple_show(&mut self, elem_tys: &[Ty]) -> String {
        let tuple_ty = Ty::Tuple(elem_tys.to_vec());
        let mangled = match self.memo_or_mangle("show", "show", &tuple_ty) {
            Ok(existing) => return existing,
            Err(fresh) => fresh,
        };

        // Resolve show for each element type. Use the unified resolver so a
        // structured element (list, Maybe, nested tuple, user ADT) gets its
        // specialized show rather than the type-erased generic `show` — which
        // would, e.g., render an empty-list element as "Nothing".
        let elem_show_names: Vec<String> =
            elem_tys.iter().map(|et| self.resolve_show_for(et)).collect();

        // Build body: "(" ++ show_E1(t[1]) ++ "," ++ show_E2(t[2]) ++ ... ++ ")"
        // — GHC's tuple show has no space after the comma: show (1,2) is
        // "(1,2)". We generate this as a chain of InfixApp(++, ...)
        let param_name = "_t".to_string();
        let str_ty = Ty::Con("String".to_string());

        let mut parts: Vec<TExpr> = vec![
            TExpr::new(TExprKind::Lit(TLiteral::Str(b"(".to_vec())), str_ty.clone()),
        ];
        for (i, show_fn) in elem_show_names.iter().enumerate() {
            if i > 0 {
                parts.push(TExpr::new(TExprKind::Lit(TLiteral::Str(b",".to_vec())), str_ty.clone()));
            }
            // show_Elem(t[i+1]) — represented as App(Var(show_fn), SpecCall to access field)
            let field_access = TExpr::new(
                TExprKind::SpecCall {
                    original: format!("_t_{}", i),
                    specialized: SpecKind::TupGet(i + 1),
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
        parts.push(TExpr::new(TExprKind::Lit(TLiteral::Str(b")".to_vec())), str_ty.clone()));

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
                span: None,
                patterns: vec![TPattern::Var(param_name, Ty::Unit)],
                guards: vec![],
                body: Some(body),
                where_binds: vec![],
            }],
            specialized: true,
            spec_origin: None,
            dict_params: vec![],
            derived_strict: false,
        };
        self.generated.push(func);
        mangled
    }

    /// Rewrite every reference to a purged specialization to what now
    /// serves it (see `purged_specs`): a constraint-free function's
    /// generic copy — a plain function, right in any position — or an
    /// arrow-shape copy, which for a dictionary-passing function is
    /// dictionary-passing itself and right only because the call-site
    /// rewrite that follows in the same walk (`rewrite_non_dict_fn`, on
    /// every pass) builds its dictionaries: a widening use is a call or a
    /// first-class use, both of which that rewrite reaches. A purged copy
    /// of a constrained function at its own arity is not in the map at
    /// all — its name stays a function of its type (`ground_forms`). A
    /// dictionary table (`Dict`/`DictCtor`) is NOT rewritten: a ground
    /// type's table is decided after the last trip (`materialize_dicts`),
    /// so it never names a purged copy — and renaming an entry to the
    /// method's original, by then a dictionary form taking a context
    /// dictionary first, called the method through the table with its
    /// arguments shifted by one (T13).
    fn revert_purged(&self, expr: TExpr) -> TExpr {
        let expr = match expr.kind {
            TExprKind::Var(name) => {
                let name = self.purged_specs.get(&name).cloned().unwrap_or(name);
                TExpr { kind: TExprKind::Var(name), ty: expr.ty }
            }
            // The threading SpecKind payloads carry function-name STRINGS
            // too (a list/Maybe eq or show's element function); a purged
            // specialization left there named a never-emitted function — a
            // nil lua_ref at runtime. Rewrite them through the same
            // mapping as Var references.
            TExprKind::SpecCall { original, mut specialized, args } => {
                let fix = |n: &mut String| {
                    if let Some(orig) = self.purged_specs.get(n) {
                        *n = orig.clone();
                    }
                };
                match &mut specialized {
                    SpecKind::ListEq(n)
                    | SpecKind::MaybeEq(n)
                    | SpecKind::NotEq(n)
                    | SpecKind::ShowList(n)
                    | SpecKind::ShowMaybe(n)
                    | SpecKind::ShowsPrecOf(n) => fix(n),
                    SpecKind::TupleEq(ns) => ns.iter_mut().for_each(&mut { fix }),
                    _ => {}
                }
                TExpr {
                    kind: TExprKind::SpecCall { original, specialized, args },
                    ty: expr.ty,
                }
            }
            other => TExpr { kind: other, ty: expr.ty },
        };
        expr.map_children(&mut |c| self.revert_purged(c))
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
            }
            // A body variable that appears nowhere in the signature is left
            // alone. Such a variable is genuinely unconstrained (a class
            // constraint over it would have been rejected as ambiguous), so
            // any method use involving it stays deferred to the generic
            // fallback. The old "single type parameter: default every
            // unmapped var to the one concrete type" rule invented types here
            // — that is exactly how a `Monad m`-constrained body once ended up
            // resolving at String.
        }
        Subst::from_map(map)
    }

    /// One structural matcher behind both substitution collectors: walk
    /// `pattern` (a declared/polymorphic type) against `concrete` (the type
    /// at a use) and report each variable binding, in traversal order, to
    /// `insert`. The two public collectors differ only in their map key
    /// (variable NAME for the signature-name epoch, full `TyVar` for exact
    /// identity) — they used to be byte-identical 35-line copies of this
    /// match.
    fn collect_subst_with(pattern: &Ty, concrete: &Ty, insert: &mut impl FnMut(&TyVar, &Ty)) {
        match (pattern, concrete) {
            (Ty::Var(v), _) => insert(v, concrete),
            (Ty::Arrow(pa, pb, _), Ty::Arrow(ca, cb, _)) |
            (Ty::App(pa, pb), Ty::App(ca, cb)) => {
                Self::collect_subst_with(pa, ca, insert);
                Self::collect_subst_with(pb, cb, insert);
            }
            (Ty::List(pa), Ty::List(ca)) |
            (Ty::IO(pa), Ty::IO(ca)) => Self::collect_subst_with(pa, ca, insert),
            // A constraint variable applied to an argument (`m a`) matched
            // against a sugared concrete type: IO b binds m := IO, [b] binds
            // m := [], LuaIO s b binds m := LuaIO s. Without these cases a
            // result-only `Monad m` never gets bound here, so its method uses
            // (`pure`, `>>=`) could not be dispatched at the use site.
            // Mirrors the App-vs-IO/List/LuaIO cases in types::unify.
            (Ty::App(pf, pa), Ty::IO(ca)) => {
                Self::collect_subst_with(pf, &Ty::Con("IO".into()), insert);
                Self::collect_subst_with(pa, ca, insert);
            }
            (Ty::App(pf, pa), Ty::List(ca)) => {
                Self::collect_subst_with(pf, &Ty::Con("[]".into()), insert);
                Self::collect_subst_with(pa, ca, insert);
            }
            (Ty::App(pf, pa), Ty::LuaIO(s, ca)) => {
                let lua_io_s = Ty::App(Box::new(Ty::Con("LuaIO".into())), Box::new(Ty::Var(s.clone())));
                Self::collect_subst_with(pf, &lua_io_s, insert);
                Self::collect_subst_with(pa, ca, insert);
            }
            (Ty::Tuple(ps), Ty::Tuple(cs)) if ps.len() == cs.len() => {
                for (p, c) in ps.iter().zip(cs.iter()) {
                    Self::collect_subst_with(p, c, insert);
                }
            }
            (Ty::Forall(_, pi), _) => Self::collect_subst_with(pi, concrete, insert),
            _ => {}
        }
    }

    fn collect_subst_by_name(pattern: &Ty, concrete: &Ty, map: &mut HashMap<String, Ty>) {
        Self::collect_subst_with(pattern, concrete, &mut |v, t| {
            map.insert(v.name.clone(), t.clone());
        });
    }

    fn collect_subst_exact(pattern: &Ty, concrete: &Ty, map: &mut HashMap<TyVar, Ty>) {
        Self::collect_subst_with(pattern, concrete, &mut |v, t| {
            map.insert(v.clone(), t.clone());
        });
    }

    fn collect_clause_vars(clause: &TClause, vars: &mut Vec<TyVar>) {
        for p in &clause.patterns { Self::collect_pattern_vars(p, vars); }
        if let Some(cb) = &clause.body { Self::collect_expr_vars(cb, vars); }
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
            TPattern::As(_, p) => Self::collect_pattern_vars(p, vars),
            TPattern::Tuple(ps) => { for p in ps { Self::collect_pattern_vars(p, vars); } }
            _ => {}
        }
    }

    /// Every type variable occurring in an expression tree: each node's type,
    /// the types carried by case-branch and let-bound patterns, and the
    /// children through `for_each_child` (the hand-rolled walk this replaces
    /// ended in `_ => {}` and skipped SpecCall/DictCall/DictMethod
    /// arguments and the pattern types).
    fn collect_expr_vars(expr: &TExpr, vars: &mut Vec<TyVar>) {
        for v in expr.ty.free_vars() { if !vars.contains(&v) { vars.push(v); } }
        match &expr.kind {
            TExprKind::Case { branches, .. } => {
                for b in branches { Self::collect_pattern_vars(&b.pattern, vars); }
            }
            TExprKind::Let { binds, .. } => {
                for b in binds {
                    for p in &b.patterns { Self::collect_pattern_vars(p, vars); }
                }
            }
            _ => {}
        }
        expr.for_each_child(&mut |c| Self::collect_expr_vars(c, vars));
    }

    /// The name of the dictionary parameter standing for the `index`th
    /// (1-based) constraint of a dictionary-passing function or a
    /// dictionary-form instance method: `__dict1_Show` for a first
    /// constraint `Show a`, `__dict2_Show` for a second `Show b`. The ONE
    /// function that mints these names — every table that looks one up is
    /// built from its output by `dict_param_tables`. The position is what
    /// makes the name unique within a function: `(Show a, Show b)` once
    /// produced two parameters both called `__dict_Show`, so the second
    /// shadowed the first and both constraints read one dictionary (T4);
    /// naming by class and variable would collide again on underscores
    /// (`C_x y` against `C x_y`) and on two compound constraints of one
    /// class (`GEncode (Rep a)`, whose `type_var` is an inert placeholder
    /// anyway). The `__` prefix is the compiler's reserved namespace (the
    /// lexer refuses it in source), so no program binder can capture a
    /// dictionary parameter; the Lua spelling goes through `sanitize_name`
    /// like any other variable of the body.
    fn dict_param_name(index: usize, c: &TyConstraint) -> String {
        format!("__dict{}_{}", index, c.class_name)
    }

    /// The three views of a constraint list's dictionary parameters, all
    /// minted by `dict_param_name` in the list's order (the list is a
    /// superclass closure, see `superclass_closure`): the ordered
    /// `(class, parameter)` pairs (`TFunction::dict_params`, the call
    /// convention), the per-class fallback `class -> parameter` (the FIRST
    /// constraint of the class, for a method use whose variable name the
    /// body no longer matches), and the precise `(class, variable) ->
    /// parameter` map a method use at exactly that variable resolves
    /// through.
    fn dict_param_tables(constraints: &[TyConstraint]) -> (Vec<(String, String)>, DictEnv) {
        let dict_params: Vec<(String, String)> = constraints.iter().enumerate()
            .map(|(i, c)| (c.class_name.clone(), Self::dict_param_name(i + 1, c)))
            .collect();
        let mut class_to_dict: HashMap<String, String> = HashMap::new();
        for (cls, param) in &dict_params {
            class_to_dict.entry(cls.clone()).or_insert_with(|| param.clone());
        }
        let env: HashMap<(String, String), String> = constraints.iter()
            .zip(dict_params.iter())
            .map(|(c, (_, p))| ((c.class_name.clone(), c.type_var.clone()), p.clone()))
            .collect();
        (dict_params, DictEnv { class_to_dict, env })
    }

    /// Rewrite a function to use dictionary-passing.
    fn rewrite_dict_passing_fn(&mut self, func: &mut TFunction) {
        let constraints = match self.fn_constraints.get(&func.name) {
            Some(cs) => cs.clone(),
            // Defensive: every flag into `dict_passing_fns` is gated on a
            // context (a constraint-free function past the cap is a shared
            // `arity_only_fns` member instead), so there is nothing to
            // rewrite a context-less function to.
            None => return,
        };
        // Start from the PRISTINE polymorphic body, not the main-walk mono'd
        // one: ordinary monomorphization resolves a still-polymorphic call
        // (gdTag's `gdConBody p j`) to an arbitrary existing specialization
        // as a runtime-fallback heuristic, which would weld one type's baked
        // metadata into this shared body. Re-monomorphize under in_dictform,
        // which keeps such calls by NAME for the dictionary rewrite below.
        if let Some(orig) = self.poly_fns.get(&func.name).cloned() {
            let saved_in_dictform = std::mem::replace(&mut self.in_dictform, true);
            let fresh = self.mono_function(orig);
            self.in_dictform = saved_in_dictform;
            func.clauses = fresh.clauses;
        }
        let (dict_params, ev) = Self::dict_param_tables(&constraints);
        func.dict_params = dict_params.clone();

        // The (class, argument type, parameter) triples for COMPOUND-constraint
        // routing: pair each constraint's structured argument (parallel, by
        // declaration order) with its dictionary parameter. Plain `Class a`
        // constraints (a bare `Var` argument) are EXCLUDED — those are matched
        // by variable name in the `Some(Ty::Var)` arm, and a bare variable would
        // otherwise match every structured binding and steal its dictionary.
        let dict_by_arg: Vec<(String, Ty, String)> = self.fn_constraint_args
            .get(&func.name).cloned().unwrap_or_default()
            .into_iter()
            .zip(dict_params.iter())
            .filter(|((_, arg), _)| !matches!(arg, Ty::Var(_)))
            .map(|((cls, arg), (_, param))| (cls, arg, param.clone()))
            .collect();

        let func_name = func.name.clone();
        let saved_dict_by_arg = std::mem::replace(&mut self.cur_dict_by_arg, dict_by_arg);
        let saved_dict_params = std::mem::replace(&mut self.cur_dict_params, dict_params);
        for clause in &mut func.clauses {
            self.map_clause_exprs_scoped(clause, &mut |m, e| {
                m.rewrite_dict_expr(e, &func_name, &ev)
            });
        }
        self.cur_dict_params = saved_dict_params;
        self.cur_dict_by_arg = saved_dict_by_arg;
    }

    /// Rewrite an expression for dictionary-passing: every method use the
    /// main walk deferred takes its evidence from `ev` through the same
    /// `dispatch_method` decision (named, infix, first-class, prefix
    /// negation, literal), and every call to a constrained function gets
    /// its dictionaries built here.
    fn rewrite_dict_expr(&mut self, expr: TExpr, func_name: &str, ev: &DictEnv) -> TExpr {
        let ty = expr.ty.clone();
        let kind = match expr.kind {
            TExprKind::Var(ref name) => {
                if self.locals.contains(name) {
                    // A locally-bound name shadows any method, dict-passing
                    // global, or the function itself for the rest of this
                    // node (same discipline as mono_expr_node's Var arm).
                    return expr;
                }
                // An hm* use the main walk deferred (its key type needs
                // Hashable evidence): the operation's dictionary form over
                // this body's evidence.
                if let Some(op) = Self::hm_key_op(name)
                    && let Some(key_ty) = Self::hm_key_type(op, &ty)
                    && let Some(rewritten) = self.hm_op_use(op, &key_ty, &ty, ev)
                {
                    return rewritten;
                }
                if self.method_to_class.contains_key(name) {
                    match self.dispatch_method(name, &ty, ev) {
                        MethodUse::Impl(mangled) => return TExpr { kind: TExprKind::Var(mangled), ty },
                        MethodUse::Evidence(kind) => return TExpr { kind, ty },
                        // A missing instance was reported by the main walk.
                        MethodUse::Native | MethodUse::Deferred | MethodUse::Missing(_) => {}
                    }
                }
                // A FIRST-CLASS reference to a dict-passing function (the
                // function itself, or a constrained sibling) inside this
                // dictionary-form body: the bare name compiles to the
                // dictionary-form Lua function, so whoever receives it would
                // pass a value where a dictionary is expected. Eta-expand
                // over the value parameters, building the dictionaries here —
                // from this body's own dictionary parameters when the use is
                // at an enclosing constrained variable, constructed
                // otherwise. (The call-site analogue lives in
                // `rewrite_dict_call_sites`; this body's calls never pass
                // through it.)
                if (*name == *func_name || self.poly_fns.contains_key(name))
                    && self.fn_constraints.get(name).is_some_and(|cs| !cs.is_empty())
                    && (self.dict_passing_fns.contains(name) || self.is_polymorphic(&ty))
                    && self.dictify_feasible(name, 0, &ty)
                {
                    let name = name.clone();
                    // A still-polymorphic first-class use can never resolve
                    // to a specialization — mark the callee dict-passing so
                    // run()'s worklist rewrites it too (same discovery as
                    // the call arm below).
                    self.dict_passing_fns.insert(name.clone());
                    let constraints = self.fn_constraints
                        .get(&name).cloned().unwrap_or_default();
                    let decl_ty = self.poly_fns.get(&name).map(|f| f.ty.clone());
                    let mut bind: HashMap<String, Ty> = HashMap::new();
                    if let Some(dt) = &decl_ty {
                        let blanked = self.blank_family_apps(dt);
                        Self::collect_subst_by_name(&blanked, &ty, &mut bind);
                    }
                    let dict_args = self.build_body_dict_args(&constraints, &bind, ev);
                    let n_params = self.dict_fn_arity(&name).unwrap_or(0);
                    let inst_fn_ty = decl_ty.map(|mut t| {
                        for (v, s) in &bind {
                            t = Self::subst_ty_by_name(&t, v, s);
                        }
                        t
                    });
                    return Self::saturate_dict_call(
                        &name, n_params, dict_args, Vec::new(), ty, inst_fn_ty);
                }
                return expr;
            }
            TExprKind::App(_, _) => {
                // A saturated constrained-existential constructor whose hidden
                // type is this body's class variable: the first pass could not
                // build the dictionary (no concrete type); here it is the
                // body's own dictionary parameter.
                if let Some(ex_name) = self.ex_saturated_ctor(&expr)
                    && let Some(packed) = self.ex_pack_saturated(ex_name, expr.clone(), false, ev)
                {
                    return self.map_children_scoped(packed, &mut |m, c| {
                        m.rewrite_dict_expr(c, func_name, ev)
                    });
                }
                let head_name = Self::app_head_name(&expr);
                // A call to the function itself, OR to any OTHER constrained
                // polymorphic function at a still-polymorphic type. Both need
                // dictionaries: the recursive call because that is the point
                // of dictionary passing, the sibling call (gdTag calling
                // gdConBody at `C1 c f`) because inside this polymorphic body
                // no specialization can ever resolve it — its dictionaries
                // must be built from this function's own parameters. The
                // callee is marked dict-passing so run()'s worklist rewrites
                // it too.
                // The node type being polymorphic marks a callee that can
                // never specialize; a callee ALREADY flagged dict-passing
                // needs dictionaries even when this call's result type is
                // concrete (`gshow x` inside this body returns String, but
                // gshow's dictionary-form still takes the Show dictionary
                // first).
                let general_callee = |s: &Self, name: &str, t: &Ty| {
                    name != func_name
                        && s.poly_fns.contains_key(name)
                        && s.fn_constraints.get(name).is_some_and(|cs| !cs.is_empty())
                        && (s.is_polymorphic(t) || s.dict_passing_fns.contains(name))
                };
                if let Some(call_name) = head_name
                    && !self.locals.contains(&call_name)
                    && (call_name == func_name || general_callee(self, &call_name, &ty)) {
                        if call_name != *func_name {
                            self.dict_passing_fns.insert(call_name.clone());
                        }
                        let (_, args) = Self::split_app_chain(expr);
                        // The call may be at a CHANGED type — a recursion at
                        // `[a]`, a sibling at this instance's `C1 c f`. Bind
                        // the declared signature against the argument types
                        // to find what each constrained variable stands for
                        // at THIS call, and build the dictionary for that
                        // type: the plain parameter when it is still an
                        // enclosing constrained variable, a constructed
                        // dictionary otherwise. (Passing the enclosing
                        // parameter unconditionally handed the `a`
                        // dictionary to a `[a]` recursion.)
                        let constraints = self.fn_constraints
                            .get(&call_name).cloned().unwrap_or_default();
                        let decl_ty = self.poly_fns.get(&call_name).map(|f| f.ty.clone());
                        let mut bind: HashMap<String, Ty> = HashMap::new();
                        if let Some(dt) = &decl_ty {
                            self.match_fn_args(dt, &args, &mut bind);
                            // An argument cannot carry a variable that sits
                            // in an unapplied parameter or the RESULT: bind
                            // those by matching the remaining declared type
                            // against the node type (families blanked —
                            // they are not injective, see
                            // `class_var_binding`). This must run for
                            // SATURATED calls too: `head (defAt (n - 1))`
                            // recursing at `[a]` carries the changed type
                            // only in its result, and matching nothing
                            // handed the ENCLOSING dictionary to a
                            // one-level-deeper recursion (F6b's body-side
                            // twin). Over-saturated calls keep the
                            // args-only path — the node type sits past the
                            // declared result, so the suffix match would
                            // mis-align.
                            if args.len() <= Self::spine_arrow_count(dt) {
                                let mut remaining = dt;
                                for _ in 0..args.len() {
                                    if let Ty::Arrow(_, to, _) = remaining {
                                        remaining = to;
                                    }
                                }
                                let remaining = self.blank_family_apps(remaining);
                                let node_ty = self.reduce_families(&ty).unwrap_or_else(|| ty.clone());
                                Self::collect_subst_by_name(&remaining, &node_ty, &mut bind);
                            }
                        }
                        let mut dict_args = self.build_body_dict_args(&constraints, &bind, ev);
                        if dict_args.is_empty() {
                            // No recorded constraints (defensive): pass the
                            // enclosing function's dictionary parameters
                            // through, in constraint declaration order —
                            // `cur_dict_params` holds them ordered, whereas
                            // enumerating `class_to_dict` (a HashMap) would
                            // pass them in nondeterministic order.
                            dict_args = self.cur_dict_params.iter().map(|(_, dp)| {
                                TExpr::new(TExprKind::Var(dp.clone()), Ty::Unit)
                            }).collect();
                        }
                        let value_args: Vec<TExpr> = args.into_iter()
                            .map(|a| self.rewrite_dict_expr(a, func_name, ev))
                            .collect();
                        let n_params = self.dict_fn_arity(&call_name)
                            .unwrap_or(value_args.len());
                        let inst_fn_ty = decl_ty.map(|mut t| {
                            for (v, s) in &bind {
                                t = Self::subst_ty_by_name(&t, v, s);
                            }
                            t
                        });
                        return Self::saturate_dict_call(
                            &call_name, n_params, dict_args, value_args, ty, inst_fn_ty);
                    }
                // Not a dict-passing call: generic descent below.
                expr.kind
            }
            TExprKind::InfixApp { op, lhs, rhs } => {
                // An operator that is a class method (`==`, `/=`, `<>`, a
                // user class's operator) at a binding this body has
                // evidence for: read it from the dictionary. The decision
                // is made on the operator's own instantiated type, so an
                // operator whose class variable sits in the RIGHT operand
                // (`2 *. x` for `(*.) :: Int -> a -> a`) is rewritten too.
                // Left unrewritten, a dict-form `x /= y` reached codegen as
                // a raw InfixApp and compiled to Lua's `~=` — table
                // identity, ignoring the instance's method entirely (F6d).
                if !self.locals.contains(&op) && self.method_to_class.contains_key(&op) {
                    let op_ty = Ty::fun(&[lhs.ty.clone(), rhs.ty.clone()], ty.clone());
                    let method_expr: Option<TExpr> = match self.dispatch_method(&op, &op_ty, ev) {
                        MethodUse::Impl(mangled) => Some(TExpr::new(TExprKind::Var(mangled), Ty::Unit)),
                        MethodUse::Evidence(kind) => Some(TExpr::new(kind, Ty::Unit)),
                        MethodUse::Native | MethodUse::Deferred | MethodUse::Missing(_) => None,
                    };
                    if let Some(method_expr) = method_expr {
                        let lhs = self.rewrite_dict_expr(*lhs, func_name, ev);
                        let rhs = self.rewrite_dict_expr(*rhs, func_name, ev);
                        return Self::method_operator_app(method_expr, lhs, rhs, ty);
                    }
                }
                // An InfixApp whose operator is not a dict method: generic
                // descent below, like every other structural node.
                TExprKind::InfixApp { op, lhs, rhs }
            }
            // A FIRST-CLASS operator or section of a class method (`zipWith
            // (==) xs ys`, `foldr (+) 0 xs`) at a binding this body has
            // evidence for: the dictionary's method as a value.
            TExprKind::OpFunc(ref op) => {
                let op = op.clone();
                if !self.locals.contains(&op) && self.method_to_class.contains_key(&op) {
                    match self.dispatch_method(&op, &ty, ev) {
                        MethodUse::Impl(mangled) => return TExpr { kind: TExprKind::Var(mangled), ty },
                        MethodUse::Evidence(kind) => return TExpr { kind, ty },
                        MethodUse::Native | MethodUse::Deferred | MethodUse::Missing(_) => {}
                    }
                }
                TExprKind::OpFunc(op)
            }
            // Prefix negation at a binding this body has evidence for: the
            // dictionary's `negate`. Left as a Negate node it reached
            // codegen as Lua's unary minus on a table (L14).
            TExprKind::Negate(inner) => {
                let inner = self.rewrite_dict_expr(*inner, func_name, ev);
                match self.negate_use(&ty, inner, ev) {
                    Ok(e) => return e,
                    Err(inner) => return TExpr { kind: TExprKind::Negate(Box::new(inner)), ty },
                }
            }
            // An unsaturated constrained-existential constructor at this
            // body's class variable (see the Con arm of mono_expr_node).
            TExprKind::Con(ref name) if self.ex_ctors.contains_key(name) => {
                let name = name.clone();
                match self.ex_eta_expand(&name, ty.clone(), ev) {
                    Some(kind) => kind,
                    None => TExprKind::Con(name),
                }
            }
            // A numeric literal at a binding this body has evidence for
            // (the class type variable, or a type over it) is
            // `fromInteger n` / `fromRational r` through the DICTIONARY —
            // GHC's overloaded-literal semantics. The main walk left it raw
            // (deferred), and a raw machine literal reaching an Integer
            // instance's `*` ("attempt to index a number") or a user data
            // instance's pattern match was a crash / wrong answer past the
            // specialization limit.
            TExprKind::Lit(lit @ (TLiteral::Integer(_) | TLiteral::Number(_) | TLiteral::BigInteger(_))) => {
                match self.literal_use(lit, &ty, ev) {
                    Ok(e) => return e,
                    Err(lit) => TExprKind::Lit(lit),
                }
            }
            other => other,
        };
        self.map_children_scoped(TExpr { kind, ty }, &mut |m, c| {
            m.rewrite_dict_expr(c, func_name, ev)
        })
    }

    /// Dictionary arguments for a use of a dict-passing function INSIDE a
    /// dictionary-form body: each constraint's variable resolved through
    /// `bind` (this use's substitution), built via `build_dict_expr` — the
    /// enclosing body's own dictionary parameter when it resolves to an
    /// enclosing constrained variable, a constructed dictionary otherwise.
    /// An unresolved variable falls back to the constraint's own variable
    /// name, which `env` matches by name (the recursion-at-`a` case).
    fn build_body_dict_args(
        &mut self,
        constraints: &[TyConstraint],
        bind: &HashMap<String, Ty>,
        ev: &DictEnv,
    ) -> Vec<TExpr> {
        constraints.iter().map(|c| {
            let bound = bind.get(&c.type_var).cloned()
                .unwrap_or_else(|| Ty::Var(TyVar { name: c.type_var.clone(), id: 0 }));
            self.build_dict_expr(&c.class_name, &bound, ev)
        }).collect()
    }

    /// Decompose nested App into (head_function, [arg1, arg2, ...])
    /// The head of an application spine (`f` in `f a b c`), borrowed — the
    /// test a rewrite makes BEFORE deciding to take the spine apart. (An
    /// earlier version cloned every argument just to look at the head, and
    /// again on the match; nested spines made that quadratic per pass.)
    fn app_head(expr: &TExpr) -> &TExpr {
        let mut e = expr;
        while let TExprKind::App(f, _) = &e.kind {
            e = f.as_ref();
        }
        e
    }

    /// The name at the head of an application spine, if it is a variable.
    fn app_head_name(expr: &TExpr) -> Option<String> {
        match &Self::app_head(expr).kind {
            TExprKind::Var(n) => Some(n.clone()),
            _ => None,
        }
    }

    /// Take an application spine apart, owning: the head and its arguments in
    /// call order. No clones — the caller has decided to rebuild the call.
    fn split_app_chain(expr: TExpr) -> (TExpr, Vec<TExpr>) {
        let mut args = Vec::new();
        let mut e = expr;
        loop {
            match e.kind {
                TExprKind::App(f, a) => {
                    args.push(*a);
                    e = *f;
                }
                kind => {
                    args.reverse();
                    return (TExpr { kind, ty: e.ty }, args);
                }
            }
        }
    }

    /// Rewrite call sites to dict-passing functions.
    fn rewrite_dict_call_sites(&mut self, expr: TExpr) -> TExpr {
        let ty = expr.ty.clone();
        match expr.kind {
            TExprKind::App(_, _) => {
                let head_name = Self::app_head_name(&expr);
                if let Some(call_name) = head_name
                    && self.dict_passing_fns.contains(&call_name)
                        && !self.locals.contains(&call_name) && !self.is_polymorphic(&ty)
                        && self.fn_constraints.contains_key(&call_name) {
                            let n_args = Self::app_chain_len(&expr);
                            if self.dictify_feasible(&call_name, n_args, &ty) {
                                let (_head, args) = Self::split_app_chain(expr);
                                return self.dictify_use(&call_name, args, ty);
                            }
                        }
                // Not a dict-passing call: generic descent (which also
                // covers the App operands themselves).
                self.map_children_scoped(expr, &mut |m, c| m.rewrite_dict_call_sites(c))
            }
            // A FIRST-CLASS reference to a dict-passing function (`map gshow
            // xs`): the bare name compiles to the dictionary-form function,
            // whose first Lua parameters are the dictionaries — the caller
            // (`map`) would pass a list element where a dictionary is
            // expected. Eta-expand at the use's concrete type: a lambda over
            // the value parameters wrapping a saturated DictCall whose
            // dictionaries are built here, at compile time, from that type.
            TExprKind::Var(ref name)
                if self.dict_passing_fns.contains(name) && !self.locals.contains(name)
                    && self.fn_constraints.get(name).is_some_and(|c| !c.is_empty()) =>
            {
                let name = name.clone();
                if self.is_polymorphic(&ty) {
                    // No concrete type to build the dictionaries at. GHC
                    // would pass dictionaries at runtime; mata-ll builds
                    // them at compile time, so this use cannot be compiled —
                    // report it rather than emit the silent miscompile.
                    self.push_first_class_dict_error(&name, &ty);
                    return TExpr { kind: TExprKind::Var(name), ty };
                }
                if self.dictify_feasible(&name, 0, &ty) {
                    self.dictify_use(&name, Vec::new(), ty)
                } else {
                    TExpr { kind: TExprKind::Var(name), ty }
                }
            }
            _ => self.map_children_scoped(expr, &mut |m, c| m.rewrite_dict_call_sites(c)),
        }
    }

    /// Number of arguments an application spine supplies.
    fn app_chain_len(expr: &TExpr) -> usize {
        let mut n = 0;
        let mut e = expr;
        while let TExprKind::App(f, _) = &e.kind {
            n += 1;
            e = f.as_ref();
        }
        n
    }

    /// Does this use of polymorphic `name` at concrete `use_ty` need its own
    /// specialization? Every ordinary polymorphic function is specialized per
    /// instantiation. A builtin (`arity_only_fns`) is shared instead — its
    /// one compiled copy serves every use — EXCEPT where the instantiation
    /// widens its arity, which the shared copy cannot serve: codegen emits it
    /// with the declared type's arrow count of parameters, while this use
    /// would hand it the instantiated count (`const inc "x" 3` against
    /// `const :: a -> b -> a`). There the arity, not the type, is what forces
    /// a separate copy.
    fn specialization_wanted(&self, name: &str, use_ty: &Ty) -> bool {
        if !self.arity_only_fns.contains(name) {
            return true;
        }
        self.arity_widening(name, use_ty)
    }

    /// Does this use instantiate `name` at MORE arrows than its declared
    /// type has — i.e. a result type variable became a function? Such a use
    /// cannot be served by any copy compiled at fewer arrows: the emitted
    /// parameter list and every call inside the body count arrows from the
    /// type they can see, and the two views disagree exactly here (the
    /// 76923e0 invariant).
    fn arity_widening(&self, name: &str, use_ty: &Ty) -> bool {
        self.poly_fns.get(name).is_some_and(|f|
            Self::spine_arrow_count(use_ty) > Self::spine_arrow_count(&f.ty))
    }

    /// The arrow shape of a use of shared (`arity_only_fns`) or
    /// dictionary-passing `name` at `use_ty`: the key of the copy that
    /// serves every use of that shape (`widening_copy`). It is the declared
    /// type with the image of each UNCONSTRAINED variable reduced to its
    /// arrow skeleton (`arrow_skeleton`); a constrained variable stays the
    /// declared variable itself, so the shape of a constrained function is
    /// a constrained polymorphic type over the original's variables, which
    /// its dictionary form is built from. The body can observe nothing
    /// about an unconstrained variable's image except its arrows, which the
    /// N-ary convention turns into parameters and flat calls
    /// (`residual_vars_dead` makes the same argument), so the copy compiled
    /// at the shape is the right code for every type of that shape, and a
    /// function has as many copies as it has widening shapes — a few —
    /// rather than one per type. Deterministic: the skeleton variables are
    /// numbered over the declared variables in name order. Falls back to
    /// the canonicalized use type when the declared type does not match the
    /// use (defensive: a checked use type is an instance of it).
    fn arity_shape(&self, name: &str, use_ty: &Ty) -> Ty {
        let fallback = Self::canonicalize_vars(use_ty);
        let Some(poly_fn) = self.poly_fns.get(name) else { return fallback };
        let mut live: HashSet<String> = HashSet::new();
        if let Some(cs) = self.fn_constraints.get(name) {
            live.extend(cs.iter().map(|c| c.type_var.clone()));
        }
        if let Some(args) = self.fn_constraint_args.get(name) {
            for (_, arg) in args {
                live.extend(arg.free_vars().into_iter().map(|v| v.name));
            }
        }
        let mut images: HashMap<String, Ty> = HashMap::new();
        Self::collect_subst_by_name(&poly_fn.ty, use_ty, &mut images);
        let mut images: Vec<(String, Ty)> = images.into_iter().collect();
        images.sort_by(|a, b| a.0.cmp(&b.0));
        let mut fresh = 0u32;
        let mut shape = poly_fn.ty.clone();
        for (declared, image) in &images {
            if live.contains(declared) {
                continue;
            }
            let to = Self::arrow_skeleton(image, &mut fresh);
            shape = Self::subst_ty_by_name(&shape, declared, &to);
        }
        if Self::spine_arrow_count(&shape) != Self::spine_arrow_count(use_ty) {
            return fallback;
        }
        shape
    }

    /// `ty` with every arrow-free subterm replaced by a fresh variable, so
    /// only its arrows (and the constructor heads over them) survive — see
    /// `arity_shape`. `Maybe (Int -> Int)` becomes `Maybe (_s0 -> _s1)`,
    /// `Int` becomes `_s0`.
    fn arrow_skeleton(ty: &Ty, fresh: &mut u32) -> Ty {
        fn has_arrow(ty: &Ty) -> bool {
            match ty {
                Ty::Arrow(..) => true,
                Ty::App(a, b) => has_arrow(a) || has_arrow(b),
                Ty::List(t) | Ty::IO(t) | Ty::LuaIO(_, t) | Ty::Forall(_, t) => has_arrow(t),
                Ty::Tuple(ts) => ts.iter().any(has_arrow),
                Ty::Var(_) | Ty::Con(_) | Ty::Unit | Ty::Skolem(..) | Ty::Promoted(_) => false,
            }
        }
        // The head of a type application is kept as it is: a fresh
        // variable in constructor position would be a higher-kinded hole.
        fn head(ty: &Ty, fresh: &mut u32) -> Ty {
            match ty {
                Ty::App(a, b) => Ty::App(
                    Box::new(head(a, fresh)),
                    Box::new(Monomorphizer::arrow_skeleton(b, fresh)),
                ),
                Ty::Var(_) | Ty::Con(_) | Ty::Skolem(..) | Ty::Promoted(_) => ty.clone(),
                other => Monomorphizer::arrow_skeleton(other, fresh),
            }
        }
        if !has_arrow(ty) {
            let n = *fresh;
            *fresh += 1;
            return Ty::Var(TyVar { name: format!("_s{n}"), id: n });
        }
        match ty {
            Ty::Arrow(a, b, m) => Ty::Arrow(
                Box::new(Self::arrow_skeleton(a, fresh)),
                Box::new(Self::arrow_skeleton(b, fresh)),
                *m,
            ),
            Ty::App(a, b) => Ty::App(
                Box::new(head(a, fresh)),
                Box::new(Self::arrow_skeleton(b, fresh)),
            ),
            Ty::List(t) => Ty::List(Box::new(Self::arrow_skeleton(t, fresh))),
            Ty::IO(t) => Ty::IO(Box::new(Self::arrow_skeleton(t, fresh))),
            Ty::LuaIO(s, t) => Ty::LuaIO(s.clone(), Box::new(Self::arrow_skeleton(t, fresh))),
            Ty::Forall(vs, t) => Ty::Forall(vs.clone(), Box::new(Self::arrow_skeleton(t, fresh))),
            Ty::Tuple(ts) => Ty::Tuple(ts.iter().map(|t| Self::arrow_skeleton(t, fresh)).collect()),
            Ty::Var(_) | Ty::Con(_) | Ty::Unit | Ty::Skolem(..) | Ty::Promoted(_) => ty.clone(),
        }
    }

    /// Are all the type variables still free in `use_ty` DEAD for a use of
    /// polymorphic `name` there? A residual variable is dead when it lies
    /// only in the image of declared type variables the function's context
    /// leaves unconstrained. Such a variable cannot reach the body: no
    /// method dispatches on an unconstrained signature variable (the
    /// checker rejects `show x` at a bare `a` without `Show a` in the
    /// context), and no dictionary is ever built for it — so the body
    /// compiled at `use_ty` is the same code for every instantiation of the
    /// variable, and the use can be specialized at the canonicalized type
    /// exactly as a concrete use would be. A residual variable inside the
    /// image of a CONSTRAINED declared variable (`a := Maybe t1` under
    /// `Show a`) is live: the specialization would need a `Show t1`
    /// dictionary nobody can name. Constraint arguments count as
    /// constraining every variable they mention (`GEncode (Rep a)`). A
    /// residual variable no declared variable accounts for (the declared
    /// type could not be matched against the use type) is treated as live
    /// — the conservative answer.
    fn residual_vars_dead(&self, name: &str, use_ty: &Ty) -> bool {
        let Some(poly_fn) = self.poly_fns.get(name) else { return false };
        let residual = use_ty.free_vars();
        if residual.is_empty() {
            return true;
        }
        let mut live: HashSet<String> = HashSet::new();
        if let Some(cs) = self.fn_constraints.get(name) {
            live.extend(cs.iter().map(|c| c.type_var.clone()));
        }
        if let Some(args) = self.fn_constraint_args.get(name) {
            for (_, arg) in args {
                live.extend(arg.free_vars().into_iter().map(|v| v.name));
            }
        }
        let mut images: HashMap<String, Ty> = HashMap::new();
        Self::collect_subst_by_name(&poly_fn.ty, use_ty, &mut images);
        let mut accounted: HashSet<TyVar> = HashSet::new();
        for (declared, image) in &images {
            let vars = image.free_vars();
            if vars.is_empty() {
                continue;
            }
            if live.contains(declared) {
                return false;
            }
            accounted.extend(vars);
        }
        residual.iter().all(|v| accounted.contains(v))
    }

    /// Rename every type variable to a canonical one in first-occurrence
    /// order, so two structurally identical still-polymorphic types key the
    /// SAME dead-variable specialization (see the Var arm) regardless of
    /// which fresh unification variables the checker happened to mint.
    /// Structure is untouched — only `Var` identities change — so the
    /// canonical type has exactly the arrow shape an arity-widening copy
    /// exists to fix.
    fn canonicalize_vars(ty: &Ty) -> Ty {
        fn walk(ty: &Ty, map: &mut HashMap<(String, u32), u32>) -> Ty {
            match ty {
                Ty::Var(v) => {
                    let n = map.len() as u32;
                    let id = *map.entry((v.name.clone(), v.id)).or_insert(n);
                    Ty::Var(crate::types::TyVar { name: format!("_w{id}"), id })
                }
                Ty::Arrow(a, b, m) => Ty::Arrow(
                    Box::new(walk(a, map)),
                    Box::new(walk(b, map)),
                    m.clone(),
                ),
                Ty::App(a, b) => {
                    Ty::App(Box::new(walk(a, map)), Box::new(walk(b, map)))
                }
                Ty::List(t) => Ty::List(Box::new(walk(t, map))),
                Ty::IO(t) => Ty::IO(Box::new(walk(t, map))),
                Ty::Tuple(ts) => Ty::Tuple(ts.iter().map(|t| walk(t, map)).collect()),
                // Variants that either carry no nested value types the
                // arrow shape depends on, or cannot appear in an
                // arity-widening builtin's instantiation (LuaIO's scope
                // variable, rank-2 foralls, skolems): kept as they are —
                // a use containing them simply keys on itself.
                other => other.clone(),
            }
        }
        walk(ty, &mut HashMap::new())
    }

    /// Number of leading arrows in a type (the arguments it can still take).
    fn spine_arrow_count(ty: &Ty) -> usize {
        let mut n = 0;
        let mut t = ty;
        while let Ty::Arrow(_, to, _) = t {
            n += 1;
            t = to.as_ref();
        }
        n
    }

    /// The value arity of a dict-passing function: the arrow count of its
    /// DECLARED type. That is codegen's N-ary convention (`count_arrows` in
    /// codegen/util.rs) — a definition with fewer patterns than arrows (a
    /// body that returns a lambda, a point-free alias) is `_eta`-padded to
    /// the full arrow count when emitted, so the dictionary-form Lua
    /// function takes (dicts.., one value per arrow).
    fn dict_fn_arity(&self, name: &str) -> Option<usize> {
        self.poly_fns.get(name).map(|f| Self::spine_arrow_count(&f.ty))
    }

    /// An under-saturated use can only be eta-expanded if the node's type
    /// exposes an arrow for every missing parameter (always true for a
    /// checked program; this guards the rewrite against ever emitting an
    /// under-saturated DictCall from a malformed type).
    fn dictify_feasible(&self, name: &str, n_args: usize, node_ty: &Ty) -> bool {
        match self.dict_fn_arity(name) {
            Some(n_params) if n_args < n_params =>
                Self::spine_arrow_count(node_ty) >= n_params - n_args,
            Some(_) => true,
            None => false,
        }
    }

    /// Record an error for a first-class reference to a dict-passing
    /// function at a type that is still polymorphic: there is no concrete
    /// type to build its dictionaries at.
    fn push_first_class_dict_error(&mut self, name: &str, ty: &Ty) {
        let msg = format!(
            "cannot pass '{}' as a function value at the polymorphic type '{}'",
            name, ty);
        // One error per (message, site). The rewrite worklist re-walks a
        // function whenever a new dict-passing flag appears, and a guarded
        // clause presents the same expression through several top-level
        // slots — without this, one root cause surfaced as a cascade of
        // identical diagnostics.
        if self.deferred_errors.iter().any(|(_, e)| e.span == self.cur_span
            && e.context == self.cur_ctx
            && matches!(&e.kind, DiagnosticKind::Other(m) if *m == msg))
        {
            return;
        }
        let diag = Diagnostic {
            kind: DiagnosticKind::Other(msg.clone()),
            context: self.cur_ctx.clone(),
            span: self.cur_span,
            file: None,
            notes: vec![
                match self.dict_passing_counts.get(name) {
                    Some(n) => format!(
                        "'{}' is compiled with dictionary passing: it reached \
                         {} distinct type instantiations (the limit is {}, a \
                         guard against polymorphic-recursion ladders — one \
                         genuine instantiation too many trips it the same \
                         way). Note \
                         that the instantiation that crossed the limit may be \
                         far from this use site.",
                        name, n, SPEC_LIMIT),
                    None => format!(
                        "'{}' is compiled with dictionary passing (it \
                         exceeded the specialization limit of {}).",
                        name, SPEC_LIMIT),
                },
                "a first-class use needs one concrete type to build the \
                 dictionaries at compile time. GHC resolves this by passing \
                 dictionaries at runtime; in mata-ll, annotate the use so \
                 its type is concrete.".to_string(),
            ],
            baseline: false, excerpt: None, names: None,
        };
        // Deferred to reachability (see `deferred_errors`): the enclosing
        // function is usually a generic original that dies once every real
        // use specialized, and dead code cannot hit a backend limitation.
        match &self.cur_rewrite_fn {
            Some(f) => self.deferred_errors.push((f.clone(), diag)),
            // Defensive: no enclosing function recorded — report eagerly
            // rather than lose the diagnosis.
            None => self.errors.push(diag),
        }
    }

    /// Build a SATURATED use of dict-passing `call_name` from `args` (the
    /// supplied application spine — possibly empty, for a first-class
    /// reference) at concrete node type `node_ty`. The emitted DictCall's
    /// value arguments always match the function's value arity: missing
    /// arguments are eta-expanded into an enclosing lambda, and extra
    /// arguments (a call into a function-returning body) become ordinary
    /// applications of the DictCall's result. The dictionary-form Lua
    /// function is called with exactly its declared parameters either way —
    /// an unsaturated direct call would misalign values against dictionary
    /// slots (too few) or drop arguments (too many).
    fn dictify_use(&mut self, call_name: &str, mut args: Vec<TExpr>, node_ty: Ty) -> TExpr {
        let constraints = self.fn_constraints.get(call_name).cloned().unwrap_or_default();
        let poly_fn_ty = self.poly_fns.get(call_name).map(|f| f.ty.clone());
        let n_params = self.dict_fn_arity(call_name).unwrap_or(args.len());
        let no_ev = DictEnv::default();
        // Signature variables := this use's types. Supplied argument types
        // bind variables in applied positions; matching the REMAINING
        // declared type against the node type binds variables that sit in
        // unapplied parameters or the result — a first-class or partial
        // reference has no argument carrying them. Family applications in
        // the declared type are blanked before matching (they are not
        // injective; matching them would bind a variable to a piece of the
        // family's reduct — see `class_var_binding`).
        let mut subst: HashMap<String, Ty> = HashMap::new();
        if let Some(ft) = &poly_fn_ty {
            self.match_fn_args(ft, &args, &mut subst);
            let mut remaining = ft;
            for _ in 0..args.len().min(Self::spine_arrow_count(ft)) {
                if let Ty::Arrow(_, to, _) = remaining {
                    remaining = to;
                }
            }
            let remaining = self.blank_family_apps(remaining);
            // The node type is reduced like the argument types
            // (`match_fn_args`): a specialized body's result type can be
            // an unreduced family application too.
            let node_red = self.reduce_families(&node_ty).unwrap_or_else(|| node_ty.clone());
            Self::collect_subst_by_name(&remaining, &node_red, &mut subst);
        }
        // For a COMPOUND constraint (`GEncode (Rep a)`) the recorded
        // argument type drives construction: substitute this use's types
        // for the signature variables, reduce any family (`Rep T` -> the
        // representation), then build the dictionary over it. A plain
        // `Class a` prefers the full substitution at EVERY arity: it also
        // binds a variable that occurs only in an unapplied parameter or
        // the RESULT (matched against the node type above), which the
        // argument-driven resolver below cannot see — a saturated
        // `defAt 3 :: Int` with `defAt :: Mk a => Int -> a` used to fall
        // to the resolver's `_` placeholder and build a dictionary that
        // resolved no method (F6b). The argument-driven resolver stays as
        // the fallback for a variable the substitution missed.
        let arg_tys = self.fn_constraint_args.get(call_name).cloned();
        let mut dict_args: Vec<TExpr> = Vec::new();
        for (i, c) in constraints.iter().enumerate() {
            let compound_arg = arg_tys.as_ref()
                .and_then(|a| a.get(i))
                .filter(|(_, arg)| !matches!(arg, Ty::Var(_)));
            if let Some((cls, arg)) = compound_arg {
                let mut concrete = arg.clone();
                for (v, t) in &subst {
                    concrete = Self::subst_ty_by_name(&concrete, v, t);
                }
                let concrete = self.reduce_families(&concrete)
                    .unwrap_or(concrete);
                dict_args.push(self.build_dict_expr(cls, &concrete, &no_ev));
            } else {
                let concrete = subst.get(&c.type_var).cloned().unwrap_or_else(||
                    self.resolve_constraint_type(&c.type_var, poly_fn_ty.as_ref(), &args));
                dict_args.push(self.build_dict_expr(&c.class_name, &concrete, &no_ev));
            }
        }
        let args: Vec<TExpr> = args.drain(..)
            .map(|a| self.rewrite_dict_call_sites(a))
            .collect();
        // The callee's declared type at this use's substitution — the
        // over-saturated case peels applied parameters off it to type the
        // inner DictCall node.
        let inst_fn_ty = poly_fn_ty.map(|mut t| {
            for (v, s) in &subst {
                t = Self::subst_ty_by_name(&t, v, s);
            }
            self.reduce_families(&t).unwrap_or(t)
        });
        Self::saturate_dict_call(call_name, n_params, dict_args, args, node_ty, inst_fn_ty)
    }

    /// Assemble a use of dict-passing `call_name` from already-built
    /// dictionaries and already-rewritten supplied arguments, SATURATING to
    /// the function's value arity `n_params`. The dictionary-form Lua
    /// function is a direct call with exactly (dicts.., values..)
    /// parameters, so an unsaturated DictCall misaligns values against
    /// dictionary slots (too few arguments) or silently drops arguments
    /// (too many):
    /// - exactly n_params arguments: a plain DictCall;
    /// - fewer (a first-class reference or partial application): a lambda
    ///   over the missing parameters — their types peeled off the node
    ///   type, which IS the remaining function type — wrapping the
    ///   saturated DictCall;
    /// - more (the body returns a function that the call applies further):
    ///   a DictCall over the first n_params arguments, applied to the rest
    ///   with ordinary Apps.
    fn saturate_dict_call(
        call_name: &str,
        n_params: usize,
        dict_args: Vec<TExpr>,
        mut args: Vec<TExpr>,
        node_ty: Ty,
        inst_fn_ty: Option<Ty>,
    ) -> TExpr {
        if args.len() == n_params {
            return TExpr {
                kind: TExprKind::DictCall {
                    func_name: call_name.to_string(),
                    dict_args,
                    value_args: args,
                },
                ty: node_ty,
            };
        }
        if args.len() < n_params {
            let missing = n_params - args.len();
            let mut params: Vec<(String, Ty)> = Vec::new();
            let mut rest = &node_ty;
            for i in 0..missing {
                if let Ty::Arrow(from, to, _) = rest {
                    params.push((format!("__eta{}", i), (**from).clone()));
                    rest = to;
                }
            }
            let mut value_args = args;
            for (p, pty) in &params {
                value_args.push(TExpr {
                    kind: TExprKind::Var(p.clone()),
                    ty: pty.clone(),
                });
            }
            let body = TExpr {
                kind: TExprKind::DictCall {
                    func_name: call_name.to_string(),
                    dict_args,
                    value_args,
                },
                ty: rest.clone(),
            };
            return TExpr {
                kind: TExprKind::Lambda { params, body: Box::new(body) },
                ty: node_ty,
            };
        }
        let extra = args.split_off(n_params);
        let mut dc_ty = inst_fn_ty.unwrap_or_else(|| node_ty.clone());
        for _ in 0..n_params {
            if let Ty::Arrow(_, to, _) = dc_ty {
                dc_ty = *to;
            }
        }
        let mut result = TExpr {
            kind: TExprKind::DictCall {
                func_name: call_name.to_string(),
                dict_args,
                value_args: args,
            },
            ty: dc_ty,
        };
        let n_extra = extra.len();
        for (i, a) in extra.into_iter().enumerate() {
            let next_ty = if i + 1 == n_extra {
                node_ty.clone()
            } else if let Ty::Arrow(_, to, _) = &result.ty {
                (**to).clone()
            } else {
                node_ty.clone()
            };
            result = TExpr {
                kind: TExprKind::App(Box::new(result), Box::new(a)),
                ty: next_ty,
            };
        }
        result
    }

    fn resolve_constraint_type(&self, type_var: &str, poly_fn_ty: Option<&Ty>, args: &[TExpr]) -> Ty {
        if let Some(fn_ty) = poly_fn_ty {
            let mut subst = HashMap::new();
            self.match_fn_args(fn_ty, args, &mut subst);
            if let Some(ty) = subst.get(type_var) { return ty.clone(); }
        }
        if !args.is_empty()
            && let Some(inner) = Self::extract_inner_type(&args[0].ty) { return inner; }
        Ty::Con("_".into())
    }

    /// Bind the declared parameter types of `fn_ty` against the supplied
    /// arguments' types, by variable name. An argument's type is reduced
    /// first: a specialized body carries the family applications its
    /// substitution produced (`from x :: Rep Person`), and matched as they
    /// are they bind an instance's variables to the wrong pieces — `D1 d f`
    /// against `Rep Person` bound `f := Person` and left `d` to a fallback,
    /// so the `Datatype`/`GSum` dictionaries for a dictionary-passing
    /// `gEncode` were built at `Person` and resolved to nothing.
    fn match_fn_args(&self, fn_ty: &Ty, args: &[TExpr], subst: &mut HashMap<String, Ty>) {
        let mut param_ty = fn_ty;
        for arg in args {
            if let Ty::Arrow(from, to, _) = param_ty {
                let arg_ty = self.reduce_families(&arg.ty).unwrap_or_else(|| arg.ty.clone());
                Self::collect_subst_by_name(from, &arg_ty, subst);
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

    fn build_concrete_dict(&mut self, class_name: &str, concrete_ty: &Ty) -> TExpr {
        let class_info = match self.classes.get(class_name) {
            Some(ci) => ci.clone(),
            None => return TExpr::new(TExprKind::Lit(TLiteral::Unit), Ty::Unit),
        };
        let mut method_impls = Vec::new();
        for (method_name, method_sig) in &class_info.methods {
            // A method with constraints of its own (`traverse`'s
            // `Applicative f`) is polymorphic in a variable the class
            // variable does not fix, so no one specialization can be its
            // entry: the entry is the instance's dictionary form of the
            // method, which takes the method-level dictionaries first (see
            // `method_dict_constraints`). Before this, the entry was the
            // generic `traverse·[]`, whose `list_traverse` at the
            // applicative variable was a bare name — nil at runtime.
            if self.method_dict_constraints.contains_key(method_name)
                && let Some(head) = InstHead::of(concrete_ty)
                && let Some(methods) = self.ensure_dictform(class_name, &head)
                && let Some((_, dictform, _)) = methods.iter().find(|(m, _, _)| m == method_name)
            {
                method_impls.push((method_name.clone(), dictform.clone()));
                continue;
            }
            // Implementations already generated for this exact type (tuple/
            // list/Maybe show and eq) win; then full instance resolution —
            // the same specializing resolver ordinary method dispatch uses,
            // so a user instance's method lands on its real (specialized)
            // implementation, not a bare method name that resolves to
            // nothing at runtime; the raw method name is the last resort
            // (the shape-generic runtime fallbacks, e.g. `show`).
            let impl_name = self.generated_impls
                .get(&(method_name.clone(), concrete_ty.clone()))
                .cloned()
                .or_else(|| {
                    if concrete_ty.free_vars().is_empty() {
                        let use_ty = Self::subst_ty_by_name(
                            method_sig,
                            &class_info.type_var,
                            concrete_ty,
                        );
                        self.resolve_at_type(method_name, concrete_ty, &use_ty)
                    } else {
                        None
                    }
                })
                .or_else(|| InstHead::of(concrete_ty).and_then(|h| {
                    self.instance_methods.get(&(method_name.clone(), h)).cloned()
                }));
            let impl_name = match impl_name {
                Some(n) => n,
                None => {
                    // Last resort: the bare method name. Legitimate only
                    // when a definition actually exists under it — `show`
                    // is shape-generic at runtime, and a method that is a
                    // top-level polymorphic function has a generic copy to
                    // land on. Anything else would emit a reference that
                    // resolves to NOTHING (`==` sanitizes to
                    // `_usr_eq__eq_`, defined nowhere) and crash with a
                    // nil call at first use — report the compile error
                    // instead of emitting it (F6a).
                    // Reported once: a ground type's dictionary is decided
                    // again by every pass of `run`'s fixpoint.
                    let msg = format!(
                        "cannot build the '{}' dictionary for type '{}': \
                         method '{}' has no implementation at this type",
                        class_name, concrete_ty, method_name);
                    let reported = self.errors.iter()
                        .any(|e| matches!(&e.kind, DiagnosticKind::Other(m) if *m == msg));
                    if method_name != "show" && !self.poly_fns.contains_key(method_name) && !reported {
                        self.errors.push(Diagnostic {
                            kind: DiagnosticKind::Other(msg),
                            context: self.cur_ctx.clone(),
                            span: self.cur_span,
                            file: None,
                            notes: vec![format!(
                                "the enclosing function is compiled with dictionary \
                                 passing (it exceeded the specialization limit or is \
                                 polymorphically recursive), which needs every class \
                                 method resolved to a real implementation when a \
                                 dictionary is built; '{}' resolved to nothing at \
                                 '{}', and emitting the bare method name would crash \
                                 at runtime",
                                method_name, concrete_ty)],
                            baseline: false, excerpt: None, names: None,
                        });
                    }
                    method_name.clone()
                }
            };
            method_impls.push((method_name.clone(), impl_name));
        }
        let spec = SpecKind::Dict {
            class: class_name.to_string(),
            methods: method_impls,
        };
        TExpr::new(
            TExprKind::SpecCall {
                original: format!("__dict_{}", class_name),
                specialized: spec,
                args: vec![],
            },
            Ty::Unit,
        )
    }

    /// Substitute every type variable NAMED `var` in `ty` with `to` — the
    /// class variable of a method signature bound to a concrete type.
    fn subst_ty_by_name(ty: &Ty, var: &str, to: &Ty) -> Ty {
        match ty {
            Ty::Var(v) if v.name == var => to.clone(),
            Ty::Var(_) | Ty::Con(_) | Ty::Unit | Ty::Promoted(_) | Ty::Skolem(..) => ty.clone(),
            Ty::Arrow(a, b, m) => Ty::Arrow(
                Box::new(Self::subst_ty_by_name(a, var, to)),
                Box::new(Self::subst_ty_by_name(b, var, to)),
                *m,
            ),
            Ty::App(a, b) => Ty::App(
                Box::new(Self::subst_ty_by_name(a, var, to)),
                Box::new(Self::subst_ty_by_name(b, var, to)),
            ),
            Ty::List(a) => Ty::List(Box::new(Self::subst_ty_by_name(a, var, to))),
            Ty::IO(a) => Ty::IO(Box::new(Self::subst_ty_by_name(a, var, to))),
            Ty::LuaIO(s, a) => Ty::LuaIO(s.clone(), Box::new(Self::subst_ty_by_name(a, var, to))),
            Ty::Forall(vs, a) => Ty::Forall(vs.clone(), Box::new(Self::subst_ty_by_name(a, var, to))),
            Ty::Tuple(es) => Ty::Tuple(es.iter().map(|e| Self::subst_ty_by_name(e, var, to)).collect()),
        }
    }

    /// The dictionary-routed form of a class-method use whose instantiated
    /// method type is `use_ty`, given that `dict_evidence_for` holds for its
    /// binding: a plain access on the enclosing dictionary parameter when
    /// the class variable is bound to a bare type variable, the captured
    /// binder's method at an existential skolem, or an access on a
    /// dictionary EXPRESSION built for the bound type (`[a]`, `Maybe a`, a
    /// ground type) otherwise. A method with constraints of its own
    /// (`traverse`'s `Applicative f`) is applied to the dictionaries for
    /// those, built for the variables' bindings at this use — GHC's
    /// dictionary layout, where such an entry is a function of them.
    fn dict_method_use(
        &mut self,
        class_name: &str,
        method: &str,
        use_ty: &Ty,
        ev: &DictEnv,
    ) -> TExprKind {
        let binding = self.class_var_binding(method, use_ty);
        let access = match binding {
            Some(Ty::Var(v)) => {
                let param = ev.env
                    .get(&(class_name.to_string(), v.name.clone()))
                    .or_else(|| ev.class_to_dict.get(class_name))
                    .cloned();
                match param {
                    Some(p) => TExprKind::DictAccess {
                        dict_param: p,
                        method_name: method.to_string(),
                    },
                    None => TExprKind::Var(method.to_string()),
                }
            }
            // An existential skolem: the dictionary the enclosing
            // constructor match bound. Read through DictMethod on the
            // binder (a lazily-bound pattern variable, forced at the read),
            // not DictAccess (a raw parameter index).
            Some(Ty::Skolem(_, id)) => match self.skolem_dict_for(class_name, id) {
                Some(binder) => TExprKind::DictMethod {
                    dict: Box::new(TExpr::new(TExprKind::Var(binder), Ty::Unit)),
                    method_name: method.to_string(),
                },
                None => TExprKind::Var(method.to_string()),
            },
            Some(bound) => {
                // A compound binding (`Rep a`) that IS one of this function's
                // constraints (`GEncode (Rep a)`) takes the dictionary PARAMETER
                // standing for it — the whole point of dictionary passing —
                // rather than trying to rebuild a dictionary for a type that is
                // still polymorphic (which would bottom out at the raw, nil
                // method name).
                if let Some((_, _, param)) = self.cur_dict_by_arg.iter()
                    .find(|(cls, arg, _)| cls == class_name && Self::arg_matches(arg, &bound))
                {
                    TExprKind::DictAccess {
                        dict_param: param.clone(),
                        method_name: method.to_string(),
                    }
                } else {
                    let dict = self.build_dict_expr(class_name, &bound, ev);
                    TExprKind::DictMethod {
                        dict: Box::new(dict),
                        method_name: method.to_string(),
                    }
                }
            }
            None => {
                // The binding could not be recovered (an unusual method
                // shape): the enclosing parameter.
                match ev.class_to_dict.get(class_name) {
                    Some(p) => TExprKind::DictAccess {
                        dict_param: p.clone(),
                        method_name: method.to_string(),
                    },
                    None => TExprKind::Var(method.to_string()),
                }
            }
        };
        if matches!(access, TExprKind::Var(_)) {
            return access;
        }
        let Some(extra) = self.method_dict_constraints.get(method).cloned() else {
            return access;
        };
        // The method's own variables, bound by matching its declared type
        // (families blanked — see `class_var_binding`) against the use.
        let mut bind: HashMap<String, Ty> = HashMap::new();
        if let Some(info) = self.classes.get(class_name)
            && let Some((_, decl_ty)) = info.methods.iter().find(|(n, _)| n == method)
        {
            let decl_ty = self.blank_family_apps(decl_ty);
            Self::collect_subst_by_name(&decl_ty, use_ty, &mut bind);
        }
        let mut applied = TExpr::new(access, Ty::Unit);
        for c in &extra {
            let bound = bind.get(&c.type_var).cloned()
                .unwrap_or_else(|| Ty::Var(TyVar { name: c.type_var.clone(), id: 0 }));
            let dict = self.build_dict_expr(&c.class_name, &bound, ev);
            applied = TExpr::new(TExprKind::App(Box::new(applied), Box::new(dict)), Ty::Unit);
        }
        applied.kind
    }

    /// Is the instance method for `class_name` at `ty`'s head constructor
    /// dictionary-passing (purged for tripping the specialization cap)? Such a
    /// method cannot be specialized to one function for a concrete type, so its
    /// dictionary must be COMPOSED from the parameterized instance rather than
    /// looked up whole. False for a type with no registered instance head.
    fn head_method_is_dict_passing(&self, class_name: &str, ty: &Ty) -> bool {
        InstHead::of(ty)
            .and_then(|h| self.instances.get(&(class_name.to_string(), h)))
            .is_some_and(|inst| inst.context.is_some()
                && inst.method_fns.values().any(|f| self.dict_passing_fns.contains(f)))
    }

    /// Build a runtime dictionary EXPRESSION for `class_name` at type `ty`,
    /// inside a dictionary-passing body whose own dictionaries are `ev`'s
    /// parameters (empty in the main walk, where the only evidence is the
    /// captured dictionary of an enclosing existential match). The
    /// decision tree `dict_evidence_for` mirrors:
    ///
    ///   - the constrained variable itself -> the parameter;
    ///   - an existential skolem -> the captured binder;
    ///   - a ground type -> a `DictDemand` placeholder, decided after the
    ///     fixpoint of `run` (`materialize_dicts` → `decide_dict`): a
    ///     static dictionary of fully-resolved methods, or — once one of
    ///     its instance's methods is dictionary-passing — the composed
    ///     instance dictionary;
    ///   - a still-polymorphic structured type with a context instance
    ///     (`[a]`, `T a`) -> the instance's dictionary-form methods closed
    ///     over the dictionaries the context demands (built recursively);
    ///   - a structural shape of Eq/Ord/Show over such elements -> the
    ///     shape's synthesized dictionary form over the element
    ///     dictionaries (`structural_dict`)
    ///   (the last two decided now, by `decide_dict`, from the evidence in
    ///   scope).
    fn build_dict_expr(&mut self, class_name: &str, ty: &Ty, ev: &DictEnv) -> TExpr {
        if let Ty::Var(v) = ty {
            if let Some(p) = ev.env
                .get(&(class_name.to_string(), v.name.clone()))
                .or_else(|| ev.class_to_dict.get(class_name))
            {
                return TExpr::new(TExprKind::Var(p.clone()), Ty::Unit);
            }
            return TExpr::new(TExprKind::Lit(TLiteral::Unit), Ty::Unit);
        }
        // An existential skolem with a captured dictionary in scope: the
        // binder IS the dictionary. (Without one — a rank-2 skolem, or a
        // class the constructor's context does not guarantee — fall
        // through to the static path, as before.)
        if let Ty::Skolem(_, id) = ty
            && let Some(binder) = self.skolem_dict_for(class_name, *id)
        {
            return TExpr::new(TExprKind::Var(binder), Ty::Unit);
        }
        // A ground type needs no evidence from this scope, so its
        // dictionary is decided once the fixpoint is final — not here,
        // where a later trip could still purge what it names.
        if ty.free_vars().is_empty() && !Self::contains_skolem(ty) {
            return self.demand_dict(class_name, ty);
        }
        self.decide_dict(class_name, ty, ev)
    }

    /// Leave a `DictDemand` placeholder for the `class_name` dictionary of
    /// ground `ty`, recording the demand — once per (class, type), with
    /// this site's diagnostic context — for `materialize_dicts`.
    fn demand_dict(&mut self, class_name: &str, ty: &Ty) -> TExpr {
        let key = (class_name.to_string(), ty.clone());
        if self.dict_demanded.insert(key.clone()) {
            self.dict_demands.push((key.0, key.1, self.cur_ctx.clone(), self.cur_span));
        }
        TExpr::new(
            TExprKind::SpecCall {
                original: format!("__dict_{}", class_name),
                specialized: SpecKind::DictDemand { class: class_name.to_string(), ty: ty.clone() },
                args: vec![],
            },
            Ty::Unit,
        )
    }

    /// THE decision of what the `class_name` dictionary for a structured
    /// or ground `ty` is — one place, for a type with evidence in scope
    /// (from `build_dict_expr`, now) and for a ground type (from
    /// `materialize_dicts`, after the fixpoint) alike:
    ///
    ///   - structured over the constrained variable(s) or over a hidden
    ///     type with captured dictionaries, OR a concrete structured type
    ///     whose instance method is itself dictionary-passing (purged for
    ///     tripping the specialization cap, as the generic representation
    ///     combinators do across many deriving types) -> the instance's
    ///     dictionary-form methods closed over the dictionaries its context
    ///     demands (`compose_instance_dict`): in both cases the method
    ///     cannot be resolved to one specialized function;
    ///   - a structural shape of Eq/Ord/Show/Hashable over such elements ->
    ///     the shape's synthesized dictionary form over the element
    ///     dictionaries (`structural_dict`);
    ///   - otherwise a static dictionary of fully-resolved methods
    ///     (`build_concrete_dict`; for a head with no parameterized
    ///     instance with a context, the head-keyed builtin whose runtime
    ///     methods are shape-generic, e.g. `show`).
    fn decide_dict(&mut self, class_name: &str, ty: &Ty, ev: &DictEnv) -> TExpr {
        let over_hidden = Self::contains_skolem(ty) && self.has_skolem_dicts(ty);
        if (!ty.free_vars().is_empty() || over_hidden || self.head_method_is_dict_passing(class_name, ty))
            && let Some(dict) = self.compose_instance_dict(class_name, ty, ev)
        {
            return dict;
        }
        if (!ty.free_vars().is_empty() || over_hidden)
            && let Some(dict) = self.structural_dict(class_name, ty, ev)
        {
            return dict;
        }
        self.build_concrete_dict(class_name, ty)
    }

    /// The `class_name` dictionary for `ty` composed from the parameterized
    /// instance of its head: the instance's dictionary-form methods
    /// (`ensure_dictform`) closed over the dictionaries the instance
    /// context demands of the bound arguments, each built recursively
    /// (`build_dict_expr`: a parameter, a captured binder, or a demand).
    /// `None` when the head has no instance with a context.
    fn compose_instance_dict(&mut self, class_name: &str, ty: &Ty, ev: &DictEnv) -> Option<TExpr> {
        let head = InstHead::of(ty)?;
        let inst = self.instances.get(&(class_name.to_string(), head.clone())).cloned()?;
        let ctx = inst.context.clone()?;
        let methods = self.ensure_dictform(class_name, &head)?;
        let mut bind: HashMap<String, Ty> = HashMap::new();
        Self::collect_subst_by_name(&inst.target_type, ty, &mut bind);
        let mut sub_dicts = Vec::new();
        for c in &ctx {
            let sub_ty = bind.get(&c.type_var).cloned()
                .unwrap_or_else(|| Ty::Var(TyVar { name: c.type_var.clone(), id: 0 }));
            sub_dicts.push(self.build_dict_expr(&c.class_name, &sub_ty, ev));
        }
        Some(TExpr::new(
            TExprKind::SpecCall {
                original: format!("__dict_{}", class_name),
                specialized: SpecKind::DictCtor { class: class_name.to_string(), methods },
                args: sub_dicts,
            },
            Ty::Unit,
        ))
    }

    /// The classes whose builtin container instances (list, Maybe, tuple)
    /// are compiler-owned STRUCTURAL implementations rather than registered
    /// context instances: the specialized path generates their methods per
    /// element type (`generate_list_eq` & co., `structural_key_enc`), and
    /// the dictionary path composes them from element dictionaries
    /// (`structural_dict`). Hashable is one of them: its evidence is the
    /// key type's encoder and compare, composed over the elements' exactly
    /// as Eq and Ord are.
    fn is_structural_class(class: &str) -> bool {
        matches!(class, "Eq" | "Ord" | "Show" | "Hashable")
    }

    /// The canonical memo shape of a structural type (element types erased
    /// — a shape's dictionary form is generic over them) and the element
    /// types a use needs dictionaries for: `[a]` -> (`[()]`, [a]), `Maybe
    /// a` -> (`Maybe ()`, [a]), `(a, b)` -> (`((), ())`, [a, b]). `None`
    /// for every other type.
    fn structural_shape(ty: &Ty) -> Option<(Ty, Vec<Ty>)> {
        match ty {
            Ty::List(e) => Some((Ty::List(Box::new(Ty::Unit)), vec![(**e).clone()])),
            Ty::App(f, e) if matches!(f.as_ref(), Ty::Con(n) if n == "Maybe") =>
                Some((Ty::app(Ty::Con("Maybe".into()), Ty::Unit), vec![(**e).clone()])),
            Ty::Tuple(es) if !es.is_empty() =>
                Some((Ty::Tuple(vec![Ty::Unit; es.len()]), es.clone())),
            _ => None,
        }
    }

    /// A composed dictionary for a structural type over still-polymorphic
    /// or existential elements — `[a]`, `Maybe a`, tuples — for Eq, Ord or
    /// Show. The builtin container instances have no `instances` entry
    /// for `build_dict_expr` to compose from (the specialized path
    /// generates their methods on demand), so the fallback chain once
    /// bottomed out at build_concrete_dict's raw method name: an
    /// `_usr_eq__eq_` reference that exists nowhere — a nil-call crash at
    /// the first recursive use (F6a) — and `compare [x] [y]` under `Ord a`
    /// was rejected outright ("cannot build the 'Ord' dictionary", X2).
    /// Builds a DictCtor over synthesized dictionary-form functions (one
    /// per class, shape and method, memoized) that thread the element
    /// dictionaries' method through the same runtime helpers the
    /// specialized path uses (`__mll_list_eq`/`__mll_list_cmp`/
    /// `__mll_show_list`, their Maybe twins, inline tuple projection); the
    /// class's remaining methods are GHC's defaults over the base one.
    fn structural_dict(&mut self, class: &str, ty: &Ty, ev: &DictEnv) -> Option<TExpr> {
        if !Self::is_structural_class(class) {
            return None;
        }
        let (shape, elems) = Self::structural_shape(ty)?;
        let n_dicts = elems.len();
        let methods: Vec<(String, String, usize)> = match class {
            "Eq" => {
                let eq = self.structural_eq_dictform(&shape, n_dicts);
                let bool_ty = Ty::Con("Bool".into());
                // `/=`: GHC's class default `not (a == b)` over the dictform `==`.
                let ne = self.derived_default_dictform(
                    ("/=__dictform", shape.clone()), "ne_dictform", n_dicts,
                    &["_a", "_b"], &["_a", "_b"], bool_ty.clone(),
                    |base_call| TExpr::new(
                        TExprKind::App(
                            Box::new(TExpr::new(TExprKind::Var("not".to_string()), Ty::Unit)),
                            Box::new(base_call),
                        ),
                        bool_ty.clone(),
                    ),
                    &eq,
                );
                vec![("==".to_string(), eq, 2), ("/=".to_string(), ne, 2)]
            }
            "Ord" => {
                let cmp = self.structural_cmp_dictform(&shape, n_dicts, "compare", "ord_compare_dictform");
                let bool_ty = Ty::Con("Bool".into());
                let mut methods = vec![("compare".to_string(), cmp.clone(), 2)];
                // The operators and selectors: GHC's class defaults over
                // `compare` — `x < y = case compare x y of LT -> True; _ ->
                // False`, …, `max x y = if x <= y then y else x` (`y` on EQ),
                // `min x y = if x <= y then x else y` (`x` on EQ) — as a
                // case on the Ordering constructor.
                let bool_lit = |b: bool| TExpr::new(TExprKind::Lit(TLiteral::Bool(b)), Ty::Con("Bool".into()));
                let var = |n: &str| TExpr::new(TExprKind::Var(n.to_string()), Ty::Unit);
                let on_ordering = |call: TExpr, con: &str, then: TExpr, otherwise: TExpr, result_ty: Ty| TExpr::new(
                    TExprKind::Case {
                        scrutinee: Box::new(call),
                        branches: vec![
                            TCaseBranch {
                                pattern: TPattern::ctor(con.to_string(), vec![]),
                                guards: vec![],
                                body: Some(then),
                            },
                            TCaseBranch {
                                pattern: TPattern::Wildcard,
                                guards: vec![],
                                body: Some(otherwise),
                            },
                        ],
                    },
                    result_ty,
                );
                for (method, base, con, hit, miss) in [
                    ("<", "ord_lt_dictform", "LT", true, false),
                    ("<=", "ord_le_dictform", "GT", false, true),
                    (">", "ord_gt_dictform", "GT", true, false),
                    (">=", "ord_ge_dictform", "LT", false, true),
                ] {
                    let name = self.derived_default_dictform(
                        (&format!("{method}__dictform"), shape.clone()), base, n_dicts,
                        &["_a", "_b"], &["_a", "_b"], bool_ty.clone(),
                        |call| on_ordering(call, con, bool_lit(hit), bool_lit(miss), bool_ty.clone()),
                        &cmp,
                    );
                    methods.push((method.to_string(), name, 2));
                }
                for (method, base, on_gt, otherwise) in [
                    ("max", "ord_max_dictform", "_a", "_b"),
                    ("min", "ord_min_dictform", "_b", "_a"),
                ] {
                    let name = self.derived_default_dictform(
                        (&format!("{method}__dictform"), shape.clone()), base, n_dicts,
                        &["_a", "_b"], &["_a", "_b"], shape.clone(),
                        |call| on_ordering(call, "GT", var(on_gt), var(otherwise), shape.clone()),
                        &cmp,
                    );
                    methods.push((method.to_string(), name, 2));
                }
                methods
            }
            "Hashable" => {
                // The key encoder over the element dictionaries' encoders
                // and the key compare over their compares — the two
                // entries every hm* dictionary form reads.
                let enc = self.structural_hashkey_dictform(&shape, n_dicts);
                let cmp = self.structural_cmp_dictform(&shape, n_dicts, "__hashCompare", "hashcompare_dictform");
                vec![("__hashKey".to_string(), enc, 1), ("__hashCompare".to_string(), cmp, 2)]
            }
            _ => {
                let show = self.structural_show_dictform(&shape, n_dicts);
                let str_ty = Ty::Con("String".into());
                // `showsPrec`: the runtime precedence rule over the dictform
                // `show` (what every builtin/derived showsPrec is).
                let sp_str_ty = str_ty.clone();
                let sp = self.derived_default_dictform(
                    ("showsPrec__dictform", shape.clone()), "showsPrec_dictform", n_dicts,
                    &["_d", "_x", "_s"], &["_x"], str_ty,
                    |shown| {
                        let var = |n: &str| TExpr::new(TExprKind::Var(n.to_string()), Ty::Unit);
                        let app = |f: TExpr, a: TExpr| TExpr::new(
                            TExprKind::App(Box::new(f), Box::new(a)), Ty::Unit);
                        let call = app(app(app(var("__mll_shows_prec"), var("_d")), shown), var("_s"));
                        TExpr { kind: call.kind, ty: sp_str_ty }
                    },
                    &show,
                );
                vec![("show".to_string(), show, 1), ("showsPrec".to_string(), sp, 3)]
            }
        };
        let sub_dicts: Vec<TExpr> = elems.iter()
            .map(|e| self.build_dict_expr(class, e, ev))
            .collect();
        Some(TExpr::new(
            TExprKind::SpecCall {
                original: format!("__dict_{}", class),
                specialized: SpecKind::DictCtor { class: class.to_string(), methods },
                args: sub_dicts,
            },
            Ty::Unit,
        ))
    }

    /// Register a synthesized dictionary-form function for a structural
    /// shape: `name(__d0, …, params…) = body`, where the `n_dicts` leading
    /// parameters are the element dictionaries (read inside the body as
    /// `DictAccess`) and `params` the value parameters. Memoized per
    /// (key, shape) in `generated_impls` — the memo is checked first, so
    /// `body` is built once per shape.
    fn structural_dictform(
        &mut self,
        key: (&str, &Ty),
        base_name: &str,
        n_dicts: usize,
        params: &[&str],
        result_ty: Ty,
        body: impl FnOnce(&[String]) -> TExpr,
    ) -> String {
        let (key_method, shape) = key;
        let memo_key = (key_method.to_string(), shape.clone());
        if let Some(existing) = self.generated_impls.get(&memo_key) {
            return existing.clone();
        }
        let fname = self.mangle_name(base_name, shape);
        self.generated_impls.insert(memo_key, fname.clone());
        let dict_params: Vec<String> = (0..n_dicts).map(|i| format!("__d{i}")).collect();
        let body = body(&dict_params);
        let mut patterns: Vec<TPattern> = dict_params.iter()
            .map(|d| TPattern::Var(d.clone(), Ty::Unit)).collect();
        patterns.extend(params.iter().map(|p| TPattern::Var((*p).to_string(), Ty::Unit)));
        let param_tys: Vec<Ty> = vec![Ty::Unit; n_dicts + params.len()];
        self.generated.push(TFunction {
            name: fname.clone(),
            ty: Ty::fun(&param_tys, result_ty),
            clauses: vec![TClause {
                span: None,
                patterns,
                guards: vec![],
                body: Some(body),
                where_binds: vec![],
            }],
            specialized: true,
            spec_origin: None,
            dict_params: vec![],
            derived_strict: false,
        });
        fname
    }

    /// The element method `method` of the `i`th element dictionary
    /// parameter of a structural dictform, as an expression.
    fn dictform_elem_method(dict_param: &str, method: &str) -> TExpr {
        TExpr::new(
            TExprKind::DictAccess {
                dict_param: dict_param.to_string(),
                method_name: method.to_string(),
            },
            Ty::Unit,
        )
    }

    /// Field `i` (1-based) of the tuple bound to `var`, for a structural
    /// dictform body.
    fn dictform_tuple_field(var: &str, i: usize) -> TExpr {
        TExpr::new(
            TExprKind::SpecCall {
                original: "__mll_tup_get".to_string(),
                specialized: SpecKind::TupGet(i),
                args: vec![TExpr::new(TExprKind::Var(var.to_string()), Ty::Unit)],
            },
            Ty::Unit,
        )
    }

    /// The dictionary-form `==` of a structural shape: `__mll_list_eq`/
    /// `__mll_maybe_eq` threading the element dictionary's `==`, or the
    /// conjunction of the element dictionaries' `==` over the tuple's
    /// fields.
    fn structural_eq_dictform(&mut self, shape: &Ty, n_dicts: usize) -> String {
        let bool_ty = Ty::Con("Bool".into());
        let shape_owned = shape.clone();
        self.structural_dictform(("==__dictform", shape), "eq_dictform", n_dicts, &["_a", "_b"], bool_ty.clone(), move |dicts| {
            let var = |n: &str| TExpr::new(TExprKind::Var(n.to_string()), Ty::Unit);
            let app = |f: TExpr, a: TExpr, t: Ty| TExpr::new(
                TExprKind::App(Box::new(f), Box::new(a)), t);
            match &shape_owned {
                Ty::Tuple(es) => {
                    // (d0.== (tup_get 1 a) (tup_get 1 b)) && … per field.
                    let mut acc: Option<TExpr> = None;
                    for i in 0..es.len() {
                        let cmp = app(
                            app(Self::dictform_elem_method(&dicts[i], "=="),
                                Self::dictform_tuple_field("_a", i + 1), Ty::Unit),
                            Self::dictform_tuple_field("_b", i + 1),
                            bool_ty.clone(),
                        );
                        acc = Some(match acc {
                            None => cmp,
                            Some(prev) => TExpr::new(
                                TExprKind::InfixApp {
                                    op: "&&".to_string(),
                                    lhs: Box::new(prev),
                                    rhs: Box::new(cmp),
                                },
                                bool_ty.clone(),
                            ),
                        });
                    }
                    acc.expect("tuple shapes are non-empty")
                }
                Ty::List(_) => app(
                    app(app(var("__mll_list_eq"), Self::dictform_elem_method(&dicts[0], "=="), Ty::Unit),
                        var("_a"), Ty::Unit),
                    var("_b"), bool_ty.clone(),
                ),
                _ => app(
                    app(app(var("__mll_maybe_eq"), Self::dictform_elem_method(&dicts[0], "=="), Ty::Unit),
                        var("_a"), Ty::Unit),
                    var("_b"), bool_ty.clone(),
                ),
            }
        })
    }

    /// The dictionary-form `compare` of a structural shape, the Ord twin
    /// of `structural_eq_dictform`: `__mll_list_cmp`/`__mll_maybe_cmp`
    /// threading the element dictionary's comparator (lexicographic lists,
    /// Nothing < Just), or the fields' comparators chained on EQ for a
    /// tuple — exactly the specialized path's `ListCmp`/`MaybeCmp`/
    /// `TupleCmp` with the element comparator read from a dictionary.
    /// `elem_method` names the element dictionary's entry: Ord's `compare`,
    /// or Hashable's `__hashCompare` (the same order, read off the
    /// Hashable dictionary a HashMap write has in hand).
    fn structural_cmp_dictform(
        &mut self,
        shape: &Ty,
        n_dicts: usize,
        elem_method: &str,
        base_name: &str,
    ) -> String {
        let ordering = Ty::Con("Ordering".into());
        let shape_owned = shape.clone();
        let elem_method = elem_method.to_string();
        let memo_key = format!("{elem_method}__dictform");
        self.structural_dictform((&memo_key, shape), base_name, n_dicts, &["_a", "_b"], ordering.clone(), move |dicts| {
            let var = |n: &str| TExpr::new(TExprKind::Var(n.to_string()), Ty::Unit);
            let app = |f: TExpr, a: TExpr, t: Ty| TExpr::new(
                TExprKind::App(Box::new(f), Box::new(a)), t);
            match &shape_owned {
                Ty::Tuple(es) => {
                    // case d0.compare (tup_get 1 a) (tup_get 1 b) of
                    //   EQ -> <the rest>; r -> r
                    // with the last field's compare as the innermost result.
                    let field_cmp = |i: usize| app(
                        app(Self::dictform_elem_method(&dicts[i], &elem_method),
                            Self::dictform_tuple_field("_a", i + 1), Ty::Unit),
                        Self::dictform_tuple_field("_b", i + 1),
                        ordering.clone(),
                    );
                    let mut acc = field_cmp(es.len() - 1);
                    for i in (0..es.len() - 1).rev() {
                        acc = TExpr::new(
                            TExprKind::Case {
                                scrutinee: Box::new(field_cmp(i)),
                                branches: vec![
                                    TCaseBranch {
                                        pattern: TPattern::ctor("EQ".to_string(), vec![]),
                                        guards: vec![],
                                        body: Some(acc),
                                    },
                                    TCaseBranch {
                                        pattern: TPattern::Var("_r".to_string(), ordering.clone()),
                                        guards: vec![],
                                        body: Some(TExpr::new(TExprKind::Var("_r".to_string()), ordering.clone())),
                                    },
                                ],
                            },
                            ordering.clone(),
                        );
                    }
                    acc
                }
                Ty::List(_) => app(
                    app(app(var("__mll_list_cmp"), Self::dictform_elem_method(&dicts[0], &elem_method), Ty::Unit),
                        var("_a"), Ty::Unit),
                    var("_b"), ordering.clone(),
                ),
                _ => app(
                    app(app(var("__mll_maybe_cmp"), Self::dictform_elem_method(&dicts[0], &elem_method), Ty::Unit),
                        var("_a"), Ty::Unit),
                    var("_b"), ordering.clone(),
                ),
            }
        })
    }

    /// The dictionary-form key encoder of a structural shape (Hashable's
    /// `__hashKey`): `__mll_key_list`/`__mll_key_maybe` threading the
    /// element dictionary's encoder, or the tuple's `"(" e1 "," e2 … ")"`
    /// over the element dictionaries' encoders — byte-for-byte the
    /// specialized path's `KeyEncList`/`KeyEncMaybe`/`KeyEncTuple`, so a
    /// dictionary-form body and a specialized one encode one key one way.
    fn structural_hashkey_dictform(&mut self, shape: &Ty, n_dicts: usize) -> String {
        let str_ty = Ty::Con("String".into());
        let shape_owned = shape.clone();
        self.structural_dictform(("__hashKey__dictform", shape), "hashkey_dictform", n_dicts, &["_k"], str_ty.clone(), move |dicts| {
            let var = |n: &str| TExpr::new(TExprKind::Var(n.to_string()), Ty::Unit);
            let app = |f: TExpr, a: TExpr, t: Ty| TExpr::new(
                TExprKind::App(Box::new(f), Box::new(a)), t);
            match &shape_owned {
                Ty::Tuple(es) => {
                    let lit = |s: &str| TExpr::new(TExprKind::Lit(TLiteral::Str(s.as_bytes().to_vec())), str_ty.clone());
                    let cat = |l: TExpr, r: TExpr| TExpr::new(TExprKind::InfixApp {
                        op: "<>".to_string(), lhs: Box::new(l), rhs: Box::new(r),
                    }, str_ty.clone());
                    let mut acc = lit("(");
                    for i in 0..es.len() {
                        if i > 0 {
                            acc = cat(acc, lit(","));
                        }
                        let encoded = app(Self::dictform_elem_method(&dicts[i], "__hashKey"),
                            Self::dictform_tuple_field("_k", i + 1), str_ty.clone());
                        acc = cat(acc, encoded);
                    }
                    cat(acc, lit(")"))
                }
                Ty::List(_) => app(
                    app(var("__mll_key_list"), Self::dictform_elem_method(&dicts[0], "__hashKey"), Ty::Unit),
                    var("_k"), str_ty.clone(),
                ),
                _ => app(
                    app(var("__mll_key_maybe"), Self::dictform_elem_method(&dicts[0], "__hashKey"), Ty::Unit),
                    var("_k"), str_ty.clone(),
                ),
            }
        })
    }

    /// The dictionary-form `show` of a structural shape: `__mll_show_list`
    /// threading the element dictionary's `show`, `__mll_show_maybe`
    /// threading its `showsPrec` (`Just x` shows its payload at
    /// precedence 11, see generate_container_show), or the tuple's
    /// concatenation over the element dictionaries' `show`. Without it a
    /// `show [x]` at a hidden type bottomed out at the type-erased `show`
    /// (a user ADT element printed as a tuple).
    fn structural_show_dictform(&mut self, shape: &Ty, n_dicts: usize) -> String {
        let str_ty = Ty::Con("String".into());
        let shape_owned = shape.clone();
        self.structural_dictform(("show_structural", shape), "show_dictform", n_dicts, &["_x"], str_ty.clone(), move |dicts| {
            let var = |n: &str| TExpr::new(TExprKind::Var(n.to_string()), Ty::Unit);
            let app = |f: TExpr, a: TExpr, t: Ty| TExpr::new(
                TExprKind::App(Box::new(f), Box::new(a)), t);
            match &shape_owned {
                Ty::Tuple(es) => {
                    // "(" <> d0.show (tup_get 1 x) <> "," <> … <> ")"
                    let lit = |s: &str| TExpr::new(TExprKind::Lit(TLiteral::Str(s.as_bytes().to_vec())), str_ty.clone());
                    let cat = |l: TExpr, r: TExpr| TExpr::new(TExprKind::InfixApp {
                        op: "<>".to_string(), lhs: Box::new(l), rhs: Box::new(r),
                    }, str_ty.clone());
                    let mut acc = lit("(");
                    for i in 0..es.len() {
                        if i > 0 {
                            acc = cat(acc, lit(","));
                        }
                        let shown = app(Self::dictform_elem_method(&dicts[i], "show"),
                            Self::dictform_tuple_field("_x", i + 1), str_ty.clone());
                        acc = cat(acc, shown);
                    }
                    cat(acc, lit(")"))
                }
                Ty::List(_) => app(
                    app(var("__mll_show_list"), Self::dictform_elem_method(&dicts[0], "show"), Ty::Unit),
                    var("_x"), str_ty.clone(),
                ),
                _ => app(
                    app(var("__mll_show_maybe"), Self::dictform_elem_method(&dicts[0], "showsPrec"), Ty::Unit),
                    var("_x"), str_ty.clone(),
                ),
            }
        })
    }

    /// A dictionary-form method that is a class DEFAULT over another
    /// dictform method of the same shape (`/=` over `==`, the Ord operators
    /// and `max`/`min` over `compare`, `showsPrec` over `show`):
    /// `name(dicts…, params…) = wrap(base(dicts…, base_args…))`,
    /// where the base call forwards the same context dictionaries and the
    /// value parameters named in `base_args`. Memoized per (key, shape)
    /// like the base dictforms.
    #[allow(clippy::too_many_arguments)]
    fn derived_default_dictform(
        &mut self,
        key: (&str, Ty),
        base_name: &str,
        n_dicts: usize,
        params: &[&str],
        base_args: &[&str],
        result_ty: Ty,
        wrap: impl FnOnce(TExpr) -> TExpr,
        base_fn: &str,
    ) -> String {
        let (key_method, shape) = key;
        let memo_key = (key_method.to_string(), shape.clone());
        if let Some(existing) = self.generated_impls.get(&memo_key) {
            return existing.clone();
        }
        let fname = self.mangle_name(base_name, &shape);
        self.generated_impls.insert(memo_key, fname.clone());
        let dict_params: Vec<String> = (0..n_dicts).map(|i| format!("__d{i}")).collect();
        let var = |n: &str| TExpr::new(TExprKind::Var(n.to_string()), Ty::Unit);
        // base(dicts…, base_args…): the base dictform's parameter list is
        // its dictionaries followed by its value parameters.
        let mut call = var(base_fn);
        for d in &dict_params {
            call = TExpr::new(TExprKind::App(Box::new(call), Box::new(var(d))), Ty::Unit);
        }
        for p in base_args {
            call = TExpr::new(TExprKind::App(Box::new(call), Box::new(var(p))), Ty::Unit);
        }
        let body = wrap(call);
        let mut patterns: Vec<TPattern> = dict_params.iter()
            .map(|d| TPattern::Var(d.clone(), Ty::Unit)).collect();
        patterns.extend(params.iter().map(|p| TPattern::Var((*p).to_string(), Ty::Unit)));
        let param_tys: Vec<Ty> = vec![Ty::Unit; n_dicts + params.len()];
        self.generated.push(TFunction {
            name: fname.clone(),
            ty: Ty::fun(&param_tys, result_ty),
            clauses: vec![TClause {
                span: None,
                patterns,
                guards: vec![],
                body: Some(body),
                where_binds: vec![],
            }],
            specialized: true,
            spec_origin: None,
            dict_params: vec![],
            derived_strict: false,
        });
        fname
    }

    /// Make sure dictionary-form methods exist for the parameterized
    /// instance (class, head): each instance method rewritten to take its
    /// context's dictionaries (the superclass closure of the declared
    /// context — `instances` holds it closed) as leading parameters,
    /// followed by the dictionaries of the method's OWN constraints
    /// (`method_dict_constraints`: `traverse`'s Applicative and Functor),
    /// with every still-polymorphic method use inside routed through them.
    /// Returns the per-method function names in class-declaration order
    /// with each method's VALUE arity, or None when the instance (or a
    /// polymorphic original) cannot be found.
    fn ensure_dictform(
        &mut self,
        class_name: &str,
        head: &InstHead,
    ) -> Option<Vec<(String, String, usize)>> {
        let key = (class_name.to_string(), head.clone());
        if let Some(ms) = self.dictform_impls.get(&key) {
            return Some(ms.clone());
        }
        let inst = self.instances.get(&key)?.clone();
        let ctx = inst.context.clone()?;
        let class_info = self.classes.get(class_name)?.clone();
        // Methods in class-declaration order (method_fns is a HashMap and
        // must not drive emission order).
        let mut names: Vec<(String, String, usize)> = Vec::new();
        let mut work: Vec<(String, String, String)> = Vec::new();
        for (method, method_sig) in &class_info.methods {
            let base = inst.method_fns.get(method)?.clone();
            if !self.poly_fns.contains_key(&base) {
                return None;
            }
            let dictform = crate::ast::minted(&format!("{}_", base), "dictform");
            // The method's VALUE arity decides the DictCtor field shape:
            // a closure over the context dictionaries for arity > 0, the
            // constructed VALUE for a nullary method (see SpecKind::DictCtor).
            names.push((method.clone(), dictform.clone(), Self::spine_arrow_count(method_sig)));
            work.push((method.clone(), base, dictform));
        }
        // Seed the cache BEFORE rewriting bodies: a method whose body uses
        // the class at the instance's own type (`csize xs` inside the `[a]`
        // instance) builds this same dictionary recursively.
        self.dictform_impls.insert(key.clone(), names.clone());

        // The dictform's own dictionary parameters are current while its
        // bodies are rewritten — BOTH routing tables: the plain per-class
        // parameters and the compound-constraint triples. An instance
        // context is plain (`Class var`), so the dictform has no compound
        // triples; leaving the ENCLOSING dict-passing function's triples in
        // place (as this once did) would route a compound method use inside
        // the instance body to a parameter that does not exist here.
        let saved_dict_params = std::mem::take(&mut self.cur_dict_params);
        let saved_dict_by_arg = std::mem::take(&mut self.cur_dict_by_arg);
        for (method, base, dictform) in work {
            let Some(mut f) = self.poly_fns.get(&base).cloned() else {
                // Near-dead (the pre-scan above verified every base), but
                // a bail here must UNSEED the cache: leaving the seed would
                // make a later query hand out method names whose dictforms
                // were never generated — a nil lua_ref at runtime.
                self.dictform_impls.remove(&key);
                self.cur_dict_params = saved_dict_params;
                self.cur_dict_by_arg = saved_dict_by_arg;
                return None;
            };
            // The instance context's dictionaries, then the method's own.
            let mut constraints = ctx.clone();
            if let Some(extra) = self.method_dict_constraints.get(&method) {
                constraints.extend(extra.iter().cloned());
            }
            // Each constraint named after the variable as the BODY spells
            // it: the context names the instance head's variables (`a`,
            // `b`), the method's type carries the checker's own (`a2114`,
            // or `_inst196` in a synthesized default method), and the
            // evidence table is read by name. With the declared names a
            // use at `b` missed the table and fell to the first dictionary
            // of the class: `instance (Show a, Show b) => Show (Pair a b)`
            // showed `y` with `x`'s dictionary ("attempt to mod a
            // 'string'" on `Pair 1 "a"`). The method's variables are
            // matched the same way, through the class's declared method
            // type (families blanked — see `class_var_binding`).
            let mut spelled: HashMap<String, String> = HashMap::new();
            if let Some((_, decl_ty)) = class_info.methods.iter().find(|(n, _)| *n == method) {
                let decl_ty = self.blank_family_apps(decl_ty);
                let mut by_decl: HashMap<String, Ty> = HashMap::new();
                Self::collect_subst_by_name(&decl_ty, &f.ty, &mut by_decl);
                if let Some(inst_ty) = by_decl.get(&class_info.type_var) {
                    let mut by_head: HashMap<String, Ty> = HashMap::new();
                    Self::collect_subst_by_name(&inst.target_type, inst_ty, &mut by_head);
                    spelled.extend(by_head.into_iter()
                        .filter_map(|(v, t)| match t { Ty::Var(b) => Some((v, b.name)), _ => None }));
                }
                spelled.extend(by_decl.into_iter()
                    .filter(|(v, _)| *v != class_info.type_var)
                    .filter_map(|(v, t)| match t { Ty::Var(b) => Some((v, b.name)), _ => None }));
            }
            for c in constraints.iter_mut() {
                if let Some(body_name) = spelled.get(&c.type_var) {
                    c.type_var = body_name.clone();
                }
            }
            let (dict_params, ev) = Self::dict_param_tables(&constraints);
            self.cur_dict_params = dict_params.clone();
            f.name = dictform.clone();
            // Ordinary monomorphization first: ground method uses and helper
            // calls resolve as usual; still-polymorphic method uses stay
            // deferred (bare Vars) for the dictionary rewrite below —
            // in_dictform keeps the arbitrary-specialization fallback from
            // welding one type's spec into this shared body.
            let saved_in_dictform = std::mem::replace(&mut self.in_dictform, true);
            f = self.mono_function(f);
            self.in_dictform = saved_in_dictform;
            f.dict_params = dict_params;
            f.specialized = true;
            // The SCOPED walk: a clause binder named like a method of the
            // class (`describe (Box label other) = …` under a class with a
            // `label` method) is that binder, not a dictionary access —
            // the same discipline as `rewrite_dict_passing_fn` (T12).
            for clause in f.clauses.iter_mut() {
                self.map_clause_exprs_scoped(clause, &mut |m, e| {
                    m.rewrite_dict_expr(e, "", &ev)
                });
            }
            self.generated.push(f);
        }
        self.cur_dict_params = saved_dict_params;
        self.cur_dict_by_arg = saved_dict_by_arg;
        Some(names)
    }
}

// ---- Constrained existentials: dictionary capture (see `ExCtor`) ----
impl Monomorphizer {
    /// Push captured-dictionary bindings for the duration of `f`.
    fn with_ex_scope<T>(
        &mut self,
        caps: Vec<((String, u32), String)>,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let mark = self.skolem_dicts.len();
        self.skolem_dicts.extend(caps);
        let out = f(self);
        self.skolem_dicts.truncate(mark);
        out
    }

    /// The binder holding the captured `class` dictionary for skolem `id`,
    /// innermost binding first.
    fn skolem_dict_for(&self, class: &str, id: u32) -> Option<String> {
        self.skolem_dicts.iter().rev()
            .find(|((c, i), _)| c == class && *i == id)
            .map(|(_, b)| b.clone())
    }

    /// Does every skolem in `ty` have SOME captured dictionary in scope?
    /// (The class-specific check happens per skolem in `build_dict_expr`;
    /// this gates whether composing a dictionary over the type can succeed
    /// at all, so a rank-2 skolem keeps the static path.)
    fn has_skolem_dicts(&self, ty: &Ty) -> bool {
        let mut ids = Vec::new();
        Self::collect_skolem_ids(ty, &mut ids);
        !ids.is_empty()
            && ids.iter().all(|id| self.skolem_dicts.iter().any(|((_, i), _)| i == id))
    }

    fn collect_skolem_ids(ty: &Ty, out: &mut Vec<u32>) {
        match ty {
            Ty::Skolem(_, id) => out.push(*id),
            Ty::Con(_) | Ty::Var(_) | Ty::Unit | Ty::Promoted(_) => {}
            Ty::Arrow(a, b, _) | Ty::App(a, b) => {
                Self::collect_skolem_ids(a, out);
                Self::collect_skolem_ids(b, out);
            }
            Ty::List(a) | Ty::IO(a) | Ty::LuaIO(_, a) | Ty::Forall(_, a) => Self::collect_skolem_ids(a, out),
            Ty::Tuple(es) => es.iter().for_each(|e| Self::collect_skolem_ids(e, out)),
        }
    }

    fn ex_rewrite_patterns(
        &mut self,
        pats: Vec<TPattern>,
        caps: &mut Vec<((String, u32), String)>,
    ) -> Vec<TPattern> {
        pats.into_iter().map(|p| self.ex_rewrite_pattern(p, caps)).collect()
    }

    /// Give every constrained-existential constructor pattern its hidden
    /// dictionary binders (trailing, one per capture) and record which
    /// (class, skolem) each binder answers for. The skolem is the one the
    /// checker minted for THIS pattern's own existential variable, read off
    /// the pattern node (`TPattern::Constructor::skolems`): a capture
    /// `(Eq, a)` binds to this constructor's `a`, never to a same-named
    /// `a` of an existential pattern nested in one of its fields. The
    /// mapping does not depend on what the fields bind, so a field that
    /// binds nothing at the hidden type (a wildcard) maps like any other.
    fn ex_rewrite_pattern(
        &mut self,
        pat: TPattern,
        caps: &mut Vec<((String, u32), String)>,
    ) -> TPattern {
        match pat {
            TPattern::Constructor { name, args, skolems } => {
                let args: Vec<TPattern> = args.into_iter().map(|a| self.ex_rewrite_pattern(a, caps)).collect();
                let Some(ex) = self.ex_ctors.get(&name).cloned() else {
                    return TPattern::Constructor { name, args, skolems };
                };
                let n = ex.field_types.len();
                let own_skolem = |var: &str| skolems.iter().find(|(v, _)| v == var).map(|(_, id)| *id);
                // A pattern the main walk already extended (the dictionary
                // rewrite passes revisit finished TIR) keeps its binders
                // and hands back the same (class, skolem) -> binder
                // mapping, so the captured dictionaries are in scope for
                // the rewrite exactly as they were for the main walk.
                if args.len() == n + ex.captures.len() {
                    for (i, (cls, var)) in ex.captures.iter().enumerate() {
                        if let TPattern::Var(binder, _) = &args[n + i]
                            && let Some(id) = own_skolem(var)
                        {
                            caps.push(((cls.clone(), id), binder.clone()));
                        }
                    }
                    return TPattern::Constructor { name, args, skolems };
                }
                // Only a pattern at the DECLARED arity is rewritten.
                if args.len() != n {
                    return TPattern::Constructor { name, args, skolems };
                }
                let mut args = args;
                for (i, (cls, var)) in ex.captures.iter().enumerate() {
                    let binder = format!("__exd{}_{}", self.ex_binder_counter, i);
                    if let Some(id) = own_skolem(var) {
                        caps.push(((cls.clone(), id), binder.clone()));
                    }
                    args.push(TPattern::Var(binder, Ty::Unit));
                }
                self.ex_binder_counter += 1;
                TPattern::Constructor { name, args, skolems }
            }
            TPattern::Paren(inner) => TPattern::Paren(Box::new(self.ex_rewrite_pattern(*inner, caps))),
            TPattern::As(n, inner) => TPattern::As(n, Box::new(self.ex_rewrite_pattern(*inner, caps))),
            TPattern::Tuple(ps) => TPattern::Tuple(self.ex_rewrite_patterns(ps, caps)),
            other => other,
        }
    }

    /// The constrained-existential constructor at the head of a spine that
    /// applies exactly its declared fields.
    fn ex_saturated_ctor(&self, expr: &TExpr) -> Option<String> {
        let head = Self::app_head(expr);
        let TExprKind::Con(name) = &head.kind else { return None };
        let ex = self.ex_ctors.get(name)?;
        let n = ex.field_types.len();
        // The head must still carry its DECLARED type: a packed spine's head
        // has the extended one, and the dictionary-form pass revisits a
        // packed spine's children — the sub-spine below the appended
        // dictionaries applies exactly the declared fields and would
        // otherwise be packed again, without end.
        (Self::app_chain_len(expr) == n && Self::spine_arrow_count(&head.ty) == n).then(|| name.clone())
    }

    /// The dictionary arguments a pack of `ex_name` needs, from the ARGUMENT
    /// types: each field type (over the existential variables) is matched
    /// against the argument's type to bind the variables, then a dictionary
    /// is built per capture. `None` when some binding is a still-free type
    /// variable with no dictionary parameter for it (the first pass over a
    /// body that later goes dictionary-form: that pass packs it) or a
    /// skolem without a captured dictionary.
    fn ex_pack_dicts(
        &mut self,
        ex_name: &str,
        arg_tys: &[Ty],
        ev: &DictEnv,
    ) -> Option<Vec<TExpr>> {
        let ex = self.ex_ctors.get(ex_name)?.clone();
        let mut bind: HashMap<String, Ty> = HashMap::new();
        for (fty, aty) in ex.field_types.iter().zip(arg_tys) {
            Self::collect_subst_by_name(fty, aty, &mut bind);
        }
        let mut dicts = Vec::with_capacity(ex.captures.len());
        for (cls, var) in &ex.captures {
            let bound = bind.get(var).cloned()?;
            if !self.dict_evidence_for(cls, &bound, ev) {
                return None;
            }
            dicts.push(self.build_dict_expr(cls, &bound, ev));
        }
        Some(dicts)
    }

    /// The constructor's type with `k` hidden `()` parameters appended
    /// after its `n` declared fields.
    fn ex_extend_ctor_ty(ty: &Ty, n: usize, k: usize) -> Ty {
        let mut params = Vec::with_capacity(n + k);
        let mut t = ty;
        for _ in 0..n {
            match t {
                Ty::Arrow(a, b, _) => { params.push((**a).clone()); t = b; }
                _ => break,
            }
        }
        params.extend(std::iter::repeat_n(Ty::Unit, k));
        Ty::fun(&params, t.clone())
    }

    /// Rewrite a saturated pack: monomorphize the arguments (first pass
    /// only — the dictionary-form pass receives already-processed
    /// arguments), append the captured dictionaries, and give the head the
    /// extended constructor type so codegen sees a saturated application.
    fn ex_pack_saturated(
        &mut self,
        ex_name: String,
        whole: TExpr,
        mono_args: bool,
        ev: &DictEnv,
    ) -> Option<TExpr> {
        let ty = whole.ty.clone();
        let arg_tys: Vec<Ty> = {
            let mut tys = Vec::new();
            let mut e = &whole;
            while let TExprKind::App(f, a) = &e.kind {
                tys.push(a.ty.clone());
                e = f.as_ref();
            }
            tys.reverse();
            tys
        };
        let dicts = self.ex_pack_dicts(&ex_name, &arg_tys, ev)?;
        let (head, args) = Self::split_app_chain(whole);
        let args: Vec<TExpr> = if mono_args {
            args.into_iter().map(|a| self.mono_expr(a)).collect()
        } else {
            args
        };
        let n = args.len();
        let head = TExpr::new(head.kind, Self::ex_extend_ctor_ty(&head.ty, n, dicts.len()));
        let mut e = head;
        let total = n + dicts.len();
        for (i, a) in args.into_iter().chain(dicts).enumerate() {
            let node_ty = if i + 1 == total { ty.clone() } else { Ty::Unit };
            e = TExpr::new(TExprKind::App(Box::new(e), Box::new(a)), node_ty);
        }
        Some(e)
    }

    /// An unsaturated constructor reference as a lambda over its declared
    /// fields whose body is the packed application. `None` when the
    /// dictionaries cannot be built here (see `ex_pack_dicts`).
    fn ex_eta_expand(
        &mut self,
        name: &str,
        ty: Ty,
        ev: &DictEnv,
    ) -> Option<TExprKind> {
        let ex = self.ex_ctors.get(name)?.clone();
        let n = ex.field_types.len();
        // A constructor reference has exactly its declared arrows; the head
        // of an already-packed spine carries the EXTENDED type (declared
        // fields plus dictionaries) and must not be expanded again — the
        // dictionary-form pass revisits a packed spine's children.
        if Self::spine_arrow_count(&ty) != n {
            return None;
        }
        let mut params: Vec<(String, Ty)> = Vec::with_capacity(n);
        let mut t = &ty;
        for i in 0..n {
            let Ty::Arrow(a, b, _) = t else { return None };
            params.push((format!("__exp{}_{}", self.ex_binder_counter, i), (**a).clone()));
            t = b;
        }
        self.ex_binder_counter += 1;
        let result_ty = t.clone();
        let mut app = TExpr::new(TExprKind::Con(name.to_string()), ty.clone());
        for (i, (p, pty)) in params.iter().enumerate() {
            let node_ty = if i + 1 == n { result_ty.clone() } else { Ty::Unit };
            app = TExpr::new(
                TExprKind::App(Box::new(app), Box::new(TExpr::new(TExprKind::Var(p.clone()), pty.clone()))),
                node_ty,
            );
        }
        let packed = self.ex_pack_saturated(name.to_string(), app, false, ev)?;
        Some(TExprKind::Lambda { params, body: Box::new(packed) })
    }

    /// Is this a use of a constrained polymorphic function at a type that
    /// instantiates it over an existential skolem with captured
    /// dictionaries? Such a use cannot be specialized (nothing inside a
    /// copy at a skolem could dispatch) and takes dictionary passing.
    fn ex_dict_call_wanted(&self, name: &str, use_ty: &Ty) -> bool {
        !self.skolem_dicts.is_empty()
            && !self.locals.contains(name)
            && self.poly_fns.contains_key(name)
            && self.fn_constraints.get(name).is_some_and(|c| !c.is_empty())
            && Self::contains_skolem(use_ty)
            && self.has_skolem_dicts(use_ty)
    }

}

#[cfg(test)]
mod first_class_dict_error_tests {
    use super::*;

    /// The first-class-use diagnostic names the mechanism (A10): with a
    /// recorded trip count it states how many distinct instantiations were
    /// reached and what the limit is — the instantiation that crosses the
    /// cap can be far from the use site it breaks — and without one it
    /// still names the limit. (The reachable-in-full-compiles shape is the
    /// accept side, dict_dead_generic.mll: since G7 the common case
    /// compiles, which is exactly why the message is pinned here at its
    /// construction site.)
    #[test]
    fn diagnostic_names_limit_and_trip_count() {
        let checker = crate::typechecker::Checker::new();
        let mut m = Monomorphizer::new(&checker);
        m.dict_passing_counts.insert("grow".into(), SPEC_LIMIT + 1);

        m.push_first_class_dict_error("grow", &Ty::Var(crate::types::TyVar { name: "a".into(), id: 0 }));
        m.push_first_class_dict_error("plain", &Ty::Var(crate::types::TyVar { name: "b".into(), id: 1 }));

        let counted = &m.errors[0];
        let text = format!("{}", counted);
        assert!(text.contains("cannot pass 'grow'"), "got: {text}");
        assert!(text.contains(&format!("{} distinct type instantiations", SPEC_LIMIT + 1)),
            "the note must state the reached count, got: {text}");
        assert!(text.contains(&format!("the limit is {}", SPEC_LIMIT)),
            "the note must state the limit, got: {text}");
        assert!(text.contains("far from this use site"),
            "the note must warn about action at a distance, got: {text}");

        let uncounted = format!("{}", m.errors[1]);
        assert!(uncounted.contains(&format!("specialization limit of {}", SPEC_LIMIT)),
            "the fallback note must still name the limit, got: {uncounted}");
    }
}

#[cfg(test)]
mod spec_cap_tests {
    use super::*;

    fn key(name: &str, i: u32) -> SpecKey {
        SpecKey { name: name.into(), ty: Ty::fun(&[Ty::Con(format!("T{i}"))], Ty::Unit) }
    }

    /// The trip counts strictly more than `SPEC_LIMIT` existing copies:
    /// with 16 of them a 17th is still generated, and the 18th distinct
    /// instantiation trips (the documented "16" is the constant, not the
    /// copy count — pinned here so the prose and the code say the same
    /// thing). The trip records the count including the instantiation
    /// that crossed the cap, purges the copies, and switches a
    /// constraint-free function to the shared generic copy
    /// (`arity_only_fns`), a constrained one to dictionary passing.
    #[test]
    fn cap_trips_at_the_eighteenth_instantiation() {
        let checker = crate::typechecker::Checker::new();
        let mut m = Monomorphizer::new(&checker);
        for i in 0..(SPEC_LIMIT as u32) {
            m.specializations.insert(key("plain", i), format!("plain·{i}"));
        }
        assert!(!m.trip_spec_cap("plain"), "{} copies leave room for one more", SPEC_LIMIT);
        m.specializations.insert(key("plain", 99), "plain·99".into());
        assert!(m.trip_spec_cap("plain"), "the {}th distinct instantiation trips", SPEC_LIMIT + 2);
        assert_eq!(m.dict_passing_counts.get("plain"), Some(&(SPEC_LIMIT + 2)));
        assert!(m.arity_only_fns.contains("plain"), "a constraint-free function is shared");
        assert!(!m.dict_passing_fns.contains("plain"), "it has no dictionary form");
        assert!(m.specializations.keys().all(|k| k.name != "plain"), "its copies are purged");
        assert_eq!(m.purged_specs.get("plain·99").map(String::as_str), Some("plain"));

        m.fn_constraints.insert("shown".into(), vec![TyConstraint {
            class_name: "Show".into(), type_var: "a".into(),
        }]);
        for i in 0..(SPEC_LIMIT as u32 + 1) {
            m.specializations.insert(key("shown", i), format!("shown·{i}"));
        }
        assert!(m.trip_spec_cap("shown"));
        assert!(m.dict_passing_fns.contains("shown"), "a constrained function goes dictionary-passing");
        assert!(!m.arity_only_fns.contains("shown"));
    }

    /// A constrained function's purged copies keep their names: a copy
    /// still pending becomes the stub of its type (a dictionary-passing
    /// call to the original), an emitted one is left as it is, and neither
    /// is redirected — a reference to the bare dictionary-passing original
    /// is right only at a call site the rewrite reaches (T13).
    #[test]
    fn purged_copies_of_a_constrained_function_keep_their_names() {
        let checker = crate::typechecker::Checker::new();
        let mut m = Monomorphizer::new(&checker);
        let a = Ty::Var(TyVar { name: "a".into(), id: 0 });
        let poly = TFunction {
            name: "shown".into(),
            ty: Ty::fun(&[a.clone()], Ty::Con("String".into())),
            clauses: vec![],
            specialized: false,
            spec_origin: None,
            dict_params: vec![],
            derived_strict: false,
        };
        m.poly_fns.insert("shown".into(), poly.clone());
        m.fn_constraints.insert("shown".into(), vec![TyConstraint {
            class_name: "Show".into(), type_var: "a".into(),
        }]);
        for i in 0..(SPEC_LIMIT as u32 + 1) {
            let ty = Ty::fun(&[Ty::Con(format!("T{i}"))], Ty::Con("String".into()));
            let mangled = m.mangle_name("shown", &ty);
            m.specializations.insert(SpecKey { name: "shown".into(), ty }, mangled.clone());
            // The even copies are still pending, the odd ones were emitted.
            if i % 2 == 0 {
                m.generated.push(TFunction { name: mangled, ..poly.clone() });
            }
        }
        assert!(m.trip_spec_cap("shown"));
        assert!(m.dict_passing_fns.contains("shown"));
        assert!(m.purged_specs.is_empty(), "no copy of a constrained function is redirected");
        for i in 0..(SPEC_LIMIT as u32 + 1) {
            let ty = Ty::fun(&[Ty::Con(format!("T{i}"))], Ty::Con("String".into()));
            let name = m.mangle_name("shown", &ty);
            assert!(m.ground_forms.contains(&name), "{name} stays a function of its type");
            let stubbed = m.generated.iter().filter(|f| f.name == name).count();
            assert_eq!(stubbed, usize::from(i % 2 == 0), "a pending copy is replaced by one stub, an emitted one by none");
            if i % 2 == 0 {
                let stub = m.generated.iter().find(|f| f.name == name).unwrap();
                assert_eq!(stub.ty, ty);
                assert!(matches!(
                    stub.clauses[0].body.as_ref().map(|b| &b.kind),
                    Some(TExprKind::DictCall { func_name, dict_args, value_args })
                        if func_name == "shown" && dict_args.len() == 1 && value_args.len() == 1
                ), "the stub passes the dictionary and the value");
            }
            // A later resolution at the same type is that same name.
            assert_eq!(m.specialize_instance_fn("shown", &ty), name);
            assert_eq!(m.generated.iter().filter(|f| f.name == name).count(), usize::from(i % 2 == 0));
        }
        // A new ground type after the trip gets a stub too, once.
        let fresh = Ty::fun(&[Ty::Con("Fresh".into())], Ty::Con("String".into()));
        let stub = m.specialize_instance_fn("shown", &fresh);
        assert_eq!(stub, m.specialize_instance_fn("shown", &fresh));
        assert_eq!(m.generated.iter().filter(|f| f.name == stub).count(), 1);
        assert!(m.specializations.keys().all(|k| k.name != "shown"), "a stub is not a specialization");
    }

    /// A function that is already shared or dictionary-passing can only
    /// want arrow-shape copies; past `SPEC_LIMIT` of those the trip reports
    /// the exhaustion once and generates nothing — it never purges or
    /// switches a second time.
    #[test]
    fn shared_function_shape_copies_are_capped_with_an_error() {
        let checker = crate::typechecker::Checker::new();
        let mut m = Monomorphizer::new(&checker);
        m.arity_only_fns.insert("shared".into());
        m.poly_fns.insert("shared".into(), TFunction {
            name: "shared".into(),
            ty: Ty::fun(&[Ty::Var(TyVar { name: "a".into(), id: 0 })], Ty::Var(TyVar { name: "b".into(), id: 1 })),
            clauses: vec![],
            specialized: false,
            spec_origin: None,
            dict_params: vec![],
            derived_strict: false,
        });
        for i in 0..(SPEC_LIMIT as u32 + 1) {
            // one more arrow than the declared type: a widening shape
            let ty = Ty::fun(&[Ty::Con("A".into()), Ty::Con(format!("T{i}"))], Ty::Unit);
            m.specializations.insert(SpecKey { name: "shared".into(), ty }, format!("shared·{i}"));
        }
        assert!(m.trip_spec_cap("shared"));
        assert!(m.trip_spec_cap("shared"));
        assert_eq!(m.errors.len(), 1, "reported once");
        let text = format!("{}", m.errors[0]);
        assert!(text.contains(&format!("more than {} distinct function-type shapes", SPEC_LIMIT)), "got: {text}");
        assert_eq!(m.specializations.len(), SPEC_LIMIT + 1, "nothing purged");
        assert!(m.purged_specs.is_empty());
    }
}
