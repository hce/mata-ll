//! The Lua AST this code generator emits, and its printer.
//!
//! Deliberately concrete: the node inventory models exactly the shapes this
//! codegen produces, not all of Lua. Two properties follow from that choice:
//!
//! * Statement well-formedness is structural — a `Return` can only hold an
//!   expression, an `If` can only hold blocks — so the malformed-output class
//!   of bugs (B4: section composition emitted invalid Lua) is unrepresentable
//!   once an emission path builds nodes instead of strings.
//! * The printer reproduces the string emitter's exact formatting byte for
//!   byte: 4-space indents, `; `-joined inline function bodies, the same
//!   line-break placement. Grouping is EXPLICIT — `Paren` nodes sit exactly
//!   where emission historically wrote parens, and the printer never adds or
//!   drops parens on its own (no precedence logic). Redundant-paren cleanup
//!   is a deliberate, separate output change, not this module's business.
//!
//! Layout is part of the node, not derived: a function literal is either
//! `Inline` (`function(p) s1; s2 end` on one line) or `Block` (body on its
//! own lines at indent+1), because the emitter uses both forms and the choice
//! is made per site.
//!
//! Escape hatches: `Expr::Raw` / `Stmt::Raw` hold one preformatted
//! fragment/line produced outside the AST builders — pattern condition and
//! binding paths from pattern.rs, FFI decode/marshal descriptor text from
//! ffi.rs, and module.rs's fixed template lines. Raw text is printed
//! verbatim; that it forms a valid expression or a complete statement is the
//! producer's obligation.

/// A Lua expression as this code generator shapes it.
#[derive(Clone)]
pub(super) enum Expr {
    /// A reference printed verbatim: a bare name, `__mll_fn[3]`, `_v[2]`,
    /// a dotted path like `math.pi`. Produced by name resolution (`lua_ref`),
    /// which owns the spelling.
    Name(String),
    /// A rendered literal token: a number, a quoted string (already escaped
    /// by `lua_quoted_string`), `true`, `false`, `nil`.
    Lit(String),
    /// Bridge: preformatted Lua expression text (FFI descriptors, pattern
    /// scrutinee/binding paths). See the module comment.
    Raw(String),
    /// `(e)` — explicit grouping, placed by the builder, never synthesized.
    Paren(Box<Expr>),
    /// `f(a, b)`. The callee prints as-is; a callee that needs parens (a bare
    /// function literal) is wrapped in `Paren` by the builder.
    Call(Box<Expr>, Vec<Expr>),
    /// `recv:m(a, b)`.
    Method(Box<Expr>, String, Vec<Expr>),
    /// `base[3]` / `base.field` / `base["key"]` — the suffix arrives rendered
    /// (from `lua_field_index` or an explicit `[i]`), keys already escaped.
    Index(Box<Expr>, String),
    /// `a <op> b` with single spaces and NO parens of its own; grouping is an
    /// enclosing `Paren`.
    Binop(String, Box<Expr>, Box<Expr>),
    /// `-e`.
    Neg(Box<Expr>),
    /// `{a, b}` / `{k = v}` — no padding inside the braces.
    Table(Vec<Item>),
    /// `{ k = v, k2 = v2 }` — the dictionary-literal form with padded braces
    /// (`{  }` when empty, exactly as the emitter wrote it).
    TableSpaced(Vec<Item>),
    /// `function(p1, p2) … end` — layout per `FuncBody`.
    Func(Vec<String>, FuncBody),
}

/// One table-constructor item.
#[derive(Clone)]
pub(super) enum Item {
    Pos(Expr),
    /// Keyed entry; the key text arrives rendered INCLUDING the ` = ` suffix
    /// (from `lua_field_assign`, or `format!("{} = ", k)`).
    KV(String, Expr),
}

/// Function-literal layout.
#[derive(Clone)]
pub(super) enum FuncBody {
    /// One line: `function(p) s1; s2; sn end`.
    Inline(Vec<Stmt>),
    /// Multi-line: body statements at indent+1, `end` at the function's own
    /// indent.
    Block(Block),
}

/// A statement sequence printed one statement per line at a given indent.
#[derive(Clone)]
pub(super) struct Block(pub Vec<Stmt>);

