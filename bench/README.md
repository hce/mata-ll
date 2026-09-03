# Speed-of-light benchmarks

Each workload in `cases/` is idiomatic mata-ll; its twin in `twins/` is
the same job handwritten the way a Lua programmer would write it. The
harness times both (CPU time via `os.clock`, minimum over several
processes) and reports the ratio mll/twin per workload — the distance
between the generated code and the speed of light for that job. The
ratios rank optimization work; they are measured locally, not gated in
CI (machine variance would make a threshold dishonest — the CI speed
gate is `mll-tests/perf-test.sh`).

A twin must print byte-identical stdout to its mll program. The harness
enforces this, so a twin that drifts from its workload fails the run
instead of skewing the ratio. Workloads print checksums or lengths, not
bulk data, and keep every printed number well inside exact-double range
so PUC Lua (integers) and LuaJIT (doubles) format them identically.

Run:

    cargo build --release
    bench/run.sh                  # lua5.5
    bench/run.sh lua5.5 luajit    # both VMs

`BENCH_ONLY=name` runs one workload, `BENCH_RUNS=N` sets the runs per
program (default 3, minimum taken), `MLL=` overrides the compiler.

Workloads:

| workload      | measures                                              |
|---------------|-------------------------------------------------------|
| arith_loop    | Int tail recursion — pure call/arith overhead floor   |
| list_pipeline | lazy map/filter/foldl' vs a fused Lua loop            |
| string_build  | mconcat string building vs table.concat               |
| hm_lookup     | hmLookup hammer on a static map vs table reads        |
| hm_churn      | interleaved persistent insert/delete vs table mutation |
| integer_arith | always-boxed Integer vs native numbers (small values) |
| generics_json | derived ToJSON encoding vs handwritten concatenation  |
| ioref_loop    | modifyIORef' in an IO loop vs a local-variable loop   |

Baseline ratios on the reference machine (2026-09-03, Apple Silicon,
min of 5; after the site-forced calling convention, the WHNF-return
claim for direct calls, the closure-free IO self-loops, the
mconcat@String builder, the HashMap strictness rows, the closure-free
thunk representation, the case-of-hmLookup fusion, the list-pipeline
fusion with inlined stage/fold bodies, the persistent diff+reroot
HashMap representation, and the exact-first-force let eagerization
plus hm-runtime entry fast paths — the
pre-round numbers were string_build 14.1/16.3, arith_loop 22.5/1.0,
ioref 110/27, generics 148/54, list_pipeline 215/179,
integer_arith 1436/717, and the RETIRED original hm_churn 59/39,
which bundled build + lookups + deletes and was 81% lookup wall time;
it split into hm_lookup and a rebuilt hm_churn that actually churns):

| workload      | Lua 5.5 | LuaJIT  |
|---------------|--------:|--------:|
| list_pipeline |    8.6x |    1.4x |
| arith_loop    |    3.4x |    1.1x |
| ioref_loop    |   15.5x |    1.0x |
| string_build  |    8.1x |    7.1x |
| hm_lookup     |   17.0x |    1.1x |
| generics_json |   66.0x |   37.7x |
| integer_arith |  227.9x |   94.0x |
| hm_churn      |   29.6x |   12.2x |

list_pipeline's walls after fusion are 0.010s (5.5) and 0.001s
(LuaJIT), from 0.213s/0.031s before it. The fused loop now inlines the
stage and fold BODIES (`a = (a + x0) % p`, the twin's own shape); the
residual 5.5 ratio is the `__mll_rem` helper call per element (`rem`
is truncated where Lua's `%` floors, so it cannot lower to the bare
operator) plus interpreter loop overhead.

The diff+reroot HashMap representation (one mutable store per version
family, old versions replayed on demand) took hm_churn from 803x/834x
down to 56x/44x (walls 0.46s -> 0.036s/0.012s; the LuaJIT twin is
sub-millisecond, so that ratio is wall-noise-dominated) and hm_lookup
from 28x/7.4x — reads on the newest version are a raw index behind one
root check, which LuaJIT hoists out of the loop.

The exact-first-force eagerization then took hm_churn to the numbers
above (walls 0.017s/0.003s): the loop's derived-map bindings are
provably forced by the case scrutinee before anything else can bottom,
so they emit as direct eager calls — no thunk, no closure, no re-force
— and the same analysis halved ioref_loop on PUC. The residual
hm_churn cost is the per-write handle allocation (a persistent map
version IS a fresh handle) plus the runtime call per operation; the
write bodies now take the newest-version store behind one inline check
(the shape lookups always had), gate their entry forces on the thunk
metatable, and an Int literal pattern (the loop's `churn 0 acc m` base
case) compares with native `==` instead of a helper call.
