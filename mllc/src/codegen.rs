use crate::demand::DemandInfo;
use crate::tir::*;
use crate::types::Ty;

/// Tracks constructor info for code generation.
struct CodeGen {
    /// (con_name, type_name, variant_index, total, is_enum)
    constructors: Vec<(String, String, usize, usize, bool)>,
    /// Newtype constructor names (identity at runtime)
    newtypes: Vec<String>,
    /// Names that have been forward-declared (skip `local` on definition)
    forward_declared: std::collections::HashSet<String>,
    /// Variables known to hold concrete values (not thunks), skip __force
    concrete_vars: std::collections::HashSet<String>,
    /// Whole-program call-site analysis: for each function, which param
    /// positions are always passed cheap (non-thunk) arguments at every call site.
    /// If true, the param never needs __force at entry.
    params_always_cheap: std::collections::HashMap<String, Vec<bool>>,
    /// Small pure functions eligible for inlining at call sites.
    /// Maps function name to (param_names, body).
    inline_fns: std::collections::HashMap<String, (Vec<String>, TExpr)>,
    /// Names that are top-level or prelude definitions (not local params/binds).
    /// Used to distinguish known-safe function calls from potentially expensive
    /// calls to unknown function parameters in non-strict contexts.
    top_level_names: std::collections::HashSet<String>,
    /// Record field accessors: maps accessor name to 1-based field index.
    /// Used to inline field access as direct table indexing.
    record_accessors: std::collections::HashMap<String, usize>,
    /// Function table: maps sanitized function names to __mll_fn[N] slots.
    /// Used to pack all forward-declared functions into a single table,
    /// avoiding Lua's 200-local-variable limit.
    fn_table: std::collections::HashMap<String, usize>,
    /// Demand analysis: per-function parameter strictness.
    demand_info: DemandInfo,
    output: String,
    indent: usize,
}

impl CodeGen {
    fn new() -> Self {
        CodeGen {
            constructors: Vec::new(), newtypes: Vec::new(),
            forward_declared: std::collections::HashSet::new(),
            concrete_vars: std::collections::HashSet::new(),
            params_always_cheap: std::collections::HashMap::new(),
            inline_fns: std::collections::HashMap::new(),
            top_level_names: std::collections::HashSet::new(),
            record_accessors: std::collections::HashMap::new(),
            fn_table: std::collections::HashMap::new(),
            demand_info: DemandInfo { strict_params: std::collections::HashMap::new() },
            output: String::new(), indent: 0,
        }
    }

    fn is_newtype(&self, name: &str) -> bool {
        self.newtypes.iter().any(|n| n == name)
    }

    /// Returns "local function name" or "name = function" depending on
    /// whether the name was forward-declared.
    /// Resolve a sanitized name to its Lua reference.
    /// Forward-declared names use __mll_fn[N], others use the name directly.
    fn lua_ref(&self, lua_name: &str) -> String {
        if let Some(&slot) = self.fn_table.get(lua_name) {
            format!("__mll_fn[{}]", slot)
        } else {
            lua_name.to_string()
        }
    }

    fn fn_decl(&self, lua_name: &str, params: &str) -> String {
        if let Some(&slot) = self.fn_table.get(lua_name) {
            format!("__mll_fn[{}] = function({})", slot, params)
        } else {
            format!("local function {}({})", lua_name, params)
        }
    }

    /// Returns "local name = " or "name = " depending on forward declaration.
    fn var_decl(&self, lua_name: &str) -> String {
        if let Some(&slot) = self.fn_table.get(lua_name) {
            format!("__mll_fn[{}] = ", slot)
        } else {
            format!("local {} = ", lua_name)
        }
    }

    /// Create a sub-CodeGen that shares lookup tables but has its own output buffer.
    fn new_sub(&self) -> CodeGen {
        let mut sub = CodeGen::new();
        sub.constructors = self.constructors.clone();
        sub.newtypes = self.newtypes.clone();
        sub.fn_table = self.fn_table.clone();
        sub.concrete_vars = self.concrete_vars.clone();
        sub.record_accessors = self.record_accessors.clone();
        sub.top_level_names = self.top_level_names.clone();
        sub
    }

    fn emit(&mut self, s: &str) { self.output.push_str(s); }
    fn emit_indent(&mut self) { for _ in 0..self.indent { self.output.push_str("    "); } }
    fn emit_line(&mut self, s: &str) { self.emit_indent(); self.output.push_str(s); self.output.push('\n'); }

    fn constructor_info(&self, name: &str) -> Option<(usize, usize, bool)> {
        for (cn, _, idx, total, is_enum) in &self.constructors {
            if cn == name { return Some((*idx, *total, *is_enum)); }
        }
        None
    }

    fn register_data_type(&mut self, def: &TDataDef) {
        let is_enum = def.constructors.iter().all(|c| matches!(&c.fields, TConFields::Positional(f) if f.is_empty()));
        for (i, con) in def.constructors.iter().enumerate() {
            self.constructors.push((con.name.clone(), def.name.clone(), i + 1, def.constructors.len(), is_enum));
        }
    }

    fn generate_module(&mut self, module: &TModule) {
        self.emit_line("-- Generated by MATA-LL compiler (https://matall.org/)");
        self.emit_line("");

        // All Prelude runtime names are plain local functions — never thunks.
        // Seed concrete_vars so references skip __force throughout user code.
        for name in &[
            "__force", "__thunk", "__mll_cons", "__mll_lazy_cons", "__mll_head",
            "__mll_tail", "__mll_to_lua", "__mll_wrap_callback", "__mll_run", "__mll_perform",
            "not_", "engage", "liftIO", "show", "error_", "max", "min", "undefined",
            "pure", "return_", "Just",
            "show_Integer", "show_Number", "show_String", "show_Bool",
            "show_List_", "show_Maybe", "show_ByteString", "show_HashMap",
            "eq_Integer", "eq_Number", "eq_String", "eq_Bool", "eq_ByteString",
            "eq_Ordering",
            "ord_lt__Integer", "ord_lt__Number", "ord_lt__String",
            "ord_gt__Integer", "ord_gt__Number", "ord_gt__String",
            "ord_le__Integer", "ord_le__Number", "ord_le__String",
            "ord_ge__Integer", "ord_ge__Number", "ord_ge__String",
            "head", "tail", "map", "filter", "take", "zipWith",
            "__mll_hashstr", "hashmap_empty", "hashmap_insert", "hashmap_lookup",
            "hashmap_delete", "hashmap_size", "hashmap_keys", "hashmap_values",
            "hashmap_member", "hashmap_fromList",
            "__mll_show_list", "__mll_list_eq", "__mll_maybe_eq",
            "__mll_try", "__mll_iter", "getArgs", "exit_",
            "__mll_bxor", "__mll_band", "__mll_bor", "__mll_bnot",
            "__mll_shl", "__mll_shr",
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
            "hmKeys", "hmValues", "hmMember", "hmFromList",
            "return", "pure", "not", "print", "error", "show", "undefined",
        ] {
            self.top_level_names.insert(sanitize_name(name));
        }

        for def in &module.data_defs {
            self.register_data_type(def);
        }
        self.newtypes = module.newtypes.clone();

        // Emit constructors
        for def in &module.data_defs {
            self.gen_data_constructors(def);
        }

        // Emit newtype constructors (identity functions)
        for name in &module.newtypes {
            self.emit_line(&format!("local function {}(_v) return _v end", name));
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

        // Record field accessors: inline as direct table indexing instead of
        // emitting local functions (saves Lua local variable slots)
        for (name, idx) in &module.record_accessors {
            self.record_accessors.insert(sanitize_name(name), *idx);
        }

        // Forward-declare all functions in a table to avoid Lua's
        // 200-local-variable limit. Each function gets __mll_fn[N].
        let all_fn_names: Vec<String> = module.functions.iter()
            .map(|f| sanitize_name(&f.name))
            .filter(|n| !n.starts_with("__mll_"))  // preamble builtins are already local
            .collect();
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

        // Whole-program call-site analysis: determine which function params
        // are always passed cheap (non-thunk) arguments at every call site.
        self.analyze_call_sites(module);

        // Identify small pure functions eligible for inlining
        self.find_inline_candidates(module);

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
            self.emit_line("-- Entry point (skip when loaded via require)");
            self.emit_line("local __mll_modname = ...");
            let run_ref = self.lua_ref("__run");
            self.emit_line(&format!("if __mll_modname == nil then __mll_run({}()) end", run_ref));
        }

        // Generate module return table for exports
        // Wrap each export so return values are deep-forced for Lua consumption
        if !module.exports.is_empty() {
            self.emit_line("");
            self.emit_line("-- Exports");
            self.emit_indent();
            self.emit("return {\n");
            self.indent += 1;
            for name in &module.exports {
                let sname = sanitize_name(name);
                self.emit_indent();
                self.emit(&format!("{name} = function(...)\n"));
                self.indent += 1;
                self.emit_indent();
                self.emit(&format!("local args = {{n = select('#', ...), ...}}\n"));
                self.emit_indent();
                self.emit("for i = 1, args.n do if type(args[i]) == \"function\" then args[i] = __mll_wrap_callback(args[i]) end end\n");
                self.emit_indent();
                let fn_ref = self.lua_ref(&sname);
                self.emit(&format!("local __result = {}(__unpack(args, 1, args.n))\n", fn_ref));
                self.emit_indent();
                self.emit("if type(__result) == \"function\" then __result = __result() end\n");
                self.emit_indent();
                self.emit("return __mll_to_lua(__result)\n");
                self.indent -= 1;
                self.emit_indent();
                self.emit("end,\n");
            }
            self.indent -= 1;
            self.emit_line("}");
        }
    }

    fn gen_data_constructors(&mut self, def: &TDataDef) {
        let is_enum = def.constructors.iter().all(|c| matches!(&c.fields, TConFields::Positional(f) if f.is_empty()));
        let single = def.constructors.len() == 1;

        for (i, con) in def.constructors.iter().enumerate() {
            let tag = i + 1;
            let field_count = match &con.fields {
                TConFields::Positional(f) => f.len(),
                TConFields::Named(f) => f.len(),
            };

            if field_count == 0 {
                if is_enum {
                    self.emit_line(&format!("local {} = {}", con.name, tag));
                } else {
                    self.emit_line(&format!("local {} = {{{}}}", con.name, tag));
                }
            } else {
                let params: Vec<String> = (0..field_count).map(|i| format!("_p{}", i)).collect();
                let params_str = params.join(", ");
                if single {
                    self.emit_line(&format!("local {} = function({}) return {{{}}} end", con.name, params_str, params_str));
                } else {
                    let mut entries = vec![format!("{}", tag)];
                    entries.extend(params.iter().cloned());
                    self.emit_line(&format!("local {} = function({}) return {{{}}} end", con.name, params_str, entries.join(", ")));
                }
            }
        }
        self.emit_line("");
    }

