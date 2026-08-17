//! IO/ST action emission: bind chains, the pure-box convention, and the
//! two-runner protocol. `bind_chain_block` flattens do-notation into a
//! statement `Block`; the `*_ast` builders produce the run-position
//! expressions.
//!
//! Performing an action goes through a runner. `action_run_ast(tail=false)`
//! emits the CONSUMING `__mll_run` (bind RHSes and value positions — the
//! result is inspected, so any pending pure box is stripped);
//! `tail=true` emits the FORWARDING `__mll_run_tail`, whose Lua tail call
//! keeps interprocedural action recursion constant-stack and may leave at
//! most one pending box for the consuming site at the chain's root to strip.
//! A `pure e` value escaping to a caller's `__mll_run` is left bare only
//! when forcing it is a harmless no-op AND its type can never be a Lua
//! function (`pure_value_bare_is_safe`); otherwise it is boxed in
//! `__mll_pure` so the runner hands it back untouched. The `*_is_whnf`
//! predicates are claims about the emission arms, must mirror them exactly,
//! and default to conservative.

use crate::tir::*;
use crate::types::Ty;
use super::CodeGen;
use super::lua::{Block, Expr, Stmt};
use super::names::{sanitize_name};
use super::strictness::{bare_var_alias, strict_binding_safe};

impl CodeGen {
    /// Emit an ST/IO action in a flattened bind chain.
    /// Bare Var references to zero-arg IO/ST bindings are deferred functions
    /// in Lua and need () to execute. Everything else self-evaluates.
    /// Emit code that PERFORMS an IO/ST action (used inside bind chains).
    /// Inlines known action patterns to avoid closure allocation:
    /// - SpecCall __mll_io: → emit Lua call directly
    /// - SpecCall for ST primitives → emit operation directly
    /// - pure/return → emit the value
    ///
    /// Falls back to __force(expr)() for unknown actions.
    ///
    /// Whether performing `action` yields a value already in WHNF (a forced
    /// value) rather than a possibly-suspended thunk. A `<-`-bound variable is
    /// marked `concrete` (read force-free downstream) only when this holds.
    ///
    /// This must mirror action_run_ast's emission arms exactly — it is a claim
    /// about what the emitted code produces, so each `true` arm corresponds to
    /// an arm of action_run_ast whose result is provably forced:
    ///
    ///   * `return e` / `pure e` (prefix or `$`): the result is `e` left
    ///     UNFORCED per the non-strict contract — WHNF only when `e` is itself
    ///     provably total (`is_cheap_to_force`); otherwise action_run_ast suspends
    ///     it in a thunk.
    ///   * literal / constructor / tuple actions: emitted as the value itself,
    ///     which is WHNF by construction.
    ///   * FFI SpecCalls (`__mll_io:`): a raw host value (plus decode), never
    ///     a mata-ll thunk.
    ///   * fused ST intrinsics: `__mll_st_write` forces on store, so reads and
    ///     the other intrinsics return forced values.
    ///
    /// Everything else — in particular a call to a USER-DEFINED action, which
    /// goes through `__mll_run` — defaults to `false`: a user function whose
    /// body ends in `pure <expr>` compiles to an action closure whose result
    /// `__mll_run` returns UNFORCED, so the bound variable can hold a thunk.
    /// Claiming WHNF there emitted force-free reads of a thunk table (e.g.
    /// bare `v + 1` → "attempt to perform arithmetic on a table value").
    /// Being conservative here only costs an idempotent `__force` probe at the
    /// use sites; being aggressive miscompiles.
    pub(super) fn action_result_is_whnf(&self, action: &TExpr) -> bool {
        let mut a = action;
        while let TExprKind::Paren(inner) = &a.kind {
            a = inner.as_ref();
        }
        match &a.kind {
            TExprKind::App(func, arg)
                if matches!(&func.kind, TExprKind::Var(n) if n == "pure" || n == "return") =>
                self.is_cheap_to_force(arg),
            TExprKind::InfixApp { op, lhs, rhs }
                if op == "$"
                    && matches!(&lhs.kind, TExprKind::Var(n) if n == "pure" || n == "return") =>
                self.is_cheap_to_force(rhs),
            TExprKind::Lit(_) | TExprKind::Con(_) | TExprKind::Tuple(_) => true,
            TExprKind::SpecCall { specialized, .. } if specialized.starts_with("__mll_io:") => true,
            _ if Self::st_intrinsic_fused(a).is_some() => true,
            _ => false,
        }
    }

    /// Whether a `pure e` / `return e` value may be emitted as a BARE value in
    /// an escaping-action position — i.e. one that flows out of its defining
    /// function to a caller's `__mll_run`. `__mll_run` must force-and-inspect
    /// an action to tell an action *closure* (call it) from a *value* (return
    /// it); leaving a pure value bare is sound only when that forcing is a
    /// harmless no-op AND the value is never a Lua function that would be
    /// wrongly called. Both hold exactly when `e` is provably WHNF
    /// (`is_cheap_to_force`) and its type's runtime representation is never a
    /// Lua function. Otherwise the value is wrapped in `__mll_pure` so
    /// `__mll_run` hands it back untouched — this is what keeps `mk n = do …;
    /// pure ⊥` from raising at an interprocedural `v <- mk n` bind, and a
    /// returned `pure (\x -> …)` from being called with no arguments.
    pub(super) fn pure_value_bare_is_safe(&self, arg: &TExpr) -> bool {
        Self::ty_never_lua_function(&arg.ty) && self.pure_payload_force_is_total(arg)
    }

