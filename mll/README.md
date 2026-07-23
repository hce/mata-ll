# mata-ll

The command-line compiler and runner for **mata-ll** — a typed subset of Haskell
that compiles to a single, self-contained Lua file with no external dependencies.

Install (the binary is named `mll`):

```
cargo install mata-ll
```

Write some Haskell — `fib.mll`:

```haskell
fib :: [Int]
fib = 1 : 1 : zipWith (+) fib (tail fib)

export fibonacci :: Int -> [Int]
fibonacci = flip take fib
```

Compile it to one self-contained `fib.lua`, which any Lua 5.4 or LuaJIT host can
`require`:

```
mll fib.mll        # -> fib.lua
mll -r fib.mll     # compile and run immediately
```

> Note: `cargo install mata-ll` builds a vendored Lua, so it needs a C compiler
> on the host.

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
