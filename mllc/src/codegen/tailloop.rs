//! Pass 5 — self-tail-call → loop conversion, the structured tier's first
//! pass (see opt.rs's pipeline comment and annot.rs's structured-tier
//! contract).
//!
//! A named function whose body contains `return <self>(e1..en)` in
//! statement-tree tail position — the last statement of the body, or the
//! last statement of an `if`/`elseif`/`else` arm (or `do` block) that is
//! itself in tail position — converts to
//!
//! ```text
//! function(p1..pn)
//!     while true do
//!         local w1 = p1 … local wn = pn
//!         <body, with every p_i renamed to w_i>
//!     end
//! end
//! ```
//!
//! where each tail self-call becomes the simultaneous parameter update
//! `p1, …, pn = e1', …, en'` (the arguments, renamed like the rest of the
//! body) and every other `return`/raise leaves the loop exactly as it left
//! the function before. The win is one interpreter dispatch (and, under
//! LuaJIT, a loop that traces where a tail-recursive call would not).
//!
//! Correctness decisions:
//!
//! * SIMULTANEITY — the update is ONE multiple assignment: Lua evaluates the
//!   entire RHS list before assigning any lvalue, so an update like the
//!   swap `go b a` cannot read an already-overwritten parameter. On top of
//!   that, the renamed RHS reads only the `w` copies — the parameters are
//!   read only by the per-iteration copy at the loop head — so there is no
//!   read-after-write pair even in principle.
//! * EVALUATION ORDER — the RHS keeps the call's argument order. Both a
//!   call's argument list and a multiple assignment's RHS evaluate
//!   left-to-right in every interpreter mata-ll targets (PUC Lua 5.4/5.5,
//!   LuaJIT); the reference manual leaves both orders implementation-
//!   defined, which is noted here rather than engineered around.
//! * PER-ITERATION LOCALS — recursion gives every call fresh parameter
//!   locals; a loop reuses them. Any closure built in the body that
//!   survives past the tail call (returned, stored, thunked — almost every
//!   `__thunk(function() … end)` here captures) would observe later
//!   iterations' mutations if it captured the parameters. So the loop's
//!   carried state lives in the real parameters, but the body only ever
//!   touches the per-iteration `w` copies: Lua creates fresh locals for
//!   every execution of a loop body, so a closure over `w_i` sees exactly
//!   the value of its own iteration — the recursion semantics. This is done
//!   UNCONDITIONALLY, not only when a capturing closure is detected: the
//!   copy is one register move per parameter per iteration, and the
//!   unconditional shape needs no closure-escape analysis that would have
//!   to be sound against Raw text.
//! * RENAME — the p→w rename is a real scoped walk over the AST: nested
//!   binders shadow (a `local p`, a function-literal parameter), field
//!   positions (index suffixes, method names, table keys) are never
//!   variable references and are left alone, and any mention of a parameter
//!   the walk cannot prove to be a plain variable occurrence — Raw text, a
//!   composite `Name` spelling, a composite lvalue, a nested `Function`
//!   header — blocks the whole conversion (`rename_blocked`).
//! * CONTROL FLOW — when the body diverges (every path returns or raises,
//!   `opt::block_diverges`), a tail update simply falls to the end of the
//!   loop body: the update site replaced a statement in tail position, so
//!   every enclosing level has nothing after it — the fall-through IS the
//!   next iteration, no goto needed. When the body can fall off its end
//!   (implicitly returning zero values), falling off must NOT loop, so the
//!   loop body ends with `do return end ::continue::` and every tail update
//!   jumps `goto continue` past the return. All supported interpreters
//!   (5.4, 5.5, LuaJIT — see CI's compat matrix) have goto, and the label
//!   sits in end-of-block position, the one place Lua exempts from the
//!   no-jump-into-a-local's-scope rule.
//! * SELF-IDENTITY — the callee must provably be this function:
//!   `local function f` needs `f` bound exactly once in its binding scope's
//!   whole subtree (the header itself); `f = function` needs exactly the
//!   forward declaration plus the header, with the forward `local f`
//!   lexically in scope at the definition; `__mll_fn[i] = function` needs
//!   the module-wide slot census to show exactly one store and no Raw
//!   mention. All three reuse the engine's ScopeFacts/SlotStores machinery
//!   (annot.rs), Raw-poisoned like every name fact.
//! * MULTI-VALUE — `return f(x)` propagates all of f's values, the update
//!   propagates none, so conversion requires the callee to provably return
//!   one value. For `__mll_fn[i]` callees that is the compiled-function
//!   single-return axiom `opt::single_return_callee` already trusts for
//!   paren shedding. For name-called functions the same argument is
//!   established directly on this function's own returns
//!   (`single_return_body`): every non-self return operand yields one value
//!   (self-call returns are covered by induction on call depth), so the
//!   loop's exit returns — the only values the converted function can
//!   produce — are single. A parenthesized self-call `return (f(x))`
//!   converts too: under the single-return proof the truncating paren is
//!   the identity.
//! * VARARGS / headers — the header is structured (`FnTarget` + params);
//!   `self_target` admits the three target forms with plain-identifier
//!   parameters only. Anything else — `...`, spill-slot `_v[i]` assignment
//!   targets, duplicate parameters — is skipped.
//!
//! Non-tail self-calls stay ordinary calls (they re-enter the converted
//! function from the top — same semantics); IO self-loops through
//! `__mll_run_tail(<self>(…))` are not self-calls at the callee position and
//! are left alone.

