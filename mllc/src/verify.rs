//! Post-monomorphization invariant check.
//!
//! Monomorphization has the static type of every expression. For the
//! type-directed classes (`show`, `==`, …) it is supposed to resolve every
//! concrete-typed use to a typed implementation (`show_Integer`,
//! `show_LInteger`, a derived `show_Tree_…`, etc.). When it fails to, the call
//! falls back to the *type-erased* runtime function (`show`, `__mll_eq`), which
//! cannot recover structure from the value alone — that is exactly how an empty
//! list printed as "Nothing", `Just 5` printed as "5", and derived Show printed
//! numeric tags.
//!
//! This pass makes that failure loud: it walks the final TIR and reports any
//! call to a *type-erased* show — the bare class method `show` or one of the
//! generic runtime wrappers `show_Maybe`/`show_List_` (which are defined as
//! `\x -> show x`) — applied at a *concrete structured* type. At a concrete
//! type, monomorphization should have resolved to a *specialized* show (a
//! `__mll_show_list`/`__mll_show_maybe` thread, or a per-type derived
//! `show_Tree_…`); reaching a type-erased one instead is a compiler bug, so the
//! driver turns it into a hard error rather than emitting known-wrong code.
//!
//! Primitives are exempt: the generic runtime `show` is faithful for
//! `Integer`/`Number`/`Bool`/`String`/`ByteString`, so `show_Integer` & co. are
//! fine. Only *structured* types (lists, tuples, and applied type constructors
//! like `Maybe a`, `Tree a b`) lose information, and only those are flagged.
//!
//! Scope: this is the resolution-layer seam. The `print x = putStrLn (show x)`
//! erasure is handled structurally (print is a normal prelude function, not a
//! codegen builtin, so its `show` specializes); a `show`/`eq` injected purely
//! in codegen would be below this pass and is out of its reach by construction.

use std::collections::HashSet;
use crate::tir::*;
use crate::types::Ty;

/// The type-erased show functions: the bare class method and the two generic
/// runtime wrappers that delegate to it. A concrete structured call must never
/// resolve to one of these. (`eq`/`==` belong here in principle but flow mostly
/// through `InfixApp`; left for a follow-up rather than asserted before it is
/// verified clean.)
const TYPE_ERASED_SHOWS: &[&str] = &["show", "show_Maybe", "show_List_"];

/// Check the module's invariants. Returns one message per violation; empty
/// means the module is clean. `class_methods` confirms `show` is in fact a
/// class method here (it always is once anything is shown).
pub fn check(module: &TModule, _class_methods: &HashSet<String>) -> Vec<String> {
    let erased: HashSet<&str> = TYPE_ERASED_SHOWS.iter().copied().collect();
    let mut v = Verifier { erased, violations: Vec::new() };
    for f in &module.functions {
        v.check_function(f);
    }
    for f in &module.instance_fns {
        v.check_function(f);
    }
    v.violations
}

struct Verifier {
    erased: HashSet<&'static str>,
    violations: Vec<String>,
}

impl Verifier {
    fn check_function(&mut self, f: &TFunction) {
        for clause in &f.clauses {
            for g in &clause.guards {
                self.walk(&g.condition, &f.name);
                self.walk(&g.body, &f.name);
            }
            self.walk(&clause.body, &f.name);
            for wb in &clause.where_binds {
                self.walk(&wb.body, &f.name);
            }
        }
    }

