/// Source location
#[derive(Debug, Clone, Copy, Default)]
pub struct Span {
    pub line: usize,
    pub col: usize,
}

impl Span {
    pub fn new(line: usize, col: usize) -> Self {
        Span { line, col }
    }
}

impl std::fmt::Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

/// An mll module corresponds to a single .mll file.
#[derive(Debug, Clone)]
pub struct Module {
    pub decls: Vec<Decl>,
    /// Module export list. None = export everything, Some = only these names.
    pub exports: Option<Vec<String>>,
    /// Names imported but hidden (from modules with export lists).
    /// The typechecker rejects direct references to these.
    pub hidden: std::collections::HashSet<String>,
}

/// Top-level declarations.
#[derive(Debug, Clone)]
pub enum Decl {
    /// Type signature: `add :: Int -> Int -> Int`
    TypeSig {
        name: String,
        ty: Type,
    },
    /// Function definition: `add a b = a + b`
    FunDef {
        name: String,
        clauses: Vec<Clause>,
    },
    /// Data type: `data Tree a = Branch (Tree a) (Tree a) | Leaf a`
    DataDef {
        name: String,
        type_vars: Vec<String>,
        constructors: Vec<Constructor>,
        deriving: Vec<String>,
    },
    /// Newtype: `newtype A = Int`
    NewtypeDef {
        name: String,
        type_vars: Vec<String>,
        inner: Type,
    },
    /// Typeclass declaration: `class Eq a => Ord a where compare :: a -> a -> Int`
    ClassDecl {
        name: String,
        type_var: String,
        superclasses: Vec<String>,
        methods: Vec<ClassMethod>,
    },
    /// Typeclass instance: `instance Show Int where show x = ...`
    InstanceDecl {
        class_name: String,
        target_type: Type,
        /// Instance context: `instance (Show a, Eq a) => C (T a)` carries
        /// [Show a, Eq a]. The context is what lets a method body use those
        /// class methods on the instance's type variables, and what a use of
        /// the instance at a concrete type must satisfy.
        context: Vec<Constraint>,
        methods: Vec<InstanceMethod>,
    },
    /// Export declaration: `export add :: Int -> Int -> Int`
    ExportSig {
        name: String,
        ty: Type,
    },
    /// Type family: `type family Element container where Element [a] = a`
    TypeFamily {
        name: String,
        /// The header parameter names (`container`). Not matched against — the
        /// equations do the matching — but their COUNT fixes the family's
        /// arity/kind even when it has zero equations (an initially-empty
        /// family the compiler later extends, like `Rep`).
        params: Vec<String>,
        equations: Vec<TypeFamilyEq>,
    },
    /// Import: `import Data.Tree (depth, Tree(..))`
    Import {
        module_path: Vec<String>,
        items: ImportItems,
    },
    /// Type alias: `type String = [Char]`
    TypeAlias {
        name: String,
        params: Vec<String>,
        ty: Type,
    },
    /// Fixity declaration: `infixl 6 +` or `infixr 5 :`
    FixityDecl {
        assoc: Assoc,
        prec: u8,
        op: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    Left,
    Right,
    None,
}

#[derive(Debug, Clone)]
pub enum ImportItems {
    All,
    Specific(Vec<ImportItem>),
    Hiding(Vec<ImportItem>),
    Qualified(String),
}

#[derive(Debug, Clone)]
pub enum ImportItem {
    Value(String),
    TypeAll(String),
    TypeOnly(String),
}

/// A single clause of a function definition (pattern matching).
///
/// A clause has EITHER a plain body (`f x = e`, `guards` empty, `body`
/// `Some`) OR a guard chain (`f x | c = e | ...`, `guards` non-empty,
/// `body` `None`) — never both. `Option` makes the exclusion structural;
/// this used to be an `Expr::Var("undefined")` sentinel every downstream
/// pass had to silently know about.
#[derive(Debug, Clone)]
pub struct Clause {
    pub patterns: Vec<Pattern>,
    pub guards: Vec<Guard>,
    pub body: Option<Expr>,
    pub where_binds: Vec<LocalDef>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Guard {
    pub condition: Expr,
    pub body: Expr,
}

#[derive(Debug, Clone)]
pub struct LocalDef {
    pub name: String,
    pub patterns: Vec<Pattern>,
    pub body: Expr,
}

/// A method signature (and optional default implementation) in a class declaration
#[derive(Debug, Clone)]
pub struct ClassMethod {
    pub name: String,
    pub ty: Type,
    pub default_clauses: Option<Vec<Clause>>,
}

/// A method implementation in an instance declaration
#[derive(Debug, Clone)]
pub struct InstanceMethod {
    pub name: String,
    pub clauses: Vec<Clause>,
}

/// Patterns for pattern matching.
#[derive(Debug, Clone)]
pub enum Pattern {
    /// Variable binding: `x`
    Var(String),
    /// Wildcard: `_`
    Wildcard,
    /// Constructor pattern: `Just x`, `Branch l r`
    Constructor {
        name: String,
        args: Vec<Pattern>,
    },
    /// Literal pattern: `0`, `"hello"`
    LitPat(Literal),
    /// Parenthesized pattern
    Paren(Box<Pattern>),
    /// Tuple pattern: `(x, y, z)`
    Tuple(Vec<Pattern>),
}

impl Pattern {
    /// Call `f` on every variable this pattern binds, in source order — THE
    /// enumeration of a surface pattern's binders (the parser's tuple-let
    /// selectors and the module resolver's scope tracking both need it; each
    /// once carried its own walk).
    pub fn for_each_var(&self, f: &mut impl FnMut(&str)) {
        match self {
            Pattern::Var(n) => f(n),
            Pattern::Wildcard | Pattern::LitPat(_) => {}
            Pattern::Constructor { args, .. } => {
                for a in args {
                    a.for_each_var(f);
                }
            }
            Pattern::Paren(inner) => inner.for_each_var(f),
            Pattern::Tuple(elems) => {
                for e in elems {
                    e.for_each_var(f);
                }
            }
        }
    }

