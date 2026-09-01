//! Pass 0 — closure-free thunk lifting, run BEFORE every other pass (their
//! engines and stamps then analyze the final shapes; see opt.rs).
//!
//! Every `__thunk(function() … end)` allocates a fresh CLOSURE per
//! execution — under LuaJIT that is the FNEW trace abort, so any loop that
//! suspends work per iteration falls off the trace; on PUC it is a closure
//! plus a table where a table would do. This pass lifts an eligible thunk
//! body to a module-level function created ONCE
//! (`__mll_tkf[k] = function(caps…) … end`, inserted before the top-level
//! statement that contained the site) and rewrites the site to a
//! closure-free thunk carrying the captured VALUES in the table:
//!
//! ```text
//! __thunk(function() return f(x, y) end)
//!     →   __mll_tk2(__mll_tkf[3], x, y)        -- {f, false, 2, x, y}
//! __mll_lazy_cons(h, function() return go(t) end)
//!     →   __mll_lazy_cons(h, __mll_tk1(__mll_tkf[4], t))
//! ```
//!
//! A zero-capture body needs no carrier at all: the shared function itself
//! is the `__thunk` payload (or the lazy-cons generator — both consumers
//! accept a bare callable).
//!
//! CAPTURE SEMANTICS. Lua closures capture VARIABLES (upvalues); the lift
//! captures VALUES at allocation time. The two agree exactly when every
//! captured local is effectively single-assignment — never reassigned, and
//! never forward-declared (`local a; … a = …`, the recursive let shape,
//! whose thunks must see the later assignment). So a candidate lifts only
//! when every free name of its body that resolves to an enclosing FUNCTION
//! scope (chunk-level names resolve at the definition site and are left
//! alone) is bound by a frame that (a) contains no `Raw` statement — a
//! rendered fragment could assign or declare invisibly — and (b) never
//! assigns that name (plain-identifier `Assign`/`MultiAssign`/`AssignIf`
//! targets and initializer-less `local`s, nested literals included; an
//! assignment THROUGH a captured table, `_v[3] = …`, is not an assignment
//! of `_v` — the table's identity is what both capture forms share, so
//! mutations stay visible either way). Tail-loop parameters (`_arg0`,
//! reassigned by the loop update) decline themselves by (b); the
//! per-iteration `_w` copies those loops make for exactly this capture
//! reason are single-assignment and lift fine.
//!
//! The BODY must also be Raw-free (its free names must be trustworthy) and
//! must not assign any of its captures (the value copy would break the
//! write-through). Captures are capped at 3 (`__mll_tk1..3`); richer bodies
//! keep the closure form. Bodies are processed bottom-up, so a nested
//! suspension lifts first and the outer body captures whatever the inner
//! rewrite still references.
//!
//! `__mll_tkf` is one table (a single chunk local) so lifted definitions
//! respect no per-function local budget; each definition is inserted
//! immediately before its originating top-level statement — every chunk
//! local it references is declared by then (it was in scope at the site,
//! which lives INSIDE that statement), and it executes before any code of
//! that statement can allocate the thunk.

use std::collections::HashSet;

use super::annot::is_plain_ident;
use super::hoist::free_names_func;
use super::lua::{Block, Expr, FnTarget, FuncBody, Stmt, TKF_TABLE};

/// One enclosing function scope on the walk path.
struct Frame {
    /// Names bound so far on the path to the current position (params plus
    /// the `local`s already walked in enclosing blocks of this function).
    bound: HashSet<String>,
    /// Names this function's body (nested literals included) ever assigns
    /// or forward-declares — capturing one by value would diverge from the
    /// closure's upvalue view.
    assigned: HashSet<String>,
    /// The function contains a Raw STATEMENT: its rendered text could
    /// declare or assign locals invisibly, so nothing bound here lifts.
    has_raw: bool,
}

struct Lift {
    defs: Vec<Stmt>,
    counter: u32,
}

pub(super) fn run(stmts: &mut Vec<Stmt>) {
    let mut st = Lift { defs: Vec::new(), counter: 0 };
    let mut i = 0;
    let mut any = false;
    while i < stmts.len() {
        let mut frames: Vec<Frame> = Vec::new();
        walk_stmt(&mut stmts[i], &mut st, &mut frames);
        let n = st.defs.len();
        if n > 0 {
            any = true;
            for (j, d) in st.defs.drain(..).enumerate() {
                stmts.insert(i + j, d);
            }
        }
        i += n + 1;
    }
    if any {
        stmts.insert(
            0,
            Stmt::Local(vec![TKF_TABLE.to_string()], Some(Expr::Table(vec![]))),
        );
    }
}

