//! Operational annotations over the emitted Lua tree: the stamp lattice, the
//! ONE trusted analysis that derives stamps, and the rewrite engine that owns
//! them.
//!
//! Stamps carry OPERATIONAL facts about the value an expression evaluates to
//! — not source types. The vocabulary is a small shape lattice (WHNF /
//! constructor-shape / closure / thunk / unknown) plus effect bits (pure /
//! may-trap / may-allocate). The lattice is monotone: recomputation and
//! rewriting may only weaken a stamp toward unknown; nothing in this module
//! strengthens one except the bottom-up analysis itself. When the analysis is
//! in doubt it says unknown — an overclaim here is the one bug class the
//! architecture cannot catch, so every arm that claims more than unknown
//! states why it may.
//!
//! Write monopoly: passes have NO write access to stamps. `Stamp` and
//! `StampNode` keep their fields private to this module, and the only
//! construction paths are the analysis and the engine's justification rules —
//! a pass sees stamps through the read-only `StampView` and changes the tree
//! only by returning a `Request` from `ExprPass::request`. A request declares
//! the justification for the result node's stamp:
//!
//! * `ReplaceWithChild(i)` — the node becomes its own canonical child `i`, so
//!   the child's stamp travels with it. Inherit-from-named-source, sound by
//!   construction: the engine builds the replacement from the child itself,
//!   the pass never supplies an expression.
//! * `Replace(expr, Justify::MeetOfChildren(..))` — the result is stamped
//!   with the meet (weakest common claim) of the named canonical children of
//!   the ORIGINAL node.
//! * `Replace(expr, Justify::Unknown)` — no claim.
//!
//! Any `Replace` puts the mirror in doubt beyond its own node (the new
//! expression's subtree carries no derived stamps), so the engine marks the
//! run dirty and recomputes the whole mirror from the rewritten tree
//! afterward — invalidate-and-recompute, no incremental stamp preservation:
//! at mata-ll's whole-program sizes a recomputation is cheap, and a second
//! preservation logic would be a second trusted base.
//!
//! Storage: a mirror tree (`StampNode`) walked in lockstep with the Lua tree,
//! one node per expression in a fixed canonical order (statement expression
//! slots in statement order; expression children callee-then-arguments,
//! left-then-right; a function literal's children are its body's slots).
//! Identity is positional, so there are no keys to dangle across rewrites —
//! a side table keyed by node pointers would go stale the moment a pass
//! rebuilds a subtree, which is exactly when correctness matters.
//!
//! Raw policy (binding, same as opt.rs): `Expr::Raw` / `Stmt::Raw` are
//! opaque. A Raw expression is stamped unknown with every effect bit set, a
//! name mentioned in any Raw text in scope never qualifies for a name stamp,
//! and no rewrite ever touches Raw content.
//!
//! What the analysis re-derives: the generation-time machinery
//! (`expr_yields_whnf`, `concrete_vars`, `forced_ast` in thunks.rs) proves
//! WHNF facts on the TIR side and discards them at emission. This analysis
//! recovers the recoverable part post-hoc from the emitted tree alone:
//! literal tokens, function literals, table constructors, cons cells,
//! `__force` results (WHNF by the emitter's invariant that thunk bodies
//! return forced values — the axiom the former force-of-WHNF-locals pass
//! already relied on), `__thunk` wrappers (thunk-shaped), and names bound
//! single-assignment to a stamped producer under the same qualification
//! rules that pass used (one binding site, or a parameter/forward-declaration
//! rebind with exactly one assignment; never a name mentioned in Raw text).

use super::lua::{Block, Expr, FnTarget, Item, Stmt};
use std::collections::{HashMap, HashSet};

// ---- The stamp lattice ----

/// Value shape, ordered by claim strength: `Cons`, `Closure` and `Thunk` are
/// the precise claims, `Whnf` is the common weakening of the first two, and
/// `Unknown` claims nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Shape {
    /// Not a thunk (head-evaluated), nothing more specific known.
    Whnf,
    /// A runtime cons cell (`__mll_cons` / `__mll_lazy_cons` result).
    Cons,
    /// A Lua function value.
    Closure,
    /// A `__thunk` suspension (possibly already memoized — the wrapper table
    /// stays a thunk table after forcing, so mutation cannot invalidate this).
    Thunk,
    Unknown,
}

impl Shape {
    pub(super) fn is_whnf(self) -> bool {
        matches!(self, Shape::Whnf | Shape::Cons | Shape::Closure)
    }

    /// Weakest common claim.
    fn meet(self, other: Shape) -> Shape {
        if self == other {
            self
        } else if self.is_whnf() && other.is_whnf() {
            Shape::Whnf
        } else {
            Shape::Unknown
        }
    }

    /// `self` claims nothing `sound` does not prove.
    fn no_stronger_than(self, sound: Shape) -> bool {
        self == Shape::Unknown || self == sound || (self == Shape::Whnf && sound.is_whnf())
    }
}

/// One node's annotation: shape plus effect bits describing the evaluation
/// of the whole (sub)expression. The strong claims are `pure`,
/// NOT-`may_trap`, NOT-`may_alloc`; weakening drops `pure` and sets the
/// other two.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct Stamp {
    shape: Shape,
    pure: bool,
    may_trap: bool,
    may_alloc: bool,
}

impl Stamp {
    fn new(shape: Shape, pure: bool, may_trap: bool, may_alloc: bool) -> Stamp {
        Stamp { shape, pure, may_trap, may_alloc }
    }

    fn unknown() -> Stamp {
        Stamp::new(Shape::Unknown, false, true, true)
    }

    /// A rendered literal token: denotes a value, evaluation cannot do
    /// anything.
    fn scalar() -> Stamp {
        Stamp::new(Shape::Whnf, true, false, false)
    }

    /// Reading a name whose referent has shape `shape`: the read itself is
    /// effect-free (a local slot or an `_ENV` lookup, neither can raise).
    fn name_read(shape: Shape) -> Stamp {
        Stamp::new(shape, true, false, false)
    }

    pub(super) fn shape(&self) -> Shape {
        self.shape
    }

    pub(super) fn is_whnf(&self) -> bool {
        self.shape.is_whnf()
    }

    pub(super) fn is_pure(&self) -> bool {
        self.pure
    }

    /// The effect bits already participate in the refutation (`meet`,
    /// `absorb_effects`, `no_stronger_than`); these direct accessors have no
    /// caller yet. Their consumers are the remaining structured-tier passes
    /// (closure hoisting, liveness-based slot reuse — see doc/articles/TODO.md,
    /// "Lua-AST optimization layer, structured-pass tier"), which decide per
    /// node whether hoisting past a possibly-trapping or allocating
    /// neighbour is sound.
    #[allow(dead_code)]
    pub(super) fn may_trap(&self) -> bool {
        self.may_trap
    }

    #[allow(dead_code)]
    pub(super) fn may_allocate(&self) -> bool {
        self.may_alloc
    }

    fn meet(&self, o: &Stamp) -> Stamp {
        Stamp::new(
            self.shape.meet(o.shape),
            self.pure && o.pure,
            self.may_trap || o.may_trap,
            self.may_alloc || o.may_alloc,
        )
    }

    /// Fold a child's effects into a composite whose own contribution is
    /// `self` (the shape stays `self`'s — effects accumulate, shapes do not).
    fn absorb_effects(mut self, child: &Stamp) -> Stamp {
        self.pure = self.pure && child.pure;
        self.may_trap = self.may_trap || child.may_trap;
        self.may_alloc = self.may_alloc || child.may_alloc;
        self
    }

    /// Monotonicity order: `self` claims nothing `sound` does not prove.
    pub(super) fn no_stronger_than(&self, sound: &Stamp) -> bool {
        self.shape.no_stronger_than(sound.shape)
            && (!self.pure || sound.pure)
            && (self.may_trap || !sound.may_trap)
            && (self.may_alloc || !sound.may_alloc)
    }
}

// ---- The mirror tree ----

/// Stamp mirror of one expression: `children` follow the canonical child
/// order (see the module comment). Fully private — only the engine builds or
/// mutates these.
#[derive(Clone, Debug)]
struct StampNode {
    stamp: Stamp,
    children: Vec<StampNode>,
}

impl StampNode {
    fn leaf(stamp: Stamp) -> StampNode {
        StampNode { stamp, children: Vec::new() }
    }

    /// An all-unknown mirror of `e` — the shape a `Replace`d subtree gets
    /// until the post-pass recomputation.
    fn unknown_for(e: &Expr) -> StampNode {
        StampNode {
            stamp: Stamp::unknown(),
            children: expr_children(e)
                .into_iter()
                .map(|(c, _)| StampNode::unknown_for(c))
                .collect(),
        }
    }
}

/// Read-only stamp access for passes.
pub(super) struct StampView<'a> {
    node: &'a StampNode,
}

