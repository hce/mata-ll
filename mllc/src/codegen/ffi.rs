//! FFI boundary marshalling: calls into host Lua and type-directed decoding
//! of what comes back.
//!
//! Every host value crossing IN as a result goes through ONE decision,
//! `CodeGen::ffi_result`: how many of a call's return values the declared
//! result consumes (a top-level tuple is Lua's multiple return values,
//! packed by arity; anything else is one value) and how that one value is
//! decoded (by descriptor: the `ffi_decode_desc_inner` family builds a Lua
//! descriptor expression consumed at runtime by `__mll_ffi_decode`, `None`
//! when the raw host value already matches the mata-ll representation, so
//! no wrapper is emitted). Every call form asks there — `LuaPure`, `LuaIO`,
//! their `:method` forms, `LuaTry`, `LuaCatch`, `LuaIOCatch`, a
//! `LuaIterator` step, a constant read, a host callback handed to an
//! exported function — and renders the answer one of two ways: inline,
//! where the compiled code holds the host value itself (`ffi_result_ast`),
//! or as the `n, desc, root` arguments of the runtime helper that makes the
//! call (`ffi_result_args`, read by `__mll_ffi_ret`). Descriptor text is
//! carried as `lua::Expr::Raw` leaves (the descriptor mini-language stays
//! textual); the call shapes around it are built as AST. Arguments are
//! forced at the boundary; `Maybe`-declared arguments become optional Lua
//! parameters (`__mll_opt` / `__mll_opt_tail`). LuaCatch / LuaIOCatch calls
//! go through `pcall` wrappers (`pcall_call_ast`) that build the `Either`
//! tags.

use crate::tir::*;
use crate::types::Ty;
use super::CodeGen;
use super::lua::{Expr, FuncBody, Item, Stmt};
use super::names::{lua_quoted_string};
use super::util::{con_name, decompose_app, subst_tyvars};

/// The container classification BOTH boundary directions share.
///
/// PARITY INVARIANT: the argument marshaller (`ffi_arg_marshal_desc`) and the
/// result decoder (`ffi_decode_desc_inner`) must descend into exactly the
/// same container types, so encode-then-decode is identity at every nesting
/// depth. Both obtain their classification here and match on it exhaustively
/// (no `_` arm), so the parity holds by construction: a new container variant
/// fails to compile until BOTH directions handle it. Only the descriptor
/// formats — `t`/`w` diagnostics and `rb` rebuild flags exist on the decode
/// side alone — stay direction-specific.
pub(super) enum FfiShape {
    List(Ty),
    Tuple(Vec<Ty>),
    Maybe(Ty),
    /// A `LuaDict` record, its declared fields already instantiated at the
    /// use's type arguments.
    Record { name: String, fields: Vec<(String, Ty)> },
    HashMap { key: Ty, value: Ty },
    Any,
    /// Everything without a designed container shape: scalars, type
    /// variables, `LuaUserData`, functions, plain ADTs. The value crosses
    /// as-is (possibly under a scalar check on the decode side).
    Opaque,
}

/// How a host result becomes one mata-ll value — the answer of
/// `CodeGen::ffi_result`, the one place the question is decided.
pub(super) struct FfiResult {
    /// `Some(n)`: the declared result is a top-level n-tuple, received as
    /// the call's first n return values packed into one tuple table.
    /// `None`: one value — a call's first, a read's only one.
    tuple: Option<usize>,
    /// The decode descriptor of that one value (the packed tuple
    /// included); `None` when the host's value already is the mata-ll
    /// representation.
    desc: Option<String>,
    /// The one value is a bare `Number` (`desc` is then its `num` leaf):
    /// where the compiled code holds the value itself the normalization is
    /// an inline multiply instead of a decoder call.
    number: bool,
    /// The location phrase a decode error names ("in the result of …").
    root: String,
}

