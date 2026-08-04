# DESIGN

Design notes for mata-ll. This document describes what *is* and how it is
achieved, as opposed to SPEC.md which describes what *should be*.


## Compilation pipeline

The compiler is split into two Rust crates: `mllc` (the library) and
`mll` (the CLI). A third crate, `mllc-wasm`, exposes `compile_mll()`
for the browser playground. The pipeline is:

    Source (.mll)
        → Lexer          tokens
        → Parser          AST
        → Import resolver merged AST (prelude + imports + user module)
        → Desugarer       AST with do-notation eliminated
        → Type checker    Typed IR (TIR)
        → Monomorphizer   specialized TIR
        → Code generator  Lua source
        → .lua output

Between the monomorphizer and the code generator sit four smaller
TIR→TIR passes: verification (`verify.rs`, post-mono invariant checks),
constant folding (`fold.rs`), expression splitting (`split.rs`, hoists
deep nesting into `let`s so the emitted Lua stays within Lua's own
parser limits), and dead-code elimination (`dce.rs`).

Each stage is a separate module (`lexer.rs`, `parser.rs`, `modules.rs`,
`desugar.rs`, the `typechecker/` directory, `mono.rs`, and the
`codegen/` directory). The prelude (`lib/Prelude.mll`) is embedded at
compile time via `include_str!` and prepended to every module before
desugaring.


## Lexer

The lexer is layout-sensitive. It emits `Indent(n)` tokens at the
start of each indented line and `Newline` tokens between lines. The
parser uses these to determine where declarations begin and where
continuation lines belong. Blank lines and comment-only lines are
skipped.

Identifiers starting with a lowercase letter produce `Ident` tokens;
those starting with uppercase produce `UpperIdent`. Operators are
sequences of the characters `! # $ % & * + . / < = > ? @ ^ | - ~ :`,
except that `->`, `=>`, `::`, `<-`, `=`, `|`, `..` are recognized as
distinct tokens. Block comments `{- ... -}` nest.


## Parser

The parser is a hand-written recursive-descent parser with operator
precedence climbing for infix expressions. It maintains a fixity table
(`HashMap<String, (Assoc, u8)>`) seeded before parsing from a token
scan of the module's own `infixl`/`infixr`/`infix` declarations (a
declaration governs the whole module, not just the text below it) and
from the fixities of imported modules and the implicit Prelude, which
the module loader collects (fixity travels with an import, as in GHC).
While climbing, the operator whose right-hand side is being parsed is
carried into the recursion so same-precedence neighbors can be
checked: a chain is rejected when either operator is non-associative
or their associativities disagree — the GHC precedence-parsing rule.
Backtick notation (`` `foo` ``) turns any function into an infix
operator.

Layout-sensitivity is handled by an indentation stack. Continuation
lines indented deeper than the start of an expression are merged into
it. This gives Haskell-like layout without explicit braces.

Record construction and record update use the same cross-line
continuation rule as application arguments: the `{ … }` brace may open
on a following line, provided that line is indented strictly past the
enclosing layout block's column. Chained updates may also break the
line between braces, matching GHC's postfix grammar.

The parser produces an AST with these main node kinds:

- **Declarations**: type signatures, function definitions, data/newtype
  definitions, class/instance declarations, export signatures, type
  families, imports, fixity declarations.
- **Expressions**: variables, constructors, literals, application,
  lambdas, infix application, negation, if/then/else, case, let/in,
  do blocks, type ascriptions, record construction, tuples.
- **Patterns**: variable, wildcard, constructor, literal, tuple.
- **Types**: concrete, variable, application, arrow, list, IO,
  ScopedLuaIO, Forall, LuaPure, LuaIO (FFI), LuaIterator, LuaTry,
  LuaCatch, LuaIOCatch, tuple, constrained.

GADTs are detected by a `where` keyword after the type name in a data
declaration. Each GADT constructor carries its full type signature
rather than a field list.


## Desugaring

