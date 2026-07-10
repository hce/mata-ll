-- deriving (FromJSON): the native derive generates a decoder over the JSON
-- module's combinators, and return-nested dispatch resolves it through
-- `decodeJSON s :: Either String T` (fromJSON's class variable sits inside
-- `Either String a`). Covers: records, sum types (tagged objects and bare
-- nullary strings), single positional constructors, nested user types,
-- [a] and Maybe a fields, recursive and mutually recursive types, Json
-- passthrough fields, integer strictness, and the error paths with their
-- exact field-naming messages.
import JSON

-- Single-constructor record: decodes from an object keyed by field name.
data Person = Person { name :: String, age :: Integer }
    deriving (Eq, FromJSON)

-- Multi-constructor sum: record constructor (fields inline next to the tag),
-- positional constructor (arguments under "contents"), nullary constructor.
data Shape = Circle { radius :: Number } | Rect Integer Integer | Point0
    deriving (Eq, FromJSON)

-- All-nullary sum: decodes from bare constructor-name strings.
data Color = RedC | GreenC | BlueC
    deriving (Eq, FromJSON)

-- Single positional constructors: one argument is the value itself,
-- several are an array.
data Wrap = Wrap Integer
    deriving (Eq, FromJSON)

data Pair2 = Pair2 Integer String
    deriving (Eq, FromJSON)

-- Single positional Maybe: null <-> Nothing at the top level.
data OptW = OptW (Maybe Integer)
    deriving (Eq, FromJSON)

-- Lone nullary constructor: the constructor name is the payload.
data Unit1 = Unit1
    deriving (Eq, FromJSON)

-- Nested user types, list of user type, optional field.
data Team = Team { leader :: Person, members :: [Person], motto :: Maybe String }
    deriving (Eq, FromJSON)

-- Deep nesting.
data Org = Org { teams :: [Team] }
    deriving (Eq, FromJSON)

-- Recursive types.
data JTree = JLeaf Integer | JNode [JTree]
    deriving (Eq, FromJSON)

data LL = LNil | LCons Integer LL
    deriving (Eq, FromJSON)

-- Mutual recursion: Fwd's decoder references Back's, declared later.
data Fwd = Fwd { fb :: Maybe Back }
    deriving (Eq, FromJSON)

data Back = Back { bn :: Integer, bf :: Maybe Fwd }
    deriving (Eq, FromJSON)

-- A raw Json field decodes as-is.
data Doc = Doc { title :: String, body :: Json }
    deriving (Eq, FromJSON)

-- Lists of Maybe and nested lists as field types.
data Grid = Grid { cells :: [[Integer]], marks :: [Maybe Integer] }
    deriving (Eq, FromJSON)

-- Decode helpers (one per type: Eq dispatch is monomorphic) -----------

decP :: String -> Either String Person
decP s = decodeJSON s

okP :: Either String Person -> Person -> Bool
okP (Right a) b = a == b
okP (Left _) _ = False

decSh :: String -> Either String Shape
decSh s = decodeJSON s

okSh :: Either String Shape -> Shape -> Bool
okSh (Right a) b = a == b
okSh (Left _) _ = False

decC :: String -> Either String Color
decC s = decodeJSON s

okC :: Either String Color -> Color -> Bool
okC (Right a) b = a == b
okC (Left _) _ = False

decW :: String -> Either String Wrap
decW s = decodeJSON s

okW :: Either String Wrap -> Wrap -> Bool
okW (Right a) b = a == b
okW (Left _) _ = False

decP2 :: String -> Either String Pair2
decP2 s = decodeJSON s

okP2 :: Either String Pair2 -> Pair2 -> Bool
okP2 (Right a) b = a == b
okP2 (Left _) _ = False

decOW :: String -> Either String OptW
decOW s = decodeJSON s

okOW :: Either String OptW -> OptW -> Bool
okOW (Right a) b = a == b
okOW (Left _) _ = False

decU :: String -> Either String Unit1
decU s = decodeJSON s

okU :: Either String Unit1 -> Unit1 -> Bool
okU (Right a) b = a == b
okU (Left _) _ = False

decT :: String -> Either String Team
decT s = decodeJSON s

okT :: Either String Team -> Team -> Bool
okT (Right a) b = a == b
okT (Left _) _ = False

decO :: String -> Either String Org
decO s = decodeJSON s

okO :: Either String Org -> Org -> Bool
okO (Right a) b = a == b
okO (Left _) _ = False

decJT :: String -> Either String JTree
decJT s = decodeJSON s

