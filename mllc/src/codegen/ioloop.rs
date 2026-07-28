//! Pass 6 — IO self-loop conversion, the structured tier's second pass
//! (see opt.rs's pipeline comment and annot.rs's structured-tier contract).
//!
//! An IO/ST function with pattern dispatch compiles TWO-LEVEL: the outer
//! function runs the dispatch (argument forces + pattern tests + clause
//! bindings) at action-BUILD time and returns a per-branch action closure;
//! the closure runs the effects. A self-recursive step is
//! `return __mll_run_tail(<self>(e1..en))` in tail position of a branch
//! closure: the self CALL re-dispatches and builds the next closure, the
//! forwarding runner tail-calls it. Per iteration that is one closure
//! allocation (LuaJIT's FNEW trace breaker), two calls and a runner
//! dispatch. This pass converts the whole shape to
//!
//! ```text
//! function(p1..pn)
//!     local _lp = function()
//!         while true do
//!             local w1 = p1 … local wn = pn
//!             <dispatch, renamed p→w, with each branch's action-closure
//!              BODY spliced in place of `return <closure>`, and each tail
//!              `return __mll_run_tail(self(e…))` replaced by
//!              `p1, …, pn = e1', …, en'` + `goto continue`;
//!              each `return <value>` branch becomes
//!              `return __mll_run_tail(<value>')`>
//!             ::continue::
//!         end
//!     end
//!     <the ORIGINAL body, verbatim, with each `return <closure>` replaced
//!      by `return _lp`>
//! end
//! ```
//!
//! so a running self-loop stays inside one closure — no per-iteration
//! allocation, no runner dispatch, and a loop LuaJIT can trace.
//!
//! Correctness decisions (beyond those shared with tailloop.rs — the
//! simultaneous multiple-assignment update, argument order, per-iteration
//! `w` copies for capture semantics, the scoped rename, self-identity, and
//! the header forms are identical machinery, reused from there):
//!
//! * BUILD-TIME DISPATCH IS PRESERVED, NOT MOVED. GHC parity: forcing
//!   `f undefined` to WHNF runs the match and raises BEFORE the action is
//!   ever run (`seq (f undefined) ()` raises; the ghc_oracle pins it). So
//!   the converted outer function keeps the ENTIRE original body — every
//!   force, pattern test, clause binding and the non-exhaustive raise run
//!   at build exactly as before; only the branch-closure returns are
//!   redirected to `_lp`. The first run of `_lp` re-runs that dispatch to
//!   find its branch again; iterations ≥ 2 dispatch at run time in the
//!   original too (the self call executes inside the running closure), so
//!   only the FIRST iteration repeats work — and the REPEAT-SAFE gate below
//!   confines that repeat to pure, deterministic computation.
//! * REPEAT-SAFE SKELETON — everything the outer body executes outside its
//!   `return` statements must be safe to run twice (build + first
//!   iteration) with identical outcome. The gate admits only: local
//!   bindings and plain-identifier assignments, `if`/`do` structure, the
//!   `error(…)` raise, nested function definitions (defining is pure; the
//!   body does not run), and expressions built from names, literals,
//!   indexing, operators, table/function construction, and calls to a
//!   closed helper set: the idempotent cell-memoizers `__force`,
//!   `__mll_head`, `__mll_tail`, `__mll_tail_lazy` (exactly the calls
//!   pattern paths emit — each mutates its cell only monotonically,
//!   unevaluated → memoized value, so a repeat run reads what the first
//!   memoized) and the pure allocators/inspectors `__thunk`, `__mll_cons`,
//!   `__mll_lazy_cons`, `__mll_pure`, `getmetatable`, `type`, `error`.
//!   Determinism of the repeat then rests on facts the runtime guarantees:
//!   a thunk or cons cell reached from a parameter is the SAME object in
//!   both runs, constructor cells are otherwise immutable, module slots are
//!   stored once at load, and none of the runtime metatables define
//!   comparison/arithmetic metamethods. A thunk
//!   CREATED in the skeleton and forced there re-runs its suspended body on
//!   the first iteration; suspended bodies are pure mata-ll computation by
//!   the thunk-emission discipline (actions are never `__thunk`ed — they
//!   are suspended as plain closures and only run through the runners,
//!   which the vocabulary excludes), so the repeat recomputes the same
//!   value and only costs time. Skeleton allocations (thunks, tables,
//!   closures) get fresh identities per run, but the build run's are
//!   unreachable afterward — `_lp` is declared before any skeleton local,
//!   so it cannot capture them — and pure values' identity is not
//!   observable in the language. Any statement or call outside the
//!   vocabulary (a Raw, a runner call, an unknown callee — e.g. an
//!   eagerized where-binding calling a compiled function) declines the
//!   whole conversion.
//! * BRANCH TERMINALS. Lua's grammar makes every `return` the last
//!   statement of its block, so the outer body's returns ARE the branch
//!   terminals. Three kinds:
//!   - `return <zero-parameter function literal>` — a branch action
//!     closure. Skeleton: `return _lp`. Loop: the literal's body is spliced
//!     in place (its statements land in block-final position, exactly where
//!     the return stood). Every branch maps to the ONE `_lp`; its
//!     re-dispatch selects the branch, so distinct per-branch closures are
//!     not needed. Closure identity (`rawequal` on two action values) is
//!     not observable in the language.
//!   - `return <anything else>` — a value branch: the action is a value
//!     already built (a forward to another compiled function, a boxed or
//!     bare pure result). Skeleton: unchanged, the first call returns it to
//!     the consumer exactly as before. Loop: `return __mll_run_tail(<e>')`
//!     — literally the treatment the ORIGINAL applied to it: on iterations
//!     ≥ 2 the original evaluated `self(args)` (which returned `<e>`'s
//!     value) inside `__mll_run_tail(…)`, so the loop applies the same
//!     runner to the same value at the same point and the result leaves the
//!     loop by a proper Lua tail call.
//!   - inside a spliced closure body, tail `return __mll_run_tail(self(…))`
//!     becomes the parameter update; every OTHER return (a terminal value,
//!     a pending `__mll_pure` box, a tail forward `__mll_run_tail(other(…))`)
//!     is kept verbatim and returns from `_lp`.
//! * THE BOX CONVENTION IS UNTOUCHED. The forwarding runner's function arm
//!   is a bare `return action()`, so in the original a terminal's value —
//!   boxed or bare — travels through the tail chain to the ONE consuming
//!   site at the chain's root unchanged. The loop returns exactly the same
//!   terminal expressions from `_lp` to the same root; no box is added or
//!   stripped. Effect order is also unchanged: the original interleaves
//!   [iteration N's effects][build of N+1: dispatch + bindings][closure
//!   N+1's effects]; the loop runs [effects][update][dispatch + bindings]
//!   [effects] — the same sequence.
//! * MIXED FUNCTIONS. Value branches reached on iterations ≥ 2 (a tail
//!   forward to ANOTHER compiled function, a base case built by `pure`)
//!   exit the loop by `return`; only self sites loop. Non-tail self uses
//!   (`__mll_run(self(…))` binds, first-class references) call the
//!   converted function, whose external contract — build-time dispatch,
//!   then one action closure — is preserved, so every other caller is
//!   correct by construction.
//! * DIVERGENCE OF SPLICED BODIES — a spliced body's statements sit where
//!   `return <closure>` stood, which (for guarded matches) may be followed
//!   by further clause tests and the non-exhaustive raise. A body that
//!   could FALL OFF its end would fall into those. The original closure's
//!   fall-off meant "action result nil"; the splice would mean "try the
//!   next clause" — wrong. So every spliced body must diverge
//!   (`opt::block_diverges`); the bind-chain emitter always ends closure
//!   bodies in a return, so this declines only hand-shaped trees.
//! * CONTINUE MECHANISM — unlike tailloop's, an update site is generally
//!   NOT in loop-body tail position (guarded chains put statements after
//!   the enclosing `if`), so every update jumps `goto continue` to the
//!   label in end-of-block position of the loop. When the constructed body
//!   can fall off its end (the skeleton could — meaning the original built
//!   a nil action), `do return end` sits before the label so falling off
//!   exits the loop exactly like the original's nil result did after the
//!   runners' normalization.
//! * MULTI-VALUE ARITY — no gate is needed: every consumer of an action
//!   value or an action result receives it in an argument or single-RHS
//!   position (the runners, `__mll_unbox`, a bind local), all of which
//!   truncate identically before and after conversion, and `return
//!   __mll_run_tail(…)` forwards from `_lp` exactly like the original
//!   forwarded through the runner chain.
//!
//! The other IO recursion shape — a single-clause function that PERFORMS at
//! call time and recurses through `return __mll_run_tail(self(…))` at the
//! OUTER body's tail — is out of scope here: its effects live in skeleton
//! position, so the repeat-safe gate declines it. (That shape recurses one
//! Lua frame per step today; converting it is a separate, simpler pass with
//! a different equivalence argument.)

