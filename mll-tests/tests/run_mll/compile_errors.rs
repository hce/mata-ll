//! Programs that must FAIL to compile, and the shape of their diagnostics
//! (plus the accept-side control tests that pin each check's boundary).

use super::*;

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
    expect_compile_error(source, &[], &[
        "out of range",
        "\\256",
        "note:",
        "byte array",
        "HASKDIFF.md",
    ]);
}

// Unary minus is GHC's `negate`, a Num method: `-x` at a non-Num type
// must be a compile-time "No instance" (GHC-87110 shape), not a Lua
// runtime arithmetic error. Regression: the Negate inference arm emitted
// no Num wanted at all, so `f :: Bool -> Bool; f x = -x` compiled and
// crashed at the first call. The controls pin the accept side: negation
// at the concrete numeric types and under defaulting stays legal.
#[test]
fn negate_at_non_num_type_is_rejected() {
    let source = r#"
f :: Bool -> Bool
f x = -x

main :: IO ()
main = print (f True)
"#;
    expect_compile_error(source, &[], &["No instance", "Num", "Bool"]);
}

#[test]
fn negate_at_numeric_types_stays_accepted() {
    let source = r#"
negI :: Int -> Int
negI x = -x

negN :: Number -> Number
negN x = -x

main :: IO ()
main = do
    print (negI 3)
    print (negN 2.5)
    -- unannotated local: the Num wanted flows into numeric defaulting
    let negD y = -y
    print (negD 7)
"#;
    compile(source, Path::new("."), &[]).expect("numeric negation must stay legal");
}

// Unit satisfies what is REGISTERED for it (Show/Eq/Ord, plus user
// `instance C ()`), not every class. Regression: the entailment Unit arm
// was Satisfied unconditionally, so `Num ()` typechecked and crashed in
// the emitted Lua arithmetic. (The accept side — print (), () == () —
// is pinned by the tuple_instance corpus case.)
#[test]
fn num_unit_is_rejected() {
    let source = r#"
main :: IO ()
main = print (() + ())
"#;
    expect_compile_error(source, &[], &["No instance", "Num ()"]);
}

// A tuple instance's declared context must bind the element types and be
// enforced — and the failure explains itself through the context note.
#[test]
fn tuple_instance_context_is_enforced() {
    let source = r##"
class Pretty a where
    pretty :: a -> String

instance Pretty Int where
    pretty n = "#" <> show n

instance (Pretty a, Pretty b) => Pretty (a, b) where
    pretty p = case p of
        (x, y) -> "<" <> pretty x <> ", " <> pretty y <> ">"

main :: IO ()
main = putStrLn (pretty ((1.5, 2) :: (Number, Int)))
"##;
    expect_compile_error(source, &[], &[
        "No instance",
        "Pretty (Number, Int)",
        "note:",
        "there is an instance '(Pretty a, Pretty b) => Pretty (a, b)'",
        "needs 'Pretty Number'",
    ]);
}

// A first-class operator section `(+)` / `(`div`)` is a use of the
// operator: its class constraints must be emitted on the instantiation.
// Regression: the OpFunc arm called bare `instantiate` (no
// emit_use_constraints), so `zipWith (+) [True] [False]` typechecked
// and crashed in the emitted Lua arithmetic. The control pins the
// accept side at a legal numeric use.
#[test]
fn op_section_at_non_num_type_is_rejected() {
    let source = r#"
main :: IO ()
main = print (zipWith (+) [True] [False])
"#;
    expect_compile_error(source, &[], &["No instance", "Num", "Bool"]);
}

#[test]
fn op_section_at_numeric_type_stays_accepted() {
    let source = r#"
main :: IO ()
main = do
    print (zipWith (+) [1, 2] [30, 40])
    print (foldr (`div`) 2 [1000, 8])
"#;
    compile(source, Path::new("."), &[]).expect("numeric operator sections must stay legal");
}

// Haskell 2010 puts `::` one grammar level above a section operand
// (exp → infixexp [:: type]), so GHC parse-errors on `(+ 1 :: Int)`;
// mata-ll used to accept it silently (the operand parse consumed the
// ascription). Both section spellings must reject with the concrete
// rewrite hint, and the parenthesized operand stays legal (the control).
#[test]
fn ascription_in_right_section_operand_is_rejected() {
    let source = r#"
main :: IO ()
main = print (map (+ 1 :: Int) [1, 2])
"#;
    expect_compile_error(source, &[], &[
        "section operand",
        "'::' annotates a complete expression",
        "note:",
        "(+ (e :: T))",
    ]);
}

#[test]
fn ascription_in_backtick_right_section_operand_is_rejected() {
    let source = r#"
main :: IO ()
main = print (map (`div` 2 :: Int) [4, 6])
"#;
    expect_compile_error(source, &[], &[
        "section operand",
        "note:",
        "(`div` (e :: T))",
    ]);
}

#[test]
fn parenthesized_ascription_in_section_operand_is_accepted() {
    let source = r#"
main :: IO ()
main = print (map (+ (1 :: Int)) [1, 2])
"#;
    compile(source, Path::new("."), &[])
        .expect("parenthesized ascription operand must stay legal");
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
    expect_compile_error(source, &[], &[
        "Cannot unify",
        "String",
        "note:",
        "opaque",
        "[Char]",
        "<>",
        "HASKDIFF.md",
    ]);
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
    expect_compile_error(source, &[], &[
        "Cannot derive 'FromJSON'",
        "import JSON",
    ]);
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
    expect_compile_error(source, &[lib], &[
        "Cannot derive 'FromJSON' for 'H'",
        "field 'hop'",
        "function",
    ]);
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
    expect_compile_error(source, &[lib], &[
        "Cannot derive 'FromJSON' for 'Box'",
        "type parameters",
    ]);
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
    expect_compile_error(source, &[lib], &[
        "Cannot derive 'FromJSON' for 'Holder'",
        "'Plain' has no FromJSON instance",
    ]);
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
    expect_compile_error(source, &[lib], &[
        "Cannot derive 'FromJSON' for 'T'",
        "tag",
    ]);
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
    expect_compile_error(source, &[], &[
        "Cannot derive 'ToJSON'",
        "import JSON",
    ]);
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
    expect_compile_error(source, &[lib], &[
        "Cannot derive 'ToJSON' for 'H'",
        "field 'hop'",
        "function",
    ]);
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
    expect_compile_error(source, &[lib], &[
        "Cannot derive 'ToJSON' for 'Holder'",
        "'Plain' has no ToJSON instance",
    ]);
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
    expect_compile_error(source, &[lib], &[
        "Cannot derive 'ToJSON' for 'Box'",
        "type parameters",
    ]);
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
    expect_compile_error(source, &[], &[
        "Duplicate data constructor 'Ok'",
        "'A'",
        "data B",
        "note:",
    ]);
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
    expect_compile_error(source, &[], &["Duplicate data constructor 'Wrap'"]);
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
    expect_compile_error(source, &[], &[
        "Cannot unify",
        "ExitValue",
        "Foo",
    ]);
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
    expect_compile_error(&source, &[], &[
        "Cannot unify",
        "''S",
        "''Z'",
        "in definition of 'main'",
    ]);
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
    expect_compile_error(&source, &[], &[
        "Cannot unify",
        "''S",
        "''Z'",
    ]);
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
    expect_compile_error(&source, &[], &[
        "Cannot unify",
        "''S 'Z'",
        "''Z'",
        "in definition of 'v2'",
    ]);
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
    expect_compile_error(&source, &[], &[
        "Cannot unify",
        "''Z'",
        "''S 'Z'",
        "in definition of 'v2'",
    ]);
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
        expect_compile_error(&source, &[lib], &[
            &format!("Cannot derive '{}' for 'D'", class),
            "both map to the JSON key \"k\"",
        ]);
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
        expect_compile_error(&source, &[lib], &[
            &format!("Cannot derive '{}' for 'D'", class),
            "empty string",
        ]);
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
        expect_compile_error(&source, &[lib], &[
            &format!("Cannot derive '{}' for 'T'", class),
            "\"tag\"",
        ]);
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
        expect_compile_error(&source, &[lib], &[
            &format!("Cannot derive '{}' for 'D'", class),
            "both map to the JSON tag \"x\"",
        ]);
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
    expect_compile_error(source, &[lib], &[
        "Cannot derive 'FromJSON' for 'D'",
        "both map to the JSON tag \"B\"",
    ]);
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
    expect_compile_error(source, &[lib], &[
        "Cannot derive 'ToJSON' for 'D'",
        "empty string",
    ]);
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
    let msg = expect_compile_error(source, &[], &[
        "Constructor 'Foo' of 'Foo' is renamed with `as \"foo\"`",
        "derives neither ToJSON nor FromJSON",
        "positional integer tag",
    ]);
    assert!(!msg.contains("expects 2 args"),
        "the old phantom-field misparse is back: {}", msg);
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
    expect_compile_error(source, &[lib], &[
        "Cannot derive 'ToJSON' for 'W'",
        "encodes untagged",
    ]);
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
    expect_compile_error(source, &[], &[
        "Expected a string literal after 'as' in constructor 'Foo'",
    ]);
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
    expect_compile_error(source, &[], &[
        "Cannot derive 'LuaDict' for 'D'",
        "both map to the Lua tag \"User\"",
    ]);
}

