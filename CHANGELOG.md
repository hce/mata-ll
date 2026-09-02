# Changelog

All notable changes to mata-ll are recorded here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Both crates — `mllc` (the compiler library) and `mata-ll` (the `mll`
command-line compiler and runner) — share a single version line, so each entry
below applies to both.

## Versioning

mata-ll has not reached 1.0, so it uses a pre-1.0 scheme (`0.MINOR.PATCH`) with
a stronger stability promise than Semantic Versioning requires for `0.x`:

- **The `0.1.x` series is initial development.** Any release may change the
  language, the CLI, or the generated Lua in backward-incompatible ways relative
  to the previous release. `0.1.x` releases are not assumed interchangeable.
- **From `0.2.0` onward the minor version is the compatibility boundary.**
  Within a `0.MINOR` line, patch releases are backward compatible: upgrading
  `0.2.3 → 0.2.4` never breaks a program that compiled and ran under `0.2.3`. A
  breaking change requires a minor bump (`0.2.4 → 0.3.0`).

A change is *breaking* if an `.mll` program, a build invocation, or a Lua-host
integration that worked on the previous release must be modified to keep
compiling and behaving the same. (This covers the language, the `mll` CLI, and
observable behavior of the generated Lua; it does not govern the internal Rust
API of the `mllc` library crate.)

## [Unreleased]

### Added

- **`Data.IORef`** — GHC's plain mutable IO cell: `newIORef`, `readIORef`,
  `writeIORef`, `modifyIORef`, `modifyIORef'`, with GHC's exact laziness
  (writes don't force the value; `modifyIORef` stores the unevaluated
  `f old`; `modifyIORef'` forces to WHNF) and `instance Eq (IORef a)` as
  pointer identity. Polymorphic in the element, not region-scoped; in
  run-once do-block position the operations compile to bare Lua table
  reads/writes. The `atomicModifyIORef`/`mkWeakIORef` family is
  deliberately absent (single-threaded host, no weak-reference hook) —
  see HASKDIFF.

- **A speed-of-light benchmark suite** (`bench/`): idiomatic workloads,
  each timed against a handwritten-Lua twin that must print
  byte-identical output; the mll/twin ratio per workload ranks
  optimization work (baseline in `bench/README.md`). The original
  hm_churn mislabeled itself — measured, 81% of its wall time was a
  million lookups against a static map — so it split into `hm_lookup`
  (the read hammer) and a rebuilt `hm_churn` that actually churns
  (interleaved persistent insert/delete): the split surfaced that
  value-semantics write turnover really costs ~800x the mutating twin,
  a number the old blend hid at 47x.

### Changed