impl FfiResult {
    /// The decision as Lua argument text for a runtime helper that makes
    /// the host call itself: `n` (`false` = one value), `desc` (`false` =
    /// no decoding), `root` — read by `__mll_ffi_ret`.
    pub(super) fn runtime_args(&self) -> [String; 3] {
        [
            self.tuple.map_or_else(|| "false".to_string(), |n| n.to_string()),
            self.desc.clone().unwrap_or_else(|| "false".to_string()),
            self.root_literal(),
        ]
    }

    /// `root` as a Lua string literal.
    fn root_literal(&self) -> String {
        format!("{:?}", self.root)
    }
}

/// A host value as the compiled code holds it, for the inline rendering
/// of a result: a CALL may return any number of values (so a single-value
/// result truncates it and a tuple result counts its values), a READ of a
/// host name has exactly one.
pub(super) enum HostValue {
    Call(Expr),
    Read(Expr),
}

impl HostValue {
    /// The value in a position that may take several values (an operand,
    /// a `return`): a call is truncated to its first value by the paren.
    fn single(self) -> Expr {
        match self {
            HostValue::Call(call) => Expr::paren(call),
            HostValue::Read(name) => name,
        }
    }

    /// The value as a non-final argument of a call, a position Lua itself
    /// truncates to one value.
    fn as_argument(self) -> Expr {
        match self {
            HostValue::Call(e) | HostValue::Read(e) => e,
        }
    }
}

impl CodeGen {
    /// Classify `ty` for the FFI boundary (see [`FfiShape`]).
    pub(super) fn ffi_shape(&self, ty: &Ty) -> FfiShape {
        match ty {
            Ty::List(inner) => FfiShape::List((**inner).clone()),
            Ty::Tuple(elems) => FfiShape::Tuple(elems.clone()),
            _ => {
                let (head, args) = decompose_app(ty);
                match head {
                    Some("Maybe") if args.len() == 1 => FfiShape::Maybe(args[0].clone()),
                    Some("HashMap") if args.len() == 2 => FfiShape::HashMap {
                        key: args[0].clone(),
                        value: args[1].clone(),
                    },
                    Some("Any") if args.is_empty() => FfiShape::Any,
                    Some(name) if self.luadict_type_fields.contains_key(name) => {
                        let (tvars, fields) = self.luadict_type_fields.get(name).unwrap();
                        let mut smap = std::collections::HashMap::new();
                        for (tv, a) in tvars.iter().zip(args.iter()) {
                            smap.insert(tv.clone(), (*a).clone());
                        }
                        FfiShape::Record {
                            name: name.to_string(),
                            fields: fields.iter()
                                .map(|(n, t)| (n.clone(), subst_tyvars(t, &smap)))
                                .collect(),
                        }
                    }
                    _ => FfiShape::Opaque,
                }
            }
        }
    }
    /// THE nil convention of the FFI boundary, as a predicate on a declared
    /// type: whether a value of `ty` can be Lua's `nil` on the HOST side.
    ///
    /// Inside mata-ll `nil` represents three different values — the empty
    /// list, `Nothing` and `()` — which only the type tells apart, so the
    /// boundary decides by type, in both directions, and this is the one
    /// statement of the answer:
    ///
    ///   * `Maybe a` — `Nothing` is `nil` (and so is a `Just` whose payload
    ///     is itself `nil`-formed: `Just Nothing`, `Just ()`);
    ///   * `Any` — `AnyNull` is `nil`;
    ///   * `()` — the unit value is `nil`;
    ///   * every other type is NEVER `nil`: a list is a Lua array (the empty
    ///     list the empty array `{}`), a `HashMap` a keyed table (empty:
    ///     `{}`), a tuple a positional table, a `LuaDict` record a keyed
    ///     table, a scalar itself. A newtype is `Opaque` to `ffi_shape` and
    ///     so answers `false` whatever it wraps.
    ///
    /// The runtime's two halves follow it: the marshaller
    /// (`__mll_arg_marshal`) hands every value, a `nil`-represented one
    /// included, to the descriptor of its position's type, so only these
    /// types' arms produce a `nil`; the decoder (`__mll_ffi_decode`) reads a
    /// host `nil` as `Nothing`/`AnyNull`/`()` at these types and refuses it
    /// at a tuple, a record, a map or a checked scalar (at a list it reads
    /// the empty list: a host may leave an empty array out). What the
    /// predicate itself decides in generated code is where an array can
    /// have HOLES: the marshaller writes every array by explicit index, so
    /// a `nil` element keeps its place, and the decode descriptor of a list
    /// whose element type answers `true` carries `h=true`, so the decoder
    /// takes that array's length from its largest index instead of `#`,
    /// which is undefined past a hole.
    ///
    /// The match is exhaustive over [`FfiShape`] for the reason that enum
    /// gives: a new container variant must be placed here too.
    pub(super) fn ffi_nil_form(&self, ty: &Ty) -> bool {
        match self.ffi_shape(ty) {
            FfiShape::Maybe(_) | FfiShape::Any => true,
            FfiShape::List(_)
            | FfiShape::Tuple(_)
            | FfiShape::Record { .. }
            | FfiShape::HashMap { .. } => false,
            FfiShape::Opaque => matches!(ty, Ty::Unit),
        }
    }

