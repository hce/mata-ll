-- GHC cgrun015: Guards and where clauses
-- Tests guard expressions with local bindings

classify :: Integer -> String
classify n
    | n < 0     = "negative"
    | n == 0    = "zero"
    | n < 10    = "small"
    | n < 100   = "medium"
    | otherwise = "large"

bmi :: Number -> String
bmi x
    | x < thin    = "underweight"
    | x < normal  = "normal"
    | x < fat     = "overweight"
    | otherwise   = "obese"
  where
    thin   = 18.5
    normal = 25.0
    fat    = 30.0

main :: IO ()
main = do
    assert (classify (-5) == "negative") "negative"
    assert (classify 0 == "zero") "zero"
    assert (classify 5 == "small") "small"
    assert (classify 42 == "medium") "medium"
    assert (classify 999 == "large") "large"
    assert (bmi 15.0 == "underweight") "underweight"
    assert (bmi 22.0 == "normal") "normal bmi"
    assert (bmi 27.0 == "overweight") "overweight"
    assert (bmi 35.0 == "obese") "obese"
    putStrLn "ok"
