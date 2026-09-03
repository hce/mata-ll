//! Eager-vs-thunk decisions at emission sites: arguments, callees, forcing.
//! These builders produce `lua::Expr` nodes; the decision logic is unchanged.
//!
//! `arg_ast` decides each argument: emitted eagerly when the position is
//! strict or the expression is provably total (`strict ||
//! is_cheap_to_force`); otherwise suspended in a thunk — except a bare Var
//! or nullary Con (already a thunk-or-value, passed raw) and a tuple
//! literal (building the table forces nothing). `expr_yields_whnf` is
//! the single point of truth for "wrapping this emission in `__force` is
//! redundant", consulted by `forced_ast` / `forced_prefix_ast`.
//! `flatten_lambda` / `lambda_param_names` implement the N-ary calling
//! convention for curried lambdas; `runtime_generic_adapter` curries
//! arguments handed to the erased runtime generics (map, zipWith);
//! `callee_ast` parenthesizes bare function literals in Lua call position,
//! and `known_callee_arity` / `split_call_ast` keep a flat call from handing
//! a fixed-arity callee more arguments than it has parameters.

use crate::tir::*;
use crate::types::Ty;
use super::CodeGen;
use super::lua::{Expr, FuncBody, Stmt};
use super::names::{is_builtin_op, primitive_method_lua_op, sanitize_name};
use super::util::{count_arrows};

impl CodeGen {
    /// Build an expression in function-call position.
    /// Variables known to be concrete (already forced) are emitted bare.
    /// Unknown variables are forced — they may be let-bound thunks.
    fn expr_raw_ast(&mut self, expr: &TExpr) -> Expr {
        if let TExprKind::Var(name) = &expr.kind {
            match name.as_str() {
                // A local binder shadows every special name (see the
                // expr_ast Var arm): fall to the ordinary variable path.
                _ if self.is_local_shadowed(name) => {
                    let sname = sanitize_name(name);
                    let lref = self.lua_ref(&sname);
                    if self.concrete_vars.contains(&sname) {
                        Expr::name(lref)
                    } else {
                        Expr::force(Expr::name(lref))
                    }
                }
                "otherwise" => Expr::lit("true"),
                // First-class / partially-applied `seq` as a callee resolves to
                // the runtime primitive (see the expr_ast Var arm).
                "seq" => Expr::name("__mll_seq"),
                // Prefix / partially-applied / first-class `div` and `mod`
                // resolve to their forcing wrappers (see the expr_ast Var arm);
                // the inline backtick form stays on the strict core.
                "div" => Expr::name("__mll_div_fn"),
                "mod" => Expr::name("__mll_mod_fn"),
                "quot" => Expr::name("__mll_quot_fn"),
                "rem" => Expr::name("__mll_rem_fn"),
                _ => {
                    let sname = sanitize_name(name);
                    let lref = self.lua_ref(&sname);
                    if self.concrete_vars.contains(&sname) {
                        Expr::name(lref)
                    } else if self.runtime_fn_ref(&sname) {
                        // A runtime prelude function value — never a thunk
                        // (see runtime_fn_ref); checked in refutation mode
                        // like every other bare-read claim.
                        self.claim_checked("__assert_whnf", Expr::name(lref))
                    } else {
                        Expr::force(Expr::name(lref))
                    }
                }
            }
        } else {
            self.expr_ast(expr)
        }
    }

