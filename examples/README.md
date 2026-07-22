# Examples

Canonical programs demonstrating mata-ll language features. For the broader
corpus of compiler try-outs and stress tests, see [`../experiments/`](../experiments/).

Build the compiler first (`cargo build --release -p mata-ll`); the commands
below use the in-tree binary `target/release/mll`, which auto-loads the
standard library from `lib/`.

## Showcases

- `primes_check.mll` — **lazy evaluation.** A self-referential prime sieve over
  an infinite list: `primes` is consumed (via `takeWhile`) while it is still
  being produced. Real non-strict semantics, in eight lines.

  ```bash
  target/release/mll examples/primes_check.mll -r
  ```

- `itermll.mll` + `iterdemo.lua` — **Lua FFI and lazy iterator lists.** A Lua
  host hands coroutine-backed iterators to mata-ll through `LuaIterator`
  bindings; mata-ll consumes them as lazy lists, so the host's `print` calls
  and mata-ll's `putStrLn` calls interleave — including a `take n` over an
  endless iterator. mata-ll is a typed guest in a Lua host, so calling host
  functions is first-class.

  ```bash
  target/release/mll examples/itermll.mll   # -> examples/itermll.lua
  (cd examples && lua iterdemo.lua)
  ```

- `nat_hkt.mll` — **typeclasses & higher-kinded types.** A `Tree` of kind
  `Type -> Type` with `Functor` and `Foldable` instances; one `fmap`/`foldr`
  works for any element type. ADTs, pattern matching, and type inference,
  self-checking against fixed oracles.

  ```bash
  target/release/mll examples/nat_hkt.mll -r
  ```

- `atdg.mll` — **whole-program FFI + a `contrib` library.** LZ4 decompression
  (`Lz4`, `Hex`) that decodes an embedded blob and prints an ASCII comic. These
  modules live in [`../contrib/`](../contrib/), which is *not* baked into the
  compiler, so pass `-L contrib` explicitly (the in-tree binary also finds it
  automatically, but the flag makes the command work with any `mll`):

  ```bash
  target/release/mll -L contrib examples/atdg.mll -r
  ```
