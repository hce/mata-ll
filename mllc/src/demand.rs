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
}

/// Which notion of "demanded" the analysis computes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DemandMode {
    /// What evaluating the expression to WHNF must force under Haskell
    /// semantics. Used for parameter strictness (a strict parameter is
    /// forced at function entry, so this must never over-claim).
    Semantic,
    /// What the emitted Lua actually forces when it evaluates the
    /// expression. A superset of Semantic on two counts: codegen's gen_arg
    /// evaluates *cheap* call arguments eagerly at every call site (even
    /// for callees that are semantically lazy in that position), and the
    /// continuation of a flattened IO/ST bind chain always runs. Used by
    /// codegen to decide which let/where bindings may be assigned strictly:
    /// forcing such a binding early only reorders a force the emitted code
    /// performs regardless.
    Emission,
}

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
    loop {
        let mut changed = false;
        for func in &functions {
            if func.clauses.is_empty() {
                continue;
            }
            let new_strict = analyze_function(func, &strict_params);
            if strict_params.get(&func.name) != Some(&new_strict) {
                strict_params.insert(func.name.clone(), new_strict);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    DemandInfo { strict_params }
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

    // Compute demanded variables from the body (and guards).
    let demanded = if clause.guards.is_empty() {
        demanded_vars(&clause.body, env)
    } else {
        demanded_guards(&clause.guards, env)
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
    demanded_guards_mode(guards, env, DemandMode::Semantic, &|_| false)
}

fn demanded_guards_mode(
    guards: &[TGuard],
    env: &HashMap<String, Vec<bool>>,
    mode: DemandMode,
    inlined: &dyn Fn(&str) -> bool,
) -> HashSet<String> {
    // Demand past the end of the chain: fallthrough demands nothing.
    let mut acc: HashSet<String> = HashSet::new();
    for g in guards.iter().rev() {
        let body_d = demanded_vars_mode(&g.body, env, mode, inlined);
        let is_otherwise = matches!(&g.condition.kind, TExprKind::Var(n) if n == "otherwise");
        acc = if is_otherwise {
            // Condition is `true`: the body runs unconditionally here.
            body_d
        } else {
            let mut s = demanded_vars_mode(&g.condition, env, mode, inlined);
            s.extend(&body_d & &acc);
            s
        };
    }
    acc
}

/// Emission-mode variant of `demanded_vars` (see `DemandMode::Emission`):
/// the variables the *emitted Lua* forces when it evaluates `expr`.
/// `inlined` reports whether a callee is an inline candidate — inlined
/// calls substitute arguments into the body instead of routing them
/// through gen_arg, so the cheap-argument rule must not apply to them.
pub fn forced_vars(
    expr: &TExpr,
    env: &HashMap<String, Vec<bool>>,
    inlined: &dyn Fn(&str) -> bool,
) -> HashSet<String> {
    demanded_vars_mode(expr, env, DemandMode::Emission, inlined)
}

/// Emission-mode variant of `demanded_guards`.
pub fn forced_guards(
    guards: &[TGuard],
    env: &HashMap<String, Vec<bool>>,
    inlined: &dyn Fn(&str) -> bool,
) -> HashSet<String> {
    demanded_guards_mode(guards, env, DemandMode::Emission, inlined)
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
    demanded_vars_mode(expr, env, DemandMode::Semantic, &|_| false)
}

fn demanded_vars_mode(
    expr: &TExpr,
    env: &HashMap<String, Vec<bool>>,
    mode: DemandMode,
    inlined: &dyn Fn(&str) -> bool,
) -> HashSet<String> {
    let rec = |e: &TExpr| demanded_vars_mode(e, env, mode, inlined);
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

            // Emission mode: gen_arg evaluates cheap arguments eagerly at
            // every call site, regardless of the callee's semantic
            // strictness — so their reads happen when the call is emitted.
            // Excluded: inlined callees (arguments are substituted into the
            // body, where a branch-only use is not forced) and
            // semigroup_List (codegen thunks its second argument).
            if mode == DemandMode::Emission {
                let skip = match &f.kind {
                    TExprKind::Var(name) => inlined(name) || name == "semigroup_List",
                    _ => false,
                };
                if !skip {
                    for arg in &args_rev {
                        if arg_emitted_eagerly(arg) {
                            s.extend(rec(arg));
                        }
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
                // Monadic bind/sequence forces both actions. In Emission
                // mode a `>>= \x -> rest` continuation always runs (the
                // flattened IO/ST bind chain executes it in sequence), so
                // its demands count, minus the lambda-bound names.
                ">>=" | ">>" => {
                    let mut s = rec(lhs);
                    if mode == DemandMode::Emission
                        && op == ">>="
                        && let TExprKind::Lambda { params, body } = &rhs.kind {
                            let mut b = rec(body);
                            for (p, _) in params {
                                b.remove(p);
                            }
                            s.extend(b);
                        } else {
                            s.extend(rec(rhs));
                        }
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
                        demanded_guards_mode(&b.guards, env, mode, inlined)
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
