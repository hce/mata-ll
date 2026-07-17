MATA-LL TODO
============

## Planned — top priority

## Completed

- [x] **Linear types (`a %1 -> b`), full — exactly-once + multiplicity
      polymorphism.** `%1` is GHC's linear arrow: consumed EXACTLY once
      (using it zero times leaks, more than once double-frees; both reject);
      `%Many` / `->` are unrestricted. Syntax: `%1`, `%Many`, `%'Many`, and
      `%m` (a named multiplicity variable) before `->`. Multiplicity lives on
      `Ty::Arrow` with identity-blind Eq/Hash and its own unification lattice
      (One / Many / flexible Var / rigid `%m`, invariant like GHC — a
      plain-arrow function is rejected at a `%1` type). Multiplicity
      polymorphism: schemes quantify rigid `%m` ids (minus those free in the
      env, closing an alias-laundering hole), instantiate to fresh flexible
      vars per use, and a rigid var is caller-chosen — so `apply :: (a %m -> b)
      -> a %m -> b` threads a linear value through helpers, local `where`/`let`
      functions (per-parameter multiplicity inferred to a fixpoint), and
      IO/ST/Maybe binds. Enforcement is a dedicated usage pass over the final
      typed IR (`mllc/src/typechecker/usage.rs`), NOT a re-threading of
      Algorithm-W: 0/1/ω counting with sequential add, context scaling, a
      branch-join LOWER bound (used in every `case`/`if`/guard alternative or
      it is dropped on some path), and an absence-is-leak check at every scope
      exit. Under laziness a `let`/`where` RHS is scaled by its binder's use
      count, so a never-forced binding counts zero and leaks; consumption
      reachable only through a bypassable path (Maybe continuation,
      short-circuit operand, wildcard, discarded non-`()` result) rejects.
      Scalar aliases keep the memoization relaxation (usable repeatedly, forced
      at least once). Dictionaries stay unrestricted. Everything erases: the
      emitted Lua is byte-identical (regression-tested against the tracker
      decode). Boundary (all deviations REJECT, never false-accept — except the
      one noted): the Lua host side of a `%1` FFI signature is trusted; some
      constructs over-reject conservatively (wildcard over a tainted scalar
      scrutinee, non-`()` result discards, record updates on tainted records).
      One documented ACCEPT-direction gap inherited from the scalar-memoization
      exemption: an *untracked* scalar laundered multi-step through unrestricted
      code can be counted as consumed though laziness may not force it (GHC has
      no scalar exemption; direct forms all reject). Tests:
      linear_affine_basic.mll, linear_mult_poly.mll + the linear_rejects_* /
      erasure tests in run_mll.rs.

