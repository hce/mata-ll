/// Typed Intermediate Representation
///
/// Like the AST but every expression carries its resolved type.
/// Produced by the type checker, consumed by the monomorphizer and codegen.

use crate::types::{Ty, Subst};

#[derive(Debug, Clone)]
pub struct TModule {
    pub data_defs: Vec<TDataDef>,
    pub functions: Vec<TFunction>,
    /// Instance method implementations, keyed as "ClassName_Type_method"
    pub instance_fns: Vec<TFunction>,
    pub has_main: bool,
    /// Functions exported to Lua
    pub exports: Vec<String>,
    /// Record field accessors: (field_name, lua_index)
    pub record_accessors: Vec<(String, usize)>,
    /// Newtype names (zero-cost wrappers, constructor = identity)
    pub newtypes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TDataDef {
    pub name: String,
    pub type_vars: Vec<String>,
    pub constructors: Vec<TConstructor>,
}

#[derive(Debug, Clone)]
pub struct TConstructor {
    pub name: String,
    pub fields: TConFields,
}

#[derive(Debug, Clone)]
pub enum TConFields {
    Positional(Vec<Ty>),
    Named(Vec<(String, Ty)>),
}

#[derive(Debug, Clone)]
pub struct TFunction {
    pub name: String,
    pub ty: Ty,
    pub clauses: Vec<TClause>,
    /// If true, this is a monomorphized specialization
    pub specialized: bool,
    /// Dictionary parameters for polymorphic-recursive functions
    /// Each entry is (class_name, param_name), e.g. ("Show", "__dict_Show")
    pub dict_params: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct TClause {
    pub patterns: Vec<TPattern>,
    pub guards: Vec<TGuard>,
    pub body: TExpr,
    pub where_binds: Vec<TLocalDef>,
}

#[derive(Debug, Clone)]
pub struct TGuard {
    pub condition: TExpr,
    pub body: TExpr,
}

#[derive(Debug, Clone)]
pub struct TLocalDef {
    pub name: String,
    pub patterns: Vec<TPattern>,
    pub body: TExpr,
}

/// Every expression carries its resolved type.
#[derive(Debug, Clone)]
pub struct TExpr {
    pub kind: TExprKind,
    pub ty: Ty,
}

impl TExpr {
    pub fn new(kind: TExprKind, ty: Ty) -> Self {
        TExpr { kind, ty }
    }