    /// The variables this pattern binds, in source order.
    pub fn var_names(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.for_each_var(&mut |v| out.push(v.to_string()));
        out
    }
}

/// Expressions.
#[derive(Debug, Clone)]
pub enum Expr {
    /// Variable reference
    Var(String),
    /// Constructor reference
    Con(String),
    /// Literal value
    Lit(Literal),
    /// Function application: `f x`
    App(Box<Expr>, Box<Expr>),
    /// Lambda: `\x -> e`
    Lambda {
        params: Vec<String>,
        body: Box<Expr>,
    },
    /// Infix operator application: `a + b`
    InfixApp {
        op: String,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// Prefix negation: `-x`
    Negate(Box<Expr>),
    /// If-then-else
    If {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Box<Expr>,
    },
    /// Case expression
    Case {
        scrutinee: Box<Expr>,
        branches: Vec<CaseBranch>,
    },
    /// Let-in expression
    Let {
        binds: Vec<LocalDef>,
        body: Box<Expr>,
    },
    /// Do-notation block
    Do(Vec<DoStmt>),
    /// Type ascription: expr :: Type
    Ascription(Box<Expr>, Type),
    /// Record construction with named fields: Person { perName = "Alice", perAge = 30 }
    RecordCon {
        constructor: String,
        fields: Vec<(String, Expr)>,
    },
    /// Record update: expr { field = newVal, ... }
    RecordUpdate {
        expr: Box<Expr>,
        updates: Vec<(String, Expr)>,
    },
    /// Parenthesized expression
    Paren(Box<Expr>),
    /// Operator as function: `(+)`
    OpFunc(String),
    /// Tuple expression: `(1, "hello", True)`
    Tuple(Vec<Expr>),
    /// A transparent source-location marker wrapping a statement-boundary
    /// expression (a do-statement, a let/where binding body, a case-branch or
    /// guard body). It carries no semantics — every pass treats `Spanned(_, e)`
    /// exactly as `e` — but lets the type checker attribute an error to the
    /// offending statement's line instead of the enclosing clause head. The
    /// wrapper is erased when the checker lowers to typed IR, so it never
    /// reaches desugaring's output types or codegen. Introduced by the parser
    /// at statement boundaries and survives `do`-desugaring (which is why it
    /// lives on the expression, not on the statement nodes that desugar away).
    Spanned(Span, Box<Expr>),
}

impl Expr {
    /// Peel any `Spanned` markers to reach the underlying expression, for the
    /// occasional structural inspection that must see the real node shape.
    pub fn unspanned(&self) -> &Expr {
        let mut e = self;
        while let Expr::Spanned(_, inner) = e {
            e = inner;
        }
        e
    }

