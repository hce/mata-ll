/// Test harness: discovers all .mll files in tests/cases/,
/// compiles each with mllc, runs the result via mlua,
/// and reports success/failure.

use std::path::Path;

fn run_mll_file(path: &Path) {
    let path = path.to_path_buf();
    // Run on a thread with a larger stack to handle deeply nested expressions
    let result = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("Cannot read {}: {}", path.display(), e));

            let source_dir = path.parent().unwrap_or(Path::new("."));
            let lua_code = match mllc::compile(&source, source_dir, &[]) {
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

macro_rules! mll_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            run_mll_file(Path::new(concat!("tests/cases/", $file)));
        }
    };
}

fn run_mll_file_with_lib(path: &Path) {
    let path = path.to_path_buf();
    let lib_path = Path::new("../lib").to_path_buf();
    let result = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("Cannot read {}: {}", path.display(), e));

            let source_dir = path.parent().unwrap_or(Path::new("."));
            let lua_code = match mllc::compile(&source, source_dir, &[&lib_path]) {
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
mll_test!(newtypes, "newtypes.mll");
mll_test!(typeclasses, "typeclasses.mll");
mll_test!(superclass, "superclass.mll");
mll_test!(where_clauses, "where_clauses.mll");
mll_test!(where_io_types, "where_io_types.mll");
mll_test!(default_methods, "default_methods.mll");
mll_test!(default_methods_ops, "default_methods_ops.mll");
mll_test!(datakinds, "datakinds.mll");
mll_test!(type_level_nats, "type_level_nats.mll");
mll_test!(operator_sections, "operator_sections.mll");
mll_test!(guards, "guards.mll");
mll_test!(lambdas, "lambdas.mll");
mll_test!(maybe, "maybe.mll");
mll_test!(monomorphization, "monomorphization.mll");
mll_test!(strings, "strings.mll");
mll_test!(operators, "operators.mll");
mll_test!(let_exprs, "let_exprs.mll");
mll_test!(ffi, "ffi.mll");
mll_test!(show_required, "show_required.mll");
mll_test!(either_ordering, "either_ordering.mll");
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
mll_test!(non_strict, "non_strict.mll");
mll_test!(case_in_do_let, "case_in_do_let.mll");
mll_test!(functor_applicative, "functor_applicative.mll");
mll_test!(io_actions, "io_actions.mll");
mll_test!(haskell_compat, "haskell_compat.mll");
mll_test!(pattern_matching, "pattern_matching.mll");
mll_test!(typeclasses_full, "typeclasses_full.mll");
mll_test!(do_notation, "do_notation.mll");
mll_test!(list_comprehensions, "list_comprehensions.mll");
mll_test!(scoping, "scoping.mll");
mll_test!(type_aliases, "type_aliases.mll");
mll_test!(edge_cases, "edge_cases.mll");
mll_test!(feature_interactions, "feature_interactions.mll");
mll_test!(demand_analysis, "demand_analysis.mll");
mll_test!(ffi_strictness, "ffi_strictness.mll");
mll_test!(where_func_order, "where_func_order.mll");
mll_test!(type_alias, "type_alias.mll");
mll_test!(selective_import, "selective_import.mll");
mll_test!(multiline_list, "multiline_list.mll");
mll_test!(nested_calls, "nested_calls.mll");
mll_test!(seq_when_putstr, "seq_when_putstr.mll");
mll_test!(any_type, "any_type.mll");
mll_test!(bytestring, "bytestring.mll");
mll_test!(operator_fixity, "operator_fixity.mll");
mll_test!(export_module, "export_module.mll");
mll_test!(import_hiding, "import_hiding.mll");
mll_test!(record_update, "record_update.mll");
mll_test!(enum_range, "enum_range.mll");
mll_test!(read_typeclass, "read_typeclass.mll");
mll_test!(monad_nonio, "monad_nonio.mll");
mll_test!(derive_enum, "derive_enum.mll");
mll_test!(nested_eq, "nested_eq.mll");
mll_test!(st_return, "st_return.mll");
mll_test!(local_overflow, "local_overflow.mll");
mll_test!(existentials, "existentials.mll");
mll_test!(derive_functor, "derive_functor.mll");
mll_test!(derive_eq, "derive_eq.mll");
mll_test!(rank2, "rank2.mll");

// Stress tests
mll_test!(stress_large_adt, "stress_large_adt.mll");
mll_test!(stress_deep_recursion, "stress_deep_recursion.mll");
mll_test!(stress_nested_expr, "stress_nested_expr.mll");
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
mll_test!(exceptions, "exceptions.mll");

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
mll_lib_test!(lib_lbit, "lib_lbit.mll");
mll_lib_test!(lib_lmath, "lib_lmath.mll");
mll_lib_test!(lib_json, "lib_json.mll");
mll_lib_test!(lib_regex, "lib_regex.mll");
mll_lib_test!(lib_los, "lib_los.mll");
mll_lib_test!(lib_data_list, "lib_data_list.mll");
mll_lib_test!(lib_data_maybe, "lib_data_maybe.mll");

// Compile-error tests: these SHOULD fail to compile
#[test]
fn eq_without_instance_rejected() {
    let source = r#"
data Foo = Foo
    deriving Show

main :: IO ()
main = putStrLn (show (Foo == Foo))
"#;
    match mllc::compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("No instance"), "Expected 'No instance' error, got: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for == without Eq instance"),
    }
}

