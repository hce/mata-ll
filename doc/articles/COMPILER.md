# mata-ll Compiler Internals

A map of how `mllc` turns a `.mll` source file into Lua, written so the
internals can be picked up and maintained without re-deriving them from
scratch. It documents the *why* and the invariants, not just the *what* —
the code is the source of truth for the *what*.

> Drafted by reading the code at commit `e1d2bdf`. Where it disagrees with
> the code, the code wins — fix this file. Section anchors use
> `file.rs:function` so they stay clickable as line numbers drift.

---

## 1. The pipeline at a glance

Everything is orchestrated by `mllc/src/lib.rs:compile`. The stages, in order:

| # | Stage | Entry point | Input → Output |
|---|-------|-------------|----------------|
| 1 | Lex | `lexer::lex` | source text → `Vec<Token>` |
| 2 | Parse | `parser::parse` | tokens → `ast::Module` (infix expressions still flat: `Expr::OpChain`, `Expr::Section`, `Type::OpChain`) |
| 3 | Resolve imports | `modules::ModuleLoader::resolve_imports` | module → module (imported decls merged, every reference bound to a canonical name — the program's in its module's scope, the compiler's own to their Prelude entities —, operator chains grouped; see §9) |
| 4 | Prepend prelude | `parse_prelude` + merge in `compile` | → single `ast::Module` |
| 5 | Desugar do-notation | `desugar::desugar_module` | AST → AST |
| 6 | Type check | `typechecker::Checker::check_module_with_local_start` | AST → **TIR** (`tir::TModule`) |
| 7 | Monomorphize | `mono::Monomorphizer::run` | TIR → TIR (specialized) |
| 8 | Verify | `mono::Monomorphizer::verify` (→ `verify::check`) | TIR → TIR (unchanged; errors if an invariant is violated) |
| 9 | Constant fold | `fold::fold_module` | TIR → TIR |
| 10 | Split deep expressions | `split::split_module` | TIR → TIR (deep nesting hoisted into `let`s so the emitted Lua stays within Lua's own parser limits) |
| 11 | Dead-code elimination | `dce::eliminate` | TIR → TIR (unreachable functions dropped) |
| 12 | Codegen (+ demand analysis) | `codegen::generate` | TIR → Lua source `String` |

Two ordering facts that are easy to get wrong and matter:

- **Demand (strictness) analysis is not a top-level stage.** It runs
  *inside* codegen: `codegen::generate` (`codegen/mod.rs`) calls
  `crate::demand::analyze(module)` first and stores the result in
  `cg.demand_info`. So it sees the *monomorphized, folded* TIR.
- **Constant folding runs after monomorphization on purpose.** Before mono,
  `==`, `<`, `<>` etc. are unresolved typeclass methods. After mono they are
  concrete functions (`eq_Integer`, `ord_gt__Number`, `semigroup_String`),
  which `fold.rs` pattern-matches as `App(App(Var(name), lhs), rhs)` and
  reduces. Folding earlier would miss most opportunities.

- **"Declared twice" is decided in the parser, per source file.** Stages 3
  and 4 merge the Prelude, every import and the user's file into ONE
  declaration list. Stage 3 gives every module's declarations canonical
  names of their own, so two modules never meet in that list; but the
  checks that one module declares a name once are easiest where the
  file is read. `parser.rs` therefore keeps
  two logs while it reads one file: the declaration table
  (`Parser::declared`, checked once by `check_declarations` at the end of
  `parse_module`: one declaration per name and namespace, and equations of
  a function grouped only when adjacent — `push_decl`; the signature
  namespace holds plain, `intrinsic` and `export` signatures alike,
  because `export f :: T` is parsed into an `ExportSig` plus the
  `TypeSig` the typechecker reads, of which it keeps one per name), and
  the pattern
  binder log (`Parser::pattern_binders`, checked by `check_linear` at
  every construct that binds a list of patterns: each variable once). The
  typechecker's own duplicate checks (constructors, a field in two types,
  a method in two classes, data/newtype/synonym names) work on the merged
  list; with canonical names they can only fire for two declarations of
  one module.

### The prelude / `local_start` trick

The prelude (`lib/Prelude.mll`, embedded via `include_str!`) is parsed and
**prepended** to the user's declarations, so the whole program — prelude +
imports + user code — is type-checked as one `Module`. But we still need to
know which declarations are the user's own, e.g. to decide what `main`/exports
belong to this file. That's what `own_count` / `local_start` carry: everything
at index `>= local_start` in `module.decls` is the user's own source. This is
threaded into the checker via `check_module_with_local_start`.

---

## 2. The two IRs

**AST (`ast.rs`)** — the parse tree. Untyped. Source `Pos` (col/row) attached
for error messages. Do-notation and list comprehensions exist here only
briefly: list comprehensions are desugared *in the parser*; do-notation is
desugared in stage 5.

**TIR — Typed Intermediate Representation (`tir.rs`)** — "like the AST but
every expression carries its resolved type." This is the central data
structure for the back half of the compiler. Key types:

- `TModule` — the typed program.
- `TExpr` (`tir.rs:77`) — an expression **plus its resolved `Ty`**. This
  invariant — *every `TExpr` has a concrete-after-substitution type* — is
  what makes monomorphization and type-directed codegen (e.g. type-specialized
  `show`) possible.
- `TExprKind` (`tir.rs:270`) — the actual expression shapes.
- `TFunction` / `TClause` / `TGuard` — functions are multi-clause with guards.
- `TPattern` — patterns for case/clause matching.

The type checker produces TIR; the monomorphizer, constant folder, demand
analysis, and codegen all consume and (except codegen) rewrite it.

---

## 3. Type system core (`typechecker/`, `types.rs`)

This is a Hindley–Milner inferencer in the Algorithm-W tradition, extended
with typeclasses, type families, GADTs, and kinds.

The checker is a directory, `mllc/src/typechecker/`, split by concern:
`mod.rs` (the `Checker` itself: instantiate/generalize/unify, the module
passes), `infer.rs` (expression/pattern inference + exhaustiveness),
`solve.rs` (typeclass constraint solving), `derive.rs` (`deriving`
implementations), `kind.rs` (kind inference/checking), and `usage.rs`
(linear-usage checking for `%1` arrows).

### Representation (`types.rs`)
- `Ty` (`types.rs:35`) — types: variables, constructors, applications, etc.
- `TyVar` — unification variables.
- `Scheme` (`types.rs:257`) — a polymorphic type: `forall` quantified vars +
  constraints + body. Produced by generalization, consumed by instantiation.
- `Subst` (`types.rs:300`) — substitution map from `TyVar` → `Ty`. The
  workhorse; `apply_subst` is applied throughout.
- `TyConstraint` — a class constraint like `Show a`.
- `Kind` (`types.rs:13`) — `Type` / `Symbol` / `Fn`, for kind checking (type
  families and type-level symbols need this).

### The three HM primitives
- `instantiate` (`typechecker/mod.rs:547`) — replaces a scheme's quantified vars
  with fresh unification vars when a polymorphic value is *used*.
- `generalize` (`typechecker/mod.rs:573`) — quantifies over free vars not bound in
  the environment when a binding is *defined* (let-generalization).
- **Unification** — solves `Ty ~ Ty` into the `Subst`, with the **occurs
  check** to reject infinite types. (Search `unify` in `typechecker/mod.rs`.)
  A `Ty::Forall` (a higher-rank parameter's type) unifies only with an
  alpha-equivalent `Forall` — both binders renamed to one fresh rigid
  constant, bodies unified, the constant captured by nothing — and with
  nothing else, a flexible variable included (`unify_inner` in
  `types.rs`; that is GHC's no-impredicative-instantiation rule). The
  only place a polymorphic ARGUMENT is accepted is the App rule
  (`infer_app` in `infer.rs`): the parameter's quantifier is skolemized,
  the argument checked against the rigid body, and the skolem may escape
  neither into the result type nor into the type of any binding of the
  environment. `f $ x` is typed by the same rule as `f x`; a lambda met
  by a rank-3 parameter type is checked against it (`check_lambda`), so
  its binder is bound at the quantified type as a scheme, exactly as a
  function clause's pattern is under a rank-2 signature; and a scheme
  whose type is itself quantified is instantiated at the occurrence
  (`instantiate_with_map`).

The mental model: inference walks expressions producing types and accumulating
a `Subst`; `instantiate` opens schemes at use sites, `generalize` closes them
at definition sites, `unify` reconciles. At the end the final substitution is
applied so every `TExpr.ty` is fully resolved.

### Module checking is multi-pass (`check_module`, `typechecker/mod.rs:3008`)
Order matters because later passes depend on earlier registrations:
1. **Pass 1** — type aliases, data types, newtypes.
2. **Pass 2** — typeclass declarations + type families.
3. **Pass 3** — type signatures + FFI info (`LuaPure`/`LuaIO`/`LuaTry` etc.).
4. **Pass 4a** — `deriving` clauses (must run before 4b so derived instances
   exist when explicit instances are checked). A derived instance is
   registered with its obligations (`DerivedObligation`: the superclass
   requirement, and for Show/Eq/Ord/a Bounded product the class at every
   field type), which are not decided here — a derived instance may lean
   on an instance declared anywhere in the module.
5. **Pass 4b** — explicit instance declarations (+ orphan detection).
   Every instance head is registered first (`preregister_instance`, over
   all instance decls); then the derived obligations are checked
   (`check_derived_obligations`), and then each written instance
   (`check_instance`): head shape, duplicates, orphans, and the
   superclass requirement — H2010 §4.3.2, each superclass constraint at
   the instance head entailed by the instance's context together with
   the instances in scope (`entailed_under_givens`), so a witness
   declared later in the module, a derived one (pass 4a) or a structural
   one (never registered — mono synthesizes it) all count. The
   superclass rule is one function for both origins
   (`check_superclasses`, `InstanceOrigin::Written`/`Derived`); a derived
   instance's field requirement is the same entailment under the
   derived context, per (constructor, field type).
6. **Pass 5** — synthesize FFI functions from sigs that have no body.
7. **Pass 6** — collect exports + check function definitions (the actual
   inference over bodies).

### Other checker responsibilities
- **Exhaustiveness checking** (`check_exhaustiveness`, `typechecker/infer.rs`)
  — warns/errors on non-exhaustive pattern matches (see §6 for the algorithm
  family).
- **Typeclass handling** (`typechecker/solve.rs`) — resolves which instance
  satisfies each constraint; this feeds monomorphization. Every class
  method carries its class's constraint on the class variable and the
  method signature's own context (`method_constraints`:
  `register_builtin_class` in `prelude.rs` lists both per builtin
  method — `traverse`'s `Traversable t` and `Applicative f` —
  `register_class` synthesizes the class's and keeps the source
  signature's through `class_method_own_context`, which rejects at the
  declaration what the list cannot hold: a constraint on the class
  variable, a compound argument, an unknown class, a variable the type
  does not mention), and `emit_use_constraints` turns each use into a
  *wanted* at the instantiated type, tagged with the span of the
  statement it arose in (`stmt_span`, the innermost `Expr::Spanned`
  marker being inferred), which is where a failure is reported; the
  binding's span is the fallback outside any statement. A builtin class also states GHC's
  `MINIMAL` formula (`ClassInfo::minimal`, beside its method list) and
  its default methods as mata-ll source snippets
  (`register_builtin_default`, parsed once by
  `install_builtin_defaults` into the same `default_methods` a source
  class fills); a source class gets GHC's no-pragma rule (every method
  without a default is required), the Prelude's own source classes
  their pragma from `prelude_source_class_minimal`. `check_instance`
  fills each omitted method from the defaults and warns
  (`warn_missing_methods`) for every minimal group the instance leaves
  empty — GHC's `-Wmissing-methods`, at the declaration. At the end of `check_function`,
  `discharge_wanted_constraints` (`infer.rs`) judges the leftovers in one
  place: a wanted with no instance is "No instance"; one mentioning a
  variable the binding's FINAL type does not determine (the `determined`
  set — the only place that decision is made, after every clause's
  unification) is "Ambiguous type", after `compute_numeric_defaults` has
  applied Haskell 2010 defaulting; one on a signature variable the
  declared context does not provide is the missing-context rejection
  (an instance method's declared context is the instance's context
  followed by the class method's own constraints — `check_instance`
  declares both under the method's minted name, so the body's givens
  and the dictionary convention come from one list). The monad
  hierarchy takes exactly these paths. A let/where group keeps monomorphic the variables its pending
  wanteds still need once reduced through the instances
  (`constraint_bound_vars`: `Monad (ST s)` needs nothing of `s`) and
  replaces a wanted over a variable it generalized by that residual
  (`reduce_wanted_over`), so no decided constraint reaches the boundary
  as an undetermined one. Instance resolution itself is one recursion,
  `entail_step`: what satisfying `class ty` at this node reduces to
  (satisfied, failed, deferred, or demands on the arguments — through a
  registered instance's context, or the structural rule for Eq/Ord/
  Show/Hashable over lists, tuples and Maybe). `has_instance` (a use
  site: a deferred variable is satisfiable, the enclosing context
  discharges it), `entailed_under_givens` (an instance declaration's
  superclass check: a deferred variable is satisfied only by the
  instance's context or a superclass of a context class, since nothing
  discharges it later), `collect_required_var_constraints` and
  `context_failure_note` are all thin walks over it.
