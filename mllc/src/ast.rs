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
    /// Type signature: `add :: Integer -> Integer -> Integer`
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
    /// Newtype: `newtype A = Integer`
    NewtypeDef {
        name: String,
        type_vars: Vec<String>,
        inner: Type,
    },
    /// Typeclass declaration: `class Eq a => Ord a where compare :: a -> a -> Integer`
    ClassDecl {
        name: String,
        type_var: String,
        superclasses: Vec<String>,
        methods: Vec<ClassMethod>,
    },
    /// Typeclass instance: `instance Show Integer where show x = ...`
    InstanceDecl {
        class_name: String,
        target_type: Type,
        methods: Vec<InstanceMethod>,
    },
    /// Export declaration: `export add :: Integer -> Integer -> Integer`
    ExportSig {
        name: String,
        ty: Type,
    },
    /// Type family: `type family Element container where Element [a] = a`
    TypeFamily {
        name: String,
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

#[derive(Debug, Clone, Copy)]
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
#[derive(Debug, Clone)]
pub struct Clause {
    pub patterns: Vec<Pattern>,
    pub guards: Vec<Guard>,
    pub body: Expr,
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
}

#[derive(Debug, Clone)]
pub struct CaseBranch {
    pub pattern: Pattern,
    pub guards: Vec<Guard>,
    pub body: Expr,
}

/// Do-notation statements.
#[derive(Debug, Clone)]
pub enum DoStmt {
    /// `x <- expr`
    Bind { name: String, expr: Expr },
    /// `expr` (bare expression)
    Expr(Expr),
    /// `let x = expr`
    DoLet { name: String, expr: Expr },
}

/// Literal values.
#[derive(Debug, Clone)]
pub enum Literal {
    Integer(i64),
    Number(f64),
    Str(String),
    Bool(bool),
    Unit,
}

/// Type representation.
#[derive(Debug, Clone)]
pub enum Type {
    /// Named type: `Integer`, `String`, `Tree`
    Con(String),
    /// Type variable: `a`, `b`
    Var(String),
    /// Type application: `Maybe String`, `Tree a`
    App(Box<Type>, Box<Type>),
    /// Function type: `a -> b`
    Arrow(Box<Type>, Box<Type>),
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
    /// FFI iterator: `LuaIterator "string.gmatch" String` reduces to `[String]`
    LuaIterator { lua_name: String, result: Box<Type> },
    /// Tuple type: `(Integer, String, Bool)`
    Tuple(Vec<Type>),
    /// FFI with Lua error convention: `LuaTry "io.open" FileHandle` reduces to `IO (Either String FileHandle)`
    LuaTry { lua_name: String, result: Box<Type> },
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

/// Data constructor definition.
#[derive(Debug, Clone)]
pub struct Constructor {
    pub name: String,
    pub fields: ConstructorFields,
    /// GADT constructor type signature, e.g. `Integer -> Expr Integer`.
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
    Named(Vec<(String, Type)>),
}
