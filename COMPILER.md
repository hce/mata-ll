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
| 2 | Parse | `parser::parse` | tokens → `ast::Module` |
| 3 | Resolve imports | `modules::ModuleLoader::resolve_imports` | module → module (with imported decls merged) |
| 4 | Prepend prelude | `parse_prelude` + merge in `compile` | → single `ast::Module` |
| 5 | Desugar do-notation | `desugar::desugar_module` | AST → AST |
| 6 | Type check | `typechecker::Checker::check_module_with_local_start` | AST → **TIR** (`tir::TModule`) |
| 7 | Monomorphize | `mono::Monomorphizer::run` | TIR → TIR (specialized) |
| 8 | Constant fold | `fold::fold_module` | TIR → TIR |
| 9 | Codegen (+ demand analysis) | `codegen::generate` | TIR → Lua source `String` |

Two ordering facts that are easy to get wrong and matter:

- **Demand (strictness) analysis is not a top-level stage.** It runs
  *inside* codegen: `codegen.rs` calls `crate::demand::analyze(module)` near
  the end (`codegen.rs:~2655`) and stores the result in `cg.demand_info`.
  So it sees the *monomorphized, folded* TIR.
- **Constant folding runs after monomorphization on purpose.** Before mono,
  `==`, `<`, `<>` etc. are unresolved typeclass methods. After mono they are
  concrete functions (`eq_Integer`, `ord_gt__Number`, `semigroup_String`),
  which `fold.rs` pattern-matches as `App(App(Var(name), lhs), rhs)` and
  reduces. Folding earlier would miss most opportunities.

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

## 3. Type system core (`typechecker.rs`, `types.rs`)

This is a Hindley–Milner inferencer in the Algorithm-W tradition, extended
with typeclasses, type families, GADTs, and kinds.

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
- `instantiate` (`typechecker.rs:157`) — replaces a scheme's quantified vars
  with fresh unification vars when a polymorphic value is *used*.
- `generalize` (`typechecker.rs:167`) — quantifies over free vars not bound in
  the environment when a binding is *defined* (let-generalization).
- **Unification** — solves `Ty ~ Ty` into the `Subst`, with the **occurs
  check** to reject infinite types. (Search `unify` in `typechecker.rs`.)

The mental model: inference walks expressions producing types and accumulating
a `Subst`; `instantiate` opens schemes at use sites, `generalize` closes them
at definition sites, `unify` reconciles. At the end the final substitution is
applied so every `TExpr.ty` is fully resolved.

### Module checking is multi-pass (`check_module`, `typechecker.rs:1430`)
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
- **Exhaustiveness checking** (`typechecker.rs:2844`) — warns/errors on
  non-exhaustive pattern matches (see §6 for the algorithm family).
- **Typeclass handling** (`typechecker.rs:1612`) — resolves which instance
  satisfies each constraint; this feeds monomorphization.
- **Deriving** (`typechecker.rs:1807`) — auto-generates `Show`/`Eq`/etc.

---

## 4. Monomorphization (`mono.rs`)

The primary mechanism for compiling polymorphism and typeclasses to plain Lua.

Idea: walk the TIR, collect every **concrete** type instantiation of each
polymorphic function actually used, and emit one specialized copy per unique
instantiation with a **mangled name**, rewriting call sites to point at the
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

- Produces per-function strictness info (`DemandInfo`).
- **Cross-function, fixed-point:** if callee `g` is strict in argument `j`,
  then `f(... g(x) ...)` makes `f` strict in `x`. The analysis iterates
  rounds until no new strict parameters are discovered (`analyze`).
- Runs late, inside codegen, so it sees final monomorphized code.

This is an optimization: getting it *wrong in the conservative direction*
(marking too few params strict) only costs speed, not correctness. Marking a
param strict that is *not* always forced would change semantics — that's the
direction to be careful about when editing.

---

## 6. Exhaustiveness & pattern matching

Pattern checking returns typed patterns (`typechecker.rs:3122`); exhaustiveness
lives at `typechecker.rs:2844`. Nested/overlapping constructor patterns (e.g.
red-black-tree-style `Branch R (Branch R a x b) y c`) are the stress case.
The algorithm family is matrix-based usefulness checking — see references.

---

## 7. Constant folding (`fold.rs`)
Post-mono TIR→TIR pass. Folds compile-time-known arithmetic, comparisons,
boolean logic, string concatenation, and literal negation. Recognizes both
built-in operators and the now-resolved monomorphic typeclass methods. Pure
peephole-style reduction; safe to extend with more recognized patterns.

---

## 8. Codegen (`codegen.rs`)
TIR → Lua source. Responsibilities beyond the obvious translation:
- **Laziness:** emits thunks for non-strict values; uses `demand_info` to skip
  thunking strict params and `analyze_call_sites` to skip redundant `__force`
  calls on values already known concrete.
- **Constructor tracking:** record/ADT layout for construction and field access.
- **Type-specialized operations:** e.g. `show` for lists/tuples is emitted per
  element type, using the `Ty` carried on each `TExpr`.
- **FFI:** `LuaPure`/`LuaIO` bindings become direct Lua calls; method-call FFI
  (`":write"`) becomes `handle:write(...)`.
- **IO:** `__mll_run` forces IO thunks in `>>=`; `main` is skipped when the
  module is loaded via `require` (library use).

---

## 9. Modules (`modules.rs`)
Each `.mll` file is a module. `import Data.Tree` → `Data/Tree.mll`, searched in
the source dir + configured lib paths. Imported modules are parsed and their
declarations merged into the current module before type checking, so the whole
program is checked together (see the prelude/`local_start` note in §1).

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
- Wrong/odd **type error** → `typechecker.rs`, find the relevant `infer_*` and
  check what `unify` is being asked to reconcile.
- **`nil` at runtime where a value/method expected** → `mono.rs` (missing
  specialization or unreached call-site rewrite).
- **Wrong strictness / unexpected eager-or-lazy behavior** → `demand.rs`, then
  how `codegen.rs` consumes `demand_info`.
- **Bad Lua output** → `codegen.rs`; isolate by compiling a minimal `.mll` and
  reading the emitted Lua.
- **Import not found / wrong merge** → `modules.rs`.
- Reproduce any of these against the test suite in `mll-tests/tests/run_mll.rs`
  (246 tests; each compiles and runs an `.mll`).