    /// Apply a substitution to all types in this expression tree.
    /// Uses iterative right-spine processing for bind chains (from do-blocks)
    /// to avoid stack overflow on deeply nested expressions.
    pub fn apply_subst(self, subst: &Subst) -> Self {
        // Walk the right spine of bind chains iteratively, collecting
        // frames on the heap. Only recurse for non-spine children.
        enum SpineFrame {
            Bind { ty: Ty, op: String, lhs: TExpr, lambda_ty: Ty, params: Vec<(String, Ty)> },
            Seq { ty: Ty, op: String, lhs: TExpr },
            Let { ty: Ty, binds: Vec<TLocalDef> },
        }

        let mut spine: Vec<SpineFrame> = Vec::new();
        let mut current = self;

        loop {
            match current.kind {
                TExprKind::InfixApp { ref op, .. } if op == ">>=" || op == ">>" => {
                    let ty = current.ty.apply_subst(subst);
                    if let TExprKind::InfixApp { op, lhs, rhs } = current.kind {
                        if op == ">>=" {
                            let rhs_ty = rhs.ty.clone();
                            if let TExprKind::Lambda { params, body } = rhs.kind {
                                let lhs = lhs.apply_subst(subst);
                                let lambda_ty = rhs_ty.apply_subst(subst);
                                let params = params.into_iter().map(|(n, t)| (n, t.apply_subst(subst))).collect();
                                spine.push(SpineFrame::Bind { ty, op, lhs, lambda_ty, params });
                                current = *body;
                                continue;
                            }
                        }
                        // >> or >>= without Lambda rhs
                        let lhs = lhs.apply_subst(subst);
                        spine.push(SpineFrame::Seq { ty, op, lhs });
                        current = *rhs;
                        continue;
                    }
                    unreachable!();
                }
                TExprKind::Let { binds, body } if !spine.is_empty() => {
                    let ty = current.ty.apply_subst(subst);
                    let binds = binds.into_iter().map(|b| TLocalDef {
                        name: b.name,
                        patterns: b.patterns.into_iter().map(|p| p.apply_subst(subst)).collect(),
                        body: b.body.apply_subst(subst),
                    }).collect();
                    spine.push(SpineFrame::Let { ty, binds });
                    current = *body;
                    continue;
                }
                _ => break,
            }
        }

        // Process terminal node with normal recursion (bounded depth)
        let mut result = current.apply_subst_node(subst);

        // Reconstruct spine bottom-up
        for frame in spine.into_iter().rev() {
            result = match frame {
                SpineFrame::Bind { ty, op, lhs, lambda_ty, params } => TExpr {
                    kind: TExprKind::InfixApp {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(TExpr {
                            kind: TExprKind::Lambda { params, body: Box::new(result) },
                            ty: lambda_ty,
                        }),
                    },
                    ty,
                },
                SpineFrame::Seq { ty, op, lhs } => TExpr {
                    kind: TExprKind::InfixApp { op, lhs: Box::new(lhs), rhs: Box::new(result) },
                    ty,
                },
                SpineFrame::Let { ty, binds } => TExpr {
                    kind: TExprKind::Let { binds, body: Box::new(result) },
                    ty,
                },
            };
        }
        result
    }

