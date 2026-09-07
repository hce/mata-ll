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
//! captured local's value is FINAL at the allocation. So a candidate lifts
//! only when every free name of its body that resolves to an enclosing
//! FUNCTION scope (chunk-level names resolve at the definition site and are
//! left alone) is bound by a frame that (a) contains no `Raw` statement — a
//! rendered fragment could assign or declare invisibly — and (b) either
//! never assigns that name (plain-identifier `Assign`/`MultiAssign`/
//! `AssignIf` targets and initializer-less `local`s, nested literals
//! included; an assignment THROUGH a captured table, `_v[3] = …`, is not
//! an assignment of `_v` — the table's identity is what both capture forms
//! share, so mutations stay visible either way), or has SETTLED it: the
//! name is declared by a `local` of the block being walked (`local a` —
//! the multi-binding let shape, `local a; local b; a = …; b = __thunk(… a
//! …)` — or `local x = …`), and EVERY assignment the frame makes to it
//! has been passed by the walk: each sits in a statement of that same
//! block that precedes the site, at that level or inside a branch, `do`
//! or loop body of such a statement. A block's statements run in order
//! once per execution of the block and a block-scoped `local` is a fresh
//! variable per execution, so once the last assignment is behind the
//! site the value it sees is the variable's final value — exactly what
//! the closure would read. The inline ST read is the branch case:
//! `local x = arr[i]; if <thunk> then x = __mll_st_settle(…) end`, then
//! a suspension over `x`. An assignment inside a function body — a
//! statement-level definition or a literal — runs at some later time or
//! never, so it is never passed and the name never settles; one in a
//! nested block that still lies AHEAD of the site (or one the walk is
//! still inside, a site within the loop body that assigns the name)
//! leaves the count short, so the name stays unsettled there. Tail-loop
//! parameters (`_arg0`, reassigned by the loop update) decline
//! themselves by (b); the per-iteration `_w` copies those loops make for
//! exactly this capture reason are single-assignment and lift fine.
//!
//! The BODY must also be Raw-free (its free names must be trustworthy) and
//! must not assign any of its captures (the value copy would break the
//! write-through). Captures are capped at 4 for a `__thunk` site
//! (`__mll_tk1..4`) and 3 for a lazy-cons generator (`__mll_gen1..3`, the
//! tail evaluator's flat call); richer bodies keep the closure form.
//! Bodies are processed bottom-up, so a nested suspension lifts first and
//! the outer body captures whatever the inner rewrite still references.
//!
//! `__mll_tkf` is one table (a single chunk local) so lifted definitions
//! respect no per-function local budget; each definition is inserted
//! immediately before its originating top-level statement — every chunk
//! local it references is declared by then (it was in scope at the site,
//! which lives INSIDE that statement), and it executes before any code of
//! that statement can allocate the thunk.

use std::collections::{HashMap, HashSet};

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
    /// closure's upvalue view, unless the walk has settled the name.
    assigned: HashSet<String>,
    /// How many plain-identifier assignments the body (nested literals
    /// included) makes to each name; a forward-declared name assigned
    /// exactly once can settle (see the module doc).
    assign_counts: HashMap<String, usize>,
    /// Names declared by a `local` of a walked block whose every
    /// assignment the walk has passed within that block: their value is
    /// final at every later site of that block.
    settled: HashSet<String>,
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

