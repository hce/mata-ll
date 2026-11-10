//! Pass 9 — the upvalue budget, the last pass over the finished tree (see
//! opt.rs's pipeline comment).
//!
//! A Lua function literal captures every enclosing local it mentions as an
//! upvalue, and a host caps how many one function may have: LuaJIT allows
//! 60 (`LJ_MAX_UPVAL` in lj_def.h, enforced by `checklimit(fs, fs->nuv,
//! LJ_MAX_UPVAL, "upvalues")` in lj_parse.c — the 61st fails the whole
//! chunk's load with "function at line N has more than 60 upvalues"), PUC
//! Lua 255 (`MAXUPVAL` in lfunc.h). Nothing upstream counts captures: the
//! emitter budgets LOCALS per function (`LOCAL_LIMIT`, the `_v` spill),
//! thunklift.rs lifts a suspension out of its closure only up to a few
//! captures, and a lambda or a thunk over a where group of seventy
//! bindings — or over fifty bindings plus a dozen runtime helpers, which
//! are locals of the chunk — keeps every one of them as an upvalue.
//!
//! The pass counts, per function literal and named function, the distinct
//! upvalues it needs exactly as the host's parser will: every name its body
//! mentions that resolves to a local of an enclosing function or of the
//! chunk (the emitted module's own `local`s, and the runtime prelude's,
//! which `runtime::binds_name` knows), including the names its nested
//! functions capture through it (a grandchild's capture is an upvalue of
//! the child too). A free name no scope binds is a global and costs no
//! upvalue (LuaJIT resolves globals through the function environment; PUC
//! charges one `_ENV` upvalue per function for all of them together, which
//! the margin below covers). Names in opaque text — a `Raw` fragment, a
//! composite `Name` such as `_v[3]`, an index suffix, a rendered lvalue —
//! are read as identifier tokens, which can only over-count; a `Lit` is
//! never a reference, except the hoisted big-integer literal, which
//! indexes the chunk-level `__mll_biglit` table and counts as a mention
//! of it.
//!
//! A function over [`UPVALUE_BUDGET`] has captures routed through a
//! table: the declaring scope gets one table local, and a routed
//! variable is REPLACED by a slot of it — its declaration, every
//! assignment and every read, in the declaring function and in every
//! closure, through the one resolution that maps a name to its binding
//! (`Walk::reference`), so there is no second copy of the variable
//! anywhere. A Lua upvalue is a shared mutable cell, and so is a table
//! slot: an in-place force `x = __force(x)`, a chain-split rebind, a loop
//! accumulator or a tail-loop parameter update writes the slot every
//! closure reads. The function then captures the table (one upvalue) in
//! place of the routed names.
//!
//! Which table a variable goes to decides freshness. A `local` is a fresh
//! variable per execution of its block, and closures made in different
//! executions must keep different cells — a tail-call loop's per-iteration
//! `_w` copies exist for exactly that. So a table belongs to the innermost
//! enclosing function body or `while true` body (or the chunk), declared
//! at that block's start: the block runs once per call or per iteration,
//! and every other sub-block of it (`if` arms, `do`) runs at most once per
//! execution of it — the vocabulary has no backward `goto` — so a slot is
//! written at most once per table, exactly like the local it replaces, and
//! a forward declaration `local x` needs no statement at all (the slot is
//! nil in a fresh table). A parameter is routed by copying it into its
//! slot at entry and rewriting every later mention; a `local function f` by
//! assigning the slot. Captures from different declaring scopes go to
//! different tables, each one upvalue.
//!
//! A name is routable when no opaque text inside its table's block
//! mentions it (a Raw fragment there could read, assign or redeclare it
//! invisibly), when it is not itself a routing table, and when the owning
//! function has room for the table local under the host's limit on active
//! locals ([`LUA_MAX_LOCALS`]): this pass runs last, so the margin the
//! emitter keeps under `LOCAL_LIMIT` for the passes is its to spend — a
//! function whose bindings spilled into `_v` holds 180 locals plus the
//! spill table, and its one routing table makes 182. A runtime helper has
//! no declaration in the tree: it is routed by a copy into the chunk's
//! table at the chunk's start — exact because the prelude has fully run
//! by then and the emitted module never assigns a prelude name (the pass
//! checks the whole tree) — and only the mentions inside the over-budget
//! function are rewritten, so every other function keeps its upvalue
//! access. A function that cannot reach the budget with what is routable
//! would be a chunk the host refuses to load, so the pass reports it as a
//! compile error naming the function, the count and the captures it could
//! not route, instead of emitting it.
//!
//! Under budget everywhere — every program whose functions capture at most
//! [`UPVALUE_BUDGET`] names — the pass changes nothing and the output is
//! byte-identical to the tree it was given. Routing picks the declaring
//! scope with the largest routable group first and routes only as many
//! names as the budget needs, in declaration order, so the rewrite is
//! deterministic and as small as the budget allows.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::annot::is_plain_ident;
use super::lua::{BIGLIT_TABLE, Expr, FN_TABLE, FnTarget, Stmt, TKF_TABLE, render_stmts};
use super::opt;
use super::runtime;

