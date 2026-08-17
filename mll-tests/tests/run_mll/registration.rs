//! The runnable-case registries: the mll_test!/mll_lib_test!/ghc_test!
//! case lists that compile-and-run every .mll under tests/cases/ and
//! tests/ghc/.

use super::*;

macro_rules! mll_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            run_mll_file(Path::new(concat!("tests/cases/", $file)), &[]);
        }
    };
}

macro_rules! mll_lib_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            run_mll_file(
                Path::new(concat!("tests/cases/", $file)),
                &[Path::new("../lib")],
            );
        }
    };
}

mll_test!(basics, "basics.mll");
mll_test!(lists, "lists.mll");
mll_test!(data_types, "data_types.mll");
mll_test!(records, "records.mll");
mll_test!(luadict, "luadict.mll");
mll_test!(newtypes, "newtypes.mll");
mll_test!(typeclasses, "typeclasses.mll");
mll_test!(superclass, "superclass.mll");
mll_test!(superclass_context, "superclass_context.mll");
mll_test!(where_clauses, "where_clauses.mll");
mll_test!(where_io_types, "where_io_types.mll");
mll_test!(bind_first_class, "bind_first_class.mll");
mll_test!(show_ghc_parity, "show_ghc_parity.mll");
mll_test!(prefix_minus, "prefix_minus.mll");
mll_test!(default_methods, "default_methods.mll");
mll_test!(default_methods_ops, "default_methods_ops.mll");
mll_test!(num_polymorphic, "num_polymorphic.mll");
mll_test!(num_user_instance, "num_user_instance.mll");
mll_test!(integral_semantics, "integral_semantics.mll");
mll_test!(datakinds, "datakinds.mll");
mll_test!(kinds_hkt, "kinds_hkt.mll");
mll_test!(type_level_nats, "type_level_nats.mll");
mll_test!(vec_nat, "vec_nat.mll");
mll_test!(type_family_arithmetic, "type_family_arithmetic.mll");
mll_test!(type_family_clause_priority, "type_family_clause_priority.mll");
mll_test!(promoted_nat_kind, "promoted_nat_kind.mll");
mll_test!(operator_sections, "operator_sections.mll");
mll_test!(section_composition, "section_composition.mll");
mll_test!(guards, "guards.mll");
mll_test!(guarded_caf, "guarded_caf.mll");
mll_test!(guard_strict_entry, "guard_strict_entry.mll");
mll_test!(lambdas, "lambdas.mll");
mll_test!(maybe, "maybe.mll");
mll_test!(monomorphization, "monomorphization.mll");
mll_test!(strings, "strings.mll");
mll_test!(operators, "operators.mll");
mll_test!(let_exprs, "let_exprs.mll");
mll_test!(ffi, "ffi.mll");
mll_test!(ffi_maybe_args, "ffi_maybe_args.mll");
mll_test!(ffi_multi_return, "ffi_multi_return.mll");
mll_test!(luacatch, "luacatch.mll");
mll_test!(lua_iterator_method, "lua_iterator_method.mll");
mll_test!(tuple_ctor, "tuple_ctor.mll");
mll_test!(lua_keywords, "lua_keywords.mll");
mll_test!(mapm, "mapm.mll");
mll_test!(mapm_underscore, "mapm_underscore.mll");
mll_test!(mapm_return_position, "mapm_return_position.mll");
mll_test!(result_only_monad, "result_only_monad.mll");
mll_test!(show_required, "show_required.mll");
mll_test!(either_ordering, "either_ordering.mll");
mll_test!(show_either, "show_either.mll");
mll_test!(case_guards, "case_guards.mll");
mll_test!(infix_def, "infix_def.mll");
mll_test!(seq_tco, "seq_tco.mll");
mll_test!(tco_case_let, "tco_case_let.mll");
mll_test!(tailloop_deep, "tailloop_deep.mll");
mll_test!(tailloop_capture, "tailloop_capture.mll");
mll_test!(tailloop_swap, "tailloop_swap.mll");
mll_test!(ioloop_deep, "ioloop_deep.mll");
mll_test!(ioloop_capture, "ioloop_capture.mll");
mll_test!(ioloop_mixed, "ioloop_mixed.mll");
mll_test!(ioloop_box, "ioloop_box.mll");
mll_test!(ioloop_seq_parity, "ioloop_seq_parity.mll");
mll_test!(performloop_deep, "performloop_deep.mll");
mll_test!(performloop_dispatch, "performloop_dispatch.mll");
mll_test!(performloop_pure_bottom, "performloop_pure_bottom.mll");
mll_test!(case_pure_bottom, "case_pure_bottom.mll");
mll_test!(if_pure_bottom, "if_pure_bottom.mll");
mll_test!(first_class_pure_bottom, "first_class_pure_bottom.mll");
mll_test!(perform_bare_tco_deep, "perform_bare_tco_deep.mll");

