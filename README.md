Modest Attempt at Typesystem Augmenting the Lua Language (mata-ll)
==================================================================

mata-ll is a subset of haskell that is compiled directly into a single
Lua file with no external dependencies.

If you make a mistake, the compiler is already there to
stop you before any harm can spread to the runtime.

Calling from Lua into mata-ll:

| `callfib.lua` | `fib.mll` |
|---------------|-----------|
| <pre>local fib = require "fib"<br><br>local fibs = fib.fibonacci(8)<br>for i, n in ipairs(fibs) do<br>    print(i, n)<br>end</pre> | <pre>fib :: [Int]<br>fib = 1:1:zipWith (+) fib (tail fib)<br><br>export fibonacci :: Int -> [Int]<br>fibonacci = flip take fib</pre> |

Calling Lua library functions from mata-ll:

```haskell
rr :: LuaIO "math.random" Number
rr2 :: Int -> Int -> LuaIO "math.random" Int

main :: IO ()
main = do
    randNum <- rr
    putStrLn $ "A number between 0.0 and 1.0: " <> show randNum
    randNum2 <- rr2 23 42
    putStrLn $ "An integer between 23 and 42: " <> show randNum2
```

Passing Lua callbacks to mata-ll:

| `callwritefibs.lua` | `writefibs.mll` |
|---------------------|-----------------|
| <pre>local wf = require "writefibs"<br>local writer = function(fibString)<br>    print("From mata-ll:", fibString)<br>end<br>wf.writeFibs(writer, 12)</pre> | <pre>export writeFibs :: (String -> LuaIO s ())<br>                 -> Int -> LuaIO s ()<br>writeFibs writer = loop 1 1<br>  where<br>    loop _ _ 0 = return ()<br>    loop cur next count = do<br>      writer (show cur)<br>      loop next (cur+next) (count-1)</pre> |

Lua functions like `string.format` that accept a variable number of
arguments cannot be given a single type signature in mata-ll. Instead,
declare multiple monomorphic FFI bindings for the arities you need:

```haskell
fmt1 :: String -> String -> LuaPure "string.format" String
fmt2 :: String -> String -> String -> LuaPure "string.format" String
```

For dynamic formatting with a variable number of values or mixed types,
prefer mata-ll native constructs (`<>` and `show`) over `string.format`.
List functions like `intercalate` do not apply to `String`,
which is opaque rather than `[Char]`; to join a list of strings, fold
them with `<>`.

## Project goals

Make available a useful subset of modern haskell to Lua. It is not
intended to be a replacement for haskell, but rather as a way to write
haskell code where you would otherwise write Lua code.

Primary focus is on writing embedded code in a safer way than is
possible with Lua without breaking boundaries to Lua.

Specifically:

* Add the expressiveness, fun and safety of haskell to Lua
* Target Lua 5.4 and LuaJIT; compile to Lua source for safe loading via mlua
* Use non-strict evaluation; skip thunks only where that provably cannot
  change the result (bottom is never evaluated eagerly)
* Require no separately installed runtime: emitted files are
  self-contained. Abstractions are zero-cost where the compiler can
  prove them away; a small embedded runtime (laziness, IO) and
  library functions cover the rest
* Incorporate new type system research where possible and useful
* Once a stable version is reached, stay backwards compatible
* Have an easy interface to plain Lua
* Be portable and small; the compiler core -- the main part of this
  project -- shall not incorporate 3rd party rust libraries. Convenience
  wrappers around it (the `mll` CLI, the wasm playground) may use a few.
* Use rust's versioning for dependency resolving, not haskell's

## What's the difference between a runtime and library functions?

A library works within the semantics the language already has:
monads need only first-class functions and closures, and Lua has
both. A runtime provides semantics the language lacks, and needs
the compiler's cooperation, because the emitted code must take a
different shape: laziness needs thunks and forcing; green threads
would need compilation to a state machine or inserted yield
points. mata-ll's runtime is embedded in each emitted file --
there is nothing to install separately.

