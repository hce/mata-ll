//! Typed Intermediate Representation
//!
//! Like the AST but every expression carries its resolved type.
//! Produced by the type checker, consumed by the monomorphizer and codegen.

use crate::types::{Ty, Subst};

#[derive(Debug, Clone)]
pub struct TModule {
    pub data_defs: Vec<TDataDef>,
    /// Data definitions dropped by constructor-level DCE (`dce.rs`): no kept
    /// function constructs (`Con`) or pattern-matches any of their
    /// constructors, so no constructor function is emitted for them. Codegen
    /// still REGISTERS them (constructor tags, LuaDict keys, FFI field types)
    /// because a value of such a type can flow through kept code without ever
    /// being constructed or matched there — e.g. a LuaDict record built by
    /// the Lua host and consumed only through field accessors, whose keyed
    /// layout comes from this metadata. Empty until DCE runs.
    pub dropped_data_defs: Vec<TDataDef>,
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
    /// True when the type derives `LuaDict`. Two shapes qualify:
    /// - a single record constructor, laid out as a Lua table keyed by field
    ///   name (`{width = …, height = …}`) instead of a positional array; or
    /// - an all-nullary sum type (every constructor has zero fields), whose
    ///   runtime value at the Lua boundary is a *string* — the constructor's
    ///   `external_name` tag when present, its name otherwise — instead of the
    ///   usual positional integer.
    ///
    /// Both exist for interop with Lua APIs that speak dictionaries / string
    /// enums. See codegen's LuaDict handling.
    pub is_luadict: bool,
}

#[derive(Debug, Clone)]
pub struct TConstructor {
    pub name: String,
    /// The `as "tag"` constructor rename, threaded through from the AST. Used
    /// by a derived JSON codec and, for a LuaDict all-nullary sum type, as the
    /// constructor's runtime string value at the Lua boundary.
    pub external_name: Option<String>,
    pub fields: TConFields,
}

impl TConstructor {
    /// The tag this constructor presents at external boundaries (JSON tag, and
    /// the runtime string of a LuaDict enum): the `as "tag"` rename when
    /// present, the constructor name otherwise.
    pub fn effective_tag(&self) -> &str {
        self.external_name.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone)]
pub enum TConFields {
    Positional(Vec<Ty>),
    Named(Vec<TRecordField>),
}

/// A typed named record field. `external_key`, when present, is the shared
/// external rename from `name as "key" :: T`: codegen uses
/// `external_key.unwrap_or(name)` as the runtime Lua table key of a LuaDict
/// type, and the derived ToJSON/FromJSON codecs use it as the JSON object
/// key, while the Haskell-side accessor keeps `name`.
#[derive(Debug, Clone)]
pub struct TRecordField {
    pub name: String,
    pub external_key: Option<String>,
    pub ty: Ty,
}

impl TRecordField {
    /// The name this field presents at external boundaries (Lua table key,
    /// JSON object key).
    pub fn effective_key(&self) -> &str {
        self.external_key.as_deref().unwrap_or(&self.name)
    }
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
    /// True only for compiler-DERIVED Eq/Ord instance methods (`eq_T`,
    /// `ord_compare__T`, `ord_lt__T`, …) from a `deriving` clause. Their
    /// bodies force every argument to WHNF on every path by construction:
    /// structural comparison must inspect both constructor tags before any
    /// clause — including the `_ == _ = False` catch-all — can be selected.
    /// The demand analysis pins their strictness row to all-true, which the
    /// clause-wise AND over the wildcard catch-all otherwise under-approximates
    /// to all-false (see `PRIMITIVE_BINOP_METHODS` in demand.rs for the
    /// primitive-type analogue). Never set for USER-WRITTEN instances, whose
    /// methods may legitimately be lazy in an argument. Monomorphization
    /// clones the function, so specializations of a derived method keep the
    /// marker.
    pub derived_strict: bool,
}

#[derive(Debug, Clone)]
pub struct TClause {
    pub patterns: Vec<TPattern>,
    pub guards: Vec<TGuard>,
    pub body: TExpr,
    pub where_binds: Vec<TLocalDef>,
    /// Source location of the clause this was checked from. `None` for
    /// compiler-synthesized clauses (derived instances, generated impls).
    /// Downstream passes (the monomorphizer) use it to give their
    /// diagnostics a source location.
    pub span: Option<crate::ast::Span>,
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

