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
use super::lua::{Block, Expr, FnTarget, FuncBody, Stmt};
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
    /// Spilled-local name -> full spill lvalue (`_v[3]`, `_v2[1]`, …).
    var_slots: std::collections::HashMap<String, String>,
    /// Next free `_v` index.
    var_slots_next: usize,
    /// Whether `local _v = {}` has been emitted in the current scope.
    var_table_emitted: bool,
    /// Strictness rows of the where-local functions in scope.
    local_strict_params: std::collections::HashMap<String, Vec<bool>>,
    /// Structured twin of `local_strict_params`: their demand rows.
    local_demand_rows: std::collections::HashMap<String, crate::demand::LocalRows>,
    /// Emitted parameter counts of the where-local functions in scope (the
    /// local analog of `fixed_arity`): a GENERALIZED where-fn (A19) can be
    /// used at an instantiation with more arrows than its emitted closure
    /// has parameters, and the call must split exactly as for a top-level
    /// callee — locals used to be exempt from `known_callee_arity` on the
    /// grounds that local bindings are monomorphic, which A19 ended.
    local_fn_arity: std::collections::HashMap<String, usize>,
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
            local_fn_arity: cg.local_fn_arity.clone(),
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
        cg.local_fn_arity = self.local_fn_arity;
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
        cg.local_fn_arity = self.local_fn_arity;
    }

}

/// Narrow snapshot of name visibility (`local_vars`) and concreteness
/// (`concrete_vars`) — the pair every expression-level scope saves and
/// restores (guarded-case closures, let-IIFEs, lambdas, do-block binder
/// scopes, the inliner's lambdas, the where-group function bodies). These
/// sites never reset the slot counters, so locals declared inside the
/// nested emission deliberately stay counted toward the enclosing function's
/// `_v` spill budget, and any where-scope rows are restored by the pattern
/// emitters themselves. A full [`ScopeSnapshot`] at these per-node sites
/// cloned five collections — two of them module-sized — to write back two;
/// and two sites hand-rolled exactly this pair, the drift the snapshot types
/// exist to prevent.
pub(super) struct VarsSnapshot {
    local_vars: std::collections::HashSet<String>,
    concrete_vars: std::collections::HashSet<String>,
}

impl VarsSnapshot {
    pub(super) fn capture(cg: &CodeGen) -> Self {
        VarsSnapshot { local_vars: cg.local_vars.clone(), concrete_vars: cg.concrete_vars.clone() }
    }
    pub(super) fn restore(self, cg: &mut CodeGen) {
        cg.local_vars = self.local_vars;
        cg.concrete_vars = self.concrete_vars;
    }
}

/// Narrow snapshot for the per-BRANCH loop of the plain (guard-free)
/// value-position `case` emission, which registers pattern-bound names for
/// reference resolution but makes no concreteness claims of its own. A
/// full [`ScopeSnapshot`] there cloned five collections per branch —
/// including the module-sized `concrete_vars` — only to write back one.
/// The capture/restore pair keeps the field list in one place, same as the
/// full snapshot's variants.
pub(super) struct LocalVarsSnapshot {
    local_vars: std::collections::HashSet<String>,
}

impl LocalVarsSnapshot {
    pub(super) fn capture(cg: &CodeGen) -> Self {
        LocalVarsSnapshot { local_vars: cg.local_vars.clone() }
    }
    pub(super) fn restore(self, cg: &mut CodeGen) {
        cg.local_vars = self.local_vars;
    }
}

/// Spill scope for an emitted INNER Lua function (a lambda, a let or
/// guarded-case IIFE, a where-group function, a where-IIFE). The inner
/// function has its own 200-local budget, and its spilled locals are
/// per-invocation state — spilling them into the ENCLOSING function's
/// table (which the shared counters used to do) turned them into shared
/// upvalue slots that a recursive call clobbers in its caller's frame.
/// Entering resets the slot counters and switches to a depth-unique table
/// name (`_v2`, `_v3`, …) so the inner `local _vN = {}` never shadows an
/// enclosing table the body still references; entries for OUTER spilled
/// locals stay in `var_slots`, so references to them keep resolving to the
/// enclosing table as genuine upvalues. Exit restores everything.
pub(super) struct FnSpillScope {
    local_count: usize,
    var_slots: std::collections::HashMap<String, String>,
    var_slots_next: usize,
    var_table_emitted: bool,
    spill_table: String,
    spill_depth: usize,
}

