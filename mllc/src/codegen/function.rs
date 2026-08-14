//! Top-level function emission: clause dispatch and where-binding groups.
//!
//! `function_stmts` builds each function as one N-ary Lua function
//! (`Stmt::Function` with clause parameters plus `_eta` padding) and hands
//! multi-clause or refutable definitions to the pattern-match builders. The
//! `where_*` family builds a clause's where bindings: function groups are
//! forward-declared and then assigned so mutual recursion resolves, value
//! bindings are assigned strictly only when `strict_binding_ok` proves it
//! sound, and the clause's local strictness and demand rows are installed
//! for the scope and restored at exit so they never leak into a sibling
//! clause. `function_stmts` is the streaming boundary kept for the module
//! layer.

use crate::tir::*;
use crate::types::Ty;
use super::CodeGen;
use super::lua::{Block, Expr, FuncBody, Stmt};
use super::names::{sanitize_name};
use super::util::{count_arrows, expr_evaluates_global_ref, expr_references_name};
use super::strictness::{bare_var_alias, strict_binding_safe};

/// A snapshot of every `CodeGen` field that constitutes lexical-scope state.
///
/// This struct IS the field list: every scope save/restore in the code
/// generator goes through it, so adding a scope-scoped field to `CodeGen`
/// means adding it here — once — and every site inherits it. The hand-cloned
/// per-site subsets this replaces produced a series of leak bugs, each one a
/// site forgetting one field.
///
/// `capture` clones the full set. `restore` writes the full set back. The
/// `restore_*` variants write back a documented subset — they exist because
/// some scope exits deliberately let part of the state persist (each variant
/// says which part and why; the call sites carry the site-specific rationale).
pub(super) struct ScopeSnapshot {
    /// Names known to hold WHNF values (references skip `__force`).
    concrete_vars: std::collections::HashSet<String>,
    /// Locally-bound names; they shadow `fn_table` slots in `lua_ref`.
    local_vars: std::collections::HashSet<String>,
    /// Count of `local` declarations in the current function scope
    /// (drives the 200-local `_v` spill).
    local_count: usize,
    /// Spilled-local name -> 1-based `_v` table index.
    var_slots: std::collections::HashMap<String, usize>,
    /// Next free `_v` index.
    var_slots_next: usize,
    /// Whether `local _v = {}` has been emitted in the current scope.
    var_table_emitted: bool,
    /// Strictness rows of the where-local functions in scope.
    local_strict_params: std::collections::HashMap<String, Vec<bool>>,
    /// Structured twin of `local_strict_params`: their demand rows.
    local_demand_rows: std::collections::HashMap<String, crate::demand::LocalRows>,
}

impl ScopeSnapshot {
    pub(super) fn capture(cg: &CodeGen) -> Self {
        ScopeSnapshot {
            concrete_vars: cg.concrete_vars.clone(),
            local_vars: cg.local_vars.clone(),
            local_count: cg.local_count,
            var_slots: cg.var_slots.clone(),
            var_slots_next: cg.var_slots_next,
            var_table_emitted: cg.var_table_emitted,
            local_strict_params: cg.local_strict_params.clone(),
            local_demand_rows: cg.local_demand_rows.clone(),
        }
    }

    /// Full restore: every scope field returns to its captured value.
    pub(super) fn restore(self, cg: &mut CodeGen) {
        cg.concrete_vars = self.concrete_vars;
        cg.local_vars = self.local_vars;
        cg.local_count = self.local_count;
        cg.var_slots = self.var_slots;
        cg.var_slots_next = self.var_slots_next;
        cg.var_table_emitted = self.var_table_emitted;
        cg.local_strict_params = self.local_strict_params;
        cg.local_demand_rows = self.local_demand_rows;
    }

    /// Restores concreteness and the where-scope rows, deliberately KEEPING
    /// `local_vars` and the slot counters (`local_count`, `var_slots`,
    /// `var_slots_next`, `var_table_emitted`) as grown. Used by the
    /// module-level value-binding arm of `function_stmts`: the binding's
    /// `local` declaration persists at module scope, so its name must stay
    /// registered as a local (later references use the bare name, not a
    /// `fn_table` slot) and its slot stays counted.
    pub(super) fn restore_keeping_locals(self, cg: &mut CodeGen) {
        cg.concrete_vars = self.concrete_vars;
        cg.local_strict_params = self.local_strict_params;
        cg.local_demand_rows = self.local_demand_rows;
    }

