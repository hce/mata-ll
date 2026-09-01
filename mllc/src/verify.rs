//! Post-monomorphization invariant check.
//!
//! Monomorphization has the static type of every expression. For the
//! type-directed classes (`show`, `==`, …) it is supposed to resolve every
//! concrete-typed use to a typed implementation (`show_Int`,
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
//! `Int`/`Number`/`Bool`/`String`/`ByteString`, so `show_Int` & co. are
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
/// runtime wrappers that delegate to it. A concrete structured call must
/// never resolve to one of these. Equality has NO entry here: at TIR level
/// the Eq method only ever appears as the `==`/`/=` operator (`InfixApp`
/// applied, `OpFunc` first-class) — both have their own checks in `walk` —
/// while a bare `eq` Var is an ordinary binder (`nubBy eq xs` in the
/// Prelude), which is exactly the false positive that fired the first time
/// `"eq"` was added to this list.
const TYPE_ERASED_METHODS: &[&str] = &["show", "show_Maybe", "show_List_"];

/// Check the module's invariants. Returns one message per violation; empty
/// means the module is clean.
pub fn check(module: &TModule) -> Vec<String> {
    // Pass-order witness: the invariant is checked on mono's OUTPUT,
    // before any pass rewrites it.
    debug_assert_eq!(
        module.passes_run,
        ["mono"],
        "verify::check must run directly on mono's output"
    );
    let erased: HashSet<&str> = TYPE_ERASED_METHODS.iter().copied().collect();
    let mut v = Verifier { erased, violations: Vec::new() };
    for f in &module.functions {
        v.check_function(f);
    }
    for f in &module.instance_fns {
        v.check_function(f);
    }
    v.violations
}

