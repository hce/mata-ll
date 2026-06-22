# Why the `.umx` decode is much slower than the `.it`

Follow-up to `PERF-REGRESSION.md`. That document benchmarks
`HongKong_Music.it`. The bisect script (`bisect-perf.sh`) and casual runs use
`HongKong_Music.umx`, which is far slower (≈1400s under plain Lua 5.5, ≈624s
under LuaJIT). This note explains why — short version: it is a **different,
longer arrangement**, and the per-pattern cost is **JIT-cold**, not a memory or
GC problem.

## The two files are not the same song length

Both are titled "Hong Kong Streets" and share the same 28 samples and 63
patterns, but the order list (the play sequence) differs:

| File | Container | Orders | Audio rendered | Output bytes |
|---|---|---|---|---|
| `HongKong_Music.it`  | raw IT                    | **16** | 135.5s | 23,896,320 |
| `HongKong_Music.umx` | Unreal package (+wrapper) | **46** | 533.5s | 94,116,652 |

The `.umx` is a 169-byte Unreal package header + the embedded IT module + a
trailer; `tracker.mll`'s `findIMPM` (≈line 445) scans for the `IMPM` magic and
decodes from there, correctly. The `.umx` simply sequences the patterns into a
~3× longer arrangement, so it renders ~4× the audio. Producing 4× the audio is
the bulk of the extra wall-time. (Both modules are copyrighted; not committed.)

## Throughput, by interpreter

Audio seconds = output bytes ÷ 176400 (44.1 kHz × 2ch × 16-bit).

| Run | Interpreter | Wall | ×realtime |
|---|---|---|---|
| `.it`  | LuaJIT   | 106.7s | 0.79× |
| `.umx` | LuaJIT   | 624.3s | 1.17× |
| `.umx` | Lua 5.5  | 1400.9s | 2.63× |
| `.it` (PERF-REGRESSION.md) | Lua 5.5 | 352.9s | 2.60× |

Two things to note:

1. Under **plain Lua 5.5** the per-sample throughput of `.it` and `.umx` is
   essentially identical (2.60× vs 2.63×). The interpreter has constant
   per-bytecode cost, so song length scales decode time linearly.
2. Under **LuaJIT** the `.umx` is ~48% slower per sample than the `.it` (1.17×
   vs 0.79×). LuaJIT is what exposes the real effect below.

## The cost is bursty and JIT-cold (not GC, not memory)

Instrumenting the decode (per-100-chunk wall time):

- Cost arrives in **bursts of ~10s every ~400–500 chunks** — i.e. once per
  pattern/order region. Between bursts the work is ~free.
- The inner per-sample **mixing loop is JIT-hot**: it runs millions of times,
  traces cleanly, and costs ≈0 in the between-burst windows. The **per-pattern
  work is JIT-cold** — it runs rarely, never gets hot, and stays interpreted, so
  it dominates the LuaJIT run. Plain Lua 5.5 JITs nothing, which is why it does
  *not* show this asymmetry.
- The bursts **persist with the GC disabled** (`collectgarbage("stop")`), so
  they are **not GC pauses**.
- Real resident memory (OS `ps rss`) growth is **modest** (tens to low hundreds
  of MB), and under normal GC the live heap stays ~7–11 MB. This is a speed
  characteristic, not a memory one.

The underlying cause is how non-strict evaluation is lowered onto Lua:
mata-ll compiles laziness to `setmetatable` thunks and cons cells. The
per-pattern path creates and forces many small objects, and thunk-forcing is
dynamic dispatch (`getmetatable` checks, indirect calls) — exactly the shape
that triggers LuaJIT trace aborts / NYI fallbacks. So that code stays
interpreted while the numeric mixing loop traces fine. It is an
impedance mismatch between Haskell-style laziness and LuaJIT's tracer, not a
property of the (ancient, tiny) tracker format.

### Measurement caveat

`collectgarbage("count")` under LuaJIT with the GC stopped misreports badly — it
claimed ~11 GB where the OS showed ~150 MB resident. Trust OS RSS, not that
counter, when GC is off.

## Practical note for `bisect-perf.sh`

The script runs plain `lua` on the `.umx` with a 600s "BAD" threshold. With Lua
5.5 that combination now always reports BAD (≈1400s); even under LuaJIT the
`.umx` is ≈624s, just over the line. To use it for bisection, switch the
interpreter to `luajit` and/or raise the threshold, or point it at the `.it`.
