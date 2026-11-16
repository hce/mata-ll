use std::collections::HashMap;
use crate::ast::*;
use crate::lexer::{Located, Token};
use crate::types::Diagnostic;

/// Internal parser result. The error is boxed because `Diagnostic` is large
/// (it embeds `Ty`s) and would otherwise bloat every parser `Result` on the
/// stack (clippy::result_large_err). The public `parse` unboxes into a
/// `Vec<Diagnostic>`.
type PResult<T> = Result<T, Box<Diagnostic>>;

/// What a declaration of a binding group begins with, as `parse_decl_head`
/// reads it — the left-hand side of Haskell 2010 §4.4.3's
/// `decl → gendecl | (funlhs | pat) rhs`, the same in every declaration
/// group: the module's top level, a `where` or `let` group, a class body,
/// an instance body.
enum DeclHead {
    /// `vars ::` — the names of a type signature (`f`, `(<+>)`, `a, b`),
    /// each with the token that spells it; the `::` is the next token.
    Signature(Vec<(String, usize)>),
    /// A `funlhs`: the function being defined and its parameter patterns.
    Function(FunLhs),
    /// A `pat` that is no `funlhs`: the left-hand side of a pattern binding.
    Pattern(Pattern),
}

/// A function left-hand side, Haskell 2010 §4.4.3:
/// `funlhs → var apat {apat} | pat varop pat | ( funlhs ) apat {apat}`.
/// All three forms define `name` over `patterns`; a variable binding
/// (`x = e`) is the first form with no parameters.
struct FunLhs {
    name: String,
    /// The position the equation is reported at: the first parameter (or
    /// the `=`) of a prefix head, the left operand of an infix one.
    span: Span,
    patterns: Vec<Pattern>,
    /// See `Clause::operand_cons`.
    operand_cons: Vec<HeadOperand>,
}

/// List comprehension qualifier (internal to parser, desugared before AST)
enum ListCompQual {
    Generator { pattern: Pattern, expr: Expr },
    Guard(Expr),
    /// `let decls` — binds for the body and every later qualifier.
    Let(Vec<LocalDef>),
}

/// A parsed guard chain. `Plain` is the all-boolean form every stage
/// downstream understands (comma qualifiers folded to `&&`, exactly as
/// before); `Raw` appears when a chain uses a BINDING qualifier (Haskell
/// 2010 §3.13 pattern guards / `let`) and is lowered by the parser itself
/// — immediately where the construct is local (case alternatives, where
/// bindings), or at end of parse for function clause groups, whose
/// fall-through needs the clauses that follow.
enum GuardChain {
    Plain(Vec<Guard>),
    Raw(Vec<RawGuard>),
}

impl GuardChain {
    fn is_empty(&self) -> bool {
        match self {
            GuardChain::Plain(g) => g.is_empty(),
            GuardChain::Raw(r) => r.is_empty(),
        }
    }
}

/// One equation of a local binding group — a `where` block, a
/// let-expression, a do-`let`, a guard or comprehension `let` — as parsed:
/// its name, parameter patterns, right-hand side, its own `where` block
/// and the location of its head. Equations are collected first and lowered
/// to `LocalDef`s only when the group closes (`close_local_group`): the
/// equations of one function are grouped by Haskell 2010 §4.4.3.1, and a
/// guarded equation's fall-through target is the NEXT equation, which is
/// not known while the equation itself is parsed.
/// A top-level pattern binding, as `parse_module` remembers it until the
/// module is read: the scrutinee binding it lowered to still needs its
/// type signature (`top_pattern_signatures`).
struct TopPattern {
    /// The fresh name of the scrutinee binding.
    scrutinee: String,
    /// The variables the pattern binds, in source order.
    vars: Vec<String>,
    /// The pattern as written, and where.
    text: String,
    span: Span,
}

struct LocalEquation {
    name: String,
    patterns: Vec<Pattern>,
    rhs: Rhs,
    where_binds: Vec<LocalDef>,
    span: Span,
    /// See `Clause::operand_cons`.
    operand_cons: Vec<HeadOperand>,
}

/// The right-hand side of a declaration (Haskell 2010 §4.4.3 `rhs`):
/// `= body`, or a guard chain.
enum Rhs {
    Body(Expr),
    Guards(GuardChain),
}

/// An entry of a local binding group in source order: an equation, or a
/// pattern binding (`(a, b) = e`, `Just k = e`) already lowered to its
/// scrutinee and selector bindings, with the names it binds (for the
/// conflicting-definitions check) and where it binds them.
enum LocalEntry {
    Equation(LocalEquation),
    Lowered { defs: Vec<LocalDef>, bound: Vec<(String, Span)> },
}

/// One clause of a clause group being collapsed into a single match over
/// fresh parameters (`collapse_match_rows`): its patterns, right-hand side
/// and `where` block. A top-level function clause and a local equation
/// both reduce to this.
struct MatchRow {
    patterns: Vec<Pattern>,
    rhs: RowRhs,
    where_binds: Vec<LocalDef>,
}

enum RowRhs {
    Body(Expr),
    Plain(Vec<Guard>),
    Raw(Vec<RawGuard>),
}

/// What a module's source declares a top-level name as: the kind of one
/// entry of the parser's declaration table (`Parser::declared`). The table
/// exists to enforce that one module's source declares each name at most
/// once per namespace (`check_declarations`).
#[derive(Clone, PartialEq, Eq)]
enum DeclaredAs {
    /// A function or value binding — one contiguous group of equations.
    Binding,
    /// A method of the named class (its signature line in the class body).
    Method(String),
    /// A record field of the named data type or newtype: the field's
    /// selector is a top-level function of that name.
    Field(String),
    Data,
    Newtype,
    Synonym,
    Family,
    Class,
    /// A type signature: `f :: T` (an FFI import is one, its result type
    /// names the Lua callee) or `intrinsic f :: T`.
    Signature,
    /// An `export f :: T` declaration. It IS the type signature of `f`
    /// (`parse_export_decl` emits the `TypeSig`), so it occupies the
    /// signature namespace like the plain form: a name takes one
    /// signature-bearing declaration.
    ExportSignature,
    /// A fixity declaration's entry for one operator.
    Fixity,
}

/// The namespaces of the declaration table. A name may be declared once in
/// each: `data Foo` and a function `foo`, or a signature and its binding,
/// do not clash; types and classes share one namespace, as do functions,
/// class methods and record fields.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum DeclSpace {
    Value,
    Type,
    Signature,
    Fixity,
}

impl DeclaredAs {
    fn space(&self) -> DeclSpace {
        match self {
            DeclaredAs::Binding | DeclaredAs::Method(_) | DeclaredAs::Field(_) => DeclSpace::Value,
            DeclaredAs::Data
            | DeclaredAs::Newtype
            | DeclaredAs::Synonym
            | DeclaredAs::Family
            | DeclaredAs::Class => DeclSpace::Type,
            DeclaredAs::Signature | DeclaredAs::ExportSignature => DeclSpace::Signature,
            DeclaredAs::Fixity => DeclSpace::Fixity,
        }
    }

    /// The declaration in prose, for the middle of a message.
    fn describe(&self) -> String {
        match self {
            DeclaredAs::Binding => "a top-level definition".to_string(),
            DeclaredAs::Method(class) => format!("a method of class '{}'", class),
            DeclaredAs::Field(ty) => format!("a record field of '{}'", ty),
            DeclaredAs::Data => "a data type".to_string(),
            DeclaredAs::Newtype => "a newtype".to_string(),
            DeclaredAs::Synonym => "a type synonym".to_string(),
            DeclaredAs::Family => "a type family".to_string(),
            DeclaredAs::Class => "a class".to_string(),
            DeclaredAs::Signature => "a type signature".to_string(),
            DeclaredAs::ExportSignature => "an export declaration".to_string(),
            DeclaredAs::Fixity => "a fixity declaration".to_string(),
        }
    }
}

/// One entry of the declaration table: a name, what the source declares it
/// as, and where (the declaring token).
struct Declared {
    name: String,
    kind: DeclaredAs,
    span: Span,
}

/// The innermost layout context (Haskell 2010 §10.3). The lexer marks
/// every line start with an `Indent(n)` token, `n` the line's indentation;
/// what such a mark means is decided here, against this context, by the
/// three cases of the layout algorithm: a line indented PAST an implicit
/// block's column continues the block's current item (the mark is
/// whitespace), a line AT the column starts the block's next item (the
/// mark is the `;` between items), and a line indented LESS closes the
/// block (the mark is its `}`, and is then re-read against the enclosing
/// context). The parser's token access (`peek`, `advance`, `at`, …) treats
/// a continuation mark as whitespace, so no parse site has to know that a
/// construct may span lines; only the block loops see the other two.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Layout {
    /// An implicit block whose items start at this 0-based column: the
    /// module's declarations, the members of a class or instance body,
    /// the constructors of a GADT, the equations of a type family, the
    /// bindings of a `where` or `let`, the statements of a `do`, the
    /// alternatives of a `case`.
    Block(usize),
    /// No implicit block is open: the module header before its `where`,
    /// and the inside of any `{ … }` pair — a record construction, update
    /// or declaration body as much as an explicit `case … of { … }` —
    /// where line starts mean nothing at all. Haskell 2010 §10.3 pushes
    /// the context 0 on every `{` (`L ({ : ts) ms = { : L ts (0 : ms)`),
    /// since the lexer cannot tell a record brace from a layout brace, and
    /// inserts no `;` or `}` until the matching `}` pops it; that is what
    /// lets a record body stand at, or left of, the enclosing block's
    /// column. Parentheses and brackets get no such context.
    Explicit,
}

struct Parser {
    tokens: Vec<Located>,
    pos: usize,
    /// The innermost layout context; see [`Layout`]. Every construct that
    /// opens an implicit block sets it for the block's extent and restores
    /// the enclosing context afterwards (`in_layout`); a `{` consumed by
    /// `advance` makes it `Explicit` until the matching `}`.
    layout: Layout,
    /// The contexts suspended by the `{` tokens currently open, innermost
    /// last: `advance` pushes the context in force at each `{` and
    /// restores it at the matching `}` (Haskell 2010 §10.3's `0 : ms`).
    brace_stack: Vec<Layout>,
    /// Current recursion depth of the nesting productions (expressions,
    /// types, patterns). Bounded by `crate::MAX_NESTING_DEPTH` via
    /// `enter_nested`, so absurdly nested input yields a clean diagnostic
    /// instead of overflowing the native stack.
    depth: usize,
    /// Fresh-name counter for the pattern-guard desugar (`__pg{n}` join
    /// points and parameters). One counter for the whole parse keeps every
    /// synthesized name unique across nested desugars.
    pg_fresh: usize,
    /// Fresh-name counter for the scrutinee bindings of TOP-LEVEL pattern
    /// bindings (`__pat_{n}`; see `top_pattern_binding`). A local group
    /// numbers its own from zero: its names are scoped to the group.
    pat_fresh: usize,
    /// The module's top-level pattern bindings, in source order.
    top_patterns: Vec<TopPattern>,
    /// How many let-EXPRESSIONS have parsed their bindings and now await
    /// their `in`. Decides what an `in` at a do block's statement column
    /// means after a `let` statement: with none pending it is the parse
    /// error GHC reports on the token (a do-`let` has no `in`); with one
    /// pending it is that let's `in`, which closes the do block (Haskell's
    /// parse-error(t) layout rule) — and GHC then reports the block
    /// ending in a `let` statement, as `parse_do_block` does.
    let_in_pending: usize,
    /// Token indices of the pattern variables read so far in the current
    /// top-level declaration, in source order — the raw material of the
    /// linearity check (`check_linear`). Every pattern variable is recorded
    /// where it is parsed (`bind_pattern_var`), `rewind` drops the entries
    /// of an abandoned speculative parse, and each construct that binds a
    /// list of patterns at once checks and removes its own entries once it
    /// has committed to them. So the log never holds a token twice, and it
    /// is the only stage that still knows the binder list AS WRITTEN: the
    /// parser's own desugars (pattern lambdas, pattern bindings, pattern
    /// guards, comprehensions) spread one list over several AST nodes.
    pattern_binders: Vec<usize>,
    /// The declaration table: every top-level name this module's source
    /// declares, in source order. Filled as the declarations are parsed
    /// (`declare`), checked once at the end of the module
    /// (`check_declarations`). It covers one source file — the
    /// only place where "declared twice in one module" is decidable, since
    /// import resolution later merges every module into one declaration
    /// list in which a second declaration may legitimately come from
    /// another module.
    declared: Vec<Declared>,
}

/// A parser position to rewind to when a speculative parse fails. Captures
/// BOTH the token cursor and the layout context: a speculative parse that
/// fails inside a block it opened (a record-update field value holding a
/// `do`, say) leaves the block's context set, and restoring `pos` alone
/// would read the following lines against the wrong column.
#[derive(Clone)]
struct Checkpoint {
    pos: usize,
    layout: Layout,
    brace_stack: Vec<Layout>,
}

impl Parser {
    fn new(tokens: Vec<Located>) -> Self {
        Parser {
            tokens,
            pos: 0,
            layout: Layout::Explicit,
            brace_stack: Vec::new(),
            depth: 0,
            pg_fresh: 0,
            pat_fresh: 0,
            top_patterns: Vec::new(),
            let_in_pending: 0,
            pattern_binders: Vec::new(),
            declared: Vec::new(),
        }
    }

