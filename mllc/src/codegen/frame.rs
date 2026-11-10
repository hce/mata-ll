//! Pass 8 — the frame budget: how many parameters one emitted function
//! names, and how many arguments one emitted call passes in registers.
//!
//! Both counts follow the source shape, and both are bounded by the host:
//!
//!   * A parameter is a local of its function, and a function may have at
//!     most 200 locals active at once (`MAXVARS` in lparser.c, which
//!     refuses the 200th; `LJ_MAX_LOCVAR` in lj_def.h) — "too many local
//!     variables" / "function at line N has more than 200 local variables"
//!     at LOAD time, for the whole chunk. The emitter budgets the locals
//!     it declares (`LOCAL_LIMIT`, the `_v` spill, parameters counted)
//!     but a parameter list cannot spill: a constructor of 205 fields is
//!     a function of 205 parameters, as is a function of 205 arguments, a
//!     lambda over 205 names or the closure a partial application of
//!     either builds over its remaining positions.
//!   * A call's arguments are built in consecutive registers above the
//!     caller's active locals, together with the callee and its frame
//!     link, and one frame holds 250 (`LJ_MAX_SLOTS`; PUC's `MAXREGS` is
//!     255) — "function or expression too complex" / "too many registers".
//!     The split pass (split.rs) bounds how deep an expression nests, and
//!     hoists a call too wide for its clause's frame into a binding of its
//!     own; it cannot make the call narrower, and a call at the local
//!     limit's frame fits about 57 arguments.
//!
//! Over the finished tree, after every pass that reads or rewrites
//! parameter lists (the loop conversions update parameters by name) and
//! before the upvalue budget (a routed parameter table is one capture
//! where the parameters were many):
//!
//!   * A function with more than [`PARAM_BUDGET`] parameters is rewritten
//!     to take them as varargs: `function(p1, …, pN) body` becomes
//!     `function(...) local _va = {...} body'`, where every mention of
//!     parameter `k` in the body — a read, an assignment target, a
//!     multiple-assignment target, a function header's assignment target —
//!     is `_va[k]`, resolved by Lua's scoping (an inner binder of the same
//!     name shadows the parameter and is left alone; a `local function` or
//!     `local` of that name inside the body rebinds it from there on).
//!     The varargs keep every position, nil included: `{...}` stores
//!     `select(k, ...)` at `k`, and a nil field reads back as nil, exactly
//!     as the parameter would have. Callers are untouched: the function is
//!     still called with N positional arguments, by a saturated call, by
//!     a partial application's closure or through the runtime's curried
//!     dispatch, and takes them as before.
//!   * A call with more than [`ARG_BUDGET`] arguments passes them through
//!     a table: `f(a1, …, aN)` becomes `f(__unpack({a1, …, aN}, 1, N))`.
//!     The table constructor evaluates its items left to right, the order
//!     the call evaluated its arguments, flushing every fifty into the
//!     table so it holds at most about fifty registers whatever N is; the
//!     explicit bounds `1, N` hand the callee exactly N values, nil
//!     arguments included (`#` would stop at the first). Every argument
//!     of an emitted call is single-valued — the paren pass keeps the
//!     truncating paren around a call whose callee might return several
//!     values in the last position, and drops it only where the callee
//!     provably returns one — so packing changes no argument's value and
//!     no argument count. A method call is packed the same way.
//!
//! A function within both budgets and a call within the second are left
//! as they are: the pass is a no-op over every program whose shapes fit,
//! and the output is byte-identical to the tree it was given. A parameter
//! the pass must route but cannot see — one mentioned inside opaque text
//! (a `Raw` fragment, a rendered index suffix, a table key, a composite
//! name) — is reported as a compile error naming the function, since a
//! chunk the host refuses to load is the alternative; the emitter writes
//! such text for FFI descriptors and a few rendered paths, never for a
//! parameter of a user function.

use std::collections::{HashMap, HashSet};

use super::annot::is_plain_ident;
use super::lua::{Expr, FnTarget, Item, Stmt, render_stmts};
use super::opt;