## Why rust, not C

While C may seem to be more portable, that is slowly changing: rust is
adding many targets, and for those, keeping C out is making the build
process more robust.

Since I think the combination of rust and Lua is a good one, one of
the primary goals of this project is to make the Lua part a bit 
more statically typed.

I miss writing haskell code but have mostly decided to do production
work in rust. A lot of "business logic" is hard to *write* efficiently
in rust, though, because of rust's focus on memory efficiency. Lua
fills that gap, but haskell could also fill it. However, a full blown
haskell stack has disadvantages:

Huge ecosystem that often experiences "dependency hell";
large dependencies for building, huge binaries generated;
hard to get it to interoperate with rust.

Besides, haskell and its ecosystem offer a full tooling suite, while
Lua is primarily focused on embedding. Using normal haskell would
increase complexity for any project embedding it, which is often not
feasible.

By writing the compiler in rust but targeting the Lua IR, I am hoping
to make it easier to write code that does not require the raw
performance that rust offers in a haskell-like language.

In addition, type safety allows to catch bugs during compile time,
which makes development with the help of an LLM much easier.

## Why rust, not haskell?

Because the project's purpose is to make haskell available where it
otherwise wouldn't be. Making ghc or another haskell compiler a
requirement would defeat that purpose.

## Language properties:

File extension should be .mll.

Each .mll file is a module, just like in haskell.

When compiling an .mll file, included .mll files will be merged into
the resulting output .lua file.

While the language targets the Lua VM and no additional
runtime is required, there is no need to stay closely compatible otherwise.
Interaction between mll and other Lua functions and modules
happens through well defined interfaces only.

For example, within mll scope, a Lua dictionary can and should be
reused to implement the haskell data construct.

For interacting with non-mll Lua, an FFI interface is provided.
This interface is used both to call into Lua as well as to export
functions to Lua.

## Evaluation strategy

MATA-LL uses non-strict evaluation, like Haskell. Function arguments
and let bindings are not evaluated until their values are needed.
This enables infinite data structures, avoids unnecessary computation,
and makes the language behave as Haskell programmers expect.

To avoid the overhead of thunking everything (and the classic space
leak in accumulator patterns), the compiler skips the thunk where it
can prove that is safe, under one normative rule: **bottom is never
evaluated eagerly.** An expression is evaluated eagerly only when
demand analysis proves the consumer forces it (this covers tail
accumulators, so accumulator loops build no thunk chain), or when it is
provably total — a literal, an already-forced variable, a constructor
or tuple of such, or non-trapping arithmetic over such. Everything else
is wrapped in a memoizing thunk. See the eagerness contract in
`doc/articles/SPEC.md` for the full rule.

For the rare cases where explicit control is needed, `seq :: a -> b -> b`
forces evaluation of its first argument before returning the second.


## Acknowledgements

This project was developed collaboratively by a human and an AI.
The design, direction and taste are Hans-Christian's; much of the
implementation was written by Claude (Anthropic). It came together over
about two weeks of close back-and-forth -- neither of us could have
built it alone.

## Contributing

If you are using and/or testing the mata-ll compiler: Thank you! If
you feel like it, you can send me a quick note via e-mail letting me
know that you're using it and what you are doing with it.

If you find bugs, do feel free to open an issue on github.

To discuss features and architecture ideas, please send an e-mail to
me directly (see git logs for e-mail address)

Please do not send in pull requests. If you would like something
changed, let's talk about it first.

Notwithstanding the above: By making a contribution, you agree to
license fully your contribution under the MIT License, the same
license that covers this project, and you acknowledge that you fully
own the copyright to your contribution and have the authority to
license it accordingly.

## Dependencies

So far, no dependencies (MLL libraries) are supported. I don't think
that's a primary scope for now. But once support is added, they should
be resolved in the rust way. Conflicting transitive dependencies must
not let a build fail; rather, version numbers should be part of the
internal symbols, so that an arbitrary number of conflicting versions
can coexist in parallel.


