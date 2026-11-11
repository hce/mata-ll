use std::fmt;

use crate::ast::Span;
use crate::types::Diagnostic;

/// Convert MSB-first radix digits to their decimal-digit string, exactly
/// (schoolbook bignum over base-10 cells): `BigIntLit` carries DECIMAL
/// digits — that is what the runtime's `__int_from_decimal` parses — so a
/// hex/octal/binary literal past `maxBound :: Int` must be re-based here.
fn radix_to_decimal(digits: &[u32], base: u32) -> String {
    let mut dec: Vec<u32> = vec![0]; // least-significant decimal digit first
    for &d in digits {
        let mut carry = d;
        for cell in dec.iter_mut() {
            let v = *cell * base + carry;
            *cell = v % 10;
            carry = v / 10;
        }
        while carry > 0 {
            dec.push(carry % 10);
            carry /= 10;
        }
    }
    dec.iter()
        .rev()
        .map(|d| char::from_digit(*d, 10).unwrap())
        .collect()
}

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

    // Module-qualified names. Haskell 2010 §2.4 makes a qualified name ONE
    // lexeme — `qvarid → [modid .] varid`, `qconid → [modid .] conid`,
    // `qvarsym → [modid .] varsym`, `modid → {conid .} conid` — so `M.x`,
    // `Data.Map.Map` and `O.<+>` are single tokens, spelled as written. The
    // dot between the module and the name is the qualifier only when
    // nothing separates it from either side; `Just . f`, `Just .f` and
    // `Just. f` keep the dot as the composition operator. Which module a
    // qualifier names is import resolution's business (`modules.rs`): the
    // lexer just hands over the lexeme, as GHC's does. The parser accepts
    // these wherever a name is REFERRED to and rejects them where a name is
    // BOUND, which is GHC's "Qualified name in binding position".
    /// `M.x`, `Data.Map.insert` — a qualified variable.
    QualIdent(String),
    /// `M.Just`, `Data.Map.Map` — a qualified constructor or type; also the
    /// spelling of every dotted module name (`module Data.Map where`,
    /// `import Data.Map`), which the parser splits into its segments.
    QualUpperIdent(String),
    /// `M.+`, `M..` (the qualified composition operator), `M.:+` — a
    /// qualified operator. The operator part is maximal-munched like a bare
    /// one, so `Monday..` is this token — GHC reads `[Monday..]` the same
    /// way, which is why the range needs a space.
    QualOperator(String),

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
            Token::QualIdent(s) | Token::QualUpperIdent(s) | Token::QualOperator(s) => {
                return write!(f, "'{}'", s)
            }
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
    /// One past the token's LAST source column (the column the lexer's
    /// cursor sits on after consuming it). Carried from the source so
    /// adjacency checks (field-access/qualified dots) are exact — a
    /// re-derived length is wrong wherever the token's rendering differs
    /// from its source spelling (string escapes, radix prefixes, numeric
    /// underscores, big literals).
    pub end_col: usize,
}

/// A `{-# … #-}` pragma the lexer consumed: its inner text (between the
/// `#` markers, trimmed) and the source position of its opening `{-`.
/// mata-ll honors NO pragmas — GHC features it supports (GADTs, type
/// families, linear types, …) are always on and need no LANGUAGE pragma,
/// and everything else is unsupported — so callers turn these into
/// "ignored pragma" warnings (see lib.rs / modules.rs). Until 2026-08 a
/// pragma was silently swallowed as an ordinary `{- -}` block comment,
/// which is also exactly how it still LEXES; this only makes the swallow
/// loud.
pub struct Pragma {
    pub text: String,
    pub line: usize,
    pub col: usize,
}

pub fn lex(source: &str) -> Result<Vec<Located>, Box<Diagnostic>> {
    lex_with_pragmas(source).map(|(tokens, _)| tokens)
}