- **Fused pipelines inline small stage and fold bodies — the loop IS
  the handwritten one.** When a `map`/`filter` stage or the fold
  function of a fused pipeline is a small pure function (a module or
  where-bound single-clause definition with a cheap body, a lambda, a
  section), its body is emitted in place of the per-element call with
  the parameters substituted by the loop locals:
  `foldl' step 0 (filter odd (map (* 3) [1 .. n]))` with a where-bound
  `step a x = (a + x) \`mod\` p` now compiles to
  `a = (a + x0) % p` under `if x0 % 2 ~= 0`-style native tests — zero
  per-element calls, exactly the twin. The fusion strictness gates are
  computed on the original function values, unchanged; only the call's
  emission is replaced by the same body. Relocation is gated: a body
  referencing any local at its definition, or a name locally shadowed
  at the fusion site, keeps the closure call (pinned by the shadow trap
  in `cases/list_fusion_inline.mll`, byte-exact against GHC).
  list_pipeline drops from 29x to 8.1x the handwritten twin on Lua 5.5
  (wall 0.038s -> 0.010s) and from 2.1x to 1.4x on LuaJIT. Three
  general improvements feed the same rewrite and help all code:
  Prelude `odd` is a direct definition (`n \`rem\` 2 /= 0`,
  extensionally identical to `not . even`), saturated Prelude `not`
  emits natively (`(operand == false)` — the operand is typed `Bool`,
  on which that comparison IS Lua's `not`), and the cheapness
  predicates now recognize `rem`/`quot` and saturated primitive
  typeclass-method applications (`x > 5` reaches codegen as
  `ord_gt__Int x 5`) as the operator expressions they emit as.

- **Integer division of multi-limb operands is schoolbook (Knuth
  Algorithm D), replacing the bit-by-bit binary loop.** The magnitude
  divide now runs O(#dividend-limbs × #divisor-limbs) limb steps
  instead of O(dividend-bits) shift-subtract rounds, with a dedicated
  single-limb short-division path; every intermediate stays below 2^49,
  so the routine is exact on integer-Lua and doubles-only hosts alike.
  Dividing a ~9600-bit number by a ~4800-bit one is 16x faster on
  Lua 5.5 and 45x on LuaJIT (the gap grows with operand size), and
  decimal `show` of large Integers — repeated division — speeds up with
  it. Small-operand division keeps its native fast path unchanged.
  Pinned by `cases/integer_bigdiv.mll`: a byte-exact GHC (GMP) golden
  over every algorithm path — one-limb divisors, top-heavy divisors
  whose quotient estimate clamps, the add-back shape (2^95 over
  2^71+1), exact multiples and their ±1 neighbors — across all four
  sign combinations, with the divMod/quotRem and remainder-sign laws
  asserted per pair; the algorithm core was additionally verified
  against Python bignum on 6738 magnitude pairs (481 of them through
  the rare add-back branch) on both VMs.

- **Generated code allocates less and forces less on hot paths.** Four
  measurement-driven codegen changes, semantics unchanged (all pinned by
  the strictness-contract harness and the GHC goldens): references to
  runtime-provided functions are no longer defensively `__force`d (they
  can never be thunks); the strict-accumulator shape
  `let z = e in z `seq` rest` binds `z` eagerly instead of allocating a
  thunk it forces on the next line — this also removes the closure birth
  that aborted LuaJIT traces in strict folds; the boxed-Integer runtime
  ops carry per-argument strictness rows, so their call sites pass raw
  expressions instead of argument thunks; and Integer literals intern
  into the existing `__mll_biglit` constant pool instead of re-running
  `fromInteger` at every evaluation. A second batch continues the same
  program: a bare variable in a strict argument position passes raw (the
  callee's pinned force does the work — the call-site force was a
  duplicate); `map`/`filter`/`zipWith` force their function argument
  once per call instead of once per element and read the head of a
  just-forced cell directly; and `mod` by a nonzero integer literal
  emits native `%` (identical floor semantics, no trap possible)
  instead of the `__mll_mod` helper call. Measured on the bench suite
  across both batches (Lua 5.5 wall time): list_pipeline −22%,
  integer_arith −18%, arith_loop −13%, ioref_loop −10%; LuaJIT's
  integer_arith −37%.

- **Strict parameters take a site-forced calling convention.** When a
  parameter is forced on every path through its function and every
  delivery of an argument into it is a visible, position-covering call
  site — the function never escapes as a value, no partial application
  or `$`/`.` closure forwards into the position, and the function is
  not exported to the Lua host — the entry `__force` moves to the call
  sites, which pass raw when the emission proves WHNF-ness and force
  otherwise. One analysis (`analyze_param_conventions`) decides each
  parameter's convention and both ends read it, so callee and call
  sites cannot drift; in WHNF-refutation builds the callee re-checks
  the claim at entry with `__assert_whnf`, so the corpus second pass
  exercises the whole contract. On top of this, `expr_yields_whnf` now
  claims the WHNF-return invariant for direct applications of known
  module functions (declined for newtype-typed and action results — a
  newtype constructor is transparent, so such a result can be a raw
  thunk — and for inline candidates), which lets those call results
  flow into strict positions with no force at all. Together the two
  changes cut the generics_json encode path from 566 to 68 `__force`
  calls per record. Bench (Lua 5.5 wall time): arith_loop −84%
  (19.5x → 3.1x speed-of-light ratio), generics_json −55%
  (144x → 61x), list_pipeline and integer_arith −5% each; the tracker
  canary improves ~9% under LuaJIT (5.3x → 5.8x realtime). The scan
  also learned two escape channels it was blind to: a specialization
  payload's embedded functions (an element eq/show/compare threaded
  through a runtime helper, a dictionary method table) and
  user-operator infix call sites.

- **IO self-loops over IORefs run closure-free.** Two changes: the IO
  self-loop conversion's repeat-safe vocabulary now admits
  `__mll_lit_eq` (the numeric-literal pattern comparison — one
  idempotent force plus a pure compare), whose absence declined the
  conversion for every `go r 0 = …` loop; and a discarded
  `modifyIORef' r (\v -> e)` statement splices the lambda body into
  the read-compute-write sequence instead of allocating the argument
  closure — the loop's last per-iteration allocation. ioref_loop under
  LuaJIT drops from 22x to **1.1x** the handwritten twin (the trace
  compiles end to end; 27x before the round), and Lua 5.5 from 90x to
  31x (−66% wall). Pinned by the GHC-goldened ioref cases, which hit
  the spliced path throughout.

- **`mconcat` is a `Monoid` class method, and the `String` instance
  overrides it with a linear builder.** The class now carries `mconcat`
  with GHC's default (`foldr mappend mempty`), exactly as in base —
  a GHC-parity improvement in itself — and `instance Monoid String`
  overrides it with a `table.concat` builder: the right-nested fold
  copies the whole suffix at every step (O(total bytes²) for a flat
  concatenation), the builder collects the forced elements and joins
  once. Result and forcing behavior are identical to the default
  (String is WHNF-atomic; the spine walk is the demand either way),
  pinned byte-exact against GHC by the `mconcat_method` case and the
  strictness-contract probe. string_build drops 43% on Lua 5.5
  (11.8x → 6.9x speed-of-light) and 37% under LuaJIT; the tracker
  canary improves to 6.0x realtime. A flat `mconcat` over thousands of
  strings also no longer risks LuaJIT's fixed C stack — the builder is
  iterative where the fold recursed per element.

- **The scalar-key HashMap operations carry strictness rows, and a
  nonzero-literal divisor no longer counts as trapping.** Every
  `hashmap_*` body (and its dynamic/encoded twins) forces every
  argument on every path, so `hmInsert`/`hmLookup`/… now have
  per-argument strictness rows (probed by the strictness-contract
  harness like the Integer core), and the eager-argument judgment
  learned that `div`/`mod`/`quot`/`rem` by a nonzero integer literal
  cannot trap (the family's one failure is the zero divisor) —
  mirroring the emission rule that already lowers such `mod` to native
  `%`. Together these stop the hm_churn lookup loop allocating a key
  thunk per iteration and let the insert/delete accumulators run
  eagerly. hm_churn: −78% under LuaJIT (36x → 7.7x speed-of-light; the
  loop now traces) and −25% on Lua 5.5 (55x → 45x); the tracker canary
  reaches 6.2x realtime.

- **Thunks are closure-free.** A suspension used to be
  `__thunk(function() … end)` — one fresh closure per thunk, and closure
  creation (FNEW) is the one bytecode LuaJIT cannot trace through, so
  every lazy allocation aborted the surrounding trace. A new pass
  (thunklift, pass 0 of the optimization pipeline) lifts an eligible
  thunk body to a module-level function created once and rewrites the
  site to a plain table carrying the captured values
  (`__mll_tk2(__mll_tkf[k], x, y)`); lazy-cons tails get an even lighter
  metatable-free carrier (`__mll_gen*`, flavor marked in the cell's
  `__lazy` flag), and the runtime's own list producers (map, filter,
  zipWith, take, append) use it too. Eligibility is where value capture
  provably equals Lua's reference capture: every free local of the body
  is single-assignment and read-only (recursive-let forward references
  and loop-carried parameters keep the closure form), bodies are
  Raw-free, and captures cap at three. The WHNF-refutation corpus pass
  exercises the new `__force` dispatch end to end. LuaJIT wall times:
  list_pipeline −56% (131x → 62x speed-of-light), string_build −50%,
  generics_json −28%, and the tracker canary jumps 6.2x → 8.1x
  realtime; PUC Lua is neutral (the lighter carriers exist precisely to
  keep it so).

- **`case hmLookup k m of Just v -> …; Nothing -> …` fuses to a raw
  slot read.** hashmap_lookup allocates a `Just` cell per hit for a
  value that shape tears apart on the next line — a million such
  allocations in the hm_lookup benchmark. When the scrutinee is a
  direct call of the Prelude `hmLookup` (scalar keys; structural and
  polymorphic keys were already rewritten to generated wrappers) and
  the branches are exactly unguarded `Just`-of-variable-or-wildcard
  and `Nothing`, the case emits a nil test on the raw slot instead,
  unwrapping the stored-nil sentinel in the Just branch exactly as the
  lookup would have. Pinned by hm_lookup_case_fused (hit, miss,
  stored-nil binding, both branch orders, wildcard, and a guarded
  shape that must keep the general path). hm_lookup drops 32% on
  Lua 5.5 (44x → 28x speed-of-light); LuaJIT is unchanged (its traces
  already sank the allocation).

- **List-pipeline fusion.** `foldl' f z` over chains of `map`/`filter`
  stages ending in a range (`[a .. b]` at `Int`) or any list expression
  compiles to ONE loop with no intermediate lists — the deforestation
  the handwritten twin does by hand. It fires when the fold function is
  a named function provably strict in both parameters (its demand row);
  then everything the loop computes eagerly is exactly what the lazy
  pipeline forced, elements flow in the same order, and a filter guards
  only the stages outside it. Mono specializations resolve through a
  new `TFunction::spec_origin` (the mangled `foldl'_IntT…` carries its
  source name), so identity is never parsed out of a spelling. Pinned
  byte-exact against GHC by `list_pipeline_fused` (range and leaf
  sources, stage nesting on both sides of a filter, empty sources, seed
  and argument-order checks). list_pipeline: Lua 5.5 −83% (0.213s →
  0.036s, 162x → 27x speed-of-light), LuaJIT −97% (0.031s → 0.001s —
  2x, the handwritten twin's speed).

- **The boxed-Integer ops take small-magnitude fast paths.** When both
  operands fit two limbs (< 2^48), add/sub/mul/divmod compute natively
  and box the result, skipping the limb walks — divmod's bit-by-bit
  long division was the dominant cost of every small-value `mod`. The
  representation is unchanged (an Integer is the same always-boxed
  table; this is not the rejected type-erased small-int fast path), and
  every bound stays inside the 2^53 double-exact range with explicit
  rounding slack. A GHC-goldened boundary matrix
  (`integer_smallpath_bounds`) straddles the limb and exactness edges
  with both signs and pins byte-exact agreement with GMP. The
  integer_arith benchmark drops another 5.1x on Lua 5.5 (1.72s →
  0.34s; 2.10s → 0.34s for the round) and 4x under LuaJIT.

- **HashMaps are persistent diff+reroot tables.** A map value is now a
  handle onto one mutable Lua store per version family
  (Conchon–Filliâtre persistent hash tables): a write mutates the store
  in place and flips the old handle into a diff recording the previous
  value, so linear derivation — each new map built from the last, the
  shape accumulation folds produce — costs O(1) per write instead of
  copying the whole table, and reads on the newest version stay one raw
  index behind a root check. Reading an *old* version reroots (replays
  the diff chain, reversing it); a chain longer than the map is
  materialized into a fresh store instead, so ping-ponging reads between
  far-apart versions degrade to the old copy-per-op cost rather than
  thrashing. Observationally nothing changes — every version keeps
  answering as an independent value (pinned by `hashmap_versions` across
  read orders, forks, overwrites, stored-`Nothing`, structural keys, and
  the materialize path, and probed by the grown fuzzer below); `hmSize`
  is now O(1) from the handle's maintained count. Bench: hm_churn wall
  time drops ~13x on Lua 5.5 and ~40x under LuaJIT (803x/834x → 56x/44x
  speed-of-light), and hm_lookup *improves* to 23x/1.2x (from 28x/7.4x)
  — under LuaJIT the persistent map now reads at near-native-table
  speed.

- **The backend fuzzer holds map versions.** `HashMap` is a first-class
  type in the fuzz fragment: maps are let-bound, forked, derived
  through insert/delete chains, and read again *after* later versions
  exist — in both read orders, against the reference evaluator's
  independent map values — rather than only built and observed inline.
  The versioned vocabulary ran green against the old copy-on-write
  runtime first, so it was a trustworthy oracle for the representation
  change, and 2000 programs pass against the new one.

- **List fusion grew consumers, a stage, and fold-function forms.**
  `sum` (at Int/Number — native `+`, GHC's left-associated walk) and
  `length` (a pure count: map stages with no filter outside them are
  dropped, exactly the demand lazy `length` exerts, and elements are
  extracted unforced unless a filter's strict predicate demands them)
  now fuse like `foldl'`; a `take` stage fuses with its budget counter
  decremented at the stage's pipeline position and checked before the
  source advances — a spent budget stops the loop before the next cell
  is pulled or forced, exactly where laziness stopped, including the
  cell *after* the element that spends the last budget. The fold
  function may now be a first-class operator (`foldl' (+) 0 …` becomes a
  native in-place step, no call), a lambda or section (a conservative
  syntactic walk proves the body forces its parameters), or a partial
  application (the named row's tail covers the remaining parameters).
  Everything the loop consumes natively — range bounds, take budgets, a
  native step's initial accumulator — is delivered WHNF, and a native
  step forces elements produced by lambda map stages (their result can
  be a raw captured thunk). GHC-goldened across all of it
  (`list_fusion_growth`, 24 pinned lines including bottoms the loop must
  not touch).

- **The demand analyzer sees definitions as emitted.** Two growth items
  with effect beyond fusion: clauses with fewer patterns than their type
  has arrows are analyzed in their eta-padded form (the padded parameter
  applied to the body — codegen's N-ary emission), so point-free
  definitions (`odd = not . even`) finally carry real strictness rows;
  and `(f . g) x` propagates demand through the composition when both
  sides are provably strict unary values. `rem`/`quot` joined the
  strict-operator table (they were missing — `even`'s row was lazy).
  Rows feed the site-forced calling convention everywhere, so these
  widen that optimization too; bench rows and the tracker canary are
  unchanged (7.9x).

### Fixed

- **`show` of an Integer no longer leaks Lua's float subtype as a
  trailing `.0`.** Integer limbs may carry the float subtype on 5.3+
  hosts (carry propagation and machine-number decomposition divide with
  `/`), and `__int_tostring` printed its top decimal group with
  `tostring` — so a float-typed limb reaching a value with a single
  decimal group printed `6.0`. Reachable on the committed runtime
  (`(16777217 - 16777211) :: Integer` printed `6.0` via the
  small-magnitude subtraction fast path) and surfaced by the fuzzer the
  moment the new short division handed a big dividend's limbs to a
  small remainder. The top group is now formatted with `%d` like every
  other group (each group is < 10^7, exact on every host). Pinned by
  `cases/integer_show_subtype.mll` against the GHC golden.

- **The runtime generics have GHC's laziness in their function
  argument and heads.** Day-one behavior, surfaced while auditing the
  fusion gates: `map` applied `f` per cell at SPINE demand (heads were
  computed eagerly, one demand-level early) and `map`/`filter`/`zipWith`
  forced `f` itself once per call — so `length (map ⊥ xs)` crashed where
  GHC prints the length, and `filter (const False) (map (\_ -> ⊥) xs)`
  crashed where GHC folds nothing. Now `map`/`zipWith` build each head
  as a call-by-need suspension of `f x` (the closure-free `__mll_tk`
  carriers; applied once per demanded head, shared thereafter) and never
  force `f` themselves; `filter` forces its predicate only when an
  element exists to test; the type-erased `foldr`/`foldl` fallbacks
  force `f` only when there is a structure to fold. Demand rows and the
  strictness-contract probes updated to match (`map` is Lazy in `f`;
  `filter` is the mask's second deliberate under-claim). The fused
  pipelines absorb the cost where it would matter — every bench row and
  the tracker canary (7.9x) are unchanged, and fused `length` now drops
  undemanded map stages without evaluating their function expressions
  at all, exactly the lazy demand. Pinned GHC-goldened
  (`lazy_generics_parity`: nine shapes whose errors must stay
  untouched, including head-sharing under a double fold).

- **Fused pipelines no longer force what laziness would not have.** The
  round-1 list fusion gated only the FOLD function's strictness; a lazy
  map or filter function (`map (\_ -> 1)`, `filter (\_ -> False)`) left
  elements undemanded in the lazy pipeline that the fused loop computed
  eagerly — `foldl' step 0 (map (\_ -> 1) [error "boom"])` crashed where
  GHC prints the fold of ones (found by predicting the divergence, then
  confirming against runghc). Every stage function is now gated exactly
  like the fold's: a named row, an operator at native scalars, or a
  provably strict lambda — anything unproven declines to the general
  path, whose demand behavior is the reference.

- **A zero-arg `LuaPure` declaration now decodes its result.** A
  host-value read (`props :: LuaPure "system_properties" (HashMap
  String String)`) compiled to a bare global access with no boundary
  conversion, so a raw host table leaked into pure code as if it were
  the internal representation: a host array declared `[Int]` arrived
  as a Lua array instead of a cons list, a bare value declared
  `Maybe a` was never wrapped in `Just`, and a keyed table declared
  `HashMap` stopped working the moment the map representation changed.
  The constant read now runs through the same result decoder as every
  FFI call (scalars still pass through bare). Caught by the proprietary
  acceptance suite; pinned by `ffi_constant_values_decoded`.

- **A partial application no longer defeats the always-cheap parameter
  judgment.** The whole-program call-site analysis lets a function skip
  its entry `__force` when every visible call site passes a cheap
  (never-thunked) argument — but a partial application only covers its
  own spine positions, and the closure it builds forwards the remaining
  parameters raw. One full call with cheap arguments plus one partial
  application could therefore grant always-cheap on a position the
  partial application later fed a thunk, and the callee then inspected
  the thunk table as a value (`pb == "q"` was false for a thunk of
  `"q"` — a wrong answer, not a crash). Call sites now mark every
  position beyond their own spine as potentially thunked; the hidden
  extra positions behind `$`/`.` get the same closure. Pinned by
  `partial_app_uncovered_param` (GHC-goldened).

- **Nil-represented values (`Nothing`, `[]`, `()`) stored under scalar
  `HashMap` keys no longer vanish.** `t[k] = nil` is Lua's delete, so
  `hmFromList [(7, Nothing)]` had size 0 and lookups missed. The scalar
  path now boxes nil values behind a runtime sentinel on every write and
  unwraps on every read, on both FFI marshalling directions too.
  Structural (encoded) keys were unaffected. Found by the grown backend
  fuzzer on its first run.

## [0.1.7] - 2026-08-28

### Added

- **as-patterns.** `name@(Just y)`, `all@(x : rest)` — the full Haskell
  form, in every pattern position.

- **The remaining Haskell `newtype` declaration forms** — record syntax
  (with the brace allowed on the next line), and the same strictness and
  representation guarantees as the positional form.

- **Hexadecimal, octal and binary integer literals, and numeric
  underscores.** `0xFF`, `0o755`, `0b1010`, `1_000_000` — GHC's grammar.

- **A round of surface-syntax parity.** Multi-line import lists; operator
  names as import/`hiding` items; the infix definition form inside class
  and instance bodies (`a <+> b = …`); tuple-pattern bindings in `where`
  blocks; comma-separated pattern-guard qualifiers; implicit `do`/`case`
  blocks close on `,` `]` `}` and `then`/`else`/`of`/`in`/`where`, as
  GHC's layout rule closes them.

- **First-class `($)` and `(.)`.** The bare sections are real functions
  now — `($) (+) 1 2`, `foldr (.) id fs` — with `($)` forwarding the flat
  call protocol and `(.)` building the composed closure; `(&&)`/`(||)`
  as sections keep their short-circuit behavior.

- **`deriving (Generic)` gives `as` renames a meaning.** The derived
  metadata reflects a field's or constructor's *effective* external name
  (`selName`/`conName` return the `as "…"` rename when present), so a
  type deriving only `Generic` may carry renames — the hook for writing
  generic codecs against wire names instead of source names.

- **`deriving (Bounded)`** for enums and single-constructor products,
  plus `Bounded Int`/`Bool`; `Either` derives `Eq` and `Ord`; `Ord Bool`.

- **GADT-syntax constructors work with the derive machinery.** Vanilla
  constructors declared in GADT syntax are accepted by the stock derives
  and by `Generic`/`ToJSON`/`FromJSON`/`LuaDict`; genuinely existential
  or index-refined heads are rejected with the reason.

- **A compiler warnings channel.** `CompileResult` now carries warnings
  (the CLI and the wasm playground print them). First residents: a
  literal pattern match without a catch-all warns with a witness
  (`(not one of 1, 2)`) — on by default, where GHC needs
  `-Wincomplete-patterns`, and the message says so — and a data
  constructor colliding with an import alias warns at the import.

- **The REPL runs IO actions.** An expression of type `IO a` executes
  (declarations and pure values keep printing); the REPL embeds Lua 5.4
  matching the `mll` runner, truncates long output at a character
  boundary, and survives closed stdout and non-UTF-8 stdin.

- **An embedding surface for the `mllc` crate.** `with_compiler_stack`
  (the calibrated-stack prerequisite) and `GIT_COMMIT` are exported, and
  the crate-level docs state the embedding contract and the
  whole-program compilation model.

### Changed

- **`max` and `min` are `Ord` class methods**, overridable per instance,
  as in GHC.

- **Exhaustiveness checking is a Maranget-style usefulness matrix.** All
  columns at once, tuple components, nested constructor arguments, `Bool`
  as a two-constructor domain, and real witnesses (`Just False`,
  `(False, G)`) in the error. Deliberately permissive where coverage is
  undecidable (guarded rows count as covering; non-Bool literals match
  anything for the hard error — the new warning handles the literal
  residue).

- **`head`/`tail` on `[]` carry GHC's messages** —
  `Prelude.head: empty list`, verbatim, with no position prefix.

- **Nothing of the runtime reaches `_G`.** The callback wrappers became
  locals; embedding hosts see a clean global table.

- **Optimizer additions.** Loop-invariant closure hoisting (pass 7);
  cross-binding constant propagation with literal beta-reduction and
  compile-time `show` of `Int`/`Bool` literals; partial applications
  share their thunked captures; iterator lists allocate nothing per
  single-value step; the prelude chunk index is built once per process.

- **Sharper rejections and diagnostics.** Import cycles are reported as
  the actual chain; duplicate type-name declarations, a class method
  declared by two classes, a record field declared by two types, and
  partially applied type aliases are rejected instead of misbehaving
  later; a character literal gets the no-`Char` explanation; a
  missing-context diagnostic names the source variable; an unterminated
  `{-` is an error; every `note:` is a structured note.

### Fixed

- **Multi-clause functions returning functions no longer drop or ignore
  arguments.** The N-ary calling convention declares eta-padding
  parameters for a function whose type has more arrows than its clauses
  bind — but a multi-clause or guarded function never *consumed* the
  padding (`pick True h = h` at four arrows returned `h` unapplied), and
  `where`/`let`-local functions were never padded at all (extra
  arguments in a saturated call were silently discarded). Clause results
  now apply the padding they declared, and local functions get the same
  padding as top-level ones.

- **The dictionary-passing fallback (past the 16-specialisation cap) had
  a cluster of wrong-code and wrong-rejection paths, all closed.** A
  local binder that shadowed a dict-passing global was rewritten as if
  it were the global (Lua "arithmetic on a table value", or a cascade of
  30+ spurious errors); `/=` dispatched through the wrong method;
  list/`Maybe`/tuple `Eq` needed structural dictionaries that were never
  synthesized; a class variable only in result position lost its
  dictionary at some arities; nullary class methods died in the
  dictform; under-applied uses weren't saturated. And a first-class-dict
  error in *dead* generic code no longer rejects the program — the
  diagnosis defers to DCE reachability.

- **An action-typed value binding re-performs.** `let step = putStrLn "x"`
  used twice runs twice — the binding is re-performable, not a memoized
  first run.

- **A round of laziness repairs.** Record-update fields and the
  scrutinee of an irrefutable-first `case` bind lazily; `$` applications
  are no longer treated as cheap to force; a `where`-local of one value
  binding no longer leaks into later bindings; a CAF whose body can trap
  (division, partial match) is thunked instead of evaluated at module
  load; demand analysis respects shadowing (a rebound name no longer
  inherits the outer binding's strictness).

- **Local binders shadow every rewrite.** A local named like a top-level
  function, a runtime special name, or a fast-path callee is just a
  local, in every compiler pass; user `_`/`__`-prefixed names live in a
  namespace disjoint from compiler temporaries.

- **Prelude/runtime GHC parity.** `sortBy` is a stable merge sort;
  `foldl'` seqs the accumulator it passes; `drop` returns the list for
  `n <= 0`; `(!!)` raises on a negative index; `read` validates its
  input; `abs`/`signum` match GHC's signed-zero and NaN edges;
  ByteString accessors are bounds-checked and `bsXor` truncates to the
  shorter operand; `quot`/`rem` get GHC's `infixl 7` fixity; `Integer`
  comparisons coerce plain-number operands.

- **Library fixes.** `Data.Map` `intersection`/`difference` probe the
  second map per entry instead of scanning a keys list (linear, not
  quadratic); `Regex` rejects dangling quantifiers and unknown
  alphanumeric escapes, and a negated character class matches newline;
  `LMath` gains a portable `frexp` and `logBase` takes GHC's argument
  order; `Control.Monad`'s `void`/`join` are `Monad`-polymorphic.

- **Module-system fixes.** Diamond imports merge each module's
  declarations once (no more spurious "Duplicate instance" from a shared
  ancestor); repeated imports of one module merge like GHC's; qualified
  import prefixes reach instance heads and bodies, class defaults, and
  backtick operators.

- **Parser fixes.** A `-` after a record-update brace is subtraction; a
  bare negative literal is not a pattern atom; an operator continuation
  must out-indent its block column; dash runs followed by a symbol
  character lex as operators; block-comment-only lines are whitespace to
  layout; an empty `where` no longer swallows the declarations after it;
  a `do` block must end in an expression statement, reported at the
  offending line.

- **Typechecker fixes.** Composed self-referential substitutions can no
  longer hang inference; GADT exhaustiveness demands a *reachable*
  constructor, not merely a unifiable one, and universals the header
  doesn't name are quantified; operator sections and unary minus emit
  their class constraints; ascription type variables are rigid; every
  equation of a function must bind the same number of arguments (GHC's
  rule); `LuaIO` tuple results are multi-return again.

- **Linear types:** `(!!)` is not a consume-once operator, and a later
  guard's condition is a skippable path — both no longer reject valid
  programs.

- **A tail call from one IO function to another runs in constant
  stack.** An IO function that performs at call time and ends by
  calling a *different* such function (`ping n = … pong (n - 1)`,
  `main = … putStrLn s`, sed's line loop through its helper) forwarded
  the callee through the runner's argument position — one pinned Lua
  frame per crossing, a stack overflow past ~250 000 mutual-recursion
  levels. Only a call to the function *itself* had been emitted as a
  bare Lua tail call. The compiler now classifies every such
  "direct-perform" function module-wide before emitting any body, so a
  saturated tail call to any of them — defined earlier or later in the
  file — is a bare `return callee(...)`, which Lua's tail-call
  elimination reclaims. Same effects, same forcing (the forwarding
  runner was the identity on these results); 2e6-deep `ping`↔`pong`
  now completes on PUC Lua and LuaJIT.

- **A thunked top-level binding referenced before its own definition is
  now forced.** The module layout forward-declares every top-level name
  and seeded them all as "concrete" (safe to read without `__force`);
  a value binding emitted as a thunk only cleared that claim at its own
  emission, so any reference in an earlier-emitted body read the raw
  thunk table. In a strict position that is silently wrong on every
  host — an `if` on a False Bool CAF defined after its user took the
  True branch (a thunk table is truthy). The seeding now predicts each
  slot's shape with the same rules the emission uses (kept in sync by a
  debug assertion), so thunked slots keep their force everywhere. Found
  on LuaJIT, where it made `hasIntegerSubtype` — False there, defined
  below its users — read as True inside the JSON module, mis-routing
  the new `Integer` codecs.

- **An `Integer`-typed literal between 2^53 and 2^63 is exact on
  doubles-only hosts.** Such a literal was emitted as a bare Lua
  numeral and read back by the host, so LuaJIT rounded it before the
  bignum ever saw it (`9223372036854775807 :: Integer` became 2^63).
  A literal bound for a real `fromInteger` now routes through the
  exact `__mll_biglit` decimal pool past 2^53, on every host; machine
  `Int`/`Number` literals keep their documented host representation.

### Removed

- **The `performloop` optimization pass, and its `MLL_OPT_DISABLE`
  name.** It looped the older `return __mll_run_tail(self(...))`
  self-recursion shape, which the emitter stopped producing when
  self tails became bare (0.1.6): two corpus sweeps with the pass
  disabled were byte-identical to the enabled output. Its shape is
  now a plain Lua tail call that the `tailloop` pass loops.
  `MLL_OPT_DISABLE=performloop` is an unknown-pass warning from here
  on; the known names are `parens`, `dead`, `iife`, `force`,
  `tailloop`, `ioloop`.

## [0.1.6] - 2026-08-05

### Changed

- **Orphan instances are rejected only in the main module.** An
  imported module — the stdlib and user libraries — may declare
  instances for classes and types it does not define (the `JSON` module
  now carries `ToJSON`/`FromJSON` instances for `Int`, `[a]`, …).
  mata-ll compiles the whole program together, so there is no
  cross-build incoherence for the rule to guard against in library
  code; the check still catches a rogue instance in the program's own
  module.

- **Direct-perform IO recursion runs in constant stack and stops forcing
  `pure` payloads on the way out — with no loop pass needed.** A
  single-clause IO function's body IS its action ("direct-perform"), but
  every action terminal used to funnel into
  `return __mll_run_tail(...)`: a recursive call sat in the runner's
  ARGUMENT position (one pinned Lua frame per level, stack overflow near
  1e6 depth on shapes the loop passes declined), and each unwinding
  frame re-applied the runner, whose `__force` evaluated a thunk `pure`
  payload GHC never forces (`r <- f 0 undefined` raised where GHC binds
  the bottom unforced). Two emission changes close both at the source:
  `case` action terminals now flatten to statements exactly like `if`
  (guards included), so each branch's `pure e` takes the pure-box
  convention; and a saturated tail call to the function itself emits
  bare — `return self(...)`, the exact form Lua's tail-call elimination
  reclaims the frame for. A 2e6-deep run with every loop pass disabled
  completes in constant stack on PUC Lua and LuaJIT (pinned). The
  tailloop pass now claims these bare self-tails (same loops as before),
  and the performloop pass converts nothing on new output — it stays as
  a backstop. Non-exhaustive `case` expressions in flattened positions
  now raise GHC's `Non-exhaustive patterns` error instead of silently
  yielding `nil`.

- **A first-class `pure e` protects an unsafe payload with the pure box.**
  The higher-order emission (`fmap f (return e)`, an inlined
  `id (pure undefined)`) built `function() return <payload> end` with the
  payload always bare; when that closure reached a forwarding runner, the
  raw payload thunk escaped to a consumer that forced it — raising where
  GHC binds the bottom unforced (pinned: first_class_pure_bottom). The
  payload now takes the same escape decision as an escaping `pure`
  terminal: provably-safe values stay bare, anything a runner could
  wrongly force or call crosses in a `__mll_pure` box stripped by exactly
  one consuming unbox.

- **`undefined` is no longer treated as an already-evaluated value.** The
  module-level "these prelude names are plain functions, never thunks"
  concreteness seed wrongly listed `undefined` — which the runtime binds
  to a THUNK that raises when forced. The claim let `pure undefined`
  escape bare (the consumer's runner then forced it, raising where GHC
  9.14.1 prints the program's real output; pinned: case_pure_bottom,
  if_pure_bottom) and let bindings read it force-free. `undefined` now
  gets the ordinary lazy treatment: referenced freely, raising only when
  demanded.

- **`mllc::CompileOptions` gains `disable_opt_passes`** — the
  `MLL_OPT_DISABLE` pass-skip list as a per-compile option (the
  environment variable still works and is overridden when the option is
  set). Lets embedders and the test suite pin unoptimized emission
  without mutating process-global state.

- **Eta-expanded point-free definitions shed a runtime `__force` call —
  and usually the closure behind it.** A definition like `f = foldr g z`
  (fewer clause parameters than the type has arrows) compiled to
  `return __force(<closure>)(_eta0)`: the `__force` was a no-op on the
  already-evaluated closure, kept only because its call parentheses
  provided the grouping Lua's grammar demands before `(args)`. The
  emission now writes the grouping parens directly, and the existing
  immediate-call collapse then inlines the application — the body
  becomes `local _pa0 = _eta0; return foldr(g, z, _pa0)`, dropping one
  runtime call plus one closure allocation per invocation of each
  eta-expanded function. Behavior is byte-identical on every affected
  program; 24 of 428 corpus files change, all in exactly this shape.

- **Self-recursive IO functions compile to loops.** The IO emission
  builds an action closure per recursive step (`countdown n = do …;
  countdown (n - 1)` allocated a fresh closure, two calls and a runner
  dispatch each iteration); such functions now run their self-loop
  inside one closure as a `while` loop. Build-time semantics are
  unchanged — the pattern-match dispatch still runs when the action is
  built, so `seq (f undefined) ()` raises exactly as GHC does (pinned
  against real GHC) — as are effect order, laziness and sharing (fresh
  per-iteration locals for captures). The tracker benchmark's full-song
  decode drops from 107 s to 76 s under LuaJIT with byte-identical
  output; a plain IO counting loop runs ~1.5× faster. Functions where
  safety cannot be proven keep the call form; disable with
  `MLL_OPT_DISABLE=ioloop` for diagnostics.

- **Self-tail-recursive functions compile to loops.** A function whose
  recursive calls are self tail calls (`go (n - 1) b (a + b)` in result
  position) now emits a `while true` loop with one simultaneous
  parameter update per tail call instead of a recursive call — Lua's
  proper tail calls already kept the stack flat, but loops trace where
  tail recursion does not: a recursion-heavy microbenchmark runs 3.4×
  faster under LuaJIT (~4–5% under PUC Lua). Closures and thunks created
  in the body capture fresh per-iteration locals, so sharing and
  laziness behave exactly as under recursion. Functions where the
  conversion cannot be proven safe (varargs, rebound names, FFI text
  mentioning a parameter) keep the call form; disable with
  `MLL_OPT_DISABLE=tailloop` for diagnostics.

- **Generated Lua sheds provably redundant `__force` calls.** The code
  generator now carries operational annotations (value shape and effect
  facts) on the emitted Lua tree, and a peephole collapses `__force(e)`
  to `e` wherever `e` is proven already-evaluated — forces of literals
  substituted by the inliner, of aliases of already-forced locals, of
  fresh table constructors, and of cons cells in discard position.
  Behavior is identical; the redundant runtime calls simply disappear.
  The annotation engine (codegen/annot.rs) holds an annotation-write
  monopoly — passes request rewrites and justify result stamps — and a
  stamp-refutation check re-derives every annotation from scratch in
  test builds. Optimization passes are individually disableable via
  `MLL_OPT_DISABLE=parens,dead,iife,force` (diagnostic use only;
  defaults unchanged).

- **Breaking: an exported result of list type now hands a Lua host an empty
  table for the empty list, not `nil`.** mata-ll represents `[]` as `nil`
  internally, and the export edge used to let that representation leak: a
  top-level `[a]` result (or `[a]` value export) crossed as `nil` — even
  though the same empty list crossed as `{}` everywhere else (FFI call
  arguments at every nesting depth, callback results, and even one level
  deeper in the export's own result, e.g. a `Just []`). The declared export
  type has distinguished `[]` from `Nothing` since results became
  type-directed; the boundary now uses it: `[]` is a fresh `{}` a host can
  `ipairs` without a nil check, and `Nothing` stays `nil`. A host that
  nil-checked a list result to detect emptiness must switch to `#t == 0`
  (`next(t) == nil`).

- **Breaking: only types with a DESIGNED FFI shape may cross the boundary, in
  both directions.** The marshallability check previously accepted "any data
  type iff every field marshals", so a plain user ADT — and prelude `Either`
  (outside `LuaTry`/`LuaIOCatch`), `Ordering`, and `ExitValue` — crossed as
  mata-ll's internal `{tag, fields…}` table, a shape with no meaning to a Lua
  host. Such a type is now REJECTED at compile time even when its fields would
  each marshal. The allowed set is the designed shapes: scalars, `()`,
  `LuaUserData`, `[a]`, tuples, `HashMap`, `Maybe a`, `Any`, a `LuaDict` record,
  a NEWTYPE over a marshallable type (transparent — the value IS its field, so
  `newtype FileHandle = FileHandle LuaUserData` and the whole `LIO` file API
  keep crossing), and `Either String a` as a `LuaTry`/`LuaIOCatch` result (the
  `pcall` wrapper builds its tags). The check is now also SYMMETRIC: FFI imports
  (`LuaPure`/`LuaIO`/`LuaTry`/`LuaIOCatch`, which call into Lua) are validated
  like exports — arguments cross out, the result comes in, outgoing callbacks
  are checked with directions swapped, and the polymorphic threaded-state fold
  variable stays allowed. The error names the culprit sub-type, the position and
  the direction, with a `note:` explaining the tagged-table leak and pointing at
  a `LuaDict` record / `Any` / a scalar-or-list encoding. To carry a plain ADT
  across, wrap its data in a `LuaDict` record or a newtype.
- **`[a]`-vs-`String` type errors now explain the opaque-String design.**
  mata-ll's `String` is deliberately opaque — it IS the Lua string, a byte
  array, not `[Char]` — so list operations (`++`, `map`, `length`,
  `intercalate`) do not apply to it. The mismatch error for that shape (both
  directions) previously stated the two types and stopped; it now says why:
  that `String` is not a list by design, that `<>` concatenates `String`s and a
  list of `String`s folds with `<>`/`mconcat`, and it points at HASKDIFF.md
  ("Strings and ByteStrings") for the rationale. Error-path only: nothing that
  typechecked before changes, and the note fires specifically for the
  `[a]`-vs-`String` shape.
- **All published and internal Rust crates forbid `unsafe` code.** `mllc`,
  `mll`, `mll-tests`, and `mll-repl` now set `unsafe_code = "forbid"` — the
  crates contained no unsafe code, and `forbid` (not `deny`) locks that in as
  a verifiable guarantee that a stray `#[allow]` cannot reopen.
- **Breaking: sections enforce GHC's operand-precedence rule (Haskell 2010
  §3.5).** A section operand that is itself an infix expression must bind
  tighter than the section operator; previously mata-ll accepted `(== a || b)`
  and read it as `(== (a || b))` — a grouping GHC rejects outright, because
  the section's expansion `x == a || b` groups as `(x == a) || b`. Both
  directions now reject exactly as GHC does — `(== a || b)`, `(a + b *)`,
  `(+ a + b)`, `(a ++ b ++)`, `(-a *)`, and the backtick forms — with declared
  fixities participating and prefix minus counting as an infixl 6 operand.
  The error states the rule, both fixities, the meaning the section cannot
  have, and how the unparenthesized expansion groups, with a note showing the
  parenthesized operand that keeps the intended meaning. The legal
  same-precedence chains work — and left sections with compound operands,
  which previously failed to parse at all (`(2 * 3 +)` died with `Expected
  expression, found RightParen`), now parse: an infixl operand chains in a
  left section (`(2 + 3 +)`), an infixr operand in a right section
  (`(++ a ++ b)`), and a negated operand joins an infixl 6 left section
  (`(-1 +)`, and `(-1 <>)`-style forms that were previously rejected).
  Verified accept/reject-identical against GHC 9.14.1 on every shape;
  corpus-checked — no existing program changes acceptance. Covered by
  `section_operand_precedence_matches_ghc` plus GHC-goldened cases in
  `operator_sections.mll` and `operator_fixity.mll`.

### Added

- **`deriving (Generic)` and `Data.Generics` — datatype-generic
  programming.** A concrete type deriving `Generic` gets a structural
  sum-of-products representation: the compiler extends the closed `Rep`
  type family with one equation per derive, synthesises the
  `from`/`to` conversions, and reflects datatype/constructor/field
  metadata (`datatypeName`, `datatypeConCount`, `conName`, `conArity`,
  `conIsRecord`, `selName` — effective external names, `as` renames
  applied). A generic function is an ordinary typeclass over the
  representation combinators (`U1`, `K1`, `:+:`, `:*:`, `D1`/`C1`/`S1`),
  written once and resolved by monomorphization at each concrete type.
  Deviations from `GHC.Generics` are surface-level and documented in
  HASKDIFF (module name, three distinct metadata wrappers instead of a
  tagged `M1`, combinators of kind `Type`, no `V1`). For generic
  producers, `Data.Generics` also exports `gProxy` and per-layer proxy
  re-typers.

- **`genericToJSON` and `genericFromJSON` in the `JSON` module.** A
  generic encoder and decoder over the `Generic` representation whose
  wire format AND error messages agree byte-for-byte with the derived
  native codecs — usable on any `deriving (Generic)` type, and the
  substrate's proof of fidelity (pinned by integration tests). The
  module now also carries `ToJSON`/`FromJSON` instances for the
  primitive leaf types (`Int`, `Number`, `String`, `Bool`, `Json`,
  `[a]`, `Maybe a`), and the `FromJSON` class gained a defaulted
  `fromJSONField` method (the `Maybe` instance overrides it, so
  missing-key/null optionality is a property of the field lookup).

- **Infix type operators.** `:`-leading symbolic names are type
  constructors, in declarations (`data (:+:) a b = L1 a | R1 b`) and in
  use (`f :+: g`), grouping by their declared fixity — the notation the
  generic representation is written in.

- **Breaking: arbitrary-precision `Integer`, and it is the numeric
  default.** 0.1.5 removed the misnamed wrapping `Integer`, leaving
  `Int` (64-bit, wrapping) as the only integer type; this release
  restores GHC's model with a real bignum. Unannotated numeric literals
  default to `Integer` — defaulting is `(Integer, Number)`, matching
  GHC's `(Integer, Double)` — and `Int` stays the explicit machine-word
  type. `fromInteger` takes an `Integer` (was `Int`), and `toInteger`
  returns to `Integral`. The runtime implementation is portable
  base-2^24 limbs — exact on both Lua 5.3+ (native integers) and LuaJIT
  (doubles) — with the full `Num`/`Real`/`Integral`/`Enum`/`Eq`/`Ord`/
  `Show`/`Read` instance set and GHC's `divMod`/`quotRem` sign rules.
  Literals past `maxBound :: Int` work in expressions and in patterns;
  each distinct big literal is pooled into a module-level table and
  parsed once at load, not at every use. Byte-identical to GHC on the
  oracle cases. Breaking because defaulted arithmetic that previously
  wrapped at 64 bits now computes the exact result.

- **`ToJSON`/`FromJSON` cover `Integer` — exactly, at any magnitude.**
  Encoding emits the bare decimal digits aeson emits; decoding is exact
  from integer-syntax JSON of any length. `Json` gains a `JInt Integer`
  constructor carrying integer syntax beyond the host number's exact
  window (the full 64-bit range with Lua's integer subtype, 2^53 on a
  doubles-only host), so parseJSON/encodeJSON never lose digits; inside
  the window numbers stay ordinary `JNum`s, so encode/parse round-trips
  a `Json` value unchanged. The native derives, the generic codecs'
  leaves, and the new `toJSONInteger`/`fromJSONInteger` combinators all
  agree byte-for-byte; a `jInteger` accessor reads the exact value.
  Float/exponent syntax is held as a host double and decodes to
  `Integer` only within the 64-bit range — aeson, which parses every
  number exactly, also accepts e.g. `1e30`; spell big integers in
  digits.

- **The `(^)` exponentiation operator.** `(^) :: Num a => a -> Int -> a`,
  `infixr 8`, exponentiation by squaring over `Num` multiplication — so
  it is exact at `Integer` (`2 ^ 100` matches GHC byte-for-byte), and a
  negative exponent is an error. Lua's own `^` is float power and is
  deliberately not used.

- **`pOpen` in `LIO`.** `pOpen :: String -> String ->
  LuaTry "io.popen" (Either String FileHandle)` — run a command and read
  from or write to it through the existing `FileHandle` methods.

- **`schema2mll`, a JSON Schema → mata-ll type generator.** A standalone
  utility written in pure mata-ll (`utilities/schema2mll.mll`): it reads a
  JSON Schema on stdin and emits a data type deriving `FromJSON`/`ToJSON` on
  stdout. Objects map to records, non-required fields to `Maybe`, arrays to
  lists, nested objects to their own named records, `$ref`/`definitions`/
  `$defs` to named types, and free-form objects to the `Json` passthrough.
  Field labels are the JSON keys verbatim, so the derived codecs round-trip.
  The reverse direction (schema from type) is deferred to the planned Generics
  substrate rather than added as a one-off native derive.
- **Full GHC `read`-side string-escape parity — the last input-syntax gap.**
  The lexer accepted only `\n \t \r \\ \" \0`; string literals now decode the
  whole Haskell 2010 §2.6 escape grammar: the shorthand control escapes `\a`
  `\b` `\f` `\v` alongside the existing ones; decimal, octal (`\o37`), and hex
  (`\xff`) numeric escapes with maximal munch; the full named-control table
  `\NUL`..`\US` plus `\SP` and `\DEL`, longest-match so `\SOH` wins over `\SO`
  followed by `H`; the `\&` zero-width separator (`"\137\&0"` is two bytes,
  `"\SO\&H"` disambiguates the name); and the `\<whitespace>\` string gap,
  newlines included. The decoder's control-name table is kept byte-for-byte
  identical to the runtime's show-side table, so `read . show == id` holds
  through both halves. This also corrects a silent misdecode: `\0` was not
  maximal-munch, so GHC source `"\05"` (one character, code 5) decoded to the
  two characters `['\0','5']` — a wrong value, not a rejection. One deliberate,
  documented deviation, forced by mata-ll's byte-string model (`String` is the
  Lua string, a byte array — HASKDIFF.md, "Strings and ByteStrings"): a numeric
  escape above 255, which GHC accepts up to `\1114111` as a Unicode code point,
  has no single-byte representation and is a hard lexer error carrying a
  `note:` explaining why — never a silent wrong value. Generated Lua stays
  byte-identical for all existing programs. Covered by `string_escapes.mll`
  (every new escape, maximal munch, `\&`, string gaps, and the `read . show`
  round-trip, asserted against the Report's byte values) and the out-of-range
  rejection test.
- **Scientific-notation numeric literals.** `1.0e-2`, `1e5`, `2.5E+3`, and
  `6.022e23` now lex as float literals (Haskell 2010 §2.5: `(e|E) [+|-]
  decimal`, lowercase or uppercase `e`, optional sign, ≥1 digit). As in GHC a
  bare-mantissa exponent like `1e5` is `Fractional` — a float, not an `Int` —
  and types through the existing `NumLit` path (defaulting to `Number`).
  Maximal munch requires an exponent digit, so `1e` still lexes as `1` then the
  identifier `e`, and `1..3` stays a range. Previously `1.0e-2` lexed as the
  application `1.0 e - 2` (`Unbound variable: e`); the asymmetry that `show`
  emitted exponent notation (`1.2345678e7`) the lexer could not read back is
  closed — `read . show` now round-trips on such values. Covered in
  `num_polymorphic.mll`, pinned against GHC by the differential oracle.
- **`let` qualifiers in list comprehensions.** `[ y | x <- xs, let y = f x,
  p y ]` now parses: the `let` binds are visible in the comprehension body and
  every later qualifier, desugaring to `let binds in <rest>` exactly as GHC
  specifies. Bindings use the full let-binding grammar — simple, function
  (`let g a = ...`), and tuple-pattern binds, multiple layout-separated
  bindings in one `let`, and mutual recursion — because the qualifier shares the
  same `parse_let_binds` routine as a `let`-expression (extracted so the two
  cannot drift). Multi-line qualifiers work too (the bar/comma/`]` layout change
  below). Previously any `let` qualifier failed, parsed as a guard expression
  that then demanded `in` (`Expected In, found ...`). Covered by single-line,
  multi-line, chained, and multiple-binding cases in `list_comprehensions.mll`,
  pinned against GHC by the differential oracle.
- **List brackets are layout-insensitive: comprehensions, list literals, and
  ranges may span multiple lines.** A comprehension bar, a range `..`, a
  separating comma, or the closing `]` can now sit on a continuation line, so
  the GHC-idiomatic multi-line form parses:
  `[ (x, y)` / `| x <- xs` / `, y <- ys` / `, x < y` / `]`. Previously any
  newline before the `|` (or before a qualifier's comma, or the closing `]`)
  aborted with `Expected RightBracket, found Pipe`, forcing the whole
  comprehension onto one line. Inside `[ ]` newlines and indentation are now
  uniformly insignificant, matching how the parser already treated commas in a
  multi-line list literal. Accept-only change: nothing that parsed before parses
  differently. Covered by multi-line cases in `list_comprehensions.mll`, pinned
  against GHC by the differential oracle.
- **Parenthesized `( )` expressions are layout-insensitive too.** The closing
  `)`, a tuple comma, or a `::` ascription may sit on a continuation line, so
  `( a` / `+ b` / `)`, a multi-line tuple, and `( e` / `:: T )` all parse. The
  interior was already layout-free after the `(`; `continue_infix` stops at the
  newline, so the close-side decisions (`)`, comma, `::`) now skip newlines and
  indentation the same way. Previously a newline before the close aborted with
  `Expected RightParen`. Accept-only change; covered by multi-line cases in
  `tuples.mll`, pinned against GHC by the differential oracle.
- **`Any` converts to and from plain Lua scalars at the FFI boundary.** The
  dynamic `Any` type (`AnyString`/`AnyInt`/`AnyNumber`/`AnyBool`/`AnyNull`)
  no longer crosses the boundary as its raw constructor table. A host scalar
  coming IN is tagged by its Lua type — a string becomes `AnyString`, an
  integer-valued number `AnyInt`, a fractional number `AnyNumber`, a boolean
  `AnyBool`, and `nil` `AnyNull`; an `Any` going OUT is untagged to its bare
  scalar (`AnyNull` becomes `nil`), so the host only ever sees a plain
  string/number/boolean/nil. The conversion descends into containers like every
  other marshalled type, so `[Any]`, `(Int, Any)`, and a record/`HashMap` with
  an `Any` field round-trip through the host unchanged. A value that is neither a
  scalar nor `nil` (a table, function, or userdata) cannot cross as `Any` and
  fails at the boundary with a localized error, since `Any` models only scalar
  Lua values.

### Fixed

- **Past the 16-specialisation guard, polymorphic functions and
  instance methods now switch to WORKING dictionary passing.** The
  guard against runaway specialisation (polymorphic recursion) used to
  leave an instance method past the cap pointing at its raw polymorphic
  original — whose unresolved method references were `nil` at runtime —
  so a generic function applied to more than ~16 types died with an
  opaque nil call. Now an instance method past the guard is purged to
  dictionary passing like a top-level function; dictionaries for
  parameterized instances are composed recursively from the instance's
  context; the rewrite runs to a fixpoint (sibling constrained calls
  pass dictionaries and mark their callees; bodies are re-monomorphized
  from their pristine polymorphic copies so no one type's specialisation
  gets welded into a shared body); and dead-code elimination parses the
  dictionary strings format-structurally, so a live function whose
  mangled name contains `:` (a type-operator instance) is no longer
  dropped. Correct at any number of types; the guard remains a
  performance boundary only.

- **Dictionary-passing call sites pass value arguments lazily.** They
  were emitted eagerly, which evaluated a possibly-bottom argument the
  callee never demands — `g (f x)` with `f _ = error …` and `g`
  ignoring its argument raised where GHC returns. Value arguments now
  take the same lazy call protocol as ordinary calls.

- **`fromInteger` to `Int` is exact through the full 64-bit range.** The
  machine-value reconstruction accumulated in float arithmetic (bignum
  limbs can carry Lua's float subtype: carry propagation divides with
  `/`, float division), so `fromInteger (toInteger (maxBound :: Int))`
  rounded to 2^63 — off by one. It now re-anchors each limb as an
  integer and accumulates in integer arithmetic: exact through int64,
  and past 2^64 it wraps exactly like GHC's `Integer`-to-`Int`
  narrowing. Conversion to `Number` keeps float accumulation, so a huge
  `Integer` approximates to the nearest double, as GHC's `fromInteger`
  to `Double` does.

- **A function body can no longer narrow its signature's type
  variables.** `f :: a -> Int; f x = x` and
  `g :: Monad m => m (); g = putStrLn "hi"` both compiled: signature
  variables were freshened to flexible unification variables, so a
  clause body could silently pin one to a concrete type — the first
  example handed runtime a `String` typed as `Int` (a Lua "add string to
  number" crash waiting to happen). GHC rejects both, and mata-ll now
  does too: signature variables are rigid (skolemized) while the body is
  checked, with the signature's declared context as the usable
  instances. GADT index refinement still narrows them legitimately, and
  the rigid-mismatch error names the signature the variable came from.

- **A higher-rank argument must really be polymorphic.**
  `apply2 :: (forall a. a -> a) -> …` applied to `\x -> x + 1` compiled,
  and `apply2` using its argument at both `Int` and `Bool` ran
  `True + 1` at runtime. The `Num` constraint on the sealed argument was
  deferred to a caller that cannot exist; it is now reported as
  unsatisfiable, with a note explaining the argument must work for every
  type.

- **Four inference bugs that `Int`'s eager evaluation had masked** —
  found while making `Integer` the default: record-update values unify
  against the field's type instead of defaulting alone; a shadowed
  `do`-`let` binding gets a fresh local, so a lazy thunk no longer
  observes the rebinding; prefix minus and operator-section values
  dispatch at the operand's type; `where` bindings are inferred against
  the running substitution.

- **Self-recursive IO functions that perform as they run no longer
  overflow the Lua stack.** An IO function without pattern dispatch runs
  its effects when called and recurses through the forwarding runner
  (`loop n = do { …; loop (n - 1) }` behind an `if` or `case`); the self
  call sat in the runner's argument position — not a Lua tail call — so
  every step pinned one interpreter frame and the program crashed with
  `stack overflow` at roughly a million iterations on PUC Lua and LuaJIT
  both. sed-style line loops, REPLs and game loops are exactly this
  shape. Such functions now compile to a `while` loop (one simultaneous
  parameter update per recursive step, fresh per-iteration locals for
  captures) and run in constant stack at any depth; effect order, effect
  count and laziness are unchanged — each loop iteration runs exactly
  what one call ran. The conversion also restores a piece of GHC parity
  the recursive shape broke: the runner was re-applied once per pinned
  frame on the way out, and the re-application forced a `pure` result's
  thunk, so `r <- f 1 undefined` raised where GHC binds the bottom
  unforced (pinned against real GHC). Functions where the conversion
  cannot be proven safe keep the call form — and, with it, the old
  depth limit; disable with `MLL_OPT_DISABLE=performloop` for
  diagnostics.

- **Every compiler front-end now runs `compile` on the calibrated 2 GiB
  stack.** The compiler's nesting-depth limit is calibrated against
  `mllc::COMPILER_STACK_SIZE` — every caller of `mllc::compile` must
  provide a thread of that size, and the depth guard then turns any
  too-deep input into a clean diagnostic instead of a crash. The REPL's
  `:lua` command and its startup baseline compiled on the calling thread
  instead, so a deeply nested `:lua` input could abort the whole REPL
  session where `mll` itself compiles the same program fine; both now go
  through a calibrated helper. The test harness had the same gap in ~180
  direct `mllc::compile` call sites (the CI debug build overflowed on an
  8-line example whose inference frames grew with the do-block indexing
  work above); every harness compile is now routed through one wrapper on
  the calibrated stack. The stack-size documentation carries fresh
  measurements: a debug build burns ~70 KB of stack per inference level,
  and the deepest expression spine the limit admits compiles with at
  least a 4× margin.

- **Compiling a long do-block now scales linearly in its length, not
  quadratically.** Found by the parser fuzzer's deep probes: every do-`let`
  statement re-walked state proportional to everything before it — the
  whole type environment (each generalization re-collected every scheme's
  free variables; each substitution step re-cloned every binding), the
  accumulated substitution (each composition re-walked the whole map, and
  the unifier moved a chained type's representative every statement,
  re-pointing every image), and the remaining statements (the demand
  analyzer re-walked the rest of the chain per `let` for its eagerization
  seed). Each walk is now indexed or incremental: the environment caches
  per-scheme free-variable footprints with aggregate counts and reverse
  indexes, substitutions accumulate through a reverse-indexed composer,
  variable-variable unification binds the younger variable (keeping
  representatives stable), the nested-`let` spine is inferred iteratively,
  and the demand analyzer computes all suffix seeds in one backward pass.
  Measured on do-blocks of 500/1000/2000/3000 chained `let`s (debug
  build): 2.0 s / 6.0 s / 20.6 s / 43.6 s before, 0.10 s / 0.14 s /
  0.24 s / 0.36 s after; release build 0.48 s / 1.19 s / 3.65 s / 7.61 s
  before, 0.03 s / 0.04 s / 0.06 s / 0.10 s after. Real programs compile
  4–6× faster (tracker 0.39 s → 0.06 s, the zpool reader 0.64 s →
  0.13 s, Ed25519 0.51 s → 0.07 s, release). Emitted Lua is
  byte-identical across the corpus, except one `show` call site in the
  zpool reader that now resolves to the exact type-directed `show_Int`
  instead of the last-resort runtime-dispatch `show` — the stable
  representative lets monomorphization see the `Int` it previously lost;
  same output at runtime.

- **The call-site inliner no longer duplicates argument work.** Inlining a
  small helper substitutes the call-site argument expression into the
  helper's body — and previously did so at EVERY occurrence of the
  parameter, so `sq x = x * x` applied to `nfib 30` emitted and evaluated
  the call twice (measured 2× wall time). Pure code, so values and ⊥ were
  unaffected, but GHC's inliner never loses sharing this way, and the
  "shares like GHC" contract (HASKDIFF.md) had this corner open. The
  inliner now applies GHC's rule: an argument may be substituted at
  several occurrences (or under a lambda, which can run it per call) only
  when re-emitting it duplicates no work — a literal, or a variable, whose
  second force returns the memoized thunk value. A non-trivial argument is
  substituted only where its parameter is emitted at most once, counting
  `if`/`case` alternatives at their maximum since only one branch runs;
  any other call site falls back to the ordinary call, which evaluates (or
  thunks) the argument exactly once. Trivial-argument sites — the hot
  paths the inliner exists for, including the whole tracker decode — emit
  byte-identically to before.
- **Type errors point at the offending statement, not the clause head.**
  Previously every type error in a multi-line body was reported at the
  function's first line, because the expression AST carried no source spans
  below the clause. The parser now marks statement boundaries — let/where
  binding bodies, do-statements, case-branch and guard bodies, and `if`
  branches — with a transparent `Expr::Spanned` marker (erased when lowering to
  typed IR), and the checker attributes an error to the innermost such marker it
  was inside. So `c = a <> "oops"` in a `let` reports at the `c` line, and a
  mismatched case/`if` branch reports at that branch. Errors that surface only
  when reconciling a whole binding against its uses, and deferred class-instance
  errors ("No instance for …"), still fall back to the clause head.
- **A constrained FFI import is no longer rejected as undefined.** A body-less
  FFI signature carrying a class context — e.g.
  `dbQuery :: LuaDict b => Db -> (a -> [b] -> a) -> … -> LuaIO ":query_array" a`,
  where the constraint bounds a marshalled argument — was misread as an ordinary
  signature with no accompanying definition, because the FFI-import detector
  stopped at the `=>` qualifier. It now peels the context (and any `forall`) to
  find the trailing `LuaIO`/`LuaPure`/… form, matching type reduction.

## [0.1.5] - 2026-07-23

### Changed

- **Breaking: the integer type is `Int`, not `Integer`.** mata-ll's integer is
  Lua's 64-bit signed integer — it wraps on overflow, exactly like GHC's `Int`
  — so it now carries that name instead of `Integer` (GHC's arbitrary-precision
  type), which mata-ll never implemented. Naming a wrapping type `Integer` was a
  silent soundness deviation against the GHC oracle; this closes it. `Integer`
  written in a type is now a compile error with a `note:` pointing at `Int`;
  `toInteger` is likewise gone (its result would be the absent `Integer`), while
  `fromInteger` stays; and an integer literal past `maxBound :: Int` is a hard
  compile error (GHC only warns, via `-Woverflowed-literals`). Numeric
  defaulting is now `default (Int, Number)`. The `Int`/`Integer` alias that made
  both spellings interchangeable is removed — no back door. Migration is
  mechanical: replace `Integer` with `Int` (and `AnyInteger` →`AnyInt`,
  `toJSONInteger`/`fromJSONInteger` → `toJSONInt`/`fromJSONInt`). Generated Lua
  and runtime behavior are unchanged — only the type's name and the three new
  rejections differ.

- **Breaking: `show` output now matches GHC exactly.** The three measured
  divergences recorded in `DIVERGENCES.md` are converged:
  - `show` of `String` quotes and escapes by GHC's rules: `show "hi"` is
    `"\"hi\""`, control characters take their GHC names (`\NUL`, `\ESC`),
    `\n`/`\t`/`\a`-style shorthands apply, bytes above `\DEL` escape
    numerically, and GHC's `\&` rule breaks the two ambiguous
    juxtapositions (a numeric escape before a digit, `\SO` before `H`).
  - List and tuple separators are `,` with no space (`show [1,2]` is
    `"[1,2]"`, `show (1,2)` is `"(1,2)"`), as in GHC. Record constructors
    show in GHC's record syntax — `P {px = 1, py = "s"}`, fields at
    precedence 0 — instead of positionally.
  - `Number` (`Double`) show is a faithful port of GHC's Burger-Dybvig
    `floatToDigits` plus `showFloat`'s layout: shortest identifying digits,
    positional inside `[0.1, 10^7)` with a mandatory `.0` (`show 3.0` is
    `"3.0"`), `d.ddde<exp>` outside (`show 0.01` is `"1.0e-2"`,
    `show 12345678.0` is `"1.2345678e7"`), and GHC's `NaN`/`Infinity`/
    `-0.0` spellings. Not a printf probe: GHC's stopping bounds are strict
    and its tie rounds up, which differs from correctly-rounding shortest
    printers in half-ulp boundary cases; the port is verified
    byte-identical to GHC 9.14.1 over a 100k random-bit-pattern corpus on
    Lua 5.5 and LuaJIT.
  Negative numbers and negative specials parenthesize at argument
  precedence exactly as GHC: `show (Just (-1))` is `"Just (-1)"`,
  `MkN (-Infinity)` parenthesizes, `MkN NaN` does not. With this,
  `DIVERGENCES.md` carries no pinned runtime divergences; the nineteen
  formerly divergent cases are ordinary GHC-goldened oracle cases now.

- **Breaking: `LuaIterator`'s result must now be written as an explicit
  list.** `LuaIterator "string.gmatch" [String]` names the result list
  directly; the old bare-element shorthand (`LuaIterator "string.gmatch"
  String`, implicitly wrapped to `[String]`) is rejected at parse time with an
  error showing the required shape. The shorthand made the argument ambiguous
  once list elements were supported: `[[Integer]]` means "an iterator yielding
  `[Integer]`", so a bare `[Integer]` argument could not also mean "yielding
  `Integer`". Only the surface syntax changes; the reduction and the generated
  code are unchanged for signatures already written as lists.
- **Cons tails are extracted lazily — GHC one-cell strictness.** Extracting a
  list tail no longer forces the next spine cell to WHNF, so spine walkers
  stop exactly where GHC does: `mapM_ print (1 : error "boom")` prints `1`
  before raising, and `take 2 (1 : 2 : error "boom")` is `[1, 2]` instead of
  an error.
- **IO sequencing is stack-safe at chain terminals; `mapM_` streams large
  lists.** Consuming a large list with `mapM_` previously overflowed the Lua
  stack at around 14k elements (LuaJIT) and pinned the realized spine for the
  whole walk. Each sequencing step is now a proper Lua tail call through the
  runtime's forwarding runner, so the walk runs in constant stack and consumed
  cells become garbage as the cursor advances — measured flat at about 73 KB
  (LuaJIT) / 55 KB (Lua 5.5) over a million elements.
- **Generated output is fully deterministic.** The last source of run-to-run
  variation — emission order of dictionary-form functions (and with it
  `__mll_fn` slot numbering) in the monomorphizer's dictionary-passing
  fallback, which followed `HashSet`/`HashMap` iteration order — is now
  sorted. Compiling the same source twice yields byte-identical Lua, guarded
  by the `codegen_is_deterministic` test.
- **Internal: the code generator is a module directory and emission is
  AST-based.** `mllc/src/codegen.rs` was restructured into
  `mllc/src/codegen/` (fifteen Rust modules plus `runtime.lua`), and the
  string-streaming emitter was replaced: generators build a
  `lua::Expr`/`lua::Stmt` tree (`codegen/lua.rs`) that is printed once, so
  malformed statements are unrepresentable by construction. Output was
  verified byte-identical across the whole corpus during the conversion.
- **Performance: demand analysis reaches further.** Strictness rows for
  runtime-implemented Prelude functions and for `where`-bound local functions,
  captured-variable demand propagation from local functions, strict derived
  `Eq`/`Ord` instances, weighed `$` arguments, aliased bare-variable
  `let`/`where` bindings, and unthunked saturated constructor arguments —
  each removes thunk allocations the previous analysis kept.

- **Breaking: `LuaTry`'s result must now be written as `(Either String a)`,
  matching `LuaCatch`/`LuaIOCatch`.** `LuaTry "io.open" (Either String
  FileHandle)` reduces to `IO (Either String FileHandle)` — the same reduction
  as before, but the `Either String` the binding produces is now explicit in
  the signature instead of implicitly wrapped around a bare payload. The old
  form (`LuaTry "io.open" FileHandle`) is rejected at parse time with an error
  explaining the required shape. Only the surface syntax changes: the generated
  Lua, the `(val, err)` decode (a nil value still becomes `Left`), and the
  reduced type are all identical. The bundled libraries are migrated
  (`Prelude.ffi_getLine`, `LIO.ffi_readLine`/`fOpen`,
  `LOS.getenv`/`remove`/`rename`).

### Added

- **Parser fuzzing.** A deterministic, fully offline fuzz pass
  (`mll-tests/tests/parser_fuzz.rs`) generates random-but-structured .mll
  modules — operator chains over randomly declared fixities, backtick
  operators, sections, prefix minus, layout continuations, and nesting
  probes straddling `MAX_NESTING_DEPTH` — and asserts the parser accepts
  or rejects cleanly: no panics, no aborts, no hangs (a watchdog enforces
  a per-input timeout). Inputs derive from a fixed seed through SplitMix64,
  so any failure reproduces from the seed and index alone. A 2,000-input
  smoke batch runs on every `cargo test`; a 100,000-input batch is
  available behind `--ignored`.
- **AST optimization pipeline over the emitted Lua.** A pass pipeline
  (`codegen/opt.rs`) now runs on the finished statement tree before printing;
  runtime behavior is unchanged (the GHC differential goldens, the laziness
  contract cases, and the full corpus pass on Lua 5.5 and LuaJIT), while the
  output gets smaller and faster:
  - *Paren normalization.* Redundant grouping parens are dropped exactly where
    the enclosing position proves them so; `return (f(x))` becomes the proper
    tail call `return f(x)` for provably single-return callees and in thunk
    bodies. Parens around possibly multi-returning host calls are kept — there
    the paren is Lua's truncation operator.
  - *Dead-branch cleanup.* The `otherwise` guard arm becomes `else`,
    complementary two-arm chains collapse to if/else, and statements after a
    diverging statement are dropped. With the new exhaustive-match emission
    (below), dead `error("Non-exhaustive patterns")` fall-offs in the test
    corpus went from 790 to 296.
  - *IIFE flattening.* Value- and return-position `case`/`let` closures splice
    into the enclosing block (corpus IIFE count 3153 → 2468), budgeted against
    Lua's 200-local limit and declining on any name collision.
  - *Force-of-known-WHNF locals.* `__force(x)` of a single-assignment local
    whose one value is WHNF by construction rewrites to `x`.
- **Exhaustive constructor matches emit `else`.** When a guard-free clause
  chain covers every constructor of the scrutinized type, the last clause is
  emitted as `else` and the unreachable non-exhaustive error is dropped.
- **Fewer redundant forces.** Where-bound function-group names and
  `_warg`/`_arg` entry rebinds are marked concrete, so their uses are no
  longer re-forced (`__force(go)(…)` → `go(…)`); guard chains that provably
  force a parameter on every path now force it once at entry instead of at
  every use. Guard conditions also render at the correct indentation (they
  were previously printed relative to column 0). Tracker benchmark: ~9%
  faster on LuaJIT (median 11.6 s → 10.6 s for the 47 s reference decode).
- **Single-result FFI wrappers truncate.** A foreign import whose declared
  result is a single value now truncates a multi-returning host call
  (`return (math.modf(x))`) instead of forwarding stray extra values — the
  declared type is the contract.
- **`even` and `odd` in the Prelude (GHC parity).** GHC's exact signatures
  (`even, odd :: Integral a => a -> Bool`) and semantics, including
  negatives. HASKDIFF.md's closing example `take 10 (filter even [1 ..])`
  now compiles and runs as documented. Pinned against real GHC in the
  differential oracle (`even_odd.mll`); unused, the definitions are removed
  by dead-code elimination, so existing programs compile byte-identical.
- **Record braces may open on a following line.** Record construction and
  record update now use the same cross-line continuation rule as application
  arguments: the `{ … }` may sit on the next line when indented strictly past
  the enclosing layout block's column, and chained updates may break the line
  between braces — matching GHC's postfix grammar. A brace at do-statement or
  sibling-binding indent is never captured. The accepted forms are pinned
  against real GHC in the differential oracle
  (`record_brace_next_line.mll`).
- **GHC as a differential oracle for the parity corpus.**
  `mll-tests/regenerate-ghc-goldens.sh` runs a mechanical GHC twin of every
  eligible test case (via the shared shim
  `mll-tests/tests/ghc-golden/MllShim.hs`) and pins GHC's stdout as committed
  goldens; the `ghc_oracle_*` tests diff mata-ll's runtime output against them
  byte-exactly on every run (CI needs no GHC). Known divergences are pinned
  and enumerated in `mll-tests/tests/ghc-golden/DIVERGENCES.md` — all reduce
  to three `show` behaviours (unquoted `String` show, `", "` list/tuple
  separators, `Number` via `%.14g`) — and both a silent drift and a silent
  fix fail the suite.
- **`LIOLinear`: linear (`%1`) file-handle IO.** A resource-safe file-writing
  module in the style of linear-base's `System.IO.Resource`: a `WHandle`
  wraps the FFI file handle at `%1`, so the usage checker proves each handle
  is written and closed exactly once. Operations consume and return the
  handle (`hPut` threads it; `hClose` ends the chain); `withOutFile` brackets
  open/close with a `%1` callback, and `openOut` opens a handle directly.
- **`getLine :: IO String` in the Prelude — GHC parity.** Available with no
  import, exactly as in GHC: reads one line from stdin without the trailing
  newline. At end of input it raises the clean, catchable error
  `Prelude.getLine: end of input` (mata-ll's analog of GHC throwing an
  `isEOFError` exception; mata-ll errors are strings, so there is no typed
  predicate) — where a raw `io.read` binding would have let a Lua `nil` escape
  into a `String` and crash later with "attempt to concatenate a nil value".
  Internally built on a `LuaTry "io.read"` binding (`ffi_getLine`), whose
  bare-nil EOF becomes `Left`.
- **`LIO.readLine` no longer crashes at end of input.** `readLine` (still in
  `LIO`, still requires `import LIO`) now presents as `IO String` and receives
  the same hardening as `getLine`: at end of input it raises the clean,
  catchable error `LIO.readLine: end of input` instead of letting `io.read()`'s
  nil escape into a `String` and crash downstream with "attempt to concatenate
  a nil value". A normal line is still returned without the trailing newline
  (`io.read`'s default "l" format — unchanged). Internally built on
  `ffi_readLine :: LuaTry "io.read" (Either String String)`, exactly like the Prelude's
  `getLine`. (Its former declared type was `LuaIO "io.read" String`, which
  reduces to `IO String`, so no caller-visible type change — only the EOF
  behavior changes, from a raw Lua crash to a catchable error.)
  `LIO.readStdin` (the format-argument variant) is unchanged: `io.read(fmt)`'s
  nil is format-dependent ("n" also returns nil for an unparseable number), so
  it needs its own decision.
- **FFI value/constant exports.** An `export` may now be a plain value, not only
  a function or an IO/LuaIO action: `export answer :: Integer` (with `answer =
  42`) is marshalled to Lua directly as `exports.answer = 42`, and a record
  crosses as a keyed table, a tuple as a positional table, an ADT/`Maybe`/list
  by the same result contract a function's return value uses. Previously every
  export was wrapped in a calling wrapper, so a value export emitted
  `__force(42)(…)` and crashed at the boundary. Function and IO/LuaIO-action
  exports are byte-identical to before.
- **Export signatures are checked for FFI marshallability.** An `export` whose
  argument or result uses a type that cannot cross the Lua boundary is now
  rejected at compile time, with an error naming the binder, the offending type,
  the position (argument N / result), and the crossing direction. Rejected: a
  bare polymorphic type variable, a class-constrained type (a dictionary cannot
  cross), a region-scoped `ST`/`STArray`/`STRef` handle, an `IO`/`LuaIO` action
  in argument position, and a callback (function) anywhere but as a DIRECT
  top-level argument — nested in a container, in result position, or as a
  callback's own argument, all of which the code generator can only pass opaque.
  A top-level callback argument stays fully supported (its arguments cross out,
  its `LuaIO` result decoded back in). The whitelist is derived from exactly
  what the marshaller round-trips, replacing a silent, undefined-at-the-boundary
  conversion with a clear rejection; every previously valid export (the
  `ffi_export_*` suite) still compiles unchanged.

- **Numeric typeclass hierarchy — `Num`, `Fractional`, `Real`, `Integral` — with
  polymorphic numeric literals, to GHC parity.** The arithmetic operators are
  now class methods with GHC's exact signatures: `Num` (`+`, `-`, `*`, `negate`,
  `abs`, `signum`, `fromInteger`), `Num a => Fractional` (`/`, `recip`,
  `fromRational`), `(Num a, Ord a) => Real`, and `(Real a, Enum a) => Integral`
  (`quot`, `rem`, `div`, `mod`, `quotRem`, `divMod`, `toInteger`). `Integer` is
  `Num`/`Real`/`Integral`, `Number` is `Num`/`Real`/`Fractional` (as GHC,
  `Integer` is not `Fractional` and `Number` is not `Integral`). You can now
  write ordinary polymorphic numeric code (`sum :: Num a => [a] -> a`,
  `average :: Fractional a => [a] -> a`) and give a user type a hand-written
  `Num` instance (e.g. modular arithmetic on a `newtype`). Integer literals are
  `Num a => a` and decimal literals `Fractional a => a` (via `fromInteger`/
  `fromRational`), resolved by GHC's `default (Integer, Number)` rule when
  otherwise unconstrained. `quot`/`rem` truncate toward zero and `div`/`mod`
  floor, with GHC's exact negative-number remainder signs. `sum`/`product` are
  generalised from `Integer` to `(Foldable t, Num a) => t a -> a`. **Concrete
  `Integer`/`Number` arithmetic is unchanged in the generated Lua** — the
  classes are erased at concrete types (bare Lua operators and the existing
  `div`/`mod` strict cores, no dictionary), verified byte-identical on the
  example corpus and the tracker benchmark. Two deliberate deviations, both
  because mata-ll has no `Rational` type: `fromRational` takes a `Number`, and
  `Real` has no `toRational`. `Floating`/`RealFrac` remain functions, not yet
  classes.
- **Linear types (`%1`), matching GHC's `LinearTypes`.** A function arrow may
  carry a multiplicity: `a %1 -> b` promises its argument is consumed *exactly
  once*, while `a -> b` (= `a %Many -> b`) stays unrestricted. A `%1` value used
  zero times — dropped, left in a `let`/`where` binding that is never forced,
  consumed in only some branches of a `case`/`if`/guard group, partly matched
  by a wildcard, discarded as a non-`()` action result, or consumed inside a
  Maybe bind's continuation (skipped on `Nothing`) — is a compile error, as is
  using it more than once. This catches the double-free / leak class of
  resource bug: e.g. a file handle closed exactly once across the FFI. A scalar
  (`Integer`/`Number`/`Bool`/`String`) derived from a `%1` value — destructured
  from a match, `<-`-bound, or held in a `let`/`where` binding — is tracked
  **exactly once like every other alias**, in strict parity with GHC (which has
  no Movable-style scalar exemption); only a `()`-typed derived result is exempt
  (the run-for-effect idiom). Diagnostics name the variable and explain both
  failure directions (leak vs. double-free) in plain language.
- **Multiplicity polymorphism.** A function may be generic over a multiplicity:
  `apply :: (a %m -> b) -> a %m -> b` lets each caller choose `m`, so a linear
  value threads through helpers, local `where`/`let` functions, and IO/ST/Maybe
  binds without losing its exactly-once guarantee.
- **Dead-code elimination now prunes unused data constructors, not just
  unreachable functions.** A constructor is live only if a kept function
  constructs it (a `Con`/`Var` reference) or matches it in a pattern — DCE now
  walks clause, `case`-branch and `let`/`where` binding patterns to find those
  matches. A data definition none of whose constructors is live is dropped from
  emission at whole-definition granularity, so constructor tags never shift.
  Dropped definitions keep their *metadata* (constructor tags, `LuaDict` string
  tags and field keys, FFI-decoder field types), because a value of a dropped
  type can still flow through live code without being constructed or matched
  there — canonically a `LuaDict` record built by the Lua host and read only
  through field accessors — so only the constructor *function* is elided, never
  the layout information. Effect: the four Prelude datatypes with constructor
  slots (`ExitValue`, `Any`, `Either`, `Ordering` — 12 runtime slots) no longer
  ship in programs that never touch them. Output-shrinking only; behavior
  unchanged. Tests: constructor_dce_unused_data_adds_nothing,
  constructor_dce_keeps_metadata_for_flow_through_types.
- Multiplicities are checked only — they **erase** entirely after type checking,
  so the emitted Lua is byte-identical with or without annotations.
- Boundary (documented in HASKDIFF/SPEC/CAVEATS): the usage checker's
  approximations are all in the *reject* direction — the Lua side of a `%1` FFI
  signature is trusted, and a wildcard over a tainted scalar scrutinee, a
  non-`()` result discard, and a record update on a tainted record over-reject
  conservatively — with a single *accept*-direction relaxation that is sound
  under the lazy runtime: an unannotated `let`/`where` binding that is never
  forced charges zero uses (its thunk truly never runs), where GHC's rule
  charges the right-hand side unconditionally. There is no scalar-memoization
  accept-gap. Tests: linear_affine_basic.mll, linear_mult_poly.mll, and the
  linear_rejects_* / erasure suites in run_mll.rs.

### Fixed

- **Prefix minus follows GHC's grammar.** Prefix minus now has the fixity
  of binary subtraction (`infixl 6`), with GHC's exact consequences:
  `a + -b`, `a - -b`, `a * -b`, and ``a `div` -b`` are parse errors (the
  error explains the rule and suggests `a + (-b)`), the same rejection
  applies inside right sections (`(+ -2)`), and `-a <> b` is rejected
  because `infixr 6` defines no grouping against the negation. Groupings
  changed to GHC's: `-a * b` is now `negate (a * b)` and ``-a `div` b`` is
  ``negate (a `div` b)`` — observably different from the previous
  `(-a) `div` b` for odd negatives — while `-a + b` stays `negate a + b`.
  A parenthesized `(-a op b)` follows the same grammar instead of negating
  the whole body: `(-a + b)` was previously `negate (a + b)`, which is
  simply the wrong value. Previously all the rejected forms were silently
  accepted with groupings GHC refuses. A `Number` literal with a whole
  value (`10.0`, `1.0`) was emitted as a bare integer spelling, which Lua
  5.3+ reads as a native integer — so Double-typed arithmetic silently ran
  on wrapping 64-bit integers (`10.0^20`-style products wrapped past 2^63)
  and `negate 0.0` lost the sign of zero. Number literals now always carry
  the float marker (`10.0`, `1e20`) in the generated Lua, in expressions
  and patterns both.
- **A non-lambda bind as the final do-statement now typechecks.** With at
  least one statement before it, a final `step 1 >>= print` (or
  `a >> print 9`, or a chain) was rejected — "Cannot unify 'IO a' with
  'b -> IO ()'" — even though the same expression typechecked at top level
  and in non-final statement position. The do-chain flattener treated the
  operator's right operand as the chain's next statement, so `print`'s
  function type was unified with the do-block's monad (and `>>`'s second
  argument against a synthetic continuation arrow). A non-lambda right
  operand now ends the flattened spine: the whole expression is the chain's
  terminal, typed by the ordinary infix rule. Ill-typed finals keep GHC's
  rejection: a final `IO Integer` statement in an `IO ()` do-block fails
  with the same Integer-vs-() mismatch as GHC. This defect predates the
  fixity fix (it reproduces on builds where `>>=` was still
  right-associative); the `infixl 1` change only made the shape common.
- First-class `>>=`/`>>` no longer crash or miscompile. `m >>= f` with a
  non-lambda continuation (`step 1 >>= print`) executed the chain and then
  raised "attempt to call a nil value": the generated code called the result
  of `f x` as if it were an action closure, but applying an IO-typed function
  already performs the action and returns its result. The application now
  flows through the runtime's forwarding runner, which returns a plain result
  as-is, forwards a pure box, and calls a first-class action closure — so
  non-lambda continuations, chains (`a >>= f >>= g`), and continuations
  ending in `return` all behave as in GHC. Passing the operators themselves
  as values (`apply2 (>>=)`, `thenOp (>>)`) emitted `_a >>= _b` verbatim into
  the Lua output — a Lua syntax error; they now build proper deferred action
  closures. Both shapes were unreachable while `>>=` was wrongly
  right-associative and became ordinary code with the `infixl 1` fixity fix.
- **Standalone programs run `main` even when invoked with command-line
  arguments.** The entry-point stub decided "loaded via `require`?" by testing
  whether the chunk's first vararg was nil — but a standalone interpreter
  (`lua prog.lua x`) passes the CLI arguments as varargs, so any argument
  made the program look like a required module and `main` silently did not
  run. The stub now compares the first vararg against `arg[1]`, which
  distinguishes the two invocation styles reliably.
- Non-associative operators no longer chain. `a == b == c` is now a parse
  error, exactly as in GHC: `==` is `infix 4`, so the expression has no defined
  grouping (the error suggests parenthesizing one side, or `&&` for a
  three-way comparison). The full GHC precedence-parsing rule is enforced — a
  chain of same-precedence operators is rejected when any of them is
  non-associative or when their associativities disagree (an `infixl` next to
  an `infixr` at the same level). Previously every precedence-4 operator
  parsed left-associatively and such chains were silently accepted with a
  grouping GHC would refuse.
- Fixity now travels with an import, as in GHC. An operator imported from
  another module keeps the fixity its defining module declared — `infixr 6 -.`
  in the exporting module groups `10 -. 3 -. 2` rightward at the import site,
  and an imported `infix 4` operator is non-associative there too. Previously
  imported operators silently fell back to the `infixl 9` default. A fixity
  declaration also now governs its whole module, including uses that precede
  it textually (Haskell scoping), instead of applying only from its line down.
- The Prelude's fixity interface now matches GHC and reaches user code:
  `infixl 4 <$>, <*>` apply in every module (previously only inside the
  Prelude itself), and `div`, `mod`, `elem`, and `seq` carry their GHC
  fixities (`infixl 7`, `infix 4`, `infixr 0`). This corrects real groupings:
  ``5 * 2 `div` 4`` is now `(5 * 2) `div` 4` = 2 as in GHC, previously
  ``5 * (2 `div` 4)`` = 0. `>>=` and `>>` are now `infixl 1` as in GHC
  (previously right-associative). Fixity declarations additionally accept
  GHC's full form: backtick-quoted names (``infixl 7 `div` ``), comma lists
  (`infixl 7 *, /`), and a precedence outside 0-9 is rejected.
- A parenthesized operator in a module export list — `module M ((-.)) where`,
  the GHC form — now parses. It was previously skipped as an unknown token,
  corrupting the export list and failing on the rest of the header.

## [0.1.4] - 2026-07-17

### Changed

- **`LuaIterator`'s type argument now names the RESULT list, and each yielded
  element is decoded.** A list argument `LuaIterator "f" [E]` reduces to `[E]`
  (the iterator yields one `E` per step) rather than the old `[[E]]`; a bare
  element type `T` remains the `[T]` shorthand, so existing scalar-argument
  bindings (`LuaIterator "string.gmatch" String -> [String]`) are unchanged.
  Each yielded value is now decoded as the element type through the same
  `__mll_ffi_decode` path as any other FFI result, so a structured element
  (chiefly a list — `LuaIterator "f" [[Integer]]`, whose host yields Lua
  arrays) becomes a proper cons list instead of a raw Lua table; before, such
  a value surfaced later as `show: expected a list but got a raw … value`.
  Scalar/opaque elements need no decode and keep their exact old codegen.
  Fixes the `examples/iterator/` example. Regression test:
  `lua_iterator_type_argument_is_the_result_list_and_elements_decode`.

### Added

- **Promoted data types now have real kinds (DataKinds).** A parameterless,
  non-GADT data type promotes to a kind named after it: `data Nat = Z | S Nat`
  gives the kind `Nat`, with `'Z :: Nat` and `'S :: Nat -> Nat` (each field
  type promoted — a field that is itself a promotable type contributes its
  kind); the builtin `Bool` promotes too (`'True`/`'False :: Bool`). An index
  variable's kind is inferred from promoted constructors in GADT return types
  (`n : Nat` from `VNil :: Vec 'Z a`) and type-family patterns
  (`Plus :: Nat -> Nat -> Nat`), replacing the previous approximation that
  classified every promoted constructor at kind `Type`. Consequences: an index
  at the wrong promoted kind is a clear error — `Vec 'True Integer` reports
  *"Vec needs an argument of kind Nat, but 'True has kind Bool"* — and a
  natural-number family applied to a `Bool` tag (`Plus 'True 'Z`) is rejected;
  a promoted data type stays usable as an ordinary value type (type/kind
  duality). Type-family REDUCTION is unchanged (it still matches promoted
  constructors structurally); only argument/result kinds are now checked. New
  kind `Kind::Promoted`. Honest limits (documented in HASKDIFF/SPEC/CAVEATS):
  only parameterless, non-GADT data types promote to a real kind (others keep
  the `Type` approximation — a precise promotion would need kind polymorphism,
  which mata-ll does not have); and, with no kind-signature syntax, a non-GADT
  phantom parameter's kind defaults to `Type`, so a promoted tag must be pinned
  through a GADT constructor return type (as `Vec`/`Light`/`Input` do — GHC
  requires a kind signature in the same case). Tests: promoted_nat_kind.mll
  plus promoted_kind_rejects_* / _type_family_argument_is_checked /
  _well_kinded_index_accepted / _non_gadt_phantom_tag_rejected_but_gadt_pins_it
  / promoted_type_still_usable_as_a_value_type.
- **Closed type families now reduce during unification, symbolically.**
  Previously a family only reduced on ground/concrete arguments (at
  AST-to-`Ty` conversion) and the unifier treated a family application as
  opaque, so length arithmetic failed (`Plus 'Z m` would not become `m`, and
  the occurs check spuriously fired on `m occurs in Plus 'Z m`). The unifier
  now reduces closed-family applications to normal form on both sides before
  matching — including when arguments contain type variables — so a
  length-indexed `vappend :: Vec n a -> Vec m a -> Vec (Plus n m) a`
  type-checks, runs with correct lengths, and keeps length mismatches a
  compile error. A family application stuck on an unknown variable is left
  irreducible and deferred (not an occurs-check failure); families are NOT
  assumed injective (`F a ~ F b` does not give `a ~ b`, and two distinct
  stuck applications do not unify); and a non-terminating family
  (`Loop x = Loop x`) is reported as "type family did not terminate" rather
  than looping (the reduction is a fuel-bounded, iterative normalizer shared
  by the eager and unification paths, so it cannot overflow the stack — which
  the old recursive concrete reduction did). Eager concrete reduction (the
  `Id`-style case) and the FFI type families (`LuaIO`, …) are unchanged. This
  is step 1 of type-level naturals; a real promoted `Nat` kind is future
  work (`'Z`/`'S` are still classified at kind `Type`). New diagnostic:
  `TypeFamilyDivergence`. Tests: type_family_arithmetic.mll plus
  type_family_concrete_reduction_still_works / _length_mismatch_rejected /
  _head_of_empty_append_rejected / _non_injectivity_not_assumed /
  _divergence_errors_not_hangs.
- **A full kind system.** Kinds (`Type`, `Symbol`, and arrow kinds like
  `Type -> Type`) are now inferred for everything declared at the type level
  and every written type is kind-checked. A `data`/`newtype` parameter's kind
  comes from its use in the fields (`data Wrap f = Wrap (f Integer)` infers
  `Wrap : (Type -> Type) -> Type`; mutually recursive declarations are solved
  together); a class variable's kind comes from the method signatures
  (`foldr :: (a -> b -> b) -> b -> t a -> b` forces `t : Type -> Type`) and
  from superclass agreement; constraints fix kinds too (`Foldable t => t ->
  Integer` is now a kind error). Unconstrained kinds default to `Type`
  (Haskell-2010 defaulting); there are no kind annotations and no kind
  polymorphism. Checked positions: type signatures, export signatures, data
  fields (per-constructor scope, so existential variables must be used
  consistently), newtype bodies, alias bodies, class method signatures,
  instance heads AND instance contexts, type-family equations, and expression
  ascriptions. An instance head must have exactly the class variable's kind:
  `instance Foldable []` (the new bare-`[]` type syntax, kind `Type -> Type`)
  is well-formed, while `instance Foldable [a]`, `instance Foldable Integer`
  and `instance Show Maybe` are compile-time kind errors with plain-language
  explanations (the `[a]`-for-`[]` trap gets a targeted note). Breaking only
  for programs that were silently ill-kinded before (e.g. applying a
  `Type`-kinded alias, or a bare unsaturated constructor in a field). Tests:
  kinds_hkt.mll plus the kind_error_* / bare_list_constructor_* /
  higher_kinded_* suites.
- The bare list constructor `[]` is now valid type syntax (kind
  `Type -> Type`), so instance heads can name it: `instance Foldable []`.
- The builtin `Foldable`, `Traversable`, `Semigroup` and `Monoid` instances
  for `[]`/`Maybe`/`Either` (Foldable/Traversable) and `String`/`[a]`
  (Semigroup/Monoid) moved out of the compiler's Rust tables into ordinary
  `instance … where …` declarations in `lib/Prelude.mll`, now that the kind
  system can check their heads. The Prelude is exempt from the
  orphan-instance rule (it is the home module of the builtin classes and
  types, so its instances are never orphans — GHC's rule). No user-visible
  change. The list `Semigroup`/`Monoid` bodies use the ordinary `(++)`
  operator; the `String` bodies call the runtime string-concatenation
  primitive `semigroup_String` (Lua `..`), now exposed to source because a
  mata-ll `String` is opaque and has no `(++)`. The now-unused
  `semigroup_List` runtime helper was removed. The `Semigroup`/`Monoid`
  *class declarations themselves* also moved to `lib/Prelude.mll` (see the
  source-class constraint-synthesis entry below, which is what let the
  classes move while keeping `mempty`'s ambiguity check). `mappend` still
  works on lists, and the `(<>)` operator on a concrete list is still a
  compile error directing to `(++)` (the divergence lives in the
  monomorphizer's dispatch, independent of where the instance is declared).
  Tests: monoid_instances.mll (constructed-value append/mempty/foldMap) plus
  the list_semigroup_operator_still_rejected / mappend_on_lists_still_works /
  mempty_ambiguity_preserved suite.
- **Source-defined classes now get the same class-constraint synthesis the
  builtin classes had.** `register_class` synthesizes, for each method that
  mentions the class variable, the constraint `ClassName classVar` (the same
  `method_constraints` mechanism the builtins registered by hand for
  `show`/`==`/`mempty`/…). A use of a class method therefore emits a wanted
  the solver must discharge to a concrete instance. The payoff: a
  return-position-only method whose type variable nothing determines — e.g. a
  user `class Default a where def :: a`, used without an annotation — is now
  a compile-time **ambiguity** error with the usual "add a type annotation"
  guidance, instead of silently compiling and crashing at runtime with
  "attempt to call a nil value". A use at a concrete instance-less type is a
  compile-time "No instance" error. Scoped precisely so nothing over-
  constrains: the constraint is emitted only when the method's signature
  mentions the class variable, and the existing discharge machinery
  (`binder_types` / signature variables) still leaves it satisfied whenever an
  argument determines the variable — so `op :: t a -> Integer` resolves
  silently, exactly as before. This is what enabled the `Semigroup`/`Monoid`
  class declarations to move to the Prelude (above). It also fixed a latent
  gap in `has_instance` that this newly exercised: a user
  `instance C (Maybe a)` for a non-structural class `C` is now recognized
  (the `Maybe` shortcut previously ignored the instance registry, unlike the
  `[a]` path). Tests: source_class_nullary.mll plus
  source_class_nullary_ambiguity_rejected /
  source_class_method_resolves_when_determined.
- `Foldable` and `Traversable` typeclasses, with instances for lists, `Maybe`
  and `Either` (folding/traversing `Right`), plus the `Monoid` class
  (superclass `Semigroup`; methods `mempty`/`mappend`; `String` and `[a]`
  instances) behind `foldMap`. `foldr` and `foldl` are now Foldable class
  methods rather than list-only Prelude functions, and `length`, `null`,
  `elem`, `sum` and `product` are generic over Foldable (`sum`/`product`
  remain fixed at `Integer` — there is still no `Num` class). New Prelude
  functions: `maximum`, `minimum`, `foldMap`, `mempty`, `mappend`,
  `traverse`, `sequenceA` and `liftA2`; new stdlib modules `Data.Foldable`
  (home of `toList`, which stays out of the Prelude exactly like GHC's) and
  `Data.Traversable` (re-exports). User types join all three classes with
  ordinary `instance` declarations (`instance Foldable Tree where …`), and
  the generic functions dispatch through them. Laziness is preserved:
  `null`/`elem` still short-circuit on infinite lists, and type-erased
  generic contexts fall back to new runtime `foldr`/`foldl` implementations
  (the same role `map` plays for `fmap`). `liftA2` is a real Applicative
  method (as in GHC) because a `<$>`/`<*>` chain routes a function-valued
  intermediate through the applicative, which the type-erased IO runtime
  cannot represent; `traverse` is built on `liftA2` so it works at `IO`.
  Tuples have no Foldable/Traversable instance (mata-ll has no
  partially-applied tuple constructor). Deviations from GHC are documented
  in HASKDIFF.md. Tests: foldable, traversable, foldable_user_instance,
  lib_data_foldable.
- Redefining a Prelude function whose type became Foldable-generic at its old
  monomorphic list type (e.g. `sum :: [Integer] -> Integer`) is now the
  documented user-wins redefinition case rather than a same-type duplicate,
  and compiles. Exact-type duplicates are still rejected. Tests:
  prelude_same_type_duplicate_definition_rejected (now probed with
  `reverse`), prelude_foldable_generic_allows_monomorphic_redefinition.

### Fixed

- **Type-soundness hole closed: unpacking an existential constructor now
  skolemizes the hidden type variable.** Previously
  `coerce (MkShowBox x _) = x` type-checked with ANY declared return type
  — an unchecked coercion deferring the type confusion to a Lua runtime
  crash — and GADT-syntax existentials leaked identically. Each pattern
  match now mints a fresh rigid skolem for every existential variable: it
  unifies only with itself, and an occurrence in any type that outlives
  the match (the function's own type, a `case` result, a `where`-function's
  type) is rejected as an escape. Every diagnostic mentioning a skolem
  carries a note naming the constructor that hid the type and what its
  declared context guarantees. Constructor contexts
  (`data Showable = forall a. Show a => Showable a`, and GADT
  `MkBox :: Show a => a -> Box`) are enforced in both directions: packing
  proves the instance at the concrete type, unpacking provides exactly the
  declared classes (plus superclasses) on the skolem and nothing more. The
  record back doors are closed the way GHC closes them: a field whose type
  mentions the hidden variable has no selector function and cannot be
  record-updated (both get targeted errors; construction, positional
  matching, and other fields' selectors are unaffected). A malformed
  constructor context (unknown class, constraint on a non-existential
  variable, non-variable constraint argument) is now reported at the data
  declaration instead of being silently dropped. Breaking only for
  programs that exploited the hole. Tests: existential_constraints.mll
  (constrained/GADT/record existentials run end-to-end) and the
  existential_unpacking_skolemizes / existential_skolem_cannot_escape /
  existential_constraints_enforced_both_ways /
  existential_record_fields_have_no_selector_or_update error-path tests.

- Constraint discharge no longer applies the structural element rule to
  higher-kinded instances: a wanted like `Functor (Either String)` — the
  class variable bound to a partially-applied constructor — was wrongly
  rejected with "No instance for 'Functor (Either String)'" because the
  built-in instance's empty context fell back to requiring `Functor String`.
  Built-in higher-kinded instances now carry an explicit empty context, and
  a registered non-structural list-head instance (e.g. `Monoid [a]`) is
  consulted instead of being unconditionally rejected.

- Every compiled module now carries compiler provenance: `__MLLC_VERSION` (the
  `mllc` crate version) and `__MLLC_COMMIT` (the full git commit the compiler
  was built from). Both are emitted as top-level `local`s so they are present
  in every generated file, and are surfaced through the module's export table
  when it has one, so a Lua host can read them as properties of the required
  module (`require("M").__MLLC_VERSION`). The commit is captured at build time
  by a new `mllc` build script (`git rev-parse HEAD`), degrading to `"unknown"`
  when git or the `.git` directory is absent (e.g. a packaged tarball build),
  so the build never fails.
- Compiling a module with neither a `main` nor any `export` declaration now
  emits a warning instead of silently producing a Lua file with no runnable or
  callable code (dead-code elimination is rooted at exactly `main` and the
  exports, so such a module compiles to an empty shell). The warning's notes
  call out the classic mixup: a `module M (foo) where` header export list
  controls only which names other `.mll` modules may import — as documented,
  it does not export anything to the Lua host, which is exclusively the
  `export` keyword's job. Warnings are carried on a new
  `CompileResult.warnings` field; the `mll` CLI prints them to stderr and the
  wasm playground shows them as a leading comment block. The compile still
  succeeds and the file is still written.

### Fixed

- A `<-`-bound result of a user-defined action is no longer assumed to be an
  already-forced value. The non-strict `return`/`pure` fix (0.1.3) made a
  user action whose body ends in `pure <expr>` yield its result as an
  unevaluated thunk, but the bind site still marked the bound variable
  "concrete" (readable without forcing) for every non-`return` action — so a
  strict use of the variable compiled to a force-free read of a thunk table
  and crashed at runtime ("attempt to perform arithmetic on a table value"):

      v <- stHelper arr n      -- stHelper ends in `pure (g x * 2)`
      return (v + 1)           -- emitted bare `v + 1` over a thunk: crash

  The WHNF claim now mirrors the code generator's emission arms exactly and
  defaults to *not proven*: it is kept only for shapes whose emitted result is
  provably forced (`pure` of a provably-total value, literal/constructor/tuple
  actions, FFI calls, fused ST intrinsics), and dropped for calls to
  user-defined actions, whose uses then force normally (an idempotent, cheap
  probe). Relatedly, `runST` now forces the state thread's result to WHNF —
  in GHC, demanding `runST m` *is* demanding the returned value — so a
  suspended terminal `pure e` can no longer escape the ST boundary as a raw
  thunk and reach force-free consumers like `show` (which rendered the thunk's
  internal table as a spurious tuple). Structured laziness is unchanged: WHNF
  of a tuple/cons/constructor does not touch its fields, and `return ⊥` stays
  inert until demanded. Test: action_result_whnf.
- The tracker example no longer retains a whole pattern of per-frame sample
  thunks: `mixFrames` now concatenates each tick's accumulated PCM strictly
  (`seq`, GHC's `return $!`) instead of returning the concat as a lazy thunk.
  With the now-correct non-strict `return` (0.1.3), the previous code kept
  every per-frame cons cell and arithmetic thunk alive until the chunk was
  finally written — the classic lazy-accumulator space leak (GHC behaves
  identically on the same code) — inflating peak heap from ~20 MB to ~4.3 GB
  on the CI benchmark. Decoded output is byte-identical and peak heap is back
  to ~15 MB. (A separate speed regression that also shipped in 0.1.3 — the
  thunked mixer that got JIT-blacklisted — is addressed by the demand-analysis
  and redundant-force work under Performance below, not by this fix.)
- **Every Lua string literal is now escaped through one canonical routine.**
  String escaping had been hand-written in three places and only the expression
  path (`gen_literal`) was correct: a string literal used in a *pattern* was
  emitted with no escaping at all, and a `LuaDict` record `as` key escaped only
  the quote and backslash. A quote, newline, or other control character in a
  string pattern (`f "a\"b" = …`) or an `as` key (`field as "a\nb"`) therefore
  produced generated Lua that would not even load. The expression path, the
  pattern-literal path, `lua_key_string`, and the two FFI decode-descriptor
  field-name sites now all route through a single `lua_quoted_string`, which
  also emits control characters as three-digit `\ddd` escapes so a short escape
  can never silently merge with a following digit (`"\01"` is now `"\0"`
  followed by `"1"`, not the single byte `\1`). Regression tests:
  `string_pattern_literal_with_quote_and_newline_is_escaped`,
  `luadict_as_key_with_control_chars_is_escaped`.
- **FFI target names are validated as well-formed Lua callees.** An FFI target
  string (`LuaPure`/`LuaIO`/`LuaIterator`/`LuaTry`/`LuaCatch`/`LuaIOCatch`) is
  emitted verbatim as the thing being called in the generated Lua, so a
  malformed name such as `LuaPure "a b" Int` produced `a b(...)` — broken Lua
  that fails to load — instead of a clear error. The target is now checked once
  at the declaration (`validate_ffi_callee`), accepting exactly the well-formed
  callee shapes — a bare name, a dotted path (`math.floor`), an indexed path
  (`handlers[1].run`), and a trailing or bare `:method` — and rejecting anything
  else with a compile-time diagnostic that names the offending string and lists
  the valid forms. The legitimate dotted and method forms are unaffected.
  Regression tests: `ffi_target_with_space_is_rejected_at_compile_time`,
  `ffi_target_other_malformed_forms_are_rejected`,
  `ffi_target_dotted_and_method_forms_still_work`,
  `ffi_target_indexed_and_trailing_method_forms_compile`.
- **Deeply nested input yields a clean error instead of crashing the compiler.**
  The parser, the typechecker, and codegen all recursed on the native stack with
  no depth bound, so pathologically nested input — thousands of nested parens, a
  long operator spine (`1+1+1+…`), a deep signature or pattern — aborted the
  whole process with a stack overflow. The compiler thread now runs on a large
  shared stack (`COMPILER_STACK_SIZE`) and every recursive-descent traversal
  that could overflow carries a depth guard (`MAX_NESTING_DEPTH`): the parser's
  expression/type/pattern productions, `ast_type_to_ty`, `infer_expr` /
  `check_expr_typed`, and codegen's `gen_expr` walk. Stack size and the depth
  limit are sized together, so input past the limit reports a plain "nested too
  deeply" diagnostic while realistic code — including a 1000-plus-element list
  literal — still compiles. The test harness and REPL compile on the same stack
  size so the limit behaves identically there. Regression tests:
  `deeply_nested_parens_yield_clean_depth_error`,
  `deeply_nested_types_yield_clean_depth_error`,
  `operator_spine_past_limit_yields_clean_depth_error`,
  `thousand_element_list_literal_still_compiles_and_runs`.
- **Type-alias expansion is bounded by size, so a doubling alias tower no longer
  hangs the compiler.** A self-doubling tower (`type Pi a = P(i-1) (P(i-1) a)`)
  expands to a type whose *size* is exponential in the number of levels while
  its *depth* stays small — so the recursion-depth guard above never saw it, and
  the compiler ground through the exponential expansion for a long time (it had
  previously masked itself as a stack overflow, and became a hang once the stack
  grew). Type-alias expansion is now charged fuel by the size of each expanded
  body — mirroring the closed-type-family reducer's size-charged fuel — and
  reports a clean "type alias expansion did not terminate" diagnostic once the
  per-signature budget is exhausted; the same guard catches a self-referential
  alias (`type A = [A]`). Ordinary multi-level alias use charges only a few
  hundred units and is unaffected. Regression test:
  `doubling_alias_tower_yields_clean_size_error`.

### Performance

- **The tracker recovers the JIT multiplier it lost in 0.1.3.** Two changes in
  0.1.3 — thunking the audio mixer's per-frame results (the non-strict-argument
  work) and routing `div`/`mod` through a runtime wrapper — made the mixer's hot
  loop allocate closures each iteration, which LuaJIT cannot compile: the trace
  aborted on `NYI: bytecode FNEW`, the loop was blacklisted, and the whole inner
  mixer fell back to the interpreter, losing roughly the entire ~4× JIT speedup.
  Decoding `HongKong_Music.it` had regressed to **338 s wall (2.50× real-time),
  78 MB peak RSS**. Two fixes restore it: (1) per-field demand analysis — a
  greatest-fixpoint, branch-aware pass that keeps a provably-demanded binding
  eager instead of thunked, so the mixer's arithmetic no longer builds closures
  (338 s → 120 s); and (2) a WHNF-emission predicate that is the single source of
  truth for "this expression is already forced", suppressing redundant `__force`
  wraps at every strict site (120 s → **102 s / 0.76× real-time / 15 MB**). That
  matches the pre-regression eager build (101 s) and the decoded output stays
  byte-identical (`cdd386f6985dca3561fe1a2689231c78`) throughout. The demand
  pass and the WHNF predicate are general codegen improvements, not
  tracker-specific. See `experiments/tracker/PERF-REGRESSION.md` (the
  directory has since moved from `examples/` to `experiments/`).

## [0.1.3] - 2026-07-14

### Added

- FFI-result shape mismatches now fail with a clear, localized runtime error.
  The type-directed decoder that runs on every value crossing the Lua FFI
  boundary checks the declared shape as it decodes: a record missing a declared
  field, a wrong-typed field or element, a scalar where a list/record/tuple was
  declared, or a missing multi-return value now raises
  `FFI result: declared T but the host returned X`, naming the position (e.g.
  `field 'certPort' of record Cert`, `element 2 of the tuple declared
  (String, Integer)`) and the host function whose result was being decoded —
  instead of surfacing later as an arbitrary Lua error (nil index, arithmetic
  on nil) deep in user code. Multi-return tuple results are now decoded like
  every other FFI result, so structured elements (lists, Maybe, records) are
  converted correctly there too. Valid host values are never rejected: bare
  scalar results stay check-free (the hot path), and a mata-ll value
  round-tripping through the host unchanged — such as the lazy tuple state of
  an outgoing-callback fold — passes through untouched, preserving its
  laziness.
- `mll -v` / `mll --version` prints a short MIT license summary, the crate
  version, and the git commit the binary was built from, then exits (no source
  file required). The commit hash is captured at build time by a build script
  via `git rev-parse --short HEAD`, degrading to `unknown` when git or the
  `.git` directory is absent (e.g. a packaged tarball build), so the build never
  fails. The flag is lowercase `-v`; clap's default uppercase `-V` version flag
  is disabled.

### Fixed

- The FFI argument marshaller is now a complete structural dual of the result
  decoder, closing a class of bugs where a container type the decoder converts
  was not converted on the way out. The concrete gap: a `HashMap` with
  structured values (`HashMap String [Integer]`, `HashMap String (Maybe X)`,
  `HashMap String Record`, nested maps) passed to a host had its values left
  unmarshalled — each reached the host as a raw cons cell / `Just` wrapper /
  lazy field instead of a plain array / bare value / dict, so a host iterating
  `pairs(m)` saw `getmetatable(v) ~= nil`. HashMap values are now marshalled by
  the value type at any nesting (keys, being scalars already usable as Lua keys,
  are kept, matching the decoder and `__mll_to_lua`). This is a long-standing
  gap, not a regression — `HashMap` was never in the argument marshaller, so no
  released version marshalled a structured map value. The marshaller now covers
  every container the decoder descends into — list, tuple, `LuaDict` record,
  `HashMap`, `Maybe` — so encode-then-decode is identity at every depth
  (regression test `ffi_arg_marshal_roundtrips_all_containers` echoes each
  container and nesting through a host and asserts identity). Two further
  correctness fixes were made in the same pass: (1) the marshaller is now
  **non-destructive** — a converted container is rebuilt into a fresh Lua value
  instead of mutating the mata-ll value in place, so a value passed to a host
  and then reused in mata-ll code is no longer corrupted (previously the reused
  value found a Lua array where a cons list was expected and crashed in `show`);
  (2) an opaque value (type variable, `LuaUserData`, function, plain ADT) is still
  left raw, so the fold-state opaque round-trip (`examples/ffi_fold`) is intact.
  Test: ffi_hashmap_structured_values_marshalled.
- List-typed FFI arguments crossing OUT to a Lua host are now marshalled into
  plain Lua arrays. A list passed to a host function — on its own
  (`f [1,2,3]`), nested inside a `LuaDict` record field, or nested inside
  another list — reached the host as a raw mata-ll cons cell (head at `[1]`,
  the spine's tail thunk at `[2]`) instead of a 1-based array, so the host read
  `arr[1]` as the first element but `arr[2]` as the tail function and any use of
  it failed (`attempt to perform arithmetic on a function value`). The argument
  marshaller only descended into tuples and `Maybe`, skipping lists and records
  entirely, so any list crossing to a host was affected. The argument-direction
  marshaller is now the dual of the result decoder: a list becomes an array
  with its elements forced and
  recursively marshalled, and a tuple's or record's lazy fields are forced in
  place (with nested lists converted), at every depth. Opaque arguments — a
  polymorphic type variable, `LuaUserData`, a function, a plain ADT — still pass
  through raw with a shallow force, so a fold's threaded state (including a
  tuple state) round-trips untouched exactly as before.
  This also repairs a regression within the 0.1.x line: a `String` that is
  *built* rather than written as a literal — decoded from JSON, or produced by
  any `[Char]` list operation — is a cons structure, not a native Lua string.
  Passing such a `String` as an FFI argument worked in 0.1.2 but, after cons
  heads were made lazy during this cycle, it began reaching the host as a raw
  cons table, so a host reading e.g. `params.hostname` got a table and failed
  with `error converting Lua table to String`. The marshaller now collapses a
  declared-`String` argument to a native Lua string regardless of how it was
  constructed. (A `String` *literal* is already native, which is why the earlier
  literal-only tests never caught this.) Tests:
  ffi_list_argument_marshalled_to_array,
  ffi_json_decoded_string_argument_is_native_string; example: examples/ffi.
- A `Maybe` field inside a `LuaDict` record crossing OUT to a Lua host is now
  unwrapped. `Just x` reached the host as the raw `{x}` wrapper table (a
  metatable-tagged one-element table) instead of the bare value, so a host
  reading e.g. `params.port + 1` failed with `attempt to perform arithmetic on
  a table value`; `Nothing` likewise did not become an absent (`nil`) field.
  This is a long-standing gap — a `Maybe` field never marshalled correctly: in
  0.1.2 the argument marshaller did not descend into records at all, and after
  records/lists were handled this cycle it still descended into the `Just`
  wrapper without stripping it, so no released version handed the host a bare
  value. The structural marshaller now unwraps a `Maybe` reached through a
  record field, list element, or tuple field — `Just x` becomes the bare `x`
  recursively marshalled by `x`'s type (`Just 443` → `443`, `Just [1,2,3]` → a
  real array, `Just "s"` → a native string), and `Nothing` becomes `nil` —
  exactly inverting the result decoder and matching `__mll_to_lua`, so a record
  with a `Maybe` field round-trips through a host unchanged. The separate
  top-level *optional positional argument* feature (a `Maybe` FFI parameter that
  `Nothing` omits) is unaffected: that path still keeps the wrapper until it is
  consumed positionally and only marshals the payload. Test:
  ffi_maybe_field_marshalled_and_roundtrips.
- `return`/`pure` are now non-strict, matching GHC and the eagerness contract:
  a returned value is left unforced until it is demanded. Previously the IO
  bind/`return` path forced the value to WHNF eagerly, so `_ <- return (error
  "x")`, a bare `return (error "x")` do-block statement, and `fmap f (return ⊥)`
  all raised where GHC leaves the value untouched. `return`/`pure` now emit their
  argument through the same eagerness weighing used for call arguments — a
  provably-total value stays eager (`return 0` is still a bare `0`, no thunk),
  while a possibly-⊥ value is suspended and raises only when forced; a `<-`-bound
  returned value is forced at its use site rather than at the bind. One
  user-visible consequence, also matching GHC: a bottom returned *inside* `try`
  is no longer caught unless it is forced there — use `` e `seq` pure () `` (GHC's
  `evaluate`) to demand a pure value inside the tried action. Structured laziness
  is unchanged (a returned tuple keeps its field laziness; the Maybe monad stays
  lazy). Test: return_non_strict.
- Prefix and partially applied `div`/`mod` now work, not just the backtick
  forms. `div 7 2`, `map (div 10) xs`, and any first-class or higher-order use
  (`foldr div z xs`, passing `div` to another function) previously type-checked
  and then crashed at runtime with "attempt to call a nil value" — they compiled
  to a call of a Lua global `div`/`mod` that does not exist; only ``a `div` b``
  and its backtick section worked. `div`/`mod` are now reified as first-class
  functions the same way `seq` was this cycle: a prefix, partial, or first-class
  occurrence resolves to a runtime wrapper (`__mll_div_fn`/`__mll_mod_fn`) that
  forces both arguments to WHNF and then runs the existing strict core, so the
  result is identical to the backtick form in every shape — floor semantics on
  negative operands and the divide-by-zero error included, and safe when a caller
  hands the operator unforced (thunk) arguments (`map` over a lazy list). The
  inline backtick `` a `div` b `` is unchanged: it stays on the bare strict core
  with pre-forced operands, so the arithmetic hot path (e.g. the tracker mixer)
  is byte-identical and carries no extra force. Test: div_mod_prefix_forms.
- Redefining a name the Prelude or a builtin provides (`error`, `map`, `sum`,
  …) at the top level is now reported once, clearly, at the user's own
  definition site: `'error' is already provided by the Prelude and cannot be
  redefined`, with a `note:` explaining that mata-ll compiles the Prelude and
  the program into one flat namespace (so a top-level definition does not
  shadow the Prelude name as it would in GHC) and why the specific collision
  is rejected. Previously the collision surfaced as unification errors at
  Prelude-internal source lines, blaming functions the user never wrote
  (`in clause 2 of 'assert'` at `15:8` for a redefined `error`) — or worse:
  redefining a Prelude function at its exact type (`sum`) hung the compiler,
  and redefining a name the Prelude uses internally at a compatible type
  (`map`) compiled silently and corrupted Prelude behavior (`<*>` on lists).
  Rejected are exactly the collisions that cannot work: names the Prelude's
  own implementation depends on, same-type duplicates of Prelude definitions,
  and (as a safety net) any redefinition that makes the Prelude itself fail to
  type-check — reported as the redefinition, with the Prelude-internal errors
  suppressed. Benign GHC-style shadowing keeps compiling as before: a builtin
  no Prelude code depends on (`head`) or a Prelude function redefined at a
  genuinely different type (a monomorphic `replicate` for FFI export). When
  the CLI knows the source file name, the error location is rendered as
  `at file.mll:2:1` to make clear it is in the user's file.

- `head`, `tail` chains, and `(!!)` return the element again instead of leaking
  an internal thunk (a regression introduced by the unreleased lazy-cons-heads
  change in this cycle; v0.1.2 behaves correctly, so no released version is
  affected). The lazy-cons-heads change stores an unevaluated thunk in
  the head slot of a cons cell; the runtime's `head` and list-index primitives
  returned that slot raw, violating the WHNF-return invariant every compiled
  function obeys (a function never returns an unforced thunk). A call site
  that wrapped the result in its own thunk — `print (head (tail xs))` thunks
  the print argument — then held a thunk inside a thunk, and the one-level
  `__force` handed the inner thunk out as if it were the value: `show` printed
  `(function: 0x…, False)` and arithmetic crashed with "attempt to perform
  arithmetic on a table value". `head` and `(!!)` now force the element they
  return (they are value-consumers under the head-consumption contract).
  Laziness is unchanged: only the returned element is forced —
  `head [1, error "boom"]` is still `1`, `length [error "boom"]` is still `1`,
  and infinite/self-referential lists still work. Test: lazy_head_projection.
- The constant folder now agrees with the runtime (and Haskell) on `div` and
  `mod` with negative operands. Both use floor semantics — the quotient rounds
  toward negative infinity, the remainder takes the sign of the divisor — but
  the folder used Rust's Euclidean `div_euclid`/`rem_euclid`, so a constant
  ``7 `div` (-2)`` folded to `-3` while the same expression on runtime values
  computed `-4` (and ``7 `mod` (-2)`` folded to `1` instead of `-1`): one
  expression, two answers, depending on whether the operands were literals.
  Test: div_mod_fold_runtime.
- `div` no longer runs through float division. It was emitted as
  `math.floor(a / b)`, so ``1 `div` 0`` yielded `inf` — a float silently
  flowing on as an Integer — and quotients past 2^53 were wrong (float
  mantissa: ``4611686018427387905 `div` 3`` came out 85 too small). `div` and
  `mod` now go through runtime helpers that raise a plain-language
  "divide by zero" error on a zero divisor on every host, and use Lua 5.3+'s
  native integer floor division (`//`, probed via `load` so the generated file
  still parses on Lua 5.1/LuaJIT), which is exact over the full 64-bit range.
  On LuaJIT every number is already a double — including the literals — so
  exactness past 2^53 is a documented host limitation (CAVEATS.md), not
  something division can recover there. Tracker benchmark timing is unchanged
  and its decoded output byte-identical. Test: div_exact_and_zero.
- An operator in type position is now rejected with a clear parse error instead
  of being silently parsed as the unit type. `f :: (+) -> Integer` previously
  compiled — the `(+)` was read as `()`, so the signature meant something
  entirely different from what was written and `f ()` ran without complaint.
  The parser now explains that `(+)` names a function (a value) and that a type
  must be built from type names, type variables, lists, tuples, and `->`, with
  a `note:` on the GHC deviation (mata-ll has no TypeOperators).
- Non-strict semantics for arguments the callee does not demand. An argument in
  a position the callee never forces is no longer evaluated, so
  `g _ = 42; g (error "boom")` returns `42` instead of raising, and a discarded
  `g (100 `div` 0)` no longer traps. The previous "cheap argument" heuristic
  could eagerly force a possibly-bottom value — a bare variable, arithmetic over
  lazy variables, a saturated inlinable call, or a trapping `div`/`mod`/`%` —
  even in a position whose value is never used.
- Non-strict semantics now hold through function composition, list elements,
  and tuple fields. `(g . h) (error "boom")` no longer forces the error when `g`
  is non-strict, and a bottom stored in a list element or a tuple field is not
  forced until it is consumed: `length [error "boom"]` is `1`,
  `map g [error "boom"]` does not run the element, and `fst (1, error "boom")`
  is `1` / `snd (error "boom", 2)` is `2`. A cons head and a tuple field are
  suspended at construction and forced only at the point of consumption
  (arithmetic, `fst`/`snd`, a nested pattern, `show`, equality, the FFI
  boundary, …); infinite lists, lazy tails, and self-referential lists are
  unaffected. The eagerness contract now covers tuple fields and every cons-head
  construction site (list literals, both `:` arms, and self-referential lists).
  A tuple (like any constructor) is still built to WHNF eagerly — only a
  possibly-⊥ *field* is suspended — so cheap/total fields stay eager and no
  thunk wraps the always-total construction. This does carry a measured cost on
  the tuple-threaded ST hot loop of the tracker benchmark (~211s → ~331s wall on
  the dev machine, byte-identical output), dominated by suspending a state-tuple
  field whose variable is already forced on the taken path but not provably so
  at construction; recovering it needs per-field (product) demand analysis. This
  is the same correctness-over-throughput tradeoff as the ST-closure change.
- Deep tail recursion no longer overflows the stack. Recursive calls in tail
  position — direct or through `if`/`case`/`let`, and mutual recursion — now
  compile to Lua's native proper tail calls and run in constant stack. (A
  parenthesised or otherwise wrapped tail call previously defeated Lua's
  tail-call optimization.)
- `seq` now works in every application form, not just prefix. The backtick infix
  (``a `seq` b``), partial application (`seq a`), and first-class/higher-order
  uses (`foldr seq z xs`, `map (seq x) ys`) previously compiled to a call of a
  nonexistent global `seq` and crashed at runtime with "attempt to call a nil
  value"; only the curried prefix `seq a b` worked. All forms now share the same
  semantics — force the first argument to WHNF, then yield the second (forced to
  WHNF only, so its subparts stay lazy; a bottom in the first argument raises, a
  bottom in the second is not forced until the result is demanded). The prefix
  and backtick forms are lowered inline so a `seq`-strict tail-recursive second
  operand stays a proper tail call (constant stack); the other forms route
  through a runtime `__mll_seq` primitive with identical behaviour.

### Changed

- Argument and binding evaluation is now decided by a soundness-first weighing:
  **bottom is never evaluated eagerly.** An expression is evaluated eagerly
  only when the consumer is proven to force it (demand analysis) or it is
  provably total (a literal, an already-forced variable, a constructor/tuple of
  such, non-trapping arithmetic over such). See the normative "eagerness
  contract" in `doc/articles/SPEC.md`. This trades some throughput for correct
  non-strict semantics — the earlier speed came from eagerly forcing values
  that might be bottom; most of the loop throughput is recovered by the stronger
  strictness analysis below.
- Strictness analysis is now a greatest fixed point (seeded optimistically and
  shrunk to consistency), so self- and mutually-recursive parameters — notably
  tail accumulators such as `loop n acc = loop (n-1) (acc+n)` — are recognized
  as strict and no longer build a thunk chain. Runtime primitives that are
  strict by construction (the ByteString operations; the ST-array read/write/
  modify operations in their array and index arguments, with the stored value
  kept lazy) are seeded directly.

## [0.1.2] - 2026-07-12

### Fixed

- Instance-evidence resolution.
- Cheap-eagerness let-binding.
- Nested-`Just` forcing.
- Unified diagnostics.
- Constructor shadowing.
- SPEC-audit bugs B1-B5: instance contexts, FFI `Maybe` optional arguments,
  exit codegen, section composition, and `fmap`-over-`pure` in do-chains.
- `Show (Either a b)`.
- Curried let/where-bound lambda application.

### Added

- Per-constructor `as "tag"` JSON renaming.
- `LuaDict` for all-nullary enums: a string-tagged Lua boundary, with
  declaration-order `Ord`/`Enum`/`Bounded`/`Show`.

### Docs

- SPEC audit-and-fix pass.
- `LuaDict`-enum documentation across the user manual, SPEC, and HASKDIFF.

## [0.1.1] - 2026-07-10

### Fixed

- Generalized `mapM_`/`forM_`/`sequence_`.
- First-class `()`.
- Field-wise derived `Ord`, plus derived `Eq` on parameterized ADTs.

## [0.1.0] - 2026-07-10

- First crates.io release of `mllc` and `mata-ll`.
