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

- List-typed FFI arguments crossing OUT to a Lua host are now marshalled into
  plain Lua arrays. A list passed to a host function — on its own
  (`f [1,2,3]`), nested inside a `LuaDict` record field, or nested inside
  another list — reached the host as a raw mata-ll cons cell (head at `[1]`,
  the spine's tail thunk at `[2]`) instead of a 1-based array, so the host read
  `arr[1]` as the first element but `arr[2]` as the tail function and any use of
  it failed (`attempt to perform arithmetic on a function value`). This is a
  long-standing bug — list-typed FFI arguments never worked; the argument
  marshaller only descended into tuples and `Maybe` and skipped lists and
  records entirely. It is not a regression from any recent change (the released
  0.1.2 fails the same way). The argument-direction marshaller is now the dual
  of the result decoder: a list becomes an array with its elements forced and
  recursively marshalled, and a tuple's or record's lazy fields are forced in
  place (with nested lists converted), at every depth. Opaque arguments — a
  polymorphic type variable, `LuaData`, a function, a plain ADT — still pass
  through raw with a shallow force, so a fold's threaded state (including a
  tuple state) round-trips untouched exactly as before. Test:
  ffi_list_argument_marshalled_to_array; example: examples/ffi.
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
