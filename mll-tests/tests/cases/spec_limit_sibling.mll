-- Regression: the specialization limit (>16 instantiations switches a
-- function to the dictionary-passing/generic path) used to purge generated
-- specializations BY NAME PREFIX — tripping the limit on `poly` would also
-- delete the specializations of a sibling `poly_snd`, leaving its call sites
-- dangling. Purging is now by exact specialization identity, and call sites
-- already rewritten to a purged specialization are reverted to the original
-- function.

poly :: a -> a
poly x = x

poly_snd :: Show a => a -> String
poly_snd x = show x

main :: IO ()
main = do
    -- Specialize the sibling FIRST so its call site is rewritten before
    -- `poly` trips the limit.
    assert (poly_snd (poly (1 :: Int)) == "1") "sibling before limit"
    -- 17+ distinct instantiations of `poly` trip the limit.
    assert (poly "s" == "s") "poly String"
    assert (poly True == True) "poly Bool"
    assert (poly (1.5 :: Number) == 1.5) "poly Number"
    assert (poly [1 :: Int] == [1]) "poly [Int]"
    assert (poly ["a"] == ["a"]) "poly [String]"
    assert (poly [True] == [True]) "poly [Bool]"
    assert (poly (Just (1 :: Int)) == Just 1) "poly Maybe Int"
    assert (poly (Just "x") == Just "x") "poly Maybe String"
    assert (poly (Just True) == Just True) "poly Maybe Bool"
    assert (poly (1 :: Int, 2 :: Int) == (1, 2)) "poly (I, I)"
    assert (poly ("a", "b") == ("a", "b")) "poly (S, S)"
    assert (poly (True, False) == (True, False)) "poly (B, B)"
    assert (poly (1 :: Int, "a") == (1, "a")) "poly (I, S)"
    assert (poly ("a", 1 :: Int) == ("a", 1)) "poly (S, I)"
    assert (poly (True, 1 :: Int) == (True, 1)) "poly (B, I)"
    assert (poly (1 :: Int, True) == (1, True)) "poly (I, B)"
    assert (poly [[1 :: Int]] == [[1]]) "poly [[Int]]"
    -- The sibling's specialization must have survived the purge, and the
    -- early `poly` call sites must still reach a live function.
    assert (poly_snd (2 :: Int) == "2") "sibling after limit"
    putStrLn "ok"
