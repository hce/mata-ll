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

/// Source names of runtime-library entities whose Lua name is spelled
/// differently: the ByteString primitives (entries of the `__mll_bs`
/// table), `runST`, the HashMap family, and the builtins whose bare
/// spelling is a Lua keyword or a Lua global (`return`, `not`, `error`,
/// `exit`, `try`, `catch`) or needs a prime (`foldl'`). One table:
/// `runtime_entity` reads it, module.rs seeds the known top-level names
/// from its keys, and the prelude-name check pins every target to a
/// binding in the runtime text. (The ST array / IORef intrinsics rename
/// through their own family table, crate::intrinsics.)
pub(super) const RUNTIME_RENAMES: &[(&str, &str)] = &[
    ("return", "return_"),
    ("not", "not_"),
    // error_ forces its message before raising; Lua's bare `error` would
    // hand a thunk to error() and print "table: 0x...".
    ("error", "error_"),
    // exit_ unwraps the ExitValue ADT (Normal / Err code) and calls
    // os.exit; a bare `exit` would reference an undefined Lua global.
    ("exit", "exit_"),
    ("try", "try_"),
    ("catch", "catch_"),
    ("foldl'", "foldl_prime"),
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

/// The Lua name of the runtime-library entity the mata-ll name `name`
/// denotes, or None when `name` is no such entity. Three families:
///
///   * the ST array / IORef intrinsics (their first-class action closures,
///     crate::intrinsics);
///   * the entities `RUNTIME_RENAMES` lists;
///   * every other entity the runtime text binds under a plain name, which
///     is its mata-ll name as well (`map`, `show`, `Just`, `eq_Int`,
///     `ord_compare__Int`, `add_Integer`, …).
///
/// The domain of this function is what `runtime_spellings` hands to
/// import resolution, which keeps every declaration and binder of a
/// program off these spellings (`modules::PreludeScope::claimed`): after
/// resolution a name this function answers for IS the runtime entity,
/// in every scope. That is what makes `sanitize_name` injective across
/// its two halves, and what lets fold.rs, demand.rs and the emitter key
/// their tables on these spellings.
pub(crate) fn runtime_entity(name: &str) -> Option<&'static str> {
    if let Some(i) = crate::intrinsics::st_intrinsic(name) {
        return Some(i.closure);
    }
    if let Some((_, target)) = RUNTIME_RENAMES.iter().find(|(source, _)| *source == name) {
        return Some(target);
    }
    if is_rename_target(name) {
        return None;
    }
    super::runtime::plain_runtime_name(name)
}

fn is_rename_target(lua_name: &str) -> bool {
    RUNTIME_RENAMES.iter().any(|(_, target)| *target == lua_name)
}

/// Every mata-ll spelling that denotes a runtime-library entity: the
/// domain of `runtime_entity`.
pub(crate) fn runtime_spellings() -> impl Iterator<Item = &'static str> {
    crate::intrinsics::ST_INTRINSICS
        .iter()
        .map(|i| i.source)
        .chain(RUNTIME_RENAMES.iter().map(|(source, _)| *source))
        .chain(super::runtime::plain_runtime_names().filter(|n| !is_rename_target(n)))
}

/// Global names of the Lua standard libraries (5.1 through 5.5 and
/// LuaJIT). The emitter and the runtime templates refer to several of
/// them by name inside function bodies (`select`, `type`, `rawequal`,
/// `error`, `tostring`, `math`, `string`, …), and an FFI declaration may
/// name any of them as its callee, so no program name may be given one of
/// these as its Lua name. `global` is a reserved word in Lua 5.5 builds
/// without the compatibility switch.
const LUA_GLOBALS: &[&str] = &[
    "arg", "assert", "bit", "bit32", "collectgarbage", "coroutine", "debug",
    "dofile", "error", "gcinfo", "getfenv", "getmetatable", "global", "io",
    "ipairs", "jit", "load", "loadfile", "loadstring", "math", "module",
    "newproxy", "next", "os", "package", "pairs", "pcall", "print",
    "rawequal", "rawget", "rawlen", "rawset", "require", "select", "setfenv",
    "setmetatable", "string", "table", "tonumber", "tostring", "type",
    "unpack", "utf8", "warn", "xpcall",
];

