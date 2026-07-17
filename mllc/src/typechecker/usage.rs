//! Affine-usage checking for `%1` arrows — the enforcement half of mata-ll's
//! linear types. Runs at the end of `check_function`, over the fully
//! substituted typed IR of each clause, so every arrow multiplicity a
//! program pinned down (directly in a signature, or through unification —
//! e.g. a lambda checked against a `%1` parameter) is visible on the types.
//!
//! WHAT IS ENFORCED (the affine fragment). A binder is *affine* — limited to
//! at most one use — when it is:
//!   * an argument bound at a `%1` arrow of the function's own type
//!     (including every variable bound inside that argument's pattern),
//!   * a lambda parameter whose lambda type resolved to a `%1` arrow, or
//!   * a *derived* alias of an affine value: a pattern binder of a `case`
//!     whose scrutinee uses an affine variable, or a `<-` binder whose
//!     action uses one (both exempting scalar-typed binders, see below).
//!
//! HOW USES ARE COUNTED (the 0/1/ω lattice). Sequential composition adds
//! usages; the alternatives of a `case`/`if`/guard take the per-variable
//! maximum (a use in *every* branch is still one use, because only one
//! branch runs). A use is charged where the variable occurs, scaled by what
//! the surrounding context may do with it:
//!   * an argument position charges by the applied function's arrow: `%1`
//!     charges one use, a plain `->` (or an arrow whose multiplicity nothing
//!     determined) charges ω — an unrestricted function may duplicate it;
//!   * data-constructor fields (including `:` and tuple components) charge
//!     one use: a constructor stores the value exactly once, and any later
//!     duplication of the *container* is charged where the container is
//!     duplicated;
//!   * a `let`/`where` value binding charges its right-hand side scaled by
//!     how often the bound name is used (0/1/ω). This is the laziness rule:
//!     a thunk is FORCED at most once (the runtime memoizes, see codegen's
//!     __force), but the value it yields is consumed once per use of the
//!     binder, so the binder's use count — not the forcing count — is the
//!     sound bound. An unused binding charges nothing (never forced, never
//!     consumed: fine for affine). One relaxation: a binding whose value is
//!     a builtin scalar charges once even when used repeatedly — its
//!     right-hand side still runs at most once (memoization), and the
//!     scalar result duplicates harmlessly;
//!   * a variable captured under a lambda (or used inside a local
//!     where/let *function*) charges ω — the closure may be called any
//!     number of times;
//!   * the continuation of an IO/LuaIO `>>=`/`>>` charges its body ONCE:
//!     running the composed action runs the continuation exactly once, and
//!     running the action more than once requires using the action value
//!     more than once, which is charged at that use. Any other monad's
//!     continuation charges ω (a list bind runs it per element);
//!   * a fixed set of Prelude operators (`+`, `==`, `++`, …) charge one use
//!     per operand: they are primitives that consume each operand exactly
//!     once and cannot capture or duplicate it. (The Prelude cannot be
//!     redefined — the typechecker rejects that separately — so the set is
//!     stable.)
//!
//! SCALAR EXEMPTION. A *derived* binder (case/`<-` taint propagation) whose
//! type is a builtin scalar — Integer, Number, Bool, String, () — is not
//! made affine: a scalar is plain data, carries no close/free obligation,
//! and duplicating it cannot double-use a resource. This is the pragmatic
//! counterpart of linear-base's `Movable`; a binder the user annotates `%1`
//! DIRECTLY (`f :: Integer %1 -> …; f n = …`) is still enforced as written.
//!
//! WHAT IS NOT COVERED (documented boundary; all deviations are in the
//! REJECT direction except where noted):
//!   * Exactly-once (full linear) is not enforced — dropping an affine value
//!     is always allowed. That is the deliberate affine scope.
//!   * There is no multiplicity polymorphism: a helper that merely forwards
//!     its argument still charges ω unless its own signature says `%1`.
//!   * The Lua side of a `%1` FFI declaration is trusted: mata-ll charges
//!     the argument once per call and cannot see what the host does with it.

use super::*;
use crate::types::Mult;

/// A nonzero use count: one use, or "more than one / unbounded" (ω).
/// Zero is represented by absence from the usage map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Count {
    One,
    Many,
}

/// How often one variable is used, plus — when the count reached `Many`
/// through scaling rather than plain repetition — a plain-language reason.
#[derive(Debug, Clone)]
struct UseInfo {
    count: Count,
    cause: Option<String>,
}