/// Raw tail-call elimination alone must carry a deep direct-perform
/// self-recursion: compile perform_bare_tco_deep.mll with every loop pass
/// disabled — via `CompileOptions::disable_opt_passes`, which is
/// per-compile and cannot race concurrently compiling tests the way
/// mutating `MLL_OPT_DISABLE` would — and run the 2e6-deep
/// bare-name-terminal case. This pins the bare `return self(...)`
/// direct-perform self-tail emission (action.rs) independently of the
/// tailloop conversion the normal build applies.
#[test]
fn perform_bare_tco_deep_unoptimized() {
    with_compiler_stack(|| {
        let path = Path::new("tests/cases/perform_bare_tco_deep.mll");
        let source = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Cannot read {}: {}", path.display(), e));
        let opts = mllc::CompileOptions {
            disable_opt_passes: Some("tailloop,ioloop,performloop".into()),
            ..Default::default()
        };
        let lua_code =
            mllc::compile_with_options(&source, Path::new("tests/cases"), &[], &opts)
                .expect("perform_bare_tco_deep compiles")
                .lua_code;
        let lua = mlua::Lua::new();
        lua.load(&lua_code)
            .set_name("perform_bare_tco_deep (loop passes disabled)")
            .exec()
            .expect("2e6-deep bare-TCO run in constant stack");
    });
}
mll_test!(perform_bare_tco_mutual, "perform_bare_tco_mutual.mll");