okJT :: Either String JTree -> JTree -> Bool
okJT (Right a) b = a == b
okJT (Left _) _ = False

decL :: String -> Either String LL
decL s = decodeJSON s

okL :: Either String LL -> LL -> Bool
okL (Right a) b = a == b
okL (Left _) _ = False

decF :: String -> Either String Fwd
decF s = decodeJSON s

okF :: Either String Fwd -> Fwd -> Bool
okF (Right a) b = a == b
okF (Left _) _ = False

decD :: String -> Either String Doc
decD s = decodeJSON s

okD :: Either String Doc -> Doc -> Bool
okD (Right a) b = a == b
okD (Left _) _ = False

decG :: String -> Either String Grid
decG s = decodeJSON s

okG :: Either String Grid -> Grid -> Bool
okG (Right a) b = a == b
okG (Left _) _ = False

-- Generic: the decode failed with exactly this message.
errIs :: Either String a -> String -> Bool
errIs (Left e) m = e == m
errIs (Right _) _ = False

mustParse :: String -> Json
mustParse s = case parseJSON s of
    Left _ -> JNull
    Right v -> v

-- Full int64 boundary decoding needs Lua's integer number subtype (5.3+),
-- probed via hasIntegerSubtype.
-- note: LuaJIT has no integer subtype — every number is an IEEE-754 double,
-- so integers beyond 2^53 are not exactly representable there and the strict
-- 64-bit range check correctly rejects the int64 boundaries. On such
-- interpreters we assert exact decoding at the double-safe boundary (±2^53)
-- instead; on Lua 5.3+ the full int64 asserts always run.
int64DecodeChecks :: Bool -> IO ()
int64DecodeChecks True = do
    assert (okP (decP "{\"name\":\"x\",\"age\":9007199254740993}") (Person "x" 9007199254740993)) "integer beyond 2^53 exact"
    assert (okP (decP "{\"name\":\"x\",\"age\":9223372036854775807}") (Person "x" 9223372036854775807)) "int64 max"
    assert (okP (decP "{\"name\":\"x\",\"age\":-9223372036854775808}") (Person "x" (0 - 9223372036854775807 - 1))) "int64 min exact"
int64DecodeChecks False = do
    putStrLn "note: no integer subtype (LuaJIT) - asserting the double-safe 2^53 boundary instead of the full int64 range"
    assert (okP (decP "{\"name\":\"x\",\"age\":9007199254740992}") (Person "x" 9007199254740992)) "2^53 decodes exactly (no integer subtype)"
    assert (okP (decP "{\"name\":\"x\",\"age\":-9007199254740992}") (Person "x" (-9007199254740992))) "-2^53 decodes exactly (no integer subtype)"