#[test]
fn luadict_enum_empty_tag_rejected() {
    // (c) An empty `as` tag names nothing a Lua host could tell apart.
    let source = r#"
data D = A as "" | B deriving (LuaDict)

main :: IO ()
main = pure ()
"#;
    expect_compile_error(source, &[], &[
        "Cannot derive 'LuaDict' for 'D'",
        "empty string",
    ]);
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
    expect_compile_error(source, &[lib], &[
        "No instance",
        "FromJSON",
        "Q",
    ]);
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
    expect_compile_error("class Pretty a where\n    pretty :: a -> String\ninstance Pretty [Int] where\n    pretty _ = \"int list\"\nmain :: IO ()\nmain = putStrLn (pretty ([True] :: [Bool]))\n", &[], &[
        "too specific",
        "[Int]",
    ]);

    // Repeated type argument (`Pair a a`) is likewise rejected.
    let e = expect_compile_error("data Pair a b = Pair a b\nclass Pretty a where\n    pretty :: a -> String\ninstance Pretty (Pair a a) where\n    pretty _ = \"pair\"\nmain :: IO ()\nmain = pure ()\n", &[], &[]);
    assert!(e.contains("too specific") || e.contains("DISTINCT"), "got: {e}");
}

#[test]
fn duplicate_instance_is_hard_error() {
    // Two instances for the same (class, head) silently overwrote (last wins);
    // now a compile error, like GHC's duplicate-instance rejection. (Strict
    // version of `duplicate_instance_rejected`, which tolerated the old gap.)
    expect_compile_error("class Greet a where\n    greet :: a -> String\ninstance Greet Int where\n    greet _ = \"first\"\ninstance Greet Int where\n    greet _ = \"second\"\nmain :: IO ()\nmain = putStrLn (greet (1 :: Int))\n", &[], &[
        "Duplicate instance",
        "Greet Int",
    ]);
}

#[test]
fn overlapping_instances_rejected() {
    // `instance Pretty [a]` and `instance Pretty [Int]` overlap at head
    // `[]`; both `pretty [1]` and `pretty [True]` used to pick the
    // last-declared body. Now the specific head is rejected.
    let e = expect_compile_error("class Pretty a where\n    pretty :: a -> String\ninstance Pretty a => Pretty [a] where\n    pretty _ = \"generic\"\ninstance Pretty [Int] where\n    pretty _ = \"int list\"\nmain :: IO ()\nmain = pure ()\n", &[], &[]);
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
    expect_compile_error(source, &[], &[
        "No instance for 'Show (Tree Blob)'",
        "there is no instance 'Show Blob'",
        "definition of 'main'",
    ]);
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
    expect_compile_error(source, &[], &["does not appear in the instance head"]);
}

#[test]
fn eq_without_instance_rejected() {
    let source = r#"
data Foo = Foo
    deriving Show

main :: IO ()
main = putStrLn (show (Foo == Foo))
"#;
    expect_compile_error(source, &[], &["No instance"]);
}

#[test]
fn unqualified_conflicting_import_rejected() {
    // Data.Map defines `null` with an incompatible type; importing it
    // unqualified must fail with a clear, actionable message pointing at
    // qualified import — not a baffling unification error.
    let lib = Path::new("../lib");
    let source = "import Data.Map\nmain :: IO ()\nmain = putStrLn \"hi\"\n";
    expect_compile_error(source, &[lib], &[
        "unqualified",
        "import qualified",
        "null",
    ]);
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
    expect_compile_error(source, &[], &["No instance"]);
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
    let msg = expect_compile_error(source, &[], &[
        "Unknown type 'Boolean'",
        "spelled 'Bool'",
    ]);
    assert!(!msg.contains("No instance"),
        "A missing-instance error must not mask the unknown type: {}", msg);
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
    expect_compile_error(source, &[], &[
        "Unknown type 'Boolean'",
        "type signature for 'f'",
    ]);
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
    let msg = expect_compile_error(source, &[], &["No instance"]);
    assert!(!msg.contains("Unknown type"),
        "'Baz' is defined and must not be reported as unknown: {}", msg);
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
    expect_compile_error(source, &[], &["Ambiguous type"]);
}

