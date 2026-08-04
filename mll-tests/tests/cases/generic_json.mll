-- genericToJSON: the JSON module's generic encoder over the Generic
-- representation must reproduce the native deriving (ToJSON) wire format
-- BYTE-EXACT. Every type derives both, and each shape asserts
-- encodeJSON (genericToJSON x) == encodeToJSON x (the native output), plus
-- exact strings for the shape-defining cases so a bug shared by both
-- encoders cannot hide behind the comparison. Covers: records, tagged sums
-- (record/positional/nullary constructors), all-nullary enums, single
-- positional constructors, Maybe (null) fields, nesting, lists, recursion,
-- and `as`-renamed fields and constructors.
--
-- The zoo stays UNDER the monomorphiser's 16-specialisation guard (the
-- generic gSum/gFields instance functions are specialised once per
-- CONSTRUCTOR / FIELD across all types), so this test pins the fully
-- specialised path; generic_json_many is the over-the-guard twin that pins
-- the dictionary-passing fallback.
import JSON

data Person = Person { name :: String, age :: Int }
    deriving (Eq, ToJSON, Generic)

data Shape = Circle { radius :: Number } | Rect Int Int | Point0
    deriving (Eq, ToJSON, Generic)

data Color = RedC | GreenC | BlueC
    deriving (Eq, ToJSON, Generic)

data Wrap = Wrap Int
    deriving (Eq, ToJSON, Generic)

data OptW = OptW (Maybe Int)
    deriving (Eq, ToJSON, Generic)

data Team = Team { leader :: Person, members :: [Person], motto :: Maybe String }
    deriving (Eq, ToJSON, Generic)

data LL = LNil | LCons Int LL
    deriving (Eq, ToJSON, Generic)

data Acct = Acct { acctName as "name" :: String, acctScore :: Int, acctNote as "note" :: Maybe String }
    deriving (Eq, ToJSON, Generic)

data Ev = Tick { evAt as "at" :: Int } | Stop
    deriving (Eq, ToJSON, Generic)

-- The generic encoding, as a string.
gEnc :: (Generic a, GEncode (Rep a)) => a -> String
gEnc x = encodeJSON (genericToJSON x)

-- Generic and native agree byte-for-byte.
agrees :: (Generic a, GEncode (Rep a), ToJSON a) => a -> Bool
agrees x = gEnc x == encodeToJSON x

main :: IO ()
main = do
    -- exact strings: the shape-defining cases
    assert (gEnc (Person "Ann" 30) == "{\"name\":\"Ann\",\"age\":30}") "record object"
    assert (gEnc (Circle 2.5) == "{\"tag\":\"Circle\",\"radius\":2.5}") "tagged record con"
    assert (gEnc (Rect 3 4) == "{\"tag\":\"Rect\",\"contents\":[3,4]}") "tagged positional contents"
    assert (gEnc Point0 == "\"Point0\"") "tagged nullary bare string"
    assert (gEnc RedC == "\"RedC\"") "all-nullary bare string"
    assert (gEnc (Wrap 7) == "7") "single positional is the value"
    assert (gEnc (OptW Nothing) == "null") "positional Maybe Nothing"
    assert (gEnc (Acct "zoe" 9 (Just "hi")) == "{\"name\":\"zoe\",\"acctScore\":9,\"note\":\"hi\"}") "renamed field keys"
    assert (gEnc (Tick 12) == "{\"tag\":\"Tick\",\"at\":12}") "renamed field in tagged sum"

    -- generic == native across the zoo
    assert (agrees (Person "Ann" 30)) "agree record"
    assert (agrees (Person "a\"b\nc" 1)) "agree escaped string"
    assert (agrees (Circle 2.5)) "agree sum record con"
    assert (agrees (Rect 3 4)) "agree sum positional con"
    assert (agrees Point0) "agree sum nullary con"
    assert (agrees RedC) "agree enum first"
    assert (agrees BlueC) "agree enum last"
    assert (agrees (Wrap 7)) "agree single positional"
    assert (agrees (OptW (Just 5))) "agree Maybe just"
    assert (agrees (OptW Nothing)) "agree Maybe nothing"
    assert (agrees (Team (Person "A" 1) [Person "B" 2, Person "C" 3] (Just "go"))) "agree nested"
    assert (agrees (Team (Person "A" 1) [] Nothing)) "agree nested empty/Nothing"
    assert (agrees (LCons 1 (LCons 2 LNil))) "agree recursive list"
    assert (agrees LNil) "agree recursive list nil"
    assert (agrees (Acct "zoe" 9 Nothing)) "agree renamed Maybe Nothing"
    assert (agrees (Tick 12)) "agree renamed in sum"
    assert (agrees Stop) "agree nullary next to renamed"

    putStrLn "generic_json ok"
