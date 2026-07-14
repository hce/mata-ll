//! Embedded standard library.
//!
//! The stdlib `.mll` sources live in `mllc/lib/` and are compiled *into* the
//! `mllc` crate with `include_str!`, so a published/installed `mllc` is fully
//! self-contained: it needs no `lib/` directory on disk at runtime. The module
//! loader (see `modules.rs`) consults this table only when a module is not
//! found on any filesystem search path, so a `-L <dir>` override still wins and
//! lets developers iterate on the stdlib without rebuilding the compiler.
//!
//! Keys are dotted module names as written in `import` (e.g. `Data.Map`),
//! matching `module_path.join(".")` in the loader.

/// The Prelude source, always parsed as the compilation baseline.
pub(crate) const PRELUDE: &str = include_str!("../lib/Prelude.mll");

/// All importable stdlib modules, keyed by dotted module name.
pub(crate) const EMBEDDED_MODULES: &[(&str, &str)] = &[
    ("Prelude", PRELUDE),
    ("ByteString", include_str!("../lib/ByteString.mll")),
    ("Control.Monad", include_str!("../lib/Control/Monad.mll")),
    ("Data.Foldable", include_str!("../lib/Data/Foldable.mll")),
    ("Data.List", include_str!("../lib/Data/List.mll")),
    ("Data.Map", include_str!("../lib/Data/Map.mll")),
    ("Data.Maybe", include_str!("../lib/Data/Maybe.mll")),
    ("Data.Traversable", include_str!("../lib/Data/Traversable.mll")),
    ("JSON", include_str!("../lib/JSON.mll")),
    ("LBit", include_str!("../lib/LBit.mll")),
    ("LIO", include_str!("../lib/LIO.mll")),
    ("LMath", include_str!("../lib/LMath.mll")),
    ("LOS", include_str!("../lib/LOS.mll")),
    ("LString", include_str!("../lib/LString.mll")),
    ("Regex", include_str!("../lib/Regex.mll")),
];

/// Look up an embedded stdlib module by its dotted name (e.g. `Data.Map`).
pub(crate) fn embedded_module(dotted_name: &str) -> Option<&'static str> {
    EMBEDDED_MODULES.iter()
        .find(|(name, _)| *name == dotted_name)
        .map(|(_, src)| *src)
}
