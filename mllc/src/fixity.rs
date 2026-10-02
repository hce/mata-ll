//! Operator fixity: which entity carries which precedence, and the grouping
//! of infix chains.
//!
//! A fixity belongs to an operator ENTITY — the declaration a module makes
//! — never to a spelling. Two modules may each define a `<+>`, a local
//! binder may be used in backticks under the name of a Prelude function,
//! and every occurrence groups under the fixity of the one entity it
//! denotes in its own scope. That is why the parser does not group infix
//! expressions: it produces flat chains (`Expr::OpChain`, `Expr::Section`,
//! `Type::OpChain`), and module resolution — the one place where a name is
//! bound to its entity (`modules::Rename`) — hands each chain to this
//! module together with the fixity of every operator in it.
//!
//! The grouping itself is the Haskell 2010 fixity resolution (section
//! 10.6), including its treatment of prefix minus, with GHC's rule for
//! section operands (section 3.5).

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::ast::{Assoc, Decl, Expr, SectionSide, Span, Type};
use crate::types::Diagnostic;

/// The invariant the passes after module resolution rely on, as the message
/// of their `unreachable!` arms for the chain nodes.
pub const UNGROUPED: &str =
    "infix chains and sections are grouped by module resolution (modules::Rename)";

/// Associativity and precedence (0-9).
pub type Fixity = (Assoc, u8);

/// The fixity of an operator without a fixity declaration (Haskell 2010
/// section 4.4.2).
pub const DEFAULT_FIXITY: Fixity = (Assoc::Left, 9);

/// The fixity of every declared operator entity of a program, by CANONICAL
/// name (see `modules::ModuleLoader::resolve_module`), one table per
/// namespace. An entity without an entry has `DEFAULT_FIXITY`.
#[derive(Clone, Debug, Default)]
pub struct Fixities {
    pub vals: HashMap<String, Fixity>,
    pub tys: HashMap<String, Fixity>,
}

/// The Prelude's fixity declarations: the table every program starts from.
/// The Prelude's names are their own canonical names, and it declares no
/// type operators. Parsed once per process from the embedded Prelude
/// source, whose fixity declarations are the only statement of these
/// fixities in the compiler.
pub fn prelude() -> &'static Fixities {
    static TABLE: OnceLock<Fixities> = OnceLock::new();
    TABLE.get_or_init(|| {
        // The embedded Prelude is compiler-owned text and is parsed on
        // every compilation: it always lexes and parses.
        let tokens = crate::lexer::lex(crate::stdlib::PRELUDE)
            .unwrap_or_else(|e| panic!("the embedded Prelude does not lex: {e}"));
        let module = crate::parser::parse(tokens).unwrap_or_else(|errs| {
            let msgs: Vec<String> = errs.iter().map(|d| d.to_string()).collect();
            panic!("the embedded Prelude does not parse: {}", msgs.join("\n"))
        });
        let mut table = Fixities::default();
        for d in &module.decls {
            if let Decl::FixityDecl { assoc, prec, op, .. } = d {
                table.vals.insert(op.clone(), (*assoc, *prec));
            }
        }
        table
    })
}

/// An operator occurrence bound to its entity: the name as written (for
/// messages), the entity's canonical name (for the grouped tree), the
/// occurrence's position and the entity's fixity.
#[derive(Clone, Debug)]
pub struct Op {
    pub written: String,
    pub name: String,
    pub span: Span,
    pub assoc: Assoc,
    pub prec: u8,
}

/// What a grouped chain is at its root: the operator applied last, or a
/// prefix minus over the whole chain. A section's operand is checked
/// against it (`check_section_operand`).
#[derive(Clone, Debug)]
pub enum Top {
    Op { written: String, assoc: Assoc, prec: u8 },
    Negation,
}

/// One operand of a chain, already resolved: the expression and the
/// position of its prefix minus, if it has one.
pub type Operand = (Option<Span>, Expr);

/// Minimum binding power for the operand of prefix minus: Haskell's
/// `lexp6 -> - exp7` takes everything binding tighter than precedence 6, so
/// the operand continues through operators with precedence >= 7 (left
/// binding power >= 7*2) and stops at precedence 6 and below.
const NEGATION_OPERAND_MIN_PREC: u8 = 14;