/// Per-variable usage of an expression. Keys are every variable name the
/// expression references; only the affine-tracked ones are ever *checked*,
/// but counting all of them uniformly is what makes `let`-scaling and
/// shadowing work without a second bookkeeping structure.
type Usage = HashMap<String, UseInfo>;

/// Sequential composition: both usages happen.
fn add_usage(acc: &mut Usage, other: Usage) {
    for (k, v) in other {
        match acc.get_mut(&k) {
            None => { acc.insert(k, v); }
            Some(info) => {
                // One + anything nonzero = Many. A plain double occurrence
                // carries no cause — the default message ("used more than
                // once") is the accurate one.
                if info.cause.is_none() {
                    info.cause = v.cause;
                }
                info.count = Count::Many;
            }
        }
    }
}

/// Branch alternatives: only one side runs, so take the per-variable max.
fn join_usage(acc: &mut Usage, other: Usage) {
    for (k, v) in other {
        match acc.get_mut(&k) {
            None => { acc.insert(k, v); }
            Some(info) => {
                if matches!(v.count, Count::Many) {
                    info.count = Count::Many;
                    if info.cause.is_none() {
                        info.cause = v.cause;
                    }
                }
            }
        }
    }
}

/// Scale a usage by what the surrounding context may do with the value:
/// consume it exactly/at most once (`Once`), or any number of times (`Any`,
/// with a reason attached to every use it inflates).
#[derive(Clone, Copy)]
enum Factor {
    Once,
    Any,
}

fn scale_usage(u: Usage, factor: Factor, cause: &Option<String>) -> Usage {
    match factor {
        Factor::Once => u,
        Factor::Any => u
            .into_iter()
            .map(|(k, mut info)| {
                if matches!(info.count, Count::One) {
                    info.count = Count::Many;
                    info.cause = cause.clone();
                }
                (k, info)
            })
            .collect(),
    }
}

/// Prelude operators that consume each operand exactly once: strict
/// primitives over scalars/lists that cannot capture or duplicate an
/// operand. (`.` and `$` are deliberately absent — composition captures its
/// operands in a closure, and `$` is an ordinary unrestricted function.)
/// The Prelude cannot be redefined (the typechecker rejects interference
/// with it), so matching by name is reliable.
const CONSUME_ONCE_OPS: &[&str] = &[
    "+", "-", "*", "/", "^", "==", "/=", "<", ">", "<=", ">=",
    "&&", "||", "<>", "++", "!!", "div", "mod", "rem", "quot",
];

/// Prelude functions that consume their argument exactly once, by the same
/// reasoning as data constructors: `pure`/`return` store the value in an
/// action/container exactly once (duplicating the RESULT is charged where it
/// is duplicated), `id` passes it through, and `fst`/`snd` project one
/// component and drop the other (dropping is fine — affine, not linear).
/// Applies only when the name is not shadowed by a local binder (see
/// `UsageCk::locals`); top-level redefinition of a Prelude name is rejected
/// elsewhere, so the Prelude meaning is otherwise guaranteed.
const CONSUME_ONCE_FNS: &[&str] = &["pure", "return", "id", "fst", "snd"];

/// Builtin scalar types: plain data with no resource obligation, exempt
/// from *derived* (taint) affinity. See the module comment.
fn is_scalar(ty: &Ty) -> bool {
    match ty {
        Ty::Unit => true,
        Ty::Con(n) => matches!(n.as_str(), "Integer" | "Number" | "Bool" | "String"),
        _ => false,
    }
}

/// One affine violation: which binder, why it was limited to one use, and —
/// when the overuse came from scaling — why the context may use it more
/// than once.
struct Violation {
    name: String,
    origin: String,
    cause: Option<String>,
}

/// The variables a typed pattern binds, with their types. `direct` is true
/// only for a bare top-level `Var` pattern — the binder that IS the `%1`
/// argument itself (enforced even at scalar type, because the user wrote the
/// annotation on it), as opposed to a binder destructured out of it.
fn pattern_binders(p: &TPattern, direct: bool, out: &mut Vec<(String, Ty, bool)>) {
    match p {
        TPattern::Var(n, ty) => out.push((n.clone(), ty.clone(), direct)),
        TPattern::Wildcard | TPattern::LitPat(_) => {}
        TPattern::Paren(inner) => pattern_binders(inner, direct, out),
        TPattern::Constructor { args, .. } => {
            for a in args {
                pattern_binders(a, false, out);
            }
        }
        TPattern::Tuple(elems) => {
            for e in elems {
                pattern_binders(e, false, out);
            }
        }
    }
}

