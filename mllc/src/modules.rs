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

        let file_path = self.resolve_path(module_path)
            .ok_or_else(|| format!("Cannot find module '{}'", key))?;

        let source = fs::read_to_string(&file_path)
            .map_err(|e| format!("Error reading {}: {}", file_path.display(), e))?;

        let tokens = lexer::lex(&source)?;
        let module = parser::parse(&tokens)?;

        self.loaded.insert(key.clone(), module);
        Ok(self.loaded.get(&key).unwrap())
    }

    /// Process all imports in a module, returning merged declarations.
    /// The imported declarations are prepended to the module's own declarations.
    pub fn resolve_imports(&mut self, module: &Module) -> Result<Module, String> {
        let mut imported_decls: Vec<Decl> = Vec::new();
        let mut own_decls: Vec<Decl> = Vec::new();
        let mut seen_imports: HashSet<String> = HashSet::new();
        let mut hidden_names: HashSet<String> = module.hidden.clone();

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

                    // Compute hidden names: names in the module but not exported
                    let parsed_exports = self.loaded.get(&key).and_then(|m| m.exports.clone());
                    if let Some(ref exports) = parsed_exports {
                        for d in &all_decls {
                            if let Some(name) = decl_name(d)
                                && !exports.contains(&name) {
                                    hidden_names.insert(name);
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
                            for d in &all_decls {
                                imported_decls.push(prefix_decl(d, alias));
                            }
                        }
                    }
                }
                _ => {
                    own_decls.push(decl.clone());
                }
            }
        }

        // Merge: imported first, then own
        imported_decls.extend(own_decls);
        Ok(Module { decls: imported_decls, exports: None, hidden: hidden_names })
    }
}

/// Get the primary name of a declaration for import filtering
/// Prefix a declaration's name with a qualified alias: "T.foo"
fn prefix_decl(decl: &Decl, alias: &str) -> Decl {
    let prefix = |name: &str| format!("{}.{}", alias, name);
    match decl {
        Decl::TypeSig { name, ty } => Decl::TypeSig {
            name: prefix(name), ty: ty.clone(),
        },
        Decl::FunDef { name, clauses } => Decl::FunDef {
            name: prefix(name), clauses: clauses.clone(),
        },
        Decl::DataDef { name, type_vars, constructors, deriving } => Decl::DataDef {
            name: prefix(name), type_vars: type_vars.clone(),
            constructors: constructors.clone(), deriving: deriving.clone(),
        },
        Decl::NewtypeDef { name, type_vars, inner } => Decl::NewtypeDef {
            name: prefix(name), type_vars: type_vars.clone(), inner: inner.clone(),
        },
        // Don't prefix class/instance names — they're global
        other => other.clone(),
    }
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
