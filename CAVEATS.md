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

## No tail call optimization

mata-ll does not emit Lua tail calls. Recursive functions compile to ordinary
calls, so deep recursion will eventually hit Lua's call stack limit (typically
around 200 levels in Lua 5.4, ~65000 in LuaJIT). The error looks like:

    stack overflow

Functions that loop via self-recursion (accumulators, fold-style loops) work
fine in practice because each iteration is a single call frame. But mutual
recursion or deeply nested recursive data traversals can overflow. If you hit
this, refactor to use an accumulator pattern or an iterative list operation.

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

## Lazy evaluation can cause space leaks

mata-ll uses non-strict evaluation by default. Unevaluated thunks accumulate
on the heap if not forced. The classic example is a left fold over a large
list:

    foldl (+) 0 [1..1000000]  -- builds a chain of 1M thunks

Use `seq` to force intermediate values, or prefer strict accumulator patterns.
The demand analysis pass eliminates many unnecessary thunks, but it cannot
catch every case.

## `let` binds values, not functions

`let` and do-block `let` bind values only. A binding with parameters, e.g.

    let f x = x + 1 in f 10

is not supported and fails type checking with `Unbound variable: x` (the
parameter is never brought into scope). Value bindings are mutually recursive,
including self-referential lazy values:

    let fib = [1, 1] ++ zipWith (+) fib (drop 1 fib) in fib !! 11   -- 144

For a local function, bind a lambda (`let f = \x -> x + 1`) or use a `where`
clause, which does support multi-clause local functions.

## Lua errors from FFI calls are not wrapped

When a Lua function called via the FFI raises an error (e.g. `io.open` on a
missing file via `LuaIO` instead of `LuaTry`), the raw Lua error propagates
as a runtime crash. Use `LuaTry` for Lua functions that can fail, or wrap
calls in `try` to catch errors as `Either String a`.
