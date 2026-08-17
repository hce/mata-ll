//! FFI boundary validation: the marshallability whitelist and the
//! export/import signature checks built on it.
//!
//! Exports (mata-ll functions the Lua host calls) and FFI imports
//! (`LuaPure`/`LuaIO`/… declarations mata-ll calls into Lua with) are exact
//! mirrors: an export decodes its arguments IN from Lua and marshals its
//! result OUT, an import marshals its arguments OUT and decodes its result
//! IN, and a callback inverts whichever side it sits on. One implementation,
//! parameterized over [`BoundaryKind`], validates both — the two sides used
//! to be maintained as hand-mirrored near-duplicates whose direction
//! assignments had to be kept in sync by prose.

use super::*;

/// The direction a value crosses the FFI boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FfiDir {
    /// Lua → mata-ll. Decoded by the argument-decode path.
    Import,
    /// mata-ll → Lua. Marshalled by the result path.
    Export,
}

/// Which side of the FFI boundary a signature describes. Fixes the crossing
/// direction of every position and the diagnostic phrasing.
#[derive(Clone, Copy)]
pub(super) enum BoundaryKind {
    /// An `export`ed mata-ll function the host calls: arguments arrive from
    /// Lua, the result goes out.
    Export,
    /// An FFI import mata-ll calls into Lua: arguments go out to the host,
    /// the result comes back in.
    FfiImport,
}

impl BoundaryKind {
    /// The direction a top-level argument of the signature crosses.
    fn arg_dir(self) -> FfiDir {
        match self {
            BoundaryKind::Export => FfiDir::Import,
            BoundaryKind::FfiImport => FfiDir::Export,
        }
    }

    /// The direction the result crosses — always the arguments' opposite.
    fn result_dir(self) -> FfiDir {
        match self {
            BoundaryKind::Export => FfiDir::Export,
            BoundaryKind::FfiImport => FfiDir::Import,
        }
    }

    /// How the failing crossing is described in the diagnostic.
    fn dir_phrase(self, dir: FfiDir) -> &'static str {
        match (self, dir) {
            (BoundaryKind::Export, FfiDir::Import) =>
                "cross into mata-ll from Lua (argument direction)",
            (BoundaryKind::Export, FfiDir::Export) =>
                "cross out to Lua from mata-ll (result direction)",
            (BoundaryKind::FfiImport, FfiDir::Import) =>
                "cross into mata-ll from Lua (the host's return value)",
            (BoundaryKind::FfiImport, FfiDir::Export) =>
                "cross out to Lua from mata-ll (an argument to the host)",
        }
    }

    /// The diagnostic's subject prefix ("Export 'f'…" / "FFI import 'f'…").
    fn subject(self) -> &'static str {
        match self {
            BoundaryKind::Export => "Export",
            BoundaryKind::FfiImport => "FFI import",
        }
    }

    /// The diagnostic's context line.
    fn context(self) -> &'static str {
        match self {
            BoundaryKind::Export => "export declaration of",
            BoundaryKind::FfiImport => "FFI declaration of",
        }
    }
}

/// The scalar type names the FFI marshaller round-trips as bare Lua values
/// (derived from `codegen::Codegen::scalar_lua_type`: numbers, strings,
/// booleans). Keep in sync with that function.
fn ffi_scalar_name(n: &str) -> bool {
    matches!(n,
        "Int" | "Number" | "Double" | "Float"
        | "String" | "Char" | "ByteString" | "Bool")
}

/// Peel `App(App(Con(H), a), b)` into `(Some("H"), [a, b])`; the argument-source
/// order dual of `codegen::decompose_app`, so `HashMap k v` → `("HashMap",
/// [k, v])` and `Tree a` → `("Tree", [a])`. A non-`Con` head yields `None`.
pub(super) fn decompose_ty_app(ty: &Ty) -> (Option<&str>, Vec<&Ty>) {
    let mut args: Vec<&Ty> = Vec::new();
    let mut head = ty;
    while let Ty::App(f, a) = head {
        args.push(a.as_ref());
        head = f.as_ref();
    }
    args.reverse();
    match head {
        Ty::Con(n) => (Some(n.as_str()), args),
        _ => (None, args),
    }
}

