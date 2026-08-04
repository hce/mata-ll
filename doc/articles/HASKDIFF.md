# Important differences to Haskell to be aware of.

## Why GHC parity — and the criterion for deviations

mata-ll tracks GHC because the alternative is inventing a language.
Haskell's design decisions, debatable as some are, have been debugged
by three decades of use; every deviation mata-ll invents is a decision
the project must then own, defend, and document forever. So GHC parity
is the primary design goal — but "parity" is two properties, and they
are not equally binding:

**Soundness against GHC.** Any program mata-ll accepts and runs
produces what GHC produces. This half is non-negotiable. It is what
makes GHC usable as a differential oracle (the golden-test suite in
`mll-tests/`), and it keeps "correct" an objective, externally
checkable property rather than an opinion. Every silent deviation
shrinks the domain where GHC can adjudicate.

**Completeness against GHC.** Every valid Haskell program is accepted.
This half is negotiable, prioritized by how common the idiom is. A
rejection is loud — the user knows they are off the map, and nothing
wrong is ever computed. Where mata-ll declines to implement something,
the rejection should say so with a `note:`.

The resulting rule: **a deviation must be loud and documented; a
silent one is a defect.** One example on each side:

- String is opaque rather than `[Char]` (next section): `++` on a
  String is a compile-time type error. A completeness gap —
  acceptable, and kept deliberately.
- An integer type wrapping at 64 bits under the arbitrary-precision name
  `Integer` would be a soundness violation — a GHC-valid program accepted
  and computing a different value with no error. mata-ll closes the hole
  the way GHC does: `Integer` is a real arbitrary-precision bignum and the
  numeric default (implicit `default (Integer, Number)`, GHC's
  `(Integer, Double)`), while `Int` is the 64-bit wrapping machine word.
  Defaulted arithmetic therefore agrees with GHC, overflow included. See
  "Integers: arbitrary-precision `Integer`, machine-word `Int`" below.

The deliberate exception category is the FFI, where mata-ll is a Lua
guest rather than a Haskell twin — e.g. `LBit` carries Lua bit
semantics by design.

## Deep thunk chains crash sooner (fixed Lua stack)

Evaluation is call-by-need, as in GHC — the laziness and strictness
idioms behave identically, so this is not a section of differences (the
contract is in SPEC.md, the machinery in DESIGN.md). The one user-visible
difference is the failure mode of a space leak. A thunk is forced by
recursing on the Lua interpreter stack, which is fixed-size, so a leaked
deep thunk chain that GHC's growable RTS stack would force to completion
(slowly, at leaked-heap cost) instead dies here with a Lua `stack
overflow` once the chain is around 10^6 deep (10^5 completes). The leak is
the same bug in both systems and takes the same fix — `foldl (+) 0 [1..n]`
wants `foldl'`, a value built lazily inside `return` wants
`` x `seq` return x `` (GHC's `return $!`) — but Lua turns it into a crash
sooner rather than a slow completion.

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
Lua is a ByteString with no encoding awareness.

A proper Text type in mata-ll is planned for later.

## No Char type

There is no `Char` type or character literals (`'a'`). Characters are
represented as their integer code points. Use `strByte` to read a
character code from a string and `strChar` to convert a code back to a
single-character string.

## Integers: arbitrary-precision `Integer`, machine-word `Int`

mata-ll has GHC's two integer types. `Integer` is arbitrary precision and
is the default for unannotated numeric literals, so a defaulted
computation cannot silently overflow — exactly GHC's model. `Int` is the
machine word: it maps to Lua's integer type (64-bit signed on Lua 5.4+ and
LuaJIT) and overflow wraps silently, precisely GHC's `Int`. Annotate a
value `:: Int` to opt into the fast wrapping word.

Numeric defaulting is GHC's implicit `default (Integer, Number)` (`Number`
is GHC's `Double`), so an unannotated literal used only in integer
arithmetic becomes `Integer` and agrees with GHC bit for bit, overflow
included. An integer literal larger than `maxBound :: Int` is an ordinary
`Integer` literal (GHC needs no annotation for it either).

`Integer` arithmetic routes to bignum runtime helpers — portable
base-2^24 limbs, exact on both Lua 5.3+ (native i64) and LuaJIT (doubles),
with no boxing observable in the language. `Int` (and `Number`) arithmetic
at a concrete type inlines to bare Lua operators. `fromInteger :: Integer
-> a` and `toInteger :: Integral a => a -> Integer` both exist, as in GHC.

`Number` maps to Lua's float type (double-precision IEEE 754).

## Numeric typeclasses: Num / Fractional / Real / Integral

