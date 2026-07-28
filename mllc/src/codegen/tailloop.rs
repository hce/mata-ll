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
//! * VARARGS / headers — the header is pre-rendered text; only the three
//!   spellings verified against the corpus are parsed (`local function
//!   f(…)`, `f = function(…)`, `__mll_fn[i] = function(…)`, plain-identifier
//!   parameters). Anything else — `...`, spill-slot `_v[i]` targets, the
//!   one-line Raw accessor adapters — is skipped.
//!
//! Non-tail self-calls stay ordinary calls (they re-enter the converted
//! function from the top — same semantics); IO self-loops through
//! `__mll_run_tail(<self>(…))` are not self-calls at the callee position and
//! are left alone.

use std::collections::{HashMap, HashSet};

use super::annot::{self, ScopeView, is_plain_ident};
use super::lua::{Block, Expr, FuncBody, Item, Stmt};
use super::opt;

pub(super) struct TailLoop;

impl annot::StructuredPass for TailLoop {
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

/// The self-name a header binds, in the spelling body calls use.
/// Shared with the IO self-loop pass (ioloop.rs), which converts the same
/// three header spellings.
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
    fn spelling(&self) -> &str {
        match self {
            SelfName::LocalFn(s) | SelfName::Assigned(s) | SelfName::Slot(s) => s,
        }
    }
}