impl<'a> StampView<'a> {
    pub(super) fn stamp(&self) -> &'a Stamp {
        &self.node.stamp
    }

    /// The canonical child `i`'s stamps (callee = 0, first argument = 1, …).
    pub(super) fn child(&self, i: usize) -> Option<StampView<'a>> {
        self.node.children.get(i).map(|node| StampView { node })
    }
}

// ---- The rewrite vocabulary ----

/// The syntactic class of the hole an expression node sits in. Same
/// taxonomy as pass 1's `Ctx` (opt.rs) plus statement position, viewed from
/// the replacement side: the engine applies a rewrite only when the
/// replacement can stand in the hole under the printer's
/// no-parens-synthesized discipline AND yields exactly the one value the
/// replaced node did. A pass never sees holes — requesting an unfit rewrite
/// is simply declined, so a sloppy pass cannot emit invalid Lua.
#[derive(Clone, Copy, PartialEq)]
enum Hole {
    /// Expression-statement position: Lua only allows a call here.
    Stmt,
    /// Lua prefixexp position (callee, index base, method receiver).
    Prefix,
    /// Grouping matters (binop/neg operand, the child of a `Paren`): only a
    /// self-delimiting prefixexp-or-literal shape stands alone.
    Grouped,
    /// Delimited single-value position (condition, single-lvalue RHS,
    /// non-last argument, keyed table value): any expression fits.
    Delim,
    /// Delimited multi-value position (return operand, last argument, last
    /// positional table item, multi-lvalue RHS): a call spreads its values
    /// here, so only a single-return callee fits; `Raw` and method calls
    /// may multi-return and never fit.
    DelimLast,
}

/// May `e` stand alone in a hole of class `hole`? (See `Hole`.)
fn fits(e: &Expr, hole: Hole) -> bool {
    match hole {
        Hole::Stmt => matches!(e, Expr::Call(..) | Expr::Method(..)),
        Hole::Prefix => matches!(
            e,
            Expr::Name(_) | Expr::Index(..) | Expr::Paren(_) | Expr::Call(..) | Expr::Method(..)
        ),
        Hole::Grouped => matches!(
            e,
            Expr::Name(_)
                | Expr::Lit(_)
                | Expr::Index(..)
                | Expr::Paren(_)
                | Expr::Call(..)
                | Expr::Method(..)
        ),
        Hole::Delim => true,
        Hole::DelimLast => match e {
            Expr::Call(f, _) => super::opt::single_return_callee(f),
            Expr::Method(..) | Expr::Raw(_) => false,
            _ => true,
        },
    }
}

/// The closed justification vocabulary. `Replace` and both `Justify` forms
/// have no production consumer yet — the force-collapse peephole only
/// inherits — but they are the contract the structured-pass tier builds on,
/// and the engine tests exercise them.
#[allow(dead_code)]
pub(super) enum Justify {
    /// Stamp the result with the meet of these canonical children of the
    /// node being replaced.
    MeetOfChildren(Vec<usize>),
    /// No claim: the result is stamped unknown.
    Unknown,
}

#[allow(dead_code)]
pub(super) enum Request {
    /// Replace the node with its canonical child `i` (the engine extracts
    /// the child itself); the result inherits that child's stamps.
    ReplaceWithChild(usize),
    /// Replace the node with an arbitrary expression; the result's stamp
    /// follows the declared justification and the whole mirror is recomputed
    /// after the pass.
    Replace(Expr, Justify),
}

/// A rewrite-rule-tier pass: offered every expression node bottom-up, in
/// canonical order, with read access to the node's stamps. Returning a
/// request applies it and re-offers the resulting node (so nested matches
/// collapse in one run).
pub(super) trait ExprPass {
    fn request(&mut self, e: &Expr, stamps: &StampView<'_>) -> Option<Request>;
}

/// The trusted no-op: analysis is a pass run that never rewrites.
struct NoRewrite;

impl ExprPass for NoRewrite {
    fn request(&mut self, _e: &Expr, _stamps: &StampView<'_>) -> Option<Request> {
        None
    }
}

// ---- Scope facts (name qualification) ----

/// Per-named-function-scope binding counts, collected over the whole scope
/// subtree (nested function literals included: their params and locals
/// shadow, and their bodies may capture this scope's names). Same
/// qualification data the former force-of-WHNF-locals pass used.
#[derive(Default)]
struct ScopeFacts {
    /// name -> number of binding sites: `local` names, plain-identifier
    /// assignment lvalues, `AssignIf` lvalues, `Function` targets and
    /// parameters, nested function-literal parameters.
    binds: HashMap<String, usize>,
    /// name -> number of assignment sites (subset of binds).
    assigns: HashMap<String, usize>,
    /// Names mentioned inside any Raw text in the scope: poisoned — the
    /// analysis cannot see what the Raw does with them.
    raw_mentions: HashSet<String>,
}

impl ScopeFacts {
    fn bind(&mut self, n: &str) {
        *self.binds.entry(n.to_string()).or_insert(0) += 1;
    }

    fn assign(&mut self, n: &str) {
        self.bind(n);
        *self.assigns.entry(n.to_string()).or_insert(0) += 1;
    }

    fn raw(&mut self, text: &str) {
        super::opt::token_set(text, &mut self.raw_mentions);
    }

