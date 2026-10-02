//! Module resolution and loading
//!
//! Each .mll file is a module. Module names map to file paths:
//!   import Data.Tree  =>  Data/Tree.mll
//!
//! The compiler works on ONE flat declaration list per program. Import
//! resolution builds it: every imported module's declarations are merged
//! in front of the entry module's, each origin once, and — before that —
//! every module's own source is rewritten so that each reference is spelled
//! as the canonical name of the one declaration it denotes in THAT
//! module's scope (see `ModuleLoader::resolve_module` for the model). The
//! flat list can therefore hold two modules' `helper` side by side: they
//! are `A.helper` and `B.helper`, and no reference is ambiguous.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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
    /// Already resolved modules (key -> canonical region and surface)
    resolved: HashMap<String, Resolved>,
    /// Modules currently being resolved (cycle detection)
    in_progress: HashSet<String>,
    /// The same modules in RESOLUTION ORDER, so a detected cycle can be
    /// reported as the actual chain (`A -> B -> A`) instead of degrading
    /// to an empty module — which surfaced as a far-away "Unbound
    /// variable" with nothing pointing at the cycle (F13).
    in_progress_stack: Vec<String>,
    /// Fixities a module carries to its importers (its own declarations plus
    /// those of its imports, transitively: fixity is keyed by the operator's
    /// spelling and travels along every import edge).
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
    /// What the implicit Prelude provides (see `PreludeScope`); set by the
    /// compile pipeline before resolution.
    prelude: PreludeScope,
    /// Record-field and class-method names of every module in the import
    /// closure: global names, which the entry module's own declarations
    /// must not share a spelling with (see `canon_val`).
    program_globals: HashSet<String>,
    /// The module name the entry module's clashing declarations are
    /// qualified with (see `begin_program`).
    root_prefix: String,
    /// The entry module's value / type declarations that keep their bare
    /// names. A bare name in the flat list that an IMPORTED module does not
    /// have in scope must not resolve to one of these.
    root_vals: HashSet<String>,
    root_tys: HashSet<String>,
    /// Program-wide by-products of resolution, handed to the type checker
    /// with the resolved entry module (see `ast::Resolution`).
    scope_errors: HashMap<String, ScopeError>,
    source_names: HashMap<String, String>,
    /// Signature shape of every signed value, by canonical name (for the
    /// import-against-Prelude check).
    shapes: HashMap<String, String>,
    /// Canonical names that are types, and how many type declarations in
    /// the program share each source spelling (for diagnostics' display
    /// names).
    canon_types: HashSet<String>,
    type_spellings: HashMap<String, usize>,
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
            prelude: PreludeScope::default(),
            program_globals: HashSet::new(),
            root_prefix: String::new(),
            root_vals: HashSet::new(),
            root_tys: HashSet::new(),
            scope_errors: HashMap::new(),
            source_names: HashMap::new(),
            shapes: HashMap::new(),
            canon_types: HashSet::new(),
            type_spellings: HashMap::new(),
        }
    }

    /// Declare what the implicit Prelude provides. Must be called before
    /// `resolve_imports`: scopes, canonical names and the import check all
    /// depend on it.
    pub fn set_prelude(&mut self, prelude: PreludeScope) {
        self.prelude = prelude;
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
            if full_path.exists() && Self::exact_case_exists(&full_path) {
                return Some(full_path);
            }
        }
        None
    }

    /// Does the file exist under exactly this spelling? `Path::exists` says
    /// yes for `lmath.mll` when only `LMath.mll` exists on a
    /// case-insensitive filesystem (macOS, Windows), so a root file named
    /// like an imported module in another case resolved the import to
    /// ITSELF ("module imports form a cycle: LMath -> LMath"). The
    /// directory listing carries the on-disk spelling.
    fn exact_case_exists(path: &Path) -> bool {
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else { return true };
        let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
        match std::fs::read_dir(dir) {
            Ok(entries) => entries.flatten().any(|e| e.file_name() == name),
            // Unreadable directory: trust `exists` (the load reports the
            // real error).
            Err(_) => true,
        }
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
            // `import Prelude ...` names the implicit Prelude, whose
            // fixities are in force already; it is never loaded as a module.
            if path.len() == 1 && path[0] == PRELUDE_KEY {
                continue;
            }
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
                if import.len() == 1 && import[0] == PRELUDE_KEY {
                    continue;
                }
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

    /// Resolve the entry module: every import (transitively) is resolved
    /// and merged in front of the module's own declarations, and every
    /// reference is bound in the scope of the module that wrote it (see
    /// `resolve_module` for the model). The returned module carries the
    /// program's `Resolution` for the type checker.
    pub fn resolve_imports(&mut self, module: &Module) -> Result<Module, String> {
        self.begin_program(module);
        let mut resolved = self.resolve_module(module, ROOT_KEY)?.module;
        self.check_host_exports(&resolved)?;
        let mut display_names = HashMap::new();
        for (canon, base) in &self.source_names {
            // A type whose source spelling another type shares keeps its
            // module-qualified form: `A.Config` against `B.Config` (or
            // against the Prelude's) must stay distinguishable.
            let shared = self.canon_types.contains(canon)
                && self.type_spellings.get(base).copied().unwrap_or(0)
                    + usize::from(self.prelude.types.contains(base)) > 1;
            if !shared {
                display_names.insert(canon.clone(), base.clone());
            }
        }
        for (placeholder, err) in &self.scope_errors {
            display_names.insert(placeholder.clone(), err.written().to_string());
        }
        resolved.resolution = Resolution {
            scope_errors: std::mem::take(&mut self.scope_errors),
            source_names: self.source_names.clone(),
            display_names,
        };
        Ok(resolved)
    }

    /// Program-wide facts the per-module resolution needs before it starts:
    /// the globally visible record-field and class-method names of every
    /// imported module, the prefix the entry module's clashing names are
    /// qualified with, and which of the entry module's names keep their
    /// bare spelling. The whole import closure is loaded (parsed) here;
    /// a module that fails to load is skipped and reported by resolution.
    fn begin_program(&mut self, root: &Module) {
        let mut pending: Vec<Vec<String>> = import_paths_of(root);
        let mut seen: HashSet<String> = HashSet::new();
        while let Some(path) = pending.pop() {
            let key = path.join(".");
            if key == PRELUDE_KEY || !seen.insert(key) {
                continue;
            }
            if let Ok(m) = self.load_module(&path) {
                pending.extend(import_paths_of(m));
            }
        }
        self.program_globals.clear();
        for key in &seen {
            if let Some(m) = self.loaded.get(key) {
                for d in &m.decls {
                    self.program_globals.extend(global_value_names(d));
                }
            }
        }
        // GHC's name for the entry module. A module of that name in the
        // import closure would share the prefix, so step past it.
        let mut prefix = "Main".to_string();
        let mut n = 1;
        while self.loaded.contains_key(&prefix) {
            prefix = format!("Main{n}");
            n += 1;
        }
        self.root_prefix = prefix;
        self.root_vals.clear();
        self.root_tys.clear();
        for d in &root.decls {
            match renamable(d) {
                Some((name, false)) if self.canon_val(ROOT_KEY, name) == name => {
                    self.root_vals.insert(name.to_string());
                }
                Some((name, true)) if self.canon_ty(ROOT_KEY, name) == name => {
                    self.root_tys.insert(name.to_string());
                }
                _ => {}
            }
        }
    }

    /// The canonical name of value `name` declared by module `key`.
    ///
    /// An imported module's declaration is ALWAYS `Module.name`: a spelling
    /// no source identifier can have, so it cannot collide with the
    /// Prelude, the entry module, another import, or a local binder. The
    /// entry module's declarations keep their bare names — a program
    /// without imports compiles to the same Lua names as ever — except
    /// where the bare name is already taken program-wide: by the Prelude
    /// (source definition or builtin), or by a record field or class
    /// method of an imported module (those are global names). `main` is
    /// the entry point and always bare.
    fn canon_val(&self, key: &str, name: &str) -> String {
        if key != ROOT_KEY {
            return format!("{key}.{name}");
        }
        if name != "main"
            && (self.prelude.values.contains(name) || self.program_globals.contains(name))
        {
            format!("{}.{name}", self.root_prefix)
        } else {
            name.to_string()
        }
    }

    /// The canonical name of type `name` declared by module `key` (see
    /// `canon_val`; the only bare type names taken program-wide are the
    /// Prelude's).
    fn canon_ty(&self, key: &str, name: &str) -> String {
        if key != ROOT_KEY {
            return format!("{key}.{name}");
        }
        if self.prelude.types.contains(name) {
            format!("{}.{name}", self.root_prefix)
        } else {
            name.to_string()
        }
    }

    /// Resolve one module: its imports (recursively, through the cache),
    /// then its own declarations.
    ///
    /// THE MODEL. Every top-level value and type declaration is an ENTITY
    /// with one canonical name fixed by its defining module alone
    /// (`canon_val` / `canon_ty`), whoever imports it and however. A
    /// module's SCOPE maps the bare names it may write to entities:
    ///
    ///   1. its own declarations;
    ///   2. what its unqualified imports make visible — each import edge
    ///      contributes the imported module's export SURFACE, filtered by
    ///      the edge's import list / `hiding` clause;
    ///   3. the implicit Prelude (or what explicit `import Prelude` forms
    ///      leave of it).
    ///
    /// An own declaration wins over an imported one, and an imported one
    /// over the Prelude's. Two DIFFERENT imported entities under one bare
    /// name are ambiguous, reported where the name is used (as in GHC) —
    /// never at the import, and never by picking one. `Alias.name`
    /// resolves in the alias's own table: the surfaces of the modules
    /// imported under that alias, nothing else.
    ///
    /// References are bound HERE, once, while the module's own source is
    /// rewritten to canonical spelling (`Rename`): a local binder shadows
    /// the module scope, a reference found in scope becomes its entity's
    /// canonical name, and a reference that denotes nothing in scope
    /// becomes a placeholder the type checker reports (`ScopeError`).
    /// Nothing an importer does can change what this module's references
    /// mean, so the resolved region is cached and merged verbatim — once
    /// per origin — into every importer.
    ///
    /// Record fields, class methods, classes and constructors are not part
    /// of this model: they remain global names, as before.
    fn resolve_module(&mut self, module: &Module, own_key: &str) -> Result<Resolved, String> {
        let is_root = own_key == ROOT_KEY;

        // Import declarations grouped per module, in first-appearance order.
        // GHC MERGES repeated imports — each one contributes visibility
        // (`import M (a)` + `import M (b)` makes both visible; qualified
        // and unqualified forms of one module coexist; two aliases both
        // work). `import Prelude ...` loads nothing: the Prelude is always
        // part of the program, and such an import only declares which of
        // its names this module sees (`prelude_forms`).
        let mut import_order: Vec<String> = Vec::new();
        let mut import_paths: HashMap<String, Vec<String>> = HashMap::new();
        let mut import_forms: HashMap<String, Vec<ImportItems>> = HashMap::new();
        let mut prelude_forms: Vec<ImportItems> = Vec::new();
        for decl in &module.decls {
            if let Decl::Import { module_path, items } = decl {
                let key = module_path.join(".");
                if key == PRELUDE_KEY {
                    prelude_forms.push(items.clone());
                    continue;
                }
                if !import_paths.contains_key(&key) {
                    import_order.push(key.clone());
                    import_paths.insert(key.clone(), module_path.clone());
                }
                import_forms.entry(key).or_default().push(items.clone());
            }
        }

        // Phase 1: resolve every imported module and capture its REGION —
        // the canonical declaration list with the origin runs that
        // partition it — and its export surface.
        let mut edges: Vec<Edge> = Vec::new();
        for key in &import_order {
            let module_path = &import_paths[key];
            let resolved = if let Some(r) = self.resolved.get(key) {
                r.clone()
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
                     mata-ll compiles a program as one unit in which every \
                     module's declarations come after those of the modules it \
                     imports, which has no meaning when the modules import \
                     each other (GHC rejects import cycles for the same \
                     structural reason). Break the cycle by moving the shared \
                     definitions into a module both can import.",
                    chain.join(" -> ")));
            } else {
                self.in_progress.insert(key.clone());
                self.in_progress_stack.push(key.clone());
                let imported = self.load_module(module_path)?.clone();
                let r = self.resolve_module(&imported, key);
                self.in_progress.remove(key);
                self.in_progress_stack.pop();
                let r = r?;
                self.resolved.insert(key.clone(), r.clone());
                r
            };

            // Include ALL non-import declarations for compilation
            // (exported functions may depend on internal helpers).
            let child_spans = resolved.module.origin_spans;
            let decls: Vec<Decl> = resolved.module.decls.into_iter()
                .filter(|d| !matches!(d, Decl::Import { .. }))
                .collect();
            let spans: Vec<(String, usize)> =
                if child_spans.iter().map(|(_, n)| n).sum::<usize>() == decls.len()
                    && !child_spans.is_empty()
                {
                    child_spans
                } else {
                    vec![(key.clone(), decls.len())]
                };
            let mut unqual: Vec<ImportItems> = Vec::new();
            let mut aliases: Vec<String> = Vec::new();
            for form in &import_forms[key] {
                match form {
                    ImportItems::Qualified(alias) => {
                        if !aliases.contains(alias) {
                            aliases.push(alias.clone());
                        }
                    }
                    other => unqual.push(other.clone()),
                }
            }
            let listed = self.loaded.get(key).is_some_and(|m| m.exports.is_some());
            edges.push(Edge {
                key: key.clone(), unqual, aliases, decls, spans,
                surface: resolved.surface, listed,
            });
        }

        // `module X` export entries (Haskell 2010 §5.2). X naming the
        // module itself exports every own declaration; X naming an import
        // (by module name or alias) re-exports what that import edge makes
        // visible here.
        let mut reexported_edges: Vec<usize> = Vec::new();
        let mut self_export = false;
        if let Some(ex) = &module.exports {
            for x in &ex.reexports {
                if x == own_key || module.name.as_deref() == Some(x.as_str()) {
                    self_export = true;
                    continue;
                }
                let matching: Vec<usize> = edges.iter().enumerate()
                    .filter(|(_, e)| e.key == *x || e.aliases.iter().any(|a| a == x))
                    .map(|(i, _)| i)
                    .collect();
                if matching.is_empty() {
                    return Err(format!(
                        "the export list re-exports 'module {x}', but this \
                         module neither is nor imports '{x}'\n\
                         A 'module X' export entry passes an imported \
                         module's names on to this module's importers \
                         (Haskell 2010 section 5.2), so it needs a matching \
                         'import {x}' (or a qualified import aliased as \
                         '{x}') in this module."));
                }
                let mut any_unqual = false;
                for i in matching {
                    // A qualified-only import leaves nothing in scope
                    // unqualified, and H2010 defines `module X` as the
                    // entities in scope BOTH unqualified and as X.e — so
                    // such an entry exports nothing (GHC warns likewise).
                    if edges[i].unqual.is_empty() { continue; }
                    any_unqual = true;
                    if !reexported_edges.contains(&i) {
                        reexported_edges.push(i);
                    }
                }
                if !any_unqual {
                    self.warnings.push(crate::types::Diagnostic {
                        kind: crate::types::DiagnosticKind::Other(format!(
                            "export entry 'module {x}' exports nothing: \
                             '{x}' is only imported qualified")),
                        context: None,
                        span: None,
                        file: None,
                        notes: vec![
                            "a 'module X' export entry covers the entities \
                             in scope both unqualified and as X.e (Haskell \
                             2010 section 5.2); a qualified-only import \
                             puts nothing in scope unqualified. Add an \
                             unqualified 'import' of the module to \
                             re-export it.".to_string()],
                        baseline: false, excerpt: None, names: None,
                    });
                }
            }
        }

        // Phase 2: this module's own entities. Record fields and class
        // methods are global names; they are in scope here under their
        // bare spelling and win over anything imported, like every own
        // declaration.
        let mut own_vals: HashMap<String, String> = HashMap::new();
        let mut own_tys: HashMap<String, String> = HashMap::new();
        let mut own_globals: HashSet<String> = HashSet::new();
        let mut own_classes: HashSet<String> = HashSet::new();
        for d in &module.decls {
            match renamable(d) {
                Some((n, false)) => {
                    if !own_vals.contains_key(n) {
                        own_vals.insert(n.to_string(), self.canon_val(own_key, n));
                    }
                }
                Some((n, true)) => {
                    if !own_tys.contains_key(n) {
                        let canon = self.canon_ty(own_key, n);
                        *self.type_spellings.entry(n.to_string()).or_insert(0) += 1;
                        self.canon_types.insert(canon.clone());
                        own_tys.insert(n.to_string(), canon);
                    }
                }
                None => {}
            }
            own_globals.extend(global_value_names(d));
            if let Decl::ClassDecl { name, .. } = d {
                own_classes.insert(name.clone());
            }
        }
        for (n, canon) in own_vals.iter().chain(own_tys.iter()) {
            if canon != n {
                self.source_names.insert(canon.clone(), n.clone());
            }
        }
        for d in &module.decls {
            if let Decl::TypeSig { name, ty } | Decl::ExportSig { name, ty } = d {
                self.shapes.entry(own_vals[name].clone()).or_insert_with(|| type_shape(ty));
            }
        }

        // Phase 3: the import scope. Each unqualified edge contributes the
        // child's surface, minus what the edge's forms leave out. A name
        // this module hides on a DIRECT import of its defining module does
        // not come back through another import that merely passes it on.
        // Everything an edge could have named but does not make visible
        // is remembered as `hidden`, for the diagnostic on a bare use.
        let direct_hidden = |origin: &str, n: &str| -> bool {
            edges.iter().any(|d| d.key == origin
                && !d.unqual.is_empty()
                && hidden_by_forms(&d.unqual, n))
        };
        let visible_through = |e: &Edge, origin: &str, n: &str| -> bool {
            !e.unqual.is_empty() && !hidden_by_forms(&e.unqual, n) && !direct_hidden(origin, n)
        };
        let mut imp_vals: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
        let mut imp_tys: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
        // name -> the module whose import form leaves it out (None: the
        // imported module itself keeps it back).
        let mut hidden: HashMap<String, Option<String>> = HashMap::new();
        let mut region_globals: HashSet<String> = HashSet::new();
        for (via, e) in edges.iter().enumerate() {
            for d in &e.decls {
                region_globals.extend(global_value_names(d));
            }
            for n in &e.surface.withheld {
                hidden.entry(n.clone()).or_insert(None);
            }
            // An import (or hiding) list may only name entities the child
            // actually exports (Haskell 2010 §5.3.1; GHC: "Module 'A' does
            // not export 'f'"). Only a list-carrying child is checked: a
            // list-less module offers its whole import closure, mata-ll's
            // long-standing behavior. Constructors, record fields and
            // class methods are global names that travel with their type
            // or class, so they always count as offered.
            if e.listed {
                let mut offered: HashSet<String> = e.surface.vals.keys()
                    .chain(e.surface.tys.keys())
                    .chain(e.surface.classes.iter())
                    .cloned()
                    .collect();
                for d in &e.decls {
                    offered.extend(decl_constructors(d));
                    offered.extend(global_value_names(d));
                }
                for form in &e.unqual {
                    let (items, hiding) = match form {
                        ImportItems::Specific(items) => (items, false),
                        ImportItems::Hiding(items) => (items, true),
                        _ => continue,
                    };
                    let mut names: Vec<String> = item_names(items).into_iter().collect();
                    names.sort();
                    for n in names {
                        if offered.contains(&n) { continue; }
                        let key = &e.key;
                        if hiding {
                            // Hiding what isn't offered is harmless;
                            // GHC accepts it too (a -Wdodgy-imports
                            // warning, not an error).
                            self.warnings.push(crate::types::Diagnostic {
                                kind: crate::types::DiagnosticKind::Other(
                                    format!("module '{key}' does not \
                                             export '{n}'")),
                                context: None,
                                span: None,
                                file: None,
                                notes: vec![format!(
                                    "the 'hiding' list of 'import {key}' \
                                     names '{n}', which '{key}''s export \
                                     list does not offer — there is \
                                     nothing to hide. Remove it from the \
                                     list (GHC warns likewise, under \
                                     -Wdodgy-imports).")],
                                baseline: false, excerpt: None, names: None,
                            });
                            continue;
                        }
                        return Err(format!(
                            "module '{key}' does not export '{n}'\n\
                             An import list can only request names the \
                             imported module's export list offers: its \
                             listed entities plus everything its \
                             'module X' re-export entries pass on. Add \
                             '{n}' to '{key}''s export list, or import \
                             '{n}' from its defining module directly \
                             (GHC rejects this import the same way)."));
                    }
                }
            }
            if e.unqual.is_empty() {
                continue;
            }
            for (names, out) in [(&e.surface.vals, &mut imp_vals), (&e.surface.tys, &mut imp_tys)] {
                for (n, ents) in names {
                    for ent in ents {
                        if !visible_through(e, &ent.origin, n) {
                            hidden.entry(n.clone()).or_insert_with(|| Some(e.key.clone()));
                            continue;
                        }
                        let cands = out.entry(n.clone()).or_default();
                        if !cands.iter().any(|c| c.entity.canon == ent.canon) {
                            cands.push(Candidate { entity: ent.clone(), via });
                        }
                    }
                }
            }
        }

        // Phase 4: the Prelude's share of the scope. Without an explicit
        // `import Prelude` every Prelude name is visible; with one (or
        // several), exactly what the unqualified forms admit — none, when
        // the Prelude is only imported qualified (Haskell 2010 §5.6.1).
        let prelude_restricted = !prelude_forms.is_empty();
        let prelude_unqual: Vec<ImportItems> = prelude_forms.iter()
            .filter(|f| !matches!(f, ImportItems::Qualified(_)))
            .cloned()
            .collect();
        let prelude_visible = |n: &str| -> bool {
            !prelude_restricted
                || (!prelude_unqual.is_empty() && !hidden_by_forms(&prelude_unqual, n))
        };

        // An unqualified import that brings in a name the Prelude also
        // provides, AT A DIFFERENT TYPE, is rejected here with the fix
        // spelled out: every bare use of the name would otherwise mean the
        // import's declaration, which is rarely what the importer intends
        // (`import Data.Map` and then `null xs` on a list). GHC reports the
        // same clash as an ambiguity where the name is used. At the SAME
        // type shape the import's declaration is taken as this module's
        // replacement for the Prelude's (an FFI re-declaration like
        // `sqrt`, a library's own copy of a Prelude function) and simply
        // shadows it in this module.
        let mut collisions: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for (n, cands) in &imp_vals {
            if own_vals.contains_key(n) || own_globals.contains(n) || !prelude_visible(n) {
                continue;
            }
            let Some(prelude_shape) = self.prelude.shapes.get(n) else { continue };
            for c in cands {
                if c.entity.origin != PRELUDE_KEY
                    && self.shapes.get(&c.entity.canon).is_some_and(|s| s != prelude_shape)
                {
                    collisions.entry(c.via).or_default().push(n.clone());
                }
            }
        }
        if let Some((via, names)) = collisions.into_iter().next() {
            let importer = (!is_root).then(|| self.module_label(own_key));
            return Err(collision_error(&edges[via].key, &names, importer.as_deref()));
        }

        // Phase 5: the bare-name tables the rewrite of this module's own
        // source consults — bare name -> the spelling to put in its place.
        // A name absent from a table is left as written: it is a local
        // binder's business, a Prelude or global name, or unknown (which
        // the type checker reports).
        let placeholders = Placeholders::new(self.scope_errors.len());
        let mut vals: HashMap<String, String> = HashMap::new();
        let mut tys: HashMap<String, String> = HashMap::new();
        let mut cons: HashMap<String, String> = HashMap::new();
        {
            let mut names: BTreeSet<&str> = BTreeSet::new();
            names.extend(own_vals.keys().map(String::as_str));
            names.extend(imp_vals.keys().map(String::as_str));
            names.extend(hidden.keys().map(String::as_str).filter(|n| !is_type_spelling(n)));
            if prelude_restricted {
                names.extend(self.prelude.values.iter().map(String::as_str));
            }
            if !is_root {
                names.extend(self.root_vals.iter().map(String::as_str));
            }
            for n in names {
                let target = if let Some(canon) = own_vals.get(n) {
                    Some(canon.clone())
                } else if own_globals.contains(n) {
                    None
                } else if let Some(cands) = imp_vals.get(n) {
                    Some(match cands.as_slice() {
                        [one] => one.entity.canon.clone(),
                        many => placeholders.make(ambiguous_occurrence(n, many, &edges)),
                    })
                } else if region_globals.contains(n) {
                    None
                } else if self.prelude.values.contains(n) {
                    if prelude_visible(n) || DESUGARED_TO.contains(&n) {
                        None
                    } else {
                        Some(placeholders.make(prelude_hidden(n)))
                    }
                } else if let Some(by) = hidden.get(n) {
                    Some(placeholders.make(not_exported(n, by.as_deref())))
                } else if !is_root && self.root_vals.contains(n) {
                    // The entry module's declaration of this name is not
                    // in scope in a module it imports.
                    Some(placeholders.make(ScopeError::Unbound(n.to_string())))
                } else {
                    None
                };
                if let Some(t) = target
                    && t != n
                {
                    vals.insert(n.to_string(), t);
                }
            }
        }
        {
            let mut names: BTreeSet<&str> = BTreeSet::new();
            names.extend(own_tys.keys().map(String::as_str));
            names.extend(imp_tys.keys().map(String::as_str));
            names.extend(hidden.keys().map(String::as_str).filter(|n| is_type_spelling(n)));
            if prelude_restricted {
                names.extend(self.prelude.types.iter().map(String::as_str));
            }
            if !is_root {
                names.extend(self.root_tys.iter().map(String::as_str));
            }
            for n in names {
                let target = if let Some(canon) = own_tys.get(n) {
                    Some(canon.clone())
                } else if let Some(cands) = imp_tys.get(n) {
                    Some(match cands.as_slice() {
                        [one] => one.entity.canon.clone(),
                        many => placeholders.make(ambiguous_occurrence(n, many, &edges)),
                    })
                } else if self.prelude.types.contains(n) {
                    if prelude_visible(n) {
                        None
                    } else {
                        Some(placeholders.make(ScopeError::UnknownType(n.to_string())))
                    }
                } else if let Some(by) = hidden.get(n) {
                    Some(placeholders.make(not_exported(n, by.as_deref())))
                } else if !is_root && self.root_tys.contains(n) {
                    Some(placeholders.make(ScopeError::UnknownType(n.to_string())))
                } else {
                    None
                };
                if let Some(t) = target
                    && t != n
                {
                    tys.insert(n.to_string(), t);
                }
            }
        }

        // Phase 6: the alias tables. `A.name` names what the modules
        // imported as `A` EXPORT — their surfaces — and nothing else: not
        // their private names, and not a declaration of some other module
        // that happens to share the spelling. Two modules under one alias
        // offering different entities for one name make `A.name`
        // ambiguous, reported where it is used. Constructors, record
        // fields and class methods are global names; `A.C` / `A.field`
        // name them for every declaration in the module's region.
        let mut alias_table: BTreeMap<String, AliasNames> = BTreeMap::new();
        for e in &edges {
            for alias in &e.aliases {
                let entry = alias_table.entry(alias.clone()).or_default();
                for (names, out) in [(&e.surface.vals, &mut entry.vals), (&e.surface.tys, &mut entry.tys)] {
                    for (n, ents) in names {
                        let slot = out.entry(n.clone()).or_default();
                        for ent in ents {
                            if !slot.iter().any(|x| x.canon == ent.canon) {
                                slot.push(ent.clone());
                            }
                        }
                    }
                }
                for d in &e.decls {
                    entry.globals.extend(global_value_names(d));
                    entry.cons.extend(decl_constructors(d));
                }
            }
        }
        for form in &prelude_forms {
            if let ImportItems::Qualified(alias) = form {
                let entry = alias_table.entry(alias.clone()).or_default();
                for (names, out) in [(&self.prelude.values, &mut entry.vals), (&self.prelude.types, &mut entry.tys)] {
                    for n in names {
                        let slot = out.entry(n.clone()).or_default();
                        if !slot.iter().any(|x| x.canon == *n) {
                            slot.push(Entity { canon: n.clone(), origin: PRELUDE_KEY.to_string() });
                        }
                    }
                }
                entry.cons.extend(self.prelude.constructors.iter().cloned());
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
            .chain(edges.iter().flat_map(|e| e.decls.iter()))
            .flat_map(decl_constructors)
            .collect();
        let qualified_aliases: HashSet<String> = alias_table.keys()
            .filter(|a| {
                let keep = !ctor_names.contains(*a);
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
                        baseline: false, excerpt: None, names: None,
                    });
                }
                keep
            })
            .cloned()
            .collect();
        // Use sites in this module: `A.f` (collapsed from the field-access
        // shape), `A.T` and `A.C` resolve through the alias table. A
        // qualified TYPE or CONSTRUCTOR is unambiguous, so it resolves
        // through every alias, including one a constructor shadows at the
        // value level. A qualified name the table does not hold is given
        // no entry here; the rewrite reports it as unbound (`Rename::v`,
        // `Rename::t`) rather than leave a spelling that could coincide
        // with some module's canonical name.
        for (alias, names) in &alias_table {
            for (n, ents) in &names.tys {
                tys.insert(format!("{alias}.{n}"), match ents.as_slice() {
                    [one] => one.canon.clone(),
                    many => placeholders.make(ambiguous_alias(alias, n, many)),
                });
            }
            for c in &names.cons {
                cons.insert(format!("{alias}.{c}"), c.clone());
            }
            if qualified_aliases.contains(alias) {
                for g in &names.globals {
                    vals.insert(format!("{alias}.{g}"), g.clone());
                }
                for (n, ents) in &names.vals {
                    vals.insert(format!("{alias}.{n}"), match ents.as_slice() {
                        [one] => one.canon.clone(),
                        many => placeholders.make(ambiguous_alias(alias, n, many)),
                    });
                }
            }
        }

        // Phase 7: rewrite this module's own declarations to canonical
        // spelling, binding every reference in the scope built above.
        let alias_names: HashSet<String> = alias_table.keys().cloned().collect();
        let rename = Rename {
            vals: &vals, tys: &tys, cons: &cons,
            aliases: &alias_names, placeholders: &placeholders,
        };
        let mut own_decls: Vec<Decl> = Vec::new();
        for decl in &module.decls {
            if matches!(decl, Decl::Import { .. }) {
                continue;
            }
            let d = if qualified_aliases.is_empty() {
                decl.clone()
            } else {
                rewrite_qualified_uses_decl(decl.clone(), &qualified_aliases)
            };
            own_decls.push(rename.decl(&d));
        }

        // Phase 8: what this module offers its importers. Without an
        // export list: every own declaration plus everything its
        // unqualified imports make visible here (mata-ll's long-standing
        // pass-through; GHC exports the own declarations only). With one:
        // the listed own declarations (all of them under the self
        // re-export), each listed name that resolves to an import or to
        // the Prelude, and what the `module X` entries' import edges make
        // visible. Everything else the module declares or sees is
        // `withheld`: a bare use in an importer is told it is not
        // exported, instead of plain "unbound".
        let own_entity = |canon: &String| vec![Entity {
            canon: canon.clone(), origin: own_key.to_string(),
        }];
        let entities = |cands: &Vec<Candidate>| -> Vec<Entity> {
            cands.iter().map(|c| c.entity.clone()).collect()
        };
        let mut surface = Surface::default();
        match &module.exports {
            None => {
                for (n, canon) in &own_vals {
                    surface.vals.insert(n.clone(), own_entity(canon));
                }
                for (n, canon) in &own_tys {
                    surface.tys.insert(n.clone(), own_entity(canon));
                }
                for (n, cands) in &imp_vals {
                    if !own_globals.contains(n) {
                        surface.vals.entry(n.clone()).or_insert_with(|| entities(cands));
                    }
                }
                for (n, cands) in &imp_tys {
                    surface.tys.entry(n.clone()).or_insert_with(|| entities(cands));
                }
                surface.classes.extend(own_classes.iter().cloned());
                for e in edges.iter().filter(|e| !e.unqual.is_empty()) {
                    surface.classes.extend(e.surface.classes.iter().cloned());
                }
            }
            Some(ex) => {
                let listed: HashSet<&str> = ex.names.iter().map(String::as_str).collect();
                for (n, canon) in &own_vals {
                    if self_export || listed.contains(n.as_str()) {
                        surface.vals.insert(n.clone(), own_entity(canon));
                    }
                }
                for (n, canon) in &own_tys {
                    if self_export || listed.contains(n.as_str()) {
                        surface.tys.insert(n.clone(), own_entity(canon));
                    }
                }
                for c in &own_classes {
                    if self_export || listed.contains(c.as_str()) {
                        surface.classes.insert(c.clone());
                    }
                }
                let prelude_entity = |n: &String| vec![Entity {
                    canon: n.clone(), origin: PRELUDE_KEY.to_string(),
                }];
                for n in &ex.names {
                    if own_vals.contains_key(n) || own_tys.contains_key(n)
                        || own_globals.contains(n) || own_classes.contains(n)
                    {
                        continue;
                    }
                    if let Some(cands) = imp_vals.get(n) {
                        surface.vals.insert(n.clone(), entities(cands));
                    } else if let Some(cands) = imp_tys.get(n) {
                        surface.tys.insert(n.clone(), entities(cands));
                    } else if self.prelude.values.contains(n) && !region_globals.contains(n) {
                        surface.vals.insert(n.clone(), prelude_entity(n));
                    } else if self.prelude.types.contains(n) {
                        surface.tys.insert(n.clone(), prelude_entity(n));
                    }
                    if edges.iter().any(|e| e.surface.classes.contains(n)) {
                        surface.classes.insert(n.clone());
                    }
                }
                for &i in &reexported_edges {
                    let e = &edges[i];
                    for (names, out) in [(&e.surface.vals, &mut surface.vals), (&e.surface.tys, &mut surface.tys)] {
                        for (n, ents) in names {
                            for ent in ents {
                                if !visible_through(e, &ent.origin, n) {
                                    continue;
                                }
                                let slot = out.entry(n.clone()).or_default();
                                if !slot.iter().any(|x| x.canon == ent.canon) {
                                    slot.push(ent.clone());
                                }
                            }
                        }
                    }
                    surface.classes.extend(e.surface.classes.iter().cloned());
                }
                for n in own_vals.keys().chain(imp_vals.keys()) {
                    if !surface.vals.contains_key(n) {
                        surface.withheld.insert(n.clone());
                    }
                }
                for n in own_tys.keys().chain(imp_tys.keys()) {
                    if !surface.tys.contains_key(n) {
                        surface.withheld.insert(n.clone());
                    }
                }
            }
        }

        // Merge: imported regions first — each origin ONCE, however many
        // edges carry it (a diamond's shared module arrives through every
        // path) — then own. Every region is already canonical, so nothing
        // is renamed here. The own span is keyed by this module's import
        // key so a PARENT merging this resolved module can recognize these
        // declarations when they also arrive via a sibling.
        let mut decls: Vec<Decl> = Vec::new();
        let mut out_spans: Vec<(String, usize)> = Vec::new();
        let mut merged_origins: HashSet<String> = HashSet::new();
        for e in &edges {
            for (okey, slice) in runs(&e.spans, &e.decls) {
                if merged_origins.insert(okey.to_string()) {
                    decls.extend(slice.iter().cloned());
                    out_spans.push((okey.to_string(), slice.len()));
                }
            }
        }
        out_spans.push((own_key.to_string(), own_decls.len()));
        decls.extend(own_decls);
        self.scope_errors.extend(placeholders.into_made());
        Ok(Resolved {
            module: Module {
                decls,
                name: module.name.clone(),
                exports: None,
                resolution: Resolution::default(),
                origin_spans: out_spans,
            },
            surface,
        })
    }

    /// How a diagnostic names module `key`: its file where it has one.
    fn module_label(&self, key: &str) -> String {
        match self.module_sources.get(key) {
            Some((display, _)) => format!("'{key}' ({display})"),
            None => format!("'{key}'"),
        }
    }

    /// The compiled program returns ONE table to the Lua host, keyed by
    /// the names its `export` declarations carry in their own modules. Two
    /// modules exporting one name would overwrite each other's entry.
    fn check_host_exports(&self, program: &Module) -> Result<(), String> {
        let mut seen: HashMap<String, String> = HashMap::new();
        for (okey, slice) in runs(&program.origin_spans, &program.decls) {
            for d in slice {
                let Decl::ExportSig { name, .. } = d else { continue };
                let host = self.source_names.get(name).unwrap_or(name).clone();
                let here = if okey == ROOT_KEY { "this file".to_string() } else { format!("module '{okey}'") };
                if let Some(prev) = seen.get(&host) {
                    if *prev != here {
                        return Err(format!(
                            "'{host}' is exported to the Lua host by both {prev} and {here}\n\
                             The compiled program returns one table with one entry \
                             per exported name, so two 'export {host}' declarations \
                             cannot both be honoured. Rename one of them, or drop \
                             one of the 'export' declarations."));
                    }
                } else {
                    seen.insert(host, here);
                }
            }
        }
        Ok(())
    }
}

