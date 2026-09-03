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
Lua's stack limit on deep input:

    stack overflow

Refactor to an accumulator in tail position, or use an iterative list
operation. Suspending the accumulator with a lazy `$`
(`loop (n-1) $ acc+n`) keeps the call in tail position but builds a thunk
chain that overflows when finally forced — pass the accumulator directly
(`loop (n-1) (acc+n)`) so demand analysis can keep it strict.

## Non-exhaustive patterns are rejected at compile time (mostly)

When a case expression or function definition doesn't cover all constructors,
the compiler rejects it with a hard error naming a witness of the gap:

    Type error: Non-exhaustive patterns in 'name': missing patterns for Blue

The check is matrix-based: every argument column of a function's clauses
participates, tuples are checked component-wise, constructor arguments are
checked recursively, and `True`/`False` count as the two constructors of
`Bool` (GHC merely warns in all of these positions; mata-ll rejects).

Gaps the checker deliberately does NOT reject — they compile to a runtime
fallthrough:

  - a guard chain with no always-true fallback (`f n | n < 0 = …` with no
    `otherwise`) — the fall-off is your stated intent;
  - matches decided by numeric, string, or char literals (`f 0 = …;
    f 1 = …`) — an infinite domain can never be proven covered, and a hard
    error would reject legal Haskell;
  - coverage that depends on GADT index refinement in nested patterns or
    across linked columns (`vzip VNil VNil = …`) — matched constructors of
    an index-refined type are trusted; only top-level missing constructors
    (filtered by the scrutinee's index) are still reported.

The runtime fallthrough calls:

    error("Non-exhaustive patterns")

That runtime message does not include the function name or source location, so
it can be hard to track down.

## Int overflow wraps silently

`Int` values are Lua integers (64-bit signed on Lua 5.3+, floats on Lua 5.2
and earlier — 5.3 is where Lua gained the integer subtype). Overflow wraps
silently — there is no checked arithmetic
at runtime. If you need to detect overflow, check the bounds manually.
On LuaJIT, integers are represented as doubles, so precision is lost beyond
2^53.

A second consequence of LuaJIT's doubles-only representation: the
LAST-RESORT type-erased `show` — the runtime dispatch used only when the
compiler cannot resolve a type at all (a genuinely polymorphic position
that neither specialization nor dictionary passing reached) — cannot tell
`1 :: Int` from `1.0 :: Double` there, because the two are the same Lua
value and LuaJIT has no `math.type`. It prints the whole double
integer-style: `1`, not `1.0`. Every type-directed path — `show` at a
known type, derived instances, containers with known element types, i.e.
everything realistic code reaches — is exact on every interpreter, and on
Lua 5.3+ even the erased path splits correctly on the native integer
subtype. This is the host's number representation, not recoverable by
dispatch; carrying a type tag would mean boxing scalars on LuaJIT, the
representation change the Integer design already rejected.

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
own `Int`-pair arithmetic and `Show`/`Eq`) would be disproportionate to its
value in a Lua-hosted language whose fractional type is already a double. For
the same reason the `Real` class carries no `toRational` method; it exists only
as the `(Num a, Ord a) =>` superclass marker that `Integral` sits above.
`Integral`'s `toInteger` and `Num`'s `fromInteger` are both present with
their GHC signatures: `Integer` is a real arbitrary-precision type — the
default for unannotated integer literals — and `Int`↔`Integer`
conversions go through exactly this pair (see HASKDIFF.md, "Integers").

## `Floating` and `RealFrac` are functions, not classes

The higher rungs of the numeric tower are not yet typeclasses. `pi`, `exp`,
`log`, `sqrt`, `sin`, `cos`, … exist as `Number`-typed functions (in `LMath`;
`sqrt` also in the Prelude), and rounding is `LMath.floor` / `LMath.ceil`
(`Number -> Int`). GHC's `round`, `truncate` and `ceiling` are not provided
(`truncate` is `LMath.floor` on a non-negative value; `round` — half to even
in GHC — has no shim yet). So you can compute with these at `Number`, but
you cannot yet write code generic over `Floating a`/`RealFrac a`, nor give
those classes a user instance. Generalising them is deferred.

## Some literal type errors defer to the enclosing binding

Because integer literals are `Num a => a`, a mismatch
like `let x = 5 in putStrLn x` is reported as a deferred `No instance for
(Num String)` at the enclosing binding rather than a use-site "cannot unify
Integer with String" — the program is still rejected, but the message is the
instance error GHC also gives. (Numeric *literal patterns* are
`Num`-polymorphic like GHC's `(== fromInteger n)` and compare
type-directed at runtime.)