/// A Lua statement as this code generator shapes it.
#[derive(Clone)]
pub(super) enum Stmt {
    /// Bridge: one preformatted line (no indent prefix, no newline).
    Raw(String),
    /// `local a, b` / `local x = e` / `local a, b = e` (multi-return).
    Local(Vec<String>, Option<Expr>),
    /// `lhs = e` — the lvalue arrives rendered (a name, `_v[3]`,
    /// `__mll_fn[7]`, `_u[2]`, `_u.field`).
    Assign(String, Expr),
    /// `return e`.
    Return(Expr),
    /// An expression in statement position (an effectful call).
    Expr(Expr),
    /// `if c then … elseif c2 then … else … end`.
    If {
        cond: Expr,
        then_b: Block,
        elseifs: Vec<(Expr, Block)>,
        else_b: Option<Block>,
    },
    /// One-line conditional assignment: `if c then lhs = t else lhs = e end`.
    /// The bind chain's strict-if fast path (a demanded `let x = if …`)
    /// emits this shape.
    AssignIf {
        lhs: String,
        cond: Expr,
        then_e: Expr,
        else_e: Expr,
    },
    /// `do … end` — an irrefutable clause's independent block in the
    /// guarded pattern-match layout.
    Do(Block),
    /// `while true do … end`. Deliberately condition-less: the only loops
    /// this codegen emits are the self-tail-call and IO self-loop drivers
    /// (tailloop.rs / ioloop.rs, opt passes 5 and 6), which exit through
    /// `return`/`error`, never through the condition. There is no `break` in
    /// the vocabulary, so control cannot pass this statement normally —
    /// `stmt_diverges` relies on that.
    WhileTrue(Block),
    /// `a, b = e1, e2` — simultaneous multiple assignment: Lua evaluates the
    /// whole RHS list before assigning any lvalue, which is what makes it the
    /// correct parameter-update form for the tail-call loop (a cascade of
    /// single assignments would let a later RHS read an already-overwritten
    /// name). The lvalues arrive rendered, like `Assign`'s.
    MultiAssign(Vec<String>, Vec<Expr>),
    /// Bare `return` (no operand — zero values, exactly what falling off a
    /// function body yields). Lua only allows it as the last statement of a
    /// block; the emitter wraps it as `do return end` where something follows.
    ReturnNone,
    /// `goto name`.
    Goto(String),
    /// `::name::`. Only emitted as the LAST statement of a loop body: Lua
    /// forbids a goto from jumping into a local's scope, and only a label in
    /// end-of-block position (followed by nothing) is exempt from that rule.
    Label(String),
    /// A named function definition: what the header binds or stores to
    /// (`FnTarget`), the parameter names, and the body at indent+1 with
    /// `end` back at the statement's indent. The printer renders the header
    /// as `local function f(a, b)` / `__mll_fn[3] = function(a, b)` /
    /// `go = function(a, b)` (see `FnTarget::render_header`).
    Function {
        target: FnTarget,
        params: Vec<String>,
        body: Block,
    },
    /// The module's export table: `return {` with one `key = value,` entry
    /// per line at indent+1 and `}` back at the statement's indent.
    ReturnTable(Vec<(String, Expr)>),
}

/// The forward-declaration function table's reserved name. Structured
/// consumers match [`FnTarget::Slot`]; the residual substring scans (see
/// `name_mentions_fn_table`) and the store-key spelling share this one
/// constant instead of repeating the literal.
pub(super) const FN_TABLE: &str = "__mll_fn";
/// The lifted-thunk function table (see thunklift.rs) — the one spelling,
/// shared by the pass that fills it and the renderer.
pub(super) const TKF_TABLE: &str = "__mll_tkf";

/// Does a NON-`Slot` header mention the function table — in its target
/// name or any parameter? Both consumers (the slot census's poison arm in
/// annot.rs and ioloop's repeat-safety gate) reproduce the former
/// whole-header TEXT scan, so this is deliberately a substring test, not
/// an exact match: parity with the retired scan is what the corpus stamp
/// refutation pins. Callers handle `Slot` structurally first.
pub(super) fn name_mentions_fn_table(name: &str, params: &[String]) -> bool {
    name.contains(FN_TABLE) || params.iter().any(|p| p.contains(FN_TABLE))
}

