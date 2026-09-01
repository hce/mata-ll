//! Whole-program analyses run before emission.
//!
//! `analyze_call_sites` determines, per function, which parameter positions
//! receive cheap (non-thunk) arguments at every call site — such parameters
//! never need `__force` at entry — and which positions can take the
//! SITE-FORCED convention (`analyze_param_conventions`): the callee drops
//! its entry `__force` and every call site passes a WHNF argument instead,
//! emitting a force only when it cannot prove WHNF-ness (`forced_ast`).
//! `mark_hidden_call_site` accounts for the extra argument that `$` and `.`
//! emission introduces, so that position is never judged always-cheap or
//! site-forced. `find_inline_candidates` selects small pure functions
//! (single clause, simple patterns, no guards, no where bindings, cheap
//! body, not self-recursive) for call-site inlining by inline.rs, recording
//! per-parameter occurrence counts so the call site can refuse
//! substitutions that would duplicate argument work. (A constructor-
//! application gate used to exclude ctor bodies for a long-obsolete
//! expr_ast reason — and missed Lambda/If bodies anyway; the TIR-level
//! substitution the inliner performs handles constructors like any other
//! expression, so the gate is gone.)

use crate::tir::*;
use super::CodeGen;
use super::util::{count_name_occurrences, expr_references_name};

/// How a function parameter's WHNF obligation is discharged — computed once
/// (`analyze_param_conventions`), consumed by BOTH the parameter binding in
/// function_stmts and every call-site argument emission, so the two ends of
/// the calling convention cannot drift apart.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum ParamConv {
    /// Not demanded on every path — bound raw, forced per use.
    Lazy,
    /// Every call site passes a context-free cheap argument
    /// (`params_always_cheap`) — bound raw AND treated as concrete.
    Cheap,
    /// Forced on every path, and some delivery is outside the rewritable
    /// call sites — the callee forces once at entry (the classic protocol).
    EntryForced,
    /// Forced on every path, and EVERY delivery is a registered, covering
    /// call site — the sites pass WHNF (raw when provable, `__force`d
    /// otherwise) and the callee binds the argument bare as concrete.
    SiteForced,
}

/// Mutable state of the call-site scan: per parameter position, whether any
/// site passed a possibly-thunked argument, whether any site covered the
/// position with its own spine, and whether any delivery bypasses the
/// visible sites entirely (`ever_uncovered` — a partial application's
/// closure forwarding, the hidden `$`/`.` extras, a first-class escape).
pub(super) struct CallScan {
    pub(super) ever_thunked: std::collections::HashMap<String, Vec<bool>>,
    pub(super) ever_called: std::collections::HashMap<String, Vec<bool>>,
    pub(super) ever_uncovered: std::collections::HashMap<String, Vec<bool>>,
}

impl CodeGen {
    /// The dispatch predicate of function_stmts' single-clause arm, shared
    /// with analyze_param_conventions so the convention rows are computed
    /// for exactly the parameter-emission path the function will take.
    pub(super) fn emission_single_clause(func: &TFunction) -> bool {
        func.clauses.len() == 1 && func.clauses[0].guards.is_empty()
    }

    /// The all-simple-patterns predicate of the single-clause arm (its
    /// destructuring sibling forces per `forces_scrutinee` instead).
    pub(super) fn emission_all_simple(clause: &TClause) -> bool {
        clause.patterns.iter().all(|p| matches!(p, TPattern::Var(_, _) | TPattern::Wildcard))
    }

