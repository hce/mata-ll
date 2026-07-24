//! Expression-splitting pass: bound the syntactic nesting depth of emitted Lua.
//!
//! Lua's own parser has a hard recursion limit on how deeply expressions may
//! nest (`LUAI_MAXCCALLS`, ~200 in Lua 5.4; LuaJIT reports "chunk has too many
//! syntax levels"). mata-ll emits a whole function/CAF body as a single, often
//! deeply-nested Lua expression, so a valid mata-ll program with a long chain
//! (e.g. `a + b + c + ... ` over ~190 operands, a `<>` string-concat chain, or
//! a long `x : x : ... : []` cons chain) compiles to Lua that Lua itself
//! refuses to *load* — a `C stack overflow` before the program even runs.
//!
//! This pass runs on the monomorphized, constant-folded TIR just before codegen.
//! Wherever a pure sub-expression's nesting depth exceeds a safe threshold, it
//! pulls that sub-expression out into a `let` binding and references it by name.
//! Codegen already lowers a `Let` to an IIFE whose bindings are *sibling*
//! `local` statements, so each extracted piece is parsed at shallow depth and
//! the overall nesting stays well under Lua's limit.
//!
//! Correctness rests on referential transparency plus the existing `Let`
//! lowering:
//!   * Only *pure*, non-function, non-effectful sub-expressions are extracted
//!     (never IO/LuaIO/ST actions, lambdas, or partial applications), so sharing
//!     and ordering are irrelevant.
//!   * Each extracted expression is referenced exactly once, so no work is
//!     duplicated or newly shared.
//!   * `Let` bindings are thunked (lazy) by codegen, so an extracted binding is
//!     forced at exactly the point its single reference is forced — identical to
//!     leaving it inline. Short-circuit operators (`&&`/`||`) keep their
//!     semantics because the reference sits inside the same Lua `and`/`or`.
//!   * Extraction never crosses a binder: lambda bodies, `case` branches and
//!     `let` bodies each get their own fresh `Let` scope, so an extracted piece
//!     can only reference names already in scope where it is placed.
//!
//! For expressions below the threshold nothing changes, so existing (byte-exact,
//! determinism-guarded) output is unaffected.

use crate::tir::*;
use crate::types::Ty;

/// Maximum expression nesting depth left inline. Anything deeper is split out.
///
/// This counts TIR nodes, but a single node can lower to several nested Lua
/// syntax levels (e.g. `a <> b` becomes `(a .. __force((b ..  ...)))` — a
/// binop, a call and a paren per operand). Lua's parser caps nesting near ~200,
/// so we keep the TIR threshold low enough that even a ~3x lowering multiplier
/// plus the fixed codegen wrappers around a body stays comfortably under it.
/// It is still far above any hand-written expression, so ordinary programs are
/// emitted unchanged.
const MAX_DEPTH: usize = 40;

/// Rewrite every function/CAF body so no emitted Lua expression nests beyond
/// `MAX_DEPTH`.
pub fn split_module(mut module: TModule) -> TModule {
    for f in module.functions.iter_mut() {
        split_function(f);
    }
    for f in module.instance_fns.iter_mut() {
        split_function(f);
    }
    module
}

fn split_function(f: &mut TFunction) {
    for clause in f.clauses.iter_mut() {
        let mut ctr = 0usize;
        // where-bindings are in scope for the body and guards; flatten each in
        // its own scope so extracted temps stay local to that binding.
        for wb in clause.where_binds.iter_mut() {
            let body = std::mem::replace(&mut wb.body, dummy());
            wb.body = flatten_scope(body, &mut ctr);
        }
        for g in clause.guards.iter_mut() {
            let cond = std::mem::replace(&mut g.condition, dummy());
            g.condition = flatten_scope(cond, &mut ctr);
            let body = std::mem::replace(&mut g.body, dummy());
            g.body = flatten_scope(body, &mut ctr);
        }
        let body = std::mem::replace(&mut clause.body, dummy());
        clause.body = flatten_scope(body, &mut ctr);
    }
}

