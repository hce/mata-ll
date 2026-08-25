//! Module-level emission: data-type registration and the layout of the
//! generated file's body.
//!
//! `register_data_type` records constructor tags, newtypes, record accessors
//! and the LuaDict layouts (string-tagged enums, name-keyed records) so that
//! construction, pattern matching, accessors and the FFI decoder all agree on
//! one representation. `module_stmts` builds the body as one statement list:
//! provenance locals, forward declarations packed into `__mll_fn` slots to
//! stay under Lua's 200-local limit, constructors
//! (`data_constructor_stmts`), functions, and the export table
//! (`Stmt::ReturnTable`). Boundary template lines (the accessor wrapper, the
//! export runner's box-unwrapping, the entry-point condition) stay as
//! `Stmt::Raw` bridges.

use crate::embed;
use crate::tir::*;
use crate::types::Ty;
use super::CodeGen;
use super::lua::{Block, Expr, FuncBody, Item, Stmt};
use super::names::{lua_field_assign, lua_field_index, lua_quoted_string, sanitize_name};

/// First line of the entry-point section. `generate` (codegen/mod.rs) locates
/// this exact `Stmt::Raw` in the finished statement list to compute
/// `CompileResult::entry_point_start`, so the section boundary is defined
/// once, here, at emission — front-ends must consume the published offset,
/// never rediscover the boundary by scanning the rendered text.
pub(super) const ENTRY_POINT_COMMENT: &str =
    "-- Entry point (run main unless this file was loaded via require)";

impl CodeGen {
    pub(super) fn register_data_type(&mut self, def: &TDataDef) {
        let is_enum = def.constructors.iter().all(|c| matches!(&c.fields, TConFields::Positional(f) if f.is_empty()));
        for (i, con) in def.constructors.iter().enumerate() {
            // `constructor_info` resolves by first match, so a second
            // registration of the same name with a different tag would be
            // silently ignored while the typechecker (last-writer-wins map)
            // used the other one — the exact split-brain that produced silent
            // miscompiles before constructor shadowing/duplicate detection.
            // The typechecker guarantees unique names here; enforce it.
            if let Some((prev_ty, prev_idx, prev_total, prev_enum)) = self.constructors.iter()
                .find(|(cn, ..)| cn == &con.name)
                .map(|(_, tn, idx, total, en)| (tn.clone(), *idx, *total, *en))
                && (prev_ty.as_str(), prev_idx, prev_total, prev_enum)
                    != (def.name.as_str(), i + 1, def.constructors.len(), is_enum) {
                    panic!(
                        "internal compiler error: constructor '{}' of '{}' reached codegen \
                         under the same name as a constructor of '{}' with a different tag — \
                         the typechecker's duplicate/shadowing handling should have prevented this",
                        con.name, def.name, prev_ty,
                    );
                }
            self.constructors.push((con.name.clone(), def.name.clone(), i + 1, def.constructors.len(), is_enum));
        }
        // A LuaDict type is one of two shapes (validated by the typechecker's
        // derive_luadict): an all-field-less sum type laid out as Lua strings,
        // or a single record constructor laid out as a name-keyed table.
        // Distinguish them the same way the typechecker does — by whether every
        // constructor is field-less — so construction and pattern matching
        // agree, including on the degenerate empty-record case.
        let all_field_less = def.constructors.iter().all(|c| match &c.fields {
            TConFields::Positional(fs) => fs.is_empty(),
            TConFields::Named(fs) => fs.is_empty(),
        });
        if def.is_luadict && all_field_less {
            // LuaDict all-nullary sum type: each constructor is a Lua string at
            // runtime (its `as "tag"` rename, or its name). Record the tag so
            // construction and pattern matching agree on the string.
            for con in &def.constructors {
                self.luadict_enum_tag.insert(con.name.clone(), con.effective_tag().to_string());
            }
        } else if def.is_luadict
            && let Some(con) = def.constructors.first()
                && let TConFields::Named(fields) = &con.fields {
                    // Every map stores the *effective* Lua key — the `as "key"`
                    // rename when present, the Haskell field name otherwise —
                    // so construction, pattern matching, accessors, record
                    // update and the FFI decoder all agree on the table layout.
                    let keys: Vec<String> = fields.iter()
                        .map(|f| f.effective_key().to_string())
                        .collect();
                    for field in fields {
                        self.luadict_field_key.insert(
                            sanitize_name(&field.name),
                            field.effective_key().to_string(),
                        );
                    }
                    self.luadict_con_fields.insert(con.name.clone(), keys);
                    // Keyed by *type* name (as referenced in FFI result types),
                    // with field types retained for the FFI-boundary decoder.
                    self.luadict_type_fields.insert(
                        def.name.clone(),
                        (def.type_vars.clone(),
                         fields.iter().map(|f| (f.effective_key().to_string(), f.ty.clone())).collect()),
                    );
                }
    }