A single pass over the AST that eliminates do-notation:

    do { x <- e; rest }     →  e >>= \x -> rest
    do { e; rest }           →  e >>= \_ -> rest
    do { let x = e; rest }   →  let x = e in rest
    do { e }                 →  e

Guards and where clauses are preserved as-is for the type checker.


## Type system

### Hindley-Milner with extensions

The type checker uses Robinson unification at its core. Type variables
carry a unique `u32` ID; rigid (skolem) variables use `u32::MAX` and
refuse to unify with anything else. Type schemes (`Scheme`) quantify
over a list of type variables.

Top-level definitions require explicit type signatures. The checker
operates in synthesis mode (infer and unify) for most sub-expressions,
with the top-level signature providing the starting type that flows
inward.

### Kind system

A small, fixed kind system:

- `Type` — the kind of ordinary types
- `Symbol` — the kind of type-level strings (used in FFI)
- `Arrow(k1, k2)` — the kind of type constructors
- `Promoted(name)` — the kind a data type promotes to under DataKinds

No kind polymorphism; promoted data types have real kinds (DataKinds —
see SPEC). Only parameterless, non-GADT, non-existential data types
promote, which keeps promotion monomorphic. The checker validates
kinds for data definitions, type families, and type applications.

### Typeclasses

The built-in classes are `Functor`, `Applicative`, `Monad`,
`Foldable`, `Traversable`, `Enum`, `Bounded`, `Show`, `Read`, `Eq`,
`Ord`, and the numeric hierarchy `Num`, `Fractional`, `Real`, and
`Integral`. Each class is registered with its methods and a set of
built-in instances for primitive types. (`Semigroup` and `Monoid` are
ordinary source classes in the bundled Prelude, not compiler
built-ins.) User-defined classes and instances are also supported.

Instance resolution maps `(class_name, type_name)` to an `InstanceInfo`
containing the mangled method names (e.g., `eq_Int`, `show_String`,
`ord_lt__Number`). Superclass constraints are tracked.