fn dummy() -> TExpr {
    TExpr::new(TExprKind::Lit(TLiteral::Unit), Ty::Unit)
}

/// Flatten an expression that begins a fresh binding scope (a whole body, a
/// branch, a lambda body, ...). Extracted bindings are collected here and, if
/// any, wrapped around the result in a single `Let` (sibling bindings).
fn flatten_scope(e: TExpr, ctr: &mut usize) -> TExpr {
    let ty = e.ty.clone();
    let mut binds: Vec<TLocalDef> = Vec::new();
    let inner = flatten_expr(e, &mut binds, ctr);
    if binds.is_empty() {
        inner
    } else {
        TExpr::new(
            TExprKind::Let { binds, body: Box::new(inner) },
            ty,
        )
    }
}

/// Rebuild `e`, hoisting any deep pure child into `binds` (post-order, so a
/// binding that references an earlier temp appears after it).
fn flatten_expr(e: TExpr, binds: &mut Vec<TLocalDef>, ctr: &mut usize) -> TExpr {
    let ty = e.ty.clone();
    let kind = match e.kind {
        // Leaves.
        k @ (TExprKind::Var(_)
        | TExprKind::Con(_)
        | TExprKind::Lit(_)
        | TExprKind::OpFunc(_)
        | TExprKind::DictAccess { .. }) => k,

        TExprKind::Paren(inner) => {
            TExprKind::Paren(Box::new(flatten_expr(*inner, binds, ctr)))
        }
        // A constructed dictionary's method: recurse into the dictionary
        // expression but never hoist it (it carries dictionary structure).
        TExprKind::DictMethod { dict, method_name } => TExprKind::DictMethod {
            dict: Box::new(flatten_expr(*dict, binds, ctr)),
            method_name,
        },
        TExprKind::Negate(inner) => {
            // Negation forces its operand — a strict position.
            TExprKind::Negate(Box::new(hoist(*inner, true, binds, ctr)))
        }
        TExprKind::App(f, a) => {
            // Recurse into the callee spine (so deep args nested inside it are
            // handled) but never hoist the callee itself: codegen pattern-matches
            // application spines (cons, seq, method inlining, ...) and a bare
            // Var head would defeat that. Arguments are a non-strict hoist point
            // (a callee may not force them), so an argument is only pulled out
            // when doing so keeps it lazy (see `hoist`).
            let f = flatten_expr(*f, binds, ctr);
            let a = hoist(*a, false, binds, ctr);
            TExprKind::App(Box::new(f), Box::new(a))
        }
        TExprKind::InfixApp { op, lhs, rhs } => {
            if is_control_op(&op) {
                // IO bind / sequencing / `$` / composition: leave the shape
                // intact (their operands carry effect/closure structure), just
                // recurse so nested pure sub-expressions inside are still split.
                let lhs = flatten_expr(*lhs, binds, ctr);
                let rhs = flatten_expr(*rhs, binds, ctr);
                TExprKind::InfixApp { op, lhs: Box::new(lhs), rhs: Box::new(rhs) }
            } else {
                // Per-operand strictness decides whether an operand may be pulled
                // out *eagerly*. A strict operand is forced by the operator
                // anyway, so naming it changes nothing. A non-strict operand
                // (short-circuited `&&`/`||` rhs, a lazy list `<>`/`++`/`:` tail)
                // must stay lazy — `hoist` only extracts it if the binding would
                // be thunked, never eagerly.
                let (ls, rs) = operand_strictness(&op, &lhs.ty, &rhs.ty);
                let lhs = hoist(*lhs, ls, binds, ctr);
                let rhs = hoist(*rhs, rs, binds, ctr);
                TExprKind::InfixApp { op, lhs: Box::new(lhs), rhs: Box::new(rhs) }
            }
        }
        TExprKind::If { cond, then_branch, else_branch } => {
            // cond is always evaluated: hoist into the current scope. The
            // branches are conditional and each get their own scope.
            let cond = hoist(*cond, true, binds, ctr);
            let then_branch = flatten_scope(*then_branch, ctr);
            let else_branch = flatten_scope(*else_branch, ctr);
            TExprKind::If {
                cond: Box::new(cond),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            }
        }
        TExprKind::Case { scrutinee, branches } => {
            // The scrutinee is always evaluated (matched), so it is strict.
            let scrutinee = hoist(*scrutinee, true, binds, ctr);
            let branches = branches
                .into_iter()
                .map(|mut b| {
                    // A branch binds pattern variables; keep its temps inside it.
                    let mut inner_ctr = *ctr;
                    for g in b.guards.iter_mut() {
                        let c = std::mem::replace(&mut g.condition, dummy());
                        g.condition = flatten_scope(c, &mut inner_ctr);
                        let bd = std::mem::replace(&mut g.body, dummy());
                        g.body = flatten_scope(bd, &mut inner_ctr);
                    }
                    let body = std::mem::replace(&mut b.body, dummy());
                    b.body = flatten_scope(body, &mut inner_ctr);
                    *ctr = inner_ctr;
                    b
                })
                .collect();
            TExprKind::Case { scrutinee: Box::new(scrutinee), branches }
        }
        TExprKind::Lambda { params, body } => {
            // Cannot hoist across the parameter binder: fresh scope for the body.
            TExprKind::Lambda { params, body: Box::new(flatten_scope(*body, ctr)) }
        }
        TExprKind::Let { binds: mut lbinds, body } => {
            for b in lbinds.iter_mut() {
                let bd = std::mem::replace(&mut b.body, dummy());
                b.body = flatten_scope(bd, ctr);
            }
            TExprKind::Let { binds: lbinds, body: Box::new(flatten_scope(*body, ctr)) }
        }
        TExprKind::SpecCall { original, specialized, args } => {
            // Call arguments are non-strict positions (the callee decides).
            let args = args.into_iter().map(|a| hoist(a, false, binds, ctr)).collect();
            TExprKind::SpecCall { original, specialized, args }
        }
        TExprKind::Tuple(elems) => {
            // Tuple fields are stored lazily.
            let elems = elems.into_iter().map(|a| hoist(a, false, binds, ctr)).collect();
            TExprKind::Tuple(elems)
        }
        TExprKind::DictCall { func_name, dict_args, value_args } => {
            let dict_args = dict_args.into_iter().map(|a| flatten_expr(a, binds, ctr)).collect();
            let value_args = value_args.into_iter().map(|a| hoist(a, false, binds, ctr)).collect();
            TExprKind::DictCall { func_name, dict_args, value_args }
        }
        TExprKind::RecordUpdate { record, updates, num_fields } => {
            // Record and its new field values are stored lazily.
            let record = hoist(*record, false, binds, ctr);
            let updates = updates
                .into_iter()
                .map(|(n, i, v)| (n, i, hoist(v, false, binds, ctr)))
                .collect();
            TExprKind::RecordUpdate { record: Box::new(record), updates, num_fields }
        }
        // A callback lowered out to Lua: recurse into the callee in its own
        // scope but do not hoist it (it carries closure/marshalling structure).
        TExprKind::OutgoingCallback {
            callee, arity, run_io,
        } => TExprKind::OutgoingCallback {
            callee: Box::new(flatten_scope(*callee, ctr)),
            arity, run_io,
        },
        // An optional FFI argument: recurse but never hoist the wrapper itself
        // — it must stay directly inside its SpecCall argument list.
        TExprKind::FfiMaybeArg { value } => TExprKind::FfiMaybeArg {
            value: Box::new(flatten_scope(*value, ctr)),
        },
    };
    TExpr::new(kind, ty)
}

