# Caveats

## Out-of-bounds errors surface as cryptic Lua messages

Lua does not perform bounds checking on string access — `string.byte(s, i)`
returns `nil` when `i` is out of range rather than raising an error. This means
that an out-of-bounds read in mata-ll (e.g. calling `getU16LE` at the last byte
of a ByteString, or `strByte` past the end of a String) will not produce a
clear "index out of bounds" message. Instead, the `nil` propagates until it
hits an arithmetic operation, resulting in errors like:

    attempt to perform arithmetic on local 'hi' (a nil value)

Adding bounds checks to the ByteString and String primitives would fix the
messages but degrade performance for all callers, so this is left as-is. When
you see `attempt to perform arithmetic on … (a nil value)` in compiled output,
suspect an out-of-bounds access in the source.

## Tail calls run in constant stack; non-tail recursion does not

Recursive calls in *tail position* compile to Lua's native proper tail calls,
so self-recursive loops (direct or through `if`/`case`/`let`) and mutual
recursion run in constant stack no matter how deep. A call is in tail position
when its result is returned directly.

A recursive call whose result is used further is *not* a tail call and still
grows the stack:

    length []     = 0
    length (_:xs) = 1 + length xs      -- `1 + …` consumes the call: not a tail call

Such non-tail recursion (and a deeply nested recursive data traversal) will hit
Lua's stack limit on very deep input:

    stack overflow

Refactor to an accumulator in tail position, or use an iterative list
operation. Note that suspending the accumulator with a lazy `$`
(`loop (n-1) $ acc+n`) keeps the call in tail position but builds a thunk
chain that overflows when finally forced — pass the accumulator directly
(`loop (n-1) (acc+n)`) so demand analysis can keep it strict.

## Non-exhaustive patterns produce a runtime error

When a case expression or function definition doesn't cover all constructors,
the compiler emits a fallthrough that calls:

    error("Non-exhaustive patterns")

The error message does not include the function name or source location, so it
can be hard to track down. The exhaustiveness checker warns at compile time for
most cases, but guards and nested patterns can still slip through.

## Integer overflow wraps silently

mata-ll integers are Lua integers (64-bit signed on Lua 5.4+, or floats on
Lua 5.3 and earlier). Overflow wraps silently — there is no checked arithmetic
at runtime. If you need to detect overflow, check the bounds manually.
On LuaJIT, integers are represented as doubles, so precision is lost beyond
2^53.

`div` and `mod` follow Haskell's floor semantics and raise a plain
"divide by zero" error on a zero divisor, on every host. On Lua 5.3+ they
use native integer floor division (`//`) and are exact over the full 64-bit
range. On LuaJIT / Lua 5.1–5.2 there is no integer type at all — every
number, including the literal you wrote, is already a double — so `div`
results beyond 2^53 are approximate there. That is the host's number
representation, not something the division operator can recover; if you
need exact 62-bit quotients, run on a Lua 5.3+ host (the embedded runner
is Lua 5.4).

`quot`/`rem` (Integral) truncate toward zero, so their remainder takes the
dividend's sign, while `div`/`mod` floor and their remainder takes the
divisor's sign — matching GHC exactly (e.g. `(-17) \`quot\` 5 == -3` but
`(-17) \`div\` 5 == -4`). They share the same zero-divisor error and the same
host precision limits as `div`/`mod`.

## `fromRational` takes a `Number`, and `Real` has no `toRational`

mata-ll has no `Rational` type. A decimal literal is a `Number` (an IEEE-754
double) at the source level, so the `Fractional` method `fromRational` takes a
`Number` argument rather than GHC's `Rational` — this is the numeric tower's
one signature deviation, made because introducing an exact `Rational` (with its
own `Integer`-pair arithmetic and `Show`/`Eq`) would be disproportionate to its
value in a Lua-hosted language whose fractional type is already a double. For
the same reason the `Real` class carries no `toRational` method; it exists only
as the `(Num a, Ord a) =>` superclass marker that `Integral` sits above.
`toInteger` (Integral) is present and behaves as in GHC.

## `Floating` and `RealFrac` are functions, not classes

The higher rungs of the numeric tower are not yet typeclasses. `pi`, `exp`,
`log`, `sqrt`, `sin`, `cos`, … exist as `Number`-typed functions (in `LMath`),
and rounding/truncation (`floor`, `ceiling`, `truncate`, `round`) likewise
operate on `Number`. So you can compute with them at `Number`, but you cannot
yet write code generic over `Floating a`/`RealFrac a`, nor give those classes a
user instance. Generalising them is deferred; the underlying operations are all
present, so this is a missing abstraction, not missing functionality.

## Numeric-literal patterns are monomorphic; some literal errors defer