/// The most upvalues a function literal is allowed to keep. LuaJIT's limit
/// is 60 (`LJ_MAX_UPVAL`), PUC's 255 (`MAXUPVAL`). Ten below LuaJIT's: the
/// count here is the host's except where opaque text over-counts, PUC
/// charges one `_ENV` upvalue on top for any global access, and a routed
/// function captures a table per declaring scope in place of the names.
pub(super) const UPVALUE_BUDGET: usize = 50;

/// The most locals a Lua function may have active at once: 200 on both
/// hosts (`LJ_MAX_LOCVAR` in lj_def.h, `MAXVARS` in lparser.c). The
/// emitter stops declaring plain locals at `LOCAL_LIMIT` (180) to leave
/// the passes room; `opt::count_locals` sums every declaration of the
/// function, branches included, so it bounds the active count from above
/// and one routing table fits whenever that sum stays below this.
pub(super) const LUA_MAX_LOCALS: usize = 200;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
struct BindId(usize);
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
struct FnId(usize);
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
struct TblId(usize);

/// One upvalue of a function: a binding of the tree, or a name the runtime
/// prelude binds at the chunk level (declared outside the tree).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Cap {
    Bind(BindId),
    Prelude(String),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BindKind {
    Local,
    Param,
    LocalFn,
}

struct Binding {
    name: String,
    kind: BindKind,
    /// The innermost function body, loop body or chunk block holding the
    /// binding — where its routing table lives.
    tbl: TblId,
}

/// What one walk of the tree learns (ids are walk-order positions, so the
/// rewrite walk of the same tree sees the same ids).
#[derive(Default)]
struct Analysis {
    binds: Vec<Binding>,
    /// Per function: the distinct upvalues it needs.
    fn_captures: Vec<BTreeSet<Cap>>,
    /// Per function: how a diagnostic names it (its header, or the header
    /// of the named function a literal sits in).
    fn_labels: Vec<String>,
    /// Per table block: every identifier token of opaque text inside it
    /// (nested blocks and function bodies included).
    tbl_opaque: Vec<HashSet<String>>,
    /// Per table block: the local count of the function owning it (the
    /// chunk's own for the chunk), parameters included.
    tbl_owner_locals: Vec<usize>,
    /// Every plain name an assignment or forward declaration in the tree
    /// targets, anywhere.
    assigned: HashSet<String>,
}

/// The routing table of one table block and what its declaration carries.
struct TableDecl {
    name: String,
    /// The table is new in this round: emit `local <name> = {}`.
    declare: bool,
    /// `slot = param` copies at entry.
    params: Vec<(String, String)>,
    /// `slot = helper` copies of prelude names (chunk table only).
    prelude: Vec<(String, String)>,
}

struct Plan {
    /// Binding → the slot lvalue that replaces it everywhere.
    route: HashMap<BindId, String>,
    /// (function, prelude name) → the slot lvalue replacing mentions inside
    /// that function.
    prelude: HashMap<(FnId, String), String>,
    tables: HashMap<TblId, TableDecl>,
}

/// What the pass remembers across rounds: the tables it has declared and
/// their next free slot (table-block ids are stable across rounds — a
/// round adds and removes statements, never blocks or functions).
#[derive(Default)]
struct State {
    tables: BTreeMap<TblId, String>,
    next_slot: BTreeMap<TblId, usize>,
    /// Identifier tokens of the module, for fresh table names; filled on
    /// the first round that needs one.
    used: Option<HashSet<String>>,
}

/// Route the captures of every function over the budget. Returns whether
/// the tree changed, or the diagnostic for a function the pass cannot
/// bring under the budget.
pub(super) fn run(stmts: &mut Vec<Stmt>) -> Result<bool, String> {
    let mut st = State::default();
    let mut any = false;
    // (function, its count) of the previous round's rewrite: the recount
    // must come out below it, or the plan's projection was wrong — an
    // internal error, reported rather than looped on.
    let mut last: Option<(FnId, usize)> = None;
    loop {
        let an = analyze(stmts);
        if let Some((f, before)) = last.take()
            && an.fn_captures[f.0].len() >= before
        {
            return Err(format!(
                "internal compiler error: routing the captures of {} did not lower its \
                 upvalue count ({} before, {} after)",
                an.fn_labels[f.0],
                before,
                an.fn_captures[f.0].len()
            ));
        }
        let over = (0..an.fn_captures.len())
            .map(FnId)
            .find(|f| an.fn_captures[f.0].len() > UPVALUE_BUDGET);
        let Some(f) = over else { break };
        match choose(f, &an, &mut st, stmts.as_slice()) {
            Err(stuck) => return Err(over_budget_error(f, &an, &stuck)),
            Ok(plan) => {
                last = Some((f, an.fn_captures[f.0].len()));
                apply(stmts, &plan);
                any = true;
            }
        }
    }
    Ok(any)
}