    fn gen_function(&mut self, func: &TFunction) {
        let lua_name = sanitize_name(&func.name);
        let clauses = &func.clauses;
        let saved_concrete = self.concrete_vars.clone();

        if clauses.is_empty() { self.concrete_vars = saved_concrete; return; }

        // Eta-expand: if the function has fewer patterns than type arrows,
        // add extra params so the Lua function matches the expected arity.
        // This handles point-free definitions like: f x = g x  (written as f = g)
        let type_arity = count_arrows(&func.ty);
        let pat_arity = if clauses[0].patterns.is_empty() { 0 } else { clauses[0].patterns.len() };
        let eta_count = if type_arity > pat_arity { type_arity - pat_arity } else { 0 };

        if clauses.len() == 1 && clauses[0].patterns.is_empty() && clauses[0].guards.is_empty() {
            // Check if this is a value binding (non-function type) or a
            // zero-arg function (IO action / thunk)
            let is_io_action = matches!(&func.ty, Ty::IO(_) | Ty::LuaIO(_, _) | Ty::Forall(_, _));

            let is_concrete;
            if is_io_action {
                // Wrap in a function (IO action, needs to be called)
                self.emit_indent();
                self.emit(&self.fn_decl(&lua_name, ""));
                self.emit("\n");
                self.indent += 1;
                self.gen_where_binds(&clauses[0].where_binds);
                self.emit_indent(); self.emit("return "); self.gen_expr(&clauses[0].body); self.emit("\n");
                self.indent -= 1;
                self.emit_line("end");
                is_concrete = true;
            } else if expr_references_name(&clauses[0].body, &func.name) {
                // Self-referencing value binding (e.g., infinite list)
                if !self.forward_declared.contains(&lua_name) {
                    self.emit_line(&format!("local {}", lua_name));
                }
                self.emit_indent();
                self.emit(&format!("{} = ", lua_name));
                self.gen_expr_lazy(&clauses[0].body, &func.name);
                self.emit("\n");
                is_concrete = true;
            } else if Self::is_cheap(&clauses[0].body) {
                // Cheap value binding — evaluate eagerly
                self.emit_indent();
                self.emit(&self.var_decl(&lua_name));
                self.gen_expr(&clauses[0].body);
                self.emit("\n");
                is_concrete = true;
            } else {
                // Expensive value binding — thunk for lazy evaluation
                self.emit_indent();
                self.emit(&self.var_decl(&lua_name));
                self.emit("__thunk(function() return ");
                self.gen_expr(&clauses[0].body);
                self.emit(" end)");
                self.emit("\n");
                is_concrete = false;
            }
            self.emit_line("");
            self.concrete_vars = saved_concrete;
            if is_concrete { self.concrete_vars.insert(lua_name); }
            return;
        }

        if clauses.len() == 1 && clauses[0].guards.is_empty() {
            let clause = &clauses[0];
            let dict_param_names: Vec<String> = func.dict_params.iter().map(|(_, p)| p.clone()).collect();
            let mut params: Vec<String> = (0..clause.patterns.len()).map(|i| format!("_arg{}", i)).collect();
            let eta_params: Vec<String> = (0..eta_count).map(|i| format!("_eta{}", i)).collect();
            params.extend(eta_params.iter().cloned());
            let mut all_params = dict_param_names.clone();
            all_params.extend(params.iter().cloned());
            let params_str = all_params.join(", ");
            self.emit_indent();
            self.emit(&self.fn_decl(&lua_name, &params_str));
            self.emit("\n");
            self.indent += 1;
            // The function name is concrete (it's a function value) — allow
            // self-recursive calls to skip __force
            self.concrete_vars.insert(lua_name.clone());
            for dp in &dict_param_names { self.concrete_vars.insert(dp.clone()); }

            let all_simple = clause.patterns.iter().all(|p| matches!(p, TPattern::Var(_, _) | TPattern::Wildcard));
            if all_simple {
                // Mark params concrete based on call-site and demand analysis:
                // - If all callers pass cheap args, skip __force entirely.
                // - If demand analysis says param is strict, force at entry.
                // - Otherwise, stay lazy (param might never be used).
                let call_site_cheap = self.params_always_cheap.get(&func.name).cloned();
                let demand_strict = self.demand_info.strict_params.get(&func.name).cloned();
                for (i, pat) in clause.patterns.iter().enumerate() {
                    if let TPattern::Var(v, _) = pat {
                        let sname = sanitize_name(v);
                        let always_cheap = call_site_cheap.as_ref().map_or(false, |v| v.get(i).copied().unwrap_or(false));
                        let is_strict = demand_strict.as_ref().map_or(false, |v| v.get(i).copied().unwrap_or(false));
                        if always_cheap {
                            // All callers pass concrete values — no __force needed
                            self.emit_line(&format!("local {} = _arg{}", sname, i));
                            self.concrete_vars.insert(sname);
                        } else if is_strict {
                            // Demand analysis: body forces this param — force at entry
                            self.emit_line(&format!("local {} = __force(_arg{})", sname, i));
                            self.concrete_vars.insert(sname);
                        } else {
                            // Not demanded — stay lazy
                            self.emit_line(&format!("local {} = _arg{}", sname, i));
                        }
                    }
                }
                self.gen_where_binds(&clause.where_binds);
                if eta_count > 0 {
                    // Eta-expand: apply extra params to the body
                    self.emit_indent(); self.emit("return __force(");
                    self.gen_expr(&clause.body);
                    self.emit(")(");
                    self.emit(&eta_params.join(", "));
                    self.emit(")\n");
                } else if Self::returns_action(&func.ty) {
                    // IO/ST-returning function: flatten bind chains, performing
                    // sub-actions directly. The function itself acts as the action
                    // closure — callers use gen_action to invoke it.
                    self.gen_bind_chain_io(&clause.body);
                } else {
                    // Pure function: use gen_bind_chain for the body so
                    // If/>>=/>> flatten into statements instead of IIFEs
                    self.gen_bind_chain(&clause.body);
                }
            } else {
                // Force only args that are destructured
                for (i, p) in params.iter().enumerate() {
                    if !matches!(&clause.patterns[i], TPattern::Var(_, _) | TPattern::Wildcard) {
                        self.emit_line(&format!("{} = __force({})", p, p));
                        self.concrete_vars.insert(p.clone());
                    }
                }
                self.gen_where_binds(&clause.where_binds);
                self.gen_pattern_match(&params, clauses);
            }
            self.indent -= 1;
            self.emit_line("end");
            self.emit_line("");
            self.concrete_vars = saved_concrete;
            self.concrete_vars.insert(lua_name);
            return;
        }

        // Multiple clauses or guards
        let dict_param_names: Vec<String> = func.dict_params.iter().map(|(_, p)| p.clone()).collect();
        let num_params = clauses.iter().map(|c| c.patterns.len()).max().unwrap_or(0);
        let mut params: Vec<String> = (0..num_params).map(|i| format!("_arg{}", i)).collect();
        let eta_params_multi: Vec<String> = (0..eta_count).map(|i| format!("_eta{}", i)).collect();
        params.extend(eta_params_multi.iter().cloned());
        let mut all_params = dict_param_names.clone();
        all_params.extend(params.iter().cloned());
        let params_str = all_params.join(", ");
        self.emit_indent();
        self.emit(&self.fn_decl(&lua_name, &params_str));
        self.emit("\n");
        self.indent += 1;
        self.concrete_vars.insert(lua_name.clone());
        for dp in &dict_param_names { self.concrete_vars.insert(dp.clone()); }
        // Force params that are destructured OR where call-site analysis
        // shows all callers pass cheap args (so the value is already concrete).
        let call_site_cheap = self.params_always_cheap.get(&func.name).cloned();
        for (i, p) in params.iter().enumerate() {
            if i >= num_params { break; }
            let always_cheap = call_site_cheap.as_ref().map_or(false, |v| v.get(i).copied().unwrap_or(false));
            let needs_force = clauses.iter().any(|c| {
                c.patterns.get(i).map_or(false, |pat| {
                    !matches!(pat, TPattern::Var(_, _) | TPattern::Wildcard)
                })
            });
            if needs_force {
                // Destructured param — must force for pattern matching
                self.emit_line(&format!("{} = __force({})", p, p));
                self.concrete_vars.insert(p.clone());
            } else if always_cheap {
                // All callers pass concrete values — mark concrete, no force needed
                self.concrete_vars.insert(p.clone());
            }
        }
        self.gen_pattern_match(&params, clauses);
        self.indent -= 1;
        self.emit_line("end");
        self.emit_line("");
        self.concrete_vars = saved_concrete;
        self.concrete_vars.insert(lua_name);
    }

    fn gen_where_binds(&mut self, binds: &[TLocalDef]) {
        // Group bindings by name to merge multi-clause local functions
        let mut i = 0;
        while i < binds.len() {
            let bind = &binds[i];
            if bind.patterns.is_empty() {
                // Simple value binding: local x = expr
                let sname = sanitize_name(&bind.name);
                self.emit_indent();
                if Self::is_cheap(&bind.body) {
                    self.emit(&format!("local {} = ", sname));
                    self.gen_expr(&bind.body);
                    self.concrete_vars.insert(sname);
                } else {
                    self.emit(&format!("local {} = __thunk(function() return ", sname));
                    self.gen_expr(&bind.body);
                    self.emit(" end)");
                }
                self.emit("\n");
                i += 1;
            } else {
                // Collect all consecutive clauses with the same name
                let name = &bind.name;
                let mut clauses = Vec::new();
                let num_params = bind.patterns.len();
                while i < binds.len() && binds[i].name == *name && !binds[i].patterns.is_empty() {
                    clauses.push(TClause {
                        patterns: binds[i].patterns.clone(),
                        guards: vec![],
                        body: binds[i].body.clone(),
                        where_binds: vec![],
                    });
                    i += 1;
                }

                let params: Vec<String> = (0..num_params)
                    .map(|j| format!("_warg{}", j))
                    .collect();
                let params_str = params.join(", ");
                self.emit_indent();
                self.emit(&format!("local function {}({})\n",
                    sanitize_name(name), params_str));
                self.indent += 1;

                if clauses.len() == 1 {
                    let clause = &clauses[0];
                    let all_simple = clause.patterns.iter().all(|p|
                        matches!(p, TPattern::Var(_, _) | TPattern::Wildcard));

                    if all_simple {
                        for (j, pat) in clause.patterns.iter().enumerate() {
                            if let TPattern::Var(v, _) = pat {
                                self.emit_line(&format!("local {} = _warg{}", v, j));
                            }
                        }
                        self.emit_indent();
                        self.emit("return ");
                        self.gen_expr(&clause.body);
                        self.emit("\n");
                    } else {
                        // Force params that are pattern-matched (not just var/wildcard)
                        for (j, pat) in clause.patterns.iter().enumerate() {
                            if !matches!(pat, TPattern::Var(_, _) | TPattern::Wildcard) {
                                self.emit_line(&format!("_warg{} = __force(_warg{})", j, j));
                            }
                        }
                        self.gen_pattern_match(&params, &clauses);
                    }
                } else {
                    // Force params that are pattern-matched in any clause
                    for j in 0..num_params {
                        let needs_force = clauses.iter().any(|c| {
                            c.patterns.get(j).map_or(false, |pat| {
                                !matches!(pat, TPattern::Var(_, _) | TPattern::Wildcard)
                            })
                        });
                        if needs_force {
                            self.emit_line(&format!("_warg{} = __force(_warg{})", j, j));
                        }
                    }
                    self.gen_pattern_match(&params, &clauses);
                }

                self.indent -= 1;
                self.emit_line("end");
            }
        }
    }

    fn gen_pattern_match(&mut self, params: &[String], clauses: &[TClause]) {
        for (i, clause) in clauses.iter().enumerate() {
            let keyword = if i == 0 { "if" } else { "elseif" };

            if !clause.guards.is_empty() {
                let mut bindings = Vec::new();
                let mut conditions = Vec::new();
                for (pi, pat) in clause.patterns.iter().enumerate() {
                    self.collect_pattern_conditions(&params[pi], pat, &mut conditions, &mut bindings);
                }
                if !conditions.is_empty() {
                    // Wrap in a pattern-matching if block, then test guards inside
                    self.emit_indent();
                    self.emit(&format!("{} {} then\n", keyword, conditions.join(" and ")));
                    self.indent += 1;
                    for (var, val) in &bindings {
                        self.emit_line(&format!("local {} = {}", var, val));
                    }
                    self.gen_where_binds(&clause.where_binds);
                    for (gi, guard) in clause.guards.iter().enumerate() {
                        let gkw = if gi == 0 { "if" } else { "elseif" };
                        self.emit_indent(); self.emit(&format!("{} ", gkw));
                        let mut sub = self.new_sub();
                        sub.gen_expr(&guard.condition);
                        self.emit(&sub.output);
                        self.emit(" then\n");
                        self.indent += 1;
                        self.emit_indent(); self.emit("return "); self.gen_expr(&guard.body); self.emit("\n");
                        self.indent -= 1;
                    }
                    self.emit_line("end");
                    self.indent -= 1;
                } else {
                    // No pattern conditions, just guards
                    for (var, val) in &bindings {
                        self.emit_line(&format!("local {} = {}", var, val));
                    }
                    self.gen_where_binds(&clause.where_binds);
                    for (gi, guard) in clause.guards.iter().enumerate() {
                        let gkw = if i == 0 && gi == 0 { "if" } else { "elseif" };
                        self.emit_indent(); self.emit(&format!("{} ", gkw));
                        let mut sub = self.new_sub();
                        sub.gen_expr(&guard.condition);
                        self.emit(&sub.output);
                        self.emit(" then\n");
                        self.indent += 1;
                        self.emit_indent(); self.emit("return "); self.gen_expr(&guard.body); self.emit("\n");
                        self.indent -= 1;
                    }
                }
            } else {
                let mut conditions = Vec::new();
                let mut bindings = Vec::new();
                for (pi, pat) in clause.patterns.iter().enumerate() {
                    self.collect_pattern_conditions(&params[pi], pat, &mut conditions, &mut bindings);
                }

                if conditions.is_empty() {
                    if i > 0 { self.emit_indent(); self.emit("else\n"); self.indent += 1; }
                    for (var, val) in &bindings {
                        self.emit_line(&format!("local {} = {}", var, val));
                    }
                    self.gen_where_binds(&clause.where_binds);
                    self.emit_indent(); self.emit("return "); self.gen_expr(&clause.body); self.emit("\n");
                    if i > 0 { self.indent -= 1; self.emit_line("end"); }
                    return;
                }

                self.emit_indent();
                self.emit(&format!("{} {} then\n", keyword, conditions.join(" and ")));
                self.indent += 1;
                for (var, val) in &bindings {
                    self.emit_line(&format!("local {} = {}", var, val));
                }
                self.gen_where_binds(&clause.where_binds);
                self.emit_indent(); self.emit("return "); self.gen_expr(&clause.body); self.emit("\n");
                self.indent -= 1;
            }
        }
        self.emit_line("end");
        self.emit_line("error(\"Non-exhaustive patterns\")");
    }

