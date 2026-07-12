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
