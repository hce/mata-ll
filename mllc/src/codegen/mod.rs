//! Lua code generation: turns a typed, monomorphized TIR module into a single
//! self-contained Lua source file.
//!
//! `generate` is the crate-facing entry point and the only public item.
//! Emission is AST-based: the generators build a `lua::Stmt`/`lua::Expr`
//! tree (lua.rs) and the tree is printed once at the end — no generator
//! writes output text directly, so statement well-formedness and grouping
//! are carried by structure, not re-proven per emission site. The pipeline:
//!
//! 1. `crate::demand::analyze` computes per-function parameter strictness and
//!    result demand; the result seeds `CodeGen::demand_info`.
//! 2. `module_stmts` (module.rs) registers data types, runs the
//!    whole-program call-site and inlining analyses (analysis.rs), and builds
//!    the program body as one statement list: constructors, functions,
//!    exports. `generate` prints it via `lua::Block::render`.
//! 3. `ondemand_prelude` (runtime.rs) scans the printed body and prepends only
//!    the runtime-prelude definitions it transitively references.
//! 4. When source embedding is requested, `embed::embed_block` places the
//!    embedded source block above everything else.
//!
//! The only error this pass can produce is the expression-depth diagnostic
//! (see `expr_ast` in expr.rs); everything else was rejected by earlier
//! passes.
//!
//! This file holds the `CodeGen` state struct and its basic plumbing: name
//! resolution (`lua_ref`), local declaration and the `_v` spill table for
//! Lua's 200-local limit (`declare_local_parts` / `LocalDecl`), and
//! sub-generator management (`new_sub` / `absorb_sub_error`).
//!
//! Child modules:
//! - lua.rs — the Lua AST the generators build, and its printer
//! - module.rs — module body layout: data-type registration, constructors,
//!   forward declarations, exports (`module_stmts`)
//! - function.rs — top-level functions, clauses, where-binding groups
//!   (`function_stmts`, `where_binds_stmts`)
//! - pattern.rs — pattern-match compilation (`pattern_match_block`)
//! - expr.rs — the main expression walk (`expr_ast` / `expr_ast_inner`)
//! - thunks.rs — eager-vs-thunk decisions for arguments, callees, forcing
//! - strictness.rs — cheapness and demand predicates behind those decisions
//! - action.rs — IO/ST actions: bind chains (`bind_chain_block`), pure
//!   boxes, the two runners (`action_run_ast`)
//! - inline.rs — substitution-based inlining twins of the builder paths
//! - analysis.rs — whole-program call-site and inline-candidate analyses
//! - ffi.rs — FFI marshalling and type-directed boundary decoding
//! - names.rs — Lua identifier, keyword and string-literal helpers
//! - opt.rs — AST optimization passes run on the finished statement list
//! - util.rs — type- and TIR-shape helpers shared across the module
//! - runtime.rs — the runtime prelude and its on-demand chunk selection

use crate::demand::DemandInfo;
use crate::embed::{self, EmbedMode};
use crate::tir::*;
use crate::types::Ty;

mod action;
mod analysis;
mod annot;
mod expr;
mod ffi;
mod function;
mod inline;
mod ioloop;
mod lua;
mod module;
mod names;
mod opt;
mod pattern;
mod performloop;
mod runtime;
mod strictness;
mod tailloop;
mod thunks;
mod util;

pub(crate) use names::is_lua_keyword;
use runtime::ondemand_prelude;

/// A freshly declared local's assignment target: a real `local name`
/// declaration under Lua's per-function local limit, or a `_v[N]` slot in
/// the spill table over it (the slot exists implicitly; assignment is plain).
/// See `CodeGen::declare_local_parts`.
enum LocalDecl {
    Fresh(String),
    Slot(String),
}

impl LocalDecl {
    /// The statement that binds `rhs` to this declaration.
    fn stmt(self, rhs: lua::Expr) -> lua::Stmt {
        match self {
            LocalDecl::Fresh(n) => lua::Stmt::Local(vec![n], Some(rhs)),
            LocalDecl::Slot(lv) => lua::Stmt::Assign(lv, rhs),
        }
    }
}