A numeric *literal pattern* (`f 0 = …`) matches at the concrete literal type
(`Integer` or `Number`), not at a polymorphic `Num a`. This differs from GHC,
where a literal pattern is `(== fromInteger n)` and works at any `Num`/`Eq`
type; in practice the scrutinee is almost always concrete, so this rarely
shows. Separately, because integer literals are now `Num a => a`, a mismatch
like `let x = 5 in putStrLn x` is reported as a deferred `No instance for
(Num String)` at the enclosing binding rather than a use-site "cannot unify
Integer with String" — the program is still rejected, but the message is the
instance error GHC also gives.

## Numeric defaulting only applies to standard classes

An unconstrained numeric literal defaults `Integer` then `Number` (GHC's
`default (Integer, Double)`). As in GHC, defaulting applies only when every
class constraining the variable is *standard* (a numeric class, `Eq`, `Ord`,
`Show`, `Read`, `Enum`, `Bounded`). A literal also constrained by a user class
— e.g. `myClassMethod 5` where `5 :: (MyClass a, Num a) => a` — is genuinely
ambiguous and must be annotated (`myClassMethod (5 :: Integer)`), exactly as
GHC requires.

## Existential record fields have no selector and no record update

Unpacking an existential constructor skolemizes the hidden type variable
(it is rigid inside the match and cannot leave it — see SPEC.md), which
rules out two record conveniences on fields whose type mentions the hidden
variable:

    data Foo = forall a. Foo { getIt :: a, label :: String }

`getIt` exists as a field (construction and pattern matching work, and
`label` keeps its ordinary selector), but there is no `getIt` selector
function — `getIt :: Foo -> a` would hand the hidden type to any caller,
outside every match — and `f { getIt = … }` is rejected because the type
the new value would have to match was erased when the record was packed.
Pattern-match the constructor positionally, and rebuild with the
constructor instead of updating. GHC restricts both the same way.

One adjacent divergence to know about: `where`-bindings are monomorphic in
mata-ll, so a polymorphic where-helper (`where ident v = v`) applied to
values unpacked from two *different* existential boxes is rejected — the
first use pins the helper to the first box's hidden type, and the second
box's hidden type is a different rigid type. GHC generalizes
where-bindings and accepts this. Inline the helper or make it a top-level
function with a signature.

## `try (pure e)` does not catch an error inside `e`

`return` / `pure` are non-strict, exactly as in GHC: `return e` yields `e`
unforced, so a bottom in `e` does not raise until something demands the value.
This is the correct eagerness-contract behavior, but it has a consequence for
exception handling — a bottom returned inside `try` escapes the `try`:

    r <- try (pure (1 `div` 0))   -- r is Right <thunk>, NOT caught
    case r of
        Right v -> print v        -- the divide-by-zero raises HERE, uncaught
        Left _  -> ...

To catch an error in a *pure* value, force it to WHNF inside the tried action —
`seq` is the portable way (GHC would use `evaluate`):

    r <- try (1 `div` 0 `seq` pure ())   -- forced inside try: r is Left ...

IO effects inside `try` are still caught normally; only a lazily-returned pure
value needs the explicit force.

## An IO action's result must not itself be a function

The compiled IO runtime distinguishes "an action to run" from "a result value"
by shape: after forcing, a Lua function is treated as an action and called.
That heuristic is ambiguous exactly when an action's *result* is a function —
`IO (a -> b)` — so generic code that routes a function through IO with
`g <$> action <*> action2` (which builds an `IO (b -> c)` intermediate) can
misfire at run time even though GHC accepts it. Direct `<*>` chains in
do-blocks are compiled specially and work; the caveat applies to
`Applicative f =>`-generic bodies instantiated at IO. Write such code with
`liftA2` (a real Applicative method here, as in GHC), which keeps only
fully-applied values in the container — the Prelude's `traverse` is built on
it for exactly this reason.

## Lazy evaluation can cause space leaks

mata-ll uses non-strict evaluation by default. Unevaluated thunks accumulate
on the heap if not forced. The classic example is a left fold over a large
list:

    foldl (+) 0 [1..1000000]  -- builds a chain of 1M thunks

Use `seq` to force intermediate values, or prefer strict accumulator patterns.
`seq` works in every application form — prefix `seq a b`, backtick
``a `seq` b``, partial application, and as a first-class value (`foldr seq z`) —
all with the same semantics (force the first argument, yield the second). The
demand analysis pass eliminates many unnecessary thunks, but it cannot catch
every case.

## Unused arguments are not evaluated (non-strict semantics hold)

An argument in a position the callee does not force is left suspended, so a
diverging or erroring expression there does not crash — matching Haskell:

    g _ = 42
    main = print (g (error "boom"))   -- prints 42

