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
    /// The same modules in RESOLUTION ORDER, so a detected cycle can be
    /// reported as the actual chain (`A -> B -> A`) instead of degrading
    /// to an empty module — which surfaced as a far-away "Unbound
    /// variable" with nothing pointing at the cycle (F13).
    in_progress_stack: Vec<String>,
    /// Fixities a module carries to its importers (its own declarations plus
    /// those of its imports, transitively — mata-ll merges every import into
    /// one namespace, and fixity travels with the operator).
    fixity_cache: HashMap<String, HashMap<String, (Assoc, u8)>>,
    /// Modules whose fixities are currently being computed (cycle detection).
    fixities_in_progress: HashSet<String>,
    /// The Prelude's fixity declarations, in force in every module (the
    /// implicit `import Prelude`).
    prelude_fixities: HashMap<String, (Assoc, u8)>,
    /// Each loaded module's display name and source text, by module key —
    /// retained for diagnostics: file attribution (`Diagnostic::file`) and
    /// the excerpt enrichment pass in lib.rs, which needs the text a span's
    /// line/col indexes. The display name is the resolved path for a file on
    /// disk and `<module key>` for an embedded stdlib module.
    module_sources: HashMap<String, (String, String)>,
    /// Non-fatal diagnostics collected during resolution (an import alias
    /// shadowed by a data constructor). Drained by the compile pipeline
    /// into `CompileResult.warnings`.
    warnings: Vec<crate::types::Diagnostic>,
}

impl ModuleLoader {
    pub fn new(source_dir: &Path) -> Self {
        // The embedded Prelude always lexes — it is compiled into the crate
        // and parsed on every compilation.
        let prelude_fixities = lexer::lex(crate::stdlib::PRELUDE)
            .map(|tokens| {
                parser::scan_fixities(&tokens)
                    .into_iter()
                    .map(|(op, assoc, prec)| (op, (assoc, prec)))
                    .collect()
            })
            .unwrap_or_default();
        ModuleLoader {
            search_paths: vec![source_dir.to_path_buf()],
            loaded: HashMap::new(),
            resolved: HashMap::new(),
            in_progress: HashSet::new(),
            in_progress_stack: Vec::new(),
            fixity_cache: HashMap::new(),
            fixities_in_progress: HashSet::new(),
            prelude_fixities,
            warnings: Vec::new(),
            module_sources: HashMap::new(),
        }
    }

    /// The display name and source text of loaded module `key`, when it was
    /// loaded from source (see `module_sources`).
    pub fn module_source(&self, key: &str) -> Option<(&str, &str)> {
        self.module_sources.get(key).map(|(n, s)| (n.as_str(), s.as_str()))
    }

    /// Every loaded module's (display name, source text) — the excerpt
    /// enrichment pass in lib.rs resolves a diagnostic's `file` back to the
    /// text its span indexes through this.
    pub fn loaded_sources(&self) -> impl Iterator<Item = (&str, &str)> {
        self.module_sources.values().map(|(n, s)| (n.as_str(), s.as_str()))
    }

    /// Drain the non-fatal diagnostics collected so far (see `warnings`).
    pub fn take_warnings(&mut self) -> Vec<crate::types::Diagnostic> {
        std::mem::take(&mut self.warnings)
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
        let (display_name, source) = match self.resolve_path(module_path) {
            Some(file_path) => {
                let text = fs::read_to_string(&file_path)
                    .map_err(|e| format!("Error reading {}: {}", file_path.display(), e))?;
                (file_path.display().to_string(), text)
            }
            None => {
                let text = crate::stdlib::embedded_module(&key)
                    .map(str::to_string)
                    .ok_or_else(|| format!("Cannot find module '{}'", key))?;
                (format!("<{}>", key), text)
            }
        };
        self.module_sources.insert(key.clone(), (display_name.clone(), source.clone()));

        // An imported module's lex and parse errors surface through the
        // import-error channel, prefixed with the module that failed.
        let (tokens, pragmas) = lexer::lex_with_pragmas(&source)
            .map_err(|d| format!("in module '{}': {}", key, d))?;
        // Its pragmas become "ignored pragma" warnings attributed to the
        // imported file, exactly as the root's are in compile_impl.
        for p in &pragmas {
            self.warnings.push(crate::pragma_warning(p, Some(display_name.as_str())));
        }
        // Fixity is part of a module's interface: this module's operators
        // must group under the fixities its imports (and the implicit
        // Prelude) declare, so those are collected before parsing.
        let fixities = self.fixities_for(&tokens);
        // An imported module's syntax errors surface through the import-error
        // channel, prefixed with the module that failed to parse.
        let module = parser::parse_with_fixities(tokens, &fixities).map_err(|diags| {
            let msgs: Vec<String> = diags.iter().map(|d| d.to_string()).collect();
            format!("in module '{}': {}", key, msgs.join("\n"))
        })?;

        self.loaded.insert(key.clone(), module);
        Ok(self.loaded.get(&key).unwrap())
    }

