/// Desugaring pass: transforms do-notation into >>= chains.
///
/// do { x <- e; rest }     =>  e >>= \x -> do { rest }
/// do { e; rest }          =>  e >>= \_ -> do { rest }
/// do { let x = e; rest }  =>  let x = e in do { rest }
/// do { e }                =>  e

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

fn desugar_expr(expr: Expr) -> Expr {
    match expr {
        Expr::Do(stmts) => desugar_do(stmts),
        Expr::App(f, a) => Expr::App(
            Box::new(desugar_expr(*f)),
            Box::new(desugar_expr(*a)),
        ),
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
        Expr::Negate(e) => Expr::Negate(Box::new(desugar_expr(*e))),
        Expr::Paren(e) => Expr::Paren(Box::new(desugar_expr(*e))),
        Expr::Ascription(e, ty) => Expr::Ascription(Box::new(desugar_expr(*e)), ty),
        Expr::RecordCon { constructor, fields } => Expr::RecordCon {
            constructor,
            fields: fields.into_iter().map(|(n, e)| (n, desugar_expr(e))).collect(),
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
        DoStmt::DoLet { expr, .. } => desugar_expr(expr.clone()),
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
            DoStmt::DoLet { name, expr } => {
                let expr = desugar_expr(expr.clone());
                result = Expr::Let {
                    binds: vec![LocalDef {
                        name: name.clone(),
                        patterns: vec![],
                        body: expr,
                    }],
                    body: Box::new(result),
                };
            }
        }
    }
    result
}