    /// Whether `__force` applied to what `arg_ast(arg, false)` emits cannot
    /// bottom — i.e. the emitted form is already WHNF. `arg_ast` suspends a
    /// possibly-⊥ payload in a thunk (so forcing it could raise) EXCEPT for two
    /// forms it emits directly, both provably total to WHNF: a `is_cheap_to_force`
    /// expression, and a TUPLE literal (building the table forces nothing — each
    /// field is independently suspended, so a ⊥ field stays inert until demanded).
    /// For those two, `__mll_run`'s force is a harmless no-op and the value is
    /// safe to leave bare; everything else must be boxed. This mirrors the
    /// arg_ast emission arms exactly.
    pub(super) fn pure_payload_force_is_total(&self, arg: &TExpr) -> bool {
        let mut a = arg;
        while let TExprKind::Paren(inner) = &a.kind {
            a = inner.as_ref();
        }
        matches!(&a.kind, TExprKind::Tuple(_)) || self.is_cheap_to_force(arg)
    }

    /// A type whose every WHNF value is a plain datum (number / string /
    /// boolean / nil / table) and NEVER a Lua function. Deliberately narrow:
    /// only known scalars, unit, lists and tuples qualify. Anything that could
    /// be an arrow, an IO / ST / LuaIO action, a newtype that may wrap a
    /// function (`App` / a non-scalar `Con`), or an unresolved variable is
    /// excluded — a value of such a type may be a Lua closure at runtime, so it
    /// must be boxed rather than handed to `__mll_run`'s function test.
    pub(super) fn ty_never_lua_function(ty: &Ty) -> bool {
        match ty {
            Ty::Unit | Ty::List(_) | Ty::Tuple(_) => true,
            Ty::Con(n) => matches!(
                n.as_str(),
                "Int" | "Word" | "Number" | "Double" | "Float"
                    | "Bool" | "Char" | "String" | "Ordering"
            ),
            _ => false,
        }
    }

    /// Build the action produced by `pure arg` / `return arg` in a position
    /// where it may ESCAPE to a caller's `__mll_run` (a function's terminal
    /// action, a discarded statement). The value is left bare when that is
    /// provably safe (see `pure_value_bare_is_safe`) and otherwise wrapped in
    /// `__mll_pure`, which `__mll_run` unwraps without forcing or calling it.
    /// `arg` is still built through the eagerness weighing (`arg_ast`,
    /// non-strict), so a possibly-⊥ payload stays suspended inside the box.
    pub(super) fn pure_action_ast(&mut self, arg: &TExpr) -> Expr {
        if self.pure_value_bare_is_safe(arg) {
            self.arg_ast(arg, false)
        } else {
            let payload = self.arg_ast(arg, false);
            Expr::call_named("__mll_pure", vec![payload])
        }
    }

    /// Build the RHS of an `x <- action` bind whose result is assigned DIRECTLY
    /// to `x` (no enclosing runner). A syntactic `pure e` / `return e`
    /// short-circuits to its payload — bound as a value/thunk and forced only
    /// on use — and must stay UNBOXED here, since nothing will unwrap a
    /// `__mll_pure` box on this path. Every other action goes through
    /// `action_run_ast`, which emits its own `__mll_run` (unwrapping any box
    /// produced deeper). This mirrors `action_result_is_whnf`'s pure arms, so
    /// the concreteness decision that follows the bind stays exact.
    pub(super) fn bound_action_ast(&mut self, action: &TExpr) -> Expr {
        let mut a = action;
        while let TExprKind::Paren(inner) = &a.kind {
            a = inner.as_ref();
        }
        let payload = match &a.kind {
            TExprKind::App(func, arg)
                if matches!(&func.kind, TExprKind::Var(n) if n == "pure" || n == "return") =>
                Some(arg.as_ref()),
            TExprKind::InfixApp { op, lhs, rhs }
                if op == "$"
                    && matches!(&lhs.kind, TExprKind::Var(n) if n == "pure" || n == "return") =>
                Some(rhs.as_ref()),
            _ => None,
        };
        match payload {
            Some(p) => self.arg_ast(p, false),
            None => self.action_run_ast(a, false),
        }
    }