/// What a named function definition (`Stmt::Function`) binds or stores to.
/// Three forms, matching the three spellings name resolution produces
/// (`CodeGen::fn_target` for the first two, the where-group assignment in
/// function.rs for the third).
#[derive(Clone)]
pub(super) enum FnTarget {
    /// `local function <name>(…)` — the header itself declares and binds.
    LocalFn(String),
    /// `__mll_fn[<slot>] = function(…)` — a store to the module function
    /// table's slot.
    Slot(u32),
    /// `<lvalue> = function(…)` — assignment to a forward-declared local.
    /// The lvalue arrives rendered, like `Stmt::Assign`'s: a bare name
    /// (`go`) or its `_v[N]` spill slot.
    Assigned(String),
    /// `__mll_tkf[<slot>] = function(…)` — a lifted thunk body
    /// (thunklift.rs). A dedicated variant, not an `Assigned` spelling:
    /// the paren pass keys its Delim return context off it (a lifted
    /// body's one consumer is `__force`'s truncating call), and a string
    /// prefix match would go silently stale if the spelling moved.
    ThunkSlot(u32),
}

impl FnTarget {
    /// Render the full header line up to and including the parameter list's
    /// closing paren — byte-identical to the pre-rendered header strings
    /// this node used to carry.
    fn render_header(&self, params: &[String], out: &mut String) {
        match self {
            FnTarget::LocalFn(n) => {
                out.push_str("local function ");
                out.push_str(n);
            }
            FnTarget::Slot(i) => {
                out.push_str("__mll_fn[");
                out.push_str(&i.to_string());
                out.push_str("] = function");
            }
            FnTarget::Assigned(lhs) => {
                out.push_str(lhs);
                out.push_str(" = function");
            }
            FnTarget::ThunkSlot(i) => {
                out.push_str(TKF_TABLE);
                out.push('[');
                out.push_str(&i.to_string());
                out.push_str("] = function");
            }
        }
        out.push('(');
        out.push_str(&params.join(", "));
        out.push(')');
    }

    /// The rendered header as a string (fresh-name token scans, reverse
    /// self-check diagnostics).
    pub(super) fn header_text(&self, params: &[String]) -> String {
        let mut out = String::new();
        self.render_header(params, &mut out);
        out
    }
}

fn pad(ind: usize, out: &mut String) {
    for _ in 0..ind {
        out.push_str("    ");
    }
}

impl Expr {
    pub(super) fn name(s: impl Into<String>) -> Expr {
        Expr::Name(s.into())
    }

    pub(super) fn lit(s: impl Into<String>) -> Expr {
        Expr::Lit(s.into())
    }

    pub(super) fn raw(s: impl Into<String>) -> Expr {
        Expr::Raw(s.into())
    }

    pub(super) fn paren(e: Expr) -> Expr {
        Expr::Paren(Box::new(e))
    }

    pub(super) fn call(f: Expr, args: Vec<Expr>) -> Expr {
        Expr::Call(Box::new(f), args)
    }

    pub(super) fn method(recv: Expr, m: impl Into<String>, args: Vec<Expr>) -> Expr {
        Expr::Method(Box::new(recv), m.into(), args)
    }

    pub(super) fn index(base: Expr, suffix: impl Into<String>) -> Expr {
        Expr::Index(Box::new(base), suffix.into())
    }

    pub(super) fn binop(op: impl Into<String>, l: Expr, r: Expr) -> Expr {
        Expr::Binop(op.into(), Box::new(l), Box::new(r))
    }

    pub(super) fn neg(e: Expr) -> Expr {
        Expr::Neg(Box::new(e))
    }

    /// Convenience: `__force(e)`.
    pub(super) fn force(e: Expr) -> Expr {
        Expr::Call(Box::new(Expr::Name("__force".into())), vec![e])
    }

    /// Convenience: `__thunk(function() return e end)`.
    pub(super) fn thunk(e: Expr) -> Expr {
        Expr::Call(
            Box::new(Expr::Name("__thunk".into())),
            vec![Expr::inline_fn0(e)],
        )
    }

