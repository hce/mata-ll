//! Test harness: discovers all .mll files in tests/cases/,
//! compiles each with mllc, runs the result via mlua,
//! and reports success/failure.

use std::path::Path;

fn run_mll_file(path: &Path, libs: &[&Path]) {
    let path = path.to_path_buf();
    let libs: Vec<std::path::PathBuf> = libs.iter().map(|p| p.to_path_buf()).collect();
    // Run on a thread with the same stack size as the mll CLI driver: the
    // compiler's nesting-depth limit (mllc::MAX_NESTING_DEPTH) is calibrated
    // against mllc::COMPILER_STACK_SIZE, so a smaller test stack would
    // overflow on input the real compiler handles (or cleanly rejects).
    let result = std::thread::Builder::new()
        .stack_size(mllc::COMPILER_STACK_SIZE)
        .spawn(move || {
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("Cannot read {}: {}", path.display(), e));

            let source_dir = path.parent().unwrap_or(Path::new("."));
            let lib_refs: Vec<&Path> = libs.iter().map(|p| p.as_path()).collect();
            // The stamp-refutation twin of mllc::compile: same output, plus
            // the emitted-Lua annotation check every corpus program should
            // exercise (see verify::check_stamps).
            let lua_code =
                match mllc::compile_with_stamp_refutation(&source, source_dir, &lib_refs) {
                    Ok(r) => r.lua_code,
                    Err(e) => panic!("{}: compilation failed:\n{}", path.display(), e),
                };

            let lua = mlua::Lua::new();
            match lua.load(&lua_code).set_name(path.to_str().unwrap()).exec() {
                Ok(()) => {}
                Err(e) => panic!("{}: runtime error:\n{}", path.display(), e),
            }
        })
        .unwrap()
        .join();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

/// Run `f` on a thread with the compiler's calibrated stack and hand its
/// value back; a panic in `f` resumes on the caller. Scoped, so `f` may
/// borrow from the enclosing test.
fn with_compiler_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| {
        match std::thread::Builder::new()
            .stack_size(mllc::COMPILER_STACK_SIZE)
            .spawn_scoped(s, f)
            .expect("Failed to spawn compiler thread")
            .join()
        {
            Ok(v) => v,
            Err(e) => std::panic::resume_unwind(e),
        }
    })
}

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
