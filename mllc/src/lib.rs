#![allow(dead_code, unused_variables, unused_mut, unused_imports)]

pub mod ast;
pub mod codegen;
pub mod demand;
pub mod desugar;
pub mod fold;
pub mod lexer;
pub mod modules;
pub mod mono;
pub mod parser;
pub mod tir;
pub mod typechecker;
pub mod types;

use std::path::Path;

/// Result of compilation
pub struct CompileResult {
    pub lua_code: String,
    pub has_main: bool,
    pub exports: Vec<String>,
}

/// Compile error
#[derive(Debug)]
pub enum CompileError {
    Lex(String),
    Parse(String),
    Import(String),
    Type(Vec<String>),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::Lex(e) => write!(f, "Lexer error: {}", e),
            CompileError::Parse(e) => write!(f, "Parse error: {}", e),
            CompileError::Import(e) => write!(f, "Import error: {}", e),
            CompileError::Type(errors) => {
                for e in errors {
                    writeln!(f, "Type error: {}", e)?;
                }
                Ok(())
            }
        }
    }
}

/// The MLL prelude source, embedded at compile time.
const PRELUDE_MLL: &str = include_str!("../../lib/Prelude.mll");

/// Parse and return the prelude declarations.
fn parse_prelude() -> Result<Vec<ast::Decl>, CompileError> {
    let tokens = lexer::lex(PRELUDE_MLL).map_err(CompileError::Lex)?;
    let module = parser::parse(&tokens).map_err(CompileError::Parse)?;
    Ok(module.decls)
}

/// Compile mll source code to Lua.
///
/// `source`: the .mll source code
/// `source_dir`: directory of the source file (for import resolution)
/// `lib_paths`: additional search paths for library modules
pub fn compile(source: &str, source_dir: &Path, lib_paths: &[&Path]) -> Result<CompileResult, CompileError> {
    // Lex
    let tokens = lexer::lex(source).map_err(CompileError::Lex)?;

    // Parse
    let parsed = parser::parse(&tokens).map_err(CompileError::Parse)?;

    // Resolve imports
    let mut loader = modules::ModuleLoader::new(source_dir);
    for path in lib_paths {
        loader.add_search_path(path.to_path_buf());
    }
    let module = loader.resolve_imports(&parsed).map_err(CompileError::Import)?;

    // Count own (non-import) declarations from the parsed source before
    // import resolution merges everything together.
    let own_count = parsed.decls.iter()
        .filter(|d| !matches!(d, ast::Decl::Import { .. }))
        .count();

    // Merge prelude declarations (prepend before user declarations)
    let prelude_decls = parse_prelude()?;
    let hidden = module.hidden.clone();
    let mut module = ast::Module {
        decls: prelude_decls.into_iter()
            .chain(module.decls.into_iter())
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
        let errors: Vec<String> = checker.errors.iter()
            .map(|e| format!("{}", e))
            .collect();
        return Err(CompileError::Type(errors));
    }

    // Monomorphize
    let mut mono_pass = mono::Monomorphizer::new(&checker);
    let mono_module = mono_pass.run(tir_module);

    if !mono_pass.errors.is_empty() {
        return Err(CompileError::Type(mono_pass.errors));
    }

    // Constant folding
    let mono_module = fold::fold_module(mono_module);

    // Generate Lua
    let lua_code = codegen::generate(&mono_module);

    Ok(CompileResult {
        lua_code,
        has_main: mono_module.has_main,
        exports: mono_module.exports,
    })
}