    /// Restores name visibility (`local_vars`) and concreteness
    /// (`concrete_vars`) only. Used by expression-level scopes (guarded-case
    /// closures, let-IIFEs, lambdas): these sites never reset the slot
    /// counters, so locals declared inside the nested emission deliberately
    /// stay counted toward the enclosing function's `_v` spill budget, and
    /// any where-scope rows are restored by the pattern emitters themselves.
    pub(super) fn restore_vars(self, cg: &mut CodeGen) {
        cg.local_vars = self.local_vars;
        cg.concrete_vars = self.concrete_vars;
    }

    /// Restores `local_vars` only. Used per-branch by the plain (guard-free)
    /// value-position `case` emission, which registers pattern-bound names
    /// for reference resolution but makes no concreteness claims of its own.
    pub(super) fn restore_local_vars(self, cg: &mut CodeGen) {
        cg.local_vars = self.local_vars;
    }

    /// Restores the where-scope rows (`local_strict_params`,
    /// `local_demand_rows`) only. Used per-clause by the guarded
    /// independent-block pattern emitter, which restores no other scope
    /// state between clauses.
    pub(super) fn restore_rows(self, cg: &mut CodeGen) {
        cg.local_strict_params = self.local_strict_params;
        cg.local_demand_rows = self.local_demand_rows;
    }

    /// Restores `concrete_vars` only. Used by the where-group function-body
    /// emitter to scope its `_warg` concreteness marks.
    pub(super) fn restore_concrete_vars(self, cg: &mut CodeGen) {
        cg.concrete_vars = self.concrete_vars;
    }
}

impl CodeGen {
    /// Will this binding's slot hold a directly-usable (WHNF) value from the
    /// moment it is assigned — i.e. never a thunk? Seeds `concrete_vars` at
    /// forward-declaration time (see the module layout), where a reference
    /// emitted EARLIER than the binding must already know whether to
    /// `__force` it. The answer must be exact on the `true` side: a missed
    /// force hands a raw thunk to strict positions (an `if` condition read a
    /// thunked False CAF as a truthy table). `false` merely emits an
    /// idempotent extra force. Mirrors the branch structure of
    /// `function_stmts`' value-binding arm, which debug_asserts agreement.
    pub(super) fn slot_always_whnf(func: &TFunction) -> bool {
        let clauses = &func.clauses;
        let Some(first) = clauses.first() else { return true };
        let type_arity = count_arrows(&func.ty);
        let pat_arity = if first.patterns.is_empty() { 0 } else { first.patterns.len() };
        let eta_count = type_arity.saturating_sub(pat_arity);
        if !(clauses.len() == 1 && first.patterns.is_empty() && first.guards.is_empty()
            && eta_count == 0)
        {
            // A real (or eta-expanded point-free) function: the slot holds a
            // Lua function value.
            return true;
        }
        // Value binding: the arms of function_stmts that stay concrete.
        if matches!(&func.ty, Ty::IO(_) | Ty::LuaIO(_, _) | Ty::Forall(_, _)) {
            return true;
        }
        if expr_references_name(&first.body, &func.name) {
            return Self::is_cons_headed(&first.body);
        }
        first.where_binds.is_empty()
            && Self::is_cheap(&first.body)
            && !expr_evaluates_global_ref(&first.body)
    }

