pub mod ast;
pub mod codegen;
pub mod dce;
pub mod demand;
pub mod desugar;
pub mod embed;
pub mod fold;
pub mod lexer;
pub mod modules;
pub mod mono;
pub mod parser;
pub mod split;
mod stdlib;
pub mod tir;
pub mod typechecker;
pub mod types;
pub mod verify;

use std::path::Path;

pub use embed::EmbedMode;

/// Result of compilation
pub struct CompileResult {
    pub lua_code: String,
    pub has_main: bool,
    pub exports: Vec<String>,
    /// Non-fatal diagnostics about the compiled result. Frontends should
    /// surface these to the user (the `mll` CLI prints them to stderr).
    /// Currently emitted: the module compiled to Lua with no runnable or
    /// callable code because it has neither `main` nor any `export`
    /// declaration (see `no_host_surface_warning`).
    pub warnings: Vec<types::Diagnostic>,
}

/// Options controlling the emitted Lua.
#[derive(Debug, Default, Clone)]
pub struct CompileOptions {
    /// Embed the original source text into the emitted Lua so the .lua file
    /// can later be recompiled without the .mll (see `embed::extract_source`).
    /// `None` embeds nothing.
    pub embed_source: Option<EmbedMode>,
    /// Display name of the file being compiled (e.g. `foo.mll`). When set,
    /// diagnostics that point into the user's own file render the location as
    /// `at foo.mll:1:1` — used where the distinction from Prelude-internal
    /// line numbers matters. `None` keeps the bare `at line:col` rendering.
    pub source_name: Option<String>,
}

/// Compile error. Parse and type errors carry structured [`types::Diagnostic`]s
/// (message, source span, enclosing definition, notes); the Display impl
/// renders them exactly as before.
#[derive(Debug)]
pub enum CompileError {
    Lex(String),
    Parse(Vec<types::Diagnostic>),
    Import(String),
    Type(Vec<types::Diagnostic>),
    /// A post-monomorphization invariant was violated — a compiler bug, not the
    /// user's. Failing here beats emitting known-wrong Lua.
    Internal(Vec<String>),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::Lex(e) => write!(f, "Lexer error: {}", e),
            CompileError::Parse(errors) => {
                for (i, e) in errors.iter().enumerate() {
                    if i > 0 { writeln!(f)?; }
                    write!(f, "Parse error: {}", e)?;
                }
                Ok(())
            }
            CompileError::Import(e) => write!(f, "Import error: {}", e),
            CompileError::Type(errors) => {
                for e in errors {
                    writeln!(f, "Type error: {}", e)?;
                }
                Ok(())
            }
            CompileError::Internal(errors) => {
                for e in errors {
                    writeln!(f, "Internal compiler error: {}", e)?;
                }
                Ok(())
            }
        }
    }
}

/// Parse and return the prelude declarations.
fn parse_prelude() -> Result<Vec<ast::Decl>, CompileError> {
    let tokens = lexer::lex(stdlib::PRELUDE).map_err(CompileError::Lex)?;
    let module = parser::parse(&tokens).map_err(CompileError::Parse)?;
    Ok(module.decls)
}

/// Compile mll source code to Lua with default options.
///
/// `source`: the .mll source code
/// `source_dir`: directory of the source file (for import resolution)
/// `lib_paths`: additional search paths for library modules
pub fn compile(source: &str, source_dir: &Path, lib_paths: &[&Path]) -> Result<CompileResult, CompileError> {
    compile_with_options(source, source_dir, lib_paths, &CompileOptions::default())
}