use std::collections::{HashMap, HashSet};

use super::annot::{self, ScopeView, is_plain_ident};
use super::lua::{Block, Expr, FnTarget, Stmt};
use super::opt;

pub(super) struct TailLoop;

impl annot::StructuredPass for TailLoop {
    fn request(
        &mut self,
        target: &FnTarget,
        params: &[String],
        body: &Block,
        view: &ScopeView<'_>,
        locals_in_scope: &HashSet<String>,
    ) -> Option<Block> {
        convert(target, params, body, view, locals_in_scope)
    }
}

/// The self-name a function's target binds, in the spelling body calls use.
/// Shared with the IO self-loop pass (ioloop.rs), which converts the same
/// three target forms.
pub(super) enum SelfName {
    /// `local function f(…)` — bound by the header itself.
    LocalFn(String),
    /// `f = function(…)` — a forward-declared local assigned here.
    Assigned(String),
    /// `__mll_fn[i] = function(…)` — a module-global slot; the string is
    /// the full `__mll_fn[i]` reference (calls spell it as one `Name`).
    Slot(String),
}

impl SelfName {
    pub(super) fn spelling(&self) -> &str {
        match self {
            SelfName::LocalFn(s) | SelfName::Assigned(s) | SelfName::Slot(s) => s,
        }
    }
}

/// The self-name of a `Stmt::Function`, from its structured target — with
/// the gates the former header parser enforced. A composite `Assigned`
/// lvalue (the `_v[i]` spill form), a non-identifier parameter (hand-built
/// `...` varargs), or a duplicate parameter returns `None` and the function
/// is skipped: duplicates would make both the rename and the multiple
/// assignment ambiguous (the emitter never produces them, but the gate
/// protects both).
pub(super) fn self_target(target: &FnTarget, params: &[String]) -> Option<SelfName> {
    let name = match target {
        FnTarget::LocalFn(n) if is_plain_ident(n) => SelfName::LocalFn(n.clone()),
        FnTarget::Assigned(n) if is_plain_ident(n) => SelfName::Assigned(n.clone()),
        FnTarget::Slot(i) => SelfName::Slot(format!("__mll_fn[{}]", i)),
        FnTarget::LocalFn(_) | FnTarget::Assigned(_) => return None,
    };
    if !params.iter().all(|p| is_plain_ident(p)) {
        return None;
    }
    let unique: HashSet<&String> = params.iter().collect();
    if unique.len() != params.len() {
        return None;
    }
    Some(name)
}

/// The self-identity gate (see the module comment).
pub(super) fn self_qualifies(
    name: &SelfName,
    view: &ScopeView<'_>,
    locals_in_scope: &HashSet<String>,
) -> bool {
    match name {
        // The header token is the single binding site; nothing else in the
        // scope subtree binds, assigns, or Raw-mentions the name.
        SelfName::LocalFn(f) => view.binding_sites(f) == (1, 0) && !view.raw_mentions(f),
        // Exactly two binding sites — the forward `local f` and this
        // header's token — no assignment statements, no Raw mention, and
        // the forward declaration is lexically visible here (so the header
        // assigns THAT local, not an enclosing scope's name).
        SelfName::Assigned(f) => {
            view.binding_sites(f) == (2, 0)
                && !view.raw_mentions(f)
                && locals_in_scope.contains(f)
        }
        SelfName::Slot(s) => view.slot_single_store(s),
    }
}

/// Does `e` (paren layers peeled) call the self-name? Returns the argument
/// list. The paren form `return (f(x))` truncates to one value, which the
/// single-return requirement makes the identity.
fn self_call_args<'a>(e: &'a Expr, name: &SelfName) -> Option<&'a Vec<Expr>> {
    let mut e = e;
    while let Expr::Paren(inner) = e {
        e = inner;
    }
    let Expr::Call(f, args) = e else { return None };
    if is_self_ref(f, name) { Some(args) } else { None }
}

