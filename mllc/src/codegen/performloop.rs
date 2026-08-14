//! Pass 7 — direct-perform IO self-loop conversion, the structured tier's
//! third pass (see opt.rs's pipeline comment and annot.rs's structured-tier
//! contract).
//!
//! The shape: a self-recursive IO/ST function that PERFORMS at call time —
//! no build/run split, so calling it runs the effects — and recurses through
//! the forwarding runner at the outer body's action tail:
//!
//! ```text
//! __mll_fn[3] = function(_arg0)
//!     local n = __force(_arg0)
//!     __mll_run_tail(__mll_fn[9](n))            -- effect, runs at call time
//!     if n == 0 then
//!         return nil                            -- terminal
//!     else
//!         return __mll_run_tail(__mll_fn[3](n - 1))   -- self site
//!     end
//! end
//! ```
//!
//! The self CALL sits in the runner's ARGUMENT position, so it is not a Lua
//! tail call: every step leaves one frame pinned mid-expression, and a
//! 1e6-deep run overflows the interpreter stack on PUC Lua and LuaJIT both
//! (verified; this is the correctness gap the ioloop pass's gates recorded —
//! its repeat-safe gate declines this shape BY DESIGN, because the effects
//! live in what ioloop would treat as build-time skeleton). The emitter also
//! spells the same recursion through an action-tail TREE: the outer tail is
//! `return __mll_run_tail((function() <dispatch> end)())` (a case/guard
//! dispatch IIFE) whose branches return either a plain action expression, a
//! zero-parameter action closure (whose body performs and recurses), or the
//! self call itself — sed's fn102 and basic's REPL loop are corpus instances.
//!
//! The conversion has two steps, both scoped to one function body and
//! applied only when every gate below holds:
//!
//! 1. NORMALIZE the action tail: dissolve the tree into the direct shape.
//!    `__mll_run_tail(<zero-arg call of a zero-parameter function literal>)`
//!    splices the literal's body in place of the return (the runner ran the
//!    IIFE right there; the splice runs the same statements at the same
//!    point), with each of the IIFE's `return e` rewritten to
//!    `return __mll_run_tail(e)` — the application the pending outer runner
//!    performed on exactly that value. `__mll_run_tail(<zero-parameter
//!    function literal>)` splices the literal's body verbatim: the runner's
//!    function arm is a bare `action()`, so running the closure IS running
//!    its statements at that point. Both splices recurse, and both are
//!    admitted only where control cannot fall out of the spliced statements
//!    into following code: the body diverges (`opt::block_diverges`), or the
//!    return sits in function-tail position so a fall-off reaches the loop
//!    end's bare `return` — reproducing the original's nil-action fall-off
//!    through the runner's normalization (nil vs zero values is
//!    indistinguishable at every consumer, which is a runner argument).
//! 2. LOOP: tailloop mechanics on the normalized body. `while true do` with
//!    per-iteration parameter copies (`local _w0 = p0; …`, fresh locals per
//!    iteration so surviving closures and thunks capture their own
//!    iteration's values — recursion's semantics), the body renamed p→w, and
//!    every `return __mll_run_tail(<self>(e…))` becoming the simultaneous
//!    multiple assignment `p… = e…'` plus `goto continue` to the label in
//!    end-of-block position. Iteration n of the loop runs exactly what call
//!    n of the original ran — same statements, same order, ONCE — so unlike
//!    ioloop there is no repeated work and no repeat-safe vocabulary: any
//!    body statement is admissible.
//!
//! THE RUN-TAIL-IDEMPOTENCE ARGUMENT. The original applies the forwarding
//! runner once per recursion level: at depth k the function's result is
//! `__mll_run_tail`^k applied to the innermost terminal's value. The loop
//! applies each level's GENUINE application (the splice rewrites and the
//! kept `__mll_run_tail(…)` returns) and drops the k−1 pending outer ones.
//! That is sound because `__mll_run_tail` is idempotent: its result is
//! always either a `__mll_pure` box (returned unchanged by the box arms) or
//! a bare non-thunk non-function value (the box convention — an action's
//! result that IS a function or thunk is boxed by gen_pure_action precisely
//! so no runner calls or forces it), and on both of those a further
//! application returns the argument unchanged with no effects. So every
//! pending outer application the loop drops was the identity.
//!
//! For the loop's own terminals the same fact is needed in the other
//! direction — a terminal reached on iteration n ≥ 2 had n−1 pending
//! applications in the original, a terminal on iteration 1 had none, and
//! the loop emits ONE spelling for both. The pass therefore keeps a
//! terminal verbatim only when `__mll_run_tail` is provably the identity on
//! it, making every depth agree byte-for-byte:
//!
//! * `return __mll_run_tail(…)` — already a runner result; idempotence.
//! * a literal, a fresh table constructor, or a `__mll_pure(…)` box — none
//!   is a thunk or a function, and the box arm returns boxes unchanged, so
//!   the runner passes each through untouched with no effects.
//! * a PURE-SUSPENSION closure — `function() return <e> end`, zero
//!   parameters, exactly one bare return — kept verbatim as a terminal (its
//!   body is NOT walked; a self mention inside re-enters the converted
//!   function), except that an identity-terminal payload needs no
//!   protection and returns bare. This is gen_pure_action's closure
//!   protection for a `pure` payload that may be a thunk: the consumer's
//!   runner calls it once and hands the payload over UNFORCED, which is
//!   exactly what the depth-0 original did. The depth ≥ 1 original differed: its pending outer
//!   re-applications forced a thunk payload, so `r <- f 1 undefined` with
//!   `f 1 a = f 0 a; f 0 a = pure a` raised where GHC binds the bottom
//!   unforced (confirmed against runghc 9.14.1 before this was written, and
//!   pinned as an executed case, performloop_pure_bottom). That forcing is a
//!   deviation OF THE UNCONVERTED SHAPE from GHC — the same runner
//!   re-application that pins the frames — and the loop, which applies each
//!   genuine runner application exactly once, restores GHC's behavior at
//!   every depth. The remaining deviation on shapes this pass declines is
//!   recorded in doc/articles/TODO.md.
//!
//! Any other terminal (a bare name, an unknown call, a multi-statement
//! closure — the last is an action BUILDER's branch, ioloop's territory)
//! declines the whole conversion: converting it would need a claim about
//! the value the tree cannot prove. Inside a spliced IIFE no such judgment
//! is needed — every plain return gets the wrap the original applied, which
//! is faithful for ANY value.
//!
//! Other decisions, shared with tailloop/ioloop and reused from there: the
//! three header spellings and the self-identity proof (single-store census
//! for `__mll_fn` slots, binding-scope single-assignment plus lexical
//! visibility for names), the scoped p→w rename with its Raw/composite
//! blocking, argument order and simultaneity of the update, the locals
//! budget, and the zero-parameter exception (no assignment exists to carry
//! extra arguments' evaluation; such a site stays a kept runner return,
//! which is sound either way). Non-tail self uses (`__mll_run(self(…))`
//! binds, discarded `__mll_run_tail(self(…))` statements, first-class
//! captures) stay verbatim: they re-enter the converted function, whose
//! external contract — perform on call, return a runner-normalized result —
//! is preserved. A body already holding loop scaffolding (`while true`,
//! goto/label, a bare return, a multiple assignment) or a Raw statement
//! whose text mentions `return` is declined whole: those shapes only arise
//! from other passes' output or hand-shaped trees, and this pass's return
//! accounting cannot see into them.
//!
//! Mechanical review is permanent, same pattern as ioloop: a tree-level
//! reverse transformer runs as a debug_assert inside `convert` — every
//! debug/test compile un-converts step 2 (strip the scaffold, restore the
//! sites, rename back) and byte-compares the result against the normalized
//! body. Step 1's splices dissolve closure boundaries the output no longer
//! records, so they are not blindly invertible; each splice is instead a
//! small local rewrite justified above (the iife pass's class), and the
//! normalized tree the assert pins is exactly step 2's input.