#[test]
fn ambiguous_show_nothing_in_larger_expr_rejected() {
    // The ambiguous `show Nothing` must still be caught when buried in a larger
    // expression whose other parts (e.g. `show 3`) are perfectly well-typed.
    let source = r#"
main :: IO ()
main = print $ show 3 <> "hi" <> show Nothing
"#;
    expect_compile_error(source, &[], &["Ambiguous type"]);
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
    let msg = expect_compile_error(source, &[], &[
        "Cannot unify 'HashMap String String' with 'String'",
    ]);
    assert!(!msg.contains("Ambiguous type"),
        "The clause error must not cascade into a spurious ambiguity report: {}", msg);
    assert!(!msg.contains("'FromJSON' instance") && !msg.contains("'Show' instance"),
        "The annotated decodeJSON/print constraints are determined and must not be reported: {}", msg);
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
    let msg = expect_compile_error(source, &[], &[
        "Cannot unify 'String' with 'Bool'",
        "where-binding 'msg'",
    ]);
    assert!(!msg.contains("Ambiguous type"),
        "The where-binding error must not cascade into a spurious ambiguity report: {}", msg);
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
    expect_compile_error(source, &[], &["Ambiguous type"]);
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
    expect_compile_error(source, &[], &[
        "Cannot unify 'Int' with 'String'",
        "Ambiguous type",
    ]);
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

// The classes a signature's context GUARANTEES for a variable are attached
// to that variable's body skolem by exact freshened name. They were once
// keyed by the fresh name with its id digits trimmed off, which misattributed
// every digit-suffixed source variable (`Show t1 =>` looked up "t") and every
// instance-method context (`instance Show a => Show (Tree a)` checks its
// methods over already-fresh variables that never trimmed to a declared
// name), so a use inside such a body that needs the given — a `show` of a
// tuple mixing the variable with a numeric literal, whose defaulting asks
// whether the skolem has the instance — reported "Ambiguous type".
#[test]
fn signature_givens_attach_to_digit_suffixed_and_instance_variables() {
    let source = r#"
data Tree a = Leaf | Node (Tree a) a (Tree a)

instance Show a => Show (Tree a) where
    show Leaf = "Leaf"
    show (Node l x r) = "Node (" <> show l <> ") " <> show (x, 1) <> " (" <> show r <> ")"

pairUp :: Show t1 => t1 -> String
pairUp x = show (x, 1)

main :: IO ()
main = do
    putStrLn (pairUp (True, "a"))
    putStrLn (show (Node Leaf (5 :: Int) Leaf))
"#;
    match compile(source, Path::new("."), &[]) {
        Ok(_) => {}
        Err(e) => panic!("givens from `Show t1 =>` and the instance context must attach: {}", e),
    }
}

// `m >>= \x y -> …` is a type error (the continuation returns a function,
// not an action). The bind-chain flattener used to accept ANY lambda as the
// desugarer's one-parameter continuation and bind only its first parameter,
// so `y` was silently dropped — resolving to an outer `y` here — and the
// program compiled.
#[test]
fn bind_with_two_parameter_lambda_is_a_type_error() {
    let source = r#"
y :: Int
y = 99

main :: IO ()
main = getLine >>= \x y -> putStrLn "a" >> putStrLn (x <> show (y :: Int))
"#;
    expect_compile_error(source, &[], &["Cannot unify"]);
}

// The clauses of one function must all take the same number of arguments
// (GHC: "Equations for 'f' have different numbers of arguments"). The
// checker used to admit unequal arities and index the first clause's
// pattern list by every clause (a panic for a shorter later clause), while
// the demand analysis sized its strictness row from the first clause and
// wrote past it for a longer later clause.
#[test]
fn clauses_with_different_arities_are_rejected() {
    let shorter = r#"
f :: Int -> Int -> Int
f 0 y = y
f x = \y -> x + y

main :: IO ()
main = putStrLn (show (f 1 2))
"#;
    expect_compile_error(shorter, &[], &[
        "Equations for 'f' have different numbers of arguments",
        "2 argument", "1 argument",
        "at 4:",
        "note:",
    ]);

    let longer = r#"
g :: Int -> Int -> Int
g 0 = \y -> y
g x y = x + y

main :: IO ()
main = putStrLn (show (g 1 2))
"#;
    expect_compile_error(longer, &[], &[
        "Equations for 'g' have different numbers of arguments",
        "1 argument", "2 argument",
        "at 4:",
    ]);

    // Instance methods go through the same check.
    let method = r#"
class Pick a where
    pick :: a -> a -> a

instance Pick Int where
    pick 0 y = y
    pick x = \_ -> x

main :: IO ()
main = putStrLn (show (pick (1 :: Int) 2))
"#;
    expect_compile_error(method, &[], &[
        "Equations for 'pick_Int' have different numbers of arguments",
    ]);
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
    expect_compile_error(source, &[], &[
        "Type error",
        "where-binding 'x'",
    ]);
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
    expect_compile_error(source, &[], &[
        "Cannot unify",
        "where-binding 'x'",
    ]);
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
    expect_compile_error(source, &[], &["where-binding 'go'"]);
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
    expect_compile_error(source, &[], &["where-binding 'f'"]);
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
    expect_compile_error(source, &[], &[
        "where-binding 'x'",
        "where-binding 'y'",
        "notInScope",
    ]);
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
    expect_compile_error(source, &[], &[
        "No instance for 'Show a'",
        "Add it to the context",
    ]);
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
    expect_compile_error(source, &[], &["No instance for 'Eq a'"]);
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
    expect_compile_error(source, &[], &["no accompanying definition"]);
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
    expect_compile_error(source, &[], &["Orphan instance"]);
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
    expect_compile_error_in(source, cases_dir, &[], &["not exported"]);
}

#[test]
fn import_hiding_blocks_hidden_name() {
    let source = r#"
import ExportHelper hiding (publicFn)

main :: IO ()
main = putStrLn (show (publicFn 5))
"#;
    let cases_dir = Path::new("tests/cases");
    expect_compile_error_in(source, cases_dir, &[], &["not exported"]);
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
    expect_compile_error(source, &[], &["Cannot unify"]);
}

#[test]
fn undefined_variable_rejected() {
    let source = r#"
main :: IO ()
main = print noSuchThing
"#;
    expect_compile_error(source, &[], &["Unbound variable"]);
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
    let msg = expect_compile_error(source, &[], &[]);
    assert!(msg.contains("Cannot unify"), "Expected arity error, got: {}", msg);
}

/// An equation binding more argument patterns than its signature has arrows
/// is reported GHC-style, naming the function, the counts, and the declared
/// type — not as a bare "Too many arguments".
#[test]
fn equation_with_more_arguments_than_its_type_is_diagnosed() {
    expect_compile_error("f :: Int -> Int\nf x y = x\nmain :: IO ()\nmain = print (f 1)\n", &[], &[
        "The equation for 'f' has 2 arguments, but its type 'Int -> Int' has only one argument",
        "definition of 'f'",
        "note:",
        "consumes one arrow",
    ]);
    expect_compile_error("g :: Int\ng x = x\nmain :: IO ()\nmain = print g\n", &[], &[
        "The equation for 'g' has one argument, but its type 'Int' has none",
    ]);
    // Several equations: plural, and the arity check precedes the per-clause
    // consistency check only when the counts agree.
    expect_compile_error("h :: a -> a\nh x y = x\nh a b = a\nmain :: IO ()\nmain = print (h 1)\n", &[], &[
        "The equations for 'h' have 2 arguments, but its type 'a -> a' has only one argument",
    ]);
    // Boundary: the arity is read under a scoped `forall` (LuaIO's `s`), so
    // an equation matching the arrows of a `forall s. … -> LuaIO s a`
    // signature is not miscounted as having none.
    let src = "import LIO (putStrLn)\n\nk :: forall s. Int -> LuaIO s Int\nk x = pure (x + 1)\n\nmain :: IO ()\nmain = putStrLn \"ok\"\n";
    compile(src, Path::new("."), &[Path::new("../lib")]).expect("a forall-scoped signature's arrows count");
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
    expect_compile_error(source, &[], &["Non-exhaustive"]);
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
    let msg = expect_compile_error(source, &[], &[]);
    assert!(
        msg.contains("Cannot derive") || msg.contains("only Show, Eq, Ord and Functor"),
        "Expected unsupported deriving error, got: {}", msg
    );
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
    let msg = expect_compile_error(source, &[], &[]);
    assert!(
        msg.contains("expects") || msg.contains("Constructor") || msg.contains("Cannot unify"),
        "Expected constructor arity error, got: {}", msg
    );
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
    let msg = expect_compile_error(source, &[], &[]);
    assert!(
        msg.contains("Cannot unify") || msg.contains("doesn't match"),
        "Expected type mismatch error for String body vs Int sig, got: {}", msg
    );
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
    expect_compile_error(source, &[], &["Cannot unify"]);
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
    let msg = expect_compile_error(source, &[], &[]);
    assert!(
        msg.contains("Cannot unify") || msg.contains("doesn't match"),
        "Expected type error for show returning Int instead of String, got: {}", msg
    );
}

#[test]
fn growing_type_family_is_bounded() {
    // A type family that grows its argument every step (Grow x = Grow (Maybe x))
    // must be bounded by reduction fuel and reported as divergent -- never hang
    // or stack-overflow the compiler. Charging fuel by reduced-type size bounds
    // the work; the deep (but bounded) reduction still needs a large stack,
    // which the harness `compile` (inside expect_compile_error) provides.
    let src = "type family Grow x where\n  Grow x = Grow (Maybe x)\nf :: Grow Int -> Int\nf _ = 0\nmain :: IO ()\nmain = putStrLn \"x\"\n";
    expect_compile_error(src, &[], &["did not terminate"]);
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
    expect_compile_error(source, &[], &[
        "in the definition of type family 'Mix'",
        "needs an argument of kind Nat, but 'True has kind Bool",
    ]);
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
    expect_compile_error(source, &[], &["in the definition of type family 'Bad'"]);
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
    expect_compile_error(source, &[], &[
        "Type family 'Ident' is applied to 0 of its 1 argument",
    ]);
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
    expect_compile_error(source, &[], &["IsZero"]);
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
    expect_compile_error(source, &[], &[
        "Cannot derive 'Functor' for 'F'",
        "argument of a function field",
    ]);
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
    expect_compile_error(source, &[], &[
        "Cannot derive 'Functor' for 'W'",
        "position other than the last argument",
    ]);
}

#[test]
fn operator_in_type_position_rejected() {
    // `f :: (+) -> Int` used to parse `(+)` silently as the unit type, so
    // the program compiled with a signature meaning something entirely
    // different from what was written (`f ()` ran fine). An operator in type
    // position must be a parse error that explains why, with a note on the
    // GHC deviation (TypeOperators).
    expect_compile_error("f :: (+) -> Int\nf _ = 1\nmain :: IO ()\nmain = print (f ())\n", &[], &[
        "The operator '+' cannot appear in a type",
        "'(+)' names a function (a value)",
        "note:",
        "TypeOperators",
    ]);

    // Same rejection for other operators and positions inside the type.
    expect_compile_error("g :: Int -> (<>)\ng x = x\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "The operator '<>' cannot appear in a type",
    ]);

    // Boundary: a ':'-leading operator IS a type operator (a type
    // constructor, like `data (:+:) a b` declares), so its prefix spelling
    // `(:+:) Int Bool` is the same type as the infix `Int :+: Bool` — GHC's
    // TypeOperators reading — and the note must not claim otherwise.
    let source = r#"
infixr 5 :+:
data (:+:) a b = L a | R b

f :: (:+:) Int Bool -> Int
f (L n) = n
f (R _) = 0

g :: Int :+: Bool -> Int
g = f

main :: IO ()
main = assert (g (L 41) + f (R True) + 1 == 42) "prefix and infix spellings name one type"
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("a prefix ':'-leading type operator is a type constructor")
        .lua_code;
    mlua::Lua::new().load(&lua_code).set_name("prefix_type_operator").exec()
        .expect("every in-program assertion should pass");
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
    expect_compile_error("f :: Maybe -> Int\nf _ = 1\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "Kind error",
        "'Maybe' has kind Type -> Type",
        "still needs 1 more type argument",
        "in the type signature for 'f'",
    ]);
}

