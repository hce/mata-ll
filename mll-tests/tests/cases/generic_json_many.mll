-- genericToJSON BEYOND the specialisation cap: a generic function must keep
-- working however many types it is applied to. This zoo (17 types, 23
-- constructors, 28 fields) pushes every layer of the generic encoder past
-- the monomorphiser's 16-specialisation polymorphic-recursion guard —
-- gEnc/agrees per type, gEncode's D1 instance per type, gSum's C1 instance
-- per constructor, gFields' S1 instance per field — so all of them are
-- purged to the dictionary-passing fallback, which must resolve the
-- Generic/metadata classes correctly. Byte-exact agreement with the native
-- deriving (ToJSON) output is asserted for every shape, as in generic_json.
import JSON

data Person = Person { name :: String, age :: Int }
    deriving (Eq, ToJSON, Generic)

data Shape = Circle { radius :: Number } | Rect Int Int | Point0
    deriving (Eq, ToJSON, Generic)

data Color = RedC | GreenC | BlueC
    deriving (Eq, ToJSON, Generic)

data Wrap = Wrap Int
    deriving (Eq, ToJSON, Generic)

data Pair2 = Pair2 Int String
    deriving (Eq, ToJSON, Generic)

data OptW = OptW (Maybe Int)
    deriving (Eq, ToJSON, Generic)

data Unit1 = Unit1
    deriving (Eq, ToJSON, Generic)

data Team = Team { leader :: Person, members :: [Person], motto :: Maybe String }
    deriving (Eq, ToJSON, Generic)

data JTree = JLeaf Int | JNode [JTree]
    deriving (Eq, ToJSON, Generic)

data LL = LNil | LCons Int LL
    deriving (Eq, ToJSON, Generic)

data Fwd = Fwd { fb :: Maybe Back }
    deriving (Eq, ToJSON, Generic)

data Back = Back { bn :: Int, bf :: Maybe Fwd }
    deriving (Eq, ToJSON, Generic)

data Doc = Doc { title :: String, body :: Json }
    deriving (Eq, ToJSON, Generic)

data Grid = Grid { cells :: [[Int]], marks :: [Maybe Int] }
    deriving (Eq, ToJSON, Generic)

data Acct = Acct { acctName as "name" :: String, acctScore :: Int, acctNote as "note" :: Maybe String }
    deriving (Eq, ToJSON, Generic)

data Ev = Tick { evAt as "at" :: Int } | Stop
    deriving (Eq, ToJSON, Generic)

data Extra = Extra { ex :: Int }
    deriving (Eq, ToJSON, Generic)

-- A USER generic function over the same zoo: the constructor index. Its
-- GIx instances trip the same cap (one C1 specialisation per constructor)
-- and must survive the dictionary-passing fallback like the encoder's.
class GIx f where
    gix :: f -> Int

instance GIx U1 where
    gix _ = 0

instance GIx (K1 c) where
    gix _ = 0

instance (GIx a, GIx b) => GIx (a :+: b) where
    gix (L1 x) = gix x
    gix (R1 y) = 1 + gix y

instance (GIx a, GIx b) => GIx (a :*: b) where
    gix _ = 0

instance GIx f => GIx (D1 d f) where
    gix (D1 x) = gix x

instance GIx f => GIx (C1 c f) where
    gix (C1 x) = gix x

instance GIx f => GIx (S1 s f) where
    gix (S1 x) = gix x

conIndex :: (Generic a, GIx (Rep a)) => a -> Int
conIndex x = gix (from x)

-- The generic encoding, as a string.
gEnc :: (Generic a, GEncode (Rep a)) => a -> String
gEnc x = encodeJSON (genericToJSON x)

-- Generic and native agree byte-for-byte.
agrees :: (Generic a, GEncode (Rep a), ToJSON a) => a -> Bool
agrees x = gEnc x == encodeToJSON x

mustParse :: String -> Json
mustParse s = case parseJSON s of
    Left _ -> JNull
    Right v -> v

