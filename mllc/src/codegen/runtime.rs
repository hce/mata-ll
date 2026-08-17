//! The runtime prelude and its on-demand selection.
//!
//! `PRELUDE` assembles runtime.lua and runtime_integer.lua (spliced in at a
//! marker) into one byte-identical text stream. `ondemand_prelude` emits only the prelude
//! definitions reachable from the generated body: the roots are the prelude
//! identifiers appearing in the body, closed transitively over inter-chunk
//! references. References are read from raw chunk text (comments and
//! strings included), so the emitted set only ever OVER-approximates — a
//! real dependency is never dropped, and completeness of the emitted set
//! is a consequence of the closure (asserted, see `ondemand_prelude`).
//! The whole prelude is never emitted: it declares more top-level locals
//! than Lua's 200-per-function limit, so as a "safety net" it would have
//! been an unloadable chunk, not a larger file.

/// One top-level runtime-prelude definition: the names it introduces and its
/// full source text (including any leading comment and multi-line body).
struct PChunk {
    provides: Vec<String>,
    text: String,
}

/// Emit only the runtime-prelude definitions reachable from `body`.
///
/// Roots are the prelude identifiers that appear in the generated program;
/// the reachable set is the transitive closure over inter-chunk references.
/// References are read from raw chunk text (comments and strings included), so
/// the closure only ever *over*-approximates — it never drops a real dependency.
/// Completeness (every prelude name referenced by the body or by an emitted
/// chunk is defined in the emitted set) follows from the closure — the same
/// chunk texts feed both — and is asserted below. An earlier version fell
/// back to the WHOLE prelude when the check failed; that text declares more
/// top-level locals than Lua's 200-per-function limit, so the fallback would
/// have been an unloadable chunk, not a safety net (see `prelude_*` tests).
pub(super) fn ondemand_prelude(body: &str) -> String {
    let chunks = parse_prelude_chunks();
    let all_names: std::collections::HashSet<&str> =
        chunks.iter().flat_map(|c| c.provides.iter().map(String::as_str)).collect();

    // name -> chunks that provide it (a name may be forward-declared then
    // assigned, so more than one chunk can provide it; include them all).
    let mut providers: std::collections::HashMap<&str, Vec<usize>> = std::collections::HashMap::new();
    for (i, c) in chunks.iter().enumerate() {
        for n in &c.provides {
            providers.entry(n.as_str()).or_default().push(i);
        }
    }

    // Roots: prelude names referenced by the generated body.
    let mut needed: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut work: Vec<&str> = Vec::new();
    for tok in idents(body) {
        if all_names.contains(tok) && needed.insert(tok) {
            work.push(tok);
        }
    }
    // Transitive closure over the references inside each providing chunk.
    while let Some(name) = work.pop() {
        if let Some(idxs) = providers.get(name) {
            for &i in idxs {
                for dep in idents(&chunks[i].text) {
                    if all_names.contains(dep) && needed.insert(dep) {
                        work.push(dep);
                    }
                }
            }
        }
    }

    // Assemble the reachable chunks in their original order.
    let mut out = String::from("-- MLL Runtime (on-demand subset)\n");
    let mut provided: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for c in &chunks {
        if c.provides.iter().any(|n| needed.contains(n.as_str())) {
            out.push_str(&c.text);
            for n in &c.provides {
                provided.insert(n.as_str());
            }
        }
    }

    // Every prelude name referenced by the body or by an emitted chunk is
    // defined in the emitted set — by construction of the closure above.
    debug_assert!(
        idents(body).chain(idents(&out))
            .filter(|t| all_names.contains(t))
            .all(|t| provided.contains(t)),
        "ondemand_prelude: the reachability closure left a referenced prelude name unprovided"
    );
    out
}

/// Split the prelude into top-level definition chunks. A chunk starts at a
/// column-0 `local function`, `local …`, or `IDENT = …` line and runs until the
/// next such line; everything else (bodies, `end`, `if/else`, leading comments)
/// stays with its definition.
fn parse_prelude_chunks() -> Vec<PChunk> {
    let mut chunks: Vec<PChunk> = Vec::new();
    let mut cur: Option<PChunk> = None;
    let mut pending = String::new(); // comments/blanks awaiting the next def
    for line in PRELUDE.lines() {
        if is_def_start(line) {
            if let Some(c) = cur.take() {
                chunks.push(c);
            }
            let mut text = std::mem::take(&mut pending);
            text.push_str(line);
            text.push('\n');
            cur = Some(PChunk { provides: provided_names(line), text });
        } else {
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with("--") {
                // Buffer: attaches to the next definition (or flushed into the
                // current body if a continuation line comes first).
                pending.push_str(line);
                pending.push('\n');
            } else if let Some(c) = cur.as_mut() {
                c.text.push_str(&pending);
                pending.clear();
                c.text.push_str(line);
                c.text.push('\n');
            } else {
                pending.push_str(line);
                pending.push('\n');
            }
        }
    }
    if let Some(mut c) = cur {
        c.text.push_str(&pending);
        chunks.push(c);
    }
    chunks
}