/// `lex`, additionally returning the pragmas encountered (see [`Pragma`]).
pub fn lex_with_pragmas(
    source: &str,
) -> Result<(Vec<Located>, Vec<Pragma>), Box<Diagnostic>> {
    let mut tokens = Vec::new();
    let mut pragmas: Vec<Pragma> = Vec::new();
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
                    end_col: col,
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

        // Block comment {- ... -} — and the pragma form {-# ... #-}, which
        // lexes identically (the `-}` scan below closes it) but is recorded
        // so the front-end can warn instead of silently compiling as if the
        // pragma were absent (see `Pragma`).
        if ch == '{' && pos + 1 < chars.len() && chars[pos + 1] == '-' {
            let (open_line, open_col) = (line, col);
            pos += 2;
            col += 2;
            let is_pragma = pos < chars.len() && chars[pos] == '#';
            let content_start = pos;
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
            if is_pragma {
                // Content between `{-` and the closing `-}`, shorn of the
                // `#` markers: `# LANGUAGE GADTs #` → `LANGUAGE GADTs`.
                let text: String = chars[content_start..pos.saturating_sub(2)]
                    .iter()
                    .collect::<String>()
                    .trim_matches(|c: char| c == '#' || c.is_whitespace())
                    .to_string();
                pragmas.push(Pragma { text, line: open_line, col: open_col });
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
                end_col: col,
            });
            continue;
        }

        // Number literal
        if ch.is_ascii_digit() {
            // Consume a run of digits of `base`, with NumericUnderscores
            // separators (GHC2021): an underscore run is part of the
            // literal exactly when a digit of the base follows it, so
            // `1_000_000` and `0xFF_FF` lex whole while `1_` is the
            // literal `1` followed by the identifier `_` (maximal munch).
            let digit_run = |pos: &mut usize, col: &mut usize, base: u32| {
                loop {
                    if *pos < chars.len() && chars[*pos].to_digit(base).is_some() {
                        *pos += 1;
                        *col += 1;
                        continue;
                    }
                    if *pos < chars.len() && chars[*pos] == '_' {
                        let mut j = *pos;
                        while j < chars.len() && chars[j] == '_' {
                            j += 1;
                        }
                        if j < chars.len() && chars[j].to_digit(base).is_some() {
                            *col += j - *pos;
                            *pos = j;
                            continue;
                        }
                    }
                    break;
                }
            };

            // Radix prefix: 0x/0X hexadecimal, 0o/0O octal (Haskell 2010
            // §2.5), 0b/0B binary (BinaryLiterals, in GHC2021). The prefix
            // counts only when at least one digit of the base follows —
            // otherwise maximal munch makes `0x` the literal 0 and an
            // identifier `x`, exactly as before. These used to be missed
            // entirely, so `0xFF` silently lexed as the APPLICATION
            // `0 xFF` and surfaced as a baffling type error.
            if ch == '0' && pos + 1 < chars.len() {
                let base = match chars[pos + 1] {
                    'x' | 'X' => Some(16u32),
                    'o' | 'O' => Some(8),
                    'b' | 'B' => Some(2),
                    _ => None,
                };
                if let Some(base) = base
                    && pos + 2 < chars.len()
                    && chars[pos + 2].to_digit(base).is_some()
                {
                    let marker = chars[pos + 1];
                    pos += 2;
                    col += 2;
                    let dig_start = pos;
                    digit_run(&mut pos, &mut col, base);
                    // A DECIMAL digit right after the run can only be a
                    // digit of the wrong base (`0o18`, `0b102`): loudly
                    // reject it instead of lexing two adjacent numbers
                    // that surface as an application.
                    if pos < chars.len() && chars[pos].is_ascii_digit() {
                        let base_name = match base { 2 => "binary", 8 => "octal", _ => "hexadecimal" };
                        return Err(err_at(
                            format!(
                                "Invalid digit '{}' in {} literal: '0{}…' digits must be {}",
                                chars[pos], base_name, marker,
                                match base { 2 => "0 or 1", 8 => "0-7", _ => "0-9 or a-f" },
                            ),
                            tok_line, tok_col,
                        ));
                    }
                    let digits: Vec<u32> = chars[dig_start..pos]
                        .iter()
                        .filter_map(|c| c.to_digit(base))
                        .collect();
                    // Accumulate into i64; past maxBound :: Int the literal
                    // is an Integer (BigIntLit carries DECIMAL digits, so
                    // convert the radix digits — exact schoolbook bignum).
                    let mut small: Option<i64> = Some(0);
                    for &d in &digits {
                        small = small.and_then(|v| {
                            v.checked_mul(base as i64)?.checked_add(d as i64)
                        });
                    }
                    let token = match small {
                        Some(n) => Token::IntLit(n),
                        None => Token::BigIntLit(radix_to_decimal(&digits, base)),
                    };
                    tokens.push(Located { token, line: tok_line, col: tok_col, end_col: col });
                    continue;
                }
            }

            let start = pos;
            digit_run(&mut pos, &mut col, 10);
            let mut is_float = false;
            // Fractional part: `.` digit+ . A dot NOT followed by a digit is
            // not part of the number (so `1..3` stays a range and `x.field`
            // an accessor).
            if pos + 1 < chars.len() && chars[pos] == '.' && chars[pos + 1].is_ascii_digit() {
                is_float = true;
                pos += 1; // skip dot
                col += 1;
                digit_run(&mut pos, &mut col, 10);
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
                    digit_run(&mut pos, &mut col, 10);
                }
            }
            // Underscore separators are lexed but carry no value.
            let s: String = chars[start..pos].iter().filter(|c| **c != '_').collect();
            if is_float {
                let n: f64 = s.parse().map_err(|e| {
                    err_at(format!("Invalid number '{}': {}", s, e), tok_line, tok_col)
                })?;
                tokens.push(Located {
                    token: Token::NumLit(n),
                    line: tok_line,
                    col: tok_col,
                    end_col: col,
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
                    end_col: col,
                });
            }
            continue;
        }

        // Identifier or keyword
        if ch.is_alphabetic() || ch == '_' {
            let start = pos;
            while pos < chars.len() && is_ident_char(chars[pos]) {
                pos += 1;
                col += 1;
            }
            let word: String = chars[start..pos].iter().collect();
            let mut token = word_token(&word, tok_line, tok_col)?;

            // A constructor name followed by an unspaced `.` may be the
            // module qualifier of a qualified name (Haskell 2010 §2.4, see
            // `Token::QualIdent`): `M.x`, `M.Just`, `M.+`, and through
            // further conid segments `Data.Map.insert`. Maximal munch
            // decides, exactly as in GHC's lexer: the longest lexeme at `M`
            // wins, so the dot joins the name whenever a name can follow it
            // and stays an operator otherwise (`Just .f` has a space; `M.1`,
            // `(Just.)` have no name after the dot).
            while let Token::UpperIdent(_) | Token::QualUpperIdent(_) = &token {
                let Some(qualified) = lex_qualifier_tail(&chars, pos, &token, line, col)? else {
                    break;
                };
                let (next_token, consumed) = qualified;
                pos += consumed;
                col += consumed;
                token = next_token;
            }
            tokens.push(Located {
                token,
                line: tok_line,
                col: tok_col,
                end_col: col,
            });
            continue;
        }

        // Operators and symbols
        match ch {
            '(' => {
                tokens.push(Located { token: Token::LeftParen, line: tok_line, col: tok_col, end_col: tok_col + 1 });
                pos += 1; col += 1;
            }
            ')' => {
                tokens.push(Located { token: Token::RightParen, line: tok_line, col: tok_col, end_col: tok_col + 1 });
                pos += 1; col += 1;
            }
            '[' => {
                tokens.push(Located { token: Token::LeftBracket, line: tok_line, col: tok_col, end_col: tok_col + 1 });
                pos += 1; col += 1;
            }
            ']' => {
                tokens.push(Located { token: Token::RightBracket, line: tok_line, col: tok_col, end_col: tok_col + 1 });
                pos += 1; col += 1;
            }
            '{' => {
                tokens.push(Located { token: Token::LeftBrace, line: tok_line, col: tok_col, end_col: tok_col + 1 });
                pos += 1; col += 1;
            }
            '}' => {
                tokens.push(Located { token: Token::RightBrace, line: tok_line, col: tok_col, end_col: tok_col + 1 });
                pos += 1; col += 1;
            }
            ',' => {
                tokens.push(Located { token: Token::Comma, line: tok_line, col: tok_col, end_col: tok_col + 1 });
                pos += 1; col += 1;
            }
            ';' => {
                tokens.push(Located { token: Token::Semicolon, line: tok_line, col: tok_col, end_col: tok_col + 1 });
                pos += 1; col += 1;
            }
            '`' => {
                tokens.push(Located { token: Token::Backtick, line: tok_line, col: tok_col, end_col: tok_col + 1 });
                pos += 1; col += 1;
            }
            '\\' => {
                tokens.push(Located { token: Token::Backslash, line: tok_line, col: tok_col, end_col: tok_col + 1 });
                pos += 1; col += 1;
            }
            '@' => {
                tokens.push(Located { token: Token::At, line: tok_line, col: tok_col, end_col: tok_col + 1 });
                pos += 1; col += 1;
            }
            '\'' if pos + 1 < chars.len() && chars[pos + 1].is_uppercase() => {
                // DataKinds promoted constructor prefix: 'Empty, 'NonEmpty
                tokens.push(Located { token: Token::Tick, line: tok_line, col: tok_col, end_col: tok_col + 1 });
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
                    end_col: col,
                });
            }
            '\'' => {
                // A `'` here is neither a promoted-constructor tick (handled
                // above) nor part of an identifier (`f'` consumes its primes
                // in the ident path): the author is almost certainly writing
                // a character literal, which mata-ll does not have.
                let mut diag = err_at(
                    "Character literal: mata-ll has no Char type",
                    line, col,
                );
                diag.notes.push(
                    "String is the opaque Lua string (a byte array), not \
                     [Char], so there is no character type to give a literal \
                     like 'a'. Use a one-character string (\"a\"), or the \
                     LString byte functions (strByte s i / strChar b) to work \
                     with individual bytes. See HASKDIFF.md, \"Strings and \
                     ByteStrings\"."
                        .to_string(),
                );
                return Err(diag);
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
        end_col: col,
    });

    // A line whose only content is a BLOCK comment contributes an Indent
    // but no tokens (the Indent is pushed before the `{-` is seen, and the
    // comment then swallows the rest of the line). Comments are whitespace
    // to layout — GHC's algorithm never sees them — but a stray Indent
    // pair broke the operator-continuation check ("Unexpected token '+' at
    // top level"). Drop every Indent that introduces no token on its line:
    // its successor is another Indent or the end of the stream.
    let mut cleaned: Vec<Located> = Vec::with_capacity(tokens.len());
    for (i, t) in tokens.iter().enumerate() {
        if matches!(t.token, Token::Indent(_))
            && matches!(
                tokens.get(i + 1).map(|n| &n.token),
                None | Some(Token::Indent(_)) | Some(Token::EOF)
            )
        {
            continue;
        }
        cleaned.push(t.clone());
    }

    Ok((cleaned, pragmas))
}

