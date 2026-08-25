//! The GHC differential oracle: byte-compares each case's output against
//! pinned real-GHC goldens (see the design comment below).

use super::*;

// ========================= GHC differential oracle =========================
//
// The parity suite used to assert what the author believed GHC does; the
// oracle replaces belief with measurement. For every eligible case in
// tests/cases/ and tests/ghc/, mll-tests/regenerate-ghc-goldens.sh runs a
// mechanical GHC twin of the .mll source (real GHC via runghc, shared shim
// tests/ghc-golden/MllShim.hs) and pins its stdout as
// tests/ghc-golden/{cases,ghc}/<name>.stdout. The goldens are committed, so
// these tests never need GHC: each one compiles the .mll with mllc, runs it
// under mlua with `print`/`io.write` captured, and byte-compares the output
// against GHC's.
//
// Known divergences are pinned, not hidden: if mata-ll's output for a case
// is KNOWN to differ from GHC's, the exact current mata-ll output lives in
// tests/ghc-golden/divergent/{cases,ghc}/<name>.stdout and the difference is
// documented in tests/ghc-golden/DIVERGENCES.md. For such a case the test
// asserts that (a) mata-ll still produces exactly the pinned divergent
// output, and (b) the divergence is still real (pinned != golden) — so a fix
// or a drift both fail loudly, and the divergence list can never go stale.

