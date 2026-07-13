//! Module resolution and loading
//!
//! Each .mll file is a module. Module names map to file paths:
//!   import Data.Tree  =>  Data/Tree.mll
//!
//! When compiling a module, imported .mll files are parsed, type-checked,
//! and their declarations are merged into the current module's environment.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::fs;

use crate::ast::*;
use crate::lexer;
use crate::parser;

/// Resolved module with its declarations
#[derive(Debug)]
pub struct ResolvedModule {
    pub path: PathBuf,
    pub module: Module,
}

/// Module loader
pub struct ModuleLoader {
    /// Search paths for modules
    search_paths: Vec<PathBuf>,
    /// Already loaded (parsed) modules (key -> AST)
    loaded: HashMap<String, Module>,
    /// Already resolved modules (key -> fully resolved module)
    resolved: HashMap<String, Module>,
    /// Modules currently being resolved (cycle detection)
    in_progress: HashSet<String>,
}

impl ModuleLoader {
    pub fn new(source_dir: &Path) -> Self {
        ModuleLoader {
            search_paths: vec![source_dir.to_path_buf()],
            loaded: HashMap::new(),
            resolved: HashMap::new(),
            in_progress: HashSet::new(),
        }
    }

    pub fn add_search_path(&mut self, path: PathBuf) {
        self.search_paths.push(path);
    }

    /// Resolve a module path (e.g., ["Data", "Tree"]) to a file path
    fn resolve_path(&self, module_path: &[String]) -> Option<PathBuf> {
        let relative: PathBuf = module_path.iter().collect();
        let filename = format!("{}.mll", relative.display());

        for search_dir in &self.search_paths {
            let full_path = search_dir.join(&filename);
            if full_path.exists() {
                return Some(full_path);
            }
        }
        None
    }

    /// Load and parse a module, caching the result
    pub fn load_module(&mut self, module_path: &[String]) -> Result<&Module, String> {
        let key = module_path.join(".");

        if self.loaded.contains_key(&key) {
            return Ok(self.loaded.get(&key).unwrap());
        }

        // Filesystem search paths take precedence (so `-L <dir>` can shadow a
        // stdlib module); fall back to the embedded stdlib baked into the crate
        // so an installed compiler needs no `lib/` directory on disk.
        let source = match self.resolve_path(module_path) {
            Some(file_path) => fs::read_to_string(&file_path)
                .map_err(|e| format!("Error reading {}: {}", file_path.display(), e))?,
            None => crate::stdlib::embedded_module(&key)
                .map(str::to_string)
                .ok_or_else(|| format!("Cannot find module '{}'", key))?,
        };

        let tokens = lexer::lex(&source)?;
        // An imported module's syntax errors surface through the import-error
        // channel, prefixed with the module that failed to parse.
        let module = parser::parse(&tokens).map_err(|diags| {
            let msgs: Vec<String> = diags.iter().map(|d| d.to_string()).collect();
            format!("in module '{}': {}", key, msgs.join("\n"))
        })?;

        self.loaded.insert(key.clone(), module);
        Ok(self.loaded.get(&key).unwrap())
    }

