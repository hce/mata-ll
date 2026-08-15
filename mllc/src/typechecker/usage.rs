//! Linear-usage checking for `%1` arrows — the enforcement half of mata-ll's
//! linear types. Runs at the end of `check_function`, over the fully
//! substituted typed IR of each clause, so every arrow multiplicity a
//! program pinned down (directly in a signature, or through unification —
//! e.g. a lambda checked against a `%1` parameter) is visible on the types.
//!
//! WHAT IS ENFORCED (GHC-style linearity). A binder is *linear* — it must be
//! consumed EXACTLY once: a second use is an error (double-free class), and
//! zero uses is an error too (the resource leaks) — when it is:
//!   * an argument bound at a `%1` arrow of the function's own type
//!     (including every variable bound inside that argument's pattern),
//!   * an argument bound at a `%m` arrow — a signature MULTIPLICITY VARIABLE
//!     (`Mult::Rigid`, rigid inside its own definition): a caller may
//!     instantiate `m` to `1`, so the body is held to the polymorphic
//!     reading — at most "m" uses, which the 0/1/ω accounting tracks as "one
//!     use, and only in a context that itself consumes at most `%m` times"
//!     (`Count::OneAt`; see MULTIPLICITY POLYMORPHISM below),
//!   * a lambda parameter whose lambda type resolved to a `%1` (or rigid
//!     `%m`) arrow, or
//!   * a *derived* alias of a linear value: a pattern binder of a `case`
//!     whose scrutinee uses a linear variable, a `<-` binder whose action
//!     uses one, or a `let`/`where` value binding whose right-hand side
//!     uses one. A derived binder inherits the source binder's bound (`%1`
//!     or `%m`) at EVERY type — scalars included; only `()`-typed derived
//!     binders are exempt (see SCALARS AND UNIT below).
//!
//! THE EXACTLY-ONCE OBLIGATION (where the lower bound attaches). The usage
//! COUNT (absent / one / ω) is tracked separately from the per-binder
//! POLICY (`Bound` + `check_binder`), and the lower bound is enforced at
//! two kinds of point:
//!   * scope exit — lambda exit, case-branch exit, bind-frame unwind and
//!     clause end all run `check_binder`, and a tracked binder that is
//!     ABSENT from its scope's usage there was consumed zero times: error.
//!     The laziness scaling below makes this compose: a linear variable
//!     consumed only inside a never-used `let`/`where` right-hand side is
//!     scaled to zero and is therefore absent at clause end;
//!   * path joins — the alternatives of a `case`/`if`/guard group run
//!     exactly one of their number, so a tracked binder consumed in SOME
//!     alternatives but not ALL of them leaks on the path through a
//!     non-consuming alternative (`join_alternatives`). This is a lower
//!     bound the per-variable maximum cannot see (max(1, absent) = 1), so
//!     it is checked at the join itself. The same "skipped path" reasoning
//!     rejects consumption in places control flow may bypass: the right
//!     operand of a short-circuiting `&&`/`||`, and the continuation of a
//!     `Maybe` bind (skipped on `Nothing`; the IO/LuaIO/ST binds run their
//!     continuation exactly once and are exempt). Discarding an alias
//!     outright — a wildcard over part of a tainted match, a `_ <-` or `>>`
//!     that drops a non-`()` result built from a linear value, a record
//!     update over a tainted record, or an argument to a local function
//!     with a never-using path — is rejected the same way.
//!
//! THE LAZINESS RULE (what "consumed" means here). mata-ll is lazy, so a
//! syntactic occurrence of `x` is not by itself a consumption — it is a
//! consumption CONTINGENT on the chain of thunks/containers that carry it
//! being consumed in turn (GHC's definition: `e` consumes `x` exactly once
//! iff consuming `e`'s result exactly once consumes `x` exactly once). The
//! pass charges the occurrence where it appears and then enforces every
//! link of the chain separately: a `let`/`where` right-hand side is scaled
//! by its binder's use count (an unused binding is never forced, so it
//! contributes zero — which the absence check then reports); a derived
//! alias must itself be consumed exactly once; and constructs that sever
//! the chain (wildcards, discarded results, skippable continuations) are
//! rejected outright.
//!
//! HOW USES ARE COUNTED (the 0/1/ω lattice). Sequential composition adds
//! usages; the alternatives of a `case`/`if`/guard take the per-variable
//! maximum (a use in *every* branch is still one use, because only one
//! branch runs), plus the lower-bound parity check described above. A use
//! is charged where the variable occurs, scaled by what the surrounding
//! context may do with it:
//!   * an argument position charges by the applied function's arrow: `%1`
//!     charges one use, a plain `->` (or an arrow whose multiplicity nothing
//!     determined) charges ω — an unrestricted function may duplicate it;
//!   * data-constructor fields (including `:` and tuple components) charge
//!     one use: a constructor stores the value exactly once, and any later
//!     duplication of the *container* is charged where the container is
//!     duplicated;
//!   * a `let`/`where` value binding charges its right-hand side scaled by
//!     how often the bound name is used (0/1/ω). This is the laziness rule
//!     in action: a thunk is FORCED at most once (the runtime memoizes, see
//!     codegen's __force), but the value it yields is consumed once per use
//!     of the binder, so the binder's use count — not the forcing count —
//!     is the sound bound. An unused binding charges nothing (never forced,
//!     never consumed) — under exactly-once that zero then trips the
//!     absence check at clause end: a `%1` value parked in a binding that
//!     is never forced is a leak;
//!   * an argument to a local where/let function (including the desugared
//!     `let g x = …`, a lambda-bodied value binding) charges by the
//!     function's INFERRED per-parameter multiplicity: the pass measures how
//!     each parameter is used in the local function's body (to a fixpoint
//!     across a recursive/mutual group, starting from "once") and charges
//!     call arguments accordingly, so a helper that merely forwards a
//!     linear value to a `%1` consumer charges one use, while a duplicating
//!     helper still charges ω. The inference also records whether any
//!     clause/path of the local function can DROP the parameter (an unused
//!     binder, or a wildcard in its pattern); passing a linear value at
//!     such a parameter is rejected — on the dropping path it would never
//!     be consumed;
//!   * a variable captured under a lambda — or under a local where/let
//!     function's body — charges ω: the closure may be called any number of
//!     times. (Only the function's own PARAMETERS get the refined charging
//!     above; captures are deliberately ω, the same promise the closure
//!     rule makes.);
//!   * the continuation of a `>>=`/`>>` in IO, LuaIO, ST or Maybe charges
//!     its body ONCE: those binds run the continuation at most once per run
//!     of the composed action (IO/LuaIO/ST run it exactly once; Maybe skips
//!     it on Nothing — so consuming a linear variable inside a Maybe
//!     continuation is additionally rejected as a skippable path, see
//!     above), and running the composed action more than once requires
//!     using the action value more than once, which is charged at that use.
//!     These four are the monads whose bind implementation is fixed by the
//!     Prelude (a user instance for them is rejected as a duplicate/
//!     orphan). Any other monad's continuation charges ω — a list bind runs
//!     it per element, and a user monad's `>>=` may run it any number of
//!     times;
//!   * a fixed set of Prelude operators (`+`, `==`, `++`, …) charge one use
//!     per operand: they are strict primitives that consume each operand
//!     exactly once and cannot capture, duplicate or drop it. `&&`/`||`
//!     are the exception on the right: the right operand is skipped when
//!     the left already decides the result, so a linear variable there is
//!     rejected (skippable path). (The Prelude cannot be redefined — the
//!     typechecker rejects that separately — so the set is stable.)
//!
//! SCALARS AND UNIT. A *derived* binder of builtin scalar type — Int,
//! Number, Bool, String — gets NO exemption: it is tracked exactly-once
//! like every other derived alias, matching GHC (which has no Movable-style
//! scalar rule in the type system). Duplicating such a scalar would be
//! operationally harmless under the memoizing lazy runtime — the thunk
//! that carries the pending `%1` consumption runs at most once — but an
//! earlier at-least-once relaxation for exactly that reason stopped
//! tracking the scalar once it flowed into unrestricted position, and a
//! pending consumption parked in its never-forced thunk could then be
//! counted as consumed (a leak accepted). Holding scalars to exactly-once
//! closes that laundering hole at the price of also rejecting the harmless
//! duplication (`go + go where go = useOnce t`), a deliberate parity
//! decision. `()`-typed derived binders remain fully exempt — the
//! run-for-effect idiom (`shred t >> …`) discards unit results by design.
//! A binder the user annotates `%1` DIRECTLY (`f :: Int %1 -> …;
//! f n = …`) is enforced as written, exactly once — as it always was.
//!
//! MULTIPLICITY POLYMORPHISM. A signature may quantify over a multiplicity
//! (`apply :: (a %m -> b) -> a %m -> b`). Inside `apply`'s own body `m` is
//! rigid (`Mult::Rigid`) and the accounting is held to every instantiation
//! at once: a binder bound at a `%m` arrow must be used exactly once (a
//! caller may instantiate `m` to `1`, and `1` demands consumption — so
//! dropping a `%m` binder is an error just like dropping a `%1` one), and
//! that one use must be at multiplicity `1` or at the SAME `m` (`f x` is
//! fine: `x` is consumed "m times", and the binder allows "m"). This is the
//! `Count::OneAt`/`Factor::Rigid` extension of the 0/1/ω lattice: a use at a
//! different multiplicity variable, or a second use, may exceed `m = 1`.
//! At every USE of the function, scheme instantiation hands out a fresh
//! flexible variable per quantified `m`, so unification resolves the arrow
//! to `One` for a `%1` argument and the ordinary rules above apply.
//!
//! WHAT IS NOT COVERED (documented boundary; all deviations are in the
//! REJECT direction except where noted):
//!   * `case scalar_expr of 0 -> …; _ -> …` over a TAINTED scalar scrutinee
//!     is rejected (the wildcard rule is blanket), although forcing the
//!     scrutinee to compare literals would in fact consume it. Replace the
//!     `_` with a variable pattern and consume that binder in its branch
//!     (`case n of 0 -> …; m -> … m …`) to branch on a tracked scalar.
//!   * GHC's typing rule for unannotated `let`/`where` charges the
//!     right-hand side at ω, rejecting e.g. `let u = t in useOnce t` even
//!     though `u` is never forced. mata-ll's use-count scaling ACCEPTS such
//!     dead bindings — operationally sound under the memoizing lazy
//!     runtime (the thunk never runs), but more permissive than GHC.
//!   * Guard CONDITIONS are charged sequentially (all conditions up to the
//!     matching one may run) — conservative, and consumption in a guard
//!     condition that can fall through to a later clause is not modeled
//!     across clauses (each clause is checked on its own, as GHC does).
//!   * An OPERATOR whose declared type uses `%1`/`%m` arrows is charged at
//!     `%1` only when BOTH operand arrows are literally `%1`; a rigid `%m`
//!     operand arrow charges ω (reject direction).
//!   * A data constructor field of arrow type keeps its rigid `%m` shared
//!     across all uses of the constructor (constructor schemes do not
//!     quantify multiplicities) — reject direction: uses must agree.
//!   * A record update over a tainted record is rejected outright (it
//!     discards the previous value of the updated fields, which the pass
//!     cannot prove resource-free).
//!   * The Lua side of a `%1` FFI declaration is trusted: mata-ll charges
//!     the argument once per call and cannot see what the host does with
//!     it — including whether it consumes the argument at all.