/// The walker. Borrows the checker read-only (for the operator environment);
/// violations are collected and turned into diagnostics by the driver.
struct UsageCk<'a> {
    ck: &'a Checker,
    /// name -> why this binder is limited to at most one use. Doubles as the
    /// taint set: a scrutinee/action that *uses* any name in here makes the
    /// binders it feeds affine too.
    affine: HashMap<String, String>,
    /// Names currently bound by a LOCAL binder (nesting count). Guards the
    /// `CONSUME_ONCE_FNS` whitelist: a local binding named `pure` is not the
    /// Prelude's `pure`.
    locals: HashMap<String, u32>,
    viols: Vec<Violation>,
}

/// Saved shadowing state for a scope's binders, restored on exit.
type SavedBinders = Vec<(String, Option<String>)>;

/// Bookkeeping for one `let`/`where` binding group, split into enter/exit
/// halves so the iterative bind-chain walker can interleave group scopes
/// with its spine frames.
struct GroupCtx {
    saved: SavedBinders,
    rhs_usages: Vec<Usage>,
}

impl<'a> UsageCk<'a> {
    /// Shadow `name` for an inner scope (returns the outer entry to restore).
    fn shadow(&mut self, name: &str) -> (String, Option<String>) {
        *self.locals.entry(name.to_string()).or_insert(0) += 1;
        (name.to_string(), self.affine.remove(name))
    }

    fn restore(&mut self, saved: SavedBinders) {
        for (name, old) in saved {
            if let Some(c) = self.locals.get_mut(&name) {
                *c = c.saturating_sub(1);
            }
            match old {
                Some(origin) => { self.affine.insert(name, origin); }
                None => { self.affine.remove(&name); }
            }
        }
    }

    /// Is `name` currently bound by a local binder (as opposed to naming a
    /// top-level / Prelude definition)?
    fn is_local(&self, name: &str) -> bool {
        self.locals.get(name).is_some_and(|c| *c > 0)
    }

    /// Record a violation if `u` shows the affine binder `name` used more
    /// than once.
    fn check_binder(&mut self, u: &Usage, name: &str) {
        if let Some(info) = u.get(name)
            && matches!(info.count, Count::Many) {
                let origin = self.affine.get(name).cloned().unwrap_or_default();
                self.viols.push(Violation {
                    name: name.to_string(),
                    origin,
                    cause: info.cause.clone(),
                });
            }
    }

    /// Does this usage consume any affine-tracked variable? Returns the
    /// first such name (for the taint-origin message).
    fn taint_source(&self, u: &Usage) -> Option<String> {
        let mut names: Vec<&String> = u.keys().filter(|k| self.affine.contains_key(*k)).collect();
        // Deterministic pick for stable diagnostics.
        names.sort();
        names.first().map(|s| (*s).to_string())
    }

    // --- Expression usage ---

    /// Usage of an expression. Bind chains (`>>=`/`>>` spines from
    /// do-blocks) are dispatched to the iterative walker — they are the one
    /// TIR shape whose depth is NOT bounded by the expression-nesting limit,
    /// exactly as in `TExpr::apply_subst`.
    fn expr_usage(&mut self, e: &TExpr) -> Usage {
        match &e.kind {
            TExprKind::InfixApp { op, .. } if op == ">>=" || op == ">>" => {
                self.bind_chain_usage(e)
            }
            _ => self.expr_usage_node(e),
        }
    }