/// A Lua name the emitted program may not use for one of its own names:
/// a reserved word, a standard global, or a name the runtime text binds.
fn lua_reserved(lua_name: &str) -> bool {
    is_lua_keyword(lua_name)
        || LUA_GLOBALS.contains(&lua_name)
        || super::runtime::binds_name(lua_name)
}

/// The length of the longest reserved name, a bound for
/// `clear_of_reserved`'s scan.
static LONGEST_RESERVED: std::sync::LazyLock<usize> = std::sync::LazyLock::new(|| {
    LUA_GLOBALS
        .iter()
        .map(|n| n.len())
        .chain(super::runtime::bound_names().map(|n| n.len()))
        .max()
        .unwrap_or(0)
        .max("function".len())
});

/// `cand` moved clear of the reserved Lua names by trailing underscores.
///
/// Take the chain of names that differ from `cand` only in the number of
/// trailing underscores: `end`, `end_`, `end__`, … A chain with no
/// reserved member is left alone. Otherwise every member from the FIRST
/// reserved one on is shifted along the chain by the same distance — the
/// span from the first reserved member to the last one, inclusive — so
/// each lands past the last reserved member and no two land on one name
/// (`end` -> `end_`, `end_` -> `end__`; for `not`, where the runtime's
/// `not_` is reserved as well, `not` -> `not__`, `not_` -> `not___`).
/// Members before the first reserved one are not reserved and stay.
/// The map is injective on every chain, maps a chain into itself, and
/// never yields a reserved name.
fn clear_of_reserved(cand: String, reserved: fn(&str) -> bool) -> String {
    let stem = cand.trim_end_matches('_');
    let trailing = cand.len() - stem.len();
    let mut first: Option<usize> = None;
    let mut last = 0;
    let mut probe = stem.to_string();
    for k in 0.. {
        if probe.len() > *LONGEST_RESERVED {
            break;
        }
        if reserved(&probe) {
            first.get_or_insert(k);
            last = k;
        }
        probe.push('_');
    }
    match first {
        Some(first) if trailing >= first => {
            let mut out = cand;
            out.push_str(&"_".repeat(last - first + 1));
            out
        }
        _ => cand,
    }
}

/// The escape word of an operator character, written `_word_`.
fn operator_word(c: char) -> Option<&'static str> {
    Some(match c {
        '<' => "lt",
        '>' => "gt",
        '+' => "plus",
        '-' => "minus",
        '*' => "star",
        '/' => "slash",
        '!' => "bang",
        '?' => "q",
        '|' => "pipe",
        '&' => "amp",
        '=' => "eq",
        '^' => "caret",
        '~' => "tilde",
        '@' => "at",
        '$' => "dollar",
        '.' => "dot",
        ':' => "colon",
        '%' => "pct",
        '#' => "hash",
        _ => return None,
    })
}

/// The escape word of a character of a fully escaped name (`escaped`):
/// an operator character's word, a word for the other characters that
/// occur in names the compiler builds, or `u` and the code point in hex.
fn escape_word(c: char) -> String {
    if let Some(w) = operator_word(c) {
        return w.to_string();
    }
    match c {
        '\'' => "prime".to_string(),
        ' ' => "sp".to_string(),
        '(' => "lp".to_string(),
        ')' => "rp".to_string(),
        ',' => "comma".to_string(),
        '[' => "lb".to_string(),
        ']' => "rb".to_string(),
        crate::ast::MINT => "of".to_string(),
        _ => format!("u{:x}", c as u32),
    }
}

/// What an operator's Lua name starts with: `<+>` is `_usr_lt__plus__gt_`.
/// The same prefix serves a single-underscore identifier (`_x` is
/// `_usr_x`); no temporary of the emitter and no runtime name starts with
/// it.
const USR: &str = "_usr";

/// What a fully escaped name starts with. In the compiler's own `__`
/// namespace, and no compiler-made name starts with it (one that did is
/// escaped itself, see `plain`).
const ESCAPED: &str = "__usr_";

