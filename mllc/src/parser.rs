use std::collections::HashMap;
use crate::ast::*;
use crate::lexer::{Located, Token};
use crate::types::Diagnostic;

/// Internal parser result. The error is boxed because `Diagnostic` is large
/// (it embeds `Ty`s) and would otherwise bloat every parser `Result` on the
/// stack (clippy::result_large_err). The public `parse` unboxes into a
/// `Vec<Diagnostic>`.
type PResult<T> = Result<T, Box<Diagnostic>>;

/// List comprehension qualifier (internal to parser, desugared before AST)
enum ListCompQual {
    Generator { pattern: Pattern, expr: Expr },
    Guard(Expr),
    /// `let decls` — binds for the body and every later qualifier.
    Let(Vec<LocalDef>),
}

/// The infix operator whose right-hand side is currently being parsed.
/// Carried into the recursive infix parse so a same-precedence neighbor can
/// be checked against it: Haskell only defines a grouping for such a pair
/// when both are infixl (groups left) or both infixr (groups right) —
/// anything else is ambiguous and rejected (the GHC precedence-parsing rule).
struct ParentOp {
    op: String,
    prec: u8,
    assoc: Assoc,
}

struct Parser {
    tokens: Vec<Located>,
    pos: usize,
    /// Current line's indentation
    current_indent: usize,
    /// Minimum indentation for current expression context
    expr_min_indent: usize,
    /// Column of the current layout block's items (0 at top level; the item
    /// column inside where/let/do/case). A cross-line application-argument
    /// continuation is only consumed when indented strictly past this, which
    /// is exactly the Haskell rule: deeper than the block = continuation,
    /// at the block column = a new item.
    block_indent: usize,
    /// User-defined operator fixity: op -> (assoc, precedence)
    fixities: HashMap<String, (Assoc, u8)>,
    /// Current recursion depth of the nesting productions (expressions,
    /// types, patterns). Bounded by `crate::MAX_NESTING_DEPTH` via
    /// `enter_nested`, so absurdly nested input yields a clean diagnostic
    /// instead of overflowing the native stack.
    depth: usize,
}

/// A parser position to rewind to when a speculative parse fails. Captures
/// BOTH the token cursor and the layout state: restoring `pos` alone leaves
/// `current_indent` describing a line the parser is no longer on, and the
/// backtrack sites used to apply that two-field discipline inconsistently
/// by hand. (`block_indent` is scoped by its own save/restore at the block
/// constructs and is never part of speculative backtracking.)
#[derive(Clone, Copy)]
struct Checkpoint {
    pos: usize,
    current_indent: usize,
}

impl Parser {
    fn new(tokens: Vec<Located>) -> Self {
        Parser {
            tokens,
            pos: 0,
            current_indent: 0,
            expr_min_indent: 0,
            block_indent: 0,
            fixities: HashMap::new(),
            depth: 0,
        }
    }