The compiler still evaluates an argument eagerly (skipping the thunk) when that
is provably safe: the callee is proven to force it (demand analysis, plus the
strict FFI/ByteString/ST-array primitives), or the expression is provably total
(a literal, an already-forced variable, a constructor, non-trapping arithmetic
over such). See "The eagerness contract" in SPEC.md for the normative rule —
bottom is never evaluated eagerly. This keeps the hot-loop performance that the
earlier, unsound "evaluate any cheap-looking argument" heuristic bought, without
its `⊥`-leaking behavior. It holds through function composition
(`(g . h) (error "boom")` does not force the error when `g` is non-strict),
through list elements (`length [error "boom"]` is `1`; `map g [error "boom"]`
does not force the element — a cons head is suspended until it is consumed, at
every construction site including self-referential lists), and through *tuple
fields* (`fst (1, error "boom")` is `1`; `snd (error "boom", 2)` is `2` — a
tuple field is suspended until a value-consumer reads it, just like a cons
head). Data-constructor fields other than the cons head were always lazy.

## `let` binds values, not functions

`let` and do-block `let` bind values only. A binding with parameters, e.g.

    let f x = x + 1 in f 10

is not supported and fails type checking with `Unbound variable: x` (the
parameter is never brought into scope). Value bindings are mutually recursive,
including self-referential lazy values:

    let fib = [1, 1] ++ zipWith (+) fib (drop 1 fib) in fib !! 11   -- 144

For a local function, bind a lambda (`let f = \x -> x + 1`) or use a `where`
clause, which does support multi-clause local functions.

## Do-block `<-` binds a variable, `_`, or a tuple only

A monadic bind `pat <- action` accepts a plain variable, a wildcard `_`, or a
tuple pattern — nothing else. A unit pattern or a constructor pattern on the
left is a parse error:

    () <- return ()          -- Parse error: Expected expression, found Bind
    Just x <- lookupThing k  -- not supported (no MonadFail-style refutable bind)

Use `_ <- action` to discard a result, and match constructor results with a
`case` on the bound value instead:

    r <- lookupThing k
    case r of
        Just x  -> ...
        Nothing -> ...

## Lua errors from FFI calls are not wrapped

When a Lua function called via the FFI raises an error (e.g. `io.open` on a
missing file via a plain `LuaIO` binding), the raw Lua error propagates as a
runtime crash. To capture it instead:

- **`LuaCatch "name" (Either String a)`** / **`LuaIOCatch "name" (Either
  String a)`** — run the call under `pcall`, returning `Left msg` on a raised
  error and `Right a` on success. This is the right tool when the Lua function
  signals failure by *raising*.
- **`LuaTry "name" a`** — for the `(nil, err)` return convention (`io.open`
  style), not raised errors.
- Wrapping a `LuaIO` call in **`try`** also catches errors as
  `IO (Either String a)`.

## The module-header export list does not export to Lua

`module M (addup) where` looks like it publishes `addup`, and inside
mata-ll it does: the list controls which of the module's names other
`.mll` files may import (anything omitted is private to the module).
But that is its entire job. It puts nothing in the compiled module's
Lua `return { … }` table — only `export` declarations do that:

    export addup :: Integer -> Integer -> Integer
    addup a b = a + b

A module with a header export list but neither a `main` nor any
`export` therefore compiles, standalone, to a Lua file with no
runnable or callable code — dead-code elimination is rooted at `main`
and the exports, so every definition is removed. The compiler warns
when this happens rather than writing the empty shell silently. Such
library modules are meant to be *imported* by another `.mll` module
(the compilation root), not compiled on their own.

## An export's types must be marshallable, or it is rejected

`export` signatures may only use types the FFI marshaller can move
across the Lua boundary — see the SPEC's "Boundaries" section for the
full allowed set. The compiler rejects, at compile time, an export whose
argument or result (recursing through tuples/lists/`Maybe`/records)
uses a type with no marshalling:

  - a bare polymorphic type variable (`export id :: a -> a`) — Lua has
    no representation for a polymorphic value, so give the export a
    concrete type (GHC's FFI rejects polymorphic foreign exports too);
  - a class-constrained type (`export f :: Num a => a -> a`) — a
    typeclass dictionary cannot cross the boundary;
  - a region-scoped `ST`/`STArray`/`STRef` handle — it must not outlive
    its `runST` and has no Lua representation;
  - an `IO`/`LuaIO` action in ARGUMENT position — a Lua caller has no
    action to hand in; only a *top-level callback* (a function returning
    `LuaIO`) may carry an effect inward;
  - a function (callback) anywhere but as a DIRECT top-level argument of
    the export — nested inside a tuple/list/`Maybe`/record, in result
    position, or as a callback's own argument (a
    callback-taking-a-callback). Only a top-level callback argument is
    fully marshalled by the code generator (arguments crossing out, its
    `LuaIO` result decoded back in); every other arrow position would be
    handed to Lua opaque and leak, so it is refused rather than
    silently miscompiled.

