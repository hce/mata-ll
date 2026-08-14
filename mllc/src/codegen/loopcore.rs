//! Mechanics shared by the loop-conversion passes (tailloop, ioloop,
//! performloop): the runner-site predicate and its rewrite/unrewrite pair,
//! the loop scaffold and its peel, the statement-tree return walkers, and
//! the reverse self-check plumbing.
//!
//! Only genuinely shared machinery lives here — each pass's GATES and
//! skeleton logic stay in its own file, and each pass's module comment
//! carries the soundness argument for what it feeds these helpers. The
//! rewrite/unrewrite and build/peel pairs are one trusted implementation
//! each: the passes' reverse self-checks (transform, reverse, byte-compare
//! the rendered form) rely on them being exact inverses, so any change here
//! is exercised by every debug/test compile over the corpus.

use super::lua::{Block, Expr, Stmt};
use super::opt;
use super::tailloop::SelfName;

// ---- Runner-site predicates ----

/// `__mll_run_tail(<e>)` in the exact spelling the emitter produces (one
/// `Name` callee, one argument, no paren layers). Exactness keeps the
/// reverse transforms an inverse.
pub(super) fn run_tail_arg(e: &Expr) -> Option<&Expr> {
    let Expr::Call(f, args) = e else { return None };
    if matches!(f.as_ref(), Expr::Name(n) if n == "__mll_run_tail") && args.len() == 1 {
        Some(&args[0])
    } else {
        None
    }
}

/// `__mll_run_tail(<self>(e1..en))` — a rewritable runner site: both calls
/// in the exact spelling the action emitter produces (no paren layers — a
/// paren-wrapped form is left alone, which is correct either way; in ioloop
/// it stays a value branch, in performloop a kept terminal). Strictness
/// here keeps the reverse transforms exact inverses. The same
/// zero-parameter exception as tailloop's `rewritable_site` applies: with
/// no parameters there is no assignment to carry extra arguments'
/// evaluation, and the kept runner return is sound either way.
pub(super) fn run_tail_self_args<'a>(
    e: &'a Expr,
    name: &SelfName,
    params: &[String],
) -> Option<&'a Vec<Expr>> {
    let arg = run_tail_arg(e)?;
    // The one spelling the emitter uses (`Name`, slot refs included);
    // exactness keeps the reverse transform an inverse.
    let Expr::Call(callee, call_args) = arg else { return None };
    if !matches!(callee.as_ref(), Expr::Name(s) if s == name.spelling()) {
        return None;
    }
    if params.is_empty() && !call_args.is_empty() {
        return None;
    }
    Some(call_args)
}

// ---- Return walkers ----

/// Does any statement-tree tail position — the last statement, or the last
/// statement of a tail `if`/`elseif`/`else` arm or `do` block — hold a
/// `return` whose operand satisfies `pred`? The dry-run twin of the passes'
/// tail rewrites, over the same positions.
pub(super) fn tail_position_has(stmts: &[Stmt], pred: &impl Fn(&Expr) -> bool) -> bool {
    match stmts.last() {
        Some(Stmt::Return(e)) => pred(e),
        Some(Stmt::If { then_b, elseifs, else_b, .. }) => {
            tail_position_has(&then_b.0, pred)
                || elseifs.iter().any(|(_, b)| tail_position_has(&b.0, pred))
                || else_b.as_ref().is_some_and(|b| tail_position_has(&b.0, pred))
        }
        Some(Stmt::Do(b)) => tail_position_has(&b.0, pred),
        _ => false,
    }
}

/// Visit every `return` of a body (any depth of `if`/`do` nesting — guarded
/// matches put returns in non-tail blocks). Nested function bodies' returns
/// belong to those functions and are not visited.
pub(super) fn each_return(stmts: &[Stmt], f: &mut impl FnMut(&Expr)) {
    for s in stmts {
        match s {
            Stmt::Return(e) => f(e),
            Stmt::If { then_b, elseifs, else_b, .. } => {
                each_return(&then_b.0, f);
                for (_, b) in elseifs {
                    each_return(&b.0, f);
                }
                if let Some(b) = else_b {
                    each_return(&b.0, f);
                }
            }
            Stmt::Do(b) => each_return(&b.0, f),
            _ => {}
        }
    }
}