/// The most parameters an emitted function names. The host's limit is on
/// locals — parameters and `local`s together — and the emitter keeps its
/// declared locals under `LOCAL_LIMIT` with the parameters counted, so a
/// function at this budget still has room for the spill table and the
/// locals the later passes add (a routing table, a hoisted closure).
pub(super) const PARAM_BUDGET: usize = super::CodeGen::LOCAL_LIMIT;

/// The most arguments an emitted call passes in registers. A call needs
/// its callee, the frame link and one register per argument above the
/// caller's active locals; the emitter's frames hold at most `LOCAL_LIMIT`
/// locals plus the spill table, and the tree is checked against
/// `nesting::MAX_SLOTS`, so a call this wide fits at statement level in
/// every frame the emitter produces. The packed form costs the same frame
/// about 55 registers (the callee, the `__unpack` call and a fifty-item
/// flush), also within the budget.
pub(super) const ARG_BUDGET: usize =
    super::nesting::MAX_SLOTS - (super::CodeGen::LOCAL_LIMIT + 1) - 2;

/// Rewrite every function over the parameter budget and every call over
/// the argument budget. Returns whether the tree changed, or the
/// diagnostic for a parameter the pass cannot route.
pub(super) fn run(stmts: &mut [Stmt]) -> Result<bool, String> {
    // A read-only census first: nearly every program is within budget,
    // and then nothing is rendered or rewritten.
    if !needs_rewrite(stmts) {
        return Ok(false);
    }
    let mut used = HashSet::new();
    opt::token_set(&render_stmts(stmts), &mut used);
    let mut w = Walk {
        frames: Vec::new(),
        labels: vec!["the module body".to_string()],
        routed: 0,
        used,
        next_table: 1,
        error: None,
    };
    w.block(stmts);
    match w.error {
        Some(e) => Err(e),
        None => Ok(true),
    }
}

/// Does any function exceed the parameter budget, or any call the
/// argument budget?
fn needs_rewrite(stmts: &[Stmt]) -> bool {
    fn over_in_block(stmts: &[Stmt]) -> bool {
        stmts.iter().any(|s| {
            let mut found = matches!(s, Stmt::Function { params, .. } if params.len() > PARAM_BUDGET);
            s.for_each_expr(&mut |e| found |= over_in_expr(e));
            s.for_each_block(&mut |b| found |= over_in_block(b));
            found
        })
    }
    fn over_in_expr(e: &Expr) -> bool {
        let mut found = match e {
            Expr::Func(params, body) => params.len() > PARAM_BUDGET || over_in_block(body.stmts()),
            Expr::Call(_, args) | Expr::Method(_, _, args) => args.len() > ARG_BUDGET,
            _ => false,
        };
        e.for_each_subexpr(&mut |c| found |= over_in_expr(c));
        found
    }
    over_in_block(stmts)
}

/// A name's binding at the current position.
enum Binding {
    /// A parameter the enclosing function routed: its table slot.
    Routed(String),
    /// Any other local binding (or a free name).
    Plain,
}

struct Walk {
    /// One entry per open function (frame 0 is the chunk): the blocks
    /// open in it, innermost last, each mapping a bound name to its route.
    frames: Vec<Vec<HashMap<String, Option<String>>>>,
    /// The header of the nearest named function, for diagnostics.
    labels: Vec<String>,
    /// How many open functions routed their parameters: opaque text needs
    /// a mention check only while this is non-zero.
    routed: usize,
    /// Every identifier token of the module, for fresh table names.
    used: HashSet<String>,
    next_table: usize,
    error: Option<String>,
}

impl Walk {
    fn resolve(&self, name: &str) -> Binding {
        for frame in self.frames.iter().rev() {
            for block in frame.iter().rev() {
                if let Some(route) = block.get(name) {
                    return match route {
                        Some(lv) => Binding::Routed(lv.clone()),
                        None => Binding::Plain,
                    };
                }
            }
        }
        Binding::Plain
    }

    fn bind(&mut self, name: &str) {
        self.frames
            .last_mut()
            .expect("a frame is open")
            .last_mut()
            .expect("a block is open")
            .insert(name.to_string(), None);
    }

