//! Demand analysis: determines which function parameters are strict —
//! guaranteed to be forced whenever the function's result is forced to WHNF.
//!
//! A parameter marked strict can be passed eagerly at call sites
//! (no thunk allocation) and forced at function entry: forcing it early only
//! reorders a force the demanded result performs anyway, so it cannot
//! introduce a bottom the callee would not have produced.
//!
//! The result is the greatest fixed point of the demand equations, seeded
//! optimistically (every parameter strict) and shrunk to consistency. The
//! greatest fixed point — rather than the least — is what lets self- and
//! mutually-recursive parameters (notably tail accumulators) be recognized as
//! strict; see the extended note in `analyze`.
//!
//! Cross-function propagation: if callee `g` is strict in position j,
//! then `f(... g(x) ...)` where x is a parameter of f propagates
//! strictness — x is demanded because g will force it.

use std::collections::{HashMap, HashSet};
use crate::tir::*;

/// Per-function strictness info produced by the analysis.
pub struct DemandInfo {
    /// function name -> Vec<bool> indexed by parameter position.
    /// true = parameter is forced on every code path (strict).
    pub strict_params: HashMap<String, Vec<bool>>,
    /// Structured per-field / per-element demand rows (see `Rows`).
    pub rows: Rows,
}

// The legacy boolean analysis below computes the SEMANTIC notion of
// demand: what evaluating an expression to WHNF must force under Haskell
// semantics. It feeds `strict_params` (a strict parameter is forced at
// function entry, so this must never over-claim).
//
// The EMISSION notion — what the emitted Lua actually forces when a
// flattened bind chain runs, including per-tuple-field and per-list-element
// demand — lives in the structured `Demand`/`Rows` analysis further down,
// which codegen uses to decide which let/where bindings may be assigned
// strictly.

/// Compiler builtins with known per-argument strictness. These are not mata-ll
/// functions, so the fixed point below never derives a body for them; their
/// strict positions are stated here and seeded into the demand environment
/// keyed by the source name gen_arg looks up. Two families:
///   * ByteString primitives — first-order, strict in every argument (they
///     read a ByteString/Integer/String immediately; you cannot index/measure/
///     slice through a thunk).
///   * ST array primitives — strict in the array and index (always forced),
///     lazy in the stored value (matching Haskell's `newArray`/`writeArray`).
const STRICT_BUILTINS: &[(&str, &[bool])] = &[
    ("bsLength", &[true]),
    ("bsIndex", &[true, true]),
    ("bsSub", &[true, true, true]),
    ("bsNull", &[true]),
    ("bsHead", &[true]),
    ("bsTail", &[true]),
    ("bsCons", &[true, true]),
    ("bsSnoc", &[true, true]),
    ("bsConcat", &[true, true]),
    ("bsSingleton", &[true]),
    ("bsReplicate", &[true, true]),
    ("bsGetU16LE", &[true, true]),
    ("bsGetU32LE", &[true, true]),
    ("bsGetI8", &[true, true]),
    ("bsGetI16LE", &[true, true]),
    ("bsPutI16LE", &[true, true, true]),
    ("bsToString", &[true]),
    ("bsFromString", &[true]),
    // ST array primitives. An array and an index are always forced (you cannot
    // allocate/read/write/measure through a thunk); the *stored value* stays
    // lazy, matching Haskell's `newArray`/`writeArray`. These masks mirror the
    // fused `__mll_st_*` masks in codegen so the run-once and closure emission
    // paths agree. `modifySTArray`'s function argument is forced (it is called).
    ("newSTArray", &[true, false]),
    ("readSTArray", &[true, true]),
    ("writeSTArray", &[true, true, false]),
    ("modifySTArray", &[true, true, true]),
    ("stArrayLength", &[true]),
    ("newSTArrayFromList", &[true]),
    ("stArrayToList", &[true]),
    // List-consuming ByteString intrinsics: the runtime walks the whole
    // spine and forces every element (see the `__mll_bs` concatList/pack
    // implementations), so the list argument is forced at least to WHNF.
    // The structured rows below additionally record the element demand.
    ("bsConcatList", &[true]),
    ("bsPack", &[true]),
];

/// Monomorphized primitive typeclass methods that codegen inlines as Lua
/// binary operators (see the `lua_op` table in `gen_expr`) and whose runtime
/// fallbacks (`local function eq_Integer(a, b) … __force(a); __force(b) …`)
/// force both operands. They are strict in both arguments exactly like the
/// corresponding `InfixApp` operators — without this seed, every guard or
/// condition written with `==`/`<`/`>=` on a primitive type hides its
/// operands from the analysis (the comparison survives to TIR as
/// `App(App(Var("ord_ge__Integer"), a), b)`), which is what kept hot-loop
/// counters lazy (see examples/tracker/PERF-REGRESSION.md).
const PRIMITIVE_BINOP_METHODS: &[&str] = &[
    "eq_Integer", "eq_Number", "eq_String", "eq_Bool", "eq_ByteString",
    "ord_lt__Integer", "ord_lt__Number", "ord_lt__String", "ord_lt__ByteString",
    "ord_gt__Integer", "ord_gt__Number", "ord_gt__String", "ord_gt__ByteString",
    "ord_le__Integer", "ord_le__Number", "ord_le__String", "ord_le__ByteString",
    "ord_ge__Integer", "ord_ge__Number", "ord_ge__String", "ord_ge__ByteString",
    "semigroup_String",
];

