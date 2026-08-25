-- The other F1 failure shape: the guard holds the module's ONLY big-integer
-- literal (the test inputs are built arithmetically so no other literal
-- reaches the pool). Pre-fix the sub-generator's pool was discarded, the
-- main pool stayed empty, no `local __mll_biglit = {…}` table was emitted at
-- all — and the guard's __mll_biglit[1] indexed a nil global (runtime crash).
module Main where

huge :: Integer -> Bool
huge n
  | n > 99999999999999999999 = True
  | otherwise                = False

main :: IO ()
main = do
  print (huge (10 ^ 20))
  print (huge 17)

-- expect: True
-- expect: False