/// Does a `--` line comment start at `pos`? Two dashes begin a comment
/// unless they are the head of an operator symbol (`-->`, `--|`, `--.`);
/// a run of three or more dashes is still a comment. One rule for both
/// the line-start scan and the mid-line scan — they once differed, and a
/// continuation line beginning with `-->` was dropped as a comment.
fn is_line_comment_start(chars: &[char], pos: usize) -> bool {
    if !(pos + 1 < chars.len() && chars[pos] == '-' && chars[pos + 1] == '-') {
        return false;
    }
    // Haskell 2010 §2.3: a run of two or more dashes opens a comment only
    // when the character after the WHOLE run is not a symbol character —
    // `-->` and `--->` are operator tokens, while `--`, `---`, `--- text`
    // are comments. The old check looked at one character past the first
    // two dashes and treated any further dash as "still a comment", so
    // `--->` swallowed the rest of the line.
    let mut j = pos;
    while j < chars.len() && chars[j] == '-' {
        j += 1;
    }
    j >= chars.len() || !is_operator_char(chars[j])
}

/// ASCII only. Generated names rely on it: `ast::MINT` (`·`) must be a
/// character no identifier and no operator can contain.
fn is_operator_char(c: char) -> bool {
    matches!(c, '!' | '#' | '$' | '%' | '&' | '*' | '+' | '.' | '/' |
                '<' | '=' | '>' | '?' | '@' | '^' | '|' | '-' | '~' | ':')
}

