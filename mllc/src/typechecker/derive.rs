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
        // The six structural derives walk every constructor's fields by
        // arity and match on the constructor's own type; a constructor with
        // existential variables or a refined (GADT) result type has neither
        // a plain arity nor an instance head that covers it — GHC rejects
        // these too ("has existentials or constraints in its type").
        if matches!(class, "Show" | "Eq" | "Ord" | "Enum" | "Bounded" | "Functor")
            && let Some((con, why)) = self.non_vanilla_constructor(type_vars, constructors)
        {
            self.reject_derive(
                class,
                type_name,
                &format!("constructor '{}' {}", con, why),
                "a derived instance is one head `C (T a b …)` over plain fields; \
                 write the instance by hand for this type",
            );
            return vec![];
        }
        match class {
            "Show" => self.derive_show(type_name, type_vars, constructors),
            "Eq" => self.derive_eq(type_name, type_vars, constructors),
            "Ord" => self.derive_ord(type_name, type_vars, constructors),
            "Enum" => self.derive_enum(type_name, type_vars, constructors),
            "Bounded" => self.derive_bounded(type_name, type_vars, constructors),
            "Functor" => self.derive_functor(type_name, type_vars, constructors),
            "ToJSON" => self.derive_tojson(type_name, type_vars, constructors),
            "FromJSON" => self.derive_fromjson(type_name, type_vars, constructors),
            "Generic" => self.ensure_generic(type_name, type_vars, constructors),
            "LuaDict" => { self.derive_luadict(type_name, constructors); vec![] }
            other => {
                self.push_error_ctx(
                    DiagnosticKind::Other(format!("Cannot derive '{}' — only Show, Eq, Ord, Enum, Bounded, Functor, Generic, ToJSON, FromJSON and LuaDict are supported", other)),
                    format!("data {}", type_name),
                );
                vec![]
            }
        }
    }

    /// Report "Cannot derive 'class' for 'type': reason" with a `note:` —
    /// the one shape every derive's rejection takes.
    pub(super) fn reject_derive(&mut self, class: &str, type_name: &str, reason: &str, note: &str) {
        self.push_error_ctx(
            DiagnosticKind::Other(format!(
                "Cannot derive '{}' for '{}': {}\nnote: {}",
                class, type_name, reason, note,
            )),
            format!("data {}", type_name),
        );
    }

    /// The registered field types of a constructor — the arity every
    /// derive walks and the types it matches on. Read from the constructor
    /// registry, NOT from the parser's `Constructor::fields`: that list is
    /// EMPTY for a GADT-syntax constructor (`Con :: a -> b -> T` keeps its
    /// whole signature in `gadt_type`, and registration decomposes it), and
    /// reading it made the derived Eq/Show/Ord of such a type ignore every
    /// field. Empty for a constructor that failed to register.
    fn derived_field_tys(&self, con: &Constructor) -> Vec<Ty> {
        let key = self.resolve_con_name(&con.name);
        self.constructors.get(key).map(|ci| ci.field_types.clone()).unwrap_or_default()
    }

    /// Nullary as registered (see `derived_field_tys`).
    fn derived_is_nullary(&self, con: &Constructor) -> bool {
        self.derived_field_tys(con).is_empty()
    }

    /// The first constructor a structural derive cannot cover, with the
    /// reason: one with existential variables, or one whose result type is
    /// not the data type applied to its own distinct parameters (a GADT
    /// refinement such as `IntE :: Int -> Expr Int`).
    fn non_vanilla_constructor(
        &self,
        type_vars: &[String],
        constructors: &[Constructor],
    ) -> Option<(String, &'static str)> {
        for con in constructors {
            let key = self.resolve_con_name(&con.name);
            let Some(ci) = self.constructors.get(key) else { continue };
            if !ci.existential_vars.is_empty() {
                return Some((con.name.clone(), "has existential type variables"));
            }
            // Peel `T a b …` into its argument list.
            let mut args = Vec::new();
            let mut t = &ci.result_type;
            while let Ty::App(f, a) = t {
                args.push(a.as_ref());
                t = f;
            }
            args.reverse();
            let plain = args.len() == type_vars.len()
                && args.iter().all(|a| matches!(a, Ty::Var(_)))
                && {
                    let mut seen = HashSet::new();
                    args.iter().all(|a| match a { Ty::Var(v) => seen.insert(&v.name), _ => false })
                };
            if !plain {
                return Some((con.name.clone(), "refines the result type (a GADT constructor)"));
            }
        }
        None
    }

    /// `LuaDict` is an intrinsic deriving: it generates no instance methods but
    /// changes the runtime layout so the value is a Lua table keyed by field
    /// name (`{width = …}`) instead of a positional array. That representation
    /// only makes sense for a single record constructor whose fields all have
    /// names to use as keys, so we validate that here and reject anything else
    /// with an explanation of *why* rather than a bare "cannot derive".
    pub(super) fn derive_luadict(&mut self, type_name: &str, constructors: &[Constructor]) {
        let reject = |checker: &mut Self, reason: String, note: &str| {
            checker.reject_derive("LuaDict", type_name, &reason, note);
        };

        // Shape 1: an all-nullary sum type (every constructor has zero fields).
        // Its runtime value at the Lua boundary is the constructor's string tag
        // — the `as "tag"` rename when present, the constructor name otherwise —
        // rather than a positional integer, so a Lua host reads and writes it as
        // a plain string. A single nullary constructor (`data T = T`) is the
        // degenerate one-variant case. Ordering (`Ord`/`Enum`/`Bounded`) still
        // follows declaration order; the tag is boundary-only.
        let all_nullary = constructors.iter().all(|c| match &c.fields {
            ConstructorFields::Positional(fs) => fs.is_empty(),
            ConstructorFields::Named(fs) => fs.is_empty(),
        });
        if all_nullary {
            // The effective tags become the wire values, so — exactly like the
            // record field keys below — each must be a non-empty string and no
            // two may collide, else two constructors would be indistinguishable
            // at the Lua boundary.
            let mut seen: HashMap<&str, &str> = HashMap::new();
            for con in constructors {
                let tag = con.effective_tag();
                if tag.is_empty() {
                    reject(self,
                        format!("constructor '{}' renames its Lua tag to the empty string", con.name),
                        "the constructor becomes a string at the Lua boundary, and an empty tag names nothing a Lua host could tell apart; give `as` a non-empty string.");
                    return;
                }
                if let Some(prev) = seen.insert(tag, &con.name) {
                    reject(self,
                        format!("constructors '{}' and '{}' both map to the Lua tag \"{}\"", prev, con.name, tag),
                        "each constructor becomes one string at the Lua boundary, so two sharing a tag would be indistinguishable there; rename one with `as \"otherTag\"`.");
                    return;
                }
            }
            self.luadict_types.insert(type_name.to_string());
            return;
        }

        // Shape 2: a single record constructor laid out as a name-keyed table.
        // Reaching here means the type is neither all-nullary nor a single
        // constructor: it has multiple constructors and at least one has fields.
        if constructors.len() != 1 {
            reject(self,
                format!("LuaDict needs one constructor (a record) or an all-nullary sum type, but '{}' has multiple constructors and at least one has fields", type_name),
                "a name-keyed Lua table needs a single record constructor to key by field name; a string enum needs every constructor to be nullary — a multi-constructor type with fields has no single Lua representation.");
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
            // TIR references use the registered key (mangled when this local
            // constructor shadows a Prelude/import one); the *displayed* name
            // stays the source name the user wrote.
            let con_key = self.resolve_con_name(&con.name).to_string();
            let field_tys = self.derived_field_tys(con);
            let field_count = field_tys.len();

            // Build patterns: Con p0 p1 p2 ...
            let param_names: Vec<String> = (0..field_count)
                .map(|i| format!("_s{}", i))
                .collect();

            let patterns = vec![
                TPattern::Constructor {
                    name: con_key,
                    args: param_names.iter().enumerate().map(|(i, n)| {
                        let ty = field_tys.get(i).cloned().unwrap_or(Ty::Unit);
                        TPattern::Var(n.clone(), ty)
                    }).collect(),
                }
            ];

            let str_ty = Ty::Con("String".into());
            let lit = |s: &str| TExpr::new(
                TExprKind::Lit(TLiteral::Str(s.as_bytes().to_vec())),
                Ty::Con("String".into()),
            );
            let concat = |lhs: TExpr, rhs: TExpr| TExpr::new(
                TExprKind::InfixApp {
                    op: "<>".into(),
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                Ty::Con("String".into()),
            );
            // show field_i — precedence depends on the syntax (see below).
            let show_field = |i: usize, pname: &String, arg_prec: bool| {
                let field_ty = field_tys.get(i).cloned().unwrap_or(Ty::Unit);
                let shown = TExpr::new(
                    TExprKind::App(
                        Box::new(TExpr::new(
                            TExprKind::Var("show".into()),
                            Ty::arrow(field_ty.clone(), Ty::Con("String".into())),
                        )),
                        Box::new(TExpr::new(TExprKind::Var(pname.clone()), field_ty)),
                    ),
                    Ty::Con("String".into()),
                );
                if !arg_prec {
                    return shown;
                }
                // __mll_show_arg (show field_i) — parenthesize the field if
                // it is a constructor application or negative number (GHC
                // showsPrec 11).
                TExpr::new(
                    TExprKind::App(
                        Box::new(TExpr::new(
                            TExprKind::Var("__mll_show_arg".into()),
                            Ty::arrow(str_ty.clone(), str_ty.clone()),
                        )),
                        Box::new(shown),
                    ),
                    Ty::Con("String".into()),
                )
            };

            let named_fields: Option<Vec<String>> = match &con.fields {
                ConstructorFields::Named(fs) if !fs.is_empty() =>
                    Some(fs.iter().map(|f| f.name.clone()).collect()),
                _ => None,
            };

            let body = match named_fields {
                // Record syntax shows as GHC does: `Con {f1 = v1, f2 = v2}`,
                // with ", " between fields and each value at precedence 0
                // (never parenthesized — `P {px = -1}` has no inner parens).
                Some(fnames) => {
                    let mut body = lit(&format!("{} {{", con.name));
                    for (i, (pname, fname)) in
                        param_names.iter().zip(fnames.iter()).enumerate()
                    {
                        if i > 0 {
                            body = concat(body, lit(", "));
                        }
                        body = concat(body, lit(&format!("{} = ", fname)));
                        body = concat(body, show_field(i, pname, false));
                    }
                    concat(body, lit("}"))
                }
                // Positional: "ConName" ++ " " ++ show p0 ++ " " ++ show p1 …
                // with each field at argument precedence (showsPrec 11).
                None => {
                    let mut body = lit(&con.name);
                    for (i, pname) in param_names.iter().enumerate() {
                        body = concat(body, lit(" "));
                        body = concat(body, show_field(i, pname, true));
                    }
                    body
                }
            };

            clauses.push(TClause {
                span: None,
                patterns,
                guards: vec![],
                body: Some(body),
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
            derived_strict: false,
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
            let con_key = self.resolve_con_name(&con.name).to_string();
            let field_tys = self.derived_field_tys(con);
            let field_count = field_tys.len();

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
                body: Some(body),
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
                body: Some(TExpr::new(TExprKind::Lit(TLiteral::Bool(false)), Ty::Con("Bool".into()))),
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
            // Strict in both operands by construction: every per-constructor
            // clause matches a constructor pattern on BOTH positions, and the
            // `_ _ = False` catch-all is reachable only after clause dispatch
            // has forced both scrutinees (the first argument's constructor is
            // inspected by clause 1, and whichever constructor it is, that
            // constructor's own clause then inspects the second argument).
            derived_strict: true,
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
        let is_enum = constructors.iter().all(|c| self.derived_is_nullary(c));

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
            let fc_a = self.derived_field_tys(con_a).len();
            for (j, con_b) in constructors.iter().enumerate() {
                let fc_b = self.derived_field_tys(con_b).len();
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
                        body: Some(TExpr::new(TExprKind::Con(ord_con.to_string()), ordering_ty.clone())),
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
                                    body: Some(body),
                                },
                                TCaseBranch {
                                    pattern: TPattern::Var("_o".into(), ordering_ty.clone()),
                                    guards: vec![],
                                    body: Some(TExpr::new(TExprKind::Var("_o".into()), ordering_ty.clone())),
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
                    body: Some(body),
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
            // Strict in both operands by construction: the clauses cover every
            // (constructor, constructor) pair, each matching a constructor
            // pattern on BOTH positions, so no clause can be selected before
            // both scrutinees are forced.
            derived_strict: true,
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
                            body: Some(TExpr::new(TExprKind::Lit(TLiteral::Bool(result)), bool_ty.clone())),
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
                    body: Some(TExpr::new(
                        TExprKind::Case {
                            scrutinee: Box::new(cmp_call),
                            branches: vec![
                                TCaseBranch {
                                    pattern: TPattern::Constructor { name: match_con.into(), args: vec![] },
                                    guards: vec![],
                                    body: Some(TExpr::new(TExprKind::Lit(TLiteral::Bool(on_match)), bool_ty.clone())),
                                },
                                TCaseBranch {
                                    pattern: TPattern::Wildcard,
                                    guards: vec![],
                                    body: Some(TExpr::new(TExprKind::Lit(TLiteral::Bool(!on_match)), bool_ty.clone())),
                                },
                            ],
                        },
                        bool_ty.clone(),
                    )),
                    where_binds: vec![],
                }]
            };

            functions.push(TFunction {
                name: mangled.clone(),
                ty: fn_ty.clone(),
                clauses,
                specialized: false,
                dict_params: vec![],
                // Strict in both operands by construction, in both shapes: the
                // enum clauses match constructor patterns on both positions,
                // and the non-enum body immediately scrutinizes
                // `compare _a _b`, whose derived implementation (above) forces
                // both.
                derived_strict: true,
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
        let is_enum = constructors.iter().all(|c| self.derived_is_nullary(c));
        if !is_enum || constructors.is_empty() {
            self.reject_derive(
                "Enum",
                type_name,
                "an enumeration is one or more constructors with no fields",
                "fromEnum/toEnum number the constructors in declaration order; a constructor with fields has no single number, and an empty type has none to number",
            );
            return vec![];
        }

        let result_type = Ty::Con(type_name.to_string());
        let int_ty = Ty::Con("Int".into());
        let list_ty = Ty::List(Box::new(result_type.clone()));
        let n = constructors.len();

        let mut functions = Vec::new();

        // fromEnum_T :: T -> Int
        let from_name = format!("fromEnum_{}", type_name);
        {
            let clauses: Vec<TClause> = constructors.iter().enumerate().map(|(i, con)| {
                TClause {
                    span: None,
                    patterns: vec![TPattern::Constructor { name: self.resolve_con_name(&con.name).to_string(), args: vec![] }],
                    guards: vec![],
                    body: Some(TExpr::new(TExprKind::Lit(TLiteral::Integer(i as i64)), int_ty.clone())),
                    where_binds: vec![],
                }
            }).collect();
            functions.push(TFunction {
                name: from_name.clone(),
                ty: Ty::arrow(result_type.clone(), int_ty.clone()),
                clauses,
                specialized: false,
                dict_params: vec![],
                derived_strict: false,
            });
        }

        // toEnum_T :: Int -> T
        let to_name = format!("toEnum_{}", type_name);
        {
            let mut clauses: Vec<TClause> = constructors.iter().enumerate().map(|(i, con)| {
                TClause {
                    span: None,
                    patterns: vec![TPattern::LitPat(TLiteral::Integer(i as i64))],
                    guards: vec![],
                    body: Some(TExpr::new(TExprKind::Con(self.resolve_con_name(&con.name).to_string()), result_type.clone())),
                    where_binds: vec![],
                }
            }).collect();
            // Error clause for out of range
            clauses.push(TClause {
                span: None,
                patterns: vec![TPattern::Wildcard],
                guards: vec![],
                body: Some(TExpr::new(
                    TExprKind::App(
                        Box::new(TExpr::new(TExprKind::Var("error".into()), Ty::Unit)),
                        Box::new(TExpr::new(TExprKind::Lit(TLiteral::Str(
                            format!("toEnum: index out of range for {}", type_name).into_bytes()
                        )), Ty::Con("String".into()))),
                    ),
                    result_type.clone(),
                )),
                where_binds: vec![],
            });
            functions.push(TFunction {
                name: to_name.clone(),
                ty: Ty::arrow(int_ty.clone(), result_type.clone()),
                clauses,
                specialized: false,
                dict_params: vec![],
                derived_strict: false,
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
                    body: Some(TExpr::new(TExprKind::Con(self.resolve_con_name(&constructors[i+1].name).to_string()), result_type.clone())),
                    where_binds: vec![],
                });
            }
            // succ of last = error
            clauses.push(TClause {
                span: None,
                patterns: vec![TPattern::Wildcard],
                guards: vec![],
                body: Some(TExpr::new(
                    TExprKind::App(
                        Box::new(TExpr::new(TExprKind::Var("error".into()), Ty::Unit)),
                        Box::new(TExpr::new(TExprKind::Lit(TLiteral::Str(
                            format!("succ: already at maxBound for {}", type_name).into_bytes()
                        )), Ty::Con("String".into()))),
                    ),
                    result_type.clone(),
                )),
                where_binds: vec![],
            });
            functions.push(TFunction {
                name: succ_name.clone(),
                ty: Ty::arrow(result_type.clone(), result_type.clone()),
                clauses,
                specialized: false,
                dict_params: vec![],
                derived_strict: false,
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
                    body: Some(TExpr::new(TExprKind::Con(self.resolve_con_name(&constructors[i-1].name).to_string()), result_type.clone())),
                    where_binds: vec![],
                });
            }
            // pred of first = error
            clauses.push(TClause {
                span: None,
                patterns: vec![TPattern::Wildcard],
                guards: vec![],
                body: Some(TExpr::new(
                    TExprKind::App(
                        Box::new(TExpr::new(TExprKind::Var("error".into()), Ty::Unit)),
                        Box::new(TExpr::new(TExprKind::Lit(TLiteral::Str(
                            format!("pred: already at minBound for {}", type_name).into_bytes()
                        )), Ty::Con("String".into()))),
                    ),
                    result_type.clone(),
                )),
                where_binds: vec![],
            });
            functions.push(TFunction {
                name: pred_name.clone(),
                ty: Ty::arrow(result_type.clone(), result_type.clone()),
                clauses,
                specialized: false,
                dict_params: vec![],
                derived_strict: false,
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
                    guards: vec![], body: Some(body), where_binds: vec![],
                }],
                specialized: false,
                dict_params: vec![],
                derived_strict: false,
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
                    guards: vec![], body: Some(body), where_binds: vec![],
                }],
                specialized: false,
                dict_params: vec![],
                derived_strict: false,
            });
        }

        // The stepped ranges share one recursive helper over Int indices.
        // Recursing on T values (the enumFromTo approach) does not work here:
        // with step s, the successor pair needs toEnum (i + s), which can
        // overshoot the constructor range before the termination check runs.
        // On indices the check comes first, so toEnum only ever sees an index
        // between the start and the (in-range) limit.
        //
        // GHC semantics, matched exactly: ascending (step >= 0) stops past
        // the limit, descending stops below it, and step 0 with a reachable
        // limit is an infinite list of the start element — as for
        // [x, x ..] :: [Int]. The runtime is lazy, so the infinite case is as
        // representable here as it is in GHC.

        let int_var = |name: &str| TExpr::new(TExprKind::Var(name.into()), int_ty.clone());
        let int_lit = |v: i64| TExpr::new(TExprKind::Lit(TLiteral::Integer(v)), int_ty.clone());
        let int_op = |op: &str, lhs: TExpr, rhs: TExpr| TExpr::new(TExprKind::InfixApp {
            op: op.into(),
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        }, int_ty.clone());
        let cmp = |op: &str, lhs: TExpr, rhs: TExpr| TExpr::new(TExprKind::InfixApp {
            op: op.into(),
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        }, Ty::Con("Bool".into()));
        let apply = |fn_name: &str, args: Vec<TExpr>, ret: Ty| {
            let mut e = TExpr::new(TExprKind::Var(fn_name.into()), Ty::Unit);
            let last = args.len() - 1;
            for (i, arg) in args.into_iter().enumerate() {
                let ty = if i == last { ret.clone() } else { Ty::Unit };
                e = TExpr::new(TExprKind::App(Box::new(e), Box::new(arg)), ty);
            }
            e
        };
        let from_enum = |v: &str| apply(&from_name, vec![
            TExpr::new(TExprKind::Var(v.into()), result_type.clone()),
        ], int_ty.clone());

        // enumStep_T :: Int -> Int -> Int -> [T]
        // enumStep i step limit — the index walk behind both stepped ranges.
        let enum_step_name = format!("enumStep_{}", type_name);
        {
            // if step >= 0 then (if i > limit then [] else toEnum i : go)
            //              else (if i < limit then [] else toEnum i : go)
            let step_branch = |stop_op: &str| {
                let stop = cmp(stop_op, int_var("_i"), int_var("_l"));
                let head = apply(&to_name, vec![int_var("_i")], result_type.clone());
                let tail = apply(&enum_step_name, vec![
                    int_op("+", int_var("_i"), int_var("_s")),
                    int_var("_s"),
                    int_var("_l"),
                ], list_ty.clone());
                let cons = TExpr::new(TExprKind::InfixApp {
                    op: ":".into(),
                    lhs: Box::new(head),
                    rhs: Box::new(tail),
                }, list_ty.clone());
                TExpr::new(TExprKind::If {
                    cond: Box::new(stop),
                    then_branch: Box::new(TExpr::new(TExprKind::Lit(TLiteral::Unit), list_ty.clone())),
                    else_branch: Box::new(cons),
                }, list_ty.clone())
            };
            let body = TExpr::new(TExprKind::If {
                cond: Box::new(cmp(">=", int_var("_s"), int_lit(0))),
                then_branch: Box::new(step_branch(">")),
                else_branch: Box::new(step_branch("<")),
            }, list_ty.clone());
            functions.push(TFunction {
                name: enum_step_name.clone(),
                ty: Ty::fun(&[int_ty.clone(), int_ty.clone(), int_ty.clone()], list_ty.clone()),
                clauses: vec![TClause {
                    span: None,
                    patterns: vec![
                        TPattern::Var("_i".into(), int_ty.clone()),
                        TPattern::Var("_s".into(), int_ty.clone()),
                        TPattern::Var("_l".into(), int_ty.clone()),
                    ],
                    guards: vec![], body: Some(body), where_binds: vec![],
                }],
                specialized: false,
                dict_params: vec![],
                derived_strict: false,
            });
        }

        // enumFromThen_T :: T -> T -> [T]
        // enumFromThen a b = enumStep (fromEnum a) (fromEnum b - fromEnum a) limit
        //   where limit = maxBound index when ascending, 0 when descending
        // (GHC picks the limit from the step direction the same way.)
        let enum_from_then_name = format!("enumFromThen_{}", type_name);
        {
            let limit = TExpr::new(TExprKind::If {
                cond: Box::new(cmp(">=", from_enum("_b"), from_enum("_a"))),
                then_branch: Box::new(int_lit(n as i64 - 1)),
                else_branch: Box::new(int_lit(0)),
            }, int_ty.clone());
            let body = apply(&enum_step_name, vec![
                from_enum("_a"),
                int_op("-", from_enum("_b"), from_enum("_a")),
                limit,
            ], list_ty.clone());
            functions.push(TFunction {
                name: enum_from_then_name.clone(),
                ty: Ty::fun(&[result_type.clone(), result_type.clone()], list_ty.clone()),
                clauses: vec![TClause {
                    span: None,
                    patterns: vec![
                        TPattern::Var("_a".into(), result_type.clone()),
                        TPattern::Var("_b".into(), result_type.clone()),
                    ],
                    guards: vec![], body: Some(body), where_binds: vec![],
                }],
                specialized: false,
                dict_params: vec![],
                derived_strict: false,
            });
        }

        // enumFromThenTo_T :: T -> T -> T -> [T]
        // enumFromThenTo a b c = enumStep (fromEnum a) (fromEnum b - fromEnum a) (fromEnum c)
        let enum_from_then_to_name = format!("enumFromThenTo_{}", type_name);
        {
            let body = apply(&enum_step_name, vec![
                from_enum("_a"),
                int_op("-", from_enum("_b"), from_enum("_a")),
                from_enum("_c"),
            ], list_ty.clone());
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
                    guards: vec![], body: Some(body), where_binds: vec![],
                }],
                specialized: false,
                dict_params: vec![],
                derived_strict: false,
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
        let is_enum = constructors.iter().all(|c| self.derived_is_nullary(c));
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
                body: Some(TExpr::new(TExprKind::Con(self.resolve_con_name(&constructors.first().unwrap().name).to_string()), result_type.clone())),
                where_binds: vec![],
            }],
            specialized: false,
            dict_params: vec![],
            derived_strict: false,
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
                body: Some(TExpr::new(TExprKind::Con(self.resolve_con_name(&constructors.last().unwrap().name).to_string()), result_type.clone())),
                where_binds: vec![],
            }],
            specialized: false,
            dict_params: vec![],
            derived_strict: false,
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
            Ty::Arrow(a, b, _) | Ty::App(a, b) => {
                Self::ty_mentions_var(a, var_name) || Self::ty_mentions_var(b, var_name)
            }
            Ty::List(a) | Ty::IO(a) => Self::ty_mentions_var(a, var_name),
            Ty::LuaIO(_, a) => Self::ty_mentions_var(a, var_name),
            Ty::Forall(_, a) => Self::ty_mentions_var(a, var_name),
            Ty::Tuple(elems) => elems.iter().any(|e| Self::ty_mentions_var(e, var_name)),
        }
    }

    /// Generate the expression that maps a field VALUE through the functor
    /// function — recursing structurally, GHC-style, so the function reaches
    /// every occurrence of the class variable no matter how deeply nested
    /// (`Maybe [a]` maps the list's elements, `[Rose a]` maps each subtree,
    /// tuples map their relevant components, and a covariant function field
    /// `Int -> a` post-composes). Positions a Functor cannot map — the class
    /// variable in a function ARGUMENT (contravariant) or in a non-last
    /// argument of a type constructor — are reported as errors, exactly the
    /// cases GHC's DeriveFunctor rejects.
    ///
    /// `value` must be a pure, cheaply duplicable expression (a variable or a
    /// projection of one); tuple fields duplicate it per component.
    pub(super) fn functor_map_value(
        &self,
        field_ty: &Ty,
        last_var: &str,
        value: TExpr,
        b_ty: &Ty,
        self_fmap: &str,
        fresh: &mut usize,
    ) -> Result<TExpr, String> {
        if !Self::ty_mentions_var(field_ty, last_var) {
            // No occurrence of the functor parameter → pass through.
            return Ok(value);
        }
        match field_ty {
            Ty::Var(tv) if tv.name == last_var => {
                // The field IS the functor parameter → apply _f.
                Ok(TExpr::new(
                    TExprKind::App(
                        Box::new(TExpr::new(
                            TExprKind::Var("_f".to_string()),
                            Ty::arrow(field_ty.clone(), b_ty.clone()),
                        )),
                        Box::new(value),
                    ),
                    b_ty.clone(),
                ))
            }
            Ty::Var(_) => Ok(value),
            Ty::Tuple(elems) => {
                // Map each component that mentions the variable; the others
                // pass through. Components are reached by projection.
                let mut mapped = Vec::new();
                for (i, ety) in elems.iter().enumerate() {
                    let proj = TExpr::new(
                        TExprKind::SpecCall {
                            original: format!("_t_{}", i),
                            specialized: format!("__mll_tup_get:{}", i + 1),
                            args: vec![value.clone()],
                        },
                        ety.clone(),
                    );
                    mapped.push(self.functor_map_value(ety, last_var, proj, b_ty, self_fmap, fresh)?);
                }
                Ok(TExpr::new(TExprKind::Tuple(mapped), field_ty.clone()))
            }
            Ty::Arrow(arg, res, _) => {
                if Self::ty_mentions_var(arg, last_var) {
                    // Contravariant occurrence: fmap would have to transform
                    // the function's ARGUMENT backwards, which no Functor can.
                    return Err(format!(
                        "the type variable '{}' appears in the argument of a function \
                         field ({}). fmap can only transform values a structure \
                         CONTAINS; here it would have to transform values the field \
                         CONSUMES, running the mapping backwards. GHC rejects this \
                         deriving for the same reason",
                        last_var, field_ty
                    ));
                }
                // Covariant function field (`Int -> a`): post-compose.
                *fresh += 1;
                let g_name = format!("_g{}", fresh);
                let x_name = format!("_x{}", fresh);
                let applied = TExpr::new(
                    TExprKind::App(
                        Box::new(TExpr::new(TExprKind::Var(g_name.clone()), field_ty.clone())),
                        Box::new(TExpr::new(TExprKind::Var(x_name.clone()), (**arg).clone())),
                    ),
                    (**res).clone(),
                );
                let mapped_res = self.functor_map_value(res, last_var, applied, b_ty, self_fmap, fresh)?;
                let inner = TExpr::new(
                    TExprKind::Lambda {
                        params: vec![(x_name, (**arg).clone())],
                        body: Box::new(mapped_res),
                    },
                    field_ty.clone(),
                );
                let wrap = TExpr::new(
                    TExprKind::Lambda {
                        params: vec![(g_name, field_ty.clone())],
                        body: Box::new(inner),
                    },
                    Ty::arrow(field_ty.clone(), field_ty.clone()),
                );
                Ok(TExpr::new(
                    TExprKind::App(Box::new(wrap), Box::new(value)),
                    field_ty.clone(),
                ))
            }
            Ty::List(elem) | Ty::IO(elem) => {
                self.functor_map_container(field_ty, elem, last_var, value, b_ty, self_fmap, fresh)
            }
            Ty::App(_, _) => {
                // Peel the application spine: every occurrence of the class
                // variable must be inside the LAST argument (GHC's rule) —
                // fmap for the head container only reaches that position.
                let mut head = field_ty;
                let mut args: Vec<&Ty> = Vec::new();
                while let Ty::App(f, a) = head {
                    args.push(a.as_ref());
                    head = f.as_ref();
                }
                args.reverse();
                if Self::ty_mentions_var(head, last_var)
                    || args[..args.len() - 1].iter().any(|a| Self::ty_mentions_var(a, last_var))
                {
                    return Err(format!(
                        "the type variable '{}' is used in a position other than the \
                         last argument of '{}'. fmap can only map over a type \
                         constructor's last argument, so no lawful Functor exists for \
                         this shape. GHC rejects this deriving for the same reason",
                        last_var, field_ty
                    ));
                }
                let elem = *args.last().unwrap();
                self.functor_map_container(field_ty, elem, last_var, value, b_ty, self_fmap, fresh)
            }
            _ => Err(format!(
                "the type variable '{}' occurs in a field of shape '{}' that \
                 fmap cannot map over",
                last_var, field_ty
            )),
        }
    }

    /// Map over a container field (`[u]`, `Maybe u`, `Tree u`, `IO u`): apply
    /// the container's fmap (resolved at derive time) to the mapping function
    /// for the ELEMENT type — `_f` itself when the element is the class
    /// variable, a recursive mapping lambda otherwise (this is what makes
    /// `Maybe [a]` and `[Rose a]` map every level, where the old single-level
    /// code applied `_f` to the whole inner structure).
    #[allow(clippy::too_many_arguments)]
    fn functor_map_container(
        &self,
        container_ty: &Ty,
        elem_ty: &Ty,
        last_var: &str,
        value: TExpr,
        b_ty: &Ty,
        self_fmap: &str,
        fresh: &mut usize,
    ) -> Result<TExpr, String> {
        let a_ty = Ty::Var(TyVar { name: last_var.to_string(), id: u32::MAX });
        let mapper = if matches!(elem_ty, Ty::Var(tv) if tv.name == last_var) {
            TExpr::new(
                TExprKind::Var("_f".to_string()),
                Ty::arrow(a_ty.clone(), b_ty.clone()),
            )
        } else {
            *fresh += 1;
            let e_name = format!("_e{}", fresh);
            let e_var = TExpr::new(TExprKind::Var(e_name.clone()), elem_ty.clone());
            let mapped = self.functor_map_value(elem_ty, last_var, e_var, b_ty, self_fmap, fresh)?;
            TExpr::new(
                TExprKind::Lambda {
                    params: vec![(e_name, elem_ty.clone())],
                    body: Box::new(mapped),
                },
                Ty::arrow(elem_ty.clone(), elem_ty.clone()),
            )
        };
        let fmap_resolved = self.resolve_functor_fmap(container_ty, self_fmap);
        let fmap_f = TExpr::new(
            TExprKind::App(
                Box::new(TExpr::new(
                    TExprKind::Var(fmap_resolved),
                    Ty::arrow(
                        Ty::arrow(a_ty, b_ty.clone()),
                        Ty::arrow(container_ty.clone(), container_ty.clone()),
                    ),
                )),
                Box::new(mapper),
            ),
            Ty::arrow(container_ty.clone(), container_ty.clone()),
        );
        Ok(TExpr::new(
            TExprKind::App(Box::new(fmap_f), Box::new(value)),
            container_ty.clone(),
        ))
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
            let con_key = self.resolve_con_name(&con.name).to_string();
            let field_tys = self.derived_field_tys(con);
            let field_count = field_tys.len();

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
            let mut fresh = 0usize;
            for (i, pname) in param_names.iter().enumerate() {
                let field_ty = field_tys.get(i).cloned().unwrap_or(Ty::Unit);
                let value = TExpr::new(TExprKind::Var(pname.clone()), field_ty.clone());
                let mapped = match self.functor_map_value(
                    &field_ty, &last_tv_name, value, &b_ty_val, &mangled, &mut fresh,
                ) {
                    Ok(m) => m,
                    Err(reason) => {
                        self.push_error_ctx(
                            DiagnosticKind::Other(format!(
                                "Cannot derive 'Functor' for '{}': {}",
                                type_name, reason
                            )),
                            format!("data {}", type_name),
                        );
                        return vec![];
                    }
                };
                body = TExpr::new(
                    TExprKind::App(Box::new(body), Box::new(mapped)),
                    output_type.clone(),
                );
            }

            clauses.push(TClause {
                span: None,
                patterns,
                guards: vec![],
                body: Some(body),
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
            derived_strict: false,
        }]
    }

    // --- Generic deriving ---

    /// Register a compiler-synthesised metadata marker type: a nullary type
    /// constructor (kind `Type`) that carries no values, existing only so a
    /// `Datatype`/`Constructor`/`Selector` instance can be keyed on it. Each
    /// datatype, constructor and field gets its own marker, so the per-name
    /// instances never collide under head-keyed dispatch.
    pub(super) fn register_meta_type(&mut self, name: &str) {
        self.kinds.insert(name.to_string(), Kind::Type);
    }

    /// A metadata reflection function `\_ -> lit` returning a compile-time
    /// constant (a name, arity or record flag baked in at derive time). It
    /// ignores its argument, so its parameter type is a fresh variable.
    fn meta_const_fn(&self, fn_name: &str, lit: TLiteral, ret_ty: Ty) -> TFunction {
        let arg_ty = Ty::Var(TyVar { name: "_mw".into(), id: u32::MAX });
        TFunction {
            name: fn_name.to_string(),
            ty: Ty::arrow(arg_ty.clone(), ret_ty.clone()),
            clauses: vec![TClause {
                span: None,
                patterns: vec![TPattern::Var("_meta".into(), arg_ty)],
                guards: vec![],
                body: Some(TExpr::new(TExprKind::Lit(lit), ret_ty)),
                where_binds: vec![],
            }],
            specialized: false,
            dict_params: vec![],
            derived_strict: false,
        }
    }

    pub(super) fn meta_string_fn(&self, fn_name: &str, value: &str) -> TFunction {
        self.meta_const_fn(fn_name, TLiteral::Str(value.as_bytes().to_vec()), Ty::Con("String".into()))
    }

    fn meta_int_fn(&self, fn_name: &str, value: i64) -> TFunction {
        self.meta_const_fn(fn_name, TLiteral::Integer(value), Ty::Con("Int".into()))
    }

    fn meta_bool_fn(&self, fn_name: &str, value: bool) -> TFunction {
        self.meta_const_fn(fn_name, TLiteral::Bool(value), Ty::Con("Bool".into()))
    }

    /// Make sure `type_name` has its Generic substrate (Rep equation,
    /// from/to conversions, metadata markers and instances). The JSON
    /// derives are built on it, and a type may derive any combination of
    /// (Generic, ToJSON, FromJSON) in any order — the first derive that
    /// needs the substrate creates it; later ones find it present.
    pub(super) fn ensure_generic(
        &mut self,
        type_name: &str,
        type_vars: &[String],
        constructors: &[Constructor],
    ) -> Vec<TFunction> {
        if self.instances.contains_key(&("Generic".to_string(), InstHead::Con(type_name.to_string()))) {
            return vec![];
        }
        self.derive_generic(type_name, type_vars, constructors)
    }

    /// `deriving (Generic)`: synthesise the structural representation `Rep T`,
    /// the `from`/`to` conversions, and the datatype/constructor/selector
    /// metadata — the substrate every generic function (JSON codecs, …) runs
    /// on. Requires `Data.Generics` to be in scope (it declares `Generic`, the
    /// `Rep` family and the representation combinators).
    pub(super) fn derive_generic(
        &mut self,
        type_name: &str,
        type_vars: &[String],
        constructors: &[Constructor],
    ) -> Vec<TFunction> {
        if !self.classes.contains_key("Generic") {
            self.push_error_ctx(
                DiagnosticKind::Other(format!(
                    "Cannot derive 'Generic' for '{}': the Generic class is not in scope — add `import Data.Generics`",
                    type_name)),
                format!("data {}", type_name),
            );
            return vec![];
        }
        for con in constructors {
            if con.gadt_type.is_some() || !con.existential_vars.is_empty() {
                self.push_error_ctx(
                    DiagnosticKind::Other(format!(
                        "Cannot derive 'Generic' for '{}': constructor '{}' is a GADT or existential constructor, which has no structural representation",
                        type_name, con.name)),
                    format!("data {}", type_name),
                );
                return vec![];
            }
        }
        if constructors.is_empty() {
            self.push_error_ctx(
                DiagnosticKind::Other(format!(
                    "Cannot derive 'Generic' for '{}': a type with no constructors has no representation",
                    type_name)),
                format!("data {}", type_name),
            );
            return vec![];
        }

        let tvars: Vec<TyVar> = type_vars.iter()
            .map(|n| TyVar { name: n.clone(), id: u32::MAX })
            .collect();
        let result_type = tvars.iter().fold(
            Ty::Con(type_name.to_string()),
            |acc, tv| Ty::app(acc, Ty::Var(tv.clone())),
        );

        // Rep type builders.
        let k1_t = |t: Ty| Ty::app(Ty::Con("K1".into()), t);
        let sum_t = |a: Ty, b: Ty| Ty::app(Ty::app(Ty::Con(":+:".into()), a), b);
        let prod_t = |a: Ty, b: Ty| Ty::app(Ty::app(Ty::Con(":*:".into()), a), b);
        let d1_t = |d: Ty, f: Ty| Ty::app(Ty::app(Ty::Con("D1".into()), d), f);
        let c1_t = |c: Ty, f: Ty| Ty::app(Ty::app(Ty::Con("C1".into()), c), f);
        let s1_t = |s: Ty, f: Ty| Ty::app(Ty::app(Ty::Con("S1".into()), s), f);
        let u1_t = Ty::Con("U1".into());

        // Value/pattern helpers.
        let con_e = |name: &str, ty: Ty| TExpr::new(TExprKind::Con(name.into()), ty);
        let app_e = |f: TExpr, a: TExpr, ty: Ty|
            TExpr::new(TExprKind::App(Box::new(f), Box::new(a)), ty);

        let mut out_fns: Vec<TFunction> = Vec::new();

        // Datatype metadata marker + instance (name + constructor count).
        let d_marker = format!("__Meta_D_{}", type_name);
        self.register_meta_type(&d_marker);
        let dt_fn = format!("__meta_datatypeName_{}", type_name);
        let dc_fn = format!("__meta_datatypeConCount_{}", type_name);
        out_fns.push(self.meta_string_fn(&dt_fn, type_name));
        out_fns.push(self.meta_int_fn(&dc_fn, constructors.len() as i64));
        self.register_instance(InstanceInfo {
            class_name: "Datatype".to_string(),
            target_type: Ty::Con(d_marker.clone()),
            method_fns: HashMap::from([
                ("datatypeName".to_string(), dt_fn),
                ("datatypeConCount".to_string(), dc_fn),
            ]),
            context: None,
        });

        // Per constructor: gather field info, meta markers, and the C1 rep type.
        struct ConRep { rep_ty: Ty, field_tys: Vec<Ty>, con_key: String,
                        s_markers: Vec<String> }
        let mut cons: Vec<ConRep> = Vec::new();

        for con in constructors {
            let con_key = self.resolve_con_name(&con.name).to_string();
            let field_tys: Vec<Ty> = self.constructors.get(&con_key)
                .map(|c| c.field_types.clone()).unwrap_or_default();
            let field_names: Vec<String> = match &con.fields {
                ConstructorFields::Named(fs) => fs.iter().map(|f| f.effective_key().to_string()).collect(),
                ConstructorFields::Positional(_) => vec![String::new(); field_tys.len()],
            };

            // Constructor metadata marker + instance (name, arity, record-ness).
            let c_marker = format!("__Meta_C_{}_{}", type_name, con.name);
            self.register_meta_type(&c_marker);
            let cn_fn = format!("__meta_conName_{}_{}", type_name, con.name);
            let ca_fn = format!("__meta_conArity_{}_{}", type_name, con.name);
            let cr_fn = format!("__meta_conIsRecord_{}_{}", type_name, con.name);
            let is_record = matches!(&con.fields, ConstructorFields::Named(fs) if !fs.is_empty());
            let arity = match &con.fields {
                ConstructorFields::Named(fs) => fs.len(),
                ConstructorFields::Positional(fs) => fs.len(),
            };
            out_fns.push(self.meta_string_fn(&cn_fn, con.effective_tag()));
            out_fns.push(self.meta_int_fn(&ca_fn, arity as i64));
            out_fns.push(self.meta_bool_fn(&cr_fn, is_record));
            self.register_instance(InstanceInfo {
                class_name: "Constructor".to_string(),
                target_type: Ty::Con(c_marker.clone()),
                method_fns: HashMap::from([
                    ("conName".to_string(), cn_fn),
                    ("conArity".to_string(), ca_fn),
                    ("conIsRecord".to_string(), cr_fn),
                ]),
                context: None,
            });

            // Selector metadata markers + instances, and each field's S1(K1 t).
            let mut s_markers = Vec::new();
            let mut field_reps: Vec<Ty> = Vec::new();
            for (fi, fty) in field_tys.iter().enumerate() {
                let s_marker = format!("__Meta_S_{}_{}_{}", type_name, con.name, fi);
                self.register_meta_type(&s_marker);
                let sn_fn = format!("__meta_selName_{}_{}_{}", type_name, con.name, fi);
                out_fns.push(self.meta_string_fn(&sn_fn, &field_names[fi]));
                self.register_instance(InstanceInfo {
                    class_name: "Selector".to_string(),
                    target_type: Ty::Con(s_marker.clone()),
                    method_fns: HashMap::from([("selName".to_string(), sn_fn)]),
                    context: None,
                });
                field_reps.push(s1_t(Ty::Con(s_marker.clone()), k1_t(fty.clone())));
                s_markers.push(s_marker);
            }

            // Product of the fields (right-nested), or U1 when there are none.
            let prod_ty = field_reps.into_iter().rev()
                .reduce(|acc, t| prod_t(t, acc))
                .unwrap_or_else(|| u1_t.clone());
            let rep_ty = c1_t(Ty::Con(c_marker.clone()), prod_ty);
            cons.push(ConRep { rep_ty, field_tys, con_key, s_markers });
        }

        let n = cons.len();
        // Sum suffixes: suffix[i] = rep_i :+: rep_{i+1} :+: … (full sum = suffix[0]).
        let mut suffix: Vec<Ty> = vec![Ty::Unit; n];
        suffix[n - 1] = cons[n - 1].rep_ty.clone();
        for i in (0..n - 1).rev() {
            suffix[i] = sum_t(cons[i].rep_ty.clone(), suffix[i + 1].clone());
        }
        let sum_ty = suffix[0].clone();
        let rep_type = d1_t(Ty::Con(d_marker.clone()), sum_ty.clone());

        // Extend the `Rep` family with `Rep <result_type> = rep_type`.
        self.ty_families.add_equation("Rep", vec![result_type.clone()], rep_type.clone());

        // Build `from` and `to` clauses per constructor.
        let mut from_clauses: Vec<TClause> = Vec::new();
        let mut to_clauses: Vec<TClause> = Vec::new();

        for (ci, cr) in cons.iter().enumerate() {
            let fc = cr.field_tys.len();
            let params: Vec<String> = (0..fc).map(|i| format!("_g{}", i)).collect();

            // ---- from: pattern `Con p0 p1 …`, body the wrapped rep value ----
            let from_pat = TPattern::Constructor {
                name: cr.con_key.clone(),
                args: params.iter().enumerate()
                    .map(|(i, nm)| TPattern::Var(nm.clone(), cr.field_tys[i].clone()))
                    .collect(),
            };
            // product value
            let prod_val = if fc == 0 {
                con_e("U1", u1_t.clone())
            } else {
                let fields: Vec<TExpr> = params.iter().enumerate().map(|(i, nm)| {
                    let fty = cr.field_tys[i].clone();
                    let k1v = app_e(
                        con_e("K1", Ty::arrow(fty.clone(), k1_t(fty.clone()))),
                        TExpr::new(TExprKind::Var(nm.clone()), fty.clone()),
                        k1_t(fty.clone()));
                    let s1ty = s1_t(Ty::Con(cr.s_markers[i].clone()), k1_t(fty.clone()));
                    app_e(con_e("S1", Ty::arrow(k1v.ty.clone(), s1ty.clone())), k1v, s1ty)
                }).collect();
                fields.into_iter().rev().reduce(|acc, v| {
                    let pty = prod_t(v.ty.clone(), acc.ty.clone());
                    app_e(app_e(con_e("Prod", Ty::arrow(v.ty.clone(), Ty::arrow(acc.ty.clone(), pty.clone()))), v, Ty::arrow(acc.ty.clone(), pty.clone())), acc, pty)
                }).unwrap()
            };
            let c1_val = app_e(
                con_e("C1", Ty::arrow(prod_val.ty.clone(), cr.rep_ty.clone())),
                prod_val, cr.rep_ty.clone());
            // inject into the sum
            let injected = Self::inject_sum(ci, n, c1_val, &suffix, &con_e, &app_e);
            let from_body = app_e(
                con_e("D1", Ty::arrow(injected.ty.clone(), rep_type.clone())),
                injected, rep_type.clone());
            from_clauses.push(TClause {
                span: None, patterns: vec![from_pat], guards: vec![],
                body: Some(from_body), where_binds: vec![],
            });

            // ---- to: pattern the wrapped rep, body `Con p0 p1 …` ----
            let prod_pat = if fc == 0 {
                TPattern::Constructor { name: "U1".into(), args: vec![] }
            } else {
                let field_pats: Vec<TPattern> = params.iter().map(|nm| {
                    TPattern::Constructor { name: "S1".into(), args: vec![
                        TPattern::Constructor { name: "K1".into(), args: vec![
                            TPattern::Var(nm.clone(), Ty::Unit),
                        ]},
                    ]}
                }).collect();
                field_pats.into_iter().rev().reduce(|acc, p|
                    TPattern::Constructor { name: "Prod".into(), args: vec![p, acc] }
                ).unwrap()
            };
            let c1_pat = TPattern::Constructor { name: "C1".into(), args: vec![prod_pat] };
            let inj_pat = Self::inject_sum_pat(ci, n, c1_pat);
            let to_pat = TPattern::Constructor { name: "D1".into(), args: vec![inj_pat] };
            // body: Con p0 p1 …
            let mut to_body = TExpr::new(TExprKind::Con(cr.con_key.clone()),
                Ty::fun(&cr.field_tys, result_type.clone()));
            for (i, nm) in params.iter().enumerate() {
                let rest_ty = to_body.ty.clone();
                let res_ty = match &rest_ty { Ty::Arrow(_, r, _) => (**r).clone(), _ => result_type.clone() };
                to_body = app_e(to_body, TExpr::new(TExprKind::Var(nm.clone()), cr.field_tys[i].clone()), res_ty);
            }
            to_clauses.push(TClause {
                span: None, patterns: vec![to_pat], guards: vec![],
                body: Some(to_body), where_binds: vec![],
            });
        }

        let from_fn = format!("__generic_from_{}", type_name);
        let to_fn = format!("__generic_to_{}", type_name);
        out_fns.push(TFunction {
            name: from_fn.clone(),
            ty: Ty::arrow(result_type.clone(), rep_type.clone()),
            clauses: from_clauses, specialized: false, dict_params: vec![], derived_strict: false,
        });
        out_fns.push(TFunction {
            name: to_fn.clone(),
            ty: Ty::arrow(rep_type.clone(), result_type.clone()),
            clauses: to_clauses, specialized: false, dict_params: vec![], derived_strict: false,
        });
        self.register_instance(InstanceInfo {
            class_name: "Generic".to_string(),
            target_type: result_type,
            method_fns: HashMap::from([
                ("from".to_string(), from_fn),
                ("to".to_string(), to_fn),
            ]),
            context: None,
        });

        out_fns
    }

    /// Inject a constructor's `C1` value at position `ci` of `n` into the sum
    /// spine: `L1`/`R1` nesting matching `derive_generic`'s right-nested sum
    /// (`rep0 :+: rep1 :+: …`). The last constructor is all `R1`s (no `L1`).
    fn inject_sum(
        ci: usize, n: usize, mut v: TExpr, suffix: &[Ty],
        con_e: &dyn Fn(&str, Ty) -> TExpr,
        app_e: &dyn Fn(TExpr, TExpr, Ty) -> TExpr,
    ) -> TExpr {
        if n == 1 {
            return v;
        }
        let sum_t = |a: Ty, b: Ty| Ty::app(Ty::app(Ty::Con(":+:".into()), a), b);
        if ci < n - 1 {
            // L1 into suffix[ci], then R1 up through suffix[ci-1] … suffix[0].
            let sty = suffix[ci].clone();
            v = app_e(con_e("L1", Ty::arrow(v.ty.clone(), sty.clone())), v, sty);
            for j in (0..ci).rev() {
                let ty = suffix[j].clone();
                v = app_e(con_e("R1", Ty::arrow(v.ty.clone(), ty.clone())), v, ty);
            }
        } else {
            // Last constructor: R1 through suffix[n-2] … suffix[0].
            for j in (0..n - 1).rev() {
                let ty = suffix[j].clone();
                v = app_e(con_e("R1", Ty::arrow(v.ty.clone(), ty.clone())), v, ty);
            }
        }
        let _ = sum_t; // types come from `suffix`; helper kept for clarity
        v
    }

    /// The `to`-side pattern mirror of `inject_sum`.
    fn inject_sum_pat(ci: usize, n: usize, inner: TPattern) -> TPattern {
        if n == 1 {
            return inner;
        }
        let mut p = inner;
        if ci < n - 1 {
            p = TPattern::Constructor { name: "L1".into(), args: vec![p] };
            for _ in 0..ci {
                p = TPattern::Constructor { name: "R1".into(), args: vec![p] };
            }
        } else {
            for _ in 0..n - 1 {
                p = TPattern::Constructor { name: "R1".into(), args: vec![p] };
            }
        }
        p
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
        TExpr::new(TExprKind::Lit(TLiteral::Str(s.as_bytes().to_vec())), Ty::Con("String".into()))
    }

    pub(super) fn jx_int(i: i64) -> TExpr {
        TExpr::new(TExprKind::Lit(TLiteral::Integer(i)), Ty::Con("Int".into()))
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
                Ty::Arrow(_, b, _) => (**b).clone(),
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
                    body: Some(rethrow),
                },
                TCaseBranch {
                    pattern: TPattern::Constructor {
                        name: "Right".into(),
                        args: vec![TPattern::Var(ok_name.to_string(), ok_ty)],
                    },
                    guards: vec![],
                    body: Some(ok_body),
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
    /// for one field of a FromJSON-derived constructor. See
    /// `json_field_codec` — the decoder and the encoder are ONE resolution
    /// over the direction table, so everything the derived decoder can read
    /// the derived encoder writes, and vice versa.
    pub(super) fn fromjson_field_decoder(&self, field_ty: &Ty) -> Result<TExpr, (String, String)> {
        self.json_field_codec(JsonDir::Decode, field_ty)
    }

    /// Build the encoder expression (of type `field_ty -> Json`) for one
    /// field of a ToJSON-derived constructor (see `json_field_codec`).
    pub(super) fn tojson_field_encoder(&self, field_ty: &Ty) -> Result<TExpr, (String, String)> {
        self.json_field_codec(JsonDir::Encode, field_ty)
    }

    /// The codec expression for one field of a derived JSON instance, in
    /// either direction. Resolution is STRUCTURAL at derive time (mirroring
    /// derive_functor's fmap resolution): mata-ll cannot register class
    /// instances on library/container types, so primitives use the
    /// fromJSON*/toJSON* combinators, `[t]` and `Maybe t` route through the
    /// List/Maybe combinators, and a field of another FromJSON/ToJSON type
    /// calls that type's own `fromJSON_T`/`toJSON_T` codec — including self-
    /// and mutually-recursive types, via the prescan sets. `Err` carries
    /// (reason, note) for the rejection message. The two directions once
    /// lived as a 55-line pair differing only in the direction's words.
    fn json_field_codec(&self, dir: JsonDir, field_ty: &Ty) -> Result<TExpr, (String, String)> {
        let codec_ty = dir.codec_ty(field_ty);
        let comb = |name: &str, ty: Ty| Self::jx_var(&format!("{}{}", dir.prefix(), name), ty);
        match field_ty {
            Ty::Con(n) if n == "Int" => Ok(comb("Int", codec_ty)),
            Ty::Con(n) if n == "Integer" => Ok(comb("Integer", codec_ty)),
            Ty::Con(n) if n == "Number" => Ok(comb("Number", codec_ty)),
            Ty::Con(n) if n == "String" => Ok(comb("String", codec_ty)),
            Ty::Con(n) if n == "Bool" => Ok(comb("Bool", codec_ty)),
            Ty::Con(n) if n == "Json" => Ok(comb("Value", codec_ty)),
            Ty::List(elem) => {
                let inner = self.json_field_codec(dir, elem)?;
                let list_fn_ty = Ty::arrow(inner.ty.clone(), codec_ty.clone());
                Ok(Self::jx_app(comb("List", list_fn_ty), inner, codec_ty))
            }
            _ if Self::ty_maybe_inner(field_ty).is_some() => {
                let inner_ty = Self::ty_maybe_inner(field_ty).unwrap();
                let inner = self.json_field_codec(dir, inner_ty)?;
                let maybe_fn_ty = Ty::arrow(inner.ty.clone(), codec_ty.clone());
                Ok(Self::jx_app(comb("Maybe", maybe_fn_ty), inner, codec_ty))
            }
            Ty::Con(n) => {
                let derived_here = match dir {
                    JsonDir::Decode => self.fromjson_types.contains(n),
                    JsonDir::Encode => self.tojson_types.contains(n),
                };
                if derived_here
                    || self.instances.contains_key(&(dir.class().to_string(), InstHead::Con(n.clone()))) {
                    Ok(Self::jx_var(&format!("{}_{}", dir.prefix(), n), codec_ty))
                } else {
                    Err((
                        format!("the type '{}' has no {} instance", n, dir.class()),
                        format!("every field of a derived {} needs its own {}; add `deriving ({})` to '{}' or write `instance {} {}` in the module that defines it.",
                            dir.noun(), dir.noun(), dir.class(), n, dir.class(), n),
                    ))
                }
            }
            Ty::Arrow(..) => Err((
                "it is a function type".to_string(),
                format!("a function has no JSON representation, so no {} can {} one; store data instead of behavior, or write the {} instance by hand for an encoding you define.",
                    dir.noun(), dir.produce(), dir.class()),
            )),
            Ty::Tuple(_) => Err((
                format!("the tuple type '{}' has no JSON {} convention in mata-ll", field_ty, dir.gerund()),
                format!("GHC's aeson {}; mata-ll does not — wrap the tuple in a small record type deriving ({}), which also gives the components names in the JSON.",
                    dir.tuple_convention(), dir.class()),
            )),
            Ty::App(..) => Err((
                format!("the type '{}' is parameterized", field_ty),
                format!("a derived {} resolves one concrete {} per field at compile time, and mata-ll instances cannot cover a parameterized type at every instantiation; wrap the concrete instantiation in its own data type deriving ({}).",
                    dir.noun(), dir.noun(), dir.class()),
            )),
            Ty::IO(_) | Ty::LuaIO(..) => Err((
                "it is an effectful action type".to_string(),
                "an IO action has no JSON representation.".to_string(),
            )),
            Ty::Var(v) => Err((
                format!("its type is the type parameter '{}'", v.name),
                format!("a type parameter has no {} the compiler can pick at derive time.", dir.noun()),
            )),
            other => Err((
                format!("the type '{}' cannot be {}", other, dir.past()),
                format!("derived {} supports Int, Number, String, Bool, Json, lists, Maybe, and types that themselves have a {} instance.",
                    dir.class(), dir.class()),
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
    /// finishing with `Right (Con _f0 …)`. `json_name` is the name runtime
    /// decode errors use for the constructor — the effective TAG in a tagged
    /// type (that is what the document contains), the source name in an
    /// untagged one (where the two never differ: a rename on an untagged
    /// constructor is rejected). Derive-time errors keep the source name.
    pub(super) fn fromjson_positional_chain(
        &self,
        con_name: &str,
        json_name: &str,
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
                    Self::jx_str(json_name),
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
                        &con.name, con.effective_tag(), field_tys,
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
                    let chain = self.fromjson_positional_chain(&con.name, con.effective_tag(), field_tys, elems, estr, result_ty)?;
                    let arrn = Self::jx_call(
                        "jExpectArrN",
                        vec![
                            Self::jx_str(con.effective_tag()),
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
    ///
    /// Also validates the effective TAGS (the constructor-level `as "name"`
    /// rename when present, the constructor name otherwise): in a tagged
    /// type each must be non-empty and no two constructors may share one —
    /// the tag is the only thing the decoder has to tell constructors apart,
    /// so a shared tag would make every decode of it ambiguous (and two
    /// values encode identically). This also catches a rename colliding with
    /// another constructor's unrenamed source name. In an UNTAGGED type
    /// (single non-nullary constructor) no tag ever appears in the JSON, so
    /// a rename there is rejected rather than silently ignored.
    ///
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
            checker.reject_derive(class, type_name, &reason, note);
        };
        if tagged {
            let mut seen_tags: HashMap<&str, &str> = HashMap::new();
            for con in constructors {
                let tag = con.effective_tag();
                if tag.is_empty() {
                    reject(self,
                        format!("constructor '{}' renames its JSON tag to the empty string", con.name),
                        "the tag is the string the codec writes and reads to tell the constructors apart, and an empty one identifies nothing; give `as` a non-empty string.");
                    return false;
                }
                if let Some(prev) = seen_tags.insert(tag, &con.name) {
                    reject(self,
                        format!("constructors '{}' and '{}' both map to the JSON tag \"{}\"", prev, con.name, tag),
                        "the tag is the only thing the decoder has to tell the constructors apart, so two constructors sharing one would encode identically and decode ambiguously; rename one with `as \"otherName\"`.");
                    return false;
                }
            }
        } else {
            for con in constructors {
                if let Some(ext) = &con.external_name {
                    reject(self,
                        format!("constructor '{}' is renamed with `as \"{}\"`, but a single-constructor type encodes untagged", con.name, ext),
                        "a lone non-nullary constructor encodes as its bare contents — an object of its record fields, or its positional argument(s) — with no tag anywhere in the JSON, so there is nothing the rename could apply to; drop the rename (only the constructors of a multi-constructor type, or a lone nullary constructor, carry a tag).");
                    return false;
                }
            }
        }
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

    /// The checks every derived JSON instance shares — the class and its
    /// combinators in scope, no type parameters, no GADT/existential
    /// constructors, valid keys/tags — reporting the first failure through
    /// reject_derive. Returns whether the encoding is TAGGED (a multi-
    /// constructor type, or a lone nullary constructor whose NAME is the
    /// payload; the two directions must agree on this) and each
    /// constructor's registered field types.
    fn json_derive_preamble(
        &mut self,
        dir: JsonDir,
        type_name: &str,
        type_vars: &[String],
        constructors: &[Constructor],
    ) -> Option<(bool, Vec<Vec<Ty>>)> {
        let class = dir.class();
        // The class and every combinator the generated codec calls live in
        // the JSON library module; without the import nothing can resolve.
        if !self.classes.contains_key(class) || self.env.lookup(dir.scope_probe()).is_none() {
            self.reject_derive(class, type_name,
                &format!("the {} class and its {} combinators are not in scope", class, dir.noun()),
                &format!("the {} class and the codec combinators the derived {} calls ({}) live in the JSON library module; add `import JSON` at the top of this file.",
                    class, dir.noun(), dir.scope_examples()));
            return None;
        }
        if !type_vars.is_empty() {
            self.reject_derive(class, type_name,
                &format!("'{}' has type parameters", type_name),
                &format!("a derived {n} must pick one concrete {n} per field at compile time, and a field whose type is a type parameter has none. GHC's aeson handles this with a `{c} a` constraint on the instance; mata-ll does not derive constrained codecs, so derive {c} for concrete types only (wrap each instantiation you need in its own data type).",
                    n = dir.noun(), c = class));
            return None;
        }
        for con in constructors {
            if con.gadt_type.is_some() || !con.existential_vars.is_empty() {
                self.reject_derive(class, type_name,
                    &format!("constructor '{}' is a GADT / existential constructor", con.name),
                    &format!("{} must name every field's type to choose how to {} it, which GADT and existential constructors do not allow.",
                        dir.a_noun(), dir.verb()));
                return None;
            }
        }
        let single_nullary = constructors.len() == 1 && self.derived_is_nullary(&constructors[0]);
        let tagged = constructors.len() > 1 || single_nullary;
        if !self.validate_json_keys(class, type_name, constructors, tagged) {
            return None;
        }
        // Constructor field types as registered in pass 1.
        let con_field_tys: Vec<Vec<Ty>> =
            constructors.iter().map(|con| self.derived_field_tys(con)).collect();
        Some((tagged, con_field_tys))
    }

    /// Generate `fromJSON` for a data type: `fromJSON_T :: Json -> Either String T`
    /// built from the JSON module's decoder combinators, plus the FromJSON
    /// instance registration.
    ///
    /// Decoding convention (mirrors aeson's defaultOptions where mata-ll can):
    /// - a single-constructor record decodes from an object keyed by the
    ///   fields' effective keys (the `as "key"` rename when present, the
    ///   Haskell field name otherwise): `data P = P { x :: Int }` ⇐ `{"x":1}`
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
            checker.reject_derive("FromJSON", type_name, &reason, note);
        };
        let Some((tagged, con_field_tys)) =
            self.json_derive_preamble(JsonDir::Decode, type_name, type_vars, constructors)
        else {
            return vec![];
        };

        let result_ty = Ty::Con(type_name.to_string());
        let estr = Self::estr_ty(&result_ty);
        let json = Self::json_ty();
        let str_ty = Ty::Con("String".into());
        let bool_ty = Ty::Con("Bool".into());
        let mangled = format!("fromJSON_{}", type_name);

        let body_inner: TExpr = if tagged {
            // "'A', 'B' or 'C'" for the unknown-tag message — the effective
            // tags (the `as "name"` rename when present): a runtime decode
            // error names what the document must contain, and the document
            // carries the external tag, not the source constructor name.
            let names: Vec<String> = constructors.iter().map(|c| format!("'{}'", c.effective_tag())).collect();
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
                    // JSON tag strings are the effective tags (the `as
                    // "name"` rename when present, the source name otherwise).
                    Self::jx_ok_con(self.resolve_con_name(&con.name), &[], &result_ty, &estr)
                } else {
                    Self::jx_call("jTagNeedsObject", vec![Self::jx_str(con.effective_tag())], estr.clone())
                };
                str_chain = TExpr::new(TExprKind::If {
                    cond: Box::new(TExpr::new(TExprKind::InfixApp {
                        op: "==".into(),
                        lhs: Box::new(Self::jx_var("_s", str_ty.clone())),
                        rhs: Box::new(Self::jx_str(con.effective_tag())),
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
                        rhs: Box::new(Self::jx_str(con.effective_tag())),
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
                        body: Some(str_chain),
                    },
                    TCaseBranch {
                        pattern: TPattern::Constructor {
                            name: "JObj".into(),
                            args: vec![TPattern::Wildcard],
                        },
                        guards: vec![],
                        body: Some(obj_body),
                    },
                    TCaseBranch {
                        pattern: TPattern::Wildcard,
                        guards: vec![],
                        body: Some(Self::jx_call(
                            "jExpectTagged",
                            vec![Self::jx_var("_j", json.clone())],
                            estr.clone(),
                        )),
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
                        &con.name, &con.name, ftys,
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
                    self.fromjson_positional_chain(&con.name, &con.name, ftys, elems, &estr, &result_ty)
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

        // The class's defaulted fromJSONField, specialized to this type's
        // decoder. register_instance bypasses the default-method fill that
        // source instances get, so the derive must provide it itself — the
        // generic decoder (genericFromJSON) reads every record field through
        // fromJSONField, including fields of natively-derived types.
        let field_fn = format!("fromJSONField_{}", type_name);
        let dec_ty = Ty::arrow(json.clone(), estr.clone());
        let field_body = Self::jx_call(
            "jFieldWith",
            vec![
                Self::jx_var(&mangled, dec_ty),
                Self::jx_var("_k", str_ty.clone()),
                Self::jx_var("_j", json.clone()),
            ],
            estr.clone(),
        );

        // Register the instance
        let mut method_fns = HashMap::new();
        method_fns.insert("fromJSON".to_string(), mangled.clone());
        method_fns.insert("fromJSONField".to_string(), field_fn.clone());
        self.register_instance(InstanceInfo {
            class_name: "FromJSON".to_string(),
            target_type: result_ty.clone(),
            method_fns,
            context: None,
        });

        vec![TFunction {
            name: mangled,
            ty: Ty::arrow(json.clone(), estr.clone()),
            clauses: vec![TClause {
                span: None,
                patterns: vec![TPattern::Var("_j".into(), Self::json_ty())],
                guards: vec![],
                body: Some(body),
                where_binds: vec![],
            }],
            specialized: false,
            dict_params: vec![],
            derived_strict: false,
        },
        TFunction {
            name: field_fn,
            ty: Ty::arrow(str_ty, Ty::arrow(json, estr.clone())),
            clauses: vec![TClause {
                span: None,
                patterns: vec![
                    TPattern::Var("_k".into(), Ty::Con("String".into())),
                    TPattern::Var("_j".into(), Self::json_ty()),
                ],
                guards: vec![],
                body: Some(field_body),
                where_binds: vec![],
            }],
            specialized: false,
            dict_params: vec![],
            derived_strict: false,
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
    ///
    /// Every emitted tag string is the constructor's effective tag (the
    /// `as "name"` rename when present, the source name otherwise) — the
    /// same string the derived decoder dispatches on. Derive-time error
    /// messages keep the source name.
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
                    pairs.insert(0, Self::jx_pair("tag", Self::jx_jstr(con.effective_tag())));
                }
                Ok(Self::jx_obj(pairs))
            }
            _ if field_tys.is_empty() => Ok(Self::jx_jstr(con.effective_tag())),
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
                        Self::jx_pair("tag", Self::jx_jstr(con.effective_tag())),
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
    ///   Haskell field name otherwise): `data P = P { x :: Int }` ⇒ `{"x":1}`
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
            checker.reject_derive("ToJSON", type_name, &reason, note);
        };
        let Some((tagged, con_field_tys)) =
            self.json_derive_preamble(JsonDir::Encode, type_name, type_vars, constructors)
        else {
            return vec![];
        };

        let result_ty = Ty::Con(type_name.to_string());
        let json = Self::json_ty();
        let mangled = format!("toJSON_{}", type_name);

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
                        body: Some(body),
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
            derived_strict: false,
        }]
    }
}

/// The two derived JSON codecs differ only in these words and types; the
/// field-codec resolution and the derive preamble are written once over
/// this table.
#[derive(Clone, Copy)]
enum JsonDir {
    Decode,
    Encode,
}

impl JsonDir {
    fn class(self) -> &'static str {
        match self { JsonDir::Decode => "FromJSON", JsonDir::Encode => "ToJSON" }
    }
    /// The combinator/function prefix: `fromJSONInt`, `toJSON_T`, ….
    fn prefix(self) -> &'static str {
        match self { JsonDir::Decode => "fromJSON", JsonDir::Encode => "toJSON" }
    }
    /// A decoder maps `Json -> Either String t`; an encoder `t -> Json`.
    fn codec_ty(self, field_ty: &Ty) -> Ty {
        match self {
            JsonDir::Decode => Ty::arrow(Checker::json_ty(), Checker::estr_ty(field_ty)),
            JsonDir::Encode => Ty::arrow(field_ty.clone(), Checker::json_ty()),
        }
    }
    /// A library name whose presence shows the JSON module is imported.
    fn scope_probe(self) -> &'static str {
        match self { JsonDir::Decode => "jContext", JsonDir::Encode => "toJSONList" }
    }
    fn scope_examples(self) -> &'static str {
        match self {
            JsonDir::Decode => "jContext, jFieldWith, …",
            JsonDir::Encode => "toJSONList, toJSONMaybe, …",
        }
    }
    fn noun(self) -> &'static str {
        match self { JsonDir::Decode => "decoder", JsonDir::Encode => "encoder" }
    }
    fn a_noun(self) -> &'static str {
        match self { JsonDir::Decode => "a decoder", JsonDir::Encode => "an encoder" }
    }
    fn verb(self) -> &'static str {
        match self { JsonDir::Decode => "decode", JsonDir::Encode => "encode" }
    }
    fn gerund(self) -> &'static str {
        match self { JsonDir::Decode => "decoding", JsonDir::Encode => "encoding" }
    }
    fn produce(self) -> &'static str {
        match self { JsonDir::Decode => "produce", JsonDir::Encode => "serialize" }
    }
    fn past(self) -> &'static str {
        match self { JsonDir::Decode => "decoded from JSON", JsonDir::Encode => "encoded to JSON" }
    }
    fn tuple_convention(self) -> &'static str {
        match self {
            JsonDir::Decode => "decodes tuples from fixed-length arrays",
            JsonDir::Encode => "encodes tuples as fixed-length arrays",
        }
    }
}