/// Rename EVERY free type variable in the given types to friendly single
/// letters (`a`, `b`, …), sharing one map so the same variable reads the same
/// on every side of the message. Unlike the diagnostic renderer's
/// `pretty_var_subst` — which preserves user-written names — this renames a
/// freshened user var (`a890`) too, because an export error's variables have no
/// meaningful source name to keep (the export is being rejected *for* being
/// polymorphic) and a leaked internal spelling is noise.
fn friendly_export_tys(tys: &[&Ty]) -> Vec<Ty> {
    let mut vars: Vec<TyVar> = Vec::new();
    for t in tys {
        for v in t.free_vars() {
            if !vars.contains(&v) { vars.push(v); }
        }
    }
    let mut map: HashMap<TyVar, Ty> = HashMap::new();
    for (i, v) in vars.iter().enumerate() {
        map.insert(v.clone(), Ty::Var(TyVar { name: crate::types::pretty_var_name(i), id: v.id }));
    }
    let sub = Subst::from_map(map);
    tys.iter().map(|t| t.apply_subst(&sub)).collect()
}

/// The `note:` line explaining why `culprit` cannot cross in direction `dir`.
fn export_ffi_note(culprit: &Ty, dir: FfiDir) -> String {
    let (head, _) = decompose_ty_app(culprit);
    match culprit {
        Ty::Var(_) | Ty::Skolem(..) =>
            "Lua has no representation for a polymorphic value; give the export a \
             concrete type.".to_string(),
        Ty::IO(_) | Ty::LuaIO(_, _) if dir == FfiDir::Import =>
            "a Lua caller cannot supply an IO/LuaIO action; only a callback (a \
             function returning LuaIO) may cross inward.".to_string(),
        Ty::Arrow(..) =>
            "a callback is only marshalled as a DIRECT top-level argument of the \
             export (returning LuaIO); a function nested in a container/result, or \
             a callback that itself takes a callback, is not.".to_string(),
        _ if matches!(head, Some("ST") | Some("STArray") | Some("STRef")) =>
            "an ST handle is region-scoped (it must not outlive its runST) and has \
             no Lua representation.".to_string(),
        // A plain user/prelude `data` type (Either/Ordering/ExitValue or a
        // user ADT) reached here has real constructors but no designed FFI
        // shape — it would cross only as its internal `{tag, fields…}` table,
        // which the host cannot interpret. Say so and point at the fixes.
        _ if head.is_some() =>
            "this type has real constructors but no FFI representation: it would \
             cross only as mata-ll's internal `{tag, ...}` tagged table, which has \
             no meaning to a Lua host. To carry structured data, use a `LuaDict` \
             record (a name-keyed table); for a dynamic scalar, use `Any`; or \
             encode the value as a scalar or a list. A newtype over a marshallable \
             type crosses transparently; a plain `data` type does not."
                .to_string(),
        _ =>
            "only scalars, (), lists, tuples, Maybe, HashMap, LuaDict records, \
             newtypes over marshallable types, Any, and a top-level callback may \
             cross the FFI boundary. Either crosses only as a LuaTry/LuaIOCatch \
             result.".to_string(),
    }
}

/// Free type variables a callback carries through its argument and result
/// *values*, excluding the phantom `LuaIO s` scope variable.
pub(crate) fn callback_value_vars(cb_ty: &Ty) -> Vec<TyVar> {
    let (args, ret) = cb_ty.peel_arrows();
    let mut vars = Vec::new();
    for a in &args {
        for v in a.free_vars() {
            if !vars.contains(&v) { vars.push(v); }
        }
    }
    let produced = match ret {
        Ty::IO(inner) | Ty::LuaIO(_, inner) => inner.as_ref(),
        other => other,
    };
    for v in produced.free_vars() {
        if !vars.contains(&v) { vars.push(v); }
    }
    vars
}