/// The diagnostic for a function whose routable captures cannot bring it
/// under the budget: `stuck` names the captures the pass could not route.
fn over_budget_error(f: FnId, an: &Analysis, stuck: &[String]) -> String {
    let count = an.fn_captures[f.0].len();
    let mut shown: Vec<&str> = stuck.iter().map(String::as_str).collect();
    shown.sort();
    let more = shown.len().saturating_sub(12);
    shown.truncate(12);
    let mut list = shown.join(", ");
    if more > 0 {
        list.push_str(&format!(", and {more} more"));
    }
    format!(
        "{} would need {count} upvalues — enclosing variables a Lua closure keeps a \
         reference to — and LuaJIT allows a function at most 60. The compiler keeps every \
         function at {UPVALUE_BUDGET} by routing captured variables through a table of the \
         scope declaring them, but these could not be routed: {list}. A variable cannot be \
         routed when a host-level fragment in its scope (an FFI descriptor, a Lua pattern \
         path) mentions it, or when its function has no room for one more local.\n  \
         note: split the closure's free variables across several functions or group them \
         in a record; this is a limit of the Lua host, not of Haskell.",
        an.fn_labels[f.0]
    )
}

fn analyze(stmts: &mut Vec<Stmt>) -> Analysis {
    let mut w = Walk::new(None);
    w.walk_chunk(stmts);
    w.an
}

fn apply(stmts: &mut Vec<Stmt>, plan: &Plan) {
    let mut w = Walk::new(Some(plan));
    w.walk_chunk(stmts);
}

/// Pick what to route for `f`; when the routable captures cannot bring it
/// under budget, the captures that could not be routed.
fn choose(f: FnId, an: &Analysis, st: &mut State, stmts: &[Stmt]) -> Result<Plan, Vec<String>> {
    let caps = &an.fn_captures[f.0];
    let count = caps.len();
    let cap_name = |cap: &Cap| match cap {
        Cap::Bind(b) => an.binds[b.0].name.clone(),
        Cap::Prelude(name) => name.clone(),
    };
    // Routable captures grouped by the table block they belong to; the
    // rest are what a failure reports.
    let mut groups: BTreeMap<TblId, Vec<Cap>> = BTreeMap::new();
    let mut stuck: Vec<String> = Vec::new();
    for cap in caps {
        let routable = match cap {
            Cap::Bind(b) => {
                let bd = &an.binds[b.0];
                !an.tbl_opaque[bd.tbl.0].contains(&bd.name)
                    && st.tables.get(&bd.tbl) != Some(&bd.name)
            }
            Cap::Prelude(name) => !an.assigned.contains(name),
        };
        if !routable {
            stuck.push(cap_name(cap));
            continue;
        }
        let tbl = match cap {
            Cap::Bind(b) => an.binds[b.0].tbl,
            Cap::Prelude(_) => TblId(0),
        };
        groups.entry(tbl).or_default().push(cap.clone());
    }
    // A table block whose owning function has no room for the table local
    // cannot be routed to; one whose table `f` already captures costs no
    // new upvalue.
    let mut ranked: Vec<(usize, TblId, bool)> = Vec::new();
    for (tbl, members) in &groups {
        let existing = st.tables.get(tbl);
        if existing.is_none() && an.tbl_owner_locals[tbl.0] + 1 > LUA_MAX_LOCALS {
            stuck.extend(members.iter().map(cap_name));
            continue;
        }
        let already = existing.is_some_and(|tn| {
            caps.iter().any(|c| {
                matches!(c, Cap::Bind(b)
                if an.binds[b.0].name == *tn && an.binds[b.0].tbl == *tbl)
            })
        });
        let gain = members.len() - usize::from(!already);
        if gain > 0 {
            ranked.push((gain, *tbl, already));
        }
    }
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut remaining = count - UPVALUE_BUDGET;
    let mut chosen: Vec<(TblId, bool, Vec<Cap>)> = Vec::new();
    for (_, tbl, already) in ranked {
        if remaining == 0 {
            break;
        }
        let members = &groups[&tbl];
        // Only as many as the budget needs: a new table pays for itself
        // with its first member.
        let take = (remaining + usize::from(!already)).min(members.len());
        let gain = take - usize::from(!already);
        remaining -= gain.min(remaining);
        chosen.push((tbl, already, members[..take].to_vec()));
    }
    if remaining > 0 {
        return Err(stuck);
    }
    let used = st.used.get_or_insert_with(|| {
        let mut toks = HashSet::new();
        opt::token_set(&render_stmts(stmts), &mut toks);
        toks
    });
    let mut plan = Plan {
        route: HashMap::new(),
        prelude: HashMap::new(),
        tables: HashMap::new(),
    };
    for (tbl, _, members) in chosen {
        let existed = st.tables.contains_key(&tbl);
        let name = match st.tables.get(&tbl) {
            Some(n) => n.clone(),
            None => {
                let n = (0..)
                    .map(|i| format!("_uv{i}"))
                    .find(|cand| !used.contains(cand))
                    .expect("an unused table name exists");
                used.insert(n.clone());
                st.tables.insert(tbl, n.clone());
                n
            }
        };
        let decl = plan.tables.entry(tbl).or_insert_with(|| TableDecl {
            name: name.clone(),
            declare: !existed,
            params: Vec::new(),
            prelude: Vec::new(),
        });
        let next = st.next_slot.entry(tbl).or_insert(1);
        for cap in members {
            let lv = format!("{name}[{}]", *next);
            *next += 1;
            match cap {
                Cap::Bind(b) => {
                    let bd = &an.binds[b.0];
                    if bd.kind == BindKind::Param {
                        decl.params.push((lv.clone(), bd.name.clone()));
                    }
                    plan.route.insert(b, lv);
                }
                Cap::Prelude(n) => {
                    decl.prelude.push((lv.clone(), n.clone()));
                    plan.prelude.insert((f, n), lv);
                }
            }
        }
    }
    Ok(plan)
}

