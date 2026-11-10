//! The host parser's two nesting budgets, measured over the finished Lua
//! tree exactly as the hosts charge them.
//!
//! A Lua chunk is refused at LOAD time, before anything runs, when its
//! syntax nests too deeply or one function needs too many registers:
//!
//!   * SYNTAX LEVELS. PUC Lua's parser calls `enterlevel` in `statement`
//!     and in `subexpr`, and the count shares `LUAI_MAXCCALLS` (200) with
//!     the C-call counter — the overflow is reported as "C stack
//!     overflow". LuaJIT keeps its own counter (`LJ_MAX_XLEVEL`, 200,
//!     "chunk has too many syntax levels") at the same grammar points.
//!     The level does NOT reset at a function literal: a thunk's body is
//!     parsed inside the expression that holds the thunk. On PUC the
//!     budget is further shared with the host's own call depth at load
//!     time — a chunk `require`d from inside `k` nested `pcall`s has `k`
//!     fewer levels (measured: 1 per frame).
//!   * REGISTER SLOTS. A function's active locals and the temporaries of
//!     the expression being compiled share one frame of at most
//!     `LJ_MAX_SLOTS` (250, "function or expression too complex") on
//!     LuaJIT and `MAXREGS` (255, "too many registers") on PUC. This one
//!     DOES reset at a function literal (its own prototype), and it is
//!     what fails first for nested calls: every pending call holds its
//!     callee, its frame link (two slots on a GC64 LuaJIT) and the
//!     arguments already evaluated.
//!
//! Measured per construct on both hosts (2026-11, Lua 5.5 and LuaJIT
//! 2.1, a chunk loaded directly by the interpreter; the deepest loadable
//! `n` of each shape):
//!
//! ```text
//!   shape                         PUC 5.5     LuaJIT     cost per level
//!   f(f(…(x)…))                   195 lv      123 sl     1 level; 2 slots (LJ)
//!   f(x, f(x, …))                 126 sl       82 sl     1 level; nargs+1 slots
//!   x + (x + (…))                  97 lv       98 lv     2 levels (operand + paren)
//!   (…(x + x) + x) + x            194 lv      195 lv     1 level (the paren)
//!   x .. (x .. (…))                97 lv       98 lv     2 levels, 1 slot
//!   x and (x and (…))              97 lv       98 lv     2 levels, no slots
//!   x and x and x …              flat        flat       0 (left-assoc loop)
//!   x .. x .. x …                 195 lv      196 lv     1 level (right-assoc)
//!   g(x, function() return …)      65 lv       65 lv     3 levels, slots reset
//!   f((function(_x) return …)(x))  48 lv       49 lv     4 levels
//!   -(-(…))                        97 lv       98 lv     2 levels
//!   t[t[…]]                       195 lv      196 lv     1 level
//!   if x then if x then …         195 lv      196 lv     1 level
//!   if … else if … end end        195 lv      196 lv     1 level
//!   if … elseif … elseif …       flat        flat       0
//!   {x, x, x, …}                 flat        flat       0 (flushed per 50)
//!   with 180 locals: f(f(…))       73 sl       33 sl     the locals' slots
//! ```
//!
//! `measure` charges the tree by those rules — statements, right-hand
//! binop operands, call arguments, parens, negations, table items and
//! index keys each open a level; a function literal opens a new slot
//! frame holding its parameters and `local`s — and reports the deepest
//! level and the fullest frame with the top-level statement they occur
//! in. Opaque text (`Raw` fragments, rendered index suffixes) is charged
//! by its bracket nesting, an upper bound for the shapes the emitters
//! write there. The split pass (split.rs) keeps every function under the
//! budgets below at the TIR level; this measurement is the check that it
//! did, over the tree the hosts will see: the test harness runs it on
//! every compiled program (`nesting_violations`).

use super::lua::{Expr, Item, Stmt};

/// Syntax levels a chunk of ours may nest. The hosts allow 200 from a
/// direct load; the remainder is what a host embedding the chunk may
/// spend on its own load-time call depth (PUC charges one level per
/// frame) and what a Raw fragment's text may hide from the count.
pub(super) const MAX_LEVELS: usize = 150;

/// Register slots one function literal may need, parameters and locals
/// included. LuaJIT's frame holds 250 (`LJ_MAX_SLOTS`), PUC's 255
/// (`MAXREGS`); the margin covers the frame's own bookkeeping slots and
/// the varargs base.
pub(super) const MAX_SLOTS: usize = 240;

