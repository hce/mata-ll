-- The in-window lines of int_literal_wrap on EVERY host: an integer
-- literal beyond the Int range at type Int is the value modulo 2^64, and
-- when that value lies within 2^53 a doubles-only host holds it exactly
-- too. `18446744073709551617 :: Int` is 1 (it was an Integer bignum, a
-- table, and `show` of it crashed), 2^64 is 0, -(2^64 + 1) is -1,
-- 2^65 + 3 is 3; folded arithmetic over such literals, equality with the
-- literal minBound (-2^63 is a double, exact everywhere), and the literal
-- patterns at the same numerals.
module Main where

classify :: Int -> String
classify 18446744073709551617 = "one"
classify 36893488147419103235 = "three"
classify (-18446744073709551617) = "minus one"
classify _ = "other"

isMin :: Int -> Bool
isMin (-9223372036854775808) = True
isMin _ = False

main :: IO ()
main = do
    print (18446744073709551617 :: Int)
    print (18446744073709551616 :: Int)
    print ((-18446744073709551617) :: Int)
    print (36893488147419103235 :: Int)
    print (18446744073709551617 + 1 :: Int)
    print (18446744073709551617 * 3 :: Int)
    print (negate 18446744073709551617 :: Int)
    print (18446744073709551617 == (1 :: Int))
    print (9223372036854775808 == (minBound :: Int))
    print ((-9223372036854775808) == (minBound :: Int))
    putStrLn (classify 1)
    putStrLn (classify 3)
    putStrLn (classify (-1))
    putStrLn (classify 18446744073709551617)
    putStrLn (classify 2)
    print (isMin minBound, isMin 9223372036854775808, isMin 0)
    print [18446744073709551617 :: Int, 18446744073709551618, 18446744073709551619]
    let one = 18446744073709551617 :: Int
    print (one, one + one, show one == "1")

-- expect: 1
-- expect: 0
-- expect: -1
-- expect: 3
-- expect: 2
-- expect: 3
-- expect: -1
-- expect: True
-- expect: True
-- expect: True
-- expect: one
-- expect: three
-- expect: minus one
-- expect: one
-- expect: other
-- expect: (True,True,False)
-- expect: [1,2,3]
-- expect: (1,2,True)
