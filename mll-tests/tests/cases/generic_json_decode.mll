-- genericFromJSON: the generic decoder over the Generic representation must
-- reproduce the native deriving (FromJSON) decoder EXACTLY — accepted
-- forms, decoded values, and error messages byte-for-byte. Every type
-- derives both; each input asserts the generic and native decoders agree
-- (Right == Right or the same Left message), and the shape-defining error
-- messages are additionally pinned as absolute strings so a shared bug
-- cannot hide behind the agreement. The zoo's 21 constructors also push the
-- generic C1/S1 instances past the 16-specialisation cap, so the
-- dictionary-passing fallback is exercised on the decode path too.
-- Full generic round-trips (genericFromJSON . genericToJSON == Right) close
-- the loop.
import JSON

data Person = Person { name :: String, age :: Int }
    deriving (Eq, ToJSON, FromJSON, Generic)

data Shape = Circle { radius :: Number } | Rect Int Int | Point0
    deriving (Eq, ToJSON, FromJSON, Generic)

data Color = RedC | GreenC | BlueC
    deriving (Eq, ToJSON, FromJSON, Generic)

data Wrap = Wrap Int
    deriving (Eq, ToJSON, FromJSON, Generic)

data Pair2 = Pair2 Int String
    deriving (Eq, ToJSON, FromJSON, Generic)

data OptW = OptW (Maybe Int)
    deriving (Eq, ToJSON, FromJSON, Generic)

data Unit1 = Unit1
    deriving (Eq, ToJSON, FromJSON, Generic)

data Team = Team { leader :: Person, members :: [Person], motto :: Maybe String }
    deriving (Eq, ToJSON, FromJSON, Generic)

data Org = Org { teams :: [Team] }
    deriving (Eq, ToJSON, FromJSON, Generic)

data JTree = JLeaf Int | JNode [JTree]
    deriving (Eq, ToJSON, FromJSON, Generic)

data LL = LNil | LCons Int LL
    deriving (Eq, ToJSON, FromJSON, Generic)

data Fwd = Fwd { fb :: Maybe Back }
    deriving (Eq, ToJSON, FromJSON, Generic)

data Back = Back { bn :: Int, bf :: Maybe Fwd }
    deriving (Eq, ToJSON, FromJSON, Generic)

data Doc = Doc { title :: String, body :: Json }
    deriving (Eq, ToJSON, FromJSON, Generic)

data Grid = Grid { cells :: [[Int]], marks :: [Maybe Int] }
    deriving (Eq, ToJSON, FromJSON, Generic)

data Acct = Acct { acctName as "name" :: String, acctScore :: Int, acctNote as "note" :: Maybe String }
    deriving (Eq, ToJSON, FromJSON, Generic)

data Ev = Tick { evAt as "at" :: Int } | Stop
    deriving (Eq, ToJSON, FromJSON, Generic)