fn is_self_ref(f: &Expr, name: &SelfName) -> bool {
    match f {
        Expr::Name(s) => s == name.spelling(),
        // Defensive: the `__mll_fn[i]` spelling as an Index node (the
        // emitter uses the Name spelling, but opt.rs already recognizes
        // both).
        Expr::Index(base, suffix) => {
            matches!(name, SelfName::Slot(s)
                if matches!(base.as_ref(), Expr::Name(b) if b == "__mll_fn")
                    && format!("__mll_fn{}", suffix) == *s)
        }
        _ => false,
    }
}

/// A rewritable tail site: a tail self-call whose update form is exact. A
/// multiple assignment evaluates its whole RHS like the call evaluated its
/// arguments — extras discarded, missing lvalues nil, both matching the
/// call's parameter adjustment — EXCEPT with zero parameters, where there is
/// no assignment to carry extra arguments' evaluation; such a call (which a
/// saturated emitter never produces) is left as a call.
fn rewritable_site<'a>(e: &'a Expr, name: &SelfName, params: &[String]) -> Option<&'a Vec<Expr>> {
    let args = self_call_args(e, name)?;
    if params.is_empty() && !args.is_empty() {
        return None;
    }
    Some(args)
}

/// Is there at least one tail self-call to rewrite? A dry-run twin of
/// `rewrite_tails` over the same positions.
fn has_tail_self_call(stmts: &[Stmt], name: &SelfName, params: &[String]) -> bool {
    tail_position_has(stmts, &|e| rewritable_site(e, name, params).is_some())
}

/// Does any statement-tree tail position — the last statement, or the last
/// statement of a tail `if`/`elseif`/`else` arm or `do` block — hold a
/// `return` whose operand satisfies `pred`? The dry-run twin of the loop
/// passes' tail rewrites, over the same positions (ioloop's spliced-body
/// gate uses it too).
pub(super) fn tail_position_has(stmts: &[Stmt], pred: &impl Fn(&Expr) -> bool) -> bool {
    let mut found = false;
    for_each_tail_block(stmts, &mut |b| {
        if let Some(Stmt::Return(e)) = b.last() {
            found = found || pred(e);
        }
    });
    found
}

/// The statement-tree TAIL BLOCKS of a statement list: the list itself when
/// its last statement is not a branch, otherwise (recursively) each arm of
/// a last `if`/`elseif`/`else` and the body of a last `do`. `f` sees every
/// innermost tail block — the places a function's own tail return can sit.
/// One walk for the four passes' predicates and rewrites (tail_position_has,
/// rewrite_tails, ioloop's rewrite/unrewrite of run-tail sites), which each
/// once spelled the same descent.
pub(super) fn for_each_tail_block(stmts: &[Stmt], f: &mut impl FnMut(&[Stmt])) {
    match stmts.last() {
        Some(Stmt::If { then_b, elseifs, else_b, .. }) => {
            for_each_tail_block(&then_b.0, f);
            for (_, b) in elseifs {
                for_each_tail_block(&b.0, f);
            }
            if let Some(b) = else_b {
                for_each_tail_block(&b.0, f);
            }
        }
        Some(Stmt::Do(b)) => for_each_tail_block(&b.0, f),
        _ => f(stmts),
    }
}

/// Mutable twin of [`for_each_tail_block`].
pub(super) fn for_each_tail_block_mut(stmts: &mut Vec<Stmt>, f: &mut impl FnMut(&mut Vec<Stmt>)) {
    match stmts.last_mut() {
        Some(Stmt::If { then_b, elseifs, else_b, .. }) => {
            for_each_tail_block_mut(&mut then_b.0, f);
            for (_, b) in elseifs.iter_mut() {
                for_each_tail_block_mut(&mut b.0, f);
            }
            if let Some(b) = else_b.as_mut() {
                for_each_tail_block_mut(&mut b.0, f);
            }
        }
        Some(Stmt::Do(b)) => for_each_tail_block_mut(&mut b.0, f),
        _ => f(stmts),
    }
}

/// Replace every tail self-call with the parameter update (and, in the
/// goto shape, the jump to the loop's continue label).
fn rewrite_tails(stmts: &mut Vec<Stmt>, name: &SelfName, params: &[String], with_goto: bool) {
    for_each_tail_block_mut(stmts, &mut |b| {
        let Some(Stmt::Return(e)) = b.last() else { return };
        if rewritable_site(e, name, params).is_none() {
            return;
        }
        let Some(Stmt::Return(mut e)) = b.pop() else { unreachable!() };
        while let Expr::Paren(inner) = e {
            e = *inner;
        }
        let Expr::Call(_, args) = e else { unreachable!() };
        if !params.is_empty() {
            b.push(Stmt::MultiAssign(params.to_vec(), args));
        }
        // Zero parameters: nothing to update — the site becomes bare
        // fall-through (shape A) or just the goto (shape B).
        if with_goto {
            b.push(Stmt::Goto("continue".into()));
        }
    });
}