    /// A name's stamp may enter the environment once its single
    /// value-producing site has run: a sole `local n = <e>` (one binding
    /// site), or a sole assignment `n = <e>` whose one OTHER binding site is
    /// the parameter/forward declaration it rebinds.
    fn qualifies(&self, n: &str, is_assign: bool) -> bool {
        if self.raw_mentions.contains(n) {
            return false;
        }
        let binds = self.binds.get(n).copied().unwrap_or(0);
        let assigns = self.assigns.get(n).copied().unwrap_or(0);
        if is_assign {
            binds == 2 && assigns == 1
        } else {
            binds == 1 && assigns == 0
        }
    }
}

pub(super) fn is_plain_ident(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with(|c: char| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn collect_facts_block(stmts: &[Stmt], f: &mut ScopeFacts) {
    for s in stmts {
        collect_facts_stmt(s, f);
    }
}

// The census decision — which fields bind, assign, or poison — stays
// spelled per variant with NO wildcard: an unclassified binder would
// under-count and let `qualifies` claim a name it should not, so a new
// statement kind must be classified here before this compiles. The descent
// itself is the AST's own (`for_each_expr` / `for_each_block`); the facts
// are commutative counts, so visiting order does not matter.
fn collect_facts_stmt(s: &Stmt, f: &mut ScopeFacts) {
    match s {
        Stmt::Raw(t) => f.raw(t),
        Stmt::Local(names, _) => {
            for n in names {
                f.bind(n);
            }
        }
        Stmt::Assign(lhs, _) | Stmt::AssignIf { lhs, .. } => {
            if is_plain_ident(lhs) {
                f.assign(lhs);
            }
        }
        // A multiple assignment assigns every plain-identifier lvalue — the
        // tail-loop pass's parameter update is one more assignment site for
        // each parameter, so any name-stamp qualification over those names
        // correctly fails afterward.
        Stmt::MultiAssign(lhs, _) => {
            for l in lhs {
                if is_plain_ident(l) {
                    f.assign(l);
                }
            }
        }
        Stmt::Function { target, params, .. } => {
            // Precise now that the header is structured: the parameters are
            // exactly the header's parameter binding occurrences, and the
            // target contributes its bound/stored name. The target counts as
            // a BIND (not an assign), exactly as the former header
            // tokenization did — tailloop's Assigned self-identity gate
            // requires (2 binds, 0 assigns): the forward `local` plus this
            // header.
            match target {
                FnTarget::LocalFn(n) => f.bind(n),
                FnTarget::Assigned(lhs) if is_plain_ident(lhs) => f.bind(lhs),
                // Composite `_v[N]` spill lvalue: deliberately conservative,
                // reproducing the former tokenization — each identifier
                // token (`_v`) counts as a binding site, so the spill table's
                // name never qualifies for a name stamp. (Binding only the
                // precise store target is available now, but would change
                // `_v`'s counts.)
                FnTarget::Assigned(lhs) => {
                    let mut toks = HashSet::new();
                    super::opt::token_set(lhs, &mut toks);
                    for t in toks {
                        f.bind(&t);
                    }
                }
                // Deliberately conservative, reproducing the former
                // tokenization: the slot store counts one `__mll_fn` binding
                // site, keeping the table's name disqualified from name
                // stamps. (The slot census below is the precise view.)
                FnTarget::Slot(_) => f.bind("__mll_fn"),
            }
            for p in params {
                f.bind(p);
            }
        }
        Stmt::Return(_)
        | Stmt::Expr(_)
        | Stmt::If { .. }
        | Stmt::Do(_)
        | Stmt::WhileTrue(_)
        | Stmt::ReturnNone
        | Stmt::Goto(_)
        | Stmt::Label(_)
        | Stmt::ReturnTable(_) => {}
    }
    s.for_each_expr(&mut |e| collect_facts_expr(e, f));
    s.for_each_block(&mut |b| collect_facts_block(b, f));
}

fn collect_facts_expr(e: &Expr, f: &mut ScopeFacts) {
    match e {
        Expr::Raw(t) => f.raw(t),
        // A literal's parameters bind (they shadow); its body is statements,
        // which `for_each_subexpr` does not reach, so descend it here.
        Expr::Func(params, body) => {
            for p in params {
                f.bind(p);
            }
            collect_facts_block(body.stmts(), f);
        }
        Expr::Name(_)
        | Expr::Lit(_)
        | Expr::Paren(_)
        | Expr::Neg(_)
        | Expr::Call(..)
        | Expr::Method(..)
        | Expr::Index(..)
        | Expr::Binop(..)
        | Expr::Table(_)
        | Expr::TableSpaced(_) => {}
    }
    e.for_each_subexpr(&mut |c| collect_facts_expr(c, f));
}

// ---- The engine ----

pub(super) struct Engine {
    /// Mirror of every top-level expression slot of the module body, in
    /// canonical order.
    roots: Vec<StampNode>,
}

impl Engine {
    /// Derive stamps without rewriting anything.
    pub(super) fn analyze(stmts: &mut [Stmt]) -> Engine {
        Engine::run_pass(stmts, &mut NoRewrite)
    }

    /// Run one pass over the module body: derive stamps bottom-up and apply
    /// the pass's rewrite requests as they are offered. If any request went
    /// beyond the by-construction-sound vocabulary (`Replace`), the whole
    /// mirror is recomputed from the rewritten tree before returning.
    pub(super) fn run_pass(stmts: &mut [Stmt], pass: &mut dyn ExprPass) -> Engine {
        let mut w = Walker { pass, dirty: false };
        let roots = w.function_scope(stmts);
        if w.dirty {
            let mut clean = Walker { pass: &mut NoRewrite, dirty: false };
            let roots = clean.function_scope(stmts);
            return Engine { roots };
        }
        Engine { roots }
    }

    /// Stamp refutation over the final tree (test builds): recompute the
    /// analysis fresh and report
    ///
    /// 1. every carried stamp that is stronger than the fresh one (an
    ///    engine/justification overclaim — the failure mode the write
    ///    monopoly exists to contain), and
    /// 2. when `check_residual_force` (the collapse pass ran): every
    ///    remaining `__force(e)` where the FRESH analysis stamps `e`
    ///    WHNF-and-pure — a collapse the pass owed and did not deliver.
    ///
    /// Each violation names the node's rendered text.
    pub(super) fn refute(&self, stmts: &[Stmt], check_residual_force: bool) -> Vec<String> {
        let mut copy = stmts.to_vec();
        let fresh = Engine::analyze(&mut copy);
        let mut v = Vec::new();
        let mut slots = Vec::new();
        for s in stmts {
            stmt_slots(s, &mut slots);
        }
        if slots.len() != self.roots.len() || slots.len() != fresh.roots.len() {
            v.push(format!(
                "internal: stamp mirror out of alignment: {} slots, {} carried, {} fresh",
                slots.len(),
                self.roots.len(),
                fresh.roots.len()
            ));
            return v;
        }
        for (((e, hole), carried), fresh) in slots.iter().zip(&self.roots).zip(&fresh.roots) {
            zip_refute(e, *hole, carried, fresh, check_residual_force, &mut v);
        }
        v
    }
}

// ---- The structured tier ----
//
// A structured pass rewrites whole named-function bodies (`Stmt::Function`)
// instead of single expression nodes. It extends the rewrite vocabulary, so
// it carries the engine-side obligations the design recorded:
//
// * Write monopoly: the pass never sees or touches stamps at all. The engine
//   treats any applied structured rewrite as invalidate-everything — no stamp
//   from before the pass survives; the whole mirror is recomputed from the
//   final tree (`Engine::analyze`), the same cheap answer `Replace` already
//   uses. A pass that rewrites nothing leaves the tree untouched and the
//   caller's existing engine valid.
// * Grammatical validity: the engine validates every rewritten body against
//   the structured vocabulary's own rules (`structured_valid`) before
//   applying it — multi-assignment shapes, bare-return placement, and the
//   goto/label discipline Lua imposes — and declines the whole rewrite
//   otherwise. Nodes outside that vocabulary are moved verbatim from an
//   already-valid tree into holes of the same class, so they need no
//   re-check.

/// A structured-tier pass: offered every named function definition
/// (`Stmt::Function`), innermost scopes first, with read access to the
/// binding facts of the scope the function's name is bound in.
/// `locals_in_scope` holds the names `local`-declared in this scope and
/// lexically visible at the definition site — the proof that an
/// `X = function(…)` header assigns THAT local rather than reaching an
/// enclosing scope or `_ENV` (the flat per-scope binding counts alone
/// cannot tell a forward declaration from, say, a lambda parameter in some
/// other sub-tree of the scope). Returning a block replaces the function's
/// body — after the engine validates it.
pub(super) trait StructuredPass {
    fn request(
        &mut self,
        target: &FnTarget,
        params: &[String],
        body: &Block,
        view: &ScopeView<'_>,
        locals_in_scope: &HashSet<String>,
    ) -> Option<Block>;
}

/// Read-only binding facts for a structured pass: the enclosing scope's
/// name-binding counts (the same `ScopeFacts` the analysis qualifies name
/// stamps with) plus the module-wide `__mll_fn` slot-store census.
pub(super) struct ScopeView<'a> {
    facts: &'a ScopeFacts,
    slots: &'a SlotStores,
}

impl ScopeView<'_> {
    /// (binding sites, assignment sites) of `name` in the scope it is bound
    /// in — collected over the whole scope subtree, nested functions
    /// included, so any shadowing rebind anywhere disqualifies the name.
    pub(super) fn binding_sites(&self, name: &str) -> (usize, usize) {
        (
            self.facts.binds.get(name).copied().unwrap_or(0),
            self.facts.assigns.get(name).copied().unwrap_or(0),
        )
    }

    /// `name` appears in Raw text somewhere in the scope: poisoned.
    pub(super) fn raw_mentions(&self, name: &str) -> bool {
        self.facts.raw_mentions.contains(name)
    }

    /// The slot reference (`__mll_fn[3]`) is stored to exactly once in the
    /// whole module, the `__mll_fn` table itself is declared exactly once
    /// and never reassigned, and no Raw text mentions the slot (a Raw
    /// mention counts as a potential store).
    pub(super) fn slot_single_store(&self, slot_ref: &str) -> bool {
        !self.slots.poisoned
            && self.slots.table_locals == 1
            && self.slots.stores.get(slot_ref).copied() == Some(1)
    }
}

/// Module-wide census of `__mll_fn[i]` stores. `local __mll_fn` declarations
/// are counted separately (exactly one is the module layout's table init);
/// any spelling the scan cannot attribute to a single slot poisons the whole
/// census — a structured pass then converts nothing slot-named.
#[derive(Default)]
struct SlotStores {
    stores: HashMap<String, usize>,
    table_locals: usize,
    poisoned: bool,
}

impl SlotStores {
    fn collect(stmts: &[Stmt]) -> SlotStores {
        let mut s = SlotStores::default();
        slot_scan_block(stmts, &mut s);
        s
    }

    fn store(&mut self, slot_ref: &str) {
        *self.stores.entry(slot_ref.to_string()).or_insert(0) += 1;
    }

