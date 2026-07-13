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

Unreleased (next release):

- Correctness: non-strict semantics now hold for arguments the callee does not
  demand — `g _ = 42; g (error "boom")` returns `42`. Argument and binding
  evaluation uses a soundness-first weighing in which bottom is never forced
  eagerly, trading some throughput for correct laziness.
- Performance: recursive calls in tail position compile to Lua proper tail calls
  (deep tail recursion runs in constant stack); a greatest-fixpoint strictness
  analysis stops tail accumulators from leaking thunks; the ByteString and
  ST-array primitives are seeded as strict in their value/index arguments.
- Correctness: `seq` now works in every application form — backtick infix
  (``a `seq` b``), partial application (`seq a`), and first-class uses
  (`foldr seq z xs`) — not just prefix. The other forms previously crashed at
  runtime calling a nonexistent global `seq`. The prefix and backtick forms are
  lowered inline so a `seq`-strict tail call stays a proper tail call.

Latest release — 0.1.2:

- Fixed: instance-evidence resolution, cheap-eagerness let-binding, nested-`Just`
  forcing, constructor shadowing, the SPEC-audit bugs B1-B5, `Show (Either a b)`,
  and curried let/where-bound lambda application.
- Added: per-constructor `as "tag"` JSON renaming, and `LuaDict` for all-nullary
  enums (string-tagged Lua boundary, declaration-order `Ord`/`Enum`/`Bounded`/`Show`).

See [CHANGELOG.md](https://github.com/hce/mata-ll/blob/main/CHANGELOG.md) for the
full history.

- Website: <https://matall.org>
- Source: <https://github.com/hce/mata-ll>

Licensed under the MIT License.
