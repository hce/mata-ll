MATA-LL TODO
============

## Planned — top priority

- [ ] **Direct-perform bare tails, stages 2–3: interprocedural
      classification, then performloop retirement.** Stage 1 (2026-07-28,
      see Completed) fixed the direct-perform tail emission at the
      generation site: SELF tails are bare Lua tail calls and case
      terminals flatten, so the frame-per-step depth limit and the
      runner's `pure`-payload forcing are gone for self-recursion — with
      no loop pass needed. What remains is interprocedural: a tail call
      to a DIFFERENT direct-perform function (`f`↔`g` mutual recursion,
      sed's fn142↔fn143) still rides `__mll_run_tail`'s argument
      position, one pinned frame per crossing. Stage 2: a module-level
      classification pre-pass that records which emitted functions are
      direct-perform, so any saturated tail call to a KNOWN
      direct-perform module-local callee — not just self — emits bare;
      that closes mutual recursion. Stage 3: retire performloop after a
      corpus sweep confirms zero conversions on stage-2 output (stage 1
      already measures zero conversions corpus-wide; the pass stays as a
      backstop until the sweep is repeated over stage 2, and its
      hand-built-tree unit tests keep pinning the old shapes until the
      pass itself is deleted). Also worth taking along: tailloop declines
      a body whose pattern chain keeps the trailing
      `error("Non-exhaustive patterns")` (coverage unproven — huffman's
      3-arm list match, basic's fn200), leaving bare TCO instead of a
      loop; teaching the chain builder or tailloop that shape recovers
      the loop form for those functions.

- [ ] **Lua-AST optimization layer, structured-pass tier (remaining).**
      The foundation AND all three loop passes are COMPLETE (see
      Completed: annotation layer + engine + `__force`-collapse
      peephole; self-tail-call → loop; IO self-loop conversion;
      direct-perform IO self-loop conversion). Remaining, on the
      same annotation/justification API: (1) loop-invariant /
      capture-free closure hoisting (syntactic backstop for the FNEW
      JIT-killer class). Later candidate: liveness-based local-slot
      reuse. Per-pass verification discipline stays: corpus byte-diff
      (empty or reviewed), tracker decode as the semantic + perf canary,
      `codegen_is_deterministic` green, stamp refutation over the final
      tree. The engine's known limit, accepted at design time: the
      no-stale-annotations guarantee holds over the closed rewrite
      vocabulary — a structured pass that needs a new rewrite form
      carries an engine-side proof obligation (relocated bug surface,
      not eliminated); the tail-call pass paid it (WhileTrue /
      MultiAssign / ReturnNone / Goto / Label entered the vocabulary with
      engine-owned validation). Bytecode generation was considered and
      dropped 2026-07-27 (format unstable by design, hardened hosts load
      text-only; `mlua`-based precompile covers the load-time case if
      ever wanted).

- [ ] **`::` ascription inside a right-section operand is accepted; GHC
      parse-errors.** Found 2026-07-27 during the section-precedence work:
      mata-ll accepts `(+ 1 :: Int)` where GHC rejects the `::` in that
      grammar position. Accept-only (no wrong grouping), separate grammar
      production from the operand-precedence rule. Low priority; closing it
      is an acceptance change, so it wants the same corpus-checked shape as
      the precedence fix.

- [ ] **Type-erased generic `show` cannot split Integer/Double on LuaJIT.**
      LuaJIT has no `math.type`, so the last-resort runtime-dispatch `show`
      path shows the double `1.0` as `1` there. Every type-directed path
      (`show_Number`, `show_Integer`, containers with known element types —
      i.e. everything realistic code reaches) is exact on every interpreter.
      Options if it ever matters: carry a type tag into the generic path, or
      accept and document as an interpreter limitation alongside the
      existing 64-bit LuaJIT skips.

## Completed

- [x] **Generics substrate, stress-tested by JSON, shipped with native
      derives** (2026-08-04; commits `ccd466d` + `c61a643`).
      `deriving (Generic)` + `Data.Generics` end to end: compiler-
      populated `Rep` closed family (equation-less declaration, one
      equation per derive), `from`/`to` synthesis, marker-type metadata
      instances (effective names), type operators (`:+:`/`:*:`), stuck-
      family deferral + same-family metavariable unification, proxy
      navigation for generic producers (`gProxy`, `p*` re-typers). The
      JSON module gained `genericToJSON`/`genericFromJSON` — BYTE-EXACT
      against the derived codecs, wire format and every pinned error
      message — as the substrate's stress test; it exposed and forced
      the real fixes: instance methods past the 16-specialisation guard
      now purge to composed dictionary passing (was: raw original → nil
      call), the dictionary phase runs to a fixpoint with bodies
      re-monomorphized from pristine copies under `in_dictform`
      (the arbitrary-specialization fallback had welded one type's baked
      metadata into shared bodies), sibling constrained calls pass
      dictionaries, DCE parses dictionary strings format-structurally
      (`:` in type-operator instance names shredded the old whole-string
      split), and DictCall value arguments take the lazy protocol (eager
      emission forced unused bottoms — a GHC-parity strictness bug).
      DECISION: the generic codecs were measured at +16% (Lua 5.5) /
      ~2.1x (LuaJIT) / +39% emitted code against the native derives, so
      `deriving (ToJSON/FromJSON)` keeps the specialised native
      generators (restored verbatim, plus `fromJSONField_T` emission the
      generic decoder reads); genericToJSON/genericFromJSON stay as the
      library-programmable pair, byte-agreement pinned by generic_json
      (under the guard), generic_json_many (17 types past the guard,
      user GIx included), generic_json_decode (decoder agreement +
      absolute error strings + full generic round-trips), and
      derive_generic (round-trips, conIndex, metadata reflection).
      975 integration tests green; tracker perf canary unchanged
      (5.0x/1.8x realtime); proprietary acceptance green. A future
      rep-collapse/fusion pass could re-attempt generic-backed derives
      at native speed; do not re-switch without that evidence.

- [x] **Direct-perform IO tails, stage 1: case terminals flatten and
      saturated self tails emit bare Lua tail calls** (2026-07-28;
      codegen/action.rs, pattern.rs, function.rs, module.rs). The
      single-clause direct-perform emission had one decision causing two
      GHC deviations: every non-`pure` terminal funneled into
      `return __mll_run_tail(e)`, so (a) a self call sat in the runner's
      ARGUMENT position — one pinned frame per recursion level, ~1e6
      depth limit — and (b) each unwinding frame re-applied the runner,
      whose `__force` evaluated a thunk `pure` payload GHC never forces.
      Now: `case` terminals flatten at statement level exactly like `if`
      (via the pattern emitter's new `tails` mode, guards included), so
      each branch's `pure e` goes through the box convention; and a
      saturated tail call to SELF — known for free while emitting the
      direct-perform arm — returns bare (`return self(...)`), the exact
      form Lua's tail-call elimination reclaims, sound by the
      one-root-application invariant now documented at the runtime's
      runner contract. Two-level builders (multi-clause IO, ST) keep
      their runner: for them it performs. En route the probes exposed a
      third gate with the same symptom: the module-level concrete-vars
      seed listed `undefined` — a runtime THUNK — among "plain local
      functions", so `pure undefined` escaped BARE and the consumer
      forced it; case-flattening alone did NOT close the non-recursive
      probe until that seed entry was removed (the deviation existed in
      the `if` spelling too, and both are fixed and pinned:
      case_pure_bottom, if_pure_bottom). The post-flattening audit found
      one remaining conduit for the first-class pure-suspension closure
      (expr.rs's return/pure arm) into a forwarding-runner argument:
      inlining (`g n = id (pure undefined)` — live, runghc-confirmed);
      that closure's payload now takes the same pure_action_ast escape
      decision, so an unsafe payload crosses boxed (pinned:
      first_class_pure_bottom). Depth is pinned optimized and
      raw (perform_bare_tco_deep at 2e6 with the loop passes disabled via
      the new `CompileOptions::disable_opt_passes`, PUC 5.5 + LuaJIT);
      performloop now converts NOTHING corpus-wide (tailloop claims the
      flattened self-tails) and stays as a backstop with its unit tests
      untouched until stage 3. Corpus swept: 389/408 files re-emitted
      (346 comment-only), every hunk reviewed, A/B-executed
      byte-identical except path/line/timing/randomness; goldens
      re-pinned byte-identical; DIVERGENCES.md still empty.

- [x] **Eta-expanded callees no longer carry a grouping `__force` that is
      really a paren** (codegen/function.rs eta-expansion callee). Found
      by the peephole's engine declining the collapse: the site emitted
      `__force(<function literal>)(_eta…)` where the `__force` doubled as
      the prefixexp grouping Lua requires — the literal is WHNF by
      construction, but collapsing would emit the ungrammatical
      `function…end(args)`, so the engine (correctly) declined and the
      runtime call stayed (nat_hkt.lua:847). Generation-side fix: the
      site now matches the EMITTED Lua expression — a bare `Func` gets
      `Paren` grouping, a `Paren(Func)` (the closure a partial
      application builds; the call-argument paren cleanup had been
      dropping its own parens inside the `__force`, which is why the
      force was carrying the grouping) is used as-is, everything else
      keeps the force (the callee must be a function value, not a
      thunk). The peephole's decline logic is untouched — it was right.
      Bonus the plan didn't predict: with the callee now `(function…)
      (_eta…)`, the existing immediate-call beta-reduction collapses the
      whole application to `local <param> = _eta…; <body>` — the closure
      allocation AND the call disappear, not just the `__force`. The
      TExpr-level twin sites (expr.rs / inline.rs lambda eta-expansion)
      already guarded via `expr_yields_whnf`/`callee_ast`; this was the
      one unconditional site. Corpus 428 files: 24 diffs, every hunk the
      same reviewed shape, zero `__force(function` left corpus-wide;
      nat_hkt runs byte-identical, 17 diffed programs A/B byte-identical
      stdout. Tracker canary: compile byte-identical, decode
      byte-identical (8 336 640 bytes). Suite 1030/0 debug and release;
      GHC oracle 292 pinned / 48 excluded / 0 failed; determinism +
      stamp refutation green; clippy adds zero warnings; proprietary
      acceptance passes. 2026-07-28.

- [x] **Direct-perform IO self-recursion converts to a loop — structured-
      tier pass 7, the ~1e6-depth stack-overflow fix** (codegen/
      performloop.rs, new; `MLL_OPT_DISABLE=performloop`). The shape the
      ioloop pass's repeat-safe gate declined BY DESIGN: an IO/ST
      function that PERFORMS at call time and recurses through
      `return __mll_run_tail(self(…))` — the self call sits in the
      runner's ARGUMENT position, not a Lua tail call, so each step
      pinned one frame (sed's line loop died at ~5e5 lines under PUC
      5.5, verified). Two steps: NORMALIZE dissolves the emitter's
      action-tail tree — dispatch IIFEs splice with their plain returns
      given the pending runner application, action closures splice
      verbatim, both only where control cannot fall out (diverging body
      or function-tail position) — then tailloop mechanics loop the
      direct shape (per-iteration `_w` copies, simultaneous multiple-
      assignment update, always-goto continue). No repeat-safe gate:
      iteration n runs exactly what call n ran, once. The run-tail-
      idempotence argument carries the terminals: the original applied
      `__mll_run_tail` once per unwind level, and on everything in the
      runner's range (a box, or a bare non-thunk non-function value by
      the box convention) a further application is the identity with no
      effects — so the loop keeps each GENUINE application and drops the
      identity re-applications, and a terminal is kept only when the
      runner is provably the identity on it (a `__mll_run_tail(…)`
      result, a literal/fresh-table/`__mll_pure` box, or a pure-
      suspension closure `function() return e end`, whose identity-safe
      payloads unwrap). Anything else declines whole — no new rewrite
      vocabulary entered the engine (WhileTrue/MultiAssign/Goto/Label/
      ReturnNone reused under the existing engine-owned validation).
      Along the way the re-application deviation was found and fixed for
      every converted shape: the unwind's extra runner applications
      FORCED a thunk `pure` payload where GHC binds it unforced —
      predicted, then confirmed against runghc 9.14.1, pinned as
      performloop_pure_bottom (GHC-oracle); the declined-shape remainder
      is the new open item above. Mechanical review permanent, ioloop's
      pattern: a reverse transformer runs as a debug_assert inside
      convert — every debug/test compile un-converts the loop step and
      byte-compares against the normalized body. Corpus 428 files: 6
      diffs, all reviewed and intended — sed's fn102 main loop, basic's
      REPL (fn241) + RUN (fn200) loops, sokoban's game loop, the 3 new
      cases; sed A/B on 1.5e6 stdin lines: unconverted overflows at
      ~499 917, converted completes on PUC 5.5 and LuaJIT with
      byte-identical output on the bounded diff; basic demo/arrays/REPL
      and scripted sokoban byte-identical A/B. Direct repro constant-
      stack at 2e6 and 1e7 on PUC 5.5 + LuaJIT (and 2e6 under mlua via
      performloop_deep). Tracker canary: compile byte-identical, decode
      byte-identical (8 336 640 bytes), 5.1× realtime LuaJIT, full song
      66 s (was ~76). Suite 1030/0 debug and release; GHC oracle 292
      pinned, 0 failed, all pre-existing goldens byte-identical (3 new);
      determinism + stamp refutation green; clippy adds zero warnings;
      proprietary acceptance passes. 2026-07-28.

- [x] **IO self-loop conversion — structured-tier pass 6, the tracker's
      hot shape** (codegen/ioloop.rs, new; committed 5dc0312). The
      two-level IO emission (build-time dispatch returning per-branch
      action closures; closures end in `return __mll_run_tail(self(…))`)
      paid one closure allocation (LuaJIT's FNEW trace-killer), two calls
      and a runner dispatch per self-recursive step. Converted shape: one
      `_lp` closure whose `while true` re-dispatches (params renamed to
      per-iteration copies), splices the branch closure bodies, and turns
      each tail self site into a simultaneous parameter update — followed
      by the ORIGINAL body verbatim with only branch-closure returns
      redirected to `_lp`, so every build-time force, pattern test and
      raise is unchanged: `seq (f undefined) ()` raises at build before
      and after, confirmed against GHC first and pinned as an executed
      oracle case (ioloop_seq_parity). The first iteration's re-dispatch
      repeats only pure, memoized work, enforced (not assumed) by a
      repeat-safe vocabulary gate — names/literals/indexing/operators/
      construction, idempotent cell-memoizers, pure allocators; Raw,
      runner calls or unknown callees decline the conversion. Box
      convention preserved verbatim (pinned by ioloop_box); effect order
      identical; other-function tail forwards stay proper tail calls out
      of the loop. Mechanical review is PERMANENT: a tree-level reverse
      transformer runs as a debug_assert inside convert itself — every
      debug/test compile un-converts and byte-compares every conversion
      (corpus sweep: 0 mismatches / 275 files, 30 functions converted in
      15). Perf: tracker full-song decode 107.0 → 75.9 s under LuaJIT
      (1.41×, byte-identical output), perf canary 4.5× → 5.1× realtime
      (PUC 5.5: 1.7× → 1.8×), IO countdown microbench ~1.5× (LuaJIT,
      independently replicated 1.24 → 0.74 s). Suite 1004/0; GHC oracle
      286/286 (281 baseline reproduced byte-identically under full
      regeneration, 5 new); 2e6-deep IO recursion constant-stack on
      mlua/5.5/LuaJIT both ways; refutation + determinism green;
      proprietary acceptance passes. Finding recorded as a new open item:
      the direct-perform recursion shape overflows today, pre-existing.
      2026-07-28.

- [x] **Self-tail-call → loop conversion — the first structured-tier pass**
      (codegen/tailloop.rs, new; StructuredPass engine extension in
      annot.rs; five real AST nodes added to lua.rs: WhileTrue, MultiAssign,
      ReturnNone, Goto, Label — real nodes, not Raw, so the stamp
      refutation stays meaningful inside converted functions). Shape:
      `while true do local _w0 = p0; …; <body, params renamed to w's>; end`
      with each tail self-call becoming ONE simultaneous multiple
      assignment to the parameters. The per-iteration `w` copies are
      emitted unconditionally: Lua creates fresh locals per loop-body
      execution, so closures and thunks capture their own iteration's
      locals — exactly recursion's fresh-parameter semantics — for one
      register move per parameter per iteration, and no closure-escape
      analysis that would have to be sound against Raw. Simultaneity: the
      multi-assign RHS reads only `w`s, so no read-after-write pair exists
      even in principle. Continue mechanism: fall-through when the body
      diverges (227 of 233 corpus conversions); `goto continue` with the
      label in end-of-block position otherwise (6) — goto is available on
      all targets (Lua 5.4/5.5/LuaJIT per CI matrix and README; 5.1 is not
      a target). Conversion gates: self-identity proven by module-wide
      single-store census for `__mll_fn` slots and binding-scope-subtree
      single-assignment + lexical visibility for named functions (the
      where-group `go` reuse makes literal module-wide uniqueness wrong
      for names — 54 conversions would have been forfeited for nothing);
      single-return proof for the callee; varargs, `_v`-spill headers,
      Raw mentions of self or any parameter, composite lvalues all block.
      Engine guarantees carried over: the pass returns a tree and never
      sees stamps (write monopoly; applied rewrite = invalidate +
      recompute), and the engine validates every rewritten body against
      the extended grammar (MultiAssign holes, bare return block-last,
      goto only to the innermost while's end label) — an invalid rewrite
      is declined whole. Corpus: 233 functions converted across 82 files
      (179 slot-form, 54 assigned-form), every diff verified mechanically
      by a reverse-transformer (un-convert B, byte-compare to A: 0
      mismatches), all outputs pass luac -p and LuaJIT compilation, all
      82 diffed files executed A-vs-B identical. Perf: recursion
      microbench 3.4× faster under LuaJIT (95→28 ms; independently
      replicated 0.08→0.01 s), ~4–5% under PUC 5.5; tracker canary
      unchanged (its hot loops are IO self-loops through __mll_run_tail —
      recorded as the next candidate). Suite 981/0 (965 + 16); GHC oracle
      281/281 unchanged; lua-compat 5.5 and LuaJIT green; 10⁶-deep
      recursion cases green on mlua/5.5/LuaJIT; stamp refutation green
      over converted output; determinism green; proprietary acceptance
      passes. 2026-07-28.

- [x] **Lua-AST optimization layer, foundation: annotation layer, transformer
      engine with an annotation-write monopoly, and the `__force`-collapse
      peephole as its first consumer** (codegen/annot.rs, new, ~1300 lines;
      design agreed 2026-07-27, recorded here before implementation).
      *Stamps*: shape lattice (WHNF / Cons / Closure / Thunk / Unknown,
      monotone — recomputation and rewriting only weaken) plus effect bits
      (pure, may-trap, may-allocate). *Write monopoly, compiler-enforced*:
      `Stamp`/`StampNode` fields are module-private with no public
      constructors; passes implement `ExprPass::request` and see stamps only
      through the read-only `StampView`, rewriting only by returning a
      `Request` with a declared justification — `ReplaceWithChild(i)`
      (inherit; sound by construction, the engine extracts the child
      itself) or `Replace(e, MeetOfChildren|Unknown)`, after which the
      engine invalidates and recomputes the whole mirror (no incremental
      stamp preservation — a second preservation logic would be a second
      trusted base). *Storage*: a mirror stamp tree in lockstep with the
      Lua tree, identity positional over one canonical traversal — no keys
      to dangle across rewrites. *One addition to the agreed design, found
      by the first corpus run*: the engine owns grammatical validity — it
      tracks every node's syntactic hole class (Stmt/Prefix/Grouped/Delim/
      DelimLast, reusing the paren pass's taxonomy) and declines any
      request whose replacement cannot stand in the hole, so a sloppy pass
      cannot emit invalid Lua (the eta-expansion shape
      `__force(<function literal>)(args)` uses the force AS its prefixexp
      grouping; collapsing it would be ungrammatical). *Peephole*: the
      entire pass is eleven lines — `__force(e)` → `e` where `e` is
      WHNF-stamped, justification inherit. It subsumed the old
      force-of-WHNF-locals pass (superset confirmed on the corpus: pass 4's
      11 lines across 4 files all reproduced; pass deleted) and found 20
      further provably redundant forces across 9 corpus files in four
      classes, each argued and verified sound (alias-of-forced
      transitivity, inliner-substituted literals under `__force`, fresh
      table constructors, cons cells in discard position) — these are
      generation-time misses the differential surfaced, not annotation
      overclaims. *Verification*: stamp refutation wired through
      `mllc::compile_with_stamp_refutation` → `verify::check_stamps` so
      every corpus test recomputes the analysis fresh and refutes both
      overclaims (carried stamp stronger than fresh) and residual forces
      (collapse owed but not performed); per-pass toggles
      (`MLL_OPT_DISABLE=parens,dead,iife,force`); suite 965/0 (946 + 19
      added: lattice laws, write-monopoly, inherit/decline, refutation
      positive/negative, mirror alignment, toggles, subsumption); tracker
      canary byte-identical; determinism green; proprietary acceptance
      passes; the 9 behavior-diffed files are executed suite cases.
      2026-07-27.

- [x] **Compiling long do-blocks is now linear in their length.** Found by
      the parser fuzzer's deep probes; every do-`let` statement re-walked
      state proportional to everything before it, in five stacked places,
      each now indexed or incremental: (1) `TypeEnv` caches per-binding
      free-variable footprints at insert (`EnvEntry`, typechecker/mod.rs)
      with aggregate multisets and stale-tolerant reverse indexes, so
      `generalize` asks O(1) membership questions and `apply_subst_mut`
      rewrites only affected bindings — cached in the env, not in `Scheme`,
      because the env owns its entries while `Scheme` has dozens of literal
      construction sites and a public-field mutation site; (2) the
      accumulated substitution composes through reverse indexes
      (`AccSubst`, types.rs) touching only images the incoming substitution
      can change, result identical to `compose`; (3) variable-variable
      unification binds the YOUNGER fresh flexible var to the older
      (types.rs `unify_inner`) so a chained representative (a do-block's
      shared `Num` var) stays put instead of re-pointing every accumulated
      image per statement — either direction is an MGU, compile-time only,
      user-written vars keep the old behavior; (4) the nested-`let` spine
      infers iteratively with one threaded env and one `AccSubst`
      (infer.rs `infer_let_group`); (5) the demand analyzer computes all
      let-to-case suffix seeds in one backward pass (`let_spine_maps`,
      demand.rs) via the same extracted `let_group_close` the recursive
      walk uses, with a direct-computation fallback on cache miss.
      Measured (release, chained 2000/3000-let): 3.79 s / 7.33 s before →
      0.06 s / 0.08 s after, verified against a HEAD-built baseline; real
      programs: tracker compile 0.39→0.06 s, zpool 0.64→0.13 s. Corpus
      A/B (272 files): 271 byte-identical; the one deviation is a single
      zpr.lua line where a provably-`Int` `show` monomorphizes to
      `show_Int` (stable representatives let monomorphization see the
      type) — same rendered output, strictly more precise. Suite 946/0;
      proprietary acceptance passes; CHANGELOG Fixed entry. Known mild
      tail at 12000 chained lets (0.60 s, allocation traffic), noted and
      not chased. 2026-07-27.

- [x] **Exported empty-list results now cross to the Lua host as `{}`, not
      `nil`.** mata-ll represents `[]` as `nil` internally, and the export
      edge let the representation leak: a top-level `[a]` function result or
      `[a]` value export crossed as `nil` while the same empty list crossed
      as `{}` at every other edge — FFI call arguments at every nesting
      depth, callback results, and even one level deeper in the export's own
      result (`Just []` already marshalled to `{}`). The type-directed
      descriptor has been able to tell `[]` from `Nothing` since export
      results gained it; the fix deletes the two blanket
      `if __result == nil then return nil end` short-circuits
      (codegen/module.rs, function-export and value-export emission) that
      predated it, letting `__mll_arg_marshal` decide: `{k="list",..}`
      rebuilds `nil` into a fresh `{}` a host can `ipairs` without a nil
      check; `{k="maybe",..}` passes `Nothing` through as `nil`;
      record/hashmap descriptors keep their own nil guards. Contract change,
      updated deliberately: `ffi_export_string_lists` and
      `export_results_marshal_type_directed` now pin the empty table, the
      new `ffi_export_empty_list_is_table_nothing_is_nil` pins the
      `[]`-vs-`Just []`-vs-`Nothing` trio plus the value-export path, and
      the usermanual FFI note teaches the new rule (it taught the asymmetry
      as a rule before). Breaking CHANGELOG entry written. Suite 946/0;
      proprietary acceptance passes. 2026-07-27.

- [x] **Call-site inliner no longer duplicates argument work — sharing
      restored to GHC's rule.** A non-trivial argument is now substituted
      only for a parameter the body emits at most once; multiply-used
      parameters accept only trivial arguments (literal, variable — `__force`
      memoizes, so re-forcing duplicates nothing — bare constructor/operator
      ref, parens/negation thereof); every other site falls back to the
      ordinary call, which evaluates or thunks the argument exactly once.
      Occurrence counting (`count_name_occurrences`, codegen/util.rs) measures
      work duplication, not syntax: `if`/`case` alternatives contribute their
      maximum (one branch runs, GHC's one-branch allowance), an occurrence
      under a lambda counts double (the lambda may run per call), lambda
      parameters shadow. Call-site let-binding was considered and rejected:
      preserving laziness for a possibly-undemanded parameter needs a
      memoized thunk plus closure — exactly what the ordinary call already
      emits. Hot paths verified untouched: the tracker's emitted Lua is
      byte-identical pre/post (the gate never fires there), decode output
      identical across six A/B runs, perf gate 1.7× realtime. Corpus: 217 of
      218 outputs byte-identical; the one change is the new
      `inline_sharing.mll` case itself, whose diff is exactly the intended
      shape (`sq (sumTo 10)` was `sumTo(10) * sumTo(10)`, now one call).
      Tests: `inlining_preserves_argument_sharing` (marker literal appears
      exactly once in emitted Lua; asserted to count 2 under the pre-fix
      compiler) plus runtime cases in `inline_sharing.mll`. Suite 945/0;
      proprietary acceptance passes. Closes the one corner HASKDIFF.md's
      "shares like GHC" carried. 2026-07-27.

- [x] **Section operand precedence now matches GHC exactly — both
      directions.** A section operand that is an infix expression (or prefix
      minus, `infixl 6`) must bind tighter than the section operator; the one
      legal equal-precedence shape is a chain in the section's own direction
      (infixl operand in a left section, infixr in a right). `(== a || b)` —
      previously ACCEPTED with the wrong grouping `(== (a || b))` — now
      rejects with an error stating the rule, both fixities, why the intended
      meaning is not what the expression would group as, and a `note:` giving
      the parenthesized fix. Enforced at parse time in
      `check_section_operand` (parser.rs), which is correct there because a
      pre-parse scan seeds declared `infixl`/`infixr`/`infix` fixities.
      Alongside: `continue_infix` now stops before an operator directly
      followed by `)`, which COMPLETES left sections with compound operands —
      `(2 * 3 +)` previously failed to parse at all (GHC accepts it) — and
      `(-1 <>)`-style forms mata-ll over-rejected are accepted. Every
      accept/reject decision verified against real GHC 9.14.1 (23 probes);
      rejection is now identical to GHC on every shape, no deviation note
      needed. Corpus check: all 415 .mll files, zero acceptance changes.
      Tests: `section_operand_precedence_matches_ghc` (8 rejection shapes
      incl. declared-fixity, 9 accepted) plus oracle-pinned additions to
      `operator_sections.mll` and `operator_fixity.mll` (regen 280 pinned,
      0 failed). One adjacent accept-only deviation found and logged as the
      open `::`-ascription item above. Suite 943/0; proprietary acceptance
      passes. 2026-07-27.

- [x] **The user-facing evaluation-strategy contract is documented.**
      HASKDIFF.md now carries "Evaluation: call-by-need, with proof-gated
      eagerness": memoizing thunks and CAFs (sharing verified by timing),
      which positions suspend vs force (WHNF only), the verified idiom
      list (ignored arguments, infinite structures, knot-tying `fibs`
      at `!! 85`, lazy trapping `div` bindings), the eagerness contract
      (demand-analysis or totality proof; unobservable in results,
      observable only in time/heap/stack), bottom-and-`try` with the
      `seq` idiom, strict-accumulator idioms (`foldl` leaks / `foldl'`
      fixes, the tracker's ``x `seq` return x`` case), the Lua
      fixed-stack failure mode on ~10^6-deep thunk chains (GHC grows its
      stack; Lua crashes sooner — the leak is the bug in both), and the
      linear-types zero-use lazy-`let` cross-reference. Every claim was
      confirmed by a compiled probe before being written. One
      undocumented sharing loss surfaced during verification and is
      logged as the open inliner item above. 2026-07-27.

- [x] **Scientific-notation numeric literals.** `1.0e-2`, `1e5`, `2.5E+3`,
      `6.022e23` now lex as float literals (Haskell 2010 §2.5: `(e|E) [+|-]
      decimal`, lower/upper `e`, optional sign, ≥1 digit). A bare-mantissa
      exponent like `1e5` is `Fractional` — a float, not an `Int` — and types
      through the existing `NumLit` path (defaulting to `Number`), so it was
      lexer-only. Maximal munch requires an exponent digit, so `1e` still lexes
      as `1` then `e` and `1..3` stays a range. Previously `1.0e-2` lexed as the
      application `1.0 e - 2` (`Unbound variable: e`); the asymmetry that `show`
      emitted exponent notation the lexer could not read back is closed —
      `read . show` round-trips. Cases in `num_polymorphic.mll`, pinned against
      real GHC via the differential oracle. Commit `3397e29`.

- [x] **`let` qualifiers in list comprehensions.** `[ y | x <- xs, let y = f x,
      p y ]` now parses — the `let` binds are visible in the body and every
      later qualifier, desugaring to `let binds in <rest>` as GHC specifies.
      Previously any `let` qualifier failed (single-line too): it was read as a
      guard expression, so `parse_expr` hit `let` and demanded `in`
      (`Expected In, found ...`). The let-binding-group loop was extracted from
      the let-expression atom into a shared `parse_let_binds` helper, so a
      comprehension `let` binds identically — simple, function, and
      tuple-pattern binds, multiple layout-separated bindings, mutual recursion;
      single- and multi-line both work. Cases (single-line, multi-line, chained,
      multiple bindings) in `list_comprehensions.mll`, pinned against real GHC
      via the differential oracle. Commit `6205764`.

- [x] **Parenthesized `( )` expressions are layout-insensitive too.** The
      interior of `( )` was already layout-free (the leading skip after the open
      paren), but `continue_infix` stops at a newline, so the close-side
      decisions did not: a newline before the closing `)`, a tuple comma, or a
      `::` ascription aborted with `Expected RightParen`. Skip newlines/indents
      before each (`parser.rs`), so `( a` / `+ b` / `)`, multi-line tuples, and
      `( e` / `:: T )` all parse — the same principle as the bracket fix.
      Accept-only. Multi-line cases added to `tuples.mll` and pinned against real
      GHC via the differential oracle (only `tuples.stdout` changed; other
      goldens byte-identical under runghc 9.14.1). Commit `13c0932`.

- [x] **List brackets are layout-insensitive: multi-line comprehensions,
      literals, and ranges parse.** Inside `[ ]` the parser only skipped
      newlines/indents before a list literal's commas, so a comprehension bar,
      a range `..`, a qualifier's comma, or the closing `]` on a continuation
      line aborted with `Expected RightBracket, found Pipe` — forcing every
      comprehension onto one line. The head-expr parse returned with a `Newline`
      current, `self.at(&Token::Pipe)` was false, and the literal path then hit
      `|` where it wanted `]`. Fixed by skipping newlines/indents uniformly at
      every bracket-interior decision point (`parser.rs`), so the GHC-idiomatic
      multi-line form works. Accept-only: nothing that parsed before parses
      differently. Multi-line cases added to `list_comprehensions.mll` and
      pinned against real GHC via the differential oracle (golden regenerated
      with runghc 9.14.1). Parenthesized `( )` expressions still need the
      closing paren's line to carry preceding content — a separate construct.
      Commit `616d808`.

- [x] **GHC golden regeneration runs clean again (0 failures).** Three cases
      broke `regenerate-ghc-goldens.sh` so goldens could not be re-pinned:
      `any_ffi_marshal` and `string_escapes` (added without exclusion entries;
      both import Lua-FFI surfaces GHC cannot express) are now excluded like the
      other FFI cases, and `num_user_instance` — whose golden predated the
      `Integer`->`Int` rename that made its `fromInteger :: Integer -> a` twin a
      compile-time type error against `Z5`'s `Int` field — is excluded as a
      deliberate divergence (same category as the `linear_*` cases),
      deregistered from `GHC_ORACLE_CASES`, and its stale golden removed. Still
      exercised by `mll_test!(num_user_instance)`. Regen: 280 pinned, 48
      excluded, 0 failed; all other goldens byte-identical. Commit `150a107`.

- [x] **FFI boundary now converts `Any` to/from plain Lua scalars, and
      undefined-behavior types are rejected at the boundary.** `Any`'s purpose
      is dynamic Lua interop, but nothing marshalled it: the descriptor path
      passed the ADT layout through untouched. `Any` now decodes at the
      boundary — Lua nil/string/number/boolean map to `AnyNull`/`AnyString`/
      `AnyInt`|`AnyNumber`/`AnyBool` (the number split is the standard
      `math.type=="integer"` test with a `% 1 == 0` fallback for LuaJIT), and
      marshals back by forcing the carried payload — via a new `k="any"` arm in
      both `__mll_ffi_decode` and `__mll_arg_marshal` (`codegen/runtime.lua`)
      emitted from `ffi_decode_desc_inner`/`ffi_arg_marshal_desc`
      (`codegen/ffi.rs`). Alongside, `ffi_marshallable` (`typechecker/mod.rs`)
      was tightened from a permissive default to a designed-behavior allowlist:
      scalars, Unit, LuaUserData, List, Tuple, HashMap, Maybe, Any, LuaDict
      records and transparent newtypes pass; plain ADTs (including Either
      outside `LuaTry`, Ordering, ExitValue) are now a loud error at the import
      site rather than silently emitting a table the host cannot read. Import
      signatures are validated by `validate_ffi_import_types` /
      `validate_ffi_import_callback`. Commit `495fe35`.

- [x] **Type errors now locate the offending statement, not the clause head.**
      Every type error pointed at the first line of the enclosing function
      definition regardless of which statement failed. A transparent
      `Expr::Spanned(Span, Box<Expr>)` wrapper (`ast.rs`), applied by
      `parse_stmt_expr` at do-statements, let/where bodies, case branches, guard
      bodies and if branches (`parser.rs`), carries the source position through
      desugar and module rewriting into typecheck; `infer.rs` latches the span
      of the innermost failing marker into `Checker::error_span` and reports
      there. The wrapper is erased at TIR lowering, so codegen is untouched.
      Remaining fallback-to-head cases (documented in the changelog): errors
      that surface only when reconciling a binding against its uses, and
      deferred "No instance" class errors. Commit `3d91016`.

- [x] **Constrained FFI imports now compile.** A signature like
      `dbQuery :: LuaDict b => CryptLiteDb -> ... -> LuaIO ":query_array" a`
      was rejected with a spurious "accompanying definition is lacking" —
      `extract_ffi_info` (`typechecker/mod.rs`) did not see through
      `Type::Constrained`/`Type::Forall` wrappers to find the `LuaIO` marker, so
      the import looked like an ordinary undefined binding. It now recurses
      through both wrappers. Commit `161bce9`.

- [x] **Lexer string escapes are now full GHC `read`-side parity (was the
      last input-syntax gap).** The lexer accepted only `\n \t \r \\ \" \0`
      and — worse than the missing escapes — `\0` was not maximal-munch, so
      GHC source `"\05"` silently decoded to `['\0','5']` instead of `['\5']`.
      The decoder (`lex_string_escape` in `mllc/src/lexer.rs`) now matches the
      Haskell 2010 Report §2.6: the shorthand control escapes `\a`(7) `\b`(8)
      `\f`(12) `\v`(11) alongside the existing ones; decimal, octal (`\o37`)
      and hex (`\xff`) numeric escapes with MAXIMAL MUNCH (so `"\05"` is one
      byte 5); the full named-control table `\NUL`..`\US` plus `\SP`(32) and
      `\DEL`(127), longest-match so `\SOH` wins over `\SO`+`H`; the `\&`
      zero-width separator (`"\137\&0"` is two bytes, `"\SO\&H"` disambiguates
      the name) and the `\<whitespace>\` string gap (newlines allowed). The
      Rust-side `CTRL_ESCAPE_NAMES` table is the input mirror of
      `__mll_ctrl_names` in `codegen/runtime.lua` — kept identical byte-for-byte
      so `read . show == id` through both halves. The deliberate deviation
      forced by mata-ll's byte-string model (String is the Lua string, a byte
      array — HASKDIFF.md "Strings and ByteStrings", so a character is one byte
      0..=255): a numeric escape above 255 (which GHC accepts up to `\1114111`
      as a Unicode code point) has no single-byte representation and is a LOUD
      lexer error carrying a `note:` explaining why, never a silent wrong
      value. Implementation surface: string literals now lex to a byte
      sequence, so `Token::StrLit`, `Literal::Str` and `TLiteral::Str` carry
      `Vec<u8>`; `lua_quoted_string` emits each byte exactly (printable ASCII
      verbatim, everything else as `\ddd`), keeping generated `.lua`
      byte-identical for all existing programs (no example or case has a
      non-ASCII byte in a string literal — the corpus's high bytes are all in
      comments). Symbol-position string literals (FFI Lua names, type-level
      Symbols, JSON/field renames, constructor renames) decode back through
      `Parser::strlit_as_symbol`, which rejects non-UTF-8 bytes rather than
      pass a lossy name. Tests: `string_escapes.mll` (every new escape,
      maximal munch, `\&`, string gaps, and `read . show == id` for the byte
      escapes, all asserted against the Report's byte values since GHC cannot
      run locally) and `string_escape_above_byte_range_is_rejected` (the
      out-of-range `\256` rejection with its note) in `run_mll.rs`.

- [x] **`String`-vs-list type errors now explain the opaque-String design.**
      String stays opaque — decided 2026-07-22: the `[Char]` fiction would buy
      neither speed nor expressiveness, and `[Char]` itself remains usable as
      an ordinary list type. `++` on a String failing is a completeness gap,
      not a soundness violation (HASKDIFF.md, "Why GHC parity"), so the work
      was purely to make the rejection maximally informative per the
      error-message convention. The `Mismatch`/`RigidMismatch` hint for a
      `[a]`-vs-`String` unification (both directions, via the existing
      `is_string_list_mismatch` gate in `mllc/src/types.rs`) now states that
      String is opaque — the Lua string (a byte array), NOT `[Char]` — that
      list operations (`++`, `map`, `length`, `intercalate`) do not apply, that
      `<>` concatenates Strings and a list of Strings folds with `<>` /
      `mconcat`, and points at HASKDIFF.md ("Strings and ByteStrings").
      Error-path only: nothing that typechecked before changes, and the note
      fires specifically for the `[a]`-vs-`String` shape, not for unrelated
      unify failures. The pre-existing `type_errors_are_explained_not_cryptic`
      assertion that the note must NOT prescribe string ops was superseded by
      this deliberate change and updated in lockstep. Test:
      `string_vs_list_mismatch_note_explains_the_design` in `run_mll.rs`
      (asserts the opaque/`[Char]`, `<>`, and HASKDIFF.md content on
      `"a" ++ "b"`).

- [x] **Renamed `Integer` → `Int` — the type is 64-bit and wrapping, and
      its name now says so.** Decided 2026-07-22, done in the same batch;
      rationale in HASKDIFF.md ("Why GHC parity" and "Integers are
      fixed-width"). mata-ll's integer wraps at 64 bits (a double on
      LuaJIT), which is exactly GHC's `Int` — so carrying the
      arbitrary-precision name `Integer` was a silent soundness deviation
      against the GHC oracle. `Int` is now the sole canonical builtin
      integer: the `Int`→`Integer` alias and its warning are gone (no back
      door), and every internal dispatch helper moved in lockstep
      (`show_Int`, `eq_Int`, the Enum/Num/Integral `*_Int` runtime helpers,
      the JSON `toJSONInt`/`fromJSONInt`, the `AnyInt` constructor). Three
      new rejections, each loud with a `note:`: (1) `Integer` in a type is
      an unknown-type error pointing at `Int`; (2) `toInteger` (whose
      result would be the absent `Integer`) is removed from `Integral` and
      reports the same note — `fromInteger` stays, its argument being just
      the `Int` literal type; (3) an integer literal past `maxBound :: Int`
      is a hard lexer error (GHC only warns via `-Woverflowed-literals`;
      stricter-than-GHC rejection is criterion-permitted). Defaulting is
      now `default (Int, Number)`, and `regenerate-ghc-goldens.sh` injects
      `default (Int, Double)` after the last import of each twin (the
      `MllShim` list/STArray primitives retype to `Int`), so GHC stays the
      referee for defaulted arithmetic including overflow. The rename does
      not change runtime output, so the committed goldens stay byte-valid;
      full workspace suite green. Golden *regeneration* is deferred to a
      GHC host (none locally), with no regression in the meantime.

- [x] **GHC as a differential oracle (was planned #2).** The parity suite
      asserted what the author believed GHC does; belief-driven tests share
      the author's blind spots. Now real GHC is the referee:
      `mll-tests/regenerate-ghc-goldens.sh` runs a mechanical GHC twin of
      every eligible case (shared shim
      `mll-tests/tests/ghc-golden/MllShim.hs`) and pins GHC's stdout as
      committed goldens; the `ghc_oracle_*` tests in
      `mll-tests/tests/run_mll.rs` diff mata-ll's runtime output against
      them byte-exactly (254 cases; CI needs no GHC). Known differences are
      pinned and enumerated in `mll-tests/tests/ghc-golden/DIVERGENCES.md`;
      a fix or a drift on any of them fails the suite. (The three `show`
      divergences this system originally caught — unquoted strings, `", "`
      separators, `%.14g` for `Number` — are resolved: `show` matches GHC.) Suggested by the independent 2026-07-21
      review.

- [x] **Lua AST in codegen (was planned #1).** String-based emission replaced
      with a small Lua AST and printer (`mllc/src/codegen/lua.rs`): every
      generator builds `lua::Expr`/`lua::Stmt` trees, printed once. Malformed
      statements are unrepresentable by construction; grouping is explicit
      (`Paren` nodes reproduce the historical parens — no precedence logic;
      redundant-paren cleanup remains a separate, deliberate output change).
      Conversion contract held: byte-identical output across the whole corpus
      and the acceptance program, verified same-HEAD. Suggested by the
      independent 2026-07-21 review.

- [x] **`LIO.readLine` hardened at end of input (same as `getLine`).**
      `readLine` is now `IO String` in `LIO.mll`, a wrapper over
      `ffi_readLine :: LuaTry "io.read" (Either String String)`:
      `io.read()`'s bare-nil EOF
      decodes to `Left` and is re-raised as the clean, catchable error
      `LIO.readLine: end of input` — instead of letting the nil escape into a
      `String` and crash later with "attempt to concatenate a nil value". It
      stays in `LIO` (still needs `import LIO`; not a Prelude alias — the
      error names readLine), and a normal line is still returned without the
      trailing newline. Covered by `mll-tests/tests/cases/readline.mll` (real
      EOF via an `io.input`-redirected fixture file, `try` and `catch` both
      capture the error). NOTE: `readStdin :: String -> LuaIO "io.read"
      String` (unused anywhere) still passes `io.read(fmt)`'s nil through; it
      was left alone deliberately because nil there is format-dependent ("n"
      returns nil for an unparseable number, not only EOF; "a" never returns
      nil), so a blanket end-of-input error would mislabel "n" failures — its
      hardening (likely a `Maybe String` result like `fReadLine`) is a
      separate API decision. `fRead` has the same format-dependent nil.

- [x] **`getLine` in the Prelude (GHC parity).** `getLine :: IO String`, no
      import needed, strips the trailing newline (`io.read`'s default "l"
      format). EOF handling is the point: instead of letting `io.read()`'s
      nil escape into a `String` (the `LIO.readLine` failure mode — a later
      "attempt to concatenate a nil value" crash), `getLine` is a Prelude
      wrapper over `ffi_getLine :: LuaTry "io.read" (Either String String)`,
      so the bare-nil EOF becomes `Left` and is re-raised as the catchable error
      `Prelude.getLine: end of input` — the string-error analog of GHC's
      `isEOFError`. Covered by `mll-tests/tests/cases/getline.mll` (real EOF
      via an `io.input`-redirected fixture file). `LIO.readLine` has since
      received the same hardening (see the entry above).

- [x] **FFI export marshallability check (whitelist, strict).** An `export`
      whose signature uses a type that cannot cross the Lua boundary is rejected
      at compile time — a polymorphic type variable, a class-constrained type (a
      dictionary cannot cross), a region-scoped `ST`/`STArray`/`STRef` handle, an
      `IO`/`LuaIO` action in argument position, or a callback (function) anywhere
      but as a DIRECT top-level export argument (nested in a container, in result
      position, or a callback-taking-a-callback — all of which codegen can only
      pass opaque). The allowed VALUE set is derived from what the marshaller
      (`ffi_arg_marshal_desc` / `ffi_decode_desc_inner` / the deep-force
      fallback) actually round-trips — scalars/`()`/`LuaUserData`/`[a]`/tuples/
      `Maybe`/`Either`/`Ordering`/`Any`/user ADTs+newtypes/`LuaDict` records/
      `HashMap` (scalar key) — with `IO`/`LuaIO` allowed only in result position.
      A function is marshallable in exactly one position (`validate_top_level_
      callback`): a direct top-level export argument, whose own arguments cross
      out (exportable) and whose `LuaIO` result is decoded back in (importable),
      and whose arguments are not themselves functions — mirroring exactly the
      one callback shape the code generator's `__mll_wrap_callback_in` branch
      emits a real descriptor for. The error names the binder, the offending
      type, the position (argument N / result / callback argument / callback
      result) and the direction. Runs after typechecking on the resolved export
      types, before codegen. No previously valid export regresses (the whole
      `ffi_export_*` family still compiles and passes).

- [x] **FFI marshallability tightened to DESIGNED shapes + import-side check.**
      The whitelist above accepted "any data type iff every field marshals",
      which let a plain user ADT (and prelude `Either`/`Ordering`/`ExitValue`)
      cross as MATA-LL's internal `{tag, fields…}` table — a shape with no
      host-facing meaning. `ffi_marshallable` now dispatches on DEFINED
      behavior: scalars/`()`/`LuaUserData`, `[a]`/tuples/`HashMap`, `Maybe a`
      (nil ↔ Nothing), `Any` (by name), a `LuaDict` record (name-keyed table),
      and a NEWTYPE (transparent — recurse its single field, keeping
      `FileHandle`/`LIO` alive). A plain `data` ADT is REJECTED even when its
      fields marshal; `Either` is allowed only as a `LuaTry`/`LuaIOCatch` result
      (peeled). Validation is now SYMMETRIC: FFI imports
      (`LuaPure`/`LuaIO`/`LuaTry`/`LuaIOCatch`) get the mirror check
      (`validate_ffi_import_types`) — arguments cross out (Export), the result
      comes in (Import), outgoing callbacks are validated with directions
      swapped, and the threaded-state fold variable stays whitelisted (its
      soundness is `validate_ffi_callbacks`'s job). Diagnostics name the culprit,
      position and direction, with a `note:` explaining the tagged-table leak and
      pointing at `LuaDict`/`Any`/a scalar-or-list encoding.

- [x] **FFI value/constant exports.** `export foo :: Integer` (with `foo = 123`)
      now marshals the FORCED value directly to Lua (`exports.foo = 123`), by the
      same result contract a function's return value uses (records → keyed
      tables, tuples → positional, ADTs/`Maybe`/lists structurally) — no calling
      wrapper. Previously every export was wrapped in `__force(fn)(args)`, so a
      value export emitted `__force(123)(…)` and crashed. The branch is chosen by
      the export's TYPE (arrow → function wrapper; IO/LuaIO → performing wrapper;
      anything else → direct value), so function and action exports stay
      byte-identical.

- [x] **Numeric typeclass hierarchy (`Num`/`Fractional`/`Real`/`Integral`) with
      polymorphic numeric literals — GHC parity.** Arithmetic operators are now
      class methods with GHC's exact signatures: `Num` (`+`/`-`/`*`/`negate`/
      `abs`/`signum`/`fromInteger`), `Num => Fractional` (`/`/`recip`/
      `fromRational`), `(Num, Ord) => Real`, `(Real, Enum) => Integral`
      (`quot`/`rem`/`div`/`mod`/`quotRem`/`divMod`/`toInteger`). Built-in
      instances: `Integer` is `Num`/`Real`/`Integral`, `Number` is `Num`/`Real`/
      `Fractional` (not vice-versa, as GHC). Integer literals are `Num a => a`
      and decimals `Fractional a => a`, resolved by GHC's `default (Integer,
      Number)` when unconstrained (standard-class-only, so a literal under a user
      class stays ambiguous exactly as GHC). User types take hand-written `Num`
      instances; `sum`/`product` generalised to `(Foldable t, Num a) => t a -> a`.
      The classes PLUG INTO the existing operator-inlining/monomorphization: at
      concrete `Integer`/`Number` the operator methods map to themselves (so they
      stay inline `InfixApp`s / the `div`/`mod`/`quot`/`rem` strict cores) and
      `fromInteger`/`fromRational` are erased — generated Lua is byte-identical
      for existing programs (the example corpus and the tracker benchmark diff
      clean; `codegen_is_deterministic` and a new `numeric_classes_erased_at_
      concrete_types` test guard it). A user `Num` type instead materialises its
      instance methods + `fromInteger` around literals. `let` bindings now apply
      the monomorphism restriction so a literal's `Num` constraint stays attached
      to its use. Deviations (both from the absent `Rational` type): `fromRational
      :: Number -> a`, and `Real` has no `toRational`. `Floating`/`RealFrac`
      assessed and deferred (their ops exist as `Number` functions; only the
      class abstraction is missing).

- [x] **Linear types phase 3: scalar-laundering accept-gap closed — strict GHC
      parity on scalars.** The scalar-memoization exemption is GONE: a scalar
      (`Integer`/`Number`/`Bool`/`String`) derived from a `%1`/`%m` value —
      pattern-bound, `<-`-bound, or a `let`/`where` value binding — is now
      tracked exactly-once like every other alias, exactly as GHC does (GHC has
      no Movable-style scalar rule in the type system). This was a deliberate
      semantics decision, not a pure win: the operationally-harmless
      scalar-duplication the design previously allowed (`go + go where
      go = useOnce t` — memoization forces the thunk once) now REJECTS, the
      accepted price of parity. What it buys: the one ACCEPT-direction hole is
      closed — a pending consumption parked in a scalar's thunk can no longer
      be counted as consumed after the scalar flows into unrestricted position
      (the multi-step `let n = useOnce t in constUnit n` launder, the
      derived-alias flow into an unrestricted function, and a tracked scalar
      captured by a lambda all reject now). Implementation: derived binders
      inherit the source bound at every non-`()` type; scalar-typed value
      bindings join the taint set; the used-many-times-scalar acceptance in the
      binding-group scaling is gone; `Bound::AtLeastOnce` (and the vacuous
      `Violation::bound`) are deleted outright — nothing produces them anymore.
      The `()` run-for-effect exemption is untouched, and the unannotated-`let`
      use-count scaling (dead bindings charge zero; more permissive than GHC
      but operationally sound) deliberately remains. Checker-only: codegen is
      byte-identical (erasure tests unchanged and green). Tests:
      linear_rejects_scalar_where_binding_double_use,
      linear_rejects_scalar_laundered_through_let_binding,
      linear_rejects_scalar_alias_flow_into_unrestricted_function,
      linear_rejects_scalar_captured_by_lambda, plus the updated positive
      fixtures (linear_affine_basic.mll, linear_mult_poly.mll).

- [x] **Linear types (`a %1 -> b`), full — exactly-once + multiplicity
      polymorphism.** `%1` is GHC's linear arrow: consumed EXACTLY once
      (using it zero times leaks, more than once double-frees; both reject);
      `%Many` / `->` are unrestricted. Syntax: `%1`, `%Many`, `%'Many`, and
      `%m` (a named multiplicity variable) before `->`. Multiplicity lives on
      `Ty::Arrow` with identity-blind Eq/Hash and its own unification lattice
      (One / Many / flexible Var / rigid `%m`, invariant like GHC — a
      plain-arrow function is rejected at a `%1` type). Multiplicity
      polymorphism: schemes quantify rigid `%m` ids (minus those free in the
      env, closing an alias-laundering hole), instantiate to fresh flexible
      vars per use, and a rigid var is caller-chosen — so `apply :: (a %m -> b)
      -> a %m -> b` threads a linear value through helpers, local `where`/`let`
      functions (per-parameter multiplicity inferred to a fixpoint), and
      IO/ST/Maybe binds. Enforcement is a dedicated usage pass over the final
      typed IR (`mllc/src/typechecker/usage.rs`), NOT a re-threading of
      Algorithm-W: 0/1/ω counting with sequential add, context scaling, a
      branch-join LOWER bound (used in every `case`/`if`/guard alternative or
      it is dropped on some path), and an absence-is-leak check at every scope
      exit. Under laziness a `let`/`where` RHS is scaled by its binder's use
      count, so a never-forced binding counts zero and leaks; consumption
      reachable only through a bypassable path (Maybe continuation,
      short-circuit operand, wildcard, discarded non-`()` result) rejects.
      Scalar aliases are tracked exactly-once like everything else — the
      scalar-memoization relaxation was removed in phase 3 (see the entry
      above): strict GHC parity, no remaining accept-direction gap.
      Dictionaries stay unrestricted. Everything erases: the
      emitted Lua is byte-identical (regression-tested against the tracker
      decode). Boundary (all deviations REJECT, never false-accept): the Lua
      host side of a `%1` FFI signature is trusted; some
      constructs over-reject conservatively (wildcard over a tainted scalar
      scrutinee, non-`()` result discards, record updates on tainted records).
      Tests:
      linear_affine_basic.mll, linear_mult_poly.mll + the linear_rejects_* /
      erasure tests in run_mll.rs.

- [x] Lambda-calculus reducer — examples/lambda.mll; untyped de Bruijn lambda calculus, capture-free substitution + index shifting, normal-order reduction (fuel-bounded), deriving Eq on recursive Term; Church-encoding oracle (identity, boolean not/and, succ/plus/mult). Targeted the laziness/forcing machinery and found NO bug — that area now handles its hardest workload cleanly. Test: example_lambda_reduction.
- [x] Forcing-gap audit: a thunk-valued field reached by projection/destructuring must be forced before structural use. Fixed two cases (record accessor result; nested case-pattern fields) and verified the rest force at the consumer (tuple-get via show, tuple/struct Eq, cons elements, newtype-in-arithmetic, if-conditions, ==). Audit also found that record accessors were not first-class.
- [x] Record field accessors are first-class — emitted as real functions (with varargs forwarding) in addition to the inline `field r` fast-path, so `map field xs` and over-applied function-typed fields (`fnField r x`) work. Test: record_accessor_first_class.
- [x] Record field accessors (person.name)
- [x] newtype codegen (zero-cost wrapping)
- [x] Exhaustiveness checking for pattern matches
- [x] Better error messages (line numbers on type errors)
- [x] where clauses in functions
- [x] Operator sections: (+1), (1+)
- [x] deriving (auto-generate Show, Eq instances)
- [x] Apply final substitution to TIR
- [x] Prelude as .mll
- [x] User-defined type families
- [x] Kind checking (Type, Symbol, Fn)
- [x] Superclass constraints on instance declarations
- [x] Either, Ordering types in prelude
- [x] Show instance enforcement
- [x] Mutual recursion support
- [x] Composition codegen fix
- [x] GADTs (full pipeline: parser, type checker, exhaustiveness, codegen)
- [x] Non-strict evaluation with cheapness analysis
- [x] seq :: a -> b -> b (explicit forcing; preserves tail calls so seq-strict accumulators run in constant stack)
- [x] Guards in where-clause bindings
- [x] Do-notation: break on closing paren
- [x] __mll_run for IO thunk forcing in >>=
- [x] Orphan instance detection
- [x] Process intrinsic declarations properly
- [x] when :: Bool -> IO () -> IO ()
- [x] Concrete variable tracking to skip redundant __force calls
- [x] Tuple types: (a, b, c) with fst, snd
- [x] Type-specialized show for containers (lists of tuples etc.)
- [x] LuaIterator type family (Lua iterators → lazy MLL lists)
- [x] >> operator (IO then)
- [x] Zero-arg LuaPure constant access (math.pi)
- [x] Haskell-style newtype syntax (newtype Rad = Rad Number)
- [x] Method-call FFI (":write" → handle:write())
- [x] LIO library (file handles, stdin/stdout)
- [x] LMath library (math.* bindings)
- [x] CI pipeline with auto-merge dev → main
- [x] String escape sequences in codegen (\n, \t, \\, \" properly escaped)
- [x] Eq for tuples (element-wise comparison with type dispatch)
- [x] LuaTry type family (Lua nil-means-error → Either String a)
- [x] LuaCatch/LuaIOCatch type families (Lua raised error → Left, via pcall)
- [x] Operator fixity declarations (infixl, infixr, infix)
- [x] Infix-LHS definitions (a |+| b = ... and x `f` y = ..., not just prefix)
- [x] STArray with rank-2 scoped mutability (runST, newSTArray, etc.)
- [x] ByteString intrinsic type with binary I/O operations
- [x] Standard library: Regex, JSON, LOS, LString, LBit modules
- [x] Export keyword for Lua interop (export foo :: ...)
- [x] Polymorphic recursion via dictionary-passing fallback
- [x] Type substitution in monomorphized specializations
- [x] undefined (bottom) value — thunk that errors when forced
- [x] WASM build target (mllc-wasm crate, browser playground)
- [x] Type aliases (`type Pair a = (a, a)`) — note: `Int` is no longer an
      alias; it is the canonical builtin integer type (see the Integer→Int
      rename above)
- [x] `module Name (exports) where` header parsing
- [x] `putStr` (io.write FFI)
- [x] Skip main when loaded via require
- [x] Multi-line record syntax in data declarations
- [x] Lua compat CI (5.4, LuaJIT) and performance benchmark
- [x] IO action semantics test suite

## Typeclasses and dispatch

- [x] Eq as a proper typeclass gating == and /=
- [x] Ord as a proper typeclass gating <, >, <=, >=
- [x] Monad typeclass and >>= operator
- [x] Desugar do-notation through >>= instead of hardwiring

## Missing types and values

- [x] HashMap k v (intrinsic dictionary type, backed by Lua tables)
- [x] Any type (Lua interop: String | Integer | Number | Bool | Null | ...)
- [x] getArgs :: IO [String]
- [x] exit :: IO ExitValue (data ExitValue = Normal | Err Integer)
- [x] takeWhile, dropWhile (prelude)
- [x] Common list helpers in the auto-prelude: null, last, init, concat, span,
      zip, unzip, replicate, iterate, and, or, any, all, sum, product. Defined
      once in Prelude; Data.List re-exports them (so `import Data.List (...)`
      still works) and keeps the less common helpers (sortBy, nubBy, groupBy,
      intersperse, intercalate, partition, unfoldr, scanl, scanr, find, foldl',
      append, break').

## Codegen optimizations

- [x] Prelude runtime functions seeded as concrete
- [x] Monadic bind chain flattening (do-blocks → flat locals, no IIFEs)
- [x] If-expressions as statements in bind chain terminals
- [x] Small pure function inlining at call sites
- [x] Typeclass methods inlined as Lua operators
- [x] Whole-program call-site analysis for parameter concreteness
- [x] Eliminating __mll_run: compile-time type info instead of runtime introspection
- [x] Demand analysis for parameter strictness (per-function, branch-aware)
- [x] return/pure optimization: thunk only when argument contains unknown function calls
- [x] CI wasm build job with artifact upload
- [x] Record field accessors inlined as direct table indexing
- [x] Forward-declared functions packed into __mll_fn table (eliminates 200-local limit)
- [x] IO actions as proper closures (IO can't leak into pure code)
- [x] ST primitive inlining in gen_action (zero-overhead in bind chains)
- [x] Zero-arg IO action flattening (main/helpers use gen_bind_chain_io instead of nested IIFEs)
- [x] pure/return unwrapping in gen_action before type guard (fixes unresolved monad type variables in bind chains)
- [x] Defensive __mll_run for unresolved action types in gen_action (where-clause IO functions)
- [x] try/catch codegen: IO action argument deferred into pcall closure

## Open

- [x] **Prefix/partial `div`/`mod` crash at runtime — fixed.** `div 7 2` and
      `map (div 10) xs` type-checked but emitted `__force(div)(...)` against a
      Lua global that does not exist ("attempt to call a nil value"); only the
      backtick form and backtick sections worked. Fixed by reifying `div`/`mod`
      as first-class functions (the same treatment `seq` got): a prefix, partial,
      or first-class `div`/`mod` now resolves to the runtime wrappers
      `__mll_div_fn`/`__mll_mod_fn`, which force both arguments to WHNF and run
      the existing strict cores `__mll_div`/`__mll_mod`. The inline backtick
      `` a `div` b `` stays on the bare strict core with pre-forced operands, so
      the arithmetic hot path (e.g. the tracker mixer) is byte-identical — no
      redundant force. Regression test: `div_mod_prefix_forms.mll` (prefix,
      partial-via-`map`, first-class-via-`foldr`/higher-order with thunked
      operands, floor semantics on negatives, and zero-divisor raising through
      the prefix/first-class path).
- [x] **Existential unpacking does not skolemize — type-soundness hole —
      fixed.** `data ShowBox = forall a. MkShowBox a (a -> String)` with
      `coerce (MkShowBox x _) = x` was ACCEPTED and coerced anything to
      anything (GADT-syntax existentials leaked identically). Unpacking now
      mints a fresh rigid skolem per pattern (`check_pattern`), so unifying
      the hidden variable with any outer or concrete type is rejected with
      a provenance note naming the hiding constructor. Escape checks cover
      the function's own type (`check_clause`), `case` result types, and
      `where`-function types; the record-selector and record-update back
      doors are closed (existential fields have no selector and cannot be
      updated — as in GHC); GADT-syntax existentials (any signature
      variable not reaching the result type, explicit `forall` and
      contexts included) go through the same skolemization. Declared
      contexts (`forall a. Show a => …`) are enforced both ways: packing
      emits the wanted instance at the concrete type, unpacking provides
      exactly the declared classes (plus superclasses) on the skolem.
      SPEC.md's "cannot escape the branch" promise now holds; CAVEATS.md
      documents the remaining record restrictions. Tests:
      existential_constraints.mll and the existential_unpacking_* /
      existential_* error-path tests in run_mll.rs.
- [x] **IO bind/`return` forces the bound value eagerly — fixed.**
      `_ <- return (error "x")`, a bare `return (error "x")` statement in
      a do-block, and `fmap f (return ⊥)` all raised, where GHC leaves the
      value unforced — violating SPEC.md's eagerness contract ("bottom is
      never evaluated eagerly"). Fixed by making `return`/`pure` suspend a
      possibly-⊥ argument: gen_action and the first-class return/pure closure
      now emit the argument through the eagerness weighing (gen_arg,
      strict=false), so a provably-total value stays eager (`return 0` is a
      bare `0`) while a possibly-⊥ one is thunked and stays inert until
      demanded. A `<-`-bound variable is marked concrete only when the action
      yields WHNF (action_result_is_whnf), so a bound `return ⊥` is forced on
      use, not at the bind. One observable consequence, now matching GHC: a
      bottom returned inside `try` is not caught unless forced there (with
      `seq`) — the two IO tests that pinned the old eager behavior
      (div_mod_by_zero_raises, exceptions test 7) were updated to force inside
      the `try` via `seq`, the same idiom div_exact_and_zero.mll already used.
      Regression test: `return_non_strict.mll` (discarded/bound/`$`/terminal/
      fmap/tuple-field/Maybe forms stay lazy; a demanded returned bottom still
      raises). Found by the 0.1.3 soundness audit; documented in CAVEATS.md.
- [x] **Non-deterministic codegen — fixed.** Generated `.lua` was not
      reproducible: identical source compiled twice could differ, because some
      emission order followed `HashMap` iteration order. Three sources, all
      fixed by sorting/stack-ordering rather than relying on `HashMap` order:
      (1) record field accessors — `TModule.record_accessors` Vec is now sorted
      at construction (typechecker); (2) FFI function emission — `ffi_info` is
      now iterated in sorted-key order; (3) specialization resolution — a
      still-polymorphic recursive call inside a specialization picked an
      arbitrary entry via `self.specializations.iter().last()`; now resolves to
      the enclosing specialization via an explicit generation stack (`gen_stack`),
      which is both deterministic and correct under nested specialization.
      Guarded by `codegen_is_deterministic` (compiles a feature-rich fixture 8×,
      asserts byte-identical). Verified across every example and test case; the
      tracker decode stays byte-identical.
- [x] Default method implementations in class declarations (`x /= y = not (x == y)`)
- [x] Where-clause type unification: pre-registered fresh type variables now unified with inferred types
- [x] Higher-rank polymorphism (generalize beyond ST/LuaFunction scope sealing)
- [x] Reject bare type signatures with no definition (was silently compiling to nil at runtime; now a compile error, FFI sigs still allowed body-less)
- [x] **Constructor-level dead-code elimination — done.** DCE now also prunes
      data constructors: a constructor is live iff a kept function constructs
      it (a `Con`/`Var` reference) or matches it in a pattern
      (`collect_clause`/`collect_expr` now walk clause patterns, case-branch
      patterns, and let/where binding patterns via `collect_pattern`), and a
      `data` definition none of whose constructors is live is dropped from
      emission — whole-definition granularity, so tags never shift. The four
      Prelude datatypes (`ExitValue`, `Any`, `Either`, `Ordering` — 12
      `__mll_fn` slots) no longer ship in programs that don't touch them.
      One deliberate refinement over the original plan: dropped definitions
      are NOT discarded — they move to `TModule::dropped_data_defs`, which
      codegen still REGISTERS (constructor tags, LuaDict string tags and
      field keys, FFI-decoder field types) but never emits. The metadata must
      survive because a value of a dropped type can flow through live code
      without being constructed or matched there — canonically a LuaDict
      record built by the Lua host and read only through field accessors,
      whose keyed `.field` layout (vs. positional `[i]`) comes from exactly
      this metadata; filtering `data_defs` outright would have miscompiled
      that case. Tests: `constructor_dce_unused_data_adds_nothing` (a dead
      `data` + derived instances adds nothing — byte-identical output — and a
      minimal program carries no Prelude constructor slots) and
      `constructor_dce_keeps_metadata_for_flow_through_types` (accessor stays
      keyed, FFI descriptor keeps the record shape, constructor not emitted).
      `codegen_is_deterministic` still green; full suite passes.
- [x] Well-defined runtime errors when decoding a LuaUserData/LuaDict value that
      crosses the Lua FFI boundary. The type-directed FFI-result decoder
      (`__mll_ffi_decode`) now raises a descriptive
      "declared T but the host returned X" error for *every* shape mismatch —
      a record missing a declared field, a wrong-typed field or element, a
      scalar where a list/record/tuple was declared, a missing multi-return
      value — naming the position (field/element) and the host function.
      Multi-return tuple results are decoded like every other FFI result.
      Valid values are never rejected: bare scalar results stay check-free
      (hot path), and a mata-ll thunk round-tripping through the host as
      opaque state passes through untouched (laziness preserved).
      Test: ffi_decode_shape_mismatch_errors.
- [x] Layout: a function whose first argument is on the next line (`f` then newline then `(arg)`) is now consumed as an application. The cross-line continuation no longer requires a same-line argument (has_args) — the block-column check alone keeps siblings from being grabbed, now that block_indent is tracked correctly. Test: first_argument_on_next_line. Found writing examples/lambda.mll.
- [x] Layout: multi-line application-argument continuations now use the enclosing layout-block column (Haskell rule) instead of the function column. Introduced a `block_indent` field set at each block (top-level/clause, where, let, let-in-do, do, case, class/instance methods via parse_clause) to the block's item column; parse_expr_app gates cross-line continuation on `current_indent > block_indent`. So `f = foldr g 0` then `  [1,2,3]` now parses. Surfaced and fixed a 1-space misalignment in Data.List.sortBy that the old lenient rule had tolerated. Test: shallow_multiline_continuation. All 254 tests still pass.
- [x] `$` operator emitted literally in Lua when inlined into ST action codegen path (should always desugar to function application)
## Recently completed

- [x] try/catch exception handling (pcall-based, IO errors only)
- [x] fileLines: eager IO with Maybe-returning fReadLine (no lazy IO, no LuaIterator for IO)
- [x] gen_action hardened: pure/return, ST primitives, and unresolved types all before/around type guard
- [x] Audit: 28 zero-arg IO helpers across 3 test files were silently not executing; all fixed
- [x] Local variable table fallback: constructors, newtypes, and instance methods packed into `__mll_fn` table alongside functions; function-body `_v[N]` overflow slots when binding count exceeds 180
- [x] Existential types in data constructors (`data ShowBox = forall a. MkShowBox a (a -> String)` — parser, typechecker, and pattern matching support)
- [x] Deriving Functor (requires traversing constructor fields to find type parameter)
- [x] DataKinds: promoted data constructors as type-level tags ('Empty, 'NonEmpty)
- [x] Type-level naturals: promoted constructors with arguments ('S 'Z, 'S ('S 'Z)) for length-indexed vectors

## Done

- [x] List-of-tuple equality (recursive element eq generation for nested containers)
- [x] `>>=`/`>>` on non-IO monads in let-bindings (spine walker now skips non-IO monads, bind_List added)
- [x] Typeclass-constrained library functions work via source-merging compilation (not a bug with current model)
- [x] Deriving Enum and Bounded for simple enum types (toEnum, fromEnum, succ, pred, range syntax)
- [x] Cross-function demand propagation (if callee is strict in position j, propagate to caller)
- [x] Full strictness analysis (demand-driven call-site decisions, is_cheap_arg retained for trivial expressions)
- [x] Monad typeclass dispatch for >>= and >> (instances for IO, LuaIO, ST; proper error on missing instance)

## Parser

- [x] Multi-line function application (continuation lines indented past function column)
- [x] Multi-binding `let` in `do` blocks
- [x] Guards in combination with `where` clauses

## Haskell compatibility gaps

- [x] Eq for [a] and Maybe a (parameterized typeclass instances)
- [x] deriving Ord
- [x] List comprehensions
- [x] Backtick sections (`(`div` 2)` as a function)
- [x] Local function definitions in do-let (`let f x = ...`)
- [x] Inline case syntax (`case x of { A -> ...; B -> ... }`)
- [x] Module export control (export list parsed and enforced in typechecker)
- [x] where blocks at module level

## Known limitations

- [x] Typechecker stack overflow on CPS-heavy types (fixed: iterative right-spine processing for bind chains)
- [x] Top-level let-in value bindings (thunked values removed from concrete_vars)
- [x] Inliner captures free variables in lambda bodies (gen_expr_subst now handles Lambda)
- [x] `let bottom = error "msg"; const 1 bottom` forces bottom eagerly at call site (fixed: callee-side strictness — call sites pass args without forcing, callee forces at entry based on demand analysis)
- [x] Multi-line case in do-let can cause multi-line continuation to consume next statement as argument (fixed: case loop restores parser position on break so whitespace tokens aren't consumed)
- [x] Pattern-matching generators in list comprehensions (`[x | Ok x <- rs]`)
- [x] **Interprocedural `return ⊥` forced at the bind — fixed.** For an
      APPLIED user function whose terminal action is `pure e` (e.g.
      `mk n = do { _ <- return (); pure (error "boom") }`), the bind
      `v <- mk 1` used to raise even when `v` is never used, where GHC does
      not. Cause: `return e`/`pure e` was represented as `e` itself, so
      `__mll_run` could not distinguish "a thunk that computes which action to
      run" (must force to reach the closure) from "a value-action whose result
      IS a thunk or a function" (must not force/call) — it forced, raising, and
      the same conflation *called* a returned `pure (\x -> …)` with no
      arguments. Zero-arg and intraprocedural forms already escaped it (a
      zero-arg action compiles to a deferred closure `__mll_run` calls; an
      intraprocedural `x <- pure e` binds the value directly). Fixed with a
      tagged pure box: an escaping `pure e`/`return e` emits `__mll_pure(e)`
      (via `gen_pure_action`), and every action runner — `__mll_run`,
      `__mll_perform`, `try_`/`catch_`, the exported-function wrapper, and the
      outgoing-callback wrapper — unwraps the box WITHOUT forcing or calling
      it. Left bare (no box, so no per-action allocation) when provably safe:
      the payload is a tuple literal or `is_cheap_to_force`, AND its type is
      never a Lua function (scalars/unit/list/tuple) — so `__mll_run`'s force
      is a harmless no-op. Backend-transparent: the tracker decode stays
      byte-identical, and its hot mixer/ST path emits no boxes (only the
      per-chunk PCM cons and the fold base). The direct-bind short-circuit
      (`gen_bound_action`) keeps `x <- pure e` unboxed. Regression test:
      `return_bottom_interproc.mll` (applied `pure ⊥` bound-and-unused,
      applied pure-of-function, demanded-still-raises, value-preserving
      threading), plus `return_non_strict.mll` (intraprocedural/zero-arg) still
      green. Found by the 0.1.4 soundness follow-up.

## Testing

- [x] Comprehensive test suites for each library module (Regex, JSON, LOS, LString, LBit, LMath)
- [x] Stress tests for compiler limits (large ADTs, deep recursion, nested exprs, many functions/instances, long do-blocks, large patterns, deep types, many args, list ops, BST program)
- [x] Do-notation regression tests (eval order, let scoping, bind return unwrapping)
- [x] 612 tests passing (Lua 5.4 via mlua)

## Can defer

- [x] Lambda pattern matching

## Example programs (compiler stress tests)

- [x] Scheme interpreter — examples/scheme.mll; recursive Value/Expr/Env ADTs, closures-as-values, environment chaining, eval/apply, recursion via self-application; asserts results against known answers (test: example_scheme_eval). Monomorphizer handled it cleanly with no bugs.
- [x] Red-black tree — examples/redblack.mll; Okasaki balance with doubly-nested constructor patterns, RB-invariant + in-order-sorted oracles (test: example_redblack_invariants). Surfaced and fixed a parser bug: nullary constructors were rejected as pattern arguments (`Box R n`).
- [x] Type inference engine — examples/typeinfer.mll; Algorithm W (unify + occurs check + substitution composition), recursive Ty/Term ADTs, Either error plumbing, deriving Eq on Ty; normalized-type-string oracle (test: example_typeinfer_checks). Surfaced and fixed a codegen bug: `case` matching a nested pattern under a constructor whose payload was a thunk did not force the field before destructuring it (read thunk internals as field values).
- [x] Ray tracer — examples/raytracer.mll; Vec3/Ray/Sphere records, ray-sphere intersection with nested lets, Lambertian shading + shadows, PPM P3 output; tolerance-based geometric oracle + sentinel pixels (test: example_raytracer_renders). Surfaced and fixed a codegen bug: record field projections were not forced, so thunk-valued fields (from non-cheap construction like `s * va v`) reached arithmetic as Lua tables.
- [x] Huffman coding — examples/huffman.mll; recursive HTree ADT, sortBy-based tree build, code-table DFS, LBit bit-packing, ByteString roundtrip; self-checking via assert (test: example_huffman_roundtrip)

## String types (design decision)

String = Lua string permanently. ByteString = Lua string with explicit byte semantics (same runtime representation, type-level distinction only). Text = future UTF-8 type over ByteString, if/when Unicode support is needed.