    /// mata-ll's calling convention is N-ary: every function value is ONE Lua
    /// function taking all `count_arrows(type)` arguments at once. Top-level
    /// functions are emitted that way (clause params plus `_eta` padding, see
    /// function_stmts), partial applications close over the missing arguments,
    /// and application sites flatten the whole spine into a single flat call —
    /// `f 1 2` emits `f(1, 2)` (see the App arm and __mll_wrap_callback_out).
    /// A curried lambda `\x -> \y -> e` must therefore also become one
    /// two-parameter Lua function: the nested one-parameter form would consume
    /// only the first argument of a flat call and silently return the inner
    /// closure with the remaining arguments dropped.
    ///
    /// Walks directly nested lambdas (through source parens) and returns the
    /// flattened parameter list (original names) plus the innermost body. The
    /// caller eta-pads any arrows the type still has beyond these parameters.
    pub(super) fn flatten_lambda<'a>(
        params: &'a [(String, Ty)],
        body: &'a TExpr,
    ) -> (Vec<&'a str>, &'a TExpr) {
        let mut names: Vec<&str> = params.iter().map(|(s, _)| s.as_str()).collect();
        let mut b = body;
        loop {
            let mut inner = b;
            while let TExprKind::Paren(p) = &inner.kind {
                inner = p.as_ref();
            }
            if let TExprKind::Lambda { params, body } = &inner.kind {
                names.extend(params.iter().map(|(s, _)| s.as_str()));
                b = body.as_ref();
            } else {
                break;
            }
        }
        (names, b)
    }

    /// Sanitize a flattened lambda parameter list for emission. A duplicate
    /// name (`\x -> \x -> x`) is legal in source — the inner binding shadows
    /// the outer — but after flattening both would appear in one Lua parameter
    /// list. Only the LAST occurrence is visible to the body (every earlier
    /// one was shadowed before any body could reference it), so rename the
    /// earlier occurrences to fresh dead names.
    pub(super) fn lambda_param_names(orig: &[&str]) -> Vec<String> {
        let mut names: Vec<String> = orig.iter().map(|s| sanitize_name(s)).collect();
        for i in 0..names.len() {
            if names[i + 1..].contains(&names[i]) {
                names[i] = format!("_sh{}", i);
            }
        }
        names
    }

    /// Currying adapter for the hand-written runtime generics.
    ///
    /// map and zipWith exist as ONE erased Lua copy and call their function
    /// parameter with its GENERIC arity (map's `f : a -> b` gets exactly one
    /// argument, zipWith's `f : a -> b -> c` exactly two). Compiled generic
    /// code is monomorphized per instantiation, so its call sites always match
    /// the N-ary convention — but these builtins cannot. When the result type
    /// variable is itself instantiated to a function (`map (\n -> \x -> ...)`
    /// building a list of adders), the argument's real arity exceeds the
    /// builtin's view and the plain call would silently drop arguments.
    ///
    /// Returns the runtime adapter (`__mll_curry1`/`__mll_curry2`) to wrap the
    /// argument in when `callee` (paren-stripped) is a bare reference to such
    /// a builtin, the argument lands in its function-parameter position
    /// (`args_before == 0`), and `arg_ty` has more arrows than the builtin's
    /// generic view. A same-named local or compiled (user/specialized)
    /// function is NOT the runtime builtin and needs no adapter.
    pub(super) fn runtime_generic_adapter(
        &self,
        callee: &TExpr,
        args_before: usize,
        arg_ty: &Ty,
    ) -> Option<&'static str> {
        if args_before != 0 {
            return None;
        }
        let mut c = callee;
        while let TExprKind::Paren(p) = &c.kind {
            c = p.as_ref();
        }
        let TExprKind::Var(name) = &c.kind else { return None };
        let k = match name.as_str() {
            "map" => 1,
            "zipWith" => 2,
            _ => return None,
        };
        if self.local_vars.contains(name.as_str()) || self.fn_table.contains_key(name.as_str()) {
            return None;
        }
        if count_arrows(arg_ty) > k {
            Some(if k == 1 { "__mll_curry1" } else { "__mll_curry2" })
        } else {
            None
        }
    }

    /// True when expr_ast builds this expression as a bare, unparenthesized
    /// Lua function literal (`function ... end`): lambdas — which include
    /// operator sections like `(+1)`, desugared to lambdas by the parser —
    /// and operator functions like `(+)`. Every other expression kind either
    /// emits a callable reference (a name, `__mll_fn[i]`) or already wraps
    /// itself in parentheses (Paren, If, partial-application closures, ...).
    pub(super) fn is_bare_fn_literal(expr: &TExpr) -> bool {
        matches!(&expr.kind, TExprKind::OpFunc(_) | TExprKind::Lambda { .. })
    }

    /// Build an expression in Lua *call position* — immediately followed by
    /// `(args)`. Lua's grammar rejects calling a function literal directly:
    /// `function() ... end(x)` is a syntax error; the literal must be
    /// parenthesized, `(function() ... end)(x)`. Only bare function literals
    /// get the extra parens, so all other callees emit exactly as before.
    pub(super) fn callee_ast(&mut self, f: &TExpr) -> Expr {
        let needs_wrap = Self::is_bare_fn_literal(f);
        let e = self.expr_raw_ast(f);
        if needs_wrap { Expr::paren(e) } else { e }
    }

    /// The Lua parameter count of `f` when that count is FIXED by the
    /// callee's definition rather than by its type at this use site —
    /// `None` when the two always agree and the N-ary convention's default
    /// (one flat call carrying every outstanding argument) is right.
    ///
    /// The convention holds because monomorphization gives each use its own
    /// copy, whose declared arrow count is the instantiated one — including
    /// the shared prelude builtins, which mono specializes exactly when an
    /// instantiation widens their arity (`specialization_wanted`). What is
    /// left outside that guarantee is every callee mono cannot copy:
    ///
    ///   * the runtime prelude functions (`head`, `map`, `foldr`, …), which
    ///     have no mata-ll body — one erased Lua definition serves every
    ///     instantiation;
    ///   * a function whose specializations were purged at the limit, whose
    ///     uses fall back to the shared generic copy.
    ///
    /// For those, instantiating a result type variable to a function makes a
    /// use site carry MORE arguments than the callee has parameters
    /// (`const inc "x" 3` against `const :: a -> b -> a`); Lua drops the
    /// excess and hands back the unapplied function. `split_call_ast` uses
    /// this to saturate the callee and apply its result to the rest.
    ///
    /// Only a bare name can be identified this way. A local binder shadowing
    /// the name makes the callee an unknown local (whose arity IS its type's
    /// arrow count — local bindings are monomorphic), and every other callee
    /// shape — a lambda, a partial application, a call result — is emitted
    /// with exactly `count_arrows` parameters by construction.
    pub(super) fn known_callee_arity(&self, f: &TExpr) -> Option<usize> {
        let mut c = f;
        while let TExprKind::Paren(p) = &c.kind {
            c = p.as_ref();
        }
        let TExprKind::Var(name) = &c.kind else { return None };
        // A where-local FUNCTION has a registered emitted arity (A19: a
        // generalized local can be used at more arrows than its one emitted
        // closure has parameters, so its calls split like a top-level
        // callee's). Checked before the shadowing bailout: the group name IS
        // a local. Registration is scoped (ScopeSnapshot), and sibling
        // value bindings remove their names, so a stale entry never
        // outlives the where group that made it.
        if let Some(&n) = self.local_fn_arity.get(name.as_str()) {
            return Some(n);
        }
        if self.is_local_shadowed(name) {
            return None;
        }
        if let Some(&n) = self.fixed_arity.get(name.as_str()) {
            return Some(n);
        }
        // A user definition under the same source name shadows the prelude's
        // (it is emitted after the runtime block), so the prelude parameter
        // count applies only to names the module does not define.
        if self.module_fn_names.contains(name.as_str()) {
            return None;
        }
        super::runtime::prelude_arity(&sanitize_name(name))
    }

    /// Emit `callee(args…)` under the N-ary convention, splitting the call
    /// when `arity` (from `known_callee_arity`) proves the callee takes fewer
    /// parameters than there are arguments: the first `arity` arguments
    /// saturate it, and its result — a function value, forced because a call
    /// may yield a thunk — is applied to the rest.
    ///
    /// One split always suffices. The saturating call yields an ordinary
    /// value whose own arity is the arrow count of its type, and that is
    /// exactly the number of arguments left over.
    ///
    /// A zero-arity callee is never split: a value binding or a nullary IO
    /// action reached with arguments is not an over-application of a curried
    /// function, and `f()(x)` would perform the action to call its result.
    pub(super) fn split_call_ast(callee: Expr, mut args: Vec<Expr>, arity: Option<usize>) -> Expr {
        match arity {
            Some(n) if n > 0 && args.len() > n => {
                let rest = args.split_off(n);
                Expr::call(Expr::force(Expr::call(callee, args)), rest)
            }
            _ => Expr::call(callee, args),
        }
    }

    /// True when the Lua that `expr_ast` builds for `expr` is GUARANTEED to
    /// evaluate to a WHNF (non-thunk) value, so wrapping that emission in
    /// `__force` is provably redundant.
    ///
    /// This is the single point of truth every "I need a forced value here"
    /// emission site consults (see forced_ast / forced_prefix_ast): without
    /// it, sites wrapped `__force(` around the emitted output blindly, which
    /// re-forced already-concrete variables and produced nonsensical
    /// `__force(__force(x))` doubles — pure waste on hot paths (`__force`
    /// was 27% of the tracker benchmark's runtime).
    ///
    /// `__force` is idempotent, so a `false` here only costs a cheap probe;
    /// soundness requires NO false positives — every `true` arm below must be
    /// justified by the corresponding expr_ast emission:
    ///   - Lit: denotes a value. Negate: emits `(-…)`, a number.
    ///   - Var: the expr_ast Var arm emits a bare name only when it is in
    ///     `concrete_vars` (provably WHNF) and `__force(name)` otherwise;
    ///     the special names (`otherwise`, `seq`, `div`, `mod`) emit a
    ///     boolean or a runtime function value.
    ///   - Con: `[]` is nil; a nullary constructor is a prebuilt table; a
    ///     constructor with fields references a Lua function value.
    ///   - Tuple: emits a Lua table literal (fields may be lazy, but WHNF is
    ///     about the head, and a table already is one).
    ///   - Lambda / OpFunc: emit Lua function literals.
    ///   - InfixApp: see infix_yields_whnf.
    ///   - App of a record accessor: emitted as `__force(container[i])`.
    ///   - App of a resolved primitive eq/ord/concat method (2 args): inlined
    ///     as a native Lua operator over forced operands (see the expr_ast
    ///     App arm and primitive_method_lua_op).
    ///
    /// Everything else (general calls, if/case/let IIFEs, SpecCalls) may
    /// legitimately yield a thunk and reports false.
    pub(super) fn expr_yields_whnf(&self, expr: &TExpr) -> bool {
        match &expr.kind {
            TExprKind::Lit(_) | TExprKind::Negate(_) | TExprKind::Var(_)
            | TExprKind::Con(_) | TExprKind::Tuple(_)
            | TExprKind::Lambda { .. } | TExprKind::OpFunc(_) => true,
            TExprKind::Paren(inner) => self.expr_yields_whnf(inner),
            TExprKind::InfixApp { op, .. } => Self::infix_yields_whnf(op),
            TExprKind::App(func, _) => {
                if let TExprKind::Var(name) = &func.kind
                    && self.record_accessors.contains_key(&sanitize_name(name)) {
                        return true;
                    }
                let mut n_args = 0usize;
                let mut f = expr;
                while let TExprKind::App(inner_f, _) = &f.kind {
                    n_args += 1;
                    f = inner_f.as_ref();
                }
                // Fully-applied primitive typeclass method → native operator.
                if n_args == 2
                    && matches!(&f.kind, TExprKind::Var(name)
                        if primitive_method_lua_op(name).is_some())
                {
                    return true;
                }
                // Saturated Prelude `not` → `(operand == false)`, a native
                // boolean — same gate as try_native_not_app (a locally
                // shadowed `not` is an unknown local value, no claim).
                if n_args == 1
                    && matches!(&f.kind, TExprKind::Var(name)
                        if name == "not" && !self.is_local_shadowed(name))
                {
                    return true;
                }
                // A saturated call to a scalar-HashMap runtime function
                // returns a built value, never a thunk: the bodies return
                // a fresh handle table (insert/delete/fromList), a
                // Just-cell or nil (lookup), a number (size), a boolean
                // (member), or a cons spine built with __mll_cons
                // (keys/values/toList). Suppressed when the name denotes
                // anything but the runtime function (a module definition,
                // an inline candidate, a local binding). NOT a blanket
                // runtime-prelude claim: some runtime bodies hand back raw
                // thunks by design (__mll_head returns the head unforced).
                if let TExprKind::Var(name) = &f.kind
                    && matches!(
                        (name.as_str(), n_args),
                        ("hmInsert", 3)
                            | ("hmLookup" | "hmDelete" | "hmMember", 2)
                            | ("hmSize" | "hmKeys" | "hmValues" | "hmToList"
                                | "hmFromList", 1)
                    )
                    && !self.module_fn_names.contains(name.as_str())
                    && !self.inline_fns.contains_key(name.as_str())
                    && !self.is_local_shadowed(name)
                {
                    return true;
                }
                // A direct application of a module-level function yields
                // WHNF by the runtime's WHNF-return invariant (see the
                // head-consumption contract in runtime.lua): every compiled
                // function — FFI import wrappers and their native inlinings
                // included — returns a value, never a raw thunk; a partial
                // application yields a closure, and an over-application
                // chains calls of such functions. Excluded: a locally
                // shadowed name (the callee is an unknown local value —
                // though even those, as codegen-emitted closures, uphold
                // the invariant, the claim sticks to what the name
                // provably denotes); an inline candidate (the call
                // re-emits the candidate's body, so the result is the
                // body's emission, not a function return); and any result
                // TYPE that can be a transparent newtype wrap or an action
                // (`ty_app_result_whnf`). A saturated newtype constructor
                // application is erased before codegen (newtype_erase.rs),
                // so `fromInteger n = Z5 (…)` now returns what `(…)`
                // returns; the gate stays for the residual shape — a
                // newtype-typed result that is a bare pattern variable
                // (`unwrap (Z5 x) = …; rewrap = …`) can still be the
                // wrapped value's raw thunk. Refutation builds check the
                // claim at every site (whnf_claim_checked).
                self.ty_app_result_whnf(&expr.ty)
                    && matches!(&f.kind, TExprKind::Var(name)
                        if self.module_fn_names.contains(name.as_str())
                            && !self.inline_fns.contains_key(name.as_str())
                            && !self.is_local_shadowed(name))
            }
            _ => false,
        }
    }

    /// Whether a value of this TYPE, returned by a compiled function, is
    /// guaranteed to be a WHNF representation — the type-side gate of the
    /// App arm's WHNF-return claim in `expr_yields_whnf`. A newtype is
    /// erased to its wrapped value, so a newtype-typed result can be a raw
    /// thunk; a type variable (or skolem/forall) may be instantiated at a
    /// newtype (dict-passing residuals keep them); an IO/LuaIO result flows
    /// through the runner protocol, whose pure-box payloads are exactly the
    /// values `__force` must not see early. Everything else — a function
    /// value, a cons cell, a tuple, unit, a non-newtype constructor — is
    /// WHNF as represented.
    fn ty_app_result_whnf(&self, ty: &crate::types::Ty) -> bool {
        use crate::types::Ty;
        match ty {
            Ty::Con(name) => !self.is_newtype_type(name),
            Ty::List(_) | Ty::Tuple(_) | Ty::Unit | Ty::Arrow(..) => true,
            Ty::App(head, _) => {
                let mut h = head.as_ref();
                while let Ty::App(inner, _) = h {
                    h = inner.as_ref();
                }
                matches!(h, Ty::Con(name) if !self.is_newtype_type(name))
            }
            _ => false,
        }
    }

    /// Whether the expr_ast emission for an InfixApp with this operator
    /// always yields WHNF. `div`/`mod` lower to `__mll_div`/`__mll_mod`,
    /// which return numbers. The specially-lowered operators (`$`
    /// application, `.` composition applied later, `++`/`!!`/`:` list
    /// runtime calls, `seq`'s returned second operand, `>>=`/`>>` action
    /// results) may yield an unforced value even though some are listed in
    /// is_builtin_op for cheapness, so they must be excluded explicitly.
    /// The remaining builtins emit native Lua operators over operands that
    /// forced_ast already forced, so the result is a scalar/boolean/string.
    pub(super) fn infix_yields_whnf(op: &str) -> bool {
        match op {
            "div" | "mod" | "quot" | "rem" => true,
            "$" | "." | "++" | "!!" | ":" | "seq" | ">>=" | ">>" => false,
            o => is_builtin_op(o),
        }
    }

    /// Build `expr` so the result is guaranteed WHNF, forcing it EXACTLY as
    /// often as needed: no `__force` wrapper when expr_ast's own output
    /// already yields WHNF (a concrete variable stays bare, a non-concrete
    /// variable keeps its single force, a native-operator inline stays
    /// unwrapped), and one wrapper otherwise. Used for strict-primitive
    /// operands, scrutinees, FFI arguments — every value-position that must
    /// not see a thunk.
    pub(super) fn forced_ast(&mut self, expr: &TExpr) -> Expr {
        if self.expr_yields_whnf(expr) {
            let e = self.expr_ast(expr);
            self.whnf_claim_checked(expr, e)
        } else {
            Expr::force(self.expr_ast(expr))
        }
    }

    /// TEST-ONLY (see `whnf_assert`): wrap `e` in the runtime claim checker
    /// `checker`. Production emission (`whnf_assert` unset) returns `e`
    /// untouched — the checkers must never appear in shipped output.
    pub(super) fn claim_checked(&self, checker: &str, e: Expr) -> Expr {
        if self.whnf_assert {
            Expr::call(Expr::name(checker.to_string()), vec![e])
        } else {
            e
        }
    }

    /// `claim_checked("__assert_whnf", …)` for an emission that
    /// `expr_yields_whnf` justified leaving unforced, skipping the arms whose
    /// claim needs no checking here: a variable (the Var arm of `expr_ast`
    /// asserts its own bare reads, and a non-concrete one is emitted under
    /// `__force`, which refutation mode rebinds to the checked twin), and the
    /// emissions that are Lua literals by construction (a number, a table
    /// literal, a function literal). What remains — Con, Negate, InfixApp,
    /// the App arms — is exactly the reasoning-carrying part of the mirror.
    pub(super) fn whnf_claim_checked(&self, expr: &TExpr, e: Expr) -> Expr {
        if !self.whnf_assert {
            return e;
        }
        let mut k = expr;
        while let TExprKind::Paren(inner) = &k.kind {
            k = inner.as_ref();
        }
        match &k.kind {
            TExprKind::Var(_) | TExprKind::Lit(_) | TExprKind::Tuple(_)
            | TExprKind::Lambda { .. } | TExprKind::OpFunc(_) => e,
            _ => self.claim_checked("__assert_whnf", e),
        }
    }

    /// Build an expression in Lua *prefixexp* position (a method-call
    /// receiver `<here>:m(...)` or an indexing base `<here>[i]`) so the
    /// result is guaranteed WHNF. Lua only permits a name, an index, a
    /// call, or a parenthesised expression there, so this cannot simply
    /// delegate to forced_ast: a bare literal/table/function emission would
    /// be a syntax error before `:`/`[`. A variable is safe — expr_ast
    /// emits a bare name (concrete) or a `__force(...)` call, both valid
    /// prefixexps — as is a record-accessor projection, whose emission is
    /// itself a `__force(...)` call. Everything else keeps the `__force`
    /// wrapper, whose call syntax doubles as the required prefix.
    pub(super) fn forced_prefix_ast(&mut self, expr: &TExpr) -> Expr {
        let mut e = expr;
        while let TExprKind::Paren(inner) = &e.kind {
            e = inner.as_ref();
        }
        let bare_ok = match &e.kind {
            // `otherwise` emits `true` and `[]` emits `nil` — not prefixexps.
            TExprKind::Var(name) => name != "otherwise",
            TExprKind::Con(name) => name != "[]",
            TExprKind::App(func, _) => matches!(&func.kind, TExprKind::Var(name)
                if self.record_accessors.contains_key(&sanitize_name(name))),
            _ => false,
        };
        if bare_ok {
            self.expr_ast(e)
        } else {
            Expr::force(self.expr_ast(e))
        }
    }

    /// Build a variable or nullary constructor as a raw reference WITHOUT
    /// forcing it — for lazy positions such as a cons tail, where forcing
    /// would eagerly evaluate the rest of the spine. A non-concrete variable
    /// already holds a thunk-or-value; the runtime forces it when read.
    pub(super) fn lazy_ref_ast(&mut self, expr: &TExpr) -> Expr {
        match &expr.kind {
            TExprKind::Var(name) if name == "otherwise" => Expr::lit("true"),
            // A first-class `seq` reference resolves to the runtime primitive
            // (see the expr_ast Var arm) — this is the path taken when `seq` is
            // passed as a bare argument, e.g. `foldr seq z xs`.
            TExprKind::Var(name) if name == "seq" => Expr::name("__mll_seq"),
            // First-class `div` / `mod` references resolve to their forcing
            // wrappers (see the expr_ast Var arm), e.g. `foldr div z xs`.
            TExprKind::Var(name) if name == "div" => Expr::name("__mll_div_fn"),
            TExprKind::Var(name) if name == "mod" => Expr::name("__mll_mod_fn"),
            TExprKind::Var(name) if name == "quot" => Expr::name("__mll_quot_fn"),
            TExprKind::Var(name) if name == "rem" => Expr::name("__mll_rem_fn"),
            TExprKind::Var(name) => {
                Expr::name(self.lua_ref(&sanitize_name(name)))
            }
            TExprKind::Con(name) if name == "[]" => Expr::lit("nil"),
            TExprKind::Con(name) => {
                Expr::name(self.lua_ref(&sanitize_name(name)))
            }
            _ => self.expr_ast(expr),
        }
    }

    /// Build `seq a b` inline: force `a` to WHNF, then return `b`. Shared by the
    /// prefix `seq a b` and backtick `a `seq` b` forms so the two cannot
    /// diverge. Semantically identical to the runtime `__mll_seq(a, b)` that
    /// backs every other application shape (partial application, first-class
    /// reference, over-application): both force `a` then yield `b`. The ONLY
    /// difference — and the reason this inline form is kept — is that `b` is
    /// emitted in tail position with redundant source parens stripped, so a
    /// `seq`-strict tail-recursive second operand (`go n acc = seq acc (go ..)`)
    /// stays a Lua proper tail call and runs in constant stack. `__force` is
    /// idempotent, so an already-evaluated `a` costs nothing extra; a lazy `a`
    /// (a thunk of `error`/loop) is run here, exactly as `seq` requires.
    pub(super) fn seq_inline_ast(&mut self, a: &TExpr, b: &TExpr) -> Expr {
        // Force `a` to WHNF for effect. When expr_ast's own emission already
        // yields WHNF (a variable — bare if concrete, singly forced
        // otherwise — or a native operation), evaluating it IS the force;
        // bind it to a throwaway local because a bare expression is not a
        // Lua statement. Only an emission that can yield a thunk needs the
        // explicit `__force(...)` call (which is also statement syntax).
        let first = if self.expr_yields_whnf(a) {
            let a_e = self.expr_ast(a);
            // If the claim is wrong here, the force is silently SKIPPED — a
            // missed bottom, i.e. a GHC divergence with no crash — so the
            // refutation wrapper (test-only) is the only thing that can see it.
            let a_e = self.whnf_claim_checked(a, a_e);
            Stmt::Local(vec!["_".into()], Some(a_e))
        } else {
            Stmt::Expr(Expr::force(self.expr_ast(a)))
        };
        // Strip redundant source parens around the returned expression: in Lua
        // `return f(x)` is a proper tail call but `return (f(x))` is not, so a
        // parenthesised call here would defeat TCO and blow the stack on deep
        // `seq`-strict recursion.
        let mut bb: &TExpr = b;
        while let TExprKind::Paren(inner) = &bb.kind {
            bb = inner.as_ref();
        }
        let ret = Stmt::Return(self.expr_ast(bb));
        Expr::call(
            Expr::paren(Expr::Func(vec![], FuncBody::Inline(vec![first, ret]))),
            vec![],
        )
    }

    /// Build a function-call argument, choosing eager or lazy evaluation by
    /// WEIGHING the benefit of eagerness against the risk to non-strict
    /// semantics. This is the single place that decision is made for call
    /// arguments; it replaced an earlier ad-hoc "cheap argument" heuristic.
    ///
    /// The weighing has two sides:
    ///
    ///   * LAZINESS weight — dominated by one term: if evaluating the argument
    ///     *now* could force a suspended, possibly-⊥ computation (`error`,
    ///     `undefined`, non-termination, or a trapping `div`/`mod`) that the
    ///     callee is not guaranteed to demand, the laziness weight is MAXIMAL.
    ///     Bottom always outweighs any eagerness benefit. Non-strict semantics
    ///     then *requires* the value be suspended, so it is thunked (or passed
    ///     as an already-suspended reference) and forced only if the callee
    ///     actually demands it. This is what makes
    ///     `g _ = 42 ;  g (error "boom")   ==>  42`
    ///     rather than raising "boom": `g` never forces its argument, so the
    ///     `error` thunk is never run.
    ///
    ///   * EAGERNESS weight — the saved thunk allocation (and the saved force
    ///     on use). It can only win when the laziness weight is *not* maximal,
    ///     i.e. the argument is provably total at this point: a literal, a
    ///     provably-WHNF (`concrete_vars`) variable, a constructor or tuple of
    ///     such, non-trapping arithmetic over such, etc. — exactly
    ///     `is_cheap_to_force`. Evaluating such an argument now cannot raise or
    ///     diverge where the callee would not, so eager is always the win.
    ///
    /// `strict` short-circuits the weighing: demand analysis has proven the
    /// callee forces this position on every path, so the callee would run the
    /// same ⊥ anyway — eager evaluation cannot change the observable result and
    /// the eagerness weight wins outright.
    ///
    /// Consistency with the callee: a parameter is only marked "always cheap"
    /// (callee skips `__force` and treats it as a value — see
    /// `analyze_call_sites`) when *every* call site passes an argument from the
    /// context-free floor of `is_cheap_to_force`, which is a subset of what the
    /// eager branch below accepts. So whenever the callee assumes a value, this
    /// function has indeed passed one.
    pub(super) fn arg_ast(&mut self, expr: &TExpr, strict: bool) -> Expr {
        // An argument is never the current function's result: a first-class
        // action closure emitted inside it must not inherit the deep result
        // demand (see cur_result_demand).
        let saved_result_demand =
            std::mem::replace(&mut self.cur_result_demand, crate::demand::Demand::Head);
        let e = self.arg_ast_inner(expr, strict);
        self.cur_result_demand = saved_result_demand;
        e
    }

    /// Build an argument for a SITE-FORCED callee position (see
    /// `analysis::ParamConv::SiteForced`): the callee binds the parameter
    /// bare, so this site must deliver WHNF — raw when the emission already
    /// proves it (`forced_ast`'s claim), under one `__force` otherwise.
    /// Semantically this is the entry force the callee dropped, run a few
    /// instructions earlier in the same demand: the position is forced on
    /// every path through the callee. Same result-demand hygiene as
    /// `arg_ast`.
    pub(super) fn site_forced_arg_ast(&mut self, expr: &TExpr) -> Expr {
        let saved_result_demand =
            std::mem::replace(&mut self.cur_result_demand, crate::demand::Demand::Head);
        let e = self.forced_ast(expr);
        self.cur_result_demand = saved_result_demand;
        e
    }

    fn arg_ast_inner(&mut self, expr: &TExpr, strict: bool) -> Expr {
        // Eagerness weight wins: the callee forces it anyway, or it is provably
        // total (cannot be ⊥). Evaluate in place — no thunk.
        if strict || self.is_cheap_to_force(expr) {
            // A bare variable in a STRICT position passes raw. It already
            // denotes a thunk-or-value, and strictness means the callee
            // forces this position on every path — the physical entry force
            // backing a strict parameter's concreteness claim, a G5-pinned
            // runtime-body force, or the per-use force every non-concrete
            // read emits — so a call-site `__force(v)` was a duplicate:
            // measured as the single largest cost on the IORef and
            // generics benchmarks (every force is a call plus a metatable
            // probe, and 86-100% of them found a non-thunk). The always-
            // cheap contract is untouched: register_call_site judges the
            // TIR argument (a bare Var is never context-free cheap), so a
            // parameter fed by one was never marked always-cheap even when
            // the old emission passed it forced.
            if strict {
                let mut stripped = expr;
                while let TExprKind::Paren(inner) = &stripped.kind {
                    stripped = inner.as_ref();
                }
                if matches!(&stripped.kind, TExprKind::Var(_)) {
                    return self.lazy_ref_ast(stripped);
                }
            }
            return self.expr_ast(expr);
        }
        // Laziness weight is maximal (the argument may be ⊥ and the callee may
        // not demand it): suspend it. A bare variable or nullary constructor
        // already denotes a thunk-or-value, so pass it raw rather than wrapping
        // a fresh thunk around a force of it — the runtime forces it only if
        // the callee reads it. Everything else is suspended in a thunk.
        let stripped = {
            let mut e = expr;
            while let TExprKind::Paren(inner) = &e.kind { e = inner.as_ref(); }
            e
        };
        // A tuple literal is a constructor: building it to WHNF allocates a
        // table and forces nothing, so the construction itself can never be ⊥.
        // Never wrap the whole tuple in a thunk — emit it directly and let the
        // Tuple arm weigh each field. A possibly-⊥ field is still suspended
        // (per-field arg_ast), but the always-total construction costs no extra
        // thunk, and a consumer that forces the tuple to WHNF gets the table
        // with nothing to unwrap. This keeps tuple-threaded state (the tracker's
        // hot loop) from paying a nested whole-tuple thunk allocation per frame.
        // Sound for demand analysis: demanded_vars(Tuple) is empty, so no
        // let/where binding is ever judged demanded by appearing in a tuple.
        if matches!(&stripped.kind, TExprKind::Tuple(_)) {
            return self.expr_ast(stripped);
        }
        // A SATURATED constructor application (`x : acc`, `T B a x b`) is the
        // same kind of total construction as a tuple literal: building it to
        // WHNF fills a tag and field slots and forces nothing, so the
        // construction itself can never be ⊥ and an outer thunk buys nothing
        // but an allocation. Emit it directly; the emission arms it reaches —
        // the cons arms (`__mll_cons`/`__mll_lazy_cons`) and the App
        // full-application branch (a Con head has no strictness row, so every
        // position is weighed lazily) — pass each FIELD through
        // arg_ast(field, false), so a possibly-⊥ field (a recursive tail, an
        // infinite structure) is still suspended per-field. Only the
        // redundant whole-node thunk is dropped: `ones = 1 : ones` still
        // builds one WHNF cell with a lazy tail. A PARTIAL constructor
        // application is a closure, not a construction — is_saturated_con_app
        // rejects it and it stays thunked below. Sound for demand analysis:
        // like Tuple, demanded_vars of a Con-headed application demands
        // nothing (a Con head has no strictness row either), so no let/where
        // binding is judged demanded by appearing under a constructor.
        if Self::is_saturated_con_app(stripped) {
            return self.expr_ast(stripped);
        }
        if matches!(&stripped.kind, TExprKind::Var(_) | TExprKind::Con(_)) {
            self.lazy_ref_ast(stripped)
        } else {
            Expr::thunk(self.expr_ast(expr))
        }
    }
}