    fn collect_pattern_conditions(&self, scrutinee: &str, pattern: &TPattern, conditions: &mut Vec<String>, bindings: &mut Vec<(String, String)>) {
        match pattern {
            TPattern::Var(name, _) => { bindings.push((sanitize_name(name), scrutinee.to_string())); }
            TPattern::Wildcard => {}
            TPattern::LitPat(lit) => {
                let s = match lit {
                    TLiteral::Integer(n) => format!("{}", n),
                    TLiteral::Number(n) => format!("{}", n),
                    TLiteral::Str(s) => format!("\"{}\"", s),
                    TLiteral::Bool(b) => if *b { "true".into() } else { "false".into() },
                    TLiteral::Unit => "nil".into(),
                };
                conditions.push(format!("{} == {}", scrutinee, s));
            }
            TPattern::Constructor { name, args } => {
                if self.is_newtype(name) {
                    // Newtype: zero-cost wrapper, value is the inner type directly
                    for arg in args {
                        self.collect_pattern_conditions(scrutinee, arg, conditions, bindings);
                    }
                } else if let Some((tag, total, is_enum)) = self.constructor_info(name) {
                    if is_enum {
                        conditions.push(format!("{} == {}", scrutinee, tag));
                    } else if total > 1 {
                        conditions.push(format!("{}[1] == {}", scrutinee, tag));
                        for (i, arg) in args.iter().enumerate() {
                            self.collect_pattern_conditions(&format!("{}[{}]", scrutinee, i + 2), arg, conditions, bindings);
                        }
                    } else {
                        for (i, arg) in args.iter().enumerate() {
                            self.collect_pattern_conditions(&format!("{}[{}]", scrutinee, i + 1), arg, conditions, bindings);
                        }
                    }
                } else {
                    match name.as_str() {
                        "True" => conditions.push(format!("{} == true", scrutinee)),
                        "False" => conditions.push(format!("{} == false", scrutinee)),
                        "Nothing" | "[]" => conditions.push(format!("{} == nil", scrutinee)),
                        "Just" => {
                            conditions.push(format!("{} ~= nil", scrutinee));
                            if let Some(arg) = args.first() {
                                self.collect_pattern_conditions(scrutinee, arg, conditions, bindings);
                            }
                        }
                        ":" => {
                            // Cons pattern: x:xs
                            conditions.push(format!("{} ~= nil", scrutinee));
                            if args.len() >= 1 {
                                self.collect_pattern_conditions(
                                    &format!("__mll_head({})", scrutinee),
                                    &args[0], conditions, bindings);
                            }
                            if args.len() >= 2 {
                                self.collect_pattern_conditions(
                                    &format!("__mll_tail({})", scrutinee),
                                    &args[1], conditions, bindings);
                            }
                        }
                        _ => conditions.push(format!("{} == {}", scrutinee, name)),
                    }
                }
            }
            TPattern::Paren(inner) => self.collect_pattern_conditions(scrutinee, inner, conditions, bindings),
            TPattern::Tuple(pats) => {
                // Tuple fields are at [1], [2], etc. (no tag)
                for (i, p) in pats.iter().enumerate() {
                    self.collect_pattern_conditions(
                        &format!("{}[{}]", scrutinee, i + 1),
                        p, conditions, bindings,
                    );
                }
            }
        }
    }