    /// Build an action in run-position. `tail` selects the runner: `false`
    /// emits the CONSUMING `__mll_run` (bind RHSes, value positions — the
    /// result is inspected, so any pending pure box must be stripped);
    /// `true` emits the FORWARDING `__mll_run_tail` (a `return`-position
    /// terminal, or an effect statement whose result is discarded). The
    /// forwarding runner tail-calls the action closure, which is what turns
    /// interprocedural action recursion (`mapM_ f (x:xs) = f x >> mapM_ f
    /// xs`) into a constant-stack Lua tail-call chain; the ≤1 pending box
    /// its result may carry is stripped by the consuming site at the
    /// chain's root (see the __mll_run contract comment in the runtime).
    pub(super) fn action_run_ast(&mut self, expr: &TExpr, tail: bool) -> Expr {
        let runner = if tail { "__mll_run_tail" } else { "__mll_run" };
        // Structural checks FIRST — the monad type variable may be
        // unresolved in bind chains, so we can't rely on the type alone.
        // pure(x) / return(x): performing it just yields x — and yields it
        // UNFORCED, per the eagerness contract (`return ⊥` must not raise until
        // the value is demanded). arg_ast with strict=false suspends a possibly-⊥
        // x in a thunk and leaves a provably-total x (literal, concrete var,
        // constructor of such) eager, so the common `return 0` stays a bare
        // value while `return (error "x")` / `return (n `div` 0)` become inert.
        // A bind site marks the resulting `<-` variable concrete only when this
        // yields WHNF (see action_result_is_whnf).
        if let TExprKind::App(func, arg) = &expr.kind
            && matches!(&func.kind, TExprKind::Var(n) if n == "pure" || n == "return") {
                return self.pure_action_ast(arg);
            }
        // return $ x / pure $ x: same as return(x)
        if let TExprKind::InfixApp { op, lhs, rhs } = &expr.kind
            && op == "$" && matches!(&lhs.kind, TExprKind::Var(n) if n == "pure" || n == "return") {
                return self.pure_action_ast(rhs);
            }
        // ST primitive calls now return closures — go through __mll_run like everything else
        if !Self::is_nullary_action_type(&expr.ty) {
            // If the type is concretely non-IO (resolved to a known type),
            // emit as a plain expression. But if the type is unresolved
            // (e.g. where-clause function with uninferred return type),
            // defensively wrap with __mll_run since we may be in a bind chain
            // where the expression must be an action.
            if Self::is_definitely_not_action(&expr.ty) {
                return self.expr_ast(expr);
            } else {
                let e = self.expr_ast(expr);
                return Expr::call_named(runner, vec![e]);
            }
        }
        match &expr.kind {
            TExprKind::Lit(_) | TExprKind::Con(_) | TExprKind::Tuple(_) => {
                self.expr_ast(expr)
            }
            // IO SpecCall: inline the Lua call directly (skip closure)
            TExprKind::SpecCall { specialized, args, .. } if specialized.starts_with("__mll_io:") => {
                let lua_func = &specialized["__mll_io:".len()..];
                // Type-directed decode of the FFI result: the host's raw Lua
                // value (arrays, dicts, nested records) is converted into the
                // mata-ll representation before it reaches mata-ll code.
                let decode = self.ffi_decode_desc(&expr.ty);
                let call = if let Some(method) = lua_func.strip_prefix(':') {
                    let recv = self.forced_prefix_ast(&args[0]);
                    let margs = self.ffi_args_ast(&args[1..]);
                    Expr::method(recv, method, margs)
                } else {
                    let cargs = self.ffi_args_ast(args);
                    Expr::call_named(lua_func, cargs)
                };
                match &decode {
                    Some(desc) => Expr::call_named(
                        "__mll_ffi_decode",
                        vec![
                            Expr::raw(desc.clone()),
                            call,
                            Expr::raw(format!("{:?}", Self::ffi_root_name(lua_func))),
                        ],
                    ),
                    // The declared result is ONE value: truncate the raw
                    // host call so extra return values cannot spread.
                    None => Expr::paren(call),
                }
            }
            // Fully-applied ST intrinsic in run-once position: emit the
            // effect directly, skipping the action-closure allocation and
            // the __mll_run dispatch. This path is only reached where an
            // action runs exactly once, in order, so this is safe by
            // construction. See st_intrinsic_fused.
            _ if Self::st_intrinsic_fused(expr).is_some() => {
                let (fused, fargs) = Self::st_intrinsic_fused(expr).unwrap();
                // Per-argument strictness of the ST array intrinsics. These
                // runtime helpers bypass demand analysis (they are not mata-ll
                // functions), so their strict positions are stated here. An
                // array and an index are ALWAYS forced — you cannot allocate,
                // read, or write through a thunk — so passing them eagerly is
                // sound and, on the tracker's hot loop (four writes per note,
                // every audio frame), removes a thunk allocation per index
                // expression like `ch * 14 + off`. The *stored value* and the
                // initializer are strict too: the fused runtime forces them ON
                // THIS CALL (`__mll_st_write` stores `__force(val)` — that is
                // the invariant that keeps every slot, and hence every read
                // result, in WHNF), so evaluating the argument in place only
                // moves the force a few instructions earlier within the same
                // run-once statement — it cannot change what is forced. This
                // applies ONLY to the fused (provably run-once) form; the
                // first-class `__mll_ma_*` closures keep lazy value arguments
                // because a built-but-never-run action must not force anything
                // (see STRICT_BUILTINS in demand.rs).
                let strict_mask: &[bool] = match fused {
                    "__mll_st_new" => &[true, true],         // size, init (forced on store)
                    "__mll_st_read" => &[true, true],        // arr, idx
                    "__mll_st_write" => &[true, true, true], // arr, idx, val (forced on store)
                    "__mll_st_modify" => &[true, true, true],// arr, idx, f (f is called)
                    "__mll_st_length" => &[true],
                    "__mll_st_from_list" => &[true],
                    "__mll_st_to_list" => &[true],
                    _ => &[],
                };
                let mut cargs = Vec::new();
                for (i, a) in fargs.iter().enumerate() {
                    let strict = strict_mask.get(i).copied().unwrap_or(false);
                    cargs.push(self.arg_ast(a, strict));
                }
                Expr::call_named(fused, cargs)
            }
            _ => {
                // Direct-perform tail: a saturated call to a module-level
                // DIRECT-PERFORM function (its emitted body IS the action —
                // see direct_perform_arity / direct_perform_fns) PERFORMS
                // when called and returns a result in the runners' range,
                // on which the forwarding runner is the identity. So in a
                // forwarding position the call returns bare: `return
                // callee(...)` is the exact syntactic form Lua's tail-call
                // elimination reclaims the frame for, and it skips the
                // runner re-application whose `__force` would evaluate a
                // thunk `pure` payload GHC never forces on unwind. The
                // callee may be the function being emitted (self-recursion)
                // or ANY other direct-perform function (mutual recursion,
                // `f` ↔ `g`): the invariant is the callee's, not the
                // caller's — the callee's single pending consumer
                // application simply becomes this function's (the
                // one-root-application invariant; see the __mll_run contract
                // comment in the runtime), and every forwarding position
                // (a direct-perform body's terminal, a first-class action
                // closure's terminal, a discarded effect statement) delivers
                // its value to exactly one consumer application. Builders
                // (multi-clause IO functions, the two-level shape; ST
                // closures) are never in the map and keep their runner,
                // which performs the action closure they return.
                if tail && let Some(arity) = self.direct_perform_callee_arity(expr) {
                    let e = self.expr_ast(expr);
                    if Self::call_head_is(&e, &self.direct_perform_callee_ref(expr)) {
                        return e;
                    }
                    // Arity-0 callee: the emission is the bare function
                    // reference — the call is spelled here.
                    if arity == 0 && matches!(&e, Expr::Name(n) if *n == self.direct_perform_callee_ref(expr)) {
                        return Expr::call(e, vec![]);
                    }
                    // Any other emitted shape (an inlined body, an adapter):
                    // keep the runner — it handles every action form.
                    return Expr::call_named(runner, vec![e]);
                }
                // General IO/ST action: the runner handles both direct
                // values and action closures (function or value).
                let e = self.expr_ast(expr);
                Expr::call_named(runner, vec![e])
            }
        }
    }