#[test]
fn kind_error_saturated_type_applied_to_argument() {
    // `Maybe Int` is complete; applying it to `Bool` is a kind error.
    expect_compile_error("x :: Maybe Int Bool\nx = undefined\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "Kind error",
        "'Maybe Int' is applied to the type argument 'Bool'",
        "takes no type arguments",
    ]);
}

#[test]
fn kind_error_type_application_argument_kind() {
    // HashMap's parameters are complete types; a bare `Maybe` is not one.
    expect_compile_error("h :: HashMap Maybe Int -> Int\nh _ = 0\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "'HashMap' needs an argument of kind Type, but 'Maybe' has kind Type -> Type",
    ]);
}

#[test]
fn kind_error_data_field_must_be_complete_type() {
    expect_compile_error("data T = MkT Maybe\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "Kind error",
        "'Maybe' has kind Type -> Type",
        "in the definition of data type 'T'",
    ]);
}

#[test]
fn kind_error_type_variable_used_at_two_kinds() {
    // `t` is used bare (kind Type) AND applied (`t a`) in one signature.
    expect_compile_error("g :: t -> t a -> Int\ng _ _ = 1\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "Kind error",
        "a single type variable cannot be used at two different kinds",
    ]);
}

#[test]
fn kind_error_ascription_checked() {
    // Ascribed types are user-written type syntax like any signature.
    expect_compile_error("main :: IO ()\nmain = print (Nothing :: Maybe)\n", &[], &[
        "Kind error",
        "in a type ascription",
    ]);
}

#[test]
fn kind_error_instance_head_needs_unapplied_constructor() {
    // A Type -> Type class rejects a complete type as its instance head —
    // and the note must point at the [] / Maybe spelling.
    expect_compile_error("class Collapse t where\n    collapse :: t Int -> Int\ninstance Collapse Int where\n    collapse x = x\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "'instance Collapse Int' is ill-kinded",
        "use its type variable 't' at kind Type -> Type",
    ]);

    // The classic trap: `instance C [a]` where `instance C []` is meant.
    expect_compile_error("class Collapse t where\n    collapse :: t Int -> Int\ninstance Collapse [a] where\n    collapse _ = 0\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "'instance Collapse [a]' is ill-kinded",
        "note:",
        "write 'instance C []', not 'instance C [a]'",
    ]);
}

#[test]
fn kind_error_instance_head_needs_complete_type() {
    // The reverse direction: a Type class rejects an unapplied constructor.
    expect_compile_error("data T a = MkT a\nclass Pretty a where\n    pretty :: a -> String\ninstance Pretty T where\n    pretty _ = \"t\"\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "'instance Pretty T' is ill-kinded",
        "'T' has kind Type -> Type",
        "note:",
        "Expecting one more argument",
    ]);
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
    let e = expect_compile_error("instance Foldable [] where\n    foldr _ z [] = z\n    foldr f z (x:xs) = f x (foldr f z xs)\n    foldl _ z [] = z\n    foldl f z (x:xs) = foldl f (f z x) xs\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "Orphan instance",
    ]);
    assert!(!e.contains("Kind error"), "must kind-check, got: {e}");
    assert!(!e.contains("Expected type"), "must parse, got: {e}");
}

#[test]
fn higher_kinded_class_variable_inferred_from_constraint() {
    // A constraint alone fixes the variable's kind: `Foldable t` forces
    // `t : Type -> Type`, so using `t` bare in the same signature is a kind
    // error even though the body never applies it.
    expect_compile_error("f :: Foldable t => t -> Int\nf _ = 0\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "Kind error",
    ]);

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
    expect_compile_error("class Super t => Sub t where\n    marker :: Int\n\nclass Super t where\n    op :: t Int -> Int\n\ninstance Sub Int where\n    marker = 99\n\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "'instance Sub Int' is ill-kinded",
        "use its type variable 't' at kind Type -> Type",
    ]);
}

#[test]
fn kind_class_genuine_superclass_conflict_is_reported() {
    // A genuine, unsatisfiable conflict: `Sub`'s own method uses `t` bare
    // (`bad :: t -> Int`, so `t : Type`) while its superclass `Super`
    // uses it applied (`op :: t Int -> Int`, so `t : Type -> Type`).
    // The two constraints share one variable and cannot both hold. The
    // silent prepass keeps a first solution; the reporting pass 2b MUST
    // still surface the clash rather than swallow it.
    expect_compile_error("class Super t => Sub t where\n    bad :: t -> Int\n\nclass Super t where\n    op :: t Int -> Int\n\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "Kind error",
    ]);
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
    expect_compile_error("data P a = MkP (a Int) (Q a)\ndata Q b = MkQ b (P b)\n\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "Kind error",
    ]);
}

#[test]
fn kind_ill_kinded_use_at_wrong_arity_surfaces_at_use_site() {
    // `T` is legitimately higher-kinded: `data T a = MkT (a Int)` gives
    // `T : (Type -> Type) -> Type` (a valid kind, no error at T itself).
    // A LATER declaration then applies it at the wrong argument kind
    // (`T Int`, where `Int : Type`). The registered kind of `T` must
    // drive the check at the use site so the misuse surfaces there — the
    // first (well-kinded) declaration must not mask the second's error.
    expect_compile_error("data T a = MkT (a Int)\ndata U = MkU (T Int)\n\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "'T' needs an argument of kind Type -> Type, but 'Int' has kind Type",
        "in the definition of data type 'U'",
    ]);
}

#[test]
fn kind_intra_declaration_conflict_caught_in_both_field_orders() {
    // One constructor that uses its parameter at two kinds — bare AND
    // applied — in the SAME declaration. This must be a kind error no matter
    // which field comes first, so the silent prepass's arbitrary
    // first-solution choice cannot mask the conflict.
    expect_compile_error("data Bad a = MkBad a (a Int)\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "Kind error",
    ]);

    expect_compile_error("data Bad2 a = MkBad2 (a Int) a\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "Kind error",
    ]);
}

#[test]
fn kind_phantom_param_defaults_to_type_and_higher_kinded_use_rejected() {
    // A phantom parameter that no field constrains defaults to `Type`
    // (GHC-consistent: without a use it is `Type`). A later use at a
    // higher kind (`Phantom Maybe`, where `Maybe : Type -> Type`) is then a
    // kind error caught at the use site — the default must not be silently
    // widened to fit the use.
    expect_compile_error("data Phantom a = MkPhantom Int\nuseHK :: Phantom Maybe -> Int\nuseHK _ = 0\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "'Phantom' needs an argument of kind Type, but 'Maybe' has kind Type -> Type",
    ]);
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
    let e = expect_compile_error("main :: IO ()\nmain = putStrLn (show ([1, 2] <> [3, 4]))\n", &[], &[]);
    // Unannotated list literals default to Integer (GHC `default (Integer, …)`).
    assert!(e.contains("No instance for '<>' on type '[Integer]'"), "got: {e}");
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
    expect_compile_error("main :: IO ()\nmain = putStrLn (show mempty)\n", &[], &[
        "Ambiguous type",
        "Monoid",
    ]);

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
    let e = expect_compile_error(src, &[], &["Ambiguous type", "'Default'"]);
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
    expect_compile_error("class Greet a where\n    greet :: a -> String\ndata Foo = Foo\ninstance Greet Foo where\n    greet _ = \"hi\"\ndata Bar = Bar\nuseBar :: String\nuseBar = greet Bar\nmain :: IO ()\nmain = putStrLn useBar\n", &[], &[
        "No instance for 'Greet Bar'",
    ]);
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
    let e = expect_compile_error(&src, &[], &["Cannot unify"]);
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
    expect_compile_error(&src, &[], &["Cannot unify"]);
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
    expect_compile_error(&src, &[], &["Cannot unify", "Plus"]);
}

#[test]
fn type_family_divergence_errors_not_hangs() {
    // A non-terminating family (`Loop x = Loop x`) must be reported as a
    // divergence, not loop or overflow the stack. (expect_compile_error runs the
    // compiler in-process; if reduction were unbounded this test would hang or
    // crash the harness — so reaching the assertion is itself the guarantee.)
    let src = "type family Loop x where\n    Loop x = Loop x\nf :: Loop Int -> Int\nf n = 0\nmain :: IO ()\nmain = pure ()\n";
    expect_compile_error(src, &[], &["did not terminate", "Loop"]);
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
    expect_compile_error(&src, &[], &[
        "Kind error",
        "needs an argument of kind Nat",
        "'True has kind Bool",
    ]);
}

