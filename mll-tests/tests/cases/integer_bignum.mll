-- Arbitrary-precision Integer (the numeric default) exercised against GHC:
-- literals default to Integer, big (> i64) literals, sign-correct div/mod and
-- quot/rem, ranges, list show, comparisons, and numeric-literal patterns. Int
-- is the explicit machine-word escape hatch.

factorial :: Integer -> Integer
factorial n = if n <= 1 then 1 else n * factorial (n - 1)

powI :: Integer -> Integer -> Integer
powI b e = if e <= 0 then 1 else b * powI b (e - 1)

sumTo :: Integer -> Integer
sumTo n = foldr (+) 0 [1 .. n]

myGcd :: Integer -> Integer -> Integer
myGcd a b = if b == 0 then a else myGcd b (a `mod` b)

-- Numeric-literal patterns at Integer, including a big (> i64) literal.
classify :: Integer -> String
classify 0 = "zero"
classify 340282366920938463463374607431768211456 = "2^128"
classify _ = "other"

main :: IO ()
main = do
  putStrLn (show (factorial 40))
  putStrLn (show (powI 2 200))
  putStrLn (show (sumTo 1000))
  putStrLn (show (negate (factorial 25)))
  putStrLn (show (divMod (factorial 30) 1000000007))
  putStrLn (show (quotRem (negate (factorial 30)) 1000000007))
  putStrLn (show (myGcd (factorial 20) (factorial 15)))
  putStrLn (show [factorial 10, negate (factorial 11), 0])
  putStrLn (show (factorial 30 == factorial 30))
  putStrLn (show (compare (factorial 30) (factorial 29)))
  putStrLn (classify 340282366920938463463374607431768211456)
  putStrLn (classify 0)
  putStrLn (show (123456789012345678901234567890 + 987654321098765432109876543210))
  -- Int stays the explicit machine type.
  putStrLn (show ((5 :: Int) + 3))
