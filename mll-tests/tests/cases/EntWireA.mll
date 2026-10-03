module EntWireA (Status(..), Item(..), Cfg(..), encA, decA, metaA, encItemA, cfgA) where

import JSON
import Data.Generics

data Status = Active | Gone
    deriving (Show, Eq, ToJSON, FromJSON)

data Item = Item { name :: String, count :: Int }
    deriving (Show, Eq, ToJSON, FromJSON)

data Node = Node { label :: String } | Leaf
    deriving (Generic)

data Cfg = Cfg { title :: String, width :: Int }
    deriving (LuaDict)

encA :: Status -> String
encA s = encodeToJSON s

decA :: String -> Either String Item
decA s = decodeJSON s

encItemA :: Item -> String
encItemA i = encodeToJSON i

class Meta f where
    metaOf :: f -> String

instance (Meta a, Meta b) => Meta (a :+: b) where
    metaOf (L1 x) = metaOf x
    metaOf (R1 y) = metaOf y

instance Constructor c => Meta (C1 c f) where
    metaOf c1 = conName c1

instance (Datatype d, Meta f) => Meta (D1 d f) where
    metaOf d = case d of
        D1 x -> datatypeName d <> "." <> metaOf x

metaA :: Bool -> String
metaA b = metaOf (from (if b then Node "x" else Leaf))

cfgA :: Int -> Cfg
cfgA w = Cfg { title = "a", width = w }
