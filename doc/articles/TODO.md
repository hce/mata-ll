MATA-LL TODO
============

## Planned — top priority

- [ ] **Lua-AST optimization layer: annotations + a transformer engine with
      an annotation-write monopoly.** Agreed 2026-07-27. The design, in full:

      *Annotations* on `lua::Expr`/`lua::Stmt` carry OPERATIONAL facts the
      generator already proves and currently discards at emission — NOT
      source types (post-monomorphization `Ty` has no backend consumer and
      would couple the AST to the typechecker). Vocabulary: a small shape
      lattice (WHNF / thunk / closure / constructor-shape / unknown) plus
      effect bits (pure, may-trap, may-allocate). The lattice is MONOTONE:
      facts only weaken toward unknown, never invert.

      *Engine*: passes have NO write access to annotations. They request
      rewrites; the engine applies them and assigns result stamps only by an
      explicit justification the rule declares — inherit-from-named-source,
      meet-of-sources, or unknown (the default). Beyond declared
      justifications: invalidate to unknown and RECOMPUTE via the annotation
      analysis afterward (the LLVM model; at mata-ll's whole-program sizes
      recomputation is free — do not build stamp-preservation logic twice).
      So a buggy pass can destroy information but cannot assert a false
      claim; the trusted base narrows from N passes to one engine plus one
      analysis. Known limit, accepted: the guarantee holds over the closed
      rewrite vocabulary — extending the vocabulary carries an engine-side
      proof obligation (relocated bug surface, not eliminated).

      *Two pass tiers.* Rewrite-rule tier (local, shape-driven): (1)
      redundant-paren cleanup — first, it de-noises every later diff; (2)
      `__force`-collapse peephole — the FIRST annotation consumer (WHNF +
      pure stamps enter with it, not before: unread claims rot), and it is
      differential-testable against the existing generation-time
      `gen_forced` machinery — same corpus, byte-identical expected, any
      diff is a bug in one of the two. Structured tier (hand-written
      whole-function transforms, global side conditions, same annotation
      API): (3) self-tail-call → loop conversion (interpreter dispatch win;
      loops trace better than recursion under LuaJIT) — the only pass with
      a plausible measurable benchmark win, do it after (1); (4)
      loop-invariant / capture-free closure hoisting (syntactic backstop
      for the FNEW JIT-killer class). Liveness-based local-slot reuse as a
      later candidate.

      *Verification*: `verify.rs` gains a stamp-refutation check over the
      final tree in test builds (e.g. no `__force` around a WHNF-stamped
      node); every pass individually toggleable; corpus byte-diff per pass
      (empty or reviewed); tracker decode as the semantic + perf canary;
      `codegen_is_deterministic` stays green. Bytecode generation was
      considered and dropped 2026-07-27 (format unstable by design,
      hardened hosts load text-only; `mlua`-based precompile covers the
      load-time case if ever wanted).

- [ ] **Quadratic typechecking of long do-blocks.** Found by the parser
      fuzzer's deep probes: each do-`let` binding calls `generalize`, which
      recomputes `TypeEnv::free_vars` by walking EVERY scheme in the
      environment (the whole Prelude plus all previous bindings) with
      `Vec::contains` de-duplication — O(statements × env) overall.
      Measured (debug): 500 lets 1.0 s, 1000 → 3.0 s, 2000 → 11.8 s,
      3000 → 29.6 s; `sample` shows the time in `infer_expr` →
      `TyVar::eq`/`slice_contains`. Compilation is correct, just
      superlinear. Candidate fix: cache per-scheme free variables at
      `Scheme` construction (top-level schemes are closed, so the env scan
      collapses), or maintain the env's free-variable set incrementally.
      Needs its own change with the perf benchmarks re-run.

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
