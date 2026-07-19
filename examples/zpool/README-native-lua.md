# `zpr-native-lua.lua` — a hand-written pure-Lua twin, and a runtime-cost analysis

`zpr-native-lua.lua` is a from-scratch reimplementation of the ZFS pool reader
(see [`README.md`](README.md)) in **pure Lua 5.4**, written directly against the
same on-disk format the `.mll` modules parse — not a translation of the compiled
output. It exists as a controlled comparison point: same algorithm, same parsing,
byte-identical results, but none of the mata-ll runtime. Diffing it against the
mata-ll build isolates what the compiler's runtime model costs, separately from
the algorithm.

Both files ultimately trace back to the same author working from the same spec;
"native-lua" names what it is — hand-written idiomatic Lua rather than generated.

## Requirement: Lua 5.4, not LuaJIT

Same constraint as the mata-ll reader, for the same reason: ZFS is full of 64-bit
on-disk values, and this needs native 64-bit integers and 64-bit bitwise
operators. It uses only Lua-5.4-stable features (native integers, `//`, bitwise
ops, `string.unpack`). It does not run correctly on LuaJIT.

## The comparison

Run against a 100 MB test pool image (`/var/tmp/bar.img`) holding a copy of the
mata-ll source tree — 464 regular files extracted:

| Build | Runtime model | LZ4 output algorithm | Wall-clock |
|---|---|---|---|
| `zpr-native-lua.lua` | native Lua 5.4 | buffered (chunk list + `table.concat`) | **~10 s** |
| `zpr.lua` (compiled from the `.mll` sources by `mllc`) | lazy: thunks + typeclass dictionaries | buffered — **the same algorithm** | **~13 min** |

- **stdout identical** — same dataset list, file list, per-file byte counts.
- **all 464 extracted files byte-identical** (`diff -r` clean).
- **~78× wall-clock difference.**

The two builds run the *same* on-disk traversal and the *same* LZ4 window/pending
buffering strategy (the compiled version is `mllc`'s output for `Lz4.mll`, which
already buffers). So the ~78× is **not algorithmic** — it is the cost of the
runtime model the compiler emits.

## Where the ~78× goes

This is a **structural analysis from reading the compiled `zpr.lua` and the
runtime it embeds** (`mllc/src/codegen.rs`), not a profiler decomposition. The
one experiment that would have measured the split empirically (a deliberately
un-buffered variant) was confounded — it changed the *algorithm* rather than the
runtime, so it was aborted. Treat the percentages below as reasoned estimates,
ranked by confidence, not measurements.

1. **Per-iteration thunk allocation in strict glue (~40–50%, largest).**
   The hot path is byte-level parsing (every 128-byte block pointer field by
   field, every dnode, ZAP entry, indirect-block walk, and LZ4's byte copy).
   Each recursive step in code like `groupBE`'s `beInt`, `cstringAt`'s `scan`,
   LZ4's `extend`/`go`, and `readBlock`'s `descend` heap-allocates closures and
   thunk tables to defer arithmetic (`off+1`, `k-1`, `acc*256 + …`) that is then
   forced one line later. The native version does these as plain locals in a
   `while` loop — zero allocation. The compiled file carries hundreds of
   `__thunk(` / `__force(` sites for a program that needs neither.

2. **Force / dispatch overhead around every primitive (~30–40%).**
   Every ByteString op compiles to `__force(__force(__mll_bs[N])(args))` — a
   table index, a wasted `__force` on a known function, the call, then each
   argument `__force`'d again inside. Every bitwise op is a wrapper call rather
   than the native operator it ultimately runs. One `getU64BE` — a single
   `string.unpack(">I8")` C-call in the native version — becomes a tree of a
   dozen-odd Lua calls, table indexes, and force checks.

3. **ByteString copying — real, but mostly *not* a differentiator.**
   `bsSub`/`bsConcat` allocate and copy. But the native version also backs
   ByteString by immutable Lua strings and slices with `string.sub`, so the copy
   cost is largely *shared* between the two builds. What mata-ll adds on top is
   the force/dispatch wrapper around each copy (counted in #2), not extra copies.
   Little of the ~78× is copying as such.

4. **Typeclass dictionary indirection — near-zero here (<5%).**
   The hot byte-readers are monomorphic `Integer`/`ByteString`; no dictionaries
   flow through them. Dictionaries appear only in cold spots (`Ord` for `sortBy`,
   `Show` for error strings). A genuine cost in general, but not where this
   reader's time goes.

## Inherent vs. unrealized optimization

Most of the ~78× reads as **missing optimization, not a floor inherent to
compiling a lazy typed language to Lua**:

- **Thunk allocation for strict arguments (#1) — not inherent.** The deferred
  values are demanded immediately; a demand/strictness pass would emit them
  strictly and delete the per-iteration closures. `mllc` already has a demand
  analysis (`demand.rs`); the work is extending its reach into
  arithmetic-argument positions.
- **Redundant forces (part of #2) — not inherent.** Forcing a known primitive
  function, or re-forcing a value already forced in scope, is dead work a
  peephole/liveness pass removes.
- **Primitives as wrapper calls (#2) — not inherent.** Where arguments are known
  monomorphic, inline to native `&` / `<<` / `+` instead of the wrapper.
  Monomorphism is already established (`mono.rs`); this is codegen using it.
- **Shared-backing ByteString slices and a mutable byte-buffer builder —
  genuinely intrinsic improvements with real semantic surface.** A slice-view
  (offset+len instead of a copied string) breaks the invariant "a ByteString *is*
  a Lua string" — every FFI/`bsToString` consumer must then materialize. And the
  builder's upside is bounded here since the LZ4 path already buffers. Do these
  last, if at all.

Estimate: strictness-driven thunk elision + redundant-force removal + primitive
inlining would likely land this workload around **5–15× native**, not 78×.

### A note on the bitwise path

Inlining bit-ops to native Lua operators is **safe**, not a parity risk: `LBit`
is a deliberately Lua-semantics FFI module (the `L`-prefix family), not
`Data.Bits` parity, and `mllc` already emits native operators for it on Lua
5.3+/5.4. `shiftR` is logical (zero-fill), and shift counts ≥ 64 yield 0 — Lua's
semantics, which the reader depends on. This contract is pinned by
`mll-tests/tests/cases/lbit_64bit_boundary.mll` (added as a guard before any such
optimization work), alongside a strictness guard
(`lbit_strict_primitive_arg.mll`) proving a bit-op can't skip forcing a thunked
operand, and a sign-bit 64-bit read test (`bytestring_u64_sign_bit.mll`).

## Is ~78× a fair price?

For what the mata-ll build buys — a byte-level binary parser written in a typed
Haskell subset, with linear IO handles that *prove at compile time* the output is
written and closed exactly once, GHC-parity semantics, and single-file Lua
output — ~78× on a cold, occasionally-run extraction tool (10 s → 13 min) is a
lot but tolerable; the reader's value is correctness and maintainability, not
throughput. On a hot path it would not be. But the number reads as unrealized
optimization rather than the price of the abstraction, and the highest-value
fruit (thunk elision, redundant-force removal, primitive inlining) extends passes
`mllc` already has.