main :: IO ()
main = do
    -- exact strings: the shape-defining cases
    assert (gEnc (Person "Ann" 30) == "{\"name\":\"Ann\",\"age\":30}") "record object"
    assert (gEnc (Circle 2.5) == "{\"tag\":\"Circle\",\"radius\":2.5}") "tagged record con"
    assert (gEnc (Rect 3 4) == "{\"tag\":\"Rect\",\"contents\":[3,4]}") "tagged positional contents"
    assert (gEnc Point0 == "\"Point0\"") "tagged nullary bare string"
    assert (gEnc RedC == "\"RedC\"") "all-nullary bare string"
    assert (gEnc (Wrap 7) == "7") "single positional is the value"
    assert (gEnc (Pair2 7 "a") == "[7,\"a\"]") "two positional as array"
    assert (gEnc (OptW Nothing) == "null") "positional Maybe Nothing"
    assert (gEnc Unit1 == "\"Unit1\"") "lone nullary bare string"
    assert (gEnc (Acct "zoe" 9 (Just "hi")) == "{\"name\":\"zoe\",\"acctScore\":9,\"note\":\"hi\"}") "renamed field keys"
    assert (gEnc (Tick 12) == "{\"tag\":\"Tick\",\"at\":12}") "renamed field in tagged sum"
    assert (gEnc (Extra 1) == "{\"ex\":1}") "17th type record object"

    -- generic == native across the zoo
    assert (agrees (Person "Ann" 30)) "agree record"
    assert (agrees (Person "a\"b\nc" 1)) "agree escaped string"
    assert (agrees (Circle 2.5)) "agree sum record con"
    assert (agrees (Rect 3 4)) "agree sum positional con"
    assert (agrees Point0) "agree sum nullary con"
    assert (agrees RedC) "agree enum first"
    assert (agrees BlueC) "agree enum last"
    assert (agrees (Wrap 7)) "agree single positional"
    assert (agrees (Pair2 7 "a")) "agree two positional"
    assert (agrees (OptW (Just 5))) "agree Maybe just"
    assert (agrees (OptW Nothing)) "agree Maybe nothing"
    assert (agrees Unit1) "agree lone nullary"
    assert (agrees (Team (Person "A" 1) [Person "B" 2, Person "C" 3] (Just "go"))) "agree nested"
    assert (agrees (Team (Person "A" 1) [] Nothing)) "agree nested empty/Nothing"
    assert (agrees (JNode [JLeaf 1, JNode [JLeaf 2]])) "agree recursive tree"
    assert (agrees (LCons 1 (LCons 2 LNil))) "agree recursive list"
    assert (agrees LNil) "agree recursive list nil"
    assert (agrees (Fwd (Just (Back 9 (Just (Fwd Nothing)))))) "agree mutual recursion"
    assert (agrees (Back 9 Nothing)) "agree mutual recursion other side"
    assert (agrees (Doc "t" (mustParse "{\"any\":[1,true,null]}"))) "agree Json passthrough"
    assert (agrees (Grid [[1, 2], [], [3]] [Just 1, Nothing, Just 3])) "agree nested lists"
    assert (agrees (Acct "zoe" 9 Nothing)) "agree renamed Maybe Nothing"
    assert (agrees (Tick 12)) "agree renamed in sum"
    assert (agrees Stop) "agree nullary next to renamed"
    assert (agrees (Extra 1)) "agree 17th type"

    -- the user generic function across the same zoo
    assert (conIndex (Person "x" 1) == 0) "conIndex Person"
    assert (conIndex (Circle 1.0) == 0) "conIndex Circle"
    assert (conIndex (Rect 1 2) == 1) "conIndex Rect"
    assert (conIndex Point0 == 2) "conIndex Point0"
    assert (conIndex RedC == 0) "conIndex RedC"
    assert (conIndex GreenC == 1) "conIndex GreenC"
    assert (conIndex BlueC == 2) "conIndex BlueC"
    assert (conIndex (Wrap 7) == 0) "conIndex Wrap"
    assert (conIndex (Pair2 1 "a") == 0) "conIndex Pair2"
    assert (conIndex (OptW Nothing) == 0) "conIndex OptW"
    assert (conIndex Unit1 == 0) "conIndex Unit1"
    assert (conIndex (JLeaf 1) == 0) "conIndex JLeaf"
    assert (conIndex (JNode []) == 1) "conIndex JNode"
    assert (conIndex LNil == 0) "conIndex LNil"
    assert (conIndex (LCons 1 LNil) == 1) "conIndex LCons"
    assert (conIndex (Fwd Nothing) == 0) "conIndex Fwd"
    assert (conIndex (Back 1 Nothing) == 0) "conIndex Back"
    assert (conIndex (Doc "t" JNull) == 0) "conIndex Doc"
    assert (conIndex (Grid [] []) == 0) "conIndex Grid"
    assert (conIndex (Acct "a" 1 Nothing) == 0) "conIndex Acct"
    assert (conIndex (Tick 1) == 0) "conIndex Tick"
    assert (conIndex Stop == 1) "conIndex Stop"
    assert (conIndex (Extra 1) == 0) "conIndex Extra"

    putStrLn "generic_json_many ok"
