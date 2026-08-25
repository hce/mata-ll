-- Control.Monad's void/join are Monad-polymorphic (F19: they were silently
-- fixed at IO/Maybe with nothing pointing at the narrowing); guard stays
-- list-only (no Alternative class) — a DOCUMENTED deviation, HASKDIFF
-- "Control.Monad is narrower than GHC's".
module Main where

import Control.Monad (void, join, guard)

main :: IO ()
main = do
  print (join (Just (Just 5)))
  print (join (Nothing :: Maybe (Maybe Int)))
  print (join [[1, 2], [3]])
  print (void (Just 7))
  print (void [1, 2, 3])
  void (putStrLn "effect ran")
  print (guard True :: [()])
  print (length (guard False :: [()]))

-- expect: Just 5
-- expect: Nothing
-- expect: [1,2,3]
-- expect: Just ()
-- expect: [(),(),()]
-- expect: effect ran
-- expect: [()]
-- expect: 0