// ---- The walk: one traversal, used to analyze and to rewrite ----

/// One function scope on the walk path (frame 0 is the chunk).
struct Frame {
    fn_id: Option<FnId>,
    /// The nearest enclosing named function's header, for diagnostics.
    label: String,
    /// The names bound so far, one map per open block, innermost last.
    blocks: Vec<HashMap<String, BindId>>,
    owner_locals: usize,
}

struct Walk<'a> {
    frames: Vec<Frame>,
    /// Open table blocks, innermost last.
    tbl_stack: Vec<TblId>,
    next_bind: usize,
    next_fn: usize,
    next_tbl: usize,
    an: Analysis,
    plan: Option<&'a Plan>,
}

/// What the walk did to a statement.
enum Edit {
    Keep,
    Remove,
    Replace(Vec<Stmt>),
}

impl<'a> Walk<'a> {
    fn new(plan: Option<&'a Plan>) -> Self {
        Walk {
            frames: Vec::new(),
            tbl_stack: Vec::new(),
            next_bind: 0,
            next_fn: 0,
            next_tbl: 0,
            an: Analysis::default(),
            plan,
        }
    }

    fn walk_chunk(&mut self, stmts: &mut Vec<Stmt>) {
        let owner = opt::count_locals(stmts);
        self.frames.push(Frame {
            fn_id: None,
            label: "the module body".to_string(),
            blocks: Vec::new(),
            owner_locals: owner,
        });
        let t = self.open_table(owner);
        self.walk_block(stmts, Some(t));
        self.close_table();
        self.frames.pop();
    }

    fn open_table(&mut self, owner_locals: usize) -> TblId {
        let t = TblId(self.next_tbl);
        self.next_tbl += 1;
        self.tbl_stack.push(t);
        self.an.tbl_opaque.push(HashSet::new());
        self.an.tbl_owner_locals.push(owner_locals);
        t
    }

    fn close_table(&mut self) {
        self.tbl_stack.pop();
    }

    /// Declare `name` in the innermost open block.
    fn bind(&mut self, name: &str, kind: BindKind) -> BindId {
        let id = BindId(self.next_bind);
        self.next_bind += 1;
        let tbl = *self.tbl_stack.last().expect("a table block is open");
        self.an.binds.push(Binding {
            name: name.to_string(),
            kind,
            tbl,
        });
        let frame = self.frames.last_mut().expect("a frame is open");
        frame
            .blocks
            .last_mut()
            .expect("a block is open")
            .insert(name.to_string(), id);
        id
    }

    /// Record `cap` as an upvalue of every function frame from `from` on.
    fn capture(&mut self, from: usize, cap: Cap) {
        for fr in &self.frames[from.min(self.frames.len())..] {
            if let Some(f) = fr.fn_id {
                self.an.fn_captures[f.0].insert(cap.clone());
            }
        }
    }

    /// Resolve a plain mention of `name` at the current position, record
    /// the captures it costs, and return the lvalue that replaces it when
    /// the plan routes its binding.
    fn reference(&mut self, name: &str) -> Option<String> {
        let cur = self.frames.len() - 1;
        for fi in (0..=cur).rev() {
            let hit = self.frames[fi]
                .blocks
                .iter()
                .rev()
                .find_map(|b| b.get(name).copied());
            if let Some(b) = hit {
                if fi < cur {
                    self.capture(fi + 1, Cap::Bind(b));
                }
                return self.plan.and_then(|p| p.route.get(&b).cloned());
            }
        }
        if runtime::binds_name(name) || name == BIGLIT_TABLE {
            self.capture(1, Cap::Prelude(name.to_string()));
            let plan = self.plan?;
            return self
                .frames
                .iter()
                .filter_map(|fr| fr.fn_id)
                .find_map(|f| plan.prelude.get(&(f, name.to_string())).cloned());
        }
        None
    }