/// The two maxima over a tree, each with the index of the top-level
/// statement it was found in.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Nesting {
    pub levels: usize,
    pub levels_at: usize,
    pub slots: usize,
    pub slots_at: usize,
    /// The most slots one statement's expressions needed above the locals
    /// active at that statement — the part of a frame that expression
    /// shape alone decides, which is what the split pass bounds.
    pub temps: usize,
}

/// Measure a whole chunk (the module body as `generate` prints it).
pub(super) fn measure(stmts: &[Stmt]) -> Nesting {
    let mut n = Nesting::default();
    // The chunk's own function frame: the body's top-level locals are
    // active for every later statement.
    let mut active = 0usize;
    for (i, s) in stmts.iter().enumerate() {
        let mut m = Measure { levels: 0, slots: 0, temps: 0, base: 0 };
        // A top-level statement sits at level 1 (the chunk's `statement`).
        m.stmt(s, 1, &mut active);
        if m.levels > n.levels {
            n.levels = m.levels;
            n.levels_at = i;
        }
        if m.slots > n.slots {
            n.slots = m.slots;
            n.slots_at = i;
        }
        n.temps = n.temps.max(m.temps);
    }
    n
}

/// The violations of the two budgets in a finished tree, as diagnostics
/// naming the statement — empty when the tree is within budget.
pub(super) fn violations(stmts: &[Stmt]) -> Vec<String> {
    let n = measure(stmts);
    let mut out = Vec::new();
    if n.levels > MAX_LEVELS {
        out.push(format!(
            "emitted Lua nests {} syntax levels (budget {}) in top-level statement {} — the host parser would refuse the chunk",
            n.levels, MAX_LEVELS, describe(stmts, n.levels_at)
        ));
    }
    if n.slots > MAX_SLOTS {
        out.push(format!(
            "emitted Lua needs {} register slots in one function (budget {}) in top-level statement {} — the host parser would refuse the chunk",
            n.slots, MAX_SLOTS, describe(stmts, n.slots_at)
        ));
    }
    out
}

fn describe(stmts: &[Stmt], i: usize) -> String {
    match stmts.get(i) {
        Some(Stmt::Function { target, params, .. }) => format!("{i} ({})", target.header_text(params)),
        Some(Stmt::Local(names, _)) => format!("{i} (local {})", names.join(", ")),
        Some(Stmt::Assign(lhs, _)) => format!("{i} ({lhs} = …)"),
        _ => i.to_string(),
    }
}

struct Measure {
    levels: usize,
    slots: usize,
    temps: usize,
    /// The locals active at the statement being measured.
    base: usize,
}

impl Measure {
    fn note_level(&mut self, lvl: usize) {
        if lvl > self.levels {
            self.levels = lvl;
        }
    }

    fn note_slots(&mut self, s: usize) {
        if s > self.slots {
            self.slots = s;
        }
        let t = s.saturating_sub(self.base);
        if t > self.temps {
            self.temps = t;
        }
    }

    /// A statement at level `lvl` (its `statement` call), in a frame with
    /// `active` locals; declarations extend `active` for the statements
    /// that follow in the same block.
    fn stmt(&mut self, s: &Stmt, lvl: usize, active: &mut usize) {
        self.note_level(lvl);
        self.base = *active;
        match s {
            Stmt::Raw(text) => {
                self.note_level(lvl + bracket_depth(text) + 1);
                self.note_slots(*active + bracket_depth(text) + 2);
                *active += raw_local_count(text);
            }
            Stmt::Local(names, init) => {
                if let Some(e) = init {
                    self.expr(e, lvl + 1, *active);
                }
                *active += names.len();
            }
            Stmt::Assign(_, e) | Stmt::Return(e) | Stmt::Expr(e) => {
                self.expr(e, lvl + 1, *active);
            }
            Stmt::AssignIf { cond, then_e, else_e, .. } => {
                self.expr(cond, lvl + 1, *active);
                self.expr(then_e, lvl + 1, *active);
                self.expr(else_e, lvl + 1, *active);
            }
            Stmt::If { cond, then_b, elseifs, else_b } => {
                self.expr(cond, lvl + 1, *active);
                self.block(&then_b.0, lvl, *active);
                for (c, b) in elseifs {
                    self.base = *active;
                    self.expr(c, lvl + 1, *active);
                    self.block(&b.0, lvl, *active);
                }
                if let Some(b) = else_b {
                    self.block(&b.0, lvl, *active);
                }
            }
            Stmt::Do(b) | Stmt::WhileTrue(b) => self.block(&b.0, lvl, *active),
            Stmt::MultiAssign(_, exprs) => {
                // Every value of the list is held until the assignment.
                for (i, e) in exprs.iter().enumerate() {
                    self.expr(e, lvl + 1, *active + i);
                }
            }
            Stmt::ReturnNone | Stmt::Goto(_) | Stmt::Label(_) => {}
            Stmt::Function { target, params, body } => {
                if matches!(target, super::lua::FnTarget::LocalFn(_)) {
                    *active += 1;
                }
                self.function(params.len(), &body.0, lvl);
            }
            Stmt::ReturnTable(entries) => {
                for (_, e) in entries {
                    self.expr(e, lvl + 1, *active);
                }
            }
        }
    }