/// Lua prologue that redirects `print` and `io.write` into a table of
/// output fragments, returning that table. Mirrors Lua's own conversions
/// (tostring; print joins with "\t" and appends "\n").
const ORACLE_CAPTURE_PRELUDE: &str = r##"
local out = {}
local tostring, select = tostring, select
print = function(...)
    local n = select("#", ...)
    for i = 1, n do
        if i > 1 then out[#out + 1] = "\t" end
        out[#out + 1] = tostring(select(i, ...))
    end
    out[#out + 1] = "\n"
end
io.write = function(...)
    local n = select("#", ...)
    for i = 1, n do out[#out + 1] = tostring(select(i, ...)) end
end
return out
"##;

/// Compile `tests/<sub>/<file>` and run it under mlua, returning everything
/// the program wrote to stdout (via putStr/putStrLn/print).
fn run_mll_capture_stdout(sub: &str, file: &str) -> Vec<u8> {
    let path = Path::new("tests").join(sub).join(file);
    // On the compiler's calibrated stack (with_compiler_stack), compiling
    // with mllc::compile directly — the harness `compile` wrapper would
    // spawn a second calibrated-stack thread inside this one.
    with_compiler_stack(|| {
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Cannot read {}: {}", path.display(), e));

        let source_dir = path.parent().unwrap_or(Path::new("."));
        let lib_path = Path::new("../lib");
        let lua_code = match mllc::compile(&source, source_dir, &[lib_path]) {
            Ok(r) => r.lua_code,
            Err(e) => panic!("{}: compilation failed:\n{}", path.display(), e),
        };

        let lua = mlua::Lua::new();
        let captured: mlua::Table = lua
            .load(ORACLE_CAPTURE_PRELUDE)
            .set_name("oracle capture prelude")
            .eval()
            .expect("capture prelude runs");
        match lua.load(&lua_code).set_name(path.to_str().unwrap()).exec() {
            Ok(()) => {}
            Err(e) => panic!("{}: runtime error:\n{}", path.display(), e),
        }
        let mut out = Vec::new();
        for frag in captured.sequence_values::<mlua::LuaString>() {
            out.extend_from_slice(&frag.expect("output fragment").as_bytes());
        }
        out
    })
}

/// Compare one case's mata-ll output against the pinned GHC golden (or, for
/// a recorded divergence, against the pinned divergent output).
fn ghc_oracle_case(sub: &str, file: &str) {
    let stem = file.strip_suffix(".mll").expect("oracle cases are .mll files");
    let golden_path = format!("tests/ghc-golden/{sub}/{stem}.stdout");
    let divergent_path = format!("tests/ghc-golden/divergent/{sub}/{stem}.stdout");

    let golden = std::fs::read(&golden_path).unwrap_or_else(|e| {
        panic!(
            "missing GHC golden {golden_path}: {e}\n\
             (re-pin with mll-tests/regenerate-ghc-goldens.sh on a machine with GHC)"
        )
    });
    let actual = run_mll_capture_stdout(sub, file);

    match std::fs::read(&divergent_path) {
        Ok(pinned) => {
            // Recorded divergence: mata-ll must still produce exactly the
            // pinned output, and it must still differ from GHC's.
            assert!(
                actual != golden,
                "{sub}/{file}: recorded divergence has RESOLVED — mata-ll now \
                 matches the GHC golden. Delete {divergent_path} and its entry \
                 in tests/ghc-golden/DIVERGENCES.md."
            );
            assert!(
                actual == pinned,
                "{sub}/{file}: divergent output drifted from its pinned record\n\
                 --- pinned mata-ll output ({divergent_path}):\n{}\n\
                 --- current mata-ll output:\n{}\n\
                 --- GHC golden ({golden_path}):\n{}",
                String::from_utf8_lossy(&pinned),
                String::from_utf8_lossy(&actual),
                String::from_utf8_lossy(&golden),
            );
        }
        Err(_) => {
            assert!(
                actual == golden,
                "{sub}/{file}: mata-ll output diverges from GHC\n\
                 --- GHC golden ({golden_path}):\n{}\n\
                 --- mata-ll:\n{}\n\
                 If this divergence is intended to stay, pin it: write the \
                 mata-ll output to {divergent_path} and document it in \
                 tests/ghc-golden/DIVERGENCES.md.",
                String::from_utf8_lossy(&golden),
                String::from_utf8_lossy(&actual),
            );
        }
    }
}

/// The full oracle corpus, defined once. `for_each_ghc_oracle_case!` passes
/// the list to a callback macro: `gen_ghc_oracle_tests` emits one #[test]
/// per case, `gen_ghc_oracle_index` emits the runtime index the registry
/// test checks against the files on disk.
macro_rules! gen_ghc_oracle_tests {
    ($(($name:ident, $sub:literal, $file:literal)),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                ghc_oracle_case($sub, $file);
            }
        )*
    };
}

macro_rules! gen_ghc_oracle_index {
    ($(($name:ident, $sub:literal, $file:literal)),* $(,)?) => {
        static GHC_ORACLE_CASES: &[(&str, &str)] = &[$(($sub, $file)),*];
    };
}

macro_rules! for_each_ghc_oracle_case {
    ($cb:ident) => {
        $cb! {
        (ghc_oracle_action_result_whnf, "cases", "action_result_whnf.mll"),
        (ghc_oracle_as_patterns, "cases", "as_patterns.mll"),
        (ghc_oracle_basics, "cases", "basics.mll"),
        (ghc_oracle_bind_first_class, "cases", "bind_first_class.mll"),
        (ghc_oracle_block_closers, "cases", "block_closers.mll"),
        (ghc_oracle_case_guards, "cases", "case_guards.mll"),
        (ghc_oracle_case_in_do_let, "cases", "case_in_do_let.mll"),
        (ghc_oracle_case_pure_bottom, "cases", "case_pure_bottom.mll"),
        (ghc_oracle_clause_local_scope, "cases", "clause_local_scope.mll"),
        (ghc_oracle_compose_non_strict, "cases", "compose_non_strict.mll"),
        (ghc_oracle_curried_lambda_arity, "cases", "curried_lambda_arity.mll"),
        (ghc_oracle_data_types, "cases", "data_types.mll"),
        (ghc_oracle_exponent_op, "cases", "exponent_op.mll"),
        (ghc_oracle_integer_bignum, "cases", "integer_bignum.mll"),
        (ghc_oracle_datakinds, "cases", "datakinds.mll"),
        (ghc_oracle_default_methods, "cases", "default_methods.mll"),
        (ghc_oracle_default_methods_ops, "cases", "default_methods_ops.mll"),
        (ghc_oracle_demand_analysis, "cases", "demand_analysis.mll"),
        (ghc_oracle_derive_enum, "cases", "derive_enum.mll"),
        (ghc_oracle_derive_eq, "cases", "derive_eq.mll"),
        (ghc_oracle_derive_functor, "cases", "derive_functor.mll"),
        (ghc_oracle_derive_functor_nested, "cases", "derive_functor_nested.mll"),
        (ghc_oracle_derive_ord, "cases", "derive_ord.mll"),
        (ghc_oracle_ord_max_min, "cases", "ord_max_min.mll"),
        (ghc_oracle_action_value_bindings, "cases", "action_value_bindings.mll"),
        (ghc_oracle_quot_rem_fixity, "cases", "quot_rem_fixity.mll"),
        (ghc_oracle_dollar_stays_lazy, "cases", "dollar_stays_lazy.mll"),
        (ghc_oracle_guarded_clause_scope, "cases", "guarded_clause_scope.mll"),
        (ghc_oracle_local_shadows_specials, "cases", "local_shadows_specials.mll"),
        (ghc_oracle_inline_no_capture, "cases", "inline_no_capture.mll"),
        (ghc_oracle_demand_shadowing, "cases", "demand_shadowing.mll"),
        (ghc_oracle_seq_action_value, "cases", "seq_action_value.mll"),
        (ghc_oracle_record_update_lazy, "cases", "record_update_lazy.mll"),
        (ghc_oracle_where_local_no_leak, "cases", "where_local_no_leak.mll"),
        (ghc_oracle_caf_trap_lazy, "cases", "caf_trap_lazy.mll"),
        (ghc_oracle_const_propagation, "cases", "const_propagation.mll"),
        (ghc_oracle_case_irrefutable_lazy, "cases", "case_irrefutable_lazy.mll"),
        (ghc_oracle_wrapper_splice, "cases", "wrapper_splice.mll"),
        (ghc_oracle_tuple_instance, "cases", "tuple_instance.mll"),
        (ghc_oracle_functor_forward_ref, "cases", "functor_forward_ref.mll"),
        (ghc_oracle_gadt_exhaustive_rigid, "cases", "gadt_exhaustive_rigid.mll"),
        (ghc_oracle_gadt_unnamed_universal, "cases", "gadt_unnamed_universal.mll"),
        (ghc_oracle_radix_literals, "cases", "radix_literals.mll"),
        (ghc_oracle_dot_spacing, "cases", "dot_spacing.mll"),
        (ghc_oracle_infix_method_defs, "cases", "infix_method_defs.mll"),
        (ghc_oracle_import_operator_list, "cases", "import_operator_list.mll"),
        (ghc_oracle_newtype_nextline_brace, "cases", "newtype_nextline_brace.mll"),
        (ghc_oracle_block_comment_layout, "cases", "block_comment_layout.mll"),
        (ghc_oracle_guard_qualifier_lists, "cases", "guard_qualifier_lists.mll"),
        (ghc_oracle_where_tuple_binding, "cases", "where_tuple_binding.mll"),
        (ghc_oracle_diamond_import, "cases", "diamond_import.mll"),
        (ghc_oracle_dict, "cases", "dict.mll"),
        (ghc_oracle_div_exact_and_zero, "cases", "div_exact_and_zero.mll"),
        (ghc_oracle_div_large_exact, "cases", "div_large_exact.mll"),
        (ghc_oracle_div_large_interaction, "cases", "div_large_interaction.mll"),
        (ghc_oracle_div_mod_by_zero_raises, "cases", "div_mod_by_zero_raises.mll"),
        (ghc_oracle_div_mod_fold_runtime, "cases", "div_mod_fold_runtime.mll"),
        (ghc_oracle_div_mod_fold_runtime_agree, "cases", "div_mod_fold_runtime_agree.mll"),
        (ghc_oracle_div_mod_negative_edge, "cases", "div_mod_negative_edge.mll"),
        (ghc_oracle_div_mod_negative_literal_folding, "cases", "div_mod_negative_literal_folding.mll"),
        (ghc_oracle_div_mod_prefix_forms, "cases", "div_mod_prefix_forms.mll"),
        (ghc_oracle_div_mod_small_exact, "cases", "div_mod_small_exact.mll"),
        (ghc_oracle_do_eval_order, "cases", "do_eval_order.mll"),
        (ghc_oracle_do_in_nested_positions, "cases", "do_in_nested_positions.mll"),
        (ghc_oracle_do_let_scoping, "cases", "do_let_scoping.mll"),
        (ghc_oracle_do_let_tuple_group, "cases", "do_let_tuple_group.mll"),
        (ghc_oracle_do_notation, "cases", "do_notation.mll"),
        (ghc_oracle_edge_cases, "cases", "edge_cases.mll"),
        (ghc_oracle_empty_layout_blocks, "cases", "empty_layout_blocks.mll"),
        (ghc_oracle_either_ordering, "cases", "either_ordering.mll"),
        (ghc_oracle_enum_range, "cases", "enum_range.mll"),
        (ghc_oracle_even_odd, "cases", "even_odd.mll"),
        (ghc_oracle_even_odd_64bit, "cases", "even_odd_64bit.mll"),
        (ghc_oracle_exceptions, "cases", "exceptions.mll"),
        (ghc_oracle_existential_constraints, "cases", "existential_constraints.mll"),
        (ghc_oracle_existentials, "cases", "existentials.mll"),
        (ghc_oracle_feature_interactions, "cases", "feature_interactions.mll"),
        (ghc_oracle_first_class_pure_bottom, "cases", "first_class_pure_bottom.mll"),
        (ghc_oracle_fixity_import, "cases", "fixity_import.mll"),
        (ghc_oracle_fizzbuzz, "cases", "fizzbuzz.mll"),
        (ghc_oracle_fmap_pure_bind_chain, "cases", "fmap_pure_bind_chain.mll"),
        (ghc_oracle_foldable, "cases", "foldable.mll"),
        (ghc_oracle_foldable_user_instance, "cases", "foldable_user_instance.mll"),
        (ghc_oracle_functor_applicative, "cases", "functor_applicative.mll"),
        (ghc_oracle_gadt_syntax_derives, "cases", "gadt_syntax_derives.mll"),
        (ghc_oracle_gadts, "cases", "gadts.mll"),
        (ghc_oracle_guard_strict_entry, "cases", "guard_strict_entry.mll"),
        (ghc_oracle_guarded_caf, "cases", "guarded_caf.mll"),
        (ghc_oracle_guards, "cases", "guards.mll"),
        (ghc_oracle_haskell_compat, "cases", "haskell_compat.mll"),
        (ghc_oracle_higher_order, "cases", "higher_order.mll"),
        (ghc_oracle_if_pure_bottom, "cases", "if_pure_bottom.mll"),
        (ghc_oracle_import_hiding, "cases", "import_hiding.mll"),
        (ghc_oracle_infix_def, "cases", "infix_def.mll"),
        (ghc_oracle_instance_context, "cases", "instance_context.mll"),
        (ghc_oracle_instance_context_multi, "cases", "instance_context_multi.mll"),
        (ghc_oracle_instance_context_paren, "cases", "instance_context_paren.mll"),
        (ghc_oracle_instance_context_superclass, "cases", "instance_context_superclass.mll"),
        (ghc_oracle_instance_forward_ref, "cases", "instance_forward_ref.mll"),
        (ghc_oracle_ioloop_box, "cases", "ioloop_box.mll"),
        (ghc_oracle_ioloop_capture, "cases", "ioloop_capture.mll"),
        (ghc_oracle_ioloop_deep, "cases", "ioloop_deep.mll"),
        (ghc_oracle_ioloop_mixed, "cases", "ioloop_mixed.mll"),
        (ghc_oracle_ioloop_seq_parity, "cases", "ioloop_seq_parity.mll"),
        (ghc_oracle_integral_semantics, "cases", "integral_semantics.mll"),
        (ghc_oracle_io_actions, "cases", "io_actions.mll"),
        (ghc_oracle_inline_compose_non_strict, "cases", "inline_compose_non_strict.mll"),
        (ghc_oracle_inline_sharing, "cases", "inline_sharing.mll"),
        (ghc_oracle_kinds_hkt, "cases", "kinds_hkt.mll"),
        (ghc_oracle_lambdas, "cases", "lambdas.mll"),
        (ghc_oracle_lazy_cheap_bindings, "cases", "lazy_cheap_bindings.mll"),
        (ghc_oracle_later_clause_force_once, "cases", "later_clause_force_once.mll"),
        (ghc_oracle_lazy_head_projection, "cases", "lazy_head_projection.mll"),
        (ghc_oracle_lazy_index_laziness_contract, "cases", "lazy_index_laziness_contract.mll"),
        (ghc_oracle_lazy_index_thunk_leak, "cases", "lazy_index_thunk_leak.mll"),
        (ghc_oracle_lazy_take_zip, "cases", "lazy_take_zip.mll"),
        (ghc_oracle_let_exprs, "cases", "let_exprs.mll"),
        (ghc_oracle_let_pattern_recursive, "cases", "let_pattern_recursive.mll"),
        (ghc_oracle_let_recursive_groups, "cases", "let_recursive_groups.mll"),
        (ghc_oracle_lib_data_foldable, "cases", "lib_data_foldable.mll"),
        (ghc_oracle_lib_data_list, "cases", "lib_data_list.mll"),
        (ghc_oracle_lib_data_maybe, "cases", "lib_data_maybe.mll"),
        (ghc_oracle_list_comprehensions, "cases", "list_comprehensions.mll"),
        (ghc_oracle_list_element_laziness, "cases", "list_element_laziness.mll"),
        (ghc_oracle_lists, "cases", "lists.mll"),
        (ghc_oracle_local_overflow, "cases", "local_overflow.mll"),
        (ghc_oracle_local_shadows_poly_global, "cases", "local_shadows_poly_global.mll"),
        (ghc_oracle_locals_iife_limit, "cases", "locals_iife_limit.mll"),
        (ghc_oracle_mangle_collision, "cases", "mangle_collision.mll"),
        (ghc_oracle_mapm, "cases", "mapm.mll"),
        (ghc_oracle_mapm_return_position, "cases", "mapm_return_position.mll"),
        (ghc_oracle_mapm_underscore, "cases", "mapm_underscore.mll"),
        (ghc_oracle_maybe, "cases", "maybe.mll"),
        (ghc_oracle_monad_nonio, "cases", "monad_nonio.mll"),
        (ghc_oracle_monoid_instances, "cases", "monoid_instances.mll"),
        (ghc_oracle_monoid_mappend_default, "cases", "monoid_mappend_default.mll"),
        (ghc_oracle_monomorphization, "cases", "monomorphization.mll"),
        (ghc_oracle_multi_clause_class_constraint, "cases", "multi_clause_class_constraint.mll"),
        (ghc_oracle_multiline_list, "cases", "multiline_list.mll"),
        (ghc_oracle_mutual_recursion, "cases", "mutual_recursion.mll"),
        (ghc_oracle_negate_min_int_fold, "cases", "negate_min_int_fold.mll"),
        (ghc_oracle_nested_calls, "cases", "nested_calls.mll"),
        (ghc_oracle_nested_eq, "cases", "nested_eq.mll"),
        (ghc_oracle_nested_just_pattern, "cases", "nested_just_pattern.mll"),
        (ghc_oracle_newtype_forms, "cases", "newtype_forms.mll"),
        (ghc_oracle_non_exhaustive_live, "cases", "non_exhaustive_live.mll"),
        (ghc_oracle_non_strict, "cases", "non_strict.mll"),
        (ghc_oracle_num_polymorphic, "cases", "num_polymorphic.mll"),
        (ghc_oracle_operator_fixity, "cases", "operator_fixity.mll"),
        (ghc_oracle_operator_line_start, "cases", "operator_line_start.mll"),
        (ghc_oracle_operator_sections, "cases", "operator_sections.mll"),
        (ghc_oracle_operators, "cases", "operators.mll"),
        (ghc_oracle_pair_ord_fields, "cases", "pair_ord_fields.mll"),
        (ghc_oracle_pattern_matching, "cases", "pattern_matching.mll"),
        (ghc_oracle_perform_bare_tco_deep, "cases", "perform_bare_tco_deep.mll"),
        (ghc_oracle_perform_bare_tco_mutual, "cases", "perform_bare_tco_mutual.mll"),
        (ghc_oracle_performloop_deep, "cases", "performloop_deep.mll"),
        (ghc_oracle_performloop_dispatch, "cases", "performloop_dispatch.mll"),
        (ghc_oracle_performloop_pure_bottom, "cases", "performloop_pure_bottom.mll"),
        (ghc_oracle_pointfree_caf, "cases", "pointfree_caf.mll"),
        (ghc_oracle_poly_recursion, "cases", "poly_recursion.mll"),
        (ghc_oracle_prefix_minus, "cases", "prefix_minus.mll"),
        (ghc_oracle_promoted_nat_kind, "cases", "promoted_nat_kind.mll"),
        (ghc_oracle_rank2, "cases", "rank2.mll"),
        (ghc_oracle_read_typeclass, "cases", "read_typeclass.mll"),
        (ghc_oracle_record_brace_next_line, "cases", "record_brace_next_line.mll"),
        (ghc_oracle_record_update, "cases", "record_update.mll"),
        (ghc_oracle_records, "cases", "records.mll"),
        (ghc_oracle_result_only_monad, "cases", "result_only_monad.mll"),
        (ghc_oracle_return_bottom_interproc, "cases", "return_bottom_interproc.mll"),
        (ghc_oracle_return_non_strict, "cases", "return_non_strict.mll"),
        (ghc_oracle_scoping, "cases", "scoping.mll"),
        (ghc_oracle_section_composition, "cases", "section_composition.mll"),
        (ghc_oracle_selective_import, "cases", "selective_import.mll"),
        (ghc_oracle_self_referential_caf, "cases", "self_referential_caf.mll"),
        (ghc_oracle_seq_forms, "cases", "seq_forms.mll"),
        (ghc_oracle_seq_tco, "cases", "seq_tco.mll"),
        (ghc_oracle_seq_when_putstr, "cases", "seq_when_putstr.mll"),
        (ghc_oracle_show_either, "cases", "show_either.mll"),
        (ghc_oracle_show_ghc_parity, "cases", "show_ghc_parity.mll"),
        (ghc_oracle_show_required, "cases", "show_required.mll"),
        (ghc_oracle_source_class_nullary, "cases", "source_class_nullary.mll"),
        (ghc_oracle_spec_limit_sibling, "cases", "spec_limit_sibling.mll"),
        (ghc_oracle_st_modify_repeated_action, "cases", "st_modify_repeated_action.mll"),
        (ghc_oracle_st_return, "cases", "st_return.mll"),
        (ghc_oracle_superclass_context, "cases", "superclass_context.mll"),
        (ghc_oracle_stress_complex_program, "cases", "stress_complex_program.mll"),
        (ghc_oracle_stress_deep_chain, "cases", "stress_deep_chain.mll"),
        (ghc_oracle_stress_deep_parens, "cases", "stress_deep_parens.mll"),
        (ghc_oracle_stress_deep_recursion, "cases", "stress_deep_recursion.mll"),
        (ghc_oracle_stress_deep_types, "cases", "stress_deep_types.mll"),
        (ghc_oracle_stress_large_adt, "cases", "stress_large_adt.mll"),
        (ghc_oracle_stress_large_pattern, "cases", "stress_large_pattern.mll"),
        (ghc_oracle_stress_list_ops, "cases", "stress_list_ops.mll"),
        (ghc_oracle_stress_long_do, "cases", "stress_long_do.mll"),
        (ghc_oracle_stress_long_do_200, "cases", "stress_long_do_200.mll"),
        (ghc_oracle_stress_many_args, "cases", "stress_many_args.mll"),
        (ghc_oracle_stress_many_functions, "cases", "stress_many_functions.mll"),
        (ghc_oracle_stress_many_instances, "cases", "stress_many_instances.mll"),
        (ghc_oracle_stress_nested_expr, "cases", "stress_nested_expr.mll"),
        (ghc_oracle_strings, "cases", "strings.mll"),
        (ghc_oracle_tco_case_let, "cases", "tco_case_let.mll"),
        (ghc_oracle_traversable, "cases", "traversable.mll"),
        (ghc_oracle_trees, "cases", "trees.mll"),
        (ghc_oracle_tailloop_capture, "cases", "tailloop_capture.mll"),
        (ghc_oracle_tailloop_deep, "cases", "tailloop_deep.mll"),
        (ghc_oracle_tailloop_swap, "cases", "tailloop_swap.mll"),
        (ghc_oracle_tuple_ctor, "cases", "tuple_ctor.mll"),
        (ghc_oracle_tuple_eq_adt_elems, "cases", "tuple_eq_adt_elems.mll"),
        (ghc_oracle_tuple_field_laziness, "cases", "tuple_field_laziness.mll"),
        (ghc_oracle_tuples, "cases", "tuples.mll"),
        (ghc_oracle_type_alias, "cases", "type_alias.mll"),
        (ghc_oracle_type_alias_tuple, "cases", "type_alias_tuple.mll"),
        (ghc_oracle_type_aliases, "cases", "type_aliases.mll"),
        (ghc_oracle_type_family_arithmetic, "cases", "type_family_arithmetic.mll"),
        (ghc_oracle_type_family_clause_priority, "cases", "type_family_clause_priority.mll"),
        (ghc_oracle_type_level_nats, "cases", "type_level_nats.mll"),
        (ghc_oracle_typeclasses, "cases", "typeclasses.mll"),
        (ghc_oracle_typeclasses_full, "cases", "typeclasses_full.mll"),
        (ghc_oracle_unit_type, "cases", "unit_type.mll"),
        (ghc_oracle_user_class_method_per_use, "cases", "user_class_method_per_use.mll"),
        (ghc_oracle_value_forward_alias, "cases", "value_forward_alias.mll"),
        (ghc_oracle_vec_nat, "cases", "vec_nat.mll"),
        (ghc_oracle_where_clause_order_laziness, "cases", "where_clause_order_laziness.mll"),
        (ghc_oracle_where_clauses, "cases", "where_clauses.mll"),
        (ghc_oracle_where_group_mutual, "cases", "where_group_mutual.mll"),
        (ghc_oracle_where_func_order, "cases", "where_func_order.mll"),
        (ghc_oracle_where_io_types, "cases", "where_io_types.mll"),
        (ghc_oracle_t001_fmap, "ghc", "T001_fmap.mll"),
        (ghc_oracle_t002_applicative, "ghc", "T002_applicative.mll"),
        (ghc_oracle_t003_do_maybe, "ghc", "T003_do_maybe.mll"),
        (ghc_oracle_t004_dollar_fmap, "ghc", "T004_dollar_fmap.mll"),
        (ghc_oracle_t005_list_monad, "ghc", "T005_list_monad.mll"),
        (ghc_oracle_ghc_cgrun004, "ghc", "ghc_cgrun004.mll"),
        (ghc_oracle_ghc_cgrun007, "ghc", "ghc_cgrun007.mll"),
        (ghc_oracle_ghc_cgrun008, "ghc", "ghc_cgrun008.mll"),
        (ghc_oracle_ghc_cgrun009, "ghc", "ghc_cgrun009.mll"),
        (ghc_oracle_ghc_cgrun010, "ghc", "ghc_cgrun010.mll"),
        (ghc_oracle_ghc_cgrun011, "ghc", "ghc_cgrun011.mll"),
        (ghc_oracle_ghc_cgrun012, "ghc", "ghc_cgrun012.mll"),
        (ghc_oracle_ghc_cgrun013, "ghc", "ghc_cgrun013.mll"),
        (ghc_oracle_ghc_cgrun014, "ghc", "ghc_cgrun014.mll"),
        (ghc_oracle_ghc_cgrun015, "ghc", "ghc_cgrun015.mll"),
        (ghc_oracle_ghc_cgrun016, "ghc", "ghc_cgrun016.mll"),
        (ghc_oracle_ghc_cgrun017, "ghc", "ghc_cgrun017.mll"),
        (ghc_oracle_ghc_cgrun018, "ghc", "ghc_cgrun018.mll"),
        (ghc_oracle_ghc_cgrun019, "ghc", "ghc_cgrun019.mll"),
        (ghc_oracle_ghc_cgrun020, "ghc", "ghc_cgrun020.mll"),
        (ghc_oracle_ghc_cgrun021, "ghc", "ghc_cgrun021.mll"),
        (ghc_oracle_ghc_cgrun022, "ghc", "ghc_cgrun022.mll"),
        (ghc_oracle_ghc_cgrun023, "ghc", "ghc_cgrun023.mll"),
        (ghc_oracle_ghc_cgrun024, "ghc", "ghc_cgrun024.mll"),
        (ghc_oracle_ghc_cgrun025, "ghc", "ghc_cgrun025.mll"),
        (ghc_oracle_ghc_cgrun026, "ghc", "ghc_cgrun026.mll"),
        (ghc_oracle_ghc_cgrun027, "ghc", "ghc_cgrun027.mll"),
        (ghc_oracle_ghc_cgrun028, "ghc", "ghc_cgrun028.mll"),
        (ghc_oracle_ghc_cgrun029, "ghc", "ghc_cgrun029.mll"),
        (ghc_oracle_ghc_cgrun030, "ghc", "ghc_cgrun030.mll"),
        (ghc_oracle_ghc_cgrun031, "ghc", "ghc_cgrun031.mll"),
        (ghc_oracle_ghc_cgrun032, "ghc", "ghc_cgrun032.mll"),
        (ghc_oracle_ghc_cgrun033, "ghc", "ghc_cgrun033.mll"),
        (ghc_oracle_ghc_cgrun034, "ghc", "ghc_cgrun034.mll"),
        (ghc_oracle_ghc_cgrun035, "ghc", "ghc_cgrun035.mll"),
        (ghc_oracle_ghc_cgrun036, "ghc", "ghc_cgrun036.mll"),
        (ghc_oracle_ghc_cgrun037, "ghc", "ghc_cgrun037.mll"),
        (ghc_oracle_ghc_cgrun038, "ghc", "ghc_cgrun038.mll"),
        (ghc_oracle_ghc_cgrun039, "ghc", "ghc_cgrun039.mll"),
        (ghc_oracle_ghc_cgrun040, "ghc", "ghc_cgrun040.mll"),
        (ghc_oracle_ghc_cgrun041, "ghc", "ghc_cgrun041.mll"),
        (ghc_oracle_ghc_cgrun042, "ghc", "ghc_cgrun042.mll"),
        (ghc_oracle_ghc_cgrun043, "ghc", "ghc_cgrun043.mll"),
        (ghc_oracle_ghc_cgrun044, "ghc", "ghc_cgrun044.mll"),
        (ghc_oracle_ghc_cgrun045, "ghc", "ghc_cgrun045.mll"),
        (ghc_oracle_ghc_cgrun046, "ghc", "ghc_cgrun046.mll"),
        (ghc_oracle_ghc_cgrun047, "ghc", "ghc_cgrun047.mll"),
        (ghc_oracle_ghc_cgrun048, "ghc", "ghc_cgrun048.mll"),
        (ghc_oracle_ghc_cgrun049, "ghc", "ghc_cgrun049.mll"),
        (ghc_oracle_ghc_cgrun050, "ghc", "ghc_cgrun050.mll"),
        (ghc_oracle_ghc_cgrun051, "ghc", "ghc_cgrun051.mll"),
        (ghc_oracle_ghc_cgrun052, "ghc", "ghc_cgrun052.mll"),
        (ghc_oracle_ghc_cgrun053, "ghc", "ghc_cgrun053.mll"),
        (ghc_oracle_ghc_cgrun054, "ghc", "ghc_cgrun054.mll"),
        (ghc_oracle_ghc_cgrun055, "ghc", "ghc_cgrun055.mll"),
        (ghc_oracle_ghc_cgrun056, "ghc", "ghc_cgrun056.mll"),
        (ghc_oracle_ghc_cgrun057, "ghc", "ghc_cgrun057.mll"),
        (ghc_oracle_ghc_cgrun058, "ghc", "ghc_cgrun058.mll"),
        (ghc_oracle_ghc_cgrun059, "ghc", "ghc_cgrun059.mll"),
        (ghc_oracle_ghc_cgrun060, "ghc", "ghc_cgrun060.mll"),
        (ghc_oracle_ghc_cgrun061, "ghc", "ghc_cgrun061.mll"),
        (ghc_oracle_ghc_cgrun062, "ghc", "ghc_cgrun062.mll"),
        (ghc_oracle_ghc_cgrun063, "ghc", "ghc_cgrun063.mll"),
        (ghc_oracle_ghc_cgrun064, "ghc", "ghc_cgrun064.mll"),
        (ghc_oracle_ghc_cgrun065, "ghc", "ghc_cgrun065.mll"),
        (ghc_oracle_ghc_cgrun066, "ghc", "ghc_cgrun066.mll"),
        (ghc_oracle_ghc_cgrun067, "ghc", "ghc_cgrun067.mll"),
        (ghc_oracle_ghc_cgrun068, "ghc", "ghc_cgrun068.mll"),
        (ghc_oracle_ghc_cgrun069, "ghc", "ghc_cgrun069.mll"),
        (ghc_oracle_ghc_ds001, "ghc", "ghc_ds001.mll"),
        (ghc_oracle_ghc_ds002, "ghc", "ghc_ds002.mll"),
        (ghc_oracle_ghc_ds003, "ghc", "ghc_ds003.mll"),
        (ghc_oracle_ghc_ds004, "ghc", "ghc_ds004.mll"),
        (ghc_oracle_ghc_ds005, "ghc", "ghc_ds005.mll"),
        (ghc_oracle_ghc_ds006, "ghc", "ghc_ds006.mll"),
        (ghc_oracle_ghc_ds007, "ghc", "ghc_ds007.mll"),
        (ghc_oracle_ghc_ds008, "ghc", "ghc_ds008.mll"),
        (ghc_oracle_ghc_ds009, "ghc", "ghc_ds009.mll"),
        (ghc_oracle_ghc_ds010, "ghc", "ghc_ds010.mll"),
        (ghc_oracle_ghc_ds011, "ghc", "ghc_ds011.mll"),
        (ghc_oracle_ghc_ds012, "ghc", "ghc_ds012.mll"),
        (ghc_oracle_ghc_ds013, "ghc", "ghc_ds013.mll"),
        (ghc_oracle_ghc_ds014, "ghc", "ghc_ds014.mll"),
        (ghc_oracle_ghc_regr001, "ghc", "ghc_regr001.mll"),
        (ghc_oracle_ghc_regr002, "ghc", "ghc_regr002.mll"),
        (ghc_oracle_ghc_regr003, "ghc", "ghc_regr003.mll"),
        (ghc_oracle_ghc_regr004, "ghc", "ghc_regr004.mll"),
        (ghc_oracle_ghc_regr006, "ghc", "ghc_regr006.mll"),
        (ghc_oracle_ghc_regr007, "ghc", "ghc_regr007.mll"),
        (ghc_oracle_ghc_regr008, "ghc", "ghc_regr008.mll"),
        (ghc_oracle_ghc_regr009, "ghc", "ghc_regr009.mll"),
        (ghc_oracle_ghc_regr010, "ghc", "ghc_regr010.mll"),
        (ghc_oracle_ghc_regr011, "ghc", "ghc_regr011.mll"),
        (ghc_oracle_ghc_regr012, "ghc", "ghc_regr012.mll"),
        (ghc_oracle_ghc_regr013, "ghc", "ghc_regr013.mll"),
        (ghc_oracle_ghc_regr014, "ghc", "ghc_regr014.mll"),
        (ghc_oracle_ghc_regr015, "ghc", "ghc_regr015.mll"),
        (ghc_oracle_ghc_regr016, "ghc", "ghc_regr016.mll"),
        (ghc_oracle_ghc_regr017, "ghc", "ghc_regr017.mll"),
        (ghc_oracle_ghc_regr018, "ghc", "ghc_regr018.mll"),
        (ghc_oracle_ghc_regr019, "ghc", "ghc_regr019.mll"),
        (ghc_oracle_ghc_regr020, "ghc", "ghc_regr020.mll"),
        (ghc_oracle_ghc_tc001, "ghc", "ghc_tc001.mll"),
        (ghc_oracle_ghc_tc002, "ghc", "ghc_tc002.mll"),
        (ghc_oracle_ghc_tc003, "ghc", "ghc_tc003.mll"),
        (ghc_oracle_ghc_tc004, "ghc", "ghc_tc004.mll"),
        (ghc_oracle_ghc_tc005, "ghc", "ghc_tc005.mll"),
        (ghc_oracle_ghc_tc006, "ghc", "ghc_tc006.mll"),
        (ghc_oracle_ghc_tc007, "ghc", "ghc_tc007.mll"),
        (ghc_oracle_ghc_tc008, "ghc", "ghc_tc008.mll"),
        (ghc_oracle_ghc_tc009, "ghc", "ghc_tc009.mll"),
        (ghc_oracle_ghc_tc010, "ghc", "ghc_tc010.mll"),
        (ghc_oracle_ghc_tc011, "ghc", "ghc_tc011.mll"),
        (ghc_oracle_ghc_tc012, "ghc", "ghc_tc012.mll"),
        }
    };
}

for_each_ghc_oracle_case!(gen_ghc_oracle_tests);
for_each_ghc_oracle_case!(gen_ghc_oracle_index);

/// The registry must mirror the files on disk exactly: every golden has a
/// test, every registered test has a golden, and every pinned divergence has
/// a golden and a DIVERGENCES.md entry. This is what keeps a re-pin (new
/// goldens) or a new divergence from slipping past unregistered.
#[test]
fn ghc_oracle_registry_is_complete() {
    use std::collections::BTreeSet;

    let list_stdout_files = |dir: &str| -> BTreeSet<String> {
        match std::fs::read_dir(dir) {
            Ok(entries) => entries
                .map(|e| e.expect("readable dir entry").file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".stdout"))
                .collect(),
            Err(_) => BTreeSet::new(),
        }
    };

    // Goldens on disk == registered cases.
    for sub in ["cases", "ghc"] {
        let on_disk = list_stdout_files(&format!("tests/ghc-golden/{sub}"));
        let registered: BTreeSet<String> = GHC_ORACLE_CASES
            .iter()
            .filter(|(s, _)| *s == sub)
            .map(|(_, f)| format!("{}.stdout", f.strip_suffix(".mll").unwrap()))
            .collect();
        let unregistered: Vec<_> = on_disk.difference(&registered).collect();
        let missing: Vec<_> = registered.difference(&on_disk).collect();
        assert!(
            unregistered.is_empty(),
            "goldens in tests/ghc-golden/{sub}/ without a registered ghc_oracle_* \
             test (add them to for_each_ghc_oracle_case!): {unregistered:?}"
        );
        assert!(
            missing.is_empty(),
            "registered ghc_oracle_* cases without a golden in \
             tests/ghc-golden/{sub}/ (run mll-tests/regenerate-ghc-goldens.sh): \
             {missing:?}"
        );
    }

    // Every pinned divergence has a golden and a DIVERGENCES.md entry.
    let divergences_md = std::fs::read_to_string("tests/ghc-golden/DIVERGENCES.md")
        .expect("tests/ghc-golden/DIVERGENCES.md exists");
    for sub in ["cases", "ghc"] {
        for name in list_stdout_files(&format!("tests/ghc-golden/divergent/{sub}")) {
            let stem = name.strip_suffix(".stdout").unwrap();
            assert!(
                Path::new(&format!("tests/ghc-golden/{sub}/{name}")).exists(),
                "pinned divergence divergent/{sub}/{name} has no matching golden"
            );
            assert!(
                divergences_md.contains(stem),
                "pinned divergence divergent/{sub}/{name} is not documented in \
                 tests/ghc-golden/DIVERGENCES.md"
            );
        }
    }
}