/// The Lua name of an operator: `_usr` and the word of each character.
/// A sequence of `_word_` groups parses back in one way only.
fn operator_name(op: &str) -> Option<String> {
    if op.is_empty() {
        return None;
    }
    let mut s = String::from(USR);
    for c in op.chars() {
        s.push('_');
        s.push_str(operator_word(c)?);
        s.push('_');
    }
    Some(s)
}

/// Is `lua_name` the `operator_name` of some operator?
fn is_operator_name(lua_name: &str) -> bool {
    let Some(mut rest) = lua_name.strip_prefix(USR) else { return false };
    if rest.is_empty() {
        return false;
    }
    while !rest.is_empty() {
        let Some(body) = rest.strip_prefix('_') else { return false };
        let Some(end) = body.find('_') else { return false };
        let word = &body[..end];
        let known = "<>+-*/!?|&=^~@$.:%#".chars().any(|c| operator_word(c) == Some(word));
        if !known {
            return false;
        }
        rest = &body[end + 1..];
    }
    true
}

/// The Lua name of an identifier-shaped name, when it has one that needs
/// no general escaping: letters, digits, underscores and primes only, with
/// a prime written `_prime` as ever. `None` sends the name to `escaped`.
///
/// Each condition rules out a second reading of the result:
///   * the name must not contain the text `_prime` itself (`_prime` in the
///     result then always stands for a prime; the word has no border, so
///     no occurrence can straddle a prime and its neighbours);
///   * without leading underscore it starts with a letter, and the result
///     is moved clear of the `reserved` Lua names (`clear_of_reserved`);
///   * with ONE leading underscore the result is `_usr_` and the rest —
///     clear of the emitter's own single-underscore temporaries — unless
///     that spells an operator's name;
///   * with two it is a name of the compiler's namespace (the lexer
///     refuses such a source identifier) and stays as it is, unless it
///     starts like an escaped name.
fn plain(name: &str, reserved: fn(&str) -> bool) -> Option<String> {
    let underscores = name.len() - name.trim_start_matches('_').len();
    let (lead, rest) = name.split_at(underscores.min(2));
    if !rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '\'')
        || rest.contains("_prime")
    {
        return None;
    }
    let body = rest.replace('\'', "_prime");
    match lead {
        "" => rest
            .starts_with(|c: char| c.is_ascii_alphabetic())
            .then(|| clear_of_reserved(body, reserved)),
        "_" => {
            let out = format!("{USR}_{body}");
            (!is_operator_name(&out)).then_some(out)
        }
        _ => {
            let out = format!("__{body}");
            (!out.starts_with(ESCAPED)).then_some(out)
        }
    }
}

/// The Lua name of a name `plain` has no spelling for — one with a
/// character Lua does not allow (a canonical `Module.name`, a generated
/// name, an operator inside a longer name, a non-ASCII letter) or with
/// the text `_prime` in it: `__usr_`, then the name with every letter
/// and digit as it is, every underscore doubled, and every other
/// character as `_word_`. Read left to right the result has one parse:
/// after an underscore, a second underscore is an underscore of the name
/// and anything else is a word up to the next underscore.
fn escaped(name: &str) -> String {
    let mut s = String::from(ESCAPED);
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if c == '_' {
            s.push_str("__");
        } else {
            s.push('_');
            s.push_str(&escape_word(c));
            s.push('_');
        }
    }
    s
}

