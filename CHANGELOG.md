# Changelog

All notable changes to mata-ll are recorded here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Both crates — `mllc` (the compiler library) and `mata-ll` (the `mll`
command-line compiler and runner) — share a single version line, so each entry
below applies to both.

## [Unreleased]

### Fixed

- Non-strict semantics for arguments the callee does not demand. An argument in
  a position the callee never forces is no longer evaluated, so
  `g _ = 42; g (error "boom")` returns `42` instead of raising, and a discarded
  `g (100 `div` 0)` no longer traps. The previous "cheap argument" heuristic
  could eagerly force a possibly-bottom value — a bare variable, arithmetic over
  lazy variables, a saturated inlinable call, or a trapping `div`/`mod`/`%` —
  even in a position whose value is never used.
- Deep tail recursion no longer overflows the stack. Recursive calls in tail
  position — direct or through `if`/`case`/`let`, and mutual recursion — now
  compile to Lua's native proper tail calls and run in constant stack. (A
  parenthesised or otherwise wrapped tail call previously defeated Lua's
  tail-call optimization.)

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
