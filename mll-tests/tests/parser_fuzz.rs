//! Bounded parser fuzzing for the Pratt loop, fixity handling, layout, and
//! the nesting-depth guard.
//!
//! The generator produces random-but-structured .mll snippets: operator
//! chains over randomly declared fixities (infixl/infixr/infix at random
//! precedences, symbolic and backtick operators), sections, prefix minus,
//! lambdas/let/case/do, list and tuple nests, layout continuation lines,
//! and dedicated probes hugging `mllc::MAX_NESTING_DEPTH` from both sides.
//! The property under test: `mllc::compile` either accepts or rejects with
//! a Diagnostic — it never panics, never aborts (stack overflow), and never
//! hangs (a watchdog enforces a per-input timeout).
//!
//! Everything is deterministic and offline. Each input is derived from
//! (BATCH_SEED, index) through SplitMix64, so any failure reproduces by
//! seed alone; the watchdog/panic paths print the seed, index, and the
//! offending source.

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

/// SplitMix64: tiny, deterministic, and good enough for input shaping.
/// No external RNG crate — the suite stays fully offline.
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
    /// Uniform in 0..n (n > 0).
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    /// True with probability num/den.
    fn chance(&mut self, num: u64, den: u64) -> bool {
        self.next() % den < num
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

const SYM_OPS: &[&str] = &["<+>", "<#>", "|||", "***", "+.", "~>", "<~", "=%="];
const BT_OPS: &[&str] = &["foo", "bar", "qux"];
const BUILTIN_OPS: &[&str] = &[
    "+", "-", "*", "/", "==", "/=", "<", ">", "<=", ">=", "&&", "||", "<>",
    "++", "!!", "$", ".", ">>=", ">>", ":", "<$>", "<*>",
];
const VARS: &[&str] = &["x", "y", "z", "acc", "n"];

/// Random-but-structured expression. `depth` bounds recursion so a single
/// input stays small; the deep-nesting probes are generated separately.
fn gen_expr(r: &mut Rng, depth: usize, out: &mut String) {
    if depth == 0 {
        gen_atom(r, out);
        return;
    }
    match r.below(10) {
        // Infix chain: 1-4 operators of random fixity, random operands.
        0..=3 => {
            gen_operand(r, depth - 1, out);
            for _ in 0..(1 + r.below(3)) {
                out.push(' ');
                gen_op(r, out);
                out.push(' ');
                // Occasional layout continuation: newline + deep indent
                // before the next operand (the operator stays on the
                // previous line, a legal continuation shape).
                if r.chance(1, 12) {
                    out.push('\n');
                    out.push_str("        ");
                }
                gen_operand(r, depth - 1, out);
            }
        }
        // Prefix minus over a tight operand.
        4 => {
            out.push_str("- ");
            gen_operand(r, depth - 1, out);
        }
        // Lambda.
        5 => {
            out.push('\\');
            out.push_str(r.pick(VARS));
            out.push_str(" -> ");
            gen_expr(r, depth - 1, out);
        }
        // if/then/else.
        6 => {
            out.push_str("if ");
            gen_expr(r, depth - 1, out);
            out.push_str(" then ");
            gen_expr(r, depth - 1, out);
            out.push_str(" else ");
            gen_expr(r, depth - 1, out);
        }
        // let-in.
        7 => {
            out.push_str("let ");
            out.push_str(r.pick(VARS));
            out.push_str(" = ");
            gen_expr(r, depth - 1, out);
            out.push_str(" in ");
            gen_expr(r, depth - 1, out);
        }
        // Application spine.
        8 => {
            gen_atom(r, out);
            for _ in 0..(1 + r.below(2)) {
                out.push(' ');
                gen_operand(r, 0, out);
            }
        }
        // Case with two alternatives (layout-sensitive).
        _ => {
            out.push_str("case ");
            gen_expr(r, depth - 1, out);
            out.push_str(" of\n        Just ");
            out.push_str(r.pick(VARS));
            out.push_str(" -> ");
            gen_expr(r, depth - 1, out);
            out.push_str("\n        Nothing -> ");
            gen_expr(r, depth - 1, out);
        }
    }
}

/// An operand inside an infix chain: an atom, or a parenthesized
/// subexpression (which may itself be a chain).
fn gen_operand(r: &mut Rng, depth: usize, out: &mut String) {
    if depth > 0 && r.chance(2, 5) {
        out.push('(');
        gen_expr(r, depth, out);
        out.push(')');
    } else {
        gen_atom(r, out);
    }
}

fn gen_atom(r: &mut Rng, out: &mut String) {
    match r.below(12) {
        0 => out.push_str(&format!("{}", r.below(1000))),
        1 => out.push_str(&format!("{}.{}", r.below(100), r.below(100))),
        2 => out.push_str("\"s\""),
        3 => out.push_str(r.pick(VARS)),
        4 => out.push_str("True"),
        5 => out.push_str("Nothing"),
        6 => out.push_str("(Just 1)"),
        7 => out.push_str("[1, 2, 3]"),
        8 => out.push_str("(1, \"a\")"),
        9 => out.push_str("[1 .. 9]"),
        10 => out.push_str("()"),
        // Sections: right, left, bare operator, negation-in-parens.
        _ => {
            let mut sec = String::from("(");
            match r.below(4) {
                0 => {
                    gen_op(r, &mut sec);
                    sec.push_str(" 2");
                }
                1 => {
                    sec.push_str("2 ");
                    gen_op(r, &mut sec);
                }
                2 => gen_op(r, &mut sec),
                _ => sec.push_str("- 2"),
            }
            sec.push(')');
            out.push_str(&sec);
        }
    }
}

fn gen_op(r: &mut Rng, out: &mut String) {
    match r.below(10) {
        0..=4 => out.push_str(r.pick(BUILTIN_OPS)),
        5..=7 => out.push_str(r.pick(SYM_OPS)),
        _ => {
            out.push('`');
            out.push_str(r.pick(BT_OPS));
            out.push('`');
        }
    }
}

/// A whole module: random fixity declarations for the custom operators,
/// definitions for them, a couple of value bindings, and a main.
fn gen_module(r: &mut Rng) -> String {
    let mut s = String::new();
    // Random fixities. Redeclaration of the same operator is deliberately
    // possible (duplicate fixity declarations must be rejected cleanly,
    // not crash).
    for op in SYM_OPS {
        if r.chance(2, 3) {
            let kw = *r.pick(&["infixl", "infixr", "infix"]);
            s.push_str(&format!("{} {} {}\n", kw, r.below(10), op));
        }
    }
    for op in BT_OPS {
        if r.chance(1, 3) {
            let kw = *r.pick(&["infixl", "infixr", "infix"]);
            s.push_str(&format!("{} {} `{}`\n", kw, r.below(10), op));
        }
    }
    // Definitions so the operators exist (type errors are fine; panics are
    // not).
    for op in SYM_OPS {
        s.push_str(&format!("({}) :: Int -> Int -> Int\n", op));
        s.push_str(&format!("({}) a b = a + b\n", op));
    }
    for op in BT_OPS {
        s.push_str(&format!("{} :: Int -> Int -> Int\n", op));
        s.push_str(&format!("{} a b = a * b\n", op));
    }
    // A couple of value bindings with generated bodies.
    for (i, v) in ["v1", "v2"].iter().enumerate() {
        if r.chance(1, 2) {
            s.push_str(&format!("{} :: Int\n", v));
        }
        s.push_str(&format!("{} = ", v));
        let mut body = String::new();
        gen_expr(r, 2 + i, &mut body);
        s.push_str(&body);
        s.push('\n');
    }
    // main: sometimes a do block with statements, sometimes a single print.
    s.push_str("main :: IO ()\n");
    if r.chance(1, 2) {
        s.push_str("main = do\n");
        for _ in 0..(1 + r.below(3)) {
            s.push_str("    ");
            if r.chance(1, 4) {
                s.push_str("let lv = ");
            } else if r.chance(1, 4) {
                s.push_str("bv <- return ");
            } else {
                s.push_str("print ");
            }
            let mut body = String::new();
            gen_expr(r, 2, &mut body);
            s.push_str(&body);
            s.push('\n');
        }
        s.push_str("    return ()\n");
    } else {
        s.push_str("main = print (");
        let mut body = String::new();
        gen_expr(r, 3, &mut body);
        s.push_str(&body);
        s.push_str(")\n");
    }
    s
}

/// Deep-nesting probes hugging MAX_NESTING_DEPTH from both sides, plus
/// long-but-flat shapes that must stay iterative. Accept or clean reject;
/// never a native stack overflow (which would abort the process, not fail
/// the test gracefully — that is the point of the depth guard).
fn gen_deep(r: &mut Rng) -> String {
    let limit = mllc::MAX_NESTING_DEPTH;
    let d = limit - 60 + r.below(120); // straddles the limit
    match r.below(5) {
        // Parenthesis nest.
        0 => format!(
            "main :: IO ()\nmain = print {}1{}\n",
            "(".repeat(d),
            ")".repeat(d)
        ),
        // List nest.
        1 => format!(
            "main :: IO ()\nmain = print {}{}\n",
            "[".repeat(d),
            "]".repeat(d)
        ),
        // Lambda nest.
        2 => format!(
            "main :: IO ()\nmain = print (({}1{}) 0)\n",
            "\\x -> ".repeat(d),
            " "
        ),
        // Long flat infix chain (right-spine handling must be iterative).
        3 => {
            let n = 3 * limit;
            let op = *r.pick(&["+", "<>", ":", "$"]);
            let mut s = String::from("main :: IO ()\nmain = print (0");
            for _ in 0..n {
                s.push(' ');
                s.push_str(op);
                s.push_str(" 0");
            }
            s.push_str(")\n");
            s
        }
        // Long do block.
        _ => {
            let mut s = String::from("main :: IO ()\nmain = do\n");
            for i in 0..(limit / 2) {
                s.push_str(&format!("    let v{i} = {i}\n"));
            }
            s.push_str("    return ()\n");
            s
        }
    }
}

fn input_for(seed: u64, index: u64) -> String {
    let mut r = Rng::new(seed ^ index.wrapping_mul(0x0DD5_53CC_75CB_1093));
    // Every 97th input is a deep-nesting probe (they are much slower).
    if index % 97 == 96 {
        gen_deep(&mut r)
    } else {
        gen_module(&mut r)
    }
}

/// Deterministic seed for the whole suite. Bump only deliberately: it
/// re-rolls every generated input.
const BATCH_SEED: u64 = 0x6D6C_6C5F_6675_7A7A; // "mll_fuzz"

/// Per-input wall-clock budget. Debug-mode compiles of the deep probes run
/// well under a second; anything past this is a hang or a runaway.
const PER_INPUT_TIMEOUT: Duration = Duration::from_secs(30);

/// Run `count` fuzz inputs on a compiler-calibrated stack with a per-input
/// watchdog. Any panic in `mllc::compile` fails the test; a hang trips the
/// timeout with the input's seed and source in the message.
fn fuzz_run(count: u64) {
    let (tx, rx) = mpsc::channel::<u64>();
    let worker = std::thread::Builder::new()
        .stack_size(mllc::COMPILER_STACK_SIZE)
        .spawn(move || {
            for i in 0..count {
                let src = input_for(BATCH_SEED, i);
                // Accept or reject — both fine. Panics/aborts are the bug.
                // The parser (lex + parse, own-module fixity scan included)
                // is the fuzz target and runs on EVERY input; the full
                // pipeline is sampled every 25th input — each debug-mode
                // compile re-elaborates the whole Prelude (~150 ms), which
                // would otherwise be nearly all of the batch's runtime.
                if let Ok(tokens) = mllc::lexer::lex(&src) {
                    let _ = mllc::parser::parse(tokens);
                }
                // Deep probes are excluded from pipeline sampling: they
                // target the parser's depth guard (covered by the parse
                // above); pushing a several-thousand-statement do block
                // through the typechecker gates this suite on backend
                // compile-time instead (generalize recomputes the
                // environment's free variables per do-let binding — O(n^2),
                // ~30 s in debug at 3000 lets; measured and reported, not a
                // parser property).
                if i % 25 == 0 && i % 97 != 96 {
                    let _ = mllc::compile(&src, Path::new("."), &[]);
                }
                tx.send(i).expect("watchdog alive");
            }
        })
        .expect("spawn fuzz worker");

    let mut done = 0u64;
    while done < count {
        match rx.recv_timeout(PER_INPUT_TIMEOUT) {
            Ok(i) => done = i + 1,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                panic!(
                    "parser fuzz: input {done} (seed {BATCH_SEED:#x}) exceeded \
                     {PER_INPUT_TIMEOUT:?} — likely hang. Source:\n{}",
                    input_for(BATCH_SEED, done)
                );
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    // A worker panic (from a compile panic) resurfaces here.
    if let Err(e) = worker.join() {
        eprintln!(
            "parser fuzz: compiler panicked on input {done} \
             (seed {BATCH_SEED:#x}). Source:\n{}",
            input_for(BATCH_SEED, done)
        );
        std::panic::resume_unwind(e);
    }
    assert_eq!(done, count, "worker exited early without a panic");
}

/// Always-run smoke batch: enough to catch gross regressions in the Pratt
/// loop or the depth guard on every `cargo test`.
#[test]
fn parser_fuzz_smoke() {
    fuzz_run(2_000);
}

/// The substantial batch. Ignored by default (minutes of debug-mode work);
/// run explicitly with:
///     cargo test -p mll-tests --test parser_fuzz -- --ignored
#[test]
#[ignore = "long fuzz batch; run with --ignored"]
fn parser_fuzz_batch() {
    fuzz_run(100_000);
}