    /// Returns true if an expression is cheap enough that thunking it would
    /// cost more than evaluating it eagerly. This prevents thunk chain buildup
    /// Collect elements of a literal list (cons chain ending in nil).
    /// Returns Some(vec![elem1, elem2, ...]) if the list has >= 8 literal elements,
    /// None otherwise (let normal cons generation handle short lists).
    fn collect_list_literal<'a>(expr: &'a TExpr) -> Option<Vec<&'a TExpr>> {
        let mut elems = Vec::new();
        let mut cur = expr;
        loop {
            match &cur.kind {
                TExprKind::App(func, tail) => {
                    if let TExprKind::App(inner_f, elem) = &func.kind {
                        if let TExprKind::Con(name) = &inner_f.kind {
                            if name == ":" {
                                elems.push(elem.as_ref());
                                cur = tail.as_ref();
                                continue;
                            }
                        }
                    }
                    return None;
                }
                TExprKind::Con(name) if name == "[]" => {
                    if elems.len() >= 8 {
                        return Some(elems);
                    }
                    return None;
                }
                _ => return None,
            }
        }
    }

    /// in accumulator patterns while preserving laziness for expensive
    /// computations (user function calls).
    fn is_cheap(expr: &TExpr) -> bool {
        match &expr.kind {
            TExprKind::Lit(_) | TExprKind::Con(_) | TExprKind::Var(_)
            | TExprKind::Lambda { .. } | TExprKind::OpFunc(_) => true,
            TExprKind::Paren(inner) | TExprKind::Negate(inner) => Self::is_cheap(inner),
            TExprKind::Tuple(elems) => elems.iter().all(|e| Self::is_cheap(e)),
            TExprKind::InfixApp { op, lhs, rhs } => {
                // Builtin ops (arithmetic, comparison, concat) are cheap
                // if their operands are cheap
                is_builtin_op(op) && Self::is_cheap(lhs) && Self::is_cheap(rhs)
            }
            TExprKind::App(func, arg) => {
                // Constructor applications are cheap (just table creation).
                // General function applications are NOT cheap — the function
                // body might be expensive even if the args are cheap.
                if Self::is_con_app(expr) {
                    Self::is_cheap(arg) && Self::is_cheap(func)
                } else {
                    false
                }
            }
            TExprKind::If { cond, then_branch, else_branch } => {
                Self::is_cheap(cond) && Self::is_cheap(then_branch) && Self::is_cheap(else_branch)
            }
            // Function calls, case, let — potentially expensive, thunk them
            _ => false,
        }
    }

    /// Whole-program call-site analysis. For each function, determine which
    /// parameter positions always receive cheap (non-thunk) arguments.
    fn analyze_call_sites(&mut self, module: &TModule) {
        // Initialize: for each function, track (ever_thunked, ever_called) per param
        let mut ever_thunked: std::collections::HashMap<String, Vec<bool>> = std::collections::HashMap::new();
        let mut ever_called: std::collections::HashMap<String, Vec<bool>> = std::collections::HashMap::new();
        for func in module.functions.iter().chain(module.instance_fns.iter()) {
            let num_params = func.clauses.iter().map(|c| c.patterns.len()).max().unwrap_or(0);
            if num_params > 0 {
                ever_thunked.insert(func.name.clone(), vec![false; num_params]);
                ever_called.insert(func.name.clone(), vec![false; num_params]);
            }
        }
        // Scan all function bodies (and where-clause bodies) for call sites
        for func in module.functions.iter().chain(module.instance_fns.iter()) {
            for clause in &func.clauses {
                Self::scan_call_sites(&clause.body, &mut ever_thunked, &mut ever_called);
                for wb in &clause.where_binds {
                    Self::scan_call_sites(&wb.body, &mut ever_thunked, &mut ever_called);
                }
            }
        }
        // A param is always-cheap only if it was called at least once and
        // never received a thunk at any call site.
        for (name, thunked) in &ever_thunked {
            if let Some(called) = ever_called.get(name) {
                let cheap: Vec<bool> = thunked.iter().zip(called.iter())
                    .map(|(t, c)| *c && !*t)
                    .collect();
                self.params_always_cheap.insert(name.clone(), cheap);
            }
        }
    }

    fn scan_call_sites(expr: &TExpr,
        ever_thunked: &mut std::collections::HashMap<String, Vec<bool>>,
        ever_called: &mut std::collections::HashMap<String, Vec<bool>>,
    ) {
        match &expr.kind {
            TExprKind::App(_, _) => {
                let mut args: Vec<&TExpr> = vec![];
                let mut f = expr;
                while let TExprKind::App(inner_f, inner_arg) = &f.kind {
                    args.push(inner_arg.as_ref());
                    f = inner_f.as_ref();
                }
                args.reverse();
                if let TExprKind::Var(name) = &f.kind {
                    if let Some(thunked) = ever_thunked.get_mut(name.as_str()) {
                        let called = ever_called.get_mut(name.as_str()).unwrap();
                        for (i, arg) in args.iter().enumerate() {
                            if i < thunked.len() {
                                called[i] = true;
                                if !Self::is_cheap_arg(arg) {
                                    thunked[i] = true;
                                }
                            }
                        }
                    }
                }
                for arg in &args {
                    Self::scan_call_sites(arg, ever_thunked, ever_called);
                }
                if !matches!(&f.kind, TExprKind::Var(_) | TExprKind::Con(_)) {
                    Self::scan_call_sites(f, ever_thunked, ever_called);
                }
            }
            TExprKind::InfixApp { lhs, rhs, .. } => {
                Self::scan_call_sites(lhs, ever_thunked, ever_called);
                Self::scan_call_sites(rhs, ever_thunked, ever_called);
            }
            TExprKind::Lambda { body, .. } => Self::scan_call_sites(body, ever_thunked, ever_called),
            TExprKind::If { cond, then_branch, else_branch } => {
                Self::scan_call_sites(cond, ever_thunked, ever_called);
                Self::scan_call_sites(then_branch, ever_thunked, ever_called);
                Self::scan_call_sites(else_branch, ever_thunked, ever_called);
            }
            TExprKind::Let { binds, body } => {
                for bind in binds { Self::scan_call_sites(&bind.body, ever_thunked, ever_called); }
                Self::scan_call_sites(body, ever_thunked, ever_called);
            }
            TExprKind::Case { scrutinee, branches } => {
                Self::scan_call_sites(scrutinee, ever_thunked, ever_called);
                for b in branches { Self::scan_call_sites(&b.body, ever_thunked, ever_called); }
            }
            TExprKind::Paren(inner) | TExprKind::Negate(inner) => Self::scan_call_sites(inner, ever_thunked, ever_called),
            TExprKind::Tuple(elems) => { for e in elems { Self::scan_call_sites(e, ever_thunked, ever_called); } }
            TExprKind::SpecCall { args, .. } => { for a in args { Self::scan_call_sites(a, ever_thunked, ever_called); } }
            _ => {}
        }
    }

    /// Identify small pure functions eligible for inlining at call sites.
    /// Criteria: single clause, all-simple patterns, no guards, no where bindings,
    /// body is cheap, and not self-recursive.
    fn find_inline_candidates(&mut self, module: &TModule) {
        for func in module.functions.iter().chain(module.instance_fns.iter()) {
            if func.clauses.len() != 1 { continue; }
            let clause = &func.clauses[0];
            if !clause.guards.is_empty() || !clause.where_binds.is_empty() { continue; }
            if clause.patterns.is_empty() { continue; } // value binding, not a function
            let all_simple = clause.patterns.iter().all(|p| matches!(p, TPattern::Var(_, _)));
            if !all_simple { continue; }
            if !Self::is_cheap(&clause.body) { continue; }
            if expr_references_name(&clause.body, &func.name) { continue; } // recursive
            // Only inline bodies that are arithmetic/comparison expressions,
            // not constructor applications (which need special gen_expr handling)
            if Self::body_has_constructors(&clause.body) { continue; }
            let params: Vec<String> = clause.patterns.iter().map(|p| {
                if let TPattern::Var(name, _) = p { name.clone() } else { unreachable!() }
            }).collect();
            self.inline_fns.insert(func.name.clone(), (params, clause.body.clone()));
        }
    }

    /// Check if an expression contains constructor applications (Con nodes).
    /// These need special handling in gen_expr (e.g. : → __mll_cons) that
    /// gen_expr_subst doesn't replicate, so we skip inlining for them.
    fn body_has_constructors(expr: &TExpr) -> bool {
        match &expr.kind {
            TExprKind::Con(_) => true,
            TExprKind::App(f, a) => Self::body_has_constructors(f) || Self::body_has_constructors(a),
            TExprKind::InfixApp { lhs, rhs, .. } => Self::body_has_constructors(lhs) || Self::body_has_constructors(rhs),
            TExprKind::Paren(inner) | TExprKind::Negate(inner) => Self::body_has_constructors(inner),
            TExprKind::Tuple(elems) => elems.iter().any(|e| Self::body_has_constructors(e)),
            _ => false,
        }
    }

    /// Emit an expression with parameter substitution for inlining.
    /// Only recurses into sub-expressions that might contain substitution
    /// variables; delegates to gen_expr for everything else.
    fn gen_expr_subst(&mut self, expr: &TExpr, subst: &std::collections::HashMap<String, &TExpr>) {
        // If no substitution vars appear in this expr, use normal gen_expr
        // (which handles cons, list literals, etc. correctly)
        let has_subst_vars = subst.keys().any(|k| expr_references_name(expr, k));
        if !has_subst_vars {
            self.gen_expr(expr);
            return;
        }
        match &expr.kind {
            TExprKind::Var(name) => {
                if let Some(replacement) = subst.get(name.as_str()) {
                    self.gen_expr(replacement);
                } else {
                    self.gen_expr(expr);
                }
            }
            TExprKind::InfixApp { op, lhs, rhs } => {
                if op == "div" {
                    self.emit("math.floor(");
                    self.gen_expr_subst(lhs, subst);
                    self.emit(" / ");
                    self.gen_expr_subst(rhs, subst);
                    self.emit(")");
                    return;
                }
                let lua_op = match op.as_str() {
                    "++" => "..", "&&" => "and", "||" => "or", "/=" => "~=",
                    "mod" => "%",
                    other => other,
                };
                self.emit("(");
                self.gen_expr_subst(lhs, subst);
                self.emit(&format!(" {} ", lua_op));
                self.gen_expr_subst(rhs, subst);
                self.emit(")");
            }
            TExprKind::Paren(inner) => {
                self.emit("(");
                self.gen_expr_subst(inner, subst);
                self.emit(")");
            }
            TExprKind::Negate(inner) => {
                self.emit("(-");
                self.gen_expr_subst(inner, subst);
                self.emit(")");
            }
            TExprKind::App(_, _) => {
                // Collect the application chain, substituting as we go
                let mut args: Vec<&TExpr> = vec![];
                let mut f = expr;
                while let TExprKind::App(inner_f, inner_arg) = &f.kind {
                    args.push(inner_arg.as_ref());
                    f = inner_f.as_ref();
                }
                args.reverse();
                self.gen_expr_subst(f, subst);
                self.emit("(");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 { self.emit(", "); }
                    self.gen_expr_subst(a, subst);
                }
                self.emit(")");
            }
            _ => self.gen_expr(expr),
        }
    }

    /// Like is_cheap but also treats function applications with cheap
    /// sub-expressions as cheap. Safe for function arguments where the
    /// callee will force the value immediately — avoids thunk allocation
    /// for calls like fi(ch, fiVol) that compute simple values.
    fn is_cheap_arg(expr: &TExpr) -> bool {
        if Self::is_cheap(expr) { return true; }
        match &expr.kind {
            TExprKind::App(func, arg) => {
                Self::is_cheap_arg(func) && Self::is_cheap_arg(arg)
            }
            TExprKind::Paren(inner) => Self::is_cheap_arg(inner),
            _ => false,
        }
    }

    /// Check if an expression contains a function call where the function
    /// is NOT a known top-level/prelude name. Such calls could be to
    /// arbitrary function parameters and may be expensive.
    fn has_unknown_call(&self, expr: &TExpr) -> bool {
        match &expr.kind {
            TExprKind::App(func, arg) => {
                // Find the outermost function in this application chain
                let mut f = func.as_ref();
                while let TExprKind::App(inner_f, _) = &f.kind {
                    f = inner_f.as_ref();
                }
                // If the function is a variable not in top_level_names, it's unknown
                if let TExprKind::Var(name) = &f.kind {
                    if !Self::is_con_app(expr) && !self.top_level_names.contains(&sanitize_name(name)) {
                        return true;
                    }
                }
                // Recurse into sub-expressions
                self.has_unknown_call(func) || self.has_unknown_call(arg)
            }
            TExprKind::Paren(inner) => self.has_unknown_call(inner),
            _ => false,
        }
    }

    /// Check if an expression is a constructor application (Con applied to args)
    fn is_con_app(expr: &TExpr) -> bool {
        match &expr.kind {
            TExprKind::Con(_) => true,
            TExprKind::App(func, _) => Self::is_con_app(func),
            _ => false,
        }
    }

    /// Emit an ST/IO action in a flattened bind chain.
    /// Bare Var references to zero-arg IO/ST bindings are deferred functions
    /// in Lua and need () to execute. Everything else self-evaluates.
    /// Emit code that PERFORMS an IO/ST action (used inside bind chains).
    /// Inlines known action patterns to avoid closure allocation:
    /// - SpecCall __mll_io: → emit Lua call directly
    /// - SpecCall for ST primitives → emit operation directly
    /// - pure/return → emit the value
    /// Falls back to __force(expr)() for unknown actions.
    fn gen_action(&mut self, expr: &TExpr) {
        if !Self::is_nullary_action_type(&expr.ty) {
            self.gen_expr(expr);
            return;
        }
        match &expr.kind {
            TExprKind::Lit(_) | TExprKind::Con(_) | TExprKind::Tuple(_) => {
                self.gen_expr(expr);
            }
            // pure(x) / return(x): performing it just returns x
            TExprKind::App(func, arg) if matches!(&func.kind, TExprKind::Var(n) if n == "pure" || n == "return") => {
                self.gen_expr(arg);
            }
            // IO SpecCall: inline the Lua call directly (skip closure)
            TExprKind::SpecCall { specialized, args, .. } if specialized.starts_with("__mll_io:") => {
                let lua_func = &specialized["__mll_io:".len()..];
                if lua_func.starts_with(':') {
                    let method = &lua_func[1..];
                    self.emit("__force(");
                    self.gen_expr(&args[0]);
                    self.emit(&format!("):{}", method));
                    self.emit("(");
                    for (i, a) in args.iter().enumerate().skip(1) {
                        if i > 1 { self.emit(", "); }
                        self.emit("__force(");
                        self.gen_expr(a);
                        self.emit(")");
                    }
                    self.emit(")");
                } else {
                    self.emit(lua_func);
                    self.emit("(");
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 { self.emit(", "); }
                        self.emit("__force(");
                        self.gen_expr(a);
                        self.emit(")");
                    }
                    self.emit(")");
                }
            }
            // ST primitive calls: perform directly (no closure needed).
            // Safe since gen_action is only called for immediately-consumed
            // actions (>>= / >>), not let-bound actions (gen_expr → closure).
            TExprKind::App(_, _) if Self::is_st_primitive_call(expr) => {
                self.gen_expr(expr);
            }
            _ => {
                // General IO/ST action: use __mll_run which handles both
                // direct values and action closures (function or value).
                self.emit("__mll_run(");
                self.gen_expr(expr);
                self.emit(")");
            }
        }
    }

    fn is_st_primitive(name: &str) -> bool {
        matches!(name,
            "newSTArray" | "readSTArray" | "writeSTArray"
            | "modifySTArray" | "stArrayLength" | "newSTArrayFromList"
            | "stArrayToList"
        )
    }

    /// Check if an expression is a fully-applied call to an ST primitive.
    fn is_st_primitive_call(expr: &TExpr) -> bool {
        let mut f = expr;
        while let TExprKind::App(func, _) = &f.kind { f = func; }
        matches!(&f.kind, TExprKind::Var(name) if Self::is_st_primitive(name))
    }

    fn is_nullary_action_type(ty: &Ty) -> bool {
        matches!(ty, Ty::IO(_) | Ty::LuaIO(_, _))
            || matches!(ty, Ty::App(f, _) if matches!(f.as_ref(),
                Ty::App(c, _) if matches!(c.as_ref(), Ty::Con(n) if n == "ST")))
    }

    /// Check if a function type's return type is an IO/ST action.
    fn returns_action(ty: &Ty) -> bool {
        match ty {
            Ty::Arrow(_, ret) => Self::returns_action(ret),
            _ => Self::is_nullary_action_type(ty),
        }
    }

    /// Flatten a monadic bind chain (from do-notation) into sequential
    /// local statements.
    /// When `inside_action` is true, terminal IO expressions are performed
    /// (called with `()`) because we're inside a do-block action closure.
    /// When false (regular function body), IO actions are returned as-is.
    fn gen_bind_chain(&mut self, expr: &TExpr) {
        self.gen_bind_chain_inner(expr, false);
    }

    fn gen_bind_chain_io(&mut self, expr: &TExpr) {
        self.gen_bind_chain_inner(expr, true);
    }

    fn gen_bind_chain_inner(&mut self, expr: &TExpr, inside_action: bool) {
        match &expr.kind {
            TExprKind::InfixApp { op, lhs, rhs } if op == ">>=" => {
                if let TExprKind::Lambda { params, body } = &rhs.kind {
                    // x <- action  =>  local x = action  (or action() for zero-arg IO vars)
                    let param_name = sanitize_name(&params[0].0);
                    self.emit_indent();
                    self.emit(&format!("local {} = ", param_name));
                    self.gen_action(lhs);
                    self.emit("\n");
                    self.concrete_vars.insert(param_name);
                    self.gen_bind_chain_inner(body, true);
                    return;
                }
            }
            TExprKind::InfixApp { op, lhs, rhs } if op == ">>" => {
                // action >> rest  =>  action; rest
                // Unwrap Paren to avoid Lua syntax issues with (expr) as statement
                let lhs_unwrapped = if let TExprKind::Paren(inner) = &lhs.kind { inner.as_ref() } else { lhs.as_ref() };
                self.emit_indent();
                self.gen_action(lhs_unwrapped);
                self.emit("\n");
                self.gen_bind_chain_inner(rhs, true);
                return;
            }
            TExprKind::Let { binds, body } => {
                // let x = e in rest  =>  local x = e; rest
                for bind in binds {
                    if let TExprKind::If { cond, then_branch, else_branch } = &bind.body.kind {
                        // If-expression as statement: avoids IIFE closure allocation
                        // local x; if cond then x = a else x = b end
                        let bname = sanitize_name(&bind.name);
                        self.emit_indent();
                        self.emit(&format!("local {}\n", bname));
                        self.concrete_vars.insert(bname.clone());
                        self.emit_indent();
                        self.emit("if ");
                        self.gen_expr(cond);
                        self.emit(" then ");
                        self.emit(&format!("{} = ", bname));
                        self.gen_expr(then_branch);
                        self.emit(" else ");
                        self.emit(&format!("{} = ", bname));
                        self.gen_expr(else_branch);
                        self.emit(" end\n");
                    } else if Self::is_nullary_action_type(&bind.body.ty) {
                        let bname = sanitize_name(&bind.name);
                        self.emit_indent();
                        self.emit(&format!("local {} = function() return ", bname));
                        self.gen_action(&bind.body);
                        self.emit(" end\n");
                    } else {
                        let bname = sanitize_name(&bind.name);
                        self.emit_indent();
                        if Self::is_cheap(&bind.body) {
                            self.emit(&format!("local {} = ", bname));
                            self.gen_expr(&bind.body);
                            self.emit("\n");
                            self.concrete_vars.insert(bname);
                        } else {
                            self.emit(&format!("local {} = __thunk(function() return ", bname));
                            self.gen_expr(&bind.body);
                            self.emit(" end)\n");
                        }
                    }
                }
                self.gen_bind_chain_inner(body, inside_action);
                return;
            }
            _ => {}
        }
        // Terminal expression — emit as statement when possible
        match &expr.kind {
            TExprKind::If { cond, then_branch, else_branch } => {
                // Emit if/then/else as Lua statement instead of IIFE.
                // Recursively apply gen_bind_chain to each branch so
                // nested do-blocks in branches also flatten.
                self.emit_indent();
                self.emit("if ");
                self.gen_expr(cond);
                self.emit(" then\n");
                self.indent += 1;
                self.gen_bind_chain_inner(then_branch, inside_action);
                self.indent -= 1;
                self.emit_indent();
                self.emit("else\n");
                self.indent += 1;
                self.gen_bind_chain_inner(else_branch, inside_action);
                self.indent -= 1;
                self.emit_indent();
                self.emit("end\n");
            }
            _ => {
                self.emit_indent();
                self.emit("return ");
                if inside_action {
                    self.gen_action(expr);
                } else {
                    self.gen_expr(expr);
                }
                self.emit("\n");
            }
        }
    }

    /// Emit an expression in function-call position.
    /// Variables known to be concrete (already forced) are emitted bare.
    /// Unknown variables are forced — they may be let-bound thunks.
    fn gen_expr_raw(&mut self, expr: &TExpr) {
        if let TExprKind::Var(name) = &expr.kind {
            match name.as_str() {
                "otherwise" => self.emit("true"),
                _ => {
                    let sname = sanitize_name(name);
                    let lref = self.lua_ref(&sname);
                    if self.concrete_vars.contains(&sname) {
                        self.emit(&lref);
                    } else {
                        self.emit("__force(");
                        self.emit(&lref);
                        self.emit(")");
                    }
                }
            }
        } else {
            self.gen_expr(expr);
        }
    }

    fn gen_expr(&mut self, expr: &TExpr) {
        match &expr.kind {
            TExprKind::Var(name) => {
                match name.as_str() {
                    "otherwise" => self.emit("true"),
                    _ => {
                        let sname = sanitize_name(name);
                        let lref = self.lua_ref(&sname);
                        if self.concrete_vars.contains(&sname) {
                            self.emit(&lref);
                        } else {
                            self.emit("__force(");
                            self.emit(&lref);
                            self.emit(")");
                        }
                    }
                }
            }
            TExprKind::Con(name) => {
                match name.as_str() {
                    "[]" => self.emit("nil"),
                    _ => self.emit(name),
                }
            }
            TExprKind::Lit(lit) => self.gen_literal(lit),
            TExprKind::App(func, arg) => {
                // Record field accessor: inline as direct table indexing
                if let TExprKind::Var(name) = &func.kind {
                    if let Some(&idx) = self.record_accessors.get(&sanitize_name(name)) {
                        self.gen_expr(arg);
                        self.emit(&format!("[{}]", idx));
                        return;
                    }
                }

                // Check for cons application: (:) x xs => __mll_cons(x, xs)
                if let TExprKind::App(inner_f, inner_arg) = &func.kind {
                    if let TExprKind::Con(name) = &inner_f.kind {
                        if name == ":" {
                            // Try to collect a literal list and emit compactly
                            if let Some(elems) = Self::collect_list_literal(expr) {
                                self.emit("(function() local _l = nil; ");
                                for elem in elems.iter().rev() {
                                    self.emit("_l = __mll_cons(");
                                    self.gen_expr(elem);
                                    self.emit(", _l); ");
                                }
                                self.emit("return _l end)()");
                                return;
                            }
                            self.emit("__mll_cons(");
                            self.gen_expr(inner_arg);
                            self.emit(", ");
                            self.gen_expr(arg);
                            self.emit(")");
                            return;
                        }
                    }
                }

                // seq a b => force a, return b
                if let TExprKind::App(seq_f, seq_a) = &func.kind {
                    if let TExprKind::Var(name) = &seq_f.kind {
                        if name == "seq" {
                            self.emit("(function() __force(");
                            self.gen_expr(seq_a);
                            self.emit("); return ");
                            self.gen_expr(arg);
                            self.emit(" end)()");
                            return;
                        }
                    }
                }

                // return/pure are identity — emit the argument directly.
                // Thunk arguments that contain calls to unknown functions
                // (parameters, locally-bound variables) to preserve non-strict
                // semantics: `return (f x)` must not eagerly evaluate `f x`
                // when f could be an arbitrary expensive function.
                // Calls to known top-level/prelude functions are safe to
                // evaluate eagerly.
                // return/pure wrap their argument in an IO action closure.
                if let TExprKind::Var(name) = &func.kind {
                    if name == "return" || name == "pure" {
                        self.emit("(function() return ");
                        self.gen_expr(arg);
                        self.emit(" end)");
                        return;
                    }
                }

                // Collect all applied arguments
                let mut args = vec![arg.as_ref()];
                let mut f = func.as_ref();
                while let TExprKind::App(inner_f, inner_arg) = &f.kind {
                    args.push(inner_arg.as_ref());
                    f = inner_f.as_ref();
                }
                args.reverse();

                // Typeclass methods on primitive types → inline as Lua operators
                if args.len() == 2 {
                    if let TExprKind::Var(name) = &f.kind {
                        let lua_op = match name.as_str() {
                            "eq_Integer" | "eq_Number" | "eq_String" | "eq_Bool" | "eq_ByteString" => Some("=="),
                            "ord_lt__Integer" | "ord_lt__Number" | "ord_lt__String" => Some("<"),
                            "ord_gt__Integer" | "ord_gt__Number" | "ord_gt__String" => Some(">"),
                            "ord_le__Integer" | "ord_le__Number" | "ord_le__String" => Some("<="),
                            "ord_ge__Integer" | "ord_ge__Number" | "ord_ge__String" => Some(">="),
                            _ => None,
                        };
                        if let Some(op) = lua_op {
                            self.emit("(");
                            self.gen_expr(args[0]);
                            self.emit(&format!(" {} ", op));
                            self.gen_expr(args[1]);
                            self.emit(")");
                            return;
                        }
                    }
                }

                // Inline small pure functions at call site
                if let TExprKind::Var(name) = &f.kind {
                    if let Some((params, body)) = self.inline_fns.get(name).cloned() {
                        if args.len() == params.len() {
                            let mut subst = std::collections::HashMap::new();
                            for (param, arg) in params.iter().zip(args.iter()) {
                                subst.insert(param.clone(), *arg);
                            }
                            self.emit("(");
                            self.gen_expr_subst(&body, &subst);
                            self.emit(")");
                            return;
                        }
                    }
                }

                // Check if this is a partial application:
                // the result type is still a function type
                let remaining = count_arrows(&expr.ty);
                if remaining > 0 {
                    // Partial application — generate a closure
                    // Wrapped in () so it can be immediately called in Lua
                    let extra_params: Vec<String> = (0..remaining)
                        .map(|i| format!("_pa{}", i))
                        .collect();
                    self.emit(&format!("(function({})\n", extra_params.join(", ")));
                    self.indent += 1;
                    self.emit_indent();
                    self.emit("return ");
                    let needs_wrap = matches!(&f.kind, TExprKind::OpFunc(_) | TExprKind::Lambda { .. });
                    if needs_wrap { self.emit("("); }
                    self.gen_expr_raw(f);
                    if needs_wrap { self.emit(")"); }
                    self.emit("(");
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 { self.emit(", "); }
                        self.gen_expr(a);
                    }
                    for p in &extra_params {
                        self.emit(", ");
                        self.emit(p);
                    }
                    self.emit(")\n");
                    self.indent -= 1;
                    self.emit_indent();
                    self.emit("end)");
                } else {
                    // Full application
                    // Wrap function literals in parens so Lua allows calling them
                    let needs_wrap = matches!(&f.kind, TExprKind::OpFunc(_) | TExprKind::Lambda { .. });
                    if needs_wrap { self.emit("("); }
                    self.gen_expr_raw(f);
                    if needs_wrap { self.emit(")"); }
                    // Look up callee's demand info for strict positions.
                    let callee_strict = if let TExprKind::Var(name) = &f.kind {
                        self.demand_info.strict_params.get(name).cloned()
                    } else {
                        None
                    };
                    self.emit("(");
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 { self.emit(", "); }
                        let is_strict = callee_strict.as_ref()
                            .map_or(false, |v| v.get(i).copied().unwrap_or(false));
                        let pass_eager = Self::is_cheap_arg(a) || is_strict;
                        if pass_eager {
                            self.gen_expr(a);
                        } else {
                            self.emit("__thunk(function() return ");
                            self.gen_expr(a);
                            self.emit(" end)");
                        }
                    }
                    self.emit(")");
                }
            }
            TExprKind::InfixApp { op, lhs, rhs } => {
                if op == "div" {
                    self.emit("math.floor(");
                    self.gen_expr(lhs);
                    self.emit(" / ");
                    self.gen_expr(rhs);
                    self.emit(")");
                    return;
                }
                let lua_op = match op.as_str() {
                    "++" => "..", "&&" => "and", "||" => "or", "/=" => "~=",
                    "mod" => "%",
                    ":" => {
                        self.emit("__mll_cons(");
                        self.gen_expr(lhs); self.emit(", "); self.gen_expr(rhs);
                        self.emit(")");
                        return;
                    }
                    "$" => {
                        self.gen_expr(lhs); self.emit("(__thunk(function() return "); self.gen_expr(rhs); self.emit(" end))");
                        return;
                    }
                    ">>=" => {
                        // IO actions: do-blocks produce function() closures.
                        // Bind chain flattens into sequential statements inside
                        // the action closure; each sub-action is called with ().
                        if let TExprKind::Lambda { .. } = &rhs.kind {
                            self.emit("function()\n");
                            self.indent += 1;
                            self.gen_bind_chain_io(expr);
                            self.indent -= 1;
                            self.emit_indent(); self.emit("end");
                        } else {
                            // m >>= f (non-lambda): wrap as action
                            self.emit("function() return ("); self.gen_expr(rhs); self.emit(")(");
                            self.gen_action(lhs); self.emit(")() end");
                        }
                        return;
                    }
                    ">>" => {
                        // IO-then: produce action closure
                        self.emit("function()\n");
                        self.indent += 1;
                        self.gen_bind_chain_io(expr);
                        self.indent -= 1;
                        self.emit_indent(); self.emit("end");
                        return;
                    }
                    "." => {
                        self.emit("(function(_x) return "); self.gen_expr(lhs);
                        self.emit("("); self.gen_expr(rhs); self.emit("(_x)) end)");
                        return;
                    }
                    other => other,
                };
                if is_builtin_op(op) {
                    // Lua-native operator: emit as infix
                    self.emit("("); self.gen_expr(lhs);
                    self.emit(&format!(" {} ", lua_op));
                    self.gen_expr(rhs); self.emit(")");
                } else {
                    // User-defined or non-Lua operator: emit as function call
                    let sop = sanitize_name(op);
                    self.emit(&self.lua_ref(&sop)); self.emit("(");
                    self.gen_expr(lhs); self.emit(", "); self.gen_expr(rhs); self.emit(")");
                }
            }
            TExprKind::Negate(inner) => { self.emit("(-"); self.gen_expr(inner); self.emit(")"); }
            TExprKind::If { cond, then_branch, else_branch } => {
                self.emit("(function()\n"); self.indent += 1;
                self.emit_indent(); self.emit("if "); self.gen_expr(cond); self.emit(" then\n");
                self.indent += 1; self.emit_indent(); self.emit("return "); self.gen_expr(then_branch); self.emit("\n"); self.indent -= 1;
                self.emit_indent(); self.emit("else\n");
                self.indent += 1; self.emit_indent(); self.emit("return "); self.gen_expr(else_branch); self.emit("\n"); self.indent -= 1;
                self.emit_indent(); self.emit("end\n"); self.indent -= 1;
                self.emit_indent(); self.emit("end)()");
            }
            TExprKind::Case { scrutinee, branches } => {
                self.emit("(function()\n"); self.indent += 1;
                self.emit_indent(); self.emit("local _s = __force("); self.gen_expr(scrutinee); self.emit(")\n");
                for (i, branch) in branches.iter().enumerate() {
                    let mut conditions = Vec::new();
                    let mut bindings = Vec::new();
                    self.collect_pattern_conditions("_s", &branch.pattern, &mut conditions, &mut bindings);
                    if conditions.is_empty() {
                        if i > 0 { self.emit_indent(); self.emit("else\n"); self.indent += 1; }
                        for (var, val) in &bindings { self.emit_line(&format!("local {} = {}", var, val)); }
                        self.emit_indent(); self.emit("return "); self.gen_expr(&branch.body); self.emit("\n");
                        if i > 0 { self.indent -= 1; self.emit_line("end"); }
                        break;
                    }
                    let kw = if i == 0 { "if" } else { "elseif" };
                    self.emit_indent(); self.emit(&format!("{} {} then\n", kw, conditions.join(" and ")));
                    self.indent += 1;
                    for (var, val) in &bindings { self.emit_line(&format!("local {} = {}", var, val)); }
                    self.emit_indent(); self.emit("return "); self.gen_expr(&branch.body); self.emit("\n");
                    self.indent -= 1;
                    if i == branches.len() - 1 { self.emit_line("end"); }
                }
                self.indent -= 1; self.emit_indent(); self.emit("end)()");
            }
            TExprKind::Let { binds, body } => {
                self.emit("(function()\n"); self.indent += 1;
                for bind in binds {
                    self.emit_indent();
                    if Self::is_cheap(&bind.body) {
                        self.emit(&format!("local {} = ", bind.name));
                        self.gen_expr(&bind.body); self.emit("\n");
                        self.concrete_vars.insert(bind.name.clone());
                    } else {
                        self.emit(&format!("local {} = __thunk(function() return ", bind.name));
                        self.gen_expr(&bind.body); self.emit(" end)\n");
                    }
                }
                self.emit_indent(); self.emit("return "); self.gen_expr(body); self.emit("\n");
                self.indent -= 1; self.emit_indent(); self.emit("end)()");
            }
            TExprKind::Lambda { params, body } => {
                let ps: Vec<&str> = params.iter().map(|(s, _)| s.as_str()).collect();
                self.emit(&format!("function({})\n", ps.join(", ")));
                self.indent += 1; self.emit_indent(); self.emit("return ");
                self.gen_expr(body); self.emit("\n"); self.indent -= 1;
                self.emit_indent(); self.emit("end");
            }
            TExprKind::Paren(inner) => {
                self.emit("("); self.gen_expr(inner); self.emit(")");
            }
            TExprKind::OpFunc(op) => {
                let lua_op = match op.as_str() {
                    "++" => "..", "&&" => "and", "||" => "or", "/=" => "~=",
                    other => other,
                };
                self.emit(&format!("function(_a, _b) return __force(_a) {} __force(_b) end", lua_op));
            }
            TExprKind::SpecCall { specialized, args, .. } => {
                if let Some(rest) = specialized.strip_prefix("__mll_dict:") {
                    // Dictionary table literal: { method1 = impl1, method2 = impl2 }
                    let parts: Vec<&str> = rest.splitn(2, ':').collect();
                    let methods = if parts.len() > 1 { parts[1] } else { "" };
                    self.emit("{ ");
                    let mut first = true;
                    for entry in methods.split(',') {
                        if entry.is_empty() { continue; }
                        let kv: Vec<&str> = entry.splitn(2, '=').collect();
                        if kv.len() == 2 {
                            if !first { self.emit(", "); }
                            first = false;
                            let sv = sanitize_name(kv[1]);
                            self.emit(&format!("{} = {}", sanitize_name(kv[0]), self.lua_ref(&sv)));
                        }
                    }
                    self.emit(" }");
                } else if let Some(elem_eq) = specialized.strip_prefix("__mll_list_eq:") {
                    // List eq: recursive element-wise comparison
                    self.emit(&format!("__mll_list_eq({}, ", self.lua_ref(elem_eq)));
                    self.gen_expr(&args[0]);
                    self.emit(", ");
                    self.gen_expr(&args[1]);
                    self.emit(")");
                } else if let Some(elem_eq) = specialized.strip_prefix("__mll_maybe_eq:") {
                    // Maybe eq: Nothing==Nothing, Just a == Just b iff a==b
                    self.emit(&format!("__mll_maybe_eq({}, ", self.lua_ref(elem_eq)));
                    self.gen_expr(&args[0]);
                    self.emit(", ");
                    self.gen_expr(&args[1]);
                    self.emit(")");
                } else if let Some(rest) = specialized.strip_prefix("__mll_tuple_eq:") {
                    // Tuple eq: compare element-wise
                    // Format: __mll_tuple_eq:N:eq_E1,eq_E2,...
                    let parts: Vec<&str> = rest.splitn(2, ':').collect();
                    let n: usize = parts[0].parse().unwrap();
                    let eq_fns: Vec<&str> = parts[1].split(',').collect();
                    self.emit("(");
                    for i in 0..n {
                        if i > 0 { self.emit(" and "); }
                        self.emit(&self.lua_ref(eq_fns[i]));
                        self.emit("(__force(");
                        self.gen_expr(&args[0]);
                        self.emit(&format!(")[{}], __force(", i + 1));
                        self.gen_expr(&args[1]);
                        self.emit(&format!(")[{}])", i + 1));
                    }
                    self.emit(")");
                } else if let Some(elem_show) = specialized.strip_prefix("__mll_show_list:") {
                    // Specialized list show: iterate with element show function
                    self.emit(&format!("__mll_show_list({}, ", self.lua_ref(elem_show)));
                    self.gen_expr(&args[0]);
                    self.emit(")");
                } else if let Some(lua_name) = specialized.strip_prefix("__mll_const:") {
                    // Constant access: math.pi (no function call)
                    self.emit(lua_name);
                } else if let Some(idx) = specialized.strip_prefix("__mll_tup_get:") {
                    // Tuple field access: t[N]
                    self.emit("__force(");
                    self.gen_expr(&args[0]);
                    self.emit(&format!(")[{}]", idx));
                } else if let Some(rest) = specialized.strip_prefix("__mll_tup_ret:") {
                    // Multi-return FFI: pack Lua multiple returns into a tuple table
                    // Format: __mll_tup_ret:N:lua_func
                    let parts: Vec<&str> = rest.splitn(2, ':').collect();
                    let n: usize = parts[0].parse().unwrap();
                    let lua_func = parts[1];
                    let vars: Vec<String> = (0..n).map(|i| format!("_r{}", i)).collect();
                    self.emit("(function() local ");
                    self.emit(&vars.join(", "));
                    self.emit(" = ");
                    self.emit(lua_func);
                    self.emit("(");
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 { self.emit(", "); }
                        self.emit("__force(");
                        self.gen_expr(a);
                        self.emit(")");
                    }
                    self.emit("); return {");
                    self.emit(&vars.join(", "));
                    self.emit("} end)()");
                } else if let Some(lua_func) = specialized.strip_prefix("__mll_iter:") {
                    // Iterator FFI: __mll_iter(lua_factory, arg0, arg1, ...)
                    self.emit("__mll_iter(");
                    self.emit(lua_func);
                    for a in args {
                        self.emit(", __force(");
                        self.gen_expr(a);
                        self.emit(")");
                    }
                    self.emit(")");
                } else if let Some(lua_func) = specialized.strip_prefix("__mll_try:") {
                    // Try FFI: wrap result in Either via __mll_try
                    self.emit("__mll_try(");
                    if lua_func.starts_with(':') {
                        // Method call try: handle:method(args)
                        let method = &lua_func[1..];
                        self.emit("__force(");
                        self.gen_expr(&args[0]);
                        self.emit(&format!("):{}", method));
                        self.emit("(");
                        for (i, a) in args.iter().enumerate().skip(1) {
                            if i > 1 { self.emit(", "); }
                            self.emit("__force(");
                            self.gen_expr(a);
                            self.emit(")");
                        }
                        self.emit(")");
                    } else {
                        // Global function try
                        self.emit(lua_func);
                        self.emit("(");
                        for (i, a) in args.iter().enumerate() {
                            if i > 0 { self.emit(", "); }
                            self.emit("__force(");
                            self.gen_expr(a);
                            self.emit(")");
                        }
                        self.emit(")");
                    }
                    self.emit(")");
                } else if let Some(method) = specialized.strip_prefix(':') {
                    // Method call FFI: arg0:method(arg1, arg2, ...)
                    self.emit("__force(");
                    self.gen_expr(&args[0]);
                    self.emit(&format!("):{}", method));
                    self.emit("(");
                    for (i, a) in args.iter().enumerate().skip(1) {
                        if i > 1 { self.emit(", "); }
                        self.emit("__force(");
                        self.gen_expr(a);
                        self.emit(")");
                    }
                    self.emit(")");
                } else if let Some(lua_func) = specialized.strip_prefix("__mll_io:") {
                    // IO FFI: wrap in action thunk — only performed by >>= / >>
                    self.emit("function() return ");
                    if lua_func.starts_with(':') {
                        // Method call IO: handle:method(args)
                        let method = &lua_func[1..];
                        self.emit("__force(");
                        self.gen_expr(&args[0]);
                        self.emit(&format!("):{}", method));
                        self.emit("(");
                        for (i, a) in args.iter().enumerate().skip(1) {
                            if i > 1 { self.emit(", "); }
                            self.emit("__force(");
                            self.gen_expr(a);
                            self.emit(")");
                        }
                        self.emit(")");
                    } else {
                        self.emit(lua_func);
                        self.emit("(");
                        for (i, a) in args.iter().enumerate() {
                            if i > 0 { self.emit(", "); }
                            self.emit("__force(");
                            self.gen_expr(a);
                            self.emit(")");
                        }
                        self.emit(")");
                    }
                    self.emit(" end");
                } else if let Some(rest) = specialized.strip_prefix("__mll_io_tup:") {
                    // IO FFI with multi-return: wrap in action thunk
                    let parts: Vec<&str> = rest.splitn(2, ':').collect();
                    let n: usize = parts[0].parse().unwrap();
                    let lua_func = parts[1];
                    let vars: Vec<String> = (0..n).map(|i| format!("_r{}", i)).collect();
                    self.emit("function() local ");
                    self.emit(&vars.join(", "));
                    self.emit(" = ");
                    self.emit(lua_func);
                    self.emit("(");
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 { self.emit(", "); }
                        self.emit("__force(");
                        self.gen_expr(a);
                        self.emit(")");
                    }
                    self.emit("); return {");
                    self.emit(&vars.join(", "));
                    self.emit("} end");
                } else {
                    // Regular (pure) FFI: lua_func(arg0, arg1, ...)
                    self.emit(specialized);
                    self.emit("(");
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 { self.emit(", "); }
                        self.emit("__force(");
                        self.gen_expr(a);
                        self.emit(")");
                    }
                    self.emit(")");
                }
            }
            TExprKind::Tuple(elems) => {
                self.emit("{");
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 { self.emit(", "); }
                    self.gen_expr(e);
                }
                self.emit("}");
            }
            TExprKind::DictAccess { dict_param, method_name } => {
                self.emit(&format!("{}.{}", dict_param, sanitize_name(method_name)));
            }
            TExprKind::DictCall { func_name, dict_args, value_args } => {
                let sfn = sanitize_name(func_name);
                self.emit(&self.lua_ref(&sfn));
                self.emit("(");
                let mut first = true;
                for d in dict_args {
                    if !first { self.emit(", "); }
                    first = false;
                    self.gen_expr(d);
                }
                for v in value_args {
                    if !first { self.emit(", "); }
                    first = false;
                    self.gen_expr(v);
                }
                self.emit(")");
            }
        }
    }

    /// Generate an expression with lazy cons tails for self-referencing definitions.
    /// Cons operations wrap the tail in a thunk via __mll_lazy_cons.
    fn gen_expr_lazy(&mut self, expr: &TExpr, self_name: &str) {
        // Check for infix cons: x : rest
        if let TExprKind::InfixApp { op, lhs, rhs } = &expr.kind {
            if op == ":" {
                self.emit("__mll_lazy_cons(");
                self.gen_expr(lhs);
                self.emit(", function() return ");
                self.gen_expr_lazy(rhs, self_name);
                self.emit(" end)");
                return;
            }
        }
        // Check for App(App(Con(":"), head), tail)
        if let TExprKind::App(func, tail) = &expr.kind {
            if let TExprKind::App(con, head) = &func.kind {
                if let TExprKind::Con(name) = &con.kind {
                    if name == ":" {
                        self.emit("__mll_lazy_cons(");
                        self.gen_expr(head);
                        self.emit(", function() return ");
                        self.gen_expr_lazy(tail, self_name);
                        self.emit(" end)");
                        return;
                    }
                }
            }
        }
        // Not a cons — fall through to normal gen
        self.gen_expr(expr);
    }

    fn gen_literal(&mut self, lit: &TLiteral) {
        match lit {
            TLiteral::Integer(n) => self.emit(&format!("{}", n)),
            TLiteral::Number(n) => self.emit(&format!("{}", n)),
            TLiteral::Str(s) => {
                self.emit("\"");
                for c in s.chars() {
                    match c {
                        '\n' => self.emit("\\n"),
                        '\r' => self.emit("\\r"),
                        '\t' => self.emit("\\t"),
                        '\\' => self.emit("\\\\"),
                        '"' => self.emit("\\\""),
                        '\0' => self.emit("\\0"),
                        _ => self.emit(&c.to_string()),
                    }
                }
                self.emit("\"");
            }
            TLiteral::Bool(true) => self.emit("true"),
            TLiteral::Bool(false) => self.emit("false"),
            TLiteral::Unit => self.emit("nil"),
        }
    }
}

