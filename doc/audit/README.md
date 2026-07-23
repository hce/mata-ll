# Audit findings — reproductions (2026-07)

Minimal, independently verified reproductions of bugs found by an adversarial
per-feature audit. **All of these are now fixed** (2026-07); this directory is
kept as a historical record of what the audit found. Each repro is guarded by a
regression test in `mll-tests/tests/run_mll.rs`, and every one of those tests
was confirmed to fail on the pre-fix compiler.

Because the bugs are fixed, running a repro today shows the *corrected*
behavior — an error where the compiler now rejects the program, or the right
result — not the original defect. The "Was (pre-fix)" column records the
original buggy behavior.

Reproduced by hand with `./target/debug/mll` (the `-r` runner, or `-e` + a
`caller.lua` host under `lua`/`luajit`).

| Repro | Bug | Was (pre-fix) | Regression test |
|-------|-----|---------------|-----------------|
| `t1-instance-dispatch-ignores-args.mll` | Instance resolution keyed on the outer constructor, ignoring arguments (**soundness**) | ran the `[Int]` body on a `[Bool]` | `argument_specialized_instance_head_rejected` |
| `t2-instance-overlapping-heads.mll` | Two element-specialized heads (**soundness**) | both calls picked the last-declared instance | `overlapping_instances_rejected` |
| `t3-instance-duplicate.mll` | Duplicate instance silently accepted | printed `second`, no error | `duplicate_instance_is_hard_error` |
| `t4-caf-self-ref-truncates.mll` | Self-referential value CAF lost the self-reference (**soundness**) | `[1]` instead of `[1,2,3,4]` | `self_referential_caf` |
| `t5-caf-self-ref-crash.mll` | Same, with a user constructor | runtime crash | `self_referential_caf` |
| `t6-take-0-too-strict.mll` | `take 0` forced the list | crashed instead of `[]` | `lazy_take_zip` |
| `t7-typefamily-clause-priority.mll` | Closed-family catch-all beat the specific clause | `F 'Z` reduced to `String` | `type_family_clause_priority` |
| `t8-typefamily-growing-hangs.mll` | Growing family hung the compiler | hang / stack overflow | `growing_type_family_is_bounded` |
| `t9-luatry-no-decode.mll` (+ `-caller.lua`) | `LuaTry` did not decode its payload | raw array walked as a cons cell → crash | `luatry_success_payload_decodes_and_error_is_stringified` |

## Notes
- `t1`–`t3` were one bug (instance table keyed by outer constructor with silent
  overwrite); the fix rejects overlapping / duplicate / argument-specialized
  heads at declaration, like GHC.
- `t4`/`t5` were the "`local x = <expr reading x>` reads outer/global `nil`" Lua
  scoping gotcha on the by-name self-reference path.
- `t9` was one instance of a wider finding — several FFI boundaries did not run
  the type-directed decoder; the fix made the marshalling boundary uniformly
  type-directed.
- `t8`: the fix bounds the reduction *work* (it now reports divergence instead
  of hanging) but not recursion *depth*, so the reduction still needs a large
  stack — fine from the CLI, and the regression test runs it on a 32MB-stack
  thread.

Run e.g.: `./target/debug/mll -r doc/audit/t4-caf-self-ref-truncates.mll`
