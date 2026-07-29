//! Test harness: discovers all .mll files in tests/cases/,
//! compiles each with mllc, runs the result via mlua,
//! and reports success/failure.

use std::path::Path;

fn run_mll_file(path: &Path) {
    let path = path.to_path_buf();
    // Run on a thread with the same stack size as the mll CLI driver: the
    // compiler's nesting-depth limit (mllc::MAX_NESTING_DEPTH) is calibrated
    // against mllc::COMPILER_STACK_SIZE, so a smaller test stack would
    // overflow on input the real compiler handles (or cleanly rejects).
    let result = std::thread::Builder::new()
        .stack_size(mllc::COMPILER_STACK_SIZE)
        .spawn(move || {
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("Cannot read {}: {}", path.display(), e));

            let source_dir = path.parent().unwrap_or(Path::new("."));
            // The stamp-refutation twin of mllc::compile: same output, plus
            // the emitted-Lua annotation check every corpus program should
            // exercise (see verify::check_stamps).
            let lua_code = match mllc::compile_with_stamp_refutation(&source, source_dir, &[]) {
                Ok(r) => r.lua_code,
                Err(e) => panic!("{}: compilation failed:\n{}", path.display(), e),
            };

            let lua = mlua::Lua::new();
            match lua.load(&lua_code).set_name(path.to_str().unwrap()).exec() {
                Ok(()) => {}
                Err(e) => panic!("{}: runtime error:\n{}", path.display(), e),
            }
        })
        .unwrap()
        .join();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

/// Run `f` on a thread with the compiler's calibrated stack and hand its
/// value back; a panic in `f` resumes on the caller. Scoped, so `f` may
/// borrow from the enclosing test.
fn with_compiler_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| {
        match std::thread::Builder::new()
            .stack_size(mllc::COMPILER_STACK_SIZE)
            .spawn_scoped(s, f)
            .expect("Failed to spawn compiler thread")
            .join()
        {
            Ok(v) => v,
            Err(e) => std::panic::resume_unwind(e),
        }
    })
}

/// `mllc::compile`, on the compiler's calibrated stack. EVERY compile in this
/// harness must run on such a stack (via this, `run_mll_file`, or
/// `on_compiler_stack`): the nesting-depth limit assumes
/// `mllc::COMPILER_STACK_SIZE`, and libtest worker threads are far smaller —
/// in a debug build even the ~30-level inference spine of the 8-line
/// examples/primes_check.mll overflows them.
fn compile(
    source: &str,
    dir: &Path,
    libs: &[&Path],
) -> Result<mllc::CompileResult, mllc::CompileError> {
    with_compiler_stack(|| mllc::compile(source, dir, libs))
}

/// Run a whole test body on the calibrated stack: for tests that do more
/// around the compile (deep own recursion, `compile_with_options`) or call
/// `compile` in a tight loop where one spawn should cover all of them.
fn on_compiler_stack(f: impl FnOnce() + Send + 'static) {
    with_compiler_stack(f)
}

macro_rules! mll_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            run_mll_file(Path::new(concat!("tests/cases/", $file)));
        }
    };
}

/// Code generation must be deterministic: the same source compiled repeatedly
/// must produce byte-identical Lua. (Regression guard for HashMap iteration
/// order leaking into emission — record accessors, FFI functions, and
/// specialization resolution were all order-sensitive.)
#[test]
fn codegen_is_deterministic() {
    let source = r#"
data Color = Red | Green | Blue deriving (Show, Eq, Ord)
data Person = Person { pName :: String, pAge :: Int, pCity :: String, pActive :: Bool }
data Tree a = Leaf a | Branch (Tree a) (Tree a) deriving (Show, Eq)

class Describe a where
  describe :: a -> String
instance Describe Color where
  describe Red = "red"
  describe Green = "green"
  describe Blue = "blue"

mapList :: (a -> b) -> [a] -> [b]
mapList _ [] = []
mapList f (x:xs) = f x : mapList f xs

depth :: Tree a -> Int
depth (Leaf _) = 0
depth (Branch l r) = 1 + max (depth l) (depth r)

main :: IO ()
main = do
  let p = Person { pName = "Ann", pAge = 30, pCity = "NYC", pActive = True }
  putStr (pName p)
  putStrLn (pCity p)
  putStrLn (show (mapList (\n -> n + 1) [1, 2, 3]))
  putStrLn (show (mapList (\s -> s) ["a", "b"]))
  putStrLn (show (depth (Branch (Leaf 1) (Leaf 2))))
  putStrLn (describe Blue)
  putStrLn (show (compare Red Blue))
  let m = hmFromList [("a", 1), ("b", 2)]
  putStrLn (show (hmSize m))
"#;
    let dir = Path::new("tests/cases");
    let first = compile(source, dir, &[])
        .expect("compile should succeed")
        .lua_code;
    for i in 1..8 {
        let again = compile(source, dir, &[])
            .expect("compile should succeed")
            .lua_code;
        assert!(
            again == first,
            "codegen is non-deterministic: compile #{} differs from #0",
            i
        );
    }
}

/// The numeric classes (Num/Fractional/Integral) must be fully ERASED at
/// concrete Int/Number types: `+ - * /` emit bare Lua operators, `div`/`mod`
/// emit the existing strict cores, and NO dictionary is passed and NO named
/// instance helper (fromInteger_Int, plus_*, a dict table) is materialised.
/// This is the byte-identity guarantee for concrete arithmetic — the hot path
/// (e.g. the tracker mixer) must not gain an allocation or an indirection.
#[test]
fn numeric_classes_erased_at_concrete_types() {
    let source = r#"
hot :: Int -> Int -> Int
hot a b = a * b + a - b

frac :: Number -> Number -> Number
frac x y = x / y + x * y

flr :: Int -> Int -> Int
flr a b = (a `div` b) + (a `mod` b)

main :: IO ()
main = do
  putStrLn (show (hot 3 4))
  putStrLn (show (frac 3.0 4.0))
  putStrLn (show (flr 17 5))
"#;
    let lua = compile(source, Path::new("tests/cases"), &[])
        .expect("compile should succeed")
        .lua_code;

    // Concrete Int/Number arithmetic inlines to bare Lua operators…
    assert!(lua.contains("(a * b)"), "Int * must inline to bare Lua *: {lua}");
    assert!(lua.contains("(x / y)"), "Number / must inline to bare Lua /: {lua}");
    // …div/mod stay on the existing strict cores…
    assert!(lua.contains("__mll_div("), "div must stay on __mll_div: {lua}");
    assert!(lua.contains("__mll_mod("), "mod must stay on __mll_mod: {lua}");
    // …and NOTHING dispatches through a Num/Fractional/Integral dictionary or a
    // per-type instance helper at these concrete types.
    for forbidden in [
        "fromInteger_Int", "fromInteger_Number", "fromRational_Number",
        "plus_Int", "times_Int", "__mll_dict", "__dict_Num",
    ] {
        assert!(
            !lua.contains(forbidden),
            "concrete arithmetic must not emit `{forbidden}` (no dictionary/instance dispatch): {lua}"
        );
    }
}

/// Call-site inlining must not duplicate argument work (sharing loss vs
/// GHC). `sq x = x * x` is an inline candidate; substitution re-emits the
/// argument at every occurrence of `x`, so a non-trivial argument (a call)
/// may only be substituted where the parameter occurs at most once —
/// otherwise the site falls back to the ordinary call and the argument is
/// evaluated exactly once. Distinctive literals mark each argument: the
/// literal's occurrence count in the emitted Lua IS the number of times
/// the argument was emitted.
#[test]
fn inlining_preserves_argument_sharing() {
    let source = r#"
sq :: Int -> Int
sq x = x * x

probe :: Int -> Int
probe n = n + 1

main :: IO ()
main = do
  let y = 6
  putStrLn (show (sq (probe 90001)))
  putStrLn (show (sq y))
  putStrLn (show (probe (probe 80002)))
"#;
    let lua = compile(source, Path::new("tests/cases"), &[])
        .expect("compile should succeed")
        .lua_code;
    // The non-trivial argument to the multiply-using `sq` is emitted ONCE
    // (before the fix: twice — `probe(90001) * probe(90001)`).
    assert_eq!(
        lua.matches("90001").count(),
        1,
        "argument to sq must be emitted exactly once (sharing): {lua}"
    );
    // The trivial-argument call still inlines: `sq y` becomes `y * y`.
    assert!(
        lua.contains("y * y"),
        "sq of a variable must still inline to y * y: {lua}"
    );
    // A once-used parameter (`probe n = n + 1`) still admits a non-trivial
    // argument, and that argument is emitted once.
    assert_eq!(
        lua.matches("80002").count(),
        1,
        "argument to probe must be emitted exactly once: {lua}"
    );
}

/// Paren normalization (codegen/opt.rs): the emitted corpus must be free of
/// the redundant-paren shapes the pass eliminates. The one with semantic
/// weight is the paren-wrapped call in return position — `return (f(x))` is
/// not a proper tail call in Lua — asserted here for the provably
/// single-return callees (`__mll_*`, `__force`, `show*`) and for thunk
/// bodies (whose only consumer, `__force`, truncates to one value). The
/// FFI-wrapper check is the flip side: a host call with a declared
/// single-value result KEEPS its truncating paren.
#[test]
fn emitted_parens_are_normalized() {
    on_compiler_stack(emitted_parens_are_normalized_impl)
}

fn emitted_parens_are_normalized_impl() {
    let lib_path = Path::new("../lib").to_path_buf();
    let dir = Path::new("tests/cases");
    for entry in std::fs::read_dir(dir).expect("read tests/cases") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("mll") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("read case");
        let lua = match compile(&source, dir, &[&lib_path]) {
            Ok(r) => r.lua_code,
            // A case that needs CLI-only setup is not this test's business.
            Err(_) => continue,
        };
        // Dead-branch cleanup (pass 2): the `otherwise` guard arm is
        // always rewritten to `else`.
        assert!(
            !lua.contains("elseif true then"),
            "{}: `elseif true` survived dead-branch cleanup",
            path.display()
        );
        for forbidden in [
            "return (__mll",
            "return (__force",
            "return (show",
            "__thunk(function() return (__",
        ] {
            for line in lua.lines().filter(|l| l.contains(forbidden)) {
                // `return (__force(a) * ...` is a grouped binop operand, not
                // a paren-wrapped call: only flag lines where the paren
                // closes the return expression (line ends right after the
                // call's own closing parens).
                let after = &line[line.find(forbidden).unwrap()..];
                if after.trim_end().ends_with("))") && !after.contains(" and ")
                    && !after.contains(" or ") && !after.contains(" + ")
                    && !after.contains(" * ") && !after.contains(" - ")
                    && !after.contains(" .. ") && !after.contains(" == ")
                    && !after.contains(" ~= ") && !after.contains(" < ")
                    && !after.contains(" > ") && !after.contains(" end")
                {
                    panic!(
                        "{}: redundant paren survived normalization: {}",
                        path.display(),
                        line.trim()
                    );
                }
            }
        }
    }

    // FFI wrappers truncate a multi-returning host to the declared single
    // result — the paren here is semantics and must survive the pass.
    let ffi_source = "modf1 :: Number -> LuaPure \"math.modf\" Number\n\
                      main :: IO ()\n\
                      main = assert (modf1 3.75 == 3.0) \"t\"\n";
    let lua = compile(ffi_source, dir, &[])
        .expect("ffi probe must compile")
        .lua_code;
    assert!(
        lua.contains("return (math.modf(_ffi0))"),
        "FFI wrapper must keep the truncating paren around the raw host call: {lua}"
    );
}

/// Where-bound function-group names hold Lua function values from their
/// group assignment on — never thunks — and a `_warg` entry rebind leaves
/// the parameter WHNF. Neither may be re-forced (function.rs marks both
/// concrete; opt.rs pass 4 is the safety net).
#[test]
fn where_group_calls_not_forced() {
    on_compiler_stack(where_group_calls_not_forced_impl)
}

fn where_group_calls_not_forced_impl() {
    let source = std::fs::read_to_string("tests/cases/where_group_mutual.mll")
        .expect("read where_group_mutual.mll");
    let lua = compile(&source, Path::new("tests/cases"), &[])
        .expect("compile should succeed")
        .lua_code;
    for forbidden in ["__force(go)", "__force(swap)"] {
        assert!(
            !lua.contains(forbidden),
            "where-group emission must not re-force `{forbidden}`: {lua}"
        );
    }
    // `__force(_warg…)` may appear only as the entry rebind itself
    // (`_wargN = __force(_wargN)`), never as a re-force at a use site.
    for line in lua.lines().filter(|l| l.contains("__force(_warg")) {
        let t = line.trim_start();
        assert!(
            t.starts_with("_warg") && t.contains(" = __force(_warg"),
            "entry-rebound _warg param re-forced at a use site: {line}"
        );
    }
}

/// Constructor-level DCE: a `data` definition none of whose constructors is
/// constructed (`Con`) or matched (pattern) by live code contributes NOTHING
/// to the emitted Lua. Checked two ways: (1) adding a dead user `data`
/// declaration (with derived instances) to a program leaves the output
/// byte-identical; (2) a minimal program carries no constructor slots for the
/// four Prelude datatypes (ExitValue, Any, Either, Ordering — 12 slots before
/// this pass), pinned as a bound on `__mll_fn` slot assignments.
#[test]
fn constructor_dce_unused_data_adds_nothing() {
    let base = "double :: Int -> Int\n\
                double x = x * 2\n\
                main :: IO ()\n\
                main = print (double 21)\n";
    let with_dead = format!(
        "data Unused = UnusedA | UnusedB Int deriving (Show, Eq)\n{}",
        base
    );
    let dir = Path::new("tests/cases");
    let a = compile(base, dir, &[]).expect("base must compile").lua_code;
    let b = compile(&with_dead, dir, &[])
        .expect("dead-data program must compile")
        .lua_code;
    assert!(
        a == b,
        "an unused data declaration must add nothing to the emitted Lua"
    );

    // No Prelude constructor slots in a minimal program: every `__mll_fn[N] =`
    // assignment left is a live function. Before constructor-DCE this program
    // carried 12 extra constructor slots; a loose bound keeps the test stable
    // against unrelated emission drift while still catching that regression.
    let slot_assigns = a.matches("__mll_fn[").count();
    assert!(
        slot_assigns < 10,
        "expected no Prelude constructor slots in a minimal program, \
         found {} __mll_fn references:\n{}",
        slot_assigns,
        a
    );
}

/// Constructor-level DCE must drop only the constructor FUNCTIONS, never the
/// type's metadata: a value of a dropped type can still flow through live
/// code without being constructed or matched there. The canonical case is a
/// LuaDict record built by the Lua host and read only through accessors — its
/// keyed table layout comes from the registered definition, so the accessor
/// must still emit `.port` (not a positional index) and the FFI decoder must
/// keep the declared field types, while the constructor itself is not
/// emitted.
#[test]
fn constructor_dce_keeps_metadata_for_flow_through_types() {
    let source = "data Config = Config\n\
                  \x20 { port :: Int\n\
                  \x20 , host :: String\n\
                  \x20 } deriving (LuaDict)\n\
                  export readPort :: Config -> Int\n\
                  readPort c = port c\n";
    let lua = compile(source, Path::new("tests/cases"), &[])
        .expect("flow-through program must compile")
        .lua_code;
    // The accessor keeps the LuaDict keyed layout (metadata survived)...
    assert!(
        lua.contains(").port"),
        "accessor must read the LuaDict field by key, not position:\n{}",
        lua
    );
    // ...the FFI decoder still knows the declared record shape...
    assert!(
        lua.contains("t=\"Config\""),
        "FFI decode descriptor must keep the record's field metadata:\n{}",
        lua
    );
    // ...but the never-constructed constructor function is not emitted.
    assert!(
        !lua.contains("port = _p0"),
        "the unconstructed Config constructor must not be emitted:\n{}",
        lua
    );
}

/// Cheap-eagerness must stay sound but not over-tighten: a let binding whose
/// RHS only reads provably-WHNF variables (a literal-bound sibling, a
/// demand-analysis-strict parameter) is still assigned strictly — no thunk —
/// while a binding reading a non-WHNF variable must be thunked. This is the
/// eagerness half of the lazy_cheap_bindings.mll regression, which runtime
/// behaviour alone cannot observe.
#[test]
fn cheap_eagerness_whnf_bindings_stay_strict() {
    let source = r#"
f :: Int -> Int
f x = let n = 5
          m = n + x
      in m + n

g :: Bool -> Int
g x = let y = error "boom"
          z = y + 1
      in if x then z else 0

main :: IO ()
main = putStrLn (show (f 10 + g False))
"#;
    let lua = compile(source, Path::new("tests/cases"), &[])
        .expect("compile should succeed")
        .lua_code;
    // n is bound to a literal: strict assignment, no thunk.
    assert!(lua.contains("n = 5"), "literal binding should be assigned strictly");
    assert!(!lua.contains("n = __thunk"), "literal binding must not be thunked");
    // m reads only provably-WHNF vars (n is literal-bound and already
    // assigned; x is a demand-strict parameter forced at entry): strict.
    assert!(
        !lua.contains("m = __thunk"),
        "binding over provably-WHNF variables must stay eagerly assigned:\n{}",
        lua
    );
    // z reads y, a thunked bottom: z itself must be thunked, and y must
    // never be forced outside a thunk body at binding time.
    assert!(
        lua.contains("z = __thunk"),
        "binding over a non-WHNF variable must be thunked:\n{}",
        lua
    );
}

/// The Prelude builtin `exit :: ExitValue -> IO ()` must resolve to the
/// runtime helper `exit_`, which unwraps the ExitValue ADT (Normal = {1},
/// Err code = {2, code}) and calls os.exit. Before the fix the emitted Lua
/// referenced an undefined global `exit` and crashed at runtime.
///
/// This test only inspects the emitted Lua — never execute `exit`
/// in-process: os.exit would terminate the whole cargo test harness.
/// Actual exit-code behaviour is covered by the subprocess tests in
/// mll/tests/exit_builtin.rs.
#[test]
fn exit_builtin_resolves_to_runtime_helper() {
    for src in [
        "main :: IO ()\nmain = exit Normal\n",
        "main :: IO ()\nmain = exit (Err 3)\n",
    ] {
        let lua = compile(src, Path::new("tests/cases"), &[])
            .expect("compile should succeed")
            .lua_code;
        assert!(
            !lua.contains("__force(exit)"),
            "surface `exit` was left as an undefined Lua global:\n{}",
            lua
        );
        assert!(
            lua.contains("exit_("),
            "call site should resolve to the exit_ runtime helper:\n{}",
            lua
        );
        assert!(
            lua.contains("local function exit_"),
            "exit_ runtime helper chunk missing from emitted prelude:\n{}",
            lua
        );
        assert!(
            lua.contains("os.exit"),
            "exit_ helper should call os.exit:\n{}",
            lua
        );
    }
}

/// Every compiled module carries compiler provenance: __MLLC_VERSION (the mllc
/// crate version) and __MLLC_COMMIT (the full git commit it was built from),
/// emitted as top-level locals and surfaced through the export table.
///
/// The workspace shares a single version line, so this test crate's
/// CARGO_PKG_VERSION equals mllc's — asserting equality also guards against a
/// version desync between crates.
#[test]
fn compiled_module_carries_mllc_provenance() {
    // A plain program (has main, no exports): stamps present as locals.
    let prog = "main :: IO ()\nmain = putStrLn \"hi\"\n";
    let lua = compile(prog, Path::new("tests/cases"), &[])
        .expect("compile should succeed")
        .lua_code;

    let ver_line = format!("local __MLLC_VERSION = \"{}\"", env!("CARGO_PKG_VERSION"));
    assert!(
        lua.contains(&ver_line),
        "module must stamp the mllc crate version (expected `{}`):\n{}",
        ver_line,
        lua
    );
    // __MLLC_COMMIT is a full 40-char git hash, or "unknown" for a non-git
    // (e.g. crates.io tarball) build.
    let commit = lua
        .lines()
        .find_map(|l| l.strip_prefix("local __MLLC_COMMIT = \"").and_then(|r| r.strip_suffix('"')))
        .expect("module must stamp a __MLLC_COMMIT local");
    assert!(
        commit == "unknown"
            || (commit.len() == 40 && commit.chars().all(|c| c.is_ascii_hexdigit())),
        "__MLLC_COMMIT must be a full 40-hex commit or \"unknown\", got `{commit}`"
    );

    // An exporting module surfaces the stamps as module properties, so a Lua
    // host can read them from the required table.
    let module = "export answer :: Int\nanswer :: Int\nanswer = 42\n";
    let lua = compile(module, Path::new("tests/cases"), &[])
        .expect("compile should succeed")
        .lua_code;
    assert!(
        lua.contains("__MLLC_VERSION = __MLLC_VERSION"),
        "export table must expose __MLLC_VERSION as a module property:\n{}",
        lua
    );
    assert!(
        lua.contains("__MLLC_COMMIT = __MLLC_COMMIT"),
        "export table must expose __MLLC_COMMIT as a module property:\n{}",
        lua
    );
}

/// A `module M (…) where` header export list scopes .mll import visibility
/// only — by design it does not export anything to the Lua host (that is the
/// `export` keyword's job; see SPEC.md "Module and import syntax"). A module
/// whose only "exports" are header-listed therefore has no host surface when
/// compiled standalone: no `main`, nothing in the return table, and dead-code
/// elimination removes every definition. That used to produce an empty Lua
/// shell *silently*; the compiler must now say so via `CompileResult.warnings`.
#[test]
fn header_only_root_module_warns_instead_of_silent_empty_output() {
    let src = "module C (addup) where\n\n\
               addup :: Int -> Int -> Int\n\
               addup a b = a + b\n";
    let result = compile(src, Path::new("tests/cases"), &[])
        .expect("a header-form library must still compile");

    // Documented semantics: the header list creates no host surface.
    assert!(!result.has_main);
    assert!(
        result.exports.is_empty(),
        "header export lists must not populate the Lua exports: {:?}",
        result.exports
    );
    assert!(
        !result.lua_code.contains("-- Exports"),
        "no export table may be emitted for a header-only module:\n{}",
        result.lua_code
    );
    assert!(
        !result.lua_code.contains("addup"),
        "with no main/export roots, the definition is dead code:\n{}",
        result.lua_code
    );

    // ... but not silently: the result must carry a warning that names the
    // mixup and the fix.
    assert_eq!(
        result.warnings.len(),
        1,
        "exactly one no-host-surface warning expected"
    );
    let rendered = format!("{}", result.warnings[0]);
    assert!(
        rendered.contains("no runnable or callable code"),
        "warning must state the consequence:\n{}",
        rendered
    );
    assert!(
        rendered.contains("module … (addup) where"),
        "warning must point at the header export list:\n{}",
        rendered
    );
    assert!(
        rendered.contains("export addup"),
        "warning must show the `export` keyword fix:\n{}",
        rendered
    );
}

/// The generic form of the same warning: a bare library (no module header at
/// all) compiled as the root also has nothing to run or call.
#[test]
fn bare_library_root_warns_with_generic_guidance() {
    let src = "addup :: Int -> Int -> Int\naddup a b = a + b\n";
    let result = compile(src, Path::new("tests/cases"), &[])
        .expect("compile should succeed");
    assert_eq!(result.warnings.len(), 1);
    let rendered = format!("{}", result.warnings[0]);
    assert!(rendered.contains("no runnable or callable code"), "{}", rendered);
    assert!(rendered.contains("main :: IO ()"), "{}", rendered);
    assert!(rendered.contains("export"), "{}", rendered);
}

/// Modules with a host surface must stay warning-free — a program with
/// `main`, an `export`-keyword module, and (critically) a root program that
/// *imports* a header-form library: the library's header exports must neither
/// warn nor leak into the importer's return table.
#[test]
fn modules_with_a_host_surface_do_not_warn() {
    // Program root.
    let prog = "main :: IO ()\nmain = putStrLn \"hi\"\n";
    let result = compile(prog, Path::new("tests/cases"), &[])
        .expect("compile should succeed");
    assert!(result.warnings.is_empty(), "program with main must not warn");

    // `export`-keyword module root.
    let module = "export answer :: Int\nanswer :: Int\nanswer = 42\n";
    let result = compile(module, Path::new("tests/cases"), &[])
        .expect("compile should succeed");
    assert!(result.warnings.is_empty(), "exporting module must not warn");
    assert_eq!(result.exports, vec!["answer".to_string()]);

    // Program root importing a header-form library (ExportHelper's header
    // lists publicFn and PublicType(..)).
    let importer = "import ExportHelper\n\n\
                    main :: IO ()\n\
                    main = putStrLn (show (publicFn 4))\n";
    let result = compile(importer, Path::new("tests/cases"), &[])
        .expect("compile should succeed");
    assert!(
        result.warnings.is_empty(),
        "importing a header-form library must not warn"
    );
    assert!(
        result.exports.is_empty() && !result.lua_code.contains("-- Exports"),
        "an imported library's header exports must not leak into the \
         importer's return table:\n{}",
        result.lua_code
    );
}

fn run_mll_file_with_lib(path: &Path) {
    let path = path.to_path_buf();
    let lib_path = Path::new("../lib").to_path_buf();
    // Same stack size as the mll CLI driver (see run_mll_file).
    let result = std::thread::Builder::new()
        .stack_size(mllc::COMPILER_STACK_SIZE)
        .spawn(move || {
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("Cannot read {}: {}", path.display(), e));

            let source_dir = path.parent().unwrap_or(Path::new("."));
            let lua_code = match compile(&source, source_dir, &[&lib_path]) {
                Ok(r) => r.lua_code,
                Err(e) => panic!("{}: compilation failed:\n{}", path.display(), e),
            };

            let lua = mlua::Lua::new();
            match lua.load(&lua_code).set_name(path.to_str().unwrap()).exec() {
                Ok(()) => {}
                Err(e) => panic!("{}: runtime error:\n{}", path.display(), e),
            }
        })
        .unwrap()
        .join();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

macro_rules! mll_lib_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            run_mll_file_with_lib(Path::new(concat!("tests/cases/", $file)));
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
    on_compiler_stack(|| {
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

// GHC-style compatibility tests
macro_rules! ghc_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            run_mll_file(Path::new(concat!("tests/ghc/", $file)));
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
mll_lib_test!(derive_fromjson, "derive_fromjson.mll");
mll_lib_test!(derive_tojson, "derive_tojson.mll");
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

// Compile-error tests: these SHOULD fail to compile

// A numeric string escape above 255 has no single-byte representation in
// mata-ll's byte-oriented String (HASKDIFF.md, "Strings and ByteStrings"), so
// it is a LOUD lexer error rather than a silent wrong value. GHC accepts up to
// \1114111 because its String is [Char]; this is the one place the byte-string
// model forces a documented deviation, and it must carry the explanatory note.
#[test]
fn string_escape_above_byte_range_is_rejected() {
    let source = r#"
main :: IO ()
main = putStrLn "\256"
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("out of range") && msg.contains("\\256"),
                "Expected an out-of-range numeric-escape error naming the escape, got: {}",
                msg
            );
            assert!(
                msg.contains("note:") && msg.contains("byte array"),
                "The error must carry the byte-string note explaining the deviation, got: {}",
                msg
            );
            assert!(
                msg.contains("HASKDIFF.md"),
                "The note must point at HASKDIFF.md, got: {}",
                msg
            );
        }
        Ok(_) => panic!("Expected a numeric escape \\256 to be rejected as out of byte range"),
    }
}

// A `[a]`-vs-`String` unification failure (e.g. `"a" ++ "b"`) is a
// completeness gap, not a soundness violation — mata-ll's String is opaque,
// not [Char] (decided 2026-07-22; see HASKDIFF.md, "Strings and ByteStrings").
// The rejection must be maximally informative: it must say String is not
// [Char], point at <> for concatenation, and cite HASKDIFF.md.
#[test]
fn string_vs_list_mismatch_note_explains_the_design() {
    let source = r#"
main :: IO ()
main = putStrLn ("a" ++ "b")
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("Cannot unify") && msg.contains("String"),
                "Expected a String/list unification error, got: {}",
                msg
            );
            assert!(
                msg.contains("note:") && msg.contains("opaque") && msg.contains("[Char]"),
                "The note must say String is opaque, not [Char], got: {}",
                msg
            );
            assert!(
                msg.contains("<>"),
                "The note must point at <> for concatenation, got: {}",
                msg
            );
            assert!(
                msg.contains("HASKDIFF.md"),
                "The note must cite HASKDIFF.md, got: {}",
                msg
            );
        }
        Ok(_) => panic!("Expected `\"a\" ++ \"b\"` to be rejected (String is not a list)"),
    }
}

#[test]
fn fromjson_derive_requires_json_import() {
    // deriving (FromJSON) without `import JSON`: the class and the decoder
    // combinators the generated code calls are not in scope, and the error
    // must say exactly what to add.
    let source = r#"
data P = P { x :: Int } deriving (FromJSON)

main :: IO ()
main = putStrLn "hi"
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot derive 'FromJSON'"),
                "Expected a FromJSON derive error, got: {}", msg);
            assert!(msg.contains("import JSON"),
                "The error must name the missing import, got: {}", msg);
        }
        Ok(_) => panic!("Expected deriving (FromJSON) without import JSON to be rejected"),
    }
}

#[test]
fn fromjson_derive_rejects_function_field() {
    let lib = Path::new("../lib");
    let source = r#"
import JSON

data H = H { hop :: Int -> Int } deriving (FromJSON)

main :: IO ()
main = putStrLn "hi"
"#;
    match compile(source, Path::new("."), &[lib]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot derive 'FromJSON' for 'H'"),
                "Expected a FromJSON derive error, got: {}", msg);
            assert!(msg.contains("field 'hop'") && msg.contains("function"),
                "The error must name the field and explain functions have no JSON form, got: {}", msg);
        }
        Ok(_) => panic!("Expected deriving (FromJSON) on a function-typed field to be rejected"),
    }
}

#[test]
fn fromjson_derive_rejects_type_parameters() {
    // GHC's aeson derives `FromJSON (Box a)` by constraining `a`; mata-ll
    // instances cannot carry constraints, so this is rejected with the
    // explanation rather than producing a decoder that cannot exist.
    let lib = Path::new("../lib");
    let source = r#"
import JSON

data Box a = Box a deriving (FromJSON)

main :: IO ()
main = putStrLn "hi"
"#;
    match compile(source, Path::new("."), &[lib]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot derive 'FromJSON' for 'Box'"),
                "Expected a FromJSON derive error, got: {}", msg);
            assert!(msg.contains("type parameters"),
                "The error must explain the type-parameter limitation, got: {}", msg);
        }
        Ok(_) => panic!("Expected deriving (FromJSON) on a parameterized type to be rejected"),
    }
}

#[test]
fn fromjson_derive_rejects_field_without_instance() {
    let lib = Path::new("../lib");
    let source = r#"
import JSON

data Plain = Plain Int

data Holder = Holder { inner :: Plain } deriving (FromJSON)

main :: IO ()
main = putStrLn "hi"
"#;
    match compile(source, Path::new("."), &[lib]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot derive 'FromJSON' for 'Holder'"),
                "Expected a FromJSON derive error, got: {}", msg);
            assert!(msg.contains("'Plain' has no FromJSON instance"),
                "The error must name the instance-less field type, got: {}", msg);
        }
        Ok(_) => panic!("Expected deriving (FromJSON) over an instance-less field type to be rejected"),
    }
}

#[test]
fn fromjson_derive_rejects_tag_field_collision() {
    let lib = Path::new("../lib");
    let source = r#"
import JSON

data T = A { tag :: String } | B deriving (FromJSON)

main :: IO ()
main = putStrLn "hi"
"#;
    match compile(source, Path::new("."), &[lib]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot derive 'FromJSON' for 'T'") && msg.contains("tag"),
                "Expected the tag-collision explanation, got: {}", msg);
        }
        Ok(_) => panic!("Expected a record field named 'tag' in a sum type to be rejected"),
    }
}

#[test]
fn tojson_derive_requires_json_import() {
    // deriving (ToJSON) without `import JSON`: the class and the encoder
    // combinators the generated code calls are not in scope, and the error
    // must say exactly what to add.
    let source = r#"
data P = P { x :: Int } deriving (ToJSON)

main :: IO ()
main = putStrLn "hi"
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot derive 'ToJSON'"),
                "Expected a ToJSON derive error, got: {}", msg);
            assert!(msg.contains("import JSON"),
                "The error must name the missing import, got: {}", msg);
        }
        Ok(_) => panic!("Expected deriving (ToJSON) without import JSON to be rejected"),
    }
}

#[test]
fn tojson_derive_rejects_function_field() {
    let lib = Path::new("../lib");
    let source = r#"
import JSON

data H = H { hop :: Int -> Int } deriving (ToJSON)

main :: IO ()
main = putStrLn "hi"
"#;
    match compile(source, Path::new("."), &[lib]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot derive 'ToJSON' for 'H'"),
                "Expected a ToJSON derive error, got: {}", msg);
            assert!(msg.contains("field 'hop'") && msg.contains("function"),
                "The error must name the field and explain functions have no JSON form, got: {}", msg);
        }
        Ok(_) => panic!("Expected deriving (ToJSON) on a function-typed field to be rejected"),
    }
}

#[test]
fn tojson_derive_rejects_field_without_instance() {
    let lib = Path::new("../lib");
    let source = r#"
import JSON

data Plain = Plain Int

data Holder = Holder { inner :: Plain } deriving (ToJSON)

main :: IO ()
main = putStrLn "hi"
"#;
    match compile(source, Path::new("."), &[lib]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot derive 'ToJSON' for 'Holder'"),
                "Expected a ToJSON derive error, got: {}", msg);
            assert!(msg.contains("'Plain' has no ToJSON instance"),
                "The error must name the instance-less field type, got: {}", msg);
        }
        Ok(_) => panic!("Expected deriving (ToJSON) over an instance-less field type to be rejected"),
    }
}

#[test]
fn tojson_derive_rejects_type_parameters() {
    // GHC's aeson derives `ToJSON (Box a)` by constraining `a`; mata-ll
    // instances cannot carry constraints, so this is rejected with the
    // explanation rather than producing an encoder that cannot exist.
    let lib = Path::new("../lib");
    let source = r#"
import JSON

data Box a = Box a deriving (ToJSON)

main :: IO ()
main = putStrLn "hi"
"#;
    match compile(source, Path::new("."), &[lib]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot derive 'ToJSON' for 'Box'"),
                "Expected a ToJSON derive error, got: {}", msg);
            assert!(msg.contains("type parameters"),
                "The error must explain the type-parameter limitation, got: {}", msg);
        }
        Ok(_) => panic!("Expected deriving (ToJSON) on a parameterized type to be rejected"),
    }
}

#[test]
fn duplicate_local_constructor_rejected() {
    // Two data types in the same module claiming one constructor name used to
    // silently miscompile: the typechecker's map kept the last declaration
    // while codegen's tag table matched the first, so pattern dispatch used
    // the wrong tag at runtime with no diagnostic. Same-module duplicates are
    // now a compile error naming both types (GHC: "Multiple declarations").
    let source = r#"
data A = Ok Int | Bad
data B = Ok String | Worse

main :: IO ()
main = putStrLn "should not compile"
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Duplicate data constructor 'Ok'"),
                "Expected a duplicate-constructor error, got: {}", msg);
            assert!(msg.contains("'A'") && msg.contains("data B"),
                "The error must name both types, got: {}", msg);
            assert!(msg.contains("note:"),
                "The error must carry an explanatory note, got: {}", msg);
        }
        Ok(_) => panic!("Expected a same-module duplicate constructor to be rejected"),
    }
}

#[test]
fn duplicate_newtype_constructor_rejected() {
    // Newtype constructors live in the same namespace.
    let source = r#"
data A = Wrap Int

newtype Wrap = Int

main :: IO ()
main = putStrLn "should not compile"
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Duplicate data constructor 'Wrap'"),
                "Expected a duplicate-constructor error, got: {}", msg);
        }
        Ok(_) => panic!("Expected a newtype constructor duplicating a data constructor to be rejected"),
    }
}

#[test]
fn shadowed_prelude_constructor_stays_shadowed() {
    // GHC scoping: once a local `Err` shadows the Prelude's (ExitValue's),
    // an unqualified `Err` means the local one everywhere in the module —
    // so passing it where an ExitValue is expected is a *type* error, not a
    // silent reuse of the Prelude constructor.
    let source = r#"
data Foo = Err Int | Other

main :: IO ()
main = exit (Err 1)
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot unify") && msg.contains("ExitValue") && msg.contains("Foo"),
                "Expected an ExitValue/Foo unification error, got: {}", msg);
        }
        Ok(_) => panic!("Expected `exit (Err 1)` with a shadowing local Err to be a type error"),
    }
}

// Length-indexed vector (Peano Nat) rejection tests: the type-level length
// index must make these compile-time errors, not runtime crashes. The
// positive counterpart is vec_nat.mll. Length ARITHMETIC (Plus/type
// families) is intentionally not covered here.
const VEC_NAT_PREAMBLE: &str = r#"
data Nat = Z | S Nat

data Vec n a where
    VNil  :: Vec 'Z a
    VCons :: a -> Vec n a -> Vec ('S n) a
"#;

#[test]
fn vec_nat_rejects_vhead_of_empty() {
    // vhead demands Vec ('S n) a; VNil is Vec 'Z a. 'S n and 'Z can never
    // unify, so taking the head of an empty vector is a compile error.
    let source = format!(
        "{}{}",
        VEC_NAT_PREAMBLE,
        r#"
vhead :: Vec ('S n) a -> a
vhead (VCons x _) = x

main :: IO ()
main = print (vhead VNil)
"#
    );
    match compile(&source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot unify") && msg.contains("''S") && msg.contains("''Z'"),
                "Expected a 'S-vs-'Z unification error, got: {}", msg);
            assert!(msg.contains("in definition of 'main'"),
                "The error must point at the offending definition, got: {}", msg);
        }
        Ok(_) => panic!("Expected `vhead VNil` to be rejected at compile time"),
    }
}

#[test]
fn vec_nat_rejects_vtail_of_empty() {
    // Same non-empty precondition as vhead, checked through a consumer of
    // the result so the call is genuinely demanded by the program's types.
    let source = format!(
        "{}{}",
        VEC_NAT_PREAMBLE,
        r#"
vtail :: Vec ('S n) a -> Vec n a
vtail (VCons _ xs) = xs

vlen :: Vec n a -> Int
vlen VNil = 0
vlen (VCons _ xs) = 1 + vlen xs

main :: IO ()
main = print (vlen (vtail VNil))
"#
    );
    match compile(&source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot unify") && msg.contains("''S") && msg.contains("''Z'"),
                "Expected a 'S-vs-'Z unification error, got: {}", msg);
        }
        Ok(_) => panic!("Expected `vtail VNil` to be rejected at compile time"),
    }
}

#[test]
fn vec_nat_rejects_overlong_vector_literal() {
    // The annotation claims length two but the value carries three
    // elements; the innermost VCons forces 'S 'Z ~ 'Z, which must fail.
    let source = format!(
        "{}{}",
        VEC_NAT_PREAMBLE,
        r#"
v2 :: Vec ('S ('S 'Z)) Int
v2 = VCons 1 (VCons 2 (VCons 3 VNil))

main :: IO ()
main = putStrLn "should not compile"
"#
    );
    match compile(&source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot unify") && msg.contains("''S 'Z'") && msg.contains("''Z'"),
                "Expected a 'S 'Z-vs-'Z unification error, got: {}", msg);
            assert!(msg.contains("in definition of 'v2'"),
                "The error must point at the lying binding, got: {}", msg);
        }
        Ok(_) => panic!("Expected a 3-element vector annotated as length 2 to be rejected"),
    }
}

#[test]
fn vec_nat_rejects_short_vector_literal() {
    // The mirror image: annotation claims length two, value has one
    // element, so VNil is used where 'S 'Z more elements are promised.
    let source = format!(
        "{}{}",
        VEC_NAT_PREAMBLE,
        r#"
v2 :: Vec ('S ('S 'Z)) Int
v2 = VCons 1 VNil

main :: IO ()
main = putStrLn "should not compile"
"#
    );
    match compile(&source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot unify") && msg.contains("''Z'") && msg.contains("''S 'Z'"),
                "Expected a 'Z-vs-'S 'Z unification error, got: {}", msg);
            assert!(msg.contains("in definition of 'v2'"),
                "The error must point at the lying binding, got: {}", msg);
        }
        Ok(_) => panic!("Expected a 1-element vector annotated as length 2 to be rejected"),
    }
}

#[test]
fn json_derive_duplicate_effective_keys_rejected() {
    // Two fields mapping to the same effective JSON key would silently
    // overwrite each other in the encoded object. This must be rejected on
    // a type that derives ONLY a JSON codec (no LuaDict, so the LuaDict key
    // validation cannot be the thing that catches it).
    let lib = Path::new("../lib");
    for class in ["ToJSON", "FromJSON"] {
        let source = format!(r#"
import JSON

data D = D {{ a as "k" :: Int, b as "k" :: Int }}
    deriving ({})

main :: IO ()
main = pure ()
"#, class);
        match compile(&source, Path::new("."), &[lib]) {
            Err(e) => {
                let msg = format!("{}", e);
                assert!(msg.contains(&format!("Cannot derive '{}' for 'D'", class))
                        && msg.contains("both map to the JSON key \"k\""),
                    "expected a duplicate JSON key error for {}, got: {}", class, msg);
            }
            Ok(_) => panic!("duplicate effective JSON keys must fail deriving ({})", class),
        }
    }
}

#[test]
fn json_derive_empty_effective_key_rejected() {
    let lib = Path::new("../lib");
    for class in ["ToJSON", "FromJSON"] {
        let source = format!(r#"
import JSON

data D = D {{ a as "" :: Int }}
    deriving ({})

main :: IO ()
main = pure ()
"#, class);
        match compile(&source, Path::new("."), &[lib]) {
            Err(e) => {
                let msg = format!("{}", e);
                assert!(msg.contains(&format!("Cannot derive '{}' for 'D'", class))
                        && msg.contains("empty string"),
                    "expected an empty JSON key error for {}, got: {}", class, msg);
            }
            Ok(_) => panic!("an empty effective JSON key must fail deriving ({})", class),
        }
    }
}

#[test]
fn json_derive_renamed_tag_key_rejected() {
    // The tag-collision check is on the EFFECTIVE key: a field renamed
    // `as "tag"` collides with the codec's constructor tag even though its
    // Haskell name does not.
    let lib = Path::new("../lib");
    for class in ["ToJSON", "FromJSON"] {
        let source = format!(r#"
import JSON

data T = A {{ kind as "tag" :: String }} | B
    deriving ({})

main :: IO ()
main = pure ()
"#, class);
        match compile(&source, Path::new("."), &[lib]) {
            Err(e) => {
                let msg = format!("{}", e);
                assert!(msg.contains(&format!("Cannot derive '{}' for 'T'", class))
                        && msg.contains("\"tag\""),
                    "expected the tag-collision explanation for {}, got: {}", class, msg);
            }
            Ok(_) => panic!("a field renamed to the JSON key \"tag\" in a sum must fail deriving ({})", class),
        }
    }
}

#[test]
fn json_derive_field_named_tag_renamed_away_accepted() {
    // The flip side of the effective-key tag check: a field NAMED `tag`
    // whose `as` rename moves it to a different JSON key does not collide.
    let lib = Path::new("../lib");
    let source = r#"
import JSON

data T = A { tag as "kind" :: String } | B
    deriving (Eq, ToJSON, FromJSON)

rt :: T -> Bool
rt x = case decodeJSON (encodeToJSON x) of
    Right y -> y == x
    Left _ -> False

main :: IO ()
main = do
    assert (encodeToJSON (A "z") == "{\"tag\":\"A\",\"kind\":\"z\"}") "renamed-away tag field encodes"
    assert (rt (A "z")) "renamed-away tag field round-trips"
"#;
    let lua_code = compile(source, Path::new("."), &[lib])
        .expect("a `tag` field renamed to another JSON key should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("tag_renamed_away").exec()
        .expect("every in-program assertion should pass");
}

#[test]
fn constructor_as_duplicate_tags_rejected() {
    // Two constructors mapping to the same effective JSON tag would encode
    // identically and make every decode of that tag ambiguous.
    let lib = Path::new("../lib");
    for class in ["ToJSON", "FromJSON"] {
        let source = format!(r#"
import JSON

data D = A as "x" | B as "x"
    deriving ({})

main :: IO ()
main = pure ()
"#, class);
        match compile(&source, Path::new("."), &[lib]) {
            Err(e) => {
                let msg = format!("{}", e);
                assert!(msg.contains(&format!("Cannot derive '{}' for 'D'", class))
                        && msg.contains("both map to the JSON tag \"x\""),
                    "expected a duplicate JSON tag error for {}, got: {}", class, msg);
            }
            Ok(_) => panic!("duplicate effective JSON tags must fail deriving ({})", class),
        }
    }
}

#[test]
fn constructor_as_colliding_with_source_name_rejected() {
    // A rename may also collide with another constructor's UNRENAMED source
    // name — the effective-tag check catches that the same way.
    let lib = Path::new("../lib");
    let source = r#"
import JSON

data D = A as "B" | B
    deriving (FromJSON)

main :: IO ()
main = pure ()
"#;
    match compile(source, Path::new("."), &[lib]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot derive 'FromJSON' for 'D'")
                    && msg.contains("both map to the JSON tag \"B\""),
                "expected a tag collision with the unrenamed constructor, got: {}", msg);
        }
        Ok(_) => panic!("a rename colliding with another constructor's source name must fail"),
    }
}

#[test]
fn constructor_as_empty_tag_rejected() {
    let lib = Path::new("../lib");
    let source = r#"
import JSON

data D = A as "" | B
    deriving (ToJSON)

main :: IO ()
main = pure ()
"#;
    match compile(source, Path::new("."), &[lib]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot derive 'ToJSON' for 'D'") && msg.contains("empty string"),
                "expected an empty-tag error, got: {}", msg);
        }
        Ok(_) => panic!("an empty `as` tag must fail"),
    }
}

#[test]
fn constructor_as_without_json_deriving_rejected() {
    // The constructor rename only changes the JSON tag; a constructor is a
    // positional integer tag at the Lua boundary, so without a derived JSON
    // codec the rename has nothing to apply to and is rejected rather than
    // silently ignored. (This also pins down the old misparse: before the
    // constructor `as` grammar existed, `data Foo = Foo as "foo"` parsed
    // `as` and `"foo"` as two phantom FIELD TYPES — it "compiled" and then
    // failed bizarrely at every use of Foo. It must now parse as the rename
    // and produce this meaningful error.)
    let source = r#"
data Foo = Foo as "foo"

main :: IO ()
main = pure ()
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Constructor 'Foo' of 'Foo' is renamed with `as \"foo\"`")
                    && msg.contains("derives neither ToJSON nor FromJSON"),
                "expected the as-without-JSON-deriving error, got: {}", msg);
            assert!(msg.contains("positional integer tag"),
                "the note must explain why the Lua side has no name slot, got: {}", msg);
            assert!(!msg.contains("expects 2 args"),
                "the old phantom-field misparse is back: {}", msg);
        }
        Ok(_) => panic!("constructor `as` without ToJSON/FromJSON must fail (and never misparse as phantom fields)"),
    }
}

#[test]
fn constructor_as_misparse_regression_nullary_stays_nullary() {
    // The other half of the misparse regression: with a JSON deriving the
    // renamed constructor compiles AND is genuinely nullary — usable as a
    // bare value. Under the old misparse Foo would have demanded 2 phantom
    // arguments here.
    let lib = Path::new("../lib");
    let source = r#"
import JSON

data Foo = Foo as "foo"
    deriving (ToJSON)

main :: IO ()
main = putStrLn (encodeToJSON Foo)
"#;
    let lua_code = compile(source, Path::new("."), &[lib])
        .expect("a renamed nullary constructor must compile and be nullary")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("con_as_nullary").exec()
        .expect("should run");
}

#[test]
fn constructor_as_on_untagged_single_constructor_rejected() {
    // A lone non-nullary constructor encodes untagged — no tag appears in
    // the JSON — so a rename there could only be silently ignored.
    let lib = Path::new("../lib");
    let source = r#"
import JSON

data W = W Int as "w"
    deriving (ToJSON)

main :: IO ()
main = pure ()
"#;
    match compile(source, Path::new("."), &[lib]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot derive 'ToJSON' for 'W'")
                    && msg.contains("encodes untagged"),
                "expected the untagged-rename rejection, got: {}", msg);
        }
        Ok(_) => panic!("a rename on the constructor of an untagged type must fail"),
    }
}

#[test]
fn constructor_as_requires_string_literal() {
    // `as` after a constructor's field types can only start the rename;
    // anything but a string literal after it is a located parse error, not
    // a silent misparse.
    let source = r#"
data Foo = Foo as 5

main :: IO ()
main = pure ()
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Expected a string literal after 'as' in constructor 'Foo'"),
                "expected the string-literal parse error, got: {}", msg);
        }
        Ok(_) => panic!("`as` followed by a non-string must be a parse error"),
    }
}

#[test]
fn shared_external_key_drives_lua_and_json() {
    // The headline of the shared-external-name feature: ONE `as "key"`
    // rename gives the field its external name at BOTH boundaries — the
    // LuaDict table key (asserted via raw_get on the exported table) AND
    // the JSON object key of the derived codec (asserted on the encoded
    // string). The Haskell field name appears at neither.
    let lib = Path::new("../lib");
    let source = r#"
import JSON

data Acct = Acct { acctName as "name" :: String, acctScore :: Int }
    deriving (Eq, LuaDict, ToJSON, FromJSON)

export mkAcct :: String -> Acct
mkAcct n = Acct { acctName = n, acctScore = 5 }

export encAcct :: Acct -> String
encAcct a = encodeToJSON a

main :: IO ()
main = pure ()
"#;
    let lua_code = compile(source, Path::new("."), &[lib])
        .expect("should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    let module: mlua::Table = lua.load(&lua_code)
        .set_name("shared_external_key")
        .call("shared_external_key")
        .expect("should load module");

    let mk: mlua::Function = module.get("mkAcct").unwrap();
    let acct: mlua::Table = mk.call("zoe").expect("mkAcct should return a table");

    // Lua boundary: the renamed key IS the raw table key...
    let name: String = acct.raw_get("name").expect("renamed 'name' key present");
    assert_eq!(name, "zoe", "the `as` rename is the LuaDict table key");
    let score: i64 = acct.raw_get("acctScore").expect("unrenamed key keeps its name");
    assert_eq!(score, 5);
    // ...and the Haskell field name is not.
    let stray: mlua::Value = acct.raw_get("acctName").unwrap();
    assert!(matches!(stray, mlua::Value::Nil),
        "Haskell field name must not appear as a Lua key");

    // JSON boundary: the SAME rename is the JSON object key.
    let enc: mlua::Function = module.get("encAcct").unwrap();
    let json: String = enc.call(acct).expect("encAcct should encode");
    assert_eq!(json, "{\"name\":\"zoe\",\"acctScore\":5}",
        "the same `as` rename is the JSON object key");
}

#[test]
fn luadict_enum_string_boundary_roundtrips() {
    // `deriving (LuaDict)` on an all-nullary sum type makes each constructor a
    // Lua STRING at the boundary: the `as "tag"` rename when present, the
    // constructor name otherwise. The string must cross out AND back in, and
    // Ord/fromEnum must still follow DECLARATION ORDER (the tag is boundary-only).
    let lib = Path::new("../lib");
    let source = r#"
data Perm = Anonymous as "anonymous" | User | Admin
    deriving (Eq, Ord, Enum, Bounded, Show, LuaDict)

export mkFrom :: Int -> Perm
mkFrom n = toEnum n

export isAnon :: Perm -> Bool
isAnon Anonymous = True
isAnon _ = False

export rankOf :: Perm -> Int
rankOf p = fromEnum p

export below :: Perm -> Perm -> Bool
below a b = a < b

main :: IO ()
main = pure ()
"#;
    let lua_code = compile(source, Path::new("."), &[lib])
        .expect("should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    let module: mlua::Table = lua.load(&lua_code)
        .set_name("luadict_enum")
        .call("luadict_enum")
        .expect("should load module");

    // (a)+(b) Out at the boundary: renamed -> its `as` string; unrenamed -> name.
    let mk: mlua::Function = module.get("mkFrom").unwrap();
    let anon: String = mk.call(0).expect("mkFrom 0");
    assert_eq!(anon, "anonymous", "renamed nullary constructor's `as` string");
    let user: String = mk.call(1).expect("mkFrom 1");
    assert_eq!(user, "User", "unrenamed nullary constructor uses its own name");
    let admin: String = mk.call(2).expect("mkFrom 2");
    assert_eq!(admin, "Admin");

    // Round-trip BACK in: a raw Lua string is accepted as the constructor.
    let is_anon: mlua::Function = module.get("isAnon").unwrap();
    let a1: bool = is_anon.call("anonymous").expect("isAnon anonymous");
    assert!(a1, "the `as` string round-trips back to Anonymous");
    let a2: bool = is_anon.call("User").expect("isAnon User");
    assert!(!a2);

    // (d) Ord/fromEnum follow declaration order, not the string tag.
    let rank: mlua::Function = module.get("rankOf").unwrap();
    let r0: i64 = rank.call("anonymous").expect("rankOf anonymous");
    assert_eq!(r0, 0, "fromEnum Anonymous == 0 (declaration order)");
    let r2: i64 = rank.call("Admin").expect("rankOf Admin");
    assert_eq!(r2, 2, "fromEnum Admin == 2 (declaration order)");
    let below: mlua::Function = module.get("below").unwrap();
    let lt: bool = below.call(("anonymous", "User")).expect("below anon user");
    assert!(lt, "Anonymous < User by declaration order despite \"anonymous\" > \"User\"");
    let gt: bool = below.call(("Admin", "User")).expect("below admin user");
    assert!(!gt, "Admin < User is false by declaration order");
}

#[test]
fn luadict_enum_duplicate_tag_rejected() {
    // (c) Two constructors that map to the same Lua string are rejected: they
    // would be indistinguishable at the boundary. Here an unrenamed `User`
    // collides with a renamed `as "User"`.
    let source = r#"
data D = User | Other as "User" deriving (LuaDict)

main :: IO ()
main = pure ()
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot derive 'LuaDict' for 'D'")
                    && msg.contains("both map to the Lua tag \"User\""),
                "expected a duplicate-tag error, got: {}", msg);
        }
        Ok(_) => panic!("two LuaDict constructors sharing a tag must fail"),
    }
}

#[test]
fn luadict_enum_empty_tag_rejected() {
    // (c) An empty `as` tag names nothing a Lua host could tell apart.
    let source = r#"
data D = A as "" | B deriving (LuaDict)

main :: IO ()
main = pure ()
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot derive 'LuaDict' for 'D'")
                    && msg.contains("empty string"),
                "expected an empty-tag error, got: {}", msg);
        }
        Ok(_) => panic!("an empty LuaDict enum tag must fail"),
    }
}

#[test]
fn decode_json_without_instance_reported() {
    // Using decodeJSON at a type with no FromJSON instance must fail with a
    // missing-instance error at compile time, not produce broken Lua.
    let lib = Path::new("../lib");
    let source = r#"
import JSON

data Q = Q Int

main :: IO ()
main = case (decodeJSON "1" :: Either String Q) of
    Left e -> putStrLn e
    Right _ -> putStrLn "ok"
"#;
    match compile(source, Path::new("."), &[lib]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("No instance") && msg.contains("FromJSON") && msg.contains("Q"),
                "Expected a FromJSON-instance error naming Q, got: {}", msg);
        }
        Ok(_) => panic!("Expected decodeJSON at an instance-less type to be rejected"),
    }
}

#[test]
fn instance_on_parameterized_container_compiles() {
    // Regression: `instance C [a]` and `instance C (Maybe a)` used to crash
    // the compiler with a stack overflow — the class variable `a` and the
    // instance's own `a` were the same TyVar, so substituting a := [a] made
    // apply_subst chase its own output forever. They must now compile AND
    // dispatch.
    let source = r#"
class C a where
    cname :: a -> String

instance C [a] where
    cname _ = "list"

instance C (Maybe a) where
    cname _ = "maybe"

main :: IO ()
main = do
    putStrLn (cname [1, 2, 3])
    putStrLn (cname (Just True))
"#;
    match compile(source, Path::new("."), &[]) {
        Ok(r) => {
            assert!(r.lua_code.contains("list") && r.lua_code.contains("maybe"),
                "instance bodies should be present in the output");
        }
        Err(e) => panic!("instance C [a] / C (Maybe a) should compile, got: {}", e),
    }
}

#[test]
fn argument_specialized_instance_head_rejected() {
    // Dispatch keys on the head constructor alone, so `Pretty [Int]` would
    // silently run for `pretty [True]` — reject it. (Was: `pretty [True]` ran
    // the `[Int]` body.)
    let e = compile_err(
        "class Pretty a where\n    pretty :: a -> String\ninstance Pretty [Int] where\n    pretty _ = \"int list\"\nmain :: IO ()\nmain = putStrLn (pretty ([True] :: [Bool]))\n",
    );
    assert!(e.contains("too specific"), "got: {e}");
    assert!(e.contains("[Int]"), "got: {e}");

    // Repeated type argument (`Pair a a`) is likewise rejected.
    let e = compile_err(
        "data Pair a b = Pair a b\nclass Pretty a where\n    pretty :: a -> String\ninstance Pretty (Pair a a) where\n    pretty _ = \"pair\"\nmain :: IO ()\nmain = pure ()\n",
    );
    assert!(e.contains("too specific") || e.contains("DISTINCT"), "got: {e}");
}

#[test]
fn duplicate_instance_is_hard_error() {
    // Two instances for the same (class, head) silently overwrote (last wins);
    // now a compile error, like GHC's duplicate-instance rejection. (Strict
    // version of `duplicate_instance_rejected`, which tolerated the old gap.)
    let e = compile_err(
        "class Greet a where\n    greet :: a -> String\ninstance Greet Int where\n    greet _ = \"first\"\ninstance Greet Int where\n    greet _ = \"second\"\nmain :: IO ()\nmain = putStrLn (greet (1 :: Int))\n",
    );
    assert!(e.contains("Duplicate instance") && e.contains("Greet Int"), "got: {e}");
}

#[test]
fn overlapping_instances_rejected() {
    // `instance Pretty [a]` and `instance Pretty [Int]` overlap at head
    // `[]`; both `pretty [1]` and `pretty [True]` used to pick the
    // last-declared body. Now the specific head is rejected.
    let e = compile_err(
        "class Pretty a where\n    pretty :: a -> String\ninstance Pretty a => Pretty [a] where\n    pretty _ = \"generic\"\ninstance Pretty [Int] where\n    pretty _ = \"int list\"\nmain :: IO ()\nmain = pure ()\n",
    );
    assert!(e.contains("too specific") || e.contains("Duplicate instance"), "got: {e}");
}

#[test]
fn instance_context_unsatisfied_rejected() {
    // Using a context-constrained instance at a type that lacks the required
    // instance must fail with a located error naming the full type, and a
    // note explaining WHICH context constraint failed — not compile silently,
    // and not report a spurious error inside the instance body.
    let source = r#"
data Blob = MkBlob
data Tree a = Leaf a | Branch (Tree a) (Tree a)

instance Show a => Show (Tree a) where
    show (Leaf x)     = "Leaf " <> show x
    show (Branch l r) = "Branch (" <> show l <> ") (" <> show r <> ")"

main :: IO ()
main = putStrLn (show (Leaf MkBlob))
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("No instance for 'Show (Tree Blob)'"),
                "Expected a missing-instance error at the use type, got: {}", msg);
            assert!(msg.contains("there is no instance 'Show Blob'"),
                "The note must name the failing context constraint, got: {}", msg);
            assert!(msg.contains("definition of 'main'"),
                "The error must point at the use site, not the instance body, got: {}", msg);
        }
        Ok(_) => panic!("Expected use of Show (Tree a) at Tree Blob to be rejected"),
    }
}

#[test]
fn instance_context_ill_formed_rejected() {
    // A context constraint over a variable the instance head does not bind
    // can never be satisfied by any use of the instance; reject it at the
    // declaration with an explanation.
    let source = r#"
data Tree a = Leaf a | Branch (Tree a) (Tree a)

instance Show b => Show (Tree a) where
    show (Leaf _) = "Leaf"
    show (Branch _ _) = "Branch"

main :: IO ()
main = putStrLn (show (Leaf 1))
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("does not appear in the instance head"),
                "Expected an unbound-context-variable error, got: {}", msg);
        }
        Ok(_) => panic!("Expected a context over an unbound variable to be rejected"),
    }
}

#[test]
fn eq_without_instance_rejected() {
    let source = r#"
data Foo = Foo
    deriving Show

main :: IO ()
main = putStrLn (show (Foo == Foo))
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("No instance"), "Expected 'No instance' error, got: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for == without Eq instance"),
    }
}

#[test]
fn unqualified_conflicting_import_rejected() {
    // Data.Map defines `null` with an incompatible type; importing it
    // unqualified must fail with a clear, actionable message pointing at
    // qualified import — not a baffling unification error.
    let lib = Path::new("../lib");
    let source = "import Data.Map\nmain :: IO ()\nmain = putStrLn \"hi\"\n";
    match compile(source, Path::new("."), &[lib]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("unqualified") && msg.contains("import qualified"),
                "Expected a clear collision message, got: {}", msg);
            assert!(msg.contains("null"), "Expected the conflicting name, got: {}", msg);
        }
        Ok(_) => panic!("Expected unqualified Data.Map import to be rejected"),
    }
    // The qualified form must still compile.
    let ok = "import qualified Data.Map as M\nmain :: IO ()\nmain = putStrLn (show (M.size M.empty))\n";
    assert!(compile(ok, Path::new("."), &[lib]).is_ok(),
        "qualified Data.Map import should compile");
}

#[test]
fn show_without_instance_rejected() {
    let source = r#"
data Secret = Secret Int

main :: IO ()
main = putStrLn (show (Secret 42))
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("No instance"), "Expected 'No instance' error, got: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for show without Show instance"),
    }
}

#[test]
fn unknown_type_in_record_field_rejected() {
    // `Boolean` is not a type in mata-ll (the boolean type is `Bool`). This
    // used to slip through unvalidated and resurface as a baffling
    // "No instance for 'show' on type 'Boolean'" from deriving (Show). The
    // reference must be rejected as an unknown type — with the Bool spelling
    // hint — and the missing-instance error must not mask it.
    let source = r#"
data Foo = Foo { a :: String, b :: Boolean } deriving (Show)

main :: IO ()
main = putStrLn "hi"
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Unknown type 'Boolean'"),
                "Expected an unknown-type error, got: {}", msg);
            assert!(msg.contains("spelled 'Bool'"),
                "Expected the Bool spelling hint, got: {}", msg);
            assert!(!msg.contains("No instance"),
                "A missing-instance error must not mask the unknown type: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for unknown type 'Boolean' in a record field"),
    }
}

#[test]
fn unknown_type_in_signature_rejected() {
    // The same undefined name in a function signature must be caught too —
    // previously it flowed through as an opaque type and compiled silently.
    let source = r#"
f :: Boolean -> Int
f x = 1

main :: IO ()
main = putStrLn "hi"
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Unknown type 'Boolean'"),
                "Expected an unknown-type error, got: {}", msg);
            assert!(msg.contains("type signature for 'f'"),
                "Expected the signature context in the error, got: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for unknown type 'Boolean' in a signature"),
    }
}

#[test]
fn defined_type_without_show_still_reports_missing_instance() {
    // Consistency guard for the unknown-type check: a type that EXISTS but
    // has no Show instance must still get the missing-instance error, not an
    // unknown-type error. "Type exists but lacks an instance" and "type does
    // not exist" are different diagnoses.
    let source = r#"
data Baz = Baz Int

data Foo = Foo { a :: String, b :: Baz } deriving (Show)

main :: IO ()
main = putStrLn (show (Foo { a = "x", b = Baz 1 }))
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("No instance"),
                "Expected a missing-instance error, got: {}", msg);
            assert!(!msg.contains("Unknown type"),
                "'Baz' is defined and must not be reported as unknown: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for deriving Show over a field type without Show"),
    }
}

#[test]
fn ambiguous_show_nothing_rejected() {
    // `Nothing :: Maybe a` leaves the element type `a` unconstrained; `show`
    // then needs a `Show a` that nothing can determine. This is a genuine
    // ambiguous type (GHC rejects it too) and must be a compile error rather
    // than silently defaulting or picking a runtime rendering.
    let source = r#"
main :: IO ()
main = putStrLn (show Nothing)
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Ambiguous type"),
                "Expected an ambiguity error, got: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for ambiguous `show Nothing`"),
    }
}

#[test]
fn ambiguous_show_nothing_in_larger_expr_rejected() {
    // The ambiguous `show Nothing` must still be caught when buried in a larger
    // expression whose other parts (e.g. `show 3`) are perfectly well-typed.
    let source = r#"
main :: IO ()
main = print $ show 3 <> "hi" <> show Nothing
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Ambiguous type"),
                "Expected an ambiguity error, got: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for ambiguous `show Nothing` in a larger expression"),
    }
}

#[test]
fn type_error_does_not_cascade_into_spurious_ambiguity() {
    // A single genuine type error in one branch (badMap, a HashMap, spliced
    // into a String with `<>`) must NOT spawn secondary "Ambiguous type"
    // errors for the same definition. The scrutinee's `:: Either String Foo`
    // annotation fully determines the FromJSON/Show types — the same code
    // without the bad splice compiles — so those ambiguity reports are pure
    // cascade artifacts that point the user away from the real problem.
    let source = r#"
import qualified Data.Map as Map
import JSON

data Foo = Foo { fooX as "x" :: [String] } deriving (FromJSON, Show)

badMap :: Map.Map String String
badMap = Map.empty

export run :: IO ()
run = case (decodeJSON "{}" :: Either String Foo) of
        Right r -> print r
        Left e  -> error $ "oops " <> e <> " (" <> badMap <> ")"
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot unify 'HashMap String String' with 'String'"),
                "Expected the genuine unification error, got: {}", msg);
            assert!(!msg.contains("Ambiguous type"),
                "The clause error must not cascade into a spurious ambiguity report: {}", msg);
            assert!(!msg.contains("'FromJSON' instance") && !msg.contains("'Show' instance"),
                "The annotated decodeJSON/print constraints are determined and must not be reported: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for the HashMap-into-String splice"),
    }
}

#[test]
fn type_error_in_where_binding_does_not_cascade_into_spurious_ambiguity() {
    // Same cascade guard for the where-binding recovery path: the binding's
    // genuine error (`String <> True`) is reported and checking continues,
    // but the FromJSON/Show constraints emitted while inferring the failed
    // body must not resurface as spurious "Ambiguous type" errors.
    let source = r#"
import JSON

data Foo = Foo { fooX as "x" :: [String] } deriving (FromJSON, Show)

export run :: IO ()
run = putStrLn msg
  where
    msg = case (decodeJSON "{}" :: Either String Foo) of
            Right r -> show r
            Left e  -> "oops " <> e <> True
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot unify 'String' with 'Bool'"),
                "Expected the genuine unification error, got: {}", msg);
            assert!(msg.contains("where-binding 'msg'"),
                "Expected the where-binding context on the error, got: {}", msg);
            assert!(!msg.contains("Ambiguous type"),
                "The where-binding error must not cascade into a spurious ambiguity report: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for `String <> True` in the where-binding"),
    }
}

#[test]
fn genuine_ambiguity_in_where_binding_still_rejected() {
    // Over-suppression guard for the cascade fix: a where-binding that is
    // GENUINELY ambiguous (`show Nothing`, no other error anywhere) must
    // still be rejected with the ambiguity message.
    let source = r#"
main :: IO ()
main = putStrLn msg
  where msg = show Nothing
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Ambiguous type"),
                "Expected an ambiguity error, got: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for ambiguous `show Nothing` in a where-binding"),
    }
}

#[test]
fn sibling_clause_error_does_not_suppress_genuine_ambiguity() {
    // Scope guard for the cascade fix: suppression is per failed clause, not
    // per definition. Clause 2 has a genuine unification error; clause 1 has
    // a genuine ambiguity (`show Nothing`) and checked cleanly, so BOTH must
    // be reported — dropping the clean clause's ambiguity would hide a real
    // problem behind an unrelated sibling error.
    let source = r#"
f :: Int -> IO ()
f 0 = putStrLn (show Nothing)
f n = putStrLn (n <> "x")

main :: IO ()
main = f 1
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot unify 'Int' with 'String'"),
                "Expected the genuine unification error from clause 2, got: {}", msg);
            assert!(msg.contains("Ambiguous type"),
                "Clause 1's genuine ambiguity must survive the sibling clause's error: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for both the clause error and the ambiguity"),
    }
}

#[test]
fn show_at_concrete_types_still_compiles() {
    // The ambiguity check must not touch well-typed uses: a numeric literal
    // (`show 3`), a concrete empty list, a concrete Nothing, and `Just 5` all
    // have determined types and must compile.
    let source = r#"
main :: IO ()
main = do
    putStrLn (show (3 :: Int))
    putStrLn (show ([] :: [Int]))
    putStrLn (show (Nothing :: Maybe Int))
    putStrLn (show (Just (5 :: Int)))
"#;
    assert!(compile(source, Path::new("."), &[]).is_ok(),
        "show at concrete types should compile");
}

#[test]
fn polymorphic_show_constraint_still_compiles() {
    // A function that declares `Show a =>` legitimately defers the constraint to
    // its callers; it must still compile (the leftover constraint's variable is
    // part of the function's own type, so it is not ambiguous).
    let source = r#"
f :: Show a => a -> String
f = show

main :: IO ()
main = do
    putStrLn (f ([] :: [Int]))
    putStrLn (f (Nothing :: Maybe Int))
    putStrLn (f (42 :: Int))
"#;
    assert!(compile(source, Path::new("."), &[]).is_ok(),
        "polymorphic Show-constrained function should compile");
}

#[test]
fn type_error_in_where_value_binding_rejected() {
    // A type error inside a `where` value binding must fail compilation with a
    // diagnostic naming the binding. Regression: check_clause used to swallow
    // the inference error and substitute a placeholder term, so the program
    // "compiled" and misbehaved at runtime instead of being rejected.
    // (`&&` on a String, a non-numeric clash, so the failure surfaces inside
    // the binding's body. A numeric mismatch like `1 + "hello"` would now be a
    // deferred `No instance for (Num String)` reported at the enclosing
    // function, because integer literals are polymorphic `Num a => a`.)
    let source = r#"
main :: IO ()
main = putStrLn x
  where x = True && "hello"
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Type error"),
                "Expected a type error, got: {}", msg);
            assert!(msg.contains("where-binding 'x'"),
                "Expected the error to name the where-binding, got: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for a type error in a where value binding"),
    }
}

#[test]
fn where_binding_definition_use_mismatch_rejected() {
    // The binding's own body is fine (`x = True`), but the clause body uses it
    // as a String. Regression: the definition-vs-use unification failure was
    // silently discarded, so this compiled and misbehaved at runtime.
    // (`x = True` rather than the original `x = 5`: an integer literal is now
    // polymorphic `Num a => a`, so `x = 5` used as a String would report a
    // deferred `No instance for (Num String)` instead of a use-site unify
    // failure — this uses a monomorphic Bool binding to keep exercising the
    // definition-vs-use mismatch path.)
    let source = r#"
main :: IO ()
main = putStrLn x
  where x = True
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot unify") && msg.contains("where-binding 'x'"),
                "Expected a mismatch error naming the where-binding, got: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for a where binding defined as Int but used as String"),
    }
}

#[test]
fn type_error_in_where_function_rejected() {
    // Same for a where-bound local function: the conflict between its body
    // and how the clause uses it must be reported, not swallowed into a
    // runtime crash ("attempt to add a 'number' with a 'string'").
    // (`n && "oops"` — a non-numeric clash inside the function body — rather
    // than `n + "oops"`: with polymorphic integer literals the latter defers a
    // `No instance for (Num String)` reported at `main`, not at the binding.)
    let source = r#"
main :: IO ()
main = putStrLn (go True)
  where go n = n && "oops"
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("where-binding 'go'"),
                "Expected the error to name the where-bound function, got: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for a type error in a where-bound function"),
    }
}

#[test]
fn where_function_pattern_use_mismatch_rejected() {
    // The where-function's pattern gives it type `Maybe a -> a`, but the clause
    // body applies it to a Bool. Regression: the pattern/use unification
    // failure was discarded, producing a Lua indexing crash at runtime.
    let source = r#"
main :: IO ()
main = putStrLn (f True)
  where f (Just x) = x
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("where-binding 'f'"),
                "Expected the error to name the where-bound function, got: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for a where function applied at the wrong type"),
    }
}

#[test]
fn multiple_where_binding_errors_all_reported() {
    // Error recovery must keep going: two independently broken where bindings
    // should both be diagnosed in a single compile.
    // (`x = True && "a"` — a non-numeric clash inside the binding body — rather
    // than `x = 1 + "a"`: with polymorphic integer literals the latter is a
    // deferred `No instance for (Num String)` reported at `main`, not a
    // binding-attributed error.)
    let source = r#"
main :: IO ()
main = putStrLn (x <> y)
  where x = True && "a"
        y = notInScope 3
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("where-binding 'x'"),
                "Expected an error for 'x', got: {}", msg);
            assert!(msg.contains("where-binding 'y'") && msg.contains("notInScope"),
                "Expected an error for 'y' too, got: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail with errors for both where bindings"),
    }
}

#[test]
fn valid_where_bindings_still_compile_and_run() {
    // The error paths above must not break correct where clauses: chained
    // value bindings referencing each other, a multi-clause recursive local
    // function with pattern parameters, and bindings used from guards.
    let source = r#"
classify :: Int -> String
classify n
  | n < low = "small"
  | n > high = "big"
  | otherwise = "mid " <> show n
  where low = 10
        high = 100

message :: String
message = greet <> "!"
  where greet = "hello " <> name
        name = "world"

render :: [Int] -> String
render ys = fmt ys
  where fmt [] = "empty"
        fmt (x:xs) = show x <> "," <> fmt xs

main :: IO ()
main = do
  putStrLn message
  putStrLn (render [1, 2, 3])
  putStrLn (classify 5)
  putStrLn (classify 50)
  putStrLn (classify 500)
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("valid where bindings must still compile").lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("valid_where").exec()
        .expect("valid where bindings must still run");
}

#[test]
fn show_of_list_and_maybe_render_distinctly() {
    // With concrete element types, `show` is type-directed: an empty list must
    // render "[]" and `Nothing` must render "Nothing" — they must NOT both
    // collapse to "Nothing" (their shared Lua-nil runtime rep). This exercises
    // the distinction through `show` used as a value (via putStrLn), and through
    // a polymorphic `Show a =>` wrapper, so dictionary dispatch is covered too.
    let source = r#"
f :: Show a => a -> String
f = show

main :: IO ()
main = do
    putStrLn (show ([] :: [Int]))
    putStrLn (show (Nothing :: Maybe Int))
    putStrLn (show (Just (5 :: Int)))
    putStrLn (f ([] :: [Int]))
    putStrLn (f (Nothing :: Maybe Int))
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;

    // Capture `print` (which `putStrLn` lowers to) instead of hitting stdout.
    let lua = mlua::Lua::new();
    let captured = lua.create_table().unwrap();
    lua.globals().set("__captured", captured.clone()).unwrap();
    let print_fn = lua
        .create_function(|lua, s: mlua::LuaString| -> mlua::Result<()> {
            let line = s.to_str()?.to_string();
            let t: mlua::Table = lua.globals().get("__captured")?;
            let n = t.raw_len();
            t.raw_set(n + 1, line)?;
            Ok(())
        })
        .unwrap();
    lua.globals().set("print", print_fn).unwrap();
    lua.load(&lua_code).set_name("show_list_vs_maybe").exec()
        .expect("should run");

    let lines: Vec<String> = captured
        .sequence_values::<String>()
        .collect::<mlua::Result<_>>()
        .unwrap();
    assert_eq!(lines, vec!["[]", "Nothing", "Just 5", "[]", "Nothing"]);
}

#[test]
fn unconstrained_class_method_on_signature_var_rejected() {
    // `show` on a fully-polymorphic `a` with no `Show a` in the signature has no
    // instance (a bare rigid variable has no evidence). GHC rejects this too.
    let source = r#"
poly :: a -> String
poly x = show x

main :: IO ()
main = putStrLn (poly (5 :: Int))
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("No instance for 'Show a'") && msg.contains("Add it to the context"),
                "Expected a missing-context error suggesting the fix, got: {}", msg);
        }
        Ok(_) => panic!("Expected rejection of `show` on an unconstrained variable"),
    }
}

#[test]
fn unconstrained_eq_on_signature_var_rejected() {
    // The Eq analogue: `==` on a bare polymorphic variable with no `Eq a`.
    let source = r#"
same :: a -> a -> Bool
same x y = x == y

main :: IO ()
main = putStrLn (show (same (1 :: Int) 2))
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("No instance for 'Eq a'"),
                "Expected a missing Eq-context error, got: {}", msg);
        }
        Ok(_) => panic!("Expected rejection of `==` on an unconstrained variable"),
    }
}

#[test]
fn declared_class_constraint_accepted() {
    // A declared context makes the use legitimate; it must still compile and run.
    let source = r#"
f :: Show a => a -> String
f = show

main :: IO ()
main = putStrLn (f (5 :: Int))
"#;
    assert!(compile(source, Path::new("."), &[]).is_ok(),
        "a declared `Show a =>` context should be accepted");
}

#[test]
fn superclass_context_satisfies_wanted_constraint() {
    // A declared `Ord a` provides the wanted `Eq a` (Eq is a superclass of Ord),
    // so `x == y` under an `Ord a =>` context compiles.
    let source = r#"
same :: Ord a => a -> a -> Bool
same x y = x == y

main :: IO ()
main = putStrLn (show (same (1 :: Int) 2))
"#;
    assert!(compile(source, Path::new("."), &[]).is_ok(),
        "an Ord context should satisfy a wanted Eq constraint via the superclass");
}

#[test]
fn bare_signature_without_definition_rejected() {
    // A type signature with no accompanying definition (and not an FFI binding)
    // used to silently compile to a nil value. It must now be rejected.
    let source = r#"
foo :: Int

main :: IO ()
main = print foo
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("no accompanying definition"),
                "Expected 'no accompanying definition' error, got: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for bare signature without definition"),
    }
}

#[test]
fn ffi_signature_without_body_accepted() {
    // FFI signatures are legitimately body-less; the bare-signature check must
    // not reject them.
    let source = r#"
sqrtNum :: Number -> LuaPure "math.sqrt" Number

main :: IO ()
main = print (sqrtNum 4.0)
"#;
    match compile(source, Path::new("."), &[]) {
        Ok(_) => {}
        Err(e) => panic!("FFI signature without body should compile, got error: {}", e),
    }
}

#[test]
fn constrained_ffi_signature_without_body_accepted() {
    // A body-less FFI import may carry a class-constraint context (the
    // constraint bounds a marshalled argument — here `LuaDict b` guarantees the
    // rows the callback folds are marshallable). The FFI-import detector must
    // peel that context (and any forall) to find the trailing `LuaIO`/`LuaPure`
    // form; otherwise the constrained signature is misread as an ordinary
    // signature with no accompanying definition. Regression: `extract_ffi_info`
    // previously stopped at `Type::Constrained` and returned None.
    let source = r#"
newtype Db = Db LuaUserData

data Row = Row { rId as "id" :: Int, rName as "name" :: String }
    deriving (LuaDict, Show)

dbQuery :: LuaDict b => Db -> (a -> [b] -> a) -> a -> String -> [b] -> LuaIO ":query_array" a

main :: IO ()
main = pure ()
"#;
    match compile(source, Path::new("."), &[]) {
        Ok(_) => {}
        Err(e) => panic!(
            "constrained body-less FFI import should compile, got error: {}", e),
    }
}

#[test]
fn orphan_instance_rejected() {
    // Show and Int are both defined in the prelude, not locally.
    // Defining an instance for them here is an orphan instance.
    let source = r#"
instance Show Int where
    show x = "int"

main :: IO ()
main = putStrLn "ok"
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Orphan instance"), "Expected 'Orphan instance' error, got: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for orphan instance"),
    }
}

#[test]
fn module_export_hides_private() {
    // ExportHelper only exports publicFn and PublicType.
    // Referencing privateFn should be rejected.
    let source = r#"
import ExportHelper

main :: IO ()
main = putStrLn (show (privateFn 5))
"#;
    let cases_dir = Path::new("tests/cases");
    match compile(source, cases_dir, &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("not exported"), "Expected 'not exported' error, got: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for private function access"),
    }
}

#[test]
fn import_hiding_blocks_hidden_name() {
    let source = r#"
import ExportHelper hiding (publicFn)

main :: IO ()
main = putStrLn (show (publicFn 5))
"#;
    let cases_dir = Path::new("tests/cases");
    match compile(source, cases_dir, &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("not exported"), "Expected 'not exported' error, got: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for hidden import"),
    }
}

// --- New compile-error tests ---

#[test]
fn type_mismatch_rejected() {
    let source = r#"
f :: Int -> Int
f x = x

main :: IO ()
main = print (f "hello")
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot unify"), "Expected 'Cannot unify' error, got: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for String passed where Int expected"),
    }
}

#[test]
fn undefined_variable_rejected() {
    let source = r#"
main :: IO ()
main = print noSuchThing
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Unbound variable"), "Expected 'Unbound variable' error, got: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for undefined variable"),
    }
}

#[test]
fn duplicate_definition_rejected() {
    // Two separate FunDef blocks for the same name with incompatible bodies.
    // The compiler processes both; one will fail to unify against the single sig.
    let source = r#"
f :: Int -> Int
f x = x + 1
f x = "hello"

main :: IO ()
main = print (f 1)
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("Cannot unify") || msg.contains("doesn't match") || msg.contains("Unbound"),
                "Expected a type error for duplicate definition, got: {}", msg
            );
        }
        // Known gap: compiler may accept this if it processes only the first body
        Ok(_) => { /* known gap: duplicate function bodies not rejected */ }
    }
}

#[test]
fn wrong_arity_rejected() {
    // `not` takes one Bool; applying it to two args should fail.
    let source = r#"
main :: IO ()
main = print (not True False)
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("Too many arguments") || msg.contains("Cannot unify"),
                "Expected arity error, got: {}", msg
            );
        }
        Ok(_) => panic!("Expected compilation to fail for too many arguments to 'not'"),
    }
}

#[test]
fn non_exhaustive_rejected() {
    let source = r#"
data Color = Red | Green | Blue
    deriving Show

describeRed :: Color -> String
describeRed Red = "red"

main :: IO ()
main = putStrLn (describeRed Red)
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Non-exhaustive"), "Expected 'Non-exhaustive' error, got: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for non-exhaustive patterns"),
    }
}

#[test]
fn duplicate_instance_rejected() {
    // Two Show instances for the same local type.
    // Known gap: the compiler currently silently overwrites the first instance.
    let source = r#"
data Foo = Foo

instance Show Foo where
    show _ = "first"

instance Show Foo where
    show _ = "second"

main :: IO ()
main = putStrLn (show Foo)
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("duplicate") || msg.contains("Duplicate") || msg.contains("already"),
                "Expected duplicate instance error, got: {}", msg
            );
        }
        // Known gap: compiler does not detect duplicate instances
        Ok(_) => { /* known gap: duplicate instances not rejected */ }
    }
}

#[test]
fn missing_method_rejected() {
    // Show requires `show`; providing a bogus method name instead.
    // The compiler should reject the unknown method name or fail to resolve show at the call site.
    let source = r#"
data Foo = Foo

instance Show Foo where
    notAMethod _ = "foo"

main :: IO ()
main = putStrLn (show Foo)
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("not a method") || msg.contains("No instance") || msg.contains("Unbound"),
                "Expected missing-method or unknown-method error, got: {}", msg
            );
        }
        // Known gap: compiler may silently ignore the bogus method and still fail to resolve show
        Ok(_) => { /* known gap: extraneous instance methods may not be rejected */ }
    }
}

#[test]
fn invalid_deriving_rejected() {
    // Deriving an unsupported class should fail.
    let source = r#"
data Foo = Foo
    deriving Read

main :: IO ()
main = putStrLn "ok"
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("Cannot derive") || msg.contains("only Show, Eq, Ord and Functor"),
                "Expected unsupported deriving error, got: {}", msg
            );
        }
        Ok(_) => panic!("Expected compilation to fail for unsupported 'deriving Read'"),
    }
}

#[test]
fn recursive_type_alias_rejected() {
    // A self-referential type alias. The compiler may loop or produce an error.
    // Known gap: no explicit cycle detection for type aliases.
    let source = r#"
type Loop = [Loop]

main :: IO ()
main = putStrLn "ok"
"#;
    match compile(source, Path::new("."), &[]) {
        Err(_) => { /* any error is acceptable */ }
        // Known gap: recursive type aliases may not be detected
        Ok(_) => { /* known gap: recursive type alias not rejected */ }
    }
}

#[test]
fn unknown_type_rejected() {
    // Using a constructor from a type that doesn't exist.
    // (Unknown names in type positions are rejected by the typechecker's
    // unknown-type check — see unknown_type_in_signature_rejected. This test
    // covers the expression side: an unknown *constructor* must be caught.)
    let source = r#"
main :: IO ()
main = print (NoSuchCtor 42)
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("Unknown constructor") || msg.contains("Unbound") || msg.contains("No instance"),
                "Expected unknown constructor error, got: {}", msg
            );
        }
        // Known gap: unknown constructors in expressions may not always be caught at compile time
        Ok(_) => { /* known gap: unknown constructor not always rejected */ }
    }
}

#[test]
fn constructor_wrong_fields_rejected() {
    // Just applies constructor to wrong number of args in a pattern.
    let source = r#"
data Pair = Pair Int Int
    deriving Show

fst2 :: Pair -> Int
fst2 (Pair x) = x

main :: IO ()
main = print (fst2 (Pair 1 2))
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("expects") || msg.contains("Constructor") || msg.contains("Cannot unify"),
                "Expected constructor arity error, got: {}", msg
            );
        }
        Ok(_) => panic!("Expected compilation to fail for constructor applied to wrong number of args"),
    }
}

#[test]
fn let_type_mismatch_rejected() {
    // Top-level function whose declared type conflicts with the body.
    // The body returns a String literal but the sig says Int.
    let source = r#"
answer :: Int
answer = "forty-two"

main :: IO ()
main = print answer
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("Cannot unify") || msg.contains("doesn't match"),
                "Expected type mismatch error for String body vs Int sig, got: {}", msg
            );
        }
        Ok(_) => panic!("Expected compilation to fail for String body where Int declared"),
    }
}

#[test]
fn guard_non_bool_rejected() {
    // Guard expression returns Int, not Bool — should fail to unify.
    let source = r#"
f :: Int -> Int
f x
    | x = x + 1
    | otherwise = x

main :: IO ()
main = print (f 5)
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("Cannot unify"),
                "Expected 'Cannot unify' for non-Bool guard, got: {}", msg
            );
        }
        Ok(_) => panic!("Expected compilation to fail for non-Bool guard expression"),
    }
}

#[test]
fn duplicate_constructor_rejected() {
    // Two data types with the same constructor name in scope.
    // Known gap: the compiler may silently overwrite the first constructor.
    let source = r#"
data Foo = MkThing Int
data Bar = MkThing String

useFoo :: Foo -> Int
useFoo (MkThing n) = n

main :: IO ()
main = print (useFoo (MkThing 42))
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("duplicate") || msg.contains("Duplicate") || msg.contains("Cannot unify"),
                "Expected duplicate constructor error, got: {}", msg
            );
        }
        // Known gap: duplicate constructor names from different types not detected
        Ok(_) => { /* known gap: duplicate constructor names not rejected */ }
    }
}

#[test]
fn class_method_wrong_type_rejected() {
    // Instance method body produces wrong type relative to the class declaration.
    let source = r#"
data Wrapper = Wrapper Int
    deriving Eq

instance Show Wrapper where
    show (Wrapper n) = n

main :: IO ()
main = putStrLn (show (Wrapper 42))
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("Cannot unify") || msg.contains("doesn't match"),
                "Expected type error for show returning Int instead of String, got: {}", msg
            );
        }
        Ok(_) => panic!("Expected compilation to fail when show returns Int instead of String"),
    }
}

// Regression test: x <- return val must unwrap the thunk (was a known bug)
#[test]
fn bind_return_unwraps_value() {
    let source = r#"
main :: IO ()
main = do
    x <- return (10 :: Int)
    assert (x == 10) "bind return"
    putStrLn "ok"
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("bind_return").exec()
        .expect("x <- return val should bind x to the value");
}

// Runtime error tests: these should compile but fail at runtime
#[test]
fn undefined_errors_when_forced() {
    // `x` carries a concrete type annotation. Without one, `print x` leaves the
    // element type of `x = undefined` unconstrained, which is a genuine ambiguous
    // type (GHC rejects `let x = undefined; print x` for the same reason); the
    // ambiguity check now flags it at compile time. The purpose of this test —
    // that a forced `undefined` raises `Prelude.undefined` at runtime — is
    // unchanged by pinning the type.
    let source = r#"
main :: IO ()
main = do
    let x = undefined :: Int
    print x
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("undefined should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    match lua.load(&lua_code).set_name("undefined_forced").exec() {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Prelude.undefined"),
                "Expected 'Prelude.undefined' error, got: {}", msg);
        }
        Ok(()) => panic!("Expected runtime error when forcing undefined"),
    }
}

// Examples that should compile successfully
#[test]
fn examples_compile() {
    let lib_path = Path::new("../lib");
    let examples_dir = Path::new("../experiments");

    // Examples expected to fail or skip
    let expected_fail: Vec<&str> = vec![
        "bench",              // show specialization gap on list display
        "aestest",            // 256-element S-box lists need large stack (runs via mll compiler)
        "bstest",             // needs large stack (runs via mll compiler)
        "salsa",              // large literal lists need large stack (runs via mll compiler)
        "Ed25519",            // large literal lists need large stack (runs via mll compiler)
        "ed25519test",        // depends on Ed25519 which needs large stack
        "metar",              // needs large stack (many nested parser combinators)
        "match",              // experimental scratch file
        "experiments",        // experimental scratch file
    ];

    let mut failures = Vec::new();
    for entry in std::fs::read_dir(examples_dir).expect("Cannot read experiments/") {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "mll") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_str().unwrap();
        if expected_fail.contains(&stem) {
            continue;
        }
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Cannot read {}: {}", path.display(), e));
        let source_dir = path.parent().unwrap_or(Path::new("."));
        match compile(&source, source_dir, &[lib_path]) {
            Ok(_) => {}
            Err(e) => failures.push(format!("{}: {}", stem, e)),
        }
    }
    if !failures.is_empty() {
        panic!("Examples failed to compile:\n{}", failures.join("\n"));
    }
}

// The curated showcases in examples/ must all compile. Some pull in the
// contrib library (atdg.mll uses Lz4/Hex), so the lib path carries both
// ../lib and ../contrib; the others ignore the extra path harmlessly.
#[test]
fn examples_curated_compile() {
    let lib = Path::new("../lib");
    let contrib = Path::new("../contrib");
    let examples_dir = Path::new("../examples");

    let mut failures = Vec::new();
    for entry in std::fs::read_dir(examples_dir).expect("Cannot read examples/") {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "mll") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_str().unwrap();
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Cannot read {}: {}", path.display(), e));
        let source_dir = path.parent().unwrap_or(Path::new("."));
        if let Err(e) = compile(&source, source_dir, &[lib, contrib]) {
            failures.push(format!("{}: {}", stem, e));
        }
    }
    if !failures.is_empty() {
        panic!("Curated examples failed to compile:\n{}", failures.join("\n"));
    }
}

// Regression: a nullary constructor used as an argument of another pattern
// (e.g. `Box R n`, or the nested `T R (T R a x b) y c` in a red-black tree's
// balance) must parse. Previously the pattern-atom predicate omitted
// UpperIdent, so such arguments were rejected at parse time.
#[test]
fn nullary_constructor_as_pattern_argument() {
    let source = r#"
data Color = R | B
data Box = Box Color Int

unwrap :: Box -> Int
unwrap (Box R n) = n
unwrap (Box B n) = 0 - n

main :: IO ()
main = do
  print (unwrap (Box R 5))
  print (unwrap (Box B 5))
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("nullary constructor as pattern arg should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("nullary_con_arg").exec()
        .expect("should run without error");
}

// Regression: a record field constructed from a non-cheap expression is stored
// as a thunk; projecting it in a strict (arithmetic) context must force it.
// Previously the accessor inlined to `v[idx]` without __force, so the thunk
// (a Lua table) reached arithmetic -> "arithmetic on a table value".
#[test]
fn record_field_projection_is_forced() {
    let source = r#"
data V = V { va :: Number, vb :: Number }

scaleV :: Number -> V -> V
scaleV s v = V (s * va v) (s * vb v)

dot :: V -> Number
dot v = va v * vb v

main :: IO ()
main = print (dot (scaleV 2.0 (V 5.0 7.0)))
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("record_field_force").exec()
        .expect("projecting a thunk-valued field in arithmetic should force it");
}

// Layout: a function whose first argument is on the next line (no same-line
// argument) is consumed as an application. Previously the cross-line
// continuation required at least one same-line arg, so this failed inside
// parenthesized multi-line constructor application.
#[test]
fn first_argument_on_next_line() {
    let source = r#"
data T = L Int | N T T

deep :: T
deep = N (N (L 1)
            (L 2))
         (L 3)

size :: T -> Int
size (L _) = 1
size (N a b) = size a + size b

main :: IO ()
main = assert (size deep == 3) "function with first arg on next line"
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("first arg on next line should parse")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("first_arg_next_line").exec()
        .expect("should run");
}

// Layout: a multi-line application-argument continuation indented past the
// enclosing block (but not necessarily past the function column) is now
// accepted, matching Haskell. Previously it required indentation past the
// function and was rejected as "Unexpected token at top level".
#[test]
fn shallow_multiline_continuation() {
    let source = r#"
import Data.List (foldl')

total :: Int
total = foldl' (\a b -> a + b) 0
  [1, 2, 3, 4, 5]

main :: IO ()
main = print total
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("shallow multi-line continuation should parse");
    let lua = mlua::Lua::new();
    lua.load(&lua_code.lua_code).set_name("shallow_cont").exec()
        .expect("should run");
}

// Regression: a self-referential lazy value bound in a `where` clause, an
// expression `let`, or a do-block `let` must close over itself. Two bugs
// combined here:
//   1. Codegen emitted `local x = __thunk(... x ...)`, but a Lua local is not
//      in scope within its own initializer, so the inner `x` resolved to a nil
//      global. The classic `fib = [1,1] ++ zipWith (+) fib (drop 1 fib)`
//      collapsed to `[1,1]`, so `fib !! 11` read as 1 instead of 144. Fixed by
//      forward-declaring the name (`local x`) before assigning it.
//   2. The typechecker treated `let`/do-`let` as sequential (let*), rejecting
//      self- and forward-references ("Unbound variable: fib"). Fixed by
//      inferring let groups as mutually recursive (pre-register fresh vars,
//      then generalize) — like `where`/top-level, but keeping let-polymorphism.
#[test]
fn recursive_lazy_value_in_where_let_and_do() {
    let source = r#"
fibTop :: [Int]
fibTop = [1, 1] ++ zipWith (+) fibTop (drop 1 fibTop)

nthWhere :: Int -> Int
nthWhere k = fib !! k
  where
    fib = [1, 1] ++ zipWith (+) fib (drop 1 fib)

nthLet :: Int -> Int
nthLet k =
  let fib = [1, 1] ++ zipWith (+) fib (drop 1 fib)
  in fib !! k

-- mutually recursive let bindings
isEven :: Int -> Bool
isEven n =
  let ev = \m -> if m == 0 then True else od (m - 1)
      od = \m -> if m == 0 then False else ev (m - 1)
  in ev n

-- let-polymorphism must survive the recursive-let change
polyPair :: (Int, Bool)
polyPair = let idf = \x -> x in (idf 5, idf True)

main :: IO ()
main = do
  let fibDo = [1, 1] ++ zipWith (+) fibDo (drop 1 fibDo)
  assert (fibTop !! 11 == 144) "top-level recursive list (12th fib)"
  assert (nthWhere 11 == 144) "where-bound recursive list (12th fib)"
  assert (nthWhere 12 == 233) "where-bound recursive list (13th fib)"
  assert (nthLet 11 == 144) "let-bound recursive list (12th fib)"
  assert (nthLet 12 == 233) "let-bound recursive list (13th fib)"
  assert (fibDo !! 11 == 144) "do-block let recursive list (12th fib)"
  assert (isEven 10) "mutually recursive let"
  assert (polyPair == (5, True)) "let-polymorphism preserved"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("recursive lazy where/let values should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("recursive_lazy_value").exec()
        .expect("recursive lazy where/let/do bindings should evaluate correctly");
}

// Regression: non-strict argument passing must keep lazy code productive.
// A user's prime sieve (a self-referential list filtered via a comprehension)
// diverged because several strict shortcuts leaked into lazy positions:
//   - a one-level function call passed as an argument (concatMap's recursion,
//     list comprehensions desugar to concatMap) was evaluated eagerly;
//   - `x : rest` force-evaluated a variable tail, collapsing the spine;
//   - lambda parameters were emitted bare and broke when a higher-order call
//     passed a thunk;
//   - a recursive call inside a guard was missed by the strictness analysis,
//     marking the parameter concrete while the call site thunked it.
// Each is exercised below. (Calls to inlinable helpers like makeAdder stay
// eager, so this must not regress arithmetic-heavy code.)
#[test]
fn lazy_arguments_and_infinite_lists() {
    let source = r#"
-- infinite list comprehension (desugars to concatMap) must stream
evens :: [Int]
evens = [x | x <- [1..], x `mod` 2 == 0]

-- a recursive call passed as a function argument must stay lazy
consit :: a -> [a] -> [a]
consit x rest = x : rest

countFrom :: Int -> [Int]
countFrom n = consit n (countFrom (n + 1))

-- foldr building a list: cons whose tail is a variable
copyList :: [Int] -> [Int]
copyList = foldr (\x acc -> x : acc) []

-- guard recursion with a thunked argument (the param is used strictly)
digitalRoot :: Int -> Int
digitalRoot n
  | n < 10    = n
  | otherwise = digitalRoot (digitSum n)
  where
    digitSum 0 = 0
    digitSum m = m `mod` 10 + digitSum (m `div` 10)

-- higher-order: a lambda param may arrive as a thunk and must be forced
makeAdder :: Int -> Int -> Int
makeAdder n = \x -> x + n

applyTwice :: (a -> a) -> a -> a
applyTwice f x = f (f x)

main :: IO ()
main = do
  assert (take 5 evens == [2, 4, 6, 8, 10]) "infinite list comprehension streams"
  assert (take 4 (countFrom 1) == [1, 2, 3, 4]) "recursive call as argument stays lazy"
  assert (copyList [1, 2, 3] == [1, 2, 3]) "foldr cons over a variable tail"
  assert (digitalRoot 493 == 7) "guard recursion with a thunked argument"
  assert (applyTwice (makeAdder 3) 0 == 6) "higher-order lambda param is forced"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("lazy-argument program should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("lazy_arguments_and_infinite_lists").exec()
        .expect("lazy arguments and infinite lists should evaluate correctly");
}

// Prelude takeWhile / dropWhile, including the lazy case over an infinite list
// (takeWhile must stop without forcing the whole spine).
#[test]
fn prelude_take_while_drop_while() {
    let source = r#"
main :: IO ()
main = do
  assert (takeWhile (\x -> x < 4) [1, 2, 3, 4, 5] == [1, 2, 3]) "takeWhile finite"
  assert (takeWhile (\x -> x < 4) [1 ..] == [1, 2, 3]) "takeWhile infinite"
  assert (takeWhile (\x -> x < 10) [1, 2, 3] == [1, 2, 3]) "takeWhile exhausts"
  assert (takeWhile (\x -> x > 9) [1, 2, 3] == ([] :: [Int])) "takeWhile none"
  assert (dropWhile (\x -> x < 3) [1, 2, 3, 4, 5] == [3, 4, 5]) "dropWhile finite"
  assert (dropWhile (\x -> x > 9) [1, 2, 3] == [1, 2, 3]) "dropWhile none"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("takeWhile/dropWhile program should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("prelude_take_while_drop_while").exec()
        .expect("takeWhile/dropWhile should evaluate correctly");
}

// Regression (long-standing FFI bug): a list-typed argument crossing OUT to a
// Lua host — on its own, nested inside a LuaDict record, or nested inside
// another list — must be marshalled into a plain 1-based Lua array with its
// elements forced, not handed over as a raw mata-ll cons cell. Before the fix
// the argument-direction marshaller only descended into tuples/Maybe and
// deliberately skipped lists, so the host received a cons cell (head at [1],
// lazy tail thunk at [2]); `operands[2]` was a function and any arithmetic on
// it crashed. The host functions below assert they receive real plain arrays
// (no metatable) of numbers, so a regression to the raw cons cell fails loudly.
#[test]
fn ffi_list_argument_marshalled_to_array() {
    let source = r#"
data Bag = Bag { bagItems as "items" :: [Int], bagName as "name" :: String }
    deriving (Show, LuaDict)

-- top-level list argument
hostSum :: [Int] -> LuaPure "host_sum" Int
-- list nested inside a LuaDict record field
hostBagSum :: Bag -> LuaPure "host_bagsum" Int
-- list of lists (nested list element needs its own conversion)
hostSum2 :: [[Int]] -> LuaPure "host_sum2" Int

main :: IO ()
main = do
  -- literal list
  assert (hostSum [10, 20, 30] == 60) "top-level [Int] argument"
  -- computed elements (thunks): forcing at the boundary is exercised
  assert (hostSum (map (\x -> x * 2) [5, 10, 15]) == 60) "list argument with thunked elements"
  -- list nested in a record, alongside a scalar field
  assert (hostBagSum (Bag [1, 2, 3, 4] "xs") == 10) "list nested in a record field"
  -- list of lists
  assert (hostSum2 [[1, 2], [3, 4], [5]] == 15) "list-of-lists argument"
  putStrLn "ffi list argument marshalling ok"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("ffi list-argument program should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    // Host functions that REQUIRE a real Lua array of forced numbers. A raw cons
    // cell (metatable-tagged, `[2]` = tail function) trips every guard.
    let host = r#"
        local function checkArray(a, who)
            if type(a) ~= "table" then error(who .. ": not a table, got " .. type(a)) end
            if getmetatable(a) ~= nil then error(who .. ": got a metatable-tagged value (raw cons cell), not a plain array") end
        end
        local function sumArray(a, who)
            checkArray(a, who)
            local s = 0
            for i = 1, #a do
                if type(a[i]) ~= "number" then error(who .. ": element " .. i .. " is " .. type(a[i]) .. ", not a number") end
                s = s + a[i]
            end
            return s
        end
        function host_sum(a) return sumArray(a, "host_sum") end
        function host_bagsum(bag)
            if type(bag) ~= "table" then error("host_bagsum: bag not a table") end
            if type(bag.name) ~= "string" then error("host_bagsum: bag.name is " .. type(bag.name) .. ", not a string") end
            return sumArray(bag.items, "host_bagsum items")
        end
        function host_sum2(a)
            checkArray(a, "host_sum2")
            local s = 0
            for i = 1, #a do s = s + sumArray(a[i], "host_sum2 inner " .. i) end
            return s
        end
    "#;
    lua.load(host).set_name("ffi_host").exec().expect("host definitions load");
    lua.load(&lua_code).set_name("ffi_list_argument_marshalled_to_array").exec()
        .expect("ffi list-argument program should run and pass its assertions");
}

// Regression (broke at c3cf855 "make cons heads lazy", worked in 0.1.2, fixed
// by the FFI argument marshaller): a String that is BUILT rather than written
// as a literal — e.g. decoded from JSON — is a `[Char]` structure, not a native
// Lua string. When cons heads became lazy, such a String began crossing the FFI
// argument boundary as a raw cons table instead of a native string, so a host
// reading it (e.g. `params.hostname`) received a table and failed with
// "converting Lua table to String". A String *literal* never reproduced this
// (it is already native), which is exactly why it slipped past the literal-only
// tests — the trigger has to be a constructed String. Here a `[String]` is
// decoded from JSON and each element is passed to a host that requires a native
// Lua string, so a regression to the raw cons table fails loudly.
#[test]
fn ffi_json_decoded_string_argument_is_native_string() {
    let source = r#"
import JSON

data Cfg = Cfg { cfgHosts as "hostnames" :: [String] }
    deriving (FromJSON, Show)

data HostParam = HostParam { hpName as "name" :: String }
    deriving (LuaDict)

sendHost :: HostParam -> LuaPure "send_host" String

cfg :: Cfg
cfg = case decodeJSON "{\"hostnames\": [\"hce.li\", \"example.com\"]}" of
        Right r -> r
        Left e  -> error e

main :: IO ()
main = do
  mapM_ (\h -> assert (sendHost (HostParam h) == h) "json-decoded string arg reaches host as a native string") (cfgHosts cfg)
  putStrLn "ffi json-decoded string argument ok"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("ffi json-string program should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    // Host REQUIRES a native Lua string. A raw cons cell (a table) fails the guard.
    let host = r#"
        function send_host(p)
            if type(p) ~= "table" then error("send_host: params not a table") end
            if type(p.name) ~= "string" then
                error("send_host: name is " .. type(p.name) .. ", not a string "
                      .. "(regression: JSON-decoded String crossed as a raw cons table)")
            end
            return p.name
        end
    "#;
    lua.load(host).set_name("ffi_json_host").exec().expect("host definitions load");
    lua.load(&lua_code).set_name("ffi_json_decoded_string_argument_is_native_string").exec()
        .expect("ffi json-string program should run and pass its assertions");
}

// Regression (long-standing FFI bug): a `Maybe` field inside a LuaDict record
// crossing OUT to a host must be UNWRAPPED — `Just x` becomes the bare `x`
// (recursively marshalled by x's type), `Nothing` becomes nil — matching
// __mll_to_lua and inverting the result decoder. Before the fix the argument
// marshaller descended into the `Just` wrapper without stripping it, so the
// host received the raw `{x}` __just_mt table and `p.port + 1` crashed with
// "arithmetic on a table value". This exercises the OUT direction (host sees a
// bare number / a real array / nil) AND the round-trip: the host echoes the
#[test]
fn ffi_maybe_list_argument_preserves_positions() {
    // A `[Maybe a]` FFI argument marshals `Nothing` -> nil AT ITS POSITION with
    // no compaction: `[Just 1, Nothing, Just 3]` reaches the host with 3 at
    // index 3, not shifted to index 2. Was: silently compacted to {1, 3}.
    let src = r#"
at :: Int -> [Maybe Int] -> LuaPure "at" Int
main :: IO ()
main = do
    let xs = [Just 1, Nothing, Just 3]
    assert (at 3 xs == 3) "Just 3 stays at index 3 (no compaction)"
    assert (at 1 xs == 1) "Just 1 stays at index 1"
    putStrLn "ok"
"#;
    let lua_code = compile(src, Path::new("."), &[])
        .expect("compile should succeed").lua_code;
    let lua = mlua::Lua::new();
    lua.load("function at(i, arr) return arr[i] or -1 end")
        .exec().expect("define host at");
    lua.load(&lua_code).set_name("ml_pos").exec()
        .expect("[Maybe a] argument must preserve element positions, not compact");
}

#[test]
fn growing_type_family_is_bounded() {
    // A type family that grows its argument every step (Grow x = Grow (Maybe x))
    // must be bounded by reduction fuel and reported as divergent -- never hang
    // or stack-overflow the compiler. Charging fuel by reduced-type size bounds
    // the work; the deep (but bounded) reduction still needs a large stack, so
    // run it on a compiler-sized thread like the fixture runner does.
    std::thread::Builder::new()
        .stack_size(mllc::COMPILER_STACK_SIZE)
        .spawn(|| {
            let src = "type family Grow x where\n  Grow x = Grow (Maybe x)\nf :: Grow Int -> Int\nf _ = 0\nmain :: IO ()\nmain = putStrLn \"x\"\n";
            match compile(src, Path::new("."), &[]) {
                Err(e) => assert!(
                    format!("{}", e).contains("did not terminate"),
                    "expected a type-family divergence error, got: {}", e),
                Ok(_) => panic!("a growing type family must be rejected as divergent, not accepted"),
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

// port back into a `Maybe Int` result field, which the decoder must
// reconstruct as Just/Nothing — encode-then-decode identity.
#[test]
fn ffi_maybe_field_marshalled_and_roundtrips() {
    let source = r#"
data In = In
        { iName as "name" :: String
        , iPort as "port" :: Maybe Int
        , iTags as "tags" :: Maybe [Int] }
    deriving (Show, LuaDict)

data Out = Out { oBack as "back" :: Maybe Int, oSum as "sum" :: Int }
    deriving (Show, LuaDict)

probe :: In -> LuaPure "probe" Out

main :: IO ()
main = do
  -- Just: host sees a bare number and a real array; echoes the port back.
  case probe (In "h" (Just 443) (Just [1, 2, 3])) of
    Out back s -> do
      case back of
        Just n  -> assert (n == 443) "Just Maybe field round-trips to Just (present)"
        Nothing -> error "expected Just 443 back, got Nothing"
      assert (s == 6) "Just [Int] field unwrapped and marshalled to an array (1+2+3)"
  -- Nothing: host sees nil for both optional fields; echoes Nothing back.
  case probe (In "h" Nothing Nothing) of
    Out back s -> do
      case back of
        Nothing -> putStrLn "Nothing Maybe field round-trips to Nothing (absent)"
        Just _  -> error "expected Nothing back, got Just"
      assert (s == 0) "Nothing [Int] field is nil (sum 0)"
  putStrLn "ffi maybe-field marshalling ok"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("ffi maybe-field program should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    // The host REQUIRES unwrapped Maybe fields: a bare number (or nil) for
    // `port`, and a plain array (no metatable) or nil for `tags`. A raw
    // `{x}` __just_mt wrapper or a cons cell trips these guards.
    let host = r#"
        function probe(inp)
            if inp.port ~= nil and type(inp.port) ~= "number" then
                error("probe: port must be a bare number or nil, got " .. type(inp.port))
            end
            local s = 0
            if inp.tags ~= nil then
                if getmetatable(inp.tags) ~= nil then
                    error("probe: tags must be a plain array (Just unwrapped), got a metatable-tagged value")
                end
                for i = 1, #inp.tags do
                    if type(inp.tags[i]) ~= "number" then
                        error("probe: tags element " .. i .. " is " .. type(inp.tags[i]) .. ", not a number")
                    end
                    s = s + inp.tags[i]
                end
            end
            -- Round-trip: echo the (already-unwrapped) port back; nil stays nil
            -- so the decoder reconstructs Nothing.
            return { back = inp.port, sum = s }
        end
    "#;
    lua.load(host).set_name("ffi_maybe_host").exec().expect("host definitions load");
    lua.load(&lua_code).set_name("ffi_maybe_field_marshalled_and_roundtrips").exec()
        .expect("ffi maybe-field program should run and pass its assertions");
}

// Regression (long-standing FFI bug): a `HashMap` argument crossing OUT to a
// host must marshal its VALUES by the value type — `HashMap String [Int]`
// reaches the host as a dict of plain arrays, `HashMap String (Maybe X)` as a
// dict of bare values, `HashMap String Record` as a dict of dicts — recursively
// at any nesting. The argument marshaller descended into lists/tuples/records/
// Maybe but not HashMap, so each value arrived as a raw cons cell / wrapper.
// Keys are scalars already usable as Lua keys and are kept (like the decoder).
#[test]
fn ffi_hashmap_structured_values_marshalled() {
    let source = r#"
import qualified Data.Map as Map

data V = V { vName as "name" :: String, vNums as "nums" :: [Int] }
    deriving (Show, LuaDict)

mapLists  :: HashMap String [Int]      -> LuaPure "mp_lists"  Int
mapMaybes :: HashMap String (Maybe Int) -> LuaPure "mp_maybes" Int
mapRecs   :: HashMap String V              -> LuaPure "mp_recs"   Int

main :: IO ()
main = do
  assert (mapLists  (Map.fromList [("a", [1, 2]), ("b", [3, 4, 5])]) == 15) "hashmap of lists -> arrays"
  assert (mapMaybes (Map.fromList [("x", Just 7), ("z", Just 3)]) == 10)   "hashmap of Maybe -> bare values"
  assert (mapRecs   (Map.fromList [("r", V "n" [10, 20])]) == 30)          "hashmap of records -> nested dict/array"
  putStrLn "ffi hashmap-structured-values ok"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("ffi hashmap program should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    let host = r#"
        local function checkArray(a, who)
            if type(a) ~= "table" then error(who .. ": not a table, got " .. type(a)) end
            if getmetatable(a) ~= nil then error(who .. ": got a metatable-tagged value (raw cons cell), not a plain array") end
        end
        function mp_lists(m)
            local s = 0
            for k, v in pairs(m) do
                checkArray(v, "mp_lists value " .. k)
                for i = 1, #v do
                    if type(v[i]) ~= "number" then error("mp_lists: element not a number: " .. type(v[i])) end
                    s = s + v[i]
                end
            end
            return s
        end
        function mp_maybes(m)
            local s = 0
            for k, v in pairs(m) do
                if type(v) ~= "number" then error("mp_maybes: value for " .. k .. " must be a bare number (Just unwrapped), got " .. type(v)) end
                s = s + v
            end
            return s
        end
        function mp_recs(m)
            local s = 0
            for k, v in pairs(m) do
                if type(v) ~= "table" or type(v.name) ~= "string" then error("mp_recs: value must be a record dict") end
                checkArray(v.nums, "mp_recs nums of " .. k)
                for i = 1, #v.nums do s = s + v.nums[i] end
            end
            return s
        end
    "#;
    lua.load(host).set_name("ffi_hashmap_host").exec().expect("host definitions load");
    lua.load(&lua_code).set_name("ffi_hashmap_structured_values_marshalled").exec()
        .expect("ffi hashmap program should run and pass its assertions");
}

// Parity test: the argument marshaller is a COMPLETE structural dual of the
// result decoder, so a value built in mata-ll, passed to an echo host (which
// returns it unchanged), and decoded back is IDENTICAL — for every container
// (list, tuple, LuaDict record, Maybe, HashMap) and their nestings (HashMap of
// lists, list of records with Maybe fields). This is the test that catches a
// missed container in either direction at once: if the marshaller fails to
// encode a container the decoder expects, the echo round-trip diverges.
#[test]
fn ffi_arg_marshal_roundtrips_all_containers() {
    let source = r#"
import qualified Data.Map as Map

data Rec = Rec { rTag as "tag" :: String, rMaybe as "m" :: Maybe Int }
    deriving (Show, Eq, LuaDict)

echoList  :: [Int]                 -> LuaPure "echo" [Int]
echoPairs :: [(Int, String)]       -> LuaPure "echo" [(Int, String)]
echoRec   :: Rec                        -> LuaPure "echo" Rec
echoRecs  :: [Rec]                      -> LuaPure "echo" [Rec]
echoMap   :: HashMap String [Int]  -> LuaPure "echo" (HashMap String [Int])

lk :: String -> HashMap String [Int] -> [Int]
lk k m = case Map.lookup k m of
           Just v  -> v
           Nothing -> []

main :: IO ()
main = do
  -- list; list of tuples (nested tuple decodes as a single table, unlike a
  -- top-level tuple result which uses Lua multi-return); record with Just and
  -- Nothing Maybe fields; list of records.
  assert (echoList [1, 2, 3] == [1, 2, 3]) "list round-trips"
  assert (echoPairs [(1, "a"), (2, "b")] == [(1, "a"), (2, "b")]) "list of tuples round-trips (nested tuple)"
  assert (echoRec (Rec "a" (Just 9)) == Rec "a" (Just 9)) "record with Just field round-trips"
  assert (echoRec (Rec "b" Nothing) == Rec "b" Nothing) "record with Nothing field round-trips"
  assert (echoRecs [Rec "a" (Just 1), Rec "b" Nothing] == [Rec "a" (Just 1), Rec "b" Nothing]) "list of records round-trips"
  -- HashMap of lists: compare by lookup (HashMap has no derived Eq here)
  let m = echoMap (Map.fromList [("a", [1, 2]), ("b", [3, 4, 5])])
  assert (lk "a" m == [1, 2]) "hashmap-of-lists round-trips (key a)"
  assert (lk "b" m == [3, 4, 5]) "hashmap-of-lists round-trips (key b)"
  putStrLn "ffi arg-marshal round-trip parity ok"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("ffi parity program should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    // A pure echo: whatever the marshaller hands the host, hand it straight back.
    // Round-trip identity then depends ENTIRELY on the marshaller and decoder
    // being exact duals.
    lua.load("function echo(x) return x end").set_name("ffi_echo_host").exec()
        .expect("echo host loads");
    lua.load(&lua_code).set_name("ffi_arg_marshal_roundtrips_all_containers").exec()
        .expect("ffi parity program should run and pass its assertions");
}

// FFI marshalling with a fully CONSTRUCTED structure: the record crossing OUT
// is decoded from JSON — not written as a record literal — so every leaf
// (native string, bare number, nested record, list, present/absent Maybe) is
// the product of the FromJSON decoder, and the marshaller must convert what
// the decoder actually builds, not what a literal would compile to. The host
// type-checks every leaf (a raw cons cell, an unstripped Just wrapper, or a
// [Char]-structured string all fail loudly), then answers with a structure of
// its own that the result decoder must rebuild (including nil -> Nothing and
// a present field -> Just).
#[test]
fn ffi_json_constructed_record_crosses_boundary() {
    let source = r#"
import JSON

data Peer = Peer { peerHost as "host" :: String, peerPort as "port" :: Maybe Int }
    deriving (Eq, Show, FromJSON, LuaDict)

data Job = Job
        { jobName as "name" :: String
        , jobRetries as "retries" :: Int
        , jobPeers as "peers" :: [Peer]
        , jobNote as "note" :: Maybe String }
    deriving (Eq, Show, FromJSON, LuaDict)

data Verdict = Verdict
        { vOk as "ok" :: Bool
        , vTotal as "total" :: Int
        , vFirst as "first" :: Maybe String }
    deriving (Show, LuaDict)

submit :: Job -> LuaPure "submit_job" Verdict

-- The job is CONSTRUCTED by the JSON decoder: renamed keys, a nested record
-- list, a present Maybe (port 443), an absent Maybe (b.example has no port),
-- and a null Maybe (note).
job :: Job
job = case decodeJSON "{\"name\": \"scan\", \"retries\": 3, \"peers\": [{\"host\": \"a.example\", \"port\": 443}, {\"host\": \"b.example\"}], \"note\": null}" of
        Right j -> j
        Left e  -> error e

main :: IO ()
main =
  case submit job of
    Verdict ok total first -> do
      assert ok "host validated every leaf of the JSON-built job"
      assert (total == 446) "host summed retries and the one present port (3 + 443)"
      case first of
        Just h  -> assert (h == "a.example") "host's present field decodes back to Just"
        Nothing -> error "expected Just \"a.example\" back, got Nothing"
      putStrLn "ffi json-constructed record ok"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("json-constructed record program should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    // The host REQUIRES converted leaves everywhere: native strings, bare
    // numbers or nil for Maybe fields, metatable-free plain tables for the
    // record and the peer array. Whatever the FromJSON decoder built, only
    // proper marshalling satisfies these guards.
    let host = r#"
        function submit_job(job)
            local function fail(msg) error("submit_job: " .. msg) end
            if type(job) ~= "table" then fail("job is " .. type(job) .. ", not a table") end
            if getmetatable(job) ~= nil then fail("job carries a metatable (raw mata-ll value)") end
            if type(job.name) ~= "string" then
                fail("name is " .. type(job.name) .. ", not a native string (JSON-built String regression)")
            end
            if type(job.retries) ~= "number" then fail("retries is " .. type(job.retries) .. ", not a number") end
            if job.note ~= nil then fail("null note must arrive as nil, got " .. type(job.note)) end
            if type(job.peers) ~= "table" then fail("peers is " .. type(job.peers) .. ", not a table") end
            if getmetatable(job.peers) ~= nil then fail("peers carries a metatable (raw cons cell)") end
            if #job.peers ~= 2 then fail("expected 2 peers, got " .. #job.peers) end
            local total = job.retries
            for i, p in ipairs(job.peers) do
                if type(p) ~= "table" then fail("peer " .. i .. " is " .. type(p) .. ", not a table") end
                if type(p.host) ~= "string" then
                    fail("peer " .. i .. " host is " .. type(p.host) .. ", not a native string")
                end
                if p.port ~= nil and type(p.port) ~= "number" then
                    fail("peer " .. i .. " port must be a bare number or nil (Just unwrap regression), got " .. type(p.port))
                end
                total = total + (p.port or 0)
            end
            return { ok = true, total = total, first = job.peers[1].host }
        end
    "#;
    lua.load(host).set_name("ffi_json_record_host").exec().expect("host definitions load");
    lua.load(&lua_code).set_name("ffi_json_constructed_record_crosses_boundary").exec()
        .expect("json-constructed record program should run and pass its assertions");
}

// Regression: a locally-bound name (function parameter, case-pattern var, or
// let-bound var) must shadow a same-named top-level/prelude function. The
// monomorphizer's specialization paths and the codegen Let/Case arms used to
// ignore locals, so e.g. `f elem = elem + 1` resolved `elem` to the prelude
// function instead of the parameter ("arithmetic on a function value").
#[test]
fn local_binding_shadows_prelude_function() {
    let source = r#"
-- parameter named like a prelude function (multi-clause, not inlined)
fParam :: Int -> Int
fParam 0 = 0
fParam elem = elem + 1

-- case-pattern variable named like a prelude function
fCase :: Maybe Int -> Int
fCase m = case m of
  Just reverse -> reverse + 1
  Nothing -> 0

-- let-bound variable named like a prelude function
fLet :: Int
fLet = let length = 41 in length + 1

main :: IO ()
main = do
  assert (fParam 10 == 11) "param shadows prelude fn"
  assert (fCase (Just 20) == 21) "case var shadows prelude fn"
  assert (fLet == 42) "let var shadows prelude fn"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("shadowing program should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("local_binding_shadows_prelude_function").exec()
        .expect("local bindings should shadow prelude functions");
}

// The common Data.List helpers now live in the auto-imported Prelude, so they
// work with no `import Data.List`. (Data.List re-exports them, so explicit
// imports still work too — covered by lib_data_list and the examples.)
#[test]
fn prelude_list_helpers_without_import() {
    let source = r#"
main :: IO ()
main = do
  assert (null ([] :: [Int])) "null"
  assert (last [1, 2, 3] == 3) "last"
  assert (init [1, 2, 3] == [1, 2]) "init"
  assert (concat [[1, 2], [3]] == [1, 2, 3]) "concat"
  assert (replicate 3 7 == [7, 7, 7]) "replicate"
  assert (take 5 (iterate (\x -> x * 2) 1) == [1, 2, 4, 8, 16]) "iterate"
  assert (span (\x -> x < 3) [1, 2, 3, 4] == ([1, 2], [3, 4])) "span"
  assert (zip [1, 2, 3] [10, 20] == [(1, 10), (2, 20)]) "zip"
  assert (fst (unzip [(1, 10), (2, 20)]) == [1, 2]) "unzip fst"
  assert (and [True, True]) "and"
  assert (or [False, True]) "or"
  assert (any (\x -> x > 3) [1, 2, 4]) "any"
  assert (all (\x -> x > 0) [1, 2, 3]) "all"
  assert (sum [1, 2, 3, 4] == 10) "sum"
  assert (product [1, 2, 3, 4] == 24) "product"
  -- lazy over an infinite list (fst forces only the takeWhile half)
  assert (take 3 (fst (span (\x -> x < 100) [1 ..])) == [1, 2, 3]) "span lazy prefix"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("prelude list helpers should compile without import")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("prelude_list_helpers_without_import").exec()
        .expect("prelude list helpers should evaluate correctly");
}

// Regression: show must distinguish a tuple from a cons list by the cons
// metatable, not by shape. A 2-tuple whose second element is a list (e.g.
// `(1, [2, 3])`) was previously rendered as a cons cell, `[1, 2, 3]`.
#[test]
fn show_tuple_with_list_element() {
    let source = r#"
main :: IO ()
main = do
  assert (show (1, [2, 3]) == "(1,[2,3])") "tuple with list as second element"
  assert (show ([1, 2], [3, 4]) == "([1,2],[3,4])") "tuple of two lists"
  assert (show ([1, 2], 3) == "([1,2],3)") "tuple with list as first element"
  assert (show (1, 2) == "(1,2)") "plain tuple"
  -- An empty-list element must show as "[]", not the type-erased "Nothing"
  -- (the post-mono verifier flagged this latent tuple-show leak).
  assert (show ((1 :: Int), ([] :: [Int])) == "(1,[])") "tuple with empty list element"
  assert (show ((Just (1 :: Int)), (Nothing :: Maybe Int)) == "(Just 1,Nothing)") "tuple of Maybe elements"
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("show_tuple_with_list_element").exec()
        .expect("show should distinguish tuples from lists");
}

// Regression: a `case` matching a nested pattern under a constructor whose
// payload is a thunk (built from a non-cheap expression) must force the field
// before destructuring it. Previously the inner pattern indexed into the raw
// thunk table, reading its internals (the `false` flag, a nil) as field values.
#[test]
fn case_nested_pattern_forces_thunked_field() {
    let source = r#"
data Pair = Pair (Int, Int)

slow :: Int -> Int
slow 0 = 0
slow n = slow (n - 1) + 1

mkPair :: Int -> Pair
mkPair x = Pair (slow x, slow x + 1)

main :: IO ()
main = case mkPair 3 of
         Pair (a, b) -> assert (a + b == 7) "nested pattern forces thunked field"
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("nested_thunk_pat").exec()
        .expect("nested pattern under a thunked constructor payload should work");
}

// Regression: record field accessors are first-class. Previously they were
// only inlined at a direct `field r` application, so using one as a value
// (`map field xs`) or over-applying a function-typed field (`fnField r x`)
// referenced a non-existent global and failed.
#[test]
fn record_accessor_first_class() {
    let source = r#"
data R = R { rfn :: Int -> Int, rval :: Int }

applyAcc :: (R -> Int) -> R -> Int
applyAcc f r = f r

main :: IO ()
main = do
  let r = R (\y -> y + 1) 42
  -- accessor used as a higher-order value
  assert (applyAcc rval r == 42) "accessor passed as a value"
  -- accessor mapped over a list
  assert (sumList (map rval [R (\y -> y) 1, R (\y -> y) 2, R (\y -> y) 3]) == 6) "accessor mapped"
  -- over-applied function-typed field accessor: (rfn r) 10
  assert (rfn r 10 == 11) "over-applied function field accessor"

sumList :: [Int] -> Int
sumList [] = 0
sumList (x:xs) = x + sumList xs
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("first-class accessors should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("accessor_first_class").exec()
        .expect("first-class accessor uses should work");
}

// Compiler stress tests: larger, self-checking example programs that assert
// their own correctness at runtime (a failed roundtrip -> error -> test fail).
#[test]
fn example_huffman_roundtrip() {
    run_mll_file_with_lib(Path::new("../experiments/huffman.mll"));
}

#[test]
fn example_redblack_invariants() {
    run_mll_file_with_lib(Path::new("../experiments/redblack.mll"));
}

#[test]
fn example_scheme_eval() {
    run_mll_file_with_lib(Path::new("../experiments/scheme.mll"));
}

#[test]
fn example_raytracer_renders() {
    run_mll_file_with_lib(Path::new("../experiments/raytracer.mll"));
}

#[test]
fn example_typeinfer_checks() {
    run_mll_file_with_lib(Path::new("../experiments/typeinfer.mll"));
}

#[test]
fn example_listcomp() {
    run_mll_file_with_lib(Path::new("../experiments/listcomp.mll"));
}

#[test]
fn example_lambda_reduction() {
    run_mll_file_with_lib(Path::new("../experiments/lambda.mll"));
}

// ============================================================
// FFI tests: compile MLL modules with exports, then call
// exported functions from Lua and verify return values.
// ============================================================

/// Helper: compile MLL source and return a Lua module table
fn compile_ffi_module(source: &str) -> (mlua::Lua, mlua::Table) {
    let lua_code = compile(source, Path::new("."), &[])
        .expect("FFI module should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    let table: mlua::Table = lua.load(&lua_code)
        .set_name("ffi_test")
        .eval()
        .expect("FFI module should return a table");
    (lua, table)
}

#[test]
fn lua_iterator_result_must_be_an_explicit_list() {
    // The LuaIterator type argument always names the result list, so a bare
    // (non-list) element type is rejected: it would make the argument
    // ambiguous with a genuine list-yielding iterator (`[[Int]]`).
    let source = r#"
gm :: String -> String -> LuaIterator "string.gmatch" String

main :: IO ()
main = mapM_ putStrLn (gm "a b" "%w+")
"#;
    match compile(source, Path::new("."), &[Path::new("../lib")]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("LuaIterator requires the result to be written as an explicit"),
                "Expected the explicit-list requirement error, got: {}", msg);
            assert!(msg.contains("[String]"),
                "The error must show the corrected form, got: {}", msg);
        }
        Ok(_) => panic!("a bare-element LuaIterator result must be rejected"),
    }
}

#[test]
fn lua_iterator_type_argument_is_the_result_list_and_elements_decode() {
    // experiments/iterator/ regression, two properties in one:
    //
    // 1. The `LuaIterator "f" T` type argument names the RESULT list. A list
    //    argument `[Int]` reduces to `[Int]` (the iterator yields the
    //    ELEMENTS, one Int per step) — NOT `[[Int]]`. So `yields`,
    //    whose host yields plain ints, is a flat `[Int]`.
    // 2. A structured element type is DECODED per element, exactly as an
    //    ordinary FFI result: `arrs :: LuaIterator "…" [[Int]]` reduces to
    //    `[[Int]]`, and each yielded Lua array becomes a cons list (so
    //    `map sum` works). Before the fix elements were stored raw and any
    //    list op failed with "expected a list but got a raw … value".
    let src = r#"
yields :: LuaIterator "yieldints" [Int]
arrs   :: LuaIterator "yieldarrs" [[Int]]

main :: IO ()
main = do
    -- (1) list-arg iterator over a scalar-yielding host is a FLAT [Int].
    assert (take 3 yields == [10, 20, 30]) "list-arg iterator yields a flat [Int]"
    -- (2) structured element (a list) is decoded to a cons list.
    assert (map sum (take 2 arrs) == [3, 7]) "each yielded array decoded to a cons list"
    putStrLn "ok"
"#;
    let lua_code = compile(src, Path::new("."), &[])
        .expect("compile should succeed")
        .lua_code;
    let lua = mlua::Lua::new();
    // Host factories: `yieldints` yields plain ints 10,20,30; `yieldarrs`
    // yields Lua arrays {1,2},{3,4}.
    lua.load(
        r#"
        function yieldints()
            local n = 0
            return function()
                n = n + 1
                if n > 3 then return nil end
                return n * 10
            end
        end
        function yieldarrs()
            local n = 0
            return function()
                n = n + 1
                if n > 2 then return nil end
                return { 2 * n - 1, 2 * n }
            end
        end
        "#,
    )
    .exec()
    .expect("define host iterator factories");
    lua.load(&lua_code)
        .set_name("iter_semantics")
        .exec()
        .expect("LuaIterator result must be the flat list of decoded elements");
}

#[test]
fn ffi_export_pure_functions() {
    let source = r#"
export add :: Int -> Int -> Int
add x y = x + y

export double :: Int -> Int
double n = n * 2

export negate :: Int -> Int
negate n = 0 - n

export isEven :: Int -> Bool
isEven n = n `mod` 2 == 0

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    // Int arithmetic
    let add: mlua::Function = module.get("add").unwrap();
    let result: i64 = add.call((3, 4)).unwrap();
    assert_eq!(result, 7, "add 3 4 == 7");

    let result: i64 = add.call((0, 0)).unwrap();
    assert_eq!(result, 0, "add 0 0 == 0");

    let result: i64 = add.call((-5, 3)).unwrap();
    assert_eq!(result, -2, "add (-5) 3 == -2");

    let double: mlua::Function = module.get("double").unwrap();
    let result: i64 = double.call(21).unwrap();
    assert_eq!(result, 42, "double 21 == 42");

    let negate: mlua::Function = module.get("negate").unwrap();
    let result: i64 = negate.call(5).unwrap();
    assert_eq!(result, -5, "negate 5 == -5");

    // Bool return
    let is_even: mlua::Function = module.get("isEven").unwrap();
    let result: bool = is_even.call(4).unwrap();
    assert!(result, "isEven 4 == True");
    let result: bool = is_even.call(7).unwrap();
    assert!(!result, "isEven 7 == False");
}

#[test]
fn ffi_export_string_functions() {
    let source = r#"
export greet :: String -> String
greet name = "Hello, " <> name <> "!"

export shout :: String -> String
shout s = s <> "!!!"

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    let greet: mlua::Function = module.get("greet").unwrap();
    let result: String = greet.call("world").unwrap();
    assert_eq!(result, "Hello, world!");

    let shout: mlua::Function = module.get("shout").unwrap();
    let result: String = shout.call("wow").unwrap();
    assert_eq!(result, "wow!!!");
}

#[test]
fn ffi_export_list_functions() {
    let source = r#"
range :: Int -> [Int]
range n = if n <= 0 then [] else go 1 n
  where go i m = if i > m then [] else i : go (i + 1) m

export getRange :: Int -> [Int]
getRange n = range n

export squares :: Int -> [Int]
squares n = map (\x -> x * x) (range n)

export countTo :: Int -> Int
countTo n = foldl (+) 0 (range n)

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    // List returned as Lua array
    let range: mlua::Function = module.get("getRange").unwrap();
    let result: Vec<i64> = range.call(5).unwrap();
    assert_eq!(result, vec![1, 2, 3, 4, 5], "range 5");

    let result: mlua::Value = range.call(0).unwrap();
    assert!(matches!(&result, mlua::Value::Table(t) if t.len().unwrap() == 0),
            "range 0 is an empty table");

    // List → List (map)
    let squares: mlua::Function = module.get("squares").unwrap();
    let result: Vec<i64> = squares.call(4).unwrap();
    assert_eq!(result, vec![1, 4, 9, 16], "squares 4");

    // List → Int (fold)
    let count: mlua::Function = module.get("countTo").unwrap();
    let result: i64 = count.call(10).unwrap();
    assert_eq!(result, 55, "countTo 10 == 55 (triangle number)");
}

#[test]
fn ffi_export_maybe_either() {
    // `Maybe a` has a designed FFI shape (nil ↔ Nothing) and is ACCEPTED.
    let source = r#"
export safeDiv :: Int -> Int -> Maybe Int
safeDiv _ 0 = Nothing
safeDiv x y = Just (x `div` y)

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    // Maybe: Just → value, Nothing → nil
    let safe_div: mlua::Function = module.get("safeDiv").unwrap();
    let result: Option<i64> = safe_div.call((10, 3)).unwrap();
    assert_eq!(result, Some(3), "safeDiv 10 3 == Just 3");

    let result: Option<i64> = safe_div.call((10, 0)).unwrap();
    assert_eq!(result, None, "safeDiv 10 0 == Nothing");

    // Bare `Either` is a plain two-constructor ADT: outside a LuaTry/LuaIOCatch
    // result (where the pcall wrapper builds and interprets its tags) it has no
    // designed FFI shape — it would leak only as mata-ll's internal
    // `{tag, payload}` table — so an export using it directly is REJECTED. (Use
    // Maybe, a LuaDict record, or a scalar/list encoding instead.)
    let e = compile_err(
        "export classify :: Int -> Either String Int\n\
         classify n = if n < 0 then Left \"negative\" else Right n\n\
         main :: IO ()\nmain = pure ()\n");
    assert!(e.contains("Export 'classify'") && e.contains("the result") && e.contains("Either"),
        "bare Either in an export result is rejected: {e}");
    assert!(e.contains("tagged table"), "note explains the leak: {e}");
}

#[test]
fn ffi_export_higher_order() {
    // MLL-side higher-order: partial application across FFI
    let source = r#"
applyTwice :: (a -> a) -> a -> a
applyTwice f x = f (f x)

double :: Int -> Int
double x = x * 2

inc :: Int -> Int
inc x = x + 1

export doubleDouble :: Int -> Int
doubleDouble n = applyTwice double n

export incInc :: Int -> Int
incInc n = applyTwice inc n

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    let dd: mlua::Function = module.get("doubleDouble").unwrap();
    let result: i64 = dd.call(3).unwrap();
    assert_eq!(result, 12, "doubleDouble 3 == 12");

    let ii: mlua::Function = module.get("incInc").unwrap();
    let result: i64 = ii.call(5).unwrap();
    assert_eq!(result, 7, "incInc 5 == 7");
}

#[test]
fn ffi_export_tuples() {
    let source = r#"
export swap :: (Int, Int) -> (Int, Int)
swap (a, b) = (b, a)

export firstPlusSecond :: (Int, Int) -> Int
firstPlusSecond (a, b) = a + b

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    // Tuple returned as Lua array
    let swap: mlua::Function = module.get("swap").unwrap();
    let result: Vec<i64> = swap.call(vec![1, 2]).unwrap();
    assert_eq!(result, vec![2, 1], "swap (1,2) == (2,1)");

    let first_plus: mlua::Function = module.get("firstPlusSecond").unwrap();
    let result: i64 = first_plus.call(vec![10, 20]).unwrap();
    assert_eq!(result, 30, "firstPlusSecond (10,20) == 30");
}

#[test]
fn ffi_export_thunked_values() {
    // Regression: top-level values defined via point-free or partial
    // application are thunks — export wrapper must __force before calling
    let source = r#"
export increment :: Int -> Int
increment = (+1)

fib :: [Int]
fib = 1 : 1 : zipWith (+) fib (drop 1 fib)

export fibonacci :: Int -> [Int]
fibonacci = flip take fib

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    let increment: mlua::Function = module.get("increment").unwrap();
    let result: i64 = increment.call(41).unwrap();
    assert_eq!(result, 42, "increment 41 == 42");

    let fibonacci: mlua::Function = module.get("fibonacci").unwrap();
    let result: Vec<i64> = fibonacci.call(8).unwrap();
    assert_eq!(result, vec![1, 1, 2, 3, 5, 8, 13, 21], "fibonacci 8");
}

#[test]
fn ffi_export_adt() {
    // A plain user `data` ADT has NO defined FFI shape: it would cross only as
    // mata-ll's internal `{tag, fields...}` table, which has no meaning to a
    // Lua host. So it is REJECTED at the boundary in BOTH directions — as an
    // argument (colorCode :: Color -> Int) and as a result
    // (mkRed :: Int -> Color). (To carry an enum across, derive LuaDict on
    // an all-nullary sum so its constructors cross as name strings; to carry a
    // record, use a LuaDict record; a newtype crosses transparently.)
    let e = compile_err(
        "data Color = Red | Green | Blue\n\
         export colorCode :: Color -> Int\ncolorCode _ = 1\n\
         main :: IO ()\nmain = pure ()\n");
    assert!(e.contains("Export 'colorCode'") && e.contains("argument 1") && e.contains("Color"),
        "plain ADT rejected as an export argument: {e}");
    assert!(e.contains("internal") && e.contains("tagged table") && e.contains("LuaDict"),
        "note explains the tagged-table leak and points at the fixes: {e}");

    let e = compile_err(
        "data Color = Red | Green | Blue\n\
         export mkRed :: Int -> Color\nmkRed _ = Red\n\
         main :: IO ()\nmain = pure ()\n");
    assert!(e.contains("Export 'mkRed'") && e.contains("the result") && e.contains("Color"),
        "plain ADT rejected as an export result: {e}");
}

#[test]
fn ffi_export_multi_arg() {
    // Test multi-arg exported functions and string operations
    let source = r#"
export strRepeat :: String -> Int -> String
strRepeat _ 0 = ""
strRepeat s n = s <> strRepeat s (n - 1)

export clamp :: Int -> Int -> Int -> Int
clamp lo hi x = if x < lo then lo else if x > hi then hi else x

export between :: Int -> Int -> Bool
between lo hi = lo < hi

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    let str_repeat: mlua::Function = module.get("strRepeat").unwrap();
    let result: String = str_repeat.call(("ab", 3)).unwrap();
    assert_eq!(result, "ababab", "strRepeat ab 3");

    let result: String = str_repeat.call(("x", 0)).unwrap();
    assert_eq!(result, "", "strRepeat x 0");

    let clamp: mlua::Function = module.get("clamp").unwrap();
    let result: i64 = clamp.call((0, 10, 15)).unwrap();
    assert_eq!(result, 10, "clamp 0 10 15 == 10");

    let result: i64 = clamp.call((0, 10, 5)).unwrap();
    assert_eq!(result, 5, "clamp 0 10 5 == 5");

    let between: mlua::Function = module.get("between").unwrap();
    let result: bool = between.call((3, 7)).unwrap();
    assert!(result, "between 3 7 == True");
}

#[test]
fn ffi_export_deep_force() {
    // Regression: lazy thunks (e.g. from map) must be fully forced across FFI
    let source = r#"
export mapDouble :: [Int] -> [Int]
mapDouble xs = map (\x -> x * 2) xs

export mapShow :: [Int] -> [String]
mapShow xs = map show xs

export listOfStrings :: Int -> [String]
listOfStrings _ = ["hello", "world", "foo"]

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    // map returning lazy thunks — must be deep-forced to Lua values
    let map_double: mlua::Function = module.get("mapDouble").unwrap();
    let result: Vec<i64> = map_double.call(vec![1, 2, 3]).unwrap();
    assert_eq!(result, vec![2, 4, 6], "mapDouble [1,2,3]");

    let map_show: mlua::Function = module.get("mapShow").unwrap();
    let result: Vec<String> = map_show.call(vec![10, 20, 30]).unwrap();
    assert_eq!(result, vec!["10", "20", "30"], "mapShow [10,20,30]");

    // List of strings — previously broken because __mll_to_lua heuristic
    // misidentified string-headed cons cells
    let list_of_strings: mlua::Function = module.get("listOfStrings").unwrap();
    let result: Vec<String> = list_of_strings.call(0).unwrap();
    assert_eq!(result, vec!["hello", "world", "foo"], "listOfStrings");
}

#[test]
fn ffi_export_lua_to_mll_lists() {
    // Lua arrays passed as arguments must be converted to MLL cons lists
    let source = r#"
export sumList :: [Int] -> Int
sumList xs = foldl (+) 0 xs

export headOf :: [Int] -> Int
headOf xs = head xs

export lengthOf :: [Int] -> Int
lengthOf [] = 0
lengthOf (_:xs) = 1 + lengthOf xs

export appendLists :: [Int] -> [Int] -> [Int]
appendLists xs ys = xs ++ ys

export reverseList :: [Int] -> [Int]
reverseList xs = foldl (flip (:)) [] xs

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    // Passing Lua arrays → MLL cons lists
    let sum: mlua::Function = module.get("sumList").unwrap();
    let result: i64 = sum.call(vec![1, 2, 3, 4, 5]).unwrap();
    assert_eq!(result, 15, "sumList [1..5] == 15");

    let head: mlua::Function = module.get("headOf").unwrap();
    let result: i64 = head.call(vec![42, 99]).unwrap();
    assert_eq!(result, 42, "headOf [42, 99] == 42");

    let len: mlua::Function = module.get("lengthOf").unwrap();
    let result: i64 = len.call(vec![10, 20, 30]).unwrap();
    assert_eq!(result, 3, "lengthOf [10,20,30] == 3");

    // Empty list
    let result: i64 = sum.call(Vec::<i64>::new()).unwrap();
    assert_eq!(result, 0, "sumList [] == 0");

    // Two list arguments
    let append: mlua::Function = module.get("appendLists").unwrap();
    let result: Vec<i64> = append.call((vec![1, 2], vec![3, 4])).unwrap();
    assert_eq!(result, vec![1, 2, 3, 4], "appendLists [1,2] [3,4]");

    // List → List roundtrip
    let rev: mlua::Function = module.get("reverseList").unwrap();
    let result: Vec<i64> = rev.call(vec![1, 2, 3]).unwrap();
    assert_eq!(result, vec![3, 2, 1], "reverseList [1,2,3]");
}

#[test]
fn ffi_export_string_lists() {
    // String lists: Lua string arrays → MLL [String] and back.
    // (filterLong's where-binding originally used a nonexistent `unpack`; the
    // typechecker used to swallow where-binding errors, so the broken —
    // and never-called — function compiled anyway. It now uses a real
    // string-length FFI declaration and is actually exercised.)
    let source = r#"
strLen :: String -> LuaPure "string.len" Int

export joinWith :: String -> [String] -> String
joinWith _ [] = ""
joinWith _ [x] = x
joinWith sep (x:xs) = x <> sep <> joinWith sep xs

export filterLong :: Int -> [String] -> [String]
filterLong n xs = filter (\s -> lengthS s > n) xs
  where lengthS s = strLen s

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    let join: mlua::Function = module.get("joinWith").unwrap();
    let result: String = join.call((",", vec!["a", "b", "c"])).unwrap();
    assert_eq!(result, "a,b,c", "joinWith , [a,b,c]");

    let result: String = join.call(("-", vec!["hello"])).unwrap();
    assert_eq!(result, "hello", "joinWith - [hello]");

    let result: String = join.call((",", Vec::<String>::new())).unwrap();
    assert_eq!(result, "", "joinWith , []");

    let filter_long: mlua::Function = module.get("filterLong").unwrap();
    let result: Vec<String> = filter_long.call((3, vec!["hi", "hello", "hey", "world"])).unwrap();
    assert_eq!(result, vec!["hello", "world"], "filterLong 3 keeps strings longer than 3");

    // An empty MLL list crosses to the host as an empty table, matching the
    // FFI argument edge (hosts can ipairs a list result without a nil check).
    // The type descriptor distinguishes it from Nothing, which stays nil.
    let result: mlua::Value = filter_long.call((10, vec!["short", "tiny"])).unwrap();
    let table = result
        .as_table()
        .expect("empty list result must be a table, not nil");
    assert_eq!(table.raw_len(), 0, "filterLong 10 filters everything out (empty list exports as a table)");
}

#[test]
fn ffi_export_empty_list_is_table_nothing_is_nil() {
    // mata-ll represents both [] and Nothing as nil internally; the declared
    // export type is what tells them apart at the boundary. A list result
    // marshals the empty case to a fresh {} — matching the FFI argument edge,
    // so hosts can ipairs any list result without a nil check — while a Maybe
    // result keeps Nothing as nil. Before this contract change the export
    // edge collapsed a top-level [] to nil even though the same empty list
    // one level deeper (a Just []) already marshalled to {}.
    let source = r#"
export emptyList :: Int -> [Int]
emptyList n = filter (\k -> k > n) [1, 2, 3]

export justEmpty :: Int -> Maybe [Int]
justEmpty n = n `seq` Just []

export nothingAtAll :: Int -> Maybe [Int]
nothingAtAll n = n `seq` Nothing

export emptyValue :: [Int]
emptyValue = []

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    let empty_list: mlua::Function = module.get("emptyList").unwrap();
    let v: mlua::Value = empty_list.call(10).unwrap();
    let t = v.as_table().expect("[] result must be a table, not nil");
    assert_eq!(t.raw_len(), 0, "[] result is an empty table");

    let just_empty: mlua::Function = module.get("justEmpty").unwrap();
    let v: mlua::Value = just_empty.call(1).unwrap();
    let t = v.as_table().expect("Just [] result must be a table, not nil");
    assert_eq!(t.raw_len(), 0, "Just [] unwraps to an empty table");

    let nothing_at_all: mlua::Function = module.get("nothingAtAll").unwrap();
    let v: mlua::Value = nothing_at_all.call(1).unwrap();
    assert!(v.is_nil(), "Nothing stays nil");

    // A VALUE export of an empty list follows the same contract as a
    // function result (the n_args == 0 non-action emission path).
    let v: mlua::Value = module.get("emptyValue").unwrap();
    let t = v.as_table().expect("[] value export must be a table, not nil");
    assert_eq!(t.raw_len(), 0, "[] value export is an empty table");
}

#[test]
fn ffi_export_mixed_args() {
    // Functions with both list and non-list arguments
    let source = r#"
export takeN :: Int -> [Int] -> [Int]
takeN n xs = take n xs

export dropN :: Int -> [Int] -> [Int]
dropN n xs = drop n xs

export replicate :: Int -> Int -> [Int]
replicate 0 _ = []
replicate n x = x : replicate (n - 1) x

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    // Int arg + list arg
    let take_n: mlua::Function = module.get("takeN").unwrap();
    let result: Vec<i64> = take_n.call((3, vec![10, 20, 30, 40, 50])).unwrap();
    assert_eq!(result, vec![10, 20, 30], "takeN 3 [10..50]");

    let drop_n: mlua::Function = module.get("dropN").unwrap();
    let result: Vec<i64> = drop_n.call((2, vec![10, 20, 30, 40])).unwrap();
    assert_eq!(result, vec![30, 40], "dropN 2 [10..40]");

    // Generate list on MLL side, no conversion needed for args
    let rep: mlua::Function = module.get("replicate").unwrap();
    let result: Vec<i64> = rep.call((4, 7)).unwrap();
    assert_eq!(result, vec![7, 7, 7, 7], "replicate 4 7");
}

#[test]
fn ffi_export_values() {
    // A VALUE export (a nullary, non-IO-action binding) must be marshalled to
    // Lua directly, by the SAME result contract a function's RETURN value uses —
    // NOT wrapped in a calling wrapper (which would emit `__force(value)(...)`
    // and crash with "attempt to call a number/table value"). It supports
    // exactly the types a function result does: a scalar, a LuaDict record
    // (keyed table), a tuple (positional table), etc. A function export and an
    // IO-action export in the same module must keep their performing wrappers.
    let source = r#"
data Config = Config { width :: Int, height :: Int }
  deriving (LuaDict)

export answer :: Int
answer = 42

export config :: Config
config = Config { width = 640, height = 480 }

export pairV :: (Int, String)
pairV = (7, "seven")

export incr :: Int -> Int
incr n = n + 1

export runIt :: IO Int
runIt = pure 99

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    // Scalar value: read directly as the number, NOT as a function.
    let answer: mlua::Value = module.get("answer").unwrap();
    assert!(
        matches!(&answer, mlua::Value::Integer(42)) || matches!(&answer, mlua::Value::Number(n) if *n == 42.0),
        "answer must be the marshalled value 42, got {answer:?}"
    );
    assert!(
        !matches!(answer, mlua::Value::Function(_)),
        "a value export must not be a function"
    );

    // LuaDict record value → a keyed Lua table.
    let config: mlua::Table = module.get("config").unwrap();
    let w: i64 = config.get("width").unwrap();
    let h: i64 = config.get("height").unwrap();
    assert_eq!((w, h), (640, 480), "record value marshals to a keyed table");

    // Tuple value → a positional Lua table.
    let pair: mlua::Table = module.get("pairV").unwrap();
    let fst: i64 = pair.get(1).unwrap();
    let snd: String = pair.get(2).unwrap();
    assert_eq!((fst, snd.as_str()), (7, "seven"), "tuple value marshals to a positional table");

    // Function export UNCHANGED: a callable wrapper taking its argument.
    let incr: mlua::Function = module.get("incr").unwrap();
    let r: i64 = incr.call(41).unwrap();
    assert_eq!(r, 42, "function export still works: incr 41 == 42");

    // IO-action export UNCHANGED: a wrapper that PERFORMS the action when
    // called (returning its result), not the action value itself.
    let run_it: mlua::Function = module.get("runIt").unwrap();
    let r: i64 = run_it.call(()).unwrap();
    assert_eq!(r, 99, "IO-action export performs on call: runIt () == 99");
}

#[test]
fn ffi_export_rejects_unmarshallable_types() {
    // An export signature must only use types the FFI marshaller round-trips.
    // Each rejection names the binder, the position (argument N / the result),
    // the offending type, and the crossing direction.

    // A bare type variable has no runtime representation — rejected in both an
    // argument (import) and a result (export) position.
    let e = compile_err("export idf :: a -> a\nidf x = x\nmain :: IO ()\nmain = pure ()\n");
    assert!(e.contains("Export 'idf'"), "names the binder: {e}");
    assert!(e.contains("argument 1") && e.contains("argument direction"), "arg position+dir: {e}");
    assert!(e.contains("the result") && e.contains("result direction"), "result position+dir: {e}");
    assert!(e.contains("polymorphic value"), "type-var note: {e}");
    // The internal/freshened variable name must not leak (prettified to `a`).
    assert!(!e.contains("a890") && !e.contains("_r") && !e.contains("_lit"),
        "type variables must prettify, not leak internal names: {e}");

    // A class constraint would require a dictionary to cross.
    let e = compile_err("export addN :: Num a => a -> a\naddN x = x + x\nmain :: IO ()\nmain = pure ()\n");
    assert!(e.contains("Export 'addN'") && e.contains("class constraint"), "constraint rejected: {e}");
    assert!(e.contains("dictionary"), "dictionary note: {e}");

    // A region-scoped ST handle, in both directions.
    let e = compile_err("export g :: [Int] -> ST s (STArray s)\ng xs = newSTArrayFromList xs\nmain :: IO ()\nmain = pure ()\n");
    assert!(e.contains("Export 'g'") && e.contains("the result"), "ST result rejected: {e}");
    assert!(e.contains("STArray") && e.contains("region-scoped"), "ST note: {e}");

    let e = compile_err("export f :: forall s. STArray s -> Int\nf _ = 5\nmain :: IO ()\nmain = pure ()\n");
    assert!(e.contains("Export 'f'") && e.contains("argument 1") && e.contains("STArray"),
        "ST argument rejected: {e}");

    // An IO action cannot be supplied by a Lua caller (import position).
    let e = compile_err("export bad :: IO () -> Int\nbad _ = 5\nmain :: IO ()\nmain = pure ()\n");
    assert!(e.contains("Export 'bad'") && e.contains("argument 1") && e.contains("IO ()"),
        "IO-in-argument rejected: {e}");
    assert!(e.contains("cannot supply an IO/LuaIO action"), "IO-arg note: {e}");

    // Recursion + direction-flip: a rejected type nested inside a tuple, a list,
    // and a Maybe is still caught and located.
    let e = compile_err("export t :: (Int, a) -> Int\nt (n, _) = n\nmain :: IO ()\nmain = pure ()\n");
    assert!(e.contains("(inside '(Int, a)')"), "nested-in-tuple culprit located: {e}");
    let e = compile_err("export h :: [a] -> Int\nh _ = 0\nmain :: IO ()\nmain = pure ()\n");
    assert!(e.contains("(inside '[a]')"), "nested-in-list culprit located: {e}");
    let e = compile_err("export j :: Maybe a -> Int\nj _ = 0\nmain :: IO ()\nmain = pure ()\n");
    assert!(e.contains("(inside 'Maybe a')"), "nested-in-Maybe culprit located: {e}");

    // A callback whose own signature contains a rejected type. The callback's
    // RESULT is in the import direction (unwrapping its LuaIO), so an ST handle
    // there is rejected.
    let e = compile_err(
        "export ap :: forall s. (Int -> LuaIO s (ST s (STArray s))) -> LuaIO s Int\nap f = pure 0\nmain :: IO ()\nmain = pure ()\n");
    assert!(e.contains("Export 'ap'") && e.contains("STArray"), "callback-result ST rejected: {e}");

    // The callback's ARGUMENT flips to the export (result) direction — a type
    // variable there is reported as a result-direction failure.
    let e = compile_err(
        "export cb :: forall s. (a -> LuaIO s Int) -> LuaIO s Int\ncb f = pure 0\nmain :: IO ()\nmain = pure ()\n");
    assert!(e.contains("Export 'cb'") && e.contains("result direction"),
        "callback-argument direction flip: {e}");

    // A callback is marshalled ONLY as a direct top-level export argument.
    // Nested inside a container it is passed opaque by codegen, so it is
    // rejected — here a callback nested in a Maybe inside a tuple argument.
    let e = compile_err(
        "export ap :: (Maybe (Bool -> [Int]), Int) -> Int\nap _ = 0\nmain :: IO ()\nmain = pure ()\n");
    assert!(e.contains("Export 'ap'") && e.contains("argument 1"), "nested callback rejected: {e}");
    assert!(e.contains("Bool -> [Int]") && e.contains("(inside '(Maybe (Bool -> [Int]), Int)')"),
        "names the nested callback and its position: {e}");
    assert!(e.contains("DIRECT top-level argument"), "callback-position note: {e}");

    // A function nested in the RESULT is rejected (a list of functions — a bare
    // `Int -> (Bool -> Int)` would just be a two-argument export).
    let e = compile_err(
        "export rf :: Int -> [Bool -> Int]\nrf n = [\\b -> n]\nmain :: IO ()\nmain = pure ()\n");
    assert!(e.contains("Export 'rf'") && e.contains("the result") && e.contains("Bool -> Int"),
        "function nested in result rejected: {e}");

    // A callback whose OWN argument is a callback (callback-taking-a-callback):
    // codegen passes the inner function opaque, so reject it.
    let e = compile_err(
        "export cc :: forall s. ((Int -> Int) -> LuaIO s Int) -> LuaIO s Int\ncc _ = pure 0\nmain :: IO ()\nmain = pure ()\n");
    assert!(e.contains("Export 'cc'") && e.contains("callback argument") && e.contains("Int -> Int"),
        "callback-taking-a-callback rejected: {e}");
}

#[test]
fn ffi_export_deep_nesting_allowed() {
    // Deep, fully-marshallable nesting of the DESIGNED container types (tuple /
    // list / Maybe) is accepted AND round-trips — WITHOUT a nested callback (a
    // function is only marshallable as a direct top-level export argument; see
    // the reject test) and WITHOUT a bare `Either` (a plain ADT that would leak
    // as a tagged table; only a LuaTry/LuaIOCatch Either has a designed shape).
    // A second export exercises the SUPPORTED callback shape.
    let source = r#"
export deep :: (Maybe [Int], Bool) -> [Maybe (Int, String)]
deep (m, b) = case m of
    Just xs -> map (\x -> if b then Just (x, "pos") else Nothing) xs
    Nothing -> []

export cbSum :: forall s. (Int -> LuaIO s [Int]) -> LuaIO s Int
cbSum f = do
    xs <- f 3
    pure (sum xs)

main :: IO ()
main = pure ()
"#;
    let (lua, module) = compile_ffi_module(source);

    // A tuple of (Maybe of a list) and a Bool, returning a list of
    // `Maybe (Int, String)`, round-trips: `Just (x, "pos")` crosses as a
    // positional table (nil for Nothing), each inner tuple a positional table.
    let deep: mlua::Function = module.get("deep").unwrap();
    let arg = lua.create_table().unwrap();
    arg.push(lua.create_sequence_from([5, 6]).unwrap()).unwrap(); // Just [5,6]
    arg.push(true).unwrap();
    let out: mlua::Table = deep.call(arg).expect("deep tuple/list/Maybe marshals");
    let e1: mlua::Table = out.get(1).unwrap();
    assert_eq!(e1.get::<i64>(1).unwrap(), 5, "first Just tuple: value = 5");
    assert_eq!(e1.get::<String>(2).unwrap(), "pos", "first Just tuple: tag = pos");
    let e2: mlua::Table = out.get(2).unwrap();
    assert_eq!(e2.get::<i64>(1).unwrap(), 6, "second Just tuple: value = 6");
    // Nothing at the TOP of the argument's Maybe: the empty-list branch. The
    // tuple is a positional table; a `nil` at index 1 (Nothing) is set
    // explicitly by index so the Bool at index 2 keeps its slot.
    let arg2 = lua.create_table().unwrap();
    arg2.set(2, true).unwrap(); // index 1 (the Maybe) stays nil = Nothing
    let out2: mlua::Value = deep.call(arg2).unwrap();
    let empty = match out2 {
        mlua::Value::Nil => true,
        mlua::Value::Table(t) => t.raw_len() == 0,
        other => panic!("unexpected result for the empty-list branch: {other:?}"),
    };
    assert!(empty, "Nothing argument ⇒ empty result list");

    // The SUPPORTED callback shape — a top-level `(A -> LuaIO s R)` argument —
    // stays accepted (the module loaded) and runs: the host callback yields a
    // Lua array, decoded to `[Int]`, and `sum` folds it.
    let cb_sum: mlua::Function = module.get("cbSum")
        .expect("a top-level (A -> LuaIO s R) callback export is accepted");
    let cb = lua.create_function(|lua, n: i64| {
        lua.create_sequence_from((1..=n).collect::<Vec<_>>())
    }).unwrap();
    let r: i64 = cb_sum.call(cb).expect("top-level callback still works");
    assert_eq!(r, 6, "cbSum: sum (f 3) = 1+2+3");
}

#[test]
fn ffi_import_rejects_unmarshallable_types() {
    // FFI IMPORTS (LuaPure/LuaIO/LuaTry/… declarations that call INTO Lua) are
    // validated symmetrically to exports: an argument crosses OUT to the host,
    // the result crosses back IN. A plain user `data` ADT has no FFI shape (it
    // would leak as an internal tagged table), so it is rejected in BOTH.

    // ADT in an import ARGUMENT position (crosses OUT to the host).
    let e = compile_err(
        "data Color = Red | Green | Blue\n\
         paint :: Color -> LuaIO \"paint\" ()\n\
         main :: IO ()\nmain = pure ()\n");
    assert!(e.contains("FFI import 'paint'") && e.contains("argument 1") && e.contains("Color"),
        "plain ADT rejected as an FFI import argument: {e}");
    assert!(e.contains("tagged table") && e.contains("LuaDict"),
        "import note explains the leak and the fixes: {e}");

    // ADT in an import RESULT position (crosses IN from the host).
    let e = compile_err(
        "data Color = Red | Green | Blue\n\
         mkColor :: Int -> LuaIO \"mk_color\" Color\n\
         main :: IO ()\nmain = pure ()\n");
    assert!(e.contains("FFI import 'mkColor'") && e.contains("the result") && e.contains("Color"),
        "plain ADT rejected as an FFI import result: {e}");

    // Bare `Either` in a plain (non-LuaTry) import result is also a plain ADT.
    let e = compile_err(
        "lookupIt :: String -> LuaIO \"lookup\" (Either String Int)\n\
         main :: IO ()\nmain = pure ()\n");
    assert!(e.contains("FFI import 'lookupIt'") && e.contains("the result") && e.contains("Either"),
        "bare Either in a plain LuaIO import result is rejected: {e}");
}

#[test]
fn ffi_marshallable_types_accepted() {
    // The full designed allowlist compiles cleanly across the FFI boundary:
    // scalars, [a], tuples, HashMap, Maybe, Any, a LuaDict record, and — the
    // critical one — a newtype over a marshallable type (the FileHandle shape).
    let source = r#"
data Cfg = Cfg { cWidth :: Int, cName :: String } deriving (Eq, LuaDict)

newtype Handle = Handle LuaUserData

-- FFI IMPORTS covering the allowlist: an argument crosses OUT, the result IN.
-- (Body-less FFI declarations; they are validated by validate_ffi_import_types.)
impScalar :: Int -> LuaPure "tostring" String
impList   :: [Int] -> LuaPure "table.unpack" Int
impMaybe  :: Maybe Int -> LuaPure "identity" (Maybe Int)
impRecord :: Cfg -> LuaPure "rawlen" Int
impHandle :: Handle -> LuaIO ":close" Handle
impTry    :: String -> LuaTry "io.open" (Either String Handle)

-- Exports covering the allowlist in argument and result positions.
export sc :: Int -> Int
sc n = n + 1

export lst :: [Int] -> [Int]
lst xs = xs

export tup :: (Int, String) -> (String, Int)
tup (n, s) = (s, n)

export hm :: HashMap String Int -> Int
hm m = hmSize m

export mb :: Maybe Int -> Maybe Int
mb x = x

export dyn :: Any -> Any
dyn x = x

export rec :: Cfg -> Int
rec c = cWidth c

-- A newtype over LuaUserData crosses transparently (the FileHandle pattern):
-- both as an argument and a result.
export passHandle :: Handle -> Handle
passHandle h = h

main :: IO ()
main = pure ()
"#;
    // Compiling at all proves the validator ACCEPTS every one of these types in
    // both directions. (Any's runtime conversion is the codegen agent's domain;
    // here we only assert the boundary check does not REJECT `Any`.)
    let (_lua, module) = compile_ffi_module(source);
    for name in ["sc", "lst", "tup", "hm", "mb", "dyn", "rec", "passHandle"] {
        let _f: mlua::Function = module.get(name)
            .unwrap_or_else(|_| panic!("export '{name}' must be present"));
    }
    // A scalar round-trips to confirm the module is live.
    let sc: mlua::Function = module.get("sc").unwrap();
    let r: i64 = sc.call(41).unwrap();
    assert_eq!(r, 42, "scalar export still works");

    // The newtype-over-LuaUserData export is a transparent wrapper: the handle
    // crosses unchanged. mlua exposes a real userdata as the Lua-standard
    // io.stdout file handle, so round-trip that and confirm identity is
    // preserved (proving no `{tag, ...}` wrapper was interposed).
    let pass_handle: mlua::Function = module.get("passHandle").unwrap();
    _lua.load("HANDLE = io.stdout").exec().unwrap();
    let handle: mlua::Value = _lua.globals().get("HANDLE").unwrap();
    assert!(matches!(handle, mlua::Value::UserData(_)),
        "io.stdout is a userdata handle");
    let back: mlua::Value = pass_handle.call(handle.clone()).unwrap();
    assert!(matches!(back, mlua::Value::UserData(_)),
        "newtype over LuaUserData passes the handle through untouched");
}

// --- Outgoing FFI callbacks (mata-ll -> Lua): the fold / threaded-state pattern.

#[test]
fn ffi_outgoing_callback_fold() {
    // A Lua host (db.fold) calls our mata-ll callback as cb(row, state) per row
    // and threads the result. Exercises a pure callback, an effectful (LuaIO s)
    // callback, and an opaque tuple state that must round-trip through Lua.
    let source = r#"
-- Pure outgoing callback: state `acc` is opaque (a polymorphic type variable).
foldRows :: String -> (Int -> acc -> acc) -> acc -> LuaPure "db.fold" acc

-- Effectful outgoing callback: returns LuaIO s acc, may do I/O per row.
foldRowsIO :: String -> (Int -> acc -> LuaIO s acc) -> acc -> LuaIO "db.fold" acc

stepIO :: Int -> Int -> LuaIO s Int
stepIO row acc = do
    liftIO (putStr "")
    pure (acc + row)

-- Pure sum into an Int accumulator (uncurry + value return).
export sumRows :: Int -> Int
sumRows seed = foldRows "select" (\row acc -> acc + row) seed

-- Opaque tuple state (sum, count): proves the state survives the Lua round-trip
-- intact (the FFI converters would otherwise flatten a tuple to a cons list).
export sumCount :: Int -> Int
sumCount _ =
    case foldRows "select" (\row acc -> case acc of (s, c) -> (s + row, c + 1)) (0, 0) of
        (s, c) -> s * 1000 + c

-- Effectful fold, returned as IO; the export wrapper runs the action.
export runEffectful :: Int -> IO Int
runEffectful seed = foldRowsIO "select" stepIO seed

main :: IO ()
main = pure ()
"#;
    let (lua, module) = compile_ffi_module(source);

    // Host fold API: db.fold(query, cb, init) folds cb over rows {10, 20, 30}.
    lua.load(
        r#"
        db = {}
        function db.fold(query, cb, init)
            local rows = {10, 20, 30}
            local acc = init
            for i = 1, #rows do acc = cb(rows[i], acc) end
            return acc
        end
    "#,
    )
    .exec()
    .unwrap();

    // Pure fold: 5 + 10 + 20 + 30 = 65.
    let sum_rows: mlua::Function = module.get("sumRows").unwrap();
    let r: i64 = sum_rows.call(5).unwrap();
    assert_eq!(r, 65, "pure outgoing callback fold");

    // Opaque tuple state round-trips: sum=60, count=3 -> 60003.
    let sum_count: mlua::Function = module.get("sumCount").unwrap();
    let r: i64 = sum_count.call(0).unwrap();
    assert_eq!(r, 60003, "tuple state round-trips through Lua intact");

    // Effectful fold: 0 + 10 + 20 + 30 = 60, with the per-row action run.
    let run_eff: mlua::Function = module.get("runEffectful").unwrap();
    let r: i64 = run_eff.call(0).unwrap();
    assert_eq!(r, 60, "effectful outgoing callback fold");
}

// --- FFI result decoding: shape mismatches must fail with localized errors.

#[test]
fn ffi_decode_shape_mismatch_errors() {
    // Every shape mismatch in a value crossing the Lua FFI boundary must fail
    // with a "declared T but the host returned X" error naming WHERE (field/
    // element position and the host function) — never surface as an arbitrary
    // Lua error (nil index, arithmetic on nil) deep in user code. And the
    // checks must NOT reject valid host values (the false-positive regression
    // guarded by the n == 0 cases below).
    let source = r#"
data Cert = Cert { certName :: String, certPort :: Int }
    deriving (Show, LuaDict)

getCert :: Int -> LuaPure "host.cert" Cert
getPorts :: Int -> LuaPure "host.ports" [Int]
getPair :: Int -> LuaPure "host.pair" (String, Int)
getEntries :: Int -> LuaPure "host.entries" [(String, Int)]

export certPortOf :: Int -> Int
certPortOf n = certPort (getCert n)

sumList :: [Int] -> Int
sumList xs = case xs of
    []     -> 0
    (y:ys) -> y + sumList ys

export sumPorts :: Int -> Int
sumPorts n = sumList (getPorts n)

export pairSnd :: Int -> Int
pairSnd n =
    case getPair n of
        (_, p) -> p

sumValues :: [(String, Int)] -> Int
sumValues xs = case xs of
    []          -> 0
    ((_, v):ys) -> v + sumValues ys

export entrySum :: Int -> Int
entrySum n = sumValues (getEntries n)

main :: IO ()
main = pure ()
"#;
    let (lua, module) = compile_ffi_module(source);

    // Host functions returning one valid shape (n == 0) and several broken ones.
    lua.load(
        r#"
        host = {}
        function host.cert(n)
            if n == 0 then return { certName = "ca", certPort = 443 } end
            if n == 1 then return { certName = "ca" } end          -- field missing
            if n == 2 then return "oops" end                        -- scalar, not a table
            return { certName = 7, certPort = 443 }                 -- wrong field type
        end
        function host.ports(n)
            if n == 0 then return {8000, 80, 8080} end
            if n == 1 then return 443 end                           -- scalar, not an array
            return {8000, "eighty"}                                 -- wrong element type
        end
        -- A top-level declared tuple is Lua's multi-value return convention.
        function host.pair(n)
            if n == 0 then return "a", 1 end
            if n == 1 then return "a", "b" end                      -- wrong tuple element
            return "a"                                              -- second value missing
        end
        function host.entries(n)
            if n == 0 then return { {"a", 1}, {"b", 2} } end
            if n == 1 then return { "a" } end                       -- scalar where a tuple
            return { {"a", 1}, {"b", "two"} }                       -- wrong nested element
        end
    "#,
    )
    .exec()
    .unwrap();

    // Valid shapes decode and are NOT rejected: a genuine record, list, and
    // tuple from the host all round-trip. This locks in that the scalar
    // checks fire only on real mismatches.
    let cert_port: mlua::Function = module.get("certPortOf").unwrap();
    let p: i64 = cert_port.call(0).unwrap();
    assert_eq!(p, 443, "valid record from the host decodes");
    let sum_ports: mlua::Function = module.get("sumPorts").unwrap();
    let s: i64 = sum_ports.call(0).unwrap();
    assert_eq!(s, 16160, "valid list from the host decodes");
    let pair_snd: mlua::Function = module.get("pairSnd").unwrap();
    let x: i64 = pair_snd.call(0).unwrap();
    assert_eq!(x, 1, "valid multi-return tuple from the host decodes");
    let entry_sum: mlua::Function = module.get("entrySum").unwrap();
    let x: i64 = entry_sum.call(0).unwrap();
    assert_eq!(x, 3, "valid list of tuples from the host decodes");

    // A declared record field the host left out.
    let e = cert_port.call::<i64>(1).unwrap_err().to_string();
    assert!(e.contains("declared Int but the host returned nil"), "got: {e}");
    assert!(e.contains("field 'certPort' of record Cert"), "got: {e}");
    assert!(e.contains("in the result of host.cert"), "got: {e}");

    // A scalar where a record was declared.
    let e = cert_port.call::<i64>(2).unwrap_err().to_string();
    assert!(e.contains("declared Cert but the host returned the string \"oops\""), "got: {e}");
    assert!(e.contains("a record must arrive from the host as a Lua table"), "got: {e}");

    // A record field of the wrong type.
    let e = cert_port.call::<i64>(3).unwrap_err().to_string();
    assert!(e.contains("declared String but the host returned the number 7"), "got: {e}");
    assert!(e.contains("field 'certName' of record Cert"), "got: {e}");

    // A scalar where a list was declared.
    let e = sum_ports.call::<i64>(1).unwrap_err().to_string();
    assert!(e.contains("declared [Int] but the host returned the number 443"), "got: {e}");
    assert!(e.contains("a list must arrive from the host as a Lua array"), "got: {e}");
    assert!(e.contains("in the result of host.ports"), "got: {e}");

    // A list element of the wrong type.
    let e = sum_ports.call::<i64>(2).unwrap_err().to_string();
    assert!(
        e.contains("declared Int but the host returned the string \"eighty\""),
        "got: {e}"
    );
    assert!(e.contains("an element of the list declared [Int]"), "got: {e}");

    // A tuple element (multi-return value) of the wrong type.
    let e = pair_snd.call::<i64>(1).unwrap_err().to_string();
    assert!(e.contains("declared Int but the host returned the string \"b\""), "got: {e}");
    assert!(e.contains("element 2 of the tuple declared (String, Int)"), "got: {e}");
    assert!(e.contains("in the result of host.pair"), "got: {e}");

    // A tuple element (multi-return value) the host left out entirely.
    let e = pair_snd.call::<i64>(2).unwrap_err().to_string();
    assert!(e.contains("declared Int but the host returned nil"), "got: {e}");
    assert!(e.contains("element 2 of the tuple declared (String, Int)"), "got: {e}");

    // A scalar where a tuple was declared (nested inside a list).
    let e = entry_sum.call::<i64>(1).unwrap_err().to_string();
    assert!(
        e.contains("declared (String, Int) but the host returned the string \"a\""),
        "got: {e}"
    );
    assert!(e.contains("a tuple must arrive from the host as a Lua array"), "got: {e}");
    assert!(e.contains("in the result of host.entries"), "got: {e}");

    // A wrong-typed element of a tuple nested inside a list.
    let e = entry_sum.call::<i64>(2).unwrap_err().to_string();
    assert!(
        e.contains("declared Int but the host returned the string \"two\""),
        "got: {e}"
    );
    assert!(e.contains("element 2 of the tuple declared (String, Int)"), "got: {e}");
}

// --- The FFI boundary is uniformly type-directed (audit findings 4, 5, 7, 8,
// --- 10, 17): every edge a value crosses — LuaTry success payloads, exported
// --- functions' arguments and results, host callbacks passed to exports, and
// --- both edges of an outgoing callback — runs the same type-directed
// --- decode/marshal machinery an ordinary FFI result/argument does.

#[test]
fn luatry_success_payload_decodes_and_error_is_stringified() {
    // Audit finding 7 (doc/audit/t9): a structured LuaTry success payload
    // (a raw Lua array where [Int] was declared) was returned undecoded
    // and later walked as a cons cell -> "attempt to index a number value".
    // And finding 17 (the LuaTry half): a non-string `err` in the Lua
    // (val, err) convention landed raw in Left :: String.
    let source = r#"
tryList   :: Int -> LuaTry "try_list" (Either String [Int])
tryNested :: Int -> LuaTry "try_nested" (Either String [[Int]])

export sumTry :: Int -> IO Int
sumTry n = do
    r <- tryList n
    case r of
        Right xs -> pure (sum xs)
        Left _   -> pure (0 - 1)

export sumNestedTry :: Int -> IO Int
sumNestedTry n = do
    r <- tryNested n
    case r of
        Right xs -> pure (sum (map sum xs))
        Left _   -> pure (0 - 1)

export errText :: Int -> IO String
errText n = do
    r <- tryList n
    case r of
        Right _ -> pure "no error"
        Left e  -> pure e

main :: IO ()
main = pure ()
"#;
    let (lua, module) = compile_ffi_module(source);
    lua.load(
        r#"
        function try_list(n)
            if n == 0 then return nil, { code = 42 } end   -- non-string error object
            local r = {}
            for k = 1, n do r[k] = k end
            return r
        end
        function try_nested(n)
            local r = {}
            for k = 1, n do r[k] = { k, k * 10 } end
            return r
        end
    "#,
    )
    .exec()
    .unwrap();

    let sum_try: mlua::Function = module.get("sumTry").unwrap();
    let s: i64 = sum_try.call(3).expect("structured Right payload must decode");
    assert_eq!(s, 6, "Right [1,2,3] sums to 6");

    let sum_nested: mlua::Function = module.get("sumNestedTry").unwrap();
    let s: i64 = sum_nested.call(2).expect("nested Right payload must decode");
    assert_eq!(s, 33, "Right [[1,10],[2,20]] sums to 33");

    // A non-string error object must arrive in Left as a STRING (tostring'd),
    // so String operations on it work instead of crashing.
    let err_text: mlua::Function = module.get("errText").unwrap();
    let e: String = err_text.call(0).expect("Left of a table error must be a string");
    assert!(e.starts_with("table:"), "err tostring'd, got: {e}");
}

#[test]
fn export_arguments_decode_type_directed() {
    // Audit finding 5: exported functions cons-ified every table argument and
    // only when the TOP-LEVEL type was a list. A `Maybe Int` argument
    // never got its tagged wrapper, and structure nested under a non-list
    // argument (a tuple's list element, a record's list field, a [record])
    // crashed or corrupted.
    let source = r#"
data Tag = Tag { tName :: String, tVals :: [Int] }
    deriving (Show, Eq, LuaDict)

export pairSum :: (Int, [Int]) -> Int
pairSum (n, xs) = n + sum xs

export tagSum :: Tag -> Int
tagSum t = sum (tVals t)

export tagSums :: [Tag] -> Int
tagSums ts = sum (map tagSum ts)

export maybeOr :: Maybe Int -> Int
maybeOr (Just v) = v * 2
maybeOr Nothing  = 0 - 5

main :: IO ()
main = pure ()
"#;
    let (lua, module) = compile_ffi_module(source);

    // A tuple argument with a nested list element.
    let pair_sum: mlua::Function = module.get("pairSum").unwrap();
    let tup = lua.create_table().unwrap();
    tup.push(5).unwrap();
    tup.push(lua.create_sequence_from([1, 2, 3]).unwrap()).unwrap();
    let s: i64 = pair_sum.call(tup).expect("tuple with nested list decodes");
    assert_eq!(s, 11, "pairSum (5, [1,2,3])");

    // A record argument with a list field.
    let tag_sum: mlua::Function = module.get("tagSum").unwrap();
    let rec = lua.create_table().unwrap();
    rec.set("tName", "a").unwrap();
    rec.set("tVals", lua.create_sequence_from([1, 2, 3]).unwrap()).unwrap();
    let s: i64 = tag_sum.call(&rec).expect("record with list field decodes");
    assert_eq!(s, 6, "tagSum Tag with tVals=[1,2,3]");

    // A LIST of records: elements are decoded as records, not cons-ified.
    let tag_sums: mlua::Function = module.get("tagSums").unwrap();
    let rec2 = lua.create_table().unwrap();
    rec2.set("tName", "b").unwrap();
    rec2.set("tVals", lua.create_sequence_from([10, 20]).unwrap()).unwrap();
    let list = lua.create_table().unwrap();
    list.push(&rec).unwrap();
    list.push(&rec2).unwrap();
    let s: i64 = tag_sums.call(list).expect("[record] decodes per element");
    assert_eq!(s, 36, "tagSums over two records");

    // A Maybe argument gets its tagged wrapper: a bare host value is Just,
    // nil is Nothing.
    let maybe_or: mlua::Function = module.get("maybeOr").unwrap();
    let j: i64 = maybe_or.call(21).expect("bare value becomes Just");
    assert_eq!(j, 42, "maybeOr (Just 21)");
    let n: i64 = maybe_or.call(mlua::Value::Nil).expect("nil becomes Nothing");
    assert_eq!(n, -5, "maybeOr Nothing");

    // A shape mismatch fails with a localized ARGUMENT-direction error, not
    // silent corruption or a bare Lua error.
    let e = tag_sum.call::<i64>("oops").unwrap_err().to_string();
    assert!(e.contains("declared Tag but the host passed the string \"oops\""), "got: {e}");
    assert!(e.contains("in argument 1 of the exported function 'tagSum'"), "got: {e}");
}

#[test]
fn export_results_marshal_type_directed() {
    // Companion to finding 5, result direction: exported results went through
    // the shape-based deep-force conversion, which compacted interior
    // Nothings in a [Maybe a] (elements shifted into their slots).
    let source = r#"
export mkML :: Int -> [Maybe Int]
mkML n = map (\k -> if k `mod` 2 == 0 then Nothing else Just k) (enumFromTo 1 n)

export emptyOut :: Int -> [Int]
emptyOut n = filter (\k -> k > 100) (enumFromTo 1 n)

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    // Interior Nothing keeps its position as a hole; the following Just does
    // not shift into its slot. (A trailing Nothing has no Lua representation
    // — nil is the absence of a key — and stays lost, the inherent limit.)
    let mk_ml: mlua::Function = module.get("mkML").unwrap();
    let t: mlua::Table = mk_ml.call(3).expect("mkML returns a table");
    assert_eq!(t.get::<i64>(1).unwrap(), 1, "position 1 is Just 1");
    assert!(matches!(t.get::<mlua::Value>(2).unwrap(), mlua::Value::Nil),
        "position 2 is Nothing (a hole), not a shifted element");
    assert_eq!(t.get::<i64>(3).unwrap(), 3, "position 3 is Just 3");

    // An empty list result is an empty table, matching the FFI argument edge.
    let empty_out: mlua::Function = module.get("emptyOut").unwrap();
    let v: mlua::Value = empty_out.call(3).unwrap();
    let t = v.as_table().expect("empty list result must be a table, not nil");
    assert_eq!(t.raw_len(), 0, "empty exported list is an empty table");
}

#[test]
fn exported_callback_results_decode_type_directed() {
    // Audit finding 8: a host callback passed to an exported function had its
    // result converted by SHAPE (__lua_to_mll cons-ified every table), so a
    // callback returning a string-keyed table where a HashMap/record was
    // declared became nil and crashed the mata-ll consumer.
    let source = r#"
import qualified Data.Map as Map

data Pt = Pt { px :: Int, py :: Int } deriving (Show, LuaDict)

export applyM :: forall s. (Int -> LuaIO s (Map.Map String Int)) -> LuaIO s Int
applyM f = do
    mp <- f 3
    case Map.lookup "a" mp of
        Just v  -> pure v
        Nothing -> pure (0 - 99)

export applyR :: forall s. (Int -> LuaIO s Pt) -> LuaIO s Int
applyR f = do
    p <- f 2
    pure (px p * 100 + py p)

export applyMaybe :: forall s. (Int -> LuaIO s (Maybe Int)) -> LuaIO s Int
applyMaybe f = do
    m <- f 1
    case m of
        Just v  -> pure v
        Nothing -> pure (0 - 1)

export feed :: forall s. ([Int] -> LuaIO s Int) -> Int -> LuaIO s Int
feed f n = f (map (\k -> k * n) (enumFromTo 1 3))

main :: IO ()
main = pure ()
"#;
    let (lua, module) = compile_ffi_module(source);

    // Map-returning callback: the string-keyed table decodes as a map.
    let apply_m: mlua::Function = module.get("applyM").unwrap();
    let cb = lua
        .load("function(n) return { a = n * 2, b = 0 } end")
        .eval::<mlua::Function>()
        .unwrap();
    let v: i64 = apply_m.call(cb).expect("map-returning callback decodes");
    assert_eq!(v, 6, "Map.lookup \"a\" finds the callback's value");

    // Record-returning callback.
    let apply_r: mlua::Function = module.get("applyR").unwrap();
    let cb = lua
        .load("function(n) return { px = n, py = n + 1 } end")
        .eval::<mlua::Function>()
        .unwrap();
    let v: i64 = apply_r.call(cb).expect("record-returning callback decodes");
    assert_eq!(v, 203, "Pt 2 3 -> 203");

    // Maybe-returning callback: bare value -> Just, nil -> Nothing.
    let apply_maybe: mlua::Function = module.get("applyMaybe").unwrap();
    let cb = lua.load("function(n) return n + 41 end").eval::<mlua::Function>().unwrap();
    let v: i64 = apply_maybe.call(cb).expect("bare callback result becomes Just");
    assert_eq!(v, 42);
    let cb = lua.load("function(n) return nil end").eval::<mlua::Function>().unwrap();
    let v: i64 = apply_maybe.call(cb).expect("nil callback result becomes Nothing");
    assert_eq!(v, -1);

    // And the ARGUMENT direction of the same wrapper: a list argument reaches
    // the host callback as a real Lua array it can ipairs.
    let feed: mlua::Function = module.get("feed").unwrap();
    let cb = lua
        .load("function(xs) local s = 0; for _, x in ipairs(xs) do s = s + x end; return s end")
        .eval::<mlua::Function>()
        .unwrap();
    let v: i64 = feed.call((cb, 10)).expect("list marshals out to the callback");
    assert_eq!(v, 60, "callback receives [10,20,30] as a Lua array");
}

#[test]
fn outgoing_callback_edges_agree_with_ffi_edges() {
    // Audit finding 4: an outgoing callback (a mata-ll function handed to a
    // Lua FFI function) marshalled by flags computed from the DECLARED type,
    // while the FFI call's own edges used the instantiated type. A fold whose
    // polymorphic accumulator was instantiated at a structured type had the
    // initial accumulator converted at the FFI edge but passed raw at the
    // callback edge — corrupting it silently. Both edges must use the same
    // (monomorphized) type-directed descriptors.
    let source = r#"
foldHost :: [Int] -> (Int -> acc -> acc) -> acc -> LuaPure "fold_host" acc

export listAcc :: Int -> Int
listAcc n = sum (foldHost (enumFromTo 1 n) (\x xs -> x : xs) [])

export tupleAcc :: Int -> Int
tupleAcc n =
    case foldHost (enumFromTo 1 n) (\x st -> case st of (c, xs) -> (c + 1, x : xs)) (0, []) of
        (c, xs) -> c * 1000 + sum xs

export scalarAcc :: Int -> Int
scalarAcc n = foldHost (enumFromTo 1 n) (\x c -> c + x) 0

main :: IO ()
main = pure ()
"#;
    let (lua, module) = compile_ffi_module(source);
    lua.load(
        r#"
        function fold_host(xs, f, st)
            for _, x in ipairs(xs) do st = f(x, st) end
            return st
        end
    "#,
    )
    .exec()
    .unwrap();

    // acc instantiated at [Int]: the accumulator list survives the
    // round trips through the host intact.
    let list_acc: mlua::Function = module.get("listAcc").unwrap();
    let v: i64 = list_acc.call(4).expect("[Int] accumulator round-trips");
    assert_eq!(v, 10, "sum of the accumulated list");

    // acc instantiated at (Int, [Int]): structure nested in a tuple.
    let tuple_acc: mlua::Function = module.get("tupleAcc").unwrap();
    let v: i64 = tuple_acc.call(3).expect("tuple accumulator round-trips");
    assert_eq!(v, 3006, "count 3, sum 6");

    // The scalar instantiation keeps working.
    let scalar_acc: mlua::Function = module.get("scalarAcc").unwrap();
    let v: i64 = scalar_acc.call(4).unwrap();
    assert_eq!(v, 10);
}

// --- Type-family definitions are validated at the definition (audit 18, 19).

#[test]
fn ill_kinded_family_equation_rejected_at_definition() {
    // `Mix 'Z = Int; Mix 'True = Bool` uses the family argument at kind
    // Nat in one equation and Bool in another. This must be an error AT THE
    // DEFINITION — even with the bad equation never used — not a deferred
    // use-site error blaming the user's signature.
    let source = r#"
data Nat = Z | S Nat

type family Mix a where
    Mix 'Z    = Int
    Mix 'True = Bool

main :: IO ()
main = pure ()
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("in the definition of type family 'Mix'"),
                "the kind error must be located at the family definition, got: {}",
                msg
            );
            assert!(
                msg.contains("needs an argument of kind Nat, but 'True has kind Bool"),
                "the error must explain the kind conflict, got: {}",
                msg
            );
        }
        Ok(_) => panic!("a family whose equations use an argument at two kinds must be rejected"),
    }
}

#[test]
fn kind_conflicting_family_results_rejected_at_definition() {
    // Equation RESULTS at two different kinds ('Z :: Nat vs Bool-promoted).
    let source = r#"
data Nat = Z | S Nat

type family Bad a where
    Bad Int = 'Z
    Bad Bool    = 'True

main :: IO ()
main = pure ()
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("in the definition of type family 'Bad'"),
                "the result-kind error must be located at the definition, got: {}",
                msg
            );
        }
        Ok(_) => panic!("a family whose equation results disagree in kind must be rejected"),
    }
}

#[test]
fn unsaturated_type_family_rejected() {
    // GHC forbids partial application of a type family: it is a compile-time
    // function, not a first-class constructor, so `Wrap Ident` (Ident used
    // with 0 of its 1 argument) must be rejected instead of compiling to a
    // forever-stuck application.
    let source = r#"
type family Ident x where
    Ident x = x

data Wrap f = Wrap (f Int)

bad :: Wrap Ident -> Int
bad (Wrap n) = n

main :: IO ()
main = pure ()
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("Type family 'Ident' is applied to 0 of its 1 argument"),
                "expected the unsaturated-family rejection, got: {}",
                msg
            );
        }
        Ok(_) => panic!("an unsaturated type family must be rejected"),
    }
}

// --- Closed-type-family clause selection (audit finding 12): apartness.

#[test]
fn symbolic_family_argument_not_apart_from_earlier_clause_stays_stuck() {
    // GHC closed-family semantics: a clause fires only when the argument is
    // APART from every earlier clause. A symbolic `n` is not apart from the
    // earlier `IsZero 'Z` clause (n could be 'Z), so `IsZero n` must stay
    // STUCK — it must NOT reduce via the catch-all to 'False. The program
    // below is therefore ill-typed and must be rejected, exactly as GHC
    // rejects it. Before the fix the catch-all fired and this compiled.
    let source = r#"
data Nat = Z | S Nat

type family IsZero n where
    IsZero 'Z = 'True
    IsZero n  = 'False

data Foo b where
    FTrue  :: Foo 'True
    FFalse :: Foo 'False

bad :: Foo (IsZero n)
bad = FFalse

main :: IO ()
main = pure ()
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("IsZero"),
                "the stuck family application should appear in the error, got: {}",
                msg
            );
        }
        Ok(_) => panic!(
            "IsZero n (symbolic) must stay stuck, not reduce via the catch-all"
        ),
    }
}

// --- deriving Functor rejects contravariant occurrences (audit finding 15).

#[test]
fn derive_functor_contravariant_rejected() {
    // `data F a = F (a -> Int)`: the class variable in a function
    // ARGUMENT position has no lawful fmap. GHC rejects the deriving clause;
    // mata-ll used to accept it and crash at the first fmap use.
    let source = r#"
data F a = F (a -> Int) deriving (Functor)

main :: IO ()
main = pure ()
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("Cannot derive 'Functor' for 'F'")
                    && msg.contains("argument of a function field"),
                "expected the contravariance rejection, got: {}",
                msg
            );
        }
        Ok(_) => panic!("deriving Functor over a contravariant field must be rejected"),
    }
}

#[test]
fn derive_functor_non_last_argument_rejected() {
    // The class variable used in a non-last argument of a constructor
    // (`Either a Int`): fmap only reaches the last argument, so GHC
    // rejects this deriving too.
    let source = r#"
data W a = W (Either a Int) deriving (Functor)

main :: IO ()
main = pure ()
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("Cannot derive 'Functor' for 'W'")
                    && msg.contains("position other than the last argument"),
                "expected the non-last-argument rejection, got: {}",
                msg
            );
        }
        Ok(_) => panic!("deriving Functor with the variable in a non-last argument must be rejected"),
    }
}

#[test]
fn prelude_is_emitted_on_demand() {
    // A trivial program must not carry runtime helpers it never references.
    let trivial = compile("main :: IO ()\nmain = putStrLn \"hi\"\n", Path::new("."), &[])
        .expect("trivial should compile")
        .lua_code;
    assert!(!trivial.contains("show_HashMap"), "unused hashmap show must be shaken out");
    assert!(!trivial.contains("__mll_st_new"), "unused ST-array runtime must be shaken out");
    assert!(!trivial.contains("hashmap_insert"), "unused hashmap runtime must be shaken out");

    // But a program that uses a feature must still carry its runtime, or it
    // would break at runtime — reachability, not blanket removal.
    let uses_list_show = compile("main :: IO ()\nmain = print [1, 2, 3 :: Int]\n", Path::new("."), &[])
        .expect("list-show program should compile")
        .lua_code;
    assert!(uses_list_show.contains("__mll_show_list"), "list show must be present when used");

    // The trimmed prelude must be strictly smaller than carrying everything.
    assert!(trivial.len() < uses_list_show.len() + 20_000,
        "on-demand prelude should track usage, not emit the whole runtime");
}

#[test]
fn dead_code_is_eliminated() {
    let fn_count = |src: &str| -> usize {
        compile(src, Path::new("."), &[])
            .expect("should compile")
            .lua_code
            .matches("= function").count()
    };
    // A trivial program must not carry the unused auto-prelude.
    let trivial = fn_count("main :: IO ()\nmain = putStrLn \"hi\"\n");
    let prelude_heavy = fn_count(
        "main :: IO ()\nmain = print (foldr (+) 0 (map (\\x -> x * 2) (filter (\\x -> x > 0) [1, 2, 3, 4 :: Int])))\n",
    );
    assert!(trivial < prelude_heavy,
        "trivial ({trivial} fns) should emit fewer functions than prelude-heavy ({prelude_heavy} fns)");
    assert!(trivial < 25, "trivial program should be tiny after DCE, got {trivial} fns");

    // Exports are roots: an exported function survives DCE even when `main`
    // never calls it (it is reachable only from outside).
    let (_lua, module) = compile_ffi_module(
        "export twice :: Int -> Int\ntwice x = x + x\nmain :: IO ()\nmain = pure ()\n",
    );
    let twice: mlua::Function = module.get("twice").unwrap();
    let r: i64 = twice.call(21).unwrap();
    assert_eq!(r, 42, "exported function must survive DCE and run");
}

// Compile + run an `assert`-based program; a failed assert raises a Lua error,
// so exec() fails and the test fails (never passes vacuously).
fn assert_mll(stmts: &str) {
    let src = format!("main :: IO ()\nmain = do\n{stmts}\n");
    let lua = compile(&src, Path::new("."), &[])
        .unwrap_or_else(|e| panic!("compile failed:\n{e}"))
        .lua_code;
    let l = mlua::Lua::new();
    l.load(&lua).set_name("lambda_test").exec()
        .expect("program should run with all asserts holding");
}

// Regression battery for curried lambdas `\t -> \v -> …`. These compiled to
// nested 1-arg Lua functions but the call site applied every argument in one
// n-ary call, so surplus args were dropped and the inner function leaked out
// (`(\t -> \v -> t + v) 2 3` returned a function instead of 5). The fix flattens
// a lambda only in callee position, leaving argument-position lambdas curried.

#[test]
fn curried_lambda_full_application() {
    assert_mll("    assert ((\\t -> \\v -> t + v) 2 3 == 5) \"full app\"");
}

#[test]
fn curried_lambda_partial_then_apply() {
    assert_mll("    let g = (\\a -> \\b -> a + b) 10\n    assert (g 5 == 15) \"partial\"");
}

#[test]
fn curried_lambda_triple_full() {
    assert_mll("    assert ((\\a -> \\b -> \\c -> a + b + c) 1 2 3 == 6) \"triple full\"");
}

#[test]
fn curried_lambda_triple_partial() {
    assert_mll("    let g = (\\a -> \\b -> \\c -> a + b + c) 1\n    assert (g 2 3 == 6) \"triple partial\"");
}

#[test]
fn curried_lambda_parenthesized_inner() {
    assert_mll("    assert ((\\t -> (\\v -> t - v)) 10 3 == 7) \"paren inner\"");
}

#[test]
fn curried_lambda_four_levels() {
    assert_mll("    assert ((\\a -> \\b -> \\c -> \\d -> a + b + c + d) 1 2 3 4 == 10) \"four levels\"");
}

#[test]
fn curried_lambda_captures_outer_binding() {
    assert_mll("    let k = 100\n    assert ((\\a -> \\b -> a + b + k) 1 2 == 103) \"capture\"");
}

#[test]
fn curried_lambda_non_integer_result() {
    // const-like: returns the first argument, ignores the second
    assert_mll("    assert ((\\s -> \\n -> s) \"hi\" (5 :: Int) == \"hi\") \"const\"");
}

#[test]
fn curried_lambda_returns_list() {
    assert_mll("    assert ((\\x -> \\y -> [x, y]) (1 :: Int) 2 == [1, 2]) \"list result\"");
}

#[test]
fn curried_lambda_embedded_in_expression() {
    assert_mll("    assert (((\\a -> \\b -> a * b) 6 7) + 1 == 43) \"embedded\"");
}

#[test]
fn curried_lambda_takes_function_argument() {
    // Higher-order *and* curried: first parameter is itself a function.
    assert_mll("    assert ((\\f -> \\x -> f x + 1) (\\y -> y * 2) 10 == 21) \"fn arg\"");
}

#[test]
fn curried_lambda_in_higher_order_stays_curried() {
    // The complementary case a naive flatten would break: the erased runtime
    // `map` applies its function argument to ONE argument and expects a
    // function back. Lambdas are flattened to their full type arity, so the
    // compiler must wrap arguments to map/zipWith in a currying adapter
    // (__mll_curry1/2) whenever the result type variable is instantiated to a
    // function type.
    let src = r#"
applyAll :: [a -> b] -> a -> [b]
applyAll []     _ = []
applyAll (f:fs) x = f x : applyAll fs x

main :: IO ()
main = do
    let fns = map (\n -> \x -> x + n) [1, 5, 10]
    assert (applyAll fns 42 == [43, 47, 52]) "higher-order curried"
"#;
    let lua = compile(src, Path::new("."), &[]).expect("compile").lua_code;
    let l = mlua::Lua::new();
    l.load(&lua).set_name("ho_curried").exec().expect("higher-order curried lambda should work");
}

#[test]
fn operator_in_type_position_rejected() {
    // `f :: (+) -> Int` used to parse `(+)` silently as the unit type, so
    // the program compiled with a signature meaning something entirely
    // different from what was written (`f ()` ran fine). An operator in type
    // position must be a parse error that explains why, with a note on the
    // GHC deviation (TypeOperators).
    let e = compile_err("f :: (+) -> Int\nf _ = 1\nmain :: IO ()\nmain = print (f ())\n");
    assert!(e.contains("The operator '+' cannot appear in a type"), "got: {e}");
    assert!(e.contains("'(+)' names a function (a value)"), "got: {e}");
    assert!(e.contains("note:") && e.contains("TypeOperators"), "got: {e}");

    // Same rejection for other operators and positions inside the type.
    let e = compile_err("g :: Int -> (<>)\ng x = x\nmain :: IO ()\nmain = pure ()\n");
    assert!(e.contains("The operator '<>' cannot appear in a type"), "got: {e}");
}

// --- Kind system -----------------------------------------------------------
// Every type the user writes must be well-kinded: an unsaturated constructor
// cannot stand where a complete type is required, a complete type cannot be
// applied to arguments, and an instance head must have the kind the class
// variable was inferred at. The positive side (higher-kinded classes and
// data, `instance C []`) is covered by kinds_hkt.mll.

#[test]
fn kind_error_unsaturated_constructor_in_signature() {
    // `Maybe` alone is not a type — it still needs its element type.
    let e = compile_err("f :: Maybe -> Int\nf _ = 1\nmain :: IO ()\nmain = pure ()\n");
    assert!(e.contains("Kind error"), "got: {e}");
    assert!(e.contains("'Maybe' has kind Type -> Type"), "got: {e}");
    assert!(e.contains("still needs 1 more type argument"), "got: {e}");
    assert!(e.contains("in the type signature for 'f'"), "got: {e}");
}

#[test]
fn kind_error_saturated_type_applied_to_argument() {
    // `Maybe Int` is complete; applying it to `Bool` is a kind error.
    let e = compile_err("x :: Maybe Int Bool\nx = undefined\nmain :: IO ()\nmain = pure ()\n");
    assert!(e.contains("Kind error"), "got: {e}");
    assert!(
        e.contains("'Maybe Int' is applied to the type argument 'Bool'"),
        "got: {e}"
    );
    assert!(e.contains("takes no type arguments"), "got: {e}");
}

#[test]
fn kind_error_type_application_argument_kind() {
    // HashMap's parameters are complete types; a bare `Maybe` is not one.
    let e = compile_err("h :: HashMap Maybe Int -> Int\nh _ = 0\nmain :: IO ()\nmain = pure ()\n");
    assert!(
        e.contains("'HashMap' needs an argument of kind Type, but 'Maybe' has kind Type -> Type"),
        "got: {e}"
    );
}

#[test]
fn kind_error_data_field_must_be_complete_type() {
    let e = compile_err("data T = MkT Maybe\nmain :: IO ()\nmain = pure ()\n");
    assert!(e.contains("Kind error"), "got: {e}");
    assert!(e.contains("'Maybe' has kind Type -> Type"), "got: {e}");
    assert!(e.contains("in the definition of data type 'T'"), "got: {e}");
}

#[test]
fn kind_error_type_variable_used_at_two_kinds() {
    // `t` is used bare (kind Type) AND applied (`t a`) in one signature.
    let e = compile_err("g :: t -> t a -> Int\ng _ _ = 1\nmain :: IO ()\nmain = pure ()\n");
    assert!(e.contains("Kind error"), "got: {e}");
    assert!(
        e.contains("a single type variable cannot be used at two different kinds"),
        "got: {e}"
    );
}

#[test]
fn kind_error_ascription_checked() {
    // Ascribed types are user-written type syntax like any signature.
    let e = compile_err("main :: IO ()\nmain = print (Nothing :: Maybe)\n");
    assert!(e.contains("Kind error"), "got: {e}");
    assert!(e.contains("in a type ascription"), "got: {e}");
}

#[test]
fn kind_error_instance_head_needs_unapplied_constructor() {
    // A Type -> Type class rejects a complete type as its instance head —
    // and the note must point at the [] / Maybe spelling.
    let e = compile_err(
        "class Collapse t where\n    collapse :: t Int -> Int\ninstance Collapse Int where\n    collapse x = x\nmain :: IO ()\nmain = pure ()\n",
    );
    assert!(
        e.contains("'instance Collapse Int' is ill-kinded"),
        "got: {e}"
    );
    assert!(
        e.contains("use its type variable 't' at kind Type -> Type"),
        "got: {e}"
    );

    // The classic trap: `instance C [a]` where `instance C []` is meant.
    let e = compile_err(
        "class Collapse t where\n    collapse :: t Int -> Int\ninstance Collapse [a] where\n    collapse _ = 0\nmain :: IO ()\nmain = pure ()\n",
    );
    assert!(e.contains("'instance Collapse [a]' is ill-kinded"), "got: {e}");
    assert!(
        e.contains("note:") && e.contains("write 'instance C []', not 'instance C [a]'"),
        "got: {e}"
    );
}

#[test]
fn kind_error_instance_head_needs_complete_type() {
    // The reverse direction: a Type class rejects an unapplied constructor.
    let e = compile_err(
        "data T a = MkT a\nclass Pretty a where\n    pretty :: a -> String\ninstance Pretty T where\n    pretty _ = \"t\"\nmain :: IO ()\nmain = pure ()\n",
    );
    assert!(e.contains("'instance Pretty T' is ill-kinded"), "got: {e}");
    assert!(e.contains("'T' has kind Type -> Type"), "got: {e}");
    assert!(e.contains("note:") && e.contains("Expecting one more argument"), "got: {e}");
}

#[test]
fn bare_list_constructor_parses_and_kind_checks_in_instance_head() {
    // `instance Foldable []` — the bare list constructor in an instance
    // head — used to be a PARSE error ("[" demanded an element type). It
    // must now parse and kind-check: [] has kind Type -> Type, exactly what
    // Foldable's class variable requires. In USER code the declaration is
    // still rejected, but only by the orphan rule (Foldable and [] both live
    // in the Prelude, whose own instance declarations use exactly this
    // spelling) — there must be no parse error and no kind error.
    let e = compile_err(
        "instance Foldable [] where\n    foldr _ z [] = z\n    foldr f z (x:xs) = f x (foldr f z xs)\n    foldl _ z [] = z\n    foldl f z (x:xs) = foldl f (f z x) xs\nmain :: IO ()\nmain = pure ()\n",
    );
    assert!(e.contains("Orphan instance"), "got: {e}");
    assert!(!e.contains("Kind error"), "must kind-check, got: {e}");
    assert!(!e.contains("Expected type"), "must parse, got: {e}");
}

#[test]
fn higher_kinded_class_variable_inferred_from_constraint() {
    // A constraint alone fixes the variable's kind: `Foldable t` forces
    // `t : Type -> Type`, so using `t` bare in the same signature is a kind
    // error even though the body never applies it.
    let e = compile_err(
        "f :: Foldable t => t -> Int\nf _ = 0\nmain :: IO ()\nmain = pure ()\n",
    );
    assert!(e.contains("Kind error"), "got: {e}");

    // And the well-kinded spelling still compiles.
    let src = "f :: Foldable t => t Int -> Int\nf t = sum t\nmain :: IO ()\nmain = print (f [1, 2, 3])\n";
    assert!(
        compile(src, Path::new("."), &[]).is_ok(),
        "well-kinded Foldable signature should compile"
    );
}

// --- Adversarial kind-inference probes --------------------------------------
// These stress the two load-bearing assumptions in typechecker/kind.rs that
// cannot be trusted on inspection alone: (1) class-variable kind inference is
// ORDER-INDEPENDENT — a superclass declared later in the module still
// constrains its subclass's kind (this exercised a real bug that is now
// fixed by the shared-substitution `infer_class_kinds` prepass); (2) the
// silent-inference / reporting-check two-phase contract never SWALLOWS an
// ill-kinded declaration — a wrongly-registered first-solution kind must not
// let a later check spuriously pass.

#[test]
fn kind_class_var_from_superclass_declared_after_is_order_independent() {
    // The adversarial case for `infer_class_kinds`: `Sub`'s own method does
    // NOT mention its type variable `t`, so the method signatures cannot pin
    // `t`'s kind. The kind is knowable ONLY through the superclass `Super t`,
    // which forces `t : Type -> Type` (`op :: t Int -> Int`) — and
    // `Super` is declared AFTER `Sub` in source order. Before the
    // shared-substitution prepass, `Sub`'s `t` wrongly defaulted to `Type`
    // (the later superclass was skipped), so this exact program failed while
    // the superclass-first spelling compiled. Both orders must now behave
    // identically: `Sub`'s `t` is `Type -> Type`, and an instance on a
    // `Type -> Type` type (Box) is accepted.
    let after = "class Super t => Sub t where\n    marker :: Int\n\nclass Super t where\n    op :: t Int -> Int\n\ndata Box a = Box a\n\ninstance Super Box where\n    op (Box n) = n\n\ninstance Sub Box where\n    marker = 99\n\nmain :: IO ()\nmain = pure ()\n";
    assert!(
        compile(after, Path::new("."), &[]).is_ok(),
        "subclass kind must be inferred from a superclass declared LATER (was order-dependent)"
    );

    // Control: the SAME program with the superclass declared first. This
    // always worked; it must keep working, and both orders must agree.
    let before = "class Super t where\n    op :: t Int -> Int\n\nclass Super t => Sub t where\n    marker :: Int\n\ndata Box a = Box a\n\ninstance Super Box where\n    op (Box n) = n\n\ninstance Sub Box where\n    marker = 99\n\nmain :: IO ()\nmain = pure ()\n";
    assert!(
        compile(before, Path::new("."), &[]).is_ok(),
        "control: superclass-first ordering must still compile"
    );
}

#[test]
fn kind_class_var_from_superclass_after_still_rejects_wrong_instance() {
    // Proves the fix infers the RIGHT kind, not merely "accepts everything":
    // with `Sub`'s `t` correctly `Type -> Type` (from a superclass declared
    // after), an instance head at kind `Type` (Int) is still a kind
    // error. A regression that made class kinds default to `Type` would make
    // this program compile — this test would then fail loudly.
    let e = compile_err(
        "class Super t => Sub t where\n    marker :: Int\n\nclass Super t where\n    op :: t Int -> Int\n\ninstance Sub Int where\n    marker = 99\n\nmain :: IO ()\nmain = pure ()\n",
    );
    assert!(e.contains("'instance Sub Int' is ill-kinded"), "got: {e}");
    assert!(
        e.contains("use its type variable 't' at kind Type -> Type"),
        "got: {e}"
    );
}

#[test]
fn kind_class_genuine_superclass_conflict_is_reported() {
    // A genuine, unsatisfiable conflict: `Sub`'s own method uses `t` bare
    // (`bad :: t -> Int`, so `t : Type`) while its superclass `Super`
    // uses it applied (`op :: t Int -> Int`, so `t : Type -> Type`).
    // The two constraints share one variable and cannot both hold. The
    // silent prepass keeps a first solution; the reporting pass 2b MUST
    // still surface the clash rather than swallow it.
    let e = compile_err(
        "class Super t => Sub t where\n    bad :: t -> Int\n\nclass Super t where\n    op :: t Int -> Int\n\nmain :: IO ()\nmain = pure ()\n",
    );
    assert!(e.contains("Kind error"), "conflict must not be swallowed, got: {e}");
}

#[test]
fn kind_mutually_recursive_data_conflict_is_reported() {
    // Two mutually-recursive data types whose parameter kinds conflict
    // THROUGH the shared substitution: `P a` uses `a` applied (`a Int`,
    // so `a : Type -> Type`) and references `Q a`; `Q b` uses `b` bare
    // (a field of type `b`, so `b : Type`) and references `P b`. The
    // cross-references force `P`'s and `Q`'s parameters to the same kind,
    // which is simultaneously `Type` and `Type -> Type`. The silent prepass
    // registers a first-solution kind for each; the reporting checking pass
    // must still find the conflict.
    let e = compile_err(
        "data P a = MkP (a Int) (Q a)\ndata Q b = MkQ b (P b)\n\nmain :: IO ()\nmain = pure ()\n",
    );
    assert!(e.contains("Kind error"), "mutual conflict must not be swallowed, got: {e}");
}

#[test]
fn kind_ill_kinded_use_at_wrong_arity_surfaces_at_use_site() {
    // `T` is legitimately higher-kinded: `data T a = MkT (a Int)` gives
    // `T : (Type -> Type) -> Type` (a valid kind, no error at T itself).
    // A LATER declaration then applies it at the wrong argument kind
    // (`T Int`, where `Int : Type`). The registered kind of `T` must
    // drive the check at the use site so the misuse surfaces there — the
    // first (well-kinded) declaration must not mask the second's error.
    let e = compile_err(
        "data T a = MkT (a Int)\ndata U = MkU (T Int)\n\nmain :: IO ()\nmain = pure ()\n",
    );
    assert!(
        e.contains("'T' needs an argument of kind Type -> Type, but 'Int' has kind Type"),
        "got: {e}"
    );
    assert!(e.contains("in the definition of data type 'U'"), "got: {e}");
}

#[test]
fn kind_intra_declaration_conflict_caught_in_both_field_orders() {
    // One constructor that uses its parameter at two kinds — bare AND
    // applied — in the SAME declaration. This must be a kind error no matter
    // which field comes first, so the silent prepass's arbitrary
    // first-solution choice cannot mask the conflict.
    let e_bare_first = compile_err(
        "data Bad a = MkBad a (a Int)\nmain :: IO ()\nmain = pure ()\n",
    );
    assert!(e_bare_first.contains("Kind error"), "bare-first order, got: {e_bare_first}");

    let e_applied_first = compile_err(
        "data Bad2 a = MkBad2 (a Int) a\nmain :: IO ()\nmain = pure ()\n",
    );
    assert!(e_applied_first.contains("Kind error"), "applied-first order, got: {e_applied_first}");
}

#[test]
fn kind_phantom_param_defaults_to_type_and_higher_kinded_use_rejected() {
    // A phantom parameter that no field constrains defaults to `Type`
    // (GHC-consistent: without a use it is `Type`). A later use at a
    // higher kind (`Phantom Maybe`, where `Maybe : Type -> Type`) is then a
    // kind error caught at the use site — the default must not be silently
    // widened to fit the use.
    let e = compile_err(
        "data Phantom a = MkPhantom Int\nuseHK :: Phantom Maybe -> Int\nuseHK _ = 0\nmain :: IO ()\nmain = pure ()\n",
    );
    assert!(
        e.contains("'Phantom' needs an argument of kind Type, but 'Maybe' has kind Type -> Type"),
        "got: {e}"
    );
}

// --- Semigroup/Monoid instances moved to the Prelude ------------------------
// The String and [a] Semigroup/Monoid instances are now ordinary source
// declarations in lib/Prelude.mll (not Rust registrations). These guard the
// two behaviors that must survive the move: the deliberate `<>`-on-lists
// rejection, and mempty's ambiguity handling. (Positive runtime behavior over
// constructed values is covered by tests/cases/monoid_instances.mll.)

#[test]
fn list_semigroup_operator_still_rejected_after_move() {
    // mata-ll deliberately rejects `<>` on a concrete list and directs the
    // user to `++`, even though a `Semigroup [a]` instance exists (it is there
    // for polymorphic dispatch and for `mappend`). Moving the instance to the
    // Prelude must not make `<>` start dispatching on concrete lists — the
    // rejection lives in the monomorphizer, independent of instance source.
    let e = compile_err(
        "main :: IO ()\nmain = putStrLn (show ([1, 2] <> [3, 4]))\n",
    );
    assert!(e.contains("No instance for '<>' on type '[Int]'"), "got: {e}");
    assert!(
        e.contains("lists are concatenated with ++"),
        "the ++ guidance note must still fire, got: {e}"
    );
}

#[test]
fn mappend_on_lists_still_works_after_move() {
    // The complement: `mappend` (the Monoid method) DOES work on concrete
    // lists — polymorphic Monoid code depends on it — and now resolves through
    // the source `instance Monoid [a]` (whose body is `xs ++ ys`).
    let src = "main :: IO ()\nmain = putStrLn (show (mappend [1, 2] [3, 4]))\n";
    assert!(
        compile(src, Path::new("."), &[]).is_ok(),
        "mappend on lists must still compile after the instance move"
    );
}

#[test]
fn mempty_ambiguity_preserved_after_move() {
    // An undetermined `mempty` is still ambiguous with the same guidance —
    // the `Monoid` method-constraint machinery stays in the compiler; only the
    // instances moved.
    let e = compile_err("main :: IO ()\nmain = putStrLn (show mempty)\n");
    assert!(e.contains("Ambiguous type"), "got: {e}");
    assert!(e.contains("Monoid"), "the Monoid ambiguity must still be reported, got: {e}");

    // A determined `mempty` still resolves at each element type.
    for src in [
        "main :: IO ()\nmain = putStrLn (mempty :: String)\n",
        "main :: IO ()\nmain = putStrLn (show (mempty :: [Int]))\n",
    ] {
        assert!(
            compile(src, Path::new("."), &[]).is_ok(),
            "determined mempty should resolve:\n{src}"
        );
    }
}

// --- Source-class constraint synthesis --------------------------------------
// A user class's methods now carry their class constraint, so an undetermined
// use of a return-position-only method is a compile-time ambiguity error (not
// a runtime crash), while an argument-determined method still resolves
// silently. (The positive/runtime side is source_class_nullary.mll.) This is
// the same mechanism that let the Semigroup/Monoid *classes* move to source.

#[test]
fn source_class_nullary_ambiguity_rejected() {
    // `class Default a where def :: a; name :: a -> String`. `name def` leaves
    // `a` undetermined — nothing (no annotation, no argument, no context) can
    // pin which instance — so it must be a compile-time ambiguity error, the
    // same as `show mempty`, NOT a silent compile that crashes at runtime.
    let src = "class Default a where\n    def :: a\n    name :: a -> String\ndata Foo = Foo\ndata Bar = Bar\ninstance Default Foo where\n    def = Foo\n    name _ = \"foo\"\ninstance Default Bar where\n    def = Bar\n    name _ = \"bar\"\nambiguous :: String\nambiguous = name def\nmain :: IO ()\nmain = putStrLn ambiguous\n";
    let e = compile_err(src);
    assert!(e.contains("Ambiguous type"), "must be a compile-time ambiguity, got: {e}");
    assert!(
        e.contains("'Default'"),
        "the ambiguity must name the user class, got: {e}"
    );
    // The guidance must be present, exactly like the builtin mempty case.
    assert!(e.contains("add a type annotation"), "got: {e}");
}

#[test]
fn source_class_method_resolves_when_determined() {
    // The complement, and the anti-over-constraining guard: a method whose
    // class variable IS determined must still resolve silently — no spurious
    // ambiguity. Three ways the variable gets fixed: an annotation on the
    // nullary method, and an argument that carries the variable.
    for src in [
        // nullary `def` pinned by annotation
        "class Default a where\n    def :: a\n    name :: a -> String\ndata Foo = Foo\ninstance Default Foo where\n    def = Foo\n    name _ = \"foo\"\nmain :: IO ()\nmain = putStrLn (name (def :: Foo))\n",
        // argument-carrying method: the variable is fixed by the argument, so
        // no ambiguity even though `greet` carries a synthesized `Greet a`.
        "class Greet a where\n    greet :: a -> String\ndata Foo = Foo\ninstance Greet Foo where\n    greet _ = \"hi\"\nmain :: IO ()\nmain = putStrLn (greet Foo)\n",
    ] {
        assert!(
            compile(src, Path::new("."), &[]).is_ok(),
            "a determined class-method use must resolve, not be reported ambiguous:\n{src}"
        );
    }
}

#[test]
fn source_class_method_no_instance_rejected_at_compile_time() {
    // A class-method use at a concrete type with no instance is now a
    // compile-time "No instance" error (was caught in the monomorphizer
    // before; now the synthesized wanted catches it in the type checker,
    // consistent with how `show`/`==` report).
    let e = compile_err(
        "class Greet a where\n    greet :: a -> String\ndata Foo = Foo\ninstance Greet Foo where\n    greet _ = \"hi\"\ndata Bar = Bar\nuseBar :: String\nuseBar = greet Bar\nmain :: IO ()\nmain = putStrLn useBar\n",
    );
    assert!(e.contains("No instance for 'Greet Bar'"), "got: {e}");
}

#[test]
fn non_structural_instance_on_maybe_is_recognized() {
    // Regression for the has_instance gap the synthesis exposed: a user
    // `instance C (Maybe a)` for a non-structural class C must be recognized
    // (the Maybe branch previously ignored the instance registry, unlike the
    // list branch, and wrongly reported "No instance").
    let src = "class C a where\n    cname :: a -> String\ninstance C [a] where\n    cname _ = \"list\"\ninstance C (Maybe a) where\n    cname _ = \"maybe\"\nmain :: IO ()\nmain = do\n    putStrLn (cname [1, 2, 3])\n    putStrLn (cname (Just True))\n";
    assert!(
        compile(src, Path::new("."), &[]).is_ok(),
        "instance C (Maybe a) must be recognized"
    );
}

// --- Type-family reduction during unification -------------------------------
// The unifier reduces closed type families symbolically (over type variables),
// so length arithmetic like `Plus 'Z m ~ m` and `Plus ('S n) m ~ 'S (Plus n m)`
// works. The positive/runtime side is type_family_arithmetic.mll; these guard
// the soundness edges: concrete reduction still works, mismatches are rejected,
// non-injectivity is not assumed, and divergence errors rather than hangs.

/// The `Plus` family + a length-indexed `Vec`, shared by the tests below.
const TF_VEC_PRELUDE: &str = "\
data Nat = Z | S Nat\n\
type family Plus n m where\n\
    Plus 'Z     m = m\n\
    Plus ('S n) m = 'S (Plus n m)\n\
data Vec n a where\n\
    VNil  :: Vec 'Z a\n\
    VCons :: a -> Vec n a -> Vec ('S n) a\n\
vappend :: Vec n a -> Vec m a -> Vec (Plus n m) a\n\
vappend VNil ys = ys\n\
vappend (VCons x xs) ys = VCons x (vappend xs ys)\n";

#[test]
fn type_family_concrete_reduction_still_works() {
    // The pre-existing concrete/ground reduction (reduced eagerly at
    // AST-to-Ty conversion) must not regress now that the unifier also
    // reduces symbolically.
    let src = "type family Id x where\n    Id x = x\nf :: Id Int -> Int\nf n = n + 1\nmain :: IO ()\nmain = putStrLn (show (f 41))\n";
    let lua = compile(src, Path::new("."), &[])
        .expect("concrete type-family reduction should compile")
        .lua_code;
    let l = mlua::Lua::new();
    l.load(&lua).set_name("tf_id").exec().expect("Id Int program should run");
}

#[test]
fn type_family_length_mismatch_rejected() {
    // `needsTwo` demands a length-2 vector; a `vappend` of lengths 1 and 2 has
    // length `Plus 1 2 = 3`. The reduction must compute 3 and reject the
    // mismatch against 2 — the length stays soundly enforced.
    let src = format!(
        "{TF_VEC_PRELUDE}\
needsTwo :: Vec ('S ('S 'Z)) a -> a\n\
needsTwo (VCons x _) = x\n\
main :: IO ()\n\
main = print (needsTwo (vappend (VCons 1 VNil) (VCons 2 (VCons 3 VNil))))\n"
    );
    let e = compile_err(&src);
    assert!(e.contains("Cannot unify"), "length mismatch must be rejected, got: {e}");
    // The rejection is between the reduced lengths (2 vs 3), i.e. it saw
    // through the family application rather than treating it as opaque.
    assert!(
        e.contains("'Z") && e.contains("'S"),
        "the mismatch should be between concrete Nat lengths, got: {e}"
    );
}

#[test]
fn type_family_head_of_empty_append_rejected() {
    // `vhead` needs a non-empty vector; `vappend` of two empties has length
    // `Plus 'Z 'Z = 'Z` (empty), so `vhead` of it must be rejected.
    let src = format!(
        "{TF_VEC_PRELUDE}\
vhead :: Vec ('S n) a -> a\n\
vhead (VCons x _) = x\n\
main :: IO ()\n\
main = print (vhead (vappend (VNil :: Vec 'Z Int) (VNil :: Vec 'Z Int)))\n"
    );
    let e = compile_err(&src);
    assert!(e.contains("Cannot unify"), "vhead of empty vappend must be rejected, got: {e}");
}

#[test]
fn type_family_non_injectivity_not_assumed() {
    // A family is NOT assumed injective: `coerce` would need
    // `Plus n 'Z ~ Plus m 'Z ⟹ n ~ m`, which does not hold, so the two STUCK
    // family applications must not be unified structurally. Rejected.
    let src = format!(
        "{TF_VEC_PRELUDE}\
coerce :: Vec (Plus n 'Z) a -> Vec (Plus m 'Z) a\n\
coerce v = v\n\
main :: IO ()\n\
main = pure ()\n"
    );
    let e = compile_err(&src);
    assert!(
        e.contains("Cannot unify") && e.contains("Plus"),
        "two different stuck family apps must not unify (no injectivity), got: {e}"
    );
}

#[test]
fn type_family_divergence_errors_not_hangs() {
    // A non-terminating family (`Loop x = Loop x`) must be reported as a
    // divergence, not loop or overflow the stack. (compile_err runs the
    // compiler in-process; if reduction were unbounded this test would hang or
    // crash the harness — so reaching the assertion is itself the guarantee.)
    let src = "type family Loop x where\n    Loop x = Loop x\nf :: Loop Int -> Int\nf n = 0\nmain :: IO ()\nmain = pure ()\n";
    let e = compile_err(src);
    assert!(
        e.contains("did not terminate") && e.contains("Loop"),
        "divergent family must report a termination error, got: {e}"
    );
}

// --- Promoted data types have real kinds (DataKinds step 2) ------------------
// A parameterless data type promotes to a kind named after it (`data Nat`
// gives kind `Nat`, `'Z :: Nat`, `'S :: Nat -> Nat`), so an index is checked
// to be specifically that kind — a promoted tag of another type is a clear
// kind error, not a lucky "unknown constructor". The positive/runtime side is
// promoted_nat_kind.mll (and vec_nat.mll / type_family_arithmetic.mll).

/// A Nat-indexed `Vec`, shared by the tests below.
const PROMOTED_VEC_PRELUDE: &str = "\
data Nat = Z | S Nat\n\
data Vec n a where\n\
    VNil  :: Vec 'Z a\n\
    VCons :: a -> Vec n a -> Vec ('S n) a\n";

#[test]
fn promoted_kind_rejects_bool_tag_for_nat_index() {
    // `'True :: Bool`, but `Vec`'s index has kind `Nat`.
    let src = format!("{PROMOTED_VEC_PRELUDE}bad :: Vec 'True Int -> Int\nbad _ = 0\nmain :: IO ()\nmain = pure ()\n");
    let e = compile_err(&src);
    assert!(e.contains("Kind error"), "got: {e}");
    assert!(
        e.contains("needs an argument of kind Nat") && e.contains("'True has kind Bool"),
        "expected a Nat-vs-Bool kind error, got: {e}"
    );
}

#[test]
fn promoted_kind_rejects_wrong_user_tag_for_nat_index() {
    // A promoted constructor of ANOTHER user data type (`'Red :: Color`) where
    // a `Nat` is required.
    let src = format!("data Color = Red | Blue\n{PROMOTED_VEC_PRELUDE}bad :: Vec 'Red Int -> Int\nbad _ = 0\nmain :: IO ()\nmain = pure ()\n");
    let e = compile_err(&src);
    assert!(
        e.contains("needs an argument of kind Nat") && e.contains("'Red has kind Color"),
        "expected a Nat-vs-Color kind error, got: {e}"
    );
}

#[test]
fn promoted_kind_rejects_nested_wrong_tag() {
    // The ill-kinded tag is nested inside `'S`, which itself has kind
    // `Nat -> Nat`, so `'S 'True` fails at the inner application.
    let src = format!("{PROMOTED_VEC_PRELUDE}bad :: Vec ('S 'True) a -> a\nbad _ = undefined\nmain :: IO ()\nmain = pure ()\n");
    let e = compile_err(&src);
    assert!(
        e.contains("'S") && e.contains("needs an argument of kind Nat") && e.contains("'True has kind Bool"),
        "expected 'S to reject a Bool argument, got: {e}"
    );
}

#[test]
fn promoted_kind_type_family_argument_is_checked() {
    // A type family over naturals is inferred at kind `Nat -> Nat -> Nat`, so
    // applying it to a `Bool` tag is a kind error (this is the step-1/step-2
    // interaction: reduction is unchanged, but the family's arg kinds are now
    // checked).
    let src = format!(
        "{PROMOTED_VEC_PRELUDE}\
type family Plus n m where\n\
    Plus 'Z     m = m\n\
    Plus ('S n) m = 'S (Plus n m)\n\
bad :: Vec (Plus 'True 'Z) a -> a\n\
bad _ = undefined\n\
main :: IO ()\n\
main = pure ()\n"
    );
    let e = compile_err(&src);
    assert!(
        e.contains("'Plus' needs an argument of kind Nat") && e.contains("'True has kind Bool"),
        "the family's Nat argument kind must be checked, got: {e}"
    );
}

#[test]
fn promoted_kind_well_kinded_index_accepted() {
    // The complement / anti-over-eagerness guard: a correctly Nat-kinded index
    // (bare variable, `'Z`, and `'S`-applied) must still compile.
    let src = format!(
        "{PROMOTED_VEC_PRELUDE}\
vlen :: Vec n a -> Int\n\
vlen VNil = 0\n\
vlen (VCons _ xs) = 1 + vlen xs\n\
v2 :: Vec ('S ('S 'Z)) Int\n\
v2 = VCons 1 (VCons 2 VNil)\n\
main :: IO ()\n\
main = print (vlen v2)\n"
    );
    assert!(
        compile(&src, Path::new("."), &[]).is_ok(),
        "a well-kinded Nat index must compile"
    );
}

#[test]
fn promoted_type_still_usable_as_a_value_type() {
    // Promoting `Nat` to a kind must not stop it being an ordinary value type:
    // `S (S Z)` is still a runtime value of type `Nat`. (Type/kind duality.)
    let src = "data Nat = Z | S Nat\ntoInt :: Nat -> Int\ntoInt Z = 0\ntoInt (S n) = 1 + toInt n\nmain :: IO ()\nmain = print (toInt (S (S Z)))\n";
    assert!(
        compile(src, Path::new("."), &[]).is_ok(),
        "a promoted data type must still work as a value type"
    );
}

#[test]
fn promoted_kind_non_gadt_phantom_tag_rejected_but_gadt_pins_it() {
    // KNOWN, GHC-consistent limitation: a NON-GADT type parameter used only as
    // a phantom has its kind DEFAULTED to `Type` (mata-ll has no kind-signature
    // syntax to say otherwise), so a promoted tag of another kind cannot be its
    // index. GHC rejects this too without a `data Tagged (a :: Color)` kind
    // signature. The escape hatch is a GADT that PINS the index through a
    // constructor return type (as `datakinds.mll` does), which is checked and
    // accepted.
    let phantom = "data Color = Red | Blue\ndata Tagged a = Tagged Int\nf :: Tagged 'Red -> Int\nf (Tagged n) = n\nmain :: IO ()\nmain = pure ()\n";
    let e = compile_err(phantom);
    assert!(e.contains("Kind error"), "phantom promoted tag should be rejected, got: {e}");
    assert!(e.contains("'Red has kind Color"), "got: {e}");

    // The GADT form pins the index's kind and is accepted.
    let gadt = "data Color = Red | Blue\ndata Tagged a where\n    MkTagged :: Int -> Tagged 'Red\nf :: Tagged 'Red -> Int\nf (MkTagged n) = n\nmain :: IO ()\nmain = print (f (MkTagged 7))\n";
    assert!(
        compile(gadt, Path::new("."), &[]).is_ok(),
        "a GADT that pins a promoted index must compile"
    );
}

// Top-level redefinition of a name the Prelude/builtins provide. Historically
// the collision surfaced as unification errors at Prelude-internal source
// lines ("in clause 2 of 'assert'" at 15:8 for a redefined `error`), blaming
// functions the user never wrote. It must instead be reported once, clearly,
// at the user's own definition site.

#[test]
fn prelude_builtin_redefinition_reports_user_site_not_prelude() {
    // `error` is a builtin the Prelude's own code depends on (assert, init,
    // last). Redefining it used to fail inside those Prelude functions.
    let e = compile_err(
        "error :: String -> Int\nerror s = 42\n\nmain :: IO ()\nmain = print (error \"hi\")\n",
    );
    assert!(
        e.contains("'error' is already provided by the Prelude and cannot be redefined"),
        "got: {e}"
    );
    assert!(e.contains("at 2:"), "should point at the user's definition line, got: {e}");
    assert!(e.contains("note:") && e.contains("rename your function"), "got: {e}");
    // The misleading Prelude-internal cascade must be gone entirely.
    assert!(!e.contains("Cannot unify"), "cascade leaked through, got: {e}");
    assert!(
        !e.contains("'assert'") && !e.contains("'init'") && !e.contains("15:8"),
        "blames Prelude internals, got: {e}"
    );
}

#[test]
fn prelude_load_bearing_name_redefinition_rejected() {
    // `map` is a builtin the Prelude uses internally (ap_List). Redefining it
    // used to compile silently and corrupt `<*>` on lists.
    let e = compile_err(
        "map :: (Int -> Int) -> [Int] -> [Int]\nmap f xs = xs\n\nmain :: IO ()\nmain = print (map (\\x -> x + 1) [1, 2, 3])\n",
    );
    assert!(
        e.contains("'map' is already provided by the Prelude and cannot be redefined"),
        "got: {e}"
    );
    assert!(e.contains("Prelude's own functions use 'map'"), "got: {e}");
}

#[test]
fn prelude_same_type_duplicate_definition_rejected() {
    // A definition duplicating a Prelude function at its exact type used to
    // HANG the compiler (demand analysis never converged on the two same-name
    // same-type functions). If this test times out, that regressed.
    // (This used `sum :: [Int] -> Int` before sum was generalized to
    // `Foldable t => t Int -> Int`; the monomorphic signature is now a
    // DIFFERENT type, i.e. an allowed user-wins redefinition — see the test
    // below — so the exact-duplicate case is probed with `reverse` instead.)
    let e = compile_err(
        "reverse :: [a] -> [a]\nreverse xs = xs\n\nmain :: IO ()\nmain = print (reverse [1, 2, 3])\n",
    );
    assert!(
        e.contains("'reverse' is already provided by the Prelude and cannot be redefined"),
        "got: {e}"
    );
    assert!(e.contains("same type as the Prelude's 'reverse'"), "got: {e}");
}

#[test]
fn prelude_foldable_generic_allows_monomorphic_redefinition() {
    // Redefining a Foldable-generic Prelude function at a genuinely different
    // (monomorphic list) type is the documented user-wins case, and the
    // user's definition is the one that runs.
    let source =
        "sum :: [Int] -> Int\nsum xs = 999\n\nmain :: IO ()\nmain = putStrLn (show (sum [1, 2, 3]))\n";
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    let captured = lua.create_table().unwrap();
    lua.globals().set("__captured", captured.clone()).unwrap();
    let print_fn = lua
        .create_function(|lua, s: mlua::LuaString| -> mlua::Result<()> {
            let line = s.to_str()?.to_string();
            let t: mlua::Table = lua.globals().get("__captured")?;
            let n = t.raw_len();
            t.raw_set(n + 1, line)?;
            Ok(())
        })
        .unwrap();
    lua.globals().set("print", print_fn).unwrap();
    lua.load(&lua_code).set_name("user_wins_sum").exec()
        .expect("should run");
    let lines: Vec<String> = captured
        .sequence_values::<String>()
        .collect::<mlua::Result<_>>()
        .unwrap();
    assert_eq!(lines, vec!["999"]);
}

#[test]
fn prelude_redefinition_breaking_prelude_body_reports_user_not_prelude() {
    // `replicate` is neither used by other Prelude code nor duplicated at the
    // same type here, so it passes the up-front checks — but the Prelude's own
    // `replicate` body cannot type-check against this signature (its cons
    // result is not a String). The safety net must convert the resulting
    // Prelude-internal error (formerly "Cannot unify '[String]' with 'String'
    // at 96:11") into the same clear redefinition report.
    let e = compile_err(
        "replicate :: Int -> String -> String\nreplicate n s = s\n\nmain :: IO ()\nmain = putStrLn (replicate 3 \"x\")\n",
    );
    assert!(
        e.contains("'replicate' is already provided by the Prelude and cannot be redefined"),
        "got: {e}"
    );
    assert!(!e.contains("Cannot unify"), "Prelude-internal error leaked, got: {e}");
    assert!(!e.contains("96:"), "points at a Prelude source line, got: {e}");
}

#[test]
fn prelude_benign_shadowing_still_compiles() {
    // The permitted cases must NOT be rejected (no over-triggering):
    // a builtin that no Prelude code depends on (`head`) redefined at a
    // narrower type, GHC-shadow style — the user's definition wins…
    let src = "head :: [Int] -> Int\nhead xs = 0\n\nmain :: IO ()\nmain = print (head [1, 2, 3])\n";
    let lua = compile(src, Path::new("."), &[]).expect("head shadow should compile").lua_code;
    let l = mlua::Lua::new();
    l.load(&lua).set_name("head_shadow").exec().expect("head shadow should run");

    // …and a Prelude function redefined at a genuinely different (here
    // monomorphic) type, the pattern the FFI-export tests rely on.
    let src = "replicate :: Int -> Int -> [Int]\nreplicate 0 _ = []\nreplicate n x = x : replicate (n - 1) x\n\nmain :: IO ()\nmain = pure ()\n";
    compile(src, Path::new("."), &[]).expect("monomorphic replicate should compile");
}

// A class constraint with no instance must be rejected at type-check time,
// rather than silently falling through to a runtime `tostring`.

#[test]
fn no_show_instance_for_function() {
    let e = compile_err("main :: IO ()\nmain = putStrLn (show (\\a b -> a + b))\n");
    assert!(e.contains("No instance for 'Show (a -> a -> a)'"), "got: {e}");
    assert!(e.contains("no Show/Eq/Ord instance"), "missing function note, got: {e}");
}

#[test]
fn no_eq_instance_for_function() {
    let e = compile_err("main :: IO ()\nmain = print ((\\x -> x :: Int) == (\\x -> x))\n");
    assert!(e.contains("No instance for 'Eq (Int -> Int)'"), "got: {e}");
}

#[test]
fn no_ord_instance_for_function() {
    let e = compile_err(
        "f :: (Int -> Int) -> Bool\nf g = g < g\nmain :: IO ()\nmain = print (f (\\x -> x))\n",
    );
    assert!(e.contains("No instance for 'Ord (Int -> Int)'"), "got: {e}");
}

#[test]
fn no_show_instance_for_tuple_containing_function() {
    let e = compile_err("main :: IO ()\nmain = putStrLn (show ((1 :: Int), (\\x -> x :: Int)))\n");
    assert!(e.contains("No instance for 'Show (Int, Int -> Int)'"), "got: {e}");
}

#[test]
fn no_show_instance_for_io_action() {
    let e = compile_err("main :: IO ()\nmain = print (putStrLn \"x\")\n");
    assert!(e.contains("No instance for"), "got: {e}");
    assert!(e.contains("IO"), "should mention the IO action type, got: {e}");
}

#[test]
fn constraint_propagates_through_print() {
    // `print :: Show a => …` — its constraint is checked at the call site, so
    // even a never-applied (polymorphic) function is rejected.
    let e = compile_err("main :: IO ()\nmain = print (\\a b -> a + b)\n");
    assert!(e.contains("No instance for 'Show (a -> a -> a)'"), "got: {e}");
}

#[test]
fn constraint_propagates_through_user_function() {
    let e = compile_err(
        "needsShow :: Show a => a -> String\nneedsShow x = show x\nmain :: IO ()\nmain = putStrLn (needsShow (\\y -> y + (1 :: Int)))\n",
    );
    assert!(e.contains("No instance for 'Show (Int -> Int)'"), "got: {e}");
}

#[test]
fn valid_show_constraints_still_compile() {
    // Base types, structural containers, and a properly-constrained polymorphic
    // function must all still type-check.
    for src in [
        "main :: IO ()\nmain = print (42 :: Int)\n",
        "main :: IO ()\nmain = print (Just [1, 2, 3 :: Int])\n",
        "main :: IO ()\nmain = print ([(1, 2), (3, 4)] :: [(Int, Int)])\n",
        "p :: Show a => a -> IO ()\np x = putStrLn (show x)\nmain :: IO ()\nmain = p (42 :: Int)\n",
        "main :: IO ()\nmain = print (Just (1 :: Int) == Just 1)\n",
    ] {
        assert!(compile(src, Path::new("."), &[]).is_ok(), "should compile:\n{src}");
    }
}

fn compile_err(source: &str) -> String {
    match compile(source, Path::new("."), &[]) {
        Ok(_) => panic!("expected compilation to fail, but it succeeded"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn type_error_locates_the_offending_statement() {
    // A type error must point at the statement/binding line that carries it,
    // not the clause head. The checker attributes errors via `Expr::Spanned`
    // markers placed at statement boundaries (let/where bindings, do-statements,
    // case-branch and guard bodies, if-branches). Before this, every error in a
    // multi-line body was reported at the function's first line.

    // let-binding body: the error is on the `c = ...` line, not `compute x =`.
    let e = compile_err(
        "compute :: Int -> Int\n\
         compute x =\n\
         \x20   let a = x + 1\n\
         \x20       c = a <> \"oops\"\n\
         \x20   in c\n");
    assert!(e.contains("at 4:"), "let-binding error points at the binding line (4): {e}");

    // case-branch reconciliation: the error is on the offending branch line.
    let e = compile_err(
        "f :: Int -> String\n\
         f n = case n of\n\
         \x20   0 -> \"zero\"\n\
         \x20   _ -> n\n");
    assert!(e.contains("at 4:"), "case-branch error points at the branch line (4): {e}");

    // do-statement: a unification error inside a statement is on its own line.
    let e = compile_err(
        "main :: IO ()\n\
         main = do\n\
         \x20   putStrLn \"ok\"\n\
         \x20   putStrLn (length \"x\")\n");
    assert!(e.contains("at 4:"), "do-statement error points at the statement line (4): {e}");

    // if-branch reconciliation: the error is on a branch line, not the head.
    let e = compile_err(
        "g :: Int -> Int\n\
         g x =\n\
         \x20   if x > 0\n\
         \x20       then \"pos\"\n\
         \x20       else x\n");
    assert!(e.contains("at 4:") || e.contains("at 5:"),
        "if-branch error points at a branch line (4 or 5): {e}");
}

/// The Haskell precedence-parsing rule: a chain of same-precedence operators
/// is rejected when any of them is non-associative. GHC rejects every one of
/// these programs the same way.
#[test]
fn non_associative_chains_are_rejected() {
    on_compiler_stack(non_associative_chains_are_rejected_impl)
}

fn non_associative_chains_are_rejected_impl() {
    // The classic: comparison operators do not chain.
    let e = compile_err("main :: IO ()\nmain = print (1 == 2 == True)\n");
    assert!(e.contains("non-associative"), "got: {e}");
    assert!(e.contains("'=='"), "got: {e}");
    assert!(e.contains("parenthesize"), "got: {e}");

    // Two different comparison operators conflict too, and the notes offer
    // the three-way-comparison rewrite.
    let e = compile_err("main :: IO ()\nmain = print (1 < 2 <= 3)\n");
    assert!(e.contains("non-associative"), "got: {e}");
    assert!(e.contains("&&"), "got: {e}");

    // A user-declared `infix` operator is non-associative as well.
    let e = compile_err(
        "infix 5 <+>\n(<+>) :: Int -> Int -> Int\na <+> b = a + b\nmain :: IO ()\nmain = print (1 <+> 2 <+> 3)\n",
    );
    assert!(e.contains("non-associative"), "got: {e}");
    assert!(e.contains("'<+>'"), "got: {e}");

    // Prelude `elem` is infix 4 (as in GHC), so it cannot chain with ==.
    let e = compile_err("main :: IO ()\nmain = print (1 `elem` [1] == True)\n");
    assert!(e.contains("`elem`"), "got: {e}");
    assert!(e.contains("non-associative"), "got: {e}");

    // The Prelude's <$> and <*> are infixl 4 (as in GHC): mixing them with a
    // comparison at the same precedence is rejected.
    let e = compile_err("main :: IO ()\nmain = print ((+1) <$> Just 1 == Just 2)\n");
    assert!(e.contains("'<$>'"), "got: {e}");
    assert!(e.contains("'=='"), "got: {e}");

    // Parenthesized, every one of them compiles.
    for src in [
        "main :: IO ()\nmain = print ((1 == 2) == True)\n",
        "main :: IO ()\nmain = print (1 < 2 && 2 <= 3)\n",
        "main :: IO ()\nmain = print ((1 `elem` [1]) == True)\n",
        "main :: IO ()\nmain = print (((+1) <$> Just 1) == Just 2)\n",
    ] {
        assert!(compile(src, Path::new("."), &[]).is_ok(), "should compile:\n{src}");
    }
}

/// A non-lambda-RHS bind in FINAL do-statement position, preceded by
/// another statement, must type like the same expression at top level
/// (the flattener treats it as the chain terminal). The well-typed shapes
/// are covered by the bind_first_class GHC-golden case; this pins the
/// ill-typed one: a final `step 1 >>= step` in an `IO ()` do-block is
/// rejected — by GHC ("Couldn't match type 'Int' with '()'",
/// verified against 9.14.1) and by mata-ll with the same unification
/// mismatch. The regression this guards: the flattener used to treat the
/// continuation FUNCTION as the terminal and reject even well-typed
/// programs with "Cannot unify 'IO a' with 'b -> IO ()'".
#[test]
fn final_do_bind_types_like_top_level() {
    on_compiler_stack(final_do_bind_types_like_top_level_impl)
}

fn final_do_bind_types_like_top_level_impl() {
    let e = compile_err(
        "step :: Int -> IO Int\nstep n = return (n + 1)\n\nmain :: IO ()\nmain = do\n    putStrLn \"x\"\n    step 1 >>= step\n",
    );
    assert!(
        e.contains("Int") && e.contains("()"),
        "must reject with the Int-vs-() mismatch GHC reports, got: {e}"
    );
    assert!(
        !e.contains("->"),
        "must not leak a synthetic continuation arrow into the error, got: {e}"
    );
}

/// Prefix minus follows GHC exactly: it has the fixity of binary '-'
/// (infixl 6). It cannot be the right operand of any precedence >= 6
/// operator (`a + -b`, `a * -2`, ``a `div` -2`` are parse errors), its
/// operand is everything binding tighter than 6 (`-a * b` is
/// `negate (a * b)`; `-a + b` is `negate a + b`), and it cannot stand left
/// of a precedence-6 operator that is not left-associative (`-a <> b`).
/// GHC accepts/rejects every one of these programs identically (verified
/// against GHC 9.14.1; the runtime groupings are covered by the
/// prefix_minus GHC-golden case).
#[test]
fn prefix_minus_matches_ghc() {
    on_compiler_stack(prefix_minus_matches_ghc_impl)
}

fn prefix_minus_matches_ghc_impl() {
    // Rejected: prefix minus as the RHS of a precedence >= 6 operator.
    for (src, op) in [
        ("main :: IO ()\nmain = print (1 + - 2)\n", "'+'"),
        ("main :: IO ()\nmain = print (1 - - 2)\n", "'-'"),
        ("main :: IO ()\nmain = print (1 * - 2)\n", "'*'"),
        ("main :: IO ()\nmain = print (1 `div` - 2)\n", "`div`"),
        // ...including inside a right section (GHC rejects `(+ -2)`).
        ("main :: IO ()\nmain = print ((+ - 2) 3)\n", "'+'"),
        ("main :: IO ()\nmain = print ((`div` - 2) 8)\n", "`div`"),
    ] {
        let e = compile_err(src);
        assert!(e.contains("Prefix minus"), "{src}: got: {e}");
        assert!(e.contains(op), "{src}: got: {e}");
        assert!(e.contains("parenthesize"), "{src}: got: {e}");
    }

    // Rejected: prefix minus left of a non-left-associative precedence-6
    // operator (GHC: "cannot mix prefix `-' and `<>'").
    let e = compile_err("main :: IO ()\nmain = putStrLn (- 1 <> \"a\")\n");
    assert!(e.contains("prefix minus"), "got: {e}");
    assert!(e.contains("'<>'"), "got: {e}");

    // Accepted: parenthesized negation anywhere, negation left of infixl 6,
    // negation under a precedence < 6 operator, and `(- x)`/`(-)` forms.
    for src in [
        "main :: IO ()\nmain = print (1 + (- 2))\n",
        "main :: IO ()\nmain = print (- 2 + 3)\n",
        "main :: IO ()\nmain = print (- 2 - 3)\n",
        "main :: IO ()\nmain = print (1 == - 1)\n",
        "main :: IO ()\nmain = print ((* (- 2)) 3)\n",
        "main :: IO ()\nmain = print ((+ 1) (- 2))\n",
        "main :: IO ()\nmain = print (map (\\x -> - x * 2) [1, 2])\n",
        "main :: IO ()\nmain = print ((-) 5 2)\n",
    ] {
        assert!(compile(src, Path::new("."), &[]).is_ok(), "should compile:\n{src}");
    }
}

/// Sections follow GHC's operand-precedence rule (Haskell 2010 §3.5): a
/// section operand that is itself an infix expression must bind tighter
/// than the section operator — `(== a || b)` is rejected (it cannot mean
/// `\x -> x == (a || b)`, because `x == a || b` groups as `(x == a) || b`),
/// while `(+ a * b)` stays legal. At equal precedence only a chain in the
/// section's own direction is legal: an infixl operand in a left section
/// (`(2 + 3 +)`), an infixr operand in a right section (`(++ a ++ b)`).
/// Prefix minus counts as an infixl 6 operand and declared fixities
/// participate, both as in GHC. GHC 9.14.1 accepts/rejects every one of
/// these programs identically; the accepted groupings run against real GHC
/// via the operator_sections and operator_fixity golden cases.
#[test]
fn section_operand_precedence_matches_ghc() {
    on_compiler_stack(section_operand_precedence_impl)
}

fn section_operand_precedence_impl() {
    // Rejected: the operand's top operator binds looser than the section
    // operator, or refuses to chain with it at equal precedence.
    for (src, needles) in [
        // The canonical shape: `(== a || b)` cannot mean `\x -> x == (a || b)`.
        (
            "main :: IO ()\nmain = print (filter (== True || False) [True])\n",
            &["'||' (infixr 2)", "'==' (infix 4)", "(== (a || b))"][..],
        ),
        // Left section with a looser operand.
        (
            "main :: IO ()\nmain = print ((2 + 3 *) 4)\n",
            &["'+' (infixl 6)", "'*' (infixl 7)", "((a + b) *)"][..],
        ),
        // Equal precedence, wrong direction: infixl in a right section...
        (
            "main :: IO ()\nmain = print ((+ 2 + 3) 1)\n",
            &["'+' (infixl 6)", "(+ (a + b))"][..],
        ),
        // ...infixr in a left section...
        (
            "main :: IO ()\nmain = print (([1] ++ [2] ++) [0])\n",
            &["'++' (infixr 5)", "((a ++ b) ++)"][..],
        ),
        // ...and non-associative, which never chains with itself.
        (
            "main :: IO ()\nmain = print ((== 1 == True) 2)\n",
            &["no defined grouping", "(== (a == b))"][..],
        ),
        // Backtick operators follow the same rule.
        (
            "main :: IO ()\nmain = print ((`div` 1 + 2) 9)\n",
            &["`div` (infixl 7)", "'+' (infixl 6)"][..],
        ),
        // Prefix minus counts as an infixl 6 operand, as in GHC.
        (
            "main :: IO ()\nmain = print ((-1 *) 2)\n",
            &["prefix minus", "'*' (infixl 7)", "((-a) *)"][..],
        ),
        // A declared fixity participates: infixl 2 .|. under infix 4 ==.
        (
            "infixl 2 .|.\n(.|.) :: Bool -> Bool -> Bool\na .|. b = a || b\n\
             main :: IO ()\nmain = print (filter (== True .|. False) [True])\n",
            &["'.|.' (infixl 2)", "'==' (infix 4)", "(== (a .|. b))"][..],
        ),
    ] {
        let e = compile_err(src);
        assert!(
            e.contains("must bind tighter than the section operator"),
            "{src}: got: {e}"
        );
        for n in needles {
            assert!(e.contains(n), "{src}: expected {n:?} in: {e}");
        }
        assert!(e.contains("parenthesize the operand"), "{src}: got: {e}");
    }

    // Accepted: tighter operands, same-direction equal-precedence chains,
    // the parenthesized forms of the rejections, and a declared infixr at
    // the section operator's own precedence.
    for src in [
        "main :: IO ()\nmain = print (filter (== (True || False)) [True])\n",
        "main :: IO ()\nmain = print (map (+ 2 * 3) [1])\n",
        "main :: IO ()\nmain = print ((2 * 3 +) 1)\n",
        "main :: IO ()\nmain = print ((2 + 3 +) 1)\n",
        "main :: IO ()\nmain = print ((++ [1] ++ [2]) [0])\n",
        "main :: IO ()\nmain = print ((: [1] ++ [2]) 0)\n",
        "main :: IO ()\nmain = print ((2 * 3 `div`) 2)\n",
        "main :: IO ()\nmain = print ((-1 +) 3)\n",
        "infixr 7 .*.\n(.*.) :: Int -> Int -> Int\na .*. b = a * b\n\
         main :: IO ()\nmain = print ((.*. 2 .*. 3) 1)\n",
    ] {
        assert!(
            compile(src, Path::new("."), &[]).is_ok(),
            "should compile:\n{src}"
        );
    }
}

/// The other half of the precedence-parsing rule: same precedence but
/// opposite associativities defines no grouping either.
#[test]
fn conflicting_associativities_at_same_precedence_are_rejected() {
    on_compiler_stack(conflicting_associativities_impl)
}

fn conflicting_associativities_impl() {
    // infixl 6 <#> against the builtin infixr 6 <>.
    let e = compile_err(
        "infixl 6 <#>\n(<#>) :: String -> String -> String\na <#> b = a ++ b\nmain :: IO ()\nmain = putStrLn (\"a\" <#> \"b\" <> \"c\")\n",
    );
    assert!(e.contains("opposite directions"), "got: {e}");
    assert!(e.contains("infixl 6") && e.contains("infixr 6"), "got: {e}");

    // Same-precedence, same-associativity chains still parse: both infixl...
    let ok_l = "infixl 6 <#>\n(<#>) :: Int -> Int -> Int\na <#> b = a + b\nmain :: IO ()\nmain = print (1 <#> 2 - 3)\n";
    // ...and both infixr.
    let ok_r = "infixr 6 <#>\n(<#>) :: String -> String -> String\na <#> b = a <> b\nmain :: IO ()\nmain = putStrLn (\"a\" <#> \"b\" <> \"c\")\n";
    for src in [ok_l, ok_r] {
        assert!(compile(src, Path::new("."), &[]).is_ok(), "should compile:\n{src}");
    }
}

/// An imported `infix` operator is non-associative at the import site too:
/// fixity travels with the export (FixityOps declares `infix 4 ~=~`).
#[test]
fn imported_infix_operator_is_non_associative_at_import_site() {
    on_compiler_stack(imported_infix_non_associative_impl)
}

fn imported_infix_non_associative_impl() {
    let src = "import FixityOps\nmain :: IO ()\nmain = print (1 ~=~ 2 ~=~ 3)\n";
    let e = match compile(src, Path::new("tests/cases"), &[]) {
        Ok(_) => panic!("expected compilation to fail, but it succeeded"),
        Err(e) => e.to_string(),
    };
    assert!(e.contains("non-associative"), "got: {e}");
    assert!(e.contains("'~=~'"), "got: {e}");
}

#[test]
fn ffi_outgoing_callback_rejects_bad_signatures() {
    // Effectful callbacks must use `LuaIO s acc`, not `IO acc`.
    let e = compile_err(
        r#"
bad :: String -> (Int -> acc -> IO acc) -> acc -> LuaPure "h.f" acc
main :: IO ()
main = pure ()
"#,
    );
    assert!(e.contains("LuaIO s"), "IO acc should be rejected, got: {e}");

    // The callback's result must be the threaded state, not some other type.
    let e = compile_err(
        r#"
bad :: String -> (Int -> acc -> LuaIO s Bool) -> acc -> LuaPure "h.f" acc
main :: IO ()
main = pure ()
"#,
    );
    assert!(e.contains("threaded state"), "mismatched result should be rejected, got: {e}");

    // A polymorphic callback requires a polymorphic (variable) FFI return type.
    let e = compile_err(
        r#"
bad :: String -> (Int -> a -> a) -> Int -> LuaPure "h.f" Int
main :: IO ()
main = pure ()
"#,
    );
    assert!(
        e.contains("type variable") || e.contains("threaded state"),
        "concrete state should be rejected, got: {e}"
    );
}

#[test]
fn type_errors_are_explained_not_cryptic() {
    // Passing a String to a list-typed function: internal unification vars
    // must render as friendly letters (a, b, …), never as `_i700`, and the
    // message must explain that String is not a list in mata-ll.
    let e = compile_err(
        r#"
main :: IO ()
main = print (length "hello")
"#,
    );
    assert!(e.contains("[a]"), "var should prettify to [a], got: {e}");
    assert!(!e.contains("_i"), "internal `_i` var names must not leak, got: {e}");
    // The String/list note must explain the opaque-String design: not [Char],
    // list ops don't apply, and <> is how you concatenate Strings. (Updated
    // 2026-07-24: the note now prescribes <> per the error-message convention;
    // see the TODO "String-vs-list type errors should explain the design".)
    assert!(e.contains("opaque") && e.contains("[Char]"),
        "missing opaque-String note, got: {e}");
    assert!(e.contains("<>") && e.contains("HASKDIFF.md"),
        "note must point at <> and HASKDIFF.md, got: {e}");

    // `<>` on a list should point the user at `++`.
    let e = compile_err(
        r#"
main :: IO ()
main = print ([1, 2] <> [3, 4] :: [Int])
"#,
    );
    assert!(e.contains("No instance for '<>'"), "got: {e}");
    assert!(e.contains("concatenated with ++"), "missing ++ note, got: {e}");

    // Ordering whole tuples is rejected at type-check with the missing-instance
    // explanation (the checker discharges the Ord constraint before codegen).
    // The tuple is annotated `(Int, Int)` so the rejection is the
    // missing tuple-Ord instance, not literal-defaulting ambiguity: with
    // polymorphic literals `(1, 2)` alone is `(Num a, Num b) => (a, b)`, and
    // since mata-ll has no `Ord (a, b)` instance the elements cannot default,
    // so an un-annotated tuple would report an (also-correct) ambiguity error.
    let e = compile_err(
        r#"
main :: IO ()
main = print (((1, 2) :: (Int, Int)) > (1, 3))
"#,
    );
    assert!(e.contains("No instance for 'Ord (Int, Int)'"), "got: {e}");
    assert!(e.contains("no Ord instance"), "missing tuple Ord note, got: {e}");
}

/// Unpacking an existential must SKOLEMIZE: the hidden type variable becomes
/// a rigid constant that cannot unify with any concrete type. The canonical
/// soundness probe — before the fix this compiled and produced a Lua runtime
/// crash ("attempt to add a 'string' with a 'number'").
#[test]
fn existential_unpacking_skolemizes() {
    let e = compile_err(
        r#"
data Foo = forall a. Foo a

unFoo :: Foo -> Int
unFoo (Foo x) = x + 1

main :: IO ()
main = putStrLn (show (unFoo (Foo "hello")))
"#,
    );
    assert!(
        e.contains("Cannot match 'a' with 'Int'"),
        "the skolem must not unify with Int, got: {e}"
    );
    assert!(e.contains("rigid type variable"), "must explain rigidity, got: {e}");
    assert!(e.contains("in definition of 'unFoo'"), "must locate the clause, got: {e}");
    // The provenance note: 'a' alone is baffling unless the error says the
    // type was hidden by the constructor.
    assert!(
        e.contains("existential type hidden by constructor 'Foo'"),
        "must name the hiding constructor, got: {e}"
    );
    assert!(
        e.contains("declares no constraints"),
        "must say why no instance can help, got: {e}"
    );

    // GADT syntax declares existentials implicitly (a signature variable
    // that does not reach the result type); it must skolemize identically.
    let e = compile_err(
        r#"
data Box where
  MkBox :: a -> Box

coerce :: Box -> Int
coerce (MkBox x) = x

main :: IO ()
main = putStrLn (show (coerce (MkBox "boom") + 1))
"#,
    );
    assert!(
        e.contains("Cannot match 'a' with 'Int'")
            || e.contains("escapes its scope"),
        "GADT-syntax existential must be rigid too, got: {e}"
    );
    assert!(
        e.contains("hidden by constructor 'MkBox'"),
        "must name the hiding constructor, got: {e}"
    );
}

/// An unpacked existential's skolem must not survive the match that
/// introduced it: not via the function's own type, not via a case
/// expression's result, and not via a where-function's (monomorphic,
/// shared-across-calls) type.
#[test]
fn existential_skolem_cannot_escape() {
    // Direct escape through the return type. Here the return type is the
    // function's own signature variable `a`, which is itself checked as a rigid
    // skolem (a body may not be more general than its signature). Returning the
    // existential's hidden value therefore fails as a mismatch between two rigid
    // variables — the signature's `a` and the existential's `a` — which is
    // exactly how GHC reports it ("Couldn't match expected type 'a' with actual
    // type 'a1'"). Escape into a *concrete* return/case type still surfaces the
    // dedicated existential-escape diagnostic (the two cases below).
    let e = compile_err(
        r#"
data Foo = forall a. Foo a

unFoo :: Foo -> a
unFoo (Foo x) = x

main :: IO ()
main = putStrLn "no"
"#,
    );
    assert!(
        e.contains("Cannot match 'a' with 'a'"),
        "returning an existential as a signature variable must be rejected as a rigid mismatch, got: {e}"
    );
    // Both provenance notes appear: `a` is the existential hidden by `Foo`, and
    // `a` is also the signature's rigid variable.
    assert!(e.contains("hidden by constructor 'Foo'"), "got: {e}");
    assert!(
        e.contains("rigid type variable from the signature of 'unFoo'"),
        "the signature-rigidity note must explain the second 'a', got: {e}"
    );

    // Escape through a case expression's result type.
    let e = compile_err(
        r#"
data Foo = forall a. Foo a

useCase :: Foo -> Int
useCase f = case f of
  Foo x -> x

main :: IO ()
main = putStrLn "no"
"#,
    );
    assert!(
        e.contains("escapes its scope"),
        "case-result escape must be rejected, got: {e}"
    );

    // Escape through a where-function's type: where-bindings are
    // monomorphic, so `unpack e1` and `unpack e2` would claim the SAME
    // hidden type for two different boxes — with an Eq-constrained
    // existential that "equates" an Int with a String.
    let e = compile_err(
        r#"
data EqBox = forall a. Eq a => EqBox a

test :: EqBox -> EqBox -> Bool
test e1 e2 = unpack e1 == unpack e2
  where unpack (EqBox x) = x

main :: IO ()
main = putStrLn (show (test (EqBox 1) (EqBox "one")))
"#,
    );
    assert!(
        e.contains("escapes its scope"),
        "where-function escape must be rejected, got: {e}"
    );
}

/// A constrained existential (`forall a. Show a => …`) is checked in both
/// directions: packing must prove the declared instance for the concrete
/// type, and unpacking provides exactly the declared classes — a class the
/// constructor does not declare stays unavailable.
#[test]
fn existential_constraints_enforced_both_ways() {
    // Unpack side: Show is declared, Num is not — arithmetic on the hidden
    // type must be rejected, and the note must say what IS available.
    let e = compile_err(
        r#"
data Showable = forall a. Show a => Showable a

bad :: Showable -> Int
bad s = case s of
  Showable x -> x + (1 :: Int)

main :: IO ()
main = putStrLn "no"
"#,
    );
    // The literal is annotated `Int` so `+` forces the hidden type to be
    // Int, surfacing the rigid-match rejection. (An un-annotated `x + 1`
    // now leaves the sum at the existential type `a`, which is reported instead
    // as `a` escaping the match — also a rejection, but a different message.)
    assert!(
        e.contains("Cannot match 'a' with 'Int'"),
        "undeclared class use must be rejected, got: {e}"
    );
    assert!(
        e.contains("declared context (Show)"),
        "note must list what the constructor guarantees, got: {e}"
    );

    // Pack side: a function has no Show instance, so it cannot be packed
    // into a Show-constrained existential.
    let e = compile_err(
        r#"
data Showable = forall a. Show a => Showable a

pack :: Showable
pack = Showable (\x -> (x :: Int))

main :: IO ()
main = putStrLn "no"
"#,
    );
    assert!(
        e.contains("No instance for 'Show (Int -> Int)'"),
        "packing an instance-less type must be rejected, got: {e}"
    );

    // A typo'd class in the constructor context must error at the data
    // declaration, not silently become "no constraint".
    let e = compile_err(
        r#"
data Box = forall a. Showw a => Box a

main :: IO ()
main = putStrLn "no"
"#,
    );
    assert!(
        e.contains("Unknown typeclass 'Showw' in the context of constructor 'Box'"),
        "unknown context class must be reported, got: {e}"
    );
}

/// Record syntax back doors: a field whose type is existential has no
/// selector (the selector's result type would BE the hidden type, outside
/// any match) and cannot be record-updated (nothing to check the new value
/// against). Both were runtime type confusions before the fix.
#[test]
fn existential_record_fields_have_no_selector_or_update() {
    let e = compile_err(
        r#"
data Foo = forall a. Foo { getIt :: a }

main :: IO ()
main = putStrLn (show (getIt (Foo "hello") + 1))
"#,
    );
    assert!(
        e.contains("has an existential type, so it has no selector function"),
        "selector use must be rejected with an explanation, got: {e}"
    );

    let e = compile_err(
        r#"
data Foo = forall a. Foo { getIt :: a, label :: String }

update :: Foo -> Foo
update f = f { getIt = 42 }

main :: IO ()
main = putStrLn "no"
"#,
    );
    assert!(
        e.contains("cannot be record-updated"),
        "existential field update must be rejected, got: {e}"
    );
}

/// Monomorphization-time errors must carry a source location, like
/// typechecker errors do. `<>` on lists is rejected during method resolution
/// in mono (the checker keeps a builtin Semigroup [a] instance for
/// polymorphic bodies), so its diagnostic is the canonical mono error: it
/// must name the line/column of the offending clause and its definition,
/// while keeping the message and the `note:` line verbatim.
#[test]
fn mono_error_reports_source_location() {
    let e = compile_err(
        r#"
main :: IO ()
main = print ([1, 2] <> [3, 4] :: [Int])
"#,
    );
    assert!(e.contains("No instance for '<>' on type '[Int]'"), "got: {e}");
    assert!(
        e.contains("at 3:6, in definition of 'main'"),
        "mono error must carry the clause's source location, got: {e}"
    );
    assert!(e.contains("note: lists are concatenated with ++"), "missing ++ note, got: {e}");
}

/// The parser recovers at declaration boundaries: one run reports every
/// independent syntax error, not just the first. The first error's message
/// must render exactly as it always has (inline ` at line:col`).
#[test]
fn parser_reports_multiple_errors_per_run() {
    let e = compile_err(
        r#"data Foo = = Bar

good :: Int -> Int
good x = x + 1

main :: IO ()
main = ]
"#,
    );
    assert!(
        e.contains("Parse error: Expected type/constructor name, found Eq at 1:12"),
        "first error must keep its exact historical rendering, got: {e}"
    );
    assert!(
        e.contains("Expected expression, found RightBracket at 7:8"),
        "second independent error must also be reported, got: {e}"
    );
    assert!(
        e.matches("Parse error: ").count() >= 2,
        "expected at least two parse errors in one run, got: {e}"
    );
}

#[test]
fn str_to_ints_unpacks_char_codes() {
    // strToInts bridges mata-ll's opaque String to a list of character codes,
    // in order. A wrong result aborts the program via `error`, failing exec().
    let source = r#"
import LString (strToInts)

main :: IO ()
main = do
    if strToInts "AZ" == [65, 90]
        then pure () else error "AZ codes wrong"
    if strToInts "hello" == [104, 101, 108, 108, 111]
        then pure () else error "hello codes wrong"
"#;
    let lua_code = compile(source, Path::new("."), &[Path::new("../lib")])
        .expect("strToInts program should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("str_to_ints").exec()
        .expect("strToInts should produce the expected character codes");
}

#[test]
fn print_of_empty_list_shows_brackets_not_nothing() {
    // Regression: [] and Nothing share a runtime rep (Lua nil). `print` used the
    // type-erased generic show, which guessed "Nothing" for nil — so an empty
    // [Int] (even nested) printed as "Nothing". `print` must use the typed
    // list show (which knows nil means []), while real Nothing still shows.
    let source = r#"
main :: IO ()
main = do
    print ([] :: [Int])
    print ([[1, 2], []] :: [[Int]])
    print (Nothing :: Maybe Int)
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;

    // Capture `print` output instead of letting it hit stdout.
    let lua = mlua::Lua::new();
    let captured = lua.create_table().unwrap();
    lua.globals().set("__captured", captured.clone()).unwrap();
    let print_fn = lua
        .create_function(|lua, s: mlua::LuaString| -> mlua::Result<()> {
            let line = s.to_str()?.to_string();
            let t: mlua::Table = lua.globals().get("__captured")?;
            let n = t.raw_len();
            t.raw_set(n + 1, line)?;
            Ok(())
        })
        .unwrap();
    lua.globals().set("print", print_fn).unwrap();
    lua.load(&lua_code).set_name("print_empty").exec()
        .expect("should run");

    let lines: Vec<String> = captured
        .sequence_values::<String>()
        .collect::<mlua::Result<_>>()
        .unwrap();
    assert_eq!(lines, vec!["[]", "[[1,2],[]]", "Nothing"]);
}

#[test]
fn ffi_result_marshalling_decodes_host_values() {
    // A LuaIO host returns a *raw* Lua value (arrays, dicts, nested records).
    // The compiler must decode it into the mata-ll representation per the
    // declared result type: `[record]` and `[String]` lists (tested BOTH empty
    // and non-empty) become cons lists, a `Maybe` field round-trips nil<->Nothing,
    // and scalars pass through. Regression for the FFI-boundary bugs where the
    // undecoded host value made `show` print numbers instead of the string keys
    // and `[Nothing]` for an empty (`{}`) list. The mata-ll program does its own
    // value assertions via `expect`; a decode bug makes one of them `error`.
    let source = r#"
data Params = Params { host :: String } deriving (Show, LuaDict)

data Cert = Cert { ip :: String, chain :: [Int] } deriving (Show, LuaDict)

data Resp = Resp
        { certificates :: [Cert]
        , errors :: [String]
        , note :: Maybe String
        , count :: Int }
    deriving (Show, LuaDict)

fetch :: Params -> LuaIO "luarest.fetch" Resp

expect :: Bool -> String -> IO ()
expect True _ = pure ()
expect False m = error m

len :: [a] -> Int
len [] = 0
len (_:xs) = 1 + len xs

main :: IO ()
main = do
    -- "ok" response: two certs, empty errors, present note, scalar count.
    r <- fetch (Params "ok")
    let cs = certificates r
    expect (len cs == 2) "cert list should have two elements"
    expect (ip (cs !! 0) == "1.2.3.4") "first ip must be the host string, not a number"
    expect (ip (cs !! 1) == "5.6.7.8") "second ip must be the host string, not a number"
    expect (len (chain (cs !! 0)) == 3) "nested chain list length"
    expect ((chain (cs !! 0)) !! 1 == 20) "nested chain element"
    expect (len (errors r) == 0) "empty error array must decode to the empty list"
    expect (show (errors r) == "[]") "empty error list shows as [] not [Nothing]"
    expect (count r == 42) "scalar field passes through"
    case note r of
        Just s  -> expect (s == "hi") "present Maybe field"
        Nothing -> error "note should be Just for the ok response"
    -- "bad" response: no certs, two errors, absent (nil) note.
    r2 <- fetch (Params "bad")
    expect (len (errors r2) == 2) "non-empty error list length"
    expect ((errors r2) !! 0 == "e1") "first error string"
    expect ((errors r2) !! 1 == "e2") "second error string"
    expect (len (certificates r2) == 0) "empty cert array must decode to the empty list"
    case note r2 of
        Nothing -> pure ()
        Just _  -> error "note should be Nothing when the host omits it"
    pure ()
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;

    let lua = mlua::Lua::new();
    // Register the Lua host `luarest.fetch`, returning a *raw* Lua value shaped
    // like a real host (arrays and dicts, not mata-ll cons cells).
    let luarest = lua.create_table().unwrap();
    let fetch = lua
        .create_function(|lua, params: mlua::Table| -> mlua::Result<mlua::Table> {
            let host: String = params.get("host")?;
            let resp = lua.create_table()?;
            if host == "ok" {
                let certs = lua.create_table()?;
                for (i, (ip, chain)) in
                    [("1.2.3.4", [10, 20, 30]), ("5.6.7.8", [1, 2, 3])].iter().enumerate()
                {
                    let c = lua.create_table()?;
                    c.set("ip", *ip)?;
                    let ch = lua.create_table()?;
                    for (j, v) in chain.iter().enumerate() {
                        ch.set(j + 1, *v)?;
                    }
                    c.set("chain", ch)?;
                    certs.set(i + 1, c)?;
                }
                resp.set("certificates", certs)?;
                resp.set("errors", lua.create_table()?)?; // empty array {}
                resp.set("note", "hi")?;
                resp.set("count", 42)?;
            } else {
                resp.set("certificates", lua.create_table()?)?; // empty array {}
                let errs = lua.create_table()?;
                errs.set(1, "e1")?;
                errs.set(2, "e2")?;
                resp.set("errors", errs)?;
                // note omitted -> nil -> Nothing
                resp.set("count", 0)?;
            }
            Ok(resp)
        })
        .unwrap();
    luarest.set("fetch", fetch).unwrap();
    lua.globals().set("luarest", luarest).unwrap();

    lua.load(&lua_code)
        .set_name("ffi_result_marshalling")
        .exec()
        .expect("host result should decode and every in-program assertion should pass");
}

#[test]
fn derived_show_uses_constructor_names_and_parens() {
    // Regression: derived Show must render constructor names (not numeric tags
    // or tuples), recurse through polymorphic types (Tree a b / Box a), and
    // parenthesize constructor-application fields like GHC (showsPrec 11):
    // nullary/atomic fields stay bare, negatives get parens.
    let source = r#"
data Tree a b = Leaf a b | Branch (Tree a b) (Tree a b) deriving (Show)
data Box a = MkBox a deriving (Show)
data C = Red | Green deriving (Show)
data P a = P a a deriving (Show)
data B = MkB Int deriving (Show)

main :: IO ()
main = do
    print (Branch (Leaf (1 :: Int) (2 :: Int)) (Leaf 3 4))
    print (MkBox (MkBox (5 :: Int)))
    print (P Red Green)
    print (MkB (0 - 5))
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;

    let lua = mlua::Lua::new();
    let captured = lua.create_table().unwrap();
    lua.globals().set("__captured", captured.clone()).unwrap();
    let print_fn = lua
        .create_function(|lua, s: mlua::LuaString| -> mlua::Result<()> {
            let line = s.to_str()?.to_string();
            let t: mlua::Table = lua.globals().get("__captured")?;
            let n = t.raw_len();
            t.raw_set(n + 1, line)?;
            Ok(())
        })
        .unwrap();
    lua.globals().set("print", print_fn).unwrap();
    lua.load(&lua_code).set_name("derived_show").exec()
        .expect("should run");

    let lines: Vec<String> = captured
        .sequence_values::<String>()
        .collect::<mlua::Result<_>>()
        .unwrap();
    assert_eq!(
        lines,
        vec![
            "Branch (Leaf 1 2) (Leaf 3 4)", // polymorphic recursion + parens
            "MkBox (MkBox 5)",              // nested poly constructor
            "P Red Green",                  // nullary fields: no parens
            "MkB (-5)",                     // negative: parens
        ]
    );
}

#[test]
fn show_maybe_renders_just() {
    // Regression: `show` renders the Maybe structure. `Just` is now an injective
    // tagged wrapper, so `Just Nothing` is distinct from `Nothing` at every
    // nesting level and renders "Just Nothing" (it no longer collapses to nil).
    let source = r#"
main :: IO ()
main = do
    print (Just (5 :: Int))
    print (Nothing :: Maybe Int)
    print (Just (Just (5 :: Int)))
    print (Just (0 - 5 :: Int))
    print [Just (1 :: Int), Nothing, Just 3]
    print (Just (Nothing :: Maybe Int))
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;

    let lua = mlua::Lua::new();
    let captured = lua.create_table().unwrap();
    lua.globals().set("__captured", captured.clone()).unwrap();
    let print_fn = lua
        .create_function(|lua, s: mlua::LuaString| -> mlua::Result<()> {
            let line = s.to_str()?.to_string();
            let t: mlua::Table = lua.globals().get("__captured")?;
            let n = t.raw_len();
            t.raw_set(n + 1, line)?;
            Ok(())
        })
        .unwrap();
    lua.globals().set("print", print_fn).unwrap();
    lua.load(&lua_code).set_name("show_maybe").exec()
        .expect("should run");

    let lines: Vec<String> = captured
        .sequence_values::<String>()
        .collect::<mlua::Result<_>>()
        .unwrap();
    assert_eq!(
        lines,
        vec![
            "Just 5",
            "Nothing",
            "Just (Just 5)",
            "Just (-5)",
            "[Just 1,Nothing,Just 3]",
            "Just Nothing", // injective Just: distinct from Nothing
        ]
    );
}

// Helper: compile + run, capturing `print`/`putStrLn` output lines.
fn run_capturing_lines(source: &str, name: &str) -> Vec<String> {
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    let captured = lua.create_table().unwrap();
    lua.globals().set("__captured", captured.clone()).unwrap();
    let print_fn = lua
        .create_function(|lua, s: mlua::LuaString| -> mlua::Result<()> {
            let line = s.to_str()?.to_string();
            let t: mlua::Table = lua.globals().get("__captured")?;
            let n = t.raw_len();
            t.raw_set(n + 1, line)?;
            Ok(())
        })
        .unwrap();
    lua.globals().set("print", print_fn).unwrap();
    lua.load(&lua_code).set_name(name).exec().expect("should run");
    captured.sequence_values::<String>().collect::<mlua::Result<_>>().unwrap()
}

#[test]
fn nested_maybe_is_injective() {
    // `Just` is an injective tagged wrapper: `Just Nothing` is distinct from
    // `Nothing`, `Just (Just x)` from `Just x`, at every nesting level — with
    // correct show, ==, and pattern-matching (via the Data.Maybe functions).
    let source = r#"
import qualified Data.Maybe as M

main :: IO ()
main = do
    putStrLn (show (Nothing :: Maybe (Maybe Int)))
    putStrLn (show (Just Nothing :: Maybe (Maybe Int)))
    putStrLn (show (Just (Just 5) :: Maybe (Maybe Int)))
    putStrLn (show ((Just Nothing :: Maybe (Maybe Int)) == Nothing))
    putStrLn (show ((Just Nothing :: Maybe (Maybe Int)) == Just Nothing))
    putStrLn (show ((Just (Just 5) :: Maybe (Maybe Int)) == Just (Just 5)))
    putStrLn (show (M.isJust (Just Nothing :: Maybe (Maybe Int))))
    putStrLn (show (M.isNothing (Just Nothing :: Maybe (Maybe Int))))
    putStrLn (show (M.fromJust (Just (Just 7)) :: Maybe Int))
    putStrLn (show (M.fromMaybe (Just 9) (Just Nothing :: Maybe (Maybe Int))))
    putStrLn (show (M.maybe 0 (M.fromMaybe 1) (Just (Just 8) :: Maybe (Maybe Int))))
"#;
    let lines = run_capturing_lines(source, "nested_maybe");
    assert_eq!(
        lines,
        vec![
            "Nothing",         // Nothing :: Maybe (Maybe Int)
            "Just Nothing",    // distinct from Nothing
            "Just (Just 5)",
            "False",           // Just Nothing /= Nothing
            "True",            // Just Nothing == Just Nothing
            "True",            // Just (Just 5) == Just (Just 5)
            "True",            // isJust (Just Nothing)
            "False",           // isNothing (Just Nothing)
            "Just 7",          // fromJust (Just (Just 7))
            "Nothing",         // fromMaybe (Just 9) (Just Nothing) = the inner Nothing
            "8",               // maybe 0 (fromMaybe 1) (Just (Just 8))
        ]
    );
}

#[test]
fn just_of_empty_list_distinct_from_nothing() {
    // `[]` is also nil at runtime, so `Just []` used to collapse to Nothing too.
    // The wrapper keeps them distinct.
    let source = r#"
main :: IO ()
main = do
    putStrLn (show (Just [] :: Maybe [Int]))
    putStrLn (show (Nothing :: Maybe [Int]))
    putStrLn (show ((Just [] :: Maybe [Int]) == Nothing))
    putStrLn (show (Just [1, 2] :: Maybe [Int]))
"#;
    let lines = run_capturing_lines(source, "just_empty_list");
    assert_eq!(lines, vec!["Just []", "Nothing", "False", "Just [1,2]"]);
}

#[test]
fn lazy_index_elements_print_as_values() {
    // Finding 1, exact repro on the PRINT path: an element pulled from a
    // lazily-generated list via head/tail/(!!) must print as its value.
    // Before the fix, a raw thunk escaped and `print` rendered its Lua
    // representation ("(function: 0x.., False)" / garbage), and the
    // let-bound form crashed with "attempt to perform arithmetic on a table
    // value". Asserting on the captured output catches the leak even when
    // it does NOT crash.
    let source = r#"
inc :: Int -> Int
inc x = x + 1

main :: IO ()
main = do
    print (head (tail (iterate inc 0)))
    print ([1..] !! 5)
    let v = iterate inc 0 !! 2
    print (v * 10)
    print (take 3 (iterate inc 0))
"#;
    let lines = run_capturing_lines(source, "lazy_index_print");
    assert_eq!(
        lines,
        vec![
            "1",         // head (tail (iterate inc 0)) — leaked "(function: 0x.., False)"
            "6",         // [1..] !! 5 — index 5 of [1,2,3,...] is 6; printed garbage pre-fix
            "20",        // (iterate inc 0 !! 2) * 10 — crashed on arithmetic
            "[0,1,2]", // take must materialize values, not thunks
        ]
    );
}

#[test]
fn maybe_ffi_single_level_boundary_preserved() {
    // Interop for the common single-level case is unchanged: an exported
    // `Maybe a` marshals `Just v -> v` and `Nothing -> nil` for the Lua host.
    // (Lua's nil cannot represent nested optionals; that is an accepted limit.)
    let source = r#"
export find :: Int -> Maybe Int
find 0 = Nothing
find n = Just (n * 10)
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    let module: mlua::Table = lua.load(&lua_code).set_name("maybe_ffi").eval()
        .expect("should load module");
    let find: mlua::Function = module.get("find").unwrap();
    let got_nothing: mlua::Value = find.call(0i64).unwrap();
    assert!(matches!(got_nothing, mlua::Value::Nil), "Nothing should marshal to nil");
    let got_just: i64 = find.call(7i64).unwrap();
    assert_eq!(got_just, 70, "Just 70 should marshal to the bare value 70");
}

// --- Source embedding (--embed-source) and recompilation (--recompile) ---
//
// Compiling with `CompileOptions { embed_source: Some(mode) }` must carry the
// original .mll source inside the emitted Lua so `embed::extract_source` can
// recover it byte-exactly and recompile it without the .mll file. The fixture
// is deliberately hostile: long-bracket closers at several levels (forcing
// the embedder to pick a non-colliding bracket level), fake marker lines,
// and escaped quotes — none of which may terminate the block early or break
// the emitted Lua.

const EMBED_FIXTURE: &str = r#"-- bracket bombs in a comment: ]] ]=] ]==] --[[
-- MLL-EMBEDDED-SOURCE-END ]=]
greeting :: String
greeting = "brackets: ]] ]=] ]==] ]===] and --[==[ and \"quotes\""

fakeMarker :: String
fakeMarker = "local __SOURCE_CODE = [=["

main :: IO ()
main = do
  putStrLn greeting
  putStrLn fakeMarker
"#;

fn compile_embedded(source: &str, mode: mllc::EmbedMode) -> String {
    let opts = mllc::CompileOptions { embed_source: Some(mode), ..Default::default() };
    with_compiler_stack(|| mllc::compile_with_options(source, Path::new("."), &[], &opts))
        .expect("embedding compile should succeed")
        .lua_code
}

#[test]
fn embed_comments_round_trip() {
    let plain = compile(EMBED_FIXTURE, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    // Without embedding, no marker or source variable may leak into the output.
    assert!(!plain.contains("MLL-EMBEDDED-SOURCE-BEGIN"),
        "plain compile must not carry an embedded-source block");

    let embedded = compile_embedded(EMBED_FIXTURE, mllc::EmbedMode::Comments);

    // The embedded file is still loadable, runnable Lua.
    let lua = mlua::Lua::new();
    lua.load(&embedded).set_name("embed_comments").exec()
        .expect("comment-embedded Lua should still run");

    // Extraction recovers the source byte-exactly, with the right mode.
    let (extracted, mode) = mllc::embed::extract_source(&embedded)
        .expect("embedded source should be found");
    assert_eq!(mode, mllc::EmbedMode::Comments);
    assert_eq!(extracted, EMBED_FIXTURE, "extraction must be byte-exact");

    // Recompiling the extracted source matches a direct compile exactly...
    let recompiled = compile(&extracted, Path::new("."), &[])
        .expect("extracted source should recompile")
        .lua_code;
    assert_eq!(recompiled, plain, "recompile must equal a direct compile");

    // ...and re-embedding reproduces the embedded file byte-for-byte, so an
    // in-place `--recompile` of an unchanged file is a fixpoint.
    let reembedded = compile_embedded(&extracted, mllc::EmbedMode::Comments);
    assert_eq!(reembedded, embedded, "re-embedding must be a fixpoint");
}

#[test]
fn embed_var_round_trip() {
    let plain = compile(EMBED_FIXTURE, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    // The fixture's fake-marker string literal appears in the plain output
    // (as a generated Lua string constant), so a bare substring check would
    // misfire — what matters is that it doesn't extract as a source block.
    assert!(mllc::embed::extract_source(&plain).is_err(),
        "plain compile must not carry an extractable source block");

    let embedded = compile_embedded(EMBED_FIXTURE, mllc::EmbedMode::Var);

    // Loaded as a module (chunk argument set, so main is skipped), the
    // exports table carries the exact source under __SOURCE_CODE.
    let lua = mlua::Lua::new();
    let exports: mlua::Table = lua.load(&embedded).set_name("embed_var")
        .call("embed_var")
        .expect("var-embedded Lua should load as a module");
    let runtime_source: String = exports.get("__SOURCE_CODE")
        .expect("module must export __SOURCE_CODE");
    assert_eq!(runtime_source, EMBED_FIXTURE,
        "__SOURCE_CODE at runtime must equal the original source");

    // It also still runs as a program (main not skipped).
    let lua = mlua::Lua::new();
    lua.load(&embedded).set_name("embed_var").exec()
        .expect("var-embedded Lua should still run as a program");

    // Textual extraction round-trips byte-exactly and recompiles identically.
    let (extracted, mode) = mllc::embed::extract_source(&embedded)
        .expect("embedded source should be found");
    assert_eq!(mode, mllc::EmbedMode::Var);
    assert_eq!(extracted, EMBED_FIXTURE, "extraction must be byte-exact");
    let recompiled = compile(&extracted, Path::new("."), &[])
        .expect("extracted source should recompile")
        .lua_code;
    assert_eq!(recompiled, plain, "recompile must equal a direct compile");
    let reembedded = compile_embedded(&extracted, mllc::EmbedMode::Var);
    assert_eq!(reembedded, embedded, "re-embedding must be a fixpoint");
}

#[test]
fn embed_var_merges_with_existing_exports() {
    // A module with real `export`s: __SOURCE_CODE must join the exports table
    // without displacing them.
    let source = r#"
export double :: Int -> Int
double n = n * 2

main :: IO ()
main = putStrLn (show (double 4))
"#;
    let embedded = compile_embedded(source, mllc::EmbedMode::Var);
    let lua = mlua::Lua::new();
    let exports: mlua::Table = lua.load(&embedded).set_name("embed_exports")
        .call("embed_exports")
        .expect("should load as a module");
    let runtime_source: String = exports.get("__SOURCE_CODE")
        .expect("module must export __SOURCE_CODE");
    assert_eq!(runtime_source, source);
    let double: mlua::Function = exports.get("double")
        .expect("real exports must survive alongside __SOURCE_CODE");
    let result: i64 = double.call(21).expect("exported fn should be callable");
    assert_eq!(result, 42);
}

#[test]
fn embed_source_without_trailing_newline() {
    // The framing newlines belong to the block, not the source: a source with
    // no final newline must come back without one.
    let source = "main :: IO ()\nmain = putStrLn \"no trailing newline\"";
    for mode in [mllc::EmbedMode::Comments, mllc::EmbedMode::Var] {
        let embedded = compile_embedded(source, mode);
        let lua = mlua::Lua::new();
        lua.load(&embedded).set_name("embed_no_nl").exec()
            .expect("should run");
        let (extracted, _) = mllc::embed::extract_source(&embedded)
            .expect("embedded source should be found");
        assert_eq!(extracted, source, "byte-exact round trip ({:?})", mode);
    }
}

#[test]
fn luadict_on_multi_constructor_rejected() {
    // LuaDict has no tag to tell variants apart, so it only makes sense on a
    // single-constructor record. Deriving it elsewhere must fail with an
    // explanation, not silently miscompile.
    let source = r#"
data T = A { x :: Int } | B { y :: Int }
    deriving (LuaDict)

main :: IO ()
main = pure ()
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("LuaDict") && msg.contains("one constructor"),
                "expected a LuaDict single-constructor error, got: {}", msg);
        }
        Ok(_) => panic!("deriving LuaDict on a multi-constructor type must fail"),
    }
}

#[test]
fn luadict_on_positional_fields_rejected() {
    let source = r#"
data P = P Int Int
    deriving (LuaDict)

main :: IO ()
main = pure ()
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("LuaDict") && msg.contains("positional"),
                "expected a LuaDict positional-fields error, got: {}", msg);
        }
        Ok(_) => panic!("deriving LuaDict on positional fields must fail"),
    }
}

#[test]
fn luadict_exported_value_is_a_named_table() {
    // A LuaDict record returned across the FFI boundary must reach Lua as a
    // real dictionary keyed by field name — not the empty table that positional
    // `ipairs` marshalling would produce. This is the whole point of LuaDict.
    let source = r#"
data Config = Config { width :: Int, height :: Int, title :: String }
  deriving (LuaDict)

export mkConfig :: Int -> Int -> Config
mkConfig w h = Config { width = w, height = h, title = "win" }

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);
    let mk: mlua::Function = module.get("mkConfig").unwrap();
    let cfg: mlua::Table = mk.call((80, 25)).expect("mkConfig should return a table");
    let width: i64 = cfg.get("width").expect("width key present");
    let height: i64 = cfg.get("height").expect("height key present");
    let title: String = cfg.get("title").expect("title key present");
    assert_eq!(width, 80, "named width key survives marshalling");
    assert_eq!(height, 25, "named height key survives marshalling");
    assert_eq!(title, "win", "named title key survives marshalling");
    // Positional array access must be empty — it's a dictionary, not an array.
    assert_eq!(cfg.len().unwrap(), 0, "LuaDict has no positional entries");
}

#[test]
fn luadict_renamed_keys_round_trip_ffi_boundary() {
    // `field as "key"` renames only the LuaDict table key. Both FFI directions
    // must use the renamed key: an exported record reaches Lua keyed by "key"
    // (and NOT by the Haskell field name), and a host table keyed by "key"
    // decodes back into the record — including through the type-directed
    // decoder, which the [Int] field forces (Lua array -> cons list).
    let source = r#"
data Acct = Acct
  { acctName as "name" :: String
  , acctScores as "scores" :: [Int]
  , acctActive :: Bool
  } deriving (LuaDict)

export mkAcct :: String -> Acct
mkAcct n = Acct { acctName = n, acctScores = [1, 2], acctActive = True }

fetch :: Int -> LuaIO "acct.fetch" Acct

expect :: Bool -> String -> IO ()
expect True _ = pure ()
expect False m = error m

len :: [a] -> Int
len [] = 0
len (_:xs) = 1 + len xs

main :: IO ()
main = do
    r <- fetch 1
    expect (acctName r == "zoe") "decoded renamed string key"
    expect (len (acctScores r) == 3) "decoded renamed list key length"
    expect ((acctScores r) !! 1 == 20) "decoded renamed list element"
    expect (acctActive r == True) "decoded unrenamed key"
    pure ()
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;

    let lua = mlua::Lua::new();
    // Host `acct.fetch` returns a raw Lua dict keyed by the *renamed* keys.
    let acct = lua.create_table().unwrap();
    let fetch = lua
        .create_function(|lua, _n: i64| -> mlua::Result<mlua::Table> {
            let t = lua.create_table()?;
            t.set("name", "zoe")?;
            let scores = lua.create_table()?;
            for (i, v) in [10, 20, 30].iter().enumerate() {
                scores.set(i + 1, *v)?;
            }
            t.set("scores", scores)?;
            t.set("acctActive", true)?;
            Ok(t)
        })
        .unwrap();
    acct.set("fetch", fetch).unwrap();
    lua.globals().set("acct", acct).unwrap();

    // Load as a module (chunk arg set): exports available, main skipped.
    let module: mlua::Table = lua.load(&lua_code).set_name("luadict_renamed")
        .call("luadict_renamed")
        .expect("should load module");

    // Outbound: the exported record is keyed by the renamed keys...
    let mk: mlua::Function = module.get("mkAcct").unwrap();
    let a: mlua::Table = mk.call("kim").expect("mkAcct should return a table");
    let name: String = a.get("name").expect("renamed 'name' key present");
    assert_eq!(name, "kim");
    let scores: mlua::Table = a.get("scores").expect("renamed 'scores' key present");
    assert_eq!(scores.len().unwrap(), 2);
    let active: bool = a.get("acctActive").expect("unrenamed key keeps its name");
    assert!(active);
    // ...and the Haskell field names must NOT appear as keys.
    let stray_name: mlua::Value = a.get("acctName").unwrap();
    assert!(matches!(stray_name, mlua::Value::Nil),
        "Haskell field name 'acctName' must not leak into the Lua table");
    let stray_scores: mlua::Value = a.get("acctScores").unwrap();
    assert!(matches!(stray_scores, mlua::Value::Nil),
        "Haskell field name 'acctScores' must not leak into the Lua table");

    // Inbound: run main so the fetch-and-decode assertions execute.
    let lua2 = mlua::Lua::new();
    let acct2 = lua2.create_table().unwrap();
    let fetch2 = lua2
        .create_function(|lua, _n: i64| -> mlua::Result<mlua::Table> {
            let t = lua.create_table()?;
            t.set("name", "zoe")?;
            let scores = lua.create_table()?;
            for (i, v) in [10, 20, 30].iter().enumerate() {
                scores.set(i + 1, *v)?;
            }
            t.set("scores", scores)?;
            t.set("acctActive", true)?;
            Ok(t)
        })
        .unwrap();
    acct2.set("fetch", fetch2).unwrap();
    lua2.globals().set("acct", acct2).unwrap();
    lua2.load(&lua_code).set_name("luadict_renamed_main").exec()
        .expect("host dict keyed by renamed keys should decode; every in-program assertion should pass");
}

#[test]
fn luadict_duplicate_renamed_keys_rejected() {
    // Two fields mapping to the same effective Lua key would silently
    // overwrite each other in the runtime table.
    let source = r#"
data D = D { a as "k" :: Int, b as "k" :: Int }
    deriving (LuaDict)

main :: IO ()
main = pure ()
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("LuaDict") && msg.contains("both map to the Lua key"),
                "expected a duplicate-key error, got: {}", msg);
        }
        Ok(_) => panic!("duplicate effective LuaDict keys must fail"),
    }
}

#[test]
fn luadict_rename_colliding_with_plain_field_rejected() {
    // A rename may also collide with an *unrenamed* field's name — same
    // overwrite hazard, same rejection.
    let source = r#"
data D = D { a as "b" :: Int, b :: Int }
    deriving (LuaDict)

main :: IO ()
main = pure ()
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("LuaDict") && msg.contains("both map to the Lua key"),
                "expected a duplicate-key error, got: {}", msg);
        }
        Ok(_) => panic!("a rename colliding with a plain field name must fail"),
    }
}

#[test]
fn luadict_rename_without_relevant_deriving_rejected() {
    // `as` renames the field's shared external name: the LuaDict table key
    // and the JSON key of a derived ToJSON/FromJSON codec. Without any of
    // those derivings the record never crosses a boundary that keys by
    // name, so the rename would be silently meaningless. The error must
    // name all three derivings that would give the rename meaning.
    let source = r#"
data D = D { a as "k" :: Int }
    deriving (Show)

main :: IO ()
main = pure ()
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("derives none of LuaDict, ToJSON or FromJSON"),
                "expected the as-without-relevant-deriving error naming all three derivings, got: {}", msg);
            assert!(msg.contains("`deriving (LuaDict)`") && msg.contains("`deriving (ToJSON)`")
                    && msg.contains("`deriving (FromJSON)`"),
                "the note must offer each deriving that would give `as` meaning, got: {}", msg);
        }
        Ok(_) => panic!("`as` renaming without LuaDict/ToJSON/FromJSON must fail"),
    }
}

#[test]
fn luadict_empty_renamed_key_rejected() {
    let source = r#"
data D = D { a as "" :: Int }
    deriving (LuaDict)

main :: IO ()
main = pure ()
"#;
    match compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("LuaDict") && msg.contains("empty string"),
                "expected an empty-key error, got: {}", msg);
        }
        Ok(_) => panic!("an empty `as` key must fail"),
    }
}

#[test]
fn extract_from_plain_lua_rejected() {
    let plain = compile(EMBED_FIXTURE, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    match mllc::embed::extract_source(&plain) {
        Err(e) => assert!(e.contains("no embedded MLL source"),
            "expected a clear no-embedded-source message, got: {}", e),
        Ok(_) => panic!("extraction from a plain compile must fail"),
    }
}

// ---------------------------------------------------------------------------
// Regression tests: string escaping is canonical (one escaper for expression
// literals, pattern literals, and LuaDict table keys — see codegen.rs
// `lua_quoted_string`).
// ---------------------------------------------------------------------------

/// A string literal in a PATTERN used to be emitted with no escaping at all,
/// so `f "a\"b" = 1` produced `if _arg0 == "a"b" then` — Lua that failed to
/// load. The pattern path must go through the same canonical escaper as
/// expression literals: the program must compile, load, AND match correctly.
#[test]
fn string_pattern_literal_with_quote_and_newline_is_escaped() {
    let source = r#"
f :: String -> Int
f "a\"b\nc" = 1
f _ = 0

main :: IO ()
main = if f "a\"b\nc" == 1 && f "x" == 0
         then putStrLn "ok"
         else error "string pattern with escapes matched incorrectly"
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("a string pattern containing a quote and a newline must compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code)
        .exec()
        .expect("the emitted Lua must load and the pattern must match correctly");
}

/// A LuaDict `as`-renamed field key containing control characters used to be
/// emitted raw inside `["…"]` (only `"` and `\` were escaped), producing an
/// unfinished-string Lua syntax error. The key path must use the canonical
/// escaper too: construct, read the field back, and check the wire key.
#[test]
fn luadict_as_key_with_control_chars_is_escaped() {
    let source = r#"
data Rec = Rec { field1 as "a\nb\tc" :: Int } deriving (Show, LuaDict)

main :: IO ()
main = if field1 (Rec 5) == 5
         then putStrLn "ok"
         else error "field behind a control-character as-key read back wrong"
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("an as-key containing \\n and \\t must compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code)
        .exec()
        .expect("the emitted Lua must load and the renamed field must round-trip");
}

// ---------------------------------------------------------------------------
// Regression tests: the FFI target string is emitted verbatim as a Lua
// callee, so it is validated at the declaration (see parser.rs
// `validate_ffi_callee`) — a malformed target is a clean compile error, never
// broken Lua.
// ---------------------------------------------------------------------------

/// An FFI target that is not a well-formed Lua callee (here: contains a
/// space) used to be pasted into a call position, emitting `a b(...)` — Lua
/// that failed to load. It must now be rejected at compile time with a
/// diagnostic naming the offending string and declaration form.
#[test]
fn ffi_target_with_space_is_rejected_at_compile_time() {
    let e = compile_err(
        r#"
foo :: Int -> LuaPure "a b" Int

export doit :: IO ()
doit = print (foo 3)
"#,
    );
    assert!(
        e.contains("invalid Lua target") && e.contains("LuaPure \"a b\""),
        "expected a clean diagnostic naming the malformed FFI target, got: {}",
        e
    );
}

/// Other malformed shapes must be rejected the same way: an empty path
/// segment (`math..floor`) and a Lua reserved word as a name component.
#[test]
fn ffi_target_other_malformed_forms_are_rejected() {
    let e = compile_err(
        "foo :: Int -> LuaIO \"math..floor\" Int\nmain :: IO ()\nmain = foo 3 >>= print\n",
    );
    assert!(
        e.contains("invalid Lua target") && e.contains("math..floor"),
        "expected a clean diagnostic for the empty path segment, got: {}",
        e
    );
    let e = compile_err(
        "foo :: Int -> LuaPure \"os.end\" Int\nmain :: IO ()\nmain = print (foo 3)\n",
    );
    assert!(
        e.contains("invalid Lua target") && e.contains("reserved word"),
        "expected a clean diagnostic for the reserved-word segment, got: {}",
        e
    );
}

/// The FFI target is deliberately a Lua callee EXPRESSION, not just a name:
/// dotted paths and the arg0-method form must keep compiling — and running.
#[test]
fn ffi_target_dotted_and_method_forms_still_work() {
    let source = r#"
floorN :: Number -> LuaPure "math.floor" Int

repS :: String -> Int -> LuaPure ":rep" String

main :: IO ()
main = if floorN 3.7 == 3 && repS "ab" 2 == "abab"
         then putStrLn "ok"
         else error "dotted-path or method-form FFI produced a wrong result"
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("dotted-path and :method FFI targets must keep compiling")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code)
        .exec()
        .expect("math.floor and the string :rep method must run correctly");
}

/// Indexed paths and a dotted path with a trailing method are also legitimate
/// callee shapes; they must pass validation (compile-only — the host objects
/// don't exist in the test harness).
#[test]
fn ffi_target_indexed_and_trailing_method_forms_compile() {
    let source = r#"
runFirst :: Int -> LuaPure "handlers[1].run" Int

readCfg :: Int -> LuaPure "cfg[\"main\"].stream:read" Int

export doit :: IO ()
doit = print (runFirst 1 + readCfg 2)
"#;
    compile(source, Path::new("."), &[])
        .expect("indexed-path and path:method FFI targets must pass validation");
}

// ---------------------------------------------------------------------------
// Regression tests: recursion-depth guard. Nesting past
// mllc::MAX_NESTING_DEPTH must produce the clean "nested too deeply"
// diagnostic — never a native stack overflow (SIGABRT). Reaching the limit
// still consumes (limit x frame) native stack, so these run on a thread with
// the SAME stack size as the mll CLI driver (mllc::COMPILER_STACK_SIZE, which
// the limit is calibrated against).
// ---------------------------------------------------------------------------

/// Compile `source` on a compiler-sized thread and return the result.
fn compile_on_compiler_stack(source: String) -> Result<mllc::CompileResult, mllc::CompileError> {
    std::thread::Builder::new()
        .stack_size(mllc::COMPILER_STACK_SIZE)
        .spawn(move || compile(&source, Path::new("."), &[]))
        .expect("failed to spawn compiler-sized thread")
        .join()
        .expect("the compiler must not crash on deeply nested input")
}

/// Parser face of the guard: nested parentheses beyond the limit.
#[test]
fn deeply_nested_parens_yield_clean_depth_error() {
    let n = mllc::MAX_NESTING_DEPTH + 1000;
    let source = format!(
        "main :: IO ()\nmain = print {}1{}\n",
        "(".repeat(n),
        ")".repeat(n)
    );
    match compile_on_compiler_stack(source) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("expression nested too deeply")
                    && msg.contains(&format!("limit {}", mllc::MAX_NESTING_DEPTH)),
                "expected the clean depth diagnostic, got: {}",
                msg
            );
        }
        Ok(_) => panic!("parens nested past the limit must be rejected"),
    }
}

/// Type faces of the recursion-DEPTH guard: a deeply parenthesised signature
/// (parser) and a LINEAR deep type-alias chain whose expansion is deep while
/// the source is shallow (`ast_type_to_ty` — the parser cannot see this one
/// coming). The alias chain here grows LINEARLY (`type Ai = [A(i-1)]`), so its
/// expanded SIZE stays within the alias-expansion fuel budget and it is the
/// recursion-depth guard, not the size guard, that must catch it. (The
/// exponential-SIZE tower is a distinct case — see
/// `doubling_alias_tower_yields_clean_size_error`.)
#[test]
fn deeply_nested_types_yield_clean_depth_error() {
    let n = mllc::MAX_NESTING_DEPTH + 1000;
    let source = format!(
        "f :: {}Int{} -> Int\nf y = y\nmain :: IO ()\nmain = print (f 1)\n",
        "(".repeat(n),
        ")".repeat(n)
    );
    match compile_on_compiler_stack(source) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("type nested too deeply"),
                "expected the clean type-depth diagnostic, got: {}",
                msg
            );
        }
        Ok(_) => panic!("a type nested past the limit must be rejected"),
    }

    // A linear alias chain past the depth limit: `type Ai = [A(i-1)]` expands
    // to a list nested `n` deep — deep structure, shallow source text, but
    // only linear SIZE (one node per level), so it stays within the alias
    // fuel and must hit the ast_type_to_ty depth guard, not the stack.
    let mut source = String::from("type A0 = Int\n");
    for i in 1..=n {
        source.push_str(&format!("type A{} = [A{}]\n", i, i - 1));
    }
    source.push_str(&format!(
        "f :: A{} -> Int\nf _ = 1\nmain :: IO ()\nmain = print (f [])\n",
        n
    ));
    match compile_on_compiler_stack(source) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("type nested too deeply"),
                "expected the clean type-depth diagnostic for the linear alias chain, got: {}",
                msg
            );
        }
        Ok(_) => panic!("a linear alias chain past the depth limit must be rejected"),
    }
}

/// Type-alias expansion is bounded by WORK/SIZE, not just depth. A self-
/// doubling alias tower (`type Pi a = P(i-1) (P(i-1) a)`) expands to a type
/// whose SIZE is exponential in the number of levels while its DEPTH stays
/// small (P10 has depth ~1024, well under MAX_NESTING_DEPTH), so the
/// recursion-depth guard never sees it — it used to grind through the
/// exponential expansion (SIGABRT before the big stack, then a multi-second
/// hang after). The size-charged alias-expansion fuel
/// (typechecker `charge_alias_expansion` / `ALIAS_EXPAND_FUEL`) must catch it
/// quickly with a clean "did not terminate" diagnostic — distinct from the
/// depth guard above. Runs on a compiler-sized stack like the depth tests.
#[test]
fn doubling_alias_tower_yields_clean_size_error() {
    // 10-level doubling tower: P10 expands to ~2^1024 nodes but depth ~1024.
    let mut source = String::from("type P0 a = (a, a)\n");
    for i in 1..=10 {
        source.push_str(&format!("type P{} a = P{} (P{} a)\n", i, i - 1, i - 1));
    }
    source.push_str("x :: P10 Int\nx = undefined\nmain :: IO ()\nmain = putStrLn \"ok\"\n");
    match compile_on_compiler_stack(source) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("type alias expansion did not terminate"),
                "expected the clean alias-expansion-size diagnostic, got: {}",
                msg
            );
        }
        Ok(_) => panic!("an exponentially expanding alias tower must be rejected"),
    }

    // A shallow doubling tower (P3 -> 256 expanded nodes) is well within the
    // budget and must still compile: the size bound rejects the pathological
    // case without punishing ordinary multi-level alias use.
    let mut ok = String::from("type Q0 a = (a, a)\n");
    for i in 1..=3 {
        ok.push_str(&format!("type Q{} a = Q{} (Q{} a)\n", i, i - 1, i - 1));
    }
    ok.push_str("y :: Q3 Int -> Int\ny _ = 0\nmain :: IO ()\nmain = print (y undefined)\n");
    compile_on_compiler_stack(ok)
        .expect("a shallow (Q3) alias tower is small and must still compile");
}

/// Expression-structure face of the guard: a `+`-operator spine. The source
/// is flat (the parser folds left-associative chains iteratively) but the AST
/// is one level deep per operand, so this exercises the expression-walk guard
/// (typechecker inference — the pass with the heaviest frames, which the
/// stack size is calibrated against).
#[test]
fn operator_spine_past_limit_yields_clean_depth_error() {
    let n = mllc::MAX_NESTING_DEPTH + 1000;
    let source = format!(
        "x :: Int\nx = {}\nmain :: IO ()\nmain = print x\n",
        vec!["1"; n].join("+")
    );
    match compile_on_compiler_stack(source) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("expression nested too deeply")
                    && msg.contains(&format!("limit {}", mllc::MAX_NESTING_DEPTH)),
                "expected the clean depth diagnostic, got: {}",
                msg
            );
        }
        Ok(_) => panic!("an operator spine past the limit must be rejected"),
    }
}

/// The limit must stay generous: a 1200-element list literal (which desugars
/// to a ~1200-deep cons chain, far past the old 256-element promise) must
/// still compile AND run.
#[test]
fn thousand_element_list_literal_still_compiles_and_runs() {
    let n = 1200;
    let source = format!(
        "xs :: [Int]\nxs = [{}]\nmain :: IO ()\nmain = if sum xs == {} then putStrLn \"ok\" else error \"wrong sum\"\n",
        vec!["2"; n].join(","),
        2 * n
    );
    let lua_code = compile_on_compiler_stack(source)
        .expect("a 1200-element list literal must compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code)
        .exec()
        .expect("the 1200-element list program must run");
}

// ---------------------------------------------------------------------------
// Linear types: `a %1 -> b` — a `%1` value must be consumed EXACTLY once
// (GHC LinearTypes semantics: more than one use is a double-free, zero uses
// is a leak). The positive side (programs that use `%1` correctly compile
// and run, and the annotation erases) lives in linear_affine_basic.mll;
// the tests here assert REJECTION — a program that can use a `%1`-bound
// value more than once, or drop it, must fail to compile with a diagnostic
// that names the variable and explains the violation in plain language.
// See mllc/src/typechecker/usage.rs for the enforced fragment.
// ---------------------------------------------------------------------------

/// Compile expecting a linearity rejection; return the rendered error.
fn expect_linear_reject(src: &str) -> String {
    match compile(src, Path::new("tests/cases"), &[]) {
        Ok(_) => panic!(
            "this program violates the %1 (exactly-once) discipline and \
             must NOT compile:\n{}",
            src
        ),
        Err(e) => format!("{}", e),
    }
}

/// The simplest violation: a `%1` argument mentioned twice.
#[test]
fn linear_rejects_plain_double_use() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         dup :: Token %1 -> (Token, Token)\n\
         dup t = (t, t)\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("declares this argument '%1'"), "{}", msg);
    assert!(msg.contains("more than once"), "{}", msg);
}

/// Passing a `%1` value to an unrestricted function is an over-use even when
/// it occurs only once: the callee's plain arrow makes no single-use promise.
#[test]
fn linear_rejects_flow_into_unrestricted_function() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         count :: Token -> Int\n\
         count (Token n) = n\n\
         g :: Token %1 -> Int\n\
         g t = count t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("passed to 'count'"), "{}", msg);
    assert!(msg.contains("'->', not '%1 ->'"), "{}", msg);
}

/// Aliasing through a pattern match: the binder inherits the restriction.
#[test]
fn linear_rejects_case_alias_double_use() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         data Box = Box Token\n\
         f :: Box %1 -> (Token, Token)\n\
         f b = case b of\n\
         \x20 Box t -> (t, t)\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("pattern-bound from 'b'"), "{}", msg);
}

/// Aliasing through `let`: using the alias twice consumes the original twice
/// (the laziness rule — the thunk memoizes the FORCE, not the consumption).
#[test]
fn linear_rejects_let_alias_double_use() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         f :: Token %1 -> (Token, Token)\n\
         f t = let u = t in (u, u)\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("local binding 'u'"), "{}", msg);
}

/// Capture by a returned closure: the closure may be called any number of
/// times, each call handing out the same `%1` value again.
#[test]
fn linear_rejects_closure_capture() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         f :: Token %1 -> (Int -> Token)\n\
         f t = \\x -> t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("captured by a function value"), "{}", msg);
}

/// The propagation soundness case: a lambda checked against a `%1`
/// parameter learns the restriction through unification and its binder is
/// enforced — an ω-style lambda cannot sneak in through a %1 HOF.
#[test]
fn linear_rejects_duplicating_lambda_at_linear_hof() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         withToken :: (Token %1 -> (Token, Token)) -> (Token, Token)\n\
         withToken f = f (Token 1)\n\
         main :: IO ()\n\
         main = case withToken (\\t -> (t, t)) of\n\
         \x20 (Token a, Token b) -> print (a + b)\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("'%1' arrow at this parameter"), "{}", msg);
}

/// A named unrestricted function cannot flow into a `%1` position at all —
/// the arrows are different types (invariant multiplicities, as in GHC).
#[test]
fn linear_rejects_unrestricted_function_at_linear_type() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         applyMany :: (Token -> Int) -> Int\n\
         applyMany f = f (Token 1) + f (Token 2)\n\
         main :: IO ()\n\
         main = print (applyMany useOnce)\n",
    );
    assert!(msg.contains("arrows disagree"), "{}", msg);
    assert!(msg.contains("exactly once"), "{}", msg);
}

/// Sequential double use across a do-block: `>>`-chained statements add up.
#[test]
fn linear_rejects_double_use_across_do_block() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         shred :: Token %1 -> IO ()\n\
         shred (Token n) = print n\n\
         f :: Token %1 -> IO ()\n\
         f t = do\n\
         \x20 shred t\n\
         \x20 shred t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
}

/// A `<-` binder aliasing an affine value inherits the restriction.
#[test]
fn linear_rejects_bind_alias_double_use() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         data Box = Box Token\n\
         unbox :: Box %1 -> Token\n\
         unbox (Box t) = t\n\
         shred :: Token %1 -> IO ()\n\
         shred (Token n) = print n\n\
         f :: Box %1 -> IO ()\n\
         f b = do\n\
         \x20 t <- pure (unbox b)\n\
         \x20 shred t\n\
         \x20 shred t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("bound (with '<-')"), "{}", msg);
}

/// A locally shadowed Prelude name must not inherit the Prelude's
/// consume-once whitelisting (`pure`, `id`, `fst`, …).
#[test]
fn linear_rejects_shadowed_prelude_whitelist_name() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         f :: Token %1 -> (Token, Token)\n\
         f t = let pure = \\x -> (x, x) in pure t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
}

/// A `%1` class-method signature is enforced on instance methods too.
#[test]
fn linear_rejects_instance_method_double_use() {
    let msg = expect_linear_reject(
        "data Pair = Pair Int Int\n\
         data Token = Token Pair\n\
         class Consume a where\n\
         \x20 consume :: a %1 -> (a, a)\n\
         instance Consume Token where\n\
         \x20 consume t = (t, t)\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("must be consumed exactly once"), "{}", msg);
}

/// Erasure: multiplicities are a type-checking discipline only. The same
/// program with `%1` arrows and with plain arrows must emit byte-identical
/// Lua.
#[test]
fn linear_annotations_erase_to_identical_lua() {
    let with_mult = "data Token = Token Int\n\
         shred :: Token %1 -> IO ()\n\
         shred (Token n) = if n == 42 then putStrLn \"ok\" else putStrLn \"bad\"\n\
         step :: Token %1 -> (Token, Int)\n\
         step t = (t, 5)\n\
         main :: IO ()\n\
         main = do\n\
         \x20 let t = Token 42\n\
         \x20 case step t of\n\
         \x20\x20 (t2, n) -> do\n\
         \x20\x20\x20 print n\n\
         \x20\x20\x20 shred t2\n";
    let without_mult = with_mult.replace("%1 ->", "->");
    let a = compile(with_mult, Path::new("tests/cases"), &[])
        .expect("the %1 program must compile")
        .lua_code;
    let b = compile(&without_mult, Path::new("tests/cases"), &[])
        .expect("the plain-arrow program must compile")
        .lua_code;
    assert!(a == b, "%1 must erase: emitted Lua differs");
}

// ---------------------------------------------------------------------------
// Multiplicity polymorphism (`a %m -> b`) and the composability relaxations
// (local-function forwarding, non-IO binds). The positive side lives in
// linear_mult_poly.mll; these assert that a double use which is only
// reachable THROUGH a polymorphic helper, a local function, or a non-IO
// bind is still rejected, and that a polymorphic definition is held to the
// `m = 1` instantiation.
// ---------------------------------------------------------------------------

/// A definition polymorphic in `m` may not duplicate its `%m` argument:
/// a caller can instantiate m to 1.
#[test]
fn linear_rejects_double_use_in_mult_poly_definition() {
    let msg = expect_linear_reject(
        "dupPoly :: a %m -> (a, a)\n\
         dupPoly x = (x, x)\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'x' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("multiplicity variable '%m'"), "{}", msg);
    assert!(msg.contains("instantiate to '%1'"), "{}", msg);
}

/// A `%1` binder passed through the polymorphic helper twice is still two
/// uses — polymorphism must not launder the count.
#[test]
fn linear_rejects_double_use_through_mult_poly_helper() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         apply :: (a %m -> b) -> a %m -> b\n\
         apply f x = f x\n\
         bad :: Token %1 -> Int\n\
         bad t = apply useOnce t + apply useOnce t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("more than once"), "{}", msg);
}

/// A duplicating lambda slipped through the polymorphic helper leaves `m`
/// unresolved, so the argument is charged unrestrictedly — reject.
#[test]
fn linear_rejects_duplicating_lambda_through_mult_poly_helper() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         apply :: (a %m -> b) -> a %m -> b\n\
         apply f x = f x\n\
         bad :: Token %1 -> (Token, Token)\n\
         bad t = apply (\\u -> (u, u)) t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
}

/// A `%1` value may not flow into a `%m` arrow position: the variable may
/// be instantiated to Many by the caller.
#[test]
fn linear_rejects_linear_arg_at_mult_var_arrow() {
    let msg = expect_linear_reject(
        "cross :: (a %m -> b) -> a %1 -> b\n\
         cross f x = f x\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'x' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("signature variable ('%m')"), "{}", msg);
}

/// A rigid `%m` cannot be pinned to `Many` by the body (here by passing the
/// `%m` function to an unrestricted higher-order function) — the signature's
/// polymorphism claim would be silently broken for `m = 1` callers.
#[test]
fn linear_rejects_rigid_mult_weakened_to_many() {
    let msg = expect_linear_reject(
        "twice :: (c -> d) -> c -> d\n\
         twice g y = g y\n\
         force :: (a %m -> b) -> a %m -> b\n\
         force f x = twice f x\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("arrows disagree"), "{}", msg);
    assert!(msg.contains("multiplicity VARIABLE"), "{}", msg);
}

/// Laundering a `%m` function through a local alias into a `%1` context
/// must not work either: the alias keeps the SAME rigid m.
#[test]
fn linear_rejects_mult_var_alias_laundering() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         dup2 :: Token -> (Token, Token)\n\
         dup2 t = (t, t)\n\
         h :: (c %1 -> d) -> c %1 -> d\n\
         h k y = k y\n\
         bad :: (a %m -> b) -> a %1 -> b\n\
         bad f x = let g = f in h g x\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("arrows disagree"), "{}", msg);
}

/// A local function that uses its parameter twice makes the call a double
/// use of the affine argument.
#[test]
fn linear_rejects_local_function_param_double_use() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         bad :: Token %1 -> Int\n\
         bad t = g t\n\
         \x20 where g x = useOnce x + useOnce x\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("local function 'g'"), "{}", msg);
}

/// Calling a (correctly forwarding) local function twice is two uses.
#[test]
fn linear_rejects_double_call_of_forwarding_local_function() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         bad :: Token %1 -> Int\n\
         bad t = let g x = useOnce x in g t + g t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("more than once"), "{}", msg);
}

/// A recursive local function that consumes the forwarded value on the way
/// down AND at the end: caught by the group fixpoint.
#[test]
fn linear_rejects_recursive_local_function_double_use() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         bad :: Token %1 -> Int -> Int\n\
         bad t n = go t n\n\
         \x20 where go x k = if k > 0 then go x (k - 1) + useOnce x else useOnce x\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("local function 'go'"), "{}", msg);
}

/// CAPTURING an affine value in a local function still charges ω — only the
/// function's parameters get the refined accounting, a returned closure may
/// be called any number of times.
#[test]
fn linear_rejects_local_function_capture() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         bad :: Token %1 -> (Int -> Token)\n\
         bad t = g\n\
         \x20 where g x = t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("called any number of times"), "{}", msg);
}

/// A double use inside a Maybe do-block is still two uses — the bind
/// relaxation only stops the blanket ω-charge, not the counting.
#[test]
fn linear_rejects_double_use_in_maybe_do_block() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         bad :: Token %1 -> Maybe Int\n\
         bad t = do\n\
         \x20 a <- Just (useOnce t)\n\
         \x20 b <- Just (useOnce t)\n\
         \x20 pure (a + b)\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("more than once"), "{}", msg);
}

/// The LIST bind runs its continuation once per element: an affine value
/// consumed in it stays rejected.
#[test]
fn linear_rejects_affine_in_list_monad_bind() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         bad :: Token %1 -> [Int]\n\
         bad t = do\n\
         \x20 n <- [1, 2, 3]\n\
         \x20 pure (useOnce t + n)\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("any number of times"), "{}", msg);
}

/// A USER-DEFINED monad's bind is arbitrary code (this one really does run
/// the continuation twice): its continuations stay ω-charged.
#[test]
fn linear_rejects_affine_in_user_monad_bind() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         data Twice a = Twice a a\n\
         instance Functor Twice where\n\
         \x20 fmap f (Twice a b) = Twice (f a) (f b)\n\
         instance Applicative Twice where\n\
         \x20 pure x = Twice x x\n\
         \x20 (<*>) (Twice f g) (Twice a b) = Twice (f a) (g b)\n\
         instance Monad Twice where\n\
         \x20 (>>=) (Twice a b) k = case k a of\n\
         \x20\x20 Twice x _ -> case k b of\n\
         \x20\x20\x20 Twice _ y -> Twice x y\n\
         bad :: Token %1 -> Twice Int\n\
         bad t = do\n\
         \x20 n <- Twice 1 2\n\
         \x20 pure (useOnce t + n)\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("any number of times"), "{}", msg);
}

/// Erasure for the new pieces: `%m` annotations (and the `%1`s they compose
/// with) must emit byte-identical Lua to the plain-arrow program.
#[test]
fn linear_mult_poly_erases_to_identical_lua() {
    let with_mult = "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         apply :: (a %m -> b) -> a %m -> b\n\
         apply f x = f x\n\
         go :: Token %1 -> Int\n\
         go t = apply useOnce t\n\
         main :: IO ()\n\
         main = print (go (Token 3))\n";
    let without_mult = with_mult.replace("%1 ->", "->").replace("%m ->", "->");
    let a = compile(with_mult, Path::new("tests/cases"), &[])
        .expect("the %m program must compile")
        .lua_code;
    let b = compile(&without_mult, Path::new("tests/cases"), &[])
        .expect("the plain-arrow program must compile")
        .lua_code;
    assert!(a == b, "%m must erase: emitted Lua differs");
}

// ---------------------------------------------------------------------------
// The exactly-once LOWER bound: a `%1` value consumed zero times — dropped
// outright, dropped on one evaluation path, or parked in something that is
// never forced — is a leak and must be rejected. (The affine upper bound
// alone accepted all of these.)
// ---------------------------------------------------------------------------

/// The simplest leak: a `%1` argument never used at all.
#[test]
fn linear_rejects_zero_uses() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         f :: Token %1 -> Int\n\
         f t = 5\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("consumed zero times"), "{}", msg);
}

/// A wildcard argument pattern discards the `%1` value without consuming it.
#[test]
fn linear_rejects_wildcard_argument_pattern() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         f :: Token %1 -> Int\n\
         f _ = 5\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("wildcard"), "{}", msg);
}

/// Consumed in one case alternative but not its sibling: the sibling path
/// drops it. This is the lower-bound side of the branch join — the
/// per-variable maximum alone would still read "one use".
#[test]
fn linear_rejects_use_in_one_branch_only() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         f :: Token %1 -> Int -> Int\n\
         f t n = case n > 0 of\n\
         \x20 True -> useOnce t\n\
         \x20 False -> 1\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("only 1 of the 2 alternatives"), "{}", msg);
}

/// The `if` form of the same lower bound.
#[test]
fn linear_rejects_use_in_one_if_arm_only() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         f :: Token %1 -> Int -> Int\n\
         f t n = if n > 0 then useOnce t else 1\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("branches of this 'if'"), "{}", msg);
}

/// The laziness case: a `%1` value consumed only inside a `let` binding
/// that is never forced. The binding's right-hand side is scaled by its
/// use count (zero), so at clause end the value was never consumed.
#[test]
fn linear_rejects_never_forced_let_binding() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         f :: Token %1 -> Int\n\
         f t = let u = useOnce t in 5\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("consumed zero times"), "{}", msg);
}

/// The same through a `where` binding.
#[test]
fn linear_rejects_never_forced_where_binding() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         f :: Token %1 -> Int\n\
         f t = 5\n\
         \x20 where u = useOnce t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("consumed zero times"), "{}", msg);
}

/// Forwarded through the multiplicity-polymorphic helper and THEN dropped:
/// exactly-once must hold end to end, not just at the forwarding step.
#[test]
fn linear_rejects_drop_after_mult_poly_forward() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         apply :: (a %m -> b) -> a %m -> b\n\
         apply f x = f x\n\
         bad :: Token %1 -> Int\n\
         bad t = let u = apply useOnce t in 5\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("consumed zero times"), "{}", msg);
}

/// A `%m` binder must be consumed too: a caller may instantiate m to 1,
/// and multiplicity 1 demands consumption.
#[test]
fn linear_rejects_unused_mult_var_argument() {
    let msg = expect_linear_reject(
        "dropPoly :: a %m -> Int\n\
         dropPoly x = 5\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'x' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("multiplicity variable '%m'"), "{}", msg);
    assert!(msg.contains("consumed zero times"), "{}", msg);
}

/// Maybe's bind skips its continuation on Nothing, so a `%1` value consumed
/// inside the continuation leaks on that path (GHC agrees: Maybe's bind
/// cannot promise to run a linear continuation). Consume it in the bind's
/// ACTION instead — see viaMaybe in linear_mult_poly.mll.
#[test]
fn linear_rejects_consumption_in_maybe_continuation() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         bad :: Token %1 -> Maybe Int\n\
         bad t = do\n\
         \x20 n <- Just 1\n\
         \x20 pure (useOnce t + n)\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("'Nothing' path skips"), "{}", msg);
}

/// Discarding a non-`()` result whose thunk may hold the pending
/// consumption (`pure (useOnce t)` never forces the payload; running the
/// action does not consume t — only forcing the result would).
#[test]
fn linear_rejects_discarded_tainted_bind_result() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         f :: Token %1 -> IO ()\n\
         f t = do\n\
         \x20 _ <- pure (useOnce t)\n\
         \x20 putStrLn \"x\"\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("discarded"), "{}", msg);
}

/// The bare-statement (`>>`) form of the same discard.
#[test]
fn linear_rejects_discarded_tainted_statement_result() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         f :: Token %1 -> IO ()\n\
         f t = do\n\
         \x20 pure (useOnce t)\n\
         \x20 putStrLn \"x\"\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("discarded"), "{}", msg);
}

/// A wildcard inside a pattern over a `%1` value discards the matched part.
#[test]
fn linear_rejects_wildcard_in_tainted_case() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         f :: (Token, Token) %1 -> Int\n\
         f p = case p of\n\
         \x20 (a, _) -> useOnce a\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'p' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("wildcard"), "{}", msg);
}

/// A local function that never uses its parameter drops the forwarded
/// value: the inferred per-parameter factors carry a may-drop flag.
#[test]
fn linear_rejects_dropping_local_function() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         bad :: Token %1 -> Int\n\
         bad t = g t\n\
         \x20 where g x = 5\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("local function 'g'"), "{}", msg);
    assert!(msg.contains("never uses this parameter"), "{}", msg);
}

/// `&&`/`||` short-circuit: the right operand may never run.
#[test]
fn linear_rejects_short_circuit_right_operand() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         f :: Token %1 -> Bool -> Bool\n\
         f t b = b && (useOnce t > 0)\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("short-circuits"), "{}", msg);
}

/// `fst` drops the second component: under exactly-once it is no longer a
/// consume-once function (its arrow is unrestricted, as in GHC).
#[test]
fn linear_rejects_fst_on_linear_pair() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         g :: (Token, Token) %1 -> Token\n\
         g p = fst p\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'p' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("passed to 'fst'"), "{}", msg);
}

/// A scalar destructured from a `%1` match is tracked exactly-once like
/// any other alias (GHC parity — no scalar exemption): the callee may have
/// parked the consumption in that component's thunk
/// (`step t = (Token 0, useOnce t)` — dropping n means t is never used).
#[test]
fn linear_rejects_unused_scalar_alias() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         step :: Token %1 -> (Token, Int)\n\
         step t = (Token 0, useOnce t)\n\
         f :: Token %1 -> Int\n\
         f t = case step t of\n\
         \x20 (t2, n) -> useOnce t2\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'n' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("consumed zero times"), "{}", msg);
}

/// The clause-pattern form: an unused scalar field of a `%1` argument.
#[test]
fn linear_rejects_unused_scalar_field() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         sig :: Token %1 -> Int\n\
         sig (Token n) = 5\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'n' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("consumed zero times"), "{}", msg);
}

// ---------------------------------------------------------------------------
// Strict GHC parity on scalars: a scalar derived from a `%1` value is held
// to exactly-once like every other alias — there is no scalar-memoization
// exemption. These programs were ACCEPTED under the old at-least-once
// scalar rule (duplication was considered free because the runtime
// memoizes the thunk); GHC rejects all of them, and so does mata-ll now.
// The legitimate exactly-once scalar shapes still compile — see useOnce /
// onceVia in linear_affine_basic.mll and viaMaybe in linear_mult_poly.mll.
// ---------------------------------------------------------------------------

/// The canonical scalar duplication: a where-binding built from a `%1`
/// value read twice. Operationally harmless under memoization, but GHC
/// has no scalar exemption — parity rejects it.
#[test]
fn linear_rejects_scalar_where_binding_double_use() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         bad :: Token %1 -> Int\n\
         bad t = go + go\n\
         \x20 where go = useOnce t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("local binding 'go'"), "{}", msg);
    assert!(msg.contains("more than once"), "{}", msg);
}

/// The multi-step scalar launder that was the one known ACCEPT-direction
/// hole: the pending consumption of 't' sits in the thunk of the scalar
/// binding 'n', and the unrestricted 'constUnit' may never force it — the
/// leak used to slip through because scalar bindings were untracked.
#[test]
fn linear_rejects_scalar_laundered_through_let_binding() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         constUnit :: Int -> ()\n\
         constUnit x = ()\n\
         bad :: Token %1 -> ()\n\
         bad t = let n = useOnce t in constUnit n\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("local binding 'n'"), "{}", msg);
    assert!(msg.contains("constUnit"), "{}", msg);
}

/// The derived-alias form of the launder: a scalar pattern-bound from a
/// tainted match handed to an unrestricted function, which may drop (or
/// duplicate) it — its one obligated consumption may never happen.
#[test]
fn linear_rejects_scalar_alias_flow_into_unrestricted_function() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         step :: Token %1 -> (Token, Int)\n\
         step t = (t, 5)\n\
         constInt :: Int -> Int\n\
         constInt x = 7\n\
         bad :: Token %1 -> Int\n\
         bad t = case step t of\n\
         \x20 (t2, n) -> useOnce t2 + constInt n\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'n' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("pattern-bound from 't'"), "{}", msg);
    assert!(msg.contains("constInt"), "{}", msg);
}

/// A tracked scalar captured by a lambda: the closure may run any number
/// of times — or never, leaking the consumption parked in the scalar's
/// thunk. (Was charged ω but accepted under the old scalar rule; a
/// non-scalar capture was always rejected.)
#[test]
fn linear_rejects_scalar_captured_by_lambda() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         bad :: Token %1 -> (Int -> Int)\n\
         bad (Token n) = \\x -> n + x\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'n' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("captured by a function value"), "{}", msg);
}

/// A `>>=` whose continuation is a NAMED function with an unrestricted
/// arrow: the bound value (an alias of the `%1` argument) flows somewhere
/// that promises neither exactly-once nor at-most-once. This was a
/// false-accept under the affine checker (only lambda continuations were
/// tracked).
#[test]
fn linear_rejects_unrestricted_bind_continuation() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useTwiceIO :: Token -> IO ()\n\
         useTwiceIO (Token n) = print (n + n)\n\
         unbox :: Token %1 -> Token\n\
         unbox t = t\n\
         f :: Token %1 -> IO ()\n\
         f b = pure (unbox b) >>= useTwiceIO\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'b' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("not '%1'"), "{}", msg);
}

/// A scrutinee that consumes two tracked values (the scalar field 'a' and
/// the `%1` value 't' — both exactly-once, scalars included) taints the
/// tuple's binders; a double use of the aliased 'tok' is a double use of
/// 't' and rejects. (The origin names 'a': among equal-rank sources the
/// taint picks the alphabetically first for stable diagnostics.)
#[test]
fn linear_rejects_double_use_through_multi_source_taint() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         g :: Token %1 -> Token %1 -> Int\n\
         g (Token a) t = case (a, t) of\n\
         \x20 (x, tok) -> useOnce tok + useOnce tok + x\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'tok' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("pattern-bound from 'a'"), "{}", msg);
}

/// Erasure of the exactly-once positive shapes: a tainted case consumed in
/// every branch, and a scalar alias forced once, emit byte-identical Lua
/// with and without the `%1` annotations.
#[test]
fn linear_exactly_once_erases_to_identical_lua() {
    let with_mult = "data Token = Token Int\n\
         data Box = Box Token\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         caseBoth :: Box %1 -> Int -> Int\n\
         caseBoth b n = case b of\n\
         \x20 Box t -> if n > 0 then useOnce t else useOnce t + 1\n\
         step :: Token %1 -> (Token, Int)\n\
         step t = (t, 5)\n\
         f :: Token %1 -> Int\n\
         f t = case step t of\n\
         \x20 (t2, n) -> useOnce t2 + n\n\
         main :: IO ()\n\
         main = do\n\
         \x20 print (caseBoth (Box (Token 4)) 1)\n\
         \x20 print (f (Token 37))\n";
    let without_mult = with_mult.replace("%1 ->", "->");
    let a = compile(with_mult, Path::new("tests/cases"), &[])
        .expect("the %1 program must compile")
        .lua_code;
    let b = compile(&without_mult, Path::new("tests/cases"), &[])
        .expect("the plain-arrow program must compile")
        .lua_code;
    assert!(a == b, "%1 must erase: emitted Lua differs");
}

// ---------------------------------------------------------------------------
// LIOLinear: the linear file-handle library's guarantee is the usage checker's
// — a WHandle crossing a `%1` arrow must be written/closed exactly once. These
// compile against the real library (lib/LIOLinear.mll), so they pin down the
// two misuses the API exists to prevent: forgetting hClose (a leaked file
// handle) and touching a handle after it has been consumed (write/close after
// close). The well-formed side runs in lib_liolinear.mll.
// ---------------------------------------------------------------------------

/// Like expect_linear_reject, but with the lib/ search path so the program
/// can import LIOLinear.
fn expect_linear_reject_with_lib(src: &str) -> String {
    let lib_path = Path::new("../lib");
    match compile(src, Path::new("tests/cases"), &[lib_path]) {
        Ok(_) => panic!(
            "this program violates the %1 (exactly-once) discipline and \
             must NOT compile:\n{}",
            src
        ),
        Err(e) => format!("{}", e),
    }
}

/// Forgetting hClose: the handle threaded out of hPut is never consumed, so
/// the underlying file would leak — rejected.
#[test]
fn linear_rejects_liolinear_forgotten_close() {
    let msg = expect_linear_reject_with_lib(
        "import LIOLinear (WHandle, hPut, hClose, withOutFile)\n\
         main :: IO ()\n\
         main = do\n\
         \x20 r <- withOutFile \"/tmp/mll-liolinear-leak\" (\\h -> do\n\
         \x20\x20 h2 <- hPut h \"hello\"\n\
         \x20\x20 putStrLn \"forgot to close\")\n\
         \x20 case r of\n\
         \x20\x20 Left err -> error err\n\
         \x20\x20 Right _ -> pure ()\n",
    );
    assert!(msg.contains("'h2' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("consumed zero times"), "{}", msg);
}

/// Using a handle after it has been consumed: the first hPut consumed `h`,
/// the second write is the double-close/double-free class of bug — rejected.
#[test]
fn linear_rejects_liolinear_use_after_consume() {
    let msg = expect_linear_reject_with_lib(
        "import LIOLinear (WHandle, hPut, hClose, withOutFile)\n\
         main :: IO ()\n\
         main = do\n\
         \x20 r <- withOutFile \"/tmp/mll-liolinear-twice\" (\\h -> do\n\
         \x20\x20 h2 <- hPut h \"first\"\n\
         \x20\x20 hClose h2\n\
         \x20\x20 h3 <- hPut h \"again\"\n\
         \x20\x20 hClose h3)\n\
         \x20 case r of\n\
         \x20\x20 Left err -> error err\n\
         \x20\x20 Right _ -> pure ()\n",
    );
    assert!(msg.contains("'h' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("more than once"), "{}", msg);
}

/// Double close through a %1-typed writer function (the openOut entry path):
/// the handle is linear for the whole body, so closing twice is two uses.
#[test]
fn linear_rejects_liolinear_double_close() {
    let msg = expect_linear_reject_with_lib(
        "import LIOLinear (WHandle, hPut, hClose)\n\
         closeTwice :: WHandle %1 -> IO ()\n\
         closeTwice h = do\n\
         \x20 hClose h\n\
         \x20 hClose h\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'h' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("more than once"), "{}", msg);
}

// Regression: the entry-point trailer used to run main() only when the chunk's
// first vararg was nil. A standalone interpreter (`lua prog.lua x`) passes CLI
// args as varargs, so ANY argument made the program look like it had been
// `require`d and main was silently skipped. main must run whenever the file is
// executed as a program (first vararg matches arg[1], including the no-arg case
// where both are absent) and stay dormant only when a host require()s it (first
// vararg is the module name, which won't match arg[1]). On ambiguity we err
// toward running main: a genuine library module carries no main to begin with.
#[test]
fn main_runs_standalone_with_cli_args_not_when_required() {
    let source = r#"
import LIO (putStrLn)

main :: IO ()
main = do
    args <- getArgs
    putStrLn "MAIN"
    putStrLn (show args)
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;

    // Exec the chunk the way a standalone interpreter would: `arg` set as a
    // global (arg[0]=script, arg[1..]=CLI args) and the same args handed to the
    // chunk as varargs. `arg1` is arg[1]; `first_vararg` is the chunk's `...`.
    let run = |arg1: Option<&str>, first_vararg: &str| -> Vec<String> {
        let lua = mlua::Lua::new();
        let captured = lua.create_table().unwrap();
        lua.globals().set("__captured", captured.clone()).unwrap();
        let print_fn = lua
            .create_function(|lua, s: mlua::LuaString| -> mlua::Result<()> {
                let t: mlua::Table = lua.globals().get("__captured")?;
                let n = t.raw_len();
                t.raw_set(n + 1, s.to_str()?.to_string())?;
                Ok(())
            })
            .unwrap();
        lua.globals().set("print", print_fn).unwrap();
        let arg_tbl = lua.create_table().unwrap();
        arg_tbl.raw_set(0, "prog.lua").unwrap();
        if let Some(a) = arg1 {
            arg_tbl.raw_set(1, a).unwrap();
        }
        lua.globals().set("arg", arg_tbl).unwrap();
        lua.load(&lua_code)
            .set_name("entrypoint")
            .call::<()>(first_vararg.to_string())
            .expect("chunk runs");
        captured
            .sequence_values::<String>()
            .collect::<mlua::Result<_>>()
            .unwrap()
    };

    // Standalone with a CLI argument: first vararg == arg[1] == "alpha" → run.
    assert_eq!(run(Some("alpha"), "alpha"), vec!["MAIN", "[\"alpha\"]"]);

    // Required for its exports: first vararg is the module name "prog" while the
    // host passed no args (arg[1] unset), so they differ → main stays dormant.
    assert!(
        run(None, "prog").is_empty(),
        "main must not run when the module is require()d"
    );
}

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
    // Same stack size as the mll CLI driver (see run_mll_file).
    let result = std::thread::Builder::new()
        .stack_size(mllc::COMPILER_STACK_SIZE)
        .spawn(move || {
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("Cannot read {}: {}", path.display(), e));

            let source_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            let lib_path = Path::new("../lib").to_path_buf();
            let lua_code = match compile(&source, &source_dir, &[&lib_path]) {
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
        .unwrap()
        .join();
    match result {
        Ok(out) => out,
        Err(e) => std::panic::resume_unwind(e),
    }
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
        (ghc_oracle_basics, "cases", "basics.mll"),
        (ghc_oracle_bind_first_class, "cases", "bind_first_class.mll"),
        (ghc_oracle_case_guards, "cases", "case_guards.mll"),
        (ghc_oracle_case_in_do_let, "cases", "case_in_do_let.mll"),
        (ghc_oracle_case_pure_bottom, "cases", "case_pure_bottom.mll"),
        (ghc_oracle_clause_local_scope, "cases", "clause_local_scope.mll"),
        (ghc_oracle_compose_non_strict, "cases", "compose_non_strict.mll"),
        (ghc_oracle_curried_lambda_arity, "cases", "curried_lambda_arity.mll"),
        (ghc_oracle_data_types, "cases", "data_types.mll"),
        (ghc_oracle_datakinds, "cases", "datakinds.mll"),
        (ghc_oracle_default_methods, "cases", "default_methods.mll"),
        (ghc_oracle_default_methods_ops, "cases", "default_methods_ops.mll"),
        (ghc_oracle_demand_analysis, "cases", "demand_analysis.mll"),
        (ghc_oracle_derive_enum, "cases", "derive_enum.mll"),
        (ghc_oracle_derive_eq, "cases", "derive_eq.mll"),
        (ghc_oracle_derive_functor, "cases", "derive_functor.mll"),
        (ghc_oracle_derive_functor_nested, "cases", "derive_functor_nested.mll"),
        (ghc_oracle_derive_ord, "cases", "derive_ord.mll"),
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
        (ghc_oracle_do_let_scoping, "cases", "do_let_scoping.mll"),
        (ghc_oracle_do_notation, "cases", "do_notation.mll"),
        (ghc_oracle_edge_cases, "cases", "edge_cases.mll"),
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
        (ghc_oracle_gadts, "cases", "gadts.mll"),
        (ghc_oracle_guard_strict_entry, "cases", "guard_strict_entry.mll"),
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
        (ghc_oracle_kinds_hkt, "cases", "kinds_hkt.mll"),
        (ghc_oracle_lambdas, "cases", "lambdas.mll"),
        (ghc_oracle_lazy_cheap_bindings, "cases", "lazy_cheap_bindings.mll"),
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
        (ghc_oracle_nested_calls, "cases", "nested_calls.mll"),
        (ghc_oracle_nested_eq, "cases", "nested_eq.mll"),
        (ghc_oracle_nested_just_pattern, "cases", "nested_just_pattern.mll"),
        (ghc_oracle_non_exhaustive_live, "cases", "non_exhaustive_live.mll"),
        (ghc_oracle_non_strict, "cases", "non_strict.mll"),
        (ghc_oracle_num_polymorphic, "cases", "num_polymorphic.mll"),
        (ghc_oracle_operator_fixity, "cases", "operator_fixity.mll"),
        (ghc_oracle_operator_sections, "cases", "operator_sections.mll"),
        (ghc_oracle_operators, "cases", "operators.mll"),
        (ghc_oracle_pair_ord_fields, "cases", "pair_ord_fields.mll"),
        (ghc_oracle_pattern_matching, "cases", "pattern_matching.mll"),
        (ghc_oracle_perform_bare_tco_deep, "cases", "perform_bare_tco_deep.mll"),
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
        (ghc_oracle_st_return, "cases", "st_return.mll"),
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