main :: IO ()
main = do
    -- ============================================================
    -- Records
    -- ============================================================
    assert (okP (decP "{\"name\":\"Ann\",\"age\":30}") (Person "Ann" 30)) "record decode"
    assert (okP (decP "{\"age\":30,\"name\":\"Ann\"}") (Person "Ann" 30)) "field order irrelevant"
    assert (okP (decP "{\"name\":\"Ann\",\"age\":30,\"extra\":[1,2]}") (Person "Ann" 30)) "unknown keys ignored"
    assert (okP (decP "  { \"name\" : \"Ann\" , \"age\" : 30 }  ") (Person "Ann" 30)) "whitespace tolerated"
    assert (okP (decP "{\"name\":\"\\u00e9\",\"age\":-3}") (Person (strChar 195 <> strChar 169) (-3))) "escapes and negatives"
    int64DecodeChecks hasIntegerSubtype
    case decP "{\"name\":\"x\",\"age\":-9223372036854775809}" of
        Left _ -> assert True "int64 min-1 rejected"
        Right _ -> assert False "int64 min-1 rejected"
    case decP "{\"name\":\"x\",\"age\":9223372036854775808}" of
        Left _ -> assert True "int64 max+1 rejected"
        Right _ -> assert False "int64 max+1 rejected"

    -- ascription form (read-style return-type dispatch) and fromJSON direct
    case (decodeJSON "{\"name\":\"Bo\",\"age\":7}" :: Either String Person) of
        Right p -> assert (p == Person "Bo" 7) "decodeJSON with ascription"
        Left _ -> assert False "decodeJSON with ascription"
    case (fromJSON (mustParse "{\"name\":\"Cy\",\"age\":8}") :: Either String Person) of
        Right p -> assert (p == Person "Cy" 8) "fromJSON with ascription"
        Left _ -> assert False "fromJSON with ascription"

    -- record error paths
    assert (errIs (decP "{\"name\":\"Ann\"}")
        "while decoding Person: the required field 'age' is missing") "missing field message"
    assert (errIs (decP "{\"name\":\"Ann\",\"age\":\"old\"}")
        "while decoding Person: in field 'age': expected an integer, but found a string") "wrong field type message"
    assert (errIs (decP "[1,2]")
        "while decoding Person: expected an object with a field 'name', but found an array") "non-object message"
    assert (errIs (decP "{\"name\":\"Ann\",\"age\":3.5}")
        "while decoding Person: in field 'age': expected an integer, but found the non-integral number 3.5") "integrality enforced"
    assert (errIs (decP "{\"name\":\"Ann\",\"age\":1e300}")
        "while decoding Person: in field 'age': the number 1e+300 is outside the 64-bit Integer range") "int64 range enforced"
    assert (errIs (decP "{\"name\":\"Ann\",\"age\":30} x")
        "Unexpected character 'x' at position 25: the input continues after the end of the JSON value") "trailing garbage rejected"

    -- ============================================================
    -- Sum types
    -- ============================================================
    assert (okSh (decSh "{\"tag\":\"Circle\",\"radius\":2.5}") (Circle 2.5)) "record constructor with tag"
    assert (okSh (decSh "{\"tag\":\"Rect\",\"contents\":[3,4]}") (Rect 3 4)) "positional contents array"
    assert (okSh (decSh "{\"tag\":\"Point0\"}") Point0) "nullary tagged object"
    assert (okSh (decSh "\"Point0\"") Point0) "nullary bare string"
    assert (errIs (decSh "{\"tag\":\"Blob\"}")
        "while decoding Shape: the tag 'Blob' names no constructor of this type: expected 'Circle', 'Rect' or 'Point0'") "unknown tag message"
    assert (errIs (decSh "\"Rect\"")
        "while decoding Shape: the constructor 'Rect' has fields, so a bare string cannot encode it: expected an object with \"tag\":\"Rect\"") "bare string for non-nullary"
    assert (errIs (decSh "{\"radius\":1.5}")
        "while decoding Shape: the required field 'tag' is missing") "missing tag message"
    assert (errIs (decSh "{\"tag\":\"Rect\",\"contents\":[3]}")
        "while decoding Shape: constructor 'Rect' takes 2 arguments, but the array has 1") "contents arity message"
    assert (errIs (decSh "{\"tag\":\"Rect\"}")
        "while decoding Shape: the required field 'contents' is missing") "missing contents message"
    assert (errIs (decSh "{\"tag\":\"Rect\",\"contents\":[3,\"y\"]}")
        "while decoding Shape: in argument 2 of constructor 'Rect': expected an integer, but found a string") "bad argument message"
    assert (errIs (decSh "true")
        "while decoding Shape: expected an object with a \"tag\" field naming the constructor (or a bare constructor-name string), but found a boolean") "untaggable value message"
    assert (errIs (decSh "{\"tag\":42}")
        "while decoding Shape: in field 'tag': expected a string, but found a number") "non-string tag message"

    -- all-nullary enum-like sums
    assert (okC (decC "\"RedC\"") RedC) "bare nullary RedC"
    assert (okC (decC "\"GreenC\"") GreenC) "bare nullary GreenC"
    assert (okC (decC "{\"tag\":\"BlueC\"}") BlueC) "tagged nullary BlueC"
    assert (errIs (decC "\"redc\"")
        "while decoding Color: the tag 'redc' names no constructor of this type: expected 'RedC', 'GreenC' or 'BlueC'") "case-sensitive tags"

    -- ============================================================
    -- Single positional constructors (untagged)
    -- ============================================================
    assert (okW (decW "7") (Wrap 7)) "single argument is the value itself"
    assert (errIs (decW "\"7\"")
        "while decoding Wrap: in argument 1 of constructor 'Wrap': expected an integer, but found a string") "single argument message"
    assert (okP2 (decP2 "[7,\"a\"]") (Pair2 7 "a")) "two arguments as array"
    assert (errIs (decP2 "[7]")
        "while decoding Pair2: constructor 'Pair2' takes 2 arguments, but the array has 1") "array arity message"
    assert (errIs (decP2 "{\"a\":1}")
        "while decoding Pair2: in the arguments of constructor 'Pair2': expected an array, but found an object") "array shape message"
    assert (okOW (decOW "5") (OptW (Just 5))) "positional Maybe just"
    assert (okOW (decOW "null") (OptW Nothing)) "positional Maybe null"
    assert (okU (decU "\"Unit1\"") Unit1) "lone nullary bare string"
    assert (okU (decU "{\"tag\":\"Unit1\"}") Unit1) "lone nullary tagged"

    -- ============================================================
    -- Nested user types, lists, Maybe fields
    -- ============================================================
    assert (okT (decT "{\"leader\":{\"name\":\"A\",\"age\":1},\"members\":[{\"name\":\"B\",\"age\":2},{\"name\":\"C\",\"age\":3}],\"motto\":\"go\"}")
        (Team (Person "A" 1) [Person "B" 2, Person "C" 3] (Just "go"))) "nested decode"
    assert (okT (decT "{\"leader\":{\"name\":\"A\",\"age\":1},\"members\":[]}")
        (Team (Person "A" 1) [] Nothing)) "Maybe field absent -> Nothing"
    assert (okT (decT "{\"leader\":{\"name\":\"A\",\"age\":1},\"members\":[],\"motto\":null}")
        (Team (Person "A" 1) [] Nothing)) "Maybe field null -> Nothing"
    assert (errIs (decT "{\"leader\":{\"name\":\"A\",\"age\":1},\"members\":[{\"name\":\"B\",\"age\":2},{\"name\":\"C\"}]}")
        "while decoding Team: in field 'members': at array index 1: while decoding Person: the required field 'age' is missing") "nested error path names everything"
    assert (errIs (decT "{\"leader\":{\"name\":\"A\",\"age\":1},\"members\":[],\"motto\":9}")
        "while decoding Team: in field 'motto': expected a string, but found a number") "present-but-wrong Maybe field is an error"

    -- deeply nested structure
    assert (okO (decO "{\"teams\":[{\"leader\":{\"name\":\"A\",\"age\":1},\"members\":[{\"name\":\"B\",\"age\":2}],\"motto\":\"m\"},{\"leader\":{\"name\":\"C\",\"age\":3},\"members\":[]}]}")
        (Org [Team (Person "A" 1) [Person "B" 2] (Just "m"), Team (Person "C" 3) [] Nothing])) "deep nesting"

    -- list-of-list and list-of-Maybe fields
    assert (okG (decG "{\"cells\":[[1,2],[],[3]],\"marks\":[1,null,3]}")
        (Grid [[1, 2], [], [3]] [Just 1, Nothing, Just 3])) "[[Integer]] and [Maybe Integer]"
    assert (errIs (decG "{\"cells\":[[1,\"x\"]],\"marks\":[]}")
        "while decoding Grid: in field 'cells': at array index 0: at array index 1: expected an integer, but found a string") "nested list error path"

    -- ============================================================
    -- Recursive and mutually recursive types
    -- ============================================================
    assert (okJT (decJT "{\"tag\":\"JNode\",\"contents\":[{\"tag\":\"JLeaf\",\"contents\":1},{\"tag\":\"JNode\",\"contents\":[{\"tag\":\"JLeaf\",\"contents\":2}]}]}")
        (JNode [JLeaf 1, JNode [JLeaf 2]])) "recursive tree"
    assert (okJT (decJT "{\"tag\":\"JNode\",\"contents\":[]}") (JNode [])) "empty node"
    assert (okL (decL "{\"tag\":\"LCons\",\"contents\":[1,{\"tag\":\"LCons\",\"contents\":[2,\"LNil\"]}]}")
        (LCons 1 (LCons 2 LNil))) "recursive list, nullary tail as bare string"
    assert (okL (decL "\"LNil\"") LNil) "recursive list base case"
    assert (okF (decF "{\"fb\":{\"bn\":9,\"bf\":{\"fb\":null}}}")
        (Fwd (Just (Back 9 (Just (Fwd Nothing)))))) "mutual recursion"

    -- ============================================================
    -- Json passthrough fields
    -- ============================================================
    assert (okD (decD "{\"title\":\"t\",\"body\":{\"any\":[1,true,null]}}")
        (Doc "t" (mustParse "{\"any\":[1,true,null]}"))) "Json field kept verbatim"
    assert (errIs (decD "{\"title\":\"t\"}")
        "while decoding Doc: the required field 'body' is missing") "Json field still required"

    -- malformed JSON surfaces the parser's error, not a decoder error
    case decP "{\"name\":" of
        Left e -> assert (e == "Unexpected end of input") "parse errors pass through"
        Right _ -> assert False "parse errors pass through"
