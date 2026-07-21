//! Pattern-match compilation: clause conditions, field bindings, guards.
//!
//! `pattern_match_block` compiles guard-free clauses into one `Stmt::If`
//! chain (with the non-exhaustive error after it); any guard-bearing match
//! is dispatched to `pattern_match_guarded_block`, which emits each clause
//! as an independent block (`Stmt::If` for a refutable pattern, `Stmt::Do`
//! for an irrefutable one) so a clause whose pattern matches but whose
//! guards all fail simply falls through to the next clause — Haskell
//! semantics an if/elseif chain cannot express. Because the dispatch is
//! total, the chain builder handles ONLY guard-free clauses (the old string
//! emitter carried unreachable guard branches here).
//!
//! Conditions, scrutinee paths and binding values are built as rendered
//! strings (`collect_pattern_conditions`, `field_path*`, `match_scrutinee`)
//! and enter the tree as `Raw` leaves; the statement structure around them
//! is AST. Forcing discipline: a sub-pattern is forced only when it
//! inspects its value (tag match, literal, deeper destructuring);
//! Var/Wildcard bindings stay lazy, and a refutable top-level pattern forces
//! its scrutinee inside its own clause condition so a matching earlier
//! clause never forces it.

use crate::tir::*;
use super::CodeGen;
use super::lua::{Block, Expr, Stmt};
use super::names::{lua_field_index, lua_quoted_string, sanitize_name};

impl CodeGen {
    /// The `error("Non-exhaustive patterns")` fall-off statement.
    fn non_exhaustive_stmt() -> Stmt {
        Stmt::Expr(Expr::call_named(
            "error",
            vec![Expr::lit(lua_quoted_string("Non-exhaustive patterns"))],
        ))
    }

