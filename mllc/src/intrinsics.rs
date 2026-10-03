//! The runtime intrinsic tables: ONE table per family, every consumer.
//!
//! Two families live here. `ST_INTRINSICS` is the ST array and IORef
//! family (below). `RUNTIME_CALLBACKS` states, for every name the Lua
//! runtime implements whose parameter is a FUNCTION, how many arguments
//! the runtime passes that function in one call (see `RuntimeCallback`).
//!
//! An intrinsic is a mata-ll name with no mata-ll body — its behaviour is
//! the Lua runtime's — that codegen spells in two shapes: a first-class
//! action closure (`readSTArray` → `__mll_ma_read(arr, i)`, a suspended
//! action a runner performs) and, in run-once do-block position, a fused
//! direct call (`__mll_st_read(arr, i)`, no closure, no runner). Both
//! shapes force the same argument positions — the array/ref and the index
//! always (nothing reads or stores through a thunk), the stored value and
//! initializer never (a slot holds the value as given: GHC's boxed STArray
//! and lazy `writeIORef`), the function of a strict modify (it is called).
//!
//! Before this table each consumer kept its own copy of the family — the
//! strictness rows in demand.rs, the run-position demand rows, the
//! `sanitize_name` renames, the fused-name and fused-mask matches in
//! action.rs, the known-name seeds in module.rs and mono.rs — and a change
//! to the laziness of one position (G8) had to be made in five files in
//! step. Every one of those sites now reads this table; the runtime bodies
//! themselves are held to the masks by the strictness-contract harness in
//! mll-tests, and the names to the runtime text by the prelude-name check
//! in codegen.

#[doc(hidden)]
pub struct StIntrinsic {
    /// The mata-ll name at TIR call sites (the mask key, the source name).
    pub source: &'static str,
    /// The first-class action closure the name compiles to (the
    /// `sanitize_name` target): builds the action, performs nothing.
    pub closure: &'static str,
    /// The run-once fused form (codegen's `st_intrinsic_fused`): performs
    /// the effect directly and returns the value.
    pub fused: &'static str,
    /// Per-argument strictness: `true` where the runtime forces the
    /// position on every path (the fused call at the call, the closure
    /// when run), `false` where the value is stored as given.
    pub mask: &'static [bool],
}

const fn row(
    source: &'static str,
    closure: &'static str,
    fused: &'static str,
    mask: &'static [bool],
) -> StIntrinsic {
    StIntrinsic { source, closure, fused, mask }
}

/// The intrinsic family. Order is the registration order of the known
/// names; nothing depends on it.
#[doc(hidden)]
pub const ST_INTRINSICS: &[StIntrinsic] = &[
    // ST array primitives: array and index forced, the initializer /
    // stored value stored as given (`writeSTArray a i undefined` never
    // read is silent), the list of `newSTArrayFromList` walked (its
    // elements stay lazy), `readSTArray` forcing the slot it returns,
    // `modifySTArray` calling f and storing f's result forced.
    row("newSTArray", "__mll_ma_new", "__mll_st_new", &[true, false]),
    row("readSTArray", "__mll_ma_read", "__mll_st_read", &[true, true]),
    row("writeSTArray", "__mll_ma_write", "__mll_st_write", &[true, true, false]),
    row("modifySTArray", "__mll_ma_modify", "__mll_st_modify", &[true, true, true]),
    row("stArrayLength", "__mll_ma_length", "__mll_st_length", &[true]),
    row("newSTArrayFromList", "__mll_ma_from_list", "__mll_st_from_list", &[true]),
    row("stArrayToList", "__mll_ma_to_list", "__mll_st_to_list", &[true]),
    // IORef primitives: the cell forced, the value positions lazy on every
    // path (GHC parity: `newIORef`/`writeIORef` don't force the value,
    // `modifyIORef` stores the suspension `f old`); `modifyIORef'` calls f
    // and forces its result on the run, hence strict in f.
    row("newIORef", "__mll_ref_new", "__mll_ioref_new", &[false]),
    row("readIORef", "__mll_ref_read", "__mll_ioref_read", &[true]),
    row("writeIORef", "__mll_ref_write", "__mll_ioref_write", &[true, false]),
    row("modifyIORef", "__mll_ref_modify", "__mll_ioref_modify", &[true, false]),
    row("modifyIORef'", "__mll_ref_modify_strict", "__mll_ioref_modify_strict", &[true, true]),
];

/// The intrinsic named `source` at a TIR call site, if any.
#[doc(hidden)]
pub fn st_intrinsic(source: &str) -> Option<&'static StIntrinsic> {
    ST_INTRINSICS.iter().find(|i| i.source == source)
}

/// The intrinsic whose fused form is `fused`, if any.
#[doc(hidden)]
pub fn st_intrinsic_by_fused(fused: &str) -> Option<&'static StIntrinsic> {
    ST_INTRINSICS.iter().find(|i| i.fused == fused)
}