The numeric hierarchy is present with GHC's signatures — `Num`
(`+ - *`, `negate`, `abs`, `signum`, `fromInteger`), `Fractional`
(`/`, `recip`, `fromRational`), `Real`, and `Integral` (`quot`, `rem`,
`div`, `mod`, `quotRem`, `divMod`, `toInteger`). `Int` and `Integer` are
`Num`/`Real`/`Integral`; `Number` is `Num`/`Real`/`Fractional`. You can
define `(+)` and the rest for your own types, and numeric literals are
polymorphic (`Num a => a` / `Fractional a => a`) with GHC defaulting
(`Integer`, then `Number`). Arithmetic at a concrete `Int`/`Number` type
inlines to bare Lua operators — the classes are erased, not
dictionary-dispatched; `Integer` arithmetic instead routes to bignum
runtime helpers.

The `(^)` operator (exponentiation by squaring over `Num`'s `*`, so it is
exact at `Integer`) fixes its exponent to `Int`: GHC's is
`(Num a, Integral b) => a -> b -> a`, mata-ll's is `Num a => a -> Int -> a`.
A negative exponent is an error, as in GHC.

Deliberate deviations. Because mata-ll has no `Rational` type,
`fromRational` takes a `Number` argument rather than `Rational`, and
`Real` has no `toRational` method. `Floating` and `RealFrac` are not yet
classes (their operations exist as `Number`-typed functions). See CAVEATS.

## All top-level bindings require type signatures

Haskell infers types for top-level bindings. mata-ll requires explicit
type signatures on every top-level definition:

    double :: Int -> Int
    -- Haskell: the line below compiles fine without the signature above
    -- mata-ll: the line below does not compile without the signature above
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
Lua tables. The API uses standalone builtin functions (`hmLookup`,
`hmInsert`, `hmFromList`, `hmMember`, etc.) rather than a
typeclass-based interface. (The `hashmap_*` names visible in emitted
Lua are runtime internals, not source names.) For the familiar
`Data.Map` spelling, `import qualified Data.Map as M` provides
`M.lookup`, `M.insert`, and friends as a thin wrapper over `HashMap`
— the import must be qualified; a plain `import Data.Map` is rejected
because its names clash with the Prelude.

## No lazy I/O

There is no `hGetContents`, `readFile` returning a lazy String, or
similar. All I/O is strict. Use `fileLines` to read a file as a strict
list of lines, or `fReadLine` to read line-by-line in a loop.

## `getLine` matches GHC, except EOF is a string error

`getLine :: IO String` is available from the Prelude with no import,
as in GHC: it reads one line from stdin without the trailing newline.
(It was previously absent; `LIO`'s `readLine` was the only console
input.) The one deviation is at end of input: GHC throws an
`IOException` satisfying `isEOFError`, but mata-ll has no exception
type hierarchy (see below), so `getLine` raises the string error
`Prelude.getLine: end of input` — catchable with `try`/`catch` like
any other error.

## Error handling is string-based

There is no exception type hierarchy. `error` throws a string,
`try` catches it as `Either String a`. There are no custom exception
types, `SomeException`, `IOException`, or `catches`.

## No deriving for all classes

`deriving` supports `Show`, `Eq`, `Ord`, `Enum`, `Bounded`,
`Functor`, and `Generic`, plus three that have no GHC-Prelude
equivalent: `ToJSON` and `FromJSON` (a JSON encoder/decoder, requiring
`import JSON`) and `LuaDict` (a name-based Lua-interop representation —
a name-keyed table for a single-constructor record, a plain string for
an all-nullary enum). There is no `deriving` for `Read`, `Hashable`,
`NFData`, or arbitrary classes. There is no `GeneralizedNewtypeDeriving`
or `DeriveAnyClass`.

## Generics: `Data.Generics`, not `GHC.Generics`

Datatype-generic programming is available via `import Data.Generics`
and `deriving (Generic)`, but the representation deviates from
`GHC.Generics` in surface details (the semantics — a sum of products
with datatype/constructor/selector metadata, and `from`/`to`
conversions — match). The module is `Data.Generics`; the metadata is
carried by three distinct wrapper constructors `D1`/`C1`/`S1` rather
than one index-tagged `M1 i c f` (so a generic instance dispatches on
the wrapper's head under mata-ll's head-keyed instance resolution);
`K1` carries no `R` index; the combinators are of kind `Type`, so
`from`/`to` drop the phantom parameter of GHC's `Rep a x`; there is no
`V1`, and a type with no constructors cannot derive `Generic`.
`deriving (Generic)` is for concrete (parameterless) types. `conName`
and `selName` reflect a constructor's / field's *effective* external
name (the `as "…"` rename when present, the source name otherwise).