    fn expr_usage_node(&mut self, e: &TExpr) -> Usage {
        match &e.kind {
            TExprKind::Var(n) => {
                let mut u = Usage::new();
                u.insert(n.clone(), UseInfo { count: Count::One, cause: None });
                u
            }
            TExprKind::Con(_) | TExprKind::Lit(_) | TExprKind::OpFunc(_)
            | TExprKind::DictAccess { .. } => Usage::new(),

            TExprKind::App(f, a) => {
                let mut u = self.expr_usage(f);
                let ua = self.expr_usage(a);
                let (factor, cause) = self.app_arg_factor(f);
                add_usage(&mut u, scale_usage(ua, factor, &cause));
                u
            }

            TExprKind::InfixApp { op, lhs, rhs } => {
                // `>>=`/`>>` never reach here (dispatched in expr_usage);
                // `:` is the list constructor — each side is stored exactly
                // once, like any constructor field.
                if op == ":" {
                    let mut u = self.expr_usage(lhs);
                    add_usage(&mut u, self.expr_usage(rhs));
                    return u;
                }
                let (factor, cause) = self.op_operand_factor(op);
                let mut u = scale_usage(self.expr_usage(lhs), factor, &cause);
                add_usage(&mut u, scale_usage(self.expr_usage(rhs), factor, &cause));
                u
            }

            // Negation consumes its operand exactly once and returns a new
            // number — it cannot capture or duplicate.
            TExprKind::Negate(inner) => self.expr_usage(inner),

            TExprKind::Lambda { params, body } => {
                // Each parameter's multiplicity comes from the lambda's own
                // (post-substitution) arrow chain: a lambda that was checked
                // against a `%1` parameter carries `One` here.
                let mut cur = &e.ty;
                let mut saved: SavedBinders = Vec::new();
                let mut checked: Vec<String> = Vec::new();
                for (name, _) in params {
                    let m = if let Ty::Arrow(_, rest, m) = cur {
                        let m = *m;
                        cur = rest.as_ref();
                        m
                    } else {
                        Mult::Many
                    };
                    if name != "_" {
                        saved.push(self.shadow(name));
                        if matches!(m, Mult::One) {
                            self.affine.insert(
                                name.clone(),
                                "the function that binds it has a '%1' arrow \
                                 at this parameter".to_string(),
                            );
                            checked.push(name.clone());
                        }
                    }
                }
                let mut u = self.expr_usage(body);
                for name in &checked {
                    self.check_binder(&u, name);
                }
                for (name, _) in &saved {
                    u.remove(name);
                }
                self.restore(saved);
                // Whatever the lambda captured may be consumed once per
                // CALL, and nothing bounds how often a function value is
                // called.
                scale_usage(u, Factor::Any, &Some(
                    "it is captured by a function value, which may be called \
                     any number of times".to_string()))
            }

            TExprKind::If { cond, then_branch, else_branch } => {
                let mut u = self.expr_usage(cond);
                let mut branches = self.expr_usage(then_branch);
                join_usage(&mut branches, self.expr_usage(else_branch));
                add_usage(&mut u, branches);
                u
            }

            TExprKind::Case { scrutinee, branches } => {
                let u_s = self.expr_usage(scrutinee);
                // Taint: pattern binders of a match on an affine value alias
                // parts of that value, so they inherit the restriction
                // (scalar-typed binders exempted — plain data, no resource).
                let taint_src = self.taint_source(&u_s);
                let mut joined: Option<Usage> = None;
                for br in branches {
                    let mut binders = Vec::new();
                    pattern_binders(&br.pattern, true, &mut binders);
                    let mut saved: SavedBinders = Vec::new();
                    let mut checked: Vec<String> = Vec::new();
                    for (name, ty, _) in &binders {
                        saved.push(self.shadow(name));
                        if let Some(src) = &taint_src
                            && !is_scalar(ty) {
                                self.affine.insert(name.clone(), format!(
                                    "it is pattern-bound from '{}', which is \
                                     itself limited to at most one use", src));
                                checked.push(name.clone());
                            }
                    }
                    let mut bu = self.branch_usage(&br.guards, &br.body);
                    for name in &checked {
                        self.check_binder(&bu, name);
                    }
                    for (name, _, _) in &binders {
                        bu.remove(name);
                    }
                    self.restore(saved);
                    match &mut joined {
                        None => joined = Some(bu),
                        Some(acc) => join_usage(acc, bu),
                    }
                }
                let mut u = u_s;
                add_usage(&mut u, joined.unwrap_or_default());
                u
            }

            TExprKind::Let { binds, body } => {
                let gctx = self.group_enter(binds);
                let u_body = self.expr_usage(body);
                self.group_exit(binds, gctx, u_body)
            }

            TExprKind::Paren(inner) => self.expr_usage(inner),

            // Components of a tuple are stored exactly once, like
            // constructor fields; duplicating the tuple is charged wherever
            // the tuple itself is duplicated.
            TExprKind::Tuple(elems) => {
                let mut u = Usage::new();
                for el in elems {
                    add_usage(&mut u, self.expr_usage(el));
                }
                u
            }

            // An FFI call consumes each argument exactly once per call (what
            // the Lua host then does with it is the host's business — a `%1`
            // FFI signature is the user's assertion about that host
            // function).
            TExprKind::SpecCall { args, .. } => {
                let mut u = Usage::new();
                for a in args {
                    add_usage(&mut u, self.expr_usage(a));
                }
                u
            }
            TExprKind::OutgoingCallback { callee, .. } => self.expr_usage(callee),
            TExprKind::FfiMaybeArg { value } => self.expr_usage(value),

            // A record update reads the old record once and stores each new
            // field value once.
            TExprKind::RecordUpdate { record, updates, .. } => {
                let mut u = self.expr_usage(record);
                for (_, _, val) in updates {
                    add_usage(&mut u, self.expr_usage(val));
                }
                u
            }

            // Dictionary-passing forms are introduced by the monomorphizer,
            // after this pass; treated conservatively if ever encountered.
            TExprKind::DictMethod { dict, .. } => {
                let cause = Some("it is consumed through a typeclass \
                                  dictionary".to_string());
                scale_usage(self.expr_usage(dict), Factor::Any, &cause)
            }
            TExprKind::DictCall { dict_args, value_args, .. } => {
                let cause = Some("it is passed to a dictionary-passing \
                                  function".to_string());
                let mut u = Usage::new();
                for a in dict_args.iter().chain(value_args) {
                    let ua = self.expr_usage(a);
                    add_usage(&mut u, scale_usage(ua, Factor::Any, &cause));
                }
                u
            }
        }
    }