    /// Count every `__mll_fn` occurrence in raw text: a well-formed
    /// `__mll_fn[<digits>]` counts as a potential store to that slot (the
    /// scan cannot tell a store from a read, and either disqualifies the
    /// slot for a structured pass); anything else poisons the census.
    fn scan_raw(&mut self, text: &str) {
        let bytes = text.as_bytes();
        let needle = b"__mll_fn";
        let mut i = 0;
        while i + needle.len() <= bytes.len() {
            if &bytes[i..i + needle.len()] == needle {
                let ident_ch =
                    |b: u8| b.is_ascii_alphanumeric() || b == b'_';
                let starts_ident = i > 0 && ident_ch(bytes[i - 1]);
                let after = i + needle.len();
                if !starts_ident {
                    match parse_slot_suffix(&text[after..]) {
                        Some((slot_ref, _len)) => {
                            self.store(&format!("__mll_fn{}", slot_ref))
                        }
                        None => self.poisoned = true,
                    }
                }
                i = after;
            } else {
                i += 1;
            }
        }
    }
}

/// Parse a leading `[<digits>]` and return it with its length.
fn parse_slot_suffix(s: &str) -> Option<(&str, usize)> {
    let rest = s.strip_prefix('[')?;
    let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    if !rest[digits..].starts_with(']') {
        return None;
    }
    Some((&s[..digits + 2], digits + 2))
}

/// `lhs` spells exactly `__mll_fn[<digits>]`.
pub(super) fn is_slot_ref(lhs: &str) -> bool {
    lhs.strip_prefix("__mll_fn")
        .and_then(parse_slot_suffix)
        .is_some_and(|(slot, len)| slot.len() == len && len == lhs.len() - "__mll_fn".len())
}

fn slot_scan_block(stmts: &[Stmt], s: &mut SlotStores) {
    for st in stmts {
        slot_scan_stmt(st, s);
    }
}

fn slot_scan_lvalue(lhs: &str, s: &mut SlotStores) {
    if lhs == "__mll_fn" {
        s.poisoned = true;
    } else if is_slot_ref(lhs) {
        s.store(lhs);
    } else if lhs.contains("__mll_fn") {
        s.poisoned = true;
    }
}

// Like the binding census: the store/poison decision stays spelled per
// variant with NO wildcard — a missed store site would let
// `slot_single_store` claim a twice-stored slot. Descent is the AST's own,
// and the census is commutative counts, so visiting order does not matter.
fn slot_scan_stmt(st: &Stmt, s: &mut SlotStores) {
    match st {
        Stmt::Raw(t) => s.scan_raw(t),
        Stmt::Local(names, _) => {
            if names.iter().any(|n| n == "__mll_fn") {
                s.table_locals += 1;
            }
        }
        Stmt::Assign(lhs, _) | Stmt::AssignIf { lhs, .. } => slot_scan_lvalue(lhs, s),
        Stmt::MultiAssign(lhs, _) => {
            for l in lhs {
                slot_scan_lvalue(l, s);
            }
        }
        Stmt::Function { target, params, .. } => {
            // The slot store is structural now: a `Slot` target IS the
            // `__mll_fn[i] = function(...)` store. Any other `__mll_fn`
            // spelling in the header — a target or parameter name containing
            // it — poisons, exactly as the former text scan did (shared
            // predicate: lua::name_mentions_fn_table).
            match target {
                FnTarget::Slot(i) => {
                    s.store(&format!("{}[{}]", super::lua::FN_TABLE, i))
                }
                FnTarget::LocalFn(n) | FnTarget::Assigned(n) => {
                    if super::lua::name_mentions_fn_table(n, params) {
                        s.poisoned = true;
                    }
                }
            }
        }
        Stmt::Return(_)
        | Stmt::Expr(_)
        | Stmt::If { .. }
        | Stmt::Do(_)
        | Stmt::WhileTrue(_)
        | Stmt::ReturnNone
        | Stmt::Goto(_)
        | Stmt::Label(_)
        | Stmt::ReturnTable(_) => {}
    }
    st.for_each_expr(&mut |e| slot_scan_expr(e, s));
    st.for_each_block(&mut |b| slot_scan_block(b, s));
}

fn slot_scan_expr(e: &Expr, s: &mut SlotStores) {
    match e {
        // Reads (`__mll_fn[3]` as a name, `__mll_fn` as an index base) are
        // not stores; only Raw text is scanned, because the scan cannot see
        // what raw text does with a mention.
        Expr::Raw(t) => s.scan_raw(t),
        Expr::Func(_, body) => slot_scan_block(body.stmts(), s),
        Expr::Name(_)
        | Expr::Lit(_)
        | Expr::Paren(_)
        | Expr::Neg(_)
        | Expr::Call(..)
        | Expr::Method(..)
        | Expr::Index(..)
        | Expr::Binop(..)
        | Expr::Table(_)
        | Expr::TableSpaced(_) => {}
    }
    e.for_each_subexpr(&mut |c| slot_scan_expr(c, s));
}

impl Engine {
    /// Apply a structured pass to every named function definition in the
    /// module. Returns `Some` with a freshly recomputed engine when anything
    /// was rewritten (invalidate-everything: no stamp from before the pass
    /// survives a structured rewrite), `None` when the tree is untouched —
    /// the caller's existing engine then still mirrors it.
    pub(super) fn run_structured(
        stmts: &mut [Stmt],
        pass: &mut dyn StructuredPass,
    ) -> Option<Engine> {
        let slots = SlotStores::collect(stmts);
        let mut any = false;
        structured_scope(stmts, &slots, pass, &mut any);
        if any { Some(Engine::analyze(stmts)) } else { None }
    }
}

/// One function scope (module top, a named function's body, or a function
/// literal's body): convert nested scopes first — an enclosing conversion's
/// parameter rename then walks already-final inner trees — then offer every
/// function declared directly in this scope with this scope's binding facts.
/// The facts stay valid across sibling conversions within the scope: a
/// conversion renames only non-binding occurrences of its own parameters and
/// introduces only fresh names, so no pre-existing name's binding count
/// changes. (Its parameter update adds assignment sites for its parameter
/// names — which only ever makes a later qualification question stricter.)
fn structured_scope(
    scope: &mut [Stmt],
    slots: &SlotStores,
    pass: &mut dyn StructuredPass,
    any: &mut bool,
) {
    for s in scope.iter_mut() {
        structured_descend_stmt(s, slots, pass, any);
    }
    let mut facts = ScopeFacts::default();
    collect_facts_block(scope, &mut facts);
    let view = ScopeView { facts: &facts, slots };
    offer_block(scope, &view, pass, any, &HashSet::new());
}

/// Descend into nested function scopes (named bodies and literals) below
/// this statement, without offering anything at this scope's level yet.
fn structured_descend_stmt(
    s: &mut Stmt,
    slots: &SlotStores,
    pass: &mut dyn StructuredPass,
    any: &mut bool,
) {
    // A named function's body is a scope of its own: run the scope walk
    // there instead of the plain descent. (Sibling scopes convert
    // independently — a conversion touches only its own body and draws its
    // fresh names from its own text — so the descent order between the
    // expression side and the block side is free.)
    if let Stmt::Function { body, .. } = s {
        structured_scope(&mut body.0, slots, pass, any);
        return;
    }
    s.for_each_expr_mut(&mut |e| structured_descend_expr(e, slots, pass, any));
    s.for_each_block_mut(&mut |b| {
        for s in b.iter_mut() {
            structured_descend_stmt(s, slots, pass, any);
        }
    });
}

fn structured_descend_expr(
    e: &mut Expr,
    slots: &SlotStores,
    pass: &mut dyn StructuredPass,
    any: &mut bool,
) {
    // A function literal's body is a scope of its own, and the only place
    // an expression hides statements.
    if let Expr::Func(_, body) = e {
        structured_scope(body.stmts_mut(), slots, pass, any);
        return;
    }
    e.for_each_subexpr_mut(&mut |c| structured_descend_expr(c, slots, pass, any));
}

/// Offer every `Stmt::Function` declared in this scope (reachable through
/// `If`/`Do`/`WhileTrue` blocks but not through nested function bodies —
/// those were offered by their own scope walk). `locals` accumulates the
/// `local`-declared names lexically visible so far; sub-blocks extend a
/// clone, matching Lua's scoping.
fn offer_block(
    stmts: &mut [Stmt],
    view: &ScopeView<'_>,
    pass: &mut dyn StructuredPass,
    any: &mut bool,
    locals: &HashSet<String>,
) {
    let mut locals = locals.clone();
    for s in stmts {
        match s {
            Stmt::Local(names, _) => locals.extend(names.iter().cloned()),
            Stmt::Function { target, params, body } => {
                if let Some(new_body) = pass.request(target, params, body, view, &locals)
                    && structured_valid(&new_body.0)
                {
                    *body = new_body;
                    *any = true;
                }
            }
            Stmt::If { then_b, elseifs, else_b, .. } => {
                for b in std::iter::once(&mut then_b.0)
                    .chain(elseifs.iter_mut().map(|(_, b)| &mut b.0))
                    .chain(else_b.iter_mut().map(|b| &mut b.0))
                {
                    offer_block(b, view, pass, any, &locals);
                }
            }
            Stmt::Do(b) | Stmt::WhileTrue(b) => offer_block(&mut b.0, view, pass, any, &locals),
            _ => {}
        }
    }
}

/// Grammatical validity of a structured rewrite (see the tier comment): the
/// structured vocabulary's own placement rules, checked recursively.
/// `loop_label` is the label ending the innermost enclosing `WhileTrue` body
/// within the same function — the only thing a `goto` may target, because a
/// label anywhere else would let the jump enter a local's scope (Lua only
/// exempts labels in end-of-block position).
fn structured_valid(stmts: &[Stmt]) -> bool {
    valid_block(stmts, None)
}

fn valid_block(stmts: &[Stmt], loop_label: Option<&str>) -> bool {
    let last = stmts.len().saturating_sub(1);
    stmts.iter().enumerate().all(|(i, s)| match s {
        // A bare return is only grammatical as the last statement of its
        // block (Lua's retstat rule).
        Stmt::ReturnNone => i == last,
        Stmt::Goto(l) => loop_label == Some(l.as_str()),
        // A label is only accepted where WhileTrue below consumed it; one
        // anywhere else is out of vocabulary.
        Stmt::Label(_) => false,
        Stmt::MultiAssign(lhs, exprs) => {
            !lhs.is_empty()
                && !exprs.is_empty()
                && lhs.iter().all(|l| is_plain_ident(l))
                && exprs
                    .iter()
                    .enumerate()
                    .all(|(j, e)| fits(e, arg_hole(j, exprs.len())))
                && valid_exprs(s)
        }
        Stmt::WhileTrue(b) => match b.0.split_last() {
            None => false,
            Some((Stmt::Label(l), init)) => valid_block(init, Some(l)),
            Some(_) => valid_block(&b.0, loop_label),
        },
        Stmt::If { then_b, elseifs, else_b, .. } => {
            valid_block(&then_b.0, loop_label)
                && elseifs.iter().all(|(_, b)| valid_block(&b.0, loop_label))
                && else_b.as_ref().is_none_or(|b| valid_block(&b.0, loop_label))
                && valid_exprs(s)
        }
        Stmt::Do(b) => valid_block(&b.0, loop_label),
        // A nested function body is a fresh goto scope.
        Stmt::Function { body, .. } => valid_block(&body.0, None),
        _ => valid_exprs(s),
    })
}

/// Function-literal bodies inside this statement's expressions are fresh
/// goto scopes and must themselves be valid.
fn valid_exprs(s: &Stmt) -> bool {
    let mut ok = true;
    s.for_each_expr(&mut |e| ok = ok && valid_expr(e));
    ok
}

fn valid_expr(e: &Expr) -> bool {
    match e {
        Expr::Func(_, body) => valid_block(body.stmts(), None),
        _ => expr_children(e).iter().all(|(c, _)| valid_expr(c)),
    }
}

fn render(e: &Expr) -> String {
    let mut s = String::new();
    e.render(0, &mut s);
    s
}

fn zip_refute(
    e: &Expr,
    hole: Hole,
    carried: &StampNode,
    fresh: &StampNode,
    check_residual_force: bool,
    v: &mut Vec<String>,
) {
    if !carried.stamp.no_stronger_than(&fresh.stamp) {
        v.push(format!(
            "stamp overclaim on `{}`: carried {:?}, analysis proves {:?}",
            render(e),
            carried.stamp,
            fresh.stamp
        ));
    }
    // A remaining force is a violation only where the collapse was
    // admissible: the argument WHNF-and-pure AND able to stand in the
    // call's hole (a `__force(<function literal>)` in callee position is
    // the emitter's prefixexp grouping, not a missed collapse).
    if check_residual_force
        && let Expr::Call(f, args) = e
        && matches!(f.as_ref(), Expr::Name(n) if n == "__force")
        && args.len() == 1
        && let Some(arg) = fresh.children.get(1)
        && arg.stamp.is_whnf()
        && arg.stamp.is_pure()
        && fits(&args[0], hole)
    {
        v.push(format!(
            "residual __force around a WHNF-and-pure expression: `{}`",
            render(e)
        ));
    }
    let kids = expr_children(e);
    if kids.len() != carried.children.len() || kids.len() != fresh.children.len() {
        v.push(format!(
            "internal: stamp mirror out of alignment under `{}`",
            render(e)
        ));
        return;
    }
    for (((k, kh), c), f) in kids.iter().zip(&carried.children).zip(&fresh.children) {
        zip_refute(k, *kh, c, f, check_residual_force, v);
    }
}

// ---- Canonical traversal ----

/// The hole class of a call/method argument at index `i` of `n`.
fn arg_hole(i: usize, n: usize) -> Hole {
    if i + 1 == n { Hole::DelimLast } else { Hole::Delim }
}

/// Canonical children of an expression with their hole classes, immutably
/// (mirrors `Walker::derive` arm for arm; a function literal's children are
/// its body's expression slots).
fn expr_children(e: &Expr) -> Vec<(&Expr, Hole)> {
    match e {
        Expr::Name(_) | Expr::Lit(_) | Expr::Raw(_) => Vec::new(),
        Expr::Paren(e) | Expr::Neg(e) => vec![(e.as_ref(), Hole::Grouped)],
        Expr::Call(f, args) => std::iter::once((f.as_ref(), Hole::Prefix))
            .chain(args.iter().enumerate().map(|(i, a)| (a, arg_hole(i, args.len()))))
            .collect(),
        Expr::Method(recv, _, args) => std::iter::once((recv.as_ref(), Hole::Prefix))
            .chain(args.iter().enumerate().map(|(i, a)| (a, arg_hole(i, args.len()))))
            .collect(),
        Expr::Index(base, _) => vec![(base.as_ref(), Hole::Prefix)],
        Expr::Binop(_, l, r) => vec![(l.as_ref(), Hole::Grouped), (r.as_ref(), Hole::Grouped)],
        Expr::Table(items) | Expr::TableSpaced(items) => {
            let n = items.len();
            items
                .iter()
                .enumerate()
                .map(|(i, item)| match item {
                    // Only the last POSITIONAL item spreads values.
                    Item::Pos(e) => (e, arg_hole(i, n)),
                    Item::KV(_, e) => (e, Hole::Delim),
                })
                .collect()
        }
        Expr::Func(_, body) => {
            let mut out = Vec::new();
            for s in body.stmts() {
                stmt_slots(s, &mut out);
            }
            out
        }
    }
}

/// A statement's expression slots with their hole classes, in canonical
/// order, sub-blocks and named function bodies included (mirrors
/// `Walker::stmt` arm for arm). Return operands are `DelimLast` even inside
/// thunk bodies (where pass 1 knows the position truncates): the coarser
/// class only declines rewrites, never mis-applies one.
fn stmt_slots<'a>(s: &'a Stmt, out: &mut Vec<(&'a Expr, Hole)>) {
    match s {
        Stmt::Raw(_) | Stmt::Local(_, None) => {}
        Stmt::Local(names, Some(e)) => {
            out.push((e, if names.len() == 1 { Hole::Delim } else { Hole::DelimLast }))
        }
        Stmt::Assign(_, e) => out.push((e, Hole::Delim)),
        Stmt::Return(e) => out.push((e, Hole::DelimLast)),
        Stmt::Expr(e) => out.push((e, Hole::Stmt)),
        Stmt::AssignIf { cond, then_e, else_e, .. } => {
            out.push((cond, Hole::Delim));
            out.push((then_e, Hole::Delim));
            out.push((else_e, Hole::Delim));
        }
        Stmt::If { cond, then_b, elseifs, else_b } => {
            out.push((cond, Hole::Delim));
            for s in &then_b.0 {
                stmt_slots(s, out);
            }
            for (c, b) in elseifs {
                out.push((c, Hole::Delim));
                for s in &b.0 {
                    stmt_slots(s, out);
                }
            }
            if let Some(b) = else_b {
                for s in &b.0 {
                    stmt_slots(s, out);
                }
            }
        }
        Stmt::Do(b) | Stmt::WhileTrue(b) => {
            for s in &b.0 {
                stmt_slots(s, out);
            }
        }
        // Multi-assignment RHS: the last expression's values spread across
        // the remaining lvalues, so it is the multi-value class; with as many
        // expressions as lvalues the extra values are discarded, but the
        // coarser class only declines rewrites, never mis-applies one (same
        // note as thunk-body returns above).
        Stmt::MultiAssign(_, exprs) => {
            let n = exprs.len();
            for (i, e) in exprs.iter().enumerate() {
                out.push((e, arg_hole(i, n)));
            }
        }
        Stmt::ReturnNone | Stmt::Goto(_) | Stmt::Label(_) => {}
        Stmt::Function { body, .. } => {
            for s in &body.0 {
                stmt_slots(s, out);
            }
        }
        Stmt::ReturnTable(entries) => {
            for (_, e) in entries {
                out.push((e, Hole::Delim));
            }
        }
    }
}

/// Extract canonical child `i` from an owned expression (the
/// `ReplaceWithChild` application). Only child-bearing value shapes are
/// extractable; a function literal's "children" are body slots, not
/// replacements, so requesting one is an engine-contract violation.
fn nth_child_owned(e: Expr, i: usize) -> Expr {
    match e {
        Expr::Paren(b) | Expr::Neg(b) if i == 0 => *b,
        Expr::Index(b, _) if i == 0 => *b,
        Expr::Call(f, args) | Expr::Method(f, _, args) => {
            if i == 0 {
                *f
            } else {
                args.into_iter()
                    .nth(i - 1)
                    .expect("engine: ReplaceWithChild index out of range")
            }
        }
        Expr::Binop(_, l, r) => match i {
            0 => *l,
            1 => *r,
            _ => panic!("engine: ReplaceWithChild index out of range"),
        },
        Expr::Table(items) | Expr::TableSpaced(items) => {
            match items
                .into_iter()
                .nth(i)
                .expect("engine: ReplaceWithChild index out of range")
            {
                Item::Pos(e) | Item::KV(_, e) => e,
            }
        }
        _ => panic!("engine: node has no extractable canonical child {}", i),
    }
}

// ---- The analysis + rewrite walker ----

struct Walker<'p> {
    pass: &'p mut dyn ExprPass,
    dirty: bool,
}

