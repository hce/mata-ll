//! Desugaring pass: transforms do-notation into >>= chains.
//!
//! do { x <- e; rest }     =>  e >>= \x -> do { rest }
//! do { e; rest }          =>  e >>= \_ -> do { rest }
//! do { let x = e; rest }  =>  let x = e in do { rest }
//! do { p <- e; rest }     =>  e >>= \v -> case v of { p -> do { rest }; _ -> fail "…" }
//! do { e }                =>  e
//!
//! This pass runs AFTER import resolution, on the merged program, so the
//! `>>=`, `fail` and `error` it writes are canonical names
//! (`ast::WiredIn::name`), not references resolution still has to bind.
//! They mean the Prelude's entities wherever the block stands: a module's
//! own top-level `>>=` or `fail` has another canonical name, and
//! resolution left no local binder under either spelling (see
//! `ast::WiredIn`).

use std::collections::HashSet;

use crate::ast::*;

/// Desugar every expression of the merged program. `decl_files` is the
/// per-declaration file attribution the diagnostics use (parallel to
/// `module.decls`); a `MonadFail` message names the file the way GHC
/// does, so a do-block in an imported module reports its own file.
pub fn desugar_module(module: &mut Module, decl_files: &[Option<String>]) {
    let irrefutable_cons = irrefutable_constructors(module);
    for (i, decl) in module.decls.iter_mut().enumerate() {
        let file = decl_files.get(i).and_then(|f| f.as_deref());
        Desugar { file, irrefutable_cons: &irrefutable_cons }.decl(decl);
    }
}

/// The constructors a pattern can match without the possibility of a
/// mismatch: the only constructor of its data type, or a newtype's. GHC's
/// `isIrrefutableHsPat` asks `tyConSingleDataCon_maybe` and nothing else
/// of the constructor — an existential or GADT constructor counts when it
/// is its type's only one (checked against GHC 9.14).
fn irrefutable_constructors(module: &Module) -> HashSet<String> {
    let mut cons = HashSet::new();
    for decl in &module.decls {
        match decl {
            Decl::DataDef { constructors, .. } if constructors.len() == 1 => {
                cons.insert(constructors[0].name.clone());
            }
            // After import resolution a newtype's constructor is named
            // unless it is spelled like the type itself (`newtype W = W …`).
            Decl::NewtypeDef { name, con_name, .. } => {
                cons.insert(con_name.clone().unwrap_or_else(|| name.clone()));
            }
            _ => {}
        }
    }
    cons
}

/// The per-declaration state of the pass: what a `MonadFail` message
/// needs to be GHC's.
struct Desugar<'a> {
    /// The display name of the file the declaration comes from, when the
    /// compiler was told one (`CompileOptions::source_name`, an imported
    /// module's path); `None` renders a bare `line:col`, as diagnostics do.
    file: Option<&'a str>,
    irrefutable_cons: &'a HashSet<String>,
}