/// The module key the entry module is resolved under. Not a legal module
/// name, so it cannot collide with an imported module's key.
const ROOT_KEY: &str = "//root";

/// Origin key of everything the implicit Prelude provides (its source
/// declarations and the compiler's builtins). Their canonical names are
/// their bare names.
const PRELUDE_KEY: &str = "Prelude";

/// The Prelude functions the parser spells its desugarings with — a list
/// comprehension, an arithmetic sequence, a failed pattern match are plain
/// references to these by the time resolution sees the module. Such a
/// reference is not one the module wrote, so an explicit `import Prelude`
/// form cannot take these names out of scope.
const DESUGARED_TO: &[&str] =
    &["concatMap", "enumFrom", "enumFromTo", "enumFromThen", "enumFromThenTo", "error"];

/// What the implicit Prelude contributes to every module's scope, as far
/// as name resolution is concerned. Built by the compile pipeline from the
/// Prelude source and the type checker's builtin environment.
#[derive(Default, Clone)]
pub struct PreludeScope {
    /// Every value name: source definitions, builtins, class methods.
    pub values: HashSet<String>,
    /// Every type name: source declarations and builtin types.
    pub types: HashSet<String>,
    /// Every data constructor.
    pub constructors: HashSet<String>,
    /// Type shape of each signed value (see `signature_shapes`), the
    /// baseline unqualified imports are checked against.
    pub shapes: HashMap<String, String>,
}