/// Run demand analysis on a typed module with cross-function propagation.
/// Iterates to a fixed point: each round may discover new strict params
/// by looking through call sites to already-known-strict callees.
pub fn analyze(module: &TModule) -> DemandInfo {
    let functions: Vec<&TFunction> = module.functions.iter()
        .chain(module.instance_fns.iter())
        .collect();

    // Seed FFI functions as strict in all parameters.
    // FFI functions (LuaPure/LuaIO) always force their arguments via __force()
    // in the generated Lua code, so they are strict by construction.
    // An FFI function is identified by having a single clause whose body is a
    // SpecCall referencing the function's own name (the typechecker generates
    // these synthetic bodies for type signatures with LuaPure/LuaIO return types).
    let mut strict_params: HashMap<String, Vec<bool>> = HashMap::new();
    for func in &functions {
        if func.clauses.len() == 1 {
            let clause = &func.clauses[0];
            if let TExprKind::SpecCall { original, .. } = &clause.body.kind
                && original == &func.name && !clause.patterns.is_empty() {
                    strict_params.insert(func.name.clone(), vec![true; clause.patterns.len()]);
                    continue;
                }
        }
    }

    // Seed compiler builtins that are strict but are not mata-ll functions, so
    // the fixed point below never sees a body for them. The ByteString
    // primitives immediately consume their arguments — you cannot measure,
    // index, slice, or convert through a thunk — so every ByteString/Integer/
    // String argument is forced. Marking them strict lets gen_arg pass address
    // arithmetic like `off + 17` in place instead of thunking it (hot in binary
    // decoders such as the tracker). Higher-order primitives (bsMap, bsFoldl,
    // bsZipWith) are omitted: their accumulator/element positions may legitimately
    // stay lazy, so leaving them unseeded keeps the safe default.
    for (name, mask) in STRICT_BUILTINS {
        strict_params.entry((*name).to_string()).or_insert_with(|| mask.to_vec());
    }
    for name in PRIMITIVE_BINOP_METHODS {
        strict_params.entry((*name).to_string()).or_insert_with(|| vec![true, true]);
    }

    // Seed every non-FFI function optimistically — every parameter strict —
    // then iterate the demand equations DOWNWARD to their greatest fixed point.
    //
    // Why greatest, not least: the strictness of a self- or mutually-recursive
    // parameter is only provable under the assumption that the recursive calls
    // are already strict in it. The canonical case is a tail accumulator,
    //
    //     loop 0 acc = acc
    //     loop n acc = loop (n - 1) (acc + n)
    //
    // where `acc` is strict — forcing `loop`'s result forces `acc` through both
    // the base clause (`= acc`) and, inductively, the recursive `acc + n`. A
    // least fixpoint seeded with "nothing is strict" can never make that leap:
    // the recursive clause sees `loop` as non-strict in `acc`, so `acc + n`
    // contributes no demand, so `acc` stays lazy — and every hot loop then
    // builds a thunk chain in its accumulator. Seeding optimistically and
    // shrinking to the greatest fixed point discovers the accumulator is strict.
    //
    // Soundness: `analyze_function` is built on `demanded_vars` (Semantic mode),
    // a sound UNDER-approximation of "the variables forced when the body reaches
    // WHNF" — it forces only the left operand of `&&`/`||`/`++`/`$`, nothing of
    // `:`/tuples, the intersection of `if`/`case` branches, and nothing for
    // unknown operators. The greatest fixed point of a sound, monotone demand
    // function is the standard sound strictness result: a parameter is kept
    // strict only if forcing the result genuinely forces it, so evaluating it
    // eagerly at entry cannot introduce a bottom the callee would not itself
    // have produced. (A parameter of a call that is never entered is never
    // forced — a discarded lazy call is thunked and `loop` is simply not run.)
    for func in &functions {
        if func.clauses.is_empty() {
            continue;
        }
        if strict_params.contains_key(&func.name) {
            continue; // already seeded as FFI (all-strict, stays put)
        }
        let arity = func.clauses.iter().map(|c| c.patterns.len()).max().unwrap_or(0);
        strict_params.insert(func.name.clone(), vec![true; arity]);
    }

    // Iterate to the greatest fixed point. `analyze_function` is monotone in the
    // environment (a strictly larger strict-set can only demand more), so from
    // the ⊤ seed the strict-sets only shrink; the finite lattice guarantees
    // termination.
    // Group same-named functions before iterating: a user definition can
    // shadow a prelude one under the SAME name, and the environment is
    // name-keyed — a call site cannot be attributed to one of them, so the
    // shared entry must be the MEET (per-position AND) of every member's
    // row. Analyzing them as independent map writes instead makes the loop
    // oscillate forever the moment the members' rows differ.
    let mut fn_groups: Vec<(&str, Vec<&TFunction>)> = Vec::new();
    {
        let mut index: HashMap<&str, usize> = HashMap::new();
        for func in &functions {
            if func.clauses.is_empty() {
                continue;
            }
            match index.get(func.name.as_str()) {
                Some(&i) => fn_groups[i].1.push(func),
                None => {
                    index.insert(func.name.as_str(), fn_groups.len());
                    fn_groups.push((func.name.as_str(), vec![func]));
                }
            }
        }
    }

    loop {
        let mut changed = false;
        for (name, members) in &fn_groups {
            let mut new_strict: Option<Vec<bool>> = None;
            for func in members {
                let s = analyze_function(func, &strict_params);
                new_strict = Some(match new_strict {
                    None => s,
                    Some(prev) => {
                        // Positional AND; a position missing from either row
                        // (differing arities) is lazy.
                        let n = prev.len().min(s.len());
                        (0..n).map(|i| prev[i] && s[i]).collect()
                    }
                });
            }
            let Some(new_strict) = new_strict else { continue };
            if strict_params.get(*name) != Some(&new_strict) {
                strict_params.insert((*name).to_string(), new_strict);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let rows = analyze_rows(module, &strict_params);

    if std::env::var("MLL_DEBUG_DEMAND").is_ok() {
        let mut names: Vec<&String> = strict_params.keys().collect();
        names.sort();
        for n in names {
            eprintln!("DEMAND {} {:?} run={:?} deep={:?} deep_result={}",
                n, strict_params[n],
                rows.run.get(n.as_str()), rows.deep.get(n.as_str()),
                rows.deep_result.contains(n.as_str()));
        }
    }

    DemandInfo { strict_params, rows }
}

/// Analyze a single function's parameter strictness.
fn analyze_function(func: &TFunction, env: &HashMap<String, Vec<bool>>) -> Vec<bool> {
    let clauses = &func.clauses;
    if clauses.is_empty() {
        return vec![];
    }

    let arity = clauses[0].patterns.len();
    if arity == 0 {
        return vec![];
    }

    // For each parameter position, determine strictness across all clauses.
    let mut strict = vec![true; arity];

    for clause in clauses {
        let clause_strict = analyze_clause(clause, arity, env);
        // A parameter is strict only if it's strict in ALL clauses.
        for i in 0..arity {
            strict[i] = strict[i] && clause_strict[i];
        }
    }

    strict
}

/// Analyze a single clause's parameter strictness.
fn analyze_clause(clause: &TClause, arity: usize, env: &HashMap<String, Vec<bool>>) -> Vec<bool> {
    let mut strict = vec![false; arity];

    // Collect parameter names from patterns.
    // Constructor/LitPat/Tuple patterns force the parameter (pattern dispatch).
    let mut param_names: Vec<Option<String>> = Vec::with_capacity(arity);

    for (i, pat) in clause.patterns.iter().enumerate() {
        match pat {
            TPattern::Var(name, _) => {
                param_names.push(Some(name.clone()));
                // Not strict from pattern alone — depends on body usage.
            }
            TPattern::Wildcard => {
                param_names.push(None);
                // Wildcard is never strict.
            }
            TPattern::Constructor { .. } | TPattern::LitPat(_) | TPattern::Tuple(_) => {
                param_names.push(None);
                // Pattern matching forces evaluation.
                strict[i] = true;
            }
            TPattern::Paren(inner) => {
                match inner.as_ref() {
                    TPattern::Var(name, _) => {
                        param_names.push(Some(name.clone()));
                    }
                    _ => {
                        param_names.push(None);
                        strict[i] = true;
                    }
                }
            }
        }
    }

    // Compute demanded variables from the body (and guards), closing every
    // intermediate set over the clause's where-bound VALUES: a demanded
    // where-binding is forced, so whatever its right-hand side demands is
    // demanded too (`f x | v >= 128 = … where v = g x` is strict in x).
    // The closure must run BEFORE guard sets are intersected — two guards
    // that demand the same parameter through different where-bindings (or
    // one directly and one through a binding) still make it strict.
    let close = |mut s: HashSet<String>| -> HashSet<String> {
        loop {
            let mut changed = false;
            for b in &clause.where_binds {
                if b.patterns.is_empty() && s.contains(&b.name) {
                    for v in demanded_vars(&b.body, env) {
                        if s.insert(v) {
                            changed = true;
                        }
                    }
                }
            }
            if !changed {
                return s;
            }
        }
    };
    let demanded = if clause.guards.is_empty() {
        close(demanded_vars(&clause.body, env))
    } else {
        demanded_guards_with(&clause.guards, env, &close)
    };
    // (Both are Semantic-mode: parameter strictness must not over-claim.)

    // Mark parameters whose names appear in the demanded set.
    for (i, name) in param_names.iter().enumerate() {
        if let Some(n) = name
            && demanded.contains(n) {
                strict[i] = true;
            }
    }

    strict
}

/// Compute demanded variables from a set of guards.
///
/// Guards evaluate *sequentially*: the first condition always runs; a body
/// runs only if its condition was true; a later condition runs only if all
/// earlier ones were false. So the demand of a guard chain is
/// `demand(c1) ∪ (demand(b1) ∩ demand(rest))`,
/// computed right-to-left, where the demand past the last guard is empty
/// (the whole chain can fall through to the next clause, demanding
/// nothing) — unless a guard's condition is `otherwise`, which makes its
/// body unconditional at that point. Unioning *all* conditions (the old
/// rule) over-claimed: a variable read only by a later guard condition was
/// forced at entry even when an earlier guard matched and GHC would never
/// have touched it.
pub fn demanded_guards(guards: &[TGuard], env: &HashMap<String, Vec<bool>>) -> HashSet<String> {
    demanded_guards_with(guards, env, &|s| s)
}

/// `demanded_guards` with a closure applied to every per-guard demand set
/// BEFORE the chain combines them — used to expand where-bound values so
/// the guard intersection sees through them (see analyze_clause).
fn demanded_guards_with(
    guards: &[TGuard],
    env: &HashMap<String, Vec<bool>>,
    close: &dyn Fn(HashSet<String>) -> HashSet<String>,
) -> HashSet<String> {
    // Demand past the end of the chain: fallthrough demands nothing.
    let mut acc: HashSet<String> = HashSet::new();
    for g in guards.iter().rev() {
        let body_d = close(demanded_vars(&g.body, env));
        let is_otherwise = matches!(&g.condition.kind, TExprKind::Var(n) if n == "otherwise");
        acc = if is_otherwise {
            // Condition is `true`: the body runs unconditionally here.
            body_d
        } else {
            let mut s = close(demanded_vars(&g.condition, env));
            s.extend(&body_d & &acc);
            s
        };
    }
    acc
}

/// Arguments that codegen's `gen_arg` evaluates eagerly at *every* call site,
/// regardless of the callee's strictness — the context-free floor of
/// `is_cheap_to_force`. Emission mode adds the demand of such an argument to
/// the forced set, so this MUST stay a subset of what the emitter actually
/// evaluates eagerly; otherwise a `let`/`where` binding could be judged
/// demanded and evaluated strictly when the emitter in fact thunks the
/// argument, forcing a value the program never demands.
///
/// Since the change to a bottom-safe weighing in `gen_arg`, that floor no
/// longer includes a bare variable (a non-concrete variable is passed as its
/// raw thunk-or-value, not forced) nor a trapping `div`/`mod`/`%` (which may be
/// ⊥). What remains is genuinely total: literals, nullary constructors,
/// lambdas, and non-trapping arithmetic / constructor / tuple / if structure
/// built from those. (Constructors and tuples force nothing when built, so
/// their demand is empty regardless; they are included only for structural
/// completeness.)
fn arg_emitted_eagerly(expr: &TExpr) -> bool {
    match &expr.kind {
        TExprKind::Lit(_) | TExprKind::Con(_)
        | TExprKind::Lambda { .. } | TExprKind::OpFunc(_) => true,
        // A bare variable is NOT forced eagerly by gen_arg any more.
        TExprKind::Var(_) => false,
        TExprKind::Paren(inner) | TExprKind::Negate(inner) => arg_emitted_eagerly(inner),
        TExprKind::Tuple(elems) => elems.iter().all(arg_emitted_eagerly),
        TExprKind::InfixApp { op, lhs, rhs } => {
            // Trapping ops (div/mod/%) are excluded — gen_arg thunks them.
            matches!(op.as_str(), "+" | "-" | "*" | "/" | "^" | "==" | "/=" | "~="
                | "<" | ">" | "<=" | ">=" | "++" | "<>" | "&&" | "||" | ".." | "$" | ".")
                && arg_emitted_eagerly(lhs) && arg_emitted_eagerly(rhs)
        }
        TExprKind::App(func, arg) => {
            let mut f = expr;
            while let TExprKind::App(inner, _) = &f.kind { f = inner; }
            matches!(&f.kind, TExprKind::Con(_))
                && arg_emitted_eagerly(arg) && arg_emitted_eagerly(func)
        }
        TExprKind::If { cond, then_branch, else_branch } => {
            arg_emitted_eagerly(cond)
                && arg_emitted_eagerly(then_branch)
                && arg_emitted_eagerly(else_branch)
        }
        _ => false,
    }
}

/// Core analysis: returns the set of free variables that are guaranteed
/// to be forced when `expr` is evaluated to WHNF.
///
/// `env` contains known strictness info for other functions, enabling
/// cross-function demand propagation.
///
/// Also used by codegen to decide which let/where bindings may be
/// evaluated eagerly: a binding demanded by the let body will be forced
/// anyway, so evaluating it at binding time is sound (GHC's let-to-case).
pub fn demanded_vars(expr: &TExpr, env: &HashMap<String, Vec<bool>>) -> HashSet<String> {
    let rec = |e: &TExpr| demanded_vars(e, env);
    match &expr.kind {
        TExprKind::Var(x) => {
            let mut s = HashSet::new();
            s.insert(x.clone());
            s
        }

        TExprKind::Lit(_) | TExprKind::Con(_) | TExprKind::OpFunc(_) => {
            HashSet::new()
        }

        TExprKind::Lambda { .. } => {
            // Lambda body is deferred — no demands.
            HashSet::new()
        }

        TExprKind::App(_, _) => {
            // Flatten curried application: f x y z → (f, [x, y, z])
            let mut f = expr;
            let mut args_rev = Vec::new();
            while let TExprKind::App(func, arg) = &f.kind {
                args_rev.push(arg.as_ref());
                f = func.as_ref();
            }
            args_rev.reverse();

            // Always demand the function.
            let mut s = rec(f);

            // `seq` is strict in its FIRST argument only (it forces it to WHNF)
            // and lazy in the rest, so a parameter passed as `seq`'s first
            // argument is demanded, while the second stays lazy. Mirror the
            // runtime `__mll_seq` / inline lowering exactly.
            if let TExprKind::Var(name) = &f.kind
                && name == "seq" && !args_rev.is_empty() {
                    s.extend(rec(args_rev[0]));
                }

            // Cross-function propagation: if callee is a known function
            // and is strict in position i, demand that argument's vars.
            if let TExprKind::Var(name) = &f.kind
                && let Some(callee_strict) = env.get(name) {
                    for (i, arg) in args_rev.iter().enumerate() {
                        if callee_strict.get(i).copied().unwrap_or(false) {
                            s.extend(rec(arg));
                        }
                    }
                }

            s
        }

        TExprKind::InfixApp { op, lhs, rhs } => {
            match op.as_str() {
                // Arithmetic/comparison operators force both sides.
                "+" | "-" | "*" | "/" | "^" | "div" | "mod"
                | "==" | "/=" | "<" | ">" | "<=" | ">=" => {
                    let mut s = rec(lhs);
                    s.extend(rec(rhs));
                    s
                }
                // Short-circuit: the right side runs only when the left
                // allows it (Lua `and`/`or`; GHC agrees).
                "&&" | "||" => rec(lhs),
                // List append is lazy in its right side (codegen thunks
                // it); only the left spine is forced.
                "++" => rec(lhs),
                // <> on String is Lua string concat (both sides forced);
                // on lists it behaves like ++ (right side thunked).
                "<>" => {
                    if matches!(&lhs.ty, crate::types::Ty::Con(n) if n == "String") {
                        let mut s = rec(lhs);
                        s.extend(rec(rhs));
                        s
                    } else {
                        rec(lhs)
                    }
                }
                // $ forces the function (lhs) but thunks the argument.
                "$" => rec(lhs),
                // Cons is lazy — neither side is forced.
                ":" => HashSet::new(),
                // Backtick `a `seq` b`: forces the FIRST operand only (WHNF),
                // leaves the second lazy — same as prefix `seq a b`.
                "seq" => rec(lhs),
                // Monadic bind/sequence forces both actions.
                ">>=" | ">>" => {
                    let mut s = rec(lhs);
                    s.extend(rec(rhs));
                    s
                }
                // Unknown operator — claim nothing (an over-claim here
                // would let a lazy value be forced eagerly).
                _ => HashSet::new(),
            }
        }

        TExprKind::Negate(e) => rec(e),

        TExprKind::Paren(e) => rec(e),

        TExprKind::If { cond, then_branch, else_branch } => {
            let mut s = rec(cond);
            // Only demanded if demanded in BOTH branches.
            let t = rec(then_branch);
            let e = rec(else_branch);
            s.extend(&t & &e);
            s
        }

        TExprKind::Case { scrutinee, branches } => {
            let mut s = rec(scrutinee);
            if !branches.is_empty() {
                // Intersect demanded vars across all branches
                // (minus variables bound by each branch's pattern).
                let mut branch_iter = branches.iter().map(|b| {
                    let body_demanded = if b.guards.is_empty() {
                        rec(&b.body)
                    } else {
                        demanded_guards(&b.guards, env)
                    };
                    let bound = pattern_bound_vars(&b.pattern);
                    // Remove locally bound names.
                    body_demanded.difference(&bound).cloned().collect::<HashSet<_>>()
                });
                if let Some(first) = branch_iter.next() {
                    let intersection = branch_iter.fold(first, |acc, s| &acc & &s);
                    s.extend(intersection);
                }
            }
            s
        }

        TExprKind::Let { binds, body } => {
            let mut body_demanded = rec(body);
            let bound_names: HashSet<String> = binds.iter()
                .map(|b| b.name.clone())
                .collect();

            // If the body demands a let-bound variable, the variables
            // demanded by that binding's definition are also demanded.
            for bind in binds {
                if body_demanded.contains(&bind.name) {
                    body_demanded.extend(rec(&bind.body));
                }
            }

            // Remove the let-bound names themselves.
            for name in &bound_names {
                body_demanded.remove(name);
            }
            body_demanded
        }

        TExprKind::Tuple(_) => {
            // Tuples are lazy — constructing one doesn't force elements.
            HashSet::new()
        }

        TExprKind::SpecCall { original, args, .. } => {
            // Specialized call: look up the original function's strictness.
            let mut s = HashSet::new();
            if let Some(callee_strict) = env.get(original.as_str()) {
                for (i, arg) in args.iter().enumerate() {
                    if callee_strict.get(i).copied().unwrap_or(false) {
                        s.extend(rec(arg));
                    }
                }
            }
            s
        }

        TExprKind::DictCall { func_name, value_args, .. } => {
            // Typeclass method call: look up the method's strictness.
            let mut s = HashSet::new();
            if let Some(callee_strict) = env.get(func_name.as_str()) {
                for (i, arg) in value_args.iter().enumerate() {
                    if callee_strict.get(i).copied().unwrap_or(false) {
                        s.extend(rec(arg));
                    }
                }
            }
            s
        }

        TExprKind::DictMethod { dict, .. } => rec(dict),

        TExprKind::DictAccess { .. } => {
            HashSet::new()
        }

        TExprKind::RecordUpdate { record, updates, .. } => {
            // Codegen copies the record and assigns the update expressions
            // eagerly (gen_expr), so both count in either mode.
            let mut s = rec(record);
            for (_, _, e) in updates {
                s.extend(rec(e));
            }
            s
        }

        TExprKind::OutgoingCallback { callee, .. } => {
            // The wrapped callback is invoked by the host, so it is demanded.
            rec(callee)
        }

        TExprKind::FfiMaybeArg { value } => {
            // The optional FFI argument is forced/unwrapped at the boundary,
            // so it is demanded exactly like a plain FFI argument.
            rec(value)
        }
    }
}

/// Collect all variable names bound by a pattern.
fn pattern_bound_vars(pat: &TPattern) -> HashSet<String> {
    let mut vars = HashSet::new();
    collect_pattern_vars(pat, &mut vars);
    vars
}

fn collect_pattern_vars(pat: &TPattern, vars: &mut HashSet<String>) {
    match pat {
        TPattern::Var(name, _) => { vars.insert(name.clone()); }
        TPattern::Wildcard | TPattern::LitPat(_) => {}
        TPattern::Constructor { args, .. } => {
            for a in args { collect_pattern_vars(a, vars); }
        }
        TPattern::Paren(inner) => collect_pattern_vars(inner, vars),
        TPattern::Tuple(elems) => {
            for e in elems { collect_pattern_vars(e, vars); }
        }
    }
}

// ════════════════════════════════════════════════════════════════════════
// Structured (per-field / per-element) demand analysis
// ════════════════════════════════════════════════════════════════════════
//
// The boolean analysis above answers "is this parameter forced to WHNF?".
// That is too coarse for values that are only ever forced THROUGH a
// structure: a tuple returned by an ST action whose fields every caller
// scrutinizes, or a list accumulator whose every element a `bsConcatList`
// eventually flattens. Under whole-value demand those are "not demanded",
// so codegen must thunk them — which is what turned the tracker's mixer
// loop into a closure-allocating (and therefore LuaJIT-unJITtable) loop;
// see examples/tracker/PERF-REGRESSION.md.
//
// This section computes, for every function, structured demand ROWS:
//
//   run row   — the demand each parameter receives whenever the function
//               RUNS (for an action-returning function: when its flattened
//               bind chain executes; for a pure function: when it is
//               called) and its result is demanded to WHNF.
//   deep row  — the same, under the additional assumption that the
//               function's RESULT is demanded "deeply" for its type:
//               every tuple field forced (for a tuple result), the full
//               spine and every element forced (for a list result).
//
// plus the whole-program set `deep_result`: functions for which EVERY
// reference in the program is a fully-applied call whose result provably
// receives the deep demand. For such a function, its own body may be
// analyzed (and code-generated) under the deep result demand — this is
// what lets a tail-recursive accumulator that is only "used" by being
// returned in a tuple field (the mixer's `nl`/`nr`) be proven demanded.
//
// Emission semantics: rows describe what the EMITTED code forces when the
// function runs. Like the codegen that consumes them, they assume the
// continuation of a flattened bind chain runs once the chain starts (an
// earlier statement can only abort by raising, i.e. replacing one ⊥ by
// another), and they include gen_arg's context-free eager floor
// (`arg_emitted_eagerly`). Both assumptions mirror the previous
// Emission-mode analysis this section replaces.
//
// Soundness stance (unchanged from the boolean analysis): "demanded" may
// only be claimed when the value is forced on every run that reaches its
// binding, up to ⊥-for-⊥ substitution — evaluating a demanded binding
// early can surface a different bottom (or an error instead of divergence)
// but can never introduce a force GHC's semantics would not perform on a
// completing run. A demand is NEVER claimed speculatively: lazy argument
// positions, suspended (not-run) actions, and branch-dependent uses all
// degrade to "no demand".

use crate::types::Ty;

/// Structured demand on a single value. Absence (an `Option<Demand>` of
/// `None`, or a variable missing from a demand map) means LAZY — nothing
/// may be forced. Every present demand implies at least WHNF.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Demand {
    /// Forced to WHNF only.
    Head,
    /// Forced to WHNF, and each tuple field forced with the given demand
    /// (`None` = that field stays lazy).
    Fields(Vec<Option<Demand>>),
    /// The full list spine is forced, and every element is forced with the
    /// given demand.
    Elems(Box<Demand>),
}

impl Demand {
    /// `self` subsumes `other`: any run satisfying `self` also satisfies
    /// `other`. Every demand subsumes `Head`.
    pub fn subsumes(&self, other: &Demand) -> bool {
        match (self, other) {
            (_, Demand::Head) => true,
            (Demand::Fields(a), Demand::Fields(b)) => {
                b.iter().enumerate().all(|(i, db)| match db {
                    None => true,
                    Some(db) => matches!(a.get(i), Some(Some(da)) if da.subsumes(db)),
                })
            }
            (Demand::Elems(a), Demand::Elems(b)) => a.subsumes(b),
            _ => false,
        }
    }

    /// Join: both demands occur on the same run — the combined demand is
    /// at least as deep as each. On a shape mismatch (which well-typed
    /// programs do not produce) degrade to `Head`, which under-claims and
    /// is therefore safe.
    fn join(&self, other: &Demand) -> Demand {
        match (self, other) {
            (Demand::Head, d) | (d, Demand::Head) => d.clone(),
            (Demand::Fields(a), Demand::Fields(b)) => {
                let n = a.len().max(b.len());
                Demand::Fields((0..n).map(|i| {
                    match (a.get(i).cloned().flatten(), b.get(i).cloned().flatten()) {
                        (Some(x), Some(y)) => Some(x.join(&y)),
                        (Some(x), None) | (None, Some(x)) => Some(x),
                        (None, None) => None,
                    }
                }).collect())
            }
            (Demand::Elems(a), Demand::Elems(b)) => Demand::Elems(Box::new(a.join(b))),
            _ => Demand::Head,
        }
    }

    /// Meet: only one of the two demands is guaranteed (e.g. different
    /// branches) — keep what both include. Any two demands share `Head`.
    fn meet(&self, other: &Demand) -> Demand {
        match (self, other) {
            (Demand::Head, _) | (_, Demand::Head) => Demand::Head,
            (Demand::Fields(a), Demand::Fields(b)) => {
                let n = a.len().max(b.len());
                Demand::Fields((0..n).map(|i| {
                    match (a.get(i).and_then(|x| x.as_ref()), b.get(i).and_then(|x| x.as_ref())) {
                        (Some(x), Some(y)) => Some(x.meet(y)),
                        _ => None,
                    }
                }).collect())
            }
            (Demand::Elems(a), Demand::Elems(b)) => Demand::Elems(Box::new(a.meet(b))),
            _ => Demand::Head,
        }
    }
}

fn opt_meet(a: Option<&Demand>, b: Option<&Demand>) -> Option<Demand> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.meet(y)),
        _ => None,
    }
}

