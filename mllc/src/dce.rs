//! Dead-code elimination over the monomorphized function set.
//!
//! Every program is compiled together with the mata-ll auto-prelude
//! (`map`/`filter`/`foldr`/… from `lib/Prelude.mll`), plus the monomorphized
//! specializations and derived instance methods produced by `mono`. Most
//! programs use a small fraction of these, yet all of them used to reach
//! codegen — so a one-line program carried the whole prelude as `__mll_fn`
//! slots.
//!
//! This pass keeps only the functions reachable from the program's entry
//! points — `main` and the exported functions — and drops the rest before
//! codegen. Reachability is the closure over the call graph: a function's
//! out-edges are the names it references in its clauses (Var/Con/OpFunc/
//! SpecCall/DictCall/DictAccess/operators) plus every constructor name its
//! patterns match. By construction no *kept* function can reference a
//! *dropped* one, so the emitted code stays well-formed; the integration
//! suite (which runs every output) is the backstop.
//!
//! Constructor-level DCE piggybacks on the same edge sets: a constructor is
//! live iff some kept function constructs it (a `Con`/`Var` reference) or
//! matches it in a pattern. A `data` definition none of whose constructors is
//! live moves from `data_defs` to `dropped_data_defs` — whole-definition
//! granularity, so tag numbering inside a kept definition never shifts.
//! Dropped definitions are NOT discarded: codegen still registers their
//! metadata (constructor tags, LuaDict string tags and field keys, FFI field
//! types) but emits no constructor functions for them. The metadata must
//! survive because a value of a dropped type can still flow through kept
//! code without being constructed or matched there — a LuaDict record built
//! by the Lua host and read only through field accessors needs its keyed
//! layout; only the constructor *functions* (`__mll_fn` slots) are dead
//! weight. This is what stops the four Prelude datatypes (`ExitValue`,
//! `Any`, `Either`, `Ordering` — 12 slots) from shipping in every file.

use std::collections::{HashMap, HashSet};
use crate::tir::*;

/// Drop functions and instance methods not reachable from `main`/exports.
pub fn eliminate(mut module: TModule) -> TModule {
    // Pass-order witness: DCE's reachability walk must see the final call
    // graph, after fold and split.
    debug_assert_eq!(
        module.passes_run.last(),
        Some(&"split"),
        "dce must run on the split module"
    );
    module.passes_run.push("dce");
    // Out-edges for every function in the universe (functions + instance_fns).
    let mut edges: HashMap<String, HashSet<String>> = HashMap::new();
    for f in module.functions.iter().chain(module.instance_fns.iter()) {
        let mut refs = HashSet::new();
        for c in &f.clauses {
            collect_clause(c, &mut refs);
        }
        edges.insert(f.name.clone(), refs);
    }
    // Only names defined here can be kept or dropped; everything else (runtime
    // prelude, constructors, builtins) is out of scope for this pass.
    let defined: HashSet<&str> = edges.keys().map(String::as_str).collect();

    // Roots: main (when present) and every export.
    let mut reachable: HashSet<String> = HashSet::new();
    let mut work: Vec<String> = Vec::new();
    if module.has_main && defined.contains("main") {
        reachable.insert("main".to_string());
        work.push("main".to_string());
    }
    for e in &module.exports {
        if defined.contains(e.as_str()) && reachable.insert(e.clone()) {
            work.push(e.clone());
        }
    }
    // Transitive closure over the call graph.
    while let Some(name) = work.pop() {
        if let Some(refs) = edges.get(&name) {
            for r in refs {
                if defined.contains(r.as_str()) && reachable.insert(r.clone()) {
                    work.push(r.clone());
                }
            }
        }
    }

    module.functions.retain(|f| reachable.contains(&f.name));
    module.instance_fns.retain(|f| reachable.contains(&f.name));

    // Constructor liveness: the union of every kept function's references.
    // `collect_clause` gathered both expression names (Var/Con) and pattern
    // constructor names into the same set, so this is exactly "constructed or
    // matched by live code". Set-union order does not matter — membership is
    // deterministic — and `data_defs` keeps its source order, so emission
    // stays deterministic.
    let mut used: HashSet<&str> = HashSet::new();
    for name in &reachable {
        if let Some(refs) = edges.get(name) {
            used.extend(refs.iter().map(String::as_str));
        }
    }
    // Whole-definition granularity: a `data` stays emitted if ANY of its
    // constructors is live (tags are positional indices within the
    // definition, so partial emission would buy little and cost clarity).
    // Dead definitions are kept aside for metadata-only registration.
    let (kept, dropped): (Vec<_>, Vec<_>) = module
        .data_defs
        .drain(..)
        .partition(|d| d.constructors.iter().any(|c| used.contains(c.name.as_str())));
    module.data_defs = kept;
    module.dropped_data_defs.extend(dropped);
    module
}