/// LuaDict data types keyed by type name: `(type_vars, [(field_name, field_ty)])`.
type LuaDictTypeFields = std::collections::HashMap<String, (Vec<String>, Vec<(String, Ty)>)>;

/// Tracks constructor info for code generation.
struct CodeGen {
    /// Current `expr_ast` recursion depth, bounded by
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
    /// Maps function name to (param_names, body, per-param occurrence counts).
    /// The counts are count_name_occurrences over the body: a parameter that
    /// would re-emit its argument more than once (or under a lambda) only
    /// admits trivial arguments at the call site — substituting anything
    /// else there would duplicate work GHC's inliner never duplicates
    /// (see the inline gate in expr.rs).
    inline_fns: std::collections::HashMap<String, (Vec<String>, TExpr, Vec<usize>)>,
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
    luadict_type_fields: LuaDictTypeFields,
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
    /// by where_binds_stmts when a clause's where scope opens and restored at
    /// clause-scope exit (function_stmts / pattern_match_block*), so a row
    /// never outlives its lexical scope or leaks into a sibling clause.
    local_strict_params: std::collections::HashMap<String, Vec<bool>>,
    /// Structured twin of `local_strict_params`: the demand rows of the
    /// where-bound local functions currently in scope (see
    /// demand::local_fn_rows), threaded into demanded_map /
    /// demanded_map_guards so a binding whose value is demanded THROUGH a
    /// call to a where-local counts as demanded. Installed by
    /// where_binds_stmts (via clause_local_rows, which shadows every
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
    /// Set while emitting the body of a DIRECT-PERFORM IO function — the
    /// single-clause simple-pattern IO arm and the nullary IO-value arm of
    /// function_stmts, where the emitted Lua function's body IS the action —
    /// to the function's source name and saturating argument count. A tail
    /// terminal that is a saturated call to this name returns bare
    /// (`return self(...)`, Lua's tail-call form) instead of riding the
    /// forwarding runner's argument position; see action_run_ast /
    /// is_direct_perform_self_call. `None` everywhere else: multi-clause and
    /// ST emissions build actions their caller's runner performs — a bare
    /// self call there would return an unperformed value.
    direct_perform_self: Option<(String, usize)>,
    /// Source embedding in `EmbedMode::Var`: the emitted file starts with a
    /// `local __SOURCE_CODE = …` binding (see embed.rs), and the module's
    /// return table must export it — even when there are no other exports.
    embed_var_export: bool,
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
            direct_perform_self: None,
            embed_var_export: false,
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

    /// Bind `rhs` to a top-level name — its
    /// `__mll_fn[N]` slot when forward-declared, a fresh `local` otherwise.
    fn var_decl_stmt(&self, lua_name: &str, rhs: lua::Expr) -> lua::Stmt {
        if let Some(&slot) = self.fn_table.get(lua_name) {
            lua::Stmt::Assign(format!("__mll_fn[{}]", slot), rhs)
        } else {
            lua::Stmt::Local(vec![lua_name.to_string()], Some(rhs))
        }
    }

    /// Lua's per-function local variable limit.
    const LOCAL_LIMIT: usize = 180;

    /// Declare a local variable for a block-building emission path. Returns
    /// an optional statement that must precede the declaration (the one-time
    /// `local _v = {}` spill-table setup) and the declaration itself: a fresh
    /// `local name` under the limit, a `_v[N]` slot assignment over it.
    /// Also registers the name in `local_vars` (and `var_slots` if tabled).
    fn declare_local_parts(&mut self, name: &str) -> (Option<lua::Stmt>, LocalDecl) {
        self.local_vars.insert(name.to_string());
        self.local_count += 1;
        if self.local_count > Self::LOCAL_LIMIT {
            let pre = if !self.var_table_emitted {
                // The _v table declaration (this itself is one local)
                self.var_table_emitted = true;
                Some(lua::Stmt::Local(vec!["_v".into()], Some(lua::Expr::Table(vec![]))))
            } else {
                None
            };
            self.var_slots_next += 1;
            self.var_slots.insert(name.to_string(), self.var_slots_next);
            (pre, LocalDecl::Slot(format!("_v[{}]", self.var_slots_next)))
        } else {
            (None, LocalDecl::Fresh(name.to_string()))
        }
    }