- [x] Lambda-calculus reducer — examples/lambda.mll; untyped de Bruijn lambda calculus, capture-free substitution + index shifting, normal-order reduction (fuel-bounded), deriving Eq on recursive Term; Church-encoding oracle (identity, boolean not/and, succ/plus/mult). Targeted the laziness/forcing machinery and found NO bug — that area now handles its hardest workload cleanly. Test: example_lambda_reduction.
- [x] Forcing-gap audit: a thunk-valued field reached by projection/destructuring must be forced before structural use. Fixed two cases (record accessor result; nested case-pattern fields) and verified the rest force at the consumer (tuple-get via show, tuple/struct Eq, cons elements, newtype-in-arithmetic, if-conditions, ==). Audit also found that record accessors were not first-class.
- [x] Record field accessors are first-class — emitted as real functions (with varargs forwarding) in addition to the inline `field r` fast-path, so `map field xs` and over-applied function-typed fields (`fnField r x`) work. Test: record_accessor_first_class.
- [x] Record field accessors (person.name)
- [x] newtype codegen (zero-cost wrapping)
- [x] Exhaustiveness checking for pattern matches
- [x] Better error messages (line numbers on type errors)
- [x] where clauses in functions
- [x] Operator sections: (+1), (1+)
- [x] deriving (auto-generate Show, Eq instances)
- [x] Apply final substitution to TIR
- [x] Prelude as .mll
- [x] User-defined type families
- [x] Kind checking (Type, Symbol, Fn)
- [x] Superclass constraints on instance declarations
- [x] Either, Ordering types in prelude
- [x] Show instance enforcement
- [x] Mutual recursion support
- [x] Composition codegen fix
- [x] GADTs (full pipeline: parser, type checker, exhaustiveness, codegen)
- [x] Non-strict evaluation with cheapness analysis
- [x] seq :: a -> b -> b (explicit forcing; preserves tail calls so seq-strict accumulators run in constant stack)
- [x] Guards in where-clause bindings
- [x] Do-notation: break on closing paren
- [x] __mll_run for IO thunk forcing in >>=
- [x] Orphan instance detection
- [x] Process intrinsic declarations properly
- [x] when :: Bool -> IO () -> IO ()
- [x] Concrete variable tracking to skip redundant __force calls
- [x] Tuple types: (a, b, c) with fst, snd
- [x] Type-specialized show for containers (lists of tuples etc.)
- [x] LuaIterator type family (Lua iterators → lazy MLL lists)
- [x] >> operator (IO then)
- [x] Zero-arg LuaPure constant access (math.pi)
- [x] Haskell-style newtype syntax (newtype Rad = Rad Number)
- [x] Method-call FFI (":write" → handle:write())
- [x] LIO library (file handles, stdin/stdout)
- [x] LMath library (math.* bindings)
- [x] CI pipeline with auto-merge dev → main
- [x] String escape sequences in codegen (\n, \t, \\, \" properly escaped)
- [x] Eq for tuples (element-wise comparison with type dispatch)
- [x] LuaTry type family (Lua nil-means-error → Either String a)
- [x] LuaCatch/LuaIOCatch type families (Lua raised error → Left, via pcall)
- [x] Operator fixity declarations (infixl, infixr, infix)
- [x] Infix-LHS definitions (a |+| b = ... and x `f` y = ..., not just prefix)
- [x] STArray with rank-2 scoped mutability (runST, newSTArray, etc.)
- [x] ByteString intrinsic type with binary I/O operations
- [x] Standard library: Regex, JSON, LOS, LString, LBit modules
- [x] Export keyword for Lua interop (export foo :: ...)
- [x] Polymorphic recursion via dictionary-passing fallback
- [x] Type substitution in monomorphized specializations
- [x] undefined (bottom) value — thunk that errors when forced
- [x] WASM build target (mllc-wasm crate, browser playground)
- [x] Type aliases (`type Pair a = (a, a)`, `Int` as alias for Integer)
- [x] `module Name (exports) where` header parsing
- [x] `putStr` (io.write FFI)
- [x] Skip main when loaded via require
- [x] Multi-line record syntax in data declarations
- [x] Lua compat CI (5.4, LuaJIT) and performance benchmark
- [x] IO action semantics test suite

## Typeclasses and dispatch

- [x] Eq as a proper typeclass gating == and /=
- [x] Ord as a proper typeclass gating <, >, <=, >=
- [x] Monad typeclass and >>= operator
- [x] Desugar do-notation through >>= instead of hardwiring

## Missing types and values

- [x] HashMap k v (intrinsic dictionary type, backed by Lua tables)
- [x] Any type (Lua interop: String | Integer | Number | Bool | Null | ...)
- [x] getArgs :: IO [String]
- [x] exit :: IO ExitValue (data ExitValue = Normal | Err Integer)
- [x] takeWhile, dropWhile (prelude)
- [x] Common list helpers in the auto-prelude: null, last, init, concat, span,
      zip, unzip, replicate, iterate, and, or, any, all, sum, product. Defined
      once in Prelude; Data.List re-exports them (so `import Data.List (...)`
      still works) and keeps the less common helpers (sortBy, nubBy, groupBy,
      intersperse, intercalate, partition, unfoldr, scanl, scanr, find, foldl',
      append, break').

## Codegen optimizations

- [x] Prelude runtime functions seeded as concrete
- [x] Monadic bind chain flattening (do-blocks → flat locals, no IIFEs)
- [x] If-expressions as statements in bind chain terminals
- [x] Small pure function inlining at call sites
- [x] Typeclass methods inlined as Lua operators
- [x] Whole-program call-site analysis for parameter concreteness
- [x] Eliminating __mll_run: compile-time type info instead of runtime introspection
- [x] Demand analysis for parameter strictness (per-function, branch-aware)
- [x] return/pure optimization: thunk only when argument contains unknown function calls
- [x] CI wasm build job with artifact upload
- [x] Record field accessors inlined as direct table indexing
- [x] Forward-declared functions packed into __mll_fn table (eliminates 200-local limit)
- [x] IO actions as proper closures (IO can't leak into pure code)
- [x] ST primitive inlining in gen_action (zero-overhead in bind chains)
- [x] Zero-arg IO action flattening (main/helpers use gen_bind_chain_io instead of nested IIFEs)
- [x] pure/return unwrapping in gen_action before type guard (fixes unresolved monad type variables in bind chains)
- [x] Defensive __mll_run for unresolved action types in gen_action (where-clause IO functions)
- [x] try/catch codegen: IO action argument deferred into pcall closure

## Open

- [x] **Prefix/partial `div`/`mod` crash at runtime — fixed.** `div 7 2` and
      `map (div 10) xs` type-checked but emitted `__force(div)(...)` against a
      Lua global that does not exist ("attempt to call a nil value"); only the
      backtick form and backtick sections worked. Fixed by reifying `div`/`mod`
      as first-class functions (the same treatment `seq` got): a prefix, partial,
      or first-class `div`/`mod` now resolves to the runtime wrappers
      `__mll_div_fn`/`__mll_mod_fn`, which force both arguments to WHNF and run
      the existing strict cores `__mll_div`/`__mll_mod`. The inline backtick
      `` a `div` b `` stays on the bare strict core with pre-forced operands, so
      the arithmetic hot path (e.g. the tracker mixer) is byte-identical — no
      redundant force. Regression test: `div_mod_prefix_forms.mll` (prefix,
      partial-via-`map`, first-class-via-`foldr`/higher-order with thunked
      operands, floor semantics on negatives, and zero-divisor raising through
      the prefix/first-class path).
- [x] **Existential unpacking does not skolemize — type-soundness hole —
      fixed.** `data ShowBox = forall a. MkShowBox a (a -> String)` with
      `coerce (MkShowBox x _) = x` was ACCEPTED and coerced anything to
      anything (GADT-syntax existentials leaked identically). Unpacking now
      mints a fresh rigid skolem per pattern (`check_pattern`), so unifying
      the hidden variable with any outer or concrete type is rejected with
      a provenance note naming the hiding constructor. Escape checks cover
      the function's own type (`check_clause`), `case` result types, and
      `where`-function types; the record-selector and record-update back
      doors are closed (existential fields have no selector and cannot be
      updated — as in GHC); GADT-syntax existentials (any signature
      variable not reaching the result type, explicit `forall` and
      contexts included) go through the same skolemization. Declared
      contexts (`forall a. Show a => …`) are enforced both ways: packing
      emits the wanted instance at the concrete type, unpacking provides
      exactly the declared classes (plus superclasses) on the skolem.
      SPEC.md's "cannot escape the branch" promise now holds; CAVEATS.md
      documents the remaining record restrictions. Tests:
      existential_constraints.mll and the existential_unpacking_* /
      existential_* error-path tests in run_mll.rs.
- [x] **IO bind/`return` forces the bound value eagerly — fixed.**
      `_ <- return (error "x")`, a bare `return (error "x")` statement in
      a do-block, and `fmap f (return ⊥)` all raised, where GHC leaves the
      value unforced — violating SPEC.md's eagerness contract ("bottom is
      never evaluated eagerly"). Fixed by making `return`/`pure` suspend a
      possibly-⊥ argument: gen_action and the first-class return/pure closure
      now emit the argument through the eagerness weighing (gen_arg,
      strict=false), so a provably-total value stays eager (`return 0` is a
      bare `0`) while a possibly-⊥ one is thunked and stays inert until
      demanded. A `<-`-bound variable is marked concrete only when the action
      yields WHNF (action_result_is_whnf), so a bound `return ⊥` is forced on
      use, not at the bind. One observable consequence, now matching GHC: a
      bottom returned inside `try` is not caught unless forced there (with
      `seq`) — the two IO tests that pinned the old eager behavior
      (div_mod_by_zero_raises, exceptions test 7) were updated to force inside
      the `try` via `seq`, the same idiom div_exact_and_zero.mll already used.
      Regression test: `return_non_strict.mll` (discarded/bound/`$`/terminal/
      fmap/tuple-field/Maybe forms stay lazy; a demanded returned bottom still
      raises). Found by the 0.1.3 soundness audit; documented in CAVEATS.md.
- [x] **Non-deterministic codegen — fixed.** Generated `.lua` was not
      reproducible: identical source compiled twice could differ, because some
      emission order followed `HashMap` iteration order. Three sources, all
      fixed by sorting/stack-ordering rather than relying on `HashMap` order:
      (1) record field accessors — `TModule.record_accessors` Vec is now sorted
      at construction (typechecker); (2) FFI function emission — `ffi_info` is
      now iterated in sorted-key order; (3) specialization resolution — a
      still-polymorphic recursive call inside a specialization picked an
      arbitrary entry via `self.specializations.iter().last()`; now resolves to
      the enclosing specialization via an explicit generation stack (`gen_stack`),
      which is both deterministic and correct under nested specialization.
      Guarded by `codegen_is_deterministic` (compiles a feature-rich fixture 8×,
      asserts byte-identical). Verified across every example and test case; the
      tracker decode stays byte-identical.
- [x] Default method implementations in class declarations (`x /= y = not (x == y)`)
- [x] Where-clause type unification: pre-registered fresh type variables now unified with inferred types
- [x] Higher-rank polymorphism (generalize beyond ST/LuaFunction scope sealing)
- [x] Reject bare type signatures with no definition (was silently compiling to nil at runtime; now a compile error, FFI sigs still allowed body-less)
- [ ] Strict ST monad variant (LuaStrictArray or similar) for performance-critical code — to be discussed; current closure-based ST is only ~4% slower than direct mutations
- [ ] **Constructor-level dead-code elimination (low priority).** DCE
      (`dce.rs`) prunes only the function call-graph (`functions` +
      `instance_fns`); data constructors are explicitly out of scope
      (`dce.rs:32-33`) and codegen emits *every* `module.data_defs`
      unconditionally (`codegen.rs:930-931`). So the four Prelude datatypes with
      constructor slots — `ExitValue`, `Any`, `Either`, `Ordering` — ship in
      every compiled file whether or not any live code references them (12
      `__mll_fn` slots; `Maybe`/`Bool`/lists are builtins and add nothing). A
      module that exports only code using `Either` still carries all 12. Bounded,
      fixed cost — left in when function-DCE was added because the big win was
      dropping the function-level Prelude. Fix: mark a constructor reachable iff a
      kept function constructs it (a `Con` name, already collected in
      `collect_expr`, `dce.rs:79`) or matches it in a pattern, then filter
      `module.data_defs` (drop a whole `data` only when none of its constructors
      are reached). The one gap to close: `collect_clause` (`dce.rs:64-73`) does
      not walk case-branch *patterns* today (function-DCE never needed pattern
      names), so constructor-DCE must also traverse patterns. ~15 emitted lines
      saved per file; new surface area, so not worth it until it matters.
- [x] Well-defined runtime errors when decoding a LuaUserData/LuaDict value that
      crosses the Lua FFI boundary. The type-directed FFI-result decoder
      (`__mll_ffi_decode`) now raises a descriptive
      "declared T but the host returned X" error for *every* shape mismatch —
      a record missing a declared field, a wrong-typed field or element, a
      scalar where a list/record/tuple was declared, a missing multi-return
      value — naming the position (field/element) and the host function.
      Multi-return tuple results are decoded like every other FFI result.
      Valid values are never rejected: bare scalar results stay check-free
      (hot path), and a mata-ll thunk round-tripping through the host as
      opaque state passes through untouched (laziness preserved).
      Test: ffi_decode_shape_mismatch_errors.
- [x] Layout: a function whose first argument is on the next line (`f` then newline then `(arg)`) is now consumed as an application. The cross-line continuation no longer requires a same-line argument (has_args) — the block-column check alone keeps siblings from being grabbed, now that block_indent is tracked correctly. Test: first_argument_on_next_line. Found writing examples/lambda.mll.
- [x] Layout: multi-line application-argument continuations now use the enclosing layout-block column (Haskell rule) instead of the function column. Introduced a `block_indent` field set at each block (top-level/clause, where, let, let-in-do, do, case, class/instance methods via parse_clause) to the block's item column; parse_expr_app gates cross-line continuation on `current_indent > block_indent`. So `f = foldr g 0` then `  [1,2,3]` now parses. Surfaced and fixed a 1-space misalignment in Data.List.sortBy that the old lenient rule had tolerated. Test: shallow_multiline_continuation. All 254 tests still pass.
- [x] `$` operator emitted literally in Lua when inlined into ST action codegen path (should always desugar to function application)
## Recently completed

- [x] try/catch exception handling (pcall-based, IO errors only)
- [x] fileLines: eager IO with Maybe-returning fReadLine (no lazy IO, no LuaIterator for IO)
- [x] gen_action hardened: pure/return, ST primitives, and unresolved types all before/around type guard
- [x] Audit: 28 zero-arg IO helpers across 3 test files were silently not executing; all fixed
- [x] Local variable table fallback: constructors, newtypes, and instance methods packed into `__mll_fn` table alongside functions; function-body `_v[N]` overflow slots when binding count exceeds 180
- [x] Existential types in data constructors (`data ShowBox = forall a. MkShowBox a (a -> String)` — parser, typechecker, and pattern matching support)
- [x] Deriving Functor (requires traversing constructor fields to find type parameter)
- [x] DataKinds: promoted data constructors as type-level tags ('Empty, 'NonEmpty)
- [x] Type-level naturals: promoted constructors with arguments ('S 'Z, 'S ('S 'Z)) for length-indexed vectors

## Done

- [x] List-of-tuple equality (recursive element eq generation for nested containers)
- [x] `>>=`/`>>` on non-IO monads in let-bindings (spine walker now skips non-IO monads, bind_List added)
- [x] Typeclass-constrained library functions work via source-merging compilation (not a bug with current model)
- [x] Deriving Enum and Bounded for simple enum types (toEnum, fromEnum, succ, pred, range syntax)
- [x] Cross-function demand propagation (if callee is strict in position j, propagate to caller)
- [x] Full strictness analysis (demand-driven call-site decisions, is_cheap_arg retained for trivial expressions)
- [x] Monad typeclass dispatch for >>= and >> (instances for IO, LuaIO, ST; proper error on missing instance)

## Parser

- [x] Multi-line function application (continuation lines indented past function column)
- [x] Multi-binding `let` in `do` blocks
- [x] Guards in combination with `where` clauses

## Haskell compatibility gaps

- [x] Eq for [a] and Maybe a (parameterized typeclass instances)
- [x] deriving Ord
- [x] List comprehensions
- [x] Backtick sections (`(`div` 2)` as a function)
- [x] Local function definitions in do-let (`let f x = ...`)
- [x] Inline case syntax (`case x of { A -> ...; B -> ... }`)
- [x] Module export control (export list parsed and enforced in typechecker)
- [x] where blocks at module level

## Known limitations

- [x] Typechecker stack overflow on CPS-heavy types (fixed: iterative right-spine processing for bind chains)
- [x] Top-level let-in value bindings (thunked values removed from concrete_vars)
- [x] Inliner captures free variables in lambda bodies (gen_expr_subst now handles Lambda)
- [x] `let bottom = error "msg"; const 1 bottom` forces bottom eagerly at call site (fixed: callee-side strictness — call sites pass args without forcing, callee forces at entry based on demand analysis)
- [x] Multi-line case in do-let can cause multi-line continuation to consume next statement as argument (fixed: case loop restores parser position on break so whitespace tokens aren't consumed)
- [x] Pattern-matching generators in list comprehensions (`[x | Ok x <- rs]`)
- [x] **Interprocedural `return ⊥` forced at the bind — fixed.** For an
      APPLIED user function whose terminal action is `pure e` (e.g.
      `mk n = do { _ <- return (); pure (error "boom") }`), the bind
      `v <- mk 1` used to raise even when `v` is never used, where GHC does
      not. Cause: `return e`/`pure e` was represented as `e` itself, so
      `__mll_run` could not distinguish "a thunk that computes which action to
      run" (must force to reach the closure) from "a value-action whose result
      IS a thunk or a function" (must not force/call) — it forced, raising, and
      the same conflation *called* a returned `pure (\x -> …)` with no
      arguments. Zero-arg and intraprocedural forms already escaped it (a
      zero-arg action compiles to a deferred closure `__mll_run` calls; an
      intraprocedural `x <- pure e` binds the value directly). Fixed with a
      tagged pure box: an escaping `pure e`/`return e` emits `__mll_pure(e)`
      (via `gen_pure_action`), and every action runner — `__mll_run`,
      `__mll_perform`, `try_`/`catch_`, the exported-function wrapper, and the
      outgoing-callback wrapper — unwraps the box WITHOUT forcing or calling
      it. Left bare (no box, so no per-action allocation) when provably safe:
      the payload is a tuple literal or `is_cheap_to_force`, AND its type is
      never a Lua function (scalars/unit/list/tuple) — so `__mll_run`'s force
      is a harmless no-op. Backend-transparent: the tracker decode stays
      byte-identical, and its hot mixer/ST path emits no boxes (only the
      per-chunk PCM cons and the fold base). The direct-bind short-circuit
      (`gen_bound_action`) keeps `x <- pure e` unboxed. Regression test:
      `return_bottom_interproc.mll` (applied `pure ⊥` bound-and-unused,
      applied pure-of-function, demanded-still-raises, value-preserving
      threading), plus `return_non_strict.mll` (intraprocedural/zero-arg) still
      green. Found by the 0.1.4 soundness follow-up.

## Testing

- [x] Comprehensive test suites for each library module (Regex, JSON, LOS, LString, LBit, LMath)
- [x] Stress tests for compiler limits (large ADTs, deep recursion, nested exprs, many functions/instances, long do-blocks, large patterns, deep types, many args, list ops, BST program)
- [x] Do-notation regression tests (eval order, let scoping, bind return unwrapping)
- [x] 246 tests passing (Lua 5.4 via mlua)

## Can defer

- [x] Lambda pattern matching

## Example programs (compiler stress tests)

- [x] Scheme interpreter — examples/scheme.mll; recursive Value/Expr/Env ADTs, closures-as-values, environment chaining, eval/apply, recursion via self-application; asserts results against known answers (test: example_scheme_eval). Monomorphizer handled it cleanly with no bugs.
- [x] Red-black tree — examples/redblack.mll; Okasaki balance with doubly-nested constructor patterns, RB-invariant + in-order-sorted oracles (test: example_redblack_invariants). Surfaced and fixed a parser bug: nullary constructors were rejected as pattern arguments (`Box R n`).
- [x] Type inference engine — examples/typeinfer.mll; Algorithm W (unify + occurs check + substitution composition), recursive Ty/Term ADTs, Either error plumbing, deriving Eq on Ty; normalized-type-string oracle (test: example_typeinfer_checks). Surfaced and fixed a codegen bug: `case` matching a nested pattern under a constructor whose payload was a thunk did not force the field before destructuring it (read thunk internals as field values).
- [x] Ray tracer — examples/raytracer.mll; Vec3/Ray/Sphere records, ray-sphere intersection with nested lets, Lambertian shading + shadows, PPM P3 output; tolerance-based geometric oracle + sentinel pixels (test: example_raytracer_renders). Surfaced and fixed a codegen bug: record field projections were not forced, so thunk-valued fields (from non-cheap construction like `s * va v`) reached arithmetic as Lua tables.
- [x] Huffman coding — examples/huffman.mll; recursive HTree ADT, sortBy-based tree build, code-table DFS, LBit bit-packing, ByteString roundtrip; self-checking via assert (test: example_huffman_roundtrip)

## String types (design decision)

String = Lua string permanently. ByteString = Lua string with explicit byte semantics (same runtime representation, type-level distinction only). Text = future UTF-8 type over ByteString, if/when Unicode support is needed.
