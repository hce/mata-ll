/// Source embedding: carry the original .mll source inside the emitted Lua
/// so the .lua file can later be recompiled without the .mll being present
/// (`mll --recompile file.lua`).
///
/// Two forms, both placed at the very top of the emitted file (before the
/// runtime prelude), so the genuine markers always precede any user-derived
/// content — extraction takes the *earliest* marker and therefore cannot be
/// fooled by marker-lookalike text inside the embedded source or inside
/// generated string literals:
///
///   Comments (`--embed-source comments`):
///     --[==[ MLL-EMBEDDED-SOURCE-BEGIN
///     <source, verbatim>
///     MLL-EMBEDDED-SOURCE-END ]==]
///
///   Variable (`--embed-source var`):
///     local __SOURCE_CODE = [==[
///     <source, verbatim>]==]
///     ...
///     return { __SOURCE_CODE = __SOURCE_CODE, ... }   -- module exports
///
/// Robustness: both forms use Lua long brackets whose level (the number of
/// `=` signs) is chosen so the closing sequence `]=…=]` does not occur in the
/// source, so no source text can terminate the block early. The variable
/// form exploits Lua's rule that a newline immediately after the opening
/// long bracket is skipped: at runtime `__SOURCE_CODE` is exactly the source
/// text. Extraction is textual (this module), byte-exact in both forms.

/// How to embed the original source into the emitted Lua.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedMode {
    /// Inside a delimited Lua long-comment block.
    Comments,
    /// As a module string variable `__SOURCE_CODE`, included in the exports.
    Var,
}

/// Name of the exported source variable in `EmbedMode::Var`.
pub const SOURCE_VAR: &str = "__SOURCE_CODE";

const BEGIN_WORD: &str = "MLL-EMBEDDED-SOURCE-BEGIN";
const END_WORD: &str = "MLL-EMBEDDED-SOURCE-END";

/// Smallest long-bracket level (>= 1) whose closing sequence `]=…=]` does not
/// occur in `source`. Level 0 (`]]`) is never used — it is too common in
/// ordinary code to be a useful delimiter.
fn bracket_level(source: &str) -> usize {
    (1..)
        .find(|&n| !source.contains(&format!("]{}]", "=".repeat(n))))
        .unwrap()
}

/// Render the embedded-source block for `source` in the given mode.
/// The block is self-contained Lua (a comment, or a `local` binding) and is
/// intended to be placed at the very top of the emitted file.
pub fn embed_block(source: &str, mode: EmbedMode) -> String {
    let eq = "=".repeat(bracket_level(source));
    match mode {
        // Framing: one newline after the begin marker and one before the end
        // marker belong to the frame, not the source, so extraction strips
        // exactly what embedding added — byte-exact even when the source has
        // no trailing newline.
        EmbedMode::Comments => format!(
            "--[{eq}[ {BEGIN_WORD}\n{source}\n{END_WORD} ]{eq}]\n"
        ),
        // The newline after `[{eq}[` is skipped by Lua's long-string rule, so
        // the runtime value of __SOURCE_CODE is exactly `source`.
        EmbedMode::Var => format!(
            "local {SOURCE_VAR} = [{eq}[\n{source}]{eq}]\n"
        ),
    }
}

/// Locate the embedded source in previously emitted Lua and return it
/// byte-exactly, together with the embedding mode it was found in.
///
/// Both marker forms are searched for at line starts; if both match (the
/// embedded source itself may contain lookalike text), the one appearing
/// earliest in the file wins — the genuine block always comes first.
pub fn extract_source(lua: &str) -> Result<(String, EmbedMode), String> {
    let comment = find_comment_block(lua);
    let var = find_var_block(lua);
    match (comment, var) {
        (Some((cp, c)), Some((vp, v))) => {
            if cp <= vp { Ok((c, EmbedMode::Comments)) } else { Ok((v, EmbedMode::Var)) }
        }
        (Some((_, c)), None) => Ok((c, EmbedMode::Comments)),
        (None, Some((_, v))) => Ok((v, EmbedMode::Var)),
        (None, None) => Err(
            "no embedded MLL source found in this file\n\
             note: only .lua files emitted with --embed-source comments or --embed-source var\n\
             carry their original source; recompile reads the source from that embedded block"
                .to_string(),
        ),
    }
}

