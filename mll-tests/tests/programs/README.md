# The differential program corpus

Whole programs, not feature probes. Each file here is an ordinary small
Haskell program (a parser, a solver, an interpreter, a simulation) written
in the subset mata-ll and GHC share, and every one is run three ways with
the outputs byte-compared:

* under real GHC (`../../regenerate-ghc-goldens.sh` builds the twin and pins
  its stdout as `../ghc-golden/programs/<name>.stdout`);
* in-process under `cargo test` (`ghc_oracle_prog_*` in
  `../run_mll/ghc_oracle.rs`, mlua Lua 5.4);
* under the real interpreters via `../../lua-compat.sh` (Lua 5.4, 5.5, LuaJIT).

`tests/cases` and `tests/ghc` probe one feature at a time and many of them
touch Lua-only surfaces. This corpus is the opposite: programs long enough
for the analyses to interact (strictness and demand, monomorphization,
fusion, inlining, the Lua-tree passes) over data that is computed rather
than written down, and twinnable by construction — the golden generator
refuses to exclude a program, and the registry test refuses an `EXCLUDED.tsv`
row for one. Where a program would need a Lua-only surface it does not
belong here; put a self-asserting case in `tests/cases` instead.

The corpus doubles as the 0.2 compatibility corpus: a program that GHC
accepts and runs and mata-ll rejects or runs differently is a compatibility
defect (a rejection, a divergence, or a crash), and the fix is on the
compiler side, never a rewrite of the program to dodge it. A divergence that
is accepted as a documented deviation is pinned the usual way
(`../ghc-golden/divergent/programs/`, `../ghc-golden/DIVERGENCES.md`).

## The shared subset

What a program here may use (the rules the twin generation implies; see
`doc/articles/HASKDIFF.md` for the reasons):

* every top-level binding carries a type signature; local bindings are
  single-equation (multi-clause local functions go in `where`);
* `String` is opaque: concatenate with `<>`, never `++`; there is no `Char`
  and no character literal — `import LString` gives `strByte`, `strLen`,
  `strSub`, `strChar`, `strToInts` (the twin is `../ghc-golden/LString.hs`,
  a `[Char]` port with Lua's index rules), and codes stay within ASCII;
* the Prelude list functions, `Data.List`, `Data.Maybe`, `Data.Foldable`,
  `Control.Monad` (no `foldM`/`replicateM` — write them), `Data.IORef`, the
  `ST`/`STArray` builtins, and `Data.Map`/`Data.Set` through a qualified
  import using the containers-compatible names only (`M.insertWith`,
  `M.foldrWithKey`, `S.member`, ...);
* GHC's `default (Integer, Double)`: an unannotated literal is an `Integer`,
  `Int` is reached by annotation (and a `Data.Map` key must be one);
  `Number` is `Double`;
* no `Char`-based Prelude functions (`words`, `lines`, `unwords`), no
  `Floating`/`RealFrac` classes beyond `sqrt` and the `Number -> Int`
  rounding functions.

(Historical constraints, since lifted: `fromIntegral`, pattern guards and
`let` qualifiers in guards, and `<>` at list types are all supported now —
new programs may use them.)

## Adding a program

1. Write `<name>.mll` here with a header comment saying what the program is
   and which language surfaces it leans on. Keep it deterministic, stdin-free,
   and quick (well under a second under PUC Lua).
2. Add `(ghc_oracle_prog_<name>, "programs", "<name>.mll")` to
   `for_each_ghc_oracle_case!` in `../run_mll/ghc_oracle.rs`.
3. Run `../../regenerate-ghc-goldens.sh` (needs GHC): it is the first
   referee — a twin that does not compile or run is a bug in the program.
4. `cargo test -p mll-tests --test run_mll ghc_oracle_prog_<name>` and
   `../../lua-compat.sh lua` / `luajit` must both be green.