fn sanitize_name(name: &str) -> String {
    match name {
        "main" => "__run".to_string(),
        "return" => "return_".to_string(),
        "not" => "not_".to_string(),
        "print" => "print_".to_string(),
        "end" => "end_".to_string(),
        "then" => "then_".to_string(),
        "do" => "do_".to_string(),
        "in" => "in_".to_string(),
        "or" => "or_".to_string(),
        "and" => "and_".to_string(),
        "bsEmpty" => "__mll_bs_empty".to_string(),
        "bsLength" => "__mll_bs[1]".to_string(),
        "bsIndex" => "__mll_bs[2]".to_string(),
        "bsSub" => "__mll_bs[3]".to_string(),
        "bsSingleton" => "__mll_bs[4]".to_string(),
        "bsConcat" => "__mll_bs[5]".to_string(),
        "bsNull" => "__mll_bs[6]".to_string(),
        "bsHead" => "__mll_bs[7]".to_string(),
        "bsTail" => "__mll_bs[8]".to_string(),
        "bsCons" => "__mll_bs[9]".to_string(),
        "bsSnoc" => "__mll_bs[10]".to_string(),
        "bsReplicate" => "__mll_bs[11]".to_string(),
        "bsPack" => "__mll_bs[12]".to_string(),
        "bsUnpack" => "__mll_bs[13]".to_string(),
        "bsMap" => "__mll_bs[14]".to_string(),
        "bsFoldl" => "__mll_bs[15]".to_string(),
        "bsXor" => "__mll_bs[16]".to_string(),
        "bsZipWith" => "__mll_bs[17]".to_string(),
        "bsToString" => "__mll_bs[18]".to_string(),
        "bsFromString" => "__mll_bs[19]".to_string(),
        "bsGetU16LE" => "__mll_bs[20]".to_string(),
        "bsGetU32LE" => "__mll_bs[21]".to_string(),
        "bsGetI8" => "__mll_bs[22]".to_string(),
        "bsGetI16LE" => "__mll_bs[23]".to_string(),
        "bsPutI16LE" => "__mll_bs[24]".to_string(),
        "bsConcatList" => "__mll_bs[25]".to_string(),
        "runST" => "__mll_run".to_string(),
        "newSTArray" => "__mll_ma_new".to_string(),
        "readSTArray" => "__mll_ma_read".to_string(),
        "writeSTArray" => "__mll_ma_write".to_string(),
        "modifySTArray" => "__mll_ma_modify".to_string(),
        "stArrayLength" => "__mll_ma_length".to_string(),
        "newSTArrayFromList" => "__mll_ma_from_list".to_string(),
        "stArrayToList" => "__mll_ma_to_list".to_string(),
        "hmEmpty" => "hashmap_empty".to_string(),
        "hmInsert" => "hashmap_insert".to_string(),
        "hmLookup" => "hashmap_lookup".to_string(),
        "hmDelete" => "hashmap_delete".to_string(),
        "hmSize" => "hashmap_size".to_string(),
        "hmKeys" => "hashmap_keys".to_string(),
        "hmValues" => "hashmap_values".to_string(),
        "hmMember" => "hashmap_member".to_string(),
        _ => {
            let mut s = String::new();
            for c in name.chars() {
                match c {
                    '\'' => s.push_str("_prime"),
                    '<' => s.push_str("_lt_"),
                    '>' => s.push_str("_gt_"),
                    '+' => s.push_str("_plus_"),
                    '-' => s.push('_'),
                    '*' => s.push_str("_star_"),
                    '/' => s.push_str("_slash_"),
                    '!' => s.push_str("_bang_"),
                    '?' => s.push_str("_q_"),
                    '|' => s.push_str("_pipe_"),
                    '&' => s.push_str("_amp_"),
                    '=' => s.push_str("_eq_"),
                    '^' => s.push_str("_caret_"),
                    '~' => s.push_str("_tilde_"),
                    '@' => s.push_str("_at_"),
                    '$' => s.push_str("_dollar_"),
                    '[' => s.push_str("List_"),
                    ']' => {},
                    _ => s.push(c),
                }
            }
            s
        }
    }
}

