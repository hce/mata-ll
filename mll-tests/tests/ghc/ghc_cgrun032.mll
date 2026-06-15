-- GHC cgrun032: Accumulating parameter pattern
-- Tests tail-recursive style with accumulators

-- Digits of a number
digits :: Integer -> [Integer]
digits 0 = [0]
digits n = go n []
  where
    go 0 acc = acc
    go n acc = go (n `div` 10) ((n `mod` 10) : acc)

-- Digital root (repeated digit sum until single digit)
digitalRoot :: Integer -> Integer
digitalRoot n
    | n < 10    = n
    | otherwise = digitalRoot (digitSum n)
  where
    digitSum 0 = 0
    digitSum n = n `mod` 10 + digitSum (n `div` 10)

-- Collatz sequence length
collatz :: Integer -> Integer
collatz n = go n 0
  where
    go 1 steps = steps
    go n steps
        | n `mod` 2 == 0 = go (n `div` 2) (steps + 1)
        | otherwise      = go (3 * n + 1) (steps + 1)

main :: IO ()
main = do
    assert (digits 12345 == [1, 2, 3, 4, 5]) "digits 12345"
    assert (digits 0 == [0]) "digits 0"
    assert (digits 100 == [1, 0, 0]) "digits 100"

    assert (digitalRoot 493 == 7) "droot 493"
    assert (digitalRoot 9 == 9) "droot 9"
    assert (digitalRoot 99 == 9) "droot 99"

    assert (collatz 1 == 0) "collatz 1"
    assert (collatz 2 == 1) "collatz 2"
    assert (collatz 27 == 111) "collatz 27"

    putStrLn "ok"
