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

Latest release — 0.1.4:

- Types: a full kind system. Kinds (`Type`, `Symbol`, `Type -> Type`, …) are
  inferred for every type-level declaration and every written type is
  kind-checked, so an ill-kinded instance head or signature (`instance Show
  Maybe`, `Foldable Int`) is now a compile-time error with a plain-language
  explanation instead of silently miscompiling.
- Types: promoted data types now have real kinds (DataKinds), and closed type
  families reduce symbolically during unification — so a length-indexed
  `Vec (Plus n m)` type-checks, runs with correct lengths, and keeps a length
  mismatch a compile error.
- Types: `Foldable`, `Traversable`, `Semigroup` and `Monoid` are now proper
  typeclasses. `foldr`/`foldl` are Foldable methods; `length`/`null`/`elem`/
  `sum`/`product` are generic over Foldable; user types join with ordinary
  `instance` declarations. New `Data.Foldable`/`Data.Traversable` modules.
- Soundness: unpacking an existential constructor now skolemizes the hidden
  type variable, closing a hole where `coerce (MkShowBox x _) = x` coerced any
  type to any type. Constructor contexts (`forall a. Show a => …`) are enforced
  both ways.
- Correctness: a `<-`-bound result of a user action is no longer assumed
  already-forced (fixing a runtime "arithmetic on a table value" crash on a
  strict use), and `runST` forces the state thread's result to WHNF.
- Performance: the tracker benchmark recovers the JIT multiplier a 0.1.3
  regression cost it — per-field demand analysis plus redundant-force
  elimination take `HongKong_Music.it` from 338 s to 102 s (2.50× → 0.76×
  real-time) at 15 MB, decoded output byte-identical.
- Robustness: pathologically nested input and self-doubling type aliases now
  report clean depth/size errors instead of crashing or hanging the compiler;
  every Lua string literal escapes through one canonical routine; FFI target
  names are validated at compile time; and every compiled module carries
  `__MLLC_VERSION`/`__MLLC_COMMIT` provenance.

See [CHANGELOG.md](https://github.com/hce/mata-ll/blob/main/CHANGELOG.md) for the
full history.

- Website: <https://matall.org>
- Source: <https://github.com/hce/mata-ll>

Licensed under the MIT License.