/// The interprocedural twin of perform_bare_tco_deep_unoptimized: two
/// direct-perform functions tail-calling EACH OTHER, 2e6 deep, with every
/// loop pass disabled (no pass can loop mutual recursion anyway). This pins
/// the module-level direct-perform classification (direct_perform_fns): a
/// saturated tail call to a KNOWN direct-perform callee — not just self —
/// emits the bare `return callee(...)` Lua tail call.
#[test]
fn perform_bare_tco_mutual_unoptimized() {
    with_compiler_stack(|| {
        let path = Path::new("tests/cases/perform_bare_tco_mutual.mll");
        let source = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Cannot read {}: {}", path.display(), e));
        let opts = mllc::CompileOptions {
            disable_opt_passes: Some("tailloop,ioloop,performloop".into()),
            ..Default::default()
        };
        let lua_code =
            mllc::compile_with_options(&source, Path::new("tests/cases"), &[], &opts)
                .expect("perform_bare_tco_mutual compiles")
                .lua_code;
        let lua = mlua::Lua::new();
        lua.load(&lua_code)
            .set_name("perform_bare_tco_mutual (loop passes disabled)")
            .exec()
            .expect("2e6-deep mutual bare-TCO run in constant stack");
    });
}
mll_test!(seq_forms, "seq_forms.mll");
mll_test!(self_referential_caf, "self_referential_caf.mll");
mll_test!(lazy_take_zip, "lazy_take_zip.mll");
mll_test!(dict, "dict.mll");
mll_test!(hashmap, "hashmap.mll");
mll_test!(gadts, "gadts.mll");
mll_test!(tuples, "tuples.mll");
mll_test!(trees, "trees.mll");
mll_test!(mutual_recursion, "mutual_recursion.mll");
mll_test!(higher_order, "higher_order.mll");
mll_test!(fizzbuzz, "fizzbuzz.mll");
mll_test!(purehashmap, "purehashmap.mll");
mll_test!(poly_recursion, "poly_recursion.mll");
mll_test!(poly_recursion_user_class, "poly_recursion_user_class.mll");
mll_test!(non_strict, "non_strict.mll");
mll_test!(compose_non_strict, "compose_non_strict.mll");
mll_test!(list_element_laziness, "list_element_laziness.mll");
mll_test!(tuple_field_laziness, "tuple_field_laziness.mll");
mll_test!(case_in_do_let, "case_in_do_let.mll");
mll_test!(functor_applicative, "functor_applicative.mll");
mll_test!(fmap_pure_bind_chain, "fmap_pure_bind_chain.mll");
mll_test!(io_actions, "io_actions.mll");
mll_test!(haskell_compat, "haskell_compat.mll");
mll_test!(pattern_matching, "pattern_matching.mll");
mll_test!(typeclasses_full, "typeclasses_full.mll");
mll_test!(user_class_method_per_use, "user_class_method_per_use.mll");
mll_test!(do_notation, "do_notation.mll");
mll_test!(list_comprehensions, "list_comprehensions.mll");
mll_test!(scoping, "scoping.mll");
mll_test!(type_aliases, "type_aliases.mll");
mll_test!(edge_cases, "edge_cases.mll");
mll_test!(feature_interactions, "feature_interactions.mll");
mll_test!(demand_analysis, "demand_analysis.mll");
mll_test!(ffi_strictness, "ffi_strictness.mll");
mll_test!(where_func_order, "where_func_order.mll");
mll_test!(where_group_mutual, "where_group_mutual.mll");
mll_test!(type_alias, "type_alias.mll");
mll_test!(selective_import, "selective_import.mll");
mll_test!(multiline_list, "multiline_list.mll");
mll_test!(nested_calls, "nested_calls.mll");
mll_test!(seq_when_putstr, "seq_when_putstr.mll");
mll_test!(any_type, "any_type.mll");
mll_test!(any_ffi_marshal, "any_ffi_marshal.mll");
mll_test!(bytestring, "bytestring.mll");
mll_test!(operator_fixity, "operator_fixity.mll");
mll_test!(fixity_import, "fixity_import.mll");
mll_test!(export_module, "export_module.mll");
mll_test!(import_hiding, "import_hiding.mll");
mll_test!(record_update, "record_update.mll");
mll_test!(record_brace_next_line, "record_brace_next_line.mll");
mll_test!(enum_range, "enum_range.mll");
mll_test!(read_typeclass, "read_typeclass.mll");
mll_test!(monad_nonio, "monad_nonio.mll");
mll_test!(derive_enum, "derive_enum.mll");
mll_test!(nested_eq, "nested_eq.mll");
mll_test!(st_return, "st_return.mll");
mll_test!(local_overflow, "local_overflow.mll");
mll_test!(locals_iife_limit, "locals_iife_limit.mll");
mll_test!(existentials, "existentials.mll");
// Constrained existentials (`forall a. Show a => Con a`): the pack side
// proves the instance, the unpack side gets exactly the declared classes
// on the skolemized hidden type. The rejection half (skolems must not
// unify with concrete types or escape) is exercised by the
// existential_unpacking_* tests below.
mll_test!(existential_constraints, "existential_constraints.mll");
mll_test!(derive_functor, "derive_functor.mll");
mll_test!(derive_functor_nested, "derive_functor_nested.mll");
// Foldable/Traversable: class methods (foldr/foldl/traverse), the generic
// Prelude functions over them, the Monoid class behind foldMap, liftA2,
// and user-defined instances of all three on a custom type
mll_test!(foldable, "foldable.mll");
mll_test!(traversable, "traversable.mll");
mll_test!(foldable_user_instance, "foldable_user_instance.mll");
mll_test!(monoid_instances, "monoid_instances.mll");
mll_test!(monoid_mappend_default, "monoid_mappend_default.mll");
mll_test!(source_class_nullary, "source_class_nullary.mll");
mll_test!(derive_eq, "derive_eq.mll");
mll_test!(derive_ord, "derive_ord.mll");
mll_test!(rank2, "rank2.mll");