/// Flatten a child expression and, if it is still deep and safe to name, pull it
/// into a fresh `let` binding, returning a reference to it.
///
/// `strict` says whether the surrounding position forces the child. Extraction
/// is only sound when it cannot change laziness:
///   * a strict position forces the child anyway, so evaluating the binding
///     eagerly is identical to inlining it; or
///   * the binding will be *thunked* by codegen (`!is_cheap`), so it stays lazy
///     and is forced exactly at its single reference — again identical.
///
/// A non-strict position whose binding would be emitted eagerly (`is_cheap`) is
/// left inline, so short-circuited `&&`/`||` operands and other lazy positions
/// keep their non-strict semantics.
fn hoist(child: TExpr, strict: bool, binds: &mut Vec<TLocalDef>, ctr: &mut usize) -> TExpr {
    let child = flatten_expr(child, binds, ctr);
    if depth(&child) > MAX_DEPTH && is_extractable(&child) && (strict || !is_cheap(&child)) {
        let name = format!("__split{}", *ctr);
        *ctr += 1;
        let ty = child.ty.clone();
        binds.push(TLocalDef { name: name.clone(), patterns: Vec::new(), body: child });
        TExpr::new(TExprKind::Var(name), ty)
    } else {
        child
    }
}

/// Control/effect operators whose operand shape codegen depends on.
fn is_control_op(op: &str) -> bool {
    matches!(op, ">>=" | ">>" | "$" | ".")
}