// ---- Single-return proof (name-called functions) ----

/// Every return of THIS function's own scope yields exactly one value.
/// Nested function bodies (named or literal) are separate scopes and do not
/// count; a Raw statement that even mentions `return` fails the proof.
fn single_return_body(stmts: &[Stmt], name: &SelfName) -> bool {
    stmts.iter().all(|s| single_return_stmt(s, name))
}

fn single_return_stmt(s: &Stmt, name: &SelfName) -> bool {
    match s {
        Stmt::Return(e) => {
            let mut e = e;
            let mut truncated = false;
            while let Expr::Paren(inner) = e {
                e = inner;
                truncated = true;
            }
            match e {
                // A paren already truncates whatever is inside to one value.
                _ if truncated => true,
                // `error_` — the runtime raise helper Haskell's `error`
                // sanitizes to — never returns at all, so a `return
                // error_(…)` yields one value vacuously. The name cannot
                // denote anything multi-return: a user function spelled
                // `error_` compiles to a `__mll_fn` slot or a where-local
                // (both single-return by construction), and FFI call sites
                // emit the HOST spelling, never the Haskell name. Without
                // this arm the proof declined exactly the chains whose
                // fall-off arm is a user `error` call (huffman's go).
                Expr::Call(f, _) => {
                    opt::single_return_callee(f)
                        || is_self_ref(f, name)
                        || matches!(&**f, Expr::Name(n) if n == "error_")
                }
                Expr::Method(..) | Expr::Raw(_) => false,
                _ => true,
            }
        }
        // Zero values, not one — and it cannot appear pre-conversion anyway.
        Stmt::ReturnNone => false,
        Stmt::Raw(t) => {
            let mut toks = HashSet::new();
            opt::token_set(t, &mut toks);
            !toks.contains("return")
        }
        Stmt::If { then_b, elseifs, else_b, .. } => {
            single_return_body(&then_b.0, name)
                && elseifs.iter().all(|(_, b)| single_return_body(&b.0, name))
                && else_b.as_ref().is_none_or(|b| single_return_body(&b.0, name))
        }
        Stmt::Do(b) | Stmt::WhileTrue(b) => single_return_body(&b.0, name),
        // Returns inside a nested named function belong to that function.
        Stmt::Function { .. } => true,
        _ => true,
    }
}

// ---- Rename blocking ----

/// Any parameter mention the scoped rename cannot handle blocks the whole
/// conversion (see the module comment). Field positions (index suffixes,
/// method names, table keys) are never variable references and are ignored.
pub(super) fn rename_blocked(stmts: &[Stmt], params: &HashSet<String>) -> bool {
    stmts.iter().any(|s| blocked_stmt(s, params))
}

fn text_mentions(text: &str, params: &HashSet<String>) -> bool {
    let mut toks = HashSet::new();
    opt::token_set(text, &mut toks);
    !toks.is_disjoint(params)
}

/// A rendered lvalue: an exact parameter name is a renamable occurrence; a
/// composite spelling mentioning a parameter is not provably a variable
/// reference and blocks.
fn lvalue_blocked(lhs: &str, params: &HashSet<String>) -> bool {
    !params.contains(lhs) && text_mentions(lhs, params)
}

// The blocking decision — which rendered text could hide a parameter
// mention the rename cannot touch — stays spelled per variant with NO
// wildcard: an unclassified statement kind must not silently pass as
// renamable. Descent is the AST's own (`for_each_expr` / `for_each_block`).
fn blocked_stmt(s: &Stmt, params: &HashSet<String>) -> bool {
    let own = match s {
        Stmt::Raw(t) => text_mentions(t, params),
        Stmt::Assign(lhs, _) | Stmt::AssignIf { lhs, .. } => lvalue_blocked(lhs, params),
        Stmt::MultiAssign(lhs, _) => lhs.iter().any(|l| lvalue_blocked(l, params)),
        // A nested header mentioning a parameter would shadow (its own
        // parameter) or rebind (its name/target) it in a position the rename
        // does not touch: block. Otherwise the body is renamed like any
        // sub-block. Matches what the textual token check blocked: the
        // target's identifier tokens (a composite `_v[i]` lvalue via
        // `text_mentions`, a slot target's `__mll_fn` token) and every
        // parameter name.
        Stmt::Function { target, params: ps, .. } => {
            let target_hit = match target {
                FnTarget::LocalFn(n) | FnTarget::Assigned(n) => text_mentions(n, params),
                FnTarget::Slot(_) => params.contains("__mll_fn"),
            };
            target_hit || ps.iter().any(|p| params.contains(p))
        }
        Stmt::Local(..)
        | Stmt::Return(_)
        | Stmt::Expr(_)
        | Stmt::If { .. }
        | Stmt::Do(_)
        | Stmt::WhileTrue(_)
        | Stmt::ReturnNone
        | Stmt::Goto(_)
        | Stmt::Label(_)
        | Stmt::ReturnTable(_) => false,
    };
    if own {
        return true;
    }
    let mut blocked = false;
    s.for_each_expr(&mut |e| blocked = blocked || blocked_expr(e, params));
    s.for_each_block(&mut |b| blocked = blocked || rename_blocked(b, params));
    blocked
}