    /// Whole-program call-site analysis. For each function, determine which
    /// parameter positions always receive cheap (non-thunk) arguments, and
    /// hand the raw scan on to the convention analysis.
    pub(super) fn analyze_call_sites(&mut self, module: &TModule) {
        let mut scan = CallScan {
            ever_thunked: std::collections::HashMap::new(),
            ever_called: std::collections::HashMap::new(),
            ever_uncovered: std::collections::HashMap::new(),
        };
        for func in module.functions.iter().chain(module.instance_fns.iter()) {
            let num_params = func.clauses.iter().map(|c| c.patterns.len()).max().unwrap_or(0);
            if num_params > 0 {
                scan.ever_thunked.insert(func.name.clone(), vec![false; num_params]);
                scan.ever_called.insert(func.name.clone(), vec![false; num_params]);
                scan.ever_uncovered.insert(func.name.clone(), vec![false; num_params]);
            }
        }
        // Scan all function bodies (and where-clause bodies) for call sites
        for func in module.functions.iter().chain(module.instance_fns.iter()) {
            for clause in &func.clauses {
                if let Some(b) = &clause.body {
                    self.scan_call_sites(b, &mut scan);
                }
                // Guard conditions and bodies are call sites too — without
                // scanning them, a recursive call that appears only inside a
                // guard (e.g. `f n | ... = f (g n)`) is missed, and the
                // parameter is wrongly judged always-cheap (concrete) while the
                // actual emission thunks the argument.
                for g in &clause.guards {
                    self.scan_call_sites(&g.condition, &mut scan);
                    self.scan_call_sites(&g.body, &mut scan);
                }
                for wb in &clause.where_binds {
                    self.scan_call_sites(&wb.body, &mut scan);
                }
            }
        }
        // A param is always-cheap only if it was called at least once and
        // never received a thunk at any call site.
        for (name, thunked) in &scan.ever_thunked {
            if let Some(called) = scan.ever_called.get(name) {
                let cheap: Vec<bool> = thunked.iter().zip(called.iter())
                    .map(|(t, c)| *c && !*t)
                    .collect();
                self.params_always_cheap.insert(name.clone(), cheap);
            }
        }
        self.analyze_param_conventions(module, &scan);
    }

    /// Decide each parameter's convention (see `ParamConv`), mirroring the
    /// force decisions the three parameter-emission paths of function_stmts
    /// make — through the SHARED dispatch predicates above and the same
    /// demand/always-cheap rows, so there is one source of truth — and
    /// upgrading an entry force to the site-forced convention where every
    /// delivery is visible:
    ///
    ///   * the position is forced on every path (demand row, or the
    ///     scrutinizing pattern that pins an entry force), so evaluating
    ///     the argument at the site cannot change what is forced;
    ///   * some site covers it and NO delivery bypasses the sites
    ///     (`ever_uncovered`): no partial application forwards into it, no
    ///     `$`/`.` hidden extra reaches it, the function never escapes as
    ///     a value;
    ///   * the function is not exported — a Lua host calls exports with
    ///     arguments no site rewrite can reach. (Host values are WHNF, but
    ///     a host can hand back a lazy structure it got from us.)
    ///
    /// Every general call-site emission (general_call_ast both branches,
    /// DictCall value arguments, the user-operator infix call) consults the
    /// row and passes site-forced positions through `forced_ast`; in
    /// WHNF-refutation builds the callee binds the parameter under
    /// `__assert_whnf`, so the corpus second pass checks the whole contract.
    fn analyze_param_conventions(&mut self, module: &TModule, scan: &CallScan) {
        let exported: std::collections::HashSet<&str> =
            module.exports.iter().map(|s| s.as_str()).collect();
        for func in module.functions.iter().chain(module.instance_fns.iter()) {
            let clauses = &func.clauses;
            if clauses.is_empty() { continue; }
            let num_params = clauses.iter().map(|c| c.patterns.len()).max().unwrap_or(0);
            if num_params == 0 { continue; }
            let cheap_row = self.params_always_cheap.get(&func.name);
            let strict_row = self.demand_info.strict_params.get(&func.name);
            let called = scan.ever_called.get(&func.name);
            let uncovered = scan.ever_uncovered.get(&func.name);
            let site_ok = |i: usize| -> bool {
                !exported.contains(func.name.as_str())
                    && called.is_some_and(|v| v.get(i).copied().unwrap_or(false))
                    && !uncovered.is_none_or(|v| v.get(i).copied().unwrap_or(true))
            };
            let single = Self::emission_single_clause(func);
            let all_simple = single && Self::emission_all_simple(&clauses[0]);
            let mut row = Vec::with_capacity(num_params);
            for i in 0..num_params {
                let cheap = cheap_row.is_some_and(|v| v.get(i).copied().unwrap_or(false));
                let strict = strict_row.is_some_and(|v| v.get(i).copied().unwrap_or(false));
                let upgraded = |forced: bool| if !forced {
                    ParamConv::Lazy
                } else if site_ok(i) {
                    ParamConv::SiteForced
                } else {
                    ParamConv::EntryForced
                };
                let conv = if all_simple {
                    // Single simple clause: always-cheap wins outright, then
                    // the demand row decides the entry force.
                    if cheap { ParamConv::Cheap } else { upgraded(strict) }
                } else if single {
                    // Destructuring single clause: forced exactly when the
                    // pattern scrutinizes the argument.
                    upgraded(clauses[0].patterns.get(i).is_some_and(TPattern::forces_scrutinee))
                } else {
                    // Multiple clauses / guards: forced when the FIRST
                    // clause scrutinizes the position (tried on every call)
                    // or the demand row proves every path forces it;
                    // always-cheap only marks concreteness otherwise.
                    let needs_force = clauses.first()
                        .is_some_and(|c| c.patterns.get(i).is_some_and(TPattern::forces_scrutinee));
                    if needs_force || strict { upgraded(true) }
                    else if cheap { ParamConv::Cheap }
                    else { ParamConv::Lazy }
                };
                row.push(conv);
            }
            self.param_conv.insert(func.name.clone(), row);
        }
    }

