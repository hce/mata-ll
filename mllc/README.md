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

Latest release — 0.1.3:

- Correctness: non-strict semantics now hold for arguments the callee does not
  demand — `g _ = 42; g (error "boom")` returns `42`. Argument and binding
  evaluation uses a soundness-first weighing in which bottom is never forced
  eagerly, trading some throughput for correct laziness.
- Correctness: `return`/`pure` are non-strict — a returned value is left unforced
  until it is demanded, matching GHC and the eagerness contract, so
  `_ <- return (error "x")` and a bare `return (error "x")` statement no longer
  raise. (A bottom returned inside `try` is caught only if it is forced there, as
  in GHC.)
- Performance: recursive calls in tail position compile to Lua proper tail calls
  (deep tail recursion runs in constant stack); a greatest-fixpoint strictness
  analysis stops tail accumulators from leaking thunks; the ByteString and
  ST-array primitives are seeded as strict in their value/index arguments.
- Correctness: `seq` now works in every application form — backtick infix
  (``a `seq` b``), partial application (`seq a`), and first-class uses
  (`foldr seq z xs`) — not just prefix. The other forms previously crashed at
  runtime calling a nonexistent global `seq`. The prefix and backtick forms are
  lowered inline so a `seq`-strict tail call stays a proper tail call.
- Correctness: prefix and partially applied `div`/`mod` now work — `div 7 2`,
  `map (div 10) xs`, and first-class uses (`foldr div z xs`) previously
  type-checked then crashed at runtime; only the backtick forms worked.

See [CHANGELOG.md](https://github.com/hce/mata-ll/blob/main/CHANGELOG.md) for the
full history.

- Website: <https://matall.org>
- Source: <https://github.com/hce/mata-ll>

Licensed under the MIT License.
