# Tracker performance regression: ST-monad closures

Investigation of why the tracker now decodes ~2.5× slower than the
figure documented in `TODO.md` ("121s of audio rendered in 113s,
0.94× real-time on fast machine"). The "fast machine" is the current
development machine, so hardware is **not** a variable — the entire gap
is code.

## Method

Same input every run: `HongKong_Music.it` (22 active channels, confirmed
from the IT header). Decoded to disk (2-arg `ctracker.lua` mode, so no
`sox` real-time throttling). Raw PCM throughput = output bytes ÷ 176400
(44.1 kHz × 2 ch × 16-bit). Each historical compiler was rebuilt in a
git worktree and used to compile the `tracker.mll` of *its own* commit.
**Every run produced byte-identical 135.5 s output (23,896,320 bytes).**

## Measurements (LuaJIT, this machine)

| Build | Commit | When | Wall | Real-time | Δ vs prev |
|---|---|---|---|---|---|
| eager ST (pre-fix) | `7915ed9` | Jun 17 | **65.9 s** | 0.49× | — |
| correct ST (closures) | `5886af8` | Jun 17 | **150.2 s** | 1.11× | **+128%** |
| pre-forcing baseline | `bbb6131` | session | 151.7 s | 1.12× | +1% |
| current HEAD | `eb7ff9e` | now | 161.9 s | 1.20× | +6.7% |

For reference: current under **plain Lua 5.5** is 352.9 s (2.61×).
Documented baseline (`cb3ce23`, Jun 11): 121 s audio in 113 s (0.94×).
That commit's `tracker.mll` no longer builds standalone (it references a
lib function `contains` added later), so the 113 s figure could not be
re-verified directly — but the eager-era 0.49× is consistent with, and
better than, the documented sub-real-time number. The doc was written in
the eager-ST era.

## Root cause

**`5886af8` "Make ST monad semantically correct: actions are closures,
not eager mutations" is ~85% of the regression — a 2.3× slowdown on the
hot STArray loop.** This session's forcing-correctness fixes are a
distant second (+6.7%); everything else is noise (+1%).

The decisive observation: the eager-ST output and the correct-closure
output are **byte-for-byte identical** for the tracker. The correctness
`5886af8` buys — actions that can be discarded, reordered, or duplicated
without running — is **never exercised by this program**. The tracker
runs each ST action exactly once, in order. We pay a per-action closure
allocation on every audio frame for a guarantee this code doesn't use.

### What the hot loop compiles to now

The bind-chain flattening already works (these are sequential `local`s,
not nested closures):

```lua
local c5 = __mll_run(__mll_ma_read(__force(arr), (((ch * fn94) + fn109))))
local _  = __mll_run(__mll_ma_write(__force(arr), (((ch * fn94) + fn97)), 0))
local _  = __mll_run(__mll_ma_write(__force(arr), (((ch * fn94) + fn99)), __force(inc)))
return     __mll_run(__mll_ma_write(__force(arr), (((ch * fn94) + fn103)), 1))
```

But each ST primitive **allocates a closure** that `__mll_run`
**immediately calls**:

```lua
local function __mll_ma_read(arr, idx)
    return function() return __force(arr)[__force(idx) + 1] end
end
local function __mll_ma_write(arr, idx, val)
    return function() __force(arr)[__force(idx) + 1] = __force(val) end
end
```

So per ST action, per frame, per channel (×22): one closure allocation +
one `__mll_run` dispatch (which re-introduces the `type(x) == 'function'`
check the README's "Eliminating `__mll_run`" section claimed was removed
— `5886af8` brought it back). That is the 2.3×.

## Rejected fix: user-facing strict-ST monad

A strict-ST variant is the wrong fix. It is a leaky abstraction: to use
it correctly the user must know that mata-ll compiles ST actions to Lua
closures, that the lazy default allocates one per action, and that
"strict" is only safe when actions run exactly once in order. That asks
the user to hold the compiler's internals and Lua's execution model in
their head to write a loop. It contradicts the goal of keeping the
Haskell surface clean and ownable. (Haskell's own
`Control.Monad.ST.Strict` is a rarely-reached subtlety for the same
reason.)