impl Checker {
    /// Reject exports whose declared type uses something the FFI marshaller
    /// cannot correctly move across the boundary. Runs on each export's FINAL
    /// resolved type — the same `export_types` the code generator marshals from
    /// — so the whitelist is derived from what `ffi_arg_marshal_desc` /
    /// `ffi_decode_desc_inner` / the deep-force fallback actually handle, and a
    /// silently-wrong `"false"`/`__mll_to_lua` conversion never reaches codegen.
    pub(super) fn validate_export_types(&mut self, exports: &[String], functions: &[TFunction], constrained: &[String]) {
        let tys: HashMap<&str, &Ty> = functions.iter()
            .filter(|f| exports.contains(&f.name))
            .map(|f| (f.name.as_str(), &f.ty))
            .collect();
        // Deterministic diagnostics: follow the source export order.
        for name in exports {
            // Already rejected for a class constraint — its type variable would
            // be reported a second time here.
            if constrained.contains(name) { continue; }
            let Some(&ty) = tys.get(name.as_str()) else { continue };
            // Peel a leading rank-2 forall (the LuaIO scope variable etc.).
            let mut t = ty;
            while let Ty::Forall(_, inner) = t { t = inner; }
            // An export result may be an `IO a` the export performs; the
            // marshallability check handles the wrapper itself (allowed in
            // the outgoing direction only), so the result needs no peeling.
            let (arg_tys, res) = t.peel_arrows();
            self.validate_boundary_positions(
                BoundaryKind::Export, name, &arg_tys, res, &[]);
        }
    }

    /// Validate the signature of an FFI IMPORT — a `LuaPure`/`LuaIO`/`LuaTry`/
    /// `LuaIOCatch` (or `LuaCatch`/`LuaIterator`) declaration mata-ll calls INTO
    /// Lua with. `ty` is the FINAL resolved type (the `LuaIO`/… wrappers already
    /// reduced to `IO`/the raw result), `ffi_kind` says which FFI form it is so
    /// the `Try`/`IOCatch` `Either String _` layer is peeled exactly as
    /// codegen's `ffi_catch_decode_desc` does.
    pub(super) fn validate_ffi_import_types(&mut self, name: &str, ffi_kind: FfiKind, ty: &Ty) {
        let mut t = ty;
        while let Ty::Forall(_, inner) = t { t = inner; }
        let (arg_tys, res) = t.peel_arrows();

        // The threaded-state fold pattern (see `validate_ffi_callbacks`): a
        // polymorphic outgoing callback threads an OPAQUE state variable that
        // round-trips through Lua untouched. `validate_ffi_callbacks` already
        // enforces the soundness of that variable (one shared variable across
        // the callback's accumulator/result and the FFI's initial-state
        // argument/return), so here we whitelist those variables and let every
        // OTHER type still be structurally checked. A callback whose value
        // variables are empty is concrete and contributes none.
        let mut opaque: Vec<TyVar> = Vec::new();
        for cb in arg_tys.iter().filter(|t| matches!(t, Ty::Arrow(..))) {
            for v in callback_value_vars(cb) {
                if !opaque.contains(&v) { opaque.push(v); }
            }
        }

        // The result crosses IN from Lua. For a plain Pure/IO import that is the
        // whole result (its `IO` peeled here, so a bare `IO a` result validates
        // `a`); for a Try/IOCatch import the `pcall` wrapper builds the
        // `Either String _` tags itself, so only the inner payload is decoded —
        // peel `IO`/`Either String` exactly like `ffi_catch_decode_desc`.
        let payload = match ffi_kind {
            FfiKind::Try | FfiKind::Catch | FfiKind::IOCatch => {
                let inner = match res {
                    Ty::IO(a) | Ty::LuaIO(_, a) => a.as_ref(),
                    other => other,
                };
                let (head, args) = decompose_ty_app(inner);
                match head {
                    Some("Either") if args.len() == 2 => args[1],
                    // Not the expected `Either String a` shape: validate the
                    // whole inner result rather than silently skipping it.
                    _ => inner,
                }
            }
            // A plain IO/LuaIO result: decode the yielded value.
            _ => match res {
                Ty::IO(a) | Ty::LuaIO(_, a) => a.as_ref(),
                other => other,
            },
        };
        self.validate_boundary_positions(
            BoundaryKind::FfiImport, name, &arg_tys, payload, &opaque);
    }

