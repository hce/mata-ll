MATA-LL spec.

This document is a work in progress and evolving.

Intrinsic means behavior only implementable by the compiler, not
inside MATA-LL.

Generated Lua bytecode is not shown directly but implicitly by
specifying Lua code.

Comments are done with -- just like in haskell and -- wow! -- Lua!

Our primitive data types should match the Lua ones:

    String, Int, Number, Bool

Lua tables with continuous integer keys (i.e., arrays) should be written as

    [a]

Where a is the type of the items contained inside the array.

Lua dictionaries have their own intrinsic MATA-LL type:

    intrinsic HashMap :: Type -> Type -> Type

HashMap is a compiler built-in backed by Lua tables, not a
user-defined ADT. The built-in operations are:

    hmEmpty    :: HashMap k v
    hmFromList :: [(k, v)] -> HashMap k v
    hmInsert   :: k -> v -> HashMap k v -> HashMap k v
    hmLookup   :: k -> HashMap k v -> Maybe v
    hmDelete   :: k -> HashMap k v -> HashMap k v
    hmMember   :: k -> HashMap k v -> Bool
    hmSize     :: HashMap k v -> Int
    hmKeys     :: HashMap k v -> [k]
    hmValues   :: HashMap k v -> [v]
    hmToList   :: HashMap k v -> [(k, v)]

**Key types.** A key is any `Hashable` type: the scalars (`Int`,
`Number`, `String`, `Bool`, `ByteString`) and, structurally, tuples,
lists, and `Maybe` of hashables — `HashMap (Int, Int) v` works. The two
flavors have different runtime layouts, chosen per key type at compile
time:

  1. A *scalar* key indexes the Lua table directly (value semantics).
  2. A *structural* key is a Lua table — identity semantics, so it
     cannot index directly (`hmLookup (1,2)` would never find the
     structurally-equal inserted key). Instead the compiler threads a
     type-directed injective string ENCODER: the table is keyed by the
     encoding and stores `{key, value}` entries, and the enumerating
     operations sort by the structural `compare`, so `hmKeys` and
     friends come back in Ord order. A boxed type with no structural
     ordering (e.g. `Integer`) is still not `Hashable`.

A HashMap is unordered by definition, but the enumerating operations
(`hmKeys`/`hmValues`/`hmToList`/`show`) must still be *pure functions*:
the same map has to yield the same list every time. Lua's `pairs()`
iteration order is unspecified, so relying on it would smuggle
non-determinism into the pure world. These operations therefore return
their elements sorted by key, which also makes `hmToList m` agree
element-for-element with `zip (hmKeys m) (hmValues m)`.

We support haskell's algebraic datatypes:

    data A = A String | B Int Int

This datatype is internally represented as:

    foo = A "Hello"  -- { 1 = 1, 2 = "Hello" }
    bar = B 17 23    -- { 1 = 2, 2 = 17, 3 = 23 }

We use integer indices here, where index 1
indicates the variant that is instantiated, while the subsequent
numbers reference the items.

Types only having one instance will omit the instance specification
and start with the elements immediately. Types that serve as pure
enums will translate to a Lua integer.

This works because type definitions don't change during runtime. This
works for named datatypes as well, i.e.:

    data PersonType = Human | LLM
    data Person = Person { perName :: String
                         , perFirstName :: Maybe String
                         , perAge :: Number
                         , perIsFriendly :: Bool
                         , perType :: PersonType }

Instantiating a person like this

    morpheus = Person { perName = "Morpheus", perFirstName = Nothing
                      , perAge = 4.2, perIsFriendly = True
                      , perType = LLM }

Would translate to:

    local morpheus = { 1 = "Morpheus", 2 = nil, 3 = 4.2, 4 = true, 5 = 2 }

Record update syntax creates a copy with specific fields changed:

    olderMorpheus = morpheus { perAge = 5.0 }
    renamedMorpheus = morpheus { perName = "Neo", perFirstName = Just "The One" }

In both record construction and record update, the opening brace may
sit on a line of its own below the constructor or record expression,
provided it is indented strictly past the enclosing layout block's
column (the same continuation rule application arguments use):

    morpheus = Person
        { perName = "Morpheus", perFirstName = Nothing
        , perAge = 4.2, perIsFriendly = True
        , perType = LLM }

Chained updates may also break the line between braces, matching
GHC's postfix grammar.

And also newtype:

    newtype A = A Int

In order to make it easier to interact with plain Lua, the prelude
defines:

    data Any = AnyString String | AnyInt Int
             | AnyNumber Number | AnyBool Bool | AnyNull

`Any` converts to and from a plain Lua scalar at the FFI boundary, so a
host that has no static type for a value can hand it over — and read it
back — without ever seeing the tagged ADT. A host scalar crossing IN is
tagged by its Lua type (a string becomes `AnyString`, an integer-valued
number `AnyInt`, a fractional number `AnyNumber`, a boolean `AnyBool`,
and `nil` `AnyNull`); an `Any` crossing OUT is untagged back to its bare
scalar (`AnyNull` becomes `nil`). A value that is neither a scalar nor
`nil` — a table, function, or userdata — cannot cross as `Any` and fails
at the boundary with a localized error, since `Any` models only scalar
Lua values.

## GADTs

In addition to standard algebraic datatypes, MATA-LL supports
Generalized Algebraic Data Types (GADTs). GADTs allow each constructor
to specify its own return type, refining the type variable:

    data Expr a where
        LitI :: Int -> Expr Int
        LitB :: Bool -> Expr Bool
        Add  :: Expr Int -> Expr Int -> Expr Int
        If   :: Expr Bool -> Expr a -> Expr a -> Expr a

Pattern matching on a GADT constructor introduces local type
equalities into scope for that branch. For example:

    eval :: Expr a -> a
    eval (LitI n)     = n
    eval (LitB b)     = b
    eval (Add x y)    = eval x + eval y
    eval (If c t e)   = if eval c then eval t else eval e

In the `LitI` branch, the compiler knows `a ~ Int`, so returning
`n :: Int` as `a` is valid. In the `LitB` branch, `a ~ Bool`.
This refinement is purely compile-time; the runtime representation is
identical to standard ADTs (tag at index 1, fields after). The above
could translate to:

    local eval = function(e)
        if e[1] == 1 then return e[2]              -- LitI
        elseif e[1] == 2 then return e[2]           -- LitB
        elseif e[1] == 3 then
            return eval(e[2]) + eval(e[3])          -- Add
        elseif e[1] == 4 then                       -- If
            if eval(e[2]) then return eval(e[3])
            else return eval(e[4]) end
        end
    end

GADTs require explicit type signatures on functions that pattern match
on them. This follows naturally from the rule that all top-level
definitions must have signatures, and is necessary because GADT
return types cannot be inferred by Hindley-Milner alone — the
bidirectional checker uses the known signature to validate the type
equalities introduced by each branch.

## Existential types

Data constructors can quantify type variables that do not appear in
the result type, hiding the concrete type behind an interface:

    data ShowBox = forall a. MkShowBox a (a -> String)

    showIt :: ShowBox -> String
    showIt (MkShowBox x f) = f x

The `forall a.` in the constructor declaration makes `a` existential:
it is chosen at construction time and hidden from consumers. Pattern
matching on `MkShowBox` brings `a` back into scope locally, and it
must not escape the branch.

