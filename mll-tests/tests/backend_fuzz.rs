//! Backend fuzzing with TYPE-CORRECT programs — the "open infra work" the
//! parser fuzzer's header used to name. Where parser_fuzz throws mostly
//! ill-typed text at the front end (1 in ~25 inputs reaches the pipeline),
//! every program here compiles by construction, so every input exercises
//! the typechecker, mono, demand analysis, codegen, and the optimizer.
//!
//! The oracle is a strict Rust reference evaluator over the generator's own
//! typed AST. The generated fragment is TOTAL — exhaustive cases only,
//! `div`/`mod` divisors are positive literals, `head`/`maximum`/`minimum`
//! only ever see a syntactic cons — so strict evaluation computes the same
//! values lazy GHC semantics would, and the expected stdout is
//! byte-comparable. `show` rendering follows GHC's showsPrec (negative and
//! constructor arguments parenthesized at precedence 11), which mata-ll's
//! show is byte-oracled against. What a total fragment cannot see — a
//! skipped force, a missed bottom — is covered by compiling every second
//! program through compile_with_whnf_refutation: the WHNF claim checkers
//! (see runtime.lua) then run over machine-generated shapes no hand-written
//! corpus carries.
//!
//! The generator leans into the shapes that historically broke: point-free
//! definitions, definitions with fewer patterns than arrows (eta padding),
//! builtins and the polymorphic scaffold used at function-typed
//! instantiations (arity widening), first-class ($) and (.), higher-order
//! prelude generics, and nested constructor patterns. The grown fragment
//! adds the A14-A20 surface: String (opaque, `<>`/`show`/`mconcat`, GHC
//! escape rendering), arbitrary-precision Integer (multi-limb literals,
//! floor div/mod, kept inside an i128 headroom budget so the reference
//! stays exact), Ordering, structural Eq/Ord operators and
//! sort/elem/max/min/compare at every fun-free type, HashMap operations
//! through both the scalar and the encoded-structural key paths (iteration
//! is Ord-sorted, so it is deterministic), generated top-level helpers
//! (guard chains, where-binds, eta-padded definitions, and a polymorphic
//! where-bind instantiated at several types — the A19 generalization), and
//! do-blocks with `let` statements and pattern binds through `return`.
//!
//! Everything is deterministic and offline: each program derives from
//! (BATCH_SEED, index) through SplitMix64, so any failure reproduces by
//! seed alone, and the failure message carries the source and both outputs.

use std::path::Path;
use std::rc::Rc;

/// SplitMix64 — same tiny deterministic RNG as parser_fuzz.rs.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed)
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn chance(&mut self, num: u64, den: u64) -> bool {
        self.next() % den < num
    }
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
enum Ty {
    Int,
    Integer,
    Bool,
    Str,
    Ordering,
    List(Box<Ty>),
    Pair(Box<Ty>, Box<Ty>),
    Maybe(Box<Ty>),
    Fun(Box<Ty>, Box<Ty>),
}

impl Ty {
    fn list(t: Ty) -> Ty {
        Ty::List(Box::new(t))
    }
    fn pair(a: Ty, b: Ty) -> Ty {
        Ty::Pair(Box::new(a), Box::new(b))
    }
    fn fun(a: Ty, b: Ty) -> Ty {
        Ty::Fun(Box::new(a), Box::new(b))
    }
    /// Rendered .mll type (fully parenthesized where nesting could bind).
    fn render(&self) -> String {
        match self {
            Ty::Int => "Int".into(),
            Ty::Integer => "Integer".into(),
            Ty::Bool => "Bool".into(),
            Ty::Str => "String".into(),
            Ty::Ordering => "Ordering".into(),
            Ty::List(t) => format!("[{}]", t.render()),
            Ty::Pair(a, b) => format!("({}, {})", a.render(), b.render()),
            Ty::Maybe(t) => format!("(Maybe {})", t.render()),
            Ty::Fun(a, b) => format!("({} -> {})", a.render(), b.render()),
        }
    }
    /// Is a value of this type printable (has a derived Show the reference
    /// renderer also implements)? Functions are not. Every fun-free type
    /// in the fragment is also Eq/Ord-comparable (structurally where not
    /// scalar), so this doubles as the comparability gate.
    fn showable(&self) -> bool {
        match self {
            Ty::Int | Ty::Integer | Ty::Bool | Ty::Str | Ty::Ordering => true,
            Ty::List(t) | Ty::Maybe(t) => t.showable(),
            Ty::Pair(a, b) => a.showable() && b.showable(),
            Ty::Fun(..) => false,
        }
    }
    /// May this type key a HashMap? Scalars Int/Bool/String take the
    /// direct table-index path; list/pair/Maybe composites take the A17
    /// encoded-entry path. Integer has no Hashable instance (a limb table
    /// is not a scalar — the typechecker rejects it), and Ordering none.
    fn keyable(&self) -> bool {
        match self {
            Ty::Int | Ty::Bool | Ty::Str => true,
            Ty::List(t) | Ty::Maybe(t) => t.keyable(),
            Ty::Pair(a, b) => a.keyable() && b.keyable(),
            Ty::Integer | Ty::Ordering | Ty::Fun(..) => false,
        }
    }
}

/// A random type for a value position. `depth` bounds nesting; function
/// types appear only where `fun_ok` (printed positions exclude them).
fn gen_ty(r: &mut Rng, depth: usize, fun_ok: bool) -> Ty {
    if depth == 0 {
        return match r.below(12) {
            0..=3 => Ty::Int,
            4 | 5 => Ty::Bool,
            6 | 7 => Ty::Str,
            8 | 9 => Ty::Integer,
            10 => Ty::Ordering,
            _ => Ty::Int,
        };
    }
    match r.below(if fun_ok { 12 } else { 10 }) {
        0 | 1 => Ty::Int,
        2 => Ty::Integer,
        3 => Ty::Bool,
        4 => Ty::Str,
        5 => Ty::Ordering,
        6 | 7 => Ty::list(gen_ty(r, depth - 1, false)),
        8 => Ty::pair(gen_ty(r, depth - 1, false), gen_ty(r, depth - 1, false)),
        9 => Ty::Maybe(Box::new(gen_ty(r, depth - 1, false))),
        _ => Ty::fun(gen_ty(r, depth - 1, false), gen_ty(r, depth - 1, fun_ok)),
    }
}

/// A random HashMap KEY type (see Ty::keyable).
fn gen_key_ty(r: &mut Rng, depth: usize) -> Ty {
    if depth == 0 || r.chance(1, 2) {
        return match r.below(4) {
            0 | 1 => Ty::Int,
            2 => Ty::Bool,
            _ => Ty::Str,
        };
    }
    match r.below(3) {
        0 => Ty::list(gen_key_ty(r, depth - 1)),
        1 => Ty::pair(gen_key_ty(r, depth - 1), gen_key_ty(r, depth - 1)),
        _ => Ty::Maybe(Box::new(gen_key_ty(r, depth - 1))),
    }
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Bin {
    Add,
    Sub,
    Mul,
    Eq,
    Ne,
    Lt,
    Le,
    And,
    Or,
}

/// Prelude / scaffold functions the generator may call. Each entry is a
/// distinct backend path; the polymorphic ones are instantiated at concrete
/// types when selected, so the program stays annotated and well-typed.
#[derive(Clone, Debug, PartialEq)]
enum P {
    Map,
    Filter,
    Foldr,
    Foldl,
    Take,
    Drop,
    Reverse,
    Length,
    Sum,  // sum :: [Int] -> Int instantiation
    SumI, // sum :: [Integer] -> Integer (separate so eval stays typed)
    Null,
    ZipWith,
    Fst,
    Snd,
    Head, // only ever applied to a syntactic cons (nonempty evidence)
    Seq,
    Id,
    Const,
    Flip,
    Twice,     // scaffold: fuzzTwice f x = f (f x)
    CompApp,   // ((f . g) x) — the composition emission
    DollarApp, // (f $ x)
    // --- structural Eq/Ord surface (A16) ---
    Sort,
    Elem,
    MaxP,
    MinP,
    Cmp,
    Maximum, // only ever applied to a syntactic cons
    Minimum, // likewise
    // --- String (opaque; <>/show/mconcat) ---
    ShowP,
    Append, // (<>) — rendered infix
    Mconcat,
    // --- Integer ---
    ToInteger,
    NegateI,
    AbsI,
    Even,
    Odd,
    // --- HashMap (A17/A20); rendered as composites over hmFromList so the
    // map value never needs a Show — iteration output is Ord-sorted ---
    HmToList,
    HmKeys,
    HmValues,
    HmSize,
    HmLookup,
    HmMember,
    HmDelete,
    HmInsert,
}

#[derive(Clone, Debug)]
enum Expr {
    IntLit(i64),
    IntegerLit(i128),
    BoolLit(bool),
    StrLit(String),
    OrdLit(std::cmp::Ordering),
    Var(String),
    Not(Box<Expr>),
    Bin(Bin, Box<Expr>, Box<Expr>),
    /// `div`/`mod` with a POSITIVE LITERAL divisor (totality). Works at
    /// Int and Integer; the evaluator dispatches on the operand value.
    DivMod(bool, Box<Expr>, i64),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    ListLit(Vec<Expr>),
    Cons(Box<Expr>, Box<Expr>),
    MkPair(Box<Expr>, Box<Expr>),
    Nothing,
    Just(Box<Expr>),
    Lam(Vec<String>, Box<Expr>),
    App(Box<Expr>, Vec<Expr>),
    Let(String, Box<Expr>, Box<Expr>),
    CaseList {
        scrut: Box<Expr>,
        nil_arm: Box<Expr>,
        hd: String,
        tl: String,
        cons_arm: Box<Expr>,
    },
    CaseMaybe {
        scrut: Box<Expr>,
        nothing_arm: Box<Expr>,
        var: String,
        just_arm: Box<Expr>,
    },
    CasePair {
        scrut: Box<Expr>,
        a: String,
        b: String,
        arm: Box<Expr>,
    },
    Call(P, Vec<Expr>),
    /// A bare reference to a prelude/scaffold function used first-class
    /// (`map`, `const`, `($)`, …) — the arity-widening and adapters path.
    FunRef(P),
    /// `(e :: T)` — pins a type the surrounding text does not determine.
    /// Generated in every DEAD or non-flowing position (`length []`, a
    /// lambda handed to an applier, `seq`'s first operand, a discarded
    /// `const` argument, a case scrutinee, `show`'s and `compare`'s
    /// operands, the key side of hmValues): without the pin such programs
    /// are genuinely ambiguous and both mata-ll and GHC reject them
    /// (found by batch index 1264, the first generator bug the compiler
    /// caught rather than the other way around).
    Annot(Box<Expr>, Ty),
}

fn b(e: Expr) -> Box<Expr> {
    Box::new(e)
}

// ---------------------------------------------------------------------------
// Rendering (fully parenthesized: the printed program parses exactly as the
// AST is shaped, so the reference evaluator and the compiled program agree
// by construction; the paren flood is itself an optimizer stressor)
// ---------------------------------------------------------------------------

fn ord_name(o: std::cmp::Ordering) -> &'static str {
    match o {
        std::cmp::Ordering::Less => "LT",
        std::cmp::Ordering::Equal => "EQ",
        std::cmp::Ordering::Greater => "GT",
    }
}

