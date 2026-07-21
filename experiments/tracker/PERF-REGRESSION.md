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

# 3.8× regression (July 2026): thunks in the mixer blacklist the LuaJIT trace

Current HEAD decodes `HongKong_Music.it` in **338 s wall (2.50× real-time)**
against the fused baseline's **88.7 s (0.65×)** documented above — a 3.81×
slowdown. Output is byte-identical (same 23,896,320 bytes, same md5
`cdd386f6985dca3561fe1a2689231c78`) and peak RSS is a healthy 78 MB, so this
is purely speed, not a leak and not a miscompile.

## Not the ST-closure regression again

First suspicion was a fusion break. It isn't: the generated tracker has **33
fused `__mll_st_*` call sites and 0 first-class `__mll_ma_*` sites in the hot
path** — every ST operation in the mixer still compiles to the closure-free
fused form. The ST work above is intact.

## Mechanism: `NYI: bytecode FNEW` aborts, then blacklisting

`luajit -jv` shows the mixing loop's traces aborting with `NYI: bytecode
FNEW` — closure allocation inside the loop — **766 times**, at the `mixFrame`
hot lines, after which LuaJIT **blacklists** the loop. The entire mixer then
runs in the interpreter. The arithmetic confirms we lost exactly the JIT and
nothing else:

- current LuaJIT wall 338 s ≈ the 352.9 s documented above for **plain
  Lua 5.5** on the same workload;
- the 3.81× regression ≈ the 3.98× JIT multiplier (352.9 / 88.7).

The closures are `__thunk(function() … end)` allocations for the mixer's
`let` bindings — per binding, per channel (×22), per audio frame (×44100/s).

## The thunk cascade

`mixFrame`'s arithmetic bindings (tracker.mll ~305–370):

```
let smp = if smpPos < sl then readSmp … else 0     -- call ⇒ not cheap ⇒ thunk
let sv  = if is16 == 1 then (smp*vol*gvl*128) `div` … else …
let nl  = la + (sv * (64 - pan)) `div` 64
let nr  = ra + (sv * pan) `div` 64
mixFrame mi arr (ch+1) nl nr                       -- la/ra ARE last frame's nl/nr
```

`sv`/`nl`/`nr` are `is_cheap` (small to evaluate) but not `is_cheap_to_force`
(safe to evaluate *now*): they transitively read the `smp` thunk, and their
`div` can trap. Nor can the demand analysis prove them demanded: they flow
into the recursive call's `la`/`ra`, and `mixFrame`'s base case returns those
in a **lazy tuple field** through a **non-strict `return`** — under
whole-value demand analysis that is "not demanded", so the sound weighing
thunks them. Since `la`/`ra` are the previous iteration's `nl`/`nr`, the
entire per-frame state chain turns lazy. The same pattern thunks `fPos` in
`advPos` (its value feeds `writeSTArray`, whose demand mask says the stored
value is lazy — even though the fused runtime `__mll_st_write` forces on
store) and `smpPos` (`div` is conservatively trapping even with the literal
divisor 256).

## Attribution (bisected)

The primary regression commit is `93060aa` "codegen+demand.rs: weigh eager vs
lazy so bottom is never forced". Its parent `341b878` emits `smpPos`, `smp`,
`sv`, `nl`, `nr`, and `fPos` **eagerly**; `93060aa` flips them all to
thunks. Measured on this machine, this input:

| Build | Wall | Real-time | Peak RSS | Output md5 |
|---|---|---|---|---|
| `341b878` (pre-weighing) | **101 s** | 0.75× | 14.7 MB | `cdd386f…` |
| current HEAD | **338 s** | 2.50× | 78 MB | `cdd386f…` (identical) |

This is *not* the tuple-field laziness of `218b660` (that came later and does
not change these bindings), and it is not a new soundness bug — `93060aa`'s
weighing is correct, merely too coarse: the old build evaluated these
bindings eagerly *without* a proof, which is exactly the hole the commit
closed. A secondary, much smaller cost is `d3ef741` turning inline
`math.floor(x/y)` into `__mll_div(x, y)` calls; it is not the JIT-killer.

## The sound fix: per-field (product) demand analysis

This is the pervasive form of the "Future work: per-field (product) demand
analysis" item above — no longer one per-note thunk but the whole hot loop.
A binding like `nl` **is** forced on every run: the caller (`mixFrames`)
scrutinizes the result tuple and forces both fields into every emitted PCM
frame, and the concat that `seq` forces at the end of every tick forces every
frame. Proving that requires tracking demand **per tuple/constructor field
and per list element** (plus applying the fused `__mll_st_write` runtime's
actual on-store forcing to the stored-value position), so that a value every
use forces is emitted eagerly at its binding — recovering `341b878`'s
emission for the mixer *with* a proof, without weakening the ⊥-preservation
contract anywhere it actually protects something.

## Result (implemented)

Per-field / per-element demand analysis is implemented in `demand.rs` as a
structured demand domain (`Head` / `Fields` / `Elems`) with per-function
demand ROWS — the demand each parameter receives when the function runs, and
a second row under a "result deeply forced" assumption — plus a
whole-program `deep_result` set: functions for which EVERY reference is a
fully-applied call whose result provably receives the deep demand. Codegen
seeds its demanded-binding decisions from these rows, threading the current
function's proven result demand into chain terminals. The proof chain for
the mixer lands exactly as sketched: `bsConcatList` is element-strict (its
runtime forces every element) → `reverse` transmits element demand through
its `go` accumulator (local where-functions get their own row fixpoint) →
`mixFrames` is element-strict in `acc` → `pcm`, `ml`, `mr` are demanded →
both fields of `mixFrame`'s result tuple are forced at its only external
call site → `mixFrame` is deep-result → `nl`/`nr`/`sv`/`smp`/`smpPos` are
demanded and emitted eagerly. `fPos` follows from aligning the fused
`__mll_st_write` mask with the runtime's on-store force, and the loop
counter `ch` from seeding the resolved primitive `eq_*`/`ord_*` instance
methods (which codegen inlines as Lua comparison operators) as strict —
they were invisible to the analysis, hiding every guard's operands.

Nothing is emitted eagerly without a per-path proof: a call site that
forces only one field, a spine-only consumer (`length`), a suspended
first-class action, or a partial application all degrade the claim
conservatively (verified by targeted ⊥ probes and the suite's pinned
contract tests — `return_non_strict`, `div_mod_by_zero_raises`,
`div_exact_and_zero`, exceptions — all green; 550 passed / 0 failed).

A/B on this machine (LuaJIT, `HongKong_Music.it`, 2-arg disk mode):

| Build | Wall | Real-time | Peak RSS | Output md5 |
|---|---|---|---|---|
| regressed HEAD (before) | 338 s | 2.50× | 78 MB | `cdd386f…` |
| per-field demand | 120 s | 0.89× | 15.0 MB | `cdd386f…` (identical) |
| **+ redundant-force fix** | **102 s** | **0.76×** | **15.0 MB** | `cdd386f…` (identical) |
| `341b878` eager reference | 101 s | 0.75× | 14.7 MB | `cdd386f…` |

3.3× faster than the regression, byte-identical, and the 5× thunk memory
bloat is fully recovered. `luajit -jv` confirms the mixer's arithmetic lines
no longer abort traces. The demand fix alone left a ~19% gap to the eager
build; the residual investigation below traced it — NOT to `d3ef741`'s
`__mll_div`/`__mll_mod` calls (measured ~2 s) but to redundant `__force`
emission — and a follow-up codegen fix closed it, landing at the eager
build's 101 s.

One latent bug surfaced and fixed along the way: the demand analysis'
fixed-point env is name-keyed, and a user function SHADOWING a prelude name
(the FFI test suite's `replicate`) made two bodies fight over one entry —
harmlessly converging while both computed identical rows, oscillating
forever once the new seeds made them differ. Same-named functions now share
the MEET of their rows (a call site cannot be attributed to one of them),
which is also the sound semantics.

# Residual to the eager build: redundant `__force` (fixed)

The demand fix left ~19 s between HEAD (120 s) and the `341b878` eager
reference (101 s). Triangulating with a LuaJIT `-jp` profile plus A/B code
diffs against the eager build (same source for `advPos`/`mixFrame`) placed
it precisely:

- **Profile:** 38% `__mll_run` (the ST-action `type(x)=="function"` dispatch)
  + 27% `__force` — two-thirds of runtime is machinery, not mixing math.
- **NOT div/mod:** stripping `__mll_div`/`__mll_mod` to branchless inline on a
  copy saved only ~2 s. In a traced loop LuaJIT already inlines the
  monomorphic call and constant-folds the `b == 0` guard (every hot divisor
  is a literal), so that earlier `~19% is d3ef741` guess was wrong.
- **NOT the `when`→`__mll_run` path:** byte-identical in the eager build, so
  it is inherent ST-monad overhead, not a regression.
- **The real cause — redundant forcing.** vs the eager build (same source)
  the generator emitted 349 `__force(` sites vs 219, and 57
  `__force(__force(x))` doubles vs 20. `__force` is idempotent, so
  `__force(__force(ch))` on a parameter already forced at entry is pure
  waste. Collapsing just the simple doubles on a copy (byte-identical)
  recovered ~7 s.

Root cause: `b0c9c5f` threaded demand-driven eager emission through codegen,
but the knowledge of what `gen_expr`'s own output already guarantees stayed
local to `gen_operand`. Every other "I need a forced value here" site — and
especially the inline/substitution path `gen_operand_subst` — blindly
wrapped `__force(` around `gen_expr(...)`, which itself already emits
concrete vars bare and non-concrete vars as `__force(x)`, producing the
doubles and re-forcing known-concrete values.

Fix: a single `gen_expr_yields_whnf` predicate (the one source of truth for
"this emission provably evaluates to WHNF" — literals, concrete/singly-forced
vars, constructors, tuples, native-operator infix, record projections,
inlined primitive eq/ord), plus `gen_forced`/`gen_forced_subst`/
`gen_forced_prefix` helpers that force exactly once and never wrap a
WHNF-yielding emission. Every blind wrap site routes through them and falls
back to the old wrapper whenever WHNF cannot be proven, so no load-bearing
force is dropped. Result: zero simple `__force(__force(x))` doubles remain,
tracker 120 s → 102 s (byte-identical), suite 550 / 0.