    /// Usage of one guarded body: every guard condition may run (charged
    /// sequentially — conservative for guards after the one that matches),
    /// while the guard bodies are alternatives (joined). The plain body is
    /// included in the join; when guards are present it is the synthetic
    /// fallthrough and contributes nothing.
    fn branch_usage(&mut self, guards: &[TGuard], body: &TExpr) -> Usage {
        if guards.is_empty() {
            return self.expr_usage(body);
        }
        let mut u = Usage::new();
        let mut bodies: Option<Usage> = None;
        for g in guards {
            add_usage(&mut u, self.expr_usage(&g.condition));
            let bu = self.expr_usage(&g.body);
            match &mut bodies {
                None => bodies = Some(bu),
                Some(acc) => join_usage(acc, bu),
            }
        }
        add_usage(&mut u, bodies.unwrap_or_default());
        u
    }

    /// How an application `f x` charges the uses inside `x`.
    fn app_arg_factor(&self, f: &TExpr) -> (Factor, Option<String>) {
        // Constructor applications (any depth of the spine) store each field
        // exactly once.
        let mut head = f;
        loop {
            match &head.kind {
                TExprKind::Paren(inner) => head = inner,
                TExprKind::App(g, _) => head = g,
                _ => break,
            }
        }
        if matches!(head.kind, TExprKind::Con(_)) {
            return (Factor::Once, None);
        }
        if let TExprKind::Var(n) = &head.kind
            && CONSUME_ONCE_FNS.contains(&n.as_str())
            && !self.is_local(n)
        {
            return (Factor::Once, None);
        }
        match &f.ty {
            Ty::Arrow(_, _, Mult::One) => (Factor::Once, None),
            _ => {
                let callee = match &head.kind {
                    TExprKind::Var(n) => Some(n.clone()),
                    TExprKind::OpFunc(n) => Some(format!("({})", n)),
                    _ => None,
                };
                let cause = match callee {
                    Some(n) => format!(
                        "it is passed to '{}', whose type does not promise to \
                         use this argument at most once (the arrow is '->', \
                         not '%1 ->')", n),
                    None => "it is passed to a function whose type does not \
                             promise to use this argument at most once (the \
                             arrow is '->', not '%1 ->')".to_string(),
                };
                (Factor::Any, Some(cause))
            }
        }
    }

    /// How an infix operator charges each operand. The fixed Prelude
    /// primitives consume each operand exactly once; anything else charges
    /// by the operator's declared arrow multiplicities (conservatively ω).
    fn op_operand_factor(&self, op: &str) -> (Factor, Option<String>) {
        if CONSUME_ONCE_OPS.contains(&op) {
            return (Factor::Once, None);
        }
        if let Some(scheme) = self.ck.env.lookup(op)
            && let Ty::Arrow(_, rest, m1) = &scheme.ty {
                let one_lhs = matches!(m1, Mult::One);
                let one_rhs = matches!(rest.as_ref(), Ty::Arrow(_, _, Mult::One));
                if one_lhs && one_rhs {
                    return (Factor::Once, None);
                }
            }
        (Factor::Any, Some(format!(
            "it is passed to the operator '{}', whose type does not promise \
             to use it at most once", op)))
    }

