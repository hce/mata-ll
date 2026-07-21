//! Lua code generation: turns a typed, monomorphized TIR module into a single
//! self-contained Lua source file.
//!
//! `generate` is the crate-facing entry point and the only public item. Its
//! pipeline:
//!
//! 1. `crate::demand::analyze` computes per-function parameter strictness and
//!    result demand; the result seeds `CodeGen::demand_info`.
//! 2. `generate_module` (module.rs) registers data types, runs the
//!    whole-program call-site and inlining analyses (analysis.rs), and emits
//!    the program body: constructors, functions, exports.
//! 3. `ondemand_prelude` (runtime.rs) scans the emitted body and prepends only
//!    the runtime-prelude definitions it transitively references.
//! 4. When source embedding is requested, `embed::embed_block` places the
//!    embedded source block above everything else.
//!
//! The only error this pass can produce is the expression-depth diagnostic
//! (see `gen_expr` in expr.rs); everything else was rejected by earlier
//! passes.
//!
//! This file holds the `CodeGen` state struct and its basic plumbing: name
//! resolution (`lua_ref`), local declaration and the `_v` spill table for
//! Lua's 200-local limit, sub-generator management (`new_sub` /
//! `absorb_sub_error`), and the emit helpers.
//!
//! Child modules:
//! - module.rs — module body layout: data-type registration, constructors,
//!   forward declarations, exports
//! - function.rs — top-level functions, clauses, where-binding groups
//! - pattern.rs — pattern-match compilation (conditions, bindings, guards)
//! - expr.rs — the main expression walk (`gen_expr` / `gen_expr_inner`)
//! - thunks.rs — eager-vs-thunk decisions for arguments, callees, forcing
//! - strictness.rs — cheapness and demand predicates behind those decisions
//! - action.rs — IO/ST actions: bind chains, pure boxes, the two runners
//! - inline.rs — substitution-based inlining twins of the emission paths
//! - analysis.rs — whole-program call-site and inline-candidate analyses
//! - ffi.rs — FFI marshalling and type-directed boundary decoding
//! - names.rs — Lua identifier, keyword and string-literal helpers
//! - util.rs — type- and TIR-shape helpers shared across the module
//! - runtime.rs — the runtime prelude and its on-demand chunk selection

use crate::demand::DemandInfo;
use crate::embed::{self, EmbedMode};
use crate::tir::*;
use crate::types::Ty;

mod action;
mod analysis;
mod expr;
mod ffi;
mod function;
mod inline;
mod module;
mod names;
mod pattern;
mod runtime;
mod strictness;
mod thunks;
mod util;

pub(crate) use names::is_lua_keyword;
use runtime::ondemand_prelude;