    /// Process all imports in a module, returning merged declarations.
    /// The imported declarations are prepended to the module's own declarations.
    pub fn resolve_imports(&mut self, module: &Module) -> Result<Module, String> {
        let mut imported_decls: Vec<Decl> = Vec::new();
        let mut own_decls: Vec<Decl> = Vec::new();
        let mut seen_imports: HashSet<String> = HashSet::new();
        // Aliases introduced by `import qualified X as M`. A use-site `M.foo`
        // parses as the field-access shape `App(Var "foo", Con "M")`; once we
        // know which `Con`s are really module aliases, we rewrite those into a
        // single qualified `Var "M.foo"` that matches the prefixed declaration.
        let qualified_aliases: HashSet<String> = module.decls.iter()
            .filter_map(|d| match d {
                Decl::Import { items: ImportItems::Qualified(alias), .. } => Some(alias.clone()),
                _ => None,
            })
            .collect();
        let mut hidden_names: HashSet<String> = module.hidden.clone();
        // Names explicitly requested by a Specific import. A name can be
        // merged in transitively by one import (and hidden because that import
        // didn't request it) yet be explicitly imported by another; an
        // explicit import must win, so these are subtracted from hidden_names
        // at the end. Without this, `import M (a)` followed by `import L (b)`
        // fails when M transitively pulls in b — b stays hidden forever.
        let mut visible_names: HashSet<String> = HashSet::new();
        // Names hidden because their defining module has an export list that
        // omits them (genuinely private). Unlike selection-hiding, this is
        // never overridden by an explicit import elsewhere.
        let mut private_names: HashSet<String> = HashSet::new();

        for decl in &module.decls {
            match decl {
                Decl::Import { module_path, items } => {
                    let key = module_path.join(".");
                    if seen_imports.contains(&key) {
                        continue;
                    }
                    seen_imports.insert(key.clone());

                    // Recursively resolve imports in the imported module
                    let resolved = if self.resolved.contains_key(&key) {
                        self.resolved.get(&key).unwrap().clone()
                    } else if self.in_progress.contains(&key) {
                        // Cycle: treat as a module with no declarations
                        Module { decls: Vec::new(), exports: None, hidden: HashSet::new() }
                    } else {
                        self.in_progress.insert(key.clone());
                        let imported = self.load_module(module_path)?.clone();
                        let r = self.resolve_imports(&imported)?;
                        self.in_progress.remove(&key);
                        self.resolved.insert(key.clone(), r.clone());
                        r
                    };

                    // Include ALL non-import declarations for compilation
                    // (exported functions may depend on internal helpers).
                    // Track hidden names for typechecker enforcement.
                    let all_decls: Vec<&Decl> = resolved.decls.iter()
                        .filter(|d| !matches!(d, Decl::Import { .. }))
                        .collect();

                    // Compute hidden names: names the module itself defines but
                    // does not export. Only the module's OWN declarations count
                    // — names merged in transitively from its imports are not
                    // "private to" this module just because its export list
                    // omits them, so we look at the loaded (pre-merge) module.
                    let parsed_exports = self.loaded.get(&key).and_then(|m| m.exports.clone());
                    let own_decl_names: Vec<String> = self.loaded.get(&key)
                        .map(|m| m.decls.iter()
                            .filter(|d| !matches!(d, Decl::Import { .. }))
                            .filter_map(decl_name)
                            .collect())
                        .unwrap_or_default();
                    if let Some(ref exports) = parsed_exports {
                        for name in &own_decl_names {
                            if !exports.contains(name) {
                                private_names.insert(name.clone());
                                hidden_names.insert(name.clone());
                            }
                        }
                    }

                    match items {
                        ImportItems::All => {
                            for d in &all_decls {
                                imported_decls.push((*d).clone());
                            }
                        }
                        ImportItems::Specific(items) => {
                            // Include ALL declarations (internal helpers are
                            // needed for type checking), but hide names that
                            // weren't explicitly requested.
                            let wanted: HashSet<String> = items.iter().map(|item| {
                                match item {
                                    ImportItem::Value(n) => n.clone(),
                                    ImportItem::TypeAll(n) => n.clone(),
                                    ImportItem::TypeOnly(n) => n.clone(),
                                }
                            }).collect();

                            for w in &wanted {
                                visible_names.insert(w.clone());
                            }
                            for d in &all_decls {
                                imported_decls.push((*d).clone());
                                if let Some(n) = decl_name(d)
                                    && !wanted.contains(&n) {
                                        hidden_names.insert(n);
                                    }
                            }
                        }
                        ImportItems::Hiding(items) => {
                            let excluded: HashSet<String> = items.iter().map(|item| {
                                match item {
                                    ImportItem::Value(n) => n.clone(),
                                    ImportItem::TypeAll(n) => n.clone(),
                                    ImportItem::TypeOnly(n) => n.clone(),
                                }
                            }).collect();

                            for d in &all_decls {
                                imported_decls.push((*d).clone());
                                if let Some(n) = decl_name(d)
                                    && excluded.contains(&n) {
                                        hidden_names.insert(n);
                                    }
                            }
                        }
                        ImportItems::Qualified(alias) => {
                            // Prefix every declaration to `alias.name` AND rewrite
                            // intra-module references (a sibling function call, a
                            // reference to the module's own types) to the prefixed
                            // names, so the qualified namespace is self-contained
                            // and never collides with the Prelude.
                            let names = collect_module_names(&all_decls);
                            let qual = Qual { alias, names: &names };
                            for d in &all_decls {
                                imported_decls.push(qual.decl(d));
                            }
                        }
                    }
                }
                _ => {
                    let d = if qualified_aliases.is_empty() {
                        decl.clone()
                    } else {
                        rewrite_qualified_uses_decl(decl.clone(), &qualified_aliases)
                    };
                    own_decls.push(d);
                }
            }
        }

        // An explicit import of a name overrides transitive selection-hiding,
        // but never a module's own export-list privacy.
        for v in &visible_names {
            if !private_names.contains(v) {
                hidden_names.remove(v);
            }
        }

        // Merge: imported first, then own
        imported_decls.extend(own_decls);
        Ok(Module { decls: imported_decls, exports: None, hidden: hidden_names })
    }

