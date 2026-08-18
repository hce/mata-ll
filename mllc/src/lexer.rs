use std::fmt;

use crate::ast::Span;
use crate::types::Diagnostic;

/// A lex diagnostic at a known source location. Built on the same
/// [`Diagnostic`] machinery as parse and type errors, so lex errors render
/// their span (`at line:col`) and `note:` lines the same way instead of
/// hand-formatting positions into the message text.
fn err_at(msg: impl Into<String>, line: usize, col: usize) -> Box<Diagnostic> {
    Box::new(Diagnostic::parse_at(msg, Span::new(line, col)))
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    IntLit(i64),
    /// An integer literal too large for `i64` — kept as its decimal-digit
    /// string. It can only be an `Integer` (arbitrary precision); the value is
    /// parsed to a bignum at codegen (`__int_from_decimal`).
    BigIntLit(String),
    NumLit(f64),
    /// A string literal, stored as its decoded BYTE sequence. mata-ll's
    /// `String` is the Lua string — a byte array with no encoding awareness
    /// (see HASKDIFF.md "Strings and ByteStrings") — so a source literal is
    /// lexed to bytes: a non-ASCII source character contributes its UTF-8
    /// bytes, and a numeric escape (`\181`) contributes exactly one byte.
    /// This is what keeps the input side round-trip-consistent with the
    /// `show` side, which reads and escapes per byte.
    StrLit(Vec<u8>),

    // Identifiers and operators
    Ident(String),      // lowercase start: variable, function
    UpperIdent(String),  // uppercase start: type, constructor
    Operator(String),    // +, -, *, etc.

    // Keywords
    KwModule,
    Import,
    Qualified,
    Data,
    Newtype,
    Class,
    Instance,
    Where,
    Let,
    In,
    Case,
    Of,
    If,
    Then,
    Else,
    Do,
    Intrinsic,
    Export,
    KwType,
    Deriving,
    Family,
    Infixl,
    Infixr,
    Infix,

    // Symbols
    Arrow,       // ->
    FatArrow,    // =>
    DblColon,    // ::
    Backslash,   // \.
    Comma,       // ,
    Semicolon,   // ;
    Eq,          // =
    Pipe,        // |
    Backtick,    // `
    Underscore,  // _
    LeftParen,   // (
    RightParen,  // )
    LeftBracket, // [
    RightBracket,// ]
    LeftBrace,   // {
    RightBrace,  // }
    At,          // @
    Bind,        // <-
    Tick,        // ' (DataKinds promoted constructor prefix)

    // Layout
    Indent(usize),  // indentation level at start of line
    Newline,

    EOF,
}

/// Renders every token the way it is spelled in source (quoted), or as a
/// short phrase for the tokens that have no spelling (end of file, line
/// breaks). Diagnostics interpolate this directly — "Expected '::', found
/// 'where'" — so no arm may fall back to the Rust `Debug` form.
impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let spelled = match self {
            Token::IntLit(n) => return write!(f, "integer literal '{}'", n),
            Token::BigIntLit(s) => return write!(f, "integer literal '{}'", s),
            Token::NumLit(n) => return write!(f, "number literal '{}'", n),
            Token::StrLit(s) => {
                return write!(f, "string literal \"{}\"", String::from_utf8_lossy(s))
            }
            Token::Ident(s) => return write!(f, "'{}'", s),
            Token::UpperIdent(s) => return write!(f, "'{}'", s),
            Token::Operator(s) => return write!(f, "'{}'", s),
            Token::KwModule => "module",
            Token::Import => "import",
            Token::Qualified => "qualified",
            Token::Data => "data",
            Token::Newtype => "newtype",
            Token::Class => "class",
            Token::Instance => "instance",
            Token::Where => "where",
            Token::Let => "let",
            Token::In => "in",
            Token::Case => "case",
            Token::Of => "of",
            Token::If => "if",
            Token::Then => "then",
            Token::Else => "else",
            Token::Do => "do",
            Token::Intrinsic => "intrinsic",
            Token::Export => "export",
            Token::KwType => "type",
            Token::Deriving => "deriving",
            Token::Family => "family",
            Token::Infixl => "infixl",
            Token::Infixr => "infixr",
            Token::Infix => "infix",
            Token::Arrow => "->",
            Token::FatArrow => "=>",
            Token::DblColon => "::",
            Token::Backslash => "\\",
            Token::Comma => ",",
            Token::Semicolon => ";",
            Token::Eq => "=",
            Token::Pipe => "|",
            Token::Backtick => "`",
            Token::Underscore => "_",
            Token::LeftParen => "(",
            Token::RightParen => ")",
            Token::LeftBracket => "[",
            Token::RightBracket => "]",
            Token::LeftBrace => "{",
            Token::RightBrace => "}",
            Token::At => "@",
            Token::Bind => "<-",
            Token::Tick => "'",
            Token::Indent(n) => {
                return write!(f, "start of a new line (indentation {})", n)
            }
            Token::Newline => return write!(f, "end of line"),
            Token::EOF => return write!(f, "end of file"),
        };
        write!(f, "'{}'", spelled)
    }
}