/// The Lua name of the mata-ll name `name`: the one mapping every binder,
/// reference and table key of the emitter goes through.
///
/// INJECTIVE: two different names never get one Lua name. A runtime
/// entity has its fixed name (`runtime_entity`). Every other name takes
/// the first form that applies, and the forms cannot meet — each starts
/// differently, and none is a name Lua or the runtime owns:
///
///   | name                      | Lua name                    | starts with      |
///   |---------------------------|-----------------------------|------------------|
///   | operator `<+>`            | `_usr_lt__plus__gt_`        | `_usr_word_`     |
///   | identifier `go'`          | `go_prime`                  | a letter         |
///   | … reserved in Lua: `end`  | `end_`                      | a letter         |
///   | `_acc`                    | `_usr_acc`                  | `_usr_` + rest   |
///   | compiler's `__lam0`       | `__lam0`                    | `__`             |
///   | anything else: `M.f`      | `__usr_M_dot_f`             | `__usr_`         |
///
/// (`plain` and `escaped` hold the argument for each row.) Because the
/// mapping is injective, a table keyed by Lua name — the `__mll_fn` slot
/// table, the local-variable set, the accessor and concreteness tables —
/// is keyed by the mata-ll name itself, and two definitions can share a
/// slot only by sharing a name. Top-level definitions are emitted as
/// slots, so of all these spellings only a local variable's ever appears
/// in the generated file.
pub fn sanitize_name(name: &str) -> String {
    if let Some(lua_name) = runtime_entity(name) {
        return lua_name.to_string();
    }
    encoded(name, lua_reserved)
}

/// The Lua name of a name that is no runtime entity, clear of `reserved`.
fn encoded(name: &str, reserved: fn(&str) -> bool) -> String {
    if name.chars().all(|c| operator_word(c).is_some())
        && let Some(lua_name) = operator_name(name)
    {
        return lua_name;
    }
    plain(name, reserved).unwrap_or_else(|| escaped(name))
}

/// The field a class method has in a dictionary table (`dict.show`,
/// `{ show = …, _usr_eq__eq_ = … }`). The fields of a dictionary are a
/// namespace of their own — nothing but the methods of one class — so the
/// method's name is encoded as it stands, whatever it means as a
/// variable (a method the program calls `bsLength` is the field
/// `bsLength`, not the runtime's `__mll_bs[1]`), and only Lua's reserved
/// words are in its way (`return` is the field `return_`).
pub(super) fn dict_field_name(method: &str) -> String {
    encoded(method, is_lua_keyword)
}

