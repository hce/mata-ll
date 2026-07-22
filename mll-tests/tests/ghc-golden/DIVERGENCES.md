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

## Pinned runtime divergences

None. The nineteen divergences this file used to carry (four pinned
runtime ones and fifteen assertion-level ones) all reduced to three `show`
behaviors — unquoted `String` show, `", "` list/tuple separators, and
`%.14g` fractional formatting — and were resolved by converging `show` on
GHC:

1. `show` of `String`/`Char`-free string data quotes and escapes exactly as
   GHC's `showLitString` (control-character names, numeric escapes, the
   `\&` ambiguity breaker).
2. List and tuple `show` separate with `","`; record syntax shows as
   `Con {field = value, ...}` with `", "` — both GHC's derived layouts.
3. `Number` (`Double`) `show` is a faithful port of GHC's
   Burger-Dybvig `floatToDigits` plus `showFloat`'s layout rules, verified
   byte-identical to GHC 9.14.1 over a 100k random-bit-pattern corpus.

The formerly divergent cases are ordinary goldened oracle cases now.

## Grammar/type-level differences observed while twinning

Not output divergences, but measured GHC-acceptance differences the twin
generation had to account for (details and per-case reasons in
`regenerate-ghc-goldens.sh`; the shim notes in `MllShim.hs`):

* `newtype Age = Integer` (implicit constructor named after the type) is
  mata-ll-only sugar; GHC requires `newtype Age = Age Integer`.
* Linear types: mata-ll counts an unrestricted use of a `%1`-bound scalar
  (e.g. `useOnce (Token n) = n * 2`) as the single consumption; GHC's
  `-XLinearTypes` rejects it (GHC-18872).
* mata-ll allows top-level names/classes/constructors to shadow Prelude ones
  (`Just`, `Eq`/`Ord`, record fields named `until`); under GHC these are
  ambiguity errors.