fn collect_clause(clause: &TClause, refs: &mut HashSet<String>) {
    for p in &clause.patterns {
        collect_pattern(p, refs);
    }
    for g in &clause.guards {
        collect_expr(&g.condition, refs);
        collect_expr(&g.body, refs);
    }
    if let Some(b) = &clause.body { collect_expr(b, refs); }
    for wb in &clause.where_binds {
        for p in &wb.patterns {
            collect_pattern(p, refs);
        }
        collect_expr(&wb.body, refs);
    }
}

/// Collect every constructor name a pattern matches. Function-level DCE never
/// needed patterns (matching a constructor calls no function), but
/// constructor-level DCE must count a match as a use: the matched type's
/// definition has to stay registered, and its constructors may be referenced
/// as values elsewhere in the same program.
fn collect_pattern(p: &TPattern, refs: &mut HashSet<String>) {
    match p {
        TPattern::Constructor { name, args } => {
            refs.insert(name.clone());
            for a in args {
                collect_pattern(a, refs);
            }
        }
        TPattern::Paren(inner) => collect_pattern(inner, refs),
        TPattern::As(_, inner) => collect_pattern(inner, refs),
        TPattern::Tuple(elems) => {
            for e in elems {
                collect_pattern(e, refs);
            }
        }
        TPattern::Var(_, _) | TPattern::Wildcard | TPattern::LitPat(_) => {}
    }
}

/// Collect every name an expression might reference. Over-collection is safe:
/// names that aren't defined functions are filtered out by the caller.
fn collect_expr(e: &TExpr, refs: &mut HashSet<String>) {
    // The names THIS node references (its children are walked below —
    // `for_each_child` is the one enumeration of them).
    match &e.kind {
        TExprKind::Var(n) | TExprKind::Con(n) | TExprKind::OpFunc(n) => {
            refs.insert(n.clone());
        }
        TExprKind::DictAccess { method_name, .. } | TExprKind::DictMethod { method_name, .. } => {
            refs.insert(method_name.clone());
        }
        TExprKind::InfixApp { op, .. } => {
            refs.insert(op.clone());
        }
        TExprKind::Case { branches, .. } => {
            for b in branches {
                collect_pattern(&b.pattern, refs);
            }
        }
        TExprKind::Let { binds, .. } => {
            for b in binds {
                for p in &b.patterns { collect_pattern(p, refs); }
            }
        }
        TExprKind::SpecCall { specialized, .. } => {
            // `original` is NOT a reference: no emitter reads it (emission
            // comes entirely from `specialized`), so it keeps nothing
            // live.  Rooting it used to be invisible — a SpecCall only
            // occurred inside its own wrapper's body, where `original` is
            // that same wrapper — but the fold pass now splices FFI
            // wrapper bodies to call sites, and the spliced SpecCall must
            // not retain the dead wrapper it came from.
            // The variants that thread mata-ll functions keep them live;
            // the FFI kinds carry Lua host names — nothing to keep live.
            match specialized {
                SpecKind::Dict { methods, .. } => {
                    for (_, impl_name) in methods {
                        refs.insert(impl_name.clone());
                    }
                }
                SpecKind::DictCtor { methods, .. } => {
                    for (_, impl_name, _) in methods {
                        refs.insert(impl_name.clone());
                    }
                }
                SpecKind::ListEq(name)
                | SpecKind::MaybeEq(name)
                | SpecKind::ShowList(name)
                | SpecKind::ShowMaybe(name) => {
                    refs.insert(name.clone());
                }
                SpecKind::TupleEq(names) => {
                    for name in names {
                        refs.insert(name.clone());
                    }
                }
                SpecKind::Host(_)
                | SpecKind::Io(_)
                | SpecKind::IoTup { .. }
                | SpecKind::TupRet { .. }
                | SpecKind::Iter(_)
                | SpecKind::Try(_)
                | SpecKind::Pcall(_)
                | SpecKind::IoPcall(_)
                | SpecKind::Const(_)
                | SpecKind::TupGet(_) => {}
            }
        }
        TExprKind::DictCall { func_name, .. } => {
            refs.insert(func_name.clone());
        }
        TExprKind::Lit(_)
        | TExprKind::App(..)
        | TExprKind::Lambda { .. }
        | TExprKind::Negate(_)
        | TExprKind::Paren(_)
        | TExprKind::If { .. }
        | TExprKind::Tuple(_)
        | TExprKind::RecordUpdate { .. }
        | TExprKind::OutgoingCallback { .. }
        | TExprKind::FfiMaybeArg { .. } => {}
    }
    e.for_each_child(&mut |c| collect_expr(c, refs));
}
