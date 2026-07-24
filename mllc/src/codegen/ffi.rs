//! FFI boundary marshalling: calls into host Lua and type-directed decoding
//! of what comes back.
//!
//! Results are decoded by descriptor: the `ffi_decode_desc*` family builds a
//! Lua descriptor expression consumed at runtime by `__mll_ffi_decode`, and
//! returns `None` when the raw host value already matches the mata-ll
//! representation, so no wrapper is emitted. Descriptor text is carried as
//! `lua::Expr::Raw` leaves (the descriptor mini-language stays textual);
//! the call shapes around it are built as AST. Arguments are forced at the
//! boundary; `Maybe`-declared arguments become optional Lua parameters
//! (`__mll_opt` / `__mll_opt_tail`). LuaCatch / LuaIOCatch calls go through
//! `pcall` wrappers (`pcall_call_ast`) that build the `Either` tags.

use crate::tir::*;
use crate::types::Ty;
use super::CodeGen;
use super::lua::{Expr, FuncBody, Stmt};
use super::names::{lua_quoted_string};
use super::util::{con_name, decompose_app, subst_tyvars};

impl CodeGen {
    /// Build a Lua *descriptor* expression that drives type-directed decoding of
    /// a value of type `ty` that has just crossed the Lua FFI boundary (an FFI
    /// result). The descriptor is consumed at runtime by `__mll_ffi_decode`.
    ///
    /// Returns `None` when the host's raw Lua value already matches the mata-ll
    /// representation (scalars, opaque type variables, and records/tuples/maybes
    /// built only from those), so no decode wrapper is needed. Returns `Some`
    /// when a structural conversion is required — notably any list (Lua array →
    /// cons list) or hashmap (key-type validation + value decode).
    pub(super) fn ffi_decode_desc(&self, ty: &Ty) -> Option<String> {
        // The FFI body carries the action type `IO a` / `LuaIO s a`; the value
        // that crosses the boundary is the inner `a`. Peel the effect wrapper.
        let inner = match ty {
            Ty::IO(a) | Ty::LuaIO(_, a) => a.as_ref(),
            other => other,
        };
        self.ffi_decode_desc_inner(inner, &mut Vec::new(), None).map(|d| d.0)
    }

    /// Decode descriptor for a LuaCatch/LuaIOCatch success payload. The declared
    /// result is `Either String a` (optionally under `IO` for LuaIOCatch); the
    /// `pcall` wrapper builds the `Left`/`Right` tags itself, so only the inner
    /// `a` may need FFI decoding. Peels the `IO` and `Either String _` layers,
    /// then reuses the ordinary result decoder on `a`.
    pub(super) fn ffi_catch_decode_desc(&self, ty: &Ty) -> Option<String> {
        let inner = match ty {
            Ty::IO(a) | Ty::LuaIO(_, a) => a.as_ref(),
            other => other,
        };
        let (head, args) = decompose_app(inner);
        match head {
            Some("Either") if args.len() == 2 => {
                self.ffi_decode_desc_inner(args[1], &mut Vec::new(), None).map(|d| d.0)
            }
            _ => None,
        }
    }