    /// Declare a local without a value (forward declaration for later
    /// assignment) in a block-building path. Returns the statements to place
    /// at the declaration point: the optional spill-table setup plus the
    /// `local name` line — or nothing beyond the setup for a `_v[N]` slot,
    /// which exists implicitly (nil).
    /// Only used when the variable needs to exist before its value is known
    /// (e.g., `local x; if ... then x = a else x = b end`).
    fn declare_local_fwd_stmts(&mut self, name: &str) -> Vec<lua::Stmt> {
        let (pre, decl) = self.declare_local_parts(name);
        let mut out = Vec::new();
        if let Some(s) = pre {
            out.push(s);
        }
        if let LocalDecl::Fresh(n) = decl {
            out.push(lua::Stmt::Local(vec![n], None));
        }
        out
    }

    /// Get the Lua lvalue for an already-declared local (for assignment after fwd decl).
    fn local_lvalue(&self, name: &str) -> String {
        if let Some(&idx) = self.var_slots.get(name) {
            format!("_v[{}]", idx)
        } else {
            name.to_string()
        }
    }

    /// Carry a sub-generator's depth-guard error (if any) into this
    /// generator, so `generate` sees it no matter where it fired.
    fn absorb_sub_error(&mut self, sub: &mut CodeGen) {
        if self.depth_error.is_none() {
            self.depth_error = sub.depth_error.take();
        }
    }

    /// Create a sub-CodeGen that shares this generator's lookup tables but
    /// whose state changes are discarded (used for guard conditions — see
    /// `guard_cond_ast` in pattern.rs).
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


    fn constructor_info(&self, name: &str) -> Option<(usize, usize, bool)> {
        for (cn, _, idx, total, is_enum) in &self.constructors {
            if cn == name { return Some((*idx, *total, *is_enum)); }
        }
        None
    }
}

/// Generate the Lua module. `Err` carries the codegen depth-guard
/// diagnostic (see `CodeGen::expr_ast`) — the only error this pass can
/// produce; everything else was rejected by earlier passes.
/// `opt_disable`: explicit optimization-pass skip list (comma-separated,
/// the `MLL_OPT_DISABLE` vocabulary); `None` reads the environment
/// variable — see `CompileOptions::disable_opt_passes`.
pub fn generate(
    module: &TModule,
    embed_source: Option<(EmbedMode, &str)>,
    opt_disable: Option<&str>,
) -> Result<String, String> {
    let mut cg = CodeGen::new();
    cg.embed_var_export = matches!(embed_source, Some((EmbedMode::Var, _)));
    cg.demand_info = crate::demand::analyze(module);
    // Build the program body first (as one Lua statement list, then printed)
    // so we can see which runtime-prelude functions it actually references,
    // then prepend only those (transitively).
    let mut stmts = cg.module_stmts(module);
    if let Some(msg) = cg.depth_error {
        return Err(msg);
    }
    opt::run(&mut stmts, opt_disable);
    let mut body = String::new();
    lua::Block(stmts).render(0, &mut body);
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

/// Test-only support behind `verify::check_stamps`: rebuild the module body,
/// run the optimization passes exactly as `generate` would, and return the
/// stamp-refutation violations (see `annot::Engine::refute`). Regenerates
/// rather than threading a flag through `generate` — the extra codegen run
/// only happens on the test entry.
pub(crate) fn stamp_violations(module: &TModule) -> Vec<String> {
    let mut cg = CodeGen::new();
    cg.demand_info = crate::demand::analyze(module);
    let mut stmts = cg.module_stmts(module);
    if cg.depth_error.is_some() {
        // `generate` reports this as its own diagnostic; nothing to refute.
        return Vec::new();
    }
    opt::run_refuted(&mut stmts)
}