/// A character that continues an identifier (Haskell 2010 §2.4: letters,
/// digits, `_` and `'`).
fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\''
}

/// Haskell 2010 §2.4 `reservedop`: the symbol sequences that are never an
/// operator name, so a module qualifier cannot precede them (`M.->` is the
/// constructor `M` followed by the operator `.->`, not a qualified arrow).
/// `-` alone is not reserved — `M.-` is the qualified minus, as in GHC.
fn is_reserved_op(op: &str) -> bool {
    matches!(op, ".." | ":" | "::" | "=" | "\\" | "|" | "<-" | "->" | "@" | "~" | "=>")
}

/// The token a word (an identifier-shaped lexeme) stands for: a keyword, a
/// constructor name, a variable name, or the `_` wildcard. One classifier
/// for the bare word and for the name part of a qualified name, so
/// `M.where` cannot become a qualified variable (a keyword is no `varid`).
fn word_token(word: &str, line: usize, col: usize) -> Result<Token, Box<Diagnostic>> {
    Ok(match word {
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
            // The double-underscore prefix is the compiler's fresh-name
            // namespace (__lamN, __tup_N, __compN, __sec, …): desugaring
            // substitutes those names without renaming, so a user binder
            // spelled the same way would be silently captured
            // (`f __sec = (__sec +)` once desugared to
            // `\__sec -> __sec + __sec`). GHC never reserves source
            // names because its desugarer renames — mata-ll reserves
            // the prefix instead and says so loudly.
            if word.starts_with("__") {
                let mut diag = err_at(
                    format!(
                        "Identifiers starting with '__' are reserved \
                         for compiler-generated names: '{}'",
                        word
                    ),
                    line, col,
                );
                diag.notes.push(
                    "desugaring creates '__'-prefixed helper names \
                     (lambda-pattern binders, tuple-binding scrutinees, \
                     section operands) and substitutes them without \
                     renaming, so a user name spelled the same way \
                     would be silently captured; use a single leading \
                     underscore or another name"
                        .to_string(),
                );
                return Err(diag);
            }
            if word.starts_with(|c: char| c.is_uppercase()) {
                Token::UpperIdent(word.to_string())
            } else {
                Token::Ident(word.to_string())
            }
        }
    })
}