    /// Build an FFI call's argument list (the expressions between the parens,
    /// after any receiver the caller placed first). Plain arguments are
    /// `__force(a)`, exactly as before. Arguments declared `Maybe` in the FFI
    /// signature (FfiMaybeArg) are optional Lua parameters (SPEC "Optional
    /// parameters"):
    ///   - the maximal *trailing* run of optionals is bundled into one
    ///     `__mll_opt_tail(...)` call in final argument position — its
    ///     multiple-return expands there, unwrapping each `Just` and dropping
    ///     the trailing nils, so the callee sees `Nothing` as a genuinely
    ///     omitted argument (`math.random(3)`, never `math.random(3, nil)`,
    ///     which arg-count-sensitive hosts reject);
    ///   - an optional *before* another passed argument cannot be positionally
    ///     omitted in Lua, so it goes through `__mll_opt`: `Just x` unwraps to
    ///     `x` and `Nothing` becomes an explicit nil — Lua's own idiom for a
    ///     skipped middle optional (luaL_opt* treats nil as "use default").
    pub(super) fn ffi_args_ast(&mut self, args: &[TExpr]) -> Vec<Expr> {
        let mut out = Vec::new();
        // Start of the maximal trailing run of declared-optional arguments.
        let mut tail_start = args.len();
        while tail_start > 0 && matches!(args[tail_start - 1].kind, TExprKind::FfiMaybeArg { .. }) {
            tail_start -= 1;
        }
        for a in &args[..tail_start] {
            if let TExprKind::FfiMaybeArg { value } = &a.kind {
                let v = self.ffi_boundary_value_ast(value);
                out.push(Expr::call_named("__mll_opt", vec![v]));
            } else if let Some(desc) = self.ffi_arg_marshal_desc(&a.ty, &mut Vec::new()) {
                // The argument carries structure a Lua host reads: a list must
                // be rebuilt into a plain array, and a tuple's/record's lazy
                // fields must be forced (and nested lists converted) before the
                // host sees them. An FFI call is strict in its arguments, so
                // this forces nothing the call does not already demand.
                let v = self.expr_ast(a);
                out.push(Expr::call_named("__mll_arg_marshal", vec![v, Expr::raw(desc)]));
            } else {
                // The host must see a value, never a thunk — but skip the
                // wrapper when expr_ast's own emission already yields WHNF.
                out.push(self.forced_ast(a));
            }
        }
        if tail_start < args.len() {
            let mut tail_args = Vec::new();
            for a in &args[tail_start..] {
                match &a.kind {
                    TExprKind::FfiMaybeArg { value } => {
                        tail_args.push(self.ffi_boundary_value_ast(value));
                    }
                    _ => unreachable!("non-optional argument in trailing optional run"),
                }
            }
            out.push(Expr::call_named("__mll_opt_tail", tail_args));
        }
        out
    }

    /// Build an optional FFI argument's underlying `Maybe` value. `__mll_opt` /
    /// `__mll_opt_tail` unwrap the `Just`/`Nothing` wrapper AFTER this to decide
    /// whether the argument is present, so here the wrapper is KEPT and only the
    /// payload's structure is marshalled (a list inside a `Just` must still
    /// become an array, a tuple's fields forced, etc.). This is the `just`
    /// descriptor, distinct from the structural `maybe` descriptor that unwraps
    /// (used for a `Maybe` nested in a record/list/tuple — see
    /// `ffi_arg_marshal_desc`). When the payload has no structure to marshal
    /// (a scalar or opaque payload), emit it directly — `__mll_opt` forces it.
    pub(super) fn ffi_boundary_value_ast(&mut self, value: &TExpr) -> Expr {
        let (head, args) = decompose_app(&value.ty);
        if head == Some("Maybe")
            && args.len() == 1
            && let Some(pdesc) = self.ffi_arg_marshal_desc(args[0], &mut Vec::new())
        {
            let v = self.expr_ast(value);
            Expr::call_named(
                "__mll_arg_marshal",
                vec![v, Expr::raw(format!("{{k=\"just\",e={}}}", pdesc))],
            )
        } else {
            self.expr_ast(value)
        }
    }