    /// If `expr` is a SATURATED call to a module-level direct-perform
    /// function (`direct_perform_fns`, seeded by module_stmts before any
    /// body is emitted), its arity: an application spine (grouping parens
    /// transparent) whose head is a `Var` spelling an un-shadowed name in
    /// the map, applied to exactly that entry's parameter count. Shadowing
    /// check: a local binding of the same name is a first-class value whose
    /// emission arm this claim knows nothing about — the test is the same
    /// `local_vars` membership `lua_ref` resolves references by. Partial
    /// applications (a closure value, not a perform), over-applications and
    /// SpecCall spines (runtime/FFI protocols, never a compiled function)
    /// report `None` and keep the runner.
    fn direct_perform_callee_arity(&self, expr: &TExpr) -> Option<usize> {
        let (name, nargs) = Self::var_call_spine(expr)?;
        let arity = *self.direct_perform_fns.get(name)?;
        (nargs == arity && !self.local_vars.contains(&sanitize_name(name))).then_some(arity)
    }

    /// The Lua reference a direct-perform callee is emitted as (its
    /// `__mll_fn` slot or plain name — the same resolution `expr_ast` uses
    /// for a concrete top-level function). Only meaningful after
    /// `direct_perform_callee_arity` accepted `expr`.
    fn direct_perform_callee_ref(&self, expr: &TExpr) -> String {
        let (name, _) = Self::var_call_spine(expr).expect("accepted direct-perform spine");
        self.lua_ref(&sanitize_name(name))
    }

    /// The head name and argument count of an application spine headed by
    /// a `Var` (grouping parens transparent); `None` for any other head.
    fn var_call_spine(expr: &TExpr) -> Option<(&str, usize)> {
        let mut nargs = 0usize;
        let mut f = expr;
        loop {
            match &f.kind {
                TExprKind::Paren(inner) => f = inner.as_ref(),
                TExprKind::App(inner_f, _) => {
                    nargs += 1;
                    f = inner_f.as_ref();
                }
                TExprKind::Var(n) => return Some((n, nargs)),
                _ => return None,
            }
        }
    }