// Stress tests
mll_test!(stress_large_adt, "stress_large_adt.mll");
mll_test!(stress_deep_recursion, "stress_deep_recursion.mll");
mll_test!(stress_nested_expr, "stress_nested_expr.mll");
mll_test!(stress_deep_chain, "stress_deep_chain.mll");
mll_test!(stress_deep_parens, "stress_deep_parens.mll");
mll_test!(stress_many_functions, "stress_many_functions.mll");
mll_test!(stress_many_instances, "stress_many_instances.mll");
mll_test!(stress_long_do, "stress_long_do.mll");
mll_test!(stress_large_pattern, "stress_large_pattern.mll");
mll_test!(stress_deep_types, "stress_deep_types.mll");
mll_test!(stress_many_args, "stress_many_args.mll");
mll_test!(stress_list_ops, "stress_list_ops.mll");
mll_test!(stress_complex_program, "stress_complex_program.mll");
mll_test!(stress_long_do_200, "stress_long_do_200.mll");
mll_test!(do_eval_order, "do_eval_order.mll");
mll_test!(do_let_scoping, "do_let_scoping.mll");
mll_test!(let_recursive_groups, "let_recursive_groups.mll");
mll_test!(let_pattern_recursive, "let_pattern_recursive.mll");
mll_test!(exceptions, "exceptions.mll");
mll_test!(type_alias_tuple, "type_alias_tuple.mll");
mll_test!(pointfree_caf, "pointfree_caf.mll");
mll_test!(value_forward_alias, "value_forward_alias.mll");
mll_test!(clause_local_scope, "clause_local_scope.mll");
mll_test!(diamond_import, "diamond_import.mll");
mll_test!(unit_type, "unit_type.mll");
// Instance-evidence resolution regressions (structured instance identity,
// deterministic class-variable dispatch, exact-identity specialization purge)
mll_test!(pair_ord_fields, "pair_ord_fields.mll");
mll_test!(mangle_collision, "mangle_collision.mll");
mll_test!(spec_limit_sibling, "spec_limit_sibling.mll");
mll_test!(tuple_eq_adt_elems, "tuple_eq_adt_elems.mll");
mll_test!(multi_clause_class_constraint, "multi_clause_class_constraint.mll");
mll_test!(lazy_cheap_bindings, "lazy_cheap_bindings.mll");
mll_test!(nested_just_pattern, "nested_just_pattern.mll");
mll_test!(non_exhaustive_live, "non_exhaustive_live.mll");
mll_test!(constructor_shadowing, "constructor_shadowing.mll");
mll_test!(constructor_shadowing_json, "constructor_shadowing_json.mll");
mll_test!(exitvalue_prelude, "exitvalue_prelude.mll");
// Instance contexts (`instance Show a => Show (Tree a)`): the context used to
// be parsed and discarded (bare form) or fail to parse (parenthesized form)
mll_test!(instance_context, "instance_context.mll");
mll_test!(instance_context_paren, "instance_context_paren.mll");
mll_test!(instance_context_multi, "instance_context_multi.mll");
mll_test!(instance_context_superclass, "instance_context_superclass.mll");
// Instance identities register module-wide before bodies are checked: a
// method body may use an instance declared later (or its own, recursively)
mll_test!(instance_forward_ref, "instance_forward_ref.mll");
// Application respects the callee's real arity: let/where-bound curried
// lambdas applied flat/staged/partially, nested-lambda bodies of top-level
// functions, `$`/`.` results that are still functions, and function-typed
// results flowing through the erased runtime generics (map/zipWith)
mll_test!(curried_lambda_arity, "curried_lambda_arity.mll");

// head/(!!) return the element itself, never a raw lazy-cons-head thunk
// (the WHNF-return invariant) — and stay exactly as lazy as before
mll_test!(lazy_head_projection, "lazy_head_projection.mll");

// The constant folder and the runtime agree on div/mod: Haskell FLOOR
// semantics for every sign combination (folder used Euclidean before)
mll_test!(div_mod_fold_runtime, "div_mod_fold_runtime.mll");

// div/mod by zero raise instead of yielding inf/nan; div is integer-exact
// past 2^53 on the embedded Lua 5.4 (native // floor division)
mll_test!(div_exact_and_zero, "div_exact_and_zero.mll");

// prefix (`div 7 2`), partial (`map (div 10) xs`), and first-class div/mod
// work — not just the backtick infix — with forcing of thunked operands
// (audit finding 4)
mll_test!(div_mod_prefix_forms, "div_mod_prefix_forms.mll");

// return/pure are non-strict: a returned bottom is not forced until demanded
// (audit finding 6)
mll_test!(return_non_strict, "return_non_strict.mll");
mll_test!(return_bottom_interproc, "return_bottom_interproc.mll");

// A <-bound user-action result may be a thunk (non-strict return): the binder
// must not mark it concrete, and runST must force the thread's result to WHNF.
// Regression for the miscompilation "attempt to perform arithmetic on a table
// value" introduced alongside the non-strict return fix.
mll_test!(action_result_whnf, "action_result_whnf.mll");

