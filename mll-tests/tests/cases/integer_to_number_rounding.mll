-- `fromInteger` at Number is GHC's integerToDouble: the nearest double to
-- the exact Integer at every magnitude, ties to even, Infinity past the
-- double's range. The runtime's Horner walk over 2^24 limbs rounded twice
-- from four limbs (2^72) up: 3^80 came out an ulp low and 2^80 + 2^27 + 1
-- lost the bit that breaks its tie. Pins the review's lines, every
-- limb-count boundary, ties and near-ties at each width up to 38 limbs,
-- the overflow edge, negatives, the exact-Horner fast path below 2^72,
-- fromIntegral, the dictionary-routed conversion through an existential,
-- and both literal-pattern routes (a Number signature, and an
-- existential's captured dictionaries) against a literal wider than a
-- machine word.
module Main where

data Box = forall a. (Eq a, Num a, Show a) => Box a

p2 :: Int -> Integer
p2 e = 2 ^ e

toNum :: Integer -> Number
toNum = fromInteger

scaled :: Box -> Integer -> String
scaled (Box x) n = show (x * fromInteger n)

tieUp :: Number -> Bool
tieUp 1208925819614629308923905 = True
tieUp _ = False

tieUpBoxed :: Box -> Bool
tieUpBoxed (Box x) = go x
  where
    go 1208925819614629308923905 = True
    go _ = False

ties :: Int -> Int -> [Integer]
ties e h = [p2 e + p2 h, p2 e + 3 * p2 h, p2 e + p2 h + 1, p2 e + p2 h - 1, negate (p2 e + p2 h + 1), negate (p2 e + 3 * p2 h)]

main :: IO ()
main = do
    print (toNum (3 ^ (80 :: Int)))
    print (toNum (p2 95 + p2 42 + 1))
    print (toNum (negate (3 ^ (80 :: Int))))
    print (toNum (p2 100))
    print (toNum (p2 53 + 1))
    print (toNum (p2 54 + 3))
    print (toNum (p2 80 + p2 27))
    print (toNum (p2 80 + p2 27 + 1))
    print (toNum (10 ^ (308 :: Int)))
    print (toNum (10 ^ (400 :: Int)))
    print (fromIntegral (7 ^ (60 :: Int) :: Integer) :: Number)
    print (toNum (p2 1024 - p2 970))
    print (toNum (p2 1024 - p2 971))
    mapM_ print (map toNum [p2 72 - 1, p2 72, p2 72 + 1, p2 96 - 1, p2 96, p2 96 + 1, p2 120 - 1, p2 120, p2 120 + 1])
    mapM_ print (map toNum (ties 80 27))
    mapM_ print (map toNum (ties 100 47))
    mapM_ print (map toNum (ties 130 77))
    mapM_ print (map toNum (ties 500 447))
    mapM_ print (map toNum (ties 900 847))
    mapM_ print (map toNum [p2 1024 - p2 970 - 1, p2 1024 - p2 970 + 1, p2 1024, negate (p2 1024), p2 1023, p2 1023 + p2 970, p2 1023 + p2 970 + 1, p2 1100 + 1])
    mapM_ print (map toNum [p2 53 + 3, p2 63 - 1, negate (p2 63), p2 63, p2 64 + 1, p2 71 + 3, p2 72 - 3])
    mapM_ print (map toNum [3 ^ (100 :: Int), 7 ^ (90 :: Int), 11 ^ (70 :: Int), 13 ^ (50 :: Int), 10 ^ (23 :: Int), 10 ^ (100 :: Int), 10 ^ (200 :: Int), 10 ^ (309 :: Int)])
    mapM_ print (map toNum [negate (3 ^ (100 :: Int)), negate (10 ^ (200 :: Int)), negate (p2 96 + 1), negate (p2 72 + 1)])
    print (fromIntegral (p2 80 + p2 27 + 1) :: Number, fromIntegral (negate (p2 100 + p2 47 + 1)) :: Number)
    putStrLn (scaled (Box (1 :: Number)) (3 ^ (80 :: Int)))
    putStrLn (scaled (Box (1 :: Number)) (p2 80 + p2 27 + 1))
    putStrLn (scaled (Box (1 :: Integer)) (p2 80 + p2 27 + 1))
    print (tieUp (toNum (p2 80 + p2 27 + 1)), tieUp (toNum (p2 80)), tieUp (toNum (p2 80 + p2 28)))
    print (tieUpBoxed (Box (toNum (p2 80 + p2 27 + 1))), tieUpBoxed (Box (toNum (p2 80))), tieUpBoxed (Box (p2 80 + p2 27 + 1)), tieUpBoxed (Box (p2 80 + p2 28)))

-- expect: 1.4780882941434593e38
-- expect: 3.961408125713218e28
-- expect: -1.4780882941434593e38
-- expect: 1.2676506002282294e30
-- expect: 9.007199254740992e15
-- expect: 1.8014398509481988e16
-- expect: 1.2089258196146292e24
-- expect: 1.2089258196146294e24
-- expect: 1.0e308
-- expect: Infinity
-- expect: 5.0802186073962335e50
-- expect: Infinity
-- expect: 1.7976931348623157e308
-- expect: 4.722366482869645e21
-- expect: 4.722366482869645e21
-- expect: 4.722366482869645e21
-- expect: 7.922816251426434e28
-- expect: 7.922816251426434e28
-- expect: 7.922816251426434e28
-- expect: 1.329227995784916e36
-- expect: 1.329227995784916e36
-- expect: 1.329227995784916e36
-- expect: 1.2089258196146292e24
-- expect: 1.2089258196146297e24
-- expect: 1.2089258196146294e24
-- expect: 1.2089258196146292e24
-- expect: -1.2089258196146294e24
-- expect: -1.2089258196146297e24
-- expect: 1.2676506002282294e30
-- expect: 1.26765060022823e30
-- expect: 1.2676506002282297e30
-- expect: 1.2676506002282294e30
-- expect: -1.2676506002282297e30
-- expect: -1.26765060022823e30
-- expect: 1.361129467683754e39
-- expect: 1.3611294676837545e39
-- expect: 1.3611294676837542e39
-- expect: 1.361129467683754e39
-- expect: -1.3611294676837542e39
-- expect: -1.3611294676837545e39
-- expect: 3.273390607896142e150
-- expect: 3.273390607896143e150
-- expect: 3.2733906078961426e150
-- expect: 3.273390607896142e150
-- expect: -3.2733906078961426e150
-- expect: -3.273390607896143e150
-- expect: 8.452712498170644e270
-- expect: 8.452712498170648e270
-- expect: 8.452712498170646e270
-- expect: 8.452712498170644e270
-- expect: -8.452712498170646e270
-- expect: -8.452712498170648e270
-- expect: 1.7976931348623157e308
-- expect: Infinity
-- expect: Infinity
-- expect: -Infinity
-- expect: 8.98846567431158e307
-- expect: 8.98846567431158e307
-- expect: 8.988465674311582e307
-- expect: Infinity
-- expect: 9.007199254740996e15
-- expect: 9.223372036854776e18
-- expect: -9.223372036854776e18
-- expect: 9.223372036854776e18
-- expect: 1.8446744073709552e19
-- expect: 2.3611832414348226e21
-- expect: 4.722366482869645e21
-- expect: 5.153775207320113e47
-- expect: 1.1450477594321044e76
-- expect: 7.897469567994392e72
-- expect: 4.97929222979127e55
-- expect: 9.999999999999999e22
-- expect: 1.0e100
-- expect: 1.0e200
-- expect: Infinity
-- expect: -5.153775207320113e47
-- expect: -1.0e200
-- expect: -7.922816251426434e28
-- expect: -4.722366482869645e21
-- expect: (1.2089258196146294e24,-1.2676506002282297e30)
-- expect: 1.4780882941434593e38
-- expect: 1.2089258196146294e24
-- expect: 1208925819614629308923905
-- expect: (True,False,True)
-- expect: (True,False,True,False)
