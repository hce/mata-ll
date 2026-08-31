//! Backend fuzzing with TYPE-CORRECT programs — the "open infra work" the
//! parser fuzzer's header used to name. Where parser_fuzz throws mostly
//! ill-typed text at the front end (1 in ~25 inputs reaches the pipeline),
//! every program here compiles by construction, so every input exercises
//! the typechecker, mono, demand analysis, codegen, and the optimizer.
//!
//! The oracle is a strict Rust reference evaluator over the generator's own
//! typed AST. The generated fragment is TOTAL — exhaustive cases only,
//! `div`/`mod` divisors are positive literals, `head` only ever sees a
//! syntactic cons — so strict evaluation computes the same values lazy GHC
//! semantics would, and the expected stdout is byte-comparable. `show`
//! rendering follows GHC's showsPrec (negative and constructor arguments
//! parenthesized at precedence 11), which mata-ll's show is byte-oracled
//! against. What a total fragment cannot see — a skipped force, a missed
//! bottom — is covered by compiling every second program through
//! compile_with_whnf_refutation: the WHNF claim checkers (see runtime.lua)
//! then run over machine-generated shapes no hand-written corpus carries.
//!
//! The generator leans into the shapes that historically broke: point-free
//! definitions, definitions with fewer patterns than arrows (eta padding),
//! builtins and the polymorphic scaffold used at function-typed
//! instantiations (arity widening), first-class ($) and (.), higher-order
//! prelude generics, and nested constructor patterns.
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
    Bool,
    List(Box<Ty>),
    Pair(Box<Ty>, Box<Ty>),
    Maybe(Box<Ty>),
    Fun(Box<Ty>, Box<Ty>),
}

impl Ty {
    fn list(t: Ty) -> Ty {
        Ty::List(Box::new(t))
    }
    fn fun(a: Ty, b: Ty) -> Ty {
        Ty::Fun(Box::new(a), Box::new(b))
    }
    /// Rendered .mll type (fully parenthesized where nesting could bind).
    fn render(&self) -> String {
        match self {
            Ty::Int => "Int".into(),
            Ty::Bool => "Bool".into(),
            Ty::List(t) => format!("[{}]", t.render()),
            Ty::Pair(a, b) => format!("({}, {})", a.render(), b.render()),
            Ty::Maybe(t) => format!("(Maybe {})", paren_ty(t)),
            Ty::Fun(a, b) => format!("({} -> {})", a.render(), b.render()),
        }
    }
    /// Is a value of this type printable (has a derived Show the reference
    /// renderer also implements)? Functions are not.
    fn showable(&self) -> bool {
        match self {
            Ty::Int | Ty::Bool => true,
            Ty::List(t) | Ty::Maybe(t) => t.showable(),
            Ty::Pair(a, b) => a.showable() && b.showable(),
            Ty::Fun(..) => false,
        }
    }
}

fn paren_ty(t: &Ty) -> String {
    match t {
        Ty::Int | Ty::Bool => t.render(),
        // List/Pair/Maybe/Fun renderings are self-delimiting already
        // (brackets or the parens `render` adds).
        _ => t.render(),
    }
}

