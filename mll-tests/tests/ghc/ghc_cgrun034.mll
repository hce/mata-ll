-- GHC cgrun034: Tuple operations
-- Tests tuple construction, pattern matching, fst/snd

swap :: (Integer, Integer) -> (Integer, Integer)
swap (a, b) = (b, a)

addPair :: (Integer, Integer) -> Integer
addPair (a, b) = a + b

both :: (Integer -> Integer) -> (Integer, Integer) -> (Integer, Integer)
both f (a, b) = (f a, f b)

main :: IO ()
main = do
    assert (fst (1, 2) == 1) "fst"
    assert (snd (1, 2) == 2) "snd"
    assert (swap (3, 4) == (4, 3)) "swap"
    assert (addPair (10, 20) == 30) "addPair"
    assert (both (* 2) (3, 4) == (6, 8)) "both"

    -- Tuples in lists
    let pairs = [(1, 10), (2, 20), (3, 30)]
    assert (map fst pairs == [1, 2, 3]) "map fst"
    assert (map snd pairs == [10, 20, 30]) "map snd"
    assert (map addPair pairs == [11, 22, 33]) "map addPair"

    putStrLn "ok"