    /// Convenience: `function() return e end`.
    pub(super) fn inline_fn0(e: Expr) -> Expr {
        Expr::Func(vec![], FuncBody::Inline(vec![Stmt::Return(e)]))
    }

    /// Convenience: `name(args)`.
    pub(super) fn call_named(name: &str, args: Vec<Expr>) -> Expr {
        Expr::Call(Box::new(Expr::Name(name.into())), args)
    }

    /// Fold a non-empty condition list into one left-associated `and` chain:
    /// `c1 and c2 and c3`. The conditions are comparisons and calls (never
    /// bare `and`/`or` chains of lower precedence), so no grouping parens
    /// are needed between them.
    pub(super) fn and_chain(conds: Vec<Expr>) -> Expr {
        let mut it = conds.into_iter();
        let first = it.next().expect("and_chain: empty condition list");
        it.fold(first, |acc, c| Expr::binop("and", acc, c))
    }

    /// Render at `ind` (the indent any internal line breaks are relative to)
    /// and append to `out`. No indent prefix is emitted for the expression
    /// itself — it continues the caller's current line.
    pub(super) fn render(&self, ind: usize, out: &mut String) {
        match self {
            Expr::Name(s) | Expr::Lit(s) | Expr::Raw(s) => out.push_str(s),
            Expr::Paren(e) => {
                out.push('(');
                e.render(ind, out);
                out.push(')');
            }
            Expr::Call(f, args) => {
                f.render(ind, out);
                out.push('(');
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    a.render(ind, out);
                }
                out.push(')');
            }
            Expr::Method(recv, m, args) => {
                recv.render(ind, out);
                out.push(':');
                out.push_str(m);
                out.push('(');
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    a.render(ind, out);
                }
                out.push(')');
            }
            Expr::Index(base, suffix) => {
                base.render(ind, out);
                out.push_str(suffix);
            }
            Expr::Binop(op, l, r) => {
                l.render(ind, out);
                out.push(' ');
                out.push_str(op);
                out.push(' ');
                r.render(ind, out);
            }
            Expr::Neg(e) => {
                out.push('-');
                e.render(ind, out);
            }
            Expr::Table(items) => {
                out.push('{');
                render_items(items, ind, out);
                out.push('}');
            }
            Expr::TableSpaced(items) => {
                out.push_str("{ ");
                render_items(items, ind, out);
                out.push_str(" }");
            }
            Expr::Func(params, body) => {
                out.push_str("function(");
                out.push_str(&params.join(", "));
                out.push(')');
                match body {
                    FuncBody::Inline(stmts) => {
                        out.push(' ');
                        for (i, s) in stmts.iter().enumerate() {
                            if i > 0 {
                                out.push_str("; ");
                            }
                            s.render(ind, out);
                        }
                        out.push_str(" end");
                    }
                    FuncBody::Block(block) => {
                        out.push('\n');
                        block.render(ind + 1, out);
                        pad(ind, out);
                        out.push_str("end");
                    }
                }
            }
        }
    }
}

impl Expr {
    /// Visit every DIRECT subexpression, left to right in rendering order.
    /// `Func` contributes nothing: a function literal's children are
    /// statements, so a walker that wants to enter function bodies must
    /// intercept `Expr::Func` itself before falling back to this helper.
    ///
    /// One exhaustive match, no wildcard arm — a new `Expr` variant fails to
    /// compile here until its children are routed, so no walker built on
    /// this helper can silently skip a subtree.
    pub(super) fn for_each_subexpr(&self, f: &mut impl FnMut(&Expr)) {
        match self {
            Expr::Name(_) | Expr::Lit(_) | Expr::Raw(_) => {}
            Expr::Paren(e) | Expr::Neg(e) => f(e),
            Expr::Call(callee, args) | Expr::Method(callee, _, args) => {
                f(callee);
                for a in args {
                    f(a);
                }
            }
            Expr::Index(base, _) => f(base),
            Expr::Binop(_, l, r) => {
                f(l);
                f(r);
            }
            Expr::Table(items) | Expr::TableSpaced(items) => {
                for item in items {
                    match item {
                        Item::Pos(e) | Item::KV(_, e) => f(e),
                    }
                }
            }
            Expr::Func(..) => {}
        }
    }

