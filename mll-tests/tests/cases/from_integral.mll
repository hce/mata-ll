-- fromIntegral: GHC's (Integral a, Num b) => a -> b, defined as
-- fromInteger . toInteger. Exercises every source/target pair over
-- Int/Integer/Number, the Num-defaulting of an unannotated result
-- (Integer, as GHC), and use inside fractional arithmetic.

n :: Int
n = 42

m :: Integer
m = 5

big :: Integer
big = 12345678901234567890

main :: IO ()
main = do
    print (fromIntegral n :: Integer)
    print (fromIntegral n :: Number)
    print (fromIntegral n :: Int)
    print (fromIntegral (length [1, 2, 3]) :: Number)
    print ((fromIntegral n :: Number) / 4.0)
    print (fromIntegral (-7 :: Int) :: Integer)
    print (fromIntegral m :: Int)
    print (fromIntegral m :: Number)
    print (fromIntegral (7 :: Int))
    print (fromIntegral big :: Number)

-- expect: 42
-- expect: 42.0
-- expect: 42
-- expect: 3.0
-- expect: 10.5
-- expect: -7
-- expect: 5
-- expect: 5.0
-- expect: 7
-- expect: 1.2345678901234567e19
