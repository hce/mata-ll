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
//! flat list can therefore hold two modules' `helper` (or `Node`, or
//! `Pretty`) side by side: they are `A.helper` and `B.helper`, and no
//! reference is ambiguous.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::fs;

use crate::ast::*;
use crate::fixity::{self, Fixities};
use crate::lexer;
use crate::parser;
use crate::types::Diagnostic;

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
    /// The fixity of every operator entity resolved so far, by canonical
    /// name: the Prelude's to begin with, then each module's own
    /// declarations as the module is resolved (`resolve_module`, phase 2).
    /// A module is resolved after its imports, so when its infix chains
    /// are grouped the table holds every entity its scope can name.
    fixities: Fixities,
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
    /// The module name the entry module's clashing declarations are
    /// qualified with (see `begin_program`).
    root_prefix: String,
    /// The entry module's declarations that keep their bare names, per
    /// namespace (values, types and classes, constructors). A bare name in
    /// the flat list that an IMPORTED module does not have in scope must
    /// not resolve to one of these.
    root_vals: HashSet<String>,
    root_tys: HashSet<String>,
    root_cons: HashSet<String>,
    /// The entry module's bare-named classes (a subset of `root_tys`), so a
    /// reference an imported module cannot make to one reports an unknown
    /// class rather than an unknown type.
    root_classes: HashSet<String>,
    /// The fields of every constructor resolved so far, by the
    /// constructor's canonical name: (source spelling, canonical name) in
    /// declaration order. A record construction `C { f = e }` names its
    /// fields through its constructor (`Rename`), wherever `C` is declared.
    con_fields: HashMap<String, Vec<(String, String)>>,
    /// The methods of every class resolved so far, by the class's
    /// canonical name, as (source spelling, canonical name). An instance
    /// declaration names the methods it defines through its class.
    class_methods: HashMap<String, Vec<(String, String)>>,
    /// The canonical name of every record field resolved so far, the
    /// Prelude's included: what a field selection's label may bind to
    /// (`Rename::field_selection`).
    fields: HashSet<String>,
    /// Program-wide by-products of resolution, handed to the type checker
    /// with the resolved entry module (see `ast::Resolution`).
    scope_errors: HashMap<String, ScopeError>,
    source_names: HashMap<String, String>,
    /// Signature shape of every signed value, by canonical name (for the
    /// import-against-Prelude check).
    shapes: HashMap<String, String>,
    /// The namespace of every canonical name minted for a type, class or
    /// constructor, and how many such declarations in the program share
    /// each source spelling in that namespace (for diagnostics' display
    /// names: two modules' `Config`, or `Node`, must stay distinguishable).
    canon_ns: HashMap<String, Ns>,
    spellings: HashMap<(Ns, String), usize>,
}

impl ModuleLoader {
    pub fn new(source_dir: &Path) -> Self {
        ModuleLoader {
            search_paths: vec![source_dir.to_path_buf()],
            loaded: HashMap::new(),
            resolved: HashMap::new(),
            in_progress: HashSet::new(),
            in_progress_stack: Vec::new(),
            fixities: fixity::prelude().clone(),
            warnings: Vec::new(),
            module_sources: HashMap::new(),
            prelude: PreludeScope::default(),
            root_prefix: String::new(),
            root_vals: HashSet::new(),
            root_tys: HashSet::new(),
            root_cons: HashSet::new(),
            root_classes: HashSet::new(),
            con_fields: HashMap::new(),
            class_methods: HashMap::new(),
            fields: HashSet::new(),
            scope_errors: HashMap::new(),
            source_names: HashMap::new(),
            shapes: HashMap::new(),
            canon_ns: HashMap::new(),
            spellings: HashMap::new(),
        }
    }

    /// Declare what the implicit Prelude provides. Must be called before
    /// `resolve_imports`: scopes, canonical names and the import check all
    /// depend on it.
    pub fn set_prelude(&mut self, prelude: PreludeScope) {
        self.fields.extend(prelude.fields.iter().cloned());
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
        // An imported module's syntax errors surface through the import-error
        // channel, prefixed with the module that failed to parse.
        let module = parser::parse(tokens)
            .map_err(|diags| syntax_errors_in_module(&key, &diags))?;

        self.loaded.insert(key.clone(), module);
        Ok(self.loaded.get(&key).unwrap())
    }

    /// Resolve the entry module: every import (transitively) is resolved
    /// and merged in front of the module's own declarations, and every
    /// reference is bound in the scope of the module that wrote it (see
    /// `resolve_module` for the model). The returned module carries the
    /// program's `Resolution` for the type checker.
    pub fn resolve_imports(&mut self, module: &Module) -> Result<Module, ResolveError> {
        self.begin_program(module);
        let mut resolved = self.resolve_module(module, ROOT_KEY)?.module;
        self.check_host_exports(&resolved)?;
        let mut display_names = HashMap::new();
        for (canon, base) in &self.source_names {
            // A type, class or constructor whose source spelling another
            // one of its namespace shares keeps its module-qualified form:
            // `A.Config` against `B.Config` (or against the Prelude's) must
            // stay distinguishable. A constructor is compared with the
            // program's only: one is rarely named next to a Prelude
            // constructor of its spelling, and `Err _` reads better than
            // `Main.Err _` in a witness.
            let shared = self.canon_ns.get(canon).is_some_and(|&ns| {
                let in_prelude = ns == Ns::Ty && self.prelude.types.contains(base);
                self.spellings.get(&(ns, base.clone())).copied().unwrap_or(0)
                    + usize::from(in_prelude) > 1
            });
            if !shared {
                display_names.insert(canon.clone(), base.clone());
            }
        }
        for (placeholder, err) in &self.scope_errors {
            display_names.insert(placeholder.clone(), err.written().to_string());
        }
        // A local binder that was respelled to stay clear of one of the
        // compiler's entities (`Rename::local`) is shown as its author
        // wrote it.
        for name in WiredIn::ALL.iter().map(|w| w.name())
            .chain(self.prelude.claimed.iter().map(|n| n.as_str()))
        {
            display_names.insert(crate::ast::local_binder(name), name.to_string());
        }
        resolved.resolution = Resolution {
            scope_errors: std::mem::take(&mut self.scope_errors),
            source_names: self.source_names.clone(),
            display_names,
        };
        Ok(resolved)
    }

