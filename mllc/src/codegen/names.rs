//! Lua name and string-literal helpers.
//!
//! `sanitize_name` maps mata-ll identifiers to valid Lua names.
//! `is_lua_keyword` / `lua_bare_key_ok` decide when a field key may appear
//! bare. `lua_quoted_string` is the ONE canonical double-quoted Lua string
//! literal — every emitted source string must go through it.
//! `primitive_method_lua_op` is the single point of truth for which resolved
//! primitive typeclass methods inline to native Lua operators, shared by the
//! emission arm and the WHNF predicate so the two can never disagree.

/// Lua reserved words — cannot be used as a bare `.field` key or `{field = …}`,
/// nor as a name component of an FFI callee path (see `parser::validate_ffi_callee`).
pub(crate) fn is_lua_keyword(s: &str) -> bool {
    matches!(s,
        "and" | "break" | "do" | "else" | "elseif" | "end" | "false" | "for"
        | "function" | "goto" | "if" | "in" | "local" | "nil" | "not" | "or"
        | "repeat" | "return" | "then" | "true" | "until" | "while")
}

/// True when `name` can appear as a bare Lua identifier key (`.name`, `{name = …}`).
pub(super) fn lua_bare_key_ok(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !is_lua_keyword(name)
}

/// The ONE canonical Lua double-quoted string literal for `s`. Every place
/// that emits a source string into the generated Lua — expression literals
/// (`literal_ast`), pattern-match literals (`collect_pattern_conditions`),
/// LuaDict `as`-renamed table keys (`lua_key_string`) — must go through this
/// function: string escaping used to live in three hand-rolled copies, and
/// the two incomplete ones let a quote or a control character through raw,
/// producing Lua that would not even load.
///
/// Escapes `\` and `"` (the literal's own metacharacters), spells `\n`, `\r`
/// and `\t` by name, and turns every other control character (U+0000–U+001F,
/// U+007F) into a Lua `\ddd` decimal escape. The decimal form is always
/// emitted with all three digits (`\000`, not `\0`): Lua greedily reads up to
/// three digits after `\`, so a short escape followed by a literal digit
/// character would silently change the string's value.
pub(super) fn lua_quoted_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\{:03}", c as u32));
            }
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The ONE canonical Lua literal for a `Number` (Double) value. Rust's
/// `Display` for f64 drops a whole value's fraction (`10.0.to_string()` is
/// `"10"`), and Lua 5.3+ reads a bare `10` as a native INTEGER — so a
/// `Number` literal emitted that way put integer arithmetic behind a
/// Double-typed expression: exact small results by luck, silent 64-bit
/// wraparound past 2^63 (`10.0^20` became 7766279631452241920), and integer
/// `show` output. `Debug` formatting always keeps the float marker (`10.0`,
/// `1e20`), which Lua parses back as the identical float. Non-finite values
/// cannot appear in source literals but constant folding could produce
/// them; spell them as expressions.
pub(super) fn lua_number_literal(n: f64) -> String {
    if n.is_nan() {
        "(0/0)".to_string()
    } else if n.is_infinite() {
        if n < 0.0 { "(-math.huge)".to_string() } else { "math.huge".to_string() }
    } else {
        format!("{:?}", n)
    }
}

/// A bracketed Lua string-literal table key: `["na\"me"]`. Always valid —
/// the key text is escaped by the canonical `lua_quoted_string`.
pub(super) fn lua_key_string(name: &str) -> String {
    format!("[{}]", lua_quoted_string(name))
}

/// Suffix that reads a LuaDict field from a table value: `.name` or `["name"]`.
pub(super) fn lua_field_index(name: &str) -> String {
    if lua_bare_key_ok(name) { format!(".{}", name) } else { lua_key_string(name) }
}

/// Assignment target inside a table constructor: `name = ` or `["name"] = `.
pub(super) fn lua_field_assign(name: &str) -> String {
    if lua_bare_key_ok(name) { format!("{} = ", name) } else { format!("{} = ", lua_key_string(name)) }
}

