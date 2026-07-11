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
}

/// Options controlling the emitted Lua.
#[derive(Debug, Default, Clone)]
pub struct CompileOptions {
    /// Embed the original source text into the emitted Lua so the .lua file
    /// can later be recompiled without the .mll (see `embed::extract_source`).
    /// `None` embeds nothing.
    pub embed_source: Option<EmbedMode>,
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

    // Count own (non-import) declarations from the parsed source before
    // import resolution merges everything together.
    let own_count = parsed.decls.iter()
        .filter(|d| !matches!(d, ast::Decl::Import { .. }))
        .count();
    let hidden = module.hidden.clone();
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

    // Type check
    let mut checker = typechecker::Checker::new();
    let tir_module = checker.check_module_with_local_start(&module, local_start);

    if !checker.errors.is_empty() {
        return Err(CompileError::Type(std::mem::take(&mut checker.errors)));
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

    // Generate Lua
    let embed = options.embed_source.map(|mode| (mode, source));
    let lua_code = codegen::generate(&mono_module, embed);

    Ok(CompileResult {
        lua_code,
        has_main: mono_module.has_main,
        exports: mono_module.exports,
    })
}
