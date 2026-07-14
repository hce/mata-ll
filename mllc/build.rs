// Captures the full git commit the compiler is built from and exposes it to
// codegen.rs as the MLLC_GIT_COMMIT environment variable (read via env!).
// Codegen stamps this, together with the crate version, into every compiled
// module as __MLLC_COMMIT / __MLLC_VERSION.
//
// Degrades gracefully: if git is missing or this isn't a git checkout (e.g. a
// crates.io tarball build), MLLC_GIT_COMMIT is set to "unknown" and the build
// never fails.

use std::path::Path;
use std::process::Command;

fn main() {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let manifest_dir = Path::new(&manifest_dir);

    // The workspace .git lives one level above this crate (mllc/../.git).
    // Emit rerun-if-changed lines so a new commit re-triggers the build:
    // HEAD itself, the loose ref HEAD points at, and the packed-refs file.
    // Only existing paths are emitted; pointing cargo at a missing path would
    // force a rebuild on every invocation.
    let git_dir = manifest_dir.join("..").join(".git");
    if git_dir.is_dir() {
        let head = git_dir.join("HEAD");
        if head.is_file() {
            println!("cargo:rerun-if-changed={}", head.display());
            if let Ok(contents) = std::fs::read_to_string(&head) {
                if let Some(target) = contents.strip_prefix("ref: ") {
                    let loose_ref = git_dir.join(target.trim());
                    if loose_ref.is_file() {
                        println!("cargo:rerun-if-changed={}", loose_ref.display());
                    }
                }
            }
        }
        let packed_refs = git_dir.join("packed-refs");
        if packed_refs.is_file() {
            println!("cargo:rerun-if-changed={}", packed_refs.display());
        }
    }

    // Run git from the crate directory; rev-parse finds the repository root
    // itself, so this works regardless of the build script's cwd. The full
    // (unabbreviated) hash is used so __MLLC_COMMIT is unambiguous.
    let commit = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(manifest_dir)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=MLLC_GIT_COMMIT={}", commit);
}
