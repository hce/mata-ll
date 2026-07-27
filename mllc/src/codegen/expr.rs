//! The main expression walk: `expr_ast` / `expr_ast_inner`.
//!
//! The expression layer BUILDS `lua::Expr` trees rather than streaming text:
//! each arm constructs the node shape the printer renders back byte-for-byte
//! (explicit `Paren` placement, per-site inline vs. block function layout —
//! see lua.rs).
//!
//! `expr_ast` is the depth-guard wrapper: past `crate::MAX_NESTING_DEPTH` it
//! records a clean diagnostic (surfaced by `generate`) instead of overflowing
//! the native stack. `expr_ast_inner` is the single large match over every
//! expression form and stays one function on purpose — the arms
//! cross-reference each other and the WHNF predicates mirror them arm for
//! arm. `expr_lazy_ast` builds self-referencing definitions with lazy cons
//! tails (`__mll_lazy_cons`); `literal_ast` renders literals through the
//! canonical string quoting in names.rs.

use crate::tir::*;
use crate::types::Ty;
use super::CodeGen;
use super::lua::{Block, Expr, FuncBody, Item, Stmt};
use super::names::{is_builtin_op, lua_field_index, lua_number_literal, lua_quoted_string, primitive_method_lua_op, sanitize_name};
use super::util::{count_arrows};
use super::strictness::{bare_var_alias, strict_binding_safe};

impl CodeGen {
    /// Depth-guard wrapper around `expr_ast_inner` (the whole expression
    /// walk): checked BEFORE recursing deeper, so past the limit it records a
    /// clean diagnostic (surfaced by `generate`) and builds a placeholder
    /// instead of overflowing the native stack.
    pub(super) fn expr_ast(&mut self, expr: &TExpr) -> Expr {
        if self.expr_depth >= crate::MAX_NESTING_DEPTH {
            if self.depth_error.is_none() {
                self.depth_error = Some(format!(
                    "expression nested too deeply during code generation \
                     (limit {}): the compiler walks expressions with bounded \
                     recursion so it can report this error instead of \
                     crashing; split the expression into smaller definitions",
                    crate::MAX_NESTING_DEPTH
                ));
            }
            return Expr::lit("nil");
        }
        self.expr_depth += 1;
        let e = self.expr_ast_inner(expr);
        self.expr_depth -= 1;
        e
    }

