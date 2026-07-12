# Changelog

All notable changes to mata-ll are recorded here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Both crates — `mllc` (the compiler library) and `mata-ll` (the `mll`
command-line compiler and runner) — share a single version line, so each entry
below applies to both.

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
