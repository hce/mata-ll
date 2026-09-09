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

Latest release — 0.1.8:

- Haskell parity: pattern guards and `let` qualifiers in guards (Haskell
  2010 §3.13); type signatures on `where`/`let`/do-`let` bindings;
  `fromIntegral` and `floor`/`ceiling`/`truncate`/`round` (half to
  even); `Monad (Either e)`; GHC's mutual defaults for user
  Functor/Applicative/Monad and Eq/Show instances; `<>` concatenates
  lists; `Data.IORef`.
- Performance: eleven optimization rounds against a speed-of-light
  benchmark suite — persistent diff+reroot HashMaps, list-pipeline
  fusion into single twin-shape loops, Knuth-D Integer division,
  closure-free thunks, site-forced calling conventions; several
  workloads now run within ~1x of hand-written Lua under LuaJIT.
- Correctness: a differential program corpus — 25 whole programs run
  under real GHC and byte-compared on three Lua interpreters — plus
  review rounds closed miscompiles in newtype constructors,
  dead-type-variable specialization, NaN and `-0.0` map keys,
  mid-enumeration HashMap reroots, and lazy Prelude folds.
- Structure: hand-mirrored compiler tables reduced to one source each,
  the optimizer's strictness claims refuted after every pass, and the
  LuaJIT canary's bimodality fixed (ST reads settle their slot inline).

See [CHANGELOG.md](https://github.com/hce/mata-ll/blob/main/CHANGELOG.md) for the
full history.

- Website: <https://matall.org>
- Source: <https://github.com/hce/mata-ll>

Licensed under the MIT License.
