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
//! SpecCall/DictCall/DictAccess/operators). By construction no *kept* function
//! can reference a *dropped* one, so the emitted code stays well-formed; the
//! integration suite (which runs every output) is the backstop.

use std::collections::{HashMap, HashSet};
use crate::tir::*;

/// Drop functions and instance methods not reachable from `main`/exports.
pub fn eliminate(mut module: TModule) -> TModule {
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
    module
}

fn collect_clause(clause: &TClause, refs: &mut HashSet<String>) {
    for g in &clause.guards {
        collect_expr(&g.condition, refs);
        collect_expr(&g.body, refs);
    }
    collect_expr(&clause.body, refs);
    for wb in &clause.where_binds {
        collect_expr(&wb.body, refs);
    }
}

/// Collect every name an expression might reference. Over-collection is safe:
/// names that aren't defined functions are filtered out by the caller.
fn collect_expr(e: &TExpr, refs: &mut HashSet<String>) {
    match &e.kind {
        TExprKind::Var(n) | TExprKind::Con(n) | TExprKind::OpFunc(n) => {
            refs.insert(n.clone());
        }
        TExprKind::Lit(_) => {}
        TExprKind::DictAccess { method_name, .. } => {
            refs.insert(method_name.clone());
        }
        TExprKind::App(a, b) => { collect_expr(a, refs); collect_expr(b, refs); }
        TExprKind::Lambda { body, .. } => collect_expr(body, refs),
        TExprKind::InfixApp { op, lhs, rhs } => {
            refs.insert(op.clone());
            collect_expr(lhs, refs);
            collect_expr(rhs, refs);
        }
        TExprKind::Negate(x) | TExprKind::Paren(x) => collect_expr(x, refs),
        TExprKind::If { cond, then_branch, else_branch } => {
            collect_expr(cond, refs);
            collect_expr(then_branch, refs);
            collect_expr(else_branch, refs);
        }
        TExprKind::Case { scrutinee, branches } => {
            collect_expr(scrutinee, refs);
            for b in branches {
                for g in &b.guards { collect_expr(&g.condition, refs); collect_expr(&g.body, refs); }
                collect_expr(&b.body, refs);
            }
        }
        TExprKind::Let { binds, body } => {
            for b in binds { collect_expr(&b.body, refs); }
            collect_expr(body, refs);
        }
        TExprKind::SpecCall { original, specialized, args } => {
            refs.insert(original.clone());
            // `specialized` threads element functions inside a `helper:…`
            // string. Segments come in three shapes:
            //   "__mll_list_eq:eq_State"            — one function per segment
            //   "__mll_tuple_eq:2:eq_Foo,eq_Bar"    — comma-joined function list
            //   "__mll_dict:Show:show=show_Foo,…"   — method=impl pairs
            // Capture every embedded function name so the derived show/eq
            // implementations they reference stay live.
            for seg in specialized.split(':') {
                for part in seg.split(',') {
                    let name = part.rsplit('=').next().unwrap_or(part);
                    refs.insert(name.to_string());
                }
            }
            for a in args { collect_expr(a, refs); }
        }
        TExprKind::Tuple(elems) => { for x in elems { collect_expr(x, refs); } }
        TExprKind::DictCall { func_name, dict_args, value_args } => {
            refs.insert(func_name.clone());
            for a in dict_args { collect_expr(a, refs); }
            for a in value_args { collect_expr(a, refs); }
        }
        TExprKind::RecordUpdate { record, updates, .. } => {
            collect_expr(record, refs);
            for (_, _, val) in updates { collect_expr(val, refs); }
        }
        TExprKind::OutgoingCallback { callee, .. } => collect_expr(callee, refs),
    }
}