    pub(super) fn module_stmts(&mut self, module: &TModule) -> Vec<Stmt> {
        // Compiler provenance stamped into every module: the mllc crate version
        // and the full git commit it was built from (see mllc/build.rs). Emitted
        // as top-level locals so they are present in every compiled file, and
        // surfaced through the export table below when the module has exports.
        let mut stmts = vec![
            Stmt::Raw("-- Generated by the mata-ll compiler (https://matall.org/)".into()),
            Stmt::Local(
                vec!["__MLLC_VERSION".into()],
                Some(Expr::lit(lua_quoted_string(env!("CARGO_PKG_VERSION").as_bytes()))),
            ),
            Stmt::Local(
                vec!["__MLLC_COMMIT".into()],
                Some(Expr::lit(lua_quoted_string(env!("MLLC_GIT_COMMIT").as_bytes()))),
            ),
            Stmt::Raw(String::new()),
        ];

        // All Prelude runtime names are plain local functions — never thunks.
        // Seed concrete_vars so references skip __force throughout user code.
        // `undefined` must NOT be seeded: the runtime binds it to a THUNK
        // (`local undefined = __thunk(...)`), and concreteness here feeds
        // is_cheap_to_force / pure_value_bare_is_safe — seeding it claimed
        // forcing `undefined` is a harmless no-op, so `pure undefined`
        // escaped BARE and the consumer's `__mll_run` forced it, raising
        // where GHC binds the bottom unforced (`r <- g 0` with
        // `g n = ... pure undefined ...` must not raise until r is
        // demanded; runghc-confirmed, pinned as case_pure_bottom /
        // if_pure_bottom).
        for name in &[
            "__force", "__thunk", "__mll_seq", "__mll_cons", "__mll_lazy_cons", "__mll_head",
            "__mll_tail", "__mll_tail_lazy", "__mll_to_lua", "__lua_to_mll", "__mll_wrap_callback", "__mll_run", "__mll_run_tail", "__mll_run_st",
            "__mll_ffi_decode",
            "not_", "engage", "liftIO", "show", "error_",
            "pure", "return_", "Just",
            "minBound_Int", "maxBound_Int", "minBound_Bool", "maxBound_Bool",
            "show_Int", "show_Number", "show_String", "show_Bool",
            "show_List_", "show_Maybe", "show_ByteString", "show_HashMap",
            "show_Unit",
            "eq_Int", "eq_Number", "eq_String", "eq_Bool", "eq_ByteString",
            "eq_Ordering", "eq_Unit",
            "ord_lt__Unit", "ord_gt__Unit", "ord_le__Unit", "ord_ge__Unit",
            "ord_compare__Unit",
            "ord_lt__Int", "ord_lt__Number", "ord_lt__String",
            "ord_gt__Int", "ord_gt__Number", "ord_gt__String",
            "ord_le__Int", "ord_le__Number", "ord_le__String",
            "ord_ge__Int", "ord_ge__Number", "ord_ge__String",
            "ord_compare__Int", "ord_compare__Number", "ord_compare__String",
            "ord_lt__ByteString", "ord_gt__ByteString", "ord_le__ByteString",
            "ord_ge__ByteString", "ord_compare__ByteString",
            "ord_max__Int", "ord_max__Number", "ord_max__String",
            "ord_max__ByteString", "ord_max__Unit",
            "ord_min__Int", "ord_min__Number", "ord_min__String",
            "ord_min__ByteString", "ord_min__Unit",
            "__mll_bool_n", "ord_lt__Bool", "ord_gt__Bool", "ord_le__Bool",
            "ord_ge__Bool", "ord_compare__Bool", "ord_max__Bool", "ord_min__Bool",
            "head", "tail", "map", "filter", "take", "drop", "zipWith",
            "foldr", "foldl",
            "__mll_hashstr", "__mll_hm_lt", "hashmap_empty", "hashmap_insert", "hashmap_lookup",
            "hashmap_delete", "hashmap_size", "hashmap_keys", "hashmap_values",
            "hashmap_member", "hashmap_fromList", "hashmap_toList",
            "__mll_list_append", "__mll_list_index", "semigroup_String",
            "__mll_show_list", "__mll_show_arg", "__mll_show_maybe", "__mll_list_eq", "__mll_maybe_eq", "__mll_eq",
            "__mll_try", "__mll_pcall", "__mll_iter", "getArgs", "exit_",
            "try_", "catch_",
            "__mll_bxor", "__mll_band", "__mll_bor", "__mll_bnot",
            "__mll_shl", "__mll_shr", "__mll_math_type",
            "__mll_div", "__mll_mod", "__mll_div_fn", "__mll_mod_fn",
            "__mll_quot", "__mll_rem", "__mll_quot_fn", "__mll_rem_fn",
            "negate_Int", "negate_Number", "abs_Int", "abs_Number",
            "signum_Int", "signum_Number", "fromInteger_Int", "fromInteger_Number",
            "recip_Number", "fromRational_Number",
            "quotRem_Int", "divMod_Int",
            "__mll_array_from_list", "__mll_array_index", "__mll_array_length",
            "__mll_bs_empty", "__mll_bs",
            "__mll_ma_new", "__mll_ma_read", "__mll_ma_write",
            "__mll_ma_modify", "__mll_ma_length", "__mll_ma_from_list",
            "__mll_ma_to_list",
        ] {
            self.concrete_vars.insert(name.to_string());
            self.top_level_names.insert(name.to_string());
        }

        // Also register builtin names that go through sanitize_name mapping
        // (ByteString, MutArray, HashMap ops, etc.) so has_unknown_call
        // recognizes them as known top-level functions.
        for name in &[
            "bsEmpty", "bsLength", "bsIndex", "bsSub", "bsSingleton",
            "bsConcat", "bsNull", "bsHead", "bsTail", "bsCons", "bsSnoc",
            "bsReplicate", "bsPack", "bsUnpack", "bsMap", "bsFoldl",
            "bsXor", "bsZipWith", "bsToString", "bsFromString",
            "bsGetU16LE", "bsGetU32LE", "bsGetI8", "bsGetI16LE",
            "bsPutI16LE", "bsConcatList",
            "runST", "newSTArray", "readSTArray", "writeSTArray",
            "modifySTArray", "stArrayLength", "newSTArrayFromList",
            "stArrayToList",
            "hmEmpty", "hmInsert", "hmLookup", "hmDelete", "hmSize",
            "hmKeys", "hmValues", "hmMember", "hmFromList", "hmToList",
            "return", "pure", "not", "print", "error", "show", "undefined",
            "try", "catch",
        ] {
            self.top_level_names.insert(sanitize_name(name));
        }

        // Register EVERY data definition — including the ones constructor-DCE
        // dropped from emission. A dropped type's values can still flow
        // through live code (an FFI result read only via accessors, a raw
        // `try`-built Either), so its metadata (tags, LuaDict string tags and
        // field keys, FFI-decoder field types) must stay available; only its
        // constructor functions are omitted (no `__mll_fn` slots, no
        // emission — see the two `module.data_defs`-only loops below).
        for def in module.data_defs.iter().chain(module.dropped_data_defs.iter()) {
            self.register_data_type(def);
        }
        self.newtypes = module.newtypes.clone();

        // Record field accessors: inline as direct table indexing instead of
        // emitting local functions (saves Lua local variable slots)
        for (name, idx) in &module.record_accessors {
            self.record_accessors.insert(sanitize_name(name), *idx);
        }

        // Forward-declare ALL module-level names in a single table to avoid
        // Lua's 200-local-variable limit. Constructors, newtypes, instance
        // methods, and user functions all get __mll_fn[N] slots.
        let mut all_fn_names: Vec<String> = Vec::new();

        // sanitize_name is not injective (`f'` -> f_prime, `not` -> not_):
        // two DISTINCT source names that sanitize alike would share one
        // fn-table key — and hence one slot, with the last-emitted
        // definition silently serving both names. Detect it here, where
        // every slot-receiving name passes with its source spelling still
        // in hand. Same-source duplicates stay allowed (the documented
        // user-wins case for names merged along two import paths).
        {
            let mut by_sanitized: std::collections::HashMap<String, &str> =
                std::collections::HashMap::new();
            let sources = module
                .functions
                .iter()
                .map(|f| f.name.as_str())
                .chain(module.instance_fns.iter().map(|f| f.name.as_str()))
                .chain(module.newtypes.iter().map(|n| n.as_str()))
                .chain(module.record_accessors.iter().map(|(n, _)| n.as_str()));
            for src in sources {
                let n = sanitize_name(src);
                match by_sanitized.get(n.as_str()) {
                    Some(prev) if *prev != src => {
                        self.name_collision_error = Some(format!(
                            "Definitions '{}' and '{}' collide: both compile to the \
                             Lua name '{}' (mata-ll maps identifiers onto valid Lua \
                             names — a prime becomes '_prime', a Lua keyword gains a \
                             trailing '_'), so one definition would silently replace \
                             the other. Rename one of them",
                            prev, src, n,
                        ));
                    }
                    _ => {
                        by_sanitized.insert(n, src);
                    }
                }
            }
        }

        // Data constructors
        for def in &module.data_defs {
            for con in &def.constructors {
                if !self.concrete_vars.contains(&con.name) {
                    all_fn_names.push(con.name.clone());
                }
            }
        }
        // Newtype constructors
        for name in &module.newtypes {
            if !self.concrete_vars.contains(name) {
                all_fn_names.push(name.clone());
            }
        }
        // Instance functions
        for f in &module.instance_fns {
            let n = sanitize_name(&f.name);
            if !n.starts_with("__mll_") && !self.concrete_vars.contains(&n) {
                all_fn_names.push(n);
            }
        }
        // User functions
        for f in &module.functions {
            let n = sanitize_name(&f.name);
            if !n.starts_with("__mll_") {
                all_fn_names.push(n);
            }
        }
        // Record field accessors also get a real first-class function (in
        // addition to the inline fast-path), so they work as values
        // (`map field xs`) and when over-applied (`fnField r x`).
        for (name, _idx) in &module.record_accessors {
            let n = sanitize_name(name);
            if !self.fn_table.contains_key(&n) && !all_fn_names.contains(&n) {
                all_fn_names.push(n);
            }
        }

        if !all_fn_names.is_empty() {
            // A slot that will be assigned a THUNKED value binding must not
            // be seeded concrete: a reference emitted before the binding
            // (forward reference, or any earlier-emitted body) consults
            // concrete_vars to decide its `__force`, and a missed force
            // hands strict positions a raw thunk — a False Bool CAF defined
            // after its user read as a truthy table in an `if`. The
            // prediction mirrors function_stmts' own concreteness outcome
            // (debug_asserted there); constructors, newtypes and record
            // accessors are always function values.
            let thunked_slots: std::collections::HashSet<String> = module
                .functions
                .iter()
                .chain(module.instance_fns.iter())
                .filter(|f| !Self::slot_always_whnf(f))
                .map(|f| sanitize_name(&f.name))
                .collect();
            stmts.push(Stmt::Local(vec!["__mll_fn".into()], Some(Expr::Table(vec![]))));
            for (i, name) in all_fn_names.iter().enumerate() {
                let slot = i + 1; // 1-based Lua indexing
                self.fn_table.insert(name.clone(), slot);
                self.forward_declared.insert(name.clone());
                if !thunked_slots.contains(name) {
                    self.concrete_vars.insert(name.clone());
                }
                self.top_level_names.insert(name.clone());
            }
        }

        // Direct-perform classification, whole-module and BEFORE any body is
        // emitted: a tail terminal that saturates a call to any of these
        // functions returns bare (see action_run_ast), and the callee may be
        // defined later in the file (mutual recursion). The predicate
        // mirrors function_stmts' arms, which debug_assert agreement at
        // emission — the same discipline as the concreteness prediction
        // above. A name may be defined MORE THAN ONCE in the merged module
        // (a definition reached along two import paths, or a user
        // redefinition of an imported name — the documented user-wins case);
        // all such definitions write the same slot and the last-emitted one
        // is the one calls reach. When the duplicates classify identically
        // the prediction is unambiguous and stays; when they disagree the
        // name is excluded (every call keeps its runner, which is always
        // sound) and exempted from the emission-time agreement assert.
        {
            let mut classified: std::collections::HashMap<&str, Option<usize>> =
                std::collections::HashMap::new();
            for f in module.functions.iter().chain(module.instance_fns.iter()) {
                let arity = Self::direct_perform_arity(f);
                match classified.entry(f.name.as_str()) {
                    std::collections::hash_map::Entry::Vacant(v) => {
                        v.insert(arity);
                    }
                    std::collections::hash_map::Entry::Occupied(o) => {
                        if *o.get() != arity {
                            self.direct_perform_conflicts.insert(f.name.clone());
                        }
                    }
                }
            }
            for (name, arity) in classified {
                if let Some(arity) = arity
                    && !self.direct_perform_conflicts.contains(name)
                {
                    self.direct_perform_fns.insert(name.to_string(), arity);
                }
            }
        }

        // Emit constructors (now using fn_table slots)
        for def in &module.data_defs {
            stmts.extend(self.data_constructor_stmts(def));
        }

        // Emit newtype constructors (identity functions, now using fn_table slots)
        for name in &module.newtypes {
            if let Some(&slot) = self.fn_table.get(name.as_str()) {
                stmts.push(Stmt::Assign(
                    format!("__mll_fn[{}]", slot),
                    Expr::Func(
                        vec!["_v".into()],
                        FuncBody::Inline(vec![Stmt::Return(Expr::name("_v"))]),
                    ),
                ));
            } else {
                stmts.push(Stmt::Raw(format!(
                    "local function {}(_v) return _v end",
                    sanitize_name(name)
                )));
            }
        }

        // Emit record field accessors as real functions (the inline fast-path
        // at the application site handles the common `field r` case; this makes
        // the accessor first-class for higher-order and over-applied uses).
        // Extra args are forwarded so an over-applied function-typed field
        // (`fnField r x`) applies the projected function to them.
        for (name, idx) in &module.record_accessors {
            let n = sanitize_name(name);
            let index = match self.luadict_field_key.get(&n) {
                Some(key) => lua_field_index(key),
                None => format!("[{}]", idx),
            };
            if let Some(&slot) = self.fn_table.get(&n) {
                stmts.push(Stmt::Raw(format!(
                    "__mll_fn[{}] = function(_v, ...) local _f = __force(__force(_v){}); if select(\"#\", ...) == 0 then return _f else return _f(...) end end",
                    slot, index)));
            }
        }
        if !module.newtypes.is_empty() {
            stmts.push(Stmt::Raw(String::new()));
        }

        // Emit instance method functions
        if !module.instance_fns.is_empty() {
            stmts.push(Stmt::Raw("-- Typeclass instances".into()));
        }
        for func in &module.instance_fns {
            stmts.extend(self.function_stmts(func));
        }

        // Identify small pure functions eligible for inlining. Must run before
        // analyze_call_sites: cheapness of a call argument depends on whether
        // the callee is inlinable (a saturated call to an inlinable function is
        // cheap to evaluate eagerly).
        self.find_inline_candidates(module);

        // Whole-program call-site analysis: determine which function params
        // are always passed cheap (non-thunk) arguments at every call site.
        self.analyze_call_sites(module);

        // Emit functions (main last, so specializations are defined before use)
        let mut main_fn = None;
        for func in &module.functions {
            if func.name == "main" {
                main_fn = Some(func);
            } else {
                stmts.extend(self.function_stmts(func));
            }
        }
        if let Some(func) = main_fn {
            stmts.extend(self.function_stmts(func));
        }

        if module.has_main {
            stmts.push(Stmt::Raw(String::new()));
            // Entry point: run main() only when this file is the program being
            // executed, not when a Lua host `require`s it for its exports.
            //
            // A standalone interpreter (`lua prog.lua a b c`) passes the command
            // -line arguments as the chunk's varargs AND in the global `arg`
            // table, so the first vararg equals arg[1]. With no arguments the
            // first vararg is nil. `require "prog"` instead calls the chunk with
            // the MODULE NAME as its first vararg, which won't match arg[1]. So
            // testing only `... == nil` (as we used to) misfired the moment the
            // program was run standalone WITH arguments: the CLI arg looked like
            // a require modname and main was wrongly skipped.
            stmts.push(Stmt::Raw(ENTRY_POINT_COMMENT.into()));
            stmts.push(Stmt::Local(vec!["__mll_arg1".into()], Some(Expr::name("..."))));
            let run_ref = self.lua_ref("__run");
            stmts.push(Stmt::Raw(format!(
                "if __mll_arg1 == nil or (arg ~= nil and __mll_arg1 == arg[1]) then __mll_run({}()) end",
                run_ref
            )));
        }

        // Generate module return table for exports
        // Wrap each export so return values are deep-forced for Lua consumption
        if !module.exports.is_empty() || self.embed_var_export {
            // Collect export function types for type-directed FFI conversion
            let export_types: std::collections::HashMap<String, Ty> = module.functions.iter()
                .filter(|f| module.exports.contains(&f.name))
                .map(|f| (f.name.clone(), f.ty.clone()))
                .collect();

            stmts.push(Stmt::Raw(String::new()));
            stmts.push(Stmt::Raw("-- Exports".into()));
            let mut entries: Vec<(String, Expr)> = Vec::new();
            // Compiler provenance, exposed as module properties.
            entries.push(("__MLLC_VERSION".into(), Expr::name("__MLLC_VERSION")));
            entries.push(("__MLLC_COMMIT".into(), Expr::name("__MLLC_COMMIT")));
            if self.embed_var_export {
                // The embedded original source (a plain Lua string bound at
                // the very top of the file — see embed.rs).
                entries.push((embed::SOURCE_VAR.into(), Expr::name(embed::SOURCE_VAR)));
            }
            for name in &module.exports {
                let sname = sanitize_name(name);
                // Extract argument and result types from the function type
                let (arg_tys, res_ty) = if let Some(ty) = export_types.get(name) {
                    let mut args = Vec::new();
                    let mut t = ty;
                    while let Ty::Arrow(a, b, _) = t {
                        args.push(a.as_ref().clone());
                        t = b.as_ref();
                    }
                    (args, Some(t.clone()))
                } else {
                    (Vec::new(), None)
                };

                let n_args = arg_tys.len();

                // The export's TYPE decides its emitted form:
                //   * arrow type            → a calling wrapper (below).
                //   * IO / LuaIO action     → a calling wrapper that PERFORMS
                //                             the action when the host calls it.
                //   * anything else (a pure value: scalar, tuple, record/
                //     LuaDict, ADT, Maybe, finite list, …) → the FORCED VALUE
                //     marshalled directly, with NO call — `exports.foo = 123`,
                //     a record as a keyed table, a tuple as a positional table.
                //
                // A value must not be routed through the calling wrapper: the
                // wrapper emits `__force(fn_ref)(args)`, and for a value
                // `__force(fn_ref)` is the value itself (e.g. the number 123),
                // so calling it raises "attempt to call a number/table value".
                // We branch on the actual type after recognising IO/LuaIO — a
                // nullary pure value and a nullary IO action are both "0 args"
                // but marshal differently. A value reuses the EXACT same
                // result-marshalling contract a function's *return value* uses
                // (`__mll_arg_marshal` + type descriptor, or the deep-force
                // `__mll_to_lua` fallback for descriptor-less types), so a value
                // export supports precisely the types a function result does.
                // As with a function result, a lazy/infinite structure cannot
                // cross the strict Lua boundary — that is the inherent one-way
                // boundary property, not a case to detect or reject.
                let is_action = matches!(
                    res_ty.as_ref(),
                    Some(Ty::IO(_)) | Some(Ty::LuaIO(_, _))
                );
                if n_args == 0 && res_ty.is_some() && !is_action {
                    let fn_ref = self.lua_ref(&sname);
                    let res_marshal = res_ty.as_ref()
                        .and_then(|t| self.ffi_arg_marshal_desc(t, &mut Vec::new()));
                    let mut body = vec![Stmt::Local(
                        vec!["__result".into()],
                        Some(Expr::force(Expr::name(fn_ref))),
                    )];
                    match res_marshal {
                        Some(desc) => {
                            // Same contract as a function result: the descriptor
                            // decides what a nil representation means — an empty
                            // list becomes a fresh {}, Nothing stays nil.
                            body.push(Stmt::Return(Expr::call_named(
                                "__mll_arg_marshal",
                                vec![Expr::name("__result"), Expr::raw(desc)],
                            )));
                        }
                        None => {
                            body.push(Stmt::Return(Expr::call_named(
                                "__mll_to_lua",
                                vec![Expr::name("__result")],
                            )));
                        }
                    }
                    entries.push((
                        sanitize_name(name),
                        Expr::call(
                            Expr::paren(Expr::Func(vec![], FuncBody::Block(Block(body)))),
                            vec![],
                        ),
                    ));
                    continue;
                }

                let params: Vec<String> = (0..n_args).map(|i| format!("a{}", i + 1)).collect();
                let wrapper_params: Vec<String> =
                    if n_args > 0 { params.clone() } else { vec!["...".into()] };

                let mut body = Vec::new();

                // Type-directed argument conversion: every host-supplied
                // argument crosses the Lua→mata-ll boundary here and is
                // decoded by its declared type with the same descriptors an
                // FFI result uses — a `Maybe` gets its tagged wrapper, a
                // list becomes a cons list at every depth, structure nested
                // inside tuples/records/maps is decoded in place, and shape
                // mismatches fail with a localized error instead of
                // corrupting silently.
                for (i, ty) in arg_tys.iter().enumerate() {
                    let arg = params[i].clone();
                    if matches!(ty, Ty::Arrow(..)) {
                        // A host callback: mata-ll code will call it, so its
                        // arguments cross mata-ll→Lua (marshalled by their
                        // declared types) and its result crosses Lua→mata-ll
                        // (decoded by the declared result type).
                        let (cb_args, cb_ret) = ty.peel_arrows();
                        let out_descs: Vec<String> = cb_args.iter()
                            .map(|t| {
                                if matches!(t, Ty::Arrow(..)) {
                                    // A nested mata-ll function handed to the
                                    // host callback: wrap it for positional
                                    // Lua calls (deep-force convention).
                                    "false".to_string()
                                } else {
                                    self.ffi_arg_marshal_desc(t, &mut Vec::new())
                                        .unwrap_or_else(|| "false".into())
                                }
                            })
                            .collect();
                        let produced = match cb_ret {
                            Ty::IO(a) | Ty::LuaIO(_, a) => a.as_ref(),
                            other => other,
                        };
                        let ret_desc = self
                            .ffi_decode_desc_inner(produced, &mut Vec::new(), None)
                            .map(|d| d.0)
                            .unwrap_or_else(|| "false".into());
                        let root = format!(
                            "in the result of a callback passed to the exported function '{}'",
                            name
                        );
                        body.push(Stmt::Raw(format!(
                            "if type({arg}) == \"function\" then {arg} = __mll_wrap_callback_in({arg}, {}, {{{}}}, {}, {:?}) end",
                            cb_args.len(), out_descs.join(", "), ret_desc, root)));
                    } else if let Some((desc, _)) =
                        self.ffi_decode_desc_inner(ty, &mut Vec::new(), None)
                    {
                        let root = format!(
                            "in argument {} of the exported function '{}'",
                            i + 1, name
                        );
                        body.push(Stmt::Assign(
                            arg.clone(),
                            Expr::call_named(
                                "__mll_ffi_decode",
                                vec![
                                    Expr::raw(desc),
                                    Expr::name(arg),
                                    Expr::raw(format!("{:?}", root)),
                                    Expr::raw("\"argument\""),
                                ],
                            ),
                        ));
                    }
                }

                if n_args == 0 {
                    // Fallback for exports without type info
                    body.push(Stmt::Raw("local args = {n = select('#', ...), ...}".into()));
                    body.push(Stmt::Raw(
                        "for i = 1, args.n do args[i] = __lua_to_mll(args[i]) end".into(),
                    ));
                }

                let fn_ref = self.lua_ref(&sname);
                let call_args: Vec<Expr> = if n_args > 0 {
                    params.iter().map(|p| Expr::name(p.clone())).collect()
                } else {
                    vec![Expr::raw("__unpack(args, 1, args.n)")]
                };
                body.push(Stmt::Local(
                    vec!["__result".into()],
                    Some(Expr::call(Expr::force(Expr::name(fn_ref)), call_args)),
                ));
                // Run the result if it is an action, unwrapping a pure box the
                // same way __mll_run does — a body ending in `pure e` hands back
                // its result boxed (not forced), and the marshalling below must
                // see the value, not the box.
                body.push(Stmt::Raw(
                    "if getmetatable(__result) == __mll_pure_mt then __result = __result[1]".into(),
                ));
                body.push(Stmt::Raw(
                    "elseif type(__result) == \"function\" then __result = __mll_unbox(__result()) end".into(),
                ));
                // Type-directed result conversion (mata-ll→Lua): the same
                // marshal descriptors an FFI argument of this type would get,
                // so e.g. interior Nothings in a [Maybe a] keep their
                // positions. Types without a descriptor (scalars, plain ADTs,
                // opaque values) keep the legacy deep-force conversion.
                let res_marshal = res_ty.as_ref().and_then(|t| {
                    let produced = match t {
                        Ty::IO(a) | Ty::LuaIO(_, a) => a.as_ref(),
                        other => other,
                    };
                    self.ffi_arg_marshal_desc(produced, &mut Vec::new())
                });
                match res_marshal {
                    Some(desc) => {
                        // The descriptor decides what a nil representation means
                        // at the boundary: an empty list becomes a fresh {} (the
                        // same conversion the FFI *argument* edge performs, so
                        // hosts can ipairs a list result without a nil check),
                        // Nothing stays nil, and a nil-guarded record/hashmap
                        // stays nil. There must be NO blanket top-level nil
                        // short-circuit here — it would collapse an empty list
                        // result to nil while the same list one level deeper
                        // (a `Just []`, a record field) marshals to {}.
                        body.push(Stmt::Return(Expr::call_named(
                            "__mll_arg_marshal",
                            vec![Expr::name("__result"), Expr::raw(desc)],
                        )));
                    }
                    None => {
                        body.push(Stmt::Return(Expr::call_named(
                            "__mll_to_lua",
                            vec![Expr::name("__result")],
                        )));
                    }
                }
                entries.push((
                    sanitize_name(name),
                    Expr::Func(wrapper_params, FuncBody::Block(Block(body))),
                ));
            }
            stmts.push(Stmt::ReturnTable(entries));
        }
        stmts
    }

