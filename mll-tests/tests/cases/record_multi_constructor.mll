-- Record construction for a multi-constructor record type checks the
-- CONSTRUCTOR's own fields: `B { y = 1 }` must not demand A's `x` (it did —
-- "Missing field 'x' in constructor 'B'" — because construction consulted
-- the type-wide field table). Field order in the construction is free.

data T = A { x :: Int, z :: String } | B { y :: Int } deriving (Show, Eq)
data P = P { px :: Int, py :: Int } deriving Show

main :: IO ()
main = do
    print (B { y = 1 })
    print (A { z = "s", x = 2 })
    print (P { py = 5, px = 4 })
    print (y (B { y = 7 }))
    print (map x [A 1 "a", A { x = 3, z = "c" }])
    print (B { y = 1 } == B { y = 1 }, A 1 "a" == B { y = 1 })
