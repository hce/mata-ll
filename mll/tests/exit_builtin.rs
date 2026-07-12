//! Regression tests for the Prelude builtin `exit :: ExitValue -> IO ()`
//! (SPEC "Standalone MATA-LL": `data ExitValue = Normal | Err Integer`).
//!
//! Before the fix, `exit Normal` / `exit (Err 3)` typechecked but the
//! emitted Lua referenced an undefined global `exit` and crashed at
//! runtime with "attempt to call a nil value".
//!
//! `exit` ends in os.exit, which terminates the calling process, so these
//! tests must never run the compiled program in-process. Each test runs
//! `mll -r` as a subprocess and asserts on the child's OS exit status;
//! os.exit can only ever take down the child, never the cargo test harness.

use std::process::{Command, Output};

/// Compile-and-run `source` via `mll -r` in a subprocess.
fn run_program(name: &str, source: &str) -> Output {
    let path = std::env::temp_dir().join(format!(
        "mll_exit_builtin_{}_{}.mll",
        name,
        std::process::id()
    ));
    std::fs::write(&path, source)
        .unwrap_or_else(|e| panic!("cannot write {}: {}", path.display(), e));
    let out = Command::new(env!("CARGO_BIN_EXE_mll"))
        .arg("-r")
        .arg(&path)
        .output()
        .expect("failed to spawn mll -r");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("lua"));
    out
}

/// `exit Normal` exits the process with code 0 — and actually terminates:
/// the statement after it must never run (a silent no-op `exit` would also
/// yield exit code 0, so assert the marker is absent).
#[test]
fn exit_normal_exits_zero_and_terminates() {
    let out = run_program(
        "normal",
        "main :: IO ()\nmain = do\n    exit Normal\n    putStrLn \"UNREACHABLE\"\n",
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "exit Normal should exit 0\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("UNREACHABLE"),
        "exit Normal did not terminate the process:\n{}",
        stdout
    );
}

/// `exit (Err 3)` — the SPEC example — exits the process with code 3.
#[test]
fn exit_err_exits_with_given_code() {
    let out = run_program("err3", "main :: IO ()\nmain = exit (Err 3)\n");
    assert_eq!(
        out.status.code(),
        Some(3),
        "exit (Err 3) should exit 3\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// LOS provides a separate Integer-taking FFI `exit` (LuaIO "os.exit");
/// wiring the Prelude ExitValue `exit` must not break it.
#[test]
fn los_integer_exit_still_works() {
    let out = run_program("los", "import LOS\n\nmain :: IO ()\nmain = exit 3\n");
    assert_eq!(
        out.status.code(),
        Some(3),
        "LOS exit 3 should exit 3\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