    /// Build a Lua descriptor locating the structure inside an FFI *argument*
    /// type that must be marshalled from the mata-ll representation into what a
    /// Lua host reads — the argument-direction dual of `ffi_decode_desc_inner`,
    /// interpreted at runtime by `__mll_arg_marshal`.
    ///
    /// PARITY INVARIANT: this must descend into exactly the same container types
    /// the result decoder (`ffi_decode_desc_inner`) descends into, so that
    /// encode-then-decode is identity at every nesting depth. The decoder
    /// converts: **list**, **tuple**, **`Maybe`**, **`HashMap`** (values; keys
    /// are validated but not converted), and **`LuaDict` record**. Each has its
    /// dual here. Anything the decoder leaves opaque — a type variable,
    /// `LuaUserData`, a function, a plain (non-`LuaDict`) ADT, or a bare scalar —
    /// this leaves opaque too (returns `None` → a shallow `__force` at the
    /// boundary), so an opaque round-trip value (a fold's threaded state, a
    /// polymorphic argument) passes through untouched and is never mangled.
    ///
    /// A host reads:
    ///   - a **list** as a plain 1-based Lua array — a cons list (lazy spine,
    ///     metatable-tagged cells, head/tail at `[1]`/`[2]`) is walked and
    ///     rebuilt into an array with each element marshalled (`{k="list",e=..}`);
    ///   - a **tuple** as a positional array (`{k="tuple",n=N,es={..}}`);
    ///   - a **`LuaDict` record** as a name-keyed table (`{k="record",fs={..}}`);
    ///   - a **`HashMap`** as a string-keyed dict — each value marshalled by the
    ///     value type, keys kept (`{k="hashmap",v=..}`);
    ///   - a structural **`Maybe`** UNWRAPPED (`{k="maybe",e=..}`): `Just x` → the
    ///     bare marshalled `x`, `Nothing` → nil (see the Maybe arm).
    ///
    /// `__mll_arg_marshal` rebuilds each converted container into a FRESH Lua
    /// value rather than mutating the mata-ll value in place, so a value passed
    /// to a host and then reused in mata-ll code is not corrupted. `stack` guards
    /// against a recursive record type expanding forever.
    pub(super) fn ffi_arg_marshal_desc(&self, ty: &Ty, stack: &mut Vec<String>) -> Option<String> {
        // A nested position marshals when it has its own descriptor; otherwise
        // it is opaque or a bare scalar and the runtime simply forces it
        // (`false`) — the host reads the value as-is.
        let child = |slf: &Self, t: &Ty, stack: &mut Vec<String>| {
            slf.ffi_arg_marshal_desc(t, stack).unwrap_or_else(|| "false".into())
        };
        match ty {
            // A cons list is never host-readable raw: rebuild it into an array.
            Ty::List(inner) => {
                let e = child(self, inner, stack);
                Some(format!("{{k=\"list\",e={}}}", e))
            }
            // A tuple shares Lua's positional layout; force its lazy fields (and
            // convert any nested list/record/tuple/map/Maybe field) into a fresh
            // positional table.
            Ty::Tuple(elems) => {
                let es: Vec<String> = elems.iter().map(|e| child(self, e, stack)).collect();
                Some(format!("{{k=\"tuple\",n={},es={{{}}}}}", elems.len(), es.join(",")))
            }
            _ => {
                let (head, args) = decompose_app(ty);
                if head == Some("Maybe") && args.len() == 1 {
                    // A Maybe reached through the structural descent — a record
                    // field, list element, or tuple field — is UNWRAPPED for the
                    // host: `Just x` becomes the bare `x` (recursively marshalled
                    // by x's type), `Nothing` becomes `nil` (an absent field).
                    // This matches __mll_to_lua and is the exact inverse of the
                    // result decoder's Maybe case (nil -> Nothing, value -> Just).
                    // Always Some, so even a `Maybe Int` field is unwrapped,
                    // not handed over as the raw `{x}` wrapper table.
                    //
                    // The TOP-LEVEL optional positional-argument path is separate:
                    // it keeps the wrapper for __mll_opt/__mll_opt_tail (which
                    // detect present/absent) and marshals only the payload — see
                    // ffi_boundary_value_ast, which emits the `just` descriptor
                    // and never routes a Maybe through here.
                    let e = self.ffi_arg_marshal_desc(args[0], stack)
                        .unwrap_or_else(|| "false".into());
                    Some(format!("{{k=\"maybe\",e={}}}", e))
                } else if let Some(name) = head.filter(|n| self.luadict_type_fields.contains_key(*n)) {
                    // A LuaDict record is a name-keyed table; force its lazy
                    // fields (the host reads `rec.field`) and convert nested
                    // structure into a fresh table. Cycle guard: a recursive
                    // record (e.g. a tree) would otherwise expand forever — treat
                    // the re-entry as opaque (shallow force).
                    if stack.iter().any(|s| s == name) {
                        return None;
                    }
                    let (tvars, fields) = self.luadict_type_fields.get(name).unwrap().clone();
                    let mut smap = std::collections::HashMap::new();
                    for (tv, a) in tvars.iter().zip(args.iter()) {
                        smap.insert(tv.clone(), (*a).clone());
                    }
                    stack.push(name.to_string());
                    let fs: Vec<String> = fields.iter().map(|(fname, fty)| {
                        let fty = subst_tyvars(fty, &smap);
                        let d = child(self, &fty, stack);
                        format!("{{n={},d={}}}", lua_quoted_string(fname.as_bytes()), d)
                    }).collect();
                    stack.pop();
                    Some(format!("{{k=\"record\",fs={{{}}}}}", fs.join(",")))
                } else if head == Some("HashMap") && args.len() == 2 {
                    // A HashMap is a string-keyed Lua table the host reads as a
                    // dict. Keys are scalars already usable as Lua keys — like the
                    // result decoder (and __mll_to_lua), we never convert keys,
                    // only marshal each VALUE by the value type. Always Some, the
                    // dual of the decoder, which always descends a HashMap: a
                    // `HashMap String [Int]` must reach the host as a dict of
                    // real arrays, `HashMap String (Maybe X)` / `HashMap String
                    // Record` / nested maps marshal recursively.
                    let vdesc = child(self, args[1], stack);
                    Some(format!("{{k=\"hashmap\",v={}}}", vdesc))
                } else {
                    None
                }
            }
        }
    }