    /// A plain mention: the slot that replaces it when its binding is a
    /// routed parameter.
    fn reference(&self, name: &str) -> Option<String> {
        match self.resolve(name) {
            Binding::Routed(lv) => Some(lv),
            Binding::Plain => None,
        }
    }

    /// Opaque text cannot be rewritten; a routed parameter mentioned in it
    /// would keep reading the (now absent) parameter.
    fn opaque(&mut self, text: &str) {
        if self.routed == 0 || self.error.is_some() {
            return;
        }
        let mut toks = HashSet::new();
        opt::token_set(text, &mut toks);
        let mut hit: Vec<&String> = toks.iter().filter(|t| self.reference(t).is_some()).collect();
        if hit.is_empty() {
            return;
        }
        hit.sort();
        self.error = Some(format!(
            "{} has more than {PARAM_BUDGET} parameters — more than a Lua function may hold as \
             locals (the host allows 200) — and the compiler moves such a parameter list into a \
             table, but the parameter{} {} {} mentioned by a host-level fragment it cannot \
             rewrite (`{}`).\n  \
             note: group the parameters in a record or tuple; this is a limit of the Lua host, \
             not of Haskell.",
            self.labels.last().expect("a label"),
            if hit.len() == 1 { "" } else { "s" },
            hit.iter().map(|h| format!("`{h}`")).collect::<Vec<_>>().join(", "),
            if hit.len() == 1 { "is" } else { "are" },
            text,
        ));
    }

    fn lvalue(&mut self, lhs: &mut String) {
        if is_plain_ident(lhs) {
            if let Some(lv) = self.reference(lhs) {
                *lhs = lv;
            }
        } else {
            self.opaque(lhs);
        }
    }

    fn fresh_table(&mut self) -> String {
        loop {
            let cand = format!("_va{}", self.next_table);
            self.next_table += 1;
            if !self.used.contains(&cand) {
                self.used.insert(cand.clone());
                return cand;
            }
        }
    }

    /// A function literal or named function: its parameters open its
    /// frame, routed through a vararg table when there are too many.
    fn function(&mut self, params: &mut Vec<String>, body: &mut Vec<Stmt>) {
        let mut block: HashMap<String, Option<String>> = HashMap::new();
        let route = params.len() > PARAM_BUDGET && !params.iter().any(|p| p == "...");
        if route {
            let tbl = self.fresh_table();
            // A later parameter of the same name shadows an earlier one,
            // as Lua binds them; the map keeps the last.
            for (i, p) in params.iter().enumerate() {
                block.insert(p.clone(), Some(format!("{tbl}[{}]", i + 1)));
            }
            *params = vec!["...".to_string()];
            body.insert(
                0,
                Stmt::Local(
                    vec![tbl],
                    Some(Expr::Table(vec![Item::Pos(Expr::name("..."))])),
                ),
            );
            self.routed += 1;
        } else {
            for p in params.iter() {
                block.insert(p.clone(), None);
            }
        }
        self.frames.push(vec![block]);
        self.block(body);
        self.frames.pop();
        if route {
            self.routed -= 1;
        }
    }

    fn block(&mut self, stmts: &mut [Stmt]) {
        if self.frames.is_empty() {
            // The chunk's own frame.
            self.frames.push(Vec::new());
        }
        self.frames.last_mut().expect("a frame is open").push(HashMap::new());
        // Every rewrite is in place: a routed function keeps its
        // statement, a packed call its node, so the list's length and
        // order are what they were.
        for s in stmts.iter_mut() {
            self.stmt(s);
        }
        self.frames.last_mut().expect("a frame is open").pop();
    }