    /// Whether the emitted expression is a call chain whose innermost callee
    /// is exactly the name `head` — the shape `general_call_ast` produces
    /// for a saturated call to a concrete top-level function
    /// (`__mll_fn[k](a, b)`), as opposed to an inlined body (`Paren`), a
    /// runtime-generic adapter, or a partial-application closure. The bare
    /// tail is emitted only for this shape; the gate is on the EMITTED tree,
    /// so no belief about which expr_ast arm fired is relied on.
    fn call_head_is(e: &Expr, head: &str) -> bool {
        let mut e = e;
        let mut depth = 0usize;
        while let Expr::Call(f, _) = e {
            e = f.as_ref();
            depth += 1;
        }
        depth > 0 && matches!(e, Expr::Name(n) if n == head)
    }

    pub(super) fn is_nullary_action_type(ty: &Ty) -> bool {
        matches!(ty, Ty::IO(_) | Ty::LuaIO(_, _))
            || matches!(ty, Ty::App(f, _) if matches!(f.as_ref(),
                Ty::App(c, _) if matches!(c.as_ref(), Ty::Con(n) if n == "ST")))
    }

    /// Returns true if the type is definitely NOT an IO/ST action.
    /// Unresolved type variables and type applications with variable
    /// heads return false (they might be actions).
    pub(super) fn is_definitely_not_action(ty: &Ty) -> bool {
        matches!(ty,
            Ty::Con(_) | Ty::Arrow(..) | Ty::List(_) | Ty::Unit
            | Ty::Forall(_, _) | Ty::Skolem(..))
    }

    /// If `expr` is a *fully applied* call to a known ST array intrinsic,
    /// return the closure-free runtime function name and the argument list.
    ///
    /// In run-once position (a do-block bind chain), `readSTArray arr i`
    /// compiles to `__mll_run(__mll_ma_read(arr, i))`, where `__mll_ma_read`
    /// allocates an action closure that `__mll_run` immediately calls. The
    /// fused `__mll_st_*` functions perform the effect directly and return
    /// the value, so the caller can emit a single direct call — no closure
    /// allocation, no `__mll_run` dispatch. See
    /// experiments/tracker/PERF-REGRESSION.md.
    ///
    /// Returns None for partial applications (an Arrow, never in action
    /// position), first-class action references (`__mll_run(<var>)`), and
    /// non-intrinsics — all of which keep the closure form.
    pub(super) fn st_intrinsic_fused(expr: &TExpr) -> Option<(&'static str, Vec<&TExpr>)> {
        let mut args: Vec<&TExpr> = Vec::new();
        let mut f = expr;
        while let TExprKind::App(inner_f, inner_arg) = &f.kind {
            args.push(inner_arg.as_ref());
            f = inner_f.as_ref();
        }
        args.reverse();
        let name = match &f.kind {
            TExprKind::Var(n) => n.as_str(),
            _ => return None,
        };
        let (fused, arity) = match name {
            "newSTArray" => ("__mll_st_new", 2),
            "readSTArray" => ("__mll_st_read", 2),
            "writeSTArray" => ("__mll_st_write", 3),
            "modifySTArray" => ("__mll_st_modify", 3),
            "stArrayLength" => ("__mll_st_length", 1),
            "newSTArrayFromList" => ("__mll_st_from_list", 1),
            "stArrayToList" => ("__mll_st_to_list", 1),
            _ => return None,
        };
        if args.len() == arity {
            Some((fused, args))
        } else {
            None
        }
    }

    /// Check if a function type's return type is an IO/ST action.
    pub(super) fn returns_action(ty: &Ty) -> bool {
        match ty {
            Ty::Arrow(_, ret, _) => Self::returns_action(ret),
            _ => Self::is_nullary_action_type(ty),
        }
    }

    /// Check if a function type's return type is specifically an ST action.
    pub(super) fn returns_st(ty: &Ty) -> bool {
        match ty {
            Ty::Arrow(_, ret, _) => Self::returns_st(ret),
            _ => Self::is_st_type(ty),
        }
    }

