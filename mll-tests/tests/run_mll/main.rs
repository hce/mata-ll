//! Test harness: discovers all .mll files in tests/cases/,
//! compiles each with mllc, runs the result via mlua,
//! and reports success/failure.
//!
//! A case checks itself with `assert`; a case may ALSO carry `-- expect:`
//! lines, which are compared, in order, against what the program prints
//! (`putStrLn`/`print`, i.e. Lua `print`), with `assert`'s success marker
//! (a lone `.`) filtered out. Until 2026-08-17 those lines were
//! documentation only — nothing compared them.

use std::path::Path;

/// The `-- expect:` lines of a case, in order (`-- expect: text` → `text`;
/// `-- expect:` alone → an empty line).
fn expected_lines(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|l| l.strip_prefix("-- expect:"))
        .map(|rest| rest.strip_prefix(' ').unwrap_or(rest).to_string())
        .collect()
}

fn run_mll_file(path: &Path, libs: &[&Path]) {
    // On the compiler's calibrated stack (with_compiler_stack): the
    // nesting-depth limit (mllc::MAX_NESTING_DEPTH) is calibrated against
    // mllc::COMPILER_STACK_SIZE, so a smaller test stack would overflow on
    // input the real compiler handles (or cleanly rejects).
    with_compiler_stack(|| {
        let source = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Cannot read {}: {}", path.display(), e));

        let source_dir = path.parent().unwrap_or(Path::new("."));
        // The stamp-refutation twin of mllc::compile: same output, plus
        // the emitted-Lua annotation check every corpus program should
        // exercise (see verify::check_stamps).
        let lua_code = match mllc::compile_with_stamp_refutation(&source, source_dir, libs) {
            Ok(r) => r.lua_code,
            Err(e) => panic!("{}: compilation failed:\n{}", path.display(), e),
        };

        let lua = mlua::Lua::new();
        let expected = expected_lines(&source);
        let captured = lua.create_table().unwrap();
        if !expected.is_empty() {
            // Capture `print` (putStrLn/print compile to it) line by line.
            let sink = captured.clone();
            let print_fn = lua
                .create_function(move |_, args: mlua::Variadic<mlua::Value>| -> mlua::Result<()> {
                    let parts: Vec<String> = args
                        .iter()
                        .map(|v| match v {
                            mlua::Value::String(s) => Ok(s.to_str()?.to_string()),
                            other => Ok(format!("{:?}", other)),
                        })
                        .collect::<mlua::Result<_>>()?;
                    let n = sink.raw_len();
                    sink.raw_set(n + 1, parts.join("\t"))?;
                    Ok(())
                })
                .unwrap();
            lua.globals().set("print", print_fn).unwrap();
        }
        match lua.load(&lua_code).set_name(path.to_str().unwrap()).exec() {
            Ok(()) => {}
            Err(e) => panic!("{}: runtime error:\n{}", path.display(), e),
        }
        if !expected.is_empty() {
            let printed: Vec<String> = captured
                .sequence_values::<String>()
                .collect::<mlua::Result<_>>()
                .unwrap();
            let printed: Vec<String> = printed.into_iter().filter(|l| l != ".").collect();
            assert_eq!(
                printed, expected,
                "{}: printed output (left) differs from its `-- expect:` lines (right)",
                path.display()
            );
        }
    })
}

// Every compile in this harness runs on the compiler's calibrated stack
// through this (imported so the submodules' `use super::*` picks it up).
use mllc::with_compiler_stack;

/// `mllc::compile`, on the compiler's calibrated stack. EVERY compile in this
/// harness must run on such a stack (via this, `run_mll_file`, or
/// `with_compiler_stack`): the nesting-depth limit assumes
/// `mllc::COMPILER_STACK_SIZE`, and libtest worker threads are far smaller —
/// in a debug build even the ~30-level inference spine of the 8-line
/// examples/primes_check.mll overflows them.
fn compile(
    source: &str,
    dir: &Path,
    libs: &[&Path],
) -> Result<mllc::CompileResult, mllc::CompileError> {
    with_compiler_stack(|| mllc::compile(source, dir, libs))
}

/// Compile `source` (rooted at `.`, with `libs` on the search path) expecting
/// FAILURE, and assert the rendered error contains every substring in
/// `needles`. Returns the rendered error so call sites can add checks the
/// needle form cannot express (negations, `||` alternatives, counts).
#[track_caller]
fn expect_compile_error(source: &str, libs: &[&Path], needles: &[&str]) -> String {
    expect_compile_error_in(source, Path::new("."), libs, needles)
}

/// `expect_compile_error` with an explicit source directory, for programs
/// that import helper modules from tests/cases/.
#[track_caller]
fn expect_compile_error_in(source: &str, dir: &Path, libs: &[&Path], needles: &[&str]) -> String {
    match compile(source, dir, libs) {
        Err(e) => {
            let msg = format!("{}", e);
            for needle in needles {
                assert!(
                    msg.contains(needle),
                    "expected the compile error to contain {:?}, got: {}",
                    needle,
                    msg
                );
            }
            msg
        }
        Ok(_) => panic!(
            "expected compilation to fail (with an error containing {:?}), but it succeeded",
            needles
        ),
    }
}

mod registration;
mod codegen_shape;
mod compile_errors;
mod ffi;
mod runtime;
mod linear;
mod ghc_oracle;
mod strictness_contract;