    /// THE result-shape decision of the FFI boundary: how the host's answer
    /// becomes the one mata-ll value its declared type names. Every form
    /// that receives a host result asks here and nowhere else, so a result
    /// type means the same thing under all of them.
    ///
    /// `payload` is the type of what ONE host call (or read) hands back,
    /// its form's wrappers already peeled (`ffi_payload`,
    /// `ffi_guarded_payload`, a list's element for an iterator step).
    /// `tuple` is the DECLARED result count (`HostFn::tuple`): `Some(n)`
    /// packs the call's first n return values into one tuple table — a
    /// top-level tuple is Lua's multiple return values — and `None` takes
    /// one value. `root` is the location phrase a decode error names.
    ///
    /// The value (the packed tuple, or the one value) is then decoded by
    /// its descriptor: a list becomes a cons list, a `Maybe` its tagged
    /// form, a record/tuple/map is rebuilt or validated, a `Number` is
    /// normalized to a double; a scalar or opaque value has no descriptor
    /// and passes as it is.
    pub(super) fn ffi_result(&self, payload: &Ty, tuple: Option<usize>, root: String) -> FfiResult {
        FfiResult {
            tuple,
            desc: self.ffi_decode_desc_inner(payload, &mut Vec::new(), None).map(|d| d.0),
            number: tuple.is_none() && con_name(payload) == Some("Number"),
            root,
        }
    }

    /// The type of the value an FFI result or constant of type `ty`
    /// delivers: the action type `IO a` / `LuaIO s a` peeled to the `a`
    /// that crosses the boundary.
    pub(super) fn ffi_payload(ty: &Ty) -> &Ty {
        match ty {
            Ty::IO(a) | Ty::LuaIO(_, a) => a.as_ref(),
            other => other,
        }
    }

    /// The success payload of a LuaTry/LuaCatch/LuaIOCatch result: the
    /// declared result is `Either String a` (under `IO` for the effectful
    /// forms) and the wrapper builds the `Left`/`Right` tags itself, so
    /// only the inner `a` crosses the boundary. The typechecker's
    /// `ffi_call_payload` peels the declared type the same way.
    pub(super) fn ffi_guarded_payload(ty: &Ty) -> &Ty {
        let inner = Self::ffi_payload(ty);
        let (head, args) = decompose_app(inner);
        match head {
            Some("Either") if args.len() == 2 => args[1],
            _ => inner,
        }
    }