// Independently-authored regression coverage for the same three findings
// (broader shapes than the case files above).
//
// Finding 1: elements pulled via head/tail/(!!) from lazily-generated lists
// are forced values (consumed via both arithmetic and show) ...
mll_test!(lazy_index_thunk_leak, "lazy_index_thunk_leak.mll");
// ... while unconsumed bottoms stay unevaluated and infinite/self-referential
// lists still work (the laziness half of the contract)
mll_test!(lazy_index_laziness_contract, "lazy_index_laziness_contract.mll");

// Finding 2: folder and runtime agree on floor-semantics div/mod for every
// sign combination (lit / run / agree triples), plus edge and larger
// operands, the div/mod identity law, and the divisor-sign mod-range law
mll_test!(div_mod_fold_runtime_agree, "div_mod_fold_runtime_agree.mll");
mll_test!(div_mod_negative_edge, "div_mod_negative_edge.mll");

// Finding 3: div/mod by zero raise in function, literal-infix, let-bound and
// computed-zero forms; small div/mod stay exact in all four sign quadrants;
// quotients past 2^53 are integer-exact on the embedded Lua 5.4, both at the
// point of division and flowing onward through show/arithmetic/folds;
// negative-divisor literals fold to floor (not Euclidean) answers.
// NOTE: the independent suite also had div_zero_other_forms.mll (prefix /
// partial / first-class `div 1 0` etc.); it is EXCLUDED — prefix and
// first-class div/mod compile to a nil call (unfixed Finding 4).
mll_test!(div_mod_by_zero_raises, "div_mod_by_zero_raises.mll");
mll_test!(div_mod_small_exact, "div_mod_small_exact.mll");
mll_test!(div_large_exact, "div_large_exact.mll");
mll_test!(div_large_interaction, "div_large_interaction.mll");
mll_test!(div_mod_negative_literal_folding, "div_mod_negative_literal_folding.mll");
mll_test!(linear_affine_basic, "linear_affine_basic.mll");
mll_test!(linear_mult_poly, "linear_mult_poly.mll");
mll_test!(getline, "getline.mll");
mll_test!(readline, "readline.mll");
mll_test!(even_odd, "even_odd.mll");
mll_test!(even_odd_64bit, "even_odd_64bit.mll");
mll_test!(inline_sharing, "inline_sharing.mll");
// These two were in the GHC-oracle corpus but missing from the runnable list
// (found by mll_case_registry_is_complete): the oracle compares their stdout
// against GHC, this runs their in-program assertions directly.
mll_test!(exponent_op, "exponent_op.mll");
mll_test!(integer_bignum, "integer_bignum.mll");