    /// The convention row for a callee named at a call site, or None when
    /// the name does not denote the module-level function the row was
    /// computed for (a where-bound local function's row lives in
    /// local_strict_params; any other local binder shadowing the name makes
    /// the callee an unknown local value).
    pub(super) fn callee_conv_row(&self, name: &str) -> Option<&Vec<ParamConv>> {
        if self.local_strict_params.contains_key(name) || self.is_local_shadowed(name) {
            return None;
        }
        self.param_conv.get(name)
    }

    pub(super) fn scan_call_sites(&self, expr: &TExpr, scan: &mut CallScan) {
        // Iterative right-spine walk for bind chains
        let mut expr = expr;
        loop {
            match &expr.kind {
                TExprKind::InfixApp { op, lhs, rhs } if op == ">>=" || op == ">>" => {
                    self.scan_call_sites(lhs, scan);
                    if let TExprKind::Lambda { body, .. } = &rhs.kind {
                        expr = body;
                        continue;
                    }
                    expr = rhs;
                    continue;
                }
                TExprKind::Let { binds, body } => {
                    for bind in binds { self.scan_call_sites(&bind.body, scan); }
                    expr = body;
                    continue;
                }
                _ => break,
            }
        }
        match &expr.kind {
            TExprKind::App(_, _) => {
                let mut args: Vec<&TExpr> = vec![];
                let mut f = expr;
                while let TExprKind::App(inner_f, inner_arg) = &f.kind {
                    args.push(inner_arg.as_ref());
                    f = inner_f.as_ref();
                }
                args.reverse();
                if let TExprKind::Var(name) = &f.kind {
                    Self::register_call_site(name, &args, scan);
                }
                for arg in &args {
                    self.scan_call_sites(arg, scan);
                }
                if !matches!(&f.kind, TExprKind::Var(_) | TExprKind::Con(_)) {
                    self.scan_call_sites(f, scan);
                }
            }
            TExprKind::InfixApp { op, lhs, rhs } if op == "$" || op == "." => {
                // These operators emit hidden call sites (see their expr_ast
                // arms) that the generic recursion below cannot see:
                //   f $ x  calls f with a thunked x appended to f's spine args;
                //   f . g  builds a closure that calls g with the closure's raw
                //          parameter (possibly a thunk, when the composition is
                //          applied to a non-cheap argument) and calls f with
                //          g's result (which may itself be a raw thunk, e.g.
                //          when g just returns its lazy parameter).
                // Without registering them, a parameter that is cheap at every
                // direct call site but receives a thunk here is wrongly judged
                // always-cheap and emitted without __force. Mark one extra,
                // conservatively thunked argument position on each operand's
                // application spine.
                Self::mark_hidden_call_site(lhs, scan);
                if op == "." {
                    Self::mark_hidden_call_site(rhs, scan);
                }
                // A bare-Var `$` operand is callee-like — the hidden-site
                // mark above accounts for the argument it receives, and
                // the emission calls it directly — so it skips the generic
                // scan, which would poison it as an escaping reference
                // (the Var arm below). `.` operands DO escape: the
                // composition is a value whose runtime forwards extra
                // arguments raw, so a Var there takes the poison path.
                if op != "$" || !matches!(&lhs.kind, TExprKind::Var(_)) {
                    self.scan_call_sites(lhs, scan);
                }
                self.scan_call_sites(rhs, scan);
            }
            TExprKind::InfixApp { op, lhs, rhs } => {
                // A user-defined operator IS a saturated two-argument call
                // of the operator function (operator_infix_ast emits
                // `op(lhs, rhs)`): register it like a spine call so the
                // position data covers these sites too. Builtin operators
                // never key the maps (they are not module functions), and a
                // locally SHADOWED backtick operator re-enters expr_ast as
                // an App spine of the local — the registration here then
                // attributes marks to the module function that are not real
                // deliveries to it, which can only add conservative bits.
                if scan.ever_thunked.contains_key(op.as_str()) {
                    let args = [lhs.as_ref(), rhs.as_ref()];
                    Self::register_call_site(op, &args, scan);
                }
                self.scan_call_sites(lhs, scan);
                self.scan_call_sites(rhs, scan);
            }
            TExprKind::Lambda { body, .. } => self.scan_call_sites(body, scan),
            TExprKind::If { cond, then_branch, else_branch } => {
                self.scan_call_sites(cond, scan);
                self.scan_call_sites(then_branch, scan);
                self.scan_call_sites(else_branch, scan);
            }
            TExprKind::Let { binds, body } => {
                for bind in binds { self.scan_call_sites(&bind.body, scan); }
                self.scan_call_sites(body, scan);
            }
            TExprKind::Case { scrutinee, branches } => {
                self.scan_call_sites(scrutinee, scan);
                for b in branches {
                    for g in &b.guards {
                        self.scan_call_sites(&g.condition, scan);
                        self.scan_call_sites(&g.body, scan);
                    }
                    if let Some(bb) = &b.body {
                        self.scan_call_sites(bb, scan);
                    }
                }
            }
            TExprKind::Paren(inner) | TExprKind::Negate(inner) => self.scan_call_sites(inner, scan),
            TExprKind::Tuple(elems) => { for e in elems { self.scan_call_sites(e, scan); } }
            TExprKind::SpecCall { specialized, args, .. } => {
                // A specialization EMBEDS mata-ll function names in its
                // payload — an element eq/show/compare threaded through a
                // runtime helper (`__mll_maybe_eq(eq_elem, …)` hands the
                // raw lazy payloads of both Justs to eq_elem), a key
                // encoder, a dictionary's method table. Each is a
                // first-class escape the argument scan below cannot see:
                // poison exactly like the Var arm. (Caught by the mapM
                // corpus case: the Maybe-eq helper fed raw thunks to a
                // list-eq whose parameters had been granted the
                // site-forced convention.)
                for name in Self::spec_embedded_fn_names(specialized) {
                    if let Some(thunked) = scan.ever_thunked.get_mut(name) {
                        for t in thunked.iter_mut() { *t = true; }
                    }
                    if let Some(uncov) = scan.ever_uncovered.get_mut(name) {
                        for u in uncov.iter_mut() { *u = true; }
                    }
                }
                for a in args { self.scan_call_sites(a, scan); }
            }
            TExprKind::OutgoingCallback { callee, .. } => self.scan_call_sites(callee, scan),
            TExprKind::FfiMaybeArg { value } => self.scan_call_sites(value, scan),
            // A call site hidden inside an unscanned subtree is invisible to
            // the always-cheap judgment: a visible site passing a cheap
            // argument then marks the position concrete while the hidden one
            // passes a thunk — the callee reads the raw thunk table. These
            // three node kinds carry expressions and used to fall through to
            // the catch-all.
            TExprKind::RecordUpdate { record, updates, .. } => {
                self.scan_call_sites(record, scan);
                for (_, _, val) in updates {
                    self.scan_call_sites(val, scan);
                }
            }
            TExprKind::DictMethod { dict, .. } => self.scan_call_sites(dict, scan),
            TExprKind::DictCall { func_name, dict_args, value_args } => {
                // A DictCall IS a call site of func_name: its value
                // arguments are emitted with the plain lazy argument
                // protocol (arg_ast, never strict-eager), positionally
                // aligned with the function's value parameters — register
                // them exactly like a spine call.
                let value_refs: Vec<&TExpr> = value_args.iter().collect();
                Self::register_call_site(func_name, &value_refs, scan);
                for a in dict_args { self.scan_call_sites(a, scan); }
                for a in value_args { self.scan_call_sites(a, scan); }
            }
            // A bare reference to a known function OUTSIDE call-head
            // position: the function VALUE escapes — an argument to a
            // higher-order function, a tuple or cons field, a stored
            // closure — and whoever eventually calls it passes raw lazy
            // arguments this site scan cannot see. Its always-cheap
            // judgment must die, and every position counts as delivered
            // outside the visible sites (no site-forced convention).
            // (Refuted by backend_fuzz index 61: `const 5 True` judged
            // const's first parameter always-cheap from the one visible
            // site, then `flip const (Just True) (null [])` in a lazy
            // tuple field called that copy flat with a raw thunk — whose
            // bare `return x` forwarded it, a thunk body returning a raw
            // thunk. Call HEADS never reach this arm: the App arm above
            // registers them without recursing into the head, which is
            // what keeps direct calls precise.)
            TExprKind::Var(name) => {
                if let Some(thunked) = scan.ever_thunked.get_mut(name) {
                    for t in thunked.iter_mut() {
                        *t = true;
                    }
                }
                if let Some(uncov) = scan.ever_uncovered.get_mut(name) {
                    for u in uncov.iter_mut() {
                        *u = true;
                    }
                }
            }
            _ => {}
        }
    }

