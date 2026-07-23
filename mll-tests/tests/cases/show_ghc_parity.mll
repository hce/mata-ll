-- GHC-parity `show`: string quoting/escaping, GHC's separators, record
-- syntax, Double formatting (shortest-identifying digits, positional inside
-- [0.1, 10^7), e-notation outside), and negative/special values in nested
-- (showsPrec 11) positions. The golden is derived by running this same
-- program under GHC; every expected string below is GHC's.

data P = P { px :: Int, py :: String } deriving Show
data T = MkT Int Number deriving Show
data N = MkN Number deriving Show

-- 10^n as a Double, expressible without e-notation literals.
pow10 :: Int -> Number
pow10 0 = 1.0
pow10 n = if n > 0 then 10.0 * pow10 (n - 1) else pow10 (n + 1) / 10.0

main :: IO ()
main = do
  -- string show: quotes and escapes
  print "hi"
  print ""
  print "a\nb\tc\rd\\e\"f"
  -- NUL takes its GHC control name; a following digit is a separate
  -- literal so GHC's lexer cannot extend the escape
  print ("nul:" <> "\0" <> "5")
  -- separators: lists "," / tuples "," / records ", "
  print [1, 2, 3]
  print ([] :: [Int])
  print (1, True, "x")
  print [(1, "a"), (2, "b")]
  print [[1, 2], [], [3]]
  print (P { px = 1, py = "s" })
  -- record with a negative field: precedence 0, no inner parens (GHC)
  print (P { px = -1, py = "" })
  -- record in argument position parenthesizes
  print (Just (P { px = 2, py = "q" }))
  -- positional fields at argument precedence
  print (MkT (-1) 2.5)
  print (Just (-1))
  print (Just (Just 3))
  print (Left "x" :: Either String Int)
  print [Just 1, Nothing]
  -- Double formatting
  print (0.0 :: Number)
  print (negate 0.0 :: Number)
  print (1.0 :: Number)
  print (3.0 :: Number)
  print (0.1 :: Number)
  print (0.1 + 0.2 :: Number)
  print (1.0 / 3.0 :: Number)
  print (123.456 :: Number)
  print (-2.5 :: Number)
  print (1234567.0 :: Number)
  print (9999999.5 :: Number)
  print (12345678.0 :: Number)
  print (pow10 7)
  print (pow10 2 / pow10 4)
  print (pow10 20)
  print (602214.076 * pow10 18)
  print (1.0 / pow10 9)
  print (5.0 / pow10 324)
  print (1.0 / 0.0 :: Number)
  print (negate (1.0 / 0.0) :: Number)
  print (0.0 / 0.0 :: Number)
  -- specials nested at argument precedence
  print (MkN (negate 0.0))
  print (MkN (1.0 / 0.0))
  print (MkN (negate (1.0 / 0.0)))
  print (MkN (0.0 / 0.0))
  print (Just (-2.5))
  putStrLn "show_ghc_parity: done"