    /// Rebuild this node with `f` applied to every DIRECT child expression:
    /// operands, guard conditions and bodies, let-binding bodies,
    /// record-update values, dictionary arguments — every `TExpr` exactly one
    /// level down, and nothing deeper. Recursion is the caller's choice, so a
    /// pass with its own depth strategy (e.g. `apply_subst`'s iterative
    /// bind-chain spine) keeps it.
    ///
    /// This is THE enumeration of `TExprKind`'s children: every rewriting
    /// pass goes through here (or `visit_children_mut`), and there is
    /// deliberately no `_ => ...` fallback, so a variant added to `TExprKind`
    /// fails to compile until its children are routed — instead of silently
    /// escaping some of the passes, which is how `DictMethod`'s child
    /// expression historically got skipped by hand-rolled walkers.
    pub fn map_children(self, f: &mut impl FnMut(TExpr) -> TExpr) -> TExpr {
        let TExpr { kind, ty } = self;
        let kind = match kind {
            k @ (TExprKind::Var(_)
            | TExprKind::Con(_)
            | TExprKind::Lit(_)
            | TExprKind::OpFunc(_)
            | TExprKind::DictAccess { .. }) => k,
            TExprKind::App(a, b) => TExprKind::App(Box::new(f(*a)), Box::new(f(*b))),
            TExprKind::Lambda { params, body } => TExprKind::Lambda {
                params,
                body: Box::new(f(*body)),
            },
            TExprKind::InfixApp { op, lhs, rhs } => TExprKind::InfixApp {
                op,
                lhs: Box::new(f(*lhs)),
                rhs: Box::new(f(*rhs)),
            },
            TExprKind::Negate(e) => TExprKind::Negate(Box::new(f(*e))),
            TExprKind::Paren(e) => TExprKind::Paren(Box::new(f(*e))),
            TExprKind::If { cond, then_branch, else_branch } => TExprKind::If {
                cond: Box::new(f(*cond)),
                then_branch: Box::new(f(*then_branch)),
                else_branch: Box::new(f(*else_branch)),
            },
            TExprKind::Case { scrutinee, branches } => TExprKind::Case {
                scrutinee: Box::new(f(*scrutinee)),
                branches: branches.into_iter().map(|b| TCaseBranch {
                    pattern: b.pattern,
                    guards: b.guards.into_iter().map(|g| TGuard {
                        condition: f(g.condition),
                        body: f(g.body),
                    }).collect(),
                    body: f(b.body),
                }).collect(),
            },
            TExprKind::Let { binds, body } => TExprKind::Let {
                binds: binds.into_iter().map(|b| TLocalDef {
                    name: b.name,
                    patterns: b.patterns,
                    body: f(b.body),
                }).collect(),
                body: Box::new(f(*body)),
            },
            TExprKind::SpecCall { original, specialized, args } => TExprKind::SpecCall {
                original,
                specialized,
                args: args.into_iter().map(&mut *f).collect(),
            },
            TExprKind::Tuple(elems) => {
                TExprKind::Tuple(elems.into_iter().map(&mut *f).collect())
            }
            TExprKind::DictMethod { dict, method_name } => TExprKind::DictMethod {
                dict: Box::new(f(*dict)),
                method_name,
            },
            TExprKind::DictCall { func_name, dict_args, value_args } => TExprKind::DictCall {
                func_name,
                dict_args: dict_args.into_iter().map(&mut *f).collect(),
                value_args: value_args.into_iter().map(&mut *f).collect(),
            },
            TExprKind::RecordUpdate { record, updates, num_fields } => TExprKind::RecordUpdate {
                record: Box::new(f(*record)),
                updates: updates.into_iter().map(|(n, i, e)| (n, i, f(e))).collect(),
                num_fields,
            },
            TExprKind::OutgoingCallback { callee, arity, run_io } => {
                TExprKind::OutgoingCallback { callee: Box::new(f(*callee)), arity, run_io }
            }
            TExprKind::FfiMaybeArg { value } => {
                TExprKind::FfiMaybeArg { value: Box::new(f(*value)) }
            }
        };
        TExpr { kind, ty }
    }

