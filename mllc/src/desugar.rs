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
        _ => {}
    }
}

fn desugar_clause(clause: &mut Clause) {
    clause.body = desugar_expr(std::mem::replace(&mut clause.body, Expr::Lit(Literal::Bool(false))));
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

fn desugar_expr(expr: Expr) -> Expr {
    match expr {
        Expr::Do(stmts) => desugar_do(stmts),
        Expr::App(f, a) => flatten_callee_lambda(Expr::App(
            Box::new(desugar_expr(*f)),
            Box::new(desugar_expr(*a)),
        )),
        Expr::Lambda { params, body } => Expr::Lambda {
            params,
            body: Box::new(desugar_expr(*body)),
        },
        Expr::InfixApp { op, lhs, rhs } => Expr::InfixApp {
            op,
            lhs: Box::new(desugar_expr(*lhs)),
            rhs: Box::new(desugar_expr(*rhs)),
        },
        Expr::If { cond, then_branch, else_branch } => Expr::If {
            cond: Box::new(desugar_expr(*cond)),
            then_branch: Box::new(desugar_expr(*then_branch)),
            else_branch: Box::new(desugar_expr(*else_branch)),
        },
        Expr::Case { scrutinee, branches } => Expr::Case {
            scrutinee: Box::new(desugar_expr(*scrutinee)),
            branches: branches.into_iter().map(|b| CaseBranch {
                pattern: b.pattern,
                guards: b.guards,
                body: desugar_expr(b.body),
            }).collect(),
        },
        Expr::Let { binds, body } => Expr::Let {
            binds: binds.into_iter().map(|ld| LocalDef {
                name: ld.name,
                patterns: ld.patterns,
                body: desugar_expr(ld.body),
            }).collect(),
            body: Box::new(desugar_expr(*body)),
        },
        // Transparent location marker: desugar the inner expression and keep
        // the wrapper so the checker can still locate errors by statement.
        Expr::Spanned(sp, e) => Expr::Spanned(sp, Box::new(desugar_expr(*e))),
        Expr::Negate(e) => Expr::Negate(Box::new(desugar_expr(*e))),
        Expr::Paren(e) => Expr::Paren(Box::new(desugar_expr(*e))),
        Expr::Ascription(e, ty) => Expr::Ascription(Box::new(desugar_expr(*e)), ty),
        Expr::RecordCon { constructor, fields } => Expr::RecordCon {
            constructor,
            fields: fields.into_iter().map(|(n, e)| (n, desugar_expr(e))).collect(),
        },
        Expr::RecordUpdate { expr, updates } => Expr::RecordUpdate {
            expr: Box::new(desugar_expr(*expr)),
            updates: updates.into_iter().map(|(n, e)| (n, desugar_expr(e))).collect(),
        },
        other => other,
    }
}

fn desugar_do(stmts: Vec<DoStmt>) -> Expr {
    desugar_do_stmts(&stmts, 0)
}

fn desugar_do_stmts(stmts: &[DoStmt], idx: usize) -> Expr {
    if idx >= stmts.len() {
        return Expr::Lit(Literal::Bool(false));
    }

    // Build bottom-up iteratively to avoid deep recursion.
    // Start from the last statement (the result) and wrap backwards.
    let last = stmts.len() - 1;
    let mut result = match &stmts[last] {
        DoStmt::Expr(expr) => desugar_expr(expr.clone()),
        DoStmt::Bind { expr, .. } => desugar_expr(expr.clone()),
        // A trailing `let` group has no body to bind; a do-block cannot end in
        // `let`, but guard against it by desugaring the last binding's body.
        DoStmt::DoLet { binds } => binds.last()
            .map(|b| desugar_expr(b.body.clone()))
            .unwrap_or(Expr::Lit(Literal::Bool(false))),
        DoStmt::PatternBind { expr, .. } => desugar_expr(expr.clone()),
        DoStmt::PatternDoLet { expr, .. } => desugar_expr(expr.clone()),
    };

    for i in (idx..last).rev() {
        match &stmts[i] {
            DoStmt::Expr(expr) => {
                let expr = desugar_expr(expr.clone());
                result = Expr::InfixApp {
                    op: ">>=".to_string(),
                    lhs: Box::new(expr),
                    rhs: Box::new(Expr::Lambda {
                        params: vec!["_".to_string()],
                        body: Box::new(result),
                    }),
                };
            }
            DoStmt::Bind { name, expr } => {
                let expr = desugar_expr(expr.clone());
                result = Expr::InfixApp {
                    op: ">>=".to_string(),
                    lhs: Box::new(expr),
                    rhs: Box::new(Expr::Lambda {
                        params: vec![name.clone()],
                        body: Box::new(result),
                    }),
                };
            }
            DoStmt::DoLet { binds } => {
                // Emit the whole `let` group as ONE multi-bind `Expr::Let` so
                // all bindings share a single mutually-recursive scope. Splitting
                // it into nested single-bind Lets would make each binding see
                // only its predecessors, breaking forward references.
                let binds = binds.iter().map(|b| LocalDef {
                    name: b.name.clone(),
                    patterns: b.patterns.clone(),
                    body: desugar_expr(b.body.clone()),
                }).collect();
                result = Expr::Let {
                    binds,
                    body: Box::new(result),
                };
            }
            DoStmt::PatternDoLet { pattern, expr } => {
                // `let (a, b) = expr` becomes ONE recursive binding group:
                // a fresh binding for the scrutinee plus a lazy SELECTOR
                // binding per pattern variable
                // (`a = case __tup of (a, b) -> a`). Haskell pattern
                // bindings are recursive — the variables are in scope for
                // the right-hand side itself (`let (a, b) = (1, a)`) — and
                // lazy: the match happens on first demand of a variable,
                // not eagerly the way wrapping the continuation in a case
                // would force it.
                let expr = desugar_expr(expr.clone());
                let fresh = format!("__tup_{}", i);
                let mut binds = vec![LocalDef {
                    name: fresh.clone(),
                    patterns: vec![],
                    body: expr,
                }];
                for v in crate::ast::pattern_var_names(pattern) {
                    binds.push(LocalDef {
                        name: v.clone(),
                        patterns: vec![],
                        body: Expr::Case {
                            scrutinee: Box::new(Expr::Var(fresh.clone())),
                            branches: vec![CaseBranch {
                                pattern: pattern.clone(),
                                guards: vec![],
                                body: Expr::Var(v),
                            }],
                        },
                    });
                }
                result = Expr::Let {
                    binds,
                    body: Box::new(result),
                };
            }
            DoStmt::PatternBind { pattern, expr } => {
                // (a, b) <- expr => expr >>= \__tup -> case __tup of { (a, b) -> rest }
                let expr = desugar_expr(expr.clone());
                let fresh = format!("__tup_{}", i);
                result = Expr::InfixApp {
                    op: ">>=".to_string(),
                    lhs: Box::new(expr),
                    rhs: Box::new(Expr::Lambda {
                        params: vec![fresh.clone()],
                        body: Box::new(Expr::Case {
                            scrutinee: Box::new(Expr::Var(fresh)),
                            branches: vec![CaseBranch {
                                pattern: pattern.clone(),
                                guards: vec![],
                                body: result,
                            }],
                        }),
                    }),
                };
            }
        }
    }
    result
}
