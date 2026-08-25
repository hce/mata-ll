-- Big-integer literals inside guard conditions (F1). Guard conditions are
-- emitted through a sub-generator (guard_cond_ast); the literal must intern
-- into the module's ONE shared __mll_biglit pool. Before the fix the sub
-- carried its own empty pool: the guard's __mll_biglit[1] index then read
-- whatever the MAIN pool held at slot 1 (here `threshold`), so every guard
-- compared against 50000000000000000000 instead of its own literal.
module Main where

threshold :: Integer
threshold = 50000000000000000000

classify :: Integer -> String
classify n
  | n > 99999999999999999999 = "huge"
  | otherwise                = "small"

caseGuard :: Integer -> String
caseGuard n = case n of
  m | m == 123456789012345678901234567890 -> "match"
    | otherwise -> "no"

-- The negative-literal spelling interns with its sign (`-2…0` is one pool
-- entry); pre-fix it collided the same way.
negGuard :: Integer -> String
negGuard n
  | n < (-20000000000000000000) = "low"
  | otherwise                   = "high"

main :: IO ()
main = do
  print threshold
  putStrLn (classify 60000000000000000000)
  putStrLn (classify 123456789012345678901)
  putStrLn (caseGuard 123456789012345678901234567890)
  putStrLn (caseGuard 5)
  putStrLn (negGuard (-30000000000000000000))
  putStrLn (negGuard 0)

-- expect: 50000000000000000000
-- expect: small
-- expect: huge
-- expect: match
-- expect: no
-- expect: low
-- expect: high