/// Is `line` the start of a top-level prelude definition (column 0)?
pub(super) fn is_def_start(line: &str) -> bool {
    if line.starts_with([' ', '\t']) {
        return false;
    }
    if line.starts_with("local ") {
        return true;
    }
    // Assignment `IDENT = …` to a forward-declared local (the FFI-boundary
    // functions, declared together for mutual recursion), but not `==`.
    let name_len = line.bytes().take_while(|&b| b == b'_' || b.is_ascii_alphanumeric()).count();
    if name_len == 0 || line.as_bytes()[0].is_ascii_digit() {
        return false;
    }
    let after = line[name_len..].trim_start();
    after.starts_with('=') && !after.starts_with("==")
}

/// The names a definition line introduces.
pub(super) fn provided_names(line: &str) -> Vec<String> {
    let l = line.trim();
    if let Some(rest) = l.strip_prefix("local function ") {
        return vec![rest.chars().take_while(|c| *c == '_' || c.is_ascii_alphanumeric()).collect()];
    }
    if let Some(rest) = l.strip_prefix("local ") {
        // `local A`, `local A = …`, a forward decl `local A, B, C`, or a
        // `local A; do … end` block. Take the declaration up to `=`/`;`, then
        // the leading identifier of each comma-separated name.
        let decl = rest.split(['=', ';']).next().unwrap_or("");
        return decl.split(',')
            .map(|s| s.trim().chars().take_while(|c| *c == '_' || c.is_ascii_alphanumeric()).collect::<String>())
            .filter(|s| is_ident(s))
            .collect();
    }
    // Assignment `IDENT = …` to a forward-declared local.
    let name: String = l.chars().take_while(|c| *c == '_' || c.is_ascii_alphanumeric()).collect();
    if is_ident(&name) { vec![name] } else { vec![] }
}

pub(super) fn is_ident(s: &str) -> bool {
    let mut cs = s.chars();
    matches!(cs.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && cs.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// Every maximal `[A-Za-z0-9_]` run in `s` (a superset of the Lua identifiers).
pub(super) fn idents(s: &str) -> impl Iterator<Item = &str> {
    let b = s.as_bytes();
    let mut i = 0;
    std::iter::from_fn(move || {
        while i < b.len() && !(b[i] == b'_' || b[i].is_ascii_alphanumeric()) {
            i += 1;
        }
        if i >= b.len() {
            return None;
        }
        let start = i;
        while i < b.len() && (b[i] == b'_' || b[i].is_ascii_alphanumeric()) {
            i += 1;
        }
        Some(&s[start..i])
    })
}

/// The evaluation/FFI/IO substrate, with a `--#include runtime_integer.lua`
/// marker line where the Integer library belongs.
const RUNTIME: &str = include_str!("runtime.lua");
/// The arbitrary-precision Integer library: a file header, a `--#begin`
/// separator, then the library text spliced verbatim into the prelude.
const RUNTIME_INTEGER: &str = include_str!("runtime_integer.lua");
/// Marker line in runtime.lua replaced by the Integer library text.
const INTEGER_MARKER: &str = "--#include runtime_integer.lua\n";
/// Separator in runtime_integer.lua between its file header and the payload.
const INTEGER_BEGIN: &str = "--#begin\n";

/// The full runtime prelude: runtime.lua with the Integer library spliced in
/// at its marker. The assembled text is byte-identical to the historical
/// single-file runtime.lua, so chunk boundaries, tree-shaking, and emitted
/// programs are exactly what they were before the split.
static PRELUDE: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    let (before, after) = RUNTIME
        .split_once(INTEGER_MARKER)
        .expect("runtime.lua: missing `--#include runtime_integer.lua` marker line");
    let (_header, integer) = RUNTIME_INTEGER
        .split_once(INTEGER_BEGIN)
        .expect("runtime_integer.lua: missing `--#begin` separator line");
    let mut s = String::with_capacity(before.len() + integer.len() + after.len());
    s.push_str(before);
    s.push_str(integer);
    s.push_str(after);
    s
});

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole prelude cannot be a fallback: it declares more column-0
    /// locals than Lua allows per function (200), so emitting it would
    /// produce a chunk the host refuses to load ("too many local
    /// variables"). This pins the fact that motivated dropping the fallback;
    /// if the prelude ever shrinks below the limit the assertion says so and
    /// the design note in `ondemand_prelude` can be revisited.
    #[test]
    fn prelude_exceeds_luas_local_limit() {
        let locals: usize = PRELUDE
            .lines()
            .filter(|l| is_def_start(l) && l.starts_with("local "))
            .map(|l| provided_names(l).len())
            .sum();
        assert!(locals > 200, "prelude declares {} top-level locals", locals);
    }

    /// Completeness of the on-demand subset for every possible root: each
    /// prelude name, taken alone as the body's only reference, closes to a
    /// set that provides every prelude name its chunks mention (the
    /// debug_assert inside `ondemand_prelude` fires otherwise), and the
    /// subset stays within the local limit.
    #[test]
    fn prelude_subset_is_complete_for_every_root() {
        let chunks = parse_prelude_chunks();
        let names: Vec<String> = chunks.iter().flat_map(|c| c.provides.iter().cloned()).collect();
        assert!(!names.is_empty());
        for name in &names {
            let out = ondemand_prelude(name);
            assert!(out.starts_with("-- MLL Runtime (on-demand subset)"), "root {}", name);
            let locals: usize = out
                .lines()
                .filter(|l| is_def_start(l) && l.starts_with("local "))
                .map(|l| provided_names(l).len())
                .sum();
            assert!(locals <= 200, "root {} pulls in {} top-level locals", name, locals);
        }
    }
}
