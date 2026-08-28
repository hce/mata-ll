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