/// Check if a TExpr references a given name anywhere
fn expr_references_name(expr: &TExpr, name: &str) -> bool {
    match &expr.kind {
        TExprKind::Var(n) => n == name,
        TExprKind::Con(_) | TExprKind::Lit(_) | TExprKind::OpFunc(_) => false,
        TExprKind::App(f, a) => expr_references_name(f, name) || expr_references_name(a, name),
        TExprKind::InfixApp { lhs, rhs, .. } => {
            expr_references_name(lhs, name) || expr_references_name(rhs, name)
        }
        TExprKind::Negate(e) | TExprKind::Paren(e) => expr_references_name(e, name),
        TExprKind::Lambda { body, .. } => expr_references_name(body, name),
        TExprKind::If { cond, then_branch, else_branch } => {
            expr_references_name(cond, name) ||
            expr_references_name(then_branch, name) ||
            expr_references_name(else_branch, name)
        }
        TExprKind::Case { scrutinee, branches } => {
            expr_references_name(scrutinee, name) ||
            branches.iter().any(|b| expr_references_name(&b.body, name))
        }
        TExprKind::Let { binds, body } => {
            binds.iter().any(|b| expr_references_name(&b.body, name)) ||
            expr_references_name(body, name)
        }
        TExprKind::SpecCall { args, .. } => args.iter().any(|a| expr_references_name(a, name)),
        TExprKind::Tuple(elems) => elems.iter().any(|e| expr_references_name(e, name)),
        TExprKind::DictAccess { .. } => false,
        TExprKind::DictCall { dict_args, value_args, .. } => {
            dict_args.iter().any(|a| expr_references_name(a, name)) ||
            value_args.iter().any(|a| expr_references_name(a, name))
        }
    }
}

