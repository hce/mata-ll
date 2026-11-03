# Optimization status board

State after round 11 (2026-09-03, dev @ e05194e; ratios are mll/twin,
min of 5 on the reference machine — see README.md for the raw walls).
list_pipeline re-measured 2026-11-03 (Q44 fix; the other rows were
unchanged within noise on the same run: 3.4/59.9/31.0/13.8/224/16.1/8.0x
on 5.5, 1.0/27.6/14.3/1.1/97.2/1.0/7.3x on LuaJIT).

| workload      | Lua 5.5 | LuaJIT | status                                                                  |
|---------------|--------:|-------:|-------------------------------------------------------------------------|
| integer_arith |    228x |    94x | capped by the always-boxed decision; Int is the escape hatch             |
| generics_json |     66x |    38x | off the agenda (row kept as documentation)                               |
| hm_churn      |     30x |    12x | residual: 2 runtime calls + 2 handle allocs per write — the handle IS the persistence |
| hm_lookup     |     17x |   1.1x | PUC call-overhead floor (JIT erases it)                                  |
| ioref_loop    |   15.5x |   1.0x | PUC call-overhead floor, halved in round 11                              |
| list_pipeline |    9.5x |   1.4x | fused, twin-shape loop; residual = __mll_rem call + interpreter overhead; re-measured 2026-11-03 after Q44 (the foldl' consumer had run unfused, 212x/76x, since 2026-09-03) |
| string_build  |    8.1x |   7.1x | cross-VM: real extra work — the one workload never directly attacked     |
| arith_loop    |    3.4x |   1.1x | the pure call-overhead floor itself                                      |

Round-over-round headline moves since the suite was built:
integer_arith 1436x -> 228x, list_pipeline 215x -> 9.5x,
hm_churn 803x -> 30x, ioref_loop 110x -> 15.5x,
generics_json 148x -> 66x, string_build 14.1x -> 8.1x.