use super::*;
use crate::types::Mult;

/// A nonzero use count: one use, or "more than one / unbounded" (ω).
/// Zero is represented by absence from the usage map.
///
/// `OneAt(id)` is the multiplicity-polymorphism refinement: one use, but in
/// a context that may itself consume the value up to `%m` times (the rigid
/// multiplicity variable with this id) — e.g. `f x` where `f :: a %m -> b`.
/// Such a use is within bounds only for a binder whose OWN multiplicity is
/// the same `%m` (m uses fit in an "at most m" budget; and m·m = m for both
/// possible instantiations); against a plain `%1` binder it counts as a
/// potential over-use, because `m` may be instantiated to `Many`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Count {
    One,
    OneAt(u32),
    Many,
}

/// Branch join of two nonzero counts (only one alternative runs): `Many`
/// dominates, `One` is the identity, and two `OneAt`s agree only on the
/// same variable — different variables join to `Many`, since no single
/// binder budget covers both.
fn join_count(a: Count, b: Count) -> Count {
    match (a, b) {
        (Count::Many, _) | (_, Count::Many) => Count::Many,
        (Count::One, x) | (x, Count::One) => x,
        (Count::OneAt(x), Count::OneAt(y)) if x == y => Count::OneAt(x),
        _ => Count::Many,
    }
}

/// How often one variable is used, plus — when the count reached `Many`
/// through scaling rather than plain repetition — a plain-language reason.
#[derive(Debug, Clone)]
struct UseInfo {
    count: Count,
    cause: Option<String>,
}

/// Per-variable usage of an expression. Keys are every variable name the
/// expression references; only the linear-tracked ones are ever *checked*,
/// but counting all of them uniformly is what makes `let`-scaling and
/// shadowing work without a second bookkeeping structure.
type Usage = HashMap<String, UseInfo>;

/// Sequential composition: both usages happen.
fn add_usage(acc: &mut Usage, other: Usage) {
    for (k, v) in other {
        match acc.get_mut(&k) {
            None => { acc.insert(k, v); }
            Some(info) => {
                // One + anything nonzero = Many. A plain double occurrence
                // carries no cause — the default message ("used more than
                // once") is the accurate one.
                if info.cause.is_none() {
                    info.cause = v.cause;
                }
                info.count = Count::Many;
            }
        }
    }
}

/// Branch alternatives: only one side runs, so take the per-variable max.
fn join_usage(acc: &mut Usage, other: Usage) {
    for (k, v) in other {
        match acc.get_mut(&k) {
            None => { acc.insert(k, v); }
            Some(info) => {
                let joined = join_count(info.count, v.count);
                if joined != info.count && info.cause.is_none() {
                    info.cause = v.cause;
                }
                info.count = joined;
            }
        }
    }
}

/// Scale a usage by what the surrounding context may do with the value:
/// consume it exactly/at most once (`Once`), up to a rigid signature
/// multiplicity `%m` (`Rigid` — see `Count::OneAt`), or any number of times
/// (`Any`, with a reason attached to every use it inflates).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Factor {
    Once,
    Rigid(u32),
    Any,
}

/// A local function's inferred per-parameter charge: how the body consumes
/// the parameter (`factor`), plus whether some clause or path can DROP it —
/// an unused pattern binder, or a wildcard in the parameter's pattern. A
/// linear argument at a droppable parameter is rejected: on the dropping
/// path it would be consumed zero times. (The factor alone cannot express
/// this — an unused parameter charges `Once` for the upper bound, which is
/// a sound over-charge, and `may_drop` carries the lower-bound side.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParamFactor {
    factor: Factor,
    may_drop: bool,
}

fn scale_usage(u: Usage, factor: Factor, cause: &Option<String>) -> Usage {
    match factor {
        Factor::Once => u,
        Factor::Rigid(id) => u
            .into_iter()
            .map(|(k, mut info)| {
                match info.count {
                    Count::One => info.count = Count::OneAt(id),
                    // m·m = m (for either instantiation of m), so a use
                    // already at this same variable stays within it.
                    Count::OneAt(x) if x == id => {}
                    // Two DIFFERENT multiplicity variables compose to a
                    // product no single binder budget covers.
                    Count::OneAt(_) => {
                        info.count = Count::Many;
                        if info.cause.is_none() {
                            info.cause = Some(
                                "it is consumed under two different signature \
                                 multiplicity variables, and either may be \
                                 instantiated to 'Many'".to_string());
                        }
                    }
                    Count::Many => {}
                }
                (k, info)
            })
            .collect(),
        Factor::Any => u
            .into_iter()
            .map(|(k, mut info)| {
                if !matches!(info.count, Count::Many) {
                    info.count = Count::Many;
                    info.cause = cause.clone();
                }
                (k, info)
            })
            .collect(),
    }
}

/// Prelude operators that consume each operand exactly once: strict
/// primitives over scalars/lists that cannot capture, duplicate or drop an
/// operand. (`.` and `$` are deliberately absent — composition captures its
/// operands in a closure, and `$` is an ordinary unrestricted function.
/// `&&`/`||` are handled separately: they consume the LEFT operand exactly
/// once but may skip the right one, which exactly-once cannot allow for a
/// tracked variable.) The Prelude cannot be redefined (the typechecker
/// rejects interference with it), so matching by name is reliable.
const CONSUME_ONCE_OPS: &[&str] = &[
    "+", "-", "*", "/", "^", "==", "/=", "<", ">", "<=", ">=",
    "<>", "++", "!!", "div", "mod", "rem", "quot",
];

/// Prelude functions that consume their argument exactly once, by the same
/// reasoning as data constructors: `pure`/`return` store the value in an
/// action/container exactly once (duplicating the RESULT is charged where it
/// is duplicated) and `id` passes it through. `fst`/`snd` were on this list
/// under the affine (at-most-once) regime but cannot stay: they drop the
/// other component, and under exactly-once dropping half of a `%1` pair is
/// a leak — they now charge ω through their ordinary (unrestricted) type,
/// which rejects a tracked argument, matching GHC (whose Prelude `fst` is
/// not linear either). Applies only when the name is not shadowed by a
/// local binder (see `UsageCk::locals`); top-level redefinition of a
/// Prelude name is rejected elsewhere, so the Prelude meaning is otherwise
/// guaranteed.
const CONSUME_ONCE_FNS: &[&str] = &["pure", "return", "id"];

/// Which direction a linearity violation went.
enum ViolKind {
    /// Used beyond the bound (more than once, or once at a multiplicity the
    /// binder's own multiplicity does not cover). `Violation::cause` may
    /// explain how the count was inflated.
    Overuse,
    /// Consumed zero times — the value leaks. The string says where the
    /// zero was observed ("this definition never uses it", "a wildcard
    /// discards it", …).
    Unused(String),
    /// Consumed on some evaluation paths but skipped on others — the run
    /// that takes a skipping path consumes it zero times. The string
    /// describes the skipping path.
    PathDrop(String),
}

/// One linearity violation: which binder, why it is restricted, and which
/// direction it was violated in.
struct Violation {
    name: String,
    origin: String,
    /// Only read for `Overuse`: why the context may use it more than once,
    /// when the overuse came from scaling rather than plain repetition.
    cause: Option<String>,
    kind: ViolKind,
}

