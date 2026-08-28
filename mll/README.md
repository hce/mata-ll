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

Latest release — 0.1.7:

- Correctness: a deep review round — four fresh-eyes passes, ~200 fixes.
  Highlights: multi-clause functions returning functions no longer drop
  arguments (eta padding is consumed, local functions are padded); the
  dictionary-passing fallback's wrong-code and wrong-rejection paths are
  closed; a tail call from one IO function to another runs in constant
  stack; action-typed value bindings re-perform instead of memoizing.
- Diagnostics: a warnings channel (a literal match without a catch-all
  warns with a witness), Maranget-matrix exhaustiveness with real
  witnesses in the error, import cycles reported as the actual chain.
- Syntax: as-patterns, the remaining `newtype` forms, hex/octal/binary
  literals and numeric underscores, first-class `($)` and `(.)`,
  multi-line import lists, infix definitions in class/instance bodies.
- Generics: `as` renames are reflected through the derived metadata
  (`selName`/`conName`), so a `deriving (Generic)` type can rename fields
  for user-written generic codecs.
- REPL: IO actions execute, embedded Lua 5.4 matches the runner.
- Prelude parity: `sortBy` is stable, `foldl'` is strict in its
  accumulator, `max`/`min` are `Ord` methods, `head`/`tail` carry GHC's
  messages.

See [CHANGELOG.md](https://github.com/hce/mata-ll/blob/main/CHANGELOG.md) for the
full history.

- Website: <https://matall.org>
- Source: <https://github.com/hce/mata-ll>

Licensed under the MIT License.
