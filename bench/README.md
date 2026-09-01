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
| hm_churn      | persistent hmInsert/hmLookup/hmDelete vs table mutation |
| integer_arith | always-boxed Integer vs native numbers (small values) |
| generics_json | derived ToJSON encoding vs handwritten concatenation  |
| ioref_loop    | modifyIORef' in an IO loop vs a local-variable loop   |

Baseline ratios on the reference machine (2026-09-01, Apple Silicon,
min of 5; after the site-forced calling convention, the WHNF-return
claim for direct calls, the closure-free IO self-loops, and the
mconcat@String builder — the pre-round numbers were string_build
14.1/16.3, arith_loop 22.5/1.0, hm_churn 59/39, ioref 110/27,
generics 148/54, list_pipeline 215/179, integer_arith 1436/717):

| workload      | Lua 5.5 | LuaJIT  |
|---------------|--------:|--------:|
| arith_loop    |    3.1x |    1.0x |
| ioref_loop    |   30.7x |    1.1x |
| string_build  |    6.9x |   11.6x |
| hm_churn      |   55.0x |   36.0x |
| generics_json |   61.4x |   61.2x |
| list_pipeline |  167.6x |  139.0x |
| integer_arith |  222.0x |  101.8x |
