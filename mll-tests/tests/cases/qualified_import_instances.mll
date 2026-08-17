-- Test: a `qualified` import prefixes the module's INSTANCES and CLASS
-- DEFAULTS consistently with its types and values. The resolver used to
-- prefix `data Shape` to `Q.Shape` and `render` to `Q.render` but copy
-- instance and class declarations through untouched, so `instance Show
-- Shape` named a type that no longer existed and its body called an
-- unbound `render`; a class default body had the same unbound sibling.
import qualified QualShapes as Q

-- A class declared HERE whose default body uses a qualified name: the
-- use-site rewrite (`Q.scale` → the prefixed binding) must reach class
-- default bodies, not only function and instance clauses.
class Measured a where
    measure :: a -> Number
    scaledMeasure :: a -> Number
    scaledMeasure x = Q.scale (measure x)

instance Measured Q.Shape where
    measure = Q.area

main :: IO ()
main = do
    let c = Circle 2
    let r = Rect 2 3
    -- Show instance (method body calls the sibling `render`)
    assert (show c == "shape of area 12.0") "Show instance body calls prefixed sibling"
    assert (show r == "shape of area 6.0") "Show instance on the second constructor"
    -- Eq instance on the qualified type
    assert (c == Circle 2) "Eq instance attached to the prefixed type"
    assert (not (r == Q.unit)) "Eq instance distinguishes values"
    -- class default method (body calls the sibling `label` and `render`);
    -- class names and methods stay global, like constructors
    assert (describe r == "described: shape of area 6.0") "class default calls prefixed sibling"
    assert (label Q.unit == "shape of area 1.0") "instance method of the imported class"
    -- qualified value + type in a signature
    assert (Q.area r == 6) "qualified value"
    -- class default body using a qualified sibling
    assert (scaledMeasure r == 60) "use-site rewrite reaches class default bodies"