    pub(super) fn function_stmts(&mut self, func: &TFunction) -> Vec<Stmt> {
        let lua_name = sanitize_name(&func.name);
        let clauses = &func.clauses;
        let scope = ScopeSnapshot::capture(self);
        self.local_count = 0;
        self.var_slots.clear();
        self.var_slots_next = 0;
        self.var_table_emitted = false;
        // The demand the whole program provably places on this function's
        // result (deep only when every call site applies it) — seeds the
        // demanded-binding analysis of result-position expressions.
        self.cur_result_demand = self.demand_info.rows.result_demand(&func.name);

        if clauses.is_empty() {
            scope.restore(self);
            return Vec::new();
        }

        // Eta-expand: if the function has fewer patterns than type arrows,
        // add extra params so the Lua function matches the expected arity.
        // This handles point-free definitions like: f x = g x  (written as f = g)
        let type_arity = count_arrows(&func.ty);
        let pat_arity = if clauses[0].patterns.is_empty() { 0 } else { clauses[0].patterns.len() };
        let eta_count = type_arity.saturating_sub(pat_arity);

        if clauses.len() == 1 && clauses[0].patterns.is_empty() && clauses[0].guards.is_empty()
            && eta_count == 0 {
            // A genuine value binding: no parameters and the type has no
            // outstanding arrows to eta-expand. A point-free *function* alias
            // (`f = g`, where the type still has arrows so eta_count > 0) is
            // NOT handled here — it falls through to the function branch, which
            // eta-expands it into a real callable that looks the referent up at
            // call time. Emitting it as a value instead would either capture a
            // not-yet-assigned slot (forward reference -> nil) or leave a thunk
            // where callers expect a directly-callable function.
            // Check if this is a value binding (non-function type) or a
            // zero-arg function (IO action / thunk)
            let is_io_action = matches!(&func.ty, Ty::IO(_) | Ty::LuaIO(_, _) | Ty::Forall(_, _));

            let mut stmts = Vec::new();
            let is_concrete;
            if is_io_action {
                // Wrap in a function (IO action, needs to be called)
                // Use the IO bind-chain builder to flatten do-block let/bind
                // chains into sequential local statements instead of nested
                // IIFEs.
                let header = self.fn_decl(&lua_name, "");
                let demanded = self.clause_demanded(&clauses[0]);
                let mut body = self.where_binds_stmts(&clauses[0], demanded);
                // Direct-perform: the emitted function's body IS the action,
                // so a tail self-reference may return bare (see
                // direct_perform_self / action_run_ast). Only for genuine
                // IO/LuaIO — the Ty::Forall CAF wrapper this arm also emits
                // is a deferred value, not a performing action.
                let saved_dp = self.direct_perform_self.take();
                if matches!(&func.ty, Ty::IO(_) | Ty::LuaIO(_, _))
                    && func.dict_params.is_empty()
                {
                    self.direct_perform_self = Some((func.name.clone(), 0));
                }
                body.extend(self.bind_chain_block(&clauses[0].body, true).0);
                self.direct_perform_self = saved_dp;
                stmts.push(Stmt::Function { header, body: Block(body) });
                is_concrete = true;
            } else if expr_references_name(&clauses[0].body, &func.name) {
                // Self-referencing value binding (e.g., infinite list).
                // Use the bare name (not fn_table slot) so self-references
                // resolve to this local binding, not a potentially missing slot.
                self.local_vars.insert(lua_name.clone());
                if !self.forward_declared.contains(&lua_name) {
                    stmts.push(Stmt::Local(vec![lua_name.clone()], None));
                }
                if Self::is_cons_headed(&clauses[0].body) {
                    // Cons-headed (`xs = 0 : xs`): the value is an eagerly built
                    // cons cell whose TAIL self-reference `expr_lazy_ast` defers
                    // into a thunk. The cell itself is a concrete value, so the
                    // deferred self-reference reads it after assignment.
                    let rhs = self.expr_lazy_ast(&clauses[0].body);
                    stmts.push(Stmt::Assign(lua_name.clone(), rhs));
                    is_concrete = true;
                } else {
                    // General self-reference (`xs = myCons 0 xs`, `s = S 1 s`,
                    // `xs = map (+1) (0:xs)`): the RHS is not a lazy constructor
                    // application we can build eagerly, and reading the binding
                    // by name in `local xs = f(xs)` reads `xs` BEFORE the
                    // assignment completes (the Lua `local x = <reads x>`
                    // gotcha), yielding a one-step or nil result. Emit the whole
                    // RHS as a thunk so every self-reference resolves after the
                    // binding is assigned. The self-reference INSIDE the thunk
                    // must stay a bare (deferred) name, not `__force(xs)` — that
                    // would force the in-progress thunk and loop — so mark it
                    // concrete only while emitting the body; external uses of
                    // the (thunked) binding still force it.
                    let was_concrete = self.concrete_vars.contains(&lua_name);
                    self.concrete_vars.insert(lua_name.clone());
                    let rhs = self.expr_ast(&clauses[0].body);
                    if !was_concrete { self.concrete_vars.remove(&lua_name); }
                    stmts.push(Stmt::Assign(lua_name.clone(), Expr::thunk(rhs)));
                    is_concrete = false;
                }
            } else if clauses[0].where_binds.is_empty() && Self::is_cheap(&clauses[0].body)
                && !expr_evaluates_global_ref(&clauses[0].body) {
                // Cheap value binding that does not eagerly dereference another
                // top-level binding — safe to evaluate eagerly at module load.
                // A binding like `y = x` or `useX = x + 1` that reads a global
                // (possibly defined later in the file) falls through to the
                // thunk branch below, deferring the read past module load when
                // the slot is still nil.
                let rhs = self.expr_ast(&clauses[0].body);
                stmts.push(self.var_decl_stmt(&lua_name, rhs));
                is_concrete = true;
            } else if clauses[0].where_binds.is_empty() {
                // Expensive value binding with no where clause — thunk
                let rhs = self.expr_ast(&clauses[0].body);
                stmts.push(self.var_decl_stmt(&lua_name, Expr::thunk(rhs)));
                is_concrete = false;
            } else {
                // Value binding with where clause — wrap in thunked IIFE to scope the locals
                let demanded = self.clause_demanded(&clauses[0]);
                let mut body = self.where_binds_stmts(&clauses[0], demanded);
                body.push(Stmt::Return(self.expr_ast(&clauses[0].body)));
                let thunk = Expr::call_named(
                    "__thunk",
                    vec![Expr::Func(vec![], FuncBody::Block(Block(body)))],
                );
                stmts.push(self.var_decl_stmt(&lua_name, thunk));
                is_concrete = false;
            }
            stmts.push(Stmt::Raw(String::new()));
            // Keep locals and slot counters: the binding's `local` persists
            // at module scope (see restore_keeping_locals).
            scope.restore_keeping_locals(self);
            // The forward-declaration seeding predicted this outcome from the
            // same predicate; a mismatch means an earlier-emitted reference
            // already chose its force wrongly.
            debug_assert_eq!(
                is_concrete,
                Self::slot_always_whnf(func),
                "slot_always_whnf out of sync with function_stmts for '{}'",
                func.name
            );
            if is_concrete {
                self.concrete_vars.insert(lua_name);
            } else {
                // Thunked value — must NOT be concrete (needs __force)
                self.concrete_vars.remove(&lua_name);
            }
            return stmts;
        }

        if clauses.len() == 1 && clauses[0].guards.is_empty() {
            let clause = &clauses[0];
            let dict_param_names: Vec<String> = func.dict_params.iter().map(|(_, p)| p.clone()).collect();
            let mut params: Vec<String> = (0..clause.patterns.len()).map(|i| format!("_arg{}", i)).collect();
            let eta_params: Vec<String> = (0..eta_count).map(|i| format!("_eta{}", i)).collect();
            params.extend(eta_params.iter().cloned());
            let mut all_params = dict_param_names.clone();
            all_params.extend(params.iter().cloned());
            let params_str = all_params.join(", ");
            let header = self.fn_decl(&lua_name, &params_str);
            let mut body = Vec::new();
            // The function name is concrete (it's a function value) — allow
            // self-recursive calls to skip __force
            self.concrete_vars.insert(lua_name.clone());
            for dp in &dict_param_names { self.concrete_vars.insert(dp.clone()); }

            let all_simple = clause.patterns.iter().all(|p| matches!(p, TPattern::Var(_, _) | TPattern::Wildcard));
            if all_simple {
                // Mark params concrete based on call-site and demand analysis:
                // - If all callers pass cheap args, skip __force (already concrete).
                // - If demand analysis says param is strict, force at entry.
                // - Otherwise, stay lazy (param might never be used).
                let call_site_cheap = self.params_always_cheap.get(&func.name).cloned();
                let demand_strict = self.demand_info.strict_params.get(&func.name).cloned();
                for (i, pat) in clause.patterns.iter().enumerate() {
                    if let TPattern::Var(v, _) = pat {
                        let sname = sanitize_name(v);
                        let always_cheap = call_site_cheap.as_ref().is_some_and(|v| v.get(i).copied().unwrap_or(false));
                        let is_strict = demand_strict.as_ref().is_some_and(|v| v.get(i).copied().unwrap_or(false));
                        let (pre, decl) = self.declare_local_parts(&sname);
                        if let Some(s) = pre { body.push(s); }
                        if always_cheap {
                            // All callers pass concrete values — no __force needed
                            body.push(decl.stmt(Expr::name(format!("_arg{}", i))));
                            self.concrete_vars.insert(sname);
                        } else if is_strict {
                            // Demand analysis: body forces this param — force at entry
                            body.push(decl.stmt(Expr::force(Expr::name(format!("_arg{}", i)))));
                            self.concrete_vars.insert(sname);
                        } else {
                            // Not demanded — stay lazy
                            body.push(decl.stmt(Expr::name(format!("_arg{}", i))));
                        }
                    }
                }
                let demanded = self.clause_demanded(clause);
                body.extend(self.where_binds_stmts(clause, demanded));
                if eta_count > 0 {
                    // Eta-expand: apply extra params to the body. When the
                    // emission is a function literal (a lambda, or the
                    // parenthesized closure a partial application builds), it
                    // is WHNF by construction — a __force wrapper would be a
                    // no-op the peephole cannot collapse, because the callee
                    // position needs the grouping the force call happens to
                    // provide (Lua cannot call a bare `function … end`).
                    // Emit the paren grouping directly instead. Every other
                    // shape keeps the force: the callee must be a function
                    // value, not a thunk.
                    let body_e = self.expr_ast(&clause.body);
                    let callee = match body_e {
                        Expr::Func(ps, b) => Expr::paren(Expr::Func(ps, b)),
                        Expr::Paren(inner) if matches!(inner.as_ref(), Expr::Func(..)) => {
                            Expr::Paren(inner)
                        }
                        e => Expr::force(e),
                    };
                    body.push(Stmt::Return(Expr::call(
                        callee,
                        eta_params.iter().map(|p| Expr::name(p.clone())).collect(),
                    )));
                } else if Self::returns_st(&func.ty) {
                    // ST-returning function: wrap body in a closure so the
                    // function returns an ST action (deferred computation).
                    // The closure is called by __mll_run in bind chains.
                    let chain = self.bind_chain_block(&clause.body, true);
                    body.push(Stmt::Return(Expr::Func(vec![], FuncBody::Block(chain))));
                } else if Self::returns_action(&func.ty) {
                    // IO-returning function: flatten bind chains, performing
                    // sub-actions directly. The function itself acts as the action
                    // closure — callers use the action runners to invoke it.
                    // Direct-perform: a saturated tail call to this function
                    // performs and forwards its own runner-normalized result,
                    // so it may return bare — Lua's tail-call form (see
                    // direct_perform_self / action_run_ast). Dict-taking
                    // functions are declined: their call spine carries
                    // dictionary arguments this saturation count cannot see.
                    let saved_dp = self.direct_perform_self.take();
                    if func.dict_params.is_empty() {
                        self.direct_perform_self =
                            Some((func.name.clone(), clause.patterns.len()));
                    }
                    body.extend(self.bind_chain_block(&clause.body, true).0);
                    self.direct_perform_self = saved_dp;
                } else {
                    // Pure function: use the plain bind-chain builder for the
                    // body so If/>>=/>> flatten into statements instead of IIFEs
                    body.extend(self.bind_chain_block(&clause.body, false).0);
                }
            } else {
                // Force only args that are destructured
                for (i, p) in params.iter().enumerate() {
                    if i < clause.patterns.len()
                        && !matches!(&clause.patterns[i], TPattern::Var(_, _) | TPattern::Wildcard)
                    {
                        body.push(Stmt::Assign(p.clone(), Expr::force(Expr::name(p.clone()))));
                        self.concrete_vars.insert(p.clone());
                    }
                }
                let demanded = self.clause_demanded(clause);
                body.extend(self.where_binds_stmts(clause, demanded));
                body.extend(self.pattern_match_block(&params, clauses).0);
            }
            let stmts = vec![
                Stmt::Function { header, body: Block(body) },
                Stmt::Raw(String::new()),
            ];
            scope.restore(self);
            self.concrete_vars.insert(lua_name);
            return stmts;
        }

        // Multiple clauses or guards
        let dict_param_names: Vec<String> = func.dict_params.iter().map(|(_, p)| p.clone()).collect();
        let num_params = clauses.iter().map(|c| c.patterns.len()).max().unwrap_or(0);
        let mut params: Vec<String> = (0..num_params).map(|i| format!("_arg{}", i)).collect();
        let eta_params_multi: Vec<String> = (0..eta_count).map(|i| format!("_eta{}", i)).collect();
        params.extend(eta_params_multi.iter().cloned());
        let mut all_params = dict_param_names.clone();
        all_params.extend(params.iter().cloned());
        let params_str = all_params.join(", ");
        let header = self.fn_decl(&lua_name, &params_str);
        let mut body = Vec::new();
        self.concrete_vars.insert(lua_name.clone());
        for dp in &dict_param_names { self.concrete_vars.insert(dp.clone()); }
        // Force params that are destructured OR where call-site analysis
        // shows all callers pass cheap args (so the value is already concrete).
        let call_site_cheap = self.params_always_cheap.get(&func.name).cloned();
        let demand_strict = self.demand_info.strict_params.get(&func.name).cloned();
        for (i, p) in params.iter().enumerate() {
            if i >= num_params { break; }
            let always_cheap = call_site_cheap.as_ref().is_some_and(|v| v.get(i).copied().unwrap_or(false));
            // Force at entry ONLY if the FIRST clause scrutinizes this arg — it
            // is then forced on every path (clause 0 is always tried first). An
            // arg scrutinized only by LATER clauses stays lazy and is forced
            // inside those clauses' conditions (an `elseif` reached only after
            // clause 0 fails), so a matching earlier clause never forces it.
            // This is GHC's top-to-bottom, left-to-right laziness: `zip [] _`
            // must return `[]` without forcing the second argument.
            let needs_force = clauses.first().is_some_and(|c| {
                c.patterns.get(i).is_some_and(|pat| {
                    !matches!(pat, TPattern::Var(_, _) | TPattern::Wildcard)
                })
            });
            // Demand analysis proves EVERY path through every clause (guard
            // chains included, sequenced right-to-left) forces this param,
            // so a single entry force is exactly as strict as the per-use
            // forces it replaces. The single-clause simple path has used
            // this rule all along; without it here, a guard chain like
            // `go n i | i >= 64 = n | …` re-forced n and i at every use.
            let is_strict = demand_strict.as_ref().is_some_and(|v| v.get(i).copied().unwrap_or(false));
            if needs_force || is_strict {
                // Forced on every path — force once at entry.
                body.push(Stmt::Assign(p.clone(), Expr::force(Expr::name(p.clone()))));
                self.concrete_vars.insert(p.clone());
            } else if always_cheap {
                // All callers pass concrete values — mark concrete, no force needed
                self.concrete_vars.insert(p.clone());
            }
        }
        body.extend(self.pattern_match_block(&params, clauses).0);
        let stmts = vec![
            Stmt::Function { header, body: Block(body) },
            Stmt::Raw(String::new()),
        ];
        scope.restore(self);
        self.concrete_vars.insert(lua_name);
        stmts
    }

