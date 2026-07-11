# Important differences to Haskell to be aware of.

## Strings and ByteStrings

Haskell has:

    type String = [Char]

Haskell discourages the use of String for most real world uses.
mata-ll doesn't use a type alias to [Char] but uses Lua's string type.
This is a good tradeoff as haskell discourages the use of String
anyway. By using Lua's string type internally, we can get speedups
compared to [Char] while also staying compatible with the Lua host
language.

Since mata-ll Strings are not lists, you cannot use the concatenate
operator (++); instead, use the Semigroup's (<>) operator, like so:

    "Hello" <> " " <> "world"

mata-ll also has a ByteString type. Only strict ByteStrings are
supported; haskell is slowly deprecating lazy ByteStrings anyway.
ByteString and String shares the same type in Lua, string. string in
Lua is really a ByteString with no encoding awareness.

A proper Text type in mata-ll is planned for later.

## No Char type

There is no `Char` type or character literals (`'a'`). Characters are
represented as their integer code points. Use `strByte` to read a
character code from a string and `strChar` to convert a code back to a
single-character string.

## Integers are fixed-width, not arbitrary precision

Haskell's `Integer` is arbitrary precision. mata-ll's `Integer` maps to
Lua's integer type: 64-bit signed on Lua 5.4+ and LuaJIT. Overflow
wraps silently. If you need big integers, you'll have to implement them
yourself.

`Number` maps to Lua's float type (double-precision IEEE 754).

## No Num, Integral, or Fractional typeclasses

Arithmetic operators (`+`, `-`, `*`) are builtin, not dispatched through
typeclasses. You cannot define `(+)` for your own types. Numeric
literals are always `Integer` or `Number` — there is no polymorphic
`fromInteger`.

## All top-level bindings require type signatures

Haskell infers types for top-level bindings. mata-ll requires explicit
type signatures on every top-level definition:

    -- Haskell: fine without a signature
    -- mata-ll: this won't compile without the line below
    double :: Integer -> Integer
    double x = x * 2

## Import renaming requires `qualified`

mata-ll supports `import Module`, `import Module (foo, bar)`,
`import Module hiding (foo)`, and `import qualified Module as M` (used as
`M.foo` — the `as` is required, unlike Haskell where it is optional). The
one form it does *not* support is an unqualified rename: `import Module as M`
without `qualified` is a parse error. Names from a plain (non-qualified)
import go directly into scope.

## HashMap instead of Data.Map

There is no ordered `Map` type. Use `HashMap k v`, which is backed by
Lua tables. The API uses standalone functions (`hashmap_lookup`,
`hashmap_insert`, etc.) rather than a typeclass-based interface.

## No lazy I/O

There is no `hGetContents`, `readFile` returning a lazy String, or
similar. All I/O is strict. Use `fileLines` to read a file as a strict
list of lines, or `fReadLine` to read line-by-line in a loop.

## Error handling is string-based

There is no exception type hierarchy. `error` throws a string,
`try` catches it as `Either String a`. There are no custom exception
types, `SomeException`, `IOException`, or `catches`.

## No deriving for all classes

`deriving` supports `Show`, `Eq`, `Ord`, `Enum`, `Bounded`, and
`Functor`. There is no `deriving` for `Read`, `Generic`, `Hashable`,
`NFData`, or arbitrary classes. There is no `GeneralizedNewtypeDeriving`
or `DeriveAnyClass`.

## Monomorphization instead of dictionary passing

mata-ll compiles polymorphic functions by specializing them for each
concrete type they're used at (monomorphization), rather than passing
typeclass dictionaries at runtime like GHC. This means:

- No runtime cost for typeclass dispatch (specialized code is emitted)
- Polymorphic recursion needs special handling (the compiler falls back
  to dictionary passing when it detects it)
- Code size can grow if a polymorphic function is used at many types

## The Prelude is a curated subset

The auto-imported `Prelude` is a small, hand-maintained subset
(`lib/Prelude.mll` plus a few compiler builtins), not the full GHC `Prelude`.
The list functions it provides without any import are:

    map  filter  foldl  foldr  length  reverse  head  tail  elem
    take  drop  takeWhile  dropWhile  zipWith  concatMap
    null  last  init  concat  span  zip  unzip  replicate  iterate
    and  or  any  all  sum  product

`Data.List` (explicit `import Data.List`) adds the less common helpers and
re-exports the Prelude ones, so existing imports keep working:

    append  break'  find  unfoldr  scanl  scanr  intersperse  intercalate
    partition  nubBy  groupBy  sortBy  foldl'

Other bundled modules, also explicitly imported: `Data.Map`, `Data.Maybe`,
`Control.Monad`, `ByteString`, plus `LString`, `LMath`, `LIO`, `LOS`, `LBit`,
`JSON`, `Regex`. There is no `Data.Char` (there is no `Char` type — see above).

The lazy list functions stream properly over infinite lists, so e.g.
`takeWhile (< 100) [1 ..]` and `take 10 (filter even [1 ..])` terminate.
