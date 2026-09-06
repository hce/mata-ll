-- Helper module for import_dual_forms.mll (and the dual-import
-- compile-error tests): a data type with a record constructor, instances
-- on it, a class with an instance on a builtin type, an exported and a
-- private function. The main case imports THIS module through an
-- unqualified list AND two aliases, so every declaration here must be
-- merged exactly once whatever the import forms.
module DualForms (Shape(..), Tag(..), Weighty(..), area, describe, scale) where

data Shape = Circle Number | Rect Number Number

data Tag = Tag { tagName :: String, tagWeight :: Int }

area :: Shape -> Number
area (Circle r) = 3 * r * r
area (Rect w h) = w * h

describe :: Shape -> String
describe s = "shape of area " <> show (area s)

instance Show Shape where
    show s = describe s

instance Eq Shape where
    (==) a b = area a == area b

class Weighty a where
    weight :: a -> Int

instance Weighty Tag where
    weight t = tagWeight t

-- An instance on a type this module does NOT define: under the old
-- per-alias copies it was declared once per import form and rejected
-- as a duplicate instance.
instance Weighty Int where
    weight n = n

scale :: Number -> Number
scale x = x * 10

-- Not in the export list: private, reachable through neither form.
-- Its name is the Prelude's too: a bare `filter` must stay the Prelude's.
filter :: Int -> Int
filter n = n + 1

hidden :: Int
hidden = 7