    /// Every mata-ll function name a specialization payload embeds — the
    /// element/method functions its runtime helper or dictionary calls with
    /// raw lazy arguments (see the SpecCall arm of scan_call_sites). Host
    /// paths (`Host`/`Io`/`Const`/…) are Lua names, not module functions,
    /// and never key the scan maps anyway.
    fn spec_embedded_fn_names(spec: &SpecKind) -> Vec<&str> {
        match spec {
            SpecKind::ListEq(n) | SpecKind::MaybeEq(n)
            | SpecKind::ShowList(n) | SpecKind::ShowMaybe(n)
            | SpecKind::ListCmp(n) | SpecKind::MaybeCmp(n)
            | SpecKind::KeyEncList(n) | SpecKind::KeyEncMaybe(n) => vec![n.as_str()],
            SpecKind::TupleEq(ns) | SpecKind::TupleCmp(ns) | SpecKind::KeyEncTuple(ns) =>
                ns.iter().map(|s| s.as_str()).collect(),
            SpecKind::HmOp { enc, cmp, .. } =>
                enc.iter().chain(cmp.iter()).map(|s| s.as_str()).collect(),
            SpecKind::OrdFromCmp { cmp, .. } => vec![cmp.as_str()],
            SpecKind::Dict { methods, .. } =>
                methods.iter().map(|(_, impl_fn)| impl_fn.as_str()).collect(),
            SpecKind::DictCtor { methods, .. } =>
                methods.iter().map(|(_, impl_fn, _)| impl_fn.as_str()).collect(),
            _ => vec![],
        }
    }

