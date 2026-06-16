MATA-LL spec.

This document is a work in progress and evolving.

Intrinsic means behavior only implementable by the compiler, not
inside MATA-LL.

Generated Lua bytecode is not shown directly but implicitly by
specifying Lua code.

Comments are done with -- just like in haskell and -- wow! -- Lua!

Our primitive data types should match the Lua ones:

    String, Integer, Number, Bool

Lua tables with continuous integer keys (i.e., arrays) should be written as

    [a]

Where a is the type of the items contained inside the array.

Lua dictionaries have their own intrinsic MATA-LL type:

    intrinsic HashMap :: Type -> Type -> Type

HashMap is a compiler built-in backed by Lua tables, not a
user-defined ADT. Built-in operations include hashmap_lookup,
hashmap_insert, hashmap_delete, hashmap_keys, hashmap_values,
hashmap_member, and hashmap_fromList.

We support haskell's algebraic datatypes:

    data A = A String | B Integer Integer

This datatype is internally represented as:

    foo = A "Hello"  -- { 1 = 1, 2 = "Hello" }
    bar = B 17 23    -- { 1 = 2, 2 = 17, 3 = 23 }

As you can see, we simply use integer indices here, where index 1
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

And also newtype:

    newtype A = A Integer

In order to make it easier to interact with plain Lua, the prelude
defines:

    data Any = AnyString String | AnyInteger Integer
             | AnyNumber Number | AnyBool Bool | AnyNull

Though this should rarely be used.

## GADTs

In addition to standard algebraic datatypes, MATA-LL supports
Generalized Algebraic Data Types (GADTs). GADTs allow each constructor
to specify its own return type, refining the type variable:

    data Expr a where
        LitI :: Integer -> Expr Integer
        LitB :: Bool -> Expr Bool
        Add  :: Expr Integer -> Expr Integer -> Expr Integer
        If   :: Expr Bool -> Expr a -> Expr a -> Expr a

Pattern matching on a GADT constructor introduces local type
equalities into scope for that branch. For example:

    eval :: Expr a -> a
    eval (LitI n)     = n
    eval (LitB b)     = b
    eval (Add x y)    = eval x + eval y
    eval (If c t e)   = if eval c then eval t else eval e

In the `LitI` branch, the compiler knows `a ~ Integer`, so returning
`n :: Integer` as `a` is valid. In the `LitB` branch, `a ~ Bool`.
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
matching on `MkShowBox` brings `a` back into scope locally, but it
cannot escape the branch.

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

    add :: Integer -> Integer -> Integer
    add a b = a + b

Could translate to

    local add = function(a, b) return a + b end

When called as

    add 1 2

That call could simply translate to

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
        fun :: T -> Integer
        (+) :: T -> T -> Integer

Since we don't want to support full haskell, for now we only support
typeclasses with a single type argument for now.

# Kinds

MATA-LL supports a small, fixed set of kinds. There is no kind
polymorphism and no promotion.

    Type   -- the kind of normal types (Integer, Bool, Maybe String, ...)
    Symbol -- the kind of type-level strings, used in FFI declarations
    Fn     -- the kind of function types ending in IO (a -> ... -> IO b)

Kind annotations use the `::` syntax within type signatures:

    intrinsic engage :: LuaFunction -> (a :: Fn)

`Fn` constrains `a` to types of the form `x -> ... -> IO y`. The
compiler rejects any `a` that does not end in `IO`. This ensures
that Lua functions called through `engage` are always treated as
effectful.

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
        let f = engage luafn :: Integer -> LuaIO s Integer
        result <- f 42
        liftIO $ putStrLn (show result)

    -- This would be rejected: s would escape
    -- bad :: LuaFunction s -> IO (LuaFunction s)
    -- bad f = return f   -- type error: s is not in scope

Rank-2 types are supported ONLY for this specific pattern: the
`forall s.` quantifier on exported functions receiving
`LuaFunction s`. General rank-2 polymorphism is not supported.

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

## Runtime representation

