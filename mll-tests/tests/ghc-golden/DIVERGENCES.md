# Known divergences from GHC

Every place where mata-ll's runtime output is known to differ from real
GHC's, as measured by the differential oracle (see `../../regenerate-ghc-goldens.sh`
and the `ghc_oracle_*` tests in `../run_mll.rs`). Nothing in this file is
hidden from the suite: each pinned divergence is re-checked on every test
run, and `ghc_oracle_registry_is_complete` fails if a pinned divergence is
missing from this file.

How an entry works:

* the GHC output stays pinned in `cases/<name>.stdout` / `ghc/<name>.stdout`
  (never edited to match mata-ll);
* mata-ll's current output is pinned in `divergent/<sub>/<name>.stdout`;
* the case's `ghc_oracle_*` test asserts mata-ll still produces exactly the
  pinned output AND that it still differs from GHC — so both a silent drift
  and a silent fix fail the suite.

Fixing or formally accepting each divergence is a separate decision; this
file only records the measured facts.

## Root causes (runtime output)

All four pinned runtime divergences and all fifteen assertion-level ones
reduce to three `show` behaviors:

1. **`show` of `String` omits the quotes.**
   mata-ll: `show "hi"` = `hi` — GHC: `"hi"` (with escaping).
   Also inside constructors: `show (Left "x")` = `Left x` vs `Left "x"`.
2. **List/tuple `show` separates with `", "` instead of `","`.**
   mata-ll: `[1, 2, 3]`, `(42, 2)` — GHC: `[1,2,3]`, `(42,2)`.
3. **`show` of `Number` uses Lua `%.14g` formatting, not GHC's
   shortest-round-trip `Double` show.**
   mata-ll: `show 3.0` = `3`, `show (0.1 + 0.2)` = `0.3` —
   GHC: `3.0` and `0.30000000000000004`.

## Pinned runtime divergences

| case | GHC golden | mata-ll (pinned) |
|---|---|---|
| `cases/existentials` | `"hello"`, `"two"`, `[1,2,3]` | `hello`, `two`, `[1, 2, 3]` |
| `cases/existential_constraints` | `"hi"` (twice), `"two"` | `hi` (twice), `two` |
| `cases/rank2` | `(42,"hello")`, `[1,2,3]`, `(10,"world")` | `(42, hello)`, `[1, 2, 3]`, `(10, world)` |
| `ghc/ghc_cgrun054` | `[X2,X4,X5]` | `[X2, X4, X5]` |

## Assertion-level divergences (no golden possible)

These cases contain an `assert` whose expected string encodes mata-ll's
`show` format. Under GHC the assert itself fails, so the twin aborts and no
golden can be produced; they are excluded in `regenerate-ghc-goldens.sh`
(reason `diverges: ...`) and remain covered by the ordinary `mll_test!`
suite. Expected = the string the .mll asserts (mata-ll's actual output);
GHC = what the same expression produces under GHC.

| case | expression | expected (mata-ll) | GHC |
|---|---|---|---|
| `cases/edge_cases` | `show ""` | `` (empty) | `""` |
| `cases/haskell_compat` | `show [1, 2, 3]` | `[1, 2, 3]` | `[1,2,3]` |
| `ghc/ghc_cgrun030` | `show [1, 2, 3]` | `[1, 2, 3]` | `[1,2,3]` |
| `cases/instance_context` | `show (Leaf "s")` | `Leaf s` | `Leaf "s"` |
| `cases/instance_context_multi` | `show (Pair 1 "hi")` | `Pair 1 hi` | `Pair 1 "hi"` |
| `cases/instance_context_paren` | `show (Wrap (Wrap "x"))` | `Wrap Wrap x` | `Wrap Wrap "x"` |
| `cases/lazy_index_thunk_leak` | `show (iterate inc 0 !! 1, True)` | `(1, True)` | `(1,True)` |
| `cases/mangle_collision` | `show (MkAB, MkC)` | `(MkAB, MkC)` | `(MkAB,MkC)` |
| `cases/pair_ord_fields` | `show (MkPair (MkPair 1 "x") Red)` | `MkPair (MkPair 1 x) Red` | `MkPair (MkPair 1 "x") Red` |
| `cases/poly_recursion` | `showNested n` (user fn over `show` of lists) | `Cons 1 (Cons [2, 3] (Cons [[4, 5], [6]] (Nil)))` | `Cons 1 (Cons [2,3] (Cons [[4,5],[6]] (Nil)))` |
| `cases/show_either` | `show (Left "x" :: Either String Integer)` | `Left x` | `Left "x"` |
| `cases/tuple_field_laziness` | `show (inc 41, inc 1)` | `(42, 2)` | `(42,2)` |
| `cases/typeclasses_full` | `show (Circle 3.0)` | `Circle 3` | `Circle 3.0` |
| `cases/unit_type` | `show [(), ()]` | `[(), ()]` | `[(),()]` |
| `ghc/ghc_regr003` | `show "red"` (via `showDescribed Red`) | `red` | `"red"` |

## Grammar/type-level differences observed while twinning

Not output divergences, but measured GHC-acceptance differences the twin
generation had to account for (details and per-case reasons in
`regenerate-ghc-goldens.sh`; the shim notes in `MllShim.hs`):

* mata-ll treats Haskell's non-associative precedence-4 operators
  (`==`, `/=`, `<`, `<=`, `>`, `>=`, backticked `elem`) as left-associative;
  GHC rejects e.g. `f <$> x == y` with a precedence parsing error.
* mata-ll fixity declarations do not cross module boundaries: `Prelude.mll`'s
  `infixl 4 <$>` is invisible in user code, so `<$>`/`<*>` bind at the
  default (tightest) precedence there, e.g. `a == f <$> xs` parses as
  `a == (f <$> xs)`.
* `newtype Age = Integer` (implicit constructor named after the type) is
  mata-ll-only sugar; GHC requires `newtype Age = Age Integer`.
* Linear types: mata-ll counts an unrestricted use of a `%1`-bound scalar
  (e.g. `useOnce (Token n) = n * 2`) as the single consumption; GHC's
  `-XLinearTypes` rejects it (GHC-18872).
* mata-ll allows top-level names/classes/constructors to shadow Prelude ones
  (`Just`, `Eq`/`Ord`, record fields named `until`); under GHC these are
  ambiguity errors.