/// One step of the qualified-name rule at `pos` (source position
/// `line`:`col`), just after the module qualifier `head` (an `UpperIdent`,
/// or the `QualUpperIdent` of the conid segments read so far). When `chars[pos]` is a `.` immediately followed
/// by something a name can start with, the result is the longer lexeme and
/// the number of characters it adds: a conid extends the qualifier
/// (`Data.Map`), a varid closes a qualified variable (`M.x`), a symbol run
/// closes a qualified operator (`M.+`). `None` leaves the dot alone, so
/// the operator lexer reads it next — after a space, before a digit or a
/// bracket, before a keyword (`M.where`: a keyword is no `varid`), before a
/// reserved operator (`M.->`), and before a comment-opening dash run
/// (`M.--`), all exactly the lexemes GHC produces there.
fn lex_qualifier_tail(
    chars: &[char],
    pos: usize,
    head: &Token,
    line: usize,
    col: usize,
) -> Result<Option<(Token, usize)>, Box<Diagnostic>> {
    if !(pos + 1 < chars.len() && chars[pos] == '.') {
        return Ok(None);
    }
    let modid = match head {
        Token::UpperIdent(m) | Token::QualUpperIdent(m) => m.as_str(),
        _ => return Ok(None),
    };
    let next = chars[pos + 1];
    if next.is_alphabetic() || next == '_' {
        let mut j = pos + 1;
        while j < chars.len() && is_ident_char(chars[j]) {
            j += 1;
        }
        let word: String = chars[pos + 1..j].iter().collect();
        // The name part is classified like a bare word; the reserved `__`
        // prefix is reported at the name, where the bare word would be.
        let token = match word_token(&word, line, col + 1)? {
            Token::UpperIdent(seg) => Token::QualUpperIdent(format!("{modid}.{seg}")),
            Token::Ident(seg) => Token::QualIdent(format!("{modid}.{seg}")),
            _ => return Ok(None),
        };
        return Ok(Some((token, j - pos)));
    }
    if is_operator_char(next) {
        let mut j = pos + 1;
        while j < chars.len() && is_operator_char(chars[j]) {
            j += 1;
        }
        let op: String = chars[pos + 1..j].iter().collect();
        if is_reserved_op(&op) || (op.len() >= 2 && op.chars().all(|c| c == '-')) {
            return Ok(None);
        }
        return Ok(Some((Token::QualOperator(format!("{modid}.{op}")), j - pos)));
    }
    Ok(None)
}