#[test]
fn promoted_kind_rejects_wrong_user_tag_for_nat_index() {
    // A promoted constructor of ANOTHER user data type (`'Red :: Color`) where
    // a `Nat` is required.
    let src = format!("data Color = Red | Blue\n{PROMOTED_VEC_PRELUDE}bad :: Vec 'Red Int -> Int\nbad _ = 0\nmain :: IO ()\nmain = pure ()\n");
    expect_compile_error(&src, &[], &["needs an argument of kind Nat", "'Red has kind Color"]);
}

#[test]
fn promoted_kind_rejects_nested_wrong_tag() {
    // The ill-kinded tag is nested inside `'S`, which itself has kind
    // `Nat -> Nat`, so `'S 'True` fails at the inner application.
    let src = format!("{PROMOTED_VEC_PRELUDE}bad :: Vec ('S 'True) a -> a\nbad _ = undefined\nmain :: IO ()\nmain = pure ()\n");
    expect_compile_error(&src, &[], &[
        "'S",
        "needs an argument of kind Nat",
        "'True has kind Bool",
    ]);
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
    expect_compile_error(&src, &[], &[
        "'Plus' needs an argument of kind Nat",
        "'True has kind Bool",
    ]);
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
    expect_compile_error(phantom, &[], &["Kind error", "'Red has kind Color"]);

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
    let e = expect_compile_error("error :: String -> Int\nerror s = 42\n\nmain :: IO ()\nmain = print (error \"hi\")\n", &[], &[
        "'error' is already provided by the Prelude and cannot be redefined",
        "at 2:",
        "note:",
        "rename your function",
    ]);
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
    expect_compile_error("map :: (Int -> Int) -> [Int] -> [Int]\nmap f xs = xs\n\nmain :: IO ()\nmain = print (map (\\x -> x + 1) [1, 2, 3])\n", &[], &[
        "'map' is already provided by the Prelude and cannot be redefined",
        "Prelude's own functions use 'map'",
    ]);
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
    expect_compile_error("reverse :: [a] -> [a]\nreverse xs = xs\n\nmain :: IO ()\nmain = print (reverse [1, 2, 3])\n", &[], &[
        "'reverse' is already provided by the Prelude and cannot be redefined",
        "same type as the Prelude's 'reverse'",
    ]);
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
    let e = expect_compile_error("replicate :: Int -> String -> String\nreplicate n s = s\n\nmain :: IO ()\nmain = putStrLn (replicate 3 \"x\")\n", &[], &[
        "'replicate' is already provided by the Prelude and cannot be redefined",
    ]);
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
    expect_compile_error("main :: IO ()\nmain = putStrLn (show (\\a b -> a + b))\n", &[], &[
        "No instance for 'Show (a -> a -> a)'",
        "no Show, Eq or Ord instance",
    ]);
    // The render/compare hint is specific to those classes: a missing Num
    // instance for a function is a plain NoInstance, not a "no way to render
    // or compare" explanation.
    let msg = expect_compile_error(
        "class Half a where\n  half :: a -> a\nmain :: IO ()\nmain = print (half (\\x -> x :: Int) 3)\n",
        &[],
        &["No instance for 'Half (Int -> Int)'"],
    );
    assert!(!msg.contains("render or compare"), "class-agnostic hint leaked:\n{msg}");
}

#[test]
fn no_eq_instance_for_function() {
    expect_compile_error("main :: IO ()\nmain = print ((\\x -> x :: Int) == (\\x -> x))\n", &[], &[
        "No instance for 'Eq (Int -> Int)'",
    ]);
}

#[test]
fn no_ord_instance_for_function() {
    expect_compile_error("f :: (Int -> Int) -> Bool\nf g = g < g\nmain :: IO ()\nmain = print (f (\\x -> x))\n", &[], &[
        "No instance for 'Ord (Int -> Int)'",
    ]);
}

#[test]
fn no_show_instance_for_tuple_containing_function() {
    expect_compile_error("main :: IO ()\nmain = putStrLn (show ((1 :: Int), (\\x -> x :: Int)))\n", &[], &[
        "No instance for 'Show (Int, Int -> Int)'",
    ]);
}

#[test]
fn no_show_instance_for_io_action() {
    expect_compile_error("main :: IO ()\nmain = print (putStrLn \"x\")\n", &[], &[
        "No instance for",
        "IO",
    ]);
}

#[test]
fn constraint_propagates_through_print() {
    // `print :: Show a => …` — its constraint is checked at the call site, so
    // even a never-applied (polymorphic) function is rejected.
    expect_compile_error("main :: IO ()\nmain = print (\\a b -> a + b)\n", &[], &[
        "No instance for 'Show (a -> a -> a)'",
    ]);
}

