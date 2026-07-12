//! `deriving (...)` implementations: Show, Eq, Ord, Enum, Bounded,
//! Functor, LuaDict, and the ToJSON/FromJSON codecs. Moved verbatim out
//! of the monolithic typechecker.rs; `use super::*` keeps every name
//! resolution identical.

use super::*;

impl Checker {
    // --- Deriving ---

    pub(super) fn derive_instance(
        &mut self,
        class: &str,
        type_name: &str,
        type_vars: &[String],
        constructors: &[Constructor],
    ) -> Vec<TFunction> {
        match class {
            "Show" => self.derive_show(type_name, type_vars, constructors),
            "Eq" => self.derive_eq(type_name, type_vars, constructors),
            "Ord" => self.derive_ord(type_name, type_vars, constructors),
            "Enum" => self.derive_enum(type_name, type_vars, constructors),
            "Bounded" => self.derive_bounded(type_name, type_vars, constructors),
            "Functor" => self.derive_functor(type_name, type_vars, constructors),
            "ToJSON" => self.derive_tojson(type_name, type_vars, constructors),
            "FromJSON" => self.derive_fromjson(type_name, type_vars, constructors),
            "LuaDict" => { self.derive_luadict(type_name, constructors); vec![] }
            other => {
                self.push_error_ctx(
                    DiagnosticKind::Other(format!("Cannot derive '{}' — only Show, Eq, Ord, Enum, Bounded, Functor, ToJSON, FromJSON and LuaDict are supported", other)),
                    format!("data {}", type_name),
                );
                vec![]
            }
        }
    }

    /// `LuaDict` is an intrinsic deriving: it generates no instance methods but
    /// changes the runtime layout so the value is a Lua table keyed by field
    /// name (`{width = …}`) instead of a positional array. That representation
    /// only makes sense for a single record constructor whose fields all have
    /// names to use as keys, so we validate that here and reject anything else
    /// with an explanation of *why* rather than a bare "cannot derive".
    pub(super) fn derive_luadict(&mut self, type_name: &str, constructors: &[Constructor]) {
        let reject = |checker: &mut Self, reason: String, note: &str| {
            checker.push_error_ctx(
                DiagnosticKind::Other(format!(
                    "Cannot derive 'LuaDict' for '{}': {}\nnote: {}",
                    type_name, reason, note,
                )),
                format!("data {}", type_name),
            );
        };

        if constructors.len() != 1 {
            reject(self,
                format!("LuaDict needs exactly one constructor, but '{}' has {}", type_name, constructors.len()),
                "the generated Lua table has no tag to tell variants apart, so a name-keyed dictionary can only represent a single-constructor record.");
            return;
        }

        let con = &constructors[0];
        if con.gadt_type.is_some() || !con.existential_vars.is_empty() {
            reject(self,
                format!("'{}' is a GADT / existential constructor", con.name),
                "LuaDict keys the table by record field name, which GADT and existential constructors do not provide.");
            return;
        }

        match &con.fields {
            ConstructorFields::Named(fields) if !fields.is_empty() => {
                // Validate the *effective* Lua keys (the `as "key"` rename when
                // present, the field name otherwise): each must be a non-empty
                // string, and no two fields may share one — they become the keys
                // of a single Lua table.
                let mut seen: HashMap<&str, &str> = HashMap::new();
                for field in fields {
                    let key = field.effective_key();
                    if key.is_empty() {
                        reject(self,
                            format!("field '{}' renames its Lua key to the empty string", field.name),
                            "the field name becomes a key in the runtime Lua table, and an empty key names nothing a Lua host could sensibly read; give `as` a non-empty string.");
                        return;
                    }
                    if let Some(prev) = seen.insert(key, &field.name) {
                        reject(self,
                            format!("fields '{}' and '{}' both map to the Lua key \"{}\"", prev, field.name, key),
                            "each record field becomes one key in the runtime Lua table, so two fields sharing a key would silently overwrite each other; rename one with `as \"otherKey\"`.");
                        return;
                    }
                }
                self.luadict_types.insert(type_name.to_string());
            }
            ConstructorFields::Named(_) => {
                reject(self,
                    format!("constructor '{}' has no fields", con.name),
                    "LuaDict maps record fields to Lua table keys; there is nothing to key on an empty record.");
            }
            ConstructorFields::Positional(_) => {
                reject(self,
                    format!("constructor '{}' uses positional fields", con.name),
                    "LuaDict keys the Lua table by field name, so it requires record syntax: `data … = … { field :: T, … }`.");
            }
        }
    }

    /// Generate `show` for a data type.
    /// For each constructor, generates a clause that produces "Constructor field1 field2 ...".
    pub(super) fn derive_show(
        &mut self,
        type_name: &str,
        type_vars: &[String],
        constructors: &[Constructor],
    ) -> Vec<TFunction> {
        let tvars: Vec<TyVar> = type_vars.iter()
            .map(|n| TyVar { name: n.clone(), id: u32::MAX })
            .collect();
        let result_type = tvars.iter().fold(
            Ty::Con(type_name.to_string()),
            |acc, tv| Ty::app(acc, Ty::Var(tv.clone())),
        );

        let mangled = format!("show_{}", type_name);
        let fn_ty = Ty::arrow(result_type.clone(), Ty::Con("String".into()));

        let mut clauses = Vec::new();
        for con in constructors {
            let field_count = match &con.fields {
                ConstructorFields::Positional(fs) => fs.len(),
                ConstructorFields::Named(fs) => fs.len(),
            };

            // Build patterns: Con p0 p1 p2 ...
            let param_names: Vec<String> = (0..field_count)
                .map(|i| format!("_s{}", i))
                .collect();

            // TIR references use the registered key (mangled when this local
            // constructor shadows a Prelude/import one); the *displayed* name
            // stays the source name the user wrote.
            let con_key = self.resolve_con_name(&con.name).to_string();
            let con_info = self.constructors.get(&con_key).cloned();
            let field_tys: Vec<Ty> = con_info.as_ref()
                .map(|ci| ci.field_types.clone())
                .unwrap_or_default();

            let patterns = vec![
                TPattern::Constructor {
                    name: con_key,
                    args: param_names.iter().enumerate().map(|(i, n)| {
                        let ty = field_tys.get(i).cloned().unwrap_or(Ty::Unit);
                        TPattern::Var(n.clone(), ty)
                    }).collect(),
                }
            ];

            // Build body: "ConName" ++ " " ++ show p0 ++ " " ++ show p1 ...
            let mut body = TExpr::new(
                TExprKind::Lit(TLiteral::Str(con.name.clone())),
                Ty::Con("String".into()),
            );

            for (i, pname) in param_names.iter().enumerate() {
                let field_ty = field_tys.get(i).cloned().unwrap_or(Ty::Unit);

                // " "
                let space = TExpr::new(
                    TExprKind::Lit(TLiteral::Str(" ".into())),
                    Ty::Con("String".into()),
                );
                // concat body <> " "
                body = TExpr::new(
                    TExprKind::InfixApp {
                        op: "<>".into(),
                        lhs: Box::new(body),
                        rhs: Box::new(space),
                    },
                    Ty::Con("String".into()),
                );

                // __mll_show_arg (show field_i) — parenthesize the field if it is
                // a constructor application or negative number (GHC showsPrec 11).
                let field_shown = TExpr::new(
                    TExprKind::App(
                        Box::new(TExpr::new(
                            TExprKind::Var("__mll_show_arg".into()),
                            Ty::arrow(Ty::Con("String".into()), Ty::Con("String".into())),
                        )),
                        Box::new(TExpr::new(
                            TExprKind::App(
                                Box::new(TExpr::new(
                                    TExprKind::Var("show".into()),
                                    Ty::arrow(field_ty.clone(), Ty::Con("String".into())),
                                )),
                                Box::new(TExpr::new(
                                    TExprKind::Var(pname.clone()),
                                    field_ty,
                                )),
                            ),
                            Ty::Con("String".into()),
                        )),
                    ),
                    Ty::Con("String".into()),
                );

                body = TExpr::new(
                    TExprKind::InfixApp {
                        op: "<>".into(),
                        lhs: Box::new(body),
                        rhs: Box::new(field_shown),
                    },
                    Ty::Con("String".into()),
                );
            }

            clauses.push(TClause {
                span: None,
                patterns,
                guards: vec![],
                body,
                where_binds: vec![],
            });
        }

        // Register the instance
        let mut method_fns = HashMap::new();
        method_fns.insert("show".to_string(), mangled.clone());
        self.register_instance(InstanceInfo {
            class_name: "Show".to_string(),
            target_type: result_type.clone(),
            method_fns,
            context: None,
        });

        vec![TFunction {
            name: mangled,
            ty: fn_ty,
            clauses,
            specialized: false,
            dict_params: vec![],
        }]
    }

    /// Generate `==` for a data type.
    /// Two values are equal if they have the same constructor and all fields are equal.
    pub(super) fn derive_eq(
        &mut self,
        type_name: &str,
        type_vars: &[String],
        constructors: &[Constructor],
    ) -> Vec<TFunction> {
        let tvars: Vec<TyVar> = type_vars.iter()
            .map(|n| TyVar { name: n.clone(), id: u32::MAX })
            .collect();
        let result_type = tvars.iter().fold(
            Ty::Con(type_name.to_string()),
            |acc, tv| Ty::app(acc, Ty::Var(tv.clone())),
        );

        let mangled = format!("eq_{}", type_name);
        let fn_ty = Ty::fun(&[result_type.clone(), result_type.clone()], Ty::Con("Bool".into()));

        let mut clauses = Vec::new();

        for con in constructors {
            let field_count = match &con.fields {
                ConstructorFields::Positional(fs) => fs.len(),
                ConstructorFields::Named(fs) => fs.len(),
            };

            let con_key = self.resolve_con_name(&con.name).to_string();
            let con_info = self.constructors.get(&con_key).cloned();
            let field_tys: Vec<Ty> = con_info.as_ref()
                .map(|ci| ci.field_types.clone())
                .unwrap_or_default();

            let a_names: Vec<String> = (0..field_count).map(|i| format!("_a{}", i)).collect();
            let b_names: Vec<String> = (0..field_count).map(|i| format!("_b{}", i)).collect();

            let pat_a = TPattern::Constructor {
                name: con_key.clone(),
                args: a_names.iter().enumerate().map(|(i, n)| {
                    let ty = field_tys.get(i).cloned().unwrap_or(Ty::Unit);
                    TPattern::Var(n.clone(), ty)
                }).collect(),
            };
            let pat_b = TPattern::Constructor {
                name: con_key,
                args: b_names.iter().enumerate().map(|(i, n)| {
                    let ty = field_tys.get(i).cloned().unwrap_or(Ty::Unit);
                    TPattern::Var(n.clone(), ty)
                }).collect(),
            };

            // Build body: a0 == b0 && a1 == b1 && ...
            // The monomorphizer resolves == to the appropriate eq function
            // for each field type (including polymorphic types like Tree a
            // where the type constructor has a known Eq instance).
            let mut body = TExpr::new(
                TExprKind::Lit(TLiteral::Bool(true)),
                Ty::Con("Bool".into()),
            );

            for i in (0..field_count).rev() {
                let field_ty = field_tys.get(i).cloned().unwrap_or(Ty::Unit);
                let eq_expr = TExpr::new(
                    TExprKind::InfixApp {
                        op: "==".into(),
                        lhs: Box::new(TExpr::new(
                            TExprKind::Var(a_names[i].clone()),
                            field_ty.clone(),
                        )),
                        rhs: Box::new(TExpr::new(
                            TExprKind::Var(b_names[i].clone()),
                            field_ty,
                        )),
                    },
                    Ty::Con("Bool".into()),
                );
                body = TExpr::new(
                    TExprKind::InfixApp {
                        op: "&&".into(),
                        lhs: Box::new(eq_expr),
                        rhs: Box::new(body),
                    },
                    Ty::Con("Bool".into()),
                );
            }

            clauses.push(TClause {
                span: None,
                patterns: vec![pat_a, pat_b],
                guards: vec![],
                body,
                where_binds: vec![],
            });
        }

        // Add catch-all clause for different constructors: _ _ = False
        if constructors.len() > 1 {
            clauses.push(TClause {
                span: None,
                patterns: vec![
                    TPattern::Wildcard,
                    TPattern::Wildcard,
                ],
                guards: vec![],
                body: TExpr::new(TExprKind::Lit(TLiteral::Bool(false)), Ty::Con("Bool".into())),
                where_binds: vec![],
            });
        }

        // Register the instance
        let mut method_fns = HashMap::new();
        method_fns.insert("==".to_string(), mangled.clone());
        self.register_instance(InstanceInfo {
            class_name: "Eq".to_string(),
            target_type: result_type.clone(),
            method_fns,
            context: None,
        });

        vec![TFunction {
            name: mangled,
            ty: fn_ty,
            clauses,
            specialized: false,
            dict_params: vec![],
        }]
    }

