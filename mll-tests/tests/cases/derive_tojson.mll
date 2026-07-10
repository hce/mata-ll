-- deriving (ToJSON): the native derive generates an encoder over the JSON
-- module's combinators as the exact mirror of deriving (FromJSON), so that
-- fromJSON (parseJSON (encodeToJSON x)) == Right x round-trips. Covers:
-- records, sum types (tagged objects and bare nullary strings), single
-- positional constructors, nested user types, [a] and Maybe a fields
-- (Nothing encodes as null), recursive and mutually recursive types, Json
-- passthrough fields, int64 boundaries, unicode, and `as`-renamed fields —
-- the encoded JSON uses the RENAMED key, asserted against exact strings so
-- a symmetric encode/decode bug cannot hide behind a passing round-trip.
import JSON

-- Single-constructor record: encodes to an object keyed by field name.
data Person = Person { name :: String, age :: Integer }
    deriving (Eq, ToJSON, FromJSON)

-- Multi-constructor sum: record constructor (fields inline next to the tag),
-- positional constructor (arguments under "contents"), nullary constructor
-- (bare string).
data Shape = Circle { radius :: Number } | Rect Integer Integer | Point0
    deriving (Eq, ToJSON, FromJSON)

-- All-nullary sum: encodes to bare constructor-name strings.
data Color = RedC | GreenC | BlueC
    deriving (Eq, ToJSON, FromJSON)

-- Single positional constructors: one argument is the value itself,
-- several are an array.
data Wrap = Wrap Integer
    deriving (Eq, ToJSON, FromJSON)

data Pair2 = Pair2 Integer String
    deriving (Eq, ToJSON, FromJSON)

-- Single positional Maybe: Nothing <-> null at the top level.
data OptW = OptW (Maybe Integer)
    deriving (Eq, ToJSON, FromJSON)

-- Lone nullary constructor: the constructor name is the payload.
data Unit1 = Unit1
    deriving (Eq, ToJSON, FromJSON)

-- Nested user types, list of user type, optional field.
data Team = Team { leader :: Person, members :: [Person], motto :: Maybe String }
    deriving (Eq, ToJSON, FromJSON)

-- Recursive types.
data JTree = JLeaf Integer | JNode [JTree]
    deriving (Eq, ToJSON, FromJSON)

data LL = LNil | LCons Integer LL
    deriving (Eq, ToJSON, FromJSON)

-- Mutual recursion: Fwd's encoder references Back's, declared later
-- (exercises the tojson_types prescan).
data Fwd = Fwd { fb :: Maybe Back }
    deriving (Eq, ToJSON, FromJSON)

data Back = Back { bn :: Integer, bf :: Maybe Fwd }
    deriving (Eq, ToJSON, FromJSON)

-- A raw Json field encodes as-is.
data Doc = Doc { title :: String, body :: Json }
    deriving (Eq, ToJSON, FromJSON)

-- Lists of Maybe and nested lists as field types.
data Grid = Grid { cells :: [[Integer]], marks :: [Maybe Integer] }
    deriving (Eq, ToJSON, FromJSON)

-- `as`-renamed fields: the JSON object key is the RENAMED key in both
-- directions; the Haskell accessor keeps the field name. The renamed Maybe
-- field exercises the optional-field decode path under the renamed key.
data Acct = Acct { acctName as "name" :: String, acctScore :: Integer, acctNote as "note" :: Maybe String }
    deriving (Eq, ToJSON, FromJSON)

-- A renamed field in a tagged sum: the rename applies inline in the
-- tagged object.
data Ev = Tick { evAt as "at" :: Integer } | Stop
    deriving (Eq, ToJSON, FromJSON)

-- Round-trip helpers (one per type: Eq dispatch is monomorphic) ---------

rtP :: Person -> Bool
rtP x = case decodeJSON (encodeToJSON x) of
    Right y -> y == x
    Left _ -> False

rtSh :: Shape -> Bool
rtSh x = case decodeJSON (encodeToJSON x) of
    Right y -> y == x
    Left _ -> False

rtC :: Color -> Bool
rtC x = case decodeJSON (encodeToJSON x) of
    Right y -> y == x
    Left _ -> False

rtW :: Wrap -> Bool
rtW x = case decodeJSON (encodeToJSON x) of
    Right y -> y == x
    Left _ -> False

rtP2 :: Pair2 -> Bool
rtP2 x = case decodeJSON (encodeToJSON x) of
    Right y -> y == x
    Left _ -> False

rtOW :: OptW -> Bool
rtOW x = case decodeJSON (encodeToJSON x) of
    Right y -> y == x
    Left _ -> False

rtU :: Unit1 -> Bool
rtU x = case decodeJSON (encodeToJSON x) of
    Right y -> y == x
    Left _ -> False

rtT :: Team -> Bool
rtT x = case decodeJSON (encodeToJSON x) of
    Right y -> y == x
    Left _ -> False

rtJT :: JTree -> Bool
rtJT x = case decodeJSON (encodeToJSON x) of
    Right y -> y == x
    Left _ -> False

rtL :: LL -> Bool
rtL x = case decodeJSON (encodeToJSON x) of
    Right y -> y == x
    Left _ -> False

