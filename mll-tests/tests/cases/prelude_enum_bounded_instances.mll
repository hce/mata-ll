-- GHC's Prelude instances that were missing (T18): Enum () and Bounded ()
-- (fromEnum () = 0, [() ..] = [()], the stepped forms the infinite list of
-- units, minBound = maxBound = ()), Enum Number as GHC's Enum Double
-- (succ/pred by one, toEnum converts, fromEnum truncates, ranges by base's
-- numericEnumFrom family: the k-th element is n + k * step, the limit is
-- the bound plus half a step), and Bounded for tuples. The unannotated
-- `[1.5 .. 3]` defaults to Number as GHC defaults to Double (Q28). The
-- dictionary world sees the same instances: an existential's captured
-- Bounded dictionary at (), at tuples and at a derived enumeration.
module Main where

data Day = Mon | Tue | Wed deriving (Show, Eq, Ord, Enum, Bounded)

data Any = forall a. (Bounded a, Show a) => Any a

bounds :: Any -> String
bounds (Any x) = show (x, minBound `asTypeOf` x, maxBound `asTypeOf` x)

main :: IO ()
main = do
    print (fromEnum (), toEnum 0 :: (), [() ..], [() .. ()])
    print (take 3 [(), () ..], take 2 [(), () .. ()])
    print ((minBound, maxBound) :: ((), ()))
    print [1.0, 1.5 .. 3.0 :: Number]
    print [1.5 .. 3 :: Number]
    print (succ 1.5 :: Number, pred 2.5 :: Number, toEnum 4 :: Number)
    print (fromEnum (3.7 :: Number), fromEnum (-3.7 :: Number), fromEnum (0.5 :: Number))
    print (take 3 [0.1 :: Number ..])
    print [1.0 .. 5.5 :: Number]
    print [0.1, 0.2 .. 1.0 :: Number]
    print [5.0, 4.5 .. 3.0 :: Number]
    print ([1.0, 0.5 .. 3.0 :: Number], [1.0, 2.0 .. 1.0 :: Number], [2.0, 1.0 .. 1.0 :: Number])
    print ([1.0 .. 0.4 :: Number], [1.0 .. 0.5 :: Number])
    print (take 3 [3.0, 3.0 .. 3.0 :: Number], take 3 [1.0, (1.0 :: Number) ..])
    print (take 4 [0.1, 0.3 ..] :: [Number])
    print [1.5 .. 3]
    print (take 3 [0.5 ..])
    print ([LT ..], [minBound .. maxBound :: Ordering], (minBound, maxBound) :: (Ordering, Ordering))
    print (maxBound :: (Bool, Ordering), minBound :: ((), Bool, Ordering))
    print (minBound :: (Bool, (Ordering, Day)), maxBound :: (Day, Bool, (), Ordering))
    putStrLn (bounds (Any ()))
    putStrLn (bounds (Any (True, GT)))
    putStrLn (bounds (Any Tue))
    putStrLn (bounds (Any ((), (False, Mon))))

-- expect: (0,(),[()],[()])
-- expect: ([(),(),()],[(),()])
-- expect: ((),())
-- expect: [1.0,1.5,2.0,2.5,3.0]
-- expect: [1.5,2.5,3.5]
-- expect: (2.5,1.5,4.0)
-- expect: (3,-3,0)
-- expect: [0.1,1.1,2.1]
-- expect: [1.0,2.0,3.0,4.0,5.0,6.0]
-- expect: [0.1,0.2,0.30000000000000004,0.4,0.5,0.6,0.7000000000000001,0.8,0.9,1.0]
-- expect: [5.0,4.5,4.0,3.5,3.0]
-- expect: ([],[1.0],[2.0,1.0])
-- expect: ([],[1.0])
-- expect: ([3.0,3.0,3.0],[1.0,1.0,1.0])
-- expect: [0.1,0.3,0.5,0.7]
-- expect: [1.5,2.5,3.5]
-- expect: [0.5,1.5,2.5]
-- expect: ([LT,EQ,GT],[LT,EQ,GT],(LT,GT))
-- expect: ((True,GT),((),False,LT))
-- expect: ((False,(LT,Mon)),(Wed,True,(),GT))
-- expect: ((),(),())
-- expect: ((True,GT),(False,LT),(True,GT))
-- expect: (Tue,Mon,Wed)
-- expect: (((),(False,Mon)),((),(False,Mon)),((),(True,Wed)))