type Env = HashMap<String, Stamp>;

impl Walker<'_> {
    /// A named-function (or module-top) scope: fresh facts, empty
    /// environment. Named nested functions do not inherit the enclosing
    /// scope's name stamps (their call time is unknown relative to later
    /// assignments only facts-qualification would rule out; starting empty
    /// keeps the reasoning per-scope, as the former pass did).
    fn function_scope(&mut self, stmts: &mut [Stmt]) -> Vec<StampNode> {
        let mut facts = ScopeFacts::default();
        collect_facts_block(stmts, &mut facts);
        let mut out = Vec::new();
        self.block(stmts, &facts, &Env::new(), &mut out);
        out
    }

    /// One statement sequence. The environment is cloned: names qualified
    /// inside a sub-block (an `if` arm, a `do` block) do not escape to the
    /// enclosing sequence, which may run without the sub-block having run.
    fn block(&mut self, stmts: &mut [Stmt], facts: &ScopeFacts, env: &Env, out: &mut Vec<StampNode>) {
        let mut env = env.clone();
        for s in stmts {
            self.stmt(s, facts, &mut env, out);
        }
    }

    fn stmt(&mut self, s: &mut Stmt, facts: &ScopeFacts, env: &mut Env, out: &mut Vec<StampNode>) {
        match s {
            Stmt::Raw(_) | Stmt::Local(_, None) => {}
            Stmt::Local(names, Some(e)) => {
                let hole = if names.len() == 1 { Hole::Delim } else { Hole::DelimLast };
                let node = self.expr(e, facts, env, hole);
                let stamp = node.stamp;
                out.push(node);
                if names.len() == 1
                    && stamp.shape() != Shape::Unknown
                    && facts.qualifies(&names[0], false)
                {
                    env.insert(names[0].clone(), Stamp::name_read(stamp.shape()));
                }
            }
            Stmt::Assign(lhs, e) => {
                let node = self.expr(e, facts, env, Hole::Delim);
                let stamp = node.stamp;
                out.push(node);
                if is_plain_ident(lhs)
                    && stamp.shape() != Shape::Unknown
                    && facts.qualifies(lhs, true)
                {
                    env.insert(lhs.clone(), Stamp::name_read(stamp.shape()));
                }
            }
            Stmt::Return(e) => {
                let node = self.expr(e, facts, env, Hole::DelimLast);
                out.push(node);
            }
            Stmt::Expr(e) => {
                let node = self.expr(e, facts, env, Hole::Stmt);
                out.push(node);
            }
            Stmt::AssignIf { cond, then_e, else_e, .. } => {
                let n = self.expr(cond, facts, env, Hole::Delim);
                out.push(n);
                let n = self.expr(then_e, facts, env, Hole::Delim);
                out.push(n);
                let n = self.expr(else_e, facts, env, Hole::Delim);
                out.push(n);
            }
            Stmt::If { cond, then_b, elseifs, else_b } => {
                let n = self.expr(cond, facts, env, Hole::Delim);
                out.push(n);
                self.block(&mut then_b.0, facts, env, out);
                for (c, b) in elseifs.iter_mut() {
                    let n = self.expr(c, facts, env, Hole::Delim);
                    out.push(n);
                    self.block(&mut b.0, facts, env, out);
                }
                if let Some(b) = else_b.as_mut() {
                    self.block(&mut b.0, facts, env, out);
                }
            }
            Stmt::Do(b) | Stmt::WhileTrue(b) => self.block(&mut b.0, facts, env, out),
            Stmt::MultiAssign(_, exprs) => {
                // No environment entries: a multi-assigned name has at least
                // two binding sites by construction, so it never qualifies.
                let n = exprs.len();
                for (i, e) in exprs.iter_mut().enumerate() {
                    let node = self.expr(e, facts, env, arg_hole(i, n));
                    out.push(node);
                }
            }
            Stmt::ReturnNone | Stmt::Goto(_) | Stmt::Label(_) => {}
            Stmt::Function { body, .. } => {
                let nodes = self.function_scope(&mut body.0);
                out.extend(nodes);
            }
            Stmt::ReturnTable(entries) => {
                for (_, e) in entries {
                    let n = self.expr(e, facts, env, Hole::Delim);
                    out.push(n);
                }
            }
        }
    }

    /// Derive the node's stamps bottom-up, then offer the pass rewrites
    /// until it stops requesting them. A request whose replacement cannot
    /// stand in the node's hole (see `fits`) is declined — the engine, not
    /// the pass, owns grammatical validity of the applied tree.
    fn expr(&mut self, e: &mut Expr, facts: &ScopeFacts, env: &Env, hole: Hole) -> StampNode {
        let mut node = self.derive(e, facts, env);
        loop {
            let Some(req) = self.pass.request(e, &StampView { node: &node }) else {
                return node;
            };
            match req {
                Request::ReplaceWithChild(i) => {
                    let kids = expr_children(e);
                    assert!(i < kids.len(), "engine: ReplaceWithChild index out of range");
                    if !fits(kids[i].0, hole) {
                        return node;
                    }
                    let old = std::mem::replace(e, Expr::Lit(String::new()));
                    *e = nth_child_owned(old, i);
                    node = node.children.swap_remove(i);
                }
                Request::Replace(new_e, justify) => {
                    if !fits(&new_e, hole) {
                        return node;
                    }
                    let stamp = match justify {
                        Justify::MeetOfChildren(idxs) => idxs
                            .iter()
                            .map(|&i| node.children[i].stamp)
                            .reduce(|a, b| a.meet(&b))
                            .unwrap_or_else(Stamp::unknown),
                        Justify::Unknown => Stamp::unknown(),
                    };
                    *e = new_e;
                    let mut n = StampNode::unknown_for(e);
                    n.stamp = stamp;
                    self.dirty = true;
                    // The replacement's subtree was never offered to the
                    // pass and carries no derived stamps; the post-pass
                    // recomputation covers it.
                    return n;
                }
            }
        }
    }

    fn derive(&mut self, e: &mut Expr, facts: &ScopeFacts, env: &Env) -> StampNode {
        match e {
            // A name read never traps; its shape is known only for
            // qualified single-assignment locals. Non-identifier spellings
            // (`_v[3]`, `math.pi`) go through table indexing, which may hit
            // metamethods: no claims.
            Expr::Name(s) => StampNode::leaf(if is_plain_ident(s) {
                match env.get(s) {
                    Some(st) => Stamp::name_read(st.shape()),
                    None => Stamp::new(Shape::Unknown, true, false, false),
                }
            } else {
                Stamp::unknown()
            }),
            Expr::Lit(_) => StampNode::leaf(Stamp::scalar()),
            Expr::Raw(_) => StampNode::leaf(Stamp::unknown()),
            Expr::Paren(inner) => {
                let c = self.expr(inner, facts, env, Hole::Grouped);
                StampNode { stamp: c.stamp, children: vec![c] }
            }
            // `-e` can invoke `__unm` on a non-number, whose result and
            // effects are arbitrary.
            Expr::Neg(inner) => {
                let c = self.expr(inner, facts, env, Hole::Grouped);
                StampNode { stamp: Stamp::unknown(), children: vec![c] }
            }
            Expr::Call(..) => {
                let Expr::Call(f, args) = e else { unreachable!() };
                let cf = self.expr(f, facts, env, Hole::Prefix);
                let n = args.len();
                let cargs: Vec<StampNode> = args
                    .iter_mut()
                    .enumerate()
                    .map(|(i, a)| self.expr(a, facts, env, arg_hole(i, n)))
                    .collect();
                let stamp = call_stamp(f, &cargs);
                let mut children = vec![cf];
                children.extend(cargs);
                StampNode { stamp, children }
            }
            Expr::Method(recv, _, args) => {
                let mut children = vec![self.expr(recv, facts, env, Hole::Prefix)];
                let n = args.len();
                for (i, a) in args.iter_mut().enumerate() {
                    children.push(self.expr(a, facts, env, arg_hole(i, n)));
                }
                StampNode { stamp: Stamp::unknown(), children }
            }
            // Indexing can hit `__index` metamethods: no claims.
            Expr::Index(base, _) => {
                let c = self.expr(base, facts, env, Hole::Prefix);
                StampNode { stamp: Stamp::unknown(), children: vec![c] }
            }
            Expr::Binop(..) => {
                let Expr::Binop(op, l, r) = e else { unreachable!() };
                let cl = self.expr(l, facts, env, Hole::Grouped);
                let cr = self.expr(r, facts, env, Hole::Grouped);
                let stamp = binop_stamp(op, &cl.stamp, &cr.stamp);
                StampNode { stamp, children: vec![cl, cr] }
            }
            // A table constructor is WHNF (the value IS the table) and
            // allocates; its own construction cannot trap, the children's
            // evaluation might.
            Expr::Table(..) | Expr::TableSpaced(..) => {
                let items = match e {
                    Expr::Table(items) | Expr::TableSpaced(items) => items,
                    _ => unreachable!(),
                };
                let n = items.len();
                let mut children = Vec::new();
                for (i, item) in items.iter_mut().enumerate() {
                    match item {
                        Item::Pos(e) => {
                            children.push(self.expr(e, facts, env, arg_hole(i, n)))
                        }
                        Item::KV(_, e) => {
                            children.push(self.expr(e, facts, env, Hole::Delim))
                        }
                    }
                }
                let mut stamp = Stamp::new(Shape::Whnf, true, false, true);
                for c in &children {
                    stamp = stamp.absorb_effects(&c.stamp);
                }
                StampNode { stamp, children }
            }
            // A function literal evaluates to a fresh closure: pure, cannot
            // trap, allocates. The body's stamps are derived under this
            // scope's facts with the literal's parameters shadowed out —
            // capture of a qualified single-assignment local is safe (its
            // referent never becomes a thunk no matter when the closure
            // runs).
            Expr::Func(params, body) => {
                let mut inner = env.clone();
                for p in params.iter() {
                    inner.remove(p);
                }
                let mut children = Vec::new();
                self.block(body.stmts_mut(), facts, &inner, &mut children);
                StampNode {
                    stamp: Stamp::new(Shape::Closure, true, false, true),
                    children,
                }
            }
        }
    }
}