rtF :: Fwd -> Bool
rtF x = case decodeJSON (encodeToJSON x) of
    Right y -> y == x
    Left _ -> False

rtD :: Doc -> Bool
rtD x = case decodeJSON (encodeToJSON x) of
    Right y -> y == x
    Left _ -> False

rtG :: Grid -> Bool
rtG x = case decodeJSON (encodeToJSON x) of
    Right y -> y == x
    Left _ -> False

rtA :: Acct -> Bool
rtA x = case decodeJSON (encodeToJSON x) of
    Right y -> y == x
    Left _ -> False

rtE :: Ev -> Bool
rtE x = case decodeJSON (encodeToJSON x) of
    Right y -> y == x
    Left _ -> False

mustParse :: String -> Json
mustParse s = case parseJSON s of
    Left _ -> JNull
    Right v -> v

main :: IO ()
main = do
    -- ============================================================
    -- Exact encoded strings: records
    -- ============================================================
    assert (encodeToJSON (Person "Ann" 30) == "{\"name\":\"Ann\",\"age\":30}") "record encode"
    assert (encodeToJSON (Person "x" 9223372036854775807) == "{\"name\":\"x\",\"age\":9223372036854775807}") "int64 max encodes exactly"
    assert (encodeToJSON (Person "x" (0 - 9223372036854775807 - 1)) == "{\"name\":\"x\",\"age\":-9223372036854775808}") "int64 min encodes exactly"
    assert (encodeToJSON (Person "x" 9007199254740993) == "{\"name\":\"x\",\"age\":9007199254740993}") "integer beyond 2^53 exact"
    -- unicode: UTF-8 text passes through byte-for-byte; control characters
    -- and quotes are escaped
    assert (encodeToJSON (Person (strChar 195 <> strChar 169) 1) == "{\"name\":\"" <> strChar 195 <> strChar 169 <> "\",\"age\":1}") "utf-8 passes through"
    assert (encodeToJSON (Person "a\"b\nc" 1) == "{\"name\":\"a\\\"b\\nc\",\"age\":1}") "quote and newline escaped"

    -- toJSON alone (without the encodeJSON step) dispatches too
    assert (encodeJSON (toJSON (Person "Bo" 7)) == "{\"name\":\"Bo\",\"age\":7}") "toJSON direct dispatch"

    -- ============================================================
    -- Exact encoded strings: sums
    -- ============================================================
    assert (encodeToJSON (Circle 2.5) == "{\"tag\":\"Circle\",\"radius\":2.5}") "record constructor with tag"
    assert (encodeToJSON (Rect 3 4) == "{\"tag\":\"Rect\",\"contents\":[3,4]}") "positional contents array"
    assert (encodeToJSON Point0 == "\"Point0\"") "nullary bare string"
    assert (encodeToJSON RedC == "\"RedC\"") "all-nullary bare string"
    assert (encodeToJSON BlueC == "\"BlueC\"") "all-nullary bare string 2"

    -- ============================================================
    -- Exact encoded strings: single positional constructors (untagged)
    -- ============================================================
    assert (encodeToJSON (Wrap 7) == "7") "single argument is the value itself"
    assert (encodeToJSON (Pair2 7 "a") == "[7,\"a\"]") "two arguments as array"
    assert (encodeToJSON (OptW (Just 5)) == "5") "positional Maybe just"
    assert (encodeToJSON (OptW Nothing) == "null") "positional Maybe null"
    assert (encodeToJSON Unit1 == "\"Unit1\"") "lone nullary bare string"

    -- ============================================================
    -- Exact encoded strings: nesting, lists, Maybe fields
    -- ============================================================
    assert (encodeToJSON (Team (Person "A" 1) [Person "B" 2, Person "C" 3] (Just "go"))
        == "{\"leader\":{\"name\":\"A\",\"age\":1},\"members\":[{\"name\":\"B\",\"age\":2},{\"name\":\"C\",\"age\":3}],\"motto\":\"go\"}") "nested encode"
    assert (encodeToJSON (Team (Person "A" 1) [] Nothing)
        == "{\"leader\":{\"name\":\"A\",\"age\":1},\"members\":[],\"motto\":null}") "Maybe field Nothing -> null"
    assert (encodeToJSON (JNode [JLeaf 1, JNode [JLeaf 2]])
        == "{\"tag\":\"JNode\",\"contents\":[{\"tag\":\"JLeaf\",\"contents\":1},{\"tag\":\"JNode\",\"contents\":[{\"tag\":\"JLeaf\",\"contents\":2}]}]}") "recursive tree encode"
    assert (encodeToJSON (LCons 1 (LCons 2 LNil))
        == "{\"tag\":\"LCons\",\"contents\":[1,{\"tag\":\"LCons\",\"contents\":[2,\"LNil\"]}]}") "recursive list, nullary tail as bare string"
    assert (encodeToJSON (Fwd (Just (Back 9 (Just (Fwd Nothing)))))
        == "{\"fb\":{\"bn\":9,\"bf\":{\"fb\":null}}}") "mutual recursion encode"
    assert (encodeToJSON (Doc "t" (mustParse "{\"any\":[1,true,null]}"))
        == "{\"title\":\"t\",\"body\":{\"any\":[1,true,null]}}") "Json field emitted verbatim"
    assert (encodeToJSON (Grid [[1, 2], [], [3]] [Just 1, Nothing, Just 3])
        == "{\"cells\":[[1,2],[],[3]],\"marks\":[1,null,3]}") "[[Integer]] and [Maybe Integer]"

    -- ============================================================
    -- `as`-renamed fields: the encoded JSON uses the RENAMED keys
    -- (exact string, so the Haskell name cannot leak in)
    -- ============================================================
    assert (encodeToJSON (Acct "zoe" 9 (Just "hi")) == "{\"name\":\"zoe\",\"acctScore\":9,\"note\":\"hi\"}") "renamed key in encoded record"
    assert (encodeToJSON (Acct "zoe" 9 Nothing) == "{\"name\":\"zoe\",\"acctScore\":9,\"note\":null}") "renamed Maybe field Nothing -> null"
    assert (encodeToJSON (Tick 12) == "{\"tag\":\"Tick\",\"at\":12}") "renamed key inline in tagged object"
    assert (encodeToJSON Stop == "\"Stop\"") "nullary next to renamed constructor"
    -- ...and the decoder reads the renamed keys back (both directions renamed)
    case (decodeJSON "{\"name\":\"kim\",\"acctScore\":3,\"note\":\"n\"}" :: Either String Acct) of
        Right a -> assert (a == Acct "kim" 3 (Just "n")) "renamed key decodes"
        Left _ -> assert False "renamed key decodes"
    -- a renamed OPTIONAL field may be missing entirely
    case (decodeJSON "{\"name\":\"kim\",\"acctScore\":3}" :: Either String Acct) of
        Right a -> assert (a == Acct "kim" 3 Nothing) "renamed Maybe field absent -> Nothing"
        Left _ -> assert False "renamed Maybe field absent -> Nothing"
    -- the Haskell field name is NOT a JSON key anymore
    case (decodeJSON "{\"acctName\":\"kim\",\"acctScore\":3}" :: Either String Acct) of
        Left e -> assert (e == "while decoding Acct: the required field 'name' is missing") "Haskell name rejected, message names the JSON key"
        Right _ -> assert False "Haskell name rejected"
    case (decodeJSON "{\"tag\":\"Tick\",\"at\":12}" :: Either String Ev) of
        Right e -> assert (e == Tick 12) "renamed key decodes in tagged object"
        Left _ -> assert False "renamed key decodes in tagged object"

    -- ============================================================
    -- Round-trips: fromJSON (parseJSON (encodeToJSON x)) == Right x
    -- ============================================================
    assert (rtP (Person "Ann" 30)) "round-trip record"
    assert (rtP (Person (strChar 195 <> strChar 169) (0 - 9223372036854775807 - 1))) "round-trip unicode + int64 min"
    assert (rtP (Person "x" 9223372036854775807)) "round-trip int64 max"
    assert (rtSh (Circle 2.5)) "round-trip record constructor"
    assert (rtSh (Rect 3 4)) "round-trip positional constructor"
    assert (rtSh Point0) "round-trip nullary constructor"
    assert (rtC RedC) "round-trip all-nullary 1"
    assert (rtC GreenC) "round-trip all-nullary 2"
    assert (rtC BlueC) "round-trip all-nullary 3"
    assert (rtW (Wrap 7)) "round-trip single positional"
    assert (rtP2 (Pair2 7 "a")) "round-trip two positional"
    assert (rtOW (OptW (Just 5))) "round-trip positional Maybe just"
    assert (rtOW (OptW Nothing)) "round-trip positional Maybe nothing"
    assert (rtU Unit1) "round-trip lone nullary"
    assert (rtT (Team (Person "A" 1) [Person "B" 2, Person "C" 3] (Just "go"))) "round-trip nested"
    assert (rtT (Team (Person "A" 1) [] Nothing)) "round-trip Maybe Nothing"
    assert (rtJT (JNode [JLeaf 1, JNode [JLeaf 2], JNode []])) "round-trip recursive tree"
    assert (rtL (LCons 1 (LCons 2 LNil))) "round-trip recursive list"
    assert (rtL LNil) "round-trip recursive list base"
    assert (rtF (Fwd (Just (Back 9 (Just (Fwd Nothing)))))) "round-trip mutual recursion"
    assert (rtF (Fwd Nothing)) "round-trip mutual recursion base"
    assert (rtD (Doc "t" (mustParse "{\"any\":[1,true,null]}"))) "round-trip Json passthrough"
    assert (rtG (Grid [[1, 2], [], [3]] [Just 1, Nothing, Just 3])) "round-trip nested lists"
    assert (rtA (Acct "zoe" 9 (Just "hi"))) "round-trip renamed fields"
    assert (rtA (Acct "zoe" 9 Nothing)) "round-trip renamed Maybe Nothing"
    assert (rtE (Tick 12)) "round-trip renamed field in sum"
    assert (rtE Stop) "round-trip nullary in renamed sum"