    /// Rebuild this node with `f` applied to each DIRECT sub-expression.
    ///
    /// This is the one place that knows where an `Expr`'s child expressions
    /// live — including the ones tucked inside case branches, guards,
    /// let/do `LocalDef` bodies, do-statements, and record fields. Walkers
    /// call this instead of re-encoding that structure, so a new variant only
    /// needs a match arm here (and in `for_each_subexpr`), not in every pass.
    ///
    /// Deliberately NOT recursive: `f` receives each child once and decides
    /// whether to recurse (typically by calling itself through this helper).
    /// That keeps pass-specific concerns — scope tracking, pre- vs post-order
    /// — in the pass, not here.
    ///
    /// Types are not visited (`Ascription` keeps its type untouched); a pass
    /// that rewrites types must handle that variant itself.
    pub fn map_subexprs(self, f: &mut impl FnMut(Expr) -> Expr) -> Expr {
        match self {
            // Leaves: names and literals carry no sub-expressions.
            Expr::Var(_) | Expr::Con(_) | Expr::Lit(_) | Expr::OpFunc(_) => self,
            Expr::App(func, arg) => Expr::App(Box::new(f(*func)), Box::new(f(*arg))),
            Expr::Lambda { params, body } => Expr::Lambda {
                params,
                body: Box::new(f(*body)),
            },
            Expr::InfixApp { op, lhs, rhs } => Expr::InfixApp {
                op,
                lhs: Box::new(f(*lhs)),
                rhs: Box::new(f(*rhs)),
            },
            Expr::Negate(x) => Expr::Negate(Box::new(f(*x))),
            Expr::If { cond, then_branch, else_branch } => Expr::If {
                cond: Box::new(f(*cond)),
                then_branch: Box::new(f(*then_branch)),
                else_branch: Box::new(f(*else_branch)),
            },
            Expr::Case { scrutinee, branches } => Expr::Case {
                scrutinee: Box::new(f(*scrutinee)),
                branches: branches.into_iter().map(|br| CaseBranch {
                    pattern: br.pattern,
                    guards: br.guards.into_iter().map(|g| Guard {
                        condition: f(g.condition),
                        body: f(g.body),
                    }).collect(),
                    body: br.body.map(&mut *f),
                }).collect(),
            },
            Expr::Let { binds, body } => Expr::Let {
                binds: binds.into_iter().map(|ld| LocalDef {
                    name: ld.name,
                    patterns: ld.patterns,
                    body: f(ld.body),
                }).collect(),
                body: Box::new(f(*body)),
            },
            Expr::Do(stmts) => Expr::Do(stmts.into_iter().map(|s| match s {
                DoStmt::Bind { name, expr } => DoStmt::Bind { name, expr: f(expr) },
                DoStmt::Expr(e) => DoStmt::Expr(f(e)),
                DoStmt::DoLet { binds } => DoStmt::DoLet {
                    binds: binds.into_iter().map(|ld| LocalDef {
                        name: ld.name,
                        patterns: ld.patterns,
                        body: f(ld.body),
                    }).collect(),
                },
                DoStmt::PatternBind { pattern, expr } =>
                    DoStmt::PatternBind { pattern, expr: f(expr) },
            }).collect()),
            Expr::Ascription(x, t) => Expr::Ascription(Box::new(f(*x)), t),
            Expr::RecordCon { constructor, fields } => Expr::RecordCon {
                constructor,
                fields: fields.into_iter().map(|(n, e)| (n, f(e))).collect(),
            },
            Expr::RecordUpdate { expr, updates } => Expr::RecordUpdate {
                expr: Box::new(f(*expr)),
                updates: updates.into_iter().map(|(n, e)| (n, f(e))).collect(),
            },
            Expr::Paren(x) => Expr::Paren(Box::new(f(*x))),
            Expr::Tuple(xs) => Expr::Tuple(xs.into_iter().map(|x| f(x)).collect()),
            Expr::Spanned(sp, inner) => Expr::Spanned(sp, Box::new(f(*inner))),
        }
    }