    /// Validate every top-level position of a boundary signature: each
    /// argument crosses in `kind.arg_dir()` (a function-typed argument is a
    /// CALLBACK — the one arrow position codegen fully marshals — validated
    /// by `validate_boundary_callback`; every other arrow is rejected inside
    /// `ffi_marshallable_allowing`), and `result_payload` — already peeled by
    /// the caller to what codegen actually decodes — crosses in
    /// `kind.result_dir()`. `opaque` is the threaded-state whitelist (always
    /// empty for exports).
    fn validate_boundary_positions(
        &mut self,
        kind: BoundaryKind,
        name: &str,
        arg_tys: &[&Ty],
        result_payload: &Ty,
        opaque: &[TyVar],
    ) {
        for (i, a) in arg_tys.iter().enumerate() {
            let pos = format!("argument {}", i + 1);
            if matches!(a, Ty::Arrow(..)) {
                self.validate_boundary_callback(kind, name, &pos, a, opaque);
            } else if let Err((culprit, cdir)) =
                self.ffi_marshallable_allowing(a, kind.arg_dir(), opaque, &mut Vec::new())
            {
                self.push_boundary_ffi_error(kind, name, &pos, a, &culprit, cdir);
            }
        }
        if let Err((culprit, cdir)) =
            self.ffi_marshallable_allowing(result_payload, kind.result_dir(), opaque, &mut Vec::new())
        {
            self.push_boundary_ffi_error(
                kind, name, "the result", result_payload, &culprit, cdir);
        }
    }

    /// Validate the ONE arrow position codegen fully marshals: a direct
    /// top-level argument that is a callback. A callback inverts its side's
    /// directions — for an export the host hands a function in and mata-ll
    /// calls it (its arguments go OUT, its result comes back IN, after
    /// unwrapping its `LuaIO`/`IO`); for an FFI import mata-ll hands a
    /// function out and the host calls it (arguments IN, result OUT). Codegen
    /// hard-codes the opaque `"false"` descriptor for a callback argument
    /// that is ITSELF a function, so that nesting is rejected here. (The
    /// separate pre-existing rule that an export callback's result be an
    /// action lives in `check_export_callbacks`.)
    fn validate_boundary_callback(&mut self, kind: BoundaryKind, name: &str, position: &str, cb_ty: &Ty, opaque: &[TyVar]) {
        let cb_arg_dir = kind.result_dir();
        let cb_result_dir = kind.arg_dir();
        let (cb_args, cb_ret) = cb_ty.peel_arrows();
        for a in &cb_args {
            let cb_pos = format!("{} (a callback argument)", position);
            if matches!(a, Ty::Arrow(..)) {
                // A callback taking a callback: codegen passes the inner
                // function opaque, so reject it.
                self.push_boundary_ffi_error(kind, name, &cb_pos, cb_ty, a, cb_arg_dir);
            } else if let Err((culprit, cdir)) =
                self.ffi_marshallable_allowing(a, cb_arg_dir, opaque, &mut Vec::new())
            {
                self.push_boundary_ffi_error(kind, name, &cb_pos, cb_ty, &culprit, cdir);
            }
        }
        // The callback's result crosses back the other way; codegen unwraps
        // its LuaIO/IO and converts the payload.
        let payload = match cb_ret {
            Ty::IO(a) | Ty::LuaIO(_, a) => a.as_ref(),
            other => other,
        };
        if let Err((culprit, cdir)) =
            self.ffi_marshallable_allowing(payload, cb_result_dir, opaque, &mut Vec::new())
        {
            let cb_pos = format!("{} (the callback result)", position);
            self.push_boundary_ffi_error(kind, name, &cb_pos, cb_ty, &culprit, cdir);
        }
    }

    /// Build the boundary diagnostic: name the binder, the position
    /// (argument N / the result), the whole position type and the offending
    /// sub-type, and the crossing direction — with a `note:` explaining WHY
    /// the culprit cannot cross.
    fn push_boundary_ffi_error(&mut self, kind: BoundaryKind, name: &str, position: &str, whole: &Ty, culprit: &Ty, dir: FfiDir) {
        // Rename internal/freshened type variables (e.g. `a890`, `_r7`) to
        // friendly letters, sharing one map so `whole` and `culprit` agree.
        let (whole, culprit) = {
            let pair = friendly_export_tys(&[whole, culprit]);
            (pair[0].clone(), pair[1].clone())
        };
        let nested = if whole == culprit {
            String::new()
        } else {
            format!(" (inside '{}')", whole)
        };
        self.push_error_ctx(
            DiagnosticKind::Other(format!(
                "{} '{}': {} has type '{}', which cannot {} — the type '{}'{} has no FFI marshalling.",
                kind.subject(), name, position, whole, kind.dir_phrase(dir), culprit, nested
            )),
            format!("{} '{}'", kind.context(), name),
        );
        let note = export_ffi_note(&culprit, dir);
        if let Some(diag) = self.errors.last_mut() {
            diag.notes.push(note);
        }
    }