/// A demand map: variable name → demand placed on it. Missing = lazy.
pub type DemandMap = HashMap<String, Demand>;

fn map_join_one(map: &mut DemandMap, name: &str, d: Demand) {
    match map.get_mut(name) {
        Some(existing) => { *existing = existing.join(&d); }
        None => { map.insert(name.to_string(), d); }
    }
}

pub fn map_join(into: &mut DemandMap, from: DemandMap) {
    for (k, v) in from {
        map_join_one(into, &k, v);
    }
}

/// Pointwise meet: keeps only variables demanded in BOTH maps.
fn map_meet(a: &DemandMap, b: &DemandMap) -> DemandMap {
    let mut out = DemandMap::new();
    for (k, va) in a {
        if let Some(vb) = b.get(k) {
            out.insert(k.clone(), va.meet(vb));
        }
    }
    out
}

/// Structured demand rows for every analyzed function, plus the
/// whole-program deep-result set. See the section comment above.
pub struct Rows {
    /// name → per-parameter demand when the function runs and its result
    /// is demanded to WHNF. `None` = parameter stays lazy.
    pub run: HashMap<String, Vec<Option<Demand>>>,
    /// name → per-parameter demand under the deep result demand. Only
    /// present for functions whose result type has a deep demand > Head.
    pub deep: HashMap<String, Vec<Option<Demand>>>,
    /// Functions for which EVERY program reference is a fully-applied call
    /// whose result receives the deep demand of its type — so the
    /// function's own body may be generated under that result demand.
    pub deep_result: HashSet<String>,
    /// name → deep demand of the function's (action-stripped) result type,
    /// for functions where that is more than `Head`.
    result_deep: HashMap<String, Demand>,
    /// name → arity (max clause pattern count), for full-application checks.
    arity: HashMap<String, usize>,
}