Both `s` and `forall s.` are purely compile-time constructs. They
have no runtime representation. The generated Lua code for a
function receiving a `LuaFunction s` is identical to one receiving
any other argument — the scope safety is enforced entirely by the
type checker.

# Type families and intrinsics

The `intrinsic` keyword marks definitions whose equations are part of
the language spec and visible for type checking, but whose
implementation is provided by the compiler. Users cannot define their
own intrinsic type families.

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

## FFI using type families

With the above, FFI declarations become:

    sin :: Number -> LuaPure "math.sin" Number
    rnd :: Number -> Maybe Number -> LuaIO "math.random" Number

`LuaPure` reduces to the bare return type; `LuaIO` wraps in `IO`.
The `Symbol` argument is consumed by the compiler during code
generation to resolve the target Lua function and is then erased from
the type.

A `Maybe` argument translates to an optional Lua parameter: `None`
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

    class Read a where
        read :: String -> a
        -- Instances: Integer, Number, Bool, String

    class Enum a where
        succ        :: a -> a
        pred        :: a -> a
        toEnum      :: Integer -> a
        fromEnum    :: a -> Integer
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
via the module's return value. FFI exports are restricted to
functions.

When MATA-LL is intended to run standalone, the compiler shall
generate a stub Lua file main.lua that loads the MATA-LL bytecode and
calls into it.

## Function bodies in MATA-LL

The compiler is free to split up functions defined in MATA-LL to
multiple Lua chunks or functions, as long as the semantics are
unaffected.

In particular, the compiler is free to decide whether to split a large
if block into calls of sub-functions.

The compiler may inline functions whenever deemed necessary.

## Pattern matching

Pattern matching is supported for function definitions, case blocks,
and assignments.

NOTE: The following distinction is aspirational and not yet
implemented. Currently all non-exhaustive patterns produce a runtime
error via a fallback.

For assignments, we want to distinguish between "single-case
assignments" such as a let or <- assignment inside a do block. Here,
pattern mismatch should raise an error. Multi-case assignments
include where and let assignments outside of do blocks. Here, the
compiler should enforce exhaustive definitions.

Pattern matching semantics:

    data Tree a = Branch (Tree a) (Tree a) | Leaf a

    depth :: Tree a -> Integer
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

The generated .lua file ends with a call to __run():

    __run()

We call __run, not main, because main is not declared as a function
exported to Lua. The compiler can also execute the result directly
via the embedded mlua runtime when invoked with --run.

Command line arguments are not passed to main, nor is a return value
passed back to the OS.

For both, library functions should be used:

  getArgs :: IO [String]
  exit :: ExitValue -> IO ()

  data ExitValue = Normal | Err Integer

# Do-notation

Do should be desugared just like in haskell.

    main :: IO ()
    main = do
        x <- rnd 1 (Just 6)
        putStrLn (show x)
        let y = x + 1
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

# Lambda syntax

If you write

    \ a b -> a + b

You get a lambda. Writing

    \a -> \b -> a + b

Shall be semantically equivalent to the first one.

Pattern matching in lambda arguments is supported.

# Guards

Guards are supported on function definitions and case branches:

    abs :: Integer -> Integer
    abs n | n < 0     = -n
          | otherwise = n

    classify :: Integer -> String
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

Integer literals are `Integer`. Decimal literals are `Number`. There
is no polymorphic `Num`-based literal overloading.

    42    :: Integer
    3.14  :: Number

String literals use double quotes with C-style escape sequences:

    "hello\n"
    "tab\there"
    "quote: \""

# Typeclass instances


    instance Show PersonType where
        show Human = "Human"
        show LLM   = "LLM"

    instance Show a => Show (Tree a) where
        show (Leaf x)     = "Leaf " ++ show x
        show (Branch l r) = "Branch (" ++ show l ++ ") (" ++ show r ++ ")"

Superclass constraints on instances? Yes.

Orphan instance rules? Disallowed.

# Typeclass dispatch strategy

Via monomorphization. The compiler generates a specialized copy of
each polymorphic function for every concrete type it is instantiated
at. This eliminates dictionary-passing overhead at runtime, which
matters for the Lua target where every extra table lookup and closure
allocation is felt.