    /// Emit an expression that sits in TAIL position — i.e. its value becomes
    /// the enclosing function's result via `return <expr>`.
    ///
    /// TAIL-CALL CONTRACT (this is the single place the property is enforced):
    /// Lua performs a proper, stack-frame-replacing tail call *only* for the
    /// exact syntactic form `return <functioncall>`. Wrapping the call in
    /// parentheses — `return (f(x))` — is NOT a tail call: the parentheses
    /// truncate the call to a single value and Lua keeps the current frame, so
    /// deeply tail-recursive functions overflow the stack. mata-ll functions
    /// always denote a single value, so source-level and desugarer-introduced
    /// parentheses are semantically transparent here; stripping them turns
    /// `return (f x)` into `return f(x)` and lets Lua reclaim the frame.
    ///
    /// Every genuine tail position (function-clause bodies, guard results,
    /// if/case branch results, let-body results, lambda bodies) funnels its
    /// result expression through this helper, so the tail-call property holds
    /// uniformly rather than being re-derived per construct. Note the caller
    /// still emits the literal `return `/`\n`; this only strips the wrappers
    /// and dispatches to the right emitter.
    ///
    /// Nested tail calls compose: `return (function() ... return f(x) end)()`
    /// (the IIFE that if/case/let expressions lower to) is itself a tail call
    /// to the closure, and the closure tail-calls `f`, so the whole chain runs
    /// in constant stack — which is why only the paren wrapper, not the IIFE,
    /// has to be stripped.
    ///
    /// Action terminals (`inside_action`) keep the property through the
    /// runner: `return __mll_run_tail(a)` tail-calls the forwarding runner,
    /// which tail-calls the action closure — so IO/ST recursion that crosses
    /// a function boundary per step (`mapM_`, a recursive `loop n`) is also
    /// constant-stack. See action_run_ast and the runtime's __mll_run
    /// contract comment.
    pub(super) fn tail_ast(&mut self, expr: &TExpr, inside_action: bool) -> Expr {
        let mut e = expr;
        while let TExprKind::Paren(inner) = &e.kind {
            e = inner.as_ref();
        }
        if inside_action {
            // Action terminal: run through the FORWARDING runner, whose
            // function arm is a bare `return action()` — so the whole
            // `return __mll_run_tail(a)` emission is a two-step tail chain
            // and recursive action sequencing runs in constant stack. Any
            // pending pure box rides through to the consuming site at the
            // chain's root (see action_run_ast).
            self.action_run_ast(e, true)
        } else {
            self.expr_ast(e)
        }
    }

