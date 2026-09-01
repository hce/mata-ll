-- Record-to-JSON encoding via the derived ToJSON instance: 20000 records
-- through toJSON and the Json renderer. The twin concatenates the same
-- JSON text directly (the data needs no escaping, so a handwritten
-- encoder is pure string building). Only total length is printed — it is
-- field-order-independent yet still byte-count-exact.
module Main where

import JSON
import LString (strLen)

data Person = Person
    { pName as "name" :: String
    , pAge  as "age"  :: Int
    , pTags as "tags" :: [String]
    } deriving (ToJSON)

mkPerson :: Int -> Person
mkPerson i = Person ("person-" <> show i) i ["t", "x" <> show i]

go :: Int -> Int -> Int
go 0 acc = acc
go i acc = go (i - 1) (acc + strLen (encodeToJSON (mkPerson i)))

main :: IO ()
main = print (go 20000 0)