impl FnSpillScope {
    pub(super) fn enter(cg: &mut CodeGen) -> Self {
        let saved = FnSpillScope {
            local_count: cg.local_count,
            var_slots: cg.var_slots.clone(),
            var_slots_next: cg.var_slots_next,
            var_table_emitted: cg.var_table_emitted,
            spill_table: cg.spill_table.clone(),
            spill_depth: cg.spill_depth,
        };
        cg.local_count = 0;
        cg.var_slots_next = 0;
        cg.var_table_emitted = false;
        cg.spill_depth += 1;
        cg.spill_table = format!("_v{}", cg.spill_depth + 1);
        saved
    }
    pub(super) fn exit(self, cg: &mut CodeGen) {
        cg.local_count = self.local_count;
        cg.var_slots = self.var_slots;
        cg.var_slots_next = self.var_slots_next;
        cg.var_table_emitted = self.var_table_emitted;
        cg.spill_table = self.spill_table;
        cg.spill_depth = self.spill_depth;
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
        if !(clauses.len() == 1 && first.patterns.is_empty() && eta_count == 0) {
            // A real (or eta-expanded point-free) function: the slot holds a
            // Lua function value. (A guarded ZERO-pattern clause is NOT a
            // function — it falls through to the value rules below.)
            return true;
        }
        // Value binding: the arms of function_stmts that stay concrete.
        if matches!(&func.ty, Ty::IO(_) | Ty::LuaIO(_, _) | Ty::Forall(_, _)) {
            return true;
        }
        if !first.guards.is_empty() {
            // A guarded value binding (`x | c = a | otherwise = b`) is
            // desugared by function_stmts into a guard-chain CAF; the
            // chain (an `if` with an error fallback) is never a cheap
            // expression, so the slot is always thunked.
            return false;
        }
        let first_body = first.plain_body();
        if expr_references_name(first_body, &func.name) {
            return Self::is_cons_headed(first_body);
        }
        first.where_binds.is_empty()
            && Self::is_cheap(first_body)
            && !Self::contains_trapping_op(first_body)
            && !expr_evaluates_global_ref(first_body)
    }

    /// How many VALUE parameters `function_stmts` emits for this definition —
    /// its clause patterns plus the `_eta` padding that brings the parameter
    /// list up to the declared type's arrow count. This is the arity every
    /// call site must respect under the N-ary convention, and it is fixed by
    /// the DECLARED type: a use at an instantiation that turns the result
    /// variable into a function (`const inc "x" 3`, where `const :: a -> b ->
    /// a` is emitted with two parameters) has MORE outstanding arguments than
    /// the callee takes, and Lua silently discards the excess. `known_callee_arity`
    /// reads this to split such a call into a saturating one plus an
    /// application of its result (see `split_call_ast`).
    ///
    /// Dictionary parameters are not counted: they precede the value
    /// parameters and are supplied by the DictCall lowering, not by the value
    /// spine. A value binding (no patterns, nothing to eta-expand) is 0 — its
    /// slot holds a value or a nullary action, never a function of arguments.
    ///
    /// The two function-emitting arms of `function_stmts` debug_assert their
    /// emitted parameter list against this, so the prediction and the emission
    /// cannot drift (the same discipline as `slot_always_whnf` and
    /// `direct_perform_arity`).
    pub(super) fn emitted_value_arity(func: &TFunction) -> usize {
        let Some(first) = func.clauses.first() else { return 0 };
        let pat_arity = func.clauses.iter().map(|c| c.patterns.len()).max().unwrap_or(0);
        pat_arity + count_arrows(&func.ty).saturating_sub(first.patterns.len())
    }

    /// Is this binding emitted DIRECT-PERFORM — a Lua function whose body IS
    /// the IO action, so calling it saturated performs and returns a result
    /// in the runners' range (see the __mll_run contract in the runtime)?
    /// `Some(n)` gives the saturating argument count. Two arms of
    /// `function_stmts` emit that way, and this predicate mirrors their
    /// branch structure exactly (function_stmts debug_asserts agreement at
    /// each arm, so the two cannot drift):
    ///
    ///  * the nullary IO/LuaIO value arm (`main :: IO ()`, `loop :: IO a`):
    ///    one clause, no patterns, no guards, nothing to eta-expand — the
    ///    `Ty::Forall` CAF wrapper that arm also emits is a deferred value,
    ///    not a performing action, so it is excluded;
    ///  * the single-clause, guard-free, all-simple-pattern function arm
    ///    whose type returns an IO/LuaIO action, with nothing to eta-expand
    ///    (an eta-expanded body applies a callee, it does not perform) and
    ///    not an ST action (ST bodies are wrapped in a closure the runner
    ///    calls).
    ///
    /// Both decline dictionary-taking functions: their call spine carries
    /// dictionary arguments the saturation count cannot see. Multi-clause
    /// and guarded functions are two-level builders (dispatch returns an
    /// action closure) and are never direct-perform. Consumed by
    /// `module_stmts` to seed `direct_perform_fns` before emission.
    pub(super) fn direct_perform_arity(func: &TFunction) -> Option<usize> {
        // A guarded value binding is rewritten first, exactly as
        // function_stmts does — after the rewrite it is a plain value clause
        // (never an action type: desugar_guarded_value excludes them).
        let desugared;
        let func = match Self::desugar_guarded_value(func) {
            Some(f) => { desugared = f; &desugared }
            None => func,
        };
        if !func.dict_params.is_empty() {
            return None;
        }
        let [clause] = func.clauses.as_slice() else { return None };
        if !clause.guards.is_empty() {
            return None;
        }
        let type_arity = count_arrows(&func.ty);
        let eta_count = type_arity.saturating_sub(clause.patterns.len());
        if eta_count > 0 {
            return None;
        }
        if clause.patterns.is_empty() {
            return matches!(&func.ty, Ty::IO(_) | Ty::LuaIO(_, _)).then_some(0);
        }
        let all_simple = clause
            .patterns
            .iter()
            .all(|p| matches!(p, TPattern::Var(_, _) | TPattern::Wildcard));
        (all_simple && !Self::returns_st(&func.ty) && Self::returns_action(&func.ty))
            .then_some(clause.patterns.len())
    }