    pub(super) fn derive_ord(
        &mut self,
        type_name: &str,
        type_vars: &[String],
        constructors: &[Constructor],
    ) -> Vec<TFunction> {
        let tvars: Vec<TyVar> = type_vars.iter()
            .map(|n| TyVar { name: n.clone(), id: u32::MAX })
            .collect();
        let result_type = tvars.iter().fold(
            Ty::Con(type_name.to_string()),
            |acc, tv| Ty::app(acc, Ty::Var(tv.clone())),
        );
        let bool_ty = Ty::Con("Bool".into());
        let fn_ty = Ty::fun(&[result_type.clone(), result_type.clone()], bool_ty.clone());

        // For enums (no fields), use constructor index comparison.
        // Constructors earlier in the declaration are "less than" later ones.
        let is_enum = constructors.iter().all(|c| match &c.fields {
            ConstructorFields::Positional(fs) => fs.is_empty(),
            ConstructorFields::Named(fs) => fs.is_empty(),
        });

        let mut functions = Vec::new();

        let ordering_ty = Ty::Con("Ordering".into());
        let compare_fn_ty = Ty::fun(&[result_type.clone(), result_type.clone()], ordering_ty.clone());
        let compare_name = format!("ord_compare__{}", type_name);

        // compare: cross-constructor pairs order by declaration index
        // (earlier constructor < later); same-constructor pairs compare
        // fields left-to-right, lexicographically, using each field type's
        // own `compare` (resolved by the monomorphizer exactly like the
        // per-field `==` in derive_eq). A nullary constructor is EQ to itself.
        let mut compare_clauses = Vec::new();
        for (i, con_a) in constructors.iter().enumerate() {
            let fc_a = match &con_a.fields {
                ConstructorFields::Positional(fs) => fs.len(),
                ConstructorFields::Named(fs) => fs.len(),
            };
            for (j, con_b) in constructors.iter().enumerate() {
                let fc_b = match &con_b.fields {
                    ConstructorFields::Positional(fs) => fs.len(),
                    ConstructorFields::Named(fs) => fs.len(),
                };
                if i != j {
                    // Different constructors: index decides; fields are irrelevant.
                    let ord_con = if i < j { "LT" } else { "GT" };
                    let a_args: Vec<TPattern> = (0..fc_a).map(|_| TPattern::Wildcard).collect();
                    let b_args: Vec<TPattern> = (0..fc_b).map(|_| TPattern::Wildcard).collect();
                    compare_clauses.push(TClause {
                        span: None,
                        patterns: vec![
                            TPattern::Constructor { name: self.resolve_con_name(&con_a.name).to_string(), args: a_args },
                            TPattern::Constructor { name: self.resolve_con_name(&con_b.name).to_string(), args: b_args },
                        ],
                        guards: vec![],
                        body: TExpr::new(TExprKind::Con(ord_con.to_string()), ordering_ty.clone()),
                        where_binds: vec![],
                    });
                    continue;
                }

                // Same constructor: bind fields with their real types (as
                // derive_eq does) so the monomorphizer can resolve `compare`
                // per field type.
                let con_key = self.resolve_con_name(&con_a.name).to_string();
                let field_tys: Vec<Ty> = self.constructors.get(&con_key)
                    .map(|ci| ci.field_types.clone())
                    .unwrap_or_default();
                let a_names: Vec<String> = (0..fc_a).map(|k| format!("_a{}", k)).collect();
                let b_names: Vec<String> = (0..fc_a).map(|k| format!("_b{}", k)).collect();
                let pat_a = TPattern::Constructor {
                    name: con_key.clone(),
                    args: a_names.iter().enumerate().map(|(k, n)| {
                        let ty = field_tys.get(k).cloned().unwrap_or(Ty::Unit);
                        TPattern::Var(n.clone(), ty)
                    }).collect(),
                };
                let pat_b = TPattern::Constructor {
                    name: con_key,
                    args: b_names.iter().enumerate().map(|(k, n)| {
                        let ty = field_tys.get(k).cloned().unwrap_or(Ty::Unit);
                        TPattern::Var(n.clone(), ty)
                    }).collect(),
                };

                // Lexicographic chain, built inside-out:
                //   case compare a0 b0 of { EQ -> <compare remaining>; o -> o }
                // The innermost step is the last field's bare `compare`;
                // zero fields is simply EQ.
                let mk_field_compare = |k: usize| {
                    let fty = field_tys.get(k).cloned().unwrap_or(Ty::Unit);
                    let cmp_ty = Ty::fun(&[fty.clone(), fty.clone()], ordering_ty.clone());
                    let partial_ty = Ty::fun(std::slice::from_ref(&fty), ordering_ty.clone());
                    TExpr::new(
                        TExprKind::App(
                            Box::new(TExpr::new(
                                TExprKind::App(
                                    Box::new(TExpr::new(TExprKind::Var("compare".into()), cmp_ty)),
                                    Box::new(TExpr::new(TExprKind::Var(a_names[k].clone()), fty.clone())),
                                ),
                                partial_ty,
                            )),
                            Box::new(TExpr::new(TExprKind::Var(b_names[k].clone()), fty)),
                        ),
                        ordering_ty.clone(),
                    )
                };

                let mut body = if fc_a == 0 {
                    TExpr::new(TExprKind::Con("EQ".into()), ordering_ty.clone())
                } else {
                    mk_field_compare(fc_a - 1)
                };
                for k in (0..fc_a.saturating_sub(1)).rev() {
                    body = TExpr::new(
                        TExprKind::Case {
                            scrutinee: Box::new(mk_field_compare(k)),
                            branches: vec![
                                TCaseBranch {
                                    pattern: TPattern::Constructor { name: "EQ".into(), args: vec![] },
                                    guards: vec![],
                                    body,
                                },
                                TCaseBranch {
                                    pattern: TPattern::Var("_o".into(), ordering_ty.clone()),
                                    guards: vec![],
                                    body: TExpr::new(TExprKind::Var("_o".into()), ordering_ty.clone()),
                                },
                            ],
                        },
                        ordering_ty.clone(),
                    );
                }

                compare_clauses.push(TClause {
                    span: None,
                    patterns: vec![pat_a, pat_b],
                    guards: vec![],
                    body,
                    where_binds: vec![],
                });
            }
        }
        functions.push(TFunction {
            name: compare_name.clone(),
            ty: compare_fn_ty.clone(),
            clauses: compare_clauses,
            specialized: false,
            dict_params: vec![],
        });

        for (op, op_name) in &[("<", "lt"), (">", "gt"), ("<=", "le"), (">=", "ge")] {
            let mangled = format!("ord_{}__{}", op_name, type_name);

            let clauses = if is_enum {
                // Enum: fields can't exist, so the constructor index fully
                // decides every relational op. Emit direct Bool clauses per
                // constructor pair (no Ordering round-trip on hot paths).
                let mut cls = Vec::new();
                for (i, con_a) in constructors.iter().enumerate() {
                    for (j, con_b) in constructors.iter().enumerate() {
                        let result = match *op {
                            "<" => i < j,
                            ">" => i > j,
                            "<=" => i <= j,
                            ">=" => i >= j,
                            _ => unreachable!(),
                        };
                        cls.push(TClause {
                            span: None,
                            patterns: vec![
                                TPattern::Constructor { name: self.resolve_con_name(&con_a.name).to_string(), args: vec![] },
                                TPattern::Constructor { name: self.resolve_con_name(&con_b.name).to_string(), args: vec![] },
                            ],
                            guards: vec![],
                            body: TExpr::new(TExprKind::Lit(TLiteral::Bool(result)), bool_ty.clone()),
                            where_binds: vec![],
                        });
                    }
                }
                cls
            } else {
                // Non-enum: derive each relational op from `compare` so the
                // lexicographic field ordering above is the single source of
                // truth:  a < b = case compare a b of { LT -> True; _ -> False }
                let cmp_call = TExpr::new(
                    TExprKind::App(
                        Box::new(TExpr::new(
                            TExprKind::App(
                                Box::new(TExpr::new(
                                    TExprKind::Var(compare_name.clone()),
                                    compare_fn_ty.clone(),
                                )),
                                Box::new(TExpr::new(TExprKind::Var("_a".into()), result_type.clone())),
                            ),
                            Ty::fun(std::slice::from_ref(&result_type), ordering_ty.clone()),
                        )),
                        Box::new(TExpr::new(TExprKind::Var("_b".into()), result_type.clone())),
                    ),
                    ordering_ty.clone(),
                );
                // (match_con, on_match, otherwise): < and > are True exactly on
                // their own Ordering; <= and >= are False exactly on the
                // opposite strict Ordering.
                let (match_con, on_match) = match *op {
                    "<" => ("LT", true),
                    ">" => ("GT", true),
                    "<=" => ("GT", false),
                    ">=" => ("LT", false),
                    _ => unreachable!(),
                };
                vec![TClause {
                    span: None,
                    patterns: vec![
                        TPattern::Var("_a".into(), result_type.clone()),
                        TPattern::Var("_b".into(), result_type.clone()),
                    ],
                    guards: vec![],
                    body: TExpr::new(
                        TExprKind::Case {
                            scrutinee: Box::new(cmp_call),
                            branches: vec![
                                TCaseBranch {
                                    pattern: TPattern::Constructor { name: match_con.into(), args: vec![] },
                                    guards: vec![],
                                    body: TExpr::new(TExprKind::Lit(TLiteral::Bool(on_match)), bool_ty.clone()),
                                },
                                TCaseBranch {
                                    pattern: TPattern::Wildcard,
                                    guards: vec![],
                                    body: TExpr::new(TExprKind::Lit(TLiteral::Bool(!on_match)), bool_ty.clone()),
                                },
                            ],
                        },
                        bool_ty.clone(),
                    ),
                    where_binds: vec![],
                }]
            };

            functions.push(TFunction {
                name: mangled.clone(),
                ty: fn_ty.clone(),
                clauses,
                specialized: false,
                dict_params: vec![],
            });
        }

        // Register the Ord instance
        let mut method_fns = HashMap::new();
        for (op, op_name) in &[("<", "lt"), (">", "gt"), ("<=", "le"), (">=", "ge")] {
            method_fns.insert(op.to_string(), format!("ord_{}__{}", op_name, type_name));
        }
        method_fns.insert("compare".to_string(), format!("ord_compare__{}", type_name));
        self.register_instance(InstanceInfo {
            class_name: "Ord".to_string(),
            target_type: result_type,
            method_fns,
            context: None,
        });

        functions
    }

