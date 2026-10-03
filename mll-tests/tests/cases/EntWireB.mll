module EntWireB (Status(..), Item(..), Cfg(..), encB, encItemB, selB, cfgB) where

import JSON
import Data.Generics

data Status = Gone | Active | Paused
    deriving (Show, Eq, ToJSON, FromJSON)

data Item = Item { name :: Int, tags :: [String] }
    deriving (Show, Eq, ToJSON, FromJSON)

data Node = Node { label :: Int }
    deriving (Generic)

data Cfg = Cfg { width :: Int, title :: Int }
    deriving (LuaDict)

encB :: Status -> String
encB s = encodeToJSON s

encItemB :: Item -> String
encItemB i = encodeToJSON i

class Sel f where
    selOf :: f -> String

instance Sel f => Sel (D1 d f) where
    selOf (D1 x) = selOf x

instance Sel f => Sel (C1 c f) where
    selOf (C1 x) = selOf x

instance Selector s => Sel (S1 s f) where
    selOf s1 = selName s1

selB :: Int -> String
selB n = selOf (from (Node n))

cfgB :: Int -> Cfg
cfgB n = Cfg { width = 1, title = n }