/// Stamp refutation over the emitted Lua tree — the output-side twin of
/// [`check`], active in test builds only (wired through
/// `compile_with_stamp_refutation` in lib.rs; production compiles never pay
/// for it). Re-runs codegen's module-body build and optimization passes,
/// then recomputes the annotation analysis fresh over the final tree and
/// reports (1) any stamp the engine carries that is stronger than the fresh
/// analysis proves — a justification overclaim — and (2) any remaining
/// `__force(e)` where the fresh analysis stamps `e` WHNF-and-pure — a
/// collapse the peephole owed. Each violation names the node's rendered
/// text. The check itself lives beside the Lua AST it walks
/// (`codegen::annot::Engine::refute`); this is its crate-facing entry.
pub fn check_stamps(module: &TModule) -> Vec<String> {
    // Pass-order witness: the refutation must see exactly the module
    // codegen will see, after every TIR pass.
    debug_assert_eq!(
        module.passes_run,
        ["mono", "fold", "split", "dce"],
        "verify::check_stamps must run on the final (post-dce) module"
    );
    crate::codegen::stamp_violations(module)
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
            if let Some(cb) = &clause.body { self.walk(cb, &f.name); }
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
        // `show $ x` applies through the operator, not an App node; `f . show`
        // hands the erased show to composition, which applies it to the
        // composition's argument. Both escape the App shape above (Q92).
        if let TExprKind::InfixApp { op, lhs, rhs } = &e.kind {
            // The equality OPERATOR at a structured type: mono rewrites
            // these to derived/synthetic implementations (eq_T, the
            // ListEq/MaybeEq/TupleEq SpecCalls); one surviving to codegen
            // would emit native Lua `==`, which compares TABLE IDENTITY —
            // `[1] == [1]` silently false. This was the A4 follow-up the
            // erased-show check deferred until verified clean; the whole
            // corpus and the backend fuzzer run clean against it.
            if (op == "==" || op == "/=") && is_lossy_concrete(&lhs.ty) {
                self.violations.push(format!(
                    "internal: structural '{}' at concrete type '{}' reached \
                     codegen unresolved in '{}' — monomorphization should \
                     have selected a derived or synthetic equality; native \
                     Lua == would compare table identity",
                    op, lhs.ty, ctx
                ));
            }
            if op == "$"
                && let TExprKind::Var(name) = &lhs.kind
                && self.erased.contains(name.as_str())
                && is_lossy_concrete(&rhs.ty)
            {
                self.violations.push(format!(
                    "internal: type-erased '{}' applied via '$' at concrete type '{}' in '{}' \
                     — monomorphization should have resolved a specialized show",
                    name, rhs.ty, ctx
                ));
            }
            if op == "." {
                // In `f . show` at `a -> c`, show's argument type is the
                // composition's own argument type; in `show . g`, it is g's
                // result — both sit inside the composition's TYPE, not on a
                // sibling node, so flag an erased name composed at a lossy
                // ARGUMENT/RESULT position via the operand's own fn type.
                for side in [lhs, rhs] {
                    if let TExprKind::Var(name) = &side.kind
                        && self.erased.contains(name.as_str())
                        && matches!(&side.ty, Ty::Arrow(from, _, _) if is_lossy_concrete(from))
                    {
                        self.violations.push(format!(
                            "internal: type-erased '{}' composed at concrete argument type '{}' in '{}' \
                             — monomorphization should have resolved a specialized show",
                            name, side.ty, ctx
                        ));
                    }
                }
            }
        }
        // First-class `(==)`/`(/=)` at a structured operand type — handed to
        // a higher-order consumer (`nubBy (==) points`): mono must have
        // replaced it with the derived/synthetic equality; the runtime
        // operator value compares table identity.
        if let TExprKind::OpFunc(op) = &e.kind
            && (op == "==" || op == "/=")
            && matches!(&e.ty, Ty::Arrow(from, _, _) if is_lossy_concrete(from))
        {
            self.violations.push(format!(
                "internal: first-class '{}' at concrete type '{}' reached \
                 codegen unresolved in '{}' — monomorphization should have \
                 selected a derived or synthetic equality; the runtime \
                 operator value would compare table identity",
                op, e.ty, ctx
            ));
        }
        // Recurse into every sub-expression.
        e.for_each_child(&mut |c| self.walk(c, ctx));
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
                    patterns: vec![], guards: vec![], body: Some(body), where_binds: vec![], span: None,
                }],
                specialized: false,
                spec_origin: None,
                dict_params: vec![],
                derived_strict: false,
            }],
            instance_fns: vec![],
            has_main: false,
            exports: vec![],
            record_accessors: vec![],
            newtypes: vec![],
            passes_run: vec!["mono"],
        }
    }

    #[test]
    fn flags_type_erased_show_through_dollar_and_compose() {
        // `show $ x` applies through the operator node; `f . show` hands the
        // erased show to composition — both escaped the App-only walker (Q92).
        let arg_ty = Ty::list(Ty::Con("Int".into()));
        let dollar = TExpr::new(
            TExprKind::InfixApp {
                op: "$".into(),
                lhs: Box::new(TExpr::new(
                    TExprKind::Var("show".into()),
                    Ty::arrow(arg_ty.clone(), Ty::Con("String".into())),
                )),
                rhs: Box::new(TExpr::new(TExprKind::Var("x".into()), arg_ty.clone())),
            },
            Ty::Con("String".into()),
        );
        assert_eq!(check(&module_with(dollar)).len(), 1, "show $ [Int] should flag");

        let compose = TExpr::new(
            TExprKind::InfixApp {
                op: ".".into(),
                lhs: Box::new(TExpr::new(
                    TExprKind::Var("g".into()),
                    Ty::arrow(Ty::Con("String".into()), Ty::Con("Int".into())),
                )),
                rhs: Box::new(TExpr::new(
                    TExprKind::Var("show".into()),
                    Ty::arrow(arg_ty.clone(), Ty::Con("String".into())),
                )),
            },
            Ty::arrow(arg_ty, Ty::Con("Int".into())),
        );
        assert_eq!(check(&module_with(compose)).len(), 1, "g . show at [Int] should flag");
    }

    #[test]
    fn flags_type_erased_show_on_structured_concrete() {
        // bare `show` and the generic wrappers, at a concrete structured type
        for name in ["show", "show_Maybe", "show_List_"] {
            let m = module_with(show_call(name, Ty::list(Ty::Con("Int".into()))));
            assert_eq!(check(&m).len(), 1, "{name} on [Int] should flag");
        }
        // Maybe Int == App(Con Maybe, Int)
        let maybe_int = Ty::app(Ty::Con("Maybe".into()), Ty::Con("Int".into()));
        let m = module_with(show_call("show_Maybe", maybe_int));
        assert_eq!(check(&m).len(), 1, "show_Maybe on Maybe Int should flag");
    }

    #[test]
    fn allows_resolved_and_primitive_and_polymorphic() {
        // A specialized show name is fine even on a structured type.
        let m = module_with(show_call("show_LInt", Ty::list(Ty::Con("Int".into()))));
        assert!(check(&m).is_empty(), "specialized show must not flag");

        // The generic `show` on a primitive is faithful — not flagged.
        let m = module_with(show_call("show", Ty::Con("Int".into())));
        assert!(check(&m).is_empty(), "primitive show must not flag");

        // A still-polymorphic argument is the legitimate fallback copy.
        let tv = Ty::Var(TyVar { name: "a".into(), id: u32::MAX });
        let m = module_with(show_call("show", Ty::list(tv)));
        assert!(check(&m).is_empty(), "polymorphic show must not flag");
    }
}