// GHC-style compatibility tests
macro_rules! ghc_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            run_mll_file(Path::new(concat!("tests/ghc/", $file)), &[]);
        }
    };
}
ghc_test!(ghc_t001_fmap, "T001_fmap.mll");
ghc_test!(ghc_t002_applicative, "T002_applicative.mll");
ghc_test!(ghc_t003_maybe, "T003_do_maybe.mll");
ghc_test!(ghc_t004_dollar_fmap, "T004_dollar_fmap.mll");
ghc_test!(ghc_t005_list, "T005_list_monad.mll");
ghc_test!(ghc_cgrun004, "ghc_cgrun004.mll");
ghc_test!(ghc_cgrun007, "ghc_cgrun007.mll");
ghc_test!(ghc_cgrun008, "ghc_cgrun008.mll");
ghc_test!(ghc_cgrun010, "ghc_cgrun010.mll");
ghc_test!(ghc_cgrun054, "ghc_cgrun054.mll");
ghc_test!(ghc_cgrun058, "ghc_cgrun058.mll");
ghc_test!(ghc_cgrun063, "ghc_cgrun063.mll");
ghc_test!(ghc_cgrun009, "ghc_cgrun009.mll");
ghc_test!(ghc_cgrun011, "ghc_cgrun011.mll");
ghc_test!(ghc_cgrun012, "ghc_cgrun012.mll");
ghc_test!(ghc_cgrun013, "ghc_cgrun013.mll");
ghc_test!(ghc_cgrun014, "ghc_cgrun014.mll");
ghc_test!(ghc_cgrun015, "ghc_cgrun015.mll");
ghc_test!(ghc_cgrun016, "ghc_cgrun016.mll");
ghc_test!(ghc_cgrun017, "ghc_cgrun017.mll");
ghc_test!(ghc_cgrun018, "ghc_cgrun018.mll");
ghc_test!(ghc_cgrun019, "ghc_cgrun019.mll");
ghc_test!(ghc_cgrun020, "ghc_cgrun020.mll");
ghc_test!(ghc_cgrun021, "ghc_cgrun021.mll");
ghc_test!(ghc_cgrun022, "ghc_cgrun022.mll");
ghc_test!(ghc_cgrun023, "ghc_cgrun023.mll");
ghc_test!(ghc_cgrun024, "ghc_cgrun024.mll");
ghc_test!(ghc_cgrun025, "ghc_cgrun025.mll");
ghc_test!(ghc_cgrun026, "ghc_cgrun026.mll");
ghc_test!(ghc_cgrun027, "ghc_cgrun027.mll");
ghc_test!(ghc_cgrun028, "ghc_cgrun028.mll");
ghc_test!(ghc_cgrun029, "ghc_cgrun029.mll");
ghc_test!(ghc_cgrun030, "ghc_cgrun030.mll");
ghc_test!(ghc_cgrun031, "ghc_cgrun031.mll");
ghc_test!(ghc_cgrun032, "ghc_cgrun032.mll");
ghc_test!(ghc_cgrun033, "ghc_cgrun033.mll");
ghc_test!(ghc_cgrun034, "ghc_cgrun034.mll");
ghc_test!(ghc_cgrun035, "ghc_cgrun035.mll");
ghc_test!(ghc_cgrun036, "ghc_cgrun036.mll");
ghc_test!(ghc_cgrun037, "ghc_cgrun037.mll");
ghc_test!(ghc_cgrun038, "ghc_cgrun038.mll");
ghc_test!(ghc_cgrun039, "ghc_cgrun039.mll");
ghc_test!(ghc_cgrun040, "ghc_cgrun040.mll");
ghc_test!(ghc_cgrun041, "ghc_cgrun041.mll");
ghc_test!(ghc_cgrun042, "ghc_cgrun042.mll");
ghc_test!(ghc_cgrun043, "ghc_cgrun043.mll");
ghc_test!(ghc_cgrun044, "ghc_cgrun044.mll");
ghc_test!(ghc_cgrun045, "ghc_cgrun045.mll");
ghc_test!(ghc_cgrun046, "ghc_cgrun046.mll");
ghc_test!(ghc_cgrun047, "ghc_cgrun047.mll");
ghc_test!(ghc_cgrun048, "ghc_cgrun048.mll");
ghc_test!(ghc_cgrun049, "ghc_cgrun049.mll");
ghc_test!(ghc_cgrun050, "ghc_cgrun050.mll");
ghc_test!(ghc_cgrun051, "ghc_cgrun051.mll");
ghc_test!(ghc_cgrun052, "ghc_cgrun052.mll");
ghc_test!(ghc_cgrun053, "ghc_cgrun053.mll");
ghc_test!(ghc_cgrun055, "ghc_cgrun055.mll");
ghc_test!(ghc_cgrun056, "ghc_cgrun056.mll");
ghc_test!(ghc_cgrun057, "ghc_cgrun057.mll");
ghc_test!(ghc_cgrun059, "ghc_cgrun059.mll");
ghc_test!(ghc_cgrun060, "ghc_cgrun060.mll");
ghc_test!(ghc_cgrun061, "ghc_cgrun061.mll");
ghc_test!(ghc_cgrun062, "ghc_cgrun062.mll");
ghc_test!(ghc_cgrun064, "ghc_cgrun064.mll");
ghc_test!(ghc_cgrun065, "ghc_cgrun065.mll");
ghc_test!(ghc_cgrun066, "ghc_cgrun066.mll");
ghc_test!(ghc_cgrun067, "ghc_cgrun067.mll");
ghc_test!(ghc_cgrun068, "ghc_cgrun068.mll");
ghc_test!(ghc_cgrun069, "ghc_cgrun069.mll");
ghc_test!(ghc_tc001, "ghc_tc001.mll");
ghc_test!(ghc_tc002, "ghc_tc002.mll");
ghc_test!(ghc_tc003, "ghc_tc003.mll");
ghc_test!(ghc_tc004, "ghc_tc004.mll");
ghc_test!(ghc_tc005, "ghc_tc005.mll");
ghc_test!(ghc_tc006, "ghc_tc006.mll");
ghc_test!(ghc_tc007, "ghc_tc007.mll");
ghc_test!(ghc_tc008, "ghc_tc008.mll");
ghc_test!(ghc_tc009, "ghc_tc009.mll");
ghc_test!(ghc_tc010, "ghc_tc010.mll");
ghc_test!(ghc_tc011, "ghc_tc011.mll");
ghc_test!(ghc_tc012, "ghc_tc012.mll");
ghc_test!(ghc_ds001, "ghc_ds001.mll");
ghc_test!(ghc_ds002, "ghc_ds002.mll");
ghc_test!(ghc_ds003, "ghc_ds003.mll");
ghc_test!(ghc_ds004, "ghc_ds004.mll");
ghc_test!(ghc_ds005, "ghc_ds005.mll");
ghc_test!(ghc_ds006, "ghc_ds006.mll");
ghc_test!(ghc_ds007, "ghc_ds007.mll");
ghc_test!(ghc_ds008, "ghc_ds008.mll");
ghc_test!(ghc_ds009, "ghc_ds009.mll");
ghc_test!(ghc_ds010, "ghc_ds010.mll");
ghc_test!(ghc_ds011, "ghc_ds011.mll");
ghc_test!(ghc_ds012, "ghc_ds012.mll");
ghc_test!(ghc_ds013, "ghc_ds013.mll");
ghc_test!(ghc_ds014, "ghc_ds014.mll");
ghc_test!(ghc_regr001, "ghc_regr001.mll");
ghc_test!(ghc_regr002, "ghc_regr002.mll");
ghc_test!(ghc_regr003, "ghc_regr003.mll");
ghc_test!(ghc_regr004, "ghc_regr004.mll");
ghc_test!(ghc_regr005, "ghc_regr005.mll");
ghc_test!(ghc_regr006, "ghc_regr006.mll");
ghc_test!(ghc_regr007, "ghc_regr007.mll");
ghc_test!(ghc_regr008, "ghc_regr008.mll");
ghc_test!(ghc_regr009, "ghc_regr009.mll");
ghc_test!(ghc_regr010, "ghc_regr010.mll");
ghc_test!(ghc_regr011, "ghc_regr011.mll");
ghc_test!(ghc_regr012, "ghc_regr012.mll");
ghc_test!(ghc_regr013, "ghc_regr013.mll");
ghc_test!(ghc_regr014, "ghc_regr014.mll");
ghc_test!(ghc_regr015, "ghc_regr015.mll");
ghc_test!(ghc_regr016, "ghc_regr016.mll");
ghc_test!(ghc_regr017, "ghc_regr017.mll");
ghc_test!(ghc_regr018, "ghc_regr018.mll");
ghc_test!(ghc_regr019, "ghc_regr019.mll");
ghc_test!(ghc_regr020, "ghc_regr020.mll");

