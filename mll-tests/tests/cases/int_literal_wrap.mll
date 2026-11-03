-- lua-compat-skip: luajit
-- An integer literal beyond the Int range at type Int is GHC's
-- `fromInteger n :: Int` — the value modulo 2^64 (`9223372036854775808`
-- is minBound, `18446744073709551617` is 1, `-9223372036854775809` is
-- maxBound). Such a literal was emitted as an Integer bignum, so an
-- Int-typed value was a table at runtime and `show`/`negate`/`+` on it
-- crashed. Every line is GHC's: the literals themselves, negation and
-- arithmetic over them (constant-folded, so the folder's Int arithmetic
-- wraps like the runtime's), equality with minBound, the literal
-- patterns at the same numerals (one shared wrap, so an expression and a
-- pattern spelling one numeral meet), a `Num a => a` literal specialized
-- at Int and reached through an existential's dictionary (narrowed at
-- runtime by fromInteger_Int), and `fromIntegral` of the same Integer as
-- the control. LuaJIT skip: its Int is a double, so the values past 2^53
-- are the nearest doubles there (the documented window);
-- int_literal_wrap_window runs the in-window lines on every host.
module Main where

data Boxed = forall a. (Num a, Eq a, Show a) => Boxed a

bigLit :: Num a => a
bigLit = 9223372036854775808

negBigLit :: Num a => a
negBigLit = -9223372036854775809

overLit :: Num a => a
overLit = 18446744073709551617

viaDict :: Boxed -> String
viaDict (Boxed x) = show (bigLit `asTypeOf` x) <> " " <> show (negBigLit `asTypeOf` x) <> " " <> show (overLit `asTypeOf` x) <> " " <> show (x == bigLit)

classify :: Int -> String
classify 9223372036854775808 = "minBound"
classify (-9223372036854775809) = "maxBound"
classify 18446744073709551617 = "one"
classify _ = "other"

isMin :: Int -> Bool
isMin (-9223372036854775808) = True
isMin _ = False

main :: IO ()
main = do
    print ((-9223372036854775808) :: Int)
    print (9223372036854775808 :: Int)
    print (18446744073709551617 :: Int)
    print ((-9223372036854775809) :: Int)
    print (negate 9223372036854775807 - 1 :: Int)
    print (minBound :: Int)
    print ((-9223372036854775808) + 1 :: Int)
    print (9223372036854775808 == (minBound :: Int))
    print (fromIntegral (9223372036854775808 :: Integer) :: Int)
    print (abs (-9223372036854775808) :: Int)
    print [(-9223372036854775808) :: Int, 9223372036854775807]
    print (negate (-9223372036854775808) :: Int)
    print (9223372036854775807 + 1 :: Int)
    print ((-9223372036854775808) - 1 :: Int)
    print (4294967296 * 4294967296 :: Int)
    print ((-9223372036854775808) * 2 :: Int)
    print (9223372036854775808 `div` 2 :: Int)
    print (36893488147419103232 :: Int)
    print (bigLit :: Int, negBigLit :: Int, overLit :: Int)
    putStrLn (classify minBound)
    putStrLn (classify maxBound)
    putStrLn (classify 1)
    putStrLn (classify 0)
    putStrLn (classify 9223372036854775808)
    putStrLn (classify (-9223372036854775809))
    print (isMin minBound, isMin 9223372036854775808, isMin 0)
    putStrLn (viaDict (Boxed (minBound :: Int)))
    putStrLn (viaDict (Boxed (0 :: Integer)))
    print (fromIntegral (18446744073709551617 :: Integer) :: Int)
    print (fromIntegral (-9223372036854775809 :: Integer) :: Int)
    print (fromInteger 9223372036854775808 :: Int)
    let big = 9223372036854775808 :: Int
    print (big, big == minBound, big - 1, show big == show (minBound :: Int))

-- expect: -9223372036854775808
-- expect: -9223372036854775808
-- expect: 1
-- expect: 9223372036854775807
-- expect: -9223372036854775808
-- expect: -9223372036854775808
-- expect: -9223372036854775807
-- expect: True
-- expect: -9223372036854775808
-- expect: -9223372036854775808
-- expect: [-9223372036854775808,9223372036854775807]
-- expect: -9223372036854775808
-- expect: -9223372036854775808
-- expect: 9223372036854775807
-- expect: 0
-- expect: 0
-- expect: -4611686018427387904
-- expect: 0
-- expect: (-9223372036854775808,9223372036854775807,1)
-- expect: minBound
-- expect: maxBound
-- expect: one
-- expect: other
-- expect: minBound
-- expect: maxBound
-- expect: (True,True,False)
-- expect: -9223372036854775808 9223372036854775807 1 True
-- expect: 9223372036854775808 -9223372036854775809 18446744073709551617 False
-- expect: 1
-- expect: 9223372036854775807
-- expect: -9223372036854775808
-- expect: (-9223372036854775808,True,9223372036854775807,True)
