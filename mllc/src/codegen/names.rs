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
/// Emit a Lua string literal for a mata-ll `String`, which is a raw byte
/// array (see HASKDIFF.md "Strings and ByteStrings"). Each byte is emitted
/// so the resulting Lua string has EXACTLY those bytes: printable ASCII goes
/// through verbatim, control bytes and the high half (128..=255) use Lua's
/// `\ddd` decimal byte escape. This is what keeps `\181` in source a single
/// byte 181 in the output — matching the byte-wise `show` side.
pub(super) fn lua_quoted_string(s: &[u8]) -> String {
    let mut out = String::from("\"");
    for &b in s {
        match b {
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            0x20..=0x7e => out.push(b as char),
            // Control bytes and the high half escape as a fixed-width `\ddd`
            // decimal byte escape — the historical format for control bytes,
            // and unambiguous next to a following digit.
            _ => out.push_str(&format!("\\{:03}", b)),
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
    format!("[{}]", lua_quoted_string(name.as_bytes()))
}

/// Suffix that reads a LuaDict field from a table value: `.name` or `["name"]`.
pub(super) fn lua_field_index(name: &str) -> String {
    if lua_bare_key_ok(name) { format!(".{}", name) } else { lua_key_string(name) }
}

/// Assignment target inside a table constructor: `name = ` or `["name"] = `.
pub(super) fn lua_field_assign(name: &str) -> String {
    if lua_bare_key_ok(name) { format!("{} = ", name) } else { format!("{} = ", lua_key_string(name)) }
}

/// Source names that compile to a DIFFERENTLY-NAMED runtime helper: the
/// ByteString primitives (entries of the `__mll_bs` table), `runST`, the
/// HashMap family, `main`, and the Prelude names whose bare spelling is a
/// Lua keyword or a Lua global the runtime must not shadow (`return`,
/// `not`, `print`, `error`, `exit`, `try`, `catch`). One table:
/// `sanitize_name` reads it, module.rs seeds the known top-level names
/// from its keys, and the prelude-name check pins every target but
/// `main`'s (the entry point, emitted by the module) to a binding in the
/// runtime text. (The ST array / IORef intrinsics rename through their
/// own family table, crate::intrinsics; Lua keywords that are plain user
/// identifiers — `end`, `then`, `do`, … — are escaped in `sanitize_name`'s
/// match.)
pub(super) const RUNTIME_RENAMES: &[(&str, &str)] = &[
    ("main", "__run"),
    ("return", "return_"),
    ("not", "not_"),
    ("print", "print_"),
    // error_ forces its message before raising; Lua's bare `error` would
    // hand a thunk to error() and print "table: 0x...".
    ("error", "error_"),
    // exit_ unwraps the ExitValue ADT (Normal / Err code) and calls
    // os.exit; a bare `exit` would reference an undefined Lua global.
    ("exit", "exit_"),
    ("try", "try_"),
    ("catch", "catch_"),
    ("bsEmpty", "__mll_bs_empty"),
    ("bsLength", "__mll_bs[1]"),
    ("bsIndex", "__mll_bs[2]"),
    ("bsSub", "__mll_bs[3]"),
    ("bsSingleton", "__mll_bs[4]"),
    ("bsConcat", "__mll_bs[5]"),
    ("bsNull", "__mll_bs[6]"),
    ("bsHead", "__mll_bs[7]"),
    ("bsTail", "__mll_bs[8]"),
    ("bsCons", "__mll_bs[9]"),
    ("bsSnoc", "__mll_bs[10]"),
    ("bsReplicate", "__mll_bs[11]"),
    ("bsPack", "__mll_bs[12]"),
    ("bsUnpack", "__mll_bs[13]"),
    ("bsMap", "__mll_bs[14]"),
    ("bsFoldl", "__mll_bs[15]"),
    ("bsXor", "__mll_bs[16]"),
    ("bsZipWith", "__mll_bs[17]"),
    ("bsToString", "__mll_bs[18]"),
    ("bsFromString", "__mll_bs[19]"),
    ("bsGetU16LE", "__mll_bs[20]"),
    ("bsGetU32LE", "__mll_bs[21]"),
    ("bsGetI8", "__mll_bs[22]"),
    ("bsGetI16LE", "__mll_bs[23]"),
    ("bsPutI16LE", "__mll_bs[24]"),
    ("bsConcatList", "__mll_bs[25]"),
    // runST forces the state thread's result to WHNF (GHC: demanding
    // `runST m` demands the returned value), collapsing a suspended
    // terminal `pure e` so no raw thunk escapes the ST boundary.
    ("runST", "__mll_run_st"),
    ("hmEmpty", "hashmap_empty"),
    ("hmInsert", "hashmap_insert"),
    ("hmLookup", "hashmap_lookup"),
    ("hmDelete", "hashmap_delete"),
    ("hmSize", "hashmap_size"),
    ("hmKeys", "hashmap_keys"),
    ("hmValues", "hashmap_values"),
    ("hmMember", "hashmap_member"),
    ("hmFromList", "hashmap_fromList"),
    ("hmToList", "hashmap_toList"),
];

pub fn sanitize_name(name: &str) -> String {
    // The ST array / IORef intrinsics compile to their first-class action
    // closures (one table for the family: crate::intrinsics).
    if let Some(i) = crate::intrinsics::st_intrinsic(name) {
        return i.closure.to_string();
    }
    if let Some((_, target)) = RUNTIME_RENAMES.iter().find(|(source, _)| *source == name) {
        return target.to_string();
    }
    match name {
        "end" => "end_".to_string(),
        "then" => "then_".to_string(),
        "do" => "do_".to_string(),
        "in" => "in_".to_string(),
        "or" => "or_".to_string(),
        "and" => "and_".to_string(),
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
                    // A compiler-generated name built from an instance head can
                    // carry the head's spelling — spaces between type arguments
                    // (`gix_D1 d f`), colons from an operator constructor
                    // (`gix_:+: a b`), and its parentheses/commas. None are
                    // valid in a Lua identifier; fold them to `_` so the emitted
                    // definition and every reference sanitize identically.
                    ' ' | ':' | '(' | ')' | ',' => s.push('_'),
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
            // Single-leading-underscore names are mangled into a namespace
            // disjoint from the emitter's own temporaries (_s, _cg, _r, _u,
            // _arg0, _warg0, _ffi0, _v spills, …): a user binding spelled
            // like a temp shared its emitted name with whatever temporary
            // the surrounding emission introduced, with nothing enforcing
            // disjointness. "_usr" cannot collide back — no temporary
            // starts with it — and the mapping is injective ("_usr" + name)
            // and applied uniformly at binding and use (derive-generated
            // single-underscore TIR binders like `_nw` mangle consistently
            // on both sides too). The `__` namespace is EXEMPT: it is
            // lexer-reserved from source, so a `__` name here is a
            // compiler-minted reference — possibly to a runtime helper
            // (`__mll_show_arg`) whose definition lives in runtime.lua text
            // and never passes through this function.
            if s.starts_with('_') && !s.starts_with("__") {
                s.insert_str(0, "_usr");
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
        "ne_Int" | "ne_Number" | "ne_String" | "ne_Bool" | "ne_ByteString" => Some("~="),
        "ord_lt__Int" | "ord_lt__Number" | "ord_lt__String" | "ord_lt__ByteString" => Some("<"),
        "ord_gt__Int" | "ord_gt__Number" | "ord_gt__String" | "ord_gt__ByteString" => Some(">"),
        "ord_le__Int" | "ord_le__Number" | "ord_le__String" | "ord_le__ByteString" => Some("<="),
        "ord_ge__Int" | "ord_ge__Number" | "ord_ge__String" | "ord_ge__ByteString" => Some(">="),
        "semigroup_String" => Some(".."),
        _ => None,
    }
}

/// Per-operand strictness of a NON-CONTROL infix operator (`>>=`, `>>`,
/// `$`, `.` carry effect/closure structure and are not asked): whether
/// the emitted operator forces that operand when it runs. One statement
/// of what the InfixApp emitter arms do, read by the split pass (which
/// may pull a deep operand into an eager binding only where the operator
/// forces it anyway) and asserted by `operator_infix_ast` for the native
/// path:
///
///   * the native Lua operators (`operator_infix_ast` over `is_builtin_op`)
///     and the integer division helpers (`intdiv_infix_ast`) force both
///     operands — a thunk is a table, which corrupts arithmetic and
///     comparison; `<>` is among them: it exists at String and ByteString
///     only (Lua `..`), the list Semigroup is rejected by the checker;
///   * `&&`/`||` emit Lua `and`/`or` over forced operands, and Lua
///     evaluates the right operand only when the left decided nothing —
///     left strict, right lazy;
///   * `++` is the lazy list append (`__mll_list_append`), which forces
///     its left list at entry and suspends the right — left strict, right
///     lazy; `!!` forces the index, not the list (`__mll_list_index`);
///   * `:` and `seq`'s second operand build lazily (`cons_infix_ast`,
///     `seq_inline_ast` forces only its first); every user or unknown
///     operator is a call whose convention the callee decides — nothing
///     is claimed.
pub(crate) fn infix_operand_strictness(op: &str, lhs_ty: &crate::types::Ty, rhs_ty: &crate::types::Ty) -> (bool, bool) {
    let string_like = |ty: &crate::types::Ty| matches!(ty, crate::types::Ty::Con(n) if n == "String" || n == "ByteString");
    match op {
        "+" | "-" | "*" | "/" | "%" | "==" | "/=" | "~=" | "<" | ">" | "<=" | ">="
        | "div" | "mod" | "quot" | "rem" => (true, true),
        "<>" => (string_like(lhs_ty), string_like(rhs_ty)),
        "&&" | "||" | "++" | "seq" => (true, false),
        "!!" => (false, true),
        _ => (false, false),
    }
}

pub(super) fn is_builtin_op(op: &str) -> bool {
    // NOTE: `^` is deliberately NOT here — Lua's `^` is float power. It is a
    // Prelude function ((^), exponentiation by squaring over `*`), emitted as a
    // call like any user operator.
    // `div`/`mod`/`quot`/`rem` never reach operator_infix_ast (the InfixApp
    // arm dispatches all four to intdiv_infix_ast first); they are listed
    // for the cheapness/WHNF predicates — their zero-divisor trap is
    // contains_trapping_op's separate question, same as `div` always was.
    matches!(op, "+" | "-" | "*" | "/" | "%" | "==" | "/=" | "~="
        | "<" | ">" | "<=" | ">=" | "++" | "<>" | "&&" | "||" | ".." | "$" | "."
        | "div" | "mod" | "quot" | "rem")
}
