-- GHC ds005: List comprehension desugaring
-- Tests various list comprehension forms

main :: IO ()
main = do
    -- Basic
    assert ([x * 2 | x <- [1..5]] == [2, 4, 6, 8, 10]) "basic lc"

    -- With guard
    assert ([x | x <- [1..20], x `mod` 3 == 0] == [3, 6, 9, 12, 15, 18]) "guard lc"

    -- Two generators
    assert ([(x, y) | x <- [1, 2], y <- [10, 20]] == [(1, 10), (1, 20), (2, 10), (2, 20)]) "two gen"

    -- Dependent generators
    assert ([(x, y) | x <- [1..3], y <- [x..3]] == [(1,1),(1,2),(1,3),(2,2),(2,3),(3,3)]) "dependent gen"

    -- Multiple guards
    assert ([x | x <- [1..30], x `mod` 2 == 0, x `mod` 3 == 0] == [6, 12, 18, 24, 30]) "multi guard"

    -- Nested list comprehension (flatten)
    let matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
    assert ([x | row <- matrix, x <- row, x `mod` 2 == 1] == [1, 3, 5, 7, 9]) "nested lc"

    putStrLn "ok"