    /// Opaque text: every identifier token counts as a mention (never
    /// rewritten) and poisons routing of a same-named binding in every
    /// open table block.
    fn opaque(&mut self, text: &str) {
        let mut toks = HashSet::new();
        opt::token_set(text, &mut toks);
        for tok in toks {
            let _ = self.reference(&tok);
            for t in &self.tbl_stack {
                self.an.tbl_opaque[t.0].insert(tok.clone());
            }
        }
    }

    /// An assignment target: a plain name resolves like a reference (and
    /// is noted as assigned); anything else is opaque.
    fn lvalue(&mut self, lhs: &str) -> Option<String> {
        if is_plain_ident(lhs) {
            self.an.assigned.insert(lhs.to_string());
            self.reference(lhs)
        } else {
            self.opaque(lhs);
            None
        }
    }

    /// `named`: the header of a `Stmt::Function`; a literal is labelled by
    /// the named function it sits in.
    fn walk_function(&mut self, named: Option<String>, params: &[String], body: &mut Vec<Stmt>) {
        let f = FnId(self.next_fn);
        self.next_fn += 1;
        self.an.fn_captures.push(BTreeSet::new());
        let label = match named {
            Some(header) => format!("the function `{header}`"),
            None => {
                let outer = &self.frames.last().expect("a frame is open").label;
                if outer.starts_with("a function literal in ") {
                    outer.clone()
                } else {
                    format!("a function literal in {outer}")
                }
            }
        };
        self.an.fn_labels.push(label.clone());
        let owner = opt::count_locals(body) + params.len();
        self.frames.push(Frame {
            fn_id: Some(f),
            label,
            blocks: vec![HashMap::new()],
            owner_locals: owner,
        });
        let t = self.open_table(owner);
        for p in params {
            self.bind(p, BindKind::Param);
        }
        self.walk_block(body, Some(t));
        self.close_table();
        self.frames.pop();
    }

    /// Walk one block. With `tbl`, the block owns a routing table: the
    /// rewrite inserts its declaration and entry copies at the block's
    /// start (after an earlier round's declaration), and the walk does not
    /// visit what it inserted — a copy `slot = param` must keep reading
    /// the parameter it is the one remaining read of.
    fn walk_block(&mut self, stmts: &mut Vec<Stmt>, tbl: Option<TblId>) {
        self.frames
            .last_mut()
            .expect("a frame is open")
            .blocks
            .push(HashMap::new());
        let mut skip = 0..0;
        if let (Some(t), Some(plan)) = (tbl, self.plan)
            && let Some(decl) = plan.tables.get(&t)
        {
            let mut pre = Vec::new();
            if decl.declare {
                pre.push(Stmt::Local(
                    vec![decl.name.clone()],
                    Some(Expr::Table(vec![])),
                ));
            }
            for (lv, p) in &decl.params {
                pre.push(Stmt::Assign(lv.clone(), Expr::Name(p.clone())));
            }
            for (lv, n) in &decl.prelude {
                pre.push(Stmt::Assign(lv.clone(), Expr::Name(n.clone())));
            }
            let at = if decl.declare {
                0
            } else {
                debug_assert!(
                    matches!(stmts.first(), Some(Stmt::Local(names, _))
                        if names.len() == 1 && names[0] == decl.name),
                    "an existing routing table is declared first in its block"
                );
                1
            };
            skip = at..at + pre.len();
            stmts.splice(at..at, pre);
        }
        let mut i = 0;
        while i < stmts.len() {
            if skip.contains(&i) {
                i += 1;
                continue;
            }
            match self.walk_stmt(&mut stmts[i]) {
                Edit::Keep => i += 1,
                Edit::Remove => {
                    stmts.remove(i);
                    if skip.start > i {
                        skip = skip.start - 1..skip.end - 1;
                    }
                }
                Edit::Replace(with) => {
                    let n = with.len();
                    stmts.splice(i..i + 1, with);
                    i += n;
                }
            }
        }
        self.frames
            .last_mut()
            .expect("a frame is open")
            .blocks
            .pop();
    }