use std::collections::{HashMap, HashSet};

use super::annot::{self, ScopeView, is_plain_ident};
use super::lua::{Block, Expr, FuncBody, Stmt};
use super::opt;
use super::tailloop::{
    self, SelfName, parse_header, rename_block, rename_blocked, self_qualifies,
};

pub(super) struct IoLoop;

impl annot::StructuredPass for IoLoop {
    fn request(
        &mut self,
        header: &str,
        body: &Block,
        view: &ScopeView<'_>,
        locals_in_scope: &HashSet<String>,
    ) -> Option<Block> {
        convert(header, body, view, locals_in_scope)
    }
}

/// The statements of a zero-parameter function literal — a branch action
/// closure — with grouping parens peeled (`(function() … end)` from
/// `inline_fn0`). A literal WITH parameters is a function VALUE, not an
/// action closure, and is treated as a value branch.
fn closure_stmts(e: &Expr) -> Option<&Vec<Stmt>> {
    let mut e = e;
    while let Expr::Paren(inner) = e {
        e = inner;
    }
    match e {
        Expr::Func(params, body) if params.is_empty() => Some(match body {
            FuncBody::Inline(s) => s,
            FuncBody::Block(Block(s)) => s,
        }),
        _ => None,
    }
}

/// `__mll_run_tail(<self>(e1..en))`, both calls in the exact spelling the
/// action emitter produces (no paren layers — a paren-wrapped form is left
/// as a value branch, which is correct either way; strictness here keeps
/// the reverse transform below an exact inverse). The same zero-parameter
/// exception as tailloop's `rewritable_site` applies: with no parameters
/// there is no assignment to carry extra arguments' evaluation.
fn run_tail_self_args<'a>(
    e: &'a Expr,
    name: &SelfName,
    params: &[String],
) -> Option<&'a Vec<Expr>> {
    let Expr::Call(f, args) = e else { return None };
    if !matches!(f.as_ref(), Expr::Name(n) if n == "__mll_run_tail") || args.len() != 1 {
        return None;
    }
    let Expr::Call(callee, call_args) = &args[0] else { return None };
    // The one spelling the emitter uses (`Name`, slot refs included);
    // exactness keeps the reverse transform an inverse.
    if !matches!(callee.as_ref(), Expr::Name(s) if s == name_spelling(name)) {
        return None;
    }
    if params.is_empty() && !call_args.is_empty() {
        return None;
    }
    Some(call_args)
}

