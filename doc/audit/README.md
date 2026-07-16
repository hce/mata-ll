# Audit findings — reproductions (2026-07)

Minimal, **independently verified** reproductions of bugs found by an adversarial
per-feature audit. These are NOT wired into the cargo test suite — **each one
currently FAILS on the compiler and demonstrates an open bug.** They are being
fixed; as each fix lands, its repro should become a proper regression test in
`mll-tests/tests/run_mll.rs` and be removed from here.

Every entry below was reproduced by hand with `./target/debug/mll` (the `-r`
runner, or `-e` + a `caller.lua` host under `lua`/`luajit`).

| Repro | Bug | Expected | Observed | Severity |
|-------|-----|----------|----------|----------|
| `t1-instance-dispatch-ignores-args.mll` | Instance resolution keys on the outer type constructor only, ignoring arguments | no-instance error for `Pretty [Bool]` | runs the `[Integer]` body on a `[Bool]` | **soundness** |
| `t2-instance-overlapping-heads.mll` | Two element-specialized heads (`Pair Integer Integer` / `Pair Bool Bool`) | each call picks its own instance | both print `bools` (last-declared wins) | **soundness** |
| `t3-instance-duplicate.mll` | Duplicate `instance Greet Integer` | duplicate-instance compile error | silently prints `second` | soundness / hygiene |
| `t4-caf-self-ref-truncates.mll` | Top-level self-referential value CAF whose RHS is not `:`/`++`-headed | `[1,2,3,4]` | `[1]` (self-reference is `nil` → silent truncation) | **soundness** |
| `t5-caf-self-ref-crash.mll` | Same, with a user constructor (`s = S 1 s`) | `1` | runtime crash (`index a nil value`) | soundness |
| `t6-take-0-too-strict.mll` | `take 0 (error …)` | `[]` | forces the list → crash | strictness deviation |
| `t7-typefamily-clause-priority.mll` | Closed type-family clause priority (`F 'Z = Integer; F n = String`) | `F 'Z` = `Integer` | reduces to `String` (catch-all beats the specific clause) | high (wrong reduction) |
| `t8-typefamily-growing-hangs.mll` | Growing divergent family (`Grow x = Grow (Maybe x)`). **WARNING: hangs the compiler — do not compile without a timeout.** | `TypeFamilyDivergence` error | compiler hangs / stack-overflows | high (compiler DoS) |
| `t9-luatry-no-decode.mll` (+ `-caller.lua`) | `LuaTry` does not decode its success payload | `Right [1,2,3]` → `sum` = `6` | raw Lua array walked as a cons cell → `index a number value` crash | high (FFI) |

## Notes
- `t1`–`t3` are one bug (instance table keyed by outer constructor with silent overwrite).
- `t4`/`t5` are the classic "`local x = <expr reading x>` reads outer/global `nil`" Lua scoping gotcha on the by-name self-reference path; `:`/`++`-headed self-refs and mutual CAFs already work — the regression test that exists uses exactly the `:`-headed shape that works.
- `t9` is the same class as the (already-fixed) LuaIterator element-decode bug: several FFI boundaries don't run the type-directed decoder uniformly (`LuaTry` payload, export args, callback state, `LuaCatch` `Left`).

Run e.g.: `./target/debug/mll -r doc/audit/t4-caf-self-ref-truncates.mll`