    /// Call `f` on each DIRECT sub-expression, by reference. The read-only
    /// twin of `map_subexprs` — same child set, same non-recursive contract —
    /// for collector passes that build up state instead of rewriting.
    pub fn for_each_subexpr(&self, f: &mut impl FnMut(&Expr)) {
        match self {
            Expr::Var(_) | Expr::Con(_) | Expr::Lit(_) | Expr::OpFunc(_) => {}
            Expr::App(func, arg) => { f(func); f(arg); }
            Expr::Lambda { body, .. } => f(body),
            Expr::InfixApp { lhs, rhs, .. } => { f(lhs); f(rhs); }
            Expr::Negate(x) => f(x),
            Expr::If { cond, then_branch, else_branch } => {
                f(cond);
                f(then_branch);
                f(else_branch);
            }
            Expr::Case { scrutinee, branches } => {
                f(scrutinee);
                for br in branches {
                    for g in &br.guards {
                        f(&g.condition);
                        f(&g.body);
                    }
                    if let Some(b) = &br.body { f(b); }
                }
            }
            Expr::Let { binds, body } => {
                for ld in binds { f(&ld.body); }
                f(body);
            }
            Expr::Do(stmts) => {
                for s in stmts {
                    match s {
                        DoStmt::Bind { expr, .. }
                        | DoStmt::Expr(expr)
                        | DoStmt::PatternBind { expr, .. } => f(expr),
                        DoStmt::DoLet { binds } => {
                            for ld in binds { f(&ld.body); }
                        }
                    }
                }
            }
            Expr::Ascription(x, _) => f(x),
            Expr::RecordCon { fields, .. } => {
                for (_, e) in fields { f(e); }
            }
            Expr::RecordUpdate { expr, updates } => {
                f(expr);
                for (_, e) in updates { f(e); }
            }
            Expr::Paren(x) => f(x),
            Expr::Tuple(xs) => {
                for x in xs { f(x); }
            }
            Expr::Spanned(_, inner) => f(inner),
        }
    }
}

/// One branch of a `case`. Same body/guards exclusion as [`Clause`]:
/// exactly one of `body` (`Some`) and `guards` (non-empty) is present.
#[derive(Debug, Clone)]
pub struct CaseBranch {
    pub pattern: Pattern,
    pub guards: Vec<Guard>,
    pub body: Option<Expr>,
}

/// Do-notation statements.
#[derive(Debug, Clone)]
pub enum DoStmt {
    /// `x <- expr`
    Bind { name: String, expr: Expr },
    /// `expr` (bare expression)
    Expr(Expr),
    /// `let x = expr` — a whole binding group. All bindings in the group share
    /// one mutually-recursive scope (Haskell 2010 letrec), so declaration order
    /// within the group is irrelevant.
    DoLet { binds: Vec<LocalDef> },
    /// `(a, b) <- expr` (pattern bind). A `let (a, b) = expr` statement is
    /// NOT a variant of its own: the parser desugars it into `DoLet`
    /// selector bindings, exactly like a let-expression's tuple binding.
    PatternBind { pattern: Pattern, expr: Expr },
}

/// Literal values.
#[derive(Debug, Clone)]
pub enum Literal {
    Integer(i64),
    /// An integer literal too large for `i64`, kept as its decimal string. Only
    /// an `Integer` (arbitrary precision); parsed to a bignum at codegen.
    BigInteger(String),
    Number(f64),
    /// A string literal as its decoded BYTE sequence (mata-ll's `String` is
    /// the Lua string — a byte array; see HASKDIFF.md "Strings and
    /// ByteStrings"). ASCII-only string literals constructed inside the
    /// compiler use `b"...".to_vec()` / `.into_bytes()`.
    Str(Vec<u8>),
    Bool(bool),
    Unit,
}

/// A multiplicity annotation as written in a source type: `%1`, `%Many` /
/// `%'Many`, or a named multiplicity VARIABLE (`a %m -> b` — multiplicity
/// polymorphism). The named form carries the source name; the typechecker's
/// `ast_type_to_ty` resolves each distinct name of a signature to one rigid
/// multiplicity variable (`types::Mult::Rigid`).
#[derive(Debug, Clone)]
pub enum MultAnn {
    One,
    Many,
    Var(String),
}

/// Type representation.
#[derive(Debug, Clone)]
pub enum Type {
    /// Named type: `Int`, `String`, `Tree`
    Con(String),
    /// Type variable: `a`, `b`
    Var(String),
    /// Type application: `Maybe String`, `Tree a`
    App(Box<Type>, Box<Type>),
    /// Function type: `a -> b`. The multiplicity is `Many` for a plain `->`,
    /// `One` for a `%1`-annotated arrow (`a %1 -> b`: the function consumes
    /// the argument exactly once — see `types::Mult`), and `Var` for a named
    /// multiplicity variable (`a %m -> b`).
    Arrow(Box<Type>, Box<Type>, MultAnn),
    /// List/Array type: `[a]`
    List(Box<Type>),
    /// IO type: `IO a` (Pure provenance)
    IO(Box<Type>),
    /// Scoped Lua IO: `LuaIO s a`
    ScopedLuaIO { scope_var: String, inner: Box<Type> },
    /// Rank-2 forall: `forall s. ty`
    Forall { var: String, inner: Box<Type> },
    /// Unit type: `()`
    Unit,
    /// Parenthesized type
    Paren(Box<Type>),
    /// FFI pure call: `LuaPure "math.sin" Number` reduces to `Number`
    LuaPure { lua_name: String, result: Box<Type> },
    /// FFI effectful call: `LuaIO "math.random" Number` reduces to `IO Number`
    LuaIO { lua_name: String, result: Box<Type> },
    /// FFI iterator collected into a lazy list. The type argument is always an
    /// explicit list `[E]` naming the RESULT: `LuaIterator "f" [E]` reduces to
    /// `[E]` and the iterator yields one `E` per step, each decoded as the
    /// element type. The parser rejects a bare (non-list) element type.
    LuaIterator { lua_name: String, result: Box<Type> },
    /// Tuple type: `(Int, String, Bool)`
    Tuple(Vec<Type>),
    /// FFI with Lua error convention: `LuaTry "io.open" (Either String FileHandle)`
    /// reduces to `IO (Either String FileHandle)`. The wrapped call uses Lua's
    /// `(val, err)` two-return convention (a nil value is a failure): failure
    /// becomes `Left err`, success becomes `Right val`. The result MUST be
    /// written as `Either String a`.
    LuaTry { lua_name: String, result: Box<Type> },
    /// FFI pure call guarded by `pcall`: `LuaCatch "foo.bar" (Either String T)`
    /// reduces to `Either String T`. A raised Lua `error(...)` becomes `Left msg`,
    /// success becomes `Right T`. The result MUST be written as `Either String a`.
    LuaCatch { lua_name: String, result: Box<Type> },
    /// Effectful counterpart of `LuaCatch`: `LuaIOCatch "foo.bar" (Either String T)`
    /// reduces to `IO (Either String T)`. Same `pcall` error capture, deferred as
    /// an IO action. The result MUST be written as `Either String a`.
    LuaIOCatch { lua_name: String, result: Box<Type> },
    /// Typeclass constraint: `Show a =>`
    Constrained {
        constraints: Vec<Constraint>,
        ty: Box<Type>,
    },
    /// Promoted data constructor (DataKinds): `'Empty`, `'NonEmpty`
    Promoted(String),
}

/// A type family equation: `Element [a] = a`
#[derive(Debug, Clone)]
pub struct TypeFamilyEq {
    /// The argument patterns (e.g., [a], (HashMap k v))
    pub args: Vec<Type>,
    /// The result type
    pub result: Type,
}

#[derive(Debug, Clone)]
pub struct Constraint {
    pub class_name: String,
    pub type_arg: Type,
}

/// Data constructor definition. `external_name` is the optional rename from
/// `Con field-types as "name"` — the per-constructor twin of the field-level
/// `as "key"` rename (see `RecordField`). It sets the constructor's external
/// TAG: the string a derived ToJSON/FromJSON codec writes and reads to tell
/// the constructors of a sum type apart. Nothing else names constructors
/// externally — at the Lua boundary a constructor is a positional integer
/// tag, not a name — so the rename affects only the JSON codec. Show,
/// construction, pattern matching and the runtime tag keep `name`.
#[derive(Debug, Clone)]
pub struct Constructor {
    pub name: String,
    pub external_name: Option<String>,
    pub fields: ConstructorFields,
    /// GADT constructor type signature, e.g. `Int -> Expr Int`.
    /// When present, field types and result type are extracted from this
    /// instead of from `fields`.
    pub gadt_type: Option<Type>,
    /// Existential type variables: `data ShowBox = forall a. MkShowBox a (a -> String)`
    /// Here `a` is existential — it appears in field types but not in the data type's parameters.
    pub existential_vars: Vec<String>,
    /// Constraints on existential type variables: `forall a. Show a => MkShowBox a`
    pub existential_constraints: Vec<Constraint>,
}

#[derive(Debug, Clone)]
pub enum ConstructorFields {
    /// Positional fields: `Branch (Tree a) (Tree a)`
    Positional(Vec<Type>),
    /// Named fields (record): `Person { name :: String, age :: Number }`
    Named(Vec<RecordField>),
}

/// A named record field. `external_key` is the optional rename from
/// `fieldName as "key" :: T` — one shared external name for the field at
/// every boundary where the record leaves mata-ll: the key in the runtime
/// Lua table of a `deriving (LuaDict)` type AND the JSON object key of a
/// derived ToJSON/FromJSON codec. The Haskell-side accessor, record syntax
/// and pattern matching keep `name`.
#[derive(Debug, Clone)]
pub struct RecordField {
    pub name: String,
    pub external_key: Option<String>,
    pub ty: Type,
}

impl RecordField {
    /// The name this field presents at external boundaries (Lua table key,
    /// JSON object key): the `as "key"` rename when present, the field name
    /// otherwise.
    pub fn effective_key(&self) -> &str {
        self.external_key.as_deref().unwrap_or(&self.name)
    }
}

impl Constructor {
    /// The tag this constructor presents at the JSON boundary: the
    /// `as "name"` rename when present, the constructor name otherwise —
    /// the constructor-level twin of `RecordField::effective_key`.
    pub fn effective_tag(&self) -> &str {
        self.external_name.as_deref().unwrap_or(&self.name)
    }
}
