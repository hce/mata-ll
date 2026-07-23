-- Tests for Data.List module
import Data.List

main :: IO ()
main = do
    -- Basic functions
    assert (null ([] :: [Int])) "null empty"
    assert (not (null [1])) "null nonempty"
    assert (last [1, 2, 3] == 3) "last"
    assert (init [1, 2, 3] == [1, 2]) "init"

    -- Higher-order
    assert (takeWhile (\x -> x < 3) [1, 2, 3, 4] == [1, 2]) "takeWhile"
    assert (dropWhile (\x -> x < 3) [1, 2, 3, 4] == [3, 4]) "dropWhile"
    assert (any (\x -> x > 3) [1, 2, 4] == True) "any true"
    assert (all (\x -> x > 0) [1, 2, 3] == True) "all true"
    assert (find (\x -> x > 2) [1, 2, 3, 4] == Just 3) "find"

    -- Aggregates
    assert (sum [1, 2, 3, 4] == 10) "sum"
    assert (product [1, 2, 3, 4] == 24) "product"

    -- append / concat
    assert (append [1, 2] [3, 4] == [1, 2, 3, 4]) "append"
    assert (concat [[1, 2], [3], [4, 5]] == [1, 2, 3, 4, 5]) "concat"

    -- replicate / iterate
    assert (replicate 3 7 == [7, 7, 7]) "replicate"
    assert (take 5 (iterate (\x -> x * 2) 1) == [1, 2, 4, 8, 16]) "iterate"

    -- scanl
    assert (scanl (\acc x -> acc + x) 0 [1, 2, 3] == [0, 1, 3, 6]) "scanl"

    -- drop
    assert (drop 2 [1, 2, 3, 4] == [3, 4]) "drop"

    -- intersperse
    assert (intersperse 0 [1, 2, 3] == [1, 0, 2, 0, 3]) "intersperse"

    -- partition (check each element)
    let r = partition (\x -> x > 2) [1, 2, 3, 4]
    assert (fst r == [3, 4]) "partition yes"
    assert (snd r == [1, 2]) "partition no"