    pub(super) fn data_constructor_stmts(&mut self, def: &TDataDef) -> Vec<Stmt> {
        let is_enum = def.constructors.iter().all(|c| matches!(&c.fields, TConFields::Positional(f) if f.is_empty()));
        let single = def.constructors.len() == 1;
        let mut stmts = Vec::new();

        for (i, con) in def.constructors.iter().enumerate() {
            let tag = i + 1;
            let field_count = match &con.fields {
                TConFields::Positional(f) => f.len(),
                TConFields::Named(f) => f.len(),
            };

            let sname = sanitize_name(&con.name);

            if field_count == 0 {
                if let Some(str_tag) = self.luadict_enum_tag.get(&con.name) {
                    // LuaDict enum: the constructor *is* its Lua string tag.
                    let tag_lit = Expr::lit(lua_quoted_string(str_tag.as_bytes()));
                    stmts.push(self.var_decl_stmt(&sname, tag_lit));
                } else if is_enum {
                    stmts.push(self.var_decl_stmt(&sname, Expr::lit(tag.to_string())));
                } else {
                    stmts.push(self.var_decl_stmt(
                        &sname,
                        Expr::Table(vec![Item::Pos(Expr::lit(tag.to_string()))]),
                    ));
                }
            } else {
                let params: Vec<String> = (0..field_count).map(|i| format!("_p{}", i)).collect();
                let table = if let Some(field_names) = self.luadict_con_fields.get(&con.name).cloned() {
                    // LuaDict: build a table keyed by field name for Lua interop,
                    // `function(_p0, _p1) return {width = _p0, height = _p1} end`.
                    Expr::Table(
                        field_names.iter().zip(params.iter())
                            .map(|(fname, p)| Item::KV(lua_field_assign(fname), Expr::name(p.clone())))
                            .collect(),
                    )
                } else if single {
                    Expr::Table(params.iter().map(|p| Item::Pos(Expr::name(p.clone()))).collect())
                } else {
                    let mut items = vec![Item::Pos(Expr::lit(tag.to_string()))];
                    items.extend(params.iter().map(|p| Item::Pos(Expr::name(p.clone()))));
                    Expr::Table(items)
                };
                let ctor = Expr::Func(params, FuncBody::Inline(vec![Stmt::Return(table)]));
                stmts.push(self.var_decl_stmt(&sname, ctor));
            }
        }
        stmts.push(Stmt::Raw(String::new()));
        stmts
    }
}
