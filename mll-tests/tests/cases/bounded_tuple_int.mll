-- lua-compat-skip: luajit
-- Bounded for tuples with an Int component (T18): `maxBound :: (Int,
-- Bool)` was "No instance for 'Bounded (Int, Bool)'" while the
-- component-wise `(minBound, maxBound) :: (T, Bool)` worked — there was no
-- tuple instance at all, for any component type. GHC's instances up to the
-- 15-tuple are Prelude instances now, in the specialized world and through
-- an existential's captured dictionary. LuaJIT skip: Int bounds are
-- doubles there.
module Main where

data T = A | B deriving (Eq, Ord, Show, Enum, Bounded)

data Any = forall a. (Bounded a, Show a) => Any a

bounds :: Any -> String
bounds (Any x) = show (minBound `asTypeOf` x, maxBound `asTypeOf` x)

main :: IO ()
main = do
    print ([minBound .. maxBound] :: [T])
    print ((minBound, maxBound) :: (T, Bool))
    print (((minBound, minBound) :: (Bool, T)), (maxBound :: (Int, Bool)))
    print (minBound :: (Int, T, Int))
    putStrLn (bounds (Any (0 :: Int, False)))
    putStrLn (bounds (Any ((1 :: Int, A), B)))

-- expect: [A,B]
-- expect: (A,True)
-- expect: ((False,A),(9223372036854775807,True))
-- expect: (-9223372036854775808,A,-9223372036854775808)
-- expect: ((-9223372036854775808,False),(9223372036854775807,True))
-- expect: (((-9223372036854775808,A),A),((9223372036854775807,B),B))