Unpacking SKOLEMIZES: each match on an existential constructor turns
the hidden variable into a fresh rigid type constant. Inside the match
it unifies only with itself — `coerce (MkShowBox x _) = x` with any
declared return type is rejected, as is any use that would need a
concrete type (`x + 1`). A skolem appearing in any type that outlives
the match (the function's own type, a `case` expression's result, a
`where`-function's type) is an escape error. Every diagnostic that
mentions a skolemized variable carries a note naming the constructor
that hid it. Two unpackings of the same constructor yield two
*different* skolems: values from two boxes never share a type.

Constructors can declare class constraints for the hidden type:

    data Showable = forall a. Show a => Showable a

The constraint is checked in both directions. Packing (`Showable 42`)
must prove the instance for the concrete type — the only moment it is
still known. Unpacking makes exactly the declared classes (and their
superclasses) available on the skolem, and nothing else: `show x` works
inside a match on `Showable x`; `x + 1` does not.

At runtime the constructor carries the evidence, as in GHC: packing
stores the class dictionaries for the hidden type (one hidden trailing
field per declared class and superclass), a match binds them back, and
every method use at the hidden type — direct (`show x`, `a < b`),
through a container (`show [x]`), or through a constrained helper
(`f :: Show b => b -> String` applied to `x`) — dispatches through the
captured dictionary. The monomorphizer performs the capture
(`mono.rs`, `ExCtor`); no other representation detail leaks.

GADT syntax declares existentials implicitly: a signature variable that
occurs in the constructor's fields but not in its result type is
existential (`MkBox :: Show a => a -> Box` hides `a`, with the context
travelling along). The data header's parameter names are only arity
markers in GADT syntax, so this holds even for a field variable that
happens to share a header name.

Record syntax combines with existentials, with two GHC-matching
restrictions: a field whose type mentions the hidden variable has no
selector function, and cannot be targeted by record update (see
CAVEATS.md). Construction, positional matching, and the other fields'
selectors are unaffected.

Runtime representation is identical to normal ADTs. The type erasure
is purely compile-time.

## Function application

Normal functions are defined like so:

    fun :: From -> To

Operators are functions with two arguments. Operators can be applied
like this:

    1 + 2

Or like this:

    (+) 1 2

Functions can be turned into operators by single quoting them like
this:

    1 `add` 2

The tuple constructor is likewise available as a prefix function: `(,)`
has type `a -> b -> (a, b)`, `(,,)` has type `a -> b -> c -> (a, b, c)`,
and so on. Like any function it may be partially applied — `map ((,) k)`
pairs each element with `k`.

Operators and functions may also be *defined* in infix position, with
the name between its two operands, in addition to the prefix form:

    a |+| b = a + b      -- same as (|+|) a b = a + b
    x `add` y = x + y    -- same as add x y = x + y

This infix definition form applies to top-level and local
definitions. Inside a typeclass `instance`, however, an operator
method must currently be defined in prefix form — `(#+#) (P a) (P b)
= ...`, not `(P a) #+# (P b) = ...`; the latter is a parse error.

Multi parameter functions can be partly applied, just like in haskell.

In order to support this efficiently, we do two optimizations below
the surface:

Functions with more than one parameter can be compiled to Lua
functions with multiple parameters. Functions not specified as
exported have no guaranteed representation on the Lua bytecode side.
When called from MATA-LL with all parameters specified, the compiler
can translate that call directly into a multi-parameter Lua function
call. Functions may also be inlined at the compiler's discretion.

Function application with multiple parameters where not all parameters
are specified should also be supported.

An example implementation would be this:

    add :: Int -> Int -> Int
    add a b = a + b

Could translate to

    local add = function(a, b) return a + b end

When called as

    add 1 2

That call could translate to

    add(1, 2)

When witing:

    inc = add 1

That could translate to:

    local inc = function(a) return add(1, a) end

Since types are thorougly followed throughout this, the following
should be possible with no extra effort on the compiler side:

    add a = (+) a
    add = (+)
    add a b = (+) a b

Before we can define FFI functions we need to define typeclasses, type families
and monads:

To define a typeclass:

    class X T where
        fun :: T -> Int
        (+) :: T -> T -> Int

Since we don't want to support full haskell, for now we only support
typeclasses with a single type argument for now.

# Linear types (multiplicity arrows)

A function arrow carries a *multiplicity* describing how the function
consumes its argument. This is GHC's `LinearTypes`:

    close :: Conn %1 -> IO ()      -- consumes its argument EXACTLY once
    dup   :: Conn %Many -> …       -- unrestricted (also written %'Many)
    id'   :: a -> a                -- a plain arrow is %Many

`%1` is the *linear* arrow: the function must consume its argument
exactly once — using it a second time is the double-close/double-free
class of bug, and using it *zero* times leaks the resource. Both are
compile errors. `%Many` (equivalently a plain `->`) places no
restriction. A signature may also quantify over a multiplicity
*variable*:

    apply :: (a %m -> b) -> a %m -> b

`%m` is chosen by each caller: applied to a `%1` function the argument
stays linear through `apply`; applied to an unrestricted one it is
unrestricted. Inside `apply`'s own body `m` is rigid and the binder is
held to exactly-once, because a caller may instantiate `m = 1`.

Multiplicity is invariant under unification (`%1` ≠ `%Many`, as in GHC):
a plain-arrow function is not interchangeable with a `%1` one, so
`map close conns` is a type error. The multiplicity lives only on the
arrow type; two arrows that differ *only* in multiplicity are otherwise
the same type for every map key, cache and comparison — only the
unifier and the usage checker read the slot.

## Enforcement: the usage checker

Linearity is enforced by a dedicated *usage pass* over the final,
fully-substituted typed IR of each function — it is **not** threaded
through unification. The pass counts uses per variable on a 0/1/ω
lattice: sequential composition adds, the alternatives of a
`case`/`if`/guard take the per-variable maximum (one use in every branch
is still one use, because only one branch runs), and each use is scaled
by what its context may do with it — an unrestricted call or a closure
capture charges ω, a constructor field or an IO/ST/Maybe-bind
continuation charges one, and a `let`/`where` right-hand side is scaled
by how often its bound name is used.

Because a `%1` value must be consumed *exactly* once, the pass enforces a
lower bound as well as an upper one: a tracked binder that is absent from
its scope's usage at a check point (lambda exit, case-branch exit,
bind-frame unwind, clause end) was consumed zero times and leaks; and a
binder consumed in only some alternatives of a branch group leaks on the
path through a non-consuming alternative. Aliases inherit the obligation:
a pattern binder from a match on a `%1` value, a `<-` binder from an
action that consumed one, and a `let`/`where` binding whose right-hand
side did are each tracked exactly-once themselves.

Under laziness, the `let`/`where` scaling is the key rule: a thunk's
*force* is memoized (it runs at most once), but the value it yields is
consumed once per use of the binder, so the binder's use count — not the
force count — is the sound bound. A binding that is never forced
contributes zero, which then trips the leak check.

## Scalars: strict parity, no exemption

A scalar (`Int`, `Number`, `Bool`, `String`) derived from a `%1`/`%m`
value is tracked exactly-once like every other alias — there is no
`Movable`-style relaxation. Duplicating a scalar is operationally
harmless under the memoizing runtime, but GHC's type system has no scalar
exemption and neither does mata-ll: `go + go where go = useOnce t` is
rejected. The single exempt case is a `()`-typed derived result, which
the run-for-effect idiom (`close c >> …`) discards by design.

## Erasure

Multiplicities are a type-checking discipline only. After checking, the
backend ignores the multiplicity entirely and the emitted Lua is
byte-identical to the same program written with plain arrows. Nothing
about linearity survives into the generated code.

## Boundary

Every deviation from GHC's verdict is in the *reject* direction — mata-ll
may reject a program GHC accepts, but does not accept a double-use or a
leak GHC rejects — with two exceptions:

- The Lua side of a `%1` FFI declaration is **trusted**: the argument is
  charged once per call, and what the host does with it is not visible
  (FFI is a deliberate parity exception in general).
- An unannotated `let`/`where` binding that is never forced charges zero
  uses, which is *more permissive* than GHC's rule (GHC charges its
  right-hand side unconditionally). This is sound under the lazy runtime
  — the thunk never runs — so it admits no run-time double-use
  or leak; it reflects mata-ll's laziness where GHC is syntactic.

Conservative (reject-direction) over-approximations: a wildcard over a
tainted scalar scrutinee, a discarded non-`()` result built from a linear
value, a record update over a tainted record, and an operator whose
declared arrows are not both literally `%1`.

# Kinds

MATA-LL has a full kind system. Kinds classify types the way types
classify values:

    Type          -- complete types (Int, Bool, Maybe String, ...)
    Symbol        -- type-level strings, used in FFI declarations
    k1 -> k2      -- type constructors (Maybe : Type -> Type,
                  --   Either : Type -> Type -> Type,
                  --   [] : Type -> Type)

There is no surface syntax for kinds: every kind is INFERRED from use,
and anything left unconstrained defaults to `Type` (GHC's Haskell-2010
kind defaulting). There is no kind polymorphism.

- A `data`/`newtype` parameter's kind comes from how the parameter is
  used in the fields: `data Wrap f = Wrap (f Int)` gives
  `Wrap : (Type -> Type) -> Type`. Mutually recursive declarations are
  solved together.
- A class variable's kind comes from the method signatures:
  in `class Foldable t where foldr :: (a -> b -> b) -> b -> t a -> b`
  the use `t a` forces `t : Type -> Type`. A constraint fixes kinds the
  same way (`Foldable t => …` makes `t : Type -> Type` in that
  signature). Superclasses must agree with the subclass's variable kind.
- Type aliases and type families get kinds from their definitions.

Every type the program writes is then kind-checked: signatures, data
fields, class methods, instance declarations, type-family equations and
expression ascriptions. Applying a complete type to an argument
(`Maybe Int Bool`), using an unsaturated constructor where a
complete type is required (`f :: Maybe -> Int`), and using one type
variable at two kinds (`g :: t -> t a`) are all compile-time kind
errors.

An instance head must have exactly the kind of the class's variable.
The bare, unapplied list constructor `[]` is valid type syntax for this
purpose:

    instance Foldable [] where ...          -- [] : Type -> Type
    instance Foldable Maybe where ...       -- Maybe : Type -> Type
    instance Foldable (Either c) where ...  -- partial application
    instance Foldable [a] where ...         -- KIND ERROR: [a] : Type
    instance Show Maybe where ...           -- KIND ERROR: Show wants Type

Promoted data types (DataKinds, see below) have REAL kinds. A
parameterless data type `T` promotes to a kind named `T`: `data Nat =
Z | S Nat` gives the kind `Nat` with `'Z :: Nat` and `'S :: Nat -> Nat`
(and the builtin `Bool` promotes, `'True`/`'False :: Bool`). An index is
checked to have exactly that kind, so `Vec 'True Int` is a kind
error (expected `Nat`, got `Bool`). Two limits: only parameterless,
non-GADT data types promote (others keep the `Type` approximation, as
promoting them would need kind polymorphism), and — with no kind-
signature syntax — a non-GADT phantom parameter's kind defaults to
`Type`, so a promoted tag of another kind must be pinned through a GADT
constructor return type (as `Vec`, `Light`, `Input` do below).

## The engage restriction (spec-level "Fn")

The `engage` intrinsic additionally constrains its result to function
types ending in `IO` (`x -> ... -> IO y`); the compiler rejects any
instantiation that does not end in `IO`, so Lua functions called
through `engage` are always treated as effectful. This restriction is
enforced by the FFI validation pass, not by the kind language above.

## LuaFunction, engage, and scope safety

`LuaFunction s` is an opaque type representing a Lua function value
passed into MATA-LL. The phantom type parameter `s` is a scope tag
that prevents the function from being used outside the invocation
in which it was received.

`LuaFunction s` is not callable directly; it must be given a
concrete type via `engage`:

    intrinsic engage :: LuaFunction s -> (... -> LuaIO s result)

The type annotation at the call site is mandatory — the compiler
cannot infer what signature a Lua function has. The annotation is
trusted; no runtime type checking is performed. If the Lua function
does not match the declared type, behavior is undefined.

## LuaIO monad

`LuaIO s a` is the monad for operations involving opaque Lua
function references. It is separate from `IO` and carries a phantom
scope parameter `s`:

    LuaIO s a    -- IO involving a Lua function from scope s
    IO a         -- normal MATA-LL IO (FFI calls, putStrLn, etc.)

`LuaIO s` is distinct from `IO`. Regular `IO` operations like
`putStrLn` can be lifted into `LuaIO s` via:

    intrinsic liftIO :: IO a -> LuaIO s a

## Scope safety via rank-2 types

When Lua calls an exported MLL function that receives a
`LuaFunction`, the compiler universally quantifies the scope
parameter at the entry point:

    export callback :: forall s. LuaFunction s -> LuaIO s ()

The `forall s.` ensures that `s` cannot escape the function body.
This means:

- The `LuaFunction s` can be stored in a data type (the `s` tags
  along), but it cannot be `engage`d later because no `LuaIO s`
  context with the same `s` exists outside the original call.