/// The escaping shared by the .mll string literal and GHC's `show` for the
/// characters the generator (and `show` itself) can produce: printable
/// ASCII plus `\n`/`\t`. The full GHC table (control names, `\&`
/// disambiguation) is byte-oracled by the hand corpus; the fragment stays
/// inside the subset where literal syntax and show output coincide.
fn escape_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn render(e: &Expr) -> String {
    match e {
        Expr::IntLit(n) => {
            if *n < 0 {
                format!("({})", n)
            } else {
                n.to_string()
            }
        }
        Expr::IntegerLit(n) => {
            if *n < 0 {
                format!("({})", n)
            } else {
                n.to_string()
            }
        }
        Expr::BoolLit(v) => if *v { "True" } else { "False" }.into(),
        Expr::StrLit(s) => escape_str(s),
        Expr::OrdLit(o) => ord_name(*o).into(),
        Expr::Var(v) => v.clone(),
        Expr::Not(x) => format!("(not {})", render(x)),
        Expr::Bin(op, l, r) => {
            let o = match op {
                Bin::Add => "+",
                Bin::Sub => "-",
                Bin::Mul => "*",
                Bin::Eq => "==",
                Bin::Ne => "/=",
                Bin::Lt => "<",
                Bin::Le => "<=",
                Bin::And => "&&",
                Bin::Or => "||",
            };
            format!("({} {} {})", render(l), o, render(r))
        }
        Expr::DivMod(is_div, l, d) => format!(
            "({} `{}` {})",
            render(l),
            if *is_div { "div" } else { "mod" },
            d
        ),
        Expr::If(c, t, f) => format!(
            "(if {} then {} else {})",
            render(c),
            render(t),
            render(f)
        ),
        Expr::ListLit(xs) => format!(
            "[{}]",
            xs.iter().map(render).collect::<Vec<_>>().join(", ")
        ),
        Expr::Cons(h, t) => format!("({} : {})", render(h), render(t)),
        Expr::MkPair(a, x) => format!("({}, {})", render(a), render(x)),
        Expr::Nothing => "Nothing".into(),
        Expr::Just(x) => format!("(Just {})", render(x)),
        Expr::Lam(ps, body) => format!("(\\{} -> {})", ps.join(" "), render(body)),
        Expr::App(f, args) => {
            let mut s = format!("({}", render(f));
            for a in args {
                s.push(' ');
                s.push_str(&render(a));
            }
            s.push(')');
            s
        }
        Expr::Let(v, rhs, body) => {
            format!("(let {} = {} in {})", v, render(rhs), render(body))
        }
        Expr::CaseList { scrut, nil_arm, hd, tl, cons_arm } => format!(
            "(case {} of {{ [] -> {}; ({} : {}) -> {} }})",
            render(scrut),
            render(nil_arm),
            hd,
            tl,
            render(cons_arm)
        ),
        Expr::CaseMaybe { scrut, nothing_arm, var, just_arm } => format!(
            "(case {} of {{ Nothing -> {}; Just {} -> {} }})",
            render(scrut),
            render(nothing_arm),
            var,
            render(just_arm)
        ),
        Expr::CasePair { scrut, a, b: bv, arm } => format!(
            "(case {} of {{ ({}, {}) -> {} }})",
            render(scrut),
            a,
            bv,
            render(arm)
        ),
        Expr::Call(p, args) => {
            let rendered: Vec<String> = args.iter().map(render).collect();
            match p {
                P::CompApp => format!("(({} . {}) {})", rendered[0], rendered[1], rendered[2]),
                P::DollarApp => format!("({} $ {})", rendered[0], rendered[1]),
                P::Seq => format!("(seq {} {})", rendered[0], rendered[1]),
                P::Append => format!("({} <> {})", rendered[0], rendered[1]),
                P::HmToList => format!("(hmToList (hmFromList {}))", rendered[0]),
                P::HmKeys => format!("(hmKeys (hmFromList {}))", rendered[0]),
                P::HmValues => format!("(hmValues (hmFromList {}))", rendered[0]),
                P::HmSize => format!("(hmSize (hmFromList {}))", rendered[0]),
                P::HmLookup => {
                    format!("(hmLookup {} (hmFromList {}))", rendered[0], rendered[1])
                }
                P::HmMember => {
                    format!("(hmMember {} (hmFromList {}))", rendered[0], rendered[1])
                }
                P::HmDelete => format!(
                    "(hmToList (hmDelete {} (hmFromList {})))",
                    rendered[0], rendered[1]
                ),
                P::HmInsert => format!(
                    "(hmToList (hmInsert {} {} (hmFromList {})))",
                    rendered[0], rendered[1], rendered[2]
                ),
                _ => {
                    let mut s = format!("({}", p_name(p));
                    for a in &rendered {
                        s.push(' ');
                        s.push_str(a);
                    }
                    s.push(')');
                    s
                }
            }
        }
        Expr::FunRef(p) => p_name(p).into(),
        Expr::Annot(e, ty) => format!("({} :: {})", render(e), ty.render()),
    }
}

fn p_name(p: &P) -> &'static str {
    match p {
        P::Map => "map",
        P::Filter => "filter",
        P::Foldr => "foldr",
        P::Foldl => "foldl",
        P::Take => "take",
        P::Drop => "drop",
        P::Reverse => "reverse",
        P::Length => "length",
        P::Sum | P::SumI => "sum",
        P::Null => "null",
        P::ZipWith => "zipWith",
        P::Fst => "fst",
        P::Snd => "snd",
        P::Head => "head",
        P::Seq => "seq",
        P::Id => "id",
        P::Const => "const",
        P::Flip => "flip",
        P::Twice => "fuzzTwice",
        P::CompApp => ".",
        P::DollarApp => "$",
        P::Sort => "sort",
        P::Elem => "elem",
        P::MaxP => "max",
        P::MinP => "min",
        P::Cmp => "compare",
        P::Maximum => "maximum",
        P::Minimum => "minimum",
        P::ShowP => "show",
        P::Append => "(<>)", // never emitted as a FunRef; Call renders infix
        P::Mconcat => "mconcat",
        P::ToInteger => "toInteger",
        P::NegateI => "negate",
        P::AbsI => "abs",
        P::Even => "even",
        P::Odd => "odd",
        // hm ops are only ever rendered through their Call composites
        P::HmToList => "hmToList",
        P::HmKeys => "hmKeys",
        P::HmValues => "hmValues",
        P::HmSize => "hmSize",
        P::HmLookup => "hmLookup",
        P::HmMember => "hmMember",
        P::HmDelete => "hmDelete",
        P::HmInsert => "hmInsert",
    }
}

// ---------------------------------------------------------------------------
// Reference evaluator (strict; the fragment is total, so strictness cannot
// change any computed value)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Value {
    Int(i64),
    /// Arbitrary-precision Integer, held in an i128. The GENERATOR keeps
    /// every reachable value far inside i128 (atoms ≤ ~1e24, growth per
    /// production level ≤ ×9), so checked arithmetic never trips; if it
    /// ever does, that is a generator-budget bug and the panic says so.
    Integer(i128),
    Bool(bool),
    Str(String),
    Ord3(std::cmp::Ordering),
    List(Vec<Value>),
    Pair(Box<Value>, Box<Value>),
    Maybe(Option<Box<Value>>),
    Closure(Vec<String>, Rc<Expr>, Env),
    /// A prelude/scaffold function value, possibly partially applied.
    Builtin(P, Vec<Value>),
}

type Env = Rc<EnvNode>;

#[derive(Debug)]
enum EnvNode {
    Nil,
    Cons(String, Value, Env),
}

fn env_get(env: &Env, name: &str) -> Value {
    let mut cur = env;
    loop {
        match cur.as_ref() {
            EnvNode::Nil => panic!("backend_fuzz evaluator: unbound '{name}'"),
            EnvNode::Cons(n, v, rest) => {
                if n == name {
                    return v.clone();
                }
                cur = rest;
            }
        }
    }
}

fn env_push(env: &Env, name: &str, v: Value) -> Env {
    Rc::new(EnvNode::Cons(name.to_string(), v, env.clone()))
}

fn as_int(v: &Value) -> i64 {
    match v {
        Value::Int(n) => *n,
        _ => panic!("evaluator: expected Int, got {v:?}"),
    }
}
fn as_bool(v: &Value) -> bool {
    match v {
        Value::Bool(x) => *x,
        _ => panic!("evaluator: expected Bool, got {v:?}"),
    }
}
fn as_str(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        _ => panic!("evaluator: expected String, got {v:?}"),
    }
}
fn as_list(v: &Value) -> Vec<Value> {
    match v {
        Value::List(xs) => xs.clone(),
        _ => panic!("evaluator: expected list, got {v:?}"),
    }
}