/// Per-operand strictness of a (non-control) infix operator: whether codegen
/// forces that operand. Mirrors codegen's lowering.
fn operand_strictness(op: &str, lhs_ty: &Ty, rhs_ty: &Ty) -> (bool, bool) {
    match op {
        // Arithmetic and comparison force both operands.
        "+" | "-" | "*" | "/" | "%" | "^" | "div" | "mod" | "==" | "/=" | "~="
        | "<" | ">" | "<=" | ">=" => (true, true),
        // `<>`/`++` are strict only on strings/bytestrings (Lua `..`); on lists
        // they build a lazy-tailed append, so the operands stay non-strict.
        "<>" | "++" => (is_string_type(lhs_ty), is_string_type(rhs_ty)),
        // `&&`/`||` short-circuit their right operand; `:` keeps both lazy.
        // Treat all as non-strict — they are only hoisted when kept lazy.
        _ => (false, false),
    }
}

/// String-like types compile to Lua strings and are combined with strict `..`.
fn is_string_type(ty: &Ty) -> bool {
    matches!(ty, Ty::Con(n) if n == "String" || n == "ByteString")
}

/// Whether codegen would emit this expression *eagerly* (bare + concrete)
/// rather than as a memoizing thunk. Mirrors `codegen::Codegen::is_cheap` — a
/// non-strict position may only be hoisted when this is false (stays lazy).
fn is_cheap(e: &TExpr) -> bool {
    match &e.kind {
        TExprKind::Lit(_)
        | TExprKind::Con(_)
        | TExprKind::Var(_)
        | TExprKind::Lambda { .. }
        | TExprKind::OpFunc(_) => true,
        TExprKind::Paren(inner) | TExprKind::Negate(inner) => is_cheap(inner),
        TExprKind::Tuple(elems) => elems.iter().all(is_cheap),
        TExprKind::InfixApp { op, lhs, rhs } => {
            is_builtin_op(op) && is_cheap(lhs) && is_cheap(rhs)
        }
        TExprKind::App(func, arg) => {
            if is_con_app(e) {
                is_cheap(arg) && is_cheap(func)
            } else {
                false
            }
        }
        TExprKind::If { cond, then_branch, else_branch } => {
            is_cheap(cond) && is_cheap(then_branch) && is_cheap(else_branch)
        }
        _ => false,
    }
}

/// Mirror of `codegen::is_builtin_op`.
fn is_builtin_op(op: &str) -> bool {
    matches!(op,
        "+" | "-" | "*" | "/" | "%" | "^" | "==" | "/=" | "~="
        | "<" | ">" | "<=" | ">=" | "++" | "<>" | "&&" | "||" | ".." | "$" | "."
        | "div" | "mod")
}