    /// In-place twin of [`map_children`](Self::map_children): `f` on every
    /// direct child expression, same single-enumeration guarantee.
    pub fn visit_children_mut(&mut self, f: &mut impl FnMut(&mut TExpr)) {
        match &mut self.kind {
            TExprKind::Var(_)
            | TExprKind::Con(_)
            | TExprKind::Lit(_)
            | TExprKind::OpFunc(_)
            | TExprKind::DictAccess { .. } => {}
            TExprKind::App(a, b) => {
                f(a);
                f(b);
            }
            TExprKind::Lambda { body, .. } => f(body),
            TExprKind::InfixApp { lhs, rhs, .. } => {
                f(lhs);
                f(rhs);
            }
            TExprKind::Negate(e) | TExprKind::Paren(e) => f(e),
            TExprKind::If { cond, then_branch, else_branch } => {
                f(cond);
                f(then_branch);
                f(else_branch);
            }
            TExprKind::Case { scrutinee, branches } => {
                f(scrutinee);
                for b in branches {
                    for g in &mut b.guards {
                        f(&mut g.condition);
                        f(&mut g.body);
                    }
                    f(&mut b.body);
                }
            }
            TExprKind::Let { binds, body } => {
                for b in binds {
                    f(&mut b.body);
                }
                f(body);
            }
            TExprKind::SpecCall { args, .. } => {
                for a in args {
                    f(a);
                }
            }
            TExprKind::Tuple(elems) => {
                for e in elems {
                    f(e);
                }
            }
            TExprKind::DictMethod { dict, .. } => f(dict),
            TExprKind::DictCall { dict_args, value_args, .. } => {
                for a in dict_args {
                    f(a);
                }
                for a in value_args {
                    f(a);
                }
            }
            TExprKind::RecordUpdate { record, updates, .. } => {
                f(record);
                for (_, _, e) in updates {
                    f(e);
                }
            }
            TExprKind::OutgoingCallback { callee, .. } => f(callee),
            TExprKind::FfiMaybeArg { value } => f(value),
        }
    }

    /// Apply substitution to a single node (non-spine). The child
    /// expressions route through `map_children` (recursing via the
    /// spine-aware `apply_subst`); only the non-expression payloads that
    /// carry types — lambda parameters and patterns — need their own arms.
    fn apply_subst_node(self, subst: &Subst) -> Self {
        let ty = self.ty.apply_subst(subst);
        let kind = match self.kind {
            TExprKind::Lambda { params, body } => TExprKind::Lambda {
                params: params.into_iter().map(|(n, t)| (n, t.apply_subst(subst))).collect(),
                body,
            },
            TExprKind::Case { scrutinee, branches } => TExprKind::Case {
                scrutinee,
                branches: branches.into_iter().map(|b| TCaseBranch {
                    pattern: b.pattern.apply_subst(subst),
                    guards: b.guards,
                    body: b.body,
                }).collect(),
            },
            TExprKind::Let { binds, body } => TExprKind::Let {
                binds: binds.into_iter().map(|b| TLocalDef {
                    name: b.name,
                    patterns: b.patterns.into_iter().map(|p| p.apply_subst(subst)).collect(),
                    body: b.body,
                }).collect(),
                body,
            },
            other => other,
        };
        TExpr { kind, ty }.map_children(&mut |c| c.apply_subst(subst))
    }

    /// In-place counterpart of `Ty::demote_skolems` for a whole expression
    /// tree: rewrite this node's type and recurse into every child. App/cons
    /// chains here are shallow (a comprehension's deep spine lives in the
    /// bind-chain form, handled the same as any other child), so plain
    /// recursion is bounded. Child expressions route through
    /// `visit_children_mut`; only the non-expression payloads that carry
    /// types (lambda parameters, patterns) need their own arms.
    pub fn demote_skolems(&mut self, demote: &std::collections::HashMap<u32, Ty>) {
        self.ty = self.ty.demote_skolems(demote);
        match &mut self.kind {
            TExprKind::Lambda { params, .. } => {
                for (_, t) in params.iter_mut() { *t = t.demote_skolems(demote); }
            }
            TExprKind::Case { branches, .. } => {
                for b in branches.iter_mut() { b.pattern.demote_skolems(demote); }
            }
            TExprKind::Let { binds, .. } => {
                for bind in binds.iter_mut() {
                    for p in bind.patterns.iter_mut() { p.demote_skolems(demote); }
                }
            }
            _ => {}
        }
        self.visit_children_mut(&mut |c| c.demote_skolems(demote));
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

    /// In-place skolem demotion for pattern type annotations (see
    /// `Ty::demote_skolems`).
    pub fn demote_skolems(&mut self, demote: &std::collections::HashMap<u32, Ty>) {
        match self {
            TPattern::Var(_, ty) => *ty = ty.demote_skolems(demote),
            TPattern::Constructor { args, .. } => { for p in args.iter_mut() { p.demote_skolems(demote); } }
            TPattern::Paren(p) => p.demote_skolems(demote),
            TPattern::Tuple(ps) => { for p in ps.iter_mut() { p.demote_skolems(demote); } }
            _ => {} // Wildcard, LitPat carry no type
        }
    }
}

impl TClause {
    /// Rebuild every top-level expression this clause owns with `f`: guard
    /// conditions and bodies, the clause body, and each where-binding body.
    /// This is THE coverage list for whole-clause rewrites — the
    /// monomorphizer's fixpoint loops previously inlined it four times and
    /// disagreed (guard expressions were silently skipped in dictionary
    /// contexts).
    pub fn map_exprs(&mut self, f: &mut impl FnMut(TExpr) -> TExpr) {
        fn take(e: &mut TExpr) -> TExpr {
            std::mem::replace(e, TExpr::new(TExprKind::Lit(TLiteral::Unit), Ty::Unit))
        }
        for g in &mut self.guards {
            g.condition = f(take(&mut g.condition));
            g.body = f(take(&mut g.body));
        }
        self.body = f(take(&mut self.body));
        for wb in &mut self.where_binds {
            wb.body = f(take(&mut wb.body));
        }
    }