    /// A clause's bindings (`local x = <path>`) plus its where bindings.
    fn clause_intro_stmts(&mut self, clause: &TClause, bindings: &[(String, String)]) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        for (var, val) in bindings {
            let (pre, decl) = self.declare_local_parts(var);
            if let Some(s) = pre {
                stmts.push(s);
            }
            stmts.push(decl.stmt(Expr::raw(val.clone())));
            // Propagate concreteness: if binding source is concrete, so is the target
            if self.concrete_vars.contains(val) {
                self.concrete_vars.insert(var.clone());
            }
        }
        let demanded = self.clause_demanded(clause);
        stmts.extend(self.where_binds_stmts(clause, demanded));
        stmts
    }

    /// A guard condition, rendered through a fresh sub-generator exactly as
    /// the streaming emitter did: the sub starts at indent 0, so a multiline
    /// construct inside a guard renders relative to column 0 regardless of
    /// the chain's depth. (A formatting oddity, but a byte-for-byte one.)
    fn guard_cond_raw(&mut self, cond: &TExpr) -> Expr {
        let mut sub = self.new_sub();
        let e = sub.expr_ast(cond);
        self.absorb_sub_error(&mut sub);
        let mut s = String::new();
        e.render(0, &mut s);
        Expr::raw(s)
    }

    pub(super) fn pattern_match_block(&mut self, params: &[String], clauses: &[TClause]) -> Block {
        // Clauses with guards need fallthrough semantics (a clause whose pattern
        // matches but whose guards all fail must drop to the next clause). The
        // if/elseif chain below cannot express that across a pattern boundary, so
        // route any guard-bearing match through the independent-block builder.
        // The dispatch is total, so the chain below handles only guard-free
        // clauses.
        if clauses.iter().any(|c| !c.guards.is_empty()) {
            return self.pattern_match_guarded_block(params, clauses);
        }
        let mut chain: Option<(Expr, Block)> = None;
        let mut elseifs: Vec<(Expr, Block)> = Vec::new();
        let mut else_b: Option<Block> = None;
        let mut direct: Option<Vec<Stmt>> = None;
        let mut fell_through = true;
        for (i, clause) in clauses.iter().enumerate() {
            // Each clause is an independent Lua branch (if/elseif … then … end),
            // so its locals must not leak into sibling clauses. Without this,
            // a name bound in one clause stays in `local_vars` and a later
            // clause's `let`/where binding of the same name is emitted without
            // `local` — assigning to a shared global instead, which corrupts
            // when captured by a thunk across calls (e.g. nested FOR loops).
            let scope_lv = self.local_vars.clone();
            let scope_vs = self.var_slots.clone();
            let scope_vsn = self.var_slots_next;
            let scope_lc = self.local_count;
            let scope_vte = self.var_table_emitted;
            let scope_cv = self.concrete_vars.clone();
            let scope_lsp = self.local_strict_params.clone();
            let scope_ldr = self.local_demand_rows.clone();

            let mut conditions = Vec::new();
            let mut bindings = Vec::new();
            for (pi, pat) in clause.patterns.iter().enumerate() {
                self.collect_pattern_conditions(&self.match_scrutinee(&params[pi], pat), pat, &mut conditions, &mut bindings);
            }

            if conditions.is_empty() {
                // Irrefutable clause: for the first clause its body IS the
                // whole match; after refutable clauses it becomes the chain's
                // `else` and later clauses are unreachable. Mirroring the
                // string emitter, the clause scope is NOT restored on this
                // early exit (the enclosing function restores its own scope).
                let mut bs = self.clause_intro_stmts(clause, &bindings);
                bs.push(Stmt::Return(self.tail_ast(&clause.body, false)));
                if i > 0 {
                    else_b = Some(Block(bs));
                } else {
                    direct = Some(bs);
                }
                fell_through = false;
                break;
            }

            let cond = Expr::raw(conditions.join(" and "));
            let mut bs = self.clause_intro_stmts(clause, &bindings);
            bs.push(Stmt::Return(self.tail_ast(&clause.body, false)));
            if chain.is_none() {
                chain = Some((cond, Block(bs)));
            } else {
                elseifs.push((cond, Block(bs)));
            }

            // Restore the scope captured at the start of this clause so its
            // locals do not leak into the next clause.
            self.local_vars = scope_lv;
            self.var_slots = scope_vs;
            self.var_slots_next = scope_vsn;
            self.local_count = scope_lc;
            self.var_table_emitted = scope_vte;
            self.concrete_vars = scope_cv;
            self.local_strict_params = scope_lsp;
            self.local_demand_rows = scope_ldr;
        }
        let mut stmts = Vec::new();
        match chain {
            Some((cond, then_b)) => {
                stmts.push(Stmt::If { cond, then_b, elseifs, else_b });
                if fell_through {
                    stmts.push(Self::non_exhaustive_stmt());
                }
            }
            None => {
                // First clause irrefutable: its body is the whole match.
                stmts.extend(direct.unwrap_or_default());
            }
        }
        Block(stmts)
    }

    /// Pattern match where at least one clause carries guards. Each clause is
    /// emitted as an independent block — `if <pat-conds> then …` for a refutable
    /// pattern, `do …` for an irrefutable one — rather than a single if/elseif
    /// chain. A clause whose pattern matches but whose guards all fail simply
    /// reaches the end of its block and falls through to the next clause, which
    /// is exactly Haskell's semantics. (The flat if/elseif chain cannot do this:
    /// once a pattern's `then` arm is entered there is no way back to the next
    /// `elseif`.)
    pub(super) fn pattern_match_guarded_block(&mut self, params: &[String], clauses: &[TClause]) -> Block {
        let mut stmts = Vec::new();
        for clause in clauses {
            // A clause's where-scope rows (installed by where_binds_stmts)
            // must not leak into the next clause's independent block.
            let scope_lsp = self.local_strict_params.clone();
            let scope_ldr = self.local_demand_rows.clone();
            let mut conditions = Vec::new();
            let mut bindings = Vec::new();
            for (pi, pat) in clause.patterns.iter().enumerate() {
                self.collect_pattern_conditions(&self.match_scrutinee(&params[pi], pat), pat, &mut conditions, &mut bindings);
            }
            let mut bs = self.clause_intro_stmts(clause, &bindings);
            if clause.guards.is_empty() {
                bs.push(Stmt::Return(self.tail_ast(&clause.body, false)));
            } else {
                let mut gchain: Option<(Expr, Block)> = None;
                let mut gelseifs: Vec<(Expr, Block)> = Vec::new();
                for guard in &clause.guards {
                    let cond = self.guard_cond_raw(&guard.condition);
                    let body = Block(vec![Stmt::Return(self.tail_ast(&guard.body, false))]);
                    if gchain.is_none() {
                        gchain = Some((cond, body));
                    } else {
                        gelseifs.push((cond, body));
                    }
                }
                let (gcond, gthen) = gchain.expect("non-empty guard list");
                bs.push(Stmt::If {
                    cond: gcond,
                    then_b: gthen,
                    elseifs: gelseifs,
                    else_b: None,
                });
            }
            if conditions.is_empty() {
                stmts.push(Stmt::Do(Block(bs)));
            } else {
                stmts.push(Stmt::If {
                    cond: Expr::raw(conditions.join(" and ")),
                    then_b: Block(bs),
                    elseifs: vec![],
                    else_b: None,
                });
            }
            self.local_strict_params = scope_lsp;
            self.local_demand_rows = scope_ldr;
        }
        stmts.push(Self::non_exhaustive_stmt());
        Block(stmts)
    }

    /// A sub-pattern that inspects its value (matches a tag, compares a
    /// literal, or destructures further) needs that value forced first;
    /// a Var/Wildcard just binds/ignores it and can stay lazy.
    pub(super) fn pattern_inspects_value(pattern: &TPattern) -> bool {
        match pattern {
            TPattern::Var(..) | TPattern::Wildcard => false,
            TPattern::Paren(inner) => Self::pattern_inspects_value(inner),
            _ => true,
        }
    }

    /// Build an indexing path into a field, forcing it when the sub-pattern
    /// will inspect it. The field may hold a thunk (lazy construction), so
    /// indexing into it (`field[1]`, `field == tag`, ...) requires forcing.
    pub(super) fn field_path(scrutinee: &str, idx: usize, child: &TPattern) -> String {
        let path = format!("{}[{}]", scrutinee, idx);
        if Self::pattern_inspects_value(child) {
            format!("__force({})", path)
        } else {
            path
        }
    }

    /// Like `field_path`, but for a LuaDict field addressed by name (`.width`).
    pub(super) fn field_path_key(scrutinee: &str, key: &str, child: &TPattern) -> String {
        let path = format!("{}{}", scrutinee, lua_field_index(key));
        if Self::pattern_inspects_value(child) {
            format!("__force({})", path)
        } else {
            path
        }
    }

    /// The scrutinee expression to match `pat` against for top-level parameter
    /// `param`. A refutable pattern (constructor/literal) needs the value
    /// forced to WHNF to inspect its tag; if the param was not already forced
    /// at entry (it is scrutinized only by a later clause — see `needs_force`),
    /// force it HERE, inside the clause's `elseif` condition, so a matching
    /// earlier clause never forces it. An irrefutable pattern (Var/Wildcard)
    /// binds lazily and must stay the raw (unforced) param.
    pub(super) fn match_scrutinee(&self, param: &str, pat: &TPattern) -> String {
        if matches!(pat, TPattern::Var(_, _) | TPattern::Wildcard)
            || self.concrete_vars.contains(param)
        {
            param.to_string()
        } else {
            format!("__force({})", param)
        }
    }

    pub(super) fn collect_pattern_conditions(&self, scrutinee: &str, pattern: &TPattern, conditions: &mut Vec<String>, bindings: &mut Vec<(String, String)>) {
        match pattern {
            TPattern::Var(name, _) => { bindings.push((sanitize_name(name), scrutinee.to_string())); }
            TPattern::Wildcard => {}
            TPattern::LitPat(lit) => {
                let s = match lit {
                    // See literal_ast: i64::MIN has no decimal Lua spelling.
                    TLiteral::Integer(i64::MIN) => "0x8000000000000000".to_string(),
                    TLiteral::Integer(n) => format!("{}", n),
                    TLiteral::Number(n) => format!("{}", n),
                    // The canonical escaper: an unescaped quote or control
                    // character in a string PATTERN would otherwise emit
                    // unloadable Lua (`if _arg0 == "a"b" then`).
                    TLiteral::Str(s) => lua_quoted_string(s),
                    TLiteral::Bool(b) => if *b { "true".into() } else { "false".into() },
                    TLiteral::Unit => "nil".into(),
                };
                conditions.push(format!("{} == {}", scrutinee, s));
            }
            TPattern::Constructor { name, args } => {
                if self.is_newtype(name) {
                    // Newtype: zero-cost wrapper, value is the inner type directly
                    for arg in args {
                        self.collect_pattern_conditions(scrutinee, arg, conditions, bindings);
                    }
                } else if let Some(str_tag) = self.luadict_enum_tag.get(name) {
                    // LuaDict enum: the value is the constructor's Lua string,
                    // so match by string equality (declaration-order semantics
                    // live in the derived Ord/Enum, not the wire value).
                    conditions.push(format!("{} == {}", scrutinee, lua_quoted_string(str_tag)));
                } else if let Some((tag, total, is_enum)) = self.constructor_info(name) {
                    if is_enum {
                        conditions.push(format!("{} == {}", scrutinee, tag));
                    } else if total > 1 {
                        conditions.push(format!("{}[1] == {}", scrutinee, tag));
                        for (i, arg) in args.iter().enumerate() {
                            let path = Self::field_path(scrutinee, i + 2, arg);
                            self.collect_pattern_conditions(&path, arg, conditions, bindings);
                        }
                    } else if let Some(fields) = self.luadict_con_fields.get(name) {
                        // Single LuaDict constructor: bind each positional
                        // sub-pattern from its named table key.
                        for (i, arg) in args.iter().enumerate() {
                            let path = Self::field_path_key(scrutinee, &fields[i], arg);
                            self.collect_pattern_conditions(&path, arg, conditions, bindings);
                        }
                    } else {
                        for (i, arg) in args.iter().enumerate() {
                            let path = Self::field_path(scrutinee, i + 1, arg);
                            self.collect_pattern_conditions(&path, arg, conditions, bindings);
                        }
                    }
                } else {
                    match name.as_str() {
                        "True" => conditions.push(format!("{} == true", scrutinee)),
                        "False" => conditions.push(format!("{} == false", scrutinee)),
                        "Nothing" | "[]" => conditions.push(format!("{} == nil", scrutinee)),
                        "Just" => {
                            // A Maybe value is either nil (Nothing) or the Just
                            // wrapper, so `~= nil` identifies Just; the payload is
                            // unwrapped from field [1] (which is itself nil for
                            // `Just Nothing` / `Just []`).
                            conditions.push(format!("{} ~= nil", scrutinee));
                            if let Some(arg) = args.first() {
                                // The payload may be a thunk (lazy construction),
                                // so a nested pattern that indexes it (`Just (a,b)`,
                                // `Just (Con ..)`) must force it first — matching a
                                // nested constructor forces each level to WHNF. A
                                // Var/Wildcard sub-pattern binds it lazily and needs
                                // no force. (The general ADT/tuple paths already do
                                // this via field_path; the Just special case did not.)
                                let payload = if Self::pattern_inspects_value(arg) {
                                    format!("__force(({})[1])", scrutinee)
                                } else {
                                    format!("({})[1]", scrutinee)
                                };
                                self.collect_pattern_conditions(&payload, arg, conditions, bindings);
                            }
                        }
                        ":" => {
                            // Cons pattern: x:xs
                            conditions.push(format!("{} ~= nil", scrutinee));
                            if let Some(head_pat) = args.first() {
                                // The head is stored lazily (a cons head is a
                                // lazy position — see the head-consumption
                                // contract on __mll_head), so a nested pattern
                                // that indexes it (`(a,b):_`, `(Con ..):_`, a
                                // literal) must force it to WHNF first. A
                                // Var/Wildcard sub-pattern binds it lazily and
                                // needs no force. Same rule as the `Just` payload.
                                let head = if Self::pattern_inspects_value(head_pat) {
                                    format!("__force(__mll_head({}))", scrutinee)
                                } else {
                                    format!("__mll_head({})", scrutinee)
                                };
                                self.collect_pattern_conditions(&head, head_pat, conditions, bindings);
                            }
                            if args.len() >= 2 {
                                // The tail is a lazy position too: GHC's
                                // `(x:xs)` match forces only this cell, and
                                // `xs` is the tail field UNFORCED. A nested
                                // pattern that inspects the tail (`x:y:ys`,
                                // `[x]`) needs the next cell in WHNF, so it
                                // uses the forcing reader; a Var/Wildcard
                                // sub-pattern binds the raw tail and pulls no
                                // further spine cell (expr_ast forces the
                                // bound variable at each value-use). Same rule
                                // as the head above.
                                let tail_pat = &args[1];
                                let tail = if Self::pattern_inspects_value(tail_pat) {
                                    format!("__mll_tail({})", scrutinee)
                                } else {
                                    format!("__mll_tail_lazy({})", scrutinee)
                                };
                                self.collect_pattern_conditions(&tail, tail_pat, conditions, bindings);
                            }
                        }
                        _ => conditions.push(format!("{} == {}", scrutinee, name)),
                    }
                }
            }
            TPattern::Paren(inner) => self.collect_pattern_conditions(scrutinee, inner, conditions, bindings),
            TPattern::Tuple(pats) => {
                // Tuple fields are at [1], [2], etc. (no tag)
                for (i, p) in pats.iter().enumerate() {
                    let path = Self::field_path(scrutinee, i + 1, p);
                    self.collect_pattern_conditions(&path, p, conditions, bindings);
                }
            }
        }
    }

    /// Returns true if an expression is cheap enough that thunking it would
    /// cost more than evaluating it eagerly. This prevents thunk chain buildup
    /// Collect elements of a literal list (cons chain ending in nil).
    /// Returns Some(vec![elem1, elem2, ...]) if the list has >= 8 literal elements,
    /// None otherwise (let normal cons generation handle short lists).
    pub(super) fn collect_list_literal(expr: &TExpr) -> Option<Vec<&TExpr>> {
        let mut elems = Vec::new();
        let mut cur = expr;
        loop {
            match &cur.kind {
                TExprKind::App(func, tail) => {
                    if let TExprKind::App(inner_f, elem) = &func.kind
                        && let TExprKind::Con(name) = &inner_f.kind
                            && name == ":" {
                                elems.push(elem.as_ref());
                                cur = tail.as_ref();
                                continue;
                            }
                    return None;
                }
                TExprKind::Con(name) if name == "[]" => {
                    if elems.len() >= 8 {
                        return Some(elems);
                    }
                    return None;
                }
                _ => return None,
            }
        }
    }
}