    /// Build `__mll_pcall(desc, root, fn, forced-args...)` for LuaCatch/
    /// LuaIOCatch — `root` is the human-readable name of the host function,
    /// threaded through so a decode error on the *successful* result can say
    /// whose result it was. The forced arguments are evaluated *outside* the
    /// protected call, so only errors raised by the Lua function itself become
    /// `Left` — not errors from forcing our own thunks. A leading `:` in
    /// `lua_func` is a method call on arg0; the receiver is bound once to
    /// avoid re-evaluating it.
    pub(super) fn pcall_call_ast(&mut self, lua_func: &str, desc: &Option<String>, args: &[TExpr]) -> Expr {
        let desc_str = desc.as_deref().unwrap_or("false");
        let root = Self::ffi_root_name(lua_func);
        if let Some(method) = lua_func.strip_prefix(':') {
            let recv = self.forced_ast(&args[0]);
            let mut pargs = vec![
                Expr::raw(desc_str),
                Expr::raw(format!("{:?}", root)),
                Expr::name(format!("__recv.{}", method)),
                Expr::name("__recv"),
            ];
            pargs.extend(self.ffi_args_ast(&args[1..]));
            Expr::call(
                Expr::paren(Expr::Func(
                    vec![],
                    FuncBody::Inline(vec![
                        Stmt::Local(vec!["__recv".into()], Some(recv)),
                        Stmt::Return(Expr::call_named("__mll_pcall", pargs)),
                    ]),
                )),
                vec![],
            )
        } else {
            let mut pargs = vec![
                Expr::raw(desc_str),
                Expr::raw(format!("{:?}", root)),
                Expr::name(lua_func),
            ];
            pargs.extend(self.ffi_args_ast(args));
            Expr::call_named("__mll_pcall", pargs)
        }
    }

    /// The location phrase an FFI decode error appends — "in the result of …"
    /// with the host function's own name, or a readable phrase for a
    /// `:method` call. A full phrase, because the decoder is also used for
    /// values crossing in the other direction (exported-function arguments),
    /// where "the result of" would be wrong.
    pub(super) fn ffi_root_name(lua_func: &str) -> String {
        match lua_func.strip_prefix(':') {
            Some(method) => format!("in the result of the :{} method call", method),
            None => format!("in the result of {}", lua_func),
        }
    }