- **Function clauses and GADT refinement** (`check_function` /
  `check_clause` in `infer.rs`). A signature's variables are freshened
  (`freshen_sig_type_mapped`) and every clause is checked against the
  SKOLEMIZED signature (`skolemize_sig_body_vars`): the clause's
  substitution starts as the skolemization, so its patterns and its body
  both see rigid variables and neither can narrow one (`f :: a -> Int`
  rejects `f x = x` and `f (Just _) = 1` alike). A constructor pattern
  is matched with `unify_refining` (`types.rs`): a skolem inside an
  INDEX of the scrutinee type (`G a` against `GI :: G Int`) is not a
  mismatch but a GIVEN of the branch (`SkolemMap`, `a ~ Int`), which the
  owner of the branch — a clause, a case alternative — applies to its
  view of the environment (`TypeEnv::refined`) and its expected result
  type, then forgets; a skolem at the HEAD of the scrutinee type (the
  whole type is `a`) goes to the plain unifier and is the rigid mismatch
  GHC reports. The refinement therefore never enters a clause's
  substitution: `Subst::merge` over the clauses agrees on every
  signature variable (all bind the one skolem), the function's final
  type is its declared type whichever clause refines, and a clause's
  wanteds are judged at the types its body saw (`Show Int` solved by
  the instance in the refining clause, `Show a` by the context in the
  other). The skolems are demoted back to the flexible variables once
  all clauses are checked (`demote_skolems`), so generalization, the
  dictionary convention and the monomorphizer see ordinary variables.