// Library module tests (need lib/ search path)
mll_lib_test!(lib_lstring, "lib_lstring.mll");
// GHC-parity string escape decoding on the lexer side: shorthand \a \b \f \v,
// decimal/octal/hex numeric escapes with maximal munch (the \05-is-one-byte
// fix), named control escapes (\SOH..\US, \SP, \DEL), the \& empty separator,
// and string gaps — asserted against the byte values GHC would produce, plus
// read . show == id for the byte escapes.
mll_lib_test!(string_escapes, "string_escapes.mll");
mll_lib_test!(error_forces_message, "error_forces_message.mll");
mll_lib_test!(lib_lbit, "lib_lbit.mll");
mll_lib_test!(lbit_64bit_boundary, "lbit_64bit_boundary.mll");
mll_lib_test!(lbit_strict_primitive_arg, "lbit_strict_primitive_arg.mll");
mll_lib_test!(bytestring_u64_sign_bit, "bytestring_u64_sign_bit.mll");
mll_lib_test!(lib_lmath, "lib_lmath.mll");
mll_lib_test!(lib_json, "lib_json.mll");
mll_lib_test!(json_codec, "json_codec.mll");
mll_lib_test!(integer_json, "integer_json.mll");
mll_lib_test!(caf_forward_reference, "caf_forward_reference.mll");
mll_lib_test!(derive_fromjson, "derive_fromjson.mll");
mll_lib_test!(derive_tojson, "derive_tojson.mll");
mll_lib_test!(derive_generic, "derive_generic.mll");
mll_lib_test!(generic_json, "generic_json.mll");
mll_lib_test!(generic_json_many, "generic_json_many.mll");
mll_lib_test!(generic_json_decode, "generic_json_decode.mll");
mll_lib_test!(constructor_as_rename, "constructor_as_rename.mll");
mll_lib_test!(lib_regex, "lib_regex.mll");
mll_lib_test!(lib_los, "lib_los.mll");
mll_lib_test!(lib_data_list, "lib_data_list.mll");
mll_lib_test!(lib_data_maybe, "lib_data_maybe.mll");
mll_lib_test!(lib_data_map, "lib_data_map.mll");
mll_lib_test!(lib_data_foldable, "lib_data_foldable.mll");
// FFI marshalling probed with CONSTRUCTED values (ranges, map/filter, `<>`,
// JSON decoding, computed Just/Nothing) — literals are already native Lua
// values and hide marshalling bugs.
mll_lib_test!(ffi_constructed_values, "ffi_constructed_values.mll");
// LIOLinear: the linear (%1) file-handle API — open, thread writes, close.
// The rejection side (leak / use-after-close) is in the
// linear_rejects_liolinear_* tests below.
mll_lib_test!(lib_liolinear, "lib_liolinear.mll");