/// Whole-body prescan for a function frame: every name it can assign, and
/// whether any Raw statement hides part of the answer.
fn prescan(stmts: &[Stmt], assigned: &mut HashSet<String>, has_raw: &mut bool) {
    for s in stmts {
        match s {
            Stmt::Raw(_) => *has_raw = true,
            Stmt::Local(names, None) => assigned.extend(names.iter().cloned()),
            Stmt::Assign(lhs, _) => {
                if is_plain_ident(lhs) {
                    assigned.insert(lhs.clone());
                }
            }
            Stmt::AssignIf { lhs, .. } => {
                if is_plain_ident(lhs) {
                    assigned.insert(lhs.clone());
                }
            }
            Stmt::MultiAssign(lhs, _) => {
                for l in lhs {
                    if is_plain_ident(l) {
                        assigned.insert(l.clone());
                    }
                }
            }
            _ => {}
        }
        s.for_each_block(&mut |b| prescan(b, assigned, has_raw));
        // for_each_block covers statement-level bodies (Function, If, Do,
        // WhileTrue); function LITERALS live inside expressions.
        let mut scan_expr = |e: &Expr| expr_prescan(e, assigned, has_raw);
        match s {
            Stmt::Local(_, Some(e)) | Stmt::Assign(_, e) | Stmt::Return(e) | Stmt::Expr(e) => {
                scan_expr(e)
            }
            Stmt::AssignIf { cond, then_e, else_e, .. } => {
                scan_expr(cond);
                scan_expr(then_e);
                scan_expr(else_e);
            }
            Stmt::MultiAssign(_, es) => {
                for e in es {
                    scan_expr(e);
                }
            }
            Stmt::If { cond, .. } => scan_expr(cond),
            Stmt::ReturnTable(entries) => {
                for (_, e) in entries {
                    scan_expr(e);
                }
            }
            _ => {}
        }
    }
}

fn expr_prescan(e: &Expr, assigned: &mut HashSet<String>, has_raw: &mut bool) {
    if let Expr::Func(_, fb) = e {
        prescan(fb.stmts(), assigned, has_raw);
        return;
    }
    e.for_each_subexpr(&mut |c| expr_prescan(c, assigned, has_raw));
}

fn push_frame(params: &[String], body: &[Stmt], frames: &mut Vec<Frame>) {
    let mut assigned = HashSet::new();
    let mut has_raw = false;
    prescan(body, &mut assigned, &mut has_raw);
    frames.push(Frame {
        bound: params.iter().cloned().collect(),
        assigned,
        has_raw,
    });
}

fn walk_block(stmts: &mut Vec<Stmt>, st: &mut Lift, frames: &mut Vec<Frame>) {
    let save = frames.last().map(|f| f.bound.clone());
    for s in stmts {
        walk_stmt(s, st, frames);
    }
    if let (Some(b), Some(f)) = (save, frames.last_mut()) {
        f.bound = b;
    }
}

fn walk_stmt(s: &mut Stmt, st: &mut Lift, frames: &mut Vec<Frame>) {
    match s {
        Stmt::Local(names, rhs) => {
            if let Some(e) = rhs {
                walk_expr(e, st, frames);
            }
            if let Some(f) = frames.last_mut() {
                f.bound.extend(names.iter().cloned());
            }
        }
        Stmt::Function { target, params, body } => {
            if let (FnTarget::LocalFn(n), Some(f)) = (&*target, frames.last_mut()) {
                f.bound.insert(n.clone());
            }
            push_frame(params, &body.0, frames);
            walk_block(&mut body.0, st, frames);
            frames.pop();
        }
        Stmt::If { cond, then_b, elseifs, else_b } => {
            walk_expr(cond, st, frames);
            walk_block(&mut then_b.0, st, frames);
            for (c, b) in elseifs.iter_mut() {
                walk_expr(c, st, frames);
                walk_block(&mut b.0, st, frames);
            }
            if let Some(b) = else_b {
                walk_block(&mut b.0, st, frames);
            }
        }
        Stmt::Do(b) | Stmt::WhileTrue(b) => walk_block(&mut b.0, st, frames),
        other => other.for_each_expr_mut(&mut |e| walk_expr(e, st, frames)),
    }
}