/// GHC's derived/structural `compare` over the fragment's values: numeric
/// order for Int/Integer, False < True, byte order for String (ASCII, so
/// Char order and Lua's C-locale order agree), LT < EQ < GT, lexicographic
/// lists and pairs, Nothing < Just.
fn ghc_cmp(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Integer(x), Value::Integer(y)) => x.cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Str(x), Value::Str(y)) => x.as_bytes().cmp(y.as_bytes()),
        (Value::Ord3(x), Value::Ord3(y)) => x.cmp(y),
        (Value::List(xs), Value::List(ys)) => {
            for (x, y) in xs.iter().zip(ys.iter()) {
                let c = ghc_cmp(x, y);
                if c != Equal {
                    return c;
                }
            }
            xs.len().cmp(&ys.len())
        }
        (Value::Pair(a1, b1), Value::Pair(a2, b2)) => {
            let c = ghc_cmp(a1, a2);
            if c != Equal { c } else { ghc_cmp(b1, b2) }
        }
        (Value::Maybe(x), Value::Maybe(y)) => match (x, y) {
            (None, None) => Equal,
            (None, Some(_)) => Less,
            (Some(_), None) => Greater,
            (Some(p), Some(q)) => ghc_cmp(p, q),
        },
        _ => panic!("evaluator: compare across shapes: {a:?} vs {b:?}"),
    }
}

/// Numeric ops dispatch on the value: Int is the host machine integer
/// (wrapping, like the Lua 5.4 emission), Integer is exact (checked —
/// the generator's budget keeps it in range; see Value::Integer).
fn num_bin(op: Bin, l: &Value, r: &Value) -> Value {
    match (l, r) {
        (Value::Int(a), Value::Int(b)) => Value::Int(match op {
            Bin::Add => a.wrapping_add(*b),
            Bin::Sub => a.wrapping_sub(*b),
            Bin::Mul => a.wrapping_mul(*b),
            _ => unreachable!(),
        }),
        (Value::Integer(a), Value::Integer(b)) => Value::Integer(
            match op {
                Bin::Add => a.checked_add(*b),
                Bin::Sub => a.checked_sub(*b),
                Bin::Mul => a.checked_mul(*b),
                _ => unreachable!(),
            }
            .expect("backend_fuzz: Integer magnitude budget exceeded (generator bug)"),
        ),
        _ => panic!("evaluator: mixed numeric operands {l:?} / {r:?}"),
    }
}

/// Apply a function VALUE to one argument (currying).
fn apply(f: Value, arg: Value) -> Value {
    match f {
        Value::Closure(params, body, env) => {
            let (first, rest) = params.split_first().expect("closure with no params");
            let env2 = env_push(&env, first, arg);
            if rest.is_empty() {
                eval(&body, &env2)
            } else {
                Value::Closure(rest.to_vec(), body, env2)
            }
        }
        Value::Builtin(p, mut got) => {
            got.push(arg);
            if got.len() == p_arity(&p) {
                call_builtin(&p, got)
            } else {
                Value::Builtin(p, got)
            }
        }
        other => panic!("evaluator: applied a non-function {other:?}"),
    }
}

/// Value arity of each prelude/scaffold function (Haskell arrow count of the
/// shallowest saturating call the evaluator implements).
fn p_arity(p: &P) -> usize {
    match p {
        P::Reverse
        | P::Length
        | P::Sum
        | P::SumI
        | P::Null
        | P::Fst
        | P::Snd
        | P::Head
        | P::Id
        | P::Sort
        | P::Maximum
        | P::Minimum
        | P::ShowP
        | P::Mconcat
        | P::ToInteger
        | P::NegateI
        | P::AbsI
        | P::Even
        | P::Odd
        | P::HmToList
        | P::HmKeys
        | P::HmValues
        | P::HmSize => 1,
        P::Map
        | P::Filter
        | P::Take
        | P::Drop
        | P::Seq
        | P::Const
        | P::DollarApp
        | P::Elem
        | P::MaxP
        | P::MinP
        | P::Cmp
        | P::Append
        | P::HmLookup
        | P::HmMember
        | P::HmDelete => 2,
        P::Foldr | P::Foldl | P::ZipWith | P::Flip | P::Twice | P::CompApp | P::HmInsert => 3,
    }
}

/// hmFromList semantics: entries fold left-to-right through insert, so a
/// LATER duplicate key wins (probed against the real runtime).
fn build_hm(entries: &Value) -> Vec<(Value, Value)> {
    let mut m: Vec<(Value, Value)> = Vec::new();
    for e in as_list(entries) {
        let (k, v) = match e {
            Value::Pair(k, v) => (*k, *v),
            other => panic!("hm entry is not a pair: {other:?}"),
        };
        if let Some(slot) = m
            .iter_mut()
            .find(|(ek, _)| ghc_cmp(ek, &k) == std::cmp::Ordering::Equal)
        {
            slot.1 = v;
        } else {
            m.push((k, v));
        }
    }
    m
}

/// hm iteration order: A16 structural compare on the keys = Ord order.
fn hm_sorted(mut m: Vec<(Value, Value)>) -> Vec<(Value, Value)> {
    m.sort_by(|(a, _), (b, _)| ghc_cmp(a, b));
    m
}

fn pairs_to_list(m: Vec<(Value, Value)>) -> Value {
    Value::List(m.into_iter().map(|(k, v)| Value::Pair(b2(k), b2(v))).collect())
}

fn call_builtin(p: &P, mut args: Vec<Value>) -> Value {
    match p {
        P::Id => args.pop().unwrap(),
        P::Const => args.swap_remove(0),
        P::Seq => args.pop().unwrap(),
        P::Fst => match args.pop().unwrap() {
            Value::Pair(a, _) => *a,
            v => panic!("fst on {v:?}"),
        },
        P::Snd => match args.pop().unwrap() {
            Value::Pair(_, b2) => *b2,
            v => panic!("snd on {v:?}"),
        },
        P::Head => {
            let xs = as_list(&args[0]);
            xs.into_iter().next().expect("head: generator guarantees nonempty")
        }
        P::Length => Value::Int(as_list(&args[0]).len() as i64),
        P::Null => Value::Bool(as_list(&args[0]).is_empty()),
        P::Reverse => {
            let mut xs = as_list(&args[0]);
            xs.reverse();
            Value::List(xs)
        }
        P::Sum => Value::Int(
            as_list(&args[0]).iter().fold(0i64, |a, v| a.wrapping_add(as_int(v))),
        ),
        P::SumI => Value::Integer(as_list(&args[0]).iter().fold(0i128, |a, v| match v {
            Value::Integer(n) => a
                .checked_add(*n)
                .expect("backend_fuzz: Integer sum budget exceeded (generator bug)"),
            v => panic!("sum at Integer over {v:?}"),
        })),
        P::Take => {
            let n = as_int(&args[0]).max(0) as usize;
            let xs = as_list(&args[1]);
            Value::List(xs.into_iter().take(n).collect())
        }
        P::Drop => {
            let n = as_int(&args[0]).max(0) as usize;
            let xs = as_list(&args[1]);
            Value::List(xs.into_iter().skip(n).collect())
        }
        P::Map => {
            let xs = as_list(&args[1]);
            let f = args.swap_remove(0);
            Value::List(xs.into_iter().map(|x| apply(f.clone(), x)).collect())
        }
        P::Filter => {
            let xs = as_list(&args[1]);
            let f = args.swap_remove(0);
            Value::List(
                xs.into_iter()
                    .filter(|x| as_bool(&apply(f.clone(), x.clone())))
                    .collect(),
            )
        }
        P::Foldr => {
            let xs = as_list(&args[2]);
            let z = args.swap_remove(1);
            let f = args.swap_remove(0);
            xs.into_iter()
                .rev()
                .fold(z, |acc, x| apply(apply(f.clone(), x), acc))
        }
        P::Foldl => {
            let xs = as_list(&args[2]);
            let z = args.swap_remove(1);
            let f = args.swap_remove(0);
            xs.into_iter()
                .fold(z, |acc, x| apply(apply(f.clone(), acc), x))
        }
        P::ZipWith => {
            let ys = as_list(&args[2]);
            let xs = as_list(&args[1]);
            let f = args.swap_remove(0);
            Value::List(
                xs.into_iter()
                    .zip(ys)
                    .map(|(x, y)| apply(apply(f.clone(), x), y))
                    .collect(),
            )
        }
        P::Flip => {
            let a = args.swap_remove(2);
            let bv = args.swap_remove(1);
            let f = args.swap_remove(0);
            apply(apply(f, a), bv)
        }
        P::Twice => {
            let x = args.swap_remove(1);
            let f = args.swap_remove(0);
            apply(f.clone(), apply(f, x))
        }
        P::CompApp => {
            let x = args.swap_remove(2);
            let g = args.swap_remove(1);
            let f = args.swap_remove(0);
            apply(f, apply(g, x))
        }
        P::DollarApp => {
            let x = args.swap_remove(1);
            let f = args.swap_remove(0);
            apply(f, x)
        }
        P::Sort => {
            let mut xs = as_list(&args[0]);
            xs.sort_by(ghc_cmp); // Vec::sort_by is stable, like GHC's sort
            Value::List(xs)
        }
        P::Elem => {
            let xs = as_list(&args[1]);
            let x = &args[0];
            Value::Bool(xs.iter().any(|e| ghc_cmp(e, x) == std::cmp::Ordering::Equal))
        }
        // GHC: max x y = if x <= y then y else x; min x y = if x <= y
        // then x else y (indistinguishable for equal values, but exact).
        P::MaxP => {
            let y = args.pop().unwrap();
            let x = args.pop().unwrap();
            if ghc_cmp(&x, &y) == std::cmp::Ordering::Greater { x } else { y }
        }
        P::MinP => {
            let y = args.pop().unwrap();
            let x = args.pop().unwrap();
            if ghc_cmp(&x, &y) == std::cmp::Ordering::Greater { y } else { x }
        }
        P::Cmp => Value::Ord3(ghc_cmp(&args[0], &args[1])),
        P::Maximum => {
            let xs = as_list(&args[0]);
            xs.into_iter()
                .reduce(|a, x| {
                    if ghc_cmp(&a, &x) == std::cmp::Ordering::Greater { a } else { x }
                })
                .expect("maximum: generator guarantees nonempty")
        }
        P::Minimum => {
            let xs = as_list(&args[0]);
            xs.into_iter()
                .reduce(|a, x| {
                    if ghc_cmp(&x, &a) == std::cmp::Ordering::Less { x } else { a }
                })
                .expect("minimum: generator guarantees nonempty")
        }
        P::ShowP => Value::Str(show_val(&args[0])),
        P::Append => {
            let y = as_str(&args[1]);
            Value::Str(as_str(&args[0]) + &y)
        }
        P::Mconcat => Value::Str(
            as_list(&args[0]).iter().map(as_str).collect::<Vec<_>>().concat(),
        ),
        P::ToInteger => Value::Integer(as_int(&args[0]) as i128),
        P::NegateI => match &args[0] {
            Value::Integer(n) => Value::Integer(-n),
            v => panic!("negate at Integer on {v:?}"),
        },
        P::AbsI => match &args[0] {
            Value::Integer(n) => Value::Integer(n.abs()),
            v => panic!("abs at Integer on {v:?}"),
        },
        P::Even | P::Odd => {
            let even = match &args[0] {
                Value::Int(n) => n % 2 == 0,
                Value::Integer(n) => n % 2 == 0,
                v => panic!("even/odd on {v:?}"),
            };
            Value::Bool(if matches!(p, P::Even) { even } else { !even })
        }
        P::HmToList => pairs_to_list(hm_sorted(build_hm(&args[0]))),
        P::HmKeys => Value::List(
            hm_sorted(build_hm(&args[0])).into_iter().map(|(k, _)| k).collect(),
        ),
        P::HmValues => Value::List(
            hm_sorted(build_hm(&args[0])).into_iter().map(|(_, v)| v).collect(),
        ),
        P::HmSize => Value::Int(build_hm(&args[0]).len() as i64),
        P::HmLookup => {
            let m = build_hm(&args[1]);
            let hit = m
                .into_iter()
                .find(|(k, _)| ghc_cmp(k, &args[0]) == std::cmp::Ordering::Equal);
            Value::Maybe(hit.map(|(_, v)| b2(v)))
        }
        P::HmMember => {
            let m = build_hm(&args[1]);
            Value::Bool(
                m.iter().any(|(k, _)| ghc_cmp(k, &args[0]) == std::cmp::Ordering::Equal),
            )
        }
        P::HmDelete => {
            let mut m = build_hm(&args[1]);
            m.retain(|(k, _)| ghc_cmp(k, &args[0]) != std::cmp::Ordering::Equal);
            pairs_to_list(hm_sorted(m))
        }
        P::HmInsert => {
            let entries = args.pop().unwrap();
            let v = args.pop().unwrap();
            let k = args.pop().unwrap();
            let mut m = build_hm(&entries);
            if let Some(slot) = m
                .iter_mut()
                .find(|(ek, _)| ghc_cmp(ek, &k) == std::cmp::Ordering::Equal)
            {
                slot.1 = v;
            } else {
                m.push((k, v));
            }
            pairs_to_list(hm_sorted(m))
        }
    }
}