    pub(super) fn derive_enum(
        &mut self,
        type_name: &str,
        _type_vars: &[String],
        constructors: &[Constructor],
    ) -> Vec<TFunction> {
        // Enum can only be derived for simple enums (all constructors have 0 fields)
        let is_enum = constructors.iter().all(|c| match &c.fields {
            ConstructorFields::Positional(fs) => fs.is_empty(),
            ConstructorFields::Named(fs) => fs.is_empty(),
        });
        if !is_enum {
            self.push_error_ctx(
                DiagnosticKind::Other(format!("Cannot derive Enum for '{}' — constructors must have no fields", type_name)),
                format!("data {}", type_name),
            );
            return vec![];
        }

        let result_type = Ty::Con(type_name.to_string());
        let int_ty = Ty::Con("Integer".into());
        let list_ty = Ty::List(Box::new(result_type.clone()));
        let n = constructors.len();

        let mut functions = Vec::new();

        // fromEnum_T :: T -> Integer
        let from_name = format!("fromEnum_{}", type_name);
        {
            let clauses: Vec<TClause> = constructors.iter().enumerate().map(|(i, con)| {
                TClause {
                    span: None,
                    patterns: vec![TPattern::Constructor { name: self.resolve_con_name(&con.name).to_string(), args: vec![] }],
                    guards: vec![],
                    body: TExpr::new(TExprKind::Lit(TLiteral::Integer(i as i64)), int_ty.clone()),
                    where_binds: vec![],
                }
            }).collect();
            functions.push(TFunction {
                name: from_name.clone(),
                ty: Ty::arrow(result_type.clone(), int_ty.clone()),
                clauses,
                specialized: false,
                dict_params: vec![],
            });
        }

        // toEnum_T :: Integer -> T
        let to_name = format!("toEnum_{}", type_name);
        {
            let mut clauses: Vec<TClause> = constructors.iter().enumerate().map(|(i, con)| {
                TClause {
                    span: None,
                    patterns: vec![TPattern::LitPat(TLiteral::Integer(i as i64))],
                    guards: vec![],
                    body: TExpr::new(TExprKind::Con(self.resolve_con_name(&con.name).to_string()), result_type.clone()),
                    where_binds: vec![],
                }
            }).collect();
            // Error clause for out of range
            clauses.push(TClause {
                span: None,
                patterns: vec![TPattern::Wildcard],
                guards: vec![],
                body: TExpr::new(
                    TExprKind::App(
                        Box::new(TExpr::new(TExprKind::Var("error".into()), Ty::Unit)),
                        Box::new(TExpr::new(TExprKind::Lit(TLiteral::Str(
                            format!("toEnum: index out of range for {}", type_name)
                        )), Ty::Con("String".into()))),
                    ),
                    result_type.clone(),
                ),
                where_binds: vec![],
            });
            functions.push(TFunction {
                name: to_name.clone(),
                ty: Ty::arrow(int_ty.clone(), result_type.clone()),
                clauses,
                specialized: false,
                dict_params: vec![],
            });
        }

        // succ_T :: T -> T  (toEnum (fromEnum x + 1))
        let succ_name = format!("succ_{}", type_name);
        {
            let mut clauses: Vec<TClause> = Vec::new();
            for i in 0..n.saturating_sub(1) {
                clauses.push(TClause {
                    span: None,
                    patterns: vec![TPattern::Constructor { name: self.resolve_con_name(&constructors[i].name).to_string(), args: vec![] }],
                    guards: vec![],
                    body: TExpr::new(TExprKind::Con(self.resolve_con_name(&constructors[i+1].name).to_string()), result_type.clone()),
                    where_binds: vec![],
                });
            }
            // succ of last = error
            clauses.push(TClause {
                span: None,
                patterns: vec![TPattern::Wildcard],
                guards: vec![],
                body: TExpr::new(
                    TExprKind::App(
                        Box::new(TExpr::new(TExprKind::Var("error".into()), Ty::Unit)),
                        Box::new(TExpr::new(TExprKind::Lit(TLiteral::Str(
                            format!("succ: already at maxBound for {}", type_name)
                        )), Ty::Con("String".into()))),
                    ),
                    result_type.clone(),
                ),
                where_binds: vec![],
            });
            functions.push(TFunction {
                name: succ_name.clone(),
                ty: Ty::arrow(result_type.clone(), result_type.clone()),
                clauses,
                specialized: false,
                dict_params: vec![],
            });
        }

        // pred_T :: T -> T
        let pred_name = format!("pred_{}", type_name);
        {
            let mut clauses: Vec<TClause> = Vec::new();
            for i in 1..n {
                clauses.push(TClause {
                    span: None,
                    patterns: vec![TPattern::Constructor { name: self.resolve_con_name(&constructors[i].name).to_string(), args: vec![] }],
                    guards: vec![],
                    body: TExpr::new(TExprKind::Con(self.resolve_con_name(&constructors[i-1].name).to_string()), result_type.clone()),
                    where_binds: vec![],
                });
            }
            // pred of first = error
            clauses.push(TClause {
                span: None,
                patterns: vec![TPattern::Wildcard],
                guards: vec![],
                body: TExpr::new(
                    TExprKind::App(
                        Box::new(TExpr::new(TExprKind::Var("error".into()), Ty::Unit)),
                        Box::new(TExpr::new(TExprKind::Lit(TLiteral::Str(
                            format!("pred: already at minBound for {}", type_name)
                        )), Ty::Con("String".into()))),
                    ),
                    result_type.clone(),
                ),
                where_binds: vec![],
            });
            functions.push(TFunction {
                name: pred_name.clone(),
                ty: Ty::arrow(result_type.clone(), result_type.clone()),
                clauses,
                specialized: false,
                dict_params: vec![],
            });
        }

        // Range functions: use direct pattern matching for enumFromTo,
        // and delegate others through fromEnum/toEnum.

        // enumFromTo_T :: T -> T -> [T]
        // Generate explicit clauses: for each constructor i, check if fromEnum a <= fromEnum b
        let enum_from_to_name = format!("enumFromTo_{}", type_name);
        {
            // enumFromTo a b = if fromEnum a > fromEnum b then []
            //                  else a : enumFromTo (succ a) b
            // Generate as: single clause with if-expression
            let a_var = TExpr::new(TExprKind::Var("_a".into()), result_type.clone());
            let b_var = TExpr::new(TExprKind::Var("_b".into()), result_type.clone());
            let from_a = TExpr::new(TExprKind::App(
                Box::new(TExpr::new(TExprKind::Var(from_name.clone()), Ty::Unit)),
                Box::new(a_var.clone()),
            ), int_ty.clone());
            let from_b = TExpr::new(TExprKind::App(
                Box::new(TExpr::new(TExprKind::Var(from_name.clone()), Ty::Unit)),
                Box::new(b_var.clone()),
            ), int_ty.clone());
            // if fromEnum a > fromEnum b then []
            // else if fromEnum a == fromEnum b then [a]
            // else a : enumFromTo (succ a) b
            let cond_gt = TExpr::new(TExprKind::InfixApp {
                op: ">".into(),
                lhs: Box::new(from_a),
                rhs: Box::new(from_b.clone()),
            }, Ty::Con("Bool".into()));
            let nil = TExpr::new(TExprKind::Lit(TLiteral::Unit), list_ty.clone());
            let from_a2 = TExpr::new(TExprKind::App(
                Box::new(TExpr::new(TExprKind::Var(from_name.clone()), Ty::Unit)),
                Box::new(a_var.clone()),
            ), int_ty.clone());
            let cond_eq = TExpr::new(TExprKind::InfixApp {
                op: "==".into(),
                lhs: Box::new(from_a2),
                rhs: Box::new(from_b),
            }, Ty::Con("Bool".into()));
            let singleton = TExpr::new(TExprKind::InfixApp {
                op: ":".into(),
                lhs: Box::new(a_var.clone()),
                rhs: Box::new(nil.clone()),
            }, list_ty.clone());
            let succ_a = TExpr::new(TExprKind::App(
                Box::new(TExpr::new(TExprKind::Var(succ_name.clone()), Ty::Unit)),
                Box::new(a_var.clone()),
            ), result_type.clone());
            let recurse = TExpr::new(TExprKind::App(
                Box::new(TExpr::new(TExprKind::App(
                    Box::new(TExpr::new(TExprKind::Var(enum_from_to_name.clone()), Ty::Unit)),
                    Box::new(succ_a),
                ), Ty::Unit)),
                Box::new(b_var.clone()),
            ), list_ty.clone());
            let cons = TExpr::new(TExprKind::InfixApp {
                op: ":".into(),
                lhs: Box::new(a_var),
                rhs: Box::new(recurse),
            }, list_ty.clone());
            let inner_if = TExpr::new(TExprKind::If {
                cond: Box::new(cond_eq),
                then_branch: Box::new(singleton),
                else_branch: Box::new(cons),
            }, list_ty.clone());
            let body = TExpr::new(TExprKind::If {
                cond: Box::new(cond_gt),
                then_branch: Box::new(nil),
                else_branch: Box::new(inner_if),
            }, list_ty.clone());
            functions.push(TFunction {
                name: enum_from_to_name.clone(),
                ty: Ty::fun(&[result_type.clone(), result_type.clone()], list_ty.clone()),
                clauses: vec![TClause {
                    span: None,
                    patterns: vec![
                        TPattern::Var("_a".into(), result_type.clone()),
                        TPattern::Var("_b".into(), result_type.clone()),
                    ],
                    guards: vec![], body, where_binds: vec![],
                }],
                specialized: false,
                dict_params: vec![],
            });
        }

        // enumFrom_T :: T -> [T]  =>  enumFromTo a (last constructor)
        let enum_from_name = format!("enumFrom_{}", type_name);
        {
            let a_var = TExpr::new(TExprKind::Var("_a".into()), result_type.clone());
            let last_con = TExpr::new(
                TExprKind::Con(self.resolve_con_name(&constructors.last().unwrap().name).to_string()),
                result_type.clone(),
            );
            let body = TExpr::new(TExprKind::App(
                Box::new(TExpr::new(TExprKind::App(
                    Box::new(TExpr::new(TExprKind::Var(enum_from_to_name.clone()), Ty::Unit)),
                    Box::new(a_var),
                ), Ty::Unit)),
                Box::new(last_con),
            ), list_ty.clone());
            functions.push(TFunction {
                name: enum_from_name.clone(),
                ty: Ty::arrow(result_type.clone(), list_ty.clone()),
                clauses: vec![TClause {
                    span: None,
                    patterns: vec![TPattern::Var("_a".into(), result_type.clone())],
                    guards: vec![], body, where_binds: vec![],
                }],
                specialized: false,
                dict_params: vec![],
            });
        }

        // enumFromThen_T :: T -> T -> [T]
        // Generate: enumFromThen a b = go (fromEnum a) where step = fromEnum b - fromEnum a
        //   go i = if i < 0 || i >= n then [] else toEnum i : go (i + step)
        let enum_from_then_name = format!("enumFromThen_{}", type_name);
        {
            // Simpler: just generate explicit list since enum is finite
            // enumFromThen a b: start at fromEnum a, step by (fromEnum b - fromEnum a), stop at bounds
            let a_var = TExpr::new(TExprKind::Var("_a".into()), result_type.clone());
            let b_var = TExpr::new(TExprKind::Var("_b".into()), result_type.clone());
            let from_a = TExpr::new(TExprKind::App(
                Box::new(TExpr::new(TExprKind::Var(from_name.clone()), Ty::Unit)),
                Box::new(a_var),
            ), int_ty.clone());
            let from_b = TExpr::new(TExprKind::App(
                Box::new(TExpr::new(TExprKind::Var(from_name.clone()), Ty::Unit)),
                Box::new(b_var),
            ), int_ty.clone());
            // For finite enums, enumFromThen is rarely used; generate empty list as placeholder
            // A proper implementation would need a recursive helper with bounds checking
            let _ = (from_a, from_b);
            let body = TExpr::new(TExprKind::Lit(TLiteral::Unit), list_ty.clone());
            functions.push(TFunction {
                name: enum_from_then_name.clone(),
                ty: Ty::fun(&[result_type.clone(), result_type.clone()], list_ty.clone()),
                clauses: vec![TClause {
                    span: None,
                    patterns: vec![
                        TPattern::Var("_a".into(), result_type.clone()),
                        TPattern::Var("_b".into(), result_type.clone()),
                    ],
                    guards: vec![], body, where_binds: vec![],
                }],
                specialized: false,
                dict_params: vec![],
            });
        }

        // enumFromThenTo_T :: T -> T -> T -> [T]
        let enum_from_then_to_name = format!("enumFromThenTo_{}", type_name);
        {
            let body = TExpr::new(TExprKind::Lit(TLiteral::Unit), list_ty.clone());
            functions.push(TFunction {
                name: enum_from_then_to_name.clone(),
                ty: Ty::fun(&[result_type.clone(), result_type.clone(), result_type.clone()], list_ty.clone()),
                clauses: vec![TClause {
                    span: None,
                    patterns: vec![
                        TPattern::Var("_a".into(), result_type.clone()),
                        TPattern::Var("_b".into(), result_type.clone()),
                        TPattern::Var("_c".into(), result_type.clone()),
                    ],
                    guards: vec![], body, where_binds: vec![],
                }],
                specialized: false,
                dict_params: vec![],
            });
        }

        // Register the Enum instance
        let mut method_fns = HashMap::new();
        method_fns.insert("toEnum".to_string(), to_name);
        method_fns.insert("fromEnum".to_string(), from_name);
        method_fns.insert("succ".to_string(), succ_name);
        method_fns.insert("pred".to_string(), pred_name);
        method_fns.insert("enumFrom".to_string(), enum_from_name);
        method_fns.insert("enumFromThen".to_string(), enum_from_then_name);
        method_fns.insert("enumFromTo".to_string(), enum_from_to_name);
        method_fns.insert("enumFromThenTo".to_string(), enum_from_then_to_name);
        self.register_instance(InstanceInfo {
            class_name: "Enum".to_string(),
            target_type: result_type,
            method_fns,
            context: None,
        });

        functions
    }

