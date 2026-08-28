-- `as` renames on a type deriving ONLY Generic: the derived representation
-- reflects the effective external names (selName = field rename, conName =
-- constructor rename), so `deriving (Generic)` alone gives the rename a
-- meaning — the typechecker's pass-4a rename check must accept it (it used
-- to demand LuaDict/ToJSON/FromJSON, predating the Generics substrate).
-- The payoff pattern is a user-written generic codec keyed on the renamed
-- selNames: a record whose fields carry direction-tagging newtype wrappers
-- is flattened generically into a wire-format parameter list.
import JSON
import Data.Generics

-- Field renames under Generic only (no JSON/LuaDict deriving).
data Req = Req
    { reqDn     as "dn"     :: Exporting String
    , reqKeylen as "keylen" :: Exporting Int
    , reqRows   as "rows"   :: Table Int
    } deriving (Generic)

-- Constructor rename under Generic only.
data Msg = MkMsg Int as "msg"
    deriving (Generic)

-- A miniature typed-RFC layer: the wrapper type decides the parameter
-- kind, the selName (= the `as` rename) decides the wire name.
newtype Exporting a = Exporting a
newtype Table a = Table [a]

data Param = Param { pName as "name" :: String, pKind as "kind" :: Int, pValue as "value" :: Json }
    deriving (Eq, ToJSON)

class ToParam a where
    toParam :: String -> a -> Param

instance ToJSON a => ToParam (Exporting a) where
    toParam n (Exporting x) = Param n 10 (toJSON x)

instance ToJSON a => ToParam (Table a) where
    toParam n (Table xs) = Param n 30 (toJSON xs)

class GParams f where
    gParams :: f -> [Param]

instance GParams f => GParams (D1 d f) where
    gParams (D1 y) = gParams y

instance GParams f => GParams (C1 c f) where
    gParams (C1 y) = gParams y

instance (GParams a, GParams b) => GParams (a :*: b) where
    gParams (Prod a b) = gParams a ++ gParams b

instance (Selector s, GParamLeaf f) => GParams (S1 s f) where
    gParams s1 = case s1 of
        S1 y -> gParamLeaf (selName s1) y : []

class GParamLeaf f where
    gParamLeaf :: String -> f -> Param

instance ToParam c => GParamLeaf (K1 c) where
    gParamLeaf n (K1 v) = toParam n v

genericParams :: (Generic a, GParams (Rep a)) => a -> [Param]
genericParams x = gParams (from x)

-- Request -> result linking via a closed type family: the request value
-- pins `a`, the family picks the result type, its derived FromJSON (with
-- its own `as` renames) decodes the reply — no call-site annotation.
data ReqResult = ReqResult { rrBlob as "pseblob" :: String }
    deriving (Eq, FromJSON)

type family ResultOf a where
    ResultOf Req = ReqResult

decodeResult :: FromJSON (ResultOf a) => a -> Json -> Either String (ResultOf a)
decodeResult _ j = fromJSON j

msgConName :: Msg -> String
msgConName m = case from m of
    d1 -> case d1 of
        D1 c1 -> conName c1

main :: IO ()
main = do
    let ps = genericParams (Req (Exporting "CN=x") (Exporting 2048) (Table (1 : 2 : [])))
    -- selName reflects the `as` rename, the wrapper picks the kind.
    assert (map pName ps == ("dn" : "keylen" : "rows" : [])) "renamed selNames"
    assert (map pKind ps == (10 : 10 : 30 : [])) "wrapper-derived kinds"
    assert (encodeToJSON ps == "[{\"name\":\"dn\",\"kind\":10,\"value\":\"CN=x\"},{\"name\":\"keylen\",\"kind\":10,\"value\":2048},{\"name\":\"rows\",\"kind\":30,\"value\":[1,2]}]")
        "wire format"
    -- conName reflects the constructor rename.
    assert (msgConName (MkMsg 1) == "msg") "renamed conName"
    -- The family-linked result decodes by its renamed key.
    let reply = JObj (("pseblob", JStr "3082") : [])
    assert (decodeResult (Req (Exporting "x") (Exporting 1) (Table [])) reply
                == Right (ReqResult "3082")) "family-linked result decode"
    putStrLn "ok generic-as-rename"