fn eval(e: &Expr, env: &Env) -> Value {
    match e {
        Expr::IntLit(n) => Value::Int(*n),
        Expr::IntegerLit(n) => Value::Integer(*n),
        Expr::BoolLit(v) => Value::Bool(*v),
        Expr::StrLit(s) => Value::Str(s.clone()),
        Expr::OrdLit(o) => Value::Ord3(*o),
        Expr::Var(v) => env_get(env, v),
        Expr::Not(x) => Value::Bool(!as_bool(&eval(x, env))),
        Expr::Bin(op, l, r) => {
            let lv = eval(l, env);
            let rv = eval(r, env);
            match op {
                Bin::Add | Bin::Sub | Bin::Mul => num_bin(*op, &lv, &rv),
                Bin::Eq => Value::Bool(ghc_cmp(&lv, &rv) == std::cmp::Ordering::Equal),
                Bin::Ne => Value::Bool(ghc_cmp(&lv, &rv) != std::cmp::Ordering::Equal),
                Bin::Lt => Value::Bool(ghc_cmp(&lv, &rv) == std::cmp::Ordering::Less),
                Bin::Le => Value::Bool(ghc_cmp(&lv, &rv) != std::cmp::Ordering::Greater),
                Bin::And => Value::Bool(as_bool(&lv) && as_bool(&rv)),
                Bin::Or => Value::Bool(as_bool(&lv) || as_bool(&rv)),
            }
        }
        Expr::DivMod(is_div, l, d) => match eval(l, env) {
            // Haskell floor division/modulo. The divisor is a positive
            // literal by construction, where div_euclid == floor division
            // and rem_euclid == Haskell mod.
            Value::Int(n) => {
                Value::Int(if *is_div { n.div_euclid(*d) } else { n.rem_euclid(*d) })
            }
            Value::Integer(n) => {
                let d = *d as i128;
                Value::Integer(if *is_div { n.div_euclid(d) } else { n.rem_euclid(d) })
            }
            v => panic!("div/mod on {v:?}"),
        },
        Expr::If(c, t, f) => {
            if as_bool(&eval(c, env)) {
                eval(t, env)
            } else {
                eval(f, env)
            }
        }
        Expr::ListLit(xs) => Value::List(xs.iter().map(|x| eval(x, env)).collect()),
        Expr::Cons(h, t) => {
            let hv = eval(h, env);
            let mut tv = as_list(&eval(t, env));
            tv.insert(0, hv);
            Value::List(tv)
        }
        Expr::MkPair(a, x) => Value::Pair(b2(eval(a, env)), b2(eval(x, env))),
        Expr::Nothing => Value::Maybe(None),
        Expr::Just(x) => Value::Maybe(Some(b2(eval(x, env)))),
        Expr::Lam(ps, body) => {
            Value::Closure(ps.clone(), Rc::new((**body).clone()), env.clone())
        }
        Expr::App(f, args) => {
            let mut v = eval(f, env);
            for a in args {
                v = apply(v, eval(a, env));
            }
            v
        }
        Expr::Let(name, rhs, body) => {
            let rv = eval(rhs, env);
            eval(body, &env_push(env, name, rv))
        }
        Expr::CaseList { scrut, nil_arm, hd, tl, cons_arm } => {
            let xs = as_list(&eval(scrut, env));
            match xs.split_first() {
                None => eval(nil_arm, env),
                Some((h, t)) => {
                    let env2 = env_push(env, hd, h.clone());
                    let env2 = env_push(&env2, tl, Value::List(t.to_vec()));
                    eval(cons_arm, &env2)
                }
            }
        }
        Expr::CaseMaybe { scrut, nothing_arm, var, just_arm } => {
            match eval(scrut, env) {
                Value::Maybe(None) => eval(nothing_arm, env),
                Value::Maybe(Some(v)) => eval(just_arm, &env_push(env, var, *v)),
                v => panic!("case-maybe on {v:?}"),
            }
        }
        Expr::CasePair { scrut, a, b: bv, arm } => match eval(scrut, env) {
            Value::Pair(x, y) => {
                let env2 = env_push(env, a, *x);
                let env2 = env_push(&env2, bv, *y);
                eval(arm, &env2)
            }
            v => panic!("case-pair on {v:?}"),
        },
        Expr::Call(p, args) => {
            let vals: Vec<Value> = args.iter().map(|a| eval(a, env)).collect();
            call_builtin(p, vals)
        }
        Expr::FunRef(p) => Value::Builtin(p.clone(), Vec::new()),
        Expr::Annot(e, _) => eval(e, env),
    }
}

fn b2(v: Value) -> Box<Value> {
    Box::new(v)
}

/// GHC `show` (the derived instances' rendering, which mata-ll's show is
/// byte-oracled against). `prec11` renders a constructor ARGUMENT: negative
/// numbers and saturated `Just` get parens, everything else is atomic.
fn show_val(v: &Value) -> String {
    match v {
        Value::Int(n) => n.to_string(),
        Value::Integer(n) => n.to_string(),
        Value::Bool(x) => if *x { "True" } else { "False" }.into(),
        Value::Str(s) => escape_str(s),
        Value::Ord3(o) => ord_name(*o).into(),
        Value::List(xs) => format!(
            "[{}]",
            xs.iter().map(show_val).collect::<Vec<_>>().join(",")
        ),
        Value::Pair(a, x) => format!("({},{})", show_val(a), show_val(x)),
        Value::Maybe(None) => "Nothing".into(),
        Value::Maybe(Some(x)) => format!("Just {}", show_prec11(x)),
        v => panic!("show of a function value: {v:?}"),
    }
}

fn show_prec11(v: &Value) -> String {
    match v {
        Value::Int(n) if *n < 0 => format!("({})", n),
        Value::Integer(n) if *n < 0 => format!("({})", n),
        Value::Maybe(Some(_)) => format!("({})", show_val(v)),
        _ => show_val(v),
    }
}

// ---------------------------------------------------------------------------
// Type-directed generation
// ---------------------------------------------------------------------------

/// A generated top-level helper's callable signature.
#[derive(Clone, Debug)]
struct HelperSig {
    name: String,
    args: Vec<Ty>,
    ret: Ty,
}

impl HelperSig {
    fn full_ty(&self) -> Ty {
        self.args
            .iter()
            .rev()
            .fold(self.ret.clone(), |acc, t| Ty::fun(t.clone(), acc))
    }
}

struct Gen {
    r: Rng,
    /// In-scope variables with their types (lexical; pushed and popped).
    scope: Vec<(String, Ty)>,
    fresh: usize,
    /// Completed generated helpers, callable from later helpers and main.
    helpers: Vec<HelperSig>,
    /// Inside the poly-where helper variant: the name of its `id`-shaped
    /// where-bind, usable at ANY `a -> a` instantiation (the A19
    /// where-generalization surface).
    poly_iden: Option<String>,
}

impl Gen {
    fn fresh_var(&mut self) -> String {
        self.fresh += 1;
        format!("v{}", self.fresh)
    }

    /// `expr`, wrapped in a `(… :: T)` pin — for DEAD and non-flowing
    /// positions, where nothing else in the text determines the type (see
    /// `Expr::Annot`).
    fn pinned(&mut self, ty: &Ty, depth: usize) -> Expr {
        let e = self.expr(ty, depth);
        Expr::Annot(b(e), ty.clone())
    }

    /// A pinned `[(k, v)]` entries expression for a HashMap composite.
    fn hm_entries(&mut self, k: &Ty, v: &Ty, depth: usize) -> Expr {
        self.pinned(&Ty::list(Ty::pair(k.clone(), v.clone())), depth)
    }