/// Compile mll source code to Lua. See [`compile`]; `options` additionally
/// controls output features such as source embedding.
pub fn compile_with_options(
    source: &str,
    source_dir: &Path,
    lib_paths: &[&Path],
    options: &CompileOptions,
) -> Result<CompileResult, CompileError> {
    // Lex
    let tokens = lexer::lex(source).map_err(CompileError::Lex)?;

    // Parse
    let parsed = parser::parse(&tokens).map_err(CompileError::Parse)?;

    // Parse the prelude up-front: its signature shapes are the baseline against
    // which unqualified imports are checked for incompatible-type collisions.
    let prelude_decls = parse_prelude()?;
    let prelude_shapes = modules::signature_shapes(&prelude_decls);

    // Resolve imports
    let mut loader = modules::ModuleLoader::new(source_dir);
    for path in lib_paths {
        loader.add_search_path(path.to_path_buf());
    }
    let module = loader.resolve_imports(&parsed).map_err(CompileError::Import)?;
    // Reject unqualified imports that would clash in the flattened namespace,
    // with a clear message, rather than letting the clash surface downstream.
    loader.check_import_collisions(&parsed, &prelude_shapes)
        .map_err(CompileError::Import)?;

    // The module-header export list (`module M (foo, Bar(..)) where`), kept
    // from the parsed source before the merge below discards it. It controls
    // which of this module's names other .mll modules may import (enforced in
    // modules.rs / the typechecker) and nothing else — in particular it does
    // NOT export anything to the Lua host; that is exclusively the `export`
    // keyword's job. It is retained here only to diagnose the classic mixup:
    // a library written with a header export list and compiled standalone,
    // which yields no host-callable code at all (see the warning below).
    let header_exports = parsed.exports.clone();

    // Count own (non-import) declarations from the parsed source before
    // import resolution merges everything together.
    let own_count = parsed.decls.iter()
        .filter(|d| !matches!(d, ast::Decl::Import { .. }))
        .count();
    let hidden = module.hidden.clone();
    let prelude_count = prelude_decls.len();
    let mut module = ast::Module {
        decls: prelude_decls.into_iter()
            .chain(module.decls)
            .collect(),
        exports: None,
        hidden,
    };
    let local_start = module.decls.len() - own_count;

    // Desugar do-notation to >>= chains
    desugar::desugar_module(&mut module);

    let mut checker = typechecker::Checker::new();

    // The user's own top-level value definitions that reuse a name the
    // baseline already provides (a Prelude definition or a compiler builtin).
    // mata-ll compiles the Prelude and the program into one flat namespace, so
    // such a redefinition does not shadow the Prelude name (as it would in
    // GHC) — it collides with it. Two collision classes are rejected up front,
    // before the Prelude is ever type-checked against the user's signature:
    //
    //   1. the Prelude's own implementation uses the name (`error`, `show`,
    //      `foldl`, …): the redefinition replaces the name out from under the
    //      Prelude's code, which then either fails to type-check — a cascade
    //      of errors at Prelude source lines the user never wrote — or
    //      silently compiles against the user's replacement;
    //   2. the name has a Prelude source definition and the user's definition
    //      has the same type shape (or no signature): a duplicate definition
    //      of the same function at the same type, which downstream passes
    //      cannot disambiguate.
    //
    // Anything else is left alone: redefining an unreferenced builtin (`head`)
    // or a Prelude function at a genuinely different type (a monomorphic
    // `replicate` for FFI export) works today with GHC-like user-wins
    // semantics and stays supported. If such a redefinition nevertheless
    // breaks the Prelude's own type-checking, the safety net below converts
    // the Prelude-internal errors into the same clear message.
    let redefined = collect_baseline_redefinitions(
        &module.decls[..prelude_count],
        &module.decls[local_start..],
        &checker,
    );
    let early: Vec<types::Diagnostic> = redefined.iter()
        .filter(|r| r.load_bearing || r.duplicate)
        .map(|r| redefinition_diagnostic(r, options.source_name.as_deref()))
        .collect();
    if !early.is_empty() {
        return Err(CompileError::Type(early));
    }

    // Type check
    checker.set_prelude_decl_count(prelude_count);
    let tir_module = checker.check_module_with_local_start(&module, local_start);

    if !checker.errors.is_empty() {
        let errors = std::mem::take(&mut checker.errors);
        // Safety net: an error inside the Prelude's own declarations can only
        // mean the user's program interfered with the Prelude — by itself it
        // always compiles. When the user redefined baseline names, report
        // those redefinitions at the user's definition sites and drop the
        // misleading Prelude-internal errors they caused.
        if errors.iter().any(|e| e.baseline) && !redefined.is_empty() {
            let mut diags: Vec<types::Diagnostic> = redefined.iter()
                .map(|r| redefinition_diagnostic(r, options.source_name.as_deref()))
                .collect();
            diags.extend(errors.into_iter().filter(|e| !e.baseline));
            return Err(CompileError::Type(diags));
        }
        return Err(CompileError::Type(errors));
    }

    // Monomorphize
    let mut mono_pass = mono::Monomorphizer::new(&checker);
    let mono_module = mono_pass.run(tir_module);

    if !mono_pass.errors.is_empty() {
        return Err(CompileError::Type(mono_pass.errors));
    }

    // Invariant check: every type-directed `show` must have resolved to a
    // specialized implementation at concrete structured types. A violation means
    // the compiler would emit known-wrong output, so fail loudly instead.
    let violations = mono_pass.verify(&mono_module);
    if !violations.is_empty() {
        return Err(CompileError::Internal(violations));
    }

    // Constant folding
    let mono_module = fold::fold_module(mono_module);

    // Bound emitted-expression nesting depth: pull deep pure sub-expressions
    // into `let` bindings so the generated Lua stays within Lua's own parser
    // recursion limit (a long chain would otherwise produce Lua that Lua
    // refuses to load with a "C stack overflow"). See split.rs.
    let mono_module = split::split_module(mono_module);

    // Dead-code elimination: drop functions (auto-prelude, unused specializations
    // and instance methods) not reachable from main/exports.
    let mono_module = dce::eliminate(mono_module);

    // A module with neither `main` nor any `export` declaration compiles to a
    // Lua file with no entry point and an empty (or absent) return table:
    // dead-code elimination — whose only roots are `main` and the exports —
    // just removed every definition. Emitting that shell silently is never
    // what the author meant, so say so. A module-header export list does not
    // prevent this: it only scopes .mll-level imports (see `header_exports`
    // above), which is precisely the mixup the warning's notes explain.
    let mut warnings = Vec::new();
    if !mono_module.has_main && mono_module.exports.is_empty() {
        warnings.push(no_host_surface_warning(header_exports.as_deref()));
    }

    // Generate Lua
    let embed = options.embed_source.map(|mode| (mode, source));
    let lua_code = codegen::generate(&mono_module, embed);

    Ok(CompileResult {
        lua_code,
        has_main: mono_module.has_main,
        exports: mono_module.exports,
        warnings,
    })
}