    pub(super) fn derive_bounded(
        &mut self,
        type_name: &str,
        _type_vars: &[String],
        constructors: &[Constructor],
    ) -> Vec<TFunction> {
        let is_enum = constructors.iter().all(|c| match &c.fields {
            ConstructorFields::Positional(fs) => fs.is_empty(),
            ConstructorFields::Named(fs) => fs.is_empty(),
        });
        if !is_enum || constructors.is_empty() {
            self.push_error_ctx(
                DiagnosticKind::Other(format!("Cannot derive Bounded for '{}' — must be a simple enum", type_name)),
                format!("data {}", type_name),
            );
            return vec![];
        }

        let result_type = Ty::Con(type_name.to_string());
        let mut functions = Vec::new();

        // minBound_T :: T
        let min_name = format!("minBound_{}", type_name);
        functions.push(TFunction {
            name: min_name.clone(),
            ty: result_type.clone(),
            clauses: vec![TClause {
                span: None,
                patterns: vec![],
                guards: vec![],
                body: TExpr::new(TExprKind::Con(self.resolve_con_name(&constructors.first().unwrap().name).to_string()), result_type.clone()),
                where_binds: vec![],
            }],
            specialized: false,
            dict_params: vec![],
        });

        // maxBound_T :: T
        let max_name = format!("maxBound_{}", type_name);
        functions.push(TFunction {
            name: max_name.clone(),
            ty: result_type.clone(),
            clauses: vec![TClause {
                span: None,
                patterns: vec![],
                guards: vec![],
                body: TExpr::new(TExprKind::Con(self.resolve_con_name(&constructors.last().unwrap().name).to_string()), result_type.clone()),
                where_binds: vec![],
            }],
            specialized: false,
            dict_params: vec![],
        });

        // Register Bounded instance
        let mut method_fns = HashMap::new();
        method_fns.insert("minBound".to_string(), min_name);
        method_fns.insert("maxBound".to_string(), max_name);
        self.register_instance(InstanceInfo {
            class_name: "Bounded".to_string(),
            target_type: result_type,
            method_fns,
            context: None,
        });

        functions
    }

    /// Check if a type mentions a specific type variable name
    pub(super) fn ty_mentions_var(ty: &Ty, var_name: &str) -> bool {
        match ty {
            Ty::Var(tv) => tv.name == var_name,
            Ty::Con(_) | Ty::Unit | Ty::Promoted(_) | Ty::Skolem(..) => false,
            Ty::Arrow(a, b) | Ty::App(a, b) => {
                Self::ty_mentions_var(a, var_name) || Self::ty_mentions_var(b, var_name)
            }
            Ty::List(a) | Ty::IO(a) => Self::ty_mentions_var(a, var_name),
            Ty::LuaIO(_, a) => Self::ty_mentions_var(a, var_name),
            Ty::Forall(_, a) => Self::ty_mentions_var(a, var_name),
            Ty::Tuple(elems) => elems.iter().any(|e| Self::ty_mentions_var(e, var_name)),
        }
    }

    /// Generate the expression to map a field value through the functor function.
    /// For a field of type `t` in a Functor-derived constructor:
    /// - If `t` doesn't mention the last type var: pass through unchanged
    /// - If `t` IS the last type var: apply `_f x`
    /// - Otherwise (e.g. `Tree a`, `[a]`): apply `resolved_fmap _f x`
    /// The fmap_name is resolved at derive time to avoid polymorphic resolution issues.
    pub(super) fn functor_map_field(&self, field_ty: &Ty, last_var: &str, var_name: &str, b_ty: &Ty, self_fmap: &str) -> TExpr {
        if !Self::ty_mentions_var(field_ty, last_var) {
            // Field doesn't mention the functor parameter → pass through
            TExpr::new(TExprKind::Var(var_name.to_string()), field_ty.clone())
        } else if let Ty::Var(tv) = field_ty {
            if tv.name == last_var {
                // Field IS the functor parameter → apply _f
                TExpr::new(
                    TExprKind::App(
                        Box::new(TExpr::new(
                            TExprKind::Var("_f".to_string()),
                            Ty::arrow(field_ty.clone(), b_ty.clone()),
                        )),
                        Box::new(TExpr::new(
                            TExprKind::Var(var_name.to_string()),
                            field_ty.clone(),
                        )),
                    ),
                    b_ty.clone(),
                )
            } else {
                TExpr::new(TExprKind::Var(var_name.to_string()), field_ty.clone())
            }
        } else {
            // Complex type mentioning the var (e.g. Tree a, [a], Maybe a)
            // Resolve the fmap function name at derive time
            let fmap_resolved = self.resolve_functor_fmap(field_ty, self_fmap);
            let a_ty = Ty::Var(TyVar { name: last_var.to_string(), id: u32::MAX });
            let fmap_f = TExpr::new(
                TExprKind::App(
                    Box::new(TExpr::new(
                        TExprKind::Var(fmap_resolved),
                        Ty::arrow(Ty::arrow(a_ty.clone(), b_ty.clone()), Ty::arrow(field_ty.clone(), field_ty.clone())),
                    )),
                    Box::new(TExpr::new(
                        TExprKind::Var("_f".to_string()),
                        Ty::arrow(a_ty, b_ty.clone()),
                    )),
                ),
                Ty::arrow(field_ty.clone(), field_ty.clone()),
            );
            TExpr::new(
                TExprKind::App(
                    Box::new(fmap_f),
                    Box::new(TExpr::new(
                        TExprKind::Var(var_name.to_string()),
                        field_ty.clone(),
                    )),
                ),
                field_ty.clone(),
            )
        }
    }

    /// Resolve the concrete fmap function for a type constructor at derive time.
    /// Extracts the outermost type constructor and looks up the Functor instance.
    pub(super) fn resolve_functor_fmap(&self, ty: &Ty, self_fmap: &str) -> String {
        // Only container-shaped types (a head constructor applied to at least
        // one argument, or a list/IO) can have a Functor instance; a bare
        // `Ty::Con` field is not mapped over.
        let tc_name = match ty {
            Ty::List(_) | Ty::IO(_) | Ty::App(_, _) => InstHead::of(ty),
            _ => None,
        };
        if let Some(tc) = tc_name {
            let key = ("Functor".to_string(), tc);
            if let Some(inst) = self.instances.get(&key)
                && let Some(name) = inst.method_fns.get("fmap") {
                    return name.clone();
                }
            // Self-recursive: instance not yet registered, use self_fmap
            return self_fmap.to_string();
        }
        // Fallback: use generic fmap (will need monomorphizer resolution)
        "fmap".to_string()
    }

