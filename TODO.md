MATA-LL TODO
============

## Completed

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
- [x] seq :: a -> b -> b (explicit forcing)
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
- [x] Operator fixity declarations (infixl, infixr, infix)
- [x] STArray with rank-2 scoped mutability (runST, newSTArray, etc.)
- [x] ByteString intrinsic type with binary I/O operations
- [x] Standard library: Regex, JSON, LOS, LString, LBit modules
- [x] Export keyword for Lua interop (export foo :: ...)
- [x] Polymorphic recursion via dictionary-passing fallback
- [x] Type substitution in monomorphized specializations
- [x] undefined (bottom) value — thunk that errors when forced
- [x] WASM build target (mllc-wasm crate, browser playground)
- [x] Type aliases (`type Pair a = (a, a)`, `Int` as alias for Integer)
- [x] `module Name (exports) where` header parsing
- [x] `putStr` (io.write FFI)
- [x] Skip main when loaded via require
- [x] Multi-line record syntax in data declarations
- [x] Lua compat CI (5.1, 5.4, LuaJIT) and performance benchmark
- [x] Lua 5.1 compat: graceful bitwise library fallback
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

- [x] Default method implementations in class declarations (`x /= y = not (x == y)`)
- [x] Where-clause type unification: pre-registered fresh type variables now unified with inferred types
- [ ] Higher-rank polymorphism (generalize beyond ST/LuaFunction scope sealing)
- [ ] Reject bare type signatures with no definition (currently silently compiles to nil at runtime)
- [ ] Strict ST monad variant (LuaStrictArray or similar) for performance-critical code — to be discussed; current closure-based ST is only ~4% slower than direct mutations
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

## Testing

- [x] Comprehensive test suites for each library module (Regex, JSON, LOS, LString, LBit, LMath)
- [x] Stress tests for compiler limits (large ADTs, deep recursion, nested exprs, many functions/instances, long do-blocks, large patterns, deep types, many args, list ops, BST program)
- [x] Do-notation regression tests (eval order, let scoping, bind return unwrapping)
- [x] 221 tests passing (Lua 5.4 via mlua)

## Can defer

- [x] Lambda pattern matching

## String types (design decision)

String = Lua string permanently. ByteString = Lua string with explicit byte semantics (same runtime representation, type-level distinction only). Text = future UTF-8 type over ByteString, if/when Unicode support is needed.