- The engaged function returns `LuaIO s result`, which is tied to
  the same scope — it cannot be returned as a plain `IO` value.

This is the same mechanism as Haskell's `ST` monad: the rank-2
type seals the scope, and the phantom parameter prevents escape.

Example:

    export processEvent :: forall s. LuaFunction s -> LuaIO s ()
    processEvent luafn = do
        let f = engage luafn :: Int -> LuaIO s Int
        result <- f 42
        liftIO $ putStrLn (show result)

    -- This would be rejected: s would escape
    -- bad :: LuaFunction s -> IO (LuaFunction s)
    -- bad f = return f   -- type error: s is not in scope

This `forall s.` scope-sealing pattern is the primary motivating use
of rank-2 quantification (the same mechanism backs `runST`). Rank-2
function arguments more generally also work — a parameter of type
`(forall a. a -> a)` may be instantiated at several types within the
body — though the scope-tag pattern above is the case the language is
designed around.

## Statically known FFI calls

The scope mechanism does NOT apply to statically known FFI calls.
These use `IO`, not `LuaIO`:

    sin :: Number -> LuaPure "math.sin" Number    -- pure, no IO
    rnd :: Number -> Maybe Number -> LuaIO "math.random" Number  -- IO, not LuaIO s

Note: the `LuaIO` type family (for FFI declarations) and the
`LuaIO s` monad (for scope-tagged Lua callbacks) share a name
prefix but are distinct. The type family `LuaIO "name" T` reduces
to `IO T` (plain IO). The monad `LuaIO s a` is a separate type
that carries the scope tag.

The parser disambiguates the two forms syntactically: if the first
argument is a string literal, it is the FFI type family; if it is a
type variable, it is the scoped monad. These are represented as
distinct AST nodes internally.

Arguments crossing OUT to the host are marshalled from the mata-ll
representation by their declared type. **The argument marshaller is a complete
structural dual of the result decoder: it descends into exactly the container
types the decoder converts, so encode-then-decode is identity at every nesting
depth.** Those containers are:

- a **list** becomes a plain 1-based Lua array (its elements recursively
  marshalled);
- a **tuple** becomes a positional table, a **`LuaDict` record** a name-keyed
  table (their lazy fields forced, nested structure converted);
- a **`HashMap`** becomes a string-keyed dict — each *value* recursively
  marshalled by the value type; keys are scalars already usable as Lua keys and
  are kept (the decoder likewise validates but does not convert keys);
- a **`Maybe`** reached through this structural descent is *unwrapped* — `Just x`
  becomes the bare `x` (recursively marshalled, so `Just [1,2,3]` is a real
  array and `Just "s"` a native string), `Nothing` becomes `nil` (an absent
  field), matching `__mll_to_lua`;
- an **`Any`** is *untagged* — the dynamic ADT's scalar payload is handed over
  bare (`AnyNull` becomes `nil`); the decoder tags a host scalar back the same
  way, so an `Any` round-trips through the host unchanged.

