//! Desugaring pass: transforms do-notation into >>= chains.
//!
//! do { x <- e; rest }     =>  e >>= \x -> do { rest }
//! do { e; rest }          =>  e >>= \_ -> do { rest }
//! do { let x = e; rest }  =>  let x = e in do { rest }
//! do { e }                =>  e

use crate::ast::*;

pub fn desugar_module(module: &mut Module) {
    for decl in &mut module.decls {
        desugar_decl(decl);
    }
}

fn desugar_decl(decl: &mut Decl) {
    match decl {
        Decl::FunDef { clauses, .. } => {
            for clause in clauses {
                desugar_clause(clause);
            }
        }
        Decl::InstanceDecl { methods, .. } => {
            for method in methods {
                for clause in &mut method.clauses {
                    desugar_clause(clause);
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
                        desugar_clause(clause);
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

fn desugar_clause(clause: &mut Clause) {
    clause.body = clause.body.take().map(desugar_expr);
    for guard in &mut clause.guards {
        guard.condition = desugar_expr(std::mem::replace(&mut guard.condition, Expr::Lit(Literal::Bool(false))));
        guard.body = desugar_expr(std::mem::replace(&mut guard.body, Expr::Lit(Literal::Bool(false))));
    }
    for ld in &mut clause.where_binds {
        ld.body = desugar_expr(std::mem::replace(&mut ld.body, Expr::Lit(Literal::Bool(false))));
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

/// Desugar one expression tree. Only two shapes need pass-specific
/// handling — a `do` block (rewritten to a bind chain) and an application
/// (its callee lambda is flattened after the operands are desugared);
/// every other node just recurses through `Expr::map_subexprs`, which is
/// the single owner of "where do an `Expr`'s children live" (case-branch
/// guards, tuple elements, record fields, do-statement bodies, …). A
/// hand-copied walk here once skipped guards and tuples, so a `do` in a
/// case-guard body or a tuple element reached the checker undesugared.
fn desugar_expr(expr: Expr) -> Expr {
    match expr {
        Expr::Do(stmts) => desugar_do(stmts),
        Expr::App(f, a) => flatten_callee_lambda(Expr::App(
            Box::new(desugar_expr(*f)),
            Box::new(desugar_expr(*a)),
        )),
        other => other.map_subexprs(&mut desugar_expr),
    }
}

/// `do { s1; …; sn }` as a bind chain, built bottom-up from the last
/// statement (iteratively, so a long block never recurses deeply). Owns its
/// statements: each is moved into the chain, not cloned. The fresh names a
/// pattern bind mints are indexed by the statement's position (`__tup_i`),
/// unique within one block; nested blocks are separate calls and their
/// lambdas shadow, so equal names cannot collide.
/// Syntactic irrefutability, for the do-bind fallback arm: a pattern that
/// can never fail to match — variables, wildcards, and tuples of such
/// (tuples have one constructor). Constructor, literal, and as-patterns are
/// refutable here even when the TYPE has a single constructor: that is not
/// knowable before type checking, and an extra fallback arm on an
/// exhaustive case is dead code, not a semantics change.
fn pattern_irrefutable(p: &Pattern) -> bool {
    match p {
        Pattern::Var(_) | Pattern::Wildcard => true,
        Pattern::Paren(inner) => pattern_irrefutable(inner),
        Pattern::Tuple(ps) => ps.iter().all(pattern_irrefutable),
        Pattern::Constructor { .. } | Pattern::LitPat(_) | Pattern::As(..) => false,
    }
}

fn desugar_do(stmts: Vec<DoStmt>) -> Expr {
    let mut stmts = stmts.into_iter().enumerate().rev();
    let Some((_, last)) = stmts.next() else {
        return Expr::Lit(Literal::Bool(false));
    };
    let mut result = match last {
        DoStmt::Expr(expr) | DoStmt::Bind { expr, .. } | DoStmt::PatternBind { expr, .. } => {
            desugar_expr(expr)
        }
        // A trailing `let` group has no body to bind; a do-block cannot end in
        // `let`, but guard against it by desugaring the last binding's body.
        DoStmt::DoLet { binds } => binds
            .into_iter()
            .last()
            .map(|b| desugar_expr(b.body))
            .unwrap_or(Expr::Lit(Literal::Bool(false))),
    };

    let bind = |lhs: Expr, param: String, body: Expr| Expr::InfixApp {
        op: ">>=".to_string(),
        lhs: Box::new(lhs),
        rhs: Box::new(Expr::Lambda { params: vec![param], body: Box::new(body) }),
    };

    for (i, stmt) in stmts {
        result = match stmt {
            DoStmt::Expr(expr) => bind(desugar_expr(expr), "_".to_string(), result),
            DoStmt::Bind { name, expr } => bind(desugar_expr(expr), name, result),
            DoStmt::DoLet { binds } => {
                // Emit the whole `let` group as ONE multi-bind `Expr::Let` so
                // all bindings share a single mutually-recursive scope. Splitting
                // it into nested single-bind Lets would make each binding see
                // only its predecessors, breaking forward references.
                let binds = binds.into_iter().map(|b| LocalDef {
                    name: b.name,
                    patterns: b.patterns,
                    body: desugar_expr(b.body),
                }).collect();
                Expr::Let { binds, body: Box::new(result) }
            }
            DoStmt::PatternBind { pattern, expr, span } => {
                // pat <- expr  =>  expr >>= \__pat -> case __pat of
                //     { pat -> rest [; _ -> error "Pattern match failure…"] }
                //
                // The fallback arm is GHC's MonadFail semantics for a
                // REFUTABLE bind pattern (`Just x <- action`): a mismatch
                // raises "Pattern match failure in do expression", located.
                // A syntactically irrefutable pattern — `(a, b)`, `()`,
                // nested wildcards — gets no arm, exactly as before (A15).
                let fresh = format!("__tup_{}", i);
                let mut branches =
                    vec![CaseBranch { pattern: pattern.clone(), guards: vec![], body: Some(result) }];
                if !pattern_irrefutable(&pattern) {
                    branches.push(CaseBranch {
                        pattern: Pattern::Wildcard,
                        guards: vec![],
                        body: Some(Expr::App(
                            Box::new(Expr::Var("error".into())),
                            Box::new(Expr::Lit(Literal::Str(
                                format!(
                                    "Pattern match failure in do expression at {}:{}",
                                    span.line, span.col
                                )
                                .into_bytes(),
                            ))),
                        )),
                    });
                }
                let case = Expr::Case {
                    scrutinee: Box::new(Expr::Var(fresh.clone())),
                    branches,
                };
                bind(desugar_expr(expr), fresh, case)
            }
        };
    }
    result
}