    /// An expression of type `ty`, at most `depth` productions deep.
    fn expr(&mut self, ty: &Ty, depth: usize) -> Expr {
        // A scoped variable of the right type, sometimes.
        if self.r.chance(1, 4)
            && let Some(v) = self.scoped_var(ty) {
                return v;
            }
        if depth == 0 {
            return self.atom(ty);
        }
        // Type-agnostic wrappers, occasionally.
        match self.r.below(24) {
            0 => {
                let c = self.expr(&Ty::Bool, depth - 1);
                let t = self.expr(ty, depth - 1);
                let f = self.expr(ty, depth - 1);
                return Expr::If(b(c), b(t), b(f));
            }
            1 => {
                let vt = gen_ty(&mut self.r, 1, false);
                let name = self.fresh_var();
                let rhs = self.pinned(&vt, depth - 1);
                self.scope.push((name.clone(), vt));
                let body = self.expr(ty, depth - 1);
                self.scope.pop();
                return Expr::Let(name, b(rhs), b(body));
            }
            2 => {
                // Application of a generated function to a generated
                // argument — including through ($) and (.).
                let at = gen_ty(&mut self.r, 1, false);
                let f = self.pinned(&Ty::fun(at.clone(), ty.clone()), depth - 1);
                let x = self.expr(&at, depth - 1);
                return match self.r.below(3) {
                    0 => Expr::Call(P::DollarApp, vec![f, x]),
                    _ => Expr::App(b(f), vec![x]),
                };
            }
            3 => {
                // (f . g) x — the composition emission with fresh types.
                let mid = gen_ty(&mut self.r, 1, false);
                let at = gen_ty(&mut self.r, 1, false);
                let f = self.pinned(&Ty::fun(mid.clone(), ty.clone()), depth - 1);
                let g = self.pinned(&Ty::fun(at.clone(), mid), depth - 1);
                let x = self.expr(&at, depth - 1);
                return Expr::Call(P::CompApp, vec![f, g, x]);
            }
            4 => {
                let st = gen_ty(&mut self.r, 1, false);
                let scrut = self.pinned(&Ty::Maybe(Box::new(st.clone())), depth - 1);
                let nothing_arm = self.expr(ty, depth - 1);
                let var = self.fresh_var();
                self.scope.push((var.clone(), st));
                let just_arm = self.expr(ty, depth - 1);
                self.scope.pop();
                return Expr::CaseMaybe {
                    scrut: b(scrut),
                    nothing_arm: b(nothing_arm),
                    var,
                    just_arm: b(just_arm),
                };
            }
            5 => {
                let et = gen_ty(&mut self.r, 1, false);
                let scrut = self.pinned(&Ty::list(et.clone()), depth - 1);
                let nil_arm = self.expr(ty, depth - 1);
                let hd = self.fresh_var();
                let tl = self.fresh_var();
                self.scope.push((hd.clone(), et.clone()));
                self.scope.push((tl.clone(), Ty::list(et)));
                let cons_arm = self.expr(ty, depth - 1);
                self.scope.pop();
                self.scope.pop();
                return Expr::CaseList {
                    scrut: b(scrut),
                    nil_arm: b(nil_arm),
                    hd,
                    tl,
                    cons_arm: b(cons_arm),
                };
            }
            6 => {
                let at = gen_ty(&mut self.r, 1, false);
                let bt = gen_ty(&mut self.r, 1, false);
                let scrut = self.pinned(&Ty::pair(at.clone(), bt.clone()), depth - 1);
                let a = self.fresh_var();
                let bv = self.fresh_var();
                self.scope.push((a.clone(), at));
                self.scope.push((bv.clone(), bt));
                let arm = self.expr(ty, depth - 1);
                self.scope.pop();
                self.scope.pop();
                return Expr::CasePair { scrut: b(scrut), a, b: bv, arm: b(arm) };
            }
            7 => {
                // seq: force an arbitrary value, return the payload.
                let at = gen_ty(&mut self.r, 1, false);
                let x = self.pinned(&at, depth - 1);
                let y = self.expr(ty, depth - 1);
                return Expr::Call(P::Seq, vec![x, y]);
            }
            8 | 9 => {
                // The polymorphic scaffold at this type: id / const /
                // flip const / fuzzTwice — arity-widening shapes when `ty`
                // is itself a function type.
                return match self.r.below(4) {
                    0 => Expr::Call(P::Id, vec![self.expr(ty, depth - 1)]),
                    1 => {
                        let jt = gen_ty(&mut self.r, 1, false);
                        let junk = self.pinned(&jt, depth - 1);
                        Expr::Call(P::Const, vec![self.expr(ty, depth - 1), junk])
                    }
                    2 => {
                        let jt = gen_ty(&mut self.r, 1, false);
                        let junk = self.pinned(&jt, depth - 1);
                        // flip const junk keeps = keeps
                        Expr::Call(
                            P::Flip,
                            vec![Expr::FunRef(P::Const), junk, self.expr(ty, depth - 1)],
                        )
                    }
                    _ => {
                        let f = self.pinned(&Ty::fun(ty.clone(), ty.clone()), depth - 1);
                        Expr::Call(P::Twice, vec![f, self.expr(ty, depth - 1)])
                    }
                };
            }
            10 | 11 => {
                // A saturated call of a generated helper whose return type
                // fits (over-applying an eta-padded definition when the
                // helper has fewer patterns than arrows).
                let cands: Vec<HelperSig> = self
                    .helpers
                    .iter()
                    .filter(|h| h.ret == *ty)
                    .cloned()
                    .collect();
                if !cands.is_empty() {
                    let h = &cands[self.r.below(cands.len())];
                    let args: Vec<Expr> =
                        h.args.iter().map(|t| self.expr(t, depth - 1)).collect();
                    return Expr::App(b(Expr::Var(h.name.clone())), args);
                }
            }
            12 => {
                // Ord family at this exact type — the A16 structural
                // compare and the OrdFromCmp max/min derivations.
                if ty.showable() {
                    return match self.r.below(4) {
                        0 => Expr::Call(
                            P::MaxP,
                            vec![self.pinned(ty, depth - 1), self.expr(ty, depth - 1)],
                        ),
                        1 => Expr::Call(
                            P::MinP,
                            vec![self.pinned(ty, depth - 1), self.expr(ty, depth - 1)],
                        ),
                        2 => Expr::Call(
                            P::Maximum,
                            vec![Expr::Cons(
                                b(self.expr(ty, depth - 1)),
                                b(self.expr(&Ty::list(ty.clone()), depth - 1)),
                            )],
                        ),
                        _ => Expr::Call(
                            P::Minimum,
                            vec![Expr::Cons(
                                b(self.expr(ty, depth - 1)),
                                b(self.expr(&Ty::list(ty.clone()), depth - 1)),
                            )],
                        ),
                    };
                }
            }
            _ => {}
        }
        // Type-directed productions.
        match ty {
            Ty::Int => self.int_expr(depth),
            Ty::Integer => self.integer_expr(depth),
            Ty::Bool => self.bool_expr(depth),
            Ty::Str => self.str_expr(depth),
            Ty::Ordering => self.ordering_expr(depth),
            Ty::List(t) => self.list_expr(t, depth),
            Ty::Pair(a, bt) => Expr::MkPair(
                b(self.expr(a, depth - 1)),
                b(self.expr(bt, depth - 1)),
            ),
            Ty::Maybe(t) => self.maybe_expr(t, depth),
            Ty::Fun(a, bt) => self.fun_expr(a, bt, depth),
        }
    }

    fn scoped_var(&mut self, ty: &Ty) -> Option<Expr> {
        let hits: Vec<&(String, Ty)> =
            self.scope.iter().filter(|(_, t)| t == ty).collect();
        if hits.is_empty() {
            return None;
        }
        let i = self.r.below(hits.len());
        Some(Expr::Var(hits[i].0.clone()))
    }

    /// A depth-0 value of `ty`.
    fn atom(&mut self, ty: &Ty) -> Expr {
        match ty {
            Ty::Int => Expr::IntLit(self.r.below(21) as i64 - 5),
            Ty::Integer => self.integer_atom(),
            Ty::Bool => Expr::BoolLit(self.r.chance(1, 2)),
            Ty::Str => self.str_atom(),
            Ty::Ordering => Expr::OrdLit(match self.r.below(3) {
                0 => std::cmp::Ordering::Less,
                1 => std::cmp::Ordering::Equal,
                _ => std::cmp::Ordering::Greater,
            }),
            Ty::List(t) => {
                let n = self.r.below(4);
                let xs = (0..n).map(|_| self.atom(t)).collect();
                Expr::ListLit(xs)
            }
            Ty::Pair(a, bt) => Expr::MkPair(b(self.atom(a)), b(self.atom(bt))),
            Ty::Maybe(t) => {
                if self.r.chance(1, 3) {
                    Expr::Nothing
                } else {
                    Expr::Just(b(self.atom(t)))
                }
            }
            Ty::Fun(a, bt) => {
                // Prefer a first-class prelude reference when one fits —
                // `id`, `($)` at matching instantiations — else a lambda.
                if **a == **bt && self.r.chance(1, 3) {
                    return Expr::FunRef(P::Id);
                }
                let p = self.fresh_var();
                self.scope.push((p.clone(), (**a).clone()));
                let body = self.atom(bt);
                self.scope.pop();
                Expr::Lam(vec![p], b(body))
            }
        }
    }

    fn integer_atom(&mut self) -> Expr {
        if self.r.chance(1, 3) {
            // A 20-24 digit literal: multi-limb by construction, far past
            // both float precision (2^53) and Int range (2^63), and small
            // enough that the whole-program growth budget (≤ ~×9 per
            // production level) keeps every value inside i128.
            let hi = (self.r.next() % 90_000 + 10_000) as i128; // 5 digits
            let lo = (self.r.next() % 10_000_000_000_000_000_000) as i128; // ≤ 19
            let mag = hi * 10_000_000_000_000_000_000i128 + lo;
            Expr::IntegerLit(if self.r.chance(1, 2) { -mag } else { mag })
        } else {
            Expr::IntegerLit(self.r.below(21) as i128 - 5)
        }
    }

    fn str_atom(&mut self) -> Expr {
        const COMMON: &[u8] = b"abcxyz012 ,;";
        let n = self.r.below(7);
        let mut s = String::new();
        for _ in 0..n {
            if self.r.chance(1, 8) {
                // The characters whose literal syntax and show output are
                // escape sequences.
                s.push(['"', '\\', '\n', '\t'][self.r.below(4)]);
            } else {
                s.push(COMMON[self.r.below(COMMON.len())] as char);
            }
        }
        Expr::StrLit(s)
    }