impl Default for Rows {
    fn default() -> Self {
        Rows {
            run: HashMap::new(),
            deep: HashMap::new(),
            deep_result: HashSet::new(),
            result_deep: HashMap::new(),
            arity: HashMap::new(),
        }
    }
}

impl Rows {
    /// The result demand codegen must assume for `name`'s own body: the
    /// deep demand of its result type when every call site provably
    /// applies it, plain WHNF otherwise.
    pub fn result_demand(&self, name: &str) -> Demand {
        if self.deep_result.contains(name) {
            self.result_deep.get(name).cloned().unwrap_or(Demand::Head)
        } else {
            Demand::Head
        }
    }
}

/// Strip IO/LuaIO/ST wrappers and foralls off a type: the demand of
/// interest for an action is the demand on the value the action yields.
fn strip_action_ty(ty: &Ty) -> &Ty {
    match ty {
        Ty::IO(t) => strip_action_ty(t),
        Ty::LuaIO(_, t) => strip_action_ty(t),
        Ty::Forall(_, t) => strip_action_ty(t),
        Ty::App(f, t) => {
            if let Ty::App(c, _) = f.as_ref()
                && matches!(c.as_ref(), Ty::Con(n) if n == "ST") {
                    return strip_action_ty(t);
                }
            ty
        }
        _ => ty,
    }
}

/// The deepest demand this analysis models for a value of type `ty`:
/// tuples get all fields forced (recursively), lists get spine + elements.
/// Everything else — including data constructors, whose fields the
/// analysis does not track — is `Head`.
pub fn deep_of_ty(ty: &Ty) -> Demand {
    match strip_action_ty(ty) {
        Ty::Tuple(ts) => Demand::Fields(ts.iter().map(|t| Some(deep_of_ty(t))).collect()),
        Ty::List(t) => Demand::Elems(Box::new(deep_of_ty(t))),
        _ => Demand::Head,
    }
}

/// Whether a type is (or, once fully applied, could be) an IO/ST action
/// value. Mirrors codegen's `is_nullary_action_type`: such an expression in
/// a non-run position is SUSPENDED by the emitter, so no argument demand
/// fires until the action actually runs.
fn is_action_value_ty(ty: &Ty) -> bool {
    match ty {
        Ty::IO(_) | Ty::LuaIO(_, _) => true,
        Ty::App(f, _) => matches!(f.as_ref(),
            Ty::App(c, _) if matches!(c.as_ref(), Ty::Con(n) if n == "ST")),
        Ty::Forall(_, t) => is_action_value_ty(t),
        _ => false,
    }
}

/// Argument demands of a fully-applied ST array intrinsic in RUN position
/// (the fused `__mll_st_*` form). The fused runtime forces the array, the
/// index, AND the stored value / initializer on execution (`__mll_st_write`
/// does `… = __force(val)` — that is what keeps every slot, and hence every
/// `__mll_st_read` result, in WHNF). So in a position where the intrinsic
/// provably runs, its value argument is demanded. First-class (suspended)
/// intrinsic references keep the lazy-value masks of `STRICT_BUILTINS`.
fn st_intrinsic_run_row(name: &str, argc: usize) -> Option<Vec<Option<Demand>>> {
    let row: &[Demand] = match name {
        "newSTArray" => &[Demand::Head, Demand::Head],
        "readSTArray" => &[Demand::Head, Demand::Head],
        "writeSTArray" => &[Demand::Head, Demand::Head, Demand::Head],
        "modifySTArray" => &[Demand::Head, Demand::Head, Demand::Head],
        "stArrayLength" => &[Demand::Head],
        "newSTArrayFromList" => &[Demand::Elems(Box::new(Demand::Head))],
        "stArrayToList" => &[Demand::Head],
        _ => return None,
    };
    if argc == row.len() {
        Some(row.iter().cloned().map(Some).collect())
    } else {
        None
    }
}