    /// Capture the state a failed speculative parse must restore via
    /// `rewind`. See [`Checkpoint`].
    fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            pos: self.pos,
            layout: self.layout,
            brace_stack: self.brace_stack.clone(),
        }
    }

    fn rewind(&mut self, cp: Checkpoint) {
        self.pos = cp.pos;
        self.layout = cp.layout;
        self.brace_stack = cp.brace_stack;
        // Pattern variables read past the checkpoint belong to the parse
        // being abandoned; the tokens will be read again, possibly as a
        // pattern, and must not be logged twice.
        while self.pattern_binders.last().is_some_and(|&tok| tok >= cp.pos) {
            self.pattern_binders.pop();
        }
    }

    /// The source position of the token at index `tok`.
    fn span_of(&self, tok: usize) -> Span {
        let loc = &self.tokens[tok];
        Span::new(loc.line, loc.col)
    }

    /// Record the identifier token at index `tok` as a pattern variable.
    /// Called wherever a pattern variable is born: the variable and
    /// as-pattern arms of `parse_pattern_atom_inner`, through which every
    /// pattern is read — the operands of an infix definition included.
    fn bind_pattern_var(&mut self, tok: usize) {
        debug_assert!(self.pattern_binders.last().is_none_or(|&last| last < tok));
        self.pattern_binders.push(tok);
    }

    /// THE linearity check (Haskell 2010 §3.17.1: a pattern, and the set of
    /// patterns bound together, names each variable at most once). `from`
    /// is the index of the first token of a binder list the caller has just
    /// finished and committed to — the parameters of one function equation
    /// or lambda, or the single pattern of a case alternative, a `<-`
    /// statement, generator or guard, or a pattern binding. Its variables
    /// are taken out of the log and must be pairwise distinct. `context`
    /// names the construct for the message.
    fn check_linear(&mut self, from: usize, context: &str) -> PResult<()> {
        let start = self.pattern_binders.partition_point(|&tok| tok < from);
        let list = self.pattern_binders.split_off(start);
        for (i, &tok) in list.iter().enumerate() {
            let Token::Ident(name) = &self.tokens[tok].token else { continue };
            let first = list[..i].iter().copied().find(|&earlier| {
                matches!(&self.tokens[earlier].token, Token::Ident(n) if n == name)
            });
            if let Some(first) = first {
                let mut diag = Diagnostic::parse_at(
                    format!(
                        "Conflicting definitions for '{}' in {}: bound at {} and again",
                        name, context, self.span_of(first),
                    ),
                    self.span_of(tok),
                );
                diag.notes.push(format!(
                    "the patterns bound together name each variable once: a repeated \
                     name does not compare the two positions, so there is no single \
                     value '{}' could stand for. Bind them under two names and \
                     compare those (a guard '| a == b').",
                    name,
                ));
                return Err(Box::new(diag));
            }
        }
        Ok(())
    }

    /// Enter the name spelled by the token at index `tok` into the
    /// declaration table.
    fn declare(&mut self, name: &str, kind: DeclaredAs, tok: usize) {
        let span = self.span_of(tok);
        self.declared.push(Declared { name: name.to_string(), kind, span });
    }

    /// The single-declaration rule over the declaration table: within one
    /// module's source a name is declared once per namespace (GHC:
    /// "Multiple declarations", "Duplicate type signatures", "Multiple
    /// fixity declarations"). Returns one diagnostic per redeclaration, at
    /// the later declaration, naming the first. The signature namespace
    /// holds every signature-bearing declaration — plain, `intrinsic` and
    /// `export` — so any two of them for one name are a duplicate.
    ///
    /// Three clashes are deliberately left to the typechecker, which
    /// already rejects them when it registers the declarations and has to
    /// keep doing so for the merged program (a second declaration can come
    /// from an import, which this table never sees): a record field
    /// declared by two types, a method declared by two classes, and two
    /// data/newtype/synonym declarations of one name. Reporting them here
    /// too would only give one defect two diagnostics.
    fn check_declarations(&self) -> Vec<Diagnostic> {
        let mut first: HashMap<(DeclSpace, &str), &Declared> = HashMap::new();
        let mut errors = Vec::new();
        for d in &self.declared {
            let prev = match first.entry((d.kind.space(), d.name.as_str())) {
                std::collections::hash_map::Entry::Vacant(slot) => {
                    slot.insert(d);
                    continue;
                }
                std::collections::hash_map::Entry::Occupied(slot) => *slot.get(),
            };
            let plain_type = |k: &DeclaredAs| {
                matches!(k, DeclaredAs::Data | DeclaredAs::Newtype | DeclaredAs::Synonym)
            };
            match (&prev.kind, &d.kind) {
                // Within ONE type, a field shared by several constructors
                // is a single declaration (Haskell 2010 §4.2.1); across two
                // types the clash is the typechecker's (see above), like
                // the two arms below.
                (DeclaredAs::Field(_), DeclaredAs::Field(_)) => continue,
                (DeclaredAs::Method(a), DeclaredAs::Method(b)) if a != b => continue,
                (a, b) if plain_type(a) && plain_type(b) => continue,
                _ => {}
            }
            let pair = if prev.kind == d.kind {
                format!("{} at {} and again", prev.kind.describe(), prev.span)
            } else {
                format!("{} at {} and {}", prev.kind.describe(), prev.span, d.kind.describe())
            };
            let exported = |k: &DeclaredAs| *k == DeclaredAs::ExportSignature;
            let (msg, note) = match d.kind.space() {
                DeclSpace::Signature if exported(&prev.kind) && exported(&d.kind) => (
                    format!("Duplicate type signatures for '{}': {}", d.name, pair),
                    format!(
                        "an 'export' declaration is the type signature of '{}', and a \
                         name has one type signature; remove one of the two.",
                        d.name,
                    ),
                ),
                DeclSpace::Signature if exported(&prev.kind) || exported(&d.kind) => (
                    format!("Duplicate type signatures for '{}': {}", d.name, pair),
                    format!(
                        "in mata-ll 'export {} :: T' already is the type signature of \
                         '{}': it gives the name its type and, in addition, puts it in \
                         the module's Lua return table. A name has one type signature, \
                         whether or not the two types agree; remove the other signature \
                         and keep the export declaration. (Haskell's 'foreign export' \
                         is the opposite: it accompanies the name's own signature.)",
                        d.name, d.name,
                    ),
                ),
                DeclSpace::Signature => (
                    format!("Duplicate type signatures for '{}': at {} and again", d.name, prev.span),
                    "a name has one type signature; remove one of the two.".to_string(),
                ),
                DeclSpace::Fixity => (
                    format!("Multiple fixity declarations for '{}': at {} and again", d.name, prev.span),
                    "an operator has one precedence and associativity for the whole \
                     module, so it takes one fixity declaration; remove one of the two."
                        .to_string(),
                ),
                DeclSpace::Type => (
                    format!("Multiple declarations of '{}': {}", d.name, pair),
                    "types, type families and classes share one namespace, so a module \
                     declares each such name once; rename one of the two."
                        .to_string(),
                ),
                DeclSpace::Value if prev.kind == DeclaredAs::Binding && d.kind == DeclaredAs::Binding => (
                    format!("Multiple declarations of '{}': {}", d.name, pair),
                    format!(
                        "a module defines each name once. Equations form ONE function \
                         only when they follow each other directly and the first takes \
                         arguments; anything between them — another declaration, or \
                         the type signature of '{}' itself — ends the function, and the \
                         next equation starts a second definition. Write the equations \
                         of '{}' together, or rename one definition.",
                        d.name, d.name,
                    ),
                ),
                DeclSpace::Value => (
                    format!("Multiple declarations of '{}': {}", d.name, pair),
                    "functions, class methods and record fields share one namespace: \
                     a class method and a record field are top-level functions of \
                     their name, so a module declares each such name once; rename \
                     one of the two."
                        .to_string(),
                ),
            };
            let mut diag = Diagnostic::parse_at(msg, d.span);
            diag.notes.push(note);
            errors.push(diag);
        }
        errors
    }

    /// Is the token at index `i` a line-start mark that the current layout
    /// context reads as whitespace — a line indented past the innermost
    /// implicit block's column (a continuation of the block's current
    /// item), or any line start where no implicit block is open?
    fn is_continuation_mark(&self, i: usize) -> bool {
        match (&self.tokens[i].token, self.layout) {
            (Token::Indent(n), Layout::Block(col)) => *n > col,
            (Token::Indent(_), Layout::Explicit) => true,
            _ => false,
        }
    }

    /// The index of the first token at or after `i` that the layout
    /// context does not read as whitespace. The lexer never emits two
    /// line-start marks in a row, so this moves at most one step.
    fn visible_from(&self, mut i: usize) -> usize {
        while self.is_continuation_mark(i) {
            i += 1;
        }
        i
    }

    /// The index of the current token — the one `peek` returns. Every
    /// place that remembers "the token here" for a later span or a
    /// pattern-binder record takes this, not `pos`, which may rest on a
    /// continuation mark.
    fn cur(&self) -> usize {
        self.visible_from(self.pos)
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.cur()].token
    }

    fn peek_loc(&self) -> &Located {
        &self.tokens[self.cur()]
    }

    /// The token `k` visible tokens past the current one (`nth(0)` is the
    /// current token); past the end of the stream it is the EOF token.
    fn nth(&self, k: usize) -> &Located {
        let mut i = self.cur();
        for _ in 0..k {
            if matches!(self.tokens[i].token, Token::EOF) {
                break;
            }
            i = self.visible_from(i + 1);
        }
        &self.tokens[i]
    }

    /// Consume the current token. A `{` suspends the layout context until
    /// the matching `}` (see [`Layout::Explicit`]); every brace the parser
    /// reads — record construction, update and declaration bodies, an
    /// explicit `case` block — passes through here, so no parse site has
    /// to know. An unmatched `}` (an error the caller reports) leaves the
    /// context as it is.
    fn advance(&mut self) -> &Token {
        let i = self.cur();
        self.pos = i + 1;
        match &self.tokens[i].token {
            Token::LeftBrace => {
                self.brace_stack.push(self.layout);
                self.layout = Layout::Explicit;
            }
            Token::RightBrace => {
                if let Some(outer) = self.brace_stack.pop() {
                    self.layout = outer;
                }
            }
            _ => {}
        }
        &self.tokens[i].token
    }

    /// Does the current token start its source line? (The raw token before
    /// it is the line's start mark.)
    fn starts_line(&self) -> bool {
        let i = self.cur();
        i > 0 && matches!(self.tokens[i - 1].token, Token::Indent(_))
    }

    /// Run `f` under the layout context `layout`, restoring the enclosing
    /// context afterwards whether `f` succeeds or fails — the block
    /// constructs all go through here, so an error inside a block never
    /// leaves its column in force for what follows.
    fn in_layout<T>(
        &mut self,
        layout: Layout,
        f: impl FnOnce(&mut Self) -> PResult<T>,
    ) -> PResult<T> {
        let saved = self.layout;
        self.layout = layout;
        let r = f(self);
        self.layout = saved;
        r
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
            return Err(nesting_error(what, self.span_of(self.cur())));
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
            Err(self.err_expected(&expected.to_string()))
        }
    }

    /// "Expected X, found Y" at the current token — unless the current
    /// token is a line-start mark, i.e. the construct being parsed is
    /// incomplete and the next line does NOT continue it because it is
    /// not indented past the enclosing block's column. That is GHC's
    /// `parse error on input 't'` at the next line's first token, and the
    /// note says which column the line would have to clear.
    fn err_expected(&self, what: &str) -> Box<Diagnostic> {
        match (self.peek(), self.layout) {
            (Token::Indent(n), Layout::Block(col)) => {
                let next = self.nth(1);
                let mut diag = Box::new(Diagnostic::parse_at(
                    format!("parse error on input {}", next.token),
                    Span::new(next.line, next.col),
                ));
                diag.notes.push(format!(
                    "the construct on the line above is incomplete ({what} expected), \
                     but this line does not continue it: it starts at column {}, \
                     which is not past column {} (the column of the enclosing block's \
                     items — its declarations, bindings, statements or alternatives). \
                     The layout rule reads a line at that column as the block's next \
                     item, and a line left of it as the end of the block; only a line \
                     indented past column {} continues the item above it.",
                    n + 1,
                    col + 1,
                    col + 1,
                ));
                diag
            }
            (found, _) => self.err_here(format!("Expected {what}, found {found}")),
        }
    }

    /// GHC's `parse error on input 't'` for a token that nothing open can
    /// take: no construct continues with it and no block's item begins
    /// with it. When the token starts its line, the layout rule is the
    /// likely reason (an operator meant to continue the line above, placed
    /// at the enclosing block's column) and the note spells it out.
    fn parse_error_on_input(&self) -> Box<Diagnostic> {
        // At a line-start mark (a line indented less than the enclosing
        // block's items, with no outer context left to take it) the
        // offending token is that line's first.
        let at_mark = matches!(self.peek(), Token::Indent(_));
        let loc = if at_mark { self.nth(1) } else { self.peek_loc() };
        let mut diag = Box::new(Diagnostic::parse_at(
            format!("parse error on input {}", loc.token),
            Span::new(loc.line, loc.col),
        ));
        if at_mark || self.starts_line() {
            diag.notes.push(format!(
                "this line starts at column {}. The layout rule reads a line at the \
                 column of an enclosing block's items (its declarations, bindings, \
                 statements or alternatives) as that block's next item, and a line \
                 indented less as the end of the block; only a line indented past \
                 the item column continues the item above it. Nothing that can \
                 begin or continue here starts with {}.",
                loc.col,
                loc.token,
            ));
        }
        diag
    }

    fn at(&self, tok: &Token) -> bool {
        self.peek() == tok
    }

    /// Skip an export entry's optional constructor sub-spec `(..)` (or an
    /// explicit constructor list) — `T(..)`, `(:+:)(..)`. The contents are
    /// not recorded: mata-ll's export lists control visibility per name,
    /// and the sub-spec's constructors travel with the type.
    /// The parenthesized part of a type or class export entry, if any:
    /// `Some(None)` for `(..)`, `Some(Some(names))` for an explicit list of
    /// constructors, fields or methods (an operator written in
    /// parentheses), `None` when the entry has no parentheses.
    fn parse_export_subspec(&mut self) -> Option<Option<Vec<String>>> {
        if !self.at(&Token::LeftParen) {
            return None;
        }
        self.advance();
        let mut names = Vec::new();
        let mut all = false;
        let mut depth = 0usize;
        while !(depth == 0 && self.at(&Token::RightParen)) && !self.at_eof() {
            match self.peek().clone() {
                Token::LeftParen => depth += 1,
                Token::RightParen => depth -= 1,
                Token::Operator(op) if op == ".." => all = true,
                Token::Ident(n) | Token::UpperIdent(n) | Token::Operator(n) => names.push(n),
                _ => {}
            }
            self.advance();
        }
        if self.at(&Token::RightParen) { self.advance(); }
        Some(if all { None } else { Some(names) })
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek(), Token::EOF)
    }

    /// Is the current token the layout `;` of the innermost implicit block
    /// — the start of a line at exactly the block's column?
    fn at_separator(&self) -> bool {
        matches!((self.peek(), self.layout), (Token::Indent(n), Layout::Block(col)) if *n == col)
    }

    /// Consume the layout `;` before the block's next item, if the current
    /// token is one. Only the block loops call this; everything else sees
    /// the mark as the token it is and stops in front of it.
    fn skip_separators(&mut self) {
        while self.at_separator() {
            self.advance();
        }
    }

    /// Does the innermost implicit block end here? At the end of the
    /// file, or at a line-start mark the context does not read as
    /// whitespace — after `skip_separators` that is a line indented LESS
    /// than the block's column, the block's layout `}`. The mark is left
    /// in place: the enclosing context reads it next, as its own
    /// continuation, separator or closer (Haskell 2010 §10.3, the `n < m`
    /// case re-reads `<n>` against the outer context).
    fn at_block_end(&self) -> bool {
        matches!(self.peek(), Token::EOF | Token::Indent(_))
    }

    /// The token at the start of the block's next item, if the current
    /// token is a layout `;` — what the next line begins with, without
    /// consuming the separator.
    fn after_separator(&self) -> Option<&Token> {
        self.at_separator().then(|| &self.nth(1).token)
    }

    /// Open the implicit block that follows a layout keyword (`where`,
    /// `let`, `do`, `of`), the keyword already consumed. Haskell 2010
    /// §10.3: the block's column is the column of the next token, provided
    /// that token is indented past the enclosing block's column — on the
    /// keyword's own line it always is; on a later line the layout context
    /// has already decided: a continuation mark (read as whitespace) means
    /// the token is indented past the enclosing block, while a visible
    /// mark means it is not, and the block is EMPTY (`{}`), the line
    /// belonging to the enclosing context. So an empty `class C a where`
    /// does not swallow the following declarations as methods, and a
    /// `where` alone on its line does not swallow the next definition as a
    /// binding. Returns the block's column — the caller parses the block's
    /// items under `in_layout(Layout::Block(col), …)` — or `None` for an
    /// empty block.
    fn open_block(&self) -> Option<usize> {
        if self.at_block_end() {
            return None;
        }
        Some(self.peek_loc().col.saturating_sub(1))
    }

    /// Is the next token one that ends an implicit layout block from
    /// INSIDE its line — the closing bracket of an enclosing paren/list/
    /// record, the separator of an enclosing tuple/list, or the keyword of
    /// an enclosing construct? Haskell's layout algorithm closes the block
    /// on the parse error such a token would cause (the `parse-error(t)`
    /// rule); the block loops test for these explicitly, so `(do a; b, 2)`,
    /// `[do …, …]`, `if c then do … else …` and `let x = do … in …` end
    /// the block where GHC does — also when the token stands at the
    /// block's own column, where the layout `;` precedes it (`let x = 1`
    /// with `in x` under the `x`).
    fn at_block_closer(&self) -> bool {
        matches!(
            self.peek(),
            Token::RightParen | Token::RightBracket | Token::RightBrace | Token::Comma
                | Token::Then | Token::Else | Token::Of | Token::In | Token::Where
        )
    }

    /// Parse a whole module: the optional `module … where` header, then
    /// top-level declarations at the module block's column (import decls,
    /// signatures, bindings, data/newtype/class/instance/type-family/fixity
    /// decls). Adjacent
    /// equations of one function are merged into one FunDef (`push_decl`);
    /// a declaration that fails to parse is recorded and parsing resumes at
    /// the next declaration at that column, so every declaration's error is
    /// reported at once. A source free of syntax errors is then checked for
    /// names it declares twice (`check_declarations`).
    fn parse_module(&mut self) -> Result<Module, Vec<Diagnostic>> {
        let mut decls = Vec::new();

        // The header stands outside any layout block (Haskell 2010 §10.3
        // opens the module's block at the token after `where`), so its
        // lines may be indented any way at all: `layout` starts out
        // `Explicit`, every line start is whitespace.
        //
        // Parse optional `module Name (exports) where` header
        let mut module_name: Option<String> = None;
        let mut module_exports: Option<ModuleExports> = None;
        if self.at(&Token::KwModule) {
            self.advance();
            // Module name: a dotted one (`Data.List`) is one lexeme.
            if let Token::UpperIdent(name) | Token::QualUpperIdent(name) = self.peek().clone() {
                self.advance();
                module_name = Some(name);
            }
            // Parse optional export list
            if self.at(&Token::LeftParen) {
                self.advance();
                let mut exports = Vec::new();
                let mut reexports = Vec::new();
                let mut subs = std::collections::HashMap::new();
                loop {
                    if self.at(&Token::RightParen) { break; }
                    match self.peek().clone() {
                        Token::Ident(n) => { exports.push(n); self.advance(); }
                        Token::UpperIdent(n) => {
                            self.advance();
                            if let Some(sub) = self.parse_export_subspec() {
                                subs.insert(n.clone(), sub);
                            }
                            exports.push(n);
                        }
                        // Module re-export: `module X` (Haskell 2010 §5.2),
                        // X possibly dotted. Which names that passes on is
                        // decided during import resolution, where the
                        // matching import edge is known.
                        Token::KwModule => {
                            self.advance();
                            reexports.push(self.expect_module_name().map_err(|d| vec![*d])?);
                        }
                        Token::Operator(op) => { exports.push(op); self.advance(); }
                        // Parenthesized operator export, GHC style:
                        // `module M ((-.), (~=~)) where`.
                        Token::LeftParen => {
                            self.advance();
                            let mut op_name = None;
                            if let Token::Operator(op) = self.peek().clone() {
                                exports.push(op.clone());
                                op_name = Some(op);
                                self.advance();
                            }
                            if self.at(&Token::RightParen) { self.advance(); }
                            // A type operator can carry its constructors:
                            // `(:+:)(..)` exports the type and everything it
                            // declares, exactly like `T(..)`.
                            if let Some(sub) = self.parse_export_subspec()
                                && let Some(op) = op_name
                            {
                                subs.insert(op, sub);
                            }
                        }
                        other => {
                            let diag = self.err_here(format!(
                                "This export-list entry is not understood \
                                 (found {}). An entry names a value, a \
                                 type (optionally with '(..)'), an \
                                 operator in parentheses, or a module \
                                 re-export ('module M').",
                                other
                            ));
                            return Err(vec![*diag]);
                        }
                    }
                    if self.at(&Token::Comma) { self.advance(); } else { break; }
                }
                if self.at(&Token::RightParen) { self.advance(); }
                module_exports = Some(ModuleExports { names: exports, reexports, subs });
            }
            if self.at(&Token::Where) {
                self.advance();
            }
        }

        // The module's block opens at the first declaration's column
        // (Haskell 2010 §10.3: the token after `where`, or the module's
        // first token when it has no header) — column 0 in practice.
        let top = if self.at_eof() { 0 } else { self.peek_loc().col.saturating_sub(1) };
        self.layout = Layout::Block(top);

        // Parse declarations, recovering at declaration boundaries: a failed
        // declaration records its diagnostic and parsing resumes at the next
        // line at the declaration column that can start a declaration, so
        // one run reports every independent syntax error instead of only
        // the first.
        let mut errors: Vec<Diagnostic> = Vec::new();
        loop {
            self.skip_separators();
            if self.at_eof() {
                break;
            }
            let started_at = self.cur();
            self.pattern_binders.clear();
            let result = self.parse_decl().and_then(|parsed| {
                // The declaration is complete. The next token must begin a
                // new line at the declaration column (the block's `;`) or
                // end the file; anything else stands on a continuation
                // line that the declaration has no grammar left for —
                // GHC's parse error on that token.
                if self.at_eof() || self.at_separator() {
                    Ok(parsed)
                } else {
                    Err(self.parse_error_on_input())
                }
            });
            match result {
                Ok(parsed) => {
                    for decl in parsed {
                        self.push_decl(&mut decls, decl, started_at);
                    }
                }
                Err(e) => {
                    errors.push(*e);
                    // The context is the module's again: every block parses
                    // its items under `in_layout`, which restores the
                    // enclosing context on the error path too; a brace the
                    // failed declaration left open is forgotten with it.
                    self.layout = Layout::Block(top);
                    self.brace_stack.clear();
                    self.recover_to_next_decl(started_at, top);
                }
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        // With the whole source read, every name it declares is in the
        // table: reject the ones declared twice before any later stage
        // can let the last declaration silently replace the first.
        let redeclared = self.check_declarations();
        if !redeclared.is_empty() {
            return Err(redeclared);
        }
        let mut merged = decls;
        // With every signature read, the scrutinee bindings of the
        // top-level pattern bindings get theirs.
        merged.extend(self.top_pattern_signatures(&merged)?);

        // Lower pattern-guard chains now that every clause group is
        // complete (§3.13 fall-through needs the following clauses) —
        // see desugar_pattern_guard_clauses. After this walk no clause
        // anywhere carries `raw_guards`.
        for decl in &mut merged {
            let r = match decl {
                Decl::FunDef { name, clauses } => {
                    let name = name.clone();
                    self.desugar_pattern_guard_clauses(&name, clauses)
                }
                Decl::ClassDecl { methods, .. } => methods.iter_mut().try_for_each(|m| {
                    let name = m.name.clone();
                    match &mut m.default_clauses {
                        Some(cs) => self.desugar_pattern_guard_clauses(&name, cs),
                        None => Ok(()),
                    }
                }),
                Decl::InstanceDecl { methods, .. } => methods.iter_mut().try_for_each(|m| {
                    let name = m.name.clone();
                    self.desugar_pattern_guard_clauses(&name, &mut m.clauses)
                }),
                _ => Ok(()),
            };
            if let Err(e) = r {
                return Err(vec![*e]);
            }
        }

        Ok(Module { decls: merged, name: module_name, exports: module_exports, resolution: Resolution::default(), origin_spans: Vec::new() })
    }

    /// Append one parsed declaration (which began at token `started_at`)
    /// to the module's list. A function equation either CONTINUES the
    /// function the previous declaration was defining or starts a new
    /// binding — the Haskell 2010 §4.4.3.1 rule, as GHC implements it: the
    /// equations of one function are adjacent, and only a first equation
    /// that takes arguments opens a group (`k = 1` is complete on its
    /// own). A new binding enters the declaration table, so an equation
    /// separated from its function by any other declaration — the
    /// function's own type signature included — is found there as a second
    /// definition of the name instead of silently replacing the first.
    fn push_decl(&mut self, decls: &mut Vec<Decl>, decl: Decl, started_at: usize) {
        if let Decl::FunDef { name, clauses } = decl {
            if let Some(Decl::FunDef { name: prev, clauses: prev_clauses }) = decls.last_mut()
                && prev_clauses.first().is_some_and(|c| Self::continues_function(prev, &c.patterns, &name))
            {
                prev_clauses.extend(clauses);
            } else {
                self.declare(&name, DeclaredAs::Binding, started_at);
                decls.push(Decl::FunDef { name, clauses });
            }
        } else {
            decls.push(decl);
        }
    }

    /// True when `tok` can begin a top-level declaration — the resync points
    /// for parser error recovery.
    fn starts_decl(tok: &Token) -> bool {
        matches!(tok,
            Token::Data | Token::Newtype | Token::Import | Token::Class
            | Token::Instance | Token::KwType | Token::Intrinsic | Token::Export
            | Token::Infixl | Token::Infixr | Token::Infix
            // What a signature, a function binding or a pattern binding
            // begins with (`parse_binding_decl`): a variable, a
            // parenthesis, or any other pattern.
            | Token::Ident(_) | Token::LeftParen
            | Token::UpperIdent(_) | Token::QualUpperIdent(_) | Token::LeftBracket
            | Token::Underscore | Token::IntLit(_) | Token::BigIntLit(_)
            | Token::NumLit(_) | Token::StrLit(_))
    }

    /// After a declaration fails to parse, skip forward to the next plausible
    /// declaration start: a line at the declaration column `top` whose first
    /// token can begin a declaration. `started_at` is where the failed
    /// declaration began; recovery always moves past it, so a failure on a
    /// declaration's very first token cannot loop. Scans the raw stream:
    /// the layout context is the module's again, under which that line's
    /// mark is the separator the declaration loop consumes.
    fn recover_to_next_decl(&mut self, started_at: usize, top: usize) {
        if self.pos <= started_at {
            self.pos = started_at + 1;
        }
        while self.pos < self.tokens.len() && !matches!(self.tokens[self.pos].token, Token::EOF) {
            if matches!(self.tokens[self.pos].token, Token::Indent(n) if n == top)
                && self.tokens.get(self.pos + 1).is_some_and(|next| Self::starts_decl(&next.token))
            {
                return;
            }
            self.pos += 1;
        }
    }

    fn parse_decl(&mut self) -> PResult<Vec<Decl>> {
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
            // `O.plus a b = …` binds nothing (see `qualified_binder_error`);
            // `O.plus :: T` is GHC's "Invalid qualified name in type
            // signature" — a signature names a declaration of this module.
            Token::QualIdent(name) => {
                if self.nth(1).token == Token::DblColon {
                    let mut diag = self.err_here(format!(
                        "Invalid qualified name in type signature: {name}"
                    ));
                    diag.notes.push(format!(
                        "a signature belongs to a declaration of this module, \
                         written unqualified: '{} :: …'",
                        crate::lexer::unqualified(&name)
                    ));
                    return Err(diag);
                }
                Err(self.qualified_binder_error(&name))
            }
            // Everything else that is a declaration is `gendecl | (funlhs |
            // pat) rhs` — a signature, a function binding or a pattern
            // binding — and begins like a pattern.
            _ if self.starts_binding() => self.parse_binding_decl(),
            // A token no declaration begins with — or a line indented LESS
            // than the module's declarations, the block's `}` with nothing
            // outside it to take the line.
            _ => Err(self.parse_error_on_input()),
        }
    }

    fn parse_data_decl(&mut self) -> PResult<Decl> {
        self.expect(&Token::Data)?;
        // The type name is normally an UpperIdent, but a type OPERATOR is
        // declared in prefix parenthesised form — `data (:+:) a b = …` — just
        // like an operator value binding (`(<>) a b = …`). Both spellings then
        // take the same type-variable list.
        let name_tok = self.cur();
        let name = self.parse_type_con_name()?;
        self.declare(&name, DeclaredAs::Data, name_tok);

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
            // fields, result-type indices and existentials from it). They
            // form an implicit layout block like a class body's members.
            let mut constructors = Vec::new();
            if let Some(col) = self.open_block() {
                self.in_layout(Layout::Block(col), |p| {
                    loop {
                        p.skip_separators();
                        if p.at_block_end() || p.at_block_closer() {
                            break;
                        }
                        // A line at the constructors' column that is not a
                        // constructor signature closes the block (GHC's
                        // parse-error(t) rule) — a `deriving` clause, say.
                        if !matches!(p.peek(), Token::UpperIdent(_)) {
                            break;
                        }
                        let con_name = p.expect_upper_ident()?;
                        p.expect(&Token::DblColon)?;
                        let ty = p.parse_type()?;
                        constructors.push(Constructor {
                            name: con_name,
                            external_name: None,
                            fields: ConstructorFields::Positional(vec![]),
                            gadt_type: Some(ty),
                            existential_vars: vec![],
                            existential_constraints: vec![],
                        });
                    }
                    Ok(())
                })?;
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
        constructors.push(self.parse_constructor(&name)?);
        while self.at(&Token::Pipe) {
            self.advance();
            constructors.push(self.parse_constructor(&name)?);
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

    /// One constructor of the data type `type_name` (the `=`/`|` form).
    fn parse_constructor(&mut self, type_name: &str) -> PResult<Constructor> {
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

        // Record syntax; the brace may open on a continuation line.
        if self.at(&Token::LeftBrace) {
            self.advance();
            let mut fields: Vec<crate::ast::RecordField> = Vec::new();
            // The name token of each field, parallel to `fields`.
            let mut field_toks: Vec<usize> = Vec::new();
            loop {
                if self.at(&Token::RightBrace) {
                    break;
                }
                let field_tok = self.cur();
                let field_name = self.expect_ident()?;
                // One constructor names each of its fields once: a second
                // `x ::` would be a second component under the same label,
                // which no selector, record pattern or update could tell
                // from the first.
                if let Some(i) = fields.iter().position(|f| f.name == field_name) {
                    let mut diag = Diagnostic::parse_at(
                        format!(
                            "Duplicate field name '{}' in the record declaration of '{}': \
                             at {} and again",
                            field_name, name, self.span_of(field_toks[i]),
                        ),
                        self.span_of(field_tok),
                    );
                    diag.notes.push(
                        "a field name labels one component of a constructor; rename \
                         one of the two fields."
                            .to_string(),
                    );
                    return Err(Box::new(diag));
                }
                field_toks.push(field_tok);
                self.declare(&field_name, DeclaredAs::Field(type_name.to_string()), field_tok);
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
                if self.at(&Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
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

    /// Parse optional `deriving (Show, Eq)` clause after a data declaration
    /// (on the declaration's line, or on a continuation line — the layout
    /// context makes the difference, as everywhere).
    fn parse_deriving(&mut self) -> PResult<Vec<String>> {
        if !self.at(&Token::Deriving) {
            return Ok(vec![]);
        }
        self.advance(); // consume 'deriving'

        let mut classes = Vec::new();
        if self.at(&Token::LeftParen) {
            self.advance();
            loop {
                if self.at(&Token::RightParen) {
                    self.advance();
                    break;
                }
                classes.push(self.expect_upper_ident()?);
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
        let name_tok = self.cur();
        let name = self.expect_upper_ident()?;
        self.declare(&name, DeclaredAs::Newtype, name_tok);

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
        // The record brace may sit on a continuation line; `nth` reads
        // past the line start like every other lookahead.
        let braced_con = matches!(self.peek(), Token::UpperIdent(_))
            && self.nth(1).token == Token::LeftBrace;
        if let Token::UpperIdent(con) = self.peek().clone()
            && (con == name || braced_con)
        {
            self.advance();
            con_name = Some(con);
            if self.at(&Token::LeftBrace) {
                // Record form: exactly one selector (a newtype has exactly
                // one field).
                self.advance();
                let sel_tok = self.cur();
                let sel = self.expect_ident()?;
                self.declare(&sel, DeclaredAs::Field(name.clone()), sel_tok);
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

        let module_path: Vec<String> =
            self.expect_module_name()?.split('.').map(str::to_string).collect();

        if qualified {
            // 'as' is not a keyword — check for Ident("as")
            match self.peek().clone() {
                Token::Ident(ref s) if s == "as" => { self.advance(); }
                _ => {
                    let mut diag = self.err_here(
                        "Expected 'as' in qualified import".to_string());
                    diag.notes.push(
                        "mata-ll requires an explicit alias: write \
                         `import qualified M as M`. GHC lets the bare form \
                         qualify by the module's own (possibly dotted) name; \
                         here every qualified reference goes through the one \
                         declared alias, so the alias is mandatory."
                            .to_string(),
                    );
                    return Err(diag);
                }
            }
            // The alias is a single name. Haskell 2010 lets it be dotted
            // (`as Data.Map`, used as `Data.Map.insert`); mata-ll's alias
            // tables are keyed by one segment, so the dotted form is
            // refused here with the rule spelled out rather than failing
            // later at the first qualified reference.
            if let Token::QualUpperIdent(alias) = self.peek().clone() {
                let mut diag = self.err_here(format!(
                    "A qualified import's alias is a single name, found '{alias}'"
                ));
                diag.notes.push(
                    "Haskell 2010 allows a dotted alias (`as Data.Map`); mata-ll \
                     resolves every qualified reference through a one-segment \
                     alias, so write `as M` and refer to the module as `M.name`."
                        .to_string(),
                );
                return Err(diag);
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
                // A multi-line import list: its continuation lines are
                // indented past the declaration column, so their line
                // starts are whitespace here (F5).
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

    /// The left-hand side of one declaration of a binding group, Haskell
    /// 2010 §4.4.3: `decl → gendecl | (funlhs | pat) rhs`. THE reader of a
    /// declaration's head — the module's top level, `where` and `let`
    /// groups, class bodies and instance bodies all call it, so the forms
    /// a declaration can take are the same wherever one stands (they used
    /// to be five hand-written subsets: an infix definition took a pattern
    /// operand only in a class or instance body and only in parentheses, a
    /// `where` group knew no operator at all, and a pattern binding was a
    /// tuple in a `let` or `where` and nothing anywhere else). The caller
    /// has dealt with a fixity declaration (the keyword says so) and reads
    /// what follows the head: `:: type` after a signature's names, the
    /// `rhs` (`parse_rhs`) after a function head or a pattern.
    fn parse_decl_head(&mut self) -> PResult<DeclHead> {
        if let Some(names) = self.signature_names() {
            return Ok(DeclHead::Signature(names));
        }
        self.parse_lhs()
    }

    /// The names of a type signature, if one starts here: `vars ::` with
    /// `vars → var {, var}` and a `var` an identifier or a parenthesised
    /// operator (Haskell 2010 §4.4.1 `gendecl → vars :: type`). Consumes
    /// the names and stops in front of the `::`; anything else is left
    /// unread — `(a, b) = e` and `f x = e` begin the same way.
    fn signature_names(&mut self) -> Option<Vec<(String, usize)>> {
        let save = self.checkpoint();
        let mut names = Vec::new();
        loop {
            match self.peek().clone() {
                Token::Ident(name) => {
                    names.push((name, self.cur()));
                    self.advance();
                }
                Token::LeftParen => {
                    self.advance();
                    let tok = self.cur();
                    let Token::Operator(op) = self.peek().clone() else { break };
                    self.advance();
                    if !self.at(&Token::RightParen) {
                        break;
                    }
                    self.advance();
                    names.push((op, tok));
                }
                _ => break,
            }
            if self.at(&Token::DblColon) {
                return Some(names);
            }
            if !self.at(&Token::Comma) {
                break;
            }
            self.advance();
        }
        self.rewind(save);
        None
    }

    // Depth-guard wrapper: the grammar rule itself is in `parse_lhs_inner`
    // (a parenthesised head nests).
    fn parse_lhs(&mut self) -> PResult<DeclHead> {
        self.guarded("pattern", Self::parse_lhs_inner)
    }

    /// `funlhs | pat` (Haskell 2010 §4.4.3):
    ///
    /// ```text
    /// funlhs → var apat {apat}            f x y      x      (<+>) a b
    ///        | pat varop pat              V a b |+| V c d      l `has` x
    ///        | ( funlhs ) apat {apat}     (x `op` y) z      (f . g) a
    /// ```
    ///
    /// and anything else that is a pattern is the left-hand side of a
    /// pattern binding (`(lo, hi)`, `Just k`, `x : xs`). The operands of an
    /// infix head are read by THE pattern parser (`parse_pattern`), so
    /// they are whatever a pattern is — a constructor application groups
    /// before the operator, a `:` chain belongs to the operand — and the
    /// one operator the pattern parser does not take is the name being
    /// defined. GHC decides the same way (`isFunLhs`): the head is a
    /// function binding when a variable operator stands at its root, and a
    /// pattern otherwise.
    fn parse_lhs_inner(&mut self) -> PResult<DeclHead> {
        let start = self.checkpoint();
        let start_tok = self.cur();
        match self.peek().clone() {
            // `var apat {apat}`, unless the variable turns out to be the
            // left operand of an infix head or the head of a pattern
            // (`x : xs`, `xs@(x : _)`): those are read below, as patterns.
            Token::Ident(name) if self.nth(1).token != Token::At => {
                self.advance();
                if !matches!(self.peek(), Token::Operator(_) | Token::QualOperator(_) | Token::Backtick) {
                    let span = self.span_of(self.cur());
                    let patterns = self.parse_apats()?;
                    return Ok(DeclHead::Function(FunLhs { name, span, patterns, operand_cons: Vec::new() }));
                }
                self.rewind(start);
            }
            Token::LeftParen => {
                // `(op) apat {apat}`: a `var` is an identifier or an
                // operator in parentheses.
                if let Token::Operator(op) = self.nth(1).token.clone()
                    && self.nth(2).token == Token::RightParen
                {
                    self.advance();
                    Self::definable_operator(&op).map_err(|msg| self.err_here(msg))?;
                    self.advance();
                    self.advance();
                    let span = self.span_of(self.cur());
                    let patterns = self.parse_apats()?;
                    return Ok(DeclHead::Function(FunLhs { name: op, span, patterns, operand_cons: Vec::new() }));
                }
                // `( funlhs ) apat {apat}`: a head in parentheses that
                // takes further arguments. Speculative — the parenthesis
                // may as well open a pattern (`(a, b) = e`, `(Just x) <+>
                // y = e`), which is read below.
                self.advance();
                if let Ok(DeclHead::Function(mut lhs)) = self.parse_lhs()
                    && self.at(&Token::RightParen)
                {
                    self.advance();
                    if self.is_pattern_atom_start() {
                        lhs.patterns.extend(self.parse_apats()?);
                        return Ok(DeclHead::Function(lhs));
                    }
                    // `(f x) = e`, ``(x `op` y) = e``: the grammar gives a
                    // parenthesised head at least one further argument,
                    // and what stands in the parentheses is no pattern
                    // either (GHC: "Parse error in pattern: f x").
                    if !lhs.patterns.is_empty() {
                        let mut diag = Diagnostic::parse_at(
                            format!(
                                "Parse error in pattern: {}",
                                self.source_text(start_tok + 1, self.pos - 1)
                            ),
                            self.span_of(start_tok),
                        );
                        diag.notes.push(format!(
                            "a function head in parentheses takes at least one more argument \
                             after the parenthesis — '({}) z = …' defines '{}' with one \
                             argument more; without one, write the head without the \
                             parentheses",
                            self.source_text(start_tok + 1, self.pos - 1),
                            lhs.name,
                        ));
                        return Err(Box::new(diag));
                    }
                }
                self.rewind(start);
            }
            _ => {}
        }

        // `pat varop pat`, or `pat` alone.
        let left = self.parse_pattern()?;
        let op_tok = self.cur();
        let name = match self.peek().clone() {
            Token::Operator(op) => {
                Self::definable_operator(&op).map_err(|msg| self.err_here(msg))?;
                self.advance();
                op
            }
            // `a O.<+> b = …`: the operator being defined is qualified, and
            // a definition cannot bind an imported module's operator.
            Token::QualOperator(op) => return Err(self.qualified_binder_error(&op)),
            Token::Backtick => {
                self.advance();
                let f = self.expect_ident()?;
                self.expect(&Token::Backtick)?;
                f
            }
            // No operator: a pattern. A variable in parentheses is still
            // the variable (`(x) = e` binds `x`, as `x = e` does).
            _ => {
                let mut bare = &left;
                while let Pattern::Paren(inner) = bare {
                    bare = inner;
                }
                return Ok(match bare {
                    Pattern::Var(name) => DeclHead::Function(FunLhs {
                        name: name.clone(),
                        span: self.span_of(self.cur()),
                        patterns: Vec::new(),
                        operand_cons: Vec::new(),
                    }),
                    _ => DeclHead::Pattern(left),
                });
            }
        };
        let right_tok = self.cur();
        let right = self.parse_pattern()?;
        // A second variable operator: one of the two would have to be part
        // of a pattern, and a pattern's operators are constructors (GHC:
        // "Parse error in pattern: a +++ b").
        if matches!(self.peek(), Token::Operator(_) | Token::QualOperator(_) | Token::Backtick) {
            let mut diag = Diagnostic::parse_at(
                format!("Parse error in pattern: {}", self.source_text(start_tok, self.pos)),
                self.span_of(start_tok),
            );
            diag.notes.push(format!(
                "the head of an infix definition is 'pattern operator pattern': ONE \
                 operator is being defined — here {} — and its operands are patterns, \
                 whose only operators are constructor operators (':'). The further \
                 operator {} leaves this head no reading.",
                crate::fixity::op_display(&name),
                self.peek(),
            ));
            return Err(Box::new(diag));
        }
        let operand_cons = [
            self.root_con_operator(start_tok, op_tok, false),
            self.root_con_operator(right_tok, self.pos, true),
        ]
        .into_iter()
        .flatten()
        .collect();
        Ok(DeclHead::Function(FunLhs {
            name,
            span: self.span_of(start_tok),
            patterns: vec![left, right],
            operand_cons,
        }))
    }

    /// The parameter patterns of a prefix function head: `{apat}`. A
    /// constructor here is nullary (`M.Just` included) — what follows it
    /// is the head's next parameter, not its argument.
    fn parse_apats(&mut self) -> PResult<Vec<Pattern>> {
        let mut patterns = Vec::new();
        while self.is_pattern_atom_start() {
            patterns.push(self.parse_pattern_atom()?);
        }
        Ok(patterns)
    }

    /// Can a definition bind the operator `op`? Haskell 2010 §4.4.3 defines
    /// a `varop`; an operator that starts with a colon is a constructor
    /// (`:` itself, and every `:+`), which a `data` declaration introduces
    /// and a pattern matches — never a function a binding defines. (GHC
    /// reads `(:) a b = e` and `a :+ b = e` as pattern bindings; mata-ll's
    /// patterns have no prefix `(:)` and no infix constructor but `:`,
    /// which the pattern parser has taken before this is asked.) `..` is
    /// reserved syntax (§2.4 `reservedop`).
    fn definable_operator(op: &str) -> Result<(), String> {
        if op.starts_with(':') {
            return Err(format!(
                "'{op}' cannot be defined by an equation: an operator that starts with \
                 ':' is a data constructor — a 'data' declaration introduces it and a \
                 pattern matches it (GHC reads this head as a pattern). A function \
                 operator starts with any other symbol character."
            ));
        }
        if op == ".." {
            return Err("'..' is reserved syntax and cannot be defined as an operator".to_string());
        }
        Ok(())
    }

    /// The constructor operator at the root of the head operand spelled by
    /// the tokens `from..to`, if the operand is an infix-constructor
    /// pattern written WITHOUT parentheses (see `ast::HeadOperand`). The
    /// pattern parser has taken exactly these tokens as one pattern, so an
    /// operator among them outside every bracket is a constructor
    /// operator of that pattern, and the first such is its root: pattern
    /// operators nest to the right.
    fn root_con_operator(&self, from: usize, to: usize, right: bool) -> Option<HeadOperand> {
        let mut depth = 0usize;
        for i in from..to {
            match &self.tokens[i].token {
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace => depth += 1,
                Token::RightParen | Token::RightBracket | Token::RightBrace => {
                    depth = depth.saturating_sub(1)
                }
                Token::Operator(op) if depth == 0 && op.starts_with(':') => {
                    return Some(HeadOperand { con: op.clone(), span: self.span_of(i), right });
                }
                _ => {}
            }
        }
        None
    }

    /// The source text of the tokens `from..to`, spaced as GHC prints a
    /// pattern (`Just k`, `(a, b)`, `[p, q]`, `x : xs`) — for the messages
    /// that quote a pattern as written.
    fn source_text(&self, from: usize, to: usize) -> String {
        let mut out = String::new();
        // No space after an opening bracket or backtick, nor before a
        // closing one or a comma.
        let mut glue = true;
        let mut in_backticks = false;
        for loc in &self.tokens[from..to.min(self.tokens.len())] {
            let text = match &loc.token {
                Token::Indent(_) => continue,
                Token::Ident(s) | Token::UpperIdent(s) | Token::Operator(s)
                | Token::QualIdent(s) | Token::QualUpperIdent(s) | Token::QualOperator(s)
                | Token::BigIntLit(s) => s.clone(),
                Token::IntLit(n) => n.to_string(),
                Token::NumLit(x) => format!("{x:?}"),
                Token::StrLit(bytes) => format!("{:?}", String::from_utf8_lossy(bytes)),
                Token::Underscore => "_".to_string(),
                Token::LeftParen => "(".to_string(),
                Token::RightParen => ")".to_string(),
                Token::LeftBracket => "[".to_string(),
                Token::RightBracket => "]".to_string(),
                Token::Comma => ",".to_string(),
                Token::At => "@".to_string(),
                Token::Backtick => "`".to_string(),
                other => other.to_string().trim_matches('\'').to_string(),
            };
            let backtick = loc.token == Token::Backtick;
            let closes = matches!(
                loc.token,
                Token::RightParen | Token::RightBracket | Token::Comma | Token::At
            ) || (backtick && in_backticks);
            if !glue && !closes {
                out.push(' ');
            }
            out.push_str(&text);
            in_backticks ^= backtick;
            glue = matches!(loc.token, Token::LeftParen | Token::LeftBracket | Token::At)
                || (backtick && in_backticks);
        }
        out
    }

    /// A declaration that began with the pattern spelled by the tokens
    /// `head_tok..` continues with its right-hand side — `=` or a guard —
    /// or it is no declaration: reported at the pattern, which is where
    /// the line went wrong (the token after it is often the next line's).
    fn expect_pattern_binding_rhs(&self, head_tok: usize) -> PResult<()> {
        if matches!(self.peek(), Token::Eq | Token::Pipe) {
            return Ok(());
        }
        let text = self.source_text(head_tok, self.cur());
        let found = match self.peek() {
            Token::Indent(_) => "the line ends".to_string(),
            other => format!("found {other}"),
        };
        let mut diag = Diagnostic::parse_at(
            format!(
                "parse error on input {}: a declaration that begins with the pattern \
                 '{text}' is a pattern binding and continues with '=', but {found}",
                self.tokens[head_tok].token,
            ),
            self.span_of(head_tok),
        );
        diag.notes.push(
            "a declaration is a type signature ('name :: T'), a function binding \
             ('f x = e', 'x <+> y = e') or a pattern binding ('(a, b) = e', 'Just x = \
             e'); this line is none of them."
                .to_string(),
        );
        Err(Box::new(diag))
    }

    /// A pattern binding in a class or instance body. Haskell 2010 §4.3.1
    /// and §4.3.2 give those bodies function and variable bindings only
    /// (GHC: "Pattern bindings (except simple variables) not allowed in
    /// instance declaration").
    fn pattern_binding_in_body_error(&self, head_tok: usize, end_tok: usize, body: &str) -> Box<Diagnostic> {
        let text = self.source_text(head_tok, end_tok);
        let mut diag = Diagnostic::parse_at(
            format!("Pattern bindings (except simple variables) not allowed in {body} declaration: {text}"),
            self.span_of(head_tok),
        );
        diag.notes.push(format!(
            "a class or instance body defines methods: 'name args = …', or \
             an operator method between its two argument patterns ('K a <> K b = \
             …'). '{text}' names no method — it is a pattern on its own."
        ));
        Box::new(diag)
    }

    /// Can the current token begin a member of a class or instance body —
    /// a signature or a function head (a variable, a parenthesis, or the
    /// pattern an infix head's left operand is)? A qualified variable
    /// begins none: the body ends in front of it and the declaration loop
    /// reports the token.
    fn starts_member(&self) -> bool {
        self.starts_binding() && !matches!(self.peek(), Token::QualIdent(_))
    }

    /// Can the current token begin a signature, a function binding or a
    /// pattern binding? All three begin like a pattern (Haskell 2010
    /// §3.17.1 `lpat`): an atomic pattern, a constructor, or the `-` of a
    /// negative literal (`-1 |+| k = k`).
    fn starts_binding(&self) -> bool {
        self.is_case_alt_start()
    }

    fn parse_class_decl(&mut self) -> PResult<Vec<Decl>> {
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

        let class_tok = self.cur();
        let class_name = self.expect_upper_ident()?;
        self.declare(&class_name, DeclaredAs::Class, class_tok);
        let type_var = self.expect_ident()?;
        self.expect(&Token::Where)?;

        let mut methods = Vec::new();
        let Some(method_col) = self.open_block() else {
            return Ok(vec![Decl::ClassDecl { name: class_name, type_var, superclasses, methods }]);
        };

        // The head token of each method's first default equation, and the
        // method whose default equations the previous member line belonged
        // to (None after a signature line): the default equations of one
        // method group like a top-level function's (`push_decl`).
        let mut default_toks: HashMap<String, usize> = HashMap::new();
        let mut open_default: Option<String> = None;
        // Fixity declarations a class body carries for its operator
        // methods (Haskell 2010 §4.3.1 `cdecl`); they govern the module
        // like top-level ones.
        let mut fixities: Vec<Decl> = Vec::new();
        self.in_layout(Layout::Block(method_col), |p| {
            loop {
                p.skip_separators();
                if p.at_block_end() || p.at_block_closer() {
                    break;
                }
                if matches!(p.peek(), Token::Infixl | Token::Infixr | Token::Infix) {
                    fixities.extend(p.parse_fixity_decl()?);
                    continue;
                }

                // A token no member begins with closes the body (the
                // parse-error(t) rule); the declaration loop reports it.
                if !p.starts_member() {
                    break;
                }
                // A member is a declaration like any other (Haskell 2010
                // §4.3.1 `cdecl → gendecl | (funlhs | var) rhs`): a
                // signature line declares methods, a function head defines
                // a method's default, prefix or infix alike.
                let head_tok = p.cur();
                let (name, clause) = match p.parse_decl_head()? {
                    DeclHead::Signature(names) => {
                        p.expect(&Token::DblColon)?;
                        let ty = p.parse_type()?;
                        for (i, (name, tok)) in names.into_iter().enumerate() {
                            let at = if i == 0 { head_tok } else { tok };
                            p.declare(&name, DeclaredAs::Method(class_name.clone()), at);
                            methods.push(ClassMethod { name, ty: ty.clone(), default_clauses: None });
                        }
                        open_default = None;
                        continue;
                    }
                    DeclHead::Function(lhs) => (lhs.name.clone(), p.finish_clause(lhs, head_tok)?),
                    DeclHead::Pattern(_) => {
                        return Err(p.pattern_binding_in_body_error(head_tok, p.cur(), "class"));
                    }
                };

                // Attach to the matching method signature
                let Some(m) = methods.iter_mut().find(|m| m.name == name) else {
                    return Err(Box::new(Diagnostic::parse_at(format!(
                        "Default implementation for '{}' has no preceding type signature in class '{}'",
                        name, class_name
                    ), clause.span)));
                };
                match &mut m.default_clauses {
                    Some(clauses)
                        if open_default.as_deref() == Some(name.as_str())
                            && clauses.first().is_some_and(|c| !c.patterns.is_empty()) =>
                    {
                        clauses.push(clause)
                    }
                    Some(_) => {
                        return Err(p.conflicting_method_err(
                            &name,
                            default_toks[&name],
                            head_tok,
                            &format!("class '{}'", class_name),
                        ));
                    }
                    None => {
                        m.default_clauses = Some(vec![clause]);
                        default_toks.insert(name.clone(), head_tok);
                    }
                }
                open_default = Some(name);
            }
            Ok(())
        })?;

        let mut decls = vec![Decl::ClassDecl { name: class_name, type_var, superclasses, methods }];
        decls.extend(fixities);
        Ok(decls)
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

        self.expect(&Token::Where)?;

        // An empty body (`instance C T where` with every method defaulted)
        // is legal; the following declarations are NOT its methods.
        let mut methods = Vec::new();
        let Some(method_col) = self.open_block() else {
            return Ok(vec![Decl::InstanceDecl { class_name, target_type, context, methods }]);
        };

        // The head token of each method's first equation, parallel to
        // `methods`.
        let mut method_toks: Vec<usize> = Vec::new();
        self.in_layout(Layout::Block(method_col), |p| {
            loop {
                p.skip_separators();
                if p.at_block_end() || p.at_block_closer() {
                    break;
                }

                // A token no method begins with closes the body (the
                // parse-error(t) rule); the declaration loop reports it.
                if !p.starts_member() {
                    break;
                }
                // A method definition is a function binding like any other
                // (Haskell 2010 §4.3.2 `idecl → (funlhs | var) rhs`), read
                // by the one declaration-head parser: `show x = …`, `x <> y
                // = …`, `V a b == V c d = …`, `A == A = True`.
                let head_tok = p.cur();
                let (name, clause) = match p.parse_decl_head()? {
                    DeclHead::Function(lhs) => (lhs.name.clone(), p.finish_clause(lhs, head_tok)?),
                    DeclHead::Signature(_) => {
                        let mut diag = Diagnostic::parse_at(
                            format!(
                                "Illegal type signature in instance declaration: '{} :: …'",
                                p.source_text(head_tok, p.cur())
                            ),
                            p.span_of(head_tok),
                        );
                        diag.notes.push(
                            "a method's type is declared once, in its class; an instance \
                             only defines it. Remove the signature line. (GHC accepts a \
                             signature here under its InstanceSigs extension, which its \
                             default language enables; mata-ll does not have it.)"
                                .to_string(),
                        );
                        return Err(Box::new(diag));
                    }
                    DeclHead::Pattern(_) => {
                        return Err(p.pattern_binding_in_body_error(head_tok, p.cur(), "instance"));
                    }
                };

                // The equations of one method group like a top-level
                // function's (`push_decl`): an equation continues the method
                // the previous line was defining, or starts a method — which
                // must then be one this body has not defined yet.
                match methods.last_mut() {
                    Some(InstanceMethod { name: last, clauses })
                        if *last == name && clauses.first().is_some_and(|c| !c.patterns.is_empty()) =>
                    {
                        clauses.push(clause)
                    }
                    _ => {
                        if let Some(i) = methods.iter().position(|m| m.name == name) {
                            return Err(p.conflicting_method_err(
                                &name,
                                method_toks[i],
                                head_tok,
                                &format!("this instance of '{}'", class_name),
                            ));
                        }
                        methods.push(InstanceMethod { name, clauses: vec![clause] });
                        method_toks.push(head_tok);
                    }
                }
            }
            Ok(())
        })?;

        Ok(vec![Decl::InstanceDecl { class_name, target_type, context, methods }])
    }

    /// The error for a method that one class or instance body (`body`
    /// names it) defines by two separate groups of equations (GHC:
    /// "Conflicting definitions"). `first_tok` and `again_tok` are the head
    /// tokens of the two groups.
    fn conflicting_method_err(
        &self,
        name: &str,
        first_tok: usize,
        again_tok: usize,
        body: &str,
    ) -> Box<Diagnostic> {
        let mut diag = Diagnostic::parse_at(
            format!(
                "Conflicting definitions for '{}' in {}: defined at {} and again",
                name, body, self.span_of(first_tok),
            ),
            self.span_of(again_tok),
        );
        diag.notes.push(format!(
            "a method is defined once per class or instance body. Equations form \
             ONE definition only when they follow each other directly and the \
             first takes arguments; a later equation for '{}' starts a second \
             definition. Write the equations of '{}' together.",
            name, name,
        ));
        Box::new(diag)
    }

    fn parse_export_decl(&mut self) -> PResult<Vec<Decl>> {
        self.expect(&Token::Export)?;
        let name_tok = self.cur();
        let name = self.expect_ident()?;
        self.expect(&Token::DblColon)?;
        let ty = self.parse_type()?;
        // The export declaration is the name's type signature: besides the
        // ExportSig it yields the TypeSig the typechecker checks the
        // definition against. It therefore enters the declaration table's
        // signature namespace, where a second signature-bearing
        // declaration of the name (plain, `intrinsic` or another `export`)
        // is a duplicate. The typechecker keeps one signature per name
        // (`collect_signatures`), so an unreported pair would check the
        // definition against whichever line came last while the export
        // wrapper marshals by the export's own type.
        self.declare(&name, DeclaredAs::ExportSignature, name_tok);
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
            let name_tok = self.cur();
            let name = self.expect_upper_ident()?;
            self.declare(&name, DeclaredAs::Synonym, name_tok);
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
        let name_tok = self.cur();
        let name = self.expect_upper_ident()?;
        self.declare(&name, DeclaredAs::Family, name_tok);

        // Capture the header parameter names: their count fixes the family's
        // arity (and thus kind) even when it declares no equations.
        let mut params = Vec::new();
        while let Token::Ident(p) = self.peek().clone() {
            params.push(p);
            self.advance();
        }

        self.expect(&Token::Where)?;

        // The equations form an implicit layout block; an empty one
        // (`type family F a where` followed by the next declaration) is
        // legal — an open family with no equations.
        let mut equations = Vec::new();
        if let Some(col) = self.open_block() {
            self.in_layout(Layout::Block(col), |p| {
                loop {
                    p.skip_separators();
                    if p.at_block_end() || p.at_block_closer() {
                        break;
                    }
                    // Each equation: FamilyName argType... = resultType. A
                    // token no equation begins with closes the block (the
                    // parse-error(t) rule); the declaration loop reports it.
                    let Token::UpperIdent(eq_name) = p.peek().clone() else { break };
                    if eq_name != name {
                        return Err(p.err_here(format!(
                            "Expected an equation of type family '{}' (each line of its \
                             block begins with the family's name), found '{}'",
                            name, eq_name
                        )));
                    }
                    p.advance(); // consume family name
                    let mut args = Vec::new();
                    while !p.at(&Token::Eq) && !p.at_eof() {
                        args.push(p.parse_type_atom()?);
                    }
                    p.expect(&Token::Eq)?;
                    let result = p.parse_type()?;
                    equations.push(TypeFamilyEq { args, result });
                }
                Ok(())
            })?;
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
            let name_tok = self.cur();
            let name = self.expect_ident()?;
            self.expect(&Token::DblColon)?;
            let ty = self.parse_type()?;
            self.declare(&name, DeclaredAs::Signature, name_tok);
            return Ok(vec![Decl::TypeSig { name, ty }]);
        }
        Err(self.err_here(format!(
            "An 'intrinsic' declaration is either 'intrinsic type family \
             ...' or 'intrinsic name :: type' (a signature whose \
             implementation the compiler provides); found {}",
            self.peek()
        )))
    }

    /// Parse a top-level (or class-body) fixity declaration: `infixl 6 +`,
    /// `infixr 5 :`, `infix 4 \`elem\``, or a comma list (`infixl 7 *, /`)
    /// — one `Decl::FixityDecl` per operator. Such a declaration governs
    /// the whole module, uses above it included; module resolution
    /// attaches it to the operator this module declares under that name.
    /// (A `where` or `let` group's fixity declarations go to the group's
    /// own bindings instead: `parse_local_decls`.)
    fn parse_fixity_decl(&mut self) -> PResult<Vec<Decl>> {
        let (assoc, prec, ops) = self.parse_fixity_ops()?;
        Ok(ops
            .into_iter()
            .map(|(op, op_tok)| {
                self.declare(&op, DeclaredAs::Fixity, op_tok);
                Decl::FixityDecl { assoc, prec, op, span: self.span_of(op_tok) }
            })
            .collect())
    }

    /// The text of a fixity declaration (Haskell 2010 §4.4.2 `fixity
    /// [integer] ops`): the associativity, the precedence and the
    /// operators it names, each with its token. Which binding group the
    /// declaration belongs to is the caller's.
    fn parse_fixity_ops(&mut self) -> PResult<(Assoc, u8, Vec<(String, usize)>)> {
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
        let mut ops = Vec::new();
        loop {
            let mut op_tok = self.cur();
            let op = match self.peek().clone() {
                Token::Operator(s) => { self.advance(); s }
                Token::Ident(s) => { self.advance(); s } // backtick operator, bare
                Token::Backtick => {
                    self.advance();
                    op_tok = self.cur();
                    let s = self.expect_ident()?;
                    self.expect(&Token::Backtick)?;
                    s
                }
                // `infixl 6 O.<+>`: Haskell 2010 §4.4.2 gives a fixity
                // declaration unqualified operators only (`fixity → … ops`,
                // `op → varop | conop`) — it governs operators THIS module
                // declares, and GHC's "parse error on input 'O.<+>'" is
                // that rule. Said with the reason.
                Token::QualOperator(name) | Token::QualIdent(name) => {
                    let mut diag = self.err_here(format!(
                        "A fixity declaration names an unqualified operator, found '{name}'"
                    ));
                    diag.notes.push(
                         "a fixity belongs to the operator's own declaration: the \
                         defining module declares it, and an import brings it along. \
                         Declare the fixity next to the definition in the module that \
                         defines the operator."
                            .to_string(),
                    );
                    return Err(diag);
                }
                _ => return Err(self.err_here("Expected operator after fixity precedence".to_string())),
            };
            ops.push((op, op_tok));
            if self.at(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        Ok((assoc, prec, ops))
    }

    /// A signature, a function binding or a pattern binding at the top
    /// level: `gendecl | (funlhs | pat) rhs` (`parse_decl_head`).
    fn parse_binding_decl(&mut self) -> PResult<Vec<Decl>> {
        let head_tok = self.cur();
        match self.parse_decl_head()? {
            DeclHead::Signature(names) => {
                self.expect(&Token::DblColon)?;
                let ty = self.parse_type()?;
                Ok(names
                    .into_iter()
                    .map(|(name, tok)| {
                        self.declare(&name, DeclaredAs::Signature, tok);
                        Decl::TypeSig { name, ty: ty.clone() }
                    })
                    .collect())
            }
            DeclHead::Function(lhs) => {
                let name = lhs.name.clone();
                let clause = self.finish_clause(lhs, head_tok)?;
                Ok(vec![Decl::FunDef { name, clauses: vec![clause] }])
            }
            DeclHead::Pattern(pattern) => self.top_pattern_binding(pattern, head_tok),
        }
    }

    /// A top-level pattern binding `pat rhs` (Haskell 2010 §4.4.3.2), the
    /// pattern already read from token `head_tok` on. It becomes top-level
    /// value bindings, which are evaluated on first use like every other,
    /// so the binding is as lazy as the report makes it — the match
    /// happens when a variable of the pattern is first demanded, all of it
    /// at once, and never if none is:
    ///
    /// ```text
    /// (lo, Just hi) = e      __pat_1 = case e of (lo, Just hi) -> (lo, hi)
    ///                                           _ -> error "Non-exhaustive patterns in …"
    ///                        lo = case __pat_1 of (__pv, _) -> __pv
    ///                        hi = case __pat_1 of (_, __pv) -> __pv
    /// ```
    ///
    /// The scrutinee binding holds the MATCH — the pattern's variables as
    /// a tuple (the variable itself when there is one, `()` when there is
    /// none) — rather than the right-hand side as a local pattern binding's
    /// does (`pattern_binding_defs`), because a top-level binding is
    /// checked against a type signature (HASKDIFF, "All top-level bindings
    /// require type signatures") and the match has one that follows from
    /// the signatures of the variables: `(T_lo, T_hi)`. `parse_module`
    /// writes it once the whole module is read (`top_pattern_signatures`),
    /// a signature may stand below its binding. Checking the match
    /// against it is what types the pattern and the right-hand side from
    /// the variables' declared types, as GHC does.
    ///
    /// The scrutinee comes FIRST in the returned list, which is what keeps
    /// the selectors out of `push_decl`'s equation grouping: a selector is
    /// a parameterless binding, so nothing can continue it, and it never
    /// follows a function of its own name directly — `f x = 1` and then
    /// `(f, g) = …` are two definitions of `f`, reported as such.
    fn top_pattern_binding(&mut self, pattern: Pattern, head_tok: usize) -> PResult<Vec<Decl>> {
        let text = self.source_text(head_tok, self.cur());
        let span = self.span_of(head_tok);
        self.expect_pattern_binding_rhs(head_tok)?;
        self.check_linear(head_tok, "a pattern binding")?;
        let (rhs, where_binds) = self.parse_rhs(false)?;
        let rhs = self.rhs_expr(rhs, where_binds);
        self.pat_fresh += 1;
        let fresh = format!("__pat_{}", self.pat_fresh);
        let vars = pattern.var_names();

        let matched = match vars.as_slice() {
            [] => Expr::Lit(Literal::Unit),
            [v] => Expr::Var(v.clone()),
            vs => Expr::Tuple(vs.iter().cloned().map(Expr::Var).collect()),
        };
        let mut branches = Vec::new();
        let refutable = !Self::pg_irrefutable(&pattern);
        branches.push(CaseBranch { pattern, guards: vec![], body: Some(matched) });
        if refutable {
            branches.push(CaseBranch {
                pattern: Pattern::Wildcard,
                guards: vec![],
                body: Some(Self::pg_error(&format!("Non-exhaustive patterns in {text}"))),
            });
        }
        let value = |name: String, body: Expr| Decl::FunDef {
            name,
            clauses: vec![Clause {
                patterns: Vec::new(),
                guards: Vec::new(),
                body: Some(body),
                where_binds: Vec::new(),
                span,
                raw_guards: None,
                operand_cons: Vec::new(),
            }],
        };
        let mut decls =
            vec![value(fresh.clone(), Expr::Case { scrutinee: Box::new(rhs), branches })];
        for (i, v) in vars.iter().enumerate() {
            // The selected component is bound under a name of the
            // compiler's reserved namespace, like every binder the parser
            // writes itself.
            let picked = "__pv".to_string();
            let select = if vars.len() == 1 {
                Expr::Var(fresh.clone())
            } else {
                let slots = (0..vars.len())
                    .map(|j| if i == j { Pattern::Var(picked.clone()) } else { Pattern::Wildcard })
                    .collect();
                Expr::Case {
                    scrutinee: Box::new(Expr::Var(fresh.clone())),
                    branches: vec![CaseBranch {
                        pattern: Pattern::Tuple(slots),
                        guards: vec![],
                        body: Some(Expr::Var(picked)),
                    }],
                }
            };
            decls.push(value(v.clone(), select));
        }
        self.top_patterns.push(TopPattern { scrutinee: fresh, vars, text, span });
        Ok(decls)
    }

    /// The type signatures of the scrutinee bindings of the module's
    /// top-level pattern bindings (see `top_pattern_binding`), built from
    /// the signatures `decls` holds for the variables each one binds: the
    /// tuple of the variables' types. A variable without a signature is
    /// the error it is for every top-level binding; a variable whose
    /// signature has a class context is GHC's "Overloaded signature
    /// conflicts with monomorphism restriction" — a pattern binding is
    /// one value, matched once, and cannot be one value per instance.
    fn top_pattern_signatures(&self, decls: &[Decl]) -> Result<Vec<Decl>, Vec<Diagnostic>> {
        let mut sigs: HashMap<&str, &Type> = HashMap::new();
        for d in decls {
            if let Decl::TypeSig { name, ty } | Decl::ExportSig { name, ty } = d {
                sigs.entry(name.as_str()).or_insert(ty);
            }
        }
        let mut out = Vec::new();
        let mut errors = Vec::new();
        for tp in &self.top_patterns {
            let mut quantified: Vec<String> = Vec::new();
            let mut types = Vec::new();
            for v in &tp.vars {
                let Some(mut ty) = sigs.get(v.as_str()).copied() else {
                    let mut diag = Diagnostic::parse_at(
                        format!("Missing type signature for '{v}'"),
                        tp.span,
                    );
                    diag.notes.push(format!(
                        "the pattern binding '{} = …' defines '{v}' at the top level, \
                         and mata-ll requires a type signature for every top-level \
                         binding (GHC infers one): add '{v} :: T' — the variables of one \
                         pattern can share a line, 'a, b :: T'.",
                        tp.text,
                    ));
                    errors.push(diag);
                    continue;
                };
                // An explicit `forall` of a variable's signature
                // quantifies the scrutinee's signature too.
                loop {
                    match ty {
                        Type::Forall { var, inner } => {
                            if !quantified.contains(var) {
                                quantified.push(var.clone());
                            }
                            ty = inner;
                        }
                        Type::Paren(inner) => ty = inner,
                        _ => break,
                    }
                }
                if matches!(ty, Type::Constrained { .. }) {
                    let mut diag = Diagnostic::parse_at(
                        format!(
                            "Overloaded signature conflicts with monomorphism restriction: \
                             the signature of '{v}' has a class context, and '{v}' is bound \
                             by the pattern binding '{} = …'",
                            tp.text,
                        ),
                        tp.span,
                    );
                    diag.notes.push(format!(
                        "a pattern binding is one value, matched once and shared by the \
                         variables it binds; a class-constrained '{v}' would be a \
                         different value at every instance. Give '{v}' a signature \
                         without a context, or define it by an equation of its own (GHC \
                         rejects this signature the same way)."
                    ));
                    errors.push(diag);
                    continue;
                }
                types.push(ty.clone());
            }
            if types.len() != tp.vars.len() {
                continue;
            }
            let mut ty = match types.len() {
                0 => Type::Unit,
                1 => types.pop().expect("one type"),
                _ => Type::Tuple(types),
            };
            for var in quantified.into_iter().rev() {
                ty = Type::Forall { var, inner: Box::new(ty) };
            }
            out.push(Decl::TypeSig { name: tp.scrutinee.clone(), ty });
        }
        if errors.is_empty() { Ok(out) } else { Err(errors) }
    }

    /// The rest of a function equation after its head: the `rhs`
    /// (`parse_rhs`), with the head's parameter list — which began at
    /// token `binders_from` — checked for a variable bound twice. Shared
    /// by top-level functions, class defaults and instance methods; the
    /// equation is an item of the enclosing block (the module's
    /// declarations, a class or instance body), whose layout context is
    /// already in force.
    fn finish_clause(&mut self, lhs: FunLhs, binders_from: usize) -> PResult<Clause> {
        self.check_linear(binders_from, &format!("an equation for '{}'", lhs.name))?;
        let (rhs, where_binds) = self.parse_rhs(false)?;
        // A guarded clause has no single body: the guard chain IS the
        // body. A chain with binding qualifiers is held raw: its
        // fall-through target is the NEXT clause, which is not parsed yet.
        // The clause group is lowered at end of parse
        // (desugar_pattern_guard_clauses).
        let (guards, body, raw_guards) = match rhs {
            Rhs::Body(body) => (Vec::new(), Some(body), None),
            Rhs::Guards(GuardChain::Plain(g)) => (g, None, None),
            Rhs::Guards(GuardChain::Raw(r)) => (Vec::new(), None, Some(r)),
        };
        Ok(Clause {
            patterns: lhs.patterns,
            guards,
            body,
            where_binds,
            span: lhs.span,
            raw_guards,
            operand_cons: lhs.operand_cons,
        })
    }

    /// The right-hand side of a declaration and its `where` block (Haskell
    /// 2010 §4.4.3 `rhs → = exp [where decls] | gdrhs [where decls]`),
    /// from just after the head: a guard chain (`| cond = body`, each part
    /// free to start a new line indented past the block's column) or `=
    /// body`. `spanned` marks the body as a statement-boundary expression
    /// (`parse_stmt_expr`), which the bindings of a local group are.
    fn parse_rhs(&mut self, spanned: bool) -> PResult<(Rhs, Vec<LocalDef>)> {
        let chain = self.parse_guard_chain(&Token::Eq)?;
        let rhs = if chain.is_empty() {
            self.expect(&Token::Eq)?;
            Rhs::Body(if spanned { self.parse_stmt_expr()? } else { self.parse_expr()? })
        } else {
            Rhs::Guards(chain)
        };
        let where_binds = self.parse_where()?;
        Ok((rhs, where_binds))
    }

    /// A right-hand side as ONE expression, for a binding that stands
    /// alone (a pattern binding, an equation nothing follows): the guard
    /// chain folded over the match-failure error, under the `where` block
    /// that scopes over guards and bodies alike.
    fn rhs_expr(&mut self, rhs: Rhs, where_binds: Vec<LocalDef>) -> Expr {
        let fallback = Expr::App(
            Box::new(WiredIn::Error.var()),
            Box::new(Expr::Lit(Literal::Str(b"non-exhaustive guards".to_vec()))),
        );
        let body = match rhs {
            Rhs::Body(body) => body,
            Rhs::Guards(GuardChain::Plain(guards)) => Self::pg_plain_guards(guards, &fallback),
            Rhs::Guards(GuardChain::Raw(raws)) => self.guard_chain_expr(raws, fallback),
        };
        Self::wrap_where(where_binds, body)
    }

    /// One `| cond <sep> body` guard chain, shared by function clauses
    /// (`= body`), where bindings (`= body`, then desugared to if/else),
    /// and case branches (`-> body`). The chain is parsed identically in
    /// all three positions; each guard body gets the `Spanned` statement
    /// marker so a type error inside it is reported at the body's own
    /// line, not the clause head.
    /// A `|` that starts a new line belongs to this chain only when the
    /// line continues the enclosing item — is indented past the block's
    /// column; the layout context decides, so the guard of a function
    /// clause written after a guarded case alternative
    /// (`| c == 92 = case cs of … (e : r) | e == 110 -> … \n | otherwise = …`,
    /// the last `|` left of the alternatives) ends the alternative's chain
    /// and is read by the clause.
    fn parse_guard_chain(&mut self, sep: &Token) -> PResult<GuardChain> {
        let mut raws: Vec<RawGuard> = Vec::new();
        let mut any_binding = false;
        while self.at(&Token::Pipe) {
            self.advance();
            // Haskell 2010 §3.13: a guard is a comma-separated qualifier
            // list; the guard succeeds when every qualifier holds, checked
            // left to right.
            let mut quals = vec![self.parse_guard_qualifier()?];
            while self.at(&Token::Comma) {
                self.advance();
                quals.push(self.parse_guard_qualifier()?);
            }
            if quals.iter().any(|q| !matches!(q, GuardQual::Bool(_))) {
                any_binding = true;
            }
            self.expect(sep)?;
            let body = self.parse_stmt_expr()?;
            raws.push(RawGuard { quals, body });
        }
        if any_binding {
            return Ok(GuardChain::Raw(raws));
        }
        // The all-boolean chain folds its qualifiers to short-circuit `&&`
        // (the same order and laziness as sequential qualifier checking) —
        // the exact form every downstream stage always consumed.
        let guards = raws.into_iter().map(|r| {
            let mut it = r.quals.into_iter();
            let Some(GuardQual::Bool(mut condition)) = it.next() else {
                unreachable!("non-boolean qualifier in a plain guard chain")
            };
            for q in it {
                let GuardQual::Bool(next) = q else {
                    unreachable!("non-boolean qualifier in a plain guard chain")
                };
                condition = Expr::InfixApp {
                    op: WiredIn::And.reference(),
                    lhs: Box::new(condition),
                    rhs: Box::new(next),
                };
            }
            Guard { condition, body: r.body }
        }).collect();
        Ok(GuardChain::Plain(guards))
    }

    /// One guard qualifier of Haskell 2010 §3.13: a `let` binding group, a
    /// pattern guard `pat <- expr` (tried first, backtracking to an
    /// expression exactly like a list-comprehension generator), or a
    /// boolean expression.
    fn parse_guard_qualifier(&mut self) -> PResult<GuardQual> {
        if self.at(&Token::Let) {
            self.advance();
            let binds = self.parse_let_binds()?;
            return Ok(GuardQual::Let(binds));
        }
        let save = self.checkpoint();
        if self.is_pattern_start() {
            if let Ok(pat) = self.parse_pattern()
                && self.at(&Token::Bind)
            {
                self.check_linear(save.pos, "a pattern guard")?;
                self.advance();
                let expr = self.parse_expr()?;
                return Ok(GuardQual::Pat(pat, expr));
            }
            // Not a pattern guard — backtrack and parse as a boolean.
            self.rewind(save);
        }
        Ok(GuardQual::Bool(self.parse_expr()?))
    }

    /// Does an equation of `name` continue the function binding whose first
    /// equation is `first`? Haskell 2010 §4.4.3.1 as GHC implements it: the
    /// equations of one function are adjacent, and only a first equation
    /// that takes arguments opens a group (`k = 1` is complete on its own,
    /// so a second `k = 2` is a second binding, not a second equation).
    /// Shared by the top-level declaration list (`push_decl`) and the
    /// local binding groups (`close_local_group`).
    fn continues_function(first_name: &str, first_patterns: &[Pattern], name: &str) -> bool {
        first_name == name && !first_patterns.is_empty()
    }

    /// Close a local binding group: the entries in source order become the
    /// group's `LocalDef`s. This is the ONE representation every local
    /// binding group (`where`, let-expression, do-`let`, guard and
    /// comprehension `let`) takes downstream, the same for all of them:
    /// a value binding is a patternless `LocalDef`; a function binding is
    /// its equations as ADJACENT `LocalDef`s of one name, each carrying
    /// that equation's parameter patterns and body (the typechecker types
    /// them at one shared type, codegen compiles the run as one clause
    /// matrix). Three things happen here, in this order:
    ///
    /// 1. equations are grouped per §4.4.3.1 (`continues_function`);
    /// 2. the group is checked: a name bound by two groups, or by a group
    ///    and a pattern binding, is GHC's "Conflicting definitions" (an
    ///    equation separated from its function by another binding is such
    ///    a second definition, exactly as at the top level); the equations
    ///    of one function take the same number of arguments;
    /// 3. each function's equations are lowered (`lower_local_equations`):
    ///    guards fall through to the next equation, and only past the last
    ///    equation to the match-failure error;
    /// 4. the group's signature and fixity declarations are attached to
    ///    the bindings they name (`attach_local_sigs`,
    ///    `attach_local_fixities`) — both belong to the group that binds
    ///    the name, wherever in the group they stand.
    fn close_local_group(
        &mut self,
        entries: Vec<LocalEntry>,
        sigs: Vec<(String, Type, Span)>,
        fixities: Vec<(String, (Assoc, u8), Span)>,
    ) -> PResult<Vec<LocalDef>> {
        enum Group {
            Function(Vec<LocalEquation>),
            Lowered { defs: Vec<LocalDef>, bound: Vec<(String, Span)> },
        }
        let mut groups: Vec<Group> = Vec::new();
        for entry in entries {
            match entry {
                LocalEntry::Equation(eq) => {
                    if let Some(Group::Function(eqs)) = groups.last_mut()
                        && Self::continues_function(&eqs[0].name, &eqs[0].patterns, &eq.name)
                    {
                        eqs.push(eq);
                    } else {
                        groups.push(Group::Function(vec![eq]));
                    }
                }
                LocalEntry::Lowered { defs, bound } => groups.push(Group::Lowered { defs, bound }),
            }
        }

        let mut first_bound: HashMap<&str, Span> = HashMap::new();
        for group in &groups {
            let bound: Vec<(&str, Span)> = match group {
                Group::Function(eqs) => vec![(eqs[0].name.as_str(), eqs[0].span)],
                Group::Lowered { bound, .. } => bound.iter().map(|(n, s)| (n.as_str(), *s)).collect(),
            };
            for (name, span) in bound {
                if let Some(prev) = first_bound.get(name) {
                    let mut diag = Diagnostic::parse_at(
                        format!(
                            "Conflicting definitions for '{}': bound at {} and again at {}",
                            name, prev, span
                        ),
                        span,
                    );
                    diag.notes.push(format!(
                        "a binding group defines each name once. Equations form ONE function \
                         only when they follow each other directly and the first takes \
                         arguments; anything between them — another binding, or the type \
                         signature of '{}' itself — ends the function, and the next equation \
                         starts a second definition. Write the equations of '{}' together, or \
                         rename one definition.",
                        name, name,
                    ));
                    return Err(Box::new(diag));
                }
                first_bound.insert(name, span);
            }
            if let Group::Function(eqs) = group {
                let n = eqs[0].patterns.len();
                if let Some(odd) = eqs.iter().find(|e| e.patterns.len() != n) {
                    let plural = |k: usize| if k == 1 { "argument" } else { "arguments" };
                    let mut diag = Diagnostic::parse_at(
                        format!(
                            "Equations for '{}' have different numbers of arguments: \
                             the first equation (at {}) binds {} {}, this one binds {} {}",
                            eqs[0].name, eqs[0].span, n, plural(n),
                            odd.patterns.len(), plural(odd.patterns.len()),
                        ),
                        odd.span,
                    );
                    diag.notes.push(
                        "every equation of one function matches the same number of \
                         arguments; a different count is a different function."
                            .to_string(),
                    );
                    return Err(Box::new(diag));
                }
            }
        }

        let mut binds = Vec::new();
        for group in groups {
            match group {
                Group::Function(eqs) => binds.extend(self.lower_local_equations(eqs)),
                Group::Lowered { defs, .. } => binds.extend(defs),
            }
        }
        Self::attach_local_sigs(&mut binds, sigs)?;
        Self::attach_local_fixities(&mut binds, fixities)?;
        Ok(binds)
    }

    /// Lower the equations of one local function (or the one equation of a
    /// value binding) to `LocalDef`s. An equation whose guards can fall
    /// through to a LATER equation cannot stand alone: from the first such
    /// equation on, the equations collapse into one final equation over
    /// fresh parameters whose body re-matches each of them with join-point
    /// fall-through (`collapse_match_rows`, the lowering top-level clause
    /// groups with pattern guards take). Every earlier equation, and a
    /// guarded equation that is the LAST one, keeps its own patterns and
    /// body — the shape codegen's clause matrix compiles directly — with
    /// the guard chain folded over the match-failure error.
    fn lower_local_equations(&mut self, eqs: Vec<LocalEquation>) -> Vec<LocalDef> {
        let name = eqs[0].name.clone();
        let n = eqs.len();
        let collapse_from = eqs
            .iter()
            .position(|e| matches!(e.rhs, Rhs::Guards(_)))
            .filter(|&k| k + 1 < n);
        let mut eqs = eqs;
        let tail = collapse_from.map(|k| eqs.split_off(k));
        let mut defs: Vec<LocalDef> = eqs
            .into_iter()
            .map(|eq| {
                let body = self.rhs_expr(eq.rhs, eq.where_binds);
                LocalDef {
                    operand_cons: eq.operand_cons,
                    ..LocalDef::new(eq.name, eq.patterns, body)
                }
            })
            .collect();
        if let Some(mut tail) = tail {
            let arity = tail[0].patterns.len();
            // The collapsed equation stands for every equation of the
            // tail, so it carries the head operands of all of them.
            let operand_cons =
                tail.iter_mut().flat_map(|eq| std::mem::take(&mut eq.operand_cons)).collect();
            let rows = tail
                .into_iter()
                .map(|eq| MatchRow {
                    patterns: eq.patterns,
                    rhs: match eq.rhs {
                        Rhs::Body(b) => RowRhs::Body(b),
                        Rhs::Guards(GuardChain::Plain(g)) => RowRhs::Plain(g),
                        Rhs::Guards(GuardChain::Raw(r)) => RowRhs::Raw(r),
                    },
                    where_binds: eq.where_binds,
                })
                .collect();
            let (params, body) = self.collapse_match_rows(&name, arity, rows);
            defs.push(LocalDef {
                operand_cons,
                ..LocalDef::new(name, params.into_iter().map(Pattern::Var).collect(), body)
            });
        }
        defs
    }

    /// An equation's own `where` block scopes over its guards and body.
    fn wrap_where(where_binds: Vec<LocalDef>, body: Expr) -> Expr {
        if where_binds.is_empty() {
            body
        } else {
            Expr::Let { binds: where_binds, body: Box::new(body) }
        }
    }

    /// The bindings a pattern binding `pat = rhs` lowers to, in the SAME
    /// recursive group as its siblings: one fresh binding for the scrutinee
    /// plus one lazy SELECTOR binding per pattern variable
    /// (`a = case __tup of (a, b) -> a`), so — as in Haskell — the
    /// pattern's variables are in scope for the right-hand side itself and
    /// for sibling bindings, and the match happens on first demand (never
    /// eagerly, the way wrapping the body in a case would). That is the
    /// laziness Haskell 2010 §4.4.3.2 gives every pattern binding — the
    /// pattern is matched irrefutably, `let Just k = Nothing in 5` is `5`
    /// — whatever the pattern and wherever the binding stands (a `let` or
    /// `where` group, or the top level: `top_pattern_binding`).
    ///
    /// A pattern that can fail to match (a constructor, a literal) gives
    /// each selector a second alternative raising GHC's error for the
    /// failed binding, "Non-exhaustive patterns in Just k" — `text` is the
    /// pattern as written. A pattern of variables, wildcards and tuples
    /// cannot fail and gets none.
    fn pattern_binding_defs(pat: Pattern, rhs: Expr, fresh: String, text: &str) -> Vec<LocalDef> {
        let refutable = !Self::pg_irrefutable(&pat);
        let mut defs = vec![LocalDef::new(fresh.clone(), vec![], rhs)];
        for v in pat.var_names() {
            let mut branches = vec![CaseBranch {
                pattern: pat.clone(),
                guards: vec![],
                body: Some(Expr::Var(v.clone())),
            }];
            if refutable {
                branches.push(CaseBranch {
                    pattern: Pattern::Wildcard,
                    guards: vec![],
                    body: Some(Self::pg_error(&format!("Non-exhaustive patterns in {text}"))),
                });
            }
            defs.push(LocalDef::new(
                v,
                vec![],
                Expr::Case { scrutinee: Box::new(Expr::Var(fresh.clone())), branches },
            ));
        }
        defs
    }

    /// Attach a binding group's signature lines to their bindings: every
    /// LocalDef of the named binding (a multi-equation local function is
    /// several) gets the type. A signature with no equation, or a second
    /// signature for one name, is an error — the same two GHC reports.
    fn attach_local_sigs(binds: &mut [LocalDef], sigs: Vec<(String, Type, Span)>) -> PResult<()> {
        for (name, ty, span) in sigs {
            let mut found = false;
            for ld in binds.iter_mut().filter(|ld| ld.name == name) {
                if ld.sig.is_some() {
                    return Err(Box::new(Diagnostic::parse_at(
                        format!("Duplicate type signatures for '{}'", name),
                        span,
                    )));
                }
                ld.sig = Some(ty.clone());
                found = true;
            }
            if !found {
                let mut diag = Diagnostic::parse_at(
                    format!("The type signature for '{}' lacks an accompanying binding", name),
                    span,
                );
                diag.notes.push(
                    "a signature line inside a 'where' or 'let' block declares the type of a \
                     binding defined in that same block; add the equation(s) for it, or \
                     remove the signature"
                        .to_string(),
                );
                return Err(Box::new(diag));
            }
        }
        Ok(())
    }

    /// Attach a binding group's fixity declarations to their bindings
    /// (Haskell 2010 §4.4.2: a fixity declaration stands in the
    /// declaration group that binds its operator and governs that
    /// binding's uses, like a type signature). Every LocalDef of the named
    /// binding gets the fixity; module resolution groups the infix chains
    /// in the binding's scope by it. A declaration for a name the group
    /// does not bind, or a second one for a name, is GHC's error.
    fn attach_local_fixities(
        binds: &mut [LocalDef],
        fixities: Vec<(String, (Assoc, u8), Span)>,
    ) -> PResult<()> {
        for (i, (name, fixity, span)) in fixities.iter().enumerate() {
            let shown = crate::fixity::op_display(name);
            if let Some((_, _, first)) = fixities[..i].iter().find(|(n, _, _)| n == name) {
                let mut diag = Diagnostic::parse_at(
                    format!("Multiple fixity declarations for {shown}: at {first} and again"),
                    *span,
                );
                diag.notes.push(
                    "an operator has one precedence and associativity in the group that \
                     binds it, so it takes one fixity declaration; remove one of the two."
                        .to_string(),
                );
                return Err(Box::new(diag));
            }
            let mut found = false;
            for ld in binds.iter_mut().filter(|ld| ld.name == *name) {
                ld.fixity = Some(*fixity);
                found = true;
            }
            if !found {
                let mut diag = Diagnostic::parse_at(
                    format!("The fixity signature for {shown} lacks an accompanying binding"),
                    *span,
                );
                diag.notes.push(format!(
                    "a fixity declaration inside a 'where' or 'let' block sets the \
                     precedence of an operator defined in that same block, for the uses \
                     in the block's scope. An operator bound elsewhere — at the top \
                     level, in an enclosing block, or imported — keeps the fixity \
                     declared where it is bound. Define {shown} in this block, or move \
                     the declaration next to its definition (GHC rejects this \
                     declaration the same way)."
                ));
                return Err(Box::new(diag));
            }
        }
        Ok(())
    }

    /// The optional `where` block of an equation — a top-level or
    /// class/instance clause, or a LOCAL equation inside a `let` or `where`
    /// group. The keyword belongs to the equation when it stands on the
    /// equation's line or on a line that continues it (indented past the
    /// enclosing block's column); a `where` at or left of that column is
    /// not visible here — the layout context shows the line start instead
    /// — and closes the group for the enclosing clause, as in
    /// `main = do { let x = 1; … } where helper = …`.
    fn parse_where(&mut self) -> PResult<Vec<LocalDef>> {
        if !self.at(&Token::Where) {
            return Ok(vec![]);
        }
        self.advance();

        // Bindings must be indented past the enclosing block's column, or
        // the `where` is empty and the next line is the next definition.
        let Some(where_col) = self.open_block() else {
            return Ok(Vec::new());
        };

        self.parse_local_decls(where_col, "__wtup_")
    }

    /// The declarations of a local binding group — a `where` block or the
    /// bindings of a `let` (Haskell 2010 §4.4.3 `decls`), whose implicit
    /// block starts at column `col` — up to the token that ends the block.
    /// Each is read by the one declaration-head parser (`parse_decl_head`),
    /// so a local group takes every form the top level takes: signatures
    /// (`h :: T`, `(<+>) :: T`, `a, b :: T`), fixity declarations,
    /// function bindings with prefix, infix or parenthesised heads, and
    /// pattern bindings. Signatures and fixities are collected and
    /// attached when the group closes (`close_local_group`), so they may
    /// precede or follow the bindings they name; both are scoped to this
    /// group, whose operator shadows an outer one of the same name with
    /// its own type and fixity. `fresh_prefix` names the scrutinee
    /// bindings of the group's pattern bindings.
    fn parse_local_decls(&mut self, col: usize, fresh_prefix: &str) -> PResult<Vec<LocalDef>> {
        let mut fresh_counter = 0usize;
        let mut sigs: Vec<(String, Type, Span)> = Vec::new();
        let mut fixities: Vec<(String, (Assoc, u8), Span)> = Vec::new();
        let mut entries: Vec<LocalEntry> = Vec::new();
        self.in_layout(Layout::Block(col), |p| {
            loop {
                p.skip_separators();
                // `in` at the bindings' own column closes a let group by
                // the parse-error(t) rule (it is in `at_block_closer`), as
                // do a comprehension's `,` and `]`.
                if p.at_block_end() || p.at_block_closer() {
                    break;
                }
                // Explicit `;` between bindings (`let a = 1; b = 2 in …`) —
                // the Haskell 2010 separator, accepted alongside layout.
                if p.at(&Token::Semicolon) {
                    p.advance();
                    continue;
                }
                if matches!(p.peek(), Token::Infixl | Token::Infixr | Token::Infix) {
                    let (assoc, prec, ops) = p.parse_fixity_ops()?;
                    for (op, tok) in ops {
                        fixities.push((op, (assoc, prec), p.span_of(tok)));
                    }
                    continue;
                }
                // A qualified name begins no binding, and nothing outside
                // the block could take it either: say why here.
                if let Token::QualIdent(name) = p.peek().clone() {
                    return Err(p.qualified_binder_error(&name));
                }
                // A token no declaration begins with closes the block (the
                // parse-error(t) rule) — a guard's `=` or `->` right after
                // a `let` qualifier's last binding (`| let d = v - n = …`),
                // an operator at the bindings' column. The enclosing
                // construct reads it next and reports it if nothing there
                // takes it either.
                if !p.starts_binding() {
                    break;
                }
                let head_tok = p.cur();
                let span = p.span_of(head_tok);
                match p.parse_decl_head()? {
                    // A signature line: recorded now, attached to its
                    // equations when the block closes.
                    DeclHead::Signature(names) => {
                        p.expect(&Token::DblColon)?;
                        let ty = p.parse_type()?;
                        for (name, _) in names {
                            sigs.push((name, ty.clone(), span));
                        }
                    }
                    DeclHead::Function(lhs) => {
                        if p.at(&Token::DblColon) {
                            return Err(Self::signature_with_arguments_error(&lhs, span));
                        }
                        p.check_linear(head_tok, &format!("an equation for '{}'", lhs.name))?;
                        // The guard chain is kept raw for
                        // `close_local_group`, which knows the next
                        // equation to fall through to.
                        let (rhs, where_binds) = p.parse_rhs(true)?;
                        entries.push(LocalEntry::Equation(LocalEquation {
                            name: lhs.name,
                            patterns: lhs.patterns,
                            rhs,
                            where_binds,
                            span,
                            operand_cons: lhs.operand_cons,
                        }));
                    }
                    DeclHead::Pattern(pattern) => {
                        let text = p.source_text(head_tok, p.cur());
                        p.expect_pattern_binding_rhs(head_tok)?;
                        p.check_linear(head_tok, "a pattern binding")?;
                        let (rhs, where_binds) = p.parse_rhs(true)?;
                        let body = p.rhs_expr(rhs, where_binds);
                        let fresh = format!("{fresh_prefix}{fresh_counter}");
                        fresh_counter += 1;
                        let bound = pattern.var_names().into_iter().map(|v| (v, span)).collect();
                        let defs = Self::pattern_binding_defs(pattern, body, fresh, &text);
                        entries.push(LocalEntry::Lowered { defs, bound });
                    }
                }
            }
            Ok(())
        })?;
        self.close_local_group(entries, sigs, fixities)
    }

    /// `h y :: Int -> Int` in a binding group: a signature written with
    /// the equation's argument patterns.
    fn signature_with_arguments_error(lhs: &FunLhs, span: Span) -> Box<Diagnostic> {
        let mut diag = Diagnostic::parse_at(
            format!(
                "a type signature names the binding alone: '{} :: type' on its own line, \
                 with the equations ('{} {} = …') separate",
                lhs.name, lhs.name,
                lhs.patterns.iter().map(|_| "…").collect::<Vec<_>>().join(" "),
            ),
            span,
        );
        diag.notes.push(
            "as in Haskell, a signature declares a NAME's type; the argument patterns \
             belong to the equations that define it"
                .to_string(),
        );
        Box::new(diag)
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
                // `forall a b. T` is `forall a. forall b. T`: the binders
                // nest, one `Forall` node each, innermost last.
                let mut vars = vec![self.expect_ident()?];
                while let Token::Ident(_) = self.peek() {
                    vars.push(self.expect_ident()?);
                }
                self.expect(&Token::Operator(".".to_string()))?;
                let mut ty = self.parse_type()?;
                for var in vars.into_iter().rev() {
                    ty = Type::Forall { var, inner: Box::new(ty) };
                }
                return Ok(ty);
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
        let lhs = self.parse_type_op()?;
        // A multiplicity annotation before the arrow: `a %1 -> b` (the
        // argument is consumed exactly once) or the explicit unrestricted
        // spellings `a %Many -> b` / `a %'Many -> b`. `%` only ever means
        // this in a type — types have no operators.
        if self.at(&Token::Operator("%".to_string())) {
            let mult = self.parse_multiplicity()?;
            if !self.at(&Token::Arrow) {
                return Err(self.err_here(
                    "a multiplicity annotation belongs to a function arrow: \
                     expected '->' after the '%' annotation".to_string()));
            }
            self.advance();
            let rhs = self.parse_type_arrow()?;
            return Ok(Type::Arrow(Box::new(lhs), Box::new(rhs), mult));
        }
        if self.at(&Token::Arrow) {
            self.advance();
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
    /// and `..` out. The chain is returned flat (`Type::OpChain`): its
    /// grouping depends on the fixity of the TYPE each operator names in the
    /// module's scope (a module's `infixr 6 :*:` / `infixr 5 :+:` group
    /// `a :+: b :*: c` as `a :+: (b :*: c)`), which module resolution
    /// supplies.
    fn is_type_operator(op: &str) -> bool {
        op.starts_with(':') && op.len() > 1
    }

    fn parse_type_op(&mut self) -> PResult<Type> {
        let first = self.parse_type_app()?;
        let mut rest: Vec<(ChainOp, Type)> = Vec::new();
        loop {
            let op = match self.peek() {
                Token::Operator(o) if Self::is_type_operator(o) => o.clone(),
                _ => break,
            };
            let span = self.span_of(self.cur());
            self.advance();
            let operand = self.parse_type_app()?;
            rest.push((ChainOp { name: op, span }, operand));
        }
        if rest.is_empty() {
            return Ok(first);
        }
        Ok(Type::OpChain { first: Box::new(first), rest })
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
                | Token::QualUpperIdent(_)
                | Token::Ident(_)
                | Token::LeftParen
                | Token::LeftBracket
                | Token::StrLit(_)
                | Token::Tick
        )
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
            // Module-qualified type reference: `M.Map`, `Data.Map.Map`.
            Token::QualUpperIdent(name) => {
                self.advance();
                Ok(Type::Con(name))
            }
            Token::UpperIdent(name) => {
                self.advance();
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
                        && self.nth(1).token == Token::RightParen
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
            _ => Err(self.err_expected("type")),
        }
    }

    // --- Expression parsing ---

    /// Parse a statement-boundary expression — a do-statement, a let/where
    /// binding body, a case-branch or guard body — and wrap it in an
    /// `Expr::Spanned` marker carrying the line it starts on. The marker is
    /// transparent to every later pass but lets the type checker report an
    /// error against the offending statement's line rather than the clause
    /// head.
    fn parse_stmt_expr(&mut self) -> PResult<Expr> {
        let loc = self.peek_loc();
        let span = Span::new(loc.line, loc.col);
        let e = self.parse_expr()?;
        Ok(Expr::Spanned(span, Box::new(e)))
    }

    fn parse_expr(&mut self) -> PResult<Expr> {
        let expr = self.parse_infix_chain()?;

        // Type ascription: expr :: Type
        if self.at(&Token::DblColon) {
            self.advance();
            let ty = self.parse_type()?;
            return Ok(Expr::Ascription(Box::new(expr), ty));
        }

        Ok(expr)
    }

    /// Parse an infix expression — `[-] e (op [-] e)*`, each `e` an
    /// application — WITHOUT grouping it: the result is the flat
    /// `Expr::OpChain` (or the lone operand when there is no operator and
    /// no prefix minus). How the chain nests depends on the fixity of the
    /// entity each operator denotes, which only module resolution knows
    /// (see `fixity`). Depth-guarded like every nesting production.
    fn parse_infix_chain(&mut self) -> PResult<Expr> {
        self.guarded("expression", Self::parse_infix_chain_inner)
    }

    /// The grammar rule of `parse_infix_chain`, without the depth guard:
    /// the parenthesised-expression path enters here directly, under the
    /// guard of the atom that contains it.
    fn parse_infix_chain_inner(&mut self) -> PResult<Expr> {
        let first = self.parse_chain_operand()?;
        let mut rest: Vec<(ChainOp, ChainOperand)> = Vec::new();
        loop {
            // An operator that starts a new line continues the chain when
            // the line is indented past the enclosing block's column — the
            // layout context reads such a line start as whitespace, so the
            // operator is simply the next token. At the block's column the
            // line start is the block's `;` and the chain ends in front of
            // it (`x = 1` then `+ 2` at x's own column is GHC's parse
            // error on `+`, reported by the block).
            match self.peek().clone() {
                Token::Operator(ref op) if op == ".." => {
                    break; // '..' is range syntax, not an infix operator
                }
                // A bare operator or a module-qualified one (`1 O.<+> 2`):
                // the same chain entry, named as written; which entity and
                // fixity `O.<+>` denotes is import resolution's.
                Token::Operator(op) | Token::QualOperator(op) => {
                    // An operator directly followed by ')' is a left-section
                    // tail (`(a * b +)`): it belongs to the enclosing
                    // parenthesised-expression path, which builds the
                    // section. Never take it as an infix operator — there
                    // is no expression after it to parse.
                    if self.nth(1).token == Token::RightParen {
                        break;
                    }
                    let span = self.span_of(self.cur());
                    self.advance();
                    let operand = self.parse_chain_operand()?;
                    rest.push((ChainOp { name: op, span }, operand));
                }
                Token::Backtick => {
                    let save = self.checkpoint();
                    let span = self.span_of(self.cur());
                    self.advance();
                    let func = self.expect_backtick_name()?;
                    self.expect(&Token::Backtick)?;
                    // A backtick operator directly followed by ')' is a
                    // left-section tail (``(a * b `div`)``) — same stop as
                    // the symbolic-operator arm above.
                    if self.at(&Token::RightParen) {
                        self.rewind(save);
                        break;
                    }
                    let operand = self.parse_chain_operand()?;
                    rest.push((ChainOp { name: func, span }, operand));
                }
                _ => break,
            }
        }

        if rest.is_empty() && first.neg.is_none() {
            return Ok(first.expr);
        }
        Ok(Expr::OpChain(Box::new(OpChain { first, rest })))
    }

    /// One operand of an infix chain: an application, optionally under a
    /// prefix minus. Whether the minus is legal where it stands, and how
    /// much of the chain it negates, is decided with the grouping
    /// (`fixity::group`): both depend on the neighbouring operators'
    /// fixities.
    fn parse_chain_operand(&mut self) -> PResult<ChainOperand> {
        let neg = match self.peek() {
            Token::Operator(op) if op == "-" => {
                let span = self.span_of(self.cur());
                self.advance(); // consume '-'
                Some(span)
            }
            _ => None,
        };
        let expr = self.parse_expr_app()?;
        Ok(ChainOperand { neg, expr })
    }

    /// The operand of a right section, parsed at the `infixexp` level.
    /// Haskell 2010 puts `::` one grammar level HIGHER (exp → infixexp
    /// [:: type]), so a section operand can never carry an ascription —
    /// GHC parse-errors on `(+ 1 :: Int)` — and this parser agrees, with
    /// the reason spelled out. (`parse_expr` would have consumed the
    /// `::`.) Left sections cannot reach the shape: their operand parse
    /// stops in front of `::`, so the section test never sees a trailing
    /// operator. `op_shown` is the operator as the user wrote it
    /// (backticks included), for the error's concrete rewrite hint.
    fn parse_right_section_operand(&mut self, op_shown: &str) -> PResult<Expr> {
        // `parse_expr` minus its ascription tail.
        let expr = self.parse_infix_chain()?;
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


    /// Function application: an atom followed by argument atoms. An
    /// argument may stand on a following line indented past the enclosing
    /// block's column (the layout context reads that line start as
    /// whitespace); at the block's column the line start is the block's
    /// `;`, which no atom begins with, so a sibling item is never taken
    /// as an argument.
    fn parse_expr_app(&mut self) -> PResult<Expr> {
        let mut func = self.parse_expr_atom_dotted()?;
        while self.is_expr_atom_start() {
            let arg = self.parse_expr_atom_dotted()?;
            func = Expr::App(Box::new(func), Box::new(arg));
        }
        Ok(func)
    }

    /// Parse the binding group after `let` — the `let` keyword already
    /// consumed — up to but NOT consuming the following `in` (a let-expression)
    /// or `,`/`]` (a list-comprehension `let` qualifier). Bindings are
    /// layout-separated exactly as in a let-expression and are the
    /// declarations a where block takes (`parse_local_decls`), all in one
    /// mutually recursive group. Shared by the let-expression atom, the do-`let` statement,
    /// the guard qualifier and the comprehension qualifier so they bind
    /// identically.
    fn parse_let_binds(&mut self) -> PResult<Vec<LocalDef>> {
        // The first binding's column is the group's layout block: a later
        // line at that column is the next binding, a line indented less
        // closes the group (so a do-block's next statement, at the `let`
        // line's own indent, is never read as a binding). An empty group
        // (`let` followed by a line at or left of the enclosing block's
        // column) is reported by the caller.
        let Some(let_col) = self.open_block() else {
            return Ok(Vec::new());
        };
        self.parse_local_decls(let_col, "__tup_")
    }

    /// The body of a let-expression, the `in` already consumed: `let binds
    /// in body`. The body is a full expression (ascription included), so
    /// it runs to the end of the statement or operand the let stands in.
    /// Shared by the let-expression atom and the do-block, whose
    /// `let … in e` statement is the same expression.
    fn let_expr_body(&mut self, binds: Vec<LocalDef>) -> PResult<Expr> {
        let body = self.parse_expr()?;
        Ok(Expr::Let {
            binds,
            body: Box::new(body),
        })
    }

    /// Parse list comprehension qualifiers: `x <- xs, pred, y <- ys, …`.
    /// Supports pattern-matching generators (`Ok x <- rs`, `(a, b) <- pairs`)
    /// and `let` qualifiers.
    fn parse_list_comprehension_quals(&mut self) -> PResult<Vec<ListCompQual>> {
        let mut quals = Vec::new();
        loop {
            // `let decls` qualifier: bindings visible in the body and every
            // later qualifier. Desugars to `let decls in <rest>`.
            if self.at(&Token::Let) {
                self.advance();
                let binds = self.parse_let_binds()?;
                quals.push(ListCompQual::Let(binds));
                if self.at(&Token::Comma) { self.advance(); continue; }
                break;
            }
            // Try generator: pattern <- expr
            let save = self.checkpoint();
            if self.is_pattern_start() {
                if let Ok(pat) = self.parse_pattern()
                    && self.at(&Token::Bind) {
                        self.check_linear(save.pos, "a list-comprehension generator")?;
                        self.advance();
                        let expr = self.parse_expr()?;
                        quals.push(ListCompQual::Generator { pattern: pat, expr });
                        if self.at(&Token::Comma) { self.advance(); continue; }
                        break;
                    }
                // Not a generator — backtrack and parse as guard
                self.rewind(save);
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
                                Box::new(WiredIn::ConcatMap.var()),
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
                                Box::new(WiredIn::ConcatMap.var()),
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
    /// `expr.field` desugars to `(field expr)`, the label written as a
    /// field selection (`ast::field_selection`), which import resolution
    /// binds to the field. Only applies when the `.` is
    /// adjacent to the preceding token (no space) and followed by an
    /// identifier, to distinguish it from function composition `f . g`.
    /// A dot glued to a CONSTRUCTOR name never arrives here: the lexer has
    /// already joined `M.field` into the qualified name `M.field`
    /// (Haskell 2010 §2.4), so this rule only ever sees a record value's
    /// selection (`r.field`, `(f x).field`, `M.r.field`).
    fn parse_expr_atom_dotted(&mut self) -> PResult<Expr> {
        let mut expr = self.parse_expr_atom()?;

        while self.at(&Token::Operator(".".to_string())) {
            // Check adjacency: the '.' must be on the same line as the
            // previous token and immediately follow it (no whitespace).
            // Adjacency is a raw-stream property: the token before the
            // dot's index and the one after it, line starts included.
            let dot = self.cur();
            let prev_tok = &self.tokens[dot - 1];
            let dot_tok = &self.tokens[dot];
            if dot_tok.line != prev_tok.line {
                break;
            }
            // The lexer carries the exact source end column.
            let prev_end = prev_tok.end_col;
            if dot_tok.col != prev_end {
                break; // there's a gap — this is composition, not field access
            }
            if dot + 1 < self.tokens.len()
                && let Token::Ident(_) = &self.tokens[dot + 1].token {
                    // The identifier must hug the dot on the RIGHT too
                    // (OverloadedRecordDot's rule): `negate. abs` is the
                    // composition `negate . abs`, not the field access
                    // `abs negate` it used to silently parse as.
                    let ident_tok = &self.tokens[dot + 1];
                    if ident_tok.line != dot_tok.line || ident_tok.col != dot_tok.col + 1 {
                        break;
                    }
                    self.advance(); // consume '.'
                    if let Token::Ident(field) = self.peek().clone() {
                        self.advance(); // consume field name
                        expr = Expr::App(
                            Box::new(Expr::Var(crate::ast::field_selection(&field))),
                            Box::new(expr),
                        );
                        continue;
                    }
                }
            break;
        }

        // Record update: expr { field = val, ... }
        // Loop to allow chained updates: expr { x = 1 } { y = 2 }.
        // The brace attaches on the same line, or from a following line
        // indented past the enclosing block's column (a continuation line,
        // like an application argument's); a brace at or left of the block
        // column stands behind the block's `;` and is never seen here.
        while self.at(&Token::LeftBrace) {
            let save_pos = self.checkpoint();
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

    /// Can the next token begin an expression (Haskell 2010 `exp`): an
    /// atom, a layout keyword's construct, a lambda, or a prefix minus?
    fn is_stmt_expr_start(&self) -> bool {
        self.is_expr_atom_start()
            || matches!(
                self.peek(),
                Token::If | Token::Case | Token::Let | Token::Do | Token::Backslash
            )
            || matches!(self.peek(), Token::Operator(op) if op == "-")
    }

    /// Can the next token begin an argument atom (Haskell 2010 `aexp`)?
    /// A `-` never can: Haskell has no negative literal, `-` is the
    /// prefix negation operator of the infix grammar (`exp → - exp`,
    /// §3.4), so `f -1` is `f - 1` and `x -3` is `x - 3`, and the only
    /// way to pass a negative number is `f (-1)`. The negation is read
    /// by `parse_chain_operand`. An earlier "negative literal" arm here
    /// accepted `-N` as an atom when the token before it was not an
    /// expression's last token; on one line that is unreachable (an
    /// argument position always follows an expression's last token),
    /// and on a continuation line the preceding token is the line's
    /// `Indent`, so `let b = a` / `  - 3` became `a (-3)` instead of the
    /// subtraction GHC reads (F7).
    fn is_expr_atom_start(&self) -> bool {
        matches!(
            self.peek(),
            Token::Ident(_)
                | Token::UpperIdent(_)
                | Token::QualIdent(_)
                | Token::QualUpperIdent(_)
                | Token::IntLit(_)
                | Token::BigIntLit(_)
                | Token::NumLit(_)
                | Token::StrLit(_)
                | Token::LeftParen
                | Token::LeftBracket
        )
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
    /// At a `.` inside parentheses: the hugging field chain `.a.b` up to
    /// the closing parenthesis, consumed together with that parenthesis;
    /// `None` (nothing consumed) when the shape is anything else.
    fn field_selector_chain(&mut self) -> Option<Vec<String>> {
        let cp = self.checkpoint();
        let mut chain = Vec::new();
        loop {
            // Hugging is a raw-stream property: the identifier is the very
            // next token after the dot.
            let dot_idx = self.cur();
            let dot = self.tokens[dot_idx].clone();
            if !matches!(dot.token, Token::Operator(ref o) if o == ".") {
                break;
            }
            let Some(next) = self.tokens.get(dot_idx + 1) else { break };
            let Token::Ident(name) = &next.token else { break };
            if next.line != dot.line || next.col != dot.end_col
                || !name.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c == '_') {
                break;
            }
            chain.push(name.clone());
            self.pos = dot_idx + 2;
            if self.at(&Token::RightParen) {
                self.advance();
                return Some(chain);
            }
        }
        self.rewind(cp);
        None
    }

    fn parse_paren_expr(&mut self) -> PResult<Expr> {
        self.advance();

        // (,) (,,) ... — tuple constructor as a prefix function. N commas
        // denote an (N+1)-ary constructor. Desugar to a single multi-param
        // lambda building a tuple: this compiles to a genuine N+1-arg Lua
        // function (matching how binary functions are passed to `zipWith`,
        // `foldr`, etc.), while partial application (e.g. `(,) x`) is
        // handled by the ordinary call-site eta-wrap. Type: `a -> b -> (a, b)`.
        // The parameters are compiler-made names like every other binder a
        // desugaring introduces, so they live in the reserved `__` namespace.
        if self.at(&Token::Comma) {
            let mut commas = 0;
            while self.at(&Token::Comma) { self.advance(); commas += 1; }
            self.expect(&Token::RightParen)?;
            let params: Vec<String> =
                (0..commas + 1).map(|i| format!("__tcon{}", i)).collect();
            let elems: Vec<Expr> =
                params.iter().map(|p| Expr::Var(p.clone())).collect();
            return Ok(Expr::Lambda { params, body: Box::new(Expr::Tuple(elems)) });
        }

        // `(.field)` / `(.a.b)` — a record field selector section
        // (OverloadedRecordDot): the dot hugs a lowercase identifier and
        // the parentheses close right after the chain. Desugars to
        // `\r -> r.a.b`, i.e. `\r -> b (a r)` (field access is
        // accessor application). With a space (`(. f)`) it is the
        // composition section below, exactly GHC's whitespace rule.
        if let Token::Operator(op) = self.peek()
            && op == "."
            && let Some(chain) = self.field_selector_chain()
        {
            let mut body = Expr::Var("__sec".into());
            for field in chain {
                body = Expr::App(
                    Box::new(Expr::Var(crate::ast::field_selection(&field))),
                    Box::new(body),
                );
            }
            return Ok(Expr::Lambda { params: vec!["__sec".into()], body: Box::new(body) });
        }

        // Check for operator-starting forms: (+), (+1), (-).
        // A leading '-' that is NOT the bare operator `(-)` is
        // prefix minus, never a right section (the GHC rule): it
        // falls through to the general parenthesised-expression
        // path below, which parses it by the negation grammar —
        // so `(-a + b)` is `(negate a) + b` and `(-a * b)` is
        // `negate (a * b)`, not a blanket negation of the whole
        // body.
        if let Token::Operator(op) | Token::QualOperator(op) = self.peek().clone() {
            let bare_op = self.nth(1).token == Token::RightParen;
            // Only the bare `-` is prefix minus; a qualified `M.-` is an
            // ordinary operator, as in GHC.
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
                // (op expr) — right section: \x -> x op expr. The
                // operand is checked against the operator's fixity, and
                // the section rewritten to that lambda, when the chain
                // is grouped (`Expr::Section`).
                let rhs = self.parse_right_section_operand(&op)?;
                self.expect(&Token::RightParen)?;
                return Ok(Expr::Section {
                    op: ChainOp { name: op, span: op_span },
                    side: SectionSide::Right,
                    operand: Box::new(rhs),
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
            let name = self.expect_backtick_name()?;
            self.expect(&Token::Backtick)?;
            if self.at(&Token::RightParen) {
                // (`name`) — operator as function
                self.advance();
                return Ok(Expr::OpFunc(name));
            }
            let rhs = self.parse_right_section_operand(&format!("`{}`", name))?;
            self.expect(&Token::RightParen)?;
            return Ok(Expr::Section {
                op: ChainOp { name, span: op_span },
                side: SectionSide::Right,
                operand: Box::new(rhs),
            });
        }

        // () — unit
        if self.at(&Token::RightParen) {
            self.advance();
            return Ok(Expr::Lit(Literal::Unit));
        }

        // Parse the parenthesised body ONCE: the infix chain stops in
        // front of an operator directly followed by ')', so a
        // left-section tail — with a simple operand (`(a +)`) or a full
        // infix one (`(a * b +)`) — is still unconsumed afterwards, and
        // nothing is parsed twice (re-parsing the body to test for a
        // section made nested parens cost O(2^n)).
        //
        // A leading '-' is prefix minus here (`(-a + b)` fell through
        // from the operator-section check above). Parentheses do not
        // suspend the layout rule (Haskell 2010 §10.3 knows only layout
        // keywords and explicit braces): `::`, a tuple comma or the `)`
        // may stand on a continuation line, but a line at the enclosing
        // block's column is the block's next item, as GHC reads it.
        let mut expr = self.parse_infix_chain_inner()?;

        // (expr op) — left section: \x -> expr op x. The operand must
        // satisfy the section-operand precedence rule — `(a * b +)` is
        // legal, `(a + b *)` is not — which is checked when the chain is
        // grouped (`Expr::Section`).
        if let Token::Operator(op) | Token::QualOperator(op) = self.peek().clone()
            && self.nth(1).token == Token::RightParen
        {
            let op_span = {
                let l = self.peek_loc();
                Span::new(l.line, l.col)
            };
            self.advance(); // consume operator
            self.advance(); // consume )
            return Ok(Expr::Section {
                op: ChainOp { name: op, span: op_span },
                side: SectionSide::Left,
                operand: Box::new(expr),
            });
        }

        // (expr `name`) — backtick left section: \x -> expr `name` x
        if self.at(&Token::Backtick)
            && let Token::Ident(_) | Token::QualIdent(_) = &self.nth(1).token
            && self.nth(2).token == Token::Backtick
            && self.nth(3).token == Token::RightParen
        {
            let op_span = {
                let l = self.peek_loc();
                Span::new(l.line, l.col)
            };
            self.advance(); // consume first backtick
            let name = self.expect_backtick_name()?;
            self.advance(); // consume second backtick
            self.advance(); // consume )
            return Ok(Expr::Section {
                op: ChainOp { name, span: op_span },
                side: SectionSide::Left,
                operand: Box::new(expr),
            });
        }

        // Not a section — this mirrors `parse_expr`: `::` ascription.
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

    /// A bracketed atom: list literal, range (`[a ..]`, `[a, b .. c]`),
    /// or list comprehension — disambiguated after the first element.
    /// Consumes the opening `[`.
    fn parse_list_expr(&mut self) -> PResult<Expr> {
        self.advance();
        if self.at(&Token::RightBracket) {
            self.advance();
            return Ok(Expr::Con("[]".to_string()));
        }
        let first = self.parse_expr()?;
        // A comprehension bar, range `..`, comma or closing `]` may sit
        // on a continuation line (indented past the enclosing block's
        // column); the layout context reads that line start as whitespace.
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
                    Box::new(WiredIn::EnumFrom.var()),
                    Box::new(first),
                ));
            }
            // [x..y] → enumFromTo x y
            let end = self.parse_expr()?;
                self.expect(&Token::RightBracket)?;
            return Ok(Expr::App(
                Box::new(Expr::App(
                    Box::new(WiredIn::EnumFromTo.var()),
                    Box::new(first),
                )),
                Box::new(end),
            ));
        }
        // Regular list literal or range with step
        let mut items = vec![first];
        if self.at(&Token::Comma) {
            self.advance();
                let second = self.parse_expr()?;
            // Check for [x,y..] or [x,y..z]
            if self.at(&Token::Operator("..".to_string())) {
                self.advance();
                        if self.at(&Token::RightBracket) {
                    // [x,y..] → enumFromThen x y
                    self.advance();
                    return Ok(Expr::App(
                        Box::new(Expr::App(
                            Box::new(WiredIn::EnumFromThen.var()),
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
                            Box::new(WiredIn::EnumFromThenTo.var()),
                            Box::new(items.pop().unwrap()),
                        )),
                        Box::new(second),
                    )),
                    Box::new(end),
                ));
            }
            items.push(second);
            loop {
                        if !self.at(&Token::Comma) { break; }
                self.advance();
                        items.push(self.parse_expr()?);
            }
        }
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

        // Explicit brace syntax: case x of { A -> e1; B -> e2 }. Inside
        // the braces no implicit block is open (`advance` suspended the
        // context at the `{`), so line starts mean nothing.
        if self.at(&Token::LeftBrace) {
            self.advance();
            let mut branches = Vec::new();
            loop {
                if self.at(&Token::RightBrace) { break; }
                let pattern = self.parse_case_pattern()?;
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
        // (branch index, raw chain) for alternatives whose guards use
        // binding qualifiers — lowered after the loop.
        let mut raw_branches: Vec<(usize, Vec<RawGuard>)> = Vec::new();
        let Some(case_col) = self.open_block() else {
            // `case x of` followed by a line at or left of the enclosing
            // block's column: the alternatives are `{}`.
            return Err(self.err_expected("case alternative"));
        };

        self.in_layout(Layout::Block(case_col), |p| {
            loop {
                p.skip_separators();
                if p.at_block_end() || p.at_block_closer() {
                    break;
                }
                // A token no alternative begins with closes the block (the
                // parse-error(t) rule) and is read by the enclosing
                // construct: a `|` at the alternatives' column continues
                // the enclosing clause's guard chain (`| c = case … of …`
                // then `| otherwise = …` aligned with the alternatives),
                // an operator there makes the whole `case` its left operand.
                if !p.is_case_alt_start() {
                    break;
                }
                let pattern = p.parse_case_pattern()?;

                // A guard chain may begin on the line after the pattern
                // (`Just a\n    | a < 0 -> …`), indented past the
                // alternatives' column, exactly as a function clause's
                // guards may; a `|` at or left of that column is not seen
                // here (its line start is), and the `->` check reports it.
                if p.at(&Token::Pipe) {
                    // Guards on case branch
                    match p.parse_guard_chain(&Token::Arrow)? {
                        GuardChain::Plain(guards) => branches.push(CaseBranch {
                            pattern,
                            guards,
                            body: None,
                        }),
                        GuardChain::Raw(raws) => {
                            // A binding-qualifier chain (§3.13); remembered per
                            // branch and lowered below, once the alternatives
                            // it can fall through to are parsed.
                            raw_branches.push((branches.len(), raws));
                            branches.push(CaseBranch {
                                pattern,
                                guards: vec![],
                                body: None,
                            });
                        }
                    }
                } else {
                    p.expect(&Token::Arrow)?;
                    let body = p.parse_stmt_expr()?;
                    branches.push(CaseBranch {
                        pattern,
                        guards: vec![],
                        body: Some(body),
                    });
                }
            }
            Ok(())
        })?;

        let branches = self.desugar_pattern_guard_branches(branches, raw_branches);
        Ok(Expr::Case {
            scrutinee: Box::new(scrutinee),
            branches,
        })
    }

    /// The pattern of one case alternative — a binder list of its own.
    fn parse_case_pattern(&mut self) -> PResult<Pattern> {
        let binders_from = self.cur();
        let pattern = self.parse_pattern()?;
        self.check_linear(binders_from, "a case alternative")?;
        Ok(pattern)
    }

    /// A `do` block: layout-driven statement list with `let`, `pat <-`,
    /// `_ <-`, named binds and bare expressions. Consumes the `do`.
    fn parse_do_block(&mut self) -> PResult<Expr> {
        let do_loc = self.peek_loc().clone();
        self.advance();
        let mut stmts = Vec::new();
        let empty_block_error = |do_loc: &Located| {
            Box::new(Diagnostic::parse_at(
                "Empty 'do' block: a 'do' block needs at least one \
                 statement, and its last statement must be an expression \
                 (it is the block's result)"
                    .to_string(),
                Span::new(do_loc.line, do_loc.col),
            ))
        };
        // The explicit-brace form `do { a; b }` is not supported (HASKDIFF:
        // only `case … of { … }` has one); fail at the brace, loudly.
        if self.at(&Token::LeftBrace) {
            let mut diag = self.err_here("parse error on input '{'".to_string());
            diag.notes.push(
                "mata-ll lays out a do block's statements by indentation only; the \
                 explicit brace-and-semicolon form `do { a; b }` of Haskell 2010 is \
                 not supported (an explicit `case … of { … }` is). Put each \
                 statement on its own line, aligned under the first."
                    .to_string(),
            );
            return Err(diag);
        }
        let Some(do_col) = self.open_block() else {
            return Err(empty_block_error(&do_loc));
        };

        self.in_layout(Layout::Block(do_col), |p| {
            loop {
                p.skip_separators();
                if p.at_block_end() || p.at_block_closer() {
                    break;
                }

                // `let` statement: one binding group — variable, function and
                // pattern bindings, all in one mutually recursive scope —
                // parsed by the same layout rules as a let-expression's group.
                // Haskell 2010 §3.14 has two statements that start with `let`:
                // `let decls` alone is the let STATEMENT, and `let decls in e`
                // is an expression statement — the let-expression — so the
                // keyword alone does not decide which; the `in` after the
                // bindings does (F8). Where that `in` stands matters: on the
                // bindings' line or indented past the block's statement column
                // it continues the statement; at the statement column the
                // layout rule has put a `;` before it, and a statement cannot
                // begin with `in`.
                if p.at(&Token::Let) {
                    let let_kw = p.peek_loc().clone();
                    p.advance();
                    let let_loc = p.peek_loc().clone();
                    let binds = p.parse_let_binds()?;
                    if binds.is_empty() {
                        return Err(Box::new(Diagnostic::parse_at(
                            "Expected a binding after `let`: `name = expr`, \
                             `f x = expr`, or `(a, b) = expr`"
                                .to_string(),
                            Span::new(let_loc.line, let_loc.col),
                        )));
                    }
                    // An `in` on the bindings' line or on a continuation
                    // line of the statement is the let-expression's.
                    if p.at(&Token::In) {
                        p.advance();
                        let expr = p.let_expr_body(binds)?;
                        stmts.push(DoStmt::Expr(Expr::Spanned(
                            Span::new(let_kw.line, let_kw.col),
                            Box::new(expr),
                        )));
                        continue;
                    }
                    // An `in` at the statement column stands behind the
                    // block's `;`. With no let-expression waiting for it,
                    // that is GHC's parse error on the `in`; otherwise it
                    // closes this block for that let-expression (the
                    // parse-error(t) rule) — the let statement is then the
                    // block's last, which the check below reports as GHC
                    // does. (An `in` further left closes the block by
                    // indentation.)
                    if matches!(p.after_separator(), Some(Token::In)) && p.let_in_pending == 0 {
                        let in_loc = p.nth(1).clone();
                        let mut diag = Diagnostic::parse_at(
                            "parse error on input 'in': a 'let' statement in a \
                             'do' block has no 'in' — its bindings are in scope \
                             for the statements after it, and this 'in' starts \
                             a new statement of the block"
                                .to_string(),
                            Span::new(in_loc.line, in_loc.col),
                        );
                        diag.notes.push(
                            "to bind names for the rest of the block, drop the \
                             'in'; to write the let-expression 'let … in e' as \
                             one statement, put 'in' on the bindings' line or \
                             indent it past the block's statements"
                                .to_string(),
                        );
                        return Err(Box::new(diag));
                    }
                    stmts.push(DoStmt::DoLet { binds });
                    continue;
                }

                // `pat <- expr` — a monadic bind over ANY pattern (A15):
                // `x`, `_`, `(a, b)`, `()`, `Just x`, `(x : rest)`, literals.
                // Probed exactly like a list-comprehension generator: parse a
                // full pattern and commit only on `<-` (an expression statement
                // often starts like a pattern, so anything else rewinds). Plain
                // variables and `_` stay on the simple Bind path — the
                // refutable-pattern desugar (a case with GHC's MonadFail
                // fallback, see desugar.rs) is only for the shapes that need it.
                if p.is_pattern_start() {
                    let bind_loc = p.peek_loc().clone();
                    let save_pat = p.checkpoint();
                    if let Ok(pat) = p.parse_pattern()
                        && p.at(&Token::Bind) {
                            // The pattern's last token: its end is the span's
                            // end (inclusive last column, as GHC prints it).
                            let pat_end = {
                                let last = &p.tokens[p.pos - 1];
                                Span::new(last.line, last.end_col.saturating_sub(1))
                            };
                            p.check_linear(save_pat.pos, "a 'do' statement")?;
                            p.advance();
                            let expr = p.parse_stmt_expr()?;
                            match pat {
                                Pattern::Var(name) => {
                                    stmts.push(DoStmt::Bind { name, expr });
                                }
                                Pattern::Wildcard => {
                                    stmts.push(DoStmt::Bind { name: "_".to_string(), expr });
                                }
                                pattern => stmts.push(DoStmt::PatternBind {
                                    pattern,
                                    expr,
                                    span: Span::new(bind_loc.line, bind_loc.col),
                                    end: pat_end,
                                }),
                            }
                            continue;
                        }
                    p.rewind(save_pat);
                }

                // Bare expression. A token no statement begins with closes
                // the block (the parse-error(t) rule) and is read by the
                // enclosing construct: an operator at the statement column
                // makes the whole `do` its left operand, as in GHC (where
                // `print x` then `+ 2` under it is the type error `Num (IO
                // ())`, not a parse error).
                if !p.is_stmt_expr_start() {
                    break;
                }
                let expr = p.parse_stmt_expr()?;
                stmts.push(DoStmt::Expr(expr));
            }
            Ok(())
        })?;

        // GHC's rule (Haskell 2010 §3.14): a `do` block has at least one
        // statement, and its LAST statement is an expression — it is the
        // block's result. The other endings used to slip through to the
        // desugarer, which had to invent a meaning: an empty block became
        // the literal False, and a trailing `let x = action` desugared to
        // the binding's right-hand side — silently RUNNING the action the
        // let only meant to name.
        match stmts.last() {
            None => return Err(empty_block_error(&do_loc)),
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
        let binders_from = self.cur();
        let mut pats = Vec::new();
        while matches!(
            self.peek(),
            Token::Ident(_)
                | Token::Underscore
                | Token::LeftParen
                | Token::LeftBracket
                | Token::UpperIdent(_)
                | Token::QualUpperIdent(_)
                | Token::QualIdent(_)
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
        self.check_linear(binders_from, "a lambda")?;
        self.expect(&Token::Arrow)?;
        let body = self.parse_expr()?;
        Ok(Self::lambda_from_patterns(pats, body, "non-exhaustive lambda pattern"))
    }

    /// Build a lambda over general PATTERN parameters: plain variables and
    /// wildcards become parameters directly; every other pattern gets a
    /// fresh parameter matched by a `case` wrapped around the body, with a
    /// wildcard fallback raising `fallback_msg` for a partial pattern —
    /// GHC's semantics for a refutable pattern in a lambda. (A local
    /// function binding with pattern parameters is no longer lowered this
    /// way: its equations stay a clause group, see `close_local_group`.)
    fn lambda_from_patterns(pats: Vec<Pattern>, mut body: Expr, fallback_msg: &str) -> Expr {
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
                            Box::new(WiredIn::Error.var()),
                            Box::new(Expr::Lit(Literal::Str(fallback_msg.as_bytes().to_vec()))),
                        )),
                    },
                ],
            };
        }
        Expr::Lambda { params, body: Box::new(body) }
    }

    // --- Pattern-guard desugaring (Haskell 2010 §3.13) -------------------
    //
    // A guard chain with BINDING qualifiers (`pat <- expr`, `let`) lowers
    // to the plain expression language right here in the parser: qualifiers
    // become nested `case`/`if`/`let` forms, and every "this guard failed"
    // edge jumps to a lazily `let`-bound JOIN POINT holding the rest of the
    // alternatives — evaluated at most once, on demand, so nothing is
    // duplicated and nothing is computed twice (the scrutinee of a pattern
    // guard in particular). Function clause groups need the whole group
    // (§3.13 falls through to the NEXT CLAUSE), so their lowering runs at
    // end of parse over the merged clause list: the clauses from the first
    // raw chain on merge into one clause over fresh parameters whose body
    // re-matches each original clause's patterns with the same join-point
    // fall-through. Downstream stages never see any of this — an all-boolean
    // chain still folds to `&&` conditions, and no Clause leaves the parser
    // with `raw_guards` set.

    fn pg_fresh_name(&mut self, kind: &str) -> String {
        self.pg_fresh += 1;
        format!("__pg{}_{}", kind, self.pg_fresh)
    }

    /// A pattern the lowering needs no fall-through branch for: it can only
    /// fail by diverging (forcing bottom), never by mismatch.
    fn pg_irrefutable(p: &Pattern) -> bool {
        match p {
            Pattern::Var(_) | Pattern::Wildcard => true,
            Pattern::Paren(inner) | Pattern::As(_, inner) => Self::pg_irrefutable(inner),
            Pattern::Tuple(ps) => ps.iter().all(Self::pg_irrefutable),
            Pattern::Constructor { .. } | Pattern::LitPat(_) => false,
        }
    }

    fn pg_error(msg: &str) -> Expr {
        Expr::App(
            Box::new(WiredIn::Error.var()),
            Box::new(Expr::Lit(Literal::Str(msg.as_bytes().to_vec()))),
        )
    }

    /// Bind `fail` as a join point unless it is already a variable; returns
    /// the binding to wrap around the user of the join (via `pg_wrap_join`)
    /// and the expression each failure edge references.
    fn pg_join(&mut self, fail: Expr) -> (Option<LocalDef>, Expr) {
        if matches!(fail, Expr::Var(_)) {
            return (None, fail);
        }
        self.pg_fresh += 1;
        let name = format!("{}{}", JOIN_POINT_PREFIX, self.pg_fresh);
        (
            Some(LocalDef::new(name.clone(), vec![], fail)),
            Expr::Var(name),
        )
    }

    fn pg_wrap_join(bind: Option<LocalDef>, body: Expr) -> Expr {
        match bind {
            None => body,
            Some(ld) => Expr::Let { binds: vec![ld], body: Box::new(body) },
        }
    }

    /// Lower a raw guard chain: guards try in order, a failing qualifier
    /// falls to the next guard, and past the last guard to `fallthrough`.
    fn guard_chain_expr(&mut self, raws: Vec<RawGuard>, fallthrough: Expr) -> Expr {
        let mut acc = fallthrough;
        for raw in raws.into_iter().rev() {
            let (bind, fail) = self.pg_join(acc);
            let mut inner = raw.body;
            for q in raw.quals.into_iter().rev() {
                inner = match q {
                    GuardQual::Bool(c) => Expr::If {
                        cond: Box::new(c),
                        then_branch: Box::new(inner),
                        else_branch: Box::new(fail.clone()),
                    },
                    GuardQual::Pat(p, e) => {
                        let irref = Self::pg_irrefutable(&p);
                        let mut brs = vec![CaseBranch { pattern: p, guards: vec![], body: Some(inner) }];
                        if !irref {
                            brs.push(CaseBranch {
                                pattern: Pattern::Wildcard,
                                guards: vec![],
                                body: Some(fail.clone()),
                            });
                        }
                        Expr::Case { scrutinee: Box::new(e), branches: brs }
                    }
                    GuardQual::Let(binds) => Expr::Let { binds, body: Box::new(inner) },
                };
            }
            acc = Self::pg_wrap_join(bind, inner);
        }
        acc
    }

    /// Fold an all-boolean guard list over a fall-through expression — the
    /// if/else spine the merged-clause lowering gives a PLAIN chain caught
    /// between pattern-guard clauses.
    fn pg_plain_guards(guards: Vec<Guard>, fail: &Expr) -> Expr {
        guards.into_iter().rev().fold(fail.clone(), |els, g| Expr::If {
            cond: Box::new(g.condition),
            then_branch: Box::new(g.body),
            else_branch: Box::new(els),
        })
    }

    /// Re-match `pat` against the already-bound variable `param` around
    /// `inner`: a wildcard vanishes, a variable becomes a lazy alias, and
    /// anything else a `case` whose fall-through (for a refutable pattern)
    /// is `fail`.
    fn pg_match_var(param: &str, pat: Pattern, inner: Expr, fail: &Expr) -> Expr {
        match pat {
            Pattern::Wildcard => inner,
            Pattern::Var(x) => Expr::Let {
                binds: vec![LocalDef::new(x, vec![], Expr::Var(param.to_string()))],
                body: Box::new(inner),
            },
            p => {
                let irref = Self::pg_irrefutable(&p);
                let mut brs = vec![CaseBranch { pattern: p, guards: vec![], body: Some(inner) }];
                if !irref {
                    brs.push(CaseBranch {
                        pattern: Pattern::Wildcard,
                        guards: vec![],
                        body: Some(fail.clone()),
                    });
                }
                Expr::Case { scrutinee: Box::new(Expr::Var(param.to_string())), branches: brs }
            }
        }
    }

    /// Collapse `rows` — the clauses of one function from some clause on —
    /// into one match over fresh parameters: each row re-matches its
    /// patterns against the parameters and runs its guards, and every
    /// "this row failed" edge (a pattern mismatch, an exhausted guard
    /// chain) jumps to a lazily let-bound JOIN POINT holding the rows after
    /// it; past the last row stands the match-failure error. Returns the
    /// parameter names and the body. Shared by the top-level clause groups
    /// with pattern guards (`desugar_pattern_guard_clauses`) and the local
    /// binding groups whose guards fall through to a later equation
    /// (`lower_local_equations`).
    fn collapse_match_rows(&mut self, name: &str, arity: usize, rows: Vec<MatchRow>) -> (Vec<String>, Expr) {
        let params: Vec<String> = (0..arity).map(|_| self.pg_fresh_name("a")).collect();
        let mut acc = Self::pg_error(&format!("Non-exhaustive patterns in {}", name));
        for row in rows.into_iter().rev() {
            let (bind, fail) = self.pg_join(acc);
            let inner = match row.rhs {
                RowRhs::Raw(raws) => self.guard_chain_expr(raws, fail.clone()),
                RowRhs::Plain(guards) => Self::pg_plain_guards(guards, &fail),
                RowRhs::Body(body) => body,
            };
            let mut inner = Self::wrap_where(row.where_binds, inner);
            let pats: Vec<(String, Pattern)> = params.iter().cloned().zip(row.patterns).collect();
            for (param, pat) in pats.into_iter().rev() {
                inner = Self::pg_match_var(&param, pat, inner, &fail);
            }
            acc = Self::pg_wrap_join(bind, inner);
        }
        (params, acc)
    }

    /// End-of-parse lowering for a function clause group in which some
    /// clause carries a raw (binding-qualifier) guard chain. The clauses
    /// from the first such chain on collapse into one clause over fresh
    /// parameters; earlier clauses keep the ordinary multi-clause path.
    fn desugar_pattern_guard_clauses(&mut self, name: &str, clauses: &mut Vec<Clause>) -> PResult<()> {
        let Some(k) = clauses.iter().position(|c| c.raw_guards.is_some()) else {
            return Ok(());
        };
        let arity = clauses[k].patterns.len();
        for c in &clauses[k..] {
            if c.patterns.len() != arity {
                return Err(Box::new(Diagnostic::parse_at(
                    format!(
                        "the equations of '{}' from its first pattern guard on must all \
                         have the same number of patterns",
                        name
                    ),
                    c.span,
                )));
            }
        }
        let span = clauses[k].span;
        let mut tail = clauses.split_off(k);
        // The collapsed clause stands for every clause of the tail, so it
        // carries the head operands of all of them.
        let operand_cons =
            tail.iter_mut().flat_map(|c| std::mem::take(&mut c.operand_cons)).collect();
        let rows = tail
            .into_iter()
            .map(|c| MatchRow {
                patterns: c.patterns,
                rhs: if let Some(raws) = c.raw_guards {
                    RowRhs::Raw(raws)
                } else if !c.guards.is_empty() {
                    RowRhs::Plain(c.guards)
                } else {
                    RowRhs::Body(c.body.expect("clause with neither guards nor body"))
                },
                where_binds: c.where_binds,
            })
            .collect();
        let (params, acc) = self.collapse_match_rows(name, arity, rows);
        clauses.push(Clause {
            patterns: params.into_iter().map(Pattern::Var).collect(),
            guards: vec![],
            body: Some(acc),
            where_binds: vec![],
            span,
            raw_guards: None,
            operand_cons,
        });
        Ok(())
    }

    /// The case-expression twin of `desugar_pattern_guard_clauses`: from
    /// the first alternative with a raw chain on, alternatives collapse
    /// into one final catch-all branch that binds the scrutinee value and
    /// re-matches each original alternative with join-point fall-through.
    fn desugar_pattern_guard_branches(
        &mut self,
        mut branches: Vec<CaseBranch>,
        raws: Vec<(usize, Vec<RawGuard>)>,
    ) -> Vec<CaseBranch> {
        if raws.is_empty() {
            return branches;
        }
        let k = raws[0].0;
        let mut raw_map: std::collections::HashMap<usize, Vec<RawGuard>> =
            raws.into_iter().collect();
        let tail = branches.split_off(k);
        let scrut = self.pg_fresh_name("s");
        // The same message codegen's own fall-off statement raises.
        let mut acc = Self::pg_error("Non-exhaustive patterns");
        for (i, b) in tail.into_iter().enumerate().rev() {
            let (bind, fail) = self.pg_join(acc);
            let inner = if let Some(rg) = raw_map.remove(&(k + i)) {
                self.guard_chain_expr(rg, fail.clone())
            } else if !b.guards.is_empty() {
                Self::pg_plain_guards(b.guards, &fail)
            } else {
                b.body.expect("case branch with neither guards nor body")
            };
            let inner = Self::pg_match_var(&scrut, b.pattern, inner, &fail);
            acc = Self::pg_wrap_join(bind, inner);
        }
        branches.push(CaseBranch {
            pattern: Pattern::Var(scrut),
            guards: vec![],
            body: Some(acc),
        });
        branches
    }

    /// A constructor atom: record construction `Con { f = v, ... }`
    /// (the brace may open on a continuation line indented past the
    /// layout block, like application arguments), or the bare
    /// constructor. The constructor name is already consumed.
    fn parse_con_atom(&mut self, name: String) -> PResult<Expr> {
            // Check for record construction: Con { field = val, ... }
            // The brace may open on a following line indented past the
            // enclosing block's column (a continuation line, like an
            // application argument's); at or left of the block column it
            // stands behind the block's `;` and is not seen here, so `Foo`
            // stays a bare constructor.
            if self.at(&Token::LeftBrace) {
                self.advance();
                let mut fields = Vec::new();
                loop {
                    if self.at(&Token::RightBrace) {
                        self.advance();
                        break;
                    }
                    let field_name = self.expect_ident()?;
                    self.expect(&Token::Eq)?;
                    let value = self.parse_expr()?;
                    fields.push((field_name, value));
                    if self.at(&Token::Comma) {
                        self.advance();
                    } else {
                        self.expect(&Token::RightBrace)?;
                        break;
                    }
                }
                Ok(Expr::RecordCon { constructor: name, fields })
            } else {
                Ok(Expr::Con(name))
            }
    }

    /// Haskell 2010 `aexp`. A `-` is not an atom (see `is_expr_atom_start`):
    /// negation belongs to the infix level, `parse_chain_operand`.
    fn parse_expr_atom_inner(&mut self) -> PResult<Expr> {
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
            // A module-qualified variable: `M.lookup`, `P.print`.
            Token::QualIdent(name) => {
                self.advance();
                Ok(Expr::Var(name))
            }
            // A constructor, bare or module-qualified (`M.Just`, `M.Rect`).
            Token::UpperIdent(name) | Token::QualUpperIdent(name) => {
                self.advance();
                self.parse_con_atom(name)
            }
            Token::LeftParen => self.parse_paren_expr(),
            Token::LeftBracket => self.parse_list_expr(),
            // `[Monday..]`: the lexer read `Monday..` as the qualified
            // operator `.` of a module `Monday` (Haskell 2010 §2.4, exactly
            // GHC's reading), so no expression starts here. Said in those
            // words, with the spelling that means the range.
            Token::QualOperator(op) => {
                let mut diag = self.err_expected("expression");
                let bare = crate::lexer::unqualified(&op);
                let module = crate::lexer::qualifier(&op).unwrap_or("");
                diag.notes.push(format!(
                     "'{op}' is one lexeme: the operator '{bare}' qualified with the module \
                     name '{module}' (a qualified name is written without spaces, so a '.' \
                     glued to a constructor name is a qualifier, not an operator)"
                ));
                if bare == "." {
                    diag.notes.push(format!(
                         "for an enumeration range write a space before the dots: \
                         '[{module} ..]'"
                    ));
                }
                Err(diag)
            }
            Token::If => {
                self.advance();
                let cond = self.parse_expr()?;
                // Haskell 2010 §3.6: `if exp [;] then exp [;] else exp` —
                // the optional semicolons let `then` and `else` stand at a
                // do block's statement column, where the layout rule puts
                // a `;` in front of them (DoAndIfThenElse).
                if matches!(self.after_separator(), Some(Token::Then)) {
                    self.skip_separators();
                }
                self.expect(&Token::Then)?;
                let then_branch = self.parse_stmt_expr()?;
                if matches!(self.after_separator(), Some(Token::Else)) {
                    self.skip_separators();
                }
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
                // Pending while the bindings are read: a do block inside
                // them may meet this let's `in`. Restored on the error
                // path too, since a speculative parse (a record update's
                // field value) may abandon this let and rewind.
                self.let_in_pending += 1;
                let binds = self.parse_let_binds();
                self.let_in_pending -= 1;
                let binds = binds?;
                self.expect(&Token::In)?;
                self.let_expr_body(binds)
            }
            Token::Do => self.parse_do_block(),
            Token::Backslash => self.parse_lambda(),
            _ => Err(self.err_expected("expression")),
        }
    }

    // --- Pattern parsing ---

    // Depth-guard wrapper: the grammar rule itself is in `parse_pattern_inner`.
    fn parse_pattern(&mut self) -> PResult<Pattern> {
        self.guarded("pattern", Self::parse_pattern_inner)
    }

    fn parse_pattern_inner(&mut self) -> PResult<Pattern> {
        let lhs = if let Token::UpperIdent(name) | Token::QualUpperIdent(name) = self.peek().clone() {
            self.advance();
            // `True` and `False` take no arguments here (as they always
            // have): a following atom is the next pattern. Which entity
            // they denote is decided by import resolution, which turns the
            // Prelude's into Boolean literal patterns.
            match name.as_str() {
                "True" | "False" => Pattern::Constructor { name, args: vec![] },
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

    fn is_pattern_start(&self) -> bool {
        self.is_pattern_atom_start()
    }

    /// Can the next token begin a case alternative's pattern? A pattern
    /// start, or the `-` of a negative literal that begins the pattern
    /// (`-1 -> …`; Haskell 2010's `lpat → - (integer | float)`, which a
    /// declaration's head may begin with as well: `starts_binding`).
    fn is_case_alt_start(&self) -> bool {
        self.is_pattern_start()
            || (matches!(self.peek(), Token::Operator(op) if op == "-")
                && matches!(
                    self.nth(1).token,
                    Token::IntLit(_) | Token::BigIntLit(_) | Token::NumLit(_)
                ))
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
                | Token::QualUpperIdent(_)
                // A qualified variable starts no pattern, but it stands
                // where one would: `parse_pattern_atom` reports it as GHC
                // does ("Qualified name in binding position") instead of
                // the pattern list ending in front of it.
                | Token::QualIdent(_)
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
            && op == "-" {
                match self.nth(1).token.clone() {
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
                self.bind_pattern_var(self.cur());
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
            // A constructor in atom position is nullary. Constructors with arguments must be parenthesized to
            // appear in argument position, e.g. `(T R a x b)`.
            Token::UpperIdent(name) | Token::QualUpperIdent(name) => {
                self.advance();
                Ok(Pattern::Constructor { name, args: vec![] })
            }
            // A pattern variable binds a name; a qualified one cannot.
            Token::QualIdent(name) => Err(self.qualified_binder_error(&name)),
            _ => Err(self.err_expected("pattern")),
        }
    }

    // --- Helpers ---

    fn expect_ident(&mut self) -> PResult<String> {
        match self.peek().clone() {
            Token::Ident(name) => {
                self.advance();
                Ok(name)
            }
            // Every caller wants the name of something this construct
            // introduces — a binder, a parameter, a field label, a type
            // variable — and a qualified spelling can only refer to an
            // imported entity.
            Token::QualIdent(name) => Err(self.qualified_binder_error(&name)),
            _ => Err(self.err_expected("identifier")),
        }
    }

    /// A name in operator position between backticks: a variable, bare or
    /// qualified (``x `O.plus` y``, Haskell 2010 §3: `qop → qvarop`).
    fn expect_backtick_name(&mut self) -> PResult<String> {
        match self.peek().clone() {
            Token::Ident(name) | Token::QualIdent(name) => {
                self.advance();
                Ok(name)
            }
            _ => Err(self.err_expected("identifier")),
        }
    }

    /// A module name: one `conid` or a dotted chain of them, which the
    /// lexer hands over as one `QualUpperIdent` (`Data.Map`).
    fn expect_module_name(&mut self) -> PResult<String> {
        match self.peek().clone() {
            Token::UpperIdent(name) | Token::QualUpperIdent(name) => {
                self.advance();
                Ok(name)
            }
            _ => Err(self.err_expected("module name")),
        }
    }

    /// GHC's "Qualified name in binding position": a definition, an
    /// equation's parameter, a lambda's parameter or a pattern binds a NEW
    /// name of this module, and a qualified spelling refers to an entity of
    /// the module imported under its qualifier — there is nothing it could
    /// bind. Haskell 2010 §4.4.3 (`funlhs → var apat {apat}`, `pat` over
    /// `var`) admits only unqualified variables there.
    fn qualified_binder_error(&self, name: &str) -> Box<Diagnostic> {
        let mut diag = self.err_here(format!("Qualified name in binding position: {name}"));
        let bare = crate::lexer::unqualified(name);
        let module = crate::lexer::qualifier(name).unwrap_or("");
        diag.notes.push(format!(
            "'{name}' is a qualified name — the '{bare}' of a module imported as \
             '{module}' — and refers to that module's entity; a binding introduces a \
             name of this module, written unqualified: '{bare}'"
        ));
        diag
    }

    fn expect_upper_ident(&mut self) -> PResult<String> {
        match self.peek().clone() {
            Token::UpperIdent(name) => {
                self.advance();
                Ok(name)
            }
            _ => Err(self.err_expected("type/constructor name")),
        }
    }
}

/// The diagnostic for input nested past `crate::MAX_NESTING_DEPTH`. `what`
/// names the construct ("expression", "type", "pattern").
pub(crate) fn nesting_error(what: &str, span: Span) -> Box<Diagnostic> {
    let mut diag = Box::new(Diagnostic::parse_at(
        format!("{} nested too deeply (limit {})", what, crate::MAX_NESTING_DEPTH),
        span,
    ));
    diag.notes.push(
        "the compiler reads nested syntax with bounded recursion so it \
         can report this error instead of crashing on pathological \
         input; restructure the code to nest less, e.g. by splitting \
         it into smaller definitions"
            .to_string(),
    );
    diag
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

/// Parse a token stream into a module. On failure, returns every syntax
/// error found (the parser recovers at declaration boundaries), in source
/// order; the list is never empty.
///
/// Infix expressions and infix types are returned as flat chains
/// (`Expr::OpChain`, `Expr::Section`, `Type::OpChain`): an operator's
/// fixity belongs to the entity it denotes, which only module resolution
/// knows, so the chains are grouped there (see `fixity`).
///
/// Takes the tokens by value: every caller owns its freshly lexed vector
/// and is done with it here, so the parser holds it directly instead of
/// deep-cloning the whole stream per module.
pub fn parse(tokens: Vec<Located>) -> Result<Module, Vec<Diagnostic>> {
    Parser::new(tokens).parse_module()
}