The JSON codecs are built on this substrate: `deriving (ToJSON)` and
`deriving (FromJSON)` derive the Generic representation implicitly and
their instances run the JSON module's generic encoder/decoder
(`genericToJSON :: (Generic a, GEncode (Rep a)) => a -> Json`,
`genericFromJSON :: (Generic a, GDecode (Rep a)) => Json -> Either
String a`), which are also usable directly. In GHC this is aeson's
`genericToJSON`/`genericParseJSON`; the wire format mirrors aeson's
defaultOptions where mata-ll can (see SPEC).

A performance note, not a semantic one: a polymorphic function is
specialised at up to 16 distinct types; past that the monomorphiser
switches it (and the generic-instance methods it uses) to dictionary
passing, which stays correct at any number of types but pays a runtime
dictionary indirection where the first 16 got fully specialised code.

## Orphan instances: only the top-level module is checked

An orphan instance (for a class and a type both defined elsewhere) is
rejected only in the main module. Imported modules — the stdlib and user
libraries — may declare instances for types they do not define (as the
`JSON` module does for `Int`, `[a]`, …). mata-ll compiles the whole
program together, so there is no cross-build incoherence for the rule to
guard against in library code; the check still catches a rogue instance
in the program's own module.

## Type operators

Infix type operators whose names begin with `:` are supported, both in
declarations (`data (:+:) a b = L1 a | R1 b`) and in use (`f :+: g`),
grouping by their declared fixity. This exists chiefly for the generic
representation combinators (`:+:`, `:*:`). Non-`:` type operators (a
`TypeOperators`-style `data a + b`) are not supported.

## Kinds are inferred; there are no kind annotations

mata-ll has a real kind system — data/newtype parameters, class
variables, aliases and type families all get kinds inferred from use,
every written type is kind-checked, and an instance head must match the
class variable's kind (`instance Foldable []` is well-formed,
`instance Foldable [a]` and `instance Foldable Int` are kind
errors). Differences from GHC:

- **No kind annotations or signatures.** GHC's
  `data T (f :: Type -> Type)` and standalone kind signatures do not
  exist; the kind always comes from how the parameter is used, with
  unconstrained kinds defaulting to `Type` (the same defaulting GHC
  applies without `PolyKinds`).
- **No kind polymorphism** (`PolyKinds`): a definition cannot be
  generic over kinds.
- **Promoted data types have real kinds** (DataKinds). A parameterless
  data type promotes to a kind named after it: `data Nat = Z | S Nat`
  gives the kind `Nat` with `'Z :: Nat` and `'S :: Nat -> Nat`, and the
  builtin `Bool` promotes too (`'True`/`'False :: Bool`). An index is
  checked to be exactly that kind, so `Vec 'True Int` is a kind
  error (`'True :: Bool`, but the index has kind `Nat`) — as in GHC.
  Limitations: only *parameterless, non-GADT* data types promote (a
  parameterised type would need kind polymorphism, which mata-ll lacks —
  such a type keeps the `Type` approximation for its promoted
  constructors); and, because there is no kind-signature syntax, a NON-
  GADT phantom parameter's kind defaults to `Type`, so a promoted tag of
  another kind cannot index it (`data Tagged a = Tagged Int` cannot be
  used as `Tagged 'Red` — GHC rejects that too without a `(a :: Color)`
  kind signature; pin the index via a GADT constructor return type
  instead, as `Vec`/`Light`/`Input` do).
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
- **Family argument kinds are checked** now that promoted data types
  have real kinds: a family over naturals is inferred at
  `Nat -> … -> Nat` and applying it to a `Bool` tag (`Plus 'True 'Z`) is
  a kind error. Reduction itself is unchanged — it still matches promoted
  constructors structurally.