// ---- The runner-site rewrite and its inverse ----

/// Replace `return __mll_run_tail(self(e…))` sites with the simultaneous
/// parameter update and the jump to the loop's continue label. Always the
/// goto shape: a rewritten site is not in loop-body tail position in
/// general (see each pass's module comment). The update is ONE multiple
/// assignment in the call's argument order, over tailloop's simultaneity
/// and evaluation-order arguments; with zero parameters there is no update,
/// only the jump.
///
/// The one thing the two callers vary is where a site may sit:
/// * ioloop rewrites a spliced closure BODY, whose sites live only in the
///   body's own tail positions (last statement, tail `if` arms, `do`
///   blocks) — `everywhere: false` descends only through the last
///   statement.
/// * performloop rewrites a whole normalized body: returns are block-final,
///   but the blocks holding them sit at any statement position —
///   `everywhere: true` descends into every `if`/`do`.
pub(super) fn rewrite_run_tail_sites(
    stmts: &mut Vec<Stmt>,
    name: &SelfName,
    params: &[String],
    everywhere: bool,
) {
    let n = stmts.len();
    for (i, s) in stmts.iter_mut().enumerate() {
        if !everywhere && i + 1 != n {
            continue;
        }
        match s {
            Stmt::If { then_b, elseifs, else_b, .. } => {
                rewrite_run_tail_sites(&mut then_b.0, name, params, everywhere);
                for (_, b) in elseifs.iter_mut() {
                    rewrite_run_tail_sites(&mut b.0, name, params, everywhere);
                }
                if let Some(b) = else_b.as_mut() {
                    rewrite_run_tail_sites(&mut b.0, name, params, everywhere);
                }
            }
            Stmt::Do(b) => rewrite_run_tail_sites(&mut b.0, name, params, everywhere),
            _ => {}
        }
    }
    if matches!(stmts.last(), Some(Stmt::Return(e)) if run_tail_self_args(e, name, params).is_some())
    {
        let Some(Stmt::Return(Expr::Call(_, mut runner_args))) = stmts.pop() else {
            unreachable!()
        };
        let Expr::Call(_, args) = runner_args.pop().expect("runner argument") else {
            unreachable!()
        };
        if !params.is_empty() {
            stmts.push(Stmt::MultiAssign(params.to_vec(), args));
        }
        stmts.push(Stmt::Goto("continue".into()));
    }
}

/// Mirror of `rewrite_run_tail_sites`, over the same positions: a
/// block-final `[…, params = args, goto continue]` (or a bare
/// `[…, goto continue]` for a zero-parameter function) becomes the original
/// `return __mll_run_tail(self(args))` again. Returns `None` when the tree
/// does not have the exact produced shape — the caller's self-check then
/// fails loudly.
pub(super) fn unrewrite_run_tail_sites(
    stmts: &mut Vec<Stmt>,
    name: &SelfName,
    params: &[String],
    everywhere: bool,
) -> Option<()> {
    let n = stmts.len();
    for (i, s) in stmts.iter_mut().enumerate() {
        if !everywhere && i + 1 != n {
            continue;
        }
        match s {
            Stmt::If { then_b, elseifs, else_b, .. } => {
                unrewrite_run_tail_sites(&mut then_b.0, name, params, everywhere)?;
                for (_, b) in elseifs.iter_mut() {
                    unrewrite_run_tail_sites(&mut b.0, name, params, everywhere)?;
                }
                if let Some(b) = else_b.as_mut() {
                    unrewrite_run_tail_sites(&mut b.0, name, params, everywhere)?;
                }
            }
            Stmt::Do(b) => unrewrite_run_tail_sites(&mut b.0, name, params, everywhere)?,
            _ => {}
        }
    }
    if matches!(stmts.last(), Some(Stmt::Goto(l)) if l == "continue") {
        stmts.pop();
        let args = if params.is_empty() {
            Vec::new()
        } else {
            let Some(Stmt::MultiAssign(lhs, args)) = stmts.pop() else { return None };
            if lhs != params {
                return None;
            }
            args
        };
        let self_call = Expr::call_named(name.spelling(), args);
        stmts.push(Stmt::Return(Expr::call_named(
            "__mll_run_tail",
            vec![self_call],
        )));
    }
    Some(())
}