    /// Flag unqualified imports that redefine an existing name *with an
    /// incompatible type* (against the Prelude, this file, or an earlier
    /// import). Because mata-ll flattens every import into one namespace, such a
    /// clash otherwise surfaces later as a baffling unification error deep inside
    /// the imported module. A matching type shape (an FFI re-declaration like
    /// `sqrt`, or the same definition re-exported through a diamond import) is
    /// harmless and not flagged. Call after `resolve_imports`, which populates
    /// the resolved-module cache this reads. `reserved` maps globally-provided
    /// names (the Prelude's) to their type shapes.
    pub fn check_import_collisions(
        &self,
        module: &Module,
        reserved: &HashMap<String, String>,
    ) -> Result<(), String> {
        // name -> (source label, type shape)
        let mut claimed: HashMap<String, (String, String)> = reserved.iter()
            .map(|(n, shape)| (n.clone(), ("the Prelude".to_string(), shape.clone())))
            .collect();
        for (name, shape) in signature_shapes(&module.decls) {
            claimed.entry(name).or_insert(("this file".to_string(), shape));
        }

        for d in &module.decls {
            let Decl::Import { module_path, items } = d else { continue };
            // Qualified imports are renamed to `Alias.name` and can't collide.
            if matches!(items, ImportItems::Qualified(_)) {
                continue;
            }
            let key = module_path.join(".");
            // Skip if the module didn't resolve (e.g. an import cycle).
            let Some(resolved) = self.resolved.get(&key) else { continue };

            let shapes = signature_shapes(&resolved.decls);
            let mut collisions: Vec<(String, String)> = Vec::new();
            for (name, shape) in &shapes {
                if let Some((src, claimed_shape)) = claimed.get(name)
                    && claimed_shape != shape
                {
                    collisions.push((name.clone(), src.clone()));
                }
            }
            if !collisions.is_empty() {
                collisions.sort();
                return Err(collision_error(&key, &collisions));
            }
            // Later imports compare against this one too.
            for (name, shape) in shapes {
                claimed.entry(name).or_insert((key.clone(), shape));
            }
        }
        Ok(())
    }
}

/// The top-level names a module defines, split by namespace. Used to rewrite
/// intra-module references when the module is imported `qualified`: only names
/// the module actually defines get the alias prefix — references to the Prelude
/// or to intrinsics (`elem`, `zip`, `hmInsert`, …) are left alone.
struct ModuleNames {
    /// Value-level bindings: functions, type signatures, exports.
    vals: HashSet<String>,
    /// Type-level names: data, newtype, alias, type-family.
    tys: HashSet<String>,
}

fn collect_module_names(decls: &[&Decl]) -> ModuleNames {
    let mut vals = HashSet::new();
    let mut tys = HashSet::new();
    for d in decls {
        match d {
            Decl::FunDef { name, .. }
            | Decl::TypeSig { name, .. }
            | Decl::ExportSig { name, .. } => { vals.insert(name.clone()); }
            Decl::DataDef { name, .. } | Decl::NewtypeDef { name, .. }
            | Decl::TypeAlias { name, .. } | Decl::TypeFamily { name, .. } => {
                tys.insert(name.clone());
            }
            _ => {}
        }
    }
    ModuleNames { vals, tys }
}

/// Prefixes a `qualified`-imported module's declarations with its alias and
/// rewrites references among them. Constructors and class/instance names are
/// left global (a qualified `Con` can't be written at a use site anyway), so
/// only value and type references are prefixed.
struct Qual<'a> {
    alias: &'a str,
    names: &'a ModuleNames,
}

