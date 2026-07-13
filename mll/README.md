# mata-ll

The command-line compiler and runner for **mata-ll** — a typed subset of Haskell
that compiles to a single, self-contained Lua file with no external dependencies.

Install (the binary is named `mll`):

```
cargo install mata-ll
```

Write some Haskell — `fib.mll`:

```haskell
fib :: [Integer]
fib = 1 : 1 : zipWith (+) fib (tail fib)

export fibonacci :: Integer -> [Integer]
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
- Added: `mll -v` / `mll --version` prints a short MIT license summary, the
  crate version, and the git commit the binary was built from, then exits (no
  source file required).

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