/// Stamp of a call, from its callee spelling and argument stamps. Only the
/// runtime helpers whose contracts this module owns knowledge of get more
/// than unknown; every other callee (host functions, compiled functions,
/// closures in locals) makes no promises the emitted tree can prove.
fn call_stamp(f: &Expr, args: &[StampNode]) -> Stamp {
    match f {
        // `__force` returns a non-thunk by the emitter's invariant that
        // thunk bodies return forced values (the axiom the former
        // force-of-WHNF-locals pass relied on). Forcing may run arbitrary
        // suspended code: every effect bit stays conservative.
        Expr::Name(n) if n == "__force" && args.len() == 1 => {
            Stamp::new(Shape::Whnf, false, true, true)
        }
        // A cons cell: builds one tagged table, forces nothing.
        Expr::Name(n) if (n == "__mll_cons" || n == "__mll_lazy_cons") && args.len() == 2 => {
            let mut stamp = Stamp::new(Shape::Cons, true, false, true);
            for a in args {
                stamp = stamp.absorb_effects(&a.stamp);
            }
            stamp
        }
        // A suspension wrapper: builds one tagged table around the (already
        // evaluated) closure argument, runs nothing.
        Expr::Name(n) if n == "__thunk" && args.len() == 1 => {
            let mut stamp = Stamp::new(Shape::Thunk, true, false, true);
            for a in args {
                stamp = stamp.absorb_effects(&a.stamp);
            }
            stamp
        }
        _ => Stamp::unknown(),
    }
}