- **Deriving** (`typechecker/derive.rs`) — auto-generates `Show`/`Eq`/etc.
  A derived instance is registered like a written one: with GHC's derived
  context (`derived_context` — the class at each type parameter that
  occurs in a field type, nothing for a phantom parameter), recorded on
  the `InstanceInfo` and as each method function's `fn_contexts` entry,
  so the solver reads it through `instance_context_demands` and the
  monomorphizer builds the instance's dictionary form from it exactly as
  for `instance Show a => Show (Box a)`. Builtin instances registered
  without a context (`context: None`) keep the structural rule (every
  type argument needs the class). Deriving registers the instance and
  its `DerivedObligation` (see pass 4a/4b above); the check runs after
  every instance of the module exists.

---

## 4. Monomorphization (`mono.rs`)

The primary mechanism for compiling polymorphism and typeclasses to plain Lua.

Idea: walk the TIR, collect every **concrete** type instantiation of each
polymorphic function actually used, and emit one specialized copy per unique
instantiation with a **mangled name** (`count·IntTInt`: `ast::minted`, a
spelling no source name has), rewriting call sites to point at the
specialization. After this pass, typeclass methods are gone — they've become
ordinary monomorphic functions (`eq_Integer`, …), which is exactly why
constant folding (§7) can then recognize them.