Polymorphic recursion — where a function calls itself at a
progressively different type — is supported via a fallback to
dictionary-passing. When monomorphization would diverge (more than
16 specializations of the same function), the compiler switches
that function to dictionary-passing: typeclass methods are looked
up from a Lua table parameter instead of being resolved statically.

    data Deep a = DNil | DCons a (Deep (Box a))

    showDeep :: Show a => Deep a -> String
    showDeep DNil = "end"
    showDeep (DCons x rest) = show x ++ " > " ++ showDeep rest

Here `showDeep` calls itself at `Deep (Box a)`, then
`Deep (Box (Box a))`, etc. The compiler generates up to 16
monomorphized copies (which handle the common shallow cases with
zero overhead), then falls back to a single dictionary-threaded
version for deeper nesting. Call sites pass concrete dictionaries
as Lua tables:

    showDeep({ show = show_Integer }, value)

# Module and import syntax

The README says each file is a module. But how do you import one?

    import Data.Tree
    import Data.Tree (depth, Tree(..))
    import Data.Tree hiding (internalFn)
    import qualified Data.Tree as T

That will look for Data/Tree.mll in the project's and the compiler's
default library directory.

re-exports are supported but the scope is limited to within .mll. No
exports to plain Lua are allowed that way.

# Minimal prelude

Functions: show, putStrLn, putStr, print, (++), ($), max, min,
           const, id, (.), flip, map, filter, foldl, foldr, sqrt,
           not, (&&), (||), error, undefined, otherwise, head, tail,
           take, zipWith, elem, length, reverse, fst, snd, concatMap,
           mapM_, when, assert, seq, getArgs, exit, try, catch

    (++)  :: String -> String -> String
    ($)   :: (a -> b) -> a -> b
    (.)   :: (b -> c) -> (a -> b) -> a -> c
    const :: a -> b -> a
    id    :: a -> a
    flip  :: (a -> b -> c) -> b -> a -> c
    error :: String -> a
    otherwise :: Bool  -- defined as True
    seq   :: a -> b -> b  -- explicit forcing
    assert :: Bool -> String -> IO ()

Comparison and equality operators are methods of Eq and Ord:

    (==), (/=) :: Eq a => a -> a -> Bool
    (<), (>), (<=), (>=) :: Ord a => a -> a -> Bool
    compare :: Ord a => a -> a -> Ordering
    data Ordering = LT | EQ | GT

Typeclasses: Show, Eq, Ord, Enum, Bounded, Read, Functor,
             Applicative, Monad

Types: Maybe (Just, Nothing), Either (Left, Right), IO, Ordering,
       ExitValue (Normal, Err), Any

# Operator fixity declarations

User-defined fixity is supported:

    infixl 6 +
    infixr 5 :
    infix 4 ==

Precedence levels 0-9, with left, right, or no associativity.
Haskell defaults are used for standard operators when no declaration
is given.

# Deriving

Automatic instance generation is supported for Show, Eq, Ord, Enum,
Bounded, and Functor:

    data Color = Red | Green | Blue
        deriving (Show, Eq, Ord, Enum, Bounded)

    data Tree a = Leaf a | Branch (Tree a) (Tree a)
        deriving (Show, Eq, Functor)

The compiler generates the obvious structural instances. Enum and
Bounded are supported for simple enum types (constructors with no
fields).

# Export

Functions can be exported to plain Lua via the `export` keyword:

    export fibonacci :: Integer -> [Integer]
    fibonacci = flip take fib

Exported functions appear in the module's return table and are
callable from Lua. Only functions can be exported.

# STArray (mutable arrays)

`STArray s` is an intrinsic mutable integer array scoped to `ST s`.
It uses the same rank-2 scope-sealing technique as `LuaIO s`:

    runST        :: (forall s. ST s a) -> a
    newSTArray   :: Integer -> Integer -> ST s (STArray s)
    readSTArray  :: STArray s -> Integer -> ST s Integer
    writeSTArray :: STArray s -> Integer -> Integer -> ST s ()
    modifySTArray :: STArray s -> Integer -> (Integer -> Integer) -> ST s ()
    stArrayLength :: STArray s -> ST s Integer
    newSTArrayFromList :: [Integer] -> ST s (STArray s)
    stArrayToList :: STArray s -> ST s [Integer]