fn binop_stamp(op: &str, l: &Stamp, r: &Stamp) -> Stamp {
    match op {
        // `and`/`or` return one of their operands and have no metamethods:
        // shape is the operands' meet, effects are theirs. (Short-circuit
        // may skip the right operand; counting its effects anyway only
        // weakens.)
        "and" | "or" => {
            Stamp::new(l.shape().meet(r.shape()), true, false, false)
                .absorb_effects(l)
                .absorb_effects(r)
        }
        // Comparison results are converted to booleans by Lua even through
        // `__eq`/`__lt`/`__le`, so the shape is WHNF; the metamethods
        // themselves may do anything, so no effect claims.
        "==" | "~=" | "<" | "<=" | ">" | ">=" => Stamp::new(Shape::Whnf, false, true, true),
        // Arithmetic/concat/bitwise may dispatch to metamethods whose
        // result is arbitrary.
        _ => Stamp::unknown(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::lua::FuncBody;

    fn force(e: Expr) -> Expr {
        Expr::force(e)
    }

    /// Stamp of the sole top-level slot of `stmts`.
    fn only_stamp(stmts: &mut [Stmt]) -> Stamp {
        let engine = Engine::analyze(stmts);
        assert_eq!(engine.roots.len(), 1);
        engine.roots[0].stamp
    }

    #[test]
    fn literal_is_whnf_and_pure() {
        let mut stmts = vec![Stmt::Return(Expr::lit("42"))];
        let st = only_stamp(&mut stmts);
        assert_eq!(st.shape(), Shape::Whnf);
        assert!(st.is_pure() && !st.may_trap() && !st.may_allocate());
    }

    #[test]
    fn raw_is_unknown() {
        let mut stmts = vec![Stmt::Return(Expr::raw("anything at all"))];
        let st = only_stamp(&mut stmts);
        assert_eq!(st.shape(), Shape::Unknown);
        assert!(!st.is_pure() && st.may_trap() && st.may_allocate());
    }

    #[test]
    fn thunk_cons_closure_shapes() {
        let mut stmts = vec![Stmt::Return(Expr::thunk(Expr::lit("1")))];
        assert_eq!(only_stamp(&mut stmts).shape(), Shape::Thunk);

        let mut stmts = vec![Stmt::Return(Expr::call_named(
            "__mll_cons",
            vec![Expr::lit("1"), Expr::lit("nil")],
        ))];
        let st = only_stamp(&mut stmts);
        assert_eq!(st.shape(), Shape::Cons);
        assert!(st.is_whnf() && st.is_pure() && st.may_allocate());

        let mut stmts = vec![Stmt::Return(Expr::inline_fn0(Expr::lit("1")))];
        assert_eq!(only_stamp(&mut stmts).shape(), Shape::Closure);
    }

    #[test]
    fn unknown_call_makes_no_claims() {
        let mut stmts = vec![Stmt::Return(Expr::call_named("f", vec![Expr::lit("1")]))];
        assert_eq!(only_stamp(&mut stmts).shape(), Shape::Unknown);
    }

    #[test]
    fn qualified_local_name_is_whnf() {
        // local x = __force(y); return x  — x single-assignment, WHNF rhs.
        let mut stmts = vec![
            Stmt::Local(vec!["x".into()], Some(force(Expr::name("y")))),
            Stmt::Return(Expr::name("x")),
        ];
        let engine = Engine::analyze(&mut stmts);
        assert_eq!(engine.roots[1].stamp.shape(), Shape::Whnf);
        assert!(engine.roots[1].stamp.is_pure());
    }

    #[test]
    fn raw_mention_poisons_name() {
        let mut stmts = vec![
            Stmt::Local(vec!["x".into()], Some(force(Expr::name("y")))),
            Stmt::Raw("x = nil".into()),
            Stmt::Return(Expr::name("x")),
        ];
        let engine = Engine::analyze(&mut stmts);
        assert_eq!(engine.roots[1].stamp.shape(), Shape::Unknown);
    }

    #[test]
    fn reassigned_name_does_not_qualify() {
        let mut stmts = vec![
            Stmt::Local(vec!["x".into()], Some(force(Expr::name("y")))),
            Stmt::Assign("x".into(), Expr::call_named("f", vec![])),
            Stmt::Return(Expr::name("x")),
        ];
        let engine = Engine::analyze(&mut stmts);
        assert_eq!(engine.roots[2].stamp.shape(), Shape::Unknown);
    }

    #[test]
    fn shadowed_param_drops_the_stamp() {
        // local x = __force(y); return function(x) return x end — the inner
        // x is a parameter, not the qualified local.
        let mut stmts = vec![
            Stmt::Local(vec!["x".into()], Some(force(Expr::name("y")))),
            Stmt::Return(Expr::Func(
                vec!["x".into()],
                FuncBody::Inline(vec![Stmt::Return(Expr::name("x"))]),
            )),
        ];
        let engine = Engine::analyze(&mut stmts);
        // Func node's child is the body's return slot.
        assert_eq!(engine.roots[1].children[0].stamp.shape(), Shape::Unknown);
    }

    #[test]
    fn monotonicity_order_and_meet() {
        use Shape::*;
        for s in [Whnf, Cons, Closure, Thunk, Unknown] {
            assert!(s.no_stronger_than(s));
            assert!(Unknown.no_stronger_than(s));
            assert_eq!(s.meet(s), s);
            assert_eq!(s.meet(Unknown), Unknown);
            // meet is a weakening of both operands.
            for t in [Whnf, Cons, Closure, Thunk, Unknown] {
                assert_eq!(s.meet(t), t.meet(s));
                assert!(s.meet(t).no_stronger_than(s));
                assert!(s.meet(t).no_stronger_than(t));
            }
        }
        assert!(Whnf.no_stronger_than(Cons));
        assert!(Whnf.no_stronger_than(Closure));
        assert!(!Cons.no_stronger_than(Whnf));
        assert!(!Thunk.no_stronger_than(Whnf));
        assert!(!Whnf.no_stronger_than(Thunk));
        assert_eq!(Cons.meet(Closure), Whnf);
        assert_eq!(Whnf.meet(Thunk), Unknown);

        let strong = Stamp::scalar();
        let weak = Stamp::unknown();
        assert!(weak.no_stronger_than(&strong));
        assert!(!strong.no_stronger_than(&weak));
        assert!(strong.meet(&weak).no_stronger_than(&strong));
    }

    /// A pass replacing a node with an arbitrary expression cannot pick the
    /// result's stamp: the only justifications are inherit (by
    /// construction), meet, and unknown — and a `Replace` forces a full
    /// recomputation, so the stamp the engine ends up carrying is the
    /// analysis's, not the pass's.
    #[test]
    fn write_monopoly_replace_cannot_strengthen() {
        struct SwapLitForRaw;
        impl ExprPass for SwapLitForRaw {
            fn request(&mut self, e: &Expr, _: &StampView<'_>) -> Option<Request> {
                match e {
                    Expr::Lit(s) if s == "42" => {
                        Some(Request::Replace(Expr::raw("42"), Justify::Unknown))
                    }
                    _ => None,
                }
            }
        }
        // A Delim hole (single-local RHS), where a Raw replacement fits.
        let mut stmts = vec![Stmt::Local(vec!["x".into()], Some(Expr::lit("42")))];
        let engine = Engine::run_pass(&mut stmts, &mut SwapLitForRaw);
        // The tree now holds Raw("42"); the recomputed stamp is unknown even
        // though the replaced literal was WHNF-and-pure.
        assert!(matches!(&stmts[0], Stmt::Local(_, Some(Expr::Raw(_)))));
        assert_eq!(engine.roots[0].stamp.shape(), Shape::Unknown);
        assert!(engine.refute(&stmts, true).is_empty());
    }

    /// The inherit justification carries exactly the source child's stamp.
    #[test]
    fn replace_with_child_inherits() {
        struct CollapseForce;
        impl ExprPass for CollapseForce {
            fn request(&mut self, e: &Expr, st: &StampView<'_>) -> Option<Request> {
                let Expr::Call(f, args) = e else { return None };
                if matches!(f.as_ref(), Expr::Name(n) if n == "__force")
                    && args.len() == 1
                    && st.child(1)?.stamp().is_whnf()
                {
                    return Some(Request::ReplaceWithChild(1));
                }
                None
            }
        }
        // Nested forces collapse in one run (the resulting node is
        // re-offered).
        let mut stmts = vec![Stmt::Return(force(force(Expr::lit("7"))))];
        let engine = Engine::run_pass(&mut stmts, &mut CollapseForce);
        assert!(matches!(&stmts[0], Stmt::Return(Expr::Lit(s)) if s == "7"));
        assert_eq!(engine.roots[0].stamp.shape(), Shape::Whnf);
        assert!(engine.refute(&stmts, true).is_empty());
    }

    /// A collapse whose replacement cannot stand in the hole is declined:
    /// the emitter uses `__force(<function literal>)` as its prefixexp
    /// grouping in callee position, and stripping it would emit invalid
    /// Lua (`function() … end(x)` is not a prefixexp call).
    #[test]
    fn prefix_position_collapse_declined() {
        struct Collapse;
        impl ExprPass for Collapse {
            fn request(&mut self, e: &Expr, st: &StampView<'_>) -> Option<Request> {
                let Expr::Call(f, args) = e else { return None };
                if matches!(f.as_ref(), Expr::Name(n) if n == "__force")
                    && args.len() == 1
                    && st.child(1)?.stamp().is_whnf()
                {
                    return Some(Request::ReplaceWithChild(1));
                }
                None
            }
        }
        let callee = force(Expr::inline_fn0(Expr::lit("1")));
        let mut stmts = vec![Stmt::Return(Expr::call(callee, vec![Expr::name("x")]))];
        let engine = Engine::run_pass(&mut stmts, &mut Collapse);
        let Stmt::Return(Expr::Call(f, _)) = &stmts[0] else { panic!("shape") };
        assert!(matches!(f.as_ref(), Expr::Call(..)), "__force must survive");
        assert!(
            engine.refute(&stmts, true).is_empty(),
            "a declined site is not a residual-force violation"
        );
    }

    #[test]
    fn refute_reports_residual_force() {
        let mut stmts = vec![Stmt::Return(force(Expr::lit("42")))];
        let engine = Engine::analyze(&mut stmts);
        let v = engine.refute(&stmts, true);
        assert_eq!(v.len(), 1, "violations: {:?}", v);
        assert!(v[0].contains("__force(42)"), "{}", v[0]);
        // The residual check belongs to the collapse pass; with it disabled
        // the tree is fine.
        assert!(engine.refute(&stmts, false).is_empty());
    }

    #[test]
    fn refute_reports_overclaim() {
        let mut stmts = vec![Stmt::Return(Expr::raw("anything"))];
        let mut engine = Engine::analyze(&mut stmts);
        // Forge a stronger stamp than the analysis proves (test-only access
        // to the private mirror; no public path can do this).
        engine.roots[0].stamp = Stamp::scalar();
        let v = engine.refute(&stmts, true);
        assert_eq!(v.len(), 1, "violations: {:?}", v);
        assert!(v[0].contains("overclaim") && v[0].contains("anything"), "{}", v[0]);
    }

    /// A structured pass cannot emit an invalid tree: the engine validates
    /// the rewritten body against the structured vocabulary's placement
    /// rules and declines the whole rewrite otherwise.
    #[test]
    fn structured_rewrite_validation() {
        struct Rewrite(Vec<Stmt>);
        impl StructuredPass for Rewrite {
            fn request(
                &mut self,
                _t: &FnTarget,
                _p: &[String],
                _b: &Block,
                _v: &ScopeView<'_>,
                _l: &HashSet<String>,
            ) -> Option<Block> {
                Some(Block(self.0.clone()))
            }
        }
        let module = || {
            vec![Stmt::Function {
                target: FnTarget::LocalFn("f".into()),
                params: vec!["x".into()],
                body: Block(vec![Stmt::Return(Expr::lit("1"))]),
            }]
        };

        // A goto without its loop-ending label: declined, tree untouched.
        let mut stmts = module();
        let bad = vec![Stmt::WhileTrue(Block(vec![Stmt::Goto("continue".into())]))];
        assert!(Engine::run_structured(&mut stmts, &mut Rewrite(bad)).is_none());
        let Stmt::Function { body, .. } = &stmts[0] else { panic!("shape") };
        assert!(matches!(body.0[0], Stmt::Return(_)), "declined rewrite must not apply");

        // A bare return that is not last in its block: declined.
        let mut stmts = module();
        let bad = vec![Stmt::WhileTrue(Block(vec![
            Stmt::ReturnNone,
            Stmt::Expr(Expr::call_named("print", vec![])),
        ]))];
        assert!(Engine::run_structured(&mut stmts, &mut Rewrite(bad)).is_none());

        // The designed shape passes validation and is applied; the returned
        // engine's mirror aligns with the converted tree.
        let mut stmts = module();
        let good = vec![Stmt::WhileTrue(Block(vec![
            Stmt::Local(vec!["_w0".into()], Some(Expr::name("x"))),
            Stmt::If {
                cond: Expr::name("_w0"),
                then_b: Block(vec![Stmt::Return(Expr::lit("1"))]),
                elseifs: vec![],
                else_b: None,
            },
            Stmt::MultiAssign(vec!["x".into()], vec![Expr::lit("2")]),
            Stmt::Goto("continue".into()),
            Stmt::Do(Block(vec![Stmt::ReturnNone])),
            Stmt::Label("continue".into()),
        ]))];
        let engine =
            Engine::run_structured(&mut stmts, &mut Rewrite(good)).expect("valid rewrite applies");
        assert!(engine.refute(&stmts, false).is_empty());
    }

    /// The mirror and the canonical enumeration stay aligned across every
    /// statement shape.
    #[test]
    fn mirror_alignment_over_all_statement_shapes() {
        let mut stmts = vec![
            Stmt::Raw("-- raw".into()),
            Stmt::Local(vec!["a".into()], None),
            Stmt::Local(vec!["b".into()], Some(Expr::lit("1"))),
            Stmt::Assign("a".into(), Expr::binop("+", Expr::name("b"), Expr::lit("2"))),
            Stmt::AssignIf {
                lhs: "a".into(),
                cond: Expr::binop("==", Expr::name("b"), Expr::lit("1")),
                then_e: Expr::lit("1"),
                else_e: Expr::lit("2"),
            },
            Stmt::If {
                cond: Expr::name("a"),
                then_b: Block(vec![Stmt::Expr(Expr::call_named("print", vec![Expr::name("a")]))]),
                elseifs: vec![(Expr::name("b"), Block(vec![Stmt::Return(Expr::lit("nil"))]))],
                else_b: Some(Block(vec![Stmt::Do(Block(vec![Stmt::Return(Expr::Table(vec![
                    Item::Pos(Expr::lit("1")),
                    Item::KV("k = ".into(), Expr::raw("host()")),
                ]))]))])),
            },
            Stmt::Function {
                target: FnTarget::LocalFn("go".into()),
                params: vec!["n".into()],
                body: Block(vec![Stmt::Return(Expr::method(
                    Expr::name("n"),
                    "fmt",
                    vec![Expr::neg(Expr::lit("3")), Expr::index(Expr::name("n"), "[1]")],
                ))]),
            },
            Stmt::ReturnTable(vec![("go".into(), Expr::name("go"))]),
        ];
        let engine = Engine::analyze(&mut stmts);
        assert!(engine.refute(&stmts, true).is_empty());
    }
}