    /// Apply substitution to a single node (non-spine). Recurses for children
    /// but these have bounded depth (not from bind chains).
    fn apply_subst_node(self, subst: &Subst) -> Self {
        let ty = self.ty.apply_subst(subst);
        let kind = match self.kind {
            TExprKind::App(f, a) => TExprKind::App(
                Box::new(f.apply_subst(subst)),
                Box::new(a.apply_subst(subst)),
            ),
            TExprKind::Lambda { params, body } => TExprKind::Lambda {
                params: params.into_iter().map(|(n, t)| (n, t.apply_subst(subst))).collect(),
                body: Box::new(body.apply_subst(subst)),
            },
            TExprKind::InfixApp { op, lhs, rhs } => TExprKind::InfixApp {
                op,
                lhs: Box::new(lhs.apply_subst(subst)),
                rhs: Box::new(rhs.apply_subst(subst)),
            },
            TExprKind::Negate(e) => TExprKind::Negate(Box::new(e.apply_subst(subst))),
            TExprKind::If { cond, then_branch, else_branch } => TExprKind::If {
                cond: Box::new(cond.apply_subst(subst)),
                then_branch: Box::new(then_branch.apply_subst(subst)),
                else_branch: Box::new(else_branch.apply_subst(subst)),
            },
            TExprKind::Case { scrutinee, branches } => TExprKind::Case {
                scrutinee: Box::new(scrutinee.apply_subst(subst)),
                branches: branches.into_iter().map(|b| TCaseBranch {
                    pattern: b.pattern.apply_subst(subst),
                    guards: b.guards.into_iter().map(|g| TGuard {
                        condition: g.condition.apply_subst(subst),
                        body: g.body.apply_subst(subst),
                    }).collect(),
                    body: b.body.apply_subst(subst),
                }).collect(),
            },
            TExprKind::Let { binds, body } => TExprKind::Let {
                binds: binds.into_iter().map(|b| TLocalDef {
                    name: b.name, patterns: b.patterns.into_iter().map(|p| p.apply_subst(subst)).collect(),
                    body: b.body.apply_subst(subst),
                }).collect(),
                body: Box::new(body.apply_subst(subst)),
            },
            TExprKind::Paren(e) => TExprKind::Paren(Box::new(e.apply_subst(subst))),
            TExprKind::SpecCall { original, specialized, args } => TExprKind::SpecCall {
                original, specialized,
                args: args.into_iter().map(|a| a.apply_subst(subst)).collect(),
            },
            TExprKind::Tuple(elems) => TExprKind::Tuple(
                elems.into_iter().map(|e| e.apply_subst(subst)).collect(),
            ),
            TExprKind::DictCall { func_name, dict_args, value_args } => TExprKind::DictCall {
                func_name,
                dict_args: dict_args.into_iter().map(|a| a.apply_subst(subst)).collect(),
                value_args: value_args.into_iter().map(|a| a.apply_subst(subst)).collect(),
            },
            other => other,
        };
        TExpr { kind, ty }
    }
}

impl TPattern {
    pub fn apply_subst(self, subst: &Subst) -> Self {
        match self {
            TPattern::Var(n, ty) => TPattern::Var(n, ty.apply_subst(subst)),
            TPattern::Constructor { name, args } => TPattern::Constructor {
                name,
                args: args.into_iter().map(|p| p.apply_subst(subst)).collect(),
            },
            TPattern::Paren(p) => TPattern::Paren(Box::new(p.apply_subst(subst))),
            TPattern::Tuple(ps) => TPattern::Tuple(ps.into_iter().map(|p| p.apply_subst(subst)).collect()),
            other => other, // Wildcard, LitPat
        }
    }
}

impl TClause {
    pub fn apply_subst(self, subst: &Subst) -> Self {
        TClause {
            patterns: self.patterns.into_iter().map(|p| p.apply_subst(subst)).collect(),
            guards: self.guards.into_iter().map(|g| TGuard {
                condition: g.condition.apply_subst(subst),
                body: g.body.apply_subst(subst),
            }).collect(),
            body: self.body.apply_subst(subst),
            where_binds: self.where_binds.into_iter().map(|b| TLocalDef {
                name: b.name, patterns: b.patterns.into_iter().map(|p| p.apply_subst(subst)).collect(),
                body: b.body.apply_subst(subst),
            }).collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum TExprKind {
    Var(String),
    Con(String),
    Lit(TLiteral),
    App(Box<TExpr>, Box<TExpr>),
    Lambda {
        params: Vec<(String, Ty)>,
        body: Box<TExpr>,
    },
    InfixApp {
        op: String,
        lhs: Box<TExpr>,
        rhs: Box<TExpr>,
    },
    Negate(Box<TExpr>),
    If {
        cond: Box<TExpr>,
        then_branch: Box<TExpr>,
        else_branch: Box<TExpr>,
    },
    Case {
        scrutinee: Box<TExpr>,
        branches: Vec<TCaseBranch>,
    },
    Let {
        binds: Vec<TLocalDef>,
        body: Box<TExpr>,
    },
    Paren(Box<TExpr>),
    OpFunc(String),
    /// A call to a specific monomorphized specialization.
    /// Original name + mangled specialized name.
    SpecCall {
        original: String,
        specialized: String,
        args: Vec<TExpr>,
    },
    Tuple(Vec<TExpr>),
    /// Access a method from a typeclass dictionary parameter.
    DictAccess {
        dict_param: String,
        method_name: String,
    },
    /// Call a dictionary-passing function with explicit dictionaries.
    DictCall {
        func_name: String,
        dict_args: Vec<TExpr>,
        value_args: Vec<TExpr>,
    },
}

#[derive(Debug, Clone)]
pub struct TCaseBranch {
    pub pattern: TPattern,
    pub guards: Vec<TGuard>,
    pub body: TExpr,
}


#[derive(Debug, Clone)]
pub enum TPattern {
    Var(String, Ty),
    Wildcard,
    Constructor {
        name: String,
        args: Vec<TPattern>,
    },
    LitPat(TLiteral),
    Paren(Box<TPattern>),
    Tuple(Vec<TPattern>),
}

#[derive(Debug, Clone)]
pub enum TLiteral {
    Integer(i64),
    Number(f64),
    Str(String),
    Bool(bool),
    Unit,
}