/// The warning for a compilation root that offers the outside world nothing:
/// no `main` to run and no `export` to call. When the module has a header
/// export list (`module M (foo) where`), the notes additionally explain that
/// the list governs .mll import visibility only — the GHC habit it mimics
/// does not carry over to exporting for the Lua host.
fn no_host_surface_warning(header_exports: Option<&[String]>) -> types::Diagnostic {
    let mut d = types::Diagnostic::new(types::DiagnosticKind::Other(
        "this module defines no `main` and no `export`, so the compiled Lua file \
         contains no runnable or callable code"
            .to_string(),
    ));
    d.notes.push(
        "the compiler keeps only code reachable from `main` or from an `export` \
         declaration; with neither present, every definition was removed as dead code."
            .to_string(),
    );
    // A value-level (lowercase) name from the header list makes the guidance
    // concrete; type/constructor entries (`Bar(..)`) are import-visibility
    // items with no value to export, so they can't serve as the example.
    let example = header_exports.and_then(|names| {
        names.iter().find(|n| n.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c == '_'))
    });
    if let Some(exports) = header_exports {
        d.notes.push(format!(
            "the export list in the `module … ({}) where` header only controls which \
             names other mata-ll modules may import — unlike in GHC, it does not export \
             anything to the Lua host.",
            exports.join(", "),
        ));
    }
    if let Some(name) = example {
        d.notes.push(format!(
            "to make `{name}` callable from Lua (via `require`), declare it with the \
             `export` keyword: `export {name} :: <its type>`. To make this file a \
             runnable program instead, define `main :: IO ()`.",
        ));
    } else {
        d.notes.push(
            "define `main :: IO ()` to make this a runnable program, or declare \
             `export <name> :: <type>` to expose a function to the Lua host."
                .to_string(),
        );
    }
    d
}