#[derive(Debug, Clone)]
pub struct Located {
    pub token: Token,
    pub line: usize,
    pub col: usize,
}

pub fn lex(source: &str) -> Result<Vec<Located>, Box<Diagnostic>> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let mut pos = 0;
    let mut line = 1;
    let mut col = 1;
    let mut at_line_start = true;

    while pos < chars.len() {
        // Track indentation at start of line
        if at_line_start {
            let mut indent = 0;
            while pos < chars.len() && chars[pos] == ' ' {
                indent += 1;
                pos += 1;
                col += 1;
            }
            if pos < chars.len() && chars[pos] == '\t' {
                let mut diag = err_at("Tab character in indentation", line, col);
                diag.notes.push(
                    "indent with spaces: layout-sensitive parsing would \
                     otherwise change meaning with the reader's tab width"
                        .to_string(),
                );
                return Err(diag);
            }
            // Skip blank lines
            if pos < chars.len() && chars[pos] == '\n' {
                pos += 1;
                line += 1;
                col = 1;
                continue;
            }
            // Skip comment-only lines (same test as the mid-line rule: a
            // line starting with an operator such as `-->` or `--|` is a
            // continuation line, not a comment)
            if is_line_comment_start(&chars, pos) {
                while pos < chars.len() && chars[pos] != '\n' {
                    pos += 1;
                }
                if pos < chars.len() {
                    pos += 1;
                    line += 1;
                    col = 1;
                }
                continue;
            }
            if pos < chars.len() && chars[pos] != '\n' {
                tokens.push(Located {
                    token: Token::Indent(indent),
                    line,
                    col: 1,
                });
            }
            at_line_start = false;
        }

        let ch = chars[pos];

        // Newline
        if ch == '\n' {
            pos += 1;
            line += 1;
            col = 1;
            at_line_start = true;
            continue;
        }

        // Whitespace (non-newline)
        if ch == ' ' || ch == '\t' || ch == '\r' {
            pos += 1;
            col += 1;
            continue;
        }

        // Line comment
        if is_line_comment_start(&chars, pos) {
            while pos < chars.len() && chars[pos] != '\n' {
                pos += 1;
            }
            continue;
        }

        // Block comment {- ... -}
        if ch == '{' && pos + 1 < chars.len() && chars[pos + 1] == '-' {
            let (open_line, open_col) = (line, col);
            pos += 2;
            col += 2;
            let mut depth = 1;
            while pos < chars.len() && depth > 0 {
                if chars[pos] == '{' && pos + 1 < chars.len() && chars[pos + 1] == '-' {
                    depth += 1;
                    pos += 2;
                    col += 2;
                } else if chars[pos] == '-' && pos + 1 < chars.len() && chars[pos + 1] == '}' {
                    depth -= 1;
                    pos += 2;
                    col += 2;
                } else {
                    if chars[pos] == '\n' {
                        line += 1;
                        col = 1;
                    } else {
                        col += 1;
                    }
                    pos += 1;
                }
            }
            if depth > 0 {
                // Everything to the end of the file was swallowed as
                // comment; the parser would report an unrelated "found end
                // of file" far from the cause.
                let mut diag = err_at(
                    "Unterminated block comment: this `{-` has no matching `-}`",
                    open_line,
                    open_col,
                );
                diag.notes.push(
                    "block comments nest, so every `{-` needs its own `-}`"
                        .to_string(),
                );
                return Err(diag);
            }
            continue;
        }

        let tok_line = line;
        let tok_col = col;

        // String literal
        if ch == '"' {
            pos += 1;
            col += 1;
            let mut bytes: Vec<u8> = Vec::new();
            while pos < chars.len() && chars[pos] != '"' {
                if chars[pos] == '\\' {
                    // Escape. `pos` is on the backslash; `lex_string_escape`
                    // advances it past the whole escape and pushes 0..n bytes
                    // (n == 0 for `\&` and for a string gap).
                    lex_string_escape(&chars, &mut pos, &mut col, &mut line, &mut bytes)?;
                } else {
                    if chars[pos] == '\n' {
                        return Err(err_at(
                            "Unterminated string literal",
                            tok_line, tok_col,
                        ));
                    }
                    // A source character contributes its UTF-8 bytes: mata-ll
                    // strings are byte arrays, so `μ` is the two bytes 0xC2 0xB5.
                    let mut buf = [0u8; 4];
                    bytes.extend_from_slice(chars[pos].encode_utf8(&mut buf).as_bytes());
                    pos += 1;
                    col += 1;
                }
            }
            if pos >= chars.len() {
                return Err(err_at(
                    "Unterminated string literal",
                    tok_line, tok_col,
                ));
            }
            pos += 1; // closing quote
            col += 1;
            tokens.push(Located {
                token: Token::StrLit(bytes),
                line: tok_line,
                col: tok_col,
            });
            continue;
        }

        // Number literal
        if ch.is_ascii_digit() {
            let start = pos;
            while pos < chars.len() && chars[pos].is_ascii_digit() {
                pos += 1;
                col += 1;
            }
            let mut is_float = false;
            // Fractional part: `.` digit+ . A dot NOT followed by a digit is
            // not part of the number (so `1..3` stays a range and `x.field`
            // an accessor).
            if pos + 1 < chars.len() && chars[pos] == '.' && chars[pos + 1].is_ascii_digit() {
                is_float = true;
                pos += 1; // skip dot
                col += 1;
                while pos < chars.len() && chars[pos].is_ascii_digit() {
                    pos += 1;
                    col += 1;
                }
            }
            // Exponent: (e|E) [+|-] digit+ (Haskell 2010 §2.5). Maximal munch
            // needs at least one exponent digit; otherwise the `e` begins an
            // identifier (`1e` lexes as `1` then `e`). A bare-mantissa exponent
            // (`1e5`) is Fractional in Haskell, so it is a float literal too.
            if pos < chars.len() && (chars[pos] == 'e' || chars[pos] == 'E') {
                let mut look = pos + 1;
                if look < chars.len() && (chars[look] == '+' || chars[look] == '-') {
                    look += 1;
                }
                if look < chars.len() && chars[look].is_ascii_digit() {
                    is_float = true;
                    // Consume `e`, the optional sign, then the exponent digits.
                    while pos < look {
                        pos += 1;
                        col += 1;
                    }
                    while pos < chars.len() && chars[pos].is_ascii_digit() {
                        pos += 1;
                        col += 1;
                    }
                }
            }
            let s: String = chars[start..pos].iter().collect();
            if is_float {
                let n: f64 = s.parse().map_err(|e| {
                    err_at(format!("Invalid number '{}': {}", s, e), tok_line, tok_col)
                })?;
                tokens.push(Located {
                    token: Token::NumLit(n),
                    line: tok_line,
                    col: tok_col,
                });
            } else {
                // Only ASCII digits were consumed, so the sole way `parse` fails
                // is overflow past `maxBound :: Int` (i64::MAX). Such a literal
                // does not fit `Int`; it becomes a `BigIntLit` (an `Integer`),
                // matching GHC where an integer literal is an `Integer`.
                let token = match s.parse::<i64>() {
                    Ok(n) => Token::IntLit(n),
                    Err(_) => Token::BigIntLit(s),
                };
                tokens.push(Located {
                    token,
                    line: tok_line,
                    col: tok_col,
                });
            }
            continue;
        }

        // Identifier or keyword
        if ch.is_alphabetic() || ch == '_' {
            let start = pos;
            while pos < chars.len() && (chars[pos].is_alphanumeric() || chars[pos] == '_' || chars[pos] == '\'') {
                pos += 1;
                col += 1;
            }
            let word: String = chars[start..pos].iter().collect();
            let token = match word.as_str() {
                "module" => Token::KwModule,
                "import" => Token::Import,
                "qualified" => Token::Qualified,
                // "as" not reserved — usable as variable name (the parser checks
                // Ident("as") contextually: qualified imports and record-field
                // LuaDict key renames `field as "key" :: T`)

                "data" => Token::Data,
                "newtype" => Token::Newtype,
                "class" => Token::Class,
                "instance" => Token::Instance,
                "where" => Token::Where,
                "let" => Token::Let,
                "in" => Token::In,
                "case" => Token::Case,
                "of" => Token::Of,
                "if" => Token::If,
                "then" => Token::Then,
                "else" => Token::Else,
                "do" => Token::Do,
                "intrinsic" => Token::Intrinsic,
                "export" => Token::Export,
                "type" => Token::KwType,
                "deriving" => Token::Deriving,
                "family" => Token::Family,
                "infixl" => Token::Infixl,
                "infixr" => Token::Infixr,
                "infix" => Token::Infix,
                "True" => Token::UpperIdent("True".to_string()),
                "False" => Token::UpperIdent("False".to_string()),
                "_" => Token::Underscore,
                _ => {
                    if word.starts_with(|c: char| c.is_uppercase()) {
                        Token::UpperIdent(word)
                    } else {
                        Token::Ident(word)
                    }
                }
            };
            tokens.push(Located {
                token,
                line: tok_line,
                col: tok_col,
            });
            continue;
        }

        // Operators and symbols
        match ch {
            '(' => {
                tokens.push(Located { token: Token::LeftParen, line: tok_line, col: tok_col });
                pos += 1; col += 1;
            }
            ')' => {
                tokens.push(Located { token: Token::RightParen, line: tok_line, col: tok_col });
                pos += 1; col += 1;
            }
            '[' => {
                tokens.push(Located { token: Token::LeftBracket, line: tok_line, col: tok_col });
                pos += 1; col += 1;
            }
            ']' => {
                tokens.push(Located { token: Token::RightBracket, line: tok_line, col: tok_col });
                pos += 1; col += 1;
            }
            '{' => {
                tokens.push(Located { token: Token::LeftBrace, line: tok_line, col: tok_col });
                pos += 1; col += 1;
            }
            '}' => {
                tokens.push(Located { token: Token::RightBrace, line: tok_line, col: tok_col });
                pos += 1; col += 1;
            }
            ',' => {
                tokens.push(Located { token: Token::Comma, line: tok_line, col: tok_col });
                pos += 1; col += 1;
            }
            ';' => {
                tokens.push(Located { token: Token::Semicolon, line: tok_line, col: tok_col });
                pos += 1; col += 1;
            }
            '`' => {
                tokens.push(Located { token: Token::Backtick, line: tok_line, col: tok_col });
                pos += 1; col += 1;
            }
            '\\' => {
                tokens.push(Located { token: Token::Backslash, line: tok_line, col: tok_col });
                pos += 1; col += 1;
            }
            '@' => {
                tokens.push(Located { token: Token::At, line: tok_line, col: tok_col });
                pos += 1; col += 1;
            }
            '\'' if pos + 1 < chars.len() && chars[pos + 1].is_uppercase() => {
                // DataKinds promoted constructor prefix: 'Empty, 'NonEmpty
                tokens.push(Located { token: Token::Tick, line: tok_line, col: tok_col });
                pos += 1; col += 1;
            }
            _ if is_operator_char(ch) => {
                let start = pos;
                while pos < chars.len() && is_operator_char(chars[pos]) {
                    pos += 1;
                    col += 1;
                }
                let op: String = chars[start..pos].iter().collect();
                let token = match op.as_str() {
                    "->" => Token::Arrow,
                    "=>" => Token::FatArrow,
                    "::" => Token::DblColon,
                    "=" => Token::Eq,
                    "|" => Token::Pipe,
                    "<-" => Token::Bind,
                    _ => Token::Operator(op),
                };
                tokens.push(Located {
                    token,
                    line: tok_line,
                    col: tok_col,
                });
            }
            _ => {
                return Err(err_at(
                    format!("Unexpected character '{}'", ch),
                    line, col,
                ));
            }
        }
    }

    tokens.push(Located {
        token: Token::EOF,
        line,
        col,
    });

    Ok(tokens)
}

