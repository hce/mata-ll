-- Test: `bsFoldl` and `bsZipWith` accept a step function whose result is
-- Lua nil — a `Nothing`, a `()`, an empty list — as an ordinary value.
-- The runtime treated a nil step result as "the callback must have been
-- curried" and re-invoked it with one argument, then called the result:
-- compiled functions are N-ary, so the retry called a two-parameter
-- function with one argument and then tried to call whatever came back
-- ("attempt to call a table value").

lastNonZero :: Maybe Int -> Int -> Maybe Int
lastNonZero _ 0 = Nothing
lastNonZero _ b = Just b

main :: IO ()
main = do
    let bs = bsPack [5, 0, 7]
    -- accumulator becomes Nothing mid-fold, then Just again
    assert (bsFoldl lastNonZero (Just 1) bs == Just 7) "Maybe accumulator through Nothing"
    -- accumulator ends as Nothing (the nil result is the FINAL value)
    assert (bsFoldl lastNonZero (Just 1) (bsPack [5, 0]) == Nothing) "Maybe accumulator ending Nothing"
    -- unit accumulator: every step result is nil
    assert (bsFoldl (\_ _ -> ()) () bs == ()) "unit accumulator"
    -- list accumulator that empties
    assert (bsFoldl (\acc b -> if b == 0 then [] else b : acc) [1] bs == [7]) "list accumulator through []"
    -- an ordinary Int fold still works
    assert (bsFoldl (\acc b -> acc + b) 0 bs == 12) "Int accumulator"
    -- zipWith with a plain step
    assert (bsUnpack (bsZipWith (\a b -> a + b) (bsPack [1, 2, 3]) (bsPack [10, 20, 30, 40])) == [11, 22, 33]) "zipWith"
