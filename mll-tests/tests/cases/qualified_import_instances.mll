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
    putStrLn (show c)
    putStrLn (show r)
    -- Eq instance on the qualified type
    putStrLn (show (c == Circle 2))
    putStrLn (show (r == Q.unit))
    -- class default method (body calls the sibling `label` and `render`);
    -- class names and methods stay global, like constructors
    putStrLn (describe r)
    putStrLn (label Q.unit)
    -- qualified value + type in a signature
    putStrLn (show (Q.area r))
    -- class default body using a qualified sibling
    putStrLn (show (scaledMeasure r))
-- expect: shape of area 12
-- expect: shape of area 6
-- expect: True
-- expect: False
-- expect: described: shape of area 6
-- expect: shape of area 1
-- expect: 6
-- expect: 60