fn blocked_expr(e: &Expr, params: &HashSet<String>) -> bool {
    let own = match e {
        // An exact parameter name is the renamable case; a composite
        // spelling (`math.pi`, `_v[2]`) mentioning one is not provably a
        // variable occurrence.
        Expr::Name(s) => !params.contains(s) && !is_plain_ident(s) && text_mentions(s, params),
        Expr::Raw(t) => text_mentions(t, params),
        // The literal's body: statements, which `for_each_subexpr` does not
        // reach.
        Expr::Func(_, body) => rename_blocked(body.stmts(), params),
        Expr::Lit(_)
        | Expr::Paren(_)
        | Expr::Neg(_)
        | Expr::Call(..)
        | Expr::Method(..)
        | Expr::Index(..)
        | Expr::Binop(..)
        | Expr::Table(_)
        | Expr::TableSpaced(_) => false,
    };
    if own {
        return true;
    }
    let mut blocked = false;
    e.for_each_subexpr(&mut |c| blocked = blocked || blocked_expr(c, params));
    blocked
}

// ---- The scoped rename ----

/// Rename every variable occurrence of a mapped name, narrowing the map at
/// shadowing binders. Only runs after `rename_blocked` cleared the body, so
/// every remaining mention is a plain variable occurrence.
pub(super) fn rename_block(stmts: &mut [Stmt], map: &HashMap<String, String>) {
    // Cloned per block: a `local` shadows only for the REST of its own
    // block, and sub-blocks must not leak their shadowing back out.
    let mut map = map.clone();
    for s in stmts {
        rename_stmt(s, &mut map);
    }
}

fn rename_stmt(s: &mut Stmt, map: &mut HashMap<String, String>) {
    match s {
        Stmt::Local(names, init) => {
            // The initializer reads the outer binding (`local x = x` reads
            // the old x); the shadow starts after this statement.
            if let Some(e) = init {
                rename_expr(e, map);
            }
            for n in names.iter() {
                map.remove(n);
            }
        }
        Stmt::Assign(lhs, e) => {
            rename_expr(e, map);
            if let Some(w) = map.get(lhs) {
                *lhs = w.clone();
            }
        }
        Stmt::MultiAssign(lhs, exprs) => {
            for e in exprs.iter_mut() {
                rename_expr(e, map);
            }
            for l in lhs.iter_mut() {
                if let Some(w) = map.get(l) {
                    *l = w.clone();
                }
            }
        }
        Stmt::AssignIf { lhs, cond, then_e, else_e } => {
            rename_expr(cond, map);
            rename_expr(then_e, map);
            rename_expr(else_e, map);
            if let Some(w) = map.get(lhs) {
                *lhs = w.clone();
            }
        }
        // The header is proven disjoint from the map's names
        // (`rename_blocked`), so nothing in it shadows: the body renames
        // under the same map (captured outer parameters included).
        Stmt::Function { body, .. } => rename_block(&mut body.0, map),
        // No binders and no lvalues in the rest: rename the expressions
        // under the current map, and every sub-block under its own
        // per-block clone (`rename_block`).
        Stmt::Raw(_)
        | Stmt::Return(_)
        | Stmt::Expr(_)
        | Stmt::If { .. }
        | Stmt::Do(_)
        | Stmt::WhileTrue(_)
        | Stmt::ReturnNone
        | Stmt::Goto(_)
        | Stmt::Label(_)
        | Stmt::ReturnTable(_) => {
            s.for_each_expr_mut(&mut |e| rename_expr(e, map));
            s.for_each_block_mut(&mut |b| rename_block(b, map));
        }
    }
}

fn rename_expr(e: &mut Expr, map: &HashMap<String, String>) {
    match e {
        Expr::Name(s) => {
            if let Some(w) = map.get(s) {
                *s = w.clone();
            }
        }
        // A literal's parameters shadow: its body renames under a narrowed
        // map.
        Expr::Func(ps, body) => {
            let mut inner = map.clone();
            for p in ps.iter() {
                inner.remove(p);
            }
            rename_block(body.stmts_mut(), &inner);
        }
        // Everything else has no binders and no name fields of its own —
        // `Raw` is proven mention-free by `rename_blocked` before the
        // rename ever runs.
        Expr::Lit(_)
        | Expr::Raw(_)
        | Expr::Paren(_)
        | Expr::Neg(_)
        | Expr::Call(..)
        | Expr::Method(..)
        | Expr::Index(..)
        | Expr::Binop(..)
        | Expr::Table(_)
        | Expr::TableSpaced(_) => e.for_each_subexpr_mut(&mut |c| rename_expr(c, map)),
    }
}