    fn stmt(&mut self, s: &mut Stmt) {
        match s {
            Stmt::Raw(t) => self.opaque(t),
            Stmt::Local(names, init) => {
                if let Some(e) = init {
                    self.expr(e);
                }
                for n in names.iter() {
                    self.bind(n);
                }
            }
            Stmt::Assign(lhs, e) => {
                self.lvalue(lhs);
                self.expr(e);
            }
            Stmt::AssignIf { lhs, cond, then_e, else_e } => {
                self.lvalue(lhs);
                self.expr(cond);
                self.expr(then_e);
                self.expr(else_e);
            }
            Stmt::MultiAssign(lhs, exprs) => {
                for l in lhs.iter_mut() {
                    self.lvalue(l);
                }
                for e in exprs {
                    self.expr(e);
                }
            }
            Stmt::Return(e) | Stmt::Expr(e) => self.expr(e),
            Stmt::If { cond, then_b, elseifs, else_b } => {
                self.expr(cond);
                self.block(&mut then_b.0);
                for (c, b) in elseifs {
                    self.expr(c);
                    self.block(&mut b.0);
                }
                if let Some(b) = else_b {
                    self.block(&mut b.0);
                }
            }
            Stmt::Do(b) | Stmt::WhileTrue(b) => self.block(&mut b.0),
            Stmt::Function { target, params, body } => {
                match target {
                    // `local function f` binds f for its own body too.
                    FnTarget::LocalFn(n) => self.bind(n),
                    FnTarget::Assigned(lhs) => self.lvalue(lhs),
                    FnTarget::Slot(_) | FnTarget::ThunkSlot(_) => {}
                }
                self.labels.push(format!("the function `{}`", target.header_text(params)));
                self.function(params, &mut body.0);
                self.labels.pop();
            }
            Stmt::ReturnTable(entries) => {
                for (_, e) in entries {
                    self.expr(e);
                }
            }
            Stmt::ReturnNone | Stmt::Goto(_) | Stmt::Label(_) => {}
        }
    }