    /// Whether `ty` can cross the FFI boundary in direction `dir`; a bare type
    /// variable in `opaque` is accepted rather than rejected. Returns the
    /// first offending sub-type (and the direction it was reached in) so the
    /// diagnostic can name the exact culprit. The `opaque` whitelist is
    /// the ONE designed exception: the threaded STATE of a polymorphic
    /// outgoing-callback FFI (the fold pattern) crosses Lua opaquely, and
    /// `validate_ffi_callbacks` already enforces that it is a single shared
    /// variable threaded soundly through the callback's accumulator, the
    /// callback's result, an FFI argument, and the FFI return. Exports and
    /// non-stateful imports pass an empty `opaque`.
    ///
    /// A type may cross ONLY if it has DEFINED marshalling behavior — a shape
    /// the host is meant to see, not an internal representation that happens to
    /// be a table. The allowed set:
    ///   - scalars (Int/Number/Double/Float/String/Char/ByteString/Bool),
    ///     `()`, and the opaque `LuaUserData` interop handle;
    ///   - `[a]` iff `a` allowed; tuples iff every element allowed;
    ///   - `HashMap k v` iff `k` is a scalar Lua key and `v` is allowed;
    ///   - `Maybe a` iff `a` allowed (designed: nil ↔ Nothing);
    ///   - `Any` — the dynamic boundary type, allowed by name (its runtime
    ///     conversion is defined by codegen);
    ///   - a `LuaDict` record iff every declared field is allowed (designed: a
    ///     name-keyed table);
    ///   - a NEWTYPE iff its single underlying field is allowed — a newtype is
    ///     transparent (codegen represents the value AS the field, no wrapper),
    ///     so it inherits the field's representation (this is what keeps
    ///     `newtype FileHandle = FileHandle LuaUserData` and the whole `LIO`
    ///     file API crossing);
    ///   - `IO a` / `LuaIO _ a` in EXPORT (result) position iff `a` is allowed.
    ///
    /// A recursive newtype/record re-entry is cycle-guarded and passes as opaque.
    ///
    /// Everything else is REJECTED. In particular a plain user `data` type with
    /// real constructors — a multi-constructor and/or multi-field ADT — is
    /// refused EVEN WHEN its fields would each marshal, because it would cross
    /// only as its internal `{tag, fields…}` table, which has no host-facing
    /// meaning. This is where prelude ADTs `Either`/`Ordering`/`ExitValue` land
    /// when used outside their one designed context (the `Either String a` of a
    /// `LuaTry`/`LuaIOCatch` result is peeled and validated by its caller, not
    /// here). Also rejected: a bare type variable (no runtime rep for a
    /// polymorphic value), a region-scoped `ST`/`STArray`/`STRef` handle, `IO`/
    /// `LuaIO` in IMPORT (argument) position, and any unknown constructor.
    ///
    /// A FUNCTION type is rejected here in EVERY position. Codegen only fully
    /// marshals a callback when it is a DIRECT top-level boundary argument (the
    /// branch emitting `__mll_wrap_callback_in`); a function nested inside a
    /// container/result — or a callback's own function-typed argument — is
    /// passed opaque (`"false"` descriptor) and would leak. So the only accepted
    /// arrow position is handled separately by `validate_boundary_callback`,
    /// and this recursive check treats any arrow it reaches as unmarshallable.
    fn ffi_marshallable_allowing(&self, ty: &Ty, dir: FfiDir, opaque: &[TyVar], visited: &mut Vec<String>)
        -> Result<(), (Ty, FfiDir)>
    {
        match ty {
            Ty::Forall(_, inner) => self.ffi_marshallable_allowing(inner, dir, opaque, visited),
            Ty::Unit => Ok(()),
            Ty::Con(n) if ffi_scalar_name(n) || n == "LuaUserData" => Ok(()),
            // A whitelisted opaque state variable (see the doc above) round-trips.
            Ty::Var(v) if opaque.contains(v) => Ok(()),
            Ty::Var(_) | Ty::Skolem(..) | Ty::Promoted(_) => Err((ty.clone(), dir)),
            Ty::List(a) => self.ffi_marshallable_allowing(a, dir, opaque, visited),
            Ty::Tuple(es) => {
                for e in es { self.ffi_marshallable_allowing(e, dir, opaque, visited)?; }
                Ok(())
            }
            // An action can be a RESULT (the export performs it and marshals the
            // yielded value) but never an ARGUMENT — Lua has nothing to hand in.
            Ty::IO(a) | Ty::LuaIO(_, a) => match dir {
                FfiDir::Export => self.ffi_marshallable_allowing(a, FfiDir::Export, opaque, visited),
                FfiDir::Import => Err((ty.clone(), dir)),
            },
            // A function reached in any recursive position (nested in a
            // container, in result position, or as a callback's own argument)
            // is NOT marshalled by codegen — reject it. The one accepted arrow,
            // a top-level boundary-argument callback, never reaches here.
            Ty::Arrow(..) => Err((ty.clone(), dir)),
            Ty::App(..) | Ty::Con(_) => {
                let (head, args) = decompose_ty_app(ty);
                match head {
                    Some("HashMap") if args.len() == 2 => {
                        // Keys are Lua table keys — must be a scalar; values
                        // marshal by the value type in the same direction.
                        if !matches!(args[0], Ty::Con(n) if ffi_scalar_name(n)) {
                            return Err((ty.clone(), dir));
                        }
                        self.ffi_marshallable_allowing(args[1], dir, opaque, visited)
                    }
                    // `Maybe a` has a designed shape at the boundary — `nil` for
                    // Nothing, the marshalled payload for `Just` — so it crosses
                    // iff its type argument crosses in the same direction.
                    Some("Maybe") if args.len() == 1 => {
                        self.ffi_marshallable_allowing(args[0], dir, opaque, visited)
                    }
                    // `Any` is the dynamic FFI boundary type: it has a defined
                    // runtime conversion (supplied by codegen), so it crosses in
                    // both directions regardless of what value it wraps.
                    Some("Any") => Ok(()),
                    Some(name) => {
                        // A recursive type re-entered (a newtype/record whose
                        // field mentions itself): the marshaller treats the
                        // re-entry as opaque and it round-trips, so accept and stop.
                        if visited.iter().any(|v| v == name) {
                            return Ok(());
                        }
                        // Only types with a DEFINED marshalling shape may cross.
                        // Dispatch on the designed representation, not on "does
                        // every field happen to marshal":
                        //   - a newtype is transparent (codegen represents the
                        //     value AS its single field with no wrapper), so it
                        //     crosses iff that field crosses;
                        //   - a LuaDict record crosses as a name-keyed table, so
                        //     it crosses iff every declared field crosses;
                        //   - any other user `data` type — a plain ADT with real
                        //     constructors — would cross only as its internal
                        //     `{tag, fields…}` table, an implementation detail
                        //     with no host-facing meaning, so it is REJECTED even
                        //     when its fields would each marshal. (This is where
                        //     `Either`/`Ordering`/`ExitValue` land outside their
                        //     designed contexts — a plain ADT is a plain ADT.)
                        // No constructors ⇒ an abstract/handle constructor (ST,
                        // STArray, STRef, an unknown type, …) with no marshalling
                        // ⇒ reject.
                        let is_newtype = self.newtype_types.contains(name);
                        let is_luadict = self.luadict_types.contains(name);
                        if !is_newtype && !is_luadict {
                            return Err((ty.clone(), dir));
                        }
                        let cons: Vec<ConInfo> = self.constructors.values()
                            .filter(|c| c.type_name == name)
                            .cloned()
                            .collect();
                        if cons.is_empty() {
                            return Err((ty.clone(), dir));
                        }
                        visited.push(name.to_string());
                        for con in &cons {
                            let mut smap: HashMap<TyVar, Ty> = HashMap::new();
                            for (tv, a) in con.type_vars.iter().zip(args.iter()) {
                                smap.insert(tv.clone(), (*a).clone());
                            }
                            let sub = Subst::from_map(smap);
                            for fty in &con.field_types {
                                let fty = fty.apply_subst(&sub);
                                if let Err(e) = self.ffi_marshallable_allowing(&fty, dir, opaque, visited) {
                                    visited.pop();
                                    return Err(e);
                                }
                            }
                        }
                        visited.pop();
                        Ok(())
                    }
                    None => Err((ty.clone(), dir)),
                }
            }
        }
    }
}