/// Does a `--` line comment start at `pos`? Two dashes begin a comment
/// unless they are the head of an operator symbol (`-->`, `--|`, `--.`);
/// a run of three or more dashes is still a comment. One rule for both
/// the line-start scan and the mid-line scan — they once differed, and a
/// continuation line beginning with `-->` was dropped as a comment.
fn is_line_comment_start(chars: &[char], pos: usize) -> bool {
    pos + 1 < chars.len()
        && chars[pos] == '-'
        && chars[pos + 1] == '-'
        && (pos + 2 >= chars.len() || !is_operator_char(chars[pos + 2]) || chars[pos + 2] == '-')
}

fn is_operator_char(c: char) -> bool {
    matches!(c, '!' | '#' | '$' | '%' | '&' | '*' | '+' | '.' | '/' |
                '<' | '=' | '>' | '?' | '@' | '^' | '|' | '-' | '~' | ':')
}

/// GHC's named ASCII control escapes, indexed so that entry `i` decodes to
/// byte `i`. This is the input-side mirror of `__mll_ctrl_names` in
/// codegen/runtime.lua (the `show`/`showLitString` side): the two tables are
/// kept identical byte-for-byte so a string round-trips (`read . show == id`)
/// through both halves. `\SP` (32) and `\DEL` (127) are handled separately
/// because they fall outside this 0..=31 run.
const CTRL_ESCAPE_NAMES: [&str; 32] = [
    "NUL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "BEL", "BS", "HT", "LF",
    "VT", "FF", "CR", "SO", "SI", "DLE", "DC1", "DC2", "DC3", "DC4", "NAK",
    "SYN", "ETB", "CAN", "EM", "SUB", "ESC", "FS", "GS", "RS", "US",
];

