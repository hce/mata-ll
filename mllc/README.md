# mllc

The compiler library for **mata-ll** — a typed subset of Haskell that compiles
to a single, self-contained Lua file with no external dependencies.

`mllc` takes mata-ll source and produces Lua source. Its standard library
(Prelude, `Data.Map`, `Data.List`, `Control.Monad`, `ByteString`, `JSON`, …) is
bundled into the crate, so the compiler needs no files on disk at runtime.

```rust
use std::path::Path;

let lua = mllc::compile("main :: IO ()\nmain = putStrLn \"hi\"\n", Path::new("."), &[])
    .expect("compile")
    .lua_code;
```

For the command-line compiler and runner, install the [`mata-ll`](https://crates.io/crates/mata-ll)
crate (which provides the `mll` command).

## Changelog

Latest release — 0.1.5:

- Breaking: the integer type is now `Int`, not `Integer`. mata-ll's integer
  wraps at 64 bits — exactly GHC's `Int` — so it carries that name; there is no
  arbitrary-precision `Integer`. Writing `Integer` (or `toInteger`) is a compile
  error with a note pointing at `Int`, and a literal past `maxBound :: Int` is
  rejected rather than silently wrapped. Numeric defaulting is `default (Int,
  Number)`. Migration is mechanical: `Integer` → `Int`.
- Correctness: `show` now matches GHC byte-for-byte — string quoting/escaping,
  `,`-no-space list and tuple separators, record syntax, and a faithful
  Burger-Dybvig `Number` (`Double`) formatter, all verified against GHC 9.14.1.
- Laziness: extracting a list tail no longer forces the next spine cell, so
  spine walkers stop exactly where GHC does (`take 2 (1 : 2 : error "boom")` is
  `[1, 2]`); IO sequencing is a proper Lua tail call, so `mapM_` streams a
  million-element list in constant stack and constant memory.
- Syntax: `LuaIterator` and `LuaTry` results are written explicitly as a list
  (`LuaIterator "string.gmatch" [String]`) or `Either String a`; the old
  bare-payload shorthands are rejected at parse time with the required shape.
- Determinism: compiling the same source twice yields byte-identical Lua,
  guarded by a test; the code generator is now AST-based (malformed statements
  unrepresentable), and demand analysis removes more thunk allocations.

See [CHANGELOG.md](https://github.com/hce/mata-ll/blob/main/CHANGELOG.md) for the
full history.

- Website: <https://matall.org>
- Source: <https://github.com/hce/mata-ll>

Licensed under the MIT License.