/// Is `pos` the start of a line in `s`?
fn at_line_start(s: &str, pos: usize) -> bool {
    pos == 0 || s.as_bytes()[pos - 1] == b'\n'
}

/// Find `--[=*[ MLL-EMBEDDED-SOURCE-BEGIN\n … \nMLL-EMBEDDED-SOURCE-END ]=*]`
/// (matching bracket levels) and return (position of the begin marker, source).
fn find_comment_block(lua: &str) -> Option<(usize, String)> {
    let mut search = 0;
    while let Some(rel) = lua[search..].find("--[") {
        let start = search + rel;
        search = start + 3;
        if !at_line_start(lua, start) {
            continue;
        }
        let rest = &lua[start + 3..];
        let level = rest.bytes().take_while(|&b| b == b'=').count();
        let opener_tail = format!("[ {BEGIN_WORD}\n");
        if !rest[level..].starts_with(&opener_tail) {
            continue;
        }
        let content_start = start + 3 + level + opener_tail.len();
        let end_marker = format!("\n{END_WORD} ]{}]", "=".repeat(level));
        if let Some(end) = lua[content_start..].find(&end_marker) {
            return Some((start, lua[content_start..content_start + end].to_string()));
        }
    }
    None
}

/// Find `local __SOURCE_CODE = [=*[\n … ]=*]` (matching bracket levels) and
/// return (position of the binding, source).
fn find_var_block(lua: &str) -> Option<(usize, String)> {
    let head = format!("local {SOURCE_VAR} = [");
    let mut search = 0;
    while let Some(rel) = lua[search..].find(&head) {
        let start = search + rel;
        search = start + head.len();
        if !at_line_start(lua, start) {
            continue;
        }
        let rest = &lua[start + head.len()..];
        let level = rest.bytes().take_while(|&b| b == b'=').count();
        if !rest[level..].starts_with("[\n") {
            continue;
        }
        let content_start = start + head.len() + level + 2;
        let closer = format!("]{}]", "=".repeat(level));
        if let Some(end) = lua[content_start..].find(&closer) {
            return Some((start, lua[content_start..content_start + end].to_string()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_hostile_content() {
        // Closers at several levels, fake markers at line starts, no trailing
        // newline — all must survive byte-exactly.
        let source = "a ]] b ]=] c ]==] d ]===]\n\
                      --[=[ MLL-EMBEDDED-SOURCE-BEGIN\n\
                      MLL-EMBEDDED-SOURCE-END ]=]\n\
                      local __SOURCE_CODE = [=[\n\
                      no trailing newline";
        for mode in [EmbedMode::Comments, EmbedMode::Var] {
            let block = embed_block(source, mode);
            let (out, found) = extract_source(&block).expect("block should extract");
            assert_eq!(out, source, "byte-exact round trip ({:?})", mode);
            assert_eq!(found, mode);
        }
    }

    #[test]
    fn empty_and_trailing_newline_sources() {
        for source in ["", "\n", "x", "x\n", "x\n\n"] {
            for mode in [EmbedMode::Comments, EmbedMode::Var] {
                let block = embed_block(source, mode);
                let (out, _) = extract_source(&block).expect("block should extract");
                assert_eq!(out, source, "source {:?} mode {:?}", source, mode);
            }
        }
    }

    #[test]
    fn plain_lua_yields_error() {
        let err = extract_source("local x = 1\nreturn x\n").unwrap_err();
        assert!(err.contains("no embedded MLL source"), "got: {err}");
    }
}
