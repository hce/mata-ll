A proof of concept decoder for IT (Impulse Tracker) music files. It
can feed the decoded raw audio stream to sox for real time playback
(well, not so much real time yet because it is as of yet still
slow) or alternatively write them to a raw audio stream on disk which
you can later play back with sox.

What is the ImpulseTracker format? It is the score and instruments
combined in one file. Though they are usually referred to as samples
or sound fonts, not instruments; in fact, you could consider them a
superset of instruments.

The ImpulseTracker format was used in the Deus Ex (1) and Unreal (1)
games, amongst others.

While Deus Ex music is partly ImpulseTracker, they are
wrapped in Unreal's proprietary, complex object file format with the
.umx extension. You will need to extract them from there first using
UnrealEd. Or, you use other tracker files. If you like this kind of
music, you'll know what to do. Much as I would like to ship these
files, can't, as they are copyrighted by their original authors.

## Compiler optimisations driven by the tracker

Building the tracker exposed several performance bottlenecks in the
MATA-LL compiler's code generation. The fixes are all generic — they
benefit any MLL program, not just the tracker.

### Cheapness analysis

The `is_cheap` predicate decides whether an expression is evaluated
eagerly or wrapped in a `__thunk` closure for lazy evaluation. Several
expression forms were previously considered expensive when they are
actually free or near-free:

- `div` and `mod` — integer division and modulo were treated as
  unknown function calls instead of builtin Lua operators (`//`, `%`).
- Tuple construction — `{a, b}` is just table creation, same cost as
  a constructor application.
- `if` expressions — cheap when all three sub-expressions (condition,
  then, else) are cheap. This matters for inline sample-value
  calculations in tight loops.
- Function application — `f(x, y)` is cheap when both `f` and its
  arguments are cheap (variables, literals, other cheap expressions).
  This eliminated thunks around index computations like `fi(ch, fiVol)`.

### Concrete variable tracking

The compiler tracks which variables are known to hold concrete (forced)
values, so it can skip redundant `__force()` calls. Several
improvements were made:

- All function parameters are forced once at entry and marked
  concrete, eliminating repeated `__force` calls throughout the body.
- Top-level bindings (functions, cheap values) persist as concrete
  across function boundaries, so references to e.g. `fi`, `readSmp`,
  `advPos` inside inner loops don't need forcing.
- Self-recursive function names are marked concrete inside their own
  body, so tail calls like `mixFrame(fd, arr, ...)` skip the force.
- Monadic bind continuations (`>>=` with a lambda) mark their
  parameters concrete, since `__mll_run` always produces a forced
  value.

### Monadic bind chain flattening

Do-notation desugars through `>>=`, which previously generated nested
closures — one per monadic bind. For the inner mixing loop with 7
`readSTArray` calls, this meant 7 nested closure allocations per
channel per audio frame.

The codegen now detects chains of `>>=`/`>>` with lambda
continuations and flattens them into sequential `local` statements
inside a single immediately-invoked function:

```lua
-- before: 7 nested closures
(function(pos)
  return (function(sl)
    return (function(dp)
      ...
    end)(__mll_run(readSTArray(arr, fi(ch, fiDPtr))))
  end)(__mll_run(readSTArray(arr, fi(ch, fiLen))))
end)(__mll_run(readSTArray(arr, fi(ch, fiPos))))

-- after: 1 IIFE, 0 nested closures
(function()
  local pos = readSTArray(arr, fi(ch, fiPos))
  local sl = readSTArray(arr, fi(ch, fiLen))
  local dp = readSTArray(arr, fi(ch, fiDPtr))
  ...
end)()
```

`let` bindings inside do-blocks are also folded into the flat chain
rather than generating their own IIFEs.

### Eliminating __mll_run

`__mll_run` was a runtime function that used Lua type introspection
(`type(x) == "function"`) to decide whether an IO/ST action needed
calling. The compiler now uses type information at compile time
instead: function applications and intrinsics are emitted directly,
while bare references to zero-arg IO/ST bindings get an explicit `()`
call. This removed all `__mll_run` calls from the hot path.

### bsConcatList intrinsic

`bsConcatList :: [ByteString] -> ByteString` joins a list of
ByteStrings via Lua's `table.concat` in O(n) time. The tracker's
audio mixing loop previously accumulated PCM data with repeated
`bsConcat` (O(n^2) copying); it now collects chunks in a list and
joins them in one pass.