    /// A guarded VALUE binding — one clause, zero patterns, nothing to
    /// eta-expand, a non-action type — is a CAF whose body is its guard
    /// chain. Rewrite it into a plain value clause whose body is the
    /// desugared chain (nested `if` with a non-exhaustive-guards error
    /// fallback, the same lowering where-bindings get in the parser), so
    /// the value-binding arm handles it. Without this the clause fell
    /// through to the function arm and was emitted as a NULLARY Lua
    /// function while `slot_always_whnf` predicted a WHNF value — use
    /// sites then read the slot bare and did arithmetic on a function
    /// value. Action types (IO/LuaIO/Forall) are excluded: their slots
    /// legitimately hold nullary action functions and the function arm
    /// emits their guard chains correctly.
    fn desugar_guarded_value(func: &TFunction) -> Option<TFunction> {
        let [clause] = func.clauses.as_slice() else { return None };
        if clause.guards.is_empty()
            || !clause.patterns.is_empty()
            || count_arrows(&func.ty) != 0
            || matches!(&func.ty, Ty::IO(_) | Ty::LuaIO(_, _) | Ty::Forall(_, _))
        {
            return None;
        }
        let fallback = TExpr::new(
            TExprKind::App(
                Box::new(TExpr::new(TExprKind::Var("error".into()), Ty::Unit)),
                Box::new(TExpr::new(
                    TExprKind::Lit(TLiteral::Str(
                        format!("Non-exhaustive guards in '{}'", func.name).into_bytes(),
                    )),
                    Ty::Con("String".into()),
                )),
            ),
            func.ty.clone(),
        );
        let chain = clause.guards.iter().rev().fold(fallback, |els, g| {
            TExpr::new(
                TExprKind::If {
                    cond: Box::new(g.condition.clone()),
                    then_branch: Box::new(g.body.clone()),
                    else_branch: Box::new(els),
                },
                func.ty.clone(),
            )
        });
        let mut func = func.clone();
        func.clauses[0].guards = Vec::new();
        func.clauses[0].body = Some(chain);
        Some(func)
    }

