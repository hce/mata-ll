-- GHC cgrun012: Take and drop on infinite lists
-- Tests lazy evaluation with infinite lists

nats :: Integer -> [Integer]
nats n = n : nats (n + 1)

main :: IO ()
main = do
    assert (take 5 (nats 1) == [1, 2, 3, 4, 5]) "take 5 nats"
    assert (take 0 (nats 1) == ([] :: [Integer])) "take 0"
    assert (head (nats 42) == 42) "head nats"
    assert (head (tail (nats 10)) == 11) "head tail nats"
    assert (length (take 100 (nats 0)) == 100) "length take 100"
    putStrLn "ok"