/// Convert a Haskell fixity (Assoc, precedence 0-9) to binding powers
/// (left, right): precedence doubled, with the associativity deciding which
/// side binds one tighter.
fn binding(assoc: Assoc, prec: u8) -> (u8, u8) {
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

/// Group the chain `first (op operand)*` under the operators' fixities.
/// `rest` pairs each operator with the operand to its right. Returns the
/// grouped expression — `InfixApp` nodes named by the operators' canonical
/// names, `Negate` for prefix minus — and what stands at its root, or the
/// error for a chain the fixities define no grouping for.
pub fn group(first: Operand, rest: Vec<(Op, Operand)>) -> Result<(Expr, Top), Box<Diagnostic>> {
    let mut ops = Vec::with_capacity(rest.len());
    let mut operands = Vec::with_capacity(rest.len() + 1);
    operands.push(first);
    for (op, operand) in rest {
        ops.push(op);
        operands.push(operand);
    }
    let mut g = Grouper { ops, next_op: 0, operands: operands.into_iter(), depth: 0 };
    let (lhs, negated) = g.prefix(None)?;
    let (expr, last) = g.continue_infix(lhs, negated, 0, None)?;
    debug_assert!(g.next_op == g.ops.len(), "the outermost level takes every operator");
    let top = match last {
        Some(i) => {
            let op = &g.ops[i];
            Top::Op { written: op.written.clone(), assoc: op.assoc, prec: op.prec }
        }
        None => {
            debug_assert!(negated, "a chain has an operator or a prefix minus");
            Top::Negation
        }
    };
    Ok((expr, top))
}

/// The state of one chain's grouping: a precedence-climbing parse over the
/// chain's operands and operators instead of over tokens.
struct Grouper {
    ops: Vec<Op>,
    /// Index of the next operator not yet taken by any level.
    next_op: usize,
    /// The operands not yet read, in order: operator `i` stands between
    /// operand `i` and operand `i + 1`.
    operands: std::vec::IntoIter<Operand>,
    /// Recursion depth of `infix`, bounded like the parser's productions
    /// so a pathological right-nested chain is reported, not overflowed on.
    depth: usize,
}

impl Grouper {
    /// One operand and everything that binds to it at `min_prec` or
    /// tighter. `parent` is the operator whose right-hand side this is.
    fn infix(&mut self, min_prec: u8, parent: usize) -> Result<Expr, Box<Diagnostic>> {
        if self.depth >= crate::MAX_NESTING_DEPTH {
            return Err(crate::parser::nesting_error("expression", self.ops[parent].span));
        }
        self.depth += 1;
        let result = self.prefix(Some(parent)).and_then(|(lhs, negated)| {
            self.continue_infix(lhs, negated, min_prec, Some(parent)).map(|(e, _)| e)
        });
        self.depth -= 1;
        result
    }

    /// Read the next operand. Prefix minus has the fixity of binary
    /// subtraction — infixl 6 — with two consequences the grammar
    /// (`lexp6 -> - exp7`) enforces and GHC implements exactly:
    ///   * it cannot be the right operand of any operator at precedence 6
    ///     or higher (`a + -b`, `a - -b`, `a * -b` have no defined
    ///     grouping without parentheses);
    ///   * its operand is everything binding TIGHTER than 6, so `-a * b`
    ///     is `negate (a * b)`, while `-a + b` is `negate a + b` (the `+`
    ///     stops the operand).
    /// The flag says whether the result is a prefix-minus expression.
    fn prefix(&mut self, parent: Option<usize>) -> Result<(Expr, bool), Box<Diagnostic>> {
        let (neg, expr) = self.operands.next().expect("an operand follows every operator");
        let Some(minus_at) = neg else { return Ok((expr, false)) };
        if let Some(par) = parent
            && self.ops[par].prec >= 6
        {
            return Err(prefix_minus_rhs_err(&self.ops[par], minus_at));
        }
        let (operand, _) = self.continue_infix(expr, false, NEGATION_OPERAND_MIN_PREC, None)?;
        Ok((Expr::Negate(Box::new(operand)), true))
    }

    /// Decide what to do with the next operator, given the operator whose
    /// right-hand side is being grouped (`parent`) and the minimum binding
    /// power. `Ok(true)` = this level takes it, `Ok(false)` = it belongs to
    /// an enclosing level, `Err` = the two operators share a precedence but
    /// define no grouping (the Haskell precedence-parsing rule).
    fn should_consume(&self, parent: Option<usize>, min_prec: u8, op: usize) -> Result<bool, Box<Diagnostic>> {
        let op = &self.ops[op];
        if let Some(par) = parent.map(|p| &self.ops[p])
            && par.prec == op.prec
        {
            // Same precedence directly under `parent`: Haskell defines a
            // grouping only for infixl/infixl (the enclosing level takes it)
            // and infixr/infixr (this level takes it, nesting rightward).
            return match (par.assoc, op.assoc) {
                (Assoc::Left, Assoc::Left) => Ok(false),
                (Assoc::Right, Assoc::Right) => Ok(true),
                _ => Err(fixity_conflict_err(par, op)),
            };
        }
        let (lp, _) = binding(op.assoc, op.prec);
        Ok(lp >= min_prec)
    }

    /// Extend `lhs` with the operators this level takes. Returns the
    /// expression and the index of the operator applied last, if any.
    ///
    /// `lhs_is_negation`: `lhs` is a prefix-minus expression fresh from
    /// `prefix`. As Haskell's `lexp6` it may only continue with operators
    /// of precedence < 6 or LEFT-associative precedence-6 ones (`-a + b`
    /// groups `(negate a) + b`); a precedence-6 operator with any other
    /// associativity has no defined grouping against it — GHC rejects
    /// `-a <> b`. Cleared after the first operator taken: the negation is
    /// nested inside an `InfixApp` from then on.
    fn continue_infix(
        &mut self,
        mut lhs: Expr,
        mut lhs_is_negation: bool,
        min_prec: u8,
        parent: Option<usize>,
    ) -> Result<(Expr, Option<usize>), Box<Diagnostic>> {
        let mut last = None;
        while self.next_op < self.ops.len() {
            let i = self.next_op;
            let (assoc, prec) = (self.ops[i].assoc, self.ops[i].prec);
            if lhs_is_negation && prec >= 6 && !(prec == 6 && assoc == Assoc::Left) {
                return Err(prefix_minus_lhs_err(&self.ops[i]));
            }
            if !self.should_consume(parent, min_prec, i)? {
                break;
            }
            self.next_op += 1;
            let (_, rp) = binding(assoc, prec);
            let rhs = self.infix(rp, i)?;
            lhs = Expr::InfixApp {
                op: self.ops[i].name.clone(),
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
            lhs_is_negation = false;
            last = Some(i);
        }
        Ok((lhs, last))
    }
}

/// Group the type chain `first (op operand)*`: `F a :+: G b` becomes
/// `(:+:) (F a) (G b)`, each operator named by its type's canonical name.
/// A left-associative operator takes only tighter operators on its right;
/// any other re-enters at its own precedence and nests rightward.
pub fn group_types(first: Type, rest: Vec<(Op, Type)>) -> Type {
    fn climb(
        lhs: Type,
        ops: &mut std::iter::Peekable<std::vec::IntoIter<(Op, Type)>>,
        min_prec: u8,
    ) -> Type {
        let mut lhs = lhs;
        while let Some((op, rhs)) = ops.next_if(|(op, _)| op.prec >= min_prec) {
            let next_min = if op.assoc == Assoc::Left { op.prec + 1 } else { op.prec };
            let rhs = climb(rhs, ops, next_min);
            lhs = Type::App(
                Box::new(Type::App(Box::new(Type::Con(op.name)), Box::new(lhs))),
                Box::new(rhs),
            );
        }
        lhs
    }
    climb(first, &mut rest.into_iter().peekable(), 0)
}

/// The error for two same-precedence operators whose fixities do not
/// allow an unparenthesized chain (the Haskell precedence-parsing rule):
/// either one of them is non-associative, or one is infixl and the other
/// infixr. In both cases the grammar defines no grouping, so the
/// expression is ambiguous and must be parenthesized. Reported at the
/// second operator.
fn fixity_conflict_err(parent: &Op, op2: &Op) -> Box<Diagnostic> {
    let prec = op2.prec;
    let d1 = op_display(&parent.written);
    let d2 = op_display(&op2.written);
    let e1 = op_in_expr(&parent.written);
    let e2 = op_in_expr(&op2.written);
    let msg = if parent.assoc == Assoc::None && op2.assoc == Assoc::None {
        if parent.name == op2.name {
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
    } else if parent.assoc == Assoc::None || op2.assoc == Assoc::None {
        let na = if parent.assoc == Assoc::None { &parent.written } else { &op2.written };
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
            assoc_keyword(op2.assoc)
        )
    };
    let mut diag = Diagnostic::parse_at(msg, op2.span);
    diag.notes.push(format!(
        "parenthesize one side: '(a {e1} b) {e2} c' or 'a {e1} (b {e2} c)'"
    ));
    if is_comparison_op(&parent.written) && is_comparison_op(&op2.written) {
        diag.notes.push(format!(
            "to compare three values, chain with '&&': 'a {e1} b && b {e2} c'"
        ));
    }
    Box::new(diag)
}

/// The error for prefix minus in the right operand of a precedence >= 6
/// operator, matching GHC's rejection (GHC: "cannot mix ... and prefix
/// minus in the same infix expression"). Reported at the minus.
fn prefix_minus_rhs_err(parent: &Op, minus_at: Span) -> Box<Diagnostic> {
    let d = op_display(&parent.written);
    let e = op_in_expr(&parent.written);
    let msg = format!(
        "Prefix minus cannot be the right operand of {d} ({} {}): \
         prefix minus binds like binary '-' (precedence 6), so \
         'a {e} -b' has no defined grouping",
        assoc_keyword(parent.assoc),
        parent.prec
    );
    let mut diag = Diagnostic::parse_at(msg, minus_at);
    diag.notes.push(format!("parenthesize the negation: 'a {e} (-b)'"));
    Box::new(diag)
}

/// The error for an operator that can neither take a prefix-minus
/// expression as its LEFT operand nor be part of its operand: a
/// precedence-6 operator that is not left-associative (`-a <> b`), or —
/// defensively — anything tighter that escaped the operand grouping.
fn prefix_minus_lhs_err(op: &Op) -> Box<Diagnostic> {
    let d = op_display(&op.written);
    let e = op_in_expr(&op.written);
    let msg = format!(
        "Cannot mix prefix minus and {d} ({} {}): prefix minus \
         binds like binary '-' (infixl 6), so '-a {e} b' has no \
         defined grouping",
        assoc_keyword(op.assoc),
        op.prec
    );
    let mut diag = Diagnostic::parse_at(msg, op.span);
    diag.notes.push(format!(
        "parenthesize one side: '(-a) {e} b' or '-(a {e} b)'"
    ));
    Box::new(diag)
}

/// A right section `(op e)` whose operand starts with prefix minus follows
/// the infix rule: legal only under a precedence < 6 operator (GHC rejects
/// `(* -2)` like `a * -2`). `minus_at`: the position of the operand's
/// leading minus, if it has one.
pub fn check_right_section_minus(op: &Op, minus_at: Option<Span>) -> Result<(), Box<Diagnostic>> {
    match minus_at {
        Some(at) if op.prec >= 6 => Err(prefix_minus_rhs_err(op, at)),
        _ => Ok(()),
    }
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
/// exactly as in GHC. `top` is the root of the grouped operand (`None`
/// for an operand that is not an infix expression); the error is reported
/// at the section operator.
pub fn check_section_operand(op: &Op, side: SectionSide, top: Option<&Top>) -> Result<(), Box<Diagnostic>> {
    let direction = match side {
        SectionSide::Left => Assoc::Left,
        SectionSide::Right => Assoc::Right,
    };
    let (arg_assoc, arg_prec, negation, top_op) = match top {
        Some(Top::Op { written, assoc, prec }) => (*assoc, *prec, false, written.clone()),
        Some(Top::Negation) => (Assoc::Left, 6, true, "-".to_string()),
        None => return Ok(()),
    };
    let (op_assoc, op_prec) = (op.assoc, op.prec);
    if op_prec < arg_prec || (op_prec == arg_prec && direction == arg_assoc) {
        return Ok(());
    }

    // Build the schematic pieces of the message from the operators as
    // written: the operand shape, the section as the user wrote it, the
    // expansion it would have to mean, and how the unparenthesized
    // expansion groups (if it groups at all).
    let d = op_display(&op.written);
    let e = op_in_expr(&op.written);
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
    let mut diag = Diagnostic::parse_at(msg, op.span);
    let fix = match direction {
        Assoc::Left => format!("(({operand_src}) {e})"),
        _ => format!("({e} ({operand_src}))"),
    };
    diag.notes.push(format!(
        "parenthesize the operand to get that meaning: '{fix}'"
    ));
    Err(Box::new(diag))
}

/// How an operator is written in prose: symbolic operators quoted, function
/// names in backticks (`div`), matching how they appear at the use site.
pub fn op_display(op: &str) -> String {
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
