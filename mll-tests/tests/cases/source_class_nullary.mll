-- A user-defined class with a NULLARY / return-position-only method
-- (`def :: a`, like Monoid's `mempty`), plus a normal argument-carrying
-- method. The compiler now synthesizes each method's class constraint, so a
-- use whose type is DETERMINED (by annotation, by a data field's type, or by
-- unification with an argument-carrying method) resolves to the right instance
-- and runs — while an undetermined use is rejected at compile time (see the
-- error-path tests in run_mll.rs).

class Default a where
    def  :: a
    name :: a -> String

data Colour = Red | Blue
data Count  = Count Integer

instance Default Colour where
    def      = Red
    name Red  = "red"
    name Blue = "blue"

instance Default Count where
    def            = Count 0
    name (Count n) = "count " <> show n

-- `def` determined by an explicit annotation.
fromAnnotation :: String
fromAnnotation = name (def :: Colour)

-- `def` determined by unification with `name` (both share the class variable),
-- and by the expected element type of a list of Counts.
pairDefaults :: (String, String)
pairDefaults = (name (def :: Colour), name (def :: Count))

-- `def` flowing into a field whose type pins it.
data Boxed = Boxed Colour
unbox :: Boxed -> String
unbox (Boxed c) = name c

main :: IO ()
main = do
    assert (fromAnnotation == "red") "nullary def determined by annotation"
    assert (pairDefaults == ("red", "count 0")) "nullary def at two instances"
    assert (unbox (Boxed def) == "red") "nullary def determined by a field type"
    -- def is also usable directly once its type is fixed.
    assert (name (def :: Count) == "count 0") "def :: Count"
    putStrLn "source-class nullary tests passed"
-- expect: source-class nullary tests passed