impl Desugar<'_> {
    fn decl(&self, decl: &mut Decl) {
        match decl {
            Decl::FunDef { clauses, .. } => {
                for clause in clauses {
                    self.clause(clause);
                }
            }
            Decl::InstanceDecl { methods, .. } => {
                for method in methods {
                    for clause in &mut method.clauses {
                        self.clause(clause);
                    }
                }
            }
            // Default method bodies are checked and emitted like any other
            // clause (`check_function` over `default_clauses`), so they need the
            // same desugaring — a `do` left here reaches the checker's
            // "Do should be desugared" unreachable arm.
            Decl::ClassDecl { methods, .. } => {
                for method in methods {
                    if let Some(clauses) = &mut method.default_clauses {
                        for clause in clauses {
                            self.clause(clause);
                        }
                    }
                }
            }
            // Declarations without expression bodies.
            Decl::TypeSig { .. }
            | Decl::DataDef { .. }
            | Decl::NewtypeDef { .. }
            | Decl::ExportSig { .. }
            | Decl::TypeFamily { .. }
            | Decl::Import { .. }
            | Decl::TypeAlias { .. }
            | Decl::FixityDecl { .. } => {}
        }
    }

    fn clause(&self, clause: &mut Clause) {
        clause.body = clause.body.take().map(|e| self.expr(e));
        for guard in &mut clause.guards {
            guard.condition = self.expr(std::mem::replace(&mut guard.condition, Expr::Lit(Literal::Bool(false))));
            guard.body = self.expr(std::mem::replace(&mut guard.body, Expr::Lit(Literal::Bool(false))));
        }
        for ld in &mut clause.where_binds {
            ld.body = self.expr(std::mem::replace(&mut ld.body, Expr::Lit(Literal::Bool(false))));
        }
    }

    /// Desugar one expression tree. Only two shapes need pass-specific
    /// handling — a `do` block (rewritten to a bind chain) and an application
    /// (its callee lambda is flattened after the operands are desugared);
    /// every other node just recurses through `Expr::map_subexprs`, which is
    /// the single owner of "where do an `Expr`'s children live" (case-branch
    /// guards, tuple elements, record fields, do-statement bodies, …). A
    /// hand-copied walk here once skipped guards and tuples, so a `do` in a
    /// case-guard body or a tuple element reached the checker undesugared.
    fn expr(&self, expr: Expr) -> Expr {
        match expr {
            Expr::Do(stmts) => self.do_block(stmts),
            Expr::App(f, a) => flatten_callee_lambda(Expr::App(
                Box::new(self.expr(*f)),
                Box::new(self.expr(*a)),
            )),
            other => other.map_subexprs(&mut |e| self.expr(e)),
        }
    }

    /// `do { s1; …; sn }` as a bind chain, built bottom-up from the last
    /// statement (iteratively, so a long block never recurses deeply). Owns its
    /// statements: each is moved into the chain, not cloned. The fresh names a
    /// pattern bind mints are indexed by the statement's position (`__tup_i`),
    /// unique within one block; nested blocks are separate calls and their
    /// lambdas shadow, so equal names cannot collide.
    fn do_block(&self, stmts: Vec<DoStmt>) -> Expr {
        let mut stmts = stmts.into_iter().enumerate().rev();
        let Some((_, last)) = stmts.next() else {
            return Expr::Lit(Literal::Bool(false));
        };
        let mut result = match last {
            DoStmt::Expr(expr) | DoStmt::Bind { expr, .. } | DoStmt::PatternBind { expr, .. } => {
                self.expr(expr)
            }
            // A trailing `let` group has no body to bind; a do-block cannot end in
            // `let`, but guard against it by desugaring the last binding's body.
            DoStmt::DoLet { binds } => binds
                .into_iter()
                .last()
                .map(|b| self.expr(b.body))
                .unwrap_or(Expr::Lit(Literal::Bool(false))),
        };

        let bind = |lhs: Expr, param: String, body: Expr| Expr::InfixApp {
            op: WiredIn::Bind.name().to_string(),
            lhs: Box::new(lhs),
            rhs: Box::new(Expr::Lambda { params: vec![param], body: Box::new(body) }),
        };

        for (i, stmt) in stmts {
            result = match stmt {
                DoStmt::Expr(expr) => bind(self.expr(expr), "_".to_string(), result),
                DoStmt::Bind { name, expr } => bind(self.expr(expr), name, result),
                DoStmt::DoLet { binds } => {
                    // Emit the whole `let` group as ONE multi-bind `Expr::Let` so
                    // all bindings share a single mutually-recursive scope. Splitting
                    // it into nested single-bind Lets would make each binding see
                    // only its predecessors, breaking forward references.
                    let binds = binds.into_iter().map(|b| LocalDef {
                        name: b.name,
                        patterns: b.patterns,
                        body: self.expr(b.body),
                        sig: b.sig,
                    }).collect();
                    Expr::Let { binds, body: Box::new(result) }
                }
                DoStmt::PatternBind { pattern, expr, span, end } => {
                    // pat <- expr  =>  expr >>= \__pat -> case __pat of
                    //     { pat -> rest [; _ -> fail "Pattern match failure…"] }
                    //
                    // Haskell 2010 §3.14 / GHC: the fallback arm of a FAILABLE
                    // pattern calls the block's monad's `fail` (the MonadFail
                    // method: `[]` for lists, `Nothing` for Maybe, a raised
                    // IOException for IO), so the block's monad decides what a
                    // mismatch means, and a monad without a MonadFail instance
                    // (Either, ST, a user State monad) rejects the bind at
                    // compile time, as under GHC. The message is GHC's, with
                    // the pattern's span. An irrefutable pattern gets no arm
                    // and asks nothing of the monad.
                    let fresh = format!("__tup_{}", i);
                    let mut branches =
                        vec![CaseBranch { pattern: pattern.clone(), guards: vec![], body: Some(result) }];
                    if !self.pattern_irrefutable(&pattern) {
                        branches.push(CaseBranch {
                            pattern: Pattern::Wildcard,
                            guards: vec![],
                            body: Some(Expr::App(
                                Box::new(Expr::Var(WiredIn::Fail.name().into())),
                                Box::new(Expr::Lit(Literal::Str(
                                    self.fail_message(span, end).into_bytes(),
                                ))),
                            )),
                        });
                    }
                    let case = Expr::Case {
                        scrutinee: Box::new(Expr::Var(fresh.clone())),
                        branches,
                    };
                    bind(self.expr(expr), fresh, case)
                }
            };
        }
        result
    }

    /// GHC's text for a failed do-bind: `Pattern match failure in 'do'
    /// block at FILE:SPAN`, the span rendered as GHC's `SrcSpan` is — one
    /// column `L:C`, one line `L:C1-C2`, several lines `(L1,C1)-(L2,C2)`.
    fn fail_message(&self, start: Span, end: Span) -> String {
        let span = if start.line != end.line {
            format!("({},{})-({},{})", start.line, start.col, end.line, end.col)
        } else if start.col == end.col {
            format!("{}:{}", start.line, start.col)
        } else {
            format!("{}:{}-{}", start.line, start.col, end.col)
        };
        match self.file {
            Some(file) => format!("Pattern match failure in 'do' block at {file}:{span}"),
            None => format!("Pattern match failure in 'do' block at {span}"),
        }
    }

    /// GHC's `isIrrefutableHsPat`, for the do-bind fallback arm: a pattern
    /// that can only fail by diverging, never by mismatch — variables,
    /// wildcards, as-patterns and parentheses over such, tuples and unit, and
    /// a constructor pattern whose constructor is its type's only one (a
    /// newtype's included) with irrefutable arguments. Literal patterns and
    /// constructors with siblings are failable. The rule is syntactic plus
    /// the constructor census of the merged program; what it says decides
    /// whether the bind asks its monad for `MonadFail`, so it must agree
    /// with GHC's exactly: `(a, b) <- e` and `W x <- e` must compile in
    /// every monad.
    fn pattern_irrefutable(&self, p: &Pattern) -> bool {
        match p {
            Pattern::Var(_) | Pattern::Wildcard => true,
            Pattern::LitPat(Literal::Unit) => true,
            Pattern::LitPat(_) => false,
            Pattern::Paren(inner) | Pattern::As(_, inner) => self.pattern_irrefutable(inner),
            Pattern::Tuple(ps) => ps.iter().all(|q| self.pattern_irrefutable(q)),
            Pattern::Constructor { name, args } => {
                self.irrefutable_cons.contains(name)
                    && args.iter().all(|q| self.pattern_irrefutable(q))
            }
        }
    }
}

