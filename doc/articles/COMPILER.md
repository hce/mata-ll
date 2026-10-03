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
   exist when explicit instances are checked).
5. **Pass 4b** — explicit instance declarations (+ orphan detection).
6. **Pass 5** — synthesize FFI functions from sigs that have no body.
7. **Pass 6** — collect exports + check function definitions (the actual
   inference over bodies).

### Other checker responsibilities
- **Exhaustiveness checking** (`check_exhaustiveness`, `typechecker/infer.rs`)
  — warns/errors on non-exhaustive pattern matches (see §6 for the algorithm
  family).
- **Typeclass handling** (`typechecker/solve.rs`) — resolves which instance
  satisfies each constraint; this feeds monomorphization.
- **Deriving** (`typechecker/derive.rs`) — auto-generates `Show`/`Eq`/etc.

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
  terminate."
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
- Runs late, inside codegen, so it sees final monomorphized code.

This is an optimization: getting it *wrong in the conservative direction*
(marking too few params strict) only costs speed, not correctness. Marking a
param strict that is *not* always forced would change semantics — that's the
direction to be careful about when editing.

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
  calls on values already known concrete.
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
  reaches and which returns the action closure. `main` is skipped when
  the module is loaded via `require` (library use).
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
  diagnostics; they surface as `CompileError::Parse` for the entry
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
  `canon`), a local binder the `__local_` spelling. Everything the
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
  the spelling of a dictionary parameter (`__dict_` plus the class's
  canonical name, through `sanitize_name`).

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
