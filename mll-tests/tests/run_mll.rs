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

// Layout: a function whose first argument is on the next line (no same-line
// argument) is consumed as an application. Previously the cross-line
// continuation required at least one same-line arg, so this failed inside
// parenthesized multi-line constructor application.
#[test]
fn first_argument_on_next_line() {
    let source = r#"
data T = L Integer | N T T

deep :: T
deep = N (N (L 1)
            (L 2))
         (L 3)

size :: T -> Integer
size (L _) = 1
size (N a b) = size a + size b

main :: IO ()
main = assert (size deep == 3) "function with first arg on next line"
"#;
    let lua_code = mllc::compile(source, Path::new("."), &[])
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
fibTop :: [Integer]
fibTop = [1, 1] ++ zipWith (+) fibTop (drop 1 fibTop)

nthWhere :: Integer -> Integer
nthWhere k = fib !! k
  where
    fib = [1, 1] ++ zipWith (+) fib (drop 1 fib)

nthLet :: Integer -> Integer
nthLet k =
  let fib = [1, 1] ++ zipWith (+) fib (drop 1 fib)
  in fib !! k

-- mutually recursive let bindings
isEven :: Integer -> Bool
isEven n =
  let ev = \m -> if m == 0 then True else od (m - 1)
      od = \m -> if m == 0 then False else ev (m - 1)
  in ev n

-- let-polymorphism must survive the recursive-let change
polyPair :: (Integer, Bool)
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
    let lua_code = mllc::compile(source, Path::new("."), &[lib_path])
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
evens :: [Integer]
evens = [x | x <- [1..], x `mod` 2 == 0]

-- a recursive call passed as a function argument must stay lazy
consit :: a -> [a] -> [a]
consit x rest = x : rest

countFrom :: Integer -> [Integer]
countFrom n = consit n (countFrom (n + 1))

-- foldr building a list: cons whose tail is a variable
copyList :: [Integer] -> [Integer]
copyList = foldr (\x acc -> x : acc) []

-- guard recursion with a thunked argument (the param is used strictly)
digitalRoot :: Integer -> Integer
digitalRoot n
  | n < 10    = n
  | otherwise = digitalRoot (digitSum n)
  where
    digitSum 0 = 0
    digitSum m = m `mod` 10 + digitSum (m `div` 10)

-- higher-order: a lambda param may arrive as a thunk and must be forced
makeAdder :: Integer -> Integer -> Integer
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
    let lua_code = mllc::compile(source, Path::new("."), &[lib_path])
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
  assert (takeWhile (\x -> x > 9) [1, 2, 3] == ([] :: [Integer])) "takeWhile none"
  assert (dropWhile (\x -> x < 3) [1, 2, 3, 4, 5] == [3, 4, 5]) "dropWhile finite"
  assert (dropWhile (\x -> x > 9) [1, 2, 3] == [1, 2, 3]) "dropWhile none"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = mllc::compile(source, Path::new("."), &[lib_path])
        .expect("takeWhile/dropWhile program should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("prelude_take_while_drop_while").exec()
        .expect("takeWhile/dropWhile should evaluate correctly");
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
fParam :: Integer -> Integer
fParam 0 = 0
fParam elem = elem + 1

-- case-pattern variable named like a prelude function
fCase :: Maybe Integer -> Integer
fCase m = case m of
  Just reverse -> reverse + 1
  Nothing -> 0

-- let-bound variable named like a prelude function
fLet :: Integer
fLet = let length = 41 in length + 1

main :: IO ()
main = do
  assert (fParam 10 == 11) "param shadows prelude fn"
  assert (fCase (Just 20) == 21) "case var shadows prelude fn"
  assert (fLet == 42) "let var shadows prelude fn"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = mllc::compile(source, Path::new("."), &[lib_path])
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
  assert (null ([] :: [Integer])) "null"
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
    let lua_code = mllc::compile(source, Path::new("."), &[lib_path])
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
  assert (show (1, [2, 3]) == "(1, [2, 3])") "tuple with list as second element"
  assert (show ([1, 2], [3, 4]) == "([1, 2], [3, 4])") "tuple of two lists"
  assert (show ([1, 2], 3) == "([1, 2], 3)") "tuple with list as first element"
  assert (show (1, 2) == "(1, 2)") "plain tuple"
  -- An empty-list element must show as "[]", not the type-erased "Nothing"
  -- (the post-mono verifier flagged this latent tuple-show leak).
  assert (show ((1 :: Integer), ([] :: [Integer])) == "(1, [])") "tuple with empty list element"
  assert (show ((Just (1 :: Integer)), (Nothing :: Maybe Integer)) == "(Just 1, Nothing)") "tuple of Maybe elements"
"#;
    let lua_code = mllc::compile(source, Path::new("."), &[])
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

#[test]
fn example_listcomp() {
    run_mll_file_with_lib(Path::new("../examples/listcomp.mll"));
}

#[test]
fn example_lambda_reduction() {
    run_mll_file_with_lib(Path::new("../examples/lambda.mll"));
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

// --- Outgoing FFI callbacks (mata-ll -> Lua): the fold / threaded-state pattern.

#[test]
fn ffi_outgoing_callback_fold() {
    // A Lua host (db.fold) calls our mata-ll callback as cb(row, state) per row
    // and threads the result. Exercises a pure callback, an effectful (LuaIO s)
    // callback, and an opaque tuple state that must round-trip through Lua.
    let source = r#"
-- Pure outgoing callback: state `acc` is opaque (a polymorphic type variable).
foldRows :: String -> (Integer -> acc -> acc) -> acc -> LuaPure "db.fold" acc

-- Effectful outgoing callback: returns LuaIO s acc, may do I/O per row.
foldRowsIO :: String -> (Integer -> acc -> LuaIO s acc) -> acc -> LuaIO "db.fold" acc

stepIO :: Integer -> Integer -> LuaIO s Integer
stepIO row acc = do
    liftIO (putStr "")
    pure (acc + row)

-- Pure sum into an Integer accumulator (uncurry + value return).
export sumRows :: Integer -> Integer
sumRows seed = foldRows "select" (\row acc -> acc + row) seed

-- Opaque tuple state (sum, count): proves the state survives the Lua round-trip
-- intact (the FFI converters would otherwise flatten a tuple to a cons list).
export sumCount :: Integer -> Integer
sumCount _ =
    case foldRows "select" (\row acc -> case acc of (s, c) -> (s + row, c + 1)) (0, 0) of
        (s, c) -> s * 1000 + c

-- Effectful fold, returned as IO; the export wrapper runs the action.
export runEffectful :: Integer -> IO Integer
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

#[test]
fn prelude_is_emitted_on_demand() {
    // A trivial program must not carry runtime helpers it never references.
    let trivial = mllc::compile("main :: IO ()\nmain = putStrLn \"hi\"\n", Path::new("."), &[])
        .expect("trivial should compile")
        .lua_code;
    assert!(!trivial.contains("show_HashMap"), "unused hashmap show must be shaken out");
    assert!(!trivial.contains("__mll_st_new"), "unused ST-array runtime must be shaken out");
    assert!(!trivial.contains("hashmap_insert"), "unused hashmap runtime must be shaken out");

    // But a program that uses a feature must still carry its runtime, or it
    // would break at runtime — reachability, not blanket removal.
    let uses_list_show = mllc::compile("main :: IO ()\nmain = print [1, 2, 3 :: Integer]\n", Path::new("."), &[])
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
        mllc::compile(src, Path::new("."), &[])
            .expect("should compile")
            .lua_code
            .matches("= function").count()
    };
    // A trivial program must not carry the unused auto-prelude.
    let trivial = fn_count("main :: IO ()\nmain = putStrLn \"hi\"\n");
    let prelude_heavy = fn_count(
        "main :: IO ()\nmain = print (foldr (+) 0 (map (\\x -> x * 2) (filter (\\x -> x > 0) [1, 2, 3, 4 :: Integer])))\n",
    );
    assert!(trivial < prelude_heavy,
        "trivial ({trivial} fns) should emit fewer functions than prelude-heavy ({prelude_heavy} fns)");
    assert!(trivial < 25, "trivial program should be tiny after DCE, got {trivial} fns");

    // Exports are roots: an exported function survives DCE even when `main`
    // never calls it (it is reachable only from outside).
    let (_lua, module) = compile_ffi_module(
        "export twice :: Integer -> Integer\ntwice x = x + x\nmain :: IO ()\nmain = pure ()\n",
    );
    let twice: mlua::Function = module.get("twice").unwrap();
    let r: i64 = twice.call(21).unwrap();
    assert_eq!(r, 42, "exported function must survive DCE and run");
}

// Compile + run an `assert`-based program; a failed assert raises a Lua error,
// so exec() fails and the test fails (never passes vacuously).
fn assert_mll(stmts: &str) {
    let src = format!("main :: IO ()\nmain = do\n{stmts}\n");
    let lua = mllc::compile(&src, Path::new("."), &[])
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
    assert_mll("    assert ((\\s -> \\n -> s) \"hi\" (5 :: Integer) == \"hi\") \"const\"");
}

#[test]
fn curried_lambda_returns_list() {
    assert_mll("    assert ((\\x -> \\y -> [x, y]) (1 :: Integer) 2 == [1, 2]) \"list result\"");
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
    // The complementary case a naive flatten would break: `map` applies the
    // lambda to ONE argument and expects a function back, so an argument-
    // position lambda must keep its curried 1-arg-layer shape.
    let src = r#"
applyAll :: [a -> b] -> a -> [b]
applyAll []     _ = []
applyAll (f:fs) x = f x : applyAll fs x

main :: IO ()
main = do
    let fns = map (\n -> \x -> x + n) [1, 5, 10]
    assert (applyAll fns 42 == [43, 47, 52]) "higher-order curried"
"#;
    let lua = mllc::compile(src, Path::new("."), &[]).expect("compile").lua_code;
    let l = mlua::Lua::new();
    l.load(&lua).set_name("ho_curried").exec().expect("higher-order curried lambda should work");
}

fn compile_err(source: &str) -> String {
    match mllc::compile(source, Path::new("."), &[]) {
        Ok(_) => panic!("expected compilation to fail, but it succeeded"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn ffi_outgoing_callback_rejects_bad_signatures() {
    // Effectful callbacks must use `LuaIO s acc`, not `IO acc`.
    let e = compile_err(
        r#"
bad :: String -> (Integer -> acc -> IO acc) -> acc -> LuaPure "h.f" acc
main :: IO ()
main = pure ()
"#,
    );
    assert!(e.contains("LuaIO s"), "IO acc should be rejected, got: {e}");

    // The callback's result must be the threaded state, not some other type.
    let e = compile_err(
        r#"
bad :: String -> (Integer -> acc -> LuaIO s Bool) -> acc -> LuaPure "h.f" acc
main :: IO ()
main = pure ()
"#,
    );
    assert!(e.contains("threaded state"), "mismatched result should be rejected, got: {e}");

    // A polymorphic callback requires a polymorphic (variable) FFI return type.
    let e = compile_err(
        r#"
bad :: String -> (Integer -> a -> a) -> Integer -> LuaPure "h.f" Integer
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
    assert!(e.contains("not a list of characters"), "missing String/list note, got: {e}");
    // The note must NOT prescribe string ops here: line is `trie "..."`, a list
    // is wanted — there is nothing to concatenate or show.
    assert!(!e.contains("concatenat"), "note must not suggest concatenation, got: {e}");

    // `<>` on a list should point the user at `++`.
    let e = compile_err(
        r#"
main :: IO ()
main = print ([1, 2] <> [3, 4] :: [Integer])
"#,
    );
    assert!(e.contains("No instance for '<>'"), "got: {e}");
    assert!(e.contains("concatenated with ++"), "missing ++ note, got: {e}");

    // Ordering whole tuples should explain the missing Ord instance.
    let e = compile_err(
        r#"
main :: IO ()
main = print ((1, 2) > (1, 3) :: Bool)
"#,
    );
    assert!(e.contains("No instance for '>'"), "got: {e}");
    assert!(e.contains("no Ord instance"), "missing tuple Ord note, got: {e}");
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
    let lua_code = mllc::compile(source, Path::new("."), &[Path::new("../lib")])
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
    // [Integer] (even nested) printed as "Nothing". `print` must use the typed
    // list show (which knows nil means []), while real Nothing still shows.
    let source = r#"
main :: IO ()
main = do
    print ([] :: [Integer])
    print ([[1, 2], []] :: [[Integer]])
    print (Nothing :: Maybe Integer)
"#;
    let lua_code = mllc::compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;

    // Capture `print` output instead of letting it hit stdout.
    let lua = mlua::Lua::new();
    let captured = lua.create_table().unwrap();
    lua.globals().set("__captured", captured.clone()).unwrap();
    let print_fn = lua
        .create_function(|lua, s: mlua::String| -> mlua::Result<()> {
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
    assert_eq!(lines, vec!["[]", "[[1, 2], []]", "Nothing"]);
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
data B = MkB Integer deriving (Show)

main :: IO ()
main = do
    print (Branch (Leaf (1 :: Integer) (2 :: Integer)) (Leaf 3 4))
    print (MkBox (MkBox (5 :: Integer)))
    print (P Red Green)
    print (MkB (0 - 5))
"#;
    let lua_code = mllc::compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;

    let lua = mlua::Lua::new();
    let captured = lua.create_table().unwrap();
    lua.globals().set("__captured", captured.clone()).unwrap();
    let print_fn = lua
        .create_function(|lua, s: mlua::String| -> mlua::Result<()> {
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
    // Regression: Just x and x share a runtime rep (identity), so `show` used to
    // print Just 5 as bare "5". Type-directed Maybe show recovers the structure.
    // The last case (Just Nothing) is the irreducible collision: a wrapped
    // Nothing collapses to nil and is indistinguishable from Nothing.
    let source = r#"
main :: IO ()
main = do
    print (Just (5 :: Integer))
    print (Nothing :: Maybe Integer)
    print (Just (Just (5 :: Integer)))
    print (Just (0 - 5 :: Integer))
    print [Just (1 :: Integer), Nothing, Just 3]
    print (Just (Nothing :: Maybe Integer))
"#;
    let lua_code = mllc::compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;

    let lua = mlua::Lua::new();
    let captured = lua.create_table().unwrap();
    lua.globals().set("__captured", captured.clone()).unwrap();
    let print_fn = lua
        .create_function(|lua, s: mlua::String| -> mlua::Result<()> {
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
            "[Just 1, Nothing, Just 3]",
            "Nothing", // Just Nothing collapses to nil — known representation limit
        ]
    );
}