pub(super) fn sanitize_name(name: &str) -> String {
    match name {
        "main" => "__run".to_string(),
        "return" => "return_".to_string(),
        "not" => "not_".to_string(),
        "print" => "print_".to_string(),
        // error_ forces its message before raising; Lua's bare `error` would
        // hand a thunk to error() and print "table: 0x...".
        "error" => "error_".to_string(),
        // exit_ unwraps the ExitValue ADT (Normal / Err code) and calls
        // os.exit; a bare `exit` would reference an undefined Lua global.
        "exit" => "exit_".to_string(),
        "end" => "end_".to_string(),
        "then" => "then_".to_string(),
        "do" => "do_".to_string(),
        "in" => "in_".to_string(),
        "or" => "or_".to_string(),
        "and" => "and_".to_string(),
        "try" => "try_".to_string(),
        "catch" => "catch_".to_string(),
        "bsEmpty" => "__mll_bs_empty".to_string(),
        "bsLength" => "__mll_bs[1]".to_string(),
        "bsIndex" => "__mll_bs[2]".to_string(),
        "bsSub" => "__mll_bs[3]".to_string(),
        "bsSingleton" => "__mll_bs[4]".to_string(),
        "bsConcat" => "__mll_bs[5]".to_string(),
        "bsNull" => "__mll_bs[6]".to_string(),
        "bsHead" => "__mll_bs[7]".to_string(),
        "bsTail" => "__mll_bs[8]".to_string(),
        "bsCons" => "__mll_bs[9]".to_string(),
        "bsSnoc" => "__mll_bs[10]".to_string(),
        "bsReplicate" => "__mll_bs[11]".to_string(),
        "bsPack" => "__mll_bs[12]".to_string(),
        "bsUnpack" => "__mll_bs[13]".to_string(),
        "bsMap" => "__mll_bs[14]".to_string(),
        "bsFoldl" => "__mll_bs[15]".to_string(),
        "bsXor" => "__mll_bs[16]".to_string(),
        "bsZipWith" => "__mll_bs[17]".to_string(),
        "bsToString" => "__mll_bs[18]".to_string(),
        "bsFromString" => "__mll_bs[19]".to_string(),
        "bsGetU16LE" => "__mll_bs[20]".to_string(),
        "bsGetU32LE" => "__mll_bs[21]".to_string(),
        "bsGetI8" => "__mll_bs[22]".to_string(),
        "bsGetI16LE" => "__mll_bs[23]".to_string(),
        "bsPutI16LE" => "__mll_bs[24]".to_string(),
        "bsConcatList" => "__mll_bs[25]".to_string(),
        // runST forces the state thread's result to WHNF (GHC: demanding
        // `runST m` demands the returned value), collapsing a suspended
        // terminal `pure e` so no raw thunk escapes the ST boundary.
        "runST" => "__mll_run_st".to_string(),
        "newSTArray" => "__mll_ma_new".to_string(),
        "readSTArray" => "__mll_ma_read".to_string(),
        "writeSTArray" => "__mll_ma_write".to_string(),
        "modifySTArray" => "__mll_ma_modify".to_string(),
        "stArrayLength" => "__mll_ma_length".to_string(),
        "newSTArrayFromList" => "__mll_ma_from_list".to_string(),
        "stArrayToList" => "__mll_ma_to_list".to_string(),
        "hmEmpty" => "hashmap_empty".to_string(),
        "hmInsert" => "hashmap_insert".to_string(),
        "hmLookup" => "hashmap_lookup".to_string(),
        "hmDelete" => "hashmap_delete".to_string(),
        "hmSize" => "hashmap_size".to_string(),
        "hmKeys" => "hashmap_keys".to_string(),
        "hmValues" => "hashmap_values".to_string(),
        "hmMember" => "hashmap_member".to_string(),
        "hmFromList" => "hashmap_fromList".to_string(),
        "hmToList" => "hashmap_toList".to_string(),
        _ => {
            let mut s = String::new();
            for c in name.chars() {
                match c {
                    '\'' => s.push_str("_prime"),
                    '<' => s.push_str("_lt_"),
                    '>' => s.push_str("_gt_"),
                    '+' => s.push_str("_plus_"),
                    '-' => s.push('_'),
                    '*' => s.push_str("_star_"),
                    '/' => s.push_str("_slash_"),
                    '!' => s.push_str("_bang_"),
                    '?' => s.push_str("_q_"),
                    '|' => s.push_str("_pipe_"),
                    '&' => s.push_str("_amp_"),
                    '=' => s.push_str("_eq_"),
                    '^' => s.push_str("_caret_"),
                    '~' => s.push_str("_tilde_"),
                    '@' => s.push_str("_at_"),
                    '$' => s.push_str("_dollar_"),
                    '[' => s.push_str("List_"),
                    ']' => {},
                    // Qualified-import separator: `Map.insert` -> `Map_insert`.
                    '.' => s.push('_'),
                    _ => s.push(c),
                }
            }
            // A valid mata-ll identifier can be a Lua reserved word (e.g.
            // `until`, `repeat`, `local`, `nil`, `function`). Emitting it bare
            // is a Lua syntax error, so escape with a trailing `_` — the same
            // convention as the explicit `end`/`then`/... arms above. Field
            // names sanitize identically, so record access stays consistent.
            if is_lua_keyword(&s) {
                s.push('_');
            }
            s
        }
    }
}

/// The native Lua operator a fully-applied (two-argument) resolved primitive
/// typeclass method inlines to, or None. Single point of truth shared by the
/// expr_ast App-arm inline and expr_yields_whnf, so the two can never
/// disagree about which calls become forced native operations.
pub(super) fn primitive_method_lua_op(name: &str) -> Option<&'static str> {
    match name {
        "eq_Int" | "eq_Number" | "eq_String" | "eq_Bool" | "eq_ByteString" => Some("=="),
        "ord_lt__Int" | "ord_lt__Number" | "ord_lt__String" | "ord_lt__ByteString" => Some("<"),
        "ord_gt__Int" | "ord_gt__Number" | "ord_gt__String" | "ord_gt__ByteString" => Some(">"),
        "ord_le__Int" | "ord_le__Number" | "ord_le__String" | "ord_le__ByteString" => Some("<="),
        "ord_ge__Int" | "ord_ge__Number" | "ord_ge__String" | "ord_ge__ByteString" => Some(">="),
        "semigroup_String" => Some(".."),
        _ => None,
    }
}

pub(super) fn is_builtin_op(op: &str) -> bool {
    matches!(op, "+" | "-" | "*" | "/" | "%" | "^" | "==" | "/=" | "~="
        | "<" | ">" | "<=" | ">=" | "++" | "<>" | "&&" | "||" | ".." | "$" | "."
        | "div" | "mod")
}