/// The name part of a (possibly) qualified spelling: `x` of `M.x`, `Map`
/// of `Data.Map.Map`, `.` of `M..`, `+` of `M.+`; a bare name unchanged.
/// The module qualifier is the run of conid segments each followed by a
/// dot; the name is what follows the last of them — so a spelling whose
/// segments are all conids (`M.Just`) names its final segment.
pub fn unqualified(name: &str) -> &str {
    let mut rest = name;
    loop {
        let Some(head) = rest.chars().next() else { return rest };
        if !head.is_uppercase() {
            return rest;
        }
        let seg_len = rest.chars().take_while(|c| is_ident_char(*c)).map(char::len_utf8).sum::<usize>();
        let after = &rest[seg_len..];
        match after.strip_prefix('.') {
            Some(tail) if !tail.is_empty() => rest = tail,
            _ => return rest,
        }
    }
}

/// The module qualifier of a qualified spelling (`M` of `M.x`, `Data.Map`
/// of `Data.Map.insert`), `None` for a bare name. The complement of
/// [`unqualified`].
pub fn qualifier(name: &str) -> Option<&str> {
    let name_part = unqualified(name);
    if name_part.len() == name.len() {
        return None;
    }
    Some(&name[..name.len() - name_part.len() - 1])
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `end_col` is the SOURCE end, not a re-derived rendering length —
    /// the spellings that differ from their rendering used to mislead the
    /// parser's dot-adjacency checks (Q87): string escapes (decoded
    /// shorter than source), radix prefixes, numeric underscores, and big
    /// literals (whose old estimate fell to 1).
    #[test]
    fn end_col_is_source_exact() {
        let src = "x = \"a\\nb\" 0xFF 1_000 99999999999999999999\n";
        let toks = lex(src).expect("lexes");
        let spans: Vec<(usize, usize)> = toks.iter()
            .filter(|t| !matches!(t.token, Token::Indent(_) | Token::EOF))
            .map(|t| (t.col, t.end_col))
            .collect();
        // x=1..2  ==3..4  "a\nb"=5..11 (6 source chars, decoded 3)
        // 0xFF=12..16 (4 source chars, rendered "255" is 3)
        // 1_000=17..22 (5 source chars, rendered "1000" is 4)
        // bigint=23..43 (20 digits; the old estimate fell to 1)
        assert_eq!(
            spans,
            vec![(1, 2), (3, 4), (5, 11), (12, 16), (17, 22), (23, 43)],
            "tokens: {:?}",
            toks.iter().map(|t| format!("{:?}@{}..{}", t.token, t.col, t.end_col)).collect::<Vec<_>>()
        );
    }

    /// The tokens of one line, without the layout mark and the end marker.
    fn tokens_of(src: &str) -> Vec<Token> {
        lex(src)
            .expect("lexes")
            .into_iter()
            .map(|t| t.token)
            .filter(|t| !matches!(t, Token::Indent(_) | Token::EOF))
            .collect()
    }

    fn op(s: &str) -> Token { Token::Operator(s.to_string()) }
    fn up(s: &str) -> Token { Token::UpperIdent(s.to_string()) }
    fn id(s: &str) -> Token { Token::Ident(s.to_string()) }
    fn qid(s: &str) -> Token { Token::QualIdent(s.to_string()) }
    fn qup(s: &str) -> Token { Token::QualUpperIdent(s.to_string()) }
    fn qop(s: &str) -> Token { Token::QualOperator(s.to_string()) }

    /// Haskell 2010 §2.4: a conid, a dot and a name with nothing between
    /// them are ONE lexeme — for variables, constructors and operators,
    /// through any number of conid segments. `Monday..` is the qualified
    /// `.` (the reason `[Monday..]` is not a range, in GHC and here).
    #[test]
    fn qualified_names_are_single_lexemes() {
        assert_eq!(
            tokens_of("M.x Data.Map.insert M.Just Data.Map.Map M.+ M.. M.:+ Monday.. M.-"),
            vec![
                qid("M.x"), qid("Data.Map.insert"), qup("M.Just"), qup("Data.Map.Map"),
                qop("M.+"), qop("M.."), qop("M.:+"), qop("Monday.."), qop("M.-"),
            ]
        );
        // Dotted module names in headers and imports are the same lexeme.
        assert_eq!(tokens_of("import Data.Map"), vec![Token::Import, qup("Data.Map")]);
        // `True`/`False` are conids too.
        assert_eq!(tokens_of("True.x"), vec![qid("True.x")]);
    }

    /// The dot is the qualifier only when glued to both sides: a space on
    /// either side, a digit, a bracket, a keyword, a reserved operator or
    /// a comment-opening dash run after it leave `.` an operator, and a
    /// lowercase head is never a module name (`x.field` stays the parser's
    /// field access, `f.g` composition).
    #[test]
    fn dot_without_a_qualified_name_stays_an_operator() {
        assert_eq!(tokens_of("Just . f"), vec![up("Just"), op("."), id("f")]);
        assert_eq!(tokens_of("Just .f"), vec![up("Just"), op("."), id("f")]);
        assert_eq!(tokens_of("Just. f"), vec![up("Just"), op("."), id("f")]);
        assert_eq!(tokens_of("(Just.)"), vec![Token::LeftParen, up("Just"), op("."), Token::RightParen]);
        assert_eq!(tokens_of("M.1"), vec![up("M"), op("."), Token::IntLit(1)]);
        assert_eq!(tokens_of("M.where"), vec![up("M"), op("."), Token::Where]);
        assert_eq!(tokens_of("M._"), vec![up("M"), op("."), Token::Underscore]);
        assert_eq!(tokens_of("M.->"), vec![up("M"), op(".->")]);
        assert_eq!(tokens_of("M..."), vec![up("M"), op("...")]);
        assert_eq!(tokens_of("M...."), vec![qop("M....")]);
        assert_eq!(tokens_of("M.-- c"), vec![up("M"), op(".--"), id("c")]);
        assert_eq!(tokens_of("x.field"), vec![id("x"), op("."), id("field")]);
        assert_eq!(tokens_of("[1..3]"), vec![Token::LeftBracket, Token::IntLit(1), op(".."), Token::IntLit(3), Token::RightBracket]);
        assert_eq!(tokens_of("1.5"), vec![Token::NumLit(1.5)]);
    }

    /// A qualified lexeme's `end_col` is its source end, like every other
    /// token's: the parser's adjacency checks (`M.r.field`) read it.
    #[test]
    fn qualified_lexeme_spans_are_source_exact() {
        let toks = lex("M.x Data.Map.insert O.<+>\n").expect("lexes");
        let spans: Vec<(usize, usize)> = toks.iter()
            .filter(|t| !matches!(t.token, Token::Indent(_) | Token::EOF))
            .map(|t| (t.col, t.end_col))
            .collect();
        assert_eq!(spans, vec![(1, 4), (5, 20), (21, 26)]);
    }

    /// The reserved `__` prefix is reserved after a qualifier too.
    #[test]
    fn reserved_prefix_is_rejected_in_the_name_part() {
        let err = lex("M.__x").expect_err("reserved");
        assert!(format!("{err}").contains("reserved for compiler-generated names"), "{err}");
    }

    /// The name part and the qualifier of a spelling, for the renamer's
    /// constructor-spelling test and the diagnostics.
    #[test]
    fn unqualified_and_qualifier_split_a_spelling() {
        for (name, module, bare) in [
            ("M.x", Some("M"), "x"),
            ("Data.Map.insert", Some("Data.Map"), "insert"),
            ("Data.Map.Map", Some("Data.Map"), "Map"),
            ("M.Just", Some("M"), "Just"),
            ("M..", Some("M"), "."),
            ("M.<+>", Some("M"), "<+>"),
            ("M.:+", Some("M"), ":+"),
            ("Monday..", Some("Monday"), "."),
            ("plus", None, "plus"),
            ("Just", None, "Just"),
            (":+", None, ":+"),
            ("<+>", None, "<+>"),
        ] {
            assert_eq!(qualifier(name), module, "qualifier of {name}");
            assert_eq!(unqualified(name), bare, "name part of {name}");
        }
    }
}