use std::collections::{HashMap, HashSet};

use super::annot::{self, ScopeView};
use super::loopcore::{self, each_return, run_tail_arg, run_tail_self_args};
use super::lua::{Block, Expr, FuncBody, Stmt};
use super::opt;
use super::tailloop::{
    self, SelfName, parse_header, rename_block, rename_blocked, self_qualifies,
};

pub(super) struct PerformLoop;

impl annot::StructuredPass for PerformLoop {
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

// (`run_tail_arg` — the exact-spelling runner matcher — is loopcore's,
// shared with ioloop; exactness keeps the reverse transform an inverse.)

fn run_tail_arg_owned(e: Expr) -> Result<Expr, Expr> {
    if run_tail_arg(&e).is_some() {
        let Expr::Call(_, mut args) = e else { unreachable!() };
        Ok(args.pop().expect("runner argument"))
    } else {
        Err(e)
    }
}

/// A zero-parameter function literal (grouping parens peeled), as an owned
/// statement list. A literal WITH parameters is a function value, not an
/// action, and is not accepted.
fn closure_body_owned(e: Expr) -> Result<Vec<Stmt>, Expr> {
    let mut peeled = &e;
    while let Expr::Paren(inner) = peeled {
        peeled = inner;
    }
    if !matches!(peeled, Expr::Func(params, _) if params.is_empty()) {
        return Err(e);
    }
    let mut e = e;
    loop {
        match e {
            Expr::Paren(inner) => e = *inner,
            Expr::Func(_, FuncBody::Inline(s)) => return Ok(s),
            Expr::Func(_, FuncBody::Block(Block(s))) => return Ok(s),
            _ => unreachable!(),
        }
    }
}

/// A zero-argument call of a zero-parameter function literal — the dispatch
/// IIFE `(function() … end)()` — as an owned statement list.
fn iife_body_owned(e: Expr) -> Result<Vec<Stmt>, Expr> {
    let is_iife = matches!(&e, Expr::Call(f, args) if args.is_empty() && {
        let mut f = f.as_ref();
        while let Expr::Paren(inner) = f {
            f = inner;
        }
        matches!(f, Expr::Func(params, _) if params.is_empty())
    });
    if !is_iife {
        return Err(e);
    }
    let Expr::Call(f, _) = e else { unreachable!() };
    closure_body_owned(*f).map_err(|_| unreachable!())
}

/// A terminal `__mll_run_tail` is provably the identity on: a literal, a
/// fresh table constructor, a `__mll_pure(…)` box (see the module comment).
fn identity_terminal(e: &Expr) -> bool {
    match e {
        Expr::Lit(_) | Expr::Table(_) | Expr::TableSpaced(_) => true,
        Expr::Call(f, args) => {
            matches!(f.as_ref(), Expr::Name(n) if n == "__mll_pure") && args.len() == 1
        }
        _ => false,
    }
}

/// gen_pure_action's closure protection for a possibly-thunk `pure`
/// payload: a zero-parameter literal whose whole body is one bare return
/// (see the module comment); yields the payload. A `__mll_run_tail(…)`
/// return is an action TAIL, not a suspended payload, and takes the splice
/// path instead.
fn pure_suspension_payload(e: &Expr) -> Option<&Expr> {
    let mut e = e;
    while let Expr::Paren(inner) = e {
        e = inner;
    }
    let Expr::Func(params, body) = e else { return None };
    if !params.is_empty() {
        return None;
    }
    let stmts = match body {
        FuncBody::Inline(s) => s,
        FuncBody::Block(Block(s)) => s,
    };
    match stmts.as_slice() {
        [Stmt::Return(inner)] if run_tail_arg(inner).is_none() => Some(inner),
        _ => None,
    }
}

/// The terminal statement for a pure-suspension closure: an
/// identity-terminal payload cannot be a thunk or a function, so the
/// protection is not needed and the payload returns bare (the consumer's
/// one unbox treats `__mll_pure(x)` identically either way); any other
/// payload keeps the closure, so it crosses to the consumer unforced.
fn suspension_terminal(e: Expr) -> Stmt {
    match pure_suspension_payload(&e) {
        Some(p) if identity_terminal(p) => Stmt::Return(p.clone()),
        _ => Stmt::Return(e),
    }
}

// ---- Step 1: normalization ----

/// Normalize one block. `tail` — this block's end is the function body's
/// end (a fall-off here reaches the loop's bare-return guard). `action` —
/// this block is a spliced dispatch IIFE's body, so its returns feed the
/// pending runner application (wrap them all); otherwise returns leave the
/// function (identity-terminal vocabulary applies). `None` declines the
/// whole conversion.
fn normalize_block(stmts: Vec<Stmt>, tail: bool, action: bool) -> Option<Vec<Stmt>> {
    let n = stmts.len();
    let mut out = Vec::with_capacity(n);
    for (i, s) in stmts.into_iter().enumerate() {
        let stail = tail && i + 1 == n;
        match s {
            Stmt::Return(e) => {
                // Lua's grammar makes a return the last statement of its
                // block; anything after one is outside the vocabulary.
                if i + 1 != n {
                    return None;
                }
                out.extend(normalize_return(e, stail, action)?);
            }
            Stmt::If { cond, then_b, elseifs, else_b } => out.push(Stmt::If {
                cond,
                then_b: Block(normalize_block(then_b.0, stail, action)?),
                elseifs: elseifs
                    .into_iter()
                    .map(|(c, b)| normalize_block(b.0, stail, action).map(|r| (c, Block(r))))
                    .collect::<Option<Vec<_>>>()?,
                else_b: match else_b {
                    Some(b) => Some(Block(normalize_block(b.0, stail, action)?)),
                    None => None,
                },
            }),
            Stmt::Do(b) => out.push(Stmt::Do(Block(normalize_block(b.0, stail, action)?))),
            // Loop scaffolding and multi-assignments only arise from other
            // passes' output; a Raw statement spelling `return` hides a
            // return this accounting cannot see. Decline whole.
            Stmt::WhileTrue(_)
            | Stmt::Goto(_)
            | Stmt::Label(_)
            | Stmt::ReturnNone
            | Stmt::ReturnTable(_)
            | Stmt::MultiAssign(..) => return None,
            Stmt::Raw(t) => {
                let mut toks = HashSet::new();
                opt::token_set(&t, &mut toks);
                if toks.contains("return") {
                    return None;
                }
                out.push(Stmt::Raw(t));
            }
            // Everything else — effect statements, binds, forces, nested
            // function definitions — runs once per call in the original and
            // once per iteration in the loop: kept verbatim.
            other => out.push(other),
        }
    }
    Some(out)
}

fn normalize_return(e: Expr, tail: bool, action: bool) -> Option<Vec<Stmt>> {
    match run_tail_arg_owned(e) {
        // `return __mll_run_tail(x)`: the same pending-application position
        // at both levels (at action level the extra outer application is the
        // identity by idempotence).
        Ok(x) => normalize_action_value(x, tail),
        Err(e) => {
            if action {
                // A dispatch IIFE's plain return: apply the wrap the pending
                // runner performed on exactly this value — faithful for any
                // value, so no vocabulary judgment is needed here.
                normalize_action_value(e, tail)
            } else if identity_terminal(&e) {
                Some(vec![Stmt::Return(e)])
            } else if pure_suspension_payload(&e).is_some() {
                Some(vec![suspension_terminal(e)])
            } else {
                None
            }
        }
    }
}

/// `x` stands where a `__mll_run_tail` application is pending.
fn normalize_action_value(x: Expr, tail: bool) -> Option<Vec<Stmt>> {
    // A nested runner call: the outer pending application is the identity
    // (idempotence); recurse on the inner argument.
    let x = match run_tail_arg_owned(x) {
        Ok(inner) => return normalize_action_value(inner, tail),
        Err(x) => x,
    };
    // A pure-suspension closure: a terminal — the consumer's runner
    // performs the one call the pending application would have (see the
    // module comment; the dropped re-applications forced thunk payloads,
    // which GHC does not).
    if pure_suspension_payload(&x).is_some() {
        return Some(vec![suspension_terminal(x)]);
    }
    // The dispatch IIFE: splice; its returns feed the pending application.
    let x = match iife_body_owned(x) {
        Ok(body) => {
            if !tail && !opt::block_diverges(&Block(body.clone())) {
                return None;
            }
            return normalize_block(body, tail, true);
        }
        Err(x) => x,
    };
    // The action closure: the runner's function arm ran it right here;
    // splice its statements verbatim. Its returns leave the function.
    let x = match closure_body_owned(x) {
        Ok(body) => {
            if !tail && !opt::block_diverges(&Block(body.clone())) {
                return None;
            }
            return normalize_block(body, tail, false);
        }
        Err(x) => x,
    };
    // Anything else — the self call included — keeps the application:
    // `return __mll_run_tail(x)`. Step 2 rewrites the self sites.
    Some(vec![Stmt::Return(Expr::call_named("__mll_run_tail", vec![x]))])
}

// ---- Step 2: the loop ----

// (`run_tail_self_args` — a rewritable site of the NORMALIZED body — and
// the `rewrite_run_tail_sites`/`unrewrite_run_tail_sites` pair are
// loopcore's, shared with ioloop. The zero-parameter exception is
// tailloop's: with no parameters there is no assignment to carry extra
// arguments' evaluation, and the kept runner return is sound either way.)

/// Is there at least one site anywhere in the normalized body? (Returns are
/// block-final, so every block position is visited.)
fn has_site(stmts: &[Stmt], name: &SelfName, params: &[String]) -> bool {
    let mut found = false;
    each_return(stmts, &mut |e| {
        found = found || run_tail_self_args(e, name, params).is_some();
    });
    found
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
    let normalized = normalize_block(body.0.clone(), true, false)?;
    if !has_site(&normalized, &self_name, &params) {
        return None;
    }
    let param_set: HashSet<String> = params.iter().cloned().collect();
    if rename_blocked(&normalized, &param_set) {
        return None;
    }

    // Fresh per-iteration copy names, against every identifier token of the
    // rendered function (the normalized body holds every token the splices
    // moved in).
    let used = tailloop::used_tokens(header, &normalized);
    let ws = tailloop::fresh_with_prefix(&used, "_w", params.len());

    let map: HashMap<String, String> =
        params.iter().cloned().zip(ws.iter().cloned()).collect();
    let mut loop_stmts = normalized.clone();
    rename_block(&mut loop_stmts, &map);
    // Every site (`everywhere: true`): after splicing, sites sit at any
    // block position, and none is in loop-body tail position in general —
    // always the goto shape.
    loopcore::rewrite_run_tail_sites(&mut loop_stmts, &self_name, &params, true);

    let inner = loopcore::build_loop_scaffold(&ws, &params, loop_stmts);

    // Locals budget: the loop body holds the copies plus every spliced
    // body's locals in one scope; the parameters themselves occupy slots the
    // statement-level count cannot see.
    if opt::count_locals(&inner) + params.len() > super::CodeGen::LOCAL_LIMIT {
        return None;
    }

    let out = vec![Stmt::WhileTrue(Block(inner))];

    // Self-check (debug/test builds): step 2 must be exactly reversible —
    // un-converting the loop must reproduce the normalized body; `loopcore::
    // reverse_check` does the byte-compare and the mismatch diagnostics.
    // Step 1's splices are local rewrites whose output IS that normalized
    // body (see the module comment).
    debug_assert!(
        loopcore::reverse_check(
            "performloop",
            "normalized",
            header,
            reverse(&out, &self_name, &params, &ws),
            &normalized,
        ),
        "performloop conversion is not reversible (see stderr)"
    );

    Some(Block(out))
}

// ---- The reverse transform (self-check; see convert) ----

/// Un-convert step 2: recover the normalized body from the loop alone.
/// Returns `None` when the converted tree does not have the exact produced
/// shape — the self-check then fails loudly.
fn reverse(
    converted: &[Stmt],
    name: &SelfName,
    params: &[String],
    ws: &[String],
) -> Option<Vec<Stmt>> {
    let [Stmt::WhileTrue(Block(inner))] = converted else { return None };
    let rest = loopcore::peel_loop_scaffold(inner, ws, params)?;
    let mut stmts = rest.to_vec();
    loopcore::unrewrite_run_tail_sites(&mut stmts, name, params, true)?;
    let unmap: HashMap<String, String> =
        ws.iter().cloned().zip(params.iter().cloned()).collect();
    rename_block(&mut stmts, &unmap);
    Some(stmts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::annot::Engine;

    fn converted(mut stmts: Vec<Stmt>) -> (String, bool) {
        let rewrote = Engine::run_structured(&mut stmts, &mut PerformLoop).is_some();
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

    fn iife(stmts: Vec<Stmt>) -> Expr {
        Expr::call(Expr::paren(closure(stmts)), vec![])
    }

    /// The direct shape: force at call, effect statement, `return nil`
    /// terminal, tail self site through the runner.
    fn countdown() -> Vec<Stmt> {
        vec![
            Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
            Stmt::Function {
                header: "__mll_fn[3] = function(_arg0)".into(),
                body: Block(vec![
                    Stmt::Local(vec!["n".into()], Some(Expr::force(Expr::name("_arg0")))),
                    Stmt::Expr(run_tail(Expr::call_named(
                        "__mll_fn[9]",
                        vec![Expr::name("n")],
                    ))),
                    Stmt::If {
                        cond: Expr::binop("==", Expr::name("n"), Expr::lit("0")),
                        then_b: Block(vec![Stmt::Return(Expr::lit("nil"))]),
                        elseifs: vec![],
                        else_b: Some(Block(vec![Stmt::Return(run_tail(Expr::call_named(
                            "__mll_fn[3]",
                            vec![Expr::binop("-", Expr::name("n"), Expr::lit("1"))],
                        )))])),
                    },
                ]),
            },
        ]
    }

    #[test]
    fn direct_shape_converts() {
        let (out, rewrote) = converted(countdown());
        assert!(rewrote, "{out}");
        assert!(out.contains("while true do"), "{out}");
        // Per-iteration copy, body renamed onto it.
        assert!(out.contains("local _w0 = _arg0"), "{out}");
        assert!(out.contains("local n = __force(_w0)"), "{out}");
        // The effect statement runs inside the loop, once per iteration.
        assert!(out.contains("__mll_run_tail(__mll_fn[9](n))"), "{out}");
        // The terminal stays verbatim; the site became update + goto.
        assert!(out.contains("return nil"), "{out}");
        assert!(out.contains("_arg0 = n - 1"), "{out}");
        assert!(out.contains("goto continue"), "{out}");
        assert!(out.contains("::continue::"), "{out}");
        // No recursion remains.
        assert!(!out.contains("__mll_run_tail(__mll_fn[3]"), "{out}");
    }

    /// The dispatch shape: `return __mll_run_tail((function() … end)())`
    /// whose IIFE branches return a plain action expression (gets the wrap),
    /// an action closure with an effect and the tail self site (spliced),
    /// and the bare self call (wrap makes it a site).
    fn dispatch() -> Vec<Stmt> {
        vec![
            Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
            Stmt::Function {
                header: "__mll_fn[5] = function(_arg0)".into(),
                body: Block(vec![
                    Stmt::Local(vec!["n".into()], Some(Expr::force(Expr::name("_arg0")))),
                    Stmt::Return(run_tail(iife(vec![Stmt::If {
                        cond: Expr::binop("==", Expr::name("n"), Expr::lit("0")),
                        then_b: Block(vec![Stmt::Return(Expr::call_named(
                            "__mll_fn[1]",
                            vec![Expr::lit("\"done\"")],
                        ))]),
                        elseifs: vec![(
                            Expr::binop("==", Expr::name("n"), Expr::lit("1")),
                            Block(vec![Stmt::Return(Expr::call_named(
                                "__mll_fn[5]",
                                vec![Expr::lit("0")],
                            ))]),
                        )],
                        else_b: Some(Block(vec![Stmt::Return(closure(vec![
                            Stmt::Expr(run_tail(Expr::call_named(
                                "__mll_fn[1]",
                                vec![Expr::name("n")],
                            ))),
                            Stmt::Return(run_tail(Expr::call_named(
                                "__mll_fn[5]",
                                vec![Expr::binop("-", Expr::name("n"), Expr::lit("1"))],
                            ))),
                        ]))])),
                    }]))),
                ]),
            },
        ]
    }

    #[test]
    fn dispatch_shape_converts() {
        let (out, rewrote) = converted(dispatch());
        assert!(rewrote, "{out}");
        // The IIFE dissolved: no function literal remains in the loop.
        assert!(!out.contains("function()"), "{out}");
        // The plain branch got the pending runner's wrap, reading the copy.
        assert!(out.contains("return __mll_run_tail(__mll_fn[1](\"done\"))"), "{out}");
        // The bare self-call branch became a site.
        assert!(out.contains("_arg0 = 0"), "{out}");
        // The spliced closure kept its effect and its site became the update.
        assert!(out.contains("__mll_run_tail(__mll_fn[1](n))"), "{out}");
        assert!(out.contains("_arg0 = n - 1"), "{out}");
        assert_eq!(out.matches("goto continue").count(), 2, "{out}");
    }

    #[test]
    fn nested_dispatch_converts() {
        // closure → runner(IIFE) → self site: the fn200 nesting.
        let stmts = vec![
            Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
            Stmt::Function {
                header: "__mll_fn[7] = function(_arg0)".into(),
                body: Block(vec![Stmt::If {
                    cond: Expr::name("_arg0"),
                    then_b: Block(vec![Stmt::Return(Expr::lit("nil"))]),
                    elseifs: vec![],
                    else_b: Some(Block(vec![Stmt::Return(run_tail(iife(vec![
                        Stmt::Return(closure(vec![
                            Stmt::Local(
                                vec!["res".into()],
                                Some(Expr::call_named(
                                    "__mll_run",
                                    vec![Expr::call_named(
                                        "__mll_fn[8]",
                                        vec![Expr::name("_arg0")],
                                    )],
                                )),
                            ),
                            Stmt::Return(run_tail(iife(vec![Stmt::If {
                                cond: Expr::name("res"),
                                then_b: Block(vec![Stmt::Return(Expr::call_named(
                                    "__mll_fn[7]",
                                    vec![Expr::name("res")],
                                ))]),
                                elseifs: vec![],
                                else_b: Some(Block(vec![Stmt::Return(closure(vec![
                                    Stmt::Return(Expr::lit("nil")),
                                ]))])),
                            }]))),
                        ])),
                    ])))])),
                }]),
            },
        ];
        let (out, rewrote) = converted(stmts);
        assert!(rewrote, "{out}");
        // All three nesting levels dissolved; the bind stays per iteration.
        assert!(out.contains("local res = __mll_run(__mll_fn[8](_w0))"), "{out}");
        assert!(out.contains("_arg0 = res"), "{out}");
        assert!(out.contains("return nil"), "{out}");
        assert!(!out.contains("function()"), "{out}");
    }

    #[test]
    fn action_builder_terminal_declines() {
        // A multi-statement closure at result level is an action BUILDER's
        // branch (ioloop's territory): outside the terminal vocabulary.
        let stmts = vec![
            Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
            Stmt::Function {
                header: "__mll_fn[3] = function(_arg0)".into(),
                body: Block(vec![Stmt::If {
                    cond: Expr::name("_arg0"),
                    then_b: Block(vec![Stmt::Return(closure(vec![
                        Stmt::Expr(run_tail(Expr::call_named("__mll_fn[9]", vec![]))),
                        Stmt::Return(Expr::lit("nil")),
                    ]))]),
                    elseifs: vec![],
                    else_b: Some(Block(vec![Stmt::Return(run_tail(Expr::call_named(
                        "__mll_fn[3]",
                        vec![Expr::name("_arg0")],
                    )))])),
                }]),
            },
        ];
        let (out, rewrote) = converted(stmts);
        assert!(!rewrote, "{out}");
    }

    #[test]
    fn pure_suspension_terminal_kept_verbatim() {
        // The `pure acc` protection closure — `function() return acc end` —
        // is a terminal at both levels: kept verbatim (never spliced, never
        // wrapped), so the payload crosses to the consumer unforced.
        let stmts = vec![
            Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
            Stmt::Function {
                header: "__mll_fn[3] = function(_arg0, _arg1)".into(),
                body: Block(vec![
                    Stmt::Local(vec!["acc".into()], Some(Expr::name("_arg1"))),
                    Stmt::Return(run_tail(iife(vec![Stmt::If {
                        cond: Expr::name("_arg0"),
                        then_b: Block(vec![Stmt::Return(closure(vec![Stmt::Return(
                            Expr::name("acc"),
                        )]))]),
                        elseifs: vec![],
                        else_b: Some(Block(vec![Stmt::Return(Expr::call_named(
                            "__mll_fn[3]",
                            vec![Expr::lit("true"), Expr::name("acc")],
                        ))])),
                    }]))),
                ]),
            },
        ];
        let (out, rewrote) = converted(stmts);
        assert!(rewrote, "{out}");
        // The protection closure survives (its payload could be a thunk)…
        assert!(out.contains("return function()"), "{out}");
        assert!(out.contains("return acc"), "{out}");
        // …and never gets a runner wrap, which would force the payload.
        assert!(!out.contains("__mll_run_tail(function"), "{out}");
        assert!(out.contains("_arg0, _arg1 = true, acc"), "{out}");
    }

    #[test]
    fn identity_payload_suspension_unwraps() {
        // `function() return nil end`: the payload cannot be a thunk, so
        // the protection is dropped and the literal returns bare.
        let stmts = vec![
            Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
            Stmt::Function {
                header: "__mll_fn[3] = function(_arg0)".into(),
                body: Block(vec![Stmt::Return(run_tail(iife(vec![Stmt::If {
                    cond: Expr::name("_arg0"),
                    then_b: Block(vec![Stmt::Return(closure(vec![Stmt::Return(
                        Expr::lit("nil"),
                    )]))]),
                    elseifs: vec![],
                    else_b: Some(Block(vec![Stmt::Return(Expr::call_named(
                        "__mll_fn[3]",
                        vec![Expr::lit("false")],
                    ))])),
                }])))]),
            },
        ];
        let (out, rewrote) = converted(stmts);
        assert!(rewrote, "{out}");
        assert!(out.contains("return nil"), "{out}");
        assert!(!out.contains("function()"), "{out}");
    }

    #[test]
    fn unknown_terminal_declines() {
        // `return x` — a bare name the tree cannot prove the runner passes
        // through unchanged.
        let mut stmts = countdown();
        let Stmt::Function { body, .. } = &mut stmts[1] else { panic!("shape") };
        let Stmt::If { then_b, .. } = &mut body.0[2] else { panic!("shape") };
        then_b.0[0] = Stmt::Return(Expr::name("n"));
        let (out, rewrote) = converted(stmts);
        assert!(!rewrote, "{out}");
    }

    #[test]
    fn pure_box_and_run_tail_terminals_convert() {
        let stmts = vec![
            Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
            Stmt::Function {
                header: "__mll_fn[3] = function(_arg0, _arg1)".into(),
                body: Block(vec![Stmt::If {
                    cond: Expr::name("_arg0"),
                    then_b: Block(vec![Stmt::Return(Expr::call_named(
                        "__mll_pure",
                        vec![Expr::name("_arg1")],
                    ))]),
                    elseifs: vec![(
                        Expr::name("_arg1"),
                        Block(vec![Stmt::Return(run_tail(Expr::call_named(
                            "__mll_fn[4]",
                            vec![Expr::name("_arg0")],
                        )))]),
                    )],
                    else_b: Some(Block(vec![Stmt::Return(run_tail(Expr::call_named(
                        "__mll_fn[3]",
                        vec![Expr::name("_arg1"), Expr::name("_arg0")],
                    )))])),
                }]),
            },
        ];
        let (out, rewrote) = converted(stmts);
        assert!(rewrote, "{out}");
        // The box terminal and the other-function forward stay verbatim
        // (renamed onto the copies); the swap update is simultaneous.
        assert!(out.contains("return __mll_pure(_w1)"), "{out}");
        assert!(out.contains("return __mll_run_tail(__mll_fn[4](_w0))"), "{out}");
        assert!(out.contains("_arg0, _arg1 = _w1, _w0"), "{out}");
    }

    #[test]
    fn non_tail_site_jumps_past_following_raise() {
        // A site in a non-tail if (the raise follows): the update must jump
        // to the continue label, and the raise must stay reachable.
        let stmts = vec![
            Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
            Stmt::Function {
                header: "__mll_fn[3] = function(_arg0)".into(),
                body: Block(vec![
                    Stmt::If {
                        cond: Expr::name("_arg0"),
                        then_b: Block(vec![Stmt::Return(run_tail(Expr::call_named(
                            "__mll_fn[3]",
                            vec![Expr::lit("1")],
                        )))]),
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
        let loop_part = &out[out.find("while true do").unwrap()..];
        assert!(loop_part.contains("error(\"boom\")"), "{out}");
    }

    #[test]
    fn non_tail_non_diverging_splice_declines() {
        // A dispatch IIFE that can fall off its end, at a NON-tail return:
        // splicing would fall into the raise that follows. Must decline.
        let stmts = vec![
            Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
            Stmt::Function {
                header: "__mll_fn[3] = function(_arg0)".into(),
                body: Block(vec![
                    Stmt::If {
                        cond: Expr::name("_arg0"),
                        then_b: Block(vec![Stmt::Return(run_tail(iife(vec![Stmt::If {
                            cond: Expr::name("_arg0"),
                            then_b: Block(vec![Stmt::Return(Expr::call_named(
                                "__mll_fn[3]",
                                vec![Expr::lit("1")],
                            ))]),
                            elseifs: vec![],
                            else_b: None,
                        }])))]),
                        elseifs: vec![],
                        else_b: None,
                    },
                    Stmt::Expr(Expr::call_named("error", vec![Expr::lit("\"boom\"")])),
                ]),
            },
        ];
        let (out, rewrote) = converted(stmts);
        assert!(!rewrote, "{out}");
    }

    #[test]
    fn tail_non_diverging_splice_converts() {
        // The same falling-off IIFE at the function's tail: a fall-off
        // reaches the loop's bare-return guard, reproducing the original's
        // nil-action fall-off. Converts.
        let stmts = vec![
            Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
            Stmt::Function {
                header: "__mll_fn[3] = function(_arg0)".into(),
                body: Block(vec![Stmt::Return(run_tail(iife(vec![Stmt::If {
                    cond: Expr::name("_arg0"),
                    then_b: Block(vec![Stmt::Return(Expr::call_named(
                        "__mll_fn[3]",
                        vec![Expr::lit("1")],
                    ))]),
                    elseifs: vec![],
                    else_b: None,
                }])))]),
            },
        ];
        let (out, rewrote) = converted(stmts);
        assert!(rewrote, "{out}");
        assert!(out.contains("do\n            return\n        end"), "{out}");
        assert!(out.contains("::continue::"), "{out}");
    }

    #[test]
    fn zero_param_site_is_bare_goto() {
        let stmts = vec![
            Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
            Stmt::Function {
                header: "__mll_fn[3] = function()".into(),
                body: Block(vec![
                    Stmt::Expr(run_tail(Expr::call_named("__mll_fn[9]", vec![]))),
                    Stmt::Return(run_tail(Expr::call_named("__mll_fn[3]", vec![]))),
                ]),
            },
        ];
        let (out, rewrote) = converted(stmts);
        assert!(rewrote, "{out}");
        assert!(out.contains("goto continue"), "{out}");
        let loop_part =
            &out[out.find("while true do").unwrap()..out.find("::continue::").unwrap()];
        assert!(
            !loop_part.contains(" = "),
            "no update for a zero-parameter loop: {out}"
        );
    }

    #[test]
    fn non_tail_self_run_stays() {
        // A bind `__mll_run(self(…))` re-enters the converted function: it
        // must survive verbatim.
        let mut stmts = countdown();
        let Stmt::Function { body, .. } = &mut stmts[1] else { panic!("shape") };
        body.0.insert(
            1,
            Stmt::Local(
                vec!["x".into()],
                Some(Expr::call_named(
                    "__mll_run",
                    vec![Expr::call_named("__mll_fn[3]", vec![Expr::lit("1")])],
                )),
            ),
        );
        let (out, rewrote) = converted(stmts);
        assert!(rewrote, "{out}");
        assert!(out.contains("local x = __mll_run(__mll_fn[3](1))"), "{out}");
    }

    #[test]
    fn raw_mention_of_parameter_declines() {
        let mut stmts = countdown();
        let Stmt::Function { body, .. } = &mut stmts[1] else { panic!("shape") };
        body.0.insert(0, Stmt::Raw("_arg0 = nil".into()));
        let (_, rewrote) = converted(stmts);
        assert!(!rewrote);
    }

    #[test]
    fn raw_return_declines() {
        let mut stmts = countdown();
        let Stmt::Function { body, .. } = &mut stmts[1] else { panic!("shape") };
        body.0.insert(0, Stmt::Raw("if host() then return host2() end".into()));
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
    fn converted_two_level_body_declines() {
        // The shape ioloop's conversion leaves behind: `return _lp`
        // skeleton returns. The bare name is outside the identity-terminal
        // vocabulary, so this pass never touches an ioloop result.
        let stmts = vec![
            Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
            Stmt::Function {
                header: "__mll_fn[3] = function(_arg0)".into(),
                body: Block(vec![
                    Stmt::Local(vec!["_lp".into()], Some(closure(vec![Stmt::Return(
                        Expr::lit("nil"),
                    )]))),
                    Stmt::If {
                        cond: Expr::name("_arg0"),
                        then_b: Block(vec![Stmt::Return(Expr::name("_lp"))]),
                        elseifs: vec![],
                        else_b: Some(Block(vec![Stmt::Return(run_tail(Expr::call_named(
                            "__mll_fn[3]",
                            vec![Expr::name("_arg0")],
                        )))])),
                    },
                ]),
            },
        ];
        let (out, rewrote) = converted(stmts);
        assert!(!rewrote, "{out}");
    }

    #[test]
    fn nested_runner_layers_collapse_to_one() {
        // `return __mll_run_tail(__mll_run_tail(self(…)))`: the outer
        // application is the identity (idempotence), the inner one is the
        // site.
        let stmts = vec![
            Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
            Stmt::Function {
                header: "__mll_fn[3] = function(_arg0)".into(),
                body: Block(vec![Stmt::If {
                    cond: Expr::name("_arg0"),
                    then_b: Block(vec![Stmt::Return(Expr::lit("nil"))]),
                    elseifs: vec![],
                    else_b: Some(Block(vec![Stmt::Return(run_tail(run_tail(
                        Expr::call_named("__mll_fn[3]", vec![Expr::lit("1")]),
                    )))])),
                }]),
            },
        ];
        let (out, rewrote) = converted(stmts);
        assert!(rewrote, "{out}");
        assert!(out.contains("_arg0 = 1"), "{out}");
        assert!(!out.contains("__mll_run_tail(__mll_run_tail"), "{out}");
    }

    #[test]
    fn refutation_green_over_converted_output() {
        let mut stmts = dispatch();
        let engine =
            Engine::run_structured(&mut stmts, &mut PerformLoop).expect("conversion applied");
        assert!(engine.refute(&stmts, false).is_empty());
    }
}