/// The rows as `(source, mask)` pairs — the shape of the demand.rs
/// strictness tables, for the consumers that chain the tables.
#[doc(hidden)]
pub fn strictness_rows() -> impl Iterator<Item = (&'static str, &'static [bool])> {
    ST_INTRINSICS.iter().map(|i| (i.source, i.mask))
}

/// One function-typed parameter of a runtime-implemented name: the runtime
/// calls the function it receives there with exactly `arity` arguments in
/// one Lua call.
///
/// Compiled mata-ll functions follow the N-ary convention: a value of type
/// `T` is a Lua function of `count_arrows(T)` parameters, applied to all of
/// them in one call. Compiled code meets that convention at every call site
/// because monomorphization gives each instantiation its own copy. The
/// runtime's definitions have no mata-ll body and exist ONCE, so they call
/// their function parameter at its GENERIC arity — the arrow count of the
/// declared parameter type (`map`'s `f :: a -> b` gets one argument,
/// `foldr`'s `f :: a -> b -> b` two). Where the declared result is a type
/// variable, an instantiation at a function type (`modifyIORef r (\h x ->
/// …)` at `a = Int -> Int`, a list of adders built by `map`) hands the
/// runtime a function of MORE parameters than it passes; Lua binds the
/// missing ones to nil and the body runs on them.
///
/// Codegen closes that gap at every place a value enters such a position
/// (the argument of a call, the parameter of a partial application or an
/// eta-expanded first-class reference, the callee of an FFI callback):
/// when the delivered function's arrow count exceeds `arity`, it is wrapped
/// in the currying adapter for `arity` (`__mll_curry1`, `__mll_curry2`, …),
/// which takes the runtime's `arity` arguments and returns the function of
/// the rest — the shape a compiled partial application has.
///
/// The table must name EVERY function-typed parameter of every name the
/// runtime implements, also those whose declared result is concrete
/// (`filter`'s predicate returns Bool, so it can never widen): codegen's
/// `runtime_callbacks_cover_every_function_parameter` test derives the list
/// from the builtin type signatures and fails on a missing or wrong row, so
/// a new runtime higher-order function cannot be added without stating the
/// arity it calls its function parameters with.
#[doc(hidden)]
pub struct RuntimeCallback {
    /// The mata-ll name at TIR call sites.
    pub source: &'static str,
    /// The 0-based parameter position of the function.
    pub param: usize,
    /// The number of arguments the runtime passes that function per call.
    pub arity: usize,
}

const fn cb(source: &'static str, param: usize, arity: usize) -> RuntimeCallback {
    RuntimeCallback { source, param, arity }
}

/// The runtime's function-typed parameters. Order is irrelevant.
#[doc(hidden)]
pub const RUNTIME_CALLBACKS: &[RuntimeCallback] = &[
    // The lazy list generics (runtime.lua `map`/`filter`/`zipWith`): each
    // head is the suspended `f x` / `f x y`.
    cb("map", 0, 1),
    cb("filter", 0, 1),
    cb("zipWith", 0, 2),
    // The type-erased Foldable twins, reached by a fold whose structure
    // type never resolved: `f x acc` / `f acc x`.
    cb("foldr", 0, 2),
    cb("foldl", 0, 2),
    cb("foldl'", 0, 2),
    // The IORef and ST array modifiers store / call `f old`.
    cb("modifyIORef", 1, 1),
    cb("modifyIORef'", 1, 1),
    cb("modifySTArray", 2, 1),
    // The ByteString traversals (`__mll_bs[14]`, `[15]`, `[17]`).
    cb("bsMap", 0, 1),
    cb("bsFoldl", 0, 2),
    cb("bsZipWith", 0, 2),
    // `catch` runs `handler msg` when the action raises.
    cb("catch", 1, 1),
];

/// The callback row for parameter `param` of runtime name `source`.
#[doc(hidden)]
pub fn runtime_callback(source: &str, param: usize) -> Option<&'static RuntimeCallback> {
    RUNTIME_CALLBACKS.iter().find(|c| c.source == source && c.param == param)
}

/// Does runtime name `source` take a function-typed parameter?
#[doc(hidden)]
pub fn has_runtime_callback(source: &str) -> bool {
    RUNTIME_CALLBACKS.iter().any(|c| c.source == source)
}

/// The runtime adapter that presents a function of more parameters as one
/// taking `arity` arguments and returning the function of the rest:
/// `(name, true)` for a fixed-arity adapter called as `name(f)`, or
/// `("__mll_curry_n", false)` for the general one called as `name(f, arity)`.
#[doc(hidden)]
pub fn curry_adapter(arity: usize) -> (&'static str, bool) {
    match arity {
        1 => ("__mll_curry1", true),
        2 => ("__mll_curry2", true),
        _ => ("__mll_curry_n", false),
    }
}
