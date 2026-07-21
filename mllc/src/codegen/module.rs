//! Module-level emission: data-type registration and the layout of the
//! generated file's body.
//!
//! `register_data_type` records constructor tags, newtypes, record accessors
//! and the LuaDict layouts (string-tagged enums, name-keyed records) so that
//! construction, pattern matching, accessors and the FFI decoder all agree on
//! one representation. `generate_module` emits the body: provenance locals,
//! forward declarations packed into `__mll_fn` slots to stay under Lua's
//! 200-local limit, constructors (`gen_data_constructors`), functions, and
//! the export table.

use crate::embed;
use crate::tir::*;
use crate::types::Ty;
use super::CodeGen;
use super::names::{lua_field_assign, lua_field_index, lua_quoted_string, sanitize_name};

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
        } else if def.is_luadict {
            if let Some(con) = def.constructors.first()
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
    }

    pub(super) fn generate_module(&mut self, module: &TModule) {
        self.emit_line("-- Generated by MATA-LL compiler (https://matall.org/)");
        // Compiler provenance stamped into every module: the mllc crate version
        // and the full git commit it was built from (see mllc/build.rs). Emitted
        // as top-level locals so they are present in every compiled file, and
        // surfaced through the export table below when the module has exports.
        self.emit_line(&format!(
            "local __MLLC_VERSION = {}",
            lua_quoted_string(env!("CARGO_PKG_VERSION"))
        ));
        self.emit_line(&format!(
            "local __MLLC_COMMIT = {}",
            lua_quoted_string(env!("MLLC_GIT_COMMIT"))
        ));
        self.emit_line("");

        // All Prelude runtime names are plain local functions — never thunks.
        // Seed concrete_vars so references skip __force throughout user code.
        for name in &[
            "__force", "__thunk", "__mll_seq", "__mll_cons", "__mll_lazy_cons", "__mll_head",
            "__mll_tail", "__mll_tail_lazy", "__mll_to_lua", "__lua_to_mll", "__mll_wrap_callback", "__mll_run", "__mll_run_tail", "__mll_run_st", "__mll_perform",
            "__mll_ffi_decode",
            "not_", "engage", "liftIO", "show", "error_", "max", "min", "undefined",
            "pure", "return_", "Just",
            "show_Integer", "show_Number", "show_String", "show_Bool",
            "show_List_", "show_Maybe", "show_ByteString", "show_HashMap",
            "show_Unit",
            "eq_Integer", "eq_Number", "eq_String", "eq_Bool", "eq_ByteString",
            "eq_Ordering", "eq_Unit",
            "ord_lt__Unit", "ord_gt__Unit", "ord_le__Unit", "ord_ge__Unit",
            "ord_compare__Unit",
            "ord_lt__Integer", "ord_lt__Number", "ord_lt__String",
            "ord_gt__Integer", "ord_gt__Number", "ord_gt__String",
            "ord_le__Integer", "ord_le__Number", "ord_le__String",
            "ord_ge__Integer", "ord_ge__Number", "ord_ge__String",
            "ord_compare__Integer", "ord_compare__Number", "ord_compare__String",
            "ord_lt__ByteString", "ord_gt__ByteString", "ord_le__ByteString",
            "ord_ge__ByteString", "ord_compare__ByteString",
            "head", "tail", "map", "filter", "take", "drop", "zipWith",
            "foldr", "foldl",
            "__mll_hashstr", "hashmap_empty", "hashmap_insert", "hashmap_lookup",
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
            "negate_Integer", "negate_Number", "abs_Integer", "abs_Number",
            "signum_Integer", "signum_Number", "fromInteger_Integer", "fromInteger_Number",
            "recip_Number", "fromRational_Number", "toInteger_Integer",
            "quotRem_Integer", "divMod_Integer",
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
            self.emit_line("local __mll_fn = {}");
            for (i, name) in all_fn_names.iter().enumerate() {
                let slot = i + 1; // 1-based Lua indexing
                self.fn_table.insert(name.clone(), slot);
                self.forward_declared.insert(name.clone());
                self.concrete_vars.insert(name.clone());
                self.top_level_names.insert(name.clone());
            }
        }

        // Emit constructors (now using fn_table slots)
        for def in &module.data_defs {
            self.gen_data_constructors(def);
        }

        // Emit newtype constructors (identity functions, now using fn_table slots)
        for name in &module.newtypes {
            if let Some(&slot) = self.fn_table.get(name.as_str()) {
                self.emit_line(&format!("__mll_fn[{}] = function(_v) return _v end", slot));
            } else {
                self.emit_line(&format!("local function {}(_v) return _v end", sanitize_name(name)));
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
                self.emit_line(&format!(
                    "__mll_fn[{}] = function(_v, ...) local _f = __force(__force(_v){}); if select(\"#\", ...) == 0 then return _f else return _f(...) end end",
                    slot, index));
            }
        }
        if !module.newtypes.is_empty() {
            self.emit_line("");
        }

        // Emit instance method functions
        if !module.instance_fns.is_empty() {
            self.emit_line("-- Typeclass instances");
        }
        for func in &module.instance_fns {
            self.gen_function(func);
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
                self.gen_function(func);
            }
        }
        if let Some(func) = main_fn {
            self.gen_function(func);
        }

        if module.has_main {
            self.emit_line("");
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
            self.emit_line("-- Entry point (run main unless this file was loaded via require)");
            self.emit_line("local __mll_arg1 = ...");
            let run_ref = self.lua_ref("__run");
            self.emit_line(&format!(
                "if __mll_arg1 == nil or (arg ~= nil and __mll_arg1 == arg[1]) then __mll_run({}()) end",
                run_ref
            ));
        }

        // Generate module return table for exports
        // Wrap each export so return values are deep-forced for Lua consumption
        if !module.exports.is_empty() || self.embed_var_export {
            // Collect export function types for type-directed FFI conversion
            let export_types: std::collections::HashMap<String, Ty> = module.functions.iter()
                .filter(|f| module.exports.contains(&f.name))
                .map(|f| (f.name.clone(), f.ty.clone()))
                .collect();

            self.emit_line("");
            self.emit_line("-- Exports");
            self.emit_indent();
            self.emit("return {\n");
            self.indent += 1;
            // Compiler provenance, exposed as module properties.
            self.emit_indent();
            self.emit("__MLLC_VERSION = __MLLC_VERSION,\n");
            self.emit_indent();
            self.emit("__MLLC_COMMIT = __MLLC_COMMIT,\n");
            if self.embed_var_export {
                // The embedded original source (a plain Lua string bound at
                // the very top of the file — see embed.rs).
                self.emit_indent();
                self.emit(&format!("{0} = {0},\n", embed::SOURCE_VAR));
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
                    self.emit_indent();
                    self.emit(&format!("{} = (function()\n", sanitize_name(name)));
                    self.indent += 1;
                    self.emit_indent();
                    self.emit(&format!("local __result = __force({})\n", fn_ref));
                    match res_marshal {
                        Some(desc) => {
                            // Same contract as a function result: an empty MLL
                            // list / Nothing is nil at the Lua boundary.
                            self.emit_indent();
                            self.emit("if __result == nil then return nil end\n");
                            self.emit_indent();
                            self.emit(&format!("return __mll_arg_marshal(__result, {})\n", desc));
                        }
                        None => {
                            self.emit_indent();
                            self.emit("return __mll_to_lua(__result)\n");
                        }
                    }
                    self.indent -= 1;
                    self.emit_indent();
                    self.emit("end)(),\n");
                    continue;
                }

                let params: Vec<String> = (0..n_args).map(|i| format!("a{}", i + 1)).collect();
                let params_str = if n_args > 0 { params.join(", ") } else { "...".to_string() };

                self.emit_indent();
                self.emit(&format!("{} = function({params_str})\n", sanitize_name(name)));
                self.indent += 1;

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
                        self.emit_indent();
                        self.emit(&format!(
                            "if type({arg}) == \"function\" then {arg} = __mll_wrap_callback_in({arg}, {}, {{{}}}, {}, {:?}) end\n",
                            cb_args.len(), out_descs.join(", "), ret_desc, root));
                    } else if let Some((desc, _)) =
                        self.ffi_decode_desc_inner(ty, &mut Vec::new(), None)
                    {
                        let root = format!(
                            "in argument {} of the exported function '{}'",
                            i + 1, name
                        );
                        self.emit_indent();
                        self.emit(&format!(
                            "{arg} = __mll_ffi_decode({desc}, {arg}, {root:?}, \"argument\")\n"
                        ));
                    }
                }

                if n_args == 0 {
                    // Fallback for exports without type info
                    self.emit_indent();
                    self.emit(&"local args = {n = select('#', ...), ...}\n".to_string());
                    self.emit_indent();
                    self.emit("for i = 1, args.n do args[i] = __lua_to_mll(args[i]) end\n");
                }

                self.emit_indent();
                let fn_ref = self.lua_ref(&sname);
                let call_args = if n_args > 0 { params.join(", ") } else { "__unpack(args, 1, args.n)".to_string() };
                self.emit(&format!("local __result = __force({})({call_args})\n", fn_ref));
                // Run the result if it is an action, unwrapping a pure box the
                // same way __mll_run does — a body ending in `pure e` hands back
                // its result boxed (not forced), and the marshalling below must
                // see the value, not the box.
                self.emit_indent();
                self.emit("if getmetatable(__result) == __mll_pure_mt then __result = __result[1]\n");
                self.emit_indent();
                self.emit("elseif type(__result) == \"function\" then __result = __mll_unbox(__result()) end\n");
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
                        // An empty MLL list (and Nothing) is nil at the Lua
                        // boundary — the documented export contract — so keep
                        // the top-level nil short-circuit the legacy deep-force
                        // conversion had. (At the FFI *argument* edge an empty
                        // list is a fresh {} so hosts can ipairs it; the export
                        // result contract predates that and is pinned by
                        // ffi_export_string_lists.)
                        self.emit_indent();
                        self.emit("__result = __force(__result)\n");
                        self.emit_indent();
                        self.emit("if __result == nil then return nil end\n");
                        self.emit_indent();
                        self.emit(&format!("return __mll_arg_marshal(__result, {})\n", desc));
                    }
                    None => {
                        self.emit_indent();
                        self.emit("return __mll_to_lua(__result)\n");
                    }
                }
                self.indent -= 1;
                self.emit_indent();
                self.emit("end,\n");
            }
            self.indent -= 1;
            self.emit_line("}");
        }
    }

    pub(super) fn gen_data_constructors(&mut self, def: &TDataDef) {
        let is_enum = def.constructors.iter().all(|c| matches!(&c.fields, TConFields::Positional(f) if f.is_empty()));
        let single = def.constructors.len() == 1;

        for (i, con) in def.constructors.iter().enumerate() {
            let tag = i + 1;
            let field_count = match &con.fields {
                TConFields::Positional(f) => f.len(),
                TConFields::Named(f) => f.len(),
            };

            let decl = self.var_decl(&sanitize_name(&con.name));

            if field_count == 0 {
                if let Some(str_tag) = self.luadict_enum_tag.get(&con.name) {
                    // LuaDict enum: the constructor *is* its Lua string tag.
                    self.emit_line(&format!("{}{}", decl, lua_quoted_string(str_tag)));
                } else if is_enum {
                    self.emit_line(&format!("{}{}", decl, tag));
                } else {
                    self.emit_line(&format!("{}{{{}}}", decl, tag));
                }
            } else {
                let params: Vec<String> = (0..field_count).map(|i| format!("_p{}", i)).collect();
                let params_str = params.join(", ");
                if let Some(field_names) = self.luadict_con_fields.get(&con.name).cloned() {
                    // LuaDict: build a table keyed by field name for Lua interop,
                    // `function(_p0, _p1) return {width = _p0, height = _p1} end`.
                    let entries: Vec<String> = field_names.iter().zip(params.iter())
                        .map(|(fname, p)| format!("{}{}", lua_field_assign(fname), p))
                        .collect();
                    self.emit_line(&format!("{}function({}) return {{{}}} end", decl, params_str, entries.join(", ")));
                } else if single {
                    self.emit_line(&format!("{}function({}) return {{{}}} end", decl, params_str, params_str));
                } else {
                    let mut entries = vec![format!("{}", tag)];
                    entries.extend(params.iter().cloned());
                    self.emit_line(&format!("{}function({}) return {{{}}} end", decl, params_str, entries.join(", ")));
                }
            }
        }
        self.emit_line("");
    }
}