/// One user top-level value definition that reuses a baseline-provided name
/// (a Prelude definition or a compiler builtin). See `compile_with_options`
/// for the rejection rules built on the two flags.
struct BaselineRedefinition {
    name: String,
    /// Where the user defined it: the first clause of their `FunDef`
    /// (`None` for a signature with no accompanying definition).
    span: Option<ast::Span>,
    /// The Prelude's own implementation references this name from another
    /// definition, so redefining it changes the Prelude out from under itself.
    load_bearing: bool,
    /// The name has a Prelude source definition and the user's definition has
    /// the same type shape (or no signature): a duplicate of the same
    /// function at the same type rather than an overload at a different one.
    duplicate: bool,
}

/// Scan the user's own top-level declarations for value names the baseline
/// (`prelude_decls` + the builtin environment of a fresh `checker`) already
/// provides. Each redefined name is reported once, in source order.
fn collect_baseline_redefinitions(
    prelude_decls: &[ast::Decl],
    own_decls: &[ast::Decl],
    checker: &typechecker::Checker,
) -> Vec<BaselineRedefinition> {
    use std::collections::{HashMap, HashSet};

    // What the Prelude provides, and which of it has a source FunDef.
    let mut prelude_named: HashSet<&str> = HashSet::new();
    let mut prelude_defs: HashSet<&str> = HashSet::new();
    for d in prelude_decls {
        match d {
            ast::Decl::FunDef { name, .. } => {
                prelude_named.insert(name);
                prelude_defs.insert(name);
            }
            ast::Decl::TypeSig { name, .. } | ast::Decl::ExportSig { name, .. } => {
                prelude_named.insert(name);
            }
            _ => {}
        }
    }
    let prelude_used = modules::body_references(prelude_decls);
    let prelude_shapes = modules::signature_shapes(prelude_decls);
    let own_shapes = modules::signature_shapes(own_decls);

    let mut seen: HashSet<&str> = HashSet::new();
    let mut spans: HashMap<&str, ast::Span> = HashMap::new();
    let mut order: Vec<&str> = Vec::new();
    for d in own_decls {
        match d {
            ast::Decl::FunDef { name, clauses } => {
                if seen.insert(name) {
                    order.push(name);
                }
                if let Some(c) = clauses.first() {
                    spans.entry(name).or_insert(c.span);
                }
            }
            ast::Decl::TypeSig { name, .. } | ast::Decl::ExportSig { name, .. } => {
                if seen.insert(name) {
                    order.push(name);
                }
            }
            _ => {}
        }
    }

    order.into_iter()
        .filter(|name| prelude_named.contains(*name) || checker.is_builtin(name))
        .map(|name| BaselineRedefinition {
            name: name.to_string(),
            span: spans.get(name).copied(),
            load_bearing: prelude_used.contains(name),
            duplicate: prelude_defs.contains(name)
                && match (own_shapes.get(name), prelude_shapes.get(name)) {
                    (Some(user), Some(prelude)) => user == prelude,
                    // No user signature: the definition inherits the
                    // Prelude's, so it duplicates it at the same type.
                    (None, _) => true,
                    (Some(_), None) => false,
                },
        })
        .collect()
}

/// The clear, user-located error for a rejected baseline redefinition.
fn redefinition_diagnostic(
    r: &BaselineRedefinition,
    source_name: Option<&str>,
) -> types::Diagnostic {
    let mut d = types::Diagnostic::new(types::DiagnosticKind::Other(format!(
        "'{}' is already provided by the Prelude and cannot be redefined",
        r.name
    )));
    d.span = r.span;
    d.file = source_name.map(str::to_string);
    d.notes.push(
        "mata-ll includes the Prelude implicitly and compiles it with your program \
         in one global namespace, so its names (error, map, head, …) are always in \
         scope and a top-level definition does not shadow them as it would in GHC — \
         rename your function."
            .to_string(),
    );
    if r.load_bearing {
        d.notes.push(format!(
            "the Prelude's own functions use '{}', so redefining it would break \
             Prelude code your program did not write.",
            r.name
        ));
    } else if r.duplicate {
        d.notes.push(format!(
            "your definition has the same type as the Prelude's '{}', making it a \
             duplicate definition of the same function rather than a distinct one.",
            r.name
        ));
    }
    d
}