    /// The fixities in force for a module with the given token stream: the
    /// implicit Prelude's, plus everything its imports carry (transitively).
    /// The module's own declarations are layered on top by the parser itself.
    /// An import that cannot be loaded is skipped here — `resolve_imports`
    /// reports it through the normal import-error channel.
    pub fn fixities_for(&mut self, tokens: &[lexer::Located]) -> HashMap<String, (Assoc, u8)> {
        let mut fixities = self.prelude_fixities.clone();
        for path in parser::scan_imports(tokens) {
            if let Ok(imported) = self.imported_fixities(&path) {
                fixities.extend(imported);
            }
        }
        fixities
    }

    /// The fixities a module exports to an importer: its own declarations
    /// plus its imports' (transitively), own winning on a clash. On an
    /// import cycle the cycled-on module contributes nothing, matching how
    /// `resolve_imports` breaks cycles.
    fn imported_fixities(&mut self, module_path: &[String]) -> Result<HashMap<String, (Assoc, u8)>, String> {
        let key = module_path.join(".");
        if let Some(cached) = self.fixity_cache.get(&key) {
            return Ok(cached.clone());
        }
        if self.fixities_in_progress.contains(&key) {
            return Ok(HashMap::new());
        }
        self.fixities_in_progress.insert(key.clone());
        let result: Result<HashMap<String, (Assoc, u8)>, String> = (|| {
            self.load_module(module_path)?;
            let module = self.loaded.get(&key).unwrap();
            let mut fixities = HashMap::new();
            let mut own = Vec::new();
            let mut imports = Vec::new();
            for decl in &module.decls {
                match decl {
                    Decl::FixityDecl { assoc, prec, op } => {
                        own.push((op.clone(), (*assoc, *prec)));
                    }
                    Decl::Import { module_path, .. } => imports.push(module_path.clone()),
                    _ => {}
                }
            }
            for import in imports {
                fixities.extend(self.imported_fixities(&import)?);
            }
            fixities.extend(own);
            Ok(fixities)
        })();
        self.fixities_in_progress.remove(&key);
        let fixities = result?;
        self.fixity_cache.insert(key, fixities.clone());
        Ok(fixities)
    }

    /// Process all imports in a module, returning merged declarations.
    /// The imported declarations are prepended to the module's own declarations.
    pub fn resolve_imports(&mut self, module: &Module) -> Result<Module, String> {
        // "//root" is not a legal module name, so the entry module's own
        // span can never collide with an imported module's key.
        self.resolve_imports_keyed(module, "//root")
    }