impl Qual<'_> {
    fn q(&self, name: &str) -> String {
        format!("{}.{}", self.alias, name)
    }

    fn decl(&self, decl: &Decl) -> Decl {
        match decl {
            Decl::TypeSig { name, ty } => Decl::TypeSig {
                name: self.q(name), ty: self.ty(ty),
            },
            Decl::FunDef { name, clauses } => Decl::FunDef {
                name: self.q(name),
                clauses: clauses.iter().map(|c| self.clause(c)).collect(),
            },
            Decl::ExportSig { name, ty } => Decl::ExportSig {
                name: self.q(name), ty: self.ty(ty),
            },
            Decl::DataDef { name, type_vars, constructors, deriving } => Decl::DataDef {
                name: self.q(name),
                type_vars: type_vars.clone(),
                // Rewrite field types (sibling type references) but leave the
                // constructor names global.
                constructors: constructors.iter().map(|c| self.constructor(c)).collect(),
                deriving: deriving.clone(),
            },
            Decl::NewtypeDef { name, type_vars, inner } => Decl::NewtypeDef {
                name: self.q(name), type_vars: type_vars.clone(), inner: self.ty(inner),
            },
            Decl::TypeAlias { name, params, ty } => Decl::TypeAlias {
                name: self.q(name), params: params.clone(), ty: self.ty(ty),
            },
            Decl::TypeFamily { name, equations } => Decl::TypeFamily {
                name: self.q(name),
                equations: equations.iter().map(|eq| TypeFamilyEq {
                    args: eq.args.iter().map(|t| self.ty(t)).collect(),
                    result: self.ty(&eq.result),
                }).collect(),
            },
            // Classes and instances are global — don't prefix.
            other => other.clone(),
        }
    }

    fn constructor(&self, c: &Constructor) -> Constructor {
        let fields = match &c.fields {
            ConstructorFields::Positional(ts) =>
                ConstructorFields::Positional(ts.iter().map(|t| self.ty(t)).collect()),
            ConstructorFields::Named(fs) =>
                ConstructorFields::Named(fs.iter().map(|f| crate::ast::RecordField {
                    name: f.name.clone(),
                    external_key: f.external_key.clone(),
                    ty: self.ty(&f.ty),
                }).collect()),
        };
        Constructor {
            name: c.name.clone(),
            external_name: c.external_name.clone(),
            fields,
            gadt_type: c.gadt_type.as_ref().map(|t| self.ty(t)),
            existential_vars: c.existential_vars.clone(),
            existential_constraints: c.existential_constraints.clone(),
        }
    }

    fn clause(&self, c: &Clause) -> Clause {
        // Clause parameters and where-bound names shadow module-level names,
        // so references to them must not be prefixed.
        let mut bound = HashSet::new();
        for p in &c.patterns { collect_pattern_vars(p, &mut bound); }
        for ld in &c.where_binds { bound.insert(ld.name.clone()); }
        Clause {
            patterns: c.patterns.clone(),
            guards: c.guards.iter().map(|g| Guard {
                condition: self.expr(&g.condition, &bound),
                body: self.expr(&g.body, &bound),
            }).collect(),
            body: self.expr(&c.body, &bound),
            where_binds: c.where_binds.iter().map(|ld| self.localdef(ld, &bound)).collect(),
            span: c.span,
        }
    }

    fn localdef(&self, ld: &LocalDef, outer: &HashSet<String>) -> LocalDef {
        let mut bound = outer.clone();
        for p in &ld.patterns { collect_pattern_vars(p, &mut bound); }
        LocalDef {
            name: ld.name.clone(),
            patterns: ld.patterns.clone(),
            body: self.expr(&ld.body, &bound),
        }
    }

    fn expr(&self, e: &Expr, bound: &HashSet<String>) -> Expr {
        match e {
            Expr::Var(n) => {
                if !bound.contains(n) && self.names.vals.contains(n) {
                    Expr::Var(self.q(n))
                } else {
                    Expr::Var(n.clone())
                }
            }
            Expr::OpFunc(n) => {
                // Backtick sections `(`f`)` carry a plain function name here.
                if !bound.contains(n) && self.names.vals.contains(n) {
                    Expr::OpFunc(self.q(n))
                } else {
                    Expr::OpFunc(n.clone())
                }
            }
            Expr::Con(_) | Expr::Lit(_) => e.clone(),
            Expr::App(f, x) =>
                Expr::App(Box::new(self.expr(f, bound)), Box::new(self.expr(x, bound))),
            Expr::Lambda { params, body } => {
                let mut b = bound.clone();
                for p in params { b.insert(p.clone()); }
                Expr::Lambda { params: params.clone(), body: Box::new(self.expr(body, &b)) }
            }
            Expr::InfixApp { op, lhs, rhs } => Expr::InfixApp {
                op: op.clone(),
                lhs: Box::new(self.expr(lhs, bound)),
                rhs: Box::new(self.expr(rhs, bound)),
            },
            Expr::Negate(x) => Expr::Negate(Box::new(self.expr(x, bound))),
            Expr::If { cond, then_branch, else_branch } => Expr::If {
                cond: Box::new(self.expr(cond, bound)),
                then_branch: Box::new(self.expr(then_branch, bound)),
                else_branch: Box::new(self.expr(else_branch, bound)),
            },
            Expr::Case { scrutinee, branches } => Expr::Case {
                scrutinee: Box::new(self.expr(scrutinee, bound)),
                branches: branches.iter().map(|br| {
                    let mut b = bound.clone();
                    collect_pattern_vars(&br.pattern, &mut b);
                    CaseBranch {
                        pattern: br.pattern.clone(),
                        guards: br.guards.iter().map(|g| Guard {
                            condition: self.expr(&g.condition, &b),
                            body: self.expr(&g.body, &b),
                        }).collect(),
                        body: self.expr(&br.body, &b),
                    }
                }).collect(),
            },
            Expr::Let { binds, body } => {
                let mut b = bound.clone();
                for ld in binds { b.insert(ld.name.clone()); }
                Expr::Let {
                    binds: binds.iter().map(|ld| self.localdef(ld, &b)).collect(),
                    body: Box::new(self.expr(body, &b)),
                }
            }
            Expr::Do(stmts) => {
                let mut b = bound.clone();
                Expr::Do(stmts.iter().map(|s| self.dostmt(s, &mut b)).collect())
            }
            Expr::Ascription(x, t) =>
                Expr::Ascription(Box::new(self.expr(x, bound)), self.ty(t)),
            Expr::RecordCon { constructor, fields } => Expr::RecordCon {
                constructor: constructor.clone(),
                fields: fields.iter().map(|(n, fe)| (n.clone(), self.expr(fe, bound))).collect(),
            },
            Expr::RecordUpdate { expr, updates } => Expr::RecordUpdate {
                expr: Box::new(self.expr(expr, bound)),
                updates: updates.iter().map(|(n, fe)| (n.clone(), self.expr(fe, bound))).collect(),
            },
            Expr::Paren(x) => Expr::Paren(Box::new(self.expr(x, bound))),
            Expr::Tuple(xs) => Expr::Tuple(xs.iter().map(|x| self.expr(x, bound)).collect()),
        }
    }

    fn dostmt(&self, s: &DoStmt, bound: &mut HashSet<String>) -> DoStmt {
        match s {
            DoStmt::Bind { name, expr } => {
                let e = self.expr(expr, bound);
                bound.insert(name.clone());
                DoStmt::Bind { name: name.clone(), expr: e }
            }
            DoStmt::Expr(e) => DoStmt::Expr(self.expr(e, bound)),
            DoStmt::DoLet { binds } => {
                // Recursive group: every name is in scope for every body and for
                // the rest of the do-block, so bind them all before rewriting.
                for ld in binds { bound.insert(ld.name.clone()); }
                let binds = binds.iter().map(|ld| self.localdef(ld, bound)).collect();
                DoStmt::DoLet { binds }
            }
            DoStmt::PatternBind { pattern, expr } => {
                let e = self.expr(expr, bound);
                collect_pattern_vars(pattern, bound);
                DoStmt::PatternBind { pattern: pattern.clone(), expr: e }
            }
            DoStmt::PatternDoLet { pattern, expr } => {
                let e = self.expr(expr, bound);
                collect_pattern_vars(pattern, bound);
                DoStmt::PatternDoLet { pattern: pattern.clone(), expr: e }
            }
        }
    }

    fn ty(&self, t: &Type) -> Type {
        match t {
            Type::Con(n) => {
                if self.names.tys.contains(n) { Type::Con(self.q(n)) } else { Type::Con(n.clone()) }
            }
            Type::Var(_) | Type::Unit | Type::Promoted(_) => t.clone(),
            Type::App(a, b) => Type::App(Box::new(self.ty(a)), Box::new(self.ty(b))),
            Type::Arrow(a, b) => Type::Arrow(Box::new(self.ty(a)), Box::new(self.ty(b))),
            Type::List(x) => Type::List(Box::new(self.ty(x))),
            Type::IO(x) => Type::IO(Box::new(self.ty(x))),
            Type::ScopedLuaIO { scope_var, inner } => Type::ScopedLuaIO {
                scope_var: scope_var.clone(), inner: Box::new(self.ty(inner)),
            },
            Type::Forall { var, inner } => Type::Forall {
                var: var.clone(), inner: Box::new(self.ty(inner)),
            },
            Type::Paren(x) => Type::Paren(Box::new(self.ty(x))),
            Type::LuaPure { lua_name, result } => Type::LuaPure {
                lua_name: lua_name.clone(), result: Box::new(self.ty(result)),
            },
            Type::LuaIO { lua_name, result } => Type::LuaIO {
                lua_name: lua_name.clone(), result: Box::new(self.ty(result)),
            },
            Type::LuaIterator { lua_name, result } => Type::LuaIterator {
                lua_name: lua_name.clone(), result: Box::new(self.ty(result)),
            },
            Type::LuaTry { lua_name, result } => Type::LuaTry {
                lua_name: lua_name.clone(), result: Box::new(self.ty(result)),
            },
            Type::LuaCatch { lua_name, result } => Type::LuaCatch {
                lua_name: lua_name.clone(), result: Box::new(self.ty(result)),
            },
            Type::LuaIOCatch { lua_name, result } => Type::LuaIOCatch {
                lua_name: lua_name.clone(), result: Box::new(self.ty(result)),
            },
            Type::Tuple(xs) => Type::Tuple(xs.iter().map(|x| self.ty(x)).collect()),
            Type::Constrained { constraints, ty } => Type::Constrained {
                constraints: constraints.iter().map(|c| Constraint {
                    class_name: c.class_name.clone(), type_arg: self.ty(&c.type_arg),
                }).collect(),
                ty: Box::new(self.ty(ty)),
            },
        }
    }
}