    // --- let/where binding groups ---

    /// Enter a (mutually recursive) binding group: shadow the bound names,
    /// walk every right-hand side, and mark the names whose value was built
    /// from an affine variable as affine themselves (taint), iterating to a
    /// fixpoint so sibling/recursive references are seen. Scalar-typed
    /// bindings are exempt from the taint, like scalar pattern binders.
    fn group_enter(&mut self, binds: &[TLocalDef]) -> GroupCtx {
        let saved: SavedBinders =
            binds.iter().map(|b| self.shadow(&b.name)).collect();
        if binds.is_empty() {
            return GroupCtx { saved, rhs_usages: Vec::new() };
        }
        // Taint fixpoint: a binding whose right-hand side consumes an
        // affine variable (or a tainted sibling) is tainted itself. The set
        // only grows and is bounded by the group, so this terminates; each
        // re-walk discards the previous iteration's violations so the final
        // walk reports each at most once.
        let mut tainted: HashSet<String> = HashSet::new();
        let mut rhs_usages: Vec<Usage>;
        loop {
            let viols_before = self.viols.len();
            rhs_usages = binds.iter().map(|b| self.binding_rhs_usage(b)).collect();
            let mut new_tainted = tainted.clone();
            for (b, u) in binds.iter().zip(&rhs_usages) {
                if b.patterns.is_empty()
                    && !is_scalar(&b.body.ty)
                    && self.taint_source(u).is_some()
                {
                    new_tainted.insert(b.name.clone());
                }
            }
            if new_tainted.len() == tainted.len() {
                break;
            }
            self.viols.truncate(viols_before);
            tainted = new_tainted;
            for name in &tainted {
                self.affine.entry(name.clone()).or_insert_with(||
                    "it holds a value built from a '%1'-limited variable"
                        .to_string());
            }
        }
        GroupCtx { saved, rhs_usages }
    }

    /// Usage of one binding's right-hand side. A local *function*'s
    /// parameters shadow within its body.
    fn binding_rhs_usage(&mut self, b: &TLocalDef) -> Usage {
        if b.patterns.is_empty() {
            return self.expr_usage(&b.body);
        }
        let mut binders = Vec::new();
        for p in &b.patterns {
            pattern_binders(p, true, &mut binders);
        }
        let saved: SavedBinders = binders.iter().map(|(n, _, _)| self.shadow(n)).collect();
        let mut u = self.expr_usage(&b.body);
        for (n, _, _) in &binders {
            u.remove(n);
        }
        self.restore(saved);
        u
    }

    /// Leave a binding group: charge each right-hand side scaled by how the
    /// bound name is used (the laziness rule — see the module comment), then
    /// unshadow.
    fn group_exit(&mut self, binds: &[TLocalDef], gctx: GroupCtx, u_body: Usage) -> Usage {
        let GroupCtx { saved, rhs_usages } = gctx;
        let mut result = u_body;
        // Names referenced by ANY right-hand side in the group — recursion or
        // sibling use. A binding reached that way may be evaluated on behalf
        // of another binding an unbounded number of times relative to this
        // analysis, so its charge is conservatively ω. Computed before the
        // group's names are stripped out of the RHS usages.
        let referenced_by_rhs: HashSet<&str> = binds
            .iter()
            .filter(|b| rhs_usages.iter().any(|u| u.contains_key(&b.name)))
            .map(|b| b.name.as_str())
            .collect();
        for (b, mut u_rhs) in binds.iter().zip(rhs_usages.into_iter()) {
            // Group names inside a right-hand side served the taint/recursion
            // analysis; they must not leak past the group.
            for other in binds {
                u_rhs.remove(&other.name);
            }
            let recursive = referenced_by_rhs.contains(b.name.as_str());
            let n_body = result.get(&b.name).map(|i| i.count);
            if n_body.is_none() && !recursive {
                // Never used: the thunk is never forced (and an uncalled
                // local function never runs), so nothing is consumed.
                continue;
            }
            let (factor, cause) = if !b.patterns.is_empty() {
                // Local function: its body runs once per CALL, and nothing
                // bounds how many calls a use of the name makes.
                (Factor::Any, Some(format!(
                    "it is used inside the local function '{}', which may \
                     be called any number of times", b.name)))
            } else if recursive {
                (Factor::Any, Some(format!(
                    "it is used by the recursive (or mutually recursive) \
                     local binding '{}', whose definition may be consumed \
                     any number of times", b.name)))
            } else if matches!(n_body, Some(Count::One)) {
                // Used exactly once: the thunk is forced at most once and
                // its value consumed once, so the right-hand side's own
                // usage passes through unchanged.
                (Factor::Once, None)
            } else if is_scalar(&b.body.ty) {
                // Used many times, but the value is a SCALAR: the runtime
                // memoizes thunks (see codegen's __force) and strict
                // assignment evaluates once, so the right-hand side — and
                // whatever affine value it consumed — runs at most once,
                // and duplicating the scalar result cannot double-use a
                // resource.
                (Factor::Once, None)
            } else {
                (Factor::Any, Some(format!(
                    "it is used through the local binding '{}', and '{}' is \
                     used more than once — each use of '{}' is another use \
                     of everything its definition consumed",
                    b.name, b.name, b.name)))
            };
            add_usage(&mut result, scale_usage(u_rhs, factor, &cause));
        }
        for b in binds {
            result.remove(&b.name);
        }
        self.restore(saved);
        result
    }

