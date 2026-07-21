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
pub(super) enum Item {
    Pos(Expr),
    /// Keyed entry; the key text arrives rendered INCLUDING the ` = ` suffix
    /// (from `lua_field_assign`, or `format!("{} = ", k)`).
    KV(String, Expr),
}

/// Function-literal layout.
pub(super) enum FuncBody {
    /// One line: `function(p) s1; s2; sn end`.
    Inline(Vec<Stmt>),
    /// Multi-line: body statements at indent+1, `end` at the function's own
    /// indent.
    Block(Block),
}

/// A statement sequence printed one statement per line at a given indent.
pub(super) struct Block(pub Vec<Stmt>);

/// A Lua statement as this code generator shapes it.
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
    /// A named function definition. The header arrives rendered up to and
    /// including the parameter list's closing paren — `local function f(a)`,
    /// `__mll_fn[3] = function(a)`, `go = function(a)` — because the spelling
    /// is owned by name resolution (`fn_decl` / spill-slot placement). The
    /// body sits at indent+1 with `end` back at the statement's indent.
    Function { header: String, body: Block },
    /// The module's export table: `return {` with one `key = value,` entry
    /// per line at indent+1 and `}` back at the statement's indent.
    ReturnTable(Vec<(String, Expr)>),
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
    /// Render the statement as a full line (or run of lines) at `ind`:
    /// indent prefix and trailing newline included.
    pub(super) fn render_line(&self, ind: usize, out: &mut String) {
        pad(ind, out);
        self.render(ind, out);
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
            Stmt::Function { header, body } => {
                out.push_str(header);
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