- **A non-terminating family is rejected**, not run forever: reduction
  is fuel-bounded and reports "type family did not terminate". GHC has a
  reduction-depth limit too (`-freduction-depth`).

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
`LString`, `LMath`, `LIO`, `LIOLinear` (linear `%1` file handles, in
the style of linear-base's `System.IO.Resource`), `LOS`, `LBit`,
`JSON`, `Regex`. There is no `Data.Char` (there is no `Char` type —
see above).

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
- **`sum`/`product` are `(Foldable t, Num a) => t a -> a`,** as in GHC
  (they used to be fixed at `Int` before the numeric classes
  were added).
- **`mapM`/`mapM_`/`sequence`/`forM` stay list-only** (`Monad m =>
  (a -> m b) -> [a] -> m [b]` etc.); GHC generalizes them to any
  Traversable/Foldable. Use `traverse` for non-list structures.
- **`and`/`or`/`any`/`all`/`concat`/`concatMap` stay list-only**;
  GHC's are Foldable-generic. Apply them after `toList` if needed.
- **A fold over `Either` needs the `Left` type determined.**
  `length (Right 1)` is an ambiguity error (monomorphization must pin
  the instance's type); annotate:
  `length (Right 1 :: Either String Int)`. GHC accepts the
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

## Linear types match GHC's `LinearTypes`

mata-ll implements GHC's linear arrows. A function arrow may carry a
*multiplicity*: `a %1 -> b` promises the function consumes its argument
*exactly once*, while a plain `a -> b` (spelled `a %Many -> b` or
`a %'Many -> b`) is unrestricted. A signature may quantify over a
multiplicity variable — `apply :: (a %m -> b) -> a %m -> b` — which each
caller instantiates. The semantics are GHC's: a second use of a `%1`
value is the double-close/double-free class of bug, *zero* uses leak the
resource, and both are compile errors. Unification is invariant, as in
GHC — a plain-arrow function is not interchangeable with a `%1` one
(`map close conns` is a type error either way).

**Scalars have no exemption — this is strict GHC parity.** A scalar
(`Int`, `Number`, `Bool`, `String`) derived from a `%1`/`%m` value —
destructured from a match, `<-`-bound, or held in a `let`/`where`
binding — is tracked exactly-once like every other alias, exactly as GHC
does. GHC has no `Movable`-style relaxation in its type system, and
neither does mata-ll: `go + go where go = useOnce t` (a scalar read
twice) is rejected, even though duplicating a scalar is operationally
harmless under the memoizing runtime. Enforcement is a dedicated usage
pass over the fully-substituted typed IR (0/1/ω use counting), not a
re-threading of inference; it erases completely (the emitted Lua is
byte-identical with or without the annotations).

The deviations that remain are all in the **reject** direction — mata-ll
rejects some programs GHC accepts, never the reverse — except one
accept-direction relaxation that is sound under the lazy runtime, and
the FFI trust boundary:

- **FFI boundary trust (deliberate — the standing exception to parity).**
  The Lua side of a `%1` FFI declaration is trusted: mata-ll charges the
  argument once per call and cannot see whether the host actually
  consumes it. FFI is one of mata-ll's few deliberate departures from
  parity in general, and this is its linear-types instance.
- **FFI exports must use marshallable types — like GHC.** mata-ll rejects
  an `export` whose signature crosses the Lua boundary with a type that
  has no marshalling: a polymorphic type variable, a class-constrained
  type, a region-scoped `ST` handle, or an `IO`/`LuaIO` action in
  argument position (see CAVEATS). This mirrors GHC, whose `foreign
  export` likewise admits only marshallable (FFI/`Storable`) types and
  rejects polymorphic or class-constrained foreign signatures; the
  concrete allowed set differs (mata-ll marshals lists, tuples, `Maybe`,
  records and ADTs structurally, where GHC would require `Storable`
  wrappers), but the principle — no polymorphism, no dictionaries, no
  unrepresentable handles across the boundary — is the same.
- **Unannotated `let`/`where` that is never forced charges zero uses
  (accept direction, operationally sound).** GHC's typing rule for an
  unannotated `let`/`where` charges its right-hand side unconditionally,
  so GHC rejects `let u = t in useOnce t` (with `t` linear) even though
  `u` is never forced. mata-ll's use-count scaling *accepts* it: under
  the lazy runtime the thunk never runs, so nothing is consumed
  twice and nothing leaks. This is more permissive than GHC, but it does
  not admit a double-use or a leak at run time — it reflects mata-ll's
  laziness where GHC's rule is syntactic.
- **Conservative over-rejections (reject direction).** A wildcard over a
  *tainted scalar scrutinee* (`case useOnce t of 0 -> …; _ -> …`) is
  rejected by the blanket wildcard rule, although forcing the scrutinee
  to compare literals would in fact consume it — replace the `_` with a
  variable pattern and use that binder. Discarding a non-`()` result
  built from a linear value (a `>>` or `_ <-` whose result type is not
  `()`) is rejected, since the pending consumption may live in that
  never-forced result. A record update over a tainted record is rejected
  outright. An operator whose declared type uses `%1`/`%m` arrows is
  charged `%1` only when *both* operand arrows are literally `%1`; a rigid
  `%m` operand arrow charges ω.

Because the only accept-direction difference is the operationally-sound
lazy-`let` case, there is **no remaining accept-direction gap** where a
double-use or a leak slips past the checker.