    // --- Iterative bind-chain walker ---

    /// Usage of a `>>=`/`>>` spine (do-block desugaring), processed
    /// iteratively like `TExpr::apply_subst` so a long do-block cannot
    /// overflow the native stack. IO/LuaIO continuations are charged once
    /// (see the module comment); other monads' continuations charge ω.
    fn bind_chain_usage(&mut self, e: &TExpr) -> Usage {
        // Group frames keep their binds slice in a parallel stack (a frame
        // cannot borrow it directly without tangling lifetimes).
        let mut group_binds: Vec<&[TLocalDef]> = Vec::new();
        let mut frames: Vec<BindFrame> = Vec::new();

        let mut current = e;
        loop {
            match &current.kind {
                TExprKind::InfixApp { op, lhs, rhs } if op == ">>=" || op == ">>" => {
                    let u_lhs = self.expr_usage(lhs);
                    let io = matches!(&lhs.ty, Ty::IO(_) | Ty::LuaIO(..));
                    let taint_src = self.taint_source(&u_lhs);
                    if let TExprKind::Lambda { params, body } = &rhs.kind {
                        // The continuation lambda of a do-bind. Its binder
                        // aliases the action's result: affine when the
                        // action consumed an affine variable (scalars
                        // exempt).
                        let mut saved: SavedBinders = Vec::new();
                        let mut checked: Vec<String> = Vec::new();
                        let mut names: Vec<String> = Vec::new();
                        for (name, pty) in params {
                            if name == "_" {
                                continue;
                            }
                            names.push(name.clone());
                            saved.push(self.shadow(name));
                            if let Some(src) = &taint_src
                                && !is_scalar(pty) {
                                    self.affine.insert(name.clone(), format!(
                                        "it was bound (with '<-') from an \
                                         action that consumes '{}', which is \
                                         limited to at most one use", src));
                                    checked.push(name.clone());
                                }
                        }
                        frames.push(BindFrame::Bind { u_lhs, io, params: names, saved, checked });
                        current = body;
                        continue;
                    }
                    // `>>`/`>>=` without a lambda continuation: the
                    // continuation value itself is consumed once by the
                    // bind; a non-IO monad may then run it many times.
                    let u_r = self.expr_usage(rhs);
                    let (factor, cause) = if io {
                        (Factor::Once, None)
                    } else {
                        (Factor::Any, Some(
                            "it is used under a monadic bind in a monad other \
                             than IO, whose continuation may run any number \
                             of times".to_string()))
                    };
                    let mut u = u_lhs;
                    add_usage(&mut u, scale_usage(u_r, factor, &cause));
                    return self.unwind_bind_frames(frames, group_binds, u);
                }
                TExprKind::Let { binds, body } => {
                    let gctx = self.group_enter(binds);
                    frames.push(BindFrame::Group { gctx });
                    group_binds.push(binds);
                    current = body;
                    continue;
                }
                _ => break,
            }
        }

        let u = self.expr_usage_node(current);
        self.unwind_bind_frames(frames, group_binds, u)
    }