/// Decode a single string escape. On entry `*pos` indexes the backslash; on
/// return it indexes the first character AFTER the whole escape. Zero, one, or
/// more bytes are appended to `out` (`\&` and a string gap append nothing;
/// every other escape appends one byte). Follows GHC's lexical syntax
/// (Haskell 2010 Report §2.6) with maximal munch on numeric escapes and on the
/// named-control table, deviating only where mata-ll's byte-string model forces
/// it (see the range check below).
fn lex_string_escape(
    chars: &[char],
    pos: &mut usize,
    col: &mut usize,
    line: &mut usize,
    out: &mut Vec<u8>,
) -> Result<(), Box<Diagnostic>> {
    // Position of the backslash, for error messages.
    let (esc_line, esc_col) = (*line, *col);
    *pos += 1; // consume backslash
    *col += 1;
    if *pos >= chars.len() {
        return Err(err_at("Unterminated escape sequence", esc_line, esc_col));
    }
    let c = chars[*pos];

    // Single-character shorthand escapes (GHC's `charesc`), plus `\&`.
    let simple: Option<i32> = match c {
        'a' => Some(7),
        'b' => Some(8),
        'f' => Some(12),
        'n' => Some(10),
        'r' => Some(13),
        't' => Some(9),
        'v' => Some(11),
        '\\' => Some(92),
        '"' => Some(34),
        '\'' => Some(39),
        '&' => Some(-1), // \& : the empty escape, contributes no byte
        _ => None,
    };
    if let Some(v) = simple {
        *pos += 1;
        *col += 1;
        if v >= 0 {
            out.push(v as u8);
        }
        return Ok(());
    }

    // String gap: backslash, run of whitespace (newlines allowed), backslash.
    // The whole run — including the closing backslash — produces nothing.
    if c.is_whitespace() {
        while *pos < chars.len() && chars[*pos].is_whitespace() {
            if chars[*pos] == '\n' {
                *line += 1;
                *col = 1;
            } else {
                *col += 1;
            }
            *pos += 1;
        }
        if *pos >= chars.len() || chars[*pos] != '\\' {
            return Err(err_at(
                "Malformed string gap: a `\\<whitespace>` gap must be closed \
                 by a second `\\`",
                esc_line, esc_col,
            ));
        }
        *pos += 1; // closing backslash
        *col += 1;
        return Ok(());
    }

    // `\^X` control escapes: \^@ = 0, \^A..\^Z = 1..26, \^[ \^\ \^] \^^ \^_ = 27..31.
    if c == '^' {
        *pos += 1;
        *col += 1;
        if *pos >= chars.len() {
            return Err(err_at(
                "Unterminated `\\^` control escape",
                esc_line, esc_col,
            ));
        }
        let cc = chars[*pos];
        let code = match cc {
            '@'..='_' => (cc as u32) - ('@' as u32), // '@'=64 -> 0 ... '_'=95 -> 31
            _ => {
                return Err(err_at(
                    format!(
                        "Invalid `\\^` control escape `\\^{}`: expected a \
                         character in the range `@`..`_`",
                        cc
                    ),
                    esc_line, esc_col,
                ));
            }
        };
        *pos += 1;
        *col += 1;
        out.push(code as u8);
        return Ok(());
    }

    // Numeric escapes with MAXIMAL MUNCH.
    //   \<decimal+>   e.g. \181, \5   (\05 is one byte 5, NOT \0 then '5')
    //   \o<octal+>    e.g. \o37
    //   \x<hex+>      e.g. \xff
    // GHC uses `o`/`x` (lowercase only) as radix markers; `\O`/`\X` are not
    // octal/hex escapes.
    let radix: u32 = match c {
        'o' => 8,
        'x' => 16,
        d if d.is_ascii_digit() => 10,
        _ => 0,
    };
    if radix != 0 {
        // For `\o`/`\x`, step past the marker; the digits start after it.
        if radix != 10 {
            *pos += 1;
            *col += 1;
        }
        let digit_start = *pos;
        while *pos < chars.len() && chars[*pos].is_digit(radix) {
            *pos += 1;
            *col += 1;
        }
        if *pos == digit_start {
            return Err(err_at(
                format!(
                    "Malformed numeric escape: `\\{}` must be followed by at \
                     least one {} digit",
                    c,
                    match radix { 8 => "octal", 16 => "hexadecimal", _ => "decimal" },
                ),
                esc_line, esc_col,
            ));
        }
        let digits: String = chars[digit_start..*pos].iter().collect();
        // The escape as written, for error messages (`\o37`, `\181`).
        let shown = if radix == 10 { digits.clone() } else { format!("{}{}", c, digits) };
        let value = u32::from_str_radix(&digits, radix).map_err(|_| {
            err_at(format!("Numeric escape `\\{}` overflows", shown), esc_line, esc_col)
        })?;
        // mata-ll strings are byte arrays (HASKDIFF.md "Strings and
        // ByteStrings"): a character is a single byte 0..=255 (this is exactly
        // what `strByte`/`strChar` and the byte-wise `show` operate on). GHC's
        // upper bound is 0x10FFFF (a Unicode code point), but a value above 255
        // cannot be represented as one byte here, so it is a LOUD lexer error
        // rather than a silent wrong value. This is the one place mata-ll's
        // byte-string model forces a deviation from GHC's char-string model.
        if value > 255 {
            let mut diag = err_at(
                format!(
                    "Numeric escape `\\{}` is out of range for a mata-ll String.",
                    shown
                ),
                esc_line, esc_col,
            );
            diag.notes.push(
                "a mata-ll String is a byte array (the Lua string), not a \
                 sequence of Unicode Char; a character is a single byte \
                 0..=255. GHC accepts up to \\1114111 because its String is \
                 [Char], but that value has no single-byte representation \
                 here. Encode the code point as its UTF-8 bytes if you need \
                 it (see HASKDIFF.md, \"Strings and ByteStrings\")."
                    .to_string(),
            );
            return Err(diag);
        }
        out.push(value as u8);
        return Ok(());
    }

    // Named ASCII control escapes, MAXIMAL MUNCH over the table. `\SOH` must
    // win over `\SO` + `H`; `\&` is how the writer disambiguates (`show` emits
    // `"\SO\&H"`), so we take the longest matching name.
    let rest: String = chars[*pos..].iter().collect();
    let mut best: Option<(usize, u8)> = None; // (name length, byte)
    for (i, name) in CTRL_ESCAPE_NAMES.iter().enumerate() {
        if rest.starts_with(name)
            && best.map(|(len, _)| name.len() > len).unwrap_or(true)
        {
            best = Some((name.len(), i as u8));
        }
    }
    // `\SP` (space, 32) and `\DEL` (127) are named too but sit outside the
    // 0..=31 control run.
    if rest.starts_with("SP") && best.map(|(len, _)| 2 > len).unwrap_or(true) {
        best = Some((2, 32));
    }
    if rest.starts_with("DEL") && best.map(|(len, _)| 3 > len).unwrap_or(true) {
        best = Some((3, 127));
    }
    if let Some((len, byte)) = best {
        *pos += len;
        *col += len;
        out.push(byte);
        return Ok(());
    }

    Err(err_at(
        format!("Unknown escape sequence `\\{}`", c),
        esc_line, esc_col,
    ))
}
