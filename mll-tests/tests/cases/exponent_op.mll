-- The (^) exponentiation operator: exact at Integer (exponentiation by
-- squaring over Num `*`), with an Int exponent. Base may be Int or Integer.

main :: IO ()
main = do
  putStrLn (show (2 ^ 10 :: Integer))
  putStrLn (show (2 ^ 100 :: Integer))
  putStrLn (show (2 ^ 0 :: Integer))
  putStrLn (show (7 ^ 1 :: Integer))
  putStrLn (show ((-2) ^ 7 :: Integer))
  putStrLn (show (3 ^ 5 :: Int))
  putStrLn (show (10 ^ 30 + 1))