/// Collect the variable names bound by a pattern (for scope tracking).
fn collect_pattern_vars(p: &Pattern, out: &mut HashSet<String>) {
    match p {
        Pattern::Var(n) => { out.insert(n.clone()); }
        Pattern::Constructor { args, .. } => {
            for a in args { collect_pattern_vars(a, out); }
        }
        Pattern::Paren(inner) => collect_pattern_vars(inner, out),
        Pattern::Tuple(ps) => for p in ps { collect_pattern_vars(p, out); },
        Pattern::Wildcard | Pattern::LitPat(_) => {}
    }
}

/// Rewrite qualified use-sites in one of the importing module's declarations.
/// `M.foo` parsed as the field-access shape `App(Var "foo", Con "M")`; where
/// `M` is a known qualified alias, collapse it to `Var "M.foo"`.
fn rewrite_qualified_uses_decl(decl: Decl, aliases: &HashSet<String>) -> Decl {
    match decl {
        Decl::FunDef { name, clauses } => Decl::FunDef {
            name,
            clauses: clauses.into_iter().map(|c| Clause {
                patterns: c.patterns,
                guards: c.guards.into_iter().map(|g| Guard {
                    condition: rewrite_uses_expr(g.condition, aliases),
                    body: rewrite_uses_expr(g.body, aliases),
                }).collect(),
                body: rewrite_uses_expr(c.body, aliases),
                where_binds: c.where_binds.into_iter()
                    .map(|ld| rewrite_uses_localdef(ld, aliases)).collect(),
                span: c.span,
            }).collect(),
        },
        // Instance method bodies can also reference qualified imports.
        Decl::InstanceDecl { class_name, target_type, context, methods } => Decl::InstanceDecl {
            class_name, target_type, context,
            methods: methods.into_iter().map(|m| InstanceMethod {
                name: m.name,
                clauses: m.clauses.into_iter().map(|c| Clause {
                    patterns: c.patterns,
                    guards: c.guards.into_iter().map(|g| Guard {
                        condition: rewrite_uses_expr(g.condition, aliases),
                        body: rewrite_uses_expr(g.body, aliases),
                    }).collect(),
                    body: rewrite_uses_expr(c.body, aliases),
                    where_binds: c.where_binds.into_iter()
                        .map(|ld| rewrite_uses_localdef(ld, aliases)).collect(),
                    span: c.span,
                }).collect(),
            }).collect(),
        },
        other => other,
    }
}

