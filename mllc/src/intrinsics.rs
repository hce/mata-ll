//! The ST array and IORef intrinsics: ONE table, every consumer.
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