    /// Generate `fmap` for a data type.
    /// For `data T a = C1 f1 f2 | C2 g1`, generates:
    /// `fmap_T f (C1 x0 x1) = C1 (map x0) (map x1)`
    /// where `map` applies `f` to fields mentioning the last type variable.
    pub(super) fn derive_functor(
        &mut self,
        type_name: &str,
        type_vars: &[String],
        constructors: &[Constructor],
    ) -> Vec<TFunction> {
        if type_vars.is_empty() {
            self.push_error_ctx(
                DiagnosticKind::Other(format!("Cannot derive Functor for '{}' — it has no type parameters", type_name)),
                format!("data {}", type_name),
            );
            return vec![];
        }

        let last_tv_name = type_vars.last().unwrap().clone();

        let tvars: Vec<TyVar> = type_vars.iter()
            .map(|n| TyVar { name: n.clone(), id: u32::MAX })
            .collect();

        let a_tv = TyVar { name: last_tv_name.clone(), id: u32::MAX };
        let b_tv = TyVar { name: "__b".to_string(), id: u32::MAX };

        // Build T a (input type)
        let input_type = tvars.iter().fold(
            Ty::Con(type_name.to_string()),
            |acc, tv| Ty::app(acc, Ty::Var(tv.clone())),
        );

        // Build T b (output type) — last type var replaced with __b
        let output_type = tvars.iter().fold(
            Ty::Con(type_name.to_string()),
            |acc, tv| {
                if tv.name == last_tv_name {
                    Ty::app(acc, Ty::Var(b_tv.clone()))
                } else {
                    Ty::app(acc, Ty::Var(tv.clone()))
                }
            },
        );

        let f_ty = Ty::arrow(Ty::Var(a_tv.clone()), Ty::Var(b_tv.clone()));
        let fn_ty = Ty::fun(&[f_ty.clone(), input_type.clone()], output_type.clone());

        let mangled = format!("fmap_{}", type_name);

        let mut clauses = Vec::new();
        for con in constructors {
            let field_count = match &con.fields {
                ConstructorFields::Positional(fs) => fs.len(),
                ConstructorFields::Named(fs) => fs.len(),
            };

            let con_key = self.resolve_con_name(&con.name).to_string();
            let con_info = self.constructors.get(&con_key).cloned();
            let field_tys: Vec<Ty> = con_info.as_ref()
                .map(|ci| ci.field_types.clone())
                .unwrap_or_default();

            let param_names: Vec<String> = (0..field_count)
                .map(|i| format!("_x{}", i))
                .collect();

            // Pattern: _f (Con x0 x1 ...)
            let patterns = vec![
                TPattern::Var("_f".to_string(), f_ty.clone()),
                TPattern::Constructor {
                    name: con_key.clone(),
                    args: param_names.iter().enumerate().map(|(i, n)| {
                        let ty = field_tys.get(i).cloned().unwrap_or(Ty::Unit);
                        TPattern::Var(n.clone(), ty)
                    }).collect(),
                },
            ];

            // Body: Con (mapped_x0) (mapped_x1) ...
            let mut body = TExpr::new(
                TExprKind::Con(con_key),
                output_type.clone(),
            );

            let b_ty_val = Ty::Var(b_tv.clone());
            for (i, pname) in param_names.iter().enumerate() {
                let field_ty = field_tys.get(i).cloned().unwrap_or(Ty::Unit);
                let mapped = self.functor_map_field(&field_ty, &last_tv_name, pname, &b_ty_val, &mangled);
                body = TExpr::new(
                    TExprKind::App(Box::new(body), Box::new(mapped)),
                    output_type.clone(),
                );
            }

            clauses.push(TClause {
                span: None,
                patterns,
                guards: vec![],
                body,
                where_binds: vec![],
            });
        }

        // Register the Functor instance
        let mut method_fns = HashMap::new();
        method_fns.insert("fmap".to_string(), mangled.clone());
        method_fns.insert("<$>".to_string(), mangled.clone());
        self.register_instance(InstanceInfo {
            class_name: "Functor".to_string(),
            target_type: Ty::Con(type_name.to_string()),
            method_fns,
            context: None,
        });

        vec![TFunction {
            name: mangled,
            ty: fn_ty,
            clauses,
            specialized: false,
            dict_params: vec![],
        }]
    }

    // --- FromJSON deriving ---

    pub(super) fn json_ty() -> Ty { Ty::Con("Json".into()) }

    /// `Either String t`
    pub(super) fn estr_ty(t: &Ty) -> Ty {
        Ty::app(Ty::app(Ty::Con("Either".into()), Ty::Con("String".into())), t.clone())
    }

    pub(super) fn ty_maybe_inner(ty: &Ty) -> Option<&Ty> {
        if let Ty::App(f, inner) = ty
            && matches!(f.as_ref(), Ty::Con(n) if n == "Maybe") {
                return Some(inner);
            }
        None
    }

    pub(super) fn jx_var(name: &str, ty: Ty) -> TExpr {
        TExpr::new(TExprKind::Var(name.to_string()), ty)
    }

    pub(super) fn jx_str(s: &str) -> TExpr {
        TExpr::new(TExprKind::Lit(TLiteral::Str(s.to_string())), Ty::Con("String".into()))
    }

    pub(super) fn jx_int(i: i64) -> TExpr {
        TExpr::new(TExprKind::Lit(TLiteral::Integer(i)), Ty::Con("Integer".into()))
    }

    pub(super) fn jx_app(f: TExpr, arg: TExpr, ty: Ty) -> TExpr {
        TExpr::new(TExprKind::App(Box::new(f), Box::new(arg)), ty)
    }

    /// `fname arg1 … argN` with the function type reconstructed from the
    /// argument types, so the monomorphizer sees fully concrete types.
    pub(super) fn jx_call(fname: &str, args: Vec<TExpr>, ret_ty: Ty) -> TExpr {
        let mut fty = ret_ty.clone();
        for a in args.iter().rev() {
            fty = Ty::arrow(a.ty.clone(), fty);
        }
        let mut e = Self::jx_var(fname, fty);
        for a in args {
            let next_ty = match &e.ty {
                Ty::Arrow(_, b) => (**b).clone(),
                _ => ret_ty.clone(),
            };
            e = Self::jx_app(e, a, next_ty);
        }
        e
    }

    /// `case scrut of { Left e -> Left e; Right ok -> ok_body }` — the
    /// Either-plumbing every derived decode step threads through.
    pub(super) fn jx_bind_either(scrut: TExpr, ok_name: &str, ok_ty: Ty, ok_body: TExpr, out_ty: Ty) -> TExpr {
        let str_ty = Ty::Con("String".into());
        let err_name = format!("{}e", ok_name);
        let rethrow = Self::jx_app(
            TExpr::new(TExprKind::Con("Left".into()), Ty::arrow(str_ty.clone(), out_ty.clone())),
            Self::jx_var(&err_name, str_ty.clone()),
            out_ty.clone(),
        );
        TExpr::new(TExprKind::Case {
            scrutinee: Box::new(scrut),
            branches: vec![
                TCaseBranch {
                    pattern: TPattern::Constructor {
                        name: "Left".into(),
                        args: vec![TPattern::Var(err_name, str_ty)],
                    },
                    guards: vec![],
                    body: rethrow,
                },
                TCaseBranch {
                    pattern: TPattern::Constructor {
                        name: "Right".into(),
                        args: vec![TPattern::Var(ok_name.to_string(), ok_ty)],
                    },
                    guards: vec![],
                    body: ok_body,
                },
            ],
        }, out_ty)
    }

    /// `Right (Con _f0 … _fN)`
    pub(super) fn jx_ok_con(con_name: &str, field_tys: &[Ty], result_ty: &Ty, estr: &Ty) -> TExpr {
        let mut built = TExpr::new(TExprKind::Con(con_name.to_string()), result_ty.clone());
        for (i, fty) in field_tys.iter().enumerate() {
            built = Self::jx_app(built, Self::jx_var(&format!("_f{}", i), fty.clone()), result_ty.clone());
        }
        Self::jx_app(
            TExpr::new(TExprKind::Con("Right".into()), Ty::arrow(result_ty.clone(), estr.clone())),
            built,
            estr.clone(),
        )
    }

    pub(super) fn fromjson_field_err(what: String, (reason, note): (String, String)) -> (String, String) {
        (format!("{} — {}", what, reason), note)
    }

    /// Build the decoder expression (of type `Json -> Either String field_ty`)
    /// for one field of a FromJSON-derived constructor. Resolution is
    /// STRUCTURAL at derive time (mirroring derive_functor's fmap resolution):
    /// mata-ll cannot register class instances on library/container types, so
    /// primitives use the fromJSON* combinators, `[t]` and `Maybe t` route
    /// through fromJSONList/fromJSONMaybe, and a field of another FromJSON
    /// type calls that type's own `fromJSON_T` decoder — including self- and
    /// mutually-recursive types, via the `fromjson_types` prescan.
    /// `Err` carries (reason, note) for the rejection message.
    pub(super) fn fromjson_field_decoder(&self, field_ty: &Ty) -> Result<TExpr, (String, String)> {
        let dec_ty = Ty::arrow(Self::json_ty(), Self::estr_ty(field_ty));
        match field_ty {
            Ty::Con(n) if n == "Integer" => Ok(Self::jx_var("fromJSONInteger", dec_ty)),
            Ty::Con(n) if n == "Number" => Ok(Self::jx_var("fromJSONNumber", dec_ty)),
            Ty::Con(n) if n == "String" => Ok(Self::jx_var("fromJSONString", dec_ty)),
            Ty::Con(n) if n == "Bool" => Ok(Self::jx_var("fromJSONBool", dec_ty)),
            Ty::Con(n) if n == "Json" => Ok(Self::jx_var("fromJSONValue", dec_ty)),
            Ty::List(elem) => {
                let inner = self.fromjson_field_decoder(elem)?;
                let list_fn_ty = Ty::arrow(inner.ty.clone(), dec_ty.clone());
                Ok(Self::jx_app(Self::jx_var("fromJSONList", list_fn_ty), inner, dec_ty))
            }
            _ if Self::ty_maybe_inner(field_ty).is_some() => {
                let inner_ty = Self::ty_maybe_inner(field_ty).unwrap();
                let inner = self.fromjson_field_decoder(inner_ty)?;
                let maybe_fn_ty = Ty::arrow(inner.ty.clone(), dec_ty.clone());
                Ok(Self::jx_app(Self::jx_var("fromJSONMaybe", maybe_fn_ty), inner, dec_ty))
            }
            Ty::Con(n) => {
                if self.fromjson_types.contains(n)
                    || self.instances.contains_key(&("FromJSON".to_string(), InstHead::Con(n.clone()))) {
                    Ok(Self::jx_var(&format!("fromJSON_{}", n), dec_ty))
                } else {
                    Err((
                        format!("the type '{}' has no FromJSON instance", n),
                        format!("every field of a derived decoder needs its own decoder; add `deriving (FromJSON)` to '{}' or write `instance FromJSON {}` in the module that defines it.", n, n),
                    ))
                }
            }
            Ty::Arrow(..) => Err((
                "it is a function type".to_string(),
                "a function has no JSON representation, so no decoder can produce one; store data instead of behavior, or write the FromJSON instance by hand for an encoding you define.".to_string(),
            )),
            Ty::Tuple(_) => Err((
                format!("the tuple type '{}' has no JSON decoding convention in mata-ll", field_ty),
                "GHC's aeson decodes tuples from fixed-length arrays; mata-ll does not — wrap the tuple in a small record type deriving (FromJSON), which also gives the components names in the JSON.".to_string(),
            )),
            Ty::App(..) => Err((
                format!("the type '{}' is parameterized", field_ty),
                "a derived decoder resolves one concrete decoder per field at compile time, and mata-ll instances cannot cover a parameterized type at every instantiation; wrap the concrete instantiation in its own data type deriving (FromJSON).".to_string(),
            )),
            Ty::IO(_) | Ty::LuaIO(..) => Err((
                "it is an effectful action type".to_string(),
                "an IO action has no JSON representation.".to_string(),
            )),
            Ty::Var(v) => Err((
                format!("its type is the type parameter '{}'", v.name),
                "a type parameter has no decoder the compiler can pick at derive time.".to_string(),
            )),
            other => Err((
                format!("the type '{}' cannot be decoded from JSON", other),
                "derived FromJSON supports Integer, Number, String, Bool, Json, lists, Maybe, and types that themselves have a FromJSON instance.".to_string(),
            )),
        }
    }