    /// Register one call site of `name`: mark each argument position called,
    /// and thunked unless the argument is guaranteed eager. A parameter is
    /// judged "always cheap" (the callee then skips forcing it and treats it
    /// as a value) only when EVERY call site passes an argument that arg_ast
    /// is guaranteed to evaluate eagerly regardless of context. That
    /// guarantee is the *context-free floor* of is_cheap_to_force: cheap
    /// structure built without leaning on any variable's WHNF-ness
    /// (var_ok = false) and free of trapping ops. arg_ast's eager set
    /// (strict OR is_cheap_to_force) is a superset of this, so whenever the
    /// callee assumes a value one was passed. Any other argument may be
    /// thunked by arg_ast, so mark the position thunked here.
    fn register_call_site(name: &str, args: &[&TExpr], scan: &mut CallScan) {
        if let Some(thunked) = scan.ever_thunked.get_mut(name) {
            let called = scan.ever_called.get_mut(name).unwrap();
            for (i, arg) in args.iter().enumerate() {
                if i < thunked.len() {
                    called[i] = true;
                    if !Self::is_cheap_with(arg, &|_| false)
                        || Self::contains_trapping_op(arg)
                    {
                        thunked[i] = true;
                    }
                }
            }
            // A PARTIAL application covers only its spine positions. The
            // remaining parameters are delivered later through the closure
            // general_call_ast builds, which forwards its `_pa` parameters
            // RAW — whoever calls the partial application (a runtime
            // generic's element loop, a stored closure) passes lazy
            // arguments this scan cannot see. Every uncovered position must
            // therefore be judged thunked: with only the covered positions
            // registered, one full call elsewhere with cheap arguments
            // granted always-cheap on a position a partial application
            // delivers a raw thunk to — the callee skipped its entry force
            // and inspected the thunk table as a value (a native `==`
            // against it returned false: a wrong RESULT, not a crash).
            let uncov = scan.ever_uncovered.get_mut(name).unwrap();
            for i in args.len()..thunked.len() {
                thunked[i] = true;
                uncov[i] = true;
            }
        }
    }

