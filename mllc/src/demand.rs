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
//
// The rules the two judgments agree on — the infix operand-strictness
// table, the guard-chain combine, and the if/`otherwise` rule — each live
// in exactly ONE place: the "Rules shared by both analyses" section below.
// That section also catalogues the rules that deliberately DIFFER, which is
// why the boolean analysis is NOT simply the ">= Head" projection of the
// structured one and cannot be derived from it.

/// Compiler builtins with known per-argument strictness. These are not mata-ll
/// functions, so the fixed point below never derives a body for them; their
/// strict positions are stated here and seeded into the demand environment
/// keyed by the source name gen_arg looks up. Two families:
///   * ByteString primitives — first-order, strict in every argument (they
///     read a ByteString/Int/String immediately; you cannot index/measure/
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
/// fallbacks (`local function eq_Int(a, b) … __force(a); __force(b) …`)
/// force both operands. They are strict in both arguments exactly like the
/// corresponding `InfixApp` operators — without this seed, every guard or
/// condition written with `==`/`<`/`>=` on a primitive type hides its
/// operands from the analysis (the comparison survives to TIR as
/// `App(App(Var("ord_ge__Int"), a), b)`), which is what kept hot-loop
/// counters lazy (see experiments/tracker/PERF-REGRESSION.md).
const PRIMITIVE_BINOP_METHODS: &[&str] = &[
    "eq_Int", "eq_Number", "eq_String", "eq_Bool", "eq_ByteString",
    "ord_lt__Int", "ord_lt__Number", "ord_lt__String", "ord_lt__ByteString",
    "ord_gt__Int", "ord_gt__Number", "ord_gt__String", "ord_gt__ByteString",
    "ord_le__Int", "ord_le__Number", "ord_le__String", "ord_le__ByteString",
    "ord_ge__Int", "ord_ge__Number", "ord_ge__String", "ord_ge__ByteString",
    "ord_max__Int", "ord_max__Number", "ord_max__String", "ord_max__ByteString",
    "ord_min__Int", "ord_min__Number", "ord_min__String", "ord_min__ByteString",
    "semigroup_String",
];

/// Runtime-implemented prelude functions with known per-argument strictness.
/// Like `STRICT_BUILTINS`, these have no mata-ll body for the fixed point to
/// analyze — their behaviour is fixed by the Lua runtime text codegen emits —
/// so their strict positions are stated here, keyed by the SOURCE name that
/// appears at TIR call sites (`not`/`error` are renamed to `not_`/`error_`
/// only later, by `sanitize_name`). Every mask below was read off the emitted
/// runtime body: a position is `true` only if the body `__force`s it on EVERY
/// path before returning, so evaluating the argument eagerly at the call site
/// merely reorders a force the callee performs anyway.
///
/// The deliberate laziness holes, verified against the runtime bodies:
///   * `take` is LAZY in the list — GHC's `take n _ | n <= 0 = []` returns
///     without touching it, and the runtime checks `n <= 0` before
///     `__force(xs)`, so `take 0 undefined` must stay `[]`.
///   * `show_Unit` is omitted entirely: it returns "()" without forcing.
///   * `foldr`/`foldl` are omitted: their seed argument is forced only on the
///     empty-structure path, and the accumulator must stay lazy.
///
/// `map`/`filter`/`zipWith` force their FUNCTION argument too (`f = __force(f)`
/// runs before the nil check), so that position is strict as well.
const RUNTIME_PRELUDE_STRICTNESS: &[(&str, &[bool])] = &[
    // show forces its value to WHNF first thing (`x = __force(x)`); each
    // type-directed shim is an unconditional `return show(x)` or forces
    // its argument itself (show_ByteString, show_HashMap).
    ("show", &[true]),
    ("show_Int", &[true]),
    ("show_Number", &[true]),
    ("show_String", &[true]),
    ("show_Bool", &[true]),
    ("show_List_", &[true]),
    ("show_Maybe", &[true]),
    ("show_ByteString", &[true]),
    ("show_HashMap", &[true]),
    ("not", &[true]),         // return not __force(x)
    ("error", &[true]),       // error(__force(msg)) — forces before raising
    ("head", &[true]),        // __mll_head forces the cell (l = __force(l))
    ("tail", &[true]),        // __mll_tail forces the cell (l = __force(l))
    ("map", &[true, true]),
    ("filter", &[true, true]),
    ("take", &[true, false]), // n always; the list NOT when n <= 0
    ("drop", &[true, true]),
    ("zipWith", &[true, true, true]),
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
            if let Some(cb) = &clause.body
                && let TExprKind::SpecCall { original, .. } = &cb.kind
                && original == &func.name && !clause.patterns.is_empty() {
                    strict_params.insert(func.name.clone(), vec![true; clause.patterns.len()]);
                    continue;
                }
        }
    }

    // Seed compiler builtins that are strict but are not mata-ll functions, so
    // the fixed point below never sees a body for them. The ByteString
    // primitives immediately consume their arguments — you cannot measure,
    // index, slice, or convert through a thunk — so every ByteString/Int/
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
    // Runtime prelude rows apply only when the name still refers to the
    // runtime function. A user definition under the same source name shadows
    // the prelude one (codegen emits it after the runtime block), so every
    // TIR call site resolves to the user's function — its row must come from
    // analyzing its body, seeded optimistically like any other function, not
    // from the runtime mask.
    let defined_names: HashSet<&str> = functions.iter()
        .filter(|f| !f.clauses.is_empty())
        .map(|f| f.name.as_str())
        .collect();
    for (name, mask) in RUNTIME_PRELUDE_STRICTNESS {
        if !defined_names.contains(name) {
            strict_params.entry((*name).to_string()).or_insert_with(|| mask.to_vec());
        }
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
    // termination. Same-named functions are grouped first — see
    // `group_by_name` for why per-name meets are mandatory.
    let fn_groups = group_by_name(&functions);

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
        // Where-bound local function rows, recomputed under the converged
        // environment (exactly what codegen's scoped call-site map holds).
        for func in &functions {
            for clause in &func.clauses {
                let local = local_fn_demand(clause_view(clause), &strict_params, &|_| false);
                let (locals, caps) = (local.rows, local.captured);
                let mut lnames: Vec<&String> = locals.keys().collect();
                lnames.sort();
                for n in lnames {
                    let mut cap_list: Vec<&String> = caps
                        .get(n.as_str())
                        .map(|(_, s)| s.iter().collect())
                        .unwrap_or_default();
                    cap_list.sort();
                    eprintln!("DEMAND {}.{} {:?} captures={:?} (where-local)",
                        func.name, n, locals[n], cap_list);
                }
            }
        }
    }

    DemandInfo { strict_params, rows }
}

/// Group same-named functions (with at least one clause) in first-seen
/// order, for both fixed-point drivers: a user definition can shadow a
/// prelude one under the SAME name, and both environments are name-keyed —
/// a call site cannot be attributed to one member, so the shared entry
/// must be the MEET (per-position AND for the boolean rows) of every
/// member's row. Analyzing the members as independent map writes instead
/// makes the fixed-point loop oscillate forever the moment their rows
/// differ.
fn group_by_name<'a>(functions: &[&'a TFunction]) -> Vec<(&'a str, Vec<&'a TFunction>)> {
    let mut groups: Vec<(&str, Vec<&TFunction>)> = Vec::new();
    let mut index: HashMap<&str, usize> = HashMap::new();
    for func in functions {
        if func.clauses.is_empty() {
            continue;
        }
        match index.get(func.name.as_str()) {
            Some(&i) => groups[i].1.push(func),
            None => {
                index.insert(func.name.as_str(), groups.len());
                groups.push((func.name.as_str(), vec![func]));
            }
        }
    }
    groups
}