    /// Flatten a monadic bind chain (from do-notation) into sequential
    /// local statements.
    /// When `inside_action` is true, terminal IO expressions are performed
    /// (called with `()`) because we're inside a do-block action closure.
    /// When false (regular function body), IO actions are returned as-is.
    pub(super) fn bind_chain_block(&mut self, expr: &TExpr, inside_action: bool) -> Block {
        // Iterative loop for right-spine bind chains to avoid stack overflow
        // on deeply nested do-blocks. Only recurses for non-spine children
        // (individual expressions, if-branches) which have bounded depth.
        let mut stmts: Vec<Stmt> = Vec::new();
        let mut expr = expr;
        let mut inside_action = inside_action;
        // Suffix demand maps for the current nested-`let` spine, computed in
        // one backward pass when the chain enters a run of `let` statements
        // (see demand::let_spine_maps). Without this every `let` statement
        // re-walked the whole remaining chain for its eagerization seed —
        // quadratic over a long do-block of `let`s. Keyed by node identity
        // (a NodeMap: the borrow checker pins the TIR while it lives), and
        // only valid for the result demand it was computed under, which
        // the `Let` arm checks before use.
        let mut spine_maps: Option<(crate::demand::Demand,
            crate::demand::NodeMap<'_, crate::demand::DemandMap>)> = None;
        loop {
            match &expr.kind {
                TExprKind::InfixApp { op, lhs, rhs } if op == ">>=" => {
                    if let TExprKind::Lambda { params, body } = &rhs.kind {
                        // A do-STATEMENT desugars to `action >>= \_ -> rest`.
                        // Emit it exactly like the `>>` arm below — a bare
                        // statement through the forwarding runner — instead
                        // of `local _ = __mll_run(...)`. This is not just
                        // cosmetic: the consuming runner's frame stays live
                        // for the whole call and holds the action closure,
                        // whose captured tail reference retains every list
                        // cell the walk realizes — a discarded
                        // `mapM_ f xs` over a million-element lazy list
                        // pinned the entire prefix until the walk finished.
                        // The statement form hands the closure to
                        // __mll_run_tail, whose tail call releases it, so
                        // consumed cells are collectable as the walk moves.
                        if params.len() == 1 && params[0].0 == "_" {
                            let lhs_unwrapped = if let TExprKind::Paren(inner) = &lhs.kind { inner.as_ref() } else { lhs.as_ref() };
                            let is_pure_discard = matches!(&lhs_unwrapped.kind,
                                TExprKind::App(func, _) if matches!(&func.kind,
                                    TExprKind::Var(n) if n == "pure" || n == "return"));
                            if !is_pure_discard {
                                let saved_rd = std::mem::replace(
                                    &mut self.cur_result_demand, crate::demand::Demand::Head);
                                let action = self.action_run_ast(lhs_unwrapped, true);
                                self.cur_result_demand = saved_rd;
                                stmts.push(Stmt::Expr(action));
                            }
                            expr = body;
                            inside_action = true;
                            continue;
                        }
                        let param_name = sanitize_name(&params[0].0);
                        let (pre, decl) = self.declare_local_parts(&param_name);
                        if let Some(s) = pre {
                            stmts.push(s);
                        }
                        // A statement action's result is not the function's
                        // result — no deep result demand inside it.
                        let saved_rd = std::mem::replace(
                            &mut self.cur_result_demand, crate::demand::Demand::Head);
                        let rhs_e = self.bound_action_ast(lhs);
                        self.cur_result_demand = saved_rd;
                        stmts.push(decl.stmt(rhs_e));
                        // The bound value is force-free downstream only if the
                        // action yields WHNF. A `return ⊥` binds a thunk (kept
                        // lazy per the eagerness contract), so its uses must
                        // force it — do NOT mark it concrete (and clear any
                        // concreteness a same-named outer binding left).
                        if self.action_result_is_whnf(lhs) {
                            self.concrete_vars.insert(param_name);
                        } else {
                            self.concrete_vars.remove(&param_name);
                        }
                        expr = body;
                        inside_action = true;
                        continue;
                    }
                }
                TExprKind::InfixApp { op, lhs, rhs } if op == ">>" => {
                    let lhs_unwrapped = if let TExprKind::Paren(inner) = &lhs.kind { inner.as_ref() } else { lhs.as_ref() };
                    // return/pure on the LHS of >> is a no-op (pure value discarded)
                    let is_pure_discard = matches!(&lhs_unwrapped.kind,
                        TExprKind::App(func, _) if matches!(&func.kind,
                            TExprKind::Var(n) if n == "pure" || n == "return"));
                    if !is_pure_discard {
                        // Statement position — see the ">>=" arm above. The
                        // result is DISCARDED, so the forwarding runner is
                        // used: identical forcing/effect behaviour, and it
                        // skips the consuming runner's unbox of a result
                        // nobody looks at.
                        let saved_rd = std::mem::replace(
                            &mut self.cur_result_demand, crate::demand::Demand::Head);
                        let action = self.action_run_ast(lhs_unwrapped, true);
                        self.cur_result_demand = saved_rd;
                        stmts.push(Stmt::Expr(action));
                    }
                    expr = rhs;
                    inside_action = true;
                    continue;
                }
                TExprKind::Let { binds, body } => {
                    // Forward-declare all names before assigning so do-block let
                    // bindings can be self- and mutually recursive. Lua locals
                    // are not in scope within their own initializer (see
                    // where_binds_stmts for the rationale).
                    {
                        // Forward-declare each name in THIS group with a FRESH
                        // local/slot — even one shadowing an outer binding. A
                        // do-`let` is its own (recursive) scope, so a later
                        // `let x = …` must not reuse the earlier `x`'s storage:
                        // a lazy thunk that captured the earlier binding would
                        // otherwise observe the rebind when forced (Int masked
                        // this by evaluating trivial bindings eagerly; a boxed
                        // Integer binding is a thunk and exposes it). `seen`
                        // dedups only within one mutually-recursive group.
                        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                        for bind in binds {
                            let bname = sanitize_name(&bind.name);
                            if seen.insert(bname.clone()) {
                                stmts.extend(self.declare_local_fwd_stmts(&bname));
                            }
                        }
                    }
                    // Bindings demanded by the rest of the chain may be
                    // evaluated eagerly even when they read suspended values —
                    // the force happens regardless (see demanded_bindings).
                    // The chain terminal carries the current function's result
                    // demand, so a binding that is only forced THROUGH the
                    // result (a tuple field every caller scrutinizes) counts.
                    // Seed = the demand map of the rest of the chain. Served
                    // from the precomputed spine maps when possible; falling
                    // back to the direct (suffix-walking) computation when
                    // the cache does not cover this node or was computed
                    // under a different result demand. Both produce the same
                    // map — the cache is purely a cost saving.
                    let mut seed: Option<crate::demand::DemandMap> =
                        match &spine_maps {
                            Some((rd, maps)) if *rd == self.cur_result_demand =>
                                maps.get(body).cloned(),
                            _ => None,
                        };
                    if seed.is_none()
                        && let Some(maps) = crate::demand::let_spine_maps(
                            expr,
                            &self.demand_info.rows,
                            &self.local_demand_rows,
                            &|n| self.inline_fns.contains_key(n),
                            &self.cur_result_demand,
                        ) {
                            // The spine starts at `expr` (a Let), so `body`
                            // is always covered.
                            seed = maps.get(body).cloned();
                            spine_maps =
                                Some((self.cur_result_demand.clone(), maps));
                        }
                    let seed = seed.unwrap_or_else(|| crate::demand::demanded_map(
                        body,
                        &self.demand_info.rows,
                        &self.local_demand_rows,
                        &|n| self.inline_fns.contains_key(n),
                        &self.cur_result_demand,
                    ));
                    let demanded = self.demanded_bindings(binds, seed);
                    for (i, bind) in binds.iter().enumerate() {
                        let bname = sanitize_name(&bind.name);
                        let lval = self.local_lvalue(&bname);
                        // Both the if-fast-path and the cheap-path evaluate the
                        // RHS strictly, so they may only be used when the binding
                        // does not read a still-nil sibling (see strict_binding_safe)
                        // AND evaluating the RHS now is sound: it either cannot
                        // force a suspended computation (is_cheap_to_force) or is
                        // provably demanded anyway — a let binding is not demanded
                        // until used, so eagerly evaluating one that can
                        // raise/diverge changes program behaviour.
                        let strict_ok = strict_binding_safe(binds, i);
                        if let TExprKind::If { cond, then_branch, else_branch } = &bind.body.kind
                            && strict_ok
                            && (self.is_cheap_to_force(&bind.body) || demanded.contains(&bind.name)) {
                            self.concrete_vars.insert(bname.clone());
                            let cond_e = self.expr_ast(cond);
                            let then_e = self.expr_ast(then_branch);
                            let else_e = self.expr_ast(else_branch);
                            stmts.push(Stmt::AssignIf { lhs: lval, cond: cond_e, then_e, else_e });
                        } else if Self::is_nullary_action_type(&bind.body.ty) {
                            // First-class action binding — its result is not
                            // the function's result (see cur_result_demand).
                            let saved_rd = std::mem::replace(
                                &mut self.cur_result_demand, crate::demand::Demand::Head);
                            let action = self.action_run_ast(&bind.body, false);
                            stmts.push(Stmt::Assign(lval, Expr::inline_fn0(action)));
                            self.cur_result_demand = saved_rd;
                        } else {
                            let saved_rd = std::mem::replace(
                                &mut self.cur_result_demand, crate::demand::Demand::Head);
                            if self.strict_binding_ok(bind, &demanded) && strict_ok {
                                let rhs_e = self.expr_ast(&bind.body);
                                stmts.push(Stmt::Assign(lval, rhs_e));
                                self.concrete_vars.insert(bname);
                            } else {
                                // Thunked: must not stay marked concrete (a
                                // same-named outer binding may have been).
                                self.concrete_vars.remove(&bname);
                                if let Some(v) = bare_var_alias(binds, i) {
                                    // Bare-variable RHS: share the existing
                                    // thunk-or-value (see bare_var_alias).
                                    let rhs_e = self.lazy_ref_ast(v);
                                    stmts.push(Stmt::Assign(lval, rhs_e));
                                } else {
                                    let rhs_e = self.expr_ast(&bind.body);
                                    stmts.push(Stmt::Assign(lval, Expr::thunk(rhs_e)));
                                }
                            }
                            self.cur_result_demand = saved_rd;
                        }
                    }
                    expr = body;
                    continue;
                }
                _ => {}
            }
            // Terminal expression
            match &expr.kind {
                TExprKind::If { cond, then_branch, else_branch } => {
                    let cond_e = self.expr_ast(cond);
                    let then_b = self.bind_chain_block(then_branch, inside_action);
                    let else_b = self.bind_chain_block(else_branch, inside_action);
                    stmts.push(Stmt::If {
                        cond: cond_e,
                        then_b,
                        elseifs: vec![],
                        else_b: Some(else_b),
                    });
                }
                TExprKind::Case { scrutinee, branches } => {
                    // Flatten the case terminal at statement level, exactly
                    // like the If arm above: each branch takes its own tail
                    // decision through bind_chain_block (via the shared
                    // pattern-match emitter's `tails` mode), so an action
                    // branch's terminal `pure e` goes through pure_action_ast
                    // — the box convention, a fixpoint of both runners —
                    // instead of becoming the first-class pure-suspension
                    // closure inside a dispatch IIFE handed to
                    // `__mll_run_tail`, whose extra application forced a
                    // thunk payload GHC never forces. Guard-bearing branches
                    // route through the guarded builder with the same tails.
                    let saved_locals = self.local_vars.clone();
                    let saved_concrete = self.concrete_vars.clone();
                    let (pre, decl) = self.declare_local_parts("_s");
                    if let Some(s) = pre {
                        stmts.push(s);
                    }
                    let scrut_e = self.forced_ast(scrutinee);
                    stmts.push(decl.stmt(scrut_e));
                    // The scrutinee local is forced above: mark it concrete so
                    // match_scrutinee does not re-force it per clause.
                    let sref = self.lua_ref("_s");
                    self.concrete_vars.insert(sref.clone());
                    let clauses: Vec<TClause> = branches
                        .iter()
                        .map(|b| TClause {
                            span: None,
                            patterns: vec![b.pattern.clone()],
                            guards: b.guards.clone(),
                            body: b.body.clone(),
                            where_binds: vec![],
                        })
                        .collect();
                    let b = self.pattern_match_block_tails(
                        &[sref],
                        &clauses,
                        Some(inside_action),
                    );
                    stmts.extend(b.0);
                    self.local_vars = saved_locals;
                    self.concrete_vars = saved_concrete;
                }
                _ => {
                    // Tail position: strip transparent parens so a wrapped call
                    // (`return (f x)`) becomes a proper Lua tail call. See tail_ast.
                    let tail = self.tail_ast(expr, inside_action);
                    stmts.push(Stmt::Return(tail));
                }
            }
            break;
        }
        Block(stmts)
    }

    pub(super) fn is_st_type(ty: &Ty) -> bool {
        match ty {
            Ty::App(f, _) => match f.as_ref() {
                Ty::App(c, _) => matches!(c.as_ref(), Ty::Con(n) if n == "ST"),
                _ => false,
            },
            _ => false,
        }
    }
}