    /// A block's statements each open a level above the enclosing
    /// statement; its locals end with it.
    fn block(&mut self, stmts: &[Stmt], lvl: usize, active: usize) {
        let mut active = active;
        for s in stmts {
            self.stmt(s, lvl + 1, &mut active);
        }
    }

    /// A function body: a fresh frame holding the parameters; its
    /// statements nest one level above the literal's own position.
    fn function(&mut self, nparams: usize, body: &[Stmt], lvl: usize) {
        let saved = self.base;
        self.base = nparams;
        self.note_slots(nparams);
        self.block(body, lvl, nparams);
        self.base = saved;
    }

    /// An expression parsed by a `subexpr` call at level `lvl`, in a frame
    /// with `used` slots already taken (locals and pending temporaries).
    fn expr(&mut self, e: &Expr, lvl: usize, used: usize) {
        self.note_level(lvl);
        self.note_slots(used + 1);
        match e {
            Expr::Name(_) | Expr::Lit(_) => {}
            Expr::Raw(text) => {
                self.note_level(lvl + bracket_depth(text));
                self.note_slots(used + bracket_depth(text) + 2);
            }
            Expr::Paren(inner) => self.expr(inner, lvl + 1, used),
            Expr::Call(f, args) => {
                self.prefix(f, lvl, used);
                // The callee and its frame link take two slots; each
                // argument is evaluated above the ones before it.
                for (i, a) in args.iter().enumerate() {
                    self.expr(a, lvl + 1, used + 2 + i);
                }
            }
            Expr::Method(recv, _, args) => {
                self.prefix(recv, lvl, used);
                for (i, a) in args.iter().enumerate() {
                    self.expr(a, lvl + 1, used + 3 + i);
                }
            }
            Expr::Index(base, suffix) => {
                self.prefix(base, lvl, used);
                let d = bracket_depth(suffix);
                self.note_level(lvl + d);
                self.note_slots(used + 1 + d);
            }
            Expr::Binop(op, l, r) => {
                // The left operand is read by the same `subexpr`; the
                // right one by a nested call. A value operator keeps the
                // left value in a slot while the right is built; `and`/
                // `or` branch instead and hold nothing.
                self.expr(l, lvl, used);
                let held = if op == "and" || op == "or" { 0 } else { 1 };
                self.expr(r, lvl + 1, used + held);
            }
            Expr::Neg(inner) => self.expr(inner, lvl + 1, used),
            Expr::Table(items) | Expr::TableSpaced(items) => {
                // The constructor holds the table and up to fifty pending
                // items (`LFIELDS_PER_FLUSH`) while an item is built.
                for (i, item) in items.iter().enumerate() {
                    let e = match item {
                        Item::Pos(e) | Item::KV(_, e) => e,
                    };
                    self.expr(e, lvl + 1, used + 1 + (i % 50));
                }
            }
            Expr::Func(params, body) => self.function(params.len(), body.stmts(), lvl),
        }
    }

    /// A callee / index base / method receiver: Lua's `prefixexp`, read
    /// by the same `subexpr` without opening a level of its own.
    fn prefix(&mut self, e: &Expr, lvl: usize, used: usize) {
        match e {
            Expr::Paren(inner) => self.expr(inner, lvl + 1, used),
            other => self.expr(other, lvl, used),
        }
    }
}

