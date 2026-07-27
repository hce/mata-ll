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

- **Breaking: only types with a DESIGNED FFI shape may cross the boundary, in
  both directions.** The marshallability check previously accepted "any data
  type iff every field marshals", so a plain user ADT — and prelude `Either`
  (outside `LuaTry`/`LuaIOCatch`), `Ordering`, and `ExitValue` — crossed as
  MATA-LL's internal `{tag, fields…}` table, a shape with no meaning to a Lua
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
  above). Previously any `let` qualifier failed, parsed as a guard expression
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
