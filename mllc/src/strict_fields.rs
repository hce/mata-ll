//! Strict constructor fields (a TIR pass, the last step of type checking).
//!
//! Haskell 2010 §4.2.1: a constructor declared with a strictness flag,
//! `data P = P !Int Int`, evaluates the flagged field to weak head normal
//! form whenever it is applied — the report defines `P` as the function
//!
//! ```text
//! P = \x1 x2 -> x1 `seq` P' x1 x2          -- P' the lazy constructor
//! ```
//!
//! and this pass writes exactly that into the program, at every place a
//! strict constructor is an EXPRESSION:
//!
//! ```text
//! P e1 e2          let a = e1 in a `seq` P a e2                (saturated)
//! Q e1 e2 (both)   let a = e1 in a `seq` (let b = e2 in b `seq` Q a b)
//! P e1             let a = e1 in \b -> a `seq` P a b           (partial)
//! P                \a b -> a `seq` P a b                       (unapplied)
//! r { rx = e }     let v = e in v `seq` r { rx = v }           (record update)
//! ```
//!
//! The partial forms evaluate nothing until the last argument arrives — a
//! partial application is a function value, already in WHNF, as GHC's
//! wrapper `$WP` is — and the arguments given early are let-bound outside
//! the lambda so they are shared by every call of it.
//!
//! Why here and why this way. Strictness is a property of the
//! CONSTRUCTOR (`TConstructor::strict`, from the declaration), and "a
//! constructor is applied" has one form in the typed IR: a `Con` node,
//! applied or not. Record construction syntax is such an application by
//! the time the checker has ordered its fields, and so is everything a
//! derived instance builds (`fmap`'s rebuilt value, a decoded JSON value,
//! the generic representation's `to`): all of it is TIR in the checked
//! module, and all of it passes through here. After this pass a strict
//! field is evaluated by an ordinary `seq` over ordinary bindings, which
//! every later stage already understands — the monomorphizer, the folder,
//! the demand analysis (which reads the `seq` as the demand it is and
//! evaluates a cheap argument in place), the inliner, the WHNF mirror.
//! None of them learns that constructors can be strict, so none can
//! disagree with another about it; the raw `Con` application left inside
//! the `seq` is the lazy constructor they have always known.
//!
//! What the pass does NOT cover is a value that never was a `Con`
//! expression: a LuaDict record decoded from a host table at the FFI
//! boundary. The runtime decoder (`__mll_ffi_decode`) builds it from host
//! values, which are evaluated by nature, so a strict field holds there
//! too. Patterns are untouched: matching a strict constructor is matching
//! the lazy one. (That a matched strict field is known evaluated is not
//! claimed anywhere yet — reading it still goes through `__force`, which
//! returns an evaluated value as it is.)

use crate::tir::{
    TCaseBranch, TConFields, TExpr, TExprKind, TLiteral, TLocalDef, TModule, TPattern,
};
use crate::types::Ty;
use std::collections::HashMap;

/// One constructor with a strict field.
struct StrictCon {
    strict: Vec<bool>,
}

/// One named field's standing in a constructor of its type, for record
/// updates: an update names a field, and which constructor the updated
/// value has is known only at run time.
struct FieldIn {
    con: String,
    arity: usize,
    strict: bool,
}

struct Cx {
    cons: HashMap<String, StrictCon>,
    /// Field name -> its standing in every constructor that has it. Only
    /// for fields strict in at least one constructor.
    fields: HashMap<String, Vec<FieldIn>>,
    fresh: usize,
}