`ST s` is the same runtime as IO but with a type-level distinction.
The `forall s.` in `runST` prevents mutable state from escaping.

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
    import LMath         -- math.* bindings (sin, cos, random, ...)
    import LOS           -- OS functions
    import LString       -- string utilities
    import LBit          -- bitwise operations
    import Regex         -- CPS-based regex matcher
    import JSON          -- hand-written JSON parser

# Evaluation strategy

MATA-LL uses non-strict evaluation. Function arguments and let/where
bindings are wrapped in memoizing thunks by default.

Cheapness analysis avoids thunking expressions that are cheaper to
evaluate than to thunk: literals, variable references, constructor
applications, arithmetic, tuple construction, and cheap
if-expressions.

`seq :: a -> b -> b` forces its first argument before returning the
second.

The compiler tracks concrete variables (already-forced values) to
skip redundant `__force` calls at runtime. Function parameters forced
at entry, top-level bindings, and monadic bind continuation
parameters are marked concrete.

# Compilation pipeline

    .mll source
        ↓
    Lexer — tokenize with layout-sensitive indentation tracking
        ↓
    Parser — parse to AST (including fixity declarations)
        ↓
    Desugar — do-notation to >>= chains
        ↓
    Type checker — HM unification + bidirectional checking,
                   exhaustiveness checking, kind checking
        ↓
    Monomorphizer — specialize polymorphic functions per type
        ↓
    Code generator — Lua source with optimizations
                     (bind chain flattening, function inlining,
                      cheapness analysis, concrete variable tracking,
                      cross-function demand analysis)
        ↓
    .lua output (standalone, no runtime needed)

# Planned features (by priority)

## High priority

### Higher-rank polymorphism

Generalize beyond the current narrow rank-2 support (ST and
LuaFunction scope sealing) to support general `forall` quantification
in argument positions.

### Standard library compatibility modules

Initial versions of `Data.List`, `Data.Maybe`, `Data.Map`, and
`Control.Monad` are implemented. Functions requiring typeclass
constraints (Eq/Ord) in library modules have limited support due to
monomorphization occurring at use-site, not library compilation time.
Callback-style variants (e.g. `sortBy`, `nubBy`) work.

## Medium priority

### Default method implementations in class declarations

    class Eq a where
        (==) :: a -> a -> Bool
        (/=) :: a -> a -> Bool
        x /= y = not (x == y)    -- default implementation

Currently class bodies can only contain type signatures, not
definitions.

## Low priority / deferred

### Num typeclass and numeric tower

A pure MATA-LL library providing:

    class Num a where
        (+) :: a -> a -> a
        (-) :: a -> a -> a
        (*) :: a -> a -> a
        abs :: a -> a
        signum :: a -> a
        fromInteger :: Integer -> a

This includes types like Float, Double, Word, Rational as pure
MATA-LL wrappers. Performance will be poor compared to intrinsic
arithmetic, but correctness and compatibility are the goals. Similarly
for Integral and Fractional.

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

    main :: IO ()
    main = do
        result <- try (readFile "missing.txt")
        case result of
            Right contents -> putStrLn contents
            Left err       -> putStrLn ("Error: " ++ err)

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

### IORef, MVar, STRef

Lua has no preemptive threading, so concurrent mutable state
primitives serve no purpose. `STArray` covers the scoped mutation
use case.

# Known limitations

The following are known limitations of the current implementation:

- FFI varargs (e.g. Lua's string.format)

## Design constraints

- `LuaIterator` is for pure Lua iterators only (e.g. `string.gmatch`).
  IO-producing iterators like `io.lines` must use `IO` and read
  eagerly — lazy IO is not supported. Use `fileLines :: String -> IO
  [String]` from LIO for file reading.
- No lazy IO. File and stream operations read eagerly into lists.
  For streaming, a conduit-style approach may be added in the future.