    fn walk(&mut self, e: &TExpr, ctx: &str) {
        // The violation itself: a type-erased show applied to a concrete,
        // structured argument. The instance is selected by the argument's type,
        // which is exactly this node's `arg`.
        if let TExprKind::App(func, arg) = &e.kind
            && let TExprKind::Var(name) = &func.kind
            && self.erased.contains(name.as_str())
            && is_lossy_concrete(&arg.ty)
        {
            self.violations.push(format!(
                "internal: type-erased '{}' applied at concrete type '{}' in '{}' \
                 — monomorphization should have resolved a specialized show",
                name, arg.ty, ctx
            ));
        }
        // Recurse into every sub-expression.
        match &e.kind {
            TExprKind::Var(_) | TExprKind::Con(_) | TExprKind::Lit(_)
            | TExprKind::OpFunc(_) | TExprKind::DictAccess { .. } => {}
            TExprKind::DictMethod { dict, .. } => self.walk(dict, ctx),
            TExprKind::App(a, b) => { self.walk(a, ctx); self.walk(b, ctx); }
            TExprKind::Lambda { body, .. } => self.walk(body, ctx),
            TExprKind::InfixApp { lhs, rhs, .. } => { self.walk(lhs, ctx); self.walk(rhs, ctx); }
            TExprKind::Negate(x) | TExprKind::Paren(x) => self.walk(x, ctx),
            TExprKind::If { cond, then_branch, else_branch } => {
                self.walk(cond, ctx); self.walk(then_branch, ctx); self.walk(else_branch, ctx);
            }
            TExprKind::Case { scrutinee, branches } => {
                self.walk(scrutinee, ctx);
                for b in branches {
                    for g in &b.guards { self.walk(&g.condition, ctx); self.walk(&g.body, ctx); }
                    self.walk(&b.body, ctx);
                }
            }
            TExprKind::Let { binds, body } => {
                for b in binds { self.walk(&b.body, ctx); }
                self.walk(body, ctx);
            }
            TExprKind::SpecCall { args, .. } => { for a in args { self.walk(a, ctx); } }
            TExprKind::Tuple(elems) => { for x in elems { self.walk(x, ctx); } }
            TExprKind::DictCall { dict_args, value_args, .. } => {
                for a in dict_args { self.walk(a, ctx); }
                for a in value_args { self.walk(a, ctx); }
            }
            TExprKind::RecordUpdate { record, updates, .. } => {
                self.walk(record, ctx);
                for (_, _, val) in updates { self.walk(val, ctx); }
            }
            TExprKind::OutgoingCallback { callee, .. } => self.walk(callee, ctx),
            TExprKind::FfiMaybeArg { value } => self.walk(value, ctx),
        }
    }
}

/// A concrete type whose generic (type-erased) show/eq would lose information:
/// lists, tuples, and applied type constructors (`Maybe a`, `Tree a b`).
/// Bare `Con` primitives are faithful under the generic runtime impl, so they
/// are not flagged.
fn is_lossy_concrete(ty: &Ty) -> bool {
    if !ty.free_vars().is_empty() {
        return false;
    }
    matches!(ty, Ty::List(_) | Ty::Tuple(_) | Ty::App(_, _))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TyVar;

    fn show_call(name: &str, arg_ty: Ty) -> TExpr {
        let func = TExpr::new(
            TExprKind::Var(name.to_string()),
            Ty::arrow(arg_ty.clone(), Ty::Con("String".into())),
        );
        let arg = TExpr::new(TExprKind::Var("x".into()), arg_ty);
        TExpr::new(
            TExprKind::App(Box::new(func), Box::new(arg)),
            Ty::Con("String".into()),
        )
    }

    fn module_with(body: TExpr) -> TModule {
        TModule {
            data_defs: vec![],
            dropped_data_defs: vec![],
            functions: vec![TFunction {
                name: "f".into(),
                ty: Ty::Con("String".into()),
                clauses: vec![TClause {
                    patterns: vec![], guards: vec![], body, where_binds: vec![], span: None,
                }],
                specialized: false,
                dict_params: vec![],
            }],
            instance_fns: vec![],
            has_main: false,
            exports: vec![],
            record_accessors: vec![],
            newtypes: vec![],
        }
    }

    fn cm() -> HashSet<String> {
        ["show".to_string()].into_iter().collect()
    }

    #[test]
    fn flags_type_erased_show_on_structured_concrete() {
        // bare `show` and the generic wrappers, at a concrete structured type
        for name in ["show", "show_Maybe", "show_List_"] {
            let m = module_with(show_call(name, Ty::list(Ty::Con("Integer".into()))));
            assert_eq!(check(&m, &cm()).len(), 1, "{name} on [Integer] should flag");
        }
        // Maybe Integer == App(Con Maybe, Integer)
        let maybe_int = Ty::app(Ty::Con("Maybe".into()), Ty::Con("Integer".into()));
        let m = module_with(show_call("show_Maybe", maybe_int));
        assert_eq!(check(&m, &cm()).len(), 1, "show_Maybe on Maybe Integer should flag");
    }

    #[test]
    fn allows_resolved_and_primitive_and_polymorphic() {
        // A specialized show name is fine even on a structured type.
        let m = module_with(show_call("show_LInteger", Ty::list(Ty::Con("Integer".into()))));
        assert!(check(&m, &cm()).is_empty(), "specialized show must not flag");

        // The generic `show` on a primitive is faithful — not flagged.
        let m = module_with(show_call("show", Ty::Con("Integer".into())));
        assert!(check(&m, &cm()).is_empty(), "primitive show must not flag");

        // A still-polymorphic argument is the legitimate fallback copy.
        let tv = Ty::Var(TyVar { name: "a".into(), id: u32::MAX });
        let m = module_with(show_call("show", Ty::list(tv)));
        assert!(check(&m, &cm()).is_empty(), "polymorphic show must not flag");
    }
}
