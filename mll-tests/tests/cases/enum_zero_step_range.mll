-- A stepped range's direction is GHC.Enum's: upward iff the second element
-- is not below the first (step >= 0), stopping once an element passes the
-- limit; downward only for a negative step, stopping below the limit. So a
-- zero step at Int and Integer (N5) is an infinite list of the start when
-- it does not exceed the limit and `[]` when it does — Bool walks the Int
-- helper and inherits the rule; a derived enumeration, Ordering, () and
-- Number have their own helpers and are the controls, as are the nonzero
-- steps and base's edge shapes `[1, 5 .. 3]`, `[5, 1 .. 3]`, `[3, 1 .. 5]`.
module Main where

data C = R | G | B deriving (Show, Eq, Ord, Enum, Bounded)

main :: IO ()
main = do
    print (take 3 [1, 1 .. 5 :: Int], take 3 [5, 5 .. 1 :: Int], take 3 [1, 1 .. 1 :: Int], take 3 [1, 1 .. 0 :: Int])
    print (take 3 [1, 1 .. 5 :: Integer], take 3 [5, 5 .. 1 :: Integer], take 3 [1, 1 .. 1 :: Integer])
    print (take 3 [7, 7 ..] :: [Int], take 3 [7, 7 ..] :: [Integer], take 3 [(-2), (-2) .. (-1) :: Int], take 3 [(-1), (-1) .. (-2) :: Int])
    print ([0, 0 .. (-1) :: Int], [0, 0 .. (-1) :: Integer], take 2 [0, 0 .. 0 :: Int])
    print (take 3 [1, 1 .. 5 :: Int] == [1, 1, 1], [5, 5 .. 1 :: Int] == [])
    print (length (take 100000 [3, 3 .. 3 :: Int]), sum (take 1000 [2, 2 .. 9 :: Integer]))
    print (take 3 [True, True .. False], take 3 [False, False .. True], take 3 [True, True .. True], take 3 [False, False .. False])
    print (take 3 [True, True ..], take 3 [False, False ..], [False, True ..], [True, False ..])
    print (take 3 [EQ, EQ .. GT], take 3 [EQ, EQ .. LT], take 3 [LT, LT ..], take 3 [GT, GT ..])
    print (take 3 [G, G .. B], take 3 [G, G .. R], take 3 [G, G .. G], take 3 [G, G ..])
    print (take 2 [(), () .. ()], take 3 [1.0, 1.0 .. 5.0 :: Number], take 3 [5.0, 5.0 .. 1.0 :: Number])
    print ([1, 5 .. 3 :: Int], [5, 1 .. 3 :: Int], [3, 1 .. 5 :: Int], [1, 3 .. 0 :: Int])
    print ([1, 5 .. 3 :: Integer], [5, 1 .. 3 :: Integer], [3, 1 .. 5 :: Integer])
    print ([10, 7 .. 1 :: Int], [10, 7 .. 11 :: Int], [10, 7 .. 10 :: Int], [10, 7 .. 9 :: Int], [10, 7 .. 7 :: Int], [10, 7 .. 6 :: Int])
    print ([1, 3 .. 1 :: Int], [1, 3 .. 2 :: Int], [1, 3 .. 3 :: Int], [1, 3 .. 4 :: Int], [1, 3 .. 10 :: Integer], [10, 7 .. 1 :: Integer])
    print ([R, B .. G], [B, R .. G], [G, R .. B], [LT, GT .. EQ], [GT, LT .. EQ])

-- expect: ([1,1,1],[],[1,1,1],[])
-- expect: ([1,1,1],[],[1,1,1])
-- expect: ([7,7,7],[7,7,7],[-2,-2,-2],[])
-- expect: ([],[],[0,0])
-- expect: (True,True)
-- expect: (100000,2000)
-- expect: ([],[False,False,False],[True,True,True],[False,False,False])
-- expect: ([True,True,True],[False,False,False],[False,True],[True,False])
-- expect: ([EQ,EQ,EQ],[],[LT,LT,LT],[GT,GT,GT])
-- expect: ([G,G,G],[],[G,G,G],[G,G,G])
-- expect: ([(),()],[1.0,1.0,1.0],[])
-- expect: ([1],[5],[],[])
-- expect: ([1],[5],[])
-- expect: ([10,7,4,1],[],[10],[10],[10,7],[10,7])
-- expect: ([1],[1],[1,3],[1,3],[1,3,5,7,9],[10,7,4,1])
-- expect: ([R],[B],[],[LT],[GT])
