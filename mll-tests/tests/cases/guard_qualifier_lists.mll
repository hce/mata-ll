-- Haskell 2010 §3.13 comma-separated guard qualifiers: the guard
-- succeeds when every qualifier holds, checked left to right (desugared
-- to short-circuit &&, which has the same order and laziness).
-- Regression: a comma in a guard died with a bare "Expected '='".
-- The binding qualifier forms (pattern guards, let) get explanatory
-- rejections, pinned in compile_errors.rs.

classify :: Int -> Int -> String
classify x y
    | x > 0, y > 0, x + y < 100 = "both small positive"
    | x > 0, y > 0 = "both positive"
    | otherwise = "other"

caseSide :: Int -> String
caseSide n = case n of
    v | v > 0, v `mod` 2 == 0 -> "positive even"
      | v > 0 -> "positive odd"
    _ -> "rest"

main :: IO ()
main = do
    putStrLn (classify 3 4)
    putStrLn (classify 60 50)
    putStrLn (classify (-1) 2)
    putStrLn (caseSide 4)
    putStrLn (caseSide 3)
    putStrLn (caseSide 0)