    /// `for_each_subexpr`, mutably. Kept as a spelled-out twin (Rust cannot
    /// abstract over the mutability) — change both together.
    pub(super) fn for_each_subexpr_mut(&mut self, f: &mut impl FnMut(&mut Expr)) {
        match self {
            Expr::Name(_) | Expr::Lit(_) | Expr::Raw(_) => {}
            Expr::Paren(e) | Expr::Neg(e) => f(e),
            Expr::Call(callee, args) | Expr::Method(callee, _, args) => {
                f(callee);
                for a in args {
                    f(a);
                }
            }
            Expr::Index(base, _) => f(base),
            Expr::Binop(_, l, r) => {
                f(l);
                f(r);
            }
            Expr::Table(items) | Expr::TableSpaced(items) => {
                for item in items {
                    match item {
                        Item::Pos(e) | Item::KV(_, e) => f(e),
                    }
                }
            }
            Expr::Func(..) => {}
        }
    }
}

impl FuncBody {
    /// The body's statements, layout-independent (`Inline` and `Block` hold
    /// the same thing; only the printer cares which).
    pub(super) fn stmts(&self) -> &[Stmt] {
        match self {
            FuncBody::Inline(s) => s,
            FuncBody::Block(Block(s)) => s,
        }
    }

    pub(super) fn stmts_mut(&mut self) -> &mut Vec<Stmt> {
        match self {
            FuncBody::Inline(s) => s,
            FuncBody::Block(Block(s)) => s,
        }
    }
}

fn render_items(items: &[Item], ind: usize, out: &mut String) {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        match item {
            Item::Pos(e) => e.render(ind, out),
            Item::KV(key_prefix, e) => {
                out.push_str(key_prefix);
                e.render(ind, out);
            }
        }
    }
}

impl Stmt {
    /// Visit every DIRECT expression child, in rendering order. Statements
    /// inside sub-blocks are not entered — that is `for_each_block`'s job —
    /// so an `If` contributes only its conditions here.
    ///
    /// One exhaustive match, no wildcard arm — a new `Stmt` variant fails to
    /// compile here until its children are routed, so no walker built on
    /// this helper can silently skip a subtree.
    pub(super) fn for_each_expr(&self, f: &mut impl FnMut(&Expr)) {
        match self {
            Stmt::Raw(_)
            | Stmt::ReturnNone
            | Stmt::Goto(_)
            | Stmt::Label(_)
            | Stmt::Do(_)
            | Stmt::WhileTrue(_)
            | Stmt::Function { .. } => {}
            Stmt::Local(_, init) => {
                if let Some(e) = init {
                    f(e);
                }
            }
            Stmt::Assign(_, e) | Stmt::Return(e) | Stmt::Expr(e) => f(e),
            Stmt::AssignIf { cond, then_e, else_e, .. } => {
                f(cond);
                f(then_e);
                f(else_e);
            }
            Stmt::If { cond, elseifs, .. } => {
                f(cond);
                for (c, _) in elseifs {
                    f(c);
                }
            }
            Stmt::MultiAssign(_, exprs) => {
                for e in exprs {
                    f(e);
                }
            }
            Stmt::ReturnTable(entries) => {
                for (_, e) in entries {
                    f(e);
                }
            }
        }
    }

    /// `for_each_expr`, mutably. A spelled-out twin — change both together.
    pub(super) fn for_each_expr_mut(&mut self, f: &mut impl FnMut(&mut Expr)) {
        match self {
            Stmt::Raw(_)
            | Stmt::ReturnNone
            | Stmt::Goto(_)
            | Stmt::Label(_)
            | Stmt::Do(_)
            | Stmt::WhileTrue(_)
            | Stmt::Function { .. } => {}
            Stmt::Local(_, init) => {
                if let Some(e) = init {
                    f(e);
                }
            }
            Stmt::Assign(_, e) | Stmt::Return(e) | Stmt::Expr(e) => f(e),
            Stmt::AssignIf { cond, then_e, else_e, .. } => {
                f(cond);
                f(then_e);
                f(else_e);
            }
            Stmt::If { cond, elseifs, .. } => {
                f(cond);
                for (c, _) in elseifs {
                    f(c);
                }
            }
            Stmt::MultiAssign(_, exprs) => {
                for e in exprs {
                    f(e);
                }
            }
            Stmt::ReturnTable(entries) => {
                for (_, e) in entries {
                    f(e);
                }
            }
        }
    }