    pub(super) fn function_stmts(&mut self, func: &TFunction) -> Vec<Stmt> {
        // Guarded value bindings become guard-chain CAFs first — see
        // `desugar_guarded_value` (and its mirror rule in `slot_always_whnf`).
        let desugared;
        let func = match Self::desugar_guarded_value(func) {
            Some(f) => { desugared = f; &desugared }
            None => func,
        };
        let lua_name = sanitize_name(&func.name);
        let clauses = &func.clauses;
        // The direct-perform outcome this emission actually takes (`Some(n)`
        // at the two arms whose Lua function body IS the action). Every exit
        // checks it against the module-level prediction
        // (`direct_perform_arity` via `direct_perform_fns`), on BOTH sides:
        // a predicted-but-not-emitted entry would let a caller drop the
        // runner around an unperformed action; an emitted-but-unpredicted
        // arm merely loses the bare tail. See check_direct_perform_prediction.
        let mut emitted_direct_perform: Option<usize> = None;
        let scope = ScopeSnapshot::capture(self);
        self.local_count = 0;
        self.var_slots.clear();
        self.var_slots_next = 0;
        self.var_table_emitted = false;
        self.spill_table = "_v".to_string();
        self.spill_depth = 0;
        // The demand the whole program provably places on this function's
        // result (deep only when every call site applies it) — seeds the
        // demanded-binding analysis of result-position expressions.
        self.cur_result_demand = self.demand_info.rows.result_demand(&func.name);

        if clauses.is_empty() {
            scope.restore(self);
            self.check_direct_perform_prediction(func, emitted_direct_perform);
            return Vec::new();
        }

        // Eta-expand: if the function has fewer patterns than type arrows,
        // add extra params so the Lua function matches the expected arity.
        // This handles point-free definitions like: f x = g x  (written as f = g)
        let type_arity = count_arrows(&func.ty);
        let pat_arity = if clauses[0].patterns.is_empty() { 0 } else { clauses[0].patterns.len() };
        let eta_count = type_arity.saturating_sub(pat_arity);

        if clauses.len() == 1 && clauses[0].patterns.is_empty() && clauses[0].guards.is_empty()
            && eta_count == 0 && func.dict_params.is_empty() {
            // A genuine value binding: no parameters and the type has no
            // outstanding arrows to eta-expand — and no DICTIONARY
            // parameters: a parameterized instance's dictform for a
            // NULLARY method (`def = [def]` in the `[a]` instance) has
            // empty patterns but takes its context's dictionaries, and
            // this arm once emitted it as a CAF thunk with the dictionary
            // parameter left as a free (nil) global (F23). With dict
            // params it falls through to the function branch, which emits
            // a real function over them.
            // A point-free *function* alias
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
                let target = self.fn_target(&lua_name);
                let demanded = self.clause_demanded(&clauses[0]);
                // The where-locals live inside the emitted Lua function only.
                // Contain their registration: this arm ends in
                // restore_keeping_locals (the BINDING's module-scope name must
                // persist), which would otherwise keep the where names in
                // local_vars forever — and a where name that collides with a
                // top-level binding would then shadow that binding's fn_table
                // slot in every LATER emission (`lua_ref` emitted the bare
                // name, a nil global).
                let where_scope = VarsSnapshot::capture(self);
                let mut body = self.where_binds_stmts(&clauses[0], demanded);
                // Direct-perform: the emitted function's body IS the action,
                // so a saturated tail call to it (from its own body or any
                // other) may return bare — see direct_perform_arity, which
                // predicted this arm's outcome for the module-level map,
                // and action_run_ast. Only for genuine IO/LuaIO — the
                // Ty::Forall CAF wrapper this arm also emits is a deferred
                // value, not a performing action.
                if matches!(&func.ty, Ty::IO(_) | Ty::LuaIO(_, _)) && func.dict_params.is_empty() {
                    emitted_direct_perform = Some(0);
                }
                body.extend(self.bind_chain_block(clauses[0].plain_body(), true).0);
                where_scope.restore(self);
                stmts.push(Stmt::Function { target, params: Vec::new(), body: Block(body) });
                is_concrete = true;
            } else if expr_references_name(clauses[0].plain_body(), &func.name) {
                // Self-referencing value binding (e.g., infinite list).
                // Use the bare name (not fn_table slot) so self-references
                // resolve to this local binding, not a potentially missing slot.
                self.local_vars.insert(lua_name.clone());
                if !self.forward_declared.contains(&lua_name) {
                    stmts.push(Stmt::Local(vec![lua_name.clone()], None));
                }
                if Self::is_cons_headed(clauses[0].plain_body()) {
                    // Cons-headed (`xs = 0 : xs`): the value is an eagerly built
                    // cons cell whose TAIL self-reference `expr_lazy_ast` defers
                    // into a thunk. The cell itself is a concrete value, so the
                    // deferred self-reference reads it after assignment.
                    let rhs = self.expr_lazy_ast(clauses[0].plain_body());
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
                    let rhs = self.expr_ast(clauses[0].plain_body());
                    if !was_concrete { self.concrete_vars.remove(&lua_name); }
                    stmts.push(Stmt::Assign(lua_name.clone(), Expr::thunk(rhs)));
                    is_concrete = false;
                }
            } else if clauses[0].where_binds.is_empty() && Self::is_cheap(clauses[0].plain_body())
                && !Self::contains_trapping_op(clauses[0].plain_body())
                && !expr_evaluates_global_ref(clauses[0].plain_body()) {
                // Cheap value binding that does not eagerly dereference another
                // top-level binding — safe to evaluate eagerly at module load.
                // A binding like `y = x` or `useX = x + 1` that reads a global
                // (possibly defined later in the file) falls through to the
                // thunk branch below, deferring the read past module load when
                // the slot is still nil.
                // A trapping op (a literal zero divisor the folds declined)
                // must NOT run at load: the CAF is ⊥, and GHC raises it only
                // if the binding is ever demanded — so it stays a thunk.
                let rhs = self.expr_ast(clauses[0].plain_body());
                stmts.push(self.var_decl_stmt(&lua_name, rhs));
                is_concrete = true;
            } else if clauses[0].where_binds.is_empty() {
                // Expensive value binding with no where clause — thunk
                let rhs = self.expr_ast(clauses[0].plain_body());
                stmts.push(self.var_decl_stmt(&lua_name, Expr::thunk(rhs)));
                is_concrete = false;
            } else {
                // Value binding with where clause — wrap in thunked IIFE to scope the locals.
                // Same containment as the IO arm above: the where names are
                // locals of the IIFE, not of the module scope this arm's
                // restore_keeping_locals preserves.
                let where_scope = VarsSnapshot::capture(self);
                // The thunked IIFE is its own Lua function scope.
                let spill = FnSpillScope::enter(self);
                let demanded = self.clause_demanded(&clauses[0]);
                let mut body = self.where_binds_stmts(&clauses[0], demanded);
                body.push(Stmt::Return(self.expr_ast(clauses[0].plain_body())));
                spill.exit(self);
                where_scope.restore(self);
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
            // already chose its force wrongly — a wrongly-unforced (or
            // doubly-forced) value in already-emitted code, so this must
            // fail the compile in release builds too (F11), not just
            // debug-assert.
            if is_concrete != Self::slot_always_whnf(func) && self.internal_error.is_none() {
                self.internal_error = Some(format!(
                    "internal: slot_always_whnf out of sync with function_stmts \
                     for '{}' (predicted {}, emitted {}) — an already-emitted \
                     reference chose its force from the wrong belief; please \
                     report this",
                    func.name,
                    Self::slot_always_whnf(func),
                    is_concrete
                ));
            }
            if is_concrete {
                self.concrete_vars.insert(lua_name);
            } else {
                // Thunked value — must NOT be concrete (needs __force)
                self.concrete_vars.remove(&lua_name);
            }
            self.check_direct_perform_prediction(func, emitted_direct_perform);
            return stmts;
        }

        if clauses.len() == 1 && clauses[0].guards.is_empty() {
            let clause = &clauses[0];
            let dict_param_names: Vec<String> = func.dict_params.iter().map(|(_, p)| p.clone()).collect();
            let mut params: Vec<String> = (0..clause.patterns.len()).map(|i| format!("_arg{}", i)).collect();
            let eta_params: Vec<String> = (0..eta_count).map(|i| format!("_eta{}", i)).collect();
            params.extend(eta_params.iter().cloned());
            debug_assert_eq!(params.len(), Self::emitted_value_arity(func),
                "emitted_value_arity disagrees with the single-clause emission of '{}'",
                func.name);
            let mut all_params = dict_param_names.clone();
            all_params.extend(params.iter().cloned());
            let target = self.fn_target(&lua_name);
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
                    // Eta-expand: apply the extra params to the body (see
                    // eta_call_ast for the force-vs-paren callee shapes).
                    let call = self.eta_call_ast(clause.plain_body(), &eta_params);
                    body.push(Stmt::Return(call));
                } else if Self::returns_st(&func.ty) {
                    // ST-returning function: wrap body in a closure so the
                    // function returns an ST action (deferred computation).
                    // The closure is called by __mll_run in bind chains.
                    let chain = self.bind_chain_block(clause.plain_body(), true);
                    body.push(Stmt::Return(Expr::Func(vec![], FuncBody::Block(chain))));
                } else if Self::returns_action(&func.ty) {
                    // IO-returning function: flatten bind chains, performing
                    // sub-actions directly. The function itself acts as the action
                    // closure — callers use the action runners to invoke it.
                    // Direct-perform: a saturated tail call to this function
                    // (from its own body or any other) performs and forwards
                    // its own runner-normalized result, so it may return
                    // bare — Lua's tail-call form (see direct_perform_arity,
                    // which predicted this arm's outcome for the module-level
                    // map, and action_run_ast). Dict-taking functions are
                    // declined: their call spine carries dictionary
                    // arguments this saturation count cannot see.
                    if func.dict_params.is_empty() {
                        emitted_direct_perform = Some(clause.patterns.len());
                    }
                    body.extend(self.bind_chain_block(clause.plain_body(), true).0);
                } else {
                    // Pure function: use the plain bind-chain builder for the
                    // body so If/>>=/>> flatten into statements instead of IIFEs
                    body.extend(self.bind_chain_block(clause.plain_body(), false).0);
                }
            } else {
                // Force only args that are destructured
                for (i, p) in params.iter().enumerate() {
                    if i < clause.patterns.len()
                        && clause.patterns[i].forces_scrutinee()
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
                Stmt::Function { target, params: all_params, body: Block(body) },
                Stmt::Raw(String::new()),
            ];
            scope.restore(self);
            self.concrete_vars.insert(lua_name);
            self.check_direct_perform_prediction(func, emitted_direct_perform);
            return stmts;
        }