/// Mirror of `ghc_oracle_registry_is_complete` for the runnable-case lists:
/// every .mll file under tests/cases/ must be registered via `mll_test!` or
/// `mll_lib_test!`, and every .mll under tests/ghc/ via `ghc_test!` — so a
/// case file can never sit on disk silently unexecuted. The lists are macro
/// invocations with no runtime index, so this test recovers the registered
/// set from this module's own source text (the invocations have a rigid
/// one-per-line shape) and diffs it against the files on disk.
#[test]
fn mll_case_registry_is_complete() {
    use std::collections::BTreeSet;

    // Helper MODULES, not test programs: they have no `main` and exist only
    // to be imported by registered cases (diamond_import.mll, export_module.mll,
    // import_hiding.mll, fixity_import.mll, and the module-visibility /
    // imported-fixity compile-error tests). Legitimately unregistered.
    const HELPER_MODULES: &[&str] = &[
        "DiamondLeaf.mll",
        "DiamondMid.mll",
        "ExportHelper.mll",
        "FixityOps.mll",
    ];

    let source = std::fs::read_to_string("tests/run_mll/registration.rs")
        .expect("tests/run_mll/registration.rs is readable");
    let registered_by = |macro_name: &str| -> BTreeSet<String> {
        source
            .lines()
            .filter_map(|line| {
                let rest = line.trim().strip_prefix(macro_name)?.strip_prefix("!(")?;
                let (_, file_part) = rest.split_once('"')?;
                let (file, _) = file_part.split_once('"')?;
                Some(file.to_string())
            })
            .collect()
    };

    let list_mll_files = |dir: &str| -> BTreeSet<String> {
        std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("cannot read {dir}: {e}"))
            .map(|e| e.expect("readable dir entry").file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".mll"))
            .collect()
    };

    let mut cases_registered = registered_by("mll_test");
    cases_registered.extend(registered_by("mll_lib_test"));
    let ghc_registered = registered_by("ghc_test");

    for (dir, registered) in [
        ("tests/cases", &cases_registered),
        ("tests/ghc", &ghc_registered),
    ] {
        let on_disk = list_mll_files(dir);
        let allowlisted: BTreeSet<String> = if dir == "tests/cases" {
            HELPER_MODULES.iter().map(|s| s.to_string()).collect()
        } else {
            BTreeSet::new()
        };
        let unregistered: Vec<_> = on_disk
            .iter()
            .filter(|f| !registered.contains(*f) && !allowlisted.contains(*f))
            .collect();
        assert!(
            unregistered.is_empty(),
            "case files in {dir}/ that no mll_test!/mll_lib_test!/ghc_test! \
             registration runs (add them to the lists in \
             tests/run_mll/registration.rs, or to HELPER_MODULES if they are \
             import-only helper modules): {unregistered:?}"
        );
        let missing: Vec<_> = registered.iter().filter(|f| !on_disk.contains(*f)).collect();
        assert!(
            missing.is_empty(),
            "registered cases without a file in {dir}/: {missing:?}"
        );
        // Keep the allowlist honest: an allowlisted helper must exist and
        // must not ALSO be registered (that would make the allowlist stale).
        for helper in &allowlisted {
            assert!(
                on_disk.contains(helper),
                "HELPER_MODULES entry {helper} does not exist in {dir}/"
            );
            assert!(
                !registered.contains(helper),
                "HELPER_MODULES entry {helper} is registered — remove it from \
                 the allowlist"
            );
        }
    }
}