/// Make every construction of a strict constructor evaluate its strict
/// fields. See the module documentation.
pub fn lower(mut module: TModule) -> TModule {
    let mut cx = Cx { cons: HashMap::new(), fields: HashMap::new(), fresh: 0 };
    for def in &module.data_defs {
        if !def.constructors.iter().any(|c| c.strict.iter().any(|s| *s)) {
            continue;
        }
        for con in &def.constructors {
            let arity = match &con.fields {
                TConFields::Positional(f) => f.len(),
                TConFields::Named(f) => f.len(),
            };
            let strict: Vec<bool> =
                (0..arity).map(|i| con.strict.get(i).copied().unwrap_or(false)).collect();
            if let TConFields::Named(named) = &con.fields {
                for (i, f) in named.iter().enumerate() {
                    cx.fields.entry(f.name.clone()).or_default().push(FieldIn {
                        con: con.name.clone(),
                        arity,
                        strict: strict[i],
                    });
                }
            }
            if strict.iter().any(|s| *s) {
                cx.cons.insert(con.name.clone(), StrictCon { strict });
            }
        }
    }
    cx.fields.retain(|_, standing| standing.iter().any(|f| f.strict));
    if cx.cons.is_empty() {
        return module;
    }
    for f in module.functions.iter_mut().chain(module.instance_fns.iter_mut()) {
        for clause in &mut f.clauses {
            clause.map_exprs(&mut |e| cx.expr(e));
        }
    }
    module
}

fn seq(forced: TExpr, then: TExpr) -> TExpr {
    let ty = then.ty.clone();
    TExpr::new(
        TExprKind::InfixApp { op: "seq".to_string(), lhs: Box::new(forced), rhs: Box::new(then) },
        ty,
    )
}

/// An expression that may be written twice without being evaluated twice
/// or losing sharing: a variable, or a literal.
fn is_atom(e: &TExpr) -> bool {
    matches!(e.kind, TExprKind::Var(_) | TExprKind::Lit(_))
}

/// A literal that is a value as written — at a host type, where no
/// `fromInteger` stands between the digits and the value. (At any other
/// type, or one not yet known when this pass runs, an integer literal is
/// a call the monomorphizer resolves later and is evaluated like any
/// other expression.) Evaluating one is nothing, so no `seq` is written.
fn is_value_literal(e: &TExpr) -> bool {
    match &e.kind {
        TExprKind::Lit(TLiteral::Str(_) | TLiteral::Bool(_) | TLiteral::Unit) => true,
        TExprKind::Lit(_) => {
            matches!(&e.ty, Ty::Con(c) if matches!(c.as_str(), "Int" | "Integer" | "Number" | "Double"))
        }
        _ => false,
    }
}

fn let_in(binds: Vec<TLocalDef>, body: TExpr) -> TExpr {
    if binds.is_empty() {
        return body;
    }
    let ty = body.ty.clone();
    TExpr::new(TExprKind::Let { binds, body: Box::new(body) }, ty)
}

impl Cx {
    fn fresh(&mut self, what: &str) -> String {
        self.fresh += 1;
        format!("__sf{what}_{}", self.fresh)
    }

    /// `e` as an atom: itself when it is one, else a fresh variable bound
    /// to it in `binds`.
    fn atom(&mut self, e: TExpr, binds: &mut Vec<TLocalDef>) -> TExpr {
        if is_atom(&e) {
            return e;
        }
        let name = self.fresh("a");
        let var = TExpr::new(TExprKind::Var(name.clone()), e.ty.clone());
        binds.push(TLocalDef { name, patterns: Vec::new(), body: e });
        var
    }

    fn expr(&mut self, expr: TExpr) -> TExpr {
        // The application spine of a strict constructor is rewritten as a
        // whole, top down: the constructor's arguments are rewritten on
        // their own, and the raw application built from them is not
        // visited again.
        if self.strict_head(&expr) {
            let ty = expr.ty.clone();
            let mut args = Vec::new();
            let mut head = expr;
            while let TExprKind::App(f, a) = head.kind {
                args.push(self.expr(*a));
                head = *f;
            }
            args.reverse();
            return self.construct(head, args, ty);
        }
        let expr = expr.map_children(&mut |c| self.expr(c));
        match expr.kind {
            TExprKind::RecordUpdate { record, updates, num_fields } => {
                self.update(*record, updates, num_fields, expr.ty)
            }
            kind => TExpr { kind, ty: expr.ty },
        }
    }

    /// Is `expr` a strict constructor, bare or applied to arguments?
    fn strict_head(&self, expr: &TExpr) -> bool {
        let mut head = expr;
        while let TExprKind::App(f, _) = &head.kind {
            head = f;
        }
        matches!(&head.kind, TExprKind::Con(name) if self.cons.contains_key(name))
    }