    /// The Lua runtime type a declared mata-ll scalar must arrive as, when the
    /// declared type pins one down. Used to emit `chk` leaf descriptors inside
    /// structures so a wrong-typed or missing host value fails with a clear
    /// message instead of surfacing later as an arbitrary Lua error. Opaque
    /// types (type variables, LuaUserData, functions, plain ADTs, …) return None —
    /// nothing can be checked for them, so they stay pass-through.
    pub(super) fn scalar_lua_type(ty: &Ty) -> Option<&'static str> {
        match con_name(ty) {
            Some("Int") | Some("Number") | Some("Double") | Some("Float") => {
                Some("number")
            }
            Some("String") | Some("Char") | Some("ByteString") => Some("string"),
            Some("Bool") => Some("boolean"),
            _ => None,
        }
    }

    /// Descriptor for a *nested* position (a record field, list/tuple element,
    /// hashmap value, Maybe payload). Returns the Lua descriptor text (or
    /// `"false"` for an opaque pass-through) plus whether decoding this position
    /// *converts* the value (rebuilds it) rather than merely checking it.
    /// Scalar leaves that get no structural descriptor still get a `chk`
    /// descriptor here, so a host value of the wrong Lua type — or a missing
    /// (nil) one — is reported with the declared type and its position (`w`).
    /// Bare scalar FFI *results* deliberately get no such check (see
    /// `ffi_decode_desc_inner`): they would wrap every hot scalar FFI call.
    pub(super) fn ffi_child_desc(&self, ty: &Ty, stack: &mut Vec<String>, w: &str) -> (String, bool) {
        if let Some((d, converts)) = self.ffi_decode_desc_inner(ty, stack, Some(w)) {
            (d, converts)
        } else if let Some(lt) = Self::scalar_lua_type(ty) {
            (
                format!("{{k=\"chk\",t={:?},lt=\"{}\",w={:?}}}", ty.to_string(), lt, w),
                false,
            )
        } else {
            ("false".into(), false)
        }
    }

    /// Returns `(descriptor, converts)`; `converts` says the decoded value is a
    /// *rebuilt* structure (cons list, tagged Maybe, fresh table) rather than
    /// the host's own table. Records/tuples whose fields only need *checking*
    /// carry `rb=false` and are returned as the host's table itself after
    /// validation — rebuilding would strip metatables and undeclared fields the
    /// host may rely on when the value is later passed back out.
    /// `w` is a static "where" phrase (e.g. `field 'ip' of record Cert`) baked
    /// into the descriptor for error messages; None at the top level.
    pub(super) fn ffi_decode_desc_inner(
        &self,
        ty: &Ty,
        stack: &mut Vec<String>,
        w: Option<&str>,
    ) -> Option<(String, bool)> {
        let wlua = match w {
            Some(s) => format!(",w={:?}", s),
            None => String::new(),
        };
        match ty {
            // A list always needs converting: the host hands us a Lua array
            // (1-based, possibly empty) which must become a cons list. An empty
            // array MUST decode to the empty list (nil), never a bogus element.
            Ty::List(inner) => {
                let t = ty.to_string();
                let (e, _) = self.ffi_child_desc(
                    inner,
                    stack,
                    &format!("an element of the list declared {}", t),
                );
                Some((format!("{{k=\"list\",t={:?},e={}{}}}", t, e, wlua), true))
            }
            // Tuples share mata-ll's positional-array layout with Lua, so only
            // rebuild when some element itself needs conversion; when elements
            // only need scalar checks the descriptor is validation-only
            // (rb=false) and the host array passes through unchanged.
            Ty::Tuple(elems) => {
                let t = ty.to_string();
                let mut converts = false;
                let mut any = false;
                let mut es = Vec::new();
                for (i, e) in elems.iter().enumerate() {
                    let (d, c) = self.ffi_child_desc(
                        e,
                        stack,
                        &format!("element {} of the tuple declared {}", i + 1, t),
                    );
                    converts |= c;
                    any |= d != "false";
                    es.push(d);
                }
                if !any {
                    return None;
                }
                Some((
                    format!(
                        "{{k=\"tuple\",t={:?},es={{{}}},rb={}{}}}",
                        t,
                        es.join(","),
                        converts,
                        wlua
                    ),
                    converts,
                ))
            }
            _ => {
                let (head, args) = decompose_app(ty);
                match head {
                    // Maybe: a host value crossing in must be wrapped as `Just`
                    // (nil stays Nothing), since `Just` is now a tagged wrapper
                    // rather than the identity. Always emit the descriptor so the
                    // wrapping happens; `e` decodes/checks the payload (false =
                    // pass the payload through).
                    Some("Maybe") if args.len() == 1 => {
                        let t = ty.to_string();
                        let (e, _) = self.ffi_child_desc(
                            args[0],
                            stack,
                            &format!("the payload of the declared {}", t),
                        );
                        Some((format!("{{k=\"maybe\",t={:?},e={}{}}}", t, e, wlua), true))
                    }
                    // HashMap always decodes: it validates each key's Lua type
                    // against the declared key type (catching a host array where
                    // a String-keyed map was declared) and decodes each value.
                    Some("HashMap") if args.len() == 2 => {
                        let t = ty.to_string();
                        let kt = con_name(args[0]).unwrap_or("");
                        let (v, _) = self.ffi_child_desc(
                            args[1],
                            stack,
                            &format!("a value of the map declared {}", t),
                        );
                        Some((
                            format!("{{k=\"hashmap\",t={:?},kt={:?},v={}{}}}", t, kt, v, wlua),
                            true,
                        ))
                    }
                    // A LuaDict record: recurse into declared fields, substituting
                    // the type arguments for the record's type parameters.
                    Some(name) if self.luadict_type_fields.contains_key(name) => {
                        // Cycle guard: a recursive record type (e.g. a tree) would
                        // otherwise expand forever. Treat the re-entry as opaque.
                        if stack.iter().any(|s| s == name) {
                            return None;
                        }
                        let t = ty.to_string();
                        let (tvars, fields) = self.luadict_type_fields.get(name).unwrap().clone();
                        let mut smap = std::collections::HashMap::new();
                        for (tv, a) in tvars.iter().zip(args.iter()) {
                            smap.insert(tv.clone(), (*a).clone());
                        }
                        stack.push(name.to_string());
                        let mut converts = false;
                        let mut any = false;
                        let mut fs = Vec::new();
                        for (fname, fty) in &fields {
                            let fty = subst_tyvars(fty, &smap);
                            let (d, c) = self.ffi_child_desc(
                                &fty,
                                stack,
                                &format!("field '{}' of record {}", fname, name),
                            );
                            converts |= c;
                            any |= d != "false";
                            fs.push(format!("{{n={},d={}}}", lua_quoted_string(fname.as_bytes()), d));
                        }
                        stack.pop();
                        // If every field is opaque there is nothing to convert
                        // OR check — leave the host table untouched.
                        if !any {
                            return None;
                        }
                        Some((
                            format!(
                                "{{k=\"record\",t={:?},fs={{{}}},rb={}{}}}",
                                t,
                                fs.join(","),
                                converts,
                                wlua
                            ),
                            converts,
                        ))
                    }
                    // Scalars, opaque type variables, functions, IO, etc.: the raw
                    // host value already matches the mata-ll representation. Bare
                    // scalar results are deliberately NOT wrapped in a `chk` —
                    // scalar FFI (e.g. bit ops) is the hot path, and the check
                    // would tax every call; inside structures scalars ARE checked
                    // (see ffi_child_desc).
                    _ => None,
                }
            }
        }
    }
}