/// Parse a `Stmt::Function` header into its self-name and parameter list.
/// Only the three spellings verified against corpus output are accepted;
/// any other shape (varargs, `_v[i]` spill targets, unexpected trailing
/// text) returns `None` and the function is skipped.
pub(super) fn parse_header(header: &str) -> Option<(SelfName, Vec<String>)> {
    let (name, params_text) = if let Some(rest) = header.strip_prefix("local function ") {
        let open = rest.find('(')?;
        let n = &rest[..open];
        if !is_plain_ident(n) {
            return None;
        }
        (SelfName::LocalFn(n.to_string()), &rest[open..])
    } else {
        let eq = header.find(" = function(")?;
        let n = &header[..eq];
        let name = if is_plain_ident(n) {
            SelfName::Assigned(n.to_string())
        } else if annot::is_slot_ref(n) {
            SelfName::Slot(n.to_string())
        } else {
            return None;
        };
        (name, &header[eq + " = function".len()..])
    };
    let inner = params_text.strip_prefix('(')?.strip_suffix(')')?;
    let params: Vec<String> = if inner.is_empty() {
        Vec::new()
    } else {
        inner.split(", ").map(str::to_string).collect()
    };
    if !params.iter().all(|p| is_plain_ident(p)) {
        return None;
    }
    // Duplicate parameters would make both the rename and the multiple
    // assignment ambiguous; the emitter never produces them.
    let unique: HashSet<&String> = params.iter().collect();
    if unique.len() != params.len() {
        return None;
    }
    Some((name, params))
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

pub(super) fn is_self_ref(f: &Expr, name: &SelfName) -> bool {
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
    match stmts.last() {
        Some(Stmt::Return(e)) => rewritable_site(e, name, params).is_some(),
        Some(Stmt::If { then_b, elseifs, else_b, .. }) => {
            has_tail_self_call(&then_b.0, name, params)
                || elseifs.iter().any(|(_, b)| has_tail_self_call(&b.0, name, params))
                || else_b
                    .as_ref()
                    .is_some_and(|b| has_tail_self_call(&b.0, name, params))
        }
        Some(Stmt::Do(b)) => has_tail_self_call(&b.0, name, params),
        _ => false,
    }
}

/// Replace every tail self-call with the parameter update (and, in the
/// goto shape, the jump to the loop's continue label).
fn rewrite_tails(stmts: &mut Vec<Stmt>, name: &SelfName, params: &[String], with_goto: bool) {
    match stmts.last_mut() {
        Some(Stmt::Return(e)) => {
            if rewritable_site(e, name, params).is_none() {
                return;
            }
            let Some(Stmt::Return(mut e)) = stmts.pop() else { unreachable!() };
            while let Expr::Paren(inner) = e {
                e = *inner;
            }
            let Expr::Call(_, args) = e else { unreachable!() };
            if !params.is_empty() {
                stmts.push(Stmt::MultiAssign(params.to_vec(), args));
            }
            // Zero parameters: nothing to update — the site becomes bare
            // fall-through (shape A) or just the goto (shape B).
            if with_goto {
                stmts.push(Stmt::Goto("continue".into()));
            }
        }
        Some(Stmt::If { then_b, elseifs, else_b, .. }) => {
            rewrite_tails(&mut then_b.0, name, params, with_goto);
            for (_, b) in elseifs.iter_mut() {
                rewrite_tails(&mut b.0, name, params, with_goto);
            }
            if let Some(b) = else_b.as_mut() {
                rewrite_tails(&mut b.0, name, params, with_goto);
            }
        }
        Some(Stmt::Do(b)) => rewrite_tails(&mut b.0, name, params, with_goto),
        _ => {}
    }
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
                Expr::Call(f, _) => opt::single_return_callee(f) || is_self_ref(f, name),
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

fn blocked_stmt(s: &Stmt, params: &HashSet<String>) -> bool {
    match s {
        Stmt::Raw(t) => text_mentions(t, params),
        Stmt::Local(_, init) => init.as_ref().is_some_and(|e| blocked_expr(e, params)),
        Stmt::Assign(lhs, e) => lvalue_blocked(lhs, params) || blocked_expr(e, params),
        Stmt::MultiAssign(lhs, exprs) => {
            lhs.iter().any(|l| lvalue_blocked(l, params))
                || exprs.iter().any(|e| blocked_expr(e, params))
        }
        Stmt::AssignIf { lhs, cond, then_e, else_e } => {
            lvalue_blocked(lhs, params)
                || blocked_expr(cond, params)
                || blocked_expr(then_e, params)
                || blocked_expr(else_e, params)
        }
        Stmt::Return(e) | Stmt::Expr(e) => blocked_expr(e, params),
        Stmt::ReturnNone | Stmt::Goto(_) | Stmt::Label(_) => false,
        Stmt::If { cond, then_b, elseifs, else_b } => {
            blocked_expr(cond, params)
                || rename_blocked(&then_b.0, params)
                || elseifs
                    .iter()
                    .any(|(c, b)| blocked_expr(c, params) || rename_blocked(&b.0, params))
                || else_b.as_ref().is_some_and(|b| rename_blocked(&b.0, params))
        }
        Stmt::Do(b) | Stmt::WhileTrue(b) => rename_blocked(&b.0, params),
        // A nested header mentioning a parameter would shadow (its own
        // parameter) or rebind (its name) it in text the rename cannot
        // touch: block. Otherwise the body is renamed like any sub-block.
        Stmt::Function { header, body } => {
            text_mentions(header, params) || rename_blocked(&body.0, params)
        }
        Stmt::ReturnTable(entries) => entries.iter().any(|(_, e)| blocked_expr(e, params)),
    }
}

fn blocked_expr(e: &Expr, params: &HashSet<String>) -> bool {
    match e {
        // An exact parameter name is the renamable case; a composite
        // spelling (`math.pi`, `_v[2]`) mentioning one is not provably a
        // variable occurrence.
        Expr::Name(s) => !params.contains(s) && !is_plain_ident(s) && text_mentions(s, params),
        Expr::Lit(_) => false,
        Expr::Raw(t) => text_mentions(t, params),
        Expr::Paren(e) | Expr::Neg(e) => blocked_expr(e, params),
        Expr::Call(f, args) | Expr::Method(f, _, args) => {
            blocked_expr(f, params) || args.iter().any(|a| blocked_expr(a, params))
        }
        Expr::Index(base, _) => blocked_expr(base, params),
        Expr::Binop(_, l, r) => blocked_expr(l, params) || blocked_expr(r, params),
        Expr::Table(items) | Expr::TableSpaced(items) => items.iter().any(|item| match item {
            Item::Pos(e) | Item::KV(_, e) => blocked_expr(e, params),
        }),
        Expr::Func(_, body) => rename_blocked(func_body(body), params),
    }
}

fn func_body(b: &FuncBody) -> &Vec<Stmt> {
    match b {
        FuncBody::Inline(s) => s,
        FuncBody::Block(Block(s)) => s,
    }
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
        Stmt::Raw(_) | Stmt::ReturnNone | Stmt::Goto(_) | Stmt::Label(_) => {}
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
        Stmt::Return(e) | Stmt::Expr(e) => rename_expr(e, map),
        Stmt::If { cond, then_b, elseifs, else_b } => {
            rename_expr(cond, map);
            rename_block(&mut then_b.0, map);
            for (c, b) in elseifs.iter_mut() {
                rename_expr(c, map);
                rename_block(&mut b.0, map);
            }
            if let Some(b) = else_b.as_mut() {
                rename_block(&mut b.0, map);
            }
        }
        Stmt::Do(b) | Stmt::WhileTrue(b) => rename_block(&mut b.0, map),
        // The header is proven disjoint from the map's names
        // (`rename_blocked`), so nothing in it shadows: the body renames
        // under the same map (captured outer parameters included).
        Stmt::Function { body, .. } => rename_block(&mut body.0, map),
        Stmt::ReturnTable(entries) => {
            for (_, e) in entries {
                rename_expr(e, map);
            }
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
        Expr::Lit(_) | Expr::Raw(_) => {}
        Expr::Paren(e) | Expr::Neg(e) => rename_expr(e, map),
        Expr::Call(f, args) | Expr::Method(f, _, args) => {
            rename_expr(f, map);
            for a in args {
                rename_expr(a, map);
            }
        }
        Expr::Index(base, _) => rename_expr(base, map),
        Expr::Binop(_, l, r) => {
            rename_expr(l, map);
            rename_expr(r, map);
        }
        Expr::Table(items) | Expr::TableSpaced(items) => {
            for item in items {
                match item {
                    Item::Pos(e) | Item::KV(_, e) => rename_expr(e, map),
                }
            }
        }
        Expr::Func(ps, body) => {
            let mut inner = map.clone();
            for p in ps.iter() {
                inner.remove(p);
            }
            let stmts: &mut Vec<Stmt> = match body {
                FuncBody::Inline(s) => s,
                FuncBody::Block(Block(s)) => s,
            };
            rename_block(stmts, &inner);
        }
    }
}

// ---- Fresh names ----

/// Every identifier token of the rendered function (Raw text included —
/// rendering covers it), for fresh-name selection. Shared with ioloop.rs.
pub(super) fn used_tokens(header: &str, body: &[Stmt]) -> HashSet<String> {
    let mut text = String::from(header);
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

/// Per-iteration copy names for this function: `_w0.._wn`.
fn fresh_names(header: &str, body: &[Stmt], n: usize) -> Vec<String> {
    fresh_with_prefix(&used_tokens(header, body), "_w", n)
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
    if !has_tail_self_call(&body.0, &self_name, &params) {
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

    let ws = fresh_names(header, &body.0, params.len());
    let map: HashMap<String, String> =
        params.iter().cloned().zip(ws.iter().cloned()).collect();

    let mut stmts = body.0.clone();
    rename_block(&mut stmts, &map);
    rewrite_tails(&mut stmts, &self_name, &params, falls_off);

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
                header: "__mll_fn[1] = function(n, acc)".into(),
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
                header: "go = function(a, b)".into(),
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
            header: "local function f(x)".into(),
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
            header: "local function f(x)".into(),
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
                header: "local function f(x)".into(),
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
        for header in [
            "local function f(...)",
            "local function f(a, ...)",
            "_v[3] = function(a)",
            "__mll_fn[2] = function(_v, ...)",
        ] {
            let stmts = vec![
                Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))),
                Stmt::Function {
                    header: header.into(),
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
            header: "local function f(x)".into(),
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
                header: "local function f(x)".into(),
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
            header: "local function f(x)".into(),
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
            header: "local function f(x)".into(),
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
