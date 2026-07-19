# A fun detail: entropy of `zpr.mll` vs `zpr.lua`

First-order (per-byte) Shannon entropy:

| file       | H1 (bits/byte) |
|------------|----------------|
| `zpr.mll`  | 4.430551       |
| `zpr.lua`  | 4.491251       |

Counterintuitively, the **generated** Lua scores *higher* per-byte entropy than
the terse hand-written source — even though the `.lua` is full of repeated
boilerplate (`__force(`, `__thunk(`, `function() … end`, mangled identifiers).

The catch is that first-order entropy only measures the flatness of the byte
*histogram*; it is blind to repetition. The `.lua` spreads its bytes across more
punctuation, digits, and underscore-y generated identifiers, which flattens the
histogram and *raises* H1. The `.mll` leans on the few highest-frequency bytes of
English-ish source (spaces, lowercase letters, comment prose), which skews the
histogram and *lowers* H1. The 0.06-bit gap is noise; both sit in the usual
~4.4 bits/byte "code-shaped text" band.

Let a model that can see repetition weigh in and the verdict inverts hard.
`gzip -9`:

| file       | raw     | gzip'd | ratio | compressed bits/byte |
|------------|---------|--------|-------|----------------------|
| `zpr.mll`  | 2174    | 941    | 0.433 | 3.463                |
| `zpr.lua`  | 132811  | 22451  | 0.169 | 1.352                |

So the `.lua` carries **~2.6× less** information per byte. The original
intuition ("generated boilerplate is repetitive → low entropy") was right; H1 was
just measuring the wrong order.

## Two things that make the comparison unfair anyway

1. **`zpr.mll` doesn't contain its dependencies; `zpr.lua` does.** The `.mll` is a
   2 KB *leaf* that only names its imports. The `.lua` is the whole linked,
   dead-code-eliminated program: `ZPool`, `Lz4`, `Nvlist`, `ZBytes`, `LIOLinear`,
   the reachable slices of `LIO`/`LOS`/`Prelude`, plus the runtime. So "2 KB →
   132 KB" is mostly *inclusion*, not *expansion*. Against the app-local
   source closure it is ~41 KB → 132 KB ≈ **3.2×** — an unremarkable lowering
   ratio for a lazy-functional subset compiled to strict Lua (and lower still if
   you fold in the stdlib+runtime source).

   ```
   zpr.mll     2174
   ZBytes.mll  2827
   Nvlist.mll  4268
   Lz4.mll     4371
   ZPool.mll  27272
   total      40912
   ```

2. **A big slice of `zpr.lua` is a fixed runtime tax.** The runtime + stdlib
   scaffolding is byte-identical across every mata-ll output — a hello-world
   carries most of the same `__force`/`__thunk`/prelude machinery. That is why
   the `.lua` gzips so hard: shared scaffolding is pure redundancy to a
   compressor, and its Kolmogorov cost is charged once to the compiler, not to the
   program.

## The tidy version

The measurement compared a 2 KB *reference* to a self-contained *program +
runtime*. They land within 0.06 bits/byte on H1 only because both are code-shaped
text — not because anything deep is preserved. Everything interesting (the
repetition, the generated-ness, the fixed runtime cost) lives in the orders that
first-order entropy cannot see. The ranking of "how much information is
here": first-order entropy says one thing, `gzip` says a truer thing, and
"it's deterministic compiler output" says the truest thing.