    pub fn apply_subst(self, subst: &Subst) -> Self {
        TClause {
            span: self.span,
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

    /// Demote every signature skolem in this clause's type annotations back to
    /// its flexible variable (see `Ty::demote_skolems`). Applied once, after a
    /// function's body has been checked rigidly, so the emitted TIR carries the
    /// caller-visible flexible types the monomorphizer expects — not the rigid
    /// skolems used only to reject an over-general body.
    pub fn demote_skolems(&mut self, demote: &std::collections::HashMap<u32, Ty>) {
        for p in &mut self.patterns { p.demote_skolems(demote); }
        for g in &mut self.guards {
            g.condition.demote_skolems(demote);
            g.body.demote_skolems(demote);
        }
        self.body.demote_skolems(demote);
        for b in &mut self.where_binds {
            for p in &mut b.patterns { p.demote_skolems(demote); }
            b.body.demote_skolems(demote);
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
    /// Access a method from a dictionary EXPRESSION (a constructed
    /// dictionary, e.g. the `[a]` dictionary built from the element
    /// dictionary inside a dictionary-passing body). The `dict_param`
    /// form above is the common special case of a plain parameter.
    DictMethod {
        dict: Box<TExpr>,
        method_name: String,
    },
    /// Call a dictionary-passing function with explicit dictionaries.
    DictCall {
        func_name: String,
        dict_args: Vec<TExpr>,
        value_args: Vec<TExpr>,
    },
    /// Record update: copy record, overwrite specific fields
    /// Fields are (field_name, lua_index, new_value)
    RecordUpdate {
        record: Box<TExpr>,
        updates: Vec<(String, usize, TExpr)>,
        num_fields: usize,
    },
    /// An mata-ll callback passed *out* to a Lua FFI function. Lowers to an
    /// n-ary Lua function that uncurries `callee`, marshals each argument and
    /// the result, and (for effectful callbacks) runs the returned action.
    /// The marshalling itself is TYPE-DIRECTED and derived at codegen time
    /// from `callee.ty` — which monomorphization has by then instantiated —
    /// so both boundary directions of a callback (host→callback arguments,
    /// callback→host result) use exactly the same descriptors as the
    /// enclosing FFI call's edges. Deriving it earlier (from the declared
    /// signature, pre-mono) made the two edges disagree for polymorphic
    /// state instantiated at a structured type: the FFI edge marshalled a
    /// `[a]`-instantiated accumulator while the callback edge passed it raw.
    OutgoingCallback {
        callee: Box<TExpr>,
        /// Number of positional arguments the Lua host will pass.
        arity: usize,
        /// The callback returns an action that must be run for its effect.
        run_io: bool,
    },
    /// An FFI argument whose *declared* type in the FFI signature is `Maybe a`
    /// — an optional Lua parameter. At the boundary `Just x` unwraps to `x`
    /// and `Nothing` becomes nil; codegen additionally drops the trailing run
    /// of nil optionals so the Lua callee sees them as genuinely omitted
    /// (arg-count-sensitive hosts like `math.random` distinguish nil from
    /// absent). The flag is decided from the FFI declaration, not the value's
    /// monomorphized type, so a polymorphic parameter instantiated at `Maybe`
    /// is never affected. Only ever appears directly inside a SpecCall arg
    /// list built by `generate_ffi_function`.
    FfiMaybeArg {
        value: Box<TExpr>,
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
    /// A too-large-for-i64 integer literal (an `Integer`), kept as its decimal
    /// string; codegen emits `__int_from_decimal`.
    BigInteger(String),
    Number(f64),
    /// A string literal as its decoded BYTE sequence — see `Literal::Str`.
    Str(Vec<u8>),
    Bool(bool),
    Unit,
}