    /// Unwind the collected bind-chain frames bottom-up, mirroring the
    /// recursive combination the frames replaced.
    fn unwind_bind_frames(
        &mut self,
        frames: Vec<BindFrame>,
        mut group_binds: Vec<&[TLocalDef]>,
        mut u: Usage,
    ) -> Usage {
        for frame in frames.into_iter().rev() {
            match frame {
                BindFrame::Bind { u_lhs, io, params, saved, checked } => {
                    for name in &checked {
                        self.check_binder(&u, name);
                    }
                    for name in &params {
                        u.remove(name);
                    }
                    self.restore(saved);
                    let (factor, cause) = if io {
                        (Factor::Once, None)
                    } else {
                        (Factor::Any, Some(
                            "it is used under a monadic bind in a monad other \
                             than IO, whose continuation may run any number \
                             of times".to_string()))
                    };
                    u = scale_usage(u, factor, &cause);
                    let mut combined = u_lhs;
                    add_usage(&mut combined, u);
                    u = combined;
                }
                BindFrame::Group { gctx } => {
                    let binds = group_binds.pop().expect("group frame without binds");
                    u = self.group_exit(binds, gctx, u);
                }
            }
        }
        u
    }
}

/// Frames of the iterative bind-chain walk (named at module level so the
/// helper above can take them).
enum BindFrame {
    Bind {
        u_lhs: Usage,
        io: bool,
        params: Vec<String>,
        saved: SavedBinders,
        checked: Vec<String>,
    },
    Group {
        gctx: GroupCtx,
    },
}

impl Checker {
    /// Enforce the affine (`%1`) usage discipline on one checked function:
    /// every binder bound at a `%1` arrow — and every derived alias of such
    /// a value — must be used at most once on every evaluation path. Runs
    /// over the final (post-substitution) typed clauses; pushes ordinary
    /// diagnostics. Functions that never touch a `%1` type produce an empty
    /// tracking set, so this is a no-op walk for them.
    pub(super) fn check_function_usage(&mut self, fun: &TFunction) {
        for clause in &fun.clauses {
            let mut walker = UsageCk {
                ck: self,
                affine: HashMap::new(),
                locals: HashMap::new(),
                viols: Vec::new(),
            };

            // Every clause-pattern binder is a local name for the whole
            // clause (relevant to the Prelude-name whitelist, e.g. a
            // parameter named 'pure').
            for pat in &clause.patterns {
                let mut binders = Vec::new();
                pattern_binders(pat, true, &mut binders);
                for (name, _, _) in binders {
                    *walker.locals.entry(name).or_insert(0) += 1;
                }
            }

            // Arguments bound at the function type's `%1` arrows. A binder
            // that IS the argument (a bare variable pattern) is enforced
            // even at scalar type — the user annotated it directly; binders
            // destructured out of it follow the scalar exemption.
            let mut cur = &fun.ty;
            while let Ty::Forall(_, inner) = cur {
                cur = inner;
            }
            let mut top_affine: Vec<String> = Vec::new();
            for pat in &clause.patterns {
                let Ty::Arrow(_, rest, m) = cur else { break };
                if matches!(m, Mult::One) {
                    let mut binders = Vec::new();
                    pattern_binders(pat, true, &mut binders);
                    for (name, ty, direct) in binders {
                        if direct || !is_scalar(&ty) {
                            walker.affine.insert(name.clone(), format!(
                                "the type of '{}' declares this argument \
                                 '%1'", fun.name));
                            top_affine.push(name);
                        }
                    }
                }
                cur = rest;
            }

            // Clause core (guards/body) wrapped in the where-binding group.
            let gctx = walker.group_enter(&clause.where_binds);
            let core = walker.branch_usage(&clause.guards, &clause.body);
            let u = walker.group_exit(&clause.where_binds, gctx, core);

            top_affine.sort();
            top_affine.dedup();
            for name in &top_affine {
                walker.check_binder(&u, name);
            }

            let viols = std::mem::take(&mut walker.viols);
            drop(walker);
            let span = clause.span.unwrap_or_default();
            for v in viols {
                let cause = v.cause.unwrap_or_else(|| {
                    "this definition uses it more than once along a single \
                     evaluation path".to_string()
                });
                self.push_error_span(
                    DiagnosticKind::Other(format!(
                        "'{}' is limited to at most one use — {} — but {}",
                        v.name, v.origin, cause)),
                    format!("definition of '{}'", fun.name),
                    span,
                );
                if let Some(diag) = self.errors.last_mut() {
                    diag.notes.push(
                        "a '%1' arrow is a promise that the function consumes \
                         the value at most once. A second use would act on a \
                         value that may already be gone — for an external \
                         resource such as a file handle, that is the \
                         double-close/double-free class of bug. To allow \
                         unrestricted use, write a plain '->' instead."
                            .to_string(),
                    );
                }
            }
        }
    }
}