fn walk_expr(e: &mut Expr, st: &mut Lift, frames: &mut Vec<Frame>) {
    // The two suspension sites. Their literal bodies are walked FIRST
    // (bottom-up: inner suspensions lift before the outer body's free
    // names are read), then the whole literal may lift.
    if let Expr::Call(f, args) = e {
        let callee = matches!(f.as_ref(), Expr::Name(n) if n == "__thunk").then_some(0)
            .or_else(|| {
                matches!(f.as_ref(), Expr::Name(n) if n == "__mll_lazy_cons").then_some(1)
            });
        if let Some(slot) = callee
            && args.len() == slot + 1
            && matches!(&args[slot], Expr::Func(p, _) if p.is_empty())
        {
            for (i, a) in args.iter_mut().enumerate() {
                if i != slot {
                    walk_expr(a, st, frames);
                }
            }
            let Expr::Func(_, fb) = &mut args[slot] else { unreachable!() };
            push_frame(&[], fb.stmts(), frames);
            walk_block(fb.stmts_mut(), st, frames);
            frames.pop();
            if let Some((fslot, caps)) = try_lift(fb, st, frames) {
                if caps.is_empty() {
                    // The shared function is itself a valid `__thunk`
                    // payload and a valid lazy-cons generator.
                    args[slot] = Expr::Name(fslot);
                } else if slot == 0 {
                    // A carried lift at a `__thunk` site IS the thunk —
                    // replace the whole wrapper call, not its argument
                    // (`__thunk(__mll_tk2(…))` would nest two thunks).
                    *e = carrier("__mll_tk", fslot, caps);
                } else {
                    // A struct generator: the carrier needs no metatable
                    // (the __mll_gen* family — a plain table, no
                    // setmetatable call per produced cell), and the cell
                    // marks the flavor in its __lazy flag — the
                    // __mll_lazy_consg constructor stores `1` where the
                    // closure form stores `true`, so the tail readers
                    // dispatch off the flag without a type() call.
                    args[slot] = carrier("__mll_gen", fslot, caps);
                    *f = Box::new(Expr::Name("__mll_lazy_consg".to_string()));
                }
            }
            return;
        }
    }
    if let Expr::Func(params, fb) = e {
        push_frame(params, fb.stmts(), frames);
        walk_block(fb.stmts_mut(), st, frames);
        frames.pop();
        return;
    }
    e.for_each_subexpr_mut(&mut |c| walk_expr(c, st, frames));
}

/// `__mll_tk2(__mll_tkf[k], c1, c2)` / `__mll_gen1(__mll_tkf[k], c1)` —
/// the carrier allocation for a lifted body with captures.
fn carrier(family: &str, fslot: String, caps: Vec<String>) -> Expr {
    let mut cargs = vec![Expr::Name(fslot)];
    cargs.extend(caps.into_iter().map(Expr::Name));
    Expr::call_named(&format!("{family}{}", cargs.len() - 1), cargs)
}

/// Attempt to lift one zero-parameter literal body (already walked). On
/// success the definition statement is queued; returns the `__mll_tkf[k]`
/// reference and the capture list (the caller picks the carrier family).
fn try_lift(fb: &mut FuncBody, st: &mut Lift, frames: &[Frame]) -> Option<(String, Vec<String>)> {
    if frames.is_empty() {
        // Chunk level: the site runs once at load — nothing to save.
        return None;
    }
    if body_has_raw(fb.stmts()) {
        return None;
    }
    let free = free_names_func(&[], fb.stmts());
    let mut captures: Vec<String> = Vec::new();
    for name in &free {
        // Innermost binding frame decides; a name no frame binds is
        // chunk-level or global and resolves at the definition site.
        if let Some(frame) = frames.iter().rev().find(|f| f.bound.contains(name)) {
            if frame.has_raw || frame.assigned.contains(name) {
                return None;
            }
            captures.push(name.clone());
        }
    }
    if captures.len() > 3 {
        return None;
    }
    captures.sort();
    // The body must not assign a capture (the lifted parameter is a copy).
    {
        let mut body_assigned = HashSet::new();
        let mut body_raw = false;
        prescan(fb.stmts(), &mut body_assigned, &mut body_raw);
        if body_raw || captures.iter().any(|c| body_assigned.contains(c)) {
            return None;
        }
    }
    let k = st.counter;
    st.counter += 1;
    let slot = format!("{TKF_TABLE}[{k}]");
    let body_stmts = std::mem::take(fb.stmts_mut());
    st.defs.push(Stmt::Function {
        target: FnTarget::ThunkSlot(k),
        params: captures.clone(),
        body: Block(body_stmts),
    });
    Some((slot, captures))
}

fn body_has_raw(stmts: &[Stmt]) -> bool {
    let mut hit = false;
    fn scan_expr(e: &Expr, hit: &mut bool) {
        match e {
            Expr::Raw(_) => *hit = true,
            Expr::Func(_, fb) => scan_stmts(fb.stmts(), hit),
            _ => e.for_each_subexpr(&mut |c| scan_expr(c, hit)),
        }
    }
    fn scan_stmts(stmts: &[Stmt], hit: &mut bool) {
        for s in stmts {
            if matches!(s, Stmt::Raw(_)) {
                *hit = true;
                return;
            }
            s.for_each_block(&mut |b| scan_stmts(b, hit));
            s.for_each_expr(&mut |e| scan_expr(e, hit));
        }
    }
    scan_stmts(stmts, &mut hit);
    hit
}
