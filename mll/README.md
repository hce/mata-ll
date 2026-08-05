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

Latest release — 0.1.6:

- Breaking: arbitrary-precision `Integer` — a real bignum this time — and it
  is the numeric default, restoring GHC's model: unannotated literals default
  to `Integer` (`default (Integer, Number)`), `Int` stays the explicit 64-bit
  machine-word type. Exact on both PUC Lua and LuaJIT; with the new `(^)`
  operator, `2 ^ 100` matches GHC byte-for-byte, and the JSON codecs
  round-trip `Integer` exactly at any magnitude.
- Generics: `deriving (Generic)` and a `Data.Generics` module — `from`/`to`,
  the representation types, and datatype/constructor/field metadata — plus
  `genericToJSON`/`genericFromJSON` in `JSON`, byte-exact library twins of the
  native codec derives. Infix type operators (`data (:+:) a b = …`) come with
  it.
- Soundness: a function body can no longer narrow its signature's type
  variables (`f :: a -> Int; f x = x` is rejected, as GHC rejects it), and a
  higher-rank argument must really be polymorphic — the program that ran
  `True + 1` at runtime no longer compiles.
- Performance: self-tail-recursive functions and self-recursive IO loops
  compile to Lua `while` loops and run in constant stack at any depth, and an
  annotation engine strips provably redundant `__force` calls from the
  generated code.
- Syntax: the full Haskell 2010 string-escape grammar, scientific-notation
  literals, `let` qualifiers in list comprehensions, and multi-line list
  literals, comprehensions and parenthesized expressions free of the layout
  rule.

See [CHANGELOG.md](https://github.com/hce/mata-ll/blob/main/CHANGELOG.md) for the
full history.

- Website: <https://matall.org>
- Source: <https://github.com/hce/mata-ll>

Licensed under the MIT License.