/// One top-level value or type declaration, by its canonical name and the
/// module that declares it.
#[derive(Clone, Debug)]
struct Entity {
    canon: String,
    origin: String,
}

/// What a module offers its importers, keyed by the bare name the
/// importer writes. A name normally maps to one entity; it maps to several
/// when the module passes on a name that is ambiguous in its own scope.
#[derive(Clone, Default)]
struct Surface {
    vals: BTreeMap<String, Vec<Entity>>,
    tys: BTreeMap<String, Vec<Entity>>,
    /// Class names offered (classes are global; this only feeds the
    /// import-list check).
    classes: BTreeSet<String>,
    /// Names the module declares or sees but keeps back (private to its
    /// export list, or not passed on by it).
    withheld: BTreeSet<String>,
}

/// A resolved module as the cache holds it: its region in canonical
/// spelling, and its surface.
#[derive(Clone)]
struct Resolved {
    module: Module,
    surface: Surface,
}

/// An entity visible in the importing module's bare scope, with the import
/// edge (index into the module's edges) that made it visible.
struct Candidate {
    entity: Entity,
    via: usize,
}

/// The placeholder spellings minted while one module is rewritten, each
/// with the error its use reports. A placeholder contains characters no
/// source name can (`<`, a space), so no binder and no declaration can
/// share its spelling.
struct Placeholders {
    base: usize,
    made: std::cell::RefCell<Vec<(String, ScopeError)>>,
}