// ---- The loop scaffold and its peel ----

/// The loop-body scaffold shared by the runner-shaped passes (ioloop,
/// performloop): per-iteration parameter copies `local _wN = pN` (fresh
/// locals every iteration, so surviving closures and thunks capture their
/// own iteration's values — recursion's semantics; tailloop's module
/// comment carries the full argument), then the transformed body, then the
/// continue scaffolding. When the body can fall off its end, `do return
/// end` sits before the label so falling off exits the loop exactly like
/// the original's fall-off did (each pass's module comment argues the
/// equivalence); the `::continue::` label sits in end-of-block position,
/// the one place Lua exempts from the no-jump-into-a-local's-scope rule.
///
/// tailloop's scaffold differs — its label is emitted only when the body
/// can fall off (a diverging body's updates fall through to the loop end
/// instead of jumping) — so it builds its own.
pub(super) fn build_loop_scaffold(
    ws: &[String],
    params: &[String],
    loop_stmts: Vec<Stmt>,
) -> Vec<Stmt> {
    let falls_off = !opt::block_diverges(&Block(loop_stmts.clone()));
    let mut inner: Vec<Stmt> = Vec::with_capacity(loop_stmts.len() + params.len() + 2);
    for (w, p) in ws.iter().zip(params.iter()) {
        inner.push(Stmt::Local(vec![w.clone()], Some(Expr::name(p.clone()))));
    }
    inner.extend(loop_stmts);
    if falls_off {
        inner.push(Stmt::Do(Block(vec![Stmt::ReturnNone])));
    }
    inner.push(Stmt::Label("continue".into()));
    inner
}

/// Exact inverse of `build_loop_scaffold`, for the reverse self-checks:
/// strip the per-iteration copies and the continue scaffolding, returning
/// the transformed body. `None` when the tree does not have the exact
/// produced shape — the caller's self-check then fails loudly.
pub(super) fn peel_loop_scaffold<'a>(
    inner: &'a [Stmt],
    ws: &[String],
    params: &[String],
) -> Option<&'a [Stmt]> {
    let mut rest = inner;
    for (w, p) in ws.iter().zip(params.iter()) {
        let (c, r) = rest.split_first()?;
        let Stmt::Local(ns, Some(Expr::Name(src))) = c else { return None };
        if ns != &vec![w.clone()] || src != p {
            return None;
        }
        rest = r;
    }
    match rest {
        [r @ .., Stmt::Do(guard), Stmt::Label(l)] if l == "continue" => {
            let [Stmt::ReturnNone] = guard.0.as_slice() else { return None };
            Some(r)
        }
        [r @ .., Stmt::Label(l)] if l == "continue" => Some(r),
        _ => None,
    }
}

// ---- Reverse self-check plumbing ----

pub(super) fn render_stmts(stmts: &[Stmt]) -> String {
    let mut s = String::new();
    Block(stmts.to_vec()).render(0, &mut s);
    s
}

/// The reverse self-check's comparison and diagnostics (debug/test builds;
/// runs under each pass's `debug_assert!`): the conversion must be exactly
/// reversible — un-converting the constructed body must reproduce `expect`,
/// byte-for-byte in rendered form. This is the mechanical review every
/// corpus conversion gets: a debug-build compile of a program re-derives
/// and checks every conversion in it.
pub(super) fn reverse_check(
    pass: &str,
    expect_label: &str,
    header: &str,
    reversed: Option<Vec<Stmt>>,
    expect: &[Stmt],
) -> bool {
    let expect = render_stmts(expect);
    match reversed {
        Some(r) if render_stmts(&r) == expect => true,
        Some(r) => {
            eprintln!(
                "{pass} reverse mismatch for `{header}`:\n--- {expect_label}\n{expect}\n--- reversed\n{}",
                render_stmts(&r)
            );
            false
        }
        None => {
            eprintln!("{pass} reverse failed to parse own output for `{header}`");
            false
        }
    }
}