/// Signatures of a clause's where-bound local functions (grouped defs).
struct LocalRows {
    run: Vec<Option<Demand>>,
    deep: Option<Vec<Option<Demand>>>,
    result_deep: Demand,
}

/// Everything the structured walker needs to look up.
struct RowCx<'a> {
    rows: &'a Rows,
    locals: &'a HashMap<String, LocalRows>,
    inlined: &'a dyn Fn(&str) -> bool,
    /// When present, records every fully-applied call-head visit:
    /// Var-node address → (callee, joined result demand, fully applied).
    sites: Option<&'a std::cell::RefCell<HashMap<usize, (String, Demand, bool)>>>,
}

impl<'a> RowCx<'a> {
    /// Pick the parameter row for a call to `name` whose result receives
    /// demand `rd`: the deep row when the site's demand subsumes the
    /// callee's deep result demand, the run row otherwise.
    fn callee_row(&self, name: &str, rd: &Demand) -> Option<Vec<Option<Demand>>> {
        if let Some(l) = self.locals.get(name) {
            if let Some(deep) = &l.deep
                && rd.subsumes(&l.result_deep) {
                    return Some(deep.clone());
                }
            return Some(l.run.clone());
        }
        if let Some(deep_row) = self.rows.deep.get(name)
            && let Some(rdeep) = self.rows.result_deep.get(name)
                && rd.subsumes(rdeep) {
                    return Some(deep_row.clone());
                }
        self.rows.run.get(name).cloned()
    }

    fn record_site(&self, head: &TExpr, name: &str, rd: &Demand, full: bool) {
        if let Some(sites) = self.sites
            && (self.rows.arity.contains_key(name) || self.locals.contains_key(name)) {
                let key = head as *const TExpr as usize;
                let mut sites = sites.borrow_mut();
                match sites.get_mut(&key) {
                    Some((_, d, f)) => { *d = d.join(rd); *f = *f && full; }
                    None => { sites.insert(key, (name.to_string(), rd.clone(), full)); }
                }
            }
    }
}

/// The demand a pattern match places on the matched value, given the
/// demands the branch body places on the pattern's bound variables.
///
/// - a literal or nullary-constructor match fully forces the value (an
///   empty list's spine and elements are vacuously forced, hence
///   `Elems(Head)` for `[]` — this is what lets a `go acc []`/`go acc
///   (x:xs)` recursion be recognized as element-strict);
/// - a cons match yields `Elems` only when the tail is itself demanded as
///   `Elems` and the head is demanded, i.e. every present element is
///   provably forced;
/// - a tuple match records per-field demands.
fn pattern_demand(pat: &TPattern, body: &DemandMap) -> Option<Demand> {
    match pat {
        TPattern::Var(name, _) => body.get(name).cloned(),
        TPattern::Wildcard => None,
        TPattern::LitPat(_) => Some(Demand::Head),
        TPattern::Paren(inner) => pattern_demand(inner, body),
        TPattern::Tuple(ps) => Some(Demand::Fields(
            ps.iter().map(|p| pattern_demand(p, body)).collect(),
        )),
        TPattern::Constructor { name, args } => {
            if name == "[]" && args.is_empty() {
                return Some(Demand::Elems(Box::new(Demand::Head)));
            }
            if name == ":" && args.len() == 2 {
                let dh = pattern_demand(&args[0], body);
                let dt = pattern_demand(&args[1], body);
                if let (Some(dh), Some(Demand::Elems(de))) = (dh, dt) {
                    return Some(Demand::Elems(Box::new(dh.meet(&de))));
                }
                return Some(Demand::Head);
            }
            Some(Demand::Head)
        }
    }
}

/// Structured-demand walker: the demands the emitted code places on free
/// variables when `expr` is evaluated with demand `rd` on its value.
/// `run_pos` marks action-run position — the statement/terminal spots of a
/// flattened bind chain, where an action-typed expression executes rather
/// than being suspended.
fn demand_expr(cx: &RowCx, expr: &TExpr, rd: &Demand, run_pos: bool) -> DemandMap {
    let head = |e: &TExpr| demand_expr(cx, e, &Demand::Head, false);
    match &expr.kind {
        TExprKind::Var(x) => {
            let mut m = DemandMap::new();
            m.insert(x.clone(), rd.clone());
            m
        }

        TExprKind::Lit(_) | TExprKind::Con(_) | TExprKind::OpFunc(_)
        | TExprKind::Lambda { .. } | TExprKind::DictAccess { .. } => DemandMap::new(),

        TExprKind::Paren(e) => demand_expr(cx, e, rd, run_pos),
        TExprKind::Negate(e) => head(e),

        TExprKind::Tuple(elems) => {
            let mut m = DemandMap::new();
            if let Demand::Fields(ds) = rd {
                for (i, e) in elems.iter().enumerate() {
                    if let Some(Some(d)) = ds.get(i).map(|x| x.as_ref()) {
                        map_join(&mut m, demand_expr(cx, e, d, false));
                    }
                }
            }
            m
        }

        TExprKind::App(_, _) => demand_app(cx, expr, rd, run_pos),

        TExprKind::InfixApp { op, lhs, rhs } => match op.as_str() {
            "+" | "-" | "*" | "/" | "^" | "div" | "mod"
            | "==" | "/=" | "<" | ">" | "<=" | ">=" => {
                let mut m = head(lhs);
                map_join(&mut m, head(rhs));
                m
            }
            "&&" | "||" => head(lhs),
            "++" => head(lhs),
            "<>" => {
                if matches!(&lhs.ty, Ty::Con(n) if n == "String") {
                    let mut m = head(lhs);
                    map_join(&mut m, head(rhs));
                    m
                } else {
                    head(lhs)
                }
            }
            // `return $ x` / `pure $ x` in run position yields x with the
            // result demand (nothing at plain WHNF — return is non-strict);
            // any other `f $ x` forces the function only.
            "$" => {
                if matches!(&lhs.kind, TExprKind::Var(n) if n == "pure" || n == "return") {
                    if run_pos && *rd != Demand::Head {
                        demand_expr(cx, rhs, rd, false)
                    } else {
                        DemandMap::new()
                    }
                } else {
                    head(lhs)
                }
            }
            // Cons forces nothing at WHNF; under an element demand the head
            // is an element and the tail carries the same element demand.
            ":" => {
                if let Demand::Elems(de) = rd {
                    let mut m = demand_expr(cx, lhs, de, false);
                    map_join(&mut m, demand_expr(cx, rhs, rd, false));
                    m
                } else {
                    DemandMap::new()
                }
            }
            // `a seq b` forces a to WHNF and yields b: b carries the whole
            // expression's demand.
            "seq" => {
                let mut m = head(lhs);
                map_join(&mut m, demand_expr(cx, rhs, rd, run_pos));
                m
            }
            // Flattened bind chain: the left action runs, then the
            // continuation runs (an earlier raise only replaces one ⊥ with
            // another). The bound variable's demand in the continuation IS
            // the result demand placed on the left action.
            ">>=" => {
                if let TExprKind::Lambda { params, body } = &rhs.kind {
                    let mut rest = demand_expr(cx, body, rd, run_pos);
                    let dp = if params.len() == 1 {
                        rest.get(&params[0].0).cloned()
                    } else {
                        None
                    };
                    for (p, _) in params {
                        rest.remove(p);
                    }
                    let mut m = demand_expr(cx, lhs, dp.as_ref().unwrap_or(&Demand::Head), true);
                    map_join(&mut m, rest);
                    m
                } else {
                    let mut m = demand_expr(cx, lhs, &Demand::Head, true);
                    map_join(&mut m, head(rhs));
                    m
                }
            }
            ">>" => {
                let mut m = demand_expr(cx, lhs, &Demand::Head, true);
                map_join(&mut m, demand_expr(cx, rhs, rd, run_pos));
                m
            }
            _ => DemandMap::new(),
        },

        TExprKind::If { cond, then_branch, else_branch } => {
            let mut m = head(cond);
            let t = demand_expr(cx, then_branch, rd, run_pos);
            let e = demand_expr(cx, else_branch, rd, run_pos);
            map_join(&mut m, map_meet(&t, &e));
            m
        }

        TExprKind::Case { scrutinee, branches } => {
            let mut m = head(scrutinee);
            // The variable scrutinized (if any) receives whatever demand
            // the branch patterns place on it beyond WHNF.
            let scrut_var = {
                let mut s = scrutinee.as_ref();
                while let TExprKind::Paren(inner) = &s.kind { s = inner.as_ref(); }
                match &s.kind {
                    TExprKind::Var(v) => Some(v.clone()),
                    _ => None,
                }
            };
            let mut branch_maps = branches.iter().map(|b| {
                let mut bm = if b.guards.is_empty() {
                    demand_expr(cx, &b.body, rd, run_pos)
                } else {
                    demand_guards_map(cx, &b.guards, rd, run_pos)
                };
                let pat_d = pattern_demand(&b.pattern, &bm);
                for v in pattern_bound_vars(&b.pattern) {
                    bm.remove(&v);
                }
                if let (Some(v), Some(d)) = (&scrut_var, pat_d) {
                    map_join_one(&mut bm, v, d);
                }
                bm
            });
            if let Some(first) = branch_maps.next() {
                let isect = branch_maps.fold(first, |acc, bm| map_meet(&acc, &bm));
                map_join(&mut m, isect);
            }
            m
        }

        TExprKind::Let { binds, body } => {
            let mut m = demand_expr(cx, body, rd, run_pos);
            // Pull in the demands of demanded value bindings, re-walking a
            // binding when its own demand deepens. Terminates: demands only
            // deepen and the lattice is finite for finite programs.
            let mut walked: HashMap<&str, Demand> = HashMap::new();
            loop {
                let mut changed = false;
                for b in binds {
                    if !b.patterns.is_empty() {
                        continue; // local function definitions — no row here
                    }
                    if let Some(d) = m.get(&b.name).cloned() {
                        let redo = match walked.get(b.name.as_str()) {
                            Some(prev) => !prev.subsumes(&d),
                            None => true,
                        };
                        if redo {
                            walked.insert(b.name.as_str(), d.clone());
                            map_join(&mut m, demand_expr(cx, &b.body, &d, false));
                            changed = true;
                        }
                    }
                }
                if !changed {
                    break;
                }
            }
            for b in binds {
                m.remove(&b.name);
            }
            m
        }

        TExprKind::SpecCall { original, specialized, args } => {
            let mut m = DemandMap::new();
            // Tuple projection: the argument's projected field carries the
            // whole expression's demand.
            if let Some(idx) = specialized.strip_prefix("__mll_tup_get:")
                && let Ok(idx) = idx.parse::<usize>()
                    && idx >= 1 && args.len() == 1 {
                        let mut fields: Vec<Option<Demand>> = vec![None; idx];
                        fields[idx - 1] = Some(rd.clone());
                        map_join(&mut m, demand_expr(cx, &args[0], &Demand::Fields(fields), false));
                        return m;
                    }
            if let Some(row) = cx.callee_row(original, &Demand::Head) {
                for (i, arg) in args.iter().enumerate() {
                    if let Some(Some(d)) = row.get(i).map(|x| x.as_ref()) {
                        map_join(&mut m, demand_expr(cx, arg, d, false));
                    }
                }
            }
            m
        }

        TExprKind::DictCall { func_name, value_args, .. } => {
            let mut m = DemandMap::new();
            if let Some(row) = cx.callee_row(func_name, &Demand::Head) {
                for (i, arg) in value_args.iter().enumerate() {
                    if let Some(Some(d)) = row.get(i).map(|x| x.as_ref()) {
                        map_join(&mut m, demand_expr(cx, arg, d, false));
                    }
                }
            }
            m
        }

        TExprKind::DictMethod { dict, .. } => head(dict),

        TExprKind::RecordUpdate { record, updates, .. } => {
            let mut m = head(record);
            for (_, _, e) in updates {
                map_join(&mut m, head(e));
            }
            m
        }

        TExprKind::OutgoingCallback { callee, .. } => head(callee),
        TExprKind::FfiMaybeArg { value } => head(value),
    }
}