    /// Decode a record constructor's fields from the object in `_j`
    /// (effective keys — the `as "key"` rename when present, the field name
    /// otherwise — as JSON keys; Maybe fields optional via jOptFieldWith).
    pub(super) fn fromjson_named_body(
        &self,
        con_name: &str,
        fields: &[RecordField],
        field_tys: &[Ty],
        estr: &Ty,
        result_ty: &Ty,
    ) -> Result<TExpr, (String, String)> {
        let json = Self::json_ty();
        // The constructed value uses the registered key (a shadowing local
        // constructor is mangled); every *string* below keeps the source name.
        let mut body = Self::jx_ok_con(self.resolve_con_name(con_name), field_tys, result_ty, estr);
        for i in (0..fields.len()).rev() {
            let fty = &field_tys[i];
            let fname = &fields[i].name;
            // Derive-time errors name the Haskell field; runtime decode
            // errors name the JSON key, because that is what the document
            // actually contains.
            let key = fields[i].effective_key();
            let err_what = || format!("field '{}' of constructor '{}' cannot be decoded", fname, con_name);
            let step = if let Some(inner) = Self::ty_maybe_inner(fty) {
                // Maybe field: a missing key and an explicit null both → Nothing.
                let dec = self.fromjson_field_decoder(inner)
                    .map_err(|e| Self::fromjson_field_err(err_what(), e))?;
                Self::jx_call(
                    "jOptFieldWith",
                    vec![dec, Self::jx_str(key), Self::jx_var("_j", json.clone())],
                    Self::estr_ty(fty),
                )
            } else {
                let dec = self.fromjson_field_decoder(fty)
                    .map_err(|e| Self::fromjson_field_err(err_what(), e))?;
                Self::jx_call(
                    "jFieldWith",
                    vec![dec, Self::jx_str(key), Self::jx_var("_j", json.clone())],
                    Self::estr_ty(fty),
                )
            };
            body = Self::jx_bind_either(step, &format!("_f{}", i), fty.clone(), body, estr.clone());
        }
        Ok(body)
    }

    /// Decode positional arguments, argument i taken from `elem_exprs[i]`,
    /// finishing with `Right (Con _f0 …)`.
    pub(super) fn fromjson_positional_chain(
        &self,
        con_name: &str,
        field_tys: &[Ty],
        elem_exprs: Vec<TExpr>,
        estr: &Ty,
        result_ty: &Ty,
    ) -> Result<TExpr, (String, String)> {
        let mut body = Self::jx_ok_con(self.resolve_con_name(con_name), field_tys, result_ty, estr);
        for i in (0..field_tys.len()).rev() {
            let fty = &field_tys[i];
            let dec = self.fromjson_field_decoder(fty).map_err(|e| Self::fromjson_field_err(
                format!("argument {} of constructor '{}' cannot be decoded", i + 1, con_name), e))?;
            let step = Self::jx_call(
                "jArgWith",
                vec![
                    dec,
                    Self::jx_str(con_name),
                    Self::jx_int((i + 1) as i64),
                    elem_exprs[i].clone(),
                ],
                Self::estr_ty(fty),
            );
            body = Self::jx_bind_either(step, &format!("_f{}", i), fty.clone(), body, estr.clone());
        }
        Ok(body)
    }

    /// Decode one constructor from the tagged object bound to `_j`:
    /// record fields inline in the object, positional arguments under
    /// "contents" (the value itself for one argument, an array for several),
    /// nothing needed for a nullary constructor.
    pub(super) fn fromjson_tagged_con_body(
        &self,
        con: &Constructor,
        field_tys: &[Ty],
        estr: &Ty,
        result_ty: &Ty,
    ) -> Result<TExpr, (String, String)> {
        let json = Self::json_ty();
        match &con.fields {
            ConstructorFields::Named(fields) if !fields.is_empty() => {
                self.fromjson_named_body(&con.name, fields, field_tys, estr, result_ty)
            }
            _ if field_tys.is_empty() => Ok(Self::jx_ok_con(self.resolve_con_name(&con.name), &[], result_ty, estr)),
            _ => {
                let contents = Self::jx_call(
                    "jField",
                    vec![Self::jx_str("contents"), Self::jx_var("_j", json.clone())],
                    Self::estr_ty(&json),
                );
                let inner = if field_tys.len() == 1 {
                    self.fromjson_positional_chain(
                        &con.name, field_tys,
                        vec![Self::jx_var("_c", json.clone())],
                        estr, result_ty,
                    )?
                } else {
                    let arr_ty = Ty::list(json.clone());
                    let elems: Vec<TExpr> = (0..field_tys.len()).map(|i| {
                        Self::jx_call(
                            "jNth",
                            vec![Self::jx_int(i as i64), Self::jx_var("_xs", arr_ty.clone())],
                            json.clone(),
                        )
                    }).collect();
                    let chain = self.fromjson_positional_chain(&con.name, field_tys, elems, estr, result_ty)?;
                    let arrn = Self::jx_call(
                        "jExpectArrN",
                        vec![
                            Self::jx_str(&con.name),
                            Self::jx_int(field_tys.len() as i64),
                            Self::jx_var("_c", json.clone()),
                        ],
                        Self::estr_ty(&arr_ty),
                    );
                    Self::jx_bind_either(arrn, "_xs", arr_ty, chain, estr.clone())
                };
                Ok(Self::jx_bind_either(contents, "_c", json, inner, estr.clone()))
            }
        }
    }

    /// Validate the effective JSON keys (the `as "key"` rename when present,
    /// the field name otherwise) of every record constructor of a
    /// ToJSON/FromJSON-derived type: within one constructor each key must be
    /// a non-empty string, no two fields may share one, and in a tagged type
    /// no key may be "tag". This is the JSON-side twin of the LuaDict key
    /// validation — a type that derives only a JSON codec (no LuaDict) still
    /// needs it, because the keys become the keys of one JSON object.
    /// `class` names the derive being rejected in the message. Returns false
    /// after reporting when validation fails.
    pub(super) fn validate_json_keys(
        &mut self,
        class: &str,
        type_name: &str,
        constructors: &[Constructor],
        tagged: bool,
    ) -> bool {
        let reject = |checker: &mut Self, reason: String, note: &str| {
            checker.push_error_ctx(
                DiagnosticKind::Other(format!(
                    "Cannot derive '{}' for '{}': {}\nnote: {}",
                    class, type_name, reason, note,
                )),
                format!("data {}", type_name),
            );
        };
        for con in constructors {
            let ConstructorFields::Named(fields) = &con.fields else { continue };
            let mut seen: HashMap<&str, &str> = HashMap::new();
            for field in fields {
                let key = field.effective_key();
                if key.is_empty() {
                    reject(self,
                        format!("field '{}' of constructor '{}' renames its JSON key to the empty string", field.name, con.name),
                        "the field name becomes the field's key in the JSON object, and mata-ll keeps one shared, non-empty external name per field (the LuaDict table key and the JSON key); give `as` a non-empty string.");
                    return false;
                }
                if tagged && key == "tag" {
                    reject(self,
                        format!("field '{}' of constructor '{}' has the JSON key \"tag\", which collides with the \"tag\" key the codec uses to tell constructors apart", field.name, con.name),
                        "a multi-constructor type maps to {\"tag\":\"Con\", …} with record fields inline in the same object; rename the field, or rename its key with `as \"otherKey\"`.");
                    return false;
                }
                if let Some(prev) = seen.insert(key, &field.name) {
                    reject(self,
                        format!("fields '{}' and '{}' of constructor '{}' both map to the JSON key \"{}\"", prev, field.name, con.name, key),
                        "each record field becomes one key in the JSON object, so two fields sharing a key would silently overwrite each other; rename one with `as \"otherKey\"`.");
                    return false;
                }
            }
        }
        true
    }