    fn walk_stmt(&mut self, s: &mut Stmt) -> Edit {
        match s {
            Stmt::Raw(t) => self.opaque(t),
            Stmt::Local(names, init) => {
                if let Some(e) = init {
                    self.walk_expr(e);
                }
                if init.is_none() {
                    self.an.assigned.extend(names.iter().cloned());
                }
                let ids: Vec<BindId> = names
                    .iter()
                    .map(|n| self.bind(n, BindKind::Local))
                    .collect();
                if let Some(plan) = self.plan {
                    let lvs: Vec<Option<&String>> =
                        ids.iter().map(|id| plan.route.get(id)).collect();
                    if lvs.iter().any(|l| l.is_some()) {
                        let unrouted: Vec<String> = names
                            .iter()
                            .zip(&lvs)
                            .filter(|(_, l)| l.is_none())
                            .map(|(n, _)| n.clone())
                            .collect();
                        return match init {
                            None if unrouted.is_empty() => Edit::Remove,
                            None => Edit::Replace(vec![Stmt::Local(unrouted, None)]),
                            Some(e) => {
                                let lhs: Vec<String> = names
                                    .iter()
                                    .zip(&lvs)
                                    .map(|(n, l)| l.cloned().unwrap_or_else(|| n.clone()))
                                    .collect();
                                let mut out = Vec::new();
                                if !unrouted.is_empty() {
                                    out.push(Stmt::Local(unrouted, None));
                                }
                                if lhs.len() == 1 {
                                    out.push(Stmt::Assign(
                                        lhs.into_iter().next().unwrap(),
                                        e.clone(),
                                    ));
                                } else {
                                    out.push(Stmt::MultiAssign(lhs, vec![e.clone()]));
                                }
                                Edit::Replace(out)
                            }
                        };
                    }
                }
            }
            Stmt::Assign(lhs, e) => {
                if let Some(lv) = self.lvalue(lhs) {
                    *lhs = lv;
                }
                self.walk_expr(e);
            }
            Stmt::AssignIf {
                lhs,
                cond,
                then_e,
                else_e,
            } => {
                if let Some(lv) = self.lvalue(lhs) {
                    *lhs = lv;
                }
                self.walk_expr(cond);
                self.walk_expr(then_e);
                self.walk_expr(else_e);
            }
            Stmt::MultiAssign(lhs, exprs) => {
                for l in lhs.iter_mut() {
                    if let Some(lv) = self.lvalue(l) {
                        *l = lv;
                    }
                }
                for e in exprs {
                    self.walk_expr(e);
                }
            }
            Stmt::Return(e) | Stmt::Expr(e) => self.walk_expr(e),
            Stmt::If {
                cond,
                then_b,
                elseifs,
                else_b,
            } => {
                self.walk_expr(cond);
                self.walk_block(&mut then_b.0, None);
                for (c, b) in elseifs {
                    self.walk_expr(c);
                    self.walk_block(&mut b.0, None);
                }
                if let Some(b) = else_b {
                    self.walk_block(&mut b.0, None);
                }
            }
            Stmt::Do(b) => self.walk_block(&mut b.0, None),
            Stmt::WhileTrue(b) => {
                let owner = self.frames.last().expect("a frame is open").owner_locals;
                let t = self.open_table(owner);
                self.walk_block(&mut b.0, Some(t));
                self.close_table();
            }
            Stmt::Function {
                target,
                params,
                body,
            } => {
                match target {
                    // `local function f` binds f for its own body too.
                    FnTarget::LocalFn(n) => {
                        let id = self.bind(n, BindKind::LocalFn);
                        if let Some(lv) = self.plan.and_then(|p| p.route.get(&id).cloned()) {
                            *target = FnTarget::Assigned(lv);
                        }
                    }
                    FnTarget::Assigned(lhs) => {
                        if let Some(lv) = self.lvalue(lhs) {
                            *target = FnTarget::Assigned(lv);
                        }
                    }
                    FnTarget::Slot(_) => self.opaque(FN_TABLE),
                    FnTarget::ThunkSlot(_) => self.opaque(TKF_TABLE),
                }
                let header = target.header_text(params);
                self.walk_function(Some(header), params, &mut body.0);
            }
            Stmt::ReturnTable(entries) => {
                for (_, e) in entries {
                    self.walk_expr(e);
                }
            }
            Stmt::ReturnNone | Stmt::Goto(_) | Stmt::Label(_) => {}
        }
        Edit::Keep
    }