    /// The strict constructor `head` (a `Con` node, kept as it is: the
    /// monomorphizer reads an existential constructor's declared type off
    /// it) applied to `args`; `ty` is the type of the whole application.
    fn construct(&mut self, head: TExpr, args: Vec<TExpr>, ty: Ty) -> TExpr {
        let TExprKind::Con(name) = &head.kind else { unreachable!("strict_head") };
        let strict = self.cons[name].strict.clone();
        let arity = strict.len();
        let given = args.len().min(arity);
        let saturated = given == arity;

        // The type of the application after i arguments, read off the
        // constructor's own type where that is its function type (a
        // derived instance writes the node at the RESULT type and types
        // only the finished application).
        let mut after: Vec<Option<Ty>> = Vec::with_capacity(arity + 1);
        let mut params_ty: Vec<Option<Ty>> = Vec::with_capacity(arity);
        let mut t = Some(&head.ty);
        for _ in 0..arity {
            match t {
                Some(Ty::Arrow(a, b, _)) => {
                    params_ty.push(Some((**a).clone()));
                    after.push(Some((**b).clone()));
                    t = Some(b);
                }
                _ => {
                    params_ty.push(None);
                    after.push(None);
                    t = None;
                }
            }
        }

        // Arguments given: a strict one must be nameable twice (once for
        // the `seq`, once as the field), so one that is not an atom is
        // let-bound. In a saturated application each such binding is
        // scoped over exactly its own `seq` and what follows it —
        //
        //     let a = e1 in a `seq` (let b = e2 in b `seq` P a b)
        //
        // — so every bound argument is the first thing its scope
        // evaluates (which is what lets the backend evaluate it in place
        // of allocating a suspension) and the fields are evaluated left
        // to right. In a partial application nothing is evaluated yet:
        // every argument moves out of the lambda, to be evaluated at most
        // once however often the lambda is called.
        let mut outer = Vec::new();
        let mut own: Vec<Option<TLocalDef>> = Vec::with_capacity(arity);
        let mut actuals: Vec<TExpr> = Vec::with_capacity(arity);
        let mut extra = Vec::new();
        for (i, arg) in args.into_iter().enumerate() {
            if i >= arity {
                extra.push(arg);
            } else if !saturated {
                actuals.push(self.atom(arg, &mut outer));
                own.push(None);
            } else if strict[i] {
                let mut bind = Vec::new();
                actuals.push(self.atom(arg, &mut bind));
                own.push(bind.pop());
            } else {
                actuals.push(arg);
                own.push(None);
            }
        }
        // Arguments still to come: the lambda's parameters.
        let mut params: Vec<(String, Ty)> = Vec::new();
        for pty in params_ty.iter().skip(given) {
            let name = self.fresh("p");
            let pty = pty.clone().unwrap_or(Ty::Unit);
            actuals.push(TExpr::new(TExprKind::Var(name.clone()), pty.clone()));
            params.push((name, pty));
            own.push(None);
        }

        let result_ty = if saturated {
            ty.clone()
        } else {
            after.last().cloned().flatten().unwrap_or(Ty::Unit)
        };
        let forced: Vec<Option<TExpr>> = actuals
            .iter()
            .enumerate()
            .map(|(i, a)| (strict[i] && !is_value_literal(a)).then(|| a.clone()))
            .collect();
        let mut built = head;
        for (i, actual) in actuals.into_iter().enumerate() {
            let node_ty = if i + 1 == arity {
                result_ty.clone()
            } else {
                after[i].clone().unwrap_or(Ty::Unit)
            };
            built = TExpr::new(TExprKind::App(Box::new(built), Box::new(actual)), node_ty);
        }
        for (f, bind) in forced.into_iter().zip(own).rev() {
            if let Some(f) = f {
                built = seq(f, built);
            }
            if let Some(bind) = bind {
                built = let_in(vec![bind], built);
            }
        }
        if !params.is_empty() {
            built = TExpr::new(TExprKind::Lambda { params, body: Box::new(built) }, ty.clone());
        }
        let mut out = let_in(outer, built);
        out.ty = ty;
        // A constructor has exactly its declared fields; arguments past
        // them cannot type-check, and are kept only so nothing is dropped.
        for arg in extra {
            out = TExpr::new(TExprKind::App(Box::new(out), Box::new(arg)), Ty::Unit);
        }
        out
    }