The numeric classes plug into this same machinery without a new code
path. Their operator methods (`+`, `-`, `*`, `/`, `div`, `mod`, `quot`,
`rem`) are registered in the `Int`/`Number` instances as mapping to
*themselves* — the instance method for `+` at `Int` is literally
`+`. The monomorphizer already leaves a class method whose resolved
implementation equals the operator as an ordinary `InfixApp` (the trick
that keeps IO's `>>=` inline), so at a concrete numeric type `+`/`-`/`*`
stay bare Lua operators and `div`/`mod`/`quot`/`rem` stay on their
strict runtime cores — byte-identical to before the classes existed,
with no dictionary. A *user* numeric type instead names real instance
functions, which the monomorphizer dispatches to as a call. A numeric
literal is `fromInteger`/`fromRational` applied to the raw literal; at
`Int`/`Number` that conversion is the identity and is erased in
codegen, while a user `Num` type materialises the instance's
`fromInteger` call around the literal. Unconstrained numeric variables
are resolved by GHC-style defaulting (`Int`, then `Number`) during
constraint discharge.

Orphan instances (where neither the class nor the type is defined in
the current module) are rejected — in the MAIN module only. Imported
modules, the stdlib included, may declare instances for types they do
not define (the `JSON` module carries `ToJSON`/`FromJSON` for `Int`,
`[a]`, …): mata-ll compiles the whole program together, so there is no
cross-build incoherence for the rule to guard against in library code.

Deriving is supported for `Show`, `Eq`, `Ord`, `Enum`, `Bounded`,
`Functor`, `Generic`, `ToJSON`, `FromJSON`, and `LuaDict`.

### GADTs

GADT constructors carry their full return type. Pattern matching on a
GADT constructor introduces local type equalities via unification.
The refinement is purely compile-time; runtime representation is
identical to standard ADTs.

### Rank-2 types

`forall s.` quantification is supported, with two motivating patterns:

1. Exported functions receiving `LuaFunction s` (scope safety for
   Lua callbacks, same mechanism as Haskell's ST monad).
2. `runST :: (forall s. ST s a) -> a` (scope safety for mutable
   state).

Rank-2 function arguments also work in general — a parameter of type
`(forall a. a -> a)` may be instantiated at several types within the
body — though the scope-sealing patterns above are the cases the
language is designed around.

### Type families

Both intrinsic and user-defined type families are supported. The
intrinsic ones (`LuaPure`, `LuaIO`) reduce during type checking:

    LuaPure "name" a  →  a
    LuaIO "name" a    →  IO a

User-defined type families use closed, equation-based matching. A
family may be declared with header parameters and no equations — its
arity (and kind) comes from the header — and the compiler can extend
such a family itself: `deriving (Generic)` adds one `Rep T = <rep>`
equation per derive to the equation-less `Rep` family that
`Data.Generics` declares.

A stuck family application participates in inference rather than
blocking it: instance checking defers a constraint whose head is an
unreduced family application (treated like a bare variable — reduce if
concrete, satisfiable otherwise), and the unifier decomposes two
applications of the SAME family argument-wise so a fresh metavariable
can be bound (`Rep α ~ Rep a  ⟹  α := a`, the most-general unifier —
not an injectivity deduction; two distinct rigids still fail with the
family-level mismatch). This is what lets `to (from x)` type-check and
lets a `GC (Rep a)` constraint ride a polymorphic signature until
monomorphization fixes `a`.

### Generics

`deriving (Generic)` gives a concrete type a structural representation.
The compiler contributes only synthesis; everything downstream is
ordinary library code (`Data.Generics`):

- **Representation types**: a sum of products built from `U1` (empty
  product), `K1 c` (field leaf), `L1`/`R1` of `a :+: b` (constructor
  choice), `Prod` of `a :*: b` (fields), wrapped in the metadata
  carriers `D1 d f` / `C1 c f` / `S1 s f`. All of kind `Type` — mata-ll
  drops GHC's phantom index, and three distinct wrappers replace GHC's
  single tagged `M1 i c f` so each layer dispatches on its own head
  under head-keyed instance resolution.
- **Derive synthesis** (`derive_generic`): one `Rep T` family equation;
  `from`/`to` conversion functions as TIR; and per-datatype /
  constructor / field MARKER types (`__Meta_D_T`, `__Meta_C_T_Con`,
  `__Meta_S_T_Con_i` — nullary, kind `Type`, never inhabited) carrying
  baked-string `Datatype`/`Constructor`/`Selector` instances
  (`datatypeName`, `datatypeConCount`, `conName`, `conArity`,
  `conIsRecord`, `selName`). Marker-keyed instances are how per-name
  metadata reflection works under head-keyed dispatch; the reflected
  names are the effective external names (`as` renames applied).
- **Resolution**: mata-ll's constraint core is single-variable
  (`Class var`), so a generic function's `GC (Rep a)` constraint is
  never solved as a compound given — it defers until monomorphization
  specialises the function at a concrete `a`, reduces `Rep a`, and
  resolves the instance chain. Class-variable binding blanks
  non-injective family applications in a method's declared type, so
  `from :: a -> Rep a` binds `a` from the argument, never from inside
  `Rep a`.
- **Producers navigate by proxy**: a generic consumer walks a rep value
  it already has; a generic producer (a decoder) must pick instances
  before any value exists. `Data.Generics` exports `gProxy :: a` (a
  bottom that is never forced — the metadata methods ignore their
  argument) and the `p*` re-typers (`pD1`, `pC1`, `pS1`, `pK1`,
  `pSumL`, `pSumR`, `pProdL`, `pProdR`) that retype a proxy for each
  representation layer without matching on it.

The `JSON` module's `genericToJSON`/`genericFromJSON` are the worked
proof: byte-exact against the derived native codecs — wire format and
error messages — pinned by the generic_json* integration tests. The
derives themselves keep their specialised native generators for speed.

### Linear types (multiplicity)

`Ty::Arrow` carries a `Mult` (`One`, `Many`, a flexible inference `Var`,
or a rigid signature variable `Rigid`) as a third field. `Mult`'s
`PartialEq`/`Hash` are deliberately identity-blind — any `Mult` equals
any other — so a `%1` arrow and a plain arrow are the *same* type for
every existing map key, cache and comparison; only the unifier (which
handles the slot explicitly) and the usage checker ever read it.
Multiplicity variables have their own id namespace and their own slot in
`Subst`, so minting them never perturbs type-variable numbering.
Unification of multiplicity is invariant (`One` ≠ `Many`, as in GHC's
`LinearTypes`); arrows the inference engine invents — the expected arrow
at an application, a lambda's own arrows — get fresh multiplicity
variables, which is how a lambda checked against a `%1` parameter learns
its binder's restriction.

Enforcement is *not* threaded through unification. It is a separate usage
pass (`typechecker/usage.rs`) that runs at the end of `check_function`
over the fully-substituted typed IR, counting 0/1/ω uses per variable
(sequential add, branch-join max) and scaling by context. The discipline
is *exactly-once* (GHC's linear semantics), so the pass keeps the usage
count separate from the per-binder policy and enforces a lower bound too:
a tracked binder absent from its scope's usage at a check point is a
leak, and a binder consumed in only some branches of an alternative group
leaks on the skipped path. Aliases (pattern binders of a match on a `%1`
value, `<-` binders, `let`/`where` bindings) inherit the obligation. The
module comment in `usage.rs` states the enforced fragment and every
boundary precisely.

**Design decision: no scalar exemption (strict GHC parity).** An earlier
phase relaxed scalar aliases to *at-least-once* — usable repeatedly, on
the reasoning that the runtime memoizes the thunk so duplicating a scalar
is operationally harmless. That relaxation was dropped: a scalar derived
from a `%1` value is now tracked exactly-once like every other alias. The
relaxation had stopped tracking a scalar once it flowed into unrestricted
position, which opened an accept-direction hole (a pending consumption
parked in a never-forced scalar thunk could be counted as consumed). The
project's north star is strict GHC parity wherever feasible — an
almost-but-not-quite-Haskell semantics is a worse trap than a plainly
missing feature — and GHC has no `Movable`-style scalar rule in its type
system, so parity plus soundness won over the operationally-sound-but-
non-GHC relaxation. The cost, accepted deliberately, is that the harmless
scalar-duplication idiom (`go + go where go = useOnce t`) now rejects.
Only `()`-typed derived results stay exempt (the run-for-effect idiom).

Everything erases after type checking — the backend ignores the `Mult`
field entirely and the emitted Lua is byte-identical with or without
annotations (regression-tested). Dictionaries are never linear:
dictionary-passing is introduced by the monomorphizer after this pass,
and class-method arrows default to `Many`.

### Error handling

Type errors are accumulated per definition and reported together: a
file with errors in several definitions reports one error for each of
them in a single run. Within a single definition, checking stops at
the first error, so only that one is reported for the definition.
Error kinds include unification mismatches, occurs-check failures,
unbound variables/constructors, arity mismatches, non-exhaustive
patterns, and signature mismatches.


## Monomorphization

### Strategy

The monomorphizer walks the typed IR, collecting concrete type
instantiations. For each unique `(function_name, concrete_type)` pair
it generates a specialized copy with a mangled name (e.g.,
`map_Int_List_Int`). Call sites are rewritten to use the
specialized name.

Typeclass method calls (e.g., `show`, `==`) are resolved to their
concrete instance functions during this pass.

### Polymorphic recursion fallback

When a function calls itself at progressively different types (e.g.,
`showDeep :: Show a => Deep a -> String` calling itself at
`Deep (Box a)`), monomorphization would diverge. The monomorphizer
counts specializations per function. When the count exceeds 16, it
switches that function to dictionary-passing: typeclass methods are
looked up from a Lua table parameter passed at each call site. Existing
specializations for that function are discarded and their call sites
reverted to the original name.

The same guard covers INSTANCE METHODS, which generic code trips
legitimately (a rep-combinator instance is specialised once per
constructor or field across every `deriving (Generic)` type in the
program). An instance method past the guard is purged to
dictionary-passing exactly like a top-level function; a dictionary for
a parameterized instance is COMPOSED from the instance's
dictionary-form methods closed over the sub-dictionaries its context
demands, built recursively (`__mll_dictc`). Details that make this
sound:

- The dictionary phase runs to a FIXPOINT: rewriting one body can
  discover further dictionary-passing functions (a sibling constrained
  call marks its callee; composing a dictionary can trip a fresh cap),
  each needing its own body rewrite and another call-site pass.
  Functions generated during the phase get the same treatment before
  emission.
- A dictionary-passing body is re-monomorphized from its pristine
  polymorphic copy under an `in_dictform` flag that suppresses the
  arbitrary-specialization fallback — otherwise a still-polymorphic
  call inside a SHARED body gets welded to one type's specialization
  (one constructor's baked metadata serving every type).
- Inside a dictionary-passing body, a call to ANY constrained
  polymorphic function at a still-polymorphic type passes dictionaries
  (not just self-recursion): the callee's constraints are bound against
  the call's argument types and each dictionary is either the enclosing
  parameter or a freshly composed one.
- Value arguments of dictionary-passing calls take the ordinary lazy
  call protocol — the callee may never demand one (a decoder's type
  proxy), and evaluating a possibly-⊥ argument eagerly would raise
  where GHC would not.

This gives zero-overhead dispatch for the common shallow cases and
bounded, correct-at-any-scale overhead past the guard.


## Code generation

The code generator is a module directory, `mllc/src/codegen/`, split
by concern: `mod.rs` (the `CodeGen` state struct, name resolution,
local declaration), `lua.rs` (a small Lua AST and its printer),
`module.rs` (module body layout: data-type registration, constructors,
forward declarations, exports), `function.rs` (top-level functions,
clauses, where-binding groups), `pattern.rs` (pattern-match
compilation), `expr.rs` (the main expression walk), `thunks.rs` and
`strictness.rs` (eager-vs-thunk decisions), `action.rs` (IO/ST bind
chains), `inline.rs`, `analysis.rs` (whole-program call-site and
inlining analyses), `ffi.rs`, `names.rs`, `util.rs`, and `runtime.rs`
plus `runtime.lua` (the runtime prelude).

Emission is AST-based: the generators build a `lua::Stmt`/`lua::Expr`
tree and the tree is printed once at the end. No generator writes
output text directly, so statement well-formedness and grouping are
carried by structure rather than re-proven at each emission site.

Before printing, an optimization pipeline (`opt.rs`) runs over the
finished statement list: paren normalization (grouping parens are
explicit nodes placed defensively by emission sites; the pass drops
one exactly where the enclosing position proves it redundant, keeping
`return f(x)` a proper tail call while preserving the
paren-as-truncation semantics around possibly multi-returning
callees), dead-branch cleanup (the `otherwise` arm becomes `else`,
complementary two-arm chains collapse to if/else, statements after a
diverging statement are dropped), IIFE flattening (value- and
return-position `case`/`let` closures splice into the enclosing
block, budgeted against Lua's 200-local limit), and a
force-of-known-WHNF safety net (`__force(x)` of a single-assignment
local whose one value is WHNF by construction rewrites to `x`).
`Raw` nodes are opaque to every pass. The pipeline runs before
printing, so the on-demand prelude scan sees the optimized body and
shrinks with it.

Generated output is deterministic: compiling the same source twice
produces byte-identical Lua. The last source of non-determinism
(specialization resolution order in the monomorphizer's
dictionary-passing fallback) was removed, and the property is guarded
by the `codegen_is_deterministic` test.

### Lua runtime preamble

Every generated `.lua` file begins with a preamble defining the
runtime support functions. Its source is `codegen/runtime.lua`,
embedded into the compiler via `include_str!` and emitted *on demand*:
the generator scans the printed program body and prepends only the
prelude definitions it transitively references. The full prelude
includes thunk infrastructure, list primitives, `show`/`eq`/`ord`
instances for primitives, list operations (`map`, `filter`, `take`,
`zipWith`), HashMap operations, ByteString operations, STArray
operations, bitwise operations, and FFI helpers.

### ADT representation

Multi-constructor types use Lua tables with an integer tag at index 1
and fields at subsequent indices:

    data A = A String | B Int Int

    A "hello"  →  {1, "hello"}
    B 17 23    →  {2, 17, 23}

Single-constructor types with exactly one field (including newtypes)
are not represented as Lua tables — the value *is* the field directly.
Newtypes compile to identity functions:

    newtype Radians = Radians Number
    →  local function Radians(_v) return _v end

Pure enums (all constructors zero-arity) are plain Lua integers:

    data Color = Red | Green | Blue
    →  local Red = 1; local Green = 2; local Blue = 3

### Record backing store

Records are backed by plain Lua tables with integer keys matching
constructor field order. These tables are created and consumed
exclusively by mata-ll generated code; plain Lua does not interact
with them directly.

Record field accessors are generated as simple index functions:

    local function perName(_r) return _r[1] end

#### Record update

Record update syntax (`foo { x = 3 }`) produces a new table that shares
all fields with the original except the updated ones. This is a
**shallow copy**: iterate the table slots and patch the changed fields.

A shallow copy is correct because:

- Field values are either Lua primitives or references to other mata-ll
  values (tables or thunks). These are all safe to share by reference.
- A deep clone would be wrong: it would force thunks prematurely or
  duplicate shared structure, breaking both laziness and identity
  semantics.
- The tables are exclusively managed by mata-ll, so there is no risk of
  external mutation violating the immutability invariant.

Cost is O(n) where n is the total number of fields in the record,
since all slots are copied regardless of how many are updated. This
is acceptable for typical record sizes.

### Maybe representation

`Nothing` is Lua `nil`. `Just` is the identity function. This makes
Maybe zero-cost for the common case and allows pattern matching via
`== nil` / `~= nil`.

### List representation

Lists are cons cells: two-element Lua tables `{head, tail}` where
`nil` is the empty list. Lazy tails use `__mll_lazy_cons(head, thunk)`
which sets a `__lazy` flag; `__mll_tail` forces the thunk on first
access and clears the flag.

The runtime provides `map`, `filter`, `take`, and `zipWith` as Lua
functions that produce lazy cons cells, enabling infinite lists and
fusion-like behavior.

### Tuple representation

Plain Lua tables with integer indices: `{e1, e2, e3}`. No tag; the
type system distinguishes tuples from ADTs.

### Closures and partial application

Functions with multiple parameters compile to multi-argument Lua
functions. When fully applied, the call is a direct multi-argument
call. Partial application generates a closure:

    add :: Int -> Int -> Int
    add a b = a + b
    inc = add 1

    →  local function add(a, b) return a + b end
       local function inc(a) return add(1, a) end

### Pattern matching

Pattern matching compiles to nested if/elseif chains. Constructor
tags are checked at index 1; fields are extracted from subsequent
indices. Each clause becomes a branch. Guards are interleaved as
additional conditions within branches. Non-exhaustive patterns fall
through to `error("Non-exhaustive patterns")`.

### Forward declarations

All user-defined functions are forward-declared (`local f1, f2, ...`)
before any definitions, enabling mutual recursion without ordering
constraints. Forward-declared names are marked concrete so references
to them skip `__force`.

### Exports

Exported functions appear in the module's return table. Each export is
wrapped to deep-force return values (via `__mll_to_lua`) and wrap Lua
callback arguments (via `__mll_wrap_callback`) so that the boundary
between mata-ll and plain Lua is clean.

### Standalone mode

When the module has a `main :: IO ()` declaration, the compiler
appends an entry-point stub at the end of the generated Lua file:

    local __mll_arg1 = ...
    if __mll_arg1 == nil or (arg ~= nil and __mll_arg1 == arg[1]) then
        __mll_run(__mll_fn[N]())
    end

so the file runs `main` when executed directly but stays inert when
loaded via `require`. The discriminator: a standalone interpreter
(`lua prog.lua x y`) fills both the chunk's varargs and the global
`arg` table from the same command line, so the first vararg equals
`arg[1]` (and is nil when there are no arguments); `require "prog"`
instead passes the module name as the first vararg, which does not
match `arg[1]`. `main` is renamed to `__run` internally because it is not an
exported function; the stub reaches it through its function-table
slot. The CLI can also execute the result directly via the embedded
`mlua` runtime (`--run` flag).


## Evaluation strategy

### Non-strict evaluation

Function arguments and let/where bindings are wrapped in memoizing
thunks by default. A thunk is a two-element Lua table with a
metatable: `{thunk_fn, forced_flag}`. Forcing a thunk calls the
function, replaces it with the result, and sets the flag:

    local __thunk_mt = {}
    local function __thunk(f)
        return setmetatable({f, false}, __thunk_mt)
    end
    local function __force(x)
        if getmetatable(x) == __thunk_mt then
            if x[2] then return x[1] end
            local val = x[1]()
            x[1] = val
            x[2] = true
            return val
        end
        return x
    end

### Cheapness analysis

The code generator decides whether to thunk or eagerly evaluate each
expression. Cheap expressions skip thunk allocation:

- Literals, variable references, constructor applications
- Arithmetic on cheap operands
- Tuple construction of cheap elements
- Applications of known top-level functions to cheap arguments

Expensive expressions (calls to unknown functions, calls to parameters,
complex nested expressions) are wrapped in `__thunk`.

### Demand analysis

A separate pass (`demand.rs`) determines which function parameters are
forced on every code path through the body. A parameter is strict if
it is forced in *all* clauses and all branches within each clause.

Pattern matching forces its scrutinee. Case scrutinees and if
conditions are always strict. The analysis intersects strictness
across clauses: a parameter is strict overall only if strict in every
clause.

Strict parameters can be passed eagerly at call sites (avoiding thunk
allocation) and forced at function entry rather than at each use site.

### Concrete variable tracking

The code generator maintains a set of names known to hold non-thunk
values (`concrete_vars`). References to concrete variables skip the
`__force()` call entirely. The set is seeded with all runtime
primitives and forward-declared function names, and grows as
assignments to known-concrete values are encountered.

### Call-site analysis

A whole-program pass before code generation examines every call site
to determine:

- Which parameters are always passed cheap (non-thunk) arguments
- Which parameters are ever called as functions (enabling a different
  optimization path)

When all callers pass concrete values for a given parameter, the
function entry can skip forcing that parameter entirely.

### Inlining

Small, pure, non-recursive functions with a single clause and no
guards are identified as inline candidates. At call sites, their
bodies are substituted with parameters replaced. This eliminates
function-call overhead for trivial helpers.

### Bind chain flattening

Monadic bind chains (desugared from do-notation into nested
`>>=`/lambda sequences) are flattened into sequential local
assignments:

    do { x <- action1; y <- action2 x; return y }

Instead of generating nested function calls, the code generator
unrolls these into:

    local x = action1()
    local y = action2(x)
    return y

This avoids the overhead of intermediate closures and IIFEs for
sequential IO/ST operations.

### Operator translation

Haskell operators map to Lua:

    ++    →  ..
    &&    →  and
    ||    →  or
    /=    →  ~=
    div   →  //
    mod   →  %


## FFI

### Type families

FFI bindings use type families that the compiler consumes during code
generation and then erases:

- `LuaPure "name" a` — pure call, reduces to `a`. Compiles to a
  direct Lua function call.
- `LuaIO "name" a` — effectful call, reduces to `IO a`. Compiles to
  a thunk-wrapped Lua call forced in monadic context.
- `LuaIterator "name" a` — wraps a Lua iterator factory into a lazy
  mata-ll list via `__mll_iter`.
- `LuaTry "name" (Either String a)` — wraps a Lua function that returns
  `(val, err)` into `IO (Either String a)` via `__mll_try` (a nil value
  counts as a failure). Like the pcall forms below, the result *must* be
  written as `Either String a`; the parser rejects other shapes.
- `LuaCatch "name" (Either String a)` — pure call run under `pcall`,
  reduces to `Either String a`. A raised Lua `error(...)` is captured
  as `Left msg` (via `tostring`), a normal return as `Right a`, with
  the success payload FFI-decoded via `__mll_pcall`. The result *must*
  be written as `Either String a`; the parser rejects other shapes.
- `LuaIOCatch "name" (Either String a)` — the effectful counterpart of
  `LuaCatch`, reducing to `IO (Either String a)`. Same `pcall` capture,
  deferred as an IO action. Use this instead of `LuaTry` when the Lua
  function signals failure by *raising* rather than by the `(nil, err)`
  convention.

`Maybe` arguments in FFI signatures translate to optional Lua
parameters: `Nothing` omits the argument, relying on Lua's `nil`
default.

### LuaIO s (scoped monad)

The parser disambiguates `LuaIO "name" a` (FFI type family, string
literal first argument) from `LuaIO s a` (scoped monad, type variable
first argument). These are distinct AST nodes internally.

`LuaFunction s` is opaque; it must be given a concrete type via
`engage` before calling. The `forall s.` on the enclosing function
seals the scope, preventing the function from escaping. This is the
same mechanism as Haskell's ST monad.

### Export boundary

Exported functions wrap return values with `__mll_to_lua` (deep-forces
thunks, converts cons lists to Lua arrays) and incoming Lua callbacks
with `__mll_wrap_callback` (deep-forces arguments before forwarding).
This ensures the FFI boundary is clean in both directions.


## Module system

Each `.mll` file is a module. Import syntax:

    import Data.Tree                    -- import all
    import Data.Tree (depth, Tree(..))  -- selective
    import qualified Data.Tree as T     -- qualified

`Data.Tree` maps to `Data/Tree.mll` on disk. The module loader
searches the source directory first, then added library paths. Loaded
modules are cached.

The prelude is auto-imported by the compiler (prepended to the AST
before desugaring). Additional standard library modules live in `lib/`:

    ByteString, Control.Monad, Data.Foldable, Data.List, Data.Map,
    Data.Maybe, Data.Traversable, JSON, LBit, LIO, LIOLinear, LMath,
    LOS, LString, Regex


## ST monad and STArray

`ST s a` is the pure mutable-state monad with the same runtime as IO.
The distinction is purely at the type level: `runST :: (forall s. ST s a) -> a`
uses rank-2 quantification to prevent mutable state from escaping.

`STArray s` is a mutable integer array backed by a Lua table. All
operations (`newSTArray`, `readSTArray`, `writeSTArray`,
`modifySTArray`, `stArrayLength`, `newSTArrayFromList`,
`stArrayToList`) carry the scope tag `s` and run in `ST s`. Indices
are 0-based externally, converted to 1-based internally for Lua.

At runtime, `runST` is `__mll_run` (force and call), and the
array operations are plain Lua table manipulations. The scope safety
is enforced entirely by the type checker.


## HashMap

HashMap is a compiler built-in backed by plain Lua tables (using Lua's
native hash-table implementation). It is not a user-defined ADT.
Operations (`hashmap_insert`, `hashmap_delete`, `hashmap_lookup`,
`hashmap_keys`, `hashmap_values`, `hashmap_member`, `hashmap_fromList`)
are provided in the runtime preamble. Insert and delete produce new
tables (shallow copy), preserving immutability.


## ByteString

ByteString is backed by Lua strings with explicit byte semantics. All
operations are implemented as a Lua table of functions (`__mll_bs`)
indexed by operation number. Indices are 0-based externally, converted
to 1-based for Lua's string library internally. Operations include
construction, deconstruction, querying, transforms (map, foldl, xor,
zipWith), and binary encoding (little-endian 8/16/32-bit reads and
writes).