impl Placeholders {
    /// `base`: how many placeholders the program holds already, so the
    /// numbering stays unique across modules.
    fn new(base: usize) -> Self {
        Placeholders { base, made: std::cell::RefCell::new(Vec::new()) }
    }

    fn make(&self, err: ScopeError) -> String {
        let mut made = self.made.borrow_mut();
        let spelling = format!("<scope error {}>", self.base + made.len());
        made.push((spelling.clone(), err));
        spelling
    }

    fn into_made(self) -> Vec<(String, ScopeError)> {
        self.made.into_inner()
    }
}

/// Does `module` see the Prelude's `name` unqualified? Without an explicit
/// `import Prelude` every Prelude name is visible; with one or several,
/// exactly what the unqualified forms admit (Haskell 2010 §5.6.1).
pub fn prelude_name_visible(module: &Module, name: &str) -> bool {
    let forms: Vec<&ImportItems> = module.decls.iter()
        .filter_map(|d| match d {
            Decl::Import { module_path, items }
                if module_path.len() == 1 && module_path[0] == PRELUDE_KEY => Some(items),
            _ => None,
        })
        .collect();
    if forms.is_empty() {
        return true;
    }
    let unqual: Vec<ImportItems> = forms.into_iter()
        .filter(|f| !matches!(f, ImportItems::Qualified(_)))
        .cloned()
        .collect();
    !unqual.is_empty() && !hidden_by_forms(&unqual, name)
}