    /// A record update `record { f = e, … }`. GHC defines it by cases on
    /// the record's constructor, each alternative applying that
    /// constructor — so a strict field's new value is evaluated when the
    /// updated record is. Which fields are strict may differ between the
    /// constructors of one type (`A { x :: !Int } | B { x :: Int }`); when
    /// it does, the evaluation is decided by the constructor the record
    /// turns out to have.
    fn update(
        &mut self,
        record: TExpr,
        updates: Vec<(String, usize, TExpr)>,
        num_fields: usize,
        ty: Ty,
    ) -> TExpr {
        let raw = |record: TExpr, updates: Vec<(String, usize, TExpr)>, ty: Ty| {
            TExpr::new(
                TExprKind::RecordUpdate { record: Box::new(record), updates, num_fields },
                ty,
            )
        };
        if !updates.iter().any(|(f, _, _)| self.fields.contains_key(f)) {
            return raw(record, updates, ty);
        }
        // Per constructor that has a strict updated field: the positions
        // (in `updates`) of the fields strict in it, and its arity.
        let mut per_con: Vec<(String, usize, Vec<usize>)> = Vec::new();
        // Every constructor that has an updated field at all.
        let mut carriers: Vec<String> = Vec::new();
        for (u, (field, _, _)) in updates.iter().enumerate() {
            let Some(standing) = self.fields.get(field) else { continue };
            for f in standing {
                if !carriers.contains(&f.con) {
                    carriers.push(f.con.clone());
                }
                if !f.strict {
                    continue;
                }
                match per_con.iter_mut().find(|(c, _, _)| *c == f.con) {
                    Some((_, _, us)) => us.push(u),
                    None => per_con.push((f.con.clone(), f.arity, vec![u])),
                }
            }
        }
        let uniform = per_con.len() == carriers.len()
            && per_con.iter().all(|(_, _, us)| *us == per_con[0].2);

        let mut binds = Vec::new();
        let somewhere_strict: Vec<usize> = {
            let mut all: Vec<usize> = per_con.iter().flat_map(|(_, _, us)| us.clone()).collect();
            all.sort_unstable();
            all.dedup();
            all
        };
        let updates: Vec<(String, usize, TExpr)> = updates
            .into_iter()
            .enumerate()
            .map(|(u, (f, i, e))| {
                // In the by-cases form the update is written once per
                // alternative, so every new value must be an atom.
                if somewhere_strict.contains(&u) || !uniform {
                    (f, i, self.atom(e, &mut binds))
                } else {
                    (f, i, e)
                }
            })
            .collect();
        let forced_update = |us: &[usize], record: TExpr| {
            let mut out = raw(record, updates.clone(), ty.clone());
            for u in us.iter().rev() {
                if !is_value_literal(&updates[*u].2) {
                    out = seq(updates[*u].2.clone(), out);
                }
            }
            out
        };
        let body = if uniform {
            forced_update(&per_con[0].2, record)
        } else {
            let record = self.atom(record, &mut binds);
            let mut branches: Vec<TCaseBranch> = per_con
                .iter()
                .map(|(con, arity, us)| TCaseBranch {
                    pattern: TPattern::ctor(con.clone(), vec![TPattern::Wildcard; *arity]),
                    guards: Vec::new(),
                    body: Some(forced_update(us, record.clone())),
                })
                .collect();
            branches.push(TCaseBranch {
                pattern: TPattern::Wildcard,
                guards: Vec::new(),
                body: Some(forced_update(&[], record.clone())),
            });
            TExpr::new(
                TExprKind::Case { scrutinee: Box::new(record), branches },
                ty.clone(),
            )
        };
        let mut out = let_in(binds, body);
        out.ty = ty;
        out
    }
}