-- Parse, then decode generically (parse errors pass through, exactly as
-- decodeJSON's do).
gDec :: (Generic a, GDecode (Rep a)) => String -> Either String a
gDec s = case parseJSON s of
    Left e -> Left e
    Right j -> genericFromJSON j

eqE :: Eq a => Either String a -> Either String a -> Bool
eqE (Right a) (Right b) = a == b
eqE (Left a) (Left b) = a == b
eqE _ _ = False

errIs :: Either String a -> String -> Bool
errIs (Left e) m = e == m
errIs (Right _) _ = False

isRight :: Either String a -> Bool
isRight (Right _) = True
isRight (Left _) = False

-- Generic and native agree on this input (same value or same message).
agP :: String -> Bool
agP s = eqE (gDec s :: Either String Person) (decodeJSON s)

agSh :: String -> Bool
agSh s = eqE (gDec s :: Either String Shape) (decodeJSON s)

agC :: String -> Bool
agC s = eqE (gDec s :: Either String Color) (decodeJSON s)

agW :: String -> Bool
agW s = eqE (gDec s :: Either String Wrap) (decodeJSON s)

agP2 :: String -> Bool
agP2 s = eqE (gDec s :: Either String Pair2) (decodeJSON s)

agOW :: String -> Bool
agOW s = eqE (gDec s :: Either String OptW) (decodeJSON s)

agU :: String -> Bool
agU s = eqE (gDec s :: Either String Unit1) (decodeJSON s)

agT :: String -> Bool
agT s = eqE (gDec s :: Either String Team) (decodeJSON s)

agO :: String -> Bool
agO s = eqE (gDec s :: Either String Org) (decodeJSON s)

agJT :: String -> Bool
agJT s = eqE (gDec s :: Either String JTree) (decodeJSON s)

agL :: String -> Bool
agL s = eqE (gDec s :: Either String LL) (decodeJSON s)

agF :: String -> Bool
agF s = eqE (gDec s :: Either String Fwd) (decodeJSON s)

agD :: String -> Bool
agD s = eqE (gDec s :: Either String Doc) (decodeJSON s)

agG :: String -> Bool
agG s = eqE (gDec s :: Either String Grid) (decodeJSON s)

agA :: String -> Bool
agA s = eqE (gDec s :: Either String Acct) (decodeJSON s)

agE :: String -> Bool
agE s = eqE (gDec s :: Either String Ev) (decodeJSON s)

-- Full generic round-trip: encode generically, decode generically.
grtP :: Person -> Bool
grtP x = case gDec (encodeJSON (genericToJSON x)) of
    Right y -> y == x
    Left _ -> False

grtSh :: Shape -> Bool
grtSh x = case gDec (encodeJSON (genericToJSON x)) of
    Right y -> y == x
    Left _ -> False

grtT :: Team -> Bool
grtT x = case gDec (encodeJSON (genericToJSON x)) of
    Right y -> y == x
    Left _ -> False

grtL :: LL -> Bool
grtL x = case gDec (encodeJSON (genericToJSON x)) of
    Right y -> y == x
    Left _ -> False

grtA :: Acct -> Bool
grtA x = case gDec (encodeJSON (genericToJSON x)) of
    Right y -> y == x
    Left _ -> False

main :: IO ()
main = do
    -- ============================================================
    -- Records: values and every error path
    -- ============================================================
    assert (agP "{\"name\":\"Ann\",\"age\":30}") "record decode"
    assert (agP "{\"age\":30,\"name\":\"Ann\"}") "field order irrelevant"
    assert (agP "{\"name\":\"Ann\",\"age\":30,\"extra\":[1,2]}") "unknown keys ignored"
    assert (agP "{\"name\":\"Ann\"}") "missing field"
    assert (agP "{\"name\":\"Ann\",\"age\":\"old\"}") "wrong field type"
    assert (agP "[1,2]") "non-object"
    assert (agP "{\"name\":\"Ann\",\"age\":3.5}") "non-integral"
    assert (agP "{\"name\":\"Ann\",\"age\":1e300}") "int64 range"
    assert (agP "{\"name\":") "parse error passes through"
    -- pinned absolute messages (the native wording, byte-for-byte)
    assert (errIs (gDec "{\"name\":\"Ann\"}" :: Either String Person)
        "while decoding Person: the required field 'age' is missing") "missing field message"
    assert (errIs (gDec "[1,2]" :: Either String Person)
        "while decoding Person: expected an object with a field 'name', but found an array") "non-object message"
    assert (errIs (gDec "{\"name\":\"Ann\",\"age\":\"old\"}" :: Either String Person)
        "while decoding Person: in field 'age': expected an integer, but found a string") "wrong type message"

    -- ============================================================
    -- Sum types: tagged objects, bare strings, every error path
    -- ============================================================
    assert (agSh "{\"tag\":\"Circle\",\"radius\":2.5}") "record con with tag"
    assert (agSh "{\"tag\":\"Rect\",\"contents\":[3,4]}") "positional contents"
    assert (agSh "{\"tag\":\"Point0\"}") "nullary tagged"
    assert (agSh "\"Point0\"") "nullary bare string"
    assert (agSh "{\"tag\":\"Blob\"}") "unknown tag"
    assert (agSh "\"Rect\"") "bare string for fielded"
    assert (agSh "{\"radius\":1.5}") "missing tag"
    assert (agSh "{\"tag\":\"Rect\",\"contents\":[3]}") "contents arity"
    assert (agSh "{\"tag\":\"Rect\"}") "missing contents"
    assert (agSh "{\"tag\":\"Rect\",\"contents\":[3,\"y\"]}") "bad argument"
    assert (agSh "true") "untaggable value"
    assert (agSh "{\"tag\":42}") "non-string tag"
    assert (errIs (gDec "{\"tag\":\"Blob\"}" :: Either String Shape)
        "while decoding Shape: the tag 'Blob' names no constructor of this type: expected 'Circle', 'Rect' or 'Point0'") "unknown tag message"
    assert (errIs (gDec "\"Rect\"" :: Either String Shape)
        "while decoding Shape: the constructor 'Rect' has fields, so a bare string cannot encode it: expected an object with \"tag\":\"Rect\"") "bare fielded message"
    assert (errIs (gDec "{\"tag\":\"Rect\",\"contents\":[3]}" :: Either String Shape)
        "while decoding Shape: constructor 'Rect' takes 2 arguments, but the array has 1") "arity message"
    assert (errIs (gDec "{\"tag\":\"Rect\",\"contents\":[3,\"y\"]}" :: Either String Shape)
        "while decoding Shape: in argument 2 of constructor 'Rect': expected an integer, but found a string") "argument message"
    assert (errIs (gDec "true" :: Either String Shape)
        "while decoding Shape: expected an object with a \"tag\" field naming the constructor (or a bare constructor-name string), but found a boolean") "untaggable message"

    -- all-nullary sums
    assert (agC "\"RedC\"") "bare RedC"
    assert (agC "\"GreenC\"") "bare GreenC"
    assert (agC "{\"tag\":\"BlueC\"}") "tagged BlueC"
    assert (agC "\"redc\"") "case-sensitive tags"

    -- ============================================================
    -- Single positional constructors (untagged)
    -- ============================================================
    assert (agW "7") "single argument value"
    assert (agW "\"7\"") "single argument error"
    assert (agP2 "[7,\"a\"]") "two arguments array"
    assert (agP2 "[7]") "array arity"
    assert (agP2 "{\"a\":1}") "array shape"
    assert (agOW "5") "positional Maybe just"
    assert (agOW "null") "positional Maybe null"
    assert (agU "\"Unit1\"") "lone nullary bare"
    assert (agU "{\"tag\":\"Unit1\"}") "lone nullary tagged"
    assert (errIs (gDec "[7]" :: Either String Pair2)
        "while decoding Pair2: constructor 'Pair2' takes 2 arguments, but the array has 1") "untagged arity message"
    assert (errIs (gDec "{\"a\":1}" :: Either String Pair2)
        "while decoding Pair2: in the arguments of constructor 'Pair2': expected an array, but found an object") "untagged shape message"

    -- ============================================================
    -- Nesting, lists, Maybe fields
    -- ============================================================
    assert (agT "{\"leader\":{\"name\":\"A\",\"age\":1},\"members\":[{\"name\":\"B\",\"age\":2},{\"name\":\"C\",\"age\":3}],\"motto\":\"go\"}") "nested decode"
    assert (agT "{\"leader\":{\"name\":\"A\",\"age\":1},\"members\":[]}") "Maybe field absent"
    assert (agT "{\"leader\":{\"name\":\"A\",\"age\":1},\"members\":[],\"motto\":null}") "Maybe field null"
    assert (agT "{\"leader\":{\"name\":\"A\",\"age\":1},\"members\":[{\"name\":\"B\",\"age\":2},{\"name\":\"C\"}]}") "nested error path"
    assert (agT "{\"leader\":{\"name\":\"A\",\"age\":1},\"members\":[],\"motto\":9}") "wrong Maybe field"
    assert (errIs (gDec "{\"leader\":{\"name\":\"A\",\"age\":1},\"members\":[{\"name\":\"B\",\"age\":2},{\"name\":\"C\"}]}" :: Either String Team)
        "while decoding Team: in field 'members': at array index 1: while decoding Person: the required field 'age' is missing") "nested error message"
    assert (agO "{\"teams\":[{\"leader\":{\"name\":\"A\",\"age\":1},\"members\":[{\"name\":\"B\",\"age\":2}],\"motto\":\"m\"},{\"leader\":{\"name\":\"C\",\"age\":3},\"members\":[]}]}") "deep nesting"
    assert (agG "{\"cells\":[[1,2],[],[3]],\"marks\":[1,null,3]}") "[[Int]] and [Maybe Int]"
    assert (agG "{\"cells\":[[1,\"x\"]],\"marks\":[]}") "nested list error"

    -- ============================================================
    -- Recursive and mutually recursive types
    -- ============================================================
    assert (agJT "{\"tag\":\"JNode\",\"contents\":[{\"tag\":\"JLeaf\",\"contents\":1},{\"tag\":\"JNode\",\"contents\":[{\"tag\":\"JLeaf\",\"contents\":2}]}]}") "recursive tree"
    assert (agJT "{\"tag\":\"JNode\",\"contents\":[]}") "empty node"
    assert (agL "{\"tag\":\"LCons\",\"contents\":[1,{\"tag\":\"LCons\",\"contents\":[2,\"LNil\"]}]}") "recursive list"
    assert (agL "\"LNil\"") "recursive base case"
    assert (agF "{\"fb\":{\"bn\":9,\"bf\":{\"fb\":null}}}") "mutual recursion"

    -- ============================================================
    -- Json passthrough
    -- ============================================================
    assert (agD "{\"title\":\"t\",\"body\":{\"any\":[1,true,null]}}") "Json field verbatim"
    assert (agD "{\"title\":\"t\"}") "Json field required"

    -- ============================================================
    -- `as` renames: renamed keys and tags in both directions
    -- ============================================================
    assert (agA "{\"name\":\"kim\",\"acctScore\":3,\"note\":\"n\"}") "renamed keys decode"
    assert (agA "{\"name\":\"kim\",\"acctScore\":3}") "renamed Maybe absent"
    assert (agA "{\"acctName\":\"kim\",\"acctScore\":3}") "source name rejected"
    assert (errIs (gDec "{\"acctName\":\"kim\",\"acctScore\":3}" :: Either String Acct)
        "while decoding Acct: the required field 'name' is missing") "renamed key message"
    assert (agE "{\"tag\":\"Tick\",\"at\":12}") "renamed field in sum"
    assert (agE "\"Stop\"") "nullary next to renamed"

    -- ============================================================
    -- Full generic round-trips
    -- ============================================================
    assert (grtP (Person "Ann" 30)) "roundtrip record"
    assert (grtSh (Circle 2.5)) "roundtrip sum record con"
    assert (grtSh (Rect 3 4)) "roundtrip sum positional con"
    assert (grtSh Point0) "roundtrip sum nullary"
    assert (grtT (Team (Person "A" 1) [Person "B" 2] Nothing)) "roundtrip nested"
    assert (grtL (LCons 1 (LCons 2 LNil))) "roundtrip recursive"
    assert (grtA (Acct "zoe" 9 (Just "hi"))) "roundtrip renamed"

    putStrLn "generic_json_decode ok"