    fn walk_expr(&mut self, e: &mut Expr) {
        match e {
            Expr::Name(s) => {
                if is_plain_ident(s) {
                    if let Some(lv) = self.reference(s) {
                        *s = lv;
                    }
                } else {
                    self.opaque(s);
                }
            }
            // A literal is not a reference — except the hoisted big-integer
            // literal, which the emitter spells as a `Lit` indexing the
            // chunk-level `__mll_biglit` table (see `BigLitPool`): that is
            // a mention of the table, routed like any other name (as a
            // `Name` — no pass follows this one to care about the node).
            Expr::Lit(s) => {
                if let Some(suffix) = s.strip_prefix(BIGLIT_TABLE)
                    && let Some(lv) = self.reference(BIGLIT_TABLE)
                {
                    *e = Expr::Name(format!("{lv}{suffix}"));
                }
            }
            Expr::Raw(t) => self.opaque(t),
            Expr::Index(base, suffix) => {
                self.walk_expr(base);
                self.opaque(suffix);
            }
            Expr::Func(params, body) => self.walk_function(None, params, body.stmts_mut()),
            // The method name and a table key are fields, not variables.
            Expr::Paren(_)
            | Expr::Neg(_)
            | Expr::Call(..)
            | Expr::Method(..)
            | Expr::Binop(..)
            | Expr::Table(_)
            | Expr::TableSpaced(_) => e.for_each_subexpr_mut(&mut |c| self.walk_expr(c)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::lua::{Block, FuncBody};

    /// `local x1 … xN = 1 … N` in a named function, then a lambda summing
    /// them all.
    fn lambda_over_locals(n: usize) -> Vec<Stmt> {
        let mut body = Vec::new();
        for i in 1..=n {
            body.push(Stmt::Local(
                vec![format!("x{i}")],
                Some(Expr::lit(i.to_string())),
            ));
        }
        let sum = (1..=n)
            .map(|i| Expr::name(format!("x{i}")))
            .reduce(|a, b| Expr::binop("+", a, b))
            .unwrap();
        body.push(Stmt::Return(Expr::call_named(
            "map",
            vec![
                Expr::Func(
                    vec!["k".into()],
                    FuncBody::Inline(vec![Stmt::Return(Expr::binop("+", sum, Expr::name("k")))]),
                ),
                Expr::name("ks"),
            ],
        )));
        vec![Stmt::Function {
            target: FnTarget::LocalFn("f".into()),
            params: vec!["ks".into()],
            body: Block(body),
        }]
    }

    #[test]
    fn under_budget_is_untouched() {
        let mut stmts = lambda_over_locals(UPVALUE_BUDGET);
        let before = render_stmts(&stmts);
        assert!(!run(&mut stmts).unwrap());
        assert_eq!(before, render_stmts(&stmts));
    }

    #[test]
    fn over_budget_routes_just_enough_through_one_table() {
        let mut stmts = lambda_over_locals(UPVALUE_BUDGET + 1);
        assert!(run(&mut stmts).unwrap());
        let out = render_stmts(&stmts);
        // Two names routed: the table costs one upvalue, so the lambda is
        // at the budget — `local _uv0 = {}` first, x1 and x2 become slots,
        // x3 stays a local, and every mention is rewritten.
        assert!(
            out.contains(
                "    local _uv0 = {}\n    _uv0[1] = 1\n    _uv0[2] = 2\n    local x3 = 3\n"
            ),
            "{out}"
        );
        let mut toks = HashSet::new();
        opt::token_set(&out, &mut toks);
        assert!(!toks.contains("x1") && !toks.contains("x2"), "{out}");
        assert!(out.contains("return _uv0[1] + _uv0[2] + x3 + x4"), "{out}");
        let an = analyze(&mut stmts);
        assert!(an.fn_captures.iter().all(|c| c.len() <= UPVALUE_BUDGET));
    }

    /// A capture mentioned in opaque text cannot be routed; the others are.
    #[test]
    fn opaque_mention_blocks_that_name_only() {
        let mut stmts = lambda_over_locals(UPVALUE_BUDGET + 1);
        if let Stmt::Function { body, .. } = &mut stmts[0] {
            body.0.insert(0, Stmt::Raw("-- x1".into()));
        }
        assert!(run(&mut stmts).unwrap());
        let out = render_stmts(&stmts);
        assert!(
            out.contains("local x1 = 1\n    _uv0[1] = 2\n    _uv0[2] = 3\n"),
            "{out}"
        );
    }

    /// A routed variable is one cell: the in-place force `x = __force(x)`
    /// after the closure that captures `x` is made rewrites both sides.
    #[test]
    fn rebound_capture_is_rewritten_on_both_sides() {
        let mut stmts = lambda_over_locals(UPVALUE_BUDGET + 1);
        if let Stmt::Function { body, .. } = &mut stmts[0] {
            let lambda = body.0.pop().unwrap();
            body.0.push(Stmt::Local(
                vec!["g".into()],
                Some(match lambda {
                    Stmt::Return(e) => e,
                    _ => unreachable!(),
                }),
            ));
            body.0
                .push(Stmt::Assign("x1".into(), Expr::force(Expr::name("x1"))));
            body.0.push(Stmt::Return(Expr::call(
                Expr::name("g"),
                vec![Expr::name("x1")],
            )));
        }
        assert!(run(&mut stmts).unwrap());
        let out = render_stmts(&stmts);
        assert!(
            out.contains("    _uv0[1] = __force(_uv0[1])\n    return g(_uv0[1])\n"),
            "{out}"
        );
    }

    /// A function whose bindings spilled past `LOCAL_LIMIT` still has room
    /// for its routing table under the host's limit; a function at the
    /// host's limit is reported, not emitted over budget.
    #[test]
    fn spilled_owner_gets_a_table_and_a_full_owner_is_an_error() {
        // 180 plain locals, the spill table, then a lambda over sixty of
        // them: 181 declared locals, the table makes 182.
        let mut stmts = lambda_over_locals(UPVALUE_BUDGET + 10);
        if let Stmt::Function { body, .. } = &mut stmts[0] {
            for i in 0..(180 - (UPVALUE_BUDGET + 10)) {
                body.0
                    .insert(0, Stmt::Local(vec![format!("f{i}")], Some(Expr::lit("0"))));
            }
            body.0
                .insert(0, Stmt::Local(vec!["_v".into()], Some(Expr::Table(vec![]))));
            assert_eq!(opt::count_locals(&body.0), 181);
        }
        assert!(run(&mut stmts).unwrap());
        let an = analyze(&mut stmts);
        assert!(an.fn_captures.iter().all(|c| c.len() <= UPVALUE_BUDGET));
        // At the host's limit there is no room: a compile error names the
        // function, the count and the captures.
        let mut stmts = lambda_over_locals(UPVALUE_BUDGET + 10);
        if let Stmt::Function { body, .. } = &mut stmts[0] {
            for i in 0..(LUA_MAX_LOCALS - (UPVALUE_BUDGET + 10)) {
                body.0
                    .insert(0, Stmt::Local(vec![format!("f{i}")], Some(Expr::lit("0"))));
            }
        }
        let err = run(&mut stmts).unwrap_err();
        assert!(
            err.contains("`local function f(ks)`") && err.contains("would need 60 upvalues"),
            "{err}"
        );
        assert!(err.contains("x1, x10, x11"), "{err}");
    }

    /// A loop body's local is fresh per iteration, so its table is the
    /// loop body's, declared inside the loop.
    #[test]
    fn loop_locals_route_to_a_per_iteration_table() {
        let mut body = Vec::new();
        for i in 1..=UPVALUE_BUDGET + 1 {
            body.push(Stmt::Local(
                vec![format!("x{i}")],
                Some(Expr::lit(i.to_string())),
            ));
        }
        let sum = (1..=UPVALUE_BUDGET + 1)
            .map(|i| Expr::name(format!("x{i}")))
            .reduce(|a, b| Expr::binop("+", a, b))
            .unwrap();
        body.push(Stmt::Return(Expr::inline_fn0(sum)));
        let mut stmts = vec![Stmt::Function {
            target: FnTarget::LocalFn("f".into()),
            params: vec![],
            body: Block(vec![Stmt::WhileTrue(Block(body))]),
        }];
        assert!(run(&mut stmts).unwrap());
        let out = render_stmts(&stmts);
        assert!(
            out.contains("    while true do\n        local _uv0 = {}\n        _uv0[1] = 1\n"),
            "{out}"
        );
    }

    /// A parameter is copied into its slot at entry and read from the slot
    /// after; a prelude helper is copied into the chunk's table.
    #[test]
    fn params_and_prelude_names_route_by_copy() {
        let n = UPVALUE_BUDGET + 2;
        let params: Vec<String> = (1..=n).map(|i| format!("p{i}")).collect();
        let sum = (1..=n)
            .map(|i| Expr::force(Expr::name(format!("p{i}"))))
            .reduce(|a, b| Expr::binop("+", a, b))
            .unwrap();
        let mut stmts = vec![Stmt::Function {
            target: FnTarget::LocalFn("f".into()),
            params: params.clone(),
            body: Block(vec![Stmt::Return(Expr::inline_fn0(sum))]),
        }];
        assert!(run(&mut stmts).unwrap());
        let out = render_stmts(&stmts);
        // 52 params + __force = 53 captures: four routed (the table costs
        // one), the rest stay parameters.
        assert!(out.contains("    local _uv0 = {}\n    _uv0[1] = p1\n    _uv0[2] = p2\n    _uv0[3] = p3\n    _uv0[4] = p4\n    return function() return"), "{out}");
        assert!(out.contains("__force(_uv0[1]) + __force(_uv0[2]) + __force(_uv0[3]) + __force(_uv0[4]) + __force(p5) +"), "{out}");
        // The prelude name alone over budget: copied into the chunk table.
        let mut names: Vec<&str> = runtime::bound_names()
            .filter(|n| n.starts_with("__mll_") || *n == "__force")
            .collect();
        names.sort();
        names.truncate(UPVALUE_BUDGET + 1);
        assert_eq!(names.len(), UPVALUE_BUDGET + 1);
        let helpers: Vec<Expr> = names.iter().map(|n| Expr::call_named(n, vec![])).collect();
        let mut stmts = vec![Stmt::Function {
            target: FnTarget::LocalFn("g".into()),
            params: vec![],
            body: Block(vec![Stmt::Return(Expr::Table(
                helpers
                    .into_iter()
                    .map(crate::codegen::lua::Item::Pos)
                    .collect(),
            ))]),
        }];
        assert!(run(&mut stmts).unwrap());
        let out = render_stmts(&stmts);
        assert!(out.starts_with("local _uv0 = {}\n_uv0[1] = "), "{out}");
        assert!(out.contains("return {_uv0[1](), _uv0[2]()"), "{out}");
    }
}
