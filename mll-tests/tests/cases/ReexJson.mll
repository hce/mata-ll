-- Helper module: an export-list module deriving JSON codecs. The list
-- gates the JSON module's names off from importers, but the derived
-- encoder/decoder belong to THIS module's scope (where JSON is
-- imported), so they must still compile and run — the derive references
-- the combinators by their canonical spelling when the root cannot see
-- them bare (see json_fn in typechecker/derive.rs).
module ReexJson (Payload(..), Shape(..), roundTrip, shapeTrip) where

import JSON

data Payload = Payload
        { pName  :: String
        , pCount :: Int
        , pTags  :: [String] }
    deriving (Show, Eq, ToJSON, FromJSON)

-- Two constructors: exercises the tagged-object codec path (tag
-- dispatch, jBadTag/jTagNeedsObject plumbing).
data Shape = Circle Int | Point
    deriving (Show, Eq, ToJSON, FromJSON)

roundTrip :: Payload -> Either String Payload
roundTrip p = decodeJSON (encodeToJSON p)

shapeTrip :: Shape -> Either String Shape
shapeTrip s = decodeJSON (encodeToJSON s)