/// Is `name` spelled like a type (capitalized, or a `:`-operator) rather
/// than like a value? The two namespaces never share a spelling.
fn is_type_spelling(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_uppercase() || c == ':')
}

/// The import paths a parsed module names.
fn import_paths_of(module: &Module) -> Vec<Vec<String>> {
    module.decls.iter()
        .filter_map(|d| match d {
            Decl::Import { module_path, .. } => Some(module_path.clone()),
            _ => None,
        })
        .collect()
}

/// The value-level names a declaration puts in the GLOBAL namespace:
/// record fields (their accessor functions) and class methods.
fn global_value_names(decl: &Decl) -> Vec<String> {
    let mut out = con_record_fields(decl);
    if let Decl::ClassDecl { methods, .. } = decl {
        out.extend(methods.iter().map(|m| m.name.clone()));
    }
    out
}

fn module_phrase(origin: &str) -> String {
    if origin == PRELUDE_KEY { "the Prelude".to_string() } else { format!("module '{origin}'") }
}

/// Two or more different imported entities share the bare name `n`.
fn ambiguous_occurrence(n: &str, cands: &[Candidate], edges: &[Edge]) -> ScopeError {
    let who: Vec<String> = cands.iter()
        .map(|c| format!("the '{n}' of {}", module_phrase(&c.entity.origin)))
        .collect();
    let mut notes = vec![format!(
        "more than one import brings a '{n}' into scope here, so a bare '{n}' has \
         no single meaning (GHC reports the same ambiguity). Keep one of them out \
         with an import list or 'hiding ({n})', or import one of the modules \
         'qualified' and write its '{n}' with the alias.")];
    for c in cands {
        let e = &edges[c.via];
        if c.entity.origin != e.key && c.entity.origin != PRELUDE_KEY && !e.listed {
            notes.push(format!(
                "the '{n}' of module '{}' arrives through 'import {}': a module \
                 without an export list passes the names it imports on to its \
                 importers. GHC exports only the module's own declarations in that \
                 case; an export list on '{}' stops the pass-through.",
                c.entity.origin, e.key, e.key));
        }
    }
    ScopeError::Explained {
        name: n.to_string(),
        message: format!("Ambiguous occurrence '{n}': it could refer to {}", who.join(" or to ")),
        notes,
    }
}