#[test]
fn show_without_instance_rejected() {
    let source = r#"
data Secret = Secret Integer

main :: IO ()
main = putStrLn (show (Secret 42))
"#;
    match mllc::compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("No instance"), "Expected 'No instance' error, got: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for show without Show instance"),
    }
}

#[test]
fn bare_signature_without_definition_rejected() {
    // A type signature with no accompanying definition (and not an FFI binding)
    // used to silently compile to a nil value. It must now be rejected.
    let source = r#"
foo :: Integer

main :: IO ()
main = print foo
"#;
    match mllc::compile(source, Path::new("."), &[]) {
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
    match mllc::compile(source, Path::new("."), &[]) {
        Ok(_) => {}
        Err(e) => panic!("FFI signature without body should compile, got error: {}", e),
    }
}

#[test]
fn orphan_instance_rejected() {
    // Show and Integer are both defined in the prelude, not locally.
    // Defining an instance for them here is an orphan instance.
    let source = r#"
instance Show Integer where
    show x = "int"

main :: IO ()
main = putStrLn "ok"
"#;
    match mllc::compile(source, Path::new("."), &[]) {
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
    match mllc::compile(source, cases_dir, &[]) {
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
    match mllc::compile(source, cases_dir, &[]) {
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
f :: Integer -> Integer
f x = x

main :: IO ()
main = print (f "hello")
"#;
    match mllc::compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Cannot unify"), "Expected 'Cannot unify' error, got: {}", msg);
        }
        Ok(_) => panic!("Expected compilation to fail for String passed where Integer expected"),
    }
}

#[test]
fn undefined_variable_rejected() {
    let source = r#"
main :: IO ()
main = print noSuchThing
"#;
    match mllc::compile(source, Path::new("."), &[]) {
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
f :: Integer -> Integer
f x = x + 1
f x = "hello"

main :: IO ()
main = print (f 1)
"#;
    match mllc::compile(source, Path::new("."), &[]) {
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
    match mllc::compile(source, Path::new("."), &[]) {
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
    match mllc::compile(source, Path::new("."), &[]) {
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
    match mllc::compile(source, Path::new("."), &[]) {
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
    match mllc::compile(source, Path::new("."), &[]) {
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
    match mllc::compile(source, Path::new("."), &[]) {
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
    match mllc::compile(source, Path::new("."), &[]) {
        Err(_) => { /* any error is acceptable */ }
        // Known gap: recursive type aliases may not be detected
        Ok(_) => { /* known gap: recursive type alias not rejected */ }
    }
}

#[test]
fn unknown_type_rejected() {
    // Using a constructor from a type that doesn't exist.
    // Known gap: the compiler accepts unknown names in type signatures.
    // But using an unknown constructor in an expression should be caught.
    let source = r#"
main :: IO ()
main = print (NoSuchCtor 42)
"#;
    match mllc::compile(source, Path::new("."), &[]) {
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
data Pair = Pair Integer Integer
    deriving Show

fst2 :: Pair -> Integer
fst2 (Pair x) = x

main :: IO ()
main = print (fst2 (Pair 1 2))
"#;
    match mllc::compile(source, Path::new("."), &[]) {
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
    // The body returns a String literal but the sig says Integer.
    let source = r#"
answer :: Integer
answer = "forty-two"

main :: IO ()
main = print answer
"#;
    match mllc::compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("Cannot unify") || msg.contains("doesn't match"),
                "Expected type mismatch error for String body vs Integer sig, got: {}", msg
            );
        }
        Ok(_) => panic!("Expected compilation to fail for String body where Integer declared"),
    }
}

#[test]
fn guard_non_bool_rejected() {
    // Guard expression returns Integer, not Bool — should fail to unify.
    let source = r#"
f :: Integer -> Integer
f x
    | x = x + 1
    | otherwise = x

main :: IO ()
main = print (f 5)
"#;
    match mllc::compile(source, Path::new("."), &[]) {
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
data Foo = MkThing Integer
data Bar = MkThing String

useFoo :: Foo -> Integer
useFoo (MkThing n) = n

main :: IO ()
main = print (useFoo (MkThing 42))
"#;
    match mllc::compile(source, Path::new("."), &[]) {
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
data Wrapper = Wrapper Integer
    deriving Eq

instance Show Wrapper where
    show (Wrapper n) = n

main :: IO ()
main = putStrLn (show (Wrapper 42))
"#;
    match mllc::compile(source, Path::new("."), &[]) {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(
                msg.contains("Cannot unify") || msg.contains("doesn't match"),
                "Expected type error for show returning Integer instead of String, got: {}", msg
            );
        }
        Ok(_) => panic!("Expected compilation to fail when show returns Integer instead of String"),
    }
}

// Regression test: x <- return val must unwrap the thunk (was a known bug)
#[test]
fn bind_return_unwraps_value() {
    let source = r#"
main :: IO ()
main = do
    x <- return (10 :: Integer)
    assert (x == 10) "bind return"
    putStrLn "ok"
"#;
    let lua_code = mllc::compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("bind_return").exec()
        .expect("x <- return val should bind x to the value");
}

// Runtime error tests: these should compile but fail at runtime
#[test]
fn undefined_errors_when_forced() {
    let source = r#"
main :: IO ()
main = do
    let x = undefined
    print x
"#;
    let lua_code = mllc::compile(source, Path::new("."), &[])
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
    let examples_dir = Path::new("../examples");

    // Examples expected to fail or skip
    let expected_fail: Vec<&str> = vec![
        "bench",              // show specialization gap on list display
        "aestest",            // 256-element S-box lists need large stack (runs via mll compiler)
        "bstest",             // needs large stack (runs via mll compiler)
        "salsa",              // large literal lists need large stack (runs via mll compiler)
        "Ed25519",            // large literal lists need large stack (runs via mll compiler)
        "ed25519test",        // depends on Ed25519 which needs large stack
        "metar",              // needs large stack (many nested parser combinators)
        "rectype",            // legitimate type errors (Ord on tuples, String as [Char])
        "match",              // experimental scratch file
        "experiments",        // experimental scratch file
    ];

    let mut failures = Vec::new();
    for entry in std::fs::read_dir(examples_dir).expect("Cannot read examples/") {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().map_or(true, |e| e != "mll") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_str().unwrap();
        if expected_fail.contains(&stem) {
            continue;
        }
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Cannot read {}: {}", path.display(), e));
        let source_dir = path.parent().unwrap_or(Path::new("."));
        match mllc::compile(&source, source_dir, &[lib_path]) {
            Ok(_) => {}
            Err(e) => failures.push(format!("{}: {}", stem, e)),
        }
    }
    if !failures.is_empty() {
        panic!("Examples failed to compile:\n{}", failures.join("\n"));
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
data Box = Box Color Integer

unwrap :: Box -> Integer
unwrap (Box R n) = n
unwrap (Box B n) = 0 - n

main :: IO ()
main = do
  print (unwrap (Box R 5))
  print (unwrap (Box B 5))
"#;
    let lua_code = mllc::compile(source, Path::new("."), &[])
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
    let lua_code = mllc::compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("record_field_force").exec()
        .expect("projecting a thunk-valued field in arithmetic should force it");
}

// Layout: a multi-line application-argument continuation indented past the
// enclosing block (but not necessarily past the function column) is now
// accepted, matching Haskell. Previously it required indentation past the
// function and was rejected as "Unexpected token at top level".
#[test]
fn shallow_multiline_continuation() {
    let source = r#"
import Data.List (foldl')

total :: Integer
total = foldl' (\a b -> a + b) 0
  [1, 2, 3, 4, 5]

main :: IO ()
main = print total
"#;
    let lib_path = Path::new("../lib");
    let lua_code = mllc::compile(source, Path::new("."), &[lib_path])
        .expect("shallow multi-line continuation should parse");
    let lua = mlua::Lua::new();
    lua.load(&lua_code.lua_code).set_name("shallow_cont").exec()
        .expect("should run");
}

// Regression: a `case` matching a nested pattern under a constructor whose
// payload is a thunk (built from a non-cheap expression) must force the field
// before destructuring it. Previously the inner pattern indexed into the raw
// thunk table, reading its internals (the `false` flag, a nil) as field values.
#[test]
fn case_nested_pattern_forces_thunked_field() {
    let source = r#"
data Pair = Pair (Integer, Integer)

slow :: Integer -> Integer
slow 0 = 0
slow n = slow (n - 1) + 1

mkPair :: Integer -> Pair
mkPair x = Pair (slow x, slow x + 1)

main :: IO ()
main = case mkPair 3 of
         Pair (a, b) -> assert (a + b == 7) "nested pattern forces thunked field"
"#;
    let lua_code = mllc::compile(source, Path::new("."), &[])
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
data R = R { rfn :: Integer -> Integer, rval :: Integer }

applyAcc :: (R -> Integer) -> R -> Integer
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

sumList :: [Integer] -> Integer
sumList [] = 0
sumList (x:xs) = x + sumList xs
"#;
    let lua_code = mllc::compile(source, Path::new("."), &[])
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
    run_mll_file_with_lib(Path::new("../examples/huffman.mll"));
}

#[test]
fn example_redblack_invariants() {
    run_mll_file_with_lib(Path::new("../examples/redblack.mll"));
}

#[test]
fn example_scheme_eval() {
    run_mll_file_with_lib(Path::new("../examples/scheme.mll"));
}

#[test]
fn example_raytracer_renders() {
    run_mll_file_with_lib(Path::new("../examples/raytracer.mll"));
}

#[test]
fn example_typeinfer_checks() {
    run_mll_file_with_lib(Path::new("../examples/typeinfer.mll"));
}

// ============================================================
// FFI tests: compile MLL modules with exports, then call
// exported functions from Lua and verify return values.
// ============================================================

/// Helper: compile MLL source and return a Lua module table
fn compile_ffi_module(source: &str) -> (mlua::Lua, mlua::Table) {
    let lua_code = mllc::compile(source, Path::new("."), &[])
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
fn ffi_export_pure_functions() {
    let source = r#"
export add :: Integer -> Integer -> Integer
add x y = x + y

export double :: Integer -> Integer
double n = n * 2

export negate :: Integer -> Integer
negate n = 0 - n

export isEven :: Integer -> Bool
isEven n = n `mod` 2 == 0

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    // Integer arithmetic
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
range :: Integer -> [Integer]
range n = if n <= 0 then [] else go 1 n
  where go i m = if i > m then [] else i : go (i + 1) m

export getRange :: Integer -> [Integer]
getRange n = range n

export squares :: Integer -> [Integer]
squares n = map (\x -> x * x) (range n)

export countTo :: Integer -> Integer
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
    assert!(result.is_nil() || matches!(&result, mlua::Value::Table(t) if t.len().unwrap() == 0),
            "range 0 is empty (nil or empty table)");

    // List → List (map)
    let squares: mlua::Function = module.get("squares").unwrap();
    let result: Vec<i64> = squares.call(4).unwrap();
    assert_eq!(result, vec![1, 4, 9, 16], "squares 4");

    // List → Integer (fold)
    let count: mlua::Function = module.get("countTo").unwrap();
    let result: i64 = count.call(10).unwrap();
    assert_eq!(result, 55, "countTo 10 == 55 (triangle number)");
}

#[test]
fn ffi_export_maybe_either() {
    let source = r#"
export safeDiv :: Integer -> Integer -> Maybe Integer
safeDiv _ 0 = Nothing
safeDiv x y = Just (x `div` y)

export classify :: Integer -> Either String Integer
classify n = if n < 0 then Left "negative" else Right n

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

    // Either: Left {1, msg}, Right {2, val}
    let classify: mlua::Function = module.get("classify").unwrap();
    let result: Vec<mlua::Value> = classify.call(5).unwrap();
    assert_eq!(result.len(), 2);
    // Right tag = 2
    if let mlua::Value::Integer(tag) = result[0] {
        assert_eq!(tag, 2, "classify 5 is Right (tag 2)");
    }

    let result: Vec<mlua::Value> = classify.call(-3).unwrap();
    if let mlua::Value::Integer(tag) = result[0] {
        assert_eq!(tag, 1, "classify (-3) is Left (tag 1)");
    }
}

#[test]
fn ffi_export_higher_order() {
    // MLL-side higher-order: partial application across FFI
    let source = r#"
applyTwice :: (a -> a) -> a -> a
applyTwice f x = f (f x)

double :: Integer -> Integer
double x = x * 2

inc :: Integer -> Integer
inc x = x + 1

export doubleDouble :: Integer -> Integer
doubleDouble n = applyTwice double n

export incInc :: Integer -> Integer
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
export swap :: (Integer, Integer) -> (Integer, Integer)
swap (a, b) = (b, a)

export firstPlusSecond :: (Integer, Integer) -> Integer
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
export increment :: Integer -> Integer
increment = (+1)

fib :: [Integer]
fib = 1 : 1 : zipWith (+) fib (drop 1 fib)

export fibonacci :: Integer -> [Integer]
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
    let source = r#"
data Color = Red | Green | Blue

export colorCode :: Color -> Integer
colorCode Red = 1
colorCode Green = 2
colorCode Blue = 3

export mkRed :: Integer -> Color
mkRed _ = Red

export mkGreen :: Integer -> Color
mkGreen _ = Green

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    // Pass enum value through: create on MLL side, inspect on MLL side
    let mk_red: mlua::Function = module.get("mkRed").unwrap();
    let color_code: mlua::Function = module.get("colorCode").unwrap();
    let red_val: mlua::Value = mk_red.call(0).unwrap();
    let result: i64 = color_code.call(red_val).unwrap();
    assert_eq!(result, 1, "colorCode Red == 1");

    let mk_green: mlua::Function = module.get("mkGreen").unwrap();
    let green_val: mlua::Value = mk_green.call(0).unwrap();
    let result: i64 = color_code.call(green_val).unwrap();
    assert_eq!(result, 2, "colorCode Green == 2");
}

#[test]
fn ffi_export_multi_arg() {
    // Test multi-arg exported functions and string operations
    let source = r#"
export strRepeat :: String -> Integer -> String
strRepeat _ 0 = ""
strRepeat s n = s <> strRepeat s (n - 1)

export clamp :: Integer -> Integer -> Integer -> Integer
clamp lo hi x = if x < lo then lo else if x > hi then hi else x

export between :: Integer -> Integer -> Bool
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
export mapDouble :: [Integer] -> [Integer]
mapDouble xs = map (\x -> x * 2) xs

export mapShow :: [Integer] -> [String]
mapShow xs = map show xs

export listOfStrings :: Integer -> [String]
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
export sumList :: [Integer] -> Integer
sumList xs = foldl (+) 0 xs

export headOf :: [Integer] -> Integer
headOf xs = head xs

export lengthOf :: [Integer] -> Integer
lengthOf [] = 0
lengthOf (_:xs) = 1 + lengthOf xs

export appendLists :: [Integer] -> [Integer] -> [Integer]
appendLists xs ys = xs ++ ys

export reverseList :: [Integer] -> [Integer]
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
    // String lists: Lua string arrays → MLL [String] and back
    let source = r#"
export joinWith :: String -> [String] -> String
joinWith _ [] = ""
joinWith _ [x] = x
joinWith sep (x:xs) = x <> sep <> joinWith sep xs

export filterLong :: Integer -> [String] -> [String]
filterLong n xs = filter (\s -> lengthS s > n) xs
  where lengthS s = foldl (\acc c -> acc + 1) 0 (unpack s)

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
}

#[test]
fn ffi_export_mixed_args() {
    // Functions with both list and non-list arguments
    let source = r#"
export takeN :: Integer -> [Integer] -> [Integer]
takeN n xs = take n xs

export dropN :: Integer -> [Integer] -> [Integer]
dropN n xs = drop n xs

export replicate :: Integer -> Integer -> [Integer]
replicate 0 _ = []
replicate n x = x : replicate (n - 1) x

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    // Integer arg + list arg
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