    /// A host result where the compiled code itself holds the host value,
    /// as `(statements to run first, the decoded value)`:
    ///
    ///   * a top-level tuple binds the call's first n return values to n
    ///     locals and packs them (a missing value is a nil slot the
    ///     decoder reports; extra values are dropped), then decodes the
    ///     tuple like any other;
    ///   * a bare `Number` is `(value) * 1.0` — one float multiply instead
    ///     of a decoder call per scalar result (`sqrt`). A Number is an
    ///     IEEE double on every host and a host returns numbers in
    ///     whatever subtype it has (`tonumber "5"` and `string.len` are
    ///     integers on Lua 5.3+); `* 1.0` keeps a -0.0's sign where
    ///     `+ 0.0` would not. A value Lua's arithmetic does not take
    ///     raises Lua's own error at the call;
    ///   * a value with a descriptor is `__mll_ffi_decode(desc, value,
    ///     root)`;
    ///   * every other value — a scalar, an opaque value — is the value
    ///     itself, with no call around it: scalar FFI (bit ops) is the hot
    ///     path, and a check would tax every call (inside structures
    ///     scalars ARE checked, see `ffi_child_desc`).
    ///
    /// A call that yields ONE value is truncated to it (`(call)`, or its
    /// position as a middle argument of the decoder), so extra return
    /// values cannot spread into whatever position the result lands in —
    /// which is also what makes compiled-function references provably
    /// single-return for the paren-normalization pass (opt.rs).
    pub(super) fn ffi_result_inline(res: &FfiResult, value: HostValue) -> (Vec<Stmt>, Expr) {
        let decode = |desc: &str, v: Expr| Expr::call_named(
            "__mll_ffi_decode",
            vec![Expr::raw(desc), v, Expr::lit(res.root_literal())],
        );
        match (res.tuple, value) {
            (Some(arity), HostValue::Call(call)) => {
                let vars: Vec<String> = (0..arity).map(|i| format!("_r{}", i)).collect();
                let tuple = Expr::Table(
                    vars.iter().map(|v| Item::Pos(Expr::name(v.clone()))).collect(),
                );
                let value = match &res.desc {
                    Some(desc) => decode(desc, tuple),
                    None => tuple,
                };
                (vec![Stmt::Local(vars, Some(call))], value)
            }
            (Some(_), HostValue::Read(_)) => unreachable!(
                "a constant read has one value; only a call's result is counted"),
            (None, value) => {
                let value = if res.number {
                    Expr::paren(Expr::binop("*", value.single(), Expr::lit("1.0")))
                } else if let Some(desc) = &res.desc {
                    decode(desc, value.as_argument())
                } else {
                    value.single()
                };
                (Vec::new(), value)
            }
        }
    }

    /// `ffi_result_inline` as one expression: the value itself, or — when
    /// statements must run first (the tuple's locals) — a function called
    /// on the spot.
    pub(super) fn ffi_result_ast(res: &FfiResult, value: HostValue) -> Expr {
        let (mut stmts, value) = Self::ffi_result_inline(res, value);
        if stmts.is_empty() {
            return value;
        }
        stmts.push(Stmt::Return(value));
        Expr::call(Expr::paren(Expr::Func(vec![], FuncBody::Inline(stmts))), vec![])
    }

    /// `ffi_result_inline` as an action closure: the function that makes
    /// the host call and returns its decoded result each time it is run.
    pub(super) fn ffi_result_action_ast(res: &FfiResult, value: HostValue) -> Expr {
        let (mut stmts, value) = Self::ffi_result_inline(res, value);
        stmts.push(Stmt::Return(value));
        Expr::Func(vec![], FuncBody::Inline(stmts))
    }

    /// A host result where a RUNTIME helper makes the call (the `pcall` of
    /// LuaCatch/LuaIOCatch, LuaTry's `(value, err)` test, an iterator
    /// step, a host callback): the decision as the helper's leading
    /// `n, desc, root` arguments, which it hands to `__mll_ffi_ret` with
    /// the call's results — the same pack-by-arity and the same decoder
    /// the inline rendering emits. `false` stands for "one value" and for
    /// "no descriptor". A bare `Number` has its `num` descriptor here: the
    /// helper is a call already, and the decoder's check names the
    /// declared type where the inline multiply raises Lua's own error.
    ///
    /// The count, an absent descriptor and the location phrase are literal
    /// tokens and are built as such; only a descriptor — a table
    /// constructor in the textual descriptor language — is a `Raw` leaf. A
    /// suspended call with no descriptor therefore stays free of `Raw`
    /// fragments, which the thunk lifter requires of a body it lifts.
    pub(super) fn ffi_result_args(res: &FfiResult) -> Vec<Expr> {
        let [n, desc, root] = res.runtime_args();
        let desc = match res.desc {
            Some(_) => Expr::raw(desc),
            None => Expr::lit(desc),
        };
        vec![Expr::lit(n), desc, Expr::lit(root)]
    }