        // Multiple clauses or guards
        let dict_param_names: Vec<String> = func.dict_params.iter().map(|(_, p)| p.clone()).collect();
        let num_params = clauses.iter().map(|c| c.patterns.len()).max().unwrap_or(0);
        let mut params: Vec<String> = (0..num_params).map(|i| format!("_arg{}", i)).collect();
        let eta_params_multi: Vec<String> = (0..eta_count).map(|i| format!("_eta{}", i)).collect();
        params.extend(eta_params_multi.iter().cloned());
        debug_assert_eq!(params.len(), Self::emitted_value_arity(func),
            "emitted_value_arity disagrees with the multi-clause emission of '{}'",
            func.name);
        // Eta padding must be CONSUMED, not just declared: every clause and
        // guard result is applied to the padding parameters by
        // match_tail_stmts (via clause_eta_params — the single-clause arm
        // above does the same inline; this arm's clause bodies used to
        // ignore them, G9).
        self.clause_eta_params = eta_params_multi.clone();
        let mut all_params = dict_param_names.clone();
        all_params.extend(params.iter().cloned());
        let target = self.fn_target(&lua_name);
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
            // once earlier clauses have failed — by a single rebind at the
            // chain split when the next clause provably forces it first
            // (see later_clause_force_col), per use inside the clause
            // conditions otherwise — so a matching earlier clause never
            // forces it. This is GHC's top-to-bottom, left-to-right
            // laziness: `zip [] _` must return `[]` without forcing the
            // second argument.
            let needs_force = clauses.first().is_some_and(|c| {
                c.patterns.get(i).is_some_and(TPattern::forces_scrutinee)
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
        self.clause_eta_params.clear();
        let stmts = vec![
            Stmt::Function { target, params: all_params, body: Block(body) },
            Stmt::Raw(String::new()),
        ];
        scope.restore(self);
        self.concrete_vars.insert(lua_name);
        self.check_direct_perform_prediction(func, emitted_direct_perform);
        stmts
    }

    /// Apply the `_eta` padding parameters to an emitted clause-result
    /// expression — the consuming half of the N-ary convention
    /// (`count_arrows` in util.rs): the emitted Lua function's parameter
    /// list is padded to the full arrow count and every call site passes
    /// outstanding arguments IN THE SAME flat call, and Lua silently
    /// discards arguments beyond the parameter list — so a body that
    /// returns a function must be applied to the padding here (G9:
    /// `pick True h = h` at four arrows returned `h` unapplied, and a
    /// saturated `pick True f 1 2` printed a function value).
    ///
    /// The body is emitted in CALLEE position (`callee_ast`), which for a
    /// bare name yields the name itself — never the eta-expanded closure a
    /// first-class reference gets (`widened_ref_ast`) — so a fixed-arity body
    /// reached with more padding than it has parameters (`f = const` at three
    /// arrows) is split here instead, exactly as at any other call site.
    /// When the emission is a function literal (a lambda, or the
    /// parenthesized closure a partial application builds), it is WHNF by
    /// construction — a `__force` wrapper would be a no-op the peephole
    /// cannot collapse, because the callee position needs the grouping the
    /// force call happens to provide (Lua cannot call a bare
    /// `function … end`), and it would hide the immediate application from
    /// the beta-reduction peephole. Every other shape keeps the force: the
    /// callee must be a function value, not a thunk.
    pub(super) fn eta_call_ast(&mut self, body: &TExpr, eta_params: &[String]) -> Expr {
        let arity = self.known_callee_arity(body);
        let body_e = self.callee_ast(body);
        let callee = match body_e {
            Expr::Func(ps, b) => Expr::paren(Expr::Func(ps, b)),
            Expr::Paren(inner) if matches!(inner.as_ref(), Expr::Func(..)) => {
                Expr::Paren(inner)
            }
            e => Expr::force(e),
        };
        Self::split_call_ast(
            callee,
            eta_params.iter().map(|p| Expr::name(p.clone())).collect(),
            arity,
        )
    }

    /// The direct-perform twin of the `slot_always_whnf` agreement check:
    /// the module-level prediction (`direct_perform_fns`, seeded from
    /// `direct_perform_arity` before any body was emitted) must equal what
    /// `function_stmts` just did. A mismatch means an already-emitted tail
    /// site chose bare-vs-runner from a wrong belief about this function —
    /// on the predicted-but-not-emitted side that is a dropped runner around
    /// an unperformed action, so it fails loudly here. A name whose
    /// duplicate definitions classify differently is exempt (and never
    /// predicted direct-perform; see `direct_perform_conflicts`).
    fn check_direct_perform_prediction(&mut self, func: &TFunction, emitted: Option<usize>) {
        if self.direct_perform_conflicts.contains(&func.name) {
            return;
        }
        let predicted = self.direct_perform_fns.get(&func.name).copied();
        // A mismatch on the predicted-but-not-emitted side is a dropped
        // runner around an unperformed action in already-emitted call
        // sites: fail the compile in release builds too (F11).
        if predicted != emitted && self.internal_error.is_none() {
            self.internal_error = Some(format!(
                "internal: direct_perform_arity out of sync with \
                 function_stmts for '{}' (predicted {:?}, emitted {:?}) — \
                 already-emitted tail sites chose bare-vs-runner from the \
                 wrong belief; please report this",
                func.name, predicted, emitted
            ));
        }
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
            self.local_fn_arity.remove(&b.name);
        }
        // Register each function group's emitted arity — patterns plus eta
        // padding, the formula where_func_group_body_stmts emits — AFTER the
        // shadow-removal loop above (which would otherwise delete these very
        // entries) and before any body, so calls inside the group's own
        // recursive bodies split correctly too (see `local_fn_arity`).
        {
            let mut i = 0;
            while i < binds.len() {
                if !binds[i].patterns.is_empty() {
                    self.local_fn_arity.insert(
                        binds[i].name.clone(),
                        binds[i].patterns.len() + count_arrows(&binds[i].body.ty),
                    );
                    let name = &binds[i].name;
                    while i < binds.len() && binds[i].name == *name && !binds[i].patterns.is_empty() {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
        }
        let local_rows =
            crate::demand::local_fn_strict_params(clause, &self.demand_info.strict_params, &|n| self.is_local_shadowed(n));
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
        let stmt = if Self::is_nullary_action_type(&bind.body.ty) {
            // First-class action binding — mirror bind_chain_block's Let arm:
            // emit a re-performable closure, never a memoizing thunk. A
            // thunked `t = putStrLn "hi"` performs the effect on the first
            // force and caches the unit result, so `t >> t` prints once;
            // GHC performs the action at every use.
            self.concrete_vars.insert(sname);
            Stmt::Assign(lref, Expr::inline_fn0(self.action_run_ast(&bind.body, false)))
        } else if self.strict_binding_ok(bind, demanded) && strict_binding_safe(binds, i) {
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
                body: Some(binds[i].body.clone()),
                where_binds: vec![],
            });
            i += 1;
        }

        // Local functions obey the same N-ary convention as top-level ones
        // (`count_arrows`): every call site — the partial-application
        // closure and the saturated flat call alike — passes outstanding
        // arguments IN THE SAME call, so a local whose body returns a
        // function must be padded to its full arrow count and its results
        // applied to the padding. Unpadded, Lua discarded the extra
        // arguments (G9: a where-local `go True h = h` called `go b h 1 2`
        // returned `h` unapplied).
        let eta_count = clauses.first()
            .and_then(|c| c.body.as_ref())
            .map(|b| count_arrows(&b.ty))
            .unwrap_or(0);
        let mut params: Vec<String> = (0..num_params)
            .map(|j| format!("_warg{}", j))
            .collect();
        let eta_params: Vec<String> = (0..eta_count)
            .map(|j| format!("_eta{}", j))
            .collect();
        params.extend(eta_params.iter().cloned());
        let sname = sanitize_name(name);
        let mut stmts = Vec::new();
        // Name was forward-declared; use assignment form. The lvalue is the
        // bare local or its `_v[N]` spill slot, whichever `lua_ref` resolves.
        let target = FnTarget::Assigned(self.lua_ref(&sname));
        let mut body = Vec::new();
        // The `_wargN = __force(_wargN)` entry rebinds below leave the param
        // provably WHNF: mark it concrete so the clause conditions built by
        // match_scrutinee do not re-force it. `_warg` names are shared by
        // every where-group, so the marks — and the parameter locals the
        // body registers — must not outlive this one.
        let scope = VarsSnapshot::capture(self);
        // The where-group function is its own Lua function scope: its
        // locals must spill into its OWN per-invocation table, never the
        // enclosing function's (a recursive call would clobber its
        // caller's slots — see FnSpillScope).
        let spill = FnSpillScope::enter(self);

        if clauses.len() == 1 {
            let clause = &clauses[0];
            let all_simple = clause.patterns.iter().all(|p|
                matches!(p, TPattern::Var(_, _) | TPattern::Wildcard));

            if all_simple {
                // Registered like every other binder (declare_local_parts),
                // so a parameter that shadows a top-level function resolves
                // to the parameter in the body — an unregistered `local v`
                // left `Var v` on the global path, which for a name the
                // module also defines at top level emitted that function's
                // slot instead of the parameter.
                for (j, pat) in clause.patterns.iter().enumerate() {
                    if let TPattern::Var(v, _) = pat {
                        let sname = sanitize_name(v);
                        let (pre, decl) = self.declare_local_parts(&sname);
                        if let Some(s) = pre { body.push(s); }
                        body.push(decl.stmt(Expr::name(format!("_warg{}", j))));
                    }
                }
                if eta_params.is_empty() {
                    body.push(Stmt::Return(self.expr_ast(clause.plain_body())));
                } else {
                    // Consume the padding (see eta_call_ast / G9).
                    let call = self.eta_call_ast(clause.plain_body(), &eta_params);
                    body.push(Stmt::Return(call));
                }
            } else {
                for (j, pat) in clause.patterns.iter().enumerate() {
                    if pat.forces_scrutinee() {
                        body.push(Stmt::Assign(
                            format!("_warg{}", j),
                            Expr::force(Expr::name(format!("_warg{}", j))),
                        ));
                        self.concrete_vars.insert(format!("_warg{}", j));
                    }
                }
                // Save/restore, not set/clear: this emitter runs from
                // clause_intro_stmts of an ENCLOSING clause matrix, whose
                // own padding must survive for its match_tail_stmts.
                let saved_eta = std::mem::replace(
                    &mut self.clause_eta_params, eta_params.clone());
                body.extend(self.pattern_match_block(&params, &clauses).0);
                self.clause_eta_params = saved_eta;
            }
        } else {
            // Same entry-force rule as the top-level multi-clause emitter:
            // force a parameter at entry only when the FIRST clause
            // scrutinizes it (then every path forces it — clause 0 is always
            // tried first) or the local's demand row proves every path
            // strict; a parameter scrutinized only by LATER clauses stays
            // lazy and is forced after earlier clauses fail — a single
            // chain-split rebind when sound (later_clause_force_col), else
            // per use via match_scrutinee. That is GHC's top-to-bottom, left-to-right
            // laziness — a where-local `go [] _ = []` must return [] without
            // forcing its second argument. (Forcing on ANY clause's scrutiny,
            // as this once did, raised on `go [] (error …)`.)
            let strict_row = self.local_strict_params.get(name).cloned();
            for j in 0..num_params {
                let first_scrutinizes = clauses.first().is_some_and(|c| {
                    c.patterns.get(j).is_some_and(TPattern::forces_scrutinee)
                });
                let is_strict = strict_row.as_ref()
                    .is_some_and(|v| v.get(j).copied().unwrap_or(false));
                if first_scrutinizes || is_strict {
                    body.push(Stmt::Assign(
                        format!("_warg{}", j),
                        Expr::force(Expr::name(format!("_warg{}", j))),
                    ));
                    self.concrete_vars.insert(format!("_warg{}", j));
                }
            }
            // Save/restore — same rationale as the single-clause arm above.
            let saved_eta = std::mem::replace(
                &mut self.clause_eta_params, eta_params.clone());
            body.extend(self.pattern_match_block(&params, &clauses).0);
            self.clause_eta_params = saved_eta;
        }
        spill.exit(self);
        scope.restore(self);

        stmts.push(Stmt::Function { target, params, body: Block(body) });
        stmts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F11: an emitter-agreement violation must fail the compile in every
    /// build profile — it records `internal_error` (surfaced as an error by
    /// `generate`), not just a debug assertion.
    #[test]
    fn direct_perform_disagreement_records_internal_error() {
        let mut cg = CodeGen::new();
        let func = TFunction {
            name: "act".into(),
            ty: crate::types::Ty::Unit,
            clauses: vec![],
            specialized: false,
            dict_params: vec![],
            derived_strict: false,
        };
        // Predicted direct-perform at arity 1, but emission decided otherwise.
        cg.direct_perform_fns.insert("act".into(), 1);
        cg.check_direct_perform_prediction(&func, None);
        let msg = cg.internal_error.expect("disagreement must be recorded");
        assert!(msg.contains("direct_perform_arity"), "{msg}");
        assert!(msg.contains("'act'"), "{msg}");

        // A name whose duplicate definitions classify differently is exempt.
        let mut cg = CodeGen::new();
        cg.direct_perform_fns.insert("act".into(), 1);
        cg.direct_perform_conflicts.insert("act".into());
        cg.check_direct_perform_prediction(&func, None);
        assert!(cg.internal_error.is_none(), "conflicted names are exempt");
    }
}