    /// Generate `fromJSON` for a data type: `fromJSON_T :: Json -> Either String T`
    /// built from the JSON module's decoder combinators, plus the FromJSON
    /// instance registration.
    ///
    /// Decoding convention (mirrors aeson's defaultOptions where mata-ll can):
    /// - a single-constructor record decodes from an object keyed by the
    ///   fields' effective keys (the `as "key"` rename when present, the
    ///   Haskell field name otherwise): `data P = P { x :: Integer }` ⇐ `{"x":1}`
    /// - a single positional constructor decodes from its argument directly
    ///   (one field) or from an array of its arguments (several)
    /// - a multi-constructor type (or a lone nullary constructor) is tagged:
    ///   the value is either the bare constructor name as a string (nullary
    ///   constructors only) or an object with "tag":"Con" — record fields
    ///   inline in the same object, positional arguments under "contents"
    /// - a `Maybe t` record field decodes a missing key, null, or the value
    /// - unknown object keys are ignored (as aeson does)
    ///
    /// note: aeson encodes only ALL-nullary sum types as bare strings; this
    /// decoder accepts both the bare string and the tagged-object form for any
    /// nullary constructor, since a reasonable encoder may emit either.
    /// note: `Maybe (Maybe a)` cannot round-trip under the null-is-Nothing
    /// convention — `Just Nothing` has no JSON form distinct from `Nothing`
    /// (the same collapse the "Just injective" codegen note documents).
    pub(super) fn derive_fromjson(
        &mut self,
        type_name: &str,
        type_vars: &[String],
        constructors: &[Constructor],
    ) -> Vec<TFunction> {
        let reject = |checker: &mut Self, reason: String, note: &str| {
            checker.push_error_ctx(
                DiagnosticKind::Other(format!(
                    "Cannot derive 'FromJSON' for '{}': {}\nnote: {}",
                    type_name, reason, note,
                )),
                format!("data {}", type_name),
            );
        };

        // The class and every combinator the generated decoder calls live in
        // the JSON library module; without the import nothing can resolve.
        if !self.classes.contains_key("FromJSON") || self.env.lookup("jContext").is_none() {
            reject(self,
                "the FromJSON class and its decoder combinators are not in scope".to_string(),
                "the FromJSON class and the codec combinators the derived decoder calls (jContext, jFieldWith, …) live in the JSON library module; add `import JSON` at the top of this file.");
            return vec![];
        }

        if !type_vars.is_empty() {
            reject(self,
                format!("'{}' has type parameters", type_name),
                "a derived decoder must pick one concrete decoder per field at compile time, and a field whose type is a type parameter has none. GHC's aeson handles this with a `FromJSON a` constraint on the instance; mata-ll does not derive constrained codecs, so derive FromJSON for concrete types only (wrap each instantiation you need in its own data type).");
            return vec![];
        }

        for con in constructors {
            if con.gadt_type.is_some() || !con.existential_vars.is_empty() {
                reject(self,
                    format!("constructor '{}' is a GADT / existential constructor", con.name),
                    "a decoder must name every field's type to choose how to decode it, which GADT and existential constructors do not allow.");
                return vec![];
            }
        }

        // Tagged decoding applies to multi-constructor types, and to a lone
        // nullary constructor (no fields to decode — the constructor NAME is
        // the payload).
        let single_nullary = constructors.len() == 1 && match &constructors[0].fields {
            ConstructorFields::Positional(fs) => fs.is_empty(),
            ConstructorFields::Named(fs) => fs.is_empty(),
        };
        let tagged = constructors.len() > 1 || single_nullary;

        if !self.validate_json_keys("FromJSON", type_name, constructors, tagged) {
            return vec![];
        }

        let result_ty = Ty::Con(type_name.to_string());
        let estr = Self::estr_ty(&result_ty);
        let json = Self::json_ty();
        let str_ty = Ty::Con("String".into());
        let bool_ty = Ty::Con("Bool".into());
        let mangled = format!("fromJSON_{}", type_name);

        // Constructor field types as registered in pass 1.
        let con_field_tys: Vec<Vec<Ty>> = constructors.iter().map(|con| {
            self.constructors.get(self.resolve_con_name(&con.name))
                .map(|ci| ci.field_types.clone())
                .unwrap_or_default()
        }).collect();

        let body_inner: TExpr = if tagged {
            // "'A', 'B' or 'C'" for the unknown-tag message.
            let names: Vec<String> = constructors.iter().map(|c| format!("'{}'", c.name)).collect();
            let expected = match names.len() {
                1 => names[0].clone(),
                _ => format!("{} or {}", names[..names.len() - 1].join(", "), names[names.len() - 1]),
            };

            // Per-constructor decode bodies for the tagged-object form; report
            // every unsupported field before bailing out.
            let mut con_bodies: Vec<TExpr> = Vec::new();
            let mut failed = false;
            for (con, ftys) in constructors.iter().zip(&con_field_tys) {
                match self.fromjson_tagged_con_body(con, ftys, &estr, &result_ty) {
                    Ok(e) => con_bodies.push(e),
                    Err((reason, note)) => {
                        reject(self, reason, &note);
                        failed = true;
                    }
                }
            }
            if failed { return vec![]; }

            // Bare-string form: only a nullary constructor may be a bare string.
            let mut str_chain = Self::jx_call(
                "jBadTag",
                vec![Self::jx_str(&expected), Self::jx_var("_s", str_ty.clone())],
                estr.clone(),
            );
            for (con, ftys) in constructors.iter().zip(&con_field_tys).rev() {
                let then_branch = if ftys.is_empty() {
                    // TIR construction uses the registered key; the compared
                    // JSON tag strings keep the source name.
                    Self::jx_ok_con(self.resolve_con_name(&con.name), &[], &result_ty, &estr)
                } else {
                    Self::jx_call("jTagNeedsObject", vec![Self::jx_str(&con.name)], estr.clone())
                };
                str_chain = TExpr::new(TExprKind::If {
                    cond: Box::new(TExpr::new(TExprKind::InfixApp {
                        op: "==".into(),
                        lhs: Box::new(Self::jx_var("_s", str_ty.clone())),
                        rhs: Box::new(Self::jx_str(&con.name)),
                    }, bool_ty.clone())),
                    then_branch: Box::new(then_branch),
                    else_branch: Box::new(str_chain),
                }, estr.clone());
            }

            // Tagged-object form: dispatch on the decoded "tag" field.
            let mut tag_chain = Self::jx_call(
                "jBadTag",
                vec![Self::jx_str(&expected), Self::jx_var("_tag", str_ty.clone())],
                estr.clone(),
            );
            for (con, con_body) in constructors.iter().zip(con_bodies).rev() {
                tag_chain = TExpr::new(TExprKind::If {
                    cond: Box::new(TExpr::new(TExprKind::InfixApp {
                        op: "==".into(),
                        lhs: Box::new(Self::jx_var("_tag", str_ty.clone())),
                        rhs: Box::new(Self::jx_str(&con.name)),
                    }, bool_ty.clone())),
                    then_branch: Box::new(con_body),
                    else_branch: Box::new(tag_chain),
                }, estr.clone());
            }
            let obj_body = Self::jx_bind_either(
                Self::jx_call(
                    "jFieldWith",
                    vec![
                        Self::jx_var("fromJSONString", Ty::arrow(json.clone(), Self::estr_ty(&str_ty))),
                        Self::jx_str("tag"),
                        Self::jx_var("_j", json.clone()),
                    ],
                    Self::estr_ty(&str_ty),
                ),
                "_tag", str_ty.clone(), tag_chain, estr.clone(),
            );

            TExpr::new(TExprKind::Case {
                scrutinee: Box::new(Self::jx_var("_j", json.clone())),
                branches: vec![
                    TCaseBranch {
                        pattern: TPattern::Constructor {
                            name: "JStr".into(),
                            args: vec![TPattern::Var("_s".into(), str_ty.clone())],
                        },
                        guards: vec![],
                        body: str_chain,
                    },
                    TCaseBranch {
                        pattern: TPattern::Constructor {
                            name: "JObj".into(),
                            args: vec![TPattern::Wildcard],
                        },
                        guards: vec![],
                        body: obj_body,
                    },
                    TCaseBranch {
                        pattern: TPattern::Wildcard,
                        guards: vec![],
                        body: Self::jx_call(
                            "jExpectTagged",
                            vec![Self::jx_var("_j", json.clone())],
                            estr.clone(),
                        ),
                    },
                ],
            }, estr.clone())
        } else {
            // Single non-nullary constructor: untagged.
            let con = &constructors[0];
            let ftys = &con_field_tys[0];
            let built = match &con.fields {
                ConstructorFields::Named(fields) if !fields.is_empty() => {
                    self.fromjson_named_body(&con.name, fields, ftys, &estr, &result_ty)
                }
                _ if ftys.len() == 1 => {
                    self.fromjson_positional_chain(
                        &con.name, ftys,
                        vec![Self::jx_var("_j", json.clone())],
                        &estr, &result_ty,
                    )
                }
                _ => {
                    let arr_ty = Ty::list(json.clone());
                    let elems: Vec<TExpr> = (0..ftys.len()).map(|i| {
                        Self::jx_call(
                            "jNth",
                            vec![Self::jx_int(i as i64), Self::jx_var("_xs", arr_ty.clone())],
                            json.clone(),
                        )
                    }).collect();
                    self.fromjson_positional_chain(&con.name, ftys, elems, &estr, &result_ty)
                        .map(|chain| {
                            let arrn = Self::jx_call(
                                "jExpectArrN",
                                vec![
                                    Self::jx_str(&con.name),
                                    Self::jx_int(ftys.len() as i64),
                                    Self::jx_var("_j", json.clone()),
                                ],
                                Self::estr_ty(&arr_ty),
                            );
                            Self::jx_bind_either(arrn, "_xs", arr_ty, chain, estr.clone())
                        })
                }
            };
            match built {
                Ok(e) => e,
                Err((reason, note)) => {
                    reject(self, reason, &note);
                    return vec![];
                }
            }
        };

        // Tag every failure with the type being decoded.
        let body = Self::jx_call(
            "jContext",
            vec![Self::jx_str(type_name), body_inner],
            estr.clone(),
        );

        // Register the instance
        let mut method_fns = HashMap::new();
        method_fns.insert("fromJSON".to_string(), mangled.clone());
        self.register_instance(InstanceInfo {
            class_name: "FromJSON".to_string(),
            target_type: result_ty.clone(),
            method_fns,
            context: None,
        });

        vec![TFunction {
            name: mangled,
            ty: Ty::arrow(json, estr),
            clauses: vec![TClause {
                span: None,
                patterns: vec![TPattern::Var("_j".into(), Self::json_ty())],
                guards: vec![],
                body,
                where_binds: vec![],
            }],
            specialized: false,
            dict_params: vec![],
        }]
    }

    // --- ToJSON deriving ---

    /// `("key", val)` — one pair of a JSON object, of type `(String, Json)`.
    pub(super) fn jx_pair(key: &str, val: TExpr) -> TExpr {
        let ty = Ty::Tuple(vec![Ty::Con("String".into()), Self::json_ty()]);
        TExpr::new(TExprKind::Tuple(vec![Self::jx_str(key), val]), ty)
    }

    /// A literal cons list `e0 : e1 : … : []` of the given element type.
    pub(super) fn jx_list(elems: Vec<TExpr>, elem_ty: Ty) -> TExpr {
        let list_ty = Ty::list(elem_ty);
        let mut out = TExpr::new(TExprKind::Con("[]".into()), list_ty.clone());
        for e in elems.into_iter().rev() {
            out = TExpr::new(TExprKind::InfixApp {
                op: ":".into(),
                lhs: Box::new(e),
                rhs: Box::new(out),
            }, list_ty.clone());
        }
        out
    }

    /// `JObj [pair0, pair1, …]`
    pub(super) fn jx_obj(pairs: Vec<TExpr>) -> TExpr {
        let pair_ty = Ty::Tuple(vec![Ty::Con("String".into()), Self::json_ty()]);
        let list = Self::jx_list(pairs, pair_ty);
        let con_ty = Ty::arrow(list.ty.clone(), Self::json_ty());
        Self::jx_app(TExpr::new(TExprKind::Con("JObj".into()), con_ty), list, Self::json_ty())
    }

    /// `JStr "s"`
    pub(super) fn jx_jstr(s: &str) -> TExpr {
        let con_ty = Ty::arrow(Ty::Con("String".into()), Self::json_ty());
        Self::jx_app(TExpr::new(TExprKind::Con("JStr".into()), con_ty), Self::jx_str(s), Self::json_ty())
    }

    /// `JArr [e0, e1, …]`
    pub(super) fn jx_jarr(elems: Vec<TExpr>) -> TExpr {
        let list = Self::jx_list(elems, Self::json_ty());
        let con_ty = Ty::arrow(list.ty.clone(), Self::json_ty());
        Self::jx_app(TExpr::new(TExprKind::Con("JArr".into()), con_ty), list, Self::json_ty())
    }