    /// Capture the state a failed speculative parse must restore via
    /// `rewind`. See [`Checkpoint`].
    fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            pos: self.pos,
            current_indent: self.current_indent,
        }
    }

    fn rewind(&mut self, cp: Checkpoint) {
        self.pos = cp.pos;
        self.current_indent = cp.current_indent;
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos].token
    }

    fn peek_loc(&self) -> &Located {
        &self.tokens[self.pos]
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos].token;
        self.pos += 1;
        tok
    }

    /// Decode a string-literal token used in a SYMBOL position (an FFI Lua
    /// name, a type-level Symbol, a JSON/field rename key, a constructor
    /// rename). These are identifiers/text, so a byte sequence that is not
    /// valid UTF-8 (which a `\181`-style escape can now produce) is rejected
    /// rather than passed through as a lossy name. The value string literal
    /// keeps its raw bytes.
    fn strlit_as_symbol(&self, bytes: Vec<u8>, context: &str) -> PResult<String> {
        String::from_utf8(bytes).map_err(|_| {
            self.err_here(format!(
                "String literal used as {} contains non-UTF-8 bytes (e.g. a \
                 numeric escape above \\127); a {} must be text",
                context, context
            ))
        })
    }

    /// A parse diagnostic pointing at the current token. The span renders
    /// inline as ` at line:col`, exactly the parser's historical format.
    fn err_here(&self, msg: String) -> Box<Diagnostic> {
        let loc = self.peek_loc();
        Box::new(Diagnostic::parse_at(msg, Span::new(loc.line, loc.col)))
    }

    /// Depth guard for the recursive-descent productions. Checked BEFORE
    /// descending, so the parser itself can never overflow the native stack:
    /// past the limit it stops and reports a clean diagnostic. Only called
    /// through `guarded`, which pairs the check with the exit decrement.
    fn enter_nested(&mut self, what: &str) -> PResult<()> {
        if self.depth >= crate::MAX_NESTING_DEPTH {
            let mut diag = self.err_here(format!(
                "{} nested too deeply (limit {})",
                what,
                crate::MAX_NESTING_DEPTH
            ));
            diag.notes.push(
                "the compiler reads nested syntax with bounded recursion so it \
                 can report this error instead of crashing on pathological \
                 input; restructure the code to nest less, e.g. by splitting \
                 it into smaller definitions"
                    .to_string(),
            );
            return Err(diag);
        }
        self.depth += 1;
        Ok(())
    }

    /// Run one recursive production a level deeper: the `enter_nested` check
    /// plus its paired decrement in one place, so a production cannot forget
    /// the exit bookkeeping. Recursive calls throughout the parser go
    /// through the wrappers built on this, so the counter tracks the real
    /// recursion depth.
    fn guarded<T>(
        &mut self,
        what: &str,
        f: impl FnOnce(&mut Self) -> PResult<T>,
    ) -> PResult<T> {
        self.enter_nested(what)?;
        let r = f(self);
        self.depth -= 1;
        r
    }

    fn expect(&mut self, expected: &Token) -> PResult<()> {
        if self.peek() == expected {
            self.advance();
            Ok(())
        } else {
            Err(self.err_here(format!(
                "Expected {}, found {}",
                expected, self.peek()
            )))
        }
    }

    fn at(&self, tok: &Token) -> bool {
        self.peek() == tok
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek(), Token::EOF)
    }

    fn skip_indent(&mut self) {
        while let Token::Indent(n) = self.peek() {
            self.current_indent = *n;
            self.advance();
        }
    }

    fn skip_newlines_and_indent(&mut self) {
        loop {
            match self.peek() {
                Token::Indent(n) => {
                    self.current_indent = *n;
                    self.advance();
                }
                Token::Newline => {
                    self.advance();
                }
                _ => break,
            }
        }
    }

    /// Open an implicit layout block whose items start at the NEXT token
    /// (do statements, case alternatives, let bindings): skips newlines
    /// and makes that token's column the block column — as
    /// `current_indent`, so a following line at a smaller indent closes
    /// the block also when the first item shares the keyword's line
    /// (`main = do putStrLn "a"` followed by a column-0 declaration; the
    /// line's own indent would have let the declaration in as a
    /// statement) — and as `block_indent`, so a same-column sibling is the
    /// next item rather than a continuation argument of the previous one.
    /// Returns the column; the caller restores `block_indent` at the end.
    fn open_item_block(&mut self) -> usize {
        self.skip_newlines_and_indent();
        let col = self.peek_loc().col.saturating_sub(1);
        self.current_indent = col;
        self.block_indent = col;
        col
    }

    /// Is the next token one that ends an implicit layout block from
    /// INSIDE its line — the closing bracket of an enclosing paren/list/
    /// record, the separator of an enclosing tuple/list, or the keyword of
    /// an enclosing construct? Haskell's layout algorithm closes the block
    /// on the parse error such a token would cause (the `parse-error(t)`
    /// rule); the do-block and case-alternative loops test for these
    /// explicitly, so `(do a; b, 2)`, `[do …, …]`, `if c then do … else …`
    /// and `let x = do … in …` end the block where GHC does.
    fn at_block_closer(&self) -> bool {
        matches!(
            self.peek(),
            Token::RightParen | Token::RightBracket | Token::RightBrace | Token::Comma
                | Token::Then | Token::Else | Token::Of | Token::In | Token::Where
        )
    }

    /// Open the layout block that follows a `where` (class body, instance
    /// body, clause bindings). Skips to the block's first token and returns
    /// the column (0-based) every item of the block starts at — a later
    /// line at that indent is the next item, a smaller indent closes the
    /// block — or `None` for an EMPTY block.
    ///
    /// Haskell's layout rule: the block's context column is the column of
    /// the token after the keyword; when that token sits on a later line
    /// and is not indented past the ENCLOSING context (`enclosing_indent`),
    /// the block is `{}` and the token belongs to the enclosing context.
    /// Without this an empty `class C a where` / `instance C T where`
    /// swallowed the following top-level declarations as methods (the
    /// item indent was read from the next line, i.e. 0), and a `where`
    /// alone on its line swallowed the next definition as a binding.
    ///
    /// When the first item shares the keyword's line, `current_indent` is
    /// moved to its column: that column is the layout context from here
    /// on (a following line at a smaller indent closes the block, as in
    /// GHC), and the block loops compare against `current_indent`.
    fn open_layout_block(&mut self, keyword_line: usize, enclosing_indent: usize) -> Option<usize> {
        self.skip_newlines_and_indent();
        if self.at_eof() {
            return None;
        }
        let loc = self.peek_loc();
        if loc.line > keyword_line {
            (self.current_indent > enclosing_indent).then_some(self.current_indent)
        } else {
            let col = loc.col.saturating_sub(1);
            self.current_indent = col;
            Some(col)
        }
    }

    /// Parse a whole module: the optional `module … where` header, then
    /// top-level declarations at column 0 (import decls, signatures, bindings,
    /// data/newtype/class/instance/type-family/fixity decls). Consecutive
    /// same-named clauses are merged into one FunDef; a declaration that fails to parse
    /// is recorded and parsing resumes at the next column-0 declaration, so
    /// every declaration's error is reported at once.
    fn parse_module(&mut self) -> Result<Module, Vec<Diagnostic>> {
        let mut decls = Vec::new();
        self.skip_indent();

        // Parse optional `module Name (exports) where` header
        let mut module_exports: Option<Vec<String>> = None;
        if self.at(&Token::KwModule) {
            self.advance();
            // Skip module name (may be dotted: Data.List)
            while matches!(self.peek(), Token::UpperIdent(_) | Token::Ident(_)) {
                self.advance();
                if self.at(&Token::Operator(".".to_string())) {
                    self.advance();
                }
            }
            // Parse optional export list
            self.skip_newlines_and_indent();
            if self.at(&Token::LeftParen) {
                self.advance();
                let mut exports = Vec::new();
                loop {
                    self.skip_newlines_and_indent();
                    if self.at(&Token::RightParen) { break; }
                    match self.peek().clone() {
                        Token::Ident(n) => { exports.push(n); self.advance(); }
                        Token::UpperIdent(n) => {
                            exports.push(n); self.advance();
                            // Skip optional (..) for Type(..)
                            if self.at(&Token::LeftParen) {
                                self.advance();
                                while !self.at(&Token::RightParen) && !self.at_eof() {
                                    self.advance();
                                }
                                if self.at(&Token::RightParen) { self.advance(); }
                            }
                        }
                        Token::Operator(op) => { exports.push(op); self.advance(); }
                        // Parenthesized operator export, GHC style:
                        // `module M ((-.), (~=~)) where`.
                        Token::LeftParen => {
                            self.advance();
                            if let Token::Operator(op) = self.peek().clone() {
                                exports.push(op);
                                self.advance();
                            }
                            if self.at(&Token::RightParen) { self.advance(); }
                            // A type operator can carry its constructors:
                            // `(:+:)(..)` exports the type and everything it
                            // declares, exactly like `T(..)`.
                            if self.at(&Token::LeftParen) {
                                self.advance();
                                while !self.at(&Token::RightParen) && !self.at_eof() {
                                    self.advance();
                                }
                                if self.at(&Token::RightParen) { self.advance(); }
                            }
                        }
                        other => {
                            let mut diag = self.err_here(format!(
                                "This export-list entry is not understood \
                                 (found {}). An entry names a value, a \
                                 type (optionally with '(..)'), or an \
                                 operator in parentheses.",
                                other
                            ));
                            diag.notes.push(
                                "module re-exports ('module M') are not \
                                 supported yet; export the module's names \
                                 individually"
                                    .to_string(),
                            );
                            return Err(vec![*diag]);
                        }
                    }
                    self.skip_newlines_and_indent();
                    if self.at(&Token::Comma) { self.advance(); } else { break; }
                }
                self.skip_newlines_and_indent();
                if self.at(&Token::RightParen) { self.advance(); }
                module_exports = Some(exports);
            }
            if self.at(&Token::Where) {
                self.advance();
            }
            self.skip_newlines_and_indent();
        }

        // Parse declarations, recovering at declaration boundaries: a failed
        // declaration records its diagnostic and parsing resumes at the next
        // unindented line that can start a declaration, so one run reports
        // every independent syntax error instead of only the first.
        let mut errors: Vec<Diagnostic> = Vec::new();
        while !self.at_eof() {
            let started_at = self.pos;
            match self.parse_decl() {
                Ok(decl) => decls.extend(decl),
                Err(e) => {
                    errors.push(*e);
                    self.recover_to_next_decl(started_at);
                }
            }
            self.skip_newlines_and_indent();
        }
        if !errors.is_empty() {
            return Err(errors);
        }

        // Merge consecutive FunDef declarations with the same name (moving
        // the clauses, not cloning them).
        let mut merged: Vec<Decl> = Vec::new();
        for decl in decls {
            match decl {
                Decl::FunDef { name, clauses }
                    if matches!(merged.last(), Some(Decl::FunDef { name: prev, .. }) if *prev == name) =>
                {
                    if let Some(Decl::FunDef { clauses: prev_clauses, .. }) = merged.last_mut() {
                        prev_clauses.extend(clauses);
                    }
                }
                other => merged.push(other),
            }
        }

        Ok(Module { decls: merged, exports: module_exports, hidden: std::collections::HashSet::new() })
    }

    /// True when `tok` can begin a top-level declaration — the resync points
    /// for parser error recovery.
    fn starts_decl(tok: &Token) -> bool {
        matches!(tok,
            Token::Data | Token::Newtype | Token::Import | Token::Class
            | Token::Instance | Token::KwType | Token::Intrinsic | Token::Export
            | Token::Infixl | Token::Infixr | Token::Infix
            | Token::Ident(_) | Token::LeftParen)
    }

    /// After a declaration fails to parse, skip forward to the next plausible
    /// declaration start: an unindented line (`Indent(0)`) whose first token
    /// can begin a declaration. `started_at` is where the failed declaration
    /// began; recovery always moves past it, so a failure on a declaration's
    /// very first token cannot loop.
    fn recover_to_next_decl(&mut self, started_at: usize) {
        if self.pos <= started_at {
            self.pos = started_at + 1;
        }
        while !self.at_eof() {
            if matches!(self.peek(), Token::Indent(0))
                && self.tokens.get(self.pos + 1).is_some_and(|next| Self::starts_decl(&next.token))
            {
                return;
            }
            self.pos += 1;
        }
    }

    fn parse_decl(&mut self) -> PResult<Vec<Decl>> {
        self.skip_newlines_and_indent();

        match self.peek().clone() {
            Token::Data => self.parse_data_decl().map(|d| vec![d]),
            Token::Newtype => self.parse_newtype_decl().map(|d| vec![d]),
            Token::Import => self.parse_import_decl().map(|d| vec![d]),
            Token::Class => self.parse_class_decl(),
            Token::Instance => self.parse_instance_decl(),
            Token::KwType => self.parse_type_family_decl(),
            Token::Intrinsic => self.parse_intrinsic_decl(),
            Token::Export => self.parse_export_decl(),
            Token::Infixl | Token::Infixr | Token::Infix => self.parse_fixity_decl(),
            Token::Ident(_) => self.parse_value_decl(),
            Token::LeftParen => self.parse_operator_decl(),
            _ => {
                Err(self.err_here(format!(
                    "Unexpected token {} at top level",
                    self.peek()
                )))
            }
        }
    }

    fn parse_data_decl(&mut self) -> PResult<Decl> {
        self.expect(&Token::Data)?;
        // The type name is normally an UpperIdent, but a type OPERATOR is
        // declared in prefix parenthesised form — `data (:+:) a b = …` — just
        // like an operator value binding (`(<>) a b = …`). Both spellings then
        // take the same type-variable list.
        let name = self.parse_type_con_name()?;

        let mut type_vars = Vec::new();
        while let Token::Ident(v) = self.peek() {
            type_vars.push(v.clone());
            self.advance();
        }

        // Check for GADT syntax
        if self.at(&Token::Where) {
            self.advance();
            // GADT constructors: each is `Con :: type`, with the full
            // signature kept in `gadt_type` (the typechecker derives the
            // fields, result-type indices and existentials from it).
            let mut constructors = Vec::new();
            self.skip_newlines_and_indent();
            let gadt_indent = self.current_indent;

            while !self.at_eof() && self.current_indent >= gadt_indent {
                if let Token::UpperIdent(_) = self.peek() {
                    let con_name = self.expect_upper_ident()?;
                    self.expect(&Token::DblColon)?;
                    let ty = self.parse_type()?;
                    constructors.push(Constructor {
                        name: con_name,
                        external_name: None,
                        fields: ConstructorFields::Positional(vec![]),
                        gadt_type: Some(ty),
                        existential_vars: vec![],
                        existential_constraints: vec![],
                    });
                    self.skip_newlines_and_indent();
                } else {
                    break;
                }
            }

            let deriving = self.parse_deriving()?;
            return Ok(Decl::DataDef {
                name,
                type_vars,
                constructors,
                deriving,
            });
        }

        self.expect(&Token::Eq)?;

        let mut constructors = Vec::new();
        constructors.push(self.parse_constructor()?);
        loop {
            self.skip_newlines_and_indent();
            if !self.at(&Token::Pipe) { break; }
            self.advance();
            constructors.push(self.parse_constructor()?);
        }

        let deriving = self.parse_deriving()?;

        Ok(Decl::DataDef {
            name,
            type_vars,
            constructors,
            deriving,
        })
    }

    /// A type constructor's declared name: an ordinary `UpperIdent`, or a type
    /// operator written in prefix parenthesised form `( :+: )`. Only `:`-leading
    /// symbolic operators are type constructors (see `is_type_operator`).
    fn parse_type_con_name(&mut self) -> PResult<String> {
        if self.at(&Token::LeftParen) {
            self.advance();
            let op = match self.peek().clone() {
                Token::Operator(o) if Self::is_type_operator(&o) => { self.advance(); o }
                other => return Err(self.err_here(format!(
                    "Expected a type operator (a ':'-leading symbol like ':+:') \
                     in parentheses after 'data', found {}", other))),
            };
            self.expect(&Token::RightParen)?;
            Ok(op)
        } else {
            self.expect_upper_ident()
        }
    }

    fn parse_constructor(&mut self) -> PResult<Constructor> {
        // Check for existential quantification: `forall a b. [Constraint =>] ConName fields`
        let mut existential_vars = Vec::new();
        let mut existential_constraints = Vec::new();
        if let Token::Ident(ref id) = self.peek().clone()
            && id == "forall" {
                self.advance();
                // Parse bound type variables until we see '.'
                while !self.at(&Token::Operator(".".to_string())) {
                    existential_vars.push(self.expect_ident()?);
                }
                self.expect(&Token::Operator(".".to_string()))?;
                // Check for optional constraints: `Show a =>`
                let save = self.checkpoint();
                if let Ok(constraints) = self.try_parse_constraints() {
                    if self.at(&Token::FatArrow) {
                        self.advance();
                        existential_constraints = constraints;
                    } else {
                        self.rewind(save);
                    }
                } else {
                    self.rewind(save);
                }
            }

        let name = self.expect_upper_ident()?;

        // Check for record syntax (may be on next line)
        let save_pos = self.checkpoint();
        self.skip_newlines_and_indent();
        if self.at(&Token::LeftBrace) {
            self.advance();
            let mut fields = Vec::new();
            loop {
                self.skip_newlines_and_indent();
                if self.at(&Token::RightBrace) {
                    break;
                }
                let field_name = self.expect_ident()?;
                // Optional external-key rename: `fieldName as "key" :: T`.
                // One shared name for every external boundary (LuaDict table
                // key, JSON object key). 'as' is not a reserved word — check
                // for Ident("as").
                let external_key = if matches!(self.peek(), Token::Ident(s) if s == "as") {
                    self.advance();
                    match self.peek().clone() {
                        Token::StrLit(s) => { self.advance(); Some(self.strlit_as_symbol(s, "a field rename key")?) }
                        _ => {
                            return Err(self.err_here(format!(
                                "Expected a string literal after 'as' in field '{}' (e.g. `{} as \"key\" :: T`), found {}",
                                field_name, field_name, self.peek()
                            )));
                        }
                    }
                } else {
                    None
                };
                self.expect(&Token::DblColon)?;
                let field_type = self.parse_type()?;
                fields.push(crate::ast::RecordField { name: field_name, external_key, ty: field_type });
                self.skip_newlines_and_indent();
                if self.at(&Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.skip_newlines_and_indent();
            self.expect(&Token::RightBrace)?;
            let external_name = self.parse_constructor_external_name(&name)?;
            return Ok(Constructor {
                name,
                external_name,
                fields: ConstructorFields::Named(fields),
                gadt_type: None,
                existential_vars,
                existential_constraints,
            });
        } else {
            // Not record syntax — backtrack
            self.rewind(save_pos);
        }

        // Positional fields. A bare `as` after the constructor name or a
        // field type can only start the constructor's external-name rename
        // (`Con T1 T2 as "name"`), so field-type parsing stops there — this
        // is what makes `Foo as "foo"` the rename instead of two phantom
        // field types. A type VARIABLE named `as` is still fine anywhere it
        // is not a whole bare field: `MkF (Maybe as)` parses as before.
        let mut fields = Vec::new();
        while self.is_type_atom_start() && !matches!(self.peek(), Token::Ident(s) if s == "as") {
            fields.push(self.parse_type_atom()?);
        }

        let external_name = self.parse_constructor_external_name(&name)?;
        Ok(Constructor {
            name,
            external_name,
            fields: ConstructorFields::Positional(fields),
            gadt_type: None,
            existential_vars,
            existential_constraints,
        })
    }

    /// Parse the optional trailing external-name rename of a data
    /// constructor: `Con field-types as "name"` (after the field types /
    /// the record braces, before `|`, `deriving`, or the end of the
    /// declaration). Mirrors the field-level `as "key"` rename: `as` is not
    /// a reserved word, so check for Ident("as"), and anything but a string
    /// literal after it is a located error rather than a silent misparse.
    fn parse_constructor_external_name(&mut self, con_name: &str) -> PResult<Option<String>> {
        if !matches!(self.peek(), Token::Ident(s) if s == "as") {
            return Ok(None);
        }
        self.advance();
        match self.peek().clone() {
            Token::StrLit(s) => {
                self.advance();
                Ok(Some(self.strlit_as_symbol(s, "a constructor rename name")?))
            }
            other => Err(self.err_here(format!(
                "Expected a string literal after 'as' in constructor '{}' (e.g. `{} as \"name\"`), found {}",
                con_name, con_name, other
            ))),
        }
    }

    /// Parse optional `deriving (Show, Eq)` clause after a data declaration.
    fn parse_deriving(&mut self) -> PResult<Vec<String>> {
        // Look ahead past newlines/indents for 'deriving'
        let save = self.checkpoint();
        self.skip_newlines_and_indent();
        if !self.at(&Token::Deriving) {
            self.rewind(save);
            return Ok(vec![]);
        }
        self.advance(); // consume 'deriving'

        let mut classes = Vec::new();
        if self.at(&Token::LeftParen) {
            self.advance();
            loop {
                self.skip_newlines_and_indent();
                if self.at(&Token::RightParen) {
                    self.advance();
                    break;
                }
                classes.push(self.expect_upper_ident()?);
                self.skip_newlines_and_indent();
                if self.at(&Token::Comma) {
                    self.advance();
                } else {
                    self.expect(&Token::RightParen)?;
                    break;
                }
            }
        } else {
            // deriving Show (single class, no parens)
            classes.push(self.expect_upper_ident()?);
        }

        Ok(classes)
    }

    fn parse_newtype_decl(&mut self) -> PResult<Decl> {
        self.expect(&Token::Newtype)?;
        let name = self.expect_upper_ident()?;

        let mut type_vars = Vec::new();
        while let Token::Ident(v) = self.peek() {
            type_vars.push(v.clone());
            self.advance();
        }

        self.expect(&Token::Eq)?;
        // The constructor forms the parser can settle itself: the type's own
        // name (`newtype W = W (Maybe Int)`, Haskell's common spelling) and
        // the record form under ANY constructor name
        // (`newtype Age = MkAge { unAge :: Int }`). A freely named
        // constructor WITHOUT braces (`newtype Rad = MkRad Double`) is
        // parsed as a type application here and resolved by the typechecker,
        // which knows the type names — see `register_newtype`'s shorthand
        // resolution. Everything else is the mata-ll shorthand
        // `newtype N = <type>` (constructor = type name).
        let mut con_name = None;
        let mut field = None;
        let inner;
        // The record brace may sit on the NEXT line (the data-declaration
        // path accepts that layout via checkpoint+skip; this detection used
        // to test tokens[pos+1] == LeftBrace directly, so an intervening
        // layout token broke the record form). Scan past layout tokens for
        // the detection, and skip them again after the constructor name.
        let braced_con = matches!(self.peek(), Token::UpperIdent(_)) && {
            let mut j = self.pos + 1;
            while j < self.tokens.len()
                && matches!(self.tokens[j].token, Token::Newline | Token::Indent(_))
            {
                j += 1;
            }
            j < self.tokens.len() && self.tokens[j].token == Token::LeftBrace
        };
        if let Token::UpperIdent(con) = self.peek().clone()
            && (con == name || braced_con)
        {
            self.advance();
            con_name = Some(con);
            let save_brace = self.checkpoint();
            self.skip_newlines_and_indent();
            if !self.at(&Token::LeftBrace) {
                self.rewind(save_brace);
            }
            if self.at(&Token::LeftBrace) {
                // Record form: exactly one selector (a newtype has exactly
                // one field).
                self.advance();
                let sel = self.expect_ident()?;
                self.expect(&Token::DblColon)?;
                inner = self.parse_type()?;
                if self.at(&Token::Comma) {
                    return Err(self.err_here(format!(
                        "A newtype has exactly one field, so 'newtype {} = \
                         {} {{ … }}' can declare only one selector",
                        name,
                        con_name.as_deref().unwrap_or(&name),
                    )));
                }
                self.expect(&Token::RightBrace)?;
                field = Some(sel);
            } else {
                inner = self.parse_type()?;
            }
        } else {
            inner = self.parse_type()?;
        }
        let deriving = self.parse_deriving()?;

        Ok(Decl::NewtypeDef {
            name,
            type_vars,
            con_name,
            field,
            inner,
            deriving,
        })
    }

    fn parse_import_decl(&mut self) -> PResult<Decl> {
        self.expect(&Token::Import)?;

        let qualified = if self.at(&Token::Qualified) {
            self.advance();
            true
        } else {
            false
        };

        let mut module_path = Vec::new();
        module_path.push(self.expect_upper_ident()?);
        while self.at(&Token::Operator(".".to_string())) {
            self.advance();
            module_path.push(self.expect_upper_ident()?);
        }

        if qualified {
            // 'as' is not a keyword — check for Ident("as")
            match self.peek().clone() {
                Token::Ident(ref s) if s == "as" => { self.advance(); }
                _ => return Err(self.err_here("Expected 'as' in qualified import".to_string())),
            }
            let alias = self.expect_upper_ident()?;
            return Ok(Decl::Import {
                module_path,
                items: ImportItems::Qualified(alias),
            });
        }

        // Check for `hiding` keyword (context-sensitive, parsed as Ident)
        let hiding = matches!(self.peek(), Token::Ident(s) if s == "hiding");
        if hiding {
            self.advance();
        }

        if self.at(&Token::LeftParen) {
            self.advance();
            let mut items = Vec::new();
            loop {
                if self.at(&Token::RightParen) {
                    break;
                }
                if let Token::UpperIdent(name) = self.peek().clone() {
                    self.advance();
                    if self.at(&Token::LeftParen) {
                        self.advance();
                        self.expect(&Token::Operator("..".to_string()))?;
                        self.expect(&Token::RightParen)?;
                        items.push(ImportItem::TypeAll(name));
                    } else {
                        items.push(ImportItem::TypeOnly(name));
                    }
                } else if self.at(&Token::LeftParen) {
                    // An operator item, `(&)` — the export list accepts
                    // this spelling, and the import (and hiding) lists
                    // used to reject it with "Expected identifier".
                    self.advance();
                    let op = match self.peek().clone() {
                        Token::Operator(op) => {
                            self.advance();
                            op
                        }
                        _ => {
                            return Err(self.err_here(
                                "Expected an operator inside the parentheses \
                                 of an import-list item (e.g. `(&)`)"
                                    .to_string(),
                            ));
                        }
                    };
                    self.expect(&Token::RightParen)?;
                    items.push(ImportItem::Value(op));
                } else {
                    let name = self.expect_ident()?;
                    items.push(ImportItem::Value(name));
                }
                if self.at(&Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(&Token::RightParen)?;
            return Ok(Decl::Import {
                module_path,
                items: if hiding { ImportItems::Hiding(items) } else { ImportItems::Specific(items) },
            });
        }

        Ok(Decl::Import {
            module_path,
            items: ImportItems::All,
        })
    }

    /// Parse a method name at the start of a class/instance member line: a
    /// plain identifier, or an operator in parentheses (`(+)`). Returns
    /// `Ok(None)` when the line starts with neither — the member block ends
    /// there. `what` names the construct for the error when the parentheses
    /// hold something other than an operator.
    fn parse_method_name(&mut self, what: &str) -> PResult<Option<String>> {
        if self.at(&Token::LeftParen) {
            self.advance();
            let op = match self.peek().clone() {
                Token::Operator(op) => { self.advance(); op }
                _ => return Err(self.err_here(format!("Expected operator in {}", what))),
            };
            self.expect(&Token::RightParen)?;
            Ok(Some(op))
        } else if let Token::Ident(name) = self.peek().clone() {
            self.advance();
            Ok(Some(name))
        } else {
            Ok(None)
        }
    }

    /// The infix method-definition tail: the already-consumed identifier
    /// was the LEFT OPERAND (`x <> y = …` or ``x `op` y = …``) and the
    /// operator is the method actually being defined. Mirrors
    /// parse_value_decl's top-level infix branch; class and instance
    /// bodies used to lack it, so the spelling the top level accepts died
    /// there with "Expected '='".
    fn parse_infix_method_clause(&mut self, left_name: String) -> PResult<(String, Clause)> {
        let loc = self.peek_loc();
        let span = Span::new(loc.line, loc.col);
        let saved_block = self.block_indent;
        self.block_indent = self.current_indent;
        let left = Pattern::Var(left_name);
        let op = match self.peek().clone() {
            Token::Operator(op) => {
                self.advance();
                op
            }
            Token::Backtick => {
                self.advance();
                let f = self.expect_ident()?;
                self.expect(&Token::Backtick)?;
                f
            }
            _ => unreachable!("caller checked for Operator/Backtick"),
        };
        let right = self.parse_pattern_atom()?;
        let clause = self.finish_clause(vec![left, right], span, saved_block)?;
        Ok((op, clause))
    }

    fn parse_class_decl(&mut self) -> PResult<Vec<Decl>> {
        // The declaration's own indent is the enclosing layout context of
        // its `where` block (see `open_layout_block`).
        let decl_indent = self.current_indent;
        self.expect(&Token::Class)?;

        // Parse optional superclass constraints: `Eq a =>` needs no parens,
        // `(Eq a, Show a) =>` wraps one or more. Parsing is speculative: when
        // the tokens turn out not to form a `... =>` context, they are
        // re-read as the class head itself.
        let save = self.checkpoint();
        let mut superclasses = Vec::new();

        if self.at(&Token::LeftParen) {
            // ( C1 v, C2 v, ... ) =>
            self.advance();
            let mut supers = Vec::new();
            loop {
                let Token::UpperIdent(name) = self.peek().clone() else { break };
                self.advance();
                if !matches!(self.peek(), Token::Ident(_)) {
                    supers.clear();
                    break;
                }
                self.advance();
                supers.push(name);
                if self.at(&Token::Comma) {
                    self.advance();
                    continue;
                }
                break;
            }
            let mut complete = !supers.is_empty() && self.at(&Token::RightParen);
            if complete {
                self.advance();
                complete = self.at(&Token::FatArrow);
            }
            if complete {
                self.advance(); // consume =>
                superclasses = supers;
            } else {
                // A class head cannot start with '(' — this was an attempt
                // at a context, so a targeted error beats backtracking into
                // "Expected type/constructor name" at the paren.
                self.rewind(save);
                return Err(self.err_here(
                    "A parenthesized class context lists superclass \
                     constraints, each a class name applied to the class's \
                     type variable, and ends with '=>': \
                     class (Eq a, Show a) => MyClass a"
                        .to_string(),
                ));
            }
        } else {
            // Try to parse a single constraint followed by =>
            let first = self.expect_upper_ident()?;
            if let Token::Ident(_) = self.peek() {
                let tv = self.expect_ident()?;
                if self.at(&Token::FatArrow) {
                    // Single constraint: Eq a =>
                    superclasses.push(first);
                    self.advance(); // consume =>
                } else if self.at(&Token::Comma) {
                    // `class Eq a, Show a => ...`: several constraints were
                    // meant, but a bare comma-separated context is not legal
                    // Haskell — GHC requires the parens too.
                    return Err(self.err_here(format!(
                        "Several superclass constraints must be wrapped in \
                         parentheses: class ({} {}, ...) => ...",
                        first, tv
                    )));
                } else {
                    // No constraint, backtrack
                    self.rewind(save);
                }
            } else {
                self.rewind(save);
            }
        }

        let class_name = self.expect_upper_ident()?;
        let type_var = self.expect_ident()?;
        let where_line = self.peek_loc().line;
        self.expect(&Token::Where)?;

        let mut methods = Vec::new();
        let Some(method_indent) = self.open_layout_block(where_line, decl_indent) else {
            return Ok(vec![Decl::ClassDecl { name: class_name, type_var, superclasses, methods }]);
        };

        loop {
            self.skip_newlines_and_indent();
            if self.at_eof() || self.current_indent < method_indent {
                break;
            }

            // Parse method name: a `name :: type` signature line or a
            // default method clause. Could be an operator like (+) :: ...
            let Some(name) = self.parse_method_name("class method")? else { break };

            // A `::` after the name makes it a type signature; an operator
            // (or backtick) makes it the INFIX definition form (`x <> y =
            // …` — the identifier was the left operand); anything else is
            // a prefix default method clause — `parse_clause` picks up
            // right after the already-consumed name, exactly like a
            // top-level function clause.
            if self.at(&Token::DblColon) {
                self.advance();
                let ty = self.parse_type()?;
                methods.push(ClassMethod { name, ty, default_clauses: None });
            } else {
                let (name, clause) =
                    if matches!(self.peek(), Token::Operator(_) | Token::Backtick) {
                        self.parse_infix_method_clause(name)?
                    } else {
                        (name.clone(), self.parse_clause()?)
                    };

                // Attach to the matching method signature
                if let Some(m) = methods.iter_mut().find(|m| m.name == name) {
                    match &mut m.default_clauses {
                        Some(clauses) => clauses.push(clause),
                        None => m.default_clauses = Some(vec![clause]),
                    }
                } else {
                    return Err(Box::new(Diagnostic::parse_at(format!(
                        "Default implementation for '{}' has no preceding type signature in class '{}'",
                        name, class_name
                    ), clause.span)));
                }
            }
        }

        Ok(vec![Decl::ClassDecl { name: class_name, type_var, superclasses, methods }])
    }

    fn parse_instance_decl(&mut self) -> PResult<Vec<Decl>> {
        let decl_indent = self.current_indent;
        self.expect(&Token::Instance)?;

        // Parse an optional context, then `ClassName TargetType where`.
        // Contexts come in the same three shapes as in type signatures —
        // `Show a =>`, `(Show a) =>`, `(Show a, Eq b) =>` — so reuse the
        // signature-context parser. Speculative: `instance Show (Tree a)` also
        // starts like a constraint (`Show` + a type atom), so only commit to
        // the context reading when a `=>` actually follows; otherwise backtrack
        // and treat what was parsed as the class + target.
        let save = self.checkpoint();
        let context = match self.try_parse_constraints() {
            Ok(cs) if self.at(&Token::FatArrow) => {
                self.advance(); // consume =>
                cs
            }
            _ => {
                self.rewind(save);
                Vec::new()
            }
        };

        let class_name = self.expect_upper_ident()?;
        let target_type = self.parse_type_atom()?;

        let where_line = self.peek_loc().line;
        self.expect(&Token::Where)?;

        // An empty body (`instance C T where` with every method defaulted)
        // is legal; the following declarations are NOT its methods.
        let mut methods = Vec::new();
        let Some(method_indent) = self.open_layout_block(where_line, decl_indent) else {
            return Ok(vec![Decl::InstanceDecl { class_name, target_type, context, methods }]);
        };

        loop {
            self.skip_newlines_and_indent();
            if self.at_eof() || self.current_indent < method_indent {
                break;
            }

            let Some(name) = self.parse_method_name("instance method")? else { break };

            // Collect all clauses for this method. An operator (or
            // backtick) after the identifier is the INFIX definition form
            // (`x <> y = …` — the identifier was the left operand).
            let (name, clause) =
                if matches!(self.peek(), Token::Operator(_) | Token::Backtick) {
                    self.parse_infix_method_clause(name)?
                } else {
                    (name, self.parse_clause()?)
                };

            // Check if there's an existing method we should add a clause to
            if let Some(existing) = methods.iter_mut().find(|m: &&mut InstanceMethod| m.name == name) {
                existing.clauses.push(clause);
            } else {
                methods.push(InstanceMethod { name, clauses: vec![clause] });
            }
        }

        Ok(vec![Decl::InstanceDecl { class_name, target_type, context, methods }])
    }

    fn parse_export_decl(&mut self) -> PResult<Vec<Decl>> {
        self.expect(&Token::Export)?;
        let name = self.expect_ident()?;
        self.expect(&Token::DblColon)?;
        let ty = self.parse_type()?;
        // ExportSig also serves as a TypeSig so the function gets type-checked
        Ok(vec![
            Decl::ExportSig { name: name.clone(), ty: ty.clone() },
            Decl::TypeSig { name, ty },
        ])
    }

    /// Parse: type family Name args where
    ///            Name Pattern = Result
    ///            ...
    fn parse_type_family_decl(&mut self) -> PResult<Vec<Decl>> {
        self.expect(&Token::KwType)?;

        // Plain type alias: `type Name a b = ...`
        if !self.at(&Token::Family) {
            let name = self.expect_upper_ident()?;
            let mut params = Vec::new();
            while matches!(self.peek(), Token::Ident(_)) {
                if let Token::Ident(p) = self.peek().clone() {
                    params.push(p);
                    self.advance();
                }
            }
            self.expect(&Token::Eq)?;
            let ty = self.parse_type()?;
            return Ok(vec![Decl::TypeAlias { name, params, ty }]);
        }

        self.expect(&Token::Family)?;
        let name = self.expect_upper_ident()?;

        // Capture the header parameter names: their count fixes the family's
        // arity (and thus kind) even when it declares no equations.
        let mut params = Vec::new();
        while let Token::Ident(p) = self.peek().clone() {
            params.push(p);
            self.advance();
        }

        self.expect(&Token::Where)?;
        self.skip_newlines_and_indent();

        let mut equations = Vec::new();
        let eq_indent = self.current_indent;

        loop {
            self.skip_newlines_and_indent();
            if self.at_eof() || self.current_indent < eq_indent {
                break;
            }
            // Each equation: FamilyName argType... = resultType
            if let Token::UpperIdent(ref eq_name) = self.peek().clone() {
                if *eq_name != name {
                    break;
                }
                self.advance(); // consume family name
                let mut args = Vec::new();
                while !self.at(&Token::Eq) && !self.at_eof() {
                    args.push(self.parse_type_atom()?);
                }
                self.expect(&Token::Eq)?;
                let result = self.parse_type()?;
                equations.push(TypeFamilyEq { args, result });
            } else {
                break;
            }
        }

        Ok(vec![Decl::TypeFamily { name, params, equations }])
    }

    fn parse_intrinsic_decl(&mut self) -> PResult<Vec<Decl>> {
        self.expect(&Token::Intrinsic)?;
        // intrinsic type family Name ... where ... => parse as type family
        if self.at(&Token::KwType) {
            return self.parse_type_family_decl();
        }
        // intrinsic name :: type => parse as type signature (implementation is compiler-provided)
        if let Token::Ident(_) = self.peek() {
            let name = self.expect_ident()?;
            self.expect(&Token::DblColon)?;
            let ty = self.parse_type()?;
            return Ok(vec![Decl::TypeSig { name, ty }]);
        }
        Err(self.err_here(format!(
            "An 'intrinsic' declaration is either 'intrinsic type family \
             ...' or 'intrinsic name :: type' (a signature whose \
             implementation the compiler provides); found {}",
            self.peek()
        )))
    }

    /// Look up an operator's fixity: a declared fixity (this module's or an
    /// imported one's, both seeded into `self.fixities`) overrides the
    /// builtin defaults.
    fn operator_fixity(&self, op: &str) -> (Assoc, u8) {
        if let Some(&(assoc, prec)) = self.fixities.get(op) {
            (assoc, prec)
        } else {
            default_operator_fixity(op)
        }
    }

    /// The error for two same-precedence operators whose fixities do not
    /// allow an unparenthesized chain (the Haskell precedence-parsing rule):
    /// either one of them is non-associative, or one is infixl and the other
    /// infixr. In both cases the grammar defines no grouping, so the
    /// expression is ambiguous and must be parenthesized. Reported at the
    /// second operator, which the parser is currently looking at.
    fn fixity_conflict_err(
        &self,
        parent: &ParentOp,
        op2: &str,
        assoc2: Assoc,
        prec: u8,
    ) -> Box<Diagnostic> {
        let d1 = op_display(&parent.op);
        let d2 = op_display(op2);
        let e1 = op_in_expr(&parent.op);
        let e2 = op_in_expr(op2);
        let msg = if parent.assoc == Assoc::None && assoc2 == Assoc::None {
            if parent.op == op2 {
                format!(
                    "Cannot chain {d1}: it is non-associative (infix {prec}), \
                     so 'a {e1} b {e1} c' has no defined grouping"
                )
            } else {
                format!(
                    "Cannot mix {d1} and {d2} in one chain: both are \
                     non-associative (infix {prec}), so 'a {e1} b {e2} c' \
                     has no defined grouping"
                )
            }
        } else if parent.assoc == Assoc::None || assoc2 == Assoc::None {
            let na = if parent.assoc == Assoc::None { &parent.op } else { op2 };
            format!(
                "Cannot mix {d1} and {d2} in one chain: {} is non-associative \
                 (infix {prec}), so it cannot chain with another \
                 precedence-{prec} operator",
                op_display(na)
            )
        } else {
            format!(
                "Cannot mix {d1} ({} {prec}) and {d2} ({} {prec}) in one \
                 chain: they bind at the same precedence but group in \
                 opposite directions, so 'a {e1} b {e2} c' has no defined \
                 grouping",
                assoc_keyword(parent.assoc),
                assoc_keyword(assoc2)
            )
        };
        let loc = self.peek_loc();
        let mut diag = Diagnostic::parse_at(msg, Span::new(loc.line, loc.col));
        diag.notes.push(format!(
            "parenthesize one side: '(a {e1} b) {e2} c' or 'a {e1} (b {e2} c)'"
        ));
        if is_comparison_op(&parent.op) && is_comparison_op(op2) {
            diag.notes.push(format!(
                "to compare three values, chain with '&&': 'a {e1} b && b {e2} c'"
            ));
        }
        Box::new(diag)
    }

    /// Parse a fixity declaration: `infixl 6 +`, `infixr 5 :`,
    /// `infix 4 \`elem\``, or a comma list (`infixl 7 *, /`).
    /// The operators were already seeded into `self.fixities` by the
    /// pre-parse scan (fixity is scope-wide, not textually ordered), so this
    /// only records the declarations.
    fn parse_fixity_decl(&mut self) -> PResult<Vec<Decl>> {
        let assoc = match self.peek() {
            Token::Infixl => { self.advance(); Assoc::Left }
            Token::Infixr => { self.advance(); Assoc::Right }
            Token::Infix => { self.advance(); Assoc::None }
            _ => unreachable!(),
        };
        let prec = match self.peek() {
            Token::IntLit(n) if (0..=9).contains(n) => {
                let p = *n as u8;
                self.advance();
                p
            }
            Token::IntLit(_) => {
                return Err(self.err_here(
                    "Fixity precedence must be between 0 and 9".to_string(),
                ))
            }
            _ => return Err(self.err_here("Expected precedence level (0-9) after infixl/infixr/infix".to_string())),
        };
        let mut decls = Vec::new();
        loop {
            let op = match self.peek().clone() {
                Token::Operator(s) => { self.advance(); s }
                Token::Ident(s) => { self.advance(); s } // backtick operator, bare
                Token::Backtick => {
                    self.advance();
                    let s = self.expect_ident()?;
                    self.expect(&Token::Backtick)?;
                    s
                }
                _ => return Err(self.err_here("Expected operator after fixity precedence".to_string())),
            };
            self.fixities.insert(op.clone(), (assoc, prec));
            decls.push(Decl::FixityDecl { assoc, prec, op });
            if self.at(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        Ok(decls)
    }

    /// Parse a value declaration (type signature or function definition).
    fn parse_value_decl(&mut self) -> PResult<Vec<Decl>> {
        let loc = self.peek_loc();
        let (def_line, def_col) = (loc.line, loc.col);
        let name = self.expect_ident()?;

        // Type signature: `name :: type`
        if self.at(&Token::DblColon) {
            self.advance();
            let ty = self.parse_type()?;
            return Ok(vec![Decl::TypeSig { name, ty }]);
        }

        // Infix definition: `lhs <op> rhs = ...` or ``lhs `f` rhs = ...``.
        // The leading identifier is the first operand (a var pattern), the
        // operator (or backtick'd name) is the function being defined, and the
        // following pattern is the second operand.
        if matches!(self.peek(), Token::Operator(_) | Token::Backtick) {
            let span = Span::new(def_line, def_col);
            let saved_block = self.block_indent;
            self.block_indent = self.current_indent;

            let left = Pattern::Var(name);
            let op = match self.peek().clone() {
                Token::Operator(op) => {
                    self.advance();
                    op
                }
                Token::Backtick => {
                    self.advance();
                    let f = self.expect_ident()?;
                    self.expect(&Token::Backtick)?;
                    f
                }
                _ => unreachable!(),
            };
            let right = self.parse_pattern_atom()?;
            let clause = self.finish_clause(vec![left, right], span, saved_block)?;
            return Ok(vec![Decl::FunDef {
                name: op,
                clauses: vec![clause],
            }]);
        }

        // Function definition: `name patterns = expr`
        let clause = self.parse_clause()?;

        Ok(vec![Decl::FunDef {
            name,
            clauses: vec![clause],
        }])
    }

    /// Parse an operator definition like `(+) a b = ...`
    fn parse_operator_decl(&mut self) -> PResult<Vec<Decl>> {
        self.expect(&Token::LeftParen)?;
        let op = match self.peek().clone() {
            Token::Operator(op) => {
                self.advance();
                op
            }
            _ => {
                return Err(self.err_here("Expected operator".to_string()));
            }
        };
        self.expect(&Token::RightParen)?;

        // Type signature: `(op) :: type`
        if self.at(&Token::DblColon) {
            self.advance();
            let ty = self.parse_type()?;
            return Ok(vec![Decl::TypeSig { name: op, ty }]);
        }

        let clause = self.parse_clause()?;
        Ok(vec![Decl::FunDef {
            name: op,
            clauses: vec![clause],
        }])
    }

    fn parse_clause(&mut self) -> PResult<Clause> {
        let loc = self.peek_loc();
        let span = Span::new(loc.line, loc.col);
        // The clause's column is the layout block for its RHS: continuation
        // lines must be indented past it; a line at this column is the next
        // clause/binding. Covers top-level defs and class/instance methods.
        let saved_block = self.block_indent;
        self.block_indent = self.current_indent;

        let mut patterns = Vec::new();
        while self.is_pattern_atom_start() || matches!(self.peek(), Token::UpperIdent(_)) {
            if let Token::UpperIdent(_) = self.peek() {
                // Constructor or True/False at clause level — parse as full pattern
                // but don't consume args (they're separate clause patterns)
                let pat = match self.peek().clone() {
                    Token::UpperIdent(name) => {
                        self.advance();
                        match name.as_str() {
                            "True" => Pattern::LitPat(Literal::Bool(true)),
                            "False" => Pattern::LitPat(Literal::Bool(false)),
                            _ => Pattern::Constructor { name, args: vec![] },
                        }
                    }
                    _ => unreachable!(),
                };
                patterns.push(pat);
            } else {
                patterns.push(self.parse_pattern_atom()?);
            }
        }

        self.finish_clause(patterns, span, saved_block)
    }

    /// Parse the tail of a clause (guards or `= body`, then an optional `where`)
    /// given its already-parsed parameter patterns. `saved_block` is the
    /// caller's previous `block_indent`, restored before returning. Shared by
    /// the prefix form (`f a b = ...`) and the infix form (`a `op` b = ...`).
    fn finish_clause(
        &mut self,
        patterns: Vec<Pattern>,
        span: Span,
        saved_block: usize,
    ) -> PResult<Clause> {
        // Guards
        self.skip_newlines_and_indent();
        let guards = self.parse_guard_chain(&Token::Eq)?;

        // A guarded clause has no single body: the guard chain IS the body.
        let body = if guards.is_empty() {
            self.expect(&Token::Eq)?;
            Some(self.parse_expr()?)
        } else {
            None
        };

        // where clause
        let where_binds = self.parse_where()?;

        self.block_indent = saved_block;
        Ok(Clause {
            patterns,
            guards,
            body,
            where_binds,
            span,
        })
    }

    /// One `| cond <sep> body` guard chain, shared by function clauses
    /// (`= body`), where bindings (`= body`, then desugared to if/else),
    /// and case branches (`-> body`). The chain is parsed identically in
    /// all three positions; each guard body gets the `Spanned` statement
    /// marker so a type error inside it is reported at the body's own
    /// line, not the clause head.
    fn parse_guard_chain(&mut self, sep: &Token) -> PResult<Vec<Guard>> {
        let mut guards = Vec::new();
        while self.at(&Token::Pipe) {
            self.advance();
            let mut condition = self.parse_guard_qualifier()?;
            // Haskell 2010 §3.13: a guard is a comma-separated qualifier
            // list; the guard succeeds when every qualifier holds. Boolean
            // qualifiers desugar to `&&` (short-circuit left-to-right, the
            // same order and laziness as sequential qualifier checking).
            // These used to die with a bare "Expected '='".
            while self.at(&Token::Comma) {
                self.advance();
                let next = self.parse_guard_qualifier()?;
                condition = Expr::InfixApp {
                    op: "&&".to_string(),
                    lhs: Box::new(condition),
                    rhs: Box::new(next),
                };
            }
            self.expect(sep)?;
            let body = self.parse_stmt_expr()?;
            guards.push(Guard { condition, body });
            self.skip_newlines_and_indent();
        }
        Ok(guards)
    }

    /// One guard qualifier: a boolean expression. The two BINDING qualifier
    /// forms of Haskell 2010 §3.13 — pattern guards (`Just v <- m`) and
    /// `let` qualifiers — introduce names whose scope is the rest of the
    /// guard and its body, which the Guard AST (one Bool condition) cannot
    /// carry; they are rejected with a rewrite hint instead of the bare
    /// "Expected '='" they used to die with.
    fn parse_guard_qualifier(&mut self) -> PResult<Expr> {
        let loc = self.peek_loc().clone();
        if self.at(&Token::Let) {
            let mut diag = Diagnostic::parse_at(
                "'let' qualifiers in guards are not supported",
                Span::new(loc.line, loc.col),
            );
            diag.notes.push(
                "a 'let' inside a guard (Haskell 2010 §3.13) binds names for the \
                 rest of the guard and its body; bind the name in a 'where' \
                 clause or a 'let … in …' around the right-hand side instead"
                    .to_string(),
            );
            return Err(Box::new(diag));
        }
        let expr = self.parse_expr()?;
        if self.at(&Token::Bind) {
            let mut diag = Diagnostic::parse_at(
                "pattern guards ('pat <- expr' inside a guard) are not supported",
                Span::new(loc.line, loc.col),
            );
            diag.notes.push(
                "a pattern guard (Haskell 2010 §3.13) matches a pattern against an \
                 expression and falls through to the next guard when it fails; \
                 rewrite with a 'case' expression in the right-hand side, or a \
                 'Maybe'-returning helper checked with a boolean guard"
                    .to_string(),
            );
            return Err(Box::new(diag));
        }
        Ok(expr)
    }

    /// The head of one binding-group entry: `name [patterns]`. Shared by
    /// where blocks, let-expression bindings, and do-`let` groups (the
    /// caller has already established that an identifier starts the line).
    fn parse_binding_head(&mut self) -> PResult<(String, Vec<Pattern>)> {
        let name = self.expect_ident()?;
        let mut patterns = Vec::new();
        while self.is_pattern_start() {
            patterns.push(self.parse_pattern_atom()?);
        }
        Ok((name, patterns))
    }

    /// Build the `LocalDef` for a binding-group entry: a function binding
    /// `f x y = e` desugars to the value binding `f = \x y -> e`, so the
    /// whole group stays a uniform value-binding group and is inferred and
    /// generated as one mutually recursive scope (patterns on group
    /// bindings are otherwise not handled by the let pipeline). Where
    /// blocks do NOT use this — a where binding keeps its patterns and the
    /// where pipeline handles the function form itself.
    fn group_binding(&mut self, name: String, patterns: Vec<Pattern>, body: Expr) -> PResult<LocalDef> {
        let body = if patterns.is_empty() {
            body
        } else {
            Expr::Lambda {
                params: self.lambda_param_names(patterns)?,
                body: Box::new(body),
            }
        };
        Ok(LocalDef { name, patterns: vec![], body })
    }

    fn parse_where(&mut self) -> PResult<Vec<LocalDef>> {
        self.skip_newlines_and_indent();
        if !self.at(&Token::Where) {
            return Ok(vec![]);
        }
        let where_line = self.peek_loc().line;
        self.advance();

        // The clause's column (`block_indent`, set by parse_clause) is the
        // enclosing layout context: bindings must be indented past it, or
        // the `where` is empty and the next line is the next definition.
        let mut binds = Vec::new();
        let saved_block = self.block_indent;
        let Some(where_indent) = self.open_layout_block(where_line, self.block_indent) else {
            return Ok(binds);
        };
        self.block_indent = where_indent;

        let mut fresh_counter = 0usize;
        loop {
            self.skip_newlines_and_indent();
            if self.at_eof() || self.current_indent < where_indent {
                break;
            }
            // Tuple pattern binding `(a, b) = expr`: the same lazy-selector
            // desugar as parse_let_binds — one fresh binding for the
            // scrutinee plus one selector binding per pattern variable, all
            // in the same recursive group, so the variables are in scope
            // for siblings and the match happens on first demand. A where
            // block used to break out of its loop on the '(' and the
            // binding died far away as "Expected operator".
            if matches!(self.peek(), Token::LeftParen) {
                let save = self.checkpoint();
                match self.parse_pattern_atom() {
                    Ok(pat @ Pattern::Tuple(_)) if self.at(&Token::Eq) => {
                        self.advance(); // consume '='
                        let rhs = self.parse_stmt_expr()?;
                        let fresh = format!("__wtup_{}", fresh_counter);
                        fresh_counter += 1;
                        binds.push(LocalDef { name: fresh.clone(), patterns: vec![], body: rhs });
                        for v in pat.var_names() {
                            binds.push(LocalDef {
                                name: v.clone(),
                                patterns: vec![],
                                body: Expr::Case {
                                    scrutinee: Box::new(Expr::Var(fresh.clone())),
                                    branches: vec![CaseBranch {
                                        pattern: pat.clone(),
                                        guards: vec![],
                                        body: Some(Expr::Var(v)),
                                    }],
                                },
                            });
                        }
                        continue;
                    }
                    _ => {
                        self.rewind(save);
                        break;
                    }
                }
            }
            if !matches!(self.peek(), Token::Ident(_)) {
                break;
            }
            let (name, patterns) = self.parse_binding_head()?;

            // Handle guards: go acc i | i <= 0 = acc | otherwise = ...
            self.skip_newlines_and_indent();
            if self.at(&Token::Pipe) {
                // Guarded where binding: parse the shared chain, then
                // desugar to an if/else spine (a where binding is one
                // equation — no next-clause fall-through to preserve).
                let guards = self.parse_guard_chain(&Token::Eq)?;
                let body = guards.into_iter().rev().fold(
                    Expr::App(Box::new(Expr::Var("error".into())), Box::new(Expr::Lit(Literal::Str(b"non-exhaustive guards".to_vec())))),
                    |else_branch, g| Expr::If {
                        cond: Box::new(g.condition),
                        then_branch: Box::new(g.body),
                        else_branch: Box::new(else_branch),
                    },
                );
                binds.push(LocalDef { name, patterns, body });
            } else {
                self.expect(&Token::Eq)?;
                let body = self.parse_stmt_expr()?;
                binds.push(LocalDef { name, patterns, body });
            }
        }
        self.block_indent = saved_block;

        Ok(binds)
    }

    // --- Type parsing ---

    // Depth-guard wrapper: the grammar rule itself is in `parse_type_inner`.
    fn parse_type(&mut self) -> PResult<Type> {
        self.guarded("type", Self::parse_type_inner)
    }

    fn parse_type_inner(&mut self) -> PResult<Type> {
        // Check for forall: `forall s. type`
        if let Token::Ident(ref name) = self.peek().clone()
            && name == "forall" {
                self.advance();
                let var = self.expect_ident()?;
                self.expect(&Token::Operator(".".to_string()))?;
                let inner = self.parse_type()?;
                return Ok(Type::Forall {
                    var,
                    inner: Box::new(inner),
                });
            }

        // Check for constraints: `Show a => ...`
        let save = self.checkpoint();
        if let Ok(constraints) = self.try_parse_constraints()
            && self.at(&Token::FatArrow) {
                self.advance();
                let ty = self.parse_type_arrow()?;
                return Ok(Type::Constrained {
                    constraints,
                    ty: Box::new(ty),
                });
            }
        self.rewind(save);
        self.parse_type_arrow()
    }

    fn try_parse_constraints(&mut self) -> PResult<Vec<Constraint>> {
        let mut constraints = Vec::new();
        if self.at(&Token::LeftParen) {
            self.advance();
            loop {
                let class_name = self.expect_upper_ident()?;
                let type_arg = self.parse_type_atom()?;
                constraints.push(Constraint { class_name, type_arg });
                if self.at(&Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(&Token::RightParen)?;
        } else {
            let class_name = self.expect_upper_ident()?;
            let type_arg = self.parse_type_atom()?;
            constraints.push(Constraint { class_name, type_arg });
        }
        Ok(constraints)
    }

    // Depth-guard wrapper: the grammar rule itself is in `parse_type_arrow_inner`.
    fn parse_type_arrow(&mut self) -> PResult<Type> {
        self.guarded("type", Self::parse_type_arrow_inner)
    }

    fn parse_type_arrow_inner(&mut self) -> PResult<Type> {
        let lhs = self.parse_type_op(0)?;
        self.skip_newlines_and_indent();
        // A multiplicity annotation before the arrow: `a %1 -> b` (the
        // argument is consumed exactly once) or the explicit unrestricted
        // spellings `a %Many -> b` / `a %'Many -> b`. `%` only ever means
        // this in a type — types have no operators.
        if self.at(&Token::Operator("%".to_string())) {
            let mult = self.parse_multiplicity()?;
            self.skip_newlines_and_indent();
            if !self.at(&Token::Arrow) {
                return Err(self.err_here(
                    "a multiplicity annotation belongs to a function arrow: \
                     expected '->' after the '%' annotation".to_string()));
            }
            self.advance();
            self.skip_newlines_and_indent();
            let rhs = self.parse_type_arrow()?;
            return Ok(Type::Arrow(Box::new(lhs), Box::new(rhs), mult));
        }
        if self.at(&Token::Arrow) {
            self.advance();
            self.skip_newlines_and_indent();
            let rhs = self.parse_type_arrow()?;
            Ok(Type::Arrow(Box::new(lhs), Box::new(rhs), MultAnn::Many))
        } else {
            Ok(lhs)
        }
    }

    /// Parse the multiplicity spelling after a `%` in a type: `1` (linear /
    /// linear — the argument must be consumed exactly once), `Many` / `'Many`
    /// (explicitly unrestricted, the same as a plain arrow), or a lowercase
    /// name (`a %m -> b`: a multiplicity VARIABLE the signature is
    /// polymorphic over — each caller decides whether it is `%1` or
    /// unrestricted).
    fn parse_multiplicity(&mut self) -> PResult<MultAnn> {
        self.advance(); // consume '%'
        match self.peek().clone() {
            Token::IntLit(1) => {
                self.advance();
                Ok(MultAnn::One)
            }
            Token::UpperIdent(ref name) if name == "Many" => {
                self.advance();
                Ok(MultAnn::Many)
            }
            // A named multiplicity variable, like a type variable but in the
            // multiplicity namespace.
            Token::Ident(name) => {
                self.advance();
                Ok(MultAnn::Var(name))
            }
            // The DataKinds-style promoted spelling `%'Many`.
            Token::Tick => {
                self.advance();
                match self.peek().clone() {
                    Token::UpperIdent(ref name) if name == "Many" => {
                        self.advance();
                        Ok(MultAnn::Many)
                    }
                    other => Err(self.err_here(format!(
                        "unknown multiplicity '%'{}': the only multiplicities are \
                         '%1' (the function consumes the argument exactly once), \
                         '%Many' (no restriction — the same as a plain '->'), and \
                         a lowercase multiplicity variable like '%m'",
                        other))),
                }
            }
            other => Err(self.err_here(format!(
                "unknown multiplicity '%{}': the only multiplicities are \
                 '%1' (the function consumes the argument exactly once), \
                 '%Many' (no restriction — the same as a plain '->'), and \
                 a lowercase multiplicity variable like '%m'",
                other))),
        }
    }

    /// Infix type operators (`f :+: g`, `f :*: g`), sitting between the arrow
    /// layer (looser) and application (tighter). Only symbolic operators that
    /// begin with `:` are type operators — a type-level operator is a type
    /// CONSTRUCTOR, exactly as `:`-leading value operators are data
    /// constructors — which keeps `%` (multiplicity), `.` (forall / qualifier)
    /// and `..` out. Fixity comes from the same table value operators use, so a
    /// module's `infixr 6 :*:` / `infixr 5 :+:` group `a :+: b :*: c` as `a :+:
    /// (b :*: c)`. `F a :+: G b` desugars to `(:+:) (F a) (G b)`.
    fn is_type_operator(op: &str) -> bool {
        op.starts_with(':') && op.len() > 1
    }

    fn parse_type_op(&mut self, min_prec: u8) -> PResult<Type> {
        let mut lhs = self.parse_type_app()?;
        loop {
            let op = match self.peek() {
                Token::Operator(o) if Self::is_type_operator(o) => o.clone(),
                _ => break,
            };
            let (assoc, prec) = self.operator_fixity(&op);
            if prec < min_prec {
                break;
            }
            self.advance();
            self.skip_newlines_and_indent();
            // Left-assoc consumes only tighter operators on its right (prec+1);
            // right-assoc re-enters at its own precedence so it nests rightward.
            let next_min = if assoc == Assoc::Left { prec + 1 } else { prec };
            let rhs = self.parse_type_op(next_min)?;
            lhs = Type::App(
                Box::new(Type::App(Box::new(Type::Con(op)), Box::new(lhs))),
                Box::new(rhs),
            );
        }
        Ok(lhs)
    }

    fn parse_type_app(&mut self) -> PResult<Type> {
        let mut ty = self.parse_type_atom()?;
        while self.is_type_atom_start() {
            let arg = self.parse_type_atom()?;
            ty = Type::App(Box::new(ty), Box::new(arg));
        }
        Ok(ty)
    }

    fn is_type_atom_start(&self) -> bool {
        matches!(
            self.peek(),
            Token::UpperIdent(_)
                | Token::Ident(_)
                | Token::LeftParen
                | Token::LeftBracket
                | Token::StrLit(_)
                | Token::Tick
        )
    }

    /// After consuming an UpperIdent, join an adjacent `.UpperIdent` chain into
    /// a module-qualified name like `Data.Map` or `M.Map`. Returns None when the
    /// UpperIdent stands alone. Requiring adjacency (no spaces around the dot)
    /// distinguishes a qualifier from the only other `.` a type can contain — a
    /// `forall` dot, which always follows a lowercase variable, never an
    /// UpperIdent.
    fn try_parse_qualified_tail(&mut self, head: &str) -> Option<String> {
        let mut name = head.to_string();
        let mut matched = false;
        loop {
            if self.pos == 0 || self.pos + 1 >= self.tokens.len() {
                break;
            }
            let prev = &self.tokens[self.pos - 1];
            let dot = &self.tokens[self.pos];
            if !matches!(&dot.token, Token::Operator(o) if o == ".") {
                break;
            }
            if dot.line != prev.line || dot.col != prev.col + token_len(&prev.token) {
                break; // space before the dot: not a qualifier
            }
            let seg = &self.tokens[self.pos + 1];
            let seg_name = match &seg.token {
                Token::UpperIdent(n) => n.clone(),
                _ => break,
            };
            if seg.line != dot.line || seg.col != dot.col + 1 {
                break; // space after the dot
            }
            self.advance(); // consume '.'
            self.advance(); // consume the segment
            name.push('.');
            name.push_str(&seg_name);
            matched = true;
        }
        if matched { Some(name) } else { None }
    }

    /// Parse the FFI target string of `LuaPure "…"`, `LuaIO "…"`,
    /// `LuaIterator "…"`, `LuaTry "…"`, `LuaCatch "…"`, `LuaIOCatch "…"`.
    ///
    /// The string is emitted VERBATIM as the callee of a Lua call, so it must
    /// be a well-formed Lua callee expression — otherwise the compiler would
    /// silently produce a .lua file that Lua refuses to load. Validating here,
    /// at the declaration, gives one early error that covers every FFI form.
    fn parse_ffi_lua_name(&mut self, kw: &str) -> PResult<String> {
        let lua_name = match self.peek().clone() {
            Token::StrLit(s) => { self.advance(); self.strlit_as_symbol(s, "an FFI Lua name")? }
            _ => return Err(self.err_here(format!("{} expects a string literal", kw))),
        };
        if let Err(why) = validate_ffi_callee(&lua_name) {
            let mut diag = self.err_here(format!(
                "invalid Lua target in `{} \"{}\"`: {}. The string is emitted \
                 verbatim as the thing being called in the generated Lua, so it \
                 must be a well-formed Lua callee",
                kw, lua_name, why
            ));
            diag.notes.push(
                "valid forms: a bare name (`floor`), a dotted path (`math.floor`), \
                 an indexed path (`handlers[1].run`, `t[\"key\"].f`), any of those \
                 with a trailing method (`obj.stream:read`), or a bare method \
                 (`:read`) applied to the function's first argument"
                    .to_string(),
            );
            return Err(diag);
        }
        Ok(lua_name)
    }

    // Depth-guard wrapper: the grammar rule itself is in `parse_type_atom_inner`.
    fn parse_type_atom(&mut self) -> PResult<Type> {
        self.guarded("type", Self::parse_type_atom_inner)
    }

    fn parse_type_atom_inner(&mut self) -> PResult<Type> {
        match self.peek().clone() {
            Token::UpperIdent(name) => {
                self.advance();
                // Module-qualified type reference: `M.Map`, `Data.Map.Map`.
                if let Some(qual) = self.try_parse_qualified_tail(&name) {
                    return Ok(Type::Con(qual));
                }
                match name.as_str() {
                    "IO" => {
                        if self.is_type_atom_start() {
                            let inner = self.parse_type_atom()?;
                            Ok(Type::IO(Box::new(inner)))
                        } else {
                            Ok(Type::Con(name))
                        }
                    }
                    "LuaIO" if !matches!(self.peek(), Token::StrLit(_)) => {
                        // LuaIO s a — scoped Lua IO monad (not the FFI type family)
                        let scope_var = self.expect_ident()?;
                        let inner = self.parse_type_atom()?;
                        Ok(Type::ScopedLuaIO {
                            scope_var,
                            inner: Box::new(inner),
                        })
                    }
                    "LuaPure" => {
                        // LuaPure "lua.func.name" ReturnType
                        let lua_name = self.parse_ffi_lua_name("LuaPure")?;
                        let result = self.parse_type_atom()?;
                        Ok(Type::LuaPure { lua_name, result: Box::new(result) })
                    }
                    "LuaIO" => {
                        // LuaIO "lua.func.name" ReturnType
                        let lua_name = self.parse_ffi_lua_name("LuaIO")?;
                        let result = self.parse_type_atom()?;
                        Ok(Type::LuaIO { lua_name, result: Box::new(result) })
                    }
                    "LuaIterator" => {
                        // LuaIterator "lua.func.name" [E]  ->  [E], yielding one
                        // E per step. The result must be written as an explicit
                        // list: the iterator is collected into a list, so the
                        // type argument always names that list.
                        let lua_name = self.parse_ffi_lua_name("LuaIterator")?;
                        let result = self.parse_type_atom()?;
                        if !matches!(result, Type::List(_)) {
                            return Err(self.err_here(
                                "LuaIterator requires the result to be written as an explicit \
                                 list `[E]`: the iterator is collected into a list that yields \
                                 one `E` per step (e.g. `LuaIterator \"string.gmatch\" [String]`, \
                                 not `... String`)"
                                    .to_string(),
                            ));
                        }
                        Ok(Type::LuaIterator { lua_name, result: Box::new(result) })
                    }
                    "LuaTry" => {
                        // LuaTry "lua.func.name" (Either String T)  ->  IO (Either String T)
                        // A Lua `(val, err)` failure return (including a bare nil
                        // value) is captured as `Left err`.
                        let lua_name = self.parse_ffi_lua_name("LuaTry")?;
                        let result = self.parse_type_atom()?;
                        if !is_either_string_type(&result) {
                            return Err(self.err_here(
                                "LuaTry requires the result to be written as `(Either String a)`, \
                                 so a Lua `(val, err)` failure can be returned as `Left`"
                                    .to_string(),
                            ));
                        }
                        Ok(Type::LuaTry { lua_name, result: Box::new(result) })
                    }
                    "LuaCatch" | "LuaIOCatch" => {
                        // LuaCatch    "lua.func.name" (Either String T)  ->  Either String T
                        // LuaIOCatch  "lua.func.name" (Either String T)  ->  IO (Either String T)
                        // A raised Lua error is captured as `Left msg` via pcall.
                        let lua_name = self.parse_ffi_lua_name(&name)?;
                        let result = self.parse_type_atom()?;
                        if !is_either_string_type(&result) {
                            return Err(self.err_here(format!(
                                "{} requires the result to be written as `(Either String a)`, \
                                 so a raised Lua `error(...)` can be returned as `Left`",
                                name
                            )));
                        }
                        if name == "LuaCatch" {
                            Ok(Type::LuaCatch { lua_name, result: Box::new(result) })
                        } else {
                            Ok(Type::LuaIOCatch { lua_name, result: Box::new(result) })
                        }
                    }
                    _ => Ok(Type::Con(name)),
                }
            }
            Token::Ident(name) => {
                self.advance();
                Ok(Type::Var(name))
            }
            Token::LeftParen => {
                self.advance();
                if self.at(&Token::RightParen) {
                    self.advance();
                    return Ok(Type::Unit);
                }
                if let Token::Operator(op) = self.peek().clone() {
                    // A type operator in prefix form, `(:+:) f g` — the same
                    // constructor the infix `f :+: g` spelling names (see
                    // is_type_operator), exactly as `data (:+:) a b` declares it.
                    if Self::is_type_operator(&op)
                        && self.tokens.get(self.pos + 1).is_some_and(|t| t.token == Token::RightParen)
                    {
                        self.advance();
                        self.advance();
                        return Ok(Type::Con(op));
                    }
                    // Any other operator in type position, e.g. `f :: (+) -> Int`.
                    // This used to be silently parsed as the unit type, so the
                    // program compiled with a signature that meant something
                    // entirely different from what was written — reject it with
                    // an explanation instead.
                    let mut diag = self.err_here(format!(
                        "The operator '{}' cannot appear in a type: '({})' names a \
                         function (a value), and a type must be built from type names, \
                         type variables, lists, tuples, and '->'",
                        op, op
                    ));
                    diag.notes.push(
                        "GHC can accept any operator in a type with the TypeOperators \
                         extension; in mata-ll only ':'-leading operators such as ':+:' \
                         are type operators (they name type constructors, as ':'-leading \
                         value operators name data constructors), so a value operator \
                         is always an error here"
                            .to_string(),
                    );
                    return Err(diag);
                }
                let ty = self.parse_type()?;
                if self.at(&Token::Comma) {
                    // Tuple type: (a, b, ...)
                    let mut elems = vec![ty];
                    while self.at(&Token::Comma) {
                        self.advance();
                        elems.push(self.parse_type()?);
                    }
                    self.expect(&Token::RightParen)?;
                    Ok(Type::Tuple(elems))
                } else {
                    self.expect(&Token::RightParen)?;
                    Ok(Type::Paren(Box::new(ty)))
                }
            }
            Token::LeftBracket => {
                self.advance();
                // The bare, UNAPPLIED list constructor `[]` (kind
                // Type -> Type), for positions that need the constructor
                // itself rather than a list of something — chiefly instance
                // heads: `instance Foldable []`. The typechecker registers
                // the list constructor as `Con "[]"`, so this wires straight
                // into instance resolution (InstHead::List). `[a]` below
                // stays the ordinary applied list type.
                if self.at(&Token::RightBracket) {
                    self.advance();
                    return Ok(Type::Con("[]".to_string()));
                }
                let inner = self.parse_type()?;
                self.expect(&Token::RightBracket)?;
                Ok(Type::List(Box::new(inner)))
            }
            Token::Tick => {
                // Promoted data constructor (DataKinds): 'Empty, 'NonEmpty
                self.advance();
                let name = self.expect_upper_ident()?;
                Ok(Type::Promoted(name))
            }
            Token::StrLit(s) => {
                // Type-level string literal (Symbol kind)
                self.advance();
                let sym = self.strlit_as_symbol(s, "a type-level Symbol")?;
                Ok(Type::Con(format!("\"{}\"", sym)))
            }
            _ => {
                Err(self.err_here(format!("Expected type, found {}", self.peek())))
            }
        }
    }

    // --- Expression parsing ---

    /// Parse a statement-boundary expression — a do-statement, a let/where
    /// binding body, a case-branch or guard body — and wrap it in an
    /// `Expr::Spanned` marker carrying the line it starts on. The marker is
    /// transparent to every later pass but lets the type checker report an
    /// error against the offending statement's line rather than the clause
    /// head. Skips leading layout first so the span is the real first token.
    fn parse_stmt_expr(&mut self) -> PResult<Expr> {
        self.skip_newlines_and_indent();
        let loc = self.peek_loc();
        let span = Span::new(loc.line, loc.col);
        let e = self.parse_expr()?;
        Ok(Expr::Spanned(span, Box::new(e)))
    }

    fn parse_expr(&mut self) -> PResult<Expr> {
        // Skip leading indent/newlines to find the actual expression start
        self.skip_newlines_and_indent();
        let saved_expr_min_indent = self.expr_min_indent;
        self.expr_min_indent = self.current_indent;
        let expr = self.parse_expr_infix(0, None)?;
        self.expr_min_indent = saved_expr_min_indent;

        // Type ascription: expr :: Type
        if self.at(&Token::DblColon) {
            self.advance();
            let ty = self.parse_type()?;
            return Ok(Expr::Ascription(Box::new(expr), ty));
        }

        Ok(expr)
    }

    // Depth-guard wrapper: the grammar rule itself is in `parse_expr_infix_inner`.
    fn parse_expr_infix(&mut self, min_prec: u8, parent: Option<&ParentOp>) -> PResult<Expr> {
        self.guarded("expression", |p| p.parse_expr_infix_inner(min_prec, parent))
    }

    fn parse_expr_infix_inner(&mut self, min_prec: u8, parent: Option<&ParentOp>) -> PResult<Expr> {
        // `parent` reaches the prefix parser so prefix minus can be rejected
        // in the right operand of a precedence >= 6 operator (GHC's rule).
        let lhs = self.parse_expr_prefix(parent)?;
        self.continue_infix(lhs, min_prec, parent)
    }

    /// Decide what to do with the operator the parser is looking at, given
    /// the operator whose right-hand side is being parsed (`parent`) and the
    /// Pratt minimum binding power. `Ok(true)` = this call consumes it,
    /// `Ok(false)` = it belongs to an enclosing call, `Err` = the two
    /// operators share a precedence but define no grouping (Gap: the GHC
    /// precedence-parsing rule).
    fn infix_should_consume(
        &self,
        parent: Option<&ParentOp>,
        min_prec: u8,
        op: &str,
        assoc: Assoc,
        prec: u8,
    ) -> PResult<bool> {
        if let Some(par) = parent
            && par.prec == prec
        {
            // Same precedence directly under `parent`: Haskell defines a
            // grouping only for infixl/infixl (the enclosing loop takes it)
            // and infixr/infixr (this call takes it, nesting rightward).
            return match (par.assoc, assoc) {
                (Assoc::Left, Assoc::Left) => Ok(false),
                (Assoc::Right, Assoc::Right) => Ok(true),
                _ => Err(self.fixity_conflict_err(par, op, assoc, prec)),
            };
        }
        let (lp, _) = assoc_prec_to_binding(assoc, prec);
        Ok(lp >= min_prec)
    }

    /// Continue infix-operator parsing from an already-parsed left operand.
    /// Splitting this out of `parse_expr_infix` lets callers that have already
    /// parsed an application (e.g. the parenthesised-expression path, which
    /// parses one to test for a left section) resume without re-parsing it —
    /// the parenthesised body would otherwise be parsed twice at every nesting
    /// level, giving O(2^n) parse time on deeply nested parentheses.
    fn continue_infix(&mut self, mut lhs: Expr, min_prec: u8, parent: Option<&ParentOp>) -> PResult<Expr> {
        // A bare Negate here is a prefix-minus expression fresh from
        // `parse_negation` (a parenthesized one arrives wrapped in Paren).
        // As Haskell's `lexp6` it may only continue with operators of
        // precedence < 6 or LEFT-associative precedence-6 ones
        // (`-a + b` groups `(negate a) + b`); a precedence-6 operator with
        // any other associativity has no defined grouping against it —
        // GHC rejects `-a <> b`. Cleared after the first consumed operator:
        // the negation is nested inside an InfixApp from then on.
        let mut lhs_is_negation = matches!(lhs, Expr::Negate(_));
        loop {
            // Try to consume indentation for continuation lines
            // Only if the next real token after indent is an operator and
            // the indent is STRICTLY deeper than the enclosing layout
            // block's item column: at exactly that column GHC's layout
            // inserts a `;` — the line is the block's NEXT item (`x = 1`
            // then `+ 2` at x's own column is a parse error in GHC, and
            // used to be silently accepted as a continuation here, keyed
            // to the weaker `>= expr_min_indent`). Deeper than the block
            // column is a continuation even when it equals the
            // expression-start line's own indent (`( 40` / `+ 2` aligned
            // inside a let binding), which is why the block column — not
            // the expression start — is the base.
            if let Token::Indent(n) = self.peek() {
                let n = *n;
                if n > self.block_indent {
                    let save = self.checkpoint();
                    self.advance(); // consume indent
                    self.current_indent = n;
                    // Check if next token is an operator (continuation)
                    if !matches!(self.peek(), Token::Operator(_) | Token::Backtick) {
                        // Not a continuation — put it back
                        self.rewind(save);
                    }
                }
            }

            // Check for operator
            match self.peek().clone() {
                Token::Operator(ref op) if op == ".." => {
                    break; // '..' is range syntax, not an infix operator
                }
                Token::Operator(ref op) => {
                    // An operator directly followed by ')' is a left-section
                    // tail (`(a * b +)`): it belongs to the enclosing
                    // parenthesised-expression path, which builds the section
                    // and checks its operand against the section-operand
                    // precedence rule. Never consume it as an infix operator —
                    // there is no expression after it to parse.
                    if self.pos + 1 < self.tokens.len()
                        && self.tokens[self.pos + 1].token == Token::RightParen
                    {
                        break;
                    }
                    let (assoc, prec) = self.operator_fixity(op);
                    if lhs_is_negation && prec >= 6 && !(prec == 6 && assoc == Assoc::Left) {
                        return Err(self.prefix_minus_lhs_err(op, assoc, prec));
                    }
                    if !self.infix_should_consume(parent, min_prec, op, assoc, prec)? {
                        break;
                    }
                    let op = op.clone();
                    self.advance();
                    self.skip_newlines_and_indent();
                    let (_, rp) = assoc_prec_to_binding(assoc, prec);
                    let this = ParentOp { op: op.clone(), prec, assoc };
                    let rhs = self.parse_expr_infix(rp, Some(&this))?;
                    lhs = Expr::InfixApp {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                    lhs_is_negation = false;
                }
                Token::Backtick => {
                    let save = self.checkpoint();
                    self.advance();
                    let func = self.expect_ident()?;
                    self.expect(&Token::Backtick)?;
                    // A backtick operator directly followed by ')' is a
                    // left-section tail (``(a * b `div`)``) — same stop as
                    // the symbolic-operator arm above.
                    if self.at(&Token::RightParen) {
                        self.rewind(save);
                        break;
                    }
                    let (assoc, prec) = self.operator_fixity(&func);
                    if lhs_is_negation && prec >= 6 && !(prec == 6 && assoc == Assoc::Left) {
                        return Err(self.prefix_minus_lhs_err(&func, assoc, prec));
                    }
                    if !self.infix_should_consume(parent, min_prec, &func, assoc, prec)? {
                        // The operator belongs to an enclosing call — rewind
                        // past the backticks so it can consume them itself.
                        self.rewind(save);
                        break;
                    }
                    self.skip_newlines_and_indent();
                    let (_, rp) = assoc_prec_to_binding(assoc, prec);
                    let this = ParentOp { op: func.clone(), prec, assoc };
                    let rhs = self.parse_expr_infix(rp, Some(&this))?;
                    lhs = Expr::InfixApp {
                        op: func,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                    lhs_is_negation = false;
                }
                _ => break,
            }
        }

        Ok(lhs)
    }

    fn parse_expr_prefix(&mut self, parent: Option<&ParentOp>) -> PResult<Expr> {
        // Prefix minus (negation). Haskell gives it the fixity of binary
        // subtraction — infixl 6 — with two consequences the grammar
        // (`lexp6 -> - exp7`) enforces and GHC implements exactly:
        //   * it cannot be the right operand of any operator at precedence 6
        //     or higher (`a + -b`, `a - -b`, `a * -b` are parse errors —
        //     the expression has no defined grouping without parentheses);
        //   * its operand is everything binding TIGHTER than 6, so
        //     `-a * b` is `negate (a * b)`, while `-a + b` is
        //     `negate a + b` (the `+` stops the operand).
        if let Token::Operator(ref op) = self.peek().clone()
            && op == "-" {
                if let Some(par) = parent
                    && par.prec >= 6
                {
                    return Err(self.prefix_minus_rhs_err(par));
                }
                return self.parse_negation();
            }
        self.parse_expr_app()
    }

    /// Parse `- <operand>` where `-` has already been checked to be legal
    /// here. The operand is Haskell's `exp7`: an application followed by any
    /// operators of precedence 7 and higher (`NEGATION_OPERAND_MIN_PREC`),
    /// so `- a * b` reads the whole `a * b` and `- a + b` stops at `a`.
    fn parse_negation(&mut self) -> PResult<Expr> {
        self.advance(); // consume '-'
        let operand = self.parse_expr_app()?;
        let operand = self.continue_infix(operand, NEGATION_OPERAND_MIN_PREC, None)?;
        Ok(Expr::Negate(Box::new(operand)))
    }

    /// The error for prefix minus in the right operand of a precedence >= 6
    /// operator, matching GHC's rejection (GHC: "cannot mix ... and prefix
    /// minus in the same infix expression").
    /// The operand of a right section, parsed at the `infixexp` level.
    /// Haskell 2010 puts `::` one grammar level HIGHER (exp → infixexp
    /// [:: type]), so a section operand can never carry an ascription —
    /// GHC parse-errors on `(+ 1 :: Int)` — and this parser now agrees,
    /// with the reason spelled out. (`parse_expr` would have consumed the
    /// `::`, which is exactly how the form was accepted before.) Left
    /// sections cannot reach the shape: their operand parse stops in
    /// front of `::`, so the section test never sees a trailing operator.
    /// `op_shown` is the operator as the user wrote it (backticks
    /// included), for the error's concrete rewrite hint.
    fn parse_right_section_operand(&mut self, op_shown: &str) -> PResult<Expr> {
        // Mirrors `parse_expr` (leading layout skip, `expr_min_indent`)
        // minus its ascription tail.
        self.skip_newlines_and_indent();
        let saved_expr_min_indent = self.expr_min_indent;
        self.expr_min_indent = self.current_indent;
        let expr = self.parse_expr_infix(0, None)?;
        self.expr_min_indent = saved_expr_min_indent;
        if self.at(&Token::DblColon) {
            let loc = self.peek_loc();
            let mut diag = Diagnostic::parse_at(
                format!(
                    "A section operand cannot carry a '::' type annotation: \
                     '::' annotates a complete expression, and a section \
                     operand is an operator argument one grammar level below \
                     that, so '({op_shown} e :: T)' does not parse"
                ),
                Span::new(loc.line, loc.col),
            );
            diag.notes.push(format!(
                "parenthesize the annotated operand: '({op_shown} (e :: T))'"
            ));
            return Err(Box::new(diag));
        }
        Ok(expr)
    }

    fn prefix_minus_rhs_err(&self, parent: &ParentOp) -> Box<Diagnostic> {
        let d = op_display(&parent.op);
        let e = op_in_expr(&parent.op);
        let msg = format!(
            "Prefix minus cannot be the right operand of {d} ({} {}): \
             prefix minus binds like binary '-' (precedence 6), so \
             'a {e} -b' has no defined grouping",
            assoc_keyword(parent.assoc),
            parent.prec
        );
        let loc = self.peek_loc();
        let mut diag = Diagnostic::parse_at(msg, Span::new(loc.line, loc.col));
        diag.notes.push(format!("parenthesize the negation: 'a {e} (-b)'"));
        Box::new(diag)
    }

    /// The error for an operator that can neither take a prefix-minus
    /// expression as its LEFT operand nor be part of its operand: a
    /// precedence-6 operator that is not left-associative (`-a <> b`), or —
    /// defensively — anything tighter that escaped the operand parse.
    fn prefix_minus_lhs_err(&self, op: &str, assoc: Assoc, prec: u8) -> Box<Diagnostic> {
        let d = op_display(op);
        let e = op_in_expr(op);
        let msg = format!(
            "Cannot mix prefix minus and {d} ({} {prec}): prefix minus \
             binds like binary '-' (infixl 6), so '-a {e} b' has no \
             defined grouping",
            assoc_keyword(assoc)
        );
        let loc = self.peek_loc();
        let mut diag = Diagnostic::parse_at(msg, Span::new(loc.line, loc.col));
        diag.notes.push(format!(
            "parenthesize one side: '(-a) {e} b' or '-(a {e} b)'"
        ));
        Box::new(diag)
    }

    /// Enforce the section-operand precedence rule (Haskell 2010 §3.5, the
    /// check GHC runs on every section): a section operand that is itself an
    /// infix expression must bind tighter than the section operator, because
    /// a section may only mean what its expansion means — `(== a || b)`
    /// would have to be `\x -> x == (a || b)`, but `x == a || b` groups as
    /// `(x == a) || b`, so the section is rejected rather than silently
    /// regrouped. One same-precedence shape is well-defined and stays legal:
    /// a chain that groups in the section's own direction — an infixl
    /// operand in a left section (`(a + b +)` is `\x -> (a + b) + x`) and an
    /// infixr operand in a right section (`(++ a ++ b)` is
    /// `\x -> x ++ (a ++ b)`). Prefix minus counts as an infixl 6 operand,
    /// exactly as in GHC. `direction` is `Assoc::Left` for a left section
    /// `(e op)` and `Assoc::Right` for a right section `(op e)`; `span` is
    /// the section operator's position.
    fn check_section_operand(
        &self,
        op: &str,
        op_assoc: Assoc,
        op_prec: u8,
        direction: Assoc,
        operand: &Expr,
        span: Span,
    ) -> PResult<()> {
        let (arg_assoc, arg_prec, negation, top_op) = match operand {
            Expr::InfixApp { op: top, .. } => {
                let (a, p) = self.operator_fixity(top);
                (a, p, false, top.clone())
            }
            Expr::Negate(_) => (Assoc::Left, 6, true, "-".to_string()),
            _ => return Ok(()),
        };
        if op_prec < arg_prec || (op_prec == arg_prec && direction == arg_assoc) {
            return Ok(());
        }

        // Build the schematic pieces of the message from the operators as
        // written: the operand shape, the section as the user wrote it, the
        // expansion it would have to mean, and how the unparenthesized
        // expansion groups (if it groups at all).
        let d = op_display(op);
        let e = op_in_expr(op);
        let kw = assoc_keyword(op_assoc);
        let side = if direction == Assoc::Left { "left" } else { "right" };
        let rel = if arg_prec < op_prec { "looser than" } else { "at the same precedence as" };
        let top_desc = if negation {
            "prefix minus (which binds like binary '-', infixl 6)".to_string()
        } else {
            format!(
                "{} ({} {arg_prec})",
                op_display(&top_op),
                assoc_keyword(arg_assoc)
            )
        };
        let operand_src = if negation {
            "-a".to_string()
        } else {
            format!("a {} b", op_in_expr(&top_op))
        };
        let (section_src, expansion, bare) = match direction {
            Assoc::Left => (
                format!("({operand_src} {e})"),
                format!("\\x -> ({operand_src}) {e} x"),
                format!("{operand_src} {e} x"),
            ),
            _ => (
                format!("({e} {operand_src})"),
                format!("\\x -> x {e} ({operand_src})"),
                format!("x {e} {operand_src}"),
            ),
        };
        // The unparenthesized expansion groups with the section operator
        // applied first when the operand's operator binds looser; at equal
        // precedence it only groups when both operators chain in the same
        // (other) direction. A rejected negation operand binds looser than
        // the section operator, and negation of the rest is the grouping.
        let regrouped = if negation {
            match direction {
                Assoc::Left => Some(format!("-(a {e} x)")),
                _ => None,
            }
        } else {
            let e2 = op_in_expr(&top_op);
            let groups = arg_prec < op_prec
                || (arg_assoc == op_assoc && arg_assoc != Assoc::None);
            groups.then(|| match direction {
                Assoc::Left => format!("a {e2} (b {e} x)"),
                _ => format!("(x {e} a) {e2} b"),
            })
        };
        let grouping = match regrouped {
            Some(g) => format!("'{bare}' groups as '{g}'"),
            None => format!("'{bare}' has no defined grouping"),
        };
        let msg = format!(
            "The operand of a {side} section must bind tighter than the \
             section operator, but {top_desc} binds {rel} {d} \
             ({kw} {op_prec}): '{section_src}' cannot mean '{expansion}', \
             because {grouping}"
        );
        let mut diag = Diagnostic::parse_at(msg, span);
        let fix = match direction {
            Assoc::Left => format!("(({operand_src}) {e})"),
            _ => format!("({e} ({operand_src}))"),
        };
        diag.notes.push(format!(
            "parenthesize the operand to get that meaning: '{fix}'"
        ));
        Err(Box::new(diag))
    }

    fn parse_expr_app(&mut self) -> PResult<Expr> {
        let mut func = self.parse_expr_atom_dotted()?;

        loop {
            // Same-line arguments (existing behavior)
            if self.is_expr_atom_start_in_context() {
                let arg = self.parse_expr_atom_dotted()?;
                func = Expr::App(Box::new(func), Box::new(arg));
                continue;
            }

            // Cross-line continuation: a line indented strictly past the
            // current layout block's column is a continuation (more arguments),
            // not a new item. This is the Haskell layout rule (deeper than the
            // block = continuation; at the block column = next clause/binding/
            // statement). The block-column check alone keeps siblings from being
            // grabbed, so this works even for a function whose first argument is
            // on the next line (e.g. inside explicit brackets).
            if matches!(self.peek(), Token::Newline | Token::Indent(_)) {
                let save_pos = self.checkpoint();
                self.skip_newlines_and_indent();
                if self.current_indent > self.block_indent
                    && self.is_expr_atom_start()
                {
                    let arg = self.parse_expr_atom_dotted()?;
                    func = Expr::App(Box::new(func), Box::new(arg));
                    continue;
                }
                // Not a continuation — backtrack
                self.rewind(save_pos);
            }

            break;
        }

        Ok(func)
    }

    /// Parse the binding group after `let` — the `let` keyword already
    /// consumed — up to but NOT consuming the following `in` (a let-expression)
    /// or `,`/`]` (a list-comprehension `let` qualifier). Bindings are
    /// layout-separated exactly as in a let-expression: simple `x = e`,
    /// function `f x = e` (desugared to `x = \... -> e`), and tuple-pattern
    /// `(a, b) = e` binds, all in one mutually recursive group. Shared by the
    /// let-expression atom and the comprehension qualifier so they bind
    /// identically.
    fn parse_let_binds(&mut self) -> PResult<Vec<LocalDef>> {
        let mut binds = Vec::new();
        // The first binding's column is the group's layout block: a later
        // line at that column is the next binding, a line indented less
        // closes the group (so a do-block's next statement, at the `let`
        // line's own indent, is never read as a binding).
        let saved_block = self.block_indent;
        let let_indent = self.open_item_block();
        // Tuple pattern binds: (fresh_name, pattern) pairs to wrap body in case
        let mut fresh_counter = 0usize;

        loop {
            self.skip_newlines_and_indent();
            if self.at_eof() || self.current_indent < let_indent {
                break;
            }
            if self.at(&Token::In) {
                break;
            }
            // Tuple pattern: let (a, b) = expr. Desugared into the
            // SAME recursive binding group as one fresh binding for
            // the scrutinee plus one lazy SELECTOR binding per
            // pattern variable (`a = case __tup of (a, b) -> a`), so
            // — as in Haskell — the pattern's variables are in scope
            // for the right-hand side itself, for sibling bindings,
            // and the match happens lazily on first demand (never
            // eagerly, the way wrapping the body in a case would).
            if matches!(self.peek(), Token::LeftParen) {
                let pat = self.parse_pattern_atom()?;
                if matches!(pat, Pattern::Tuple(_)) {
                    self.expect(&Token::Eq)?;
                    let rhs = self.parse_expr()?;
                    let fresh = format!("__tup_{}", fresh_counter);
                    fresh_counter += 1;
                    binds.push(LocalDef { name: fresh.clone(), patterns: vec![], body: rhs });
                    for v in pat.var_names() {
                        binds.push(LocalDef {
                            name: v.clone(),
                            patterns: vec![],
                            body: Expr::Case {
                                scrutinee: Box::new(Expr::Var(fresh.clone())),
                                branches: vec![CaseBranch {
                                    pattern: pat.clone(),
                                    guards: vec![],
                                    body: Some(Expr::Var(v)),
                                }],
                            },
                        });
                    }
                    continue;
                }
                return Err(self.err_here("Expected tuple pattern or identifier in let binding".to_string()));
            }
            if !matches!(self.peek(), Token::Ident(_)) {
                break;
            }
            let (name, patterns) = self.parse_binding_head()?;
            self.expect(&Token::Eq)?;
            let body = self.parse_stmt_expr()?;
            binds.push(self.group_binding(name, patterns, body)?);
        }

        self.skip_newlines_and_indent();
        self.block_indent = saved_block;
        Ok(binds)
    }

    /// Parse list comprehension qualifiers: `x <- xs, pred, y <- ys, …`.
    /// Supports pattern-matching generators (`Ok x <- rs`, `(a, b) <- pairs`)
    /// and `let` qualifiers.
    fn parse_list_comprehension_quals(&mut self) -> PResult<Vec<ListCompQual>> {
        let mut quals = Vec::new();
        loop {
            self.skip_newlines_and_indent();
            // `let decls` qualifier: bindings visible in the body and every
            // later qualifier. Desugars to `let decls in <rest>`.
            if self.at(&Token::Let) {
                self.advance();
                let binds = self.parse_let_binds()?;
                quals.push(ListCompQual::Let(binds));
                self.skip_newlines_and_indent();
                if self.at(&Token::Comma) { self.advance(); continue; }
                break;
            }
            // Try generator: pattern <- expr
            let save = self.checkpoint();
            if self.is_pattern_start() {
                if let Ok(pat) = self.parse_pattern()
                    && self.at(&Token::Bind) {
                        self.advance();
                        let expr = self.parse_expr()?;
                        quals.push(ListCompQual::Generator { pattern: pat, expr });
                        self.skip_newlines_and_indent();
                        if self.at(&Token::Comma) { self.advance(); continue; }
                        break;
                    }
                // Not a generator — backtrack and parse as guard
                self.rewind(save);
            }
            // Guard expression
            let expr = self.parse_expr()?;
            quals.push(ListCompQual::Guard(expr));
            self.skip_newlines_and_indent();
            if self.at(&Token::Comma) { self.advance(); continue; }
            break;
        }
        Ok(quals)
    }

    /// Desugar [expr | quals] into concatMap / if chains
    /// [e | x <- xs, rest]    => concatMap (\x -> [e | rest]) xs
    /// [e | Pat <- xs, rest]  => concatMap (\v -> case v of { Pat -> [e | rest]; _ -> [] }) xs
    /// [e | pred, rest]       => if pred then [e | rest] else []
    /// [e]                    => [e] (singleton)
    fn desugar_list_comprehension(&self, body: Expr, quals: &[ListCompQual], counter: &mut usize) -> Expr {
        if quals.is_empty() {
            // Singleton list: [body]
            return Expr::App(
                Box::new(Expr::App(
                    Box::new(Expr::Con(":".to_string())),
                    Box::new(body),
                )),
                Box::new(Expr::Con("[]".to_string())),
            );
        }
        match &quals[0] {
            ListCompQual::Generator { pattern, expr } => {
                let rest = self.desugar_list_comprehension(body, &quals[1..], counter);
                match pattern {
                    // Simple variable: concatMap (\name -> rest) expr
                    Pattern::Var(name) => {
                        Expr::App(
                            Box::new(Expr::App(
                                Box::new(Expr::Var("concatMap".to_string())),
                                Box::new(Expr::Lambda {
                                    params: vec![name.clone()],
                                    body: Box::new(rest),
                                }),
                            )),
                            Box::new(expr.clone()),
                        )
                    }
                    // Pattern: concatMap (\v -> case v of { pat -> rest; _ -> [] }) expr
                    pat => {
                        let var_name = format!("__comp{}", counter);
                        *counter += 1;
                        let case_expr = Expr::Case {
                            scrutinee: Box::new(Expr::Var(var_name.clone())),
                            branches: vec![
                                CaseBranch {
                                    pattern: pat.clone(),
                                    guards: vec![],
                                    body: Some(rest),
                                },
                                CaseBranch {
                                    pattern: Pattern::Wildcard,
                                    guards: vec![],
                                    body: Some(Expr::Con("[]".to_string())),
                                },
                            ],
                        };
                        Expr::App(
                            Box::new(Expr::App(
                                Box::new(Expr::Var("concatMap".to_string())),
                                Box::new(Expr::Lambda {
                                    params: vec![var_name],
                                    body: Box::new(case_expr),
                                }),
                            )),
                            Box::new(expr.clone()),
                        )
                    }
                }
            }
            ListCompQual::Guard(pred) => {
                // if pred then [body | rest] else []
                let rest = self.desugar_list_comprehension(body, &quals[1..], counter);
                Expr::If {
                    cond: Box::new(pred.clone()),
                    then_branch: Box::new(rest),
                    else_branch: Box::new(Expr::Con("[]".to_string())),
                }
            }
            ListCompQual::Let(binds) => {
                // let decls in [body | rest] — bindings scope over the rest.
                let rest = self.desugar_list_comprehension(body, &quals[1..], counter);
                Expr::Let {
                    binds: binds.clone(),
                    body: Box::new(rest),
                }
            }
        }
    }

    /// Parse an atom optionally followed by one or more `.field` accesses.
    /// `expr.field` desugars to `(field expr)`. Only applies when the `.` is
    /// adjacent to the preceding token (no space) and followed by an
    /// identifier, to distinguish it from function composition `f . g`.
    fn parse_expr_atom_dotted(&mut self) -> PResult<Expr> {
        let mut expr = self.parse_expr_atom()?;

        while self.at(&Token::Operator(".".to_string())) {
            // Check adjacency: the '.' must be on the same line as the
            // previous token and immediately follow it (no whitespace).
            let prev_tok = &self.tokens[self.pos - 1];
            let dot_tok = &self.tokens[self.pos];
            if dot_tok.line != prev_tok.line {
                break;
            }
            // Estimate end column of previous token
            let prev_end = prev_tok.col + token_len(&prev_tok.token);
            if dot_tok.col != prev_end {
                break; // there's a gap — this is composition, not field access
            }
            if self.pos + 1 < self.tokens.len()
                && let Token::Ident(_) = &self.tokens[self.pos + 1].token {
                    // The identifier must hug the dot on the RIGHT too
                    // (OverloadedRecordDot's rule): `negate. abs` is the
                    // composition `negate . abs`, not the field access
                    // `abs negate` it used to silently parse as.
                    let ident_tok = &self.tokens[self.pos + 1];
                    if ident_tok.line != dot_tok.line || ident_tok.col != dot_tok.col + 1 {
                        break;
                    }
                    self.advance(); // consume '.'
                    if let Token::Ident(field) = self.peek().clone() {
                        self.advance(); // consume field name
                        expr = Expr::App(Box::new(Expr::Var(field)), Box::new(expr));
                        continue;
                    }
                }
            break;
        }

        // Record update: expr { field = val, ... }
        // Loop to allow chained updates: expr { x = 1 } { y = 2 }.
        // The brace attaches on the same line, or from a following line
        // indented strictly past the current layout block's column (the
        // cross-line continuation rule from parse_expr_app) — a brace at or
        // left of the block column belongs to a sibling item (a do-statement,
        // the next binding), never to this expression.
        loop {
            let save_pos = self.checkpoint();
            if matches!(self.peek(), Token::Newline | Token::Indent(_)) {
                self.skip_newlines_and_indent();
                if !(self.at(&Token::LeftBrace)
                    && self.current_indent > self.block_indent)
                {
                    self.rewind(save_pos);
                    break;
                }
            } else {
                if !(self.at(&Token::LeftBrace) && self.pos > 0) {
                    break;
                }
                let prev_tok = &self.tokens[self.pos - 1];
                let brace_tok = &self.tokens[self.pos];
                if brace_tok.line != prev_tok.line {
                    break;
                }
            }
            if let Ok(updates) = self.try_parse_record_update() {
                expr = Expr::RecordUpdate {
                    expr: Box::new(expr),
                    updates,
                };
            } else {
                self.rewind(save_pos);
                break;
            }
        }

        Ok(expr)
    }

    fn try_parse_record_update(&mut self) -> PResult<Vec<(String, Expr)>> {
        self.expect(&Token::LeftBrace)?;
        let mut updates = Vec::new();
        loop {
            self.skip_newlines_and_indent();
            if self.at(&Token::RightBrace) {
                break;
            }
            let field_name = match self.peek().clone() {
                Token::Ident(n) => { self.advance(); n }
                _ => return Err(self.err_here("Expected field name".to_string())),
            };
            self.expect(&Token::Eq)?;
            let value = self.parse_expr()?;
            updates.push((field_name, value));
            self.skip_newlines_and_indent();
            if self.at(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        if updates.is_empty() {
            return Err(self.err_here("Empty record update".to_string()));
        }
        self.expect(&Token::RightBrace)?;
        Ok(updates)
    }

    /// Check if the next token could start an expression atom,
    /// respecting indentation context. Stops when we've returned to
    /// a line at or below the expression's starting indentation.
    fn is_expr_atom_start_in_context(&self) -> bool {
        // If there's an Indent token next, check indentation
        if let Token::Indent(n) = self.peek()
            && *n <= self.expr_min_indent {
                return false;
            }
        if !self.is_expr_atom_start() {
            return false;
        }
        let loc = self.peek_loc();
        // If current_indent is at or below expr start and the token is at
        // the beginning of its line (col == current_indent + 1), it's a new
        // statement/declaration, not a continuation argument.
        if self.current_indent <= self.expr_min_indent
            && loc.col == self.current_indent + 1
        {
            return false;
        }
        true
    }

    fn is_expr_atom_start(&self) -> bool {
        if matches!(
            self.peek(),
            Token::Ident(_)
                | Token::UpperIdent(_)
                | Token::IntLit(_)
                | Token::BigIntLit(_)
                | Token::NumLit(_)
                | Token::StrLit(_)
                | Token::LeftParen
                | Token::LeftBracket
        ) {
            return true;
        }
        // Negative literal: -N where - is not preceded by an expression-ending token
        if let Token::Operator(op) = self.peek()
            && op == "-" && self.pos + 1 < self.tokens.len() && self.is_neg_literal_context() {
                return matches!(self.tokens[self.pos + 1].token,
                    Token::IntLit(_) | Token::BigIntLit(_) | Token::NumLit(_));
            }
        false
    }

    /// Check if a `-` at the current position should be treated as a negative literal prefix.
    /// Returns true when `-` is NOT preceded by an expression-ending token (number, ident, `)`, `]`).
    fn is_neg_literal_context(&self) -> bool {
        if self.pos == 0 { return true; }
        let prev = &self.tokens[self.pos - 1].token;
        !matches!(prev,
            Token::IntLit(_) | Token::BigIntLit(_) | Token::NumLit(_) | Token::StrLit(_)
            | Token::Ident(_) | Token::UpperIdent(_)
            | Token::RightParen | Token::RightBracket)
    }

    // Depth-guard wrapper: the grammar rule itself is in `parse_expr_atom_inner`.
    fn parse_expr_atom(&mut self) -> PResult<Expr> {
        self.guarded("expression", Self::parse_expr_atom_inner)
    }

    /// A parenthesized atom: unit `()`, the tuple constructor `(,)`, a
    /// section (`(+ 2)`, `(2 +)`), an operator as a function (`(+)`), a
    /// tuple, or a plain parenthesized expression — disambiguated in ONE
    /// forward pass (see the section-vs-expression notes inside; parsing
    /// speculatively per alternative would be exponential in nesting
    /// depth). Consumes the opening `(` itself.
    fn parse_paren_expr(&mut self) -> PResult<Expr> {
        self.advance();

        // (,) (,,) ... — tuple constructor as a prefix function. N commas
        // denote an (N+1)-ary constructor. Desugar to a single multi-param
        // lambda building a tuple: this compiles to a genuine N+1-arg Lua
        // function (matching how binary functions are passed to `zipWith`,
        // `foldr`, etc.), while partial application (e.g. `(,) x`) is
        // handled by the ordinary call-site eta-wrap. Type: `a -> b -> (a, b)`.
        if self.at(&Token::Comma) {
            let mut commas = 0;
            while self.at(&Token::Comma) { self.advance(); commas += 1; }
            self.expect(&Token::RightParen)?;
            let params: Vec<String> =
                (0..commas + 1).map(|i| format!("_tup{}", i)).collect();
            let elems: Vec<Expr> =
                params.iter().map(|p| Expr::Var(p.clone())).collect();
            return Ok(Expr::Lambda { params, body: Box::new(Expr::Tuple(elems)) });
        }

        // Check for operator-starting forms: (+), (+1), (-).
        // A leading '-' that is NOT the bare operator `(-)` is
        // prefix minus, never a right section (the GHC rule): it
        // falls through to the general parenthesised-expression
        // path below, which parses it by the negation grammar —
        // so `(-a + b)` is `(negate a) + b` and `(-a * b)` is
        // `negate (a * b)`, not a blanket negation of the whole
        // body.
        if let Token::Operator(op) = self.peek().clone() {
            let bare_op = self.pos + 1 < self.tokens.len()
                && self.tokens[self.pos + 1].token == Token::RightParen;
            if op != "-" || bare_op {
                let op_span = {
                    let l = self.peek_loc();
                    Span::new(l.line, l.col)
                };
                self.advance(); // consume operator
                if self.at(&Token::RightParen) {
                    // (op) — operator as function
                    self.advance();
                    return Ok(Expr::OpFunc(op));
                }
                // (op expr) — right section: \x -> x op expr.
                // Prefix minus in the operand follows the infix
                // rule: legal only under a precedence < 6 operator
                // (GHC rejects `(* -2)` like `a * -2`).
                let (assoc, prec) = self.operator_fixity(&op);
                if prec >= 6
                    && let Token::Operator(m) = self.peek()
                    && m == "-"
                {
                    let par = ParentOp { op: op.clone(), prec, assoc };
                    return Err(self.prefix_minus_rhs_err(&par));
                }
                let rhs = self.parse_right_section_operand(&op)?;
                self.check_section_operand(
                    &op, assoc, prec, Assoc::Right, &rhs, op_span,
                )?;
                self.expect(&Token::RightParen)?;
                return Ok(Expr::Lambda {
                    params: vec!["_sec".into()],
                    body: Box::new(Expr::InfixApp {
                        op,
                        lhs: Box::new(Expr::Var("_sec".into())),
                        rhs: Box::new(rhs),
                    }),
                });
            }
        }

        // (`name` expr) — backtick right section: \x -> x `name` expr
        if self.at(&Token::Backtick) {
            let op_span = {
                let l = self.peek_loc();
                Span::new(l.line, l.col)
            };
            self.advance();
            let name = self.expect_ident()?;
            self.expect(&Token::Backtick)?;
            if self.at(&Token::RightParen) {
                // (`name`) — operator as function
                self.advance();
                return Ok(Expr::OpFunc(name));
            }
            // Prefix minus in the operand follows the infix rule
            // (see the symbolic right-section arm above).
            let (assoc, prec) = self.operator_fixity(&name);
            if prec >= 6
                && let Token::Operator(m) = self.peek()
                && m == "-"
            {
                let par = ParentOp { op: name.clone(), prec, assoc };
                return Err(self.prefix_minus_rhs_err(&par));
            }
            let rhs = self.parse_right_section_operand(&format!("`{}`", name))?;
            self.check_section_operand(
                &name, assoc, prec, Assoc::Right, &rhs, op_span,
            )?;
            self.expect(&Token::RightParen)?;
            return Ok(Expr::Lambda {
                params: vec!["_sec".into()],
                body: Box::new(Expr::InfixApp {
                    op: name,
                    lhs: Box::new(Expr::Var("_sec".into())),
                    rhs: Box::new(rhs),
                }),
            });
        }

        // () — unit
        if self.at(&Token::RightParen) {
            self.advance();
            return Ok(Expr::Lit(Literal::Unit));
        }

        // Parse the parenthesised body ONCE. To test for a left section
        // `(expr op)` we need the application-level parse; if it turns
        // out not to be a section we *continue* infix parsing from that
        // same parse rather than backtracking and re-parsing. Parsing
        // twice (the old behaviour) made nested parens cost O(2^n).
        //
        // Set up exactly as `parse_expr` does (leading layout skip and
        // `expr_min_indent = current_indent`) so the resulting AST — and
        // thus the emitted code — is identical to the previous
        // parse-then-reparse path. `expr_min_indent` is restored before
        // handling `::` ascription and the tuple/paren tail, mirroring
        // `parse_expr`.
        self.skip_newlines_and_indent();
        let saved_expr_min_indent = self.expr_min_indent;
        self.expr_min_indent = self.current_indent;
        // Prefix (not just application) level: a leading '-' is
        // negation here (`(-a + b)` fell through from the
        // operator-section check above).
        let lhs = self.parse_expr_prefix(None)?;

        // Finish the infix expression from the parse we already have
        // (no re-parse). `continue_infix` stops in front of an
        // operator directly followed by ')', so a left-section tail
        // — with a simple operand (`(a +)`) or a full infix one
        // (`(a * b +)`) — is still unconsumed here.
        let mut expr = self.continue_infix(lhs, 0, None)?;

        // (expr op) — left section: \x -> expr op x. The operand
        // must satisfy the section-operand precedence rule
        // (`check_section_operand`): `(a * b +)` is legal,
        // `(a + b *)` is not.
        if let Token::Operator(op) = self.peek().clone() {
            let after_op = self.pos + 1;
            if after_op < self.tokens.len()
                && self.tokens[after_op].token == Token::RightParen {
                    let (op_assoc, op_prec) = self.operator_fixity(&op);
                    let op_span = {
                        let l = self.peek_loc();
                        Span::new(l.line, l.col)
                    };
                    self.check_section_operand(
                        &op, op_assoc, op_prec, Assoc::Left, &expr, op_span,
                    )?;
                    self.advance(); // consume operator
                    self.advance(); // consume )
                    self.expr_min_indent = saved_expr_min_indent;
                    return Ok(Expr::Lambda {
                        params: vec!["_sec".into()],
                        body: Box::new(Expr::InfixApp {
                            op,
                            lhs: Box::new(expr),
                            rhs: Box::new(Expr::Var("_sec".into())),
                        }),
                    });
                }
        }

        // (expr `name`) — backtick left section: \x -> expr `name` x
        if self.at(&Token::Backtick) {
            let after_bt = self.pos + 1;
            if after_bt + 1 < self.tokens.len()
                && let Token::Ident(_) = &self.tokens[after_bt].token
                    && self.tokens[after_bt + 1].token == Token::Backtick
                        && after_bt + 2 < self.tokens.len()
                        && self.tokens[after_bt + 2].token == Token::RightParen
                    {
                        let op_span = {
                            let l = self.peek_loc();
                            Span::new(l.line, l.col)
                        };
                        self.advance(); // consume first backtick
                        let name = self.expect_ident()?;
                        self.advance(); // consume second backtick
                        let (op_assoc, op_prec) = self.operator_fixity(&name);
                        self.check_section_operand(
                            &name, op_assoc, op_prec, Assoc::Left, &expr, op_span,
                        )?;
                        self.advance(); // consume )
                        self.expr_min_indent = saved_expr_min_indent;
                        return Ok(Expr::Lambda {
                            params: vec!["_sec".into()],
                            body: Box::new(Expr::InfixApp {
                                op: name,
                                lhs: Box::new(expr),
                                rhs: Box::new(Expr::Var("_sec".into())),
                            }),
                        });
                    }
        }

        // Not a section — this mirrors `parse_expr`:
        // restore `expr_min_indent`, then `::` ascription.
        self.expr_min_indent = saved_expr_min_indent;
        // Inside explicit ( ) newlines are insignificant: `::`, a tuple
        // comma, or the closing `)` may sit on a continuation line.
        // continue_infix stops at the newline, so skip it before each
        // of those decisions.
        self.skip_newlines_and_indent();
        if self.at(&Token::DblColon) {
            self.advance();
            let ty = self.parse_type()?;
            expr = Expr::Ascription(Box::new(expr), ty);
            self.skip_newlines_and_indent();
        }
        if self.at(&Token::Comma) {
            // Tuple expression: (a, b, ...)
            let mut elems = vec![expr];
            while self.at(&Token::Comma) {
                self.advance();
                elems.push(self.parse_expr()?);
                self.skip_newlines_and_indent();
            }
            self.expect(&Token::RightParen)?;
            Ok(Expr::Tuple(elems))
        } else {
            self.expect(&Token::RightParen)?;
            Ok(Expr::Paren(Box::new(expr)))
        }
    }

    /// A bracketed atom: list literal, range (`[a ..]`, `[a, b .. c]`),
    /// or list comprehension — disambiguated after the first element.
    /// Consumes the opening `[`.
    fn parse_list_expr(&mut self) -> PResult<Expr> {
        self.advance();
        self.skip_newlines_and_indent();
        if self.at(&Token::RightBracket) {
            self.advance();
            return Ok(Expr::Con("[]".to_string()));
        }
        let first = self.parse_expr()?;
        // Inside brackets newlines/indents are insignificant, so a
        // comprehension bar, range `..`, comma or closing `]` may sit
        // on a continuation line.
        self.skip_newlines_and_indent();
        // Check for list comprehension: [expr | qualifiers]
        if self.at(&Token::Pipe) {
            self.advance();
            let quals = self.parse_list_comprehension_quals()?;
            self.skip_newlines_and_indent();
            self.expect(&Token::RightBracket)?;
            return Ok(self.desugar_list_comprehension(first, &quals, &mut 0));
        }
        // Check for range syntax: [x..], [x..y], [x,y..], [x,y..z]
        if self.at(&Token::Operator("..".to_string())) {
            self.advance();
            self.skip_newlines_and_indent();
            if self.at(&Token::RightBracket) {
                // [x..] → enumFrom x
                self.advance();
                return Ok(Expr::App(
                    Box::new(Expr::Var("enumFrom".to_string())),
                    Box::new(first),
                ));
            }
            // [x..y] → enumFromTo x y
            let end = self.parse_expr()?;
            self.skip_newlines_and_indent();
            self.expect(&Token::RightBracket)?;
            return Ok(Expr::App(
                Box::new(Expr::App(
                    Box::new(Expr::Var("enumFromTo".to_string())),
                    Box::new(first),
                )),
                Box::new(end),
            ));
        }
        // Regular list literal or range with step
        let mut items = vec![first];
        self.skip_newlines_and_indent();
        if self.at(&Token::Comma) {
            self.advance();
            self.skip_newlines_and_indent();
            let second = self.parse_expr()?;
            // Check for [x,y..] or [x,y..z]
            if self.at(&Token::Operator("..".to_string())) {
                self.advance();
                self.skip_newlines_and_indent();
                if self.at(&Token::RightBracket) {
                    // [x,y..] → enumFromThen x y
                    self.advance();
                    return Ok(Expr::App(
                        Box::new(Expr::App(
                            Box::new(Expr::Var("enumFromThen".to_string())),
                            Box::new(items.pop().unwrap()),
                        )),
                        Box::new(second),
                    ));
                }
                // [x,y..z] → enumFromThenTo x y z
                let end = self.parse_expr()?;
                self.skip_newlines_and_indent();
                self.expect(&Token::RightBracket)?;
                return Ok(Expr::App(
                    Box::new(Expr::App(
                        Box::new(Expr::App(
                            Box::new(Expr::Var("enumFromThenTo".to_string())),
                            Box::new(items.pop().unwrap()),
                        )),
                        Box::new(second),
                    )),
                    Box::new(end),
                ));
            }
            items.push(second);
            loop {
                self.skip_newlines_and_indent();
                if !self.at(&Token::Comma) { break; }
                self.advance();
                self.skip_newlines_and_indent();
                items.push(self.parse_expr()?);
            }
        }
        self.skip_newlines_and_indent();
        self.expect(&Token::RightBracket)?;
        let mut list = Expr::Con("[]".to_string());
        for item in items.into_iter().rev() {
            list = Expr::App(
                Box::new(Expr::App(
                    Box::new(Expr::Con(":".to_string())),
                    Box::new(item),
                )),
                Box::new(list),
            );
        }
        Ok(list)
    }

    /// A `case scrutinee of` expression with layout-aligned branches and
    /// optional guards. Consumes the `case`.
    fn parse_case_expr(&mut self) -> PResult<Expr> {
        self.advance();
        let scrutinee = self.parse_expr()?;
        self.expect(&Token::Of)?;

        // Inline brace syntax: case x of { A -> e1; B -> e2 }
        if self.at(&Token::LeftBrace) {
            self.advance();
            let mut branches = Vec::new();
            loop {
                self.skip_newlines_and_indent();
                if self.at(&Token::RightBrace) { break; }
                let pattern = self.parse_pattern()?;
                self.expect(&Token::Arrow)?;
                let body = self.parse_stmt_expr()?;
                branches.push(CaseBranch { pattern, guards: vec![], body: Some(body) });
                if self.at(&Token::Semicolon) { self.advance(); } else { break; }
            }
            self.expect(&Token::RightBrace)?;
            return Ok(Expr::Case {
                scrutinee: Box::new(scrutinee),
                branches,
            });
        }

        // Layout-based syntax
        let mut branches = Vec::new();
        let saved_block = self.block_indent;
        let case_indent = self.open_item_block();

        loop {
            let save_pos = self.checkpoint();
            self.skip_newlines_and_indent();
            if self.at_eof() || self.current_indent < case_indent || self.at_block_closer() {
                // Restore position so the caller sees the
                // newline/indent tokens and doesn't accidentally
                // consume the next statement as an argument.
                self.rewind(save_pos);
                break;
            }
            let pattern = self.parse_pattern()?;

            if self.at(&Token::Pipe) {
                // Guards on case branch
                let guards = self.parse_guard_chain(&Token::Arrow)?;
                branches.push(CaseBranch {
                    pattern,
                    guards,
                    body: None,
                });
            } else {
                self.expect(&Token::Arrow)?;
                let body = self.parse_stmt_expr()?;
                branches.push(CaseBranch {
                    pattern,
                    guards: vec![],
                    body: Some(body),
                });
            }
        }
        self.block_indent = saved_block;

        Ok(Expr::Case {
            scrutinee: Box::new(scrutinee),
            branches,
        })
    }

    /// A `do` block: layout-driven statement list with `let`, `pat <-`,
    /// `_ <-`, named binds and bare expressions. Consumes the `do`.
    fn parse_do_block(&mut self) -> PResult<Expr> {
        let do_loc = self.peek_loc().clone();
        self.advance();
        let mut stmts = Vec::new();
        let saved_block = self.block_indent;
        let do_indent = self.open_item_block();

        loop {
            self.skip_newlines_and_indent();
            if self.at_eof() || self.current_indent < do_indent || self.at_block_closer() {
                break;
            }

            // `let` statement: one binding group — simple, function and
            // tuple-pattern bindings, all in one mutually recursive scope —
            // parsed by the same layout rules as a let-expression's group.
            if self.at(&Token::Let) {
                self.advance();
                let let_loc = self.peek_loc().clone();
                let binds = self.parse_let_binds()?;
                if binds.is_empty() {
                    return Err(Box::new(Diagnostic::parse_at(
                        "Expected a binding after `let`: `name = expr`, \
                         `f x = expr`, or `(a, b) = expr`"
                            .to_string(),
                        Span::new(let_loc.line, let_loc.col),
                    )));
                }
                stmts.push(DoStmt::DoLet { binds });
                continue;
            }

            // Check for `(a, b) <- expr` (pattern bind)
            if matches!(self.peek(), Token::LeftParen) {
                let save_tup = self.checkpoint();
                if let Ok(pat) = self.parse_pattern_atom()
                    && matches!(pat, Pattern::Tuple(_)) && self.at(&Token::Bind) {
                        self.advance();
                        let expr = self.parse_stmt_expr()?;
                        stmts.push(DoStmt::PatternBind { pattern: pat, expr });
                        continue;
                    }
                self.rewind(save_tup);
            }

            // Check for `_ <- expr` (discard bind)
            if self.at(&Token::Underscore) {
                let save_u = self.checkpoint();
                self.advance();
                if self.at(&Token::Bind) {
                    self.advance();
                    let expr = self.parse_stmt_expr()?;
                    stmts.push(DoStmt::Bind { name: "_".to_string(), expr });
                    continue;
                }
                self.rewind(save_u);
            }

            // Check for `name <- expr` (bind)
            let save = self.checkpoint();
            if let Token::Ident(name) = self.peek().clone() {
                self.advance();
                if self.at(&Token::Bind) {
                    self.advance();
                    let expr = self.parse_stmt_expr()?;
                    stmts.push(DoStmt::Bind { name, expr });
                    continue;
                }
                self.rewind(save);
            }

            // Bare expression
            let expr = self.parse_stmt_expr()?;
            stmts.push(DoStmt::Expr(expr));
        }

        self.block_indent = saved_block;

        // GHC's rule (Haskell 2010 §3.14): a `do` block has at least one
        // statement, and its LAST statement is an expression — it is the
        // block's result. The other endings used to slip through to the
        // desugarer, which had to invent a meaning: an empty block became
        // the literal False, and a trailing `let x = action` desugared to
        // the binding's right-hand side — silently RUNNING the action the
        // let only meant to name.
        match stmts.last() {
            None => {
                return Err(Box::new(Diagnostic::parse_at(
                    "Empty 'do' block: a 'do' block needs at least one \
                     statement, and its last statement must be an expression \
                     (it is the block's result)"
                        .to_string(),
                    Span::new(do_loc.line, do_loc.col),
                )));
            }
            Some(DoStmt::DoLet { .. }) => {
                return Err(Box::new(Diagnostic::parse_at(
                    "The last statement in a 'do' block must be an \
                     expression, not 'let': a 'let' only names values for \
                     the statements after it. To run the bound action, \
                     write the expression itself as the final statement"
                        .to_string(),
                    Span::new(do_loc.line, do_loc.col),
                )));
            }
            Some(DoStmt::Bind { .. } | DoStmt::PatternBind { .. }) => {
                return Err(Box::new(Diagnostic::parse_at(
                    "The last statement in a 'do' block must be an \
                     expression, not a '<-' bind: the bound name would have \
                     no statement to be used in. To run the action and \
                     discard its result, write the expression alone"
                        .to_string(),
                    Span::new(do_loc.line, do_loc.col),
                )));
            }
            Some(DoStmt::Expr(_)) => {}
        }
        Ok(Expr::Do(stmts))
    }

    /// A lambda: `\apat1 apat2 … -> body`, each parameter an atomic pattern
    /// as in GHC (a variable, `_`, a literal, a nullary constructor, or a
    /// parenthesised/bracketed pattern). Consumes the leading backslash.
    ///
    /// A variable or `_` is a lambda parameter as written. Any other pattern
    /// binds a fresh parameter and matches it in the body: `\(Con x) y ->
    /// e` becomes `\__lam1 y -> case __lam1 of { Con x -> e; _ -> error … }`;
    /// several patterns nest their cases left to right, so a failing match
    /// on an earlier parameter is reported before a later one is looked at.
    fn parse_lambda(&mut self) -> PResult<Expr> {
        self.advance();
        let mut pats = Vec::new();
        while matches!(
            self.peek(),
            Token::Ident(_)
                | Token::Underscore
                | Token::LeftParen
                | Token::LeftBracket
                | Token::UpperIdent(_)
                | Token::IntLit(_)
                | Token::BigIntLit(_)
                | Token::NumLit(_)
                | Token::StrLit(_)
        ) {
            pats.push(self.parse_pattern_atom()?);
        }
        if pats.is_empty() {
            return Err(self.err_here("Expected lambda parameter".to_string()));
        }
        self.expect(&Token::Arrow)?;
        let mut body = self.parse_expr()?;

        let mut params = Vec::with_capacity(pats.len());
        let mut matched = Vec::new(); // (parameter name, pattern), in source order
        for (i, pat) in pats.into_iter().enumerate() {
            match pat {
                Pattern::Var(n) => params.push(n),
                Pattern::Wildcard => params.push("_".to_string()),
                pat => {
                    let name = format!("__lam{}", i + 1);
                    params.push(name.clone());
                    matched.push((name, pat));
                }
            }
        }
        // Wrap innermost-first so the first pattern's case ends up outermost.
        for (name, pat) in matched.into_iter().rev() {
            body = Expr::Case {
                scrutinee: Box::new(Expr::Var(name)),
                branches: vec![
                    CaseBranch { pattern: pat, guards: vec![], body: Some(body) },
                    // Wildcard fallback for a partial pattern.
                    CaseBranch {
                        pattern: Pattern::Wildcard,
                        guards: vec![],
                        body: Some(Expr::App(
                            Box::new(Expr::Var("error".into())),
                            Box::new(Expr::Lit(Literal::Str(b"non-exhaustive lambda pattern".to_vec()))),
                        )),
                    },
                ],
            };
        }
        Ok(Expr::Lambda { params, body: Box::new(body) })
    }

    /// A constructor atom: record construction `Con { f = v, ... }`
    /// (the brace may open on a continuation line indented past the
    /// layout block, like application arguments), or the bare
    /// constructor. The constructor name is already consumed.
    fn parse_con_atom(&mut self, name: String) -> PResult<Expr> {
            // Check for record construction: Con { field = val, ... }
            // The brace may open on a following line indented
            // strictly past the current layout block's column —
            // the same cross-line continuation rule application
            // arguments use (parse_expr_app). At or left of the
            // block column the brace would belong to a sibling
            // item, so `Foo` stays a bare constructor and the
            // position is restored.
            if matches!(self.peek(), Token::Newline | Token::Indent(_)) {
                let save_pos = self.checkpoint();
                self.skip_newlines_and_indent();
                if !(self.at(&Token::LeftBrace)
                    && self.current_indent > self.block_indent)
                {
                    self.rewind(save_pos);
                }
            }
            if self.at(&Token::LeftBrace) {
                self.advance();
                let mut fields = Vec::new();
                loop {
                    self.skip_newlines_and_indent();
                    if self.at(&Token::RightBrace) {
                        self.advance();
                        break;
                    }
                    let field_name = self.expect_ident()?;
                    self.expect(&Token::Eq)?;
                    let value = self.parse_expr()?;
                    fields.push((field_name, value));
                    self.skip_newlines_and_indent();
                    if self.at(&Token::Comma) {
                        self.advance();
                    } else {
                        self.skip_newlines_and_indent();
                        self.expect(&Token::RightBrace)?;
                        break;
                    }
                }
                Ok(Expr::RecordCon { constructor: name, fields })
            } else {
                Ok(Expr::Con(name))
            }
    }

    fn parse_expr_atom_inner(&mut self) -> PResult<Expr> {
        // Negative literal: -N where - is not preceded by an expression-ending token
        if let Token::Operator(op) = self.peek()
            && op == "-" && self.pos + 1 < self.tokens.len() && self.is_neg_literal_context() {
                match self.tokens[self.pos + 1].token.clone() {
                    Token::IntLit(n) => {
                        self.advance(); self.advance();
                        return Ok(Expr::Lit(Literal::Integer(-n)));
                    }
                    Token::BigIntLit(s) => {
                        self.advance(); self.advance();
                        return Ok(Expr::Lit(Literal::BigInteger(format!("-{s}"))));
                    }
                    Token::NumLit(n) => {
                        self.advance(); self.advance();
                        return Ok(Expr::Lit(Literal::Number(-n)));
                    }
                    _ => {}
                }
            }
        match self.peek().clone() {
            Token::IntLit(n) => {
                self.advance();
                Ok(Expr::Lit(Literal::Integer(n)))
            }
            Token::BigIntLit(s) => {
                self.advance();
                Ok(Expr::Lit(Literal::BigInteger(s)))
            }
            Token::NumLit(n) => {
                self.advance();
                Ok(Expr::Lit(Literal::Number(n)))
            }
            Token::StrLit(s) => {
                self.advance();
                Ok(Expr::Lit(Literal::Str(s)))
            } // s: Vec<u8>, the decoded bytes of the literal
            Token::Ident(name) => {
                self.advance();
                Ok(Expr::Var(name))
            }
            Token::UpperIdent(name) => {
                self.advance();
                match name.as_str() {
                    "True" => Ok(Expr::Lit(Literal::Bool(true))),
                    "False" => Ok(Expr::Lit(Literal::Bool(false))),
                    _ => self.parse_con_atom(name),
                }
            }
            Token::LeftParen => self.parse_paren_expr(),
            Token::LeftBracket => self.parse_list_expr(),
            Token::If => {
                self.advance();
                let cond = self.parse_expr()?;
                self.skip_newlines_and_indent();
                self.expect(&Token::Then)?;
                let then_branch = self.parse_stmt_expr()?;
                self.skip_newlines_and_indent();
                self.expect(&Token::Else)?;
                let else_branch = self.parse_stmt_expr()?;
                Ok(Expr::If {
                    cond: Box::new(cond),
                    then_branch: Box::new(then_branch),
                    else_branch: Box::new(else_branch),
                })
            }
            Token::Case => self.parse_case_expr(),
            Token::Let => {
                self.advance();
                let binds = self.parse_let_binds()?;
                self.expect(&Token::In)?;
                self.skip_newlines_and_indent();
                let body = self.parse_expr()?;

                Ok(Expr::Let {
                    binds,
                    body: Box::new(body),
                })
            }
            Token::Do => self.parse_do_block(),
            Token::Backslash => self.parse_lambda(),
            _ => {
                Err(self.err_here(format!("Expected expression, found {}", self.peek())))
            }
        }
    }

    // --- Pattern parsing ---

    // Depth-guard wrapper: the grammar rule itself is in `parse_pattern_inner`.
    fn parse_pattern(&mut self) -> PResult<Pattern> {
        self.guarded("pattern", Self::parse_pattern_inner)
    }

    fn parse_pattern_inner(&mut self) -> PResult<Pattern> {
        let lhs = if let Token::UpperIdent(name) = self.peek().clone() {
            self.advance();
            // True/False are literal patterns, not constructors
            match name.as_str() {
                "True" => Pattern::LitPat(Literal::Bool(true)),
                "False" => Pattern::LitPat(Literal::Bool(false)),
                _ => {
                    let mut args = Vec::new();
                    while self.is_pattern_atom_start() {
                        args.push(self.parse_pattern_atom()?);
                    }
                    if args.is_empty() {
                        Pattern::Constructor { name, args: vec![] }
                    } else {
                        Pattern::Constructor { name, args }
                    }
                }
            }
        } else {
            self.parse_pattern_atom()?
        };

        // Check for infix cons pattern: x : xs
        if let Token::Operator(ref op) = self.peek().clone()
            && op == ":" {
                self.advance();
                let rhs = self.parse_pattern()?;
                return Ok(Pattern::Constructor {
                    name: ":".to_string(),
                    args: vec![lhs, rhs],
                });
            }

        Ok(lhs)
    }

    /// Parameter names of a local function-form binding (`let f x y = e`,
    /// do-`let`). Local bindings support plain variable (or wildcard)
    /// parameters only; a pattern parameter is rejected here rather than
    /// silently renamed away, which would leave the pattern's variables
    /// unbound and fail far from the cause.
    fn lambda_param_names(&self, patterns: Vec<Pattern>) -> PResult<Vec<String>> {
        patterns.into_iter().map(|pat| match pat {
            Pattern::Var(n) => Ok(n),
            Pattern::Wildcard => Ok("_".to_string()),
            _ => {
                let mut diag = self.err_here(
                    "A local function binding cannot take a pattern as a \
                     parameter; bind a plain variable and match it with \
                     'case' in the body."
                        .to_string(),
                );
                diag.notes.push(
                    "GHC accepts pattern parameters in let/where bindings; \
                     mata-ll does not support that yet"
                        .to_string(),
                );
                Err(diag)
            }
        }).collect()
    }

    fn is_pattern_start(&self) -> bool {
        self.is_pattern_atom_start() || matches!(self.peek(), Token::UpperIdent(_))
    }

    fn is_pattern_atom_start(&self) -> bool {
        if matches!(
            self.peek(),
            Token::Ident(_)
                | Token::Underscore
                | Token::IntLit(_)
                | Token::BigIntLit(_)
                | Token::NumLit(_)
                | Token::StrLit(_)
                | Token::LeftParen
                | Token::LeftBracket
                // A bare constructor (e.g. `R`) is a nullary-constructor atom
                // when it appears as an argument of another pattern, as in
                // `T R (T R a x b) y c`. Constructors with arguments must be
                // parenthesized in argument position.
                | Token::UpperIdent(_)
        ) {
            return true;
        }
        // No bare `-` arm: Haskell 2010's apat has no negative literal —
        // a negative pattern in atom position must be parenthesized
        // (`f (-1) = …`, `Just (-1)`), and treating `-N` as an atom start
        // made `f (Just -1)` parse where GHC parse-errors. The
        // parenthesized form reaches parse_pattern_atom_inner's negative
        // arm through the paren, and a whole-pattern `-1` (a case branch)
        // never consults this predicate.
        false
    }

    // Depth-guard wrapper: the grammar rule itself is in `parse_pattern_atom_inner`.
    fn parse_pattern_atom(&mut self) -> PResult<Pattern> {
        self.guarded("pattern", Self::parse_pattern_atom_inner)
    }

    fn parse_pattern_atom_inner(&mut self) -> PResult<Pattern> {
        // Negative literal pattern: -N
        if let Token::Operator(op) = self.peek()
            && op == "-" && self.pos + 1 < self.tokens.len() {
                match self.tokens[self.pos + 1].token.clone() {
                    Token::IntLit(n) => {
                        self.advance(); self.advance();
                        return Ok(Pattern::LitPat(Literal::Integer(-n)));
                    }
                    Token::BigIntLit(s) => {
                        self.advance(); self.advance();
                        return Ok(Pattern::LitPat(Literal::BigInteger(format!("-{s}"))));
                    }
                    Token::NumLit(n) => {
                        self.advance(); self.advance();
                        return Ok(Pattern::LitPat(Literal::Number(-n)));
                    }
                    _ => {}
                }
            }
        match self.peek().clone() {
            Token::Ident(name) => {
                self.advance();
                if self.at(&Token::At) {
                    // As-pattern `xs@(x:_)` (Haskell 2010 §3.17: apat →
                    // var [@ apat]): bind the variable to the whole value
                    // AND match the inner pattern against it. The inner
                    // pattern is an atom, exactly as in GHC — `xs@Just y`
                    // is a parse error there and here; write `xs@(Just y)`.
                    self.advance();
                    let inner = self.parse_pattern_atom()?;
                    return Ok(Pattern::As(name, Box::new(inner)));
                }
                Ok(Pattern::Var(name))
            }
            Token::Underscore => {
                self.advance();
                Ok(Pattern::Wildcard)
            }
            Token::IntLit(n) => {
                self.advance();
                Ok(Pattern::LitPat(Literal::Integer(n)))
            }
            Token::BigIntLit(s) => {
                self.advance();
                Ok(Pattern::LitPat(Literal::BigInteger(s)))
            }
            Token::NumLit(n) => {
                self.advance();
                Ok(Pattern::LitPat(Literal::Number(n)))
            }
            Token::StrLit(s) => {
                self.advance();
                Ok(Pattern::LitPat(Literal::Str(s)))
            }
            Token::LeftParen => {
                self.advance();
                if self.at(&Token::RightParen) {
                    self.advance();
                    // `()` in pattern position is the unit literal pattern
                    // (mirroring the expression parser, where `()` is
                    // Expr::Lit(Literal::Unit)) — NOT a constructor named
                    // "()", which no constructor table will ever contain.
                    return Ok(Pattern::LitPat(Literal::Unit));
                }
                let inner = self.parse_pattern()?;
                if self.at(&Token::Comma) {
                    // Tuple pattern: (a, b, ...)
                    let mut elems = vec![inner];
                    while self.at(&Token::Comma) {
                        self.advance();
                        elems.push(self.parse_pattern()?);
                    }
                    self.expect(&Token::RightParen)?;
                    Ok(Pattern::Tuple(elems))
                } else {
                    self.expect(&Token::RightParen)?;
                    Ok(Pattern::Paren(Box::new(inner)))
                }
            }
            Token::LeftBracket => {
                self.advance();
                if self.at(&Token::RightBracket) {
                    self.advance();
                    return Ok(Pattern::Constructor {
                        name: "[]".to_string(),
                        args: vec![],
                    });
                }
                // [x, y, z] pattern => x : y : z : []
                let mut items = Vec::new();
                items.push(self.parse_pattern()?);
                while self.at(&Token::Comma) {
                    self.advance();
                    items.push(self.parse_pattern()?);
                }
                self.expect(&Token::RightBracket)?;
                let mut pat = Pattern::Constructor { name: "[]".to_string(), args: vec![] };
                for item in items.into_iter().rev() {
                    pat = Pattern::Constructor { name: ":".to_string(), args: vec![item, pat] };
                }
                Ok(pat)
            }
            // A constructor in atom position is nullary; True/False are literal
            // patterns. Constructors with arguments must be parenthesized to
            // appear in argument position, e.g. `(T R a x b)`.
            Token::UpperIdent(name) => {
                self.advance();
                match name.as_str() {
                    "True" => Ok(Pattern::LitPat(Literal::Bool(true))),
                    "False" => Ok(Pattern::LitPat(Literal::Bool(false))),
                    _ => Ok(Pattern::Constructor { name, args: vec![] }),
                }
            }
            _ => {
                Err(self.err_here(format!("Expected pattern, found {}", self.peek())))
            }
        }
    }

    // --- Helpers ---

    fn expect_ident(&mut self) -> PResult<String> {
        match self.peek().clone() {
            Token::Ident(name) => {
                self.advance();
                Ok(name)
            }
            _ => {
                Err(self.err_here(format!("Expected identifier, found {}", self.peek())))
            }
        }
    }

    fn expect_upper_ident(&mut self) -> PResult<String> {
        match self.peek().clone() {
            Token::UpperIdent(name) => {
                self.advance();
                Ok(name)
            }
            _ => {
                Err(self.err_here(format!("Expected type/constructor name, found {}", self.peek())))
            }
        }
    }
}

/// Minimum Pratt binding power for the operand of prefix minus: Haskell's
/// `lexp6 -> - exp7` takes everything binding tighter than precedence 6, so
/// the operand continues through operators with precedence >= 7 (left
/// binding power >= 7*2) and stops at precedence 6 and below.
const NEGATION_OPERAND_MIN_PREC: u8 = 14;

/// Convert a Haskell fixity (Assoc, precedence 0-9) to Pratt binding powers
/// (left, right): precedence doubled, with the associativity deciding which
/// side binds one tighter.
fn assoc_prec_to_binding(assoc: Assoc, prec: u8) -> (u8, u8) {
    let base = prec * 2;
    match assoc {
        Assoc::Left => (base, base + 1),
        Assoc::Right => (base + 1, base),
        // Non-associative: same binding powers as Left. The grouping never
        // materializes — `continue_infix` rejects a same-precedence neighbor
        // of a non-associative operator before it could chain.
        Assoc::None => (base, base + 1),
    }
}

/// Builtin operator fixities, matching the Haskell report and the GHC
/// Prelude. An operator with no `infixl`/`infixr`/`infix` declaration and no
/// entry here defaults to `infixl 9`, exactly as in Haskell.
fn default_operator_fixity(op: &str) -> (Assoc, u8) {
    match op {
        ">>=" | ">>" => (Assoc::Left, 1),
        "$" => (Assoc::Right, 0),
        "||" => (Assoc::Right, 2),
        "&&" => (Assoc::Right, 3),
        "==" | "/=" | "<" | ">" | "<=" | ">=" => (Assoc::None, 4),
        ":" => (Assoc::Right, 5),
        "++" => (Assoc::Right, 5),
        "<>" => (Assoc::Right, 6),
        "+" | "-" => (Assoc::Left, 6),
        "*" | "/" => (Assoc::Left, 7),
        // GHC Prelude: infixl 7 `div`, `mod`, `quot`, `rem`. Declared in
        // Prelude.mll too; listed here so the grouping survives even where
        // the Prelude fixity scan is not in effect. Without this they fell
        // to the infixl 9 default and `4 * 5 \`rem\` 3` regrouped silently
        // (8 instead of GHC's 2).
        "div" | "mod" | "quot" | "rem" => (Assoc::Left, 7),
        "^" => (Assoc::Right, 8),
        "." => (Assoc::Right, 9),
        "!!" => (Assoc::Left, 9),
        _ => (Assoc::Left, 9), // Haskell's default for undeclared operators
    }
}

/// How an operator is written in prose: symbolic operators quoted, function
/// names in backticks (`div`), matching how they appear at the use site.
fn op_display(op: &str) -> String {
    if op.starts_with(|c: char| c.is_alphabetic() || c == '_') {
        format!("`{}`", op)
    } else {
        format!("'{}'", op)
    }
}

/// How an operator is written inside an example expression.
fn op_in_expr(op: &str) -> String {
    if op.starts_with(|c: char| c.is_alphabetic() || c == '_') {
        format!("`{}`", op)
    } else {
        op.to_string()
    }
}

fn assoc_keyword(assoc: Assoc) -> &'static str {
    match assoc {
        Assoc::Left => "infixl",
        Assoc::Right => "infixr",
        Assoc::None => "infix",
    }
}

fn is_comparison_op(op: &str) -> bool {
    matches!(op, "==" | "/=" | "<" | "<=" | ">" | ">=")
}

/// Estimate the source length of a token for adjacency checks.
fn token_len(tok: &Token) -> usize {
    match tok {
        Token::Ident(s) | Token::UpperIdent(s) => s.len(),
        Token::StrLit(s) => s.len(),
        Token::Operator(s) => s.len(),
        Token::IntLit(n) => format!("{}", n).len(),
        Token::NumLit(n) => format!("{}", n).len(),
        Token::LeftParen | Token::RightParen | Token::LeftBracket
        | Token::RightBracket | Token::LeftBrace | Token::RightBrace
        | Token::Comma | Token::Semicolon | Token::Backtick
        | Token::Backslash | Token::Underscore | Token::At => 1,
        Token::Arrow | Token::FatArrow | Token::DblColon | Token::Eq
        | Token::Pipe | Token::Bind => 2,
        _ => 1,
    }
}

/// True when `ty` is (syntactically) `Either String a` for some `a`, ignoring
/// enclosing parentheses. Used to enforce the LuaTry/LuaCatch/LuaIOCatch result
/// shape, so a captured Lua error has a `Left String` slot to land in.
fn is_either_string_type(ty: &Type) -> bool {
    // Peel parentheses.
    let mut head = ty;
    while let Type::Paren(inner) = head { head = inner; }
    // Decompose the application spine: Either String a == App(App(Con Either, String), a).
    let mut args: Vec<&Type> = Vec::new();
    let mut cur = head;
    loop {
        match cur {
            Type::App(f, a) => { args.push(a.as_ref()); cur = f.as_ref(); }
            Type::Con(name) if name == "Either" => {
                args.reverse();
                if args.len() != 2 { return false; }
                let mut fst = args[0];
                while let Type::Paren(inner) = fst { fst = inner; }
                return matches!(fst, Type::Con(s) if s == "String");
            }
            _ => return false,
        }
    }
}

/// Validate that an FFI target string is a well-formed Lua *callee*
/// expression. The accepted grammar (deliberately an expression, not just a
/// name — see `parse_ffi_lua_name`):
///
/// ```text
/// callee := ":" ident                          -- method on the 1st argument
///         | path ( ":" ident )?                -- global path (+ method)
/// path   := ident ( "." ident | "[" index "]" )*
/// index  := digits | '"' chars '"' | "'" chars "'"
/// ident  := [A-Za-z_][A-Za-z0-9_]*  and not a Lua reserved word
/// ```
///
/// Anything else (spaces, operators, empty segments, reserved words, …) would
/// be emitted verbatim into a call position and produce Lua that fails to
/// load, so it is rejected with an explanation of what is wrong.
fn validate_ffi_callee(s: &str) -> Result<(), String> {
    fn take_ident(chars: &[char], mut i: usize) -> Result<usize, String> {
        let start = i;
        match chars.get(i) {
            Some(c) if c.is_ascii_alphabetic() || *c == '_' => i += 1,
            Some(c) => {
                return Err(format!(
                    "expected a Lua name to start here, found '{}' (a Lua name \
                     starts with a letter or '_')",
                    c
                ))
            }
            None => return Err("the target ends where a Lua name was expected".to_string()),
        }
        while matches!(chars.get(i), Some(c) if c.is_ascii_alphanumeric() || *c == '_') {
            i += 1;
        }
        let word: String = chars[start..i].iter().collect();
        if crate::codegen::is_lua_keyword(&word) {
            return Err(format!(
                "'{}' is a Lua reserved word and cannot be used as a name",
                word
            ));
        }
        Ok(i)
    }

    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() {
        return Err("the target is empty".to_string());
    }

    let mut i = 0;
    // Bare-method form `:read` — the method is called on the 1st argument.
    if chars[0] == ':' {
        i = take_ident(&chars, 1)?;
        return if i == chars.len() {
            Ok(())
        } else {
            Err(format!(
                "unexpected '{}' after the method name (a `:name` target is a \
                 single method name, nothing may follow it)",
                chars[i]
            ))
        };
    }

    i = take_ident(&chars, i)?;
    loop {
        match chars.get(i) {
            None => return Ok(()),
            Some('.') => {
                i = take_ident(&chars, i + 1)?;
            }
            Some('[') => {
                i += 1;
                match chars.get(i) {
                    Some(c) if c.is_ascii_digit() => {
                        while matches!(chars.get(i), Some(c) if c.is_ascii_digit()) {
                            i += 1;
                        }
                    }
                    Some(q @ ('"' | '\'')) => {
                        let q = *q;
                        i += 1;
                        while let Some(c) = chars.get(i) {
                            if *c == q {
                                break;
                            }
                            if *c == '\\' || *c == '\n' {
                                return Err(
                                    "a quoted index may not contain backslashes or newlines"
                                        .to_string(),
                                );
                            }
                            i += 1;
                        }
                        if chars.get(i) != Some(&q) {
                            return Err("a quoted index is missing its closing quote".to_string());
                        }
                        i += 1;
                    }
                    _ => {
                        return Err(
                            "an index in '[…]' must be a number or a quoted string".to_string()
                        )
                    }
                }
                if chars.get(i) != Some(&']') {
                    return Err("an index is missing its closing ']'".to_string());
                }
                i += 1;
            }
            // Trailing method: everything before ':' locates the object,
            // the name after it is the method.
            Some(':') => {
                i = take_ident(&chars, i + 1)?;
                return if i == chars.len() {
                    Ok(())
                } else {
                    Err(format!(
                        "unexpected '{}' after the method name (a `:method` may \
                         only end the target)",
                        chars[i]
                    ))
                };
            }
            Some(c) => {
                return Err(format!("unexpected character '{}'", c));
            }
        }
    }
}

/// Scan a token stream for fixity declarations without parsing. A Haskell
/// fixity declaration governs the whole scope it appears in — including uses
/// earlier in the file — so the parser seeds its fixity table from this scan
/// before parsing any expression, instead of discovering declarations in
/// textual order. Mirrors the `parse_fixity_decl` grammar exactly
/// (`infixl`/`infixr`/`infix`, precedence 0-9, comma-separated operators,
/// backtick-quoted or bare function names).
pub fn scan_fixities(tokens: &[Located]) -> Vec<(String, Assoc, u8)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let assoc = match tokens[i].token {
            Token::Infixl => Assoc::Left,
            Token::Infixr => Assoc::Right,
            Token::Infix => Assoc::None,
            _ => {
                i += 1;
                continue;
            }
        };
        i += 1;
        let prec = match tokens.get(i).map(|t| &t.token) {
            Some(Token::IntLit(n)) if (0..=9).contains(n) => {
                i += 1;
                *n as u8
            }
            _ => continue, // malformed — parse_fixity_decl reports it
        };
        loop {
            match tokens.get(i).map(|t| &t.token) {
                Some(Token::Operator(s)) => {
                    out.push((s.clone(), assoc, prec));
                    i += 1;
                }
                Some(Token::Ident(s)) => {
                    out.push((s.clone(), assoc, prec));
                    i += 1;
                }
                Some(Token::Backtick) => {
                    if let (Some(Token::Ident(s)), Some(Token::Backtick)) = (
                        tokens.get(i + 1).map(|t| &t.token),
                        tokens.get(i + 2).map(|t| &t.token),
                    ) {
                        out.push((s.clone(), assoc, prec));
                        i += 3;
                    } else {
                        break;
                    }
                }
                _ => break,
            }
            if matches!(tokens.get(i).map(|t| &t.token), Some(Token::Comma)) {
                i += 1;
            } else {
                break;
            }
        }
    }
    out
}

/// Scan a token stream for the module paths it imports, without parsing.
/// Multi-module compilation needs the import list before the module can be
/// parsed: an imported operator's fixity (Haskell carries fixity with the
/// export) changes how this module's expressions group.
pub fn scan_imports(tokens: &[Located]) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if !matches!(tokens[i].token, Token::Import) {
            i += 1;
            continue;
        }
        i += 1;
        if matches!(tokens.get(i).map(|t| &t.token), Some(Token::Qualified)) {
            i += 1;
        }
        let mut path = Vec::new();
        while let Some(Token::UpperIdent(s)) = tokens.get(i).map(|t| &t.token) {
            path.push(s.clone());
            match tokens.get(i + 1).map(|t| &t.token) {
                Some(Token::Operator(dot)) if dot == "." => i += 2,
                _ => {
                    i += 1;
                    break;
                }
            }
        }
        if !path.is_empty() {
            out.push(path);
        }
    }
    out
}

/// Parse a token stream into a module. On failure, returns every syntax
/// error found (the parser recovers at declaration boundaries), in source
/// order; the list is never empty.
pub fn parse(tokens: &[Located]) -> Result<Module, Vec<Diagnostic>> {
    parse_with_fixities(tokens, &HashMap::new())
}

/// Parse with operator fixities from other modules already in force.
/// Haskell fixity travels with the operator: `import M` brings M's
/// `infixr 3 <+>` into this module's expression grammar, so multi-module
/// compilation seeds each module's parser with its imports' fixities (plus
/// the implicit Prelude's). The module's own declarations, scanned up front,
/// take precedence over imported ones.
pub fn parse_with_fixities(
    tokens: &[Located],
    imported: &HashMap<String, (Assoc, u8)>,
) -> Result<Module, Vec<Diagnostic>> {
    let mut parser = Parser::new(tokens.to_vec());
    for (op, &(assoc, prec)) in imported {
        parser.fixities.insert(op.clone(), (assoc, prec));
    }
    for (op, assoc, prec) in scan_fixities(tokens) {
        parser.fixities.insert(op, (assoc, prec));
    }
    parser.parse_module()
}