/// Mirror of `codegen::Codegen::is_con_app`.
fn is_con_app(e: &TExpr) -> bool {
    match &e.kind {
        TExprKind::Con(_) => true,
        TExprKind::App(func, _) => is_con_app(func),
        _ => false,
    }
}

/// Whether an expression may be named by a `let` binding without changing
/// semantics or breaking a codegen shape assumption. Only pure values qualify.
fn is_extractable(e: &TExpr) -> bool {
    // Naming a bare reference or literal is pointless (and Vars/Cons may be part
    // of an application spine handled specially by codegen).
    if matches!(
        e.kind,
        TExprKind::Var(_) | TExprKind::Con(_) | TExprKind::Lit(_) | TExprKind::OpFunc(_)
    ) {
        return false;
    }
    is_value_type(&e.ty)
}

/// Pure, first-order value types — never functions, effects (IO/LuaIO/ST) or
/// unresolved type variables (which could stand for any of those).
fn is_value_type(ty: &Ty) -> bool {
    match ty {
        Ty::Con(_) | Ty::List(_) | Ty::Tuple(_) | Ty::Unit | Ty::Promoted(_) => true,
        Ty::App(_, _) => !is_st_type(ty),
        Ty::Arrow(..)
        | Ty::IO(_)
        | Ty::LuaIO(_, _)
        | Ty::Forall(_, _)
        | Ty::Skolem(_, _)
        | Ty::Var(_) => false,
    }
}

/// `ST s a` — an effectful (mutable) action, threaded as a closure by codegen.
fn is_st_type(ty: &Ty) -> bool {
    match ty {
        Ty::App(f, _) => match f.as_ref() {
            Ty::App(c, _) => matches!(c.as_ref(), Ty::Con(n) if n == "ST"),
            _ => false,
        },
        _ => false,
    }
}

/// Approximate the syntactic nesting depth codegen will emit for `e`. Monotone
/// in the real depth, which is all the threshold needs.
fn depth(e: &TExpr) -> usize {
    match &e.kind {
        TExprKind::Var(_)
        | TExprKind::Con(_)
        | TExprKind::Lit(_)
        | TExprKind::OpFunc(_)
        | TExprKind::DictAccess { .. } => 1,
        TExprKind::DictMethod { dict, .. } => 1 + depth(dict),
        TExprKind::Paren(inner) | TExprKind::Negate(inner) => 1 + depth(inner),
        TExprKind::App(f, a) => 1 + depth(f).max(depth(a)),
        TExprKind::InfixApp { lhs, rhs, .. } => 1 + depth(lhs).max(depth(rhs)),
        TExprKind::If { cond, then_branch, else_branch } => {
            1 + depth(cond).max(depth(then_branch)).max(depth(else_branch))
        }
        TExprKind::Case { scrutinee, branches } => {
            let mut d = depth(scrutinee);
            for b in branches {
                d = d.max(depth(&b.body));
                for g in &b.guards {
                    d = d.max(depth(&g.condition)).max(depth(&g.body));
                }
            }
            1 + d
        }
        TExprKind::Let { binds, body } => {
            let mut d = depth(body);
            for b in binds {
                d = d.max(depth(&b.body));
            }
            1 + d
        }
        TExprKind::Lambda { body, .. } => 1 + depth(body),
        TExprKind::SpecCall { args, .. } => {
            1 + args.iter().map(depth).max().unwrap_or(0)
        }
        TExprKind::Tuple(elems) => 1 + elems.iter().map(depth).max().unwrap_or(0),
        TExprKind::DictCall { dict_args, value_args, .. } => {
            1 + dict_args
                .iter()
                .chain(value_args.iter())
                .map(depth)
                .max()
                .unwrap_or(0)
        }
        TExprKind::RecordUpdate { record, updates, .. } => {
            let mut d = depth(record);
            for (_, _, v) in updates {
                d = d.max(depth(v));
            }
            1 + d
        }
        TExprKind::OutgoingCallback { callee, .. } => 1 + depth(callee),
        TExprKind::FfiMaybeArg { value } => 1 + depth(value),
    }
}