    /// `demanded` seeds which where-bound names are provably forced by the
    /// clause body/guards (see clause_demanded); such bindings may be
    /// assigned strictly even when they read suspended values.
    ///
    /// Takes the whole clause (not just its where_binds) because the local
    /// strictness rows installed here are scoped by scanning the clause for
    /// rebindings of the local function names (see local_fn_strict_params).
    pub(super) fn where_binds_stmts(&mut self, clause: &TClause, demanded: crate::demand::DemandMap) -> Vec<Stmt> {
        let binds: &[TLocalDef] = &clause.where_binds;
        let mut stmts = Vec::new();
        // Forward-declare ALL where-bound names — values as well as functions —
        // before emitting any definition. A where/let group is mutually
        // recursive in Haskell, and a value may reference itself (e.g. a
        // self-referential lazy list `fib = ... fib ...`). Lua locals are not
        // in scope within their own initializer, so `local x = ...x...` binds
        // the inner `x` to an outer/global, not the new local. Declaring every
        // name first, then assigning, makes self- and mutual references resolve
        // to the locals.
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        {
            let mut i = 0;
            while i < binds.len() {
                let is_func = !binds[i].patterns.is_empty();
                let sname = sanitize_name(&binds[i].name);
                if !self.local_vars.contains(&sname) && seen.insert(sname.clone()) {
                    stmts.extend(self.declare_local_fwd_stmts(&sname));
                }
                if is_func {
                    // A where-bound function-group name holds a Lua function
                    // from its group assignment on — never a thunk. Marking
                    // it concrete here (before any body is emitted) lets
                    // every call in the clause, including the group's own
                    // mutually recursive bodies, skip the __force. Any
                    // same-named VALUE binding in an inner scope re-decides
                    // its own concreteness at assignment (where_value_stmt).
                    self.concrete_vars.insert(sname);
                    let name = &binds[i].name;
                    while i < binds.len() && binds[i].name == *name && !binds[i].patterns.is_empty() {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
        }

        // Local functions in this where group get real strictness rows so
        // every call site emitted within the clause — inside the group's own
        // (mutually) recursive bodies, in sibling value bindings, and in the
        // clause body/guards generated after this returns — passes strict
        // arguments eagerly instead of allocating a thunk per call. Any
        // where-bound NAME first shadows an outer row of the same name (a
        // sibling value binding must not inherit an enclosing scope's row);
        // the enclosing clause emitter restores the map at scope exit.
        for b in binds {
            self.local_strict_params.remove(&b.name);
        }
        let local_rows =
            crate::demand::local_fn_strict_params(clause, &self.demand_info.strict_params);
        self.local_strict_params.extend(local_rows);
        // Structured twin: install the clause's demand rows before the
        // demanded_bindings closure below, so a sibling RHS that routes
        // demand through a local function is seen (see local_demand_rows).
        self.local_demand_rows = self.clause_local_rows(clause);

        // Close the demand seed over sibling RHSes: if the body demands z and
        // z's RHS demands y, y is demanded too (see demanded_bindings).
        let demanded = self.demanded_bindings(binds, demanded);

        // Now build all bindings in source order — functions and values
        // interleaved as written. The forward declarations above ensure
        // references resolve regardless of order.
        let mut i = 0;
        while i < binds.len() {
            if binds[i].patterns.is_empty() {
                stmts.push(self.where_value_stmt(binds, i, &demanded));
                i += 1;
            } else {
                stmts.extend(self.where_func_group_assign_stmts(binds, i));
                let name = &binds[i].name;
                while i < binds.len() && binds[i].name == *name && !binds[i].patterns.is_empty() {
                    i += 1;
                }
            }
        }
        stmts
    }

    pub(super) fn where_value_stmt(
        &mut self,
        binds: &[TLocalDef],
        i: usize,
        demanded: &std::collections::HashSet<String>,
    ) -> Stmt {
        let bind = &binds[i];
        let sname = sanitize_name(&bind.name);
        // The name was forward-declared in where_binds_stmts; assign to it
        // (rather than re-declaring) so the binding's own body can refer to
        // itself and to its mutually-recursive siblings. A cheap value may only
        // be assigned strictly when it does not read a still-nil sibling (a
        // forward or self reference); otherwise it must be thunked so the read
        // happens after every assignment in the group has run.
        let lref = self.lua_ref(&sname);
        // A where-binding's RHS is not the function's result — a first-class
        // action closure inside it must not inherit the deep result demand.
        let saved_rd =
            std::mem::replace(&mut self.cur_result_demand, crate::demand::Demand::Head);
        let stmt = if self.strict_binding_ok(bind, demanded) && strict_binding_safe(binds, i) {
            let rhs = self.expr_ast(&bind.body);
            self.concrete_vars.insert(sname);
            Stmt::Assign(lref, rhs)
        } else {
            // Thunked: the name must not be considered concrete, even if a
            // same-named outer binding was (this assignment shadows it).
            self.concrete_vars.remove(&sname);
            if let Some(v) = bare_var_alias(binds, i) {
                // Bare-variable RHS: share the existing thunk-or-value
                // (see bare_var_alias).
                let rhs = self.lazy_ref_ast(v);
                Stmt::Assign(lref, rhs)
            } else {
                let rhs = self.expr_ast(&bind.body);
                Stmt::Assign(lref, Expr::thunk(rhs))
            }
        };
        self.cur_result_demand = saved_rd;
        stmt
    }

    pub(super) fn where_func_group_assign_stmts(&mut self, binds: &[TLocalDef], start: usize) -> Vec<Stmt> {
        // Build as assignment (every group name was forward-declared by
        // where_binds_stmts — the streaming emitter's not-pre-declared
        // emission path is gone).
        // A local function's result is not the enclosing function's result:
        // its body must not inherit the outer deep result demand.
        let saved_result_demand =
            std::mem::replace(&mut self.cur_result_demand, crate::demand::Demand::Head);
        let stmts = self.where_func_group_body_stmts(binds, start);
        self.cur_result_demand = saved_result_demand;
        stmts
    }

    pub(super) fn where_func_group_body_stmts(&mut self, binds: &[TLocalDef], start: usize) -> Vec<Stmt> {
        let name = &binds[start].name;
        let mut clauses = Vec::new();
        let num_params = binds[start].patterns.len();
        let mut i = start;
        while i < binds.len() && binds[i].name == *name && !binds[i].patterns.is_empty() {
            clauses.push(TClause {
                span: None,
                patterns: binds[i].patterns.clone(),
                guards: vec![],
                body: binds[i].body.clone(),
                where_binds: vec![],
            });
            i += 1;
        }

        let params: Vec<String> = (0..num_params)
            .map(|j| format!("_warg{}", j))
            .collect();
        let params_str = params.join(", ");
        let sname = sanitize_name(name);
        let mut stmts = Vec::new();
        // Name was forward-declared; use assignment form.
        let header = format!("{} = function({})", self.lua_ref(&sname), params_str);
        let mut body = Vec::new();
        // The `_wargN = __force(_wargN)` entry rebinds below leave the param
        // provably WHNF: mark it concrete so the clause conditions built by
        // match_scrutinee do not re-force it. `_warg` names are shared by
        // every where-group, so the marks must not outlive this one.
        let scope = ScopeSnapshot::capture(self);

        if clauses.len() == 1 {
            let clause = &clauses[0];
            let all_simple = clause.patterns.iter().all(|p|
                matches!(p, TPattern::Var(_, _) | TPattern::Wildcard));

            if all_simple {
                for (j, pat) in clause.patterns.iter().enumerate() {
                    if let TPattern::Var(v, _) = pat {
                        body.push(Stmt::Local(
                            vec![sanitize_name(v)],
                            Some(Expr::name(format!("_warg{}", j))),
                        ));
                    }
                }
                body.push(Stmt::Return(self.expr_ast(&clause.body)));
            } else {
                for (j, pat) in clause.patterns.iter().enumerate() {
                    if !matches!(pat, TPattern::Var(_, _) | TPattern::Wildcard) {
                        body.push(Stmt::Assign(
                            format!("_warg{}", j),
                            Expr::force(Expr::name(format!("_warg{}", j))),
                        ));
                        self.concrete_vars.insert(format!("_warg{}", j));
                    }
                }
                body.extend(self.pattern_match_block(&params, &clauses).0);
            }
        } else {
            for j in 0..num_params {
                let needs_force = clauses.iter().any(|c| {
                    c.patterns.get(j).is_some_and(|pat| {
                        !matches!(pat, TPattern::Var(_, _) | TPattern::Wildcard)
                    })
                });
                if needs_force {
                    body.push(Stmt::Assign(
                        format!("_warg{}", j),
                        Expr::force(Expr::name(format!("_warg{}", j))),
                    ));
                    self.concrete_vars.insert(format!("_warg{}", j));
                }
            }
            body.extend(self.pattern_match_block(&params, &clauses).0);
        }
        scope.restore_concrete_vars(self);

        stmts.push(Stmt::Function { header, body: Block(body) });
        stmts
    }
}