    fn expr_ast_inner(&mut self, expr: &TExpr) -> Expr {
        match &expr.kind {
            TExprKind::Var(name) => {
                match name.as_str() {
                    "otherwise" => Expr::lit("true"),
                    // A first-class / partially-applied `seq` (e.g. `foldr seq
                    // z`, `map (seq x) ys`, `let g = seq x`) resolves to the
                    // runtime primitive, which forces its first argument and
                    // returns the second (over-application applies the returned
                    // function to the rest). The fully-applied prefix and
                    // backtick forms are lowered inline before reaching here.
                    "seq" => Expr::name("__mll_seq"),
                    // A first-class / partially-applied / prefix `div` or `mod`
                    // (`div 7 2`, `map (div 10) xs`, `foldr div z`) resolves to
                    // its forcing wrapper, which forces both arguments to WHNF
                    // then runs the strict core. Only the inline backtick
                    // `a `div` b` (InfixApp) stays on the bare strict core with
                    // pre-forced operands, keeping the arithmetic hot path free
                    // of redundant forces.
                    "div" => Expr::name("__mll_div_fn"),
                    "mod" => Expr::name("__mll_mod_fn"),
                    "quot" => Expr::name("__mll_quot_fn"),
                    "rem" => Expr::name("__mll_rem_fn"),
                    _ => {
                        let sname = sanitize_name(name);
                        let lref = self.lua_ref(&sname);
                        if self.concrete_vars.contains(&sname) {
                            Expr::name(lref)
                        } else {
                            Expr::force(Expr::name(lref))
                        }
                    }
                }
            }
            TExprKind::Con(name) => {
                match name.as_str() {
                    "[]" => Expr::lit("nil"),
                    _ => Expr::name(self.lua_ref(&sanitize_name(name))),
                }
            }
            TExprKind::Lit(lit) => Self::literal_ast(lit),
            TExprKind::App(func, arg) => {
                // Record field accessor: inline as direct table indexing.
                // The field may hold a thunk (lazy construction), so force the
                // projected value. The container is forced by expr_ast(arg) when
                // it is a non-concrete variable; __force is idempotent on values.
                // Laziness is preserved because non-strict argument positions
                // thunk-wrap the whole projection (see arg_ast).
                if let TExprKind::Var(name) = &func.kind
                    && let Some(&idx) = self.record_accessors.get(&sanitize_name(name)) {
                        // A LuaDict field is keyed by name; a plain record field
                        // by position.
                        let index = match self.luadict_field_key.get(&sanitize_name(name)) {
                            Some(key) => lua_field_index(key),
                            None => format!("[{}]", idx),
                        };
                        let container = self.expr_ast(arg);
                        return Expr::force(Expr::index(container, index));
                    }

                // Check for cons application: (:) x xs => __mll_cons(x, xs)
                if let TExprKind::App(inner_f, inner_arg) = &func.kind
                    && let TExprKind::Con(name) = &inner_f.kind
                        && name == ":" {
                            // Try to collect a literal list and emit compactly
                            if let Some(elems) = Self::collect_list_literal(expr) {
                                let mut stmts = vec![Stmt::Local(
                                    vec!["_l".into()],
                                    Some(Expr::lit("nil")),
                                )];
                                for elem in elems.iter().rev() {
                                    // A cons head is a lazy position: `:` forces
                                    // neither side. Weigh it like any argument so
                                    // a possibly-⊥ element is suspended rather
                                    // than run when the cell is built.
                                    // Value-consumers force the head on read; see
                                    // the head-consumption contract on __mll_head.
                                    let head = self.arg_ast(elem, false);
                                    stmts.push(Stmt::Assign(
                                        "_l".into(),
                                        Expr::call_named("__mll_cons", vec![head, Expr::name("_l")]),
                                    ));
                                }
                                stmts.push(Stmt::Return(Expr::name("_l")));
                                return Expr::call(
                                    Expr::paren(Expr::Func(vec![], FuncBody::Inline(stmts))),
                                    vec![],
                                );
                            }
                            // Keep the cons tail lazy. A bare reference — a
                            // variable or a nullary constructor like [] —
                            // already denotes a thunk-or-value, so emit it raw:
                            // forcing it here (expr_ast forces non-concrete
                            // vars) would evaluate the rest of the spine eagerly
                            // and diverge on infinite or self-referential lists
                            // (e.g. `cons x rest = x : rest`). Any tail that
                            // requires computation is wrapped in a thunk. The
                            // runtime forces the cell when read (__mll_head /
                            // __mll_tail), so an unforced tail is safe to store.
                            let tail = {
                                let mut t = arg.as_ref();
                                while let TExprKind::Paren(inner) = &t.kind { t = inner.as_ref(); }
                                t
                            };
                            let tail_is_ref = matches!(&tail.kind,
                                TExprKind::Var(_) | TExprKind::Con(_));
                            if tail_is_ref {
                                let head = self.arg_ast(inner_arg, false); // lazy head — see below
                                let tail_e = self.lazy_ref_ast(tail);
                                return Expr::call_named("__mll_cons", vec![head, tail_e]);
                            } else {
                                // Lazy head: suspend a possibly-⊥ element instead
                                // of running it when the cell is built.
                                let head = self.arg_ast(inner_arg, false);
                                let tail_e = self.expr_ast(arg);
                                return Expr::call_named(
                                    "__mll_lazy_cons",
                                    vec![head, Expr::inline_fn0(tail_e)],
                                );
                            }
                        }

                // seq a b (prefix, EXACTLY two args) => force a, return b,
                // inline. `seq` applied to more than two args (its result is a
                // function applied further), a partial `seq a`, a backtick
                // `a `seq` b`, and a first-class `seq` all route through the
                // runtime `__mll_seq` instead (see the Var arm and the InfixApp
                // seq case) — this inline form is kept only for the common
                // prefix shape because it preserves the proper tail call on `b`.
                if let TExprKind::App(seq_f, seq_a) = &func.kind
                    && let TExprKind::Var(name) = &seq_f.kind
                        && name == "seq" {
                            return self.seq_inline_ast(seq_a, arg);
                        }

                // return/pure wrap their argument in an IO action closure whose
                // performed value is the argument, left UNFORCED per the
                // eagerness contract: running `return ⊥` must not raise until
                // something demands the value. arg_ast(strict=false) suspends a
                // possibly-⊥ argument in a thunk and keeps a provably-total one
                // eager, so `return 0` still yields a bare `0` while
                // `return (error "x")` yields an inert thunk. This is the
                // first-class / higher-order path (e.g. `fmap f (return e)`,
                // `mapM (\x -> return (g x)) xs`); the do-block bind chain
                // flattens its own returns through action_run_ast, which suspends
                // the same way.
                if let TExprKind::Var(name) = &func.kind
                    && (name == "return" || name == "pure") {
                        let payload = self.arg_ast(arg, false);
                        return Expr::paren(Expr::inline_fn0(payload));
                    }

                // Collect all applied arguments
                let mut args = vec![arg.as_ref()];
                let mut f = func.as_ref();
                while let TExprKind::App(inner_f, inner_arg) = &f.kind {
                    args.push(inner_arg.as_ref());
                    f = inner_f.as_ref();
                }
                args.reverse();

                // try/catch: wrap IO action argument in a closure so that
                // errors are deferred into pcall rather than crashing eagerly.
                if let TExprKind::Var(name) = &f.kind {
                    if name == "try" && args.len() == 1 {
                        let action = self.action_run_ast(args[0], false);
                        return Expr::call_named("try_", vec![Expr::inline_fn0(action)]);
                    }
                    if name == "catch" && args.len() == 2 {
                        let action = self.action_run_ast(args[0], false);
                        let handler = self.expr_ast(args[1]);
                        return Expr::call_named(
                            "catch_",
                            vec![Expr::inline_fn0(action), handler],
                        );
                    }
                }

                // Typeclass methods on primitive types → inline as Lua operators
                // (primitive_method_lua_op is also what expr_yields_whnf
                // keys on to know this emission is a forced native operation).
                if args.len() == 2
                    && let TExprKind::Var(name) = &f.kind {
                        if let Some(op) = primitive_method_lua_op(name) {
                            let l = self.forced_ast(args[0]);
                            let r = self.forced_ast(args[1]);
                            return Expr::paren(Expr::binop(op, l, r));
                        }
                    }


                // Inline small pure functions at call site. Substitution
                // re-emits each argument at every occurrence of its
                // parameter, so an argument whose evaluation costs anything
                // is admitted only where the parameter is emitted at most
                // once (occ_counts, see find_inline_candidates) — otherwise
                // `sq x = x * x` at `sq (nfib 30)` would run the call twice,
                // a sharing loss GHC's inliner never allows. A declined
                // site falls through to the ordinary call below, which
                // evaluates (or thunks) the argument exactly once.
                if let TExprKind::Var(name) = &f.kind
                    && let Some((params, body, occ_counts)) = self.inline_fns.get(name)
                        && args.len() == params.len()
                        && args.iter().zip(occ_counts.iter())
                            .all(|(a, &n)| n <= 1 || Self::is_trivial_arg(a)) {
                            let (params, body) = (params.clone(), body.clone());
                            let mut subst = std::collections::HashMap::new();
                            for (param, arg) in params.iter().zip(args.iter()) {
                                subst.insert(param.clone(), *arg);
                            }
                            let inlined = self.expr_subst_ast(&body, &subst);
                            return Expr::paren(inlined);
                        }

                // Look up callee's demand info for call-site strictness
                // decisions. A where-bound local function shadows a
                // same-named top-level one, so its scoped row wins.
                let callee_strict = if let TExprKind::Var(name) = &f.kind {
                    self.local_strict_params.get(name)
                        .or_else(|| self.demand_info.strict_params.get(name))
                        .cloned()
                } else {
                    None
                };

                // The function argument of a runtime generic (map/zipWith) may
                // need a currying adapter — see runtime_generic_adapter.
                let arg0_adapter = args.first()
                    .and_then(|a| self.runtime_generic_adapter(f, 0, &a.ty));

                // Check if this is a partial application:
                // the result type is still a function type
                let remaining = count_arrows(&expr.ty);
                if remaining > 0 {
                    // Partial application — generate a closure
                    // Wrapped in () so it can be immediately called in Lua
                    let extra_params: Vec<String> = (0..remaining)
                        .map(|i| format!("_pa{}", i))
                        .collect();
                    let callee = self.callee_ast(f);
                    let mut cargs = Vec::new();
                    for (i, a) in args.iter().enumerate() {
                        let is_strict = callee_strict.as_ref()
                            .is_some_and(|v| v.get(i).copied().unwrap_or(false));
                        let arg_e = self.arg_ast(a, is_strict);
                        if i == 0 && let Some(adapter) = arg0_adapter {
                            cargs.push(Expr::call_named(adapter, vec![arg_e]));
                        } else {
                            cargs.push(arg_e);
                        }
                    }
                    for p in &extra_params {
                        cargs.push(Expr::name(p.clone()));
                    }
                    Expr::paren(Expr::Func(
                        extra_params,
                        FuncBody::Block(Block(vec![Stmt::Return(Expr::call(callee, cargs))])),
                    ))
                } else {
                    // Full application
                    // Wrap function literals in parens so Lua allows calling them
                    let callee = self.callee_ast(f);
                    let mut cargs = Vec::new();
                    for (i, a) in args.iter().enumerate() {
                        let is_strict = callee_strict.as_ref()
                            .is_some_and(|v| v.get(i).copied().unwrap_or(false));
                        let arg_e = self.arg_ast(a, is_strict);
                        if i == 0 && let Some(adapter) = arg0_adapter {
                            cargs.push(Expr::call_named(adapter, vec![arg_e]));
                        } else {
                            cargs.push(arg_e);
                        }
                    }
                    Expr::call(callee, cargs)
                }
            }
            TExprKind::InfixApp { op, lhs, rhs } => {
                if op == "div" || op == "mod" || op == "quot" || op == "rem" {
                    // Runtime helpers, not inline float math / bare `%`:
                    // math.floor(a/0) yields inf (a float escaping into
                    // Int) instead of raising, and float division is
                    // inexact past 2^53. __mll_div/__mll_mod raise a clear
                    // error on a zero divisor and use native integer floor
                    // division (Lua 5.3+ `//`) when the host has it. quot/rem
                    // truncate toward zero (remainder takes the dividend's sign).
                    let helper = match op.as_str() {
                        "div" => "__mll_div", "mod" => "__mll_mod",
                        "quot" => "__mll_quot", _ => "__mll_rem",
                    };
                    let l = self.forced_ast(lhs);
                    let r = self.forced_ast(rhs);
                    return Expr::call_named(helper, vec![l, r]);
                }
                if op == "++" {
                    let l = self.expr_ast(lhs);
                    let r = self.expr_ast(rhs);
                    return Expr::call_named(
                        "__mll_list_append",
                        vec![l, Expr::inline_fn0(r)],
                    );
                }
                if op == "!!" {
                    let l = self.expr_ast(lhs);
                    let r = self.forced_ast(rhs);
                    return Expr::call_named("__mll_list_index", vec![l, r]);
                }
                if op == "seq" {
                    // Backtick `a `seq` b`: same inline lowering as prefix
                    // `seq a b` (force a, return b in tail position). Without
                    // this the operator fell to the user-operator branch below
                    // and emitted `seq(a, b)` — a call to a nonexistent global.
                    return self.seq_inline_ast(lhs, rhs);
                }
                let lua_op = match op.as_str() {
                    "<>" => "..", "&&" => "and", "||" => "or", "/=" => "~=",
                    // "div"/"mod" never reach here: handled above via
                    // __mll_div/__mll_mod (zero-divisor check, exact // ).
                    ":" => {
                        // Keep the cons tail lazy. A bare reference (variable
                        // or []) already denotes a thunk-or-value, so emit it
                        // raw — forcing it would evaluate the rest of the spine
                        // eagerly and diverge on infinite/self-referential
                        // lists (e.g. `cons x rest = x : rest`). Any tail that
                        // requires computation is wrapped in a thunk; the
                        // runtime forces the cell when read. See lazy_ref_ast.
                        let tail = {
                            let mut t = rhs.as_ref();
                            while let TExprKind::Paren(inner) = &t.kind { t = inner.as_ref(); }
                            t
                        };
                        let tail_is_ref = matches!(&tail.kind,
                            TExprKind::Var(_) | TExprKind::Con(_));
                        // The head is a lazy position too; weigh it so a
                        // possibly-⊥ head is suspended, not run at construction.
                        if tail_is_ref {
                            let head = self.arg_ast(lhs, false);
                            let tail_e = self.lazy_ref_ast(tail);
                            return Expr::call_named("__mll_cons", vec![head, tail_e]);
                        } else {
                            let head = self.arg_ast(lhs, false);
                            let tail_e = self.expr_ast(rhs);
                            return Expr::call_named(
                                "__mll_lazy_cons",
                                vec![head, Expr::inline_fn0(tail_e)],
                            );
                        }
                    }
                    "$" => {
                        // f $ x is exactly f x, so x is weighed like a normal
                        // application argument (arg_ast): eager when f's next
                        // parameter position is strict or x is cheap/total,
                        // suspended otherwise. When the result type still has
                        // arrows, f's real Lua arity is 1 + remaining under
                        // the N-ary convention, so close over the missing
                        // arguments — exactly like the App arm's
                        // partial-application closure. Calling f with the one
                        // argument alone would leave its remaining parameters
                        // nil.
                        let remaining = count_arrows(&expr.ty);
                        // `map $ f` puts f straight into a runtime generic's
                        // function-parameter position (see
                        // runtime_generic_adapter).
                        let adapter = self.runtime_generic_adapter(lhs, 0, &rhs.ty);
                        // x occupies f's NEXT argument position: f is often a
                        // partial application (`(const 5) $ undefined` is
                        // `const 5 undefined`), so strip the applied spine off
                        // lhs and consult the head's strictness row at the
                        // index PAST the already-applied arguments — the same
                        // row/index the App arm would use for `f x`. Anything
                        // short of a known head with a strict row at that
                        // exact position stays lazy: over-forcing here would
                        // run a ⊥ that f never demands.
                        let rhs_strict = {
                            let mut head = lhs.as_ref();
                            let mut applied = 0usize;
                            loop {
                                match &head.kind {
                                    TExprKind::Paren(inner) => head = inner.as_ref(),
                                    TExprKind::App(f, _) => {
                                        applied += 1;
                                        head = f.as_ref();
                                    }
                                    _ => break,
                                }
                            }
                            matches!(&head.kind, TExprKind::Var(n)
                                if self.local_strict_params.get(n)
                                    .or_else(|| self.demand_info.strict_params.get(n))
                                    .and_then(|v| v.get(applied).copied())
                                    .unwrap_or(false))
                        };
                        let callee = self.callee_ast(lhs);
                        let arg = self.arg_ast(rhs, rhs_strict);
                        let arg = match adapter {
                            Some(a) => Expr::call_named(a, vec![arg]),
                            None => arg,
                        };
                        if remaining > 0 {
                            let extra: Vec<String> =
                                (0..remaining).map(|i| format!("_pa{}", i)).collect();
                            let mut cargs = vec![arg];
                            for p in &extra {
                                cargs.push(Expr::name(p.clone()));
                            }
                            return Expr::paren(Expr::Func(
                                extra,
                                FuncBody::Inline(vec![Stmt::Return(Expr::call(callee, cargs))]),
                            ));
                        } else {
                            return Expr::call(callee, vec![arg]);
                        }
                    }
                    ">>=" => {
                        // IO actions: do-blocks produce function() closures.
                        // Bind chain flattens into sequential statements inside
                        // the action closure; each sub-action is called with ().
                        // NOTE on cur_result_demand: this arm is reached either
                        // for a clause body in result position (the guarded /
                        // multi-clause emission path wraps the ST body here) —
                        // where the ambient result demand is exactly the demand
                        // on the action's yielded value — or for a first-class
                        // action, whose enclosing context (arg_ast, statement
                        // actions, value-binding RHSes, lambdas) has already
                        // reset the ambient demand to Head. So it is used as-is.
                        if let TExprKind::Lambda { .. } = &rhs.kind {
                            let b = self.bind_chain_block(expr, true);
                            return Expr::Func(vec![], FuncBody::Block(b));
                        } else {
                            // m >>= f (non-lambda RHS, e.g. `step 1 >>= print`):
                            // under the calling convention, applying an IO-typed
                            // function to its argument PERFORMS the action and
                            // returns the result carrying at most one pending
                            // pure box (see the __mll_run contract in the
                            // runtime). So the continuation's application must
                            // flow through the FORWARDING runner, which returns
                            // a plain result as-is, forwards a pure box, and
                            // calls a first-class action closure. Calling the
                            // application result unconditionally — the previous
                            // emission — crashed ("attempt to call a nil
                            // value") whenever `f x` returned a plain value,
                            // which is the normal case.
                            let f_e = if self.expr_yields_whnf(rhs) {
                                self.callee_ast(rhs)
                            } else {
                                // A thunk-valued continuation (a lazily bound
                                // local) must be forced to a callable first.
                                Expr::force(self.expr_ast(rhs))
                            };
                            let m_e = self.action_run_ast(lhs, false);
                            return Expr::inline_fn0(Expr::call_named(
                                "__mll_run_tail",
                                vec![Expr::call(f_e, vec![m_e])],
                            ));
                        }
                    }
                    ">>" => {
                        // IO-then: produce action closure (see the ">>=" arm
                        // for the cur_result_demand rationale).
                        let b = self.bind_chain_block(expr, true);
                        return Expr::Func(vec![], FuncBody::Block(b));
                    }
                    "." => {
                        // f . g as a value. Under the N-ary convention the
                        // closure must take ALL count_arrows(expr.ty)
                        // parameters — `(f . g) x y` is one flat 2-arg call —
                        // and the extras beyond the first belong to f (whose
                        // arity is 1 + extras, since its argument type is g's
                        // result). Likewise, g itself may have arity > 1 when
                        // it returns a function; the composition feeds it only
                        // one argument, so wrap `g _x` in the same
                        // partial-application closure the App arm would emit.
                        let extras = count_arrows(&expr.ty).saturating_sub(1);
                        let g_extras = count_arrows(&rhs.ty).saturating_sub(1);
                        // `map . g` feeds g's result straight into a runtime
                        // generic's function-parameter position (see
                        // runtime_generic_adapter).
                        let adapter = match &rhs.ty {
                            Ty::Arrow(_, res, _) => self.runtime_generic_adapter(lhs, 0, res),
                            _ => None,
                        };
                        // `(f . g) x` is `f (g x)`. A non-strict `f` must not
                        // force `g x` — doing so would force `x` and run any
                        // bottom in `g x` that `f` never demands (e.g.
                        // `(ignore . add1) (error "boom")` must return, not
                        // raise). Suspend the inner application in that case.
                        // Only the actual call form (`g_extras == 0`, no runtime
                        // adapter) can be bottom; a partial application yields a
                        // closure value, and a runtime generic (map/zipWith)
                        // forces its function argument itself, so both are safe
                        // to pass eagerly.
                        let f_strict = matches!(&lhs.kind, TExprKind::Var(n)
                            if self.local_strict_params.get(n)
                                .or_else(|| self.demand_info.strict_params.get(n))
                                .and_then(|v| v.first().copied()).unwrap_or(false));
                        let suspend = !f_strict && g_extras == 0 && adapter.is_none();
                        let f_callee = self.callee_ast(lhs);
                        let inner = if g_extras == 0 {
                            let g_callee = self.callee_ast(rhs);
                            Expr::call(g_callee, vec![Expr::name("_x")])
                        } else {
                            let pb_params: Vec<String> =
                                (0..g_extras).map(|i| format!("_pb{}", i)).collect();
                            let g_callee = self.callee_ast(rhs);
                            let mut gargs = vec![Expr::name("_x")];
                            for p in &pb_params {
                                gargs.push(Expr::name(p.clone()));
                            }
                            Expr::paren(Expr::Func(
                                pb_params,
                                FuncBody::Inline(vec![Stmt::Return(Expr::call(g_callee, gargs))]),
                            ))
                        };
                        let inner = match adapter {
                            Some(a) => Expr::call_named(a, vec![inner]),
                            None => inner,
                        };
                        let inner = if suspend { Expr::thunk(inner) } else { inner };
                        let mut outer_params = vec!["_x".to_string()];
                        let mut fargs = vec![inner];
                        for i in 0..extras {
                            outer_params.push(format!("_pa{}", i));
                            fargs.push(Expr::name(format!("_pa{}", i)));
                        }
                        return Expr::paren(Expr::Func(
                            outer_params,
                            FuncBody::Inline(vec![Stmt::Return(Expr::call(f_callee, fargs))]),
                        ));
                    }
                    other => other,
                };
                if is_builtin_op(op) {
                    // Lua-native operator: emit as infix. Operands are forced —
                    // a thunk is a table, which would corrupt arithmetic and
                    // comparison, and is truthy under `and`/`or`.
                    let l = self.forced_ast(lhs);
                    let r = self.forced_ast(rhs);
                    Expr::paren(Expr::binop(lua_op, l, r))
                } else {
                    // User-defined or non-Lua operator: emit as function call
                    let sop = sanitize_name(op);
                    let fref = self.lua_ref(&sop);
                    let l = self.expr_ast(lhs);
                    let r = self.expr_ast(rhs);
                    Expr::call_named(&fref, vec![l, r])
                }
            }
            TExprKind::Negate(inner) => Expr::paren(Expr::neg(self.expr_ast(inner))),
            TExprKind::If { cond, then_branch, else_branch } => {
                let cond_e = self.expr_ast(cond);
                let then_s = Stmt::Return(self.tail_ast(then_branch, false));
                let else_s = Stmt::Return(self.tail_ast(else_branch, false));
                Expr::call(
                    Expr::paren(Expr::Func(
                        vec![],
                        FuncBody::Block(Block(vec![Stmt::If {
                            cond: cond_e,
                            then_b: Block(vec![then_s]),
                            elseifs: vec![],
                            else_b: Some(Block(vec![else_s])),
                        }])),
                    )),
                    vec![],
                )
            }
            TExprKind::Case { scrutinee, branches } if branches.iter().any(|b| !b.guards.is_empty()) => {
                // Guarded branches: lower to clause-based matching (via the
                // shared pattern-match emitter) so a branch whose pattern
                // matches but whose guards all fail falls through to the next
                // branch, exactly like function-clause guards.
                let saved_locals = self.local_vars.clone();
                let saved_concrete = self.concrete_vars.clone();
                let mut stmts = Vec::new();
                // Entry force, skipped when the argument emission below
                // (expr_ast at the call parens) already yields WHNF.
                if !self.expr_yields_whnf(scrutinee) {
                    stmts.push(Stmt::Assign("_cg".into(), Expr::force(Expr::name("_cg"))));
                }
                self.local_vars.insert("_cg".to_string());
                self.concrete_vars.insert("_cg".to_string());
                let clauses: Vec<TClause> = branches.iter().map(|b| TClause {
                    span: None,
                    patterns: vec![b.pattern.clone()],
                    guards: b.guards.clone(),
                    body: b.body.clone(),
                    where_binds: vec![],
                }).collect();
                let b = self.pattern_match_block(&["_cg".to_string()], &clauses);
                stmts.extend(b.0);
                self.local_vars = saved_locals;
                self.concrete_vars = saved_concrete;
                let scrut = self.expr_ast(scrutinee);
                Expr::call(
                    Expr::paren(Expr::Func(
                        vec!["_cg".into()],
                        FuncBody::Block(Block(stmts)),
                    )),
                    vec![scrut],
                )
            }
            TExprKind::Case { scrutinee, branches } => {
                let scrut_stmt = Stmt::Local(vec!["_s".into()], Some(self.forced_ast(scrutinee)));
                // Assemble the branch loop's if/elseif chain structurally:
                // the first conditioned branch opens the `if`, later ones are
                // `elseif`s, an unconditioned branch past the first becomes the
                // `else` and ends the chain (later branches are unreachable),
                // and an unconditioned FIRST branch needs no `if` at all.
                let mut chain: Option<(Expr, Block)> = None;
                let mut elseifs: Vec<(Expr, Block)> = Vec::new();
                let mut else_b: Option<Block> = None;
                let mut direct: Vec<Stmt> = Vec::new();
                for (i, branch) in branches.iter().enumerate() {
                    let mut conditions = Vec::new();
                    let mut bindings = Vec::new();
                    self.collect_pattern_conditions(&Expr::name("_s"), &branch.pattern, &mut conditions, &mut bindings);
                    // Register pattern-bound names as locals (scoped to this
                    // branch) so references resolve to them rather than a
                    // same-named top-level/prelude function.
                    let saved_locals = self.local_vars.clone();
                    if conditions.is_empty() {
                        if i > 0 {
                            let mut bs = Vec::new();
                            for (var, val) in &bindings {
                                bs.push(Stmt::Local(vec![var.clone()], Some(val.clone())));
                                self.local_vars.insert(var.clone());
                            }
                            bs.push(Stmt::Return(self.tail_ast(&branch.body, false)));
                            else_b = Some(Block(bs));
                        } else {
                            for (var, val) in &bindings {
                                direct.push(Stmt::Local(vec![var.clone()], Some(val.clone())));
                                self.local_vars.insert(var.clone());
                            }
                            direct.push(Stmt::Return(self.tail_ast(&branch.body, false)));
                        }
                        self.local_vars = saved_locals;
                        break;
                    }
                    let cond = Expr::and_chain(conditions);
                    let mut bs = Vec::new();
                    for (var, val) in &bindings {
                        bs.push(Stmt::Local(vec![var.clone()], Some(val.clone())));
                        self.local_vars.insert(var.clone());
                    }
                    bs.push(Stmt::Return(self.tail_ast(&branch.body, false)));
                    if chain.is_none() {
                        chain = Some((cond, Block(bs)));
                    } else {
                        elseifs.push((cond, Block(bs)));
                    }
                    self.local_vars = saved_locals;
                }
                let mut stmts = vec![scrut_stmt];
                match chain {
                    Some((cond, then_b)) => {
                        stmts.push(Stmt::If { cond, then_b, elseifs, else_b });
                    }
                    None => stmts.extend(direct),
                }
                Expr::call(
                    Expr::paren(Expr::Func(vec![], FuncBody::Block(Block(stmts)))),
                    vec![],
                )
            }
            TExprKind::Let { binds, body } => {
                let saved_locals = self.local_vars.clone();
                let saved_concrete = self.concrete_vars.clone();
                let mut stmts = Vec::new();
                // Forward-declare all names before assigning, so let bindings
                // can be self- and mutually recursive. Lua locals are not in
                // scope within their own initializer, so `local x = ...x...`
                // would bind the inner `x` to an outer/global. See
                // where_binds_stmts for the same rationale.
                {
                    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                    let names: Vec<String> = binds.iter()
                        .map(|b| sanitize_name(&b.name))
                        .filter(|n| seen.insert(n.clone()))
                        .collect();
                    if !names.is_empty() {
                        stmts.push(Stmt::Local(names.clone(), None));
                    }
                    // Register the names as locals so references in the bodies
                    // resolve to these bindings, not a same-named top-level or
                    // prelude function (e.g. a let-bound `sum` or `last`).
                    for n in &names { self.local_vars.insert(n.clone()); }
                }
                // Bindings demanded by the let body may be evaluated eagerly
                // even when they read suspended values (see demanded_bindings).
                // Value position: this let's result is not the function's
                // result, so the demand on it is plain WHNF.
                let demanded = self.demanded_bindings(
                    binds,
                    crate::demand::demanded_map(
                        body,
                        &self.demand_info.rows,
                        &self.local_demand_rows,
                        &|n| self.inline_fns.contains_key(n),
                        &crate::demand::Demand::Head,
                    ),
                );
                for (i, bind) in binds.iter().enumerate() {
                    let sname = sanitize_name(&bind.name);
                    if self.strict_binding_ok(bind, &demanded) && strict_binding_safe(binds, i) {
                        let rhs = self.expr_ast(&bind.body);
                        stmts.push(Stmt::Assign(sname.clone(), rhs));
                        self.concrete_vars.insert(sname);
                    } else {
                        // Thunked: must not stay marked concrete (a same-named
                        // outer binding may have been).
                        self.concrete_vars.remove(&sname);
                        if let Some(v) = bare_var_alias(binds, i) {
                            // Bare-variable RHS: share the existing
                            // thunk-or-value (see bare_var_alias).
                            let rhs = self.lazy_ref_ast(v);
                            stmts.push(Stmt::Assign(sname, rhs));
                        } else {
                            let rhs = self.expr_ast(&bind.body);
                            stmts.push(Stmt::Assign(sname, Expr::thunk(rhs)));
                        }
                    }
                }
                stmts.push(Stmt::Return(self.tail_ast(body, false)));
                let out = Expr::call(
                    Expr::paren(Expr::Func(vec![], FuncBody::Block(Block(stmts)))),
                    vec![],
                );
                self.local_vars = saved_locals;
                self.concrete_vars = saved_concrete;
                out
            }
            TExprKind::Lambda { params, body } => {
                // Flatten directly nested lambdas into one Lua function and
                // eta-pad up to the type's full arrow count, so the emitted
                // function's arity matches what every call site assumes (see
                // flatten_lambda for the calling-convention rationale).
                let (orig, inner_body) = Self::flatten_lambda(params, body);
                let ps = Self::lambda_param_names(&orig);
                let eta_count = count_arrows(&expr.ty).saturating_sub(ps.len());
                let eta_params: Vec<String> =
                    (0..eta_count).map(|i| format!("_eta{}", i)).collect();
                let saved_locals = self.local_vars.clone();
                let saved_concrete = self.concrete_vars.clone();
                // A first-class lambda's result is not the enclosing
                // function's result — no deep result demand inside.
                let saved_result_demand =
                    std::mem::replace(&mut self.cur_result_demand, crate::demand::Demand::Head);
                // A lambda parameter is NOT guaranteed forced: when the lambda
                // is invoked through a higher-order position the caller cannot
                // see its strictness and may pass a thunk. Drop the params from
                // concrete_vars (a same-named outer binding may be concrete) so
                // their uses in the body are forced rather than emitted bare.
                for p in &ps {
                    self.local_vars.insert(p.clone());
                    self.concrete_vars.remove(p);
                }
                let mut all_params = ps.clone();
                all_params.extend(eta_params.iter().cloned());
                let ret = if eta_count > 0 {
                    // The body still has function type (e.g. `\x -> f x` at
                    // type a -> b -> c): apply the eta params to its value,
                    // mirroring the top-level eta-expansion in function_stmts.
                    // The callee must be WHNF; when the emission already
                    // yields that, callee_ast only adds the parens a bare fn
                    // literal needs to be called.
                    let callee = if self.expr_yields_whnf(inner_body) {
                        self.callee_ast(inner_body)
                    } else {
                        Expr::force(self.expr_ast(inner_body))
                    };
                    Expr::call(
                        callee,
                        eta_params.iter().map(|p| Expr::name(p.clone())).collect(),
                    )
                } else {
                    // Lambda body is in tail position — strip parens for PTC.
                    self.tail_ast(inner_body, false)
                };
                let out = Expr::Func(all_params, FuncBody::Block(Block(vec![Stmt::Return(ret)])));
                self.local_vars = saved_locals;
                self.concrete_vars = saved_concrete;
                self.cur_result_demand = saved_result_demand;
                out
            }
            TExprKind::Paren(inner) => Expr::paren(self.expr_ast(inner)),
            TExprKind::OpFunc(op) => {
                if op == "++" {
                    return Expr::Func(
                        vec!["_a".into(), "_b".into()],
                        FuncBody::Inline(vec![Stmt::Return(Expr::call_named(
                            "__mll_list_append",
                            vec![Expr::name("_a"), Expr::inline_fn0(Expr::name("_b"))],
                        ))]),
                    );
                }
                if op == "!!" {
                    return Expr::Func(
                        vec!["_a".into(), "_b".into()],
                        FuncBody::Inline(vec![Stmt::Return(Expr::call_named(
                            "__mll_list_index",
                            vec![Expr::name("_a"), Expr::force(Expr::name("_b"))],
                        ))]),
                    );
                }
                if op == ":" {
                    return Expr::Func(
                        vec!["_a".into(), "_b".into()],
                        FuncBody::Inline(vec![Stmt::Return(Expr::call_named(
                            "__mll_cons",
                            vec![Expr::name("_a"), Expr::name("_b")],
                        ))]),
                    );
                }
                if op == ">>=" {
                    // First-class (>>=) at an action monad (IO/LuaIO/ST —
                    // Maybe/[] resolve to bind_Maybe/bind_List in mono).
                    // Applying it must BUILD an action value, not perform
                    // it: return a deferred closure that, when run, runs
                    // the LHS action and forwards the continuation's
                    // application through the tail runner — the same shape
                    // the inline `m >>= f` non-lambda arm emits. The old
                    // fallback emitted `_a >>= _b` verbatim: a Lua syntax
                    // error.
                    return Expr::Func(
                        vec!["_a".into(), "_b".into()],
                        FuncBody::Inline(vec![Stmt::Return(Expr::inline_fn0(
                            Expr::call_named(
                                "__mll_run_tail",
                                vec![Expr::call(
                                    Expr::force(Expr::name("_b")),
                                    vec![Expr::call_named(
                                        "__mll_run",
                                        vec![Expr::name("_a")],
                                    )],
                                )],
                            ),
                        ))]),
                    );
                }
                if op == ">>" {
                    // First-class (>>) at an action monad: build a deferred
                    // closure that performs the LHS for its effects (result
                    // discarded, so the forwarding runner suffices) and
                    // tail-runs the RHS. See the (>>=) arm above.
                    return Expr::Func(
                        vec!["_a".into(), "_b".into()],
                        FuncBody::Inline(vec![Stmt::Return(Expr::Func(
                            vec![],
                            FuncBody::Inline(vec![
                                Stmt::Expr(Expr::call_named(
                                    "__mll_run_tail",
                                    vec![Expr::name("_a")],
                                )),
                                Stmt::Return(Expr::call_named(
                                    "__mll_run_tail",
                                    vec![Expr::name("_b")],
                                )),
                            ]),
                        ))]),
                    );
                }
                let lua_op = match op.as_str() {
                    "<>" => "..", "&&" => "and", "||" => "or", "/=" => "~=",
                    other => other,
                };
                Expr::Func(
                    vec!["_a".into(), "_b".into()],
                    FuncBody::Inline(vec![Stmt::Return(Expr::binop(
                        lua_op,
                        Expr::force(Expr::name("_a")),
                        Expr::force(Expr::name("_b")),
                    ))]),
                )
            }
            TExprKind::SpecCall { specialized, args, .. } => self.spec_call_ast(specialized, args, expr),
            TExprKind::Tuple(elems) => {
                let mut items = Vec::new();
                for e in elems {
                    // A tuple field is a lazy position: `(,)` forces neither
                    // side, exactly like a cons head. Weigh it like any
                    // argument so a possibly-⊥ field is suspended rather than
                    // run when the tuple is built (fst (1, error) == 1).
                    // Value-consumers force the field on read: pattern
                    // destructuring via field_path, the __mll_tuple_eq /
                    // __mll_tup_get specializations hand the raw field to an
                    // eq/show function that forces, and the FFI boundary
                    // deep-forces through __mll_arg_marshal.
                    items.push(Item::Pos(self.arg_ast(e, false)));
                }
                Expr::Table(items)
            }
            TExprKind::DictAccess { dict_param, method_name } => {
                Expr::name(format!("{}.{}", sanitize_name(dict_param), sanitize_name(method_name)))
            }
            TExprKind::DictMethod { dict, method_name } => {
                // A method of a CONSTRUCTED dictionary (e.g. the `[a]`
                // dictionary built from the element dictionary). Parenthesized
                // because the dictionary may be a table literal, which Lua
                // cannot index directly.
                let d = self.expr_ast(dict);
                Expr::index(Expr::paren(d), format!(".{}", sanitize_name(method_name)))
            }
            TExprKind::DictCall { func_name, dict_args, value_args } => {
                let sfn = sanitize_name(func_name);
                let fref = self.lua_ref(&sfn);
                let mut cargs = Vec::new();
                for d in dict_args {
                    cargs.push(self.expr_ast(d));
                }
                for v in value_args {
                    cargs.push(self.expr_ast(v));
                }
                Expr::call_named(&fref, cargs)
            }
            TExprKind::RecordUpdate { record, updates, num_fields } => {
                // A LuaDict record is keyed by name, so we can't copy it
                // positionally: shallow-copy every key with `pairs`, then
                // overwrite the updated fields by name.
                let is_luadict = updates.first()
                    .map(|(fname, _, _)| self.luadict_field_key.contains_key(&sanitize_name(fname)))
                    .unwrap_or(false);
                if is_luadict {
                    let mut stmts = vec![
                        Stmt::Local(vec!["_r".into()], Some(self.forced_ast(record))),
                        Stmt::Local(vec!["_u".into()], Some(Expr::Table(vec![]))),
                        Stmt::Raw("for _k, _v in pairs(_r) do _u[_k] = _v end".into()),
                    ];
                    for (fname, _, val) in updates {
                        // Resolve the Haskell field name to its effective Lua
                        // key (`as "key"` rename) — the copied table is keyed
                        // by effective keys, so writing the raw name would add
                        // a stray key instead of updating the field.
                        let key = self.luadict_field_key
                            .get(&sanitize_name(fname))
                            .cloned()
                            .unwrap_or_else(|| fname.clone());
                        let rhs = self.expr_ast(val);
                        stmts.push(Stmt::Assign(format!("_u{}", lua_field_index(&key)), rhs));
                    }
                    stmts.push(Stmt::Return(Expr::name("_u")));
                    return Expr::call(
                        Expr::paren(Expr::Func(vec![], FuncBody::Inline(stmts))),
                        vec![],
                    );
                }
                // Generate: (function() local _r = __force(record)
                //   local _u = {_r[1], _r[2], ...}; _u[i] = val; ...; return _u end)()
                let mut copy_items = Vec::new();
                for i in 1..=*num_fields {
                    copy_items.push(Item::Pos(Expr::name(format!("_r[{}]", i))));
                }
                let mut stmts = vec![
                    Stmt::Local(vec!["_r".into()], Some(self.forced_ast(record))),
                    Stmt::Local(vec!["_u".into()], Some(Expr::Table(copy_items))),
                ];
                for (_, idx, val) in updates {
                    let rhs = self.expr_ast(val);
                    stmts.push(Stmt::Assign(format!("_u[{}]", idx), rhs));
                }
                stmts.push(Stmt::Return(Expr::name("_u")));
                Expr::call(
                    Expr::paren(Expr::Func(vec![], FuncBody::Inline(stmts))),
                    vec![],
                )
            }
            TExprKind::OutgoingCallback { callee, arity, run_io } => {
                // Type-directed callback boundary, derived from the callback's
                // MONOMORPHIZED type so it agrees with the enclosing FFI
                // call's edges. What the host passes for each argument crosses
                // Lua→mata-ll and gets the same decode descriptor an FFI
                // result of that type would; the result crosses mata-ll→Lua
                // and gets the same marshal descriptor an FFI argument of
                // that type would. A position that is opaque at the FFI edge
                // (a type variable, plain ADT, userdata) is therefore opaque
                // here too, and round-tripped state — including polymorphic
                // state instantiated at a structured type, which the old
                // declared-type flags silently corrupted — keeps one
                // representation across both edges.
                //
                // Peel exactly `arity` arrows: the declared arity is fixed,
                // but instantiation may have substituted a function type for
                // the result, growing the arrow spine.
                let mut cur: &Ty = &callee.ty;
                let mut arg_descs: Vec<String> = Vec::new();
                for _ in 0..*arity {
                    if let Ty::Arrow(a, b, _) = cur {
                        let d = if matches!(a.as_ref(), Ty::Arrow(..)) {
                            // A host-passed Lua function nested in a callback
                            // argument: wrap it so mata-ll can call it.
                            "{k=\"func\"}".to_string()
                        } else {
                            self.ffi_decode_desc_inner(a, &mut Vec::new(), None)
                                .map(|d| d.0)
                                .unwrap_or_else(|| "false".into())
                        };
                        arg_descs.push(d);
                        cur = b.as_ref();
                    } else {
                        arg_descs.push("false".into());
                    }
                }
                let produced = match cur {
                    Ty::IO(a) | Ty::LuaIO(_, a) => a.as_ref(),
                    other => other,
                };
                let ret_desc = if let Some(d) =
                    self.ffi_arg_marshal_desc(produced, &mut Vec::new())
                {
                    d
                } else if Self::scalar_lua_type(produced).is_some() {
                    // A scalar result only needs forcing for the host.
                    "true".to_string()
                } else {
                    // Opaque result (type variable, plain ADT, userdata):
                    // return it raw — the exact representation the enclosing
                    // FFI edge passes for the same type.
                    "false".to_string()
                };
                let callee_e = self.expr_ast(callee);
                Expr::call_named(
                    "__mll_wrap_callback_out",
                    vec![
                        callee_e,
                        Expr::lit(arity.to_string()),
                        Expr::raw(format!("{{{}}}", arg_descs.join(", "))),
                        Expr::lit(run_io.to_string()),
                        Expr::raw(ret_desc),
                    ],
                )
            }
            TExprKind::FfiMaybeArg { value } => {
                // Normally consumed by ffi_args_ast inside a SpecCall argument
                // list. If one is ever emitted standalone, degrade to its
                // nullable value: Just x -> x, Nothing -> nil.
                let v = self.expr_ast(value);
                Expr::call_named("__mll_opt", vec![v])
            }
        }
    }