/// Tracks constructor info for code generation.
struct CodeGen {
    /// Current `gen_expr` recursion depth, bounded by
    /// `crate::MAX_NESTING_DEPTH`. Upstream passes (the parser's and the
    /// typechecker's own depth guards) already bound the structural depth of
    /// what reaches codegen, so this is the last-line backstop: if it ever
    /// fires, `depth_error` carries a clean diagnostic out of `generate`
    /// instead of the walk overflowing the native stack.
    expr_depth: usize,
    /// Set once when the depth guard fires; surfaced by `generate`.
    depth_error: Option<String>,
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
    /// LuaDict data types keyed by *type* name: (type_vars, [(field_name,
    /// field_ty)]). Used to build type-directed decoders for values that cross
    /// the Lua FFI boundary (see ffi_decode_desc).
    luadict_type_fields: std::collections::HashMap<String, (Vec<String>, Vec<(String, Ty)>)>,
    /// LuaDict *enum* constructors (an all-nullary sum type deriving LuaDict):
    /// TIR constructor name -> the Lua string tag it becomes at runtime (the
    /// `as "tag"` rename when present, the constructor name otherwise). Presence
    /// here means "this nullary constructor is a string, not a positional
    /// integer" — construction emits the string and pattern matching compares
    /// against it. Ordering stays declaration-order via the derived Ord/Enum,
    /// which pattern-match on the constructor, not the string.
    luadict_enum_tag: std::collections::HashMap<String, String>,
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
    /// Strictness rows for where-bound local functions currently in scope
    /// (see demand::local_fn_strict_params), keyed by source name and
    /// consulted BEFORE `demand_info.strict_params` at call sites — a local
    /// `go` shadows a same-named top-level function. Entries are installed
    /// by gen_where_binds when a clause's where scope opens and restored at
    /// clause-scope exit (gen_function / gen_pattern_match*), so a row
    /// never outlives its lexical scope or leaks into a sibling clause.
    local_strict_params: std::collections::HashMap<String, Vec<bool>>,
    /// Structured twin of `local_strict_params`: the demand rows of the
    /// where-bound local functions currently in scope (see
    /// demand::local_fn_rows), threaded into demanded_map /
    /// demanded_map_guards so a binding whose value is demanded THROUGH a
    /// call to a where-local counts as demanded. Installed by
    /// gen_where_binds (via clause_local_rows, which shadows every
    /// where-bound name first) and restored at the same scope exits as
    /// `local_strict_params`, so a row never leaks across clauses.
    local_demand_rows: std::collections::HashMap<String, crate::demand::LocalRows>,
    /// The demand the program provably places on the CURRENT function's
    /// result (see `Rows::result_demand`): deep for functions in the
    /// whole-program deep-result set, plain WHNF otherwise. Seeds the
    /// demanded-binding computation for result-position expressions; any
    /// emission whose result is NOT the current function's result (lambdas,
    /// first-class action closures, value-position lets) must reset it to
    /// `Head` around the nested generation.
    cur_result_demand: crate::demand::Demand,
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
            expr_depth: 0,
            depth_error: None,
            constructors: Vec::new(), newtypes: Vec::new(),
            forward_declared: std::collections::HashSet::new(),
            concrete_vars: std::collections::HashSet::new(),
            params_always_cheap: std::collections::HashMap::new(),
            inline_fns: std::collections::HashMap::new(),
            top_level_names: std::collections::HashSet::new(),
            record_accessors: std::collections::HashMap::new(),
            luadict_con_fields: std::collections::HashMap::new(),
            luadict_field_key: std::collections::HashMap::new(),
            luadict_type_fields: std::collections::HashMap::new(),
            luadict_enum_tag: std::collections::HashMap::new(),
            fn_table: std::collections::HashMap::new(),
            local_vars: std::collections::HashSet::new(),
            local_count: 0,
            var_slots: std::collections::HashMap::new(),
            var_slots_next: 0,
            var_table_emitted: false,
            demand_info: DemandInfo {
                strict_params: std::collections::HashMap::new(),
                rows: crate::demand::Rows::default(),
            },
            local_strict_params: std::collections::HashMap::new(),
            local_demand_rows: std::collections::HashMap::new(),
            cur_result_demand: crate::demand::Demand::Head,
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
    /// Carry a sub-generator's depth-guard error (if any) into this
    /// generator, so `generate` sees it no matter where it fired.
    fn absorb_sub_error(&mut self, sub: &mut CodeGen) {
        if self.depth_error.is_none() {
            self.depth_error = sub.depth_error.take();
        }
    }

    fn new_sub(&self) -> CodeGen {
        let mut sub = CodeGen::new();
        sub.constructors = self.constructors.clone();
        sub.newtypes = self.newtypes.clone();
        sub.fn_table = self.fn_table.clone();
        sub.concrete_vars = self.concrete_vars.clone();
        sub.record_accessors = self.record_accessors.clone();
        sub.luadict_con_fields = self.luadict_con_fields.clone();
        sub.luadict_field_key = self.luadict_field_key.clone();
        sub.luadict_type_fields = self.luadict_type_fields.clone();
        sub.luadict_enum_tag = self.luadict_enum_tag.clone();
        sub.top_level_names = self.top_level_names.clone();
        sub.local_vars = self.local_vars.clone();
        // Scoped local-function rows apply to everything emitted within the
        // same clause, including guard conditions and bodies routed through
        // a sub-generator.
        sub.local_strict_params = self.local_strict_params.clone();
        sub.local_demand_rows = self.local_demand_rows.clone();
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
}

/// Generate the Lua module. `Err` carries the codegen depth-guard
/// diagnostic (see `CodeGen::gen_expr`) — the only error this pass can
/// produce; everything else was rejected by earlier passes.
pub fn generate(module: &TModule, embed_source: Option<(EmbedMode, &str)>) -> Result<String, String> {
    let mut cg = CodeGen::new();
    cg.embed_var_export = matches!(embed_source, Some((EmbedMode::Var, _)));
    cg.demand_info = crate::demand::analyze(module);
    // Generate the program body first so we can see which runtime-prelude
    // functions it actually references, then prepend only those (transitively).
    cg.generate_module(module);
    if let Some(msg) = cg.depth_error {
        return Err(msg);
    }
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
    Ok(out)
}