    /// Visit every DIRECT sub-block: `if` arms, `do`/`while true` bodies, a
    /// named function's body. Function literals inside expressions are not
    /// reached — walkers get to those through the expression side.
    ///
    /// Same no-wildcard discipline as `for_each_expr`.
    pub(super) fn for_each_block(&self, f: &mut impl FnMut(&[Stmt])) {
        match self {
            Stmt::Raw(_)
            | Stmt::Local(..)
            | Stmt::Assign(..)
            | Stmt::Return(_)
            | Stmt::Expr(_)
            | Stmt::AssignIf { .. }
            | Stmt::MultiAssign(..)
            | Stmt::ReturnNone
            | Stmt::Goto(_)
            | Stmt::Label(_)
            | Stmt::ReturnTable(_) => {}
            Stmt::If { then_b, elseifs, else_b, .. } => {
                f(&then_b.0);
                for (_, b) in elseifs {
                    f(&b.0);
                }
                if let Some(b) = else_b {
                    f(&b.0);
                }
            }
            Stmt::Do(b) | Stmt::WhileTrue(b) => f(&b.0),
            Stmt::Function { body, .. } => f(&body.0),
        }
    }

    /// `for_each_block`, mutably (as `&mut Vec` — block-level passes
    /// truncate and splice). A spelled-out twin — change both together.
    pub(super) fn for_each_block_mut(&mut self, f: &mut impl FnMut(&mut Vec<Stmt>)) {
        match self {
            Stmt::Raw(_)
            | Stmt::Local(..)
            | Stmt::Assign(..)
            | Stmt::Return(_)
            | Stmt::Expr(_)
            | Stmt::AssignIf { .. }
            | Stmt::MultiAssign(..)
            | Stmt::ReturnNone
            | Stmt::Goto(_)
            | Stmt::Label(_)
            | Stmt::ReturnTable(_) => {}
            Stmt::If { then_b, elseifs, else_b, .. } => {
                f(&mut then_b.0);
                for (_, b) in elseifs {
                    f(&mut b.0);
                }
                if let Some(b) = else_b {
                    f(&mut b.0);
                }
            }
            Stmt::Do(b) | Stmt::WhileTrue(b) => f(&mut b.0),
            Stmt::Function { body, .. } => f(&mut body.0),
        }
    }

    /// Render the statement as a full line (or run of lines) at `ind`:
    /// indent prefix and trailing newline included.
    pub(super) fn render_line(&self, ind: usize, out: &mut String) {
        pad(ind, out);
        let start = out.len();
        self.render(ind, out);
        // A statement whose rendering begins with `(` — an IIFE
        // expression-statement, a Raw line — would otherwise parse as a
        // CALL CONTINUATION of the previous statement's trailing
        // expression: since Lua 5.2 `f()\n(g)(x)` is one call chain,
        // silently, with no ambiguity error. The `;` separator pins the
        // statement boundary (a leading `;` is itself a legal empty
        // statement, so this is safe even for the first line).
        if out.as_bytes().get(start) == Some(&b'(') {
            out.insert(start, ';');
        }
        out.push('\n');
    }

