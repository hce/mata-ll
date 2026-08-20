-- Haskell newtype forms: a freely named constructor, the record form with
-- its identity selector, deriving (Show, Eq, Ord) — stock-style, so Show
-- prints the constructor (record syntax included) — and a polymorphic
-- wrapper. All zero-cost: the wrapper is erased at runtime.
-- (The mata-ll shorthand `newtype Age = Int` lives in newtypes.mll, which
-- is excluded from the GHC oracle; everything here is GHC-legal.)

newtype Rad = MkRad Number

newtype Age = Age { unAge :: Int } deriving (Show, Eq, Ord)

newtype Tag = Tag String deriving (Show, Eq)

newtype Boxed a = MkBoxed [a] deriving (Show, Eq)

double :: Rad -> Number
double (MkRad r) = r + r

main :: IO ()
main = do
    print (double (MkRad 1.5))
    print (unAge (Age 42))
    print (Age 7)
    assert (Age 3 == Age 3) "newtype eq through the wrapper"
    assert (Age 3 < Age 4) "newtype ord through the wrapper"
    assert (compare (Age 5) (Age 4) == GT) "newtype compare"
    print (Tag "hi")
    assert (Tag "a" /= Tag "b") "newtype neq"
    print (MkBoxed [1, 2])
    assert (MkBoxed [1] == MkBoxed [1]) "polymorphic newtype eq"
    -- The selector is the identity; the constructor round-trips.
    assert (unAge (Age (unAge (Age 9))) == 9) "selector round-trip"
    putStrLn "newtype forms ok"
-- expect: 3.0
-- expect: 42
-- expect: Age {unAge = 7}
-- expect: Tag "hi"
-- expect: MkBoxed [1,2]
-- expect: newtype forms ok