    /// Program-wide facts the per-module resolution needs before it starts:
    /// the prefix the entry module's clashing names are qualified with, and
    /// which of the entry module's names keep their bare spelling. The
    /// whole import closure is loaded (parsed) here; a module that fails to
    /// load is skipped and reported by resolution.
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
        self.root_cons.clear();
        self.root_classes.clear();
        for d in &root.decls {
            for e in declared_entities(d) {
                if self.canon(e.ns, ROOT_KEY, &e.name) != e.name {
                    continue;
                }
                match e.ns {
                    Ns::Val => { self.root_vals.insert(e.name); }
                    Ns::Ty => {
                        if matches!(d, Decl::ClassDecl { .. }) {
                            self.root_classes.insert(e.name.clone());
                        }
                        self.root_tys.insert(e.name);
                    }
                    Ns::Con => { self.root_cons.insert(e.name); }
                }
            }
            // The constructor of a `newtype N = <type>` is settled in the
            // module's scope (`resolve_module`, phase 4); either reading's
            // name is the entry module's.
            if let Decl::NewtypeDef { name, con_name: None, inner, .. } = d {
                for c in [Some(name.as_str()), unsettled_newtype_head(inner)].into_iter().flatten() {
                    if self.canon(Ns::Con, ROOT_KEY, c) == c {
                        self.root_cons.insert(c.to_string());
                    }
                }
            }
        }
    }

    /// The canonical name of the entity `name` of namespace `ns` declared
    /// by module `key`.
    ///
    /// An imported module's declaration is ALWAYS `Module.name`: a spelling
    /// no source identifier can have, so it cannot collide with the
    /// Prelude, the entry module, another import, or a local binder. The
    /// entry module's declarations keep their bare names — a program
    /// without imports compiles to the same Lua names as ever — except
    /// where the bare name is already taken in its namespace by the
    /// Prelude (a source declaration or a builtin), or, for a value, by
    /// the runtime library or a builtin instance (`PreludeScope::claimed`).
    /// `main` is the entry point and always bare.
    fn canon(&self, ns: Ns, key: &str, name: &str) -> String {
        if key != ROOT_KEY {
            return format!("{key}.{name}");
        }
        let taken = match ns {
            Ns::Val => name != "main"
                && (self.prelude.values.contains(name) || self.prelude.claimed.contains(name)),
            Ns::Ty => self.prelude.types.contains(name),
            Ns::Con => self.prelude.constructors.contains(name),
        };
        if taken {
            format!("{}.{name}", self.root_prefix)
        } else {
            name.to_string()
        }
    }

    /// Resolve one module: its imports (recursively, through the cache),
    /// then its own declarations.
    ///
    /// THE MODEL. Every name a top-level declaration introduces is an
    /// ENTITY with one canonical name fixed by its defining module alone
    /// (`canon`), whoever imports it and however. There are three
    /// namespaces: values (functions, record fields, class methods), types
    /// and classes, and data constructors; a constructor, field or method
    /// belongs to its type or class, which `T(..)` in an import or export
    /// list selects by. A module's SCOPE maps the bare names it may write
    /// to entities:
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
    /// Instances are not named and belong to no scope: every instance of
    /// the program is in force everywhere (Haskell 2010 §5.4), and two
    /// classes of the same spelling in two modules are two classes.
    ///
    /// Runtime-visible spellings — what `show` prints for a constructor
    /// or field, a JSON tag or key, a LuaDict key, Generic metadata — are
    /// the source spellings (`Resolution::source_names`), never the
    /// canonical names.
    ///
    /// A reference the COMPILER wrote — the `concatMap` of a list
    /// comprehension, the `error` of a partial match — is not looked up
    /// in the module's scope at all: it denotes a fixed Prelude entity,
    /// whatever the module declares, imports, hides or binds locally
    /// (`ast::WiredIn`; `Rename::v` and `Rename::local` are the two places
    /// that make it so).
    fn resolve_module(&mut self, module: &Module, own_key: &str) -> Result<Resolved, ResolveError> {
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
                    chain.join(" -> ")).into());
            } else {
                self.in_progress.insert(key.clone());
                self.in_progress_stack.push(key.clone());
                let imported = self.load_module(module_path)?.clone();
                // An imported module's ungroupable infix chains are syntax
                // errors of that module, reported like its parse errors.
                let r = self.resolve_module(&imported, key).map_err(|e| match e {
                    ResolveError::Syntax(diags) =>
                        ResolveError::Import(syntax_errors_in_module(key, &diags)),
                    other => other,
                });
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
                         '{x}') in this module.").into());
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

        // Phase 2: this module's own entities, in every namespace: values
        // (functions, record fields, class methods), types and classes,
        // constructors. Each gets its canonical name; a constructor, field
        // or method also remembers the type or class it belongs to, which
        // is what `T(..)` in an import or export list selects by.
        let mut own: PerNs<HashMap<String, String>> = PerNs::default();
        let mut own_parent: HashMap<(Ns, String), String> = HashMap::new();
        for d in &module.decls {
            for e in declared_entities(d) {
                if own.get(e.ns).contains_key(&e.name) {
                    continue;
                }
                let canon = self.canon(e.ns, own_key, &e.name);
                if let Some(p) = e.parent {
                    own_parent.insert((e.ns, e.name.clone()), p);
                }
                own.get_mut(e.ns).insert(e.name, canon);
            }
        }
        for d in &module.decls {
            if let Decl::TypeSig { name, ty } | Decl::ExportSig { name, ty } = d {
                self.shapes.entry(own.val[name].clone()).or_insert_with(|| type_shape(ty));
            }
        }
        // A fixity declaration belongs to the operator THIS module declares
        // under that name, and is recorded under that entity's canonical
        // name — never under the spelling: another module's operator of
        // the same spelling, a local binder, or the Prelude's are different
        // entities with fixities of their own. A declaration for a name
        // the module does not declare has no entity to attach to and is
        // rejected, as in GHC.
        let mut syntax_errors: Vec<Diagnostic> = Vec::new();
        for d in &module.decls {
            let Decl::FixityDecl { assoc, prec, op, span } = d else { continue };
            let fix = (*assoc, *prec);
            let mut attached = false;
            if let Some(canon) = own.val.get(op).or_else(|| own.con.get(op)) {
                self.fixities.vals.insert(canon.clone(), fix);
                attached = true;
            }
            if let Some(canon) = own.ty.get(op) {
                self.fixities.tys.insert(canon.clone(), fix);
                attached = true;
            }
            if !attached {
                syntax_errors.push(fixity_without_definition(op, *span));
            }
        }

        // Phase 3: the import scope. Each unqualified edge contributes the
        // child's surface, minus what the edge's forms leave out. A name
        // this module hides on a DIRECT import of its defining module does
        // not come back through another import that merely passes it on.
        // Everything an edge could have named but does not make visible
        // is remembered as `hidden`, for the diagnostic on a bare use.
        let direct_hidden = |origin: &str, ns: Ns, n: &str, parent: Option<&str>| -> bool {
            edges.iter().any(|d| d.key == origin
                && !d.unqual.is_empty()
                && hidden_by_forms(&d.unqual, ns, n, parent))
        };
        let visible_through = |e: &Edge, ent: &Entity, ns: Ns, n: &str| -> bool {
            let parent = ent.parent.as_deref();
            !e.unqual.is_empty()
                && !hidden_by_forms(&e.unqual, ns, n, parent)
                && !direct_hidden(&ent.origin, ns, n, parent)
        };
        let mut imp: PerNs<BTreeMap<String, Vec<Candidate>>> = PerNs::default();
        // (namespace, name) -> the module whose import form leaves it out
        // (None: the imported module itself keeps it back).
        let mut hidden: HashMap<(Ns, String), Option<String>> = HashMap::new();
        for (via, e) in edges.iter().enumerate() {
            for (ns, n) in &e.surface.withheld {
                hidden.entry((*ns, n.clone())).or_insert(None);
            }
            // An import (or hiding) list may only name entities the child
            // actually exports (Haskell 2010 §5.3.1; GHC: "Module 'A' does
            // not export 'f'"): a value item a value it offers (a function,
            // a record field, a class method), a capitalized item a type
            // or class. Only a list-carrying child is checked: a list-less
            // module offers its whole import closure, mata-ll's
            // long-standing behavior.
            if e.listed {
                for form in &e.unqual {
                    let (items, hiding) = match form {
                        ImportItems::Specific(items) => (items, false),
                        ImportItems::Hiding(items) => (items, true),
                        _ => continue,
                    };
                    let mut names: Vec<(Ns, String)> = items.iter().map(|item| match item {
                        ImportItem::Value(n) => (Ns::Val, n.clone()),
                        ImportItem::TypeAll(n) | ImportItem::TypeOnly(n) => (Ns::Ty, n.clone()),
                    }).collect();
                    names.sort();
                    names.dedup();
                    for (ns, n) in names {
                        if e.surface.names.get(ns).contains_key(&n) { continue; }
                        let key = &e.key;
                        // A constructor is imported with its type, not by
                        // itself (Haskell 2010 §5.3.1).
                        let con_parent = (ns == Ns::Ty && !hiding)
                            .then(|| e.surface.names.con.get(&n))
                            .flatten()
                            .and_then(|ents| ents.first())
                            .and_then(|ent| ent.parent.clone());
                        if let Some(parent) = con_parent {
                            return Err(format!(
                                "module '{key}' does not export '{n}' by itself: \
                                 '{n}' is a data constructor of '{parent}'\n\
                                 An import list names a constructor through its \
                                 type: write '{parent}(..)' to import the type \
                                 with its constructors (GHC rejects the import \
                                 the same way).").into());
                        }
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
                             (GHC rejects this import the same way).").into());
                    }
                }
            }
            if e.unqual.is_empty() {
                continue;
            }
            for ns in NAMESPACES {
                for (n, ents) in e.surface.names.get(ns) {
                    for ent in ents {
                        if !visible_through(e, ent, ns, n) {
                            hidden.entry((ns, n.clone())).or_insert_with(|| Some(e.key.clone()));
                            continue;
                        }
                        let cands = imp.get_mut(ns).entry(n.clone()).or_default();
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
        let prelude_visible = |ns: Ns, n: &str| -> bool {
            !prelude_restricted
                || is_builtin_syntax(ns, n)
                || (!prelude_unqual.is_empty()
                    && !hidden_by_forms(&prelude_unqual, ns, n, self.prelude.parent_of(ns, n)))
        };

        // The constructor of a `newtype N = <type>` the parser could not
        // settle: the head of the right-hand side is the constructor when
        // it names no type in this module's scope and takes one argument
        // (`newtype Rad = MkRad Double`); otherwise the declaration is
        // mata-ll's shorthand and the constructor is named like the type
        // (`newtype Age = Int`). The Haskell reading is settled into the
        // declaration here (`Rename::decl`); the shorthand, and the
        // readings the type checker rejects, are left to it.
        let type_in_scope = |n: &str| -> bool {
            own.ty.contains_key(n) || imp.ty.contains_key(n) || self.prelude.types.contains(n)
                || n.split_once('.').is_some_and(|(a, _)| import_forms.values().flatten()
                    .any(|f| matches!(f, ImportItems::Qualified(x) if x == a))
                    || prelude_forms.iter().any(|f| matches!(f, ImportItems::Qualified(x) if x == a)))
        };
        let mut newtype_cons: HashMap<String, String> = HashMap::new();
        for d in &module.decls {
            let Decl::NewtypeDef { name, con_name: None, inner, .. } = d else { continue };
            let (head, args) = newtype_rhs_spine(inner);
            let con = match head {
                Some(h) if !type_in_scope(h) && args == 1 => {
                    newtype_cons.insert(name.clone(), h.to_string());
                    h
                }
                _ => name.as_str(),
            };
            if !own.con.contains_key(con) {
                own_parent.insert((Ns::Con, con.to_string()), name.clone());
                own.con.insert(con.to_string(), self.canon(Ns::Con, own_key, con));
            }
        }
        for ns in NAMESPACES {
            for (n, canon) in own.get(ns) {
                if canon != n {
                    self.source_names.insert(canon.clone(), n.clone());
                }
                if ns != Ns::Val {
                    *self.spellings.entry((ns, n.clone())).or_insert(0) += 1;
                    self.canon_ns.insert(canon.clone(), ns);
                }
            }
        }
        // The fields of each constructor and the methods of each class this
        // module declares, by canonical name, for the record constructions
        // and instance declarations of every module that can name them.
        for d in &module.decls {
            match d {
                Decl::DataDef { constructors, .. } => {
                    for c in constructors {
                        if let ConstructorFields::Named(fs) = &c.fields {
                            let fields: Vec<(String, String)> = fs.iter()
                                .map(|f| (f.name.clone(), own.val[&f.name].clone()))
                                .collect();
                            self.fields.extend(fields.iter().map(|(_, canon)| canon.clone()));
                            self.con_fields.insert(own.con[&c.name].clone(), fields);
                        }
                    }
                }
                Decl::NewtypeDef { con_name: Some(c), field: Some(f), .. } => {
                    self.fields.insert(own.val[f].clone());
                    self.con_fields.insert(own.con[c].clone(), vec![(f.clone(), own.val[f].clone())]);
                }
                Decl::ClassDecl { name, methods, .. } => {
                    let ms = methods.iter().map(|m| (m.name.clone(), own.val[&m.name].clone())).collect();
                    self.class_methods.insert(own.ty[name].clone(), ms);
                }
                _ => {}
            }
        }

        // An unqualified import that brings in a name the Prelude also
        // provides, AT A DIFFERENT TYPE, is rejected here with the fix
        // spelled out: every bare use of the name would otherwise mean the
        // import's declaration, which is rarely what the importer intends
        // (`import Data.Map` and then `null xs` on a list). GHC reports the
        // same clash as an ambiguity where the name is used. At the SAME
        // type shape the import's declaration is taken as this module's
        // replacement for the Prelude's (an FFI re-declaration like
        // `sqrt`, a library's own copy of a Prelude function) and simply
        // shadows it in this module. A record field or class method has
        // no signature of its own to compare, so it shadows the Prelude's
        // name like a same-shape function.
        let mut collisions: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for (n, cands) in &imp.val {
            if own.val.contains_key(n) || !prelude_visible(Ns::Val, n) {
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
            return Err(collision_error(&edges[via].key, &names, importer.as_deref()).into());
        }

        // Phase 5: the bare-name tables the rewrite of this module's own
        // source consults, one per namespace — bare name -> the spelling
        // to put in its place. A name absent from a table is left as
        // written: it is a local binder's business, a Prelude name, or
        // unknown (which the type checker reports).
        let placeholders = Placeholders::new(self.scope_errors.len());
        let mut tables: PerNs<HashMap<String, String>> = PerNs::default();
        for ns in NAMESPACES {
            let root_names = match ns {
                Ns::Val => &self.root_vals,
                Ns::Ty => &self.root_tys,
                Ns::Con => &self.root_cons,
            };
            let mut names: BTreeSet<&str> = BTreeSet::new();
            names.extend(own.get(ns).keys().map(String::as_str));
            names.extend(imp.get(ns).keys().map(String::as_str));
            names.extend(hidden.keys().filter(|(h, _)| *h == ns).map(|(_, n)| n.as_str()));
            if prelude_restricted {
                names.extend(self.prelude.names(ns).iter().map(String::as_str));
            }
            if !is_root {
                names.extend(root_names.iter().map(String::as_str));
            }
            for n in names {
                let target = if let Some(canon) = own.get(ns).get(n) {
                    Some(canon.clone())
                } else if let Some(cands) = imp.get(ns).get(n) {
                    Some(match cands.as_slice() {
                        [one] => one.entity.canon.clone(),
                        many => placeholders.make(ambiguous_occurrence(n, many, &edges)),
                    })
                } else if self.prelude.names(ns).contains(n) {
                    if prelude_visible(ns, n) {
                        None
                    } else if ns == Ns::Ty && !self.prelude.classes.contains(n) {
                        Some(placeholders.make(ScopeError::UnknownType(n.to_string())))
                    } else {
                        Some(placeholders.make(prelude_hidden(n)))
                    }
                } else if let Some(by) = hidden.get(&(ns, n.to_string())) {
                    Some(placeholders.make(not_exported(n, by.as_deref())))
                } else if !is_root && root_names.contains(n) {
                    // The entry module's declaration of this name is not
                    // in scope in a module it imports.
                    Some(placeholders.make(match ns {
                        Ns::Val => ScopeError::Unbound(n.to_string()),
                        Ns::Ty if self.root_classes.contains(n) => ScopeError::UnknownClass(n.to_string()),
                        Ns::Ty => ScopeError::UnknownType(n.to_string()),
                        Ns::Con => ScopeError::UnboundConstructor(n.to_string()),
                    }))
                } else {
                    None
                };
                if let Some(t) = target
                    && t != n
                {
                    tables.get_mut(ns).insert(n.to_string(), t);
                }
            }
        }
        // A promoted constructor written without its tick where no type of
        // that name is in scope (`Vec Z a` for `Vec 'Z a`) names the
        // constructor; its type-level reference takes the constructor's
        // binding.
        let con_names: Vec<String> = own.con.keys().chain(imp.con.keys()).cloned().collect();
        for n in con_names {
            if type_in_scope(&n) || tables.ty.contains_key(&n) {
                continue;
            }
            if let Some(t) = tables.con.get(&n).cloned() {
                tables.ty.insert(n, t);
            }
        }

        // Phase 6: the alias tables. `A.name` names what the modules
        // imported as `A` EXPORT — their surfaces — and nothing else: not
        // their private names, and not a declaration of some other module
        // that happens to share the spelling. Two modules under one alias
        // offering different entities for one name make `A.name`
        // ambiguous, reported where it is used.
        let mut alias_table: BTreeMap<String, PerNs<BTreeMap<String, Vec<Entity>>>> = BTreeMap::new();
        for e in &edges {
            for alias in &e.aliases {
                let entry = alias_table.entry(alias.clone()).or_default();
                for ns in NAMESPACES {
                    for (n, ents) in e.surface.names.get(ns) {
                        let slot = entry.get_mut(ns).entry(n.clone()).or_default();
                        for ent in ents {
                            if !slot.iter().any(|x| x.canon == ent.canon) {
                                slot.push(ent.clone());
                            }
                        }
                    }
                }
            }
        }
        for form in &prelude_forms {
            if let ImportItems::Qualified(alias) = form {
                let entry = alias_table.entry(alias.clone()).or_default();
                for ns in NAMESPACES {
                    for n in self.prelude.names(ns) {
                        let slot = entry.get_mut(ns).entry(n.clone()).or_default();
                        if !slot.iter().any(|x| x.canon == *n) {
                            slot.push(Entity {
                                canon: n.clone(),
                                origin: PRELUDE_KEY.to_string(),
                                parent: self.prelude.parent_of(ns, n).map(str::to_string),
                            });
                        }
                    }
                }
            }
        }

        // A field selection from a constructor `M.f` and a qualified
        // reference `M.f` parse to the SAME shape — `App(Var <field f>,
        // Con "M")` — so when an alias name is also a data constructor in
        // scope the two meanings cannot be told apart here. The constructor
        // wins: the selection is the meaning the expression already has
        // without the alias, and collapsing it produced a bogus "Unbound
        // variable: M.f" (or a silent call of the wrong module's function).
        // Qualified references through such an alias are therefore
        // unavailable — a deviation from GHC, whose module aliases live in
        // a separate namespace — so warn, pointing at the rename that
        // restores them.
        let qualified_aliases: HashSet<String> = alias_table.keys()
            .filter(|a| {
                let keep = !own.con.contains_key(*a) && !imp.con.contains_key(*a);
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
        // `Rename::t`, `Rename::c`) rather than leave a spelling that could
        // coincide with some module's canonical name.
        for (alias, names) in &alias_table {
            for ns in NAMESPACES {
                if ns == Ns::Val && !qualified_aliases.contains(alias) {
                    continue;
                }
                for (n, ents) in names.get(ns) {
                    tables.get_mut(ns).insert(format!("{alias}.{n}"), match ents.as_slice() {
                        [one] => one.canon.clone(),
                        many => placeholders.make(ambiguous_alias(alias, n, many)),
                    });
                }
            }
        }

        // Phase 7: rewrite this module's own declarations to canonical
        // spelling, binding every reference in the scope built above.
        let alias_names: HashSet<String> = alias_table.keys().cloned().collect();
        let chain_errors = std::cell::RefCell::new(Vec::new());
        let rename = Rename {
            vals: &tables.val, tys: &tables.ty, cons: &tables.con,
            con_fields: &self.con_fields, class_methods: &self.class_methods,
            fields: &self.fields, newtype_cons: &newtype_cons,
            aliases: &alias_names, placeholders: &placeholders,
            fixities: &self.fixities, errors: &chain_errors,
            claimed: &self.prelude.claimed,
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
        syntax_errors.extend(chain_errors.into_inner());
        if !syntax_errors.is_empty() {
            syntax_errors.sort_by_key(|d| d.span.map(|s| (s.line, s.col)));
            return Err(ResolveError::Syntax(syntax_errors));
        }

        // Phase 8: what this module offers its importers. Without an
        // export list: every own declaration plus everything its
        // unqualified imports make visible here (mata-ll's long-standing
        // pass-through; GHC exports the own declarations only). With one:
        // the listed own declarations (all of them under the self
        // re-export), each listed name that resolves to an import or to
        // the Prelude, the constructors, fields and methods a listed type
        // or class carries in parentheses (`T(..)`, `C(m)`; a type or
        // class listed bare carries none), and what the `module X`
        // entries' import edges make visible. Everything else the module
        // declares or sees is `withheld`: a bare use in an importer is
        // told it is not exported, instead of plain "unbound".
        let own_entity = |ns: Ns, n: &String, canon: &String| vec![Entity {
            canon: canon.clone(),
            origin: own_key.to_string(),
            parent: own_parent.get(&(ns, n.clone())).cloned(),
        }];
        let entities = |cands: &Vec<Candidate>| -> Vec<Entity> {
            cands.iter().map(|c| c.entity.clone()).collect()
        };
        let mut surface = Surface::default();
        match &module.exports {
            None => {
                for ns in NAMESPACES {
                    for (n, canon) in own.get(ns) {
                        surface.names.get_mut(ns).insert(n.clone(), own_entity(ns, n, canon));
                    }
                    for (n, cands) in imp.get(ns) {
                        surface.names.get_mut(ns).entry(n.clone()).or_insert_with(|| entities(cands));
                    }
                }
            }
            Some(ex) => {
                // Does the export entry of `parent` carry the child `n`?
                let carries = |parent: &str, n: &str| -> bool {
                    match ex.subs.get(parent) {
                        Some(None) => true,
                        Some(Some(listed)) => listed.iter().any(|l| l == n),
                        None => false,
                    }
                };
                for ns in NAMESPACES {
                    for (n, canon) in own.get(ns) {
                        let parent = own_parent.get(&(ns, n.clone()));
                        let exported = self_export
                            || (ns != Ns::Con && ex.names.contains(n))
                            || parent.is_some_and(|p| ex.names.contains(p) && carries(p, n));
                        if exported {
                            surface.names.get_mut(ns).insert(n.clone(), own_entity(ns, n, canon));
                        }
                    }
                }
                for n in &ex.names {
                    if own.val.contains_key(n) || own.ty.contains_key(n) {
                        continue;
                    }
                    if let Some(cands) = imp.val.get(n) {
                        surface.names.val.insert(n.clone(), entities(cands));
                    } else if let Some(cands) = imp.ty.get(n) {
                        surface.names.ty.insert(n.clone(), entities(cands));
                        for ns in [Ns::Val, Ns::Con] {
                            for (c, cands) in imp.get(ns) {
                                let ents: Vec<Entity> = entities(cands).into_iter()
                                    .filter(|e| e.parent.as_deref() == Some(n.as_str()) && carries(n, c))
                                    .collect();
                                if !ents.is_empty() {
                                    surface.names.get_mut(ns).insert(c.clone(), ents);
                                }
                            }
                        }
                    } else {
                        for ns in NAMESPACES {
                            let in_prelude = |c: &String| self.prelude.names(ns).contains(c)
                                && prelude_visible(ns, c);
                            let prelude_entity = |c: &String| vec![Entity {
                                canon: c.clone(),
                                origin: PRELUDE_KEY.to_string(),
                                parent: self.prelude.parent_of(ns, c).map(str::to_string),
                            }];
                            if ns != Ns::Con && in_prelude(n) {
                                surface.names.get_mut(ns).insert(n.clone(), prelude_entity(n));
                            }
                            if ns == Ns::Ty || !self.prelude.types.contains(n) {
                                continue;
                            }
                            for c in self.prelude.names(ns) {
                                if self.prelude.parent_of(ns, c) == Some(n.as_str())
                                    && carries(n, c) && in_prelude(c)
                                {
                                    surface.names.get_mut(ns).insert(c.clone(), prelude_entity(c));
                                }
                            }
                        }
                    }
                }
                for &i in &reexported_edges {
                    let e = &edges[i];
                    for ns in NAMESPACES {
                        for (n, ents) in e.surface.names.get(ns) {
                            for ent in ents {
                                if !visible_through(e, ent, ns, n) {
                                    continue;
                                }
                                let slot = surface.names.get_mut(ns).entry(n.clone()).or_default();
                                if !slot.iter().any(|x| x.canon == ent.canon) {
                                    slot.push(ent.clone());
                                }
                            }
                        }
                    }
                }
                for ns in NAMESPACES {
                    for n in own.get(ns).keys().chain(imp.get(ns).keys()) {
                        if !surface.names.get(ns).contains_key(n) {
                            surface.withheld.insert((ns, n.clone()));
                        }
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

/// What the implicit Prelude contributes to every module's scope, as far
/// as name resolution is concerned. Built by the compile pipeline from the
/// Prelude source and the type checker's builtin environment.
#[derive(Default, Clone)]
pub struct PreludeScope {
    /// Every value name: source definitions, builtins, class methods,
    /// record fields.
    pub values: HashSet<String>,
    /// Every type-level name: source declarations, builtin types, and
    /// classes (types and classes share one namespace).
    pub types: HashSet<String>,
    /// The classes among `types`.
    pub classes: HashSet<String>,
    /// The record fields among `values`.
    pub fields: HashSet<String>,
    /// Every data constructor.
    pub constructors: HashSet<String>,
    /// The type or class each constructor, record field and class method
    /// belongs to (what `import Prelude hiding (Bool(..))` selects by).
    /// Constructors and values never share a spelling, so one table holds
    /// both.
    pub parents: HashMap<String, String>,
    /// Type shape of each signed value (see `signature_shapes`), the
    /// baseline unqualified imports are checked against.
    pub shapes: HashMap<String, String>,
    /// Spellings the compiler has claimed for entities of its own that are
    /// not names of the Prelude's scope: the functions of the runtime
    /// library and the implementations of the builtin instances
    /// (`typechecker::Checker::claimed_names`: `show_Int`, `eq_Int`,
    /// `fmap_Maybe`, …; the builtins that ARE in scope, `map` or `not`,
    /// are in here too). The type checker and everything after it take a
    /// name of such a spelling for the compiler's entity, so resolution
    /// lets no program entity keep one: a top-level declaration of the
    /// entry module gets its qualified canonical name (`canon`), a
    /// local binder a reserved spelling (`Rename::local`). A program can
    /// declare and bind these names as in GHC; it cannot be mistaken for
    /// the runtime library by doing so.
    pub claimed: HashSet<String>,
}

impl PreludeScope {
    /// The Prelude's names of namespace `ns`.
    fn names(&self, ns: Ns) -> &HashSet<String> {
        match ns {
            Ns::Val => &self.values,
            Ns::Ty => &self.types,
            Ns::Con => &self.constructors,
        }
    }

    /// The type or class the Prelude's `name` of namespace `ns` belongs to.
    fn parent_of(&self, ns: Ns, name: &str) -> Option<&str> {
        match ns {
            Ns::Ty => None,
            Ns::Val | Ns::Con => self.parents.get(name).map(String::as_str),
        }
    }
}

/// The namespaces of the entity model (see `resolve_module`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Ns {
    /// Functions, record fields, class methods.
    Val,
    /// Types (data, newtype, synonym, family) and classes.
    Ty,
    /// Data constructors.
    Con,
}

const NAMESPACES: [Ns; 3] = [Ns::Val, Ns::Ty, Ns::Con];

/// One table per namespace.
#[derive(Clone, Default, Debug)]
struct PerNs<T> {
    val: T,
    ty: T,
    con: T,
}

impl<T> PerNs<T> {
    fn get(&self, ns: Ns) -> &T {
        match ns {
            Ns::Val => &self.val,
            Ns::Ty => &self.ty,
            Ns::Con => &self.con,
        }
    }

    fn get_mut(&mut self, ns: Ns) -> &mut T {
        match ns {
            Ns::Val => &mut self.val,
            Ns::Ty => &mut self.ty,
            Ns::Con => &mut self.con,
        }
    }
}

/// One entity of the resolution model, by its canonical name, the module
/// that declares it, and — for a constructor, record field or class
/// method — the source name of the type or class it belongs to.
#[derive(Clone, Debug)]
struct Entity {
    canon: String,
    origin: String,
    parent: Option<String>,
}

/// What a module offers its importers, keyed by namespace and the bare
/// name the importer writes. A name normally maps to one entity; it maps
/// to several when the module passes on a name that is ambiguous in its
/// own scope.
#[derive(Clone, Default)]
struct Surface {
    names: PerNs<BTreeMap<String, Vec<Entity>>>,
    /// Names the module declares or sees but keeps back (private to its
    /// export list, or not passed on by it).
    withheld: BTreeSet<(Ns, String)>,
}

/// A name a declaration introduces into its module's scope: its
/// namespace, its source spelling, and the type or class it belongs to.
struct Declared {
    ns: Ns,
    name: String,
    parent: Option<String>,
}

/// The names `decl` introduces, in every namespace. The constructor of a
/// `newtype N = <type>` the parser could not settle is not among them: it
/// depends on the module's scope (see `resolve_module`, phase 4).
fn declared_entities(decl: &Decl) -> Vec<Declared> {
    let it = |ns: Ns, name: &str, parent: Option<&str>| Declared {
        ns, name: name.to_string(), parent: parent.map(str::to_string),
    };
    match decl {
        Decl::FunDef { name, .. } | Decl::TypeSig { name, .. } | Decl::ExportSig { name, .. } =>
            vec![it(Ns::Val, name, None)],
        Decl::DataDef { name, constructors, .. } => {
            let mut out = vec![it(Ns::Ty, name, None)];
            for c in constructors {
                out.push(it(Ns::Con, &c.name, Some(name)));
                if let ConstructorFields::Named(fs) = &c.fields {
                    for f in fs {
                        out.push(it(Ns::Val, &f.name, Some(name)));
                    }
                }
            }
            out
        }
        Decl::NewtypeDef { name, con_name, field, .. } => {
            let mut out = vec![it(Ns::Ty, name, None)];
            if let Some(c) = con_name {
                out.push(it(Ns::Con, c, Some(name)));
            }
            if let Some(f) = field {
                out.push(it(Ns::Val, f, Some(name)));
            }
            out
        }
        Decl::TypeAlias { name, .. } | Decl::TypeFamily { name, .. } => vec![it(Ns::Ty, name, None)],
        Decl::ClassDecl { name, methods, .. } => {
            let mut out = vec![it(Ns::Ty, name, None)];
            out.extend(methods.iter().map(|m| it(Ns::Val, &m.name, Some(name))));
            out
        }
        Decl::InstanceDecl { .. } | Decl::Import { .. } | Decl::FixityDecl { .. } => Vec::new(),
    }
}

/// Is `n` a constructor written with special syntax — the list's `:` and
/// `[]`, the unit, a tuple? Such a name is no name of the Prelude's that
/// an `import Prelude` form could leave out (Haskell 2010 §5.6.1: they
/// are always in scope).
fn is_builtin_syntax(ns: Ns, n: &str) -> bool {
    ns == Ns::Con && (n == ":" || n == "[]" || n.starts_with('('))
}

/// The right-hand side of `newtype N = <type>` as an application spine:
/// its head name, when the head is a named type, and how many arguments
/// it is applied to.
fn newtype_rhs_spine(inner: &Type) -> (Option<&str>, usize) {
    let mut head = inner;
    let mut args = 0;
    while let Type::App(f, _) = head {
        args += 1;
        head = f;
    }
    match head {
        Type::Con(h) => (Some(h.as_str()), args),
        _ => (None, args),
    }
}

/// The head of an unsettled newtype right-hand side that the Haskell
/// reading would take for the constructor.
fn unsettled_newtype_head(inner: &Type) -> Option<&str> {
    match newtype_rhs_spine(inner) {
        (Some(h), 1) => Some(h),
        _ => None,
    }
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

    /// Is `spelling` a placeholder (of any module)?
    fn is_placeholder(spelling: &str) -> bool {
        spelling.starts_with("<scope error ")
    }
}

/// Why resolution failed: an import error (a message), or syntax errors in
/// the entry module that only resolution can see — an infix chain its
/// operators' fixities define no grouping for, a fixity declaration
/// without a definition. (The same errors in an IMPORTED module are
/// reported as an import error naming the module.)
#[derive(Debug)]
pub enum ResolveError {
    Import(String),
    Syntax(Vec<Diagnostic>),
}

impl From<String> for ResolveError {
    fn from(message: String) -> Self {
        ResolveError::Import(message)
    }
}

/// The import error for syntax errors in imported module `key`.
fn syntax_errors_in_module(key: &str, diags: &[Diagnostic]) -> String {
    let msgs: Vec<String> = diags.iter().map(|d| d.to_string()).collect();
    format!("in module '{}': {}", key, msgs.join("\n"))
}

/// A fixity declaration for a name its module does not declare.
fn fixity_without_definition(op: &str, span: Span) -> Diagnostic {
    let shown = fixity::op_display(op);
    let mut diag = Diagnostic::parse_at(
        format!(
            "The fixity declaration for {shown} has no accompanying definition: \
             this module does not define {shown}"),
        span,
    );
    diag.notes.push(format!(
        "a fixity declaration sets the precedence of an operator its own module \
         defines. An operator defined elsewhere — imported, or the Prelude's — \
         keeps the fixity its defining module gave it, in every module that uses \
         it. Remove the declaration, or define {shown} in this module (GHC rejects \
         this declaration the same way)."));
    diag
}

/// Group the infix chains of declarations written in the BASELINE scope:
/// the Prelude source, and the method bodies the type checker registers
/// for its builtin classes. Every name there is its own canonical name, so
/// what such declarations need of resolution is `Rename` with empty
/// tables — the grouping of their chains under the Prelude's fixities,
/// local binders shadowing as in every module and kept off the `claimed`
/// spellings as in every module.
pub fn group_baseline(
    decls: &[Decl],
    claimed: &HashSet<String>,
) -> Result<Vec<Decl>, Vec<Diagnostic>> {
    let (vals, tys, cons) = (HashMap::new(), HashMap::new(), HashMap::new());
    let (con_fields, class_methods, newtype_cons) = (HashMap::new(), HashMap::new(), HashMap::new());
    let fields = HashSet::new();
    let aliases = HashSet::new();
    let placeholders = Placeholders::new(0);
    let errors = std::cell::RefCell::new(Vec::new());
    let rename = Rename {
        vals: &vals, tys: &tys, cons: &cons,
        con_fields: &con_fields, class_methods: &class_methods, fields: &fields,
        newtype_cons: &newtype_cons,
        aliases: &aliases, placeholders: &placeholders,
        fixities: fixity::prelude(), errors: &errors,
        claimed,
    };
    let grouped: Vec<Decl> = decls.iter().map(|d| rename.decl(d)).collect();
    debug_assert!(placeholders.into_made().is_empty(), "baseline names are all in scope");
    let errors = errors.into_inner();
    if errors.is_empty() { Ok(grouped) } else { Err(errors) }
}

/// Does `module` see the Prelude's `name` unqualified? Without an explicit
/// `import Prelude` every Prelude name is visible; with one or several,
/// exactly what the unqualified forms admit (Haskell 2010 §5.6.1).
pub fn prelude_name_visible(module: &Module, prelude: &PreludeScope, name: &str) -> bool {
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
    !unqual.is_empty() && !hidden_by_forms(&unqual, Ns::Val, name, prelude.parent_of(Ns::Val, name))
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

/// Does import item `item` name the entity `n` of namespace `ns`, which
/// belongs to the type or class `parent`? A value item names a value (a
/// function, field or method); a capitalized item names a type or class,
/// and with `(..)` also everything that belongs to it. In a `hiding` list
/// a capitalized item also names a constructor of that spelling (Haskell
/// 2010 §5.3.1).
fn item_names(item: &ImportItem, ns: Ns, n: &str, parent: Option<&str>, hiding: bool) -> bool {
    match item {
        ImportItem::Value(v) => ns == Ns::Val && v == n,
        ImportItem::TypeOnly(t) => t == n && (ns == Ns::Ty || (hiding && ns == Ns::Con)),
        ImportItem::TypeAll(t) => (ns == Ns::Ty && t == n)
            || (ns != Ns::Ty && parent == Some(t.as_str()))
            || (hiding && ns == Ns::Con && t == n),
    }
}

/// An entity is selection-hidden only when EVERY unqualified form hides
/// it: a Specific list hides what it doesn't request, a Hiding list hides
/// what it excludes, and a plain `import M` hides nothing. (No forms hide
/// nothing.)
fn hidden_by_forms(forms: &[ImportItems], ns: Ns, n: &str, parent: Option<&str>) -> bool {
    !forms.is_empty() && forms.iter().all(|form| match form {
        ImportItems::All => false,
        ImportItems::Specific(items) => !items.iter().any(|i| item_names(i, ns, n, parent, false)),
        ImportItems::Hiding(items) => items.iter().any(|i| item_names(i, ns, n, parent, true)),
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
/// `Placeholders`): one map per namespace (values, types and classes,
/// constructors). The names an instance declaration defines and the field
/// labels of a record construction are not looked up in the scope: they
/// are named through their class or constructor, whatever the module has
/// in scope under the same spelling. A reference to the Prelude's `True`
/// or `False` becomes the Boolean literal they stand for.
///
/// The rewrite is also where the compiler's own references are kept apart
/// from the program's names (`ast::WiredIn`): a reference a desugaring
/// wrote is bound to its Prelude entity without a scope lookup (`v`), and
/// a local binder spelled like such an entity is respelled, with its
/// uses, so that the entity's bare canonical name never names a local
/// variable in the rewritten module (`local`).
struct Rename<'a> {
    vals: &'a HashMap<String, String>,
    tys: &'a HashMap<String, String>,
    cons: &'a HashMap<String, String>,
    /// The fields of every constructor resolved so far (see
    /// `ModuleLoader::con_fields`).
    con_fields: &'a HashMap<String, Vec<(String, String)>>,
    /// The methods of every class resolved so far (see
    /// `ModuleLoader::class_methods`).
    class_methods: &'a HashMap<String, Vec<(String, String)>>,
    /// Every record field resolved so far (see `ModuleLoader::fields`).
    fields: &'a HashSet<String>,
    /// The module's newtypes whose constructor is the head of their
    /// right-hand side (`newtype Rad = MkRad Double`), by type name: the
    /// constructor's source name (see `resolve_module`, phase 4).
    newtype_cons: &'a HashMap<String, String>,
    /// The module's import aliases: what makes `A.name` a qualified name.
    aliases: &'a HashSet<String>,
    placeholders: &'a Placeholders,
    /// The fixity of every operator entity this module's scope can name,
    /// by canonical name. An infix chain is grouped HERE, where each of
    /// its operators has just been bound to its entity, so the fixity that
    /// governs an occurrence is by construction the fixity of what the
    /// occurrence denotes: infixl 9 for a local binder, whatever the
    /// module scope holds under the same spelling.
    fixities: &'a Fixities,
    /// The errors of chains the fixities define no grouping for.
    errors: &'a std::cell::RefCell<Vec<Diagnostic>>,
    /// The spellings no local binder keeps (`PreludeScope::claimed`).
    claimed: &'a HashSet<String>,
}

impl Rename<'_> {
    fn v(&self, name: &str, bound: &HashSet<String>) -> String {
        // A reference the compiler wrote. It denotes its Prelude entity
        // by definition: neither the binders in scope nor the module's
        // tables (an own or imported declaration of the same spelling, an
        // `import Prelude` form that leaves the name out) are consulted.
        // The entity's canonical name is its bare name, and `local` keeps
        // every local binder off that spelling.
        if let Some(wired) = WiredIn::of_reference(name) {
            return wired.name().to_string();
        }
        if let Some(label) = crate::ast::of_field_selection(name) {
            return self.field_selection(label, bound);
        }
        if bound.contains(name) {
            return self.local(name);
        }
        let qualified = self.is_alias_qualified(name);
        match self.vals.get(name) {
            // A qualified reference that resolves to a bare name (a
            // Prelude function, field or method reached through a
            // qualified Prelude alias) under a local binder of that very
            // name: written out bare, the reference would be captured by
            // the binder and silently mean the local. (Not so for a
            // name whose binder has been respelled.)
            Some(target) if qualified && bound.contains(target)
                && self.local(target) == *target => {
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

    /// The label of a record field selection (`r.label`, `(.label)`): the
    /// field the module's scope gives that name — unqualified, else
    /// through exactly one alias (GHC selects any field in scope, and a
    /// local binder is no field). A label that names no field in scope
    /// is an ordinary reference.
    fn field_selection(&self, label: &str, bound: &HashSet<String>) -> String {
        let bare = self.vals.get(label).map_or(label, String::as_str);
        if self.fields.contains(bare) {
            return bare.to_string();
        }
        let mut via_alias: Vec<&String> = self.aliases.iter()
            .filter_map(|a| self.vals.get(&format!("{a}.{label}")))
            .filter(|canon| self.fields.contains(*canon))
            .collect();
        via_alias.sort();
        via_alias.dedup();
        match via_alias.as_slice() {
            [one] => (*one).clone(),
            _ => self.v(label, bound),
        }
    }

    /// The spelling of the local binder the source calls `name`, at its
    /// binding site and at every use. A binder that shares its spelling
    /// with an entity of the compiler's — a wired-in Prelude entity, a
    /// function of the runtime library, the implementation of a builtin
    /// instance (`PreludeScope::claimed`) — gets a reserved one instead:
    /// after resolution such a spelling must mean the compiler's entity
    /// everywhere — in the references `v` has just bound, in the ones
    /// later stages write (`desugar.rs`: `>>=`, `error`; instance
    /// resolution: `show_Int`, `fmap_Maybe`) and in the Lua name code
    /// generation gives it (`not_`, `__mll_bs[1]`) — and downstream a
    /// local variable shadows a top-level name of the same spelling.
    /// The respelling is a function of the name alone, so two binders of
    /// one name shadow each other exactly as written.
    fn local(&self, name: &str) -> String {
        if WiredIn::named(name).is_some() || self.claimed.contains(name) {
            crate::ast::local_binder(name)
        } else {
            name.to_string()
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

    /// A declared type or class name.
    fn dt(&self, name: &str) -> String {
        self.tys.get(name).cloned().unwrap_or_else(|| name.to_string())
    }

    /// A declared constructor name.
    fn dc(&self, name: &str) -> String {
        self.cons.get(name).cloned().unwrap_or_else(|| name.to_string())
    }

    /// A constructor reference.
    fn c(&self, name: &str) -> String {
        if let Some(target) = self.cons.get(name) {
            return target.clone();
        }
        // `A.C` where the alias `A` offers no `C`: unbound, as for values.
        let qualified = name.split_once('.').is_some_and(|(head, rest)| {
            !rest.is_empty() && head.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        });
        if qualified {
            return self.placeholders.make(ScopeError::UnboundConstructor(name.to_string()));
        }
        name.to_string()
    }

    /// The Boolean literal a constructor reference stands for: the
    /// Prelude's `True` and `False` (canonical names no other constructor
    /// has) are the literals.
    fn boolean(canonical: &str) -> Option<bool> {
        match canonical {
            "True" => Some(true),
            "False" => Some(false),
            _ => None,
        }
    }

    /// A name used as an operator or in operator position is a
    /// constructor when spelled like one (`Node`, `:|`).
    fn is_con_spelling(name: &str) -> bool {
        name.chars().next().is_some_and(|c| c.is_uppercase() || c == ':')
    }

    /// An operator-position reference (`a `op` b`, `(op)`), bound in the
    /// namespace its spelling selects.
    fn op_ref(&self, name: &str, bound: &HashSet<String>) -> String {
        if Self::is_con_spelling(name) { self.c(name) } else { self.v(name, bound) }
    }

    /// The canonical name of the field labelled `label` of constructor
    /// `con` (canonical), when `con` is a record constructor with such a
    /// field; the label as written otherwise (the type checker reports it).
    fn field_of(&self, con: &str, label: &str) -> String {
        self.con_fields.get(con)
            .and_then(|fs| fs.iter().find(|(src, _)| src == label))
            .map(|(_, canon)| canon.clone())
            .unwrap_or_else(|| label.to_string())
    }

    /// The canonical name of the method `name` of class `class`
    /// (canonical); the name as written for a class not declared in a
    /// module (the Prelude's and the builtin classes' methods are their
    /// own canonical names) or a name the class does not declare (the type
    /// checker reports it).
    fn method_of(&self, class: &str, name: &str) -> String {
        self.class_methods.get(class)
            .and_then(|ms| ms.iter().find(|(src, _)| src == name))
            .map(|(_, canon)| canon.clone())
            .unwrap_or_else(|| name.to_string())
    }

    fn pattern(&self, p: &Pattern) -> Pattern {
        match p {
            Pattern::Constructor { name, args } => {
                let name = self.c(name);
                if args.is_empty()
                    && let Some(b) = Self::boolean(&name)
                {
                    return Pattern::LitPat(Literal::Bool(b));
                }
                Pattern::Constructor {
                    name,
                    args: args.iter().map(|a| self.pattern(a)).collect(),
                }
            }
            Pattern::Paren(inner) => Pattern::Paren(Box::new(self.pattern(inner))),
            Pattern::Tuple(elems) => Pattern::Tuple(elems.iter().map(|e| self.pattern(e)).collect()),
            Pattern::As(n, inner) => Pattern::As(self.local(n), Box::new(self.pattern(inner))),
            Pattern::Var(n) => Pattern::Var(self.local(n)),
            Pattern::Wildcard | Pattern::LitPat(_) => p.clone(),
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
                name: self.dt(name),
                type_vars: type_vars.clone(),
                constructors: constructors.iter().map(|c| self.constructor(c)).collect(),
                deriving: deriving.iter().map(|c| self.t(c)).collect(),
            },
            Decl::NewtypeDef { name, type_vars, con_name, field, inner, deriving } => {
                // The constructor of an unsettled `newtype N = <type>`:
                // the head of the right-hand side in the Haskell reading,
                // settled here; in the shorthand reading the constructor
                // is named like the type, which the declaration says by
                // leaving it unset — unless the two canonical names part
                // (an entry-module newtype named like a Prelude type but
                // not like a Prelude constructor), where it is written out.
                let (con_name, inner) = match (con_name, self.newtype_cons.get(name)) {
                    (Some(c), _) => (Some(self.dc(c)), self.ty(inner)),
                    (None, Some(h)) => {
                        let Type::App(_, arg) = inner else {
                            unreachable!("a settled newtype head takes one argument")
                        };
                        (Some(self.dc(h)), self.ty(arg))
                    }
                    (None, None) => {
                        let con = self.dc(name);
                        (Some(con.clone()).filter(|c| *c != self.dt(name)), self.ty(inner))
                    }
                };
                Decl::NewtypeDef {
                    name: self.dt(name),
                    type_vars: type_vars.clone(),
                    con_name,
                    field: field.as_ref().map(|f| self.dv(f)),
                    inner,
                    deriving: deriving.iter().map(|c| self.t(c)).collect(),
                }
            }
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
            // An instance names its class in the module's scope and the
            // methods it defines through that class.
            Decl::InstanceDecl { class_name, target_type, context, methods } => {
                let class_name = self.t(class_name);
                Decl::InstanceDecl {
                    target_type: self.ty(target_type),
                    context: context.iter().map(|c| self.constraint(c)).collect(),
                    methods: methods.iter().map(|m| InstanceMethod {
                        name: self.method_of(&class_name, &m.name),
                        clauses: m.clauses.iter().map(|c| self.clause(c)).collect(),
                    }).collect(),
                    class_name,
                }
            }
            Decl::ClassDecl { name, type_var, superclasses, methods } => Decl::ClassDecl {
                name: self.dt(name),
                type_var: type_var.clone(),
                superclasses: superclasses.iter().map(|c| self.t(c)).collect(),
                methods: methods.iter().map(|m| ClassMethod {
                    name: self.dv(&m.name),
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
        Constraint { class_name: self.t(&c.class_name), type_arg: self.ty(&c.type_arg) }
    }

    fn constructor(&self, c: &Constructor) -> Constructor {
        let fields = match &c.fields {
            ConstructorFields::Positional(ts) =>
                ConstructorFields::Positional(ts.iter().map(|t| self.ty(t)).collect()),
            ConstructorFields::Named(fs) =>
                ConstructorFields::Named(fs.iter().map(|f| crate::ast::RecordField {
                    name: self.dv(&f.name),
                    external_key: f.external_key.clone(),
                    ty: self.ty(&f.ty),
                }).collect()),
        };
        Constructor {
            name: self.dc(&c.name),
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
            name: self.local(&ld.name),
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
            Expr::Con(n) => {
                let name = self.c(n);
                match Self::boolean(&name) {
                    Some(b) => Expr::Lit(Literal::Bool(b)),
                    None => Expr::Con(name),
                }
            }
            Expr::RecordCon { constructor, fields } => {
                let constructor = self.c(constructor);
                Expr::RecordCon {
                    fields: fields.iter()
                        .map(|(f, e)| (self.field_of(&constructor, f), self.expr(e, bound)))
                        .collect(),
                    constructor,
                }
            }
            // A record update's labels are fields in the module's scope;
            // a local binder of the same spelling is no field.
            Expr::RecordUpdate { expr, updates } => Expr::RecordUpdate {
                expr: Box::new(self.expr(expr, bound)),
                updates: updates.iter()
                    .map(|(f, e)| (self.v(f, &HashSet::new()), self.expr(e, bound)))
                    .collect(),
            },
            // Backtick sections `(`f`)` carry a plain function name here.
            Expr::OpFunc(n) => Expr::OpFunc(self.op_ref(n, bound)),
            // An InfixApp's op is a NAME too — a sibling operator (`a <+> b`)
            // or a sibling function used backtick-infix (`a `combine` b`).
            // Their DEFINITIONS are renamed like every value, so in-module
            // infix uses must follow; the uniform descent below only visits
            // subEXPRESSIONS, so without this arm the op stayed bare and
            // resolved to nothing ("undefined '<+>'").
            Expr::InfixApp { op, lhs, rhs } => Expr::InfixApp {
                op: self.op_ref(op, bound),
                lhs: Box::new(self.expr(lhs, bound)),
                rhs: Box::new(self.expr(rhs, bound)),
            },
            // Infix chains and sections are grouped here, under the
            // fixities of the entities their operators denote.
            Expr::OpChain(chain) => self.chain(chain, bound).0,
            Expr::Section { op, side, operand } => self.section(op, *side, operand, bound),
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
                Expr::Lambda {
                    params: params.iter().map(|p| self.local(p)).collect(),
                    body: Box::new(self.expr(body, &b)),
                }
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

    /// Bind an operator occurrence to its entity and that entity's fixity.
    /// A local binder has no fixity declaration (infixl 9). `Err` carries
    /// the placeholder of an operator that denotes nothing in scope.
    fn operator(&self, op: &ChainOp, bound: &HashSet<String>) -> Result<fixity::Op, String> {
        let name = self.op_ref(&op.name, bound);
        if Placeholders::is_placeholder(&name) {
            return Err(name);
        }
        let (assoc, prec) = if bound.contains(&op.name) {
            fixity::DEFAULT_FIXITY
        } else {
            self.fixities.vals.get(&name).copied().unwrap_or(fixity::DEFAULT_FIXITY)
        };
        Ok(fixity::Op { written: op.name.clone(), name, span: op.span, assoc, prec })
    }

    /// Group an infix chain. Returns the grouped expression and what
    /// stands at its root (for a section's operand check).
    ///
    /// A chain with an operator that denotes nothing in scope (hidden,
    /// ambiguous) has no fixity to group by: it is replaced by that
    /// operator's placeholder reference, so the type checker reports the
    /// scope error and no grouping is ever picked for it.
    fn chain(&self, chain: &OpChain, bound: &HashSet<String>) -> (Expr, Option<fixity::Top>) {
        let mut ops = Vec::with_capacity(chain.rest.len());
        for (op, _) in &chain.rest {
            match self.operator(op, bound) {
                Ok(op) => ops.push(op),
                Err(placeholder) => return (Expr::Var(placeholder), None),
            }
        }
        let operand = |o: &ChainOperand| (o.neg, self.expr(&o.expr, bound));
        let first = operand(&chain.first);
        let rest = ops.into_iter()
            .zip(chain.rest.iter().map(|(_, o)| operand(o)))
            .collect();
        match fixity::group(first, rest) {
            Ok((e, top)) => (e, Some(top)),
            Err(diag) => (self.chain_error(*diag), None),
        }
    }

    /// Check a section's operand against the section operator's fixity and
    /// rewrite the section to the lambda it stands for.
    fn section(&self, op: &ChainOp, side: SectionSide, operand: &Expr, bound: &HashSet<String>) -> Expr {
        let op = match self.operator(op, bound) {
            Ok(op) => op,
            Err(placeholder) => return Expr::Var(placeholder),
        };
        let (operand, top) = match operand {
            Expr::OpChain(chain) => {
                if side == SectionSide::Right
                    && let Err(diag) = fixity::check_right_section_minus(&op, chain.first.neg)
                {
                    return self.chain_error(*diag);
                }
                self.chain(chain, bound)
            }
            other => (self.expr(other, bound), None),
        };
        if let Err(diag) = fixity::check_section_operand(&op, side, top.as_ref()) {
            return self.chain_error(*diag);
        }
        let x = Box::new(Expr::Var("__sec".into()));
        let operand = Box::new(operand);
        let (lhs, rhs) = match side {
            SectionSide::Left => (operand, x),
            SectionSide::Right => (x, operand),
        };
        Expr::Lambda {
            params: vec!["__sec".into()],
            body: Box::new(Expr::InfixApp { op: op.name, lhs, rhs }),
        }
    }

    /// Record a chain's error. The expression returned in the chain's
    /// place is never compiled: resolution fails once the module is done.
    fn chain_error(&self, diag: Diagnostic) -> Expr {
        self.errors.borrow_mut().push(diag);
        Expr::Lit(Literal::Unit)
    }

    fn dostmt(&self, s: &DoStmt, bound: &mut HashSet<String>) -> DoStmt {
        match s {
            DoStmt::Bind { name, expr } => {
                let e = self.expr(expr, bound);
                bound.insert(name.clone());
                DoStmt::Bind { name: self.local(name), expr: e }
            }
            DoStmt::Expr(e) => DoStmt::Expr(self.expr(e, bound)),
            DoStmt::DoLet { binds } => {
                // Recursive group: every name is in scope for every body and for
                // the rest of the do-block, so bind them all before rewriting.
                for ld in binds { bound.insert(ld.name.clone()); }
                let binds = binds.iter().map(|ld| self.localdef(ld, bound)).collect();
                DoStmt::DoLet { binds }
            }
            DoStmt::PatternBind { pattern, expr, span, end } => {
                let e = self.expr(expr, bound);
                collect_pattern_vars(pattern, bound);
                DoStmt::PatternBind { pattern: self.pattern(pattern), expr: e, span: *span, end: *end }
            }
        }
    }

    fn ty(&self, t: &Type) -> Type {
        match t {
            Type::Con(n) => {
                Type::Con(self.t(n))
            }
            Type::Var(_) | Type::Unit => t.clone(),
            Type::Promoted(n) => Type::Promoted(self.c(n)),
            // An infix type is grouped under the fixities of the types its
            // operators name in this module's scope. An operator that names
            // no type in scope leaves nothing to group by: the chain becomes
            // that operator's placeholder, which the type checker reports.
            Type::OpChain { first, rest } => {
                let mut ops = Vec::with_capacity(rest.len());
                for (op, operand) in rest {
                    let name = self.t(&op.name);
                    if Placeholders::is_placeholder(&name) {
                        return Type::Con(name);
                    }
                    let (assoc, prec) = self.fixities.tys.get(&name).copied()
                        .unwrap_or(fixity::DEFAULT_FIXITY);
                    let op = fixity::Op { written: op.name.clone(), name, span: op.span, assoc, prec };
                    ops.push((op, self.ty(operand)));
                }
                fixity::group_types(self.ty(first), ops)
            }
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
/// `M.foo` parsed as the field-selection shape `App(Var <field foo>, Con
/// "M")` (`ast::field_selection`); where `M` is a known qualified alias,
/// collapse it to `Var "M.foo"`.
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
    // `M.f` parses as the selection of field `f` from `M` (`App(Var
    // <field f>, Con M)`); with a known alias `M` it is the qualified
    // reference `M.f`. (Only a selection forms one: an application `f M`
    // and a compiler-written head — the `enumFrom` of `[A ..]` — do not.)
    if let Expr::App(f, x) = &e
        && let Expr::Var(field) = f.as_ref()
        && let Some(field) = crate::ast::of_field_selection(field)
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
        // Shapes are taken of signatures as written, before their infix
        // types are grouped; the written order is fingerprint enough (no
        // Prelude signature has a type operator to compare with).
        Type::OpChain { first, rest } => {
            let mut out = format!("<{}", type_shape(first));
            for (op, operand) in rest {
                out.push_str(&format!(" {} {}", op.name, type_shape(operand)));
            }
            out + ">"
        }
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
        // The same references before their chain is grouped.
        Expr::OpChain(chain) => out.extend(chain.rest.iter().map(|(op, _)| op.name.clone())),
        Expr::Section { op, .. } => { out.insert(op.name.clone()); }
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