fn rewrite_uses_localdef(ld: LocalDef, aliases: &HashSet<String>) -> LocalDef {
    LocalDef {
        name: ld.name,
        patterns: ld.patterns,
        body: rewrite_uses_expr(ld.body, aliases),
    }
}

fn rewrite_uses_expr(e: Expr, aliases: &HashSet<String>) -> Expr {
    // Recurse first (post-order), then collapse the field-access shape.
    let e = match e {
        Expr::App(f, x) => Expr::App(
            Box::new(rewrite_uses_expr(*f, aliases)),
            Box::new(rewrite_uses_expr(*x, aliases)),
        ),
        Expr::Lambda { params, body } => Expr::Lambda {
            params, body: Box::new(rewrite_uses_expr(*body, aliases)),
        },
        Expr::InfixApp { op, lhs, rhs } => Expr::InfixApp {
            op,
            lhs: Box::new(rewrite_uses_expr(*lhs, aliases)),
            rhs: Box::new(rewrite_uses_expr(*rhs, aliases)),
        },
        Expr::Negate(x) => Expr::Negate(Box::new(rewrite_uses_expr(*x, aliases))),
        Expr::If { cond, then_branch, else_branch } => Expr::If {
            cond: Box::new(rewrite_uses_expr(*cond, aliases)),
            then_branch: Box::new(rewrite_uses_expr(*then_branch, aliases)),
            else_branch: Box::new(rewrite_uses_expr(*else_branch, aliases)),
        },
        Expr::Case { scrutinee, branches } => Expr::Case {
            scrutinee: Box::new(rewrite_uses_expr(*scrutinee, aliases)),
            branches: branches.into_iter().map(|br| CaseBranch {
                pattern: br.pattern,
                guards: br.guards.into_iter().map(|g| Guard {
                    condition: rewrite_uses_expr(g.condition, aliases),
                    body: rewrite_uses_expr(g.body, aliases),
                }).collect(),
                body: rewrite_uses_expr(br.body, aliases),
            }).collect(),
        },
        Expr::Let { binds, body } => Expr::Let {
            binds: binds.into_iter().map(|ld| rewrite_uses_localdef(ld, aliases)).collect(),
            body: Box::new(rewrite_uses_expr(*body, aliases)),
        },
        Expr::Do(stmts) => Expr::Do(stmts.into_iter().map(|s| match s {
            DoStmt::Bind { name, expr } => DoStmt::Bind { name, expr: rewrite_uses_expr(expr, aliases) },
            DoStmt::Expr(e) => DoStmt::Expr(rewrite_uses_expr(e, aliases)),
            DoStmt::DoLet { binds } => DoStmt::DoLet {
                binds: binds.into_iter().map(|ld| rewrite_uses_localdef(ld, aliases)).collect(),
            },
            DoStmt::PatternBind { pattern, expr } => DoStmt::PatternBind { pattern, expr: rewrite_uses_expr(expr, aliases) },
            DoStmt::PatternDoLet { pattern, expr } => DoStmt::PatternDoLet { pattern, expr: rewrite_uses_expr(expr, aliases) },
        }).collect()),
        Expr::Ascription(x, t) => Expr::Ascription(Box::new(rewrite_uses_expr(*x, aliases)), t),
        Expr::RecordCon { constructor, fields } => Expr::RecordCon {
            constructor,
            fields: fields.into_iter().map(|(n, e)| (n, rewrite_uses_expr(e, aliases))).collect(),
        },
        Expr::RecordUpdate { expr, updates } => Expr::RecordUpdate {
            expr: Box::new(rewrite_uses_expr(*expr, aliases)),
            updates: updates.into_iter().map(|(n, e)| (n, rewrite_uses_expr(e, aliases))).collect(),
        },
        Expr::Paren(x) => Expr::Paren(Box::new(rewrite_uses_expr(*x, aliases))),
        Expr::Tuple(xs) => Expr::Tuple(xs.into_iter().map(|x| rewrite_uses_expr(x, aliases)).collect()),
        other => other,
    };
    // `App(Var field, Con alias)` with a known alias is a qualified reference.
    if let Expr::App(f, x) = &e
        && let Expr::Var(field) = f.as_ref()
        && let Expr::Con(name) = x.as_ref()
        && aliases.contains(name)
    {
        return Expr::Var(format!("{}.{}", name, field));
    }
    e
}