The error names the export binder, the offending sub-type, the position
(argument N or the result), and the crossing direction. This is a pure
tightening: it turns what used to compile into a silently-wrong
`__mll_to_lua`/opaque conversion at the boundary into a clear rejection.

One inherent, pre-existing marshalling limit remains for a value that IS
accepted: an opaque ADT (one with no marshal descriptor, e.g. `Either`)
is deep-forced correctly when it is the WHOLE result, but when it is
nested inside a list its payload is only shallow-forced, so a thunked
payload can leak. Prefer a descriptor-carrying container (`Maybe`, a
`LuaDict` record) when a structured value must survive nesting.

## Type families reduce, but non-injectively and with a termination bound

Closed type families reduce during type checking, including over type
variables (so `Vec (Plus n m) a`-style length arithmetic works — see
SPEC.md). Two consequences to be aware of:

- **A family is not injective.** From `F a ~ F b` the compiler will not
  conclude `a ~ b`, and two *stuck* family applications (each blocked on
  a type variable no equation matches) will not unify. A program that
  silently relies on such an equality is rejected with a "Cannot unify"
  error naming the two family applications — annotate to pin the types.
- **A non-terminating family is rejected, not run forever.** Reduction
  is bounded; a family whose equation reduces to itself
  (`Loop x = Loop x`) is reported as "type family '…' did not
  terminate" instead of hanging or overflowing the stack. A genuinely
  huge (but terminating) type-level computation could in principle hit
  the same bound — keep type-level arithmetic small.

## A promoted index must be pinned by a GADT, not a bare phantom

Promoted data types have real kinds (`data Nat` gives the kind `Nat`;
see SPEC.md), so a type-level index is checked to be exactly the right
kind — `Vec 'True Integer` is a kind error because `'True :: Bool`, not
`Nat`. Because mata-ll has no kind-signature syntax, the *only* way to
give a type parameter a promoted kind is to pin it through a GADT
constructor's return type:

    -- OK: 'S / 'Z in the return types force  n : Nat
    data Vec n a where
        VNil  :: Vec 'Z a
        VCons :: a -> Vec n a -> Vec ('S n) a

A NON-GADT type parameter used only as a phantom defaults to kind
`Type`, so it cannot carry a promoted tag:

    data Tagged a = Tagged Integer
    f :: Tagged 'Red -> Integer   -- KIND ERROR: 'Red :: Color, param : Type

GHC rejects this too without a `data Tagged (a :: Color)` kind
signature; mata-ll has no such signature, so make `Tagged` a GADT that
pins the tag in its constructor's result type instead.

## Linear (`%1`) checking rejects conservatively, and trusts the FFI side

The linear-types usage checker (see SPEC.md) is sound in the sense that
matters — it never accepts a double-use or a leak of a `%1` value that
GHC would reject, and scalars get no special treatment (`Integer`,
`Number`, `Bool` and `String` derived from a `%1` value are tracked
exactly-once, in strict parity with GHC). But its approximations mean
some programs that are *actually* linear are still rejected, always
erring toward rejection. The ones you are most likely to hit:

- **Branching on a linear scalar with a wildcard.** A `case` over a
  tainted scalar whose default is `_` is rejected, even though forcing
  the scrutinee to compare the literals would consume it:

      case useOnce t of        -- rejected: the '_' looks like a drop
          0 -> "zero"
          _ -> "other"

  Bind the alternative to a variable and use it, which the checker can
  see is a consumption:

      case useOnce t of
          0 -> "zero"
          n -> show n          -- accepted: 'n' is used

- **Discarding a non-`()` result built from a linear value.** A `>>` or
  `_ <-` that drops a result whose type is not `()` is rejected, because
  the pending consumption may live inside that never-forced result. Make
  the action return `()`, or bind and use the result.
- **Record update over a record built from a linear value** is rejected
  outright — it discards the previous field values, which the checker
  cannot prove resource-free.

Conversely, the **Lua side of a `%1` FFI signature is trusted**. When you
declare a foreign binding as `%1`, mata-ll charges the argument once per
call and assumes the host consumes it exactly once; it cannot see across
the FFI boundary to check that. The `%1` is your assertion about the host
function, not a checked fact — this is deliberate, and the one place
linear checking is not enforced end-to-end.

One place mata-ll is *more* permissive than GHC, but still safe: an
unannotated `let`/`where` binding that is never forced consumes nothing
(the thunk never runs under lazy evaluation), so `let u = t in useOnce t`
compiles where GHC's stricter typing rule rejects it. This cannot leak or
double-use at run time — it just reflects mata-ll's laziness.