    /// Render the statement's own text at `ind`: no leading indent, no
    /// trailing newline (the enclosing `Block` or inline body adds those).
    /// Multi-line statements (`If`) indent their interior lines themselves
    /// and end on `end` at `ind`.
    fn render(&self, ind: usize, out: &mut String) {
        match self {
            Stmt::Raw(s) => out.push_str(s),
            Stmt::Local(names, init) => {
                out.push_str("local ");
                out.push_str(&names.join(", "));
                if let Some(e) = init {
                    out.push_str(" = ");
                    e.render(ind, out);
                }
            }
            Stmt::Assign(lhs, e) => {
                out.push_str(lhs);
                out.push_str(" = ");
                e.render(ind, out);
            }
            Stmt::Return(e) => {
                out.push_str("return ");
                e.render(ind, out);
            }
            Stmt::Expr(e) => e.render(ind, out),
            Stmt::Do(block) => {
                out.push_str("do\n");
                block.render(ind + 1, out);
                pad(ind, out);
                out.push_str("end");
            }
            Stmt::WhileTrue(block) => {
                out.push_str("while true do\n");
                block.render(ind + 1, out);
                pad(ind, out);
                out.push_str("end");
            }
            Stmt::MultiAssign(lhs, exprs) => {
                out.push_str(&lhs.join(", "));
                out.push_str(" = ");
                for (i, e) in exprs.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    e.render(ind, out);
                }
            }
            Stmt::ReturnNone => out.push_str("return"),
            Stmt::Goto(l) => {
                out.push_str("goto ");
                out.push_str(l);
            }
            Stmt::Label(l) => {
                out.push_str("::");
                out.push_str(l);
                out.push_str("::");
            }
            Stmt::Function { target, params, body } => {
                target.render_header(params, out);
                out.push('\n');
                body.render(ind + 1, out);
                pad(ind, out);
                out.push_str("end");
            }
            Stmt::ReturnTable(entries) => {
                out.push_str("return {\n");
                for (key, value) in entries {
                    pad(ind + 1, out);
                    out.push_str(key);
                    out.push_str(" = ");
                    value.render(ind + 1, out);
                    out.push_str(",\n");
                }
                pad(ind, out);
                out.push('}');
            }
            Stmt::AssignIf { lhs, cond, then_e, else_e } => {
                out.push_str("if ");
                cond.render(ind, out);
                out.push_str(" then ");
                out.push_str(lhs);
                out.push_str(" = ");
                then_e.render(ind, out);
                out.push_str(" else ");
                out.push_str(lhs);
                out.push_str(" = ");
                else_e.render(ind, out);
                out.push_str(" end");
            }
            Stmt::If { cond, then_b, elseifs, else_b } => {
                out.push_str("if ");
                cond.render(ind, out);
                out.push_str(" then\n");
                then_b.render(ind + 1, out);
                for (c, b) in elseifs {
                    pad(ind, out);
                    out.push_str("elseif ");
                    c.render(ind, out);
                    out.push_str(" then\n");
                    b.render(ind + 1, out);
                }
                if let Some(b) = else_b {
                    pad(ind, out);
                    out.push_str("else\n");
                    b.render(ind + 1, out);
                }
                pad(ind, out);
                out.push_str("end");
            }
        }
    }
}

impl Block {
    /// Render every statement at `ind`, one per line.
    pub(super) fn render(&self, ind: usize, out: &mut String) {
        for s in &self.0 {
            s.render_line(ind, out);
        }
    }
}

/// The rendered text of a statement list at indent 0 — the byte-compare
/// currency of the passes' reverse self-checks and idempotence refutation.
pub(super) fn render_stmts(stmts: &[Stmt]) -> String {
    let mut s = String::new();
    for st in stmts {
        st.render_line(0, &mut s);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A statement rendering that begins with `(` — an IIFE
    /// expression-statement — must be pinned with a `;` separator: since
    /// Lua 5.2, `f()` followed by `(g)(x)` on the next line parses as ONE
    /// call chain, silently, with no ambiguity error (Q81).
    #[test]
    fn paren_led_statement_gets_a_separator() {
        let stmts = vec![
            Stmt::Local(
                vec!["x".into()],
                Some(Expr::call(Expr::name("f"), vec![])),
            ),
            Stmt::Expr(Expr::call(
                Expr::paren(Expr::Func(
                    vec![],
                    FuncBody::Inline(vec![Stmt::Return(Expr::lit("1"))]),
                )),
                vec![],
            )),
        ];
        let out = render_stmts(&stmts);
        assert!(
            out.contains("\n;("),
            "paren-led statement must be `;`-pinned:\n{out}"
        );
    }
}