- A `Demand` (poorly-named collision with §5 — here it means "a specialization
  request": function name + concrete type args) drives the worklist.
- **Dictionary-passing fallback:** pure monomorphization can't handle
  *polymorphic recursion* (a function that calls itself at a different type
  ad infinitum → unbounded specializations). For those cases the compiler
  falls back to passing typeclass dictionaries at runtime. So the model is
  "monomorphize by default, dictionary-pass when specialization wouldn't
  terminate." The guard is a per-function cap (`SPEC_LIMIT`, 16): the
  trip counts strictly more existing copies than the constant, so a
  function keeps up to 17 specializations and the 18th distinct
  instantiation trips it (`trip_spec_cap`). The trip purges every copy of
  that function — by recorded name, never by prefix — and switches it to
  the form that serves any number of instantiations: a constrained
  function becomes dictionary-passing (`dict_passing_fns`; `run`'s
  worklist rewrites its body and every call site), a constraint-free one,
  which has no dictionary form, keeps its generic copy and is shared like
  a prelude builtin (`arity_only_fns`). Call sites rewritten to a purged
  copy of a constraint-free function are redirected to the generic copy
  afterwards (`purged_specs`); a purged copy of a constrained function
  keeps its name — the copy itself where it was already emitted, else a
  stub of its type that calls the dictionary form with the dictionaries
  for that type (`dict_stub`, `ground_forms`) — because a reference to
  the bare dictionary-passing function is right only at a call site the
  rewrite reaches, and the references made before the trip sit anywhere:
  a first-class use, the element function a structural thread
  (`SpecKind::ListEq` & co.) carries by name. The same stub is what a
  dictionary-passing method resolves to at a concrete type after the
  trip (`specialize_instance_fn`). The
  one use neither shared form can serve is an arity-widening one (a
  result type variable instantiated to a function type — the N-ary
  convention emits one parameter per arrow of the declared type, and the
  body's own calls count arrows from the type they can see), so a widening
  use keeps a copy keyed by its arrow shape (`arity_shape`,
  `widening_copy`): an ordinary specialization of a shared function, a
  dictionary-passing copy of a dictionary-passing one. One copy per shape
  bounds them; more than 16 shapes of one function (a polymorphic
  recursion returning a function at every level) is a compile error, the
  one case with nothing to fall back to. The dictionary convention: one
  parameter per constraint of
  the **superclass closure** of the declared context (`superclass_closure`
  in `mono.rs`; `Ord a` gives an `Ord` and an `Eq` dictionary, in that
  order), each dictionary one class's method table. The typechecker
  discharges a superclass demand by entailment (`class_satisfies`), which
  leaves no runtime evidence, so the closure is what makes `x == y` under
  `Ord a` read a dictionary. The same closure gives a context instance's
  dictionary form its parameters and an existential constructor its
  hidden dictionary fields; `TFunction::dict_params` is the call
  convention every DictCall site agrees with. A match on a constrained
  existential constructor gets hidden trailing binders for those fields
  (`ex_rewrite_pattern`), each recorded as the evidence for its class
  at the pattern's OWN skolem — the one the checker minted for that
  pattern and stored on the node (`TPattern::Constructor::skolems`).
  The node is the only binding: every existential skolem is named after
  its constructor's variable, so a nested existential pattern over a
  same-named variable (`B (A y) p q`, both over `a`) is not
  distinguishable by name. The dictionary rewrite passes revisit the
  extended patterns and read the same node, so both passes see one
  mapping. Method dispatch is ONE
  decision for both passes, `dispatch_method` (see `MethodUse`): the
  main walk and the dictionary rewrite call it from every arm where a
  method can appear — a named use, an infix operator (the operator's own
  instantiated type, so either operand may carry the class variable), a
  first-class operator or section (`OpFunc`), prefix negation
  (`Negate`, the `negate` method at `t -> t`) and an overloaded literal
  (`fromInteger`/`fromRational` at `Integer -> t` / `Number -> t`; at
  `Number` the integer literal is converted right there into the
  `TLiteral::Number` GHC's `fromInteger` at Double denotes — see "A
  Number is a double" below). A
  binding that needs no evidence (`needs_evidence`: concrete, or a head
  whose instance demands nothing of the variable parts) resolves through
  `resolve_at_type`; a binding over type variables or existential
  skolems is routed through evidence when `dict_evidence_for` — the
  side-effect-free twin of `build_dict_expr`'s decision tree — finds
  every dictionary it needs in the body's `DictEnv` or the captured
  existential binders; otherwise it is deferred to the rewrite (inside
  the main walk of a dictionary-form body) or takes the type-erased best
  effort (the retained generic copy). The container shapes' Eq, Ord,
  Show and Hashable dictionaries are synthesized per shape
  (`structural_dict`), and a method with constraints of its own
  (`method_dict_constraints`: `traverse`'s `Applicative f`, a source
  class's `describeWith :: Show b => …`) has dictionary parameters for
  them in its dictionary form, after the instance context's, applied at
  every dictionary-routed use (`dict_method_use`); a ground type whose
  head instance has a context and such a method is composed rather than
  tabled (`head_method_needs_context`), and the method's function at
  the instance carries the same constraints in `fn_constraints`, so
  past the specialization cap it is rewritten dictionary-passing like
  any constrained function. A HashMap `hm*`
  operation takes the same decision through `hm_op_use`: at a concrete
  structural key type it becomes a wrapper threading the generated key
  encoder (`structural_key_enc`, Hashable's `__hashKey` at that type)
  and, for the writes, the structural compare the runtime stamps on the
  map handle; at a key type that needs evidence it becomes the
  operation's dictionary form (`hm_dictform`) applied to the key's
  Hashable dictionary, whose encoder decides the store flavor at run
  time (`__mll_hmd_*` in runtime.lua: the scalar encoder means the
  scalar store, `Hashable Number`'s own `__mll_key_number` the scalar
  store under the Number flavor); the enumerators read the key order
  off the handle and need no evidence. At key type `Number` the writes
  and the key-returning enumerators (`hm_number_op`) take the wrapper
  calling the Number family `__mll_hmn_*`: the writes stamp the handle
  (`nk`), the enumerators hand keys back as doubles (Lua 5.3+ stores an
  integral float key as an integer), lookup/member stay the scalar
  functions.

- **A Number is a double.** `Number` is an IEEE double on every host and
  never carries Lua's integer subtype (that subtype is `Int`'s): a
  Number that holds a Lua integer does int64 arithmetic that wraps at
  2^63, has no sign of zero, compares exactly where GHC rounds, and shows
  integer-style through the erased `show`. One invariant, kept at every
  producer:
  - an integer-syntax literal at `Number` becomes `TLiteral::Number`
    in `literal_use` (the nearest double: `n as f64`, a `BigInteger`
    through Rust's correctly rounded parser), so `fold.rs` folds
    arithmetic, comparison and negation at Number in f64 (`-0` is
    `-0.0`) and `literal_ast` spells it as a Lua float
    (`lua_number_literal`); the folder's own `Lit` arm normalizes any
    Integer literal still typed Number — the second line of the same
    defense; a literal PATTERN at Number is spelled as the rounded
    float by `collect_pattern_conditions` (and `__mll_lit_eq` compares a
    float scrutinee against the rounded literal);
  - the runtime: `fromInteger_Number` (the dictionary-routed literal
    `dict.fromInteger(5)`, `fromIntegral`) returns `x * 1.0` for a
    machine number (`* 1.0`, not `+ 0.0`: the sum would lose a `-0.0`),
    `__int_to_double` for a bignum — GHC's `integerToDouble`, correctly
    rounded (nearest, ties to even, ±Infinity past the range): up to
    three 2^24 limbs a Horner walk in doubles rounds exactly once (the
    top two limbs are an exact 48-bit integer, the scaling by 2^24 is
    exact, the last `+ limb` is the one rounding); from four limbs up the
    walk would round twice and lose the low limbs that decide a tie, so
    the wide path rounds the top four limbs (73 bits or more) ONCE —
    `hi + lo`, hi the top two limbs scaled by 2^48 (a float literal, so
    a 64-bit-integer host cannot wrap the product), lo the next two —
    and then decides whether the lower limbs can change that result:
    they add less than one unit of the rounded integer's lowest bit
    while its rounding midpoints are integers, so only an exact tie that
    went to even BELOW the midpoint moves, by one ulp, when some lower
    limb is nonzero. `delta = (hi - acc) + lo` is V - acc exactly
    (Sterbenz, then an integer sum below 2^53), and "half an ulp below"
    is `acc + 2*delta` landing exactly one ulp up. The result is scaled
    by the lower limbs' width with exact power-of-two multiplications
    (no `//`, bitwise ops or `pow`, so LuaJIT and PUC agree; a value
    past 2^1024 overflows to Infinity as IEEE says). A doubles-only
    host's `__int_to_machine` IS that conversion (its `Int` is a
    double); `__mll_read_number` parses integer syntax through the float
    lexer (`"-0"` → `-0.0`);
  - the FFI boundary: `ffi_decode_desc_inner` gives a `Number` leaf the
    `num` descriptor (checked and normalized by `__mll_ffi_decode`, and
    written back into a validation-only record/tuple so the host's table
    holds the double); a BARE `Number` result is `(call) * 1.0` inline at
    every result site (`ffi_number_result_ast`: pure/IO/method calls and
    `Const` reads) — one multiply, no decoder call on the `sqrt` path.
    The arg direction needs nothing (a Number leaving is a double);
  - the HashMap Number flavor above (keys enumerate as doubles);
  - lib/JSON.mll: `JNum` is a double, integer syntax and Ints beyond 2^53
    travel as `JInt`.
  Checkable two ways: `compile_with_whnf_refutation` rebinds
  `show_Number` to `__show_Number_checked`, which raises on the integer
  subtype at the point a Number is shown (every corpus case runs under
  it), and the backend fuzzer's `Number` vocabulary compares against a
  strict IEEE reference with GHC's `show` layout.
- **First-class operators** (`opfunc_value`): a first-class `(op)` is
  the value of whatever `op` denotes, decided once in the monomorphizer
  (the main walk and the dictionary rewrite call it from their `OpFunc`
  arms). The resolver binds an operator-position name like a prefix
  reference, so `op` names one entity: a local binder or an ordinary
  function — a user operator, a Prelude source operator (`^`, `$!`,
  `=<<`, `<$`), a backtick name — is the name's own use, a `Var` at the
  instantiated type walked on like the prefix spelling (specialization,
  arity-widening copies, dictionary passing, the cap); a class method is
  dispatched like an infix use; and only a spelling codegen lowers as a
  first-class value itself (`names::first_class_op_lowered`: `++`, `!!`,
  `:`, `$`, `.`, `>>=`, `>>`, the integer division wrappers, and the
  Lua-native operators `names::native_binary_op` spells) keeps the
  `OpFunc` node, after a method of that spelling has resolved natively
  (`+` at Int, IO's `>>=`) or been deferred. Codegen's `op_func_ast` emits
  exactly those lowerings — each a codegen-emitted function returning
  WHNF, so a higher-order consumer's single unwrap of its result is sound
  (`(++)` forces the suffix it hands the runtime, which returns it
  unchanged for an empty prefix) — and `builtin_op_lambda` refuses any
  spelling outside the native table: the old fallback emitted the
  spelling as a Lua operator, which made `(^)` Lua's float power and a
  user operator a Lua syntax error.
- **Dictionary construction** (`build_dict_expr`) is one decision tree
  per demanded (class, type): the body's parameter for a constrained
  variable, the captured binder for an existential skolem, and
  otherwise `decide_dict` — the instance's dictionary form closed over
  the dictionaries its context demands (`compose_instance_dict`, a
  `SpecKind::DictCtor`) for a type over the body's variables or hidden
  types AND for a concrete type whose instance method is itself
  dictionary-passing, the synthesized shape dictionary for a container
  over such elements (`structural_dict`), and a static table of resolved
  method implementations (`build_concrete_dict`, a `SpecKind::Dict`) for
  everything else. A CONCRETE type's dictionary needs no evidence from
  the demanding scope, so it is not decided at the site: the site holds a
  `SpecKind::DictDemand` placeholder and `run`'s dictionary phase decides
  every demand in one step (`materialize_dicts`), after the body and
  call-site rewrites of the pass. The phase is a fixpoint: rewriting a
  body can flag further dictionary-passing functions (a sibling
  constrained call), and deciding a dictionary can trip an instance
  method's cap (resolving `showsPrec` at the 18th `Box T` purges the
  method's copies and switches it to dictionary passing); a pass that
  flags or trips anything is followed by another, which rewrites the
  new bodies, re-runs the call-site pass, and decides every demand
  again, and the pass that changes nothing is the last — its decisions
  are the ones put in (`fill_dict_demands`). Deciding a table at the site
  froze specialization names that a later trip purged (T13: the purge
  rewrite renamed them to the dictionary form, and every call through
  the table was off by one argument); now no table is ever renamed.
  A dictionary FIELD holds a function for a method with arguments
  (every implementation is emitted as a Lua function) and the method's
  VALUE for a nullary one — a CAF slot, a thunk until forced
  (`SpecKind::DictCtor` builds the field by `method_field_arity`:
  the class signature's arrow spine plus the method's own dictionary
  parameters). Every dictionary read (`DictAccess`/`DictMethod`)
  carries that verdict as `nullary`, and the backend's WHNF and
  cheapness judgments (`is_cheap_with`, `expr_yields_whnf`) treat a
  nullary read like a variable read — forced where WHNF is required,
  passed raw where it is not — and a function-valued read as WHNF.
  A dictionary form's evidence table (`ensure_dictform`) names each
  context constraint after the variable as the method's BODY spells it
  (the checker's `a2114`, a default method's `_inst196`), not as the
  instance head does — the table is read by name, and with the head's
  names a two-variable context (`Show (Pair a b)`) read `b`'s dictionary
  as `a`'s.
- Type substitution into specialized bodies is what makes the copies concrete.

If you ever see runtime `nil` where a method should be, suspect a missing
specialization or a call site the rewrite didn't reach — start in `mono.rs`.

---

## 5. Demand / strictness analysis (`demand.rs`)

mata-ll is **non-strict by default**: codegen represents unforced values as
thunks. That's correct but allocates. Demand analysis recovers performance by
finding parameters that are *always forced on every path* through a function
body — those can be passed eagerly (no thunk) and forced at entry.

- Produces per-function strictness info (`DemandInfo`): the boolean
  `strict_params` rows plus structured per-field/per-element demand rows
  codegen uses for let/where bindings.
- **Cross-function, greatest fixed point:** if callee `g` is strict in
  argument `j`, then `f(... g(x) ...)` makes `f` strict in `x`. The analysis
  seeds *optimistically* (every parameter strict) and shrinks rounds until the
  rows are consistent (`analyze`). The greatest fixed point — rather than the
  least — is what lets self- and mutually-recursive parameters (notably tail
  accumulators) be recognized as strict.
- **A row applies to a call only when the call saturates it.** A row says
  what the callee forces when it *runs*; a partial application builds a
  closure and runs nothing, so `f x = seq (g x) 1` is lazy in `x` for a
  two-parameter `g` (GHC prints 1 for `f undefined`). One predicate
  (`call_saturates`: supplied arguments ≥ row length) gates every row
  consumer — the boolean rows (`demand_strict_args`), the structured rows
  (`apply_callee_row`) and the captured-demand sets of where-local
  functions — for prefix, specialized and dictionary-passing calls alike;
  an over-saturated call still applies the row to its own positions. The
  cost is precision on a partial application the body saturates later
  (`case g x of k -> k 1`): that argument stays a thunk.
- Runs late, inside codegen, so it sees final monomorphized code.

This is an optimization: getting it *wrong in the conservative direction*
(marking too few params strict) only costs speed, not correctness. Marking a
param strict that is *not* always forced would change semantics — that's the
direction to be careful about when editing.

The rows feed the per-parameter calling convention
(`codegen/analysis.rs`, `analyze_param_conventions`): one row per function,
computed once and read by BOTH the parameter binding and every call site,
so the two ends cannot drift. A position is forced on every path when the
demand row says so, or when the function's FIRST clause forces it on every
call — its pattern scrutinizes the column and no column to its left can
fail (`first_clause_forces_on_every_path`; a clause matches left to right
and is abandoned at the first failing column, so `f (x : _) (y : _) = …;
f _ _ = 0` forces column 1 only after column 0 matched, and `f []
undefined` is 0 as in GHC). Every other scrutinized column is forced inside
the clause conditions by the match emitter (`pattern.rs`,
`match_scrutinee`), once at the chain split when sound
(`later_clause_force_col`). A where-local function group applies the same
predicate.

The clause condition is built once, by `collect_pattern_conditions`, for
every match emitter (the clause chain, the guarded block, the
value-position `case`), and a pattern contributes its force to it whether
or not it tests anything: a tag, a literal, a `Just` or a cons test reads
the forced scrutinee, and a tuple, a single-constructor record or a
LuaDict record that tests nothing beneath it contributes the forced
scrutinee itself as an always-true conjunct (`Cond::Force`,
`push_force_cond` — its value is the table the pattern destructures), at
the pattern's left-to-right position. The `and` chain alone therefore
reproduces GHC's match order: `(_, _)` under a `Just` diverges on ⊥, and a
tuple column left of a failing column is forced before that column is
tried. An integer literal pattern's test is Haskell 2010 §3.17.2's
`scrutinee == fromInteger k`, decided by the pattern's checked type
(`TPattern::LitPat` carries it; the checker wants `Eq` and `Num` of it):
the monomorphizer's pattern walk (`literal_pattern_use`) leaves a pattern
at `Int`, `Integer` or `Number` alone, and codegen spells those as a
native `==` against the literal's exact 64-bit or double value
(`lua_int_literal`, `wrap_decimal_to_i64` for GHC's wrap of an
out-of-range literal at `Int`) or the bignum compare against the interned
`__mll_biglit` entry; at a type whose `==` is a function it rewrites the
pattern to `TPattern::LitEq`, the type's `==` (dispatched exactly like a
guard's, `dispatch_method`) over the literal converted by the type's
`fromInteger` (`literal_use`) — a module-level constant per (literal,
type) at a ground type, the dictionaries' methods in a dictionary-passing
body (the rewrite's revisit of the patterns decides what the main walk
deferred, through `cur_dict_env`), and the captured dictionaries of an
existential pattern for a literal in one of its own fields (which codegen
reads off the matched value, `pattern_dict_paths`, since the clause binds
nothing while its match is tested). A pattern whose type codegen cannot
name — a local binding left polymorphic — keeps the runtime's
type-directed `__mll_lit_eq`. A trailing force of a pattern that binds a variable is left to the
binding statement that follows the condition (`clause_condition`), and a
scrutinee already in WHNF (entry-forced, chain-split rebind, a forced
`case` scrutinee) is a bare name there and contributes no force.

---

## 6. Exhaustiveness & pattern matching

Pattern checking returns typed patterns (`check_pattern`,
`typechecker/infer.rs:791`); exhaustiveness lives in `check_exhaustiveness`
in the same file. Nested/overlapping constructor patterns (e.g.
red-black-tree-style `Branch R (Branch R a x b) y c`) are the stress case.
The algorithm family is matrix-based usefulness checking — see references.

---

## 7. Constant folding (`fold.rs`)
Post-mono TIR→TIR pass. Folds compile-time-known arithmetic, comparisons,
boolean logic, string concatenation, and literal negation. Recognizes both
built-in operators and the now-resolved monomorphic typeclass methods. Pure
peephole-style reduction; safe to extend with more recognized patterns.
A fold at a `Number`-typed node is double arithmetic: the `Lit` arm turns
any integer literal typed Number into the `TLiteral::Number` it denotes
before the operator folds see it (see "A Number is a double" in §4).

---

## 8. Codegen (`codegen/`)
TIR → Lua source. The code generator is a module directory,
`mllc/src/codegen/`. Emission is AST-based: the generators build a
`lua::Stmt`/`lua::Expr` tree (`codegen/lua.rs`) and print it once at the
end — no generator writes output text directly. The submodules by
concern: `mod.rs` (the `CodeGen` state struct and plumbing), `module.rs`
(module body layout: constructors, forward declarations, exports),
`function.rs` (functions, clauses, where-binding groups), `pattern.rs`
(pattern-match compilation), `expr.rs` (the main expression walk),
`thunks.rs`/`strictness.rs` (eager-vs-thunk decisions), `action.rs`
(IO/ST bind chains), `inline.rs`, `analysis.rs` (whole-program call-site
and inlining analyses), `ffi.rs`, `names.rs`, `util.rs`, and
`runtime.rs` + `runtime.lua` (the runtime prelude, emitted as an
on-demand subset of the definitions the program references). Generated
output is deterministic (guarded by the `codegen_is_deterministic`
test).

Responsibilities beyond the obvious translation:
- **Laziness:** emits thunks for non-strict values; uses `demand_info` to skip
  thunking strict params and `analyze_call_sites` to skip redundant `__force`
  calls on values already known concrete. The concreteness set
  (`concrete_vars`, consulted by the `Var` emission) is keyed by Lua name,
  and a claim is a property of ONE binding: every binder is introduced
  through `bind_local_name` (mod.rs), which withdraws any claim a
  same-named enclosing binder left, and the binding site inserts its own
  claim exactly where its source proves WHNF (a convention row, a strict
  `let`/`where` value, a WHNF-yielding bound action, a fused loop local);
  the scope snapshots (`ScopeSnapshot`, `VarsSnapshot` in function.rs)
  restore the set at every scope exit.
- **Where groups:** a clause's `where` group (`where_binds_stmts`) is
  emitted exactly once, by the emitter that has just bound the clause's
  pattern variables — `clause_intro_stmts` (pattern.rs) for every
  destructuring or multi-clause clause, the all-simple single-clause arm
  of `function_stmts` (its parameters are the pattern variables) and the
  value arms (a CAF or IO action has no patterns). No arm that goes on to
  `pattern_match_block` emits the group itself: the group's values and
  functions read the pattern variables, and a copy emitted before they
  are locals reads globals.
- **Constructor tracking:** record/ADT layout for construction and field access.
- **Type-specialized operations:** e.g. `show` for lists/tuples is emitted per
  element type, using the `Ty` carried on each `TExpr`.
- **FFI:** `LuaPure`/`LuaIO` bindings become direct Lua calls; method-call FFI
  (`":write"`) becomes `handle:write(...)`.
- **IO:** an action-typed expression in a value position compiles to an
  action value and performs nothing; a runner (`__mll_run`,
  `__mll_run_tail`) performs it. A single-clause IO function is emitted as
  two Lua functions: the performer, whose body runs the effects and which
  only a saturated call in run position reaches (keeping the direct call
  and Lua's tail calls on the hot path), and the action builder in the
  function's own slot, which every other use — a value-position call, a
  first-class reference, a partial application, a dictionary entry —
  reaches and which returns the action closure. A self-recursive IO/ST
  function with pattern dispatch becomes one action closure running a
  `while true` loop (`ioloop.rs`); its loop state is locals of that
  closure, initialised from the builder's arguments on every call, so each
  run of the action — a second run, a retry after a raise, a run nested
  inside its own — starts whole. `main` is skipped when the module is
  loaded via `require` (library use).
- **Calling convention:** a function value of type `T` is a Lua function
  of `count_arrows(T)` parameters, applied to all of them in one call;
  monomorphization gives each instantiation its own copy, so compiled
  code meets the convention at every call site. A definition with fewer
  patterns than arrows and a function-valued body declares the missing
  parameters as padding (`_etaN`) and applies each clause or guard result
  to them (`eta_call_ast`); the padding is an argument of the match
  emitter that reaches the result emission alone — never the clause's
  where group, a nested `case` in its body, or a where-local group, which
  is padded by its own arrow count. The runtime's definitions
  have no mata-ll body and exist once: their parameter lists fix their
  arity (`known_callee_arity` splits a call that over-applies one), and a
  function they are handed is called with the arguments of its declared
  parameter type (`intrinsics::RUNTIME_CALLBACKS`). Where an
  instantiation gives that function more parameters (`modifyIORef` on an
  `IORef (Int -> Int)`), the code generator wraps it in a currying
  adapter (`__mll_curry1`/`2`/`_n`) at every place it reaches the
  runtime (`adapt_runtime_args`).
- **Host limits:** Lua caps a function's locals at 200 and its upvalues
  at 60 (LuaJIT) or 255 (PUC). The emitter budgets locals as it
  declares them (`declare_local_parts`, `LOCAL_LIMIT`): past 180 a
  binding becomes a slot of a per-function `_v` spill table. Upvalues
  are budgeted by the last Lua-level pass (`upvalues.rs`,
  `UPVALUE_BUDGET` = 50) over the finished tree, where every earlier
  pass has fixed the capture sets: it counts per function the distinct
  enclosing locals it captures — chunk locals and runtime helpers
  included, nested functions' captures passed through — and routes the
  captures of a function over the budget through a table of the
  declaring scope, replacing each routed variable by its slot at every
  reference so the cell stays shared. The table local is budgeted
  against the host's 200 (`LUA_MAX_LOCALS`) — the pass runs last, so
  the emitter's margin is its to spend — and a function it cannot bring
  under the budget is a compile error, not an unloadable chunk.
- **Names:** `names::sanitize_name` is the one mapping from mata-ll names
  to Lua names, and it is injective: a runtime entity has its fixed Lua
  name; an identifier keeps its spelling with `'` as `_prime`, moved by
  trailing underscores off Lua's reserved words, the standard globals and
  the runtime's names (`end` -> `end_`, `end_` -> `end__`); `_x` is
  `_usr_x`; an operator is `_usr` plus one word per character; anything
  else (a canonical `M.f`, a generated name, a name containing `_prime`)
  is escaped whole under `__usr_`. Tables keyed by Lua name are therefore
  keyed by the mata-ll name. `names::reserved_binder` checks the emitted
  tree for a local declared under a reserved name (an internal error).

---

## 9. Modules (`modules.rs`)
Each `.mll` file is a module. `import Data.Tree` → `Data/Tree.mll`, searched in
the source dir + configured lib paths. Imported modules are parsed and their
declarations merged into the current module before type checking, so the whole
program is checked together (see the prelude/`local_start` note in §1).

The type checker and everything after it work on that ONE flat declaration
list and resolve names by plain string lookup. What keeps two modules' `helper`
apart is done entirely in `modules.rs`, before the merge
(`ModuleLoader::resolve_module`, one call per module, cached):

- **Entities and canonical names.** Every name a top-level declaration
  introduces is an entity with one canonical name, fixed by its defining
  module alone (`canon`): functions, types, data constructors, record
  fields, classes and class methods alike. There are three namespaces —
  values (functions, fields, methods), types and classes, constructors —
  and a constructor, field or method belongs to its type or class
  (`Entity::parent`), which is what `T(..)` in an import or export list
  selects. The canonical name is `Module.name` for an imported module — a
  spelling no source identifier can have — and the bare name for the entry
  module and the Prelude. An entry-module declaration whose bare name the
  Prelude already has in its namespace (or, for a value, the runtime
  library: `PreludeScope::claimed`) is `Main.name` instead (`Main.Bool`,
  `Main.True`, `Main.concatMap`); `main` is always bare. A program without
  imports therefore has exactly the names it is written with, except for
  such clashes.
- **Scopes.** A module's scope maps the bare names it may write to entities,
  one table per namespace: its own declarations; then what each unqualified
  import edge makes visible (the imported module's export *surface*,
  filtered by the edge's import list or `hiding`: `hidden_by_forms`); then
  the Prelude (`PreludeScope`: the Prelude source plus the checker's
  builtin environment, narrowed by explicit `import Prelude` forms; `:`,
  `[]` and tuples are syntax, always in scope). Own wins over imported,
  imported over Prelude; two different imported entities under one name
  are ambiguous. An alias has a table of its own: the surfaces of the
  modules imported under it. A surface follows the export list
  (Haskell 2010 §5.2): a type or class listed bare is exported without its
  constructors, fields or methods; `T(..)` exports all of them, `T(C, f)`
  the ones named, and a field or method may be listed as a value.
- **Binding happens once, in the resolver.** `Rename` rewrites the module's
  own declarations: declared names become canonical, and every reference not
  shadowed by a local binder is replaced by the canonical name of the entity
  the scope gives it. A reference that denotes nothing in scope (hidden,
  ambiguous, an alias that does not offer the name, the entry module's name
  seen from an imported module) is replaced by a placeholder spelling
  (`<scope error N>`), and `ast::Resolution::scope_errors` maps the
  placeholder to the error; the type checker reports it when it meets the
  placeholder, where the statement's location is known. Because a module's
  rewritten region depends on nothing an importer does, it is merged verbatim
  into every importer, once per origin (`origin_spans`).
- **Fixity is a property of an entity, and chains are grouped here.**
  The parser does not know how `a + b * c` nests: that depends on which
  declarations `+` and `*` denote, and a local binder, the module's own
  operator, an imported one and the Prelude's can share a spelling. So
  `parser.rs` returns every infix expression as a flat chain
  (`Expr::OpChain`: operands, operators and prefix minuses in source
  order; `Expr::Section`; `Type::OpChain` for `:`-operators in types).
  `ModuleLoader::fixities` maps canonical names to fixities: seeded with
  the Prelude's declarations (`fixity::prelude`, read from the Prelude
  source — there is no operator table in Rust), then extended by each
  module's own `infix…` declarations under the canonical names of the
  operators that module declares (`resolve_module`, phase 2; a
  declaration with no such operator is an error). `Rename` groups a
  chain at the moment it binds the chain's operators (`Rename::operator`
  → `fixity::group`): the fixity looked up is that of the canonical
  name just produced, `infixl 9` for a name in the local binder set, and
  a chain with an operator bound to a scope-error placeholder is replaced
  by that placeholder, so no grouping is chosen for a name that denotes
  nothing. `fixity.rs` holds the grouping itself (Haskell 2010 §10.6,
  including prefix minus and the section-operand rule) and its
  diagnostics; a section is rewritten to its lambda here, with an
  operand that is work let-bound around the lambda so it is evaluated
  once however often the section is applied (`Rename::section`); they surface as `CompileError::Parse` for the entry
  module and as an import error naming the module otherwise. The Prelude
  source and the checker's builtin default-method snippets are not
  modules `resolve_module` sees; `modules::group_baseline` runs the same
  `Rename` over them with empty tables. After stage 3 no chain node
  exists (the checker's arms for them are `unreachable!`).
- **Names bound through their owner, and instances.** Two kinds of
  reference are not looked up in the module's scope: the methods an
  instance declaration defines are the methods of its class
  (`ModuleLoader::class_methods`, `Rename::method_of`), and the field
  labels of a record construction `C { f = e }` are the fields of `C`
  (`ModuleLoader::con_fields`, `Rename::field_of`); a record update's
  labels are fields in scope. The label of a field selection `r.f` /
  `(.f)` is written by the parser as a reserved spelling
  (`ast::field_selection`) and bound to a field in scope — unqualified,
  else through the one alias offering a field `f` — past any local binder
  of that spelling (`Rename::field_selection`); `M.f` with `M` an alias
  is the same shape and becomes the qualified reference. Instances have no names: every instance of
  the program is in force everywhere, and two modules' private classes
  `Pretty` are two classes, each with its own `instance Pretty Int`. The
  constructor of an unsettled `newtype N = <type>` is settled here too
  (resolution phase 4): the head of the right-hand side when it names no
  type in scope and takes one argument (`newtype Rad = MkRad Double`),
  otherwise the type's own name. A reference that binds to the Prelude's
  `True` or `False` becomes the Boolean literal (`Rename::boolean`); the
  parser writes both as ordinary constructors, so a program's own `True`
  (after `import Prelude hiding (Bool(..))`) is a constructor of its own
  type.
- **References the compiler writes (`ast::WiredIn`).** A desugaring is
  spelled with Prelude functions: a list comprehension with `concatMap`, a
  range with an `enumFrom…` method, a partial match with `error`, a guard
  comma with `&&`, a `do` block with `>>=`. Such a reference is not one the
  program wrote, and nothing the program declares may change what it means.
  `WiredIn` lists these entities, and two rules in `Rename` keep them apart
  from the program's names. (1) Everything the parser builds refers to them
  by a reserved spelling (`WiredIn::reference`, `<wired-in concatMap>`: no
  source name can look like that); `Rename::v` binds it to the entity's
  canonical name — the bare Prelude name — without looking at the module's
  scope, so the module's own or imported `concatMap`, and an
  `import Prelude hiding (concatMap)`, do not matter. (2) The type checker
  and everything after it tell a local variable from a top-level name by
  spelling, and a local wins; so `Rename::local` gives every local binder
  that is spelled like a wired-in entity a reserved spelling of its own
  (`__local_error`), at the binding site and at every use. After stage 3
  the bare name of a wired-in entity is never a local variable, which is
  also what lets a later stage write the name directly: `desugar.rs`
  (stage 5) writes `>>=` and `error` that way. `Resolution::display_names`
  maps the binder spellings back, so diagnostics show `error`. A new
  desugaring that needs another Prelude function adds a `WiredIn` variant;
  a plain `Expr::Var("name")` in a desugaring would be an ordinary
  reference again, bound in the user's scope. The fresh BINDERS desugaring
  introduces (`__lamN`, `__tup_N`, `__wtup_N`, `__compN`, `__sec`, `__pgK_N`,
  `__tconN`) are a separate matter: they live in the `__` namespace, which
  the lexer refuses in source.
- **Names the compiler owns (`PreludeScope::claimed`, `ast::minted`).**
  Later stages also recognize the runtime library's functions (`map`,
  `not`, `hashmap_insert`, `show_Int`) and the builtin instances'
  implementations (`eq_Int`, `fmap_Maybe`) by spelling: instance
  resolution writes `show_Int` in place of `show` at `Int`, and
  `fold.rs`, `demand.rs` and the emitter key tables on these names.
  `Checker::claimed_names` lists them, and resolution keeps every program
  entity off them the way it keeps locals off wired-in names: an
  entry-module declaration of such a name — a function, a record field, a
  class method — gets the qualified canonical name (`Main.show_Int`,
  `canon`), a local binder the `__local_` spelling. A name in operator
  position — `` a `f` b ``, `(f)` — is bound by `Rename::op_ref` through
  the same value resolution (`v`) as a prefix reference, so a backtick
  operator is respelled with its binder: after resolution an `InfixApp`
  whose operator is spelled `div`, `mod`, `quot`, `rem` or `seq` is the
  builtin, which is what lets the emitter's `InfixApp` arm, `fold.rs`
  and `demand.rs` key their lowerings and rules on those spellings
  without a shadow check (`seq` is claimed through
  `names::RUNTIME_RENAMES`, the four others as the method
  implementations of the builtin `Integral Int` instance). Everything the
  compiler generates per program — instance methods (`show·Color`),
  derived helpers, specializations and dictionary forms (`count·IntTInt`,
  `show·Color·dictform` via `Monomorphizer::mangle_name`), Generic
  metadata — is named with `ast::MINT` (`·`), a character the lexer
  accepts in no identifier or operator, so no source name can equal one;
  `ast::shown` prints it as `_` in messages.
- **Canonical names downstream.** The checker refers to the bundled library
  by canonical name where it has built-in knowledge of it
  (`Checker::json_fn`, `library_type`, `library_class`, `library_con`,
  `library_value`: `JSON.jField`, `Data.Generics.K1`, `JSON.ToJSON`,
  `Data.Generics.L1`, `JSON.toJSON`); which derivable class a `deriving`
  entry denotes is decided on the canonical name (`Checker::derivable`),
  so a program's own class `Show` or `ToJSON` is never taken for the
  Prelude's or the library's. Diagnostics are rendered through
  `types::DisplayNames`, which spells canonical names the way their module
  wrote them (`Resolution::display_names`; a type, class or constructor
  whose spelling another of its namespace shares keeps its qualified
  form). Everything the program shows at run time uses the source
  spelling (`Resolution::source_names`): the Lua host key of an `export`,
  derived `show` for constructors and fields, a JSON tag or key, a
  LuaDict key or enum string, Generic `datatypeName`/`conName`/`selName`
  (`Checker::con_tag`, `Checker::field_key`, `TConstructor::tag`,
  `TRecordField::key`), JSON error contexts and FFI decode errors. Emitted
  Lua refers to functions and constructors by slot and integer tag, a
  dictionary's fields by the methods' source names (one class per
  dictionary, `CodeGen::dict_field`); canonical names reach it only in
  the spelling of a dictionary parameter (`__dict`, the constraint's
  position, `_` and the class's canonical name — `__dict1_Show` —
  through `sanitize_name`; one function of the monomorphizer,
  `dict_param_name`, mints every such name).

---

## 10. Algorithms & references

The hard parts are standard, documented algorithms — this list is the reading
list for fixing them solo:

- **Hindley–Milner inference (Algorithm W), let-generalization, occurs check**
  — Damas & Milner, *Principal type-schemes for functional programs* (1982);
  Mark P. Jones, *Typing Haskell in Haskell* (1999) — closest match to the
  instantiate/generalize/unify structure here.
- **Typeclasses via dictionary passing** — Wadler & Blott, *How to make ad-hoc
  polymorphism less ad hoc* (1989). The dictionary-passing fallback in `mono.rs`
  follows this; the default path specializes instead.
- **Monomorphization / specialization** — as in MLton and Rust; specialize each
  polymorphic function per concrete instantiation.
- **Exhaustiveness / pattern-match usefulness** — Maranget, *Warnings for
  pattern matching* (2007).
- **Strictness / demand analysis & lazy evaluation** — Peyton Jones, *The
  Implementation of Functional Programming Languages* (1987), esp. the chapters
  on graph reduction and strictness analysis.
- **Constant folding / peephole** — any standard compiler text (Appel, *Modern
  Compiler Implementation*).

---

## 11. Debugging entry points (cheat sheet)
- Wrong/odd **type error** → `typechecker/infer.rs`, find the relevant
  `infer_*` and check what `unify` (`typechecker/mod.rs`) is being asked to
  reconcile.
- **`nil` at runtime where a value/method expected** → `mono.rs` (missing
  specialization or unreached call-site rewrite).
- **Wrong strictness / unexpected eager-or-lazy behavior** → `demand.rs`, then
  how `codegen/thunks.rs` and `codegen/strictness.rs` consume `demand_info`.
- **Bad Lua output** → `codegen/` (start in `expr.rs` for expressions,
  `module.rs` for module layout); isolate by compiling a minimal `.mll` and
  reading the emitted Lua.
- **Import not found / wrong merge** → `modules.rs`.
- Reproduce any of these against the test suite in `mll-tests/tests/run_mll.rs`
  (1290+ tests; each compiles and runs an `.mll`). The `ghc_oracle_*` subset
  additionally diffs runtime output byte-exactly against pinned GHC goldens
  (`mll-tests/tests/ghc-golden/`, regenerated by
  `mll-tests/regenerate-ghc-goldens.sh`; known divergences are enumerated in
  `mll-tests/tests/ghc-golden/DIVERGENCES.md`). The `ghc_oracle_prog_*`
  tests are the differential program corpus (`mll-tests/tests/programs/`,
  whole programs, no exclusions — see its README); a failure there is a
  compatibility defect to fix on the compiler side.