/// Count how many arrows are at the top level of a type.
/// Arrow(a, Arrow(b, c)) = 2, Arrow(a, b) = 1, Con(_) = 0
fn count_arrows(ty: &Ty) -> usize {
    match ty {
        Ty::Arrow(_, rest) => 1 + count_arrows(rest),
        _ => 0,
    }
}

fn is_builtin_op(op: &str) -> bool {
    matches!(op, "+" | "-" | "*" | "/" | "%" | "^" | "==" | "/=" | "~="
        | "<" | ">" | "<=" | ">=" | "++" | "&&" | "||" | ".." | "$" | "."
        | "div" | "mod")
}

pub fn generate(module: &TModule) -> String {
    let mut cg = CodeGen::new();
    cg.demand_info = crate::demand::analyze(module);
    cg.output.push_str(PRELUDE);
    cg.output.push('\n');
    cg.generate_module(module);
    cg.output
}

const PRELUDE: &str = r#"-- MLL Runtime
local __unpack = table.unpack or unpack

-- Thunk infrastructure (non-strict evaluation)
local __thunk_mt = {}
local function __thunk(f) return setmetatable({f, false}, __thunk_mt) end
local function __force(x)
    if getmetatable(x) == __thunk_mt then
        if x[2] then return x[1] end
        local val = x[1]()
        x[1] = val
        x[2] = true
        return val
    end
    return x
end

-- List primitives (internal)
local function __mll_cons(h, t) return {h, t} end
local function __mll_lazy_cons(h, thunk) return {h, thunk, __lazy = true} end
local function __mll_head(l) l = __force(l); return l[1] end
local function __mll_tail(l)
    l = __force(l)
    if l.__lazy then
        l[2] = l[2]()
        l.__lazy = nil
    end
    return l[2]
end

