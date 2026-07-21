//! Top-level function emission: clause dispatch and where-binding groups.
//!
//! `gen_function` emits each function as one N-ary Lua function (clause
//! parameters plus `_eta` padding) and hands multi-clause or refutable
//! definitions to the pattern-match paths. The `gen_where_*` family emits a
//! clause's where bindings: function groups are forward-declared and then
//! assigned so mutual recursion resolves, value bindings are assigned
//! strictly only when `strict_binding_ok` proves it sound, and the clause's
//! local strictness and demand rows are installed for the scope and restored
//! at exit so they never leak into a sibling clause.

use crate::tir::*;
use crate::types::Ty;
use super::CodeGen;
use super::names::{sanitize_name};
use super::util::{count_arrows, expr_evaluates_global_ref, expr_references_name};
use super::strictness::{bare_var_alias, strict_binding_safe};

impl CodeGen {
    pub(super) fn gen_function(&mut self, func: &TFunction) {
        let lua_name = sanitize_name(&func.name);
        let clauses = &func.clauses;
        let saved_concrete = self.concrete_vars.clone();
        let saved_locals = self.local_vars.clone();
        let saved_local_count = self.local_count;
        let saved_var_slots = self.var_slots.clone();
        let saved_var_slots_next = self.var_slots_next;
        let saved_var_table_emitted = self.var_table_emitted;
        let saved_local_strict = self.local_strict_params.clone();
        let saved_local_rows = self.local_demand_rows.clone();
        self.local_count = 0;
        self.var_slots.clear();
        self.var_slots_next = 0;
        self.var_table_emitted = false;
        // The demand the whole program provably places on this function's
        // result (deep only when every call site applies it) — seeds the
        // demanded-binding analysis of result-position expressions.
        self.cur_result_demand = self.demand_info.rows.result_demand(&func.name);

        if clauses.is_empty() { self.concrete_vars = saved_concrete; self.local_vars = saved_locals; self.local_count = saved_local_count; self.var_slots = saved_var_slots; self.var_slots_next = saved_var_slots_next; self.var_table_emitted = saved_var_table_emitted; self.local_strict_params = saved_local_strict; self.local_demand_rows = saved_local_rows; return; }

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

            let is_concrete;
            if is_io_action {
                // Wrap in a function (IO action, needs to be called)
                // Use gen_bind_chain_io to flatten do-block let/bind chains
                // into sequential local statements instead of nested IIFEs.
                self.emit_indent();
                self.emit(&self.fn_decl(&lua_name, ""));
                self.emit("\n");
                self.indent += 1;
                self.gen_where_binds(&clauses[0], self.clause_demanded(&clauses[0]));
                self.gen_bind_chain_io(&clauses[0].body);
                self.indent -= 1;
                self.emit_line("end");
                is_concrete = true;
            } else if expr_references_name(&clauses[0].body, &func.name) {
                // Self-referencing value binding (e.g., infinite list).
                // Use the bare name (not fn_table slot) so self-references
                // resolve to this local binding, not a potentially missing slot.
                self.local_vars.insert(lua_name.clone());
                if !self.forward_declared.contains(&lua_name) {
                    self.emit_line(&format!("local {}", lua_name));
                }
                if Self::is_cons_headed(&clauses[0].body) {
                    // Cons-headed (`xs = 0 : xs`): the value is an eagerly built
                    // cons cell whose TAIL self-reference `gen_expr_lazy` defers
                    // into a thunk. The cell itself is a concrete value, so the
                    // deferred self-reference reads it after assignment.
                    self.emit_indent();
                    self.emit(&format!("{} = ", lua_name));
                    self.gen_expr_lazy(&clauses[0].body, &func.name);
                    self.emit("\n");
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
                    self.emit_indent();
                    self.emit(&format!("{} = __thunk(function() return ", lua_name));
                    let was_concrete = self.concrete_vars.contains(&lua_name);
                    self.concrete_vars.insert(lua_name.clone());
                    self.gen_expr(&clauses[0].body);
                    if !was_concrete { self.concrete_vars.remove(&lua_name); }
                    self.emit(" end)\n");
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
                self.emit_indent();
                self.emit(&self.var_decl(&lua_name));
                self.gen_expr(&clauses[0].body);
                self.emit("\n");
                is_concrete = true;
            } else if clauses[0].where_binds.is_empty() {
                // Expensive value binding with no where clause — thunk
                self.emit_indent();
                self.emit(&self.var_decl(&lua_name));
                self.emit("__thunk(function() return ");
                self.gen_expr(&clauses[0].body);
                self.emit(" end)");
                self.emit("\n");
                is_concrete = false;
            } else {
                // Value binding with where clause — wrap in thunked IIFE to scope the locals
                self.emit_indent();
                self.emit(&self.var_decl(&lua_name));
                self.emit("__thunk(function()\n");
                self.indent += 1;
                self.gen_where_binds(&clauses[0], self.clause_demanded(&clauses[0]));
                self.emit_indent();
                self.emit("return ");
                self.gen_expr(&clauses[0].body);
                self.emit("\n");
                self.indent -= 1;
                self.emit_indent();
                self.emit("end)\n");
                is_concrete = false;
            }
            self.emit_line("");
            self.concrete_vars = saved_concrete;
            self.local_strict_params = saved_local_strict;
            self.local_demand_rows = saved_local_rows;
            if is_concrete {
                self.concrete_vars.insert(lua_name);
            } else {
                // Thunked value — must NOT be concrete (needs __force)
                self.concrete_vars.remove(&lua_name);
            }
            return;
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
            self.emit_indent();
            self.emit(&self.fn_decl(&lua_name, &params_str));
            self.emit("\n");
            self.indent += 1;
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
                        let decl = self.declare_local(&sname);
                        if always_cheap {
                            // All callers pass concrete values — no __force needed
                            self.emit_line(&format!("{} = _arg{}", decl, i));
                            self.concrete_vars.insert(sname);
                        } else if is_strict {
                            // Demand analysis: body forces this param — force at entry
                            self.emit_line(&format!("{} = __force(_arg{})", decl, i));
                            self.concrete_vars.insert(sname);
                        } else {
                            // Not demanded — stay lazy
                            self.emit_line(&format!("{} = _arg{}", decl, i));
                        }
                    }
                }
                self.gen_where_binds(clause, self.clause_demanded(clause));
                if eta_count > 0 {
                    // Eta-expand: apply extra params to the body
                    self.emit_indent(); self.emit("return __force(");
                    self.gen_expr(&clause.body);
                    self.emit(")(");
                    self.emit(&eta_params.join(", "));
                    self.emit(")\n");
                } else if Self::returns_st(&func.ty) {
                    // ST-returning function: wrap body in a closure so the
                    // function returns an ST action (deferred computation).
                    // The closure is called by __mll_run in bind chains.
                    self.emit_indent();
                    self.emit("return function()\n");
                    self.indent += 1;
                    self.gen_bind_chain_io(&clause.body);
                    self.indent -= 1;
                    self.emit_indent();
                    self.emit("end\n");
                } else if Self::returns_action(&func.ty) {
                    // IO-returning function: flatten bind chains, performing
                    // sub-actions directly. The function itself acts as the action
                    // closure — callers use gen_action to invoke it.
                    self.gen_bind_chain_io(&clause.body);
                } else {
                    // Pure function: use gen_bind_chain for the body so
                    // If/>>=/>> flatten into statements instead of IIFEs
                    self.gen_bind_chain(&clause.body);
                }
            } else {
                // Force only args that are destructured
                for (i, p) in params.iter().enumerate() {
                    if i < clause.patterns.len()
                        && !matches!(&clause.patterns[i], TPattern::Var(_, _) | TPattern::Wildcard)
                    {
                        self.emit_line(&format!("{} = __force({})", p, p));
                        self.concrete_vars.insert(p.clone());
                    }
                }
                self.gen_where_binds(clause, self.clause_demanded(clause));
                self.gen_pattern_match(&params, clauses);
            }
            self.indent -= 1;
            self.emit_line("end");
            self.emit_line("");
            self.concrete_vars = saved_concrete;
            self.local_vars = saved_locals;
            self.local_count = saved_local_count;
            self.var_slots = saved_var_slots;
            self.var_slots_next = saved_var_slots_next;
            self.var_table_emitted = saved_var_table_emitted;
            self.local_strict_params = saved_local_strict;
            self.local_demand_rows = saved_local_rows;
            self.concrete_vars.insert(lua_name);
            return;
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
        self.emit_indent();
        self.emit(&self.fn_decl(&lua_name, &params_str));
        self.emit("\n");
        self.indent += 1;
        self.concrete_vars.insert(lua_name.clone());
        for dp in &dict_param_names { self.concrete_vars.insert(dp.clone()); }
        // Force params that are destructured OR where call-site analysis
        // shows all callers pass cheap args (so the value is already concrete).
        let call_site_cheap = self.params_always_cheap.get(&func.name).cloned();
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
            if needs_force {
                // Destructured param — must force for pattern matching
                self.emit_line(&format!("{} = __force({})", p, p));
                self.concrete_vars.insert(p.clone());
            } else if always_cheap {
                // All callers pass concrete values — mark concrete, no force needed
                self.concrete_vars.insert(p.clone());
            }
        }
        self.gen_pattern_match(&params, clauses);
        self.indent -= 1;
        self.emit_line("end");
        self.emit_line("");
        self.concrete_vars = saved_concrete;
        self.local_vars = saved_locals;
        self.local_count = saved_local_count;
        self.var_slots = saved_var_slots;
        self.var_slots_next = saved_var_slots_next;
        self.var_table_emitted = saved_var_table_emitted;
        self.local_strict_params = saved_local_strict;
        self.local_demand_rows = saved_local_rows;
        self.concrete_vars.insert(lua_name);
    }

    /// `demanded` seeds which where-bound names are provably forced by the
    /// clause body/guards (see clause_demanded); such bindings may be
    /// assigned strictly even when they read suspended values.
    ///
    /// Takes the whole clause (not just its where_binds) because the local
    /// strictness rows installed here are scoped by scanning the clause for
    /// rebindings of the local function names (see local_fn_strict_params).
    pub(super) fn gen_where_binds(&mut self, clause: &TClause, demanded: crate::demand::DemandMap) {
        let binds: &[TLocalDef] = &clause.where_binds;
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
                    self.declare_local_fwd(&sname);
                }
                if is_func {
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

        // Now emit all bindings in source order — functions and values
        // interleaved as written. The forward declarations above ensure
        // references resolve regardless of order.
        let mut i = 0;
        while i < binds.len() {
            if binds[i].patterns.is_empty() {
                self.gen_where_value(binds, i, &demanded);
                i += 1;
            } else {
                self.gen_where_func_group_assign(binds, i);
                let name = &binds[i].name;
                while i < binds.len() && binds[i].name == *name && !binds[i].patterns.is_empty() {
                    i += 1;
                }
            }
        }
    }

    pub(super) fn gen_where_value(
        &mut self,
        binds: &[TLocalDef],
        i: usize,
        demanded: &std::collections::HashSet<String>,
    ) {
        let bind = &binds[i];
        let sname = sanitize_name(&bind.name);
        // The name was forward-declared in gen_where_binds; assign to it
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
        self.emit_indent();
        if self.strict_binding_ok(bind, demanded) && strict_binding_safe(binds, i) {
            self.emit(&format!("{} = ", lref));
            self.gen_expr(&bind.body);
            self.concrete_vars.insert(sname);
        } else {
            // Thunked: the name must not be considered concrete, even if a
            // same-named outer binding was (this assignment shadows it).
            self.concrete_vars.remove(&sname);
            if let Some(v) = bare_var_alias(binds, i) {
                // Bare-variable RHS: share the existing thunk-or-value
                // (see bare_var_alias).
                self.emit(&format!("{} = ", lref));
                self.gen_lazy_ref(v);
            } else {
                self.emit(&format!("{} = __thunk(function() return ", lref));
                self.gen_expr(&bind.body);
                self.emit(" end)");
            }
        }
        self.emit("\n");
        self.cur_result_demand = saved_rd;
    }

    pub(super) fn gen_where_func_group_assign(&mut self, binds: &[TLocalDef], start: usize) {
        // Emit as assignment (name already forward-declared)
        self.gen_where_func_group_impl(binds, start, true);
    }

    pub(super) fn gen_where_func_group_impl(&mut self, binds: &[TLocalDef], start: usize, pre_declared: bool) {
        // A local function's result is not the enclosing function's result:
        // its body must not inherit the outer deep result demand.
        let saved_result_demand =
            std::mem::replace(&mut self.cur_result_demand, crate::demand::Demand::Head);
        self.gen_where_func_group_body(binds, start, pre_declared);
        self.cur_result_demand = saved_result_demand;
    }

    pub(super) fn gen_where_func_group_body(&mut self, binds: &[TLocalDef], start: usize, pre_declared: bool) {
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
        if !pre_declared {
            self.local_vars.insert(sname.clone());
            self.local_count += 1;
        }
        self.emit_indent();
        if pre_declared {
            // Name was forward-declared; use assignment form
            let lref = self.lua_ref(&sname);
            self.emit(&format!("{} = function({})\n", lref, params_str));
        } else if self.local_count > Self::LOCAL_LIMIT {
            if !self.var_table_emitted {
                self.emit_line("local _v = {}");
                self.var_table_emitted = true;
            }
            self.var_slots_next += 1;
            self.var_slots.insert(sname.clone(), self.var_slots_next);
            self.emit(&format!("_v[{}] = function({})\n", self.var_slots_next, params_str));
        } else {
            self.emit(&format!("local function {}({})\n", sname, params_str));
        }
        self.indent += 1;

        if clauses.len() == 1 {
            let clause = &clauses[0];
            let all_simple = clause.patterns.iter().all(|p|
                matches!(p, TPattern::Var(_, _) | TPattern::Wildcard));

            if all_simple {
                for (j, pat) in clause.patterns.iter().enumerate() {
                    if let TPattern::Var(v, _) = pat {
                        self.emit_line(&format!("local {} = _warg{}", sanitize_name(v), j));
                    }
                }
                self.emit_indent();
                self.emit("return ");
                self.gen_expr(&clause.body);
                self.emit("\n");
            } else {
                for (j, pat) in clause.patterns.iter().enumerate() {
                    if !matches!(pat, TPattern::Var(_, _) | TPattern::Wildcard) {
                        self.emit_line(&format!("_warg{} = __force(_warg{})", j, j));
                    }
                }
                self.gen_pattern_match(&params, &clauses);
            }
        } else {
            for j in 0..num_params {
                let needs_force = clauses.iter().any(|c| {
                    c.patterns.get(j).is_some_and(|pat| {
                        !matches!(pat, TPattern::Var(_, _) | TPattern::Wildcard)
                    })
                });
                if needs_force {
                    self.emit_line(&format!("_warg{} = __force(_warg{})", j, j));
                }
            }
            self.gen_pattern_match(&params, &clauses);
        }

        self.indent -= 1;
        self.emit_line("end");
    }
}