/// Two modules imported under one alias offer different entities named `n`.
fn ambiguous_alias(alias: &str, n: &str, ents: &[Entity]) -> ScopeError {
    let who: Vec<String> = ents.iter()
        .map(|e| format!("the '{n}' of {}", module_phrase(&e.origin)))
        .collect();
    ScopeError::Explained {
        name: format!("{alias}.{n}"),
        message: format!(
            "Ambiguous occurrence '{alias}.{n}': it could refer to {}", who.join(" or to ")),
        notes: vec![format!(
            "several modules are imported under the alias '{alias}', and more than \
             one of them offers a '{n}' (GHC reports the same ambiguity). Give one \
             of the imports its own alias.")],
    }
}

/// A bare use of a name an import could have provided but does not.
/// `by`: the module whose import form leaves the name out, when it is the
/// IMPORT that does; `None` when the imported module itself keeps it back.
fn not_exported(n: &str, by: Option<&str>) -> ScopeError {
    let note = match by {
        Some(module) => format!(
            "module '{module}' offers '{n}', but the import of '{module}' in this \
             module leaves it out (it is absent from the import list, or named in \
             'hiding'). Add it to the import, or use a qualified import."),
        None => format!(
            "an imported module defines or imports '{n}' but its export list does \
             not pass it on, so it is private to that module."),
    };
    ScopeError::Explained {
        name: n.to_string(),
        message: format!("'{n}' is not exported by its module"),
        notes: vec![note],
    }
}

/// A bare use of a Prelude name this module's `import Prelude` excludes.
fn prelude_hidden(n: &str) -> ScopeError {
    ScopeError::Explained {
        name: n.to_string(),
        message: format!("'{n}' is not in scope"),
        notes: vec![format!(
            "the Prelude provides '{n}', but this module's explicit 'import Prelude' \
             leaves it out (it is named in 'hiding', absent from the import list, \
             or the Prelude is only imported qualified).")],
    }
}