An **opaque** argument — a polymorphic type variable, `LuaUserData`, a function, or
a plain (non-`LuaDict`) ADT — is left raw with only a shallow force (the decoder
likewise leaves these untouched), so a value the host holds without inspecting
(a fold's threaded state) round-trips unchanged. A converted container is
rebuilt into a **fresh** Lua value, never mutating the mata-ll value, so a value
passed to a host and then reused in mata-ll code is not corrupted. Because an
FFI call is strict in its arguments, forcing what the host reads evaluates
nothing the call does not already demand. The result crossing back IN is decoded
by the dual mechanism (arrays → cons lists, host tables → records/tuples, dict
values → `HashMap` values, a present field → `Just`/a missing one → `Nothing`),
validating each declared scalar's Lua type.

(A *top-level optional positional argument* declared `Maybe` in the FFI
signature is a separate feature — "Optional parameters" — where the
`Just`/`Nothing` wrapper is instead consumed positionally: `Nothing`
omits the argument. That path keeps the wrapper until it is consumed and
only marshals the payload's structure.)

## Passing mata-ll callbacks to FFI functions

`engage` covers the *incoming* direction: Lua hands a function to
mata-ll. The *outgoing* direction — mata-ll handing one of its own
functions to a Lua host function — is supported when an FFI argument
itself has a function type. The motivating case is a fold-style host
API that calls the callback once per item and threads its return value
as the next state (e.g. a SQL driver folding over result rows):

    foldRows :: String
             -> (Int -> acc -> acc)        -- pure callback
             -> acc
             -> LuaPure "db.fold" acc

    foldRowsIO :: String
               -> (Int -> acc -> LuaIO s acc)  -- effectful callback
               -> acc
               -> LuaIO "db.fold" acc

The compiler wraps each function-typed FFI argument so the Lua host can
call it with positional arguments. The wrapper:

- applies the mata-ll callback to all of the host's arguments at once
  (mata-ll functions are n-ary, not curried);
- marshals each argument across the boundary when its type is a list or
  nested function, and otherwise passes it raw;
- runs the returned action for an effectful callback (whose result is
  `LuaIO s acc`), or takes the value directly for a pure one (result
  `acc`);
- returns the result to the host, marshalling it unless it is the
  opaque threaded state.

### The threaded state is opaque

The accumulator (`acc`) is a polymorphic type variable, so the Lua host
cannot inspect it. The wrapper therefore passes it through *raw* in both
directions rather than marshalling it. This is what lets **any** mata-ll
value — including tuples and ADTs — be used as the state and round-trip
intact: marshalling would flatten a tuple `(a, b)` to a Lua array and
rebuild it as the cons list `a : b : []`, corrupting it.

### Soundness is type-checked

When an FFI callback threads a polymorphic state, the compiler requires
it to be **one shared type variable** across four positions: the
callback's accumulator argument, the callback's result, the FFI's
initial-state argument, and the FFI's return type. Effectful callbacks
must use `LuaIO s acc` (not `IO acc`). A callback with no type variables
(e.g. `String -> String` for `string.gsub`) threads no opaque state and
is accepted without these constraints. See `experiments/ffi_fold.mll`.

## Runtime representation

Both `s` and `forall s.` are purely compile-time constructs. They
have no runtime representation. The generated Lua code for a
function receiving a `LuaFunction s` is identical to one receiving
any other argument — the scope safety is enforced entirely by the
type checker.

# Type families and intrinsics

The `intrinsic` keyword marks definitions whose equations are part of
the language spec and visible for type checking, but whose
implementation is provided by the compiler. Users are not intended to
define their own intrinsic type families. (Note: the current parser
does not reject a user-written `intrinsic type family`; it is
accepted. Rejecting it is a candidate enforcement fix.)

## Intrinsic type families

    intrinsic type family LuaPure (name :: Symbol) a where
        LuaPure _ a = a

    intrinsic type family LuaIO (name :: Symbol) a where
        LuaIO _ a = IO a

`Symbol` is the kind of type-level strings. It is opaque and only
consumed by intrinsic type families.

## User-defined type families

Users may define their own type families without the `intrinsic`
keyword:

    type family Element container where
        Element [a]           = a
        Element (HashMap k v) = v

These are closed families: equations are matched top-to-bottom, and a
pattern constructor (`[a]`, `'S n`, …) matches only an argument of that
exact shape. The compiler reduces a family application both eagerly on
ground arguments AND **symbolically during unification** — including
when arguments contain type variables — so length arithmetic works:

    data Nat = Z | S Nat
    type family Plus n m where
        Plus 'Z     m = m
        Plus ('S n) m = 'S (Plus n m)
    data Vec n a where
        VNil  :: Vec 'Z a
        VCons :: a -> Vec n a -> Vec ('S n) a
    -- type-checks: Plus 'Z m ~ m, and Plus ('S n) m ~ 'S (Plus n m)
    vappend :: Vec n a -> Vec m a -> Vec (Plus n m) a
    vappend VNil        ys = ys
    vappend (VCons x xs) ys = VCons x (vappend xs ys)

A family application whose scrutinee is still a variable no equation
matches is *stuck*: it is left irreducible and deferred, not an error.
Families are NOT assumed injective — `F a ~ F b` does not imply
`a ~ b` — so two distinct stuck applications do not unify. A
non-terminating family (`Loop x = Loop x`) is reported as a
"did not terminate" error rather than looping. The family above is
kind-checked at `Plus :: Nat -> Nat -> Nat` (see the Kinds section on
promoted data types), so `Plus 'True 'Z` is a kind error.

## FFI using type families

With the above, FFI declarations become:

    sin :: Number -> LuaPure "math.sin" Number
    rnd :: Number -> Maybe Number -> LuaIO "math.random" Number

`LuaPure` reduces to the bare return type; `LuaIO` wraps in `IO`.
The `Symbol` argument is consumed by the compiler during code
generation to resolve the target Lua function and is then erased from
the type.

A `Maybe` argument translates to an optional Lua parameter: `Nothing`
causes the argument to be omitted, relying on Lua's treatment of
missing arguments as nil.

## The intrinsic keyword

`intrinsic` may be applied uniformly to type families, typeclasses,
and functions:

    intrinsic class Monad (IO m)
    intrinsic putStrLn :: String -> IO ()

The meaning is always the same: the definition is normative spec,
visible to the user and to the type checker, but only the compiler can
provide the implementation.


Monads are just like in haskell:


    class Functor f where
        fmap :: (a -> b) -> f a -> f b
        (<$) :: a -> f b -> f a
        (<$) = fmap . const

    class Applicative m where
        pure  :: a -> m a
        (<*>) :: m (a -> b) -> m a -> m b
        (*>)  :: m a -> m b -> m b
        a1 *> a2 = (id <$ a1) <*> a2
        (<*)  :: m a -> m b -> m a
        a1 <* a2 = const <$> a1 <*> a2

    class Monad m where
        (>>=)  :: m a -> (a -> m b) -> m b
        return :: a -> m a
        return = pure
        (>>)   :: m a -> m b -> m b
        m >> k = m >>= \_ -> k

Note: of the operators shown above, `fmap`, `(<$>)`, `pure`, `(<*>)`,
`(>>=)`, `return`, and `(>>)` are provided. The secondary combinators
`(<$)`, `(*>)`, and `(<*)` are declared here as the intended interface
but are not yet available — referring to them is currently an unbound
variable.

    class Read a where
        read :: String -> a
        -- Instances: Int, Integer, Number, Bool, String

    class Enum a where
        succ        :: a -> a
        pred        :: a -> a
        toEnum      :: Int -> a
        fromEnum    :: a -> Int
        enumFrom    :: a -> [a]          -- [n..]
        enumFromThen :: a -> a -> [a]    -- [n,m..]
        enumFromTo  :: a -> a -> [a]     -- [n..m]
        enumFromThenTo :: a -> a -> a -> [a]  -- [n,m..z]

    -- Range syntax desugars to Enum methods:
    --   [1..10]   →  enumFromTo 1 10
    --   [1,3..10] →  enumFromThenTo 1 3 10
    --   [1..]     →  enumFrom 1
    --   [1,3..]   →  enumFromThen 1 3

# Type inference

MATA-LL uses a combination of Hindley-Milner unification and
bidirectional type checking.

## Annotation rule

All top-level definitions must have explicit type signatures. All
sub-expressions (let bindings, where clauses, lambda arguments, local
definitions) are inferred and do not require annotations.

    -- required: top-level signature
    mapTree :: (a -> b) -> Tree a -> Tree b
    mapTree f (Leaf x)     = Leaf (f x)
    mapTree f (Branch l r) = Branch (mapTree f l) (mapTree f r)
        where
            -- inferred: no signature needed
            go t = mapTree f t

## How the two systems interact

Bidirectional checking is used when type information is available from
context. The known signature of a top-level definition, a typeclass
method, or an FFI declaration flows inward (checking mode), pushing
expected types into subexpressions.

Hindley-Milner unification is used for local inference where no
contextual type is available. Inside a function body, let bindings and
intermediate expressions are inferred via unification without
requiring annotations.

The boundary is clean: signatures at the top provide the starting
type, bidirectional checking pushes it down, and HM fills in the
gaps locally.

## Consequences

- Typeclass method implementations are checked against the method's
  declared signature, not inferred independently.
- FFI declarations always have full signatures, giving the
  bidirectional checker a rich starting point.
- Error messages can always point to the nearest enclosing signature
  as the source of the expected type, since one is never far away.
- The compiler never needs whole-program inference.

# Conversion to Lua bytecode

## Boundaries between standard Lua and MATA-LL

The only place where plain Lua variables may pass to MATA-LL are
through FFI function calls.

Lua modules compiled from MATA-LL must not clutter the global
namespace. All definitions must be local; FFI exports must be passed
via the module's return value. An export may be a function, an IO/LuaIO
action, or a plain value. A function export becomes a callable wrapper;
an IO/LuaIO-action export becomes a wrapper that PERFORMS the action
when the host calls it; and a value export is marshalled to Lua
directly, by the SAME result contract a function's return value uses —
the same type-directed conversion, so a scalar becomes a Lua number, a
record/LuaDict a keyed table, a tuple a positional table, a `Maybe`/ADT
its tagged form, a finite list a Lua array. As on the function-result
edge, a lazy or infinite structure cannot cross the strict Lua boundary
(Lua cannot hold an unevaluated tail); this is inherent and one-way —
the import side turns a Lua iterator INTO a lazy list, but the export
side cannot hand an infinite list back out.

The signature of anything that touches the boundary — an `export`, and
equally an FFI IMPORT (`LuaPure`/`LuaIO`/`LuaTry`/`LuaIOCatch`, which
calls INTO Lua) — is checked at compile time: every type that crosses
must have DEFINED marshalling behavior — a shape the host is meant to
see — or the declaration is rejected. Direction is symmetric: an
export's ARGUMENTS cross Lua→MATA-LL (they must be *importable*) and its
RESULT crosses MATA-LL→Lua (it must be *exportable*); an import is the
mirror — its arguments cross MATA-LL→Lua and its result comes back
Lua→MATA-LL. The allowed *value* set, in both directions and
recursively, is:

  - scalars — `Int`, `Number`, `Bool`, `String`, `ByteString` (and
    the FFI aliases `Int`/`Double`/`Float`/`Char`) — and `()`;
  - the opaque `LuaUserData` interop handle;
  - `[a]` iff `a` is allowed; tuples iff every element is allowed;
  - `HashMap k v` iff `k` is a scalar Lua key and `v` is allowed;
  - `Maybe a` iff `a` is allowed — the designed optional shape (`nil` ↔
    `Nothing`);
  - `Any` — the dynamic boundary type (its runtime conversion is defined
    by the code generator);
  - a `LuaDict` record iff every declared field is allowed — it crosses
    as a name-keyed table;
  - a NEWTYPE iff its single underlying field is allowed — a newtype is
    transparent (the value IS its field, with no wrapper), so it
    inherits the field's representation; this is how
    `newtype FileHandle = FileHandle LuaUserData` and the whole `LIO`
    file API cross. A recursive newtype/record re-entry crosses opaquely;
  - `Either String a` as the result of a `LuaTry`/`LuaIOCatch` import
    only — there the `pcall` wrapper BUILDS and interprets the
    `Left`/`Right` tags, so this is a designed shape and only the inner
    `a` is checked;
  - `IO a` / `LuaIO s a` as an export RESULT (an action export) iff `a`
    is exportable.

A plain user or prelude `data` type with real constructors — a
multi-constructor and/or multi-field ADT, including `Either` (outside a
`LuaTry`/`LuaIOCatch` result), `Ordering`, and `ExitValue` — is
REJECTED even when its fields would each marshal. It has no designed FFI
shape: it would cross only as MATA-LL's internal `{tag, fields…}` table,
which has no meaning to a Lua host. To carry structured data across the
boundary, use a `LuaDict` record (a name-keyed table); for a dynamic
scalar, use `Any`; or encode the value as a scalar or a list. A newtype
over a marshallable type crosses transparently; a plain `data` type does
not.

A function (a callback) is marshallable in exactly ONE position: as a
DIRECT top-level argument of the export. There the callback's own
arguments cross MATA-LL→Lua (each must be *exportable*) and its result
crosses back Lua→MATA-LL — its `LuaIO s a` effect is unwrapped, so the
payload `a` must be *importable*, and the result must be a `LuaIO`
action (untrusted Lua functions are assumed effectful). A function
ANYWHERE ELSE is rejected: nested inside a tuple/list/`Maybe`/record, in
result position, or as a callback's OWN argument (a
callback-taking-a-callback). This is not an arbitrary restriction — it
is exactly the shape the code generator fully marshals; every other
arrow position would be passed to Lua opaque and leak.

Rejected — a compile-time error naming the binder, the position and the
direction — is anything else: a plain `data` ADT with no designed shape
(above), a bare polymorphic type variable (Lua has no representation for
a polymorphic value), a class-constrained type (a dictionary cannot
cross), a region-scoped `ST`/`STArray` handle, an `IORef` cell (it
holds a mata-ll value, possibly unevaluated), an `IO`/`LuaIO`
action in ARGUMENT position (a Lua caller cannot supply an action; only
a top-level callback returning `LuaIO` may carry an effect inward), and
a function in any non-top-level-argument position. The one designed
exception to the type-variable rule is the threaded STATE of a
polymorphic outgoing-callback FFI (the fold pattern): a single shared
variable that round-trips through Lua opaquely, whose soundness is
enforced separately.

When MATA-LL is intended to run standalone, the compiler appends an
entry-point stub to the emitted `.lua` file itself (a single
self-contained file — no separate `main.lua` is produced) that calls
into the program. See "Standalone MATA-LL" below for the exact stub.

## Function bodies in MATA-LL

The compiler is free to split up functions defined in MATA-LL to
multiple Lua chunks or functions, as long as the semantics are
unaffected.

In particular, the compiler is free to decide whether to split a large
if block into calls of sub-functions.

The compiler may inline functions whenever deemed necessary.

## Pattern matching

Pattern matching is supported for function definitions, case blocks,
and assignments. A monadic bind `pat <- action` accepts any pattern; a
refutable one desugars with GHC's MonadFail-style fallback ("Pattern
match failure in do expression at line:col"). Local-binding PARAMETERS
(`let f (a, b) = e`, where-bindings) accept any pattern too, desugaring
like pattern lambdas with the partial-function fallback. What remains
out: a constructor pattern as the assignment LHS itself (`let Just x =
m` — bind the value and match it with `case`, or use `Just x <- return
m` in do). Constructor patterns in function definitions, case
alternatives, and lambda arguments are fully supported.

Non-exhaustive function definitions and case expressions are rejected
at compile time (the type checker performs exhaustiveness checking).

NOTE: The following distinction for assignments is aspirational and
not yet implemented. We want to distinguish between "single-case
assignments" such as a let or <- assignment inside a do block. Here,
pattern mismatch should raise an error. Multi-case assignments
include where and let assignments outside of do blocks. Here, the
compiler should enforce exhaustive definitions.

Pattern matching semantics:

    data Tree a = Branch (Tree a) (Tree a) | Leaf a

    depth :: Tree a -> Int
    depth (Leaf _)          = 0
    depth (Branch a b)      = 1 + max (depth a) (depth b)

    depth (Branch (Branch (Leaf 1) (Leaf 2)) (Leaf 3))

Could translate into:


    local depth = function(t)
        if t[1] == 1 then
            -- leaf
            return 0
        elseif t[1] == 2 then
            -- branch
            return 1 + math.max(depth(t[2]), depth(t[3]))
        end
    end


# Standalone MATA-LL

The compiler looks for a declaration of main at the top level and if
it finds one, compiles the .mll file to a standalone .lua file with
a stub at the end that calls into it:


my.mll

    main :: IO ()
    main = ...

The generated .lua file ends with an entry-point stub that runs the
program only when the file is executed directly, and stays quiet when
it is loaded as a module via `require`:

    local __mll_arg1 = ...
    if __mll_arg1 == nil or (arg ~= nil and __mll_arg1 == arg[1]) then
        __mll_run(__mll_fn[N]())
    end

The test distinguishes the two invocation styles even when command-line
arguments are present: a standalone interpreter (`lua my.lua x y`)
fills both the chunk's varargs and the global `arg` table from the
same command line, so the first vararg equals `arg[1]` (or is nil with
no arguments), while `require` passes the module name as the first
vararg, which does not match `arg[1]`.

The stub invokes an internal runner over `main`'s compiled thunk
(`__mll_fn[N]` above) rather than a bare `main`, because `main` is not
declared as a function exported to Lua. The compiler can also execute
the result directly via the embedded mlua runtime when invoked with
--run.

Command line arguments are not passed to main, nor is a return value
passed back to the OS.

For both, library functions should be used:

  getArgs :: IO [String]
  exit :: ExitValue -> IO ()

  data ExitValue = Normal | Err Int

# Do-notation

Do should be desugared just like in haskell.

    main :: IO ()
    main = do
        x <- rnd 1.0 (Just 6.0)
        putStrLn (show x)
        let y = x + 1.0
        putStrLn (show y)

Desugaring: `x <- e; rest` becomes `e >>= \x -> rest`,
bare `e; rest` becomes `e >> rest`.

# Case expressions

Pattern matching on function definitions is specified. case ... of
should be handled just like in haskell.

    describe :: Tree a -> String
    describe t = case t of
        Leaf _     -> "leaf"
        Branch _ _ -> "branch"

The compiler is free to generate multiple Lua functions for
optimization or structuring.

# if/then/else

The syntax is just like in haskell:

    if cond then whentrue else whenfalse

Both in pure and monadic code. In monadic code, we offer when in
addition:

    when :: Bool -> IO () -> IO ()
    when cond what = if cond then what >> pure () else pure ()

# let/in and where

where and let semantics should be just as in haskell.

There is a "monadic let" inside do blocks and a non-monadic one. The
non-monadic one requires exhaustive pattern matching. The monadic one
requires a single path, and will raise an exception if that path
doesn't match.

Note: as stated under Pattern matching, `let`/`where` bindings today
accept only variable and tuple patterns, so the refutable-binding
semantics above apply to those forms; constructor-pattern bindings are
not yet parsed.

# Lambda syntax

If you write

    \ a b -> a + b

You get a lambda. Writing

    \a -> \b -> a + b

Shall be semantically equivalent to the first one.

Pattern matching in lambda arguments is supported.

# Guards

Guards are supported on function definitions and case branches:

    abs :: Int -> Int
    abs n | n < 0     = -n
          | otherwise = n

    classify :: Int -> String
    classify n = case n of
        0             -> "zero"
        n | n > 0     -> "positive"
          | otherwise -> "negative"

`otherwise` is defined as `True` in the prelude.

# List comprehensions

Haskell-style list comprehensions are supported with generators,
guards, and pattern-matching generators:

    evens = [x | x <- [1..20], x `mod` 2 == 0]
    pairs = [(x, y) | x <- [1..3], y <- [1..3], x /= y]
    oks   = [v | Ok v <- results]

Comprehensions desugar to `concatMap` and `filter`.

# Literals

Numeric literals are polymorphic, exactly as in Haskell. An integer
literal has type `Num a => a` (it is `fromInteger` applied to the
literal), and a decimal literal has type `Fractional a => a` (it is
`fromRational` applied to the literal):

    42    :: Num a => a
    3.14  :: Fractional a => a

At a concrete `Int` or `Number` type `fromInteger`/`fromRational`
is the identity and is erased, so `(42 :: Int)` and
`(3.14 :: Number)` still compile to the bare Lua values `42` and
`3.14`.

An unconstrained numeric literal is resolved by GHC's defaulting rule
`default (Integer, Number)` (matching GHC's `(Integer, Double)`):
the first of `Integer` then `Number` whose instances satisfy the
variable's (standard) class constraints is chosen. So `show 5` is
`show (5 :: Integer)`, while `show (5 / 2)` resolves to `Number`
(because `Integer` is not `Fractional`). A variable also constrained
by a non-standard (user) class is not defaulted — such a use is
ambiguous and needs an annotation, matching GHC.

String literals use double quotes with C-style escape sequences:

    "hello\n"
    "tab\there"
    "quote: \""

# Numeric classes

The numeric typeclass hierarchy is built in, with GHC's signatures — one
deviation: mata-ll has no `Rational` type, so `fromRational` takes a
`Number` (see CAVEATS.md, "`fromRational` takes a `Number`"):

    class Num a where
        (+), (-), (*) :: a -> a -> a
        negate, abs, signum :: a -> a
        fromInteger :: Integer -> a

    class Num a => Fractional a where
        (/) :: a -> a -> a
        recip :: a -> a
        fromRational :: Number -> a

    class (Num a, Ord a) => Real a          -- superclass marker

    class (Real a, Enum a) => Integral a where
        quot, rem, div, mod :: a -> a -> a
        quotRem, divMod :: a -> a -> (a, a)
        toInteger :: a -> Integer

`Int` and `Integer` are `Num`, `Real`, and `Integral`. `Number` is `Num`,
`Real`, and `Fractional` (`Int` and `Integer` are deliberately not
`Fractional` and `Number` is deliberately not `Integral`, exactly as GHC). You can
write ordinary polymorphic numeric code (`sum :: Num a => [a] -> a`,
`average :: Fractional a => [a] -> a`) and give a hand-written `Num`
instance to a user type (e.g. a modular-arithmetic newtype).

`quot`/`rem` truncate toward zero (the remainder takes the dividend's
sign); `div`/`mod` floor (the remainder takes the divisor's sign) —
matching GHC's negative-number semantics exactly.

Two deliberate deviations (see CAVEATS): mata-ll has no `Rational`
type, so `fromRational` takes a `Number` (the representation a decimal
literal already uses); and `Real` carries no `toRational` method for
the same reason.

# Typeclass instances


    instance Show PersonType where
        show Human = "Human"
        show LLM   = "LLM"

    instance Show a => Show (Tree a) where
        show (Leaf x)     = "Leaf " <> show x
        show (Branch l r) = "Branch (" <> show l <> ") (" <> show r <> ")"

Superclass constraints on instances? Yes.

Orphan instance rules? Disallowed.

# Typeclass dispatch strategy

Via monomorphization. The compiler generates a specialized copy of
each polymorphic function for every concrete type it is instantiated
at. This eliminates dictionary-passing overhead at runtime, which
matters for the Lua target where every extra table lookup and closure
allocation is felt.

Polymorphic recursion — where a function calls itself at a
progressively different type — cannot be monomorphized: the type
grows without bound, so there is no finite set of specializations
to generate. The compiler detects this and falls back to
dictionary-passing for the whole function.

    data Nested a = NNil | NCons a (Nested [a])

    showNested :: Show a => Nested a -> String
    showNested NNil = "end"
    showNested (NCons x rest) = show x <> " > " <> showNested rest

    main :: IO ()
    main = do
        let n = NCons (1 :: Int)
                      (NCons [2, 3]
                             (NCons [[4, 5], [6]] NNil))
        putStrLn (showNested n)

Here `showNested` calls itself at `Nested [a]`, then `Nested [[a]]`,
and so on — each recursive occurrence is a strictly larger type. This
program compiles and runs, printing:

    1 > [2, 3] > [[4, 5], [6]] > end

Mechanically: monomorphization tries to specialize `showNested` at
`Nested Int`, which demands a specialization at `Nested [Int]`,
which demands `Nested [[Int]]`, and so on. When more than 16
trial specializations of one function accumulate, the compiler gives
up on that function, **discards those trial specializations, and
rewrites the entire function to dictionary-passing** — there is no
mix of monomorphized shallow copies and a dict-passing tail; every
call site of the function, however shallow, passes a dictionary. The
typeclass method is then looked up from a Lua table parameter instead
of being resolved statically. The emitted top-level call and the
recursive call look like:

    showNested({ show = show_Int }, value)   -- from main
    showNested(__dict_Show, rest)                -- the recursive call

Limitation — read this before relying on it. The dictionary is built
from the concrete type at the outermost call site and threaded
**unchanged** through the recursion; it is *not* re-derived at the
growing type. So every level resolves its method at the outermost
element type. That is correct when the method is faithful at the
deeper types anyway — the runtime `show` renders lists, tuples and
numbers structurally, which is why the example above prints correctly
— but it is wrong when a derived instance differs per level. With
`data Box a = Box a deriving (Show)` and `Deep (Box a)` recursion,
`show (Box 2)` on its own correctly prints `Box 2`, yet inside the
polymorphic recursion the threaded `show_Int` is applied to the
`Box` value and it prints `(2)` instead. In other words, mata-ll
compiles and runs genuinely polymorphic-recursive code, but the
dictionary-passing fallback does not transform the dictionary at each
recursive type, so class methods over a growing *user* type are
resolved incorrectly. Polymorphic recursion whose class methods stay
faithful under the outermost dictionary (structural `show`/`==` over
built-in containers and primitives) works; polymorphic recursion that
needs a genuinely different instance at each depth does not.

# Module and import syntax

The README says each file is a module. But how do you import one?

    import Data.Tree
    import Data.Tree (depth, Tree(..))
    import Data.Tree hiding (internalFn)
    import qualified Data.Tree as T

That will look for Data/Tree.mll in the project's and the compiler's
default library directory.

Two caveats on the forms above:

- **Qualified imports cover functions, not constructors.** After
  `import qualified Data.Tree as T`, a qualified *function* use like
  `T.depth` resolves, but a qualified *constructor* use like `T.Leaf`
  does not — constructors are not namespaced and must be written
  unqualified (`Leaf`), and they remain visible even under a qualified
  import.
- **The `(..)` constructor-list on an import is currently
  decorative.** `import Data.Tree (depth, Tree(..))` and
  `import Data.Tree (depth)` behave identically: the selective list
  restricts which *functions* are brought into scope, but a type's
  constructors are always exposed regardless of whether `Tree(..)` is
  written. `hiding (...)` is enforced for functions.

A module-header export list — `module M (foo, Bar(..)) where` —
controls which of the module's names *other .mll modules* may import:
names the list omits are private to the module. That is all it does.
In particular it does not export anything to plain Lua — the module's
Lua return table is populated exclusively by `export` declarations
(see "Export" below). re-exports are supported but the scope is
limited to within .mll. No exports to plain Lua are allowed that way.

Consequently, compiling a module that has neither a `main` nor any
`export` declaration (a header export list does not count) produces a
Lua file with no runnable or callable code: dead-code elimination is
rooted at `main` and the exports, so every definition is removed. The
compiler emits a warning explaining this instead of writing the empty
shell silently.

# Minimal prelude

Functions: show, putStrLn, putStr, getLine, print, (++), (<>), ($), max, min,
           const, id, (.), flip, map, filter, foldl, foldr, sqrt,
           not, (&&), (||), and, or, any, all, error, undefined,
           otherwise, head, tail, last, init, null, take, drop,
           takeWhile, dropWhile, span, zip, unzip, zipWith, elem,
           length, reverse, concat, replicate, iterate, sum, product,
           fst, snd, concatMap, mapM_, when, assert, seq, getArgs,
           exit, try, catch, foldMap, maximum, minimum, mempty,
           mappend, traverse, sequenceA, liftA2

    (++)  :: [a] -> [a] -> [a]              -- list append only
    (<>)  :: Semigroup a => a -> a -> a     -- string concatenation
    ($)   :: (a -> b) -> a -> b
    (.)   :: (b -> c) -> (a -> b) -> a -> c
    const :: a -> b -> a
    id    :: a -> a
    flip  :: (a -> b -> c) -> b -> a -> c
    error :: String -> a
    getLine :: IO String  -- one line from stdin, without the newline
    otherwise :: Bool  -- defined as True
    seq   :: a -> b -> b  -- explicit forcing
    assert :: Bool -> String -> IO ()
    sum     :: (Foldable t, Num a) => t a -> a
    product :: (Foldable t, Num a) => t a -> a

`getLine` reads one line from stdin without the trailing newline, as
in GHC, and needs no import. At end of input it raises the error
`Prelude.getLine: end of input`, catchable with `try`/`catch` — the
mata-ll analog of GHC's `isEOFError` exception (mata-ll errors are
strings, not typed exceptions).

Note: `String` is *not* a list, so `(++)` does not concatenate
strings — use `(<>)` for that. Conversely `(<>)` today applies only to
`String`; lists are joined with `(++)`. (`(<>)` is the `Semigroup`
method; only the `String` instance is usable at present.) The `Monoid`
class (superclass `Semigroup`) provides `mempty`, the *named* method
`mappend`, and `mconcat` (a class method with GHC's default
`foldr mappend mempty`, exactly as in base), with instances for
`String` and `[a]` — `mappend` does dispatch on lists (polymorphic
`Monoid` code like `foldMap` needs a working append), while the `(<>)`
operator on lists stays rejected in favour of `(++)`. The `String`
instance overrides `mconcat` with a linear builder (the elements are
collected and joined once), so a flat `mconcat` over many strings does
not pay the right-nested fold's repeated suffix copying; its result and
forcing behavior are identical to the default. The `Semigroup`/`Monoid` *classes* and
their `String`/`[a]` instances are ordinary declarations in the Prelude
source (like the `Foldable`/`Traversable` ones). An undetermined
`mempty` is still reported as an ambiguity at compile time: the
compiler synthesizes the class constraint carried by every method of a
source class (a use emits a wanted `ClassName classVar`), so a
return-position-only method whose type variable nothing determines is
rejected, exactly as for the builtin classes — and exactly as GHC does.

`Foldable` (methods `foldr`, `foldl`; instances `[]`, `Maybe`,
`Either` — folding over `Right`) generalizes the container folds, and
`length`, `null`, `elem`, `sum`, `product`, `maximum`, `minimum` and
`foldMap` are defined generically over it, so they work on any
Foldable (including user instances). `toList` lives in
`Data.Foldable` (as in GHC, whose Prelude does not export it either).
`Traversable` (method `traverse`; superclasses `Functor` and
`Foldable`; the same three instances) provides applicative traversal,
with `sequenceA` defined over it. `Applicative` additionally carries
`liftA2` as a real method (as in GHC). User types join all three
classes with ordinary `instance` declarations:

    data Tree a = Leaf | Node (Tree a) a (Tree a)
        deriving (Functor)

    instance Foldable Tree where
        foldr _ z Leaf = z
        foldr f z (Node l x r) = foldr f (f x (foldr f z r)) l
        foldl _ z Leaf = z
        foldl f z (Node l x r) = foldl f (f (foldl f z l) x) r

The builtin instances for `[]`, `Maybe` and `Either` are themselves
ordinary `instance` declarations in the Prelude (`instance Foldable []`
uses the bare list constructor at kind `Type -> Type`; see the Kinds
section).

Tuples have no Foldable/Traversable instance: the class variable has
kind `Type -> Type`, and mata-ll has no partially-applied tuple
constructor (the same reason tuples have no Ord instance).

Comparison and equality operators are methods of Eq and Ord:

    (==), (/=) :: Eq a => a -> a -> Bool
    (<), (>), (<=), (>=) :: Ord a => a -> a -> Bool
    compare :: Ord a => a -> a -> Ordering
    data Ordering = LT | EQ | GT

Typeclasses: Show, Eq, Ord, Enum, Bounded, Read, Semigroup, Monoid,
             Num, Fractional, Real, Integral,
             Functor, Applicative, Monad, Foldable, Traversable

Types: Maybe (Just, Nothing), Either (Left, Right), IO, Ordering,
       ExitValue (Normal, Err), Any

# Operator fixity declarations

User-defined fixity is supported:

    infixl 6 +
    infixr 5 :
    infix 4 ==
    infixl 7 `div`, `mod`

Precedence levels 0-9, with left, right, or no associativity; an
operator list may be comma-separated, and named (backtick) operators
may be declared with or without backticks. Haskell defaults are used
for standard operators when no declaration is given; an undeclared
operator is `infixl 9`.

Fixity follows Haskell scoping rules:

- A declaration governs the whole module, including uses that precede
  it textually.
- Fixity travels with an import: `import M` brings M's fixity
  declarations (and, transitively, those of M's imports) into the
  importing module, as in GHC. The implicit Prelude contributes its
  own (`infixl 4 <$>, <*>`, `infixl 7 div, mod`, `infix 4 elem`,
  `infixr 0 seq`).

A chain of same-precedence operators is rejected — as in GHC — when
any operator in it is non-associative (`infix`), or when the operators
disagree on associativity (an `infixl` next to an `infixr` at the same
level). So `a == b == c` is a parse error: `==` is `infix 4`, and the
expression has no defined grouping without parentheses.

Prefix minus has the fixity of binary subtraction (`infixl 6`), with
GHC's exact consequences:

- It cannot be the right operand of an operator at precedence 6 or
  higher: `a + -b`, `a - -b`, `a * -b`, and ``a `div` -b`` are parse
  errors (parenthesize the negation: `a + (-b)`). The same holds
  inside a right section: `(+ -2)` is rejected, `(* (-2))` accepted.
- Its operand is everything binding tighter than precedence 6:
  `-a * b` is `negate (a * b)` and ``-a `div` b`` is
  `negate (a `div` b)`, while `-a + b` is `negate a + b` (the
  precedence-6 `+` stops the operand).
- Left of a precedence-6 operator it participates only in an `infixl`
  grouping: `-a + b` parses, `-a <> b` is rejected (`<>` is
  `infixr 6`, and the mix has no defined grouping).

`(-x)` is negation, never a right section of subtraction; `(-)` is the
subtraction function, as in GHC.

# Deriving

Automatic instance generation is supported for Show, Eq, Ord, Enum,
Bounded, Functor, Generic, ToJSON, FromJSON, and LuaDict:

    data Color = Red | Green | Blue
        deriving (Show, Eq, Ord, Enum, Bounded)

    data Tree a = Leaf a | Branch (Tree a) (Tree a)
        deriving (Show, Eq, Functor)

    data Person = Person { pName :: String, pAge :: Int }
        deriving (Show, ToJSON, FromJSON)

The compiler generates the obvious structural instances. Enum and
Bounded are supported for simple enum types (constructors with no
fields).

`show` output matches GHC exactly. Strings are quoted and escaped by
GHC's rules (`show "a\nb"` is `"\"a\\nb\""`, control characters take
their GHC names — `\NUL`, `\ESC` — bytes above `\DEL` escape
numerically, and GHC's `\&` breaks the two ambiguous juxtapositions).
Lists and tuples separate with `,` and no space (`show [1,2]` is
`"[1,2]"`, `show (1,2)` is `"(1,2)"`); record constructors show in
record syntax (`P {px = 1, py = "s"}`, fields at precedence 0);
positional constructor fields parenthesize at argument precedence
(`Just (-1)`). `Number` (Double) uses GHC's shortest-identifying-digits
algorithm (a port of GHC's `floatToDigits`) with GHC's layout:
positional inside `[0.1, 10^7)` with a mandatory `.0` for integral
values (`show 3.0` is `"3.0"`), `d.ddde<exp>` outside (`show 0.01` is
`"1.0e-2"`, `show 12345678.0` is `"1.2345678e7"`), plus `NaN`,
`Infinity`, `-Infinity`, and a signed `-0.0`.

`LuaDict` generates no instance functions; it instead marks
the type's Lua-boundary layout — a record crosses as a keyed table,
and a nullary constructor of a `LuaDict` type crosses as its tag
string (see "Boundaries between standard Lua and MATA-LL").

`ToJSON`/`FromJSON` are provided by the `JSON` module (`import JSON`),
which offers a hand-written codec alongside the derived instances:
`encodeToJSON :: ToJSON a => a -> String` serializes a value, and
`decodeJSON :: FromJSON a => String -> Either String a` parses one.
Record fields map to JSON object keys. `Integer` round-trips exactly
at any magnitude — the `Json` type carries integer syntax beyond the
host number's exact window in a dedicated `JInt Integer` constructor,
and encoding emits bare decimal digits.

## Generic (datatype-generic programming)

`deriving (Generic)` (from `import Data.Generics`) gives a type a
*structural representation* `Rep a` and the conversions
`from :: a -> Rep a` / `to :: Rep a -> a`. A generic function is then an
ordinary typeclass whose instances pattern-match the representation
combinators — written once, it works for every deriving type. This is the
substrate datatype-generic code (custom JSON, schema, pretty-printing, …)
is built on, without a bespoke compiler pass per feature.

    import Data.Generics

    data Colour = Red | Green | Blue deriving (Generic)

    -- A generic constructor-index function, written once for all types.
    class GIx f where gix :: f -> Int
    instance GIx U1                     where gix _ = 0
    instance GIx (K1 c)                 where gix _ = 0
    instance (GIx a, GIx b) => GIx (a :+: b) where
        gix (L1 x) = gix x
        gix (R1 y) = 1 + gix y
    instance (GIx a, GIx b) => GIx (a :*: b) where gix _ = 0
    instance GIx f => GIx (D1 d f)      where gix (D1 x) = gix x
    instance GIx f => GIx (C1 c f)      where gix (C1 x) = gix x
    instance GIx f => GIx (S1 s f)      where gix (S1 x) = gix x

    conIndex :: (Generic a, GIx (Rep a)) => a -> Int
    conIndex x = gix (from x)
    -- conIndex Green == 1

The representation is a *sum of products*: `Rep a` is a `D1` datatype
wrapper around a sum (`:+:`) of constructors, each a `C1` constructor
wrapper around a product (`:*:`) of fields, each field an `S1` selector
wrapper around a `K1` constant that holds the field value. `U1` is the
empty product (a nullary constructor). `D1`/`C1`/`S1` carry the datatype,
constructor and field *metadata* — `datatypeName`, `conName` and `selName`
return the *effective external* names (the `as "…"` rename when present,
the source name otherwise; a positional field's `selName` is `""`), which
the `Datatype`/`Constructor`/`Selector` classes reflect. `deriving
(Generic)` therefore counts as a rename-consuming derive: `as` on a type
deriving only `Generic` is accepted, the rename surfacing through the
metadata.

`Rep` is a closed type family the compiler extends with one equation per
`deriving (Generic)`. A generic function's `GC (Rep a)` constraint is
carried polymorphically and discharged when a concrete call fixes `a` and
`Rep a` reduces to that type's representation.

The `JSON` module ships a generic encoder and decoder over this
representation,

    genericToJSON   :: (Generic a, GEncode (Rep a)) => a -> Json
    genericFromJSON :: (Generic a, GDecode (Rep a)) => Json -> Either String a

ordinary library code walking the representation, whose wire format AND
error messages match `deriving (ToJSON)` / `deriving (FromJSON)`
byte-for-byte — the derived metadata (constructor names and tags,
record-ness, arity, field keys) drives the output. They are the worked
proof of the substrate and usable on any `deriving (Generic)` type. The
derives themselves keep their specialised native codecs, which encode
and decode directly without building representation values (the generic
pair costs measurably more at runtime, chiefly under LuaJIT). A single
generic function is specialised once per type up to the monomorphiser's
16-specialisation guard; past it the generic machinery switches to
dictionary passing — still correct at any number of types, at some
runtime indirection cost (see HASKDIFF).

Differences from Haskell's `GHC.Generics`: the module is `Data.Generics`
(not `GHC.Generics`); the metadata wrappers are the three distinct
constructors `D1`/`C1`/`S1` rather than one tagged `M1 i c f` (so instances
dispatch on the wrapper's head); `K1` carries no index tag; the rep
combinators are of kind `Type` (no phantom `x` on `from`/`to`); and there is
no `V1`/`U1`-less void case (a type with no constructors cannot derive
`Generic`). `deriving (Generic)` is for concrete (parameterless) types.

Both levels of the external representation can be renamed with `as`.
A record field's key: `fieldName as "key" :: T` — one shared external
name used as the LuaDict table key and the JSON object key. And a sum
constructor's tag: `Con field-types as "name"` (after the field
types, before the next `|` or `deriving`) sets the string the derived
codec writes and reads to tell the constructors apart — nullary
constructors encode as the bare external string, fielded ones carry
it as the `"tag"` value:

    data Outcome = Ok Int as "ok" | Err String as "error"
        deriving (Show, ToJSON, FromJSON)

    -- encodeToJSON (Err "x") == {"tag":"error","contents":"x"}
    -- show (Err "x") still prints the source name: Err "x"

A constructor rename changes the JSON tag: Show, construction,
pattern matching, and the in-language representation keep the source
name. It reaches the Lua boundary only for the nullary constructors of
a type deriving `LuaDict`, whose runtime value is a plain string (the
rename, or the source name) rather than the usual positional integer
tag; there one `as` sets both the JSON tag and the Lua string. A
fielded variant (a positional table) and a plain enum without
`LuaDict` (a positional integer) have no boundary name to change, so
for them JSON is the only surface where constructors are renamed.
Effective tags must be unique and non-empty within a type (the tag is
all the decoder has to tell constructors apart), the rename requires a
`ToJSON`, `FromJSON`, or `LuaDict` deriving, and it is rejected on the
constructor of an untagged type
(a single non-nullary constructor), whose JSON carries no tag.

# Export

Definitions are exported to plain Lua via the `export` keyword:

    export fibonacci :: Int -> [Int]
    fibonacci = flip take fib

Exports appear in the module's return table. An export may be a
function, an IO/LuaIO action, or a plain value: a function export
becomes a wrapper callable from Lua, an action export becomes a
wrapper that performs the action when the host calls it, and a value
export is marshalled to Lua as its forced value, by the same
type-directed conversion a function result uses. An export whose
signature uses a type the marshaller cannot move — a polymorphic or
class-constrained type, an ST/STArray handle, an IORef cell, an IO/LuaIO
action in argument position, or a function anywhere but a direct
top-level `(A -> LuaIO s R)` argument — is rejected at compile time
(see "Boundaries between standard Lua and MATA-LL" for the full
contract).

# STArray (mutable arrays)

`STArray s` is an intrinsic mutable integer array scoped to `ST s`.
It uses the same rank-2 scope-sealing technique as `LuaIO s`:

    runST        :: (forall s. ST s a) -> a
    newSTArray   :: Int -> Int -> ST s (STArray s)
    readSTArray  :: STArray s -> Int -> ST s Int
    writeSTArray :: STArray s -> Int -> Int -> ST s ()
    modifySTArray :: STArray s -> Int -> (Int -> Int) -> ST s ()
    stArrayLength :: STArray s -> ST s Int
    newSTArrayFromList :: [Int] -> ST s (STArray s)
    stArrayToList :: STArray s -> ST s [Int]

`ST s` is the same runtime as IO but with a type-level distinction.
The `forall s.` in `runST` prevents mutable state from escaping.

# IORef (mutable IO cells)

`IORef a` is GHC's plain mutable cell, imported as `Data.IORef`:

    newIORef     :: a -> IO (IORef a)
    readIORef    :: IORef a -> IO a
    writeIORef   :: IORef a -> a -> IO ()
    modifyIORef  :: IORef a -> (a -> a) -> IO ()
    modifyIORef' :: IORef a -> (a -> a) -> IO ()

Unlike `STArray` it is polymorphic in the element and not
region-scoped: a ref is an ordinary first-class value, and its
operations live in `IO` (use `liftIO` from a `LuaIO` context). The
runtime representation is one tagged Lua table slot, so a read or
write in a do-block compiles to a bare table index.

Laziness follows GHC exactly. `newIORef` and `writeIORef` don't
force the value; `modifyIORef` stores the unevaluated `f old`
(GHC's `read >>= write . f` — the classic space-leak shape, so
prefer `modifyIORef'` for counters and accumulators); `modifyIORef'`
forces the new value to WHNF before storing. A value read back with
`readIORef` shares its suspension with the cell, so forcing either
memoizes both — the same sharing GHC gives.

`instance Eq (IORef a)` is pointer identity, as in GHC: two refs
are `==` exactly when they are the same cell, whatever they hold,
and the instance demands nothing of the element type. There is no
`Show`, `Ord`, or `Hashable` instance, and a ref cannot cross the
FFI boundary in either direction (read the value out and pass
that). The `atomicModifyIORef`/`atomicWriteIORef`/`mkWeakIORef`
family is absent — see HASKDIFF.

# ByteString

`ByteString` is an intrinsic type backed by Lua strings with
explicit byte semantics. Same runtime representation as String but
with a type-level distinction. All operations are intrinsic.

Construction: bsEmpty, bsSingleton, bsCons, bsSnoc, bsConcat,
bsConcatList, bsReplicate, bsPack.

Deconstruction: bsHead, bsTail, bsUnpack.

Query: bsLength, bsIndex, bsNull, bsSub.

Transforms: bsMap, bsFoldl, bsXor, bsZipWith.

Conversion: bsToString, bsFromString.

Binary: bsGetI8, bsGetI16LE, bsPutI16LE, bsGetU16LE, bsGetU32LE.

Indices are 0-based.

# Standard library modules

The prelude is auto-imported. Additional modules live in `lib/` and
are imported explicitly:

    import ByteString    -- byte sequence operations
    import LIO           -- file I/O (fOpen, fRead, fWrite, ...)
    import LIOLinear     -- linear (%1) file handles (openOut, hPut, hClose)
    import LMath         -- math.* bindings (sin, cos, random, ...)
    import LOS           -- OS functions
    import LString       -- string utilities
    import LBit          -- bitwise operations
    import Regex         -- CPS-based regex matcher
    import JSON          -- hand-written JSON parser
    import Data.List     -- sortBy, nubBy, groupBy, intercalate, etc.
    import Data.Maybe    -- fromMaybe, catMaybes, mapMaybe, etc.
    import Data.Map      -- HashMap-backed Map (import qualified as M)
    import Data.Foldable    -- toList, foldl', find, etc. (re-exports)
    import Data.Traversable -- traverse, sequenceA, etc. (re-exports)
    import Control.Monad -- mapM, forM, sequence, etc.

Several of these deliberately reuse Prelude names (`Data.Map`'s
`map`/`filter`/`lookup`/`null`, `Data.List`'s `find`). Because
mata-ll merges every import into one namespace, such modules must be
imported `qualified` (e.g. `import qualified Data.Map as M`) to avoid
clashing with the Prelude or with each other.

# Evaluation strategy

MATA-LL uses non-strict evaluation. Function arguments and let/where
bindings are wrapped in memoizing thunks by default, and are forced
only when their value is demanded.

## The eagerness contract

The compiler may evaluate a delayed expression *eagerly* (in place, with
no thunk) instead of suspending it — but only when doing so cannot change
the observable result. This is the normative rule, and every eagerness
optimization must respect it:

> **Bottom is never evaluated eagerly.** An argument or binding may be
> evaluated eagerly only when either (a) the consumer is *guaranteed to
> force it* on every path where the consumer's own result is demanded, or
> (b) the expression is *provably total* — evaluating it now cannot raise
> an error, diverge, or trap. Any expression that might be `⊥`
> (`error`, `undefined`, non-termination, or a trapping `div`/`mod`) and
> is not covered by (a) stays lazy.

The contract holds through `return` / `pure` as well: a returned value is
left unforced, so `_ <- return (error "x")`, a bare `return (error "x")`
statement in a do-block, and `fmap f (return ⊥)` do not raise — the bottom
raises only when the value is demanded (matching GHC). One user-facing
consequence: a bottom returned inside `try` is not caught, because it is not
forced there — force it with `seq` (or, in GHC, `evaluate`) inside the tried
action to make it catchable. See CAVEATS.md.

Consequently, an argument the callee ignores is never forced by the call:

    g _ = 42
    main = print (g (error "boom"))   -- prints 42, never raises "boom"

This holds regardless of how "cheap" the discarded argument looks —
`g (x + y)`, `g (h x)`, and `g (Box x)` all leave their argument
suspended when `g` does not demand it. It also holds through function
composition — `(g . h) (error "boom")` is `g (h (error "boom"))`, and a
non-strict `g` leaves `h (error "boom")` unevaluated — and through *list
elements*: the head of a cons cell is a lazy position, so
`length [error "boom"]` returns `1` and `map g [error "boom"]` does not
force the element. A cons head is suspended at construction and forced
only at the point of consumption (see the head-consumption contract in
the codegen module, `mllc/src/codegen/expr.rs` and `runtime.lua`: a
value-consumer forces the head, a laziness-preserving one
— storing it in a new cons, or passing it to a function that decides —
does not). This holds at every construction site — list literals, both
`:` emission arms, and self-referential lists (`xs = error "boom" : xs`).
Data-constructor fields are likewise suspended
(`g (Box (error "boom"))` does not force the error), and so are *tuple
fields*: `fst (1, error "boom")` returns `1` and `snd (error "boom", 2)`
returns `2`. A tuple field is suspended at construction (weighed exactly
like a cons head) and forced only at a value-consumer — `fst`/`snd`, a
pattern that inspects it, `show`, equality, or the FFI boundary (where an
outgoing argument's list/tuple/record structure is marshalled into what a
Lua host reads — a cons list becomes a plain array, and a tuple's or
record's lazy fields are forced in place; an opaque value is passed raw).

The decision is made by *weighing* the benefit of eagerness (a saved
thunk allocation) against the risk to non-strict semantics. Bottom
carries maximal weight on the laziness side, so it always wins; the
eagerness benefit can only win for an expression that is provably total
or that the callee is proven to force. The two sources of the "callee
forces it" proof are:

- **Demand analysis.** A whole-program, greatest-fixpoint strictness
  analysis marks a parameter strict when forcing the function's result
  forces that parameter. This includes tail accumulators — in
  `loop 0 acc = acc; loop n acc = loop (n-1) (acc+n)`, `acc` is strict —
  so accumulator loops run without building a thunk chain. The analysis
  is a sound under-approximation: `&&`/`||`/`++`/`$` force only their
  left operand, `:` and tuples force nothing, `if`/`case` force only
  what all branches agree on. Runtime primitives that are strict by
  construction (FFI calls, and the ByteString and ST-array intrinsics in
  their value/index arguments) are seeded directly.

- **Provable totality.** Literals, already-forced (`concrete`) variables,
  constructors and tuples of such, and non-trapping arithmetic over such
  are safe to evaluate now because they cannot be `⊥`.

Everything else is thunked. A bare variable or nullary constructor in a
lazy position is passed as its raw thunk-or-value reference rather than
re-wrapped, so no redundant thunk is allocated.

`seq :: a -> b -> b` forces its first argument to WHNF, then yields the
second (the value of `seq a b` is the value of `b`; the second argument
is forced only to WHNF, so its subparts — list heads, tuple fields —
keep their laziness, and it is forced only when the `seq` result is
itself demanded). It works in **every** application form with these same
semantics: prefix `seq a b`, backtick infix ``a `seq` b``, partial
application `seq a`, and as a first-class value (`foldr seq z xs`,
`map (seq x) ys`). The fully-applied prefix and backtick forms are
lowered inline so that when the second argument is a tail call it stays a
tail call — a `seq`-strict accumulator (`go n acc = seq acc (go (n-1) …)`,
or its backtick spelling) runs in constant stack and will not overflow on
deep recursion. The other forms route through one runtime primitive with
the identical force-first-then-yield-second behaviour.

The compiler tracks concrete variables (already-forced values) to
skip redundant `__force` calls at runtime. Function parameters forced
at entry, top-level bindings, and monadic bind continuation
parameters are marked concrete.

## Tail calls

Recursive calls in tail position compile to Lua's native proper tail
calls (`return f(...)`), so self-recursive loops — direct or through
`if`/`case`/`let`, and mutual recursion — run in constant stack. The
compiler strips the transparent parentheses and force/thunk wrappers
that would otherwise sit between `return` and the call and defeat Lua's
tail-call optimization. A recursive call is *not* in tail position when
its result is consumed further (e.g. `1 + f n`, or `f n` under a lazy
`$` that suspends the accumulator); such calls still grow the stack or
build a thunk chain, exactly as in Haskell.

# Compilation pipeline

    .mll source
        ↓
    Lexer — tokenize with layout-sensitive indentation tracking
        ↓
    Parser — parse to AST (including fixity declarations)
        ↓
    Import resolution — merge imported .mll modules and the prelude
        ↓
    Desugar — do-notation to >>= chains
        ↓
    Type checker — HM unification + bidirectional checking,
                   exhaustiveness checking, kind checking,
                   class-constraint discharge (unsatisfiable
                   constraints such as Show on a function are
                   rejected here, at compile time)
        ↓
    Monomorphizer — specialize polymorphic functions per type
        ↓
    Verifier — post-monomorphization invariant check (a violation
               signals a compiler bug, not a user error)
        ↓
    Constant folding — evaluate statically known expressions
        ↓
    Expression splitting — hoist deep nesting into lets so the
                           emitted Lua stays within Lua's own
                           parser limits
        ↓
    Dead-code elimination — drop functions unreachable from main
                            and exports, and data constructors
                            nothing constructs or matches
        ↓
    Code generator — builds a Lua AST and prints it once, with
                     optimizations (bind chain flattening, function
                     inlining, cheapness analysis, concrete variable
                     tracking, cross-function demand analysis); the
                     runtime prelude is emitted on demand,
                     tree-shaking helpers the program never uses
        ↓
    .lua output (standalone, no runtime needed)

Compilation is deterministic: the same source compiles to
byte-identical Lua on every run.

# Planned features

## Low priority / deferred

### Floating and RealFrac

The higher rungs of the numeric tower — `Floating` (`pi`, `exp`,
`log`, `sqrt`, `sin`, …) and `RealFrac` (`truncate`, `round`, `ceiling`,
`floor`, `properFraction`) — are not yet classes. The operations exist
as `Number`-typed functions in `Data`/`LMath`; only their generalisation
into classes is deferred (see CAVEATS).

### where blocks in class/instance declarations

Complex local definitions in typeclass instance declarations.
Deferred until default method implementations are done.

## Exception handling

MATA-LL provides `try` and `catch` for recovering from IO errors
(Lua-level errors raised by file operations, network calls, etc.).

    try   :: IO a -> IO (Either String a)
    catch :: IO a -> (String -> IO a) -> IO a

`try` wraps an IO action in Lua's `pcall`. If the action succeeds,
the result is `Right value`; if it raises a Lua error, the result is
`Left errorMessage`.

`catch` runs an IO action; if it raises, the handler function
receives the error message and produces a recovery action.

    import LIO

    main :: IO ()
    main = do
        result <- try (fileLines "missing.txt")
        case result of
            Right ls  -> putStrLn (show (length ls))
            Left err  -> putStrLn ("Error: " <> err)

(There is no `readFile`; `fileLines :: String -> IO [String]` from
`LIO` reads a file eagerly into a list of lines. See "No lazy IO"
under Design constraints.)

Design decisions:
- Only IO errors are catchable (category (a): Lua runtime errors).
- `error` calls and non-exhaustive patterns (category (b): logic
  errors) are also caught by `pcall` in practice, but programs should
  not rely on catching these — they indicate bugs.
- Compiler bugs (category (c)) should never be caught.
- No `throw` — use `error` for deliberate failures. The error string
  from `error` may include Lua source location information.

## Explicitly out of scope

### Char type

MATA-LL does not and will not have a `Char` type. Strings are Lua
strings; there is no `[Char]` representation. Character-level
operations use `LString` or `ByteString`.

### Multi-parameter typeclasses

Not supported. Single-parameter typeclasses cover the needed use
cases for the Lua target.

### MVar, STRef

`MVar` is a concurrency primitive and Lua has no preemptive
threading. `STRef` adds nothing over `IORef` in a single-threaded
runtime except `runST`'s purity seal; the scoped-mutation hot path
is `STArray`, and the general mutable cell is `IORef` (which, unlike
these two, IS provided — see "IORef (mutable IO cells)").

# Known limitations

The following are known limitations of the current implementation:

- FFI varargs (e.g. Lua's string.format)

## Design constraints

- `LuaIterator` is for pure Lua iterators only (e.g. `string.gmatch`).
  IO-producing iterators like `io.lines` must use `IO` and read
  eagerly — lazy IO is not supported. Use `fileLines :: String -> IO
  [String]` from LIO for file reading. The `LuaIterator "f" [E]` type
  argument is always an explicit list naming the RESULT: it reduces to
  `[E]` and the iterator yields one `E` per step, each decoded as the
  element type. A bare (non-list) element type is rejected.
- No lazy IO. File and stream operations read eagerly into lists.
  For streaming, a conduit-style approach may be added in the future.
