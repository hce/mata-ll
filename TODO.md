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
- [x] Monadic bind chain flattening (do-blocks → flat IIFEs)
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

## Open

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

- [ ] Typechecker stack overflow on CPS-heavy types (Regex library with many test functions overflows 256MB stack; needs iterative type inference)
- [ ] Top-level let-in value bindings evaluate incorrectly (thunking issue with nested let expressions as top-level values)
- [ ] Inliner captures free variables incorrectly when inlining closures assigned via do-let (e.g. `let add5 = makeAdder 5` inlines body with unbound `n`)
- [ ] `let bomb = error "msg"; const 1 bomb` forces bomb eagerly at call site (Lua evaluates all args; non-strict optimization reverted due to tracker regression from imprecise demand analysis)
- [ ] Multi-line case in do-let can cause multi-line continuation to consume next statement as argument
- [ ] Pattern-matching generators in list comprehensions (`[x | Ok x <- rs]`) not supported

## Testing

- [ ] Comprehensive test suites for each library module (Regex, JSON, LOS, LString, LBit, LMath) — the Regex test already revealed the CPS stack overflow; other modules likely have undiscovered edge cases

## Can defer

- [x] Lambda pattern matching

## String types (design decision)

String = Lua string permanently. ByteString = Lua string with explicit byte semantics (same runtime representation, type-level distinction only). Text = future UTF-8 type over ByteString, if/when Unicode support is needed.
