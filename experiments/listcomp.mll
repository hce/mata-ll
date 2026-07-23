-- List comprehensions over a lazy stream.
--
-- fibs is an infinite list, so the comprehension must be bounded with `take`
-- before filtering: filtering an infinite list never terminates (the filter
-- keeps pulling elements forever). `take 12` is enough to cover every
-- Fibonacci number below 42.

fibs :: [Int]
fibs = 1 : 1 : zipWith (+) fibs (tail fibs)

-- doubles of the Fibonacci numbers below 42
result :: [Int]
result = [x * 2 | x <- take 12 fibs, x < 42]

eqInts :: [Int] -> [Int] -> Bool
eqInts []     []     = True
eqInts (x:xs) (y:ys) = x == y && eqInts xs ys
eqInts _      _      = False

main :: IO ()
main = do
  print result
  assert (eqInts result [2, 2, 4, 6, 10, 16, 26, 42, 68]) "listcomp: doubled fibs below 42"