    fn resolve_imports_keyed(&mut self, module: &Module, own_key: &str) -> Result<Module, String> {
        let mut imported_decls: Vec<Decl> = Vec::new();
        let mut own_decls: Vec<Decl> = Vec::new();
        // Provenance of imported_decls (see Module::origin_spans) and the
        // module keys already merged: a diamond's shared module arrives
        // through every path, and must contribute its declarations once.
        let mut out_spans: Vec<(String, usize)> = Vec::new();
        let mut merged_origins: HashSet<String> = HashSet::new();
        // Import declarations grouped per module, in first-appearance order.
        // GHC MERGES repeated imports — each one contributes visibility
        // (`import M (a)` + `import M (b)` makes both visible; qualified
        // and unqualified forms of one module coexist; two aliases both
        // work). The old `seen_imports` short-circuit dropped every import
        // after a module's first, so the second list's names stayed hidden
        // and a second alias was never introduced.
        let mut import_order: Vec<String> = Vec::new();
        let mut import_paths: HashMap<String, Vec<String>> = HashMap::new();
        let mut import_forms: HashMap<String, Vec<ImportItems>> = HashMap::new();
        for decl in &module.decls {
            if let Decl::Import { module_path, items } = decl {
                let key = module_path.join(".");
                if !import_paths.contains_key(&key) {
                    import_order.push(key.clone());
                    import_paths.insert(key.clone(), module_path.clone());
                }
                import_forms.entry(key).or_default().push(items.clone());
            }
        }
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

        for key in &import_order {
            let module_path = &import_paths[key];
            let forms = &import_forms[key];

            // Recursively resolve imports in the imported module
            let resolved = if self.resolved.contains_key(key) {
                self.resolved.get(key).unwrap().clone()
            } else if self.in_progress.contains(key) {
                // Import cycle: report the actual chain. The old behavior —
                // an empty placeholder module — compiled on and surfaced
                // as an "Unbound variable" far from the cause (F13).
                let start = self.in_progress_stack.iter()
                    .position(|k| k == key)
                    .unwrap_or(0);
                let mut chain: Vec<&str> = self.in_progress_stack[start..]
                    .iter().map(String::as_str).collect();
                chain.push(key);
                return Err(format!(
                    "module imports form a cycle: {}\n\
                     mata-ll resolves an import by copying the imported module's \
                     declarations into the importer, which has no meaning when the \
                     modules import each other (GHC rejects import cycles for the \
                     same structural reason). Break the cycle by moving the shared \
                     definitions into a module both can import.",
                    chain.join(" -> ")));
            } else {
                self.in_progress.insert(key.clone());
                self.in_progress_stack.push(key.clone());
                let imported = self.load_module(module_path)?.clone();
                let r = self.resolve_imports_keyed(&imported, key);
                self.in_progress.remove(key);
                self.in_progress_stack.pop();
                let r = r?;
                self.resolved.insert(key.clone(), r.clone());
                r
            };

            // Include ALL non-import declarations for compilation
            // (exported functions may depend on internal helpers).
            // Track hidden names for typechecker enforcement. The
            // resolved module is ours (one clone out of the cache
            // above); its declarations move into the import list.
            let child_spans = resolved.origin_spans;
            let all_decls: Vec<Decl> = resolved.decls.into_iter()
                .filter(|d| !matches!(d, Decl::Import { .. }))
                .collect();
            // The child's provenance spans, when they cover its
            // declaration list exactly (a resolved module always does; a
            // raw one — no spans — counts as one span of its own).
            let child_spans: Vec<(String, usize)> =
                if child_spans.iter().map(|(_, n)| n).sum::<usize>() == all_decls.len()
                    && !child_spans.is_empty()
                {
                    child_spans
                } else {
                    vec![(key.clone(), all_decls.len())]
                };

            // Compute hidden names: names the module itself defines but
            // does not export. Only the module's OWN declarations count
            // — names merged in transitively from its imports are not
            // "private to" this module just because its export list
            // omits them, so we look at the loaded (pre-merge) module.
            let parsed_exports = self.loaded.get(key).and_then(|m| m.exports.clone());
            let own_decl_names: Vec<String> = self.loaded.get(key)
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

            // Split this module's import forms: the unqualified ones merge
            // into ONE unqualified copy of the declarations (visibility is
            // the union of what each form admits), and each distinct alias
            // gets its own prefixed copy.
            let item_names = |items: &[ImportItem]| -> HashSet<String> {
                items.iter().map(|item| match item {
                    ImportItem::Value(n) => n.clone(),
                    ImportItem::TypeAll(n) => n.clone(),
                    ImportItem::TypeOnly(n) => n.clone(),
                }).collect()
            };
            let mut aliases: Vec<&String> = Vec::new();
            let mut unqual: Vec<&ImportItems> = Vec::new();
            for form in forms {
                match form {
                    ImportItems::Qualified(alias) => {
                        if !aliases.contains(&alias) {
                            aliases.push(alias);
                        }
                    }
                    other => unqual.push(other),
                }
            }

            if !unqual.is_empty() {
                // Every explicitly requested name is recorded (the
                // explicit-import-overrides-transitive-hiding rule below).
                for form in &unqual {
                    if let ImportItems::Specific(items) = form {
                        for w in item_names(items) {
                            visible_names.insert(w);
                        }
                    }
                }
                // A name is selection-hidden only when EVERY unqualified
                // form hides it: a Specific list hides what it doesn't
                // request, a Hiding list hides what it excludes, and a
                // plain `import M` hides nothing.
                let hidden_by_all = |n: &String| {
                    unqual.iter().all(|form| match form {
                        ImportItems::All => false,
                        ImportItems::Specific(items) => !item_names(items).contains(n),
                        ImportItems::Hiding(items) => item_names(items).contains(n),
                        ImportItems::Qualified(_) => unreachable!("split above"),
                    })
                };
                // The hidden bookkeeping is name-based and runs over EVERY
                // declaration; the decl push dedups by origin module — a
                // span whose module was already merged through an earlier
                // import edge contributes nothing (its declarations are
                // identical; a second copy tripped the duplicate-instance
                // check and re-checked/re-generated every function).
                let mut idx = 0;
                for (okey, len) in &child_spans {
                    let slice = &all_decls[idx..idx + len];
                    idx += len;
                    let fresh = merged_origins.insert(okey.clone());
                    for d in slice {
                        if let Some(n) = decl_name(d)
                            && hidden_by_all(&n) {
                                hidden_names.insert(n);
                            }
                        if fresh {
                            imported_decls.push(d.clone());
                        }
                    }
                    if fresh {
                        out_spans.push((okey.clone(), *len));
                    }
                }
                debug_assert_eq!(idx, all_decls.len(), "origin spans cover the decl list");
            }
            for alias in aliases {
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

        // A constructor use `f M` and a qualified reference `M.f` parse to
        // the SAME shape — `App(Var "f", Con "M")` (field access desugars
        // accessor-first, exactly like application) — so when an alias name
        // is also a visible data constructor the two meanings cannot be
        // told apart here. The constructor wins: the plain application is
        // the meaning the expression already has without the alias, and
        // collapsing it produced a bogus "Unbound variable: M.f" (or a
        // silent call of the wrong module's function). Qualified references
        // through such an alias are therefore unavailable — a deviation
        // from GHC, whose module aliases live in a separate namespace — so
        // warn, pointing at the rename that restores them.
        let ctor_names: HashSet<String> = module.decls.iter()
            .chain(imported_decls.iter())
            .flat_map(|d| -> Vec<String> {
                match d {
                    Decl::DataDef { constructors, .. } =>
                        constructors.iter().map(|c| c.name.clone()).collect(),
                    Decl::NewtypeDef { con_name: Some(c), .. } => vec![c.clone()],
                    _ => Vec::new(),
                }
            })
            .collect();
        let qualified_aliases: HashSet<String> = qualified_aliases.into_iter()
            .filter(|a| {
                let keep = !ctor_names.contains(a);
                if !keep {
                    self.warnings.push(crate::types::Diagnostic {
                        kind: crate::types::DiagnosticKind::Other(format!(
                            "import alias '{}' is also a data constructor; \
                             qualified references '{}.name' will not resolve",
                            a, a)),
                        context: None,
                        span: None,
                        file: None,
                        notes: vec![format!(
                            "in mata-ll, a qualified reference '{a}.f' and an \
                             application 'f {a}' parse identically, so the \
                             constructor meaning wins. GHC keeps module \
                             aliases in a separate namespace and allows both; \
                             rename the alias (e.g. 'as {a}M') to use \
                             qualified references.")],
                        baseline: false, excerpt: None,
                    });
                }
                keep
            })
            .collect();
        for decl in &module.decls {
            if matches!(decl, Decl::Import { .. }) {
                continue;
            }
            let d = if qualified_aliases.is_empty() {
                decl.clone()
            } else {
                rewrite_qualified_uses_decl(decl.clone(), &qualified_aliases)
            };
            own_decls.push(d);
        }

        // An explicit import of a name overrides transitive selection-hiding,
        // but never a module's own export-list privacy.
        for v in &visible_names {
            if !private_names.contains(v) {
                hidden_names.remove(v);
            }
        }

        // Merge: imported first, then own. The own span is keyed by this
        // module's import key so a PARENT merging this resolved module can
        // recognize these declarations when they also arrive via a sibling.
        out_spans.push((own_key.to_string(), own_decls.len()));
        imported_decls.extend(own_decls);
        Ok(Module { decls: imported_decls, exports: None, hidden: hidden_names, origin_spans: out_spans })
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

fn collect_module_names(decls: &[Decl]) -> ModuleNames {
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
            Decl::NewtypeDef { name, type_vars, con_name, field, inner, deriving } => Decl::NewtypeDef {
                name: self.q(name),
                type_vars: type_vars.clone(),
                // The constructor and selector are values, not sibling type
                // references; the deriving classes are global names.
                con_name: con_name.clone(),
                field: field.clone(),
                inner: self.ty(inner),
                deriving: deriving.clone(),
            },
            Decl::TypeAlias { name, params, ty } => Decl::TypeAlias {
                name: self.q(name), params: params.clone(), ty: self.ty(ty),
            },
            Decl::TypeFamily { name, params, equations } => Decl::TypeFamily {
                name: self.q(name),
                params: params.clone(),
                equations: equations.iter().map(|eq| TypeFamilyEq {
                    args: eq.args.iter().map(|t| self.ty(t)).collect(),
                    result: self.ty(&eq.result),
                }).collect(),
            },
            // Class and instance NAMES stay global (a qualified class name
            // can't be written at a use site anyway), but their heads and
            // bodies refer to the module's own types and values, which ARE
            // prefixed — so an instance head names the prefixed type and
            // its method bodies (and class default bodies) call the
            // prefixed siblings.
            Decl::InstanceDecl { class_name, target_type, context, methods } => Decl::InstanceDecl {
                class_name: class_name.clone(),
                target_type: self.ty(target_type),
                context: context.iter().map(|c| self.constraint(c)).collect(),
                methods: methods.iter().map(|m| InstanceMethod {
                    name: m.name.clone(),
                    clauses: m.clauses.iter().map(|c| self.clause(c)).collect(),
                }).collect(),
            },
            Decl::ClassDecl { name, type_var, superclasses, methods } => Decl::ClassDecl {
                name: name.clone(),
                type_var: type_var.clone(),
                superclasses: superclasses.clone(),
                methods: methods.iter().map(|m| ClassMethod {
                    name: m.name.clone(),
                    ty: self.ty(&m.ty),
                    default_clauses: m.default_clauses.as_ref().map(|cs| {
                        cs.iter().map(|c| self.clause(c)).collect()
                    }),
                }).collect(),
            },
            Decl::Import { .. } | Decl::FixityDecl { .. } => decl.clone(),
        }
    }

    fn constraint(&self, c: &Constraint) -> Constraint {
        Constraint { class_name: c.class_name.clone(), type_arg: self.ty(&c.type_arg) }
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
            body: c.body.as_ref().map(|b| self.expr(b, &bound)),
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
            // The rename decisions: qualify a name only when it refers to a
            // module-level sibling, not a local binding.
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
            // An InfixApp's op is a NAME too — a sibling operator (`a <+> b`)
            // or a sibling function used backtick-infix (`a `combine` b`).
            // Their DEFINITIONS are prefixed like every value, so in-module
            // infix uses must follow; this arm was missing, and the uniform
            // descent below only visits subEXPRESSIONS, so the op stayed
            // bare and resolved to nothing ("undefined '<+>'").
            Expr::InfixApp { op, lhs, rhs } => {
                let op = if !bound.contains(op) && self.names.vals.contains(op) {
                    self.q(op)
                } else {
                    op.clone()
                };
                Expr::InfixApp {
                    op,
                    lhs: Box::new(self.expr(lhs, bound)),
                    rhs: Box::new(self.expr(rhs, bound)),
                }
            }
            // Ascriptions carry a type; the generic descent visits only
            // expressions, and sibling type names need qualifying too.
            Expr::Ascription(x, t) =>
                Expr::Ascription(Box::new(self.expr(x, bound)), self.ty(t)),
            // Binder nodes: their children see an extended (or, for do-blocks,
            // sequentially threaded) scope, so each computes its own `bound`
            // instead of taking the uniform descent.
            Expr::Lambda { params, body } => {
                let mut b = bound.clone();
                for p in params { b.insert(p.clone()); }
                Expr::Lambda { params: params.clone(), body: Box::new(self.expr(body, &b)) }
            }
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
                        body: br.body.as_ref().map(|body| self.expr(body, &b)),
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
            // Everything else neither names a sibling nor binds anything:
            // descend uniformly with the current scope.
            other => other.clone().map_subexprs(&mut |c| self.expr(&c, bound)),
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
            DoStmt::PatternBind { pattern, expr, span } => {
                let e = self.expr(expr, bound);
                collect_pattern_vars(pattern, bound);
                DoStmt::PatternBind { pattern: pattern.clone(), expr: e, span: *span }
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
            Type::Arrow(a, b, m) => Type::Arrow(Box::new(self.ty(a)), Box::new(self.ty(b)), m.clone()),
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
                constraints: constraints.iter().map(|c| self.constraint(c)).collect(),
                ty: Box::new(self.ty(ty)),
            },
        }
    }
}

/// Collect the variable names bound by a pattern (for scope tracking).
fn collect_pattern_vars(p: &Pattern, out: &mut HashSet<String>) {
    p.for_each_var(&mut |v| { out.insert(v.to_string()); });
}

/// Rewrite qualified use-sites in one of the importing module's declarations.
/// `M.foo` parsed as the field-access shape `App(Var "foo", Con "M")`; where
/// `M` is a known qualified alias, collapse it to `Var "M.foo"`.
fn rewrite_qualified_uses_decl(decl: Decl, aliases: &HashSet<String>) -> Decl {
    let clauses_of = |cs: Vec<Clause>| -> Vec<Clause> {
        cs.into_iter().map(|c| rewrite_uses_clause(c, aliases)).collect()
    };
    match decl {
        Decl::FunDef { name, clauses } => Decl::FunDef { name, clauses: clauses_of(clauses) },
        // Instance method bodies and class default bodies can also
        // reference qualified imports.
        Decl::InstanceDecl { class_name, target_type, context, methods } => Decl::InstanceDecl {
            class_name, target_type, context,
            methods: methods.into_iter().map(|m| InstanceMethod {
                name: m.name,
                clauses: clauses_of(m.clauses),
            }).collect(),
        },
        Decl::ClassDecl { name, type_var, superclasses, methods } => Decl::ClassDecl {
            name, type_var, superclasses,
            methods: methods.into_iter().map(|m| ClassMethod {
                name: m.name,
                ty: m.ty,
                default_clauses: m.default_clauses.map(&clauses_of),
            }).collect(),
        },
        other => other,
    }
}

fn rewrite_uses_clause(c: Clause, aliases: &HashSet<String>) -> Clause {
    Clause {
        patterns: c.patterns,
        guards: c.guards.into_iter().map(|g| Guard {
            condition: rewrite_uses_expr(g.condition, aliases),
            body: rewrite_uses_expr(g.body, aliases),
        }).collect(),
        body: c.body.map(|b| rewrite_uses_expr(b, aliases)),
        where_binds: c.where_binds.into_iter()
            .map(|ld| rewrite_uses_localdef(ld, aliases)).collect(),
        span: c.span,
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
    let e = e.map_subexprs(&mut |c| rewrite_uses_expr(c, aliases));
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
        Type::Arrow(a, b, _) => format!("({}->{})", type_shape(a), type_shape(b)),
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
    if let Some(b) = &c.body { refs_in_expr(b, out); }
    for b in &c.where_binds {
        refs_in_expr(&b.body, out);
    }
}

fn refs_in_expr(e: &Expr, out: &mut HashSet<String>) {
    // The collection decisions: which nodes NAME a value. Everything else is
    // generic descent.
    match e {
        Expr::Var(name) | Expr::OpFunc(name) => { out.insert(name.clone()); }
        // An infix use `a >>= b` is a reference to the operator itself, on
        // top of whatever its operands reference.
        Expr::InfixApp { op, .. } => { out.insert(op.clone()); }
        _ => {}
    }
    e.for_each_subexpr(&mut |c| refs_in_expr(c, out));
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