/// Curried-application arm of `demand_expr`.
fn demand_app(cx: &RowCx, expr: &TExpr, rd: &Demand, run_pos: bool) -> DemandMap {
    // Flatten f x y z, looking through parens around the head.
    let mut args_rev: Vec<&TExpr> = Vec::new();
    let mut f = expr;
    loop {
        match &f.kind {
            TExprKind::App(func, arg) => {
                args_rev.push(arg.as_ref());
                f = func.as_ref();
            }
            TExprKind::Paren(inner) => f = inner.as_ref(),
            _ => break,
        }
    }
    let args: Vec<&TExpr> = args_rev.into_iter().rev().collect();

    let mut m = DemandMap::new();
    let fname = match &f.kind {
        TExprKind::Var(name) => Some(name.as_str()),
        _ => None,
    };

    // `seq a b`: forces a to WHNF; b carries the expression's demand.
    if fname == Some("seq") {
        if let Some(a) = args.first() {
            map_join(&mut m, demand_expr(cx, a, &Demand::Head, false));
        }
        if args.len() == 2 {
            map_join(&mut m, demand_expr(cx, args[1], rd, run_pos));
        }
        map_join_one(&mut m, "seq", Demand::Head);
        return m;
    }

    // `return e` / `pure e`: non-strict at WHNF; under a deeper result
    // demand in run position, e receives that demand (forcing a field of
    // the yielded value forces through e).
    if let (Some(name @ ("return" | "pure")), 1) = (fname, args.len()) {
        if run_pos && *rd != Demand::Head {
            map_join(&mut m, demand_expr(cx, args[0], rd, false));
        }
        map_join_one(&mut m, name, Demand::Head);
        return m;
    }

    // Constructor application: cons distributes an element demand; other
    // constructions force nothing (fields are lazy).
    if let TExprKind::Con(cname) = &f.kind {
        if cname == ":" && args.len() == 2
            && let Demand::Elems(de) = rd {
                map_join(&mut m, demand_expr(cx, args[0], de, false));
                map_join(&mut m, demand_expr(cx, args[1], rd, false));
            }
        return m;
    }

    // An action-typed application outside run position is SUSPENDED by the
    // emitter (a deferred closure): none of its argument demands fire here.
    let suspended = is_action_value_ty(&expr.ty) && !run_pos;

    // The function expression itself is demanded.
    map_join(&mut m, demand_expr(cx, f, &Demand::Head, false));

    if let Some(name) = fname {
        let full = cx.rows.arity.get(name).is_some_and(|a| *a == args.len());
        cx.record_site(f, name, if suspended { &Demand::Head } else { rd }, full);
        if !suspended {
            let row = if run_pos {
                st_intrinsic_run_row(name, args.len())
                    .or_else(|| cx.callee_row(name, rd))
            } else {
                cx.callee_row(name, rd)
            };
            if let Some(row) = row {
                for (i, arg) in args.iter().enumerate() {
                    if let Some(Some(d)) = row.get(i).map(|x| x.as_ref()) {
                        map_join(&mut m, demand_expr(cx, arg, d, false));
                    }
                }
            }
        }
    }

    // gen_arg's context-free eager floor: provably-total argument
    // expressions are evaluated in place at every emitted call site, so
    // their reads happen when the call is emitted — except for inlined
    // callees (substitution, not gen_arg) and suspended actions.
    if !suspended {
        let skip = fname.is_some_and(|n| (cx.inlined)(n));
        if !skip {
            for arg in &args {
                if arg_emitted_eagerly(arg) {
                    map_join(&mut m, demand_expr(cx, arg, &Demand::Head, false));
                }
            }
        }
    }

    m
}

/// Guard-chain variant of `demand_expr` (same sequencing rule as
/// `demanded_guards`).
fn demand_guards_map(cx: &RowCx, guards: &[TGuard], rd: &Demand, run_pos: bool) -> DemandMap {
    let mut acc = DemandMap::new();
    for g in guards.iter().rev() {
        let body_d = demand_expr(cx, &g.body, rd, run_pos);
        let is_otherwise = matches!(&g.condition.kind, TExprKind::Var(n) if n == "otherwise");
        acc = if is_otherwise {
            body_d
        } else {
            let mut s = demand_expr(cx, &g.condition, &Demand::Head, false);
            map_join(&mut s, map_meet(&body_d, &acc));
            s
        };
    }
    acc
}