    /// The host call itself: `host(args…)`, or `receiver:method(args…)`
    /// for a `:method` name (the receiver is the first argument, forced —
    /// there is no call without one).
    pub(super) fn ffi_call_ast(&mut self, host: &str, args: &[TExpr]) -> Expr {
        if let Some(method) = host.strip_prefix(':') {
            let recv = self.forced_prefix_ast(&args[0]);
            let margs = self.ffi_args_ast(&args[1..]);
            Expr::method(recv, method, margs)
        } else {
            Expr::call_named(host, self.ffi_args_ast(args))
        }
    }

    /// A LuaPure/LuaIO host call with its result decoded, as the inline
    /// decision over the call (`ffi_result`): what the pure form evaluates
    /// to and what the IO form performs.
    pub(super) fn ffi_host_call(&mut self, host: &HostFn, args: &[TExpr], ty: &Ty) -> (FfiResult, HostValue) {
        let res = self.ffi_result(
            Self::ffi_payload(ty), host.tuple, Self::ffi_root_name(&host.name));
        let call = self.ffi_call_ast(&host.name, args);
        (res, HostValue::Call(call))
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
        if let FfiShape::Maybe(payload) = self.ffi_shape(&value.ty)
            && let Some(pdesc) = self.ffi_arg_marshal_desc(&payload, &mut Vec::new())
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
    /// Descends into exactly the container set [`FfiShape`] defines — the
    /// same set the result decoder descends into (the parity invariant lives
    /// on that enum). Anything `Opaque` — a type variable, `LuaUserData`, a
    /// function, a plain (non-`LuaDict`) ADT, or a bare scalar — returns
    /// `None` (a shallow `__force` at the boundary), so an opaque round-trip
    /// value (a fold's threaded state, a polymorphic argument) passes
    /// through untouched and is never mangled.
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
        match self.ffi_shape(ty) {
            // A cons list is never host-readable raw: rebuild it into an array.
            FfiShape::List(inner) => {
                let e = child(self, &inner, stack);
                Some(format!("{{k=\"list\",e={}}}", e))
            }
            // A tuple shares Lua's positional layout; force its lazy fields (and
            // convert any nested list/record/tuple/map/Maybe field) into a fresh
            // positional table.
            FfiShape::Tuple(elems) => {
                let es: Vec<String> = elems.iter().map(|e| child(self, e, stack)).collect();
                Some(format!("{{k=\"tuple\",n={},es={{{}}}}}", elems.len(), es.join(",")))
            }
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
            FfiShape::Maybe(payload) => {
                let e = child(self, &payload, stack);
                Some(format!("{{k=\"maybe\",e={}}}", e))
            }
            // A LuaDict record is a name-keyed table; force its lazy
            // fields (the host reads `rec.field`) and convert nested
            // structure into a fresh table. Cycle guard: a recursive
            // record (e.g. a tree) would otherwise expand forever — treat
            // the re-entry as opaque (shallow force).
            FfiShape::Record { name, fields } => {
                if stack.iter().any(|s| s == &name) {
                    return None;
                }
                stack.push(name);
                let fs: Vec<String> = fields.iter().map(|(fname, fty)| {
                    let d = child(self, fty, stack);
                    format!("{{n={},d={}}}", lua_quoted_string(fname.as_bytes()), d)
                }).collect();
                stack.pop();
                Some(format!("{{k=\"record\",fs={{{}}}}}", fs.join(",")))
            }
            // A HashMap is a keyed Lua table the host reads as a dict.
            // Keys are scalars already usable as Lua keys — like the
            // result decoder (and __mll_to_lua), we never convert keys,
            // only marshal each VALUE by the value type. Always Some, the
            // dual of the decoder, which always descends a HashMap: a
            // `HashMap String [Int]` must reach the host as a dict of
            // real arrays — an entry whose list is empty as a key holding
            // `{}`, like a list anywhere else (`ffi_nil_form`) —
            // `HashMap String (Maybe X)` / `HashMap String Record` /
            // nested maps marshal recursively.
            FfiShape::HashMap { value, .. } => {
                let vdesc = child(self, &value, stack);
                Some(format!("{{k=\"hashmap\",v={}}}", vdesc))
            }
            // Any is UNTAGGED for the host: the dynamic ADT's payload —
            // the scalar at field [2] of `{tag, payload}` — is handed over
            // bare (AnyNull is `{5}`, so its absent payload is nil). Always
            // Some, the dual of the result decoder's `any` arm; marshalling
            // an Any cannot fail, so no `t`/`w`.
            FfiShape::Any => Some("{k=\"any\"}".into()),
            FfiShape::Opaque => None,
        }
    }