/// Analyze a single function's parameter strictness.
fn analyze_function(func: &TFunction, env: &HashMap<String, Vec<bool>>) -> Vec<bool> {
    let clauses = &func.clauses;
    if clauses.is_empty() {
        return vec![];
    }

    let arity = clauses[0].patterns.len();
    // The strictness row is sized from the first clause and every clause
    // writes `strict[i]` for its own pattern indices: the checker rejects
    // functions whose clauses bind different numbers of arguments
    // (check_function), so the row covers every clause.
    debug_assert!(
        clauses.iter().all(|c| c.patterns.len() == arity),
        "demand: clauses with unequal arities reached the analysis"
    );
    if arity == 0 {
        return vec![];
    }

    // Compiler-DERIVED Eq/Ord instance methods are strict in every argument by
    // construction (see `TFunction::derived_strict`): structural comparison
    // must force both operands to WHNF to inspect their constructor tags
    // before ANY clause can be selected. The clause-wise AND below cannot see
    // that — the derived `_ == _ = False` catch-all matches wildcards and
    // contributes `false` for both positions, dragging the row to all-false
    // even though the catch-all is only reachable after both scrutinees were
    // forced by the preceding constructor clauses. Pinning the row here (it
    // also survives the fixed point) is the derived-instance analogue of the
    // `PRIMITIVE_BINOP_METHODS` seed. Sound ONLY because the marker is set
    // exclusively at derivation time — a user-written Eq/Ord method may be
    // lazy in an argument and is analyzed normally.
    if func.derived_strict {
        return vec![true; arity];
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
    analyze_equation(clause_view(clause), arity, env, &|_| false)
}

/// The clause-wide shadowed-name over-approximation: every name bound by
/// the clause's own patterns, by a where binding (VALUE names and every
/// where-bind's parameters — surviving function GROUP names stay visible,
/// their local rows are the intentional lookups; the caller adds the
/// DROPPED group names), or by any inner binder (lambda, case, let —
/// `collect_rebound_names`). A name in this set may refer to a local at
/// some site in the clause, so name-keyed cross-function rules are
/// suppressed for it clause-wide (see [`Shadowed`]). The precision cost is
/// the same clause-wide over-approximation the local-row `rebound` filter
/// already accepts.
fn clause_shadowed_names(clause: &TLocalDefLike<'_>) -> HashSet<String> {
    let mut s = HashSet::new();
    for p in clause.patterns {
        collect_pattern_vars(p, &mut s);
    }
    if let Some(b) = clause.body {
        collect_rebound_names(b, &mut s);
    }
    for g in clause.guards {
        collect_rebound_names(&g.condition, &mut s);
        collect_rebound_names(&g.body, &mut s);
    }
    for b in clause.where_binds {
        if b.patterns.is_empty() {
            s.insert(b.name.clone());
        }
        for p in &b.patterns {
            collect_pattern_vars(p, &mut s);
        }
        collect_rebound_names(&b.body, &mut s);
    }
    s
}

/// `analyze_clause` over a borrowed equation view — a top-level clause or a
/// where-bound local function's equation (the local-function fixpoint
/// analyzes those in place; it once deep-cloned every equation into a
/// TClause per round to call this).
fn analyze_equation(
    clause: TLocalDefLike<'_>,
    arity: usize,
    env: &HashMap<String, Vec<bool>>,
    parent_shadowed: Shadowed,
) -> Vec<bool> {
    // Where-bound local FUNCTIONS get real strictness rows and
    // captured-demand sets, visible only inside this clause (the extended
    // map is dropped when this returns, so a local row can never leak into
    // another scope). Without the rows every call to a local helper
    // contributes no demand, which blinds the ENCLOSING function's row
    // too: `reverse xs = go [] xs` was judged lazy in xs even though go
    // forces its list argument on every path. The captured sets are the
    // outward half of the same story: `sumStrict n = go 0 0 where go's
    // guard is i > n` forces the CAPTURED n on every path, so sumStrict is
    // strict in n even though n is never an argument of the call.
    let local = local_fn_demand(clause, env, parent_shadowed);
    let local_caps = local.captured;
    // The extended environment local_fn_demand converged on (the outer env
    // plus the local rows) is exactly the one this clause is analyzed
    // under; reuse it instead of cloning `env` a second time.
    let env = local.env.as_ref().unwrap_or(env);

    // The clause-wide shadowed set: the clause's own binders plus every
    // where-bound FUNCTION name whose group was dropped by the local-row
    // scope filter (rebound/ambiguous — such a name has no row in the
    // extended env, so an unsuppressed lookup would hit a same-named
    // GLOBAL's row while the call targets the local).
    let mut own_shadowed = clause_shadowed_names(&clause);
    for b in clause.where_binds {
        if !b.patterns.is_empty() && !local.rows.contains_key(&b.name) {
            own_shadowed.insert(b.name.clone());
        }
    }
    // A name with a SURVIVING local row is exempt: its row (in the
    // extended env) is the intentional lookup, and the local-row scope
    // filter already dropped every ambiguous case. Without the exemption,
    // codegen's ambient predicate — whose local_vars contains the where-fn
    // names themselves — would suppress exactly the rows this machinery
    // exists to apply (a where-group accumulator loop went lazy and
    // thunk-chained to a stack overflow).
    let shadowed = |n: &str| {
        (own_shadowed.contains(n) || parent_shadowed(n)) && !local.rows.contains_key(n)
    };
    let shadowed: Shadowed = &shadowed;

    let mut strict = vec![false; arity];

    // Collect parameter names from patterns.
    // Constructor/LitPat/Tuple patterns force the parameter (pattern dispatch).
    let mut param_names: Vec<Option<String>> = Vec::with_capacity(arity);

    for (i, pat) in clause.patterns.iter().enumerate() {
        // A whole-value binder (Var, as-pattern) names the parameter for the
        // body-usage analysis; a pattern that inspects the value forces it
        // (an as-pattern does both — `xs@(x:rest)` names AND forces).
        param_names.push(pat.top_binder().map(str::to_string));
        if pat.forces_scrutinee() {
            strict[i] = true;
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
            for b in clause.where_binds {
                if b.patterns.is_empty() && s.contains(&b.name) {
                    for v in demanded_vars_in(&b.body, env, &local_caps, shadowed) {
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
    let mut demanded = if clause.guards.is_empty() {
        close(demanded_vars_in(clause.body.expect("guard-free equation has a body"), env, &local_caps, shadowed))
    } else {
        demanded_guards_with(clause.guards, env, &local_caps, shadowed, &close)
    };
    // (Both are Semantic-mode: parameter strictness must not over-claim.)

    // A where-bound name shadows a same-named parameter (`f go = go []
    // where go … = …` calls the LOCAL go), so demand on it — including the
    // bare "callee is demanded" entry every call contributes — must not
    // mark the parameter strict.
    for b in clause.where_binds {
        demanded.remove(&b.name);
    }

    // Mark parameters whose names appear in the demanded set.
    for (i, name) in param_names.iter().enumerate() {
        if let Some(n) = name
            && demanded.contains(n) {
                strict[i] = true;
            }
    }

    strict
}

// ════════════════════════════════════════════════════════════════════════
// Rules shared by both analyses
// ════════════════════════════════════════════════════════════════════════
//
// The boolean analysis (`demanded_vars_in`) and the structured analysis
// (`demand_expr`) are DIFFERENT judgments — SEMANTIC vs EMISSION, see the
// note at the top of the file — so neither can be derived from the other:
//
//   * `seq a b`: the boolean rule claims only `a` (mirroring `__mll_seq`,
//     deliberately leaving the result operand lazy — a conservative
//     under-claim); the structured rule passes the whole expression's
//     demand through to `b`, which IS the expression's result.
//   * `>>=`/`>>`: the boolean rule demands both actions' WHNF variables;
//     the structured rule models the flattened bind chain (run position,
//     lambda continuation, result-demand threading).
//   * `return`/`pure` (and their `$` spelling), suspension of action
//     values outside run position, gen_arg's eager floor
//     (`arg_emitted_eagerly`), tuple projection (`__mll_tup_get`), and `:`
//     under an element demand exist only in the structured analysis.
//   * the let/where VALUE closure is a single ordered pass in the boolean
//     analysis but a subsumption-driven fixpoint (`let_group_close`) in
//     the structured one.
//   * the where-local machinery differs: captured-variable sets (boolean)
//     vs `LocalRows` (structured).
//
// Everything the two DO share lives here, in exactly one place each: the
// operand-strictness table for infix operators, the guard-chain combine,
// and the if/`otherwise` rule. The combinators are generic over the two
// demand summaries via `DemandLattice`.

/// A locally-bound-name predicate: true when the name may refer to a local
/// binder at some site in the scope under analysis, so every name-keyed
/// cross-function rule (env/row lookups, `seq`/`pure`/`return`/`otherwise`
/// special arms, backtick operators) must be suppressed for it — the name
/// no longer reliably means the global. Suppression only UNDER-claims
/// demand, which is always sound (a missed strictness keeps a value lazy).
/// Scoping uses the same clause-wide over-approximation as the local-row
/// `rebound` filter (see `local_fn_demand`); codegen entry points pass
/// their live `local_vars` membership instead.
type Shadowed<'a> = &'a dyn Fn(&str) -> bool;

/// A guard or `if` condition that is literally `otherwise` — constant
/// true. The parser desugars the final guard of a where-bound function
/// into `if otherwise then b else error "non-exhaustive guards"`, and
/// codegen emits the condition as the literal `true`: the then-branch (or
/// guard body) runs unconditionally at that point, so the dead
/// alternative must not water down its demands. Without this rule a
/// guarded local accumulator loop loses its recursive-branch demand and
/// stays lazy. A LOCAL binder named `otherwise` (legal Haskell) defeats
/// the rule: the condition is then an ordinary variable that may be
/// False, so the else/fallthrough alternative is live.
fn is_otherwise(cond: &TExpr, shadowed: Shadowed) -> bool {
    matches!(&cond.kind, TExprKind::Var(n) if n == "otherwise") && !shadowed("otherwise")
}

/// Operand strictness of an infix operator, for the operators whose rule
/// both analyses share — those whose forcing behaviour depends neither on
/// the result demand nor on run position.
enum OpOperands {
    /// Both operands are forced to WHNF.
    Both,
    /// Only the left operand is forced.
    Lhs,
    /// Neither operand is forced.
    Neither,
}

/// The shared operand-strictness table. Returns `None` for the operators
/// whose rules genuinely differ between the two analyses (`seq`, `>>=`,
/// `>>`; see the section comment) — each analysis has its own arm for
/// those. `$` and `:` resolve here for the shared part of their rule; the
/// structured analysis overrides them first for its emission-specific
/// cases (`return`/`pure $ x` in run position, `:` under element demand).
fn shared_op_operands(op: &str, lhs: &TExpr) -> Option<OpOperands> {
    Some(match op {
        // Arithmetic/comparison operators force both sides.
        "+" | "-" | "*" | "/" | "^" | "div" | "mod"
        | "==" | "/=" | "<" | ">" | "<=" | ">=" => OpOperands::Both,
        // Short-circuit: the right side runs only when the left
        // allows it (Lua `and`/`or`; GHC agrees).
        "&&" | "||" => OpOperands::Lhs,
        // List append is lazy in its right side (codegen thunks
        // it); only the left spine is forced.
        "++" => OpOperands::Lhs,
        // <> on String is Lua string concat (both sides forced);
        // on lists it behaves like ++ (right side thunked).
        "<>" => {
            if matches!(&lhs.ty, crate::types::Ty::Con(n) if n == "String") {
                OpOperands::Both
            } else {
                OpOperands::Lhs
            }
        }
        // $ forces the function (lhs) but thunks the argument.
        "$" => OpOperands::Lhs,
        // Cons is lazy — neither side is forced at WHNF.
        ":" => OpOperands::Neither,
        // Analysis-specific rules; handled by each analysis's own arm.
        "seq" | ">>=" | ">>" => return None,
        // Unknown operator — claim nothing (an over-claim here
        // would let a lazy value be forced eagerly).
        _ => OpOperands::Neither,
    })
}

/// The lattice hooks the shared control-flow combinators need, provided by
/// both demand summaries: `HashSet<String>` (boolean analysis; presence =
/// forced to WHNF) and `DemandMap` (structured analysis).
trait DemandLattice: Default {
    /// Meet: keep only what BOTH summaries claim — used across branch
    /// alternatives, of which only one runs.
    fn meet_with(&self, other: &Self) -> Self;
    /// Join: absorb `other` — both demands occur on the same run.
    fn join_from(&mut self, other: Self);
}

impl DemandLattice for HashSet<String> {
    fn meet_with(&self, other: &Self) -> Self {
        self & other
    }
    fn join_from(&mut self, other: Self) {
        self.extend(other);
    }
}

impl DemandLattice for DemandMap {
    fn meet_with(&self, other: &Self) -> Self {
        map_meet(self, other)
    }
    fn join_from(&mut self, other: Self) {
        map_join(self, other);
    }
}

/// THE guard-chain rule, shared by both analyses.
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
fn guard_chain<'t, S: DemandLattice>(
    guards: &'t [TGuard],
    shadowed: Shadowed,
    cond_demand: &mut dyn FnMut(&'t TExpr) -> S,
    body_demand: &mut dyn FnMut(&'t TExpr) -> S,
) -> S {
    // Demand past the end of the chain: fallthrough demands nothing.
    let mut acc = S::default();
    for g in guards.iter().rev() {
        let body_d = body_demand(&g.body);
        acc = if is_otherwise(&g.condition, shadowed) {
            // Condition is `true`: the body runs unconditionally here.
            body_d
        } else {
            let mut s = cond_demand(&g.condition);
            s.join_from(body_d.meet_with(&acc));
            s
        };
    }
    acc
}

/// THE if/then/else rule, shared by both analyses: the condition's demand,
/// plus what BOTH branches demand (only one of them runs) — unless the
/// condition is `otherwise` (see `is_otherwise`), in which case the
/// then-branch runs unconditionally and the dead else-branch is ignored.
/// Same rule `guard_chain` applies to real guard chains.
fn if_demand<'t, S: DemandLattice>(
    cond: &'t TExpr,
    then_branch: &'t TExpr,
    else_branch: &'t TExpr,
    shadowed: Shadowed,
    cond_demand: &mut dyn FnMut(&'t TExpr) -> S,
    branch_demand: &mut dyn FnMut(&'t TExpr) -> S,
) -> S {
    if is_otherwise(cond, shadowed) {
        return branch_demand(then_branch);
    }
    let mut s = cond_demand(cond);
    let t = branch_demand(then_branch);
    let e = branch_demand(else_branch);
    s.join_from(t.meet_with(&e));
    s
}

// ════════════════════════════════════════════════════════════════════════
// Boolean (semantic, whole-value) demand analysis — continued
// ════════════════════════════════════════════════════════════════════════

/// Demanded variables of a guard chain (the chain rule itself —
/// right-to-left combine, `otherwise` — lives in `guard_chain`), given the
/// in-scope captured-demand sets and a closure applied to every per-guard
/// demand set BEFORE the chain combines them — the closure expands
/// where-bound values so the guard intersection sees through them (see
/// analyze_clause).
fn demanded_guards_with(
    guards: &[TGuard],
    env: &HashMap<String, Vec<bool>>,
    captured: &CapturedEnv,
    shadowed: Shadowed,
    close: &dyn Fn(HashSet<String>) -> HashSet<String>,
) -> HashSet<String> {
    guard_chain(
        guards,
        shadowed,
        &mut |c: &TExpr| close(demanded_vars_in(c, env, captured, shadowed)),
        &mut |b: &TExpr| close(demanded_vars_in(b, env, captured, shadowed)),
    )
}

/// Boolean strict rows for a clause's where-bound local FUNCTIONS (the
/// consecutive same-named equation groups codegen emits via
/// `gen_where_func_group_body`), iterated to their own greatest fixed point
/// under the fixed outer environment `env`.
///
/// Rules are identical to top-level functions:
///   * seed every parameter strict (⊤) and shrink downward — a self- or
///     mutually-recursive accumulator (`go acc i = … go (acc + i) …`) is
///     only provable strict under the assumption the recursive call already
///     is (see the greatest-fixed-point note in `analyze`);
///   * a parameter is kept strict only if EVERY equation forces it on every
///     path (per-position AND across the group).
///
/// Demand a local places on captured OUTER variables is propagated
/// separately, through the captured-demand sets `local_fn_demand` computes
/// alongside these rows (see there).
///
/// SCOPING: a row is keyed by bare name, so it may only be consulted where
/// the name can ONLY mean this local function. Any group whose name is also
/// bound by an inner construct somewhere in the clause — a lambda
/// parameter, a case-pattern variable, a let binding, or another local
/// definition's parameter — is dropped entirely: at such a call site the
/// name may refer to an unknown function, and applying the row there could
/// eagerly force an argument the actual function never demands. The same
/// goes for a name defined by two separate groups or shared with a where
/// VALUE binding (call sites cannot be attributed). Dropping a row merely
/// keeps the argument thunked — under-approximation stays safe.
///
/// Both consumers must agree on these rows: `analyze_clause` extends its
/// environment with them (so the enclosing function's row sees through
/// local calls), and codegen installs them in its scoped call-site map
/// (`local_strict_params`). Both call this with a deterministic `env`
/// (codegen with the converged `strict_params`), so the rows coincide.
pub fn local_fn_strict_params(
    clause: &TClause,
    env: &HashMap<String, Vec<bool>>,
    shadowed: Shadowed,
) -> HashMap<String, Vec<bool>> {
    local_fn_demand(clause_view(clause), env, shadowed).rows
}

/// What `local_fn_demand` computes for one clause: the strictness rows of
/// its where-bound local functions, their captured-demand sets, and — when
/// there are any local functions — the outer environment extended by those
/// rows (the map the fixpoint converged on), so the caller analyzing the
/// clause body can use it as is.
struct LocalFnDemand {
    rows: HashMap<String, Vec<bool>>,
    captured: CapturedEnv,
    env: Option<HashMap<String, Vec<bool>>>,
}

impl LocalFnDemand {
    fn empty() -> Self {
        LocalFnDemand { rows: HashMap::new(), captured: CapturedEnv::new(), env: None }
    }
}

/// Strict rows AND captured-demand sets for a clause's where-bound local
/// functions.
///
/// The second component maps each local to (arity, the set of OUTER
/// variables its body forces on every path): the every-path demanded set
/// of its body — same branch-intersection and `otherwise` rule as
/// everywhere else — restricted to free variables (each equation's own
/// pattern variables are excluded, and inner-bound names never enter a
/// demanded set in the first place). `demanded_vars_in` unions such a set
/// into the caller's demanded set at every demanded, saturated call to the
/// local, which is what makes `sumStrict n = go 0 0 where go's guard is
/// i > n` strict in the captured n.
///
/// Fixed point: unlike the rows (greatest, seeded all-strict), the
/// captured sets are the LEAST fixed point, seeded EMPTY and grown — a
/// capture may only be claimed when a finite derivation forces it, so a
/// capture reachable only through a not-yet-proven recursive call stays
/// out (conservative; safe direction). Growth is monotone (the sets only
/// feed `demanded_vars_in` additively), so the iteration terminates.
/// Transitive captures among (mutually) recursive siblings resolve through
/// the same union: go's demanded set includes captured(aux) at go's call
/// to aux, so aux's captures surface in captured(go) as the sets grow.
///
/// SCOPING of the captured names: a set is injected into demand sets at
/// arbitrary nesting depth inside the clause, so a name is kept only if it
/// can mean just ONE thing everywhere in the clause — any name rebound
/// somewhere in the clause (lambda/case/let binders, local-function
/// parameters; the same `rebound` set that gates the rows) is dropped, as
/// are function-bind and ambiguous names. What remains are enclosing
/// parameters, where-bound VALUES (which `close` then expands), and
/// further-out/top-level names — each with a single clause-wide meaning.
/// Dropping a name merely under-approximates, which stays safe.
/// Group a clause's CONSECUTIVE same-named function equations, mirroring
/// codegen's `gen_where_func_group_body`. Shared by the boolean
/// (`local_fn_demand`) and structured (`local_fn_rows`) local-function
/// analyses, so both see exactly the groups codegen emits.
fn group_where_fn_equations(where_binds: &[TLocalDef]) -> Vec<(String, Vec<&TLocalDef>)> {
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
    groups
}

fn local_fn_demand(
    clause: TLocalDefLike<'_>,
    env: &HashMap<String, Vec<bool>>,
    parent_shadowed: Shadowed,
) -> LocalFnDemand {
    let mut groups = group_where_fn_equations(clause.where_binds);
    if groups.is_empty() {
        return LocalFnDemand::empty();
    }

    // Names rebound anywhere in the clause (see the scoping note above).
    let mut rebound: HashSet<String> = HashSet::new();
    if let Some(cb) = clause.body { collect_rebound_names(cb, &mut rebound); }
    for g in clause.guards {
        collect_rebound_names(&g.condition, &mut rebound);
        collect_rebound_names(&g.body, &mut rebound);
    }
    for b in clause.where_binds {
        collect_rebound_names(&b.body, &mut rebound);
        for p in &b.patterns {
            collect_pattern_vars(p, &mut rebound);
        }
    }
    // Ambiguous names: two separate groups, or a group sharing its name
    // with a where VALUE binding.
    let mut ambiguous: HashSet<String> = HashSet::new();
    {
        let mut seen: HashSet<&str> = HashSet::new();
        for (n, _) in &groups {
            if !seen.insert(n.as_str()) {
                ambiguous.insert(n.clone());
            }
        }
        for b in clause.where_binds {
            if b.patterns.is_empty() && seen.contains(b.name.as_str()) {
                ambiguous.insert(b.name.clone());
            }
        }
    }
    groups.retain(|(n, _)| !rebound.contains(n) && !ambiguous.contains(n));
    if groups.is_empty() {
        return LocalFnDemand::empty();
    }

    // Borrowed equation views of each group. Guards on where-binds were
    // desugared to if/else by the parser, and a TLocalDef carries no nested
    // where, so these equations are guard- and where-free (which also
    // bounds the analyze_equation -> local_fn_demand recursion at one
    // level). Arity follows the FIRST equation, matching codegen's
    // num_params in gen_where_func_group_body.
    let group_clauses: Vec<(String, usize, Vec<TLocalDefLike<'_>>)> = groups
        .iter()
        .map(|(name, defs)| {
            let arity = defs[0].patterns.len();
            let clauses = defs.iter().map(|d| local_def_view(d)).collect();
            (name.clone(), arity, clauses)
        })
        .collect();

    // Optimistic seed, then iterate downward to the greatest fixed point.
    // The outer env is fixed and analyze_clause is monotone in it, so from
    // the ⊤ seed the local rows only shrink; termination is finite descent.
    let mut ext = env.clone();
    for (name, arity, _) in &group_clauses {
        ext.insert(name.clone(), vec![true; *arity]);
    }
    // The surviving group names are exempt from the ambient shadowing:
    // their rows in `ext` ARE the intentional lookups (codegen's ambient
    // predicate claims these very names, since where-fn names are locals).
    let group_names: HashSet<&str> =
        group_clauses.iter().map(|(n, _, _)| n.as_str()).collect();
    let group_shadowed = |n: &str| parent_shadowed(n) && !group_names.contains(n);
    let group_shadowed: Shadowed = &group_shadowed;
    loop {
        let mut changed = false;
        for (name, arity, clauses) in &group_clauses {
            let mut row = vec![true; *arity];
            for c in clauses {
                let cs = analyze_equation(*c, *arity, &ext, group_shadowed);
                for i in 0..*arity {
                    row[i] = row[i] && cs[i];
                }
            }
            if ext.get(name.as_str()) != Some(&row) {
                ext.insert(name.clone(), row);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // Captured-demand sets: least fixed point, seeded empty and grown (see
    // the function comment). Computed under the CONVERGED rows in `ext`, so
    // argument demand through sibling calls is already exact. Per equation:
    // the every-path demanded set of the body minus that equation's own
    // pattern variables; equations of one local are pattern-dispatch
    // alternatives, so the group's set is their INTERSECTION (a capture
    // forced by only some equations is not every-path). The scope filter
    // then drops rebound, function-bind, and ambiguous names.
    let fn_bind_names: HashSet<String> = clause
        .where_binds
        .iter()
        .filter(|b| !b.patterns.is_empty())
        .map(|b| b.name.clone())
        .collect();
    let mut captured: CapturedEnv = group_clauses
        .iter()
        .map(|(name, arity, _)| (name.clone(), (*arity, HashSet::new())))
        .collect();
    loop {
        let mut changed = false;
        for (name, _, clauses) in &group_clauses {
            let mut set: Option<HashSet<String>> = None;
            for c in clauses {
                let mut d = demanded_vars_in(c.body.expect("local equation has a body"), &ext, &captured, group_shadowed);
                let mut bound = HashSet::new();
                for p in c.patterns {
                    collect_pattern_vars(p, &mut bound);
                }
                d.retain(|v| !bound.contains(v));
                set = Some(match set {
                    None => d,
                    Some(prev) => &prev & &d,
                });
            }
            let mut set = set.unwrap_or_default();
            set.retain(|v| {
                !rebound.contains(v) && !fn_bind_names.contains(v) && !ambiguous.contains(v)
            });
            let entry = captured.get_mut(name.as_str()).unwrap();
            if entry.1 != set {
                entry.1 = set;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let rows: HashMap<String, Vec<bool>> = group_clauses
        .iter()
        .map(|(name, _, _)| (name.clone(), ext.get(name).cloned().unwrap_or_default()))
        .collect();
    LocalFnDemand { rows, captured, env: Some(ext) }
}

/// Names bound by an inner construct anywhere inside `expr`: lambda
/// parameters, case-pattern variables, and let-bound names. Used by
/// `local_fn_strict_params` to drop rows whose bare-name keying would be
/// ambiguous at some call site.
fn collect_rebound_names(expr: &TExpr, out: &mut HashSet<String>) {
    // The names THIS node binds; the children follow through for_each_child.
    match &expr.kind {
        TExprKind::Lambda { params, .. } => {
            for (p, _) in params {
                out.insert(p.clone());
            }
        }
        TExprKind::Case { branches, .. } => {
            for b in branches {
                collect_pattern_vars(&b.pattern, out);
            }
        }
        TExprKind::Let { binds, .. } => {
            for b in binds {
                out.insert(b.name.clone());
                for p in &b.patterns {
                    collect_pattern_vars(p, out);
                }
            }
        }
        _ => {}
    }
    expr.for_each_child(&mut |c| collect_rebound_names(c, out));
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

/// Captured-demand info for the where-bound local functions in scope:
/// name → (arity, the outer variables the local's body forces on every
/// path). See `local_fn_demand` for how the sets are computed and the
/// scoping rules that keep the name-keyed injection sound.
type CapturedEnv = HashMap<String, (usize, HashSet<String>)>;

/// Demand every argument sitting in a strict position of the callee's
/// boolean mask. Shared by the prefix-application, SpecCall, and DictCall
/// arms of `demanded_vars_in`.
fn demand_strict_args<'a>(
    s: &mut HashSet<String>,
    mask: &[bool],
    args: impl IntoIterator<Item = &'a TExpr>,
    rec: &dyn Fn(&TExpr) -> HashSet<String>,
) {
    for (i, arg) in args.into_iter().enumerate() {
        if mask.get(i).copied().unwrap_or(false) {
            // A strict position forces the argument to WHNF only. For an
            // ACTION value, WHNF is the built (suspended) closure —
            // nothing inside the chain is forced, so it contributes no
            // demands (see whnf_only_demands).
            s.extend(whnf_only_demands(arg, rec));
        }
    }
}

/// The demand contribution of forcing `e` to WHNF in a position that does
/// NOT run it: for an action-typed expression the WHNF is a suspended
/// closure (the emitter defers the whole chain), so `seq`-ing or
/// strictly-passing an action value forces none of its interior — GHC
/// agrees (`act \`seq\` pure 42` never runs act). Everything else forces
/// normally. The clause-body walk stays un-gated: rows model demand when
/// the function RUNS, and a body-position chain does run.
fn whnf_only_demands(
    e: &TExpr,
    rec: &dyn Fn(&TExpr) -> HashSet<String>,
) -> HashSet<String> {
    if is_action_value_ty(&e.ty) {
        HashSet::new()
    } else {
        rec(e)
    }
}

/// Core boolean analysis: the set of free variables that are guaranteed to
/// be forced when `expr` is evaluated to WHNF. `env` carries the known
/// strictness rows of other functions (cross-function propagation);
/// `captured` the captured-demand sets of the where-bound local functions
/// in scope — a demanded, SATURATED call to such a local runs its body, so
/// the outer variables the body forces on every path are demanded at the
/// call site too (the only place `captured` is consulted).
fn demanded_vars_in(
    expr: &TExpr,
    env: &HashMap<String, Vec<bool>>,
    captured: &CapturedEnv,
    shadowed: Shadowed,
) -> HashSet<String> {
    let rec = |e: &TExpr| demanded_vars_in(e, env, captured, shadowed);
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

            // The boolean `seq` rule (both spellings): strict in its FIRST
            // argument only (it forces it to WHNF) and lazy in the rest, so
            // a parameter passed as `seq`'s first argument is demanded,
            // while the second stays lazy. Mirror the runtime `__mll_seq` /
            // inline lowering exactly. (The structured analysis claims the
            // second operand too; see the shared-rules section.)
            if let TExprKind::Var(name) = &f.kind
                && name == "seq" && !shadowed("seq") && !args_rev.is_empty() {
                    s.extend(whnf_only_demands(args_rev[0], &rec));
                }

            // Cross-function propagation: if callee is a known function
            // and is strict in position i, demand that argument's vars.
            // A shadowed callee name may be a local of unknown strictness:
            // no row applies (under-claim, safe).
            if let TExprKind::Var(name) = &f.kind
                && !shadowed(name)
                && let Some(callee_strict) = env.get(name) {
                    demand_strict_args(&mut s, callee_strict, args_rev.iter().copied(), &rec);
                }

            // Captured-demand propagation: a call to a where-bound local
            // function runs its body whenever the call's result is demanded
            // — and this arm is only reached in demanded position, the same
            // condition under which the argument demand above fires — so
            // the outer variables the body forces on every path (`go`'s
            // guard `i > n` forcing the captured `n`) are demanded here
            // too. Gated on SATURATION: a partial application only builds
            // a closure and forces none of the body's captures.
            if let TExprKind::Var(name) = &f.kind
                && !shadowed(name)
                && let Some((arity, caps)) = captured.get(name)
                    && args_rev.len() >= *arity {
                        s.extend(caps.iter().cloned());
                    }

            s
        }

        TExprKind::InfixApp { op, lhs, rhs } => {
            // A backtick operator is an ordinary identifier; when a local
            // binder shadows it (`` a `div` b `` under a parameter named
            // div), the call targets an unknown local function — claim only
            // the callee name itself, like the App arm does for unknown
            // callees.
            if op.starts_with(|c: char| c.is_alphabetic() || c == '_') && shadowed(op) {
                let mut s = HashSet::new();
                s.insert(op.clone());
                return s;
            }
            match shared_op_operands(op, lhs) {
                Some(OpOperands::Both) => {
                    let mut s = rec(lhs);
                    s.extend(rec(rhs));
                    s
                }
                Some(OpOperands::Lhs) => rec(lhs),
                Some(OpOperands::Neither) => HashSet::new(),
                None => match op.as_str() {
                    // Backtick `a `seq` b`: same rule as prefix `seq a b`
                    // (see the App arm above — first operand only, WHNF).
                    "seq" => whnf_only_demands(lhs, &rec),
                    // Monadic bind/sequence forces both actions.
                    ">>=" | ">>" => {
                        let mut s = rec(lhs);
                        s.extend(rec(rhs));
                        s
                    }
                    // Unreachable: the table returns None only for the ops
                    // above. Claiming nothing stays sound regardless.
                    _ => HashSet::new(),
                },
            }
        }

        TExprKind::Negate(e) => rec(e),

        TExprKind::Paren(e) => rec(e),

        TExprKind::If { cond, then_branch, else_branch } => if_demand(
            cond,
            then_branch,
            else_branch,
            shadowed,
            &mut |e: &TExpr| rec(e),
            &mut |e: &TExpr| rec(e),
        ),

        TExprKind::Case { scrutinee, branches } => {
            // Per-branch demand sets, plus whether the branch demands one of
            // its own pattern binders (for an irrefutable pattern every
            // binder aliases the WHOLE scrutinee, so demanding one forces it).
            let branch_data: Vec<(HashSet<String>, bool)> = branches.iter().map(|b| {
                let body_demanded = if b.guards.is_empty() {
                    rec(b.plain_body())
                } else {
                    demanded_guards_with(&b.guards, env, captured, shadowed, &|s| s)
                };
                let bound = pattern_bound_vars(&b.pattern);
                let demands_binding = !body_demanded.is_disjoint(&bound);
                (
                    body_demanded.difference(&bound).cloned().collect::<HashSet<_>>(),
                    demands_binding,
                )
            }).collect();
            // The scrutinee's demand is keyed on the FIRST pattern, exactly
            // like codegen's entry force: an irrefutable first pattern binds
            // the scrutinee UNEVALUATED (later branches are unreachable), so
            // scrutinizing forces nothing — unless the selected branch's own
            // body demands the binding, which forces the alias. When the
            // first pattern inspects the value, scrutinizing forces to WHNF
            // only (an action-typed scrutinee yields its suspended closure).
            let first_forces = branches
                .first()
                .is_none_or(|b| b.pattern.forces_scrutinee());
            let mut s = if first_forces || branch_data.first().is_some_and(|(_, db)| *db) {
                whnf_only_demands(scrutinee, &rec)
            } else {
                HashSet::new()
            };
            // Intersect demanded vars across all branches.
            let mut branch_iter = branch_data.into_iter().map(|(set, _)| set);
            if let Some(first) = branch_iter.next() {
                let intersection = branch_iter.fold(first, |acc, s| &acc & &s);
                s.extend(intersection);
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
            // VALUE bindings only, mirroring let_group_close and
            // analyze_equation's close: demanding a local FUNCTION means
            // demanding the closure, which runs nothing — pulling in its
            // body's demands (including its own parameter names) would
            // over-claim strictness for a first-class reference.
            for bind in binds {
                if !bind.patterns.is_empty() {
                    continue;
                }
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
                demand_strict_args(&mut s, callee_strict, args.iter(), &rec);
            }
            s
        }

        TExprKind::DictCall { func_name, value_args, .. } => {
            // Typeclass method call: look up the method's strictness.
            let mut s = HashSet::new();
            if let Some(callee_strict) = env.get(func_name.as_str()) {
                demand_strict_args(&mut s, callee_strict, value_args.iter(), &rec);
            }
            s
        }

        TExprKind::DictMethod { dict, .. } => rec(dict),

        TExprKind::DictAccess { .. } => {
            HashSet::new()
        }

        TExprKind::RecordUpdate { record, updates: _, .. } => {
            // Codegen forces the record to copy it, but the update fields are
            // lazy positions (arg_ast(val, false), like constructor fields):
            // `r { a = e }` suspends `e`, so nothing inside the updates is
            // forced when the new record is built.
            rec(record)
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

/// The variable names bound by a pattern, as a set (`TPattern::for_each_var`).
fn pattern_bound_vars(pat: &TPattern) -> HashSet<String> {
    let mut vars = HashSet::new();
    collect_pattern_vars(pat, &mut vars);
    vars
}

fn collect_pattern_vars(pat: &TPattern, vars: &mut HashSet<String>) {
    pat.for_each_var(&mut |v| { vars.insert(v.to_string()); });
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
// see experiments/tracker/PERF-REGRESSION.md.
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
#[derive(Default)]
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
fn deep_of_ty(ty: &Ty) -> Demand {
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
/// Opaque outside this module: codegen only carries the map from
/// `local_fn_rows` into `demanded_map`/`demanded_map_guards`.
#[derive(Clone)]
pub struct LocalRows {
    run: Vec<Option<Demand>>,
    deep: Option<Vec<Option<Demand>>>,
    result_deep: Demand,
}

/// Identity of one TIR node: its address, branded with the lifetime of the
/// tree borrow it came from — `of` takes a `&'t TExpr`, so a key can only be
/// minted from a borrow that lives as long as the brand. Only this module
/// builds keys (the constructor is private), and only inside containers
/// that carry the same brand.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct NodeKey<'t>(usize, std::marker::PhantomData<&'t TExpr>);

impl<'t> NodeKey<'t> {
    fn of(node: &'t TExpr) -> Self {
        NodeKey(node as *const TExpr as usize, std::marker::PhantomData)
    }
}

/// A map keyed by TIR node identity, branded with the lifetime `'t` of the
/// tree borrow it indexes. Node identity used to be a bare
/// `&TExpr as *const _ as usize`, whose validity silently depended on the
/// TIR never being moved, rebuilt, or dropped between the pass that
/// collected the keys and the pass that looked them up — nothing enforced
/// it. The brand makes that dependency a compile-time fact: a `NodeMap`
/// cannot outlive the borrow of the tree it was built from, so its
/// addresses can never dangle. (Cloning a subtree still yields NEW
/// identities; producer/consumer pairs must walk the SAME borrow, which
/// each entry point here does by construction.)
pub struct NodeMap<'t, V> {
    /// The key type itself carries the `'t` brand, so the map's lifetime
    /// is pinned to the tree borrow through it — no separate phantom.
    map: HashMap<NodeKey<'t>, V>,
}

impl<'t, V> NodeMap<'t, V> {
    fn new() -> Self {
        NodeMap { map: HashMap::new() }
    }
    fn with_capacity(n: usize) -> Self {
        NodeMap { map: HashMap::with_capacity(n) }
    }
    pub fn get(&self, node: &'t TExpr) -> Option<&V> {
        self.map.get(&NodeKey::of(node))
    }
    fn get_key(&self, key: NodeKey<'t>) -> Option<&V> {
        self.map.get(&key)
    }
    fn get_mut(&mut self, node: &'t TExpr) -> Option<&mut V> {
        self.map.get_mut(&NodeKey::of(node))
    }
    fn insert(&mut self, node: &'t TExpr, v: V) {
        self.map.insert(NodeKey::of(node), v);
    }
}

/// Records every fully-applied call-head visit: Var node →
/// `(callee, joined result demand, fully applied)`.
type CallSites<'t> = NodeMap<'t, (String, Demand, bool)>;

/// Everything the structured walker needs to look up. `'t` is the brand
/// of the tree the optional site recorder indexes (see [`NodeMap`]) —
/// independent of `'a` so the recorder can outlive the per-iteration
/// lookup borrows.
struct RowCx<'a, 't> {
    rows: &'a Rows,
    locals: &'a HashMap<String, LocalRows>,
    inlined: &'a dyn Fn(&str) -> bool,
    /// Locally-bound-name predicate (see [`Shadowed`]): row lookups and
    /// special-name arms are suppressed for names it claims. Module-level
    /// constructors pass `&|_| false` (clause_demand_map adds each
    /// clause's own binders); codegen entry points pass their live
    /// local_vars membership.
    shadowed: Shadowed<'a>,
    /// When present, records every fully-applied call-head visit.
    sites: Option<&'a std::cell::RefCell<CallSites<'t>>>,
}

impl<'a, 't> RowCx<'a, 't> {
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

    fn record_site(&self, head: &'t TExpr, name: &str, rd: &Demand, full: bool) {
        if let Some(sites) = self.sites
            && (self.rows.arity.contains_key(name) || self.locals.contains_key(name)) {
                let mut sites = sites.borrow_mut();
                match sites.get_mut(head) {
                    Some((_, d, f)) => { *d = d.join(rd); *f = *f && full; }
                    None => { sites.insert(head, (name.to_string(), rd.clone(), full)); }
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
        // As-pattern: the match places the inner pattern's demand; when the
        // inner pattern proves nothing, the body's demand on the whole-value
        // binder applies. (Taking one of the two is an under-approximation —
        // always sound for a demand.)
        TPattern::As(name, inner) => {
            pattern_demand(inner, body).or_else(|| body.get(name).cloned())
        }
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
fn demand_expr<'t>(cx: &RowCx<'_, 't>, expr: &'t TExpr, rd: &Demand, run_pos: bool) -> DemandMap {
    let head = |e: &'t TExpr| demand_expr(cx, e, &Demand::Head, false);
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
            // A backtick operator shadowed by a local binder targets an
            // unknown local function: claim only the callee name (mirrors
            // the boolean arm and demand_app's unknown-callee rule).
            op2 if op2.starts_with(|c: char| c.is_alphabetic() || c == '_')
                && (cx.shadowed)(op2) => {
                let mut m = DemandMap::new();
                map_join_one(&mut m, op2, Demand::Head);
                m
            }
            // `return $ x` / `pure $ x` — see `pure_demand_map`. Any other
            // `f $ x` resolves through the shared table (Lhs: the function
            // is forced, the argument thunked). A shadowed `pure`/`return`
            // is an ordinary local call.
            "$" if matches!(&lhs.kind, TExprKind::Var(n)
                if (n == "pure" || n == "return") && !(cx.shadowed)(n)) => {
                pure_demand_map(cx, rhs, rd, run_pos)
            }
            // Cons under an element demand: the head is an element and the
            // tail carries the same element demand. At plain WHNF nothing
            // is forced (the shared table's Neither, restated here because
            // the element case must be checked first).
            ":" => {
                if let Demand::Elems(de) = rd {
                    let mut m = demand_expr(cx, lhs, de, false);
                    map_join(&mut m, demand_expr(cx, rhs, rd, false));
                    m
                } else {
                    DemandMap::new()
                }
            }
            // `a seq b` — see `seq_demand_map`.
            "seq" => seq_demand_map(cx, lhs, Some(rhs), rd, run_pos),
            // Flattened bind chain: the left action runs, then the
            // continuation runs (an earlier raise only replaces one ⊥ with
            // another). The bound variable's demand in the continuation IS
            // the result demand placed on the left action.
            // OUTSIDE run position the chain is SUSPENDED by the emitter
            // (a deferred closure, same as demand_app's `suspended` gate):
            // a seq'd or strictly-passed chain value is only built, so no
            // interior demand fires — GHC never runs `act` in
            // `act `seq` pure 42`.
            ">>=" | ">>" if is_action_value_ty(&expr.ty) && !run_pos => DemandMap::new(),
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
            // Everything else follows the shared operand table.
            op => match shared_op_operands(op, lhs) {
                Some(OpOperands::Both) => {
                    let mut m = head(lhs);
                    map_join(&mut m, head(rhs));
                    m
                }
                Some(OpOperands::Lhs) => head(lhs),
                Some(OpOperands::Neither) | None => DemandMap::new(),
            },
        },

        TExprKind::If { cond, then_branch, else_branch } => if_demand(
            cond,
            then_branch,
            else_branch,
            cx.shadowed,
            &mut |e: &'t TExpr| demand_expr(cx, e, &Demand::Head, false),
            &mut |e: &'t TExpr| demand_expr(cx, e, rd, run_pos),
        ),

        TExprKind::Case { scrutinee, branches } => {
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
            let branch_data: Vec<(DemandMap, Option<Demand>)> = branches.iter().map(|b| {
                let mut bm = if b.guards.is_empty() {
                    demand_expr(cx, b.plain_body(), rd, run_pos)
                } else {
                    demand_guards_map(cx, &b.guards, rd, run_pos)
                };
                let pat_d = pattern_demand(&b.pattern, &bm);
                for v in pattern_bound_vars(&b.pattern) {
                    bm.remove(&v);
                }
                if let (Some(v), Some(d)) = (&scrut_var, &pat_d) {
                    map_join_one(&mut bm, v, d.clone());
                }
                (bm, pat_d)
            }).collect();
            // Keyed on the FIRST pattern like codegen's entry force: an
            // irrefutable first pattern binds the scrutinee UNEVALUATED, so
            // its computation runs only if the selected branch's body demands
            // the binding — and then exactly to the degree the body does
            // (pattern_demand). A forcing first pattern scrutinizes at entry.
            let first_forces = branches
                .first()
                .is_none_or(|b| b.pattern.forces_scrutinee());
            let mut m = if first_forces {
                head(scrutinee)
            } else if let Some((_, Some(d))) = branch_data.first() {
                demand_expr(cx, scrutinee, d, false)
            } else {
                DemandMap::new()
            };
            let mut branch_maps = branch_data.into_iter().map(|(bm, _)| bm);
            if let Some(first) = branch_maps.next() {
                let isect = branch_maps.fold(first, |acc, bm| map_meet(&acc, &bm));
                map_join(&mut m, isect);
            }
            m
        }

        TExprKind::Let { binds, body } => {
            let mut m = demand_expr(cx, body, rd, run_pos);
            let_group_close(cx, binds, &mut m);
            m
        }

        TExprKind::SpecCall { original, specialized, args } => {
            let mut m = DemandMap::new();
            // Tuple projection: the argument's projected field carries the
            // whole expression's demand.
            if let SpecKind::TupGet(idx) = specialized
                && *idx >= 1
                && args.len() == 1
            {
                let mut fields: Vec<Option<Demand>> = vec![None; *idx];
                fields[*idx - 1] = Some(rd.clone());
                map_join(&mut m, demand_expr(cx, &args[0], &Demand::Fields(fields), false));
                return m;
            }
            if let Some(row) = cx.callee_row(original, &Demand::Head) {
                apply_callee_row(cx, &mut m, &row, args.iter());
            }
            m
        }

        TExprKind::DictCall { func_name, value_args, .. } => {
            let mut m = DemandMap::new();
            if let Some(row) = cx.callee_row(func_name, &Demand::Head) {
                apply_callee_row(cx, &mut m, &row, value_args.iter());
            }
            m
        }

        TExprKind::DictMethod { dict, .. } => head(dict),

        TExprKind::RecordUpdate { record, updates: _, .. } => {
            // The record is forced (copied); the update fields are suspended
            // (arg_ast(val, false)), so they contribute no demand here. Field
            // demand surfaces only when a consumer forces the field, and this
            // analysis has no per-field record rows to route that through.
            head(record)
        }

        TExprKind::OutgoingCallback { callee, .. } => head(callee),
        TExprKind::FfiMaybeArg { value } => head(value),
    }
}

/// Apply a callee's structured parameter row: each argument in a demanded
/// position contributes its own demands under that position's demand.
/// Shared by the prefix-application, SpecCall, and DictCall arms.
fn apply_callee_row<'t>(
    cx: &RowCx<'_, 't>,
    m: &mut DemandMap,
    row: &[Option<Demand>],
    args: impl IntoIterator<Item = &'t TExpr>,
) {
    for (i, arg) in args.into_iter().enumerate() {
        if let Some(Some(d)) = row.get(i).map(|x| x.as_ref()) {
            map_join(m, demand_expr(cx, arg, d, false));
        }
    }
}

/// The structured `seq` rule, shared by its prefix (`seq a b`) and
/// backtick (`a `seq` b`) spellings: the first operand is forced to WHNF,
/// and the second — which IS the expression's result — carries the whole
/// expression's demand. (The boolean analysis deliberately claims only the
/// first operand; see the shared-rules section.)
fn seq_demand_map<'t>(
    cx: &RowCx<'_, 't>,
    first: &'t TExpr,
    second: Option<&'t TExpr>,
    rd: &Demand,
    run_pos: bool,
) -> DemandMap {
    let mut m = demand_expr(cx, first, &Demand::Head, false);
    if let Some(b) = second {
        map_join(&mut m, demand_expr(cx, b, rd, run_pos));
    }
    m
}

/// The structured `return e` / `pure e` rule, shared by its prefix and `$`
/// spellings: non-strict at WHNF; under a deeper result demand in run
/// position, `e` receives that demand (forcing a field of the yielded
/// value forces through `e`).
fn pure_demand_map<'t>(cx: &RowCx<'_, 't>, arg: &'t TExpr, rd: &Demand, run_pos: bool) -> DemandMap {
    if run_pos && *rd != Demand::Head {
        demand_expr(cx, arg, rd, false)
    } else {
        DemandMap::new()
    }
}

/// Curried-application arm of `demand_expr`.
fn demand_app<'t>(cx: &RowCx<'_, 't>, expr: &'t TExpr, rd: &Demand, run_pos: bool) -> DemandMap {
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

    // `seq a b` — see `seq_demand_map`; the callee name itself is demanded.
    // Shadowed specials are ordinary local calls (fall through to the
    // generic path, whose row lookups are suppressed below).
    if fname == Some("seq") && !(cx.shadowed)("seq") {
        if let Some(a) = args.first() {
            let second = if args.len() == 2 { Some(args[1]) } else { None };
            map_join(&mut m, seq_demand_map(cx, a, second, rd, run_pos));
        }
        map_join_one(&mut m, "seq", Demand::Head);
        return m;
    }

    // `return e` / `pure e` — see `pure_demand_map`.
    if let (Some(name @ ("return" | "pure")), 1) = (fname, args.len())
        && !(cx.shadowed)(name) {
        map_join(&mut m, pure_demand_map(cx, args[0], rd, run_pos));
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

    // Row lookups and site recording are name-keyed: a shadowed callee
    // name may be a local of unknown strictness, so no row applies (and
    // recording the site would attribute it to the global). EXEMPT: a name
    // with an installed LOCAL row — those maps are scope-managed by their
    // installer (clause_local_rows / where_binds_stmts), and the ambient
    // predicate's local_vars contains the where-fn names themselves.
    if let Some(name) = fname
        && (cx.locals.contains_key(name) || !(cx.shadowed)(name)) {
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
                apply_callee_row(cx, &mut m, &row, args.iter().copied());
            }
        }
    }

    // gen_arg's context-free eager floor: provably-total argument
    // expressions are evaluated in place at every emitted call site, so
    // their reads happen when the call is emitted — except for inlined
    // callees (substitution, not gen_arg) and suspended actions.
    if !suspended {
        let skip = fname.is_some_and(|n| (cx.inlined)(n) && !(cx.shadowed)(n));
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

/// Guard-chain variant of `demand_expr` — the chain rule itself lives in
/// `guard_chain`.
fn demand_guards_map<'t>(cx: &RowCx<'_, 't>, guards: &'t [TGuard], rd: &Demand, run_pos: bool) -> DemandMap {
    guard_chain(
        guards,
        cx.shadowed,
        &mut |c: &'t TExpr| demand_expr(cx, c, &Demand::Head, false),
        &mut |b: &'t TExpr| demand_expr(cx, b, rd, run_pos),
    )
}

/// Demand map of one clause body under result demand `rd`, closed over the
/// clause's where-bound VALUE definitions (a demanded where-binding's RHS
/// demands fire too, exactly as codegen's demanded_bindings evaluates it).
fn clause_demand_map<'t>(cx: &RowCx<'_, 't>, clause: TLocalDefLike<'t>, rd: &Demand) -> DemandMap {
    // The clause's own binders extend the ambient shadowed predicate for
    // the whole walk (same clause-wide over-approximation as the boolean
    // analysis): where-fn names keep their cx.locals rows, except those
    // WITHOUT a row (dropped by the local-row scope filter), whose calls
    // must not hit a same-named global's row.
    let mut own = clause_shadowed_names(&clause);
    for b in clause.where_binds {
        if !b.patterns.is_empty() && !cx.locals.contains_key(&b.name) {
            own.insert(b.name.clone());
        }
    }
    let ambient = cx.shadowed;
    let shadowed = move |n: &str| own.contains(n) || ambient(n);
    let cx = &RowCx { shadowed: &shadowed, ..*cx };
    let mut m = if clause.guards.is_empty() {
        let body = clause.body.expect("guard-free clause carries a body");
        demand_expr(cx, body, rd, true)
    } else {
        demand_guards_map(cx, clause.guards, rd, true)
    };
    // A clause's where-bound VALUES close exactly like a `let` group.
    let_group_close(cx, clause.where_binds, &mut m);
    m
}

/// Per-parameter demands of one clause under result demand `rd`:
/// pattern-match demands joined with the body's demands on pattern
/// variables.
fn clause_param_row<'t>(cx: &RowCx<'_, 't>, clause: TLocalDefLike<'t>, arity: usize, rd: &Demand) -> Vec<Option<Demand>> {
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
            // `all@(x:xs)` and parenthesized spellings seed like the plain
            // pattern beneath: the As binder adds a name, not structure —
            // without unwrapping, the as-spelling fell to the Head default
            // and the GFP could not recognize the same element-strict
            // recursion it proves for the plain spelling (a sound
            // under-claim, but a needless parity gap between spellings).
            let mut p = p;
            while let TPattern::As(_, inner) | TPattern::Paren(inner) = p {
                p = inner.as_ref();
            }
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

/// A uniform BORROWED view over top-level clauses and where-bound local
/// function equations, for shared row computation. Borrowing matters:
/// views are rebuilt per equation per round of two nested fixpoints
/// (`analyze`, `local_fn_rows`), and the owned version deep-cloned every
/// pattern, guard, body, and where-bind each time — then cloned AGAIN
/// (`as_clause`) to call the row functions.
#[derive(Clone, Copy)]
struct TLocalDefLike<'a> {
    patterns: &'a [TPattern],
    guards: &'a [TGuard],
    /// Same body/guards exclusion as `TClause` for a clause view; a
    /// local-def view always has `Some` (where binds carry no guards).
    body: Option<&'a TExpr>,
    where_binds: &'a [TLocalDef],
}

fn clause_view(c: &TClause) -> TLocalDefLike<'_> {
    TLocalDefLike {
        patterns: &c.patterns,
        guards: &c.guards,
        body: c.body.as_ref(),
        where_binds: &c.where_binds,
    }
}

fn local_def_view(d: &TLocalDef) -> TLocalDefLike<'_> {
    TLocalDefLike {
        patterns: &d.patterns,
        guards: &[],
        body: Some(&d.body),
        where_binds: &[],
    }
}

/// Compute run/deep rows for a group of equations (a function) under the
/// current environment.
fn equations_rows<'t>(
    cx: &RowCx<'_, 't>,
    eqs: &[&TLocalDefLike<'t>],
    arity: usize,
    result_deep: Option<&Demand>,
) -> (Vec<Option<Demand>>, Option<Vec<Option<Demand>>>) {
    let row_under = |rd: &Demand| -> Vec<Option<Demand>> {
        let mut row: Option<Vec<Option<Demand>>> = None;
        for eq in eqs {
            let r = clause_param_row(cx, **eq, arity, rd);
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
/// flows through helpers like `reverse`'s `go` accumulator loop. Public
/// because codegen threads the same rows into `demanded_map` /
/// `demanded_map_guards` (via its scoped `local_demand_rows` map), so the
/// demanded-binding decision sees exactly what the rows analysis saw.
pub fn local_fn_rows(cx_rows: &Rows, inlined: &dyn Fn(&str) -> bool, shadowed: Shadowed, where_binds: &[TLocalDef]) -> HashMap<String, LocalRows> {
    let groups = group_where_fn_equations(where_binds);
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
            let cx = RowCx { rows: cx_rows, locals: &locals, inlined, shadowed, sites: None };
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

/// Close a `let` (or where-VALUE) group's demand map over its own
/// bindings: pull in the demands of demanded value bindings, re-walking a
/// binding when its own demand deepens, then drop the group's names (they
/// are bound here, not free). Terminates: demands only deepen and the
/// lattice is finite for finite programs. Shared by `demand_expr`'s `Let`
/// arm, by `clause_demand_map` (a clause's where-bound values follow the
/// same rule), and by `let_spine_maps`, which must reproduce the `Let` arm
/// exactly.
fn let_group_close<'t>(cx: &RowCx<'_, 't>, binds: &'t [TLocalDef], m: &mut DemandMap) {
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
                    map_join(m, demand_expr(cx, &b.body, &d, false));
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
}

/// The demand maps of every suffix of a nested-`let` spine, in ONE backward
/// pass: for each node `e` on the spine (each nested `Let` and the final
/// non-`let` terminal), the returned map holds `e`'s address →
/// `demanded_map(e, …, rd)`, computed exactly as the per-node call would.
///
/// Purpose: the action-chain emitter needs, per `let` statement, the demand
/// map of the REST of the chain (its seed for let-to-case eagerization).
/// Calling `demanded_map` on the remaining suffix at every statement re-walks
/// the tail each time — quadratic over a long do-block of `let`s. One
/// backward pass gives all the suffix maps: the spine's demand flows from
/// the terminal back through each group via the same `let_group_close` the
/// recursive walk uses, so each returned map is identical to what the
/// per-node call computes.
///
/// Returns `None` when `expr` is not a `Let` (no spine to precompute).
pub fn let_spine_maps<'t>(
    expr: &'t TExpr,
    rows: &Rows,
    locals: &HashMap<String, LocalRows>,
    inlined: &dyn Fn(&str) -> bool,
    shadowed: Shadowed,
    rd: &Demand,
) -> Option<NodeMap<'t, DemandMap>> {
    if !matches!(expr.kind, TExprKind::Let { .. }) {
        return None;
    }
    let cx = RowCx { rows, locals, inlined, shadowed, sites: None };
    // Collect the spine top-down.
    let mut spine: Vec<&TExpr> = Vec::new();
    let mut cur = expr;
    while let TExprKind::Let { body, .. } = &cur.kind {
        spine.push(cur);
        cur = body;
    }
    let terminal = cur;
    let mut maps: NodeMap<'_, DemandMap> = NodeMap::with_capacity(spine.len() + 1);
    // Backward pass: the terminal's map, then each enclosing group's.
    let mut m = demand_expr(&cx, terminal, rd, true);
    maps.insert(terminal, m.clone());
    for node in spine.iter().rev() {
        if let TExprKind::Let { binds, .. } = &node.kind {
            let_group_close(&cx, binds, &mut m);
        }
        maps.insert(node, m.clone());
    }
    Some(maps)
}

/// Public entry point for codegen: the demand map of `expr` evaluated with
/// result demand `rd` in run position (a bind-chain statement or a clause
/// body — the positions codegen flattens). `locals` carries the rows of
/// the where-bound local functions in scope (see `local_fn_rows`); without
/// them a demand that flows through a call to a where-local is invisible
/// and the binding stays conservatively thunked.
pub fn demanded_map(
    expr: &TExpr,
    rows: &Rows,
    locals: &HashMap<String, LocalRows>,
    inlined: &dyn Fn(&str) -> bool,
    shadowed: Shadowed,
    rd: &Demand,
) -> DemandMap {
    let cx = RowCx { rows, locals, inlined, shadowed, sites: None };
    demand_expr(&cx, expr, rd, true)
}

/// Guard-chain variant of `demanded_map`.
pub fn demanded_map_guards(
    guards: &[TGuard],
    rows: &Rows,
    locals: &HashMap<String, LocalRows>,
    inlined: &dyn Fn(&str) -> bool,
    shadowed: Shadowed,
    rd: &Demand,
) -> DemandMap {
    let cx = RowCx { rows, locals, inlined, shadowed, sites: None };
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

    // Same-named functions (a user definition shadowing a prelude one)
    // share one meet-combined entry — see `group_by_name`.
    let fn_groups = group_by_name(&functions);

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
                if let Ty::Arrow(_, rest, _) = rty {
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
                    // Each clause gets its own where scope. Module level:
                    // no ambient shadowing (clause_demand_map adds each
                    // clause's own binders).
                    let locals = local_fn_rows(&rows, &inlined, &|_| false, &clause.where_binds);
                    let cx = RowCx { rows: &rows, locals: &locals, inlined: &inlined, shadowed: &|_| false, sites: None };
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
    let mut refs: HashMap<String, HashSet<NodeKey<'_>>> = HashMap::new();
    let mut poisoned: HashSet<String> = HashSet::new();
    for func in &functions {
        for clause in &func.clauses {
            let mut exprs: Vec<&TExpr> = clause.body.iter().collect();
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
        let sites = std::cell::RefCell::new(CallSites::new());
        for func in &functions {
            if func.clauses.is_empty() {
                continue;
            }
            let rd = rows.result_demand(&func.name);
            for clause in &func.clauses {
                let locals = local_fn_rows(&rows, &inlined, &|_| false, &clause.where_binds);
                let cx = RowCx { rows: &rows, locals: &locals, inlined: &inlined, shadowed: &|_| false, sites: Some(&sites) };
                // Clause body + where-value closure (records sites).
                let _ = clause_demand_map(&cx, clause_view(clause), &rd);
                // Local function bodies run with unknown result demand
                // (through clause_demand_map so each equation's own
                // binders extend the shadowed predicate).
                for b in &clause.where_binds {
                    if !b.patterns.is_empty() {
                        let _ = clause_demand_map(&cx, local_def_view(b), &Demand::Head);
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
                match sites.get_key(*r) {
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
fn collect_fn_refs<'t>(
    expr: &'t TExpr,
    fn_names: &HashSet<&str>,
    refs: &mut HashMap<String, HashSet<NodeKey<'t>>>,
    poisoned: &mut HashSet<String>,
) {
    // What THIS node contributes — a Var reference site, a SpecCall/DictCall
    // that poisons its callee — then the children through for_each_child.
    match &expr.kind {
        TExprKind::Var(name) => {
            if fn_names.contains(name.as_str()) {
                refs.entry(name.clone())
                    .or_default()
                    .insert(NodeKey::of(expr));
            }
        }
        TExprKind::SpecCall { original, specialized, .. } => {
            if fn_names.contains(original.as_str()) {
                poisoned.insert(original.clone());
            }
            // Only the bare-host form can spell a tracked name directly
            // (the string protocol's other payloads carried a helper
            // prefix, so the old whole-string comparison never matched
            // them — preserved exactly by the typed match).
            if let SpecKind::Host(name) = specialized
                && fn_names.contains(name.as_str())
            {
                poisoned.insert(name.clone());
            }
        }
        TExprKind::DictCall { func_name, .. } => {
            if fn_names.contains(func_name.as_str()) {
                poisoned.insert(func_name.clone());
            }
        }
        _ => {}
    }
    expr.for_each_child(&mut |c| collect_fn_refs(c, fn_names, refs, poisoned));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Ty;

    fn int() -> Ty {
        Ty::Con("Int".into())
    }

    fn var(n: &str) -> TExpr {
        TExpr::new(TExprKind::Var(n.into()), int())
    }

    /// `seed_param` must see through As and Paren spellings: `go
    /// all@(x:xs)` seeds the same element demand as the plain `go (x:xs)`
    /// (the GFP seed decides whether recursion-carried element strictness
    /// can be recognized at all — see the seed rationale).
    #[test]
    fn seed_param_unwraps_as_and_paren() {
        let cons = TPattern::Constructor {
            name: ":".into(),
            args: vec![
                TPattern::Var("x".into(), int()),
                TPattern::Var("xs".into(), Ty::List(Box::new(int()))),
            ],
        };
        let plain = TLocalDefLike {
            patterns: std::slice::from_ref(&cons),
            guards: &[],
            body: None,
            where_binds: &[],
        };
        let wrapped_pat = TPattern::As(
            "all".into(),
            Box::new(TPattern::Paren(Box::new(cons.clone()))),
        );
        let wrapped = TLocalDefLike {
            patterns: std::slice::from_ref(&wrapped_pat),
            guards: &[],
            body: None,
            where_binds: &[],
        };
        assert_eq!(
            seed_param(&[&plain], 0),
            seed_param(&[&wrapped], 0),
            "as/paren spellings must seed like the plain pattern"
        );
        assert_eq!(
            seed_param(&[&wrapped], 0),
            Some(Demand::Elems(Box::new(Demand::Head))),
        );
    }

    /// A first-class reference to a let-bound local FUNCTION demands the
    /// closure, not its body: the body's free variables (and its own
    /// parameter names) must NOT enter the demanded set. Regression: the
    /// boolean Let arm had no function-bind guard (unlike let_group_close
    /// and analyze_equation's close), so `let g y = y + x in g` claimed
    /// {x, y} and a caller was marked strict in them.
    #[test]
    fn let_function_bind_body_not_demanded_by_first_class_ref() {
        let g_body = TExpr::new(
            TExprKind::InfixApp {
                op: "+".into(),
                lhs: Box::new(var("y")),
                rhs: Box::new(var("x")),
            },
            int(),
        );
        let expr = TExpr::new(
            TExprKind::Let {
                binds: vec![TLocalDef {
                    name: "g".into(),
                    patterns: vec![TPattern::Var("y".into(), int())],
                    body: g_body,
                }],
                body: Box::new(var("g")),
            },
            int(),
        );
        let env = HashMap::new();
        let captured = CapturedEnv::new();
        let demanded = demanded_vars_in(&expr, &env, &captured, &|_| false);
        assert!(
            demanded.is_empty(),
            "first-class local-fn ref must demand nothing, got {demanded:?}"
        );
    }

    /// The value-bind closure still works: a demanded let-bound VALUE
    /// pulls in its RHS's demands.
    #[test]
    fn let_value_bind_body_demanded() {
        let expr = TExpr::new(
            TExprKind::Let {
                binds: vec![TLocalDef {
                    name: "v".into(),
                    patterns: vec![],
                    body: var("x"),
                }],
                body: Box::new(var("v")),
            },
            int(),
        );
        let env = HashMap::new();
        let captured = CapturedEnv::new();
        let demanded = demanded_vars_in(&expr, &env, &captured, &|_| false);
        assert_eq!(
            demanded,
            HashSet::from(["x".to_string()]),
            "demanded value binding must pull in its RHS demands"
        );
    }
}