/// A structural fingerprint of a type, with variable names erased. Two
/// signatures with the same shape are compatible enough to coexist as one
/// merged definition; differing shapes are the real, breaking clash (e.g.
/// `[a] -> Bool` vs `HashMap k v -> Bool` for two `null`s). FFI re-declarations
/// (`sqrt`) and re-exports (diamond imports) share a shape, so they don't flag.
fn type_shape(ty: &Type) -> String {
    match ty {
        Type::Con(n) => n.clone(),
        Type::Var(_) => "_".to_string(),
        Type::App(a, b) => format!("({} {})", type_shape(a), type_shape(b)),
        Type::Arrow(a, b) => format!("({}->{})", type_shape(a), type_shape(b)),
        Type::List(x) => format!("[{}]", type_shape(x)),
        Type::IO(x) => format!("IO({})", type_shape(x)),
        Type::ScopedLuaIO { inner, .. } => format!("LuaIO(_,{})", type_shape(inner)),
        Type::Forall { inner, .. } => type_shape(inner),
        Type::Unit => "()".to_string(),
        Type::Paren(x) => type_shape(x),
        Type::LuaPure { lua_name, result } => format!("LuaPure({},{})", lua_name, type_shape(result)),
        Type::LuaIO { lua_name, result } => format!("LuaIO({},{})", lua_name, type_shape(result)),
        Type::LuaIterator { lua_name, result } => format!("LuaIterator({},{})", lua_name, type_shape(result)),
        Type::LuaTry { lua_name, result } => format!("LuaTry({},{})", lua_name, type_shape(result)),
        Type::LuaCatch { lua_name, result } => format!("LuaCatch({},{})", lua_name, type_shape(result)),
        Type::LuaIOCatch { lua_name, result } => format!("LuaIOCatch({},{})", lua_name, type_shape(result)),
        Type::Tuple(xs) => format!("({})", xs.iter().map(type_shape).collect::<Vec<_>>().join(",")),
        Type::Constrained { ty, .. } => type_shape(ty),
        Type::Promoted(n) => format!("'{}", n),
    }
}

/// Map each value-level name a group of declarations signs to its type shape.
/// (Names without a signature are omitted — they can't be shape-compared.)
pub fn signature_shapes(decls: &[Decl]) -> HashMap<String, String> {
    let mut shapes = HashMap::new();
    for d in decls {
        if let Decl::TypeSig { name, ty } | Decl::ExportSig { name, ty } = d {
            shapes.entry(name.clone()).or_insert_with(|| type_shape(ty));
        }
    }
    shapes
}

/// Every value-level name the bodies of `decls` refer to, EXCLUDING each
/// definition's references to itself (plain self-recursion does not make a
/// name load-bearing for anyone else). Operators count: `>>=` in a body is a
/// reference to `>>=`.
///
/// Used on the Prelude's own declarations to compute which names its
/// implementation depends on. A user program redefining such a name would
/// have the Prelude type-checked — and its generated code resolved — against
/// the user's replacement, breaking code the user never wrote, so those
/// redefinitions are rejected up front (see `lib.rs`).
///
/// Where/let-bound locals that happen to share a referenced name are not
/// tracked as shadowing here; that only over-approximates the reference set,
/// which is the safe direction (and the Prelude introduces no such locals).
pub fn body_references(decls: &[Decl]) -> HashSet<String> {
    let mut out = HashSet::new();
    let add_clauses = |own_name: &str, clauses: &[Clause], out: &mut HashSet<String>| {
        let mut refs = HashSet::new();
        for c in clauses {
            refs_in_clause(c, &mut refs);
        }
        refs.remove(own_name);
        out.extend(refs);
    };
    for d in decls {
        match d {
            Decl::FunDef { name, clauses } => add_clauses(name, clauses, &mut out),
            Decl::ClassDecl { methods, .. } => {
                for m in methods {
                    if let Some(clauses) = &m.default_clauses {
                        add_clauses(&m.name, clauses, &mut out);
                    }
                }
            }
            Decl::InstanceDecl { methods, .. } => {
                for m in methods {
                    add_clauses(&m.name, &m.clauses, &mut out);
                }
            }
            _ => {}
        }
    }
    out
}