    /// Register the hidden call site that `$` and `.` emission creates for an
    /// operand (see the scan_call_sites `$`/`.` arm): the operand's spine
    /// callee receives one extra argument, in the position right after its
    /// explicit spine arguments, and that argument may be a thunk — so the
    /// position must never be judged always-cheap.
    pub(super) fn mark_hidden_call_site(operand: &TExpr, scan: &mut CallScan) {
        let mut extra_pos = 0usize;
        let mut f = operand;
        loop {
            match &f.kind {
                TExprKind::Paren(inner) => f = inner.as_ref(),
                TExprKind::App(inner_f, _) => {
                    extra_pos += 1;
                    f = inner_f.as_ref();
                }
                _ => break,
            }
        }
        if let TExprKind::Var(name) = &f.kind
            && let Some(thunked) = scan.ever_thunked.get_mut(name.as_str()) {
                let called = scan.ever_called.get_mut(name.as_str()).unwrap();
                if extra_pos < thunked.len() {
                    called[extra_pos] = true;
                }
                // Poison every position from the hidden argument ON: when the
                // operand's callee has parameters past extra_pos, the `$`/`.`
                // result is a function value and any further application
                // reaches those positions through closure forwarding this
                // scan cannot see — the same raw-delivery door the partial-
                // application rule in register_call_site closes. These are
                // deliveries outside the visible sites, so they also bar the
                // site-forced convention.
                let uncov = scan.ever_uncovered.get_mut(name.as_str()).unwrap();
                for i in extra_pos..thunked.len() {
                    thunked[i] = true;
                    uncov[i] = true;
                }
            }
    }

    /// Identify small pure functions eligible for inlining at call sites.
    /// Criteria: single clause, all-simple patterns, no guards, no where bindings,
    /// body is cheap, and not self-recursive.
    pub(super) fn find_inline_candidates(&mut self, module: &TModule) {
        for func in module.functions.iter().chain(module.instance_fns.iter()) {
            if func.clauses.len() != 1 { continue; }
            let clause = &func.clauses[0];
            if !clause.guards.is_empty() || !clause.where_binds.is_empty() { continue; }
            if clause.patterns.is_empty() { continue; } // value binding, not a function
            let all_simple = clause.patterns.iter().all(|p| matches!(p, TPattern::Var(_, _)));
            if !all_simple { continue; }
            if !Self::is_cheap(clause.plain_body()) { continue; }
            if expr_references_name(clause.plain_body(), &func.name) { continue; } // recursive
            let params: Vec<String> = clause.patterns.iter().map(|p| {
                if let TPattern::Var(name, _) = p { name.clone() } else { unreachable!() }
            }).collect();
            // Per-parameter emission counts (work-duplication measure): the
            // call-site gate in expr.rs substitutes a non-trivial argument
            // only for a parameter whose count is at most one — substituting
            // it at two occurrences (`sq x = x * x` applied to `nfib 30`)
            // would emit and EVALUATE the argument twice, a sharing loss
            // GHC's inliner never allows. Recording the counts here (once
            // per candidate) instead of re-walking the body per call site.
            let occ_counts: Vec<usize> = params
                .iter()
                .map(|p| count_name_occurrences(clause.plain_body(), p))
                .collect();
            self.inline_fns.insert(func.name.clone(), (params, clause.plain_body().clone(), occ_counts));
        }
    }
}