-- Deep-force an MLL value for export to Lua.
-- Converts lazy cons lists to plain Lua arrays, forces thunks, recurses into tuples.
local function __mll_to_lua(x)
    x = __force(x)
    if type(x) ~= "table" then return x end
    -- Check if it's a cons list (2-element table, not tagged)
    if x[2] ~= nil and type(x[1]) ~= "string" then
        -- Could be a cons cell or a tuple; try to walk as a list
        local result = {}
        local cur = x
        local is_list = true
        while cur ~= nil do
            if type(cur) ~= "table" then is_list = false; break end
            result[#result + 1] = __mll_to_lua(__force(cur[1]))
            cur = __mll_tail(cur)
        end
        if is_list then return result end
    end
    -- Tuple or ADT: force each element
    local result = {}
    for i, v in ipairs(x) do result[i] = __mll_to_lua(v) end
    return result
end

-- Wrap a Lua callback so it deep-forces all arguments before forwarding.
-- Used at the FFI boundary: Lua functions don't understand MLL thunks.
local function __mll_wrap_callback(f)
    return function(...)
        local args = {n = select('#', ...), ...}
        for i = 1, args.n do args[i] = __mll_to_lua(args[i]) end
        return f(__unpack(args, 1, args.n))
    end
end

-- Run an IO action: force thunks, then call the action closure
local function __mll_run(action)
    action = __force(action)
    if type(action) == "function" then return action() else return action end
end
-- Perform an IO action (guaranteed to be a function closure)
local function __mll_perform(action)
    action = __force(action)
    return action()
end

-- Primitives that require Lua runtime dispatch
local function not_(x) return not __force(x) end
local function engage(f, ...)
    if select('#', ...) > 0 then return __force(f)(...) else return __force(f) end
end
local function liftIO(action) return action end
local function show(x)
    x = __force(x)
    if type(x) == "number" then return tostring(x)
    elseif type(x) == "string" then return x
    elseif type(x) == "boolean" then
        if x then return "True" else return "False" end
    elseif type(x) == "nil" then return "Nothing"
    elseif type(x) == "table" then
        if x[2] ~= nil or (x[1] ~= nil and type(x[2]) == "nil") then
            local parts = {}
            local cur = x
            local is_list = true
            while cur ~= nil do
                if type(cur) ~= "table" then is_list = false; break end
                parts[#parts + 1] = show(__force(cur[1]))
                cur = __mll_tail(cur)
            end
            if is_list then return "[" .. table.concat(parts, ", ") .. "]" end
        end
        local parts = {}
        for i, v in ipairs(x) do parts[i] = show(v) end
        if type(x[1]) == "string" then return x[1] .. "(" .. table.concat(parts, ", ", 2) .. ")"
        else return "(" .. table.concat(parts, ", ") .. ")" end
    else return tostring(x) end
end
local undefined = __thunk(function() error("Prelude.undefined", 0) end)
local function error_(msg) error(__force(msg)) end
local function max(a, b) return math.max(__force(a), __force(b)) end
local function min(a, b) return math.min(__force(a), __force(b)) end
local function pure(x) return function() return x end end
local function return_(x) return function() return x end end
local function Just(x) return x end
local Nothing = nil
local function show_Integer(x) return show(x) end
local function show_Number(x) return show(x) end
local function show_String(x) return show(x) end
local function show_Bool(x) return show(x) end
local function show_List_(x) return show(x) end
local function show_Maybe(x) return show(x) end
local function eq_Integer(a, b) a = __force(a); b = __force(b); return a == b end
local function eq_Number(a, b) a = __force(a); b = __force(b); return a == b end
local function eq_String(a, b) a = __force(a); b = __force(b); return a == b end
local function eq_Bool(a, b) a = __force(a); b = __force(b); return a == b end
local function ord_lt__Integer(a, b) a = __force(a); b = __force(b); return a < b end
local function ord_lt__Number(a, b) a = __force(a); b = __force(b); return a < b end
local function ord_lt__String(a, b) a = __force(a); b = __force(b); return a < b end
local function ord_gt__Integer(a, b) a = __force(a); b = __force(b); return a > b end
local function ord_gt__Number(a, b) a = __force(a); b = __force(b); return a > b end
local function ord_gt__String(a, b) a = __force(a); b = __force(b); return a > b end
local function ord_le__Integer(a, b) a = __force(a); b = __force(b); return a <= b end
local function ord_le__Number(a, b) a = __force(a); b = __force(b); return a <= b end
local function ord_le__String(a, b) a = __force(a); b = __force(b); return a <= b end
local function ord_ge__Integer(a, b) a = __force(a); b = __force(b); return a >= b end
local function ord_ge__Number(a, b) a = __force(a); b = __force(b); return a >= b end
local function ord_ge__String(a, b) a = __force(a); b = __force(b); return a >= b end
local function head(xs) return __mll_head(xs) end
local function tail(xs) return __mll_tail(xs) end
local function map(f, xs)
    f = __force(f); xs = __force(xs)
    if xs == nil then return nil end
    return __mll_lazy_cons(f(__mll_head(xs)), function()
        return map(f, __mll_tail(xs))
    end)
end
local function filter(pred, xs)
    pred = __force(pred); xs = __force(xs)
    if xs == nil then return nil end
    local h = __mll_head(xs)
    if pred(h) then
        return __mll_lazy_cons(h, function() return filter(pred, __mll_tail(xs)) end)
    else
        return filter(pred, __mll_tail(xs))
    end
end
local function take(n, xs)
    n = __force(n); xs = __force(xs)
    if n <= 0 or xs == nil then return nil end
    if xs.__lazy then
        return __mll_lazy_cons(__mll_head(xs), function() return take(n - 1, __mll_tail(xs)) end)
    else
        return __mll_cons(__mll_head(xs), take(n - 1, __mll_tail(xs)))
    end
end
local function zipWith(f, xs, ys)
    f = __force(f); xs = __force(xs); ys = __force(ys)
    if xs == nil or ys == nil then return nil end
    return __mll_lazy_cons(f(__mll_head(xs), __mll_head(ys)), function()
        return zipWith(f, __mll_tail(xs), __mll_tail(ys))
    end)
end
-- Hash helper
local function __mll_hashstr(s) s = __force(s); local h = 5381 for i = 1, #s do h = ((h * 33) + string.byte(s, i)) % 2147483647 end return h end

-- HashMap runtime (backed by Lua tables)
local hashmap_empty = {}
local function hashmap_insert(k, v, m) k = __force(k); v = __force(v); m = __force(m); local t = {} for a,b in pairs(m) do t[a] = b end t[k] = v return t end
local function hashmap_lookup(k, m) k = __force(k); m = __force(m); local v = m[k] if v == nil then return nil else return v end end
local function hashmap_delete(k, m) k = __force(k); m = __force(m); local t = {} for a,b in pairs(m) do t[a] = b end t[k] = nil return t end
local function hashmap_size(m) m = __force(m); local n = 0 for _ in pairs(m) do n = n + 1 end return n end
local function hashmap_keys(m) m = __force(m); local r = nil local ks = {} for k in pairs(m) do ks[#ks+1] = k end table.sort(ks) for i = #ks, 1, -1 do r = __mll_cons(ks[i], r) end return r end
local function hashmap_values(m) m = __force(m); local r = nil local ks = {} for k in pairs(m) do ks[#ks+1] = k end table.sort(ks) for i = #ks, 1, -1 do r = __mll_cons(m[ks[i]], r) end return r end
local function hashmap_member(k, m) k = __force(k); m = __force(m); return m[k] ~= nil end
local function show_HashMap(m) m = __force(m); local parts = {} for k, v in pairs(m) do parts[#parts+1] = show(k) .. " -> " .. show(v) end table.sort(parts) return "{" .. table.concat(parts, ", ") .. "}" end
local function hashmap_fromList(xs) xs = __force(xs); local t = {} local cur = xs while cur ~= nil do local pair = __mll_head(cur) t[__force(pair[1])] = __force(pair[2]) cur = __mll_tail(cur) end return t end

-- Specialized list show: uses a typed element show function
local function __mll_list_eq(elem_eq, a, b)
    a = __force(a); b = __force(b)
    while true do
        if a == nil and b == nil then return true end
        if a == nil or b == nil then return false end
        if not elem_eq(__force(a[1]), __force(b[1])) then return false end
        a = __mll_tail(a); b = __mll_tail(b)
    end
end
local function __mll_maybe_eq(elem_eq, a, b)
    a = __force(a); b = __force(b)
    if a == nil and b == nil then return true end
    if a == nil or b == nil then return false end
    return elem_eq(a, b)
end
local function __mll_show_list(elem_show, xs)
    xs = __force(xs)
    if xs == nil then return "[]" end
    local parts = {}
    local cur = xs
    while cur ~= nil do
        parts[#parts + 1] = elem_show(__force(__mll_head(cur)))
        cur = __mll_tail(cur)
    end
    return "[" .. table.concat(parts, ", ") .. "]"
end

-- Lua error convention wrapper: converts (val, err) to Either String a
-- Success: Right val, Failure: Left errmsg
local function __mll_try(val, err)
    if val == nil then return {1, err or "unknown error"} else return {2, val} end
end

-- Iterator-to-lazy-list: calls a Lua iterator factory and builds a lazy MLL list.
-- Single-value iterators produce a flat list; multi-value iterators pack into tuples.
local function __mll_iter(factory, ...)
    local iter = factory(...)
    local function go()
        local vals = {iter()}
        if vals[1] == nil then return nil end
        local val = #vals == 1 and vals[1] or vals
        return __mll_lazy_cons(val, go)
    end
    return go()
end

local getArgs = function()
    local result = nil
    if arg then
        for i = #arg, 1, -1 do result = __mll_cons(arg[i], result) end
    end
    return result
end
local function exit_(code)
    return function()
        if code == 1 then os.exit(0) else os.exit(code[2]) end
    end
end

-- Bitwise operations (Lua 5.3+ native, LuaJIT bit.*, or bit32)
local __mll_bxor, __mll_band, __mll_bor, __mll_bnot, __mll_shl, __mll_shr
if (loadstring or load)('return 0 ~ 0') then
    __mll_bxor = (loadstring or load)('local F=... return function(a,b) return F(a) ~ F(b) end')(__force)
    __mll_band = (loadstring or load)('local F=... return function(a,b) return F(a) & F(b) end')(__force)
    __mll_bor  = (loadstring or load)('local F=... return function(a,b) return F(a) | F(b) end')(__force)
    __mll_bnot = (loadstring or load)('local F=... return function(a) return ~F(a) end')(__force)
    __mll_shl  = (loadstring or load)('local F=... return function(a,b) return F(a) << F(b) end')(__force)
    __mll_shr  = (loadstring or load)('local F=... return function(a,b) return F(a) >> F(b) end')(__force)
else
    local __ok, __mll_bit = pcall(function() return (type(jit) == 'table' and require('bit')) or bit32 or require('bit') end)
    if not __ok then __mll_bit = nil end
    if __mll_bit then
    function __mll_bxor(a, b) return __mll_bit.bxor(__force(a), __force(b)) end
    function __mll_band(a, b) return __mll_bit.band(__force(a), __force(b)) end
    function __mll_bor(a, b) return __mll_bit.bor(__force(a), __force(b)) end
    function __mll_bnot(a) return __mll_bit.bnot(__force(a)) end
    function __mll_shl(a, b) return __mll_bit.lshift(__force(a), __force(b)) end
    function __mll_shr(a, b) return __mll_bit.rshift(__force(a), __force(b)) end
    end
end

-- Array primitives (O(1) indexed access, built from MLL lists)
local function __mll_array_from_list(xs)
    xs = __force(xs)
    local arr = {}
    local cur = xs
    while cur ~= nil do
        arr[#arr + 1] = __force(__mll_head(cur))
        cur = __mll_tail(cur)
    end
    return arr
end
local function __mll_array_index(arr, i) return __force(arr)[__force(i) + 1] end
local function __mll_array_length(arr) return #__force(arr) end

-- ByteString runtime (backed by Lua strings)
-- All indices are 0-based in MLL, converted to 1-based for Lua internally.
local __mll_bs_empty = ""
local __mll_bs; do
    local F = __force
    local sb, sc, sr, ss = string.byte, string.char, string.rep, string.sub
    __mll_bs = {
        function(s) return #F(s) end,                                           -- [1] length
        function(s, i) return sb(F(s), F(i) + 1) end,                          -- [2] index
        function(s, i, len) s=F(s); i=F(i); len=F(len); return ss(s, i+1, i+len) end, -- [3] sub
        function(b) return sc(F(b)) end,                                        -- [4] singleton
        function(a, b) return F(a) .. F(b) end,                                -- [5] concat
        function(s) return #F(s) == 0 end,                                      -- [6] null
        function(s) return sb(F(s), 1) end,                                     -- [7] head
        function(s) return ss(F(s), 2) end,                                     -- [8] tail
        function(b, s) return sc(F(b)) .. F(s) end,                             -- [9] cons
        function(s, b) return F(s) .. sc(F(b)) end,                             -- [10] snoc
        function(n, b) return sr(sc(F(b)), F(n)) end,                           -- [11] replicate
        function(xs)                                                             -- [12] pack
            xs = F(xs); local t = {}; local cur = xs
            while cur ~= nil do t[#t+1] = sc(F(__mll_head(cur))); cur = __mll_tail(cur) end
            return table.concat(t)
        end,
        function(s)                                                              -- [13] unpack
            s = F(s); local r = nil
            for i = #s, 1, -1 do r = __mll_cons(sb(s, i), r) end
            return r
        end,
        function(f, s)                                                           -- [14] map
            f=F(f); s=F(s); local t = {}
            for i = 1, #s do t[i] = sc(F(f)(sb(s, i))) end
            return table.concat(t)
        end,
        function(f, acc, s)                                                      -- [15] foldl
            f=F(f); acc=F(acc); s=F(s)
            for i = 1, #s do local b=sb(s,i); local r=F(f)(acc,b); if r==nil then r=F(F(f)(acc))(b) end; acc=F(r) end
            return acc
        end,
        function(a, b)                                                           -- [16] xor
            a=F(a); b=F(b); local t = {}
            for i = 1, #a do t[i] = sc(__mll_bxor(sb(a, i), sb(b, i))) end
            return table.concat(t)
        end,
        function(f, a, b)                                                        -- [17] zipwith
            f=F(f); a=F(a); b=F(b); local len=math.min(#a, #b); local t = {}
            for i = 1, len do local ba,bb=sb(a,i),sb(b,i); local r=F(f)(ba,bb); if r==nil then r=F(F(f)(ba))(bb) end; t[i]=sc(F(r)) end
            return table.concat(t)
        end,
        function(s) return F(s) end,                                             -- [18] tostring
        function(s) return F(s) end,                                             -- [19] fromstring
        function(s, i)                                                           -- [20] getU16LE
            s=F(s); i=F(i)+1; local lo,hi=sb(s,i),sb(s,i+1); return lo+hi*256
        end,
        function(s, i)                                                           -- [21] getU32LE
            s=F(s); i=F(i)+1; local a,b,c,d=sb(s,i),sb(s,i+1),sb(s,i+2),sb(s,i+3); return a+b*256+c*65536+d*16777216
        end,
        function(s, i)                                                           -- [22] getI8 (signed)
            s=F(s); local v=sb(s,F(i)+1); if v>=128 then return v-256 else return v end
        end,
        function(s, i)                                                           -- [23] getI16LE (signed)
            s=F(s); i=F(i)+1; local v=sb(s,i)+sb(s,i+1)*256; if v>=32768 then return v-65536 else return v end
        end,
        function(v)                                                              -- [24] putI16LE (signed int to 2-byte BS)
            v=F(v); if v<0 then v=v+65536 end; return sc(v%256, math.floor(v/256)%256)
        end,
        function(xs)                                                             -- [25] concatList
            xs = F(xs); local t = {}; local cur = xs
            while cur ~= nil do t[#t+1] = F(__mll_head(cur)); cur = __mll_tail(cur) end
            return table.concat(t)
        end,
    }
end
local function show_ByteString(s) s = __force(s); local t = {} for i = 1, #s do t[i] = string.format("%02x", string.byte(s, i)) end return "ByteString " .. table.concat(t) end
local function eq_ByteString(a, b) return __force(a) == __force(b) end

-- MutArray runtime (mutable integer arrays, backed by Lua tables)
-- Operations are effectful and run inside LuaIO s.
-- 0-based indexing externally, 1-based internally.
-- ST array primitives: these run inside runST which provides scoping,
-- so they perform directly (no action closure wrapping needed).
local function __mll_ma_new(size, init)
    size = __force(size); init = __force(init)
    local t = {}; for i = 1, size do t[i] = init end; return t
end
local function __mll_ma_read(arr, idx) return __force(arr)[__force(idx) + 1] end
local function __mll_ma_write(arr, idx, val) __force(arr)[__force(idx) + 1] = __force(val) end
local function __mll_ma_modify(arr, idx, f)
    arr = __force(arr); idx = __force(idx) + 1; f = __force(f)
    arr[idx] = __force(f)(arr[idx])
end
local function __mll_ma_length(arr) return #__force(arr) end
local function __mll_ma_from_list(xs)
    xs = __force(xs); local t = {}; local cur = xs
    while cur ~= nil do t[#t+1] = __force(__mll_head(cur)); cur = __mll_tail(cur) end
    return t
end
local function __mll_ma_to_list(arr)
    arr = __force(arr); local r = nil
    for i = #arr, 1, -1 do r = __mll_cons(arr[i], r) end
    return r
end
"#;