fn refs_in_clause(c: &Clause, out: &mut HashSet<String>) {
    for g in &c.guards {
        refs_in_expr(&g.condition, out);
        refs_in_expr(&g.body, out);
    }
    refs_in_expr(&c.body, out);
    for b in &c.where_binds {
        refs_in_expr(&b.body, out);
    }
}

fn refs_in_expr(e: &Expr, out: &mut HashSet<String>) {
    match e {
        Expr::Var(name) => { out.insert(name.clone()); }
        Expr::Con(_) | Expr::Lit(_) => {}
        Expr::App(f, x) => { refs_in_expr(f, out); refs_in_expr(x, out); }
        Expr::Lambda { body, .. } => refs_in_expr(body, out),
        Expr::InfixApp { op, lhs, rhs } => {
            out.insert(op.clone());
            refs_in_expr(lhs, out);
            refs_in_expr(rhs, out);
        }
        Expr::Negate(x) => refs_in_expr(x, out),
        Expr::If { cond, then_branch, else_branch } => {
            refs_in_expr(cond, out);
            refs_in_expr(then_branch, out);
            refs_in_expr(else_branch, out);
        }
        Expr::Case { scrutinee, branches } => {
            refs_in_expr(scrutinee, out);
            for b in branches {
                for g in &b.guards {
                    refs_in_expr(&g.condition, out);
                    refs_in_expr(&g.body, out);
                }
                refs_in_expr(&b.body, out);
            }
        }
        Expr::Let { binds, body } => {
            for b in binds {
                refs_in_expr(&b.body, out);
            }
            refs_in_expr(body, out);
        }
        Expr::Do(stmts) => {
            for s in stmts {
                match s {
                    DoStmt::Bind { expr, .. }
                    | DoStmt::Expr(expr)
                    | DoStmt::PatternBind { expr, .. }
                    | DoStmt::PatternDoLet { expr, .. } => refs_in_expr(expr, out),
                    DoStmt::DoLet { binds } => {
                        for b in binds {
                            refs_in_expr(&b.body, out);
                        }
                    }
                }
            }
        }
        Expr::Ascription(x, _) => refs_in_expr(x, out),
        Expr::RecordCon { fields, .. } => {
            for (_, x) in fields {
                refs_in_expr(x, out);
            }
        }
        Expr::RecordUpdate { expr, updates } => {
            refs_in_expr(expr, out);
            for (_, x) in updates {
                refs_in_expr(x, out);
            }
        }
        Expr::Paren(x) => refs_in_expr(x, out),
        Expr::OpFunc(op) => { out.insert(op.clone()); }
        Expr::Tuple(xs) => {
            for x in xs {
                refs_in_expr(x, out);
            }
        }
    }
}

fn collision_error(module: &str, collisions: &[(String, String)]) -> String {
    let alias = module.rsplit('.').next().unwrap_or(module);
    let listed: Vec<String> = collisions.iter()
        .map(|(name, src)| format!("'{}' (already defined by {})", name, src))
        .collect();
    format!(
        "importing '{module}' unqualified conflicts with {}.\n\
         mata-ll merges every import into a single namespace, so these names would clash \
         with the existing definitions. Import '{module}' qualified instead:\n\
         \n    import qualified {module} as {alias}\n\n\
         then refer to its members as `{alias}.name` (e.g. `{alias}.{}`).",
        listed.join(", "),
        collisions.first().map(|(n, _)| n.as_str()).unwrap_or("name"),
    )
}

fn decl_name(decl: &Decl) -> Option<String> {
    match decl {
        Decl::TypeSig { name, .. } => Some(name.clone()),
        Decl::FunDef { name, .. } => Some(name.clone()),
        Decl::DataDef { name, .. } => Some(name.clone()),
        Decl::NewtypeDef { name, .. } => Some(name.clone()),
        Decl::ClassDecl { name, .. } => Some(name.clone()),
        Decl::InstanceDecl { class_name, .. } => Some(class_name.clone()),
        Decl::ExportSig { name, .. } => Some(name.clone()),
        Decl::TypeFamily { name, .. } => Some(name.clone()),
        Decl::Import { .. } => None,
        Decl::FixityDecl { .. } => None,
        Decl::TypeAlias { name, .. } => Some(name.clone()),
    }
}
