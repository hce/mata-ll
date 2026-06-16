use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    IntLit(i64),
    NumLit(f64),
    StrLit(String),

    // Identifiers and operators
    Ident(String),      // lowercase start: variable, function
    UpperIdent(String),  // uppercase start: type, constructor
    Operator(String),    // +, -, *, etc.

    // Keywords
    KwModule,
    Import,
    Qualified,
    As,
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
    Dot,         // .
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

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::IntLit(n) => write!(f, "{}", n),
            Token::NumLit(n) => write!(f, "{}", n),
            Token::StrLit(s) => write!(f, "\"{}\"", s),
            Token::Ident(s) => write!(f, "{}", s),
            Token::UpperIdent(s) => write!(f, "{}", s),
            Token::Operator(s) => write!(f, "{}", s),
            Token::Arrow => write!(f, "->"),
            Token::FatArrow => write!(f, "=>"),
            Token::DblColon => write!(f, "::"),
            Token::Eq => write!(f, "="),
            Token::Pipe => write!(f, "|"),
            Token::Bind => write!(f, "<-"),
            _ => write!(f, "{:?}", self),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Located {
    pub token: Token,
    pub line: usize,
    pub col: usize,
}

pub fn lex(source: &str) -> Result<Vec<Located>, String> {
    let mut tokens = Vec::new();
    let mut chars: Vec<char> = source.chars().collect();
    let mut pos = 0;
    let mut line = 1;
    let mut col = 1;
    let mut at_line_start = true;

    while pos < chars.len() {
        // Track indentation at start of line
        if at_line_start {
            let indent_start = pos;
            let mut indent = 0;
            while pos < chars.len() && chars[pos] == ' ' {
                indent += 1;
                pos += 1;
                col += 1;
            }
            // Skip blank lines
            if pos < chars.len() && chars[pos] == '\n' {
                pos += 1;
                line += 1;
                col = 1;
                continue;
            }
            // Skip comment-only lines
            if pos + 1 < chars.len() && chars[pos] == '-' && chars[pos + 1] == '-' {
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
        if ch == '-' && pos + 1 < chars.len() && chars[pos + 1] == '-' {
            // Make sure it's not an operator like ---
            if pos + 2 >= chars.len() || !is_operator_char(chars[pos + 2]) || chars[pos + 2] == '-' {
                while pos < chars.len() && chars[pos] != '\n' {
                    pos += 1;
                }
                continue;
            }
        }

        // Block comment {- ... -}
        if ch == '{' && pos + 1 < chars.len() && chars[pos + 1] == '-' {
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
            continue;
        }

        let tok_line = line;
        let tok_col = col;

        // String literal
        if ch == '"' {
            pos += 1;
            col += 1;
            let mut s = String::new();
            while pos < chars.len() && chars[pos] != '"' {
                if chars[pos] == '\\' && pos + 1 < chars.len() {
                    pos += 1;
                    col += 1;
                    match chars[pos] {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'r' => s.push('\r'),
                        '\\' => s.push('\\'),
                        '"' => s.push('"'),
                        '0' => s.push('\0'),
                        other => {
                            return Err(format!(
                                "Unknown escape sequence '\\{}' at {}:{}",
                                other, line, col
                            ));
                        }
                    }
                } else {
                    if chars[pos] == '\n' {
                        return Err(format!(
                            "Unterminated string literal at {}:{}",
                            tok_line, tok_col
                        ));
                    }
                    s.push(chars[pos]);
                }
                pos += 1;
                col += 1;
            }
            if pos >= chars.len() {
                return Err(format!(
                    "Unterminated string literal at {}:{}",
                    tok_line, tok_col
                ));
            }
            pos += 1; // closing quote
            col += 1;
            tokens.push(Located {
                token: Token::StrLit(s),
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
            if pos < chars.len() && chars[pos] == '.' && pos + 1 < chars.len() && chars[pos + 1].is_ascii_digit() {
                pos += 1; // skip dot
                col += 1;
                while pos < chars.len() && chars[pos].is_ascii_digit() {
                    pos += 1;
                    col += 1;
                }
                let s: String = chars[start..pos].iter().collect();
                let n: f64 = s.parse().map_err(|e| format!("Invalid number '{}': {}", s, e))?;
                tokens.push(Located {
                    token: Token::NumLit(n),
                    line: tok_line,
                    col: tok_col,
                });
            } else {
                let s: String = chars[start..pos].iter().collect();
                let n: i64 = s.parse().map_err(|e| format!("Invalid integer '{}': {}", s, e))?;
                tokens.push(Located {
                    token: Token::IntLit(n),
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
                // "as" not reserved — usable as variable name (import parser checks Ident("as"))

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
                    "." => Token::Operator(".".to_string()),
                    _ => Token::Operator(op),
                };
                tokens.push(Located {
                    token,
                    line: tok_line,
                    col: tok_col,
                });
            }
            _ => {
                return Err(format!(
                    "Unexpected character '{}' at {}:{}",
                    ch, line, col
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

fn is_operator_char(c: char) -> bool {
    matches!(c, '!' | '#' | '$' | '%' | '&' | '*' | '+' | '.' | '/' |
                '<' | '=' | '>' | '?' | '@' | '^' | '|' | '-' | '~' | ':')
}
