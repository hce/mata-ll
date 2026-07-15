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
`Functor`, plus three that have no GHC-Prelude equivalent: `ToJSON` and
`FromJSON` (a JSON encoder/decoder, requiring `import JSON`) and
`LuaDict` (a name-based Lua-interop representation — a name-keyed table
for a single-constructor record, a plain string for an all-nullary
enum). There is no `deriving` for `Read`, `Generic`, `Hashable`,
`NFData`, or arbitrary classes. There is no `GeneralizedNewtypeDeriving`
or `DeriveAnyClass`.

## Kinds are inferred; there are no kind annotations

mata-ll has a real kind system — data/newtype parameters, class
variables, aliases and type families all get kinds inferred from use,
every written type is kind-checked, and an instance head must match the
class variable's kind (`instance Foldable []` is well-formed,
`instance Foldable [a]` and `instance Foldable Integer` are kind
errors). Differences from GHC:

- **No kind annotations or signatures.** GHC's
  `data T (f :: Type -> Type)` and standalone kind signatures do not
  exist; the kind always comes from how the parameter is used, with
  unconstrained kinds defaulting to `Type` (the same defaulting GHC
  applies without `PolyKinds`).
- **No kind polymorphism** (`PolyKinds`): a definition cannot be
  generic over kinds.
- **Promoted constructors are kind-approximate.** DataKinds-style
  promotion exists (`'Red`, `'S n`), but promoted constructors are
  classified at kind `Type` rather than at their promoted data kind —
  mata-ll will not reject `Vec 'Red a` where GHC (tracking
  `'Red : Color` vs `n : Nat`) would.
- Kind errors are worded in plain language; GHC's equivalent is usually
  "Expecting one more argument to ..." or
  "Expected kind ..., but ... has kind ...".

## Closed type families reduce, but there is no real Nat kind (yet)

Closed type families reduce both on ground arguments and *symbolically
during unification* (over type variables), so type-level arithmetic like
a length-indexed `vappend :: Vec n a -> Vec m a -> Vec (Plus n m) a`
type-checks and stays sound. Differences from GHC:

- **Only closed families.** There are no open type families and no
  associated types; equations are matched top-to-bottom in one `type
  family … where` block.
- **No injectivity.** GHC's injective type families
  (`type family F a = r | r -> a`) do not exist; `F a ~ F b` never
  concludes `a ~ b`, and two distinct *stuck* family applications do not
  unify.
- **No promoted `Nat` kind.** Promotion classifies `'Z`/`'S` at kind
  `Type` (see the DataKinds point above), so a family over "naturals" is
  not kind-checked against a `Nat` kind — it just reduces structurally.
- **A non-terminating family is rejected**, not run forever: reduction
  is fuel-bounded and reports "type family did not terminate". GHC has a
  reduction-depth limit too (`-freduction-depth`).

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
`Data.Foldable`, `Data.Traversable`, `Control.Monad`, `ByteString`, plus
`LString`, `LMath`, `LIO`, `LOS`, `LBit`, `JSON`, `Regex`. There is no
`Data.Char` (there is no `Char` type — see above).

The lazy list functions stream properly over infinite lists, so e.g.
`takeWhile (< 100) [1 ..]` and `take 10 (filter even [1 ..])` terminate.

## Foldable and Traversable are narrower than GHC's

`Foldable` (methods `foldr`/`foldl`) and `Traversable` (method
`traverse`) exist with instances for `[]`, `Maybe` and `Either`, and
user types can declare their own instances. `length`, `null`, `elem`,
`sum`, `product`, `maximum`, `minimum`, `foldMap` and `sequenceA` are
generic over them, as in GHC. The differences:

- **No tuple instances.** GHC's `Foldable ((,) a)` (where
  `length (x, y) == 1`) does not exist: mata-ll has no
  partially-applied tuple constructor. Fold over tuple components
  explicitly.
- **`sum`/`product` are `t Integer -> Integer`.** There is no `Num`
  class (see above), so they are fixed at `Integer` instead of GHC's
  `Num a => t a -> a`.
- **`mapM`/`mapM_`/`sequence`/`forM` stay list-only** (`Monad m =>
  (a -> m b) -> [a] -> m [b]` etc.); GHC generalizes them to any
  Traversable/Foldable. Use `traverse` for non-list structures.
- **`and`/`or`/`any`/`all`/`concat`/`concatMap` stay list-only**;
  GHC's are Foldable-generic. Apply them after `toList` if needed.
- **A fold over `Either` needs the `Left` type determined.**
  `length (Right 1)` is an ambiguity error (monomorphization must pin
  the instance's type); annotate:
  `length (Right 1 :: Either String Integer)`. GHC accepts the
  ambiguous form.
- **`foldl'` stays list-only** (in `Data.List`/`Data.Foldable`).
- **`toList` is only in `Data.Foldable`**, matching GHC's Prelude
  (which does not export it either) — and keeping the name free for
  programs that define their own `toList`, since a mata-ll program
  cannot shadow Prelude names.
- **`mempty`/`mappend` (Monoid) have `String` and `[a]` instances.**
  `mappend` works on lists, but the `(<>)` operator on lists remains
  an error directing you to `(++)` (see Strings above). The
  `Semigroup`/`Monoid` *classes* and their `String`/`[a]` instances are
  now ordinary declarations in `lib/Prelude.mll` (no longer
  compiler-defined). An undetermined `mempty` is still a compile-time
  ambiguity error, as in GHC — because a class method's class constraint
  is synthesized for every source class now (see the next point). The
  `String` instance's append is the Lua string-concatenation primitive,
  because a mata-ll `String` is opaque and has no `(++)`; the list
  instances use `(++)`.
- **Class methods carry their class constraint, including
  return-position-only methods.** A user class `class Default a where
  def :: a` behaves like GHC's: using `def` where nothing determines `a`
  (no annotation, no argument, no context) is a compile-time *ambiguity*
  error, not a silent resolution or a runtime crash. A method whose
  class variable is fixed by an argument (`greet :: a -> String`)
  resolves silently as before; a use at a type with no instance is a
  compile-time "No instance" error. (Previously this ambiguity check
  existed only for the builtin classes; it now applies uniformly to
  every source-defined class.)
- **`liftA2` is the Applicative method to reach for in generic
  code.** A `f <$> x <*> y` chain routes a function *through* the
  applicative (an `f (b -> c)` intermediate); at `f = IO` the runtime
  cannot represent an action whose result is itself a function, so
  generic code written that way fails at run time where GHC's would
  work. `liftA2 g x y` keeps only fully-applied values in the
  container and works everywhere (`traverse` is built on it).

## Existentials: where-helpers are monomorphic

Existential types behave as in GHC: unpacking skolemizes the hidden
variable, escapes are rejected, constructor contexts
(`forall a. Show a => …`) are enforced at pack and unpack, and record
fields with existential types have no selector and no record update.
One divergence: mata-ll `where`-bindings are monomorphic, so a
polymorphic where-helper applied to values unpacked from two
*different* boxes is rejected — the first use pins the helper to the
first box's hidden type. GHC generalizes where-bindings and accepts
it. Inline the helper or make it a top-level function with a
signature.