## Numeric defaulting only applies to standard classes

An unconstrained numeric literal defaults `Integer` then `Number` — mata-ll
is defined as GHC with an implicit `default (Integer, Number)`, matching
standard GHC's `default (Integer, Double)` (`Number` = GHC's `Double`).
As in GHC, defaulting applies only when every
class constraining the variable is *standard* (a numeric class, `Eq`, `Ord`,
`Show`, `Read`, `Enum`, `Bounded`). A literal also constrained by a user class
— e.g. `myClassMethod 5` where `5 :: (MyClass a, Num a) => a` — is
ambiguous and must be annotated (`myClassMethod (5 :: Int)`), exactly as
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

One adjacent limit to know about: an UNCONSTRAINED polymorphic
where-helper (`where ident v = v`) generalizes as in GHC and may be
applied to values unpacked from two *different* existential boxes. A
where- or let-helper that carries a class constraint does not generalize
(a local binding is one Lua closure, see HASKDIFF.md "Existentials, and
how far where-bindings generalize"): `where describe v = show v` used on
two boxes' values is rejected. Make such a helper a top-level function
with a signature.

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

## `getLine`/`readLine` at end of input raise a string error, not a typed `isEOFError`

`getLine :: IO String` matches GHC (Prelude, no import, strips the trailing
newline), and at end of input it raises an error rather than returning a nil
or crashing — but mata-ll's error handling is string-based, so where GHC
throws an `IOException` satisfying `isEOFError`, mata-ll raises the string
`Prelude.getLine: end of input`. It is catchable with `try`/`catch` like any
other error; there is no `isEOFError` predicate, so a handler that must
distinguish EOF from other failures has to inspect the message. As with all
caught errors, the string carries a Lua source-position prefix
(`file:line: Prelude.getLine: end of input`), so match on the suffix, not
the whole string. `LIO`'s `readLine :: IO String` has the identical EOF
guard with its own message, `LIO.readLine: end of input` — same caveats
(string error, no typed predicate, source-position prefix). `LIO.readStdin`,
the format-argument reader, is still the raw `io.read` binding: at end of
input (or, for the "n" format, an unparseable number) its Lua `nil` escapes
into the result.

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

## `let` bindings: functions, values, and pattern parameters

`let` and do-block `let` bind values AND functions, mutually recursively —
including self-referential lazy values:

    let f x = x + 1 in f 10                                         -- 11
    let fib = [1, 1] ++ zipWith (+) fib (drop 1 fib) in fib !! 11   -- 144

A binding may take PATTERN parameters (`let dist (a, b) = a + b`), which
desugar exactly like pattern lambdas. A refutable pattern parameter
(`let fromJust' (Just v) = v`) is accepted with GHC's partial-function
semantics: a mismatch raises `Non-exhaustive patterns in <name>` at run
time. Local bindings are single-equation; multi-clause local functions
belong in a `where` clause.

## Do-block `<-` binds any pattern

A monadic bind `pat <- action` accepts any pattern: a variable, `_`,
tuples (nested too), `()`, constructor patterns, list patterns, literals.
A REFUTABLE pattern gets GHC's MonadFail-style semantics — a mismatch
raises `Pattern match failure in do expression at <line>:<col>`, catchable
with `try` like any other `error`:

    Just x <- lookupThing k       -- raises if the lookup returns Nothing
    (a : rest) <- readValues      -- raises on []
    () <- checkThing              -- irrefutable: no failure path

The message is mata-ll's `error` string; GHC renders the same failure as
an `IOException` with a file-qualified span — same semantics, different
formatting.

## Lua errors from FFI calls are not wrapped

When a Lua function called via the FFI raises an error (e.g. `io.open` on a
missing file via a plain `LuaIO` binding), the raw Lua error propagates as a
runtime crash. To capture it instead:

- **`LuaCatch "name" (Either String a)`** / **`LuaIOCatch "name" (Either
  String a)`** — run the call under `pcall`, returning `Left msg` on a raised
  error and `Right a` on success. This is the right tool when the Lua function
  signals failure by *raising*.
- **`LuaTry "name" (Either String a)`** — for the `(nil, err)` return
  convention (`io.open` style), not raised errors.
- Wrapping a `LuaIO` call in **`try`** also catches errors as
  `IO (Either String a)`.

## The module-header export list does not export to Lua

`module M (addup) where` looks like it publishes `addup`, and inside
mata-ll it does: the list controls which of the module's names other
`.mll` files may import (anything omitted is private to the module).
But that is its entire job. It puts nothing in the compiled module's
Lua `return { … }` table — only `export` declarations do that:

    export addup :: Int -> Int -> Int
    addup a b = a + b

A module with a header export list but neither a `main` nor any
`export` therefore compiles, standalone, to a Lua file with no
runnable or callable code — dead-code elimination is rooted at `main`
and the exports, so every definition is removed. The compiler warns
when this happens rather than writing the empty shell silently. Such
library modules are meant to be *imported* by another `.mll` module
(the compilation root), not compiled on their own.

## FFI types must have a designed marshalling shape, or they are rejected

Anything that touches the Lua boundary — an `export`, and equally an FFI
IMPORT (`LuaPure`/`LuaIO`/`LuaTry`/`LuaIOCatch`, which calls INTO Lua) —
may only use types with DEFINED marshalling behavior: a shape the host
is meant to see, not an internal representation that happens to be a
table. See the SPEC's "Boundaries" section for the full allowed set. The
compiler rejects, at compile time, an argument or result (recursing
through tuples/lists/`Maybe`/records) that uses a type with no designed
FFI shape:

  - a plain user or prelude `data` ADT — a multi-constructor and/or
    multi-field type, including `Either` (outside a `LuaTry`/`LuaIOCatch`
    result), `Ordering`, and `ExitValue`. It would cross only as
    MATA-LL's internal `{tag, fields…}` table, which has no meaning to a
    Lua host, so it is rejected EVEN WHEN its fields would each marshal.
    To carry structured data, use a `LuaDict` record (a name-keyed
    table); for a dynamic scalar, use `Any`; or encode the value as a
    scalar or a list. A NEWTYPE over a marshallable type crosses
    transparently (the value IS its field), so it is accepted — this is
    what keeps `newtype FileHandle = FileHandle LuaUserData` and the
    whole `LIO` file API crossing;
  - a bare polymorphic type variable (`export id :: a -> a`) — Lua has
    no representation for a polymorphic value, so give it a concrete type
    (GHC's FFI rejects polymorphic foreign exports too). The one designed
    exception is the threaded STATE of a polymorphic outgoing-callback
    FFI (the fold pattern), which round-trips opaquely and is checked
    separately;
  - a class-constrained type (`export f :: Num a => a -> a`) — a
    typeclass dictionary cannot cross the boundary;
  - a region-scoped `ST`/`STArray` handle — it must not outlive
    its `runST` and has no Lua representation;
  - an `IORef` cell — it holds a mata-ll value, possibly an unevaluated
    thunk, which has no meaning to a Lua host; read the value with
    `readIORef` and pass that;
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

The error names the binder, the offending sub-type, the position
(argument N or the result), and the crossing direction. This is a
tightening: it turns what used to compile into a silently-wrong
`__mll_to_lua`/opaque conversion at the boundary — a raw tagged table
the host cannot interpret — into a clear rejection.

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
  terminate" instead of hanging or overflowing the stack. A
  huge (but terminating) type-level computation could in principle hit
  the same bound — keep type-level arithmetic small.

## A promoted index must be pinned by a GADT, not a bare phantom

Promoted data types have real kinds (`data Nat` gives the kind `Nat`;
see SPEC.md), so a type-level index is checked to be exactly the right
kind — `Vec 'True Int` is a kind error because `'True :: Bool`, not
`Nat`. Because mata-ll has no kind-signature syntax, the *only* way to
give a type parameter a promoted kind is to pin it through a GADT
constructor's return type:

    -- OK: 'S / 'Z in the return types force  n : Nat
    data Vec n a where
        VNil  :: Vec 'Z a
        VCons :: a -> Vec n a -> Vec ('S n) a

A NON-GADT type parameter used only as a phantom defaults to kind
`Type`, so it cannot carry a promoted tag:

    data Tagged a = Tagged Int
    f :: Tagged 'Red -> Int   -- KIND ERROR: 'Red :: Color, param : Type

GHC rejects this too without a `data Tagged (a :: Color)` kind
signature; mata-ll has no such signature, so make `Tagged` a GADT that
pins the tag in its constructor's result type instead.

## Linear (`%1`) checking rejects conservatively, and trusts the FFI side

The linear-types usage checker (see SPEC.md) is sound in the sense that
matters — it never accepts a double-use or a leak of a `%1` value that
GHC would reject, and scalars get no special treatment (`Int`,
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