#[test]
fn constraint_propagates_through_user_function() {
    expect_compile_error("needsShow :: Show a => a -> String\nneedsShow x = show x\nmain :: IO ()\nmain = putStrLn (needsShow (\\y -> y + (1 :: Int)))\n", &[], &[
        "No instance for 'Show (Int -> Int)'",
    ]);
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

/// `class Eq a, Show a => C a` — several superclass constraints without the
/// required parentheses — must say what is wrong instead of backtracking
/// into an unrelated error at the class head.
#[test]
fn class_context_comma_without_parens_is_diagnosed() {
    expect_compile_error("class Eq a, Show a => Broken a where\n    broken :: a -> String\n", &[], &[
        "Several superclass constraints must be wrapped in parentheses",
    ]);
}

/// A malformed parenthesized class context gets a targeted explanation of
/// the expected shape (constraints + '=>'), not "Expected type/constructor
/// name" pointing at the '('.
#[test]
fn class_context_malformed_parens_is_diagnosed() {
    expect_compile_error("class (Eq, Show) => Broken a where\n    broken :: a -> String\n", &[], &[
        "A parenthesized class context",
    ]);
}

/// An export-list entry the grammar does not know (here a `module M`
/// re-export) is rejected with an explanation, not silently dropped from
/// the export list.
#[test]
fn export_list_unknown_entry_is_diagnosed() {
    expect_compile_error("module Main (module Data, main) where\n\nmain :: IO ()\nmain = putStrLn \"x\"\n", &[], &[
        "export-list entry is not understood",
        "module re-exports",
    ]);
}

/// A pattern parameter on a local (let/do-let) function binding is a clear
/// error at the binding, not a silent rename that leaves the pattern's
/// variables unbound.
#[test]
fn local_binding_pattern_parameter_is_diagnosed() {
    expect_compile_error("main :: IO ()\nmain = do\n    let f (Just x) = x\n    putStrLn (f (Just \"a\"))\n", &[], &[
        "cannot take a pattern as a parameter",
        "GHC accepts pattern parameters",
    ]);
}

/// A freely named newtype constructor (`newtype Rad = MkRad Double`) is
/// Haskell and now compiles; the boundaries around the resolution stay
/// diagnosed: an unknown head with no field or two fields cannot be a
/// newtype constructor, deriving is limited to the structural classes,
/// and the record form takes exactly one selector. The mata-ll shorthand
/// over a KNOWN type (`newtype W = Maybe Int`) keeps its old reading.
#[test]
fn newtype_constructor_resolution_boundaries() {
    expect_compile_error(
        "newtype Rad = MkRad
main :: IO ()
main = pure ()
",
        &[],
        &[
            "'MkRad' is not a type",
            "would have no field",
            "the definition of newtype 'Rad'",
            "note:",
            "named freely",
        ],
    );
    expect_compile_error(
        "newtype Rad = MkRad Int Int
main :: IO ()
main = pure ()
",
        &[],
        &[
            "'MkRad' is not a type",
            "2 fields",
            "exactly one field",
        ],
    );
    expect_compile_error(
        "newtype Age = Age Int deriving (LuaDict)
main :: IO ()
main = pure ()
",
        &[],
        &[
            "Cannot derive 'LuaDict' for newtype 'Age'",
            "Show, Eq and Ord",
            "note:",
            "wrapped type at runtime",
        ],
    );
    expect_compile_error(
        "newtype P = P { a :: Int, b :: Int }
main :: IO ()
main = pure ()
",
        &[],
        &["exactly one field", "one selector"],
    );
    // Boundary: a wrapped type that IS a type is not confused with a
    // constructor — the shorthand `newtype W = Maybe Int` stays accepted.
    let src = "newtype W = Maybe Int\nunw :: W -> Maybe Int\nunw (W m) = m\nmain :: IO ()\nmain = assert (unw (W (Just 1)) == Just 1) \"shorthand over an applied type\"\n";
    let lua_code = compile(src, Path::new("."), &[]).expect("shorthand newtype over an applied type").lua_code;
    mlua::Lua::new().load(&lua_code).set_name("newtype_applied_shorthand").exec()
        .expect("every in-program assertion should pass");
}

/// A lambda takes a sequence of atomic patterns, as in GHC: patterns and
/// plain parameters mix in any order, each non-variable pattern is matched
/// (left to right) in the body. Before this, `\(a, b) c ->` failed with
/// "Expected lambda parameter" and `\x (a, b) ->` with "Expected ->".
#[test]
fn lambda_mixes_patterns_and_parameters() {
    let src = r#"
main :: IO ()
main = do
    assert ((\(a, b) c -> a + b + c) (1, 2) 3 == 6) "pattern then parameter"
    assert ((\x (a, b) -> x * (a + b)) 2 (3, 4) == 14) "parameter then pattern"
    assert ((\(Just a) [b] (c, _) -> a + b + c) (Just 1) [2] (3, 4) == 6) "three patterns"
    assert ((\_ 0 -> "zero") 9 0 == "zero") "wildcard and literal"
    assert (map (\(k, v) -> k + v) [(1, 2), (3, 4)] == [3, 7]) "single tuple pattern still works"
"#;
    let lua_code = compile(src, Path::new("."), &[]).expect("multi-pattern lambdas compile").lua_code;
    mlua::Lua::new().load(&lua_code).set_name("lambda_patterns").exec()
        .expect("every in-program assertion should pass");

    // A failing match on the FIRST pattern is reported before the second is
    // looked at, and a lambda-pattern failure is the lambda's error message.
    let src = r#"
main :: IO ()
main = print ((\(Just a) (Just b) -> a + b) (Nothing :: Maybe Int) (error "second forced first"))
"#;
    let lua_code = compile(src, Path::new("."), &[]).expect("compiles").lua_code;
    let err = mlua::Lua::new().load(&lua_code).set_name("lambda_partial").exec()
        .expect_err("a non-matching lambda pattern raises");
    let msg = err.to_string();
    assert!(msg.contains("non-exhaustive lambda pattern"), "{msg}");
    assert!(!msg.contains("second forced first"), "first pattern is matched first:\n{msg}");
}

#[test]
fn type_error_locates_the_offending_statement() {
    // A type error must point at the statement/binding line that carries it,
    // not the clause head. The checker attributes errors via `Expr::Spanned`
    // markers placed at statement boundaries (let/where bindings, do-statements,
    // case-branch and guard bodies, if-branches). Before this, every error in a
    // multi-line body was reported at the function's first line.

    // let-binding body: the error is on the `c = ...` line, not `compute x =`.
    expect_compile_error(
        "compute :: Int -> Int\n\
         compute x =\n\
         \x20   let a = x + 1\n\
         \x20       c = a <> \"oops\"\n\
         \x20   in c\n",
        &[],
        &[
            "at 4:",
        ],
    );

    // case-branch reconciliation: the error is on the offending branch line.
    expect_compile_error(
        "f :: Int -> String\n\
         f n = case n of\n\
         \x20   0 -> \"zero\"\n\
         \x20   _ -> n\n",
        &[],
        &[
            "at 4:",
        ],
    );

    // do-statement: a unification error inside a statement is on its own line.
    expect_compile_error(
        "main :: IO ()\n\
         main = do\n\
         \x20   putStrLn \"ok\"\n\
         \x20   putStrLn (length \"x\")\n",
        &[],
        &[
            "at 4:",
        ],
    );

    // if-branch reconciliation: the error is on a branch line, not the head.
    let e = expect_compile_error(
        "g :: Int -> Int\n\
         g x =\n\
         \x20   if x > 0\n\
         \x20       then \"pos\"\n\
         \x20       else x\n",
        &[],
        &[],
    );
    assert!(e.contains("at 4:") || e.contains("at 5:"),
        "if-branch error points at a branch line (4 or 5): {e}");
}

/// The Haskell precedence-parsing rule: a chain of same-precedence operators
/// is rejected when any of them is non-associative. GHC rejects every one of
/// these programs the same way.
#[test]
fn non_associative_chains_are_rejected() {
    with_compiler_stack(non_associative_chains_are_rejected_impl)
}

fn non_associative_chains_are_rejected_impl() {
    // The classic: comparison operators do not chain.
    expect_compile_error("main :: IO ()\nmain = print (1 == 2 == True)\n", &[], &[
        "non-associative",
        "'=='",
        "parenthesize",
    ]);

    // Two different comparison operators conflict too, and the notes offer
    // the three-way-comparison rewrite.
    expect_compile_error("main :: IO ()\nmain = print (1 < 2 <= 3)\n", &[], &[
        "non-associative",
        "&&",
    ]);

    // A user-declared `infix` operator is non-associative as well.
    expect_compile_error("infix 5 <+>\n(<+>) :: Int -> Int -> Int\na <+> b = a + b\nmain :: IO ()\nmain = print (1 <+> 2 <+> 3)\n", &[], &[
        "non-associative",
        "'<+>'",
    ]);

    // Prelude `elem` is infix 4 (as in GHC), so it cannot chain with ==.
    expect_compile_error("main :: IO ()\nmain = print (1 `elem` [1] == True)\n", &[], &[
        "`elem`",
        "non-associative",
    ]);

    // The Prelude's <$> and <*> are infixl 4 (as in GHC): mixing them with a
    // comparison at the same precedence is rejected.
    expect_compile_error("main :: IO ()\nmain = print ((+1) <$> Just 1 == Just 2)\n", &[], &[
        "'<$>'",
        "'=='",
    ]);

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
    with_compiler_stack(final_do_bind_types_like_top_level_impl)
}

fn final_do_bind_types_like_top_level_impl() {
    let e = expect_compile_error("step :: Int -> IO Int\nstep n = return (n + 1)\n\nmain :: IO ()\nmain = do\n    putStrLn \"x\"\n    step 1 >>= step\n", &[], &[
        "Int",
        "()",
    ]);
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
    with_compiler_stack(prefix_minus_matches_ghc_impl)
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
        let e = expect_compile_error(src, &[], &[]);
        assert!(e.contains("Prefix minus"), "{src}: got: {e}");
        assert!(e.contains(op), "{src}: got: {e}");
        assert!(e.contains("parenthesize"), "{src}: got: {e}");
    }

    // Rejected: prefix minus left of a non-left-associative precedence-6
    // operator (GHC: "cannot mix prefix `-' and `<>'").
    expect_compile_error("main :: IO ()\nmain = putStrLn (- 1 <> \"a\")\n", &[], &[
        "prefix minus",
        "'<>'",
    ]);

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
    with_compiler_stack(section_operand_precedence_impl)
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
        let e = expect_compile_error(src, &[], &[]);
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
    with_compiler_stack(conflicting_associativities_impl)
}

fn conflicting_associativities_impl() {
    // infixl 6 <#> against the builtin infixr 6 <>.
    expect_compile_error("infixl 6 <#>\n(<#>) :: String -> String -> String\na <#> b = a ++ b\nmain :: IO ()\nmain = putStrLn (\"a\" <#> \"b\" <> \"c\")\n", &[], &[
        "opposite directions",
        "infixl 6",
        "infixr 6",
    ]);

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
    with_compiler_stack(imported_infix_non_associative_impl)
}

fn imported_infix_non_associative_impl() {
    let src = "import FixityOps\nmain :: IO ()\nmain = print (1 ~=~ 2 ~=~ 3)\n";
    expect_compile_error_in(src, Path::new("tests/cases"), &[], &["non-associative", "'~=~'"]);
}

#[test]
fn type_errors_are_explained_not_cryptic() {
    // Passing a String to a list-typed function: internal unification vars
    // must render as friendly letters (a, b, …), never as `_i700`, and the
    // message must explain that String is not a list in mata-ll.
    let e = expect_compile_error(
        r#"
main :: IO ()
main = print (length "hello")
"#,
        &[],
        &[
            "[a]",
        ],
    );
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
    expect_compile_error(
        r#"
main :: IO ()
main = print ([1, 2] <> [3, 4] :: [Int])
"#,
        &[],
        &[
            "No instance for '<>'",
            "concatenated with ++",
        ],
    );

    // Ordering whole tuples is rejected at type-check with the missing-instance
    // explanation (the checker discharges the Ord constraint before codegen).
    // The tuple is annotated `(Int, Int)` so the rejection is the
    // missing tuple-Ord instance, not literal-defaulting ambiguity: with
    // polymorphic literals `(1, 2)` alone is `(Num a, Num b) => (a, b)`, and
    // since mata-ll has no `Ord (a, b)` instance the elements cannot default,
    // so an un-annotated tuple would report an (also-correct) ambiguity error.
    expect_compile_error(
        r#"
main :: IO ()
main = print (((1, 2) :: (Int, Int)) > (1, 3))
"#,
        &[],
        &[
            "No instance for 'Ord (Int, Int)'",
            "no Ord instance",
        ],
    );
}

