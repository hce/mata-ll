use crate::demand::DemandInfo;
use crate::embed::{self, EmbedMode};
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
    /// LuaDict constructors: constructor name -> its field names in declaration
    /// order. A value of such a constructor is a Lua table keyed by these names
    /// rather than a positional array (used by pattern matching to bind fields).
    luadict_con_fields: std::collections::HashMap<String, Vec<String>>,
    /// LuaDict field accessors: sanitized accessor name -> the raw field name
    /// used as the Lua table key. Presence here means "index by key, not index".
    luadict_field_key: std::collections::HashMap<String, String>,
    /// Function table: maps sanitized function names to __mll_fn[N] slots.
    /// Used to pack all forward-declared functions into a single table,
    /// avoiding Lua's 200-local-variable limit.
    fn_table: std::collections::HashMap<String, usize>,
    /// Locally-bound variable names (params, let-binds, pattern-binds).
    /// These shadow fn_table entries — lua_ref returns the bare name.
    local_vars: std::collections::HashSet<String>,
    /// Count of `local` declarations in the current function scope.
    /// Used to detect when we're approaching Lua's 200-local limit.
    local_count: usize,
    /// When local_count exceeds the threshold, new locals go into a `_v` table.
    /// Maps sanitized local name to 1-based index in the `_v` table.
    var_slots: std::collections::HashMap<String, usize>,
    /// Next available `_v` index (1-based).
    var_slots_next: usize,
    /// Whether `local _v = {}` has been emitted in the current scope.
    var_table_emitted: bool,
    /// Demand analysis: per-function parameter strictness.
    demand_info: DemandInfo,
    /// Source embedding in `EmbedMode::Var`: the emitted file starts with a
    /// `local __SOURCE_CODE = …` binding (see embed.rs), and the module's
    /// return table must export it — even when there are no other exports.
    embed_var_export: bool,
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
            luadict_con_fields: std::collections::HashMap::new(),
            luadict_field_key: std::collections::HashMap::new(),
            fn_table: std::collections::HashMap::new(),
            local_vars: std::collections::HashSet::new(),
            local_count: 0,
            var_slots: std::collections::HashMap::new(),
            var_slots_next: 0,
            var_table_emitted: false,
            demand_info: DemandInfo { strict_params: std::collections::HashMap::new() },
            embed_var_export: false,
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
        if self.local_vars.contains(lua_name) {
            // Check if this local is in the _v table (overflow fallback)
            if let Some(&idx) = self.var_slots.get(lua_name) {
                format!("_v[{}]", idx)
            } else {
                lua_name.to_string()
            }
        } else if let Some(&slot) = self.fn_table.get(lua_name) {
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

    /// Lua's per-function local variable limit.
    const LOCAL_LIMIT: usize = 180;

    /// Declare a local variable, returning the Lua lvalue to assign to.
    /// When the local count is under the limit, returns `"local name"`.
    /// When over the limit, allocates a `_v[N]` slot and returns `"_v[N]"`.
    /// Also registers the name in `local_vars` (and `var_slots` if tabled).
    fn declare_local(&mut self, name: &str) -> String {
        self.local_vars.insert(name.to_string());
        self.local_count += 1;
        if self.local_count > Self::LOCAL_LIMIT {
            if !self.var_table_emitted {
                // Emit the _v table declaration (this itself is one local)
                self.emit_line("local _v = {}");
                self.var_table_emitted = true;
            }
            self.var_slots_next += 1;
            self.var_slots.insert(name.to_string(), self.var_slots_next);
            format!("_v[{}]", self.var_slots_next)
        } else {
            format!("local {}", name)
        }
    }

    /// Declare a local without a name (forward declaration for later assignment).
    /// Only used when the variable needs to exist before its value is known
    /// (e.g., `local x; if ... then x = a else x = b end`).
    fn declare_local_fwd(&mut self, name: &str) {
        self.local_vars.insert(name.to_string());
        self.local_count += 1;
        if self.local_count > Self::LOCAL_LIMIT {
            if !self.var_table_emitted {
                self.emit_line("local _v = {}");
                self.var_table_emitted = true;
            }
            self.var_slots_next += 1;
            self.var_slots.insert(name.to_string(), self.var_slots_next);
            // _v[N] slot exists implicitly (nil), no declaration needed
        } else {
            self.emit_line(&format!("local {}", name));
        }
    }

    /// Get the Lua lvalue for an already-declared local (for assignment after fwd decl).
    fn local_lvalue(&self, name: &str) -> String {
        if let Some(&idx) = self.var_slots.get(name) {
            format!("_v[{}]", idx)
        } else {
            name.to_string()
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
        sub.luadict_con_fields = self.luadict_con_fields.clone();
        sub.luadict_field_key = self.luadict_field_key.clone();
        sub.top_level_names = self.top_level_names.clone();
        sub.local_vars = self.local_vars.clone();
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
        // LuaDict types (validated by the typechecker to be single-constructor
        // records) lay their constructor out as a name-keyed table. Record the
        // ordered field names for pattern matching, and each field's key for the
        // accessor / record-update sites.
        if def.is_luadict {
            if let Some(con) = def.constructors.first()
                && let TConFields::Named(fields) = &con.fields {
                    let names: Vec<String> = fields.iter().map(|(n, _)| n.clone()).collect();
                    for name in &names {
                        self.luadict_field_key.insert(sanitize_name(name), name.clone());
                    }
                    self.luadict_con_fields.insert(con.name.clone(), names);
                }
        }
    }

    fn generate_module(&mut self, module: &TModule) {
        self.emit_line("-- Generated by MATA-LL compiler (https://matall.org/)");
        self.emit_line("");

        // All Prelude runtime names are plain local functions — never thunks.
        // Seed concrete_vars so references skip __force throughout user code.
        for name in &[
            "__force", "__thunk", "__mll_cons", "__mll_lazy_cons", "__mll_head",
            "__mll_tail", "__mll_to_lua", "__lua_to_mll", "__mll_wrap_callback", "__mll_run", "__mll_perform",
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
            "ord_compare__Integer", "ord_compare__Number", "ord_compare__String",
            "ord_lt__ByteString", "ord_gt__ByteString", "ord_le__ByteString",
            "ord_ge__ByteString", "ord_compare__ByteString",
            "head", "tail", "map", "filter", "take", "drop", "zipWith",
            "__mll_hashstr", "hashmap_empty", "hashmap_insert", "hashmap_lookup",
            "hashmap_delete", "hashmap_size", "hashmap_keys", "hashmap_values",
            "hashmap_member", "hashmap_fromList", "hashmap_toList",
            "__mll_list_append", "__mll_list_index", "semigroup_String", "semigroup_List",
            "__mll_show_list", "__mll_show_arg", "__mll_show_maybe", "__mll_list_eq", "__mll_maybe_eq", "__mll_eq",
            "__mll_try", "__mll_iter", "getArgs", "exit_",
            "try_", "catch_",
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
            "hmKeys", "hmValues", "hmMember", "hmFromList", "hmToList",
            "return", "pure", "not", "print", "error", "show", "undefined",
            "try", "catch",
        ] {
            self.top_level_names.insert(sanitize_name(name));
        }

        for def in &module.data_defs {
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
            self.emit_line("-- Entry point (skip when loaded via require)");
            self.emit_line("local __mll_modname = ...");
            let run_ref = self.lua_ref("__run");
            self.emit_line(&format!("if __mll_modname == nil then __mll_run({}()) end", run_ref));
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
            if self.embed_var_export {
                // The embedded original source (a plain Lua string bound at
                // the very top of the file — see embed.rs).
                self.emit_indent();
                self.emit(&format!("{0} = {0},\n", embed::SOURCE_VAR));
            }
            for name in &module.exports {
                let sname = sanitize_name(name);
                // Extract argument types from function type
                let arg_tys = if let Some(ty) = export_types.get(name) {
                    let mut args = Vec::new();
                    let mut t = ty;
                    while let Ty::Arrow(a, b) = t {
                        args.push(a.as_ref().clone());
                        t = b.as_ref();
                    }
                    args
                } else {
                    Vec::new()
                };

                let n_args = arg_tys.len();
                let params: Vec<String> = (0..n_args).map(|i| format!("a{}", i + 1)).collect();
                let params_str = if n_args > 0 { params.join(", ") } else { "...".to_string() };

                self.emit_indent();
                self.emit(&format!("{} = function({params_str})\n", sanitize_name(name)));
                self.indent += 1;

                // Type-directed argument conversion
                for (i, ty) in arg_tys.iter().enumerate() {
                    let arg = &params[i];
                    if matches!(ty, Ty::List(_)) {
                        self.emit_indent();
                        self.emit(&format!("{arg} = __lua_to_mll({arg})\n"));
                    } else if matches!(ty, Ty::Arrow(_, _)) {
                        self.emit_indent();
                        self.emit(&format!("if type({arg}) == \"function\" then {arg} = __mll_wrap_callback({arg}) end\n"));
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

            let decl = self.var_decl(&sanitize_name(&con.name));

            if field_count == 0 {
                if is_enum {
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

    fn gen_function(&mut self, func: &TFunction) {
        let lua_name = sanitize_name(&func.name);
        let clauses = &func.clauses;
        let saved_concrete = self.concrete_vars.clone();
        let saved_locals = self.local_vars.clone();
        let saved_local_count = self.local_count;
        let saved_var_slots = self.var_slots.clone();
        let saved_var_slots_next = self.var_slots_next;
        let saved_var_table_emitted = self.var_table_emitted;
        self.local_count = 0;
        self.var_slots.clear();
        self.var_slots_next = 0;
        self.var_table_emitted = false;

        if clauses.is_empty() { self.concrete_vars = saved_concrete; self.local_vars = saved_locals; self.local_count = saved_local_count; self.var_slots = saved_var_slots; self.var_slots_next = saved_var_slots_next; self.var_table_emitted = saved_var_table_emitted; return; }

        // Eta-expand: if the function has fewer patterns than type arrows,
        // add extra params so the Lua function matches the expected arity.
        // This handles point-free definitions like: f x = g x  (written as f = g)
        let type_arity = count_arrows(&func.ty);
        let pat_arity = if clauses[0].patterns.is_empty() { 0 } else { clauses[0].patterns.len() };
        let eta_count = type_arity.saturating_sub(pat_arity);

        if clauses.len() == 1 && clauses[0].patterns.is_empty() && clauses[0].guards.is_empty()
            && eta_count == 0 {
            // A genuine value binding: no parameters and the type has no
            // outstanding arrows to eta-expand. A point-free *function* alias
            // (`f = g`, where the type still has arrows so eta_count > 0) is
            // NOT handled here — it falls through to the function branch, which
            // eta-expands it into a real callable that looks the referent up at
            // call time. Emitting it as a value instead would either capture a
            // not-yet-assigned slot (forward reference -> nil) or leave a thunk
            // where callers expect a directly-callable function.
            // Check if this is a value binding (non-function type) or a
            // zero-arg function (IO action / thunk)
            let is_io_action = matches!(&func.ty, Ty::IO(_) | Ty::LuaIO(_, _) | Ty::Forall(_, _));

            let is_concrete;
            if is_io_action {
                // Wrap in a function (IO action, needs to be called)
                // Use gen_bind_chain_io to flatten do-block let/bind chains
                // into sequential local statements instead of nested IIFEs.
                self.emit_indent();
                self.emit(&self.fn_decl(&lua_name, ""));
                self.emit("\n");
                self.indent += 1;
                self.gen_where_binds(&clauses[0].where_binds);
                self.gen_bind_chain_io(&clauses[0].body);
                self.indent -= 1;
                self.emit_line("end");
                is_concrete = true;
            } else if expr_references_name(&clauses[0].body, &func.name) {
                // Self-referencing value binding (e.g., infinite list).
                // Use the bare name (not fn_table slot) so self-references
                // resolve to this local binding, not a potentially missing slot.
                self.local_vars.insert(lua_name.clone());
                if !self.forward_declared.contains(&lua_name) {
                    self.emit_line(&format!("local {}", lua_name));
                }
                self.emit_indent();
                self.emit(&format!("{} = ", lua_name));
                self.gen_expr_lazy(&clauses[0].body, &func.name);
                self.emit("\n");
                is_concrete = true;
            } else if clauses[0].where_binds.is_empty() && Self::is_cheap(&clauses[0].body)
                && !expr_evaluates_global_ref(&clauses[0].body) {
                // Cheap value binding that does not eagerly dereference another
                // top-level binding — safe to evaluate eagerly at module load.
                // A binding like `y = x` or `useX = x + 1` that reads a global
                // (possibly defined later in the file) falls through to the
                // thunk branch below, deferring the read past module load when
                // the slot is still nil.
                self.emit_indent();
                self.emit(&self.var_decl(&lua_name));
                self.gen_expr(&clauses[0].body);
                self.emit("\n");
                is_concrete = true;
            } else if clauses[0].where_binds.is_empty() {
                // Expensive value binding with no where clause — thunk
                self.emit_indent();
                self.emit(&self.var_decl(&lua_name));
                self.emit("__thunk(function() return ");
                self.gen_expr(&clauses[0].body);
                self.emit(" end)");
                self.emit("\n");
                is_concrete = false;
            } else {
                // Value binding with where clause — wrap in thunked IIFE to scope the locals
                self.emit_indent();
                self.emit(&self.var_decl(&lua_name));
                self.emit("__thunk(function()\n");
                self.indent += 1;
                self.gen_where_binds(&clauses[0].where_binds);
                self.emit_indent();
                self.emit("return ");
                self.gen_expr(&clauses[0].body);
                self.emit("\n");
                self.indent -= 1;
                self.emit_indent();
                self.emit("end)\n");
                is_concrete = false;
            }
            self.emit_line("");
            self.concrete_vars = saved_concrete;
            if is_concrete {
                self.concrete_vars.insert(lua_name);
            } else {
                // Thunked value — must NOT be concrete (needs __force)
                self.concrete_vars.remove(&lua_name);
            }
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
                // - If all callers pass cheap args, skip __force (already concrete).
                // - If demand analysis says param is strict, force at entry.
                // - Otherwise, stay lazy (param might never be used).
                let call_site_cheap = self.params_always_cheap.get(&func.name).cloned();
                let demand_strict = self.demand_info.strict_params.get(&func.name).cloned();
                for (i, pat) in clause.patterns.iter().enumerate() {
                    if let TPattern::Var(v, _) = pat {
                        let sname = sanitize_name(v);
                        let always_cheap = call_site_cheap.as_ref().is_some_and(|v| v.get(i).copied().unwrap_or(false));
                        let is_strict = demand_strict.as_ref().is_some_and(|v| v.get(i).copied().unwrap_or(false));
                        let decl = self.declare_local(&sname);
                        if always_cheap {
                            // All callers pass concrete values — no __force needed
                            self.emit_line(&format!("{} = _arg{}", decl, i));
                            self.concrete_vars.insert(sname);
                        } else if is_strict {
                            // Demand analysis: body forces this param — force at entry
                            self.emit_line(&format!("{} = __force(_arg{})", decl, i));
                            self.concrete_vars.insert(sname);
                        } else {
                            // Not demanded — stay lazy
                            self.emit_line(&format!("{} = _arg{}", decl, i));
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
                } else if Self::returns_st(&func.ty) {
                    // ST-returning function: wrap body in a closure so the
                    // function returns an ST action (deferred computation).
                    // The closure is called by __mll_run in bind chains.
                    self.emit_indent();
                    self.emit("return function()\n");
                    self.indent += 1;
                    self.gen_bind_chain_io(&clause.body);
                    self.indent -= 1;
                    self.emit_indent();
                    self.emit("end\n");
                } else if Self::returns_action(&func.ty) {
                    // IO-returning function: flatten bind chains, performing
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
                    if i < clause.patterns.len()
                        && !matches!(&clause.patterns[i], TPattern::Var(_, _) | TPattern::Wildcard)
                    {
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
            self.local_vars = saved_locals;
            self.local_count = saved_local_count;
            self.var_slots = saved_var_slots;
            self.var_slots_next = saved_var_slots_next;
            self.var_table_emitted = saved_var_table_emitted;
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
            let always_cheap = call_site_cheap.as_ref().is_some_and(|v| v.get(i).copied().unwrap_or(false));
            let needs_force = clauses.iter().any(|c| {
                c.patterns.get(i).is_some_and(|pat| {
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
        self.local_vars = saved_locals;
        self.local_count = saved_local_count;
        self.var_slots = saved_var_slots;
        self.var_slots_next = saved_var_slots_next;
        self.var_table_emitted = saved_var_table_emitted;
        self.concrete_vars.insert(lua_name);
    }

    fn gen_where_binds(&mut self, binds: &[TLocalDef]) {
        // Forward-declare ALL where-bound names — values as well as functions —
        // before emitting any definition. A where/let group is mutually
        // recursive in Haskell, and a value may reference itself (e.g. a
        // self-referential lazy list `fib = ... fib ...`). Lua locals are not
        // in scope within their own initializer, so `local x = ...x...` binds
        // the inner `x` to an outer/global, not the new local. Declaring every
        // name first, then assigning, makes self- and mutual references resolve
        // to the locals.
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        {
            let mut i = 0;
            while i < binds.len() {
                let is_func = !binds[i].patterns.is_empty();
                let sname = sanitize_name(&binds[i].name);
                if !self.local_vars.contains(&sname) && seen.insert(sname.clone()) {
                    self.declare_local_fwd(&sname);
                }
                if is_func {
                    let name = &binds[i].name;
                    while i < binds.len() && binds[i].name == *name && !binds[i].patterns.is_empty() {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
        }

        // Now emit all bindings in source order — functions and values
        // interleaved as written. The forward declarations above ensure
        // references resolve regardless of order.
        let mut i = 0;
        while i < binds.len() {
            if binds[i].patterns.is_empty() {
                self.gen_where_value(binds, i);
                i += 1;
            } else {
                self.gen_where_func_group_assign(binds, i);
                let name = &binds[i].name;
                while i < binds.len() && binds[i].name == *name && !binds[i].patterns.is_empty() {
                    i += 1;
                }
            }
        }
    }

    fn gen_where_value(&mut self, binds: &[TLocalDef], i: usize) {
        let bind = &binds[i];
        let sname = sanitize_name(&bind.name);
        // The name was forward-declared in gen_where_binds; assign to it
        // (rather than re-declaring) so the binding's own body can refer to
        // itself and to its mutually-recursive siblings. A cheap value may only
        // be assigned strictly when it does not read a still-nil sibling (a
        // forward or self reference); otherwise it must be thunked so the read
        // happens after every assignment in the group has run.
        let lref = self.lua_ref(&sname);
        self.emit_indent();
        if Self::is_cheap(&bind.body) && strict_binding_safe(binds, i) {
            self.emit(&format!("{} = ", lref));
            self.gen_expr(&bind.body);
            self.concrete_vars.insert(sname);
        } else {
            self.emit(&format!("{} = __thunk(function() return ", lref));
            self.gen_expr(&bind.body);
            self.emit(" end)");
        }
        self.emit("\n");
    }

    fn gen_where_func_group_assign(&mut self, binds: &[TLocalDef], start: usize) {
        // Emit as assignment (name already forward-declared)
        self.gen_where_func_group_impl(binds, start, true);
    }

    fn gen_where_func_group_impl(&mut self, binds: &[TLocalDef], start: usize, pre_declared: bool) {
        let name = &binds[start].name;
        let mut clauses = Vec::new();
        let num_params = binds[start].patterns.len();
        let mut i = start;
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
        let sname = sanitize_name(name);
        if !pre_declared {
            self.local_vars.insert(sname.clone());
            self.local_count += 1;
        }
        self.emit_indent();
        if pre_declared {
            // Name was forward-declared; use assignment form
            let lref = self.lua_ref(&sname);
            self.emit(&format!("{} = function({})\n", lref, params_str));
        } else if self.local_count > Self::LOCAL_LIMIT {
            if !self.var_table_emitted {
                self.emit_line("local _v = {}");
                self.var_table_emitted = true;
            }
            self.var_slots_next += 1;
            self.var_slots.insert(sname.clone(), self.var_slots_next);
            self.emit(&format!("_v[{}] = function({})\n", self.var_slots_next, params_str));
        } else {
            self.emit(&format!("local function {}({})\n", sname, params_str));
        }
        self.indent += 1;

        if clauses.len() == 1 {
            let clause = &clauses[0];
            let all_simple = clause.patterns.iter().all(|p|
                matches!(p, TPattern::Var(_, _) | TPattern::Wildcard));

            if all_simple {
                for (j, pat) in clause.patterns.iter().enumerate() {
                    if let TPattern::Var(v, _) = pat {
                        self.emit_line(&format!("local {} = _warg{}", sanitize_name(v), j));
                    }
                }
                self.emit_indent();
                self.emit("return ");
                self.gen_expr(&clause.body);
                self.emit("\n");
            } else {
                for (j, pat) in clause.patterns.iter().enumerate() {
                    if !matches!(pat, TPattern::Var(_, _) | TPattern::Wildcard) {
                        self.emit_line(&format!("_warg{} = __force(_warg{})", j, j));
                    }
                }
                self.gen_pattern_match(&params, &clauses);
            }
        } else {
            for j in 0..num_params {
                let needs_force = clauses.iter().any(|c| {
                    c.patterns.get(j).is_some_and(|pat| {
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

    fn gen_pattern_match(&mut self, params: &[String], clauses: &[TClause]) {
        // Clauses with guards need fallthrough semantics (a clause whose pattern
        // matches but whose guards all fail must drop to the next clause). The
        // if/elseif chain below cannot express that across a pattern boundary, so
        // route any guard-bearing match through the independent-block emitter.
        if clauses.iter().any(|c| !c.guards.is_empty()) {
            self.gen_pattern_match_guarded(params, clauses);
            return;
        }
        for (i, clause) in clauses.iter().enumerate() {
            let keyword = if i == 0 { "if" } else { "elseif" };

            // Each clause is an independent Lua branch (if/elseif … then … end),
            // so its locals must not leak into sibling clauses. Without this,
            // a name bound in one clause stays in `local_vars` and a later
            // clause's `let`/where binding of the same name is emitted without
            // `local` — assigning to a shared global instead, which corrupts
            // when captured by a thunk across calls (e.g. nested FOR loops).
            let scope_lv = self.local_vars.clone();
            let scope_vs = self.var_slots.clone();
            let scope_vsn = self.var_slots_next;
            let scope_lc = self.local_count;
            let scope_vte = self.var_table_emitted;
            let scope_cv = self.concrete_vars.clone();

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
                        let decl = self.declare_local(var);
                        self.emit_line(&format!("{} = {}", decl, val));
                        if self.concrete_vars.contains(val) {
                            self.concrete_vars.insert(var.clone());
                        }
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
                        let decl = self.declare_local(var);
                        self.emit_line(&format!("{} = {}", decl, val));
                        if self.concrete_vars.contains(val) {
                            self.concrete_vars.insert(var.clone());
                        }
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
                        let decl = self.declare_local(var);
                        self.emit_line(&format!("{} = {}", decl, val));
                        // Propagate concreteness: if binding source is concrete, so is the target
                        if self.concrete_vars.contains(val) {
                            self.concrete_vars.insert(var.clone());
                        }
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
                    let decl = self.declare_local(var);
                    self.emit_line(&format!("{} = {}", decl, val));
                    // Propagate concreteness: if binding source is concrete, so is the target
                    if self.concrete_vars.contains(val) {
                        self.concrete_vars.insert(var.clone());
                    }
                }
                self.gen_where_binds(&clause.where_binds);
                self.emit_indent(); self.emit("return "); self.gen_expr(&clause.body); self.emit("\n");
                self.indent -= 1;
            }

            // Restore the scope captured at the start of this clause so its
            // locals do not leak into the next clause.
            self.local_vars = scope_lv;
            self.var_slots = scope_vs;
            self.var_slots_next = scope_vsn;
            self.local_count = scope_lc;
            self.var_table_emitted = scope_vte;
            self.concrete_vars = scope_cv;
        }
        self.emit_line("end");
        self.emit_line("error(\"Non-exhaustive patterns\")");
    }

    /// Pattern match where at least one clause carries guards. Each clause is
    /// emitted as an independent block — `if <pat-conds> then …` for a refutable
    /// pattern, `do …` for an irrefutable one — rather than a single if/elseif
    /// chain. A clause whose pattern matches but whose guards all fail simply
    /// reaches the end of its block and falls through to the next clause, which
    /// is exactly Haskell's semantics. (The flat if/elseif chain cannot do this:
    /// once a pattern's `then` arm is entered there is no way back to the next
    /// `elseif`.)
    fn gen_pattern_match_guarded(&mut self, params: &[String], clauses: &[TClause]) {
        for clause in clauses {
            let mut conditions = Vec::new();
            let mut bindings = Vec::new();
            for (pi, pat) in clause.patterns.iter().enumerate() {
                self.collect_pattern_conditions(&params[pi], pat, &mut conditions, &mut bindings);
            }
            self.emit_indent();
            if conditions.is_empty() {
                self.emit("do\n");
            } else {
                self.emit(&format!("if {} then\n", conditions.join(" and ")));
            }
            self.indent += 1;
            for (var, val) in &bindings {
                let decl = self.declare_local(var);
                self.emit_line(&format!("{} = {}", decl, val));
                if self.concrete_vars.contains(val) {
                    self.concrete_vars.insert(var.clone());
                }
            }
            self.gen_where_binds(&clause.where_binds);
            if clause.guards.is_empty() {
                self.emit_indent(); self.emit("return "); self.gen_expr(&clause.body); self.emit("\n");
            } else {
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
            }
            self.indent -= 1;
            self.emit_line("end");
        }
        self.emit_line("error(\"Non-exhaustive patterns\")");
    }

    /// A sub-pattern that inspects its value (matches a tag, compares a
    /// literal, or destructures further) needs that value forced first;
    /// a Var/Wildcard just binds/ignores it and can stay lazy.
    fn pattern_inspects_value(pattern: &TPattern) -> bool {
        match pattern {
            TPattern::Var(..) | TPattern::Wildcard => false,
            TPattern::Paren(inner) => Self::pattern_inspects_value(inner),
            _ => true,
        }
    }

    /// Build an indexing path into a field, forcing it when the sub-pattern
    /// will inspect it. The field may hold a thunk (lazy construction), so
    /// indexing into it (`field[1]`, `field == tag`, ...) requires forcing.
    fn field_path(scrutinee: &str, idx: usize, child: &TPattern) -> String {
        let path = format!("{}[{}]", scrutinee, idx);
        if Self::pattern_inspects_value(child) {
            format!("__force({})", path)
        } else {
            path
        }
    }

    /// Like `field_path`, but for a LuaDict field addressed by name (`.width`).
    fn field_path_key(scrutinee: &str, key: &str, child: &TPattern) -> String {
        let path = format!("{}{}", scrutinee, lua_field_index(key));
        if Self::pattern_inspects_value(child) {
            format!("__force({})", path)
        } else {
            path
        }
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
                            let path = Self::field_path(scrutinee, i + 2, arg);
                            self.collect_pattern_conditions(&path, arg, conditions, bindings);
                        }
                    } else if let Some(fields) = self.luadict_con_fields.get(name) {
                        // Single LuaDict constructor: bind each positional
                        // sub-pattern from its named table key.
                        for (i, arg) in args.iter().enumerate() {
                            let path = Self::field_path_key(scrutinee, &fields[i], arg);
                            self.collect_pattern_conditions(&path, arg, conditions, bindings);
                        }
                    } else {
                        for (i, arg) in args.iter().enumerate() {
                            let path = Self::field_path(scrutinee, i + 1, arg);
                            self.collect_pattern_conditions(&path, arg, conditions, bindings);
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
                            if !args.is_empty() {
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
                    let path = Self::field_path(scrutinee, i + 1, p);
                    self.collect_pattern_conditions(&path, p, conditions, bindings);
                }
            }
        }
    }

    /// Returns true if an expression is cheap enough that thunking it would
    /// cost more than evaluating it eagerly. This prevents thunk chain buildup
    /// Collect elements of a literal list (cons chain ending in nil).
    /// Returns Some(vec![elem1, elem2, ...]) if the list has >= 8 literal elements,
    /// None otherwise (let normal cons generation handle short lists).
    fn collect_list_literal(expr: &TExpr) -> Option<Vec<&TExpr>> {
        let mut elems = Vec::new();
        let mut cur = expr;
        loop {
            match &cur.kind {
                TExprKind::App(func, tail) => {
                    if let TExprKind::App(inner_f, elem) = &func.kind
                        && let TExprKind::Con(name) = &inner_f.kind
                            && name == ":" {
                                elems.push(elem.as_ref());
                                cur = tail.as_ref();
                                continue;
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
            TExprKind::Tuple(elems) => elems.iter().all(Self::is_cheap),
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
                self.scan_call_sites(&clause.body, &mut ever_thunked, &mut ever_called);
                // Guard conditions and bodies are call sites too — without
                // scanning them, a recursive call that appears only inside a
                // guard (e.g. `f n | ... = f (g n)`) is missed, and the
                // parameter is wrongly judged always-cheap (concrete) while the
                // actual emission thunks the argument.
                for g in &clause.guards {
                    self.scan_call_sites(&g.condition, &mut ever_thunked, &mut ever_called);
                    self.scan_call_sites(&g.body, &mut ever_thunked, &mut ever_called);
                }
                for wb in &clause.where_binds {
                    self.scan_call_sites(&wb.body, &mut ever_thunked, &mut ever_called);
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

    fn scan_call_sites(&self, expr: &TExpr,
        ever_thunked: &mut std::collections::HashMap<String, Vec<bool>>,
        ever_called: &mut std::collections::HashMap<String, Vec<bool>>,
    ) {
        // Iterative right-spine walk for bind chains
        let mut expr = expr;
        loop {
            match &expr.kind {
                TExprKind::InfixApp { op, lhs, rhs } if op == ">>=" || op == ">>" => {
                    self.scan_call_sites(lhs, ever_thunked, ever_called);
                    if let TExprKind::Lambda { body, .. } = &rhs.kind {
                        expr = body;
                        continue;
                    }
                    expr = rhs;
                    continue;
                }
                TExprKind::Let { binds, body } => {
                    for bind in binds { self.scan_call_sites(&bind.body, ever_thunked, ever_called); }
                    expr = body;
                    continue;
                }
                _ => break,
            }
        }
        match &expr.kind {
            TExprKind::App(_, _) => {
                let mut args: Vec<&TExpr> = vec![];
                let mut f = expr;
                while let TExprKind::App(inner_f, inner_arg) = &f.kind {
                    args.push(inner_arg.as_ref());
                    f = inner_f.as_ref();
                }
                args.reverse();
                if let TExprKind::Var(name) = &f.kind
                    && let Some(thunked) = ever_thunked.get_mut(name.as_str()) {
                        let called = ever_called.get_mut(name.as_str()).unwrap();
                        for (i, arg) in args.iter().enumerate() {
                            if i < thunked.len() {
                                called[i] = true;
                                if !Self::is_cheap_arg(arg, &self.inline_fns) {
                                    thunked[i] = true;
                                }
                            }
                        }
                    }
                for arg in &args {
                    self.scan_call_sites(arg, ever_thunked, ever_called);
                }
                if !matches!(&f.kind, TExprKind::Var(_) | TExprKind::Con(_)) {
                    self.scan_call_sites(f, ever_thunked, ever_called);
                }
            }
            TExprKind::InfixApp { lhs, rhs, .. } => {
                self.scan_call_sites(lhs, ever_thunked, ever_called);
                self.scan_call_sites(rhs, ever_thunked, ever_called);
            }
            TExprKind::Lambda { body, .. } => self.scan_call_sites(body, ever_thunked, ever_called),
            TExprKind::If { cond, then_branch, else_branch } => {
                self.scan_call_sites(cond, ever_thunked, ever_called);
                self.scan_call_sites(then_branch, ever_thunked, ever_called);
                self.scan_call_sites(else_branch, ever_thunked, ever_called);
            }
            TExprKind::Let { binds, body } => {
                for bind in binds { self.scan_call_sites(&bind.body, ever_thunked, ever_called); }
                self.scan_call_sites(body, ever_thunked, ever_called);
            }
            TExprKind::Case { scrutinee, branches } => {
                self.scan_call_sites(scrutinee, ever_thunked, ever_called);
                for b in branches {
                    for g in &b.guards {
                        self.scan_call_sites(&g.condition, ever_thunked, ever_called);
                        self.scan_call_sites(&g.body, ever_thunked, ever_called);
                    }
                    self.scan_call_sites(&b.body, ever_thunked, ever_called);
                }
            }
            TExprKind::Paren(inner) | TExprKind::Negate(inner) => self.scan_call_sites(inner, ever_thunked, ever_called),
            TExprKind::Tuple(elems) => { for e in elems { self.scan_call_sites(e, ever_thunked, ever_called); } }
            TExprKind::SpecCall { args, .. } => { for a in args { self.scan_call_sites(a, ever_thunked, ever_called); } }
            TExprKind::OutgoingCallback { callee, .. } => self.scan_call_sites(callee, ever_thunked, ever_called),
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
            TExprKind::Tuple(elems) => elems.iter().any(Self::body_has_constructors),
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
                    self.gen_operand_subst(lhs, subst);
                    self.emit(" / ");
                    self.gen_operand_subst(rhs, subst);
                    self.emit(")");
                    return;
                }
                if op == "++" {
                    self.emit("__mll_list_append(");
                    self.gen_expr_subst(lhs, subst);
                    self.emit(", function() return ");
                    self.gen_expr_subst(rhs, subst);
                    self.emit(" end)");
                    return;
                }
                if op == "!!" {
                    self.emit("__mll_list_index(");
                    self.gen_expr_subst(lhs, subst);
                    self.emit(", ");
                    self.gen_expr_subst(rhs, subst);
                    self.emit(")");
                    return;
                }
                if op == "$" {
                    // return $ x / pure $ x in action context: just emit x
                    if matches!(&lhs.kind, TExprKind::Var(n) if n == "pure" || n == "return") {
                        self.gen_expr_subst(rhs, subst);
                        return;
                    }
                    self.gen_expr_subst(lhs, subst);
                    self.emit("(__thunk(function() return ");
                    self.gen_expr_subst(rhs, subst);
                    self.emit(" end))");
                    return;
                }
                let lua_op = match op.as_str() {
                    "<>" => "..", "&&" => "and", "||" => "or", "/=" => "~=",
                    "mod" => "%",
                    other => other,
                };
                if is_builtin_op(op) {
                    self.emit("(");
                    self.gen_operand_subst(lhs, subst);
                    self.emit(&format!(" {} ", lua_op));
                    self.gen_operand_subst(rhs, subst);
                    self.emit(")");
                } else {
                    let sop = sanitize_name(op);
                    self.emit(&self.lua_ref(&sop)); self.emit("(");
                    self.gen_expr_subst(lhs, subst); self.emit(", ");
                    self.gen_expr_subst(rhs, subst); self.emit(")");
                }
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
            TExprKind::Lambda { params, body } => {
                // Remove shadowed names from substitution
                let mut inner_subst = subst.clone();
                let saved_locals = self.local_vars.clone();
                let saved_concrete = self.concrete_vars.clone();
                // A lambda parameter is NOT guaranteed forced (it may receive a
                // thunk through a higher-order call), so drop it from
                // concrete_vars — a same-named outer binding may be concrete —
                // to force its uses in the body. See the gen_expr Lambda arm.
                for (name, _) in params {
                    inner_subst.remove(name.as_str());
                    let sp = sanitize_name(name);
                    self.local_vars.insert(sp.clone());
                    self.concrete_vars.remove(&sp);
                }
                let ps: Vec<String> = params.iter().map(|(s, _)| sanitize_name(s)).collect();
                self.emit(&format!("function({})\n", ps.join(", ")));
                self.indent += 1;
                self.emit_indent(); self.emit("return ");
                self.gen_expr_subst(body, &inner_subst);
                self.emit("\n");
                self.indent -= 1;
                self.emit_indent(); self.emit("end");
                self.local_vars = saved_locals;
                self.concrete_vars = saved_concrete;
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
            TExprKind::If { cond, then_branch, else_branch } => {
                self.emit("(function()\n");
                self.indent += 1;
                self.emit_indent(); self.emit("if ");
                self.gen_expr_subst(cond, subst);
                self.emit(" then\n");
                self.indent += 1;
                self.emit_indent(); self.emit("return ");
                self.gen_expr_subst(then_branch, subst);
                self.emit("\n");
                self.indent -= 1;
                self.emit_indent(); self.emit("else\n");
                self.indent += 1;
                self.emit_indent(); self.emit("return ");
                self.gen_expr_subst(else_branch, subst);
                self.emit("\n");
                self.indent -= 1;
                self.emit_indent(); self.emit("end\n");
                self.indent -= 1;
                self.emit_indent(); self.emit("end)()");
            }
            TExprKind::Tuple(elems) => {
                self.emit("{");
                for (i, elem) in elems.iter().enumerate() {
                    if i > 0 { self.emit(", "); }
                    self.gen_expr_subst(elem, subst);
                }
                self.emit("}");
            }
            _ => self.gen_expr(expr),
        }
    }

    /// Check if an expression is cheap enough to pass without thunking.
    /// Whether an argument is cheap enough to evaluate eagerly rather than
    /// wrap in a thunk.
    ///
    /// Cheap: literals, variables, lambdas (see is_cheap); constructor
    /// applications with cheap fields; and a *saturated call to an inlinable
    /// function* — inline candidates are non-recursive with a cheap (plain
    /// arithmetic) body, so calling one is O(1) and terminates.
    ///
    /// NOT cheap: a general or recursive function call. Even a single-level
    /// call can be expensive or non-terminating (e.g. a recursive call that
    /// streams an infinite list), so it must be thunked to preserve non-strict
    /// semantics. Treating every one-level call as cheap forced such arguments
    /// eagerly and diverged on lazy code like `concatMap` / list comprehensions
    /// over `[n..]`.
    fn is_cheap_arg(
        expr: &TExpr,
        inline_fns: &std::collections::HashMap<String, (Vec<String>, TExpr)>,
    ) -> bool {
        if Self::is_cheap(expr) { return true; }
        match &expr.kind {
            TExprKind::App(_, _) => {
                // Peel the application spine, requiring every argument cheap.
                let mut argc = 0usize;
                let mut f = expr;
                while let TExprKind::App(func, arg) = &f.kind {
                    if !Self::is_cheap_arg(arg, inline_fns) { return false; }
                    argc += 1;
                    f = func;
                }
                // Constructor application: cheap (O(1) WHNF).
                if matches!(&f.kind, TExprKind::Con(_)) {
                    return true;
                }
                // Saturated call to an inlinable function: cheap.
                if let TExprKind::Var(name) = &f.kind
                    && let Some((params, _)) = inline_fns.get(name) {
                        return params.len() == argc;
                    }
                false
            }
            TExprKind::Paren(inner) => Self::is_cheap_arg(inner, inline_fns),
            _ => false,
        }
    }

    /// Check if an expression contains a function call where the function
    /// is NOT a known top-level/prelude name. Such calls could be to
    /// arbitrary function parameters and may be expensive.
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
        // Structural checks FIRST — the monad type variable may be
        // unresolved in bind chains, so we can't rely on the type alone.
        // pure(x) / return(x): performing it just returns x
        if let TExprKind::App(func, arg) = &expr.kind
            && matches!(&func.kind, TExprKind::Var(n) if n == "pure" || n == "return") {
                self.gen_expr(arg);
                return;
            }
        // return $ x / pure $ x: same as return(x)
        if let TExprKind::InfixApp { op, lhs, rhs } = &expr.kind
            && op == "$" && matches!(&lhs.kind, TExprKind::Var(n) if n == "pure" || n == "return") {
                self.gen_expr(rhs);
                return;
            }
        // ST primitive calls now return closures — go through __mll_run like everything else
        if !Self::is_nullary_action_type(&expr.ty) {
            // If the type is concretely non-IO (resolved to a known type),
            // emit as a plain expression. But if the type is unresolved
            // (e.g. where-clause function with uninferred return type),
            // defensively wrap with __mll_run since we may be in a bind chain
            // where the expression must be an action.
            if Self::is_definitely_not_action(&expr.ty) {
                self.gen_expr(expr);
            } else {
                self.emit("__mll_run(");
                self.gen_expr(expr);
                self.emit(")");
            }
            return;
        }
        match &expr.kind {
            TExprKind::Lit(_) | TExprKind::Con(_) | TExprKind::Tuple(_) => {
                self.gen_expr(expr);
            }
            // IO SpecCall: inline the Lua call directly (skip closure)
            TExprKind::SpecCall { specialized, args, .. } if specialized.starts_with("__mll_io:") => {
                let lua_func = &specialized["__mll_io:".len()..];
                if let Some(method) = lua_func.strip_prefix(':') {
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
            // Fully-applied ST intrinsic in run-once position: emit the
            // effect directly, skipping the action-closure allocation and
            // the __mll_run dispatch. gen_action is only reached where an
            // action runs exactly once, in order, so this is safe by
            // construction. See st_intrinsic_fused.
            _ if Self::st_intrinsic_fused(expr).is_some() => {
                let (fused, fargs) = Self::st_intrinsic_fused(expr).unwrap();
                self.emit(fused);
                self.emit("(");
                for (i, a) in fargs.iter().enumerate() {
                    if i > 0 { self.emit(", "); }
                    self.gen_arg(a, false);
                }
                self.emit(")");
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

    fn is_nullary_action_type(ty: &Ty) -> bool {
        matches!(ty, Ty::IO(_) | Ty::LuaIO(_, _))
            || matches!(ty, Ty::App(f, _) if matches!(f.as_ref(),
                Ty::App(c, _) if matches!(c.as_ref(), Ty::Con(n) if n == "ST")))
    }

    /// Returns true if the type is definitely NOT an IO/ST action.
    /// Unresolved type variables and type applications with variable
    /// heads return false (they might be actions).
    fn is_definitely_not_action(ty: &Ty) -> bool {
        matches!(ty,
            Ty::Con(_) | Ty::Arrow(_, _) | Ty::List(_) | Ty::Unit
            | Ty::Forall(_, _) | Ty::Skolem(..))
    }

    /// If `expr` is a *fully applied* call to a known ST array intrinsic,
    /// return the closure-free runtime function name and the argument list.
    ///
    /// In run-once position (a do-block bind chain), `readSTArray arr i`
    /// compiles to `__mll_run(__mll_ma_read(arr, i))`, where `__mll_ma_read`
    /// allocates an action closure that `__mll_run` immediately calls. The
    /// fused `__mll_st_*` functions perform the effect directly and return
    /// the value, so the caller can emit a single direct call — no closure
    /// allocation, no `__mll_run` dispatch. See
    /// examples/tracker/PERF-REGRESSION.md.
    ///
    /// Returns None for partial applications (an Arrow, never in action
    /// position), first-class action references (`__mll_run(<var>)`), and
    /// non-intrinsics — all of which keep the closure form.
    fn st_intrinsic_fused(expr: &TExpr) -> Option<(&'static str, Vec<&TExpr>)> {
        let mut args: Vec<&TExpr> = Vec::new();
        let mut f = expr;
        while let TExprKind::App(inner_f, inner_arg) = &f.kind {
            args.push(inner_arg.as_ref());
            f = inner_f.as_ref();
        }
        args.reverse();
        let name = match &f.kind {
            TExprKind::Var(n) => n.as_str(),
            _ => return None,
        };
        let (fused, arity) = match name {
            "newSTArray" => ("__mll_st_new", 2),
            "readSTArray" => ("__mll_st_read", 2),
            "writeSTArray" => ("__mll_st_write", 3),
            "modifySTArray" => ("__mll_st_modify", 3),
            "stArrayLength" => ("__mll_st_length", 1),
            "newSTArrayFromList" => ("__mll_st_from_list", 1),
            "stArrayToList" => ("__mll_st_to_list", 1),
            _ => return None,
        };
        if args.len() == arity {
            Some((fused, args))
        } else {
            None
        }
    }

    /// Check if a function type's return type is an IO/ST action.
    fn returns_action(ty: &Ty) -> bool {
        match ty {
            Ty::Arrow(_, ret) => Self::returns_action(ret),
            _ => Self::is_nullary_action_type(ty),
        }
    }

    /// Check if a function type's return type is specifically an ST action.
    fn returns_st(ty: &Ty) -> bool {
        match ty {
            Ty::Arrow(_, ret) => Self::returns_st(ret),
            _ => Self::is_st_type(ty),
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
        // Iterative loop for right-spine bind chains to avoid stack overflow
        // on deeply nested do-blocks. Only recurses for non-spine children
        // (individual expressions, if-branches) which have bounded depth.
        let mut expr = expr;
        let mut inside_action = inside_action;
        loop {
            match &expr.kind {
                TExprKind::InfixApp { op, lhs, rhs } if op == ">>=" => {
                    if let TExprKind::Lambda { params, body } = &rhs.kind {
                        let param_name = sanitize_name(&params[0].0);
                        let decl = self.declare_local(&param_name);
                        self.emit_indent();
                        self.emit(&format!("{} = ", decl));
                        self.gen_action(lhs);
                        self.emit("\n");
                        self.concrete_vars.insert(param_name);
                        expr = body;
                        inside_action = true;
                        continue;
                    }
                }
                TExprKind::InfixApp { op, lhs, rhs } if op == ">>" => {
                    let lhs_unwrapped = if let TExprKind::Paren(inner) = &lhs.kind { inner.as_ref() } else { lhs.as_ref() };
                    // return/pure on the LHS of >> is a no-op (pure value discarded)
                    let is_pure_discard = matches!(&lhs_unwrapped.kind,
                        TExprKind::App(func, _) if matches!(&func.kind,
                            TExprKind::Var(n) if n == "pure" || n == "return"));
                    if !is_pure_discard {
                        self.emit_indent();
                        self.gen_action(lhs_unwrapped);
                        self.emit("\n");
                    }
                    expr = rhs;
                    inside_action = true;
                    continue;
                }
                TExprKind::Let { binds, body } => {
                    // Forward-declare all names before assigning so do-block let
                    // bindings can be self- and mutually recursive. Lua locals
                    // are not in scope within their own initializer (see
                    // gen_where_binds for the rationale).
                    {
                        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                        for bind in binds {
                            let bname = sanitize_name(&bind.name);
                            if !self.local_vars.contains(&bname) && seen.insert(bname.clone()) {
                                self.declare_local_fwd(&bname);
                            }
                        }
                    }
                    for (i, bind) in binds.iter().enumerate() {
                        let bname = sanitize_name(&bind.name);
                        let lval = self.local_lvalue(&bname);
                        // Both the if-fast-path and the cheap-path evaluate the
                        // RHS strictly, so they may only be used when the binding
                        // does not read a still-nil sibling (see strict_binding_safe).
                        let strict_ok = strict_binding_safe(binds, i);
                        if let TExprKind::If { cond, then_branch, else_branch } = &bind.body.kind
                            && strict_ok {
                            self.concrete_vars.insert(bname.clone());
                            self.emit_indent();
                            self.emit("if ");
                            self.gen_expr(cond);
                            self.emit(" then ");
                            self.emit(&format!("{} = ", lval));
                            self.gen_expr(then_branch);
                            self.emit(" else ");
                            self.emit(&format!("{} = ", lval));
                            self.gen_expr(else_branch);
                            self.emit(" end\n");
                        } else if Self::is_nullary_action_type(&bind.body.ty) {
                            self.emit_indent();
                            self.emit(&format!("{} = function() return ", lval));
                            self.gen_action(&bind.body);
                            self.emit(" end\n");
                        } else {
                            self.emit_indent();
                            if Self::is_cheap(&bind.body) && strict_ok {
                                self.emit(&format!("{} = ", lval));
                                self.gen_expr(&bind.body);
                                self.emit("\n");
                                self.concrete_vars.insert(bname);
                            } else {
                                self.emit(&format!("{} = __thunk(function() return ", lval));
                                self.gen_expr(&bind.body);
                                self.emit(" end)\n");
                            }
                        }
                    }
                    expr = body;
                    continue;
                }
                _ => {}
            }
            // Terminal expression
            match &expr.kind {
                TExprKind::If { cond, then_branch, else_branch } => {
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
            break;
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

    /// Emit an operand of a strict primitive (arithmetic, comparison) so the
    /// emitted Lua yields a forced scalar rather than a thunk.
    ///
    /// gen_expr emits "concrete" variables bare, on the assumption the caller
    /// already forced them (the strict-parameter convention). That assumption
    /// can fail: a parameter reaches a function as an unevaluated thunk when
    /// the strictness analysis is incomplete, or when the function is invoked
    /// through a higher-order position the caller could not specialize. A
    /// strict operator must see a value, so force variables (even concrete
    /// ones) and any other potentially-thunk expression. Literals, negations
    /// and nested primitive operations already denote values and are emitted
    /// directly.
    fn gen_operand(&mut self, expr: &TExpr) {
        match &expr.kind {
            TExprKind::Lit(_) | TExprKind::Negate(_) => self.gen_expr(expr),
            TExprKind::InfixApp { op, .. }
                if is_builtin_op(op) || op == "div" || op == "mod" => self.gen_expr(expr),
            TExprKind::Paren(inner) => self.gen_operand(inner),
            TExprKind::Var(name) if name == "otherwise" => self.emit("true"),
            TExprKind::Var(name) => {
                // Trust the concrete marking: a genuinely-concrete variable is
                // already a forced value, so emitting it bare (as gen_expr does)
                // avoids a redundant __force on the arithmetic hot path. A
                // non-concrete variable is forced.
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
            _ => {
                self.emit("__force(");
                self.gen_expr(expr);
                self.emit(")");
            }
        }
    }

    /// Substituting counterpart of gen_operand, for the inline path.
    fn gen_operand_subst(
        &mut self,
        expr: &TExpr,
        subst: &std::collections::HashMap<String, &TExpr>,
    ) {
        match &expr.kind {
            TExprKind::Lit(_) | TExprKind::Negate(_) => self.gen_expr_subst(expr, subst),
            TExprKind::InfixApp { op, .. }
                if is_builtin_op(op) || op == "div" || op == "mod" => {
                    self.gen_expr_subst(expr, subst)
                }
            TExprKind::Paren(inner) => self.gen_operand_subst(inner, subst),
            TExprKind::Var(name) if name == "otherwise" => self.emit("true"),
            TExprKind::Var(name) => {
                self.emit("__force(");
                if let Some(repl) = subst.get(name.as_str()) {
                    self.gen_expr(repl);
                } else {
                    let lref = self.lua_ref(&sanitize_name(name));
                    self.emit(&lref);
                }
                self.emit(")");
            }
            _ => {
                self.emit("__force(");
                self.gen_expr_subst(expr, subst);
                self.emit(")");
            }
        }
    }

    /// Emit a variable or nullary constructor as a raw reference WITHOUT
    /// forcing it — for lazy positions such as a cons tail, where forcing
    /// would eagerly evaluate the rest of the spine. A non-concrete variable
    /// already holds a thunk-or-value; the runtime forces it when read.
    fn gen_lazy_ref(&mut self, expr: &TExpr) {
        match &expr.kind {
            TExprKind::Var(name) if name == "otherwise" => self.emit("true"),
            TExprKind::Var(name) => {
                let lref = self.lua_ref(&sanitize_name(name));
                self.emit(&lref);
            }
            TExprKind::Con(name) if name == "[]" => self.emit("nil"),
            TExprKind::Con(name) => {
                let lref = self.lua_ref(&sanitize_name(name));
                self.emit(&lref);
            }
            _ => self.gen_expr(expr),
        }
    }

    /// Emit a function argument expression.
    /// Cheap args (vars, literals, constructor applications) are emitted via
    /// gen_expr which forces non-concrete variables. Expensive args for strict
    /// positions are also emitted via gen_expr. Expensive args for non-strict
    /// positions are wrapped in thunks to preserve non-strict semantics.
    fn gen_arg(&mut self, expr: &TExpr, strict: bool) {
        if Self::is_cheap_arg(expr, &self.inline_fns) || strict {
            self.gen_expr(expr);
        } else {
            self.emit("__thunk(function() return ");
            self.gen_expr(expr);
            self.emit(" end)");
        }
    }

    fn is_st_type(ty: &Ty) -> bool {
        match ty {
            Ty::App(f, _) => match f.as_ref() {
                Ty::App(c, _) => matches!(c.as_ref(), Ty::Con(n) if n == "ST"),
                _ => false,
            },
            _ => false,
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
                    _ => {
                        let lref = self.lua_ref(&sanitize_name(name));
                        self.emit(&lref);
                    }
                }
            }
            TExprKind::Lit(lit) => self.gen_literal(lit),
            TExprKind::App(func, arg) => {
                // Record field accessor: inline as direct table indexing.
                // The field may hold a thunk (lazy construction), so force the
                // projected value. The container is forced by gen_expr(arg) when
                // it is a non-concrete variable; __force is idempotent on values.
                // Laziness is preserved because non-strict argument positions
                // thunk-wrap the whole projection (see gen_arg).
                if let TExprKind::Var(name) = &func.kind
                    && let Some(&idx) = self.record_accessors.get(&sanitize_name(name)) {
                        // A LuaDict field is keyed by name; a plain record field
                        // by position. Compute the index expression before
                        // gen_expr borrows self mutably.
                        let index = match self.luadict_field_key.get(&sanitize_name(name)) {
                            Some(key) => lua_field_index(key),
                            None => format!("[{}]", idx),
                        };
                        self.emit("__force(");
                        self.gen_expr(arg);
                        self.emit(&format!("{})", index));
                        return;
                    }

                // Check for cons application: (:) x xs => __mll_cons(x, xs)
                if let TExprKind::App(inner_f, inner_arg) = &func.kind
                    && let TExprKind::Con(name) = &inner_f.kind
                        && name == ":" {
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
                            // Keep the cons tail lazy. A bare reference — a
                            // variable or a nullary constructor like [] —
                            // already denotes a thunk-or-value, so emit it raw:
                            // forcing it here (gen_expr forces non-concrete
                            // vars) would evaluate the rest of the spine eagerly
                            // and diverge on infinite or self-referential lists
                            // (e.g. `cons x rest = x : rest`). Any tail that
                            // requires computation is wrapped in a thunk. The
                            // runtime forces the cell when read (__mll_head /
                            // __mll_tail), so an unforced tail is safe to store.
                            let tail = {
                                let mut t = arg.as_ref();
                                while let TExprKind::Paren(inner) = &t.kind { t = inner.as_ref(); }
                                t
                            };
                            let tail_is_ref = matches!(&tail.kind,
                                TExprKind::Var(_) | TExprKind::Con(_));
                            if tail_is_ref {
                                self.emit("__mll_cons(");
                                self.gen_expr(inner_arg);
                                self.emit(", ");
                                self.gen_lazy_ref(tail);
                                self.emit(")");
                            } else {
                                self.emit("__mll_lazy_cons(");
                                self.gen_expr(inner_arg);
                                self.emit(", function() return ");
                                self.gen_expr(arg);
                                self.emit(" end)");
                            }
                            return;
                        }

                // seq a b => force a, return b
                if let TExprKind::App(seq_f, seq_a) = &func.kind
                    && let TExprKind::Var(name) = &seq_f.kind
                        && name == "seq" {
                            self.emit("(function() __force(");
                            self.gen_expr(seq_a);
                            self.emit("); return ");
                            // Strip redundant source parens around the returned
                            // expression: in Lua `return f(x)` is a proper tail
                            // call but `return (f(x))` is not, so a parenthesised
                            // call here would defeat TCO and blow the stack on
                            // deep `seq`-strict recursion.
                            let mut b: &TExpr = arg;
                            while let TExprKind::Paren(inner) = &b.kind {
                                b = inner.as_ref();
                            }
                            self.gen_expr(b);
                            self.emit(" end)()");
                            return;
                        }

                // return/pure are identity — emit the argument directly.
                // Thunk arguments that contain calls to unknown functions
                // (parameters, locally-bound variables) to preserve non-strict
                // semantics: `return (f x)` must not eagerly evaluate `f x`
                // when f could be an arbitrary expensive function.
                // Calls to known top-level/prelude functions are safe to
                // evaluate eagerly.
                // return/pure wrap their argument in an IO action closure.
                if let TExprKind::Var(name) = &func.kind
                    && (name == "return" || name == "pure") {
                        self.emit("(function() return ");
                        self.gen_expr(arg);
                        self.emit(" end)");
                        return;
                    }

                // Collect all applied arguments
                let mut args = vec![arg.as_ref()];
                let mut f = func.as_ref();
                while let TExprKind::App(inner_f, inner_arg) = &f.kind {
                    args.push(inner_arg.as_ref());
                    f = inner_f.as_ref();
                }
                args.reverse();

                // try/catch: wrap IO action argument in a closure so that
                // errors are deferred into pcall rather than crashing eagerly.
                if let TExprKind::Var(name) = &f.kind {
                    if name == "try" && args.len() == 1 {
                        self.emit("try_(function() return ");
                        self.gen_action(args[0]);
                        self.emit(" end)");
                        return;
                    }
                    if name == "catch" && args.len() == 2 {
                        self.emit("catch_(function() return ");
                        self.gen_action(args[0]);
                        self.emit(" end, ");
                        self.gen_expr(args[1]);
                        self.emit(")");
                        return;
                    }
                }

                // Typeclass methods on primitive types → inline as Lua operators
                if args.len() == 2
                    && let TExprKind::Var(name) = &f.kind {
                        let lua_op = match name.as_str() {
                            "eq_Integer" | "eq_Number" | "eq_String" | "eq_Bool" | "eq_ByteString" => Some("=="),
                            "ord_lt__Integer" | "ord_lt__Number" | "ord_lt__String" | "ord_lt__ByteString" => Some("<"),
                            "ord_gt__Integer" | "ord_gt__Number" | "ord_gt__String" | "ord_gt__ByteString" => Some(">"),
                            "ord_le__Integer" | "ord_le__Number" | "ord_le__String" | "ord_le__ByteString" => Some("<="),
                            "ord_ge__Integer" | "ord_ge__Number" | "ord_ge__String" | "ord_ge__ByteString" => Some(">="),
                            "semigroup_String" => Some(".."),
                            _ => None,
                        };
                        if let Some(op) = lua_op {
                            self.emit("(");
                            self.gen_operand(args[0]);
                            self.emit(&format!(" {} ", op));
                            self.gen_operand(args[1]);
                            self.emit(")");
                            return;
                        }
                    }

                // semigroup_List → __mll_list_append
                if args.len() == 2
                    && let TExprKind::Var(name) = &f.kind
                        && name == "semigroup_List" {
                            self.emit("__mll_list_append(");
                            self.gen_expr(args[0]);
                            self.emit(", function() return ");
                            self.gen_expr(args[1]);
                            self.emit(" end)");
                            return;
                        }

                // Inline small pure functions at call site
                if let TExprKind::Var(name) = &f.kind
                    && let Some((params, body)) = self.inline_fns.get(name).cloned()
                        && args.len() == params.len() {
                            let mut subst = std::collections::HashMap::new();
                            for (param, arg) in params.iter().zip(args.iter()) {
                                subst.insert(param.clone(), *arg);
                            }
                            self.emit("(");
                            self.gen_expr_subst(&body, &subst);
                            self.emit(")");
                            return;
                        }

                // Look up callee's demand info for call-site strictness decisions
                let callee_strict = if let TExprKind::Var(name) = &f.kind {
                    self.demand_info.strict_params.get(name).cloned()
                } else {
                    None
                };

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
                        let is_strict = callee_strict.as_ref()
                            .is_some_and(|v| v.get(i).copied().unwrap_or(false));
                        self.gen_arg(a, is_strict);
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
                    self.emit("(");
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 { self.emit(", "); }
                        let is_strict = callee_strict.as_ref()
                            .is_some_and(|v| v.get(i).copied().unwrap_or(false));
                        self.gen_arg(a, is_strict);
                    }
                    self.emit(")");
                }
            }
            TExprKind::InfixApp { op, lhs, rhs } => {
                if op == "div" {
                    self.emit("math.floor(");
                    self.gen_operand(lhs);
                    self.emit(" / ");
                    self.gen_operand(rhs);
                    self.emit(")");
                    return;
                }
                if op == "++" {
                    self.emit("__mll_list_append(");
                    self.gen_expr(lhs);
                    self.emit(", function() return ");
                    self.gen_expr(rhs);
                    self.emit(" end)");
                    return;
                }
                if op == "!!" {
                    self.emit("__mll_list_index(");
                    self.gen_expr(lhs);
                    self.emit(", ");
                    self.gen_operand(rhs);
                    self.emit(")");
                    return;
                }
                let lua_op = match op.as_str() {
                    "<>" => "..", "&&" => "and", "||" => "or", "/=" => "~=",
                    "mod" => "%",
                    ":" => {
                        // Keep the cons tail lazy. A bare reference (variable
                        // or []) already denotes a thunk-or-value, so emit it
                        // raw — forcing it would evaluate the rest of the spine
                        // eagerly and diverge on infinite/self-referential
                        // lists (e.g. `cons x rest = x : rest`). Any tail that
                        // requires computation is wrapped in a thunk; the
                        // runtime forces the cell when read. See gen_lazy_ref.
                        let tail = {
                            let mut t = rhs.as_ref();
                            while let TExprKind::Paren(inner) = &t.kind { t = inner.as_ref(); }
                            t
                        };
                        let tail_is_ref = matches!(&tail.kind,
                            TExprKind::Var(_) | TExprKind::Con(_));
                        if tail_is_ref {
                            self.emit("__mll_cons(");
                            self.gen_expr(lhs); self.emit(", "); self.gen_lazy_ref(tail);
                            self.emit(")");
                        } else {
                            self.emit("__mll_lazy_cons(");
                            self.gen_expr(lhs);
                            self.emit(", function() return ");
                            self.gen_expr(rhs);
                            self.emit(" end)");
                        }
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
                    // Lua-native operator: emit as infix. Operands are forced —
                    // a thunk is a table, which would corrupt arithmetic and
                    // comparison, and is truthy under `and`/`or`.
                    self.emit("("); self.gen_operand(lhs);
                    self.emit(&format!(" {} ", lua_op));
                    self.gen_operand(rhs); self.emit(")");
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
            TExprKind::Case { scrutinee, branches } if branches.iter().any(|b| !b.guards.is_empty()) => {
                // Guarded branches: lower to clause-based matching (via the
                // shared pattern-match emitter) so a branch whose pattern
                // matches but whose guards all fail falls through to the next
                // branch, exactly like function-clause guards.
                let saved_locals = self.local_vars.clone();
                let saved_concrete = self.concrete_vars.clone();
                self.emit("(function(_cg)\n"); self.indent += 1;
                self.emit_line("_cg = __force(_cg)");
                self.local_vars.insert("_cg".to_string());
                self.concrete_vars.insert("_cg".to_string());
                let clauses: Vec<TClause> = branches.iter().map(|b| TClause {
                    patterns: vec![b.pattern.clone()],
                    guards: b.guards.clone(),
                    body: b.body.clone(),
                    where_binds: vec![],
                }).collect();
                self.gen_pattern_match(&["_cg".to_string()], &clauses);
                self.local_vars = saved_locals;
                self.concrete_vars = saved_concrete;
                self.indent -= 1; self.emit_indent(); self.emit("end)(");
                self.gen_expr(scrutinee); self.emit(")");
            }
            TExprKind::Case { scrutinee, branches } => {
                self.emit("(function()\n"); self.indent += 1;
                self.emit_indent(); self.emit("local _s = __force("); self.gen_expr(scrutinee); self.emit(")\n");
                for (i, branch) in branches.iter().enumerate() {
                    let mut conditions = Vec::new();
                    let mut bindings = Vec::new();
                    self.collect_pattern_conditions("_s", &branch.pattern, &mut conditions, &mut bindings);
                    // Register pattern-bound names as locals (scoped to this
                    // branch) so references resolve to them rather than a
                    // same-named top-level/prelude function.
                    let saved_locals = self.local_vars.clone();
                    if conditions.is_empty() {
                        if i > 0 { self.emit_indent(); self.emit("else\n"); self.indent += 1; }
                        for (var, val) in &bindings { self.emit_line(&format!("local {} = {}", var, val)); self.local_vars.insert(var.clone()); }
                        self.emit_indent(); self.emit("return "); self.gen_expr(&branch.body); self.emit("\n");
                        if i > 0 { self.indent -= 1; self.emit_line("end"); }
                        self.local_vars = saved_locals;
                        break;
                    }
                    let kw = if i == 0 { "if" } else { "elseif" };
                    self.emit_indent(); self.emit(&format!("{} {} then\n", kw, conditions.join(" and ")));
                    self.indent += 1;
                    for (var, val) in &bindings { self.emit_line(&format!("local {} = {}", var, val)); self.local_vars.insert(var.clone()); }
                    self.emit_indent(); self.emit("return "); self.gen_expr(&branch.body); self.emit("\n");
                    self.indent -= 1;
                    if i == branches.len() - 1 { self.emit_line("end"); }
                    self.local_vars = saved_locals;
                }
                self.indent -= 1; self.emit_indent(); self.emit("end)()");
            }
            TExprKind::Let { binds, body } => {
                self.emit("(function()\n"); self.indent += 1;
                let saved_locals = self.local_vars.clone();
                let saved_concrete = self.concrete_vars.clone();
                // Forward-declare all names before assigning, so let bindings
                // can be self- and mutually recursive. Lua locals are not in
                // scope within their own initializer, so `local x = ...x...`
                // would bind the inner `x` to an outer/global. See
                // gen_where_binds for the same rationale.
                {
                    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                    let names: Vec<String> = binds.iter()
                        .map(|b| sanitize_name(&b.name))
                        .filter(|n| seen.insert(n.clone()))
                        .collect();
                    if !names.is_empty() {
                        self.emit_indent();
                        self.emit(&format!("local {}\n", names.join(", ")));
                    }
                    // Register the names as locals so references in the bodies
                    // resolve to these bindings, not a same-named top-level or
                    // prelude function (e.g. a let-bound `sum` or `last`).
                    for n in &names { self.local_vars.insert(n.clone()); }
                }
                for (i, bind) in binds.iter().enumerate() {
                    self.emit_indent();
                    let sname = sanitize_name(&bind.name);
                    if Self::is_cheap(&bind.body) && strict_binding_safe(binds, i) {
                        self.emit(&format!("{} = ", sname));
                        self.gen_expr(&bind.body); self.emit("\n");
                        self.concrete_vars.insert(sname);
                    } else {
                        self.emit(&format!("{} = __thunk(function() return ", sname));
                        self.gen_expr(&bind.body); self.emit(" end)\n");
                    }
                }
                self.emit_indent(); self.emit("return "); self.gen_expr(body); self.emit("\n");
                self.indent -= 1; self.emit_indent(); self.emit("end)()");
                self.local_vars = saved_locals;
                self.concrete_vars = saved_concrete;
            }
            TExprKind::Lambda { params, body } => {
                let ps: Vec<String> = params.iter().map(|(s, _)| sanitize_name(s)).collect();
                let saved_locals = self.local_vars.clone();
                let saved_concrete = self.concrete_vars.clone();
                // A lambda parameter is NOT guaranteed forced: when the lambda
                // is invoked through a higher-order position the caller cannot
                // see its strictness and may pass a thunk. Drop the params from
                // concrete_vars (a same-named outer binding may be concrete) so
                // their uses in the body are forced rather than emitted bare.
                for (p, _) in params {
                    let sp = sanitize_name(p);
                    self.local_vars.insert(sp.clone());
                    self.concrete_vars.remove(&sp);
                }
                self.emit(&format!("function({})\n", ps.join(", ")));
                self.indent += 1; self.emit_indent(); self.emit("return ");
                self.gen_expr(body); self.emit("\n"); self.indent -= 1;
                self.emit_indent(); self.emit("end");
                self.local_vars = saved_locals;
                self.concrete_vars = saved_concrete;
            }
            TExprKind::Paren(inner) => {
                self.emit("("); self.gen_expr(inner); self.emit(")");
            }
            TExprKind::OpFunc(op) => {
                if op == "++" {
                    self.emit("function(_a, _b) return __mll_list_append(_a, function() return _b end) end");
                    return;
                }
                if op == "!!" {
                    self.emit("function(_a, _b) return __mll_list_index(_a, __force(_b)) end");
                    return;
                }
                if op == ":" {
                    self.emit("function(_a, _b) return __mll_cons(_a, _b) end");
                    return;
                }
                let lua_op = match op.as_str() {
                    "<>" => "..", "&&" => "and", "||" => "or", "/=" => "~=",
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
                } else if let Some(elem_show) = specialized.strip_prefix("__mll_show_maybe:") {
                    // Specialized Maybe show: type-directed, so Just/Nothing are
                    // recovered from the element type (nil == Nothing).
                    self.emit(&format!("__mll_show_maybe({}, ", self.lua_ref(elem_show)));
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
                    if let Some(method) = lua_func.strip_prefix(':') {
                        // Method call try: handle:method(args)
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
                    // Zero-arg IO (e.g., os.clock): emit raw call without closure wrapper,
                    // since the function definition already wraps in function()...end.
                    let needs_wrapper = !args.is_empty();
                    if needs_wrapper { self.emit("function() return "); }
                    if let Some(method) = lua_func.strip_prefix(':') {
                        // Method call IO: handle:method(args)
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
                    if needs_wrapper { self.emit(" end"); }
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
                self.emit(&format!("{}.{}", sanitize_name(dict_param), sanitize_name(method_name)));
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
            TExprKind::RecordUpdate { record, updates, num_fields } => {
                // A LuaDict record is keyed by name, so we can't copy it
                // positionally: shallow-copy every key with `pairs`, then
                // overwrite the updated fields by name.
                let is_luadict = updates.first()
                    .map(|(fname, _, _)| self.luadict_field_key.contains_key(&sanitize_name(fname)))
                    .unwrap_or(false);
                if is_luadict {
                    self.emit("(function() local _r = __force(");
                    self.gen_expr(record);
                    self.emit("); local _u = {}; for _k, _v in pairs(_r) do _u[_k] = _v end");
                    for (fname, _, val) in updates {
                        self.emit(&format!("; _u{} = ", lua_field_index(fname)));
                        self.gen_expr(val);
                    }
                    self.emit("; return _u end)()");
                    return;
                }
                // Generate: (function() local _r = __force(record)
                //   local _u = {_r[1], _r[2], ...}; _u[i] = val; ...; return _u end)()
                self.emit("(function() local _r = __force(");
                self.gen_expr(record);
                self.emit("); local _u = {");
                for i in 1..=*num_fields {
                    if i > 1 { self.emit(", "); }
                    self.emit(&format!("_r[{}]", i));
                }
                self.emit("}");
                for (_, idx, val) in updates {
                    self.emit(&format!("; _u[{}] = ", idx));
                    self.gen_expr(val);
                }
                self.emit("; return _u end)()");
            }
            TExprKind::OutgoingCallback { callee, arity, marshal_args, run_io, marshal_ret } => {
                self.emit("__mll_wrap_callback_out(");
                self.gen_expr(callee);
                let flags = marshal_args.iter()
                    .map(|b| if *b { "true" } else { "false" })
                    .collect::<Vec<_>>().join(", ");
                self.emit(&format!(", {}, {{{}}}, {}, {})",
                    arity, flags, run_io, marshal_ret));
            }
        }
    }

    /// Generate an expression with lazy cons tails for self-referencing definitions.
    /// Cons operations wrap the tail in a thunk via __mll_lazy_cons.
    fn gen_expr_lazy(&mut self, expr: &TExpr, self_name: &str) {
        // Check for infix cons: x : rest
        if let TExprKind::InfixApp { op, lhs, rhs } = &expr.kind
            && op == ":" {
                self.emit("__mll_lazy_cons(");
                self.gen_expr(lhs);
                self.emit(", function() return ");
                self.gen_expr_lazy(rhs, self_name);
                self.emit(" end)");
                return;
            }
        // Check for App(App(Con(":"), head), tail)
        if let TExprKind::App(func, tail) = &expr.kind
            && let TExprKind::App(con, head) = &func.kind
                && let TExprKind::Con(name) = &con.kind
                    && name == ":" {
                        self.emit("__mll_lazy_cons(");
                        self.gen_expr(head);
                        self.emit(", function() return ");
                        self.gen_expr_lazy(tail, self_name);
                        self.emit(" end)");
                        return;
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

/// Lua reserved words — cannot be used as a bare `.field` key or `{field = …}`.
fn is_lua_keyword(s: &str) -> bool {
    matches!(s,
        "and" | "break" | "do" | "else" | "elseif" | "end" | "false" | "for"
        | "function" | "goto" | "if" | "in" | "local" | "nil" | "not" | "or"
        | "repeat" | "return" | "then" | "true" | "until" | "while")
}

/// True when `name` can appear as a bare Lua identifier key (`.name`, `{name = …}`).
fn lua_bare_key_ok(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !is_lua_keyword(name)
}

/// A bracketed Lua string-literal table key: `["na\"me"]`. Always valid.
fn lua_key_string(name: &str) -> String {
    let mut s = String::from("[\"");
    for c in name.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            _ => s.push(c),
        }
    }
    s.push_str("\"]");
    s
}

/// Suffix that reads a LuaDict field from a table value: `.name` or `["name"]`.
fn lua_field_index(name: &str) -> String {
    if lua_bare_key_ok(name) { format!(".{}", name) } else { lua_key_string(name) }
}

/// Assignment target inside a table constructor: `name = ` or `["name"] = `.
fn lua_field_assign(name: &str) -> String {
    if lua_bare_key_ok(name) { format!("{} = ", name) } else { format!("{} = ", lua_key_string(name)) }
}

fn sanitize_name(name: &str) -> String {
    match name {
        "main" => "__run".to_string(),
        "return" => "return_".to_string(),
        "not" => "not_".to_string(),
        "print" => "print_".to_string(),
        // error_ forces its message before raising; Lua's bare `error` would
        // hand a thunk to error() and print "table: 0x...".
        "error" => "error_".to_string(),
        "end" => "end_".to_string(),
        "then" => "then_".to_string(),
        "do" => "do_".to_string(),
        "in" => "in_".to_string(),
        "or" => "or_".to_string(),
        "and" => "and_".to_string(),
        "try" => "try_".to_string(),
        "catch" => "catch_".to_string(),
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
        "hmFromList" => "hashmap_fromList".to_string(),
        "hmToList" => "hashmap_toList".to_string(),
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
                    // Qualified-import separator: `Map.insert` -> `Map_insert`.
                    '.' => s.push('_'),
                    _ => s.push(c),
                }
            }
            s
        }
    }
}

/// Check if a TExpr references a given name anywhere
/// Would evaluating `expr` eagerly (at module-load time) read another
/// top-level binding's value? A top-level value binding with no params/where
/// has no locals, so every variable it mentions is a global reference.
/// References *inside a lambda* are safe — the closure reads them at call
/// time, after every slot is assigned — but a reference evaluated immediately
/// (a bare alias `y = x`, an operand `useX = x + 1`, a constructor field
/// `c = Just g`) is not: the referent's slot may still be nil when the eager
/// assignment runs. Such a binding must be thunked so the read is deferred to
/// first use.
fn expr_evaluates_global_ref(expr: &TExpr) -> bool {
    match &expr.kind {
        TExprKind::Var(_) => true,
        // A lambda only captures its body; the reads fire at call time.
        TExprKind::Lambda { .. } => false,
        TExprKind::Con(_) | TExprKind::Lit(_) | TExprKind::OpFunc(_) => false,
        TExprKind::App(f, a) => expr_evaluates_global_ref(f) || expr_evaluates_global_ref(a),
        TExprKind::InfixApp { lhs, rhs, .. } => {
            expr_evaluates_global_ref(lhs) || expr_evaluates_global_ref(rhs)
        }
        TExprKind::Negate(e) | TExprKind::Paren(e) => expr_evaluates_global_ref(e),
        TExprKind::If { cond, then_branch, else_branch } => {
            expr_evaluates_global_ref(cond)
                || expr_evaluates_global_ref(then_branch)
                || expr_evaluates_global_ref(else_branch)
        }
        TExprKind::Tuple(elems) => elems.iter().any(expr_evaluates_global_ref),
        // Not reachable from the is_cheap eager path; thunk to be safe.
        _ => true,
    }
}

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
            branches.iter().any(|b| {
                b.guards.iter().any(|g|
                    expr_references_name(&g.condition, name) || expr_references_name(&g.body, name))
                || expr_references_name(&b.body, name)
            })
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
        TExprKind::RecordUpdate { record, updates, .. } => {
            expr_references_name(record, name) ||
            updates.iter().any(|(_, _, e)| expr_references_name(e, name))
        }
        TExprKind::OutgoingCallback { callee, .. } => expr_references_name(callee, name),
    }
}

/// Whether `binds[i]` may be emitted as a *strict* (immediately-evaluated,
/// non-thunk) assignment without reading a still-`nil` sibling.
///
/// A `let`/`where` group is mutually recursive: all names are forward-declared,
/// then assigned in source order. A strict assignment evaluates its RHS at the
/// point it runs, so it may only read siblings whose assignment has already
/// executed — i.e. names at an *earlier* position. A reference to itself or to a
/// later binding (index `>= i`) would read `nil`, so such a binding must be
/// emitted lazily (as a thunk) instead.
///
/// A `Lambda` body is the exception: its body runs when the function is *called*,
/// by which time every assignment in the group has completed, so a function
/// value is always safe to bind strictly regardless of forward references (this
/// is what makes mutually-recursive local functions work).
fn strict_binding_safe(binds: &[TLocalDef], i: usize) -> bool {
    if matches!(binds[i].body.kind, TExprKind::Lambda { .. }) {
        return true;
    }
    // Not-yet-assigned siblings are those at position i (self) and beyond.
    !binds[i..].iter().any(|b| expr_references_name(&binds[i].body, &b.name))
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
        | "<" | ">" | "<=" | ">=" | "++" | "<>" | "&&" | "||" | ".." | "$" | "."
        | "div" | "mod")
}

pub fn generate(module: &TModule, embed_source: Option<(EmbedMode, &str)>) -> String {
    let mut cg = CodeGen::new();
    cg.embed_var_export = matches!(embed_source, Some((EmbedMode::Var, _)));
    cg.demand_info = crate::demand::analyze(module);
    // Generate the program body first so we can see which runtime-prelude
    // functions it actually references, then prepend only those (transitively).
    cg.generate_module(module);
    let body = cg.output;
    let prelude = ondemand_prelude(&body);
    // The embedded-source block goes at the very top of the file: extraction
    // takes the earliest marker, so the genuine block must precede anything
    // user-derived, and placing it before the prelude also keeps the prelude
    // scan above from picking up names mentioned in the source text.
    let mut out = match embed_source {
        Some((mode, source)) => embed::embed_block(source, mode),
        None => String::new(),
    };
    out.push_str(&prelude);
    out.push('\n');
    out.push_str(&body);
    out
}

/// One top-level runtime-prelude definition: the names it introduces and its
/// full source text (including any leading comment and multi-line body).
struct PChunk {
    provides: Vec<String>,
    text: String,
}

/// Emit only the runtime-prelude definitions reachable from `body`.
///
/// Roots are the prelude identifiers that appear in the generated program;
/// the reachable set is the transitive closure over inter-chunk references.
/// References are read from raw chunk text (comments and strings included), so
/// the closure only ever *over*-approximates — it never drops a real dependency.
/// As a final guard, if any referenced prelude name is somehow not provided by
/// the emitted set, fall back to the whole prelude: a parser bug degrades to a
/// larger file, never to broken (nil-global) output.
fn ondemand_prelude(body: &str) -> String {
    let chunks = parse_prelude_chunks();
    let all_names: std::collections::HashSet<&str> =
        chunks.iter().flat_map(|c| c.provides.iter().map(String::as_str)).collect();

    // name -> chunks that provide it (a name may be forward-declared then
    // assigned, so more than one chunk can provide it; include them all).
    let mut providers: std::collections::HashMap<&str, Vec<usize>> = std::collections::HashMap::new();
    for (i, c) in chunks.iter().enumerate() {
        for n in &c.provides {
            providers.entry(n.as_str()).or_default().push(i);
        }
    }

    // Roots: prelude names referenced by the generated body.
    let mut needed: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut work: Vec<&str> = Vec::new();
    for tok in idents(body) {
        if all_names.contains(tok) && needed.insert(tok) {
            work.push(tok);
        }
    }
    // Transitive closure over the references inside each providing chunk.
    while let Some(name) = work.pop() {
        if let Some(idxs) = providers.get(name) {
            for &i in idxs {
                for dep in idents(&chunks[i].text) {
                    if all_names.contains(dep) && needed.insert(dep) {
                        work.push(dep);
                    }
                }
            }
        }
    }

    // Assemble the reachable chunks in their original order.
    let mut out = String::from("-- MLL Runtime (on-demand subset)\n");
    let mut provided: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for c in &chunks {
        if c.provides.iter().any(|n| needed.contains(n.as_str())) {
            out.push_str(&c.text);
            for n in &c.provides {
                provided.insert(n.as_str());
            }
        }
    }

    // Safety net: every prelude name referenced by the body or by an emitted
    // chunk must be defined in the emitted set. If not, the reachability logic
    // is wrong — emit the full prelude rather than broken code.
    let complete = idents(body).chain(idents(&out))
        .filter(|t| all_names.contains(t))
        .all(|t| provided.contains(t));
    if complete { out } else { PRELUDE.to_string() }
}

/// Split the prelude into top-level definition chunks. A chunk starts at a
/// column-0 `local function`, `local …`, or `IDENT = …` line and runs until the
/// next such line; everything else (bodies, `end`, `if/else`, leading comments)
/// stays with its definition.
fn parse_prelude_chunks() -> Vec<PChunk> {
    let mut chunks: Vec<PChunk> = Vec::new();
    let mut cur: Option<PChunk> = None;
    let mut pending = String::new(); // comments/blanks awaiting the next def
    for line in PRELUDE.lines() {
        if is_def_start(line) {
            if let Some(c) = cur.take() {
                chunks.push(c);
            }
            let mut text = std::mem::take(&mut pending);
            text.push_str(line);
            text.push('\n');
            cur = Some(PChunk { provides: provided_names(line), text });
        } else {
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with("--") {
                // Buffer: attaches to the next definition (or flushed into the
                // current body if a continuation line comes first).
                pending.push_str(line);
                pending.push('\n');
            } else if let Some(c) = cur.as_mut() {
                c.text.push_str(&pending);
                pending.clear();
                c.text.push_str(line);
                c.text.push('\n');
            } else {
                pending.push_str(line);
                pending.push('\n');
            }
        }
    }
    if let Some(mut c) = cur {
        c.text.push_str(&pending);
        chunks.push(c);
    }
    chunks
}

/// Is `line` the start of a top-level prelude definition (column 0)?
fn is_def_start(line: &str) -> bool {
    if line.starts_with([' ', '\t']) {
        return false;
    }
    if line.starts_with("local ") {
        return true;
    }
    // Global assignment `IDENT = …` (the FFI-boundary functions), but not `==`.
    let name_len = line.bytes().take_while(|&b| b == b'_' || b.is_ascii_alphanumeric()).count();
    if name_len == 0 || line.as_bytes()[0].is_ascii_digit() {
        return false;
    }
    let after = line[name_len..].trim_start();
    after.starts_with('=') && !after.starts_with("==")
}

/// The names a definition line introduces.
fn provided_names(line: &str) -> Vec<String> {
    let l = line.trim();
    if let Some(rest) = l.strip_prefix("local function ") {
        return vec![rest.chars().take_while(|c| *c == '_' || c.is_ascii_alphanumeric()).collect()];
    }
    if let Some(rest) = l.strip_prefix("local ") {
        // `local A`, `local A = …`, a forward decl `local A, B, C`, or a
        // `local A; do … end` block. Take the declaration up to `=`/`;`, then
        // the leading identifier of each comma-separated name.
        let decl = rest.split(['=', ';']).next().unwrap_or("");
        return decl.split(',')
            .map(|s| s.trim().chars().take_while(|c| *c == '_' || c.is_ascii_alphanumeric()).collect::<String>())
            .filter(|s| is_ident(s))
            .collect();
    }
    // Global assignment `IDENT = …`.
    let name: String = l.chars().take_while(|c| *c == '_' || c.is_ascii_alphanumeric()).collect();
    if is_ident(&name) { vec![name] } else { vec![] }
}

fn is_ident(s: &str) -> bool {
    let mut cs = s.chars();
    matches!(cs.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && cs.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// Every maximal `[A-Za-z0-9_]` run in `s` (a superset of the Lua identifiers).
fn idents(s: &str) -> impl Iterator<Item = &str> {
    let b = s.as_bytes();
    let mut i = 0;
    std::iter::from_fn(move || {
        while i < b.len() && !(b[i] == b'_' || b[i].is_ascii_alphanumeric()) {
            i += 1;
        }
        if i >= b.len() {
            return None;
        }
        let start = i;
        while i < b.len() && (b[i] == b'_' || b[i].is_ascii_alphanumeric()) {
            i += 1;
        }
        Some(&s[start..i])
    })
}

const PRELUDE: &str = r#"-- MLL Runtime
local __unpack = table.unpack or unpack

-- Thunk infrastructure (non-strict evaluation)
local __thunk_mt = {}
local __cons_mt = {}
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
local function __mll_cons(h, t) return setmetatable({h, t}, __cons_mt) end
local function __mll_lazy_cons(h, thunk) return setmetatable({h, thunk, __lazy = true}, __cons_mt) end
local function __mll_head(l) l = __force(l); return l[1] end
local function __mll_tail(l)
    l = __force(l)
    if l.__lazy then
        l[2] = l[2]()
        l.__lazy = nil
    end
    -- The tail may be an unforced thunk: a recursive cons whose tail is a
    -- variable (e.g. `x : rest`) stores it raw so the spine is not forced
    -- eagerly at construction (which would diverge on infinite lists). Force
    -- it to WHNF here — one spine step, on demand — and memoize, so the cell
    -- meets the "tail is WHNF" invariant that show/eq/append rely on.
    local t = l[2]
    if getmetatable(t) == __thunk_mt then
        t = __force(t)
        l[2] = t
    end
    return t
end

-- List append (second arg is a thunk for laziness)
local function __mll_list_append(xs, ys_thunk)
    xs = __force(xs)
    if xs == nil then return ys_thunk() end
    return __mll_lazy_cons(__mll_head(xs), function()
        return __mll_list_append(__mll_tail(xs), ys_thunk)
    end)
end

local function __mll_list_index(xs, n)
    n = __force(n)
    xs = __force(xs)
    while n > 0 do
        if xs == nil then error("(!!): index too large") end
        xs = __force(__mll_tail(xs))
        n = n - 1
    end
    if xs == nil then error("(!!): index too large") end
    return __mll_head(xs)
end

-- Deep-force an MLL value for export to Lua.
-- Converts lazy cons lists to plain Lua arrays, forces thunks, recurses into tuples.
local function __mll_to_lua(x)
    x = __force(x)
    if type(x) ~= "table" then return x end
    -- Cons list: identified by __cons_mt metatable
    if getmetatable(x) == __cons_mt then
        local result = {}
        local cur = x
        while cur ~= nil do
            cur = __force(cur)
            if getmetatable(cur) ~= __cons_mt then break end
            result[#result + 1] = __mll_to_lua(__force(cur[1]))
            cur = __mll_tail(cur)
        end
        return result
    end
    -- LuaDict record: a name-keyed table (no positional [1]). Preserve its
    -- string keys so exported functions and callbacks hand Lua a real
    -- dictionary. (Positional ADTs and tuples always fill [1]; cons lists were
    -- handled above; so a keyless [1] can only be a LuaDict or empty table.)
    if x[1] == nil then
        local result = {}
        for k, v in pairs(x) do result[k] = __mll_to_lua(v) end
        return result
    end
    -- Tuple or ADT: force each element
    local result = {}
    for i, v in ipairs(x) do result[i] = __mll_to_lua(v) end
    return result
end

-- Forward declarations for mutual recursion
local __lua_to_mll, __mll_wrap_callback

-- Convert a Lua value to MLL representation at the FFI boundary.
-- Lua arrays become cons lists, functions become wrapped callbacks.
__lua_to_mll = function(x)
    if type(x) == "function" then return __mll_wrap_callback(x) end
    if type(x) ~= "table" then return x end
    if getmetatable(x) == __cons_mt then return x end
    local n = #x
    local result = nil
    for i = n, 1, -1 do result = __mll_cons(__lua_to_mll(x[i]), result) end
    return result
end

-- Wrap a Lua callback so it deep-forces all arguments before forwarding.
-- Used at the FFI boundary: Lua functions don't understand MLL thunks.
__mll_wrap_callback = function(f)
    return function(...)
        local args = {n = select('#', ...), ...}
        for i = 1, args.n do args[i] = __mll_to_lua(args[i]) end
        return __lua_to_mll(f(__unpack(args, 1, args.n)))
    end
end

-- Wrap an mata-ll callback `f` so a Lua host can call it with `n` positional
-- arguments (mata-ll → Lua direction). mata-ll functions are n-ary, so the n
-- arguments are applied in a single call. `marshal[i]` converts argument i
-- across the boundary (lists/nested callbacks) versus passing it raw (an opaque
-- polymorphic value such as a fold's threaded state must round-trip untouched).
-- `run_io` runs the returned action for effectful callbacks; `marshal_ret`
-- converts the result for the host versus returning it raw (opaque state stays
-- raw).
__mll_wrap_callback_out = function(f, n, marshal, run_io, marshal_ret)
    return function(...)
        -- mata-ll functions are n-ary (all arguments at once), so collect the
        -- host's n positional arguments and apply them in a single call.
        local args = {}
        for i = 1, n do
            local v = select(i, ...)
            if marshal[i] then v = __lua_to_mll(v) end
            args[i] = v
        end
        local r = __force(f)(__unpack(args, 1, n))
        if run_io then
            r = __force(r)
            if type(r) == "function" then r = r() end
        end
        if marshal_ret then return __mll_to_lua(r) end
        return r
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
        -- A non-empty list is exactly a cons cell, identified by __cons_mt.
        -- Tuples and constructor tables are plain tables; distinguishing by
        -- shape instead (does x[2] look list-like?) misrenders a tuple whose
        -- second element happens to be a list, e.g. show (1, [2, 3]).
        if getmetatable(x) == __cons_mt then
            local parts = {}
            local cur = x
            while cur ~= nil do
                parts[#parts + 1] = show(__force(cur[1]))
                cur = __mll_tail(cur)
            end
            return "[" .. table.concat(parts, ", ") .. "]"
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
local function __mll_eq(a, b) a = __force(a); b = __force(b); return a == b end
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
-- ByteString is a Lua string; `<` is byte-lexicographic, same as String.
local function ord_lt__ByteString(a, b) a = __force(a); b = __force(b); return a < b end
local function ord_gt__ByteString(a, b) a = __force(a); b = __force(b); return a > b end
local function ord_le__ByteString(a, b) a = __force(a); b = __force(b); return a <= b end
local function ord_ge__ByteString(a, b) a = __force(a); b = __force(b); return a >= b end
-- compare returns the Ordering enum: LT=1, EQ=2, GT=3 (constructor index)
local function ord_compare__Integer(a, b) a = __force(a); b = __force(b); if a < b then return 1 elseif b < a then return 3 else return 2 end end
local function ord_compare__Number(a, b) a = __force(a); b = __force(b); if a < b then return 1 elseif b < a then return 3 else return 2 end end
local function ord_compare__String(a, b) a = __force(a); b = __force(b); if a < b then return 1 elseif b < a then return 3 else return 2 end end
local function ord_compare__ByteString(a, b) a = __force(a); b = __force(b); if a < b then return 1 elseif b < a then return 3 else return 2 end end
local function semigroup_String(a, b) a = __force(a); b = __force(b); return a .. b end
local function semigroup_List(a, b) return __mll_list_append(a, function() return __force(b) end) end
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
local function drop(n, xs)
    n = __force(n); xs = __force(xs)
    while n > 0 and xs ~= nil do
        xs = __mll_tail(xs)
        n = n - 1
    end
    return xs
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
local function hashmap_toList(m) m = __force(m); local r = nil local ks = {} for k in pairs(m) do ks[#ks+1] = k end table.sort(ks) for i = #ks, 1, -1 do r = __mll_cons({ks[i], m[ks[i]]}, r) end return r end

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
local function __mll_show_arg(s)
    s = __force(s)
    -- Parenthesize a derived-Show field at argument position: a constructor
    -- application ("Con a b") or a negative number, matching GHC's showsPrec 11.
    local c = string.byte(s, 1)
    if c == nil then return s end
    local d = string.byte(s, 2)
    if (c >= 65 and c <= 90 and string.find(s, " ", 1, true))
       or (c == 45 and d ~= nil and d >= 48 and d <= 57) then
        return "(" .. s .. ")"
    end
    return s
end
local function __mll_show_maybe(elem_show, x)
    -- Type-directed Maybe show. Just x and x share a runtime rep, so the type
    -- supplies the structure: nil is Nothing, anything else is Just <elem>.
    -- (A wrapped Nothing — Just Nothing — collapses to nil and reads as Nothing.)
    x = __force(x)
    if x == nil then return "Nothing" end
    return "Just " .. __mll_show_arg(elem_show(x))
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
-- Exception handling: try wraps an IO action in pcall, returning Either String a
-- action is a closure (deferred by codegen) so errors happen inside pcall
local function try_(action)
    return function()
        local ok, result = pcall(action)
        if ok then return {2, result} else return {1, tostring(result)} end
    end
end
-- catch runs an IO action; on error, passes the message to a handler
local function catch_(action, handler)
    return function()
        local ok, result = pcall(action)
        if ok then return result
        else return __mll_run(__force(__force(handler)(tostring(result)))) end
    end
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
    return function()
        size = __force(size); init = __force(init)
        local t = {}; for i = 1, size do t[i] = init end; return t
    end
end
local function __mll_ma_read(arr, idx)
    return function() return __force(arr)[__force(idx) + 1] end
end
local function __mll_ma_write(arr, idx, val)
    return function() __force(arr)[__force(idx) + 1] = __force(val) end
end
local function __mll_ma_modify(arr, idx, f)
    return function()
        arr = __force(arr); idx = __force(idx) + 1; f = __force(f)
        arr[idx] = __force(f)(arr[idx])
    end
end
local function __mll_ma_length(arr)
    return function() return #__force(arr) end
end
local function __mll_ma_from_list(xs)
    return function()
        xs = __force(xs); local t = {}; local cur = xs
        while cur ~= nil do t[#t+1] = __force(__mll_head(cur)); cur = __mll_tail(cur) end
        return t
    end
end
local function __mll_ma_to_list(arr)
    return function()
        arr = __force(arr); local r = nil
        for i = #arr, 1, -1 do r = __mll_cons(arr[i], r) end
        return r
    end
end
-- Fused ST array ops: identical effects to __mll_ma_* but performed
-- immediately (no action-closure allocation, no __mll_run dispatch). The
-- codegen emits these only in run-once do-block position; first-class ST
-- actions keep the __mll_ma_* closure form. See st_intrinsic_fused.
local function __mll_st_new(size, init)
    size = __force(size); init = __force(init)
    local t = {}; for i = 1, size do t[i] = init end; return t
end
local function __mll_st_read(arr, idx)
    return __force(arr)[__force(idx) + 1]
end
local function __mll_st_write(arr, idx, val)
    __force(arr)[__force(idx) + 1] = __force(val)
end
local function __mll_st_modify(arr, idx, f)
    arr = __force(arr); idx = __force(idx) + 1
    arr[idx] = __force(f)(arr[idx])
end
local function __mll_st_length(arr)
    return #__force(arr)
end
local function __mll_st_from_list(xs)
    xs = __force(xs); local t = {}; local cur = xs
    while cur ~= nil do t[#t+1] = __force(__mll_head(cur)); cur = __mll_tail(cur) end
    return t
end
local function __mll_st_to_list(arr)
    arr = __force(arr); local r = nil
    for i = #arr, 1, -1 do r = __mll_cons(arr[i], r) end
    return r
end
"#;