    fn int_expr(&mut self, depth: usize) -> Expr {
        match self.r.below(12) {
            0 | 1 | 2 => {
                let op = [Bin::Add, Bin::Sub, Bin::Mul][self.r.below(3)];
                Expr::Bin(
                    op,
                    b(self.expr(&Ty::Int, depth - 1)),
                    b(self.expr(&Ty::Int, depth - 1)),
                )
            }
            3 => Expr::DivMod(
                self.r.chance(1, 2),
                b(self.expr(&Ty::Int, depth - 1)),
                1 + self.r.below(9) as i64,
            ),
            4 => {
                let t = gen_ty(&mut self.r, 1, false);
                Expr::Call(P::Length, vec![self.pinned(&Ty::list(t), depth - 1)])
            }
            5 => Expr::Call(P::Sum, vec![self.expr(&Ty::list(Ty::Int), depth - 1)]),
            6 => {
                // head with syntactic-cons evidence.
                let h = self.expr(&Ty::Int, depth - 1);
                let t = self.expr(&Ty::list(Ty::Int), depth - 1);
                Expr::Call(P::Head, vec![Expr::Cons(b(h), b(t))])
            }
            7 => {
                // foldr/foldl over Int.
                let p = if self.r.chance(1, 2) { P::Foldr } else { P::Foldl };
                let f = self.pinned(
                    &Ty::fun(Ty::Int, Ty::fun(Ty::Int, Ty::Int)),
                    depth - 1,
                );
                let z = self.expr(&Ty::Int, depth - 1);
                let xs = self.expr(&Ty::list(Ty::Int), depth - 1);
                Expr::Call(p, vec![f, z, xs])
            }
            8 => {
                let t = gen_ty(&mut self.r, 1, false);
                if self.r.chance(1, 2) {
                    let pt = Ty::pair(Ty::Int, t);
                    Expr::Call(P::Fst, vec![self.pinned(&pt, depth - 1)])
                } else {
                    let pt = Ty::pair(t, Ty::Int);
                    Expr::Call(P::Snd, vec![self.pinned(&pt, depth - 1)])
                }
            }
            9 => {
                // hmSize: neither key nor value flows anywhere — pinned.
                let k = gen_key_ty(&mut self.r, 1);
                let v = gen_ty(&mut self.r, 1, false);
                let entries = self.hm_entries(&k, &v, depth - 1);
                Expr::Call(P::HmSize, vec![entries])
            }
            _ => self.atom(&Ty::Int),
        }
    }

    fn integer_expr(&mut self, depth: usize) -> Expr {
        match self.r.below(10) {
            0 | 1 => {
                let op = if self.r.chance(1, 2) { Bin::Add } else { Bin::Sub };
                Expr::Bin(
                    op,
                    b(self.expr(&Ty::Integer, depth - 1)),
                    b(self.expr(&Ty::Integer, depth - 1)),
                )
            }
            2 => {
                // Multiplication by a small LITERAL only: exercises the
                // limb-carry path on 20+-digit operands while bounding
                // per-level growth at ×9 (the i128 headroom budget).
                let lit = self.r.below(19) as i128 - 9;
                Expr::Bin(
                    Bin::Mul,
                    b(self.expr(&Ty::Integer, depth - 1)),
                    b(Expr::IntegerLit(lit)),
                )
            }
            3 => Expr::DivMod(
                self.r.chance(1, 2),
                b(self.expr(&Ty::Integer, depth - 1)),
                1 + self.r.below(9) as i64,
            ),
            4 => Expr::Call(P::NegateI, vec![self.expr(&Ty::Integer, depth - 1)]),
            5 => Expr::Call(P::AbsI, vec![self.expr(&Ty::Integer, depth - 1)]),
            6 => Expr::Call(P::ToInteger, vec![self.expr(&Ty::Int, depth - 1)]),
            7 => Expr::Call(
                P::SumI,
                vec![self.expr(&Ty::list(Ty::Integer), depth - 1)],
            ),
            _ => self.atom(&Ty::Integer),
        }
    }

    fn bool_expr(&mut self, depth: usize) -> Expr {
        match self.r.below(10) {
            0 | 1 => {
                // Eq/Ord operators at ANY fun-free type — the scalar paths
                // for Int/Integer/Bool/String/Ordering, the A16 structural
                // walkers for containers. The lhs pins the operand type
                // (a Bool result determines nothing).
                let t = gen_ty(&mut self.r, 2, false);
                let op = [Bin::Eq, Bin::Ne, Bin::Lt, Bin::Le][self.r.below(4)];
                Expr::Bin(
                    op,
                    b(self.pinned(&t, depth - 1)),
                    b(self.expr(&t, depth - 1)),
                )
            }
            2 => {
                let op = if self.r.chance(1, 2) { Bin::And } else { Bin::Or };
                Expr::Bin(
                    op,
                    b(self.expr(&Ty::Bool, depth - 1)),
                    b(self.expr(&Ty::Bool, depth - 1)),
                )
            }
            3 => Expr::Not(b(self.expr(&Ty::Bool, depth - 1))),
            4 => {
                let t = gen_ty(&mut self.r, 1, false);
                Expr::Call(P::Null, vec![self.pinned(&Ty::list(t), depth - 1)])
            }
            5 => {
                // elem: structural Eq through the Foldable generic.
                let t = gen_ty(&mut self.r, 1, false);
                let x = self.pinned(&t, depth - 1);
                let xs = self.expr(&Ty::list(t), depth - 1);
                Expr::Call(P::Elem, vec![x, xs])
            }
            6 => {
                let k = gen_key_ty(&mut self.r, 1);
                let v = gen_ty(&mut self.r, 1, false);
                let key = self.pinned(&k, depth - 1);
                let entries = self.hm_entries(&k, &v, depth - 1);
                Expr::Call(P::HmMember, vec![key, entries])
            }
            7 => {
                let p = if self.r.chance(1, 2) { P::Even } else { P::Odd };
                let t = if self.r.chance(1, 2) { Ty::Int } else { Ty::Integer };
                Expr::Call(p, vec![self.pinned(&t, depth - 1)])
            }
            _ => self.atom(&Ty::Bool),
        }
    }

    fn str_expr(&mut self, depth: usize) -> Expr {
        match self.r.below(8) {
            0 | 1 => Expr::Call(
                P::Append,
                vec![
                    self.expr(&Ty::Str, depth - 1),
                    self.expr(&Ty::Str, depth - 1),
                ],
            ),
            2 | 3 => {
                // show at a random fun-free type (pinned: show's argument
                // type flows nowhere).
                let t = gen_ty(&mut self.r, 2, false);
                Expr::Call(P::ShowP, vec![self.pinned(&t, depth - 1)])
            }
            4 => Expr::Call(
                P::Mconcat,
                vec![self.expr(&Ty::list(Ty::Str), depth - 1)],
            ),
            _ => self.atom(&Ty::Str),
        }
    }

    fn ordering_expr(&mut self, depth: usize) -> Expr {
        if self.r.chance(2, 3) {
            // compare at a random fun-free type (pinned lhs — an Ordering
            // result determines nothing about the operands).
            let t = gen_ty(&mut self.r, 2, false);
            let lhs = self.pinned(&t, depth - 1);
            let rhs = self.expr(&t, depth - 1);
            Expr::Call(P::Cmp, vec![lhs, rhs])
        } else {
            self.atom(&Ty::Ordering)
        }
    }

    fn list_expr(&mut self, elem: &Ty, depth: usize) -> Expr {
        match self.r.below(14) {
            0 | 1 => {
                // map with a random source element type.
                let src = gen_ty(&mut self.r, 1, false);
                let f = self.pinned(&Ty::fun(src.clone(), elem.clone()), depth - 1);
                let xs = self.expr(&Ty::list(src), depth - 1);
                Expr::Call(P::Map, vec![f, xs])
            }
            2 => {
                let f = self.pinned(&Ty::fun(elem.clone(), Ty::Bool), depth - 1);
                let xs = self.expr(&Ty::list(elem.clone()), depth - 1);
                Expr::Call(P::Filter, vec![f, xs])
            }
            3 => {
                let p = if self.r.chance(1, 2) { P::Take } else { P::Drop };
                let n = self.expr(&Ty::Int, depth - 1);
                let xs = self.expr(&Ty::list(elem.clone()), depth - 1);
                Expr::Call(p, vec![n, xs])
            }
            4 => Expr::Call(
                P::Reverse,
                vec![self.expr(&Ty::list(elem.clone()), depth - 1)],
            ),
            5 => {
                let a = gen_ty(&mut self.r, 1, false);
                let bt = gen_ty(&mut self.r, 1, false);
                let f = self.pinned(
                    &Ty::fun(a.clone(), Ty::fun(bt.clone(), elem.clone())),
                    depth - 1,
                );
                let xs = self.expr(&Ty::list(a), depth - 1);
                let ys = self.expr(&Ty::list(bt), depth - 1);
                Expr::Call(P::ZipWith, vec![f, xs, ys])
            }
            6 => Expr::Cons(
                b(self.expr(elem, depth - 1)),
                b(self.expr(&Ty::list(elem.clone()), depth - 1)),
            ),
            7 => {
                // sort at the element type (every fun-free type is
                // Ord-comparable; structural for containers).
                Expr::Call(
                    P::Sort,
                    vec![self.expr(&Ty::list(elem.clone()), depth - 1)],
                )
            }
            8 | 9 => {
                // HashMap composites, when the element type fits one:
                // entry pairs iterate/mutate, keyable elements come back
                // out of hmKeys, anything comes back out of hmValues.
                if let Ty::Pair(k, v) = elem
                    && k.keyable() {
                        let entries = self.hm_entries(k, v, depth - 1);
                        return match self.r.below(3) {
                            0 => Expr::Call(P::HmToList, vec![entries]),
                            1 => {
                                let key = self.pinned(&k.clone(), depth - 1);
                                Expr::Call(P::HmDelete, vec![key, entries])
                            }
                            _ => {
                                let key = self.pinned(&k.clone(), depth - 1);
                                let val = self.expr(v, depth - 1);
                                Expr::Call(P::HmInsert, vec![key, val, entries])
                            }
                        };
                    }
                if elem.keyable() && self.r.chance(1, 2) {
                    // hmKeys: the VALUE type flows nowhere — pinned via
                    // the entries annotation.
                    let v = gen_ty(&mut self.r, 1, false);
                    let entries = self.hm_entries(elem, &v, depth - 1);
                    return Expr::Call(P::HmKeys, vec![entries]);
                }
                // hmValues: the KEY type flows nowhere — pinned likewise.
                let k = gen_key_ty(&mut self.r, 1);
                let entries = self.hm_entries(&k, elem, depth - 1);
                Expr::Call(P::HmValues, vec![entries])
            }
            _ => {
                let n = self.r.below(4);
                let xs = (0..n).map(|_| self.expr(elem, depth - 1)).collect();
                Expr::ListLit(xs)
            }
        }
    }