/// Flatten a nested lambda *only when it is the callee of an application*:
/// `(\t -> \v -> e) 2 3` becomes `(\t v -> e) 2 3`. A nested lambda compiles to
/// nested 1-arg Lua functions; the call site applies every spine argument in
/// one n-ary call, so the surplus are silently dropped and the inner function
/// leaks out. The multi-param form compiles and applies correctly (and the
/// partial-application path applies all args at once, so partial calls work
/// too). Crucially this only fires in callee position — a lambda *passed as an
/// argument* (e.g. `map (\n -> \x -> x + n) ns`, where `map` applies it one arg
/// at a time and expects a function back) keeps its curried 1-arg-layer shape.
fn flatten_callee_lambda(app: Expr) -> Expr {
    let mut args: Vec<Expr> = Vec::new();
    let mut head = app;
    while let Expr::App(f, a) = head {
        args.push(*a);
        head = *f;
    }
    args.reverse();
    // The callee is almost always parenthesized for application — `(\t -> …) x`
    // parses as `App(Paren(Lambda), x)`. Peel redundant parens to reach it.
    while let Expr::Paren(inner) = head {
        head = *inner;
    }
    if let Expr::Lambda { mut params, mut body } = head {
        loop {
            match *body {
                Expr::Lambda { params: inner, body: inner_body } => {
                    params.extend(inner);
                    body = inner_body;
                }
                // peel a redundant paren around the inner lambda
                Expr::Paren(inner) if matches!(*inner, Expr::Lambda { .. }) => body = inner,
                other => { body = Box::new(other); break; }
            }
        }
        head = Expr::Lambda { params, body };
    }
    args.into_iter().fold(head, |f, a| Expr::App(Box::new(f), Box::new(a)))
}
