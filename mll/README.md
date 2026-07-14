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
- Added: `mll -v` / `mll --version` prints a short MIT license summary, the
  crate version, and the git commit the binary was built from, then exits (no
  source file required).

See [CHANGELOG.md](https://github.com/hce/mata-ll/blob/main/CHANGELOG.md) for the
full history.

- Website: <https://matall.org>
- Source: <https://github.com/hce/mata-ll>

Licensed under the MIT License.
