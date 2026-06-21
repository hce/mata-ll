//! Demand analysis: determines which function parameters are always
//! forced on every code path through the body.
//!
//! A parameter marked strict can be passed eagerly at call sites
//! (no thunk allocation) and forced at function entry.
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

    // Initial pass: analyze each function without cross-function info
    // (but with FFI strictness already seeded above).
    for func in &functions {
        if func.clauses.is_empty() {
            continue;
        }
        if strict_params.contains_key(&func.name) {
            continue; // already seeded as FFI
        }
        let strictness = analyze_function(func, &strict_params);
        strict_params.insert(func.name.clone(), strictness);
    }

    // Fixed-point iteration: re-analyze with accumulated strictness info
    // until no new strict parameters are discovered.
    loop {
        let mut changed = false;
        for func in &functions {
            if func.clauses.is_empty() {
                continue;
            }
            let new_strict = analyze_function(func, &strict_params);
            if let Some(old) = strict_params.get(&func.name) {
                if &new_strict != old {
                    changed = true;
                    strict_params.insert(func.name.clone(), new_strict);
                }
            } else {
                changed = true;
                strict_params.insert(func.name.clone(), new_strict);
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
/// A variable is demanded if it's demanded by ALL guard branches
/// (intersection of bodies) plus any guard conditions.
fn demanded_guards(guards: &[TGuard], env: &HashMap<String, Vec<bool>>) -> HashSet<String> {
    if guards.is_empty() {
        return HashSet::new();
    }

    // Guard conditions are always evaluated (union).
    let mut result: HashSet<String> = HashSet::new();
    for g in guards {
        result.extend(demanded_vars(&g.condition, env));
    }

    // Guard bodies: intersect (demanded only if demanded in ALL branches).
    let mut body_iter = guards.iter().map(|g| demanded_vars(&g.body, env));
    if let Some(first) = body_iter.next() {
        let intersection = body_iter.fold(first, |acc, s| &acc & &s);
        result.extend(intersection);
    }

    result
}

/// Core analysis: returns the set of free variables that are guaranteed
/// to be forced when `expr` is evaluated to WHNF.
///
/// `env` contains known strictness info for other functions, enabling
/// cross-function demand propagation.
fn demanded_vars(expr: &TExpr, env: &HashMap<String, Vec<bool>>) -> HashSet<String> {
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
            let mut s = demanded_vars(f, env);

            // Cross-function propagation: if callee is a known function
            // and is strict in position i, demand that argument's vars.
            if let TExprKind::Var(name) = &f.kind
                && let Some(callee_strict) = env.get(name) {
                    for (i, arg) in args_rev.iter().enumerate() {
                        if callee_strict.get(i).copied().unwrap_or(false) {
                            s.extend(demanded_vars(arg, env));
                        }
                    }
                }

            s
        }

        TExprKind::InfixApp { op, lhs, rhs } => {
            match op.as_str() {
                // Arithmetic/comparison operators force both sides.
                "+" | "-" | "*" | "/" | "div" | "mod"
                | "==" | "/=" | "<" | ">" | "<=" | ">="
                | "&&" | "||" | "++" | "<>" => {
                    let mut s = demanded_vars(lhs, env);
                    s.extend(demanded_vars(rhs, env));
                    s
                }
                // $ forces the function (lhs) but thunks the argument.
                "$" => demanded_vars(lhs, env),
                // Cons is lazy — neither side is forced.
                ":" => HashSet::new(),
                // Monadic bind/sequence forces both actions.
                ">>=" | ">>" => {
                    let mut s = demanded_vars(lhs, env);
                    s.extend(demanded_vars(rhs, env));
                    s
                }
                // Unknown operator — conservatively demand both.
                _ => {
                    let mut s = demanded_vars(lhs, env);
                    s.extend(demanded_vars(rhs, env));
                    s
                }
            }
        }

        TExprKind::Negate(e) => demanded_vars(e, env),

        TExprKind::Paren(e) => demanded_vars(e, env),

        TExprKind::If { cond, then_branch, else_branch } => {
            let mut s = demanded_vars(cond, env);
            // Only demanded if demanded in BOTH branches.
            let t = demanded_vars(then_branch, env);
            let e = demanded_vars(else_branch, env);
            s.extend(&t & &e);
            s
        }

        TExprKind::Case { scrutinee, branches } => {
            let mut s = demanded_vars(scrutinee, env);
            if !branches.is_empty() {
                // Intersect demanded vars across all branches
                // (minus variables bound by each branch's pattern).
                let mut branch_iter = branches.iter().map(|b| {
                    let body_demanded = if b.guards.is_empty() {
                        demanded_vars(&b.body, env)
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
            let mut body_demanded = demanded_vars(body, env);
            let bound_names: HashSet<String> = binds.iter()
                .map(|b| b.name.clone())
                .collect();

            // If the body demands a let-bound variable, the variables
            // demanded by that binding's definition are also demanded.
            for bind in binds {
                if body_demanded.contains(&bind.name) {
                    body_demanded.extend(demanded_vars(&bind.body, env));
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
                        s.extend(demanded_vars(arg, env));
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
                        s.extend(demanded_vars(arg, env));
                    }
                }
            }
            s
        }

        TExprKind::DictAccess { .. } => {
            HashSet::new()
        }

        TExprKind::RecordUpdate { record, updates, .. } => {
            let mut s = demanded_vars(record, env);
            for (_, _, e) in updates {
                s.extend(demanded_vars(e, env));
            }
            s
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
