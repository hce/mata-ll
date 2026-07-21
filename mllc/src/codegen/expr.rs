//! The main expression walk: `gen_expr` / `gen_expr_inner`.
//!
//! `gen_expr` is the depth-guard wrapper: past `crate::MAX_NESTING_DEPTH`
//! it records a clean diagnostic (surfaced by `generate`) instead of
//! overflowing the native stack. `gen_expr_inner` is the single large match
//! over every expression form and stays one function on purpose — the arms
//! cross-reference each other and the WHNF predicates mirror them arm for
//! arm. `gen_expr_lazy` emits self-referencing definitions with lazy cons
//! tails (`__mll_lazy_cons`); `gen_literal` emits literals through the
//! canonical string quoting in names.rs.

use crate::tir::*;
use crate::types::Ty;
use super::CodeGen;
use super::names::{is_builtin_op, lua_field_index, lua_quoted_string, primitive_method_lua_op, sanitize_name};
use super::util::{count_arrows};
use super::strictness::{bare_var_alias, strict_binding_safe};

impl CodeGen {
    /// Depth-guard wrapper around `gen_expr_inner` (the whole expression
    /// walk): checked BEFORE recursing deeper, so past the limit it records a
    /// clean diagnostic (surfaced by `generate`) and emits a placeholder
    /// instead of overflowing the native stack.
    pub(super) fn gen_expr(&mut self, expr: &TExpr) {
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
            self.emit("nil");
            return;
        }
        self.expr_depth += 1;
        self.gen_expr_inner(expr);
        self.expr_depth -= 1;
    }

    pub(super) fn gen_expr_inner(&mut self, expr: &TExpr) {
        match &expr.kind {
            TExprKind::Var(name) => {
                match name.as_str() {
                    "otherwise" => self.emit("true"),
                    // A first-class / partially-applied `seq` (e.g. `foldr seq
                    // z`, `map (seq x) ys`, `let g = seq x`) resolves to the
                    // runtime primitive, which forces its first argument and
                    // returns the second (over-application applies the returned
                    // function to the rest). The fully-applied prefix and
                    // backtick forms are lowered inline before reaching here.
                    "seq" => self.emit("__mll_seq"),
                    // A first-class / partially-applied / prefix `div` or `mod`
                    // (`div 7 2`, `map (div 10) xs`, `foldr div z`) resolves to
                    // its forcing wrapper, which forces both arguments to WHNF
                    // then runs the strict core. Only the inline backtick
                    // `a `div` b` (InfixApp) stays on the bare strict core with
                    // pre-forced operands, keeping the arithmetic hot path free
                    // of redundant forces.
                    "div" => self.emit("__mll_div_fn"),
                    "mod" => self.emit("__mll_mod_fn"),
                    "quot" => self.emit("__mll_quot_fn"),
                    "rem" => self.emit("__mll_rem_fn"),
                    _ => {
                        let sname = sanitize_name(name);
                        let lref = self.lua_ref(&sname);
                        if self.concrete_vars.contains(&sname) {
                            self.emit(&lref);
                        } else {
                            self.emit("__force(");
                            self.emit(&lref);
                            self.emit(")");
                        }
                    }
                }
            }
            TExprKind::Con(name) => {
                match name.as_str() {
                    "[]" => self.emit("nil"),
                    _ => {
                        let lref = self.lua_ref(&sanitize_name(name));
                        self.emit(&lref);
                    }
                }
            }
            TExprKind::Lit(lit) => self.gen_literal(lit),
            TExprKind::App(func, arg) => {
                // Record field accessor: inline as direct table indexing.
                // The field may hold a thunk (lazy construction), so force the
                // projected value. The container is forced by gen_expr(arg) when
                // it is a non-concrete variable; __force is idempotent on values.
                // Laziness is preserved because non-strict argument positions
                // thunk-wrap the whole projection (see gen_arg).
                if let TExprKind::Var(name) = &func.kind
                    && let Some(&idx) = self.record_accessors.get(&sanitize_name(name)) {
                        // A LuaDict field is keyed by name; a plain record field
                        // by position. Compute the index expression before
                        // gen_expr borrows self mutably.
                        let index = match self.luadict_field_key.get(&sanitize_name(name)) {
                            Some(key) => lua_field_index(key),
                            None => format!("[{}]", idx),
                        };
                        self.emit("__force(");
                        self.gen_expr(arg);
                        self.emit(&format!("{})", index));
                        return;
                    }

                // Check for cons application: (:) x xs => __mll_cons(x, xs)
                if let TExprKind::App(inner_f, inner_arg) = &func.kind
                    && let TExprKind::Con(name) = &inner_f.kind
                        && name == ":" {
                            // Try to collect a literal list and emit compactly
                            if let Some(elems) = Self::collect_list_literal(expr) {
                                self.emit("(function() local _l = nil; ");
                                for elem in elems.iter().rev() {
                                    self.emit("_l = __mll_cons(");
                                    // A cons head is a lazy position: `:` forces
                                    // neither side. Weigh it like any argument so
                                    // a possibly-⊥ element is suspended rather
                                    // than run when the cell is built.
                                    // Value-consumers force the head on read; see
                                    // the head-consumption contract on __mll_head.
                                    self.gen_arg(elem, false);
                                    self.emit(", _l); ");
                                }
                                self.emit("return _l end)()");
                                return;
                            }
                            // Keep the cons tail lazy. A bare reference — a
                            // variable or a nullary constructor like [] —
                            // already denotes a thunk-or-value, so emit it raw:
                            // forcing it here (gen_expr forces non-concrete
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
                                self.emit("__mll_cons(");
                                self.gen_arg(inner_arg, false); // lazy head — see below
                                self.emit(", ");
                                self.gen_lazy_ref(tail);
                                self.emit(")");
                            } else {
                                self.emit("__mll_lazy_cons(");
                                // Lazy head: suspend a possibly-⊥ element instead
                                // of running it when the cell is built.
                                self.gen_arg(inner_arg, false);
                                self.emit(", function() return ");
                                self.gen_expr(arg);
                                self.emit(" end)");
                            }
                            return;
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
                            self.gen_seq_inline(seq_a, arg);
                            return;
                        }

                // return/pure wrap their argument in an IO action closure whose
                // performed value is the argument, left UNFORCED per the
                // eagerness contract: running `return ⊥` must not raise until
                // something demands the value. gen_arg(strict=false) suspends a
                // possibly-⊥ argument in a thunk and keeps a provably-total one
                // eager, so `return 0` still yields a bare `0` while
                // `return (error "x")` yields an inert thunk. This is the
                // first-class / higher-order path (e.g. `fmap f (return e)`,
                // `mapM (\x -> return (g x)) xs`); the do-block bind chain
                // flattens its own returns through gen_action, which suspends
                // the same way.
                if let TExprKind::Var(name) = &func.kind
                    && (name == "return" || name == "pure") {
                        self.emit("(function() return ");
                        self.gen_arg(arg, false);
                        self.emit(" end)");
                        return;
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
                        self.emit("try_(function() return ");
                        self.gen_action(args[0]);
                        self.emit(" end)");
                        return;
                    }
                    if name == "catch" && args.len() == 2 {
                        self.emit("catch_(function() return ");
                        self.gen_action(args[0]);
                        self.emit(" end, ");
                        self.gen_expr(args[1]);
                        self.emit(")");
                        return;
                    }
                }

                // Typeclass methods on primitive types → inline as Lua operators
                // (primitive_method_lua_op is also what gen_expr_yields_whnf
                // keys on to know this emission is a forced native operation).
                if args.len() == 2
                    && let TExprKind::Var(name) = &f.kind {
                        if let Some(op) = primitive_method_lua_op(name) {
                            self.emit("(");
                            self.gen_forced(args[0]);
                            self.emit(&format!(" {} ", op));
                            self.gen_forced(args[1]);
                            self.emit(")");
                            return;
                        }
                    }


                // Inline small pure functions at call site
                if let TExprKind::Var(name) = &f.kind
                    && let Some((params, body)) = self.inline_fns.get(name).cloned()
                        && args.len() == params.len() {
                            let mut subst = std::collections::HashMap::new();
                            for (param, arg) in params.iter().zip(args.iter()) {
                                subst.insert(param.clone(), *arg);
                            }
                            self.emit("(");
                            self.gen_expr_subst(&body, &subst);
                            self.emit(")");
                            return;
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
                    self.emit(&format!("(function({})\n", extra_params.join(", ")));
                    self.indent += 1;
                    self.emit_indent();
                    self.emit("return ");
                    self.gen_callee(f);
                    self.emit("(");
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 { self.emit(", "); }
                        let is_strict = callee_strict.as_ref()
                            .is_some_and(|v| v.get(i).copied().unwrap_or(false));
                        if i == 0 && let Some(adapter) = arg0_adapter {
                            self.emit(adapter);
                            self.emit("(");
                            self.gen_arg(a, is_strict);
                            self.emit(")");
                        } else {
                            self.gen_arg(a, is_strict);
                        }
                    }
                    for p in &extra_params {
                        self.emit(", ");
                        self.emit(p);
                    }
                    self.emit(")\n");
                    self.indent -= 1;
                    self.emit_indent();
                    self.emit("end)");
                } else {
                    // Full application
                    // Wrap function literals in parens so Lua allows calling them
                    self.gen_callee(f);
                    self.emit("(");
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 { self.emit(", "); }
                        let is_strict = callee_strict.as_ref()
                            .is_some_and(|v| v.get(i).copied().unwrap_or(false));
                        if i == 0 && let Some(adapter) = arg0_adapter {
                            self.emit(adapter);
                            self.emit("(");
                            self.gen_arg(a, is_strict);
                            self.emit(")");
                        } else {
                            self.gen_arg(a, is_strict);
                        }
                    }
                    self.emit(")");
                }
            }
            TExprKind::InfixApp { op, lhs, rhs } => {
                if op == "div" || op == "mod" || op == "quot" || op == "rem" {
                    // Runtime helpers, not inline float math / bare `%`:
                    // math.floor(a/0) yields inf (a float escaping into
                    // Integer) instead of raising, and float division is
                    // inexact past 2^53. __mll_div/__mll_mod raise a clear
                    // error on a zero divisor and use native integer floor
                    // division (Lua 5.3+ `//`) when the host has it. quot/rem
                    // truncate toward zero (remainder takes the dividend's sign).
                    self.emit(match op.as_str() {
                        "div" => "__mll_div(", "mod" => "__mll_mod(",
                        "quot" => "__mll_quot(", _ => "__mll_rem(",
                    });
                    self.gen_forced(lhs);
                    self.emit(", ");
                    self.gen_forced(rhs);
                    self.emit(")");
                    return;
                }
                if op == "++" {
                    self.emit("__mll_list_append(");
                    self.gen_expr(lhs);
                    self.emit(", function() return ");
                    self.gen_expr(rhs);
                    self.emit(" end)");
                    return;
                }
                if op == "!!" {
                    self.emit("__mll_list_index(");
                    self.gen_expr(lhs);
                    self.emit(", ");
                    self.gen_forced(rhs);
                    self.emit(")");
                    return;
                }
                if op == "seq" {
                    // Backtick `a `seq` b`: same inline lowering as prefix
                    // `seq a b` (force a, return b in tail position). Without
                    // this the operator fell to the user-operator branch below
                    // and emitted `seq(a, b)` — a call to a nonexistent global.
                    self.gen_seq_inline(lhs, rhs);
                    return;
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
                        // runtime forces the cell when read. See gen_lazy_ref.
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
                            self.emit("__mll_cons(");
                            self.gen_arg(lhs, false); self.emit(", "); self.gen_lazy_ref(tail);
                            self.emit(")");
                        } else {
                            self.emit("__mll_lazy_cons(");
                            self.gen_arg(lhs, false);
                            self.emit(", function() return ");
                            self.gen_expr(rhs);
                            self.emit(" end)");
                        }
                        return;
                    }
                    "$" => {
                        // f $ x is exactly f x, so x is weighed like a normal
                        // application argument (gen_arg): eager when f's next
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
                        if remaining > 0 {
                            let extra: Vec<String> =
                                (0..remaining).map(|i| format!("_pa{}", i)).collect();
                            self.emit(&format!("(function({}) return ", extra.join(", ")));
                            self.gen_callee(lhs);
                            self.emit("(");
                            if let Some(a) = adapter { self.emit(a); self.emit("("); }
                            self.gen_arg(rhs, rhs_strict);
                            if adapter.is_some() { self.emit(")"); }
                            for p in &extra { self.emit(", "); self.emit(p); }
                            self.emit(") end)");
                        } else {
                            self.gen_callee(lhs);
                            self.emit("(");
                            if let Some(a) = adapter { self.emit(a); self.emit("("); }
                            self.gen_arg(rhs, rhs_strict);
                            if adapter.is_some() { self.emit(")"); }
                            self.emit(")");
                        }
                        return;
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
                        // action, whose enclosing context (gen_arg, statement
                        // actions, value-binding RHSes, lambdas) has already
                        // reset the ambient demand to Head. So it is used as-is.
                        if let TExprKind::Lambda { .. } = &rhs.kind {
                            self.emit("function()\n");
                            self.indent += 1;
                            self.gen_bind_chain_io(expr);
                            self.indent -= 1;
                            self.emit_indent(); self.emit("end");
                        } else {
                            // m >>= f (non-lambda): wrap as action
                            self.emit("function() return ("); self.gen_expr(rhs); self.emit(")(");
                            self.gen_action(lhs); self.emit(")() end");
                        }
                        return;
                    }
                    ">>" => {
                        // IO-then: produce action closure (see the ">>=" arm
                        // for the cur_result_demand rationale).
                        self.emit("function()\n");
                        self.indent += 1;
                        self.gen_bind_chain_io(expr);
                        self.indent -= 1;
                        self.emit_indent(); self.emit("end");
                        return;
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
                        self.emit("(function(_x");
                        for i in 0..extras { self.emit(&format!(", _pa{}", i)); }
                        self.emit(") return ");
                        self.gen_callee(lhs);
                        self.emit("(");
                        if suspend { self.emit("__thunk(function() return "); }
                        if let Some(a) = adapter { self.emit(a); self.emit("("); }
                        if g_extras == 0 {
                            self.gen_callee(rhs);
                            self.emit("(_x)");
                        } else {
                            self.emit("(function(");
                            for i in 0..g_extras {
                                if i > 0 { self.emit(", "); }
                                self.emit(&format!("_pb{}", i));
                            }
                            self.emit(") return ");
                            self.gen_callee(rhs);
                            self.emit("(_x");
                            for i in 0..g_extras { self.emit(&format!(", _pb{}", i)); }
                            self.emit(") end)");
                        }
                        if suspend { self.emit(" end)"); }
                        if adapter.is_some() { self.emit(")"); }
                        for i in 0..extras { self.emit(&format!(", _pa{}", i)); }
                        self.emit(") end)");
                        return;
                    }
                    other => other,
                };
                if is_builtin_op(op) {
                    // Lua-native operator: emit as infix. Operands are forced —
                    // a thunk is a table, which would corrupt arithmetic and
                    // comparison, and is truthy under `and`/`or`.
                    self.emit("("); self.gen_forced(lhs);
                    self.emit(&format!(" {} ", lua_op));
                    self.gen_forced(rhs); self.emit(")");
                } else {
                    // User-defined or non-Lua operator: emit as function call
                    let sop = sanitize_name(op);
                    self.emit(&self.lua_ref(&sop)); self.emit("(");
                    self.gen_expr(lhs); self.emit(", "); self.gen_expr(rhs); self.emit(")");
                }
            }
            TExprKind::Negate(inner) => { self.emit("(-"); self.gen_expr(inner); self.emit(")"); }
            TExprKind::If { cond, then_branch, else_branch } => {
                self.emit("(function()\n"); self.indent += 1;
                self.emit_indent(); self.emit("if "); self.gen_expr(cond); self.emit(" then\n");
                self.indent += 1; self.emit_indent(); self.emit("return "); self.gen_tail(then_branch, false); self.emit("\n"); self.indent -= 1;
                self.emit_indent(); self.emit("else\n");
                self.indent += 1; self.emit_indent(); self.emit("return "); self.gen_tail(else_branch, false); self.emit("\n"); self.indent -= 1;
                self.emit_indent(); self.emit("end\n"); self.indent -= 1;
                self.emit_indent(); self.emit("end)()");
            }
            TExprKind::Case { scrutinee, branches } if branches.iter().any(|b| !b.guards.is_empty()) => {
                // Guarded branches: lower to clause-based matching (via the
                // shared pattern-match emitter) so a branch whose pattern
                // matches but whose guards all fail falls through to the next
                // branch, exactly like function-clause guards.
                let saved_locals = self.local_vars.clone();
                let saved_concrete = self.concrete_vars.clone();
                self.emit("(function(_cg)\n"); self.indent += 1;
                // Entry force, skipped when the argument emission below
                // (gen_expr at the call parens) already yields WHNF.
                if !self.gen_expr_yields_whnf(scrutinee) {
                    self.emit_line("_cg = __force(_cg)");
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
                self.gen_pattern_match(&["_cg".to_string()], &clauses);
                self.local_vars = saved_locals;
                self.concrete_vars = saved_concrete;
                self.indent -= 1; self.emit_indent(); self.emit("end)(");
                self.gen_expr(scrutinee); self.emit(")");
            }
            TExprKind::Case { scrutinee, branches } => {
                self.emit("(function()\n"); self.indent += 1;
                self.emit_indent(); self.emit("local _s = "); self.gen_forced(scrutinee); self.emit("\n");
                for (i, branch) in branches.iter().enumerate() {
                    let mut conditions = Vec::new();
                    let mut bindings = Vec::new();
                    self.collect_pattern_conditions("_s", &branch.pattern, &mut conditions, &mut bindings);
                    // Register pattern-bound names as locals (scoped to this
                    // branch) so references resolve to them rather than a
                    // same-named top-level/prelude function.
                    let saved_locals = self.local_vars.clone();
                    if conditions.is_empty() {
                        if i > 0 { self.emit_indent(); self.emit("else\n"); self.indent += 1; }
                        for (var, val) in &bindings { self.emit_line(&format!("local {} = {}", var, val)); self.local_vars.insert(var.clone()); }
                        self.emit_indent(); self.emit("return "); self.gen_tail(&branch.body, false); self.emit("\n");
                        if i > 0 { self.indent -= 1; self.emit_line("end"); }
                        self.local_vars = saved_locals;
                        break;
                    }
                    let kw = if i == 0 { "if" } else { "elseif" };
                    self.emit_indent(); self.emit(&format!("{} {} then\n", kw, conditions.join(" and ")));
                    self.indent += 1;
                    for (var, val) in &bindings { self.emit_line(&format!("local {} = {}", var, val)); self.local_vars.insert(var.clone()); }
                    self.emit_indent(); self.emit("return "); self.gen_tail(&branch.body, false); self.emit("\n");
                    self.indent -= 1;
                    if i == branches.len() - 1 { self.emit_line("end"); }
                    self.local_vars = saved_locals;
                }
                self.indent -= 1; self.emit_indent(); self.emit("end)()");
            }
            TExprKind::Let { binds, body } => {
                self.emit("(function()\n"); self.indent += 1;
                let saved_locals = self.local_vars.clone();
                let saved_concrete = self.concrete_vars.clone();
                // Forward-declare all names before assigning, so let bindings
                // can be self- and mutually recursive. Lua locals are not in
                // scope within their own initializer, so `local x = ...x...`
                // would bind the inner `x` to an outer/global. See
                // gen_where_binds for the same rationale.
                {
                    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                    let names: Vec<String> = binds.iter()
                        .map(|b| sanitize_name(&b.name))
                        .filter(|n| seen.insert(n.clone()))
                        .collect();
                    if !names.is_empty() {
                        self.emit_indent();
                        self.emit(&format!("local {}\n", names.join(", ")));
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
                    self.emit_indent();
                    let sname = sanitize_name(&bind.name);
                    if self.strict_binding_ok(bind, &demanded) && strict_binding_safe(binds, i) {
                        self.emit(&format!("{} = ", sname));
                        self.gen_expr(&bind.body); self.emit("\n");
                        self.concrete_vars.insert(sname);
                    } else {
                        // Thunked: must not stay marked concrete (a same-named
                        // outer binding may have been).
                        self.concrete_vars.remove(&sname);
                        if let Some(v) = bare_var_alias(binds, i) {
                            // Bare-variable RHS: share the existing
                            // thunk-or-value (see bare_var_alias).
                            self.emit(&format!("{} = ", sname));
                            self.gen_lazy_ref(v);
                            self.emit("\n");
                        } else {
                            self.emit(&format!("{} = __thunk(function() return ", sname));
                            self.gen_expr(&bind.body); self.emit(" end)\n");
                        }
                    }
                }
                self.emit_indent(); self.emit("return "); self.gen_tail(body, false); self.emit("\n");
                self.indent -= 1; self.emit_indent(); self.emit("end)()");
                self.local_vars = saved_locals;
                self.concrete_vars = saved_concrete;
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
                self.emit(&format!("function({})\n", all_params.join(", ")));
                self.indent += 1; self.emit_indent(); self.emit("return ");
                if eta_count > 0 {
                    // The body still has function type (e.g. `\x -> f x` at
                    // type a -> b -> c): apply the eta params to its value,
                    // mirroring the top-level eta-expansion in gen_function.
                    // The callee must be WHNF; when the emission already
                    // yields that, gen_callee only adds the parens a bare fn
                    // literal needs to be called.
                    if self.gen_expr_yields_whnf(inner_body) {
                        self.gen_callee(inner_body);
                    } else {
                        self.emit("__force(");
                        self.gen_expr(inner_body);
                        self.emit(")");
                    }
                    self.emit(&format!("({})", eta_params.join(", ")));
                } else {
                    // Lambda body is in tail position — strip parens for PTC.
                    self.gen_tail(inner_body, false);
                }
                self.emit("\n"); self.indent -= 1;
                self.emit_indent(); self.emit("end");
                self.local_vars = saved_locals;
                self.concrete_vars = saved_concrete;
                self.cur_result_demand = saved_result_demand;
            }
            TExprKind::Paren(inner) => {
                self.emit("("); self.gen_expr(inner); self.emit(")");
            }
            TExprKind::OpFunc(op) => {
                if op == "++" {
                    self.emit("function(_a, _b) return __mll_list_append(_a, function() return _b end) end");
                    return;
                }
                if op == "!!" {
                    self.emit("function(_a, _b) return __mll_list_index(_a, __force(_b)) end");
                    return;
                }
                if op == ":" {
                    self.emit("function(_a, _b) return __mll_cons(_a, _b) end");
                    return;
                }
                let lua_op = match op.as_str() {
                    "<>" => "..", "&&" => "and", "||" => "or", "/=" => "~=",
                    other => other,
                };
                self.emit(&format!("function(_a, _b) return __force(_a) {} __force(_b) end", lua_op));
            }
            TExprKind::SpecCall { specialized, args, .. } => {
                if let Some(rest) = specialized.strip_prefix("__mll_dict:") {
                    // Dictionary table literal: { method1 = impl1, method2 = impl2 }
                    let parts: Vec<&str> = rest.splitn(2, ':').collect();
                    let methods = if parts.len() > 1 { parts[1] } else { "" };
                    self.emit("{ ");
                    let mut first = true;
                    for entry in methods.split(',') {
                        if entry.is_empty() { continue; }
                        let kv: Vec<&str> = entry.splitn(2, '=').collect();
                        if kv.len() == 2 {
                            if !first { self.emit(", "); }
                            first = false;
                            let sv = sanitize_name(kv[1]);
                            self.emit(&format!("{} = {}", sanitize_name(kv[0]), self.lua_ref(&sv)));
                        }
                    }
                    self.emit(" }");
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
                    self.emit("(function(");
                    self.emit(&dict_params.join(", "));
                    self.emit(") return { ");
                    let mut first = true;
                    for entry in methods.split(',') {
                        if entry.is_empty() { continue; }
                        let kv: Vec<&str> = entry.splitn(2, '=').collect();
                        if kv.len() == 2 {
                            if !first { self.emit(", "); }
                            first = false;
                            let sv = sanitize_name(kv[1]);
                            let impl_ref = self.lua_ref(&sv);
                            self.emit(&format!(
                                "{} = function(...) return {}({}{}...) end",
                                sanitize_name(kv[0]),
                                impl_ref,
                                dict_params.join(", "),
                                if n_dicts > 0 { ", " } else { "" },
                            ));
                        }
                    }
                    self.emit(" } end)(");
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 { self.emit(", "); }
                        self.gen_expr(a);
                    }
                    self.emit(")");
                } else if let Some(elem_eq) = specialized.strip_prefix("__mll_list_eq:") {
                    // List eq: recursive element-wise comparison
                    self.emit(&format!("__mll_list_eq({}, ", self.lua_ref(elem_eq)));
                    self.gen_expr(&args[0]);
                    self.emit(", ");
                    self.gen_expr(&args[1]);
                    self.emit(")");
                } else if let Some(elem_eq) = specialized.strip_prefix("__mll_maybe_eq:") {
                    // Maybe eq: Nothing==Nothing, Just a == Just b iff a==b
                    self.emit(&format!("__mll_maybe_eq({}, ", self.lua_ref(elem_eq)));
                    self.gen_expr(&args[0]);
                    self.emit(", ");
                    self.gen_expr(&args[1]);
                    self.emit(")");
                } else if let Some(rest) = specialized.strip_prefix("__mll_tuple_eq:") {
                    // Tuple eq: compare element-wise
                    // Format: __mll_tuple_eq:N:eq_E1,eq_E2,...
                    let parts: Vec<&str> = rest.splitn(2, ':').collect();
                    let n: usize = parts[0].parse().unwrap();
                    let eq_fns: Vec<&str> = parts[1].split(',').collect();
                    self.emit("(");
                    for i in 0..n {
                        if i > 0 { self.emit(" and "); }
                        self.emit(&self.lua_ref(eq_fns[i]));
                        self.emit("(");
                        // Indexing base: the tuple cell must be WHNF, but a
                        // concrete variable / already-forcing emission needs
                        // no extra wrapper (see gen_forced_prefix).
                        self.gen_forced_prefix(&args[0]);
                        self.emit(&format!("[{}], ", i + 1));
                        self.gen_forced_prefix(&args[1]);
                        self.emit(&format!("[{}])", i + 1));
                    }
                    self.emit(")");
                } else if let Some(elem_show) = specialized.strip_prefix("__mll_show_list:") {
                    // Specialized list show: iterate with element show function
                    self.emit(&format!("__mll_show_list({}, ", self.lua_ref(elem_show)));
                    self.gen_expr(&args[0]);
                    self.emit(")");
                } else if let Some(elem_show) = specialized.strip_prefix("__mll_show_maybe:") {
                    // Specialized Maybe show: type-directed, so Just/Nothing are
                    // recovered from the element type (nil == Nothing).
                    self.emit(&format!("__mll_show_maybe({}, ", self.lua_ref(elem_show)));
                    self.gen_expr(&args[0]);
                    self.emit(")");
                } else if let Some(lua_name) = specialized.strip_prefix("__mll_const:") {
                    // Constant access: math.pi (no function call)
                    self.emit(lua_name);
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
                    self.emit("__force(");
                    self.gen_forced_prefix(&args[0]);
                    self.emit(&format!("[{}])", idx));
                } else if let Some(rest) = specialized.strip_prefix("__mll_tup_ret:") {
                    // Multi-return FFI: pack Lua multiple returns into a tuple table
                    // Format: __mll_tup_ret:N:lua_func
                    let parts: Vec<&str> = rest.splitn(2, ':').collect();
                    let n: usize = parts[0].parse().unwrap();
                    let lua_func = parts[1];
                    let vars: Vec<String> = (0..n).map(|i| format!("_r{}", i)).collect();
                    self.emit("(function() local ");
                    self.emit(&vars.join(", "));
                    self.emit(" = ");
                    self.emit(lua_func);
                    self.emit("(");
                    self.gen_ffi_args(args, false);
                    self.emit("); return ");
                    // Decode the packed tuple like every other FFI result: a
                    // missing or wrong-typed return value fails with a clear
                    // localized error, and structured elements (lists, Maybe,
                    // records) are converted to the mata-ll representation.
                    let decode = self.ffi_decode_desc(&expr.ty);
                    if let Some(desc) = &decode {
                        self.emit(&format!("__mll_ffi_decode({}, ", desc));
                    }
                    self.emit("{");
                    self.emit(&vars.join(", "));
                    self.emit("}");
                    if decode.is_some() {
                        self.emit(&format!(", {:?})", Self::ffi_root_name(lua_func)));
                    }
                    self.emit(" end)()");
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
                    // __mll_iter(factory, decode_desc, root, arg0, arg1, ...)
                    if let Some(method) = lua_func.strip_prefix(':') {
                        // Method-form iterator (`LuaIterator ":gmatch" [...]`):
                        // the factory is a method on the first argument. A
                        // method name is not a Lua expression, so bind the
                        // receiver once and pass the method function with the
                        // receiver as the factory's first argument
                        // (`__recv.m(__recv, ...)` ≡ `__recv:m(...)`).
                        self.emit("(function() local __recv = ");
                        self.gen_forced(&args[0]);
                        self.emit(&format!("; return __mll_iter(__recv.{}", method));
                        match &elem_desc {
                            Some(desc) =>
                                self.emit(&format!(", {}, {:?}", desc, Self::ffi_root_name(lua_func))),
                            None => self.emit(", nil, nil"),
                        }
                        self.emit(", __recv");
                        self.gen_ffi_args(&args[1..], true);
                        self.emit(") end)()");
                    } else {
                        self.emit("__mll_iter(");
                        self.emit(lua_func);
                        match &elem_desc {
                            Some(desc) =>
                                self.emit(&format!(", {}, {:?}", desc, Self::ffi_root_name(lua_func))),
                            None => self.emit(", nil, nil"),
                        }
                        self.gen_ffi_args(args, true);
                        self.emit(")");
                    }
                } else if let Some(lua_func) = specialized.strip_prefix("__mll_try:") {
                    // Try FFI: wrap the (val, err) convention in Either via
                    // __mll_try. The SUCCESS payload crosses the FFI boundary
                    // like any other result, so it carries the same
                    // type-directed decode descriptor (a raw Lua array where
                    // [Integer] was declared must become a cons list, not be
                    // walked as a cons cell later).
                    let desc = self.ffi_catch_decode_desc(&expr.ty);
                    let desc_str = desc.as_deref().unwrap_or("false");
                    let root = Self::ffi_root_name(lua_func);
                    self.emit(&format!("__mll_try({}, {:?}, ", desc_str, root));
                    if let Some(method) = lua_func.strip_prefix(':') {
                        // Method call try: handle:method(args)
                        self.gen_forced_prefix(&args[0]);
                        self.emit(&format!(":{}", method));
                        self.emit("(");
                        self.gen_ffi_args(&args[1..], false);
                        self.emit(")");
                    } else {
                        // Global function try
                        self.emit(lua_func);
                        self.emit("(");
                        self.gen_ffi_args(args, false);
                        self.emit(")");
                    }
                    self.emit(")");
                } else if let Some(lua_func) = specialized.strip_prefix("__mll_pcall:") {
                    // LuaCatch: pure call under pcall, result Either String a.
                    let desc = self.ffi_catch_decode_desc(&expr.ty);
                    self.gen_pcall_call(lua_func, &desc, args);
                } else if let Some(lua_func) = specialized.strip_prefix("__mll_iopcall:") {
                    // LuaIOCatch: same pcall capture, deferred as an IO action thunk.
                    // Zero-arg still needs a wrapper: the value IS the action.
                    self.emit("function() return ");
                    let desc = self.ffi_catch_decode_desc(&expr.ty);
                    self.gen_pcall_call(lua_func, &desc, args);
                    self.emit(" end");
                } else if let Some(method) = specialized.strip_prefix(':') {
                    // Method call FFI: arg0:method(arg1, arg2, ...)
                    self.gen_forced_prefix(&args[0]);
                    self.emit(&format!(":{}", method));
                    self.emit("(");
                    self.gen_ffi_args(&args[1..], false);
                    self.emit(")");
                } else if let Some(lua_func) = specialized.strip_prefix("__mll_io:") {
                    // IO FFI: wrap in action thunk — only performed by >>= / >>
                    // Zero-arg IO (e.g., os.clock): emit raw call without closure wrapper,
                    // since the function definition already wraps in function()...end.
                    let needs_wrapper = !args.is_empty();
                    if needs_wrapper { self.emit("function() return "); }
                    // Type-directed decode of the FFI result (see gen_action).
                    let decode = self.ffi_decode_desc(&expr.ty);
                    if let Some(desc) = &decode {
                        self.emit(&format!("__mll_ffi_decode({}, ", desc));
                    }
                    if let Some(method) = lua_func.strip_prefix(':') {
                        // Method call IO: handle:method(args)
                        self.gen_forced_prefix(&args[0]);
                        self.emit(&format!(":{}", method));
                        self.emit("(");
                        self.gen_ffi_args(&args[1..], false);
                        self.emit(")");
                    } else {
                        self.emit(lua_func);
                        self.emit("(");
                        self.gen_ffi_args(args, false);
                        self.emit(")");
                    }
                    if decode.is_some() {
                        self.emit(&format!(", {:?})", Self::ffi_root_name(lua_func)));
                    }
                    if needs_wrapper { self.emit(" end"); }
                } else if let Some(rest) = specialized.strip_prefix("__mll_io_tup:") {
                    // IO FFI with multi-return: wrap in action thunk
                    let parts: Vec<&str> = rest.splitn(2, ':').collect();
                    let n: usize = parts[0].parse().unwrap();
                    let lua_func = parts[1];
                    let vars: Vec<String> = (0..n).map(|i| format!("_r{}", i)).collect();
                    self.emit("function() local ");
                    self.emit(&vars.join(", "));
                    self.emit(" = ");
                    self.emit(lua_func);
                    self.emit("(");
                    self.gen_ffi_args(args, false);
                    self.emit("); return ");
                    // Decode the packed tuple (see the __mll_tup_ret arm).
                    let decode = self.ffi_decode_desc(&expr.ty);
                    if let Some(desc) = &decode {
                        self.emit(&format!("__mll_ffi_decode({}, ", desc));
                    }
                    self.emit("{");
                    self.emit(&vars.join(", "));
                    self.emit("}");
                    if decode.is_some() {
                        self.emit(&format!(", {:?})", Self::ffi_root_name(lua_func)));
                    }
                    self.emit(" end");
                } else {
                    // Regular (pure) FFI: lua_func(arg0, arg1, ...)
                    // Type-directed decode of the result, symmetric with the IO
                    // arms above: e.g. a `Maybe a` result from the host must be
                    // wrapped into the tagged `Just`/`Nothing` representation.
                    let decode = self.ffi_decode_desc(&expr.ty);
                    if let Some(desc) = &decode {
                        self.emit(&format!("__mll_ffi_decode({}, ", desc));
                    }
                    self.emit(specialized);
                    self.emit("(");
                    self.gen_ffi_args(args, false);
                    self.emit(")");
                    if decode.is_some() {
                        self.emit(&format!(", {:?})", Self::ffi_root_name(specialized)));
                    }
                }
            }
            TExprKind::Tuple(elems) => {
                self.emit("{");
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 { self.emit(", "); }
                    // A tuple field is a lazy position: `(,)` forces neither
                    // side, exactly like a cons head. Weigh it like any
                    // argument so a possibly-⊥ field is suspended rather than
                    // run when the tuple is built (fst (1, error) == 1).
                    // Value-consumers force the field on read: pattern
                    // destructuring via field_path, the __mll_tuple_eq /
                    // __mll_tup_get specializations hand the raw field to an
                    // eq/show function that forces, and the FFI boundary
                    // deep-forces through __mll_arg_marshal.
                    self.gen_arg(e, false);
                }
                self.emit("}");
            }
            TExprKind::DictAccess { dict_param, method_name } => {
                self.emit(&format!("{}.{}", sanitize_name(dict_param), sanitize_name(method_name)));
            }
            TExprKind::DictMethod { dict, method_name } => {
                // A method of a CONSTRUCTED dictionary (e.g. the `[a]`
                // dictionary built from the element dictionary). Parenthesized
                // because the dictionary may be a table literal, which Lua
                // cannot index directly.
                self.emit("(");
                self.gen_expr(dict);
                self.emit(&format!(").{}", sanitize_name(method_name)));
            }
            TExprKind::DictCall { func_name, dict_args, value_args } => {
                let sfn = sanitize_name(func_name);
                self.emit(&self.lua_ref(&sfn));
                self.emit("(");
                let mut first = true;
                for d in dict_args {
                    if !first { self.emit(", "); }
                    first = false;
                    self.gen_expr(d);
                }
                for v in value_args {
                    if !first { self.emit(", "); }
                    first = false;
                    self.gen_expr(v);
                }
                self.emit(")");
            }
            TExprKind::RecordUpdate { record, updates, num_fields } => {
                // A LuaDict record is keyed by name, so we can't copy it
                // positionally: shallow-copy every key with `pairs`, then
                // overwrite the updated fields by name.
                let is_luadict = updates.first()
                    .map(|(fname, _, _)| self.luadict_field_key.contains_key(&sanitize_name(fname)))
                    .unwrap_or(false);
                if is_luadict {
                    self.emit("(function() local _r = ");
                    self.gen_forced(record);
                    self.emit("; local _u = {}; for _k, _v in pairs(_r) do _u[_k] = _v end");
                    for (fname, _, val) in updates {
                        // Resolve the Haskell field name to its effective Lua
                        // key (`as "key"` rename) — the copied table is keyed
                        // by effective keys, so writing the raw name would add
                        // a stray key instead of updating the field.
                        let key = self.luadict_field_key
                            .get(&sanitize_name(fname))
                            .cloned()
                            .unwrap_or_else(|| fname.clone());
                        self.emit(&format!("; _u{} = ", lua_field_index(&key)));
                        self.gen_expr(val);
                    }
                    self.emit("; return _u end)()");
                    return;
                }
                // Generate: (function() local _r = __force(record)
                //   local _u = {_r[1], _r[2], ...}; _u[i] = val; ...; return _u end)()
                self.emit("(function() local _r = ");
                self.gen_forced(record);
                self.emit("; local _u = {");
                for i in 1..=*num_fields {
                    if i > 1 { self.emit(", "); }
                    self.emit(&format!("_r[{}]", i));
                }
                self.emit("}");
                for (_, idx, val) in updates {
                    self.emit(&format!("; _u[{}] = ", idx));
                    self.gen_expr(val);
                }
                self.emit("; return _u end)()");
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
                self.emit("__mll_wrap_callback_out(");
                self.gen_expr(callee);
                self.emit(&format!(", {}, {{{}}}, {}, {})",
                    arity, arg_descs.join(", "), run_io, ret_desc));
            }
            TExprKind::FfiMaybeArg { value } => {
                // Normally consumed by gen_ffi_args inside a SpecCall argument
                // list. If one is ever emitted standalone, degrade to its
                // nullable value: Just x -> x, Nothing -> nil.
                self.emit("__mll_opt(");
                self.gen_expr(value);
                self.emit(")");
            }
        }
    }

    /// Generate an expression with lazy cons tails for self-referencing definitions.
    /// Cons operations wrap the tail in a thunk via __mll_lazy_cons.
    /// Is `expr` a cons application at its head (`x : xs`, either the infix
    /// form or `App(App(Con ":"), _)`)? A cons-headed self-referential CAF is
    /// built eagerly with a deferred tail (`gen_expr_lazy`); any other head is
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

    pub(super) fn gen_expr_lazy(&mut self, expr: &TExpr, self_name: &str) {
        // Check for infix cons: x : rest
        if let TExprKind::InfixApp { op, lhs, rhs } = &expr.kind
            && op == ":" {
                self.emit("__mll_lazy_cons(");
                // A cons head is a lazy position here too — weigh it like any
                // argument so a possibly-⊥ element in a self-referential list
                // (`xs = error "boom" : xs`) is suspended, not run at
                // construction. Same rule as the other three `:` sites.
                self.gen_arg(lhs, false);
                self.emit(", function() return ");
                self.gen_expr_lazy(rhs, self_name);
                self.emit(" end)");
                return;
            }
        // Check for App(App(Con(":"), head), tail)
        if let TExprKind::App(func, tail) = &expr.kind
            && let TExprKind::App(con, head) = &func.kind
                && let TExprKind::Con(name) = &con.kind
                    && name == ":" {
                        self.emit("__mll_lazy_cons(");
                        self.gen_arg(head, false);
                        self.emit(", function() return ");
                        self.gen_expr_lazy(tail, self_name);
                        self.emit(" end)");
                        return;
                    }
        // Not a cons — fall through to normal gen
        self.gen_expr(expr);
    }

    pub(super) fn gen_literal(&mut self, lit: &TLiteral) {
        match lit {
            // i64::MIN cannot be written in decimal: Lua parses the positive
            // magnitude first (overflowing to float) and negates the float.
            // The hex spelling is defined to wrap to the integer subtype.
            TLiteral::Integer(i64::MIN) => self.emit("0x8000000000000000"),
            TLiteral::Integer(n) => self.emit(&format!("{}", n)),
            TLiteral::Number(n) => self.emit(&format!("{}", n)),
            // Routed through the canonical escaper shared with pattern
            // literals and table keys (see `lua_quoted_string`).
            TLiteral::Str(s) => self.emit(&lua_quoted_string(s)),
            TLiteral::Bool(true) => self.emit("true"),
            TLiteral::Bool(false) => self.emit("false"),
            TLiteral::Unit => self.emit("nil"),
        }
    }
}