    fn maybe_expr(&mut self, t: &Ty, depth: usize) -> Expr {
        match self.r.below(8) {
            0 => {
                // hmLookup: the key type flows nowhere — both the key and
                // the entries are pinned.
                let k = gen_key_ty(&mut self.r, 1);
                let key = self.pinned(&k, depth - 1);
                let entries = self.hm_entries(&k, t, depth - 1);
                Expr::Call(P::HmLookup, vec![key, entries])
            }
            1 | 2 => Expr::Nothing,
            _ => Expr::Just(b(self.expr(t, depth - 1))),
        }
    }

    fn fun_expr(&mut self, a: &Ty, ret: &Ty, depth: usize) -> Expr {
        // The poly-where identity, at any a -> a instantiation (A19).
        if let Some(id) = self.poly_iden.clone()
            && a == ret
            && self.r.chance(1, 3)
        {
            return Expr::Var(id);
        }
        // A generated helper used first-class at its exact full type.
        if self.r.chance(1, 4) {
            let want = Ty::fun(a.clone(), ret.clone());
            let cands: Vec<String> = self
                .helpers
                .iter()
                .filter(|h| h.full_ty() == want)
                .map(|h| h.name.clone())
                .collect();
            if !cands.is_empty() {
                return Expr::Var(cands[self.r.below(cands.len())].clone());
            }
        }
        // First-class CONSTRAINED prelude references at matching
        // instantiations — show/compare/max/min/sort/elem force the
        // widened-ref and specialization paths for class methods.
        if self.r.chance(1, 4) {
            if *ret == Ty::Str && a.showable() && self.r.chance(1, 2) {
                return Expr::FunRef(P::ShowP);
            }
            if let Ty::Fun(a2, r2) = ret
                && **a2 == *a
                && a.showable()
            {
                if **r2 == *a {
                    return Expr::FunRef(if self.r.chance(1, 2) {
                        P::MaxP
                    } else {
                        P::MinP
                    });
                }
                if **r2 == Ty::Ordering {
                    return Expr::FunRef(P::Cmp);
                }
            }
            if let Ty::List(t) = a
                && ret == a
                && t.showable()
            {
                return Expr::FunRef(P::Sort);
            }
            if let Ty::List(t) = a
                && **t != Ty::Str // avoid `elem x "..."`-shaped confusion; String is not a list
                && *ret == Ty::Bool
                && t.showable()
            {
                // Partial application of elem — a one-arg section of a
                // two-arrow constrained builtin.
                let x = self.pinned(&t.clone(), depth.saturating_sub(1));
                return Expr::App(b(Expr::FunRef(P::Elem)), vec![x]);
            }
        }
        match self.r.below(8) {
            0 if a == ret => Expr::FunRef(P::Id),
            1 => {
                // const applied once: a -> ret from a ret-value (the
                // partial-application emission; widening when ret is a
                // function type). App (not Call) so the evaluator curries.
                Expr::App(
                    b(Expr::FunRef(P::Const)),
                    vec![self.expr(ret, depth - 1)],
                )
            }
            2 => {
                // (f . g) as a VALUE via composition of generated parts,
                // rendered through a lambda to keep the AST/eval in step.
                let p = self.fresh_var();
                self.scope.push((p.clone(), a.clone()));
                let body = self.expr(ret, depth.saturating_sub(1));
                self.scope.pop();
                Expr::Lam(vec![p], b(body))
            }
            _ => {
                // Multi-parameter lambda when the result is again a
                // function (the flattened N-ary convention path).
                if let Ty::Fun(a2, r2) = ret
                    && self.r.chance(1, 2) {
                        let p1 = self.fresh_var();
                        let p2 = self.fresh_var();
                        self.scope.push((p1.clone(), a.clone()));
                        self.scope.push((p2.clone(), (**a2).clone()));
                        let body = self.expr(r2, depth.saturating_sub(1));
                        self.scope.pop();
                        self.scope.pop();
                        return Expr::Lam(vec![p1, p2], b(body));
                    }
                let p = self.fresh_var();
                self.scope.push((p.clone(), a.clone()));
                let body = self.expr(ret, depth.saturating_sub(1));
                self.scope.pop();
                Expr::Lam(vec![p], b(body))
            }
        }
    }

    // -----------------------------------------------------------------------
    // Generated top-level helpers
    // -----------------------------------------------------------------------

    /// One generated helper: guard chain + where-binds, an eta-padded
    /// definition (fewer patterns than arrows), or the poly-where shape (a
    /// where-bound identity generalized and instantiated at several types —
    /// A19). Returns (source text, desugared lambda for the evaluator) and
    /// registers the signature for later call sites. Guards desugar to an
    /// if-chain and where-binds to lets — exact semantics in a total
    /// fragment, so the reference and the source agree by construction.
    fn gen_helper(&mut self, index: usize) -> (String, Expr) {
        let scope_base = self.scope.len();
        let name = format!("fuzzHelp{index}");
        let out = match self.r.below(3) {
            0 => self.helper_guarded(&name),
            1 => self.helper_eta(&name),
            _ => self.helper_poly_where(&name),
        };
        debug_assert!(
            self.scope.len() == scope_base,
            "helper generation must unwind its scope"
        );
        out
    }

    fn helper_guarded(&mut self, name: &str) -> (String, Expr) {
        let n_args = 1 + self.r.below(3);
        let args: Vec<Ty> = (0..n_args)
            .map(|_| {
                let fun_ok = self.r.chance(1, 4);
                gen_ty(&mut self.r, 1, fun_ok)
            })
            .collect();
        let ret = gen_ty(&mut self.r, 2, false);
        let params: Vec<String> = (0..n_args).map(|_| self.fresh_var()).collect();
        for (p, t) in params.iter().zip(&args) {
            self.scope.push((p.clone(), t.clone()));
        }
        let mut wheres: Vec<(String, Expr)> = Vec::new();
        for _ in 0..self.r.below(3) {
            let wt = gen_ty(&mut self.r, 1, false);
            let rhs = self.pinned(&wt, 2);
            let wname = self.fresh_var();
            self.scope.push((wname.clone(), wt));
            wheres.push((wname, rhs));
        }
        let mut guards: Vec<(Expr, Expr)> = Vec::new();
        for _ in 0..(1 + self.r.below(2)) {
            let g = self.bool_expr(2);
            let arm = self.expr(&ret, 2);
            guards.push((g, arm));
        }
        let last = self.expr(&ret, 2);
        for _ in 0..(n_args + wheres.len()) {
            self.scope.pop();
        }

        let sig_ty: Vec<String> =
            args.iter().map(Ty::render).chain([ret.render()]).collect();
        let mut src = format!(
            "{name} :: {}\n{name} {}\n",
            sig_ty.join(" -> "),
            params.join(" ")
        );
        for (g, arm) in &guards {
            src.push_str(&format!("    | {} = {}\n", render(g), render(arm)));
        }
        src.push_str(&format!("    | otherwise = {}\n", render(&last)));
        if !wheres.is_empty() {
            src.push_str("    where\n");
            for (w, rhs) in &wheres {
                src.push_str(&format!("        {w} = {}\n", render(rhs)));
            }
        }

        let mut body = last;
        for (g, arm) in guards.into_iter().rev() {
            body = Expr::If(b(g), b(arm), b(body));
        }
        for (w, rhs) in wheres.into_iter().rev() {
            body = Expr::Let(w, b(rhs), b(body));
        }
        self.helpers.push(HelperSig { name: name.into(), args, ret });
        (src, Expr::Lam(params, b(body)))
    }

    fn helper_eta(&mut self, name: &str) -> (String, Expr) {
        // Two arrows declared, ONE pattern bound: the body is a
        // function-typed expression and the emission pads the arity.
        let t1 = gen_ty(&mut self.r, 1, false);
        let t2 = gen_ty(&mut self.r, 1, false);
        let ret = gen_ty(&mut self.r, 1, false);
        let p1 = self.fresh_var();
        self.scope.push((p1.clone(), t1.clone()));
        let body = self.expr(&Ty::fun(t2.clone(), ret.clone()), 3);
        self.scope.pop();
        let src = format!(
            "{name} :: {} -> {} -> {}\n{name} {p1} = {}\n",
            t1.render(),
            t2.render(),
            ret.render(),
            render(&body)
        );
        self.helpers.push(HelperSig { name: name.into(), args: vec![t1, t2], ret });
        (src, Expr::Lam(vec![p1], b(body)))
    }

    fn helper_poly_where(&mut self, name: &str) -> (String, Expr) {
        // A where-bound identity with NO signature, used at Bool (the
        // guard), Int (a discarded const argument), the return type, and
        // whatever instantiations the random layer adds through
        // `poly_iden` — compiles only because where-binds generalize.
        let t1 = gen_ty(&mut self.r, 1, false);
        let ret = gen_ty(&mut self.r, 2, false);
        let p1 = self.fresh_var();
        self.fresh += 1;
        let iden = format!("pf{}", self.fresh);
        let xv = self.fresh_var();
        self.scope.push((p1.clone(), t1.clone()));
        self.poly_iden = Some(iden.clone());
        let g = self.bool_expr(2);
        let cond = Expr::App(b(Expr::Var(iden.clone())), vec![g]);
        let keep = self.expr(&ret, 2);
        let junk = self.expr(&Ty::Int, 1);
        let arm1 = Expr::Call(
            P::Const,
            vec![
                Expr::App(b(Expr::Var(iden.clone())), vec![keep]),
                Expr::Annot(
                    b(Expr::App(b(Expr::Var(iden.clone())), vec![junk])),
                    Ty::Int,
                ),
            ],
        );
        let arm2 = self.expr(&ret, 2);
        self.poly_iden = None;
        self.scope.pop();

        let src = format!(
            "{name} :: {} -> {}\n{name} {p1}\n    | {} = {}\n    | otherwise = {}\n    where\n        {iden} {xv} = {xv}\n",
            t1.render(),
            ret.render(),
            render(&cond),
            render(&arm1),
            render(&arm2)
        );
        let body = Expr::Let(
            iden,
            b(Expr::Lam(vec![xv.clone()], b(Expr::Var(xv)))),
            b(Expr::If(b(cond), b(arm1), b(arm2))),
        );
        self.helpers.push(HelperSig { name: name.into(), args: vec![t1], ret });
        (src, Expr::Lam(vec![p1], b(body)))
    }
}