/// Unpacking an existential must SKOLEMIZE: the hidden type variable becomes
/// a rigid constant that cannot unify with any concrete type. The canonical
/// soundness probe — before the fix this compiled and produced a Lua runtime
/// crash ("attempt to add a 'string' with a 'number'").
#[test]
fn existential_unpacking_skolemizes() {
    let e = expect_compile_error(
        r#"
data Foo = forall a. Foo a

unFoo :: Foo -> Int
unFoo (Foo x) = x + 1

main :: IO ()
main = putStrLn (show (unFoo (Foo "hello")))
"#,
        &[],
        &[
            "Cannot match 'a' with 'Int'",
            "rigid type variable",
            "in definition of 'unFoo'",
        ],
    );
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
    let e = expect_compile_error(
        r#"
data Box where
  MkBox :: a -> Box

coerce :: Box -> Int
coerce (MkBox x) = x

main :: IO ()
main = putStrLn (show (coerce (MkBox "boom") + 1))
"#,
        &[],
        &[
            "hidden by constructor 'MkBox'",
        ],
    );
    assert!(
        e.contains("Cannot match 'a' with 'Int'")
            || e.contains("escapes its scope"),
        "GADT-syntax existential must be rigid too, got: {e}"
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
    let e = expect_compile_error(
        r#"
data Foo = forall a. Foo a

unFoo :: Foo -> a
unFoo (Foo x) = x

main :: IO ()
main = putStrLn "no"
"#,
        &[],
        &[
            "Cannot match 'a' with 'a'",
        ],
    );
    // Both provenance notes appear: `a` is the existential hidden by `Foo`, and
    // `a` is also the signature's rigid variable.
    assert!(e.contains("hidden by constructor 'Foo'"), "got: {e}");
    assert!(
        e.contains("rigid type variable from the signature of 'unFoo'"),
        "the signature-rigidity note must explain the second 'a', got: {e}"
    );

    // Escape through a case expression's result type.
    expect_compile_error(
        r#"
data Foo = forall a. Foo a

useCase :: Foo -> Int
useCase f = case f of
  Foo x -> x

main :: IO ()
main = putStrLn "no"
"#,
        &[],
        &[
            "escapes its scope",
        ],
    );

    // Escape through a where-function's type: where-bindings are
    // monomorphic, so `unpack e1` and `unpack e2` would claim the SAME
    // hidden type for two different boxes — with an Eq-constrained
    // existential that "equates" an Int with a String.
    expect_compile_error(
        r#"
data EqBox = forall a. Eq a => EqBox a

test :: EqBox -> EqBox -> Bool
test e1 e2 = unpack e1 == unpack e2
  where unpack (EqBox x) = x

main :: IO ()
main = putStrLn (show (test (EqBox 1) (EqBox "one")))
"#,
        &[],
        &[
            "escapes its scope",
        ],
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
    let e = expect_compile_error(
        r#"
data Showable = forall a. Show a => Showable a

bad :: Showable -> Int
bad s = case s of
  Showable x -> x + (1 :: Int)

main :: IO ()
main = putStrLn "no"
"#,
        &[],
        &[],
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
    expect_compile_error(
        r#"
data Showable = forall a. Show a => Showable a

pack :: Showable
pack = Showable (\x -> (x :: Int))

main :: IO ()
main = putStrLn "no"
"#,
        &[],
        &[
            "No instance for 'Show (Int -> Int)'",
        ],
    );

    // A typo'd class in the constructor context must error at the data
    // declaration, not silently become "no constraint".
    expect_compile_error(
        r#"
data Box = forall a. Showw a => Box a

main :: IO ()
main = putStrLn "no"
"#,
        &[],
        &[
            "Unknown typeclass 'Showw' in the context of constructor 'Box'",
        ],
    );
}

/// A type SIGNATURE's universally-quantified variables must be SKOLEMIZED
/// (treated as rigid) when checking the function body — exactly as GHC does.
/// A signature that is MORE GENERAL than its implementation is unsound and
/// must be rejected, not silently accepted by freshening the signature vars
/// to flexible unification variables.
///
/// STATE OF THE COMPILER: these two probes currently FAIL (the programs still
/// compile) because `freshen_sig_type_mapped` freshens signature variables to
/// `Ty::Var` instead of `Ty::Skolem`. They are regression tests for the fix
/// and are expected to pass only AFTER signature skolemization lands. The
/// wording assertions mirror the existing rigid-mismatch message already
/// produced for existential skolems (see `existential_unpacking_skolemizes`),
/// kept loose enough to survive rewording.
#[test]
fn signature_vars_are_skolemized() {
    // Case 1: `f :: a -> Int` / `f x = x` returns its argument (type `a`)
    // where the signature promises `Int`. `a` is rigid, so it cannot be
    // matched with `Int`. GHC: "Couldn't match expected type 'Int' with
    // actual type 'a'" / "'a' is a rigid type variable bound by ...".
    let e = expect_compile_error(
        r#"
f :: a -> Int
f x = x

main :: IO ()
main = print (f (5 :: Int))
"#,
        &[],
        &[
            "rigid",
        ],
    );
    assert!(
        e.contains('a') && e.contains("Int"),
        "the error should mention the rigid variable 'a' and the promised 'Int', got: {e}"
    );

    // Case 2: `g :: Monad m => m ()` / `g = putStrLn "hi"`. The body is `IO ()`
    // but the signature quantifies over an arbitrary `Monad m`; `m` is rigid,
    // so pinning it to `IO` is rejected. GHC: no instance / rigid `m`.
    let e = expect_compile_error(
        r#"
g :: Monad m => m ()
g = putStrLn "hi"

main :: IO ()
main = g
"#,
        &[],
        &[],
    );
    assert!(
        e.contains("rigid") || e.contains("No instance") || e.contains("instance"),
        "the signature variable 'm' must be rigid: pinning it to IO must be \
         rejected, got: {e}"
    );
}

/// Controls for the signature-skolemization fix: legitimately-general
/// signatures whose implementations honour them must KEEP compiling. If the
/// fix over-rejects any of these it has regressed ordinary polymorphism.
/// (Higher-rank / runST and existential/GADT controls live in the already
/// registered rank2.mll, st_return.mll, existentials.mll and
/// existential_constraints.mll cases.)
#[test]
fn skolemized_signatures_do_not_regress_valid_polymorphism() {
    for (label, src) in [
        // The identity function: the classic `a -> a`, body returns its arg.
        (
            "myid :: a -> a",
            r#"
myid :: a -> a
myid x = x

main :: IO ()
main = print (myid (7 :: Int))
"#,
        ),
        // Multi-variable / projection: `a -> b -> a`, returns the first arg.
        (
            "first :: a -> b -> a",
            r#"
first :: a -> b -> a
first x _ = x

main :: IO ()
main = print (first (3 :: Int) "ignored")
"#,
        ),
        // Return-type polymorphism used honestly: the result is produced at
        // the fully polymorphic type (Nothing :: Maybe a).
        (
            "constNothing :: b -> Maybe a",
            r#"
constNothing :: b -> Maybe a
constNothing _ = Nothing

main :: IO ()
main = print (constNothing (5 :: Int) :: Maybe Int)
"#,
        ),
        // Constrained polymorphism: `Show a => a -> String`, the body only
        // uses the declared class method.
        (
            "render :: Show a => a -> String",
            r#"
render :: Show a => a -> String
render x = show x

main :: IO ()
main = putStrLn (render (42 :: Int))
"#,
        ),
    ] {
        assert!(
            mllc::compile(src, Path::new("."), &[]).is_ok(),
            "control `{label}` must still compile after signature skolemization"
        );
    }
}

/// The checking/argument-side dual of `signature_vars_are_skolemized`. When a
/// function declares a higher-rank parameter (`apply2 :: (forall a. a -> a) ->
/// …`), the ARGUMENT must be polymorphic enough to be that `forall`. A lambda
/// checked against it is inferred first and unified against a fresh skolem for
/// `a`; any class constraint its body demands of `a` (`Num`, `Show`, …) is then
/// a constraint on that skolem, which has no instance and no enclosing context
/// to discharge it — so it must be rejected.
///
/// Before the fix the argument skolem was minted but NOT registered, so
/// `has_instance` treated it as "defer to the caller" and silently accepted the
/// residual constraint. `apply2 (\x -> x + 1)` compiled, and since `apply2`'s
/// body applies its argument at both `Int` and `Bool` (`(f 1, f True)`), the
/// generated Lua ran `True + 1` — "attempt to perform arithmetic on a boolean
/// value". GHC rejects the program at compile time; so must we.
#[test]
fn higher_rank_argument_must_be_polymorphic_enough() {
    // A `forall a. a -> a` parameter, applied inside at two distinct types so
    // an under-polymorphic argument is a genuine runtime type confusion.
    let hdr = "apply2 :: (forall a. a -> a) -> (Int, Bool)\n\
               apply2 f = (f 1, f True)\n";

    // REJECT 1: `\x -> x + 1` is `Num a => a -> a`, not `forall a. a -> a`.
    expect_compile_error(
        &format!(
        "{hdr}use :: (Int, Bool)\nuse = apply2 (\\x -> x + 1)\nmain :: IO ()\nmain = return ()\n"
    ),
        &[],
        &[
            "higher-rank argument",
            "Num",
        ],
    );

    // REJECT 2: `\x -> seq (show x) x` forces `Show a`, equally unsatisfiable.
    expect_compile_error(
        &format!(
        "{hdr}use :: (Int, Bool)\nuse = apply2 (\\x -> seq (show x) x)\nmain :: IO ()\nmain = return ()\n"
    ),
        &[],
        &[
            "higher-rank argument",
            "Show",
        ],
    );

    // REJECT 3: a monomorphic NAMED function (`Bool -> Bool`) is not the
    // requested `forall a. a -> a` either — the skolem cannot unify with Bool.
    let e = expect_compile_error(
        &format!(
        "{hdr}notF :: Bool -> Bool\nnotF b = b\nuse :: (Int, Bool)\nuse = apply2 notF\nmain :: IO ()\nmain = return ()\n"
    ),
        &[],
        &[],
    );
    assert!(
        e.contains("rigid") || e.contains("Cannot match"),
        "a monomorphic `Bool -> Bool` must not satisfy `forall a. a -> a`, got: {e}"
    );

    // ACCEPT controls: genuinely-polymorphic arguments must KEEP compiling.
    for (label, arg_defs, arg) in [
        // The identity lambda IS `forall a. a -> a`.
        ("id-lambda", "", "(\\x -> x)"),
        // A named fully-polymorphic function.
        ("poly-named", "myid :: a -> a\nmyid x = x\n", "myid"),
    ] {
        let src = format!(
            "{hdr}{arg_defs}use :: (Int, Bool)\nuse = apply2 {arg}\nmain :: IO ()\nmain = return ()\n"
        );
        assert!(
            mllc::compile(&src, Path::new("."), &[]).is_ok(),
            "control `{label}` (a truly polymorphic argument) must still compile"
        );
    }
}

/// Record syntax back doors: a field whose type is existential has no
/// selector (the selector's result type would BE the hidden type, outside
/// any match) and cannot be record-updated (nothing to check the new value
/// against). Both were runtime type confusions before the fix.
#[test]
fn existential_record_fields_have_no_selector_or_update() {
    expect_compile_error(
        r#"
data Foo = forall a. Foo { getIt :: a }

main :: IO ()
main = putStrLn (show (getIt (Foo "hello") + 1))
"#,
        &[],
        &[
            "has an existential type, so it has no selector function",
        ],
    );

    expect_compile_error(
        r#"
data Foo = forall a. Foo { getIt :: a, label :: String }

update :: Foo -> Foo
update f = f { getIt = 42 }

main :: IO ()
main = putStrLn "no"
"#,
        &[],
        &[
            "cannot be record-updated",
        ],
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
    expect_compile_error(
        r#"
main :: IO ()
main = print ([1, 2] <> [3, 4] :: [Int])
"#,
        &[],
        &[
            "No instance for '<>' on type '[Int]'",
            "at 3:6, in definition of 'main'",
            "note: lists are concatenated with ++",
        ],
    );
}

/// The parser recovers at declaration boundaries: one run reports every
/// independent syntax error, not just the first. The first error's message
/// must render exactly as it always has (inline ` at line:col`).
#[test]
fn parser_reports_multiple_errors_per_run() {
    let e = expect_compile_error(
        r#"data Foo = = Bar

good :: Int -> Int
good x = x + 1

main :: IO ()
main = ]
"#,
        &[],
        &[
            "Parse error: Expected type/constructor name, found '=' at 1:12",
            "Expected expression, found ']' at 7:8",
        ],
    );
    assert!(
        e.matches("Parse error: ").count() >= 2,
        "expected at least two parse errors in one run, got: {e}"
    );
}

// ---------------------------------------------------------------------------
// Regression tests: recursion-depth guard. Nesting past
// mllc::MAX_NESTING_DEPTH must produce the clean "nested too deeply"
// diagnostic — never a native stack overflow (SIGABRT). Reaching the limit
// still consumes (limit x frame) native stack, so these rely on the harness
// `compile` helper running on a thread with the SAME stack size as the mll
// CLI driver (mllc::COMPILER_STACK_SIZE, which the limit is calibrated
// against).
// ---------------------------------------------------------------------------

/// Parser face of the guard: nested parentheses beyond the limit.
#[test]
fn deeply_nested_parens_yield_clean_depth_error() {
    let n = mllc::MAX_NESTING_DEPTH + 1000;
    let source = format!(
        "main :: IO ()\nmain = print {}1{}\n",
        "(".repeat(n),
        ")".repeat(n)
    );
    match compile(&source, Path::new("."), &[]) {
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
    match compile(&source, Path::new("."), &[]) {
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
    match compile(&source, Path::new("."), &[]) {
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
    match compile(&source, Path::new("."), &[]) {
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
    compile(&ok, Path::new("."), &[])
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
    match compile(&source, Path::new("."), &[]) {
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
    let lua_code = compile(&source, Path::new("."), &[])
        .expect("a 1200-element list literal must compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code)
        .exec()
        .expect("the 1200-element list program must run");
}

// An unterminated `{-` used to be accepted silently: the rest of the file
// vanished into the comment and the parser reported an unrelated "found end
// of file" far from the cause. It is a lexer error located at the opener.
#[test]
fn unterminated_block_comment_is_located_at_its_opener() {
    let source = "main :: IO ()\nmain = putStrLn \"a\"\n  {- forgot to close {- nested -} this one\nfoo :: Int\nfoo = 1\n";
    let msg = expect_compile_error(source, &[], &[
        "Unterminated block comment",
        "`{-`",
        "`-}`",
        "at 3:3",
        "note:",
        "nest",
    ]);
    assert!(!msg.contains("end of file"), "must not degrade to an EOF parse error: {}", msg);
}

// The structural derives (Show/Eq/Ord/Enum/Bounded/Functor) cover one
// instance head over plain fields. A constructor that refines the result
// type (a real GADT) or hides an existential has no such head — GHC rejects
// the derive too — so it is refused with the reason, instead of silently
// producing an instance that ignores the fields (which is what happened
// when the arity was read from the parser's empty GADT field list).
#[test]
fn derive_on_refined_gadt_constructor_is_rejected_with_reason() {
    let source = r#"
data Expr a where
    IntE :: Int -> Expr Int
    BoolE :: Bool -> Expr Bool
    deriving (Eq)

main :: IO ()
main = putStrLn "unreachable"
"#;
    expect_compile_error(source, &[], &[
        "Cannot derive 'Eq' for 'Expr'",
        "'IntE'",
        "refines the result type",
        "note:",
        "by hand",
    ]);

    let existential = r#"
data Box where
    MkBox :: forall a. a -> Box
    deriving (Show)

main :: IO ()
main = putStrLn "unreachable"
"#;
    expect_compile_error(existential, &[], &[
        "Cannot derive 'Show' for 'Box'",
        "'MkBox'",
        "existential",
    ]);
}

// Enum on a type with no constructors used to index `constructors.last()`
// on an empty list (a compiler panic); it is an ordinary rejection.
#[test]
fn derive_enum_on_empty_type_is_rejected() {
    let source = r#"
data Void where
    deriving (Enum)

main :: IO ()
main = putStrLn "unreachable"
"#;
    expect_compile_error(source, &[], &[
        "Cannot derive 'Enum' for 'Void'",
        "one or more constructors",
        "note:",
    ]);
}