/// Demand map of one clause body under result demand `rd`, closed over the
/// clause's where-bound VALUE definitions (a demanded where-binding's RHS
/// demands fire too, exactly as codegen's demanded_bindings evaluates it).
fn clause_demand_map(cx: &RowCx, clause: &TClause, rd: &Demand) -> DemandMap {
    let mut m = if clause.guards.is_empty() {
        demand_expr(cx, &clause.body, rd, true)
    } else {
        demand_guards_map(cx, &clause.guards, rd, true)
    };
    let mut walked: HashMap<&str, Demand> = HashMap::new();
    loop {
        let mut changed = false;
        for b in &clause.where_binds {
            if !b.patterns.is_empty() {
                continue;
            }
            if let Some(d) = m.get(&b.name).cloned() {
                let redo = match walked.get(b.name.as_str()) {
                    Some(prev) => !prev.subsumes(&d),
                    None => true,
                };
                if redo {
                    walked.insert(b.name.as_str(), d.clone());
                    map_join(&mut m, demand_expr(cx, &b.body, &d, false));
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    for b in &clause.where_binds {
        m.remove(&b.name);
    }
    m
}

/// Per-parameter demands of one clause under result demand `rd`:
/// pattern-match demands joined with the body's demands on pattern
/// variables.
fn clause_param_row(cx: &RowCx, clause: &TClause, arity: usize, rd: &Demand) -> Vec<Option<Demand>> {
    let m = clause_demand_map(cx, clause, rd);
    (0..arity)
        .map(|i| match clause.patterns.get(i) {
            Some(pat) => pattern_demand(pat, &m),
            None => None,
        })
        .collect()
}

/// Optimistic per-parameter seed for the greatest-fixed-point iteration:
/// the deepest demand the domain models for that parameter's type (see the
/// GFP rationale in `analyze` — a too-shallow seed cannot recognize
/// recursion-carried demand such as an element-strict accumulator).
fn seed_param(clauses: &[&TLocalDefLike], i: usize) -> Option<Demand> {
    for c in clauses {
        if let Some(p) = c.patterns.get(i) {
            match p {
                TPattern::Var(_, ty) => return Some(deep_of_ty(ty)),
                TPattern::Constructor { name, .. } if name == ":" || name == "[]" => {
                    return Some(Demand::Elems(Box::new(Demand::Head)));
                }
                TPattern::Tuple(ps) => {
                    return Some(Demand::Fields(vec![Some(Demand::Head); ps.len()]));
                }
                _ => {}
            }
        }
    }
    Some(Demand::Head)
}

/// A uniform view over top-level clauses and where-bound local function
/// equations, for shared row computation.
struct TLocalDefLike {
    patterns: Vec<TPattern>,
    guards: Vec<TGuard>,
    body: TExpr,
    where_binds: Vec<TLocalDef>,
}

fn clause_view(c: &TClause) -> TLocalDefLike {
    TLocalDefLike {
        patterns: c.patterns.clone(),
        guards: c.guards.clone(),
        body: c.body.clone(),
        where_binds: c.where_binds.clone(),
    }
}

fn local_def_view(d: &TLocalDef) -> TLocalDefLike {
    TLocalDefLike {
        patterns: d.patterns.clone(),
        guards: vec![],
        body: d.body.clone(),
        where_binds: vec![],
    }
}

fn as_clause(v: &TLocalDefLike) -> TClause {
    TClause {
        patterns: v.patterns.clone(),
        guards: v.guards.clone(),
        body: v.body.clone(),
        where_binds: v.where_binds.clone(),
        span: None,
    }
}

/// Compute run/deep rows for a group of equations (a function) under the
/// current environment.
fn equations_rows(
    cx: &RowCx,
    eqs: &[&TLocalDefLike],
    arity: usize,
    result_deep: Option<&Demand>,
) -> (Vec<Option<Demand>>, Option<Vec<Option<Demand>>>) {
    let row_under = |rd: &Demand| -> Vec<Option<Demand>> {
        let mut row: Option<Vec<Option<Demand>>> = None;
        for eq in eqs {
            let c = as_clause(eq);
            let r = clause_param_row(cx, &c, arity, rd);
            row = Some(match row {
                None => r,
                Some(prev) => prev
                    .iter()
                    .zip(r.iter())
                    .map(|(a, b)| opt_meet(a.as_ref(), b.as_ref()))
                    .collect(),
            });
        }
        row.unwrap_or_default()
    };
    let run = row_under(&Demand::Head);
    let deep = result_deep.map(row_under);
    (run, deep)
}

/// Signatures for a clause's where-bound local FUNCTIONS, iterated to
/// their own fixed point under the global environment. Needed so demand
/// flows through helpers like `reverse`'s `go` accumulator loop.
fn local_fn_rows(cx_rows: &Rows, inlined: &dyn Fn(&str) -> bool, where_binds: &[TLocalDef]) -> HashMap<String, LocalRows> {
    // Group function equations by name, in order.
    let mut groups: Vec<(String, Vec<&TLocalDef>)> = Vec::new();
    for b in where_binds {
        if b.patterns.is_empty() {
            continue;
        }
        match groups.last_mut() {
            Some((n, defs)) if *n == b.name => defs.push(b),
            _ => groups.push((b.name.clone(), vec![b])),
        }
    }
    if groups.is_empty() {
        return HashMap::new();
    }

    let mut locals: HashMap<String, LocalRows> = HashMap::new();
    // Optimistic seeds.
    for (name, defs) in &groups {
        let arity = defs.iter().map(|d| d.patterns.len()).max().unwrap_or(0);
        let views: Vec<TLocalDefLike> = defs.iter().map(|d| local_def_view(d)).collect();
        let view_refs: Vec<&TLocalDefLike> = views.iter().collect();
        let seed: Vec<Option<Demand>> = (0..arity).map(|i| seed_param(&view_refs, i)).collect();
        let result_deep = deep_of_ty(&defs[0].body.ty);
        let deep = if result_deep != Demand::Head { Some(seed.clone()) } else { None };
        locals.insert(name.clone(), LocalRows { run: seed, deep, result_deep });
    }
    // Iterate downward to a fixed point.
    loop {
        let mut changed = false;
        for (name, defs) in &groups {
            let arity = defs.iter().map(|d| d.patterns.len()).max().unwrap_or(0);
            let views: Vec<TLocalDefLike> = defs.iter().map(|d| local_def_view(d)).collect();
            let view_refs: Vec<&TLocalDefLike> = views.iter().collect();
            let result_deep = locals[name.as_str()].result_deep.clone();
            let rd_opt = if result_deep != Demand::Head { Some(result_deep.clone()) } else { None };
            let cx = RowCx { rows: cx_rows, locals: &locals, inlined, sites: None };
            let (run, deep) = equations_rows(&cx, &view_refs, arity, rd_opt.as_ref());
            let entry = locals.get(name.as_str()).unwrap();
            // Meet with the previous rows: keeps the iteration strictly
            // descending (freshly computed demands are not guaranteed
            // pointwise-comparable to the seed), so it must terminate.
            let run: Vec<Option<Demand>> = entry.run.iter().zip(run.iter())
                .map(|(a, b)| opt_meet(a.as_ref(), b.as_ref())).collect();
            let deep: Option<Vec<Option<Demand>>> = match (&entry.deep, deep) {
                (Some(prev), Some(new)) => Some(prev.iter().zip(new.iter())
                    .map(|(a, b)| opt_meet(a.as_ref(), b.as_ref())).collect()),
                _ => None,
            };
            if entry.run != run || entry.deep != deep {
                locals.insert(name.clone(), LocalRows { run, deep, result_deep });
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    locals
}

/// Public entry point for codegen: the demand map of `expr` evaluated with
/// result demand `rd` in run position (a bind-chain statement or a clause
/// body — the positions codegen flattens).
pub fn demanded_map(
    expr: &TExpr,
    rows: &Rows,
    inlined: &dyn Fn(&str) -> bool,
    rd: &Demand,
) -> DemandMap {
    let locals = HashMap::new();
    let cx = RowCx { rows, locals: &locals, inlined, sites: None };
    demand_expr(&cx, expr, rd, true)
}

/// Guard-chain variant of `demanded_map`.
pub fn demanded_map_guards(
    guards: &[TGuard],
    rows: &Rows,
    inlined: &dyn Fn(&str) -> bool,
    rd: &Demand,
) -> DemandMap {
    let locals = HashMap::new();
    let cx = RowCx { rows, locals: &locals, inlined, sites: None };
    demand_guards_map(&cx, guards, rd, true)
}

/// Compute the structured rows for every function. `strict_params` (the
/// finished boolean analysis, including its builtin/FFI/primitive-method
/// seeds) provides the base rows for everything that has no analyzable
/// body; derived rows for module functions overwrite them.
fn analyze_rows(module: &TModule, strict_params: &HashMap<String, Vec<bool>>) -> Rows {
    let functions: Vec<&TFunction> = module
        .functions
        .iter()
        .chain(module.instance_fns.iter())
        .collect();
    let inlined = |_: &str| false;

    let mut rows = Rows {
        run: HashMap::new(),
        deep: HashMap::new(),
        deep_result: HashSet::new(),
        result_deep: HashMap::new(),
        arity: HashMap::new(),
    };

    // Base rows from the boolean analysis (builtins, FFI, methods).
    for (name, mask) in strict_params {
        rows.run.insert(
            name.clone(),
            mask.iter().map(|s| if *s { Some(Demand::Head) } else { None }).collect(),
        );
    }
    // Structured upgrades for list-consuming intrinsics: the runtime walks
    // the spine and forces every element (see `__mll_bs` concatList/pack).
    for name in ["bsConcatList", "bsPack"] {
        rows.run.insert(name.to_string(), vec![Some(Demand::Elems(Box::new(Demand::Head)))]);
    }

    // Group same-named functions (a user definition shadowing a prelude
    // one): the environment is name-keyed and a call site cannot be
    // attributed to one member, so a shared entry is the MEET over all of
    // them — analyzing them as independent map writes makes the iteration
    // oscillate the moment their rows differ (see the same grouping in
    // `analyze`).
    let mut fn_groups: Vec<(&str, Vec<&TFunction>)> = Vec::new();
    {
        let mut index: HashMap<&str, usize> = HashMap::new();
        for func in &functions {
            if func.clauses.is_empty() {
                continue;
            }
            match index.get(func.name.as_str()) {
                Some(&i) => fn_groups[i].1.push(func),
                None => {
                    index.insert(func.name.as_str(), fn_groups.len());
                    fn_groups.push((func.name.as_str(), vec![func]));
                }
            }
        }
    }

    // Seed module functions optimistically (see the GFP rationale in
    // `analyze`): every parameter at the deepest demand of its type, deep
    // rows for every function whose result type has structure. The
    // iteration below shrinks both to consistency. Deep rows and the
    // deep-result set only apply to UNAMBIGUOUS names (single-member
    // groups): with a shadowed name the call-site attribution needed for
    // the deep-result proof is impossible.
    for (name, members) in &fn_groups {
        let arity = members
            .iter()
            .flat_map(|f| f.clauses.iter())
            .map(|c| c.patterns.len())
            .max()
            .unwrap_or(0);
        rows.arity.insert((*name).to_string(), arity);
        let views: Vec<TLocalDefLike> = members
            .iter()
            .flat_map(|f| f.clauses.iter())
            .map(clause_view)
            .collect();
        let view_refs: Vec<&TLocalDefLike> = views.iter().collect();
        let seed: Vec<Option<Demand>> = (0..arity).map(|i| seed_param(&view_refs, i)).collect();
        if members.len() == 1 {
            // Result type: strip `arity` arrows off the function type.
            let mut rty = &members[0].ty;
            while let Ty::Forall(_, t) = rty { rty = t; }
            for _ in 0..arity {
                if let Ty::Arrow(_, rest) = rty {
                    rty = rest;
                    while let Ty::Forall(_, t) = rty { rty = t; }
                }
            }
            let rdeep = deep_of_ty(rty);
            if rdeep != Demand::Head {
                rows.result_deep.insert((*name).to_string(), rdeep);
                rows.deep.insert((*name).to_string(), seed.clone());
            }
        }
        rows.run.insert((*name).to_string(), seed);
    }

    // Phase A: shrink the rows to their greatest fixed point. Each update
    // MEETS the freshly computed row with the previous one: recomputed
    // demands are not guaranteed pointwise-comparable to the seed (a
    // pattern-derived element demand can be deeper than a type-derived
    // one), and meeting keeps the sequence strictly descending — finite
    // descent guarantees termination, and stopping at a row that claims no
    // more than any single round's equations justify keeps it sound.
    loop {
        let mut changed = false;
        for (name, members) in &fn_groups {
            let arity = rows.arity[*name];
            let mut run_row: Option<Vec<Option<Demand>>> = None;
            let mut deep_row: Option<Vec<Option<Demand>>> = None;
            let rdeep = rows.result_deep.get(*name).cloned();
            let meet_into = |acc: Option<Vec<Option<Demand>>>, r: Vec<Option<Demand>>| {
                Some(match acc {
                    None => r,
                    Some(prev) => prev.iter().zip(r.iter())
                        .map(|(a, b)| opt_meet(a.as_ref(), b.as_ref())).collect(),
                })
            };
            for func in members {
                for clause in &func.clauses {
                    // Each clause gets its own where scope.
                    let locals = local_fn_rows(&rows, &inlined, &clause.where_binds);
                    let cx = RowCx { rows: &rows, locals: &locals, inlined: &inlined, sites: None };
                    let view = clause_view(clause);
                    let eqs = [&view];
                    let (r, d) = equations_rows(&cx, &eqs, arity, rdeep.as_ref());
                    run_row = meet_into(run_row, r);
                    if let Some(d) = d {
                        deep_row = meet_into(deep_row, d);
                    }
                }
            }
            let run_row = meet_into(run_row.or_else(|| rows.run.get(*name).cloned()),
                rows.run.get(*name).cloned().unwrap_or_default()).unwrap_or_default();
            if rows.run.get(*name) != Some(&run_row) {
                rows.run.insert((*name).to_string(), run_row);
                changed = true;
            }
            if let Some(deep_row) = deep_row {
                let deep_row = meet_into(Some(deep_row),
                    rows.deep.get(*name).cloned().unwrap_or_default()).unwrap();
                if rows.deep.get(*name) != Some(&deep_row) {
                    rows.deep.insert((*name).to_string(), deep_row);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    // Phase B: the whole-program deep-result set. Seed with every function
    // whose result type has structure, then repeatedly walk each function
    // exactly as codegen will see it (its own result demand = deep iff it
    // is still in the set), recording the result demand every visited,
    // fully-applied call site applies. A function stays in the set only if
    // every reference to it in the program is such a call site with a
    // subsuming demand — a reference from an unvisited (lazy or suspended)
    // position, a partial application, a first-class use, or a
    // shallow-demand site removes it.
    let mut deep_result: HashSet<String> = rows.result_deep.keys().cloned().collect();

    // All references, by callee, as Var-node addresses (plus a poison flag
    // for reference shapes the walker cannot classify).
    let fn_names: HashSet<&str> = rows.arity.keys().map(|s| s.as_str()).collect();
    let mut refs: HashMap<String, HashSet<usize>> = HashMap::new();
    let mut poisoned: HashSet<String> = HashSet::new();
    for func in &functions {
        for clause in &func.clauses {
            let mut exprs: Vec<&TExpr> = vec![&clause.body];
            exprs.extend(clause.guards.iter().flat_map(|g| [&g.condition, &g.body]));
            exprs.extend(clause.where_binds.iter().map(|b| &b.body));
            for e in exprs {
                collect_fn_refs(e, &fn_names, &mut refs, &mut poisoned);
            }
        }
    }
    for p in &poisoned {
        deep_result.remove(p);
    }

    while !deep_result.is_empty() {
        rows.deep_result = deep_result.clone();
        let sites = std::cell::RefCell::new(HashMap::new());
        for func in &functions {
            if func.clauses.is_empty() {
                continue;
            }
            let rd = rows.result_demand(&func.name);
            for clause in &func.clauses {
                let locals = local_fn_rows(&rows, &inlined, &clause.where_binds);
                let cx = RowCx { rows: &rows, locals: &locals, inlined: &inlined, sites: Some(&sites) };
                // Clause body + where-value closure (records sites).
                let _ = clause_demand_map(&cx, clause, &rd);
                // Local function bodies run with unknown result demand.
                for b in &clause.where_binds {
                    if !b.patterns.is_empty() {
                        let _ = demand_expr(&cx, &b.body, &Demand::Head, true);
                    }
                }
            }
        }
        let sites = sites.into_inner();
        let mut next: HashSet<String> = HashSet::new();
        'cand: for name in &deep_result {
            let Some(rdeep) = rows.result_deep.get(name.as_str()) else { continue };
            let Some(name_refs) = refs.get(name.as_str()) else {
                // No references at all (e.g. an entry point): keep — its
                // body demand claim is vacuous but harmless.
                next.insert(name.clone());
                continue;
            };
            for r in name_refs {
                match sites.get(r) {
                    Some((cn, rd, full)) if cn == name && *full && rd.subsumes(rdeep) => {}
                    _ => continue 'cand,
                }
            }
            next.insert(name.clone());
        }
        if next == deep_result {
            break;
        }
        deep_result = next;
    }
    rows.deep_result = deep_result;

    rows
}

/// Collect every reference to a known function name: Var nodes by address;
/// SpecCall/DictCall references poison the callee (the walker does not
/// classify them as deep sites).
fn collect_fn_refs(
    expr: &TExpr,
    fn_names: &HashSet<&str>,
    refs: &mut HashMap<String, HashSet<usize>>,
    poisoned: &mut HashSet<String>,
) {
    match &expr.kind {
        TExprKind::Var(name) => {
            if fn_names.contains(name.as_str()) {
                refs.entry(name.clone())
                    .or_default()
                    .insert(expr as *const TExpr as usize);
            }
        }
        TExprKind::Lit(_) | TExprKind::Con(_) | TExprKind::OpFunc(_)
        | TExprKind::DictAccess { .. } => {}
        TExprKind::App(f, a) => {
            collect_fn_refs(f, fn_names, refs, poisoned);
            collect_fn_refs(a, fn_names, refs, poisoned);
        }
        TExprKind::Lambda { body, .. } => collect_fn_refs(body, fn_names, refs, poisoned),
        TExprKind::InfixApp { lhs, rhs, .. } => {
            collect_fn_refs(lhs, fn_names, refs, poisoned);
            collect_fn_refs(rhs, fn_names, refs, poisoned);
        }
        TExprKind::Negate(e) | TExprKind::Paren(e) => collect_fn_refs(e, fn_names, refs, poisoned),
        TExprKind::If { cond, then_branch, else_branch } => {
            collect_fn_refs(cond, fn_names, refs, poisoned);
            collect_fn_refs(then_branch, fn_names, refs, poisoned);
            collect_fn_refs(else_branch, fn_names, refs, poisoned);
        }
        TExprKind::Case { scrutinee, branches } => {
            collect_fn_refs(scrutinee, fn_names, refs, poisoned);
            for b in branches {
                for g in &b.guards {
                    collect_fn_refs(&g.condition, fn_names, refs, poisoned);
                    collect_fn_refs(&g.body, fn_names, refs, poisoned);
                }
                collect_fn_refs(&b.body, fn_names, refs, poisoned);
            }
        }
        TExprKind::Let { binds, body } => {
            for b in binds {
                collect_fn_refs(&b.body, fn_names, refs, poisoned);
            }
            collect_fn_refs(body, fn_names, refs, poisoned);
        }
        TExprKind::Tuple(elems) => {
            for e in elems {
                collect_fn_refs(e, fn_names, refs, poisoned);
            }
        }
        TExprKind::SpecCall { original, specialized, args } => {
            if fn_names.contains(original.as_str()) {
                poisoned.insert(original.clone());
            }
            if fn_names.contains(specialized.as_str()) {
                poisoned.insert(specialized.clone());
            }
            for a in args {
                collect_fn_refs(a, fn_names, refs, poisoned);
            }
        }
        TExprKind::DictCall { func_name, dict_args, value_args } => {
            if fn_names.contains(func_name.as_str()) {
                poisoned.insert(func_name.clone());
            }
            for a in dict_args.iter().chain(value_args.iter()) {
                collect_fn_refs(a, fn_names, refs, poisoned);
            }
        }
        TExprKind::DictMethod { dict, .. } => collect_fn_refs(dict, fn_names, refs, poisoned),
        TExprKind::RecordUpdate { record, updates, .. } => {
            collect_fn_refs(record, fn_names, refs, poisoned);
            for (_, _, e) in updates {
                collect_fn_refs(e, fn_names, refs, poisoned);
            }
        }
        TExprKind::OutgoingCallback { callee, .. } => collect_fn_refs(callee, fn_names, refs, poisoned),
        TExprKind::FfiMaybeArg { value } => collect_fn_refs(value, fn_names, refs, poisoned),
    }
}