/// One import edge of the module being resolved: the imported module, the
/// forms it is imported through, its resolved region — the child's
/// declarations (its own and its imports') with the origin runs that
/// partition them, in canonical spelling — and the surface it offers.
struct Edge {
    key: String,
    unqual: Vec<ImportItems>,
    aliases: Vec<String>,
    decls: Vec<Decl>,
    spans: Vec<(String, usize)>,
    surface: Surface,
    /// The imported module carries a header export list.
    listed: bool,
}

/// What an alias names: the surfaces of the modules imported under it, by
/// namespace, plus the global names declared anywhere in their regions.
#[derive(Default)]
struct AliasNames {
    vals: BTreeMap<String, Vec<Entity>>,
    tys: BTreeMap<String, Vec<Entity>>,
    /// Record fields and class methods: `A.field` names `field`.
    globals: BTreeSet<String>,
    /// Constructors are global names: `A.C` names `C`.
    cons: BTreeSet<String>,
}

/// The origin runs of a region: `(origin key, declarations)` slices.
fn runs<'a>(spans: &'a [(String, usize)], decls: &'a [Decl]) -> Vec<(&'a str, &'a [Decl])> {
    let mut out = Vec::with_capacity(spans.len());
    let mut idx = 0;
    for (okey, len) in spans {
        out.push((okey.as_str(), &decls[idx..idx + len]));
        idx += len;
    }
    debug_assert_eq!(idx, decls.len(), "origin spans cover the decl list");
    out
}

/// The declarations that are ENTITIES of the resolution model (see
/// `resolve_module`): value bindings (function, signature, export
/// signature — `false`) and type-level names (data, newtype, alias, family
/// — `true`). Constructors, classes, instances and class methods are
/// global names and never renamed.
fn renamable(decl: &Decl) -> Option<(&str, bool)> {
    match decl {
        Decl::FunDef { name, .. } | Decl::TypeSig { name, .. } | Decl::ExportSig { name, .. } =>
            Some((name, false)),
        Decl::DataDef { name, .. } | Decl::NewtypeDef { name, .. }
        | Decl::TypeAlias { name, .. } | Decl::TypeFamily { name, .. } => Some((name, true)),
        _ => None,
    }
}

/// Record field names declared by a data or newtype declaration.
fn con_record_fields(decl: &Decl) -> Vec<String> {
    match decl {
        Decl::DataDef { constructors, .. } => constructors.iter()
            .flat_map(|c| match &c.fields {
                ConstructorFields::Named(fs) => fs.iter().map(|f| f.name.clone()).collect(),
                ConstructorFields::Positional(_) => Vec::new(),
            })
            .collect(),
        Decl::NewtypeDef { field: Some(f), .. } => vec![f.clone()],
        _ => Vec::new(),
    }
}

fn item_names(items: &[ImportItem]) -> HashSet<String> {
    items.iter().map(|item| match item {
        ImportItem::Value(n) => n.clone(),
        ImportItem::TypeAll(n) => n.clone(),
        ImportItem::TypeOnly(n) => n.clone(),
    }).collect()
}

/// A name is selection-hidden only when EVERY unqualified form hides it: a
/// Specific list hides what it doesn't request, a Hiding list hides what it
/// excludes, and a plain `import M` hides nothing. (No forms hide nothing.)
fn hidden_by_forms(forms: &[ImportItems], n: &str) -> bool {
    !forms.is_empty() && forms.iter().all(|form| match form {
        ImportItems::All => false,
        ImportItems::Specific(items) => !item_names(items).contains(n),
        ImportItems::Hiding(items) => item_names(items).contains(n),
        ImportItems::Qualified(_) => unreachable!("qualified forms are split off"),
    })
}

/// Rewrites one module's own declarations from source spelling to
/// canonical spelling: declared names become their canonical names, and
/// every reference is bound in the module's scope — scope-aware, so a
/// reference is rewritten only when it names a module-level binding, never
/// a local that shadows it. The maps hold, per bare (or alias-qualified)
/// source name, the spelling to put in its place: the canonical name of
/// the entity it denotes, or a placeholder when it denotes none (see
/// `Placeholders`). Constructors, classes, instances and class methods are
/// global names and are never touched.
struct Rename<'a> {
    vals: &'a HashMap<String, String>,
    tys: &'a HashMap<String, String>,
    /// Qualified constructor spellings (`A.C`) to the global constructor.
    cons: &'a HashMap<String, String>,
    /// The module's import aliases: what makes `A.name` a qualified name.
    aliases: &'a HashSet<String>,
    placeholders: &'a Placeholders,
}