// ---------------------------------------------------------------------------
// Whole programs
// ---------------------------------------------------------------------------

/// The polymorphic scaffold every program carries: hand shapes that force
/// the historically fragile paths (a point-free operator alias, fewer
/// patterns than arrows, a user-defined higher-order combinator) — the
/// random layer instantiates them at whatever types come up.
const SCAFFOLD: &str = "\
fuzzTwice :: (a -> a) -> a -> a
fuzzTwice f x = f (f x)
";

/// One generated program: (source, expected stdout lines).
fn program_for(seed: u64, index: u64) -> (String, Vec<String>) {
    let mut g = Gen {
        r: Rng::new(seed ^ index.wrapping_mul(0x9E37_79B9_7F4A_7C15)),
        scope: Vec::new(),
        fresh: 0,
        helpers: Vec::new(),
        poly_iden: None,
    };
    let mut body = String::from(SCAFFOLD);

    // 0-2 generated top-level helpers, callable from everything below
    // (later helpers can call earlier ones).
    let n_help = g.r.below(3);
    let mut env: Env = Rc::new(EnvNode::Nil);
    for i in 0..n_help {
        let (src, lam) = g.gen_helper(i + 1);
        body.push('\n');
        body.push_str(&src);
        let hname = g.helpers.last().unwrap().name.clone();
        let hval = eval(&lam, &env);
        env = env_push(&env, &hname, hval);
    }

    body.push_str("\nmain :: IO ()\nmain = do\n");
    let mut expected = Vec::new();

    // A few interleaved do-statements — lets and pattern binds through
    // `return` (A14) — followed by two guaranteed printing statements.
    let n_mixed = 2 + g.r.below(3);
    for _ in 0..n_mixed {
        match g.r.below(6) {
            0 => {
                let t = gen_ty(&mut g.r, 2, false);
                let e = g.expr(&t, 2);
                let v = eval(&e, &env);
                let name = g.fresh_var();
                body.push_str(&format!(
                    "    let {name} = ({} :: {})\n",
                    render(&e),
                    t.render()
                ));
                env = env_push(&env, &name, v);
                g.scope.push((name, t));
            }
            1 => {
                let t = gen_ty(&mut g.r, 2, false);
                let e = g.expr(&t, 2);
                let v = eval(&e, &env);
                let name = g.fresh_var();
                body.push_str(&format!(
                    "    {name} <- return ({} :: {})\n",
                    render(&e),
                    t.render()
                ));
                env = env_push(&env, &name, v);
                g.scope.push((name, t));
            }
            2 => {
                let ta = gen_ty(&mut g.r, 1, false);
                let tb = gen_ty(&mut g.r, 1, false);
                let e = g.expr(&Ty::pair(ta.clone(), tb.clone()), 2);
                let (va, vb) = match eval(&e, &env) {
                    Value::Pair(a, b) => (*a, *b),
                    v => panic!("pair bind on {v:?}"),
                };
                let na = g.fresh_var();
                let nb = g.fresh_var();
                body.push_str(&format!(
                    "    ({na}, {nb}) <- return ({} :: ({}, {}))\n",
                    render(&e),
                    ta.render(),
                    tb.render()
                ));
                env = env_push(&env, &na, va);
                env = env_push(&env, &nb, vb);
                g.scope.push((na, ta));
                g.scope.push((nb, tb));
            }
            3 => {
                // A refutable `Just` bind that matches by construction —
                // the MonadFail fallback is compiled in but never taken.
                let t = gen_ty(&mut g.r, 1, false);
                let inner = g.expr(&t, 2);
                let v = eval(&inner, &env);
                let name = g.fresh_var();
                body.push_str(&format!(
                    "    Just {name} <- return ((Just {}) :: (Maybe {}))\n",
                    render(&inner),
                    t.render()
                ));
                env = env_push(&env, &name, v);
                g.scope.push((name, t));
            }
            _ => {
                emit_print(&mut g, &mut body, &mut expected, &env);
            }
        }
    }
    emit_print(&mut g, &mut body, &mut expected, &env);
    emit_print(&mut g, &mut body, &mut expected, &env);
    (body, expected)
}

/// One printing statement: `print (e :: T)` at a random fun-free type, or
/// `putStrLn` when the type came up String (the raw-output path, no show).
fn emit_print(g: &mut Gen, body: &mut String, expected: &mut Vec<String>, env: &Env) {
    let ty = gen_ty(&mut g.r, 2, false);
    let depth = 3 + g.r.below(3);
    let e = g.expr(&ty, depth);
    let v = eval(&e, env);
    if ty == Ty::Str && g.r.chance(1, 2) {
        body.push_str(&format!("    putStrLn ({})\n", render(&e)));
        expected.push(as_str(&v));
    } else {
        body.push_str(&format!("    print ({} :: {})\n", render(&e), ty.render()));
        expected.push(show_val(&v));
    }
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Deterministic seed for the whole suite. Bump only deliberately: it
/// re-rolls every generated program.
const BATCH_SEED: u64 = 0x6261_636B_665A_7A31; // "backfZz1"

/// Compile (alternating the two refutation entries, so both the stamp
/// checks and the WHNF claim checkers sweep generated shapes), run under
/// mlua with `print` captured, and byte-compare against the reference
/// evaluator's expected lines.
fn run_one(seed: u64, index: u64) {
    let (src, expected) = program_for(seed, index);
    let compiled = if index % 2 == 0 {
        mllc::compile_with_stamp_refutation(&src, Path::new("."), &[])
    } else {
        mllc::compile_with_whnf_refutation(&src, Path::new("."), &[])
    };
    let r = match compiled {
        Ok(r) => r,
        Err(e) => panic!(
            "backend fuzz: generated program failed to COMPILE \
             (index {index}, seed {seed:#x}):\n{e}\nsource:\n{src}"
        ),
    };
    let lua = mlua::Lua::new();
    let captured = lua.create_table().unwrap();
    let sink = captured.clone();
    let print_fn = lua
        .create_function(move |_, args: mlua::Variadic<mlua::Value>| -> mlua::Result<()> {
            let parts: Vec<String> = args
                .iter()
                .map(|v| match v {
                    mlua::Value::String(s) => Ok(s.to_str()?.to_string()),
                    other => Ok(format!("{:?}", other)),
                })
                .collect::<mlua::Result<_>>()?;
            let n = sink.raw_len();
            sink.raw_set(n + 1, parts.join("\t"))?;
            Ok(())
        })
        .unwrap();
    lua.globals().set("print", print_fn).unwrap();
    if let Err(e) = lua.load(&r.lua_code).set_name("backend_fuzz").exec() {
        panic!(
            "backend fuzz: generated program CRASHED at runtime \
             (index {index}, seed {seed:#x}):\n{e}\nsource:\n{src}"
        );
    }
    let printed: Vec<String> = captured
        .sequence_values::<String>()
        .collect::<mlua::Result<_>>()
        .unwrap();
    assert_eq!(
        printed, expected,
        "backend fuzz: output (left) diverges from the reference \
         evaluator (right) on index {index} (seed {seed:#x}).\nsource:\n{src}"
    );
}

fn fuzz_run(count: u64) {
    mllc::with_compiler_stack(|| {
        for i in 0..count {
            if std::env::var_os("MLL_FUZZ_TRACE").is_some() {
                // A panic inside program_for (a generator/evaluator bug)
                // carries no index; the trace names the one in flight.
                eprintln!("fuzz index {i}");
            }
            run_one(BATCH_SEED, i);
        }
    })
}

/// Always-run smoke batch: every program takes a debug-mode full-pipeline
/// compile, so the count stays modest; the substantial batch is the
/// ignored test below.
#[test]
fn backend_fuzz_smoke() {
    fuzz_run(100);
}

/// The substantial batch. Run explicitly with:
///     cargo test -p mll-tests --test backend_fuzz -- --ignored
#[test]
#[ignore = "long fuzz batch; run with --ignored"]
fn backend_fuzz_batch() {
    fuzz_run(2_000);
}

/// Minimization hook: MLL_REPRO_FILE=<path> cargo test … repro_file -- --ignored
#[test]
#[ignore = "manual minimization hook"]
fn repro_file() {
    let path = std::env::var("MLL_REPRO_FILE").expect("set MLL_REPRO_FILE");
    let src = std::fs::read_to_string(&path).expect("readable repro file");
    // MLL_REPRO_MODE=whnf runs the WHNF-refutation entry (and EXECUTES the
    // output, where its checkers live); default is the stamp entry.
    let whnf = std::env::var("MLL_REPRO_MODE").as_deref() == Ok("whnf");
    mllc::with_compiler_stack(|| {
        let compiled = if whnf {
            mllc::compile_with_whnf_refutation(&src, Path::new("."), &[])
        } else {
            mllc::compile_with_stamp_refutation(&src, Path::new("."), &[])
        };
        match compiled {
            Ok(r) => {
                if std::env::var_os("MLL_REPRO_DUMP").is_some() {
                    println!("EMITTED:\n{}", r.lua_code);
                }
                let lua = mlua::Lua::new();
                lua.globals()
                    .set("print", lua.create_function(|_, args: mlua::Variadic<mlua::Value>| {
                        let parts: Vec<String> = args.iter()
                            .map(|v| match v {
                                mlua::Value::String(s) => s.to_str().map(|s| s.to_string()),
                                other => Ok(format!("{other:?}")),
                            })
                            .collect::<mlua::Result<_>>()?;
                        println!("PRINTED: {}", parts.join("\t"));
                        Ok(())
                    }).unwrap())
                    .unwrap();
                match lua.load(&r.lua_code).set_name("repro").exec() {
                    Ok(()) => println!("REPRO: compiles clean and runs"),
                    Err(e) => println!("REPRO RUNTIME ERROR:\n{e}"),
                }
            }
            Err(e) => println!("REPRO ERROR:\n{e}"),
        }
    });
}