/// The deepest bracket nesting of opaque text — each `(`, `[` and `{`
/// opens a parser level in the shapes the emitters write there.
fn bracket_depth(text: &str) -> usize {
    let mut depth = 0usize;
    let mut max = 0usize;
    for c in text.chars() {
        match c {
            '(' | '[' | '{' => {
                depth += 1;
                max = max.max(depth);
            }
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    max
}

/// The locals a raw statement line declares (`local a, b = …`).
fn raw_local_count(text: &str) -> usize {
    let Some(rest) = text.strip_prefix("local ") else { return 0 };
    if rest.starts_with("function ") {
        return 1;
    }
    let names = rest.split('=').next().unwrap_or("");
    names.split(',').filter(|s| !s.trim().is_empty()).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::lua::{Block, FnTarget};

    fn call(f: &str, args: Vec<Expr>) -> Expr {
        Expr::call(Expr::name(f), args)
    }

    /// `f(f(…(x)…))` nested `n` deep inside `return`: one level per call
    /// argument, two slots per pending call — the measured LuaJIT cost.
    #[test]
    fn nested_calls_charge_one_level_and_two_slots_each() {
        let mut e = Expr::name("x");
        for _ in 0..10 {
            e = call("f", vec![e]);
        }
        let stmts = vec![Stmt::Function {
            target: FnTarget::Slot(1),
            params: vec!["x".into()],
            body: Block(vec![Stmt::Return(e)]),
        }];
        let n = measure(&stmts);
        // chunk statement (1) + body statement (2) + return operand (3)
        // + ten nested argument levels.
        assert_eq!(n.levels, 13);
        // one parameter + ten pending calls at two slots + the innermost
        // `x` operand.
        assert_eq!(n.slots, 1 + 20 + 1);
    }

    /// A thunk argument costs three levels (argument, body statement,
    /// return operand) and starts a fresh slot frame.
    #[test]
    fn thunk_bodies_add_levels_but_reset_slots() {
        let mut e = Expr::name("x");
        for _ in 0..5 {
            e = call("g", vec![Expr::name("x"), Expr::inline_fn0(e)]);
        }
        let stmts = vec![Stmt::Return(e)];
        let n = measure(&stmts);
        assert_eq!(n.levels, 2 + 5 * 3);
        // The outermost call: callee + link, `x`, the closure (slot 4),
        // and each thunk body's own frame is just the `return` operand.
        assert_eq!(n.slots, 4);
    }

    /// A flat `and` chain and a flat table constructor nest nothing.
    #[test]
    fn flat_chains_stay_flat() {
        let chain = Expr::and_chain((0..200).map(|_| Expr::name("x")).collect());
        let n = measure(&[Stmt::Return(chain)]);
        assert_eq!(n.levels, 3);
        let table = Expr::Table((0..200).map(|_| Item::Pos(Expr::name("x"))).collect());
        let n = measure(&[Stmt::Return(table)]);
        assert_eq!(n.levels, 3);
        assert!(n.slots <= 52, "items flush every fifty: {}", n.slots);
    }

    /// Right-nested value binops with explicit parens cost two levels and
    /// one slot per level; `else { if }` nesting costs one level per `if`.
    #[test]
    fn right_nested_binops_and_if_chains() {
        let mut e = Expr::name("x");
        for _ in 0..10 {
            e = Expr::binop("+", Expr::name("x"), Expr::paren(e));
        }
        let n = measure(&[Stmt::Return(e)]);
        assert_eq!(n.levels, 2 + 20);
        assert_eq!(n.slots, 11);
        let mut s = Stmt::Return(Expr::name("x"));
        for _ in 0..10 {
            s = Stmt::If {
                cond: Expr::name("c"),
                then_b: Block(vec![Stmt::Return(Expr::name("x"))]),
                elseifs: vec![],
                else_b: Some(Block(vec![s])),
            };
        }
        let n = measure(&[s]);
        assert_eq!(n.levels, 1 + 10 + 1);
    }

    /// Locals occupy the frame for the statements after them, and a
    /// block's locals end with the block.
    #[test]
    fn locals_fill_the_frame() {
        let body = vec![
            Stmt::Local(vec!["a".into(), "b".into()], None),
            Stmt::Do(Block(vec![
                Stmt::Local(vec!["c".into()], Some(Expr::name("x"))),
                Stmt::Return(call("f", vec![Expr::name("c")])),
            ])),
            Stmt::Return(call("f", vec![Expr::name("a")])),
        ];
        let stmts = vec![Stmt::Function {
            target: FnTarget::Slot(1),
            params: vec!["x".into()],
            body: Block(body),
        }];
        let n = measure(&stmts);
        // Inside the `do`: x, a, b, c active, then f + link + argument.
        assert_eq!(n.slots, 4 + 3);
        // The expression part alone: the call's three slots.
        assert_eq!(n.temps, 3);
    }
}
