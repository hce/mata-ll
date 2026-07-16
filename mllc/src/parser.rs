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
}

pub struct Parser {
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
        }
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

    /// A parse diagnostic pointing at the current token. The span renders
    /// inline as ` at line:col`, exactly the parser's historical format.
    fn err_here(&self, msg: String) -> Box<Diagnostic> {
        let loc = self.peek_loc();
        Box::new(Diagnostic::parse_at(msg, Span::new(loc.line, loc.col)))
    }

    fn expect(&mut self, expected: &Token) -> PResult<()> {
        if self.peek() == expected {
            self.advance();
            Ok(())
        } else {
            Err(self.err_here(format!(
                "Expected {:?}, found {:?}",
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

    /// Check if the current token is at or beyond a given indentation level
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
                        _ => { self.advance(); } // skip unknown
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

        // Merge consecutive FunDef declarations with the same name
        let mut merged: Vec<Decl> = Vec::new();
        for decl in decls {
            if let Decl::FunDef { name, clauses } = &decl
                && let Some(Decl::FunDef { name: prev_name, clauses: prev_clauses }) = merged.last_mut()
                    && prev_name == name {
                        prev_clauses.extend(clauses.clone());
                        continue;
                    }
            merged.push(decl);
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
                    "Unexpected token {:?} at top level",
                    self.peek()
                )))
            }
        }
    }

    fn parse_data_decl(&mut self) -> PResult<Decl> {
        self.expect(&Token::Data)?;
        let name = self.expect_upper_ident()?;

        let mut type_vars = Vec::new();
        while let Token::Ident(v) = self.peek() {
            type_vars.push(v.clone());
            self.advance();
        }

        // Check for GADT syntax
        if self.at(&Token::Where) {
            self.advance();
            // GADT constructors - for now just skip them and create a basic structure
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
                let save = self.pos;
                if let Ok(constraints) = self.try_parse_constraints() {
                    if self.at(&Token::FatArrow) {
                        self.advance();
                        existential_constraints = constraints;
                    } else {
                        self.pos = save;
                    }
                } else {
                    self.pos = save;
                }
            }

        let name = self.expect_upper_ident()?;

        // Check for record syntax (may be on next line)
        let save_pos = self.pos;
        let save_indent = self.current_indent;
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
                        Token::StrLit(s) => { self.advance(); Some(s) }
                        _ => {
                            return Err(self.err_here(format!(
                                "Expected a string literal after 'as' in field '{}' (e.g. `{} as \"key\" :: T`), found {:?}",
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
            self.pos = save_pos;
            self.current_indent = save_indent;
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
                Ok(Some(s))
            }
            other => Err(self.err_here(format!(
                "Expected a string literal after 'as' in constructor '{}' (e.g. `{} as \"name\"`), found {:?}",
                con_name, con_name, other
            ))),
        }
    }

    /// Parse optional `deriving (Show, Eq)` clause after a data declaration.
    fn parse_deriving(&mut self) -> PResult<Vec<String>> {
        // Look ahead past newlines/indents for 'deriving'
        let save = self.pos;
        let save_indent = self.current_indent;
        self.skip_newlines_and_indent();
        if !self.at(&Token::Deriving) {
            self.pos = save;
            self.current_indent = save_indent;
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
        // Skip optional constructor name (Haskell-style: newtype Rad = Rad Number)
        // MLL newtypes always use the type name as the constructor name.
        if let Token::UpperIdent(con) = self.peek()
            && *con == name {
                self.advance();
            }
        let inner = self.parse_type()?;

        Ok(Decl::NewtypeDef {
            name,
            type_vars,
            inner,
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

    fn parse_class_decl(&mut self) -> PResult<Vec<Decl>> {
        self.expect(&Token::Class)?;

        // Parse optional superclass constraints: Eq a => or (Eq a, Show a) =>
        let save = self.pos;
        let save_indent = self.current_indent;
        let mut superclasses = Vec::new();

        // Try to parse constraints followed by =>
        let first = self.expect_upper_ident()?;
        if let Token::Ident(_) = self.peek() {
            let _tv = self.expect_ident()?;
            if self.at(&Token::FatArrow) {
                // Single constraint: Eq a =>
                superclasses.push(first);
                self.advance(); // consume =>
            } else {
                // No constraint, backtrack
                self.pos = save;
                self.current_indent = save_indent;
            }
        } else if self.at(&Token::Comma) {
            // Multiple constraints would need parens, skip for now
            self.pos = save;
            self.current_indent = save_indent;
        } else {
            self.pos = save;
            self.current_indent = save_indent;
        }

        let class_name = self.expect_upper_ident()?;
        let type_var = self.expect_ident()?;
        self.expect(&Token::Where)?;
        self.skip_newlines_and_indent();

        let mut methods = Vec::new();
        let method_indent = self.current_indent;

        loop {
            self.skip_newlines_and_indent();
            if self.at_eof() || self.current_indent < method_indent {
                break;
            }

            // Parse method signature: name :: type
            // Could be an operator like (+) :: ...
            let save_method = self.pos;
            let save_method_indent = self.current_indent;
            let name = if self.at(&Token::LeftParen) {
                self.advance();
                let op = match self.peek().clone() {
                    Token::Operator(op) => { self.advance(); op }
                    _ => return Err(self.err_here("Expected operator in class method".to_string())),
                };
                self.expect(&Token::RightParen)?;
                op
            } else if let Token::Ident(name) = self.peek().clone() {
                self.advance();
                name
            } else {
                break;
            };

            // Check if this is a type signature (::) or a default method clause (patterns... =)
            if self.at(&Token::DblColon) {
                self.advance();
                let ty = self.parse_type()?;
                methods.push(ClassMethod { name, ty, default_clauses: None });
            } else {
                // This line is a default method definition — backtrack and parse as clause
                self.pos = save_method;
                self.current_indent = save_method_indent;

                // Parse method name (again, consuming it for the clause parser)
                let def_name = if self.at(&Token::LeftParen) {
                    self.advance();
                    let op = match self.peek().clone() {
                        Token::Operator(op) => { self.advance(); op }
                        _ => return Err(self.err_here("Expected operator in default method".to_string())),
                    };
                    self.expect(&Token::RightParen)?;
                    op
                } else if let Token::Ident(n) = self.peek().clone() {
                    self.advance();
                    n
                } else {
                    break;
                };

                let clause = self.parse_clause()?;

                // Attach to the matching method signature
                if let Some(m) = methods.iter_mut().find(|m| m.name == def_name) {
                    match &mut m.default_clauses {
                        Some(clauses) => clauses.push(clause),
                        None => m.default_clauses = Some(vec![clause]),
                    }
                } else {
                    return Err(Box::new(Diagnostic::parse_at(format!(
                        "Default implementation for '{}' has no preceding type signature in class '{}'",
                        def_name, class_name
                    ), clause.span)));
                }
            }
        }

        Ok(vec![Decl::ClassDecl { name: class_name, type_var, superclasses, methods }])
    }

    fn parse_instance_decl(&mut self) -> PResult<Vec<Decl>> {
        self.expect(&Token::Instance)?;

        // Parse an optional context, then `ClassName TargetType where`.
        // Contexts come in the same three shapes as in type signatures —
        // `Show a =>`, `(Show a) =>`, `(Show a, Eq b) =>` — so reuse the
        // signature-context parser. Speculative: `instance Show (Tree a)` also
        // starts like a constraint (`Show` + a type atom), so only commit to
        // the context reading when a `=>` actually follows; otherwise backtrack
        // and treat what was parsed as the class + target.
        let save = self.pos;
        let context = match self.try_parse_constraints() {
            Ok(cs) if self.at(&Token::FatArrow) => {
                self.advance(); // consume =>
                cs
            }
            _ => {
                self.pos = save;
                Vec::new()
            }
        };

        let class_name = self.expect_upper_ident()?;
        let target_type = self.parse_type_atom()?;

        self.expect(&Token::Where)?;
        self.skip_newlines_and_indent();

        let mut methods = Vec::new();
        let method_indent = self.current_indent;

        loop {
            self.skip_newlines_and_indent();
            if self.at_eof() || self.current_indent < method_indent {
                break;
            }

            let name = if self.at(&Token::LeftParen) {
                self.advance();
                let op = match self.peek().clone() {
                    Token::Operator(op) => { self.advance(); op }
                    _ => return Err(self.err_here("Expected operator in instance method".to_string())),
                };
                self.expect(&Token::RightParen)?;
                op
            } else if let Token::Ident(name) = self.peek().clone() {
                self.advance();
                name
            } else {
                break;
            };

            // Collect all clauses for this method
            let clause = self.parse_clause()?;

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

        // Skip type parameter names (they're just documentation here)
        while matches!(self.peek(), Token::Ident(_)) {
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

        Ok(vec![Decl::TypeFamily { name, equations }])
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
        // Unknown intrinsic form — skip
        while !self.at_eof() {
            match self.peek() {
                Token::Indent(n) if *n == 0 => break,
                Token::EOF => break,
                _ => { self.advance(); }
            }
        }
        Ok(vec![])
    }

    /// Look up operator precedence: user-defined fixity overrides defaults.
    fn operator_precedence(&self, op: &str) -> (u8, u8) {
        if let Some((assoc, prec)) = self.fixities.get(op) {
            assoc_prec_to_binding(*assoc, *prec)
        } else {
            default_operator_precedence(op)
        }
    }

    /// Parse a fixity declaration: `infixl 6 +` or `infixr 5 :`
    fn parse_fixity_decl(&mut self) -> PResult<Vec<Decl>> {
        let assoc = match self.peek() {
            Token::Infixl => { self.advance(); Assoc::Left }
            Token::Infixr => { self.advance(); Assoc::Right }
            Token::Infix => { self.advance(); Assoc::None }
            _ => unreachable!(),
        };
        let prec = match self.peek() {
            Token::IntLit(n) => {
                let p = *n as u8;
                self.advance();
                p
            }
            _ => return Err(self.err_here("Expected precedence level (0-9) after infixl/infixr/infix".to_string())),
        };
        let op = match self.peek().clone() {
            Token::Operator(s) => { self.advance(); s }
            Token::Ident(s) => { self.advance(); s } // backtick operators
            _ => return Err(self.err_here("Expected operator after fixity precedence".to_string())),
        };
        self.fixities.insert(op.clone(), (assoc, prec));
        Ok(vec![Decl::FixityDecl { assoc, prec, op }])
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
        let mut guards = Vec::new();
        self.skip_newlines_and_indent();
        if self.at(&Token::Pipe) {
            while self.at(&Token::Pipe) {
                self.advance();
                let condition = self.parse_expr()?;
                self.expect(&Token::Eq)?;
                let body = self.parse_expr()?;
                guards.push(Guard { condition, body });
                self.skip_newlines_and_indent();
            }
        }

        let body = if guards.is_empty() {
            self.expect(&Token::Eq)?;
            self.parse_expr()?
        } else {
            Expr::Var("undefined".to_string())
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

    fn parse_where(&mut self) -> PResult<Vec<LocalDef>> {
        self.skip_newlines_and_indent();
        if !self.at(&Token::Where) {
            return Ok(vec![]);
        }
        self.advance();
        self.skip_newlines_and_indent();

        let mut binds = Vec::new();
        let where_indent = self.current_indent;
        let saved_block = self.block_indent;
        self.block_indent = self.peek_loc().col.saturating_sub(1);

        loop {
            self.skip_newlines_and_indent();
            if self.at_eof() || self.current_indent < where_indent {
                break;
            }
            if !matches!(self.peek(), Token::Ident(_)) {
                break;
            }
            let name = self.expect_ident()?;
            let mut patterns = Vec::new();
            while self.is_pattern_start() {
                patterns.push(self.parse_pattern_atom()?);
            }

            // Handle guards: go acc i | i <= 0 = acc | otherwise = ...
            self.skip_newlines_and_indent();
            if self.at(&Token::Pipe) {
                // Parse guarded where binding — desugar to if/else chain
                let mut guards = Vec::new();
                while self.at(&Token::Pipe) {
                    self.advance();
                    let cond = self.parse_expr()?;
                    self.expect(&Token::Eq)?;
                    let val = self.parse_expr()?;
                    guards.push((cond, val));
                    self.skip_newlines_and_indent();
                }
                // Build nested if/else from guards
                let body = guards.into_iter().rev().fold(
                    Expr::App(Box::new(Expr::Var("error".into())), Box::new(Expr::Lit(Literal::Str("non-exhaustive guards".into())))),
                    |else_branch, (cond, val)| Expr::If {
                        cond: Box::new(cond),
                        then_branch: Box::new(val),
                        else_branch: Box::new(else_branch),
                    },
                );
                binds.push(LocalDef { name, patterns, body });
            } else {
                self.expect(&Token::Eq)?;
                let body = self.parse_expr()?;
                binds.push(LocalDef { name, patterns, body });
            }
        }
        self.block_indent = saved_block;

        Ok(binds)
    }

    // --- Type parsing ---

    fn parse_type(&mut self) -> PResult<Type> {
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
        let save = self.pos;
        if let Ok(constraints) = self.try_parse_constraints()
            && self.at(&Token::FatArrow) {
                self.advance();
                let ty = self.parse_type_arrow()?;
                return Ok(Type::Constrained {
                    constraints,
                    ty: Box::new(ty),
                });
            }
        self.pos = save;
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

    fn parse_type_arrow(&mut self) -> PResult<Type> {
        let lhs = self.parse_type_app()?;
        self.skip_newlines_and_indent();
        if self.at(&Token::Arrow) {
            self.advance();
            self.skip_newlines_and_indent();
            let rhs = self.parse_type_arrow()?;
            Ok(Type::Arrow(Box::new(lhs), Box::new(rhs)))
        } else {
            Ok(lhs)
        }
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

    fn parse_type_atom(&mut self) -> PResult<Type> {
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
                        let lua_name = match self.peek().clone() {
                            Token::StrLit(s) => { self.advance(); s }
                            _ => return Err(self.err_here("LuaPure expects a string literal".to_string())),
                        };
                        let result = self.parse_type_atom()?;
                        Ok(Type::LuaPure { lua_name, result: Box::new(result) })
                    }
                    "LuaIO" => {
                        // LuaIO "lua.func.name" ReturnType
                        let lua_name = match self.peek().clone() {
                            Token::StrLit(s) => { self.advance(); s }
                            _ => return Err(self.err_here("LuaIO expects a string literal".to_string())),
                        };
                        let result = self.parse_type_atom()?;
                        Ok(Type::LuaIO { lua_name, result: Box::new(result) })
                    }
                    "LuaIterator" => {
                        // LuaIterator "lua.func.name" ResultListType
                        // (a bare element type is the [T] shorthand — see ast.rs)
                        let lua_name = match self.peek().clone() {
                            Token::StrLit(s) => { self.advance(); s }
                            _ => return Err(self.err_here("LuaIterator expects a string literal".to_string())),
                        };
                        let result = self.parse_type_atom()?;
                        Ok(Type::LuaIterator { lua_name, result: Box::new(result) })
                    }
                    "LuaTry" => {
                        // LuaTry "lua.func.name" ResultType
                        let lua_name = match self.peek().clone() {
                            Token::StrLit(s) => { self.advance(); s }
                            _ => return Err(self.err_here("LuaTry expects a string literal".to_string())),
                        };
                        let result = self.parse_type_atom()?;
                        Ok(Type::LuaTry { lua_name, result: Box::new(result) })
                    }
                    "LuaCatch" | "LuaIOCatch" => {
                        // LuaCatch    "lua.func.name" (Either String T)  ->  Either String T
                        // LuaIOCatch  "lua.func.name" (Either String T)  ->  IO (Either String T)
                        // A raised Lua error is captured as `Left msg` via pcall.
                        let lua_name = match self.peek().clone() {
                            Token::StrLit(s) => { self.advance(); s }
                            _ => return Err(self.err_here(format!("{} expects a string literal", name))),
                        };
                        let result = self.parse_type_atom()?;
                        if !is_either_string_type(&result) {
                            return Err(self.err_here(format!(
                                "{} requires the result to be written as `(Either String a)`, \
                                 so a raised Lua error can be returned as `Left`",
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
                // An operator in type position, e.g. `f :: (+) -> Integer`.
                // This used to be silently parsed as the unit type, so the
                // program compiled with a signature that meant something
                // entirely different from what was written — reject it with
                // an explanation instead.
                if let Token::Operator(op) = self.peek().clone() {
                    let mut diag = self.err_here(format!(
                        "The operator '{}' cannot appear in a type: '({})' names a \
                         function (a value), and a type must be built from type names, \
                         type variables, lists, tuples, and '->'",
                        op, op
                    ));
                    diag.notes.push(
                        "GHC can accept an operator in a type with the TypeOperators \
                         extension; mata-ll has no type-level operators, so this is \
                         always an error here"
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
                Ok(Type::Con(format!("\"{}\"", s)))
            }
            _ => {
                Err(self.err_here(format!("Expected type, found {:?}", self.peek())))
            }
        }
    }

    // --- Expression parsing ---

    fn parse_expr(&mut self) -> PResult<Expr> {
        // Skip leading indent/newlines to find the actual expression start
        self.skip_newlines_and_indent();
        let saved_expr_min_indent = self.expr_min_indent;
        self.expr_min_indent = self.current_indent;
        let expr = self.parse_expr_infix(0)?;
        self.expr_min_indent = saved_expr_min_indent;

        // Type ascription: expr :: Type
        if self.at(&Token::DblColon) {
            self.advance();
            let ty = self.parse_type()?;
            return Ok(Expr::Ascription(Box::new(expr), ty));
        }

        Ok(expr)
    }

    fn parse_expr_infix(&mut self, min_prec: u8) -> PResult<Expr> {
        let lhs = self.parse_expr_prefix()?;
        self.continue_infix(lhs, min_prec)
    }

    /// Continue infix-operator parsing from an already-parsed left operand.
    /// Splitting this out of `parse_expr_infix` lets callers that have already
    /// parsed an application (e.g. the parenthesised-expression path, which
    /// parses one to test for a left section) resume without re-parsing it —
    /// the parenthesised body would otherwise be parsed twice at every nesting
    /// level, giving O(2^n) parse time on deeply nested parentheses.
    fn continue_infix(&mut self, mut lhs: Expr, min_prec: u8) -> PResult<Expr> {
        loop {
            // Try to consume indentation for continuation lines
            // Only if the next real token after indent is an operator
            // and the indent is at or deeper than the expression start
            if let Token::Indent(n) = self.peek() {
                let n = *n;
                if n >= self.expr_min_indent {
                    let save = self.pos;
                    self.advance(); // consume indent
                    self.current_indent = n;
                    // Check if next token is an operator (continuation)
                    if !matches!(self.peek(), Token::Operator(_) | Token::Backtick) {
                        // Not a continuation — put it back
                        self.pos = save;
                    }
                }
            }

            // Check for operator
            match self.peek().clone() {
                Token::Operator(ref op) if op == ".." => {
                    break; // '..' is range syntax, not an infix operator
                }
                Token::Operator(ref op) => {
                    let (lp, rp) = self.operator_precedence(op);
                    if lp < min_prec {
                        break;
                    }
                    let op = op.clone();
                    self.advance();
                    self.skip_newlines_and_indent();
                    let rhs = self.parse_expr_infix(rp)?;
                    lhs = Expr::InfixApp {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                }
                Token::Backtick => {
                    self.advance();
                    let func = self.expect_ident()?;
                    self.expect(&Token::Backtick)?;
                    let (lp, rp) = self.operator_precedence(&func);
                    if lp < min_prec { break; }
                    self.skip_newlines_and_indent();
                    let rhs = self.parse_expr_infix(rp)?;
                    lhs = Expr::InfixApp {
                        op: func,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                }
                _ => break,
            }
        }

        Ok(lhs)
    }

    fn parse_expr_prefix(&mut self) -> PResult<Expr> {
        // Negation
        if let Token::Operator(ref op) = self.peek().clone()
            && op == "-" {
                self.advance();
                let expr = self.parse_expr_app()?;
                return Ok(Expr::Negate(Box::new(expr)));
            }
        self.parse_expr_app()
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
                let save_pos = self.pos;
                let save_indent = self.current_indent;
                self.skip_newlines_and_indent();
                if self.current_indent > self.block_indent
                    && self.is_expr_atom_start()
                {
                    let arg = self.parse_expr_atom_dotted()?;
                    func = Expr::App(Box::new(func), Box::new(arg));
                    continue;
                }
                // Not a continuation — backtrack
                self.pos = save_pos;
                self.current_indent = save_indent;
            }

            break;
        }

        Ok(func)
    }

    /// Parse an atom optionally followed by one or more `.field` accesses.
    /// `expr.field` desugars to `(field expr)`.
    /// Only applies when `.` is adjacent to the preceding token (no space),
    /// to distinguish from function composition `f . g`.
    /// Parse list comprehension qualifiers: x <- xs, pred, y <- ys, ...
    /// Supports pattern-matching generators: Ok x <- rs, (a, b) <- pairs, ...
    fn parse_list_comprehension_quals(&mut self) -> PResult<Vec<ListCompQual>> {
        let mut quals = Vec::new();
        loop {
            self.skip_newlines_and_indent();
            // Try generator: pattern <- expr
            let save = self.pos;
            let save_indent = self.current_indent;
            if self.is_pattern_start() {
                if let Ok(pat) = self.parse_pattern()
                    && self.at(&Token::Bind) {
                        self.advance();
                        let expr = self.parse_expr()?;
                        quals.push(ListCompQual::Generator { pattern: pat, expr });
                        if self.at(&Token::Comma) { self.advance(); continue; }
                        break;
                    }
                // Not a generator — backtrack and parse as guard
                self.pos = save;
                self.current_indent = save_indent;
            }
            // Guard expression
            let expr = self.parse_expr()?;
            quals.push(ListCompQual::Guard(expr));
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
                                    body: rest,
                                },
                                CaseBranch {
                                    pattern: Pattern::Wildcard,
                                    guards: vec![],
                                    body: Expr::Con("[]".to_string()),
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
        }
    }

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
        // Only parse if '{' is on the same line (to avoid conflict with do-blocks)
        // Loop to allow chained updates: expr { x = 1 } { y = 2 }
        while self.at(&Token::LeftBrace) && self.pos > 0 {
            let prev_tok = &self.tokens[self.pos - 1];
            let brace_tok = &self.tokens[self.pos];
            if brace_tok.line != prev_tok.line {
                break;
            }
            let save = self.pos;
            if let Ok(updates) = self.try_parse_record_update() {
                expr = Expr::RecordUpdate {
                    expr: Box::new(expr),
                    updates,
                };
            } else {
                self.pos = save;
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
                return matches!(self.tokens[self.pos + 1].token, Token::IntLit(_) | Token::NumLit(_));
            }
        false
    }

    /// Check if a `-` at the current position should be treated as a negative literal prefix.
    /// Returns true when `-` is NOT preceded by an expression-ending token (number, ident, `)`, `]`).
    fn is_neg_literal_context(&self) -> bool {
        if self.pos == 0 { return true; }
        let prev = &self.tokens[self.pos - 1].token;
        !matches!(prev,
            Token::IntLit(_) | Token::NumLit(_) | Token::StrLit(_)
            | Token::Ident(_) | Token::UpperIdent(_)
            | Token::RightParen | Token::RightBracket)
    }

    fn parse_expr_atom(&mut self) -> PResult<Expr> {
        // Negative literal: -N where - is not preceded by an expression-ending token
        if let Token::Operator(op) = self.peek()
            && op == "-" && self.pos + 1 < self.tokens.len() && self.is_neg_literal_context() {
                match self.tokens[self.pos + 1].token {
                    Token::IntLit(n) => {
                        self.advance(); self.advance();
                        return Ok(Expr::Lit(Literal::Integer(-n)));
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
            Token::NumLit(n) => {
                self.advance();
                Ok(Expr::Lit(Literal::Number(n)))
            }
            Token::StrLit(s) => {
                self.advance();
                Ok(Expr::Lit(Literal::Str(s)))
            }
            Token::Ident(name) => {
                self.advance();
                Ok(Expr::Var(name))
            }
            Token::UpperIdent(name) => {
                self.advance();
                match name.as_str() {
                    "True" => Ok(Expr::Lit(Literal::Bool(true))),
                    "False" => Ok(Expr::Lit(Literal::Bool(false))),
                    _ => {
                        // Check for record construction: Con { field = val, ... }
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
                }
            }
            Token::LeftParen => {
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

                // Check for operator-starting forms: (+), (+1), (-)
                if let Token::Operator(op) = self.peek().clone() {
                    self.advance(); // consume operator
                    if self.at(&Token::RightParen) {
                        // (op) — operator as function
                        self.advance();
                        return Ok(Expr::OpFunc(op));
                    }
                    if op == "-" {
                        // (-expr) is negation, not a section
                        let inner = self.parse_expr()?;
                        self.expect(&Token::RightParen)?;
                        return Ok(Expr::Paren(Box::new(Expr::Negate(Box::new(inner)))));
                    }
                    // (op expr) — right section: \x -> x op expr
                    let rhs = self.parse_expr()?;
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

                // (`name` expr) — backtick right section: \x -> x `name` expr
                if self.at(&Token::Backtick) {
                    self.advance();
                    let name = self.expect_ident()?;
                    self.expect(&Token::Backtick)?;
                    if self.at(&Token::RightParen) {
                        // (`name`) — operator as function
                        self.advance();
                        return Ok(Expr::OpFunc(name));
                    }
                    let rhs = self.parse_expr()?;
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
                let lhs = self.parse_expr_app()?;

                // (expr op) — left section: \x -> expr op x
                if let Token::Operator(op) = self.peek().clone() {
                    let after_op = self.pos + 1;
                    if after_op < self.tokens.len()
                        && self.tokens[after_op].token == Token::RightParen {
                            self.advance(); // consume operator
                            self.advance(); // consume )
                            self.expr_min_indent = saved_expr_min_indent;
                            return Ok(Expr::Lambda {
                                params: vec!["_sec".into()],
                                body: Box::new(Expr::InfixApp {
                                    op,
                                    lhs: Box::new(lhs),
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
                                self.advance(); // consume first backtick
                                let name = self.expect_ident()?;
                                self.advance(); // consume second backtick
                                self.advance(); // consume )
                                self.expr_min_indent = saved_expr_min_indent;
                                return Ok(Expr::Lambda {
                                    params: vec!["_sec".into()],
                                    body: Box::new(Expr::InfixApp {
                                        op: name,
                                        lhs: Box::new(lhs),
                                        rhs: Box::new(Expr::Var("_sec".into())),
                                    }),
                                });
                            }
                }

                // Not a section — finish the infix expression from the parse we
                // already have (no re-parse). This mirrors `parse_expr`:
                // continue infix, restore `expr_min_indent`, then `::` ascription.
                let mut expr = self.continue_infix(lhs, 0)?;
                self.expr_min_indent = saved_expr_min_indent;
                if self.at(&Token::DblColon) {
                    self.advance();
                    let ty = self.parse_type()?;
                    expr = Expr::Ascription(Box::new(expr), ty);
                }
                if self.at(&Token::Comma) {
                    // Tuple expression: (a, b, ...)
                    let mut elems = vec![expr];
                    while self.at(&Token::Comma) {
                        self.advance();
                        elems.push(self.parse_expr()?);
                    }
                    self.expect(&Token::RightParen)?;
                    Ok(Expr::Tuple(elems))
                } else {
                    self.expect(&Token::RightParen)?;
                    Ok(Expr::Paren(Box::new(expr)))
                }
            }
            Token::LeftBracket => {
                self.advance();
                self.skip_newlines_and_indent();
                if self.at(&Token::RightBracket) {
                    self.advance();
                    return Ok(Expr::Con("[]".to_string()));
                }
                let first = self.parse_expr()?;
                // Check for list comprehension: [expr | qualifiers]
                if self.at(&Token::Pipe) {
                    self.advance();
                    let quals = self.parse_list_comprehension_quals()?;
                    self.expect(&Token::RightBracket)?;
                    return Ok(self.desugar_list_comprehension(first, &quals, &mut 0));
                }
                // Check for range syntax: [x..], [x..y], [x,y..], [x,y..z]
                if self.at(&Token::Operator("..".to_string())) {
                    self.advance();
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
            Token::If => {
                self.advance();
                let cond = self.parse_expr()?;
                self.skip_newlines_and_indent();
                self.expect(&Token::Then)?;
                let then_branch = self.parse_expr()?;
                self.skip_newlines_and_indent();
                self.expect(&Token::Else)?;
                let else_branch = self.parse_expr()?;
                Ok(Expr::If {
                    cond: Box::new(cond),
                    then_branch: Box::new(then_branch),
                    else_branch: Box::new(else_branch),
                })
            }
            Token::Case => {
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
                        let body = self.parse_expr()?;
                        branches.push(CaseBranch { pattern, guards: vec![], body });
                        if self.at(&Token::Semicolon) { self.advance(); } else { break; }
                    }
                    self.expect(&Token::RightBrace)?;
                    return Ok(Expr::Case {
                        scrutinee: Box::new(scrutinee),
                        branches,
                    });
                }

                // Layout-based syntax
                self.skip_newlines_and_indent();
                let case_indent = self.current_indent;
                let mut branches = Vec::new();
                let saved_block = self.block_indent;
                self.block_indent = self.peek_loc().col.saturating_sub(1);

                loop {
                    let save_pos = self.pos;
                    let save_indent = self.current_indent;
                    self.skip_newlines_and_indent();
                    if self.at_eof() || self.current_indent < case_indent
                        || self.at(&Token::Where)
                        || self.at(&Token::RightParen)
                        || self.at(&Token::RightBracket)
                        || self.at(&Token::RightBrace) {
                        // Restore position so the caller sees the
                        // newline/indent tokens and doesn't accidentally
                        // consume the next statement as an argument.
                        self.pos = save_pos;
                        self.current_indent = save_indent;
                        break;
                    }
                    let pattern = self.parse_pattern()?;

                    if self.at(&Token::Pipe) {
                        // Guards on case branch
                        let mut guards = Vec::new();
                        while self.at(&Token::Pipe) {
                            self.advance();
                            let condition = self.parse_expr()?;
                            self.expect(&Token::Arrow)?;
                            let body = self.parse_expr()?;
                            guards.push(Guard { condition, body });
                            self.skip_newlines_and_indent();
                        }
                        branches.push(CaseBranch {
                            pattern,
                            guards,
                            body: Expr::Var("undefined".to_string()),
                        });
                    } else {
                        self.expect(&Token::Arrow)?;
                        let body = self.parse_expr()?;
                        branches.push(CaseBranch {
                            pattern,
                            guards: vec![],
                            body,
                        });
                    }
                }
                self.block_indent = saved_block;

                Ok(Expr::Case {
                    scrutinee: Box::new(scrutinee),
                    branches,
                })
            }
            Token::Let => {
                self.advance();
                self.skip_newlines_and_indent();
                let mut binds = Vec::new();
                let let_indent = self.current_indent;
                let saved_block = self.block_indent;
                self.block_indent = self.peek_loc().col.saturating_sub(1);
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
                            for v in crate::ast::pattern_var_names(&pat) {
                                binds.push(LocalDef {
                                    name: v.clone(),
                                    patterns: vec![],
                                    body: Expr::Case {
                                        scrutinee: Box::new(Expr::Var(fresh.clone())),
                                        branches: vec![CaseBranch {
                                            pattern: pat.clone(),
                                            guards: vec![],
                                            body: Expr::Var(v),
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
                    let name = self.expect_ident()?;
                    let mut patterns = Vec::new();
                    while self.is_pattern_start() {
                        patterns.push(self.parse_pattern_atom()?);
                    }
                    self.expect(&Token::Eq)?;
                    let mut body = self.parse_expr()?;
                    // Desugar a function binding `let f x y = e` into a value
                    // binding of a lambda `f = \x y -> e`, matching do-`let`.
                    // This keeps the whole `let` group a uniform value-binding
                    // group so it is inferred and generated as one mutually
                    // recursive scope (patterns on let-binds are otherwise not
                    // handled by the let pipeline).
                    if !patterns.is_empty() {
                        body = Expr::Lambda {
                            params: Self::lambda_param_names(patterns),
                            body: Box::new(body),
                        };
                    }
                    binds.push(LocalDef { name, patterns: vec![], body });
                }

                self.skip_newlines_and_indent();
                self.block_indent = saved_block;
                self.expect(&Token::In)?;
                self.skip_newlines_and_indent();
                let body = self.parse_expr()?;

                Ok(Expr::Let {
                    binds,
                    body: Box::new(body),
                })
            }
            Token::Do => {
                self.advance();
                self.skip_newlines_and_indent();
                let do_indent = self.current_indent;
                let mut stmts = Vec::new();
                let saved_block = self.block_indent;
                self.block_indent = self.peek_loc().col.saturating_sub(1);

                loop {
                    self.skip_newlines_and_indent();
                    if self.at_eof() || self.current_indent < do_indent
                        || self.at(&Token::RightParen) {
                        break;
                    }

                    // Check for `let name = expr` or `let (a, b) = expr`
                    if self.at(&Token::Let) {
                        self.advance();
                        let let_indent = self.current_indent;
                        // Tuple pattern: let (a, b) = expr
                        if matches!(self.peek(), Token::LeftParen) {
                            let pat = self.parse_pattern_atom()?;
                            if matches!(pat, Pattern::Tuple(_)) {
                                self.expect(&Token::Eq)?;
                                let expr = self.parse_expr()?;
                                stmts.push(DoStmt::PatternDoLet { pattern: pat, expr });
                                continue;
                            }
                            return Err(self.err_here("Expected tuple pattern or identifier in let binding".to_string()));
                        }
                        // The binding column is the layout block for the
                        // binding RHS(s), so a following sibling binding at the
                        // same column is not swallowed as a continuation arg.
                        let saved_do_block = self.block_indent;
                        self.block_indent = self.peek_loc().col.saturating_sub(1);
                        let name = self.expect_ident()?;
                        // Collect optional patterns: let f x y = expr => let f = \x y -> expr
                        let mut params = Vec::new();
                        while self.is_pattern_start() && !self.at(&Token::Eq) {
                            params.push(self.parse_pattern_atom()?);
                        }
                        self.expect(&Token::Eq)?;
                        let mut expr = self.parse_expr()?;
                        // Desugar: wrap body in a single multi-param lambda
                        if !params.is_empty() {
                            expr = Expr::Lambda {
                                params: Self::lambda_param_names(params),
                                body: Box::new(expr),
                            };
                        }
                        // Accumulate all bindings of THIS `let` group into one
                        // list so they share a single mutually-recursive scope
                        // (Haskell 2010 letrec); a later binding may be referenced
                        // by an earlier one regardless of source order.
                        let mut group = vec![LocalDef { name, patterns: vec![], body: expr }];
                        // Continue parsing additional bindings at the same or deeper indent
                        loop {
                            let save_pos = self.pos;
                            let save_indent = self.current_indent;
                            self.skip_newlines_and_indent();
                            if self.current_indent >= let_indent
                                && let Token::Ident(_) = self.peek() {
                                    // Peek ahead for `name [patterns] =`
                                    let save2 = self.pos;
                                    let save2_indent = self.current_indent;
                                    let name2 = self.expect_ident().ok();
                                    if let Some(n2) = name2 {
                                        let mut params2 = Vec::new();
                                        while self.is_pattern_start() && !self.at(&Token::Eq) {
                                            if let Ok(p) = self.parse_pattern_atom() {
                                                params2.push(p);
                                            } else { break; }
                                        }
                                        if self.at(&Token::Eq) {
                                            self.advance();
                                            let mut expr2 = self.parse_expr()?;
                                            if !params2.is_empty() {
                                                expr2 = Expr::Lambda {
                                                    params: Self::lambda_param_names(params2),
                                                    body: Box::new(expr2),
                                                };
                                            }
                                            group.push(LocalDef { name: n2, patterns: vec![], body: expr2 });
                                            continue;
                                        }
                                    }
                                    self.pos = save2;
                                    self.current_indent = save2_indent;
                                }
                            // Not a continuation binding — backtrack
                            self.pos = save_pos;
                            self.current_indent = save_indent;
                            break;
                        }
                        stmts.push(DoStmt::DoLet { binds: group });
                        self.block_indent = saved_do_block;
                        continue;
                    }

                    // Check for `(a, b) <- expr` (pattern bind)
                    if matches!(self.peek(), Token::LeftParen) {
                        let save_tup = self.pos;
                        let save_tup_indent = self.current_indent;
                        if let Ok(pat) = self.parse_pattern_atom()
                            && matches!(pat, Pattern::Tuple(_)) && self.at(&Token::Bind) {
                                self.advance();
                                let expr = self.parse_expr()?;
                                stmts.push(DoStmt::PatternBind { pattern: pat, expr });
                                continue;
                            }
                        self.pos = save_tup;
                        self.current_indent = save_tup_indent;
                    }

                    // Check for `_ <- expr` (discard bind)
                    if self.at(&Token::Underscore) {
                        let save_u = self.pos;
                        self.advance();
                        if self.at(&Token::Bind) {
                            self.advance();
                            let expr = self.parse_expr()?;
                            stmts.push(DoStmt::Bind { name: "_".to_string(), expr });
                            continue;
                        }
                        self.pos = save_u;
                    }

                    // Check for `name <- expr` (bind)
                    let save = self.pos;
                    if let Token::Ident(name) = self.peek().clone() {
                        self.advance();
                        if self.at(&Token::Bind) {
                            self.advance();
                            let expr = self.parse_expr()?;
                            stmts.push(DoStmt::Bind { name, expr });
                            continue;
                        }
                        self.pos = save;
                    }

                    // Bare expression
                    let expr = self.parse_expr()?;
                    stmts.push(DoStmt::Expr(expr));
                }

                self.block_indent = saved_block;
                Ok(Expr::Do(stmts))
            }
            Token::Backslash => {
                self.advance();
                // Check for pattern-matching lambda: \(Con x) -> body
                // Desugars to \__arg -> case __arg of { pattern -> body }
                if matches!(self.peek(), Token::LeftParen | Token::UpperIdent(_) | Token::LeftBracket) {
                    let save = self.pos;
                    let save_indent = self.current_indent;
                    // Try parsing as pattern
                    if let Ok(pat) = self.parse_pattern()
                        && self.at(&Token::Arrow) {
                            self.advance();
                            let body = self.parse_expr()?;
                            let mut branches = vec![CaseBranch {
                                pattern: pat,
                                guards: vec![],
                                body,
                            }];
                            // Add wildcard fallback for partial patterns
                            branches.push(CaseBranch {
                                pattern: Pattern::Wildcard,
                                guards: vec![],
                                body: Expr::App(
                                    Box::new(Expr::Var("error".into())),
                                    Box::new(Expr::Lit(Literal::Str("non-exhaustive lambda pattern".into()))),
                                ),
                            });
                            return Ok(Expr::Lambda {
                                params: vec!["__lam".to_string()],
                                body: Box::new(Expr::Case {
                                    scrutinee: Box::new(Expr::Var("__lam".to_string())),
                                    branches,
                                }),
                            });
                        }
                    // Not a pattern lambda — backtrack
                    self.pos = save;
                    self.current_indent = save_indent;
                }
                let mut params = Vec::new();
                loop {
                    match self.peek().clone() {
                        Token::Ident(name) => {
                            params.push(name);
                            self.advance();
                        }
                        Token::Underscore => {
                            params.push("_".to_string());
                            self.advance();
                        }
                        _ => break,
                    }
                }
                if params.is_empty() {
                    return Err(self.err_here("Expected lambda parameter".to_string()));
                }
                self.expect(&Token::Arrow)?;
                let body = self.parse_expr()?;
                Ok(Expr::Lambda {
                    params,
                    body: Box::new(body),
                })
            }
            _ => {
                Err(self.err_here(format!("Expected expression, found {:?}", self.peek())))
            }
        }
    }

    // --- Pattern parsing ---

    fn parse_pattern(&mut self) -> PResult<Pattern> {
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

    /// Map a function binding's parameter patterns to plain lambda parameter
    /// names (`let f x y = e` => `f = \x y -> e`). Non-variable patterns collapse
    /// to a placeholder, matching the existing let-binding capability.
    fn lambda_param_names(patterns: Vec<Pattern>) -> Vec<String> {
        patterns.into_iter().map(|pat| match pat {
            Pattern::Var(n) => n,
            Pattern::Wildcard => "_".to_string(),
            _ => "_p".to_string(),
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
        if let Token::Operator(op) = self.peek()
            && op == "-" && self.pos + 1 < self.tokens.len() {
                return matches!(self.tokens[self.pos + 1].token, Token::IntLit(_) | Token::NumLit(_));
            }
        false
    }

    fn parse_pattern_atom(&mut self) -> PResult<Pattern> {
        // Negative literal pattern: -N
        if let Token::Operator(op) = self.peek()
            && op == "-" && self.pos + 1 < self.tokens.len() {
                match self.tokens[self.pos + 1].token {
                    Token::IntLit(n) => {
                        self.advance(); self.advance();
                        return Ok(Pattern::LitPat(Literal::Integer(-n)));
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
                Err(self.err_here(format!("Expected pattern, found {:?}", self.peek())))
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
                Err(self.err_here(format!("Expected identifier, found {:?}", self.peek())))
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
                Err(self.err_here(format!("Expected type/constructor name, found {:?}", self.peek())))
            }
        }
    }
}

/// Operator precedence (left binding power, right binding power).
/// Based on Haskell defaults.
/// Convert (Assoc, prec 0-9) to Pratt binding powers (lp, rp).
fn assoc_prec_to_binding(assoc: Assoc, prec: u8) -> (u8, u8) {
    let base = prec * 2;
    match assoc {
        Assoc::Left => (base, base + 1),
        Assoc::Right => (base + 1, base),
        Assoc::None => (base, base + 1), // like left, but could error on chaining
    }
}

fn default_operator_precedence(op: &str) -> (u8, u8) {
    match op {
        ">>=" | ">>" => assoc_prec_to_binding(Assoc::Right, 1),
        "$" => assoc_prec_to_binding(Assoc::Right, 0),
        "||" => assoc_prec_to_binding(Assoc::Right, 2),
        "&&" => assoc_prec_to_binding(Assoc::Right, 3),
        "==" | "/=" | "<" | ">" | "<=" | ">=" => assoc_prec_to_binding(Assoc::None, 4),
        ":" => assoc_prec_to_binding(Assoc::Right, 5),
        "++" => assoc_prec_to_binding(Assoc::Right, 5),
        "<>" => assoc_prec_to_binding(Assoc::Right, 6),
        "+" | "-" => assoc_prec_to_binding(Assoc::Left, 6),
        "*" | "/" => assoc_prec_to_binding(Assoc::Left, 7),
        "^" => assoc_prec_to_binding(Assoc::Right, 8),
        "." => assoc_prec_to_binding(Assoc::Right, 9),
        "!!" => assoc_prec_to_binding(Assoc::Left, 9),
        _ => assoc_prec_to_binding(Assoc::Left, 9), // default high precedence
    }
}

/// Estimate the source length of a token for adjacency checks.
fn token_len(tok: &Token) -> usize {
    match tok {
        Token::Ident(s) | Token::UpperIdent(s) | Token::StrLit(s) => s.len(),
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
/// enclosing parentheses. Used to enforce the LuaCatch/LuaIOCatch result shape,
/// so a captured Lua error has a `Left String` slot to land in.
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

/// Parse a token stream into a module. On failure, returns every syntax
/// error found (the parser recovers at declaration boundaries), in source
/// order; the list is never empty.
pub fn parse(tokens: &[Located]) -> Result<Module, Vec<Diagnostic>> {
    let mut parser = Parser::new(tokens.to_vec());
    parser.parse_module()
}