## Fix (implemented): codegen fusion of ST intrinsics at run-once call sites

Recover the performance in the **compiler**, with **zero user-facing
change**.

`__mll_run(__mll_ma_read(arr, idx))`, where the inner call is a *known
ST intrinsic* with a known body, can be emitted as the body directly:

```lua
local c5 = __force(arr)[idx + 1]    -- no closure, no __mll_run
__force(arr)[idx + 1] = 0
```

### Why this is safe without an analysis pass

The compiler **already proved run-once-ness**: it only emits
`local x = __mll_run(...)` / `return __mll_run(...)` for statements *in a
flattened do-block bind chain*, which is exactly the linearly-sequenced,
run-exactly-once position. The fusion fires only on the literal
syntactic shape `__mll_run(<direct call to a known ST intrinsic>)`.

First-class actions — stored in a structure, run conditionally via
`when`/`unless`, bound to a name and reused — never appear in that shape;
they show up as `__mll_run(some_var)` or `__mll_run(user_fn(...))`, which
the rule leaves untouched. The rule is conservative **by construction**,
not by a fallible proof.

### Properties

- **Zero user-facing change.** Normal ST code; surface stays clean.
- **Localized.** A peephole at the one codegen site that emits
  `__mll_run(...)` of a do-statement, special-casing the closed set of
  intrinsics: `ma_read`, `ma_write`, `ma_modify`, `ma_new`, `ma_length`,
  `ma_from_list`. Not a whole-program pass.
- **Verifiable.** Tracker output must stay byte-identical (reference
  output exists); ST test suite stays green; should recover most of the
  65.9 s → 150 s gap.

### Risk

Bounded but real: the danger is mis-classifying something as a known
intrinsic-in-run-once-position when it isn't, which would change effect
ordering/duplication. Contained by matching only the exact emitted shape
and the closed intrinsic set. The dangerous direction (as with all
strictness work) is being too aggressive — default to the closure form
whenever the shape doesn't match exactly.

### Smallest proving slice

Fuse just `ma_read` and `ma_write` (they dominate the loop), confirm
byte-identical tracker output against the reference, and measure. If it
recovers the bulk of the gap, extend to the rest of the intrinsic set.

## Result

Implemented exactly as proposed. `gen_action` now detects a fully-applied
ST intrinsic (`st_intrinsic_fused`) and emits a closure-free `__mll_st_*`
runtime function directly, dropping both the per-action closure allocation
and the `__mll_run` dispatch. First-class actions keep the `__mll_ma_*`
closure form. All 63 ST call sites in the tracker fuse.

A/B on this machine (LuaJIT, `HongKong_Music.it`, 2-arg disk mode):

| Build | User time | Real-time | Output md5 |
|---|---|---|---|
| closure (pre-fusion) | 152.5 s | 1.13× | `cdd386f6985dca3561fe1a2689231c78` |
| **fused** | **88.7 s** | **0.65×** | `cdd386f6985dca3561fe1a2689231c78` |

**1.72× faster, output byte-identical** (same md5, same 23,896,320 bytes).
This recovers ~76% of the 84 s ST-correctness regression (eager-era was
65.9 s) while keeping the closure semantics for first-class actions. The
tracker now runs at 0.65× real-time, comfortably under the documented
0.94×. The remaining ~23 s gap vs. the eager era is the single direct
call still present (vs. inline mutation) plus the run-once forcing fixes.

## Status

Implemented and verified. The ST test suite stays green (262 tests) and
the tracker output is byte-identical to the closure build. Scratch
worktrees and output removed.

# Future work: per-field (product) demand analysis