// ---- Branch discovery ----

/// Visit every `return` of the outer body (any depth of `if`/`do` nesting —
/// guarded matches put returns in non-tail blocks). Nested function bodies'
/// returns belong to those functions and are not visited.
fn each_return(stmts: &[Stmt], f: &mut impl FnMut(&Expr)) {
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

/// Does a spliced closure body have at least one rewritable tail self site?
/// Tail positions of the CLOSURE body (last statement, tail `if` arms, `do`
/// blocks) — a site inside a nested loop or literal re-enters the converted
/// function instead, which is the same semantics.
fn body_has_site(stmts: &[Stmt], name: &SelfName, params: &[String]) -> bool {
    match stmts.last() {
        Some(Stmt::Return(e)) => run_tail_self_args(e, name, params).is_some(),
        Some(Stmt::If { then_b, elseifs, else_b, .. }) => {
            body_has_site(&then_b.0, name, params)
                || elseifs.iter().any(|(_, b)| body_has_site(&b.0, name, params))
                || else_b.as_ref().is_some_and(|b| body_has_site(&b.0, name, params))
        }
        Some(Stmt::Do(b)) => body_has_site(&b.0, name, params),
        _ => false,
    }
}

// ---- The repeat-safe gate (see the module comment) ----

fn skeleton_repeat_safe(stmts: &[Stmt]) -> bool {
    stmts.iter().all(repeat_safe_stmt)
}

fn repeat_safe_stmt(s: &Stmt) -> bool {
    match s {
        // Branch terminals: a closure return's literal is only CONSTRUCTED
        // here (pure), and a value return ends the build — it runs once in
        // whichever world reaches it (see the module comment).
        Stmt::Return(_) => true,
        Stmt::Local(_, None) => true,
        Stmt::Local(_, Some(e)) => repeat_safe_expr(e),
        Stmt::Assign(lhs, e) => is_plain_ident(lhs) && repeat_safe_expr(e),
        Stmt::AssignIf { lhs, cond, then_e, else_e } => {
            is_plain_ident(lhs)
                && repeat_safe_expr(cond)
                && repeat_safe_expr(then_e)
                && repeat_safe_expr(else_e)
        }
        Stmt::If { cond, then_b, elseifs, else_b } => {
            repeat_safe_expr(cond)
                && skeleton_repeat_safe(&then_b.0)
                && elseifs
                    .iter()
                    .all(|(c, b)| repeat_safe_expr(c) && skeleton_repeat_safe(&b.0))
                && else_b.as_ref().is_none_or(|b| skeleton_repeat_safe(&b.0))
        }
        Stmt::Do(b) => skeleton_repeat_safe(&b.0),
        // The non-exhaustive raise: if the build raises there is no first
        // iteration; if it does not, the deterministic re-dispatch takes the
        // same branch and never reaches it.
        Stmt::Expr(Expr::Call(f, args)) => {
            matches!(f.as_ref(), Expr::Name(n) if n == "error")
                && args.iter().all(repeat_safe_expr)
        }
        // Defining a nested function is pure; the body runs only when
        // called. Re-defining per iteration matches the original's
        // per-self-call definition. A slot header here would be a store the
        // module-wide census counted once but the converted output performs
        // twice — the emitter keeps slot functions top-level, and this
        // check turns that assumption into a gate.
        Stmt::Function { header, .. } => !header.contains("__mll_fn"),
        // Raw (opaque), runner calls, multi-assignments, loop scaffolding,
        // returns of tables: outside the vocabulary.
        _ => false,
    }
}

fn repeat_safe_expr(e: &Expr) -> bool {
    match e {
        // Reads: locals/upvalues directly; composite spellings (`_v[3]`,
        // `math.pi`, `__mll_fn[7]`) read tables that carry no metamethods
        // (runtime tables, the module's own structures).
        Expr::Name(_) | Expr::Lit(_) => true,
        Expr::Raw(_) => false,
        Expr::Paren(inner) | Expr::Neg(inner) => repeat_safe_expr(inner),
        Expr::Index(base, _) => repeat_safe_expr(base),
        Expr::Binop(_, l, r) => repeat_safe_expr(l) && repeat_safe_expr(r),
        Expr::Table(items) | Expr::TableSpaced(items) => items.iter().all(|i| match i {
            super::lua::Item::Pos(e) | super::lua::Item::KV(_, e) => repeat_safe_expr(e),
        }),
        // Construction only — the body is not executed by the skeleton.
        Expr::Func(..) => true,
        Expr::Method(..) => false,
        Expr::Call(f, args) => {
            matches!(f.as_ref(), Expr::Name(n) if matches!(
                n.as_str(),
                // Idempotent cell-memoizers: a repeat run reads the value
                // (or thunk object) the first run memoized into the shared
                // cell, so both runs observe identical results and sharing.
                "__force" | "__mll_head" | "__mll_tail" | "__mll_tail_lazy"
                    // Pure allocators and inspectors.
                    | "__thunk" | "__mll_cons" | "__mll_lazy_cons" | "__mll_pure"
                    | "getmetatable" | "type" | "error"
            )) && args.iter().all(repeat_safe_expr)
        }
    }
}

// ---- The two body transformations ----

/// The build-phase skeleton: the original body with every branch-closure
/// return redirected to the shared loop closure. Value returns and
/// everything else stay verbatim, preserving the build's forces, bindings
/// and raises exactly.
fn to_skeleton(stmts: &mut [Stmt], lp: &str) {
    for s in stmts {
        match s {
            Stmt::Return(e) => {
                if closure_stmts(e).is_some() {
                    *e = Expr::name(lp.to_string());
                }
            }
            Stmt::If { then_b, elseifs, else_b, .. } => {
                to_skeleton(&mut then_b.0, lp);
                for (_, b) in elseifs.iter_mut() {
                    to_skeleton(&mut b.0, lp);
                }
                if let Some(b) = else_b.as_mut() {
                    to_skeleton(&mut b.0, lp);
                }
            }
            Stmt::Do(b) => to_skeleton(&mut b.0, lp),
            _ => {}
        }
    }
}

/// Replace tail `return __mll_run_tail(self(e…))` sites of a spliced
/// closure body with the simultaneous parameter update and the jump to the
/// loop's continue label. Always the goto shape: a spliced site is not in
/// loop-body tail position in general (see the module comment).
fn rewrite_sites(stmts: &mut Vec<Stmt>, name: &SelfName, params: &[String]) {
    match stmts.last_mut() {
        Some(Stmt::Return(e)) => {
            if run_tail_self_args(e, name, params).is_none() {
                return;
            }
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
        Some(Stmt::If { then_b, elseifs, else_b, .. }) => {
            rewrite_sites(&mut then_b.0, name, params);
            for (_, b) in elseifs.iter_mut() {
                rewrite_sites(&mut b.0, name, params);
            }
            if let Some(b) = else_b.as_mut() {
                rewrite_sites(&mut b.0, name, params);
            }
        }
        Some(Stmt::Do(b)) => rewrite_sites(&mut b.0, name, params),
        _ => {}
    }
}

/// The loop-phase body: branch closures spliced (with their tail self sites
/// rewritten), value branches run through the forwarding runner. Input
/// statements are already renamed p→w; the update's lvalues are the real
/// parameters, written here.
fn to_loop(stmts: Vec<Stmt>, name: &SelfName, params: &[String]) -> Vec<Stmt> {
    let mut out = Vec::with_capacity(stmts.len());
    for s in stmts {
        match s {
            Stmt::Return(e) => {
                if closure_stmts(&e).is_some() {
                    let mut e = e;
                    while let Expr::Paren(inner) = e {
                        e = *inner;
                    }
                    let Expr::Func(_, fb) = e else { unreachable!() };
                    let mut body = match fb {
                        FuncBody::Inline(s) => s,
                        FuncBody::Block(Block(s)) => s,
                    };
                    rewrite_sites(&mut body, name, params);
                    out.extend(body);
                } else {
                    out.push(Stmt::Return(Expr::call_named("__mll_run_tail", vec![e])));
                }
            }
            Stmt::If { cond, then_b, elseifs, else_b } => out.push(Stmt::If {
                cond,
                then_b: Block(to_loop(then_b.0, name, params)),
                elseifs: elseifs
                    .into_iter()
                    .map(|(c, b)| (c, Block(to_loop(b.0, name, params))))
                    .collect(),
                else_b: else_b.map(|b| Block(to_loop(b.0, name, params))),
            }),
            Stmt::Do(b) => out.push(Stmt::Do(Block(to_loop(b.0, name, params)))),
            other => out.push(other),
        }
    }
    out
}

// ---- The conversion ----

fn convert(
    header: &str,
    body: &Block,
    view: &ScopeView<'_>,
    locals_in_scope: &HashSet<String>,
) -> Option<Block> {
    let (self_name, params) = parse_header(header)?;
    if !self_qualifies(&self_name, view, locals_in_scope) {
        return None;
    }
    // At least one branch closure loops back to self; and every branch
    // closure diverges, so its splice cannot fall into the statements that
    // follow its return site (the divergence check runs on ALL closure
    // branches, converting or not — each is spliced).
    let mut any_site = false;
    let mut all_diverge = true;
    each_return(&body.0, &mut |e| {
        if let Some(stmts) = closure_stmts(e) {
            if body_has_site(stmts, &self_name, &params) {
                any_site = true;
            }
            if !opt::block_diverges(&Block(stmts.clone())) {
                all_diverge = false;
            }
        }
    });
    if !any_site || !all_diverge {
        return None;
    }
    if !skeleton_repeat_safe(&body.0) {
        return None;
    }
    let param_set: HashSet<String> = params.iter().cloned().collect();
    if rename_blocked(&body.0, &param_set) {
        return None;
    }

    // Fresh names for the per-iteration copies and the loop closure,
    // against every identifier token of the rendered function.
    let used = tailloop::used_tokens(header, &body.0);
    let ws = tailloop::fresh_with_prefix(&used, "_w", params.len());
    let lp = {
        let mut cand = String::from("_lp");
        while used.contains(&cand) {
            cand.push('_');
        }
        cand
    };

    // The loop body: rename p→w over the WHOLE body first (closure bodies
    // included — their splice must read the copies), then transform.
    let map: HashMap<String, String> =
        params.iter().cloned().zip(ws.iter().cloned()).collect();
    let mut loop_stmts = body.0.clone();
    rename_block(&mut loop_stmts, &map);
    let loop_stmts = to_loop(loop_stmts, &self_name, &params);

    let mut inner: Vec<Stmt> = Vec::with_capacity(loop_stmts.len() + params.len() + 3);
    for (w, p) in ws.iter().zip(params.iter()) {
        inner.push(Stmt::Local(vec![w.clone()], Some(Expr::name(p.clone()))));
    }
    let falls_off = !opt::block_diverges(&Block(loop_stmts.clone()));
    inner.extend(loop_stmts);
    if falls_off {
        inner.push(Stmt::Do(Block(vec![Stmt::ReturnNone])));
    }
    inner.push(Stmt::Label("continue".into()));

    // Locals budget, per scope: the outer body keeps its own locals plus
    // `_lp`; the loop closure is its own scope holding the copies, the
    // skeleton's locals and every spliced body's. (Same conservative 2×
    // parameter allowance as tailloop: parameters occupy slots the
    // statement-level count cannot see.)
    if opt::count_locals(&body.0) + 1 > super::CodeGen::LOCAL_LIMIT
        || opt::count_locals(&inner) + 2 * params.len() > super::CodeGen::LOCAL_LIMIT
    {
        return None;
    }

    let mut skeleton = body.0.clone();
    to_skeleton(&mut skeleton, &lp);

    let mut out: Vec<Stmt> = Vec::with_capacity(skeleton.len() + 1);
    out.push(Stmt::Local(
        vec![lp.clone()],
        Some(Expr::Func(
            vec![],
            FuncBody::Block(Block(vec![Stmt::WhileTrue(Block(inner))])),
        )),
    ));
    out.extend(skeleton);

    // Self-check (debug/test builds): the conversion must be exactly
    // reversible — un-converting the constructed body must reproduce the
    // original, byte-for-byte in rendered form (modulo the closure-spelling
    // canonicalization `reverse` documents). This is the mechanical review
    // every corpus conversion gets: a debug-build compile of a program
    // re-derives and checks every conversion in it.
    debug_assert!(
        {
            let reversed = reverse(&out, &self_name, &params, &ws, &lp);
            let expect = render_stmts(&canonical_closure_returns(&body.0));
            match reversed {
                Some(r) if render_stmts(&r) == expect => true,
                Some(r) => {
                    eprintln!(
                        "ioloop reverse mismatch for `{}`:\n--- original\n{}\n--- reversed\n{}",
                        header,
                        expect,
                        render_stmts(&r)
                    );
                    false
                }
                None => {
                    eprintln!("ioloop reverse failed to parse own output for `{}`", header);
                    false
                }
            }
        },
        "ioloop conversion is not reversible (see stderr)"
    );

    Some(Block(out))
}

// ---- The reverse transform (self-check; see convert) ----

fn render_stmts(stmts: &[Stmt]) -> String {
    let mut s = String::new();
    Block(stmts.to_vec()).render(0, &mut s);
    s
}

/// The original body with every branch-closure return in the ONE canonical
/// spelling `reverse` can reproduce: parens peeled, `FuncBody::Block`. Both
/// rewrites are render-only identities for a zero-parameter literal in
/// return position (the paren truncates a single value; inline/block bodies
/// render differently but denote the same function).
fn canonical_closure_returns(stmts: &[Stmt]) -> Vec<Stmt> {
    stmts
        .iter()
        .map(|s| match s {
            Stmt::Return(e) if closure_stmts(e).is_some() => Stmt::Return(Expr::Func(
                vec![],
                FuncBody::Block(Block(closure_stmts(e).unwrap().clone())),
            )),
            Stmt::If { cond, then_b, elseifs, else_b } => Stmt::If {
                cond: cond.clone(),
                then_b: Block(canonical_closure_returns(&then_b.0)),
                elseifs: elseifs
                    .iter()
                    .map(|(c, b)| (c.clone(), Block(canonical_closure_returns(&b.0))))
                    .collect(),
                else_b: else_b
                    .as_ref()
                    .map(|b| Block(canonical_closure_returns(&b.0))),
            },
            Stmt::Do(b) => Stmt::Do(Block(canonical_closure_returns(&b.0))),
            other => other.clone(),
        })
        .collect()
}

/// Un-convert a converted body: recover the original (canonically spelled)
/// body from the loop closure and the skeleton alone. Returns `None` when
/// the converted tree does not have the exact produced shape — the
/// self-check then fails loudly.
fn reverse(
    converted: &[Stmt],
    name: &SelfName,
    params: &[String],
    ws: &[String],
    lp: &str,
) -> Option<Vec<Stmt>> {
    // Peel `local _lp = function() while true do … end end` + skeleton.
    let (first, skeleton) = converted.split_first()?;
    let Stmt::Local(names, Some(Expr::Func(fparams, FuncBody::Block(Block(lbody))))) = first
    else {
        return None;
    };
    if names != &vec![lp.to_string()] || !fparams.is_empty() {
        return None;
    }
    let [Stmt::WhileTrue(Block(inner))] = lbody.as_slice() else { return None };
    // Strip the per-iteration copies and the continue scaffolding.
    let mut rest = inner.as_slice();
    for (w, p) in ws.iter().zip(params.iter()) {
        let (c, r) = rest.split_first()?;
        let Stmt::Local(ns, Some(Expr::Name(src))) = c else { return None };
        if ns != &vec![w.clone()] || src != p {
            return None;
        }
        rest = r;
    }
    let rest = match rest {
        [r @ .., Stmt::Do(guard), Stmt::Label(l)] if l == "continue" => {
            let [Stmt::ReturnNone] = guard.0.as_slice() else { return None };
            r
        }
        [r @ .., Stmt::Label(l)] if l == "continue" => r,
        _ => return None,
    };
    let unmap: HashMap<String, String> =
        ws.iter().cloned().zip(params.iter().cloned()).collect();
    lockstep(skeleton, rest, name, params, lp, &unmap)
}

/// Walk skeleton and loop blocks in lockstep, reconstructing the original.
fn lockstep(
    skel: &[Stmt],
    lp_side: &[Stmt],
    name: &SelfName,
    params: &[String],
    lp: &str,
    unmap: &HashMap<String, String>,
) -> Option<Vec<Stmt>> {
    let mut out = Vec::with_capacity(skel.len());
    let mut l = lp_side.iter();
    for (i, s) in skel.iter().enumerate() {
        match s {
            // A branch-closure return: the loop side holds the spliced
            // (renamed, site-rewritten) closure body — everything remaining
            // in this block, since a return is block-final.
            Stmt::Return(Expr::Name(n)) if n == lp => {
                if i + 1 != skel.len() {
                    return None;
                }
                let mut spliced: Vec<Stmt> = l.cloned().collect();
                unrewrite_sites(&mut spliced, name, params)?;
                rename_block(&mut spliced, unmap);
                out.push(Stmt::Return(Expr::Func(
                    vec![],
                    FuncBody::Block(Block(spliced)),
                )));
                return Some(out);
            }
            // A value branch: the loop side wrapped it in the runner.
            Stmt::Return(e) => {
                let Stmt::Return(Expr::Call(f, args)) = l.next()? else { return None };
                if !matches!(f.as_ref(), Expr::Name(n) if n == "__mll_run_tail")
                    || args.len() != 1
                {
                    return None;
                }
                let mut arg = vec![Stmt::Return(args[0].clone())];
                rename_block(&mut arg, unmap);
                if render_stmts(&arg) != render_stmts(&[Stmt::Return(e.clone())]) {
                    return None;
                }
                out.push(s.clone());
            }
            Stmt::If { cond, then_b, elseifs, else_b } => {
                let Stmt::If {
                    cond: lc,
                    then_b: lt,
                    elseifs: le,
                    else_b: lo,
                } = l.next()?
                else {
                    return None;
                };
                if !unrenames_to(lc, cond, unmap) || elseifs.len() != le.len() {
                    return None;
                }
                for ((c, _), (lc, _)) in elseifs.iter().zip(le.iter()) {
                    if !unrenames_to(lc, c, unmap) {
                        return None;
                    }
                }
                let else_rec = match (else_b, lo) {
                    (Some(b), Some(lb)) => {
                        Some(Block(lockstep(&b.0, &lb.0, name, params, lp, unmap)?))
                    }
                    (None, None) => None,
                    _ => return None,
                };
                out.push(Stmt::If {
                    cond: cond.clone(),
                    then_b: Block(lockstep(&then_b.0, &lt.0, name, params, lp, unmap)?),
                    elseifs: elseifs
                        .iter()
                        .zip(le.iter())
                        .map(|((c, b), (_, lb))| {
                            lockstep(&b.0, &lb.0, name, params, lp, unmap).map(|r| (c.clone(), Block(r)))
                        })
                        .collect::<Option<Vec<_>>>()?,
                    else_b: else_rec,
                });
            }
            Stmt::Do(b) => {
                let Stmt::Do(lb) = l.next()? else { return None };
                out.push(Stmt::Do(Block(lockstep(&b.0, &lb.0, name, params, lp, unmap)?)));
            }
            // Any other statement must be the skeleton statement, renamed.
            other => {
                let mut lo = vec![l.next()?.clone()];
                rename_block(&mut lo, unmap);
                if render_stmts(&lo) != render_stmts(std::slice::from_ref(other)) {
                    return None;
                }
                out.push(other.clone());
            }
        }
    }
    if l.next().is_some() {
        return None;
    }
    Some(out)
}

/// `render(unrename(l)) == render(s)` for a single expression.
fn unrenames_to(l: &Expr, s: &Expr, unmap: &HashMap<String, String>) -> bool {
    let mut probe = vec![Stmt::Return(l.clone())];
    rename_block(&mut probe, unmap);
    render_stmts(&probe) == render_stmts(&[Stmt::Return(s.clone())])
}

/// Mirror of `rewrite_sites`: a tail `[…, params = args, goto continue]`
/// (or a bare `[…, goto continue]` for a zero-parameter function) becomes
/// the original `return __mll_run_tail(self(args))`.
fn unrewrite_sites(stmts: &mut Vec<Stmt>, name: &SelfName, params: &[String]) -> Option<()> {
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
        let self_call = Expr::call_named(name_spelling(name), args);
        stmts.push(Stmt::Return(Expr::call_named("__mll_run_tail", vec![self_call])));
        return Some(());
    }
    match stmts.last_mut() {
        Some(Stmt::If { then_b, elseifs, else_b, .. }) => {
            unrewrite_sites(&mut then_b.0, name, params)?;
            for (_, b) in elseifs.iter_mut() {
                unrewrite_sites(&mut b.0, name, params)?;
            }
            if let Some(b) = else_b.as_mut() {
                unrewrite_sites(&mut b.0, name, params)?;
            }
        }
        Some(Stmt::Do(b)) => unrewrite_sites(&mut b.0, name, params)?,
        _ => {}
    }
    Some(())
}

fn name_spelling(name: &SelfName) -> &str {
    match name {
        SelfName::LocalFn(s) | SelfName::Assigned(s) | SelfName::Slot(s) => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::annot::Engine;

    fn converted(mut stmts: Vec<Stmt>) -> (String, bool) {
        let rewrote = Engine::run_structured(&mut stmts, &mut IoLoop).is_some();
        let mut out = String::new();
        Block(stmts).render(0, &mut out);
        (out, rewrote)
    }

    fn run_tail(e: Expr) -> Expr {
        Expr::call_named("__mll_run_tail", vec![e])
    }

    fn closure(stmts: Vec<Stmt>) -> Expr {
        Expr::Func(vec![], FuncBody::Block(Block(stmts)))
    }

    /// The canonical two-level countdown:
    /// `__mll_fn[3] = function(_arg0)` forcing at build, terminal branch
    /// `pure ()` closure, looping branch with an effect statement and the
    /// tail self site.
    fn countdown() -> Vec<Stmt> {
        vec![
            Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
            Stmt::Function {
                header: "__mll_fn[3] = function(_arg0)".into(),
                body: Block(vec![
                    Stmt::Assign("_arg0".into(), Expr::force(Expr::name("_arg0"))),
                    Stmt::If {
                        cond: Expr::binop("==", Expr::name("_arg0"), Expr::lit("0")),
                        then_b: Block(vec![Stmt::Return(closure(vec![Stmt::Return(
                            Expr::lit("nil"),
                        )]))]),
                        elseifs: vec![],
                        else_b: Some(Block(vec![
                            Stmt::Local(vec!["n".into()], Some(Expr::name("_arg0"))),
                            Stmt::Return(closure(vec![
                                Stmt::Expr(run_tail(Expr::call_named(
                                    "__mll_fn[9]",
                                    vec![Expr::name("n")],
                                ))),
                                Stmt::Return(run_tail(Expr::call_named(
                                    "__mll_fn[3]",
                                    vec![Expr::binop("-", Expr::name("n"), Expr::lit("1"))],
                                ))),
                            ])),
                        ])),
                    },
                ]),
            },
        ]
    }

    #[test]
    fn countdown_converts() {
        let (out, rewrote) = converted(countdown());
        assert!(rewrote, "{out}");
        // One loop closure, declared before the skeleton.
        assert!(out.contains("local _lp = function()"), "{out}");
        assert!(out.contains("while true do"), "{out}");
        // The loop re-dispatches on the per-iteration copy.
        assert!(out.contains("local _w0 = _arg0"), "{out}");
        assert!(out.contains("_w0 = __force(_w0)"), "{out}");
        // Terminal branch spliced: the box-convention-bearing return is
        // verbatim inside the loop.
        assert!(out.contains("if _w0 == 0 then\n                return nil"), "{out}");
        // Effect statement kept; tail site became update + goto.
        assert!(out.contains("__mll_run_tail(__mll_fn[9](n))"), "{out}");
        assert!(out.contains("_arg0 = n - 1"), "{out}");
        assert!(out.contains("goto continue"), "{out}");
        assert!(out.contains("::continue::"), "{out}");
        // The skeleton keeps the build-time force and returns _lp from both
        // branches.
        assert!(out.contains("_arg0 = __force(_arg0)"), "{out}");
        assert_eq!(out.matches("return _lp").count(), 2, "{out}");
        // No per-iteration closure remains in the loop.
        assert!(!out.contains("return function()"), "{out}");
    }

    #[test]
    fn value_branch_gets_runner_in_loop_only() {
        // Mixed function: one branch forwards to another compiled function
        // (a value branch). Build must return it verbatim; the loop must
        // run it through the forwarding runner.
        let stmts = vec![
            Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
            Stmt::Function {
                header: "__mll_fn[3] = function(_arg0)".into(),
                body: Block(vec![Stmt::If {
                    cond: Expr::name("_arg0"),
                    then_b: Block(vec![Stmt::Return(Expr::call_named(
                        "__mll_fn[7]",
                        vec![Expr::name("_arg0")],
                    ))]),
                    elseifs: vec![],
                    else_b: Some(Block(vec![Stmt::Return(closure(vec![Stmt::Return(
                        run_tail(Expr::call_named("__mll_fn[3]", vec![Expr::name("_arg0")])),
                    )]))])),
                }]),
            },
        ];
        let (out, rewrote) = converted(stmts);
        assert!(rewrote, "{out}");
        // Loop side: runner-wrapped, reading the copy.
        assert!(out.contains("return __mll_run_tail(__mll_fn[7](_w0))"), "{out}");
        // Skeleton side: verbatim.
        assert!(out.contains("return __mll_fn[7](_arg0)"), "{out}");
    }

    #[test]
    fn other_call_tail_forward_stays() {
        // A branch closure whose tail forwards to ANOTHER function must
        // keep that forward as a return out of the loop.
        let stmts = vec![
            Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
            Stmt::Function {
                header: "__mll_fn[3] = function(_arg0)".into(),
                body: Block(vec![Stmt::If {
                    cond: Expr::name("_arg0"),
                    then_b: Block(vec![Stmt::Return(closure(vec![Stmt::Return(run_tail(
                        Expr::call_named("__mll_fn[8]", vec![Expr::name("_arg0")]),
                    ))]))]),
                    elseifs: vec![],
                    else_b: Some(Block(vec![Stmt::Return(closure(vec![Stmt::Return(
                        run_tail(Expr::call_named("__mll_fn[3]", vec![Expr::name("_arg0")])),
                    )]))])),
                }]),
            },
        ];
        let (out, rewrote) = converted(stmts);
        assert!(rewrote, "{out}");
        assert!(out.contains("return __mll_run_tail(__mll_fn[8](_w0))"), "{out}");
        // Self site rewritten, other site untouched.
        assert!(out.contains("_arg0 = _w0"), "{out}");
    }

    #[test]
    fn effectful_skeleton_declines() {
        // A single-clause perform-at-call function: the effect statement
        // sits in skeleton position — the repeat-safe gate must decline.
        let stmts = vec![
            Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
            Stmt::Function {
                header: "__mll_fn[3] = function(_arg0)".into(),
                body: Block(vec![
                    Stmt::Expr(run_tail(Expr::call_named(
                        "__mll_fn[9]",
                        vec![Expr::name("_arg0")],
                    ))),
                    Stmt::Return(closure(vec![Stmt::Return(run_tail(Expr::call_named(
                        "__mll_fn[3]",
                        vec![Expr::name("_arg0")],
                    )))])),
                ]),
            },
        ];
        let (out, rewrote) = converted(stmts);
        assert!(!rewrote, "{out}");
    }

    #[test]
    fn unknown_call_in_dispatch_declines() {
        // An eagerized where-binding calling a compiled function in
        // skeleton position: outside the repeat-safe vocabulary.
        let mut stmts = countdown();
        let Stmt::Function { body, .. } = &mut stmts[1] else { panic!("shape") };
        body.0.insert(
            0,
            Stmt::Local(
                vec!["w".into()],
                Some(Expr::call_named("__mll_fn[12]", vec![Expr::lit("1")])),
            ),
        );
        let (out, rewrote) = converted(stmts);
        assert!(!rewrote, "{out}");
    }

    #[test]
    fn non_tail_self_run_stays() {
        // A bind `__mll_run(self(…))` inside the branch closure is a
        // non-tail use: it must survive verbatim (it re-enters the
        // converted function, whose external contract is preserved).
        let stmts = vec![
            Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
            Stmt::Function {
                header: "__mll_fn[3] = function(_arg0)".into(),
                body: Block(vec![Stmt::If {
                    cond: Expr::name("_arg0"),
                    then_b: Block(vec![Stmt::Return(closure(vec![Stmt::Return(
                        Expr::lit("nil"),
                    )]))]),
                    elseifs: vec![],
                    else_b: Some(Block(vec![Stmt::Return(closure(vec![
                        Stmt::Local(
                            vec!["x".into()],
                            Some(Expr::call_named(
                                "__mll_run",
                                vec![Expr::call_named("__mll_fn[3]", vec![Expr::lit("1")])],
                            )),
                        ),
                        Stmt::Return(run_tail(Expr::call_named(
                            "__mll_fn[3]",
                            vec![Expr::name("x")],
                        ))),
                    ]))])),
                }]),
            },
        ];
        let (out, rewrote) = converted(stmts);
        assert!(rewrote, "{out}");
        assert!(out.contains("local x = __mll_run(__mll_fn[3](1))"), "{out}");
        assert!(out.contains("_arg0 = x"), "{out}");
    }

    #[test]
    fn raw_in_skeleton_declines() {
        let mut stmts = countdown();
        let Stmt::Function { body, .. } = &mut stmts[1] else { panic!("shape") };
        body.0.insert(0, Stmt::Raw("host_hook()".into()));
        let (_, rewrote) = converted(stmts);
        assert!(!rewrote);
    }

    #[test]
    fn second_slot_store_declines() {
        let mut stmts = countdown();
        stmts.push(Stmt::Assign("__mll_fn[3]".into(), Expr::name("other")));
        let (_, rewrote) = converted(stmts);
        assert!(!rewrote);
    }

    #[test]
    fn non_diverging_closure_body_declines() {
        // A branch closure that can fall off its end: splicing it would
        // fall into the following clause tests. Must decline.
        let stmts = vec![
            Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
            Stmt::Function {
                header: "__mll_fn[3] = function(_arg0)".into(),
                body: Block(vec![
                    Stmt::If {
                        cond: Expr::name("_arg0"),
                        then_b: Block(vec![Stmt::Return(closure(vec![Stmt::If {
                            cond: Expr::name("_arg0"),
                            then_b: Block(vec![Stmt::Return(run_tail(Expr::call_named(
                                "__mll_fn[3]",
                                vec![Expr::lit("1")],
                            )))]),
                            elseifs: vec![],
                            else_b: None,
                        }]))]),
                        elseifs: vec![],
                        else_b: None,
                    },
                    Stmt::Expr(Expr::call_named("error", vec![Expr::lit("\"boom\"")])),
                ]),
            },
        ];
        let (_, rewrote) = converted(stmts);
        assert!(!rewrote);
    }

    #[test]
    fn guarded_chain_update_jumps_past_error() {
        // Guarded-match shape: the branch closure's site is NOT in loop
        // tail position (the raise follows the if). The update must jump.
        let stmts = vec![
            Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
            Stmt::Function {
                header: "__mll_fn[3] = function(_arg0)".into(),
                body: Block(vec![
                    Stmt::If {
                        cond: Expr::name("_arg0"),
                        then_b: Block(vec![Stmt::Return(closure(vec![Stmt::Return(
                            run_tail(Expr::call_named("__mll_fn[3]", vec![Expr::lit("1")])),
                        )]))]),
                        elseifs: vec![],
                        else_b: None,
                    },
                    Stmt::Expr(Expr::call_named("error", vec![Expr::lit("\"boom\"")])),
                ]),
            },
        ];
        let (out, rewrote) = converted(stmts);
        assert!(rewrote, "{out}");
        assert!(out.contains("goto continue"), "{out}");
        // The raise stays reachable in the loop (a no-match iteration must
        // raise exactly like the original build did).
        let loop_part = &out[out.find("while true do").unwrap()..];
        assert!(loop_part.contains("error(\"boom\")"), "{out}");
    }

    #[test]
    fn per_iteration_capture_uses_fresh_locals() {
        // A thunk in the looping branch captures the intro local; the loop
        // must keep the capture on per-iteration locals (the copy and the
        // re-derived intro), never on the mutated parameter.
        let (out, _) = converted(countdown());
        let loop_part =
            &out[out.find("while true do").unwrap()..out.find("::continue::").unwrap()];
        assert!(loop_part.contains("local n = _w0"), "{out}");
        assert!(!loop_part.contains("local n = _arg0"), "{out}");
    }

    #[test]
    fn refutation_green_over_converted_output() {
        let mut stmts = countdown();
        let engine =
            Engine::run_structured(&mut stmts, &mut IoLoop).expect("conversion applied");
        assert!(engine.refute(&stmts, false).is_empty());
    }
}