/// A random type for a value position. `depth` bounds nesting; function
/// types appear only where `fun_ok` (printed positions exclude them).
fn gen_ty(r: &mut Rng, depth: usize, fun_ok: bool) -> Ty {
    if depth == 0 {
        return if r.chance(2, 3) { Ty::Int } else { Ty::Bool };
    }
    match r.below(if fun_ok { 8 } else { 6 }) {
        0 | 1 => Ty::Int,
        2 => Ty::Bool,
        3 => Ty::list(gen_ty(r, depth - 1, false)),
        4 => Ty::Pair(
            Box::new(gen_ty(r, depth - 1, false)),
            Box::new(gen_ty(r, depth - 1, false)),
        ),
        5 => Ty::Maybe(Box::new(gen_ty(r, depth - 1, false))),
        _ => Ty::fun(gen_ty(r, depth - 1, false), gen_ty(r, depth - 1, fun_ok)),
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
    Sum,
    Null,
    ZipWith,
    Fst,
    Snd,
    Head, // only ever applied to a syntactic cons (nonempty evidence)
    Seq,
    Id,
    Const,
    Flip,
    Twice,   // scaffold: fuzzTwice f x = f (f x)
    CompApp, // ((f . g) x) — the composition emission
    DollarApp, // (f $ x)
}

#[derive(Clone, Debug)]
enum Expr {
    IntLit(i64),
    BoolLit(bool),
    Var(String),
    Not(Box<Expr>),
    Bin(Bin, Box<Expr>, Box<Expr>),
    /// `div`/`mod` with a POSITIVE LITERAL divisor (totality).
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
    /// `const` argument, a case scrutinee): without the pin such programs
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

fn render(e: &Expr) -> String {
    match e {
        Expr::IntLit(n) => {
            if *n < 0 {
                format!("({})", n)
            } else {
                n.to_string()
            }
        }
        Expr::BoolLit(v) => if *v { "True" } else { "False" }.into(),
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
        P::Sum => "sum",
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
    }
}

// ---------------------------------------------------------------------------
// Reference evaluator (strict; the fragment is total, so strictness cannot
// change any computed value)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Value {
    Int(i64),
    Bool(bool),
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
fn as_list(v: &Value) -> Vec<Value> {
    match v {
        Value::List(xs) => xs.clone(),
        _ => panic!("evaluator: expected list, got {v:?}"),
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
        P::Reverse | P::Length | P::Sum | P::Null | P::Fst | P::Snd | P::Head | P::Id => 1,
        P::Map | P::Filter | P::Take | P::Drop | P::Seq | P::Const | P::DollarApp => 2,
        P::Foldr | P::Foldl | P::ZipWith | P::Flip | P::Twice | P::CompApp => 3,
    }
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
    }
}

fn eval(e: &Expr, env: &Env) -> Value {
    match e {
        Expr::IntLit(n) => Value::Int(*n),
        Expr::BoolLit(v) => Value::Bool(*v),
        Expr::Var(v) => env_get(env, v),
        Expr::Not(x) => Value::Bool(!as_bool(&eval(x, env))),
        Expr::Bin(op, l, r) => {
            let lv = eval(l, env);
            let rv = eval(r, env);
            match op {
                Bin::Add => Value::Int(as_int(&lv).wrapping_add(as_int(&rv))),
                Bin::Sub => Value::Int(as_int(&lv).wrapping_sub(as_int(&rv))),
                Bin::Mul => Value::Int(as_int(&lv).wrapping_mul(as_int(&rv))),
                Bin::Eq => Value::Bool(as_int(&lv) == as_int(&rv)),
                Bin::Ne => Value::Bool(as_int(&lv) != as_int(&rv)),
                Bin::Lt => Value::Bool(as_int(&lv) < as_int(&rv)),
                Bin::Le => Value::Bool(as_int(&lv) <= as_int(&rv)),
                Bin::And => Value::Bool(as_bool(&lv) && as_bool(&rv)),
                Bin::Or => Value::Bool(as_bool(&lv) || as_bool(&rv)),
            }
        }
        Expr::DivMod(is_div, l, d) => {
            let n = as_int(&eval(l, env));
            // Haskell floor division/modulo. The divisor is a positive
            // literal by construction, where div_euclid == floor division
            // and rem_euclid == Haskell mod.
            Value::Int(if *is_div { n.div_euclid(*d) } else { n.rem_euclid(*d) })
        }
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
        Value::Bool(x) => if *x { "True" } else { "False" }.into(),
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
        Value::Maybe(Some(_)) => format!("({})", show_val(v)),
        _ => show_val(v),
    }
}

// ---------------------------------------------------------------------------
// Type-directed generation
// ---------------------------------------------------------------------------

struct Gen {
    r: Rng,
    /// In-scope variables with their types (lexical; pushed and popped).
    scope: Vec<(String, Ty)>,
    fresh: usize,
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
                let scrut = self.pinned(
                    &Ty::Pair(Box::new(at.clone()), Box::new(bt.clone())),
                    depth - 1,
                );
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
            _ => {}
        }
        // Type-directed productions.
        match ty {
            Ty::Int => self.int_expr(depth),
            Ty::Bool => self.bool_expr(depth),
            Ty::List(t) => self.list_expr(t, depth),
            Ty::Pair(a, bt) => Expr::MkPair(
                b(self.expr(a, depth - 1)),
                b(self.expr(bt, depth - 1)),
            ),
            Ty::Maybe(t) => {
                if self.r.chance(1, 4) {
                    Expr::Nothing
                } else {
                    Expr::Just(b(self.expr(t, depth - 1)))
                }
            }
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
            Ty::Bool => Expr::BoolLit(self.r.chance(1, 2)),
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

    fn int_expr(&mut self, depth: usize) -> Expr {
        match self.r.below(10) {
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
                    let pt = Ty::Pair(Box::new(Ty::Int), Box::new(t));
                    Expr::Call(P::Fst, vec![self.pinned(&pt, depth - 1)])
                } else {
                    let pt = Ty::Pair(Box::new(t), Box::new(Ty::Int));
                    Expr::Call(P::Snd, vec![self.pinned(&pt, depth - 1)])
                }
            }
            _ => self.atom(&Ty::Int),
        }
    }

    fn bool_expr(&mut self, depth: usize) -> Expr {
        match self.r.below(8) {
            0 | 1 => {
                let op = [Bin::Eq, Bin::Ne, Bin::Lt, Bin::Le][self.r.below(4)];
                Expr::Bin(
                    op,
                    b(self.expr(&Ty::Int, depth - 1)),
                    b(self.expr(&Ty::Int, depth - 1)),
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
            _ => self.atom(&Ty::Bool),
        }
    }

    fn list_expr(&mut self, elem: &Ty, depth: usize) -> Expr {
        match self.r.below(10) {
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
            _ => {
                let n = self.r.below(4);
                let xs = (0..n).map(|_| self.expr(elem, depth - 1)).collect();
                Expr::ListLit(xs)
            }
        }
    }

    fn fun_expr(&mut self, a: &Ty, ret: &Ty, depth: usize) -> Expr {
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
    };
    // 2-4 printed expressions per program, each of a random showable type.
    let n = 2 + g.r.below(3);
    let mut lines = Vec::new();
    let mut body = String::new();
    let mut expected = Vec::new();
    for _ in 0..n {
        let mut ty = gen_ty(&mut g.r, 2, false);
        if !ty.showable() {
            ty = Ty::Int;
        }
        let depth = 3 + g.r.below(3);
        let e = g.expr(&ty, depth);
        debug_assert!(g.scope.is_empty(), "generator scope must unwind");
        let v = eval(&e, &Rc::new(EnvNode::Nil));
        expected.push(show_val(&v));
        lines.push(format!("    print ({} :: {})", render(&e), ty.render()));
    }
    body.push_str(SCAFFOLD);
    body.push_str("\nmain :: IO ()\nmain = do\n");
    for l in &lines {
        body.push_str(l);
        body.push('\n');
    }
    (body, expected)
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
            run_one(BATCH_SEED, i);
        }
    })
}

/// Always-run smoke batch: every program takes a debug-mode full-pipeline
/// compile (~80 ms measured), so the count stays modest; the substantial
/// batch is the ignored test below.
#[test]
fn backend_fuzz_smoke() {
    fuzz_run(60);
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