/// Whole-body prescan for a function frame: every name it can assign (and
/// how often), and whether any Raw statement hides part of the answer.
fn prescan(
    stmts: &[Stmt],
    assigned: &mut HashSet<String>,
    counts: &mut HashMap<String, usize>,
    has_raw: &mut bool,
) {
    let note = |name: &String, assigned: &mut HashSet<String>, counts: &mut HashMap<String, usize>| {
        assigned.insert(name.clone());
        *counts.entry(name.clone()).or_insert(0) += 1;
    };
    for s in stmts {
        match s {
            Stmt::Raw(_) => *has_raw = true,
            // A forward declaration is not an assignment (it does not
            // count toward the single-assignment settling rule), but the
            // name is `assigned` in the sense that matters: its value is
            // not final at declaration.
            Stmt::Local(names, None) => assigned.extend(names.iter().cloned()),
            Stmt::Assign(lhs, _) => {
                if is_plain_ident(lhs) {
                    note(lhs, assigned, counts);
                }
            }
            Stmt::AssignIf { lhs, .. } => {
                if is_plain_ident(lhs) {
                    note(lhs, assigned, counts);
                }
            }
            Stmt::MultiAssign(lhs, _) => {
                for l in lhs {
                    if is_plain_ident(l) {
                        note(l, assigned, counts);
                    }
                }
            }
            _ => {}
        }
        s.for_each_block(&mut |b| prescan(b, assigned, counts, has_raw));
        // for_each_block covers statement-level bodies (Function, If, Do,
        // WhileTrue); function LITERALS live inside expressions.
        let mut scan_expr = |e: &Expr| expr_prescan(e, assigned, counts, has_raw);
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

fn expr_prescan(
    e: &Expr,
    assigned: &mut HashSet<String>,
    counts: &mut HashMap<String, usize>,
    has_raw: &mut bool,
) {
    if let Expr::Func(_, fb) = e {
        prescan(fb.stmts(), assigned, counts, has_raw);
        return;
    }
    e.for_each_subexpr(&mut |c| expr_prescan(c, assigned, counts, has_raw));
}

fn push_frame(params: &[String], body: &[Stmt], frames: &mut Vec<Frame>) {
    let mut assigned = HashSet::new();
    let mut counts = HashMap::new();
    let mut has_raw = false;
    prescan(body, &mut assigned, &mut counts, &mut has_raw);
    frames.push(Frame {
        bound: params.iter().cloned().collect(),
        assigned,
        assign_counts: counts,
        settled: HashSet::new(),
        has_raw,
    });
}

fn walk_block(stmts: &mut Vec<Stmt>, st: &mut Lift, frames: &mut Vec<Frame>) {
    let save = frames.last().map(|f| (f.bound.clone(), f.settled.clone()));
    // The `local`s of THIS block and, per name, how many of the frame's
    // assignments the walk has passed at this level (nested branches and
    // loop bodies of a passed statement included, function bodies never
    // — see the module doc): a declared name whose passed count reaches
    // the frame's total is settled for the rest of the block.
    let mut declared_here: HashSet<String> = HashSet::new();
    let mut passed: HashMap<String, usize> = HashMap::new();
    for s in stmts.iter_mut() {
        walk_stmt(s, st, frames);
        let Some(f) = frames.last_mut() else { continue };
        if let Stmt::Local(names, _) = s {
            declared_here.extend(names.iter().cloned());
        }
        let mut here: HashMap<String, usize> = HashMap::new();
        stmt_assigns(s, &mut here);
        for (name, n) in here {
            let p = passed.entry(name.clone()).or_insert(0);
            *p += n;
            if declared_here.contains(&name) && f.assign_counts.get(&name) == Some(p) {
                f.settled.insert(name);
            }
        }
    }
    if let (Some((b, settled)), Some(f)) = (save, frames.last_mut()) {
        f.bound = b;
        f.settled = settled;
    }
}

/// The plain-identifier assignments `s` performs when it runs: its own
/// target(s), plus those of the statements in its branches, `do` block or
/// loop body. A function body — a statement-level definition or a literal
/// inside an expression — runs at some later time or never, so its
/// assignments are not counted here (the frame's prescan total does count
/// them, which is what keeps such a name from ever settling).
fn stmt_assigns(s: &Stmt, out: &mut HashMap<String, usize>) {
    let mut note = |name: &String| {
        if is_plain_ident(name) {
            *out.entry(name.clone()).or_insert(0) += 1;
        }
    };
    match s {
        Stmt::Assign(lhs, _) | Stmt::AssignIf { lhs, .. } => note(lhs),
        Stmt::MultiAssign(lhs, _) => {
            for l in lhs {
                note(l);
            }
        }
        Stmt::If { then_b, elseifs, else_b, .. } => {
            let blocks = std::iter::once(then_b)
                .chain(elseifs.iter().map(|(_, b)| b))
                .chain(else_b.iter());
            for b in blocks {
                for inner in &b.0 {
                    stmt_assigns(inner, out);
                }
            }
        }
        Stmt::Do(b) | Stmt::WhileTrue(b) => {
            for inner in &b.0 {
                stmt_assigns(inner, out);
            }
        }
        _ => {}
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
            // A `__thunk` carrier takes up to four captures (`__mll_tk4`);
            // the lazy-cons generator family stops at three (the tail
            // evaluator's flat `g[1](g[2], g[3], g[4])` call).
            let max_caps = if slot == 0 { 4 } else { 3 };
            if let Some((fslot, caps)) = try_lift(fb, st, frames, max_caps) {
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
fn try_lift(
    fb: &mut FuncBody,
    st: &mut Lift,
    frames: &[Frame],
    max_caps: usize,
) -> Option<(String, Vec<String>)> {
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
            if frame.has_raw
                || (frame.assigned.contains(name) && !frame.settled.contains(name))
            {
                return None;
            }
            captures.push(name.clone());
        }
    }
    if captures.len() > max_caps {
        return None;
    }
    captures.sort();
    // The body must not assign a capture (the lifted parameter is a copy).
    {
        let mut body_assigned = HashSet::new();
        let mut body_counts = HashMap::new();
        let mut body_raw = false;
        prescan(fb.stmts(), &mut body_assigned, &mut body_counts, &mut body_raw);
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