The tuple-field laziness fix (`218b660`, "make tuple fields lazy so bottom
in a tuple field is not forced") completes the "bottom is never forced
eagerly" contract, but costs tracker throughput. Measured on the CI gate
input (`benchmark.it`, LuaJIT, this machine):

| Build | Commit | Wall | Real-time (audio÷wall) |
|---|---|---|---|
| pre-tuple-fix | `c3cf855` | ~22.6–24.1 s | ~2.0× |
| **tuple fields lazy** | `218b660` | ~32.8–32.9 s | **~1.4×** |

A **~43% wall-time regression** (Fable measured ~57% on the larger
`HongKong_Music.it` under interleaved load — same effect). The CI perf gate
still passes comfortably: threshold is 0.5× real-time, HEAD is ~1.4×.

**Cause:** a single per-note thunk on a state-tuple field (`off + 1`) inside
an ST-action-returning function. `off` is provably forced on the taken path
(via the `marker` thunk in the same closure), but the current whole-value
demand analysis cannot mark it strict at construction — an ST action may be
built and discarded, so a tuple field that is unconditionally used *on every
run* is not, in general, forced. Emitting it eagerly anyway would reintroduce
exactly the bottom-in-a-tuple-field leak `218b660` removes.

**The sound recovery** is per-field (product) demand analysis: track demand
per tuple/constructor position rather than per whole value, so a field that
every use forces can be proven strict and emitted eagerly without weakening
the contract. This is a real analysis feature, not a codegen patch. It would
also subsume the ST-closure fusion special-case above with a general
mechanism. Deferred — the current cost is a soundness-preserving conservative
thunk, not a correctness hole, and the gate has ~3× margin.

# CI gate failure (0.1.3 cycle): non-strict `return` exposed a space leak

The `perf` CI job went red after `d3ef741` ("fix prefix/partial div/mod crash
and non-strict IO return"). Bisected on `benchmark.it` (LuaJIT, dev machine):

| Build | Commit | Wall | Peak RSS | Real-time |
|---|---|---|---|---|
| tuple-lazy baseline | `282e93b`…`f64337c` | 31.6–34.3 s | **19.9 MB** | ~1.4× |
| non-strict return | `d3ef741` → HEAD | 49.6–60.6 s | **4.26 GB** | ~0.7–0.9× |

NOT the tuple/cons laziness of `c3cf855`/`218b660` (that cost is the 1.4×
figure above, already absorbed). The generated tracker differed in only three
lines; patching them individually isolated the whole regression to ONE thunk:

    mixFrames mi arr 0 acc = return (bsConcatList (reverse acc))

With `return` correctly non-strict, the per-tick concat stays suspended and
retains the tick's entire per-frame cons chain (each element itself a thunk
over `ml`/`mr` arithmetic thunks) until `emitChunks` finally writes it — the
classic lazy-accumulator space leak. ~200× heap turns LuaJIT GC-bound: HEAD
retired FEWER instructions (512G vs 936G) yet ran 50% longer. On a CI runner
the 4.3 GB heap is disproportionately worse, which is what pushed the gate
under 0.5×.

**Not recoverable in the compiler.** Eagerly evaluating the returned value is
sound only if it is demanded before the action's result escapes; here the
demand comes from `emitChunks`, two callers up, through a list — an
interprocedural result-demand proof. GHC does not attempt it either:
GHC-compiled equivalent code has the identical leak, and the idiomatic fix
there is `return $!` / a bang. Per-field demand analysis (above) would not
help: the thunk is on a whole scalar result, and an A/B with only the cons
eagerized showed zero effect.

**Fix (implemented):** the same strictness a GHC program needs —
`mixFrames`' base case forces the concat (`pcm \`seq\` return pcm`).
Output byte-identical (md5 `1de1d8d6217f3009158f48350ccefc20`), back to
~28–35 s / 20 MB / 1.4–1.6× realtime. The benchmark workload and the 0.5×
gate are untouched.

The same commit's WHNF-assumption on `<-`-bound user-action results was a
separate *correctness* bug (miscompiled strict uses; fixed in codegen, see
CHANGELOG and test `action_result_whnf`).