/// The first local of the emitted module body — a `local`, a `local
/// function`, a parameter — whose name Lua or the runtime owns
/// (`lua_reserved`), if there is one. The emitted code refers to the
/// runtime's functions and to Lua's globals by name from inside every
/// function body, so a local of such a name would capture those
/// references for the rest of its scope. The name mapping never gives a
/// program's variable such a name and the emitter's own temporaries start
/// with an underscore; `generate` reports a hit as a compiler bug.
pub(super) fn reserved_binder(stmts: &[super::lua::Stmt]) -> Option<String> {
    use super::lua::{Expr, FnTarget, FuncBody, Stmt};
    fn check(names: &[String], hit: &mut Option<String>) {
        if hit.is_none() {
            *hit = names.iter().find(|n| lua_reserved(n)).cloned();
        }
    }
    fn expr(e: &Expr, hit: &mut Option<String>) {
        if let Expr::Func(params, body) = e {
            check(params, hit);
            match body {
                FuncBody::Inline(stmts) => block(stmts, hit),
                FuncBody::Block(b) => block(&b.0, hit),
            }
        }
        e.for_each_subexpr(&mut |c| expr(c, hit));
    }
    fn block(stmts: &[Stmt], hit: &mut Option<String>) {
        for s in stmts {
            match s {
                Stmt::Local(names, _) => check(names, hit),
                Stmt::Function { target, params, .. } => {
                    if let FnTarget::LocalFn(n) = target {
                        check(std::slice::from_ref(n), hit);
                    }
                    check(params, hit);
                }
                _ => {}
            }
            s.for_each_expr(&mut |e| expr(e, hit));
            s.for_each_block(&mut |b| block(b, hit));
        }
    }
    let mut hit = None;
    block(stmts, &mut hit);
    hit
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Names put together from the fragments every escape rule is about —
    /// primes and the word `_prime`, underscores in every position, Lua's
    /// reserved words and the runtime's names with and without trailing
    /// underscores, operator characters and their escape words, the
    /// prefixes of the escaped forms, a canonical name's dot, a generated
    /// name's separator, a non-ASCII letter — plus every spelling of the
    /// runtime library.
    fn probe_names() -> Vec<String> {
        const PARTS: &[&str] = &[
            "x", "A", "1", "_", "'", "_prime", "prime", "usr", "_usr", "local",
            "end", "not", "select", "show", "Int", "lt", "dot", "u2e", ".", "-",
            "<", ":", "+", "\u{b7}", "\u{e9}", " ", "map", "try", "e",
        ];
        let mut names: Vec<String> = vec![String::new()];
        let mut level: Vec<String> = vec![String::new()];
        for _ in 0..3 {
            let mut next = Vec::with_capacity(level.len() * PARTS.len());
            for prefix in &level {
                for part in PARTS {
                    next.push(format!("{prefix}{part}"));
                }
            }
            names.extend(next.iter().cloned());
            level = next;
        }
        names.extend(runtime_spellings().map(str::to_string));
        for (_, target) in RUNTIME_RENAMES {
            names.push(target.to_string());
            names.push(format!("{target}_"));
        }
        for name in LUA_GLOBALS {
            names.push(name.to_string());
            names.push(format!("{name}_"));
            names.push(format!("{name}__"));
        }
        names.sort();
        names.dedup();
        names
    }

    fn is_lua_identifier(s: &str) -> bool {
        lua_bare_key_ok(s)
    }

    /// (A `__` name that IS a helper of the runtime — the compiler writes
    /// `__mll_shows_prec` into a derived instance to mean that helper —
    /// is a reference to it by its Lua name, not a second name for it.)
    #[test]
    fn distinct_names_get_distinct_lua_names() {
        let mut seen: HashMap<String, String> = HashMap::new();
        for name in probe_names() {
            if name.starts_with("__") && super::super::runtime::binds_name(&name) {
                continue;
            }
            let lua_name = sanitize_name(&name);
            if let Some(other) = seen.insert(lua_name.clone(), name.clone()) {
                panic!("'{other}' and '{name}' both map to the Lua name '{lua_name}'");
            }
        }
    }

    /// A name that is no runtime entity and not of the compiler's `__`
    /// namespace maps to a Lua identifier that is neither a reserved word,
    /// nor a standard global, nor a name of the runtime.
    #[test]
    fn a_program_name_never_gets_a_name_lua_or_the_runtime_owns() {
        for name in probe_names() {
            if runtime_entity(&name).is_some() {
                continue;
            }
            let lua_name = sanitize_name(&name);
            assert!(is_lua_identifier(&lua_name), "'{name}' -> '{lua_name}' is no Lua identifier");
            if name.starts_with("__") && !lua_name.starts_with(ESCAPED) {
                continue;
            }
            assert!(!lua_reserved(&lua_name), "'{name}' -> '{lua_name}' is a reserved Lua name");
        }
    }

    /// The escaped forms start with prefixes nothing else may use: no
    /// runtime name, no standard global, and no name the emitter writes
    /// on its own (checked on its sources: a temporary is a string
    /// literal there).
    #[test]
    fn nothing_else_starts_like_an_escaped_name() {
        for name in super::super::runtime::bound_names().chain(LUA_GLOBALS.iter().copied()) {
            for prefix in [USR, ESCAPED, "__local_"] {
                assert!(!name.starts_with(prefix), "reserved name '{name}' starts with '{prefix}'");
            }
        }
        let emitter_sources = [
            include_str!("action.rs"), include_str!("analysis.rs"), include_str!("annot.rs"),
            include_str!("expr.rs"), include_str!("ffi.rs"), include_str!("function.rs"),
            include_str!("fuse.rs"), include_str!("hoist.rs"), include_str!("inline.rs"),
            include_str!("ioloop.rs"), include_str!("lua.rs"), include_str!("mod.rs"),
            include_str!("module.rs"), include_str!("opt.rs"), include_str!("pattern.rs"),
            include_str!("runtime.rs"), include_str!("strictness.rs"), include_str!("tailloop.rs"),
            include_str!("thunklift.rs"), include_str!("thunks.rs"), include_str!("util.rs"),
        ];
        for source in emitter_sources {
            for prefix in [USR, ESCAPED, "__local_"] {
                let literal = format!("\"{prefix}");
                assert!(!source.contains(&literal),
                    "an emitter source file has a string literal starting with '{prefix}'");
            }
        }
    }

    /// Two runtime entities never share a Lua name, and the identity
    /// family is exactly the plain runtime names that no rename targets.
    #[test]
    fn runtime_entities_have_lua_names_of_their_own() {
        let mut seen: HashMap<&str, &str> = HashMap::new();
        for name in runtime_spellings() {
            let lua_name = runtime_entity(name).expect("a runtime spelling denotes an entity");
            if let Some(other) = seen.insert(lua_name, name) {
                assert_eq!(other, name, "'{other}' and '{name}' share the Lua name '{lua_name}'");
            }
        }
        for (source, target) in RUNTIME_RENAMES {
            assert_eq!(runtime_entity(source), Some(*target));
            assert_eq!(runtime_entity(target), None, "'{target}' is a Lua name, not a mata-ll name");
        }
    }

    /// The spellings ordinary programs produce are the ones they always
    /// were; only a name that needs a second look is written differently.
    #[test]
    fn ordinary_names_keep_their_spelling() {
        for (name, lua_name) in [
            ("x", "x"), ("go'", "go_prime"), ("x''", "x_prime_prime"), ("fooBar_1", "fooBar_1"),
            ("_acc", "_usr_acc"), ("_", "_usr_"), ("__lam0", "__lam0"),
            ("__local_error", "__local_error"), ("__dict_Show", "__dict_Show"),
            ("end", "end_"), ("end_", "end__"), ("function", "function_"),
            ("<+>", "_usr_lt__plus__gt_"), ("==", "_usr_eq__eq_"), (">>=", "_usr_gt__gt__eq_"),
            ("Just", "Just"), ("map", "map"), ("not", "not_"), ("foldl'", "foldl_prime"),
            ("bsLength", "__mll_bs[1]"), ("newIORef", "__mll_ref_new"),
            ("show_Int", "show_Int"), ("main", "main"),
        ] {
            assert_eq!(sanitize_name(name), lua_name, "the Lua name of '{name}'");
        }
        for (name, lua_name) in [
            ("y_prime", "__usr_y__prime"),
            ("<.>", "_usr_lt__dot__gt_"), ("<->", "_usr_lt__minus__gt_"),
            ("-", "_usr_minus_"), ("_lt_", "__usr___lt__"),
            ("select", "select_"), ("string", "string_"), ("print", "print_"),
            ("not_", "not___"), ("hashmap_insert", "hashmap_insert_"),
            ("error_", "error___"), ("foldl_prime", "__usr_foldl__prime"),
            ("Data.Map.insert", "__usr_Data_dot_Map_dot_insert"),
            ("Data_Map.insert", "__usr_Data__Map_dot_insert"),
            ("show\u{b7}Color", "__usr_show_of_Color"),
            ("show_Color", "show_Color"),
            ("__usr_x", "__usr_____usr__x"),
            ("\u{e9}t\u{e9}", "__usr__ue9_t_ue9_"),
        ] {
            assert_eq!(sanitize_name(name), lua_name, "the Lua name of '{name}'");
        }
    }

    /// A dictionary's fields are the methods of one class under their own
    /// names: valid bare keys, distinct for distinct methods, and spelled
    /// as before for the builtin classes.
    #[test]
    fn dictionary_fields_are_named_after_their_methods() {
        for (method, field) in [
            ("show", "show"), ("return", "return_"), ("==", "_usr_eq__eq_"),
            ("-", "_usr_minus_"), ("pure", "pure"), ("bsLength", "bsLength"),
            ("not", "not_"), ("end", "end_"), ("fmap", "fmap"),
        ] {
            assert_eq!(dict_field_name(method), field, "the field of method '{method}'");
        }
        let mut seen: HashMap<String, String> = HashMap::new();
        for name in probe_names() {
            let field = dict_field_name(&name);
            assert!(lua_bare_key_ok(&field), "'{name}' -> '{field}' is no bare table key");
            if let Some(other) = seen.insert(field.clone(), name.clone()) {
                panic!("methods '{other}' and '{name}' share the field '{field}'");
            }
        }
    }
}
