//! The runtime prelude and its on-demand selection.
//!
//! `PRELUDE` embeds runtime.lua. `ondemand_prelude` emits only the prelude
//! definitions reachable from the generated body: the roots are the prelude
//! identifiers appearing in the body, closed transitively over inter-chunk
//! references. References are read from raw chunk text (comments and
//! strings included), so the emitted set only ever OVER-approximates — a
//! real dependency is never dropped. If a referenced prelude name is still
//! not provided by the emitted set, the whole prelude is emitted instead: a
//! parser bug degrades to a larger file, never to broken (nil-global)
//! output.

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
/// As a final guard, if any referenced prelude name is somehow not provided by
/// the emitted set, fall back to the whole prelude: a parser bug degrades to a
/// larger file, never to broken (nil-global) output.
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

    // Safety net: every prelude name referenced by the body or by an emitted
    // chunk must be defined in the emitted set. If not, the reachability logic
    // is wrong — emit the full prelude rather than broken code.
    let complete = idents(body).chain(idents(&out))
        .filter(|t| all_names.contains(t))
        .all(|t| provided.contains(t));
    if complete { out } else { PRELUDE.to_string() }
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
    // Global assignment `IDENT = …` (the FFI-boundary functions), but not `==`.
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
    // Global assignment `IDENT = …`.
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

const PRELUDE: &str = include_str!("runtime.lua");
