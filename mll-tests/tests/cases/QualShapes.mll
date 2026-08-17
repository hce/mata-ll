-- Helper module for qualified_import_instances.mll: a data type with an
-- instance whose method body calls a sibling function, and a class with a
-- default method that calls a sibling function. Under `import qualified
-- … as Q` every sibling reference must be prefixed the same way the
-- type and the functions are.
module QualShapes (Shape(..), Describable(..), area, render, unit, scale) where

data Shape = Circle Number | Rect Number Number

area :: Shape -> Number
area (Circle r) = 3 * r * r
area (Rect w h) = w * h

render :: Shape -> String
render s = "shape of area " <> show (area s)

instance Show Shape where
    show s = render s

instance Eq Shape where
    (==) (Circle a) (Circle b) = a == b
    (==) (Rect a b) (Rect c d) = a == c && b == d
    (==) _ _ = False

class Describable a where
    describe :: a -> String
    describe x = "described: " <> label x
    label :: a -> String

instance Describable Shape where
    label s = render s

unit :: Shape
unit = Rect 1 1

scale :: Number -> Number
scale x = x * 10