    fn expr(&mut self, e: &mut Expr) {
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
            Expr::Lit(_) => {}
            Expr::Raw(t) => self.opaque(t),
            Expr::Index(base, suffix) => {
                self.expr(base);
                self.opaque(suffix);
            }
            Expr::Func(params, body) => {
                let outer = self.labels.last().expect("a label").clone();
                if !outer.starts_with("a function literal in ") {
                    self.labels.push(format!("a function literal in {outer}"));
                } else {
                    self.labels.push(outer);
                }
                self.function(params, body.stmts_mut());
                self.labels.pop();
            }
            Expr::Table(items) | Expr::TableSpaced(items) => {
                for item in items {
                    match item {
                        Item::Pos(e) => self.expr(e),
                        Item::KV(k, e) => {
                            self.opaque(k);
                            self.expr(e);
                        }
                    }
                }
            }
            Expr::Call(f, args) | Expr::Method(f, _, args) => {
                self.expr(f);
                for a in args.iter_mut() {
                    self.expr(a);
                }
                if args.len() > ARG_BUDGET {
                    let n = args.len();
                    let items: Vec<Item> = std::mem::take(args).into_iter().map(Item::Pos).collect();
                    args.push(Expr::call_named(
                        "__unpack",
                        vec![Expr::Table(items), Expr::lit("1"), Expr::lit(n.to_string())],
                    ));
                }
            }
            Expr::Paren(_) | Expr::Neg(_) | Expr::Binop(..) => {
                e.for_each_subexpr_mut(&mut |c| self.expr(c))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::lua::{Block, FuncBody};

    fn params(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("_p{i}")).collect()
    }

    fn rendered(stmts: &[Stmt]) -> String {
        render_stmts(stmts)
    }

    /// A function over the budget takes varargs, reads each parameter from
    /// the table by position, and assigns to it there; a `local` of a
    /// parameter's name inside the body shadows the slot from then on.
    #[test]
    fn parameters_over_budget_are_read_from_a_vararg_table() {
        let n = PARAM_BUDGET + 1;
        let body = vec![
            Stmt::Assign("_p0".into(), Expr::force(Expr::name("_p0"))),
            Stmt::Local(vec!["_p5".into()], Some(Expr::name("_p1"))),
            Stmt::Return(Expr::Table(vec![
                Item::Pos(Expr::name("_p0")),
                Item::Pos(Expr::name("_p5")),
                Item::Pos(Expr::name(format!("_p{}", n - 1))),
            ])),
        ];
        let mut stmts = vec![Stmt::Function {
            target: FnTarget::Slot(1),
            params: params(n),
            body: Block(body),
        }];
        assert!(run(&mut stmts).unwrap());
        let out = rendered(&stmts);
        assert!(out.starts_with("__mll_fn[1] = function(...)\n    local _va1 = {...}\n"), "{out}");
        assert!(out.contains("_va1[1] = __force(_va1[1])"), "{out}");
        assert!(out.contains("local _p5 = _va1[2]"), "{out}");
        assert!(out.contains(&format!("return {{_va1[1], _p5, _va1[{n}]}}")), "{out}");
        // Idempotent: the rewritten function has one parameter.
        assert!(!run(&mut stmts).unwrap());
    }

    /// Nested over-budget literals get distinct tables, and an inner
    /// literal's own parameter shadows the outer slot of the same name.
    #[test]
    fn nested_functions_route_through_distinct_tables() {
        let n = PARAM_BUDGET + 1;
        let mut inner_params = params(n);
        inner_params[0] = "x".into();
        let inner = Expr::Func(
            inner_params,
            FuncBody::Inline(vec![Stmt::Return(Expr::binop(
                "+",
                Expr::name("x"),
                Expr::binop("+", Expr::name("_p1"), Expr::name("_p2")),
            ))]),
        );
        let mut stmts = vec![Stmt::Function {
            target: FnTarget::LocalFn("f".into()),
            params: params(n),
            body: Block(vec![Stmt::Return(inner)]),
        }];
        assert!(run(&mut stmts).unwrap());
        let out = rendered(&stmts);
        assert!(out.contains("local function f(...)\n    local _va1 = {...}\n"), "{out}");
        // `x` is the inner literal's first parameter: slot 1 of ITS table;
        // `_p1`/`_p2` are the inner literal's too (it declares them all).
        assert!(out.contains("function(...) local _va2 = {...}; return _va2[1] + _va2[2] + _va2[3] end"), "{out}");
    }

    /// A call over the budget passes its arguments through `__unpack` with
    /// explicit bounds; a method call likewise.
    #[test]
    fn wide_calls_pack_their_arguments() {
        let n = ARG_BUDGET + 1;
        let args: Vec<Expr> = (0..n).map(|i| Expr::lit(i.to_string())).collect();
        let mut stmts = vec![
            Stmt::Return(Expr::call_named("f", args.clone())),
            Stmt::Expr(Expr::method(Expr::name("t"), "m", args)),
        ];
        assert!(run(&mut stmts).unwrap());
        let out = rendered(&stmts);
        let items = (0..n).map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
        assert_eq!(
            out,
            format!("return f(__unpack({{{items}}}, 1, {n}))\nt:m(__unpack({{{items}}}, 1, {n}))\n")
        );
        assert!(!run(&mut stmts).unwrap());
    }

    /// Within both budgets the pass renders nothing and changes nothing.
    #[test]
    fn within_budget_is_a_no_op() {
        let args: Vec<Expr> = (0..ARG_BUDGET).map(|i| Expr::lit(i.to_string())).collect();
        let mut stmts = vec![
            Stmt::Function {
                target: FnTarget::Slot(1),
                params: params(PARAM_BUDGET),
                body: Block(vec![Stmt::Return(Expr::call_named("f", args))]),
            },
            Stmt::Raw("local x = _p0".into()),
        ];
        let before = rendered(&stmts);
        assert!(!run(&mut stmts).unwrap());
        assert_eq!(rendered(&stmts), before);
    }

    /// A routed parameter mentioned in opaque text is a compile error that
    /// names the function and the parameter.
    #[test]
    fn opaque_mention_of_a_routed_parameter_is_reported() {
        let n = PARAM_BUDGET + 1;
        let mut stmts = vec![Stmt::Function {
            target: FnTarget::LocalFn("g".into()),
            params: params(n),
            body: Block(vec![Stmt::Raw("if type(_p3) == \"function\" then _p3 = nil end".into())]),
        }];
        let err = run(&mut stmts).unwrap_err();
        assert!(err.contains("the function `local function g("), "{err}");
        assert!(err.contains("`_p3`"), "{err}");
    }
}