    /// Build `__mll_pcall(n, desc, root, fn, forced-args...)` for LuaCatch/
    /// LuaIOCatch — `n, desc, root` are the result decision for the success
    /// payload (`ffi_result_args`; `root` is the human-readable name of the
    /// host function, so a decode error on the *successful* result can say
    /// whose result it was). The forced arguments are evaluated *outside*
    /// the protected call, so only errors raised by the Lua function itself
    /// become `Left` — not errors from forcing our own thunks. A leading
    /// `:` in the host name is a method call on arg0; the receiver is
    /// bound once to avoid re-evaluating it.
    pub(super) fn pcall_call_ast(&mut self, host: &HostFn, ty: &Ty, args: &[TExpr]) -> Expr {
        let res = self.ffi_result(
            Self::ffi_guarded_payload(ty), host.tuple, Self::ffi_root_name(&host.name));
        let mut pargs = Self::ffi_result_args(&res);
        if let Some(method) = host.name.strip_prefix(':') {
            let recv = self.forced_ast(&args[0]);
            pargs.push(Expr::name(format!("__recv.{}", method)));
            pargs.push(Expr::name("__recv"));
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
            pargs.push(Expr::name(host.name.as_str()));
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
            Some("Int") | Some("Number") => Some("number"),
            Some("String") | Some("ByteString") => Some("string"),
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
    /// `ty` as an FFI decode error names it at runtime: with every type
    /// under the name its own module declares it by, not the canonical
    /// one import resolution gave it.
    fn shown_ty(&self, ty: &Ty) -> String {
        crate::types::spell_names(&ty.to_string(), &self.source_names)
    }

    pub(super) fn ffi_child_desc(&self, ty: &Ty, stack: &mut Vec<String>, w: &str) -> (String, bool) {
        if let Some((d, converts)) = self.ffi_decode_desc_inner(ty, stack, Some(w)) {
            (d, converts)
        } else if let Some(lt) = Self::scalar_lua_type(ty) {
            (
                format!("{{k=\"chk\",t={:?},lt=\"{}\",w={:?}}}", self.shown_ty(ty), lt, w),
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
        // A `Number` leaf: checked like a `chk` scalar AND normalized to the
        // float subtype by the runtime (`num`), since a Number is an IEEE
        // double on every host and the host may hand over Lua's integer
        // subtype. Not a conversion of the container (`converts` false): a
        // validation-only record or tuple keeps the host's table and the
        // runtime writes the normalized field back into it.
        if con_name(ty) == Some("Number") {
            return Some((format!("{{k=\"num\",t={:?}{}}}", self.shown_ty(ty), wlua), false));
        }
        match self.ffi_shape(ty) {
            // A list always needs converting: the host hands us a Lua array
            // (1-based, possibly empty) which must become a cons list. An empty
            // array MUST decode to the empty list (nil), never a bogus element.
            // `h=true` marks an element type whose host form can be nil
            // (`ffi_nil_form`): such an array may have holes, and the
            // decoder then takes its length from the largest index.
            FfiShape::List(inner) => {
                let t = self.shown_ty(ty);
                let (e, _) = self.ffi_child_desc(
                    &inner,
                    stack,
                    &format!("an element of the list declared {}", t),
                );
                let holes = if self.ffi_nil_form(&inner) { ",h=true" } else { "" };
                Some((format!("{{k=\"list\",t={:?},e={}{}{}}}", t, e, holes, wlua), true))
            }
            // Tuples share mata-ll's positional-array layout with Lua, so only
            // rebuild when some element itself needs conversion; when elements
            // only need scalar checks the descriptor is validation-only
            // (rb=false) and the host array passes through unchanged.
            FfiShape::Tuple(elems) => {
                let t = self.shown_ty(ty);
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
            // Maybe: a host value crossing in must be wrapped as `Just`
            // (nil stays Nothing), since `Just` is now a tagged wrapper
            // rather than the identity. Always emit the descriptor so the
            // wrapping happens; `e` decodes/checks the payload (false =
            // pass the payload through).
            FfiShape::Maybe(payload) => {
                let t = self.shown_ty(ty);
                let (e, _) = self.ffi_child_desc(
                    &payload,
                    stack,
                    &format!("the payload of the declared {}", t),
                );
                Some((format!("{{k=\"maybe\",t={:?},e={}{}}}", t, e, wlua), true))
            }
            // A LuaDict record: recurse into the declared fields (already
            // instantiated at the use's type arguments by `ffi_shape`).
            FfiShape::Record { name, fields } => {
                // Cycle guard: a recursive record type (e.g. a tree) would
                // otherwise expand forever. Treat the re-entry as opaque.
                if stack.iter().any(|s| s == &name) {
                    return None;
                }
                let t = self.shown_ty(ty);
                stack.push(name.clone());
                let mut converts = false;
                let mut any = false;
                let mut fs = Vec::new();
                for (fname, fty) in &fields {
                    let (d, c) = self.ffi_child_desc(
                        fty,
                        stack,
                        &format!(
                            "field '{}' of record {}",
                            fname,
                            self.source_names.get(&name).unwrap_or(&name),
                        ),
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
            // HashMap always decodes: it validates each key's Lua type
            // against the declared key type (catching a host array where
            // a String-keyed map was declared) and decodes each value.
            FfiShape::HashMap { key, value } => {
                let t = self.shown_ty(ty);
                let kt = con_name(&key).unwrap_or("");
                let (v, _) = self.ffi_child_desc(
                    &value,
                    stack,
                    &format!("a value of the map declared {}", t),
                );
                Some((
                    format!("{{k=\"hashmap\",t={:?},kt={:?},v={}{}}}", t, kt, v, wlua),
                    true,
                ))
            }
            // Any: a host scalar crossing in is TAGGED into the dynamic
            // ADT — a Lua string becomes `AnyString`, an integer-valued
            // number `AnyInt`, a non-integer number `AnyNumber`, a
            // boolean `AnyBool`, and nil `AnyNull`. Always emit the
            // descriptor so the tagging happens; a value that is neither a
            // scalar nor nil (a table/function/userdata) fails at runtime,
            // localized by `w`, since `Any` models only scalar Lua values.
            FfiShape::Any => {
                let t = self.shown_ty(ty);
                Some((format!("{{k=\"any\",t={:?}{}}}", t, wlua), true))
            }
            // Scalars, opaque type variables, functions, IO, etc.: the raw
            // host value already matches the mata-ll representation. Bare
            // scalar results are deliberately NOT wrapped in a `chk` —
            // scalar FFI (e.g. bit ops) is the hot path, and the check
            // would tax every call; inside structures scalars ARE checked
            // (see ffi_child_desc).
            FfiShape::Opaque => None,
        }
    }
}