    /// The SpecCall arms of the expression walk, split out only to keep
    /// `expr_ast_inner` readable. Same arm-for-arm structure as before.
    fn spec_call_ast(&mut self, specialized: &str, args: &[TExpr], expr: &TExpr) -> Expr {
        if let Some(rest) = specialized.strip_prefix("__mll_dict:") {
            // Dictionary table literal: { method1 = impl1, method2 = impl2 }
            let parts: Vec<&str> = rest.splitn(2, ':').collect();
            let methods = if parts.len() > 1 { parts[1] } else { "" };
            let mut items = Vec::new();
            for entry in methods.split(',') {
                if entry.is_empty() { continue; }
                let kv: Vec<&str> = entry.splitn(2, '=').collect();
                if kv.len() == 2 {
                    let sv = sanitize_name(kv[1]);
                    items.push(Item::KV(
                        format!("{} = ", sanitize_name(kv[0])),
                        Expr::name(self.lua_ref(&sv)),
                    ));
                }
            }
            Expr::TableSpaced(items)
        } else if let Some(rest) = specialized.strip_prefix("__mll_dictc:") {
            // A CONSTRUCTED dictionary for a parameterized instance
            // (`instance C a => C [a]`): each method is the instance's
            // dictionary-form implementation partially applied to the
            // context's dictionaries, which arrive as `args` (one per
            // context constraint, in declaration order). Emits
            //   (function(__cd1, …) return { m = function(...)
            //       return impl(__cd1, …, ...) end, … } end)(<dicts>)
            let parts: Vec<&str> = rest.splitn(2, ':').collect();
            let methods = if parts.len() > 1 { parts[1] } else { "" };
            let n_dicts = args.len();
            let dict_params: Vec<String> =
                (0..n_dicts).map(|i| format!("__cd{}", i + 1)).collect();
            let mut items = Vec::new();
            for entry in methods.split(',') {
                if entry.is_empty() { continue; }
                let kv: Vec<&str> = entry.splitn(2, '=').collect();
                if kv.len() == 2 {
                    let sv = sanitize_name(kv[1]);
                    let impl_ref = self.lua_ref(&sv);
                    let mut impl_args: Vec<Expr> =
                        dict_params.iter().map(|p| Expr::name(p.clone())).collect();
                    impl_args.push(Expr::name("..."));
                    items.push(Item::KV(
                        format!("{} = ", sanitize_name(kv[0])),
                        Expr::Func(
                            vec!["...".into()],
                            FuncBody::Inline(vec![Stmt::Return(Expr::call_named(
                                &impl_ref, impl_args,
                            ))]),
                        ),
                    ));
                }
            }
            let mut cargs = Vec::new();
            for a in args {
                cargs.push(self.expr_ast(a));
            }
            Expr::call(
                Expr::paren(Expr::Func(
                    dict_params,
                    FuncBody::Inline(vec![Stmt::Return(Expr::TableSpaced(items))]),
                )),
                cargs,
            )
        } else if let Some(elem_eq) = specialized.strip_prefix("__mll_list_eq:") {
            // List eq: recursive element-wise comparison
            let eq_ref = self.lua_ref(elem_eq);
            let a0 = self.expr_ast(&args[0]);
            let a1 = self.expr_ast(&args[1]);
            Expr::call_named("__mll_list_eq", vec![Expr::name(eq_ref), a0, a1])
        } else if let Some(elem_eq) = specialized.strip_prefix("__mll_maybe_eq:") {
            // Maybe eq: Nothing==Nothing, Just a == Just b iff a==b
            let eq_ref = self.lua_ref(elem_eq);
            let a0 = self.expr_ast(&args[0]);
            let a1 = self.expr_ast(&args[1]);
            Expr::call_named("__mll_maybe_eq", vec![Expr::name(eq_ref), a0, a1])
        } else if let Some(rest) = specialized.strip_prefix("__mll_tuple_eq:") {
            // Tuple eq: compare element-wise
            // Format: __mll_tuple_eq:N:eq_E1,eq_E2,...
            let parts: Vec<&str> = rest.splitn(2, ':').collect();
            let n: usize = parts[0].parse().unwrap();
            let eq_fns: Vec<&str> = parts[1].split(',').collect();
            let mut acc: Option<Expr> = None;
            for (i, eq_fn) in eq_fns.iter().enumerate().take(n) {
                let eq_ref = self.lua_ref(eq_fn);
                // Indexing base: the tuple cell must be WHNF, but a
                // concrete variable / already-forcing emission needs
                // no extra wrapper (see forced_prefix_ast).
                let l = self.forced_prefix_ast(&args[0]);
                let r = self.forced_prefix_ast(&args[1]);
                let cmp = Expr::call_named(
                    &eq_ref,
                    vec![
                        Expr::index(l, format!("[{}]", i + 1)),
                        Expr::index(r, format!("[{}]", i + 1)),
                    ],
                );
                acc = Some(match acc {
                    None => cmp,
                    Some(prev) => Expr::binop("and", prev, cmp),
                });
            }
            Expr::paren(acc.unwrap_or_else(|| Expr::raw("")))
        } else if let Some(elem_show) = specialized.strip_prefix("__mll_show_list:") {
            // Specialized list show: iterate with element show function
            let show_ref = self.lua_ref(elem_show);
            let a0 = self.expr_ast(&args[0]);
            Expr::call_named("__mll_show_list", vec![Expr::name(show_ref), a0])
        } else if let Some(elem_show) = specialized.strip_prefix("__mll_show_maybe:") {
            // Specialized Maybe show: type-directed, so Just/Nothing are
            // recovered from the element type (nil == Nothing).
            let show_ref = self.lua_ref(elem_show);
            let a0 = self.expr_ast(&args[0]);
            Expr::call_named("__mll_show_maybe", vec![Expr::name(show_ref), a0])
        } else if let Some(lua_name) = specialized.strip_prefix("__mll_const:") {
            // Constant access: math.pi (no function call)
            Expr::name(lua_name)
        } else if let Some(idx) = specialized.strip_prefix("__mll_tup_get:") {
            // Tuple field access for the derived tuple `show`: force
            // BOTH the tuple cell (outer `__force`) AND the projected
            // field (inner `__force`). This is a value-consumer — the
            // field is handed to `show_E`, which itself forces only one
            // layer, so a now-lazily-built tuple field (a thunk) must be
            // forced to WHNF here or `show` renders the raw thunk table.
            // (Tuple `==` does the same via its `__force(a)[i]` inline;
            // this projection is otherwise the sole `__mll_tup_get`
            // consumer, generated only by generate_tuple_show.)
            let base = self.forced_prefix_ast(&args[0]);
            Expr::force(Expr::index(base, format!("[{}]", idx)))
        } else if let Some(rest) = specialized.strip_prefix("__mll_tup_ret:") {
            // Multi-return FFI: pack Lua multiple returns into a tuple table
            // Format: __mll_tup_ret:N:lua_func
            let parts: Vec<&str> = rest.splitn(2, ':').collect();
            let n: usize = parts[0].parse().unwrap();
            let lua_func = parts[1];
            let vars: Vec<String> = (0..n).map(|i| format!("_r{}", i)).collect();
            let call = Expr::call_named(lua_func, self.ffi_args_ast(args));
            // Decode the packed tuple like every other FFI result: a
            // missing or wrong-typed return value fails with a clear
            // localized error, and structured elements (lists, Maybe,
            // records) are converted to the mata-ll representation.
            let decode = self.ffi_decode_desc(&expr.ty);
            let tuple = Expr::Table(
                vars.iter().map(|v| Item::Pos(Expr::name(v.clone()))).collect(),
            );
            let ret = match &decode {
                Some(desc) => Expr::call_named(
                    "__mll_ffi_decode",
                    vec![
                        Expr::raw(desc.clone()),
                        tuple,
                        Expr::raw(format!("{:?}", Self::ffi_root_name(lua_func))),
                    ],
                ),
                None => tuple,
            };
            Expr::call(
                Expr::paren(Expr::Func(
                    vec![],
                    FuncBody::Inline(vec![
                        Stmt::Local(vars, Some(call)),
                        Stmt::Return(ret),
                    ]),
                )),
                vec![],
            )
        } else if let Some(lua_func) = specialized.strip_prefix("__mll_iter:") {
            // Iterator FFI: the result type is a list `[element]` (see the
            // LuaIterator reduction). Each iterator step yields one
            // `element`, which must be decoded the same way an ordinary
            // FFI result is — a list element becomes a cons list, a Maybe
            // is wrapped, a structured element is validated. Without this,
            // a structured element (a list, chiefly) was stored as a raw
            // Lua value, so `show`/any consumer failed later with a
            // baffling "raw value" error. A scalar/opaque element needs
            // no descriptor (`nil`), keeping the common iterator's exact
            // old codegen.
            let elem_desc = match &expr.ty {
                Ty::List(elem) =>
                    self.ffi_decode_desc_inner(elem, &mut Vec::new(), None).map(|d| d.0),
                _ => None,
            };
            let desc_args = |elem_desc: &Option<String>| -> Vec<Expr> {
                match elem_desc {
                    Some(desc) => vec![
                        Expr::raw(desc.clone()),
                        Expr::raw(format!("{:?}", Self::ffi_root_name(lua_func))),
                    ],
                    None => vec![Expr::lit("nil"), Expr::lit("nil")],
                }
            };
            // __mll_iter(factory, decode_desc, root, arg0, arg1, ...)
            if let Some(method) = lua_func.strip_prefix(':') {
                // Method-form iterator (`LuaIterator ":gmatch" [...]`):
                // the factory is a method on the first argument. A
                // method name is not a Lua expression, so bind the
                // receiver once and pass the method function with the
                // receiver as the factory's first argument
                // (`__recv.m(__recv, ...)` ≡ `__recv:m(...)`).
                let recv = self.forced_ast(&args[0]);
                let mut iter_args = vec![Expr::name(format!("__recv.{}", method))];
                iter_args.extend(desc_args(&elem_desc));
                iter_args.push(Expr::name("__recv"));
                iter_args.extend(self.ffi_args_ast(&args[1..]));
                Expr::call(
                    Expr::paren(Expr::Func(
                        vec![],
                        FuncBody::Inline(vec![
                            Stmt::Local(vec!["__recv".into()], Some(recv)),
                            Stmt::Return(Expr::call_named("__mll_iter", iter_args)),
                        ]),
                    )),
                    vec![],
                )
            } else {
                let mut iter_args = vec![Expr::name(lua_func)];
                iter_args.extend(desc_args(&elem_desc));
                iter_args.extend(self.ffi_args_ast(args));
                Expr::call_named("__mll_iter", iter_args)
            }
        } else if let Some(lua_func) = specialized.strip_prefix("__mll_try:") {
            // Try FFI: wrap the (val, err) convention in Either via
            // __mll_try. The SUCCESS payload crosses the FFI boundary
            // like any other result, so it carries the same
            // type-directed decode descriptor (a raw Lua array where
            // [Int] was declared must become a cons list, not be
            // walked as a cons cell later).
            let desc = self.ffi_catch_decode_desc(&expr.ty);
            let desc_str = desc.as_deref().unwrap_or("false").to_string();
            let root = Self::ffi_root_name(lua_func);
            let call = if let Some(method) = lua_func.strip_prefix(':') {
                // Method call try: handle:method(args)
                let recv = self.forced_prefix_ast(&args[0]);
                let margs = self.ffi_args_ast(&args[1..]);
                Expr::method(recv, method, margs)
            } else {
                // Global function try
                Expr::call_named(lua_func, self.ffi_args_ast(args))
            };
            Expr::call_named(
                "__mll_try",
                vec![Expr::raw(desc_str), Expr::raw(format!("{:?}", root)), call],
            )
        } else if let Some(lua_func) = specialized.strip_prefix("__mll_pcall:") {
            // LuaCatch: pure call under pcall, result Either String a.
            let desc = self.ffi_catch_decode_desc(&expr.ty);
            self.pcall_call_ast(lua_func, &desc, args)
        } else if let Some(lua_func) = specialized.strip_prefix("__mll_iopcall:") {
            // LuaIOCatch: same pcall capture, deferred as an IO action thunk.
            // Zero-arg still needs a wrapper: the value IS the action.
            let desc = self.ffi_catch_decode_desc(&expr.ty);
            let call = self.pcall_call_ast(lua_func, &desc, args);
            Expr::inline_fn0(call)
        } else if let Some(method) = specialized.strip_prefix(':') {
            // Method call FFI: arg0:method(arg1, arg2, ...)
            // The declared result is ONE value; parenthesize so a
            // multi-returning host method cannot spread extra values into
            // whatever position this call lands in.
            let recv = self.forced_prefix_ast(&args[0]);
            let margs = self.ffi_args_ast(&args[1..]);
            Expr::paren(Expr::method(recv, method, margs))
        } else if let Some(lua_func) = specialized.strip_prefix("__mll_io:") {
            // IO FFI: wrap in action thunk — only performed by >>= / >>
            // Zero-arg IO (e.g., os.clock): emit raw call without closure wrapper,
            // since the function definition already wraps in function()...end.
            let needs_wrapper = !args.is_empty();
            // Type-directed decode of the FFI result (see action_run_ast).
            let decode = self.ffi_decode_desc(&expr.ty);
            let call = if let Some(method) = lua_func.strip_prefix(':') {
                // Method call IO: handle:method(args)
                let recv = self.forced_prefix_ast(&args[0]);
                let margs = self.ffi_args_ast(&args[1..]);
                Expr::method(recv, method, margs)
            } else {
                Expr::call_named(lua_func, self.ffi_args_ast(args))
            };
            let inner = match &decode {
                Some(desc) => Expr::call_named(
                    "__mll_ffi_decode",
                    vec![
                        Expr::raw(desc.clone()),
                        call,
                        Expr::raw(format!("{:?}", Self::ffi_root_name(lua_func))),
                    ],
                ),
                // The declared result is ONE value (multi-return IO uses the
                // __mll_io_tup arm): truncate the raw host call so extra
                // return values cannot spread.
                None => Expr::paren(call),
            };
            if needs_wrapper { Expr::inline_fn0(inner) } else { inner }
        } else if let Some(rest) = specialized.strip_prefix("__mll_io_tup:") {
            // IO FFI with multi-return: wrap in action thunk
            let parts: Vec<&str> = rest.splitn(2, ':').collect();
            let n: usize = parts[0].parse().unwrap();
            let lua_func = parts[1];
            let vars: Vec<String> = (0..n).map(|i| format!("_r{}", i)).collect();
            let call = Expr::call_named(lua_func, self.ffi_args_ast(args));
            // Decode the packed tuple (see the __mll_tup_ret arm).
            let decode = self.ffi_decode_desc(&expr.ty);
            let tuple = Expr::Table(
                vars.iter().map(|v| Item::Pos(Expr::name(v.clone()))).collect(),
            );
            let ret = match &decode {
                Some(desc) => Expr::call_named(
                    "__mll_ffi_decode",
                    vec![
                        Expr::raw(desc.clone()),
                        tuple,
                        Expr::raw(format!("{:?}", Self::ffi_root_name(lua_func))),
                    ],
                ),
                None => tuple,
            };
            Expr::Func(
                vec![],
                FuncBody::Inline(vec![Stmt::Local(vars, Some(call)), Stmt::Return(ret)]),
            )
        } else {
            // Regular (pure) FFI: lua_func(arg0, arg1, ...)
            // Type-directed decode of the result, symmetric with the IO
            // arms above: e.g. a `Maybe a` result from the host must be
            // wrapped into the tagged `Just`/`Nothing` representation.
            let decode = self.ffi_decode_desc(&expr.ty);
            let call = Expr::call_named(specialized, self.ffi_args_ast(args));
            match &decode {
                Some(desc) => Expr::call_named(
                    "__mll_ffi_decode",
                    vec![
                        Expr::raw(desc.clone()),
                        call,
                        Expr::raw(format!("{:?}", Self::ffi_root_name(specialized))),
                    ],
                ),
                // The declared result is ONE value (multi-return uses the
                // __mll_tup_ret arm): truncate the raw host call so extra
                // return values cannot spread — this is also what makes
                // compiled-function references provably single-return for
                // the paren-normalization pass (opt.rs).
                None => Expr::paren(call),
            }
        }
    }