/// How much use a tracked binder requires and allows: exactly one (`%1`),
/// or exactly one at a rigid signature multiplicity `%m` (`OnceAt` — the
/// one use must be at multiplicity `1` or at that same `m`; see
/// `Count::OneAt`). Tracked alongside the origin message; the usage COUNT
/// is kept separately in `Usage`, so the exactly-once policy attaches at
/// the `check_binder` / join points without re-architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bound {
    Once,
    OnceAt(u32),
}

/// Why a binder is tracked (`origin`, for the diagnostic) and how much use
/// its multiplicity allows.
#[derive(Debug, Clone)]
struct LinearInfo {
    origin: String,
    bound: Bound,
}

/// Does this pattern discard anything through a wildcard? Matching part of
/// a linear value with `_` drops that part unconsumed, which exactly-once
/// rejects (a `LitPat` is fine: it forces the matched scalar to compare it,
/// which IS its consumption).
fn pattern_has_wildcard(p: &TPattern) -> bool {
    match p {
        TPattern::Wildcard => true,
        TPattern::Var(..) | TPattern::LitPat(_) => false,
        TPattern::Paren(inner) => pattern_has_wildcard(inner),
        TPattern::Constructor { args, .. } => args.iter().any(pattern_has_wildcard),
        TPattern::Tuple(elems) => elems.iter().any(pattern_has_wildcard),
    }
}

/// The variables a typed pattern binds, with their types. `direct` is true
/// only for a bare top-level `Var` pattern — the binder that IS the `%1`
/// argument itself (enforced even at `()` type, because the user wrote the
/// annotation on it), as opposed to a binder destructured out of it.
fn pattern_binders(p: &TPattern, direct: bool, out: &mut Vec<(String, Ty, bool)>) {
    match p {
        TPattern::Var(n, ty) => out.push((n.clone(), ty.clone(), direct)),
        TPattern::Wildcard | TPattern::LitPat(_) => {}
        TPattern::Paren(inner) => pattern_binders(inner, direct, out),
        TPattern::Constructor { args, .. } => {
            for a in args {
                pattern_binders(a, false, out);
            }
        }
        TPattern::Tuple(elems) => {
            for e in elems {
                pattern_binders(e, false, out);
            }
        }
    }
}

/// The walker. Borrows the checker read-only (for the operator environment);
/// violations are collected and turned into diagnostics by the driver.
struct UsageCk<'a> {
    ck: &'a Checker,
    /// name -> why this binder is tracked and how much use it allows.
    /// Doubles as the taint set: a scrutinee/action that *uses* any name in
    /// here makes the binders it feeds linear too.
    linear: HashMap<String, LinearInfo>,
    /// Names currently bound by a LOCAL binder (nesting count). Guards the
    /// `CONSUME_ONCE_FNS` whitelist: a local binding named `pure` is not the
    /// Prelude's `pure`.
    locals: HashMap<String, u32>,
    /// Per-parameter charge factors of in-scope local where/let FUNCTIONS,
    /// inferred from their bodies (see the module comment): name -> a stack
    /// (one entry per nested binder shadowing the name; `None` for a
    /// non-function binder, so an inner value binding named like an outer
    /// local function blocks the outer's factors). Consulted by
    /// `app_arg_factor` when the applied head is a local function.
    fn_params: HashMap<String, Vec<Option<Vec<ParamFactor>>>>,
    viols: Vec<Violation>,
}

/// Saved shadowing state for a scope's binders, restored on exit.
type SavedBinders = Vec<(String, Option<LinearInfo>)>;

/// Bookkeeping for one `let`/`where` binding group, split into enter/exit
/// halves so the iterative bind-chain walker can interleave group scopes
/// with its spine frames.
struct GroupCtx {
    saved: SavedBinders,
    rhs_usages: Vec<Usage>,
}

impl<'a> UsageCk<'a> {
    /// Shadow `name` for an inner scope (returns the outer entry to restore).
    /// Every scope entry also pushes a `None` frame onto the local-function
    /// factor stack, so an inner binder of any kind blocks an outer local
    /// function's inferred factors; `group_enter` overwrites the frame for
    /// the names that ARE local functions.
    fn shadow(&mut self, name: &str) -> (String, Option<LinearInfo>) {
        *self.locals.entry(name.to_string()).or_insert(0) += 1;
        self.fn_params.entry(name.to_string()).or_default().push(None);
        (name.to_string(), self.linear.remove(name))
    }

    fn restore(&mut self, saved: SavedBinders) {
        for (name, old) in saved {
            if let Some(c) = self.locals.get_mut(&name) {
                *c = c.saturating_sub(1);
            }
            if let Some(stack) = self.fn_params.get_mut(&name) {
                stack.pop();
            }
            match old {
                Some(info) => { self.linear.insert(name, info); }
                None => { self.linear.remove(&name); }
            }
        }
    }

    /// Is `name` currently bound by a local binder (as opposed to naming a
    /// top-level / Prelude definition)?
    fn is_local(&self, name: &str) -> bool {
        self.locals.get(name).is_some_and(|c| *c > 0)
    }

    /// The inferred per-parameter charge factors of the local function
    /// `name`, if the innermost binder of that name is one.
    fn local_fn_factors(&self, name: &str) -> Option<&Vec<ParamFactor>> {
        self.fn_params.get(name)?.last()?.as_ref()
    }

    /// Record a violation if `u` shows the tracked binder `name` violating
    /// its bound in either direction: consumed beyond the bound (more than
    /// once, or once in a context whose multiplicity the binder's own
    /// multiplicity does not cover), or — the exactly-once lower bound —
    /// not consumed at all. Runs at every scope exit: lambda exit,
    /// case-branch exit, bind-frame unwind and clause end.
    fn check_binder(&mut self, u: &Usage, name: &str) {
        let Some(ai) = self.linear.get(name) else { return };
        let Some(info) = u.get(name) else {
            // Absent from the scope's usage: consumed zero times. Every
            // bound includes the at-least-once lower half, so this is a
            // leak regardless of the binder's flavor.
            self.viols.push(Violation {
                name: name.to_string(),
                origin: ai.origin.clone(),
                cause: None,
                kind: ViolKind::Unused(
                    "nothing on this evaluation path consumes it".to_string()),
            });
            return;
        };
        let exceeded = match (info.count, ai.bound) {
            (Count::Many, _) => true,
            (Count::One, _) => false,
            // One use through a `%m` arrow: within bounds only when the
            // binder's own multiplicity is that same `m`.
            (Count::OneAt(x), Bound::OnceAt(m)) => x != m,
            (Count::OneAt(_), Bound::Once) => true,
        };
        if exceeded {
            let cause = info.cause.clone().or_else(|| match info.count {
                Count::OneAt(_) => Some(match ai.bound {
                    Bound::Once =>
                        "it is passed to a function whose arrow multiplicity \
                         is a signature variable ('%m'), which a caller may \
                         instantiate to 'Many' — so that function may use \
                         the value more than once".to_string(),
                    Bound::OnceAt(_) =>
                        "it is passed to a function whose arrow multiplicity \
                         is a DIFFERENT signature variable than the one that \
                         limits this binder, and either variable may be \
                         instantiated to 'Many'".to_string(),
                }),
                _ => None,
            });
            self.viols.push(Violation {
                name: name.to_string(),
                origin: ai.origin.clone(),
                cause,
                kind: ViolKind::Overuse,
            });
        }
    }