    /// Build the encoder expression (of type `field_ty -> Json`) for one
    /// field of a ToJSON-derived constructor — the exact mirror of
    /// `fromjson_field_decoder`, so that everything the derived decoder can
    /// read, the derived encoder writes (and vice versa). Resolution is
    /// STRUCTURAL at derive time: primitives use the toJSON* combinators,
    /// `[t]` and `Maybe t` route through toJSONList/toJSONMaybe, and a field
    /// of another ToJSON type calls that type's own `toJSON_T` encoder —
    /// including self- and mutually-recursive types, via the `tojson_types`
    /// prescan. `Err` carries (reason, note) for the rejection message.
    pub(super) fn tojson_field_encoder(&self, field_ty: &Ty) -> Result<TExpr, (String, String)> {
        let enc_ty = Ty::arrow(field_ty.clone(), Self::json_ty());
        match field_ty {
            Ty::Con(n) if n == "Integer" => Ok(Self::jx_var("toJSONInteger", enc_ty)),
            Ty::Con(n) if n == "Number" => Ok(Self::jx_var("toJSONNumber", enc_ty)),
            Ty::Con(n) if n == "String" => Ok(Self::jx_var("toJSONString", enc_ty)),
            Ty::Con(n) if n == "Bool" => Ok(Self::jx_var("toJSONBool", enc_ty)),
            Ty::Con(n) if n == "Json" => Ok(Self::jx_var("toJSONValue", enc_ty)),
            Ty::List(elem) => {
                let inner = self.tojson_field_encoder(elem)?;
                let list_fn_ty = Ty::arrow(inner.ty.clone(), enc_ty.clone());
                Ok(Self::jx_app(Self::jx_var("toJSONList", list_fn_ty), inner, enc_ty))
            }
            _ if Self::ty_maybe_inner(field_ty).is_some() => {
                let inner_ty = Self::ty_maybe_inner(field_ty).unwrap();
                let inner = self.tojson_field_encoder(inner_ty)?;
                let maybe_fn_ty = Ty::arrow(inner.ty.clone(), enc_ty.clone());
                Ok(Self::jx_app(Self::jx_var("toJSONMaybe", maybe_fn_ty), inner, enc_ty))
            }
            Ty::Con(n) => {
                if self.tojson_types.contains(n)
                    || self.instances.contains_key(&("ToJSON".to_string(), InstHead::Con(n.clone()))) {
                    Ok(Self::jx_var(&format!("toJSON_{}", n), enc_ty))
                } else {
                    Err((
                        format!("the type '{}' has no ToJSON instance", n),
                        format!("every field of a derived encoder needs its own encoder; add `deriving (ToJSON)` to '{}' or write `instance ToJSON {}` in the module that defines it.", n, n),
                    ))
                }
            }
            Ty::Arrow(..) => Err((
                "it is a function type".to_string(),
                "a function has no JSON representation, so no encoder can serialize one; store data instead of behavior, or write the ToJSON instance by hand for an encoding you define.".to_string(),
            )),
            Ty::Tuple(_) => Err((
                format!("the tuple type '{}' has no JSON encoding convention in mata-ll", field_ty),
                "GHC's aeson encodes tuples as fixed-length arrays; mata-ll does not — wrap the tuple in a small record type deriving (ToJSON), which also gives the components names in the JSON.".to_string(),
            )),
            Ty::App(..) => Err((
                format!("the type '{}' is parameterized", field_ty),
                "a derived encoder resolves one concrete encoder per field at compile time, and mata-ll instances cannot cover a parameterized type at every instantiation; wrap the concrete instantiation in its own data type deriving (ToJSON).".to_string(),
            )),
            Ty::IO(_) | Ty::LuaIO(..) => Err((
                "it is an effectful action type".to_string(),
                "an IO action has no JSON representation.".to_string(),
            )),
            Ty::Var(v) => Err((
                format!("its type is the type parameter '{}'", v.name),
                "a type parameter has no encoder the compiler can pick at derive time.".to_string(),
            )),
            other => Err((
                format!("the type '{}' cannot be encoded to JSON", other),
                "derived ToJSON supports Integer, Number, String, Bool, Json, lists, Maybe, and types that themselves have a ToJSON instance.".to_string(),
            )),
        }
    }

    /// Encode constructor argument #i: the resolved encoder applied to the
    /// pattern variable `_x{i}` the clause binds it to.
    pub(super) fn tojson_arg(&self, i: usize, fty: &Ty) -> Result<TExpr, (String, String)> {
        let enc = self.tojson_field_encoder(fty)?;
        Ok(Self::jx_app(enc, Self::jx_var(&format!("_x{}", i), fty.clone()), Self::json_ty()))
    }

    /// The `("key", encoded value)` pairs of a record constructor, keyed by
    /// the fields' effective keys — the same keys `fromjson_named_body`
    /// decodes, and the same shared external name LuaDict uses as the Lua
    /// table key. A Maybe field encodes Nothing as null (via toJSONMaybe in
    /// the field encoder); the decoder's jOptFieldWith reads null back as
    /// Nothing, so the pair round-trips.
    pub(super) fn tojson_named_pairs(
        &self,
        con_name: &str,
        fields: &[RecordField],
        field_tys: &[Ty],
    ) -> Result<Vec<TExpr>, (String, String)> {
        let mut pairs = Vec::new();
        for (i, field) in fields.iter().enumerate() {
            let val = self.tojson_arg(i, &field_tys[i]).map_err(|e| Self::fromjson_field_err(
                format!("field '{}' of constructor '{}' cannot be encoded", field.name, con_name), e))?;
            pairs.push(Self::jx_pair(field.effective_key(), val));
        }
        Ok(pairs)
    }

    /// Encode one constructor's value (its arguments are bound to `_x0` …):
    /// - record fields → a JSON object keyed by the effective keys, with
    ///   `"tag":"Con"` prepended in a tagged type;
    /// - a nullary constructor → the bare string `"Con"` (only reachable
    ///   tagged: a lone nullary constructor is tagged by definition);
    /// - positional arguments → the encoded argument itself (one) or an
    ///   array (several), under `"contents"` in a tagged type.
    pub(super) fn tojson_con_body(
        &self,
        con: &Constructor,
        field_tys: &[Ty],
        tagged: bool,
    ) -> Result<TExpr, (String, String)> {
        match &con.fields {
            ConstructorFields::Named(fields) if !fields.is_empty() => {
                let mut pairs = self.tojson_named_pairs(&con.name, fields, field_tys)?;
                if tagged {
                    pairs.insert(0, Self::jx_pair("tag", Self::jx_jstr(&con.name)));
                }
                Ok(Self::jx_obj(pairs))
            }
            _ if field_tys.is_empty() => Ok(Self::jx_jstr(&con.name)),
            _ => {
                let args: Vec<TExpr> = field_tys.iter().enumerate().map(|(i, fty)| {
                    self.tojson_arg(i, fty).map_err(|e| Self::fromjson_field_err(
                        format!("argument {} of constructor '{}' cannot be encoded", i + 1, con.name), e))
                }).collect::<Result<_, _>>()?;
                let contents = if field_tys.len() == 1 {
                    args.into_iter().next().unwrap()
                } else {
                    Self::jx_jarr(args)
                };
                if tagged {
                    Ok(Self::jx_obj(vec![
                        Self::jx_pair("tag", Self::jx_jstr(&con.name)),
                        Self::jx_pair("contents", contents),
                    ]))
                } else {
                    Ok(contents)
                }
            }
        }
    }

    /// Generate `toJSON` for a data type: `toJSON_T :: T -> Json` built from
    /// the JSON module's encoder combinators, plus the ToJSON instance
    /// registration. The exact mirror of `derive_fromjson`, so
    /// `fromJSON (parseJSON (encodeToJSON x)) == Right x` round-trips.
    ///
    /// Encoding convention (mirrors aeson's defaultOptions where mata-ll can):
    /// - a single-constructor record encodes to an object keyed by the
    ///   fields' effective keys (the `as "key"` rename when present, the
    ///   Haskell field name otherwise): `data P = P { x :: Integer }` ⇒ `{"x":1}`
    /// - a single positional constructor encodes to its argument directly
    ///   (one field) or an array of its arguments (several)
    /// - a multi-constructor type (or a lone nullary constructor) is tagged:
    ///   a nullary constructor is the bare constructor name as a string, and
    ///   a constructor with fields is an object with "tag":"Con" — record
    ///   fields inline in the same object, positional arguments under
    ///   "contents"
    /// - a `Maybe t` record field encodes Nothing as null (the key stays
    ///   present, as aeson's defaultOptions does)
    ///
    /// note: aeson emits `{"tag":"A"}` for a nullary constructor of a MIXED
    /// sum and the bare string only for all-nullary sums; mata-ll emits the
    /// bare string for every nullary constructor. The derived decoder accepts
    /// both forms, so mata-ll round-trips and aeson-encoded data both decode.
    pub(super) fn derive_tojson(
        &mut self,
        type_name: &str,
        type_vars: &[String],
        constructors: &[Constructor],
    ) -> Vec<TFunction> {
        let reject = |checker: &mut Self, reason: String, note: &str| {
            checker.push_error_ctx(
                DiagnosticKind::Other(format!(
                    "Cannot derive 'ToJSON' for '{}': {}\nnote: {}",
                    type_name, reason, note,
                )),
                format!("data {}", type_name),
            );
        };

        // The class and every combinator the generated encoder calls live in
        // the JSON library module; without the import nothing can resolve.
        if !self.classes.contains_key("ToJSON") || self.env.lookup("toJSONList").is_none() {
            reject(self,
                "the ToJSON class and its encoder combinators are not in scope".to_string(),
                "the ToJSON class and the codec combinators the derived encoder calls (toJSONList, toJSONMaybe, …) live in the JSON library module; add `import JSON` at the top of this file.");
            return vec![];
        }

        if !type_vars.is_empty() {
            reject(self,
                format!("'{}' has type parameters", type_name),
                "a derived encoder must pick one concrete encoder per field at compile time, and a field whose type is a type parameter has none. GHC's aeson handles this with a `ToJSON a` constraint on the instance; mata-ll does not derive constrained codecs, so derive ToJSON for concrete types only (wrap each instantiation you need in its own data type).");
            return vec![];
        }

        for con in constructors {
            if con.gadt_type.is_some() || !con.existential_vars.is_empty() {
                reject(self,
                    format!("constructor '{}' is a GADT / existential constructor", con.name),
                    "an encoder must name every field's type to choose how to encode it, which GADT and existential constructors do not allow.");
                return vec![];
            }
        }

        // Tagged encoding applies to multi-constructor types, and to a lone
        // nullary constructor (no fields to encode — the constructor NAME is
        // the payload). Must match derive_fromjson's decision exactly.
        let single_nullary = constructors.len() == 1 && match &constructors[0].fields {
            ConstructorFields::Positional(fs) => fs.is_empty(),
            ConstructorFields::Named(fs) => fs.is_empty(),
        };
        let tagged = constructors.len() > 1 || single_nullary;

        if !self.validate_json_keys("ToJSON", type_name, constructors, tagged) {
            return vec![];
        }

        let result_ty = Ty::Con(type_name.to_string());
        let json = Self::json_ty();
        let mangled = format!("toJSON_{}", type_name);

        // Constructor field types as registered in pass 1.
        let con_field_tys: Vec<Vec<Ty>> = constructors.iter().map(|con| {
            self.constructors.get(self.resolve_con_name(&con.name))
                .map(|ci| ci.field_types.clone())
                .unwrap_or_default()
        }).collect();

        // One clause per constructor; report every unsupported field before
        // bailing out.
        let mut clauses = Vec::new();
        let mut failed = false;
        for (con, ftys) in constructors.iter().zip(&con_field_tys) {
            match self.tojson_con_body(con, ftys, tagged) {
                Ok(body) => {
                    let args = ftys.iter().enumerate()
                        .map(|(i, t)| TPattern::Var(format!("_x{}", i), t.clone()))
                        .collect();
                    clauses.push(TClause {
                        span: None,
                        patterns: vec![TPattern::Constructor { name: self.resolve_con_name(&con.name).to_string(), args }],
                        guards: vec![],
                        body,
                        where_binds: vec![],
                    });
                }
                Err((reason, note)) => {
                    reject(self, reason, &note);
                    failed = true;
                }
            }
        }
        if failed { return vec![]; }

        // Register the instance
        let mut method_fns = HashMap::new();
        method_fns.insert("toJSON".to_string(), mangled.clone());
        self.register_instance(InstanceInfo {
            class_name: "ToJSON".to_string(),
            target_type: result_ty.clone(),
            method_fns,
            context: None,
        });

        vec![TFunction {
            name: mangled,
            ty: Ty::arrow(result_ty, json),
            clauses,
            specialized: false,
            dict_params: vec![],
        }]
    }
}