// ---- Fresh names ----

/// Every identifier token of the rendered function (Raw text included —
/// rendering covers it), for fresh-name selection. Shared with ioloop.rs.
pub(super) fn used_tokens(
    target: &FnTarget,
    params: &[String],
    body: &[Stmt],
) -> HashSet<String> {
    let mut text = target.header_text(params);
    for s in body {
        s.render_line(0, &mut text);
    }
    let mut used = HashSet::new();
    opt::token_set(&text, &mut used);
    used
}

/// `prefix`+`0..n`, with underscores appended to the prefix until none of
/// the candidates collides with a used token. Shared with ioloop.rs.
pub(super) fn fresh_with_prefix(used: &HashSet<String>, prefix: &str, n: usize) -> Vec<String> {
    let mut prefix = String::from(prefix);
    loop {
        let cand: Vec<String> = (0..n).map(|i| format!("{}{}", prefix, i)).collect();
        if cand.iter().all(|c| !used.contains(c)) {
            return cand;
        }
        prefix.push('_');
    }
}

// ---- The conversion ----

fn convert(
    target: &FnTarget,
    params: &[String],
    body: &Block,
    view: &ScopeView<'_>,
    locals_in_scope: &HashSet<String>,
) -> Option<Block> {
    let self_name = self_target(target, params)?;
    if !self_qualifies(&self_name, view, locals_in_scope) {
        return None;
    }
    if !has_tail_self_call(&body.0, &self_name, params) {
        return None;
    }
    let param_set: HashSet<String> = params.iter().cloned().collect();
    if rename_blocked(&body.0, &param_set) {
        return None;
    }
    // Single-return (see the module comment): slot callees by the existing
    // compiled-function axiom, name callees by this body's own returns.
    if !matches!(self_name, SelfName::Slot(_)) && !single_return_body(&body.0, &self_name) {
        return None;
    }
    // The `w` copies add one local per parameter; stay inside the same
    // budget the emitter's `_v` spill and the IIFE pass respect. (The
    // conservative 2×: parameters themselves also occupy slots the
    // statement-level count cannot see.)
    if opt::count_locals(&body.0) + 2 * params.len() > super::CodeGen::LOCAL_LIMIT {
        return None;
    }

    // Goto-free when the body diverges: a tail update replaced a statement
    // in tail position, so it falls through every enclosing level straight
    // to the loop end, and no OTHER path reaches the loop end at all. A
    // body that can fall off needs the `do return end ::continue::` tail.
    let falls_off = !opt::block_diverges(body);

    // Per-iteration copy names for this function: `_w0.._wn`.
    let ws = fresh_with_prefix(&used_tokens(target, params, &body.0), "_w", params.len());
    let map: HashMap<String, String> =
        params.iter().cloned().zip(ws.iter().cloned()).collect();

    let mut stmts = body.0.clone();
    rename_block(&mut stmts, &map);
    rewrite_tails(&mut stmts, &self_name, params, falls_off);

    let mut inner: Vec<Stmt> = Vec::with_capacity(stmts.len() + params.len() + 2);
    for (w, p) in ws.iter().zip(params.iter()) {
        inner.push(Stmt::Local(vec![w.clone()], Some(Expr::name(p.clone()))));
    }
    inner.extend(stmts);
    if falls_off {
        inner.push(Stmt::Do(Block(vec![Stmt::ReturnNone])));
        inner.push(Stmt::Label("continue".into()));
    }
    Some(Block(vec![Stmt::WhileTrue(Block(inner))]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::annot::Engine;
    use crate::codegen::lua::FuncBody;

    /// Run the pass over a module and return the rendered output.
    fn converted(mut stmts: Vec<Stmt>) -> (String, bool) {
        let rewrote = Engine::run_structured(&mut stmts, &mut TailLoop).is_some();
        let mut out = String::new();
        Block(stmts).render(0, &mut out);
        (out, rewrote)
    }

    /// `__mll_fn[1] = function(n, acc)` with a diverging if/else body whose
    /// else arm tail-calls the slot: the canonical accumulator.
    fn slot_accumulator() -> Vec<Stmt> {
        vec![
            Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
            Stmt::Function {
                target: FnTarget::Slot(1),
                params: vec!["n".into(), "acc".into()],
                body: Block(vec![Stmt::If {
                    cond: Expr::binop("==", Expr::name("n"), Expr::lit("0")),
                    then_b: Block(vec![Stmt::Return(Expr::name("acc"))]),
                    elseifs: vec![],
                    else_b: Some(Block(vec![Stmt::Return(Expr::call_named(
                        "__mll_fn[1]",
                        vec![
                            Expr::binop("-", Expr::name("n"), Expr::lit("1")),
                            Expr::binop("+", Expr::name("acc"), Expr::name("n")),
                        ],
                    ))])),
                }]),
            },
        ]
    }

    #[test]
    fn slot_accumulator_converts_to_loop() {
        let (out, rewrote) = converted(slot_accumulator());
        assert!(rewrote);
        assert!(out.contains("while true do"), "{out}");
        // Per-iteration copies…
        assert!(out.contains("local _w0 = n"), "{out}");
        assert!(out.contains("local _w1 = acc"), "{out}");
        // …a renamed body…
        assert!(out.contains("if _w0 == 0 then"), "{out}");
        // …and the simultaneous update in argument order, over the copies.
        assert!(out.contains("n, acc = _w0 - 1, _w1 + _w0"), "{out}");
        // Diverging body: fall-through shape, no goto scaffold.
        assert!(!out.contains("goto continue"), "{out}");
        assert!(!out.contains("::continue::"), "{out}");
    }

    #[test]
    fn swap_update_is_one_simultaneous_assignment() {
        // go a b -> go b a: a cascade would clobber; the multiple
        // assignment must carry both, in argument order.
        let stmts = vec![
            Stmt::Local(vec!["go".into()], None),
            Stmt::Function {
                target: FnTarget::Assigned("go".into()),
                params: vec!["a".into(), "b".into()],
                body: Block(vec![Stmt::If {
                    cond: Expr::name("a"),
                    then_b: Block(vec![Stmt::Return(Expr::name("b"))]),
                    elseifs: vec![],
                    else_b: Some(Block(vec![Stmt::Return(Expr::call_named(
                        "go",
                        vec![Expr::name("b"), Expr::name("a")],
                    ))])),
                }]),
            },
        ];
        let (out, rewrote) = converted(stmts);
        assert!(rewrote);
        assert!(out.contains("a, b = _w1, _w0"), "{out}");
    }

    #[test]
    fn do_tail_and_non_tail_rejection() {
        // The tail self-call inside a final `do` block converts; the
        // NON-tail `return f(…)` (an if that is not the last statement)
        // stays an ordinary call.
        let stmts = vec![Stmt::Function {
            target: FnTarget::LocalFn("f".into()),
            params: vec!["x".into()],
            body: Block(vec![
                Stmt::If {
                    cond: Expr::name("x"),
                    then_b: Block(vec![Stmt::Return(Expr::call_named(
                        "f",
                        vec![Expr::lit("1")],
                    ))]),
                    elseifs: vec![],
                    else_b: None,
                },
                Stmt::Do(Block(vec![Stmt::Return(Expr::call_named(
                    "f",
                    vec![Expr::lit("2")],
                ))])),
            ]),
        }];
        let (out, rewrote) = converted(stmts);
        assert!(rewrote);
        // Tail site (the do tail) became the update…
        assert!(out.contains("x = 2"), "{out}");
        // …the non-tail site stayed a call.
        assert!(out.contains("return f(1)"), "{out}");
        assert!(!out.contains("x = 1"), "{out}");
    }

    #[test]
    fn fall_off_body_gets_goto_shape() {
        // No else arm: the body can fall off, so the update jumps and the
        // loop end returns bare.
        let stmts = vec![Stmt::Function {
            target: FnTarget::LocalFn("f".into()),
            params: vec!["x".into()],
            body: Block(vec![Stmt::If {
                cond: Expr::name("x"),
                then_b: Block(vec![Stmt::Return(Expr::call_named(
                    "f",
                    vec![Expr::lit("1")],
                ))]),
                elseifs: vec![],
                else_b: None,
            }]),
        }];
        let (out, rewrote) = converted(stmts);
        assert!(rewrote);
        assert!(out.contains("goto continue"), "{out}");
        assert!(out.contains("::continue::"), "{out}");
        // The bare return sits before the label so fall-off exits the loop.
        let ret = out.find("return\n").expect("bare return");
        let lab = out.find("::continue::").expect("label");
        assert!(ret < lab, "{out}");
    }

    #[test]
    fn single_binding_gate() {
        // A second store to the slot: the callee is no longer provably this
        // function — no conversion.
        let mut stmts = slot_accumulator();
        stmts.push(Stmt::Assign(
            "__mll_fn[1]".into(),
            Expr::name("something_else"),
        ));
        let (_, rewrote) = converted(stmts);
        assert!(!rewrote);

        // Same for a rebound local-function name.
        let stmts = vec![
            Stmt::Function {
                target: FnTarget::LocalFn("f".into()),
                params: vec!["x".into()],
                body: Block(vec![Stmt::Return(Expr::call_named(
                    "f",
                    vec![Expr::lit("1")],
                ))]),
            },
            Stmt::Assign("f".into(), Expr::name("g")),
        ];
        let (_, rewrote) = converted(stmts);
        assert!(!rewrote);
    }

    #[test]
    fn varargs_and_unknown_headers_skip() {
        // Varargs parameters, `_v[i]` spill assignment targets, duplicate
        // parameters: every shape `self_target` must decline.
        let cases: Vec<(FnTarget, Vec<String>)> = vec![
            (FnTarget::LocalFn("f".into()), vec!["...".into()]),
            (FnTarget::LocalFn("f".into()), vec!["a".into(), "...".into()]),
            (FnTarget::Assigned("_v[3]".into()), vec!["a".into()]),
            (FnTarget::Slot(2), vec!["_v".into(), "...".into()]),
            (FnTarget::LocalFn("f".into()), vec!["a".into(), "a".into()]),
        ];
        for (target, params) in cases {
            let header = target.header_text(&params);
            let stmts = vec![
                Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
                Stmt::Function {
                    target,
                    params,
                    body: Block(vec![Stmt::Return(Expr::call_named(
                        "f",
                        vec![Expr::lit("1")],
                    ))]),
                },
            ];
            let (_, rewrote) = converted(stmts);
            assert!(!rewrote, "header {header:?} must be skipped");
        }
    }

    #[test]
    fn raw_mention_of_parameter_blocks() {
        let stmts = vec![Stmt::Function {
            target: FnTarget::LocalFn("f".into()),
            params: vec!["x".into()],
            body: Block(vec![
                Stmt::Raw("x = x + 1".into()),
                Stmt::Return(Expr::call_named("f", vec![Expr::lit("1")])),
            ]),
        }];
        let (out, rewrote) = converted(stmts);
        assert!(!rewrote, "{out}");
    }

    #[test]
    fn raw_mention_of_self_name_blocks() {
        let stmts = vec![
            Stmt::Function {
                target: FnTarget::LocalFn("f".into()),
                params: vec!["x".into()],
                body: Block(vec![Stmt::Return(Expr::call_named(
                    "f",
                    vec![Expr::lit("1")],
                ))]),
            },
            Stmt::Raw("host_hook(f)".into()),
        ];
        let (_, rewrote) = converted(stmts);
        assert!(!rewrote);
    }

    #[test]
    fn shadowed_parameter_is_not_renamed() {
        // The lambda's own `x` shadows the parameter: its body must keep
        // the name while the outer occurrence is renamed.
        let stmts = vec![Stmt::Function {
            target: FnTarget::LocalFn("f".into()),
            params: vec!["x".into()],
            body: Block(vec![
                Stmt::Local(
                    vec!["g".into()],
                    Some(Expr::Func(
                        vec!["x".into()],
                        FuncBody::Inline(vec![Stmt::Return(Expr::name("x"))]),
                    )),
                ),
                Stmt::If {
                    cond: Expr::name("x"),
                    then_b: Block(vec![Stmt::Return(Expr::name("g"))]),
                    elseifs: vec![],
                    else_b: Some(Block(vec![Stmt::Return(Expr::call_named(
                        "f",
                        vec![Expr::name("x")],
                    ))])),
                },
            ]),
        }];
        let (out, rewrote) = converted(stmts);
        assert!(rewrote);
        assert!(out.contains("function(x) return x end"), "{out}");
        assert!(out.contains("if _w0 then"), "{out}");
        assert!(out.contains("x = _w0"), "{out}");
    }

    #[test]
    fn multi_return_body_is_not_converted() {
        // A named function whose base case returns a possibly-multi-value
        // call (unknown callee in tail position) fails the single-return
        // proof.
        let stmts = vec![Stmt::Function {
            target: FnTarget::LocalFn("f".into()),
            params: vec!["x".into()],
            body: Block(vec![Stmt::If {
                cond: Expr::name("x"),
                then_b: Block(vec![Stmt::Return(Expr::call_named(
                    "host_fn",
                    vec![],
                ))]),
                elseifs: vec![],
                else_b: Some(Block(vec![Stmt::Return(Expr::call_named(
                    "f",
                    vec![Expr::lit("1")],
                ))])),
            }]),
        }];
        let (_, rewrote) = converted(stmts);
        assert!(!rewrote);
    }

    #[test]
    fn refutation_green_over_converted_output() {
        let mut stmts = slot_accumulator();
        let engine =
            Engine::run_structured(&mut stmts, &mut TailLoop).expect("conversion applied");
        assert!(engine.refute(&stmts, false).is_empty());
    }
}