impl Rename<'_> {
    fn v(&self, name: &str, bound: &HashSet<String>) -> String {
        if bound.contains(name) {
            return name.to_string();
        }
        let qualified = self.is_alias_qualified(name);
        match self.vals.get(name) {
            // A qualified reference that resolves to a bare name (a
            // Prelude function or a global record field / class method
            // reached through an alias) under a local binder of that very
            // name: written out bare, the reference would be captured by
            // the binder and silently mean the local.
            Some(target) if qualified && bound.contains(target) => {
                self.placeholders.make(ScopeError::Explained {
                    name: name.to_string(),
                    message: format!(
                        "'{name}' cannot be used here: the local binding '{target}' \
                         shadows the name it refers to"),
                    notes: vec![format!(
                        "mata-ll resolves '{name}' to the plain name '{target}', which \
                         the enclosing local binding of the same name would capture. \
                         GHC accepts this; rename the local binding.")],
                })
            }
            Some(target) => target.clone(),
            // `A.name` where the alias `A` offers no `name`. Left as
            // written it could coincide with the canonical name of some
            // module `A`'s private declaration; it is unbound.
            None if qualified => self.placeholders.make(ScopeError::Unbound(name.to_string())),
            None => name.to_string(),
        }
    }

    /// Is `name` an alias-qualified value spelling (`A.f`, as collapsed by
    /// `rewrite_qualified_uses_decl`)? An alias is a single identifier, so
    /// the first dot ends it.
    fn is_alias_qualified(&self, name: &str) -> bool {
        name.split_once('.')
            .is_some_and(|(head, rest)| !rest.is_empty() && self.aliases.contains(head))
    }

    /// A declared value name: never shadowed.
    fn dv(&self, name: &str) -> String {
        self.vals.get(name).cloned().unwrap_or_else(|| name.to_string())
    }

    fn t(&self, name: &str) -> String {
        if let Some(target) = self.tys.get(name) {
            return target.clone();
        }
        // A module-qualified type name (`M.Map`: the parser spells it as
        // one dotted name) the scope does not hold. Left as written it
        // could coincide with a canonical type name; it is unknown. A
        // type-level string literal (`"math.sqrt"`) is not a name.
        let qualified = name.split_once('.').is_some_and(|(head, rest)| {
            !rest.is_empty() && head.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        });
        if qualified {
            return self.placeholders.make(ScopeError::UnknownType(name.to_string()));
        }
        name.to_string()
    }

    fn c(&self, name: &str) -> String {
        self.cons.get(name).cloned().unwrap_or_else(|| name.to_string())
    }

    fn pattern(&self, p: &Pattern) -> Pattern {
        if self.cons.is_empty() {
            return p.clone();
        }
        match p {
            Pattern::Constructor { name, args } => Pattern::Constructor {
                name: self.c(name),
                args: args.iter().map(|a| self.pattern(a)).collect(),
            },
            Pattern::Paren(inner) => Pattern::Paren(Box::new(self.pattern(inner))),
            Pattern::Tuple(elems) => Pattern::Tuple(elems.iter().map(|e| self.pattern(e)).collect()),
            Pattern::As(n, inner) => Pattern::As(n.clone(), Box::new(self.pattern(inner))),
            Pattern::Var(_) | Pattern::Wildcard | Pattern::LitPat(_) => p.clone(),
        }
    }

    fn decl(&self, decl: &Decl) -> Decl {
        match decl {
            Decl::TypeSig { name, ty } => Decl::TypeSig {
                name: self.dv(name), ty: self.ty(ty),
            },
            Decl::FunDef { name, clauses } => Decl::FunDef {
                name: self.dv(name),
                clauses: clauses.iter().map(|c| self.clause(c)).collect(),
            },
            Decl::ExportSig { name, ty } => Decl::ExportSig {
                name: self.dv(name), ty: self.ty(ty),
            },
            Decl::DataDef { name, type_vars, constructors, deriving } => Decl::DataDef {
                name: self.t(name),
                type_vars: type_vars.clone(),
                // Rewrite field types (sibling type references) but leave the
                // constructor names global.
                constructors: constructors.iter().map(|c| self.constructor(c)).collect(),
                deriving: deriving.clone(),
            },
            Decl::NewtypeDef { name, type_vars, con_name, field, inner, deriving } => Decl::NewtypeDef {
                name: self.t(name),
                type_vars: type_vars.clone(),
                // The constructor and selector are global names; the
                // deriving classes too.
                con_name: con_name.clone(),
                field: field.clone(),
                inner: self.ty(inner),
                deriving: deriving.clone(),
            },
            Decl::TypeAlias { name, params, ty } => Decl::TypeAlias {
                name: self.t(name), params: params.clone(), ty: self.ty(ty),
            },
            Decl::TypeFamily { name, params, equations } => Decl::TypeFamily {
                name: self.t(name),
                params: params.clone(),
                equations: equations.iter().map(|eq| TypeFamilyEq {
                    args: eq.args.iter().map(|t| self.ty(t)).collect(),
                    result: self.ty(&eq.result),
                }).collect(),
            },
            // Class and instance NAMES stay global (a qualified class name
            // can't be written at a use site anyway), but their heads and
            // bodies refer to the module's own types and values, which
            // may be renamed — so an instance head names the renamed type
            // and its method bodies (and class default bodies) call the
            // renamed siblings.
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
            existential_constraints: c.existential_constraints.iter()
                .map(|c| self.constraint(c)).collect(),
        }
    }

    fn clause(&self, c: &Clause) -> Clause {
        // Clause parameters and where-bound names shadow module-level names,
        // so references to them must not be renamed.
        let mut bound = HashSet::new();
        for p in &c.patterns { collect_pattern_vars(p, &mut bound); }
        for ld in &c.where_binds { bound.insert(ld.name.clone()); }
        Clause {
            patterns: c.patterns.iter().map(|p| self.pattern(p)).collect(),
            guards: c.guards.iter().map(|g| Guard {
                condition: self.expr(&g.condition, &bound),
                body: self.expr(&g.body, &bound),
            }).collect(),
            body: c.body.as_ref().map(|b| self.expr(b, &bound)),
            where_binds: c.where_binds.iter().map(|ld| self.localdef(ld, &bound)).collect(),
            span: c.span,
            // The parser lowers every raw (binding-qualifier) guard chain
            // before returning; nothing reaches module resolution with one.
            raw_guards: { debug_assert!(c.raw_guards.is_none(), "raw guards survived parsing"); None },
        }
    }

    fn localdef(&self, ld: &LocalDef, outer: &HashSet<String>) -> LocalDef {
        let mut bound = outer.clone();
        for p in &ld.patterns { collect_pattern_vars(p, &mut bound); }
        LocalDef {
            name: ld.name.clone(),
            patterns: ld.patterns.iter().map(|p| self.pattern(p)).collect(),
            body: self.expr(&ld.body, &bound),
            // A local signature names types exactly as a top-level one does
            // (qualified type names included), so it takes the same rewrite.
            sig: ld.sig.as_ref().map(|t| self.ty(t)),
        }
    }

    fn expr(&self, e: &Expr, bound: &HashSet<String>) -> Expr {
        match e {
            // The rename decisions: a name is rewritten only when it refers
            // to a module-level binding, not a local one.
            Expr::Var(n) => Expr::Var(self.v(n, bound)),
            // Constructors are global; only a qualified spelling changes.
            Expr::Con(n) => Expr::Con(self.c(n)),
            Expr::RecordCon { constructor, fields } => Expr::RecordCon {
                constructor: self.c(constructor),
                fields: fields.iter().map(|(f, e)| (f.clone(), self.expr(e, bound))).collect(),
            },
            // Backtick sections `(`f`)` carry a plain function name here.
            Expr::OpFunc(n) => Expr::OpFunc(self.v(n, bound)),
            // An InfixApp's op is a NAME too — a sibling operator (`a <+> b`)
            // or a sibling function used backtick-infix (`a `combine` b`).
            // Their DEFINITIONS are renamed like every value, so in-module
            // infix uses must follow; the uniform descent below only visits
            // subEXPRESSIONS, so without this arm the op stayed bare and
            // resolved to nothing ("undefined '<+>'").
            Expr::InfixApp { op, lhs, rhs } => Expr::InfixApp {
                op: self.v(op, bound),
                lhs: Box::new(self.expr(lhs, bound)),
                rhs: Box::new(self.expr(rhs, bound)),
            },
            // Ascriptions carry a type; the generic descent visits only
            // expressions, and type names need renaming too.
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
                        pattern: self.pattern(&br.pattern),
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
            // Everything else neither names a binding nor binds anything:
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
                DoStmt::PatternBind { pattern: self.pattern(pattern), expr: e, span: *span }
            }
        }
    }

    fn ty(&self, t: &Type) -> Type {
        match t {
            Type::Con(n) => {
                Type::Con(self.t(n))
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
        raw_guards: { debug_assert!(c.raw_guards.is_none(), "raw guards survived parsing"); None },
    }
}

fn rewrite_uses_localdef(ld: LocalDef, aliases: &HashSet<String>) -> LocalDef {
    LocalDef {
        name: ld.name,
        patterns: ld.patterns,
        body: rewrite_uses_expr(ld.body, aliases),
        sig: ld.sig,
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

/// A structural fingerprint of a type as written, with variable names
/// erased. An unqualified import may shadow a Prelude name whose signature
/// has the same shape (an FFI re-declaration like `sqrt`, a library's own
/// copy of a Prelude function); a differing shape is the breaking clash
/// (`[a] -> Bool` vs `HashMap k v -> Bool` for two `null`s) the import
/// check rejects.
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
/// implementation depends on: the entry module may redefine such a name
/// only when it hides the Prelude's explicitly (see `lib.rs`).
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

/// The import-time rejection of an unqualified import whose names clash
/// with the Prelude's at a different type (see `resolve_module`, phase 4).
/// `importer`: the importing module, when it is not the entry module.
fn collision_error(module: &str, names: &[String], importer: Option<&str>) -> String {
    let alias = module.rsplit('.').next().unwrap_or(module);
    let listed: Vec<String> = names.iter()
        .map(|name| format!("'{}' (already defined by the Prelude)", name))
        .collect();
    let whence = importer.map(|m| format!("in module {m}: ")).unwrap_or_default();
    format!(
        "{whence}importing '{module}' unqualified conflicts with {}.\n\
         The import would give these names a second meaning at a different type, so \
         every bare use would be ambiguous (GHC reports that at the use; mata-ll \
         rejects the import). Import '{module}' qualified instead:\n\
         \n    import qualified {module} as {alias}\n\n\
         then refer to its members as `{alias}.name` (e.g. `{alias}.{}`).",
        listed.join(", "),
        names.first().map(String::as_str).unwrap_or("name"),
    )
}

/// Constructor names declared by a data or newtype declaration.
fn decl_constructors(decl: &Decl) -> Vec<String> {
    match decl {
        Decl::DataDef { constructors, .. } => constructors.iter().map(|c| c.name.clone()).collect(),
        Decl::NewtypeDef { con_name: Some(c), .. } => vec![c.clone()],
        _ => Vec::new(),
    }
}