    /// Generate an expression with lazy cons tails for self-referencing definitions.
    /// Cons operations wrap the tail in a thunk via __mll_lazy_cons.
    /// Is `expr` a cons application at its head (`x : xs`, either the infix
    /// form or `App(App(Con ":"), _)`)? A cons-headed self-referential CAF is
    /// built eagerly with a deferred tail (`expr_lazy_ast`); any other head is
    /// thunked so the by-name self-reference resolves after assignment.
    pub(super) fn is_cons_headed(expr: &TExpr) -> bool {
        match &expr.kind {
            TExprKind::InfixApp { op, .. } => op == ":",
            TExprKind::App(f, _) => matches!(&f.kind,
                TExprKind::App(c, _) if matches!(&c.kind, TExprKind::Con(n) if n == ":")),
            TExprKind::Paren(inner) => Self::is_cons_headed(inner),
            _ => false,
        }
    }

    pub(super) fn expr_lazy_ast(&mut self, expr: &TExpr) -> Expr {
        // Check for infix cons: x : rest
        if let TExprKind::InfixApp { op, lhs, rhs } = &expr.kind
            && op == ":" {
                // A cons head is a lazy position here too — weigh it like any
                // argument so a possibly-⊥ element in a self-referential list
                // (`xs = error "boom" : xs`) is suspended, not run at
                // construction. Same rule as the other three `:` sites.
                let head = self.arg_ast(lhs, false);
                let tail = self.expr_lazy_ast(rhs);
                return Expr::call_named("__mll_lazy_cons", vec![head, Expr::inline_fn0(tail)]);
            }
        // Check for App(App(Con(":"), head), tail)
        if let TExprKind::App(func, tail) = &expr.kind
            && let TExprKind::App(con, head) = &func.kind
                && let TExprKind::Con(name) = &con.kind
                    && name == ":" {
                        let head_e = self.arg_ast(head, false);
                        let tail_e = self.expr_lazy_ast(tail);
                        return Expr::call_named(
                            "__mll_lazy_cons",
                            vec![head_e, Expr::inline_fn0(tail_e)],
                        );
                    }
        // Not a cons — fall through to normal gen
        self.expr_ast(expr)
    }

    pub(super) fn literal_ast(lit: &TLiteral) -> Expr {
        match lit {
            // i64::MIN cannot be written in decimal: Lua parses the positive
            // magnitude first (overflowing to float) and negates the float.
            // The hex spelling is defined to wrap to the integer subtype.
            TLiteral::Integer(i64::MIN) => Expr::lit("0x8000000000000000"),
            TLiteral::Integer(n) => Expr::lit(n.to_string()),
            // Emitted as a FLOAT literal ("10.0"/"1e20", never "10"): Lua
            // 5.3+ reads a bare integer spelling as a native integer, which
            // put wrapping 64-bit integer arithmetic behind Double-typed
            // expressions (see lua_number_literal).
            TLiteral::Number(n) => Expr::lit(lua_number_literal(*n)),
            // Routed through the canonical escaper shared with pattern
            // literals and table keys (see `lua_quoted_string`).
            TLiteral::Str(s) => Expr::lit(lua_quoted_string(s)),
            TLiteral::Bool(true) => Expr::lit("true"),
            TLiteral::Bool(false) => Expr::lit("false"),
            TLiteral::Unit => Expr::lit("nil"),
        }
    }
}