    /// Join the usages of a set of ALTERNATIVES (case branches, the two
    /// arms of an `if`, guard bodies): exactly one of them runs, so the
    /// per-variable counts join (max) — and, for exactly-once, a tracked
    /// binder consumed in one alternative must be consumed in EVERY
    /// alternative, because a run that takes a non-consuming alternative
    /// consumes it zero times (a leak the max cannot see). `path` names the
    /// construct for the diagnostic.
    fn join_alternatives(&mut self, alts: Vec<Usage>, path: &str) -> Usage {
        if alts.len() > 1 {
            let mut tracked: Vec<(String, LinearInfo)> = self
                .linear
                .iter()
                .map(|(k, ai)| (k.clone(), ai.clone()))
                .collect();
            // Deterministic order for stable diagnostics.
            tracked.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, ai) in tracked {
                let n_in = alts.iter().filter(|u| u.contains_key(&name)).count();
                if n_in > 0 && n_in < alts.len() {
                    self.viols.push(Violation {
                        name,
                        origin: ai.origin,
                        cause: None,
                        kind: ViolKind::PathDrop(format!(
                            "it is consumed in only {} of the {} {}, and \
                             exactly one of them runs — a run through one of \
                             the others consumes it zero times",
                            n_in, alts.len(), path)),
                    });
                }
            }
        }
        let mut it = alts.into_iter();
        let mut acc = it.next().unwrap_or_default();
        for u in it {
            join_usage(&mut acc, u);
        }
        acc
    }

    /// Reject tracked variables consumed inside a context that control flow
    /// can skip entirely (the right operand of `&&`/`||`, the continuation
    /// of a Maybe bind): on the skipping path they are consumed zero times.
    fn flag_skippable(&mut self, u: &Usage, path: &str) {
        let mut names: Vec<(String, LinearInfo)> = u
            .keys()
            .filter_map(|k| self.linear.get(k).map(|ai| (k.clone(), ai.clone())))
            .collect();
        names.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, ai) in names {
            self.viols.push(Violation {
                name,
                origin: ai.origin,
                cause: None,
                kind: ViolKind::PathDrop(path.to_string()),
            });
        }
    }

    /// The result of an action that consumed the tracked variable `src` is
    /// discarded (`>>`, or a `_ <-` bind) even though its type is not `()`:
    /// the pending consumption may live inside that never-forced result
    /// (e.g. `pure (useOnce t) >> …`), so the drop is a potential leak.
    fn flag_discarded_result(&mut self, src: &str) {
        let Some(ai) = self.linear.get(src).cloned() else { return };
        self.viols.push(Violation {
            name: src.to_string(),
            origin: ai.origin,
            cause: None,
            kind: ViolKind::PathDrop(
                "the result of an action that consumes it is discarded (a \
                 '>>' statement, or a '_ <-' bind) even though the result \
                 type is not '()' — the consumption may still be pending \
                 inside that never-used result. Bind the result to a name \
                 and use it, or make the action return '()'".to_string()),
        });
    }

    /// Does this usage consume any linear-tracked variable? Returns the
    /// name whose bound is STRICTEST, and that bound (for the taint-origin
    /// message and the inherited restriction): a derived binder must
    /// inherit the strongest obligation among everything the expression
    /// consumed — a plain `%1` (`Once`) over a rigid `%m` (`OnceAt`), whose
    /// one use is additionally pinned to that same variable. Alphabetical
    /// within a strictness class for stable diagnostics.
    fn taint_source(&self, u: &Usage) -> Option<(String, Bound)> {
        fn rank(b: Bound) -> u8 {
            match b {
                Bound::Once => 0,
                Bound::OnceAt(_) => 1,
            }
        }
        let mut names: Vec<(&String, Bound)> = u
            .keys()
            .filter_map(|k| self.linear.get(k).map(|ai| (k, ai.bound)))
            .collect();
        names.sort_by(|a, b| rank(a.1).cmp(&rank(b.1)).then(a.0.cmp(b.0)));
        names.first().map(|(s, bound)| ((*s).to_string(), *bound))
    }

    // --- Expression usage ---

    /// Usage of an expression. Bind chains (`>>=`/`>>` spines from
    /// do-blocks) are dispatched to the iterative walker — they are the one
    /// TIR shape whose depth is NOT bounded by the expression-nesting limit,
    /// exactly as in `TExpr::apply_subst`.
    fn expr_usage(&mut self, e: &TExpr) -> Usage {
        match &e.kind {
            TExprKind::InfixApp { op, .. } if op == ">>=" || op == ">>" => {
                self.bind_chain_usage(e)
            }
            _ => self.expr_usage_node(e),
        }
    }

    fn expr_usage_node(&mut self, e: &TExpr) -> Usage {
        match &e.kind {
            TExprKind::Var(n) => {
                let mut u = Usage::new();
                u.insert(n.clone(), UseInfo { count: Count::One, cause: None });
                u
            }
            TExprKind::Con(_) | TExprKind::Lit(_) | TExprKind::OpFunc(_)
            | TExprKind::DictAccess { .. } => Usage::new(),

            TExprKind::App(f, a) => {
                let mut u = self.expr_usage(f);
                let ua = self.expr_usage(a);
                let (factor, cause, drop_fn) = self.app_arg_factor(f);
                if let Some(fn_name) = drop_fn {
                    self.flag_skippable(&ua, &format!(
                        "it is passed to the local function '{}', which has \
                         a clause or path that never uses this parameter — a \
                         call taking that path consumes the value zero times",
                        fn_name));
                }
                add_usage(&mut u, scale_usage(ua, factor, &cause));
                u
            }

            TExprKind::InfixApp { op, lhs, rhs } => {
                // `>>=`/`>>` never reach here (dispatched in expr_usage);
                // `:` is the list constructor — each side is stored exactly
                // once, like any constructor field.
                if op == ":" {
                    let mut u = self.expr_usage(lhs);
                    add_usage(&mut u, self.expr_usage(rhs));
                    return u;
                }
                // Short-circuit operators consume the LEFT operand exactly
                // once, but skip the right one whenever the left already
                // decides the result — a linear variable there would be
                // consumed zero times on the skipping path.
                if op == "&&" || op == "||" {
                    let mut u = self.expr_usage(lhs);
                    let ur = self.expr_usage(rhs);
                    self.flag_skippable(&ur, &format!(
                        "it is consumed in the right operand of '{}', which \
                         short-circuits: when the left operand already \
                         decides the result, the right operand never runs \
                         and the value is consumed zero times", op));
                    add_usage(&mut u, ur);
                    return u;
                }
                let (factor, cause) = self.op_operand_factor(op);
                let mut u = scale_usage(self.expr_usage(lhs), factor, &cause);
                add_usage(&mut u, scale_usage(self.expr_usage(rhs), factor, &cause));
                u
            }

            // Negation consumes its operand exactly once and returns a new
            // number — it cannot capture or duplicate.
            TExprKind::Negate(inner) => self.expr_usage(inner),

            TExprKind::Lambda { params, body } => {
                let (u, _) = self.lambda_usage(params, &e.ty, body);
                // Whatever the lambda captured may be consumed once per
                // CALL, and nothing bounds how often a function value is
                // called.
                scale_usage(u, Factor::Any, &Some(
                    "it is captured by a function value, which may be called \
                     any number of times".to_string()))
            }

            TExprKind::If { cond, then_branch, else_branch } => {
                let mut u = self.expr_usage(cond);
                let alts = vec![
                    self.expr_usage(then_branch),
                    self.expr_usage(else_branch),
                ];
                let branches = self.join_alternatives(alts, "branches of this 'if'");
                add_usage(&mut u, branches);
                u
            }

            TExprKind::Case { scrutinee, branches } => {
                let u_s = self.expr_usage(scrutinee);
                // Taint: pattern binders of a match on a linear value alias
                // parts of that value, so they inherit the restriction at
                // every type — only unit binders are exempt (see SCALARS
                // AND UNIT in the module comment).
                let taint_src = self.taint_source(&u_s);
                let mut alts: Vec<Usage> = Vec::with_capacity(branches.len());
                for br in branches {
                    if let Some((src, _)) = &taint_src
                        && pattern_has_wildcard(&br.pattern)
                    {
                        // A wildcard over (part of) a linear value drops
                        // whatever it matches without consuming it.
                        let ai = self.linear.get(src).cloned();
                        if let Some(ai) = ai {
                            self.viols.push(Violation {
                                name: src.clone(),
                                origin: ai.origin,
                                cause: None,
                                kind: ViolKind::PathDrop(
                                    "part (or all) of its value is matched by \
                                     a wildcard ('_') in this case pattern, \
                                     which discards it without consuming it"
                                        .to_string()),
                            });
                        }
                    }
                    let mut binders = Vec::new();
                    pattern_binders(&br.pattern, true, &mut binders);
                    let mut saved: SavedBinders = Vec::new();
                    let mut checked: Vec<String> = Vec::new();
                    for (name, ty, _) in &binders {
                        saved.push(self.shadow(name));
                        if let Some((src, bound)) = &taint_src
                            && !matches!(ty, Ty::Unit)
                        {
                            self.linear.insert(name.clone(), LinearInfo {
                                origin: format!(
                                    "it is pattern-bound from '{}', which \
                                     must itself be consumed exactly \
                                     once", src),
                                bound: *bound,
                            });
                            checked.push(name.clone());
                        }
                    }
                    let mut bu = self.branch_usage(&br.guards, &br.body);
                    for name in &checked {
                        self.check_binder(&bu, name);
                    }
                    for (name, _, _) in &binders {
                        bu.remove(name);
                    }
                    self.restore(saved);
                    alts.push(bu);
                }
                let joined = self.join_alternatives(alts, "alternatives of this 'case'");
                let mut u = u_s;
                add_usage(&mut u, joined);
                u
            }

            TExprKind::Let { binds, body } => {
                let gctx = self.group_enter(binds);
                let u_body = self.expr_usage(body);
                self.group_exit(binds, gctx, u_body)
            }

            TExprKind::Paren(inner) => self.expr_usage(inner),

            // Components of a tuple are stored exactly once, like
            // constructor fields; duplicating the tuple is charged wherever
            // the tuple itself is duplicated.
            TExprKind::Tuple(elems) => {
                let mut u = Usage::new();
                for el in elems {
                    add_usage(&mut u, self.expr_usage(el));
                }
                u
            }

            // An FFI call consumes each argument exactly once per call (what
            // the Lua host then does with it is the host's business — a `%1`
            // FFI signature is the user's assertion about that host
            // function).
            TExprKind::SpecCall { args, .. } => {
                let mut u = Usage::new();
                for a in args {
                    add_usage(&mut u, self.expr_usage(a));
                }
                u
            }
            TExprKind::OutgoingCallback { callee, .. } => self.expr_usage(callee),
            TExprKind::FfiMaybeArg { value } => self.expr_usage(value),

            // A record update reads the old record once and stores each new
            // field value once — but it also DISCARDS the record's previous
            // value for each updated field, which for a record built from a
            // linear value may drop a resource unconsumed. Rejected when
            // tainted (conservative; the pass cannot see the field types of
            // the dropped values).
            TExprKind::RecordUpdate { record, updates, .. } => {
                let mut u = self.expr_usage(record);
                self.flag_skippable(&u,
                    "it is consumed through a record update, which discards \
                     the record's previous value for every updated field — \
                     any part of the value stored there is dropped without \
                     being consumed");
                for (_, _, val) in updates {
                    add_usage(&mut u, self.expr_usage(val));
                }
                u
            }

            // Dictionary-passing forms are introduced by the monomorphizer,
            // after this pass; treated conservatively if ever encountered.
            TExprKind::DictMethod { dict, .. } => {
                let cause = Some("it is consumed through a typeclass \
                                  dictionary".to_string());
                scale_usage(self.expr_usage(dict), Factor::Any, &cause)
            }
            TExprKind::DictCall { dict_args, value_args, .. } => {
                let cause = Some("it is passed to a dictionary-passing \
                                  function".to_string());
                let mut u = Usage::new();
                for a in dict_args.iter().chain(value_args) {
                    let ua = self.expr_usage(a);
                    add_usage(&mut u, scale_usage(ua, Factor::Any, &cause));
                }
                u
            }
        }
    }

    /// Walk a lambda's parameters and body. Each parameter's multiplicity
    /// comes from the lambda's own (post-substitution) arrow chain: a lambda
    /// checked against a `%1` (or rigid `%m`) parameter carries that
    /// multiplicity here, and its binder is tracked and checked accordingly.
    /// Returns the body's usage with the parameters REMOVED and UNSCALED —
    /// the caller applies the capture rule (`Factor::Any` for a lambda
    /// value) — plus the per-parameter charge factors the body implies,
    /// which `group_enter` uses to infer a lambda-bodied local function's
    /// parameter multiplicities.
    fn lambda_usage(
        &mut self,
        params: &[(String, Ty)],
        lam_ty: &Ty,
        body: &TExpr,
    ) -> (Usage, Vec<ParamFactor>) {
        let mut cur = lam_ty;
        let mut saved: SavedBinders = Vec::new();
        let mut checked: Vec<String> = Vec::new();
        for (name, _) in params {
            let m = if let Ty::Arrow(_, rest, m) = cur {
                let m = *m;
                cur = rest.as_ref();
                m
            } else {
                Mult::Many
            };
            if name == "_" && matches!(m, Mult::One | Mult::Rigid(_)) {
                // `\_ -> …` at a `%1` (or rigid `%m`) arrow: the parameter
                // is discarded outright, so the value is never consumed.
                self.viols.push(Violation {
                    name: "_".to_string(),
                    origin: "the function that binds it has a '%1' (or \
                             multiplicity-variable '%m') arrow at this \
                             parameter".to_string(),
                    cause: None,
                    kind: ViolKind::Unused(
                        "the parameter is a wildcard ('_'), which discards \
                         the value".to_string()),
                });
            }
            if name != "_" {
                saved.push(self.shadow(name));
                match m {
                    Mult::One => {
                        self.linear.insert(name.clone(), LinearInfo {
                            origin: "the function that binds it has a '%1' \
                                     arrow at this parameter".to_string(),
                            bound: Bound::Once,
                        });
                        checked.push(name.clone());
                    }
                    // A rigid `%m` arrow: a caller may instantiate m to 1,
                    // so the binder is held to the polymorphic reading (at
                    // most one use, at multiplicity 1 or at this same m).
                    Mult::Rigid(id) => {
                        self.linear.insert(name.clone(), LinearInfo {
                            origin: "the function that binds it has a '%m' \
                                     (multiplicity-variable) arrow at this \
                                     parameter, and a caller may instantiate \
                                     that variable to '%1'".to_string(),
                            bound: Bound::OnceAt(id),
                        });
                        checked.push(name.clone());
                    }
                    _ => {}
                }
            }
        }
        let mut u = self.expr_usage(body);
        for name in &checked {
            self.check_binder(&u, name);
        }
        let factors: Vec<ParamFactor> = params.iter().map(|(name, _)| {
            if name == "_" {
                // A wildcard parameter drops its argument (charging `Once`
                // is the sound upper bound; `may_drop` carries the leak).
                ParamFactor { factor: Factor::Once, may_drop: true }
            } else {
                Self::param_factor(u.get(name).map(|i| i.count))
            }
        }).collect();
        for (name, _) in &saved {
            u.remove(name);
        }
        self.restore(saved);
        (u, factors)
    }

    /// Usage of one guarded body: every guard condition may run (charged
    /// sequentially — conservative for guards after the one that matches),
    /// while the guard bodies are alternatives (joined, with the
    /// exactly-once parity check). When guards are present the body is
    /// structurally absent (`None`) — the guard chain IS the body.
    fn branch_usage(&mut self, guards: &[TGuard], body: &Option<TExpr>) -> Usage {
        if guards.is_empty() {
            return self.expr_usage(body.as_ref().expect("guard-free branch carries a body"));
        }
        let mut u = Usage::new();
        let mut bodies: Vec<Usage> = Vec::with_capacity(guards.len());
        for g in guards {
            add_usage(&mut u, self.expr_usage(&g.condition));
            bodies.push(self.expr_usage(&g.body));
        }
        let joined = self.join_alternatives(bodies, "guard alternatives");
        add_usage(&mut u, joined);
        u
    }

    /// How an application `f x` charges the uses inside `x`. The third
    /// component is `Some(local_fn_name)` when the applied head is a local
    /// function whose matching parameter has a dropping path (see
    /// `ParamFactor::may_drop`) — the caller flags any tracked variable in
    /// the argument, because on that path it would never be consumed.
    fn app_arg_factor(&self, f: &TExpr) -> (Factor, Option<String>, Option<String>) {
        // Constructor applications (any depth of the spine) store each field
        // exactly once. While peeling, count the spine depth: it is the
        // index of the argument this application supplies, which selects the
        // matching inferred parameter factor of a local function.
        let mut head = f;
        let mut arg_idx: usize = 0;
        loop {
            match &head.kind {
                TExprKind::Paren(inner) => head = inner,
                TExprKind::App(g, _) => {
                    head = g;
                    arg_idx += 1;
                }
                _ => break,
            }
        }
        if matches!(head.kind, TExprKind::Con(_)) {
            return (Factor::Once, None, None);
        }
        if let TExprKind::Var(n) = &head.kind
            && CONSUME_ONCE_FNS.contains(&n.as_str())
            && !self.is_local(n)
        {
            return (Factor::Once, None, None);
        }
        match &f.ty {
            Ty::Arrow(_, _, Mult::One) => (Factor::Once, None, None),
            // A rigid `%m` arrow (the enclosing signature's multiplicity
            // variable): the argument is consumed "up to m times" — within
            // bounds for a binder limited by the same m (see Count::OneAt).
            Ty::Arrow(_, _, Mult::Rigid(id)) => (Factor::Rigid(*id), None, None),
            _ => {
                // A local where/let function: charge by its inferred
                // per-parameter multiplicity (module comment) rather than
                // its arrow type, which carries no `%1` a user could write.
                if let TExprKind::Var(n) = &head.kind
                    && let Some(factors) = self.local_fn_factors(n)
                    && arg_idx < factors.len()
                {
                    let pf = factors[arg_idx];
                    let drop_fn = if pf.may_drop { Some(n.clone()) } else { None };
                    return match pf.factor {
                        Factor::Once => (Factor::Once, None, drop_fn),
                        Factor::Rigid(id) => (Factor::Rigid(id), None, drop_fn),
                        Factor::Any => (Factor::Any, Some(format!(
                            "it is passed to the local function '{}', whose \
                             body may use this parameter more than once (or \
                             lets it escape into something that may)", n)), drop_fn),
                    };
                }
                let callee = match &head.kind {
                    TExprKind::Var(n) => Some(n.clone()),
                    TExprKind::OpFunc(n) => Some(format!("({})", n)),
                    _ => None,
                };
                let cause = match callee {
                    Some(n) => format!(
                        "it is passed to '{}', whose type does not promise to \
                         consume this argument exactly once (the arrow is \
                         '->', not '%1 ->')", n),
                    None => "it is passed to a function whose type does not \
                             promise to consume this argument exactly once \
                             (the arrow is '->', not '%1 ->')".to_string(),
                };
                (Factor::Any, Some(cause), None)
            }
        }
    }

    /// How an infix operator charges each operand. The fixed Prelude
    /// primitives consume each operand exactly once; anything else charges
    /// by the operator's declared arrow multiplicities (conservatively ω).
    fn op_operand_factor(&self, op: &str) -> (Factor, Option<String>) {
        if CONSUME_ONCE_OPS.contains(&op) {
            return (Factor::Once, None);
        }
        if let Some(scheme) = self.ck.env.lookup(op)
            && let Ty::Arrow(_, rest, m1) = &scheme.ty {
                let one_lhs = matches!(m1, Mult::One);
                let one_rhs = matches!(rest.as_ref(), Ty::Arrow(_, _, Mult::One));
                if one_lhs && one_rhs {
                    return (Factor::Once, None);
                }
            }
        (Factor::Any, Some(format!(
            "it is passed to the operator '{}', whose type does not promise \
             to consume it exactly once", op)))
    }

    // --- let/where binding groups ---

    /// Join two per-parameter charge factors (clause alternatives / fixpoint
    /// growth): `Any` dominates, `Once` is the identity, and two rigid
    /// multiplicity variables agree only when they are the same one.
    fn join_factor(a: Factor, b: Factor) -> Factor {
        match (a, b) {
            (Factor::Any, _) | (_, Factor::Any) => Factor::Any,
            (Factor::Once, x) | (x, Factor::Once) => x,
            (Factor::Rigid(x), Factor::Rigid(y)) if x == y => Factor::Rigid(x),
            _ => Factor::Any,
        }
    }

    /// Join two per-parameter charges: the factors join, and a parameter
    /// that may be dropped on EITHER side may be dropped.
    fn join_param_factor(a: ParamFactor, b: ParamFactor) -> ParamFactor {
        ParamFactor {
            factor: Self::join_factor(a.factor, b.factor),
            may_drop: a.may_drop || b.may_drop,
        }
    }

    /// The charge a parameter's body usage implies for a call argument:
    /// used once → the argument is consumed at most once; used once through
    /// a `%m` arrow → consumed "up to m times"; used repeatedly →
    /// unbounded. UNUSED charges `Once` (a sound over-charge for the upper
    /// bound) but records the dropping path — a linear argument at that
    /// parameter is never consumed on it.
    fn param_factor(c: Option<Count>) -> ParamFactor {
        match c {
            None => ParamFactor { factor: Factor::Once, may_drop: true },
            Some(Count::One) => ParamFactor { factor: Factor::Once, may_drop: false },
            Some(Count::OneAt(id)) => ParamFactor { factor: Factor::Rigid(id), may_drop: false },
            Some(Count::Many) => ParamFactor { factor: Factor::Any, may_drop: false },
        }
    }

    /// Publish `factors` as the inferred parameter multiplicities of the
    /// local function `name` (overwriting the innermost scope frame that
    /// `shadow` pushed for it).
    fn set_local_fn_factors(&mut self, name: &str, factors: Vec<ParamFactor>) {
        if let Some(stack) = self.fn_params.get_mut(name)
            && let Some(top) = stack.last_mut() {
                *top = Some(factors);
            }
    }

    /// Enter a (mutually recursive) binding group: shadow the bound names,
    /// walk every right-hand side, and mark the names whose value was built
    /// from a linear variable as linear themselves (taint), iterating to a
    /// fixpoint so sibling/recursive references are seen. Only `()`-typed
    /// bindings are exempt from the taint, like unit pattern binders.
    ///
    /// The same fixpoint infers each local FUNCTION's per-parameter charge
    /// factors (see the module comment): factors start at the optimistic
    /// `Once`, each walk recomputes them from the body's usage of the
    /// parameters (calls to siblings/self consult the current factors
    /// through `app_arg_factor`), and both the taint set and the factors
    /// only grow — the factor lattice has height 3 and the taint set is
    /// bounded by the group, so this terminates. Starting optimistic and
    /// taking the least fixpoint is sound: any finite evaluation's call
    /// tree charges each argument through the factor at its call site, and
    /// the fixpoint equations include every such site.
    fn group_enter(&mut self, binds: &[TLocalDef]) -> GroupCtx {
        let saved: SavedBinders =
            binds.iter().map(|b| self.shadow(&b.name)).collect();
        if binds.is_empty() {
            return GroupCtx { saved, rhs_usages: Vec::new() };
        }
        // Optimistic initial factors. A name bound MORE THAN ONCE in one
        // group is a multi-clause where-function: its clauses are
        // alternatives, so its factors are the join over the same-name
        // bindings (clause arity is uniform; a mismatch degrades to Any).
        const OPTIMISTIC: ParamFactor = ParamFactor { factor: Factor::Once, may_drop: false };
        const DEGRADED: ParamFactor = ParamFactor { factor: Factor::Any, may_drop: true };
        let mut factors: HashMap<String, Vec<ParamFactor>> = HashMap::new();
        for b in binds {
            let Some(arity) = Self::local_fn_arity(b) else { continue };
            match factors.get_mut(&b.name) {
                Some(existing) => {
                    if existing.len() != arity {
                        let n = existing.len().min(arity);
                        *existing = vec![DEGRADED; n];
                    }
                }
                None => {
                    factors.insert(b.name.clone(), vec![OPTIMISTIC; arity]);
                }
            }
        }
        for (name, f) in &factors {
            self.set_local_fn_factors(name, f.clone());
        }
        // Taint + factor fixpoint; each re-walk discards the previous
        // iteration's violations so the final walk reports each at most once.
        let mut tainted: HashMap<String, Bound> = HashMap::new();
        let mut rhs_usages: Vec<Usage>;
        loop {
            let viols_before = self.viols.len();
            let mut param_charges: Vec<Option<Vec<ParamFactor>>> = Vec::with_capacity(binds.len());
            rhs_usages = binds.iter().map(|b| {
                let (u, pc) = self.binding_rhs_usage(b);
                param_charges.push(pc);
                u
            }).collect();
            let mut changed = false;
            // Grow the factors (join with the previous iteration's, so they
            // are monotone even if a recomputation could locally shrink).
            for (b, pc) in binds.iter().zip(&param_charges) {
                let Some(pc) = pc else { continue };
                match factors.get_mut(&b.name) {
                    None => {
                        factors.insert(b.name.clone(), pc.clone());
                        changed = true;
                    }
                    Some(entry) => {
                        if entry.len() > pc.len() {
                            // Arity disagreement (degenerate); degrade to ω.
                            *entry = vec![DEGRADED; pc.len()];
                            changed = true;
                        }
                        for (slot, new) in entry.iter_mut().zip(pc) {
                            let joined = Self::join_param_factor(*slot, *new);
                            if joined != *slot {
                                *slot = joined;
                                changed = true;
                            }
                        }
                    }
                }
            }
            for (name, f) in &factors {
                self.set_local_fn_factors(name, f.clone());
            }
            // Grow the taint set.
            for (b, u) in binds.iter().zip(&rhs_usages) {
                if b.patterns.is_empty()
                    && !matches!(b.body.ty, Ty::Unit)
                    && !tainted.contains_key(&b.name)
                    && let Some((_, bound)) = self.taint_source(u)
                {
                    tainted.insert(b.name.clone(), bound);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
            self.viols.truncate(viols_before);
            for (name, bound) in &tainted {
                self.linear.entry(name.clone()).or_insert_with(|| LinearInfo {
                    origin: "it holds a value built from a variable that must \
                             be consumed exactly once".to_string(),
                    bound: *bound,
                });
            }
        }
        GroupCtx { saved, rhs_usages }
    }

    /// The parameter count of a local binding that is a FUNCTION: explicit
    /// patterns (`where go x y = …`), or the parameters of a lambda-bodied
    /// value binding (`let g x = …` desugars to `g = \x -> …`).
    fn local_fn_arity(b: &TLocalDef) -> Option<usize> {
        if !b.patterns.is_empty() {
            return Some(b.patterns.len());
        }
        let mut body = &b.body;
        while let TExprKind::Paren(inner) = &body.kind {
            body = inner;
        }
        if let TExprKind::Lambda { params, .. } = &body.kind {
            Some(params.len())
        } else {
            None
        }
    }

    /// Usage of one binding's right-hand side. A local *function*'s
    /// parameters shadow within its body; for a function the per-parameter
    /// charge factors implied by the body (worst binder of each parameter
    /// pattern, plus whether the pattern can drop part of the argument) are
    /// returned alongside.
    fn binding_rhs_usage(&mut self, b: &TLocalDef) -> (Usage, Option<Vec<ParamFactor>>) {
        if b.patterns.is_empty() {
            // A lambda-bodied value binding is a local function in disguise
            // (`let g x = e` desugars to `g = \x -> e`): infer its parameter
            // factors like a where-function's. The captures still charge ω
            // here (the lambda capture rule), so the value-binding scaling
            // in `group_exit` cannot undo it.
            let mut body = &b.body;
            while let TExprKind::Paren(inner) = &body.kind {
                body = inner;
            }
            if let TExprKind::Lambda { params, body: lbody } = &body.kind {
                let (u, factors) = self.lambda_usage(params, &body.ty, lbody);
                let u = scale_usage(u, Factor::Any, &Some(
                    "it is captured by a function value, which may be called \
                     any number of times".to_string()));
                return (u, Some(factors));
            }
            return (self.expr_usage(&b.body), None);
        }
        let mut per_param: Vec<Vec<String>> = Vec::with_capacity(b.patterns.len());
        let mut binders = Vec::new();
        for p in &b.patterns {
            let start = binders.len();
            pattern_binders(p, true, &mut binders);
            per_param.push(binders[start..].iter().map(|(n, _, _)| n.clone()).collect());
        }
        let saved: SavedBinders = binders.iter().map(|(n, _, _)| self.shadow(n)).collect();
        let mut u = self.expr_usage(&b.body);
        // Each parameter's factor covers every binder its pattern binds: two
        // pattern components each used once is one consumption of the
        // argument (they alias disjoint parts), so the join — not the sum —
        // is the right combination. A wildcard in the pattern, or an unused
        // binder, marks the parameter droppable (part of the argument is
        // never consumed on this clause).
        let param_factors: Vec<ParamFactor> = b.patterns.iter().zip(&per_param)
            .map(|(p, names)| {
                let base = ParamFactor {
                    factor: Factor::Once,
                    may_drop: pattern_has_wildcard(p),
                };
                names.iter().fold(base, |acc, n| {
                    Self::join_param_factor(
                        acc, Self::param_factor(u.get(n).map(|i| i.count)))
                })
            }).collect();
        for (n, _, _) in &binders {
            u.remove(n);
        }
        self.restore(saved);
        (u, Some(param_factors))
    }

    /// Leave a binding group: charge each right-hand side scaled by how the
    /// bound name is used (the laziness rule — see the module comment), then
    /// unshadow. A local FUNCTION's captures always charge ω (same rule as a
    /// lambda's captures — a function value may be called any number of
    /// times); the per-parameter refinement happens at call sites instead.
    fn group_exit(&mut self, binds: &[TLocalDef], gctx: GroupCtx, u_body: Usage) -> Usage {
        let GroupCtx { saved, rhs_usages } = gctx;
        let mut result = u_body;
        // Names referenced by ANY right-hand side in the group — recursion or
        // sibling use. A binding reached that way may be evaluated on behalf
        // of another binding an unbounded number of times relative to this
        // analysis, so its charge is conservatively ω. Computed before the
        // group's names are stripped out of the RHS usages.
        let referenced_by_rhs: HashSet<&str> = binds
            .iter()
            .filter(|b| rhs_usages.iter().any(|u| u.contains_key(&b.name)))
            .map(|b| b.name.as_str())
            .collect();
        for (b, mut u_rhs) in binds.iter().zip(rhs_usages) {
            // Group names inside a right-hand side served the taint/recursion
            // analysis; they must not leak past the group.
            for other in binds {
                u_rhs.remove(&other.name);
            }
            let recursive = referenced_by_rhs.contains(b.name.as_str());
            let n_body = result.get(&b.name).map(|i| i.count);
            if n_body.is_none() && !recursive {
                // Never used: the thunk is never forced (and an uncalled
                // local function never runs), so nothing is consumed.
                continue;
            }
            let (factor, cause) = if !b.patterns.is_empty() {
                // Local function: its body runs once per CALL, and nothing
                // bounds how many calls a use of the name makes — captures
                // charge ω, exactly like a lambda's captures. (What the
                // function does with its ARGUMENTS is refined separately,
                // by the inferred per-parameter factors at each call site.)
                (Factor::Any, Some(format!(
                    "it is used inside the local function '{}', which may \
                     be called any number of times", b.name)))
            } else if recursive {
                (Factor::Any, Some(format!(
                    "it is used by the recursive (or mutually recursive) \
                     local binding '{}', whose definition may be consumed \
                     any number of times", b.name)))
            } else if matches!(n_body, Some(Count::One)) {
                // Used exactly once: the thunk is forced at most once and
                // its value consumed once, so the right-hand side's own
                // usage passes through unchanged.
                (Factor::Once, None)
            } else if let Some(Count::OneAt(id)) = n_body {
                // Used once, in a context that may consume it up to `%m`
                // times: everything the right-hand side consumed is consumed
                // up to m times too.
                (Factor::Rigid(id), None)
            } else {
                // The binder's count is ω — literally used more than once,
                // or used once in a context that may duplicate (or drop) it,
                // in which case the count carries the recorded reason and
                // that reason is the accurate one to report.
                let inflated = result.get(&b.name).and_then(|i| i.cause.clone());
                let cause = match inflated {
                    Some(why) => format!(
                        "it is used through the local binding '{}', and '{}' \
                         may itself be consumed more than once (or never): \
                         {}", b.name, b.name, why),
                    None => format!(
                        "it is used through the local binding '{}', and '{}' \
                         is used more than once — each use of '{}' is \
                         another use of everything its definition consumed",
                        b.name, b.name, b.name),
                };
                (Factor::Any, Some(cause))
            };
            add_usage(&mut result, scale_usage(u_rhs, factor, &cause));
        }
        for b in binds {
            result.remove(&b.name);
        }
        self.restore(saved);
        result
    }

    // --- Iterative bind-chain walker ---

    /// Does `>>=`/`>>` at this action type run its continuation EXACTLY
    /// once per run of the composed action? True for IO, LuaIO and ST —
    /// the monads whose bind implementation is fixed by the Prelude and has
    /// that property. (A user cannot override them: an instance for a
    /// builtin class on a builtin type is rejected as a duplicate/orphan.)
    fn bind_runs_cont_exactly_once(ty: &Ty) -> bool {
        match ty {
            Ty::IO(_) | Ty::LuaIO(..) => true,
            // ST s a = App(App(Con "ST", s), a)
            Ty::App(f, _) => matches!(
                f.as_ref(),
                Ty::App(g, _) if matches!(g.as_ref(), Ty::Con(n) if n == "ST")),
            _ => false,
        }
    }

    /// Is this action type `Maybe a`? Maybe's bind runs its continuation AT
    /// MOST once — exactly once on `Just`, zero times on `Nothing`. The
    /// once-charge is therefore a sound UPPER bound, but a linear variable
    /// consumed inside the continuation is rejected separately: on the
    /// `Nothing` path it would be consumed zero times.
    fn bind_is_maybe(ty: &Ty) -> bool {
        matches!(ty, Ty::App(f, _)
            if matches!(f.as_ref(), Ty::Con(n) if n == "Maybe"))
    }

    /// Does `>>=`/`>>` at this action type run its continuation AT MOST
    /// ONCE per run of the composed action (IO/LuaIO/ST exactly once, Maybe
    /// once or not at all)? The list monad runs it per element, and a user
    /// monad's `>>=` is arbitrary code — both charge ω.
    fn bind_runs_cont_at_most_once(ty: &Ty) -> bool {
        Self::bind_runs_cont_exactly_once(ty) || Self::bind_is_maybe(ty)
    }

    /// The result type an action of this monadic type yields to its
    /// continuation, if the shape is recognized.
    fn bind_result_ty(ty: &Ty) -> Option<&Ty> {
        match ty {
            Ty::IO(a) => Some(a),
            Ty::LuaIO(_, a) => Some(a),
            Ty::List(a) => Some(a),
            // Maybe a, ST s a, user monads m a: the last applied argument.
            Ty::App(_, a) => Some(a),
            _ => None,
        }
    }

    /// The skip-path description for consumption inside a Maybe bind's
    /// continuation.
    fn maybe_skip_path() -> String {
        "it is consumed inside the continuation of a 'Maybe' bind \
         ('>>='/do-block), and the 'Nothing' path skips that continuation — \
         on that path it is consumed zero times".to_string()
    }

    /// The ω-cause for a continuation in a monad outside the at-most-once
    /// set above.
    fn many_shot_bind_cause() -> Option<String> {
        Some(
            "it is used under a monadic bind whose '>>=' may run the \
             continuation any number of times (only the IO, LuaIO, ST and \
             Maybe binds are known to run it at most once — a list bind \
             runs it per element, and a user-defined monad's bind is \
             arbitrary code)".to_string())
    }

    /// Usage of a `>>=`/`>>` spine (do-block desugaring), processed
    /// iteratively like `TExpr::apply_subst` so a long do-block cannot
    /// overflow the native stack. Continuations of the at-most-once monads
    /// (see `bind_runs_cont_at_most_once`) are charged once; other monads'
    /// continuations charge ω.
    fn bind_chain_usage(&mut self, e: &TExpr) -> Usage {
        // Group frames keep their binds slice in a parallel stack (a frame
        // cannot borrow it directly without tangling lifetimes).
        let mut group_binds: Vec<&[TLocalDef]> = Vec::new();
        let mut frames: Vec<BindFrame> = Vec::new();

        let mut current = e;
        loop {
            match &current.kind {
                TExprKind::InfixApp { op, lhs, rhs } if op == ">>=" || op == ">>" => {
                    let u_lhs = self.expr_usage(lhs);
                    let once = Self::bind_runs_cont_at_most_once(&lhs.ty);
                    let skippable = Self::bind_is_maybe(&lhs.ty);
                    let taint_src = self.taint_source(&u_lhs);
                    // `>>` (and a `_ <-` binder below) DISCARDS the action's
                    // result. When the action consumed a linear variable,
                    // its result may carry the pending consumption (e.g.
                    // `pure (useOnce t) >> …` never forces the payload), so
                    // only a `()` result — the run-for-effect idiom — may be
                    // dropped.
                    if op == ">>"
                        && let Some((src, _)) = &taint_src
                        && !matches!(Self::bind_result_ty(&lhs.ty), Some(Ty::Unit))
                    {
                        self.flag_discarded_result(src);
                    }
                    if let TExprKind::Lambda { params, body } = &rhs.kind {
                        // The continuation lambda of a do-bind. Its binder
                        // aliases the action's result: linear when the
                        // action consumed a linear variable (inheriting that
                        // variable's bound at every type; only unit binders
                        // are exempt).
                        let mut saved: SavedBinders = Vec::new();
                        let mut checked: Vec<String> = Vec::new();
                        let mut names: Vec<String> = Vec::new();
                        for (name, pty) in params {
                            if name == "_" {
                                if let Some((src, _)) = &taint_src
                                    && !matches!(pty, Ty::Unit)
                                {
                                    self.flag_discarded_result(src);
                                }
                                continue;
                            }
                            names.push(name.clone());
                            saved.push(self.shadow(name));
                            if let Some((src, bound)) = &taint_src
                                && !matches!(pty, Ty::Unit)
                            {
                                self.linear.insert(name.clone(), LinearInfo {
                                    origin: format!(
                                        "it was bound (with '<-') from an \
                                         action that consumes '{}', which \
                                         must itself be consumed exactly \
                                         once", src),
                                    bound: *bound,
                                });
                                checked.push(name.clone());
                            }
                        }
                        frames.push(BindFrame::Bind {
                            u_lhs, once, skippable, params: names, saved, checked,
                        });
                        current = body;
                        continue;
                    }
                    // `>>=` with a non-lambda continuation: the action's
                    // result flows into an arbitrary function the tracker
                    // cannot see into. Sound only when that function's own
                    // arrow promises exactly-once consumption.
                    if op == ">>="
                        && let Some((src, bound)) = &taint_src
                    {
                        let promises = match &rhs.ty {
                            Ty::Arrow(_, _, Mult::One) => true,
                            Ty::Arrow(_, _, Mult::Rigid(id)) =>
                                matches!(bound, Bound::OnceAt(m) if m == id),
                            _ => false,
                        };
                        if !promises {
                            let ai = self.linear.get(src).cloned();
                            if let Some(ai) = ai {
                                self.viols.push(Violation {
                                    name: src.clone(),
                                    origin: ai.origin,
                                    cause: Some(
                                        "the value bound from an action that \
                                         consumes it flows to a '>>=' \
                                         continuation whose parameter arrow \
                                         is not '%1' — that continuation may \
                                         consume the value any number of \
                                         times, or never".to_string()),
                                    kind: ViolKind::Overuse,
                                });
                            }
                        }
                    }
                    // The continuation value itself is consumed once by the
                    // bind; a many-shot monad may then run it many times,
                    // and Maybe may skip it.
                    let u_r = self.expr_usage(rhs);
                    if skippable {
                        self.flag_skippable(&u_r, &Self::maybe_skip_path());
                    }
                    let (factor, cause) = if once {
                        (Factor::Once, None)
                    } else {
                        (Factor::Any, Self::many_shot_bind_cause())
                    };
                    let mut u = u_lhs;
                    add_usage(&mut u, scale_usage(u_r, factor, &cause));
                    return self.unwind_bind_frames(frames, group_binds, u);
                }
                TExprKind::Let { binds, body } => {
                    let gctx = self.group_enter(binds);
                    frames.push(BindFrame::Group { gctx });
                    group_binds.push(binds);
                    current = body;
                    continue;
                }
                _ => break,
            }
        }

        let u = self.expr_usage_node(current);
        self.unwind_bind_frames(frames, group_binds, u)
    }

    /// Unwind the collected bind-chain frames bottom-up, mirroring the
    /// recursive combination the frames replaced.
    fn unwind_bind_frames(
        &mut self,
        frames: Vec<BindFrame>,
        mut group_binds: Vec<&[TLocalDef]>,
        mut u: Usage,
    ) -> Usage {
        for frame in frames.into_iter().rev() {
            match frame {
                BindFrame::Bind { u_lhs, once, skippable, params, saved, checked } => {
                    for name in &checked {
                        self.check_binder(&u, name);
                    }
                    for name in &params {
                        u.remove(name);
                    }
                    self.restore(saved);
                    // A Maybe bind may skip its continuation on `Nothing`:
                    // any (outer) tracked variable consumed in it would be
                    // consumed zero times on that path. Checked after the
                    // continuation's own binders are removed, so it applies
                    // exactly to variables from enclosing scopes.
                    if skippable {
                        self.flag_skippable(&u, &Self::maybe_skip_path());
                    }
                    let (factor, cause) = if once {
                        (Factor::Once, None)
                    } else {
                        (Factor::Any, Self::many_shot_bind_cause())
                    };
                    u = scale_usage(u, factor, &cause);
                    let mut combined = u_lhs;
                    add_usage(&mut combined, u);
                    u = combined;
                }
                BindFrame::Group { gctx } => {
                    let binds = group_binds.pop().expect("group frame without binds");
                    u = self.group_exit(binds, gctx, u);
                }
            }
        }
        u
    }
}

/// Frames of the iterative bind-chain walk (named at module level so the
/// helper above can take them).
enum BindFrame {
    Bind {
        u_lhs: Usage,
        /// Whether this bind's monad runs the continuation at most once
        /// (`bind_runs_cont_at_most_once`).
        once: bool,
        /// Whether it may also SKIP the continuation (Maybe on `Nothing`) —
        /// tracked variables consumed inside are flagged as path drops.
        skippable: bool,
        params: Vec<String>,
        saved: SavedBinders,
        checked: Vec<String>,
    },
    Group {
        gctx: GroupCtx,
    },
}

impl Checker {
    /// Enforce the linear (`%1`) usage discipline on one checked function:
    /// every binder bound at a `%1` arrow — and every derived alias of such
    /// a value — must be consumed exactly once on every evaluation path.
    /// Runs over the final (post-substitution) typed clauses; pushes
    /// ordinary diagnostics. Functions that never touch a `%1` type produce
    /// an empty tracking set, so this is a no-op walk for them.
    pub(super) fn check_function_usage(&mut self, fun: &TFunction) {
        for clause in &fun.clauses {
            let mut walker = UsageCk {
                ck: self,
                linear: HashMap::new(),
                locals: HashMap::new(),
                fn_params: HashMap::new(),
                viols: Vec::new(),
            };

            // Every clause-pattern binder is a local name for the whole
            // clause (relevant to the Prelude-name whitelist, e.g. a
            // parameter named 'pure').
            for pat in &clause.patterns {
                let mut binders = Vec::new();
                pattern_binders(pat, true, &mut binders);
                for (name, _, _) in binders {
                    *walker.locals.entry(name).or_insert(0) += 1;
                }
            }

            // Arguments bound at the function type's `%1` arrows — and its
            // rigid `%m` arrows, whose binders are held to the polymorphic
            // budget (a caller may instantiate m to 1; see the module
            // comment). Every binder in the pattern is enforced exactly
            // once — scalars destructured out of the argument included
            // (only `()`-typed non-direct binders are exempt; SCALARS AND
            // UNIT in the module comment) — and a wildcard anywhere in the
            // pattern is an immediate leak (whatever it matches is
            // discarded unconsumed).
            let mut cur = &fun.ty;
            while let Ty::Forall(_, inner) = cur {
                cur = inner;
            }
            let mut top_linear: Vec<String> = Vec::new();
            for pat in &clause.patterns {
                let Ty::Arrow(_, rest, m) = cur else { break };
                let tracked = match m {
                    Mult::One => Some((Bound::Once, format!(
                        "the type of '{}' declares this argument '%1'",
                        fun.name))),
                    Mult::Rigid(id) => Some((Bound::OnceAt(*id), format!(
                        "the type of '{}' declares this argument with the \
                         multiplicity variable '%m', which a caller may \
                         instantiate to '%1'", fun.name))),
                    _ => None,
                };
                if let Some((bound, origin)) = tracked {
                    if pattern_has_wildcard(pat) {
                        walker.viols.push(Violation {
                            name: "_".to_string(),
                            origin: origin.clone(),
                            cause: None,
                            kind: ViolKind::Unused(
                                "a wildcard ('_') in this argument's pattern \
                                 discards part (or all) of the value without \
                                 consuming it".to_string()),
                        });
                    }
                    let mut binders = Vec::new();
                    pattern_binders(pat, true, &mut binders);
                    for (name, ty, direct) in binders {
                        if direct || !matches!(ty, Ty::Unit) {
                            walker.linear.insert(name.clone(), LinearInfo {
                                origin: origin.clone(),
                                bound,
                            });
                            top_linear.push(name);
                        }
                    }
                }
                cur = rest;
            }

            // Clause core (guards/body) wrapped in the where-binding group.
            let gctx = walker.group_enter(&clause.where_binds);
            let core = walker.branch_usage(&clause.guards, &clause.body);
            let u = walker.group_exit(&clause.where_binds, gctx, core);

            top_linear.sort();
            top_linear.dedup();
            for name in &top_linear {
                walker.check_binder(&u, name);
            }

            let viols = std::mem::take(&mut walker.viols);
            drop(walker);
            let span = clause.span.unwrap_or_default();
            // The same violation can be observed at more than one check
            // point (e.g. a leak seen at a branch join and again at clause
            // end); report each rendered message once.
            let mut seen: HashSet<String> = HashSet::new();
            for v in viols {
                let req = "must be consumed exactly once";
                let (problem, note) = match &v.kind {
                    ViolKind::Overuse => {
                        let cause = v.cause.clone().unwrap_or_else(|| {
                            "this definition uses it more than once along a \
                             single evaluation path".to_string()
                        });
                        (cause,
                         "a '%1' arrow is a promise that the function \
                          consumes the value exactly once. A second use \
                          would act on a value that may already be gone — \
                          for an external resource such as a file handle, \
                          that is the double-close/double-free class of \
                          bug. To allow unrestricted use, write a plain \
                          '->' instead.")
                    }
                    ViolKind::Unused(detail) => (
                        format!("it is consumed zero times: {}", detail),
                        "a '%1' value must be used exactly once; leaving it \
                         unused leaks the resource it stands for (e.g. a \
                         file handle that is never closed). Consume it, \
                         pass it to another '%1' consumer, or write a plain \
                         '->' if the value needs no cleanup.",
                    ),
                    ViolKind::PathDrop(desc) => (
                        desc.clone(),
                        "a '%1' value must be consumed exactly once on \
                         EVERY evaluation path: a path that skips the \
                         consumption leaks the resource (e.g. a file handle \
                         that is never closed). Consume it on every path, \
                         or write a plain '->' if the value needs no \
                         cleanup.",
                    ),
                };
                let msg = format!("'{}' {} — {} — but {}", v.name, req, v.origin, problem);
                if !seen.insert(msg.clone()) {
                    continue;
                }
                self.push_error_span(
                    DiagnosticKind::Other(msg),
                    format!("definition of '{}'", fun.name),
                    span,
                );
                if let Some(diag) = self.errors.last_mut() {
                    diag.notes.push(note.to_string());
                }
            }
        }
    }
}
